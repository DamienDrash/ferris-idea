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
pub fn load_recent_projects(file: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .take(RECENT_PROJECTS_LIMIT)
        .collect()
}

/// Puts `project` at the front of the list in `file` (creating the folder), keeping at most
/// [`RECENT_PROJECTS_LIMIT`] entries.
pub fn remember_project(file: &Path, project: &Path) -> std::io::Result<()> {
    let mut list = load_recent_projects(file);
    list.retain(|p| p != project);
    list.insert(0, project.to_path_buf());
    list.truncate(RECENT_PROJECTS_LIMIT);
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    crate::write_atomic(file, text)
}
