//! Folded regions of an editor and the mapping between buffer lines and visible rows (`SPEC-0083`),
//! free of the UI toolkit. Rows of their own above some lines (usage counts, `SPEC-0292`) take part
//! in the mapping.

/// The folded regions of one editor: inclusive line ranges whose first line stays visible and whose
/// other lines are hidden.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    /// Folded regions, sorted by start line, none inside another.
    folded: Vec<(usize, usize)>,
    /// Folds kept inside a folded region, not shown while it is folded and folded again when it is
    /// opened (`SPEC-0083` v6 behaviour 13); sorted by start line, an outer one before those in it.
    kept: Vec<(usize, usize)>,
    /// Text shown in place of a folded region instead of `…`, by start line (one-line methods,
    /// `SPEC-0083` v2 behaviour 7).
    placeholders: Vec<(usize, String)>,
    /// How the regions of the text look while folded (`SPEC-0083` v8 behaviour 21); they move
    /// with edits until the next syntax tree brings new ones.
    looks: Vec<Look>,
    /// Lines with a usage count and its text, sorted by line (`SPEC-0292` behaviours 4–4a).
    above: Vec<(usize, String)>,
    /// The counts stand at the end of their lines and take no rows (`SPEC-0292` v2 behaviour 4);
    /// otherwise each is a row of its own above its line (behaviour 4a).
    at_line_end: bool,
}

impl Folds {
    /// The folded regions, sorted.
    pub fn regions(&self) -> &[(usize, usize)] {
        &self.folded
    }

    /// Whether the region starting at `start` is folded.
    pub fn is_folded(&self, start: usize) -> bool {
        self.folded.iter().any(|&(s, _)| s == start)
    }

    /// Whether the lines `start..=end` partly overlap a fold: they share lines with it and
    /// neither lies inside the other (`SPEC-0083` v7 behaviours 15 and 16).
    pub fn overlaps_partly(&self, start: usize, end: usize) -> bool {
        self.folded.iter().any(|&(s, e)| {
            let shares = s <= end && start <= e;
            let inside = s >= start && e <= end;
            let around = s <= start && e >= end;
            shares && !inside && !around
        })
    }

    /// Whether the lines `start..=end` lie inside a fold that is not exactly these lines.
    pub fn covers(&self, start: usize, end: usize) -> bool {
        self.folded
            .iter()
            .any(|&(s, e)| s <= start && end <= e && (s, e) != (start, end))
    }

    /// Folds `start..=end` (behaviour 2). Regions inside it are absorbed; folding inside a folded
    /// region does nothing. Regions of one line cannot be folded, and neither can lines that
    /// partly overlap a fold or lie inside one (v7: a fold made from a selection need not follow
    /// the syntax).
    pub fn fold(&mut self, start: usize, end: usize) {
        if end <= start
            || self.hidden(start)
            || self.overlaps_partly(start, end)
            || self.covers(start, end)
        {
            return;
        }
        self.folded.retain(|&(s, e)| !(s >= start && e <= end));
        self.kept.retain(|&(s, e)| !(s >= start && e <= end));
        self.folded.push((start, end));
        self.folded.sort_unstable();
        self.prune();
    }

    /// Folds `start..=end` like [`Folds::fold`] and keeps the regions of `inside` that lie in it
    /// folded for when it is opened again (`SPEC-0083` v6 behaviours 11–13). A region starting on
    /// the fold's own first line is not kept: it would fold that line again at once.
    pub fn fold_keeping(&mut self, start: usize, end: usize, inside: &[(usize, usize)]) {
        self.fold(start, end);
        if self.folded_at(start) != Some((start, end)) {
            return;
        }
        for &(s, e) in inside {
            if e > s && s > start && e <= end && !self.kept.contains(&(s, e)) {
                self.kept.push((s, e));
            }
        }
        self.kept
            .sort_unstable_by_key(|&(s, e)| (s, std::cmp::Reverse(e)));
    }

    /// The folds kept inside folded regions, sorted (for tests).
    pub fn kept(&self) -> &[(usize, usize)] {
        &self.kept
    }

    /// Folds the kept folds that are no longer inside a fold, outermost first. With `shown`, a line
    /// that has to stay visible, those that would hide it are dropped — with `whole` also the one
    /// starting on it (`SPEC-0083` v6 behaviour 13).
    fn restore(&mut self, shown: Option<usize>, whole: bool) {
        for (s, e) in std::mem::take(&mut self.kept) {
            if self.hidden(s) {
                self.kept.push((s, e));
                continue;
            }
            let hides = shown.is_some_and(|l| l <= e && if whole { l >= s } else { l > s });
            if !hides {
                self.folded.push((s, e));
            }
        }
        self.folded.sort_unstable();
    }

    /// Unfolds `start..=end` and everything folded or kept as folded inside it (`SPEC-0083` v6
    /// behaviour 11).
    pub fn unfold_within(&mut self, start: usize, end: usize) {
        self.folded.retain(|&(s, e)| !(s >= start && e <= end));
        self.kept.retain(|&(s, e)| !(s >= start && e <= end));
        self.prune();
    }

