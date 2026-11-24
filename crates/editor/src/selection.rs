//! Selections and multiple carets (`SPEC-0025`).
//!
//! Offsets are char offsets into a [`TextBuffer`]. Every editing action runs
//! as one undo group.

use std::ops::Range;

use crate::{Position, TextBuffer};

/// A caret with an optional selection between `anchor` and `head`.
#[derive(Debug, Clone, Copy)]
pub struct Caret {
    /// Fixed end of the selection.
    pub anchor: usize,
    /// Moving end; where the caret is drawn.
    pub head: usize,
    /// Column kept across vertical moves.
    goal: Option<usize>,
    /// When the caret was added, counting up per [`Carets`]; the latest goes first on
    /// Unselect Occurrence (behaviour 8). Not part of equality.
    added: u64,
}

impl PartialEq for Caret {
    fn eq(&self, other: &Self) -> bool {
        (self.anchor, self.head, self.goal) == (other.anchor, other.head, other.goal)
    }
}

impl Eq for Caret {}

impl Caret {
    /// An empty caret at `offset`.
    pub fn at(offset: usize) -> Self {
        Self::select(offset, offset)
    }

    /// A caret selecting from `anchor` to `head`.
    pub fn select(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            goal: None,
            added: 0,
        }
    }

    /// Selected range (start ≤ end).
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// How carets move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// One character left.
    Left,
    /// One character right.
    Right,
    /// One line up, keeping the goal column.
    Up,
    /// One line down, keeping the goal column.
    Down,
    /// Start of the line.
    LineStart,
    /// End of the line.
    LineEnd,
    /// Start of the previous word.
    WordLeft,
    /// End of the next word.
    WordRight,
    /// Start of the document.
    DocumentStart,
    /// End of the document.
    DocumentEnd,
}

/// All carets of one editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carets {
    carets: Vec<Caret>,
    primary: usize,
    /// "Add next occurrence" matches whole words only: it selected the word at an empty caret,
    /// and nothing but occurrence commands happened since (`SPEC-0025` v4 behaviour 4b).
    whole_words: bool,
}

pub(crate) fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_blank(c: Option<char>) -> bool {
    matches!(c, Some(' ' | '\t'))
}

fn is_break(c: Option<char>) -> bool {
    matches!(c, Some('\n' | '\r'))
}

/// Next word stop (`SPEC-0057` v3 behaviour 14): over spaces, then over a word or one other
/// character; a line end moves to the next line's start, spaces before a line end stop there.
pub(crate) fn word_right(buf: &TextBuffer, from: usize) -> usize {
    let len = buf.len_chars();
    let mut i = from;
    if is_break(buf.char_at(i)) {
        if buf.char_at(i) == Some('\r') && buf.char_at(i + 1) == Some('\n') {
            i += 1;
        }
        return (i + 1).min(len);
    }
    while is_blank(buf.char_at(i)) {
        i += 1;
    }
    if i >= len || is_break(buf.char_at(i)) {
        return i;
    }
    if buf.char_at(i).is_some_and(is_word) {
        while buf.char_at(i).is_some_and(is_word) {
            i += 1;
        }
        i
    } else {
        (i + 1).min(len)
    }
}

