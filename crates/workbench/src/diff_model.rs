//! What the diff viewer draws on a line (`SPEC-0084` v2 behaviours 7–8), free of the UI toolkit:
//! the syntax colours of each side by line, the changed words, and both as styled pieces of the line
//! as it is drawn (tabs as four spaces).

use std::ops::Range;
use std::path::Path;

/// Syntax capture kinds per line of `text` (the content of `path`), as char ranges within each
/// line (behaviour 7); one entry per line as `str::lines` counts them, empty without a grammar.
pub fn syntax_by_line(path: &Path, text: &str) -> Vec<Vec<(Range<usize>, String)>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Vec<(Range<usize>, String)>> = vec![Vec::new(); lines.len()];
    let Some(language) = ferris_idea_languages::LanguageId::detect(path) else {
        return out;
    };
    if text.len() > ferris_idea_languages::syntax::MAX_SYNTAX_BYTES {
        return out;
    }
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    let Ok(doc) = ferris_idea_languages::syntax::SyntaxDocument::new(language, tsx, text) else {
        return out;
    };
    // Char offset where each line starts (the separators `\n` count one char).
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for piece in text.split('\n').take(lines.len()) {
        starts.push(at);
        at += piece.chars().count() + 1;
    }
    for span in doc.highlight() {
        let first = starts
            .partition_point(|&s| s <= span.start)
            .saturating_sub(1);
        for (i, &start) in starts.iter().enumerate().skip(first) {
            if start >= span.end {
                break;
            }
            let len = lines[i].chars().count();
            let from = span.start.saturating_sub(start).min(len);
            let to = (span.end - start).min(len);
            if from < to {
                out[i].push((from..to, span.kind.clone()));
            }
        }
    }
    out
}

/// A piece of a drawn line: display byte range, the syntax kind over it, and whether it is part of
/// a changed word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// Display bytes.
    pub bytes: Range<usize>,
    /// Syntax capture kind, if any.
    pub kind: Option<String>,
    /// Inside a changed word (behaviour 8).
    pub word: bool,
}

/// `line` as drawn (tabs as four spaces) and the pieces that carry a colour or a word mark; `kinds`
/// and `words` are char ranges of `line`. Later kinds win where captures overlap.
pub fn styled_line(
    line: &str,
    kinds: &[(Range<usize>, String)],
    words: &[Range<usize>],
) -> (String, Vec<Piece>) {
    let mut text = String::with_capacity(line.len());
    let mut byte_of = Vec::with_capacity(line.len() + 1);
    for ch in line.chars() {
        byte_of.push(text.len());
        if ch == '\t' {
            text.push_str("    ");
        } else {
            text.push(ch);
        }
    }
    byte_of.push(text.len());
    let chars = byte_of.len() - 1;
    let mut cuts: Vec<usize> = vec![0, chars];
    for r in kinds.iter().map(|k| &k.0).chain(words) {
        cuts.push(r.start.min(chars));
        cuts.push(r.end.min(chars));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut pieces: Vec<Piece> = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let kind = kinds
            .iter()
            .rev()
            .find(|(r, _)| r.start <= a && b <= r.end)
            .map(|(_, k)| k.clone());
        let word = words.iter().any(|r| r.start <= a && b <= r.end);
        if kind.is_none() && !word {
            continue;
        }
        let bytes = byte_of[a]..byte_of[b];
        match pieces.last_mut() {
            Some(last)
                if last.bytes.end == bytes.start && last.kind == kind && last.word == word =>
            {
                last.bytes.end = bytes.end;
            }
            _ => pieces.push(Piece { bytes, kind, word }),
        }
    }
    (text, pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0084 v2 AK-6.
    #[test]
    fn syntax_runs_follow_their_lines() {
        let text = "class A {\n  int n = 1;\n}\n";
        let by = syntax_by_line(Path::new("A.java"), text);
        assert_eq!(by.len(), 3);
        assert!(
            by[0]
                .iter()
                .any(|(r, k)| *r == (0..5) && k.contains("keyword")),
            "{:?}",
            by[0]
        );
        assert!(by[1].iter().any(|(r, _)| *r == (2..5)), "{:?}", by[1]);
        // No grammar: plain lines.
        let plain = syntax_by_line(Path::new("notes.unknown-ext"), "a\nb\n");
        assert_eq!(plain, vec![Vec::new(), Vec::new()]);
    }

    // SPEC-0084 v2 AK-5: colours and changed words as pieces of the drawn line.
    #[test]
    fn pieces_of_a_line() {
        let kinds = vec![(0..3, "keyword".to_owned())];
        let words = vec![8..12, 20..20];
        let (text, pieces) = styled_line("int\tx = cost;", &kinds, &words);
        assert_eq!(text, "int    x = cost;");
        assert_eq!(
            pieces,
            [
                Piece {
                    bytes: 0..3,
                    kind: Some("keyword".into()),
                    word: false
                },
                Piece {
                    bytes: 11..15,
                    kind: None,
                    word: true
                },
            ]
        );
        assert!(styled_line("abc", &[], &[]).1.is_empty());
    }
}
