//! State that survives any exit (`SPEC-0262`): state files are written atomically; a decided quit
//! ends the process within a deadline (`SPEC-0261` v4 behaviour 17, `SPEC-0262` v2 behaviour 3).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long a decided quit may take before the process ends anyway (`SPEC-0261` v4 behaviour 17).
pub const QUIT_DEADLINE: Duration = Duration::from_secs(5);

/// Whether the quit deadline is armed.
static QUIT_ARMED: AtomicBool = AtomicBool::new(false);

/// Arms the quit deadline, once per process: if the process still runs [`QUIT_DEADLINE`] from now
/// (the platform refused or stalled the quit), the programs of run configurations end
/// (`SPEC-0261` behaviour 13) and the process exits with status 0. Returns whether this call armed
/// it. Call it whenever quitting is decided; the normal quit usually ends the process first.
pub fn arm_quit_deadline() -> bool {
    arm_deadline(QUIT_DEADLINE, &QUIT_ARMED, || end_now(QUIT_DEADLINE))
}

/// How long a process asked to end by a signal may take before it ends anyway: with the half
/// second for the programs of run configurations it is gone within 5 s (`SPEC-0262` v3
/// behaviour 3).
pub const SIGNAL_DEADLINE: Duration = Duration::from_secs(4);

/// [`arm_quit_deadline`] for a signal, with [`SIGNAL_DEADLINE`]; called by the thread that receives
/// the signal, so a blocked window thread cannot keep the process alive (`SPEC-0262` v3
/// behaviour 3). Shares the once-only arming with the quit deadline.
pub fn arm_signal_deadline() -> bool {
    arm_deadline(SIGNAL_DEADLINE, &QUIT_ARMED, || end_now(SIGNAL_DEADLINE))
}

/// Ends the programs of run configurations and the process, `after` the quit was decided.
fn end_now(after: Duration) {
    eprintln!(
        "warning: quitting did not finish within {} s; ending the process",
        after.as_secs()
    );
    crate::run::end_all_runs(Duration::from_millis(500));
    std::process::exit(0);
}

/// Runs `end` on its own thread `after` from now, unless `armed` was already set.
fn arm_deadline(
    after: Duration,
    armed: &'static AtomicBool,
    end: impl FnOnce() + Send + 'static,
) -> bool {
    if armed.swap(true, Ordering::SeqCst) {
        return false;
    }
    let spawned = std::thread::Builder::new()
        .name("quit-deadline".into())
        .spawn(move || {
            std::thread::sleep(after);
            end();
        });
    if spawned.is_err() {
        armed.store(false, Ordering::SeqCst);
        return false;
    }
    true
}

/// Writes `contents` to `path` through a temporary file in the same folder and a rename, creating
/// the folder; a reader sees the old or the new content, never a mix (behaviour 2). On failure the
/// old file stays and the temporary file is removed.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    let tmp = path.with_file_name(name);
    let written = std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_ends_once_and_not_before_its_time() {
        static ARMED: AtomicBool = AtomicBool::new(false);
        static ENDED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let start = std::time::Instant::now();
        assert!(arm_deadline(Duration::from_millis(300), &ARMED, || {
            ENDED.fetch_add(1, Ordering::SeqCst);
        }));
        // Armed once: a second quit request does not add a second end.
        assert!(!arm_deadline(Duration::from_millis(10), &ARMED, || {
            ENDED.fetch_add(1, Ordering::SeqCst);
        }));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(ENDED.load(Ordering::SeqCst), 0, "ended before the deadline");
        while ENDED.load(Ordering::SeqCst) == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(start.elapsed() >= Duration::from_millis(300));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(ENDED.load(Ordering::SeqCst), 1);
        assert_eq!(QUIT_DEADLINE, Duration::from_secs(5));
    }

    /// SPEC-0262 AK-5 (v3, behaviour 3): with the half second for the run configurations' programs,
    /// a signal's deadline ends the process within 5 s.
    #[test]
    fn the_signal_deadline_ends_within_five_seconds() {
        assert_eq!(SIGNAL_DEADLINE, Duration::from_secs(4));
        assert!(SIGNAL_DEADLINE + Duration::from_millis(500) < Duration::from_secs(5));
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-atomic-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect()
    }

    #[test]
    fn replaces_content_and_creates_the_folder() {
        let dir = scratch("replace");
        let file = dir.join("nested/state.json");
        write_atomic(&file, "one").unwrap();
        write_atomic(&file, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
        assert!(leftovers(&dir.join("nested")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_keeps_the_old_file() {
        let dir = scratch("fail");
        let file = dir.join("state.json");
        write_atomic(&file, "old").unwrap();
        // A folder in the way of the rename: the write fails after the temporary file exists.
        let blocked = dir.join("blocked");
        std::fs::create_dir_all(blocked.join("inside")).unwrap();
        assert!(write_atomic(&blocked, "new").is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "old");
        assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