    /// Folds `start..=end` showing `placeholder` in its place (`SPEC-0083` v2 behaviour 7).
    pub fn fold_with(&mut self, start: usize, end: usize, placeholder: String) {
        self.fold(start, end);
        if self.folded_at(start) == Some((start, end)) {
            self.placeholders.retain(|(s, _)| *s != start);
            self.placeholders.push((start, placeholder));
        }
    }

    /// The text shown in place of the fold starting at `start`, if it has its own.
    pub fn placeholder(&self, start: usize) -> Option<&str> {
        self.placeholders
            .iter()
            .find(|(s, _)| *s == start)
            .map(|(_, t)| t.as_str())
    }

    /// Sets how the regions of the text look while folded (`SPEC-0083` v8 behaviour 21).
    pub fn set_looks(&mut self, looks: Vec<Look>) {
        self.looks = looks;
    }

    /// The look of the fold starting at `start`: that of the region with exactly its lines.
    /// `None` for a fold with a placeholder of its own, for one that is no region of the text
    /// (made from a selection) and for a line that is not folded.
    pub fn look(&self, start: usize) -> Option<&Look> {
        let (_, end) = self.folded_at(start)?;
        if self.placeholder(start).is_some() {
            return None;
        }
        self.looks.iter().find(|l| (l.start, l.end) == (start, end))
    }

    /// How many chars of line `line` are shown when a fold starts there whose placeholder
    /// stands for the rest of the line (`SPEC-0083` v8 behaviour 21).
    pub fn kept_chars(&self, line: usize) -> Option<usize> {
        self.look(line)?.keep
    }

    /// The placeholder of the fold starting at `start`: its own, that of its region, or three
    /// dots (`SPEC-0083` v8 behaviour 21). `None` when no fold starts there.
    pub fn shown_placeholder(&self, start: usize) -> Option<&str> {
        self.folded_at(start)?;
        Some(
            self.placeholder(start)
                .or_else(|| self.look(start).map(|l| l.text.as_str()))
                .unwrap_or(ferris_idea_languages::folding::ELLIPSIS),
        )
    }

    /// Forgets the kept folds among `regions`, so they stay open when their outer fold is
    /// opened (`SPEC-0083` v8 behaviour 20).
    pub fn forget_kept(&mut self, regions: &[(usize, usize)]) {
        self.kept.retain(|r| !regions.contains(r));
    }

    /// Drops placeholders of regions that are no longer folded, and kept folds that are no longer
    /// inside a fold.
    fn prune(&mut self) {
        let folded = &self.folded;
        self.placeholders
            .retain(|(s, _)| folded.iter().any(|&(f, _)| f == *s));
        self.kept
            .retain(|&(s, e)| folded.iter().any(|&(fs, fe)| s > fs && e <= fe));
    }

    /// Unfolds the region starting at `start`; the folds kept inside it are folded (v6 behaviour
    /// 13).
    pub fn unfold(&mut self, start: usize) {
        self.folded.retain(|&(s, _)| s != start);
        self.restore(None, false);
        self.prune();
    }

    /// Unfolds every region that hides `line`, so a caret or a jump there is visible (behaviour 4).
    pub fn reveal(&mut self, line: usize) -> bool {
        let before = self.folded.len();
        self.folded.retain(|&(s, e)| !(line > s && line <= e));
        let changed = before != self.folded.len();
        if changed {
            self.restore(Some(line), false);
        }
        self.prune();
        changed
    }

    /// Unfolds every region a jump to `line` lands in: those hiding it and the one starting on it, so
    /// a method folded to one line opens (`SPEC-0071` v7 behaviour 10).
    pub fn open_at(&mut self, line: usize) -> bool {
        let before = self.folded.len();
        self.folded.retain(|&(s, e)| !(line >= s && line <= e));
        let changed = before != self.folded.len();
        if changed {
            self.restore(Some(line), true);
        }
        self.prune();
        changed
    }

    /// Unfolds everything.
    pub fn clear(&mut self) {
        self.folded.clear();
        self.kept.clear();
        self.placeholders.clear();
    }

    /// Whether `line` is hidden by a fold.
    pub fn hidden(&self, line: usize) -> bool {
        self.folded.iter().any(|&(s, e)| line > s && line <= e)
    }

    /// The fold whose first line is `line`, if any (shown with a placeholder).
    pub fn folded_at(&self, line: usize) -> Option<(usize, usize)> {
        self.folded.iter().copied().find(|&(s, _)| s == line)
    }

    /// Hidden lines before `line`.
    fn hidden_before(&self, line: usize) -> usize {
        self.folded
            .iter()
            .map(|&(s, e)| {
                if line > e {
                    e - s
                } else if line > s {
                    line - s - 1
                } else {
                    0
                }
            })
            .sum()
    }

    /// Sets the rows above lines: line and text (`SPEC-0292` behaviour 4).
    pub fn set_above(&mut self, mut above: Vec<(usize, String)>) {
        above.sort_by_key(|(l, _)| *l);
        above.dedup_by_key(|(l, _)| *l);
        self.above = above;
    }

