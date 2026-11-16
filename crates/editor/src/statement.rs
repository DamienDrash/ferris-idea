//! Complete Current Statement (`EditorCompleteStatement`, `SPEC-0056` v5 behaviour 20) and
//! Unwrap/Remove (`Unwrap`, behaviour 21).
//!
//! Both read lines, brackets and keywords, not a syntax tree, like the other helpers of
//! [`crate::smart`]: string literals and comments are skipped, so their brackets do not count.
//! The functions take the document's lines without their line breaks and answer what replaces
//! which lines; [`crate::commands`] puts that into the buffer as one undo step.

use std::ops::Range;

/// Languages whose statements end with `;` (behaviour 20).
const SEMICOLON_LANGUAGES: &[&str] = &[
    "java",
    "javascript",
    "typescript",
    "javascriptreact",
    "typescriptreact",
    "c",
    "cpp",
    "csharp",
    "php",
    "rust",
];

/// Keywords that open the block of a control statement in a language with braces.
const BLOCK_KEYWORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "try",
    "catch",
    "finally",
    "synchronized",
    "when",
    "match",
    "loop",
];

/// Python's keywords whose line ends with `:`.
const PYTHON_BLOCK_KEYWORDS: &[&str] = &[
    "if", "elif", "else", "for", "while", "def", "class", "try", "except", "finally", "with",
    "match", "case",
];

/// Keywords of the blocks Unwrap offers (behaviour 21).
const UNWRAP_KEYWORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "try",
    "synchronized",
    "unsafe",
    "loop",
];

/// What Complete Current Statement puts in place of one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The 0-based line that is replaced.
    pub line: usize,
    /// The lines that replace it.
    pub lines: Vec<String>,
    /// Where the caret goes: the index into [`Completion::lines`] and the column in characters.
    pub caret: (usize, usize),
}

/// A block Unwrap can take away (behaviour 21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unwrapping {
    /// What the list shows: `Unwrap 'if…'` or `Unwrap block`.
    pub label: String,
    /// The 0-based line that opens the block.
    pub open: usize,
    /// The 0-based line that closes it.
    pub close: usize,
}

/// What a scan of text outside string literals and comments found.
#[derive(Debug, Default)]
struct Scan {
    /// Brackets of the line in order, with their columns (characters).
    brackets: Vec<(char, usize)>,
    /// The quote of a string literal still open at the end, if any.
    open_quote: Option<char>,
    /// Column where a line comment starts, if any.
    comment: Option<usize>,
}

/// Scans `line` for brackets outside string literals and comments; `in_block` says whether a
/// block comment is open at its start and is updated to its end.
fn scan(line: &str, in_block: &mut bool, python: bool) -> Scan {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Scan::default();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if *in_block {
            if ch == '*' && chars.get(i + 1) == Some(&'/') {
                *in_block = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if (!python && ch == '/' && chars.get(i + 1) == Some(&'/')) || (python && ch == '#') {
            out.comment = Some(i);
            break;
        }
        if !python && ch == '/' && chars.get(i + 1) == Some(&'*') {
            *in_block = true;
            i += 2;
            continue;
        }
        let literal = match ch {
            '"' | '`' => true,
            // A character literal (`'a'`, `'\n'`), not a Rust lifetime (`'a`); Python's strings.
            '\'' => python || chars.get(i + 2) == Some(&'\'') || chars.get(i + 1) == Some(&'\\'),
            _ => false,
        };
        if literal {
            let mut j = i + 1;
            let mut closed = false;
            while j < chars.len() {
                if chars[j] == '\\' {
                    j += 2;
                    continue;
                }
                if chars[j] == ch {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                out.open_quote = Some(ch);
                return out;
            }
            i = j + 1;
            continue;
        }
        if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}') {
            out.brackets.push((ch, i));
        }
        i += 1;
    }
    out
}

fn indent_of(line: &str) -> &str {
    let n: usize = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(char::len_utf8)
        .sum();
    &line[..n]
}

/// The first word of `text` (letters, digits, `_`).
fn first_word(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    &text[..end]
}

