//! Sorting, filtering and paging of a result grid (`SPEC-0099`) on the loaded rows, for the window
//! and for agents.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

pub use crate::order::Direction;

/// Rows per page, as in the reference's default.
pub const PAGE_SIZE: usize = 500;

/// What a grid shows of its rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GridState {
    /// Sort columns in priority order.
    pub sort: Vec<(usize, Direction)>,
    /// Rows must contain this text in some cell (case-insensitive); empty shows all.
    pub filter: String,
    /// 0-based page.
    pub page: usize,
    /// Rows per page; 0 means [`PAGE_SIZE`] (v2 behaviour 5).
    pub page_size: usize,
    /// Value filters per column (v3 behaviour 9): a row is shown only when its value in each
    /// filtered column is one of the checked values (`None` is NULL).
    pub values: BTreeMap<usize, BTreeSet<Option<String>>>,
}

/// How a result is shown (v3 behaviour 10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResultView {
    /// Rows and columns.
    #[default]
    Table,
    /// One node per row, its columns as children.
    Tree,
    /// A plain-text table.
    Text,
}

impl ResultView {
    /// The view's name in the view menu.
    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "Table",
            Self::Tree => "Tree",
            Self::Text => "Text",
        }
    }

    /// The view called `name` (`table`, `tree`, `text`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Table, Self::Tree, Self::Text]
            .into_iter()
            .find(|v| v.label().eq_ignore_ascii_case(name.trim()))
    }
}

/// The values of `column` over all loaded rows with how often each occurs, in sort order (NULL
/// first) — the list of a column's value filter (v3 behaviour 9). Counting runs over every loaded
/// row, not just the page shown (IMP-0012).
pub fn column_values(rows: &[Vec<Option<String>>], column: usize) -> Vec<(Option<String>, usize)> {
    let mut counts: BTreeMap<Option<String>, usize> = BTreeMap::new();
    for row in rows {
        *counts
            .entry(row.get(column).cloned().flatten())
            .or_default() += 1;
    }
    let mut out: Vec<_> = counts.into_iter().collect();
    out.sort_by(|a, b| compare(&a.0, &b.0));
    out
}

impl GridState {
    /// Checks or unchecks `value` in the value filter of `column` (v3 behaviour 9); `all` are
    /// the column's values. A filter that keeps every value is removed. Goes back to the first
    /// page.
    pub fn toggle_value(&mut self, column: usize, value: Option<String>, all: &[Option<String>]) {
        let set = self
            .values
            .entry(column)
            .or_insert_with(|| all.iter().cloned().collect());
        if !set.remove(&value) {
            set.insert(value);
        }
        if all.iter().all(|v| set.contains(v)) {
            self.values.remove(&column);
        }
        self.page = 0;
    }

    /// Keeps only `value` in the value filter of `column` ("only this").
    pub fn only_value(&mut self, column: usize, value: Option<String>) {
        self.values.insert(column, std::iter::once(value).collect());
        self.page = 0;
    }

    /// Removes the value filter of `column` (`None`: of every column).
    pub fn clear_values(&mut self, column: Option<usize>) {
        match column {
            Some(c) => {
                self.values.remove(&c);
            }
            None => self.values.clear(),
        }
        self.page = 0;
    }

    /// Whether `column` has a value filter (its header shows the filter mark).
    pub fn filters(&self, column: usize) -> bool {
        self.values.contains_key(&column)
    }

    /// Whether `value` of `column` is checked in its value filter.
    pub fn checked(&self, column: usize, value: &Option<String>) -> bool {
        self.values
            .get(&column)
            .is_none_or(|set| set.contains(value))
    }
    /// A click on the header of `column` (behaviour 1): without `add` the column becomes the only
    /// sort key, cycling ascending → descending → unsorted; with `add` (⇧-click) it is added as the
    /// next key or its direction cycles in place. Goes back to the first page.
    pub fn click_header(&mut self, column: usize, add: bool) {
        let at = self.sort.iter().position(|(c, _)| *c == column);
        let next = match at.map(|i| self.sort[i].1) {
            None => Some(Direction::Asc),
            Some(Direction::Asc) => Some(Direction::Desc),
            Some(Direction::Desc) => None,
        };
        if add {
            match (at, next) {
                (Some(i), Some(d)) => self.sort[i].1 = d,
                (Some(i), None) => {
                    self.sort.remove(i);
                }
                (None, Some(d)) => self.sort.push((column, d)),
                (None, None) => {}
            }
        } else {
            self.sort = next.map(|d| vec![(column, d)]).unwrap_or_default();
        }
        self.page = 0;
    }

