//! Terminal tabs kept between runs (`SPEC-0205` behaviours 1 and 6).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File under the project folder.
pub const SAVED_FILE: &str = ".ferris-idea/terminal-sessions.json";
/// History lines kept per shell.
pub const SAVED_LINES: usize = 1000;

/// The terminal tabs of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTerminals {
    /// Tabs left to right.
    pub tabs: Vec<SavedTab>,
}

/// One tab: its name and its shells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    /// Name as shown (renamed or numbered).
    pub name: String,
    /// The shells are side by side (true) or one above the other.
    #[serde(default)]
    pub side_by_side: bool,
    /// Shells in order.
    pub shells: Vec<SavedShell>,
}

/// One shell: its folder and its last lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedShell {
    /// Current folder when saved.
    pub cwd: PathBuf,
    /// Last history lines, oldest first.
    #[serde(default)]
    pub lines: Vec<String>,
    /// Cell of the tab the shell sits in ([`crate::cell_runs`]); missing in files written before
    /// shells could be stacked, where every shell had its own cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<usize>,
}

impl SavedShell {
    /// A shell with at most [`SAVED_LINES`] of `lines` (the last ones).
    pub fn new(cwd: PathBuf, mut lines: Vec<String>) -> Self {
        let skip = lines.len().saturating_sub(SAVED_LINES);
        lines.drain(..skip);
        Self {
            cwd,
            lines,
            group: None,
        }
    }

    /// The folder to start in: the saved one while it exists, else `root`.
    pub fn start_dir(&self, root: &Path) -> PathBuf {
        if self.cwd.is_dir() {
            self.cwd.clone()
        } else {
            root.to_path_buf()
        }
    }
}

impl SavedTerminals {
    /// The saved tabs of the project at `root`; empty when there are none or the file is unreadable.
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(SAVED_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Writes the tabs; no tabs remove the file.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = root.join(SAVED_FILE);
        if self.tabs.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&path, text + "\n")
    }
}

/// Name for a new tab: "Local", then "Local (2)", "Local (3)", … with the lowest number no tab
/// has (`SPEC-0205` behaviour 1).
pub fn next_tab_name(names: &[String]) -> String {
    let name = |n: usize| {
        if n == 1 {
            "Local".to_owned()
        } else {
            format!("Local ({n})")
        }
    };
    (1..)
        .map(name)
        .find(|candidate| !names.contains(candidate))
        .unwrap_or_default()
}

/// Writes `contents` through a temporary file and a rename, so the file is never half written
/// (`SPEC-0262` behaviour 2).
fn write_atomic(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
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
    fn names_take_the_lowest_free_number() {
        let names = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(next_tab_name(&[]), "Local");
        assert_eq!(next_tab_name(&names(&["Local"])), "Local (2)");
        assert_eq!(next_tab_name(&names(&["Local", "Local (3)"])), "Local (2)");
        assert_eq!(next_tab_name(&names(&["Local (2)", "build"])), "Local");
    }

    #[test]
    fn saved_tabs_round_trip_with_a_line_cap() {
        let root = std::env::temp_dir().join(format!("ferris-idea-saved-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let lines: Vec<String> = (0..1500).map(|i| i.to_string()).collect();
        let shell = SavedShell::new(root.join("gone"), lines);
        assert_eq!(shell.lines.len(), SAVED_LINES);
        assert_eq!(shell.lines[0], "500");
        assert_eq!(shell.start_dir(&root), root);
        let saved = SavedTerminals {
            tabs: vec![SavedTab {
                name: "build".into(),
                side_by_side: true,
                shells: vec![shell, SavedShell::new(root.clone(), Vec::new())],
            }],
        };
        saved.save(&root).unwrap();
        assert_eq!(SavedTerminals::load(&root), saved);
        assert_eq!(saved.tabs[0].shells[1].start_dir(&root), root);
        SavedTerminals::default().save(&root).unwrap();
        assert!(!root.join(SAVED_FILE).exists());
        assert_eq!(SavedTerminals::load(&root), SavedTerminals::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
