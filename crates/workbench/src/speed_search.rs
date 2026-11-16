//! Speed search in a tree (`SPEC-0089` v5 behaviour 10): typed letters find the first visible row
//! whose name contains them, ignoring case; ↑/↓ go to the previous and next match. Free of the UI
//! toolkit, so the Database tool window and its tests share it.

use std::ops::Range;

/// The part of `label` that `query` matches, ignoring case, as a byte range of `label`; `None`
/// when it does not match or the query is empty. When lower-casing changes byte lengths the whole
/// label is marked rather than a wrong part.
pub fn match_range(label: &str, query: &str) -> Option<Range<usize>> {
    if query.is_empty() {
        return None;
    }
    let lower = label.to_lowercase();
    let at = lower.find(&query.to_lowercase())?;
    if lower.len() != label.len() {
        return Some(0..label.len());
    }
    Some(at..at + query.to_lowercase().len())
}

/// The row to select for `query`: the first matching row at or after `from`, wrapping around;
/// `None` without a match. Rows that are not objects (`None`) never match.
pub fn first_match(labels: &[Option<&str>], query: &str, from: usize) -> Option<usize> {
    let n = labels.len();
    (0..n)
        .map(|k| (from + k) % n)
        .find(|&i| labels[i].is_some_and(|l| match_range(l, query).is_some()))
}

/// The next (`forward`) or previous matching row after `from`, wrapping around; `from` itself when
/// it is the only match; `None` without a match.
pub fn step(labels: &[Option<&str>], query: &str, from: usize, forward: bool) -> Option<usize> {
    let n = labels.len();
    if n == 0 {
        return None;
    }
    (1..=n)
        .map(|k| {
            if forward {
                (from + k) % n
            } else {
                (from + n * 2 - k) % n
            }
        })
        .find(|&i| labels[i].is_some_and(|l| match_range(l, query).is_some()))
}

/// Whether a typed character starts or extends the search: printable, and not a space at the start.
pub fn types_into(query: &str, ch: &str) -> bool {
    !ch.is_empty()
        && !ch.chars().any(char::is_control)
        && !(query.is_empty() && ch.chars().all(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0089 v5 AK-8: matching ignores case and marks the matched part.
    #[test]
    fn matching_ignores_case_and_marks_the_part() {
        assert_eq!(match_range("Orders", "ord"), Some(0..3));
        assert_eq!(match_range("customers", "TOM"), Some(3..6));
        assert_eq!(match_range("orders", "x"), None);
        assert_eq!(match_range("orders", ""), None);
        // A label whose lower case is longer is marked whole.
        assert_eq!(match_range("İd", "d"), Some(0..3));
    }

    // SPEC-0089 v5 AK-8: the first visible match from the selection, then ↑/↓ between matches.
    #[test]
    fn selection_moves_between_visible_matches() {
        let rows = [
            Some("shop"),
            Some("tables"),
            Some("customers"),
            None,
            Some("orders"),
            Some("order_items"),
        ];
        assert_eq!(first_match(&rows, "ord", 0), Some(4));
        // The selection is kept while it still matches.
        assert_eq!(first_match(&rows, "ord", 5), Some(5));
        assert_eq!(first_match(&rows, "zzz", 0), None);
        assert_eq!(step(&rows, "ord", 4, true), Some(5));
        assert_eq!(step(&rows, "ord", 5, true), Some(4));
        assert_eq!(step(&rows, "ord", 4, false), Some(5));
        assert_eq!(step(&rows, "cust", 2, true), Some(2));
        assert_eq!(step(&rows, "zzz", 2, true), None);
        assert_eq!(step(&[], "a", 0, true), None);
    }

    #[test]
    fn what_types_into_the_search() {
        assert!(types_into("", "o"));
        assert!(types_into("", "_"));
        assert!(!types_into("", " "));
        assert!(types_into("order", " "));
        assert!(!types_into("", "\u{1b}"));
        assert!(!types_into("", ""));
    }
}
