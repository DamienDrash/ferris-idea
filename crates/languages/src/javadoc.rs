//! Java documentation rendered on our side when jdtls garbles it (`SPEC-0072` v8 behaviour 27).
//!
//! jdtls converts some Javadoc into broken Markdown (an inline `{@return …}` with nested tags
//! leaves stray `}` and links whose text runs into the next sentence). Such text is recognised
//! here; the comment is then read from the source jdtls points at (the JDK's `lib/src.zip` next to
//! the `jrt-fs.jar` in its `jdt://` links, a `-sources.jar` next to a library jar, or a project
//! file) and converted to Markdown by an own converter. Without a source the broken text is
//! cleaned up.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::insight::CompletionItem;

/// Whether `markdown` from jdtls is garbled: a `}` or `{@` outside code spans, code blocks and link
/// targets.
pub fn looks_garbled(markdown: &str) -> bool {
    let prose = prose_of(markdown);
    prose.contains('}') || prose.contains("{@")
}

/// `markdown` without fenced blocks, code spans and link targets.
fn prose_of(markdown: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push_str(line);
            out.push('\n');
        }
    }
    let mut prose = String::new();
    let mut rest = out.as_str();
    while let Some(at) = rest.find(['`', ']']) {
        prose.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix('`') {
            match after.find('`') {
                Some(end) => rest = &after[end + 1..],
                None => {
                    rest = after;
                }
            }
        } else if let Some(target) = tail.strip_prefix("](") {
            rest = target.find(')').map_or("", |end| &target[end + 1..]);
        } else {
            rest = &tail[1..];
        }
    }
    prose.push_str(rest);
    prose
}

