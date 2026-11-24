//! What the gutter marks of declarations say (`SPEC-0320` behaviours 8 and 9).

use ferris_idea_languages::ts_structure::{DeclarationMark, MarkKind, MarkTarget};

/// The mark of one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMark {
    /// 0-based line.
    pub line: usize,
    /// `↑`, `↓` or `↕`.
    pub glyph: &'static str,
    /// What it means, a line per kind.
    pub tip: String,
    /// Where a click leads.
    pub targets: Vec<MarkTarget>,
}

/// The glyph of a bean's mark (`SPEC-0115` v5 behaviour 14).
pub const BEAN: &str = "●";
/// The glyph of an injection point's mark.
pub const INJECTION: &str = "→";
/// The glyph of a line that is both: a producer method with parameters.
pub const BOTH: &str = "◉";

/// The marks of a file, by line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclMarks {
    marks: Vec<LineMark>,
}

fn phrase(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::Overrides => "Overrides in",
        MarkKind::Implements => "Implements",
        MarkKind::Overridden => "Overridden in",
        MarkKind::Implemented => "Implemented in",
        MarkKind::Inherited => "Has inheritors:",
    }
}

impl DeclMarks {
    /// The marks of `found`, those on one line merged.
    pub fn from_marks(found: &[DeclarationMark]) -> Self {
        let mut marks: Vec<LineMark> = Vec::new();
        for mark in found {
            let line = mark.line as usize;
            let names: Vec<&str> = mark.targets.iter().map(|t| t.name.as_str()).collect();
            let text = format!("{} {}", phrase(mark.kind), names.join(", "));
            let glyph = if mark.kind.up() { "↑" } else { "↓" };
            match marks.iter_mut().find(|m| m.line == line) {
                Some(m) => {
                    if m.glyph != glyph {
                        m.glyph = "↕";
                    }
                    m.tip.push('\n');
                    m.tip.push_str(&text);
                    for t in &mark.targets {
                        if !m.targets.contains(t) {
                            m.targets.push(t.clone());
                        }
                    }
                }
                None => marks.push(LineMark {
                    line,
                    glyph,
                    tip: text,
                    targets: mark.targets.clone(),
                }),
            }
        }
        marks.sort_by_key(|m| m.line);
        Self { marks }
    }

    /// The marks `lines`, those on one line merged into one that carries both tips and all
    /// targets (`SPEC-0115` v5 behaviour 14: a producer method is a bean and has injection
    /// points); `both` is the glyph of a merged mark.
    pub fn from_lines(lines: Vec<LineMark>) -> Self {
        let mut marks: Vec<LineMark> = Vec::new();
        for mark in lines {
            match marks.iter_mut().find(|m| m.line == mark.line) {
                Some(m) => {
                    if m.glyph != mark.glyph {
                        m.glyph = BOTH;
                    }
                    m.tip.push('\n');
                    m.tip.push_str(&mark.tip);
                    for t in mark.targets {
                        if !m.targets.contains(&t) {
                            m.targets.push(t);
                        }
                    }
                }
                None => marks.push(mark),
            }
        }
        marks.sort_by_key(|m| m.line);
        Self { marks }
    }

    /// Whether there is no mark.
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The mark of `line`.
    pub fn at(&self, line: usize) -> Option<&LineMark> {
        self.marks
            .binary_search_by_key(&line, |m| m.line)
            .ok()
            .map(|i| &self.marks[i])
    }

    /// Follows an edit from `line` that removed and added line breaks: marks below move, a mark on
    /// a line the edit touched is dropped.
    pub fn edited(&mut self, line: usize, removed: usize, added: usize) {
        self.marks.retain_mut(|m| {
            if m.line < line {
                return true;
            }
            if m.line <= line + removed {
                return false;
            }
            m.line = m.line - removed + added;
            true
        });
    }

    /// Follows `added` whole lines put in before line `first` while no other line changed
    /// (`SPEC-0320` v4 behaviour 20): marks from `first` on move, the others stay — also the mark
    /// of the line the new lines were appended to.
    pub fn lines_inserted(&mut self, first: usize, added: usize) {
        for m in &mut self.marks {
            if m.line >= first {
                m.line += added;
            }
        }
    }
}

