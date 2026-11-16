//! Semantic tokens of an editor (`SPEC-0290`), free of the UI toolkit: char ranges with colour keys,
//! moved by edits until the next answer, and laid over the syntax colours of a line.

use std::ops::Range;

/// The tokens an editor paints over its syntax colours.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticSpans {
    /// Char ranges with their colour keys, sorted and not overlapping.
    spans: Vec<(Range<usize>, &'static str)>,
}

impl SemanticSpans {
    /// Spans from the server's tokens; `line_start` gives the char offset of a line. Tokens without
    /// a colour key are left out.
    pub fn from_tokens(
        tokens: &[idecraft_languages::semantic::SemanticToken],
        line_start: impl Fn(u64) -> Option<usize>,
    ) -> Self {
        let mut spans: Vec<(Range<usize>, &'static str)> = tokens
            .iter()
            .filter_map(|t| {
                let key = t.key?;
                let at = line_start(t.line)?;
                Some((at + t.start as usize..at + t.end as usize, key))
            })
            .collect();
        spans.sort_by_key(|(r, _)| (r.start, r.end));
        let mut end = 0;
        spans.retain(|(r, _)| {
            let keep = r.start >= end;
            if keep {
                end = r.end;
            }
            keep
        });
        Self { spans }
    }

    /// The spans.
    pub fn spans(&self) -> &[(Range<usize>, &'static str)] {
        &self.spans
    }

    /// Spans intersecting `range`.
    pub fn within(&self, range: Range<usize>) -> &[(Range<usize>, &'static str)] {
        let first = self.spans.partition_point(|(r, _)| r.end <= range.start);
        let last = self.spans.partition_point(|(r, _)| r.start < range.end);
        &self.spans[first..last.max(first)]
    }

    /// Moves the spans after a change of `deleted` chars at `offset` replaced by `inserted` chars;
    /// spans the change touches are dropped (behaviour 5).
    pub fn edited(&mut self, offset: usize, deleted: usize, inserted: usize) {
        let end = offset + deleted;
        self.spans.retain_mut(|(r, _)| {
            if r.end <= offset {
                true
            } else if r.start >= end && !(deleted == 0 && r.start == offset) {
                r.start = r.start - deleted + inserted;
                r.end = r.end - deleted + inserted;
                true
            } else {
                false
            }
        });
    }

    /// Forgets every span.
    pub fn clear(&mut self) {
        self.spans.clear();
    }
}

/// Lays `top` over `base` (behaviour 4): both sorted runs of one line; inside a `top` range its
/// style replaces the base, outside it the base stays. The result is sorted and not overlapping.
pub fn overlay<T: Clone>(
    base: &[(Range<usize>, T)],
    top: &[(Range<usize>, T)],
) -> Vec<(Range<usize>, T)> {
    if top.is_empty() {
        return base.to_vec();
    }
    let mut out = Vec::with_capacity(base.len() + top.len() * 2);
    let push_base = |out: &mut Vec<(Range<usize>, T)>, r: Range<usize>, t: &T| {
        if r.start < r.end {
            out.push((r, t.clone()));
        }
    };
    // Base pieces between the top ranges.
    let mut cut: Vec<Range<usize>> = top.iter().map(|(r, _)| r.clone()).collect();
    cut.sort_by_key(|r| r.start);
    for (r, t) in base {
        let mut at = r.start;
        for c in cut.iter().filter(|c| c.end > r.start && c.start < r.end) {
            push_base(&mut out, at..c.start.max(at).min(r.end), t);
            at = at.max(c.end);
        }
        push_base(&mut out, at.min(r.end)..r.end, t);
    }
    for (r, t) in top {
        if r.start < r.end {
            out.push((r.clone(), t.clone()));
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

/// Counts the failed questions of `file` in a row in `retries` and says how long to wait before
/// asking again (`SPEC-0290` v3 behaviour 1a): `error` is the failure, or `None` for an answer,
/// which ends the count. `None` when the failure is final or the tries are used up.
pub fn retry_after(
    retries: &mut std::collections::HashMap<std::path::PathBuf, u32>,
    file: &std::path::Path,
    error: Option<&idecraft_languages::lsp::LspError>,
) -> Option<std::time::Duration> {
    let Some(error) = error else {
        retries.remove(file);
        return None;
    };
    if !idecraft_languages::semantic::worth_asking_again(error) {
        return None;
    }
    let attempt = retries.entry(file.to_path_buf()).or_insert(0);
    *attempt = attempt.saturating_add(1);
    idecraft_languages::semantic::retry_delay(*attempt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_languages::semantic::SemanticToken;

    /// SPEC-0290 AK-8 (v3): a file's failed questions are asked again after growing pauses, five
    /// times at most; an answer starts the count anew; a missing server is never asked again.
    #[test]
    fn a_starting_server_is_asked_again() {
        use idecraft_languages::lsp::LspError;
        let mut retries = std::collections::HashMap::new();
        let file = std::path::Path::new("/p/Main.kt");
        let slow = LspError::Timeout("initialize".into(), 30);
        let pauses: Vec<u64> = (0..7)
            .filter_map(|_| retry_after(&mut retries, file, Some(&slow)))
            .map(|d| d.as_secs())
            .collect();
        assert_eq!(pauses, [3, 10, 30, 60, 120]);
        assert_eq!(retry_after(&mut retries, file, None), None);
        assert!(retries.is_empty());
        assert_eq!(
            retry_after(&mut retries, file, Some(&slow)).map(|d| d.as_secs()),
            Some(3)
        );
        let other = std::path::Path::new("/p/notes.kt");
        let missing = LspError::NoServer("kotlin".into());
        assert_eq!(retry_after(&mut retries, other, Some(&missing)), None);
        assert!(!retries.contains_key(other));
    }

    fn token(line: u64, start: u64, end: u64, key: Option<&'static str>) -> SemanticToken {
        SemanticToken {
            line,
            start,
            end,
            token_type: "method".into(),
            modifiers: Vec::new(),
            key,
        }
    }

    /// SPEC-0290 AK-3: tokens become char ranges per line, keyless ones are left out.
    #[test]
    fn tokens_become_char_ranges() {
        let text = "ab\ncdef\n";
        let starts = [0usize, 3, 8];
        let s = SemanticSpans::from_tokens(
            &[
                token(1, 1, 3, Some("STATIC_FIELD_ATTRIBUTES")),
                token(0, 0, 2, Some("METHOD_DECLARATION_ATTRIBUTES")),
                token(0, 1, 2, None),
                token(9, 0, 1, Some("X")),
            ],
            |l| starts.get(l as usize).copied(),
        );
        assert_eq!(
            s.spans(),
            [
                (0..2, "METHOD_DECLARATION_ATTRIBUTES"),
                (4..6, "STATIC_FIELD_ATTRIBUTES")
            ]
        );
        assert_eq!(&text[4..6], "de");
        assert_eq!(s.within(3..8), [(4..6, "STATIC_FIELD_ATTRIBUTES")]);
        assert!(s.within(2..3).is_empty());
    }

    /// SPEC-0290 AK-3: an insert before a span moves it, an edit inside drops it, an insert right at
    /// its start drops it (the name may have changed).
    #[test]
    fn edits_move_or_drop_spans() {
        let mut s = SemanticSpans {
            spans: vec![(2..5, "A"), (10..14, "B"), (20..22, "C")],
        };
        s.edited(0, 0, 3); // insert three chars at the start
        assert_eq!(s.spans(), [(5..8, "A"), (13..17, "B"), (23..25, "C")]);
        s.edited(14, 1, 0); // delete inside B
        assert_eq!(s.spans(), [(5..8, "A"), (22..24, "C")]);
        s.edited(22, 0, 1); // type right before C
        assert_eq!(s.spans(), [(5..8, "A")]);
        s.edited(8, 0, 2); // type right after A
        assert_eq!(s.spans(), [(5..8, "A")]);
    }

    /// SPEC-0290 AK-3: the top runs replace the base only inside their ranges.
    #[test]
    fn tokens_override_syntax_runs_inside_their_range() {
        let base = vec![(0..6, 'k'), (7..12, 's')];
        let top = vec![(2..4, 'm'), (8..10, 'f'), (13..15, 'p')];
        assert_eq!(
            overlay(&base, &top),
            [
                (0..2, 'k'),
                (2..4, 'm'),
                (4..6, 'k'),
                (7..8, 's'),
                (8..10, 'f'),
                (10..12, 's'),
                (13..15, 'p')
            ]
        );
        assert_eq!(overlay(&base, &[]), base);
        assert_eq!(overlay(&[], &[(1..3, 'x')]), [(1..3, 'x')]);
        // A top run covering a base run entirely removes it.
        assert_eq!(overlay(&[(2..4, 'k')], &[(0..6, 'x')]), [(0..6, 'x')]);
    }
}
