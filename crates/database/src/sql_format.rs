//! IDECraft's own SQL formatter (`SPEC-0043` v6 behaviour 11): one clause per line, joins
//! under the table name of their `from` line, `and`/`or` of a condition aligned under `where`,
//! one row per line after `values`, one column per line in `create table` between parentheses on
//! lines of their own and with the types aligned, spaces around operators and after commas.
//!
//! It changes whitespace between tokens and nothing else: keyword case, comments, string
//! contents and quoted names stay as written. The text is read without knowing the dialect, so
//! every reading in which a piece of text could be a literal or a comment keeps that piece as it
//! is, and a text whose tokens depend on the dialect is refused. Statements it has no layout for
//! (anything but queries, `insert`, `update`, `delete`, `create table` and `create view`) stay as
//! written. Before a result is handed out it is read again: its tokens must be the tokens of the
//! source, and formatting it once more must change nothing.

use crate::script::{dollar_tag_end, find};

/// What a token is, as far as the layout needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Keyword, name, number, parameter (`:name`, `@name`, `$1`).
    Word,
    /// String, quoted name, dollar-quoted body, `[bracketed]` text: kept as it is.
    Quoted,
    /// `-- …` or `# …` to the end of the line.
    LineComment,
    /// `/* … */`.
    BlockComment,
    /// Operator.
    Op,
    Comma,
    Semi,
    Open,
    Close,
    Dot,
    /// Anything else (`{`, `}`, `\`, a lone `[` or `$`).
    Other,
}

/// One token and the whitespace before it.
#[derive(Debug, Clone)]
struct Tok {
    kind: Kind,
    start: usize,
    end: usize,
    /// Whitespace stands before it.
    space: bool,
    /// Line breaks in that whitespace.
    newlines: usize,
}

impl Tok {
    fn is_comment(&self) -> bool {
        matches!(self.kind, Kind::LineComment | Kind::BlockComment)
    }
}

/// Operators of more than one character that are read as one token, longest first.
const OPERATORS: [&str; 22] = [
    "->>", "<=>", "!~*", "<=", ">=", "<>", "!=", "||", "::", ":=", "->", "@>", "<@", "&&", "<<",
    ">>", "==", "!~", "~*", "=>", "?|", "?&",
];

const OPERATOR_CHARS: &[u8] = b"+-*/<>=~!@%^&|?:";

fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

fn line_of(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Index just past the quote that closes the literal opened at `open`; `None` when it never
/// closes. A doubled quote is part of the literal, and with `backslash` a backslash takes the
/// next character with it (MySQL strings).
fn quoted_end(bytes: &[u8], open: usize, backslash: bool) -> Option<usize> {
    let quote = bytes[open];
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if backslash && quote != b'`' => i += 2,
            c if c == quote => {
                if bytes.get(i + 1) == Some(&quote) {
                    i += 2;
                } else {
                    return Some(i + 1);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// The tokens of `text`. `backslash` chooses how a backslash in a string is read.
fn lex(text: &str, backslash: bool) -> Result<Vec<Tok>, String> {
    let bytes = text.as_bytes();
    let mut tokens: Vec<Tok> = Vec::new();
    let (mut i, mut space, mut newlines) = (0, false, 0);
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            space = true;
            newlines += usize::from(b == b'\n');
            i += 1;
            continue;
        }
        let line_end = || {
            bytes[i..]
                .iter()
                .position(|&c| c == b'\n')
                .map_or(bytes.len(), |p| i + p)
        };
        let (kind, end) = match b {
            b'-' if bytes.get(i + 1) == Some(&b'-') => (Kind::LineComment, line_end()),
            // A comment in MySQL, an operator elsewhere: kept as it is to the end of the line.
            b'#' => (Kind::LineComment, line_end()),
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let close = find(bytes, i + 2, b"*/").ok_or_else(|| {
                    format!(
                        "line {}: the comment that starts here never ends",
                        line_of(text, i)
                    )
                })?;
                // PostgreSQL nests comments, other databases end at the first `*/`.
                if find(&bytes[..close], i + 2, b"/*").is_some() {
                    return Err(format!(
                        "line {}: a comment opens inside this comment, which databases read differently",
                        line_of(text, i)
                    ));
                }
                (Kind::BlockComment, close + 2)
            }
            b'\'' | b'"' | b'`' => {
                let end = quoted_end(bytes, i, backslash).ok_or_else(|| {
                    format!(
                        "line {}: the quoted text that starts here never ends",
                        line_of(text, i)
                    )
                })?;
                (Kind::Quoted, end)
            }
            b'$' if dollar_tag_end(bytes, i).is_some() => {
                let tag_end = dollar_tag_end(bytes, i).unwrap_or(i);
                let tag = &bytes[i..=tag_end];
                let close = find(bytes, tag_end + 1, tag).ok_or_else(|| {
                    format!(
                        "line {}: the dollar-quoted text that starts here never ends",
                        line_of(text, i)
                    )
                })?;
                (Kind::Quoted, close + tag.len())
            }
            // `[name]` in SQL Server, an array index elsewhere: kept as it is.
            b'[' => match bytes[i..line_end()].iter().position(|&c| c == b']') {
                Some(p) => (Kind::Quoted, i + p + 1),
                None => (Kind::Other, i + 1),
            },
            b',' => (Kind::Comma, i + 1),
            b';' => (Kind::Semi, i + 1),
            b'(' => (Kind::Open, i + 1),
            b')' => (Kind::Close, i + 1),
            b'.' => (Kind::Dot, i + 1),
            _ => {
                let sigil = match b {
                    b'@' if bytes.get(i + 1) == Some(&b'@') => 2,
                    b'@' | b'$' => 1,
                    b':' if i == 0 || bytes[i - 1] != b':' => 1,
                    _ => 0,
                };
                let named = |at: usize| {
                    bytes
                        .get(at)
                        .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80)
                };
                if word_byte(b) && b != b'$' || sigil > 0 && named(i + sigil) {
                    (Kind::Word, word_end(bytes, i, sigil))
                } else if OPERATOR_CHARS.contains(&b) {
                    let rest = &text[i..];
                    let len = OPERATORS
                        .iter()
                        .find(|op| rest.starts_with(**op))
                        .map_or(1, |op| op.len());
                    (Kind::Op, i + len)
                } else {
                    // One character, whatever its width.
                    let len = text[i..].chars().next().map_or(1, char::len_utf8);
                    (Kind::Other, i + len)
                }
            }
        };
        tokens.push(Tok {
            kind,
            start: i,
            end,
            space,
            newlines,
        });
        (space, newlines) = (false, 0);
        i = end;
    }
    Ok(tokens)
}

