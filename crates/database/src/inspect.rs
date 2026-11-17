//! SQL inspections (`SPEC-0015`, v2: unqualified columns, closest-name fixes, IMP-0060/0062).

use std::ops::Range;

use serde::Serialize;

use crate::completion::{
    CLAUSE_ENDS, KEYWORDS, TABLE_INTRODUCERS, Tok, Token, dialect_keywords, insert_text, tokenize,
};
use crate::{Dialect, Table, split_statements};

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The statement will fail or does something else than written.
    Error,
    /// The statement runs, but probably not as intended.
    Warning,
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// How serious it is.
    pub severity: Severity,
    /// What is wrong and what to do.
    pub message: String,
    /// Byte range in the script.
    pub start: usize,
    /// End of the byte range (exclusive).
    pub end: usize,
    /// The table an unsafe change touches, as written (v2 behaviour 10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// Edits that fix the problem, best first (v2 behaviour 9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
}

impl Problem {
    /// For an unsafe change, the question to ask before running it (`SPEC-0192` behaviour 8):
    /// "Delete all 42 rows of `stock`?", or "… all rows …" when the count is unknown.
    pub fn confirmation(&self, rows: Option<u64>) -> Option<String> {
        let table = self.table.as_deref()?;
        let verb = if self.message.starts_with("DELETE") {
            "Delete"
        } else {
            "Update"
        };
        Some(match rows {
            Some(1) => format!("{verb} the only row of `{table}`?"),
            Some(n) => format!("{verb} all {n} rows of `{table}`?"),
            None => format!("{verb} all rows of `{table}`?"),
        })
    }
}

/// An edit that fixes a problem: replace `start..end` of the script with `replacement`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    /// What the fix does ("Replace with `name`").
    pub label: String,
    /// Start of the replaced byte range.
    pub start: usize,
    /// End of the replaced byte range (exclusive).
    pub end: usize,
    /// New text.
    pub replacement: String,
}

/// Words that may stand where a column could, without being one.
const NOT_COLUMNS: &[&str] = &[
    "true",
    "false",
    "unknown",
    "current_date",
    "current_time",
    "current_timestamp",
    "localtime",
    "localtimestamp",
    "current_user",
    "session_user",
    "user",
    "current_schema",
    "current_catalog",
    "interval",
    "date",
    "time",
    "timestamp",
    "by",
    "nulls",
    "first",
    "last",
    "any",
    "some",
    "cast",
    "collate",
    "escape",
    "ilike",
    "similar",
    "to",
    "row",
    "rows",
    "filter",
    "over",
    "partition",
    "within",
    "array",
    "at",
    "zone",
    "both",
    "leading",
    "trailing",
    "for",
    "default",
    "rollup",
    "cube",
    "grouping",
    "sets",
    "with",
    "ties",
    "fetch",
    "next",
    "only",
    "top",
    "percent",
    "into",
    "natural",
    "cross",
    "full",
    "lateral",
    "rlike",
    "regexp",
    "div",
    "mod",
    "xor",
    "binary",
    "unsigned",
    "signed",
    "integer",
    "int",
    "text",
    "varchar",
    "char",
    "numeric",
    "decimal",
    "boolean",
    "bigint",
    "smallint",
    "real",
    "double",
    "precision",
    "float",
    "rowid",
    "oid",
    "excluded",
    "new",
    "old",
    "glob",
    "match",
    "isnull",
    "notnull",
    "window",
    "range",
    "groups",
    "preceding",
    "following",
    "unbounded",
    "current",
    "row_number",
    "exclude",
    "others",
];

