//! Blocks of the documentation popup (`SPEC-0077`): the Markdown a language server sends, as
//! headings, paragraphs, labelled sections, code and rules, free of the UI toolkit.

use std::ops::Range;

/// One block of the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocBlock {
    /// A heading line (`#`–`######`).
    Heading(String),
    /// Running text; lines of one paragraph joined with spaces, with the byte ranges of inline code
    /// (text between backticks, `SPEC-0077` v3).
    Text(String, Vec<Range<usize>>),
    /// A labelled section from a javadoc tag list (`* **Returns:**` with its items, v3).
    Section(String, Vec<String>),
    /// A fenced code block, lines kept.
    Code(String),
    /// A horizontal rule (`---`, `***`, `___`).
    Rule,
    /// The type a member belongs to, grey above its signature (`SPEC-0077` v5).
    Owner(String),
    /// The module the declaration is in, grey at the bottom (v5).
    Footer(String),
}

/// Whether `t` reads as a declaration: names, dots, brackets, generics, commas and a few marks,
/// not a sentence (`SPEC-0077` v5).
fn is_declaration(t: &str) -> bool {
    let allowed = |c: char| c.is_alphanumeric() || " _$.<>[](),@?&".contains(c);
    !t.is_empty()
        && t.chars().all(allowed)
        && !t.ends_with(['.', ':', '!', '?'])
        && t.chars().any(char::is_alphabetic)
        // A call's parentheses or a qualified name (`a.b`), so a plain sentence stays text.
        && (t.contains('(')
            || t.split_whitespace().any(|w| {
                w.split('.').filter(|p| !p.is_empty()).count() > 1
            }))
}

/// Splits `s` at commas outside `<>`, `()` and `[]`.
fn top_level_parts(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in s.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }
    parts
}

/// A declaration line as owner and code: the qualifier of the name before `(` (or of the last
/// name) above, the parameters one per line (`SPEC-0077` v5).
fn signature(line: &str) -> (Option<String>, String) {
    let open = line.find('(');
    let head = open.map_or(line, |i| &line[..i]);
    let name_start = head.rfind(' ').map_or(0, |i| i + 1);
    let qualified = &head[name_start..];
    // Generics in the qualifier (`Box<T>.get`) keep their dots.
    let (owner, name) = match qualified.rfind('.') {
        Some(dot) if !qualified[dot..].contains('>') => {
            (Some(qualified[..dot].to_owned()), &qualified[dot + 1..])
        }
        _ => (None, qualified),
    };
    let short_head = format!("{}{name}", &head[..name_start]);
    let Some(open) = open else {
        return (owner, short_head);
    };
    // The parameter list ends at the `)` that closes the first `(`.
    let mut depth = 0i32;
    let mut close = None;
    for (i, c) in line[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return (owner, format!("{short_head}{}", &line[open..]));
    };
    let params = top_level_parts(&line[open + 1..close]);
    let tail = &line[close..];
    if params.is_empty() {
        return (owner, format!("{short_head}(){}", &tail[1..]));
    }
    let mut code = format!("{short_head}(");
    for (i, p) in params.iter().enumerate() {
        code.push_str("\n    ");
        code.push_str(p);
        if i + 1 < params.len() {
            code.push(',');
        }
    }
    code.push('\n');
    code.push_str(tail);
    (owner, code)
}

/// The module name in jdtls's `Source: <project>_<id>` (the id dropped, `SPEC-0077` v5).
fn source_module(text: &str) -> Option<String> {
    let name = text.strip_prefix("Source:")?.trim();
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    let shown = match name.rfind('_') {
        Some(i) if name.len() - i > 6 && name[i + 1..].chars().all(|c| c.is_ascii_hexdigit()) => {
            &name[..i]
        }
        _ => name,
    };
    Some(shown.to_owned())
}

