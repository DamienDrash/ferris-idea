//! SPEC-0328 AK-5 to AK-8: the login shell's environment through stub shells — a script used as
//! the shell that prints noise and then answers, one that hangs, one that answers nothing.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use idecraft_core::shell_env::{Request, Resolution, Source, find_in, resolve};

/// One stub at a time: a script being written while another test starts a program cannot be
/// started on Linux ("text file busy").
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Stub {
    dir: PathBuf,
    shell: PathBuf,
}

impl Stub {
    /// A folder with `home/`, `tools/mytool` and the script `body` as the shell.
    fn new(name: &str, body: &str) -> Self {
        let dir = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("idecraft-shellenv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::create_dir_all(dir.join("tools")).unwrap();
        std::fs::write(dir.join("tools/mytool"), "#!/bin/sh\n").unwrap();
        let shell = dir.join("stubsh");
        std::fs::write(&shell, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        Stub { dir, shell }
    }

    fn request(&self, extra: &[(&str, &str)], timeout: Duration) -> Request {
        let mut env: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), self.dir.join("home").into()),
            ("SHELL".into(), self.shell.clone().into()),
            ("TOOLS".into(), self.dir.join("tools").into()),
            ("ONLY_IN_THE_PROCESS".into(), "1".into()),
        ];
        env.extend(extra.iter().map(|(k, v)| (k.into(), v.into())));
        Request {
            env,
            window: true,
            timeout,
            tool_dirs: vec![self.dir.join("tools"), self.dir.join("missing")],
            new_session: None,
        }
    }

    /// What the stub wrote next to itself as `stubsh.<suffix>`.
    fn wrote(&self, suffix: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.join(format!("stubsh.{suffix}")))
            .ok()
            .map(|s| s.trim().to_owned())
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn value<'a>(r: &'a Resolution, name: &str) -> Option<&'a OsString> {
    r.set.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

/// Whether the process `pid` is gone (a killed child of the stub is reaped by the system a moment
/// later).
fn gone(pid: &str) -> bool {
    let pid = nix::unistd::Pid::from_raw(pid.parse().unwrap());
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if nix::sys::signal::kill(pid, None) == Err(nix::errno::Errno::ESRCH) {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The answering stub: noise first (also a line that looks like a variable), then the setup an rc
/// file would do, then the command it was given.
const ANSWERS: &str = r#"echo "Welcome to the stub shell"
echo "PATH=/noise"
echo "$*" > "$0.args"
echo "$IDECRAFT_RESOLVING_ENVIRONMENT" > "$0.resolving"
pwd > "$0.cwd"
cat > "$0.stdin"
export PATH="$TOOLS:$PATH"
export MULTI='one
two=2'
export LD_STUB_TEST=1
export DYLD_STUB_TEST=1
exec /bin/sh -c "$4""#;

#[test]
fn a_stub_shell_gives_the_path_and_a_tool_on_it_is_found() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let stub = Stub::new("answers", ANSWERS);
    let request = stub.request(&[], Duration::from_secs(20));
    assert_eq!(find_in("/usr/bin:/bin".as_ref(), "mytool"), None);

    let r = resolve(&request);
    assert_eq!(r.report.source, Source::LoginShell, "{:?}", r.report);
    let path = value(&r, "PATH").expect("PATH is set");
    assert_eq!(
        find_in(path, "mytool"),
        Some(stub.dir.join("tools/mytool")),
        "the tool is found on the shell's PATH"
    );
    assert!(!path.to_string_lossy().contains("/noise"));
    assert_eq!(value(&r, "MULTI").unwrap(), "one\ntwo=2");
    // Not taken: loader variables, the shell's own state, the marker for rc files; and nothing
    // the process has is set again or removed.
    for (name, _) in &r.set {
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with("LD_")
                && !name.starts_with("DYLD_")
                && !["PWD", "SHLVL", "_", "IDECRAFT_RESOLVING_ENVIRONMENT"].contains(&&*name)
                && name != "ONLY_IN_THE_PROCESS"
                && name != "HOME",
            "{name}"
        );
    }
    // An interactive login shell with the command, in the home folder, without input, told that
    // it is asked for the environment.
    let args = stub.wrote("args").unwrap();
    assert!(
        args.starts_with("-l -i -c printf %s __IDECRAFT_ENV_"),
        "{args}"
    );
    assert!(
        args.contains("; /usr/bin/env -0; printf %s __IDECRAFT_ENV_"),
        "{args}"
    );
    assert_eq!(
        Path::new(&stub.wrote("cwd").unwrap()),
        stub.dir.join("home")
    );
    assert_eq!(stub.wrote("resolving").unwrap(), "1");
    assert_eq!(stub.wrote("stdin").unwrap(), "");
    // The note names the shell and a count, no value.
    assert_eq!(
        r.report.note,
        format!("login shell stubsh: {} variables set", r.set.len())
    );
}