/// Inspects every statement of `script`. `schema` may be empty when unknown;
/// name checks are then skipped.
pub fn inspect(script: &str, dialect: Dialect, schema: &[Table]) -> Vec<Problem> {
    let mut problems = Vec::new();
    // Tables an earlier statement of the script creates, temporary ones included: they are no
    // unknown tables in the later statements, before and after a run (SPEC-0192 v7 behaviour 11b).
    let mut created: Vec<String> = Vec::new();
    for statement in split_statements(script, dialect) {
        let text = &script[statement.range.clone()];
        let Some(tokens) = tokenize(text, 0, dialect) else {
            continue;
        };
        let at = |r: &Range<usize>| statement.range.start + r.start..statement.range.start + r.end;
        let word = |t: &Token| text[t.range.clone()].to_ascii_lowercase();
        let words: Vec<String> = tokens
            .iter()
            .map(|t| {
                if t.kind == Tok::Word {
                    word(t)
                } else {
                    String::new()
                }
            })
            .collect();
        let first = words.first().map(String::as_str).unwrap_or("");

        if matches!(first, "update" | "delete") && !words.iter().any(|w| w == "where") {
            let table = changed_table(text, &tokens, &words);
            let what = match first {
                "delete" => "DELETE without WHERE removes every row of",
                _ => "UPDATE without WHERE changes every row of",
            };
            let r = at(&tokens[0].range);
            problems.push(Problem {
                severity: Severity::Warning,
                message: format!(
                    "{what} `{}`; add a WHERE clause (or `WHERE true` to make the intent explicit)",
                    table.as_deref().unwrap_or("the table")
                ),
                start: r.start,
                end: r.end,
                table,
                fixes: Vec::new(),
            });
        }

        let mut push = |severity, message: String, r: Range<usize>, names: Vec<String>| {
            let r = at(&r);
            let fixes = names
                .into_iter()
                .map(|n| Fix {
                    label: format!("Replace with `{n}`"),
                    start: r.start,
                    end: r.end,
                    replacement: insert_text(&n, dialect),
                })
                .collect();
            problems.push(Problem {
                severity,
                message,
                start: r.start,
                end: r.end,
                table: None,
                fixes,
            });
        };
        if dialect == Dialect::MySql
            && first == "create"
            && words.get(1).is_some_and(|w| w == "table")
        {
            for (column, references) in ignored_references(text, &tokens) {
                push(
                    Severity::Warning,
                    format!(
                        "MySQL ignores REFERENCES in the definition of column `{column}`; no foreign key is created. Use a table-level `FOREIGN KEY ({column}) REFERENCES …` instead"
                    ),
                    references,
                    Vec::new(),
                );
            }
        }

        if first == "create" {
            created.extend(created_table(&tokens, &words));
        }
        if schema.is_empty() || matches!(first, "create" | "alter" | "drop") {
            continue;
        }
        let mut ctes = cte_names(&tokens, &words);
        let with_columns = cte_columns(text, &tokens, &words);
        // A WITH query with a column list (`big(id, body) as (…)`) is one too.
        for (name, _) in &with_columns {
            if !ctes.contains(name) {
                ctes.push(name.clone());
            }
        }
        let (refs, opaque) = references(&tokens, &words, text);
        let mut all_known = true;
        for (name, range, _) in &refs {
            // Oracle's and MySQL's one-row table is no table of the schema (`SPEC-0015` v5
            // behaviour 13, OBS-0083 L4); its columns are not checked.
            if dialect_table(dialect, name) {
                all_known = false;
                continue;
            }
            let known = schema.iter().any(|t| t.name.eq_ignore_ascii_case(name))
                || ctes.iter().any(|c| c == name)
                || created.iter().any(|c| c.eq_ignore_ascii_case(name));
            if !known {
                all_known = false;
                push(
                    Severity::Error,
                    format!("unknown table `{name}`"),
                    range.clone(),
                    closest(name, schema.iter().map(|t| t.name.as_str())),
                );
            }
        }
        // q.column
        for i in 0..tokens.len().saturating_sub(2) {
            let (q, dot, col) = (&tokens[i], &tokens[i + 1], &tokens[i + 2]);
            if q.kind != Tok::Word || dot.kind != Tok::Dot || col.kind != Tok::Word {
                continue;
            }
            if i > 0 && tokens[i - 1].kind == Tok::Dot {
                continue; // schema.table.column: not resolved here
            }
            let qualifier = &words[i];
            let table_name = refs
                .iter()
                .find(|(_, _, alias)| alias.as_deref() == Some(qualifier.as_str()))
                .map(|(n, _, _)| n.clone())
                .unwrap_or_else(|| qualifier.clone());
            // A WITH query with known result columns (v4 behaviour 12).
            if let Some((query, columns)) = with_columns.iter().find(|(n, _)| *n == table_name) {
                let column = &words[i + 2];
                if let Some(columns) = columns
                    && !columns.iter().any(|c| c.eq_ignore_ascii_case(column))
                {
                    push(
                        Severity::Error,
                        format!("unknown column `{column}` in `{query}`"),
                        col.range.clone(),
                        closest(column, columns.iter().map(String::as_str)),
                    );
                }
                continue;
            }
            let Some(table) = schema
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(&table_name))
            else {
                continue;
            };
            let column = &words[i + 2];
            if !table
                .columns
                .iter()
                .any(|c| c.name.eq_ignore_ascii_case(column))
                && !hides(table, column)
            {
                push(
                    Severity::Error,
                    format!("unknown column `{column}` in `{}`", table.name),
                    col.range.clone(),
                    closest(column, table.columns.iter().map(|c| c.name.as_str())),
                );
            }
        }
        // Bare column names (v2 behaviour 8).
        if first == "select" && all_known && !opaque && ctes.is_empty() && !refs.is_empty() {
            let tables: Vec<&Table> = refs
                .iter()
                .filter_map(|(n, _, _)| schema.iter().find(|t| t.name.eq_ignore_ascii_case(n)))
                .collect();
            for (column, range) in bare_columns(text, &tokens, &words, &refs, dialect) {
                let exists = tables.iter().any(|t| {
                    t.columns
                        .iter()
                        .any(|c| c.name.eq_ignore_ascii_case(&column))
                        || hides(t, &column)
                });
                if !exists {
                    push(
                        Severity::Error,
                        format!("unknown column `{column}`"),
                        range,
                        closest(
                            &column,
                            tables
                                .iter()
                                .flat_map(|t| t.columns.iter().map(|c| c.name.as_str())),
                        ),
                    );
                }
            }
        }
    }
    problems.sort_by_key(|p| (p.start, p.end));
    problems
}