    /// Rows per page in effect (v2 behaviour 5).
    pub fn size(&self) -> usize {
        if self.page_size == 0 {
            PAGE_SIZE
        } else {
            self.page_size
        }
    }

    /// Chooses `size` rows per page and goes back to the first page (v2 behaviour 5).
    pub fn set_page_size(&mut self, size: usize) {
        self.page_size = size;
        self.page = 0;
    }

    /// The sort mark of `column` for its header: arrow, plus its priority when several columns sort.
    pub fn mark(&self, column: usize) -> Option<String> {
        let i = self.sort.iter().position(|(c, _)| *c == column)?;
        let arrow = match self.sort[i].1 {
            Direction::Asc => "▲",
            Direction::Desc => "▼",
        };
        Some(if self.sort.len() > 1 {
            format!("{arrow}{}", i + 1)
        } else {
            arrow.to_owned()
        })
    }
}

/// Orders two cells (ascending puts NULL first, descending last): NULL before values; two numbers by value; otherwise case-insensitively, then
/// exactly (behaviour 1).
pub fn compare(a: &Option<String>, b: &Option<String>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
            (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a
                .to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b)),
        },
    }
}

/// Indices of the rows to show, filtered and sorted (behaviours 1 and 2). Sorting is stable: rows
/// equal in every sort column keep the order the database returned.
pub fn visible(rows: &[Vec<Option<String>>], state: &GridState) -> Vec<usize> {
    let needle = state.filter.trim().to_lowercase();
    let mut out: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            needle.is_empty()
                || rows[i].iter().any(|v| {
                    v.as_deref()
                        .is_some_and(|t| t.to_lowercase().contains(&needle))
                })
        })
        .filter(|&i| {
            state
                .values
                .iter()
                .all(|(&c, set)| set.contains(&rows[i].get(c).cloned().flatten()))
        })
        .collect();
    if !state.sort.is_empty() {
        out.sort_by(|&x, &y| {
            for &(column, direction) in &state.sort {
                let a = rows[x].get(column).unwrap_or(&None);
                let b = rows[y].get(column).unwrap_or(&None);
                let o = match direction {
                    Direction::Asc => compare(a, b),
                    Direction::Desc => compare(b, a),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        });
    }
    out
}

/// Number of pages for `count` visible rows (at least one).
pub fn pages(count: usize) -> usize {
    pages_of(count, PAGE_SIZE)
}

/// The positions in the visible rows that page `page` shows (behaviour 3).
pub fn page_range(count: usize, page: usize) -> Range<usize> {
    page_range_of(count, page, PAGE_SIZE)
}

/// The paging label: "1–500 of 1200", "0 of 3 rows (filtered)", "3 of 10 rows (filtered)".
pub fn page_label(count: usize, total: usize, page: usize) -> String {
    page_label_of(count, total, page, PAGE_SIZE)
}

/// Page sizes offered next to the paging label (v2 behaviour 5); [`ALL_ROWS`] shows every row.
pub const PAGE_SIZES: [usize; 6] = [10, 100, 250, PAGE_SIZE, 1000, ALL_ROWS];

/// The page size that shows every row on one page (v2 behaviour 5).
pub const ALL_ROWS: usize = usize::MAX;

/// The name of a page size in the size menu: "500 (default)", "All".
pub fn page_size_label(size: usize) -> String {
    match size {
        ALL_ROWS => "All".to_owned(),
        PAGE_SIZE => format!("{PAGE_SIZE} (default)"),
        n => n.to_string(),
    }
}

/// The view a reloaded result keeps (v4 behaviour 12): the old sort, filters and page when the
/// columns are the same, the page clamped to the new row count; otherwise a fresh view.
pub fn kept_view(old: &GridState, same_columns: bool, rows: usize) -> GridState {
    if !same_columns {
        return GridState::default();
    }
    let mut kept = old.clone();
    kept.page = kept.page.min(pages_of(rows, kept.size()) - 1);
    kept
}

/// [`pages`] for pages of `size` rows (v2 behaviour 5).
pub fn pages_of(count: usize, size: usize) -> usize {
    count.div_ceil(size.max(1)).max(1)
}

/// [`page_range`] for pages of `size` rows (v2 behaviour 5).
pub fn page_range_of(count: usize, page: usize, size: usize) -> Range<usize> {
    let size = size.max(1);
    let page = page.min(pages_of(count, size) - 1);
    let start = page.saturating_mul(size);
    start..start.saturating_add(size).min(count)
}

/// [`page_label`] for pages of `size` rows (v2 behaviour 5).
pub fn page_label_of(count: usize, total: usize, page: usize, size: usize) -> String {
    let filtered = if count < total {
        format!(" (filtered from {total})")
    } else {
        String::new()
    };
    if count <= size {
        let rows = if count == 1 { "row" } else { "rows" };
        return format!("{count} {rows}{filtered}");
    }
    let r = page_range_of(count, page, size);
    format!("{}–{} of {count}{filtered}", r.start + 1, r.end)
}

/// Whether a cell text reads as a number: an optional sign, digits with at most one decimal point,
/// an optional exponent (`-9`, `0.25`, `1e3`); not `NaN`, `inf` or empty text.
pub fn is_number(text: &str) -> bool {
    let t = text.trim();
    let t = t.strip_prefix(['-', '+']).unwrap_or(t);
    let (mantissa, exponent) = match t.find(['e', 'E']) {
        Some(at) => (&t[..at], Some(&t[at + 1..])),
        None => (t, None),
    };
    let digits = mantissa.chars().filter(char::is_ascii_digit).count();
    let points = mantissa.chars().filter(|c| *c == '.').count();
    let mantissa_ok =
        digits > 0 && points <= 1 && mantissa.chars().all(|c| c.is_ascii_digit() || c == '.');
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
    });
    mantissa_ok && exponent_ok
}

