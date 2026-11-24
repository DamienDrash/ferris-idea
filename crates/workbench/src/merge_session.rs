//! The merge session of one conflicted file (`SPEC-0101` v7): the result column as one editable
//! text over the regions of the three-way merge, with the sides taken or ignored per region,
//! undo and redo of its own, and nothing written anywhere until Apply (behaviour 21). The text is
//! a [`TextBuffer`] with one caret (`SPEC-0003`, `SPEC-0025`); every region of the merge owns a
//! stretch of it. A file that takes a whole side (v6 behaviour 12) has no text to edit.

use std::ops::Range;

use ferris_idea_editor::{Caret, Carets, Edit, Motion, Position, TextBuffer};
use ferris_idea_vcs::{
    Chunk, ChunkKind, Conflict, Merge3, Outcome, Resolution, changed_words, has_conflict_markers,
    merge_model, merge_outcome,
};

/// Undo steps kept per session.
const UNDO_LIMIT: usize = 500;

/// What happened to one side of a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideState {
    /// Neither taken nor ignored.
    Pending,
    /// Its text went into the result.
    Taken,
    /// The user ignored it.
    Ignored,
}

/// The stretch of the result one region of the merge owns.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Region {
    /// Chars of the result that belong to it.
    len: usize,
    left: SideState,
    right: SideState,
    /// An edit by hand changed its text (behaviour 17).
    edited: bool,
}

/// Everything but the text that an undo step restores.
#[derive(Debug, Clone, PartialEq)]
struct Meta {
    regions: Vec<Region>,
    caret: Caret,
    /// The side a whole-side file takes.
    whole: Option<Resolution>,
}

struct Step {
    edits: Vec<Edit>,
    before: Meta,
    after: Meta,
    /// Typed text that the next typed text may join (behaviour 18).
    typing: bool,
}

/// One line of the result column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultLine {
    /// Line of the result text, from 0.
    pub line: usize,
    /// Its text without the line end.
    pub text: String,
}

/// What the header counts (behaviour 24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    /// Changes that do not conflict.
    pub changes: usize,
    /// Of those, ignored ones.
    pub ignored: usize,
    /// Conflicts of the file.
    pub conflicts: usize,
    /// Conflicts still open.
    pub open: usize,
}

/// The counter of the header: changes, ignored changes and open conflicts (behaviour 24).
pub fn counter(c: Counts) -> String {
    let conflicts = if c.open == 0 {
        "All conflicts resolved".to_owned()
    } else {
        format!("{} of {} conflicts open", c.open, c.conflicts)
    };
    let changes = match (c.changes, c.ignored) {
        (0, _) => return conflicts,
        (1, 0) => "1 change".to_owned(),
        (n, 0) => format!("{n} changes"),
        (1, i) => format!("1 change ({i} ignored)"),
        (n, i) => format!("{n} changes ({i} ignored)"),
    };
    format!("{changes} · {conflicts}")
}

/// Changed words of the lines of one side column: char ranges per line.
pub type LineWords = Vec<Vec<Range<usize>>>;

/// Changed words of the lines of both side columns of `chunk`, as char ranges per line (behaviour
/// 23): against the base line at the same place when the side has as many lines as the base,
/// else — in a conflict whose sides have equally many lines — against the other side.
pub fn side_words(chunk: &Chunk) -> (LineWords, LineWords) {
    let bare = |l: &String| l.trim_end_matches(['\n', '\r']).to_owned();
    let against = |side: &[String], other: &[String]| -> Vec<Vec<Range<usize>>> {
        let reference = if chunk.kind == ChunkKind::Stable {
            None
        } else if side.len() == chunk.base.len() {
            Some(&chunk.base[..])
        } else if chunk.kind == ChunkKind::Conflict && side.len() == other.len() {
            Some(other)
        } else {
            None
        };
        match reference {
            Some(reference) => side
                .iter()
                .zip(reference)
                .map(|(s, r)| {
                    changed_words(&bare(r), &bare(s))
                        .map(|w| w.1)
                        .unwrap_or_default()
                })
                .collect(),
            None => vec![Vec::new(); side.len()],
        }
    };
    (
        against(&chunk.ours, &chunk.theirs),
        against(&chunk.theirs, &chunk.ours),
    )
}

/// The merge session of one file.
pub struct MergeSession {
    conflict: Conflict,
    rebasing: bool,
    /// The regions with their side texts; the left column is the model's "ours". Its resolutions
    /// are used for a whole-side file only.
    merge: Merge3,
    buf: TextBuffer,
    carets: Carets,
    regions: Vec<Region>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Region and offset the last F7 / ⇧F7 went to, while the caret is still there.
    stepped: Option<(usize, usize)>,
}

impl MergeSession {
    /// A session on `conflict`; in a stopped rebase the left column is Git's stage 3.
    pub fn new(conflict: &Conflict, rebasing: bool) -> Self {
        let merge = merge_model(conflict, rebasing);
        let whole = conflict.whole_side();
        let mut text = String::new();
        let regions = merge
            .chunks
            .iter()
            .map(|chunk| {
                let (part, left, right) = match chunk.kind {
                    _ if whole => (String::new(), SideState::Pending, SideState::Pending),
                    ChunkKind::Stable => {
                        (chunk.ours.concat(), SideState::Pending, SideState::Pending)
                    }
                    ChunkKind::Ours => (chunk.ours.concat(), SideState::Taken, SideState::Pending),
                    ChunkKind::Theirs => {
                        (chunk.theirs.concat(), SideState::Pending, SideState::Taken)
                    }
                    ChunkKind::Both => (chunk.ours.concat(), SideState::Taken, SideState::Taken),
                    // An open conflict holds its base text (behaviour 15).
                    ChunkKind::Conflict => {
                        (chunk.base.concat(), SideState::Pending, SideState::Pending)
                    }
                };
                let len = part.chars().count();
                text.push_str(&part);
                Region {
                    len,
                    left,
                    right,
                    edited: false,
                }
            })
            .collect();
        let mut buf = TextBuffer::from_text(&text);
        buf.track_changes();
        Self {
            conflict: conflict.clone(),
            rebasing,
            merge,
            buf,
            carets: Carets::new(0),
            regions,
            undo: Vec::new(),
            redo: Vec::new(),
            stepped: None,
        }
    }

