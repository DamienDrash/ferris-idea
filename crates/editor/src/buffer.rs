//! Rope-backed text buffer with grouped undo/redo (`SPEC-0003`).
//!
//! All offsets are counted in Unicode scalar values (`char`s), never bytes,
//! so that a caret can never land inside a multi-byte character.

use std::ops::Range;

use ropey::Rope;

use crate::selection::Carets;

/// A caret position: zero-based line and zero-based column in `char`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    /// Zero-based line index.
    pub line: usize,
    /// Zero-based column, counted in `char`s from the start of the line.
    pub column: usize,
}

impl Position {
    /// Creates a position.
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Dominant line terminator of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// `\n`
    Lf,
    /// `\r\n`
    CrLf,
    /// `\r` alone (`SPEC-0336` behaviour 6).
    Cr,
}

impl LineEnding {
    /// The terminator text.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }
}

/// Errors for out-of-range edits and lookups.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// Offset or range lies outside the buffer.
    #[error("range {start}..{end} is outside the buffer of length {len}")]
    OutOfBounds {
        /// Requested start.
        start: usize,
        /// Requested end.
        end: usize,
        /// Buffer length in chars.
        len: usize,
    },
    /// Position lies outside the buffer.
    #[error("position {0:?} is outside the buffer")]
    InvalidPosition(Position),
}

/// One primitive change: `deleted` was replaced by `inserted` at `offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Char offset where the change starts.
    pub offset: usize,
    /// Text removed by the change.
    pub deleted: String,
    /// Text inserted by the change.
    pub inserted: String,
}

impl Edit {
    fn inverse(&self) -> Edit {
        Edit {
            offset: self.offset,
            deleted: self.inserted.clone(),
            inserted: self.deleted.clone(),
        }
    }
}

/// Reaction to a change of the buffer's file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconcile {
    /// Disk and buffer have the same text.
    Unchanged,
    /// The buffer has no unsaved changes: load the disk text.
    Reload,
    /// Both changed: the user decides, nothing is overwritten.
    Conflict,
}

/// An unchangeable copy of a buffer's text that shares its storage with the buffer.
#[derive(Debug, Clone, Default)]
pub struct TextSnapshot(Rope);

impl TextSnapshot {
    /// Snapshot of `text` (tests, callers without a buffer).
    pub fn from_text(text: &str) -> Self {
        Self(Rope::from_str(text))
    }

    /// Snapshot sharing the storage of `rope` (the session backend's documents, `SPEC-0317`).
    pub fn from_rope(rope: Rope) -> Self {
        Self(rope)
    }

    /// Length in bytes of the text as UTF-8.
    pub fn len_bytes(&self) -> usize {
        self.0.len_bytes()
    }
}

impl std::fmt::Display for TextSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for chunk in self.0.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl PartialEq for TextSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

/// A text document held in memory.
#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: Rope,
    undo: Vec<Step>,
    redo: Vec<Step>,
    open_group: Option<Vec<Edit>>,
    next_step_id: u64,
    saved_state: u64,
    /// Applied edits since the last [`TextBuffer::take_changes`], when tracking is on.
    changes: Option<Vec<Edit>>,
}

/// One undo step; `id` identifies the document state the step leads to.
#[derive(Debug, Clone)]
struct Step {
    id: u64,
    edits: Vec<Edit>,
    /// The carets before the step's command and after it, when the command noted them
    /// (`SPEC-0057` v7 behaviour 9).
    before: Option<Carets>,
    after: Option<Carets>,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::from_text("")
    }
}

