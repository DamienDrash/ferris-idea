//! One undo step for a refactoring across files (`SPEC-0193`): which files and editor steps a
//! refactoring changed, the checks before it is undone or redone, and writing the files on disk.

use std::path::{Path, PathBuf};

/// How many groups are kept (behaviour 6).
pub const KEPT: usize = 20;

/// One file a refactoring changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The file.
    pub path: PathBuf,
    /// Its text before the refactoring.
    pub before: String,
    /// Its text after the refactoring.
    pub after: String,
    /// The editor's undo step holding the change; `None` when the file was written on disk.
    pub step: Option<u64>,
}

/// The files of one refactoring (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Name shown in the status bar and the local history, e.g. "Rename helper to calc".
    pub name: String,
    /// Changed files.
    pub parts: Vec<Part>,
    /// Whether the group is undone (and can be redone).
    pub undone: bool,
    /// Files the refactoring renamed, as (before, after); their parts name the file after
    /// (`SPEC-0148` v2 behaviour 6: a Java class takes its file along).
    pub moves: Vec<(PathBuf, PathBuf)>,
}

impl Group {
    /// A group of `parts` that is not undone.
    pub fn new(name: impl Into<String>, parts: Vec<Part>) -> Self {
        Self {
            name: name.into(),
            parts,
            undone: false,
            moves: Vec::new(),
        }
    }

    /// The group with the files it renamed.
    pub fn with_moves(mut self, moves: Vec<(PathBuf, PathBuf)>) -> Self {
        self.moves = moves;
        self
    }

    /// Whether plain undo already takes it back: one file, changed in its editor, not renamed
    /// (behaviour 6).
    pub fn is_single_step(&self) -> bool {
        self.parts.len() == 1 && self.parts[0].step.is_some() && self.moves.is_empty()
    }

    /// Where the file of `part` is while the group is done (`undone` false) or undone: a renamed
    /// file is at its earlier name while undone.
    pub fn location(&self, part: &Part, undone: bool) -> PathBuf {
        match self.moves.iter().find(|(_, to)| *to == part.path) {
            Some((from, _)) if undone => from.clone(),
            _ => part.path.clone(),
        }
    }

    /// Renames the group's files back (`redo` false) or again; stops at the first failure.
    pub fn move_files(&self, redo: bool) -> Result<(), String> {
        for (from, to) in &self.moves {
            let (a, b) = if redo { (from, to) } else { (to, from) };
            std::fs::rename(a, b).map_err(|e| format!("{} → {}: {e}", a.display(), b.display()))?;
        }
        Ok(())
    }
}

/// What a part looks like now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Now {
    /// The file is open: its newest undo step and its next redo step.
    Editor {
        /// Newest undo step.
        top: Option<u64>,
        /// Next redo step.
        redo: Option<u64>,
    },
    /// Not open (or written on disk): the file's text, `None` if it cannot be read.
    Disk(Option<String>),
}

/// Checks every part before undoing (`redo` false) or redoing the group; the error is the first
/// file that changed since (behaviours 3, 4).
pub fn check(group: &Group, redo: bool, now: impl Fn(&Part) -> Now) -> Result<(), PathBuf> {
    for part in &group.parts {
        let unchanged = match (now(part), part.step) {
            (Now::Editor { top, redo: next }, Some(step)) => {
                if redo {
                    next == Some(step)
                } else {
                    top == Some(step)
                }
            }
            // A file written on disk that is open now changed through its editor since.
            (Now::Editor { .. }, None) => false,
            (Now::Disk(text), _) => {
                text.as_deref() == Some(if redo { &part.before } else { &part.after })
            }
        };
        if !unchanged {
            return Err(part.path.clone());
        }
    }
    Ok(())
}

/// Writes the earlier text (`redo` false) or the new text of `parts` on disk; all are written even
/// when one fails, and the failures are named.
pub fn write(parts: &[&Part], redo: bool) -> Result<(), String> {
    let failed: Vec<String> = parts
        .iter()
        .filter_map(|p| {
            let text = if redo { &p.after } else { &p.before };
            std::fs::write(&p.path, text)
                .err()
                .map(|e| format!("{}: {e}", p.path.display()))
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join(", "))
    }
}

/// Records `paths` in the local history of the workspace at `root` (behaviour 2); with a label,
/// labels each file's new revision and the workspace afterwards, so the file's history names the
/// refactoring (`SPEC-0193` v2 behaviour 8, D46). Failures of the history are not the refactoring's.
pub fn record_history(root: &Path, paths: &[&Path], label: Option<&str>) {
    record(root, paths, label, true);
}