/// jdtls's hover made readable: a plain declaration line as owner and code, `@param` names
/// without a description left out, the `Source:` line as the module footer (`SPEC-0077` v5).
fn jdtls_blocks(mut out: Vec<DocBlock>) -> Vec<DocBlock> {
    if let Some(DocBlock::Text(t, code)) = out.first()
        && code.is_empty()
        && is_declaration(t)
    {
        let (owner, code) = signature(t);
        out.remove(0);
        out.insert(0, DocBlock::Code(code));
        if let Some(owner) = owner {
            out.insert(0, DocBlock::Owner(owner));
        }
    }
    for b in &mut out {
        if let DocBlock::Section(label, items) = b
            && (label == "Parameters" || label == "Type Parameters")
        {
            items.retain(|i| i.split_whitespace().nth(1).is_some());
        }
    }
    out.retain(|b| !matches!(b, DocBlock::Section(_, items) if items.is_empty()));
    if let Some(DocBlock::Text(t, _)) = out.last()
        && let Some(module) = source_module(t)
    {
        out.pop();
        out.push(DocBlock::Footer(module));
    }
    out
}

/// The link `[text](target)` starting at `chars[i]` (on its `[`): its text and the index after it.
/// Also returns the target, with repeated `/` collapsed.
fn link_at(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let close = i + 1 + chars[i + 1..].iter().position(|c| *c == ']')?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let found = |j: usize| {
        let target: String = chars[close + 2..j].iter().collect();
        let (scheme, rest) = target.split_once("://").unwrap_or(("", &target));
        let mut path = String::new();
        for ch in rest.chars() {
            if !(ch == '/' && path.ends_with('/')) {
                path.push(ch);
            }
        }
        let text = chars[i + 1..close].iter().collect();
        Some((text, format!("{scheme}://{path}"), j + 1))
    };
    // The target may hold balanced parentheses itself.
    let mut depth = 0usize;
    for (j, c) in chars.iter().enumerate().skip(close + 2) {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return found(j),
            ')' => depth -= 1,
            _ => {}
        }
    }
    // An opening parenthesis that is never closed (jdtls: `jdt://…(Collection.class#728)`): the
    // target ends at the last `)` before whitespace or the line end (SPEC-0077 v3).
    let end = chars[close + 2..]
        .iter()
        .position(|c| c.is_whitespace())
        .map_or(chars.len(), |k| close + 2 + k);
    let last = (close + 2..end).rev().find(|&j| chars[j] == ')')?;
    found(last)
}

/// Inline Markdown removed: emphasis markers, backticks around code, backslash escapes, and links
/// and images, which keep only their text (`SPEC-0077` v2: ruby-lsp's `[greeter.rb](file://…)`).
fn plain(line: &str) -> String {
    inline(line).0
}

/// Like [`plain`], with the byte ranges of the text that stood between backticks (v3).
fn inline(line: &str) -> (String, Vec<Range<usize>>) {
    let all: Vec<char> = line.chars().collect();
    let mut linked = String::with_capacity(line.len());
    let mut targets: Vec<String> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let image = all[i] == '!' && all.get(i + 1) == Some(&'[');
        let at = if image { i + 1 } else { i };
        if all[at] == '['
            && let Some((text, target, next)) = link_at(&all, at)
        {
            if targets.contains(&target) {
                // The same target again (ruby-lsp lists a file indexed twice): left out with
                // the separator before it.
                let kept = linked
                    .trim_end()
                    .trim_end_matches(['|', ','])
                    .trim_end()
                    .len();
                linked.truncate(kept);
            } else {
                targets.push(target);
                linked.push_str(&text);
            }
            i = next;
            continue;
        }
        linked.push(all[i]);
        i += 1;
    }
    let mut out = String::with_capacity(linked.len());
    let mut code: Vec<Range<usize>> = Vec::new();
    let mut open: Option<usize> = None;
    let mut chars = linked.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(&next) = chars.peek()
                    && next.is_ascii_punctuation()
                {
                    out.push(next);
                    chars.next();
                } else {
                    out.push(ch);
                }
            }
            '`' => match open.take() {
                Some(start) if start < out.len() => code.push(start..out.len()),
                Some(_) => {}
                None => open = Some(out.len()),
            },
            '*' if open.is_none() && chars.peek() == Some(&'*') => {
                chars.next();
            }
            // Single-star emphasis (`*text*`) loses its markers too (v4); a star between
            // blanks or inside a word (`2*3`) stays.
            '*' if open.is_none() && single_emphasis(out.chars().last(), chars.peek().copied()) => {
            }
            _ => out.push(ch),
        }
    }
    let lead = out.len() - out.trim_start().len();
    let text = out.trim().to_owned();
    let code = code
        .into_iter()
        .filter_map(|r| {
            let start = r.start.saturating_sub(lead).min(text.len());
            let end = r.end.saturating_sub(lead).min(text.len());
            (start < end).then_some(start..end)
        })
        .collect();
    (text, code)
}