type Reference = (String, Range<usize>, Option<String>);

/// Table references: (name, range of the name, alias), and whether the `FROM` also holds something
/// whose columns are unknown here (a subquery or table function).
fn references(tokens: &[Token], words: &[String], text: &str) -> (Vec<Reference>, bool) {
    let mut out = Vec::new();
    let mut opaque = false;
    let mut in_from = false;
    let mut depth = 0usize;
    let mut from_depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        let w = words[i].as_str();
        match &text[t.range.clone()] {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            _ => {}
        }
        // Only commas of the FROM list itself, not of function arguments in it.
        let continues = t.kind == Tok::Comma && in_from && depth == from_depth;
        if w == "from" || w == "join" {
            in_from = true;
            from_depth = depth;
        } else if CLAUSE_ENDS.contains(&w) {
            in_from = false;
        }
        if !(TABLE_INTRODUCERS.contains(&w) || continues) {
            continue;
        }
        // Last part of a possibly qualified name.
        let mut j = i + 1;
        let mut name = None;
        while let Some(t) = tokens.get(j).filter(|t| t.kind == Tok::Word) {
            name = Some((words[j].clone(), t.range.clone()));
            if tokens.get(j + 1).map(|n| n.kind) == Some(Tok::Dot) {
                j += 2;
            } else {
                j += 1;
                break;
            }
        }
        let Some((name, range)) =
            name.filter(|(n, _)| n.starts_with(|c: char| c.is_alphabetic() || c == '_'))
        else {
            if tokens
                .get(i + 1)
                .is_some_and(|t| &text[t.range.clone()] == "(")
            {
                opaque = true; // subquery
            }
            continue;
        };
        if tokens.get(j).is_some_and(|t| &text[t.range.clone()] == "(") {
            opaque = true; // table function
            continue;
        }
        let mut k = j;
        if words.get(k).is_some_and(|w| w == "as") {
            k += 1;
        }
        let alias = tokens
            .get(k)
            .filter(|t| t.kind == Tok::Word)
            .map(|_| words[k].clone())
            .filter(|a| {
                !CLAUSE_ENDS.contains(&a.as_str())
                    && !crate::completion::KEYWORDS.contains(&a.as_str())
            });
        out.push((name, range, alias));
    }
    (out, opaque)
}

/// The table after `DELETE FROM` / `UPDATE`, as written (with schema and quotes).
fn changed_table(text: &str, tokens: &[Token], words: &[String]) -> Option<String> {
    let keyword = if words.get(1).is_some_and(|w| w == "from") {
        1
    } else {
        0
    };
    let rest = text[tokens.get(keyword)?.range.end..].trim_start();
    let mut end = 0;
    let mut quote = None;
    for (i, c) in rest.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if matches!(c, '"' | '`' | '[') => quote = Some(if c == '[' { ']' } else { c }),
            None if c.is_whitespace() || matches!(c, ';' | '(') => break,
            None => {}
        }
        end = i + c.len_utf8();
    }
    let name = &rest[..end];
    (!name.is_empty() && !matches!(name.to_ascii_lowercase().as_str(), "set" | "where"))
        .then(|| name.to_owned())
}

