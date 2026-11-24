//! Processes started for a debug session end with it, together with what they started, and never
//! outlive Ferris IDEA, however it ended (`SPEC-0263` v2 behaviour 3).
//!
//! js-debug starts the browser of a page session in a session of its own, so ending the adapter's
//! process group does not reach it: the adapter's direct children are noted and ended one by one,
//! each only while it still runs the same command line (a process id the system gave to another
//! program since is left alone).

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A running process: its id and its command line, which tell it from a later process with the
/// same id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Process id.
    pub pid: u32,
    /// Full command line as `ps` shows it.
    pub command: String,
}

/// What `parent` started in `ps -A -o pid=,ppid=,pgid=,command=` output: its direct children and,
/// for a child that leads a process group of its own (the browser), every member of that group
/// (the browser's helpers, which can outlive a browser that was killed).
pub fn parse_children(ps: &str, parent: u32) -> Vec<Started> {
    let rows: Vec<(u32, u32, u32, String)> = ps
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid: u32 = words.next()?.parse().ok()?;
            let ppid: u32 = words.next()?.parse().ok()?;
            let pgid: u32 = words.next()?.parse().ok()?;
            Some((pid, ppid, pgid, words.collect::<Vec<_>>().join(" ")))
        })
        .collect();
    let leaders: Vec<u32> = rows
        .iter()
        .filter(|(pid, ppid, pgid, _)| *ppid == parent && pid == pgid)
        .map(|(pid, ..)| *pid)
        .collect();
    rows.into_iter()
        .filter(|(pid, ppid, pgid, _)| {
            *pid != parent && (*ppid == parent || leaders.contains(pgid))
        })
        .map(|(pid, _, _, command)| Started { pid, command })
        .collect()
}

