//! What a hosted agent's turn changed in the workspace, and undoing it (`SPEC-0348` v2
//! behaviours 26–30, IMP-0548, IMP-0549).
//!
//! The host records the workspace before it sends a prompt and compares it after the turn ended.
//! The change list therefore comes from the file system, not from what the agent says: files a
//! command of the agent created or changed count as much as its edits, and an edit the person
//! refused does not appear. Undo restores the recorded contents, all or nothing, and refuses
//! files that changed again after the turn.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Largest file whose content is recorded (bytes); larger files keep size and time only.
pub const FILE_BYTES: u64 = 512 * 1024;
/// Most content recorded per turn (bytes).
pub const TOTAL_BYTES: u64 = 64 * 1024 * 1024;
/// Most files looked at per turn.
pub const MAX_FILES: usize = 20_000;
/// Turns whose changes are kept per agent.
pub const TURNS_KEPT: usize = 8;
/// Largest unified diff of one answer (bytes).
pub const DIFF_BYTES: usize = 200 * 1024;
/// Folders never recorded: the repository, Ferris IDEA's own folder.
const SKIPPED: [&str; 2] = [".git", ".ferris-idea"];

#[derive(Debug, Clone)]
struct FileState {
    size: u64,
    modified: Option<SystemTime>,
    hash: Option<[u8; 32]>,
    content: Option<Vec<u8>>,
}

/// The workspace as it was at one moment (behaviour 26).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    files: BTreeMap<String, FileState>,
    /// More files than [`MAX_FILES`]: files beyond were not looked at.
    pub truncated: bool,
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Records the workspace below `root`: every regular file the ignore rules of the workspace do
/// not exclude, without `.git` and `.ferris-idea`; contents of files up to [`FILE_BYTES`] while the
/// total stays within [`TOTAL_BYTES`].
pub fn snapshot(root: &Path) -> Snapshot {
    let mut snap = Snapshot::default();
    let mut total = 0u64;
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| {
            !(e.depth() > 0
                && e.file_type().is_some_and(|t| t.is_dir())
                && SKIPPED.iter().any(|s| e.file_name() == *s))
        })
        .sort_by_file_path(|a, b| a.cmp(b))
        .build();
    for entry in walk.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if snap.files.len() >= MAX_FILES {
            snap.truncated = true;
            break;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let Some(rel) = rel.to_str().map(|s| s.replace('\\', "/")) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        let size = meta.len();
        let (content, h) = if size <= FILE_BYTES && total + size <= TOTAL_BYTES {
            match std::fs::read(entry.path()) {
                Ok(b) => {
                    total += b.len() as u64;
                    let h = hash(&b);
                    (Some(b), Some(h))
                }
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };
        snap.files.insert(
            rel,
            FileState {
                size,
                modified: meta.modified().ok(),
                hash: h,
                content,
            },
        );
    }
    snap
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// It did not exist before the turn.
    Created,
    /// Its content changed.
    Modified,
    /// It existed before and does not now.
    Deleted,
}

/// One file a turn changed (behaviour 27).
#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    /// Path relative to the workspace, with `/`.
    pub path: String,
    /// Created, modified or deleted.
    pub kind: ChangeKind,
    /// Lines added (0 for binary files).
    pub added: usize,
    /// Lines removed (0 for binary files).
    pub removed: usize,
    /// The content is not text.
    pub binary: bool,
    /// Undo can put it back: its content before (and after) the turn was recorded.
    pub restorable: bool,
    #[serde(skip)]
    before: Option<Vec<u8>>,
    #[serde(skip)]
    after: Option<Vec<u8>>,
    #[serde(skip)]
    after_hash: Option<[u8; 32]>,
}

/// What one turn changed.
#[derive(Debug, Clone, Serialize)]
pub struct TurnChanges {
    /// The turn's number (1 for the first prompt).
    pub turn: u64,
    /// The beginning of the prompt (at most 80 characters).
    pub prompt: String,
    /// The files, by path.
    pub files: Vec<FileChange>,
    /// The workspace had more files than were looked at.
    pub truncated: bool,
    /// The turn was undone.
    pub undone: bool,
}

fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// The longest common subsequence of two line lists as pairs of indices, for lists whose
/// product stays small; otherwise none (every middle line counts as changed).
fn common_lines(a: &[&str], b: &[&str]) -> Option<Vec<(usize, usize)>> {
    if a.len().saturating_mul(b.len()) > 4_000_000 {
        return None;
    }
    let (n, m) = (a.len(), b.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * (m + 1) + j] = if a[i] == b[j] {
                t[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                t[(i + 1) * (m + 1) + j].max(t[i * (m + 1) + j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if t[(i + 1) * (m + 1) + j] >= t[i * (m + 1) + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    Some(out)
}

/// One line of a line diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same(usize, usize),
    Removed(usize),
    Added(usize),
}

fn line_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut ops: Vec<Op> = (0..pre).map(|i| Op::Same(i, i)).collect();
    let common = common_lines(am, bm).unwrap_or_default();
    let (mut i, mut j) = (0, 0);
    for (ci, cj) in common.into_iter().chain([(am.len(), bm.len())]) {
        ops.extend((i..ci).map(|k| Op::Removed(pre + k)));
        ops.extend((j..cj).map(|k| Op::Added(pre + k)));
        if ci < am.len() {
            ops.push(Op::Same(pre + ci, pre + cj));
        }
        i = ci + 1;
        j = cj + 1;
    }
    ops.extend((0..suf).map(|k| Op::Same(a.len() - suf + k, b.len() - suf + k)));
    ops
}

/// Lines added and removed between two texts.
pub fn line_counts(before: &str, after: &str) -> (usize, usize) {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    line_ops(&a, &b)
        .iter()
        .fold((0, 0), |(ad, rm), op| match op {
            Op::Added(_) => (ad + 1, rm),
            Op::Removed(_) => (ad, rm + 1),
            Op::Same(..) => (ad, rm),
        })
}

/// A unified diff of one file with three lines of context (behaviour 29).
pub fn unified_diff(path: &str, before: &str, after: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    let ops = line_ops(&a, &b);
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, o)| !matches!(o, Op::Same(..)))
        .map(|(i, _)| i)
        .collect();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(3);
        let mut end = changed[k];
        while k < changed.len() && changed[k] <= end + 6 {
            end = changed[k];
            k += 1;
        }
        let end = (end + 4).min(ops.len());
        let hunk = &ops[start..end];
        let a0 = hunk
            .iter()
            .find_map(|o| match o {
                Op::Same(i, _) | Op::Removed(i) => Some(*i),
                Op::Added(_) => None,
            })
            .unwrap_or(0);
        let b0 = hunk
            .iter()
            .find_map(|o| match o {
                Op::Same(_, j) | Op::Added(j) => Some(*j),
                Op::Removed(_) => None,
            })
            .unwrap_or(0);
        let an = hunk.iter().filter(|o| !matches!(o, Op::Added(_))).count();
        let bn = hunk.iter().filter(|o| !matches!(o, Op::Removed(_))).count();
        out.push_str(&format!(
            "@@ -{},{an} +{},{bn} @@\n",
            if an == 0 { a0 } else { a0 + 1 },
            if bn == 0 { b0 } else { b0 + 1 }
        ));
        for o in hunk {
            match o {
                Op::Same(i, _) => out.push_str(&format!(" {}\n", a[*i])),
                Op::Removed(i) => out.push_str(&format!("-{}\n", a[*i])),
                Op::Added(j) => out.push_str(&format!("+{}\n", b[*j])),
            }
        }
    }
    out
}

