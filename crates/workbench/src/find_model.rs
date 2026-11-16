//! What the find bar of the editor shows and selects (`SPEC-0065` behaviours 1–4), without toolkit.

use std::ops::Range;

use idecraft_core::{Finder, Query};

/// Each match of `query` in `text` with its replacement (capture references resolved with a regex),
/// in text order (`SPEC-0065` behaviour 8).
pub fn replacements(query: &Query, text: &str, replacement: &str) -> Vec<(Range<usize>, String)> {
    Finder::new(query)
        .map(|f| {
            f.replace(text, replacement)
                .0
                .into_iter()
                .map(|r| (r.found.start..r.found.end, r.new))
                .collect()
        })
        .unwrap_or_default()
}

/// The replacement in the case of the matched text (`SPEC-0065` v4 behaviour 16): all upper case
/// stays upper case, a capital first letter stays capital, all lower case stays lower case; other
/// mixes keep the replacement as typed.
pub fn preserve_case(matched: &str, replacement: &str) -> String {
    let letters: Vec<char> = matched.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return replacement.to_owned();
    }
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return replacement.to_uppercase();
    }
    if letters.iter().all(|c| c.is_lowercase()) {
        return replacement.to_lowercase();
    }
    if letters[0].is_uppercase() {
        let mut out = String::new();
        let mut chars = replacement.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
        }
        out.extend(chars);
        return out;
    }
    replacement.to_owned()
}

/// Each match with its replacement as [`replacements`] gives, with the case of each match kept when
/// `keep_case` is set, leaving out the matches starting at an `excluded` offset (v4 behaviours 16, 17).
pub fn replacements_with(
    query: &Query,
    text: &str,
    replacement: &str,
    keep_case: bool,
    excluded: &[usize],
) -> Vec<(Range<usize>, String)> {
    let chars: Vec<char> = text.chars().collect();
    replacements(query, text, replacement)
        .into_iter()
        .filter(|(r, _)| !excluded.contains(&r.start))
        .map(|(r, new)| {
            if keep_case {
                let old: String = chars[r.start.min(chars.len())..r.end.min(chars.len())]
                    .iter()
                    .collect();
                let new = preserve_case(&old, &new);
                (r, new)
            } else {
                (r, new)
            }
        })
        .collect()
}

/// Keeps the matches inside `within` (search in selection) that `context` allows (comments and
/// string literals in the file `path`), `SPEC-0065` v4 behaviours 12–13.
pub fn narrow_matches(
    matches: Vec<Range<usize>>,
    within: Option<&Range<usize>>,
    context: idecraft_languages::syntax::SearchContext,
    path: &std::path::Path,
    text: &str,
) -> Vec<Range<usize>> {
    let matches: Vec<Range<usize>> = matches
        .into_iter()
        .filter(|m| within.is_none_or(|w| w.start <= m.start && m.end <= w.end))
        .collect();
    context.filter(path, text, matches)
}

/// Matches of a query in one text and the current one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindState {
    /// Char ranges of all matches, in text order.
    pub matches: Vec<Range<usize>>,
    /// Index of the current match.
    pub current: Option<usize>,
    /// Why the query cannot run (invalid regular expression).
    pub error: Option<String>,
}

impl FindState {
    /// Searches `text`; the current match is the first one starting at or after `from` (the caret),
    /// else the first one (behaviour 2).
    pub fn search(query: &Query, text: &str, from: usize) -> Self {
        match Finder::new(query) {
            Ok(finder) => {
                let matches: Vec<Range<usize>> = finder
                    .find(text)
                    .into_iter()
                    .map(|m| m.start..m.end)
                    .collect();
                let current = (!matches.is_empty())
                    .then(|| matches.iter().position(|m| m.start >= from).unwrap_or(0));
                Self {
                    matches,
                    current,
                    error: None,
                }
            }
            Err(e) => Self {
                error: Some(e.to_string()),
                ..Self::default()
            },
        }
    }

    /// Keeps only `kept` of the matches (selection, comments, strings; v4) and picks the current
    /// one again from `from`.
    pub fn retain(&mut self, kept: Vec<Range<usize>>, from: usize) {
        self.matches = kept;
        self.current = (!self.matches.is_empty()).then(|| {
            self.matches
                .iter()
                .position(|m| m.start >= from)
                .unwrap_or(0)
        });
    }

    /// Moves to the next or previous match, wrapping around (behaviour 3).
    pub fn step(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = Some(match (self.current, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        });
    }

