//! The Welcome window's content, toolkit-free (`SPEC-0355`): its sections, the rows of the
//! recent projects with their missing mark, the search, the keyboard steps, Learn's entries and
//! the gear menu.

use std::path::{Path, PathBuf};

use ferris_idea_core::RecentEntry;

/// The window's size when it opens, in points (behaviour 5).
pub const WINDOW_SIZE: (f32, f32) = (800.0, 650.0);

/// The smallest size it can be made (behaviour 5).
pub const MIN_SIZE: (f32, f32) = (640.0, 480.0);

/// The window's title (behaviour 5).
pub const TITLE: &str = "Welcome to Ferris IDEA";

/// A section of the column on the left (behaviour 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Recent projects and the ways to get one (behaviours 7 to 11).
    Projects,
    /// Remote Development › SSH (behaviour 12, v3).
    Ssh,
    /// Theme, editor, keymap, settings (behaviours 14 to 17).
    Customize,
    /// The installed plugins (behaviour 18).
    Plugins,
    /// Ferris IDEA's own material (behaviour 19).
    Learn,
}

impl Section {
    /// Every section, in the column's order; ⌘1 to ⌘5 choose them (behaviour 11, v3).
    pub const ALL: [Section; 5] = [
        Section::Projects,
        Section::Ssh,
        Section::Customize,
        Section::Plugins,
        Section::Learn,
    ];

    /// The section's name in the column.
    pub fn title(self) -> &'static str {
        match self {
            Section::Projects => "Projects",
            Section::Ssh => "SSH",
            Section::Customize => "Customize",
            Section::Plugins => "Plugins",
            Section::Learn => "Learn",
        }
    }

    /// The bundled icon beside the name.
    pub fn icon(self) -> &'static str {
        match self {
            Section::Projects => "nodes/folder",
            Section::Ssh => "ferris-idea/remote",
            Section::Customize => "fileTypes/config",
            Section::Plugins => "nodes/module",
            Section::Learn => "status/info",
        }
    }

    /// The group heading the section stands under in the column: SSH under "Remote Development"
    /// (behaviour 5, v3).
    pub fn group(self) -> Option<&'static str> {
        (self == Section::Ssh).then_some("Remote Development")
    }

    /// The section ⌘ and a digit chooses: ⌘1 is the first (behaviour 11).
    pub fn of_digit(key: &str) -> Option<Section> {
        let n: usize = key.parse().ok()?;
        Section::ALL.get(n.checked_sub(1)?).copied()
    }
}

/// One row of the recent projects (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    /// The project folder.
    pub path: PathBuf,
    /// The folder's name.
    pub name: String,
    /// The initials in the square (`SPEC-0061` rule).
    pub initials: String,
    /// The folder with `~` for the home folder.
    pub shown: String,
    /// The branch checked out, if it is a Git repository with a branch.
    pub branch: Option<String>,
    /// The folder no longer exists (behaviour 9).
    pub missing: bool,
}

