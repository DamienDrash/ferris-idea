//! Go programs under Delve (`SPEC-0039` v7 behaviour 11): what a `go run` or `go test` command
//! means as a `dlv dap` launch, and Delve as a server that ends with its session.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

use crate::{Breakpoint, DapClient, DapError, DebugSession, SourceBreakpoint, SpecialBreakpoints};

/// Flags of `go build` that take a value as their next word.
const VALUED_BUILD_FLAGS: [&str; 16] = [
    "-tags",
    "-ldflags",
    "-gcflags",
    "-asmflags",
    "-gccgoflags",
    "-mod",
    "-modfile",
    "-overlay",
    "-pkgdir",
    "-p",
    "-compiler",
    "-buildmode",
    "-installsuffix",
    "-toolexec",
    "-pgo",
    "-covermode",
];

/// Flags of `go build` without a value that `go test` also takes.
const PLAIN_BUILD_FLAGS: [&str; 10] = [
    "-race",
    "-msan",
    "-asan",
    "-cover",
    "-trimpath",
    "-a",
    "-x",
    "-work",
    "-buildvcs",
    "-linkshared",
];

/// Flags that cannot be given to Delve's own build: they move the build or its result.
const REFUSED_FLAGS: [&str; 5] = ["-C", "-o", "-exec", "-c", "-n"];

/// Flags of `go test` without a value that the test binary takes as `-test.<name>`.
const PLAIN_TEST_FLAGS: [&str; 5] = ["-v", "-short", "-failfast", "-benchmem", "-fullpath"];

/// Flags of `go test` with a value that the test binary takes as `-test.<name> <value>`.
const VALUED_TEST_FLAGS: [&str; 18] = [
    "-run",
    "-skip",
    "-bench",
    "-count",
    "-timeout",
    "-parallel",
    "-cpu",
    "-benchtime",
    "-shuffle",
    "-list",
    "-fuzz",
    "-fuzztime",
    "-blockprofile",
    "-cpuprofile",
    "-memprofile",
    "-mutexprofile",
    "-trace",
    "-outputdir",
];

/// A `go run` or `go test` command as Delve needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoCommand {
    /// `go test`: the tests of the package are debugged.
    pub test: bool,
    /// Flags for the build (`-race`, `-tags`, `integration`).
    pub build_flags: Vec<String>,
    /// The package or file as written (`.`, `./cmd/app`, `main.go`, an import path).
    pub target: String,
    /// Arguments of the built program; for a test the test binary's own flags (`-test.run`, …).
    pub args: Vec<String>,
}

fn flag_name(word: &str) -> &str {
    // `--race` is the same flag as `-race`.
    let word = word
        .strip_prefix('-')
        .filter(|w| w.starts_with('-'))
        .unwrap_or(word);
    word.split_once('=').map_or(word, |(name, _)| name)
}

fn refused(name: &str) -> String {
    format!(
        "the go flag `{name}` cannot be used under the debugger: Delve builds the program itself"
    )
}

/// Reads `words`, the words after `go`: `run [build flags] <package or file> [arguments]` or
/// `test [build and test flags] [package] [-args arguments]`. Delve builds one package, so
/// several files or packages, a pattern with `...` and flags that move the build are refused
/// with the reason.
pub fn go_command(words: &[String]) -> Result<GoCommand, String> {
    let (verb, rest) = words
        .split_first()
        .ok_or("an empty go command cannot be debugged")?;
    match verb.as_str() {
        "run" => go_run(rest),
        "test" => go_test(rest),
        other => Err(format!(
            "`go {other}` cannot be debugged; `go run` and `go test` can"
        )),
    }
}

fn go_run(rest: &[String]) -> Result<GoCommand, String> {
    let mut build_flags = Vec::new();
    let mut i = 0;
    while let Some(word) = rest.get(i).filter(|w| w.starts_with('-')) {
        let name = flag_name(word);
        if REFUSED_FLAGS.contains(&name) {
            return Err(refused(name));
        }
        build_flags.push(word.clone());
        i += 1;
        if VALUED_BUILD_FLAGS.contains(&name) && !word.contains('=') {
            build_flags.extend(rest.get(i).cloned());
            i += 1;
        }
    }
    let target = rest
        .get(i)
        .cloned()
        .ok_or("`go run` names no package or file")?;
    let args = &rest[i + 1..];
    if target.ends_with(".go") && args.first().is_some_and(|a| a.ends_with(".go")) {
        return Err(
            "Delve builds one file or one package: debug the package folder (`go run .`) instead of several files"
                .into(),
        );
    }
    Ok(GoCommand {
        test: false,
        build_flags,
        target,
        args: args.to_vec(),
    })
}

