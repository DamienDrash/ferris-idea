//! Exporting result data (`SPEC-0021`): the formats for files, the clipboard and agents.

use std::str::FromStr;

use serde_json::{Map, Value};

use crate::{Dialect, literal, quote_identifier};

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// RFC 4180 comma-separated values.
    Csv,
    /// Tab-separated values with backslash escapes.
    Tsv,
    /// Pipe-separated values (v2 behaviour 7).
    Psv,
    /// Array of row objects.
    Json,
    /// Pipe table.
    Markdown,
    /// HTML table (v2 behaviour 8).
    Html,
    /// One element per row (v2 behaviour 8).
    Xml,
    /// One `INSERT` statement per row.
    SqlInsert,
    /// One `UPDATE` statement per row (v2 behaviour 9).
    SqlUpdate,
    /// One condition per row, joined by `OR` (v2 behaviour 9).
    WhereClause,
}

impl ExportFormat {
    /// Every format, in the order the copy list shows them.
    pub const ALL: [ExportFormat; 10] = [
        Self::Tsv,
        Self::Csv,
        Self::Psv,
        Self::Markdown,
        Self::Json,
        Self::Html,
        Self::Xml,
        Self::SqlInsert,
        Self::SqlUpdate,
        Self::WhereClause,
    ];

    /// The name agents and settings use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Psv => "psv",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Xml => "xml",
            Self::SqlInsert => "sql-insert",
            Self::SqlUpdate => "sql-update",
            Self::WhereClause => "where-clause",
        }
    }

    /// The name shown in the copy list.
    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Tsv => "TSV",
            Self::Psv => "Pipe-separated",
            Self::Json => "JSON",
            Self::Markdown => "Markdown",
            Self::Html => "HTML table",
            Self::Xml => "XML",
            Self::SqlInsert => "SQL INSERT",
            Self::SqlUpdate => "SQL UPDATE",
            Self::WhereClause => "WHERE clause",
        }
    }

    /// File extension for exports.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Psv => "psv",
            Self::Json => "json",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Xml => "xml",
            Self::SqlInsert | Self::SqlUpdate | Self::WhereClause => "sql",
        }
    }

    /// Whether the header setting applies (behaviour 1).
    pub fn has_header(self) -> bool {
        matches!(
            self,
            Self::Csv | Self::Tsv | Self::Psv | Self::Markdown | Self::Html
        )
    }

    /// Whether the format needs a table name.
    pub fn needs_table(self) -> bool {
        matches!(self, Self::SqlInsert | Self::SqlUpdate)
    }
}

impl FromStr for ExportFormat {
    type Err = ExportError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|f| f.name() == s)
            .ok_or_else(|| ExportError::UnknownFormat(s.into()))
    }
}

/// Why nothing was exported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    /// Not one of the formats.
    #[error(
        "unknown format `{0}` (csv, tsv, psv, json, markdown, html, xml, sql-insert, sql-update, where-clause)"
    )]
    UnknownFormat(String),
    /// `INSERT` and `UPDATE` statements need a table.
    #[error("sql-insert and sql-update need a table name")]
    MissingTable,
}

/// How to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOptions {
    /// Output format.
    pub format: ExportFormat,
    /// Write column names first, for every format with a header row (IMP-0061).
    pub header: bool,
    /// Table for `INSERT` and `UPDATE` statements.
    pub table: Option<String>,
    /// Key columns for `UPDATE` statements; empty uses the first column.
    pub key: Vec<String>,
    /// Dialect for SQL output.
    pub dialect: Dialect,
}

impl ExportOptions {
    /// `format` with the header on, no table and no key.
    pub fn new(format: ExportFormat, dialect: Dialect) -> Self {
        Self {
            format,
            header: true,
            table: None,
            key: Vec::new(),
            dialect,
        }
    }
}

type Row = Vec<Option<String>>;