/// The broken text cleaned up when no source is found: `jdt://` link targets and stray braces
/// removed, link brackets dropped.
pub fn cleaned(markdown: &str) -> String {
    let mut out = String::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("](") {
        out.push_str(&rest[..at]);
        let target = &rest[at + 2..];
        let end = target.find(')').unwrap_or(target.len());
        if target.starts_with("jdt://") || target.starts_with("file://") {
            out.push(' ');
            rest = target.get(end + 1..).unwrap_or("");
        } else {
            out.push_str("](");
            out.push_str(&target[..end.min(target.len())]);
            rest = &target[end..];
        }
    }
    out.push_str(rest);
    // Brackets of links whose target is gone are dropped; a stray brace stands where jdtls lost a
    // space.
    let mut result = String::new();
    for c in out.chars() {
        match c {
            '[' | ']' => {}
            '}' => result.push(' '),
            c => result.push(c),
        }
    }
    // Runs of spaces become one, line by line.
    result
        .lines()
        .map(|l| {
            let lead = l.len() - l.trim_start().len();
            format!(
                "{}{}",
                &l[..lead],
                l.split(' ')
                    .filter(|w| !w.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The member a completion item names: its declaring type (simple name), its name and, for
/// methods, the number of parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Simple name of the declaring type, e.g. `String`.
    pub class: String,
    /// The member's name; the type's own name for a type.
    pub name: String,
    /// Number of parameters of a method; `None` for fields and types.
    pub params: Option<usize>,
}

/// The member of a jdtls completion item: `String.isBlank() : boolean` with label `isBlank` →
/// `String`, `isBlank`, 0 parameters; a type item (`java.util.ArrayList`) is its own class.
pub fn member_of(item: &CompletionItem) -> Option<Member> {
    let detail = item.detail.as_deref()?.trim();
    let kind = item.kind.as_deref().unwrap_or_default();
    if matches!(kind, "class" | "interface" | "enum" | "struct") {
        let name = detail.split('<').next()?.rsplit('.').next()?.to_owned();
        return Some(Member {
            class: name.clone(),
            name,
            params: None,
        });
    }
    let head = detail.split(" : ").next()?.split('(').next()?.trim();
    let (class, name) = head.rsplit_once('.')?;
    let class = class.rsplit('.').next()?.to_owned();
    let params = item
        .label_detail
        .as_deref()
        .filter(|d| d.trim_start().starts_with('('))
        .map(parameter_count)
        .or_else(|| detail.contains('(').then(|| parameter_count(detail)));
    Some(Member {
        class,
        name: name.to_owned(),
        params,
    })
}

/// The number of parameters in the first parenthesised list of `signature`.
fn parameter_count(signature: &str) -> usize {
    let Some(open) = signature.find('(') else {
        return 0;
    };
    let mut depth = 0usize;
    let mut commas = 0;
    let mut any = false;
    for c in signature[open + 1..].chars() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' if depth == 0 => break,
            ')' | '>' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas += 1,
            c if !c.is_whitespace() => any = true,
            _ => {}
        }
    }
    if any { commas + 1 } else { 0 }
}

/// Where a source file can be read from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// An entry of a zip archive (`src.zip`, `-sources.jar`).
    Zip(PathBuf, String),
    /// A file on disk.
    File(PathBuf),
}

/// One `jdt://` or `file://` link of jdtls documentation: the archive or file and the class.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    /// The jar or `jrt-fs.jar` (or the file for `file://`).
    container: PathBuf,
    /// `java.base` for JDK modules, empty otherwise.
    module: String,
    /// Package, dotted.
    package: String,
    /// Simple class name.
    class: String,
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The links in `markdown`.
fn links(markdown: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("](") {
        let target = &rest[at + 2..];
        let end = target.find(')').unwrap_or(target.len());
        if let Some(link) = parse_link(&target[..end]) {
            out.push(link);
        }
        rest = &target[end.min(target.len())..];
    }
    out
}

/// `jdt://contents/java.base/java.lang/String.java?=proj/%5C/path%5C/lib%5C/jrt-fs.jar%60java.base=…`
/// or `file:///…/Foo.java#3`.
fn parse_link(uri: &str) -> Option<Link> {
    if let Some(path) = uri.strip_prefix("file://") {
        let path = PathBuf::from(percent_decode(path.split('#').next()?));
        return Some(Link {
            class: path.file_stem()?.to_str()?.to_owned(),
            container: path,
            module: String::new(),
            package: String::new(),
        });
    }
    let rest = uri.strip_prefix("jdt://contents/")?;
    let (head, query) = rest.split_once('?')?;
    let parts: Vec<&str> = head.split('/').collect();
    let file = parts.last()?;
    let class = file
        .strip_suffix(".java")
        .or_else(|| file.strip_suffix(".class"))?
        .split('$')
        .next()?
        .to_owned();
    let (module, package) = match parts.as_slice() {
        [module, package, _] if !module.contains(".jar") => {
            ((*module).to_owned(), (*package).to_owned())
        }
        [.., package, _] => (String::new(), (*package).to_owned()),
        _ => return None,
    };
    // After `=`: the project name, then the escaped path of the container.
    let decoded = percent_decode(query.strip_prefix('=')?);
    let after_project = &decoded[decoded.find('/')?..];
    let jar_end = after_project.find(".jar")? + 4;
    let container: String = after_project[..jar_end]
        .chars()
        .filter(|c| *c != '\\')
        .collect();
    Some(Link {
        container: PathBuf::from(container),
        module,
        package,
        class,
    })
}

/// Where the source of `class` can be read, judged from the links of its documentation.
fn sources_for(class: &str, links: &[Link]) -> Vec<Source> {
    let mut out = Vec::new();
    // Links to the class itself first, then the other links' containers with their packages.
    let mut ordered: Vec<&Link> = links.iter().filter(|l| l.class == class).collect();
    ordered.extend(links.iter().filter(|l| l.class != class));
    for l in ordered {
        let source = if l.container.extension().is_some_and(|e| e == "java") {
            let file = l.container.with_file_name(format!("{class}.java"));
            Source::File(file)
        } else if l.container.file_name().is_some_and(|n| n == "jrt-fs.jar") {
            let zip = l.container.with_file_name("src.zip");
            Source::Zip(
                zip,
                format!("{}/{}/{class}.java", l.module, l.package.replace('.', "/")),
            )
        } else {
            let stem = l
                .container
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            // Beside the jar, or in another folder of the Gradle cache (SPEC-0149 v3 behaviour 11).
            let archive = ferris_idea_core::jvmsources::sources_archive_for(&l.container)
                .unwrap_or_else(|| l.container.with_file_name(format!("{stem}-sources.jar")));
            Source::Zip(
                archive,
                format!("{}/{class}.java", l.package.replace('.', "/")),
            )
        };
        if !out.contains(&source) {
            out.push(source);
        }
    }
    out
}

/// The simple name of the class a `jdt://contents/…` address of jdtls points at.
pub fn library_class(uri: &str) -> Option<String> {
    uri.starts_with("jdt://")
        .then(|| parse_link(uri).map(|l| l.class))
        .flatten()
}

/// The source of the library class at a `jdt://contents/…` address (`SPEC-0149` v3 behaviour 11):
/// read from the library's sources archive (beside the jar, in another folder of the Gradle cache,
/// or the JDK's `src.zip`) and written to `cache/library-sources/<archive>/<entry>`, which is
/// returned. `None` when no archive has the class.
pub fn library_source(uri: &str, cache: &Path) -> Option<PathBuf> {
    if !uri.starts_with("jdt://") {
        return None;
    }
    let link = parse_link(uri)?;
    for source in sources_for(&link.class, std::slice::from_ref(&link)) {
        let Source::Zip(zip, entry) = &source else {
            continue;
        };
        let Some(text) = read_zip_entry(zip, entry) else {
            continue;
        };
        let archive = zip.file_name()?.to_string_lossy().into_owned();
        let file = cache.join("library-sources").join(archive).join(entry);
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, text).ok()?;
        return Some(file);
    }
    None
}