fn record(root: &Path, paths: &[&Path], label: Option<&str>, workspace: bool) {
    let Ok(mut history) = idecraft_core::LocalHistory::open(root) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let root = root.canonicalize().unwrap_or(root.to_path_buf());
    for path in paths {
        let path = path.canonicalize().unwrap_or(path.to_path_buf());
        if let Ok(relative) = path.strip_prefix(&root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            let _ = match label {
                // Records the file, then labels that revision.
                Some(label) => history.put_label_at(Some(&relative), label, now),
                None => history.record(&relative).map(|_| ()),
            };
        }
    }
    if let Some(label) = label.filter(|_| workspace) {
        let _ = history.put_label_at(None, label, now);
    }
}

/// Records the saved state of files a refactoring changes only in their editors (open files) and
/// labels it "Before <name>", so their local history has the state to go back to (`SPEC-0193` v2
/// behaviour 8, D46); the changed text is recorded when the editor saves.
pub fn record_history_before(root: &Path, paths: &[&Path], name: &str) {
    record(root, paths, Some(&format!("Before {name}")), false);
}

/// The newest groups of a workspace (behaviours 3, 4, 6).
#[derive(Debug, Clone, Default)]
pub struct UndoGroups {
    groups: Vec<Group>,
}

impl UndoGroups {
    /// Keeps `group`; the oldest goes beyond [`KEPT`]. Redo of undone groups ends with a new one.
    pub fn push(&mut self, group: Group) {
        self.groups.retain(|g| !g.undone);
        self.groups.push(group);
        if self.groups.len() > KEPT {
            self.groups.remove(0);
        }
    }

    /// The newest group not undone whose part in `path` is editor step `top`.
    pub fn for_undo(&self, path: &Path, top: u64) -> Option<usize> {
        self.find(path, top, false)
    }

    /// The newest undone group whose part in `path` is editor step `next`.
    pub fn for_redo(&self, path: &Path, next: u64) -> Option<usize> {
        self.find(path, next, true)
    }

    fn find(&self, path: &Path, step: u64, undone: bool) -> Option<usize> {
        self.groups.iter().rposition(|g| {
            g.undone == undone
                && g.parts
                    .iter()
                    .any(|p| g.location(p, undone) == path && p.step == Some(step))
        })
    }

    /// The newest group that is not undone (MCP `undo_refactoring`, behaviour 7).
    pub fn newest(&self) -> Option<usize> {
        self.groups.iter().rposition(|g| !g.undone)
    }

    /// The group a redo takes next (MCP `redo_refactoring`): the undone group that was undone
    /// last. Undo goes from the newest group backwards, so the undone groups are the newest ones
    /// and the oldest of them was undone last.
    pub fn next_redo(&self) -> Option<usize> {
        self.groups.iter().position(|g| g.undone)
    }

    /// Group `i`.
    pub fn get(&self, i: usize) -> Option<&Group> {
        self.groups.get(i)
    }

    /// Marks group `i` undone or redone.
    pub fn set_undone(&mut self, i: usize, undone: bool) {
        if let Some(g) = self.groups.get_mut(i) {
            g.undone = undone;
        }
    }

    /// Drops group `i` (it cannot be undone as a whole any more).
    pub fn forget(&mut self, i: usize) -> Option<Group> {
        (i < self.groups.len()).then(|| self.groups.remove(i))
    }

