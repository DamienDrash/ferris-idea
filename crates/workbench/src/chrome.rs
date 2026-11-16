//! Toolkit-free parts of the window chrome (`SPEC-0061`): project widget, branch, breadcrumbs and
//! status bar texts.

use std::path::{Path, PathBuf};

/// Initials of a project name for the project widget: first letters of the first two words (split at
/// `-`, `_`, `.` and spaces), else the first two letters, upper case (behaviour 2).
pub fn project_initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(['-', '_', '.', ' '])
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect(),
        // The first and the last word, as the reference does (`SPEC-0061` v6 behaviour 28,
        // OBS-0020: `cmp-win-ij` reads `CI`).
        [a, .., b] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

/// The checked-out branch of the Git repository at `root`, read from `HEAD` (a worktree's `.git` file
/// is followed); `None` without repository or on a detached head (behaviour 2).
pub fn current_branch(root: &Path) -> Option<String> {
    let git = root.join(".git");
    let dir = if git.is_file() {
        let text = std::fs::read_to_string(&git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        root.join(target)
    } else {
        git
    };
    let head = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

/// `path` with the home folder written as `~` (behaviour 3).
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `path` as a popup's footer shows it: with `~` for the home folder or, in a remote project,
/// as the path on the host (`remote`: the stand-in folder and the host folder it stands for).
pub fn shown_path(path: &Path, home: Option<&Path>, remote: Option<&(PathBuf, String)>) -> String {
    match remote.and_then(|(stand_in, host)| Some((path.strip_prefix(stand_in).ok()?, host))) {
        Some((rest, host)) if rest.as_os_str().is_empty() => host.clone(),
        Some((rest, host)) => format!("{}/{}", host.trim_end_matches('/'), rest.display()),
        None => tilde(path, home),
    }
}

/// The folder shown beside the project name in the tree: the project's own, or in a remote project
/// the folder on the host, never the stand-in (`SPEC-0317` v6 window detail 11).
pub fn root_label(root: &Path, home: Option<&Path>, remote: Option<&(PathBuf, String)>) -> String {
    match remote {
        Some((_, host)) => host.clone(),
        None => tilde(root, home),
    }
}

/// What the navigation bar shows (SPEC-0061 v13 behaviour 60): while a diff tab is the active tab
/// of the focused group, the file it compares (or the project alone when it compares none), not
/// the file tab that was active before it; otherwise the active file. `editor` tells whether the
/// caret's definitions and position belong to the shown file.
pub fn shown_file<'a>(
    diff_front: Option<Option<&'a Path>>,
    active: Option<&'a Path>,
) -> (Option<&'a Path>, bool) {
    match diff_front {
        Some(file) => (file, false),
        None => (active, true),
    }
}

/// Breadcrumbs of the status bar: project name, then the folders and file of `file` below `root`
/// (behaviour 5), then `symbols`, the definitions around the caret (v3 behaviour 5a).
pub fn breadcrumbs(root: &Path, file: Option<&Path>, symbols: &[String]) -> Vec<String> {
    let mut out = vec![
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string()),
    ];
    if let Some(rel) = file.and_then(|f| f.strip_prefix(root).ok()) {
        out.extend(rel.iter().map(|c| c.to_string_lossy().into_owned()));
        out.extend(symbols.iter().cloned());
    }
    out
}

/// Status bar label of an indentation unit: `4 spaces`, `Tab` (behaviour 5).
pub fn indent_label(unit: &str) -> String {
    if unit == "\t" {
        "Tab".into()
    } else {
        format!("{} spaces", unit.chars().count())
    }
}

/// Lines of the empty editor area (`SPEC-0059` v5 behaviour 11): what opens something, and the
/// shortcut of `keymap` beside it ("" without one). With the project tree hidden the project view
/// follows Search Everywhere; the navigation bar is always last (`SPEC-0061` v6 behaviour 29,
/// OBS-0020).
pub fn empty_editor_hints(
    keymap: &idecraft_ui::keymap::Keymap,
    macos: bool,
    tree_shown: bool,
) -> Vec<(&'static str, String)> {
    let mut lines = vec![("Search Everywhere", "SearchEverywhere")];
    if !tree_shown {
        lines.push(("Project View", "ActivateProjectToolWindow"));
    }
    lines.extend([
        ("Go to File", "GotoFile"),
        ("Recent Files", "RecentFiles"),
        ("Navigation Bar", "ShowNavBar"),
    ]);
    lines
        .into_iter()
        .map(|(label, action)| {
            let shortcut = keymap
                .shortcuts(action)
                .first()
                .map(|s| s.display(macos))
                .unwrap_or_default();
            // A double-modifier gesture reads "⇧ ⇧" on macOS (OBS-0020), "Double Shift" elsewhere.
            let shortcut = match shortcut.strip_prefix("double ") {
                Some(key) if macos && key.eq_ignore_ascii_case("shift") => "⇧ ⇧".to_owned(),
                Some(key) => format!("Double {key}"),
                None => shortcut,
            };
            (label, shortcut)
        })
        .collect()
}