/// Whether an insertion of `inserted` at char `offset` only put whole lines into a text, and the
/// first line that moved down: `Some(line + 1)` for text starting with a line break appended to the
/// end of `line`, `Some(line)` for text ending with a line break put at the start of `line`.
/// `before` and `after` are the chars around the inserted text (`None` at the text's ends), `line`
/// the line of `offset`.
pub fn whole_lines_inserted(
    inserted: &str,
    line: usize,
    before: Option<char>,
    after: Option<char>,
) -> Option<usize> {
    if !inserted.contains('\n') {
        return None;
    }
    let at_line_end = after.is_none_or(|c| c == '\n' || c == '\r');
    let at_line_start = before.is_none_or(|c| c == '\n');
    if at_line_end && (inserted.starts_with('\n') || inserted.starts_with("\r\n")) {
        Some(line + 1)
    } else if at_line_start && inserted.ends_with('\n') {
        Some(line)
    } else {
        None
    }
}

/// What a replacement really adds once the text it deletes and the text it inserts are trimmed
/// of what they share at the start and at the end: the number of shared characters before, and the
/// inserted text left, when nothing that was there is gone (`SPEC-0083` v5 behaviour 5a). An edit
/// that replaces `"\n    "` by `"\n\n    @Override …}\n    "` only puts lines in.
pub fn net_insertion<'a>(deleted: &str, inserted: &'a str) -> Option<(usize, &'a str)> {
    let prefix: usize = deleted
        .chars()
        .zip(inserted.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let (rest_deleted, rest_inserted) = (&deleted[prefix..], &inserted[prefix..]);
    let suffix: usize = rest_deleted
        .chars()
        .rev()
        .zip(rest_inserted.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    if rest_deleted.len() != suffix {
        return None;
    }
    let added = &rest_inserted[..rest_inserted.len() - suffix];
    Some((deleted[..prefix].chars().count(), added))
}

#[cfg(test)]
mod tests {
    /// SPEC-0083 v5 AK-11: replacements that only put text in, and those that change text.
    #[test]
    fn replacements_that_only_put_lines_in() {
        // jdtls-like: the whitespace before the closing brace is replaced by a member and itself.
        let (before, added) =
            super::net_insertion("\n    ", "\n\n    @Override\n    void a() {}\n    ").unwrap();
        assert_eq!(before, 1);
        assert_eq!(added, "\n    @Override\n    void a() {}\n");
        // A pure insertion stays one.
        assert_eq!(super::net_insertion("", "\nx\n"), Some((0, "\nx\n")));
        // Text that goes is no insertion.
        assert_eq!(super::net_insertion("return 1;", "return 2;"), None);
        assert_eq!(super::net_insertion("ab", "a\nb\nc"), None);
        assert_eq!(super::net_insertion("x", ""), None);
        // Multi-byte characters are counted as characters.
        assert_eq!(super::net_insertion("é\n", "é\nü\n"), Some((2, "ü\n")));
    }

    use super::*;
    use ferris_idea_languages::insight::{Location, Range};
    use std::path::PathBuf;

    fn target(name: &str) -> MarkTarget {
        MarkTarget {
            name: name.into(),
            location: Location {
                path: PathBuf::from("/a.ts"),
                range: Range {
                    start_line: 0,
                    start_character: 0,
                    end_line: 0,
                    end_character: 1,
                },
            },
        }
    }

    fn mark(line: u64, kind: MarkKind, to: &str) -> DeclarationMark {
        DeclarationMark {
            line,
            character: 2,
            name: "m".into(),
            kind,
            targets: vec![target(to)],
        }
    }

    /// SPEC-0320 AK-9.
    #[test]
    fn glyphs_and_tips() {
        let m = DeclMarks::from_marks(&[
            mark(1, MarkKind::Implements, "Shape"),
            mark(1, MarkKind::Overridden, "Circle"),
            mark(2, MarkKind::Overrides, "Base"),
            mark(4, MarkKind::Inherited, "Square"),
        ]);
        let one = m.at(1).unwrap();
        assert_eq!(one.glyph, "↕");
        assert_eq!(one.tip, "Implements Shape\nOverridden in Circle");
        assert_eq!(one.targets.len(), 2);
        assert_eq!(m.at(2).unwrap().glyph, "↑");
        assert_eq!(m.at(4).unwrap().glyph, "↓");
        assert!(m.at(3).is_none());
    }

    /// SPEC-0320 AK-9.
    #[test]
    fn follows_edits() {
        let mut m = DeclMarks::from_marks(&[
            mark(1, MarkKind::Overrides, "A"),
            mark(5, MarkKind::Overrides, "B"),
            mark(9, MarkKind::Overrides, "C"),
        ]);
        m.edited(3, 0, 2);
        assert!(m.at(1).is_some() && m.at(7).is_some() && m.at(11).is_some());
        m.edited(7, 0, 0);
        assert!(m.at(7).is_none(), "an edit on its line drops the mark");
        m.edited(0, 1, 0);
        assert!(m.at(0).is_none() && m.at(1).is_none());
        assert!(m.at(10).is_some());
        assert!(!m.is_empty());
    }

    /// SPEC-0320 AK-12 (v4): a member appended after the line of an overriding member keeps that
    /// member's mark; the marks below move.
    #[test]
    fn whole_lines_put_in_keep_the_marks_beside_them() {
        let mut m = DeclMarks::from_marks(&[
            mark(3, MarkKind::Overrides, "Animal"),
            mark(6, MarkKind::Overrides, "Animal"),
        ]);
        // "\n    override val owner: String\n        get() = TODO()" at the end of line 3.
        let inserted = "\n    override val owner: String\n        get() = TODO()";
        let first = whole_lines_inserted(inserted, 3, Some('"'), Some('\n'));
        assert_eq!(first, Some(4));
        m.lines_inserted(4, 2);
        assert!(m.at(3).is_some(), "the mark of the line appended to stays");
        assert!(m.at(8).is_some() && m.at(6).is_none());
        // Lines put in at the start of a line move that line's mark.
        assert_eq!(
            whole_lines_inserted("// note\n", 3, Some('\n'), Some(' ')),
            Some(3)
        );
        assert_eq!(whole_lines_inserted("x\n", 0, None, Some('a')), Some(0));
        assert_eq!(whole_lines_inserted("\nlast", 9, Some('}'), None), Some(10));
        // In the middle of a line, or without a line break, the line itself changed.
        assert_eq!(whole_lines_inserted("\nfoo", 3, Some('a'), Some('b')), None);
        assert_eq!(whole_lines_inserted("foo", 3, Some('a'), Some('\n')), None);
        assert_eq!(
            whole_lines_inserted("foo\nbar", 3, Some('\n'), Some('\n')),
            None
        );
    }

    /// SPEC-0115 AK-14 (v5): a line that declares a bean and takes beans has one mark with both
    /// tips and all targets; the marks are found by line and follow edits like the others.
    #[test]
    fn marks_of_one_line_merge() {
        let line = |line: usize, glyph: &'static str, tip: &str, target: &str| LineMark {
            line,
            glyph,
            tip: tip.into(),
            targets: vec![MarkTarget {
                name: target.into(),
                location: Location {
                    path: "/p/A.java".into(),
                    range: ferris_idea_languages::insight::Range {
                        start_line: 1,
                        start_character: 0,
                        end_line: 1,
                        end_character: 0,
                    },
                },
            }],
        };
        let mut m = DeclMarks::from_lines(vec![
            line(8, BEAN, "Bean report", "Orders"),
            line(2, BEAN, "Bean config", "X"),
            line(8, INJECTION, "Injects Orders", "orders"),
            line(8, INJECTION, "Injects Clock", "orders"),
        ]);
        let both = m.at(8).unwrap();
        assert_eq!(both.glyph, BOTH);
        assert_eq!(both.tip, "Bean report\nInjects Orders\nInjects Clock");
        let names: Vec<&str> = both.targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Orders", "orders"]);
        assert_eq!(m.at(2).unwrap().glyph, BEAN);
        m.edited(4, 0, 1);
        assert!(m.at(9).is_some() && m.at(2).is_some() && m.at(8).is_none());
    }
}