    /// The rows above lines.
    pub fn above(&self) -> &[(usize, String)] {
        &self.above
    }

    /// Puts the counts at the end of their lines (`true`, no rows of their own) or on rows above
    /// them (`SPEC-0292` v2 behaviours 4 and 4a).
    pub fn set_at_line_end(&mut self, at_line_end: bool) {
        self.at_line_end = at_line_end;
    }

    /// Whether the counts stand at the end of their lines.
    pub fn at_line_end(&self) -> bool {
        self.at_line_end
    }

    /// The text drawn at the end of `line` when the counts stand there (`SPEC-0292` v2
    /// behaviour 4).
    pub fn line_end_text(&self, line: usize) -> Option<&str> {
        if !self.at_line_end {
            return None;
        }
        self.above
            .iter()
            .find(|(l, _)| *l == line)
            .map(|(_, t)| t.as_str())
    }

    /// Shown rows above lines at or before `line`; a row above a hidden line is hidden too.
    fn above_up_to(&self, line: usize) -> usize {
        if self.at_line_end {
            return 0;
        }
        self.above
            .iter()
            .take_while(|(l, _)| *l <= line)
            .filter(|(l, _)| !self.hidden(*l))
            .count()
    }

    /// Number of visible rows of a buffer with `lines` lines.
    pub fn rows(&self, lines: usize) -> usize {
        let shown = lines.saturating_sub(self.hidden_before(lines));
        match lines.checked_sub(1) {
            Some(last) => shown + self.above_up_to(last),
            None => shown,
        }
    }

    /// The visible row of `line`; a hidden line maps to the row of its fold.
    pub fn row_of_line(&self, line: usize) -> usize {
        let line = self
            .folded
            .iter()
            .find(|&&(s, e)| line > s && line <= e)
            .map_or(line, |&(s, _)| s);
        line - self.hidden_before(line) + self.above_up_to(line)
    }

    /// The buffer line shown in visible row `row`; a row above a line maps to that line.
    pub fn line_of_row(&self, row: usize) -> usize {
        if self.above.is_empty() || self.at_line_end {
            return self.plain_line_of_row(row);
        }
        // The first line whose row is at or after `row` (rows only grow with lines).
        let (mut lo, mut hi) = (0, self.plain_line_of_row(row));
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.row_of_line(mid) >= row {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo
    }

    /// The line and text of the row above a line shown in visible row `row`, if it is one.
    pub fn above_row(&self, row: usize) -> Option<(usize, &str)> {
        if self.at_line_end {
            return None;
        }
        let line = self.line_of_row(row);
        if self.row_of_line(line) != row + 1 || self.hidden(line) {
            return None;
        }
        self.above
            .iter()
            .find(|(l, _)| *l == line)
            .map(|(l, t)| (*l, t.as_str()))
    }

    /// The buffer line in row `row` counting folds only.
    fn plain_line_of_row(&self, row: usize) -> usize {
        let mut line = row;
        for &(s, e) in &self.folded {
            if s < line {
                line += e - s;
            } else {
                break;
            }
        }
        line
    }

    /// Moves folds after `added` whole lines were put in before line `first` and no other line
    /// changed (`SPEC-0083` v4 behaviour 5a): folds from `first` on shift, folds above stay folded —
    /// also one that ends on the line before — and only a fold the new lines landed inside is
    /// unfolded.
    pub fn lines_inserted(&mut self, first: usize, added: usize) {
        self.folded.retain(|&(s, e)| e < first || s >= first);
        for (s, e) in &mut self.folded {
            if *s >= first {
                *s += added;
                *e += added;
            }
        }
        for (s, _) in &mut self.placeholders {
            if *s >= first {
                *s += added;
            }
        }
        // A kept fold lies inside its outer fold and moves as it does.
        for (s, e) in &mut self.kept {
            if *s >= first {
                *s += added;
                *e += added;
            }
        }
        self.looks.retain(|l| l.end < first || l.start >= first);
        for look in &mut self.looks {
            if look.start >= first {
                look.start += added;
                look.end += added;
            }
        }
        self.prune();
        for (l, _) in &mut self.above {
            if *l >= first {
                *l += added;
            }
        }
    }

    /// Moves folds after an edit that turned `removed` lines starting at `line` into `added` lines
    /// (behaviour 5): folds below shift, folds touched by the edit are unfolded.
    pub fn edited(&mut self, line: usize, removed: usize, added: usize) {
        let last = line + removed;
        self.folded.retain(|&(s, e)| e < line || s > last);
        for (s, e) in &mut self.folded {
            if *s > last {
                *s = *s + added - removed;
                *e = *e + added - removed;
            }
        }
        for (s, _) in &mut self.placeholders {
            if *s > last {
                *s = *s + added - removed;
            }
        }
        for (s, e) in &mut self.kept {
            if *s > last {
                *s = *s + added - removed;
                *e = *e + added - removed;
            }
        }
        self.looks.retain(|l| l.end < line || l.start > last);
        for look in &mut self.looks {
            if look.start > last {
                look.start = look.start + added - removed;
                look.end = look.end + added - removed;
            }
        }
        self.prune();
        // A row above an edited line goes; rows below move (SPEC-0292 behaviour 3).
        self.above.retain(|(l, _)| *l < line || *l > last);
        for (l, _) in &mut self.above {
            if *l > last {
                *l = *l + added - removed;
            }
        }
    }
}

pub use ferris_idea_languages::folding::{Look, innermost_around, level_folds, levels};

/// The block Fold Code Block folds (`SPEC-0083` v7 behaviour 15): of `blocks`, the code blocks
/// around the caret from the innermost outwards, the first that spans more than one line and is
/// neither folded as exactly its lines nor hidden inside a fold. `None` when there is none or
/// when that block partly overlaps a fold.
pub fn block_to_fold(blocks: &[(usize, usize)], folds: &Folds) -> Option<(usize, usize)> {
    for &(start, end) in blocks {
        if end <= start || folds.folded_at(start) == Some((start, end)) || folds.covers(start, end)
        {
            continue;
        }
        return (!folds.overlaps_partly(start, end)).then_some((start, end));
    }
    None
}

/// What Fold Selection / Remove Region does (`SPEC-0083` v7 behaviour 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionFold {
    /// Fold these lines, the first staying visible.
    Fold(usize, usize),
    /// Open the fold that starts on this line.
    Unfold(usize),
    /// The selection is within one line: nothing to fold.
    OneLine,
    /// The selected lines partly overlap a fold: nothing is folded.
    Overlaps,
    /// No selection, no fold on the caret's line and no region around it.
    Nothing,
}