impl TextBuffer {
    /// Creates a buffer holding `text`. The new buffer counts as saved.
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            undo: Vec::new(),
            redo: Vec::new(),
            open_group: None,
            next_step_id: 1,
            saved_state: 0,
            changes: None,
        }
    }

    /// The current text as a shared snapshot; costs no copy of the text (`SPEC-0220` behaviour 3).
    pub fn snapshot(&self) -> TextSnapshot {
        TextSnapshot(self.rope.clone())
    }

    /// Full text of the buffer.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Length in `char`s.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Number of lines. An empty buffer and a buffer ending in a line break
    /// both count the (empty) last line.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Text of line `line` without its terminator.
    pub fn line(&self, line: usize) -> Option<String> {
        if line >= self.rope.len_lines() {
            return None;
        }
        let mut s = self.rope.line(line).to_string();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        Some(s)
    }

    /// Dominant line ending; a buffer without line breaks reports [`LineEnding::Lf`]. With equal
    /// counts LF wins over CRLF and CRLF over a lone CR (`SPEC-0336` behaviour 6).
    pub fn line_ending(&self) -> LineEnding {
        // Counted per chunk on bytes, which vectorises; a `\r\n` may straddle two chunks.
        let (mut newlines, mut returns, mut crlf) = (0usize, 0usize, 0usize);
        let mut after_cr = false;
        for chunk in self.rope.chunks() {
            let bytes = chunk.as_bytes();
            newlines += bytes.iter().filter(|&&b| b == b'\n').count();
            returns += bytes.iter().filter(|&&b| b == b'\r').count();
            crlf += chunk.matches("\r\n").count();
            if after_cr && bytes.first() == Some(&b'\n') {
                crlf += 1;
            }
            after_cr = bytes.last() == Some(&b'\r');
        }
        let (lf, cr) = (newlines - crlf, returns - crlf);
        if cr > lf && cr > crlf {
            LineEnding::Cr
        } else if crlf > lf {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }

    /// What to do when the file of this buffer changed on disk to `disk` (`SPEC-0026`).
    pub fn reconcile(&self, disk: &str) -> Reconcile {
        if self.rope == disk {
            Reconcile::Unchanged
        } else if self.is_modified() {
            Reconcile::Conflict
        } else {
            Reconcile::Reload
        }
    }

    /// Replaces the content with `text` as one undo step and marks it saved. Only the changed
    /// lines are replaced, last first, so line marks such as breakpoints keep their lines (D40: a
    /// reload from disk moved every breakpoint to line 1).
    pub fn reload(&mut self, text: &str) {
        self.end_group();
        let old = self.rope.to_string();
        self.begin_group();
        for (range, new) in line_hunks(&old, text).into_iter().rev() {
            self.replace(range, &new)
                .expect("hunks lie inside the buffer");
        }
        self.mark_saved();
    }

    /// The char at `offset`, if any; does not build the whole text.
    pub fn char_at(&self, offset: usize) -> Option<char> {
        self.rope.get_char(offset)
    }

    /// Like [`TextBuffer::undo`], returning where the caret goes: the end of the restored text of the
    /// step's first edit.
    pub fn undo_caret(&mut self) -> Option<usize> {
        self.undo_carets().map(|(at, _)| at)
    }

    /// Like [`TextBuffer::redo`], returning the end of the step's last inserted text.
    pub fn redo_caret(&mut self) -> Option<usize> {
        self.redo_carets().map(|(at, _)| at)
    }

    /// Undoes one step. Returns the end of the restored text of the step's first edit and, when
    /// the step's command noted them, the carets as they were before it (`SPEC-0057` v7
    /// behaviour 9); noted carets that do not fit the restored text are left out.
    pub fn undo_carets(&mut self) -> Option<(usize, Option<Carets>)> {
        self.end_group();
        let step = self.undo.last()?;
        let first = step.edits.first().cloned();
        let noted = step.before.clone();
        self.undo();
        let at = first.map(|e| e.offset + e.deleted.chars().count())?;
        Some((at, self.fitting(noted)))
    }

    /// Redoes one step. Returns the end of the step's last inserted text and, when the step's
    /// command noted them, the carets as they were after it.
    pub fn redo_carets(&mut self) -> Option<(usize, Option<Carets>)> {
        let step = self.redo.last()?;
        let last = step.edits.last().cloned();
        let noted = step.after.clone();
        self.redo();
        let at = last.map(|e| e.offset + e.inserted.chars().count())?;
        Some((at, self.fitting(noted)))
    }

    fn fitting(&self, carets: Option<Carets>) -> Option<Carets> {
        let len = self.rope.len_chars();
        carets.filter(|c| c.carets().iter().all(|c| c.anchor <= len && c.head <= len))
    }

    /// Notes on the newest undo step where the carets were before its command and where they are
    /// after it, for undo and redo to put them back (`SPEC-0057` v7 behaviour 9). A step that
    /// already knows its carets before — a run of typing joined into one step — keeps them.
    pub fn note_carets(&mut self, before: &Carets, after: &Carets) {
        if let Some(step) = self.undo.last_mut() {
            step.before.get_or_insert_with(|| before.clone());
            step.after = Some(after.clone());
        }
    }

    /// Notes only the carets after the newest undo step, for a caller that places them once the
    /// step is made.
    pub fn note_carets_after(&mut self, after: &Carets) {
        if let Some(step) = self.undo.last_mut() {
            step.after = Some(after.clone());
        }
    }

    /// Text of the chars in `range`.
    pub fn slice(&self, range: Range<usize>) -> Result<String, EditError> {
        self.check_range(range.clone())?;
        Ok(self.rope.slice(range).to_string())
    }

    /// Converts a char offset to a line/column position.
    pub fn offset_to_position(&self, offset: usize) -> Result<Position, EditError> {
        self.check_range(offset..offset)?;
        let line = self.rope.char_to_line(offset);
        Ok(Position::new(line, offset - self.rope.line_to_char(line)))
    }

    /// Converts a line/column position to a char offset. The column may point
    /// at the end of the line content but not past its terminator.
    pub fn position_to_offset(&self, pos: Position) -> Result<usize, EditError> {
        let content = self.line(pos.line).ok_or(EditError::InvalidPosition(pos))?;
        if pos.column > content.chars().count() {
            return Err(EditError::InvalidPosition(pos));
        }
        Ok(self.rope.line_to_char(pos.line) + pos.column)
    }

    /// Replaces the chars in `range` with `text`.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), EditError> {
        self.check_range(range.clone())?;
        let deleted = self.rope.slice(range.clone()).to_string();
        if deleted.is_empty() && text.is_empty() {
            return Ok(());
        }
        let edit = Edit {
            offset: range.start,
            deleted,
            inserted: text.to_owned(),
        };
        self.apply(&edit);
        self.redo.clear();
        match &mut self.open_group {
            Some(group) => group.push(edit),
            None => self.push_step(vec![edit]),
        }
        Ok(())
    }

    /// Inserts `text` at `offset`.
    pub fn insert(&mut self, offset: usize, text: &str) -> Result<(), EditError> {
        self.replace(offset..offset, text)
    }

    /// Deletes the chars in `range`.
    pub fn delete(&mut self, range: Range<usize>) -> Result<(), EditError> {
        self.replace(range, "")
    }

    /// Starts an undo group: all edits until [`TextBuffer::end_group`] are
    /// undone and redone as one step. Nested calls are flattened.
    pub fn begin_group(&mut self) {
        if self.open_group.is_none() {
            self.open_group = Some(Vec::new());
        }
    }

    /// Closes the current undo group. Empty groups leave no history entry.
    pub fn end_group(&mut self) {
        if let Some(group) = self.open_group.take()
            && !group.is_empty()
        {
            self.push_step(group);
        }
    }

    /// Reverts the most recent undo step. Returns `false` if there is none.
    pub fn undo(&mut self) -> bool {
        self.end_group();
        let Some(step) = self.undo.pop() else {
            return false;
        };
        for edit in step.edits.iter().rev() {
            self.apply(&edit.inverse());
        }
        self.redo.push(step);
        true
    }

    /// Re-applies the most recently undone step. Returns `false` if there is none.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        for edit in &step.edits {
            self.apply(edit);
        }
        self.undo.push(step);
        true
    }

    /// Whether the buffer left the last saved state. Undoing or redoing back
    /// to the saved state counts as unmodified.
    pub fn is_modified(&self) -> bool {
        self.open_group.as_ref().is_some_and(|g| !g.is_empty())
            || self.current_state() != self.saved_state
    }

    /// Marks the current content as saved. Closes an open undo group.
    pub fn mark_saved(&mut self) {
        self.end_group();
        self.saved_state = self.current_state();
    }

    /// The places of the undo steps, newest first (`SPEC-0071` v9 behaviour 13): for each step the
    /// end of the text its last edit put in, moved along with the edits of the steps after it. A
    /// place on the line of the place before it is left out. At most 100 places, from the newest
    /// 1 000 steps.
    pub fn change_places(&self) -> Vec<usize> {
        const PLACES: usize = 100;
        const STEPS: usize = 1000;
        let steps = &self.undo[self.undo.len().saturating_sub(STEPS)..];
        // Offset, chars removed and chars put in of every edit.
        let sizes: Vec<Vec<(usize, usize, usize)>> = steps
            .iter()
            .map(|s| {
                s.edits
                    .iter()
                    .map(|e| {
                        (
                            e.offset,
                            e.deleted.chars().count(),
                            e.inserted.chars().count(),
                        )
                    })
                    .collect()
            })
            .collect();
        let len = self.rope.len_chars();
        let mut out = Vec::new();
        let mut last_line = None;
        for i in (0..sizes.len()).rev() {
            let Some(&(offset, _, put_in)) = sizes[i].last() else {
                continue;
            };
            let mut at = offset + put_in;
            for &(o, removed, added) in sizes[i + 1..].iter().flatten() {
                if at >= o + removed {
                    at = at + added - removed;
                } else if at > o {
                    at = o + added;
                }
            }
            let at = at.min(len);
            let line = self.rope.char_to_line(at);
            if last_line != Some(line) {
                out.push(at);
                last_line = Some(line);
                if out.len() == PLACES {
                    break;
                }
            }
        }
        out
    }

    /// Identifies the newest undo step, if any.
    pub fn top_step(&self) -> Option<u64> {
        self.undo.last().map(|step| step.id)
    }

    /// Identifies the step the next redo re-applies, if any.
    pub fn redo_step(&self) -> Option<u64> {
        self.redo.last().map(|step| step.id)
    }

    /// Joins the newest undo step into the one before, so one undo reverts both; refused when the
    /// earlier step is the saved state, so undo still stops there. Returns whether it joined.
    pub fn join_last_steps(&mut self) -> bool {
        self.end_group();
        let n = self.undo.len();
        if n < 2 || self.undo[n - 2].id == self.saved_state {
            return false;
        }
        let last = self.undo.pop().expect("two steps");
        let earlier = self.undo.last_mut().expect("two steps");
        earlier.edits.extend(last.edits);
        earlier.id = last.id;
        // One step now: the carets before its first part and after its last.
        earlier.after = last.after;
        true
    }

    fn current_state(&self) -> u64 {
        self.undo.last().map_or(0, |step| step.id)
    }

    fn push_step(&mut self, edits: Vec<Edit>) {
        self.undo.push(Step {
            id: self.next_step_id,
            edits,
            before: None,
            after: None,
        });
        self.next_step_id += 1;
    }

    fn apply(&mut self, edit: &Edit) {
        let end = edit.offset + edit.deleted.chars().count();
        self.rope.remove(edit.offset..end);
        self.rope.insert(edit.offset, &edit.inserted);
        if let Some(changes) = &mut self.changes {
            changes.push(edit.clone());
        }
    }

    /// Starts recording every applied edit (including undo and redo), for views that mirror the
    /// text, e.g. a syntax tree.
    pub fn track_changes(&mut self) {
        self.changes.get_or_insert_with(Vec::new);
    }

    /// Edits applied since the last call, in order; empty when tracking is off.
    pub fn take_changes(&mut self) -> Vec<Edit> {
        self.changes
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// UTF-16 code units before char `offset` (for platform input methods).
    pub fn char_to_utf16(&self, offset: usize) -> usize {
        self.rope
            .char_to_utf16_cu(offset.min(self.rope.len_chars()))
    }

    /// Char offset of UTF-16 code unit `units`.
    pub fn utf16_to_char(&self, units: usize) -> usize {
        self.rope
            .utf16_cu_to_char(units.min(self.rope.len_utf16_cu()))
    }

    fn check_range(&self, range: Range<usize>) -> Result<(), EditError> {
        let len = self.rope.len_chars();
        if range.start > range.end || range.end > len {
            return Err(EditError::OutOfBounds {
                start: range.start,
                end: range.end,
                len,
            });
        }
        Ok(())
    }
}