/// The 0-based line of `text` (a Java source) that declares type `class`; 0 when none does.
pub fn declaration_line(text: &str, class: &str) -> usize {
    text.lines()
        .position(|l| {
            let words: Vec<&str> = l
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '@' || c == '$'))
                .filter(|w| !w.is_empty())
                .collect();
            words.windows(2).any(|w| {
                matches!(
                    w[0],
                    "class" | "interface" | "enum" | "record" | "@interface"
                ) && w[1] == class
            })
        })
        .unwrap_or(0)
}

/// Reads `entry` from the zip archive at `zip` (stored or deflated).
pub fn read_zip_entry(zip: &Path, entry: &str) -> Option<String> {
    let mut f = std::fs::File::open(zip).ok()?;
    let len = f.seek(SeekFrom::End(0)).ok()?;
    let tail_len = len.min(66_000);
    f.seek(SeekFrom::Start(len - tail_len)).ok()?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).ok()?;
    let eocd = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])?;
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let cd_size = u32_at(&tail, eocd + 12) as usize;
    let cd_offset = u32_at(&tail, eocd + 16) as u64;
    f.seek(SeekFrom::Start(cd_offset)).ok()?;
    let mut cd = vec![0u8; cd_size];
    f.read_exact(&mut cd).ok()?;
    let mut i = 0;
    while i + 46 <= cd.len() && cd[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] {
        let method = u16_at(&cd, i + 10);
        let compressed = u32_at(&cd, i + 20) as usize;
        let name_len = u16_at(&cd, i + 28);
        let extra = u16_at(&cd, i + 30);
        let comment = u16_at(&cd, i + 32);
        let local = u32_at(&cd, i + 42) as u64;
        let name = cd.get(i + 46..i + 46 + name_len)?;
        if name == entry.as_bytes() {
            f.seek(SeekFrom::Start(local)).ok()?;
            let mut header = [0u8; 30];
            f.read_exact(&mut header).ok()?;
            let skip = u16_at(&header, 26) + u16_at(&header, 28);
            f.seek(SeekFrom::Current(skip as i64)).ok()?;
            let mut data = vec![0u8; compressed];
            f.read_exact(&mut data).ok()?;
            let bytes = match method {
                0 => data,
                8 => {
                    let mut out = Vec::new();
                    flate2::read::DeflateDecoder::new(&data[..])
                        .read_to_end(&mut out)
                        .ok()?;
                    out
                }
                _ => return None,
            };
            return String::from_utf8(bytes).ok();
        }
        i += 46 + name_len + extra + comment;
    }
    None
}

fn read_source(source: &Source) -> Option<String> {
    match source {
        Source::File(p) => std::fs::read_to_string(p).ok(),
        Source::Zip(zip, entry) => read_zip_entry(zip, entry),
    }
}