/// End of the word that starts at `start` after `sigil` bytes of `@`, `@@`, `$` or `:`. A number
/// takes its fraction and its exponent with it (`1.5`, `2e-3`).
fn word_end(bytes: &[u8], start: usize, sigil: usize) -> usize {
    let mut i = start + sigil;
    while i < bytes.len() && word_byte(bytes[i]) {
        i += 1;
    }
    if sigil > 0 || !bytes[start].is_ascii_digit() {
        return i;
    }
    if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while i < bytes.len() && word_byte(bytes[i]) {
            i += 1;
        }
    }
    let mantissa = &bytes[start..i - 1];
    if matches!(bytes[i - 1], b'e' | b'E')
        && !mantissa.is_empty()
        && mantissa.iter().all(|c| c.is_ascii_digit() || *c == b'.')
        && matches!(bytes.get(i), Some(b'+' | b'-'))
        && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
    {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    i
}

/// The tokens of `text` when every dialect reads them alike; an error when the text cannot be
/// read, or when a backslash in a string makes MySQL end it elsewhere than the others.
fn lex_agreed(text: &str) -> Result<Vec<Tok>, String> {
    let spans = |t: &[Tok]| t.iter().map(|t| (t.start, t.end)).collect::<Vec<_>>();
    match (lex(text, false), lex(text, true)) {
        (Ok(a), Ok(b)) if spans(&a) == spans(&b) => Ok(a),
        (Err(e), Err(_)) => Err(e),
        (Ok(a), Ok(b)) => {
            let at = a
                .iter()
                .zip(&b)
                .find(|(x, y)| (x.start, x.end) != (y.start, y.end))
                .map_or(0, |(x, _)| x.start);
            Err(backslash_message(text, at))
        }
        // Only the backslash is read differently, so it is what one reading fails on.
        _ => {
            let at = text.find("\\'").or_else(|| text.find("\\\"")).unwrap_or(0);
            Err(backslash_message(text, at))
        }
    }
}

fn backslash_message(text: &str, at: usize) -> String {
    format!(
        "line {}: a backslash before a quote ends this string in standard SQL and not in MySQL, so the file reads differently per database",
        line_of(text, at)
    )
}

/// The texts of the tokens of `text`, whitespace left out: what formatting must not change.
pub fn sql_tokens(text: &str) -> Result<Vec<String>, String> {
    Ok(lex_agreed(text)?
        .iter()
        .map(|t| text[t.start..t.end].to_owned())
        .collect())
}

/// Lays out `text` with `indent` spaces per level; an error (nothing to change then) when the
/// text cannot be read as SQL without doubt. Line breaks stay `\r\n` in a file that has only
/// those.
pub fn format_sql(text: &str, indent: usize) -> Result<String, String> {
    let indent = indent.clamp(1, 16);
    let crlf = text.contains("\r\n") && !text.replace("\r\n", "").contains('\n');
    if crlf {
        let unix = text.replace("\r\n", "\n");
        return Ok(format_checked(&unix, indent)?.replace('\n', "\r\n"));
    }
    format_checked(text, indent)
}

fn format_checked(text: &str, indent: usize) -> Result<String, String> {
    let tokens = lex_agreed(text)?;
    if tokens.is_empty() {
        return Ok(text.to_owned());
    }
    let out = layout(text, &tokens, indent)?;
    // The promise, checked on every result: same tokens, and nothing left to do.
    let unsafe_result = || {
        "IDECraft's SQL formatter would have changed more than whitespace here, so it left the file as it is (please report the file)"
            .to_owned()
    };
    let after = lex_agreed(&out).map_err(|_| unsafe_result())?;
    let same = tokens.len() == after.len()
        && tokens
            .iter()
            .zip(&after)
            .all(|(a, b)| a.kind == b.kind && text[a.start..a.end] == out[b.start..b.end]);
    if !same || layout(&out, &after, indent).as_deref() != Ok(out.as_str()) {
        return Err(unsafe_result());
    }
    Ok(out)
}

