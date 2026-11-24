//! Colours of SQL text by the lexis of its dialect (`SPEC-0005` v7 behaviour 20).
//!
//! The editor and `highlight_file` colour SQL with one general grammar. Its tree does not know the
//! dialects: a MySQL backtick, a `\'` in a MySQL string, a `#` comment, a T-SQL `[name]` or `@var`
//! and Oracle's `q'[…]'` break the tree, and with it the colours of the rest of the line. This
//! module reads the text as the splitter of the dialect does (`script.rs`: the same rules for
//! comments, literals and quoted names) and names what it is sure of — comments, strings, quoted
//! names, numbers, variables and parameters, separators and the dialect's keywords. The callers lay
//! these runs over the grammar's colours; everything else keeps the grammar's colour.

use std::ops::Range;

use crate::Dialect;
use crate::script::{
    block_comment_end, dash_comment, delimiter_line, dollar_tag_end, escape_string, find,
    go_line_end, nested_comments, q_quote_end, skip_bracketed, skip_quoted, slash_line_end,
};

/// A run of SQL text the dialect's lexis decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlColour {
    /// Char range (Unicode scalar values) in the text.
    pub range: Range<usize>,
    /// The capture name to show (`comment`, `string`, `number`, `keyword`, `parameter`,
    /// `punctuation.delimiter`); `None` for a quoted name, which keeps the role the grammar gave
    /// it if it gave it one of a name, else shows as a variable.
    pub kind: Option<&'static str>,
}

/// Words that are keywords in every dialect, wherever they stand.
const COMMON: &[&str] = &[
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "begin",
    "between",
    "by",
    "cascade",
    "case",
    "check",
    "column",
    "commit",
    "constraint",
    "create",
    "cross",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "except",
    "exists",
    "explain",
    "false",
    "fetch",
    "foreign",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "in",
    "inner",
    "insert",
    "intersect",
    "into",
    "is",
    "join",
    "like",
    "natural",
    "not",
    "null",
    "nulls",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "over",
    "partition",
    "primary",
    "recursive",
    "references",
    "revoke",
    "rollback",
    "select",
    "set",
    "table",
    "then",
    "true",
    "union",
    "unique",
    "update",
    "using",
    "values",
    "when",
    "where",
    "with",
];

/// Keywords that are also names of functions: keywords unless a `(` follows.
const ALSO_FUNCTIONS: &[&str] = &["if", "left", "right", "replace", "truncate"];

/// Kinds of objects, keywords after a word that names what happens to one (`create`, `drop`, …).
const OBJECTS: &[&str] = &[
    "database",
    "extension",
    "function",
    "index",
    "package",
    "procedure",
    "proc",
    "role",
    "schema",
    "sequence",
    "trigger",
    "type",
    "user",
    "view",
    "body",
];

/// Words after which an object kind is a keyword.
const OBJECT_VERBS: &[&str] = &[
    "create",
    "alter",
    "drop",
    "replace",
    "temporary",
    "temp",
    "unique",
    "materialized",
    "editionable",
    "noneditionable",
    "package",
];

/// Names of data types: keywords after a variable (`DECLARE @n int`), after `returns`, and after
/// `as` inside parentheses (`cast(x as int)`).
const TYPES: &[&str] = &[
    "bigint",
    "binary",
    "bit",
    "blob",
    "bool",
    "boolean",
    "char",
    "clob",
    "date",
    "datetime",
    "datetime2",
    "decimal",
    "double",
    "float",
    "int",
    "integer",
    "interval",
    "json",
    "jsonb",
    "money",
    "nchar",
    "nclob",
    "ntext",
    "number",
    "numeric",
    "nvarchar",
    "nvarchar2",
    "real",
    "smallint",
    "text",
    "time",
    "timestamp",
    "tinyint",
    "uniqueidentifier",
    "uuid",
    "varbinary",
    "varchar",
    "varchar2",
];

