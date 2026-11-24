//! What debugging a run configuration's command means (`SPEC-0091` v2 behaviour 2, `SPEC-0097`
//! behaviour 6, `SPEC-0168`), on the machine this code runs on: the window's for a local project,
//! the host's for a remote one (`SPEC-0322` behaviour 6).

use std::path::{Path, PathBuf};

use ferris_idea_debugger::Kind;

/// The arguments of a package manager command that runs a `package.json` script: `run <script>
/// …` of npm, pnpm, yarn and bun, and npm's `start`/`test` shorthands (as `run start`).
fn package_script(manager: &str, rest: &[String]) -> Option<Vec<String>> {
    let manager = manager.trim_end_matches(".cmd");
    if !["npm", "pnpm", "yarn", "bun"].contains(&manager) {
        return None;
    }
    match rest.first().map(String::as_str) {
        Some("run" | "run-script") if rest.len() > 1 => {
            let mut args = vec!["run".to_owned()];
            args.extend(rest[1..].iter().cloned());
            Some(args)
        }
        Some(short @ ("start" | "test")) if manager == "npm" => {
            let mut args = vec!["run".to_owned(), short.to_owned()];
            args.extend(rest[1..].iter().cloned());
            Some(args)
        }
        _ => None,
    }
}

/// The program to debug for a run configuration's command: its first word, relative to `cwd` when
/// it is a path (`SPEC-0091` behaviour 2). `None` for an empty command.
pub fn program_of(command: &[String], cwd: &Path) -> Option<(PathBuf, Vec<String>)> {
    let (first, rest) = command.split_first()?;
    let path = PathBuf::from(first);
    let program = if path.is_absolute() || !first.contains('/') {
        if path.is_absolute() {
            path
        } else {
            which(first).unwrap_or_else(|| cwd.join(first))
        }
    } else {
        cwd.join(path)
    };
    Some((program, rest.to_vec()))
}

/// What debugging a run configuration's command means: a Python script when the command starts with
/// a `python…` interpreter or a `.py` file, else a native program (behaviour 2). Returns the kind, the
/// interpreter for a script, the program and its arguments.
pub fn plan(command: &[String], cwd: &Path) -> Option<DebugPlan> {
    let first = command.first()?;
    let name = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.starts_with("python") {
        let (interpreter, _) = program_of(&command[..1], cwd)?;
        let (script, args) = program_of(&command[1..], cwd).map(|(s, a)| {
            // A bare script name is a file in the working directory, not a program on the PATH.
            if command[1].contains('/') || Path::new(&command[1]).is_absolute() {
                (s, a)
            } else {
                (cwd.join(&command[1]), a)
            }
        })?;
        return Some((Kind::Python, Some(interpreter), script, args));
    }
    // `node script.js …` (SPEC-0091 v4 behaviour 10), also TypeScript (SPEC-0168 behaviour 3).
    if name == "node" || name == "node.exe" {
        let script = command.get(1).filter(|s| {
            [".js", ".mjs", ".cjs", ".ts", ".mts", ".cts"]
                .iter()
                .any(|e| s.ends_with(e))
        })?;
        let path = Path::new(script);
        let program = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        return Some((Kind::Node, None, program, command[2..].to_vec()));
    }
    // `go run …` and `go test …` run under Delve (SPEC-0039 v7 behaviour 11). The words after
    // `go` are kept: the launch reads them and says what Delve cannot debug (`go build`, several
    // packages) instead of debugging the go tool itself.
    if name == "go" || name == "go.exe" {
        let words = command[1..].to_vec();
        let target = ferris_idea_debugger::go_command(&words)
            .map(|c| c.target)
            .unwrap_or_default();
        return Some((Kind::Go, None, cwd.join(target), words));
    }
    // `npm run dev`, `pnpm run build -- --watch`, `npm start` (SPEC-0168 behaviour 1).
    if let Some(args) = package_script(&name, &command[1..]) {
        let runner = which(&name).unwrap_or_else(|| PathBuf::from(first));
        return Some((Kind::PackageScript, None, runner, args));
    }
    // `npx jest …` of a Jest test configuration or ▶ marker (SPEC-0045 v6 behaviour 12): npx
    // under js-debug like a package script, the tests in Jest's own process (`--runInBand`), where
    // the debugger is attached.
    if is_npx_jest(command) {
        let manager = name.trim_end_matches(".cmd").to_owned();
        let runner = which(&manager).unwrap_or_else(|| PathBuf::from(first));
        let mut args = command[1..].to_vec();
        if !args.iter().any(|a| a == "--runInBand" || a == "-i") {
            args.push("--runInBand".to_owned());
        }
        return Some((Kind::PackageScript, None, runner, args));
    }
    // `sh -c "<build> && kotlin -cp <classes> <MainKt>"` of a Kotlin module (SPEC-0132).
    if name == "sh"
        && command.get(1).map(String::as_str) == Some("-c")
        && let Some(script) = command.get(2)
        && let Some((build, run)) = script.rsplit_once(" && ")
        && let Some(class) = run
            .strip_prefix("kotlin -cp ")
            .and_then(|r| r.split_whitespace().nth(1))
    {
        return Some((
            Kind::Kotlin,
            None,
            PathBuf::from(class),
            vec![build.to_owned()],
        ));
    }
    if let Some(class) = java_main(command, cwd) {
        let args = if name == "java" {
            command.iter().skip(2).cloned().collect()
        } else {
            Vec::new()
        };
        return Some((Kind::Java, None, PathBuf::from(class), args));
    }
    if name.ends_with(".py") {
        let (script, args) = program_of(command, cwd)?;
        let script = if first.contains('/') {
            script
        } else {
            cwd.join(first)
        };
        return Some((Kind::Python, None, script, args));
    }
    // `cargo run [--bin n | --example n] [-- args]`: the built program under lldb-dap, not cargo
    // (`SPEC-0097` v3 behaviour 9); the build is the launch's preparation ([`cargo_run`]).
    if let Some(run) = cargo_run(command, cwd) {
        return Some((Kind::Native, None, run.program, run.args));
    }
    let (program, args) = program_of(command, cwd)?;
    Some((Kind::Native, None, program, args))
}

