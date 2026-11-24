//! The rows of the project popup and its two forms, toolkit-free (`SPEC-0094` v5 behaviours 1,
//! 12 to 14).

use std::path::{Path, PathBuf};

/// One row of the project popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupRow {
    /// New Project…: an empty folder, optionally with `git init`.
    NewProject,
    /// Open…: the system's folder dialog.
    Open,
    /// Clone Repository…: address, folder, progress.
    Clone,
    /// Remote Development… (`SPEC-0317` v3 behaviour 1).
    RemoteDevelopment,
    /// A project open in a window of this Ferris IDEA; choosing it brings that window to the front.
    OpenProject {
        /// The project folder.
        root: PathBuf,
        /// Whether it is the window the popup was opened in.
        current: bool,
    },
    /// A recently opened project that is not open now.
    Recent(PathBuf),
}

impl PopupRow {
    /// The text of an action row; `None` for a project row.
    pub fn title(&self) -> Option<&'static str> {
        match self {
            PopupRow::NewProject => Some("New Project…"),
            PopupRow::Open => Some("Open…"),
            PopupRow::Clone => Some("Clone Repository…"),
            PopupRow::RemoteDevelopment => Some("Remote Development…"),
            _ => None,
        }
    }
}

/// The rows: the four actions, the projects open in windows (in opening order, the current one
/// marked), then the recent projects that are not open (behaviours 1 and 12).
pub fn popup_rows(open: &[PathBuf], current: &Path, recent: &[PathBuf]) -> Vec<PopupRow> {
    let mut rows = vec![
        PopupRow::NewProject,
        PopupRow::Open,
        PopupRow::Clone,
        PopupRow::RemoteDevelopment,
    ];
    rows.extend(open.iter().map(|root| PopupRow::OpenProject {
        root: root.clone(),
        current: root == current,
    }));
    rows.extend(
        recent
            .iter()
            .filter(|p| p.as_path() != current && !open.contains(p))
            .map(|p| PopupRow::Recent(p.clone())),
    );
    rows
}

/// What the Clone Repository form would do: the checked address and the new folder `location`
/// / the repository's name (behaviour 13).
pub fn clone_plan(
    url: &str,
    location: &str,
) -> Result<(ferris_idea_vcs::CloneSource, PathBuf), String> {
    let source = ferris_idea_vcs::check_clone_url(url)?;
    let target = expand_home(location.trim()).join(&source.folder_name);
    ferris_idea_vcs::check_new_folder(&target)?;
    Ok((source, target))
}

/// What the New Project form would make: the folder `location` / `name` (behaviour 14).
pub fn new_project_plan(location: &str, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter the project's name.".into());
    }
    if name.contains(['/', '\\']) || name == "." || name == ".." || name.starts_with('-') {
        return Err("The name must be one folder name.".into());
    }
    let target = expand_home(location.trim()).join(name);
    ferris_idea_vcs::check_new_folder(&target)?;
    Ok(target)
}

/// Trusts the project New Project just made at `target` (window check A16, NP2): Ferris IDEA created
/// the folder a moment ago and it holds nothing but what `git init` wrote, so there is no code of
/// anyone else to guard against. `file` is the trust file (`None`: trust is not enforced, nothing
/// is written). A folder with anything else in it is left to the trust question; clones always
/// are (`SPEC-0278`).
pub fn trust_new_project(file: Option<&Path>, target: &Path) -> Result<bool, String> {
    let Some(file) = file else {
        return Ok(false);
    };
    let only_git = std::fs::read_dir(target)
        .map_err(|e| e.to_string())?
        .flatten()
        .all(|e| e.file_name() == ".git");
    if !only_git {
        return Ok(false);
    }
    ferris_idea_core::trust::add_location(
        file,
        target,
        ferris_idea_core::trust::TrustKind::Project,
    )?;
    Ok(true)
}

/// The trust file New Project writes to: the user's while trust is enforced (the application).
pub fn new_project_trust_file() -> Option<PathBuf> {
    ferris_idea_core::trust::enforced()
        .then(ferris_idea_core::trust::trust_file)
        .flatten()
}

/// The line under a form while it names no folder: the rule of its first field (window check
/// A16, NP1: New Project showed the clone's hint).
pub fn form_hint(clone: bool) -> &'static str {
    if clone {
        "Only HTTPS and SSH; no password or token in the address."
    } else {
        "One folder name; the new folder goes into the Location, which must exist."
    }
}