/// Bare names in the select list, `WHERE`, `GROUP BY`, `HAVING` and `ORDER BY` at the top level of
/// a select that could be columns (v2 behaviour 8), with their ranges.
fn bare_columns(
    text: &str,
    tokens: &[Token],
    words: &[String],
    refs: &[Reference],
    dialect: Dialect,
) -> Vec<(String, Range<usize>)> {
    #[derive(PartialEq)]
    enum Clause {
        Select,
        Checked,
        Skipped,
    }
    let raw = |i: usize| &text[tokens[i].range.clone()];
    // Something skipped (a literal, quoted name or comment) stands right before token `i`.
    let after_gap = |i: usize| {
        i > 0
            && !text[tokens[i - 1].range.end..tokens[i].range.start]
                .trim()
                .is_empty()
    };
    let keyword = |w: &str| {
        KEYWORDS.contains(&w)
            || dialect_keywords(dialect).contains(&w)
            || NOT_COLUMNS.contains(&w)
            || CLAUSE_ENDS.contains(&w)
            || TABLE_INTRODUCERS.contains(&w)
    };
    // Aliases: `as x` anywhere, and a name directly after an expression in the select list.
    let mut aliases: Vec<String> = refs.iter().filter_map(|(_, _, a)| a.clone()).collect();
    aliases.extend(refs.iter().map(|(n, _, _)| n.clone()));
    let mut out = Vec::new();
    let mut candidates = Vec::new();
    let mut clause = Clause::Skipped;
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match raw(i) {
            "(" => {
                depth += 1;
                continue;
            }
            ")" => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if depth > 0 || t.kind != Tok::Word {
            continue;
        }
        let w = words[i].as_str();
        match w {
            "select" => {
                clause = Clause::Select;
                continue;
            }
            "where" | "having" | "group" | "order" => {
                clause = Clause::Checked;
                continue;
            }
            "from" | "join" | "on" | "using" | "limit" | "offset" | "union" | "returning"
            | "window" | "into" | "fetch" => {
                clause = Clause::Skipped;
                continue;
            }
            _ => {}
        }
        if clause == Clause::Skipped || keyword(w) || w.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let prev = i.checked_sub(1).map(|p| (&tokens[p], words[p].as_str()));
        let next = tokens.get(i + 1).map(|n| &text[n.range.clone()]);
        if prev.is_some_and(|(p, pw)| {
            p.kind == Tok::Dot || pw == "as" || text[p.range.clone()].ends_with(':')
        }) {
            if prev.is_some_and(|(_, pw)| pw == "as") {
                aliases.push(w.to_owned());
            }
            continue;
        }
        if matches!(next, Some("(" | ".")) {
            continue;
        }
        let implicit_alias = clause == Clause::Select
            && (after_gap(i)
                || prev.is_some_and(|(p, pw)| {
                    (p.kind == Tok::Word && !keyword(pw)) || matches!(raw(i - 1), ")" | "*")
                }));
        if implicit_alias {
            aliases.push(w.to_owned());
            continue;
        }
        if after_gap(i) {
            continue;
        }
        candidates.push((w.to_owned(), t.range.clone()));
    }
    for (w, r) in candidates {
        if !aliases.iter().any(|a| a == &w) {
            out.push((w, r));
        }
    }
    out
}

/// Up to three existing names closest to `name` (v2 behaviour 9).
fn closest<'a>(name: &str, names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let lower = name.to_lowercase();
    let mut found: Vec<(usize, String)> = Vec::new();
    for n in names {
        let d = distance(&lower, &n.to_lowercase());
        let longer = lower.chars().count().max(n.chars().count());
        let limit = (longer / 3).clamp(1, 3);
        if d > 0 && d <= limit && !found.iter().any(|(_, f)| f == n) {
            found.push((d, n.to_owned()));
        }
    }
    found.sort();
    found.into_iter().take(3).map(|(_, n)| n).collect()
}

/// Levenshtein distance over characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// Names defined by `WITH name AS (` / `, name AS (` in the statement.
fn cte_names(tokens: &[Token], words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i].kind == Tok::Word && words.get(i + 1).is_some_and(|w| w == "as") {
            let prev = i.checked_sub(1).map(|p| words[p].as_str());
            let after_comma = i
                .checked_sub(1)
                .is_some_and(|p| tokens[p].kind == Tok::Comma);
            if prev == Some("with") || prev == Some("recursive") || after_comma {
                out.push(words[i].clone());
            }
        }
    }
    out
}