/// `path` as it fits into `width` points at `char_width` points per character (`SPEC-0355` v4
/// behaviour 24): whole when it fits, else shortened in the middle at folder boundaries — its
/// start and as many of its last folders as fit, joined by `…` (`/private/tmp/…/night/web`). The
/// folder's own name always stays; only a name too long by itself is cut in its middle.
pub fn middle_path(path: &str, width: f32, char_width: f32) -> String {
    let max = (width / char_width.max(1.)).floor().max(1.) as usize;
    let len = |s: &str| s.chars().count();
    if len(path) <= max {
        return path.to_owned();
    }
    let parts: Vec<&str> = path.split('/').collect();
    // The first part with its leading slash: "/private", "~".
    let (head_end, first) = match parts.as_slice() {
        ["", second, ..] => (2, format!("/{second}")),
        [first, ..] => (1, (*first).to_owned()),
        [] => (0, String::new()),
    };
    let join = |head: usize, tail: usize| -> String {
        let mut text = first.clone();
        for p in &parts[head_end..head] {
            text.push('/');
            text.push_str(p);
        }
        text.push_str("/…");
        for p in &parts[parts.len() - tail..] {
            text.push('/');
            text.push_str(p);
        }
        text
    };
    // Something must be left out for the `…` to stand in for it.
    if parts.len() > head_end + 1 {
        let (mut head, mut tail) = (head_end, 1);
        if len(&join(head, tail)) <= max {
            // Grow the end first, then the start, while it fits and leaves something out.
            loop {
                let grown_tail = head + tail + 1 < parts.len() && len(&join(head, tail + 1)) <= max;
                if grown_tail {
                    tail += 1;
                    continue;
                }
                let grown_head = head + tail + 1 < parts.len() && len(&join(head + 1, tail)) <= max;
                if grown_head {
                    head += 1;
                    continue;
                }
                return join(head, tail);
            }
        }
    }
    // Only the folder's name: whole, else cut in its middle.
    let name = parts.last().copied().unwrap_or(path);
    let short = format!("…/{name}");
    if len(&short) <= max {
        return short;
    }
    let chars: Vec<char> = name.chars().collect();
    let keep = max.saturating_sub(1);
    let front = keep.div_ceil(2);
    let back = keep - front;
    let mut text: String = chars[..front.min(chars.len())].iter().collect();
    text.push('…');
    text.extend(&chars[chars.len().saturating_sub(back)..]);
    text
}

/// The rows of `entries`, most recent first, as the list shows them; `home` is the user's home
/// folder.
pub fn project_rows(entries: &[RecentEntry], home: Option<&Path>) -> Vec<ProjectRow> {
    entries
        .iter()
        .map(|e| {
            let name = e
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| e.path.display().to_string());
            ProjectRow {
                initials: crate::chrome::project_initials(&name),
                shown: crate::chrome::tilde(&e.path, home),
                branch: if e.exists {
                    crate::chrome::current_branch(&e.path)
                } else {
                    None
                },
                missing: !e.exists,
                path: e.path.clone(),
                name,
            }
        })
        .collect()
}

/// The indexes of the rows the search `query` leaves, in order: those whose name or shown folder
/// contain it, ignoring case; every row for an empty query (behaviour 10).
pub fn filter(rows: &[ProjectRow], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, r)| {
            q.is_empty()
                || r.name.to_lowercase().contains(&q)
                || r.shown.to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The selection after ↓ (`down`) or ↑ in a list of `n` rows; it wraps at both ends
/// (behaviour 11).
pub fn step(selected: usize, n: usize, down: bool) -> usize {
    match n {
        0 => 0,
        _ if down => (selected + 1) % n,
        _ => (selected + n - 1) % n,
    }
}

/// What opening a row does (behaviours 8 and 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRow {
    /// Open this folder.
    Open(PathBuf),
    /// The folder is gone: say so.
    Missing(String),
}

/// What opening `row` does.
pub fn open_row(row: &ProjectRow) -> OpenRow {
    if row.missing {
        OpenRow::Missing(format!(
            "{} no longer exists at {}; remove it from the list or restore the folder.",
            row.name, row.shown
        ))
    } else {
        OpenRow::Open(row.path.clone())
    }
}