    /// Path of the file in the repository.
    pub fn path(&self) -> &str {
        &self.conflict.path
    }

    /// Whether the file takes a whole side and has no text to edit (behaviour 22).
    pub fn whole_side(&self) -> bool {
        self.conflict.whole_side()
    }

    /// The regions of the merge with the lines of the side columns.
    pub fn chunks(&self) -> &[Chunk] {
        &self.merge.chunks
    }

    /// The result text as it stands (behaviour 20: what Apply writes once nothing is open).
    pub fn text(&self) -> String {
        self.buf.text()
    }

    /// The caret with its selection.
    pub fn caret(&self) -> Caret {
        self.carets.primary()
    }

    /// Line and column of the caret.
    pub fn caret_position(&self) -> Position {
        self.position(self.caret().head)
    }

    /// Line and column of char `offset`.
    pub fn position(&self, offset: usize) -> Position {
        self.buf
            .offset_to_position(offset.min(self.buf.len_chars()))
            .unwrap_or(Position::new(0, 0))
    }

    fn start(&self, index: usize) -> usize {
        self.regions[..index].iter().map(|r| r.len).sum()
    }

    /// The region char `offset` lies in; the last one at the end of the text.
    fn region_of(&self, offset: usize) -> usize {
        let mut start = 0;
        for (i, r) in self.regions.iter().enumerate() {
            if offset < start + r.len {
                return i;
            }
            start += r.len;
        }
        self.regions.len().saturating_sub(1)
    }

    /// The region typed text at `offset` goes to: an open conflict with an empty region at that
    /// place first (behaviour 17), else the region the offset lies in.
    fn insertion_region(&self, offset: usize) -> usize {
        let mut start = 0;
        for (i, r) in self.regions.iter().enumerate() {
            if r.len == 0 && start == offset && self.is_open(i) {
                return i;
            }
            start += r.len;
        }
        self.region_of(offset)
    }

    fn region_text(&self, index: usize) -> String {
        let start = self.start(index);
        self.buf
            .slice(start..start + self.regions[index].len)
            .unwrap_or_default()
    }

    /// Whether region `index` is a conflict without a resolution.
    pub fn is_open(&self, index: usize) -> bool {
        let Some(r) = self.regions.get(index) else {
            return false;
        };
        if self.merge.chunks[index].kind != ChunkKind::Conflict {
            return false;
        }
        if self.whole_side() {
            return merge_outcome(&self.conflict, self.rebasing, &self.merge).is_none();
        }
        !(r.edited
            || r.left == SideState::Taken
            || r.right == SideState::Taken
            || (r.left == SideState::Ignored && r.right == SideState::Ignored))
    }

    /// Conflicts still open.
    pub fn open_conflicts(&self) -> usize {
        (0..self.regions.len()).filter(|&i| self.is_open(i)).count()
    }

    /// Number of conflicts of the file.
    pub fn conflict_count(&self) -> usize {
        self.merge.conflicts().len()
    }

    /// What the header counts (behaviour 24).
    pub fn counts(&self) -> Counts {
        let change =
            |k: ChunkKind| matches!(k, ChunkKind::Ours | ChunkKind::Theirs | ChunkKind::Both);
        let changes = self.merge.chunks.iter().filter(|c| change(c.kind)).count();
        let ignored = (0..self.regions.len())
            .filter(|&i| change(self.merge.chunks[i].kind) && !self.applied(i))
            .count();
        Counts {
            changes,
            ignored,
            conflicts: self.conflict_count(),
            open: self.open_conflicts(),
        }
    }

    /// Whether the change of region `index` (one that does not conflict) is in the result.
    fn applied(&self, index: usize) -> bool {
        let r = &self.regions[index];
        r.left == SideState::Taken || r.right == SideState::Taken || r.edited
    }

    /// The state of the left (`true`) or right side of region `index`.
    pub fn side_state(&self, index: usize, left: bool) -> SideState {
        match self.regions.get(index) {
            Some(r) if left => r.left,
            Some(r) => r.right,
            None => SideState::Pending,
        }
    }

    /// Whether the gutter offers to take the side of region `index` (behaviour 16).
    pub fn can_accept(&self, index: usize, left: bool) -> bool {
        let Some(chunk) = self.merge.chunks.get(index) else {
            return false;
        };
        let has_side = match chunk.kind {
            ChunkKind::Stable => false,
            ChunkKind::Ours => left,
            ChunkKind::Theirs => !left,
            ChunkKind::Both | ChunkKind::Conflict => true,
        };
        if self.whole_side() {
            let taken = self.merge.chunks[0].resolution.as_ref();
            return taken
                != Some(if left {
                    &Resolution::Ours
                } else {
                    &Resolution::Theirs
                });
        }
        has_side && self.side_state(index, left) != SideState::Taken
    }

    /// Whether the gutter offers to ignore the side of region `index` (behaviour 16).
    pub fn can_ignore(&self, index: usize, left: bool) -> bool {
        if self.whole_side() {
            return false;
        }
        let Some(chunk) = self.merge.chunks.get(index) else {
            return false;
        };
        let state = self.side_state(index, left);
        match chunk.kind {
            ChunkKind::Stable => false,
            ChunkKind::Conflict => self.is_open(index) && state == SideState::Pending,
            ChunkKind::Ours => left && state == SideState::Taken,
            ChunkKind::Theirs => !left && state == SideState::Taken,
            ChunkKind::Both => state == SideState::Taken,
        }
    }