/// Whether a single `*` between `before` and `after` opens or closes emphasis (`SPEC-0077` v4):
/// at a word's start after a blank or punctuation, or at its end before one.
fn single_emphasis(before: Option<char>, after: Option<char>) -> bool {
    let edge = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || c.is_ascii_punctuation());
    let word = |c: Option<char>| c.is_some_and(|c| !c.is_whitespace());
    (edge(before) && word(after)) || (word(before) && edge(after))
}

/// The written-out form of a javadoc label (`SPEC-0077` v3).
fn section_label(label: &str) -> String {
    match label {
        "Impl Spec" => "Implementation Requirements",
        "Impl Note" => "Implementation Note",
        "API Note" | "Api Note" => "API Note",
        "See" => "See Also",
        other => other,
    }
    .to_owned()
}

/// The text of a list item `* x`, `- x` or `+ x`, if `t` (trimmed) is one.
fn bullet(t: &str) -> Option<&str> {
    ["* ", "- ", "+ "]
        .iter()
        .find_map(|b| t.strip_prefix(b))
        .map(str::trim_start)
}

/// A section label item `**Label:**` with the value after it, if `item` is one (v3).
fn label_item(item: &str) -> Option<(String, String)> {
    let rest = item.strip_prefix("**")?;
    let end = rest.find("**")?;
    let raw = rest[..end].trim();
    let after = rest[end + 2..].trim_start();
    let (label, after) = match raw.strip_suffix(':') {
        Some(l) => (l.trim(), after),
        None => (raw, after.strip_prefix(':')?.trim_start()),
    };
    (!label.is_empty()).then(|| (section_label(label), plain(after)))
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|m| t.chars().all(|c| c.to_string() == *m))
}

/// The blocks of an inspection's description (`SPEC-0078` v2 behaviour 6): paragraphs separated by
/// blank lines; a paragraph whose lines are all indented by four spaces is code.
pub fn description_blocks(text: &str) -> Vec<DocBlock> {
    let mut out = Vec::new();
    for paragraph in text.split("\n\n").filter(|p| !p.trim().is_empty()) {
        let lines: Vec<&str> = paragraph.lines().collect();
        if lines.iter().all(|l| l.starts_with("    ")) {
            let code: Vec<&str> = lines.iter().map(|l| &l[4..]).collect();
            out.push(DocBlock::Code(code.join("\n")));
        } else {
            out.extend(
                lines
                    .split(|l| l.starts_with("    "))
                    .filter(|part| !part.is_empty())
                    .map(|part| DocBlock::Text(part.join(" "), Vec::new())),
            );
            // Code lines right after a heading line such as "Reported:".
            let code: Vec<&str> = lines
                .iter()
                .filter(|l| l.starts_with("    "))
                .map(|l| &l[4..])
                .collect();
            if !code.is_empty() {
                out.push(DocBlock::Code(code.join("\n")));
            }
        }
    }
    out
}

