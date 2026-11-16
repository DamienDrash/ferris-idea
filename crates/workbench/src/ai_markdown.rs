//! The Markdown of the assistant's answers as the AI Chat draws it (`SPEC-0138` v4 behaviour 7):
//! blocks (headings, paragraphs, list items, quotes, code blocks, tables, rules) of inline runs
//! (plain, bold, italic, inline code, links). The parser is forgiving: an answer that is still
//! arriving, with a fence not yet closed or a table half written, gives blocks all the same.

/// A piece of text with one look.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Run {
    /// The text as shown.
    pub text: String,
    /// `**bold**` or `__bold__`.
    pub bold: bool,
    /// `*italic*` or `_italic_`.
    pub italic: bool,
    /// `` `code` ``, drawn as a chip in the editors' font.
    pub code: bool,
    /// `[text](url)` or `<url>`: the address.
    pub link: Option<String>,
}

/// A block of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `#` … `######`.
    Heading(u8, Vec<Run>),
    /// Lines joined by spaces.
    Paragraph(Vec<Run>),
    /// `- `, `* `, `+ ` or `1. `: depth (0 for the outer list), the marker as shown, the item.
    Item(usize, String, Vec<Run>),
    /// `> `.
    Quote(Vec<Run>),
    /// A fenced block: the language of its info string (lower case, may be empty) and its text.
    Code(String, String),
    /// A table: header cells, then the rows' cells.
    Table(Vec<Vec<Run>>, Vec<Vec<Vec<Run>>>),
    /// `---`, `***` or `___`.
    Rule,
}

/// The blocks of `text`.
pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let flush = |paragraph: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            let joined = paragraph
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join(" ");
            out.push(Block::Paragraph(inline(&joined)));
            paragraph.clear();
        }
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        // A fenced code block, to its closing fence or the end (still arriving).
        if let Some(fence) = fence_of(t) {
            flush(&mut paragraph, &mut out);
            let info = t[fence.len()..].trim();
            let language = info
                .split(|c: char| c.is_whitespace() || c == '{' || c == ',')
                .next()
                .unwrap_or_default()
                .to_lowercase();
            let indent = line.len() - t.len();
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.trim_start().starts_with(fence)
                    && l.trim()
                        .trim_start_matches(fence.chars().next().unwrap_or('`'))
                        .is_empty()
                {
                    i += 1;
                    break;
                }
                // The fence's own indentation is not part of the code.
                let cut = l.len() - l.trim_start().len();
                code.push(&l[cut.min(indent)..]);
                i += 1;
            }
            out.push(Block::Code(language, code.join("\n")));
            continue;
        }
        if t.trim().is_empty() {
            flush(&mut paragraph, &mut out);
            i += 1;
            continue;
        }
        if let Some((level, rest)) = heading(t) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Heading(level, inline(rest)));
            i += 1;
            continue;
        }
        if is_rule(t) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        // A table: a row with `|` followed by a separator row.
        if t.contains('|') && lines.get(i + 1).is_some_and(|n| is_separator(n)) {
            flush(&mut paragraph, &mut out);
            let header = cells(t);
            let mut rows = Vec::new();
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                rows.push(cells(lines[i]));
                i += 1;
            }
            out.push(Block::Table(header, rows));
            continue;
        }
        if let Some((depth, marker, rest)) = item(line) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Item(depth, marker, inline(rest)));
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix('>') {
            flush(&mut paragraph, &mut out);
            out.push(Block::Quote(inline(rest.trim_start())));
            i += 1;
            continue;
        }
        paragraph.push(line);
        i += 1;
    }
    flush(&mut paragraph, &mut out);
    out
}

/// The fence a line opens with: three or more backticks or tildes.
fn fence_of(t: &str) -> Option<&str> {
    for ch in ['`', '~'] {
        let n = t.chars().take_while(|c| *c == ch).count();
        if n >= 3 {
            return Some(&t[..n]);
        }
    }
    None
}