    /// Whether choices or edits were made that are not applied (behaviour 25).
    pub fn is_dirty(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether the result has conflict markers (behaviour 20).
    pub fn has_markers(&self) -> bool {
        !self.whole_side() && has_conflict_markers(&self.buf.text())
    }

    /// What Apply writes; `None` while a conflict is open. A whole-side file gives the side, never
    /// a text (v6 behaviour 12); any other file exactly the result text (behaviour 20).
    pub fn outcome(&self) -> Option<Outcome> {
        if self.whole_side() {
            return merge_outcome(&self.conflict, self.rebasing, &self.merge);
        }
        (self.open_conflicts() == 0).then(|| Outcome::Text(self.buf.text()))
    }

    /// The lines of the result column region by region: a line belongs to the region its first
    /// char lies in. The empty line after a final line break is listed while the caret is on it.
    pub fn result_lines(&self) -> Vec<Vec<ResultLine>> {
        let mut out = vec![Vec::new(); self.regions.len()];
        if self.whole_side() || self.regions.is_empty() {
            return out;
        }
        let text = self.buf.text();
        let mut offset = 0;
        let mut region = 0;
        let mut end = self.regions[0].len;
        let mut line = 0;
        for l in text.split_inclusive('\n') {
            while offset >= end && region + 1 < self.regions.len() {
                region += 1;
                end += self.regions[region].len;
            }
            out[region].push(ResultLine {
                line,
                text: l.trim_end_matches(['\n', '\r']).to_owned(),
            });
            offset += l.chars().count();
            line += 1;
        }
        let last_line_open = text.is_empty() || text.ends_with('\n');
        if last_line_open && self.caret_position().line == line {
            let last = out.len() - 1;
            out[last].push(ResultLine {
                line,
                text: String::new(),
            });
        }
        out
    }

    fn meta(&self) -> Meta {
        Meta {
            regions: self.regions.clone(),
            caret: self.carets.primary(),
            whole: self
                .whole_side()
                .then(|| self.merge.chunks.first().and_then(|c| c.resolution.clone()))
                .flatten(),
        }
    }

    fn restore(&mut self, meta: &Meta) {
        self.regions = meta.regions.clone();
        self.carets = Carets::from_carets(vec![meta.caret]);
        if self.whole_side()
            && let Some(chunk) = self.merge.chunks.first_mut()
        {
            chunk.resolution = meta.whole.clone();
        }
        self.stepped = None;
    }

    fn push(&mut self, step: Step) {
        self.redo.clear();
        if step.typing
            && let Some(last) = self.undo.last_mut()
            && last.typing
            && last.after.caret == step.before.caret
        {
            last.edits.extend(step.edits);
            last.after = step.after;
            return;
        }
        self.undo.push(step);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    /// Moves the lengths of the regions after `edit` changed the text, and marks every region it
    /// changed as edited by hand (behaviour 17).
    fn follow(&mut self, edit: &Edit) {
        let deleted = edit.deleted.chars().count();
        let inserted = edit.inserted.chars().count();
        let (from, to) = (edit.offset, edit.offset + deleted);
        let target = if deleted == 0 {
            Some(self.insertion_region(from))
        } else {
            None
        };
        let mut target = target;
        let mut start = 0;
        for (i, r) in self.regions.iter_mut().enumerate() {
            let end = start + r.len;
            let cut = end.min(to).saturating_sub(start.max(from));
            if cut > 0 {
                r.len -= cut;
                r.edited = true;
                target.get_or_insert(i);
            }
            start = end;
        }
        if inserted > 0
            && let Some(r) = target.and_then(|i| self.regions.get_mut(i))
        {
            r.len += inserted;
            r.edited = true;
        }
    }

    /// An edit by hand through the caret; nothing in a whole-side file (behaviour 22).
    fn edit(&mut self, typing: bool, change: impl FnOnce(&mut Carets, &mut TextBuffer)) -> bool {
        if self.whole_side() {
            return false;
        }
        let before = self.meta();
        change(&mut self.carets, &mut self.buf);
        let edits = self.buf.take_changes();
        if edits.is_empty() {
            return false;
        }
        for e in &edits {
            self.follow(e);
        }
        self.stepped = None;
        let after = self.meta();
        self.push(Step {
            edits,
            before,
            after,
            typing,
        });
        true
    }

    /// Types `text` at the caret, replacing the selection. Typed text without a line break joins
    /// the step of the text typed just before (behaviour 18).
    pub fn type_text(&mut self, text: &str) -> bool {
        let typing = !text.contains('\n') && self.caret().is_empty();
        self.edit(typing, |carets, buf| carets.type_text(buf, text))
    }

    /// Enter: the file's line end at the caret.
    pub fn newline(&mut self) -> bool {
        let eol = self.buf.line_ending().as_str();
        self.edit(false, |carets, buf| carets.type_text(buf, eol))
    }

    /// Deletes the selection or the char before the caret.
    pub fn backspace(&mut self) -> bool {
        self.edit(false, |carets, buf| carets.backspace(buf))
    }

    /// Deletes the selection or the char after the caret.
    pub fn delete(&mut self) -> bool {
        self.edit(false, |carets, buf| carets.delete(buf))
    }

    /// Pastes `text` at the caret, replacing the selection.
    pub fn paste(&mut self, text: &str) -> bool {
        self.edit(false, |carets, buf| carets.type_text(buf, text))
    }

    /// The selected text; `None` without a selection.
    pub fn copy(&self) -> Option<String> {
        (!self.whole_side() && !self.caret().is_empty()).then(|| self.carets.copy(&self.buf))
    }

    /// Removes the selected text and returns it; `None` without a selection.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        self.backspace();
        Some(text)
    }

    /// Selects the whole result.
    pub fn select_all(&mut self) {
        if !self.whole_side() {
            self.carets = Carets::from_carets(vec![Caret::select(0, self.buf.len_chars())]);
            self.stepped = None;
        }
    }

    /// Moves the caret; with `extend` the selection grows.
    pub fn move_caret(&mut self, motion: Motion, extend: bool) {
        if !self.whole_side() {
            self.carets.move_all(&self.buf, motion, extend);
            self.stepped = None;
        }
    }

    /// Puts the caret on `line` at `column` (cut to the line's length); with `extend` the
    /// selection reaches from where it started to there.
    pub fn set_caret(&mut self, line: usize, column: usize, extend: bool) {
        if self.whole_side() {
            return;
        }
        let line = line.min(self.buf.len_lines().saturating_sub(1));
        let width = self.buf.line(line).map_or(0, |l| l.chars().count());
        let head = self
            .buf
            .position_to_offset(Position::new(line, column.min(width)))
            .unwrap_or(self.buf.len_chars());
        let anchor = if extend { self.caret().anchor } else { head };
        self.carets = Carets::from_carets(vec![Caret::select(anchor, head)]);
        self.stepped = None;
    }

    /// A step that is a choice, not typing: `change` sets regions with [`Self::set_region`].
    fn choice(&mut self, change: impl FnOnce(&mut Self)) -> bool {
        let before = self.meta();
        change(self);
        let edits = self.buf.take_changes();
        let after = self.meta();
        if edits.is_empty() && before == after {
            return false;
        }
        self.stepped = None;
        self.push(Step {
            edits,
            before,
            after,
            typing: false,
        });
        true
    }

    /// Gives region `index` the text `text` and puts the caret at its start.
    fn set_region(&mut self, index: usize, text: &str) {
        let start = self.start(index);
        let len = self.regions[index].len;
        if self.buf.replace(start..start + len, text).is_ok() {
            self.regions[index].len = text.chars().count();
        }
        self.carets = Carets::new(start);
    }

    /// `text` closed with the file's line end unless it is empty or ends with a line break.
    fn with_break(&self, mut text: String) -> String {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(self.buf.line_ending().as_str());
        }
        text
    }

