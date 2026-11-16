//! Links in terminal output (`SPEC-0153`): web addresses, file paths with an optional line and
//! column, and hyperlinks a program marks with OSC 8.

use serde::Serialize;

use crate::screen::{Cell, Screen};

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkTarget {
    /// A web address (`http`, `https`, `ftp`, `file`, `mailto` with OSC 8).
    Url {
        /// The address.
        url: String,
    },
    /// A file, maybe with a 1-based line and column. The path is as written; whether it exists
    /// is up to the caller.
    File {
        /// Path as written (relative to the terminal's folder, absolute, or `~/…`).
        path: String,
        /// 1-based line.
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<usize>,
        /// 1-based column.
        #[serde(skip_serializing_if = "Option::is_none")]
        column: Option<usize>,
    },
}

/// A link on one line: the columns it covers (end exclusive) and its target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    /// First column.
    pub start: usize,
    /// Column after the last one.
    pub end: usize,
    /// Where it leads.
    pub target: LinkTarget,
}

const URL_SCHEMES: [&str; 4] = ["https://", "http://", "ftp://", "file://"];

/// Characters that end a link when they come last (punctuation of the sentence around it).
fn trailing(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '>' | '`'
    )
}

/// Removes trailing punctuation and closing brackets without a partner inside.
fn trim_end(word: &str) -> &str {
    let mut w = word;
    loop {
        let Some(last) = w.chars().last() else {
            return w;
        };
        let unbalanced = |open: char, close: char| {
            last == close && w.matches(open).count() < w.matches(close).count()
        };
        if trailing(last) || unbalanced('(', ')') || unbalanced('[', ']') || unbalanced('{', '}') {
            w = &w[..w.len() - last.len_utf8()];
        } else {
            return w;
        }
    }
}

fn path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '+' | '@' | '$')
}

/// `path`, `path:line` or `path:line:column` (also `path(line,column)` and `path(line)`), when it
/// looks like a file: a folder separator or a file extension, and a letter.
fn file_link(word: &str) -> Option<(usize, LinkTarget)> {
    let end = word.find(|c: char| !path_char(c)).unwrap_or(word.len());
    let path = word[..end].trim_end_matches('.');
    if path.is_empty() || path.starts_with("//") || !path.chars().any(char::is_alphabetic) {
        return None;
    }
    let has_extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 12
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.starts_with(|c: char| c.is_ascii_alphabetic())
        });
    let is_path = path.contains('/') && path.len() > 1;
    if !(has_extension || is_path) {
        return None;
    }
    let rest = &word[path.len()..];
    let number = |s: &str| -> (Option<usize>, usize) {
        let digits = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        (s[..digits].parse().ok().filter(|n| *n > 0), digits)
    };
    let (mut line, mut column, mut used) = (None, None, 0);
    if let Some(after) = rest.strip_prefix(':') {
        let (l, n) = number(after);
        if l.is_some() {
            line = l;
            used = 1 + n;
            if let Some(after) = after[n..].strip_prefix(':') {
                let (c, m) = number(after);
                if c.is_some() {
                    column = c;
                    used += 1 + m;
                }
            }
        }
    } else if let Some(after) = rest.strip_prefix('(') {
        // `file.cs(12,5)` as compilers of the .NET world write it.
        let (l, n) = number(after);
        if l.is_some() {
            let mut k = n;
            let mut c = None;
            if let Some(more) = after[n..].strip_prefix(',') {
                let (cc, m) = number(more);
                c = cc;
                k += 1 + m;
            }
            if after[k..].starts_with(')') {
                line = l;
                column = c;
                used = 1 + k + 1;
            }
        }
    }
    Some((
        path.len() + used,
        LinkTarget::File {
            path: path.to_owned(),
            line,
            column,
        },
    ))
}

/// Links in a line of text; `start`/`end` count characters (`SPEC-0153` behaviour 1).
pub fn find_links(text: &str) -> Vec<Link> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let char_at = |byte: usize| chars.partition_point(|(b, _)| *b < byte);
    let mut out = Vec::new();
    let separators = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`');
    let mut byte = 0;
    while byte < text.len() {
        let rest = &text[byte..];
        let Some(skip) = rest.find(|c: char| !separators(c)) else {
            break;
        };
        let start = byte + skip;
        let len = text[start..].find(separators).unwrap_or(text.len() - start);
        let word = &text[start..start + len];
        byte = start + len;
        // A word may carry a link after an opening bracket or `=` (`(see https://…)`, `--out=a/b`).
        let lead = word
            .find(|c: char| !matches!(c, '(' | '[' | '{' | '='))
            .unwrap_or(word.len());
        let mut inner = &word[lead..];
        if let Some(eq) = inner.find('=').filter(|_| !inner.contains("://")) {
            inner = &inner[eq + 1..];
        }
        let inner_start = start + word.len() - inner.len();
        if let Some(scheme) = URL_SCHEMES
            .iter()
            .filter_map(|s| inner.find(s).map(|at| (at, s)))
            .min_by_key(|(at, _)| *at)
        {
            let (at, s) = scheme;
            let url = trim_end(&inner[at..]);
            if url.len() > s.len() {
                let from = inner_start + at;
                out.push(Link {
                    start: char_at(from),
                    end: char_at(from + url.len()),
                    target: LinkTarget::Url {
                        url: url.to_owned(),
                    },
                });
            }
            continue;
        }
        let trimmed = trim_end(inner);
        if let Some((used, target)) = file_link(trimmed) {
            out.push(Link {
                start: char_at(inner_start),
                end: char_at(inner_start + used),
                target,
            });
        }
    }
    python_traceback(text, &char_at, &mut out);
    out
}

