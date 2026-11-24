//! The Ferris IDEA log folder and its line logs, e.g. the keyboard diagnostics (`SPEC-0270`).

use std::io::Write;
use std::path::{Path, PathBuf};

/// File name of the keyboard diagnostics log in [`log_dir`] (`SPEC-0270` behaviour 3).
pub const KEYBOARD_LOG: &str = "keyboard-diagnostics.log";

/// A log grows to this size, then the older half is kept as `<name>.1` (behaviour 3).
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// The Ferris IDEA log folder: `$FERRIS_IDEA_LOG_DIR`, else `~/Library/Logs/FerrisIDEA` on macOS,
/// `%LOCALAPPDATA%\FerrisIDEA\Logs` on Windows, `$XDG_STATE_HOME/ferris-idea/log` or
/// `~/.local/state/ferris-idea/log` elsewhere.
pub fn log_dir() -> Option<PathBuf> {
    if let Some(d) = crate::product_var_os("FERRIS_IDEA_LOG_DIR") {
        return Some(PathBuf::from(d));
    }
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Logs/FerrisIDEA"));
    }
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("Ferris IDEA\\Logs"));
    }
    if let Some(d) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(d).join("ferris-idea/log"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/ferris-idea/log"))
}

/// The keyboard diagnostics log.
pub fn keyboard_log() -> Option<PathBuf> {
    log_dir().map(|d| d.join(KEYBOARD_LOG))
}

/// Appends `line` to the log `path`, creating its folder; a log over 1 MiB is moved to `<path>.1`
/// first, replacing an older one.
pub fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        std::fs::rename(path, old)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", line.replace('\n', " "))
}

/// `time` as UTC in ISO 8601 with milliseconds, e.g. `2026-11-04T10:15:03.123Z`.
pub fn utc_stamp(time: std::time::SystemTime) -> String {
    let since = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// The last `n` lines of the log `path` (none when it does not exist).
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_utc_times() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(utc_stamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // 2026-11-04T10:15:03.123Z
        let t = UNIX_EPOCH + Duration::from_millis(1_793_787_303_123);
        assert_eq!(utc_stamp(t), "2026-11-04T10:15:03.123Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(utc_stamp(leap), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn appends_rotates_and_reads_the_tail() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = dir.join("sub").join(KEYBOARD_LOG);
        assert!(tail(&log, 3).is_empty());
        for i in 0..5 {
            append_line(&log, &format!("line {i}\nwrapped")).unwrap();
        }
        assert_eq!(
            tail(&log, 2),
            ["line 3 wrapped".to_owned(), "line 4 wrapped".to_owned()]
        );
        std::fs::write(&log, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        append_line(&log, "fresh").unwrap();
        assert_eq!(tail(&log, 5), ["fresh".to_owned()]);
        assert!(dir.join("sub").join(format!("{KEYBOARD_LOG}.1")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
