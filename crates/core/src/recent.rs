//! Recently opened projects (`SPEC-0094`): one folder per line, most recent first, in the user's
//! settings folder.

use std::path::{Path, PathBuf};

use crate::user_config_dir;

/// Most projects remembered (behaviour 3).
pub const RECENT_PROJECTS_LIMIT: usize = 25;

/// Where the list is kept: `recent-projects` in [`user_config_dir`].
pub fn recent_projects_file() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join("recent-projects"))
}

/// One line of the list as the Welcome window shows it (`SPEC-0094` v10 behaviour 3,
/// `SPEC-0355` behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    /// The folder: its canonical path while it exists, else as the line names it.
    pub path: PathBuf,
    /// Whether the folder exists now.
    pub exists: bool,
}

/// Every entry of `file`, most recent first, folders that no longer exist included (marked), at
/// most [`RECENT_PROJECTS_LIMIT`]. An existing folder is named by its canonical path, and
/// different spellings of one folder (a trailing `/`, `..`, a link such as `/tmp` for
/// `/private/tmp`) count once, at the place of the most recent (`SPEC-0094` v8 behaviour 22).
pub fn load_recent_entries(file: &Path) -> Vec<RecentEntry> {
    let mut list: Vec<RecentEntry> = Vec::new();
    for line in std::fs::read_to_string(file).unwrap_or_default().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry = match std::fs::canonicalize(line) {
            Ok(dir) if dir.is_dir() => RecentEntry {
                path: dir,
                exists: true,
            },
            // A file where the folder was is not a project either.
            Ok(other) => RecentEntry {
                path: other,
                exists: false,
            },
            Err(_) => RecentEntry {
                path: written(line),
                exists: false,
            },
        };
        if !list.iter().any(|e| e.path == entry.path) {
            list.push(entry);
        }
        if list.len() == RECENT_PROJECTS_LIMIT {
            break;
        }
    }
    list
}

/// A line naming a folder that is gone, without a trailing `/` (`SPEC-0094` v8 behaviour 22).
fn written(line: &str) -> PathBuf {
    let trimmed = line.trim_end_matches('/');
    PathBuf::from(if trimmed.is_empty() { line } else { trimmed })
}

/// The remembered folders in `file`, most recent first; folders that no longer exist are left out
/// (they stay in the file for the Welcome window, `SPEC-0094` v10 behaviour 3). Each folder is
/// named by its canonical path, and different spellings of one folder count once, at the place of
/// the most recent (v8 behaviour 22).
pub fn load_recent_projects(file: &Path) -> Vec<PathBuf> {
    load_recent_entries(file)
        .into_iter()
        .filter(|e| e.exists)
        .map(|e| e.path)
        .collect()
}

/// Writes `list` to `file`, one folder per line.
fn write_list(file: &Path, list: &[RecentEntry]) -> std::io::Result<()> {
    let text: String = list
        .iter()
        .map(|e| format!("{}\n", e.path.display()))
        .collect();
    crate::write_atomic(file, text)
}

/// Takes the folder `project` (any spelling) out of the list in `file`; whether it was there
/// (`SPEC-0355` behaviour 9). The folder itself is never touched.
pub fn forget_project(file: &Path, project: &Path) -> std::io::Result<bool> {
    let wanted =
        std::fs::canonicalize(project).unwrap_or_else(|_| written(&project.display().to_string()));
    let mut list = load_recent_entries(file);
    let before = list.len();
    list.retain(|e| e.path != wanted);
    if list.len() == before {
        return Ok(false);
    }
    write_list(file, &list)?;
    Ok(true)
}

/// Puts `project` at the front of the list in `file` (creating the folder), keeping at most
/// [`RECENT_PROJECTS_LIMIT`] entries; the project is kept by its canonical path. Entries whose
/// folder is gone are kept until the user removes them (`SPEC-0094` v10 behaviour 3).
pub fn remember_project(file: &Path, project: &Path) -> std::io::Result<()> {
    let project = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let mut list = load_recent_entries(file);
    list.retain(|e| e.path != project);
    let exists = project.is_dir();
    list.insert(
        0,
        RecentEntry {
            path: project,
            exists,
        },
    );
    list.truncate(RECENT_PROJECTS_LIMIT);
    write_list(file, &list)
}
