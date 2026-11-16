//! SPEC-0328 AK-10: through the binary — a process told to ask the login shell uses a stub shell's
//! environment for its own tool lookups and says where it came from, without any value.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// A value the stub shell exports; it must not show up in any output or log.
const SECRET: &str = "s3cr3t-value-of-the-stub-shell";

struct Setup {
    dir: PathBuf,
}

impl Setup {
    /// A project, a settings and a log folder, a tool folder with stub language servers, and a stub shell
    /// that puts the tool folder on `PATH`.
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "idecraft-app-shellenv-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["project", "config", "logs", "home", "tools"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("project/main.py"), "import requests\n").unwrap();
        let script = |path: &Path, body: &str| {
            std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        for server in ["basedpyright-langserver", "pyright-langserver"] {
            script(&dir.join("tools").join(server), "exit 0");
        }
        script(
            &dir.join("stubsh"),
            &format!(
                "echo 'noise before the answer'\necho asked > \"$0.asked\"\nexport PATH=\"{}:$PATH\"\nexport STUB_TOKEN={SECRET}\nexec /bin/sh -c \"$4\"",
                dir.join("tools").display()
            ),
        );
        Setup { dir }
    }

    /// `idecraft call <project> <tool> [<json>]` with the bare system `PATH` and the stub as the
    /// user's shell.
    fn call(&self, switch: Option<&str>, tool: &str, arguments: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_idecraft"));
        cmd.arg("call").arg(self.dir.join("project")).arg(tool);
        if let Some(a) = arguments {
            cmd.arg(a);
        }
        cmd.env("PATH", "/usr/bin:/bin")
            .env("SHELL", self.dir.join("stubsh"))
            .env("HOME", self.dir.join("home"))
            .env("IDECRAFT_CONFIG_DIR", self.dir.join("config"))
            .env("IDECRAFT_LOG_DIR", self.dir.join("logs"))
            .env_remove("IDECRAFT_TRUST_PROJECTS")
            .env_remove("IDECRAFT_SHELL_ENV");
        if let Some(s) = switch {
            cmd.env("IDECRAFT_SHELL_ENV", s);
        }
        cmd.output().unwrap()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("logs/environment.log")).unwrap_or_default()
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn json(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn all_output(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_forced_process_takes_the_stub_shells_path_and_finds_its_tool() {
    let s = Setup::new("forced");
    let tools = s.dir.join("tools").display().to_string();

    let out = s.call(Some("force"), "environment_source", None);
    let got = json(&out);
    assert_eq!(got["source"], "login_shell", "{got}");
    assert_eq!(got["path"][0], tools.as_str(), "{got}");
    assert!(
        got["note"]
            .as_str()
            .unwrap()
            .starts_with("login shell stubsh: "),
        "{got}"
    );
    assert!(!all_output(&out).contains(SECRET));

    // The process's own lookups use it: a Python language server is found (OBS-0034 L5 began
    // with none being found).
    let servers = s.call(Some("force"), "language_servers", None);
    let text = all_output(&servers);
    assert!(servers.status.success(), "{text}");
    assert!(
        text.contains(&format!("{tools}/basedpyright-langserver"))
            || text.contains(&format!("{tools}/pyright-langserver")),
        "{text}"
    );
    assert!(!text.contains(SECRET));
    let plain = all_output(&s.call(None, "language_servers", None));
    assert!(!plain.contains(&tools), "{plain}");

    // One log line per asked start, with source, duration and counts — no value.
    let log = s.log();
    assert_eq!(log.lines().count(), 2, "{log}");
    for line in log.lines() {
        assert!(
            line.contains("\tsource=login_shell\tduration_ms="),
            "{line}"
        );
        assert!(line.contains("\tpath_entries=3\t"), "{line}");
    }
    assert!(!log.contains(SECRET) && !log.contains(&tools), "{log}");
}

#[test]
fn without_the_switch_a_call_keeps_its_environment() {
    let s = Setup::new("plain");
    for switch in [None, Some("off")] {
        let got = json(&s.call(switch, "environment_source", None));
        assert_eq!(got["source"], "inherited", "{got}");
        assert_eq!(got["duration_ms"], 0);
        assert_eq!(got["path"], serde_json::json!(["/usr/bin", "/bin"]));
    }
    assert!(
        !s.dir.join("stubsh.asked").exists(),
        "the shell was started"
    );
    assert_eq!(s.log(), "");
}