/// Decides Fold Selection / Remove Region (`SPEC-0083` v7 behaviour 16). `selection` is the
/// first and the last line of the primary caret's selection (a line break at its end not
/// counted), `line` the caret's line and `regions` the foldable regions of the syntax tree.
pub fn selection_fold(
    folds: &Folds,
    regions: &[(usize, usize)],
    selection: Option<(usize, usize)>,
    line: usize,
) -> SelectionFold {
    match selection {
        Some((first, last)) if last <= first => SelectionFold::OneLine,
        Some((first, last)) if folds.folded_at(first) == Some((first, last)) => {
            SelectionFold::Unfold(first)
        }
        Some((first, last))
            if folds.overlaps_partly(first, last)
                || folds.covers(first, last)
                || folds.hidden(first) =>
        {
            SelectionFold::Overlaps
        }
        Some((first, last)) => SelectionFold::Fold(first, last),
        None if folds.folded_at(line).is_some() => SelectionFold::Unfold(line),
        None => regions
            .iter()
            .filter(|&&(s, e)| {
                s <= line
                    && line <= e
                    && !folds.is_folded(s)
                    && !folds.overlaps_partly(s, e)
                    && !folds.covers(s, e)
            })
            .min_by_key(|&&(s, e)| e - s)
            .map_or(SelectionFold::Nothing, |&(s, e)| SelectionFold::Fold(s, e)),
    }
}

/// How the placeholder of a one-line method is drawn (`SPEC-0083` v3 behaviour 7): the statement
/// with the colours of the hidden line, the closing brace on the folded-text background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderLook<S> {
    /// Coloured byte ranges of the placeholder (the statement's pieces with a colour).
    pub coloured: Vec<(std::ops::Range<usize>, S)>,
    /// Byte range of the closing brace in the placeholder.
    pub brace: std::ops::Range<usize>,
}