/// Exports `columns` and `rows` (`SPEC-0021`).
pub fn export(
    columns: &[String],
    rows: &[Row],
    options: &ExportOptions,
) -> Result<String, ExportError> {
    let header = options.header;
    let d = options.dialect;
    Ok(match options.format {
        ExportFormat::Csv | ExportFormat::Psv => {
            let (sep, end) = if options.format == ExportFormat::Csv {
                (',', "\r\n")
            } else {
                ('|', "\n")
            };
            let mut out = String::new();
            let mut line = |fields: Vec<String>| {
                out.push_str(&fields.join(&sep.to_string()));
                out.push_str(end);
            };
            if header {
                line(columns.iter().map(|c| csv_field(Some(c), sep)).collect());
            }
            for row in rows {
                line(row.iter().map(|v| csv_field(v.as_deref(), sep)).collect());
            }
            out
        }
        ExportFormat::Tsv => {
            let mut out = String::new();
            if header {
                let names: Vec<String> = columns.iter().map(|c| tsv_field(Some(c))).collect();
                out.push_str(&names.join("\t"));
                out.push('\n');
            }
            for row in rows {
                let fields: Vec<String> = row.iter().map(|v| tsv_field(v.as_deref())).collect();
                out.push_str(&fields.join("\t"));
                out.push('\n');
            }
            out
        }
        ExportFormat::Json => {
            let keys = unique_names(columns);
            let array: Vec<Value> = rows
                .iter()
                .map(|row| {
                    let mut object = Map::new();
                    for (key, value) in keys.iter().zip(row) {
                        object.insert(
                            key.clone(),
                            value.clone().map_or(Value::Null, Value::String),
                        );
                    }
                    Value::Object(object)
                })
                .collect();
            let mut text = serde_json::to_string_pretty(&array).expect("strings serialise");
            text.push('\n');
            text
        }
        ExportFormat::Markdown => {
            let cells = |values: Vec<String>| format!("| {} |\n", values.join(" | "));
            let mut out = String::new();
            if header {
                out.push_str(&cells(
                    columns.iter().map(|c| markdown_cell(Some(c))).collect(),
                ));
                out.push_str(&cells(
                    (0..columns.len())
                        .map(|k| {
                            if numeric_column(rows, k) {
                                "---:".to_owned()
                            } else {
                                "---".to_owned()
                            }
                        })
                        .collect(),
                ));
            }
            for row in rows {
                out.push_str(&cells(
                    row.iter().map(|v| markdown_cell(v.as_deref())).collect(),
                ));
            }
            out
        }
        ExportFormat::Html => {
            let mut out = String::from("<table>\n");
            if header {
                out.push_str("  <thead>\n    <tr>");
                for c in columns {
                    out.push_str(&format!("<th>{}</th>", escape_markup(c)));
                }
                out.push_str("</tr>\n  </thead>\n");
            }
            out.push_str("  <tbody>\n");
            for row in rows {
                out.push_str("    <tr>");
                for v in row {
                    match v {
                        Some(v) => out.push_str(&format!("<td>{}</td>", escape_markup(v))),
                        None => out.push_str("<td class=\"null\"></td>"),
                    }
                }
                out.push_str("</tr>\n");
            }
            out.push_str("  </tbody>\n</table>\n");
            out
        }
        ExportFormat::Xml => {
            let names: Vec<String> = unique_names(columns).iter().map(|c| xml_name(c)).collect();
            let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rows>\n");
            for row in rows {
                out.push_str("  <row>\n");
                for (name, v) in names.iter().zip(row) {
                    match v {
                        Some(v) => {
                            out.push_str(&format!("    <{name}>{}</{name}>\n", escape_markup(v)))
                        }
                        None => out.push_str(&format!("    <{name} null=\"true\"/>\n")),
                    }
                }
                out.push_str("  </row>\n");
            }
            out.push_str("</rows>\n");
            out
        }
        ExportFormat::SqlInsert => {
            let table = options.table.as_deref().ok_or(ExportError::MissingTable)?;
            let names: Vec<String> = columns.iter().map(|c| quote_identifier(c, d)).collect();
            let prefix = format!(
                "INSERT INTO {} ({}) VALUES (",
                quote_identifier(table, d),
                names.join(", ")
            );
            let mut out = String::new();
            for row in rows {
                let values: Vec<String> = row.iter().map(|v| literal(v, d)).collect();
                out.push_str(&prefix);
                out.push_str(&values.join(", "));
                out.push_str(");\n");
            }
            out
        }
        ExportFormat::SqlUpdate => {
            let table = options.table.as_deref().ok_or(ExportError::MissingTable)?;
            let key: Vec<usize> = if options.key.is_empty() {
                (0..columns.len().min(1)).collect()
            } else {
                columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| options.key.contains(c))
                    .map(|(k, _)| k)
                    .collect()
            };
            let mut out = String::new();
            for row in rows {
                let set: Vec<String> = (0..columns.len())
                    .filter(|k| !key.contains(k))
                    .map(|k| {
                        format!(
                            "{} = {}",
                            quote_identifier(&columns[k], d),
                            literal(&row.get(k).cloned().flatten(), d)
                        )
                    })
                    .collect();
                let condition: Vec<String> = key
                    .iter()
                    .map(|&k| condition(&columns[k], &row.get(k).cloned().flatten(), d))
                    .collect();
                out.push_str(&format!(
                    "UPDATE {} SET {} WHERE {};\n",
                    quote_identifier(table, d),
                    set.join(", "),
                    condition.join(" AND ")
                ));
            }
            out
        }
        ExportFormat::WhereClause => {
            let rows: Vec<String> = rows
                .iter()
                .map(|row| {
                    let parts: Vec<String> = columns
                        .iter()
                        .zip(row)
                        .map(|(c, v)| condition(c, v, d))
                        .collect();
                    format!("({})", parts.join(" AND "))
                })
                .collect();
            format!("{}\n", rows.join("\n OR "))
        }
    })
}