/// Per column, whether it holds numbers: at least one value and every non-NULL value a number
/// (v2 behaviour 6: such columns are aligned right, like the reference).
pub fn numeric_columns(columns: usize, rows: &[Vec<Option<String>>]) -> Vec<bool> {
    (0..columns)
        .map(|k| {
            let mut any = false;
            for row in rows {
                if let Some(Some(v)) = row.get(k) {
                    if !is_number(v) {
                        return false;
                    }
                    any = true;
                }
            }
            any
        })
        .collect()
}

/// The transposed view of `rows` (v2 behaviour 7): one line per column, its name first, then the
/// value of each given row.
pub fn transpose(
    columns: &[String],
    rows: &[&[Option<String>]],
) -> Vec<(String, Vec<Option<String>>)> {
    columns
        .iter()
        .enumerate()
        .map(|(k, name)| {
            let values = rows.iter().map(|r| r.get(k).cloned().flatten()).collect();
            (name.clone(), values)
        })
        .collect()
}

/// Longest cell text in the text view before it is cut with `…` (v3 behaviour 10).
pub const TEXT_CELL_LIMIT: usize = 60;

/// One cell as the text and tree views show it: `<null>` for NULL, line breaks as `⏎`, cut after
/// `limit` characters with `…`.
fn cell_text(value: &Option<String>, limit: usize) -> String {
    let text = match value {
        None => return "<null>".to_owned(),
        Some(v) => v.replace("\r\n", "⏎").replace(['\n', '\r'], "⏎"),
    };
    if text.chars().count() > limit {
        let cut: String = text.chars().take(limit.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        text
    }
}

/// The text view of `rows` (v3 behaviour 10): a table drawn with `+`, `-` and `|`, the header
/// first, numeric columns aligned right, as it can be copied into a ticket or a commit message.
pub fn text_view(columns: &[String], rows: &[&[Option<String>]], numeric: &[bool]) -> String {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            (0..columns.len())
                .map(|k| cell_text(&r.get(k).cloned().flatten(), TEXT_CELL_LIMIT))
                .collect()
        })
        .collect();
    let widths: Vec<usize> = (0..columns.len())
        .map(|k| {
            cells
                .iter()
                .map(|r| r[k].chars().count())
                .chain(std::iter::once(columns[k].chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let rule = format!(
        "+{}+",
        widths
            .iter()
            .map(|w| "-".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("+")
    );
    let line = |texts: &[String], header: bool| {
        let parts: Vec<String> = texts
            .iter()
            .enumerate()
            .map(|(k, t)| {
                let pad = widths[k] - t.chars().count();
                if !header && numeric.get(k).copied().unwrap_or(false) {
                    format!(" {}{t} ", " ".repeat(pad))
                } else {
                    format!(" {t}{} ", " ".repeat(pad))
                }
            })
            .collect();
        format!("|{}|", parts.join("|"))
    };
    let mut out = vec![rule.clone(), line(columns, true), rule.clone()];
    out.extend(cells.iter().map(|r| line(r, false)));
    out.push(rule);
    out.join("\n")
}

/// One line of the tree view (v3 behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeLine {
    /// 0 for a row, 1 for one of its columns.
    pub depth: usize,
    /// The shown row's position on the page (row lines and their columns).
    pub row: usize,
    /// The column of a column line.
    pub column: Option<usize>,
    /// The text: `3 {id: 7, sku: A-1, …}` for a row, `sku: A-1` for a column.
    pub text: String,
    /// Whether a row line is expanded.
    pub expanded: bool,
}

/// The tree view of `rows` (v3 behaviour 10): one node per row, numbered from `first + 1`, with
/// its first values as a summary; an expanded row shows one child per column.
pub fn tree_view(
    columns: &[String],
    rows: &[&[Option<String>]],
    first: usize,
    expanded: &BTreeSet<usize>,
) -> Vec<TreeLine> {
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let shown = expanded.contains(&i);
        let mut summary: Vec<String> = columns
            .iter()
            .enumerate()
            .take(3)
            .map(|(k, c)| format!("{c}: {}", cell_text(&r.get(k).cloned().flatten(), 24)))
            .collect();
        if columns.len() > 3 {
            summary.push("…".into());
        }
        out.push(TreeLine {
            depth: 0,
            row: i,
            column: None,
            text: format!("{} {{{}}}", first + i + 1, summary.join(", ")),
            expanded: shown,
        });
        if shown {
            out.extend(columns.iter().enumerate().map(|(k, c)| TreeLine {
                depth: 1,
                row: i,
                column: Some(k),
                text: format!(
                    "{c}: {}",
                    cell_text(&r.get(k).cloned().flatten(), TEXT_CELL_LIMIT)
                ),
                expanded: false,
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0099 v3 AK-6: the value list counts every loaded row; checking and "only" filter rows,
    /// together with the text filter; a filter keeping every value disappears.
    #[test]
    fn value_filters_per_column() {
        let r = rows(&[
            &[Some("1"), Some("A-1")],
            &[Some("2"), Some("B-2")],
            &[Some("3"), Some("A-1")],
            &[Some("4"), None],
        ]);
        let values = column_values(&r, 1);
        assert_eq!(
            values,
            [
                (None, 1),
                (Some("A-1".to_owned()), 2),
                (Some("B-2".to_owned()), 1)
            ]
        );
        let all: Vec<_> = values.iter().map(|(v, _)| v.clone()).collect();
        let mut s = GridState {
            page: 2,
            ..GridState::default()
        };
        s.toggle_value(1, Some("A-1".into()), &all);
        assert_eq!(s.page, 0);
        assert!(s.filters(1) && !s.checked(1, &Some("A-1".into())));
        assert_eq!(visible(&r, &s), [1, 3]);
        s.toggle_value(1, None, &all);
        assert_eq!(visible(&r, &s), [1]);
        s.only_value(1, Some("A-1".into()));
        assert_eq!(visible(&r, &s), [0, 2]);
        s.filter = "3".into();
        assert_eq!(visible(&r, &s), [2]);
        s.filter.clear();
        s.toggle_value(1, None, &all);
        s.toggle_value(1, Some("B-2".into()), &all);
        assert!(!s.filters(1), "every value checked removes the filter");
        s.only_value(0, Some("4".into()));
        s.clear_values(None);
        assert_eq!(visible(&r, &s).len(), 4);
    }

    /// SPEC-0099 v3 AK-7: the text view aligns numbers right, shows NULL and cuts long values; the
    /// tree view numbers rows from the page start and expands one row into its columns.
    #[test]
    fn text_and_tree_views() {
        let columns = vec!["id".to_owned(), "note".to_owned()];
        let long = "x".repeat(80);
        let r = rows(&[
            &[Some("7"), Some("a\nb")],
            &[Some("12"), None],
            &[Some("3"), Some(&long)],
        ]);
        let refs: Vec<&[Option<String>]> = r.iter().map(Vec::as_slice).collect();
        let text = text_view(&columns, &refs, &[true, false]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], format!("| id | note{} |", " ".repeat(56)));
        assert!(lines[3].starts_with("|  7 | a⏎b "), "{}", lines[3]);
        assert!(lines[4].starts_with("| 12 | <null> "));
        assert!(lines[5].contains(&format!("{}…", "x".repeat(59))));
        assert_eq!(lines.len(), 7);
        assert!(lines[0].starts_with("+----+--") && lines[6] == lines[0]);

        let open: BTreeSet<usize> = [1].into_iter().collect();
        let tree = tree_view(&columns, &refs, 100, &open);
        assert_eq!(tree[0].text, "101 {id: 7, note: a⏎b}");
        assert_eq!(tree[1].text, "102 {id: 12, note: <null>}");
        assert!(tree[1].expanded);
        assert_eq!(
            (tree[2].depth, tree[2].column, tree[2].text.as_str()),
            (1, Some(0), "id: 12")
        );
        assert_eq!(tree.len(), 5);
        assert_eq!(ResultView::parse("TEXT"), Some(ResultView::Text));
        assert_eq!(ResultView::parse("chart"), None);
    }

    fn rows(cells: &[&[Option<&str>]]) -> Vec<Vec<Option<String>>> {
        cells
            .iter()
            .map(|r| r.iter().map(|c| c.map(str::to_owned)).collect())
            .collect()
    }

    #[test]
    fn header_clicks_cycle_and_add_keys() {
        let mut s = GridState {
            page: 3,
            ..GridState::default()
        };
        s.click_header(1, false);
        assert_eq!(s.sort, [(1, Direction::Asc)]);
        assert_eq!(s.page, 0);
        s.click_header(1, false);
        assert_eq!(s.sort, [(1, Direction::Desc)]);
        s.click_header(0, true);
        assert_eq!(s.sort, [(1, Direction::Desc), (0, Direction::Asc)]);
        assert_eq!(s.mark(0).as_deref(), Some("▲2"));
        s.click_header(1, true);
        assert_eq!(s.sort, [(0, Direction::Asc)], "descending → removed");
        assert_eq!(s.mark(0).as_deref(), Some("▲"));
        s.click_header(2, false);
        s.click_header(2, false);
        s.click_header(2, false);
        assert!(s.sort.is_empty());
    }

    #[test]
    fn sorting_is_numeric_null_first_and_stable() {
        let data = rows(&[
            &[Some("10"), Some("b")],
            &[Some("9"), Some("a")],
            &[None, Some("c")],
            &[Some("9"), Some("A")],
            &[Some("x"), Some("d")],
        ]);
        let mut s = GridState::default();
        s.click_header(0, false);
        assert_eq!(
            visible(&data, &s),
            [2, 1, 3, 0, 4],
            "NULL, 9, 9 (stable), 10, text"
        );
        s.click_header(0, false);
        assert_eq!(visible(&data, &s), [4, 0, 1, 3, 2]);
        s.sort = vec![(1, Direction::Asc)];
        assert_eq!(visible(&data, &s), [3, 1, 0, 2, 4], "A before a");
    }

    #[test]
    fn filter_and_pages() {
        let data = rows(&[&[Some("Alice")], &[None], &[Some("bob")], &[Some("ALAN")]]);
        let s = GridState {
            filter: " al ".into(),
            ..GridState::default()
        };
        assert_eq!(visible(&data, &s), [0, 3]);
        assert_eq!(page_label(2, 4, 0), "2 rows (filtered from 4)");
        assert_eq!(pages(0), 1);
        assert_eq!(pages(1200), 3);
        assert_eq!(page_range(1200, 2), 1000..1200);
        assert_eq!(
            page_range(1200, 9),
            1000..1200,
            "past the end shows the last page"
        );
        assert_eq!(page_label(1200, 1200, 1), "501–1000 of 1200");
        assert_eq!(page_label(1, 1, 0), "1 row");
    }

    #[test]
    fn page_sizes() {
        let mut s = GridState {
            page: 3,
            ..GridState::default()
        };
        assert_eq!(s.size(), PAGE_SIZE);
        s.set_page_size(100);
        assert_eq!((s.size(), s.page), (100, 0), "a new size starts at page 1");
        assert_eq!(pages_of(10_000, 100), 100);
        assert_eq!(page_range_of(10_000, 2, 100), 200..300);
        assert_eq!(
            page_label_of(10_000, 10_000, 99, 100),
            "9901–10000 of 10000"
        );
        assert_eq!(pages_of(10_000, ALL_ROWS), 1);
        assert_eq!(page_range_of(10_000, 5, ALL_ROWS), 0..10_000);
        assert_eq!(page_label_of(10_000, 10_000, 0, ALL_ROWS), "10000 rows");
        assert_eq!(page_size_label(ALL_ROWS), "All");
        assert_eq!(page_size_label(PAGE_SIZE), "500 (default)");
        assert_eq!(page_size_label(10), "10");
        assert_eq!(PAGE_SIZES.len(), 6);
    }

    #[test]
    fn numbers_and_numeric_columns() {
        for n in ["-9", "0", "25", "0.25", "1e3", "+7", " 12 ", "6.02E-23"] {
            assert!(is_number(n), "{n}");
        }
        for t in [
            "",
            "A-1",
            "1.2.3",
            "NaN",
            "inf",
            "2026-01-01",
            "1e",
            "-",
            "0x1f",
            "12 a",
        ] {
            assert!(!is_number(t), "{t}");
        }
        let data = rows(&[
            &[Some("1"), Some("A-1"), None, Some("-9")],
            &[Some("2"), Some("B-2"), None, None],
        ]);
        assert_eq!(
            numeric_columns(4, &data),
            [true, false, false, true],
            "an all-NULL column is not numeric"
        );
    }

    #[test]
    fn transposed_view() {
        let data = rows(&[&[Some("1"), Some("pen")], &[Some("2"), None]]);
        let refs: Vec<&[Option<String>]> = data.iter().map(Vec::as_slice).collect();
        let t = transpose(&["id".into(), "name".into()], &refs);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].0, "id");
        assert_eq!(t[0].1, [Some("1".to_owned()), Some("2".to_owned())]);
        assert_eq!(t[1].1, [Some("pen".to_owned()), None]);
    }

    /// SPEC-0099 v4 AK-10: a reload of the same query keeps sort, filters and page.
    #[test]
    fn a_reload_keeps_the_view() {
        let old = GridState {
            sort: vec![(3, Direction::Asc)],
            filter: "ber".into(),
            page: 2,
            page_size: 10,
            values: BTreeMap::from([(2, BTreeSet::from([Some("Berlin".to_owned())]))]),
        };
        assert_eq!(kept_view(&old, true, 120), old);
        assert_eq!(kept_view(&old, true, 15).page, 1);
        assert_eq!(kept_view(&old, false, 120), GridState::default());
    }
}
