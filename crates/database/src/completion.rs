//! Schema-aware SQL completion (`SPEC-0014`, v2: schemas and ranking, IMP-0059; v3: join clauses
//! from foreign keys and the schema in the detail of tables).

use std::collections::BTreeMap;
use std::ops::Range;

use serde::Serialize;

use crate::script::{dollar_tag_end, find, skip_bracketed, skip_quoted};
use crate::{Dialect, Table, quote_identifier, split_statements};

/// What a candidate stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    /// A column of a referenced table.
    Column,
    /// A table.
    Table,
    /// A view.
    View,
    /// An SQL keyword.
    Keyword,
    /// Another schema of the database (v2 behaviour 10).
    Schema,
    /// A table with alias and `on` condition built from a foreign key (v3 behaviour 14).
    Join,
}

/// A schema of the database other than the current one (`SPEC-0014` v2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaEntry {
    /// Schema name.
    pub name: String,
    /// Its tables and views, once read.
    pub tables: Option<Vec<Table>>,
}

/// Whether `name` is a system schema of `dialect` (v2 behaviour 10), never offered unprompted.
pub fn is_system_schema(dialect: Dialect, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    match dialect {
        Dialect::PostgreSql => lower.starts_with("pg_") || lower == "information_schema",
        Dialect::MySql | Dialect::MariaDb => matches!(
            lower.as_str(),
            "mysql" | "information_schema" | "performance_schema" | "sys"
        ),
        Dialect::SqlServer => {
            matches!(lower.as_str(), "sys" | "information_schema" | "guest")
                || lower.starts_with("db_")
        }
        Dialect::Sqlite => lower == "temp",
        Dialect::Oracle => false,
        Dialect::DuckDb => matches!(lower.as_str(), "information_schema" | "pg_catalog"),
    }
}

/// One completion proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// Name shown in the list.
    pub label: String,
    /// What it is.
    pub kind: CandidateKind,
    /// Type and owner (columns); kind and schema (tables, views, join clauses); or empty.
    pub detail: String,
    /// Text to insert in place of the prefix, quoted if needed.
    pub insert_text: String,
}

pub(crate) const KEYWORDS: &[&str] = &[
    "all", "and", "as", "asc", "between", "by", "case", "count", "delete", "desc", "distinct",
    "else", "end", "exists", "from", "group", "having", "in", "inner", "insert", "into", "is",
    "join", "left", "like", "limit", "not", "null", "offset", "on", "or", "order", "outer",
    "right", "select", "set", "sum", "table", "then", "union", "update", "values", "when", "where",
    "with",
];

/// Keywords only one dialect knows, offered next to `KEYWORDS`.
pub(crate) fn dialect_keywords(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::SqlServer => &[
            "top", "output", "merge", "declare", "exec", "go", "nvarchar", "identity", "nolock",
        ],
        Dialect::MariaDb => &["returning", "sequence", "system_time"],
        Dialect::PostgreSql => &["returning", "ilike"],
        Dialect::Oracle => &[
            "dual",
            "rownum",
            "sysdate",
            "systimestamp",
            "nvl",
            "decode",
            "fetch",
            "first",
            "rows",
            "only",
            "connect",
            "prior",
            "start",
            "merge",
            "varchar2",
            "number",
            "clob",
            "begin",
            "declare",
            "exception",
            "returning",
            "minus",
        ],
        Dialect::DuckDb => &[
            "returning",
            "ilike",
            "qualify",
            "pivot",
            "unpivot",
            "summarize",
            "describe",
        ],
        Dialect::MySql | Dialect::Sqlite => &[],
    }
}

/// Keywords after which a table name follows.
pub(crate) const TABLE_INTRODUCERS: &[&str] = &["from", "join", "into", "update", "table"];

