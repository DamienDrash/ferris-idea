//! Finding text in a terminal's history (`SPEC-0206` behaviour 1).

use crate::screen::Screen;

/// One occurrence: history line (scrollback first, then the screen) and its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// History line, as [`Screen::line`] counts.
    pub line: usize,
    /// First column.
    pub start: usize,
    /// Column after the last one.
    pub end: usize,
}

/// Every occurrence of `query` in history and screen, top to bottom; a match does not span lines.
/// Without `match_case` letters compare by their lower case.
pub fn search(screen: &Screen, query: &str, match_case: bool) -> Vec<Match> {
    let fold = |c: char| {
        if match_case {
            c
        } else {
            c.to_lowercase().next().unwrap_or(c)
        }
    };
    let needle: Vec<char> = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in 0..screen.history_len() {
        let Some(cells) = screen.line(line) else {
            continue;
        };
        // Characters of the line with the column each starts in.
        let mut chars = Vec::with_capacity(cells.len());
        for (col, cell) in cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            chars.push((fold(cell.ch), col, col + usize::from(cell.width.max(1))));
            for m in cell.combining.chars() {
                chars.push((fold(m), col, col + usize::from(cell.width.max(1))));
            }
        }
        let mut i = 0;
        while i + needle.len() <= chars.len() {
            if chars[i..i + needle.len()]
                .iter()
                .zip(&needle)
                .all(|((c, _, _), n)| c == n)
            {
                out.push(Match {
                    line,
                    start: chars[i].1,
                    end: chars[i + needle.len() - 1].2,
                });
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Terminal;

    #[test]
    fn finds_in_history_and_screen_with_and_without_case() {
        let mut t = Terminal::new(2, 12);
        t.feed("Error one\r\nok\r\nerror 世界 error\r\nlast ERROR".as_bytes());
        let found = search(&t.screen, "error", false);
        assert_eq!(
            found,
            [
                Match {
                    line: 0,
                    start: 0,
                    end: 5
                },
                Match {
                    line: 2,
                    start: 0,
                    end: 5
                },
                Match {
                    line: 4,
                    start: 5,
                    end: 10
                },
            ],
            "the wrapped `error` after the wide characters is cut by the line end"
        );
        let cased = search(&t.screen, "ERROR", true);
        assert_eq!(
            cased,
            [Match {
                line: 4,
                start: 5,
                end: 10
            }]
        );
        // Columns count wide characters twice.
        assert_eq!(
            search(&t.screen, "界", false),
            [Match {
                line: 2,
                start: 8,
                end: 10
            }]
        );
        assert!(search(&t.screen, "", false).is_empty());
        assert!(search(&t.screen, "one\nok", false).is_empty());
    }
}
