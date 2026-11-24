//! Toolkit-free parts of the editor view (`SPEC-0058`): tab expansion, gutter width and scrolling
//! arithmetic. Kept apart from GPUI so they are unit-tested on every
//! platform.

/// Columns a tab advances to (multiples of this).
pub const TAB_WIDTH: usize = 4;

/// A line prepared for display: tabs expanded to spaces, with the byte offset in the display text of
/// every char boundary of the original line (`starts.len()` = chars + 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLine {
    /// Text as drawn.
    pub text: String,
    /// Display byte offset of each original char boundary.
    pub starts: Vec<usize>,
}

impl DisplayLine {
    /// Expands tabs to the next multiple of [`TAB_WIDTH`] (behaviour 2).
    pub fn new(line: &str) -> Self {
        Self::with_tab_width(line, TAB_WIDTH)
    }

    /// Expands tabs to the next multiple of `width` columns (EditorConfig `tab_width`, SPEC-0137).
    pub fn with_tab_width(line: &str, width: usize) -> Self {
        let width = width.max(1);
        let mut text = String::with_capacity(line.len());
        let mut starts = Vec::with_capacity(line.len() + 1);
        let mut column = 0;
        for ch in line.chars() {
            starts.push(text.len());
            if ch == '\t' {
                let n = width - column % width;
                text.extend(std::iter::repeat_n(' ', n));
                column += n;
            } else {
                text.push(ch);
                column += 1;
            }
        }
        starts.push(text.len());
        Self { text, starts }
    }

    /// Display byte offset of char column `column` (clamped to the line end).
    pub fn byte_of(&self, column: usize) -> usize {
        self.starts[column.min(self.starts.len() - 1)]
    }

    /// Char column whose display offset is closest at or before display byte `byte`.
    pub fn column_of(&self, byte: usize) -> usize {
        match self.starts.binary_search(&byte) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }
}

/// Width in pixels of a caret (`SPEC-0057` v6 behaviour 25): a thin bar; in overwrite mode
/// (`block`) the width of the character it stands on, `next`, or one character cell at a line
/// end, where nothing follows.
pub fn caret_width(block: bool, next: f32, cell: f32) -> f32 {
    match block {
        false => 2.0,
        true if next > 0.0 => next,
        true => cell.max(2.0),
    }
}

/// Digits shown in the gutter: at least 3, enough for the last line number (behaviour 1).
pub fn gutter_digits(lines: usize) -> usize {
    lines.max(1).to_string().len().max(3)
}

/// First line and number of lines to draw for a scroll offset (in pixels) and viewport height.
pub fn visible_lines(scroll_y: f32, height: f32, line_height: f32, total: usize) -> (usize, usize) {
    let first = ((scroll_y / line_height).floor().max(0.0) as usize).min(total.saturating_sub(1));
    let count = ((height / line_height).ceil() as usize + 1).min(total - first);
    (first, count)
}

/// Scroll offset that keeps `line` fully visible, changing `scroll_y` as little as possible
/// (behaviour 6).
pub fn reveal(scroll_y: f32, height: f32, line_height: f32, line: usize) -> f32 {
    let top = line as f32 * line_height;
    let bottom = top + line_height;
    if top < scroll_y {
        top
    } else if bottom > scroll_y + height {
        (bottom - height).max(0.0)
    } else {
        scroll_y
    }
}

/// Scroll offset that puts row `row` in the vertical middle of a view `height` high, within the
/// scroll limits of `total` rows (`SPEC-0057` v4 behaviour 22).
pub fn centred(height: f32, line_height: f32, row: usize, total: usize) -> f32 {
    (row as f32 * line_height - (height - line_height) / 2.0)
        .clamp(0.0, max_scroll(total, line_height))
}

/// Largest scroll offset: the last line may scroll up to the top of the view.
pub fn max_scroll(total: usize, line_height: f32) -> f32 {
    total.saturating_sub(1) as f32 * line_height
}

/// A change marker in the gutter: kind, first painted row (0 = first visible line, and
///  number of rows; a deletion has 0 rows and sits on the boundary above `row`
/// (`SPEC-0064` behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkerSpan {
    /// Added, modified or deleted lines.
    pub kind: ferris_idea_vcs::LineChangeKind,
    /// Row relative to the first visible line.
    pub row: usize,
    /// Rows covered; 0 for a deletion.
    pub rows: usize,
}