/// Keywords that end a `FROM` list.
pub(crate) const CLAUSE_ENDS: &[&str] = &[
    "where",
    "group",
    "order",
    "having",
    "limit",
    "on",
    "set",
    "union",
    "select",
    "values",
    "using",
    "offset",
    "returning",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tok {
    Word,
    Dot,
    Comma,
    Other,
    /// A quoted identifier; only [`tokenize_with_names`] gives these.
    Quoted,
}

pub(crate) struct Token {
    pub(crate) kind: Tok,
    pub(crate) range: Range<usize>,
}

/// Proposals for the cursor at byte offset `cursor` in `script`.
pub fn complete(script: &str, cursor: usize, dialect: Dialect, schema: &[Table]) -> Vec<Candidate> {
    complete_in(script, cursor, dialect, schema, &[])
}

/// Like [`complete_in`], naming `current` (the current schema) in the detail of its tables, views
/// and join clauses (v3 behaviour 16).
pub fn complete_in_schema(
    script: &str,
    cursor: usize,
    dialect: Dialect,
    schema: &[Table],
    others: &[SchemaEntry],
    current: Option<&str>,
) -> Vec<Candidate> {
    complete_impl(script, cursor, dialect, schema, others, current)
}

/// [`complete_in_schema`] for a profile's database system: adds the connector's keywords
/// (`SPEC-0300` behaviour 9) that start with the word before the cursor.
pub fn complete_for(
    script: &str,
    cursor: usize,
    dialect: Dialect,
    system: Option<&str>,
    schema: &[Table],
    others: &[SchemaEntry],
    current: Option<&str>,
) -> Vec<Candidate> {
    let mut out = complete_in_schema(script, cursor, dialect, schema, others, current);
    let Some(connector) = system.and_then(|id| crate::connector::catalogue().get(id).cloned())
    else {
        return out;
    };
    let before = script.get(..cursor).unwrap_or("");
    let word: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if before[..before.len() - word.len()]
        .trim_end()
        .ends_with('.')
    {
        return out;
    }
    let lower = word.to_lowercase();
    for k in &connector.sql.keywords {
        let label = k.to_lowercase();
        if label.starts_with(&lower) && !out.iter().any(|c| c.label.eq_ignore_ascii_case(&label)) {
            out.push(Candidate {
                label: label.clone(),
                kind: CandidateKind::Keyword,
                detail: connector.name.clone(),
                insert_text: label,
            });
        }
    }
    out
}

/// The schema named by a `schema.` qualifier before the cursor whose tables are not read yet, so the
/// caller can read them and complete again (v2 behaviour 11).
pub fn wanted_schema(
    script: &str,
    cursor: usize,
    dialect: Dialect,
    schema: &[Table],
    others: &[SchemaEntry],
) -> Option<String> {
    let entry = qualifier_schema(script, cursor, dialect, schema, others)?;
    entry.tables.is_none().then(|| entry.name.clone())
}

fn qualifier_schema<'a>(
    script: &str,
    cursor: usize,
    dialect: Dialect,
    schema: &[Table],
    others: &'a [SchemaEntry],
) -> Option<&'a SchemaEntry> {
    let before = script.get(..cursor)?;
    let head = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    let head = head.strip_suffix('.')?;
    let qualifier: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if qualifier.is_empty() || head[..head.len() - qualifier.len()].ends_with('.') {
        return None;
    }
    let start = statement_start(script, cursor, dialect);
    let tokens = tokenize(&script[start..cursor], cursor - start, dialect)?;
    let refs = table_refs(&script[start..cursor], &tokens, schema);
    let is_table = refs.iter().any(|(a, t)| {
        a.as_deref() == Some(&qualifier.to_lowercase()) || t.name.eq_ignore_ascii_case(&qualifier)
    }) || schema
        .iter()
        .any(|t| t.name.eq_ignore_ascii_case(&qualifier));
    if is_table {
        return None;
    }
    others
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(&qualifier))
}

/// Like [`complete`], with the other schemas of the database (v2 behaviours 10–11).
pub fn complete_in(
    script: &str,
    cursor: usize,
    dialect: Dialect,
    schema: &[Table],
    others: &[SchemaEntry],
) -> Vec<Candidate> {
    complete_impl(script, cursor, dialect, schema, others, None)
}

fn complete_impl(
    script: &str,
    cursor: usize,
    dialect: Dialect,
    schema: &[Table],
    others: &[SchemaEntry],
    current: Option<&str>,
) -> Vec<Candidate> {
    if cursor > script.len() || !script.is_char_boundary(cursor) {
        return Vec::new();
    }
    let start = statement_start(script, cursor, dialect);
    let end = split_statements(script, dialect)
        .into_iter()
        .find(|s| s.range.start == start)
        .map_or(cursor, |s| s.range.end.max(cursor));
    let text = &script[start..end];
    let rel = cursor - start;
    let Some(tokens) = tokenize(text, rel, dialect) else {
        return Vec::new();
    };

    let prefix_start = text[..rel]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(rel, |(i, _)| i);
    let prefix = &text[prefix_start..rel];
    let before: Vec<&Token> = tokens
        .iter()
        .filter(|t| t.range.end <= prefix_start)
        .collect();
    let word = |t: &Token| text[t.range.clone()].to_ascii_lowercase();

    // Quoted table names count as names here, so their aliases and foreign keys are known
    // (SPEC-0014 v4 behaviour 18).
    let with_names = tokenize_with_names(text, rel, dialect).unwrap_or_default();
    let refs = table_refs(text, &with_names, schema);

    // `qualifier.` → columns of that table or alias.
    if let [.., q, dot] = before.as_slice()
        && dot.kind == Tok::Dot
        && q.kind == Tok::Word
        && dot.range.end == prefix_start
    {
        let qualifier = word(q);
        let table = refs
            .iter()
            .find(|(alias, _)| alias.as_deref() == Some(qualifier.as_str()))
            .map(|(_, t)| *t)
            .or_else(|| {
                schema
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(&qualifier))
            });
        if table.is_none()
            && let Some(entry) = qualifier_schema(script, cursor, dialect, schema, others)
        {
            let tables = entry.tables.as_deref().unwrap_or_default();
            return rank(
                prefix,
                tables_of(tables, dialect, Some(&entry.name)),
                dialect,
            );
        }
        return rank(prefix, columns_of(table.into_iter(), dialect), dialect);
    }

    let wants_tables = match before.last() {
        Some(t) if t.kind == Tok::Word => TABLE_INTRODUCERS.contains(&word(t).as_str()),
        Some(t) if t.kind == Tok::Comma => in_from_list(&before, &word),
        _ => false,
    };
    if wants_tables {
        let mut candidates = Vec::new();
        if before
            .last()
            .is_some_and(|t| t.kind == Tok::Word && word(t) == "join")
        {
            // Only the tables named before the cursor count as already joined (v3 behaviour 14).
            let earlier = with_names
                .iter()
                .take_while(|t| t.range.end <= prefix_start)
                .count();
            let refs = table_refs(text, &with_names[..earlier], schema);
            candidates.extend(join_clauses(&refs, schema, dialect, current));
        }
        candidates.extend(tables_of(schema, dialect, current));
        candidates.extend(
            others
                .iter()
                .filter(|e| !is_system_schema(dialect, &e.name))
                .map(|e| Candidate {
                    label: e.name.clone(),
                    kind: CandidateKind::Schema,
                    detail: "schema".into(),
                    insert_text: insert_text(&e.name, dialect),
                }),
        );
        return rank(prefix, candidates, dialect);
    }

    let referenced: Vec<&Table> = if refs.is_empty() {
        schema.iter().collect()
    } else {
        let mut seen: Vec<&Table> = Vec::new();
        for (_, t) in &refs {
            if !seen.iter().any(|s| s.name == t.name) {
                seen.push(t);
            }
        }
        seen
    };
    let mut candidates = columns_of(referenced.into_iter(), dialect);
    candidates.extend(
        KEYWORDS
            .iter()
            .chain(dialect_keywords(dialect))
            .map(|k| Candidate {
                label: (*k).to_owned(),
                kind: CandidateKind::Keyword,
                detail: String::new(),
                insert_text: (*k).to_owned(),
            }),
    );
    rank(prefix, candidates, dialect)
}

