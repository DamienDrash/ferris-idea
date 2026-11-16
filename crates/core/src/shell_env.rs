//! The environment of the user's login shell for a window that was not started from a shell
//! (`SPEC-0328`): whether it is needed, asking the shell for it under a time limit, which variables
//! are taken, and the tool folders added to `PATH` when the shell does not answer.
//!
//! Nothing here changes the process environment or writes a value anywhere: [`resolve`] returns
//! the variables to set, and the caller sets them at the very start of `main`, before any thread
//! exists. Values are never logged, shown or sent; a [`Report`] holds only where the environment
//! came from, how long that took and a note without values.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The switch (`SPEC-0328` behaviour 3): `off` never asks the shell, `force` asks it for every
/// command, anything else decides by the start.
pub const SWITCH: &str = "IDECRAFT_SHELL_ENV";

/// Set to `1` for the shell while it is asked, so rc files can skip slow or interactive parts
/// (behaviour 4). Never taken over.
pub const RESOLVING: &str = "IDECRAFT_RESOLVING_ENVIRONMENT";

/// How long the shell may take (behaviour 5).
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// File name of the log in [`crate::logs::log_dir`] with one line per resolved start (behaviour 9).
pub const ENVIRONMENT_LOG: &str = "environment.log";

/// The `PATH` a program gets that macOS starts from Finder, the Dock or `open`.
const LAUNCHD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// More output than this is not an environment (rc files printing without end).
#[cfg(unix)]
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

/// After its answer the shell gets this long to end by itself.
#[cfg(unix)]
const EXIT_GRACE: Duration = Duration::from_millis(250);

/// Where the process environment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// As the process got it from its parent.
    Inherited,
    /// From the user's login shell.
    LoginShell,
    /// The shell failed or took too long: inherited, with the usual tool folders added to `PATH`.
    Fallback,
}

impl Source {
    /// The name in the log and over MCP.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Inherited => "inherited",
            Source::LoginShell => "login_shell",
            Source::Fallback => "fallback",
        }
    }
}

/// The outcome without any value of the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Where the environment came from.
    pub source: Source,
    /// How long asking the shell took (zero when it was not asked).
    pub duration: Duration,
    /// Why, in words; names the shell and added folders, never a variable's value.
    pub note: String,
}

/// What [`resolve`] found: the report and the variables the caller has to set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The outcome.
    pub report: Report,
    /// Variables to set in the process, name and value.
    pub set: Vec<(OsString, OsString)>,
}

/// The value of [`SWITCH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// Decide by the start.
    Auto,
    /// Never ask the shell.
    Off,
    /// Ask the shell whatever the command and the start.
    Force,
}

impl Switch {
    /// Reads the switch's value; unknown values decide by the start.
    pub fn parse(value: Option<&OsStr>) -> Self {
        match value.and_then(OsStr::to_str).map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("off") => Switch::Off,
            Some(v) if v.eq_ignore_ascii_case("force") => Switch::Force,
            _ => Switch::Auto,
        }
    }
}

/// What [`resolve`] works on. Tests build it by hand; the program uses [`Request::for_process`].
#[derive(Debug, Clone)]
pub struct Request {
    /// The environment the process has.
    pub env: Vec<(OsString, OsString)>,
    /// Whether the command opens the IDE window.
    pub window: bool,
    /// How long the shell may take.
    pub timeout: Duration,
    /// Folders added to `PATH` when the shell does not answer, those that exist.
    pub tool_dirs: Vec<PathBuf>,
    /// Makes the shell's command start in a session of its own, away from the terminal the
    /// process may have (behaviour 4); the program supplies it, since it needs code this crate
    /// does not allow. Without it the shell only gets a process group of its own.
    pub new_session: Option<fn(&mut std::process::Command)>,
}