/// The look of `placeholder` (` statement }`) whose statement is the text of `hidden`, the folded
/// line, coloured by `colours` (char columns of `hidden`). `None` when the placeholder is not of
/// that form; then it is drawn as any other placeholder.
pub fn placeholder_look<S: Clone>(
    placeholder: &str,
    hidden: &str,
    colours: &[(std::ops::Range<usize>, S)],
) -> Option<PlaceholderLook<S>> {
    let statement = hidden.trim();
    let rest = placeholder.strip_prefix(' ')?.strip_suffix('}')?;
    if statement.is_empty() || rest.strip_suffix(' ')? != statement {
        return None;
    }
    // Char column of the statement in `hidden`, and the byte of each of its chars in the
    // placeholder (one past the end too).
    let lead = hidden.chars().take_while(|c| c.is_whitespace()).count();
    let chars = statement.chars().count();
    let byte_of = |k: usize| -> usize {
        1 + statement
            .char_indices()
            .nth(k)
            .map_or(statement.len(), |(b, _)| b)
    };
    let coloured = colours
        .iter()
        .filter_map(|(r, s)| {
            let from = r.start.max(lead) - lead;
            let to = r.end.min(lead + chars).saturating_sub(lead);
            (from < to).then(|| (byte_of(from)..byte_of(to), s.clone()))
        })
        .collect();
    Some(PlaceholderLook {
        coloured,
        brace: placeholder.len() - 1..placeholder.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0083 v3 AK-6: the statement keeps the hidden line's colours, the brace is found.
    #[test]
    fn placeholder_keeps_the_statements_colours() {
        let hidden = "\t    return qty * price;";
        // `return` keyword, `qty` field, `price` field (char columns of `hidden`).
        let colours = [
            (5..11, "kw"),
            (12..15, "field"),
            (18..23, "field"),
            (0..2, "none"),
        ];
        let look = placeholder_look(" return qty * price; }", hidden, &colours).unwrap();
        assert_eq!(
            look.coloured,
            [(1..7, "kw"), (8..11, "field"), (14..19, "field")]
        );
        assert_eq!(look.brace, 21..22);
        let text = " return qty * price; }";
        assert_eq!(&text[look.brace.clone()], "}");
        // Non-ASCII text maps by characters.
        let look =
            placeholder_look(" s = \"ä\" + b; }", "  s = \"ä\" + b;", &[(10..11, "v")]).unwrap();
        assert_eq!(&" s = \"ä\" + b; }"[look.coloured[0].0.clone()], "+");
        // Other placeholders are not of this form.
        assert!(placeholder_look(" … ", "x", &colours).is_none());
        assert!(placeholder_look(" other(); }", "  this(); ", &colours).is_none());
    }

    /// SPEC-0292 v2 AK-5: counts at the end of the line add no rows; switching moves them.
    #[test]
    fn counts_at_the_line_end_take_no_rows() {
        let mut f = Folds::default();
        f.set_above(vec![(6, "1 usage".into()), (2, "3 usages".into())]);
        f.set_at_line_end(true);
        assert_eq!(f.rows(12), 12);
        for line in 0..12 {
            assert_eq!(f.row_of_line(line), line);
            assert_eq!(f.line_of_row(line), line);
            assert_eq!(f.above_row(line), None);
        }
        assert_eq!(f.line_end_text(2), Some("3 usages"));
        assert_eq!(f.line_end_text(6), Some("1 usage"));
        assert_eq!(f.line_end_text(3), None);
        f.fold(6, 9);
        assert_eq!(f.rows(12), 9);
        assert_eq!(f.line_end_text(6), Some("1 usage"));
        f.clear();
        // Edits move and drop counts as with rows.
        f.edited(0, 0, 1);
        assert_eq!(f.line_end_text(3), Some("3 usages"));
        f.edited(7, 0, 0);
        assert_eq!(f.line_end_text(7), None);
        // Back to rows above: the remaining count takes its row again.
        f.set_at_line_end(false);
        assert_eq!(f.rows(13), 14);
        assert_eq!(f.above_row(3), Some((3, "3 usages")));
        assert_eq!(f.line_end_text(3), None);
    }

    #[test]
    fn rows_and_lines_map_both_ways() {
        let mut f = Folds::default();
        f.fold(2, 5); // lines 3–5 hidden
        f.fold(8, 9); // line 9 hidden
        assert_eq!(f.rows(12), 8);
        let lines: Vec<usize> = (0..8).map(|r| f.line_of_row(r)).collect();
        assert_eq!(lines, [0, 1, 2, 6, 7, 8, 10, 11]);
        for (row, &line) in lines.iter().enumerate() {
            assert_eq!(f.row_of_line(line), row);
        }
        assert_eq!(f.row_of_line(4), 2);
        assert!(f.hidden(5) && !f.hidden(2) && !f.hidden(6));
        assert_eq!(f.folded_at(8), Some((8, 9)));
    }

    #[test]
    fn folding_absorbs_inner_folds_and_reveal_unfolds() {
        let mut f = Folds::default();
        f.fold(3, 4);
        f.fold(1, 6);
        assert_eq!(f.regions(), [(1, 6)]);
        f.fold(2, 3); // inside a fold: ignored
        assert_eq!(f.regions(), [(1, 6)]);
        f.fold(7, 7); // one line: ignored
        assert!(f.reveal(4));
        assert!(f.regions().is_empty());
        assert!(!f.reveal(4));
    }

    /// SPEC-0083 v6 AK-13.
    #[test]
    fn folds_kept_inside_a_fold() {
        // A class 0–20 with methods 2–6 (a block 3–5 inside) and 8–12.
        let regions = [(0, 20), (2, 6), (3, 5), (8, 12)];
        let mut f = Folds::default();
        f.fold_keeping(0, 20, &regions);
        assert_eq!(f.regions(), [(0, 20)]);
        assert_eq!(f.kept(), [(2, 6), (3, 5), (8, 12)]);
        // Unfolding the outer fold folds the outermost kept ones; the block stays kept.
        f.unfold(0);
        assert_eq!(f.regions(), [(2, 6), (8, 12)]);
        assert_eq!(f.kept(), [(3, 5)]);
        f.unfold(2);
        assert_eq!(f.regions(), [(3, 5), (8, 12)]);
        assert!(f.kept().is_empty());

        // A reveal drops only the kept folds hiding the line.
        let mut f = Folds::default();
        f.fold_keeping(0, 20, &regions);
        assert!(f.reveal(4));
        assert_eq!(f.regions(), [(8, 12)], "2–6 and 3–5 would hide line 4");
        let mut f = Folds::default();
        f.fold_keeping(0, 20, &regions);
        assert!(f.reveal(2));
        assert_eq!(f.regions(), [(2, 6), (8, 12)], "line 2 is a first line");
        // A jump opens the fold starting on its line too; the block in it comes back.
        let mut f = Folds::default();
        f.fold_keeping(0, 20, &regions);
        assert!(f.open_at(2));
        assert_eq!(f.regions(), [(3, 5), (8, 12)]);

        // Kept folds move with their outer fold and go with an edit inside it.
        let mut f = Folds::default();
        f.fold_keeping(2, 6, &regions);
        f.lines_inserted(1, 2);
        assert_eq!((f.regions(), f.kept()), (&[(4, 8)][..], &[(5, 7)][..]));
        f.edited(0, 1, 0);
        assert_eq!((f.regions(), f.kept()), (&[(3, 7)][..], &[(4, 6)][..]));
        f.edited(5, 0, 0);
        assert!(f.regions().is_empty() && f.kept().is_empty());

        // A plain fold keeps nothing, also over folds that kept some; unfolding within drops all.
        let mut f = Folds::default();
        f.fold_keeping(2, 6, &regions);
        f.fold(0, 20);
        assert!(f.kept().is_empty());
        f.unfold(0);
        assert!(f.regions().is_empty());
        f.fold_keeping(0, 20, &regions);
        f.unfold_within(0, 20);
        assert!(f.regions().is_empty() && f.kept().is_empty());
        f.fold_keeping(0, 20, &regions);
        f.clear();
        assert!(f.kept().is_empty());
        // A region on the fold's own first line is not kept.
        f.fold_keeping(0, 20, &[(0, 4), (5, 6)]);
        assert_eq!(f.kept(), [(5, 6)]);
    }

    /// SPEC-0083 v8 AK-23: a fold takes the look of its region, a fold with its own placeholder
    /// and one made from a selection do not; looks move with edits and go with touched lines.
    #[test]
    fn folds_take_the_look_of_their_region() {
        let look = |start, end, keep, text: &str| Look {
            start,
            end,
            keep,
            text: text.into(),
        };
        let mut f = Folds::default();
        f.set_looks(vec![
            look(0, 2, Some(7), "..."),
            look(4, 9, None, "...}"),
            look(5, 7, None, " return a; }"),
            look(11, 13, Some(4), "Fields"),
        ]);
        assert_eq!(f.shown_placeholder(0), None, "nothing is folded");
        f.fold(0, 2);
        f.fold(4, 9);
        f.fold(11, 13);
        assert_eq!(f.shown_placeholder(0), Some("..."));
        assert_eq!(f.kept_chars(0), Some(7));
        assert_eq!(f.shown_placeholder(4), Some("...}"));
        assert_eq!(f.kept_chars(4), None);
        assert_eq!(f.shown_placeholder(11), Some("Fields"));
        // Lines 4–8 folded from a selection are no region: three dots, the whole line.
        f.unfold(4);
        f.fold(4, 8);
        assert_eq!(f.shown_placeholder(4), Some("..."));
        assert_eq!(f.kept_chars(4), None);
        f.unfold(4);
        // A placeholder of its own wins.
        f.unfold(0);
        f.fold_with(0, 2, " own }".into());
        assert_eq!(f.shown_placeholder(0), Some(" own }"));
        assert_eq!(f.kept_chars(0), None);
        // Two whole lines put in before line 10: the region below moves with its look; an edit
        // in line 0 drops fold and look there.
        f.lines_inserted(10, 2);
        assert_eq!(f.regions(), [(0, 2), (13, 15)]);
        assert_eq!(f.shown_placeholder(13), Some("Fields"));
        assert_eq!(f.kept_chars(13), Some(4));
        f.edited(1, 0, 0);
        assert_eq!(f.regions(), [(13, 15)]);
        f.edited(3, 1, 0);
        assert_eq!(f.regions(), [(12, 14)]);
        assert_eq!(f.shown_placeholder(12), Some("Fields"));
        // Kept folds can be forgotten.
        let mut f = Folds::default();
        f.fold_keeping(0, 20, &[(2, 5), (7, 9)]);
        f.forget_kept(&[(2, 5)]);
        f.unfold(0);
        assert_eq!(f.regions(), [(7, 9)]);
    }

    /// SPEC-0071 v7 AK-6: a jump to the first line of a fold opens it; other folds stay.
    #[test]
    fn a_jump_opens_the_fold_it_lands_in() {
        let mut f = Folds::default();
        f.fold(4, 6);
        f.fold(10, 13);
        assert!(
            !f.reveal(10),
            "reveal keeps a fold whose first line is shown"
        );
        assert!(f.open_at(10));
        assert_eq!(f.regions(), [(4, 6)]);
        assert!(f.open_at(5));
        assert!(f.regions().is_empty());
        assert!(!f.open_at(8));
    }

    /// SPEC-0083 v2 AK-4: a fold with its own placeholder keeps it while folded, moves with edits
    /// and loses it when unfolded.
    #[test]
    fn placeholders_follow_their_folds() {
        let mut f = Folds::default();
        f.fold_with(2, 4, " return a; }".into());
        f.fold(8, 10);
        assert_eq!(f.placeholder(2), Some(" return a; }"));
        assert_eq!(f.placeholder(8), None);
        f.edited(0, 0, 1);
        assert_eq!(f.placeholder(3), Some(" return a; }"));
        assert!(f.reveal(4));
        assert_eq!(f.placeholder(3), None);
        f.fold_with(3, 5, "x".into());
        // Absorbed by an outer fold, which takes the other fold in whole as well: lines that
        // only partly overlap a fold are not folded (v7 behaviour 17).
        f.fold(0, 12);
        assert_eq!(f.placeholder(3), None);
        f.clear();
        f.fold_with(3, 3, "one line".into()); // not foldable
        assert_eq!(f.placeholder(3), None);
    }

    /// SPEC-0083 AK-9 (v4): whole lines put in after a fold's last line, or before its first, leave
    /// it folded with its placeholder; lines put inside it unfold it; rows above lines move.
    #[test]
    fn inserted_lines_keep_the_folds_beside_them() {
        let mut f = Folds::default();
        f.fold_with(2, 4, " return a; }".into());
        f.fold(8, 10);
        f.set_above(vec![(2, "1 usage".into()), (8, "no usages".into())]);
        // A member added after the closing brace of the first method (line 4): lines 5–7 are new.
        f.lines_inserted(5, 3);
        assert_eq!(f.regions(), [(2, 4), (11, 13)]);
        assert_eq!(f.placeholder(2), Some(" return a; }"));
        assert_eq!(f.above(), [(2, "1 usage".into()), (11, "no usages".into())]);
        // Lines put in right before the first fold's signature line move it whole.
        f.lines_inserted(2, 2);
        assert_eq!(f.regions(), [(4, 6), (13, 15)]);
        assert_eq!(f.placeholder(4), Some(" return a; }"));
        // New lines inside a fold open it; the other stays.
        f.lines_inserted(5, 1);
        assert_eq!(f.regions(), [(14, 16)]);
        assert_eq!(f.placeholder(4), None);
        // The old rule for comparison: an edit on a fold's last line unfolds it.
        f.edited(16, 0, 1);
        assert!(f.regions().is_empty());
    }

    /// SPEC-0292 AK-2: rows above lines 2 and 6 with a fold over 6–9 and one over 3–4.
    #[test]
    fn rows_above_lines_take_part_in_the_mapping() {
        let mut f = Folds::default();
        f.set_above(vec![(6, "1 usage".into()), (2, "3 usages".into())]);
        // Rows: 0, 1, [above 2], 2, 3, 4, 5, [above 6], 6, 7, 8, 9, 10, 11
        assert_eq!(f.rows(12), 14);
        assert_eq!(f.row_of_line(2), 3);
        assert_eq!(f.row_of_line(6), 8);
        assert_eq!(f.line_of_row(2), 2);
        assert_eq!(f.above_row(2), Some((2, "3 usages")));
        assert_eq!(f.above_row(3), None);
        assert_eq!(f.above_row(7), Some((6, "1 usage")));
        for line in 0..12 {
            assert_eq!(f.line_of_row(f.row_of_line(line)), line, "line {line}");
        }
        assert_max_scroll(&f, 12, 14);
        // A fold keeps the row above its first line; a row above a hidden line hides with it.
        f.fold(6, 9);
        f.fold(1, 2);
        // Rows: 0, 1, 3, 4, 5, [above 6], 6, 10, 11
        assert_eq!(f.rows(12), 9);
        assert_eq!(f.above_row(1), None);
        assert_eq!(f.above_row(5), Some((6, "1 usage")));
        assert_eq!(f.line_of_row(6), 6);
        assert_eq!(f.line_of_row(7), 10);
        assert_eq!(f.row_of_line(8), 6);
        assert_eq!(f.row_of_line(2), 1);
        f.clear();
        // Edits: a line inserted above moves rows; an edit on the line drops its row.
        f.edited(0, 0, 1);
        assert_eq!(f.above(), [(3, "3 usages".into()), (7, "1 usage".into())]);
        f.edited(7, 0, 0);
        assert_eq!(f.above(), [(3, "3 usages".into())]);
    }

    fn assert_max_scroll(f: &Folds, lines: usize, rows: usize) {
        assert_eq!(f.rows(lines), rows);
        assert_eq!(f.line_of_row(rows - 1), lines - 1);
    }

    #[test]
    fn edits_shift_or_unfold() {
        let mut f = Folds::default();
        f.fold(2, 4);
        f.fold(10, 12);
        f.edited(6, 0, 2); // two lines added between the folds
        assert_eq!(f.regions(), [(2, 4), (12, 14)]);
        f.edited(3, 1, 1); // an edit inside the first fold unfolds it
        assert_eq!(f.regions(), [(12, 14)]);
        f.edited(0, 3, 0);
        assert_eq!(f.regions(), [(9, 11)]);
    }

    /// SPEC-0083 v7 AK-17: Fold Code Block takes the innermost open block and, used again, the
    /// next one outwards; a block on one line is passed over; a partly overlapping fold ends the
    /// walk.
    #[test]
    fn fold_code_block_goes_outwards() {
        // The `if` on one line, the loop 5–9, the method 3–11.
        let blocks = [(6, 6), (5, 9), (3, 11)];
        let mut f = Folds::default();
        assert_eq!(block_to_fold(&blocks, &f), Some((5, 9)));
        f.fold(5, 9);
        assert_eq!(block_to_fold(&blocks, &f), Some((3, 11)));
        f.fold(3, 11);
        assert_eq!(f.regions(), [(3, 11)], "the inner fold is absorbed");
        assert_eq!(block_to_fold(&blocks, &f), None, "everything is folded");
        assert_eq!(block_to_fold(&[], &f), None);
        assert_eq!(block_to_fold(&[(4, 4)], &Folds::default()), None);

        // A block hidden inside a fold is passed over as well.
        let mut f = Folds::default();
        f.fold(3, 11);
        assert_eq!(
            block_to_fold(&[(3, 7), (3, 11), (1, 12)], &f),
            Some((1, 12))
        );

        // A fold made from a selection that partly overlaps the block ends the walk: the next
        // block outwards is not folded in its place.
        let mut f = Folds::default();
        f.fold(8, 10);
        assert!(f.overlaps_partly(5, 9));
        assert!(!f.overlaps_partly(3, 11), "the method contains it");
        assert!(!f.overlaps_partly(8, 10) && !f.overlaps_partly(9, 10));
        assert!(!f.overlaps_partly(11, 12) && !f.overlaps_partly(0, 7));
        assert_eq!(block_to_fold(&blocks, &f), None);
        // And `fold` itself refuses such lines, so the rows stay right.
        f.fold(5, 9);
        assert_eq!(f.regions(), [(8, 10)]);
        f.fold(9, 12);
        assert_eq!(f.regions(), [(8, 10)]);
        assert_eq!(f.rows(20), 18);
    }

    /// SPEC-0083 v7 AK-17: Fold Selection folds the selected lines, opens the same lines again,
    /// refuses one line and a partial overlap, and without a selection opens the fold on the
    /// line or folds the region around it.
    #[test]
    fn fold_selection_by_lines() {
        use SelectionFold::*;
        let regions = [(2, 12), (3, 11), (5, 9)];
        let mut f = Folds::default();
        // Lines 6–8, whatever the syntax says.
        assert_eq!(selection_fold(&f, &regions, Some((6, 8)), 8), Fold(6, 8));
        f.fold(6, 8);
        assert_eq!(f.regions(), [(6, 8)]);
        assert_eq!((f.rows(14), f.row_of_line(9)), (12, 7));
        // The same lines again: the fold is opened.
        assert_eq!(selection_fold(&f, &regions, Some((6, 8)), 6), Unfold(6));
        // Within one line.
        assert_eq!(selection_fold(&f, &regions, Some((4, 4)), 4), OneLine);
        // Partly overlapping: 4–7 and 7–10 cross the fold's edge, 6–7 lies inside it.
        assert_eq!(selection_fold(&f, &regions, Some((4, 7)), 4), Overlaps);
        assert_eq!(selection_fold(&f, &regions, Some((7, 10)), 10), Overlaps);
        assert_eq!(selection_fold(&f, &regions, Some((6, 7)), 6), Overlaps);
        // Lines around it: folded, the inner fold absorbed.
        assert_eq!(selection_fold(&f, &regions, Some((4, 10)), 10), Fold(4, 10));
        f.fold(4, 10);
        assert_eq!(f.regions(), [(4, 10)]);

        // Without a selection: the fold on the caret's line is opened …
        assert_eq!(selection_fold(&f, &regions, None, 4), Unfold(4));
        f.unfold(4);
        assert!(
            f.regions().is_empty(),
            "a fold from a selection is gone once opened"
        );
        // … else the innermost region around the caret is folded …
        assert_eq!(selection_fold(&f, &regions, None, 7), Fold(5, 9));
        assert_eq!(selection_fold(&f, &regions, None, 4), Fold(3, 11));
        f.fold(5, 9);
        assert_eq!(selection_fold(&f, &regions, None, 5), Unfold(5));
        // … and outside every region nothing happens.
        assert_eq!(selection_fold(&f, &regions, None, 0), Nothing);
        assert_eq!(selection_fold(&f, &[], None, 7), Nothing);
        // A region a selection's fold partly overlaps is not folded.
        let mut f = Folds::default();
        f.fold(8, 10);
        assert_eq!(selection_fold(&f, &regions, None, 6), Fold(3, 11));
    }
}
