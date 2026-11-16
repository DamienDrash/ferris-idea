//! Resolving one conflicted file (`SPEC-0101` v6 behaviours 12–14): a file whose sides are all
//! text is merged region by region (`SPEC-0037`); a file with a side that is not text, or that one
//! side deleted, takes one side as a whole — exactly what the index holds for it, or the removal
//! of the file. Such a file never goes through text the IDE built, so a binary file is not
//! overwritten with an empty one and a deleted side does not leave an empty file behind.

use serde::Serialize;

use crate::git::{Conflict, GitError, Repo};
use crate::merge3::{Chunk, ChunkKind, Labels, MarkerStyle, Merge3, Resolution};
use crate::ops::LOCAL_TIMEOUT;
use crate::remote::run_git;

/// One side of a conflicted file, by its index stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    /// The common ancestor (stage 1).
    Base,
    /// The current branch, HEAD (stage 2). In a stopped rebase that is the target.
    Ours,
    /// The branch being merged, or the commit being replayed or picked (stage 3).
    Theirs,
}

impl Side {
    fn stage(self) -> u8 {
        match self {
            Side::Base => 1,
            Side::Ours => 2,
            Side::Theirs => 3,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Side::Base => "base",
            Side::Ours => "ours",
            Side::Theirs => "theirs",
        }
    }
}

impl Conflict {
    /// Whether the file takes one side as a whole instead of a merge region by region: a side is
    /// not text, or ours or theirs has no such file (v6 behaviour 12).
    pub fn whole_side(&self) -> bool {
        self.binary || !self.has_ours || !self.has_theirs
    }

    /// Whether `side` has the file.
    pub fn has(&self, side: Side) -> bool {
        match side {
            Side::Base => self.has_base,
            Side::Ours => self.has_ours,
            Side::Theirs => self.has_theirs,
        }
    }

    /// The text of `side`; `None` when it has no file or the file is not text.
    pub fn side_text(&self, side: Side) -> Option<&str> {
        match side {
            Side::Base => self.base.as_deref(),
            Side::Ours => self.ours.as_deref(),
            Side::Theirs => self.theirs.as_deref(),
        }
    }
}

/// What resolving a file writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The merged text.
    Text(String),
    /// One side as the index holds it; the file is removed when that side has none.
    Side(Side),
}

/// The sides shown left and right: ours and theirs, swapped in a stopped rebase, where the
/// replayed commit (stage 3) is the user's own (`SPEC-0101` v2 behaviour 2).
fn columns(rebasing: bool) -> (Side, Side) {
    if rebasing {
        (Side::Theirs, Side::Ours)
    } else {
        (Side::Ours, Side::Theirs)
    }
}

/// The regions of `conflict` for the merge view, the left column as the model's "ours". A
/// whole-side file is one conflict whose columns show each side's text, or say that it is binary
/// or missing; that text is for display only and never written (v6 behaviour 12).
pub fn merge_model(conflict: &Conflict, rebasing: bool) -> Merge3 {
    let (left, right) = columns(rebasing);
    if !conflict.whole_side() {
        return Merge3::new(
            conflict.base.as_deref().unwrap_or(""),
            conflict.side_text(left).unwrap_or(""),
            conflict.side_text(right).unwrap_or(""),
        );
    }
    let shown = |side: Side| -> Vec<String> {
        match conflict.side_text(side) {
            Some(text) => text.split_inclusive('\n').map(str::to_owned).collect(),
            None if conflict.has(side) => vec!["Binary file".to_owned()],
            None => vec!["No file on this side".to_owned()],
        }
    };
    Merge3::from_chunks(vec![Chunk {
        kind: ChunkKind::Conflict,
        base: Vec::new(),
        ours: shown(left),
        theirs: shown(right),
        resolution: None,
    }])
}

