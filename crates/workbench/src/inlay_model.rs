//! Parameter name hints of an editor (`SPEC-0291`), free of the UI toolkit: hints at char offsets,
//! moved by edits until the next answer, and inserted into a line's display text.

use std::ops::Range;

use crate::layout::DisplayLine;

/// A hint as the editor draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// Char offset in the document the hint stands before.
    pub offset: usize,
    /// Text drawn, padding included.
    pub text: String,
    /// Bytes of `text` drawn in the hint colours (the label without its padding).
    pub label: Range<usize>,
}

/// The hints an editor draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    /// Sorted by offset.
    hints: Vec<Hint>,
}

impl Hints {
    /// Hints from the server's parameter hints; `line_start` gives the char offset of a line.
    pub fn from_hints(
        hints: &[idecraft_languages::inlay::ParameterHint],
        line_start: impl Fn(u64) -> Option<usize>,
    ) -> Self {
        let mut out: Vec<Hint> = hints
            .iter()
            .filter_map(|h| {
                let offset = line_start(h.line)? + h.character as usize;
                let left = if h.padding_left { " " } else { "" };
                let right = if h.padding_right { " " } else { "" };
                Some(Hint {
                    offset,
                    text: format!("{left}{}{right}", h.label),
                    label: left.len()..left.len() + h.label.len(),
                })
            })
            .collect();
        out.sort_by_key(|h| h.offset);
        Self { hints: out }
    }

    /// The hints.
    pub fn hints(&self) -> &[Hint] {
        &self.hints
    }

    /// Hints at offsets in `range` (a line: its start up to and including its end).
    pub fn within(&self, range: Range<usize>) -> &[Hint] {
        let first = self.hints.partition_point(|h| h.offset < range.start);
        let last = self.hints.partition_point(|h| h.offset <= range.end);
        &self.hints[first..last.max(first)]
    }

    /// Moves the hints after a change of `deleted` chars at `offset` replaced by `inserted` chars;
    /// a hint at or inside the changed range is dropped (behaviour 5).
    pub fn edited(&mut self, offset: usize, deleted: usize, inserted: usize) {
        let end = offset + deleted;
        self.hints.retain_mut(|h| {
            if h.offset < offset {
                true
            } else if h.offset > end {
                h.offset = h.offset - deleted + inserted;
                true
            } else {
                false
            }
        });
    }

    /// Forgets every hint.
    pub fn clear(&mut self) {
        self.hints.clear();
    }
}

