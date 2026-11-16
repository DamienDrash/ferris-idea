//! Commit hooks (`SPEC-0230`): found where Git finds them and run as `git commit --only` runs them,
//! against a temporary index holding exactly the commit.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::git::{GitError, Repo};

/// The hooks a commit runs, in order (behaviour 1).
pub const COMMIT_HOOKS: [&str; 4] = [
    "pre-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
];

/// Hooks that block a commit and are skipped by "Run Git hooks" off (behaviours 3, 6).
const VERIFY_HOOKS: [&str; 2] = ["pre-commit", "commit-msg"];

/// Time limit of one hook (behaviour 3).
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(300);

/// Lines of hook output kept (behaviour 3).
const OUTPUT_LINES: usize = 200;

/// One hook that ran, or would run in a dry run (`exit` is `None` then).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRun {
    /// Hook name, e.g. `pre-commit`.
    pub hook: String,
    /// Exit code; `None` when it did not run (dry run), was killed or could not start.
    pub exit: Option<i32>,
    /// Standard output and error as written, the last 200 lines.
    pub output: String,
}

impl Repo {
    /// Path of hook `name` as Git resolves it (`core.hooksPath`, linked worktrees), if it exists and
    /// is executable.
    pub fn hook_path(&self, name: &str) -> Option<PathBuf> {
        let dir = self.workdir()?;
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "--git-path", &format!("hooks/{name}")])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        let path = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        is_executable_file(&path).then_some(path)
    }

    /// The commit hooks a commit would run (behaviour 1); without `verify` the blocking ones are left
    /// out (behaviour 6).
    pub fn commit_hooks(&self, verify: bool) -> Vec<String> {
        COMMIT_HOOKS
            .iter()
            .filter(|h| verify || !VERIFY_HOOKS.contains(h))
            .filter(|h| self.hook_path(h).is_some())
            .map(|h| (*h).to_owned())
            .collect()
    }

    /// Runs hook `name` with `args` and the extra environment `env`, if it exists; `Ok(None)` when it
    /// does not. A hook that cannot start or times out reports `exit: None`.
    pub(crate) fn run_hook(
        &self,
        name: &str,
        args: &[&str],
        env: &[(&str, &Path)],
    ) -> Result<Option<HookRun>, GitError> {
        let Some(path) = self.hook_path(name) else {
            return Ok(None);
        };
        let dir = self
            .workdir()
            .ok_or_else(|| GitError::Rejected("the repository has no working tree".into()))?;
        Ok(Some(run_program(&path, name, args, env, dir, HOOK_TIMEOUT)))
    }
}

/// Runs `program` in `dir` with stdin empty, standard output and error in one stream (behaviour 2).
fn run_program(
    program: &Path,
    name: &str,
    args: &[&str],
    env: &[(&str, &Path)],
    dir: &Path,
    timeout: Duration,
) -> HookRun {
    // One pipe for both streams keeps their order, as a terminal shows it.
    #[cfg(unix)]
    let mut cmd = {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("exec \"$0\" \"$@\" 2>&1")
            .arg(program)
            .args(args);
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        cmd
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    };
    cmd.current_dir(dir)
        .env("GIT_EDITOR", ":")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let failed = |output: String| HookRun {
        hook: name.to_owned(),
        exit: None,
        output,
    };
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return failed(format!("cannot start {}: {e}", program.display())),
    };
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                #[cfg(unix)]
                if let Ok(pid) = i32::try_from(child.id()) {
                    let _ = nix::sys::signal::killpg(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let mut output = out.join().unwrap_or_default();
    output.push_str(&err.join().unwrap_or_default());
    if status.is_none() {
        output.push_str(&format!(
            "\n{name} hook timed out after {} s and was stopped",
            timeout.as_secs()
        ));
    }
    HookRun {
        hook: name.to_owned(),
        exit: status.and_then(|s| s.code()),
        output: last_lines(&output, OUTPUT_LINES),
    }
}

/// The last `n` lines of `text`, trimmed at the end.
fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// The commit message as hooks leave it: comment lines dropped, trailing blank lines trimmed
/// (behaviour 4).
pub(crate) fn clean_message(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let mut out = kept.join("\n").trim_end().to_owned();
    while out.starts_with('\n') {
        out.remove(0);
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_cleaning_and_output_tail() {
        assert_eq!(clean_message("fix\n\n# comment\nbody\n\n"), "fix\n\nbody\n");
        assert_eq!(clean_message("# only\n"), "");
        let long: String = (0..250).map(|i| format!("l{i}\n")).collect();
        let tail = last_lines(&long, 200);
        assert!(tail.starts_with("l50\n") && tail.ends_with("l249"));
    }
}