/// The `WITH` queries of a statement with their result columns where they are known (v4 behaviour
/// 12): the column list, or the name of every item of the first select list (an alias, or the
/// column a reference names, also through a `::` cast). `None` for `*`, an expression without an
/// alias, a quoted name or literal in the list (they are no tokens here) or a query that is no
/// `SELECT`.
fn cte_columns(
    text: &str,
    tokens: &[Token],
    words: &[String],
) -> Vec<(String, Option<Vec<String>>)> {
    let raw = |k: usize| tokens.get(k).map_or("", |t| &text[t.range.clone()]);
    let close_of = |open: usize| {
        let mut depth = 0usize;
        for k in open..tokens.len() {
            match raw(k) {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
        None
    };
    let is_name = |k: usize| tokens.get(k).is_some_and(|t| t.kind == Tok::Word);
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if w != "with" {
            continue;
        }
        let mut j = i + 1;
        if words.get(j).is_some_and(|w| w == "recursive") {
            j += 1;
        }
        while is_name(j) {
            let query = words[j].clone();
            j += 1;
            let mut columns: Option<Vec<String>> = None;
            if raw(j) == "(" {
                let Some(c) = close_of(j) else { break };
                columns = Some(
                    (j + 1..c)
                        .filter(|k| is_name(*k))
                        .map(|k| words[k].clone())
                        .collect(),
                );
                j = c + 1;
            }
            if words.get(j).is_none_or(|w| w != "as") {
                break;
            }
            j += 1;
            while words
                .get(j)
                .is_some_and(|w| w == "not" || w == "materialized")
            {
                j += 1;
            }
            if raw(j) != "(" {
                break;
            }
            let Some(close) = close_of(j) else { break };
            if columns.is_none() {
                columns = select_names(text, tokens, words, j + 1, close);
            }
            out.push((query, columns));
            j = close + 1;
            if raw(j) == "," {
                j += 1;
            } else {
                break;
            }
        }
    }
    out
}

/// The names of the items of the select list that starts at token `from` (a `SELECT`), up to its
/// `FROM` or `end`; `None` when one has no name of its own or something skipped stands in it.
fn select_names(
    text: &str,
    tokens: &[Token],
    words: &[String],
    from: usize,
    end: usize,
) -> Option<Vec<String>> {
    let raw = |k: usize| tokens.get(k).map_or("", |t| &text[t.range.clone()]);
    let is_name = |k: usize| tokens.get(k).is_some_and(|t| t.kind == Tok::Word);
    // A literal, quoted name or comment right before token `k`.
    let gap = |k: usize| {
        k > 0
            && k < tokens.len()
            && !text[tokens[k - 1].range.end..tokens[k].range.start]
                .trim()
                .is_empty()
    };
    if words.get(from).is_none_or(|w| w != "select") {
        return None;
    }
    let mut k = from + 1;
    if words.get(k).is_some_and(|w| w == "distinct" || w == "all") {
        k += 1;
        if words.get(k).is_some_and(|w| w == "on") {
            return None;
        }
    }
    let mut items: Vec<Vec<usize>> = vec![Vec::new()];
    let mut depth = 0usize;
    while k < end {
        let w = words.get(k).map_or("", String::as_str);
        if depth == 0
            && matches!(
                w,
                "from"
                    | "where"
                    | "group"
                    | "having"
                    | "order"
                    | "limit"
                    | "union"
                    | "intersect"
                    | "except"
                    | "into"
                    | "window"
                    | "offset"
                    | "fetch"
            )
        {
            break;
        }
        if gap(k) {
            return None;
        }
        match raw(k) {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                items.push(Vec::new());
                k += 1;
                continue;
            }
            _ => {}
        }
        items.last_mut().expect("an item").push(k);
        k += 1;
    }
    if gap(k) {
        return None;
    }
    let mut names = Vec::new();
    for item in &items {
        let (&first, &last) = (item.first()?, item.last()?);
        let len = item.len();
        // `expr as x` or `expr x` (a name right after the expression).
        if len >= 2 && is_name(last) && !matches!(raw(last - 1), "." | ":") {
            let before = words.get(last - 1).map_or("", String::as_str);
            if before == "as" || is_name(last - 1) || raw(last - 1) == ")" {
                // `case … end` ends in a keyword, not in a name.
                if before != "as" && KEYWORDS.contains(&words[last].as_str()) {
                    return None;
                }
                names.push(words[last].clone());
                continue;
            }
        }
        // `x`, `t.x`, maybe with `::type`.
        let reference_end = item.iter().position(|&k| raw(k) == ":").unwrap_or(len);
        let cast_ok =
            reference_end == len || item.get(reference_end + 1).is_some_and(|&k| raw(k) == ":");
        let reference = &item[..reference_end];
        let plain = match reference.len() {
            1 => is_name(reference[0]),
            3 => is_name(reference[0]) && raw(reference[1]) == "." && is_name(reference[2]),
            _ => false,
        };
        if !plain || !cast_ok || raw(first) == "*" {
            return None;
        }
        names.push(words[*reference.last()?].clone());
    }
    Some(names)
}