/// The blocks of `markdown`; empty paragraphs and rules at either end are dropped.
pub fn blocks(markdown: &str) -> Vec<DocBlock> {
    let mut out = Vec::new();
    let mut paragraph: Vec<(String, Vec<Range<usize>>)> = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    // The open labelled section: the indentation of its label item, label, items.
    let mut section: Option<(usize, String, Vec<String>)> = None;
    let flush = |paragraph: &mut Vec<(String, Vec<Range<usize>>)>, out: &mut Vec<DocBlock>| {
        if !paragraph.is_empty() {
            let mut text = String::new();
            let mut ranges = Vec::new();
            for (part, code) in paragraph.drain(..) {
                if !text.is_empty() {
                    text.push(' ');
                }
                let at = text.len();
                ranges.extend(code.into_iter().map(|r| r.start + at..r.end + at));
                text.push_str(&part);
            }
            out.push(DocBlock::Text(text, ranges));
        }
    };
    let close = |section: &mut Option<(usize, String, Vec<String>)>, out: &mut Vec<DocBlock>| {
        if let Some((_, label, items)) = section.take() {
            out.push(DocBlock::Section(label, items));
        }
    };
    for line in markdown.lines() {
        if let Some(lines) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                out.push(DocBlock::Code(lines.join("\n")));
                code = None;
            } else {
                lines.push(line);
            }
            continue;
        }
        let t = line.trim();
        let indent = line.len() - line.trim_start().len();
        // Javadoc tag lists become labelled sections (v3).
        if let Some((label, value)) = bullet(t).and_then(label_item) {
            flush(&mut paragraph, &mut out);
            close(&mut section, &mut out);
            let items = if value.is_empty() {
                vec![]
            } else {
                vec![value]
            };
            section = Some((indent, label, items));
            continue;
        }
        if let Some((at, _, items)) = section.as_mut() {
            if t.is_empty() {
                continue;
            }
            if indent > *at {
                match bullet(t) {
                    Some(item) => items.push(plain(item)),
                    None => match items.last_mut() {
                        Some(last) => {
                            last.push(' ');
                            last.push_str(&plain(t));
                        }
                        None => items.push(plain(t)),
                    },
                }
                continue;
            }
            close(&mut section, &mut out);
        }
        if t.starts_with("```") {
            flush(&mut paragraph, &mut out);
            code = Some(Vec::new());
        } else if is_rule(t) {
            flush(&mut paragraph, &mut out);
            out.push(DocBlock::Rule);
        } else if let Some(h) = t.strip_prefix('#') {
            flush(&mut paragraph, &mut out);
            out.push(DocBlock::Heading(plain(h.trim_start_matches('#'))));
        } else if t.is_empty() {
            flush(&mut paragraph, &mut out);
        } else {
            let (text, code) = inline(t);
            if !text.is_empty() {
                paragraph.push((text, code));
            }
        }
    }
    flush(&mut paragraph, &mut out);
    close(&mut section, &mut out);
    if let Some(lines) = code {
        out.push(DocBlock::Code(lines.join("\n")));
    }
    let mut out = jdtls_blocks(out);
    while out.first() == Some(&DocBlock::Rule) {
        out.remove(0);
    }
    while out.last() == Some(&DocBlock::Rule) {
        out.pop();
    }
    out.dedup_by(|a, b| *a == DocBlock::Rule && *b == DocBlock::Rule);
    // A server that found one declaration twice repeats its documentation (ruby-lsp, hand test
    // 110): a paragraph or code block already shown is left out.
    let mut seen: Vec<DocBlock> = Vec::new();
    out.retain(|b| {
        if matches!(b, DocBlock::Rule | DocBlock::Heading(_)) {
            return true;
        }
        if seen.contains(b) {
            return false;
        }
        seen.push(b.clone());
        true
    });
    out.dedup_by(|a, b| *a == DocBlock::Rule && *b == DocBlock::Rule);
    while out.last() == Some(&DocBlock::Rule) {
        out.pop();
    }
    out
}

/// Whether `c` belongs to a word the pointer can rest on (`SPEC-0077` v6 behaviour 5).
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The longest stretch looked at on either side of the pointer.
const WORD_REACH: usize = 256;

/// The char range of the word around char `offset`, reading chars with `char_at`; `None` when the
/// char at `offset` is no word char or there is none (`SPEC-0077` v6 behaviour 5).
pub fn word_at(char_at: impl Fn(usize) -> Option<char>, offset: usize) -> Option<Range<usize>> {
    if !char_at(offset).is_some_and(word_char) {
        return None;
    }
    let mut start = offset;
    while start > 0 && offset - start < WORD_REACH && char_at(start - 1).is_some_and(word_char) {
        start -= 1;
    }
    let mut end = offset + 1;
    while end - offset < WORD_REACH && char_at(end).is_some_and(word_char) {
        end += 1;
    }
    Some(start..end)
}

/// The longest type Type Info shows; a longer one ends with three dots.
pub const TYPE_INFO_LIMIT: usize = 240;

/// What Type Info says when the language server has no answer for the place.
pub const NO_TYPE_INFO: &str = "No type information for this place";