/// `~` and `~/…` in the home folder.
fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (path, home) {
        ("~", Some(h)) => h,
        (p, Some(h)) if p.starts_with("~/") => h.join(&p[2..]),
        (p, _) => PathBuf::from(p),
    }
}

/// The folder the forms suggest: the one the current project lies in.
pub fn default_location(current: &Path) -> String {
    current
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// The text of a clone's progress (behaviour 13).
pub fn progress_text(name: &str, progress: Option<&ferris_idea_vcs::CloneProgress>) -> String {
    match progress {
        Some(p) => match p.percent {
            Some(n) => format!("Cloning {name}: {} {n}%", p.phase),
            None => format!("Cloning {name}: {}", p.phase),
        },
        None => format!("Cloning {name}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    /// SPEC-0094 v5 AK-10: actions first, then the open projects with the current one marked,
    /// then the recent projects that are not open.
    #[test]
    fn open_projects_come_before_the_recent_ones() {
        let open = paths(&["/p/a", "/p/b"]);
        let recent = paths(&["/p/b", "/p/c", "/p/a", "/p/d"]);
        let rows = popup_rows(&open, Path::new("/p/b"), &recent);
        assert_eq!(
            rows.iter().filter_map(PopupRow::title).collect::<Vec<_>>(),
            [
                "New Project…",
                "Open…",
                "Clone Repository…",
                "Remote Development…"
            ]
        );
        assert_eq!(
            rows[4..],
            [
                PopupRow::OpenProject {
                    root: "/p/a".into(),
                    current: false
                },
                PopupRow::OpenProject {
                    root: "/p/b".into(),
                    current: true
                },
                PopupRow::Recent("/p/c".into()),
                PopupRow::Recent("/p/d".into()),
            ]
        );
    }

    /// SPEC-0094 v5 AK-11: the forms name the new folder or the reason it cannot be made.
    #[test]
    fn the_forms_name_the_new_folder_or_the_reason() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-popup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("taken")).unwrap();
        let loc = dir.display().to_string();
        let (source, target) = clone_plan("https://example.org/acme/shop.git", &loc).unwrap();
        assert_eq!(source.folder_name, "shop");
        assert_eq!(target, dir.join("shop"));
        assert!(
            clone_plan("https://example.org/acme/taken.git", &loc)
                .unwrap_err()
                .contains("exists already")
        );
        assert!(
            clone_plan("https://u:p@example.org/acme/x.git", &loc)
                .unwrap_err()
                .contains("password")
        );
        assert_eq!(new_project_plan(&loc, "demo").unwrap(), dir.join("demo"));
        assert!(new_project_plan(&loc, "").is_err());
        assert!(new_project_plan(&loc, "a/b").is_err());
        assert!(new_project_plan(&loc, "taken").is_err());
        assert_eq!(default_location(&dir.join("x")), loc);
        let p = ferris_idea_vcs::CloneProgress {
            phase: "Receiving objects".into(),
            percent: Some(45),
        };
        assert_eq!(
            progress_text("shop", Some(&p)),
            "Cloning shop: Receiving objects 45%"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0094 v10 AK-22 (window check A16, NP1 and NP2): each form has its own hint; a folder
    /// New Project just made with `git init` is trusted, one with other files is not, and nothing
    /// is written while trust is not enforced.
    #[test]
    fn new_projects_are_trusted_and_hints_belong_to_their_form() {
        assert!(form_hint(true).contains("HTTPS"));
        assert!(!form_hint(false).contains("HTTPS"));
        let dir = std::env::temp_dir().join(format!("ferris-idea-np-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("fresh/.git")).unwrap();
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(dir.join("other/build.sh"), "echo hi\n").unwrap();
        let file = dir.join("trusted.toml");
        assert!(!trust_new_project(None, &dir.join("fresh")).unwrap());
        assert!(
            !file.exists(),
            "nothing written while trust is not enforced"
        );
        assert!(trust_new_project(Some(&file), &dir.join("fresh")).unwrap());
        assert!(!trust_new_project(Some(&file), &dir.join("other")).unwrap());
        let listed = ferris_idea_core::trust::load_locations(&file).unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
