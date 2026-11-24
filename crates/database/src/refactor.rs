//! Renaming tables and columns in SQL (`SPEC-0019`).
//!
//! Each statement is scanned on its own. References that can be decided are
//! turned into text edits; places that could belong to another table are
//! reported instead of being guessed.

use std::ops::Range;

use serde::Serialize;

use crate::completion::{CLAUSE_ENDS, KEYWORDS, insert_text};
use crate::script::{dollar_tag_end, find, skip_bracketed, skip_quoted};
use crate::{ConstraintName, Dialect, Table, quote_identifier, split_statements};

/// Why a rename cannot be done.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenameError {
    /// The table is not in the schema.
    #[error("unknown table `{0}`")]
    UnknownTable(String),
    /// The column is not in the table.
    #[error("`{0}` has no column `{1}`")]
    UnknownColumn(String, String),
    /// The new name is taken.
    #[error("`{0}` already exists")]
    Exists(String),
    /// The new name is empty.
    #[error("the new name is empty")]
    EmptyName,
    /// The database has no statement for it (`SPEC-0164`).
    #[error("{0}")]
    Unsupported(String),
    /// The index is not in the schema.
    #[error("unknown index `{0}`")]
    UnknownIndex(String),
    /// The type is empty or not a plain type name.
    #[error("`{0}` is not a column type")]
    BadType(String),
}

/// One replacement in the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextEdit {
    /// Byte offset where the replaced text starts.
    pub start: usize,
    /// Byte offset after the replaced text.
    pub end: usize,
    /// New text.
    pub replacement: String,
}

/// A place that may refer to the object but was not changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ambiguity {
    /// Byte offset where the name starts.
    pub start: usize,
    /// Byte offset after the name.
    pub end: usize,
    /// Why it was not changed.
    pub reason: String,
}

/// The result of [`rename`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rename {
    /// Statement that renames the object in the database.
    pub ddl: String,
    /// Edits in text order.
    pub edits: Vec<TextEdit>,
    /// Places left unchanged because they are ambiguous.
    pub ambiguous: Vec<Ambiguity>,
    /// The text with all edits applied.
    pub sql: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word { quoted: bool },
    Dot,
    Comma,
    Open,
    Close,
    Other,
}

struct Tok {
    kind: Kind,
    range: Range<usize>,
    /// Identifier without quotes (words only).
    value: String,
}

impl Tok {
    fn is_word(&self) -> bool {
        matches!(self.kind, Kind::Word { .. })
    }

    /// Whether this word names `name` (quoted: exactly, else ignoring case).
    fn names(&self, name: &str) -> bool {
        match self.kind {
            Kind::Word { quoted: true } => self.value == name,
            Kind::Word { quoted: false } => self.value.eq_ignore_ascii_case(name),
            _ => false,
        }
    }

    fn keyword(&self) -> Option<String> {
        matches!(self.kind, Kind::Word { quoted: false }).then(|| self.value.to_ascii_lowercase())
    }
}

fn scan(text: &str, dialect: Dialect) -> Vec<Tok> {
    let mysql = dialect.mysql_like();
    let ident_quote = if mysql { b'`' } else { b'"' };
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'[' && dialect == Dialect::SqlServer {
            let end = skip_bracketed(bytes, i);
            let inner = &text[i + 1..end.saturating_sub(1).max(i + 1)];
            out.push(Tok {
                kind: Kind::Word { quoted: true },
                range: i..end,
                value: inner.replace("]]", "]"),
            });
            i = end;
            continue;
        }
        if b == ident_quote {
            let end = skip_quoted(bytes, i, b, false);
            let inner = &text[i + 1..end.saturating_sub(1).max(i + 1)];
            let q = char::from(b).to_string();
            out.push(Tok {
                kind: Kind::Word { quoted: true },
                range: i..end,
                value: inner.replace(&format!("{q}{q}"), &q),
            });
            i = end;
            continue;
        }
        let skipped = match b {
            b'\'' | b'"' | b'`' => Some(skip_quoted(bytes, i, b, mysql)),
            b'-' if bytes.get(i + 1) == Some(&b'-') => Some(
                bytes[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(bytes.len(), |p| i + p),
            ),
            b'#' if mysql => Some(
                bytes[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(bytes.len(), |p| i + p),
            ),
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                Some(find(bytes, i + 2, b"*/").map_or(bytes.len(), |p| p + 2))
            }
            b'$' if dialect.postgres_like() => dollar_tag_end(bytes, i).map(|tag_end| {
                let tag = &bytes[i..=tag_end];
                find(bytes, tag_end + 1, tag).map_or(bytes.len(), |p| p + tag.len())
            }),
            _ => None,
        };
        if let Some(end) = skipped {
            i = end.max(i + 1);
            continue;
        }
        if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b'$'
                    || bytes[i] >= 0x80)
            {
                i += 1;
            }
            out.push(Tok {
                kind: Kind::Word { quoted: false },
                range: start..i,
                value: text[start..i].to_owned(),
            });
            continue;
        }
        let kind = match b {
            b'.' => Kind::Dot,
            b',' => Kind::Comma,
            b'(' => Kind::Open,
            b')' => Kind::Close,
            c if c.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            _ => Kind::Other,
        };
        out.push(Tok {
            kind,
            range: i..i + 1,
            value: String::new(),
        });
        i += 1;
    }
    out
}

/// A table reference: index of the name token, how it was introduced, alias.
struct Ref {
    name: usize,
    introducer: String,
    alias: Option<usize>,
}

fn is_reserved(word: &str) -> bool {
    KEYWORDS.contains(&word)
        || CLAUSE_ENDS.contains(&word)
        || matches!(
            word,
            "on" | "using"
                | "set"
                | "values"
                | "inner"
                | "left"
                | "right"
                | "full"
                | "cross"
                | "natural"
                | "outer"
                | "join"
                | "select"
                | "default"
                | "where"
        )
}

fn table_refs(tokens: &[Tok]) -> Vec<Ref> {
    let first: Vec<String> = tokens.iter().take(3).filter_map(Tok::keyword).collect();
    let create_index =
        first.first().is_some_and(|w| w == "create") && first.iter().any(|w| w == "index");
    let mut refs = Vec::new();
    let (mut in_from, mut depth, mut from_depth) = (false, 0usize, 0usize);
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth = depth.saturating_sub(1),
            _ => {}
        }
        let w = t.keyword().unwrap_or_default();
        let continues = t.kind == Kind::Comma && in_from && depth == from_depth;
        if w == "from" || w == "join" {
            in_from = true;
            from_depth = depth;
        } else if CLAUSE_ENDS.contains(&w.as_str()) {
            in_from = false;
        }
        let introduces = matches!(
            w.as_str(),
            "from" | "join" | "into" | "update" | "table" | "references"
        ) || (w == "on" && create_index);
        if !(introduces || continues) {
            continue;
        }
        let mut j = i + 1;
        if w == "table" {
            // CREATE TABLE IF NOT EXISTS name
            while tokens
                .get(j)
                .and_then(Tok::keyword)
                .is_some_and(|k| matches!(k.as_str(), "if" | "not" | "exists"))
            {
                j += 1;
            }
        }
        let mut name = None;
        while tokens.get(j).is_some_and(Tok::is_word) {
            name = Some(j);
            if tokens.get(j + 1).map(|n| n.kind) == Some(Kind::Dot) {
                j += 2;
            } else {
                j += 1;
                break;
            }
        }
        let Some(name) = name else { continue };
        let mut k = j;
        if tokens.get(k).and_then(Tok::keyword).as_deref() == Some("as") {
            k += 1;
        }
        let alias = tokens
            .get(k)
            .filter(|t| t.is_word())
            .filter(|t| t.keyword().is_none_or(|w| !is_reserved(&w)))
            .map(|_| k);
        refs.push(Ref {
            name,
            introducer: if continues { "from".into() } else { w },
            alias,
        });
    }
    refs
}

/// What a name in a statement stands for (`SPEC-0019` v5 behaviour 13).
#[derive(Clone, Copy)]
enum Target<'a> {
    /// A table or view of the schema.
    Table(&'a Table),
    /// A `WITH` query or a subquery in `FROM`, by its index in [`sources`].
    Source(usize),
    /// Nothing known.
    Unknown,
}

/// A `WITH` query or a subquery in `FROM` with an alias: a scope of its own whose result columns
/// its readers see (`SPEC-0019` v5 behaviour 13).
struct Source {
    /// The token naming it: the `WITH` query's name or the subquery's alias.
    name: usize,
    /// Whether it has a column list (`big(id, body)`, `(…) s(id, body)`).
    columns: bool,
    /// The `(` and `)` around the query.
    open: usize,
    close: usize,
    /// A `WITH` query (visible by name in the whole statement) rather than a subquery.
    with: bool,
}