#[test]
fn an_answer_counts_even_while_something_keeps_the_output_open() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let stub = Stub::new(
        "lingers",
        "sleep 60 &\necho $! > \"$0.child\"\nexport PATH=\"$TOOLS:$PATH\"\nexec /bin/sh -c \"$4\"",
    );
    let t0 = Instant::now();
    let r = resolve(&stub.request(&[], Duration::from_secs(20)));
    assert_eq!(r.report.source, Source::LoginShell, "{:?}", r.report);
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    assert!(find_in(value(&r, "PATH").unwrap(), "mytool").is_some());
    assert!(
        gone(&stub.wrote("child").unwrap()),
        "the background job is gone"
    );
}

#[test]
fn a_shell_that_hangs_is_ended_and_the_tool_folders_are_added() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let stub = Stub::new(
        "hangs",
        "echo noise\necho $$ > \"$0.pid\"\nsleep 60 &\necho $! > \"$0.child\"\nwait",
    );
    let t0 = Instant::now();
    let r = resolve(&stub.request(&[], Duration::from_millis(800)));
    let took = t0.elapsed();
    assert_eq!(r.report.source, Source::Fallback, "{:?}", r.report);
    assert!(
        took >= Duration::from_millis(800) && took < Duration::from_secs(10),
        "{took:?}"
    );
    assert!(r.report.duration >= Duration::from_millis(800));
    // Only PATH changes: the tool folder that exists is appended.
    let tools = stub.dir.join("tools");
    assert_eq!(
        r.set,
        [(
            OsString::from("PATH"),
            OsString::from(format!("/usr/bin:/bin:{}", tools.display()))
        )]
    );
    assert!(find_in(&r.set[0].1, "mytool").is_some());
    assert_eq!(
        r.report.note,
        format!(
            "login shell stubsh did not answer within 0.8 s; added to PATH: {}",
            tools.display()
        )
    );
    // The shell and its child are gone.
    assert!(gone(&stub.wrote("pid").unwrap()), "the shell is gone");
    assert!(gone(&stub.wrote("child").unwrap()), "its child is gone");
}

#[test]
fn a_shell_without_an_answer_falls_back() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let stub = Stub::new("silent", "echo \"PATH=/noise\"\nexit 3");
    let r = resolve(&stub.request(&[], Duration::from_secs(20)));
    assert_eq!(r.report.source, Source::Fallback, "{:?}", r.report);
    assert!(
        r.report
            .note
            .starts_with("login shell stubsh gave no environment; added to PATH: "),
        "{}",
        r.report.note
    );
    assert_eq!(r.set.len(), 1);
}

#[test]
fn the_shell_is_not_asked_when_it_is_not_needed() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let stub = Stub::new("unasked", "echo asked > \"$0.asked\"");
    let long = Duration::from_secs(20);
    // Started from a shell, switched off, or not a window.
    let mut from_shell = stub.request(&[("SHLVL", "1"), ("TERM", "xterm-256color")], long);
    from_shell.env[0].1 = "/opt/tools/bin:/usr/bin:/bin".into();
    let off = stub.request(&[("IDECRAFT_SHELL_ENV", "off")], long);
    let mut no_window = stub.request(&[], long);
    no_window.window = false;
    for request in [from_shell, off, no_window] {
        let r = resolve(&request);
        assert_eq!(r.report.source, Source::Inherited, "{:?}", r.report);
        assert!(r.set.is_empty());
        assert_eq!(r.report.duration, Duration::ZERO);
    }
    assert_eq!(stub.wrote("asked"), None);
    // The switch asks for it without a window.
    let mut forced = stub.request(&[("IDECRAFT_SHELL_ENV", "force")], long);
    forced.window = false;
    assert_eq!(resolve(&forced).report.source, Source::Fallback);
    assert_eq!(stub.wrote("asked").as_deref(), Some("asked"));
}