fn heading(t: &str) -> Option<(u8, &str)> {
    let n = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&n) && t[n..].starts_with(' ') {
        Some((n as u8, t[n..].trim().trim_end_matches('#').trim_end()))
    } else {
        None
    }
}

fn is_rule(t: &str) -> bool {
    let s: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    s.len() >= 3 && ['-', '*', '_'].iter().any(|ch| s.chars().all(|c| c == *ch))
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.contains('-')
        && t.contains('|') | t.starts_with(':') | t.starts_with('-')
        && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// The cells of a table row, outer pipes optional.
fn cells(line: &str) -> Vec<Vec<Run>> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| inline(c.trim())).collect()
}

/// A list item: depth (two spaces or a tab per level), marker, text.
fn item(line: &str) -> Option<(usize, String, &str)> {
    let indent: usize = line
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum();
    let t = line.trim_start();
    let depth = indent / 2;
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some((depth, "•".to_owned(), rest));
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits) {
        let after = &t[digits..];
        if let Some(rest) = after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))
        {
            return Some((depth, format!("{}.", &t[..digits]), rest));
        }
    }
    None
}

/// The inline runs of `text`.
pub fn inline(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Run> = Vec::new();
    let mut bold = false;
    let mut italic = false;
    let mut buf = String::new();
    let push = |out: &mut Vec<Run>, buf: &mut String, bold: bool, italic: bool| {
        if !buf.is_empty() {
            match out.last_mut() {
                Some(last)
                    if last.bold == bold
                        && last.italic == italic
                        && !last.code
                        && last.link.is_none() =>
                {
                    last.text.push_str(buf)
                }
                _ => out.push(Run {
                    text: std::mem::take(buf),
                    bold,
                    italic,
                    ..Run::default()
                }),
            }
            buf.clear();
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // An escaped character stands as it is.
        if c == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() {
            buf.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            let n = chars[i..].iter().take_while(|c| **c == '`').count();
            let close = (i + n..chars.len()).find(|&j| {
                chars[j - 1] != '`' && chars[j..].iter().take_while(|c| **c == '`').count() == n
            });
            if let Some(j) = close {
                push(&mut out, &mut buf, bold, italic);
                let code: String = chars[i + n..j].iter().collect();
                out.push(Run {
                    text: code.trim().to_owned(),
                    code: true,
                    ..Run::default()
                });
                i = j + n;
                continue;
            }
        }
        if c == '['
            && let Some((label, url, next)) = link_at(&chars, i)
        {
            push(&mut out, &mut buf, bold, italic);
            out.push(Run {
                text: label,
                bold,
                italic,
                link: Some(url),
                ..Run::default()
            });
            i = next;
            continue;
        }
        if c == '<' {
            let end = chars[i..].iter().position(|c| *c == '>').map(|p| i + p);
            if let Some(end) = end {
                let inner: String = chars[i + 1..end].iter().collect();
                if (inner.starts_with("http://") || inner.starts_with("https://"))
                    && !inner.contains(char::is_whitespace)
                {
                    push(&mut out, &mut buf, bold, italic);
                    out.push(Run {
                        text: inner.clone(),
                        link: Some(inner),
                        ..Run::default()
                    });
                    i = end + 1;
                    continue;
                }
            }
        }
        if (c == '*' || c == '_') && i + 1 < chars.len() && chars[i + 1] == c {
            let opens = !bold;
            if !opens || closes_later(&chars, i + 2, c, 2) {
                push(&mut out, &mut buf, bold, italic);
                bold = !bold;
                i += 2;
                continue;
            }
        }
        if c == '*' || c == '_' {
            // `_` inside a word (snake_case) is no emphasis.
            let inside_word = c == '_'
                && i > 0
                && chars[i - 1].is_alphanumeric()
                && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            let opens = !italic;
            let next_space = chars.get(i + 1).is_none_or(|n| n.is_whitespace());
            if !inside_word && (!opens || (!next_space && closes_later(&chars, i + 1, c, 1))) {
                push(&mut out, &mut buf, bold, italic);
                italic = !italic;
                i += 1;
                continue;
            }
        }
        buf.push(c);
        i += 1;
    }
    push(&mut out, &mut buf, bold, italic);
    out
}

/// Whether `n` of `c` stand somewhere after `from` (so an opening marker has its partner).
fn closes_later(chars: &[char], from: usize, c: char, n: usize) -> bool {
    (from..chars.len()).any(|j| chars[j..].iter().take_while(|x| **x == c).count() >= n && j > from)
}

/// `[label](url)` at `i`: the label, the address and where the text goes on.
fn link_at(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let close = (i + 1..chars.len()).find(|&j| chars[j] == ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = (close + 2..chars.len()).find(|&j| chars[j] == ')')?;
    let label: String = chars[i + 1..close].iter().collect();
    let url: String = chars[close + 2..end].iter().collect();
    let url = url.split_whitespace().next().unwrap_or_default().to_owned();
    (!url.is_empty()).then_some((label, url, end + 1))
}

/// The words of `runs` for drawing: each word is the pieces (run index, text) between two spaces,
/// so a line breaks only between words, never inside one or before its punctuation; inline code
/// is one piece however many spaces it holds (`SPEC-0138` v4 behaviour 7).
pub fn words(runs: &[Run]) -> Vec<Vec<(usize, String)>> {
    let mut out = Vec::new();
    let mut word: Vec<(usize, String)> = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        if run.code {
            word.push((i, run.text.clone()));
            continue;
        }
        for (k, part) in run.text.split(' ').enumerate() {
            if k > 0 && !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            if !part.is_empty() {
                word.push((i, part.to_owned()));
            }
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// The plain text of `runs` (tests, accessible names).
pub fn plain(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// Whether the address may be opened in the browser after confirmation: `http` and `https`
/// only (`SPEC-0138` v4 behaviour 7).
pub fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The file name the editors' highlighter takes for a fenced block's language, so the block is
/// coloured as a file of that language (`SPEC-0138` v4 behaviour 8); `None` when unknown.
pub fn file_for_language(language: &str) -> Option<&'static str> {
    Some(match language {
        "rust" | "rs" => "x.rs",
        "python" | "py" | "python3" => "x.py",
        "javascript" | "js" | "jsx" | "node" => "x.js",
        "typescript" | "ts" => "x.ts",
        "tsx" => "x.tsx",
        "java" => "x.java",
        "kotlin" | "kt" => "x.kt",
        "go" | "golang" => "x.go",
        "c" | "h" => "x.c",
        "cpp" | "c++" | "cxx" | "hpp" => "x.cpp",
        "csharp" | "cs" | "c#" => "x.cs",
        "ruby" | "rb" => "x.rb",
        "php" => "x.php",
        "swift" => "x.swift",
        "scala" => "x.scala",
        "bash" | "sh" | "shell" | "zsh" | "console" => "x.sh",
        "json" => "x.json",
        "yaml" | "yml" => "x.yaml",
        "toml" => "x.toml",
        "sql" => "x.sql",
        "html" => "x.html",
        "css" => "x.css",
        "scss" => "x.scss",
        "xml" => "x.xml",
        "markdown" | "md" => "x.md",
        "dockerfile" | "docker" => "Dockerfile",
        "lua" => "x.lua",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(text: &str) -> Run {
        Run {
            text: text.into(),
            ..Run::default()
        }
    }

    /// SPEC-0138 v4 AK-6: the blocks of an answer with every kind.
    #[test]
    fn blocks_of_an_answer() {
        let answer = "## Two bugs\n\nThe function `mean` divides by **zero** for an empty list.\nSee [the docs](https://docs.python.org/3/).\n\n1. Check the length\n2. Return `None`\n   - nested\n\n> Note this.\n\n| Function | Bug |\n|---|:-:|\n| `mean` | empty list |\n| median | even count |\n\n---\n\n```python\ndef mean(xs):\n    return sum(xs) / len(xs)\n```\n";
        let blocks = parse(answer);
        assert_eq!(blocks[0], Block::Heading(2, vec![r("Two bugs")]));
        let Block::Paragraph(p) = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert_eq!(
            plain(p),
            "The function mean divides by zero for an empty list. See the docs."
        );
        assert!(p.iter().any(|r| r.code && r.text == "mean"));
        assert!(p.iter().any(|r| r.bold && r.text == "zero"));
        assert!(
            p.iter()
                .any(|r| r.link.as_deref() == Some("https://docs.python.org/3/"))
        );
        assert_eq!(
            blocks[2],
            Block::Item(0, "1.".into(), vec![r("Check the length")])
        );
        let Block::Item(0, m, item) = &blocks[3] else {
            panic!()
        };
        assert_eq!(m, "2.");
        assert!(item[1].code);
        assert_eq!(blocks[4], Block::Item(1, "•".into(), vec![r("nested")]));
        assert_eq!(blocks[5], Block::Quote(vec![r("Note this.")]));
        let Block::Table(header, rows) = &blocks[6] else {
            panic!("{:?}", blocks[6])
        };
        assert_eq!(
            header.iter().map(|c| plain(c)).collect::<Vec<_>>(),
            ["Function", "Bug"]
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0][0][0].code);
        assert_eq!(blocks[7], Block::Rule);
        assert_eq!(
            blocks[8],
            Block::Code(
                "python".into(),
                "def mean(xs):\n    return sum(xs) / len(xs)".into()
            )
        );
        assert_eq!(blocks.len(), 9);
    }

    /// An answer that is still arriving: an open fence is a code block already.
    #[test]
    fn an_open_fence_and_plain_text() {
        assert_eq!(
            parse("Here:\n```rust\nfn main() {"),
            vec![
                Block::Paragraph(vec![r("Here:")]),
                Block::Code("rust".into(), "fn main() {".into())
            ]
        );
        assert_eq!(parse(""), vec![]);
        // snake_case, a lone star, 2 * 3 and an escaped star stay text.
        assert_eq!(
            plain(&inline("call my_function_name with 2 * 3 and \\*x\\*")),
            "call my_function_name with 2 * 3 and *x*"
        );
        let runs = inline("an *italic* and __bold__ word");
        assert!(runs.iter().any(|r| r.italic && r.text == "italic"));
        assert!(runs.iter().any(|r| r.bold && r.text == "bold"));
        assert_eq!(plain(&inline("`a``b`")), "a``b");
        assert_eq!(plain(&inline("``x ` y``")), "x ` y");
    }

    /// Lines break between words only: `mean(xs)` and "zero," stay whole (OBS-0063 item 5).
    #[test]
    fn words_keep_code_and_punctuation() {
        let runs = inline("call `stats.mean(xs) / 2` on **zero**, (see [docs](https://x.org)).");
        let words: Vec<String> = words(&runs)
            .into_iter()
            .map(|w| w.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("|"))
            .collect();
        assert_eq!(
            words,
            [
                "call",
                "stats.mean(xs) / 2",
                "on",
                "zero|,",
                "(see",
                "docs|)."
            ]
        );
    }

    #[test]
    fn links_open_only_over_http() {
        assert!(openable("https://example.com/a"));
        assert!(openable("http://localhost:8080"));
        assert!(!openable("file:///etc/passwd"));
        assert!(!openable("javascript:alert(1)"));
        assert!(!openable("https://x y"));
        assert_eq!(file_for_language("python"), Some("x.py"));
        assert_eq!(file_for_language("klingon"), None);
    }
}