/// Keywords of one dialect, wherever they stand.
fn own_words(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::PostgreSql => &[
            "analyze",
            "do",
            "ilike",
            "language",
            "lateral",
            "limit",
            "returning",
            "similar",
            "vacuum",
        ],
        Dialect::DuckDb => &[
            "anti",
            "asof",
            "attach",
            "copy",
            "describe",
            "detach",
            "exclude",
            "ilike",
            "install",
            "lateral",
            "limit",
            "load",
            "macro",
            "pivot",
            "positional",
            "qualify",
            "returning",
            "sample",
            "semi",
            "summarize",
            "unpivot",
        ],
        Dialect::MySql | Dialect::MariaDb => {
            const MYSQL: &[&str] = &[
                "auto_increment",
                "call",
                "close",
                "cursor",
                "declare",
                "delimiter",
                "describe",
                "div",
                "do",
                "duplicate",
                "elseif",
                "engine",
                "handler",
                "ignore",
                "iterate",
                "leave",
                "limit",
                "loop",
                "open",
                "regexp",
                "repeat",
                "return",
                "returns",
                "rlike",
                "show",
                "signal",
                "unsigned",
                "until",
                "use",
                "while",
                "xor",
            ];
            const MARIADB: &[&str] = &[
                "auto_increment",
                "call",
                "close",
                "cursor",
                "declare",
                "delimiter",
                "describe",
                "div",
                "do",
                "duplicate",
                "elseif",
                "engine",
                "handler",
                "ignore",
                "iterate",
                "leave",
                "limit",
                "loop",
                "open",
                "regexp",
                "repeat",
                "return",
                "returning",
                "returns",
                "rlike",
                "show",
                "signal",
                "system_time",
                "unsigned",
                "until",
                "use",
                "while",
                "xor",
            ];
            if dialect == Dialect::MariaDb {
                MARIADB
            } else {
                MYSQL
            }
        }
        Dialect::SqlServer => &[
            "apply",
            "catch",
            "declare",
            "exec",
            "execute",
            "holdlock",
            "identity",
            "merge",
            "nolock",
            "output",
            "percent",
            "pivot",
            "print",
            "raiserror",
            "readpast",
            "return",
            "returns",
            "rowlock",
            "tablock",
            "throw",
            "top",
            "tran",
            "transaction",
            "try",
            "unpivot",
            "updlock",
            "use",
            "waitfor",
            "while",
        ],
        Dialect::Oracle => &[
            "bulk",
            "collect",
            "connect",
            "continue",
            "cursor",
            "declare",
            "elsif",
            "exception",
            "execute",
            "exit",
            "for",
            "forall",
            "immediate",
            "loop",
            "merge",
            "minus",
            "nocopy",
            "others",
            "out",
            "pragma",
            "prior",
            "raise",
            "return",
            "returning",
            "rownum",
            "start",
            "while",
        ],
        Dialect::Sqlite => &[
            "abort",
            "attach",
            "autoincrement",
            "conflict",
            "detach",
            "fail",
            "glob",
            "ignore",
            "indexed",
            "limit",
            "pragma",
            "regexp",
            "returning",
            "rowid",
            "strict",
            "vacuum",
            "virtual",
            "without",
        ],
    }
}

/// The last thing the lexer saw outside comments, for the words whose role depends on it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Last {
    Nothing,
    Word(String),
    Number,
    Variable,
    Other(u8),
}