/// The text being written: spaces are held back until something follows them, so no line ends
/// with one the formatter put there.
struct Out {
    text: String,
    /// Indentation of the line being written.
    indent: usize,
    line_start: bool,
    space: bool,
}

impl Out {
    fn newline(&mut self, indent: usize) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.indent = indent;
        self.line_start = true;
        self.space = false;
    }

    fn space(&mut self) {
        self.space = !self.line_start;
    }

    fn write(&mut self, s: &str) {
        if self.line_start {
            self.text.extend(std::iter::repeat_n(' ', self.indent));
        } else if self.space {
            self.text.push(' ');
        }
        self.text.push_str(s);
        self.line_start = false;
        self.space = false;
    }

    /// A new line for what stands between statements, after a blank line when the source had
    /// one.
    fn between(&mut self, t: &Tok) {
        let started = !self.text.is_empty();
        self.newline(0);
        if started && t.newlines >= 2 {
            self.text.push('\n');
        }
    }
}

/// What a statement is to the formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// `select`, `with`, `insert`, `delete`, `values`, `create view`.
    Query,
    Update,
    CreateTable,
    /// Kept as written.
    Verbatim,
    /// Procedural code whose `;` end no statement: it and all that follows is kept as written.
    Rest,
}

fn lower(text: &str, t: &Tok) -> String {
    if t.kind == Kind::Word {
        text[t.start..t.end].to_ascii_lowercase()
    } else {
        String::new()
    }
}

fn classify(text: &str, tokens: &[Tok]) -> Class {
    let code: Vec<&Tok> = tokens.iter().filter(|t| !t.is_comment()).collect();
    // Template braces, psql commands, arrays over several lines: no layout for those.
    if code.iter().any(|t| t.kind == Kind::Other) {
        return Class::Verbatim;
    }
    let words: Vec<String> = code.iter().take(9).map(|t| lower(text, t)).collect();
    let dollar_body = code
        .iter()
        .any(|t| t.kind == Kind::Quoted && text[t.start..].starts_with('$'));
    let routine = if dollar_body {
        Class::Verbatim
    } else {
        Class::Rest
    };
    match words.first().map(String::as_str) {
        Some("select" | "with" | "insert" | "delete" | "values") => Class::Query,
        Some("update") => Class::Update,
        Some(first @ ("create" | "alter")) => {
            for w in &words[1..] {
                match w.as_str() {
                    "table" if first == "create" => return Class::CreateTable,
                    "view" if first == "create" => return Class::Query,
                    "function" | "procedure" | "proc" | "trigger" | "package" | "event" => {
                        return routine;
                    }
                    "table" | "view" | "index" | "sequence" | "schema" | "database" | "type"
                    | "extension" | "role" | "user" | "policy" | "domain" => {
                        return Class::Verbatim;
                    }
                    _ => {}
                }
            }
            Class::Verbatim
        }
        Some("begin") => match words.get(1).map(String::as_str) {
            None | Some("transaction" | "work" | "tran" | "isolation" | "deferred") => {
                Class::Verbatim
            }
            Some("immediate" | "exclusive" | "read") => Class::Verbatim,
            Some("") if code.get(1).is_some_and(|t| t.kind == Kind::Semi) => Class::Verbatim,
            _ => Class::Rest,
        },
        Some("do") => routine,
        Some("declare" | "delimiter" | "if" | "while" | "loop" | "for" | "repeat") => Class::Rest,
        _ => Class::Verbatim,
    }
}

/// Index just past the statement that starts at `from`: after its `;` outside parentheses, or
/// the end.
fn statement_end(text: &str, tokens: &[Tok], from: usize) -> Result<usize, String> {
    let mut open: Vec<usize> = Vec::new();
    for (i, t) in tokens.iter().enumerate().skip(from) {
        match t.kind {
            Kind::Open => open.push(t.start),
            Kind::Close if open.pop().is_none() => {
                return Err(format!(
                    "line {}: this `)` closes nothing",
                    line_of(text, t.start)
                ));
            }
            Kind::Semi if open.is_empty() => return Ok(i + 1),
            _ => {}
        }
    }
    match open.first() {
        Some(at) => Err(format!(
            "line {}: this `(` is never closed",
            line_of(text, *at)
        )),
        None => Ok(tokens.len()),
    }
}

fn layout(text: &str, tokens: &[Tok], indent: usize) -> Result<String, String> {
    let mut o = Out {
        text: String::new(),
        indent: 0,
        line_start: true,
        space: false,
    };
    let own_line = |i: usize| i == 0 || tokens[i].newlines > 0;
    // Where the line of a token that stands first on it begins.
    let line_start = |t: &Tok| text[..t.start].rfind('\n').map_or(0, |p| p + 1);
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.is_comment() {
            if own_line(i) {
                o.between(t);
            } else {
                o.space();
            }
            o.write(&text[t.start..t.end]);
            i += 1;
            continue;
        }
        // SQL Server's batch separator, alone on its line, ends no statement of its own.
        if lower(text, t) == "go"
            && own_line(i)
            && tokens.get(i + 1).is_none_or(|next| next.newlines > 0)
        {
            o.between(t);
            o.write(&text[t.start..t.end]);
            i += 1;
            continue;
        }
        let end = statement_end(text, tokens, i)?;
        o.between(t);
        let from = if own_line(i) { line_start(t) } else { t.start };
        match classify(text, &tokens[i..end]) {
            Class::Verbatim => o.write_raw(&text[from..tokens[end - 1].end]),
            Class::Rest => {
                o.write_raw(text[from..].trim_end());
                break;
            }
            class => statement(text, &tokens[i..end], class, indent, &mut o),
        }
        i = end;
    }
    o.text.push('\n');
    Ok(o.text)
}