/// Compares the workspace now with `before` (behaviour 27): every file created, modified or
/// deleted, by path.
pub fn compare(root: &Path, before: &Snapshot, turn: u64, prompt: &str) -> TurnChanges {
    let after = snapshot(root);
    let mut files = Vec::new();
    let paths: BTreeSet<&String> = before.files.keys().chain(after.files.keys()).collect();
    for path in paths {
        let (b, a) = (before.files.get(path), after.files.get(path));
        let kind = match (b, a) {
            (None, Some(_)) => ChangeKind::Created,
            (Some(_), None) => {
                // A file beyond the limit of the second walk is not known to be gone.
                if after.truncated {
                    continue;
                }
                ChangeKind::Deleted
            }
            (Some(b), Some(a)) => {
                let same = match (b.hash, a.hash) {
                    (Some(x), Some(y)) => x == y,
                    _ => b.size == a.size && b.modified == a.modified,
                };
                if same {
                    continue;
                }
                ChangeKind::Modified
            }
            (None, None) => continue,
        };
        let before_bytes = b.and_then(|s| s.content.clone());
        let after_bytes = a.and_then(|s| s.content.clone());
        let restorable = match kind {
            ChangeKind::Created => a.is_some_and(|s| s.hash.is_some()),
            ChangeKind::Deleted => before_bytes.is_some(),
            ChangeKind::Modified => before_bytes.is_some() && after_bytes.is_some(),
        };
        let text = |x: &Option<Vec<u8>>| x.as_deref().is_none_or(is_text);
        let binary = !(text(&before_bytes) && text(&after_bytes))
            || (before_bytes.is_none() && kind != ChangeKind::Created)
            || (after_bytes.is_none() && kind != ChangeKind::Deleted);
        let (added, removed) = if binary {
            (0, 0)
        } else {
            let s = |x: &Option<Vec<u8>>| {
                x.as_deref()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default()
            };
            line_counts(&s(&before_bytes), &s(&after_bytes))
        };
        files.push(FileChange {
            path: path.clone(),
            kind,
            added,
            removed,
            binary,
            restorable,
            before: before_bytes,
            after_hash: a.and_then(|s| s.hash),
            after: after_bytes,
        });
    }
    TurnChanges {
        turn,
        prompt: prompt.chars().take(80).collect(),
        files,
        truncated: before.truncated || after.truncated,
        undone: false,
    }
}

impl TurnChanges {
    /// The unified diff of the turn's text files, at most [`DIFF_BYTES`]; binary files and
    /// files without recorded content are named only.
    pub fn diff(&self) -> String {
        let mut out = String::new();
        for f in &self.files {
            let part = if f.binary {
                format!("Binary or unrecorded file {} ({:?})\n", f.path, f.kind)
            } else {
                let s = |x: &Option<Vec<u8>>| {
                    x.as_deref()
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                        .unwrap_or_default()
                };
                unified_diff(&f.path, &s(&f.before), &s(&f.after))
            };
            if out.len() + part.len() > DIFF_BYTES {
                out.push_str("… (diff cut at the size limit)\n");
                break;
            }
            out.push_str(&part);
        }
        out
    }

    /// The text of `path` before and after the turn, when both are recorded text (the window's
    /// diff view).
    pub fn texts(&self, path: &str) -> Option<(String, String)> {
        let f = self.files.iter().find(|f| f.path == path)?;
        if f.binary {
            return None;
        }
        let s = |x: &Option<Vec<u8>>| {
            x.as_deref()
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default()
        };
        Some((s(&f.before), s(&f.after)))
    }

    /// Why Undo cannot run now: files that cannot be restored or that changed again after the
    /// turn (behaviour 28). Empty when it can.
    pub fn undo_blockers(&self, root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        if self.undone {
            out.push("the turn was undone already".into());
            return out;
        }
        for f in &self.files {
            if !f.restorable {
                out.push(format!(
                    "{}: its content was not recorded (too large or beyond the limit)",
                    f.path
                ));
                continue;
            }
            let now = std::fs::read(root.join(&f.path)).ok();
            let unchanged = match (f.kind, &now) {
                (ChangeKind::Deleted, None) => true,
                (ChangeKind::Deleted, Some(_)) => false,
                (_, Some(b)) => Some(hash(b)) == f.after_hash,
                (_, None) => false,
            };
            if !unchanged {
                out.push(format!("{}: changed again after the turn", f.path));
            }
        }
        out
    }