/// Whether the block opened on line `open` (whose last bracket is `{`) is closed further down.
fn block_closed(lines: &[String], open: usize, python: bool) -> bool {
    let mut depth = 1i64;
    let mut in_block = false;
    for line in &lines[open + 1..] {
        for (b, _) in scan(line, &mut in_block, python).brackets {
            match b {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                return true;
            }
        }
    }
    false
}

/// Complete Current Statement on line `line` of `lines` (behaviour 20): `language` is the
/// editor's language id, `unit` one level of indentation. `None` when `line` is past the end.
pub fn complete_statement(
    lines: &[String],
    line: usize,
    language: Option<&str>,
    unit: &str,
) -> Option<Completion> {
    let text = lines.get(line)?;
    let python = language == Some("python");
    let braces = crate::smart::has_braces(language);
    let indent = indent_of(text).to_owned();
    // The block comment state at the start of the line.
    let mut in_block = false;
    for l in &lines[..line] {
        scan(l, &mut in_block, python);
    }
    let was_in_comment = in_block;
    let found = scan(text, &mut in_block, python);
    // The code part and the trailing comment, which keeps its place after the closers.
    let chars: Vec<char> = text.chars().collect();
    let cut = found.comment.unwrap_or(chars.len());
    let code: String = chars[..cut].iter().collect();
    let comment: String = chars[cut..].iter().collect();
    let mut code = code.trim_end().to_owned();
    let gap = if comment.is_empty() {
        String::new()
    } else {
        chars[..cut]
            .iter()
            .rev()
            .take_while(|c| c.is_whitespace())
            .collect::<String>()
    };
    if let Some(q) = found.open_quote {
        code.push(q);
    }
    let mut open: Vec<char> = Vec::new();
    for (b, _) in &found.brackets {
        match b {
            '(' | '[' => open.push(*b),
            ')' | ']' => {
                open.pop();
            }
            _ => {}
        }
    }
    for b in open.iter().rev() {
        code.push(if *b == '(' { ')' } else { ']' });
    }
    let trimmed = code.trim_start();
    let with_comment = |code: &str| {
        if comment.is_empty() {
            code.to_owned()
        } else {
            format!("{code}{gap}{comment}")
        }
    };
    let new_line_below = |first: String, deeper: bool| {
        let inner = if deeper {
            format!("{indent}{unit}")
        } else {
            indent.clone()
        };
        let column = inner.chars().count();
        Completion {
            line,
            lines: vec![first, inner],
            caret: (1, column),
        }
    };
    if trimmed.is_empty() || was_in_comment {
        return Some(new_line_below(with_comment(&code), false));
    }
    let word = first_word(trimmed.trim_start_matches('}').trim_start());
    if python {
        let word = match word {
            "async" => first_word(trimmed["async".len()..].trim_start()),
            w => w,
        };
        if PYTHON_BLOCK_KEYWORDS.contains(&word) {
            if !code.ends_with(':') {
                code.push(':');
            }
            return Some(new_line_below(with_comment(&code), true));
        }
        return Some(new_line_below(with_comment(&code), false));
    }
    if braces {
        let block_line = BLOCK_KEYWORDS.contains(&word);
        if code.ends_with('{') {
            let inner = format!("{indent}{unit}");
            let column = inner.chars().count();
            let mut out = vec![with_comment(&code), inner];
            if !block_closed(lines, line, false) {
                out.push(format!("{indent}}}"));
            }
            return Some(Completion {
                line,
                lines: out,
                caret: (1, column),
            });
        }
        if block_line {
            let inner = format!("{indent}{unit}");
            let column = inner.chars().count();
            return Some(Completion {
                line,
                lines: vec![
                    with_comment(&format!("{code} {{")),
                    inner,
                    format!("{indent}}}"),
                ],
                caret: (1, column),
            });
        }
    }
    let statement_language = language.is_some_and(|l| SEMICOLON_LANGUAGES.contains(&l));
    let no_statement = trimmed.starts_with('@')
        || trimmed.starts_with('#')
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*");
    if statement_language && !no_statement && !code.ends_with([';', '{', '}', ',', ':']) {
        code.push(';');
    }
    Some(new_line_below(with_comment(&code), false))
}