/// `column = literal`, or `column IS NULL`.
fn condition(column: &str, value: &Option<String>, d: Dialect) -> String {
    match value {
        Some(_) => format!("{} = {}", quote_identifier(column, d), literal(value, d)),
        None => format!("{} IS NULL", quote_identifier(column, d)),
    }
}

/// Whether every non-NULL value of column `k` is a number, and one exists (IMP-0061).
fn numeric_column(rows: &[Row], k: usize) -> bool {
    let mut any = false;
    for v in rows.iter().filter_map(|r| r.get(k)).flatten() {
        if v.trim().parse::<f64>().is_err() || v.trim() != v || v.is_empty() {
            return false;
        }
        any = true;
    }
    any
}

fn escape_markup(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// An XML element name from a column name: characters other than letters, digits, `_`, `-` and
/// `.` become `_`, and a name not starting with a letter or `_` gets a leading `_`.
fn xml_name(c: &str) -> String {
    let mut name: String = c
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || "_-.".contains(ch) {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if !name.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') {
        name.insert(0, '_');
    }
    name
}

fn csv_field(value: Option<&str>, sep: char) -> String {
    let Some(v) = value else {
        return String::new();
    };
    let needs_quotes = v.is_empty()
        || v.contains([sep, '"', '\r', '\n'])
        || v.starts_with(' ')
        || v.ends_with(' ');
    if needs_quotes {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_owned()
    }
}

fn tsv_field(value: Option<&str>) -> String {
    let Some(v) = value else {
        return "\\N".into();
    };
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn markdown_cell(value: Option<&str>) -> String {
    value
        .unwrap_or_default()
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

/// Column names made unique with `_2`, `_3`, … suffixes.
fn unique_names(columns: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(columns.len());
    for c in columns {
        let mut name = c.clone();
        let mut n = 2;
        while out.contains(&name) {
            name = format!("{c}_{n}");
            n += 1;
        }
        out.push(name);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(format: ExportFormat) -> ExportOptions {
        ExportOptions::new(format, Dialect::PostgreSql)
    }

    fn data() -> (Vec<String>, Vec<Row>) {
        let columns = vec!["id".to_owned(), "note".to_owned()];
        let rows = vec![
            vec![Some("1".into()), Some("a, \"b\"".into())],
            vec![Some("2".into()), None],
            vec![Some("3".into()), Some(String::new())],
            vec![Some("4".into()), Some(" x\ny|z\t\\".into())],
        ];
        (columns, rows)
    }

    #[test]
    fn csv_keeps_null_and_empty_apart() {
        let (c, r) = data();
        assert_eq!(
            export(&c, &r, &options(ExportFormat::Csv)).unwrap(),
            "id,note\r\n1,\"a, \"\"b\"\"\"\r\n2,\r\n3,\"\"\r\n4,\" x\ny|z\t\\\"\r\n"
        );
        let mut no_header = options(ExportFormat::Csv);
        no_header.header = false;
        assert!(export(&c, &r, &no_header).unwrap().starts_with("1,"));
    }

    #[test]
    fn tsv_markdown_and_json() {
        let (c, r) = data();
        assert_eq!(
            export(&c, &r, &options(ExportFormat::Tsv)).unwrap(),
            "id\tnote\n1\ta, \"b\"\n2\t\\N\n3\t\n4\t x\\ny|z\\t\\\\\n"
        );
        assert_eq!(
            export(&c, &r, &options(ExportFormat::Markdown)).unwrap(),
            "| id | note |\n| ---: | --- |\n| 1 | a, \"b\" |\n| 2 |  |\n| 3 |  |\n| 4 |  x<br>y\\|z\t\\ |\n"
        );
        let json: Value =
            serde_json::from_str(&export(&c, &r, &options(ExportFormat::Json)).unwrap()).unwrap();
        assert_eq!(json[1], serde_json::json!({"id": "2", "note": null}));
        assert_eq!(json[2]["note"], "");
        let dup = export(
            &["a".into(), "a".into(), "a".into()],
            &[vec![Some("1".into()), Some("2".into()), None]],
            &options(ExportFormat::Json),
        )
        .unwrap();
        let dup: Value = serde_json::from_str(&dup).unwrap();
        assert_eq!(
            dup[0],
            serde_json::json!({"a": "1", "a_2": "2", "a_3": null})
        );
    }

    #[test]
    fn sql_insert_per_dialect() {
        let (c, r) = data();
        let mut o = options(ExportFormat::SqlInsert);
        assert_eq!(export(&c, &r, &o), Err(ExportError::MissingTable));
        o.table = Some("t".into());
        let pg = export(&c, &r[..2], &o).unwrap();
        assert_eq!(
            pg,
            "INSERT INTO \"t\" (\"id\", \"note\") VALUES ('1', 'a, \"b\"');\nINSERT INTO \"t\" (\"id\", \"note\") VALUES ('2', NULL);\n"
        );
        o.dialect = Dialect::MySql;
        let my = export(&c, &r[3..], &o).unwrap();
        assert_eq!(
            my,
            "INSERT INTO `t` (`id`, `note`) VALUES ('4', ' x\ny|z\t\\\\');\n"
        );
        assert_eq!("tsv".parse::<ExportFormat>(), Ok(ExportFormat::Tsv));
        assert!("xlsx".parse::<ExportFormat>().is_err());
    }
    #[test]
    fn one_header_setting_and_numeric_markdown() {
        let columns = vec!["id".to_owned(), "name".to_owned(), "price".to_owned()];
        let rows = vec![
            vec![Some("1".into()), Some("a_b".into()), Some("2.50".into())],
            vec![Some("2".into()), Some("7".into()), None],
        ];
        let md = export(&columns, &rows, &options(ExportFormat::Markdown)).unwrap();
        assert_eq!(
            md,
            "| id | name | price |\n| ---: | --- | ---: |\n| 1 | a_b | 2.50 |\n| 2 | 7 |  |\n"
        );
        for format in ExportFormat::ALL.into_iter().filter(|f| f.has_header()) {
            let mut o = options(format);
            assert!(
                export(&columns, &rows, &o).unwrap().contains("price"),
                "{format:?}"
            );
            o.header = false;
            assert!(
                !export(&columns, &rows, &o).unwrap().contains("price"),
                "{format:?}"
            );
        }
        let mut o = options(ExportFormat::Markdown);
        o.header = false;
        assert!(!export(&columns, &rows, &o).unwrap().contains("---"));
    }

    #[test]
    fn psv_html_and_xml() {
        let (c, r) = data();
        assert_eq!(
            export(&c, &r[..3], &options(ExportFormat::Psv)).unwrap(),
            "id|note\n1|\"a, \"\"b\"\"\"\n2|\n3|\"\"\n"
        );
        let html = export(
            &c,
            &[vec![Some("<1>".into()), None]],
            &options(ExportFormat::Html),
        )
        .unwrap();
        assert!(html.contains("<th>id</th><th>note</th>"));
        assert!(html.contains("<tr><td>&lt;1&gt;</td><td class=\"null\"></td></tr>"));
        let xml = export(
            &["a b".into(), "1x".into()],
            &[vec![Some("x&y".into()), None]],
            &options(ExportFormat::Xml),
        )
        .unwrap();
        assert!(xml.contains("<a_b>x&amp;y</a_b>"));
        assert!(xml.contains("<_1x null=\"true\"/>"));
        assert_eq!("psv".parse::<ExportFormat>(), Ok(ExportFormat::Psv));
        for f in ExportFormat::ALL {
            assert_eq!(f.name().parse::<ExportFormat>(), Ok(f));
        }
    }

    #[test]
    fn sql_update_and_where_clause() {
        let (c, r) = data();
        let mut o = options(ExportFormat::SqlUpdate);
        assert_eq!(export(&c, &r, &o), Err(ExportError::MissingTable));
        o.table = Some("t".into());
        assert_eq!(
            export(&c, &r[1..2], &o).unwrap(),
            "UPDATE \"t\" SET \"note\" = NULL WHERE \"id\" = '2';\n"
        );
        o.key = vec!["note".into()];
        assert_eq!(
            export(&c, &r[1..2], &o).unwrap(),
            "UPDATE \"t\" SET \"id\" = '2' WHERE \"note\" IS NULL;\n"
        );
        o.format = ExportFormat::WhereClause;
        o.dialect = Dialect::MySql;
        assert_eq!(
            export(&c, &r[..2], &o).unwrap(),
            "(`id` = '1' AND `note` = 'a, \"b\"')\n OR (`id` = '2' AND `note` IS NULL)\n"
        );
    }
}