/// The token after the `)` matching the `(` at `open`, or `None` when it is not closed.
fn matching_close(tokens: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, t) in tokens.iter().enumerate().skip(open) {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// The `WITH` queries and the subqueries in `FROM`/`JOIN` with an alias of a statement.
fn sources(tokens: &[Tok]) -> Vec<Source> {
    let mut out = Vec::new();
    let keyword = |k: usize| tokens.get(k).and_then(Tok::keyword);
    // Whether a `FROM` list is open at each depth of parentheses, so a comma continues it.
    let mut from_at = vec![false];
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            Kind::Open => {
                depth += 1;
                if from_at.len() <= depth {
                    from_at.resize(depth + 1, false);
                }
                from_at[depth] = false;
            }
            Kind::Close => depth = depth.saturating_sub(1),
            _ => {}
        }
        let word = t.keyword();
        match word.as_deref() {
            Some("from" | "join") => from_at[depth] = true,
            Some(k) if CLAUSE_ENDS.contains(&k) => from_at[depth] = false,
            _ => {}
        }
        if word.as_deref() == Some("with") {
            let mut j = i + 1;
            if keyword(j).as_deref() == Some("recursive") {
                j += 1;
            }
            while tokens.get(j).is_some_and(Tok::is_word) {
                let name = j;
                j += 1;
                let columns = tokens.get(j).map(|t| t.kind) == Some(Kind::Open);
                if columns {
                    let Some(c) = matching_close(tokens, j) else {
                        break;
                    };
                    j = c + 1;
                }
                if keyword(j).as_deref() != Some("as") {
                    break;
                }
                j += 1;
                while matches!(keyword(j).as_deref(), Some("not" | "materialized")) {
                    j += 1;
                }
                if tokens.get(j).map(|t| t.kind) != Some(Kind::Open) {
                    break;
                }
                let open = j;
                let Some(close) = matching_close(tokens, open) else {
                    break;
                };
                out.push(Source {
                    name,
                    columns,
                    open,
                    close,
                    with: true,
                });
                j = close + 1;
                if tokens.get(j).map(|t| t.kind) == Some(Kind::Comma) {
                    j += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        let introduces = matches!(word.as_deref(), Some("from" | "join" | "lateral"))
            || (t.kind == Kind::Comma && from_at[depth]);
        let open = i + 1;
        if !introduces
            || tokens.get(open).map(|t| t.kind) != Some(Kind::Open)
            || !matches!(
                keyword(open + 1).as_deref(),
                Some("select" | "with" | "values")
            )
        {
            continue;
        }
        let Some(close) = matching_close(tokens, open) else {
            continue;
        };
        let mut k = close + 1;
        if keyword(k).as_deref() == Some("as") {
            k += 1;
        }
        let Some(alias) = tokens
            .get(k)
            .filter(|t| t.is_word())
            .filter(|t| t.keyword().is_none_or(|w| !is_reserved(&w)))
            .map(|_| k)
        else {
            continue;
        };
        out.push(Source {
            name: alias,
            columns: tokens.get(alias + 1).map(|t| t.kind) == Some(Kind::Open),
            open,
            close,
            with: false,
        });
    }
    out
}

/// The innermost source whose query holds each token (`None`: the statement itself).
fn scopes_of(tokens: &[Tok], sources: &[Source]) -> Vec<Option<usize>> {
    (0..tokens.len())
        .map(|i| {
            sources
                .iter()
                .enumerate()
                .filter(|(_, s)| s.open < i && i < s.close)
                .min_by_key(|(_, s)| s.close - s.open)
                .map(|(n, _)| n)
        })
        .collect()
}

/// The items of the select lists at the top level of the query inside `open..close`: the token
/// range of each item and the index of its `SELECT`.
fn select_items(tokens: &[Tok], open: usize, close: usize) -> Vec<(Range<usize>, usize)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut k = open + 1;
    while k < close {
        match tokens[k].kind {
            Kind::Open => depth += 1,
            Kind::Close => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth > 0 || tokens[k].keyword().as_deref() != Some("select") {
            k += 1;
            continue;
        }
        let select = k;
        k += 1;
        // DISTINCT [ON (…)] or ALL.
        if matches!(
            tokens.get(k).and_then(Tok::keyword).as_deref(),
            Some("distinct" | "all")
        ) {
            k += 1;
            if tokens.get(k).and_then(Tok::keyword).as_deref() == Some("on")
                && tokens.get(k + 1).map(|t| t.kind) == Some(Kind::Open)
            {
                k = matching_close(tokens, k + 1).map_or(close, |c| c + 1);
            }
        }
        let mut start = k;
        let mut d = 0usize;
        while k < close {
            let t = &tokens[k];
            match t.kind {
                Kind::Open => d += 1,
                Kind::Close => d = d.saturating_sub(1),
                Kind::Comma if d == 0 => {
                    out.push((start..k, select));
                    start = k + 1;
                }
                _ => {}
            }
            let ends = d == 0
                && t.keyword().is_some_and(|w| {
                    matches!(
                        w.as_str(),
                        "from"
                            | "into"
                            | "where"
                            | "group"
                            | "having"
                            | "window"
                            | "order"
                            | "limit"
                            | "offset"
                            | "fetch"
                            | "union"
                            | "intersect"
                            | "except"
                    )
                });
            if ends {
                break;
            }
            k += 1;
        }
        if start < k {
            out.push((start..k, select));
        }
    }
    out
}

/// `text` with every name qualified by schema `from` qualified by `to` instead, or without the
/// qualifier for `None` (`SPEC-0018` v6 behaviour 18). A quoted qualifier matches exactly, an
/// unquoted one ignoring case; literals, comments and names of three parts stay.
pub(crate) fn requalify(text: &str, dialect: Dialect, from: &str, to: Option<&str>) -> String {
    let tokens = scan(text, dialect);
    let mut edits = Vec::new();
    for i in 0..tokens.len().saturating_sub(2) {
        let t = &tokens[i];
        let first = i == 0 || tokens[i - 1].kind != Kind::Dot;
        let named = tokens[i + 2].is_word()
            || (tokens[i + 2].kind == Kind::Other && &text[tokens[i + 2].range.clone()] == "*");
        if !first || !t.names(from) || tokens[i + 1].kind != Kind::Dot || !named {
            continue;
        }
        edits.push(match to {
            Some(to) => TextEdit {
                start: t.range.start,
                end: t.range.end,
                replacement: match t.kind {
                    Kind::Word { quoted: true } => quote_identifier(to, dialect),
                    _ => insert_text(to, dialect),
                },
            },
            None => TextEdit {
                start: t.range.start,
                end: tokens[i + 1].range.end,
                replacement: String::new(),
            },
        });
    }
    apply_edits(text, &edits)
}

/// Occurrences of `column` inside the body of `CREATE TABLE table (…)`: the
/// column definition itself and unqualified uses in checks, keys and defaults
/// (`SPEC-0019` v3). Column lists after `REFERENCES other (…)` belong to the
/// other table and are left alone; names right after `CONSTRAINT` are
/// constraint names.
fn create_table_body(tokens: &[Tok], table: &str, column: &str, done: &mut Vec<usize>) {
    if tokens.first().and_then(Tok::keyword).as_deref() != Some("create") {
        return;
    }
    let Some(mut j) = tokens
        .iter()
        .take(5)
        .position(|t| t.keyword().as_deref() == Some("table"))
        .map(|p| p + 1)
    else {
        return;
    };
    while tokens
        .get(j)
        .and_then(Tok::keyword)
        .is_some_and(|k| matches!(k.as_str(), "if" | "not" | "exists"))
    {
        j += 1;
    }
    let mut name = None;
    while tokens.get(j).is_some_and(Tok::is_word) {
        name = Some(j);
        if tokens.get(j + 1).map(|n| n.kind) == Some(Kind::Dot) {
            j += 2;
        } else {
            j += 1;
            break;
        }
    }
    if !name.is_some_and(|n| tokens[n].names(table))
        || tokens.get(j).map(|t| t.kind) != Some(Kind::Open)
    {
        return;
    }
    let mut depth = 1usize;
    let mut k = j + 1;
    let mut element_start = true;
    while k < tokens.len() && depth > 0 {
        let t = &tokens[k];
        match t.kind {
            Kind::Open => {
                depth += 1;
                k += 1;
                continue;
            }
            Kind::Close => {
                depth -= 1;
                k += 1;
                continue;
            }
            Kind::Comma => {
                element_start = depth == 1;
                k += 1;
                continue;
            }
            _ => {}
        }
        let starts = std::mem::take(&mut element_start);
        match t.keyword().as_deref() {
            Some("references") => {
                // Skip the referenced table name and its column list.
                k += 1;
                while tokens
                    .get(k)
                    .is_some_and(|t| t.is_word() || t.kind == Kind::Dot)
                {
                    k += 1;
                }
                if tokens.get(k).map(|t| t.kind) == Some(Kind::Open) {
                    let mut d = 0usize;
                    while k < tokens.len() {
                        match tokens[k].kind {
                            Kind::Open => d += 1,
                            Kind::Close => {
                                d -= 1;
                                if d == 0 {
                                    k += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                }
                continue;
            }
            Some("constraint") => {
                k += 2;
                continue;
            }
            _ => {}
        }
        if t.names(column) {
            let prev = k.checked_sub(1).map(|p| tokens[p].kind);
            let next = tokens.get(k + 1).map(|n| n.kind);
            let qualified = prev == Some(Kind::Dot) || next == Some(Kind::Dot);
            let call = next == Some(Kind::Open) && !starts;
            if !qualified && !call {
                done.push(k);
            }
        }
        k += 1;
    }
}

/// A string literal of `s` for `sp_rename`.
fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The statement renaming a table, view or column in `dialect` (`SPEC-0164` behaviour 1).
pub fn rename_ddl(
    dialect: Dialect,
    view: bool,
    table: &str,
    column: Option<&str>,
    new_name: &str,
) -> Result<String, RenameError> {
    let q = |n: &str| insert_text(n, dialect);
    let unsupported = |what: &str| Err(RenameError::Unsupported(what.to_owned()));
    Ok(match (dialect, view, column) {
        (Dialect::SqlServer, _, Some(c)) => format!(
            "EXEC sp_rename {}, {}, 'COLUMN'",
            literal(&format!("{table}.{c}")),
            literal(new_name)
        ),
        (Dialect::SqlServer, _, None) => {
            format!("EXEC sp_rename {}, {}", literal(table), literal(new_name))
        }
        (Dialect::PostgreSql, true, Some(c)) => format!(
            "ALTER VIEW {} RENAME COLUMN {} TO {}",
            q(table),
            q(c),
            q(new_name)
        ),
        (_, true, Some(_)) => {
            return unsupported(
                "this database cannot rename a column of a view; change the view's definition",
            );
        }
        (Dialect::PostgreSql, true, None) => {
            format!("ALTER VIEW {} RENAME TO {}", q(table), q(new_name))
        }
        (Dialect::MySql | Dialect::MariaDb, true, None) => {
            format!("RENAME TABLE {} TO {}", q(table), q(new_name))
        }
        (Dialect::Oracle, true, None) => format!("RENAME {} TO {}", q(table), q(new_name)),
        (Dialect::Sqlite, true, None) => {
            return unsupported("SQLite cannot rename a view; drop it and create it again");
        }
        (Dialect::DuckDb, true, None) => {
            format!("ALTER VIEW {} RENAME TO {}", q(table), q(new_name))
        }
        (_, false, Some(c)) => format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {}",
            q(table),
            q(c),
            q(new_name)
        ),
        (_, false, None) => format!("ALTER TABLE {} RENAME TO {}", q(table), q(new_name)),
    })
}

/// What a database refactoring does besides renaming a table or column (`SPEC-0164`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refactoring {
    /// Rename the table, view or column.
    Rename,
    /// Rename this index (of the table).
    RenameIndex(String),
    /// Rename this function or procedure.
    RenameRoutine(String),
    /// Change the column's type.
    ChangeType,
    /// Rename this sequence (`SPEC-0164` v3 behaviour 8).
    RenameSequence(String),
    /// Rename this named constraint of the table (`SPEC-0164` v3 behaviour 9); `kind` is
    /// `primary key`, `unique`, `check` or `foreign key`.
    RenameConstraint {
        /// Current name.
        name: String,
        /// Its kind.
        kind: String,
    },
}

/// Whether `dialect` rolls DDL back with its transaction, so a refactoring of several statements
/// runs as one (`SPEC-0164` v3 behaviour 11). MySQL, MariaDB and Oracle commit each DDL statement.
pub fn ddl_in_transaction(dialect: Dialect) -> bool {
    matches!(
        dialect,
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::SqlServer | Dialect::DuckDb
    )
}

/// The statements opening, committing and rolling back a transaction in `dialect`.
fn transaction_statements(dialect: Dialect) -> (&'static str, &'static str, &'static str) {
    match dialect {
        Dialect::SqlServer => (
            "BEGIN TRANSACTION",
            "COMMIT TRANSACTION",
            "ROLLBACK TRANSACTION",
        ),
        Dialect::DuckDb => ("BEGIN TRANSACTION", "COMMIT", "ROLLBACK"),
        Dialect::MySql | Dialect::MariaDb => ("START TRANSACTION", "COMMIT", "ROLLBACK"),
        _ => ("BEGIN", "COMMIT", "ROLLBACK"),
    }
}

/// A note shown before a refactoring runs (`SPEC-0164` v3 behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefactorNote {
    /// The text.
    pub text: String,
    /// The statement may lose data.
    pub data_loss: bool,
}

/// What a refactoring risks and how it runs in `dialect`, said before it runs.
pub fn refactoring_notes(what: &Refactoring, dialect: Dialect) -> Vec<RefactorNote> {
    let mut notes = Vec::new();
    if *what == Refactoring::ChangeType {
        notes.push(RefactorNote {
            text: "May lose data: the database converts every stored value, and values that do \
                   not fit the new type make it refuse the change or are changed"
                .into(),
            data_loss: true,
        });
    } else {
        notes.push(RefactorNote {
            text: "Views, triggers and routines stored in the database that use the old name are \
                   not rewritten by Ferris IDEA"
                .into(),
            data_loss: false,
        });
    }
    notes.push(RefactorNote {
        text: if ddl_in_transaction(dialect) {
            "Runs in one transaction: if the database refuses a statement, nothing changes".into()
        } else {
            format!(
                "{} commits each statement on its own: if it refuses a later one, the earlier \
                 ones stay done",
                dialect.label()
            )
        },
        data_loss: false,
    });
    notes
}

/// How a refactoring's statements ran (`SPEC-0164` v3 behaviour 11).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RefactorRun {
    /// Whether they ran in one transaction.
    pub transaction: bool,
    /// The statement that failed and the database's reason; then nothing was changed in a
    /// transaction, and outside one the main statement failed.
    pub failed: Option<(String, String)>,
    /// Outside a transaction: later statements the database refused, with the reason; the main
    /// statement stays done.
    pub refused: Vec<(String, String)>,
}

impl RefactorRun {
    /// Whether the main statement changed the database.
    pub fn changed(&self) -> bool {
        self.failed.is_none()
    }
}

/// Why a run did not change the database, or `None` when it did: the database's refusal of a
/// statement, or that no statement ran at all (`SPEC-0164` v2 behaviour 7).
pub fn run_failure(run: &crate::ScriptRun) -> Option<String> {
    if let Some(e) = run.outcomes.iter().find_map(|o| match &o.outcome {
        crate::Outcome::Error(e) => Some(e.message.clone()),
        _ => None,
    }) {
        return Some(e);
    }
    run.outcomes
        .is_empty()
        .then(|| "no statement was run".to_owned())
}

/// Runs `main` and then `more` on `session`: in one transaction where the dialect rolls DDL back
/// (a refusal then undoes all), else one by one, a refused later statement being reported and the
/// main one staying done (`SPEC-0164` v3 behaviour 11).
pub async fn run_refactoring(session: &crate::Session, main: &str, more: &[String]) -> RefactorRun {
    use crate::OnError;
    let dialect = session.dialect();
    let transaction = ddl_in_transaction(dialect);
    let mut out = RefactorRun {
        transaction,
        ..RefactorRun::default()
    };
    let (begin, commit, rollback) = transaction_statements(dialect);
    if transaction {
        let run = session.run_script(begin, OnError::Stop).await;
        if let Some(e) = run_failure(&run) {
            out.failed = Some((begin.into(), e));
            return out;
        }
    }
    for (i, statement) in std::iter::once(main)
        .chain(more.iter().map(String::as_str))
        .enumerate()
    {
        let run = session.run_script(statement, OnError::Stop).await;
        let Some(e) = run_failure(&run) else {
            continue;
        };
        if transaction {
            let _ = session.run_script(rollback, OnError::Stop).await;
            out.failed = Some((statement.into(), e));
            return out;
        }
        if i == 0 {
            out.failed = Some((statement.into(), e));
            return out;
        }
        out.refused.push((statement.into(), e));
    }
    if transaction {
        let run = session.run_script(commit, OnError::Stop).await;
        if let Some(e) = run_failure(&run) {
            let _ = session.run_script(rollback, OnError::Stop).await;
            out.failed = Some((commit.into(), e));
        }
    }
    out
}

/// Renames sequence `sequence` to `new_name`: the statement and its places in `script` — in
/// sequence statements, after `NEXT VALUE FOR`, before `.nextval` / `.currval`; a name inside a
/// literal (`nextval('s')`) is listed as left unchanged (`SPEC-0164` v3 behaviour 8), unless it
/// names the sequence without doubt (v5, see [`rename_sequence_in`]; the sequence's schema is not
/// known here).
pub fn rename_sequence(
    script: &str,
    dialect: Dialect,
    sequence: &str,
    new_name: &str,
) -> Result<Rename, RenameError> {
    rename_sequence_in(script, dialect, sequence, None, new_name)
}

/// [`rename_sequence`] for a sequence of schema `home` (the schema the connection works in), when
/// known. On PostgreSQL the name inside `nextval('…')`, `currval('…')`, `setval('…', …)` and
/// `'…'::regclass` changes too when it names this sequence without doubt — written with `home`, or
/// without a schema where the script has not set a search path of its own; the other such literals
/// stay listed (`SPEC-0164` v5 behaviour 8, OBS-0070 V3).
pub fn rename_sequence_in(
    script: &str,
    dialect: Dialect,
    sequence: &str,
    home: Option<&str>,
    new_name: &str,
) -> Result<Rename, RenameError> {
    if new_name.is_empty() {
        return Err(RenameError::EmptyName);
    }
    let q = |n: &str| insert_text(n, dialect);
    let ddl = match dialect {
        Dialect::PostgreSql => format!("ALTER SEQUENCE {} RENAME TO {}", q(sequence), q(new_name)),
        Dialect::MariaDb => format!("RENAME TABLE {} TO {}", q(sequence), q(new_name)),
        Dialect::Oracle => format!("RENAME {} TO {}", q(sequence), q(new_name)),
        Dialect::SqlServer => format!(
            "EXEC sp_rename {}, {}, 'OBJECT'",
            literal(sequence),
            literal(new_name)
        ),
        Dialect::MySql | Dialect::Sqlite => {
            return Err(RenameError::Unsupported(format!(
                "{} has no sequences",
                dialect.label()
            )));
        }
        Dialect::DuckDb => {
            return Err(RenameError::Unsupported(
                "DuckDB cannot rename a sequence; create it under the new name and drop the old one"
                    .into(),
            ));
        }
    };
    let mut r = rename_words(script, dialect, ddl, new_name, |tokens, i| {
        if !tokens[i].names(sequence) {
            return false;
        }
        let kw = |k: usize| tokens.get(k).and_then(Tok::keyword).unwrap_or_default();
        let first = kw(0);
        let about_sequence = matches!(first.as_str(), "create" | "alter" | "drop")
            && tokens
                .iter()
                .take(i)
                .filter_map(Tok::keyword)
                .any(|k| k == "sequence");
        let next_value =
            i >= 3 && kw(i - 1) == "for" && kw(i - 2) == "value" && kw(i - 3) == "next";
        let dot_call = tokens.get(i + 1).is_some_and(|t| t.kind == Kind::Dot)
            && matches!(kw(i + 2).as_str(), "nextval" | "currval");
        about_sequence || next_value || dot_call
    });
    if dialect != Dialect::PostgreSql {
        r.ambiguous = literal_places(script, dialect, sequence);
        return Ok(r);
    }
    let (edits, ambiguous) = sequence_literal_places(script, sequence, home, new_name);
    if !edits.is_empty() {
        r.edits.extend(edits);
        r.edits.sort_by_key(|e| e.start);
        r.sql = apply_edits(script, &r.edits);
    }
    r.ambiguous = ambiguous;
    Ok(r)
}

/// A string literal of a PostgreSQL script (`SPEC-0164` v5 behaviour 8).
struct PgLiteral {
    /// Its place, quotes included.
    range: Range<usize>,
    /// Written `E'…'`: backslashes escape, so its text is not read as a name.
    escape: bool,
    /// The first argument of `nextval`, `currval` or `setval`, or cast with `::regclass`.
    names_sequence: bool,
}

/// Whether `b` may be part of an unquoted identifier.
fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// The string literals of PostgreSQL `text` from `from` to `to`, also inside dollar-quoted bodies
/// (a function's body is SQL too); comments and quoted identifiers are skipped.
fn pg_literals(text: &str, from: usize, to: usize, out: &mut Vec<PgLiteral>) {
    let bytes = &text.as_bytes()[..to];
    let mut i = from;
    while i < to {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i = bytes[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(to, |p| i + p);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = find(bytes, i + 2, b"*/").map_or(to, |p| p + 2);
            }
            b'"' => i = skip_quoted(bytes, i, b'"', false).max(i + 1),
            b'$' if i == 0 || !ident_byte(bytes[i - 1]) => match dollar_tag_end(bytes, i) {
                Some(tag_end) => {
                    let tag = &bytes[i..=tag_end];
                    let close = find(bytes, tag_end + 1, tag).unwrap_or(to);
                    pg_literals(text, tag_end + 1, close, out);
                    i = (close + tag.len()).min(to).max(i + 1);
                }
                None => i += 1,
            },
            b'\'' => {
                let escape = i > 0
                    && matches!(bytes[i - 1], b'e' | b'E')
                    && (i < 2 || !ident_byte(bytes[i - 2]));
                let end = skip_quoted(bytes, i, b'\'', escape).max(i + 1);
                out.push(PgLiteral {
                    range: i..end,
                    escape,
                    names_sequence: !escape && sequence_argument(bytes, i, end),
                });
                i = end;
            }
            _ => i += 1,
        }
    }
}

/// Whether the literal `start..end` of `bytes` is the first argument of `nextval`, `currval` or
/// `setval` (also `pg_catalog.`-qualified), or is cast with `::regclass`.
fn sequence_argument(bytes: &[u8], start: usize, end: usize) -> bool {
    let back = |mut k: usize| {
        while k > 0 && bytes[k - 1].is_ascii_whitespace() {
            k -= 1;
        }
        k
    };
    let k = back(start);
    if k > 0 && bytes[k - 1] == b'(' {
        let word_end = back(k - 1);
        let mut word_start = word_end;
        while word_start > 0 && ident_byte(bytes[word_start - 1]) {
            word_start -= 1;
        }
        let word = String::from_utf8_lossy(&bytes[word_start..word_end]).to_ascii_lowercase();
        if matches!(word.as_str(), "nextval" | "currval" | "setval") {
            return true;
        }
    }
    let skip = |mut k: usize| {
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        k
    };
    let k = skip(end);
    if !bytes[k..].starts_with(b"::") {
        return false;
    }
    let k = skip(k + 2);
    let word_end = (k..bytes.len())
        .find(|&j| !(ident_byte(bytes[j]) || bytes[j] == b'.'))
        .unwrap_or(bytes.len());
    let word = String::from_utf8_lossy(&bytes[k..word_end]).to_ascii_lowercase();
    word == "regclass" || word == "pg_catalog.regclass"
}

/// A part of a name as PostgreSQL reads it from a `regclass` text: the name (an unquoted part
/// folded to lower case), its place in the text and whether it was quoted.
struct NamePart {
    name: String,
    range: Range<usize>,
    quoted: bool,
}

/// The dot-separated parts of `text` read like a `regclass` input, or `None` when it is no name.
/// A text holding a quote mark (`''` in the literal) is not read here.
fn regclass_parts(text: &str) -> Option<Vec<NamePart>> {
    if text.contains('\'') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut i = 0;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        let part = if bytes.get(i) == Some(&b'"') {
            let end = skip_quoted(bytes, i, b'"', false);
            if end <= i + 2 || bytes[end - 1] != b'"' {
                return None;
            }
            i = end;
            NamePart {
                name: text[start + 1..end - 1].replace("\"\"", "\""),
                range: start..end,
                quoted: true,
            }
        } else {
            while i < bytes.len()
                && !matches!(bytes[i], b'.' | b'"')
                && !bytes[i].is_ascii_whitespace()
            {
                i += 1;
            }
            if i == start {
                return None;
            }
            NamePart {
                name: text[start..i].to_ascii_lowercase(),
                range: start..i,
                quoted: false,
            }
        };
        parts.push(part);
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            None => return Some(parts),
            Some(b'.') => i += 1,
            Some(_) => return None,
        }
    }
}