/// Markers of `changes` that fall into the visible lines `first..first + count` (0-based), clipped.
pub fn marker_spans(
    changes: &[ferris_idea_vcs::LineChange],
    first: usize,
    count: usize,
) -> Vec<MarkerSpan> {
    let end = first + count;
    changes
        .iter()
        .filter_map(|c| {
            // 0-based half-open range of new lines; a deletion is the boundary before `new_start - 1`.
            let (from, to) = (c.new_start - 1, c.new_end);
            if c.kind == ferris_idea_vcs::LineChangeKind::Deleted {
                return (from >= first && from <= end).then(|| MarkerSpan {
                    kind: c.kind,
                    row: from - first,
                    rows: 0,
                });
            }
            let (a, b) = (from.max(first), to.min(end));
            (a < b).then(|| MarkerSpan {
                kind: c.kind,
                row: a - first,
                rows: b - a,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_expand_to_stops_and_map_back() {
        let d = DisplayLine::new("\tab\tc");
        assert_eq!(d.text, "    ab  c");
        assert_eq!(d.starts, [0, 4, 5, 6, 8, 9]);
        assert_eq!(d.byte_of(3), 6);
        assert_eq!(d.byte_of(99), 9);
        assert_eq!(d.column_of(7), 3);
        assert_eq!(d.column_of(8), 4);
        let u = DisplayLine::new("äb");
        assert_eq!(u.starts, [0, 2, 3]);
    }

    #[test]
    fn gutter_width() {
        assert_eq!(gutter_digits(7), 3);
        assert_eq!(gutter_digits(100_000), 6);
    }

    #[test]
    fn scrolling() {
        assert_eq!(visible_lines(0.0, 100.0, 20.0, 1000), (0, 6));
        assert_eq!(visible_lines(410.0, 100.0, 20.0, 1000), (20, 6));
        assert_eq!(visible_lines(0.0, 100.0, 20.0, 3), (0, 3));
        assert_eq!(reveal(0.0, 100.0, 20.0, 2), 0.0);
        assert_eq!(reveal(0.0, 100.0, 20.0, 10), 120.0);
        assert_eq!(reveal(300.0, 100.0, 20.0, 5), 100.0);
        assert_eq!(max_scroll(10, 20.0), 180.0);
        // SPEC-0057 v4 AK-17: row 50 in the middle of five rows, the limits at both ends.
        assert_eq!(centred(100.0, 20.0, 50, 1000), 960.0);
        assert_eq!(centred(100.0, 20.0, 1, 1000), 0.0);
        assert_eq!(centred(100.0, 20.0, 9, 10), 140.0);
        assert_eq!(centred(100.0, 20.0, 0, 1), 0.0);
    }

    #[test]
    fn markers_are_clipped_to_the_visible_lines() {
        use ferris_idea_vcs::{LineChange, LineChangeKind::*};
        let ch = |kind, new_start, new_end| LineChange {
            kind,
            new_start,
            new_end,
            old_start: 1,
            old_end: 1,
        };
        let changes = [
            ch(Added, 2, 3),
            ch(Modified, 9, 12),
            ch(Deleted, 6, 5),
            ch(Added, 30, 31),
        ];
        let spans = marker_spans(&changes, 4, 6);
        assert_eq!(
            spans,
            [
                MarkerSpan {
                    kind: Modified,
                    row: 4,
                    rows: 2
                },
                MarkerSpan {
                    kind: Deleted,
                    row: 1,
                    rows: 0
                },
            ]
        );
        assert_eq!(
            marker_spans(&changes, 0, 3)[0],
            MarkerSpan {
                kind: Added,
                row: 1,
                rows: 2
            }
        );
    }

    /// SPEC-0057 v6 behaviour 25: a bar in insert mode, a block in overwrite mode.
    #[test]
    fn caret_widths() {
        assert_eq!(caret_width(false, 7.8, 7.8), 2.0);
        assert_eq!(caret_width(true, 7.8, 7.8), 7.8);
        // A wide character under the caret, and the line end.
        assert_eq!(caret_width(true, 15.6, 7.8), 15.6);
        assert_eq!(caret_width(true, 0.0, 7.8), 7.8);
        assert_eq!(caret_width(true, 0.0, 0.0), 2.0);
    }
}