/// The declared type in `line` for the name `name`: `name: T` (Rust, TypeScript, Python, Kotlin,
/// with what the servers put in front: `let mut`, `const`, `(parameter)`), cut before an
/// initial value. Nothing when the name stands inside a parameter list.
fn declared_type(line: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = line[from..].find(name) {
        let start = from + at;
        let end = start + name.len();
        from = end;
        let before = &line[..start];
        let whole = !before.chars().next_back().is_some_and(word_char);
        // A leading `(kind) ` of pyright and the TypeScript server is no parameter list.
        let prefix = match before.trim_start().strip_prefix('(') {
            Some(rest) => rest.split_once(')').map_or(before, |(_, after)| after),
            None => before,
        };
        if !whole || prefix.contains('(') {
            continue;
        }
        let rest = line[end..].trim_start_matches('?');
        let Some(ty) = rest.strip_prefix(':') else {
            continue;
        };
        let ty = ty.split(" = ").next().unwrap_or(ty).trim();
        return (!ty.is_empty()).then(|| ty.to_owned());
    }
    None
}

/// The type a hover text of a language server names for the expression at the caret (`SPEC-0077`
/// v7 behaviour 6), for Type Info. `name` is the word at the caret, if it stands on one.
///
/// In this order: a `Type:` line (clangd); the declared type of `name` in a code block
/// (`let x: i32`, `(variable) x: int`, `const x: number`); `var x T` (gopls) and `T x` (jdtls);
/// else the last code block of the hover as it is — the signature of a function, or the type
/// alone that a server answers for an expression. Without a code block, the first line of text.
pub fn type_info(hover: &str, name: Option<&str>) -> Option<String> {
    let name = name.filter(|n| !n.is_empty());
    // The code blocks, each as its lines; a hover without fences is one block of all its lines.
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    let mut fenced = false;
    for line in hover.lines() {
        if line.trim_start().starts_with("```") {
            fenced = true;
            open = !open;
            if open {
                blocks.push(Vec::new());
            }
        } else if open
            && !line.trim().is_empty()
            && let Some(block) = blocks.last_mut()
        {
            block.push(line.trim());
        }
    }
    blocks.retain(|b| !b.is_empty());
    let plain: Vec<&str> = hover
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("```"))
        .collect();
    let found = plain
        .iter()
        .find_map(|l| {
            l.strip_prefix("Type:")
                .map(|t| t.trim().trim_matches('`').to_owned())
        })
        .filter(|t| !t.is_empty())
        .or_else(|| {
            let name = name?;
            let lines: Vec<&str> = if fenced {
                blocks.iter().flatten().copied().collect()
            } else {
                plain.clone()
            };
            lines
                .iter()
                .find_map(|l| declared_type(l, name))
                .or_else(|| {
                    lines.iter().find_map(|l| {
                        let rest = l.strip_prefix("var ")?.strip_prefix(name)?;
                        let ty = rest.strip_prefix(' ')?.split(" = ").next()?.trim();
                        (!ty.is_empty()).then(|| ty.to_owned())
                    })
                })
                // `T x` of jdtls. The `name:` forms are `declared_type`'s alone: in a parameter
                // list (`fn a(a: i32)`) they are no declaration of the name.
                .or_else(|| {
                    let colon = [format!("{name}:"), format!("{name}?:")];
                    if lines
                        .iter()
                        .any(|l| colon.iter().any(|c| l.contains(c.as_str())))
                    {
                        return None;
                    }
                    idecraft_languages::smart::type_from_hover(hover, name)
                })
        })
        .or_else(|| match blocks.last() {
            Some(block) => Some(block.join(" ")),
            None if fenced => None,
            None => plain.first().map(|l| (*l).to_owned()),
        })?;
    let found = found.split_whitespace().collect::<Vec<_>>().join(" ");
    if found.is_empty() {
        return None;
    }
    Some(if found.chars().count() > TYPE_INFO_LIMIT {
        let cut: String = found.chars().take(TYPE_INFO_LIMIT).collect();
        format!("{cut}...")
    } else {
        found
    })
}

/// A hover asked for by the resting pointer: the file, the start of the word and the text's
/// revision (`SPEC-0077` v6 behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverAsk {
    /// The file.
    pub path: std::path::PathBuf,
    /// Char offset of the word's start.
    pub start: usize,
    /// The editor's text revision when asked.
    pub revision: u64,
}

impl HoverAsk {
    /// Whether the answer to this question is still wanted: the pointer still rests on the same
    /// word (`pending`) and the text is unchanged (`revision`).
    pub fn still_wanted(&self, pending: Option<&HoverAsk>, revision: u64) -> bool {
        pending == Some(self) && self.revision == revision
    }
}