/// Previous word stop, mirroring [`word_right`]; with only whitespace before the caret on its line
/// it goes to the end of the previous line.
pub(crate) fn word_left(buf: &TextBuffer, from: usize) -> usize {
    let mut i = from;
    if i == 0 {
        return 0;
    }
    while i > 0 && is_blank(buf.char_at(i - 1)) {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    if is_break(buf.char_at(i - 1)) {
        i -= 1;
        if i > 0 && buf.char_at(i) == Some('\n') && buf.char_at(i - 1) == Some('\r') {
            i -= 1;
        }
        return i;
    }
    if buf.char_at(i - 1).is_some_and(is_word) {
        while i > 0 && buf.char_at(i - 1).is_some_and(is_word) {
            i -= 1;
        }
        i
    } else {
        i - 1
    }
}

fn line_len(buf: &TextBuffer, line: usize) -> usize {
    buf.line(line).map_or(0, |l| l.chars().count())
}

fn position(buf: &TextBuffer, offset: usize) -> Position {
    buf.offset_to_position(offset.min(buf.len_chars()))
        .unwrap_or_default()
}

fn offset(buf: &TextBuffer, line: usize, column: usize) -> usize {
    let column = column.min(line_len(buf, line));
    buf.position_to_offset(Position::new(line, column))
        .unwrap_or_else(|_| buf.len_chars())
}

impl Carets {
    /// Carets from a list (sorted and merged like after any edit); an empty list means one caret at 0.
    pub fn from_carets(list: Vec<Caret>) -> Self {
        let carets = if list.is_empty() {
            vec![Caret::at(0)]
        } else {
            list
        };
        let mut out = Self {
            carets,
            primary: 0,
            whole_words: false,
        };
        out.normalize();
        out
    }

    /// Replaces the carets by `list`, one new caret per old caret in document order (as after an
    /// edit that moved each of them). Each keeps when it was added and the primary stays primary,
    /// so Unselect Occurrence still removes the latest caret after typing (D52). A list of another
    /// length starts afresh like [`Carets::from_carets`].
    pub fn moved_to(&mut self, list: Vec<Caret>) {
        if list.len() != self.carets.len() {
            *self = Self::from_carets(list);
            return;
        }
        let carets = list
            .into_iter()
            .zip(&self.carets)
            .map(|(mut new, old)| {
                new.added = old.added;
                new
            })
            .collect();
        let primary = self.primary;
        let mut out = Self {
            carets,
            primary,
            whole_words: false,
        };
        out.normalize();
        *self = out;
    }

    /// One empty caret at `offset`.
    pub fn new(offset: usize) -> Self {
        Self {
            carets: vec![Caret::at(offset)],
            primary: 0,
            whole_words: false,
        }
    }

    /// Carets in document order.
    pub fn carets(&self) -> &[Caret] {
        &self.carets
    }

    /// The most recently added caret.
    pub fn primary(&self) -> Caret {
        self.carets[self.primary]
    }

    /// Adds an empty caret at `offset`; it becomes primary.
    pub fn add_caret(&mut self, offset: usize) {
        self.push(Caret::at(offset));
    }

    /// Add or Remove Caret (`SPEC-0130` v2 behaviour 5): removes the caret without selection that
    /// stands at `offset` while another one remains, else adds a caret there. Returns whether a
    /// caret was removed.
    pub fn toggle_caret(&mut self, offset: usize) -> bool {
        let at = self
            .carets
            .iter()
            .position(|c| c.is_empty() && c.head == offset);
        match at {
            Some(i) if self.carets.len() > 1 => {
                self.carets.remove(i);
                let latest = self.carets.iter().map(|c| c.added).max().unwrap_or(0);
                self.primary = self
                    .carets
                    .iter()
                    .rposition(|c| c.added == latest)
                    .unwrap_or(0);
                true
            }
            Some(_) => false,
            None => {
                self.add_caret(offset);
                false
            }
        }
    }

    /// Adds a caret selecting `anchor..head`; it becomes primary.
    pub fn add_selection(&mut self, anchor: usize, head: usize) {
        self.push(Caret::select(anchor, head));
    }

    fn push(&mut self, mut caret: Caret) {
        caret.added = self.next_added();
        self.carets.push(caret);
        self.primary = self.carets.len() - 1;
        self.normalize();
    }

    fn next_added(&self) -> u64 {
        self.carets.iter().map(|c| c.added).max().unwrap_or(0) + 1
    }

    /// Unselect Occurrence (behaviour 8): removes the most recently added caret and makes the one
    /// added before it primary; with one caret, removes its selection. Returns whether anything
    /// changed.
    pub fn remove_last_added(&mut self) -> bool {
        if self.carets.len() > 1 {
            self.carets.remove(self.primary);
            let latest = self.carets.iter().map(|c| c.added).max().unwrap_or(0);
            self.primary = self
                .carets
                .iter()
                .rposition(|c| c.added == latest)
                .unwrap_or(0);
            return true;
        }
        let c = &mut self.carets[0];
        if c.is_empty() {
            return false;
        }
        c.anchor = c.head;
        true
    }

    /// Sorts carets and merges overlapping selections and equal empty carets.
    fn normalize(&mut self) {
        let primary_head = self.carets[self.primary].head;
        self.carets
            .sort_by_key(|c| (c.range().start, c.range().end));
        let mut merged: Vec<Caret> = Vec::with_capacity(self.carets.len());
        for c in self.carets.drain(..) {
            if let Some(last) = merged.last_mut() {
                let (a, b) = (last.range(), c.range());
                // Overlap, equal ranges, or an empty caret on the boundary of a selection.
                let touches_empty = (a.is_empty() || b.is_empty()) && b.start <= a.end;
                if b.start < a.end || a == b || touches_empty {
                    let range = a.start..a.end.max(b.end);
                    let forward = last.head >= last.anchor;
                    let goal = last.goal.or(c.goal);
                    let added = last.added.max(c.added);
                    *last = if forward {
                        Caret::select(range.start, range.end)
                    } else {
                        Caret::select(range.end, range.start)
                    };
                    last.goal = goal;
                    last.added = added;
                    continue;
                }
            }
            merged.push(c);
        }
        self.carets = merged;
        self.primary = self
            .carets
            .iter()
            .position(|c| c.head == primary_head)
            .or_else(|| {
                self.carets
                    .iter()
                    .position(|c| c.range().contains(&primary_head))
            })
            .unwrap_or(self.carets.len() - 1);
    }

    /// Moves every caret; with `extend` only the head moves.
    pub fn move_all(&mut self, buf: &TextBuffer, motion: Motion, extend: bool) {
        let len = buf.len_chars();
        for c in &mut self.carets {
            if !extend && !c.is_empty() && matches!(motion, Motion::Left | Motion::Right) {
                let target = if motion == Motion::Left {
                    c.range().start
                } else {
                    c.range().end
                };
                *c = Caret {
                    added: c.added,
                    ..Caret::at(target)
                };
                continue;
            }
            let here = position(buf, c.head);
            let mut goal = None;
            let head = match motion {
                Motion::Left => c.head.saturating_sub(1),
                Motion::Right => (c.head + 1).min(len),
                Motion::Up | Motion::Down => {
                    let column = c.goal.unwrap_or(here.column);
                    goal = Some(column);
                    let line = if motion == Motion::Up {
                        here.line.checked_sub(1)
                    } else {
                        Some(here.line + 1).filter(|l| *l < buf.len_lines())
                    };
                    line.map_or(c.head, |l| offset(buf, l, column))
                }
                Motion::LineStart => offset(buf, here.line, 0),
                Motion::LineEnd => offset(buf, here.line, usize::MAX),
                Motion::WordLeft => word_left(buf, c.head),
                Motion::WordRight => word_right(buf, c.head),
                Motion::DocumentStart => 0,
                Motion::DocumentEnd => len,
            };
            let anchor = if extend { c.anchor } else { head };
            *c = Caret {
                anchor,
                head,
                goal,
                added: c.added,
            };
        }
        self.normalize();
    }

    /// Adds a caret one line above (`down == false`) or below each caret.
    pub fn add_vertical(&mut self, buf: &TextBuffer, down: bool) {
        let next = self.next_added();
        let mut added = Vec::new();
        for c in &self.carets {
            let here = position(buf, c.head);
            let column = c.goal.unwrap_or(here.column);
            let line = if down {
                Some(here.line + 1).filter(|l| *l < buf.len_lines())
            } else {
                here.line.checked_sub(1)
            };
            if let Some(line) = line {
                let o = offset(buf, line, column);
                added.push(Caret {
                    anchor: o,
                    head: o,
                    goal: Some(column),
                    added: next + added.len() as u64,
                });
            }
        }
        for c in added {
            self.carets.push(c);
            self.primary = self.carets.len() - 1;
        }
        self.normalize();
    }

    /// Replaces all carets with a block selection from `from` to `to`.
    pub fn block(&mut self, buf: &TextBuffer, from: Position, to: Position) {
        let lines: Vec<usize> = if from.line <= to.line {
            (from.line..=to.line).collect()
        } else {
            (to.line..=from.line).rev().collect()
        };
        self.carets = lines
            .iter()
            .filter(|l| **l < buf.len_lines())
            .map(|&l| Caret::select(offset(buf, l, from.column), offset(buf, l, to.column)))
            .collect();
        if self.carets.is_empty() {
            self.carets.push(Caret::at(0));
        }
        let last = *self.carets.last().expect("not empty");
        self.primary = self.carets.len() - 1;
        self.normalize();
        self.primary = self
            .carets
            .iter()
            .position(|c| *c == last)
            .unwrap_or(self.primary);
    }

    /// Ends the whole-word matching of "add next occurrence": any command other than an
    /// occurrence command does (`SPEC-0025` v4 behaviour 4b).
    pub fn end_occurrence_row(&mut self) {
        self.whole_words = false;
    }

    /// Whether "add next occurrence" matches whole words only (behaviour 4b).
    pub fn whole_words(&self) -> bool {
        self.whole_words
    }

    /// Selects the word at the primary caret, or adds the next occurrence of
    /// its selection. Returns whether anything changed. After the word was selected from an empty
    /// caret, only whole words match (`SPEC-0025` v4 behaviour 4b).
    pub fn add_next_occurrence(&mut self, buf: &TextBuffer) -> bool {
        let whole = self.whole_words;
        self.next_occurrence(buf, whole)
    }

    fn next_occurrence(&mut self, buf: &TextBuffer, whole_words: bool) -> bool {
        let chars: Vec<char> = buf.text().chars().collect();
        let p = self.primary();
        if p.is_empty() {
            let mut start = p.head;
            while start > 0 && is_word(chars[start - 1]) {
                start -= 1;
            }
            let mut end = p.head;
            while end < chars.len() && is_word(chars[end]) {
                end += 1;
            }
            if start == end {
                return false;
            }
            // The caret keeps its place in the word: at its start it stays there, else it stands
            // at the end (`SPEC-0025` v3 behaviour 4a).
            let word = if p.head == start {
                Caret::select(end, start)
            } else {
                Caret::select(start, end)
            };
            self.carets[self.primary] = Caret {
                added: p.added,
                ..word
            };
            self.normalize();
            self.whole_words = true;
            return true;
        }
        let needle = &chars[p.range()];
        let whole = |r: &Range<usize>| {
            (r.start == 0 || !is_word(chars[r.start - 1]))
                && (r.end == chars.len() || !is_word(chars[r.end]))
        };
        // Added carets stand where the primary's caret stands in its selection (behaviour 4a).
        let at_start = p.head < p.anchor;
        let n = chars.len();
        for step in 1..=n {
            let start = (p.range().start + step) % n.max(1);
            if start + needle.len() > n || &chars[start..start + needle.len()] != needle {
                continue;
            }
            let range = start..start + needle.len();
            if whole_words && !whole(&range) {
                continue;
            }
            if self.carets.iter().any(|c| c.range() == range) {
                continue;
            }
            if self.carets.iter().any(|c| {
                let r = c.range();
                r.start < range.end && range.start < r.end
            }) {
                continue;
            }
            self.push(if at_start {
                Caret::select(range.end, range.start)
            } else {
                Caret::select(range.start, range.end)
            });
            return true;
        }
        false
    }

    /// Selects every occurrence of the primary selection, or of the word at an empty caret; the
    /// word's occurrences are whole words only (`SPEC-0025` v4 behaviour 4b).
    pub fn select_all_occurrences(&mut self, buf: &TextBuffer) {
        let whole = self.primary().is_empty();
        if whole && !self.next_occurrence(buf, false) {
            return;
        }
        while self.next_occurrence(buf, whole) {}
        self.whole_words = false;
    }

    /// Replaces each caret's range with its text, as one undo group; carets
    /// end up empty after their inserted text.
    pub(crate) fn replace_each(
        &mut self,
        buf: &mut TextBuffer,
        ranges: &[Range<usize>],
        texts: &[String],
    ) {
        self.whole_words = false;
        buf.begin_group();
        for (range, text) in ranges.iter().zip(texts).rev() {
            buf.replace(range.clone(), text)
                .expect("caret ranges lie inside the buffer");
        }
        buf.end_group();
        let mut shift: isize = 0;
        for (i, (range, text)) in ranges.iter().zip(texts).enumerate() {
            let len = text.chars().count();
            let at = (range.start as isize + shift) as usize + len;
            self.carets[i] = Caret {
                added: self.carets[i].added,
                ..Caret::at(at)
            };
            shift += len as isize - range.len() as isize;
        }
        self.normalize();
    }

    /// Types `text` at every caret, replacing selections.
    pub fn type_text(&mut self, buf: &mut TextBuffer, text: &str) {
        let ranges: Vec<_> = self.carets.iter().map(Caret::range).collect();
        let texts = vec![text.to_owned(); ranges.len()];
        self.replace_each(buf, &ranges, &texts);
    }

    /// Deletes each selection, or the character before each empty caret.
    pub fn backspace(&mut self, buf: &mut TextBuffer) {
        let ranges: Vec<_> = self
            .carets
            .iter()
            .map(|c| {
                if c.is_empty() {
                    c.head.saturating_sub(1)..c.head
                } else {
                    c.range()
                }
            })
            .collect();
        let texts = vec![String::new(); ranges.len()];
        self.replace_each(buf, &ranges, &texts);
    }

    /// Deletes each selection, or the character after each empty caret.
    pub fn delete(&mut self, buf: &mut TextBuffer) {
        let len = buf.len_chars();
        let ranges: Vec<_> = self
            .carets
            .iter()
            .map(|c| {
                if c.is_empty() {
                    c.head..(c.head + 1).min(len)
                } else {
                    c.range()
                }
            })
            .collect();
        let texts = vec![String::new(); ranges.len()];
        self.replace_each(buf, &ranges, &texts);
    }

    /// Selected texts joined by line breaks.
    pub fn copy(&self, buf: &TextBuffer) -> String {
        self.carets
            .iter()
            .map(|c| buf.slice(c.range()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Pastes `text`: one line per caret when the line count matches, else
    /// the whole text at every caret.
    pub fn paste(&mut self, buf: &mut TextBuffer, text: &str) {
        let lines: Vec<String> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
            .collect();
        let ranges: Vec<_> = self.carets.iter().map(Caret::range).collect();
        let texts = if self.carets.len() > 1 && lines.len() == self.carets.len() {
            lines
        } else {
            vec![text.to_owned(); ranges.len()]
        };
        self.replace_each(buf, &ranges, &texts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(c: &Carets) -> Vec<(usize, usize)> {
        c.carets().iter().map(|c| (c.anchor, c.head)).collect()
    }

    #[test]
    fn normalisation_merges_and_keeps_primary() {
        let mut c = Carets::new(5);
        c.add_selection(1, 3);
        c.add_caret(5);
        c.add_selection(2, 4);
        assert_eq!(
            heads(&c),
            [(1, 4), (5, 5)],
            "overlap merged, equal empty merged"
        );
        assert_eq!(c.primary(), Caret::select(1, 4));
        c.add_caret(9);
        assert_eq!(c.primary().head, 9);
        c.add_selection(9, 12);
        assert_eq!(
            heads(&c),
            [(1, 4), (5, 5), (9, 12)],
            "empty caret at a selection start merges"
        );
        let mut back = Carets::new(0);
        back.carets = vec![Caret::select(6, 2), Caret::select(4, 8)];
        back.primary = 1;
        back.normalize();
        assert_eq!(heads(&back), [(8, 2)], "direction of the first kept");
    }

    #[test]
    fn movement_goal_column_words_and_collapse() {
        let buf = TextBuffer::from_text("long line\nab\nanother line\nüber_all straße");
        let mut c = Carets::new(7);
        c.move_all(&buf, Motion::Down, false);
        assert_eq!(c.primary().head, 12, "clamped to `ab` end");
        c.move_all(&buf, Motion::Down, false);
        assert_eq!(c.primary().head, 20, "goal column 7 restored");
        c.move_all(&buf, Motion::Up, false);
        c.move_all(&buf, Motion::Up, false);
        assert_eq!(c.primary().head, 7);
        c.move_all(&buf, Motion::Up, false);
        assert_eq!(c.primary().head, 7, "no line above");

        let start = buf.position_to_offset(Position::new(3, 0)).unwrap();
        let mut w = Carets::new(start);
        w.move_all(&buf, Motion::WordRight, true);
        assert_eq!(
            w.primary(),
            Caret::select(start, start + 8),
            "über_all is one word"
        );
        w.move_all(&buf, Motion::WordRight, true);
        assert_eq!(w.primary().head, start + 15);
        w.move_all(&buf, Motion::Left, false);
        assert_eq!(w.primary(), Caret::at(start), "collapse to start");
        w.move_all(&buf, Motion::LineEnd, false);
        w.move_all(&buf, Motion::WordLeft, false);
        assert_eq!(w.primary().head, start + 9);
        w.move_all(&buf, Motion::DocumentStart, false);
        assert_eq!(w.primary().head, 0);
        w.move_all(&buf, Motion::DocumentEnd, true);
        assert_eq!(w.primary(), Caret::select(0, buf.len_chars()));
    }

    /// SPEC-0130 AK-5 (v2).
    #[test]
    fn a_click_on_a_caret_removes_it() {
        let mut carets = Carets::new(2);
        assert!(!carets.toggle_caret(2), "the only caret stays");
        assert_eq!(carets.carets().len(), 1);
        assert!(!carets.toggle_caret(7));
        assert!(!carets.toggle_caret(11));
        let heads = |c: &Carets| c.carets().iter().map(|c| c.head).collect::<Vec<_>>();
        assert_eq!(heads(&carets), [2, 7, 11]);
        assert_eq!(carets.primary().head, 11);
        // The caret clicked again goes; the one added last before it is primary.
        assert!(carets.toggle_caret(7));
        assert_eq!(heads(&carets), [2, 11]);
        assert_eq!(carets.primary().head, 11);
        assert!(carets.toggle_caret(11));
        assert_eq!(heads(&carets), [2]);
        assert_eq!(carets.primary().head, 2);
        // A selection is not a caret to remove: the click adds one inside or beside it.
        let mut carets = Carets::from_carets(vec![Caret::select(0, 3), Caret::at(9)]);
        assert!(!carets.toggle_caret(20));
        assert_eq!(carets.carets().len(), 3);
    }

    #[test]
    fn vertical_carets_and_block_selection() {
        let buf = TextBuffer::from_text("abcdef\nab\nabcdef");
        let mut c = Carets::new(4);
        c.add_vertical(&buf, true);
        c.add_vertical(&buf, true);
        assert_eq!(
            heads(&c),
            [(4, 4), (9, 9), (14, 14)],
            "goal column survives `ab`"
        );
        c.add_vertical(&buf, true);
        assert_eq!(
            c.carets().len(),
            3,
            "nothing below the last line: {:?}",
            heads(&c)
        );

        let mut b = Carets::new(0);
        b.block(&buf, Position::new(0, 1), Position::new(2, 4));
        assert_eq!(heads(&b), [(1, 4), (8, 9), (11, 14)]);
        assert_eq!(b.primary(), Caret::select(11, 14));
    }

    #[test]
    fn occurrences() {
        let buf = TextBuffer::from_text("foo bar foo_x foo\nfoo");
        let mut c = Carets::new(14);
        assert!(c.add_next_occurrence(&buf));
        // The caret stood at the word's start and stays there (SPEC-0025 v3 behaviour 4a).
        assert_eq!(c.primary(), Caret::select(17, 14), "word at caret");
        assert!(c.add_next_occurrence(&buf));
        assert_eq!(c.primary(), Caret::select(21, 18));
        assert!(c.add_next_occurrence(&buf));
        assert_eq!(c.primary(), Caret::select(3, 0), "wrapped");
        // The word was selected from an empty caret: whole words only, `foo_x` is skipped (v4
        // behaviour 4b).
        assert!(!c.add_next_occurrence(&buf));
        // A selection made otherwise matches the exact text, also inside `foo_x`.
        let mut c = Carets::from_carets(vec![Caret::select(0, 3)]);
        assert!(c.add_next_occurrence(&buf));
        assert_eq!(
            c.primary(),
            Caret::select(8, 11),
            "inside foo_x: exact text match"
        );
        let mut all = Carets::new(5);
        all.select_all_occurrences(&buf);
        assert_eq!(heads(&all), [(4, 7)]);
        let mut none = Carets::new(3);
        none.carets[0] = Caret::at(3);
        let space = TextBuffer::from_text("a  b");
        none.carets[0] = Caret::at(2);
        assert!(!none.add_next_occurrence(&space));
    }

    /// SPEC-0025 v3 AK-9: added carets keep the caret's offset within the first selection — at
    /// the start from a caret at the word's start or a backward selection, at the end from a
    /// caret at the word's end or inside it (the latter Ferris IDEA's choice, not observed).
    #[test]
    fn occurrence_carets_keep_their_offset() {
        let buf = TextBuffer::from_text("foo one foo\nbar foo two\nfoo three");
        let all_heads = |c: &Carets| c.carets.iter().map(|c| c.head).collect::<Vec<_>>();
        // At the start: every caret at the start of its foo.
        let mut c = Carets::new(0);
        c.select_all_occurrences(&buf);
        assert_eq!(all_heads(&c), [0, 8, 16, 24]);
        assert!(c.carets.iter().all(|c| c.range().len() == 3));
        // At the end and inside the word: at the ends.
        for at in [3, 1] {
            let mut c = Carets::new(at);
            c.select_all_occurrences(&buf);
            assert_eq!(all_heads(&c), [3, 11, 19, 27], "caret at {at}");
        }
        // A selection made backwards (head before anchor): the added carets at the starts.
        let mut c = Carets::new(0);
        c.carets[0] = Caret::select(11, 8);
        assert!(c.add_next_occurrence(&buf));
        assert_eq!(c.primary(), Caret::select(19, 16));
        // Forwards: at the ends, as before.
        let mut c = Carets::new(0);
        c.carets[0] = Caret::select(8, 11);
        assert!(c.add_next_occurrence(&buf));
        assert_eq!(c.primary(), Caret::select(16, 19));
    }

    /// SPEC-0025 v4 AK-10: from an empty caret ⌃G and ⌃⌘G match whole words only; from a
    /// selection the exact text; an edit ends the whole-word row.
    #[test]
    fn occurrences_of_a_word_are_whole_words() {
        let buf = TextBuffer::from_text("x xz x");
        let mut all = Carets::new(0);
        all.select_all_occurrences(&buf);
        assert_eq!(heads(&all), [(1, 0), (6, 5)], "not the x in xz");
        let mut next = Carets::new(0);
        assert!(next.add_next_occurrence(&buf));
        assert!(next.whole_words());
        assert!(next.add_next_occurrence(&buf));
        assert!(!next.add_next_occurrence(&buf), "two carets");
        assert_eq!(next.carets().len(), 2);
        // From a selection: the exact text, three carets.
        let mut sel = Carets::from_carets(vec![Caret::select(0, 1)]);
        sel.select_all_occurrences(&buf);
        assert_eq!(sel.carets().len(), 3);
        // Another command in between ends the whole-word row.
        let mut ended = Carets::new(0);
        assert!(ended.add_next_occurrence(&buf));
        ended.end_occurrence_row();
        assert!(ended.add_next_occurrence(&buf));
        assert_eq!(ended.primary(), Caret::select(3, 2), "the x in xz");
    }

    /// SPEC-0025 v2 behaviour 8.
    #[test]
    fn unselect_occurrence() {
        let mut buf = TextBuffer::from_text("foo bar foo foo\nfoo");
        let mut c = Carets::new(1);
        assert!(c.add_next_occurrence(&buf));
        assert!(c.add_next_occurrence(&buf));
        assert!(c.add_next_occurrence(&buf));
        assert!(c.add_next_occurrence(&buf));
        assert_eq!(heads(&c), [(0, 3), (8, 11), (12, 15), (16, 19)]);
        // Typing keeps the order in which the carets were added.
        c.type_text(&mut buf, "ab");
        assert_eq!(buf.text(), "ab bar ab ab\nab");
        assert!(c.remove_last_added());
        assert_eq!(heads(&c), [(2, 2), (9, 9), (12, 12)]);
        assert_eq!(
            c.primary(),
            Caret::at(12),
            "the one added before is primary"
        );
        // A caret added below is the latest now.
        c.add_vertical(&buf, true);
        assert_eq!(c.carets().len(), 4);
        assert!(c.remove_last_added());
        assert_eq!(heads(&c), [(2, 2), (9, 9), (12, 12)]);
        assert!(c.remove_last_added());
        assert!(c.remove_last_added());
        assert_eq!(heads(&c), [(2, 2)]);
        assert!(!c.remove_last_added(), "one empty caret stays");
        // One selection: it is removed, the caret stays at its head.
        let mut one = Carets::from_carets(vec![Caret::select(4, 7)]);
        assert!(one.remove_last_added());
        assert_eq!(one.carets(), [Caret::at(7)]);
        // Occurrences added out of document order go in reverse order of adding.
        let buf = TextBuffer::from_text("x y x y x");
        let mut w = Carets::new(8);
        w.add_next_occurrence(&buf);
        w.add_next_occurrence(&buf);
        w.add_next_occurrence(&buf);
        // The caret stood at the start of its `x`, so every caret stands at a start (v3 4a).
        assert_eq!(heads(&w), [(1, 0), (5, 4), (9, 8)]);
        assert!(w.remove_last_added());
        assert_eq!(heads(&w), [(1, 0), (9, 8)]);
        assert_eq!(w.primary(), Caret::select(1, 0));
        assert!(w.remove_last_added());
        assert_eq!(heads(&w), [(9, 8)]);
    }

    #[test]
    fn editing_at_three_carets_is_one_undo_step() {
        let mut buf = TextBuffer::from_text("a1\nb2\nc3");
        let mut c = Carets::new(1);
        c.add_caret(4);
        c.add_caret(7);
        c.type_text(&mut buf, "xy");
        assert_eq!(buf.text(), "axy1\nbxy2\ncxy3");
        assert_eq!(heads(&c), [(3, 3), (8, 8), (13, 13)]);
        c.backspace(&mut buf);
        assert_eq!(buf.text(), "ax1\nbx2\ncx3");
        c.delete(&mut buf);
        assert_eq!(buf.text(), "ax\nbx\ncx");
        assert_eq!(heads(&c), [(2, 2), (5, 5), (8, 8)]);
        c.move_all(&buf, Motion::LineStart, true);
        c.type_text(&mut buf, "€");
        assert_eq!(buf.text(), "€\n€\n€");
        assert!(buf.undo());
        assert_eq!(buf.text(), "ax\nbx\ncx", "one undo per action");
    }

    #[test]
    fn copy_and_paste_distribution() {
        let mut buf = TextBuffer::from_text("one\ntwo\nsix");
        let mut c = Carets::new(0);
        c.block(&buf, Position::new(0, 0), Position::new(2, 3));
        assert_eq!(c.copy(&buf), "one\ntwo\nsix");
        c.paste(&mut buf, "1\r\n2\n3");
        assert_eq!(buf.text(), "1\n2\n3");
        c.paste(&mut buf, "ab");
        assert_eq!(buf.text(), "1ab\n2ab\n3ab");
    }
}