impl Request {
    /// The request for this process.
    pub fn for_process(window: bool) -> Self {
        let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let home = lookup(&env, "HOME").map(PathBuf::from);
        Request {
            tool_dirs: tool_dirs(home.as_deref()),
            env,
            window,
            timeout: TIMEOUT,
            new_session: None,
        }
    }
}

fn lookup<'a>(env: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    env.iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_os_str())
        .filter(|v| !v.is_empty())
}

/// Whether the environment looks like one a shell handed down: a shell level, or a terminal type
/// other than `dumb`.
pub fn from_shell(env: &[(OsString, OsString)]) -> bool {
    lookup(env, "SHLVL").is_some() || lookup(env, "TERM").is_some_and(|t| t != "dumb")
}

/// Whether `PATH` is missing or the one macOS gives programs it starts itself.
pub fn default_path(env: &[(OsString, OsString)]) -> bool {
    lookup(env, "PATH").is_none_or(|p| p == LAUNCHD_PATH)
}

/// Whether the shell has to be asked (behaviours 2–3); the error says why not.
pub fn needed(env: &[(OsString, OsString)], window: bool) -> Result<(), &'static str> {
    match Switch::parse(lookup(env, SWITCH)) {
        Switch::Off => Err("switched off with IDECRAFT_SHELL_ENV=off"),
        Switch::Force => Ok(()),
        Switch::Auto if !window => Err("not a window start"),
        Switch::Auto if from_shell(env) && !default_path(env) => Err("started from a shell"),
        Switch::Auto => Ok(()),
    }
}

/// Whether a variable of the shell is taken over (behaviour 7). Not taken: what changes how
/// IDECraft itself loads code (`DYLD_*`, `LD_*`), functions exported by bash, what describes the
/// asked shell rather than the user's setup, and names the process could not set.
pub fn importable(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    const SHELL_STATE: [&str; 9] = [
        "_", "PWD", "OLDPWD", "SHLVL", "TERM", "TERMCAP", "COLUMNS", "LINES", RESOLVING,
    ];
    !(name.is_empty()
        || name.contains(['=', '\0'])
        || name.starts_with("DYLD_")
        || name.starts_with("LD_")
        || name.starts_with("BASH_FUNC_")
        || SHELL_STATE.contains(&name))
}

/// The variables to set so the process has the shell's environment: those taken over
/// ([`importable`]) whose value differs. Variables only the process has stay as they are.
pub fn changes(
    current: &[(OsString, OsString)],
    shell: Vec<(OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    shell
        .into_iter()
        .filter(|(name, value)| {
            importable(name)
                && !value.as_encoded_bytes().contains(&0)
                && !current.iter().any(|(k, v)| k == name && v == value)
        })
        .collect()
}

/// The usual tool folders (behaviour 8), in the order they are added.
pub fn tool_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".cargo/bin"));
    }
    dirs
}

/// `path` with those of `dirs` appended that exist and are not on it yet, and the folders added.
/// `None` when nothing was added.
pub fn fallback_path(path: Option<&OsStr>, dirs: &[PathBuf]) -> Option<(OsString, Vec<PathBuf>)> {
    let mut entries: Vec<PathBuf> =
        std::env::split_paths(path.unwrap_or(OsStr::new(LAUNCHD_PATH))).collect();
    let mut added = Vec::new();
    for dir in dirs {
        if dir.is_dir() && !entries.contains(dir) {
            let mut longer = entries.clone();
            longer.push(dir.clone());
            // A folder whose name cannot be on a search path is left out.
            if std::env::join_paths(&longer).is_ok() {
                entries = longer;
                added.push(dir.clone());
            }
        }
    }
    if added.is_empty() {
        return None;
    }
    std::env::join_paths(&entries).ok().map(|p| (p, added))
}