    fn side_text(&self, index: usize, left: bool) -> String {
        let chunk = &self.merge.chunks[index];
        if left {
            chunk.ours.concat()
        } else {
            chunk.theirs.concat()
        }
    }

    /// Takes the left (`true`) or right side of region `index` (behaviour 16): in an open
    /// conflict or an ignored change it replaces the base text, in a resolved conflict it is
    /// appended. A whole-side file takes that side as a whole.
    pub fn accept(&mut self, index: usize, left: bool) -> bool {
        if !self.can_accept(index, left) {
            return false;
        }
        if self.whole_side() {
            return self.choice(|s| {
                s.merge.chunks[0].resolution = Some(if left {
                    Resolution::Ours
                } else {
                    Resolution::Theirs
                });
            });
        }
        let side = self.side_text(index, left);
        let both = self.merge.chunks[index].kind == ChunkKind::Both;
        self.choice(|s| {
            let r = &s.regions[index];
            let untouched = !r.edited && r.left != SideState::Taken && r.right != SideState::Taken;
            let text = if untouched {
                side
            } else {
                s.with_break(s.region_text(index)) + &side
            };
            s.set_region(index, &text);
            let r = &mut s.regions[index];
            if left || both {
                r.left = SideState::Taken;
            }
            if !left || both {
                r.right = SideState::Taken;
            }
        })
    }

    /// Ignores the left (`true`) or right side of region `index` (behaviour 16): an open conflict
    /// stays open for the other side, and holds its base text when both are ignored; a change
    /// that does not conflict gets its base text back.
    pub fn ignore(&mut self, index: usize, left: bool) -> bool {
        if !self.can_ignore(index, left) {
            return false;
        }
        let conflict = self.merge.chunks[index].kind == ChunkKind::Conflict;
        let base = self.merge.chunks[index].base.concat();
        self.choice(|s| {
            if conflict {
                let r = &mut s.regions[index];
                if left {
                    r.left = SideState::Ignored;
                } else {
                    r.right = SideState::Ignored;
                }
                return;
            }
            s.set_region(index, &base);
            let r = &mut s.regions[index];
            r.edited = false;
            if r.left == SideState::Taken {
                r.left = SideState::Ignored;
            }
            if r.right == SideState::Taken {
                r.right = SideState::Ignored;
            }
        })
    }

    fn resolve_region(&mut self, index: usize, resolution: &Resolution) {
        let chunk = &self.merge.chunks[index];
        let (ours, theirs) = (chunk.ours.concat(), chunk.theirs.concat());
        let (text, left, right, edited) = match resolution {
            Resolution::Ours => (ours, SideState::Taken, SideState::Pending, false),
            Resolution::Theirs => (theirs, SideState::Pending, SideState::Taken, false),
            Resolution::OursThenTheirs => (
                self.with_break(ours) + &theirs,
                SideState::Taken,
                SideState::Taken,
                false,
            ),
            Resolution::TheirsThenOurs => (
                self.with_break(theirs) + &ours,
                SideState::Taken,
                SideState::Taken,
                false,
            ),
            Resolution::Base => (
                chunk.base.concat(),
                SideState::Ignored,
                SideState::Ignored,
                false,
            ),
            Resolution::Custom(text) => {
                (text.clone(), SideState::Pending, SideState::Pending, true)
            }
        };
        self.set_region(index, &text);
        let r = &mut self.regions[index];
        (r.left, r.right, r.edited) = (left, right, edited);
    }

    /// Resolves conflict `n` (0-based among the conflicts) with `resolution`, replacing what its
    /// region holds. A whole-side file takes the left or the right side, nothing else.
    pub fn resolve(&mut self, n: usize, resolution: Resolution) -> bool {
        let Some(&index) = self.merge.conflicts().get(n) else {
            return false;
        };
        if self.whole_side() {
            if !matches!(resolution, Resolution::Ours | Resolution::Theirs) {
                return false;
            }
            return self.choice(|s| s.merge.chunks[index].resolution = Some(resolution));
        }
        self.choice(|s| s.resolve_region(index, &resolution))
    }

