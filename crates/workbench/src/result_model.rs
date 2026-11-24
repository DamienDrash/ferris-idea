//! What the result grid shows besides its cells (`SPEC-0190`, `SPEC-0021` v2): the record and
//! value views of the side panel, the rectangular selection and the text it copies as.

use std::ops::Range;

pub use ferris_idea_database::ExportFormat;
use ferris_idea_database::{Dialect, ExportOptions, export, summarize_loaded};

use crate::sql_console_model::NULL_TEXT;

/// Shown for a column left to its default in an added row (behaviour 8).
pub const DEFAULT_TEXT: &str = "<default>";

/// The side panel's views (behaviour 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideView {
    /// The selected row, one line per column.
    #[default]
    Record,
    /// The selected cell's full text.
    Value,
    /// Distinct values of the selected column.
    Summary,
}

impl SideView {
    /// Every view, in tab order.
    pub const ALL: [SideView; 3] = [Self::Record, Self::Value, Self::Summary];

    /// The tab's title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Record => "Record",
            Self::Value => "Value",
            Self::Summary => "Summary",
        }
    }
}

/// The record view: column name and shown value per column (behaviour 6).
pub fn record_lines(columns: &[String], values: &[Option<String>]) -> Vec<(String, String)> {
    columns
        .iter()
        .enumerate()
        .map(|(k, name)| {
            let value = values.get(k).cloned().flatten();
            (name.clone(), value.unwrap_or_else(|| NULL_TEXT.to_owned()))
        })
        .collect()
}

/// The value view: the full text, JSON objects and arrays indented (behaviour 6).
pub fn value_text(value: &Option<String>) -> String {
    let Some(v) = value else {
        return NULL_TEXT.to_owned();
    };
    let t = v.trim_start();
    if (t.starts_with('{') || t.starts_with('['))
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(v)
        && let Ok(pretty) = serde_json::to_string_pretty(&json)
    {
        return pretty;
    }
    v.clone()
}

/// The summary view: the scope line, then `value — count` lines (behaviour 6).
pub fn summary_lines(values: &[Option<String>], total: Option<u64>) -> (String, Vec<String>) {
    let summary = summarize_loaded(values, total);
    let lines = summary
        .values
        .iter()
        .map(|(v, n)| format!("{} — {n}", v.as_deref().unwrap_or(NULL_TEXT)))
        .collect();
    (format!("Over {}", summary.scope), lines)
}

/// A rectangular selection of the grid: anchor and cursor as (row id, column), where row ids are
/// the grid's (loaded row, or loaded rows + added row index) (behaviour 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: (usize, usize),
    /// The cursor cell.
    pub cursor: (usize, usize),
}

impl Selection {
    /// One cell.
    pub fn cell(row: usize, col: usize) -> Self {
        Self {
            anchor: (row, col),
            cursor: (row, col),
        }
    }

    /// The selected row ids in shown order (`shown` lists the row ids as shown) and columns.
    pub fn rect(&self, shown: &[usize]) -> (Vec<usize>, Range<usize>) {
        let pos = |id: usize| shown.iter().position(|&r| r == id);
        let cols = self.anchor.1.min(self.cursor.1)..self.anchor.1.max(self.cursor.1) + 1;
        let rows = match (pos(self.anchor.0), pos(self.cursor.0)) {
            (Some(a), Some(b)) => shown[a.min(b)..=a.max(b)].to_vec(),
            (None, Some(b)) => vec![shown[b]],
            _ => Vec::new(),
        };
        (rows, cols)
    }

    /// Whether cell (`row`, `col`) is inside, given the shown order.
    pub fn contains(&self, shown: &[usize], row: usize, col: usize) -> bool {
        let (rows, cols) = self.rect(shown);
        cols.contains(&col) && rows.contains(&row)
    }

    /// Whether only one cell is selected.
    pub fn single(&self) -> bool {
        self.anchor == self.cursor
    }
}

/// How copying and exporting are set up (SPEC-0021 v2 behaviours 10–11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopySettings {
    /// Format for ⌘C and the export dialog.
    pub format: ExportFormat,
    /// Header row for every format with one (IMP-0061), on by default.
    pub header: bool,
}

impl Default for CopySettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Tsv,
            header: true,
        }
    }
}