/// What debugging `cargo run …` means (`SPEC-0097` v3 behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoRun {
    /// The shell command that builds the target first: `cargo build` with the same selection.
    pub build: String,
    /// The program it builds: `<target>/debug/<name>` or `<target>/debug/examples/<name>`.
    pub program: PathBuf,
    /// The program's arguments: the words after `--`.
    pub args: Vec<String>,
}

/// The build and the program of a `cargo run` command run in `cwd`, or `None` for another command
/// or options Ferris IDEA does not follow (`--release`, `--profile`, `--target`, `-p`, `--package`,
/// `--manifest-path`; the plain program would be the wrong one). The target folder is
/// `CARGO_TARGET_DIR` (relative to `cwd`), else `target` in the topmost folder from `cwd` up
/// whose `Cargo.toml` has a `[workspace]` table, else in `cwd`. The name is the `--bin` or
/// `--example`, else the package's `default-run`, else its `name`.
pub fn cargo_run(command: &[String], cwd: &Path) -> Option<CargoRun> {
    let first = Path::new(command.first()?).file_name()?.to_string_lossy();
    if first != "cargo" || command.get(1).map(String::as_str) != Some("run") {
        return None;
    }
    let rest = &command[2..];
    let split = rest.iter().position(|w| w == "--").unwrap_or(rest.len());
    let (options, args) = (&rest[..split], rest.get(split + 1..).unwrap_or_default());
    let mut selection: Option<(&str, String)> = None;
    let mut words = options.iter();
    while let Some(w) = words.next() {
        let (flag, value) = match w.split_once('=') {
            Some((f, v)) => (f, Some(v.to_owned())),
            None => (w.as_str(), None),
        };
        match flag {
            "--bin" | "--example" => {
                let value = value.or_else(|| words.next().cloned())?;
                selection = Some((flag, value));
            }
            "-q" | "--quiet" | "-v" | "--verbose" | "--locked" | "--offline" | "--frozen" => {}
            _ => return None,
        }
    }
    let name = match &selection {
        Some((_, n)) => n.clone(),
        None => package_binary(&std::fs::read_to_string(cwd.join("Cargo.toml")).ok()?)?,
    };
    let target = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(d) => cwd.join(d),
        None => cwd
            .ancestors()
            .filter(|d| {
                std::fs::read_to_string(d.join("Cargo.toml"))
                    .is_ok_and(|t| t.lines().any(|l| l.trim() == "[workspace]"))
            })
            .last()
            .unwrap_or(cwd)
            .join("target"),
    };
    let mut build = "cargo build".to_owned();
    let mut program = target.join("debug");
    if let Some((flag, n)) = &selection {
        build.push_str(&format!(" {flag} '{}'", n.replace('\'', "'\\''")));
        if *flag == "--example" {
            program.push("examples");
        }
    }
    program.push(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    Some(CargoRun {
        build,
        program,
        args: args.to_vec(),
    })
}