    /// Takes one side for every open conflict (Accept Left / Accept Right), as one step.
    pub fn accept_all(&mut self, left: bool) -> bool {
        let resolution = if left {
            Resolution::Ours
        } else {
            Resolution::Theirs
        };
        if self.whole_side() {
            return self.resolve(0, resolution);
        }
        let open: Vec<usize> = (0..self.regions.len())
            .filter(|&i| self.is_open(i))
            .collect();
        self.choice(|s| {
            // From the last to the first, so the caret ends on the first conflict.
            for &i in open.iter().rev() {
                s.resolve_region(i, &resolution);
            }
        })
    }

    /// Resolves the open simple conflicts word by word (v3 behaviour 6), as one step; returns how
    /// many.
    pub fn resolve_simple(&mut self) -> usize {
        if self.whole_side() {
            return 0;
        }
        let simple: Vec<(usize, String)> = (0..self.regions.len())
            .filter(|&i| self.is_open(i))
            .filter_map(|i| Some((i, self.merge.simple_text(i)?)))
            .collect();
        self.choice(|s| {
            for (i, text) in simple.iter().rev() {
                s.set_region(*i, text);
                let r = &mut s.regions[*i];
                (r.left, r.right, r.edited) = (SideState::Taken, SideState::Taken, false);
            }
        });
        simple.len()
    }

    fn apply_edits(&mut self, edits: &[Edit], back: bool) {
        if back {
            for e in edits.iter().rev() {
                let end = e.offset + e.inserted.chars().count();
                let _ = self.buf.replace(e.offset..end, &e.deleted);
            }
        } else {
            for e in edits {
                let end = e.offset + e.deleted.chars().count();
                let _ = self.buf.replace(e.offset..end, &e.inserted);
            }
        }
        // These are no edits by hand: the regions come from the step.
        self.buf.take_changes();
    }