/// Byte offset where the statement containing `cursor` starts.
fn statement_start(script: &str, cursor: usize, dialect: Dialect) -> usize {
    let statements = split_statements(script, dialect);
    let mut start = 0;
    for s in &statements {
        if s.range.start > cursor {
            break;
        }
        // A terminator between this statement and the cursor starts a new, empty one.
        start = match script
            .get(s.range.end..cursor)
            .and_then(|gap| gap.rfind(';'))
        {
            Some(semicolon) => s.range.end + semicolon + 1,
            None => s.range.start,
        };
    }
    start
}

/// Tokens of `text`; `None` when `cursor` lies inside a literal, quoted identifier or comment.
pub(crate) fn tokenize(text: &str, cursor: usize, dialect: Dialect) -> Option<Vec<Token>> {
    tokens_of(text, cursor, dialect, false)
}

/// As [`tokenize`], with a [`Tok::Quoted`] token for each quoted identifier (`"…"`, `` `…` ``,
/// `[…]` in T-SQL) instead of nothing.
fn tokenize_with_names(text: &str, cursor: usize, dialect: Dialect) -> Option<Vec<Token>> {
    tokens_of(text, cursor, dialect, true)
}

/// The name inside a quoted identifier token, its doubled quotes undone.
fn unquoted(token: &str) -> String {
    let mut chars = token.chars();
    let (Some(open), Some(_)) = (chars.next(), chars.next_back()) else {
        return token.to_owned();
    };
    let close = if open == '[' { ']' } else { open };
    chars
        .as_str()
        .replace(&format!("{close}{close}"), &close.to_string())
}

fn tokens_of(text: &str, cursor: usize, dialect: Dialect, names: bool) -> Option<Vec<Token>> {
    let mysql = dialect.mysql_like();
    let bytes = text.as_bytes();
    let inside = |from: usize, to: usize| {
        from < cursor && cursor < to.max(from + 1)
            || (to >= bytes.len() && cursor >= bytes.len() && from < cursor)
    };
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let skipped = match b {
            b'\'' | b'"' | b'`' => Some(skip_quoted(bytes, i, b, mysql)),
            b'[' if dialect == Dialect::SqlServer => Some(skip_bracketed(bytes, i)),
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
            if inside(i, end) {
                return None;
            }
            // In MySQL a double quote opens a string unless ANSI_QUOTES is set.
            let identifier = match b {
                b'`' | b'[' => true,
                b'"' => !mysql,
                _ => false,
            };
            if names && identifier && end > i + 1 {
                tokens.push(Token {
                    kind: Tok::Quoted,
                    range: i..end,
                });
            }
            i = end.max(i + 1);
            continue;
        }
        if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80)
            {
                i += 1;
            }
            tokens.push(Token {
                kind: Tok::Word,
                range: start..i,
            });
            continue;
        }
        let kind = match b {
            b'.' => Tok::Dot,
            b',' => Tok::Comma,
            c if c.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            _ => Tok::Other,
        };
        tokens.push(Token {
            kind,
            range: i..i + 1,
        });
        i += 1;
    }
    Some(tokens)
}