#[cfg(test)]
mod tests {
    /// SPEC-0077/AK-12 (v7): the type Type Info shows, from the hover texts of the language
    /// servers IDECraft starts.
    #[test]
    fn type_info_reads_the_type_from_a_hover() {
        let of = |hover: &str, name: &str| super::type_info(hover, Some(name));
        let some = |s: &str| Some(s.to_owned());
        // rust-analyzer: a local, a field behind its owner, a function.
        assert_eq!(
            of("```rust\nlet mut total: Vec<u8>\n```", "total"),
            some("Vec<u8>")
        );
        assert_eq!(
            of(
                "```rust\nshop::Cart\n```\n\n```rust\nitems: Vec<Item>\n```\n\nThe items.",
                "items"
            ),
            some("Vec<Item>")
        );
        assert_eq!(
            of(
                "```rust\nshop\n```\n\n```rust\nfn add(a: i32, b: i32) -> i32\n```",
                "add"
            ),
            some("fn add(a: i32, b: i32) -> i32")
        );
        assert_eq!(
            of("```rust\nconst LIMIT: usize = 4\n```", "LIMIT"),
            some("usize")
        );
        // A function named like one of its parameters is still a function.
        assert_eq!(
            of("```rust\nfn a(a: i32) -> u8\n```", "a"),
            some("fn a(a: i32) -> u8")
        );
        // The TypeScript server and pyright put the kind in front.
        assert_eq!(
            of("```typescript\nconst count: number\n```", "count"),
            some("number")
        );
        assert_eq!(
            of(
                "```typescript\n(parameter) sku?: string | undefined\n```",
                "sku"
            ),
            some("string | undefined")
        );
        assert_eq!(
            of("```python\n(variable) names: list[str]\n```", "names"),
            some("list[str]")
        );
        assert_eq!(
            of(
                "```python\n(function) def load(path: str) -> dict\n```",
                "load"
            ),
            some("(function) def load(path: str) -> dict")
        );
        // gopls, jdtls, clangd.
        assert_eq!(of("```go\nvar total int\n```", "total"), some("int"));
        assert_eq!(
            of("List<String> other - demo.A.m(String)", "other"),
            some("List<String>")
        );
        assert_eq!(
            of("### variable `x`\n\n---\nType: `int`\nValue = 4", "x"),
            some("int")
        );
        // An expression: the server answers with the type alone.
        assert_eq!(super::type_info("```rust\ni32\n```", None), some("i32"));
        assert_eq!(
            super::type_info("```rust\nOption<&str>\n```", Some("len")),
            some("Option<&str>")
        );
        // Nothing to show.
        assert_eq!(super::type_info("", Some("x")), None);
        assert_eq!(super::type_info("```rust\n```", None), None);
        // A long type is cut.
        let long = format!("```ts\nconst x: {}\n```", "A | ".repeat(200));
        let shown = of(&long, "x").unwrap();
        assert_eq!(shown.chars().count(), super::TYPE_INFO_LIMIT + 3);
        assert!(shown.ends_with("..."));
    }

    /// SPEC-0077/AK-10 (v6).
    #[test]
    fn word_under_the_pointer() {
        let text: Vec<char> = "  {{ msg }} $ref_2 + café.".chars().collect();
        let at = |offset: usize| super::word_at(|i| text.get(i).copied(), offset);
        assert_eq!(at(5), Some(5..8), "first letter");
        assert_eq!(at(7), Some(5..8), "last letter");
        assert_eq!(at(12), Some(12..18), "$ and _ and digits");
        assert_eq!(at(22), Some(21..25), "letters beyond ASCII");
        for offset in [0, 4, 8, 11, 19, 25, 26, 100] {
            assert_eq!(at(offset), None, "{offset}");
        }
        // A very long word is cut, not scanned to its ends.
        let long: Vec<char> = "a".repeat(10_000).chars().collect();
        let range = super::word_at(|i| long.get(i).copied(), 5_000).unwrap();
        assert!(range.contains(&5_000) && range.len() <= 2 * super::WORD_REACH + 1);
    }

    /// SPEC-0077/AK-10 (v6).
    #[test]
    fn hover_answers_for_the_word_still_under_the_pointer() {
        let asked = super::HoverAsk {
            path: "src/App.vue".into(),
            start: 5,
            revision: 3,
        };
        assert!(asked.still_wanted(Some(&asked.clone()), 3));
        assert!(!asked.still_wanted(None, 3), "the pointer left the word");
        let other = super::HoverAsk {
            start: 9,
            ..asked.clone()
        };
        assert!(!asked.still_wanted(Some(&other), 3), "another word");
        assert!(
            !asked.still_wanted(Some(&asked.clone()), 4),
            "the text changed"
        );
    }