impl Out {
    /// Text kept as written, its own indentation included.
    fn write_raw(&mut self, s: &str) {
        self.text.push_str(s);
        self.line_start = false;
        self.space = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    /// A statement or a subquery in parentheses: clauses start lines.
    Query,
    /// Parentheses around anything else.
    List,
    /// The column list of `create table`.
    Columns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Clause {
    None,
    Where,
    Having,
    Values,
    Other,
}

struct Frame {
    kind: FrameKind,
    /// Indentation of the frame's clauses or entries.
    base: usize,
    /// Indentation of the line its `)` goes to.
    close: usize,
    clause: Clause,
    /// A `between` waits for its `and`.
    between: bool,
    /// Open `case … end`.
    cases: usize,
    /// Nothing of the frame is written yet.
    first: bool,
    /// The next token starts an entry (column list).
    entry: bool,
    /// Width of the longest column name (column list).
    names: usize,
    /// What a join line is indented by beyond one level: the width of `from` and its space
    /// after a `from` that started a line of this frame, nothing otherwise.
    joins: usize,
}

impl Frame {
    fn new(kind: FrameKind, base: usize, close: usize) -> Self {
        Self {
            kind,
            base,
            close,
            clause: Clause::None,
            between: false,
            cases: 0,
            first: true,
            entry: kind == FrameKind::Columns,
            names: 0,
            joins: 0,
        }
    }
}

/// The columns `from` and the space after it take: a join starts one level right of the table
/// name that follows them.
const FROM_AND_SPACE: usize = "from ".len();

/// Words after which an operator is a sign, not an operation between two values.
const BEFORE_A_VALUE: [&str; 38] = [
    "select",
    "distinct",
    "all",
    "where",
    "and",
    "or",
    "not",
    "on",
    "when",
    "then",
    "else",
    "case",
    "by",
    "having",
    "set",
    "values",
    "in",
    "like",
    "ilike",
    "between",
    "is",
    "limit",
    "offset",
    "returning",
    "return",
    "as",
    "from",
    "join",
    "using",
    "union",
    "except",
    "intersect",
    "with",
    "over",
    "default",
    "check",
    "exists",
    "any",
];

const JOIN_WORDS: [&str; 7] = ["join", "inner", "left", "right", "full", "cross", "natural"];

/// First words of a `create table` entry that is no column.
const CONSTRAINT_WORDS: [&str; 11] = [
    "constraint",
    "primary",
    "foreign",
    "unique",
    "check",
    "key",
    "index",
    "fulltext",
    "spatial",
    "like",
    "exclude",
];

/// What stands between a token and the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sep {
    None,
    Space,
    /// A space where the source had whitespace.
    Written,
    Line(usize),
}

/// Whether a space may be put between the operators `a` and `b` without making other tokens of
/// them in any database: `a` is plain arithmetic or comparison and `b` a sign.
fn sign_after(a: &str, b: &str) -> bool {
    a.bytes().all(|c| b"=<>*/+-".contains(&c)) && matches!(b, "-" | "+")
}

fn statement(text: &str, tokens: &[Tok], class: Class, indent: usize, o: &mut Out) {
    let word = |k: usize| lower(text, &tokens[k]);
    let next_code = |k: usize| (k + 1..tokens.len()).find(|&j| !tokens[j].is_comment());
    let next_word = |k: usize| next_code(k).map(word).unwrap_or_default();
    let mut frames = vec![Frame::new(FrameKind::Query, 0, 0)];
    let mut prev: Option<usize> = None;
    // The operator before this token stands between two values.
    let mut prev_binary = false;
    // A comment took the rest of the line.
    let mut broken = false;
    // Spaces that align the type of a column.
    let mut pad = 0;
    // `create table`: its column list is still to come.
    let mut columns_ahead = class == Class::CreateTable;

    for (k, t) in tokens.iter().enumerate() {
        let s = &text[t.start..t.end];
        if t.is_comment() {
            let own = t.newlines > 0;
            if own {
                o.newline(o.indent);
            } else if t.kind == Kind::LineComment || t.space {
                o.space();
            }
            o.write(s);
            broken |= own || t.kind == Kind::LineComment;
            continue;
        }
        let w = word(k);
        let w = w.as_str();
        let p = prev.map(|j| &tokens[j]);
        let prev_word = prev.map(word).unwrap_or_default();
        let depth = frames.len();
        let Some(f) = frames.last_mut() else {
            return;
        };
        let mut line = None;
        let mut binary = false;

        // Clauses start lines.
        if f.kind == FrameKind::Query {
            let starts_join = JOIN_WORDS.contains(&w)
                && !JOIN_WORDS.contains(&prev_word.as_str())
                && prev_word != "outer"
                && (w == "join" || {
                    let mut j = next_code(k);
                    let mut found = false;
                    for _ in 0..3 {
                        match j.map(|j| (j, word(j))) {
                            Some((_, n)) if n == "join" => {
                                found = true;
                                break;
                            }
                            Some((at, n)) if n == "outer" || JOIN_WORDS.contains(&n.as_str()) => {
                                j = next_code(at);
                            }
                            _ => break,
                        }
                    }
                    found
                });
            let clause = match w {
                "select" => Some(Clause::Other),
                // The common table expressions of a statement that started otherwise.
                "with"
                    if !f.first
                        && (next_word(k) == "recursive"
                            || next_code(k).and_then(next_code).map(word).as_deref()
                                == Some("as")) =>
                {
                    Some(Clause::Other)
                }
                "from" if !matches!(prev_word.as_str(), "delete" | "distinct") => {
                    Some(Clause::Other)
                }
                "where" => Some(Clause::Where),
                "having" => Some(Clause::Having),
                "values"
                    if !p.is_some_and(|p| matches!(p.kind, Kind::Op | Kind::Comma | Kind::Dot)) =>
                {
                    Some(Clause::Values)
                }
                "limit" | "offset" | "returning" | "union" | "intersect" | "except" => {
                    Some(Clause::Other)
                }
                "set" if class == Class::Update && depth == 1 && f.clause == Clause::None => {
                    Some(Clause::Other)
                }
                "group" | "order" if next_word(k) == "by" => Some(Clause::Other),
                _ => None,
            };
            if let Some(clause) = clause {
                f.clause = clause;
                (f.between, f.cases) = (false, 0);
                f.joins = if w == "from" { FROM_AND_SPACE } else { 0 };
                line = Some(f.base);
            } else if starts_join {
                f.clause = Clause::Other;
                line = Some(f.base + f.joins + indent);
            } else if matches!(f.clause, Clause::Where | Clause::Having) && f.cases == 0 {
                let keyword = if f.clause == Clause::Where { 5 } else { 6 };
                match w {
                    "between" => f.between = true,
                    "and" if f.between => f.between = false,
                    "and" | "or" => line = Some(f.base + keyword - w.len()),
                    _ => {}
                }
            }
            match w {
                "case" => f.cases += 1,
                "end" => f.cases = f.cases.saturating_sub(1),
                _ => {}
            }
            if f.clause == Clause::Values && w == "on" {
                f.clause = Clause::Other;
            }
            // One row per line after `values`.
            if f.clause == Clause::Values && p.is_some_and(|p| p.kind == Kind::Comma) {
                line = Some(f.base + "values ".len());
            }
        }
        if f.first && depth > 1 && f.kind == FrameKind::Query {
            line = Some(f.base);
        }
        let column = f.kind == FrameKind::Columns && f.entry;
        if column {
            line = Some(f.base);
        }

        let mut sep = match t.kind {
            Kind::Comma | Kind::Semi => Sep::None,
            Kind::Close => match f.kind {
                FrameKind::List => Sep::None,
                _ => Sep::Line(f.close),
            },
            _ => match p.map(|p| p.kind) {
                None => Sep::None,
                Some(Kind::Open) => Sep::None,
                Some(Kind::Comma) => Sep::Space,
                Some(Kind::Op) if !prev_binary => Sep::Written,
                Some(Kind::Op) if t.kind != Kind::Op => Sep::Space,
                Some(Kind::Op) => {
                    let before = p.map_or("", |p| &text[p.start..p.end]);
                    if sign_after(before, s) {
                        Sep::Space
                    } else {
                        Sep::Written
                    }
                }
                Some(_) => Sep::Written,
            },
        };
        if t.kind == Kind::Op {
            binary = p.is_some_and(|p| match p.kind {
                Kind::Quoted | Kind::Close => true,
                Kind::Word => !BEFORE_A_VALUE.contains(&prev_word.as_str()),
                _ => false,
            });
            if s == "::" {
                // A cast stays as close to its value as it was written.
                (binary, sep) = (false, Sep::Written);
            } else if binary {
                sep = Sep::Space;
            }
        }
        if let Some(n) = line.filter(|_| t.kind != Kind::Close) {
            sep = Sep::Line(n);
        }
        if prev.is_none() {
            sep = Sep::None;
        } else if broken && !matches!(sep, Sep::Line(_)) {
            sep = Sep::Line(f.base + indent);
        }
        // The column list of `create table` opens on a line of its own.
        let opens_columns = t.kind == Kind::Open
            && columns_ahead
            && depth == 1
            && !matches!(next_word(k).as_str(), "select" | "with");
        // So does the first row after `values`, to stand above the others.
        let opens_row = t.kind == Kind::Open
            && f.kind == FrameKind::Query
            && f.clause == Clause::Values
            && prev_word == "values";
        if opens_columns {
            sep = Sep::Line(0);
        } else if opens_row {
            sep = Sep::Space;
        }
        if columns_ahead && matches!(w, "as" | "select" | "like") {
            columns_ahead = false;
        }

        match sep {
            Sep::None => {}
            Sep::Space => o.space(),
            Sep::Written if t.space => o.space(),
            Sep::Written => {}
            Sep::Line(n) => o.newline(n),
        }
        if pad > 0 && !o.line_start {
            o.text.extend(std::iter::repeat_n(' ', pad));
        }
        pad = 0;
        o.write(s);
        broken = false;
        f.first = false;
        f.entry = false;

        // The type of a column starts where the longest name ends.
        if column && is_column(text, tokens, k) && next_code(k) == Some(k + 1) {
            pad = f.names.saturating_sub(s.chars().count());
        }
        match t.kind {
            Kind::Comma if f.kind == FrameKind::Columns => f.entry = true,
            Kind::Open => {
                let frame = if opens_columns {
                    columns_ahead = false;
                    let mut frame = Frame::new(FrameKind::Columns, indent, 0);
                    frame.names = column_names(text, tokens, k);
                    frame
                } else if matches!(next_word(k).as_str(), "select" | "with") {
                    Frame::new(FrameKind::Query, o.indent + indent, o.indent)
                } else {
                    Frame::new(FrameKind::List, 0, 0)
                };
                frames.push(frame);
            }
            Kind::Close if frames.len() > 1 => {
                frames.pop();
            }
            _ => {}
        }
        prev = Some(k);
        prev_binary = binary;
    }
}

/// Whether the `create table` entry that starts at token `k` is a column: a name followed by a
/// type.
fn is_column(text: &str, tokens: &[Tok], k: usize) -> bool {
    let name = &tokens[k];
    let next = tokens[k + 1..].iter().find(|t| !t.is_comment());
    matches!(name.kind, Kind::Word | Kind::Quoted)
        && !CONSTRAINT_WORDS.contains(&lower(text, name).as_str())
        && next.is_some_and(|t| t.kind == Kind::Word)
}

/// Width of the longest column name in the column list opened at token `open`.
fn column_names(text: &str, tokens: &[Tok], open: usize) -> usize {
    let (mut depth, mut entry, mut width) = (0usize, true, 0);
    for k in open + 1..tokens.len() {
        let t = &tokens[k];
        if t.is_comment() {
            continue;
        }
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close if depth == 0 => break,
            Kind::Close => depth -= 1,
            Kind::Comma if depth == 0 => {
                entry = true;
                continue;
            }
            _ => {}
        }
        if entry && is_column(text, tokens, k) {
            width = width.max(text[t.start..t.end].chars().count());
        }
        entry = false;
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formatted(sql: &str) -> String {
        let out = format_sql(sql, 4).unwrap_or_else(|e| panic!("{e}\n{sql}"));
        assert_eq!(
            format_sql(&out, 4).as_deref(),
            Ok(out.as_str()),
            "idempotent"
        );
        assert_eq!(sql_tokens(sql), sql_tokens(&out), "tokens");
        out
    }

    /// SPEC-0043/AK-9: one clause per line, the join under `from`, `and` under `where`.
    #[test]
    fn a_query_gets_one_clause_per_line() {
        let ugly = "select c.name,count(*)   as n from customers c join orders o on o.customer_id=c.id and o.open left   outer join x on x.id=o.id where c.active=1 and o.total>=100 or c.vip group by c.name having count(*)>1 and max(o.total)<5 order by n desc,c.name limit 10;\n";
        assert_eq!(
            formatted(ugly),
            "select c.name, count(*) as n\n\
             from customers c\n         \
                 join orders o on o.customer_id = c.id and o.open\n         \
                 left outer join x on x.id = o.id\n\
             where c.active = 1\n  \
               and o.total >= 100\n   \
                or c.vip\n\
             group by c.name\n\
             having count(*) > 1\n   \
                and max(o.total) < 5\n\
             order by n desc, c.name\n\
             limit 10;\n"
        );
    }

    /// AK-9: keyword case stays, `between … and` and `case … end` stay on their line, signs
    /// stay signs.
    #[test]
    fn case_between_and_signs() {
        assert_eq!(
            formatted(
                "SELECT a , -b,x*-1 FROM t WHERE a BETWEEN 1 AND 2 AND CASE WHEN a>1 AND b<2 THEN 1 ELSE -1 END=1 AND d IS DISTINCT FROM e"
            ),
            "SELECT a, -b, x * -1\n\
             FROM t\n\
             WHERE a BETWEEN 1 AND 2\n  \
               AND CASE WHEN a > 1 AND b < 2 THEN 1 ELSE -1 END = 1\n  \
               AND d IS DISTINCT FROM e\n"
        );
        assert_eq!(
            formatted("select a=-1, b - -c;\n"),
            "select a = -1, b - -c;\n"
        );
        assert_eq!(
            formatted("select 1e-5+2.5,x::int,j->>'k' from t;\n"),
            "select 1e-5 + 2.5, x::int, j ->> 'k'\nfrom t;\n"
        );
    }

    /// AK-9: `insert … values` with one row per line, `update` and `delete`.
    #[test]
    fn insert_update_delete() {
        assert_eq!(
            formatted("insert into t(a,b) values (1,'x'),(2,'y'),\n(3,'z');"),
            "insert into t(a, b)\nvalues (1, 'x'),\n       (2, 'y'),\n       (3, 'z');\n"
        );
        assert_eq!(
            formatted("INSERT INTO t VALUES(1),(2) ON CONFLICT (a,b) DO NOTHING;"),
            "INSERT INTO t\nVALUES (1),\n       (2) ON CONFLICT (a, b) DO NOTHING;\n"
        );
        assert_eq!(
            formatted("insert into t (a) with s as (select 1) select * from s;"),
            "insert into t (a)\nwith s as (\n    select 1\n)\nselect *\nfrom s;\n"
        );
        assert_eq!(
            formatted("update t set a=1,b=b+1 where id=3 and x is null;"),
            "update t\nset a = 1, b = b + 1\nwhere id = 3\n  and x is null;\n"
        );
        assert_eq!(
            formatted("delete from t where id in (1,2);"),
            "delete from t\nwhere id in (1, 2);\n"
        );
    }

    /// AK-9: `create table` with one column per line and the types in one column.
    #[test]
    fn create_table_aligns_the_types() {
        assert_eq!(
            formatted(
                "create table if not exists orders(id integer primary key,customer_id bigint not null references customers(id) on delete set null,total numeric(10,2) default 0,constraint positive check(total>=0 and id>0),primary key(id,customer_id));"
            ),
            "create table if not exists orders\n(\n    \
                 id          integer primary key,\n    \
                 customer_id bigint not null references customers(id) on delete set null,\n    \
                 total       numeric(10, 2) default 0,\n    \
                 constraint positive check(total >= 0 and id > 0),\n    \
                 primary key(id, customer_id)\n\
             );\n"
        );
        assert_eq!(
            formatted("create table a as select 1 as x;"),
            "create table a as\nselect 1 as x;\n"
        );
        // The parenthesis keeps its own line after a comment, and what follows `)` stays there.
        assert_eq!(
            formatted("CREATE TABLE t -- why\n(a int,\"b c\" text) engine=InnoDB;\n"),
            "CREATE TABLE t -- why\n(\n    a     int,\n    \"b c\" text\n) engine = InnoDB;\n"
        );
    }

    /// AK-9 (v6, OBS-0036 FM5b): a join starts one level right of the table name of its `from`
    /// line, whatever the case of `from` and at every depth; without a `from` line one level.
    #[test]
    fn joins_stand_under_the_table_name() {
        assert_eq!(
            formatted(
                "SELECT * FROM a INNER JOIN b ON b.id=a.id natural join c cross join d full outer join e using(id) WHERE a.x=1;"
            ),
            "SELECT *\n\
             FROM a\n         \
                 INNER JOIN b ON b.id = a.id\n         \
                 natural join c\n         \
                 cross join d\n         \
                 full outer join e using(id)\n\
             WHERE a.x = 1;\n"
        );
        // Nine columns right of the `from` of the subquery and of the common table expression.
        assert_eq!(
            formatted(
                "with w as (select 1 from p right join q on q.id=p.id) select * from t join (select id from u left join v on v.id=u.id where u.x=1) s on s.id=t.id join w on w.id=t.id;"
            ),
            "with w as (\n    \
                 select 1\n    \
                 from p\n             \
                 right join q on q.id = p.id\n\
             )\n\
             select *\n\
             from t\n         \
                 join (\n             \
                 select id\n             \
                 from u\n                      \
                 left join v on v.id = u.id\n             \
                 where u.x = 1\n         \
                 ) s on s.id = t.id\n         \
                 join w on w.id = t.id;\n"
        );
        // The second query of a union counts from its own `from`.
        assert_eq!(
            formatted("select 1 from a join b on 1=1 union select 2 from c join d on 1=1;"),
            "select 1\nfrom a\n         join b on 1 = 1\nunion\nselect 2\nfrom c\n         join d on 1 = 1;\n"
        );
        // No `from` line before the join: one level.
        assert_eq!(
            formatted("delete from t join u on u.id=t.id where u.b>1;"),
            "delete from t\n    join u on u.id = t.id\nwhere u.b > 1;\n"
        );
        assert_eq!(
            format_sql("select 1 from t join u on a=b;", 2).unwrap(),
            "select 1\nfrom t\n       join u on a = b;\n"
        );
    }

    /// AK-9 (v6): the `query.sql` of OBS-0036 as the observation describes its result.
    #[test]
    fn the_observed_file() {
        assert_eq!(
            formatted(
                "select s.id,s.name,count(*) as n from shapes s join tags t on t.shape_id=s.id where s.w>0 and s.h>0 group by s.id,s.name having count(*)>1 order by n desc;\ninsert into shapes(id,name,w,h) values(1,'a',2,3),(2,'b',4,5);\ncreate table shapes(id int primary key,name varchar(20) not null,w int,h int);\n"
            ),
            "select s.id, s.name, count(*) as n\n\
             from shapes s\n         \
                 join tags t on t.shape_id = s.id\n\
             where s.w > 0\n  \
               and s.h > 0\n\
             group by s.id, s.name\n\
             having count(*) > 1\n\
             order by n desc;\n\
             insert into shapes(id, name, w, h)\n\
             values (1, 'a', 2, 3),\n       \
                    (2, 'b', 4, 5);\n\
             create table shapes\n\
             (\n    \
                 id   int primary key,\n    \
                 name varchar(20) not null,\n    \
                 w    int,\n    \
                 h    int\n\
             );\n"
        );
    }

    /// AK-9: a subquery is laid out one level deeper, a common table expression too.
    #[test]
    fn subqueries() {
        assert_eq!(
            formatted(
                "with big as (select id from t where n>1) select * from big where id in (select id from u where u.x=1) and y=2;"
            ),
            "with big as (\n    \
                 select id\n    \
                 from t\n    \
                 where n > 1\n\
             )\n\
             select *\n\
             from big\n\
             where id in (\n    \
                 select id\n    \
                 from u\n    \
                 where u.x = 1\n\
             )\n  \
               and y = 2;\n"
        );
        assert_eq!(
            formatted("select 1 union all select 2;"),
            "select 1\nunion all\nselect 2;\n"
        );
    }

    /// AK-10: comments and string contents stay as they are; a comment at the end of a line
    /// stays there.
    #[test]
    fn comments_and_strings_stay() {
        let sql = "-- head  \n\n/* block\n   two */\nselect 'a  ,b'  ,\"my  col\" -- why\n ,x /* in */ from t  -- tail\nwhere a='it''s' # mysql\n;\n\n\n-- end";
        let out = formatted(sql);
        assert_eq!(
            out,
            "-- head  \n\n/* block\n   two */\nselect 'a  ,b', \"my  col\" -- why\n    , x /* in */\nfrom t -- tail\nwhere a = 'it''s' # mysql\n    ;\n\n-- end\n"
        );
        for kept in [
            "-- head  \n",
            "/* block\n   two */",
            "'a  ,b'",
            "\"my  col\"",
            "'it''s'",
        ] {
            assert!(out.contains(kept), "{kept}");
        }
    }

    /// AK-10: statements the formatter has no layout for stay as written, with their blank
    /// lines; procedural code keeps everything after it.
    #[test]
    fn other_statements_stay() {
        let sql = "drop   table if exists a;\n\n  create index i\n    on a (x);\nselect 1;\n";
        assert_eq!(
            formatted(sql),
            "drop   table if exists a;\n\n  create index i\n    on a (x);\nselect 1;\n"
        );
        let function = "select  1;\ncreate function f() returns int as $$\nbegin  return 1; end\n$$ language plpgsql;\nselect  2;\n";
        assert_eq!(
            formatted(function),
            "select 1;\ncreate function f() returns int as $$\nbegin  return 1; end\n$$ language plpgsql;\nselect 2;\n"
        );
        let procedure = "select  1;\ncreate procedure p()\nbegin\n  select  2;\n  select  3;\nend;\nselect  4;\n";
        assert_eq!(
            formatted(procedure),
            "select 1;\ncreate procedure p()\nbegin\n  select  2;\n  select  3;\nend;\nselect  4;\n"
        );
        let batches = "select  1;\nGO\nselect  2;\ngo\n";
        assert_eq!(formatted(batches), "select 1;\nGO\nselect 2;\ngo\n");
        let transaction = "begin;\nselect  1;\ncommit;\n";
        assert_eq!(formatted(transaction), "begin;\nselect 1;\ncommit;\n");
        // Template text is not SQL the formatter knows.
        let template = "select  * from {{ ref('a') }} where x=1;\n";
        assert_eq!(formatted(template), template);
    }

    /// AK-10: what cannot be read without doubt is refused with the line.
    #[test]
    fn unreadable_text_is_refused() {
        for (sql, why) in [
            ("select 'abc from t;\n", "line 1: the quoted text"),
            ("select 1;\n/* open\nselect 2;\n", "line 2: the comment"),
            ("select $a$ x from t;\n", "line 1: the dollar-quoted text"),
            ("select (1;\n", "line 1: this `(` is never closed"),
            ("select 1);\n", "line 1: this `)` closes nothing"),
            (
                "select 1;\nselect 'it\\'s' , 'b';\n",
                "line 2: a backslash before a quote",
            ),
            (
                "/* a /* b */ c */ select 1;\n",
                "line 1: a comment opens inside",
            ),
        ] {
            let e = format_sql(sql, 4).unwrap_err();
            assert!(e.starts_with(why), "{sql}: {e}");
        }
        // A backslash that every database reads alike is no problem.
        assert_eq!(formatted("select 'a\\\\' ,1;\n"), "select 'a\\\\', 1;\n");
    }

    /// AK-10: line breaks of a Windows file stay, an empty file stays empty, the indentation
    /// follows the option.
    #[test]
    fn line_breaks_and_indent() {
        assert_eq!(
            format_sql("select 1 from t\r\njoin u on a=b;\r\n", 2).unwrap(),
            "select 1\r\nfrom t\r\n       join u on a = b;\r\n"
        );
        assert_eq!(format_sql("", 4).unwrap(), "");
        assert_eq!(format_sql("  \n", 4).unwrap(), "  \n");
    }

    /// AK-10: every SQL file of the reference projects keeps its tokens and is stable after one
    /// run.
    #[test]
    fn the_repository_s_sql_files_keep_their_tokens() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
        let mut files = Vec::new();
        let mut folders = vec![root];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "sql") {
                    files.push(path);
                }
            }
        }
        assert!(files.len() >= 10, "{files:?}");
        let mut changed = 0;
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let out = match format_sql(&text, 4) {
                Ok(out) => out,
                Err(e) => panic!("{}: {e}", file.display()),
            };
            assert_eq!(
                sql_tokens(&text),
                sql_tokens(&out),
                "tokens of {}",
                file.display()
            );
            assert_eq!(
                format_sql(&out, 4).as_deref(),
                Ok(out.as_str()),
                "second run of {}",
                file.display()
            );
            changed += usize::from(out != text);
        }
        assert!(changed > 0);
    }
}