/// Tables referenced in the statement with their aliases (`None` = no alias).
fn table_refs<'a>(
    text: &str,
    tokens: &[Token],
    schema: &'a [Table],
) -> Vec<(Option<String>, &'a Table)> {
    let word = |t: &Token| {
        let raw = &text[t.range.clone()];
        if t.kind == Tok::Quoted {
            unquoted(raw).to_ascii_lowercase()
        } else {
            raw.to_ascii_lowercase()
        }
    };
    let is_name = |t: &Token| matches!(t.kind, Tok::Word | Tok::Quoted);
    let mut refs = Vec::new();
    let mut in_from = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        let w = if t.kind == Tok::Word {
            word(t)
        } else {
            String::new()
        };
        let introduces = TABLE_INTRODUCERS.contains(&w.as_str());
        let continues = t.kind == Tok::Comma && in_from;
        if w == "from" || w == "join" {
            in_from = true;
        } else if CLAUSE_ENDS.contains(&w.as_str()) {
            in_from = false;
        }
        if introduces || continues {
            // Name, possibly qualified: take the last part.
            let mut j = i + 1;
            let mut name = None;
            while j < tokens.len() && is_name(&tokens[j]) {
                name = Some(word(&tokens[j]));
                if tokens.get(j + 1).map(|n| n.kind) == Some(Tok::Dot) {
                    j += 2;
                } else {
                    j += 1;
                    break;
                }
            }
            if let Some(table) =
                name.and_then(|n| schema.iter().find(|t| t.name.eq_ignore_ascii_case(&n)))
            {
                let mut k = j;
                if tokens
                    .get(k)
                    .is_some_and(|t| t.kind == Tok::Word && word(t) == "as")
                {
                    k += 1;
                }
                let alias = tokens
                    .get(k)
                    .filter(|t| is_name(t))
                    .map(|t| (t.kind, word(t)))
                    .filter(|(kind, a)| {
                        *kind == Tok::Quoted
                            || (!KEYWORDS.contains(&a.as_str())
                                && !CLAUSE_ENDS.contains(&a.as_str()))
                    })
                    .map(|(_, a)| a);
                refs.push((alias, table));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    refs
}

fn in_from_list(before: &[&Token], word: &impl Fn(&Token) -> String) -> bool {
    // Walking back, an unmatched `(` means the comma separates function arguments.
    let mut depth = 0i32;
    for t in before.iter().rev() {
        if t.kind == Tok::Other {
            match word(t).as_str() {
                ")" => depth += 1,
                "(" if depth == 0 => return false,
                "(" => depth -= 1,
                _ => {}
            }
            continue;
        }
        if t.kind != Tok::Word || depth > 0 {
            continue;
        }
        let w = word(t);
        if w == "from" {
            return true;
        }
        if CLAUSE_ENDS.contains(&w.as_str()) || w == "join" {
            return false;
        }
    }
    false
}

/// Tables and views as candidates; with a known schema the detail is `table · schema` or
/// `view · schema` (v3 behaviour 16), else `view` or empty as before.
fn tables_of(tables: &[Table], dialect: Dialect, schema: Option<&str>) -> Vec<Candidate> {
    tables
        .iter()
        .map(|t| {
            let view = t.kind == "view";
            Candidate {
                label: t.name.clone(),
                kind: if view {
                    CandidateKind::View
                } else {
                    CandidateKind::Table
                },
                detail: match (view, schema.filter(|s| !s.is_empty())) {
                    (true, Some(s)) => format!("view · {s}"),
                    (false, Some(s)) => format!("table · {s}"),
                    (true, None) => "view".into(),
                    (false, None) => String::new(),
                },
                insert_text: insert_text(&t.name, dialect),
            }
        })
        .collect()
}

/// The last part of a possibly qualified table name.
fn base_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// An alias for `table`: the initials of its `_`-separated parts in lower case, made unique
/// against `taken` (aliases and tables of the statement) and keywords by appending 2, 3 …
/// (v3 behaviour 15).
pub(crate) fn alias_for(table: &str, taken: &[String]) -> String {
    let mut base: String = table
        .split(|c: char| c == '_' || !c.is_ascii_alphanumeric())
        .filter_map(|part| part.chars().next())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
        base.insert(0, 't');
    }
    let free = |a: &str| {
        !taken.iter().any(|t| t == a) && !KEYWORDS.contains(&a) && !CLAUSE_ENDS.contains(&a)
    };
    if free(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}{n}"))
        .find(|a| free(a))
        .unwrap_or(base)
}

/// The (left column, new table's column) pairs of the foreign keys from `from` to `to`; a key
/// without referenced columns references `to`'s primary key.
fn key_pairs<'a>(from: &'a Table, to: &'a Table) -> Vec<Vec<(&'a str, &'a str)>> {
    from.foreign_keys
        .iter()
        .filter(|fk| base_name(&fk.referenced_table).eq_ignore_ascii_case(&to.name))
        .filter_map(|fk| {
            let referenced = if fk.referenced_columns.is_empty() {
                &to.primary_key
            } else {
                &fk.referenced_columns
            };
            (referenced.len() == fk.columns.len() && !fk.columns.is_empty()).then(|| {
                fk.columns
                    .iter()
                    .map(String::as_str)
                    .zip(referenced.iter().map(String::as_str))
                    .collect()
            })
        })
        .collect()
}