/// The table a `CREATE [TEMPORARY | TEMP | GLOBAL TEMPORARY | LOCAL TEMPORARY | UNLOGGED] TABLE
/// [IF NOT EXISTS] [schema.]name` statement creates, without its schema.
fn created_table(tokens: &[Token], words: &[String]) -> Option<String> {
    let mut i = 1;
    while words.get(i).is_some_and(|w| {
        matches!(
            w.as_str(),
            "temporary" | "temp" | "global" | "local" | "unlogged"
        )
    }) {
        i += 1;
    }
    if words.get(i)? != "table" {
        return None;
    }
    i += 1;
    if words.get(i).is_some_and(|w| w == "if") {
        i += 3; // if not exists
    }
    let mut name = None;
    while let Some(t) = tokens.get(i) {
        if t.kind != Tok::Word {
            break;
        }
        name = Some(words[i].clone());
        if tokens.get(i + 1).is_some_and(|d| d.kind == Tok::Dot) {
            i += 2;
        } else {
            break;
        }
    }
    name.filter(|n| !n.is_empty())
}

/// Column definitions of a CREATE TABLE that carry an inline REFERENCES:
/// (column name, range of the REFERENCES keyword).
fn ignored_references(text: &str, tokens: &[Token]) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut element: Vec<usize> = Vec::new();
    let mut flush = |element: &mut Vec<usize>| {
        let lower = |i: usize| text[tokens[i].range.clone()].to_ascii_lowercase();
        if let Some(&first) = element.first() {
            let head = lower(first);
            let is_constraint = matches!(
                head.as_str(),
                "foreign" | "constraint" | "primary" | "unique" | "key" | "index" | "check"
            );
            if !is_constraint
                && tokens[first].kind == Tok::Word
                && let Some(&r) = element
                    .iter()
                    .find(|&&i| tokens[i].kind == Tok::Word && lower(i) == "references")
            {
                out.push((
                    text[tokens[first].range.clone()].to_owned(),
                    tokens[r].range.clone(),
                ));
            }
        }
        element.clear();
    };
    for (i, t) in tokens.iter().enumerate() {
        let s = &text[t.range.clone()];
        match (t.kind, s) {
            (Tok::Other, "(") => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            (Tok::Other, ")") => {
                depth -= 1;
                if depth == 0 {
                    flush(&mut element);
                    continue;
                }
            }
            (Tok::Comma, _) if depth == 1 => {
                flush(&mut element);
                continue;
            }
            _ => {}
        }
        if depth >= 1 {
            element.push(i);
        }
    }
    out
}

/// Whether `table` is a virtual table with the hidden column `column` (SQLite: FTS5's `rank`,
/// `SPEC-0015` v3 behaviour 11).
fn hides(table: &Table, column: &str) -> bool {
    table
        .hidden_columns
        .iter()
        .any(|h| h.eq_ignore_ascii_case(column))
}