fn go_test(rest: &[String]) -> Result<GoCommand, String> {
    let mut build_flags = Vec::new();
    let mut args = Vec::new();
    let mut packages: Vec<&String> = Vec::new();
    let mut verbose = false;
    let mut i = 0;
    while let Some(word) = rest.get(i) {
        i += 1;
        if word == "-args" || word == "--args" {
            args.extend(rest[i..].iter().cloned());
            break;
        }
        if !word.starts_with('-') {
            packages.push(word);
            continue;
        }
        let name = flag_name(word);
        let inline = word.split_once('=').map(|(_, value)| value);
        if REFUSED_FLAGS.contains(&name) {
            return Err(refused(name));
        }
        // `go test -json` reads the verbose output of the test binary; under the debugger the
        // binary runs directly and its verbose output is what the console shows.
        if name == "-json" || name == "-v" {
            verbose |= inline != Some("false");
        } else if PLAIN_TEST_FLAGS.contains(&name) {
            args.push(format!("-test.{}", word.trim_start_matches('-')));
        } else if VALUED_TEST_FLAGS.contains(&name) {
            args.push(format!("-test.{}", &name[1..]));
            match inline {
                Some(value) => args.push(value.to_owned()),
                None => {
                    args.push(
                        rest.get(i)
                            .cloned()
                            .ok_or_else(|| format!("the go test flag `{name}` has no value"))?,
                    );
                    i += 1;
                }
            }
        } else if VALUED_BUILD_FLAGS.contains(&name) {
            build_flags.push(word.clone());
            if inline.is_none() {
                build_flags.extend(rest.get(i).cloned());
                i += 1;
            }
        } else if PLAIN_BUILD_FLAGS.contains(&name) {
            build_flags.push(word.clone());
        } else {
            return Err(format!(
                "the go test flag `{name}` is not known to the debugger; give it after `-args` if it is the test binary's"
            ));
        }
    }
    if verbose {
        args.insert(0, "-test.v".into());
    }
    let target = match packages.as_slice() {
        [] => ".".to_owned(),
        [one] if !one.contains("...") => (*one).clone(),
        _ => {
            return Err(format!(
                "Delve debugs the tests of one package; `{}` names several. Debug a test file or one test instead",
                packages
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
    };
    Ok(GoCommand {
        test: true,
        build_flags,
        target,
        args,
    })
}

/// `path` without `.` parts.
fn tidy(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}

/// The `dlv dap` launch request for the go command `words` (the words after `go`) started in
/// `cwd` (`SPEC-0039` v7 behaviour 11). Delve builds in the package's folder (`dlvCwd`) with the
/// package named relative to it, so the build never depends on how the folder is reached; a
/// program runs in `cwd`, tests run in their package's folder as `go test` runs them. The
/// program's output comes back as events (`outputMode`).
pub fn go_launch(
    words: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Value, String> {
    let command = go_command(words)?;
    let target = Path::new(&command.target);
    let on_disk = target.is_absolute() || command.target.starts_with('.');
    let (folder, program, run_in) = if command.target.ends_with(".go") {
        let file = tidy(&cwd.join(target));
        let folder = file.parent().unwrap_or(cwd).to_path_buf();
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (folder.clone(), format!("./{name}"), folder)
    } else if on_disk {
        let folder = tidy(&cwd.join(target));
        (folder.clone(), ".".to_owned(), folder)
    } else {
        // An import path: the go tool finds it from the module of `cwd`.
        (cwd.to_path_buf(), command.target.clone(), cwd.to_path_buf())
    };
    let mut launch = json!({
        "request": "launch",
        "mode": if command.test { "test" } else { "debug" },
        "program": program,
        "dlvCwd": folder,
        "cwd": if command.test { run_in.as_path() } else { cwd },
        "args": command.args,
        "env": env,
        "stopOnEntry": false,
        "outputMode": "remote",
    });
    if !command.build_flags.is_empty() {
        launch["buildFlags"] = json!(command.build_flags);
    }
    Ok(launch)
}

/// Said in the debug console after attaching to a Delve server (`SPEC-0147` v3 behaviour 7).
pub const SERVER_NOTE: &str = "[Ferris IDEA] Stop disconnects from the Delve server. A server started without --accept-multiclient then ends, together with the program it started.\n";

/// Whether `host` names this machine's loopback.
pub fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Where Delve finds a folder that is reached through symbolic links: the go tool names sources
/// by their resolved path, breakpoints by the path the project was opened with. Returns `(as
/// opened, as resolved)` for every link on the way to `folder`, the longest first; empty when
/// there is none.
pub fn link_mapping(folder: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut rules: Vec<(PathBuf, PathBuf)> = folder
        .ancestors()
        .filter(|a| {
            a.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        })
        .filter_map(|a| Some((a.to_path_buf(), std::fs::canonicalize(a).ok()?)))
        .collect();
    rules.sort_by_key(|(from, _)| std::cmp::Reverse(from.as_os_str().len()));
    rules
}

/// Where the folder of a session's socket goes: the temporary folder, or `/tmp` when a socket
/// path below it would not fit a socket address (104 bytes on macOS; `SPEC-0322` behaviour 2).
#[cfg(unix)]
fn socket_parent(temporary: PathBuf) -> PathBuf {
    /// The longest `/ferris-idea-dlv-<pid>-<nanos>/dap.sock`, and the smaller limit.
    const NAME: usize = 58;
    const LIMIT: usize = 104;
    if temporary.as_os_str().len() + NAME < LIMIT {
        temporary
    } else {
        PathBuf::from("/tmp")
    }
}

/// A new folder of mode 0700 for one session; creating it fails on anything already there.
#[cfg(unix)]
fn private_folder() -> Result<PathBuf, DapError> {
    use std::os::unix::fs::DirBuilderExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
        + u128::from(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let dir = socket_parent(std::env::temp_dir())
        .join(format!("ferris-idea-dlv-{}-{nanos}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| DapError::Start(format!("{}: {e}", dir.display())))?;
    Ok(dir)
}

/// `dlv dap` serving one session (`SPEC-0039` v7 behaviour 11): on a Unix socket in a folder only
/// this user can enter, elsewhere on a loopback port Delve picks itself. It ends with the value,
/// together with the program it started, and never outlives Ferris IDEA (`SPEC-0263` v2 behaviour 3).
pub struct DelveServer {
    child: Child,
    guard: Option<crate::lifetime::ExitGuard>,
    address: String,
    /// The session's private folder: the socket and the built program.
    folder: Option<PathBuf>,
}

impl DelveServer {
    /// Starts `command` (`dlv dap`) and waits until it listens.
    pub fn start(command: &[String]) -> Result<Self, DapError> {
        use std::io::BufRead;
        let (program, args) = command
            .split_first()
            .ok_or_else(|| DapError::Start("empty dlv command".into()))?;
        // Without a Unix socket Delve would listen on a loopback port every local account can
        // reach, so it is refused (`SPEC-0322` v6 behaviour 3).
        if cfg!(not(unix)) {
            return Err(DapError::Start(
                "dlv is not supported on this platform yet: it cannot listen privately".into(),
            ));
        }
        #[cfg(unix)]
        let folder = Some(private_folder()?);
        #[cfg(not(unix))]
        let folder: Option<PathBuf> = None;
        let listen = match &folder {
            Some(dir) => format!("--listen=unix:{}", dir.join("dap.sock").display()),
            None => "--listen=127.0.0.1:0".to_owned(),
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .arg(listen)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Its own process group (SPEC-0263 behaviour 3).
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                if let Some(dir) = &folder {
                    let _ = std::fs::remove_dir_all(dir);
                }
                return Err(DapError::Start(format!("{program}: {e}")));
            }
        };
        let stdout = child.stdout.take();
        let guard = crate::lifetime::ExitGuard::watch(child.id());
        let mut server = Self {
            child,
            guard,
            address: String::new(),
            folder,
        };
        let mut lines = std::io::BufReader::new(stdout.ok_or(DapError::Closed)?).lines();
        // "DAP server listening at: <address>"; dropping `server` ends Delve otherwise.
        let line = match lines.next() {
            Some(Ok(line)) if line.contains("listening") => line,
            other => {
                return Err(DapError::Start(format!(
                    "dlv did not start: {}",
                    other.and_then(Result::ok).unwrap_or_default()
                )));
            }
        };
        server.address = match &server.folder {
            Some(dir) => format!("unix:{}", dir.join("dap.sock").display()),
            None => line
                .split_whitespace()
                .filter_map(|w| w.parse::<std::net::SocketAddr>().ok())
                .find(|a| a.ip().is_loopback())
                .map(|a| a.to_string())
                .ok_or_else(|| DapError::Start(format!("dlv named no loopback port: {line}")))?,
        };
        // Keep reading so Delve never blocks on a full pipe.
        std::thread::spawn(move || lines.for_each(drop));
        Ok(server)
    }

    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// What [`DapClient::connect`] connects to: `unix:<socket>` or `127.0.0.1:<port>`.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Completes a launch request for this server: the program is built into the session's
    /// private folder instead of the package's (removed with the session), and a project reached
    /// through a symbolic link gets Delve's path substitution, so its breakpoints bind.
    pub fn complete(&self, launch: &mut Value) {
        if let Some(dir) = &self.folder {
            launch["output"] = json!(dir.join("debuggee"));
        }
        let rules = launch["dlvCwd"]
            .as_str()
            .map(|folder| link_mapping(Path::new(folder)))
            .unwrap_or_default();
        if !rules.is_empty() && launch["substitutePath"].is_null() {
            launch["substitutePath"] = rules
                .iter()
                .map(|(from, to)| json!({ "from": from, "to": to }))
                .collect();
        }
    }
}

impl Drop for DelveServer {
    fn drop(&mut self) {
        // Ferris IDEA ends it now, so the guard stands down first.
        drop(self.guard.take());
        crate::lifetime::end_tree(&mut self.child, std::time::Duration::from_secs(3));
        if let Some(dir) = &self.folder {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

type Started = (DebugSession, BTreeMap<PathBuf, Vec<Breakpoint>>);

/// Starts Delve (`adapter` is `dlv dap`) and debugs `launch` ([`go_launch`]) with `breakpoints`
/// set before the program runs. Delve and the program end with the session.
pub fn launch_go(
    adapter: &[String],
    mut launch: Value,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    special: &SpecialBreakpoints,
) -> Result<Started, DapError> {
    let server = DelveServer::start(adapter)?;
    server.complete(&mut launch);
    let address = server.address().to_owned();
    let client = DapClient::connect(&address, Some(Box::new(server)))?;
    DebugSession::launch_on(client, "dlv", launch, breakpoints, special)
}

/// Starts Delve and attaches it to a running Go process (`attach` from
/// [`crate::attach_arguments`]); ending the session detaches and ends Delve only.
pub fn attach_go(
    adapter: &[String],
    attach: Value,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
    special: &SpecialBreakpoints,
) -> Result<Started, DapError> {
    let server = DelveServer::start(adapter)?;
    let address = server.address().to_owned();
    let client = DapClient::connect(&address, Some(Box::new(server)))?;
    DebugSession::attach_on(client, "dlv", attach, breakpoints, special)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    /// `go run`: build flags before the target, the program's arguments after it; what Delve's
    /// own build cannot take is refused with the reason (`SPEC-0039` v7 behaviour 11).
    #[test]
    fn go_run_commands() {
        let c = go_command(&words("run main.go -v 3")).unwrap();
        assert_eq!(
            (c.test, c.target.as_str(), c.args.as_slice()),
            (false, "main.go", words("-v 3").as_slice())
        );
        assert!(c.build_flags.is_empty());
        let c = go_command(&words("run -race -tags integration,slow ./cmd/app serve")).unwrap();
        assert_eq!(c.build_flags, words("-race -tags integration,slow"));
        assert_eq!((c.target.as_str(), c.args), ("./cmd/app", words("serve")));
        let c = go_command(&words("run -mod=vendor .")).unwrap();
        assert_eq!(
            (c.build_flags, c.target.as_str()),
            (words("-mod=vendor"), ".")
        );
        assert!(
            go_command(&words("run a.go b.go"))
                .unwrap_err()
                .contains("one file")
        );
        assert!(
            go_command(&words("run -C sub ."))
                .unwrap_err()
                .contains("`-C`")
        );
        assert!(
            go_command(&words("run -race"))
                .unwrap_err()
                .contains("no package")
        );
        assert!(
            go_command(&words("build ./..."))
                .unwrap_err()
                .contains("go build")
        );
        assert!(go_command(&[]).is_err());
    }

    /// `go test`: the test flags become the test binary's, `-json` and `-v` its verbose output,
    /// build flags stay build flags, one package only.
    #[test]
    fn go_test_commands() {
        let c = go_command(&words("test -json -run ^TestAdd$ ./calc")).unwrap();
        assert!(c.test);
        assert_eq!(c.target, "./calc");
        assert_eq!(c.args, words("-test.v -test.run ^TestAdd$"));
        let c = go_command(&words("test -count=1 -short -race -tags=e2e -timeout 30s")).unwrap();
        assert_eq!(c.target, ".");
        assert_eq!(c.args, words("-test.count 1 -test.short -test.timeout 30s"));
        assert_eq!(c.build_flags, words("-race -tags=e2e"));
        let c = go_command(&words("test ./calc -run X -args -fixture data --deep")).unwrap();
        assert_eq!(c.args, words("-test.run X -fixture data --deep"));
        assert!(
            go_command(&words("test -json ./..."))
                .unwrap_err()
                .contains("one package")
        );
        assert!(
            go_command(&words("test ./a ./b"))
                .unwrap_err()
                .contains("./a ./b")
        );
        assert!(
            go_command(&words("test -frobnicate ."))
                .unwrap_err()
                .contains("-frobnicate")
        );
        assert!(
            go_command(&words("test -run"))
                .unwrap_err()
                .contains("no value")
        );
        assert!(
            go_command(&words("test -c ."))
                .unwrap_err()
                .contains("`-c`")
        );
    }

    /// The launch request: built in the package's folder with a relative name, a program runs in
    /// the working directory, tests in their package's folder.
    #[test]
    fn go_launch_requests() {
        let env = BTreeMap::from([("MODE".to_owned(), "dev".to_owned())]);
        let cwd = Path::new("/work/shop");
        let run = go_launch(&words("run cmd/app/main.go --port 80"), cwd, &env).unwrap();
        assert_eq!(run["mode"], "debug");
        assert_eq!(run["program"], "./main.go");
        assert_eq!(run["dlvCwd"], "/work/shop/cmd/app");
        assert_eq!(run["cwd"], "/work/shop");
        assert_eq!(run["args"], json!(["--port", "80"]));
        assert_eq!(run["env"], json!({"MODE": "dev"}));
        assert_eq!(run["outputMode"], "remote");
        assert!(run["buildFlags"].is_null());
        assert!(
            run["output"].is_null(),
            "the server names the built program"
        );
        let package = go_launch(&words("run -race ."), cwd, &env).unwrap();
        assert_eq!(package["program"], ".");
        assert_eq!(package["dlvCwd"], "/work/shop");
        assert_eq!(package["buildFlags"], json!(["-race"]));
        let test = go_launch(&words("test -json -run ^TestTotal$ ./cart"), cwd, &env).unwrap();
        assert_eq!(test["mode"], "test");
        assert_eq!(test["program"], ".");
        assert_eq!(test["dlvCwd"], "/work/shop/cart");
        assert_eq!(test["cwd"], "/work/shop/cart");
        assert_eq!(test["args"], json!(["-test.v", "-test.run", "^TestTotal$"]));
        let import = go_launch(&words("run example.com/shop/cmd/app"), cwd, &env).unwrap();
        assert_eq!(import["program"], "example.com/shop/cmd/app");
        assert_eq!(import["dlvCwd"], "/work/shop");
        assert!(go_launch(&words("test ./..."), cwd, &env).is_err());
    }

    #[test]
    fn loopback_hosts() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("localhost"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("10.0.0.5"));
        assert!(!is_loopback("build.example"));
    }

    /// A folder behind a symbolic link maps from the link to what it names; a plain folder needs
    /// no mapping.
    #[cfg(unix)]
    #[test]
    fn symbolic_links_map_by_their_prefix() {
        let base =
            std::env::temp_dir().join(format!("ferris-idea-dlv-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("real/proj/calc")).unwrap();
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
        let real = std::fs::canonicalize(base.join("real")).unwrap();
        assert_eq!(link_mapping(&real.join("proj/calc")), Vec::new());
        let opened = std::fs::canonicalize(&base).unwrap().join("link");
        let rules = link_mapping(&opened.join("proj/calc"));
        assert_eq!(rules, [(opened, real.clone())]);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
