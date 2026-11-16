//! Resolved configuration values shown in place of `${…}` placeholders (`SPEC-0115` v5 behaviour
//! 13), free of the UI toolkit: the placeholders of a file at char offsets, moved by edits, and a
//! line's display text with the values in their place. The text itself is never changed, and a
//! placeholder a caret touches is shown as written.

use std::ops::Range;

use crate::layout::DisplayLine;

/// One placeholder and what is shown for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueFold {
    /// Char range of the placeholder in the document, `${` and `}` included.
    pub range: Range<usize>,
    /// The text shown: the value, or the mask of a secret.
    pub text: String,
}

/// The placeholders of one editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueFolds {
    /// Sorted by start, none overlapping.
    folds: Vec<ValueFold>,
}

impl ValueFolds {
    /// The folds, sorted; an empty range, an empty text and a fold that overlaps the one before
    /// it are dropped.
    pub fn new(mut folds: Vec<ValueFold>) -> Self {
        folds.retain(|f| f.range.start < f.range.end && !f.text.is_empty());
        folds.sort_by_key(|f| f.range.start);
        let mut end = 0;
        folds.retain(|f| {
            let keep = f.range.start >= end;
            if keep {
                end = f.range.end;
            }
            keep
        });
        Self { folds }
    }

    /// All folds.
    pub fn folds(&self) -> &[ValueFold] {
        &self.folds
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.folds.is_empty()
    }

    /// The folds that lie in `range` (a line), except those a caret or a selection in `carets`
    /// touches (char ranges, an empty one for a caret): they are shown as written.
    pub fn shown_within(&self, range: Range<usize>, carets: &[Range<usize>]) -> Vec<&ValueFold> {
        self.folds
            .iter()
            .filter(|f| f.range.start >= range.start && f.range.end <= range.end)
            .filter(|f| {
                !carets
                    .iter()
                    .any(|c| c.start <= f.range.end && c.end >= f.range.start)
            })
            .collect()
    }

    /// Moves the folds after a change of `deleted` chars at `offset` replaced by `inserted` chars;
    /// a fold the change touches is dropped until the model is read again.
    pub fn edited(&mut self, offset: usize, deleted: usize, inserted: usize) {
        let end = offset + deleted;
        self.folds.retain_mut(|f| {
            if f.range.end < offset {
                true
            } else if f.range.start > end {
                f.range = f.range.start - deleted + inserted..f.range.end - deleted + inserted;
                true
            } else {
                false
            }
        });
    }

    /// Forgets every fold.
    pub fn clear(&mut self) {
        self.folds.clear();
    }
}

/// `display` with the columns of each of `folds` (column range in the line, text) replaced by its
/// text: the first column of a fold maps to the start of the text, its other columns to the end.
/// Returns the new line and, for each fold drawn, its first column and the byte length of its
/// text. Folds must be sorted and must not overlap; one that reaches past the line is left out.
pub fn with_folds(
    display: &DisplayLine,
    folds: &[(Range<usize>, &str)],
) -> (DisplayLine, Vec<(usize, usize)>) {
    let columns = display.starts.len();
    let fits = |r: &Range<usize>| r.start < r.end && r.end < columns;
    if !folds.iter().any(|(r, _)| fits(r)) {
        return (display.clone(), Vec::new());
    }
    let mut text = String::with_capacity(display.text.len());
    let mut starts = Vec::with_capacity(columns);
    let mut drawn = Vec::new();
    for column in 0..columns {
        starts.push(text.len());
        match folds
            .iter()
            .find(|(r, _)| fits(r) && r.start <= column && column < r.end)
        {
            Some((r, shown)) => {
                if column == r.start {
                    drawn.push((column, shown.len()));
                    text.push_str(shown);
                }
            }
            None => {
                if column + 1 < columns {
                    text.push_str(
                        &display.text[display.starts[column]..display.starts[column + 1]],
                    );
                }
            }
        }
    }
    (DisplayLine { text, starts }, drawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(range: Range<usize>, text: &str) -> ValueFold {
        ValueFold {
            range,
            text: text.into(),
        }
    }

    /// SPEC-0115 AK-13: the value stands in place of the placeholder; the columns before and
    /// after it keep their text, and every column of the placeholder maps into the value.
    #[test]
    fn the_value_replaces_the_placeholder_in_the_display() {
        let line = "\t@Value(\"${shop.name}\") String name;";
        let display = DisplayLine::with_tab_width(line, 4);
        let from = line.chars().position(|c| c == '$').unwrap();
        let to = line.chars().position(|c| c == '}').unwrap() + 1;
        let (shown, drawn) = with_folds(&display, &[(from..to, "Corner Shop")]);
        assert_eq!(shown.text, "    @Value(\"Corner Shop\") String name;");
        assert_eq!(shown.starts.len(), display.starts.len());
        let at = shown.text.find("Corner").unwrap();
        assert_eq!(drawn, [(from, "Corner Shop".len())]);
        assert_eq!(shown.byte_of(from), at);
        assert_eq!(shown.byte_of(from + 1), at + "Corner Shop".len());
        assert_eq!(shown.byte_of(to), at + "Corner Shop".len());
        assert_eq!(&shown.text[shown.byte_of(to)..], "\") String name;");
        // Two placeholders in one string.
        let line = "x(\"${a} and ${b}\")";
        let display = DisplayLine::with_tab_width(line, 4);
        let (shown, drawn) = with_folds(&display, &[(3..7, "1"), (12..16, "•••")]);
        assert_eq!(shown.text, "x(\"1 and •••\")");
        assert_eq!(drawn.len(), 2);
        // A fold past the line end is left out.
        let (same, drawn) = with_folds(&display, &[(12..40, "1")]);
        assert_eq!(same, display);
        assert!(drawn.is_empty());
    }

    /// SPEC-0115 AK-13: a caret or a selection that touches a placeholder shows it as written;
    /// an edit moves the placeholders after it and drops the one it touches.
    #[test]
    fn carets_open_a_placeholder_and_edits_move_it() {
        let mut folds = ValueFolds::new(vec![
            fold(30..40, "b"),
            fold(10..20, "a"),
            fold(15..18, "inside"),
            fold(50..50, "empty"),
        ]);
        assert_eq!(folds.folds(), [fold(10..20, "a"), fold(30..40, "b")]);
        let texts = |f: &ValueFolds, carets: &[Range<usize>]| -> Vec<String> {
            f.shown_within(0..45, carets)
                .iter()
                .map(|f| f.text.clone())
                .collect()
        };
        let caret = |r: Range<usize>| std::iter::once(r).collect::<Vec<_>>();
        assert_eq!(texts(&folds, &[]), ["a", "b"]);
        // A caret at either end or inside opens it; one elsewhere does not.
        assert_eq!(texts(&folds, &caret(10..10)), ["b"]);
        assert_eq!(texts(&folds, &caret(20..20)), ["b"]);
        assert_eq!(texts(&folds, &[9..9, 21..29]), ["a", "b"]);
        assert_eq!(texts(&folds, &caret(5..35)), [""; 0]);
        // Only folds of the line.
        assert_eq!(folds.shown_within(0..25, &[]).len(), 1);
        // Three chars typed before both; then one deleted inside the second.
        folds.edited(2, 0, 3);
        assert_eq!(folds.folds(), [fold(13..23, "a"), fold(33..43, "b")]);
        folds.edited(35, 1, 0);
        assert_eq!(folds.folds(), [fold(13..23, "a")]);
        folds.clear();
        assert!(folds.is_empty());
    }
}
