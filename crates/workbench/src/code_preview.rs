//! Code excerpts with syntax colours for previews: the Search Everywhere preview (`SPEC-0067` v5
//! behaviour 12), the Find in Files preview (`SPEC-0066` v3 behaviour 9) and the snippets of Recent
//! Locations (`SPEC-0069` v2 behaviour 8).

use std::ops::Range;
use std::path::Path;

use ferris_idea_languages::LanguageId;
use ferris_idea_languages::syntax::SyntaxDocument;
use gpui::{ParentElement, Styled, div, px};

use crate::palette::Palette;

/// Largest text coloured; larger files are shown plain.
const MAX_COLOURED: usize = 512 * 1024;

/// One line of an excerpt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcerptLine {
    /// 1-based line number.
    pub number: usize,
    /// The line's text.
    pub text: String,
    /// Whether this is the line the excerpt is about.
    pub current: bool,
    /// Syntax spans as char ranges of the line and their capture names.
    pub spans: Vec<(Range<usize>, String)>,
    /// The marked match (char range of the line), on the current line.
    pub mark: Option<Range<usize>>,
}

/// The lines `line - before ..= line + after` of `text` (the content of `path`), coloured by the
/// file's grammar, with `mark` (char range of the line) on `line`.
pub fn excerpt(
    path: &Path,
    text: &str,
    line: usize,
    mark: Option<Range<usize>>,
    before: usize,
    after: usize,
) -> Vec<ExcerptLine> {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let line = line.min(lines.len() - 1);
    let first = line.saturating_sub(before);
    let last = (line + after).min(lines.len() - 1);
    // Char offset of each line's start.
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for l in &lines {
        starts.push(at);
        at += l.chars().count() + 1;
    }
    let range = starts[first]..starts[last] + lines[last].chars().count();
    let spans = (text.len() <= MAX_COLOURED)
        .then(|| {
            let language = LanguageId::detect(path)?;
            let tsx = path.extension().is_some_and(|e| e == "tsx");
            let doc = SyntaxDocument::new(language, tsx, text).ok()?;
            Some(doc.highlight_range(range))
        })
        .flatten()
        .unwrap_or_default();
    (first..=last)
        .map(|i| {
            let t = lines[i].trim_end_matches('\r').to_owned();
            let n = t.chars().count();
            let (s, e) = (starts[i], starts[i] + n);
            let own = spans
                .iter()
                .filter(|sp| sp.start < e && sp.end > s)
                .map(|sp| (sp.start.max(s) - s..sp.end.min(e) - s, sp.kind.clone()))
                .collect();
            ExcerptLine {
                number: i + 1,
                current: i == line,
                mark: mark
                    .clone()
                    .filter(|_| i == line)
                    .map(|m| m.start.min(n)..m.end.min(n)),
                text: t,
                spans: own,
            }
        })
        .collect()
}

/// A line's text with its syntax colours and the mark's background.
pub fn styled_line(l: &ExcerptLine) -> gpui::StyledText {
    let p = Palette::current();
    let chars: Vec<(usize, char)> = l.text.char_indices().collect();
    let byte = |c: usize| chars.get(c).map_or(l.text.len(), |(b, _)| *b);
    // Later spans win where they overlap: keep the innermost capture per char.
    let mut colour: Vec<Option<u32>> = vec![None; chars.len()];
    for (r, kind) in &l.spans {
        let fg = p.capture(kind).foreground;
        for c in colour.iter_mut().take(r.end).skip(r.start) {
            if fg.is_some() {
                *c = fg;
            }
        }
    }
    let mut highlights: Vec<(Range<usize>, gpui::HighlightStyle)> = Vec::new();
    let mut i = 0;
    while i < colour.len() {
        let j = (i..colour.len())
            .find(|&j| colour[j] != colour[i])
            .unwrap_or(colour.len());
        if let Some(fg) = colour[i] {
            highlights.push((
                byte(i)..byte(j),
                gpui::HighlightStyle {
                    color: Some(gpui::rgb(fg).into()),
                    ..Default::default()
                },
            ));
        }
        i = j;
    }
    if let Some(m) = l.mark.clone().filter(|m| !m.is_empty()) {
        let bg = p
            .scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
            .unwrap_or(p.selection);
        // The mark's background over the colours of the chars under it.
        let mut out = Vec::new();
        let (ms, me) = (byte(m.start), byte(m.end));
        for (r, h) in highlights {
            if r.end <= ms || r.start >= me {
                out.push((r, h));
                continue;
            }
            if r.start < ms {
                out.push((r.start..ms, h));
            }
            if r.end > me {
                out.push((me..r.end, h));
            }
        }
        let mut inside: Vec<(Range<usize>, gpui::HighlightStyle)> = Vec::new();
        let mut c = m.start;
        while c < m.end.min(colour.len()) {
            let j = (c..m.end.min(colour.len()))
                .find(|&j| colour[j] != colour[c])
                .unwrap_or(m.end.min(colour.len()));
            inside.push((
                byte(c)..byte(j),
                gpui::HighlightStyle {
                    color: colour[c].map(|fg| gpui::rgb(fg).into()),
                    background_color: Some(gpui::rgb(bg).into()),
                    ..Default::default()
                },
            ));
            c = j;
        }
        out.extend(inside);
        out.sort_by_key(|(r, _)| r.start);
        highlights = out;
    }
    gpui::StyledText::new(l.text.clone()).with_highlights(highlights)
}

/// The excerpt as rows with line numbers in the editor font; the current line on the caret row's
/// background.
pub fn render(lines: &[ExcerptLine]) -> gpui::Div {
    let p = Palette::current();
    div()
        .flex()
        .flex_col()
        .font_family(crate::fonts::editor_family())
        .text_size(px(12.))
        .children(lines.iter().map(|l| {
            div()
                .flex()
                .h(px(18.))
                .items_center()
                .whitespace_nowrap()
                .overflow_hidden()
                .when_current(l.current, p.caret_row)
                .child(
                    div()
                        .w(px(40.))
                        .flex_none()
                        .pr_2()
                        .flex()
                        .justify_end()
                        .text_color(gpui::rgb(p.line_number))
                        .child(l.number.to_string()),
                )
                .child(styled_line(l))
        }))
}

trait WhenCurrent {
    fn when_current(self, on: bool, colour: u32) -> Self;
}

impl WhenCurrent for gpui::Div {
    fn when_current(self, on: bool, colour: u32) -> Self {
        if on { self.bg(gpui::rgb(colour)) } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_lines_spans_and_mark() {
        let text = "class A {\n    int total;\n    // order total\n}\n";
        let lines = excerpt(Path::new("A.java"), text, 2, Some(7..12), 1, 1);
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines.iter().map(|l| l.number).collect::<Vec<_>>(),
            [2, 3, 4]
        );
        assert!(lines[1].current && !lines[0].current);
        assert_eq!(lines[1].mark, Some(7..12));
        assert!(lines[0].mark.is_none());
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|(r, k)| k.starts_with("comment") && r.start == 4),
            "{:?}",
            lines[1].spans
        );
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|(_, k)| k.starts_with("keyword") || k.starts_with("type"))
        );
        // Unknown languages and the edges of the text.
        let plain = excerpt(Path::new("x.unknown"), "a\nb", 0, None, 5, 5);
        assert_eq!(plain.len(), 2);
        assert!(plain.iter().all(|l| l.spans.is_empty()));
        assert!(excerpt(Path::new("x"), "", 0, None, 1, 1).len() <= 1);
    }
}