/// What `merge` (from [`merge_model`], with the user's choices) writes; `None` while a conflict
/// is open. A whole-side file gives the side taken, never a text, and counts as open with any
/// other choice (v6 behaviour 12).
pub fn merge_outcome(conflict: &Conflict, rebasing: bool, merge: &Merge3) -> Option<Outcome> {
    let (left, right) = columns(rebasing);
    if conflict.whole_side() {
        return match merge.chunks.first()?.resolution {
            Some(Resolution::Ours) => Some(Outcome::Side(left)),
            Some(Resolution::Theirs) => Some(Outcome::Side(right)),
            _ => None,
        };
    }
    (merge.open_conflicts() == 0)
        .then(|| Outcome::Text(merge.text(&Labels::default(), MarkerStyle::Merge)))
}

/// Conflicts of the file still open: for a whole-side file one until a side is taken, whatever
/// else was chosen for it (v6 behaviour 12).
pub fn merge_open(conflict: &Conflict, rebasing: bool, merge: &Merge3) -> usize {
    if conflict.whole_side() {
        usize::from(merge_outcome(conflict, rebasing, merge).is_none())
    } else {
        merge.open_conflicts()
    }
}

/// One conflict of a file and its place in the result text (`SPEC-0101` v7 behaviour 26).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictRegion {
    /// Number of the conflict in the file, from 0.
    pub index: usize,
    /// Whether it has no resolution yet.
    pub open: bool,
    /// Whether the word-by-word merge resolves it (`SPEC-0037` v2 behaviour 8).
    pub simple: bool,
    /// First line of its region in the result, from 1.
    pub start_line: usize,
    /// Lines its region has in the result.
    pub lines: usize,
    /// Base text of the region.
    pub base: String,
    /// Ours text of the region.
    pub ours: String,
    /// Theirs text of the region.
    pub theirs: String,
}

/// The result text of `merge` as the merge view holds it, and its conflicts: changes that do not
/// conflict are applied, a resolved conflict has its text, an open one its base text — never
/// conflict markers (`SPEC-0101` v7 behaviours 15 and 26).
pub fn merge_result(merge: &Merge3) -> (String, Vec<ConflictRegion>) {
    let mut text = String::new();
    let mut line = 1;
    let mut regions = Vec::new();
    for (at, chunk) in merge.chunks.iter().enumerate() {
        let part = chunk.result().unwrap_or_else(|| chunk.base.concat());
        let lines = part.split_inclusive('\n').count();
        if chunk.kind == ChunkKind::Conflict {
            regions.push(ConflictRegion {
                index: regions.len(),
                open: chunk.resolution.is_none(),
                simple: merge.simple_text(at).is_some(),
                start_line: line,
                lines,
                base: chunk.base.concat(),
                ours: chunk.ours.concat(),
                theirs: chunk.theirs.concat(),
            });
        }
        line += part.matches('\n').count();
        text.push_str(&part);
    }
    (text, regions)
}

impl Repo {
    /// Resolves the conflict of `path` with one whole side (v6 behaviour 12): writes what the
    /// index holds for that side, byte for byte, and marks the path resolved; when the side has no
    /// file, removes the file and stages the removal. Returns whether the file is there afterwards.
    /// The base can only be taken when it has the file.
    pub fn resolve_conflict_with_side(&self, path: &str, side: Side) -> Result<bool, GitError> {
        let dir = self.dir()?;
        let git = |args: &[&str]| -> Result<String, GitError> {
            let (ok, out, err, timed_out) = run_git(dir, args, LOCAL_TIMEOUT)?;
            if timed_out {
                return Err(GitError::Git(format!("git {} timed out", args[0])));
            }
            if !ok {
                return Err(GitError::Git(err.trim().to_owned()));
            }
            Ok(out)
        };
        // Mode and stage of every side of the path: `<mode> <id> <stage>\t<path>`.
        let listed = git(&["ls-files", "-u", "-z", "--", path])?;
        let stages: Vec<(&str, u8)> = listed
            .split('\0')
            .filter_map(|entry| {
                let (head, name) = entry.split_once('\t')?;
                let mut fields = head.split(' ');
                let mode = fields.next()?;
                let stage = fields.nth(1)?.parse().ok()?;
                (name == path).then_some((mode, stage))
            })
            .collect();
        if stages.is_empty() {
            return Err(GitError::Rejected(format!("{path} has no merge conflict")));
        }
        match stages.iter().find(|(_, stage)| *stage == side.stage()) {
            Some(("160000", _)) => Err(GitError::Rejected(format!(
                "{path} is a submodule: resolve it with Git"
            ))),
            Some(_) => {
                let stage = format!("--stage={}", side.stage());
                git(&["checkout-index", "-f", &stage, "--", path])?;
                git(&["add", "--", path])?;
                Ok(true)
            }
            None if side == Side::Base => Err(GitError::Rejected(format!(
                "{path} has no base version: both sides added it"
            ))),
            None => {
                git(&["rm", "-q", "-f", "--", path])?;
                Ok(false)
            }
        }
    }