    /// Number of groups kept.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether no group is kept.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Undoes (`redo` false) or redoes the newest group of files written on disk (MCP, behaviour 7):
/// checks every file, writes them, records them in the local history of `root` and labels it.
/// Returns the group's name and files.
pub fn undo_on_disk(
    groups: &mut UndoGroups,
    root: &Path,
    redo: bool,
) -> Result<(String, Vec<PathBuf>), String> {
    let i = if redo {
        groups.next_redo()
    } else {
        groups.newest()
    }
    .ok_or_else(|| format!("nothing to {}", if redo { "redo" } else { "undo" }))?;
    let group = groups.get(i).cloned().expect("found");
    if let Err(path) = check(&group, redo, |p| {
        Now::Disk(std::fs::read_to_string(group.location(p, redo)).ok())
    }) {
        groups.forget(i);
        return Err(format!(
            "cannot {} {}: {} changed since",
            if redo { "redo" } else { "undo" },
            group.name,
            path.display()
        ));
    }
    let parts: Vec<&Part> = group.parts.iter().collect();
    // Renamed files are written under their later name, then renamed back (and the other way).
    if redo {
        group.move_files(true)?;
    }
    write(&parts, redo)?;
    if !redo {
        group.move_files(false)?;
    }
    let located: Vec<PathBuf> = group
        .parts
        .iter()
        .map(|p| group.location(p, !redo))
        .collect();
    let paths: Vec<&Path> = located.iter().map(PathBuf::as_path).collect();
    let verb = if redo { "Redo" } else { "Undo" };
    record_history(root, &paths, Some(&format!("{verb} {}", group.name)));
    groups.set_undone(i, !redo);
    Ok((group.name.clone(), located))
}

/// Applies file edits on disk as a group named `name` (MCP, behaviours 1, 2, 7): every file is checked
/// first, then all are written, recorded in the local history and the workspace labelled.
pub fn apply_on_disk(
    root: &Path,
    name: &str,
    files: &[crate::rename::FileEdits],
) -> Result<Group, String> {
    apply_on_disk_moving(root, name, files, &[])
}

/// Like [`apply_on_disk`], then renames the files of `moves` (before, after) as part of the group.
pub fn apply_on_disk_moving(
    root: &Path,
    name: &str,
    files: &[crate::rename::FileEdits],
    moves: &[(PathBuf, PathBuf)],
) -> Result<Group, String> {
    let mut parts = Vec::new();
    for file in files {
        let before = std::fs::read_to_string(&file.path)
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
        let after = crate::rename::apply_edits(&before, &file.edits)
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
        parts.push(Part {
            path: file.path.clone(),
            before,
            after,
            step: None,
        });
    }
    let paths: Vec<&Path> = parts.iter().map(|p| p.path.as_path()).collect();
    record_history(root, &paths, None);
    write(&parts.iter().collect::<Vec<_>>(), true)?;
    // Files renamed with the refactoring (`SPEC-0148` v2 behaviour 6): their parts name them after.
    let group = Group::new(name, Vec::new()).with_moves(moves.to_vec());
    group.move_files(true)?;
    for part in &mut parts {
        if let Some((_, to)) = moves.iter().find(|(from, _)| *from == part.path) {
            part.path = to.clone();
        }
    }
    let paths: Vec<&Path> = parts.iter().map(|p| p.path.as_path()).collect();
    record_history(root, &paths, Some(name));
    Ok(Group { parts, ..group })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0148 v2 behaviour 6 (D29): a renamed file goes back with undo and again with redo.
    #[test]
    fn renamed_files_follow_undo_and_redo() {
        use crate::insight::Range;
        use crate::rename::{Edit, FileEdits};
        let root = std::env::temp_dir().join(format!("idecraft-undo-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (lager, speicher) = (root.join("Lager.java"), root.join("Speicher.java"));
        std::fs::write(&lager, "public class Lager {}\n").unwrap();
        let edit = Edit {
            range: Range {
                start_line: 0,
                start_character: 13,
                end_line: 0,
                end_character: 18,
            },
            old_text: "Lager".into(),
            new_text: "Speicher".into(),
        };
        let files = [FileEdits {
            path: lager.clone(),
            edits: vec![edit],
        }];
        let moves = [(lager.clone(), speicher.clone())];
        let group =
            apply_on_disk_moving(&root, "Rename Lager to Speicher", &files, &moves).unwrap();
        assert!(!lager.exists());
        assert_eq!(
            std::fs::read_to_string(&speicher).unwrap(),
            "public class Speicher {}\n"
        );
        let mut groups = UndoGroups::default();
        groups.push(group);
        let (_, undone) = undo_on_disk(&mut groups, &root, false).unwrap();
        assert_eq!(undone, std::slice::from_ref(&lager));
        assert!(!speicher.exists());
        assert_eq!(
            std::fs::read_to_string(&lager).unwrap(),
            "public class Lager {}\n"
        );
        let (_, redone) = undo_on_disk(&mut groups, &root, true).unwrap();
        assert_eq!(redone, std::slice::from_ref(&speicher));
        assert!(!lager.exists());
        assert_eq!(
            std::fs::read_to_string(&speicher).unwrap(),
            "public class Speicher {}\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn part(path: &str, step: Option<u64>) -> Part {
        Part {
            path: PathBuf::from(path),
            before: format!("{path} before"),
            after: format!("{path} after"),
            step,
        }
    }

    #[test]
    fn finds_groups_by_editor_step() {
        let mut groups = UndoGroups::default();
        groups.push(Group::new(
            "Rename a to b",
            vec![part("/p/a.c", Some(4)), part("/p/b.c", None)],
        ));
        assert_eq!(groups.for_undo(Path::new("/p/a.c"), 4), Some(0));
        assert_eq!(groups.for_undo(Path::new("/p/a.c"), 5), None);
        assert_eq!(
            groups.for_undo(Path::new("/p/b.c"), 4),
            None,
            "b was on disk"
        );
        assert_eq!(groups.for_redo(Path::new("/p/a.c"), 4), None, "not undone");
        groups.set_undone(0, true);
        assert_eq!(groups.for_undo(Path::new("/p/a.c"), 4), None);
        assert_eq!(groups.for_redo(Path::new("/p/a.c"), 4), Some(0));
        assert!(
            Group::new("x", vec![part("/p/a.c", Some(1))]).is_single_step(),
            "plain undo suffices"
        );
        assert!(!groups.get(0).unwrap().is_single_step());
    }

    #[test]
    fn a_changed_part_refuses_the_group() {
        let group = Group::new("Fix", vec![part("/p/a.c", Some(4)), part("/p/b.c", None)]);
        let fine = |p: &Part| match p.step {
            Some(s) => Now::Editor {
                top: Some(s),
                redo: None,
            },
            None => Now::Disk(Some(p.after.clone())),
        };
        assert_eq!(check(&group, false, fine), Ok(()));
        let typed_after = |p: &Part| match p.step {
            Some(s) => Now::Editor {
                top: Some(s + 1),
                redo: None,
            },
            None => Now::Disk(Some(p.after.clone())),
        };
        assert_eq!(
            check(&group, false, typed_after),
            Err(PathBuf::from("/p/a.c"))
        );
        let disk_changed = |p: &Part| match p.step {
            Some(s) => Now::Editor {
                top: Some(s),
                redo: None,
            },
            None => Now::Disk(Some("other".into())),
        };
        assert_eq!(
            check(&group, false, disk_changed),
            Err(PathBuf::from("/p/b.c"))
        );
        // Redo wants the editor's next redo step and the earlier text on disk.
        let undone = |p: &Part| match p.step {
            Some(s) => Now::Editor {
                top: None,
                redo: Some(s),
            },
            None => Now::Disk(Some(p.before.clone())),
        };
        assert_eq!(check(&group, true, undone), Ok(()));
        // An editor closed since counts as its file on disk.
        let closed = |p: &Part| Now::Disk(Some(p.after.clone()));
        assert_eq!(check(&group, false, closed), Ok(()));
    }

    /// SPEC-0193 v3 behaviour 3, AK-7: a refused group is kept and refused again until the later
    /// change is gone, then the whole group can be undone.
    #[test]
    fn a_refused_group_stays_until_it_can_be_undone() {
        let mut groups = UndoGroups::default();
        groups.push(Group::new(
            "Rename a to b",
            vec![part("/p/a.c", Some(4)), part("/p/b.c", None)],
        ));
        let b_now = std::cell::RefCell::new(String::from("changed outside"));
        let now = |p: &Part| match p.step {
            Some(s) => Now::Editor {
                top: Some(s),
                redo: None,
            },
            None => Now::Disk(Some(b_now.borrow().clone())),
        };
        for _ in 0..2 {
            let i = groups.for_undo(Path::new("/p/a.c"), 4).expect("kept");
            assert_eq!(
                check(groups.get(i).unwrap(), false, now),
                Err(PathBuf::from("/p/b.c"))
            );
        }
        *b_now.borrow_mut() = String::from("/p/b.c after");
        let i = groups.for_undo(Path::new("/p/a.c"), 4).expect("kept");
        assert_eq!(check(groups.get(i).unwrap(), false, now), Ok(()));
    }

    #[test]
    fn keeps_the_newest_groups() {
        let mut groups = UndoGroups::default();
        for i in 0..KEPT + 3 {
            groups.push(Group::new(
                format!("g{i}"),
                vec![part("/p/a.c", Some(i as u64))],
            ));
        }
        assert_eq!(groups.len(), KEPT);
        assert_eq!(groups.get(0).unwrap().name, "g3");
        assert_eq!(groups.newest(), Some(KEPT - 1));
        // A new group drops what was undone: it cannot be redone any more.
        groups.set_undone(KEPT - 1, true);
        groups.push(Group::new("new", vec![part("/p/a.c", Some(99))]));
        assert_eq!(groups.len(), KEPT);
        assert!(
            groups
                .for_redo(Path::new("/p/a.c"), (KEPT + 2) as u64)
                .is_none()
        );
    }

    #[test]
    fn undoes_and_redoes_files_on_disk() {
        use crate::insight::Range;
        use crate::rename::{Edit, FileEdits};
        let root = std::env::temp_dir().join(format!("idecraft-undo-group-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (a, b) = (root.join("a.c"), root.join("b.c"));
        std::fs::write(&a, "int helper();\n").unwrap();
        std::fs::write(&b, "x = helper();\n").unwrap();
        let edit = |line_start: u64| Edit {
            range: Range {
                start_line: 0,
                start_character: line_start,
                end_line: 0,
                end_character: line_start + 6,
            },
            old_text: "helper".into(),
            new_text: "calc".into(),
        };
        let files = vec![
            FileEdits {
                path: a.clone(),
                edits: vec![edit(4)],
            },
            FileEdits {
                path: b.clone(),
                edits: vec![edit(4)],
            },
        ];
        let mut groups = UndoGroups::default();
        groups.push(apply_on_disk(&root, "Rename helper to calc", &files).unwrap());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "int calc();\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "x = calc();\n");
        let history = idecraft_core::LocalHistory::open(&root).unwrap();
        assert!(
            history
                .revisions("a.c")
                .iter()
                .any(|r| r.label.as_deref() == Some("Rename helper to calc")),
            "labelled in the local history"
        );

        let (name, paths) = undo_on_disk(&mut groups, &root, false).unwrap();
        assert_eq!(name, "Rename helper to calc");
        assert_eq!(paths.len(), 2);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "int helper();\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "x = helper();\n");
        assert!(
            undo_on_disk(&mut groups, &root, false).is_err(),
            "nothing left"
        );

        undo_on_disk(&mut groups, &root, true).unwrap();
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "x = calc();\n");

        // A file changed since refuses the whole group and leaves every file as it is.
        std::fs::write(&b, "x = calc() + 1;\n").unwrap();
        let err = undo_on_disk(&mut groups, &root, false).unwrap_err();
        assert!(err.contains("b.c changed since"), "{err}");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "int calc();\n");
        assert!(groups.is_empty(), "the group is forgotten");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0193 v6 behaviour 11: two refactorings of one file, both undone, are redone in the
    /// order they were made — the one undone last first. Before v6 redo took the newest undone
    /// group, whose check failed, and that group was forgotten.
    #[test]
    fn redo_takes_the_group_undone_last() {
        let one = |before: &str, after: &str| {
            Group::new(
                format!("{before} to {after}"),
                vec![Part {
                    path: PathBuf::from("/p/a.c"),
                    before: before.into(),
                    after: after.into(),
                    step: None,
                }],
            )
        };
        let mut groups = UndoGroups::default();
        groups.push(one("a", "b"));
        groups.push(one("b", "c"));
        assert_eq!(groups.next_redo(), None, "nothing undone");
        groups.set_undone(groups.newest().unwrap(), true);
        assert_eq!(groups.newest(), Some(0));
        groups.set_undone(0, true);
        assert_eq!(groups.next_redo(), Some(0), "a to b was undone last");
        groups.set_undone(0, false);
        assert_eq!(groups.next_redo(), Some(1));

        let root = std::env::temp_dir().join(format!("idecraft-redo-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let a = root.join("a.c");
        let on_disk = |before: &str, after: &str| {
            Group::new(
                format!("{before} to {after}"),
                vec![Part {
                    path: a.clone(),
                    before: before.into(),
                    after: after.into(),
                    step: None,
                }],
            )
        };
        std::fs::write(&a, "c").unwrap();
        let mut groups = UndoGroups::default();
        groups.push(on_disk("a", "b"));
        groups.push(on_disk("b", "c"));
        undo_on_disk(&mut groups, &root, false).unwrap();
        undo_on_disk(&mut groups, &root, false).unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        let (name, _) = undo_on_disk(&mut groups, &root, true).unwrap();
        assert_eq!(name, "a to b");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "b");
        let (name, _) = undo_on_disk(&mut groups, &root, true).unwrap();
        assert_eq!(name, "b to c");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "c");
        assert_eq!(groups.len(), 2, "both kept");
        let err = undo_on_disk(&mut groups, &root, true).unwrap_err();
        assert_eq!(err, "nothing to redo");
        let _ = std::fs::remove_dir_all(&root);
    }
}