    /// Undoes the turn (behaviour 28): restores every file, all or nothing. Answers the paths it
    /// put back.
    pub fn undo(&mut self, root: &Path) -> Result<Vec<String>, String> {
        let blockers = self.undo_blockers(root);
        if !blockers.is_empty() {
            return Err(format!("nothing was undone: {}", blockers.join("; ")));
        }
        let mut done = Vec::new();
        for f in &self.files {
            let p: PathBuf = root.join(&f.path);
            let r = match f.kind {
                ChangeKind::Created => std::fs::remove_file(&p),
                ChangeKind::Modified | ChangeKind::Deleted => {
                    let bytes = f.before.as_deref().unwrap_or_default();
                    p.parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| std::fs::write(&p, bytes))
                }
            };
            r.map_err(|e| {
                format!(
                    "undoing stopped at {}: {e}; already put back: {}",
                    f.path,
                    if done.is_empty() {
                        "nothing".to_owned()
                    } else {
                        done.join(", ")
                    }
                )
            })?;
            done.push(f.path.clone());
        }
        self.undone = true;
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ferris-idea-turns-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    #[test]
    fn a_turn_lists_created_modified_and_deleted_files_and_undo_restores_them() {
        let root = dir("undo");
        std::fs::write(root.join("src/a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(root.join("gone.txt"), "bye\n").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref\n").unwrap();
        let before = snapshot(&root);
        std::fs::write(root.join("src/a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        std::fs::write(root.join("src/A.class"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(root.join(".git/HEAD"), "other\n").unwrap();
        let mut t = compare(&root, &before, 1, "change things");
        let got: Vec<(&str, ChangeKind, usize, usize, bool)> = t
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.kind, f.added, f.removed, f.binary))
            .collect();
        assert_eq!(
            got,
            vec![
                ("gone.txt", ChangeKind::Deleted, 0, 1, false),
                ("src/A.class", ChangeKind::Created, 0, 0, true),
                ("src/a.txt", ChangeKind::Modified, 2, 1, false),
            ]
        );
        let diff = t.diff();
        assert!(diff.contains("-two\n+TWO\n"), "{diff}");
        assert!(diff.contains("+four\n"), "{diff}");
        assert!(
            diff.contains("Binary or unrecorded file src/A.class"),
            "{diff}"
        );
        assert!(t.undo_blockers(&root).is_empty());
        let done = t.undo(&root).unwrap();
        assert_eq!(done.len(), 3);
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("gone.txt")).unwrap(),
            "bye\n"
        );
        assert!(!root.join("src/A.class").exists());
        assert!(t.undone);
        assert!(t.undo(&root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn undo_refuses_files_changed_after_the_turn_and_changes_nothing() {
        let root = dir("conflict");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        let before = snapshot(&root);
        std::fs::write(root.join("a.txt"), "a2\n").unwrap();
        std::fs::write(root.join("b.txt"), "b2\n").unwrap();
        let mut t = compare(&root, &before, 1, "x");
        std::fs::write(root.join("b.txt"), "b3 by the person\n").unwrap();
        let e = t.undo(&root).unwrap_err();
        assert!(e.contains("b.txt: changed again after the turn"), "{e}");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a2\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unchanged_workspace_has_no_changes_and_ignored_files_are_skipped() {
        let root = dir("same");
        std::fs::write(root.join(".gitignore"), "out/\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let before = snapshot(&root);
        std::fs::create_dir_all(root.join("out")).unwrap();
        std::fs::write(root.join("out/build.log"), "log\n").unwrap();
        let t = compare(&root, &before, 2, "nothing");
        assert!(t.files.is_empty(), "{:?}", t.files);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn line_counts_and_hunks_follow_the_common_lines() {
        assert_eq!(line_counts("a\nb\nc\n", "a\nc\nd\n"), (1, 1));
        assert_eq!(line_counts("", "x\ny\n"), (2, 0));
        let d = unified_diff(
            "f",
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
            "1\n2\n3\n4\nfive\n6\n7\n8\n9\n10\n",
        );
        assert!(
            d.contains("@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n"),
            "{d}"
        );
    }
}