/// The binary `cargo run` runs in a package: its `default-run`, else its `name` (`[package]` of
/// a `Cargo.toml`).
fn package_binary(manifest: &str) -> Option<String> {
    let mut in_package = false;
    let (mut name, mut default_run) = (None, None);
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        match key.trim() {
            "name" => name = Some(value),
            "default-run" => default_run = Some(value),
            _ => {}
        }
    }
    default_run.or(name)
}

/// Whether `command` runs Jest through npx (`npx jest …`), which can be debugged with js-debug
/// (`SPEC-0045` v6 behaviour 12).
pub fn is_npx_jest(command: &[String]) -> bool {
    let Some(first) = command.first() else {
        return false;
    };
    let name = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.trim_end_matches(".cmd") == "npx" && command.get(1).is_some_and(|w| w == "jest")
}

/// What [`plan`] returns: the kind, the interpreter for a script, the program and its arguments.
pub type DebugPlan = (Kind, Option<PathBuf>, PathBuf, Vec<String>);

/// [`plan`] for run configuration `config` of workspace `root`, from the command it starts:
/// variables replaced and a bare `python`/`python3` replaced by the workspace's selected
/// interpreter (`SPEC-0050` v2 behaviour 8). Err: why it cannot start; `Ok(None)`: it cannot be
/// debugged.
pub fn plan_run_config(
    root: &Path,
    config: &ferris_idea_core::RunConfig,
    cwd: &Path,
) -> Result<Option<DebugPlan>, String> {
    // Requests of a request file are sent, not debugged (`SPEC-0047` v6 behaviour 17).
    if config.http.is_some() {
        return Ok(None);
    }
    let command = ferris_idea_core::launch_command(root, config)?;
    Ok(plan(&command, cwd))
}