    /// Takes the last step back: text, states of the regions and caret (behaviour 18).
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        self.apply_edits(&step.edits, true);
        self.restore(&step.before);
        self.redo.push(step);
        true
    }

    /// Does the step taken back last again.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        self.apply_edits(&step.edits, false);
        self.restore(&step.after);
        self.undo.push(step);
        true
    }

    /// Moves the caret to the start of the next (`forward`) or previous change or conflict
    /// (behaviour 24); false at the last or first one.
    pub fn step(&mut self, forward: bool) -> bool {
        if self.whole_side() {
            return false;
        }
        let head = self.caret().head;
        let here = match self.stepped {
            Some((region, offset)) if offset == head => region,
            _ => self.region_of(head),
        };
        let changed = |i: &usize| self.merge.chunks[*i].kind != ChunkKind::Stable;
        let target = if forward {
            (here + 1..self.regions.len()).find(changed)
        } else {
            (0..here).rev().find(changed)
        };
        let Some(target) = target else {
            return false;
        };
        let offset = self.start(target);
        self.carets = Carets::new(offset);
        self.stepped = Some((target, offset));
        true
    }

    /// The region the caret is in (the one the last step went to while the caret stays there).
    pub fn caret_region(&self) -> usize {
        let head = self.caret().head;
        match self.stepped {
            Some((region, offset)) if offset == head => region,
            _ => self.region_of(head),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(base: &str, ours: &str, theirs: &str) -> Conflict {
        Conflict {
            path: "f.txt".into(),
            base: Some(base.to_owned()),
            ours: Some(ours.to_owned()),
            theirs: Some(theirs.to_owned()),
            has_base: true,
            has_ours: true,
            has_theirs: true,
            binary: false,
        }
    }

    /// Ours changes line 1, both change line 3 differently, theirs changes line 5.
    fn session() -> MergeSession {
        MergeSession::new(
            &conflict(
                "one\ntwo\nthree\nfour\nfive\n",
                "ONE\ntwo\nthree (ours)\nfour\nfive\n",
                "one\ntwo\nthree (theirs)\nfour\nFIVE\n",
            ),
            false,
        )
    }

    /// Region of the conflict in [`session`].
    const CONFLICT: usize = 2;

    fn kinds(s: &MergeSession) -> Vec<ChunkKind> {
        s.chunks().iter().map(|c| c.kind).collect()
    }

    // SPEC-0101 AK-13 (v7).
    #[test]
    fn the_result_starts_with_changes_applied_and_the_base_in_a_conflict() {
        let s = session();
        assert_eq!(
            kinds(&s),
            [
                ChunkKind::Ours,
                ChunkKind::Stable,
                ChunkKind::Conflict,
                ChunkKind::Stable,
                ChunkKind::Theirs
            ]
        );
        assert_eq!(s.text(), "ONE\ntwo\nthree\nfour\nFIVE\n");
        assert_eq!(s.open_conflicts(), 1);
        assert_eq!(
            s.outcome(),
            None,
            "nothing to apply while a conflict is open"
        );
        assert!(!s.is_dirty());
        let lines = s.result_lines();
        assert_eq!(
            lines[CONFLICT],
            [ResultLine {
                line: 2,
                text: "three".into()
            }]
        );
        assert_eq!(lines[4][0].text, "FIVE");
    }

    // SPEC-0101 AK-13 (v7).
    #[test]
    fn typing_inside_an_open_conflict_resolves_it_by_hand() {
        let mut s = session();
        s.set_caret(2, 5, false);
        assert!(s.type_text("!"));
        assert_eq!(s.text(), "ONE\ntwo\nthree!\nfour\nFIVE\n");
        assert_eq!(s.open_conflicts(), 0);
        assert_eq!(
            s.outcome(),
            Some(Outcome::Text("ONE\ntwo\nthree!\nfour\nFIVE\n".into()))
        );
        assert!(s.is_dirty());
        // A conflict without base text takes what is typed at its place.
        let mut s = MergeSession::new(&conflict("a\nz\n", "a\nours\nz\n", "a\ntheirs\nz\n"), false);
        assert_eq!(s.text(), "a\nz\n");
        assert_eq!(s.open_conflicts(), 1);
        s.set_caret(1, 0, false);
        s.type_text("mine");
        s.newline();
        assert_eq!(s.text(), "a\nmine\nz\n");
        assert_eq!(s.open_conflicts(), 0);
        assert_eq!(
            s.result_lines()[1].len(),
            1,
            "the typed line is the conflict's"
        );
    }

    // SPEC-0101 AK-13 (v7).
    #[test]
    fn typing_elsewhere_leaves_the_conflict_open() {
        let mut s = session();
        s.set_caret(1, 3, false);
        s.type_text(" 2");
        s.set_caret(0, 0, false);
        s.delete();
        assert_eq!(s.text(), "NE\ntwo 2\nthree\nfour\nFIVE\n");
        assert_eq!(s.open_conflicts(), 1);
        assert!(s.is_dirty());
        // The conflict still knows its place: the left side lands where the base text was.
        assert!(s.accept(CONFLICT, true));
        assert_eq!(s.text(), "NE\ntwo 2\nthree (ours)\nfour\nFIVE\n");
    }

    // SPEC-0101 AK-13 (v7).
    #[test]
    fn an_edit_across_regions_counts_for_each() {
        let mut s = session();
        // From inside "two" to inside "three": the stable region and the conflict both change.
        s.set_caret(1, 1, false);
        s.set_caret(2, 2, true);
        assert!(s.backspace());
        assert_eq!(s.text(), "ONE\ntree\nfour\nFIVE\n");
        assert_eq!(
            s.open_conflicts(),
            0,
            "the conflict's text was changed by hand"
        );
        assert!(s.undo());
        assert_eq!(s.text(), "ONE\ntwo\nthree\nfour\nFIVE\n");
        assert_eq!(s.open_conflicts(), 1);
    }

    // SPEC-0101 AK-13 (v7).
    #[test]
    fn apply_text_is_the_result_text_with_its_line_ends() {
        let mut s = MergeSession::new(
            &conflict("a\r\nb\r\n", "a\r\nours\r\n", "a\r\ntheirs\r\n"),
            false,
        );
        s.accept(1, false);
        s.move_caret(Motion::DocumentEnd, false);
        s.type_text("end");
        s.newline();
        assert_eq!(
            s.outcome(),
            Some(Outcome::Text("a\r\ntheirs\r\nend\r\n".into()))
        );
        assert_eq!(s.text(), "a\r\ntheirs\r\nend\r\n");
    }

    // SPEC-0101 AK-13 (v7).
    #[test]
    fn paste_and_cut_change_the_result() {
        let mut s = session();
        assert_eq!(s.copy(), None, "nothing selected");
        s.set_caret(3, 0, false);
        s.set_caret(4, 0, true);
        assert_eq!(s.cut().as_deref(), Some("four\n"));
        assert_eq!(s.text(), "ONE\ntwo\nthree\nFIVE\n");
        s.move_caret(Motion::DocumentStart, false);
        assert!(s.paste("four\n"));
        assert_eq!(s.text(), "four\nONE\ntwo\nthree\nFIVE\n");
        s.select_all();
        assert_eq!(s.copy().as_deref(), Some("four\nONE\ntwo\nthree\nFIVE\n"));
        assert_eq!(
            s.open_conflicts(),
            1,
            "the conflict's own text is untouched"
        );
    }

    // SPEC-0101 AK-14 (v7).
    #[test]
    fn taking_a_side_resolves_in_one_step_and_the_other_appends() {
        let mut s = session();
        assert!(s.can_accept(CONFLICT, true) && s.can_ignore(CONFLICT, true));
        assert!(s.accept(CONFLICT, true));
        assert_eq!(s.text(), "ONE\ntwo\nthree (ours)\nfour\nFIVE\n");
        assert_eq!(s.open_conflicts(), 0, "IMP-0039: one step");
        assert_eq!(s.caret_position(), Position::new(2, 0));
        assert!(!s.can_accept(CONFLICT, true) && !s.can_ignore(CONFLICT, false));
        // The other side is still on offer and is appended.
        assert!(s.can_accept(CONFLICT, false));
        assert!(s.accept(CONFLICT, false));
        assert_eq!(
            s.text(),
            "ONE\ntwo\nthree (ours)\nthree (theirs)\nfour\nFIVE\n"
        );
        assert!(!s.accept(CONFLICT, false), "taken once");
        // After an edit by hand a side is appended too, never put over the typed text.
        let mut s = session();
        s.set_caret(2, 0, false);
        s.type_text("my ");
        s.accept(CONFLICT, false);
        assert_eq!(s.text(), "ONE\ntwo\nmy three\nthree (theirs)\nfour\nFIVE\n");
        // The inserted side is ordinary text.
        s.set_caret(3, 0, false);
        s.type_text("> ");
        assert_eq!(s.result_lines()[CONFLICT][1].text, "> three (theirs)");
        // The old way of choosing: both at once, and a side over what is there.
        let mut s = session();
        s.resolve(0, Resolution::TheirsThenOurs);
        assert_eq!(
            s.text(),
            "ONE\ntwo\nthree (theirs)\nthree (ours)\nfour\nFIVE\n"
        );
        s.resolve(0, Resolution::Ours);
        assert_eq!(s.text(), "ONE\ntwo\nthree (ours)\nfour\nFIVE\n");
    }

    // SPEC-0101 AK-14 (v7).
    #[test]
    fn ignoring_both_sides_leaves_the_base() {
        let mut s = session();
        assert!(s.ignore(CONFLICT, true));
        assert_eq!(s.open_conflicts(), 1, "open for the other side");
        assert!(!s.can_ignore(CONFLICT, true) && s.can_accept(CONFLICT, true));
        assert!(s.can_ignore(CONFLICT, false));
        assert!(s.ignore(CONFLICT, false));
        assert_eq!(s.open_conflicts(), 0);
        assert_eq!(s.text(), "ONE\ntwo\nthree\nfour\nFIVE\n");
        // A side taken after all replaces the base text.
        assert!(s.accept(CONFLICT, false));
        assert_eq!(s.text(), "ONE\ntwo\nthree (theirs)\nfour\nFIVE\n");
        // Ignoring one side and taking the other.
        let mut s = session();
        s.ignore(CONFLICT, false);
        s.accept(CONFLICT, true);
        assert_eq!(s.text(), "ONE\ntwo\nthree (ours)\nfour\nFIVE\n");
        assert_eq!(s.open_conflicts(), 0);
    }

    // SPEC-0101 AK-14 (v7).
    #[test]
    fn an_ignored_change_gets_the_base_and_comes_back() {
        let mut s = session();
        assert!(!s.can_ignore(0, false), "the change is the left side's");
        assert!(!s.can_accept(0, true), "applied from the start");
        assert!(s.ignore(0, true));
        assert_eq!(s.text(), "one\ntwo\nthree\nfour\nFIVE\n");
        assert_eq!(s.counts().ignored, 1);
        assert!(s.can_accept(0, true) && !s.can_ignore(0, true));
        assert!(s.accept(0, true));
        assert_eq!(s.text(), "ONE\ntwo\nthree\nfour\nFIVE\n");
        assert_eq!(s.counts().ignored, 0);
        assert!(s.ignore(4, false));
        assert_eq!(s.text(), "ONE\ntwo\nthree\nfour\nfive\n");
        assert!(
            !s.ignore(1, true),
            "an unchanged region has nothing to ignore"
        );
        assert_eq!(s.open_conflicts(), 1);
    }

    // SPEC-0101 AK-15 (v7).
    #[test]
    fn undo_and_redo_restore_text_states_and_caret() {
        let mut s = session();
        let start = s.text();
        s.set_caret(1, 3, false);
        s.type_text("!");
        s.accept(CONFLICT, true);
        s.accept(CONFLICT, false);
        s.ignore(0, true);
        let end = s.text();
        assert_eq!(end, "one\ntwo!\nthree (ours)\nthree (theirs)\nfour\nFIVE\n");
        assert!(s.undo());
        assert_eq!(s.counts().ignored, 0);
        assert!(s.undo());
        assert_eq!(s.text(), "ONE\ntwo!\nthree (ours)\nfour\nFIVE\n");
        assert!(s.can_accept(CONFLICT, false));
        assert!(s.undo());
        assert_eq!(s.open_conflicts(), 1, "the conflict is open again");
        assert_eq!(s.text(), "ONE\ntwo!\nthree\nfour\nFIVE\n");
        assert_eq!(
            s.caret_position(),
            Position::new(1, 4),
            "where the caret was"
        );
        assert!(s.undo());
        assert_eq!(s.text(), start);
        assert_eq!(s.caret_position(), Position::new(1, 3));
        assert!(!s.undo());
        assert!(!s.is_dirty());
        for _ in 0..4 {
            assert!(s.redo());
        }
        assert!(!s.redo());
        assert_eq!(s.text(), end);
        assert_eq!(s.open_conflicts(), 0);
        // Accept Left and Resolve Simple are one step each.
        let mut s = MergeSession::new(
            &conflict(
                "let total = price * count;\nkeep\nvalue = 1\n",
                "let sum = price * count;\nkeep\nvalue = 2\n",
                "let total = price * amount;\nkeep\nvalue = 3\n",
            ),
            false,
        );
        assert_eq!(s.open_conflicts(), 2);
        assert_eq!(s.resolve_simple(), 1);
        assert_eq!(s.text(), "let sum = price * amount;\nkeep\nvalue = 1\n");
        assert!(s.accept_all(false));
        assert_eq!(s.text(), "let sum = price * amount;\nkeep\nvalue = 3\n");
        assert_eq!(s.open_conflicts(), 0);
        s.undo();
        assert_eq!(s.open_conflicts(), 1);
        s.undo();
        assert_eq!(s.open_conflicts(), 2);
        assert_eq!(
            s.text(),
            "let total = price * count;\nkeep\nvalue = 1\n",
            "base text again"
        );
        // A new edit after an undo drops what could be redone.
        s.type_text("x");
        assert!(!s.redo());
    }

    // SPEC-0101 AK-15 (v7).
    #[test]
    fn typing_in_a_row_is_one_step() {
        let mut s = session();
        s.set_caret(1, 3, false);
        for c in ["a", "b", "c"] {
            s.type_text(c);
        }
        s.newline();
        s.type_text("d");
        assert_eq!(s.text(), "ONE\ntwoabc\nd\nthree\nfour\nFIVE\n");
        s.undo();
        assert_eq!(s.text(), "ONE\ntwoabc\n\nthree\nfour\nFIVE\n");
        s.undo();
        assert_eq!(s.text(), "ONE\ntwoabc\nthree\nfour\nFIVE\n");
        s.undo();
        assert_eq!(s.text(), "ONE\ntwo\nthree\nfour\nFIVE\n");
        // Typing somewhere else starts a new step.
        s.type_text("a");
        s.set_caret(0, 0, false);
        s.type_text("b");
        s.undo();
        assert_eq!(s.text(), "ONE\ntwoa\nthree\nfour\nFIVE\n");
    }

    fn one(range: Range<usize>) -> Vec<Range<usize>> {
        [range].to_vec()
    }

    // SPEC-0101 AK-16 (v7).
    #[test]
    fn changed_words_against_the_base_or_the_other_side() {
        let s = MergeSession::new(
            &conflict(
                "value = 1\nkeep\nold line\n",
                "value = 2\nkeep\nlet a = compute(x);\nlet b = 1;\n",
                "value = 3\nkeep\nlet a = compute(y);\nlet c = 1;\n",
            ),
            false,
        );
        let chunks = s.chunks();
        assert_eq!(chunks[0].kind, ChunkKind::Conflict);
        // As many lines as the base: against the base.
        let (ours, theirs) = side_words(&chunks[0]);
        assert_eq!(ours, [one(8..9)]);
        assert_eq!(theirs, [one(8..9)]);
        // Another number of lines than the base, but as many as the other side: against it.
        let last = chunks.last().expect("regions");
        assert_eq!(last.kind, ChunkKind::Conflict);
        let (ours, theirs) = side_words(last);
        assert_eq!(ours, [one(16..17), one(4..5)]);
        assert_eq!(theirs, [one(16..17), one(4..5)]);
        // Unchanged regions and lines that differ too much have none.
        assert_eq!(side_words(&chunks[1]), (vec![vec![]], vec![vec![]]));
        let s = MergeSession::new(&conflict("alpha\n", "beta gamma\n", "delta\n"), false);
        assert_eq!(side_words(&s.chunks()[0]), (vec![vec![]], vec![vec![]]));
    }

    // SPEC-0101 AK-17 (v7).
    #[test]
    fn stepping_goes_region_by_region_and_stops_at_the_ends() {
        let mut s = session();
        s.set_caret(1, 1, false);
        assert!(s.step(true));
        assert_eq!((s.caret_region(), s.caret_position().line), (CONFLICT, 2));
        assert!(s.step(true));
        assert_eq!((s.caret_region(), s.caret_position().line), (4, 4));
        assert!(!s.step(true), "the last one");
        assert!(s.step(false));
        assert_eq!(s.caret_region(), CONFLICT);
        assert!(s.step(false));
        assert_eq!((s.caret_region(), s.caret_position().line), (0, 0));
        assert!(!s.step(false), "the first one");
        // A change that left no line in the result is still a stop, forwards and back.
        let mut s = MergeSession::new(
            &conflict("a\ngone\nb\nc\n", "a\nb\nc\n", "a\ngone\nb\nC\n"),
            false,
        );
        assert_eq!(s.text(), "a\nb\nC\n");
        assert!(s.step(true));
        assert_eq!((s.caret_region(), s.caret_position().line), (1, 1));
        assert!(s.step(true));
        assert_eq!(s.caret_position().line, 2);
        assert!(s.step(false));
        assert_eq!((s.caret_region(), s.caret_position().line), (1, 1));
        assert!(!s.step(false));
    }

    // SPEC-0101 AK-17 (v7).
    #[test]
    fn the_counter_names_changes_and_open_conflicts() {
        let mut s = session();
        assert_eq!(
            s.counts(),
            Counts {
                changes: 2,
                ignored: 0,
                conflicts: 1,
                open: 1
            }
        );
        assert_eq!(counter(s.counts()), "2 changes · 1 of 1 conflicts open");
        s.ignore(0, true);
        s.accept(CONFLICT, true);
        assert_eq!(
            counter(s.counts()),
            "2 changes (1 ignored) · All conflicts resolved"
        );
        let s = MergeSession::new(&conflict("a\n", "b\n", "c\n"), false);
        assert_eq!(counter(s.counts()), "1 of 1 conflicts open");
    }

    // SPEC-0101 AK-18 (v7).
    #[test]
    fn a_whole_side_file_takes_no_edit() {
        let binary = Conflict {
            base: None,
            ours: None,
            theirs: None,
            binary: true,
            ..conflict("", "", "")
        };
        let mut s = MergeSession::new(&binary, false);
        assert!(s.whole_side());
        assert_eq!(s.open_conflicts(), 1);
        assert!(!s.type_text("x") && !s.paste("x") && !s.newline() && !s.backspace());
        assert!(!s.ignore(0, true) && !s.can_ignore(0, false));
        assert!(!s.resolve(0, Resolution::Custom("x".into())));
        assert!(!s.resolve(0, Resolution::OursThenTheirs));
        assert_eq!(s.resolve_simple(), 0);
        assert!(!s.step(true));
        assert_eq!(s.text(), "");
        assert!(!s.is_dirty());
        assert_eq!(s.outcome(), None);
        assert!(s.accept(0, false));
        assert_eq!(
            s.outcome(),
            Some(Outcome::Side(ferris_idea_vcs::Side::Theirs))
        );
        assert_eq!(s.open_conflicts(), 0);
        assert!(s.can_accept(0, true) && !s.can_accept(0, false));
        assert!(s.undo());
        assert_eq!(s.open_conflicts(), 1);
        assert!(s.redo());
        assert!(s.accept_all(true));
        assert_eq!(
            s.outcome(),
            Some(Outcome::Side(ferris_idea_vcs::Side::Ours))
        );
        // In a rebase the left column is stage 3.
        let mut s = MergeSession::new(&binary, true);
        s.accept(0, true);
        assert_eq!(
            s.outcome(),
            Some(Outcome::Side(ferris_idea_vcs::Side::Theirs))
        );
    }

    // SPEC-0101 AK-18 (v7).
    #[test]
    fn only_choices_and_edits_make_a_session_dirty() {
        let mut s = session();
        s.set_caret(2, 1, false);
        s.move_caret(Motion::Right, true);
        s.select_all();
        assert!(s.copy().is_some());
        s.set_caret(0, 0, false);
        s.step(true);
        assert_eq!(s.copy().as_deref(), None);
        assert!(
            !s.is_dirty(),
            "moving the caret and selecting change nothing"
        );
        s.ignore(CONFLICT, true);
        assert!(s.is_dirty(), "an ignored side is a choice");
        s.undo();
        assert!(!s.is_dirty());
        s.type_text("x");
        assert!(s.is_dirty());
    }

    // SPEC-0101 AK-18 (v7).
    #[test]
    fn markers_typed_by_hand_are_recognised() {
        let mut s = session();
        assert!(!s.has_markers());
        s.accept(CONFLICT, true);
        s.move_caret(Motion::DocumentEnd, false);
        s.paste("<<<<<<< HEAD\na\n=======\nb\n>>>>>>> topic\n");
        assert!(s.has_markers());
        assert_eq!(s.open_conflicts(), 0, "the view asks before it writes them");
        s.undo();
        assert!(!s.has_markers());
        s.paste("=======\n");
        assert!(!s.has_markers(), "an underline is no marker");
    }
}