/// `display` with `hints` (column in the line, text) inserted before their columns (behaviour 4):
/// every column maps to the display offset after the hints before it. Returns the new line and the
/// display byte ranges of the hints' labels.
pub fn with_hints(
    display: &DisplayLine,
    hints: &[(usize, &Hint)],
) -> (DisplayLine, Vec<Range<usize>>) {
    if hints.is_empty() {
        return (display.clone(), Vec::new());
    }
    let mut text = String::with_capacity(display.text.len() + 16 * hints.len());
    let mut starts = Vec::with_capacity(display.starts.len());
    let mut labels = Vec::with_capacity(hints.len());
    let mut next = hints.iter().peekable();
    let columns = display.starts.len();
    for column in 0..columns {
        while let Some((_, h)) = next.next_if(|(c, _)| *c <= column) {
            labels.push(text.len() + h.label.start..text.len() + h.label.end);
            text.push_str(&h.text);
        }
        starts.push(text.len());
        if column + 1 < columns {
            text.push_str(&display.text[display.starts[column]..display.starts[column + 1]]);
        }
    }
    (DisplayLine { text, starts }, labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_languages::inlay::ParameterHint;

    fn hint(line: u64, character: u64, label: &str) -> ParameterHint {
        ParameterHint {
            line,
            character,
            label: label.into(),
            padding_left: false,
            padding_right: true,
            is_type: false,
            is_value: false,
        }
    }

    /// SPEC-0291 AK-3: two hints on a line are inserted before their arguments; columns map to the
    /// shifted offsets and back.
    #[test]
    fn hints_shift_the_display_line() {
        let line = "\tadd(1, 2);";
        let display = DisplayLine::with_tab_width(line, 4);
        let hints = Hints::from_hints(&[hint(0, 8, "right:"), hint(0, 5, "left:")], |l| {
            (l == 0).then_some(0)
        });
        let on_line: Vec<(usize, &Hint)> =
            hints.within(0..11).iter().map(|h| (h.offset, h)).collect();
        let (shown, labels) = with_hints(&display, &on_line);
        assert_eq!(shown.text, "    add(left: 1, right: 2);");
        assert_eq!(
            labels
                .iter()
                .map(|r| &shown.text[r.clone()])
                .collect::<Vec<_>>(),
            ["left:", "right:"]
        );
        // Column 5 is `1`, after the first hint; column 4 is `(`, before it.
        assert_eq!(&shown.text[shown.byte_of(5)..shown.byte_of(6)], "1");
        assert!(shown.text[shown.byte_of(4)..].starts_with("(left: 1"));
        assert_eq!(&shown.text[shown.byte_of(8)..shown.byte_of(9)], "2");
        assert_eq!(shown.byte_of(11), shown.text.len());
        for column in 0..=11 {
            assert_eq!(shown.column_of(shown.byte_of(column)), column);
        }
        // Without hints the line stays as it was.
        assert_eq!(with_hints(&display, &[]).0, display);
    }

    /// SPEC-0291 AK-2e (v7): an enum member's value and a field's type stand behind their names
    /// with the space the server asked for; the text itself keeps its columns.
    #[test]
    fn a_value_hint_stands_behind_its_name() {
        let line = "enum Color { Red, Green }";
        let display = DisplayLine::new(line);
        let value = |character: u64, label: &str| ParameterHint {
            padding_left: true,
            padding_right: false,
            is_value: true,
            ..hint(0, character, label)
        };
        let hints = Hints::from_hints(&[value(16, "= 0"), value(23, "= 1")], |l| {
            (l == 0).then_some(0)
        });
        let on_line: Vec<(usize, &Hint)> = hints
            .within(0..line.len())
            .iter()
            .map(|h| (h.offset, h))
            .collect();
        let (shown, labels) = with_hints(&display, &on_line);
        assert_eq!(shown.text, "enum Color { Red = 0, Green = 1 }");
        assert_eq!(
            labels
                .iter()
                .map(|r| &shown.text[r.clone()])
                .collect::<Vec<_>>(),
            ["= 0", "= 1"]
        );
        for column in 0..=line.len() {
            assert_eq!(shown.column_of(shown.byte_of(column)), column);
        }

        let field = "    count = 0;";
        let hints = Hints::from_hints(
            &[ParameterHint {
                padding_left: true,
                padding_right: false,
                is_type: true,
                ..hint(0, 9, ": number")
            }],
            |l| (l == 0).then_some(0),
        );
        let on_line: Vec<(usize, &Hint)> = hints
            .within(0..field.len())
            .iter()
            .map(|h| (h.offset, h))
            .collect();
        let (shown, _) = with_hints(&DisplayLine::new(field), &on_line);
        assert_eq!(shown.text, "    count : number = 0;");
    }

    /// SPEC-0291 AK-3: a hint at the end of the line is drawn after the last character.
    #[test]
    fn a_hint_at_the_line_end() {
        let display = DisplayLine::new("f(");
        let h = Hint {
            offset: 2,
            text: "x:".into(),
            label: 0..2,
        };
        let (shown, labels) = with_hints(&display, &[(2, &h)]);
        assert_eq!(shown.text, "f(x:");
        assert_eq!(labels, vec![2..4]);
        assert_eq!(shown.byte_of(2), 4);
    }

    /// SPEC-0291 AK-3: edits move hints after them and drop hints they touch.
    #[test]
    fn edits_move_or_drop_hints() {
        let mut h = Hints::from_hints(
            &[hint(0, 2, "a:"), hint(0, 8, "b:"), hint(0, 12, "c:")],
            |_| Some(0),
        );
        h.edited(0, 0, 2);
        assert_eq!(
            h.hints().iter().map(|h| h.offset).collect::<Vec<_>>(),
            [4, 10, 14]
        );
        h.edited(10, 0, 1); // typing at the second hint's argument
        h.edited(5, 2, 0); // deleting after the first
        assert_eq!(
            h.hints().iter().map(|h| h.offset).collect::<Vec<_>>(),
            [4, 13]
        );
        assert_eq!(h.within(4..4).len(), 1);
        assert_eq!(h.within(5..20).len(), 1);
    }
}
