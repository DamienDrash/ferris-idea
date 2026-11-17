//! The last test runs of each configuration (`SPEC-0088` v3 behaviour 9, OBS-0034 part 15 R3): at
//! most [`HISTORY_LENGTH`] per project and configuration, newest first, in the user's settings
//! folder (`<settings>/test-history`), never in the project. Only the tree is kept — names,
//! outcomes and durations; messages, stack traces, output, the command and its environment are
//! not, since they can hold secrets.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::testreport::{TestReport, TestSuite};

/// Runs kept per configuration.
pub const HISTORY_LENGTH: usize = 10;

/// One remembered run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// When it finished, seconds since the epoch.
    pub time: u64,
    /// The configuration's name.
    pub config: String,
    /// The tree: names, outcomes and durations only.
    pub report: TestReport,
}

/// The folder of the histories below the settings folder `settings`.
pub fn history_dir(settings: &Path) -> PathBuf {
    settings.join("test-history")
}

/// The file of `config`'s history in the project at `root`: named by a hash of both, so neither
/// the project's path nor the name appears in the file name.
pub fn history_file(dir: &Path, root: &Path, config: &str) -> PathBuf {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut h = Sha256::new();
    h.update(root.to_string_lossy().as_bytes());
    h.update([0]);
    h.update(config.as_bytes());
    let hex: String = h.finalize()[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    dir.join(format!("{hex}.json"))
}

/// `report` without what can hold secrets: messages, details and output of tests and suites.
pub fn tree_only(report: &TestReport) -> TestReport {
    fn strip(s: &TestSuite) -> TestSuite {
        TestSuite {
            name: s.name.clone(),
            duration: s.duration,
            tests: s
                .tests
                .iter()
                .map(|t| {
                    let mut t = t.clone();
                    t.message = None;
                    t.details = None;
                    t.output = None;
                    t
                })
                .collect(),
            suites: s.suites.iter().map(strip).collect(),
            output: None,
            totals: s.totals,
            // The tree only: no paths (SPEC-0088 v3 behaviour 9).
            file: None,
        }
    }
    TestReport {
        format: report.format,
        suites: report.suites.iter().map(strip).collect(),
        totals: report.totals,
    }
}

/// The remembered runs of `config` in the project at `root`, newest first; none when the file is
/// missing or unreadable.
pub fn load_history(dir: &Path, root: &Path, config: &str) -> Vec<HistoryEntry> {
    std::fs::read_to_string(history_file(dir, root, config))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<HistoryEntry>>(&t).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.config == config)
        .collect()
}

/// Remembers a finished run of `config` at `time`; the oldest beyond [`HISTORY_LENGTH`] go. The
/// file is written atomically and readable by the user only. Returns the history after it.
pub fn record_history(
    dir: &Path,
    root: &Path,
    config: &str,
    report: &TestReport,
    time: u64,
) -> std::io::Result<Vec<HistoryEntry>> {
    let mut all = load_history(dir, root, config);
    all.insert(
        0,
        HistoryEntry {
            time,
            config: config.to_owned(),
            report: tree_only(report),
        },
    );
    all.truncate(HISTORY_LENGTH);
    std::fs::create_dir_all(dir)?;
    // The folder is the user's only, so the file is never readable by others, also not between
    // writing and renaming it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let file = history_file(dir, root, config);
    let text = serde_json::to_string(&all).map_err(std::io::Error::other)?;
    crate::lifecycle::write_atomic(&file, text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testreport::{ReportFormat, TestCase, TestStatus, Totals};

    fn report(failed: bool) -> TestReport {
        TestReport {
            format: ReportFormat::Junit,
            suites: vec![TestSuite {
                name: "test_prices".into(),
                duration: Some(0.2),
                tests: vec![TestCase {
                    name: "test_total".into(),
                    class: Some("test_prices".into()),
                    status: if failed {
                        TestStatus::Failed
                    } else {
                        TestStatus::Passed
                    },
                    duration: Some(0.1),
                    message: Some("token=s3cr3t".into()),
                    details: Some("Traceback".into()),
                    output: Some("password: hunter2".into()),
                }],
                suites: Vec::new(),
                output: Some("env".into()),
                totals: Totals::default(),
                file: Some("/work/test_prices.py".into()),
            }],
            totals: Totals::default(),
        }
    }

    /// SPEC-0088 v3 behaviour 9, AK-6: ten runs per configuration, newest first, only the tree,
    /// in a file only the user reads.
    #[test]
    fn the_last_ten_runs_are_kept_without_output() {
        let base = std::env::temp_dir().join(format!("idecraft-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, root) = (base.join("settings/test-history"), base.join("project"));
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..12 {
            record_history(&dir, &root, "test_prices.py", &report(i % 2 == 0), 1000 + i).unwrap();
        }
        let all = load_history(&dir, &root, "test_prices.py");
        assert_eq!(all.len(), HISTORY_LENGTH);
        assert_eq!(all[0].time, 1011);
        assert_eq!(all[9].time, 1002);
        let file = history_file(&dir, &root, "test_prices.py");
        let text = std::fs::read_to_string(&file).unwrap();
        for secret in ["s3cr3t", "hunter2", "Traceback", "env\"", "/work/"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        assert!(text.contains("test_total"));
        // Neither the project's path nor the name is in the file name.
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.contains("test_prices") && !name.contains("project"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Another configuration has its own; nothing yet for one never run.
        record_history(&dir, &root, "other", &report(false), 5).unwrap();
        assert_eq!(load_history(&dir, &root, "other").len(), 1);
        assert!(load_history(&dir, &root, "never").is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
