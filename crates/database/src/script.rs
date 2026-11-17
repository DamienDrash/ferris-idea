//! Splitting SQL scripts into statements (`SPEC-0005`).
//!
//! A statement ends at a `;` that is not inside a string literal, a quoted
//! identifier, a comment or a PostgreSQL dollar-quoted body. Statements that
//! contain only whitespace and comments are dropped; comments before the
//! first token of a statement are not part of it. Lexical rules that differ
//! between dialects are listed in `SPEC-0005`.

use std::ops::Range;

use crate::Dialect;

/// One statement of a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Byte range in the script, trimmed, without the terminating `;`.
    pub range: Range<usize>,
    /// Statement text (the script sliced by `range`).
    pub text: String,
}

/// Splits `script` into statements using the lexical rules of `dialect`.
pub fn split_statements(script: &str, dialect: Dialect) -> Vec<Statement> {
    let mysql = dialect.mysql_like();
    let tsql = dialect == Dialect::SqlServer;
    let oracle = dialect == Dialect::Oracle;
    let sqlite = dialect == Dialect::Sqlite;
    let nested = nested_comments(dialect);
    let bytes = script.as_bytes();
    let mut out = Vec::new();
    let mut code_start: Option<usize> = None;
    // The `mysql` client's statement delimiter (v3 behaviour 16); `None` while it is `;`.
    let mut delimiter: Option<Vec<u8>> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if mysql
            && code_start.is_none()
            && let Some((end, new)) = delimiter_line(bytes, i)
        {
            if let Some(new) = new {
                delimiter = (new != b";").then_some(new);
            }
            i = end;
            continue;
        }
        if let Some(d) = &delimiter
            && bytes[i..].starts_with(d)
        {
            if let Some(start) = code_start.take() {
                push_trimmed(script, start..i, &mut out);
            }
            i += d.len();
            continue;
        }
        let custom = delimiter.is_some();
        match b {
            b'q' | b'Q' | b'n' | b'N' if oracle && q_quote_end(bytes, i).is_some() => {
                code_start.get_or_insert(i);
                i = q_quote_end(bytes, i).unwrap_or(bytes.len());
                continue;
            }
            b'e' | b'E' if dialect.postgres_like() && escape_string(bytes, i) => {
                code_start.get_or_insert(i);
                i = skip_quoted(bytes, i + 1, b'\'', true);
                continue;
            }
            b'\'' | b'"' | b'`' => {
                code_start.get_or_insert(i);
                i = skip_quoted(bytes, i, b, mysql);
                continue;
            }
            b'/' if oracle && slash_line_end(bytes, i).is_some() => {
                if let Some(start) = code_start.take() {
                    push_trimmed(script, start..i, &mut out);
                }
                i = slash_line_end(bytes, i).unwrap_or(bytes.len());
                continue;
            }
            b';' if custom => {}
            b';' if oracle && code_start.is_some_and(|s| plsql_block(&script[s..i])) => {}
            // The body of a trigger, procedure or function (SPEC-0005 v2 behaviours 9 to 11).
            b';' if !oracle && code_start.is_some_and(|s| inside_body(&script[s..i], dialect)) => {}
            b'[' if tsql => {
                code_start.get_or_insert(i);
                i = skip_bracketed(bytes, i);
                continue;
            }
            // SQLite quotes identifiers in brackets too (v3 behaviour 14), without `]]`.
            b'[' if sqlite => {
                code_start.get_or_insert(i);
                i = bytes[i + 1..]
                    .iter()
                    .position(|&c| c == b']')
                    .map_or(bytes.len(), |p| i + p + 2);
                continue;
            }
            b'g' | b'G' if tsql && go_line_end(bytes, i).is_some() => {
                if let Some(start) = code_start.take() {
                    push_trimmed(script, start..i, &mut out);
                }
                i = go_line_end(bytes, i).unwrap_or(bytes.len());
                continue;
            }
            b'-' if dash_comment(bytes, i, mysql) => {
                i = line_end(bytes, i);
                continue;
            }
            b'#' if mysql => {
                i = line_end(bytes, i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = block_comment_end(bytes, i, nested);
                continue;
            }
            b'$' if dialect.postgres_like() => {
                if let Some(tag_end) = dollar_tag_end(bytes, i) {
                    code_start.get_or_insert(i);
                    let tag = &bytes[i..=tag_end];
                    i = find(bytes, tag_end + 1, tag).map_or(bytes.len(), |p| p + tag.len());
                    continue;
                }
                code_start.get_or_insert(i);
            }
            b';' => {
                if let Some(start) = code_start.take() {
                    push_trimmed(script, start..i, &mut out);
                }
            }
            c if !c.is_ascii_whitespace() => {
                code_start.get_or_insert(i);
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(start) = code_start {
        push_trimmed(script, start..bytes.len(), &mut out);
    }
    out
}

/// The index just past the line `i` is on (past its `\n`), or the end.
fn line_end(bytes: &[u8], i: usize) -> usize {
    bytes[i..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(bytes.len(), |p| i + p + 1)
}

/// Whether block comments nest in `dialect` (`SPEC-0005` v3 behaviour 13).
pub(crate) fn nested_comments(dialect: Dialect) -> bool {
    dialect.postgres_like() || dialect == Dialect::SqlServer
}

/// The index just past the block comment opening at `i` (`/*`); with `nested` an inner `/*` opens
/// another level. An unclosed comment runs to the end.
pub(crate) fn block_comment_end(bytes: &[u8], i: usize, nested: bool) -> usize {
    let mut depth = 1;
    let mut j = i + 2;
    while j + 1 < bytes.len() {
        match (bytes[j], bytes[j + 1]) {
            (b'*', b'/') => {
                depth -= 1;
                j += 2;
                if depth == 0 {
                    return j;
                }
            }
            (b'/', b'*') if nested => {
                depth += 1;
                j += 2;
            }
            _ => j += 1,
        }
    }
    bytes.len()
}

/// Whether `--` at `i` starts a comment: always, except in MySQL and MariaDB, where a blank, a
/// control character or the end must follow (`SPEC-0005` v3 behaviour 15).
pub(crate) fn dash_comment(bytes: &[u8], i: usize, mysql: bool) -> bool {
    bytes.get(i + 1) == Some(&b'-')
        && (!mysql
            || bytes
                .get(i + 2)
                .is_none_or(|c| c.is_ascii_whitespace() || c.is_ascii_control()))
}

/// Whether a PostgreSQL escape string `E'…'` starts at `i` (`SPEC-0005` v3 behaviour 12).
pub(crate) fn escape_string(bytes: &[u8], i: usize) -> bool {
    bytes.get(i + 1) == Some(&b'\'')
        && (i == 0
            || !(bytes[i - 1].is_ascii_alphanumeric()
                || bytes[i - 1] == b'_'
                || bytes[i - 1] >= 0x80))
}

/// If a `mysql` client `DELIMITER` line starts its text at `i`, the index past the line and the new
/// delimiter (`None` when the line names none) (`SPEC-0005` v3 behaviour 16).
pub(crate) fn delimiter_line(bytes: &[u8], i: usize) -> Option<(usize, Option<Vec<u8>>)> {
    const WORD: &[u8] = b"delimiter";
    if !bytes.get(i..i + WORD.len())?.eq_ignore_ascii_case(WORD) {
        return None;
    }
    let line_start = bytes[..i]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    if !bytes[line_start..i].iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let after = i + WORD.len();
    if bytes.get(after).is_some_and(|c| !c.is_ascii_whitespace()) {
        return None;
    }
    let end = bytes[after..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(bytes.len(), |p| after + p);
    let new = bytes[after..end].trim_ascii();
    Some((
        (end + 1).min(bytes.len()),
        (!new.is_empty()).then(|| new.to_vec()),
    ))
}

/// The byte ranges of the block comments of `sql` as splitting sees them (`SPEC-0005` v5
/// behaviour 19): nested in PostgreSQL, DuckDB and SQL Server, to the first `*/` elsewhere; a `/*`
/// in a literal, a quoted identifier, a dollar quote or a line comment opens none. An unclosed
/// comment runs to the end.
pub fn block_comments(sql: &str, dialect: Dialect) -> Vec<std::ops::Range<usize>> {
    let mysql = dialect.mysql_like();
    let nested = nested_comments(dialect);
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'q' | b'Q' | b'n' | b'N' if dialect == Dialect::Oracle => {
                if let Some(end) = q_quote_end(bytes, i) {
                    i = end;
                    continue;
                }
            }
            b'e' | b'E' if dialect.postgres_like() && escape_string(bytes, i) => {
                i = skip_quoted(bytes, i + 1, b'\'', true);
                continue;
            }
            b'\'' | b'"' | b'`' => {
                i = skip_quoted(bytes, i, b, mysql);
                continue;
            }
            b'[' if dialect == Dialect::SqlServer => {
                i = skip_bracketed(bytes, i);
                continue;
            }
            b'-' if dash_comment(bytes, i, mysql) => {
                i = line_end(bytes, i);
                continue;
            }
            b'#' if mysql => {
                i = line_end(bytes, i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = block_comment_end(bytes, i, nested);
                out.push(i..end);
                i = end;
                continue;
            }
            b'$' if dialect.postgres_like() => {
                if let Some(tag_end) = dollar_tag_end(bytes, i) {
                    let tag = &bytes[i..=tag_end];
                    i = find(bytes, tag_end + 1, tag).map_or(bytes.len(), |p| p + tag.len());
                    continue;
                }
            }
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                // A word: an `e'…'` or `q'…'` start is only one at a word's beginning.
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Returns the index just past the closing quote. Doubled quotes are part of
/// the literal; backslash escapes only count in MySQL string literals.
/// Lower-cased words of `sql` outside literals, quoted identifiers and
/// comments, in order (used to classify statements, `SPEC-0012`).
pub(crate) fn code_words(sql: &str, dialect: Dialect) -> Vec<String> {
    code_tokens(sql, dialect, false)
}

/// Lower-cased words of `sql` outside literals, quoted identifiers and comments, in order, and
/// with `punctuation` also the characters `;`, `:`, `,`, `(` and `)` there, each a token.
fn code_tokens(sql: &str, dialect: Dialect, punctuation: bool) -> Vec<String> {
    let mysql = dialect.mysql_like();
    let bytes = sql.as_bytes();
    let mut words = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'q' | b'Q' | b'n' | b'N' if dialect == Dialect::Oracle => {
                if let Some(end) = q_quote_end(bytes, i) {
                    i = end;
                    continue;
                }
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                words.push(sql[start..i].to_ascii_lowercase());
                continue;
            }
            b'e' | b'E' if dialect.postgres_like() && escape_string(bytes, i) => {
                i = skip_quoted(bytes, i + 1, b'\'', true);
                continue;
            }
            b'\'' | b'"' | b'`' => {
                i = skip_quoted(bytes, i, b, mysql);
                continue;
            }
            b'[' if dialect == Dialect::SqlServer => {
                i = skip_bracketed(bytes, i);
                continue;
            }
            b'-' if dash_comment(bytes, i, mysql) => {
                i = line_end(bytes, i);
                continue;
            }
            b'#' if mysql => {
                i = line_end(bytes, i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = block_comment_end(bytes, i, nested_comments(dialect));
                continue;
            }
            b'$' if dialect.postgres_like() => {
                if let Some(tag_end) = dollar_tag_end(bytes, i) {
                    let tag = &bytes[i..=tag_end];
                    i = find(bytes, tag_end + 1, tag).map_or(bytes.len(), |p| p + tag.len());
                    continue;
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                words.push(sql[start..i].to_ascii_lowercase());
                continue;
            }
            b';' | b':' | b',' | b'(' | b')' if punctuation => {
                words.push((b as char).to_string());
            }
            _ => {}
        }
        i += 1;
    }
    words
}

pub(crate) fn skip_quoted(bytes: &[u8], open: usize, quote: u8, backslash_escapes: bool) -> usize {
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if backslash_escapes && quote != b'`' => i += 2,
            c if c == quote => {
                if bytes.get(i + 1) == Some(&quote) {
                    i += 2;
                } else {
                    return i + 1;
                }
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Returns the index just past the `]` closing a T-SQL `[identifier]`; `]]` is part of it.
pub(crate) fn skip_bracketed(bytes: &[u8], open: usize) -> usize {
    let mut i = open + 1;
    while i < bytes.len() {
        if bytes[i] == b']' {
            if bytes.get(i + 1) == Some(&b']') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// If a T-SQL batch separator line (`GO`, optionally with a count) starts at `i`, returns the
/// index just past its line. `GO` must be the only word on its line.
pub(crate) fn go_line_end(bytes: &[u8], i: usize) -> Option<usize> {
    let line_start = bytes[..i]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    if !bytes[line_start..i].iter().all(|c| c.is_ascii_whitespace()) {
        return None;
    }
    if !bytes.get(i..i + 2)?.eq_ignore_ascii_case(b"go") {
        return None;
    }
    let line_end = bytes[i..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(bytes.len(), |p| i + p);
    let rest = &bytes[i + 2..line_end];
    let rest = rest.trim_ascii();
    (rest.is_empty() || rest.iter().all(u8::is_ascii_digit))
        .then(|| (line_end + 1).min(bytes.len()))
}

/// If an Oracle alternative-quoted literal (`q'[…]'`, `nq'!…!'`) starts at `i`, returns the index
/// just past it. The delimiter after the quote closes with its pair (`[]`, `{}`, `()`, `<>`) or
/// itself, followed by a quote (SPEC-0135).
pub(crate) fn q_quote_end(bytes: &[u8], i: usize) -> Option<usize> {
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        return None;
    }
    let mut j = i;
    if bytes[j].eq_ignore_ascii_case(&b'n') {
        j += 1;
    }
    if !bytes.get(j)?.eq_ignore_ascii_case(&b'q') || *bytes.get(j + 1)? != b'\'' {
        return None;
    }
    let open = *bytes.get(j + 2)?;
    if open.is_ascii_whitespace() {
        return None;
    }
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        b'(' => b')',
        b'<' => b'>',
        c => c,
    };
    let body = j + 3;
    Some(
        (body..bytes.len().saturating_sub(1))
            .find(|&k| bytes[k] == close && bytes[k + 1] == b'\'')
            .map_or(bytes.len(), |k| k + 2),
    )
}

/// If a line holding only `/` starts its non-blank text at `i` (SQL*Plus runs the buffer), returns
/// the index just past that line.
pub(crate) fn slash_line_end(bytes: &[u8], i: usize) -> Option<usize> {
    let line_start = bytes[..i]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    if !bytes[line_start..i].iter().all(|c| c.is_ascii_whitespace()) {
        return None;
    }
    let line_end = bytes[i..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(bytes.len(), |p| i + p);
    bytes[i + 1..line_end]
        .trim_ascii()
        .is_empty()
        .then(|| (line_end + 1).min(bytes.len()))
}

/// Whether a statement beginning with `head` is a PL/SQL block, whose `;` end its inner
/// statements: an anonymous block (`DECLARE`, `BEGIN`) or a stored unit (`CREATE [OR REPLACE]
/// [EDITIONABLE|NONEDITIONABLE] PROCEDURE|FUNCTION|PACKAGE|TRIGGER|TYPE`).
fn plsql_block(head: &str) -> bool {
    let mut words = head
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase);
    match words.next().as_deref() {
        Some("declare" | "begin") => true,
        Some("create") => {
            for w in words.take(4) {
                match w.as_str() {
                    "or" | "replace" | "editionable" | "noneditionable" => {}
                    "procedure" | "function" | "package" | "trigger" | "type" => return true,
                    _ => return false,
                }
            }
            false
        }
        _ => false,
    }
}

/// How the `;` inside a statement beginning with a stored unit are treated (`SPEC-0005` v2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Body {
    /// No stored unit: every `;` ends the statement.
    None,
    /// A body of `BEGIN … END` blocks (SQLite triggers; MySQL and MariaDB triggers, procedures,
    /// functions and events; PostgreSQL and DuckDB `BEGIN ATOMIC`): a `;` inside a block does not
    /// end it.
    Blocks,
    /// A T-SQL `CREATE`/`ALTER` of a procedure, function, trigger or view, or a statement that
    /// declares a variable or begins control of flow (`DECLARE`, `IF`, `WHILE`, a `BEGIN` block):
    /// it runs to the end of its batch (`GO` or the end of the script).
    Batch,
}

/// What kind of stored unit a statement whose code words start with `words` is.
fn body_of(words: &[String], dialect: Dialect) -> Body {
    let mut words = words.iter().map(String::as_str);
    let first = words.next();
    if dialect == Dialect::SqlServer {
        match first {
            // A variable lives for its batch, and control of flow takes the statements after it as
            // its body: the rest of the batch is one statement (v6 behaviour 11a).
            Some("declare" | "if" | "while") => return Body::Batch,
            Some("begin") => {
                return match words.next() {
                    Some("tran" | "transaction" | "distributed") => Body::None,
                    _ => Body::Batch,
                };
            }
            Some("create" | "alter") => {}
            _ => return Body::None,
        }
        for w in words.take(3) {
            match w {
                "or" | "alter" => {}
                "procedure" | "proc" | "function" | "trigger" | "view" => return Body::Batch,
                _ => return Body::None,
            }
        }
        return Body::None;
    }
    if first != Some("create") {
        return Body::None;
    }
    let units: &[&str] = match dialect {
        Dialect::Sqlite => &["trigger"],
        Dialect::MySql | Dialect::MariaDb => &["trigger", "procedure", "function", "event"],
        Dialect::PostgreSql | Dialect::DuckDb => &["function", "procedure"],
        Dialect::SqlServer | Dialect::Oracle => &[],
    };
    // Between `CREATE` and the unit stand only modifiers: `OR REPLACE`, `TEMP`, `DEFINER = …`,
    // `IF NOT EXISTS`, `AGGREGATE`, `SQL SECURITY …`.
    for w in words.take(8) {
        if units.contains(&w) {
            return Body::Blocks;
        }
        if matches!(
            w,
            "table"
                | "view"
                | "index"
                | "unique"
                | "schema"
                | "database"
                | "sequence"
                | "virtual"
                | "user"
                | "role"
                | "type"
                | "domain"
                | "extension"
                | "materialized"
                | "as"
        ) {
            return Body::None;
        }
    }
    Body::None
}

/// Whether the statement `text` so far is a stored unit whose body is still open, so the `;`
/// after it does not end the statement.
fn inside_body(text: &str, dialect: Dialect) -> bool {
    // The head decides cheaply; only a stored unit's text is read as a whole.
    let head_end = text.char_indices().nth(400).map_or(text.len(), |(b, _)| b);
    match body_of(&code_words(&text[..head_end], dialect), dialect) {
        Body::None => false,
        Body::Batch => true,
        Body::Blocks => open_blocks(&code_tokens(text, dialect, true), dialect) > 0,
    }
}

/// How many `BEGIN … END` blocks are open after `tokens`. `CASE` (an expression or MySQL's
/// statement) closes with `END` as well. In MySQL and MariaDB `IF`, `WHILE`, `REPEAT` and `LOOP`
/// at the start of a statement open a block that `END IF`, `END WHILE`, `END REPEAT` and
/// `END LOOP` close; `IF(a, b, c)` and `REPEAT('x', 3)` are functions.
fn open_blocks(tokens: &[String], dialect: Dialect) -> i64 {
    let mysql = dialect.mysql_like();
    let mut depth: i64 = 0;
    let mut i = 0;
    while i < tokens.len() {
        let prev = if i == 0 { ";" } else { tokens[i - 1].as_str() };
        match tokens[i].as_str() {
            "begin" | "case" => depth += 1,
            "end" => {
                depth -= 1;
                if tokens.get(i + 1).is_some_and(|n| {
                    matches!(n.as_str(), "if" | "case" | "loop" | "while" | "repeat")
                }) {
                    i += 1;
                }
            }
            "if" | "while" | "repeat" | "loop"
                if mysql
                    && matches!(
                        prev,
                        ";" | ":" | "begin" | "then" | "else" | "do" | "loop" | "repeat"
                    )
                    && !function_call(tokens, i) =>
            {
                depth += 1
            }
            _ => {}
        }
        i += 1;
    }
    depth
}

/// Whether the word at `i` is called as a function: `(` follows, and the parentheses hold a `,`
/// at their own level (`IF(a, b, c)`), which a condition in parentheses does not.
fn function_call(tokens: &[String], i: usize) -> bool {
    if tokens.get(i + 1).map(String::as_str) != Some("(") {
        return false;
    }
    let mut level = 0;
    for t in &tokens[i + 1..] {
        match t.as_str() {
            "(" => level += 1,
            ")" => {
                level -= 1;
                if level == 0 {
                    return false;
                }
            }
            "," if level == 1 => return true,
            _ => {}
        }
    }
    false
}

/// If a dollar-quote tag (`$$` or `$name$`) starts at `i`, returns the index
/// of its closing `$`.
pub(crate) fn dollar_tag_end(bytes: &[u8], i: usize) -> Option<usize> {
    // `$1` is a positional parameter, not a tag.
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        return None;
    }
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'$' => return Some(j),
            c if c.is_ascii_alphabetic() || c == b'_' => j += 1,
            c if c.is_ascii_digit() && j > i + 1 => j += 1,
            _ => return None,
        }
    }
    None
}

pub(crate) fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn push_trimmed(script: &str, range: Range<usize>, out: &mut Vec<Statement>) {
    let slice = &script[range.clone()];
    let lead = slice.len() - slice.trim_start().len();
    let trimmed = slice.trim();
    let start = range.start + lead;
    out.push(Statement {
        range: start..start + trimmed.len(),
        text: trimmed.to_owned(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0005 v5 AK-20 (OBS-0051 L7).
    #[test]
    fn block_comments_follow_the_dialect() {
        let sql = "/* a /* b */ c */ select 1;";
        let text = |d: Dialect| {
            block_comments(sql, d)
                .into_iter()
                .map(|r| &sql[r])
                .collect::<Vec<_>>()
        };
        for d in [Dialect::PostgreSql, Dialect::DuckDb, Dialect::SqlServer] {
            assert_eq!(text(d), ["/* a /* b */ c */"], "{d:?}");
        }
        for d in [
            Dialect::MySql,
            Dialect::MariaDb,
            Dialect::Sqlite,
            Dialect::Oracle,
        ] {
            assert_eq!(text(d), ["/* a /* b */"], "{d:?}");
        }
        let skipped = "select '/* no', $x$ /* no $x$, \"/*\" -- /* no\n/* yes */";
        let found: Vec<_> = block_comments(skipped, Dialect::PostgreSql)
            .into_iter()
            .map(|r| &skipped[r])
            .collect();
        assert_eq!(found, ["/* yes */"]);
        assert_eq!(block_comments("/* open", Dialect::PostgreSql), vec![(0..7)]);
    }

    fn texts_in(script: &str, dialect: Dialect) -> Vec<String> {
        split_statements(script, dialect)
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    fn texts(script: &str) -> Vec<String> {
        texts_in(script, Dialect::PostgreSql)
    }

    #[test]
    fn splits_on_semicolons_and_trims() {
        assert_eq!(
            texts("select 1;\n  select 2 ;select 3"),
            ["select 1", "select 2", "select 3"]
        );
    }

    #[test]
    fn ignores_semicolons_in_literals_identifiers_and_comments() {
        let script = "insert into t values ('a;b', 'it''s;');\n\
                      select \"x;y\" from t; -- c;omment\n\
                      /* block; comment */ select 3;";
        assert_eq!(
            texts(script),
            [
                "insert into t values ('a;b', 'it''s;')",
                "select \"x;y\" from t",
                "select 3"
            ]
        );
    }

    #[test]
    fn mysql_rules_backticks_hash_comments_and_backslash_escapes() {
        let script = "select `a;b`, 'x\\';y' from t; # c;omment\nselect 2;";
        assert_eq!(
            texts_in(script, Dialect::MySql),
            ["select `a;b`, 'x\\';y' from t", "select 2"]
        );
    }

    #[test]
    fn t_sql_brackets_and_go_batches() {
        let script = "select [a;b], 'x;y' from t\nGO\ncreate procedure p as select 1\n  go 2\n\
                      select [x]]y;z] from t; select 'gone'\ngoto_label:";
        assert_eq!(
            texts_in(script, Dialect::SqlServer),
            [
                "select [a;b], 'x;y' from t",
                "create procedure p as select 1",
                "select [x]]y;z] from t",
                "select 'gone'\ngoto_label:"
            ]
        );
        // Elsewhere `go` is an ordinary word and `[` is not a quote.
        assert_eq!(
            texts_in("select 1\ngo\nselect [a;b]", Dialect::PostgreSql),
            ["select 1\ngo\nselect [a", "b]"]
        );
    }

    #[test]
    fn mariadb_follows_mysql_lexis() {
        let script = "select `a;b`, 'x\\';y' from t; # c;omment\nselect 2;";
        assert_eq!(
            texts_in(script, Dialect::MariaDb),
            ["select `a;b`, 'x\\';y' from t", "select 2"]
        );
    }

    #[test]
    fn postgres_hash_is_an_operator_and_backslash_is_literal() {
        // `#` is bitwise XOR in PostgreSQL; a backslash does not escape in a
        // standard string, so the literal ends at the second quote.
        let script = "select 5 # 3; select 'a\\'; select 2";
        assert_eq!(texts(script), ["select 5 # 3", "select 'a\\'", "select 2"]);
    }

    #[test]
    fn dollar_quotes_only_in_postgres() {
        assert_eq!(
            texts_in("select $$a;b$$", Dialect::Sqlite),
            ["select $$a", "b$$"]
        );
    }

    #[test]
    fn handles_dollar_quoted_bodies_and_parameters() {
        let script = "create function f() returns int as $body$ begin; return 1; end $body$ language plpgsql;\n\
                      select $1 + $$;$$;";
        let got = texts(script);
        assert_eq!(got.len(), 2);
        assert!(got[0].ends_with("language plpgsql"));
        assert_eq!(got[1], "select $1 + $$;$$");
    }

    #[test]
    fn drops_comment_only_and_empty_statements() {
        assert_eq!(
            texts(";;\n-- only a comment\n;  /* x */ ;\nselect 1;"),
            ["select 1"]
        );
        assert!(texts("").is_empty());
    }

    #[test]
    fn ranges_point_into_the_script() {
        let script = "  select 1 ;\nselect 2";
        for s in split_statements(script, Dialect::Sqlite) {
            assert_eq!(&script[s.range.clone()], s.text);
        }
    }

    #[test]
    fn unterminated_literal_runs_to_end() {
        assert_eq!(
            texts("select 'open; still open"),
            ["select 'open; still open"]
        );
    }

    #[test]
    fn oracle_plsql_blocks_slash_lines_and_q_quotes() {
        let script = "select q'[it's; fine]' from dual;\n\
                      create or replace procedure p is\nbegin\n  null;\n  dbms_output.put_line('a;b');\nend;\n/\n\
                      begin\n  p;\nend;\n/\n\
                      select nq'!x;y!' , 1/2 from dual\n/\n\
                      update t set a = 1;";
        assert_eq!(
            texts_in(script, Dialect::Oracle),
            [
                "select q'[it's; fine]' from dual",
                "create or replace procedure p is\nbegin\n  null;\n  dbms_output.put_line('a;b');\nend;",
                "begin\n  p;\nend;",
                "select nq'!x;y!' , 1/2 from dual",
                "update t set a = 1"
            ]
        );
        // A plain `create table` still ends at `;`; a word ending in q starts no quote.
        assert_eq!(
            texts_in(
                "create table t (a int); select seq'x;y' from dual",
                Dialect::Oracle
            ),
            ["create table t (a int)", "select seq'x;y' from dual"]
        );
        // A `/` after a statement that already ended at `;` runs nothing a second time.
        assert_eq!(
            texts_in(
                "select 1 from dual;\n/\nselect 2 from dual",
                Dialect::Oracle
            ),
            ["select 1 from dual", "select 2 from dual"]
        );
    }

    /// SPEC-0005 v2 AK-9: an SQLite trigger's body stays one statement, with a CASE expression
    /// inside; what follows splits again.
    #[test]
    fn sqlite_trigger_bodies_are_one_statement() {
        let trigger = "create temp trigger if not exists audit after update on t\n\
                       when new.a > 0\n\
                       begin\n  insert into log values (new.a, case when new.a > 9 then 'big' else 'small' end);\n  \
                       update t set b = 1 where id = new.id;\n\
                       end";
        let script = format!("{trigger};\nselect 1;\ncreate table x(a);\n");
        assert_eq!(
            texts_in(&script, Dialect::Sqlite),
            [trigger, "select 1", "create table x(a)"]
        );
        // `BEGIN` alone is a transaction, not a body.
        assert_eq!(
            texts_in("begin; insert into t values (1); commit;", Dialect::Sqlite),
            ["begin", "insert into t values (1)", "commit"]
        );
    }

    /// SPEC-0005 v2 AK-10: MySQL and MariaDB procedures, functions, triggers and events with nested
    /// blocks; `IF(…)` and `REPEAT(…)` are functions; `DROP TABLE IF EXISTS` is no block.
    #[test]
    fn mysql_compound_bodies_are_one_statement() {
        let procedure = "create definer = `root`@`localhost` procedure fill(in n int)\n\
                         begin\n  declare i int default 0;\n  drop table if exists tmp;\n  \
                         l1: while i < n do\n    if (i % 2 = 0) then\n      insert into t values (i, if(i > 5, 'a;b', repeat('x', 2)));\n    \
                         elseif i = 3 then\n      set i = i + 1;\n    else\n      begin\n        set @x = 1;\n      end;\n    end if;\n    \
                         case i when 1 then set i = i + 1; else set i = i + 1; end case;\n  \
                         end while l1;\n  repeat set i = i - 1; until i < 0 end repeat;\n  loop leave l1; end loop;\nend";
        let function = "create function twice(x int) returns int deterministic return x * 2";
        let trigger = "create trigger t_bi before insert on t for each row\nbegin\n  set new.a = coalesce(new.a, 0);\nend";
        let event =
            "create event if not exists e on schedule every 1 day do\nbegin\n  delete from t;\nend";
        let script = format!("{procedure};\n{function};\n{trigger};\n{event};\nselect 1;");
        for dialect in [Dialect::MySql, Dialect::MariaDb] {
            assert_eq!(
                texts_in(&script, dialect),
                [procedure, function, trigger, event, "select 1"],
                "{dialect:?}"
            );
        }
    }

    /// SPEC-0005 v2 AK-10: PostgreSQL's and DuckDB's standard bodies (`BEGIN ATOMIC … END`);
    /// dollar-quoted bodies as before.
    #[test]
    fn postgres_begin_atomic_bodies_are_one_statement() {
        let function = "create or replace function add(a int, b int) returns int language sql\n\
                        begin atomic\n  select a + b;\nend";
        let script = format!("{function};\nselect add(1, 2);");
        for dialect in [Dialect::PostgreSql, Dialect::DuckDb] {
            assert_eq!(
                texts_in(&script, dialect),
                [function, "select add(1, 2)"],
                "{dialect:?}"
            );
        }
    }

    /// SPEC-0005 v2 AK-11: a T-SQL procedure, function, trigger or view runs to the end of its
    /// batch; other statements split at `;` as before.
    #[test]
    fn t_sql_units_run_to_the_end_of_their_batch() {
        let procedure = "create or alter procedure dbo.fill as\nbegin\n  set nocount on;\n  insert into t values (1);\nend;";
        let script = format!("select 1;\ngo\n{procedure}\ngo\nexec dbo.fill; select 2;\n");
        assert_eq!(
            texts_in(&script, Dialect::SqlServer),
            [
                "select 1",
                // The batch ends at `GO`; the `;` is the body's last statement's, as in Oracle.
                procedure,
                "exec dbo.fill",
                "select 2"
            ]
        );
        // `CREATE TABLE` is no unit of its own.
        assert_eq!(
            texts_in("create table t(a int); select 1;", Dialect::SqlServer),
            ["create table t(a int)", "select 1"]
        );
    }

    /// SPEC-0005 v6 AK-21: a T-SQL variable or control of flow keeps the rest of its batch.
    #[test]
    fn t_sql_variables_and_blocks_keep_their_batch() {
        let variable = "declare @n int = 2;\nselect @n as n;";
        let attempt = "begin try\n  select 1/0 as x;\nend try\nbegin catch\n  select error_number();\nend catch";
        let branch = "if 1 = 1\nbegin\n  select 'a';\n  select 'b';\nend";
        let loop_ = "while @i < 3\n  set @i += 1;\nselect @i";
        let script = format!(
            "create table t (a int); insert into t values (1);\n{variable}\nGO\n{attempt}\nGO\n\
             {branch}\ngo\ndeclare @i int = 0;\n{loop_}\nGO\nbegin tran; update t set a = 2; commit;"
        );
        assert_eq!(
            texts_in(&script, Dialect::SqlServer),
            [
                "create table t (a int)",
                "insert into t values (1)",
                // The batch's `;` stay in the text, the last one too (as for a procedure).
                variable,
                attempt,
                branch,
                &format!("declare @i int = 0;\n{loop_}"),
                "begin tran",
                "update t set a = 2",
                "commit"
            ]
        );
        // `BEGIN TRANSACTION` and `BEGIN DISTRIBUTED TRANSACTION` are no blocks.
        assert_eq!(
            texts_in(
                "begin transaction x; select 1; begin distributed transaction; select 2",
                Dialect::SqlServer
            ),
            [
                "begin transaction x",
                "select 1",
                "begin distributed transaction",
                "select 2"
            ]
        );
        // Elsewhere `DECLARE` and `IF` change nothing.
        assert_eq!(
            texts_in("declare x int; select 1;", Dialect::PostgreSql),
            ["declare x int", "select 1"]
        );
    }

    /// SPEC-0005 v3 AK-12: `E'…'` takes backslash escapes in PostgreSQL and DuckDB only.
    #[test]
    fn postgres_escape_strings() {
        let script = r"select E'it\'s; x'; select 2";
        for d in [Dialect::PostgreSql, Dialect::DuckDb] {
            assert_eq!(texts_in(script, d), [r"select E'it\'s; x'", "select 2"]);
        }
        assert_eq!(texts_in(script, Dialect::Sqlite).len(), 2);
        // A word ending in `e` before a quote is no escape string; a plain literal keeps `\` as text.
        assert_eq!(
            texts_in(r"select 'a\'; select 2", Dialect::PostgreSql),
            [r"select 'a\'", "select 2"]
        );
        assert_eq!(
            texts_in(r"select some'x\'; select 2", Dialect::PostgreSql),
            [r"select some'x\'", "select 2"]
        );
    }

    /// SPEC-0005 v3 AK-13.
    #[test]
    fn block_comments_nest_where_the_database_nests_them() {
        let script = "/* a /* b */ ; */ select 1; select 2";
        for d in [Dialect::PostgreSql, Dialect::DuckDb, Dialect::SqlServer] {
            assert_eq!(texts_in(script, d), ["select 1", "select 2"], "{d:?}");
        }
        for d in [Dialect::MySql, Dialect::Sqlite, Dialect::Oracle] {
            assert_eq!(
                texts_in(script, d),
                ["*/ select 1", "select 2"],
                "{d:?}: the first */ ends the comment"
            );
        }
    }

    /// SPEC-0005 v3 AK-14.
    #[test]
    fn sqlite_brackets_quote_identifiers() {
        assert_eq!(
            texts_in("select [a;b] from t; select 2", Dialect::Sqlite),
            ["select [a;b] from t", "select 2"]
        );
        // In PostgreSQL `[` is an array subscript, not a quote.
        assert_eq!(
            texts_in("select a[1]; select 2", Dialect::PostgreSql),
            ["select a[1]", "select 2"]
        );
    }

    /// SPEC-0005 v3 AK-15.
    #[test]
    fn mysql_double_dash_needs_a_blank() {
        for d in [Dialect::MySql, Dialect::MariaDb] {
            assert_eq!(
                texts_in("select 5--1; select 2", d),
                ["select 5--1", "select 2"]
            );
            assert_eq!(
                texts_in("select 1 -- c; x\n; select 2", d),
                ["select 1 -- c; x", "select 2"]
            );
            assert_eq!(texts_in("select 1; --", d), ["select 1"]);
        }
        assert_eq!(
            texts_in("select 5--1; select 2", Dialect::PostgreSql),
            ["select 5--1; select 2"]
        );
    }

    /// SPEC-0005 v3 AK-16: the `mysql` client's `DELIMITER` command.
    #[test]
    fn mysql_delimiter_command() {
        let procedure = "create procedure p()\nbegin\n  select 1;\n  select ';//';\nend";
        let script = format!(
            "DELIMITER //\n{procedure} //\ndelimiter ;\nselect 2;\nDELIMITER $$\nselect 3$$\nselect 4 $$\n"
        );
        for d in [Dialect::MySql, Dialect::MariaDb] {
            assert_eq!(
                texts_in(&script, d),
                [procedure, "select 2", "select 3", "select 4"],
                "{d:?}"
            );
        }
        // The word in a literal or in the middle of a statement is text.
        assert_eq!(
            texts_in(
                "select 'x\ndelimiter //'; select\ndelimiter; select 2",
                Dialect::MySql
            ),
            ["select 'x\ndelimiter //'", "select\ndelimiter", "select 2"]
        );
        // A bare `DELIMITER` line changes nothing and is not sent.
        assert_eq!(
            texts_in("delimiter\nselect 1; select 2", Dialect::MySql),
            ["select 1", "select 2"]
        );
        // PostgreSQL has no such command.
        assert_eq!(
            texts_in("DELIMITER //\nselect 1;", Dialect::PostgreSql),
            ["DELIMITER //\nselect 1"]
        );
        // Ranges still point into the script.
        for s in split_statements(&script, Dialect::MySql) {
            assert_eq!(&script[s.range.clone()], s.text);
        }
    }
}