    // SPEC-0078 v2 AK-4.
    #[test]
    fn popup_description_has_text_and_code() {
        let blocks = super::description_blocks(
            "Reports a thing. It matters.\n\nReported:\n    a == b\n\nFixed:\n    a.equals(b)",
        );
        assert_eq!(
            blocks,
            [
                super::DocBlock::Text("Reports a thing. It matters.".into(), Vec::new()),
                super::DocBlock::Text("Reported:".into(), Vec::new()),
                super::DocBlock::Code("a == b".into()),
                super::DocBlock::Text("Fixed:".into(), Vec::new()),
                super::DocBlock::Code("a.equals(b)".into()),
            ]
        );
    }

    use super::*;

    fn text(t: &str) -> DocBlock {
        DocBlock::Text(t.into(), vec![])
    }

    /// One inline-code range.
    fn code(start: usize, end: usize) -> Vec<Range<usize>> {
        std::iter::once(start..end).collect()
    }

    /// SPEC-0077 v5 AK-9: the hover jdtls sent for `Discount.apply` (OBS-0016).
    #[test]
    fn a_jdtls_signature_becomes_code() {
        let md = "int lab.inventory.Discount.apply(int cents)\n\nReturns the price in cents after this rule.\n\n* **Parameters:**\n  * **cents**\n\nSource: *[navp_b75acabe](file:///w/navp/src/Discount.java#6)*";
        assert_eq!(
            blocks(md),
            [
                DocBlock::Owner("lab.inventory.Discount".into()),
                DocBlock::Code("int apply(\n    int cents\n)".into()),
                text("Returns the price in cents after this rule."),
                DocBlock::Footer("navp".into()),
            ]
        );
        // A described parameter stays; no parameters stay on one line.
        let md = "java.lang.String lab.A.name()\n\n* **Parameters:**\n  * **x** the value";
        assert_eq!(
            blocks(md),
            [
                DocBlock::Owner("lab.A".into()),
                DocBlock::Code("java.lang.String name()".into()),
                DocBlock::Section("Parameters".into(), vec!["x the value".into()]),
            ]
        );
    }

    /// SPEC-0077 v5 AK-9.
    #[test]
    fn parameters_split_at_top_level_commas() {
        let (owner, code) =
            signature("<T> void a.B.put(java.util.Map<String, T> m, int[] xs) throws E");
        assert_eq!(owner.as_deref(), Some("a.B"));
        assert_eq!(
            code,
            "<T> void put(\n    java.util.Map<String, T> m,\n    int[] xs\n) throws E"
        );
        // A field keeps its type and loses the qualifier.
        assert_eq!(
            signature("int lab.Item.cents"),
            (Some("lab.Item".into()), "int cents".into())
        );
        assert_eq!(source_module("Source: shop"), Some("shop".into()));
        assert_eq!(source_module("Source: a_b"), Some("a_b".into()));
    }

    /// SPEC-0077 v5 AK-9: a sentence and other servers' hovers stay as they were.
    #[test]
    fn sentences_stay_text() {
        assert_eq!(
            blocks("Returns the price.\n\nMore."),
            [text("Returns the price."), text("More.")]
        );
        // Without a `(` or a qualified name a line is not taken for a declaration.
        assert_eq!(
            blocks("A rule that lowers a price"),
            [text("A rule that lowers a price")]
        );
        let clangd = blocks("### function `helper`\n\n```cpp\nint helper(int a, int b)\n```");
        assert_eq!(clangd[1], DocBlock::Code("int helper(int a, int b)".into()));
    }

    #[test]
    fn splits_a_clangd_hover_into_blocks() {
        let md = "### function `helper`  \n\n---\n→ `int`  \n**Returns** a \\_value\\_\nsecond line\n\n---\n```cpp\nint helper(void)\n{ }\n```\n";
        assert_eq!(
            blocks(md),
            [
                DocBlock::Heading("function helper".into()),
                DocBlock::Rule,
                DocBlock::Text("→ int Returns a _value_ second line".into(), code(4, 7)),
                DocBlock::Rule,
                DocBlock::Code("int helper(void)\n{ }".into()),
            ]
        );
    }