/// The search path a PostgreSQL script has set for what follows a place.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchPath {
    /// The connection's own: nothing set, or reset.
    Default,
    /// Set; its first schema.
    First(String),
    /// Set in a way not read here (`$user` first, `set_config`, an odd value).
    Unknown,
}

/// Where PostgreSQL `script` changes its search path and to what: `SET [SESSION | LOCAL]
/// search_path`, `SET SCHEMA`, `RESET search_path`, `RESET ALL`, `set_config(…)`. Statements inside
/// dollar-quoted bodies are not counted.
fn search_path_changes(script: &str) -> Vec<(usize, SearchPath)> {
    let tokens = scan(script, Dialect::PostgreSql);
    let kw = |k: usize| tokens.get(k).and_then(Tok::keyword).unwrap_or_default();
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        let at = tokens[i].range.start;
        match kw(i).as_str() {
            "set_config" => out.push((at, SearchPath::Unknown)),
            "reset" if matches!(kw(i + 1).as_str(), "search_path" | "all") => {
                out.push((at, SearchPath::Default));
            }
            "set" => {
                let mut k = i + 1;
                if matches!(kw(k).as_str(), "session" | "local") {
                    k += 1;
                }
                if !matches!(kw(k).as_str(), "search_path" | "schema") {
                    continue;
                }
                let rest = &script[tokens[k].range.end..];
                let value = &rest[..rest.find(';').unwrap_or(rest.len())];
                out.push((at, first_schema(value)));
            }
            _ => {}
        }
    }
    out
}

/// The first schema of a search path value (`TO a, b`, `= 'a'`, `"A"`), as PostgreSQL reads it.
fn first_schema(value: &str) -> SearchPath {
    let value = value.trim_start();
    let value = value
        .strip_prefix('=')
        .or_else(|| {
            let after = value.get(2..)?;
            (value.get(..2)?.eq_ignore_ascii_case("to")
                && after.starts_with(|c: char| c.is_whitespace() || c == '\'' || c == '"'))
            .then_some(after)
        })
        .unwrap_or(value)
        .trim();
    let first = value.split(',').next().unwrap_or("").trim();
    if first.eq_ignore_ascii_case("default") {
        return SearchPath::Default;
    }
    let inner = first
        .strip_prefix('\'')
        .and_then(|f| f.strip_suffix('\''))
        .unwrap_or(first);
    match regclass_parts(inner).as_deref() {
        Some([part]) if !part.name.starts_with('$') => SearchPath::First(part.name.clone()),
        _ => SearchPath::Unknown,
    }
}

/// `name` written inside a `regclass` text: as it is when it is a plain lower-case name and the old
/// part was not quoted, else in double quotes; a `'` doubled for the SQL literal.
fn regclass_text(name: &str, quoted: bool) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$');
    let text = if plain && !quoted {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    };
    text.replace('\'', "''")
}

/// The sequence literals of PostgreSQL `script` that name `sequence` (of schema `home`, when known):
/// the edits renaming it to `new_name` where they name it without doubt, and the places left alone
/// with the reason (`SPEC-0164` v5 behaviour 8).
fn sequence_literal_places(
    script: &str,
    sequence: &str,
    home: Option<&str>,
    new_name: &str,
) -> (Vec<TextEdit>, Vec<Ambiguity>) {
    let mut literals = Vec::new();
    pg_literals(script, 0, script.len(), &mut literals);
    let paths = search_path_changes(script);
    let path_at = |at: usize| {
        paths
            .iter()
            .rev()
            .find(|(p, _)| *p < at)
            .map_or(SearchPath::Default, |(_, s)| s.clone())
    };
    let (mut edits, mut ambiguous) = (Vec::new(), Vec::new());
    for lit in literals {
        let (start, end) = (lit.range.start, lit.range.end);
        let inner_start = start + 1;
        let inner = &script[inner_start..end.saturating_sub(1).max(inner_start)];
        let left = |reason: &str| Ambiguity {
            start,
            end,
            reason: reason.into(),
        };
        // A plain string, an escape string or a text that is no name: listed when its last part
        // reads like the name, as before v5.
        let parts = (lit.names_sequence && !lit.escape)
            .then(|| regclass_parts(inner))
            .flatten();
        let Some(parts) = parts else {
            let last = inner
                .rsplit('.')
                .next()
                .unwrap_or(inner)
                .trim()
                .trim_matches('"');
            if last.eq_ignore_ascii_case(sequence) {
                ambiguous.push(left("the name inside a string is left as it is"));
            }
            continue;
        };
        let Some(last) = parts.last() else { continue };
        if last.name != sequence {
            if last.name.eq_ignore_ascii_case(sequence) {
                ambiguous.push(left(
                    "written in another case, so it may name another sequence: left as it is",
                ));
            }
            continue;
        }
        let sure = match (parts.len(), home) {
            (1, _) => match path_at(start) {
                SearchPath::Default => Ok(()),
                SearchPath::First(s) if Some(s.as_str()) == home => Ok(()),
                _ => Err(
                    "the file sets its own search path before it, so it may name another \
                     schema's sequence: left as it is",
                ),
            },
            (2, Some(h)) if parts[0].name == h => Ok(()),
            // Another schema's sequence of the same name.
            (2, Some(_)) => continue,
            (2, None) => {
                Err("names a schema, and the sequence's schema is not known: left as it is")
            }
            _ => Err("names a database too: left as it is"),
        };
        match sure {
            Ok(()) => edits.push(TextEdit {
                start: inner_start + last.range.start,
                end: inner_start + last.range.end,
                replacement: regclass_text(new_name, last.quoted),
            }),
            Err(why) => ambiguous.push(left(why)),
        }
    }
    (edits, ambiguous)
}

/// String literals of `script` whose text is `name`, optionally schema-qualified (case ignored):
/// places a rename leaves alone and names.
fn literal_places(script: &str, dialect: Dialect, name: &str) -> Vec<Ambiguity> {
    let bytes = script.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            let end = skip_quoted(bytes, i, b'\'', dialect.mysql_like());
            let inner = &script[i + 1..end.saturating_sub(1).max(i + 1)];
            let last = inner.rsplit('.').next().unwrap_or(inner).trim_matches('"');
            if last.eq_ignore_ascii_case(name) {
                out.push(Ambiguity {
                    start: i,
                    end,
                    reason: "the name inside a string is left as it is".into(),
                });
            }
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

/// Renames the named constraint `name` (of `kind`) of `table` to `new_name`: the statement and
/// its places in the DDL of `script` (`SPEC-0164` v3 behaviour 9).
pub fn rename_constraint(
    script: &str,
    dialect: Dialect,
    table: &str,
    name: &str,
    kind: &str,
    new_name: &str,
) -> Result<Rename, RenameError> {
    if new_name.is_empty() {
        return Err(RenameError::EmptyName);
    }
    let q = |n: &str| insert_text(n, dialect);
    let ddl = match dialect {
        Dialect::PostgreSql | Dialect::Oracle => format!(
            "ALTER TABLE {} RENAME CONSTRAINT {} TO {}",
            q(table),
            q(name),
            q(new_name)
        ),
        Dialect::SqlServer => format!(
            "EXEC sp_rename {}, {}, 'OBJECT'",
            literal(name),
            literal(new_name)
        ),
        Dialect::MySql | Dialect::MariaDb if kind == "unique" => format!(
            "ALTER TABLE {} RENAME INDEX {} TO {}",
            q(table),
            q(name),
            q(new_name)
        ),
        Dialect::MySql | Dialect::MariaDb => {
            return Err(RenameError::Unsupported(format!(
                "{} cannot rename a {kind} without dropping and adding it again",
                dialect.label()
            )));
        }
        Dialect::Sqlite | Dialect::DuckDb => {
            return Err(RenameError::Unsupported(format!(
                "{} cannot rename constraints",
                dialect.label()
            )));
        }
    };
    let edits = derived_name_edits(script, dialect, name, new_name);
    let sql = apply_edits(script, &edits);
    Ok(Rename {
        ddl,
        edits,
        ambiguous: Vec::new(),
        sql,
    })
}

/// Runs `what` over `script`: for renames `input` is the new name, for [`Refactoring::ChangeType`]
/// the new type (`SPEC-0164`).
pub fn refactor(
    script: &str,
    dialect: Dialect,
    schema: &[Table],
    table: &str,
    column: Option<&str>,
    what: &Refactoring,
    input: &str,
) -> Result<Rename, RenameError> {
    match what {
        Refactoring::Rename => rename(script, dialect, schema, table, column, input),
        Refactoring::RenameIndex(index) => rename_index(script, dialect, schema, index, input),
        Refactoring::RenameRoutine(routine) => rename_routine(script, dialect, routine, input),
        Refactoring::RenameSequence(sequence) => rename_sequence(script, dialect, sequence, input),
        Refactoring::RenameConstraint { name, kind } => {
            rename_constraint(script, dialect, table, name, kind, input)
        }
        Refactoring::ChangeType => {
            let ddl = change_type_ddl(dialect, schema, table, column.unwrap_or_default(), input)?;
            Ok(Rename {
                ddl,
                edits: Vec::new(),
                ambiguous: Vec::new(),
                sql: script.to_owned(),
            })
        }
    }
}

/// The refactoring that takes a database refactoring back (`SPEC-0164` v5 behaviour 16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WayBack {
    /// The table it works on, by the name it has after the refactoring.
    pub table: String,
    /// The column, by its name after the refactoring.
    pub column: Option<String>,
    /// What is done.
    pub what: Refactoring,
    /// The old name, or the column's old type.
    pub input: String,
}

/// How to take back `what` on `table` / `column` that set `input` (a new name or type), with
/// `schema` read before it ran: the new name renamed back to the old one, or the column changed
/// back to the type it had. `None` when the old type is not in `schema`.
pub fn way_back(
    schema: &[Table],
    table: &str,
    column: Option<&str>,
    what: &Refactoring,
    input: &str,
) -> Option<WayBack> {
    let back = |table: &str, column: Option<&str>, what: Refactoring, old: &str| WayBack {
        table: table.to_owned(),
        column: column.map(str::to_owned),
        what,
        input: old.to_owned(),
    };
    Some(match (what, column) {
        (Refactoring::Rename, Some(c)) => back(table, Some(input), Refactoring::Rename, c),
        (Refactoring::Rename, None) => back(input, None, Refactoring::Rename, table),
        (Refactoring::RenameIndex(n), _) => {
            back(table, None, Refactoring::RenameIndex(input.into()), n)
        }
        (Refactoring::RenameRoutine(n), _) => {
            back(table, None, Refactoring::RenameRoutine(input.into()), n)
        }
        (Refactoring::RenameSequence(n), _) => {
            back(table, None, Refactoring::RenameSequence(input.into()), n)
        }
        (Refactoring::RenameConstraint { name, kind }, _) => back(
            table,
            None,
            Refactoring::RenameConstraint {
                name: input.into(),
                kind: kind.clone(),
            },
            name,
        ),
        (Refactoring::ChangeType, Some(c)) => {
            let old = schema
                .iter()
                .find(|t| t.name == table)?
                .columns
                .iter()
                .find(|col| col.name == c)?
                .data_type
                .clone();
            back(table, Some(c), Refactoring::ChangeType, &old)
        }
        (Refactoring::ChangeType, None) => return None,
    })
}

/// `schema` as it is after `what` set `input` on `table` / `column`: the table, column or index
/// renamed, or the column's type changed.
fn schema_after(
    schema: &[Table],
    table: &str,
    column: Option<&str>,
    what: &Refactoring,
    input: &str,
) -> Vec<Table> {
    let mut after = schema.to_vec();
    let Some(t) = after.iter_mut().find(|t| t.name == table) else {
        return after;
    };
    let col = column.and_then(|c| t.columns.iter().position(|col| col.name == c));
    match (what, col) {
        (Refactoring::Rename, Some(i)) => t.columns[i].name = input.into(),
        (Refactoring::Rename, None) if column.is_none() => t.name = input.into(),
        (Refactoring::ChangeType, Some(i)) => t.columns[i].data_type = input.into(),
        (Refactoring::RenameIndex(n), _) => {
            if let Some(i) = t.indexes.iter_mut().find(|i| i.name == *n) {
                i.name = input.into();
            }
        }
        _ => {}
    }
    after
}

/// The statement that takes back `what` on `table` / `column` that set `input`, with `schema` read
/// before it ran (`SPEC-0164` v5 behaviour 16): what `rename_sql_object` and `change_column_type`
/// return as `inverse_ddl`. Undoing the file edits never runs it.
pub fn inverse_ddl(
    dialect: Dialect,
    schema: &[Table],
    table: &str,
    column: Option<&str>,
    what: &Refactoring,
    input: &str,
) -> Result<String, RenameError> {
    let back = way_back(schema, table, column, what, input)
        .ok_or_else(|| RenameError::UnknownTable(table.into()))?;
    let after = schema_after(schema, table, column, what, input);
    refactor(
        "",
        dialect,
        &after,
        &back.table,
        back.column.as_deref(),
        &back.what,
        &back.input,
    )
    .map(|r| r.ddl)
}