/// The blocks around 0-based `line` and `column` that Unwrap can take away, innermost first
/// (behaviour 21). Empty outside a language with braces.
pub fn unwrap_choices(
    lines: &[String],
    line: usize,
    column: usize,
    language: Option<&str>,
) -> Vec<Unwrapping> {
    if !crate::smart::has_braces(language) || language == Some("json") {
        return Vec::new();
    }
    // Every pair of braces: (open line, open column, close line, close column).
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut pairs: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut in_block = false;
    for (l, text) in lines.iter().enumerate() {
        for (b, c) in scan(text, &mut in_block, false).brackets {
            match b {
                '{' => stack.push((l, c)),
                '}' => {
                    if let Some((ol, oc)) = stack.pop() {
                        pairs.push((ol, oc, l, c));
                    }
                }
                _ => {}
            }
        }
    }
    let at = (line, column);
    let mut around: Vec<_> = pairs
        .into_iter()
        .filter(|&(ol, oc, cl, cc)| (ol, oc) < at && at <= (cl, cc))
        .collect();
    // Innermost first: the latest opening.
    around.sort_by_key(|a| std::cmp::Reverse((a.0, a.1)));
    let mut out = Vec::new();
    for (ol, oc, cl, cc) in around {
        let open = &lines[ol];
        let close = &lines[cl];
        if ol == cl {
            continue;
        }
        // The `{` ends the opening line.
        if open.chars().count() != oc + 1 && open.chars().skip(oc + 1).any(|c| !c.is_whitespace()) {
            continue;
        }
        // The `}` starts the closing line, and nothing but `;` (or `while (…);` of a `do`)
        // follows it.
        if close.chars().take(cc).any(|c| !c.is_whitespace()) {
            continue;
        }
        let head: String = open.chars().take(oc).collect();
        let head = head.trim();
        let word = first_word(head);
        let after: String = close.chars().skip(cc + 1).collect();
        let after = after.trim();
        let label = if head.is_empty() {
            "Unwrap block".to_owned()
        } else if UNWRAP_KEYWORDS.contains(&word) {
            format!("Unwrap '{word}…'")
        } else {
            continue;
        };
        let tail_ok = match word {
            "do" => first_word(after) == "while" || after.is_empty(),
            _ => after.is_empty() || after == ";",
        };
        if !tail_ok {
            continue;
        }
        out.push(Unwrapping {
            label,
            open: ol,
            close: cl,
        });
    }
    out
}

