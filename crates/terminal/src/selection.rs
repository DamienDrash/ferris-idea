//! Text selection in the terminal (`SPEC-0118` behaviour 4): by character, word or line over the
//! whole history (scrollback and screen), copied as plain text.

use crate::screen::{Cell, Screen};

/// What a drag extends by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionUnit {
    /// Single click: characters.
    Char,
    /// Double click: words.
    Word,
    /// Triple click: whole lines.
    Line,
}

/// A position: line of the history ([`Screen::line`]) and column.
pub type Point = (usize, usize);

/// A selection from where the button went down to where the mouse is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Where it started.
    pub anchor: Point,
    /// Where it ends now.
    pub head: Point,
    /// What it extends by.
    pub unit: SelectionUnit,
}

/// Characters that belong to a word for double clicks: letters, digits and those common in paths,
/// URLs and options.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || "_-./~:@%+=#?&".contains(c)
}

fn ch(line: &[Cell], col: usize) -> char {
    line.get(col).map_or(' ', |c| c.ch)
}

impl Selection {
    /// A selection starting and ending at `at`.
    pub fn new(at: Point, unit: SelectionUnit) -> Self {
        Self {
            anchor: at,
            head: at,
            unit,
        }
    }

    /// Start and end (inclusive) in reading order, widened to words or lines.
    pub fn range(&self, screen: &Screen) -> (Point, Point) {
        let (mut start, mut end) = if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        };
        match self.unit {
            SelectionUnit::Char => {}
            SelectionUnit::Line => {
                start.1 = 0;
                end.1 = usize::MAX;
            }
            SelectionUnit::Word => {
                if let Some(line) = screen.line(start.0)
                    && word_char(ch(line, start.1))
                {
                    while start.1 > 0 && word_char(ch(line, start.1 - 1)) {
                        start.1 -= 1;
                    }
                }
                if let Some(line) = screen.line(end.0)
                    && word_char(ch(line, end.1))
                {
                    while end.1 + 1 < line.len() && word_char(ch(line, end.1 + 1)) {
                        end.1 += 1;
                    }
                }
            }
        }
        (start, end)
    }

    /// Columns of history line `line` that are selected (inclusive), if any.
    pub fn columns(&self, screen: &Screen, line: usize) -> Option<(usize, usize)> {
        let (start, end) = self.range(screen);
        if line < start.0 || line > end.0 {
            return None;
        }
        let from = if line == start.0 { start.1 } else { 0 };
        let to = if line == end.0 { end.1 } else { usize::MAX };
        Some((from, to))
    }

    /// Whether the selection covers nothing (a click without a drag).
    pub fn is_empty(&self) -> bool {
        self.unit == SelectionUnit::Char && self.anchor == self.head
    }

    /// The selected text: trailing blanks of each line dropped, lines joined with `\n`.
    pub fn text(&self, screen: &Screen) -> String {
        let (start, end) = self.range(screen);
        let mut lines = Vec::new();
        for index in start.0..=end.0 {
            let Some(line) = screen.line(index) else {
                break;
            };
            let from = if index == start.0 { start.1 } else { 0 };
            let to = if index == end.0 { end.1 } else { usize::MAX };
            let mut text = String::new();
            for (col, cell) in line.iter().enumerate() {
                if col < from || col > to || cell.width == 0 {
                    continue;
                }
                text.push(cell.ch);
                text.push_str(&cell.combining);
            }
            lines.push(text.trim_end().to_owned());
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Terminal;

    fn term(input: &str) -> Terminal {
        let mut t = Terminal::new(3, 20);
        t.feed(input.as_bytes());
        t
    }

    #[test]
    fn characters_words_and_lines() {
        let t = term("one\r\nls -la ./src/main.rs\r\nthree");
        let s = &t.screen;
        let chars = Selection {
            anchor: (1, 7),
            head: (1, 0),
            unit: SelectionUnit::Char,
        };
        assert_eq!(chars.text(s), "ls -la .");
        let word = Selection::new((1, 12), SelectionUnit::Word);
        assert_eq!(word.text(s), "./src/main.rs");
        assert_eq!(word.columns(s, 1), Some((7, 19)));
        assert_eq!(word.columns(s, 0), None);
        let lines = Selection {
            anchor: (2, 1),
            head: (0, 2),
            unit: SelectionUnit::Line,
        };
        assert_eq!(lines.text(s), "one\nls -la ./src/main.rs\nthree");
        assert!(Selection::new((0, 0), SelectionUnit::Char).is_empty());
        assert!(!Selection::new((0, 3), SelectionUnit::Word).is_empty());
    }

    #[test]
    fn selections_reach_into_the_scrollback() {
        let t = term("a1\r\na2\r\na3\r\na4\r\na5");
        let s = &t.screen;
        assert_eq!(s.scrollback_len(), 2);
        assert_eq!(s.line(0).map(|l| l[1].ch), Some('1'));
        assert_eq!(s.line(4).map(|l| l[1].ch), Some('5'));
        assert!(s.line(5).is_none());
        let across = Selection {
            anchor: (1, 0),
            head: (3, 1),
            unit: SelectionUnit::Char,
        };
        assert_eq!(across.text(s), "a2\na3\na4");
    }
}