/// Renames index `index` to `new_name`: the statement and the places in `script` that name it in
/// index statements (`SPEC-0164` behaviour 2).
pub fn rename_index(
    script: &str,
    dialect: Dialect,
    schema: &[Table],
    index: &str,
    new_name: &str,
) -> Result<Rename, RenameError> {
    if new_name.is_empty() {
        return Err(RenameError::EmptyName);
    }
    let table = schema
        .iter()
        .find(|t| t.indexes.iter().any(|i| i.name == index))
        .ok_or_else(|| RenameError::UnknownIndex(index.into()))?;
    if schema
        .iter()
        .any(|t| t.indexes.iter().any(|i| i.name == new_name))
    {
        return Err(RenameError::Exists(new_name.into()));
    }
    let q = |n: &str| insert_text(n, dialect);
    let ddl = match dialect {
        Dialect::PostgreSql | Dialect::Oracle => {
            format!("ALTER INDEX {} RENAME TO {}", q(index), q(new_name))
        }
        Dialect::MySql | Dialect::MariaDb => format!(
            "ALTER TABLE {} RENAME INDEX {} TO {}",
            q(&table.name),
            q(index),
            q(new_name)
        ),
        Dialect::SqlServer => format!(
            "EXEC sp_rename {}, {}, 'INDEX'",
            literal(&format!("{}.{index}", table.name)),
            literal(new_name)
        ),
        Dialect::Sqlite => {
            return Err(RenameError::Unsupported(
                "SQLite cannot rename an index; drop it and create it again".into(),
            ));
        }
        Dialect::DuckDb => {
            return Err(RenameError::Unsupported(
                "DuckDB cannot rename an index; drop it and create it again".into(),
            ));
        }
    };
    Ok(rename_words(script, dialect, ddl, new_name, |tokens, i| {
        tokens[i].names(index)
            && tokens
                .iter()
                .filter_map(Tok::keyword)
                .any(|k| k == "index" || k == "key")
    }))
}

/// Edits renaming constraint or index `old` to `new_name` where `script` names it in DDL: in
/// statements that create, alter or drop an index, key or constraint (`SPEC-0019` v3 behaviour 9;
/// the file follows a derived rename like the database does).
pub fn derived_name_edits(
    script: &str,
    dialect: Dialect,
    old: &str,
    new_name: &str,
) -> Vec<TextEdit> {
    rename_words(script, dialect, String::new(), new_name, |tokens, i| {
        let first = tokens.first().and_then(Tok::keyword).unwrap_or_default();
        tokens[i].names(old)
            && matches!(first.as_str(), "create" | "alter" | "drop")
            && tokens
                .iter()
                .filter_map(Tok::keyword)
                .any(|k| matches!(k.as_str(), "index" | "key" | "constraint"))
    })
    .edits
}

/// Built-in functions that are never taken for a routine of the schema.
const BUILTINS: &[&str] = &[
    "abs",
    "avg",
    "cast",
    "coalesce",
    "concat",
    "count",
    "date",
    "extract",
    "greatest",
    "ifnull",
    "least",
    "length",
    "lower",
    "max",
    "min",
    "now",
    "nullif",
    "position",
    "replace",
    "round",
    "row_number",
    "substr",
    "substring",
    "sum",
    "trim",
    "upper",
    "exists",
    "in",
    "values",
];

/// Renames the function or procedure `routine` to `new_name`: the statement and its calls and
/// definitions in `script` (`SPEC-0164` behaviour 3).
pub fn rename_routine(
    script: &str,
    dialect: Dialect,
    routine: &str,
    new_name: &str,
) -> Result<Rename, RenameError> {
    if new_name.is_empty() {
        return Err(RenameError::EmptyName);
    }
    let q = |n: &str| insert_text(n, dialect);
    let ddl = match dialect {
        Dialect::PostgreSql => format!("ALTER ROUTINE {} RENAME TO {}", q(routine), q(new_name)),
        Dialect::SqlServer => format!("EXEC sp_rename {}, {}", literal(routine), literal(new_name)),
        Dialect::MySql | Dialect::MariaDb | Dialect::Sqlite | Dialect::Oracle | Dialect::DuckDb => {
            return Err(RenameError::Unsupported(format!(
                "{dialect:?} cannot rename a function or procedure; create it under the new name \
                 and drop the old one"
            )));
        }
    };
    Ok(rename_words(script, dialect, ddl, new_name, |tokens, i| {
        let prev = i
            .checked_sub(1)
            .and_then(|p| tokens[p].keyword())
            .unwrap_or_default();
        tokens[i].names(routine)
            && (tokens.get(i + 1).is_some_and(|t| t.kind == Kind::Open)
                || matches!(
                    prev.as_str(),
                    "function" | "procedure" | "routine" | "call" | "exec" | "execute" | "perform"
                ))
    }))
}

/// The routine named at a word of `tokens`, if the word looks like one: after FUNCTION, PROCEDURE,
/// CALL or EXEC, or a call that is no built-in function or keyword.
fn routine_at(tokens: &[Tok], i: usize) -> bool {
    let prev = i
        .checked_sub(1)
        .and_then(|p| tokens[p].keyword())
        .unwrap_or_default();
    let word = tokens[i].value.to_lowercase();
    matches!(
        prev.as_str(),
        "function" | "procedure" | "routine" | "call" | "exec" | "execute" | "perform"
    ) || (tokens.get(i + 1).is_some_and(|t| t.kind == Kind::Open)
        && !BUILTINS.contains(&word.as_str())
        && !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(&word))
        && prev != "into"
        && prev != "table"
        && prev != "references"
        && prev != "on")
}

/// Edits replacing the word tokens of `script` that `hit` picks with `new_name`.
fn rename_words(
    script: &str,
    dialect: Dialect,
    ddl: String,
    new_name: &str,
    hit: impl Fn(&[Tok], usize) -> bool,
) -> Rename {
    let mut edits = Vec::new();
    for statement in split_statements(script, dialect) {
        let base = statement.range.start;
        let tokens = scan(&script[statement.range.clone()], dialect);
        for i in 0..tokens.len() {
            if tokens[i].is_word() && hit(&tokens, i) {
                let r = &tokens[i].range;
                edits.push(TextEdit {
                    start: base + r.start,
                    end: base + r.end,
                    replacement: match tokens[i].kind {
                        Kind::Word { quoted: true } => quote_identifier(new_name, dialect),
                        _ => insert_text(new_name, dialect),
                    },
                });
            }
        }
    }
    let sql = apply_edits(script, &edits);
    Rename {
        ddl,
        edits,
        ambiguous: Vec::new(),
        sql,
    }
}

/// The statement changing the type of `table.column` to `new_type` in `dialect`, keeping its
/// nullability and default where the statement replaces the whole column (`SPEC-0164` behaviour 4).
pub fn change_type_ddl(
    dialect: Dialect,
    schema: &[Table],
    table: &str,
    column: &str,
    new_type: &str,
) -> Result<String, RenameError> {
    let ty = new_type.trim();
    // A type name with an optional size or precision, e.g. `varchar(200)`, `numeric(12, 2)`,
    // `double precision`; nothing that could end the statement.
    let plain = !ty.is_empty()
        && ty
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " _(),".contains(c));
    if !plain {
        return Err(RenameError::BadType(new_type.into()));
    }
    let t = schema
        .iter()
        .find(|t| t.name == table)
        .ok_or_else(|| RenameError::UnknownTable(table.into()))?;
    if t.kind == "view" {
        return Err(RenameError::Unsupported(
            "a view's column type follows its definition; change the view".into(),
        ));
    }
    let col = t
        .columns
        .iter()
        .find(|c| c.name == column)
        .ok_or_else(|| RenameError::UnknownColumn(table.into(), column.into()))?;
    let q = |n: &str| insert_text(n, dialect);
    let null = if col.nullable { "NULL" } else { "NOT NULL" };
    Ok(match dialect {
        Dialect::PostgreSql => format!(
            "ALTER TABLE {} ALTER COLUMN {} TYPE {ty}",
            q(table),
            q(column)
        ),
        Dialect::MySql | Dialect::MariaDb => {
            let default = col
                .default
                .as_ref()
                .map(|d| format!(" DEFAULT {d}"))
                .unwrap_or_default();
            format!(
                "ALTER TABLE {} MODIFY COLUMN {} {ty} {null}{default}",
                q(table),
                q(column)
            )
        }
        Dialect::SqlServer => format!(
            "ALTER TABLE {} ALTER COLUMN {} {ty} {null}",
            q(table),
            q(column)
        ),
        Dialect::Oracle => format!("ALTER TABLE {} MODIFY ({} {ty})", q(table), q(column)),
        Dialect::Sqlite => {
            return Err(RenameError::Unsupported(
                "SQLite cannot change a column's type; the table has to be rebuilt".into(),
            ));
        }
        Dialect::DuckDb => format!(
            "ALTER TABLE {} ALTER COLUMN {} TYPE {ty}",
            q(table),
            q(column)
        ),
    })
}

/// An index or routine named at byte `offset` of `script` (`SPEC-0164` behaviour 5): the
/// refactoring that renames it and, for an index, its table.
pub fn named_object_at(
    script: &str,
    dialect: Dialect,
    schema: &[Table],
    offset: usize,
) -> Option<(Refactoring, String)> {
    let (_, tokens, i) = token_at(script, dialect, offset)?;
    let tok = tokens.get(i).filter(|t| t.is_word())?;
    if let Some(t) = schema
        .iter()
        .find(|t| t.indexes.iter().any(|x| tok.names(&x.name)))
    {
        let name = t
            .indexes
            .iter()
            .find(|x| tok.names(&x.name))
            .map(|x| x.name.clone())?;
        return Some((Refactoring::RenameIndex(name), t.name.clone()));
    }
    if schema.iter().any(|t| tok.names(&t.name)) {
        return None;
    }
    routine_at(&tokens, i).then(|| (Refactoring::RenameRoutine(tok.value.clone()), String::new()))
}