/// The Javadoc comment (`/** … */`) of `member` in the Java `source`.
pub fn doc_comment(source: &str, member: &Member) -> Option<String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let wanted = match node.kind() {
            "method_declaration" | "constructor_declaration" => member.params.is_some(),
            "field_declaration" | "constant_declaration" => member.params.is_none(),
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration" => member.params.is_none() && member.name == member.class,
            _ => false,
        };
        if wanted && declares(node, source, member) {
            let mut prev = node.prev_sibling();
            while let Some(p) = prev {
                if p.kind() == "block_comment" {
                    let text = &source[p.byte_range()];
                    return text.starts_with("/**").then(|| text.to_owned());
                }
                if p.kind() != "line_comment" {
                    break;
                }
                prev = p.prev_sibling();
            }
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    None
}

/// Whether the declaration `node` is `member`.
fn declares(node: tree_sitter::Node, source: &str, member: &Member) -> bool {
    let text = |n: tree_sitter::Node| &source[n.byte_range()];
    if node.kind().ends_with("field_declaration") || node.kind() == "constant_declaration" {
        let mut cursor = node.walk();
        return node
            .children_by_field_name("declarator", &mut cursor)
            .any(|d| {
                d.child_by_field_name("name")
                    .is_some_and(|n| text(n) == member.name)
            });
    }
    if node.child_by_field_name("name").map(text) != Some(member.name.as_str()) {
        return false;
    }
    match (member.params, node.child_by_field_name("parameters")) {
        (Some(n), Some(params)) => {
            let mut cursor = params.walk();
            params
                .named_children(&mut cursor)
                .filter(|c| matches!(c.kind(), "formal_parameter" | "spread_parameter"))
                .count()
                == n
        }
        (None, _) => true,
        (Some(_), None) => false,
    }
}

/// Converts a Javadoc comment to Markdown: description, then block tags as the labelled sections
/// the documentation view shows (`SPEC-0077` v3).
pub fn to_markdown(comment: &str) -> String {
    let body = comment
        .trim()
        .trim_start_matches("/**")
        .trim_end_matches("*/");
    let lines: Vec<String> = body
        .lines()
        .map(|l| {
            let t = l.trim_start();
            let t = t.strip_prefix('*').unwrap_or(t);
            t.strip_prefix(' ').unwrap_or(t).trim_end().to_owned()
        })
        .collect();
    // Block tags start a line with `@` outside `<pre>`.
    let mut description = Vec::new();
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut pre = false;
    for line in lines {
        let t = line.trim_start();
        if t.contains("<pre") {
            pre = true;
        }
        if t.contains("</pre>") {
            pre = false;
        }
        if !pre && t.starts_with('@') {
            let (tag, rest) = t[1..]
                .split_once(char::is_whitespace)
                .unwrap_or((&t[1..], ""));
            tags.push((tag.to_owned(), rest.trim().to_owned()));
        } else if let Some(last) = tags.last_mut() {
            last.1.push('\n');
            last.1.push_str(&line);
        } else {
            description.push(line);
        }
    }
    let mut out = html(&inline(&description.join("\n")));
    let mut sections: Vec<(&str, Vec<String>)> = Vec::new();
    let mut add =
        |label: &'static str, item: String| match sections.iter_mut().find(|(l, _)| *l == label) {
            Some((_, items)) => items.push(item),
            None => sections.push((label, vec![item])),
        };
    for (tag, text) in &tags {
        let words = |t: &str| {
            let t = t.trim();
            match t.split_once(char::is_whitespace) {
                Some((a, b)) => (a.to_owned(), b.trim().to_owned()),
                None => (t.to_owned(), String::new()),
            }
        };
        let flat = |t: &str| one_line(&html(&inline(t)));
        match tag.as_str() {
            "param" => {
                let (name, desc) = words(text);
                add("Parameters", format!("**{name}** {}", flat(&desc)));
            }
            "return" => add("Returns", flat(text)),
            "throws" | "exception" => {
                let (name, desc) = words(text);
                add("Throws", format!("`{name}` - {}", flat(&desc)));
            }
            "since" => add("Since", flat(text)),
            "see" => add("See Also", flat(&reference_text(text.trim(), true))),
            "deprecated" => add("Deprecated", flat(text)),
            "author" => add("Author", flat(text)),
            "version" => add("Version", flat(text)),
            "apiNote" => add("API Note", flat(text)),
            "implSpec" => add("Implementation Requirements", flat(text)),
            "implNote" => add("Implementation Note", flat(text)),
            _ => {}
        }
    }
    if !sections.is_empty() {
        out.push_str("\n\n");
        for (label, items) in sections {
            out.push_str(&format!("* **{label}:**\n"));
            for item in items {
                out.push_str(&format!("  * {item}\n"));
            }
        }
    }
    out.trim().to_owned()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Character#isWhitespace(int)` → `Character.isWhitespace(int)`, `#isEmpty` → `isEmpty`; a