/// The folder New Project and Clone Repository suggest: the one that holds the most recent
/// existing project, else the home folder (behaviour 7).
pub fn default_location(entries: &[RecentEntry], home: Option<&Path>) -> String {
    entries
        .iter()
        .find(|e| e.exists)
        .and_then(|e| e.path.parent())
        .or(home)
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// Where a Learn entry leads (behaviour 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnTarget {
    /// Settings › Keymap, inside Ferris IDEA.
    Keymap,
    /// This address in the system's browser, only on the user's click.
    Address(&'static str),
}

/// Ferris IDEA's public repository, where its documentation lives.
pub const REPOSITORY: &str = "https://github.com/DamienDrash/ferris-idea";

/// Learn's entries: title, what it is, where it leads. Nothing is fetched to list them.
pub const LEARN: [(&str, &str, LearnTarget); 4] = [
    (
        "Keyboard Shortcuts",
        "Every action and its keys, in Settings › Keymap",
        LearnTarget::Keymap,
    ),
    (
        "Documentation",
        "The guides in Ferris IDEA's repository",
        LearnTarget::Address("https://github.com/DamienDrash/ferris-idea/tree/main/docs"),
    ),
    (
        "Release Notes",
        "What changed in each version",
        LearnTarget::Address("https://github.com/DamienDrash/ferris-idea/blob/main/CHANGELOG.md"),
    ),
    (
        "Report a Problem",
        "Open an issue; nothing is sent until you submit it",
        LearnTarget::Address("https://github.com/DamienDrash/ferris-idea/issues"),
    ),
];

/// An entry of the gear menu (behaviour 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GearItem {
    /// The Settings dialog.
    AllSettings,
    /// An exchange file's import preview.
    ImportSettings,
    /// Settings › Keymap.
    Keymap,
    /// Settings › Plugins.
    Plugins,
}

impl GearItem {
    /// The menu's entries in order.
    pub const ALL: [GearItem; 4] = [
        GearItem::AllSettings,
        GearItem::ImportSettings,
        GearItem::Keymap,
        GearItem::Plugins,
    ];

    /// The entry's text.
    pub fn title(self) -> &'static str {
        match self {
            GearItem::AllSettings => "All Settings…",
            GearItem::ImportSettings => "Import Settings…",
            GearItem::Keymap => "Keymap…",
            GearItem::Plugins => "Plugins…",
        }
    }
}

/// The kinds of exchange file the Welcome window imports: the user's own settings; the others
/// carry project data (behaviour 17).
pub const IMPORT_KINDS: [&str; 3] = ["settings", "keymap", "theme"];

/// Why the Welcome window does not import a file of `kind`, or `None` when it does.
pub fn import_refusal(kind: Option<&str>) -> Option<String> {
    match kind {
        Some(k) if IMPORT_KINDS.contains(&k) => None,
        Some(k) => Some(format!(
            "A `{k}` file belongs to a project; open the project and import it there."
        )),
        None => Some("This is not a Ferris IDEA exchange file; nothing was imported.".into()),
    }
}

/// The name of the keymap in use (behaviour 16).
pub fn keymap_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Default"
    }
}

