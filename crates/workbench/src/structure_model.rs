//! Rows of the File Structure popup (`SPEC-0079`): the file's symbols as a tree, filtered by typing,
//! free of the UI toolkit.

use std::collections::HashSet;
use std::ops::Range;

use idecraft_languages::insight::Symbol;

/// Indices of the symbols to show for `query`: those whose name contains every typed word
/// (ignoring case), together with their parents so the tree stays readable (behaviour 3).
pub fn filter(symbols: &[Symbol], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return (0..symbols.len()).collect();
    }
    let mut keep = vec![false; symbols.len()];
    for (i, s) in symbols.iter().enumerate() {
        let name = s.name.to_lowercase();
        if !words.iter().all(|w| name.contains(w.as_str())) {
            continue;
        }
        keep[i] = true;
        // Parents are the nearest earlier symbols with a smaller depth.
        let mut depth = s.depth;
        for j in (0..i).rev() {
            if depth == 0 {
                break;
            }
            if symbols[j].depth < depth {
                keep[j] = true;
                depth = symbols[j].depth;
            }
        }
    }
    (0..symbols.len()).filter(|&i| keep[i]).collect()
}

/// Whether row `i` of `shown` matches the query itself (not only kept as a parent).
pub fn matches(symbol: &Symbol, query: &str) -> bool {
    let name = symbol.name.to_lowercase();
    query
        .split_whitespace()
        .all(|w| name.contains(&w.to_lowercase()))
}

/// The row to select first: the innermost symbol of the file around the caret (`line`,
/// `character`), else the one starting last before it, so the popup opens on the symbol you are
/// in (behaviour 2; v4: the order is no longer the document's).
pub fn at_caret(symbols: &[Symbol], shown: &[usize], line: u64, character: u64) -> usize {
    let own = |i: &usize| symbols[*i].path.is_none();
    if let Some(row) = shown
        .iter()
        .enumerate()
        .filter(|(_, i)| own(i) && symbols[**i].contains(line, character))
        .max_by_key(|(_, i)| symbols[**i].depth)
        .map(|(row, _)| row)
    {
        return row;
    }
    shown
        .iter()
        .enumerate()
        .filter(|(_, i)| own(i) && (symbols[**i].line, symbols[**i].character) <= (line, character))
        .max_by_key(|(_, i)| (symbols[**i].line, symbols[**i].character))
        .map_or(0, |(row, _)| row)
}

/// Whether symbol `i` has rows below it.
pub fn has_children(symbols: &[Symbol], i: usize) -> bool {
    symbols
        .get(i + 1)
        .is_some_and(|next| next.depth > symbols[i].depth)
}

/// The nested types closed when the popup opens: every type below another symbol, unless the
/// caret is inside it (`SPEC-0079` v4 behaviour 8).
pub fn closed_at_start(symbols: &[Symbol], line: u64, character: u64) -> HashSet<usize> {
    (0..symbols.len())
        .filter(|&i| {
            let s = &symbols[i];
            s.depth > 0
                && idecraft_languages::structure::is_type(s)
                && has_children(symbols, i)
                && !(s.path.is_none() && s.contains(line, character))
        })
        .collect()
}

/// `shown` without the rows below a closed one (behaviour 8).
pub fn open_rows(symbols: &[Symbol], shown: &[usize], closed: &HashSet<usize>) -> Vec<usize> {
    let mut out = Vec::with_capacity(shown.len());
    let mut hidden_below: Option<usize> = None;
    for &i in shown {
        if let Some(d) = hidden_below {
            if symbols[i].depth > d {
                continue;
            }
            hidden_below = None;
        }
        out.push(i);
        if closed.contains(&i) {
            hidden_below = Some(symbols[i].depth);
        }
    }
    out
}