/// quoted string or an HTML link of `@see` is kept as its text.
fn reference_text(reference: &str, see: bool) -> String {
    if see && (reference.starts_with('"') || reference.starts_with('<')) {
        return reference.trim_matches('"').to_owned();
    }
    let (target, label) = split_reference(reference);
    if !label.is_empty() {
        return label;
    }
    format!("`{}`", target.trim_start_matches('#').replace('#', "."))
}

/// The reference of a `{@link}` and its label: `Character#isWhitespace(int) white space` →
/// (`Character#isWhitespace(int)`, `white space`).
fn split_reference(content: &str) -> (String, String) {
    let content = content.trim();
    let mut depth = 0usize;
    for (i, c) in content.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => {
                return (content[..i].to_owned(), content[i..].trim().to_owned());
            }
            _ => {}
        }
    }
    (content.to_owned(), String::new())
}

/// Inline tags converted: `{@code x}` → `` `x` ``, `{@link a b}` → `b`, `{@return x}` → `Returns x.`.
pub fn inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' && chars.get(i + 1) == Some(&'@') {
            // The tag's end, counting nested braces.
            let mut depth = 0usize;
            let mut end = i;
            for (k, c) in chars.iter().enumerate().skip(i) {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end == i {
                out.extend(&chars[i..]);
                break;
            }
            let inner: String = chars[i + 2..end].iter().collect();
            let (tag, content) = inner
                .split_once(char::is_whitespace)
                .map_or((inner.as_str(), ""), |(t, c)| (t, c));
            let content = content.trim();
            out.push_str(&match tag {
                "code" | "literal" | "value" => {
                    if content.contains('`') {
                        format!("`` {content} ``")
                    } else {
                        format!("`{content}`")
                    }
                }
                "link" | "linkplain" => {
                    let (target, label) = split_reference(content);
                    if !label.is_empty() {
                        inline(&label)
                    } else if tag == "link" {
                        format!("`{}`", target.trim_start_matches('#').replace('#', "."))
                    } else {
                        target.trim_start_matches('#').replace('#', ".")
                    }
                }
                "return" => {
                    let body = inline(content);
                    let body = body.trim_end();
                    if body.ends_with('.') {
                        format!("Returns {body}")
                    } else {
                        format!("Returns {body}.")
                    }
                }
                "inheritDoc" => String::new(),
                "index" => split_reference(content).0,
                _ => inline(content),
            });
            i = end + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Common HTML of Javadoc as Markdown.
pub fn html(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut pre = false;
    while let Some(at) = rest.find('<') {
        let piece = &rest[..at];
        if pre {
            out.push_str(piece);
        } else {
            out.push_str(&entities(piece));
        }
        let tail = &rest[at..];
        let Some(close) = tail.find('>') else {
            out.push_str(&entities(tail));
            rest = "";
            break;
        };
        let tag = tail[1..close].trim().to_ascii_lowercase();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default()
            .to_owned();
        let closing = tag.starts_with('/');
        let known = matches!(
            name.as_str(),
            "p" | "br"
                | "code"
                | "tt"
                | "b"
                | "strong"
                | "i"
                | "em"
                | "ul"
                | "ol"
                | "li"
                | "pre"
                | "a"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "blockquote"
                | "div"
                | "span"
                | "dl"
                | "dt"
                | "dd"
                | "table"
                | "tr"
                | "td"
                | "th"
                | "sup"
                | "sub"
                | "hr"
        );
        if !known {
            // Not a tag (`a < b`): keep the text.
            out.push_str(&entities("<"));
            rest = &tail[1..];
            continue;
        }
        out.push_str(match (name.as_str(), closing) {
            ("p", false) => "\n\n",
            ("br", _) | ("hr", _) => "\n",
            ("code" | "tt", _) if !pre => "`",
            ("b" | "strong", _) => "**",
            ("i" | "em", _) => "*",
            ("li" | "dt", false) => "\n* ",
            ("dd", false) => "\n  ",
            ("ul" | "ol" | "dl" | "table", _) => "\n",
            ("pre", false) => {
                pre = true;
                "\n```\n"
            }
            ("pre", true) => {
                pre = false;
                "\n```\n"
            }
            ("h1" | "h2" | "h3" | "h4", false) => "\n\n**",
            ("h1" | "h2" | "h3" | "h4", true) => "**\n\n",
            ("tr", true) => "\n",
            ("td" | "th", false) => " ",
            _ => "",
        });
        rest = &tail[close + 1..];
    }
    if pre {
        out.push_str(rest);
    } else {
        out.push_str(&entities(rest));
    }
    // Code blocks: `{@code …}` inside `<pre>` is already code; leading blank lines removed.
    let mut tidy = String::new();
    let mut blank = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !tidy.is_empty() && blank > 0 {
            tidy.push_str("\n\n");
        } else if !tidy.is_empty() {
            tidy.push('\n');
        }
        blank = 0;
        tidy.push_str(line.trim_end());
    }
    tidy.replace("```\n\n", "```\n").replace("\n\n```", "\n```")
}