    /// Range of the current match.
    pub fn current_range(&self) -> Option<Range<usize>> {
        self.matches.get(self.current?).cloned()
    }

    /// Counter text of the bar: `2/5`, `0 results`, or the error (behaviour 4).
    pub fn label(&self, pattern: &str) -> String {
        if let Some(e) = &self.error {
            return e.clone();
        }
        match self.current {
            Some(i) => format!("{}/{}", i + 1, self.matches.len()),
            None if pattern.is_empty() => String::new(),
            None => "0 results".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pattern: &str) -> Query {
        Query {
            pattern: pattern.into(),
            ..Query::default()
        }
    }

    #[test]
    fn current_match_follows_the_caret_and_wraps() {
        let text = "ab ab äb ab";
        let mut s = FindState::search(&q("b"), text, 4);
        assert_eq!(s.matches, [1..2, 4..5, 7..8, 10..11]);
        assert_eq!(s.current, Some(1));
        assert_eq!(s.label("b"), "2/4");
        s.step(false);
        s.step(false);
        assert_eq!(s.current_range(), Some(10..11), "wraps backwards");
        s.step(true);
        assert_eq!(s.current, Some(0), "wraps forwards");
        let after_last = FindState::search(&q("b"), text, 11);
        assert_eq!(after_last.current, Some(0));
    }

    #[test]
    fn options_empty_and_errors() {
        let text = "Foo foo food";
        let case = Query {
            match_case: true,
            ..q("foo")
        };
        assert_eq!(FindState::search(&case, text, 0).matches.len(), 2);
        let words = Query {
            whole_words: true,
            ..q("foo")
        };
        assert_eq!(FindState::search(&words, text, 0).matches.len(), 2);
        let empty = FindState::search(&q(""), text, 0);
        assert_eq!((empty.current, empty.label("")), (None, String::new()));
        assert_eq!(
            FindState::search(&q("zzz"), text, 0).label("zzz"),
            "0 results"
        );
        let bad = Query {
            regex: true,
            ..q("(")
        };
        let bad = FindState::search(&bad, text, 0);
        assert!(bad.label("(").starts_with("invalid regular expression"));
    }

    /// SPEC-0065 v4 AK-6: preserve case, excluded matches, selection and comment filter.
    #[test]
    fn preserve_case_exclude_and_narrowing() {
        assert_eq!(preserve_case("total", "sum"), "sum");
        assert_eq!(preserve_case("Total", "sum"), "Sum");
        assert_eq!(preserve_case("TOTAL", "sum"), "SUM");
        assert_eq!(preserve_case("tOtal", "sum"), "sum");
        assert_eq!(preserve_case("42", "sum"), "sum");
        let text = "total Total TOTAL";
        let all = replacements_with(&q("total"), text, "sum", true, &[]);
        let news: Vec<&str> = all.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(news, ["sum", "Sum", "SUM"]);
        let some = replacements_with(&q("total"), text, "sum", false, &[6]);
        assert_eq!(some.len(), 2);
        assert!(some.iter().all(|(r, n)| r.start != 6 && n == "sum"));
        let code = "int total; // total\nint x = total;\n";
        let path = std::path::Path::new("A.java");
        let found = FindState::search(&q("total"), code, 0).matches;
        assert_eq!(found.len(), 3);
        let first_line = narrow_matches(
            found.clone(),
            Some(&(0..19)),
            idecraft_languages::syntax::SearchContext::Anywhere,
            path,
            code,
        );
        assert_eq!(first_line.len(), 2);
        let no_comments = narrow_matches(
            found,
            None,
            idecraft_languages::syntax::SearchContext::ExceptComments,
            path,
            code,
        );
        assert_eq!(no_comments.len(), 2);
        let mut s = FindState::search(&q("total"), code, 0);
        s.retain(no_comments, 5);
        assert_eq!(s.label("total"), "2/2");
    }

    #[test]
    fn replacements_resolve_captures() {
        let text = "a1 b2 a3";
        let re = Query {
            regex: true,
            ..q(r"a(\d)")
        };
        assert_eq!(
            replacements(&re, text, "x$1"),
            [(0..2, "x1".to_owned()), (6..8, "x3".to_owned())]
        );
        assert_eq!(replacements(&q("$1"), "$1", "y"), [(0..2, "y".to_owned())]);
        assert!(
            replacements(
                &Query {
                    regex: true,
                    ..q("(")
                },
                text,
                "z"
            )
            .is_empty()
        );
    }
}