/// Join clauses after `JOIN` (v3 behaviour 14): for each foreign key between a table of the schema
/// and a table already in the statement, in either direction, `table alias on left.col =
/// alias.col` (composite keys joined with `and`). The table in the statement keeps its alias (or
/// its name when it has none). Tables already in the statement are only offered again for a key
/// that references the table itself. No joins are guessed from equal column names.
fn join_clauses(
    refs: &[(Option<String>, &Table)],
    schema: &[Table],
    dialect: Dialect,
    current: Option<&str>,
) -> Vec<Candidate> {
    let taken: Vec<String> = refs
        .iter()
        .flat_map(|(alias, t)| alias.iter().cloned().chain([t.name.to_lowercase()]))
        .collect();
    let detail = match current.filter(|s| !s.is_empty()) {
        Some(s) => format!("foreign key · {s}"),
        None => "foreign key".to_owned(),
    };
    let mut out: Vec<Candidate> = Vec::new();
    for table in schema.iter().filter(|t| t.kind != "view") {
        let in_statement = refs.iter().any(|(_, t)| t.name == table.name);
        let alias = alias_for(&table.name, &taken);
        for (ref_alias, left) in refs {
            let same = left.name == table.name;
            if in_statement && !same {
                continue;
            }
            // Keys of the left table to the new one, then of the new table to the left one
            // (flipped so the left column comes first); a self-reference only once.
            let mut keys = key_pairs(left, table);
            if !same {
                keys.extend(
                    key_pairs(table, left)
                        .into_iter()
                        .map(|k| k.into_iter().map(|(n, l)| (l, n)).collect()),
                );
            }
            let (qualifier, quoted_qualifier) = match ref_alias {
                Some(a) => (a.clone(), a.clone()),
                None => (left.name.clone(), insert_text(&left.name, dialect)),
            };
            for key in keys {
                let condition = |q: &str, quote: bool| {
                    key.iter()
                        .map(|(l, r)| {
                            if quote {
                                format!(
                                    "{q}.{} = {alias}.{}",
                                    insert_text(l, dialect),
                                    insert_text(r, dialect)
                                )
                            } else {
                                format!("{q}.{l} = {alias}.{r}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" and ")
                };
                let label = format!("{} {alias} on {}", table.name, condition(&qualifier, false));
                if out.iter().any(|c| c.label == label) {
                    continue;
                }
                out.push(Candidate {
                    insert_text: format!(
                        "{} {alias} on {}",
                        insert_text(&table.name, dialect),
                        condition(&quoted_qualifier, true)
                    ),
                    label,
                    kind: CandidateKind::Join,
                    detail: detail.clone(),
                });
            }
        }
    }
    out
}

fn columns_of<'a>(tables: impl Iterator<Item = &'a Table>, dialect: Dialect) -> Vec<Candidate> {
    let mut merged: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order = Vec::new();
    for table in tables {
        for c in &table.columns {
            let entry = merged.entry(c.name.clone()).or_insert_with(|| {
                order.push(c.name.clone());
                Vec::new()
            });
            entry.push(format!("{} · {}", c.data_type, table.name));
        }
    }
    order
        .into_iter()
        .map(|name| Candidate {
            detail: merged[&name].join(", "),
            insert_text: insert_text(&name, dialect),
            label: name,
            kind: CandidateKind::Column,
        })
        .collect()
}

pub(crate) fn insert_text(name: &str, dialect: Dialect) -> String {
    let plain = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !KEYWORDS.contains(&name);
    if plain {
        name.to_owned()
    } else {
        quote_identifier(name, dialect)
    }
}

fn rank(prefix: &str, candidates: Vec<Candidate>, _dialect: Dialect) -> Vec<Candidate> {
    let lower = prefix.to_lowercase();
    let mut out: Vec<Candidate> = candidates
        .into_iter()
        .filter(|c| c.label.to_lowercase().starts_with(&lower))
        .collect();
    out.sort_by(|a, b| {
        let kind = |c: &Candidate| match c.kind {
            CandidateKind::Join => 0,
            CandidateKind::Table | CandidateKind::View => 1,
            CandidateKind::Column => 2,
            CandidateKind::Keyword => 3,
            CandidateKind::Schema => 4,
        };
        kind(a)
            .cmp(&kind(b))
            .then_with(|| (!a.label.starts_with(prefix)).cmp(&!b.label.starts_with(prefix)))
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, Table};

    fn table(name: &str, kind: &str, columns: &[(&str, &str)]) -> Table {
        Table {
            storage: Vec::new(),
            hidden_columns: Vec::new(),
            name: name.into(),
            kind: kind.into(),
            columns: columns
                .iter()
                .map(|(n, t)| Column {
                    name: (*n).into(),
                    data_type: (*t).into(),
                    nullable: false,
                    default: None,
                    generated: None,
                })
                .collect(),
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
            unreadable: None,
            constraints: Vec::new(),
        }
    }

    fn reference() -> Vec<Table> {
        vec![
            table(
                "item",
                "table",
                &[
                    ("sku", "text"),
                    ("name", "text"),
                    ("unit_price_cents", "integer"),
                ],
            ),
            table(
                "movement",
                "table",
                &[
                    ("id", "integer"),
                    ("warehouse_id", "integer"),
                    ("sku", "text"),
                    ("delta", "integer"),
                ],
            ),
            table(
                "stock",
                "table",
                &[
                    ("warehouse_id", "integer"),
                    ("sku", "text"),
                    ("qty", "integer"),
                ],
            ),
            table(
                "stock_value",
                "view",
                &[("warehouse_id", "integer"), ("value_cents", "bigint")],
            ),
            table("warehouse", "table", &[("id", "integer"), ("city", "text")]),
        ]
    }

    /// Completes at the `|` marker.
    fn at(sql_with_cursor: &str) -> Vec<Candidate> {
        at_in(sql_with_cursor, Dialect::PostgreSql, &reference())
    }

    fn at_in(sql_with_cursor: &str, dialect: Dialect, schema: &[Table]) -> Vec<Candidate> {
        let cursor = sql_with_cursor.find('|').unwrap();
        let sql = sql_with_cursor.replacen('|', "", 1);
        complete(&sql, cursor, dialect, schema)
    }

    fn labels(c: &[Candidate], kind: CandidateKind) -> Vec<String> {
        c.iter()
            .filter(|c| c.kind == kind)
            .map(|c| c.label.clone())
            .collect()
    }

    #[test]
    fn tables_after_from() {
        let c = at("select * from |");
        let names: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            names,
            ["item", "movement", "stock", "stock_value", "warehouse"]
        );
        assert_eq!(c[3].kind, CandidateKind::View);
        assert!(
            at("select * from stock, w|")
                .iter()
                .all(|c| c.label == "warehouse")
        );
        // A comma between function arguments does not continue the FROM list.
        assert!(
            at("select * from generate_series(1, |")
                .iter()
                .all(|c| c.kind != CandidateKind::Table)
        );
    }

    #[test]
    fn qualified_columns_through_aliases() {
        let mut stock = labels(&at("select s.| from stock s"), CandidateKind::Column);
        stock.sort();
        assert_eq!(stock, ["qty", "sku", "warehouse_id"]);
        let mut item = labels(
            &at("select * from stock s join item i on i.|"),
            CandidateKind::Column,
        );
        item.sort();
        assert_eq!(item, ["name", "sku", "unit_price_cents"]);
        assert_eq!(at("select stock.q| from stock")[0].label, "qty");
    }

    #[test]
    fn columns_of_referenced_tables_then_keywords() {
        let c = at("select * from stock where q|");
        assert_eq!(c[0].label, "qty");
        let cols = labels(
            &at("select * from item; select | from stock"),
            CandidateKind::Column,
        );
        assert!(cols.contains(&"qty".to_owned()));
        assert!(!cols.contains(&"name".to_owned()));
        let all = at("select |");
        assert!(labels(&all, CandidateKind::Column).contains(&"city".to_owned()));
        assert!(labels(&all, CandidateKind::Keyword).contains(&"from".to_owned()));
        let merged = at("select s| from stock join movement on true");
        let sku = merged.iter().find(|c| c.label == "sku").unwrap();
        assert_eq!(sku.detail, "text · stock, text · movement");
    }

    #[test]
    fn nothing_inside_literals_and_comments() {
        assert!(at("select 'fro|").is_empty());
        assert!(at("-- from |").is_empty());
        assert!(at("select \"st|\" from stock").is_empty());
    }

    #[test]
    fn quotes_names_that_need_it() {
        let schema = vec![table("Order Items", "table", &[("order", "integer")])];
        let pg = at_in("select * from |", Dialect::PostgreSql, &schema);
        assert_eq!(pg[0].insert_text, "\"Order Items\"");
        let my = at_in("select * from |", Dialect::MySql, &schema);
        assert_eq!(my[0].insert_text, "`Order Items`");
        let col = at_in("select o| from x", Dialect::PostgreSql, &schema);
        assert_eq!(col[0].insert_text, "\"order\"");
        let ms = at_in("select * from |", Dialect::SqlServer, &schema);
        assert_eq!(ms[0].insert_text, "[Order Items]");
    }

    #[test]
    fn t_sql_keywords_and_brackets() {
        let schema = vec![table("stock", "table", &[("sku", "nvarchar(20)")])];
        let top = at_in("select to|", Dialect::SqlServer, &schema);
        assert!(top.iter().any(|c| c.label == "top"));
        assert!(
            !at_in("select to|", Dialect::PostgreSql, &schema)
                .iter()
                .any(|c| c.label == "top")
        );
        assert!(at_in("select [st| from stock", Dialect::SqlServer, &schema).is_empty());
    }

    #[test]
    fn oracle_keywords_and_quotes() {
        let schema = vec![table("Order Items", "table", &[("qty", "number")])];
        let du = at_in("select sysd|", Dialect::Oracle, &schema);
        assert!(du.iter().any(|c| c.label == "sysdate"), "{du:?}");
        let ora = at_in("select * from |", Dialect::Oracle, &schema);
        assert_eq!(ora[0].insert_text, "\"Order Items\"");
    }

    fn others() -> Vec<SchemaEntry> {
        vec![
            SchemaEntry {
                name: "audit".into(),
                tables: Some(vec![table("log", "table", &[("at", "timestamp")])]),
            },
            SchemaEntry {
                name: "pg_catalog".into(),
                tables: Some(vec![table("pg_am", "table", &[("amname", "name")])]),
            },
            SchemaEntry {
                name: "staging".into(),
                tables: None,
            },
        ]
    }

    fn with_others(sql_with_cursor: &str) -> Vec<Candidate> {
        let cursor = sql_with_cursor.find('|').unwrap();
        let sql = sql_with_cursor.replacen('|', "", 1);
        complete_in(&sql, cursor, Dialect::PostgreSql, &reference(), &others())
    }

    #[test]
    fn own_tables_first_then_other_schemas_never_system_ones() {
        let c = with_others("select * from |");
        let order: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            order,
            [
                "item",
                "movement",
                "stock",
                "stock_value",
                "warehouse",
                "audit",
                "staging"
            ]
        );
        assert_eq!(c[5].kind, CandidateKind::Schema);
        assert_eq!(
            labels(&with_others("select * from audit.|"), CandidateKind::Table),
            ["log"]
        );
        assert_eq!(
            labels(
                &with_others("select * from pg_catalog.|"),
                CandidateKind::Table
            ),
            ["pg_am"]
        );
        assert!(with_others("select * from staging.|").is_empty());
        let cursor = "select * from staging.".len();
        assert_eq!(
            wanted_schema(
                "select * from staging.",
                cursor,
                Dialect::PostgreSql,
                &reference(),
                &others()
            ),
            Some("staging".into())
        );
        assert_eq!(
            wanted_schema(
                "select i. from item i",
                9,
                Dialect::PostgreSql,
                &reference(),
                &others()
            ),
            None
        );
        assert!(is_system_schema(Dialect::MySql, "performance_schema"));
        assert!(!is_system_schema(Dialect::PostgreSql, "public"));
    }

    fn fk(columns: &[&str], table: &str, referenced: &[&str]) -> crate::ForeignKey {
        crate::ForeignKey {
            name: None,
            columns: columns.iter().map(|c| (*c).into()).collect(),
            referenced_table: table.into(),
            referenced_columns: referenced.iter().map(|c| (*c).into()).collect(),
        }
    }

    /// The reference schema with its foreign keys, plus `stock_check` with a composite key to
    /// `stock`.
    fn keyed() -> Vec<Table> {
        let mut tables = reference();
        for t in &mut tables {
            match t.name.as_str() {
                "movement" | "stock" => {
                    t.foreign_keys = vec![
                        fk(&["sku"], "item", &["sku"]),
                        fk(&["warehouse_id"], "warehouse", &["id"]),
                    ];
                }
                "item" => t.primary_key = vec!["sku".into()],
                "warehouse" => t.primary_key = vec!["id".into()],
                _ => {}
            }
        }
        let mut check = table(
            "stock_check",
            "table",
            &[
                ("warehouse_id", "integer"),
                ("sku", "text"),
                ("counted", "integer"),
            ],
        );
        check.foreign_keys = vec![fk(
            &["warehouse_id", "sku"],
            "stock",
            &["warehouse_id", "sku"],
        )];
        tables.push(check);
        tables
    }

    fn joins(sql_with_cursor: &str) -> Vec<String> {
        labels(
            &at_in(sql_with_cursor, Dialect::PostgreSql, &keyed()),
            CandidateKind::Join,
        )
    }

    #[test]
    fn join_clauses_from_foreign_keys_both_directions() {
        // movement references item and warehouse; stock shares column names with movement but
        // has no key to it, so no join is guessed.
        assert_eq!(
            joins("select * from movement m join |"),
            [
                "item i on m.sku = i.sku",
                "warehouse w on m.warehouse_id = w.id"
            ]
        );
        // The other direction: tables whose keys reference the table in the statement.
        assert_eq!(
            joins("select * from warehouse w left outer join |"),
            [
                "movement m on w.id = m.warehouse_id",
                "stock s on w.id = s.warehouse_id"
            ]
        );
        // A key without referenced columns references the primary key.
        let mut schema = keyed();
        schema[1].foreign_keys = vec![fk(&["warehouse_id"], "warehouse", &[])];
        let c = at_in(
            "select * from movement m join |",
            Dialect::PostgreSql,
            &schema,
        );
        assert_eq!(
            labels(&c, CandidateKind::Join),
            ["warehouse w on m.warehouse_id = w.id"]
        );
        // Only after JOIN, and only with a known table before it.
        assert!(joins("select * from |").is_empty());
        assert!(joins("select * from movement m, |").is_empty());
        assert!(joins("select * from nope n join |").is_empty());
        assert!(joins("select * from movement m join | join item i").len() == 2);
    }

    #[test]
    fn composite_keys_are_joined_with_and() {
        let found = joins("select * from stock s join |");
        assert!(
            found.contains(
                &"stock_check sc on s.warehouse_id = sc.warehouse_id and s.sku = sc.sku".to_owned()
            ),
            "{found:?}"
        );
        assert!(found.contains(&"item i on s.sku = i.sku".to_owned()));
    }

    #[test]
    fn join_aliases_are_unique_and_existing_ones_kept() {
        assert_eq!(alias_for("stock_value", &[]), "sv");
        assert_eq!(alias_for("warehouse", &[]), "w");
        assert_eq!(alias_for("item", &["i".into(), "i2".into()]), "i3");
        // Never a keyword: `order_note` would be `on`.
        assert_eq!(alias_for("order_note", &[]), "on2");
        assert_eq!(alias_for("2024_sales", &[]), "t2s");
        // `w` is taken by the alias of movement.
        assert_eq!(
            joins("select * from movement w join |"),
            [
                "item i on w.sku = i.sku",
                "warehouse w2 on w.warehouse_id = w2.id"
            ]
        );
        // The table in the statement keeps its alias, or its name without one.
        assert!(
            joins("select * from movement as mv join |")
                .contains(&"warehouse w on mv.warehouse_id = w.id".to_owned())
        );
        assert!(
            joins("select * from movement join |")
                .contains(&"warehouse w on movement.warehouse_id = w.id".to_owned())
        );
        // A table already joined is not offered again; its own alias counts as taken.
        assert_eq!(
            joins("select * from movement m join item i join |"),
            [
                "stock s on i.sku = s.sku",
                "warehouse w on m.warehouse_id = w.id"
            ]
        );
    }

    #[test]
    fn join_clauses_rank_first_and_filter_by_table_name() {
        let c = at_in(
            "select * from movement m join |",
            Dialect::PostgreSql,
            &keyed(),
        );
        let kinds: Vec<CandidateKind> = c.iter().map(|c| c.kind).collect();
        assert_eq!(&kinds[..2], [CandidateKind::Join, CandidateKind::Join]);
        // The plain tables stay available after the clauses.
        assert_eq!(
            labels(&c, CandidateKind::Table),
            ["item", "movement", "stock", "stock_check", "warehouse"]
        );
        assert!(c.iter().any(|c| c.label == "stock_value"));
        let w = at_in(
            "select * from movement m join wa|",
            Dialect::PostgreSql,
            &keyed(),
        );
        let names: Vec<&str> = w.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(names, ["warehouse w on m.warehouse_id = w.id", "warehouse"]);
        assert_eq!(w[0].insert_text, "warehouse w on m.warehouse_id = w.id");
        assert_eq!(w[0].detail, "foreign key");
    }

    #[test]
    fn tables_show_their_schema() {
        let schema = keyed();
        let cursor = "select * from movement m join ".len();
        let c = complete_in_schema(
            "select * from movement m join ",
            cursor,
            Dialect::PostgreSql,
            &schema,
            &others(),
            Some("public"),
        );
        let detail = |label: &str| c.iter().find(|c| c.label == label).unwrap().detail.clone();
        assert_eq!(detail("item"), "table · public");
        assert_eq!(detail("stock_value"), "view · public");
        assert_eq!(detail("item i on m.sku = i.sku"), "foreign key · public");
        let audit = complete_in_schema(
            "select * from audit.",
            20,
            Dialect::PostgreSql,
            &schema,
            &others(),
            Some("public"),
        );
        assert_eq!(audit[0].detail, "table · audit");
        // Without a known schema the details stay as before.
        assert_eq!(at("select * from |")[0].detail, "");
        assert_eq!(at("select * from |")[3].detail, "view");
    }

    #[test]
    fn join_clauses_quote_names_that_need_it() {
        let mut lines = table("Order Lines", "table", &[("Order Id", "integer")]);
        lines.foreign_keys = vec![fk(&["Order Id"], "orders", &["id"])];
        let schema = vec![table("orders", "table", &[("id", "integer")]), lines];
        let c = at_in("select * from orders join |", Dialect::PostgreSql, &schema);
        assert_eq!(c[0].label, "Order Lines ol on orders.id = ol.Order Id");
        assert_eq!(
            c[0].insert_text,
            "\"Order Lines\" ol on orders.id = ol.\"Order Id\""
        );
    }

    /// SPEC-0014 v4 behaviour 18: a quoted table name is a table of the statement.
    #[test]
    fn quoted_tables_give_columns_and_join_clauses() {
        let mut lines = table(
            "order items",
            "table",
            &[
                ("id", "integer"),
                ("group by", "integer"),
                ("order id", "integer"),
            ],
        );
        lines.foreign_keys = vec![fk(&["order id"], "orders", &["id"])];
        let quote = table("wei\"rd", "table", &[("x", "integer")]);
        let schema = vec![table("orders", "table", &[("id", "integer")]), lines, quote];
        let columns = |sql: &str, dialect: Dialect| {
            labels(&at_in(sql, dialect, &schema), CandidateKind::Column)
        };
        assert_eq!(
            columns("select * from \"order items\" o where o.|", Dialect::Sqlite),
            ["group by", "id", "order id"]
        );
        assert_eq!(
            columns("select o.| from \"order items\" as o", Dialect::PostgreSql),
            ["group by", "id", "order id"]
        );
        assert_eq!(
            columns("select w.| from \"wei\"\"rd\" w", Dialect::Sqlite),
            ["x"]
        );
        assert_eq!(
            columns("select o.| from `order items` o", Dialect::MySql),
            ["group by", "id", "order id"]
        );
        assert_eq!(
            columns("select o.| from [order items] o", Dialect::SqlServer),
            ["group by", "id", "order id"]
        );
        // In MySQL a double quote opens a string, not a name.
        assert!(columns("select o.| from \"order items\" o", Dialect::MySql).is_empty());
        let joins = at_in(
            "select * from \"order items\" oi join |",
            Dialect::Sqlite,
            &schema,
        );
        assert_eq!(joins[0].kind, CandidateKind::Join);
        assert_eq!(joins[0].insert_text, "orders o on oi.\"order id\" = o.id");
        // Still nothing inside the quotes themselves.
        assert!(at_in("select * from \"order |", Dialect::Sqlite, &schema).is_empty());
    }

    #[test]
    fn only_columns_after_alias_dot() {
        let c = with_others("select i.| from item i");
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|c| c.kind == CandidateKind::Column));
        assert_eq!(
            labels(&c, CandidateKind::Column),
            ["name", "sku", "unit_price_cents"]
        );
    }
}