    /// Writes `outcome` for the conflicted `path` and marks it resolved.
    pub fn apply_resolution(&self, path: &str, outcome: &Outcome) -> Result<(), GitError> {
        match outcome {
            Outcome::Text(text) => self.resolve_conflict(path, text),
            Outcome::Side(side) => self.resolve_conflict_with_side(path, *side).map(|_| ()),
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(base: Option<&str>, ours: Option<&str>, theirs: Option<&str>) -> Conflict {
        Conflict {
            path: "f".into(),
            base: base.map(str::to_owned),
            ours: ours.map(str::to_owned),
            theirs: theirs.map(str::to_owned),
            has_base: base.is_some(),
            has_ours: ours.is_some(),
            has_theirs: theirs.is_some(),
            binary: false,
        }
    }

    /// SPEC-0101 AK-11 (v6).
    #[test]
    fn a_whole_side_file_is_open_until_a_side_is_taken() {
        let binary = Conflict {
            base: None,
            ours: None,
            theirs: None,
            binary: true,
            ..conflict(Some(""), Some(""), Some(""))
        };
        let deleted_by_us = conflict(Some("a\nb\n"), None, Some("a\nB\n"));
        // Ours deleted the file, theirs emptied it: both sides read as empty text, and still it
        // is a conflict of whole sides.
        let deleted_and_emptied = conflict(Some("a\n"), None, Some(""));
        for c in [&binary, &deleted_by_us, &deleted_and_emptied] {
            assert!(c.whole_side());
            let mut merge = merge_model(c, false);
            assert_eq!(merge.conflicts(), [0]);
            assert_eq!(merge_open(c, false, &merge), 1, "never resolved by itself");
            assert_eq!(merge_outcome(c, false, &merge), None);
            // A word-by-word merge of what the columns show is no resolution either.
            merge.resolve_simple();
            assert_eq!(merge_open(c, false, &merge), 1);
            assert_eq!(merge_outcome(c, false, &merge), None);
            // Both sides, the base or a text are no choice here.
            for other in [
                Resolution::OursThenTheirs,
                Resolution::TheirsThenOurs,
                Resolution::Base,
                Resolution::Custom(String::new()),
            ] {
                merge.resolve(0, Some(other));
                assert_eq!(merge_outcome(c, false, &merge), None);
                assert_eq!(merge_open(c, false, &merge), 1);
            }
            merge.resolve(0, Some(Resolution::Ours));
            assert_eq!(
                merge_outcome(c, false, &merge),
                Some(Outcome::Side(Side::Ours))
            );
            merge.resolve(0, Some(Resolution::Theirs));
            assert_eq!(
                merge_outcome(c, false, &merge),
                Some(Outcome::Side(Side::Theirs))
            );
            assert_eq!(merge_open(c, false, &merge), 0);
        }
        // The columns say what is there.
        let merge = merge_model(&binary, false);
        assert_eq!(merge.chunks[0].ours, ["Binary file"]);
        let merge = merge_model(&deleted_by_us, false);
        assert_eq!(merge.chunks[0].ours, ["No file on this side"]);
        assert_eq!(merge.chunks[0].theirs, ["a\n", "B\n"]);
    }

    #[test]
    fn text_files_give_their_text() {
        let c = conflict(
            Some("a\nb\nc\n"),
            Some("a\nours\nc\n"),
            Some("a\ntheirs\nc\n"),
        );
        assert!(!c.whole_side());
        let mut merge = merge_model(&c, false);
        assert_eq!(merge_outcome(&c, false, &merge), None, "one conflict open");
        merge.resolve(1, Some(Resolution::OursThenTheirs));
        assert_eq!(
            merge_outcome(&c, false, &merge),
            Some(Outcome::Text("a\nours\ntheirs\nc\n".into()))
        );
        // Both sides added the file: no base, still text.
        let added = conflict(None, Some("x\nours\n"), Some("x\ntheirs\n"));
        assert!(!added.whole_side());
        let mut merge = merge_model(&added, false);
        assert_eq!(merge.open_conflicts(), 1);
        merge.resolve(1, Some(Resolution::Theirs));
        assert_eq!(
            merge_outcome(&added, false, &merge),
            Some(Outcome::Text("x\ntheirs\n".into()))
        );
    }

    /// SPEC-0101 AK-19 (v7).
    #[test]
    fn the_result_lists_conflicts_with_their_lines() {
        let c = conflict(
            Some("a\nb\nc\nd\ne\n"),
            Some("A\nb\nours\nd\nlet x = 1; // one\n"),
            Some("a\nb\ntheirs 1\ntheirs 2\nd\nlet y = 1; // one\n"),
        );
        let mut merge = merge_model(&c, false);
        let (text, conflicts) = merge_result(&merge);
        // The change of ours is applied; the open conflicts hold their base text, no markers.
        assert_eq!(text, "A\nb\nc\nd\ne\n");
        assert_eq!(conflicts.len(), 2);
        assert_eq!(
            (conflicts[0].index, conflicts[0].open, conflicts[0].simple),
            (0, true, false)
        );
        assert_eq!((conflicts[0].start_line, conflicts[0].lines), (3, 1));
        assert_eq!(conflicts[0].base, "c\n");
        assert_eq!(conflicts[0].ours, "ours\n");
        assert_eq!(conflicts[0].theirs, "theirs 1\ntheirs 2\n");
        assert_eq!((conflicts[1].start_line, conflicts[1].lines), (5, 1));
        // A resolved conflict has its text, and the lines after it move.
        let first = merge.conflicts()[0];
        merge.resolve(first, Some(Resolution::Theirs));
        let (text, conflicts) = merge_result(&merge);
        assert_eq!(text, "A\nb\ntheirs 1\ntheirs 2\nd\ne\n");
        assert!(!conflicts[0].open);
        assert_eq!((conflicts[0].start_line, conflicts[0].lines), (3, 2));
        assert_eq!((conflicts[1].start_line, conflicts[1].open), (6, true));
    }

    #[test]
    fn a_rebase_swaps_the_sides() {
        // Stage 2 is the rebase target, stage 3 the replayed commit: the user's own, on the left.
        let c = conflict(Some("a\n"), Some("target\n"), Some("mine\n"));
        let mut merge = merge_model(&c, true);
        assert_eq!(merge.chunks[0].ours, ["mine\n"]);
        merge.resolve(0, Some(Resolution::Ours));
        assert_eq!(
            merge_outcome(&c, true, &merge),
            Some(Outcome::Text("mine\n".into()))
        );
        let deleted_in_target = conflict(Some("a\n"), None, Some("mine\n"));
        let mut merge = merge_model(&deleted_in_target, true);
        assert_eq!(merge.chunks[0].theirs, ["No file on this side"]);
        merge.resolve(0, Some(Resolution::Ours));
        assert_eq!(
            merge_outcome(&deleted_in_target, true, &merge),
            Some(Outcome::Side(Side::Theirs)),
            "left is stage 3"
        );
        merge.resolve(0, Some(Resolution::Theirs));
        assert_eq!(
            merge_outcome(&deleted_in_target, true, &merge),
            Some(Outcome::Side(Side::Ours))
        );
    }
}