/// The text of `columns` and `rows` in the chosen format; `table` and `key` feed the SQL formats
/// (a missing table falls back to `table` so the text stays useful).
pub fn format_rows(
    columns: &[String],
    rows: &[Vec<Option<String>>],
    settings: &CopySettings,
    table: Option<&str>,
    key: &[String],
    dialect: Dialect,
) -> String {
    let options = ExportOptions {
        format: settings.format,
        header: settings.header,
        table: Some(table.unwrap_or("table").to_owned()),
        key: key.to_vec(),
        dialect,
    };
    export(columns, rows, &options).unwrap_or_else(|e| e.to_string())
}

/// What ⌘C copies (SPEC-0021 v2 behaviour 10): a single cell as its plain value (NULL as empty
/// text), several cells in the chosen format with the selected columns.
#[allow(clippy::too_many_arguments)]
pub fn copy_text(
    columns: &[String],
    values: &dyn Fn(usize, usize) -> Option<String>,
    selection: &Selection,
    shown: &[usize],
    settings: &CopySettings,
    table: Option<&str>,
    key: &[String],
    dialect: Dialect,
) -> String {
    if selection.single() {
        let (r, c) = selection.cursor;
        return values(r, c).unwrap_or_default();
    }
    let (rows, cols) = selection.rect(shown);
    let names: Vec<String> =
        columns[cols.start.min(columns.len())..cols.end.min(columns.len())].to_vec();
    let data: Vec<Vec<Option<String>>> = rows
        .iter()
        .map(|&r| cols.clone().map(|c| values(r, c)).collect())
        .collect();
    format_rows(&names, &data, settings, table, key, dialect)
}

/// The file an export goes to by default: `<root>/<name>.<extension>`.
pub fn default_export_path(
    root: &std::path::Path,
    name: &str,
    format: ExportFormat,
) -> std::path::PathBuf {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join(format!("{clean}.{}", format.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_value_and_summary() {
        let columns = vec!["id".to_owned(), "doc".to_owned()];
        assert_eq!(
            record_lines(&columns, &[Some("1".into()), None]),
            vec![
                ("id".to_owned(), "1".to_owned()),
                ("doc".to_owned(), "<null>".to_owned())
            ]
        );
        assert_eq!(
            value_text(&Some("{\"a\":[1,2]}".into())),
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
        assert_eq!(value_text(&Some("{not json".into())), "{not json");
        assert_eq!(value_text(&Some("12".into())), "12");
        assert_eq!(value_text(&None), "<null>");
        let (scope, lines) = summary_lines(&[Some("a".into()), None, Some("a".into())], Some(10));
        assert_eq!(scope, "Over 3 loaded rows of 10");
        assert_eq!(lines, ["a — 2", "<null> — 1"]);
    }

    #[test]
    fn rectangles_follow_the_shown_order() {
        let shown = [2, 0, 1, 5];
        let s = Selection {
            anchor: (0, 2),
            cursor: (5, 1),
        };
        assert_eq!(s.rect(&shown), (vec![0, 1, 5], 1..3));
        assert!(s.contains(&shown, 1, 2));
        assert!(!s.contains(&shown, 2, 1));
        assert!(Selection::cell(1, 1).single());
    }

    #[test]
    fn copying_cells_uses_the_format_and_header() {
        let columns = vec!["id".to_owned(), "name".to_owned(), "n".to_owned()];
        let data = [["1", "Ada", "3"], ["2", "Linus", "10"]];
        let values = |r: usize, c: usize| Some(data[r][c].to_owned());
        let shown = [0, 1];
        let all = Selection {
            anchor: (0, 1),
            cursor: (1, 2),
        };
        let mut settings = CopySettings::default();
        let copy = |s: &Selection, settings: &CopySettings| {
            copy_text(
                &columns,
                &values,
                s,
                &shown,
                settings,
                Some("t"),
                &[],
                Dialect::Sqlite,
            )
        };
        assert_eq!(copy(&all, &settings), "name\tn\nAda\t3\nLinus\t10\n");
        settings.format = ExportFormat::Markdown;
        assert_eq!(
            copy(&all, &settings),
            "| name | n |\n| --- | ---: |\n| Ada | 3 |\n| Linus | 10 |\n"
        );
        settings.header = false;
        assert_eq!(copy(&all, &settings), "| Ada | 3 |\n| Linus | 10 |\n");
        assert_eq!(copy(&Selection::cell(1, 1), &settings), "Linus");
        assert_eq!(
            default_export_path(
                std::path::Path::new("/p"),
                "Result 1",
                ExportFormat::Markdown
            ),
            std::path::PathBuf::from("/p/Result_1.md")
        );
    }
}