fn entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#64;", "@")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// The documentation of a Java completion `item` as jdtls sent it, repaired when garbled: rendered
/// from the source jdtls refers to, else cleaned up. Documentation that is not garbled comes back
/// unchanged.
pub fn repair(item: &CompletionItem, markdown: &str, root: Option<&Path>) -> String {
    if !looks_garbled(markdown) {
        return markdown.to_owned();
    }
    let rendered = member_of(item).and_then(|member| {
        let mut sources = sources_for(&member.class, &links(markdown));
        if let Some(root) = root {
            sources.extend(project_sources(root, &member.class));
        }
        sources
            .iter()
            .filter_map(read_source)
            .find_map(|text| doc_comment(&text, &member))
            .map(|c| to_markdown(&c))
    });
    rendered
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| cleaned(markdown))
}

/// Java files named after `class` in the project (at most a few levels of `src`).
fn project_sources(root: &Path, class: &str) -> Vec<Source> {
    let mut out = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    let name = format!("{class}.java");
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let file = e.file_name();
            let file = file.to_string_lossy();
            if p.is_dir() {
                if depth < 12 && !file.starts_with('.') && file != "target" && file != "build" {
                    dirs.push((p, depth + 1));
                }
            } else if file == name {
                out.push(Source::File(p));
            }
        }
        if out.len() > 3 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape jdtls produced for an inline `{@return}` with nested tags (OBS-0017 step 11),
    /// written with our own words.
    const GARBLED: &str = "Returns .`yes}when the box is `[closed}or holds\nnothing](jdt://contents/java.base/java.lang/Box.java?=p/%5C/opt%5C/jdk%5C/lib%5C/jrt-fs.jar%60java.base=/x=/%3Cjava.lang%28Box.class#12)[at all}really,\nelse](jdt://contents/java.base/java.lang/Other.java?=p/%5C/opt%5C/jdk%5C/lib%5C/jrt-fs.jar%60java.base=/x)`no`\n\n* **Since:**\n  * 11";

    /// SPEC-0072 v8 AK-18: garbled text is recognised, good text is not.
    #[test]
    fn garbled_documentation_is_detected() {
        assert!(looks_garbled(GARBLED));
        assert!(!looks_garbled(
            "Returns `true` if the box is [closed](jdt://x/Box.java#3).\n\n```\nif (a) { b(); }\n```"
        ));
        assert!(looks_garbled("See {@link Foo}"));
    }

    /// SPEC-0072 v8 AK-18: without a source the text is cleaned up.
    #[test]
    fn falls_back_to_a_cleaned_text() {
        let c = cleaned(GARBLED);
        assert!(!c.contains("jdt://"), "{c}");
        assert!(!c.contains('}'), "{c}");
        assert!(c.contains("* **Since:**"), "{c}");
        let item = CompletionItem {
            label: "isClosed".into(),
            kind: Some("method".into()),
            detail: Some("Box.isClosed() : boolean".into()),
            label_detail: Some("()".into()),
            ..Default::default()
        };
        // The JDK path of the links does not exist here: cleaned text.
        assert_eq!(repair(&item, GARBLED, None), c);
        // Text that is not garbled stays.
        assert_eq!(repair(&item, "Fine `x`.", None), "Fine `x`.");
    }

    /// A zip archive with stored (uncompressed) entries.
    fn stored_zip(path: &Path, entries: &[(&str, &str)]) {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, text) in entries {
            let offset = out.len() as u32;
            let data = text.as_bytes();
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central
                .extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        std::fs::write(path, out).unwrap();
    }

    /// SPEC-0149 v3 AK-9: a library class is found in the sources archive beside its jar and in
    /// another folder of the Gradle cache, copied to the cache folder and its declaration found.
    #[test]
    fn library_source_from_archives() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-libsrc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let m2 = dir.join("m2/org/x/lib/1.0");
        std::fs::create_dir_all(&m2).unwrap();
        std::fs::write(m2.join("lib-1.0.jar"), "").unwrap();
        let source = "package org.x;\n\n/** Pads. */\npublic final class Strings {\n    public static String pad() { return \"\"; }\n}\n";
        stored_zip(
            &m2.join("lib-1.0-sources.jar"),
            &[("org/x/Strings.java", source)],
        );
        let jar = m2.join("lib-1.0.jar").display().to_string();
        let uri = format!(
            "jdt://contents/lib-1.0.jar/org.x/Strings$Inner.class?=app/{}%3Corg.x(Strings$Inner.class",
            jar.replace('/', "%5C/")
        );
        assert_eq!(library_class(&uri).as_deref(), Some("Strings"));
        let cache = dir.join("cache");
        let file = library_source(&uri, &cache).unwrap();
        assert!(file.ends_with("library-sources/lib-1.0-sources.jar/org/x/Strings.java"));
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, source);
        assert_eq!(declaration_line(&text, "Strings"), 3);
        assert_eq!(declaration_line("enum Strings {}", "Strings"), 0);
        // Gradle keeps the jar and its sources in folders of their own.
        let files = dir.join("gradle/caches/modules-2/files-2.1/org.y/g/2.0");
        std::fs::create_dir_all(files.join("aaa")).unwrap();
        std::fs::create_dir_all(files.join("bbb")).unwrap();
        std::fs::write(files.join("aaa/g-2.0.jar"), "").unwrap();
        stored_zip(
            &files.join("bbb/g-2.0-sources.jar"),
            &[("org/y/G.java", "package org.y;\ninterface G {}\n")],
        );
        let jar = files.join("aaa/g-2.0.jar").display().to_string();
        let uri = format!(
            "jdt://contents/g-2.0.jar/org.y/G.class?=app/{}%3Corg.y(G.class",
            jar.replace('/', "%5C/")
        );
        let file = library_source(&uri, &cache).unwrap();
        assert_eq!(
            declaration_line(&std::fs::read_to_string(file).unwrap(), "G"),
            1
        );
        // Without a sources archive there is nothing to open.
        let none = uri.replace("g-2.0.jar", "h-2.0.jar");
        assert!(library_source(&none, &cache).is_none());
        assert!(library_source("file:///x/A.java", &cache).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0072 v8 AK-18: links name the JDK's source archive and the class.
    #[test]
    fn links_lead_to_the_source_archive() {
        let found = links(GARBLED);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].container, PathBuf::from("/opt/jdk/lib/jrt-fs.jar"));
        assert_eq!(found[0].module, "java.base");
        assert_eq!(found[0].package, "java.lang");
        assert_eq!(found[0].class, "Box");
        let sources = sources_for("Other", &found);
        assert_eq!(
            sources[0],
            Source::Zip(
                PathBuf::from("/opt/jdk/lib/src.zip"),
                "java.base/java/lang/Other.java".into()
            )
        );
        let member = member_of(&CompletionItem {
            label: "isClosed".into(),
            kind: Some("method".into()),
            detail: Some("Box.isClosed(int a, Map<K, V> b) : boolean".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            member,
            Member {
                class: "Box".into(),
                name: "isClosed".into(),
                params: Some(2)
            }
        );
    }

    /// SPEC-0072 v8 AK-18: Javadoc from a source file becomes Markdown with labelled sections.
    #[test]
    fn renders_javadoc_to_markdown() {
        let source = "package p;\n\nclass Box {\n    /**\n     * {@return {@code true} if the box is {@linkplain #isEmpty empty} or holds\n     * only {@linkplain Thing#isAir(int) air}, otherwise {@code false}}\n     *\n     * <p>Use {@link #open()} first.\n     * @param level how deep, see <b>levels</b>\n     * @throws IllegalStateException when broken\n     * @see Thing#isAir(int)\n     * @since 11\n     */\n    public boolean isClosed(int level) { return true; }\n\n    /** Another. */\n    public boolean isClosed() { return true; }\n}\n";
        let member = Member {
            class: "Box".into(),
            name: "isClosed".into(),
            params: Some(1),
        };
        let comment = doc_comment(source, &member).unwrap();
        let md = to_markdown(&comment);
        assert!(
            md.starts_with(
                "Returns `true` if the box is empty or holds\nonly air, otherwise `false`."
            ),
            "{md}"
        );
        assert!(md.contains("Use `open()` first."), "{md}");
        assert!(
            md.contains("* **Parameters:**\n  * **level** how deep, see **levels**"),
            "{md}"
        );
        assert!(
            md.contains("* **Throws:**\n  * `IllegalStateException` - when broken"),
            "{md}"
        );
        assert!(
            md.contains("* **See Also:**\n  * `Thing.isAir(int)`"),
            "{md}"
        );
        assert!(md.contains("* **Since:**\n  * 11"), "{md}");
        assert!(!looks_garbled(&md), "{md}");
        // The overload without parameters has its own comment.
        let other = Member {
            params: Some(0),
            ..member
        };
        assert_eq!(
            to_markdown(&doc_comment(source, &other).unwrap()),
            "Another."
        );
        // HTML: lists and code blocks.
        assert_eq!(
            html("A<ul><li>one</li><li>two &amp; more</li></ul>"),
            "A\n\n* one\n* two & more"
        );
        assert_eq!(html("<pre>\nx < 1\n</pre>"), "```\nx < 1\n```");
    }

    /// SPEC-0072 v8 AK-18: entries of a zip archive (stored and deflated) are read.
    #[test]
    fn reads_zip_entries() {
        use std::io::Write;
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-javadoc-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("src.zip");
        // One deflated entry written by hand.
        let content = b"class A {}\n".repeat(20);
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&content).unwrap();
        let data = enc.finish().unwrap();
        let name = b"m/p/A.java";
        let mut file = Vec::new();
        let local = 0u32;
        file.extend([0x50, 0x4b, 0x03, 0x04]);
        file.extend([20, 0, 0, 0, 8, 0, 0, 0, 0, 0]);
        file.extend(0u32.to_le_bytes());
        file.extend((data.len() as u32).to_le_bytes());
        file.extend((content.len() as u32).to_le_bytes());
        file.extend((name.len() as u16).to_le_bytes());
        file.extend(0u16.to_le_bytes());
        file.extend(name);
        file.extend(&data);
        let cd_at = file.len() as u32;
        let mut cd = Vec::new();
        cd.extend([0x50, 0x4b, 0x01, 0x02]);
        cd.extend([20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0]);
        cd.extend(0u32.to_le_bytes());
        cd.extend((data.len() as u32).to_le_bytes());
        cd.extend((content.len() as u32).to_le_bytes());
        cd.extend((name.len() as u16).to_le_bytes());
        cd.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.extend(local.to_le_bytes());
        cd.extend(name);
        file.extend(&cd);
        file.extend([0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 1, 0, 1, 0]);
        file.extend((cd.len() as u32).to_le_bytes());
        file.extend(cd_at.to_le_bytes());
        file.extend([0, 0]);
        std::fs::write(&zip, &file).unwrap();
        assert_eq!(
            read_zip_entry(&zip, "m/p/A.java").as_deref(),
            Some(std::str::from_utf8(&content).unwrap())
        );
        assert_eq!(read_zip_entry(&zip, "m/p/B.java"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