/// The changed regions between `old` and `new` by whole lines, in order: the char range in `old`
/// and its new text. Lines are matched by a longest common subsequence; beyond a size limit the
/// changed middle is one region.
pub fn line_hunks(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    // Char offset of each old line.
    let mut starts = Vec::with_capacity(a.len() + 1);
    let mut at = 0;
    for l in &a {
        starts.push(at);
        at += l.chars().count();
    }
    starts.push(at);
    // Matched (old, new) line pairs of the middle, in order.
    let mut pairs = Vec::new();
    if am.len().saturating_mul(bm.len()) <= 4_000_000 {
        let (n, m) = (am.len(), bm.len());
        let mut lcs = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * (m + 1) + j] = if am[i] == bm[j] {
                    lcs[(i + 1) * (m + 1) + j + 1] + 1
                } else {
                    lcs[(i + 1) * (m + 1) + j].max(lcs[i * (m + 1) + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if am[i] == bm[j] {
                pairs.push((i, j));
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * (m + 1) + j] >= lcs[i * (m + 1) + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    pairs.push((am.len(), bm.len()));
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in pairs {
        if pi > i || pj > j {
            let range = starts[prefix + i]..starts[prefix + pi];
            out.push((range, bm[j..pj].concat()));
        }
        i = pi + 1;
        j = pj + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0071 v9 AK-9.
    #[test]
    fn change_places_follow_later_edits() {
        let mut b = TextBuffer::from_text("one\ntwo\nthree\n");
        assert!(b.change_places().is_empty());
        b.insert(13, "A").unwrap(); // after `three`
        b.insert(3, "B").unwrap(); // after `one`
        b.insert(5, "C").unwrap(); // before `two`
        b.insert(0, "top\n").unwrap();
        assert_eq!(b.text(), "top\noneB\nCtwo\nthreeA\n");
        // Newest first, each where its text ends now: line 1 (the new first line ends before
        // it), line 2, line 1, line 3.
        assert_eq!(b.change_places(), [4, 10, 8, 20]);
        // A second edit on the line of the newest place: that older place counts no more.
        b.insert(6, "y").unwrap();
        assert_eq!(b.text(), "top\nonyeB\nCtwo\nthreeA\n");
        assert_eq!(b.change_places(), [7, 11, 9, 21]);
        // A deletion is a place where it was made; places in removed text move there.
        b.delete(0..4).unwrap();
        assert_eq!(b.text(), "onyeB\nCtwo\nthreeA\n");
        assert_eq!(b.change_places(), [0, 7, 5, 17]);
        // An undone step is no place.
        assert!(b.undo());
        assert_eq!(b.change_places(), [7, 11, 9, 21]);
        // The last edit of a grouped step counts.
        b.begin_group();
        b.insert(0, "x").unwrap();
        b.insert(22, "z").unwrap();
        b.end_group();
        assert_eq!(b.change_places()[0], 23);
    }

    /// D40: a reload changes only the lines that differ.
    #[test]
    fn a_reload_replaces_only_the_changed_lines() {
        let old = "a\nb\nc\nd\n";
        let new = "x\na\nb\nc\nD\n";
        assert_eq!(
            line_hunks(old, new),
            vec![(0..0, "x\n".to_owned()), (6..8, "D\n".to_owned())]
        );
        assert!(line_hunks(old, old).is_empty());
        assert_eq!(line_hunks("", "a"), vec![(0..0, "a".to_owned())]);
        assert_eq!(line_hunks("a\nb", "a\n"), vec![(2..3, String::new())]);
        let mut buf = TextBuffer::from_text(old);
        buf.track_changes();
        buf.reload(new);
        assert_eq!(buf.text(), new);
        assert!(!buf.is_modified());
        let changes = buf.take_changes();
        // Last first: the edit on line 4 and then the line put in above line 1.
        assert_eq!(changes.len(), 2);
        assert_eq!(
            (changes[0].offset, changes[0].inserted.as_str()),
            (6, "D\n")
        );
        assert_eq!(
            (changes[1].offset, changes[1].inserted.as_str()),
            (0, "x\n")
        );
        buf.undo();
        assert_eq!(buf.text(), old);
    }

    #[test]
    fn reconcile_and_undoable_reload() {
        let mut buf = TextBuffer::from_text("v1");
        assert_eq!(buf.reconcile("v1"), Reconcile::Unchanged);
        assert_eq!(buf.reconcile("v2"), Reconcile::Reload);
        buf.reload("v2 from disk");
        assert_eq!(buf.text(), "v2 from disk");
        assert!(!buf.is_modified(), "reloaded content counts as saved");
        buf.insert(0, "x").unwrap();
        assert_eq!(buf.reconcile("v3"), Reconcile::Conflict);
        assert_eq!(buf.reconcile("xv2 from disk"), Reconcile::Unchanged);
        assert!(buf.undo());
        assert!(buf.undo(), "the reload itself is one undo step");
        assert_eq!(buf.text(), "v1");
        assert!(buf.is_modified());
    }

    #[test]
    fn insert_delete_and_replace() {
        let mut b = TextBuffer::from_text("hello world");
        b.insert(5, ",").unwrap();
        b.delete(6..7).unwrap();
        b.replace(6..11, "Rust").unwrap();
        assert_eq!(b.text(), "hello,Rust");
    }

    #[test]
    fn offsets_count_chars_not_bytes() {
        let mut b = TextBuffer::from_text("äöü\n€x");
        assert_eq!(b.len_chars(), 6);
        assert_eq!(b.offset_to_position(5).unwrap(), Position::new(1, 1));
        b.insert(b.position_to_offset(Position::new(1, 1)).unwrap(), "y")
            .unwrap();
        assert_eq!(b.line(1).as_deref(), Some("€yx"));
    }

    #[test]
    fn position_round_trip_and_bounds() {
        let b = TextBuffer::from_text("ab\r\ncd\n");
        assert_eq!(b.len_lines(), 3);
        assert_eq!(b.line(0).as_deref(), Some("ab"));
        assert_eq!(b.position_to_offset(Position::new(1, 2)).unwrap(), 6);
        assert_eq!(b.offset_to_position(6).unwrap(), Position::new(1, 2));
        assert!(b.position_to_offset(Position::new(0, 3)).is_err());
        assert!(b.position_to_offset(Position::new(5, 0)).is_err());
        assert_eq!(
            b.position_to_offset(Position::new(2, 0)).unwrap(),
            b.len_chars()
        );
    }

    #[test]
    fn out_of_bounds_edits_are_rejected_without_change() {
        let mut b = TextBuffer::from_text("abc");
        assert!(matches!(b.delete(2..9), Err(EditError::OutOfBounds { .. })));
        #[allow(clippy::reversed_empty_ranges)]
        let reversed = 2..1;
        assert!(b.delete(reversed).is_err());
        assert_eq!(b.text(), "abc");
        assert!(!b.undo());
    }

    #[test]
    fn undo_redo_single_steps() {
        let mut b = TextBuffer::from_text("a");
        b.insert(1, "b").unwrap();
        b.insert(2, "c").unwrap();
        assert!(b.undo());
        assert_eq!(b.text(), "ab");
        assert!(b.redo());
        assert_eq!(b.text(), "abc");
        assert!(b.undo() && b.undo());
        assert_eq!(b.text(), "a");
        assert!(!b.undo());
    }

    #[test]
    fn redo_step_is_the_undone_step() {
        let mut b = TextBuffer::from_text("");
        b.insert(0, "a").unwrap();
        let step = b.top_step();
        assert!(step.is_some() && b.redo_step().is_none());
        b.undo();
        assert_eq!(b.redo_step(), step);
        assert_eq!(b.top_step(), None);
        b.redo();
        assert_eq!(b.top_step(), step);
    }

    #[test]
    fn grouped_edits_undo_as_one_step() {
        let mut b = TextBuffer::from_text("x = 1");
        b.begin_group();
        b.replace(0..1, "y").unwrap();
        b.begin_group();
        b.replace(4..5, "2").unwrap();
        b.end_group();
        assert_eq!(b.text(), "y = 2");
        assert!(b.undo());
        assert_eq!(b.text(), "x = 1");
        assert!(b.redo());
        assert_eq!(b.text(), "y = 2");
    }

    /// SPEC-0003 v3 behaviour 11 (AK-11).
    #[test]
    fn carets_noted_on_a_step() {
        use crate::selection::Caret;
        let two = |a: usize, b: usize| Carets::from_carets(vec![Caret::at(a), Caret::at(b)]);
        let mut b = TextBuffer::from_text("ab");
        // A step without noted carets hands out none.
        b.insert(2, "c").unwrap();
        assert_eq!(b.undo_carets(), Some((2, None)));
        assert_eq!(b.redo_carets(), Some((3, None)));
        // Noted carets come back; the first note of the carets before stays.
        b.note_carets(&two(0, 2), &two(1, 3));
        b.note_carets(&two(1, 1), &two(0, 3));
        assert_eq!(b.undo_carets(), Some((2, Some(two(0, 2)))));
        assert_eq!(b.redo_carets(), Some((3, Some(two(0, 3)))));
        // Joined steps: the carets before the first and after the last.
        b.insert(3, "d").unwrap();
        b.note_carets(&Carets::new(3), &Carets::new(4));
        assert!(b.join_last_steps());
        assert_eq!(b.undo_carets(), Some((2, Some(two(0, 2)))));
        assert_eq!(b.text(), "ab");
        assert_eq!(b.redo_carets(), Some((4, Some(Carets::new(4)))));
        assert_eq!(b.text(), "abcd");
        // Carets that do not fit the text are not handed out.
        b.delete(0..4).unwrap();
        b.note_carets(&Carets::new(9), &Carets::new(0));
        assert_eq!(b.undo_carets(), Some((4, None)));
        // Nothing to note on, nothing to undo.
        let mut empty = TextBuffer::from_text("");
        empty.note_carets(&Carets::new(0), &Carets::new(0));
        empty.note_carets_after(&Carets::new(0));
        assert_eq!(empty.undo_carets(), None);
        assert_eq!(empty.redo_carets(), None);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut b = TextBuffer::from_text("");
        b.insert(0, "a").unwrap();
        b.undo();
        b.insert(0, "b").unwrap();
        assert!(!b.redo());
        assert_eq!(b.text(), "b");
    }

    #[test]
    fn modified_flag_tracks_saved_state_through_history() {
        let mut b = TextBuffer::from_text("a");
        assert!(!b.is_modified());
        b.insert(1, "b").unwrap();
        assert!(b.is_modified());
        b.mark_saved();
        assert!(!b.is_modified());
        b.undo();
        assert!(b.is_modified());
        b.redo();
        assert!(!b.is_modified());
        b.undo();
        b.insert(1, "c").unwrap();
        assert_eq!(b.text(), "ac");
        assert!(
            b.is_modified(),
            "different edit at same history depth must count as modified"
        );
    }

    #[test]
    fn line_ending_detection() {
        assert_eq!(
            TextBuffer::from_text("a\r\nb\r\nc\n").line_ending(),
            LineEnding::CrLf
        );
        assert_eq!(TextBuffer::from_text("a\nb").line_ending(), LineEnding::Lf);
        assert_eq!(TextBuffer::from_text("").line_ending(), LineEnding::Lf);
        // Large enough for many rope chunks, so some `\r\n` straddle a chunk boundary.
        for (line, ending) in [("abc\r\n", LineEnding::CrLf), ("abcd\n", LineEnding::Lf)] {
            let text: String = (0..20_000).map(|i| format!("{i}{line}")).collect();
            assert_eq!(TextBuffer::from_text(&text).line_ending(), ending);
        }
        let mixed = format!("{}{}", "x\r\n".repeat(3000), "y\n".repeat(3001));
        assert_eq!(TextBuffer::from_text(&mixed).line_ending(), LineEnding::Lf);
        // A lone CR is a separator of its own (SPEC-0336 AK-1); ties go LF, CRLF, CR.
        assert_eq!(
            TextBuffer::from_text("a\rb\rc").line_ending(),
            LineEnding::Cr
        );
        assert_eq!(
            TextBuffer::from_text("a\rb\nc").line_ending(),
            LineEnding::Lf
        );
        assert_eq!(
            TextBuffer::from_text("a\rb\r\nc").line_ending(),
            LineEnding::CrLf
        );
        assert_eq!(
            TextBuffer::from_text("a\rb\rc\r\nd\ne").line_ending(),
            LineEnding::Cr
        );
    }
}

#[cfg(test)]
mod change_tests {
    use super::*;

    #[test]
    fn changes_are_recorded_when_tracked_and_utf16_maps_both_ways() {
        let mut b = TextBuffer::from_text("aä😀b");
        b.insert(0, "x").unwrap();
        assert!(b.take_changes().is_empty(), "off by default");
        b.track_changes();
        b.replace(1..2, "y").unwrap();
        b.undo();
        let changes = b.take_changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].inserted, "y");
        assert_eq!(changes[1].deleted, "y");
        assert!(b.take_changes().is_empty());
        // "xaä😀b": the emoji takes two UTF-16 units.
        assert_eq!(b.char_to_utf16(3), 3);
        assert_eq!(b.char_to_utf16(4), 5);
        assert_eq!(b.utf16_to_char(5), 4);
    }
}