/// The runs of `sql` the lexis of `dialect` decides, in text order, without overlaps
/// (`SPEC-0005` v7 behaviour 20).
pub fn dialect_colours(sql: &str, dialect: Dialect) -> Vec<SqlColour> {
    let bytes = sql.as_bytes();
    let mysql = dialect.mysql_like();
    let tsql = dialect == Dialect::SqlServer;
    let oracle = dialect == Dialect::Oracle;
    let sqlite = dialect == Dialect::Sqlite;
    let pg = dialect.postgres_like();
    let nested = nested_comments(dialect);
    let own = own_words(dialect);
    let mut out: Vec<(Range<usize>, Option<&'static str>)> = Vec::new();
    let mut last = Last::Nothing;
    // The word before `last` when `last` is a number or a word (for `FETCH FIRST 1 ROWS ONLY`).
    let mut fetch_phrase = false;
    let mut depth: usize = 0;
    let mut delimiter: Option<Vec<u8>> = None;
    let mut i = 0;
    let word_byte = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80;
    while i < bytes.len() {
        let b = bytes[i];
        // `DELIMITER //` of the `mysql` client: the word and the new delimiter.
        if mysql
            && (b == b'd' || b == b'D')
            && let Some((end, new)) = delimiter_line(bytes, i)
        {
            out.push((i..i + "delimiter".len(), Some("keyword")));
            if let Some(new) = &new {
                let at = i + "delimiter".len();
                let start = at
                    + bytes[at..]
                        .iter()
                        .take_while(|c| c.is_ascii_whitespace())
                        .count();
                out.push((start..start + new.len(), Some("punctuation.delimiter")));
            }
            if let Some(new) = new {
                delimiter = (new != b";").then_some(new);
            }
            last = Last::Other(b';');
            fetch_phrase = false;
            i = end;
            continue;
        }
        if let Some(d) = &delimiter
            && bytes[i..].starts_with(d)
        {
            out.push((i..i + d.len(), Some("punctuation.delimiter")));
            i += d.len();
            last = Last::Other(b';');
            fetch_phrase = false;
            continue;
        }
        match b {
            b'\'' => {
                let end = skip_quoted(bytes, i, b'\'', mysql);
                out.push((i..end, Some("string")));
                last = Last::Other(b'\'');
                i = end;
                continue;
            }
            // MySQL and MariaDB read `"…"` as a string unless ANSI_QUOTES is on.
            b'"' if mysql => {
                let end = skip_quoted(bytes, i, b'"', true);
                out.push((i..end, Some("string")));
                last = Last::Other(b'\'');
                i = end;
                continue;
            }
            b'"' => {
                let end = skip_quoted(bytes, i, b'"', false);
                out.push((i..end, None));
                last = Last::Word(String::new());
                i = end;
                continue;
            }
            b'`' if mysql || sqlite => {
                let end = skip_quoted(bytes, i, b'`', false);
                out.push((i..end, None));
                last = Last::Word(String::new());
                i = end;
                continue;
            }
            b'[' if tsql || sqlite => {
                let end = if tsql {
                    skip_bracketed(bytes, i)
                } else {
                    bytes[i + 1..]
                        .iter()
                        .position(|&c| c == b']')
                        .map_or(bytes.len(), |p| i + p + 2)
                };
                out.push((i..end, None));
                last = Last::Word(String::new());
                i = end;
                continue;
            }
            b'-' if dash_comment(bytes, i, mysql) => {
                let end = line_end(bytes, i);
                out.push((i..end, Some("comment")));
                i = end;
                continue;
            }
            b'#' if mysql => {
                let end = line_end(bytes, i);
                out.push((i..end, Some("comment")));
                i = end;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = block_comment_end(bytes, i, nested);
                out.push((i..end, Some("comment")));
                i = end;
                continue;
            }
            // SQL*Plus runs the buffer with a `/` alone on its line.
            b'/' if oracle && slash_line_end(bytes, i).is_some() => {
                out.push((i..i + 1, Some("punctuation.delimiter")));
                last = Last::Other(b';');
                fetch_phrase = false;
                i = slash_line_end(bytes, i).unwrap_or(bytes.len());
                continue;
            }
            b'$' if pg => {
                if let Some(tag_end) = dollar_tag_end(bytes, i) {
                    let tag = &bytes[i..=tag_end];
                    let end = find(bytes, tag_end + 1, tag).map_or(bytes.len(), |p| p + tag.len());
                    out.push((i..end, Some("string")));
                    last = Last::Other(b'\'');
                    i = end;
                    continue;
                }
                let digits = digits_from(bytes, i + 1);
                if digits > 0 {
                    out.push((i..i + 1 + digits, Some("parameter")));
                    last = Last::Variable;
                    i += 1 + digits;
                    continue;
                }
            }
            // `@var` and `@@var` of T-SQL and MySQL.
            b'@' if tsql || mysql => {
                let start = i;
                let mut j = i + 1;
                if bytes.get(j) == Some(&b'@') {
                    j += 1;
                }
                let name = j;
                while j < bytes.len() && (word_byte(bytes[j]) || (tsql && bytes[j] == b'#')) {
                    j += 1;
                }
                if j > name {
                    out.push((start..j, Some("parameter")));
                    last = Last::Variable;
                    i = j;
                    continue;
                }
            }
            // Positional and named parameters: `?`, `?1`, `:name`, `$name`.
            b'?' if sqlite || mysql || dialect == Dialect::DuckDb => {
                let end = i + 1 + digits_from(bytes, i + 1);
                out.push((i..end, Some("parameter")));
                last = Last::Variable;
                i = end;
                continue;
            }
            b':' | b'$'
                if (oracle || sqlite)
                    && bytes
                        .get(i + 1)
                        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
                    && (i == 0 || (bytes[i - 1] != b':' && !word_byte(bytes[i - 1]))) =>
            {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                out.push((i..j, Some("parameter")));
                last = Last::Variable;
                i = j;
                continue;
            }
            b'0'..=b'9' | b'.'
                if (b != b'.' || bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
                    && (i == 0 || !word_byte(bytes[i - 1])) =>
            {
                let end = number_end(bytes, i, mysql || tsql || sqlite);
                if end < bytes.len() && word_byte(bytes[end]) && !bytes[i..end].contains(&b'.') {
                    // A name that starts with a digit (MySQL allows `1abc`).
                    let mut j = end;
                    while j < bytes.len() && word_byte(bytes[j]) {
                        j += 1;
                    }
                    last = Last::Word(String::new());
                    i = j;
                    continue;
                }
                out.push((i..end, Some("number")));
                last = Last::Number;
                i = end;
                continue;
            }
            b'(' => {
                depth += 1;
                last = Last::Other(b'(');
            }
            b')' => {
                depth = depth.saturating_sub(1);
                last = Last::Other(b')');
            }
            b';' => {
                fetch_phrase = false;
                depth = 0;
                last = Last::Other(b';');
            }
            c if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 || (tsql && c == b'#') => {
                // Prefixed strings: `N'…'`, `E'…'`, `X'…'`, `B'…'`, Oracle's `q'[…]'`.
                if oracle && let Some(end) = q_quote_end(bytes, i) {
                    out.push((i..end, Some("string")));
                    last = Last::Other(b'\'');
                    i = end;
                    continue;
                }
                if pg && (c == b'e' || c == b'E') && escape_string(bytes, i) {
                    let end = skip_quoted(bytes, i + 1, b'\'', true);
                    out.push((i..end, Some("string")));
                    last = Last::Other(b'\'');
                    i = end;
                    continue;
                }
                if matches!(c, b'n' | b'N' | b'x' | b'X' | b'b' | b'B')
                    && bytes.get(i + 1) == Some(&b'\'')
                {
                    let end = skip_quoted(bytes, i + 1, b'\'', mysql);
                    out.push((i..end, Some("string")));
                    last = Last::Other(b'\'');
                    i = end;
                    continue;
                }
                // T-SQL `GO` alone on its line ends a batch.
                if tsql
                    && (c == b'g' || c == b'G')
                    && let Some(end) = go_line_end(bytes, i)
                {
                    out.push((i..i + 2, Some("keyword")));
                    let after = i + 2;
                    let count_at = after
                        + bytes[after..]
                            .iter()
                            .take_while(|c| **c == b' ' || **c == b'\t')
                            .count();
                    let count = digits_from(bytes, count_at);
                    if count > 0 {
                        out.push((count_at..count_at + count, Some("number")));
                    }
                    last = Last::Other(b';');
                    fetch_phrase = false;
                    depth = 0;
                    i = end;
                    continue;
                }
                let start = i;
                while i < bytes.len()
                    && (word_byte(bytes[i]) || ((tsql || oracle) && bytes[i] == b'#'))
                {
                    i += 1;
                }
                let word = sql[start..i].to_ascii_lowercase();
                let next = bytes[i..]
                    .iter()
                    .find(|c| !c.is_ascii_whitespace())
                    .copied();
                // A name after `.` (`t.select`) is a name.
                let qualified = start > 0 && bytes[start - 1] == b'.';
                let keyword =
                    !qualified && is_keyword(&word, own, &last, depth, fetch_phrase, next);
                if keyword {
                    out.push((start..i, Some("keyword")));
                }
                if word == "fetch" || word == "offset" {
                    fetch_phrase = true;
                }
                last = Last::Word(word);
                continue;
            }
            c if !c.is_ascii_whitespace() => {
                last = Last::Other(c);
            }
            _ => {}
        }
        i += 1;
    }
    to_chars(sql, out)
}

/// Whether `word` is a keyword where it stands: one of every dialect or of this one (`own`), an
/// object kind after a verb, a type after a variable, `returns` or a cast's `as`, or a word of a
/// `FETCH`/`OFFSET` phrase.
fn is_keyword(
    word: &str,
    own: &[&str],
    last: &Last,
    depth: usize,
    fetch_phrase: bool,
    next: Option<u8>,
) -> bool {
    let before = match last {
        Last::Word(w) => w.as_str(),
        _ => "",
    };
    if ALSO_FUNCTIONS.contains(&word) {
        return next != Some(b'(');
    }
    if COMMON.contains(&word) || own.contains(&word) {
        return true;
    }
    if OBJECTS.contains(&word) && OBJECT_VERBS.contains(&before) {
        return true;
    }
    if word == "key" && matches!(before, "primary" | "foreign" | "unique") {
        return true;
    }
    if TYPES.contains(&word)
        && (matches!(last, Last::Variable) || before == "returns" || (before == "as" && depth > 0))
    {
        return true;
    }
    if fetch_phrase {
        return match word {
            "first" | "next" => matches!(before, "fetch"),
            "row" | "rows" => matches!(last, Last::Number) || matches!(before, "first" | "next"),
            "only" => matches!(before, "row" | "rows"),
            "ties" => before == "with",
            _ => false,
        };
    }
    if matches!(word, "first" | "last") && before == "nulls" {
        return true;
    }
    false
}

/// The index just past the line `i` is on, without its line break.
fn line_end(bytes: &[u8], i: usize) -> usize {
    bytes[i..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(bytes.len(), |p| i + p)
}

/// The number of ASCII digits from `i` on.
fn digits_from(bytes: &[u8], i: usize) -> usize {
    bytes.get(i..).map_or(0, |rest| {
        rest.iter().take_while(|c| c.is_ascii_digit()).count()
    })
}

/// The index just past a number starting at `i`: digits, a fraction, an exponent; with `hex` also
/// `0x…`.
fn number_end(bytes: &[u8], i: usize, hex: bool) -> usize {
    if hex
        && bytes[i] == b'0'
        && bytes.get(i + 1).is_some_and(|c| *c == b'x' || *c == b'X')
        && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        let mut j = i + 2;
        while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
            j += 1;
        }
        return j;
    }
    let mut j = i + digits_from(bytes, i);
    if bytes.get(j) == Some(&b'.') {
        j += 1;
        j += digits_from(bytes, j);
    }
    if bytes.get(j).is_some_and(|c| *c == b'e' || *c == b'E') {
        let sign = usize::from(bytes.get(j + 1).is_some_and(|c| *c == b'+' || *c == b'-'));
        let exp = digits_from(bytes, j + 1 + sign);
        if exp > 0 {
            j += 1 + sign + exp;
        }
    }
    j
}

/// `runs` (byte ranges in text order) as char ranges, in one pass over `sql`.
fn to_chars(sql: &str, runs: Vec<(Range<usize>, Option<&'static str>)>) -> Vec<SqlColour> {
    let mut out = Vec::with_capacity(runs.len());
    let mut byte = 0;
    let mut chars = 0;
    let mut at = |target: usize| {
        let target = target.min(sql.len());
        chars += sql[byte..target].chars().count();
        byte = target;
        chars
    };
    for (range, kind) in runs {
        if range.start >= range.end {
            continue;
        }
        let start = at(range.start);
        let end = at(range.end);
        out.push(SqlColour {
            range: start..end,
            kind,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text and kind of every run, for readable assertions.
    fn runs(sql: &str, dialect: Dialect) -> Vec<(String, &'static str)> {
        let chars: Vec<char> = sql.chars().collect();
        dialect_colours(sql, dialect)
            .into_iter()
            .map(|c| {
                (
                    chars[c.range].iter().collect::<String>(),
                    c.kind.unwrap_or("name"),
                )
            })
            .collect()
    }

    fn has(sql: &str, dialect: Dialect, text: &str, kind: &str) -> bool {
        runs(sql, dialect)
            .iter()
            .any(|(t, k)| t == text && *k == kind)
    }

    const MARIA: &str = include_str!("../tests/fixtures/sql-colours/maria.sql");
    const TSQL: &str = include_str!("../tests/fixtures/sql-colours/tsql.sql");
    const ORACLE: &str = include_str!("../tests/fixtures/sql-colours/oracle.sql");
    const LITE: &str = include_str!("../tests/fixtures/sql-colours/lite.sql");
    const DUCK: &str = include_str!("../tests/fixtures/sql-colours/duck.sql");

    /// SPEC-0005 v7 AK-22 (OBS-0083 L7): the MariaDB sample.
    #[test]
    fn mariadb_backticks_escapes_hash_comments_and_delimiter() {
        let d = Dialect::MariaDb;
        assert!(has(MARIA, d, "# MariaDB comment", "comment"));
        assert!(has(MARIA, d, "`id`", "name"));
        assert!(has(MARIA, d, r"'a\'b'", "string"));
        for k in [
            "SELECT",
            "FROM",
            "WHERE",
            "RETURNING",
            "DELIMITER",
            "CREATE",
            "PROCEDURE",
            "BEGIN",
            "END",
        ] {
            assert!(has(MARIA, d, k, "keyword"), "{k}: {:?}", runs(MARIA, d));
        }
        assert!(has(MARIA, d, "//", "punctuation.delimiter"));
        assert!(has(MARIA, d, "1", "number"));
        // `-- ` needs a blank in MySQL; `--x` is no comment there, `-- x` is.
        assert!(!has("select 5--1", d, "--1", "comment"));
        assert!(has("select 5 -- x", d, "-- x", "comment"));
        // MySQL reads `"…"` as a string, `@x` and `@@x` as variables.
        assert!(has(
            r#"select "a\"b", @x, @@version"#,
            d,
            r#""a\"b""#,
            "string"
        ));
        assert!(has("select @x, @@version", d, "@@version", "parameter"));
        // MySQL has no RETURNING.
        assert!(!has(MARIA, Dialect::MySql, "RETURNING", "keyword"));
    }

    /// SPEC-0005 v7 AK-22: the SQL Server sample.
    #[test]
    fn sql_server_brackets_variables_go_top_and_n_strings() {
        let d = Dialect::SqlServer;
        let r = runs(TSQL, d);
        assert!(has(TSQL, d, "-- T-SQL", "comment"), "{r:?}");
        for k in [
            "SELECT", "TOP", "AS", "FROM", "WITH", "NOLOCK", "GO", "DECLARE", "int",
        ] {
            assert!(has(TSQL, d, k, "keyword"), "{k}: {r:?}");
        }
        assert!(has(TSQL, d, "[id]", "name"));
        assert!(has(TSQL, d, "[f060_note]", "name"));
        assert!(has(TSQL, d, "N'text'", "string"));
        assert!(has(TSQL, d, "@n", "parameter"));
        assert!(has(TSQL, d, "5", "number") && has(TSQL, d, "1", "number"));
        assert_eq!(
            r.iter()
                .filter(|(t, k)| t == "GO" && *k == "keyword")
                .count(),
            2
        );
        // `int` is a type after a variable only; a column named `text` stays a name.
        assert!(!has("select text from t", d, "text", "keyword"));
        assert!(has("select cast(x as int)", d, "int", "keyword"));
        // `GO` inside a line is no separator, nor is `GOTO`.
        assert!(!has("select go from t", d, "go", "keyword"));
    }

    /// SPEC-0005 v7 AK-22: the Oracle sample.
    #[test]
    fn oracle_q_quotes_slash_and_pl_sql() {
        let d = Dialect::Oracle;
        let r = runs(ORACLE, d);
        assert!(has(ORACLE, d, "q'[a;b/c]'", "string"), "{r:?}");
        assert!(has(ORACLE, d, "/", "punctuation.delimiter"), "{r:?}");
        for k in [
            "BEGIN", "NULL", "END", "FETCH", "FIRST", "ROWS", "ONLY", "FROM",
        ] {
            assert!(has(ORACLE, d, k, "keyword"), "{k}: {r:?}");
        }
        assert!(has(ORACLE, d, "1", "number"));
        // `dual` and `sysdate` keep the grammar's colour; a column named `first` stays a name.
        assert!(!has(ORACLE, d, "dual", "keyword"));
        assert!(!has("select first from t", d, "first", "keyword"));
        assert!(has("select :name from dual", d, ":name", "parameter"));
        assert!(has(
            "create or replace procedure p is begin null; end;",
            d,
            "procedure",
            "keyword"
        ));
    }

    /// SPEC-0005 v7 AK-22: SQLite and DuckDB samples.
    #[test]
    fn sqlite_and_duckdb_words() {
        assert!(has(LITE, Dialect::Sqlite, "PRAGMA", "keyword"));
        assert!(has(LITE, Dialect::Sqlite, "?1", "parameter"));
        assert!(!has(LITE, Dialect::PostgreSql, "PRAGMA", "keyword"));
        assert!(has(DUCK, Dialect::DuckDb, "$$dollar$$", "string"));
        assert!(has(DUCK, Dialect::DuckDb, "'x.csv'", "string"));
        assert!(!has(DUCK, Dialect::DuckDb, "read_csv_auto", "keyword"));
        let d = Dialect::DuckDb;
        let q = "select * from t qualify row_number() over (partition by a) = 1; summarize t;";
        for k in ["qualify", "over", "partition", "summarize"] {
            assert!(has(q, d, k, "keyword"), "{k}");
        }
        // Words of one dialect are not keywords of another.
        assert!(!has(q, Dialect::Sqlite, "qualify", "keyword"));
    }

    /// SPEC-0005 v7 behaviour 20: comments nest as split (behaviour 19), quotes hide comment
    /// marks, `REPLACE(` and `LEFT(` stay calls, char ranges count characters.
    #[test]
    fn comments_strings_and_functions() {
        let text = "/* a /* b */ c */ select 1";
        assert!(has(
            text,
            Dialect::PostgreSql,
            "/* a /* b */ c */",
            "comment"
        ));
        assert!(has(text, Dialect::MySql, "/* a /* b */", "comment"));
        assert!(has(
            "select '/* no */'",
            Dialect::PostgreSql,
            "'/* no */'",
            "string"
        ));
        assert!(!has(
            "select replace(a, 'x', 'y')",
            Dialect::MySql,
            "replace",
            "keyword"
        ));
        assert!(has(
            "create or replace view v as select 1",
            Dialect::PostgreSql,
            "replace",
            "keyword"
        ));
        assert!(!has(
            "select left(a, 2)",
            Dialect::PostgreSql,
            "left",
            "keyword"
        ));
        assert!(has(
            "select * from a left join b on true",
            Dialect::PostgreSql,
            "left",
            "keyword"
        ));
        // A name after a dot is a name.
        let dotted = runs("select t.select from t", Dialect::PostgreSql);
        assert_eq!(
            dotted.iter().filter(|(_, k)| *k == "keyword").count(),
            2,
            "{dotted:?}"
        );
        let ranges = dialect_colours("select 'ä', 1", Dialect::PostgreSql);
        let number = ranges.iter().find(|c| c.kind == Some("number")).unwrap();
        assert_eq!(number.range, 12..13);
        // Runs are ordered and do not overlap, also for an unclosed string.
        for d in Dialect::ALL {
            let r = dialect_colours("select 'open -- x\n/* y", d);
            assert!(
                r.windows(2).all(|w| w[0].range.end <= w[1].range.start),
                "{d:?}"
            );
        }
    }
}