/// The byte ranges of `name` that match the typed words, ignoring case, merged where they
/// overlap (`SPEC-0079` v4 behaviour 10).
pub fn matched_ranges(name: &str, query: &str) -> Vec<Range<usize>> {
    let lower = name.to_lowercase();
    // Lower-casing may change byte lengths; then nothing is marked rather than the wrong part.
    if lower.len() != name.len() {
        return Vec::new();
    }
    let mut ranges: Vec<Range<usize>> = query
        .split_whitespace()
        .filter_map(|w| {
            let w = w.to_lowercase();
            lower.find(&w).map(|at| at..at + w.len())
        })
        .collect();
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

pub use idecraft_languages::structure::without_packages;

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0079 v3 AK-5.
    #[test]
    fn the_package_row_is_left_out() {
        let mut package = sym("lab.inventory", 0, 0);
        package.kind = "package".into();
        let shown = without_packages(vec![
            package.clone(),
            sym("Discounts", 0, 2),
            sym("None", 1, 3),
        ]);
        let names: Vec<(&str, usize)> = shown.iter().map(|s| (s.name.as_str(), s.depth)).collect();
        assert_eq!(names, [("Discounts", 0), ("None", 1)]);
        // Symbols nested in a package move up a level.
        let nested = without_packages(vec![
            package,
            sym("A", 1, 1),
            sym("f", 2, 2),
            sym("B", 0, 4),
        ]);
        let names: Vec<(&str, usize)> = nested.iter().map(|s| (s.name.as_str(), s.depth)).collect();
        assert_eq!(names, [("A", 0), ("f", 1), ("B", 0)]);
    }

    fn sym(name: &str, depth: usize, line: u64) -> Symbol {
        Symbol {
            name: name.into(),
            kind: "function".into(),
            line,
            depth,
            ..Symbol::default()
        }
    }

    fn tree() -> Vec<Symbol> {
        vec![
            sym("Point", 0, 0),
            sym("x", 1, 1),
            sym("length", 1, 2),
            sym("main", 0, 5),
            sym("helper", 0, 9),
        ]
    }

    #[test]
    fn filter_keeps_parents_of_matches() {
        let t = tree();
        assert_eq!(filter(&t, ""), [0, 1, 2, 3, 4]);
        assert_eq!(filter(&t, "len"), [0, 2]);
        assert_eq!(filter(&t, "MA"), [3]);
        assert!(filter(&t, "zzz").is_empty());
        assert!(matches(&t[2], "len") && !matches(&t[0], "len"));
    }

    /// SPEC-0079 v4 AK-6: nested types start closed unless the caret is inside.
    #[test]
    fn nested_types_start_closed() {
        let s = |name: &str, kind: &str, depth: usize, lines: (u64, u64)| Symbol {
            name: name.into(),
            kind: kind.into(),
            depth,
            line: lines.0,
            end_line: lines.1,
            end_character: 1,
            ..Symbol::default()
        };
        let t = vec![
            s("Discounts", "class", 0, (2, 23)),
            s("Half", "class", 1, (9, 13)),
            s("apply(int)", "method", 2, (10, 12)),
            s("None", "class", 1, (3, 7)),
            s("apply(int)", "method", 2, (4, 6)),
            s("priceOf(Item, Discount)", "method", 1, (15, 17)),
        ];
        let closed = closed_at_start(&t, 11, 4);
        assert_eq!(closed, HashSet::from([3]), "Half holds the caret");
        let all: Vec<usize> = (0..t.len()).collect();
        assert_eq!(open_rows(&t, &all, &closed), [0, 1, 2, 3, 5]);
        assert_eq!(at_caret(&t, &all, 11, 4), 2, "the method around the caret");
        assert_eq!(at_caret(&t, &all, 16, 0), 5);
        assert!(has_children(&t, 1) && !has_children(&t, 2));
    }

    /// SPEC-0079 v4 AK-6.
    #[test]
    fn matched_parts_of_a_name() {
        assert_eq!(matched_ranges("priceOf(Item)", "ce it"), [3..5, 8..10]);
        let r = matched_ranges("apply", "AP p");
        assert_eq!((r.len(), r[0].start, r[0].end), (1, 0, 2));
        assert!(matched_ranges("apply", "").is_empty());
    }

    #[test]
    fn selection_starts_at_the_symbol_around_the_caret() {
        let t = tree();
        let all = filter(&t, "");
        assert_eq!(at_caret(&t, &all, 0, 0), 0);
        assert_eq!(at_caret(&t, &all, 3, 4), 2);
        assert_eq!(at_caret(&t, &all, 7, 0), 3);
        assert_eq!(at_caret(&t, &all, 20, 0), 4);
    }
}