/// The lines that replace lines `u.open..=u.close` after unwrapping `u`: the body, moved left by
/// the difference between its indentation and the opening line's.
pub fn unwrapped(lines: &[String], u: &Unwrapping) -> (Range<usize>, Vec<String>) {
    let base = indent_of(&lines[u.open]).chars().count();
    let body = &lines[u.open + 1..u.close];
    let least = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l).chars().count())
        .min()
        .unwrap_or(base);
    let shift = least.saturating_sub(base);
    let out = body
        .iter()
        .map(|l| {
            let skip = indent_of(l).chars().count().min(shift);
            l.chars().skip(skip).collect()
        })
        .collect();
    (u.open..u.close + 1, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    fn complete(text: &str, line: usize, language: &str) -> (String, (usize, usize)) {
        let ls = lines(text);
        let c = complete_statement(&ls, line, Some(language), "    ").unwrap();
        (c.lines.join("\n"), c.caret)
    }

    /// SPEC-0056 AK-15 (v5): calls and brackets are closed, `;` added, the caret below.
    #[test]
    fn statements_get_their_closers_and_semicolon() {
        assert_eq!(
            complete("    System.out.println(\"x\"", 0, "java"),
            ("    System.out.println(\"x\");\n    ".to_owned(), (1, 4))
        );
        assert_eq!(
            complete("let v = a(b[1], c(2", 0, "rust"),
            ("let v = a(b[1], c(2));\n".to_owned(), (1, 0))
        );
        // Brackets inside strings and after a line comment do not count; the comment stays last.
        assert_eq!(
            complete("f(\"(\", ')' // (", 0, "c").0,
            "f(\"(\", ')'); // (\n"
        );
        // An open string literal is closed first.
        assert_eq!(complete("say(\"hi", 0, "javascript").0, "say(\"hi\");\n");
        // A complete line gets nothing but the new line below.
        assert_eq!(complete("  x = 1;", 0, "java").0, "  x = 1;\n  ");
        // An annotation, a blank line and plain text get no semicolon.
        assert_eq!(complete("@Override", 0, "java").0, "@Override\n");
        assert_eq!(complete("", 0, "java"), ("\n".to_owned(), (1, 0)));
        assert_eq!(complete("hello(", 0, "plaintext").0, "hello()\n");
        // Kotlin has no semicolons.
        assert_eq!(complete("println(1", 0, "kotlin").0, "println(1)\n");
    }

    /// SPEC-0056 AK-15 (v5): a control line gets its block, the caret inside it.
    #[test]
    fn control_lines_get_a_block() {
        assert_eq!(
            complete("    if (a > b", 0, "java"),
            ("    if (a > b) {\n        \n    }".to_owned(), (1, 8))
        );
        assert_eq!(
            complete("while ready", 0, "rust").0,
            "while ready {\n    \n}"
        );
        // A line ending in `{` gets the inner line, and its `}` only when the block is open.
        assert_eq!(
            complete("for (x in xs) {", 0, "kotlin").0,
            "for (x in xs) {\n    \n}"
        );
        assert_eq!(complete("fn a() {\n}\n", 0, "rust").0, "fn a() {\n    ");
    }

    /// SPEC-0056 AK-15 (v5): Python's block lines end with `:`.
    #[test]
    fn python_block_lines_get_a_colon() {
        assert_eq!(
            complete("    def run(self", 0, "python"),
            ("    def run(self):\n        ".to_owned(), (1, 8))
        );
        assert_eq!(
            complete("async with lock", 0, "python").0,
            "async with lock:\n    "
        );
        assert_eq!(complete("x = f(1", 0, "python").0, "x = f(1)\n");
    }

    const JAVA: &str = "class A {
    void m() {
        if (ok) {
            run(\"}\");
            // }
            stop();
        }
        do {
            n++;
        } while (n < 3);
        {
            x();
        }
        if (a) {
            b();
        } else {
            c();
        }
    }
}";

    fn labels(line: usize, column: usize) -> Vec<String> {
        unwrap_choices(&lines(JAVA), line, column, Some("java"))
            .into_iter()
            .map(|u| u.label)
            .collect()
    }

    /// SPEC-0056 AK-16 (v5): the blocks around the caret, innermost first; braces in strings and
    /// comments do not count; a method body and a class are not offered.
    #[test]
    fn blocks_around_the_caret_are_offered() {
        assert_eq!(labels(3, 14), ["Unwrap 'if…'"]);
        assert_eq!(labels(8, 12), ["Unwrap 'do…'"]);
        assert_eq!(labels(11, 12), ["Unwrap block"]);
        // `} else {` is neither offered for the `if` nor for the `else`.
        assert!(labels(14, 12).is_empty());
        assert!(labels(16, 12).is_empty());
        assert!(unwrap_choices(&lines(JAVA), 3, 14, Some("python")).is_empty());
    }

    /// SPEC-0056 AK-16 (v5): the body moves left, the two lines go.
    #[test]
    fn unwrapping_keeps_the_body() {
        let ls = lines(JAVA);
        let u = &unwrap_choices(&ls, 3, 14, Some("java"))[0];
        let (range, body) = unwrapped(&ls, u);
        assert_eq!(range, 2..7);
        assert_eq!(
            body,
            ["        run(\"}\");", "        // }", "        stop();"]
        );
        let nested = lines("for (;;) {\n\tif (x) {\n\t\ty();\n\t}\n}");
        let all = unwrap_choices(&nested, 2, 2, Some("c"));
        assert_eq!(
            all.iter().map(|u| u.label.as_str()).collect::<Vec<_>>(),
            ["Unwrap 'if…'", "Unwrap 'for…'"]
        );
        assert_eq!(unwrapped(&nested, &all[1]).1, ["if (x) {", "\ty();", "}"]);
        let d = &unwrap_choices(&ls, 8, 12, Some("java"))[0];
        assert_eq!(unwrapped(&ls, d), (7..10, vec!["        n++;".to_owned()]));
    }
}