/// `File "path", line N` of Python tracebacks: the line number joins the quoted path's link.
fn python_traceback(text: &str, char_at: &dyn Fn(usize) -> usize, out: &mut Vec<Link>) {
    let Some(at) = text.find("File \"") else {
        return;
    };
    let from = at + 6;
    let Some(len) = text[from..].find('"') else {
        return;
    };
    let path = &text[from..from + len];
    let after = &text[from + len + 1..];
    let line = after
        .strip_prefix(", line ")
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok());
    let (start, end) = (char_at(from), char_at(from + len));
    out.retain(|l| l.end <= start || l.start >= end);
    out.push(Link {
        start,
        end,
        target: LinkTarget::File {
            path: path.to_owned(),
            line,
            column: None,
        },
    });
    out.sort_by_key(|l| l.start);
}

/// Links of one screen line: hyperlinks a program marked with OSC 8 first, then the links found
/// in the rest of the text. Columns are screen columns (`SPEC-0153` behaviours 1–2).
pub fn line_links(screen: &Screen, cells: &[Cell]) -> Vec<Link> {
    let mut out = Vec::new();
    let mut c = 0;
    while c < cells.len() {
        let id = cells[c].style.link;
        if id == 0 {
            c += 1;
            continue;
        }
        let start = c;
        while c < cells.len() && cells[c].style.link == id {
            c += 1;
        }
        if let Some(uri) = screen.link_uri(id) {
            let target = match uri.strip_prefix("file://") {
                Some(rest) => {
                    // `file://host/path` → the path.
                    let path = rest.find('/').map_or(rest, |i| &rest[i..]);
                    LinkTarget::File {
                        path: path.to_owned(),
                        line: None,
                        column: None,
                    }
                }
                None => LinkTarget::Url {
                    url: uri.to_owned(),
                },
            };
            out.push(Link {
                start,
                end: c,
                target,
            });
        }
    }
    // Text of the line with each character's column.
    let mut text = String::new();
    let mut columns = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        text.push(cell.ch);
        columns.push(col);
        for m in cell.combining.chars() {
            text.push(m);
            columns.push(col);
        }
    }
    let column_of = |i: usize| match columns.get(i) {
        Some(c) => *c,
        None => cells.len(),
    };
    for link in find_links(&text) {
        let (start, end) = (column_of(link.start), column_of(link.end));
        if out.iter().any(|l| l.start < end && start < l.end) {
            continue;
        }
        out.push(Link {
            start,
            end,
            target: link.target,
        });
    }
    out.sort_by_key(|l| l.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, line: Option<usize>, column: Option<usize>) -> LinkTarget {
        LinkTarget::File {
            path: path.into(),
            line,
            column,
        }
    }

    fn targets(text: &str) -> Vec<(String, LinkTarget)> {
        let chars: Vec<char> = text.chars().collect();
        find_links(text)
            .into_iter()
            .map(|l| (chars[l.start..l.end].iter().collect(), l.target))
            .collect()
    }

    #[test]
    fn finds_web_addresses_without_the_sentence_around_them() {
        assert_eq!(
            targets("see https://example.com/a_(b)?x=1, (and http://localhost:8080/)."),
            vec![
                (
                    "https://example.com/a_(b)?x=1".into(),
                    LinkTarget::Url {
                        url: "https://example.com/a_(b)?x=1".into()
                    }
                ),
                (
                    "http://localhost:8080/".into(),
                    LinkTarget::Url {
                        url: "http://localhost:8080/".into()
                    }
                ),
            ]
        );
    }

    #[test]
    fn finds_files_with_lines_and_columns() {
        assert_eq!(
            targets("  --> src/main.rs:3:5"),
            vec![(
                "src/main.rs:3:5".into(),
                file("src/main.rs", Some(3), Some(5))
            )]
        );
        assert_eq!(
            targets("main.go:12: undefined: x"),
            vec![("main.go:12".into(), file("main.go", Some(12), None))]
        );
        assert_eq!(
            targets("Program.cs(7,13): error CS0103"),
            vec![(
                "Program.cs(7,13)".into(),
                file("Program.cs", Some(7), Some(13))
            )]
        );
        assert_eq!(
            targets("modified:   crates/app/Cargo.toml"),
            vec![(
                "crates/app/Cargo.toml".into(),
                file("crates/app/Cargo.toml", None, None)
            )]
        );
        assert_eq!(
            targets("at ./lib/x.js:10:2)"),
            vec![(
                "./lib/x.js:10:2".into(),
                file("./lib/x.js", Some(10), Some(2))
            )]
        );
        assert_eq!(targets("version 1.5 took 2.3s"), vec![]);
        assert_eq!(
            targets("a/b"),
            vec![("a/b".into(), file("a/b", None, None))]
        );
    }

    #[test]
    fn python_tracebacks_link_the_quoted_file_with_its_line() {
        assert_eq!(
            targets("  File \"/tmp/my app/x.py\", line 4, in <module>"),
            vec![(
                "/tmp/my app/x.py".into(),
                file("/tmp/my app/x.py", Some(4), None)
            )]
        );
    }

    #[test]
    fn osc8_hyperlinks_and_wide_characters_use_screen_columns() {
        let mut t = crate::Terminal::new(2, 60);
        t.feed("日本 \x1b]8;;https://example.org/\x1b\\docs\x1b]8;;\x1b\\ a.rs:2".as_bytes());
        let links = line_links(&t.screen, t.screen.line(0).unwrap());
        assert_eq!(
            links,
            vec![
                Link {
                    start: 5,
                    end: 9,
                    target: LinkTarget::Url {
                        url: "https://example.org/".into()
                    }
                },
                Link {
                    start: 10,
                    end: 16,
                    target: file("a.rs", Some(2), None)
                },
            ]
        );
    }
}