/// The main class a Java command runs: `java File.java` (package and file name) or a build tool
/// command with `-Dexec.mainClass=…` (`SPEC-0097` behaviour 6).
fn java_main(command: &[String], cwd: &Path) -> Option<String> {
    if let Some(class) = command
        .iter()
        .find_map(|a| a.strip_prefix("-Dexec.mainClass="))
    {
        return Some(class.to_owned());
    }
    let name = Path::new(command.first()?).file_name()?.to_string_lossy();
    // `sh -c "… javac … && java -cp <classes> <Class>"` of a module without a build tool
    // (`SPEC-0097` v4 behaviour 10): jdtls builds and launches the class itself.
    if name == "sh"
        && command.get(1).map(String::as_str) == Some("-c")
        && let Some(script) = command.get(2)
        && let Some((build, run)) = script.rsplit_once(" && ")
        && build.contains("javac ")
        && run.starts_with("java -cp ")
    {
        // The class is quoted like every other word of the line.
        return run
            .split_whitespace()
            .last()
            .map(|w| w.trim_matches('\'').to_owned());
    }
    let file = command.get(1).filter(|f| f.ends_with(".java"))?;
    if name != "java" {
        return None;
    }
    let path = cwd.join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    crate::java_debug::main_class(&path, &text)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0097 v4 AK-7 (behaviour 10): the compile-and-run command of a module without a build
    /// tool is debugged as its main class.
    #[test]
    fn a_compiled_module_is_debugged_as_its_class() {
        let script = "mkdir -p .ferris-idea/run 'out/production/lab' && find 'src/main/java' \
                      -name '*.java' > '.ferris-idea/run/lab.sources' && javac -d \
                      'out/production/lab' @'.ferris-idea/run/lab.sources' && java -cp \
                      'out/production/lab' 'lab.stock.Report'";
        let command: Vec<String> = ["sh", "-c", script].map(String::from).to_vec();
        let (kind, _, class, args) = plan(&command, Path::new("/w")).unwrap();
        assert_eq!(kind, Kind::Java);
        assert_eq!(class, PathBuf::from("lab.stock.Report"));
        assert!(args.is_empty());
    }

    /// SPEC-0097 v3 AK-6 (behaviour 9): `cargo run` is debugged as the program it builds.
    #[test]
    fn cargo_run_debugs_the_built_program() {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-cargo-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.as_path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"shapes\"]\n",
        )
        .unwrap();
        let member = root.join("shapes");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n\n[dependencies]\nname = \"x\"\n",
        )
        .unwrap();
        // The test may run with CARGO_TARGET_DIR set; the expected folder follows it.
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map_or_else(|| root.join("target"), |d| member.join(d));
        let exe = std::env::consts::EXE_SUFFIX;
        let plain = cargo_run(&words("cargo run"), &member).unwrap();
        assert_eq!(plain.build, "cargo build");
        assert_eq!(plain.program, target.join(format!("debug/shapes{exe}")));
        assert!(plain.args.is_empty());
        let bin = cargo_run(&words("cargo run -q --bin tool -- --size 3"), &member).unwrap();
        assert_eq!(bin.build, "cargo build --bin 'tool'");
        assert_eq!(bin.program, target.join(format!("debug/tool{exe}")));
        assert_eq!(bin.args, ["--size", "3"]);
        let example = cargo_run(&words("cargo run --example=demo"), &member).unwrap();
        assert_eq!(example.build, "cargo build --example 'demo'");
        assert_eq!(
            example.program,
            target.join(format!("debug/examples/demo{exe}"))
        );
        // The plan debugs that program natively, not cargo.
        let (kind, _, program, args) = plan(&words("cargo run --bin tool -- a"), &member).unwrap();
        assert_eq!(
            (kind, program, args),
            (Kind::Native, bin.program.clone(), vec!["a".to_owned()])
        );
        // Options that pick another program are not guessed at.
        assert!(cargo_run(&words("cargo run --release"), &member).is_none());
        assert!(cargo_run(&words("cargo run -p other"), &member).is_none());
        assert!(cargo_run(&words("cargo test"), &member).is_none());
        assert_eq!(
            package_binary("[package]\nname = \"a\"\ndefault-run = \"b\"\n").as_deref(),
            Some("b")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    /// SPEC-0045 v6 AK-15 (behaviour 12): Jest through npx is debugged with js-debug, in one
    /// process; Vitest and other npx commands are not planned for js-debug.
    #[test]
    fn jest_through_npx_runs_under_js_debug() {
        let cwd = Path::new("/work/app");
        let command =
            words("npx jest --json --outputFile=.ferris-idea/jest.json sum.test.js -t ^sum$");
        assert!(is_npx_jest(&command));
        let (kind, interpreter, program, args) = plan(&command, cwd).unwrap();
        assert_eq!(kind, Kind::PackageScript);
        assert_eq!(kind.adapter_name(), "js-debug");
        assert_eq!(interpreter, None);
        assert!(program.ends_with("npx"), "{program:?}");
        assert_eq!(
            args,
            words(
                "jest --json --outputFile=.ferris-idea/jest.json sum.test.js -t ^sum$ --runInBand"
            )
        );
        // Already in one process: nothing added.
        let (_, _, _, args) = plan(&words("npx jest -i a.test.js"), cwd).unwrap();
        assert_eq!(args, words("jest -i a.test.js"));
        for other in ["npx vitest run --reporter=json", "npx eslint ."] {
            assert!(!is_npx_jest(&words(other)), "{other}");
            assert_ne!(
                plan(&words(other), cwd).unwrap().0,
                Kind::PackageScript,
                "{other}"
            );
        }
    }

    /// The commands of a Go file, of its tests and of one test function (the ▶ markers) are
    /// debugged with Delve, never as the native program `go` (`SPEC-0039` v7 behaviour 11).
    #[test]
    fn go_commands_are_planned_for_delve() {
        let cwd = Path::new("/work/shop");
        let (kind, interpreter, program, args) = plan(&words("go run main.go -v"), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        assert_eq!(kind.adapter_name(), "dlv");
        assert_eq!(interpreter, None);
        assert_eq!(program, cwd.join("main.go"));
        assert_eq!(args, words("run main.go -v"));
        let (kind, _, program, args) =
            plan(&words("go test -json -run ^TestTotal$ ./cart"), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        assert_eq!(program, cwd.join("./cart"));
        let launch = ferris_idea_debugger::go_launch(&args, cwd, &Default::default()).unwrap();
        assert_eq!(launch["mode"], "test");
        assert_eq!(
            launch["args"],
            serde_json::json!(["-test.v", "-test.run", "^TestTotal$"])
        );
        // What Delve cannot debug is still planned for it, so the start says why.
        let (kind, _, _, args) = plan(&words("go test -json ./..."), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        assert!(ferris_idea_debugger::go_command(&args).is_err());
        let (kind, ..) = plan(&words("go build ./..."), cwd).unwrap();
        assert_eq!(kind, Kind::Go);
        // Other programs are untouched.
        let (kind, ..) = plan(&words("./gopher run"), cwd).unwrap();
        assert_eq!(kind, Kind::Native);
    }
}