/// Renames `table` (or its `column`) to `new_name` in `script` (`SPEC-0019`).
pub fn rename(
    script: &str,
    dialect: Dialect,
    schema: &[Table],
    table: &str,
    column: Option<&str>,
    new_name: &str,
) -> Result<Rename, RenameError> {
    if new_name.is_empty() {
        return Err(RenameError::EmptyName);
    }
    let t = schema
        .iter()
        .find(|t| t.name == table)
        .ok_or_else(|| RenameError::UnknownTable(table.into()))?;
    if let Some(column) = column {
        if !t.columns.iter().any(|c| c.name == column) {
            return Err(RenameError::UnknownColumn(table.into(), column.into()));
        }
        if t.columns.iter().any(|c| c.name == new_name) {
            return Err(RenameError::Exists(format!("{table}.{new_name}")));
        }
    } else if schema.iter().any(|t| t.name == new_name) {
        return Err(RenameError::Exists(new_name.into()));
    }
    let ddl = rename_ddl(dialect, t.kind == "view", table, column, new_name)?;
    let replacement = |tok: &Tok| match tok.kind {
        Kind::Word { quoted: true } => quote_identifier(new_name, dialect),
        _ => insert_text(new_name, dialect),
    };

    let mut edits = Vec::new();
    let mut ambiguous = Vec::new();
    for statement in split_statements(script, dialect) {
        let base = statement.range.start;
        let text = &script[statement.range.clone()];
        let tokens = scan(text, dialect);
        let refs = table_refs(&tokens);
        let edit = |i: usize, edits: &mut Vec<TextEdit>| {
            let r = &tokens[i].range;
            edits.push(TextEdit {
                start: base + r.start,
                end: base + r.end,
                replacement: replacement(&tokens[i]),
            });
        };
        let schema_table = |tok: &Tok| schema.iter().find(|s| tok.names(&s.name));
        // Qualifier → table name: alias first, then a referenced or known table.
        let resolve = |tok: &Tok| -> Option<&Table> {
            refs.iter()
                .find(|r| r.alias.is_some_and(|a| tokens[a].names(&tok.value)))
                .and_then(|r| schema_table(&tokens[r.name]))
                .or_else(|| schema_table(tok))
        };

        let Some(column) = column else {
            let mut seen = Vec::new();
            for r in &refs {
                if tokens[r.name].names(table) {
                    seen.push(r.name);
                }
            }
            for i in 0..tokens.len().saturating_sub(1) {
                if tokens[i].names(table)
                    && tokens[i + 1].kind == Kind::Dot
                    && !seen.contains(&i)
                    && resolve(&tokens[i]).is_some_and(|x| x.name == table)
                {
                    seen.push(i);
                }
            }
            seen.sort_unstable();
            for i in seen {
                edit(i, &mut edits);
            }
            continue;
        };

        // WITH queries and subqueries in FROM are scopes of their own (v5 behaviour 13).
        let srcs = sources(&tokens);
        let scope = scopes_of(&tokens, &srcs);
        let chain = |at: usize| {
            let mut out = vec![scope[at]];
            let mut s = scope[at];
            while let Some(n) = s {
                s = scope[srcs[n].open];
                out.push(s);
            }
            out
        };
        // A name: a WITH query of that name, else a table of the schema.
        let by_name = |tok: &Tok| {
            if let Some(n) = srcs
                .iter()
                .position(|s| s.with && tokens[s.name].names(&tok.value))
            {
                Target::Source(n)
            } else {
                schema_table(tok).map_or(Target::Unknown, Target::Table)
            }
        };
        // A qualifier at token `at`: an alias or name of its scope, then of the scopes around it,
        // then anywhere in the statement, then a WITH query or table of that name.
        let resolve_in = |q: &Tok, at: usize| {
            for sc in chain(at) {
                if let Some(r) = refs.iter().find(|r| {
                    scope[r.name] == sc && r.alias.is_some_and(|a| tokens[a].names(&q.value))
                }) {
                    return by_name(&tokens[r.name]);
                }
                if let Some(n) = srcs
                    .iter()
                    .position(|s| !s.with && scope[s.open] == sc && tokens[s.name].names(&q.value))
                {
                    return Target::Source(n);
                }
                if let Some(r) = refs.iter().find(|r| {
                    scope[r.name] == sc && r.alias.is_none() && tokens[r.name].names(&q.value)
                }) {
                    return by_name(&tokens[r.name]);
                }
            }
            if let Some(r) = refs
                .iter()
                .find(|r| r.alias.is_some_and(|a| tokens[a].names(&q.value)))
            {
                return by_name(&tokens[r.name]);
            }
            if let Some(n) = srcs
                .iter()
                .position(|s| !s.with && tokens[s.name].names(&q.value))
            {
                return Target::Source(n);
            }
            by_name(q)
        };
        let is_t = |target: Target, passes: &[bool]| match target {
            Target::Table(t) => t.name == table,
            Target::Source(m) => passes[m],
            Target::Unknown => false,
        };
        // Queries that pass the table's columns through with `*` or `t.*`: their result column
        // takes the new name (v5 behaviour 13).
        let mut passes = vec![false; srcs.len()];
        loop {
            let before = passes.clone();
            for (n, s) in srcs.iter().enumerate() {
                if passes[n] || s.columns {
                    continue;
                }
                let star = select_items(&tokens, s.open, s.close)
                    .iter()
                    .any(|(item, _)| {
                        let Some(last) = item.end.checked_sub(1).filter(|l| *l >= item.start)
                        else {
                            return false;
                        };
                        let star = tokens[last].kind == Kind::Other
                            && &text[tokens[last].range.clone()] == "*";
                        if !star {
                            false
                        } else if item.len() == 1 {
                            refs.iter()
                                .filter(|r| scope[r.name] == Some(n))
                                .any(|r| is_t(by_name(&tokens[r.name]), &before))
                                || srcs
                                    .iter()
                                    .enumerate()
                                    .any(|(m, d)| !d.with && scope[d.open] == Some(n) && before[m])
                        } else {
                            item.len() == 3
                                && tokens[item.start].is_word()
                                && tokens[item.start + 1].kind == Kind::Dot
                                && is_t(resolve_in(&tokens[item.start], item.start), &before)
                        }
                    });
                if star {
                    passes[n] = true;
                }
            }
            if passes == before {
                break;
            }
        }
        // Column lists of WITH queries and subqueries name their result columns, not the table's.
        let column_lists: Vec<Range<usize>> = srcs
            .iter()
            .filter(|s| s.columns)
            .filter_map(|s| {
                let open = s.name + 1;
                matching_close(&tokens, open).map(|c| open..c)
            })
            .collect();

        let mut done: Vec<usize> = Vec::new();
        // q.column
        for i in 0..tokens.len().saturating_sub(2) {
            if tokens[i].is_word()
                && tokens[i + 1].kind == Kind::Dot
                && tokens[i + 2].names(column)
                && is_t(resolve_in(&tokens[i], i), &passes)
            {
                done.push(i + 2);
            }
        }
        // Column lists that belong to the table for sure.
        let list_after = |open: usize, done: &mut Vec<usize>| {
            let mut k = open + 1;
            while k < tokens.len() && tokens[k].kind != Kind::Close {
                if tokens[k].names(column) {
                    done.push(k);
                }
                k += 1;
            }
        };
        for r in &refs {
            let introducer = r.introducer.as_str();
            if !tokens[r.name].names(table) || !matches!(introducer, "references" | "into" | "on") {
                continue;
            }
            let open = r.name + 1;
            if tokens.get(open).map(|t| t.kind) == Some(Kind::Open) {
                list_after(open, &mut done);
            }
        }
        create_table_body(&tokens, table, column, &mut done);
        let words: Vec<String> = tokens.iter().filter_map(Tok::keyword).take(3).collect();
        let ddl_statement = words.first().is_some_and(|w| {
            matches!(w.as_str(), "alter" | "drop")
                || (w == "create" && !words.iter().any(|w| w == "index"))
        });
        if !ddl_statement {
            // Per scope: whether it reads the table (or a query passing it through), and the
            // other tables of the scope with a column of that name.
            let reads = |sc: Option<usize>| {
                refs.iter()
                    .filter(|r| scope[r.name] == sc)
                    .any(|r| is_t(by_name(&tokens[r.name]), &passes))
                    || srcs
                        .iter()
                        .enumerate()
                        .any(|(m, d)| !d.with && scope[d.open] == sc && passes[m])
            };
            let others_in = |sc: Option<usize>| -> Vec<&Table> {
                let mut out: Vec<&Table> = Vec::new();
                for r in refs.iter().filter(|r| scope[r.name] == sc) {
                    if let Target::Table(o) = by_name(&tokens[r.name])
                        && o.name != table
                        && o.columns
                            .iter()
                            .any(|c| c.name.eq_ignore_ascii_case(column))
                        && !out.iter().any(|x| x.name == o.name)
                    {
                        out.push(o);
                    }
                }
                out
            };
            let update = words.first().is_some_and(|w| w == "update");
            let mut in_set = false;
            for (i, tok) in tokens.iter().enumerate() {
                if let Some(k) = tok.keyword() {
                    if k == "set" {
                        in_set = true;
                    } else if matches!(k.as_str(), "where" | "from" | "returning") {
                        in_set = false;
                    }
                }
                if !tok.names(column) || done.contains(&i) || !reads(scope[i]) {
                    continue;
                }
                let prev = i.checked_sub(1).map(|p| &tokens[p]);
                let next = tokens.get(i + 1);
                if prev.is_some_and(|p| p.kind == Kind::Dot)
                    || next.is_some_and(|n| matches!(n.kind, Kind::Dot | Kind::Open))
                    || prev.and_then(Tok::keyword).as_deref() == Some("as")
                    || refs.iter().any(|r| r.name == i || r.alias == Some(i))
                    || srcs.iter().any(|s| s.name == i)
                    || column_lists.iter().any(|l| l.contains(&i))
                {
                    continue;
                }
                let assigned =
                    update && in_set && next.is_some_and(|n| &text[n.range.clone()] == "=");
                let others = others_in(scope[i]);
                if others.is_empty() || assigned {
                    done.push(i);
                } else {
                    let names: Vec<&str> = others.iter().map(|o| o.name.as_str()).collect();
                    ambiguous.push(Ambiguity {
                        start: base + tok.range.start,
                        end: base + tok.range.end,
                        reason: format!(
                            "`{}` is also a column of {}; qualify it to rename",
                            tok.value,
                            names
                                .iter()
                                .map(|n| format!("`{n}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
            }
        }
        done.sort_unstable();
        done.dedup();
        // A result column of a WITH query or subquery that is the renamed column itself keeps its
        // name: `new AS old` (v5 behaviour 13). The last token of its item and its SELECT.
        let mut keeps_name: Vec<(usize, usize, usize)> = Vec::new();
        let cast = |from: usize, end: usize| {
            let colon = |k: usize| {
                tokens
                    .get(k)
                    .is_some_and(|t| t.kind == Kind::Other && &text[t.range.clone()] == ":")
            };
            colon(from)
                && colon(from + 1)
                && tokens.get(from + 2).is_some_and(Tok::is_word)
                && (end == from + 3
                    || (tokens.get(from + 3).map(|t| t.kind) == Some(Kind::Open)
                        && matching_close(&tokens, from + 3) == Some(end - 1)))
        };
        for s in srcs.iter().filter(|s| !s.columns) {
            for (item, select) in select_items(&tokens, s.open, s.close) {
                let (a, len) = (item.start, item.len());
                let col = if len >= 1 && tokens[a].is_word() && (len == 1 || cast(a + 1, item.end))
                {
                    a
                } else if len >= 3
                    && tokens[a].is_word()
                    && tokens[a + 1].kind == Kind::Dot
                    && tokens[a + 2].is_word()
                    && (len == 3 || cast(a + 3, item.end))
                {
                    a + 2
                } else {
                    continue;
                };
                if done.binary_search(&col).is_ok() {
                    keeps_name.push((col, item.end - 1, select));
                }
            }
        }
        let mut statement_edits = Vec::new();
        for i in done {
            match keeps_name.iter().find(|(c, _, _)| *c == i) {
                Some(&(_, last, select)) => {
                    let lower = text[tokens[select].range.clone()]
                        .chars()
                        .any(|c| c.is_ascii_lowercase());
                    let as_kw = if lower { "as" } else { "AS" };
                    let old = &text[tokens[i].range.clone()];
                    let r = &tokens[i].range;
                    if last == i {
                        statement_edits.push(TextEdit {
                            start: base + r.start,
                            end: base + r.end,
                            replacement: format!("{} {as_kw} {old}", replacement(&tokens[i])),
                        });
                    } else {
                        edit(i, &mut statement_edits);
                        let at = base + tokens[last].range.end;
                        statement_edits.push(TextEdit {
                            start: at,
                            end: at,
                            replacement: format!(" {as_kw} {old}"),
                        });
                    }
                }
                None => edit(i, &mut statement_edits),
            }
        }
        statement_edits.sort_by_key(|e| (e.start, e.end));
        edits.extend(statement_edits);
    }
    let sql = apply_edits(script, &edits);
    Ok(Rename {
        ddl,
        edits,
        ambiguous,
        sql,
    })
}

/// A table, or a column of it, named in SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SqlObject {
    /// Table name as in the schema.
    pub table: String,
    /// Column name as in the schema, for a column.
    pub column: Option<String>,
}

/// The statement of `script` around byte `offset`, its tokens and the index of the token there.
fn token_at(script: &str, dialect: Dialect, offset: usize) -> Option<(usize, Vec<Tok>, usize)> {
    let statement = split_statements(script, dialect)
        .into_iter()
        .find(|s| s.range.start <= offset && offset <= s.range.end)?;
    let base = statement.range.start;
    let tokens = scan(&script[statement.range.clone()], dialect);
    let local = offset - base;
    let i = tokens
        .iter()
        .position(|t| t.range.start <= local && local < t.range.end)
        .or_else(|| {
            tokens
                .iter()
                .position(|t| t.is_word() && t.range.end == local)
        })?;
    Some((base, tokens, i))
}

/// The table or column named at byte `offset` of `script` (`SPEC-0152` behaviour 1): a table where
/// it is introduced (`FROM`, `JOIN`, `INTO`, `UPDATE`, `TABLE`, `REFERENCES`) or qualifies a
/// column; `q.c` is a column of the table `q` names or aliases; an unqualified name is a column
/// when exactly one table of the statement has it. Otherwise the reason why not.
pub fn object_at(
    script: &str,
    dialect: Dialect,
    schema: &[Table],
    offset: usize,
) -> Result<SqlObject, String> {
    let Some((_, tokens, i)) = token_at(script, dialect, offset) else {
        return Err("No table or column at the caret".into());
    };
    let tok = &tokens[i];
    if !tok.is_word() {
        return Err("No table or column at the caret".into());
    }
    let refs = table_refs(&tokens);
    let schema_table = |t: &Tok| schema.iter().find(|s| t.names(&s.name));
    let alias_of = |t: &Tok| {
        refs.iter()
            .find(|r| r.alias.is_some_and(|a| tokens[a].names(&t.value)))
            .and_then(|r| schema_table(&tokens[r.name]))
    };
    let column_of = |table: &Table, t: &Tok| {
        table
            .columns
            .iter()
            .find(|c| t.names(&c.name))
            .map(|c| SqlObject {
                table: table.name.clone(),
                column: Some(c.name.clone()),
            })
    };
    // q.column
    if i >= 2 && tokens[i - 1].kind == Kind::Dot && tokens[i - 2].is_word() {
        let q = &tokens[i - 2];
        let Some(table) = alias_of(q).or_else(|| schema_table(q)) else {
            return Err(format!("`{}` is not a known table", q.value));
        };
        return column_of(table, tok)
            .ok_or_else(|| format!("`{}` has no column `{}`", table.name, tok.value));
    }
    if refs.iter().any(|r| r.alias == Some(i)) {
        return Err(format!("`{}` is an alias, not a table", tok.value));
    }
    let introduced = refs.iter().any(|r| r.name == i);
    let qualifies = tokens.get(i + 1).map(|t| t.kind) == Some(Kind::Dot);
    if let Some(table) = schema_table(tok)
        && (introduced || qualifies)
    {
        return Ok(SqlObject {
            table: table.name.clone(),
            column: None,
        });
    }
    let mut seen: Vec<&str> = Vec::new();
    let found: Vec<SqlObject> = refs
        .iter()
        .filter_map(|r| schema_table(&tokens[r.name]))
        .filter(|t| {
            let new = !seen.contains(&t.name.as_str());
            seen.push(&t.name);
            new
        })
        .filter_map(|t| column_of(t, tok))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => match schema_table(tok) {
            Some(table) => Ok(SqlObject {
                table: table.name.clone(),
                column: None,
            }),
            None => Err(format!(
                "`{}` is not a table or column of the statement",
                tok.value
            )),
        },
        many => Err(format!(
            "`{}` is a column of {}; qualify it to rename",
            tok.value,
            many.iter()
                .map(|o| format!("`{}`", o.table))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The edit that replaces the `*` (or `q.*`) at byte `offset` of a select list with the columns
/// of the statement's tables (`SPEC-0152` behaviour 4): `q.*` with `q.` and the columns of the
/// table `q` names; `*` with the columns of the only table, or with each table's columns qualified
/// by its alias or name when the statement joins several. `None` where there is no such `*` or a
/// table is not in the schema.
pub fn expand_wildcard(
    script: &str,
    dialect: Dialect,
    schema: &[Table],
    offset: usize,
) -> Option<TextEdit> {
    let (base, tokens, mut i) = token_at(script, dialect, offset)
        .or_else(|| token_at(script, dialect, offset.checked_sub(1)?))?;
    if tokens[i].kind != Kind::Other {
        // The caret right after the star.
        i = i.checked_sub(1)?;
    }
    let star = &tokens[i];
    if star.kind != Kind::Other || &script[base + star.range.start..base + star.range.end] != "*" {
        return None;
    }
    let refs = table_refs(&tokens);
    let schema_table = |t: &Tok| schema.iter().find(|s| t.names(&s.name));
    let q = |n: &str| insert_text(n, dialect);
    let qualified = i >= 2 && tokens[i - 1].kind == Kind::Dot && tokens[i - 2].is_word();
    if qualified {
        let qual = &tokens[i - 2];
        let table = refs
            .iter()
            .find(|r| r.alias.is_some_and(|a| tokens[a].names(&qual.value)))
            .and_then(|r| schema_table(&tokens[r.name]))
            .or_else(|| schema_table(qual))?;
        let prefix = &script[base + qual.range.start..base + qual.range.end];
        let columns: Vec<String> = table
            .columns
            .iter()
            .enumerate()
            .map(|(n, c)| {
                if n == 0 {
                    q(&c.name)
                } else {
                    format!("{prefix}.{}", q(&c.name))
                }
            })
            .collect();
        return Some(TextEdit {
            start: base + star.range.start,
            end: base + star.range.end,
            replacement: columns.join(", "),
        });
    }
    // Only a star of the select list, not `count(*)` or a product.
    let prev = i.checked_sub(1).map(|p| &tokens[p]);
    let in_list = prev.is_some_and(|p| {
        p.kind == Kind::Comma
            || p.keyword()
                .is_some_and(|k| matches!(k.as_str(), "select" | "distinct" | "all"))
    });
    if !in_list {
        return None;
    }
    let from: Vec<&Ref> = refs
        .iter()
        .filter(|r| r.introducer == "from" || r.introducer == "join")
        .collect();
    if from.is_empty() {
        return None;
    }
    let mut columns = Vec::new();
    for r in &from {
        let table = schema_table(&tokens[r.name])?;
        let name_tok = &tokens[r.alias.unwrap_or(r.name)];
        let prefix = &script[base + name_tok.range.start..base + name_tok.range.end];
        for c in &table.columns {
            columns.push(if from.len() == 1 {
                q(&c.name)
            } else {
                format!("{prefix}.{}", q(&c.name))
            });
        }
    }
    Some(TextEdit {
        start: base + star.range.start,
        end: base + star.range.end,
        replacement: columns.join(", "),
    })
}

/// The edits of Inline on the `WITH` query named at byte `offset` of `script` (`SPEC-0152` v2
/// behaviours 6–7), in text order, with the query's name; otherwise why not. `Ok(None)` when the
/// caret is on no name.
pub fn inline_cte(
    script: &str,
    dialect: Dialect,
    offset: usize,
) -> Result<Option<(String, Vec<TextEdit>)>, String> {
    let Some((base, tokens, at)) = token_at(script, dialect, offset)
        .or_else(|| token_at(script, dialect, offset.checked_sub(1)?))
    else {
        return Ok(None);
    };
    if !tokens[at].is_word() {
        return Ok(None);
    }
    let word = tokens[at].value.clone();
    let not_a_query = || Err(format!("`{word}` is no WITH query of this statement"));
    if tokens.first().and_then(Tok::keyword).as_deref() != Some("with") {
        return not_a_query();
    }
    let recursive = tokens.get(1).and_then(Tok::keyword).as_deref() == Some("recursive");
    // The WITH queries: name, column list, materialized, `(` and `)` token indexes.
    struct Query {
        name: usize,
        columns: bool,
        materialized: bool,
        open: usize,
        close: usize,
    }
    let matching = |open: usize| {
        let mut depth = 0usize;
        for (k, t) in tokens.iter().enumerate().skip(open) {
            match t.kind {
                Kind::Open => depth += 1,
                Kind::Close => {
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
    let mut queries = Vec::new();
    let mut j = if recursive { 2 } else { 1 };
    while tokens.get(j).is_some_and(Tok::is_word) {
        let name = j;
        j += 1;
        let columns = tokens.get(j).map(|t| t.kind) == Some(Kind::Open);
        if columns {
            let Some(c) = matching(j) else { break };
            j = c + 1;
        }
        if tokens.get(j).and_then(Tok::keyword).as_deref() != Some("as") {
            break;
        }
        j += 1;
        let mut materialized = false;
        while let Some(k) = tokens.get(j).and_then(Tok::keyword) {
            if k == "not" || k == "materialized" {
                materialized = true;
                j += 1;
            } else {
                break;
            }
        }
        if tokens.get(j).map(|t| t.kind) != Some(Kind::Open) {
            break;
        }
        let open = j;
        let Some(close) = matching(open) else { break };
        queries.push(Query {
            name,
            columns,
            materialized,
            open,
            close,
        });
        j = close + 1;
        if tokens.get(j).map(|t| t.kind) == Some(Kind::Comma) {
            j += 1;
        } else {
            break;
        }
    }
    let Some(main) = queries
        .last()
        .map(|q| q.close + 1)
        .filter(|m| *m < tokens.len())
    else {
        return not_a_query();
    };
    let qualified = |k: usize| {
        k.checked_sub(1)
            .is_some_and(|p| tokens[p].kind == Kind::Dot)
            || tokens.get(k + 1).map(|t| t.kind) == Some(Kind::Dot)
    };
    // The query named at the caret: its definition, or a word that names one.
    let Some(qi) = queries.iter().position(|q| {
        q.name == at || (!qualified(at) && at > q.close && tokens[at].names(&tokens[q.name].value))
    }) else {
        return not_a_query();
    };
    let q = &queries[qi];
    let name = tokens[q.name].value.clone();
    if q.columns {
        return Err(format!(
            "`{name}` has a column list: inline it by hand or drop the list first"
        ));
    }
    if q.materialized {
        return Err(format!(
            "`{name}` is marked MATERIALIZED or NOT MATERIALIZED, which a derived table does not keep"
        ));
    }
    if (q.open + 1..q.close).any(|k| tokens[k].names(&name)) {
        return Err(format!(
            "`{name}` reads itself (recursive): it cannot be inlined"
        ));
    }
    let refs: Vec<Ref> = table_refs(&tokens)
        .into_iter()
        .filter(|r| r.name > q.close && tokens[r.name].names(&name) && !qualified(r.name))
        .collect();
    if let Some(w) = refs
        .iter()
        .find(|r| r.introducer != "from" && r.introducer != "join")
    {
        return Err(format!(
            "`{name}` is written to ({}): only a query that is read can be inlined",
            w.introducer.to_uppercase()
        ));
    }
    if refs.is_empty() {
        return Err(format!("`{name}` is not read anywhere: nothing to inline"));
    }
    let abs = |k: usize| base + tokens[k].range.start;
    let end = |k: usize| base + tokens[k].range.end;
    let body = script[end(q.open)..abs(q.close)].trim();
    let mut edits = Vec::new();
    // The definition: with `WITH` when it is the only one, else with one comma.
    let removal = if queries.len() == 1 {
        abs(0)..abs(main)
    } else if qi + 1 < queries.len() {
        abs(q.name)..abs(queries[qi + 1].name)
    } else {
        end(queries[qi - 1].close)..end(q.close)
    };
    edits.push(TextEdit {
        start: removal.start,
        end: removal.end,
        replacement: String::new(),
    });
    for r in &refs {
        let text = &script[abs(r.name)..end(r.name)];
        edits.push(TextEdit {
            start: abs(r.name),
            end: end(r.name),
            replacement: match r.alias {
                Some(_) => format!("({body})"),
                None => format!("({body}) {text}"),
            },
        });
    }
    edits.sort_by_key(|e| e.start);
    Ok(Some((name, edits)))
}

/// Applies non-overlapping `edits` (in text order) to `text`.
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for e in edits {
        out.push_str(&text[at..e.start]);
        out.push_str(&e.replacement);
        at = e.end;
    }
    out.push_str(&text[at..]);
    out
}

/// The dialect a SQL file's path names (`SPEC-0019` v2 behaviour 8): a name ending in
/// `.<dialect>.sql`, else the nearest folder named after a dialect; `None` when nothing does.
pub fn file_dialect(path: &std::path::Path) -> Option<Dialect> {
    let named = |word: &str| match word.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" | "pgsql" => Some(Dialect::PostgreSql),
        "mysql" => Some(Dialect::MySql),
        "mariadb" => Some(Dialect::MariaDb),
        "sqlite" | "sqlite3" => Some(Dialect::Sqlite),
        "sqlserver" | "mssql" | "tsql" => Some(Dialect::SqlServer),
        "oracle" | "plsql" => Some(Dialect::Oracle),
        _ => None,
    };
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    if let Some((_, last)) = stem.rsplit_once('.')
        && let Some(d) = named(last)
    {
        return Some(d);
    }
    path.parent()?
        .components()
        .rev()
        .find_map(|c| named(&c.as_os_str().to_string_lossy()))
}

/// A constraint or index whose name embeds the renamed name (`SPEC-0019` v2 behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DerivedRename {
    /// `primary key`, `unique`, `check`, `foreign key` or `index`.
    pub kind: String,
    /// Current name.
    pub old: String,
    /// Offered name.
    pub new: String,
    /// The statement, when the database can rename it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ddl: Option<String>,
    /// Why it cannot be renamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `name` with each whole `_`-separated run of parts equal to `old` (case ignored) replaced by `new`.
fn replace_part(name: &str, old: &str, new: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('_').collect();
    let want: Vec<String> = old.split('_').map(str::to_ascii_lowercase).collect();
    if want.iter().any(String::is_empty) || want.len() > parts.len() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    let mut hit = false;
    while i < parts.len() {
        let run = parts.get(i..i + want.len());
        if run.is_some_and(|r| {
            r.iter()
                .zip(&want)
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
        }) {
            out.push(new.to_owned());
            i += want.len();
            hit = true;
        } else {
            out.push(parts[i].to_owned());
            i += 1;
        }
    }
    hit.then(|| out.join("_"))
}

/// The constraints of `constraints` and indexes of `schema` on `table` whose names embed the renamed
/// name — `column` when given, else `table` — with the new name and the statement of `dialect`.
/// Offers whose new name is taken are dropped.
pub fn derived_renames(
    dialect: Dialect,
    schema: &[Table],
    constraints: &[ConstraintName],
    table: &str,
    column: Option<&str>,
    new_name: &str,
) -> Vec<DerivedRename> {
    let old = column.unwrap_or(table);
    if new_name.is_empty() || old == new_name {
        return Vec::new();
    }
    // Statements run after the main rename, so a renamed table goes by its new name.
    let table_now = if column.is_some() { table } else { new_name };
    let q = |n: &str| insert_text(n, dialect);
    let mut names: Vec<(String, String)> = constraints
        .iter()
        .filter(|c| c.table == table)
        .map(|c| (c.kind.clone(), c.name.clone()))
        .collect();
    let indexes = schema
        .iter()
        .find(|t| t.name == table)
        .map(|t| t.indexes.clone())
        .unwrap_or_default();
    for i in &indexes {
        if i.name != "PRIMARY" && !names.iter().any(|(_, n)| n == &i.name) {
            names.push(("index".into(), i.name.clone()));
        }
    }
    let taken = |n: &str| {
        constraints.iter().any(|c| c.name.eq_ignore_ascii_case(n))
            || schema
                .iter()
                .any(|t| t.indexes.iter().any(|i| i.name.eq_ignore_ascii_case(n)))
    };
    let mut out = Vec::new();
    for (kind, name) in names {
        let Some(new) = replace_part(&name, old, new_name) else {
            continue;
        };
        if taken(&new) {
            continue;
        }
        let is_index = kind == "index";
        let ddl: Result<String, &str> = match dialect {
            Dialect::PostgreSql | Dialect::Oracle if is_index => {
                Ok(format!("ALTER INDEX {} RENAME TO {}", q(&name), q(&new)))
            }
            Dialect::PostgreSql | Dialect::Oracle => Ok(format!(
                "ALTER TABLE {} RENAME CONSTRAINT {} TO {}",
                q(table_now),
                q(&name),
                q(&new)
            )),
            Dialect::MySql | Dialect::MariaDb if is_index || kind == "unique" => Ok(format!(
                "ALTER TABLE {} RENAME INDEX {} TO {}",
                q(table_now),
                q(&name),
                q(&new)
            )),
            Dialect::MySql | Dialect::MariaDb => Err(
                "MySQL and MariaDB cannot rename a foreign key or check constraint without dropping and adding it again",
            ),
            Dialect::Sqlite => Err("SQLite cannot rename constraints or indexes"),
            Dialect::DuckDb => Err("DuckDB cannot rename constraints or indexes"),
            Dialect::SqlServer if is_index => Ok(format!(
                "EXEC sp_rename {}, {}, 'INDEX'",
                literal(&format!("{table_now}.{name}")),
                literal(&new)
            )),
            Dialect::SqlServer => Ok(format!(
                "EXEC sp_rename {}, {}, 'OBJECT'",
                literal(&name),
                literal(&new)
            )),
        };
        out.push(DerivedRename {
            kind,
            old: name,
            new,
            ddl: ddl.as_ref().ok().cloned(),
            reason: ddl.err().map(str::to_owned),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Column;

    /// SPEC-0152 v2 AK-5.
    #[test]
    fn inline_a_common_table_expression() {
        let d = Dialect::PostgreSql;
        let run = |sql: &str, at: &str| {
            let offset = sql.find(at).unwrap();
            inline_cte(sql, d, offset).map(|r| r.map(|(n, e)| (n, apply_edits(sql, &e))))
        };
        let sql = "with big as (select * from item where qty > 5) select b.id from big b join big on true";
        // From the definition and from a reference: the same result.
        let want = (
            "big".to_owned(),
            "select b.id from (select * from item where qty > 5) b join (select * from item where qty > 5) big on true".to_owned(),
        );
        assert_eq!(run(sql, "big as").unwrap(), Some(want.clone()));
        assert_eq!(run(sql, "big b").unwrap(), Some(want));
        // One of two, read by the later one; the other keeps its WITH.
        let two = "WITH a AS (SELECT 1 AS x), b AS (SELECT x FROM a) SELECT * FROM b, a";
        assert_eq!(
            run(two, "a AS").unwrap().unwrap().1,
            "WITH b AS (SELECT x FROM (SELECT 1 AS x) a) SELECT * FROM b, (SELECT 1 AS x) a"
        );
        assert_eq!(
            run(two, "b AS").unwrap().unwrap().1,
            "WITH a AS (SELECT 1 AS x) SELECT * FROM (SELECT x FROM a) b, a"
        );
        // Refusals.
        let refused = |sql: &str, at: &str, why: &str| {
            let e = run(sql, at).unwrap_err();
            assert!(e.contains(why), "{sql}: {e}");
        };
        refused(
            "with recursive n as (select 1 union all select 1 from n) select * from n",
            "n as",
            "recursive",
        );
        refused(
            "with c (x) as (select 1) select * from c",
            "c (x)",
            "column list",
        );
        refused(
            "with c as materialized (select 1) select * from c",
            "c as",
            "MATERIALIZED",
        );
        refused(
            "with c as (select 1) update c set x = 1",
            "c as",
            "written to (UPDATE)",
        );
        refused("with c as (select 1) select 2", "c as", "not read");
        refused("select * from item", "item", "no WITH query");
        refused(
            "with c as (select 1) select * from c",
            "select 1",
            "`select` is no WITH query",
        );
        assert_eq!(inline_cte("  ", d, 1), Ok(None));
    }

    fn table(name: &str, columns: &[&str]) -> Table {
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: name.into(),
            kind: "table".into(),
            columns: columns
                .iter()
                .map(|c| Column {
                    name: c.to_string(),
                    data_type: "text".into(),
                    nullable: true,
                    default: None,
                    generated: None,
                    expression: None,
                    identity: None,
                    comment: None,
                })
                .collect(),
            primary_key: vec![],
            foreign_keys: vec![],
            indexes: vec![],
            unreadable: None,
            constraints: Vec::new(),
            comment: None,
        }
    }

    fn schema() -> Vec<Table> {
        vec![
            table("item", &["sku", "name", "unit_price_cents"]),
            table("stock", &["warehouse_id", "sku", "qty"]),
            table("warehouse", &["id", "city"]),
        ]
    }

    fn schema_with_more() -> Vec<Table> {
        let mut s = schema();
        s[1].indexes.push(crate::introspect::Index {
            name: "stock_sku_idx".into(),
            unique: false,
            columns: vec!["sku".into()],
            condition: None,
            collations: Vec::new(),
            method: None,
            orders: Vec::new(),
            prefixes: Vec::new(),
        });
        s[1].columns[2].nullable = false;
        s[1].columns[2].default = Some("0".into());
        let mut v = table("stock_value", &["sku", "value"]);
        v.kind = "view".into();
        s.push(v);
        s
    }

    #[test]
    fn create_table_column_definitions_are_renamed() {
        // D45: the column definition and its check in schema.sql stayed old.
        let sql = "create table item (\n    sku text primary key,\n    name text not null,\n    unit_price_cents integer not null check (unit_price_cents >= 0),\n    unique (unit_price_cents, sku)\n);\ncreate table stock (\n    sku text not null references item (sku),\n    unit_price_cents integer\n);\ncreate view v as select i.unit_price_cents from item i;";
        let r = rename(
            sql,
            Dialect::PostgreSql,
            &schema(),
            "item",
            Some("unit_price_cents"),
            "price_cents",
        )
        .unwrap();
        assert!(
            r.sql
                .contains("    price_cents integer not null check (price_cents >= 0),"),
            "{}",
            r.sql
        );
        assert!(r.sql.contains("unique (price_cents, sku)"), "{}", r.sql);
        assert!(
            r.sql.contains("select i.price_cents from item i"),
            "{}",
            r.sql
        );
        // Another table's column of the same name stays.
        assert!(
            r.sql.contains("    unit_price_cents integer\n);"),
            "{}",
            r.sql
        );
        assert_eq!(r.edits.len(), 4, "{}", r.sql);
        // A column referenced from another table follows in its REFERENCES list only.
        let r = rename(
            sql,
            Dialect::PostgreSql,
            &schema(),
            "item",
            Some("sku"),
            "code",
        )
        .unwrap();
        assert!(r.sql.contains("    code text primary key,"), "{}", r.sql);
        assert!(r.sql.contains("references item (code)"), "{}", r.sql);
        assert!(
            r.sql.contains("    sku text not null references"),
            "{}",
            r.sql
        );
    }

    #[test]
    fn views_tables_and_columns_get_their_dialects_statement() {
        let s = schema_with_more();
        let ddl = |d, t: &str, c: Option<&str>| rename(";", d, &s, t, c, "x").map(|r| r.ddl);
        assert_eq!(
            ddl(Dialect::PostgreSql, "stock_value", None).unwrap(),
            "ALTER VIEW stock_value RENAME TO x"
        );
        assert_eq!(
            ddl(Dialect::MySql, "stock_value", None).unwrap(),
            "RENAME TABLE stock_value TO x"
        );
        assert!(ddl(Dialect::Sqlite, "stock_value", None).is_err());
        assert!(ddl(Dialect::MySql, "stock_value", Some("sku")).is_err());
        assert_eq!(
            ddl(Dialect::SqlServer, "stock", Some("qty")).unwrap(),
            "EXEC sp_rename 'stock.qty', 'x', 'COLUMN'"
        );
        assert_eq!(
            ddl(Dialect::PostgreSql, "stock", None).unwrap(),
            "ALTER TABLE stock RENAME TO x"
        );
    }

    #[test]
    fn indexes_are_renamed_in_index_statements_only() {
        let s = schema_with_more();
        let sql = "CREATE INDEX stock_sku_idx ON stock (sku);\nSELECT stock_sku_idx FROM t;\nDROP INDEX stock_sku_idx;";
        let r = rename_index(
            sql,
            Dialect::PostgreSql,
            &s,
            "stock_sku_idx",
            "stock_by_sku",
        )
        .unwrap();
        assert_eq!(r.ddl, "ALTER INDEX stock_sku_idx RENAME TO stock_by_sku");
        assert_eq!(r.edits.len(), 2);
        assert!(r.sql.contains("SELECT stock_sku_idx FROM t"));
        let my = rename_index("", Dialect::MySql, &s, "stock_sku_idx", "i2").unwrap();
        assert_eq!(my.ddl, "ALTER TABLE stock RENAME INDEX stock_sku_idx TO i2");
        assert!(rename_index("", Dialect::Sqlite, &s, "stock_sku_idx", "i2").is_err());
        assert_eq!(
            rename_index("", Dialect::PostgreSql, &s, "nope", "i2"),
            Err(RenameError::UnknownIndex("nope".into()))
        );
        assert_eq!(
            named_object_at(sql, Dialect::PostgreSql, &s, 15),
            Some((
                Refactoring::RenameIndex("stock_sku_idx".into()),
                "stock".into()
            ))
        );
    }

    #[test]
    fn routines_are_renamed_at_definitions_and_calls() {
        let sql = "CREATE FUNCTION add_tax(p int) RETURNS int AS $$ SELECT p $$ LANGUAGE sql;\nSELECT add_tax(qty), count(*) FROM stock;\nCALL add_tax(1);";
        let r = rename_routine(sql, Dialect::PostgreSql, "add_tax", "with_tax").unwrap();
        assert_eq!(r.ddl, "ALTER ROUTINE add_tax RENAME TO with_tax");
        assert_eq!(r.edits.len(), 3, "{}", r.sql);
        assert!(rename_routine(sql, Dialect::MySql, "add_tax", "x").is_err());
        let s = schema_with_more();
        let at = sql.find("add_tax(qty)").unwrap() + 2;
        assert_eq!(
            named_object_at(sql, Dialect::PostgreSql, &s, at),
            Some((Refactoring::RenameRoutine("add_tax".into()), String::new()))
        );
        let count = sql.find("count(").unwrap() + 1;
        assert_eq!(named_object_at(sql, Dialect::PostgreSql, &s, count), None);
    }

    #[test]
    fn column_types_change_keeping_nullability() {
        let s = schema_with_more();
        let ddl = |d, ty: &str| change_type_ddl(d, &s, "stock", "qty", ty);
        assert_eq!(
            ddl(Dialect::PostgreSql, "bigint").unwrap(),
            "ALTER TABLE stock ALTER COLUMN qty TYPE bigint"
        );
        assert_eq!(
            ddl(Dialect::MySql, "numeric(12, 2)").unwrap(),
            "ALTER TABLE stock MODIFY COLUMN qty numeric(12, 2) NOT NULL DEFAULT 0"
        );
        assert_eq!(
            ddl(Dialect::SqlServer, "bigint").unwrap(),
            "ALTER TABLE stock ALTER COLUMN qty bigint NOT NULL"
        );
        assert!(ddl(Dialect::Sqlite, "bigint").is_err());
        assert_eq!(
            ddl(Dialect::PostgreSql, "int; DROP TABLE x"),
            Err(RenameError::BadType("int; DROP TABLE x".into()))
        );
        assert!(change_type_ddl(Dialect::PostgreSql, &s, "stock_value", "sku", "int").is_err());
        let r = refactor(
            "x",
            Dialect::PostgreSql,
            &s,
            "stock",
            Some("qty"),
            &Refactoring::ChangeType,
            "int",
        )
        .unwrap();
        assert!(r.edits.is_empty() && r.ddl.ends_with("TYPE int"));
    }

    fn sql(script: &str, column: Option<&str>, new: &str, d: Dialect) -> String {
        rename(script, d, &schema(), "item", column, new)
            .unwrap()
            .sql
    }

    #[test]
    fn renames_table_references_only() {
        let script = "select item.sku, 'item', item_name from item join stock item2 on item2.sku = item.sku; \
                      -- item\n insert into item (sku) values ('x'); update item set name = 'y'; \
                      create table t (s text references item (sku)); select * from \"item\"; \
                      select i.name from stock item, item i";
        assert_eq!(
            sql(script, None, "article", Dialect::PostgreSql),
            "select article.sku, 'item', item_name from article join stock item2 on item2.sku = article.sku; \
             -- item\n insert into article (sku) values ('x'); update article set name = 'y'; \
             create table t (s text references article (sku)); select * from \"article\"; \
             select i.name from stock item, article i"
        );
    }

    #[test]
    fn renames_column_references() {
        let script = "select i.sku, item.sku, s.sku from item i join stock s on s.sku = i.sku; \
                      select sku, 1 as sku from item where sku > 'A'; \
                      insert into item (sku, name) values ('a', 'b'); \
                      create index item_sku on item (sku); \
                      create table t (sku text, foreign key (sku) references item (sku)); \
                      update item set sku = upper(sku)";
        assert_eq!(
            sql(script, Some("sku"), "code", Dialect::Sqlite),
            "select i.code, item.code, s.sku from item i join stock s on s.sku = i.code; \
             select code, 1 as sku from item where code > 'A'; \
             insert into item (code, name) values ('a', 'b'); \
             create index item_sku on item (code); \
             create table t (sku text, foreign key (sku) references item (code)); \
             update item set code = upper(code)"
        );
    }

    #[test]
    fn reports_ambiguous_places() {
        let script = "select sku from item join stock using (warehouse_id)";
        let r = rename(
            script,
            Dialect::PostgreSql,
            &schema(),
            "item",
            Some("sku"),
            "code",
        )
        .unwrap();
        assert!(r.edits.is_empty());
        assert_eq!(r.ambiguous.len(), 1);
        assert_eq!(&script[r.ambiguous[0].start..r.ambiguous[0].end], "sku");
        assert!(r.ambiguous[0].reason.contains("`stock`"));
        // INSERT column list and SET target are decided even with another table in the statement.
        let r = rename(
            "insert into item (sku) select sku from stock",
            Dialect::PostgreSql,
            &schema(),
            "item",
            Some("sku"),
            "code",
        )
        .unwrap();
        assert_eq!(r.sql, "insert into item (code) select sku from stock");
        assert_eq!(r.ambiguous.len(), 1);
    }

    #[test]
    fn ddl_quoting_and_errors() {
        let r = rename("", Dialect::MySql, &schema(), "item", None, "Order Line").unwrap();
        assert_eq!(r.ddl, "ALTER TABLE item RENAME TO `Order Line`");
        let r = rename(
            "",
            Dialect::PostgreSql,
            &schema(),
            "item",
            Some("sku"),
            "code",
        )
        .unwrap();
        assert_eq!(r.ddl, "ALTER TABLE item RENAME COLUMN sku TO code");
        assert_eq!(
            sql("select `item`.sku from `item`", None, "a", Dialect::MySql),
            "select `a`.sku from `a`"
        );
        assert_eq!(
            sql("select * from item", None, "select", Dialect::PostgreSql),
            "select * from \"select\""
        );
        let err = |t, c, n| rename("", Dialect::Sqlite, &schema(), t, c, n).unwrap_err();
        assert_eq!(
            err("nope", None, "x"),
            RenameError::UnknownTable("nope".into())
        );
        assert_eq!(
            err("item", Some("nope"), "x"),
            RenameError::UnknownColumn("item".into(), "nope".into())
        );
        assert_eq!(
            err("item", None, "stock"),
            RenameError::Exists("stock".into())
        );
        assert_eq!(
            err("item", Some("sku"), "name"),
            RenameError::Exists("item.name".into())
        );
        assert_eq!(err("item", None, ""), RenameError::EmptyName);
    }

    #[test]
    fn literals_comments_and_dollar_bodies_stay() {
        let script = "select $$ from item $$, E'item', /* item */ x from item";
        assert_eq!(
            sql(script, None, "a", Dialect::PostgreSql),
            "select $$ from item $$, E'item', /* item */ x from a"
        );
    }

    #[test]
    fn finds_the_object_at_the_caret() {
        let d = Dialect::Sqlite;
        let script = "select i.name, qty, sku from item i join stock s on s.sku = i.sku";
        let at = |w: &str, n: usize| script.match_indices(w).nth(n).unwrap().0 + 1;
        let object = |o: usize| object_at(script, d, &schema(), o);
        let col = |t: &str, c: &str| {
            Ok(SqlObject {
                table: t.into(),
                column: Some(c.into()),
            })
        };
        assert_eq!(object(at("name", 0)), col("item", "name"));
        assert_eq!(object(at("qty", 0)), col("stock", "qty"));
        assert_eq!(
            object(at("item", 0)),
            Ok(SqlObject {
                table: "item".into(),
                column: None
            })
        );
        assert_eq!(object(at("s.sku", 0) + 2), col("stock", "sku"));
        assert!(object(at("sku", 0)).unwrap_err().contains("qualify"));
        assert!(object(at(" i ", 0)).unwrap_err().contains("alias"));
        assert!(object(at("select", 0)).is_err());
    }

    #[test]
    fn expands_the_wildcard_of_a_select_list() {
        let d = Dialect::Sqlite;
        let one = "select * from item where sku = 'a'";
        let e = expand_wildcard(one, d, &schema(), 7).unwrap();
        assert_eq!(
            apply_edits(one, &[e]),
            "select sku, name, unit_price_cents from item where sku = 'a'"
        );
        let two = "select * from stock s join warehouse on warehouse.id = s.warehouse_id";
        let e = expand_wildcard(two, d, &schema(), 8).unwrap();
        assert_eq!(
            apply_edits(two, &[e]),
            "select s.warehouse_id, s.sku, s.qty, warehouse.id, warehouse.city from stock s join warehouse on warehouse.id = s.warehouse_id"
        );
        let qualified = "select s.*, city from stock s join warehouse w on w.id = s.warehouse_id";
        let e = expand_wildcard(qualified, d, &schema(), 9).unwrap();
        assert!(
            apply_edits(qualified, &[e]).starts_with("select s.warehouse_id, s.sku, s.qty, city")
        );
        assert_eq!(
            expand_wildcard("select count(*) from item", d, &schema(), 14),
            None
        );
        assert_eq!(
            expand_wildcard("select * from nowhere", d, &schema(), 7),
            None
        );
    }

    /// SPEC-0019/AK-7: dialect of a file from its name and folders.
    #[test]
    fn dialect_of_a_file_from_its_path() {
        use std::path::Path;
        assert_eq!(
            file_dialect(Path::new("mysql/schema.sql")),
            Some(Dialect::MySql)
        );
        assert_eq!(
            file_dialect(Path::new("db/seed-movements.postgresql.sql")),
            Some(Dialect::PostgreSql)
        );
        assert_eq!(
            file_dialect(Path::new("sqlite/x.mariadb.sql")),
            Some(Dialect::MariaDb),
            "the file name wins over the folder"
        );
        assert_eq!(
            file_dialect(Path::new("Oracle/sub/a.sql")),
            Some(Dialect::Oracle)
        );
        assert_eq!(file_dialect(Path::new("schema.sql")), None);
        assert_eq!(
            file_dialect(Path::new("queries/my.sql")),
            None,
            "`my` is no dialect"
        );
    }

    /// SPEC-0019/AK-8: derived constraint and index names per dialect.
    #[test]
    fn derived_constraint_and_index_names() {
        let mut item = crate::Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: "item".into(),
            kind: "table".into(),
            columns: vec![],
            primary_key: vec!["sku".into()],
            foreign_keys: vec![],
            indexes: vec![
                crate::Index {
                    name: "item_pkey".into(),
                    unique: true,
                    columns: vec!["sku".into()],
                    condition: None,
                    collations: Vec::new(),
                    method: None,
                    orders: Vec::new(),
                    prefixes: Vec::new(),
                },
                crate::Index {
                    name: "idx_item_unit_price_cents".into(),
                    unique: false,
                    columns: vec!["unit_price_cents".into()],
                    condition: None,
                    collations: Vec::new(),
                    method: None,
                    orders: Vec::new(),
                    prefixes: Vec::new(),
                },
            ],
            unreadable: None,
            constraints: Vec::new(),
            comment: None,
        };
        let c = |name: &str, kind: &str| ConstraintName {
            table: "item".into(),
            name: name.into(),
            kind: kind.into(),
        };
        let cons = [
            c("item_pkey", "primary key"),
            c("item_unit_price_cents_check", "check"),
            c("items_unit_price_centsx", "check"),
        ];
        let pg = derived_renames(
            Dialect::PostgreSql,
            std::slice::from_ref(&item),
            &cons,
            "item",
            Some("unit_price_cents"),
            "price_cents",
        );
        let pairs: Vec<(&str, &str, Option<&str>)> = pg
            .iter()
            .map(|d| (d.old.as_str(), d.new.as_str(), d.ddl.as_deref()))
            .collect();
        assert_eq!(
            pairs,
            [
                (
                    "item_unit_price_cents_check",
                    "item_price_cents_check",
                    Some(
                        "ALTER TABLE item RENAME CONSTRAINT item_unit_price_cents_check TO item_price_cents_check"
                    )
                ),
                (
                    "idx_item_unit_price_cents",
                    "idx_item_price_cents",
                    Some("ALTER INDEX idx_item_unit_price_cents RENAME TO idx_item_price_cents")
                ),
            ]
        );
        let table = derived_renames(
            Dialect::PostgreSql,
            std::slice::from_ref(&item),
            &cons,
            "item",
            None,
            "article",
        );
        assert_eq!(
            table[0].ddl.as_deref(),
            Some("ALTER TABLE article RENAME CONSTRAINT item_pkey TO article_pkey"),
            "runs after the table rename"
        );
        assert!(
            table.iter().all(|d| d.old != "items_unit_price_centsx"),
            "only whole parts"
        );
        let my = derived_renames(
            Dialect::MySql,
            std::slice::from_ref(&item),
            &cons,
            "item",
            Some("unit_price_cents"),
            "price_cents",
        );
        assert!(
            my[0].ddl.is_none()
                && my[0]
                    .reason
                    .as_deref()
                    .unwrap()
                    .contains("check constraint")
        );
        assert_eq!(
            my[1].ddl.as_deref(),
            Some("ALTER TABLE item RENAME INDEX idx_item_unit_price_cents TO idx_item_price_cents")
        );
        let lite = derived_renames(
            Dialect::Sqlite,
            std::slice::from_ref(&item),
            &[],
            "item",
            Some("unit_price_cents"),
            "p",
        );
        assert_eq!(lite.len(), 1);
        assert!(lite[0].reason.as_deref().unwrap().starts_with("SQLite"));
        let ss = derived_renames(
            Dialect::SqlServer,
            std::slice::from_ref(&item),
            &cons,
            "item",
            Some("unit_price_cents"),
            "p",
        );
        assert_eq!(
            ss[0].ddl.as_deref(),
            Some("EXEC sp_rename 'item_unit_price_cents_check', 'item_p_check', 'OBJECT'")
        );
        assert_eq!(
            ss[1].ddl.as_deref(),
            Some("EXEC sp_rename 'item.idx_item_unit_price_cents', 'idx_item_p', 'INDEX'")
        );
        item.indexes.push(crate::Index {
            name: "idx_item_price_cents".into(),
            unique: false,
            columns: vec![],
            condition: None,
            collations: Vec::new(),
            method: None,
            orders: Vec::new(),
            prefixes: Vec::new(),
        });
        let taken = derived_renames(
            Dialect::PostgreSql,
            std::slice::from_ref(&item),
            &cons,
            "item",
            Some("unit_price_cents"),
            "price_cents",
        );
        assert_eq!(taken.len(), 1, "a taken new name is not offered");
        assert!(
            derived_renames(
                Dialect::PostgreSql,
                &[item],
                &cons,
                "item",
                Some("sku"),
                "sku"
            )
            .is_empty()
        );
    }

    /// SPEC-0164 v3 AK-6 (behaviour 8): statements and refusals per dialect; the places in files;
    /// the name in a literal is listed, not changed.
    #[test]
    fn sequences_are_renamed_per_dialect() {
        let r = |d| rename_sequence("", d, "order_seq", "orders_seq").map(|r| r.ddl);
        assert_eq!(
            r(Dialect::PostgreSql).unwrap(),
            "ALTER SEQUENCE order_seq RENAME TO orders_seq"
        );
        assert_eq!(
            r(Dialect::MariaDb).unwrap(),
            "RENAME TABLE order_seq TO orders_seq"
        );
        assert_eq!(
            r(Dialect::Oracle).unwrap(),
            "RENAME order_seq TO orders_seq"
        );
        assert_eq!(
            r(Dialect::SqlServer).unwrap(),
            "EXEC sp_rename 'order_seq', 'orders_seq', 'OBJECT'"
        );
        for d in [Dialect::MySql, Dialect::Sqlite, Dialect::DuckDb] {
            assert!(matches!(r(d), Err(RenameError::Unsupported(_))), "{d:?}");
        }
        let script = "create sequence order_seq start 1;\n\
                      alter table orders alter column id set default nextval('order_seq');\n\
                      select next value for order_seq;\n\
                      select order_seq.nextval from dual;\n\
                      select order_seq from order_seq_log;";
        let out = rename_sequence(script, Dialect::PostgreSql, "order_seq", "orders_seq").unwrap();
        assert!(
            out.sql.contains("create sequence orders_seq start 1"),
            "{}",
            out.sql
        );
        assert!(out.sql.contains("next value for orders_seq"), "{}", out.sql);
        assert!(out.sql.contains("orders_seq.nextval"), "{}", out.sql);
        // v5: on PostgreSQL the literal of `nextval` names the sequence (behaviour 8).
        assert!(out.sql.contains("nextval('orders_seq')"), "{}", out.sql);
        assert!(
            out.sql.contains("select order_seq from order_seq_log"),
            "{}",
            out.sql
        );
        assert!(out.ambiguous.is_empty(), "{:?}", out.ambiguous);
        // Other dialects have no such literal: it stays listed.
        let out = rename_sequence(script, Dialect::Oracle, "order_seq", "orders_seq").unwrap();
        assert!(out.sql.contains("nextval('order_seq')"), "{}", out.sql);
        assert_eq!(out.ambiguous.len(), 1);
        assert_eq!(
            &script[out.ambiguous[0].start..out.ambiguous[0].end],
            "'order_seq'"
        );
    }

    /// SPEC-0164 v5 AK-13 (OBS-0070 V3): a sequence literal changes where it names the renamed
    /// sequence without doubt; another schema's is left alone; the doubtful ones are listed.
    #[test]
    fn sequence_literals_follow_when_they_name_the_sequence() {
        let pg = Dialect::PostgreSql;
        let run = |script: &str, home: Option<&str>, new: &str| {
            let r = rename_sequence_in(script, pg, "pd_seq", home, new).unwrap();
            let listed: Vec<String> = r
                .ambiguous
                .iter()
                .map(|a| script[a.start..a.end].to_owned())
                .collect();
            (r.sql, listed)
        };
        // Changed: the arguments of nextval, currval and setval and a regclass cast, folded and
        // quoted forms, the sequence's schema, a body in dollar quotes.
        let script = "insert into pd_note values (nextval('pd_seq'), 'pd_seq');\n\
                      alter table pd_note alter id set default nextval('pd_seq'::regclass);\n\
                      select setval('PD_SEQ', 1), currval( 'public.pd_seq' ), nextval('\"pd_seq\"');\n\
                      select pg_catalog.nextval('pd_seq'), nextval('other.pd_seq');\n\
                      do $$ begin perform nextval('pd_seq'); end $$;\n\
                      -- nextval('pd_seq') in a comment\n\
                      select E'pd_seq', nextval('\"PD_SEQ\"'), nextval('db.public.pd_seq');";
        let (sql, listed) = run(script, Some("public"), "pd_seq2");
        assert_eq!(
            sql,
            "insert into pd_note values (nextval('pd_seq2'), 'pd_seq');\n\
             alter table pd_note alter id set default nextval('pd_seq2'::regclass);\n\
             select setval('pd_seq2', 1), currval( 'public.pd_seq2' ), nextval('\"pd_seq2\"');\n\
             select pg_catalog.nextval('pd_seq2'), nextval('other.pd_seq');\n\
             do $$ begin perform nextval('pd_seq2'); end $$;\n\
             -- nextval('pd_seq') in a comment\n\
             select E'pd_seq', nextval('\"PD_SEQ\"'), nextval('db.public.pd_seq');"
        );
        // Listed: a plain string, an escape string, another case in quotes, a database part.
        assert_eq!(
            listed,
            ["'pd_seq'", "'pd_seq'", "'\"PD_SEQ\"'", "'db.public.pd_seq'"]
        );
        // The schema not known: a qualified literal is listed, an unqualified one changes.
        let (sql, listed) = run(
            "select nextval('public.pd_seq'), nextval('pd_seq');",
            None,
            "pd_seq2",
        );
        assert_eq!(sql, "select nextval('public.pd_seq'), nextval('pd_seq2');");
        assert_eq!(listed, ["'public.pd_seq'"]);
        // After the file sets another search path, an unqualified literal is listed; after its own
        // schema, or a reset, it changes.
        let script = "select nextval('pd_seq');\n\
                      set search_path to other, public;\n\
                      select nextval('pd_seq');\n\
                      set local search_path = 'public';\n\
                      select nextval('pd_seq');\n\
                      set schema '$user';\n\
                      select nextval('pd_seq');\n\
                      reset search_path;\n\
                      select nextval('pd_seq');";
        let (sql, listed) = run(script, Some("public"), "pd_seq2");
        assert_eq!(sql.matches("nextval('pd_seq2')").count(), 3, "{sql}");
        assert_eq!(listed.len(), 2, "{listed:?}");
        let r = rename_sequence_in(script, pg, "pd_seq", Some("public"), "pd_seq2").unwrap();
        assert!(
            r.ambiguous[0].reason.contains("search path"),
            "{:?}",
            r.ambiguous
        );
        // A new name that needs quotes is quoted inside the literal, a quote mark doubled.
        let (sql, _) = run("select nextval('pd_seq');", Some("public"), "Pd Seq's");
        assert_eq!(sql, "select nextval('\"Pd Seq''s\"');");
        // The edits are places like the others.
        let r = rename_sequence_in("select nextval('pd_seq');", pg, "pd_seq", None, "s2").unwrap();
        assert_eq!(r.edits.len(), 1);
        assert_eq!((r.edits[0].start, r.edits[0].end), (16, 22));
    }

    /// SPEC-0164 v5 AK-15, AK-16 (OBS-0070 V4): each refactoring has a way back, and its statement
    /// takes the database change back.
    #[test]
    fn inverse_statements_take_each_refactoring_back() {
        let s = schema_with_more();
        let pg = Dialect::PostgreSql;
        let inverse = |table: &str, column: Option<&str>, what: Refactoring, input: &str| {
            inverse_ddl(pg, &s, table, column, &what, input).unwrap()
        };
        assert_eq!(
            inverse("stock", Some("qty"), Refactoring::Rename, "amount"),
            "ALTER TABLE stock RENAME COLUMN amount TO qty"
        );
        assert_eq!(
            inverse("stock", None, Refactoring::Rename, "stock2"),
            "ALTER TABLE stock2 RENAME TO stock"
        );
        // A view stays a view by its new name.
        assert_eq!(
            inverse("stock_value", None, Refactoring::Rename, "sv"),
            "ALTER VIEW sv RENAME TO stock_value"
        );
        assert_eq!(
            inverse(
                "stock",
                None,
                Refactoring::RenameIndex("stock_sku_idx".into()),
                "stock_sku_i"
            ),
            "ALTER INDEX stock_sku_i RENAME TO stock_sku_idx"
        );
        assert_eq!(
            inverse(
                "",
                None,
                Refactoring::RenameSequence("pd_seq".into()),
                "pd_seq2"
            ),
            "ALTER SEQUENCE pd_seq2 RENAME TO pd_seq"
        );
        assert_eq!(
            inverse(
                "stock",
                None,
                Refactoring::RenameConstraint {
                    name: "stock_qty_check".into(),
                    kind: "check".into()
                },
                "stock_amount_check"
            ),
            "ALTER TABLE stock RENAME CONSTRAINT stock_amount_check TO stock_qty_check"
        );
        // A type change goes back to the type the column had, keeping NOT NULL and the default
        // where the dialect restates the column.
        assert_eq!(
            inverse("stock", Some("qty"), Refactoring::ChangeType, "bigint"),
            "ALTER TABLE stock ALTER COLUMN qty TYPE text"
        );
        assert_eq!(
            inverse_ddl(
                Dialect::MySql,
                &s,
                "stock",
                Some("qty"),
                &Refactoring::ChangeType,
                "bigint"
            )
            .unwrap(),
            "ALTER TABLE stock MODIFY COLUMN qty text NOT NULL DEFAULT 0"
        );
        let back = way_back(&s, "stock", Some("qty"), &Refactoring::Rename, "amount").unwrap();
        assert_eq!(
            (
                back.table.as_str(),
                back.column.as_deref(),
                back.input.as_str()
            ),
            ("stock", Some("amount"), "qty")
        );
        assert!(way_back(&s, "gone", Some("x"), &Refactoring::ChangeType, "int").is_none());
    }

    /// SPEC-0164 v3 AK-6 (behaviour 9).
    #[test]
    fn constraints_are_renamed_on_their_own() {
        let script = "create table item (sku text, constraint item_sku_key unique (sku));\nselect 'item_sku_key';";
        let r =
            |d, kind| rename_constraint(script, d, "item", "item_sku_key", kind, "item_code_key");
        let pg = r(Dialect::PostgreSql, "unique").unwrap();
        assert_eq!(
            pg.ddl,
            "ALTER TABLE item RENAME CONSTRAINT item_sku_key TO item_code_key"
        );
        assert!(
            pg.sql.contains("constraint item_code_key unique"),
            "{}",
            pg.sql
        );
        assert!(pg.sql.contains("select 'item_sku_key'"), "{}", pg.sql);
        assert_eq!(
            r(Dialect::MySql, "unique").unwrap().ddl,
            "ALTER TABLE item RENAME INDEX item_sku_key TO item_code_key"
        );
        assert!(matches!(
            r(Dialect::MariaDb, "foreign key"),
            Err(RenameError::Unsupported(_))
        ));
        assert!(matches!(
            r(Dialect::Sqlite, "check"),
            Err(RenameError::Unsupported(_))
        ));
        assert!(
            r(Dialect::SqlServer, "check")
                .unwrap()
                .ddl
                .contains("'OBJECT'")
        );
        assert!(
            r(Dialect::Oracle, "check")
                .unwrap()
                .ddl
                .contains("RENAME CONSTRAINT")
        );
    }

    /// SPEC-0164 v3 AK-8 (behaviour 11).
    #[test]
    fn which_databases_roll_ddl_back() {
        for d in [
            Dialect::PostgreSql,
            Dialect::Sqlite,
            Dialect::SqlServer,
            Dialect::DuckDb,
        ] {
            assert!(ddl_in_transaction(d), "{d:?}");
        }
        for d in [Dialect::MySql, Dialect::MariaDb, Dialect::Oracle] {
            assert!(!ddl_in_transaction(d), "{d:?}");
        }
        assert!(run_failure(&crate::ScriptRun::default()).is_some());
    }

    /// SPEC-0164 v3 AK-9 (behaviour 12).
    #[test]
    fn notes_name_what_a_statement_risks() {
        let change = refactoring_notes(&Refactoring::ChangeType, Dialect::PostgreSql);
        assert!(change[0].data_loss && change[0].text.starts_with("May lose data"));
        assert!(change[1].text.contains("one transaction"));
        let rename = refactoring_notes(&Refactoring::Rename, Dialect::MySql);
        assert!(rename.iter().all(|n| !n.data_loss));
        assert!(rename[0].text.contains("not rewritten"));
        assert!(rename[1].text.starts_with("MySQL commits each statement"));
    }

    /// SPEC-0019 v5 AK-13 (OBS-0068 L3): a WITH query or subquery keeps its result columns.
    #[test]
    fn with_queries_keep_their_result_columns() {
        let schema = vec![
            table("pd_note", &["id", "body", "qty"]),
            table("other", &["id", "label"]),
        ];
        let renamed = |sql: &str| {
            rename(
                sql,
                Dialect::PostgreSql,
                &schema,
                "pd_note",
                Some("body"),
                "text_body",
            )
            .unwrap()
            .sql
        };
        // The query of OBS-0068: the inner place keeps its name for the outer one.
        assert_eq!(
            renamed(
                "with big as (select id, body from pd_note where qty > 10)\nselect b.body from big b;"
            ),
            "with big as (select id, text_body as body from pd_note where qty > 10)\nselect b.body from big b;"
        );
        assert_eq!(
            renamed("WITH big AS (SELECT n.id, n.body FROM pd_note n) SELECT big.body FROM big"),
            "WITH big AS (SELECT n.id, n.text_body AS body FROM pd_note n) SELECT big.body FROM big"
        );
        // A cast keeps the column's name, so the alias goes after it.
        assert_eq!(
            renamed("with big as (select body::text from pd_note) select body from big"),
            "with big as (select text_body::text as body from pd_note) select body from big"
        );
        // `*` passes the new name through: the readers follow it.
        assert_eq!(
            renamed(
                "with big as (select * from pd_note where qty > 10) select b.body, body from big b"
            ),
            "with big as (select * from pd_note where qty > 10) select b.text_body, text_body from big b"
        );
        assert_eq!(
            renamed(
                "with a as (select n.* from pd_note n), b as (select * from a) select b.body from b"
            ),
            "with a as (select n.* from pd_note n), b as (select * from a) select b.text_body from b"
        );
        // A column list or an alias of its own keeps the result columns already.
        assert_eq!(
            renamed(
                "with big(id, body) as (select id, body from pd_note) select b.body from big b"
            ),
            "with big(id, body) as (select id, text_body from pd_note) select b.body from big b"
        );
        assert_eq!(
            renamed("with big as (select body as b2 from pd_note) select b2 from big"),
            "with big as (select text_body as b2 from pd_note) select b2 from big"
        );
        // An unqualified name outside reads the query, not the table.
        assert_eq!(
            renamed(
                "with big as (select id, body from pd_note) select body from big order by body"
            ),
            "with big as (select id, text_body as body from pd_note) select body from big order by body"
        );
        // A subquery in FROM behaves like the WITH query; one in WHERE stays part of its statement.
        assert_eq!(
            renamed("select s.body from (select body from pd_note) s join other o on o.id = 1"),
            "select s.body from (select text_body as body from pd_note) s join other o on o.id = 1"
        );
        assert_eq!(
            renamed(
                "select o.label from other o where exists (select 1 from pd_note where body = o.label)"
            ),
            "select o.label from other o where exists (select 1 from pd_note where text_body = o.label)"
        );
        // The statement's own result columns follow the rename, as before.
        assert_eq!(
            renamed("select body from pd_note"),
            "select text_body from pd_note"
        );
        // The ambiguity check looks at the tables of the place's own scope.
        let r = rename(
            "with x as (select id from other) select body from pd_note join x on x.id = pd_note.id",
            Dialect::PostgreSql,
            &schema,
            "pd_note",
            Some("body"),
            "text_body",
        )
        .unwrap();
        assert!(r.ambiguous.is_empty(), "{r:?}");
        assert_eq!(
            r.sql,
            "with x as (select id from other) select text_body from pd_note join x on x.id = pd_note.id"
        );
    }

    /// SPEC-0018 v6 AK-14: qualifiers of one schema move to another or go.
    #[test]
    fn qualifiers_of_a_schema_are_rewritten() {
        let d = Dialect::PostgreSql;
        assert_eq!(
            requalify(
                "select i.sku from pd_b.item i join \"pd_b\".w on true, PD_B.x, other.item, 'pd_b.item' -- pd_b.item\n, a.pd_b.t",
                d,
                "pd_b",
                Some("public")
            ),
            "select i.sku from public.item i join \"public\".w on true, public.x, other.item, 'pd_b.item' -- pd_b.item\n, a.pd_b.t"
        );
        assert_eq!(
            requalify("select * from pd_b.item", d, "pd_b", Some("New Schema")),
            "select * from \"New Schema\".item"
        );
        assert_eq!(
            requalify("select public.item.sku from public.item", d, "public", None),
            "select item.sku from item"
        );
        assert_eq!(
            requalify(
                "select * from `shop`.item",
                Dialect::MySql,
                "shop",
                Some("arch")
            ),
            "select * from `arch`.item"
        );
    }
}