/// A table every database of `dialect` has without it being in the catalog: `DUAL` on Oracle,
/// MySQL and MariaDB.
fn dialect_table(dialect: Dialect, name: &str) -> bool {
    matches!(dialect, Dialect::Oracle | Dialect::MySql | Dialect::MariaDb)
        && name.eq_ignore_ascii_case("dual")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Column;

    fn table(name: &str, columns: &[&str]) -> Table {
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: name.into(),
            kind: "table".into(),
            columns: columns
                .iter()
                .map(|c| Column {
                    name: (*c).into(),
                    data_type: "int".into(),
                    nullable: true,
                    default: None,
                    generated: None,
                    expression: None,
                    identity: None,
                    comment: None,
                })
                .collect(),
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
            unreadable: None,
            constraints: Vec::new(),
            comment: None,
        }
    }

    fn schema() -> Vec<Table> {
        vec![
            table("item", &["sku", "name"]),
            table("stock", &["warehouse_id", "sku", "qty"]),
            table("warehouse", &["id", "city"]),
        ]
    }

    fn pg(sql: &str) -> Vec<Problem> {
        inspect(sql, Dialect::PostgreSql, &schema())
    }

    /// SPEC-0015 v5 AK-12 (OBS-0083 L4): `DUAL` is no unknown table on Oracle, MySQL and MariaDB,
    /// and its columns are not checked; on PostgreSQL it still is one.
    #[test]
    fn dual_is_known_where_the_dialect_has_it() {
        for d in [Dialect::Oracle, Dialect::MySql, Dialect::MariaDb] {
            assert!(
                inspect("select sysdate, dummy from dual", d, &schema()).is_empty(),
                "{d:?}"
            );
            assert!(
                inspect("SELECT 1 FROM DUAL", d, &schema()).is_empty(),
                "{d:?}"
            );
        }
        assert_eq!(pg("select 1 from dual")[0].message, "unknown table `dual`");
        let ora = inspect("select * from stok, dual", Dialect::Oracle, &schema());
        assert_eq!(ora.len(), 1);
        assert_eq!(ora[0].message, "unknown table `stok`");
    }

    #[test]
    fn unknown_tables() {
        let sql = "select * from stok";
        let p = pg(sql);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].severity, Severity::Error);
        assert_eq!(p[0].message, "unknown table `stok`");
        assert_eq!(&sql[p[0].start..p[0].end], "stok");
        assert!(pg("select * from stock s join item i on i.sku = s.sku").is_empty());
        assert!(pg("with x as (select 1 as a) select * from x").is_empty());
        assert!(pg("select * from generate_series(1, 3)").is_empty());
        assert!(pg("create table newt (a int)").is_empty());
        // SPEC-0192 v7 behaviour 11b (OBS-0070 window check): a table the script creates earlier,
        // temporary or not, is known in the later statements; before its CREATE it is not.
        let script = "select * from pd_tmp;\n\
                      create temporary table pd_tmp (id int);\n\
                      insert into pd_tmp values (1);\n\
                      select * from pd_tmp;\n\
                      create table if not exists public.other_t (a int);\n\
                      select * from other_t;";
        let p = pg(script);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].start, script.find("pd_tmp").unwrap());
        assert!(pg("create temp table t1 (a int); select * from t1").is_empty());
        assert!(inspect("select * from stok", Dialect::PostgreSql, &[]).is_empty());
    }

    /// SPEC-0015 v3 AK-10: a virtual table's hidden columns (FTS5's `rank` and the column named
    /// after the table) are no unknown columns, qualified or bare; other names still are.
    #[test]
    fn hidden_columns_of_virtual_tables_are_known() {
        let mut docs = table("docs", &["title", "body"]);
        docs.hidden_columns = vec!["docs".into(), "rank".into()];
        let schema = [docs];
        let problems = |sql: &str| -> Vec<String> {
            inspect(sql, Dialect::Sqlite, &schema)
                .into_iter()
                .map(|p| p.message)
                .collect()
        };
        assert!(
            problems("select title, rank from docs where docs match 'x' order by rank").is_empty()
        );
        assert!(problems("select d.rank, d.Docs from docs d").is_empty());
        assert_eq!(
            problems("select d.rang from docs d"),
            ["unknown column `rang` in `docs`"]
        );
        assert_eq!(
            problems("select score from docs"),
            ["unknown column `score`"]
        );
    }

    #[test]
    fn unknown_columns() {
        let sql = "select s.qtty from stock s";
        let p = pg(sql);
        assert_eq!(p[0].message, "unknown column `qtty` in `stock`");
        assert_eq!(&sql[p[0].start..p[0].end], "qtty");
        assert!(pg("select s.*, stock.qty from stock s").is_empty());
    }

    #[test]
    fn unsafe_changes() {
        for sql in ["delete from stock", "update stock set qty = 0"] {
            let p = pg(sql);
            assert_eq!(p.len(), 1, "{sql}");
            assert_eq!(p[0].severity, Severity::Warning);
        }
        assert!(pg("delete from stock where qty = 0").is_empty());
    }

    #[test]
    fn mysql_ignored_references() {
        let sql =
            "create table t (a int references w (id), b int, foreign key (b) references w (id))";
        let my = inspect(sql, Dialect::MySql, &schema());
        assert_eq!(my.len(), 1);
        assert!(my[0].message.contains("column `a`"), "{}", my[0].message);
        assert_eq!(&sql[my[0].start..my[0].end], "references");
        assert!(inspect(sql, Dialect::PostgreSql, &schema()).is_empty());
    }

    #[test]
    fn literals_and_comments_are_ignored() {
        assert!(pg("select 'from stok' -- from stok").is_empty());
    }

    #[test]
    fn closest_names_come_first_as_fixes() {
        let sql = "select nme from item";
        let p = pg(sql);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].message, "unknown column `nme`");
        assert_eq!(&sql[p[0].start..p[0].end], "nme");
        assert_eq!(p[0].fixes[0].label, "Replace with `name`");
        assert_eq!(p[0].fixes[0].replacement, "name");
        assert_eq!((p[0].fixes[0].start, p[0].fixes[0].end), (7, 10));
        assert_eq!(pg("select * from stok")[0].fixes[0].replacement, "stock");
        assert_eq!(
            pg("select s.qtty from stock s")[0].fixes[0].replacement,
            "qty"
        );
        assert!(pg("select * from zzzzzz")[0].fixes.is_empty());
        assert_eq!(distance("kitten", "sitting"), 3);
    }

    #[test]
    fn bare_columns_skip_aliases_functions_and_literals() {
        for ok in [
            "select qty q, count(*) as n from stock group by q order by n",
            "select now()",
            "select x.a from (select 1 a) x",
            "select sku, name from item where name like 'a%' and sku is not null",
            "select 'text' label, 1 as one from item order by label",
            "select qty::text from stock",
            "select * from stock s join item i on i.sku = s.sku where s.qty > 0",
            "select sku from item where exists (select 1 from stock where stock.sku = item.sku)",
            "select sum(qty) total from stock having sum(qty) > 1",
            "select true, current_date from item",
            "select sku from stock union select sku from item",
        ] {
            assert!(pg(ok).is_empty(), "{ok}: {:?}", pg(ok));
        }
        let p = pg("select city, qty from stock");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].message, "unknown column `city`");
        let p = pg("select sku from item where nam = 'x'");
        assert_eq!(p[0].fixes[0].replacement, "name");
    }

    #[test]
    fn unsafe_change_names_the_table() {
        let p = pg("delete from stock");
        assert_eq!(p[0].table.as_deref(), Some("stock"));
        assert!(
            p[0].message
                .starts_with("DELETE without WHERE removes every row of `stock`")
        );
        let p = pg("update public.stock set qty = 0");
        assert_eq!(p[0].table.as_deref(), Some("public.stock"));
        assert!(
            p[0].message
                .starts_with("UPDATE without WHERE changes every row of `public.stock`")
        );
        assert_eq!(
            pg("delete from stock")[0].confirmation(Some(42)).as_deref(),
            Some("Delete all 42 rows of `stock`?")
        );
        assert_eq!(
            p[0].confirmation(None).as_deref(),
            Some("Update all rows of `public.stock`?")
        );
        assert_eq!(
            p[0].confirmation(Some(1)).as_deref(),
            Some("Update the only row of `public.stock`?")
        );
        let p = inspect("delete from \"Order Items\"", Dialect::PostgreSql, &[]);
        assert_eq!(p[0].table.as_deref(), Some("\"Order Items\""));
    }

    /// SPEC-0015 v4 AK-11 (OBS-0068 L3): the outer place of a WITH query names one of its result
    /// columns, or it is marked.
    #[test]
    fn columns_of_with_queries() {
        let schema = vec![table("pd_note", &["id", "text_body", "qty"])];
        let check = |sql: &str| inspect(sql, Dialect::PostgreSql, &schema);
        let sql = "with big as (select id, text_body from pd_note where qty > 10) select b.body from big b";
        let p = check(sql);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].message, "unknown column `body` in `big`");
        assert_eq!(&sql[p[0].start..p[0].end], "body");
        let p = check("with big as (select id, text_body from pd_note) select big.body from big");
        assert_eq!(p.len(), 1, "{p:?}");
        for fine in [
            "with big as (select id, text_body as body from pd_note) select b.body from big b",
            "with big as (select id, text_body body from pd_note) select b.body from big b",
            "with big(id, body) as (select id, text_body from pd_note) select b.body from big b",
            "with big as (select * from pd_note) select b.body from big b",
            "with big as (select n.id, n.text_body::text from pd_note n) select b.text_body from big b",
            "with big as (select upper(text_body) from pd_note) select b.upper from big b",
            "with big as (select case when qty > 1 then id end from pd_note) select b.x from big b",
            "with big as (select id, \"Body\" from pd_note) select b.body from big b",
            "with big as (values (1)) select b.x from big b",
        ] {
            assert!(check(fine).is_empty(), "{fine}: {:?}", check(fine));
        }
        let p =
            check("with big as (select id, text_bodi from pd_note) select b.text_body from big b");
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].message, "unknown column `text_body` in `big`");
        assert_eq!(p[0].fixes[0].replacement, "text_bodi");
        // A second query sees the first one's columns by its own list.
        let p = check(
            "with a as (select id from pd_note), b as (select a.id, a.qty from a) select b.id from b",
        );
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].message, "unknown column `qty` in `a`");
    }
}