/// The line under the hints of the empty editor area.
pub const DROP_HINT: &str = "Drop files here to open them";

/// The files of a drop to open, in order; folders are left out (`SPEC-0059` v5 behaviour 11).
pub fn dropped_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| p.is_file()).cloned().collect()
}

/// One entry of the navigation bar popup (`SPEC-0059` v6 behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavEntry {
    /// Shown text: the name, folders ending in `/`.
    pub label: String,
    /// Full path.
    pub path: PathBuf,
    /// Whether it is a folder.
    pub dir: bool,
    /// Icon key of the project tree: the folder icon or the file type icon (`SPEC-0059` v7
    /// behaviour 14).
    pub icon: &'static str,
}

/// The folders, then the files of `dir`, each sorted by name ignoring case, without the names the
/// project tree hides (`SPEC-0059` v6 behaviour 13).
pub fn nav_entries(dir: &Path) -> Vec<NavEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<NavEntry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if crate::project_tree::is_ignored(&name) {
                return None;
            }
            let dir = e.path().is_dir();
            let icon = if dir {
                "nodes/folder"
            } else {
                idecraft_ui::icons::file_icon(&name)
            };
            Some(NavEntry {
                label: if dir { format!("{name}/") } else { name },
                path: e.path(),
                dir,
                icon,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    entries
}

/// Heading of the navigation bar popup for `dir`: the project folder's name and the folders below
/// it, joined by ` / ` (`SPEC-0059` v6 behaviour 13).
pub fn nav_heading(root: &Path, dir: &Path) -> String {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    let below = dir.strip_prefix(root).unwrap_or(Path::new(""));
    std::iter::once(name)
        .chain(
            below
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0059 v6 AK-9.
    #[test]
    fn navigation_bar_instead_of_project_view_while_the_tree_is_shown() {
        let mac = empty_editor_hints(&idecraft_ui::keymap::Keymap::bundled_macos(), true, true);
        assert_eq!(mac[3], ("Navigation Bar", "⌘↑".to_owned()));
        let default =
            empty_editor_hints(&idecraft_ui::keymap::Keymap::bundled_default(), false, true);
        assert_eq!(default[3], ("Navigation Bar", "Alt+Home".to_owned()));
        let hidden = empty_editor_hints(&idecraft_ui::keymap::Keymap::bundled_macos(), true, false);
        // SPEC-0061 v6 behaviour 29 (OBS-0020): with the tree hidden "Project View" follows
        // Search Everywhere and the navigation bar line stays.
        assert_eq!(hidden[1], ("Project View", "⌘1".to_owned()));
        assert!(hidden.iter().any(|(l, _)| *l == "Navigation Bar"));
    }

    // SPEC-0059 v6 AK-9.
    #[test]
    fn nav_entries_folders_first_without_hidden_names() {
        let dir = std::env::temp_dir().join(format!("idecraft-navbar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/main")).unwrap();
        std::fs::create_dir_all(dir.join("__pycache__")).unwrap();
        std::fs::create_dir_all(dir.join("Docs")).unwrap();
        std::fs::write(dir.join("b.txt"), "").unwrap();
        std::fs::write(dir.join("A.md"), "").unwrap();
        std::fs::write(dir.join("c.pyc"), "").unwrap();
        let labels: Vec<String> = nav_entries(&dir).into_iter().map(|e| e.label).collect();
        assert_eq!(labels, ["Docs/", "src/", "A.md", "b.txt"]);
        let entries = nav_entries(&dir);
        assert!(entries[0].dir && !entries[2].dir);
        assert_eq!(entries[2].path, dir.join("A.md"));
        assert!(nav_heading(&dir, &dir.join("src/main")).ends_with(" / src / main"));
        assert_eq!(nav_heading(&dir, &dir).split(" / ").count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // SPEC-0059 v7 AK-10.
    #[test]
    fn nav_entries_carry_icons() {
        let dir = std::env::temp_dir().join(format!("idecraft-navicons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("README.md"), "").unwrap();
        std::fs::write(dir.join("Edit.java"), "").unwrap();
        let icons: Vec<&str> = nav_entries(&dir).iter().map(|e| e.icon).collect();
        assert_eq!(
            icons,
            [
                "nodes/folder",
                idecraft_ui::icons::file_icon("Edit.java"),
                idecraft_ui::icons::file_icon("README.md")
            ]
        );
        assert_ne!(icons[1], icons[2]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // SPEC-0059 v5 AK-8.
    #[test]
    fn empty_editor_hints_follow_the_keymap() {
        let mac = empty_editor_hints(&idecraft_ui::keymap::Keymap::bundled_macos(), true, false);
        let labels: Vec<&str> = mac.iter().map(|(l, _)| *l).collect();
        assert_eq!(
            labels,
            [
                "Search Everywhere",
                "Project View",
                "Go to File",
                "Recent Files",
                "Navigation Bar"
            ]
        );
        let keys: Vec<&str> = mac.iter().map(|(_, k)| k.as_str()).collect();
        assert_eq!(keys, ["⇧ ⇧", "⌘1", "⇧⌘O", "⌘E", "⌘↑"]);
        let shown = empty_editor_hints(&idecraft_ui::keymap::Keymap::bundled_macos(), true, true);
        assert_eq!(shown.len(), 4, "no project view while the tree is shown");
        let default = empty_editor_hints(
            &idecraft_ui::keymap::Keymap::bundled_default(),
            false,
            false,
        );
        let keys: Vec<&str> = default.iter().map(|(_, k)| k.as_str()).collect();
        assert_eq!(
            keys[..4],
            ["Double Shift", "Alt+1", "Ctrl+Shift+N", "Ctrl+E"]
        );
    }

    // SPEC-0059 v5 AK-8.
    #[test]
    fn dropped_files_open_without_folders() {
        let dir = std::env::temp_dir().join(format!("idecraft-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        let dropped = [dir.join("b.txt"), dir.join("sub"), dir.join("a.txt")];
        assert_eq!(
            dropped_files(&dropped),
            [dir.join("b.txt"), dir.join("a.txt")]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn initials() {
        assert_eq!(project_initials("sql-basic"), "SB");
        assert_eq!(project_initials("idecraft"), "ID");
        assert_eq!(project_initials("my_cool app"), "MA");
        assert_eq!(project_initials("cmp-win-ij"), "CI");
        assert_eq!(project_initials(""), "");
    }

    #[test]
    fn branch_from_head_and_worktree_file() {
        let dir = std::env::temp_dir().join(format!("idecraft-chrome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
        std::fs::write(dir.join("repo/.git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        assert_eq!(current_branch(&dir.join("repo")).as_deref(), Some("feat/x"));
        std::fs::create_dir_all(dir.join("wt-git")).unwrap();
        std::fs::write(dir.join("wt-git/HEAD"), "0123456789abcdef\n").unwrap();
        std::fs::create_dir_all(dir.join("wt")).unwrap();
        std::fs::write(dir.join("wt/.git"), "gitdir: ../wt-git\n").unwrap();
        assert_eq!(current_branch(&dir.join("wt")), None, "detached head");
        assert_eq!(current_branch(&dir), None, "no repository");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn paths_and_labels() {
        let home = PathBuf::from("/Users/d");
        assert_eq!(tilde(Path::new("/Users/d/p/x"), Some(&home)), "~/p/x");
        assert_eq!(tilde(Path::new("/opt/x"), Some(&home)), "/opt/x");
        // The project tree's root row names the host folder, whatever the stand-in is called.
        let stand_in = (PathBuf::from("/private/tmp/x/app"), "/srv/app".to_owned());
        assert_eq!(
            root_label(
                Path::new("/private/tmp/x/app"),
                Some(&home),
                Some(&stand_in)
            ),
            "/srv/app"
        );
        assert_eq!(
            root_label(Path::new("/Users/d/p"), Some(&home), None),
            "~/p"
        );
        // Copy Path of a remote project gives the host path (no `~`, no stand-in; SPEC-0317 v6).
        assert_eq!(
            shown_path(
                Path::new("/private/tmp/x/app/a/b.py"),
                None,
                Some(&stand_in)
            ),
            "/srv/app/a/b.py"
        );
        // A remote project shows the host's path, never its stand-in folder (SPEC-0317 v5).
        let remote = (PathBuf::from("/Users/d/state/p"), "/srv/app/".to_owned());
        let shown = |p: &str| shown_path(Path::new(p), Some(&home), Some(&remote));
        assert_eq!(shown("/Users/d/state/p/src/a.rs"), "/srv/app/src/a.rs");
        assert_eq!(shown("/Users/d/state/p"), "/srv/app/");
        assert_eq!(shown("/Users/d/p/x"), "~/p/x");
        assert_eq!(
            breadcrumbs(
                Path::new("/p/proj"),
                Some(Path::new("/p/proj/src/a.rs")),
                &[]
            ),
            ["proj", "src", "a.rs"]
        );
        // SPEC-0061 v3 AK-1a: the definitions around the caret follow the file.
        let symbols = ["Inventory".to_owned(), "main()".to_owned()];
        assert_eq!(
            breadcrumbs(
                Path::new("/p/proj"),
                Some(Path::new("/p/proj/Inventory.java")),
                &symbols
            ),
            ["proj", "Inventory.java", "Inventory", "main()"]
        );
        assert_eq!(breadcrumbs(Path::new("/p/proj"), None, &symbols), ["proj"]);
        // SPEC-0061 v13 AK-v13b: a diff tab in front shows its own file, not the tab before it.
        let (a, d) = (Path::new("/p/proj/a.rs"), Path::new("/p/proj/src/d.rs"));
        assert_eq!(shown_file(None, Some(a)), (Some(a), true));
        assert_eq!(shown_file(Some(Some(d)), Some(a)), (Some(d), false));
        assert_eq!(shown_file(Some(None), Some(a)), (None, false));
        assert_eq!(indent_label("    "), "4 spaces");
        assert_eq!(indent_label("\t"), "Tab");
    }
}