/// `program` in the folders of the search path `path`.
pub fn find_in(path: &OsStr, program: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// Decides and, when needed, asks the login shell (behaviours 2–8). Changes nothing: the caller
/// sets [`Resolution::set`].
pub fn resolve(request: &Request) -> Resolution {
    if let Err(why) = needed(&request.env, request.window) {
        return inherited(why);
    }
    resolve_needed(request)
}

fn inherited(why: &str) -> Resolution {
    Resolution {
        report: Report {
            source: Source::Inherited,
            duration: Duration::ZERO,
            note: why.to_owned(),
        },
        set: Vec::new(),
    }
}

#[cfg(not(unix))]
fn resolve_needed(_request: &Request) -> Resolution {
    inherited("not used on this system")
}

#[cfg(unix)]
fn resolve_needed(request: &Request) -> Resolution {
    let started = Instant::now();
    let shell = unix::shell_program(lookup(&request.env, "SHELL"));
    let name = shell
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let home = lookup(&request.env, "HOME").map(Path::new);
    match unix::capture(&shell, request, home) {
        Ok(vars) => {
            let set = changes(&request.env, vars);
            Resolution {
                report: Report {
                    source: Source::LoginShell,
                    duration: started.elapsed(),
                    note: format!("login shell {name}: {} variables set", set.len()),
                },
                set,
            }
        }
        Err(failure) => {
            let fallback = fallback_path(lookup(&request.env, "PATH"), &request.tool_dirs);
            let added = match &fallback {
                Some((_, dirs)) => format!(
                    "added to PATH: {}",
                    dirs.iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None => "no tool folder to add to PATH".to_owned(),
            };
            Resolution {
                report: Report {
                    source: Source::Fallback,
                    duration: started.elapsed(),
                    note: format!(
                        "login shell {name} {}; {added}",
                        failure.words(request.timeout)
                    ),
                },
                set: fallback
                    .map(|(path, _)| vec![(OsString::from("PATH"), path)])
                    .unwrap_or_default(),
            }
        }
    }
}

static REPORT: OnceLock<Report> = OnceLock::new();
static NOTE_TAKEN: AtomicBool = AtomicBool::new(false);

/// Keeps the outcome for [`report`]; the first call counts.
pub fn record(report: Report) {
    let _ = REPORT.set(report);
}

/// Where this process's environment came from; inherited when nothing was recorded.
pub fn report() -> Report {
    REPORT.get().cloned().unwrap_or(Report {
        source: Source::Inherited,
        duration: Duration::ZERO,
        note: "not resolved".to_owned(),
    })
}

/// The note the first window shows when the shell did not answer (behaviour 9); `None` otherwise
/// and after the first call.
pub fn take_window_note() -> Option<String> {
    let report = REPORT.get()?;
    if report.source != Source::Fallback || NOTE_TAKEN.swap(true, Ordering::SeqCst) {
        return None;
    }
    Some(format!("Tools may be missing: {}", report.note))
}

/// The log line of an outcome (behaviour 9): time, source, duration, the number of `PATH` entries
/// and the note — no value of the environment.
pub fn log_line(report: &Report, path_entries: usize, time: std::time::SystemTime) -> String {
    format!(
        "{}\tsource={}\tduration_ms={}\tpath_entries={path_entries}\tnote={}",
        crate::logs::utc_stamp(time),
        report.source.as_str(),
        report.duration.as_millis(),
        report.note
    )
}

/// Asking the shell.
#[cfg(unix)]
mod unix {
    use super::{EXIT_GRACE, MAX_OUTPUT, RESOLVING};
    use std::ffi::{OsStr, OsString};
    use std::io::Read;
    use std::os::fd::AsFd;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::{Pid, Uid, User};

    /// Why the shell gave no environment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Failure {
        /// It could not be started.
        Start,
        /// It did not answer in time.
        Timeout,
        /// It ended, or printed too much, without a complete answer.
        NoAnswer,
    }

    impl Failure {
        pub fn words(self, timeout: Duration) -> String {
            match self {
                Failure::Start => "could not be started".to_owned(),
                Failure::Timeout => {
                    format!("did not answer within {} s", timeout.as_secs_f32())
                }
                Failure::NoAnswer => "gave no environment".to_owned(),
            }
        }
    }

    /// The shell to ask (behaviour 4): `$SHELL`, else the one of the user's account, each only as
    /// an absolute path to a file; else `/bin/zsh` on macOS and `/bin/sh` elsewhere.
    pub fn shell_program(from_env: Option<&OsStr>) -> PathBuf {
        let usable = |p: &PathBuf| p.is_absolute() && p.is_file();
        from_env
            .map(PathBuf::from)
            .filter(usable)
            .or_else(|| {
                User::from_uid(Uid::current())
                    .ok()
                    .flatten()
                    .map(|u| u.shell)
                    .filter(usable)
            })
            .unwrap_or_else(|| {
                PathBuf::from(if cfg!(target_os = "macos") {
                    "/bin/zsh"
                } else {
                    "/bin/sh"
                })
            })
    }

    /// The arguments before the command: an interactive login shell. The csh family takes the
    /// login flag only alone, so it is asked as an interactive shell.
    fn shell_arguments(shell: &Path) -> &'static [&'static str] {
        match shell.file_name().and_then(OsStr::to_str) {
            Some("csh" | "tcsh") => &["-i", "-c"],
            _ => &["-l", "-i", "-c"],
        }
    }

    /// The environment between the first and the last `marker` in `output`: entries end with NUL,
    /// so values keep their line breaks; a name ends at the first `=`. `None` until both markers
    /// are there.
    pub fn parse(output: &[u8], marker: &[u8]) -> Option<Vec<(OsString, OsString)>> {
        let find = |hay: &[u8]| hay.windows(marker.len()).position(|w| w == marker);
        let start = find(output)? + marker.len();
        let rest = &output[start..];
        let end = rest
            .windows(marker.len())
            .rposition(|w| w == marker)
            .filter(|_| !marker.is_empty())?;
        Some(
            rest[..end]
                .split(|b| *b == 0)
                .filter_map(|entry| {
                    let eq = entry.iter().position(|b| *b == b'=')?;
                    (eq > 0).then(|| {
                        (
                            OsString::from_vec(entry[..eq].to_vec()),
                            OsString::from_vec(entry[eq + 1..].to_vec()),
                        )
                    })
                })
                .collect(),
        )
    }

    /// Runs `shell` with the request's environment as an interactive login shell in `home` that
    /// prints its environment, and reads it on this thread. The shell has no input, its own process group, and is gone with
    /// everything in that group when this returns.
    pub fn capture(
        shell: &Path,
        request: &super::Request,
        home: Option<&Path>,
    ) -> Result<Vec<(OsString, OsString)>, Failure> {
        let marker = format!(
            "__IDECRAFT_ENV_{}_{}__",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let mut cmd = Command::new(shell);
        cmd.args(shell_arguments(shell))
            .arg(format!(
                "printf %s {marker}; /usr/bin/env -0; printf %s {marker}"
            ))
            .env_clear()
            .envs(request.env.iter().map(|(k, v)| (k, v)))
            .env(RESOLVING, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Either way the shell leads a process group named by its process id.
        match request.new_session {
            Some(new_session) => new_session(&mut cmd),
            None => {
                cmd.process_group(0);
            }
        }
        // Not in the project folder: nothing of an untrusted project is run or read.
        if let Some(home) = home.filter(|h| h.is_dir()) {
            cmd.current_dir(home);
        }
        let mut child = crate::spawn::spawn(&mut cmd).map_err(|_| Failure::Start)?;
        let answer = read_answer(
            &mut child,
            marker.as_bytes(),
            Instant::now() + request.timeout,
        );
        if answer.is_ok() {
            let until = Instant::now() + EXIT_GRACE;
            while Instant::now() < until && matches!(child.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // Before reaping, so the group's id still belongs to the shell.
        if let Ok(id) = i32::try_from(child.id()) {
            let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
        }
        let _ = child.wait();
        answer
    }

    fn read_answer(
        child: &mut Child,
        marker: &[u8],
        deadline: Instant,
    ) -> Result<Vec<(OsString, OsString)>, Failure> {
        let mut out = child.stdout.take().ok_or(Failure::Start)?;
        let mut output = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Failure::Timeout);
            }
            let wait = u16::try_from(left.as_millis().clamp(1, 200)).unwrap_or(200);
            let mut fds = [PollFd::new(out.as_fd(), PollFlags::POLLIN)];
            match poll(&mut fds, PollTimeout::from(wait)) {
                Ok(0) | Err(nix::errno::Errno::EINTR) => continue,
                Ok(_) => {}
                Err(_) => return Err(Failure::NoAnswer),
            }
            match out.read(&mut chunk) {
                // The shell and everything holding its output ended.
                Ok(0) => return parse(&output, marker).ok_or(Failure::NoAnswer),
                Ok(n) => {
                    output.extend_from_slice(&chunk[..n]);
                    // Complete with the second marker, whoever still holds the pipe.
                    if let Some(vars) = parse(&output, marker) {
                        return Ok(vars);
                    }
                    if output.len() > MAX_OUTPUT {
                        return Err(Failure::NoAnswer);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(Failure::NoAnswer),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    #[test]
    fn the_switch_reads_off_and_force() {
        assert_eq!(Switch::parse(None), Switch::Auto);
        assert_eq!(Switch::parse(Some(OsStr::new("off"))), Switch::Off);
        assert_eq!(Switch::parse(Some(OsStr::new(" OFF "))), Switch::Off);
        assert_eq!(Switch::parse(Some(OsStr::new("force"))), Switch::Force);
        assert_eq!(Switch::parse(Some(OsStr::new("yes"))), Switch::Auto);
    }

    #[test]
    fn only_a_window_not_started_from_a_shell_asks() {
        let finder = env(&[("PATH", LAUNCHD_PATH), ("HOME", "/Users/u")]);
        let terminal = env(&[
            ("PATH", "/opt/homebrew/bin:/usr/bin:/bin"),
            ("TERM", "xterm-256color"),
            ("SHLVL", "1"),
        ]);
        assert_eq!(needed(&finder, true), Ok(()));
        assert_eq!(needed(&finder, false), Err("not a window start"));
        assert_eq!(needed(&terminal, true), Err("started from a shell"));
        // A shell with the bare system path has not read the user's setup.
        let bare = env(&[("PATH", LAUNCHD_PATH), ("TERM", "xterm"), ("SHLVL", "2")]);
        assert_eq!(needed(&bare, true), Ok(()));
        assert_eq!(needed(&env(&[("SHLVL", "1")]), true), Ok(()));
        // A full path without a terminal (a desktop session, another program's child).
        let desktop = env(&[("PATH", "/usr/local/bin:/usr/bin"), ("TERM", "dumb")]);
        assert_eq!(needed(&desktop, true), Ok(()));
        let only_term = env(&[("PATH", "/usr/local/bin:/usr/bin"), ("TERM", "xterm")]);
        assert_eq!(needed(&only_term, true), Err("started from a shell"));
    }

    #[test]
    fn the_switch_wins_over_the_start() {
        let mut finder = env(&[("PATH", LAUNCHD_PATH), (SWITCH, "off")]);
        assert_eq!(
            needed(&finder, true),
            Err("switched off with IDECRAFT_SHELL_ENV=off")
        );
        finder[1].1 = OsString::from("force");
        assert_eq!(needed(&finder, false), Ok(()));
        let terminal = env(&[("PATH", "/x"), ("SHLVL", "1"), (SWITCH, "force")]);
        assert_eq!(needed(&terminal, false), Ok(()));
    }

    #[test]
    fn loader_shell_state_and_function_variables_are_not_taken() {
        for name in [
            "PATH",
            "JAVA_HOME",
            "SSH_AUTH_SOCK",
            "LANG",
            "LDFLAGS",
            "IDECRAFT_LOG_DIR",
        ] {
            assert!(importable(OsStr::new(name)), "{name}");
        }
        for name in [
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "BASH_FUNC_x%%",
            "_",
            "PWD",
            "OLDPWD",
            "SHLVL",
            "TERM",
            "COLUMNS",
            RESOLVING,
            "",
            "A=B",
        ] {
            assert!(!importable(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn changes_keep_what_only_the_process_has() {
        let current = env(&[("PATH", "/usr/bin"), ("ONLY_HERE", "1"), ("SAME", "x")]);
        let shell = env(&[
            ("PATH", "/tools:/usr/bin"),
            ("SAME", "x"),
            ("NEW", "a\nb=c"),
            ("LD_PRELOAD", "/evil.so"),
            ("SHLVL", "1"),
        ]);
        assert_eq!(
            changes(&current, shell),
            env(&[("PATH", "/tools:/usr/bin"), ("NEW", "a\nb=c")])
        );
    }

    #[test]
    fn the_fallback_appends_existing_tool_folders_once() {
        let dir = std::env::temp_dir().join(format!("idecraft-shellenv-fb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (bin, cargo) = (dir.join(".local/bin"), dir.join(".cargo/bin"));
        std::fs::create_dir_all(&bin).unwrap();
        let dirs = tool_dirs(Some(&dir));
        assert_eq!(dirs[2..], [bin.clone(), cargo]);
        // Only what exists, and nothing twice.
        let (path, added) = fallback_path(Some(OsStr::new("/usr/bin:/bin")), &dirs[2..]).unwrap();
        assert_eq!(added, std::slice::from_ref(&bin));
        assert_eq!(
            std::env::split_paths(&path).collect::<Vec<_>>(),
            [PathBuf::from("/usr/bin"), PathBuf::from("/bin"), bin]
        );
        assert_eq!(fallback_path(Some(&path), &dirs[2..]), None);
        // Without a search path the system's default is the start.
        let (path, _) = fallback_path(None, &dirs[2..]).unwrap();
        assert!(path.to_string_lossy().starts_with(LAUNCHD_PATH));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_answer_is_read_between_the_markers() {
        let m = b"__M__";
        let out = b"Welcome!\nPATH=/noise\n__M__PATH=/a:/b\0MULTI=line one\nline two\0EQ=a=b=c\0EMPTY=\0=odd\0novalue\0__M__bye\n";
        assert_eq!(
            unix::parse(out, m).unwrap(),
            env(&[
                ("PATH", "/a:/b"),
                ("MULTI", "line one\nline two"),
                ("EQ", "a=b=c"),
                ("EMPTY", ""),
            ])
        );
        // Incomplete without the second marker; nothing without any.
        assert_eq!(unix::parse(b"noise__M__PATH=/a\0", m), None);
        assert_eq!(unix::parse(b"PATH=/a\0", m), None);
        assert_eq!(unix::parse(b"__M____M__", m), Some(Vec::new()));
    }

    #[test]
    fn the_log_line_and_the_window_note_carry_no_values() {
        let report = Report {
            source: Source::LoginShell,
            duration: Duration::from_millis(412),
            note: "login shell zsh: 42 variables set".to_owned(),
        };
        let t = std::time::UNIX_EPOCH + Duration::from_millis(1_793_787_303_123);
        assert_eq!(
            log_line(&report, 14, t),
            "2026-11-04T10:15:03.123Z\tsource=login_shell\tduration_ms=412\tpath_entries=14\tnote=login shell zsh: 42 variables set"
        );
        // Nothing recorded in a test process: inherited, and no note for the window.
        assert_eq!(super::report().source, Source::Inherited);
        assert_eq!(take_window_note(), None);
    }
}
