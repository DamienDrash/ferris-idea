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

/// The remembered folders in `file`, most recent first; folders that no longer exist are left out.
/// Each folder is named by its canonical path, and different spellings of one folder (a trailing
/// `/`, `..`, a link such as `/tmp` for `/private/tmp`) count once, at the place of the most
/// recent (`SPEC-0094` v8 behaviour 22).
pub fn load_recent_projects(file: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = Vec::new();
    for line in std::fs::read_to_string(file).unwrap_or_default().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(dir) = std::fs::canonicalize(line) else {
            continue;
        };
        if dir.is_dir() && !list.contains(&dir) {
            list.push(dir);
        }
        if list.len() == RECENT_PROJECTS_LIMIT {
            break;
        }
    }
    list
}

/// Puts `project` at the front of the list in `file` (creating the folder), keeping at most
/// [`RECENT_PROJECTS_LIMIT`] entries; the project is kept by its canonical path.
pub fn remember_project(file: &Path, project: &Path) -> std::io::Result<()> {
    let project = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let mut list = load_recent_projects(file);
    list.retain(|p| *p != project);
    list.insert(0, project);
    list.truncate(RECENT_PROJECTS_LIMIT);
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    crate::write_atomic(file, text)
}