/// What `parent` started that still runs ([`parse_children`]); empty where `ps` is missing.
pub fn children(parent: u32) -> Vec<Started> {
    if !cfg!(unix) {
        return Vec::new();
    }
    Command::new("ps")
        .args(["-A", "-ww", "-o", "pid=,ppid=,pgid=,command="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| parse_children(&String::from_utf8_lossy(&o.stdout), parent))
        .unwrap_or_default()
}

/// The command line of process `pid`, `None` when it no longer runs.
pub fn command_of(pid: u32) -> Option<String> {
    if !cfg!(unix) {
        return None;
    }
    let out = Command::new("ps")
        .args(["-ww", "-o", "command=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// The processes whose command line holds `--user-data-dir=<folder>`: a browser still using that
/// profile folder.
pub fn using_profile(folder: &std::path::Path) -> Vec<Started> {
    if !cfg!(unix) {
        return Vec::new();
    }
    // The browser may show the folder with symbolic links resolved (`/var` → `/private/var`).
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let needle = format!("--user-data-dir={}", folder.display());
    let Ok(out) = Command::new("ps")
        .args(["-A", "-ww", "-o", "pid=,ppid=,command="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid: u32 = words.next()?.parse().ok()?;
            let _ppid = words.next()?;
            let command = words.collect::<Vec<_>>().join(" ");
            let ends = command.match_indices(&needle).any(|(at, _)| {
                matches!(
                    command[at + needle.len()..].chars().next(),
                    None | Some(' ')
                )
            });
            ends.then_some(Started { pid, command })
        })
        .collect()
}

#[cfg(unix)]
fn signal(pid: u32, signal: Option<nix::sys::signal::Signal>) -> bool {
    i32::try_from(pid).is_ok_and(|pid| {
        pid > 1 && nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal).is_ok()
    })
}

/// Whether `p` still runs (its id exists; the command line is checked before a kill).
fn exists(p: &Started) -> bool {
    #[cfg(unix)]
    return signal(p.pid, None);
    #[cfg(not(unix))]
    {
        let _ = p;
        false
    }
}

/// Ends `processes`: `SIGTERM` to each that still runs the same command line, `SIGKILL` to those
/// still running after `grace`.
pub fn end_processes(processes: &[Started], grace: Duration) {
    #[cfg(unix)]
    {
        use nix::sys::signal::Signal;
        let same = |p: &Started| command_of(p.pid).is_some_and(|c| c == p.command);
        let asked: Vec<&Started> = processes.iter().filter(|p| same(p)).collect();
        for p in &asked {
            signal(p.pid, Some(Signal::SIGTERM));
        }
        let until = Instant::now() + grace;
        while asked.iter().any(|p| exists(p)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        for p in asked {
            if exists(p) && same(p) {
                signal(p.pid, Some(Signal::SIGKILL));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (processes, grace);
}

/// Ends `child` and what it started: its children are noted, it gets `SIGTERM` and after `grace`
/// `SIGKILL` and is reaped, then its children that still run end the same way.
pub fn end_tree(child: &mut Child, grace: Duration) {
    let started = children(child.id());
    if let Ok(None) = child.try_wait() {
        #[cfg(unix)]
        signal(child.id(), Some(nix::sys::signal::Signal::SIGTERM));
        let until = Instant::now() + grace;
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Ok(None) = child.try_wait() {
            let _ = child.kill();
        }
    }
    let _ = child.wait();
    end_processes(&started, grace);
}

/// The shell of an [`ExitGuard`]. `$1` is the guarded process, `$2` its command line. A line on
/// the input means Ferris IDEA ended the process itself; the end of the input means Ferris IDEA is gone
/// (quit, crashed or killed), so the process and its direct children end, each only while it still
/// runs the command line it had.
const GUARD: &str = r#"read -r _ && exit 0
cmd() { ps -ww -o command= -p "$1" 2>/dev/null; }
[ "$(cmd "$1")" = "$2" ] || exit 0
kids=$(ps -A -o pid= -o ppid= -o pgid= | awk -v p="$1" '
  { pid[NR] = $1; ppid[NR] = $2; pgid[NR] = $3; if ($2 == p && $1 == $3) lead[$1] = 1 }
  END { for (i = 1; i <= NR; i++) if (pid[i] != p && (ppid[i] == p || (pgid[i] in lead))) print pid[i] }')
for k in $kids; do eval "c_$k=\$(cmd $k)"; done
kill -TERM "$1" $kids 2>/dev/null
n=0
while [ $n -lt 30 ]; do
  alive=
  for k in "$1" $kids; do kill -0 "$k" 2>/dev/null && alive=1; done
  [ -z "$alive" ] && exit 0
  sleep 0.1
  n=$((n + 1))
done
[ "$(cmd "$1")" = "$2" ] && kill -KILL "$1" 2>/dev/null
for k in $kids; do eval "c=\$c_$k"; [ "$(cmd "$k")" = "$c" ] && kill -KILL "$k" 2>/dev/null; done
exit 0
"#;

/// Ends a process and its direct children when Ferris IDEA ends without ending them: a small shell
/// in a process group of its own reads a pipe only Ferris IDEA writes to, and the system closes that
/// pipe however Ferris IDEA ends. Dropping the guard tells the shell to leave everything alone.
pub struct ExitGuard {
    shell: Child,
}

impl ExitGuard {
    /// Guards process `pid`; `None` where no shell is available (then only a normal end of
    /// Ferris IDEA ends the process).
    pub fn watch(pid: u32) -> Option<Self> {
        if !cfg!(unix) {
            return None;
        }
        let command = command_of(pid)?;
        let mut shell = Command::new("/bin/sh");
        shell
            .args(["-c", GUARD, "ferris-idea-guard", &pid.to_string(), &command])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A terminal's Ctrl-C to Ferris IDEA does not reach the guard, which then still does its job.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut shell, 0);
        shell.spawn().ok().map(|shell| Self { shell })
    }

    /// Closes the pipe as if Ferris IDEA had ended, and waits for the shell (tests).
    #[doc(hidden)]
    pub fn trip(mut self) {
        drop(self.shell.stdin.take());
        let _ = self.shell.wait();
        std::mem::forget(self);
    }
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        use std::io::Write;
        if let Some(mut input) = self.shell.stdin.take() {
            let _ = input.write_all(b"ended\n");
        }
        let _ = self.shell.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_by_parent_id() {
        // 200 leads its own group (a browser): its helpers 400 and the orphaned 500 belong to it;
        // 600 is someone else's.
        let ps = "  1     0     1 /sbin/launchd\n 200   100   200 /Applications/Chrome --user-data-dir=/a b\n 300   100   100 node app.js\n 400   200   200 helper\n 500     1   200 orphaned helper\n 600     1   600 other\n 100     1   100 node dapDebugServer.js 0\n";
        let kids = parse_children(ps, 100);
        let pids: Vec<u32> = kids.iter().map(|s| s.pid).collect();
        assert_eq!(pids, vec![200, 300, 400, 500]);
        assert_eq!(kids[0].command, "/Applications/Chrome --user-data-dir=/a b");
        assert!(parse_children("garbage\n", 100).is_empty());
    }
}