/// The entries of the recent list as `welcome` `list_recent` answers them (behaviour 22).
pub fn report(rows: &[ProjectRow]) -> serde_json::Value {
    serde_json::Value::Array(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path, "name": r.name, "exists": !r.missing, "branch": r.branch,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0355 v4 AK-9 (behaviour 24, OBS-0102 W2): a long path is shortened in its middle at
    /// folder boundaries; the folder's name stays, so projects in the same long folder differ.
    #[test]
    fn long_paths_are_shortened_in_the_middle() {
        let path = "/private/tmp/claude-1002/scratch/session-4711/night/web";
        assert_eq!(middle_path(path, 1000., 7.), path);
        let short = middle_path(path, 26. * 7., 7.);
        assert_eq!(short, "/private/tmp/…/night/web");
        assert!(short.chars().count() <= 26);
        let other = middle_path(
            "/private/tmp/claude-1002/scratch/session-4711/night/api",
            26. * 7.,
            7.,
        );
        assert_ne!(short, other);
        assert_eq!(middle_path(path, 16. * 7., 7.), "/private/…/web");
        assert_eq!(middle_path(path, 8. * 7., 7.), "…/web");
        assert_eq!(
            middle_path("~/a-very-long-project-name", 10. * 7., 7.)
                .chars()
                .count(),
            10
        );
        assert_eq!(middle_path("~/projects/shop", 300., 7.), "~/projects/shop");
    }

    fn entry(path: &str, exists: bool) -> RecentEntry {
        RecentEntry {
            path: path.into(),
            exists,
        }
    }

    /// SPEC-0355 AK-1: rows with initials, `~` folder, branch and the missing mark.
    #[test]
    fn rows_show_initials_folder_branch_and_missing() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-welcome-{}", std::process::id()));
        let repo = dir.join("shop-front");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feature/cart\n").unwrap();
        let entries = [
            RecentEntry {
                path: repo.clone(),
                exists: true,
            },
            entry("/elsewhere/gone_project", false),
        ];
        let rows = project_rows(&entries, Some(&dir));
        assert_eq!(rows[0].name, "shop-front");
        assert_eq!(rows[0].initials, "SF");
        assert_eq!(rows[0].shown, "~/shop-front");
        assert_eq!(rows[0].branch.as_deref(), Some("feature/cart"));
        assert!(!rows[0].missing);
        assert_eq!(rows[1].shown, "/elsewhere/gone_project");
        assert!(rows[1].missing);
        assert_eq!(
            rows[1].branch, None,
            "nothing is read from a missing folder"
        );
        assert_eq!(open_row(&rows[0]), OpenRow::Open(repo.clone()));
        match open_row(&rows[1]) {
            OpenRow::Missing(text) => assert!(text.contains("no longer exists"), "{text}"),
            other => panic!("{other:?}"),
        }
        let report = report(&rows);
        assert_eq!(report[1]["exists"], false);
        assert_eq!(report[0]["branch"], "feature/cart");
        assert_eq!(
            default_location(&entries, Some(Path::new("/home/u"))),
            dir.display().to_string()
        );
        assert_eq!(
            default_location(&entries[1..], Some(Path::new("/home/u"))),
            "/home/u",
            "without an existing project: the home folder"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0355 AK-1: the search matches name and folder, ignoring case.
    #[test]
    fn the_search_matches_name_and_folder_ignoring_case() {
        let rows = project_rows(
            &[
                entry("/w/Shop", false),
                entry("/w/blog", false),
                entry("/other/notes", false),
            ],
            None,
        );
        assert_eq!(filter(&rows, ""), [0, 1, 2]);
        assert_eq!(filter(&rows, "SHO"), [0]);
        assert_eq!(filter(&rows, "/w/"), [0, 1]);
        assert!(filter(&rows, "zzz").is_empty());
    }

    /// SPEC-0355 AK-1: ↓ and ↑ wrap; the sections in their order with ⌘1 to ⌘5 (v3).
    #[test]
    fn steps_wrap_and_digits_choose_sections() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(0, 0, true), 0);
        assert_eq!(
            Section::ALL.map(Section::title),
            ["Projects", "SSH", "Customize", "Plugins", "Learn"]
        );
        assert_eq!(Section::Ssh.group(), Some("Remote Development"));
        assert_eq!(Section::Projects.group(), None);
        assert_eq!(Section::of_digit("1"), Some(Section::Projects));
        assert_eq!(Section::of_digit("2"), Some(Section::Ssh));
        assert_eq!(Section::of_digit("5"), Some(Section::Learn));
        assert_eq!(Section::of_digit("6"), None);
        assert_eq!(Section::of_digit("0"), None);
    }

    /// SPEC-0355 AK-1: Learn names its targets — Ferris IDEA's own Keymap page or Ferris IDEA's
    /// repository over HTTPS — and opens nothing by itself; the import takes only the user's own
    /// settings.
    #[test]
    fn learn_names_its_targets_and_import_takes_user_settings_only() {
        for (title, _, target) in LEARN {
            match target {
                LearnTarget::Keymap => assert_eq!(title, "Keyboard Shortcuts"),
                LearnTarget::Address(a) => {
                    assert!(a.starts_with(REPOSITORY), "{title}: {a}");
                    assert!(a.starts_with("https://"));
                }
            }
        }
        assert_eq!(import_refusal(Some("settings")), None);
        assert_eq!(import_refusal(Some("keymap")), None);
        assert!(import_refusal(Some("run")).unwrap().contains("project"));
        assert!(import_refusal(None).is_some());
        assert_eq!(
            GearItem::ALL.map(GearItem::title),
            ["All Settings…", "Import Settings…", "Keymap…", "Plugins…"]
        );
    }
}