    #[test]
    fn plain_text_and_unclosed_code_survive() {
        assert_eq!(
            blocks("one\ntwo\n\nthree"),
            [text("one two"), text("three")]
        );
        assert_eq!(
            blocks("---\n```\nx = 1\n"),
            [DocBlock::Code("x = 1".into())]
        );
        assert!(blocks("  \n---\n").is_empty());
    }

    /// SPEC-0077 v2 (hand test 110): a repeated paragraph is shown once.
    #[test]
    fn repeated_documentation_is_shown_once() {
        let md = "```ruby\nGreeter\n```\n\nGreets someone.\n\n---\n\nGreets someone.\n";
        assert_eq!(
            blocks(md),
            [DocBlock::Code("Greeter".into()), text("Greets someone."),]
        );
    }

    /// SPEC-0077 v2 (hand test 110): links keep only their text, the target is not shown.
    #[test]
    fn links_show_their_text() {
        let md = "```ruby\nGreeter\n```\n\n**Definitions**: [greeter.rb](file:///tmp//greeter.rb#L2,1-10,4) | [greeter.rb](file:///tmp/greeter.rb#L2,1-10,4)\n\nGreets [someone](https://x.org/a_(b)).\n![logo](x.png) [not a link] and [x]";
        assert_eq!(
            blocks(md),
            [
                DocBlock::Code("Greeter".into()),
                text("Definitions: greeter.rb"),
                text("Greets someone. logo [not a link] and [x]"),
            ]
        );
    }

    /// SPEC-0077 v3 (OBS-0012): jdtls targets hold an unclosed `(`; only the text is shown.
    #[test]
    fn jdt_links_show_their_text() {
        let md = "See [spliterator()](jdt://contents/java.base/java.util/Collection.class?=java.base/%3Cjava.util(Collection.class#728) and [Stream](jdt://contents/java.base/java.util.stream/Stream.class?=java.base/%3Cjava.util.stream(Stream.class) here.";
        assert_eq!(blocks(md), [text("See spliterator() and Stream here.")]);
    }

    /// SPEC-0077 v3 (OBS-0012): javadoc tag bullets become labelled sections.
    #[test]
    fn javadoc_tags_become_sections() {
        let md = "Returns a sequential `Stream` over this collection.\n\n *  **Returns:**\n    \n     *  a sequential `Stream` over the\n        elements in this collection\n *  **Since:**\n    \n     *  1.8\n *  **Impl Spec:**\n    \n     *  The default creates a stream.\n * **See:** [List](jdt://x(List.class)\n\nAfter.";
        assert_eq!(
            blocks(md),
            [
                DocBlock::Text(
                    "Returns a sequential Stream over this collection.".into(),
                    code(21, 27)
                ),
                DocBlock::Section(
                    "Returns".into(),
                    vec!["a sequential Stream over the elements in this collection".into()]
                ),
                DocBlock::Section("Since".into(), vec!["1.8".into()]),
                DocBlock::Section(
                    "Implementation Requirements".into(),
                    vec!["The default creates a stream.".into()]
                ),
                DocBlock::Section("See Also".into(), vec!["List".into()]),
                text("After."),
            ]
        );
        // A bold item that is no label stays text.
        assert_eq!(blocks("* **bold** word"), [text("* bold word")]);
    }

    /// SPEC-0077 v4 AK-8: single-star emphasis is removed, other stars stay.
    #[test]
    fn single_star_emphasis_is_removed() {
        assert_eq!(
            blocks("Source: *[cmp-nav-ic](file:///w/D.java#6)*"),
            // Since v5 the source line is the module footer.
            [DocBlock::Footer("cmp-nav-ic".into())]
        );
        assert_eq!(blocks("a *b* c"), [text("a b c")]);
        assert_eq!(blocks("a * b and 2*3"), [text("a * b and 2*3")]);
    }

    /// SPEC-0077 v3: inline code keeps its place across joined lines.
    #[test]
    fn inline_code_is_marked() {
        assert_eq!(
            blocks("Use `a` and\n`b_c` now, `**x**`."),
            [DocBlock::Text(
                "Use a and b_c now, **x**.".into(),
                vec![4..5, 10..13, 19..24]
            )]
        );
        assert_eq!(blocks("`unclosed"), [text("unclosed")]);
    }
}
