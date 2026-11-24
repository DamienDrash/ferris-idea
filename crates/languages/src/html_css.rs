//! Navigation between HTML and stylesheets and the structure of an HTML file, from Ferris IDEA's own
//! syntax trees (`SPEC-0042` v17 behaviours 28–33): a class or id of an HTML file leads to the
//! rules that define it, a jump link to the element with that id, a class or id selector of a
//! stylesheet lists its uses in HTML files, and the elements of an HTML file are listed nested.
//! Since v22 (behaviours 38–40) a class or id is also renamed here, in every stylesheet and HTML
//! file of the project with its `.` or `#` kept, and the class names and ids of the stylesheets
//! are offered in `class` and `id` values. The HTML language server knows nothing of the
//! stylesheets a page links and lists elements without their nesting; the CSS server renamed a
//! class selector with its mark and in one file only (OBS-0069 G2).
//!
//! Only the project's own files are read ([`ferris_idea_core::OwnSources`]), each through a handle
//! that follows no symbolic link, within the bounds [`MAX_STYLESHEETS`], [`MAX_HTML_FILES`] and
//! [`MAX_FILE_BYTES`]; an address with a scheme or a host is never followed. Names are compared
//! exactly; no pattern is built from file content.

use std::collections::HashMap;
use std::io::Read;
use std::ops::Range as ByteRange;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use ferris_idea_core::{OwnSources, open_regular};
use tree_sitter::{Node, Parser, Tree};

use crate::LanguageId;
use crate::insight::{CompletionItem, Location, Range, Symbol};
use crate::rename::{Edit, FileEdits, RenamePlan};

/// Most stylesheet files read for one question, linked ones first (behaviour 32).
pub const MAX_STYLESHEETS: usize = 200;

/// Most HTML files read for one question (behaviour 32).
pub const MAX_HTML_FILES: usize = 500;

/// A file larger than this is not read (behaviour 32).
pub const MAX_FILE_BYTES: u64 = 1 << 20;

/// Most elements listed in the structure of one file (behaviour 31).
pub const MAX_ELEMENTS: usize = 20_000;

/// Most files whose names are remembered; beyond it the memory is emptied (behaviour 32).
const MAX_REMEMBERED: usize = 2048;

/// A detail longer than this many characters is cut (behaviour 31).
const DETAIL_LIMIT: usize = 200;

/// The symbol kind of an HTML element in the structure (behaviour 31).
pub const ELEMENT_KIND: &str = "element";

/// Whether `file` is an HTML file.
pub fn is_html(file: &Path) -> bool {
    LanguageId::detect(file) == Some(LanguageId::Html)
}

/// The stylesheet language of `file`, by its extension.
fn stylesheet_language(file: &Path) -> Option<LanguageId> {
    LanguageId::detect(file)
        .filter(|l| matches!(l, LanguageId::Css | LanguageId::Scss | LanguageId::Less))
}

/// Whether `file` is a CSS, SCSS or Less file.
pub fn is_stylesheet(file: &Path) -> bool {
    stylesheet_language(file).is_some()
}

/// What a name names: a class or an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameKind {
    Class,
    Id,
}

/// A class or id name at a place of a file: in a selector of a stylesheet, or in a `class` or
/// `id` attribute of an HTML file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Named {
    kind: NameKind,
    /// The name as compared (behaviour 28): without an escaping backslash.
    name: String,
    /// The whole place: for a selector from its `.` or `#` to the end of the name.
    range: Range,
    /// The name as written, without the mark: the target of a declaration (v22 behaviour 28)
    /// and what a rename replaces (behaviour 38).
    written: Range,
    /// The bytes of [`Named::written`] in the file.
    bytes: ByteRange<usize>,
}

impl Named {
    /// Whether the place holds 0-based `at` (line, character), its ends included.
    fn holds(&self, at: (u64, u64)) -> bool {
        (self.range.start_line, self.range.start_character) <= at
            && at <= (self.range.end_line, self.range.end_character)
    }

    /// The same name in a block that starts at byte `start`, `line` and `character` of its file.
    fn moved(self, start: usize, line: u64, character: u64) -> Named {
        Named {
            kind: self.kind,
            name: self.name,
            range: shifted(&self.range, line, character),
            written: shifted(&self.written, line, character),
            bytes: self.bytes.start + start..self.bytes.end + start,
        }
    }
}

fn parse(language: LanguageId, text: &str) -> Option<Tree> {
    let grammar: tree_sitter::Language = match language {
        LanguageId::Html => tree_sitter_html::LANGUAGE.into(),
        LanguageId::Css => tree_sitter_css::LANGUAGE.into(),
        LanguageId::Scss => tree_sitter_scss::language(),
        LanguageId::Less => tree_sitter_less::language(),
        _ => return None,
    };
    if text.len() > crate::syntax::MAX_SYNTAX_BYTES {
        return None;
    }
    let mut parser = Parser::new();
    parser.set_language(&grammar).ok()?;
    parser.parse(text, None)
}

fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

/// Line and character (in chars) of byte offsets of a text, each in constant time whatever the
/// length of the line (a minified file is one long line).
struct Lines<'a> {
    text: &'a str,
    /// Byte offset of the start of each line.
    starts: Vec<usize>,
    /// The number of chars before each block of [`Lines::BLOCK`] bytes.
    chars_before: Vec<usize>,
}

impl<'a> Lines<'a> {
    const BLOCK: usize = 64;

    /// A byte starts a char unless it is a continuation byte (`0b10xx_xxxx`).
    fn starts_char(b: &u8) -> bool {
        (*b as i8) >= -0x40
    }

    fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let mut chars_before = Vec::with_capacity(text.len() / Self::BLOCK + 1);
        let mut seen = 0;
        for block in text.as_bytes().chunks(Self::BLOCK) {
            chars_before.push(seen);
            seen += block.iter().filter(|b| Self::starts_char(b)).count();
        }
        // The end of a text that fills its last block.
        chars_before.push(seen);
        Self {
            text,
            starts,
            chars_before,
        }
    }

    /// The number of chars that start before `byte`.
    fn chars_to(&self, byte: usize) -> usize {
        let block = byte / Self::BLOCK;
        let from = (block * Self::BLOCK).min(byte);
        self.chars_before.get(block).copied().unwrap_or_default()
            + self.text.as_bytes()[from..byte]
                .iter()
                .filter(|b| Self::starts_char(b))
                .count()
    }

    fn position(&self, byte: usize) -> (u64, u64) {
        let byte = byte.min(self.text.len());
        let line = self.starts.partition_point(|&s| s <= byte) - 1;
        let character = self.chars_to(byte) - self.chars_to(self.starts[line]);
        (line as u64, character as u64)
    }

    fn range(&self, bytes: ByteRange<usize>) -> Range {
        let (start_line, start_character) = self.position(bytes.start);
        let (end_line, end_character) = self.position(bytes.end);
        Range {
            start_line,
            start_character,
            end_line,
            end_character,
        }
    }

    /// The byte offset of 0-based `line` and `character` (in chars); the end of the line when it
    /// is shorter, `None` for a line the text does not have.
    fn offset(&self, line: u64, character: u64) -> Option<usize> {
        let start = *self.starts.get(usize::try_from(line).ok()?)?;
        let rest = &self.text[start..];
        let rest = rest.split('\n').next().unwrap_or_default();
        let rest = rest.strip_suffix('\r').unwrap_or(rest);
        let within = rest
            .char_indices()
            .nth(usize::try_from(character).ok()?)
            .map_or(rest.len(), |(i, _)| i);
        Some(start + within)
    }
}

/// An attribute of a tag: its name in lower case and the bytes of its value without quotes
/// (empty at the end of the attribute for one without a value).
struct Attribute {
    name: String,
    value: ByteRange<usize>,
}

/// The attributes of a start tag or self-closing tag.
fn attributes(tag: Node, text: &str) -> Vec<Attribute> {
    children(tag)
        .into_iter()
        .filter(|a| a.kind() == "attribute")
        .filter_map(attribute)
        .map(|(name, value)| Attribute {
            name: text
                .get(name.byte_range())
                .unwrap_or_default()
                .to_ascii_lowercase(),
            value,
        })
        .collect()
}

/// The name node and the value bytes of an `attribute` node.
fn attribute(attr: Node) -> Option<(Node, ByteRange<usize>)> {
    let parts = children(attr);
    let name = parts
        .iter()
        .copied()
        .find(|p| p.kind() == "attribute_name")?;
    let value = parts
        .iter()
        .find_map(|p| match p.kind() {
            "attribute_value" => Some(p.byte_range()),
            "quoted_attribute_value" => Some(
                children(*p)
                    .into_iter()
                    .find(|v| v.kind() == "attribute_value")
                    .map_or_else(
                        // `""`: an empty value between the quotes.
                        || {
                            let at = (p.start_byte() + 1).min(p.end_byte());
                            at..at
                        },
                        |v| v.byte_range(),
                    ),
            ),
            _ => None,
        })
        .unwrap_or(attr.end_byte()..attr.end_byte());
    Some((name, value))
}

/// The tag that opens an element (`start_tag` or `self_closing_tag`).
fn opening_tag(element: Node) -> Option<Node> {
    children(element)
        .into_iter()
        .find(|c| matches!(c.kind(), "start_tag" | "self_closing_tag"))
}

fn is_element(kind: &str) -> bool {
    matches!(kind, "element" | "script_element" | "style_element")
}

/// The words of an attribute value separated by white space, with their bytes in the file.
fn words(text: &str, value: ByteRange<usize>) -> Vec<(ByteRange<usize>, &str)> {
    let Some(content) = text.get(value.clone()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in content.char_indices() {
        match (c.is_ascii_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((value.start + s..value.start + i, &content[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((value.start + s..value.end, &content[s..]));
    }
    out
}

/// `text` with runs of white space folded to one space, cut after [`DETAIL_LIMIT`] characters.
fn short(text: &str) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() > DETAIL_LIMIT {
        let mut cut: String = folded.chars().take(DETAIL_LIMIT).collect();
        cut.push('…');
        cut
    } else {
        folded
    }
}

/// The elements of the HTML document `text`, nested as in the document and in its order
/// (behaviour 31): each named by its tag with `#id` and `.class` for every class, with the
/// `href` of a `link` or `a` and the `src` of an `img` or `script` as detail. Text, comments and
/// the document type are not listed; at most [`MAX_ELEMENTS`] elements are.
pub fn structure(text: &str) -> Vec<Symbol> {
    let Some(tree) = parse(LanguageId::Html, text) else {
        return Vec::new();
    };
    let lines = Lines::new(text);
    let mut out = Vec::new();
    // Depth first, in document order, without recursion: a file may nest as deep as it likes.
    let mut stack: Vec<(Node, usize)> = vec![(tree.root_node(), 0)];
    while let Some((node, depth)) = stack.pop() {
        if out.len() >= MAX_ELEMENTS {
            break;
        }
        let mut below = depth;
        if is_element(node.kind())
            && let Some(tag) = opening_tag(node)
            && let Some(name) = children(tag).into_iter().find(|c| c.kind() == "tag_name")
        {
            let tag_name = text.get(name.byte_range()).unwrap_or_default();
            let attrs = attributes(tag, text);
            let value = |wanted: &str| {
                attrs
                    .iter()
                    .find(|a| a.name == wanted)
                    .and_then(|a| text.get(a.value.clone()))
            };
            let mut label = tag_name.to_owned();
            if let Some(id) = value("id").map(str::trim).filter(|id| !id.is_empty()) {
                label.push('#');
                label.push_str(id);
            }
            if let Some(class) = attrs.iter().find(|a| a.name == "class") {
                for (_, word) in words(text, class.value.clone()) {
                    label.push('.');
                    label.push_str(word);
                }
            }
            let detail = match tag_name.to_ascii_lowercase().as_str() {
                "link" | "a" => value("href"),
                "img" | "script" => value("src"),
                _ => None,
            }
            .map(short)
            .filter(|d| !d.is_empty());
            let (line, character) = lines.position(name.start_byte());
            let (end_line, end_character) = lines.position(node.end_byte());
            out.push(Symbol {
                name: label,
                kind: ELEMENT_KIND.to_owned(),
                detail,
                line,
                character,
                depth,
                end_line,
                end_character,
                ..Symbol::default()
            });
            below = depth + 1;
        }
        // Tags hold no elements; what a parse error wraps keeps the depth of the error.
        if !matches!(node.kind(), "start_tag" | "self_closing_tag" | "end_tag") {
            stack.extend(children(node).into_iter().rev().map(|c| (c, below)));
        }
    }
    out
}

/// The class and id names in the attributes of the HTML document `text`, and the `<link>`ed
/// stylesheet addresses and `<style>` blocks, in document order.
#[derive(Debug, Default)]
struct HtmlFacts {
    /// Every word of a `class` attribute and every `id` value.
    names: Vec<Named>,
    /// The `href` of every `<link rel="stylesheet">`.
    links: Vec<String>,
    /// The content bytes of every `<style>` element with its stylesheet language.
    styles: Vec<(ByteRange<usize>, LanguageId)>,
}

fn html_facts(text: &str, tree: &Tree) -> HtmlFacts {
    let lines = Lines::new(text);
    let mut facts = HtmlFacts::default();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "start_tag" | "self_closing_tag") {
            let tag_name = children(node)
                .into_iter()
                .find(|c| c.kind() == "tag_name")
                .and_then(|n| text.get(n.byte_range()))
                .unwrap_or_default()
                .to_ascii_lowercase();
            let attrs = attributes(node, text);
            let value = |wanted: &str| {
                attrs
                    .iter()
                    .find(|a| a.name == wanted)
                    .and_then(|a| text.get(a.value.clone()))
            };
            for a in &attrs {
                match a.name.as_str() {
                    "class" => {
                        for (bytes, word) in words(text, a.value.clone()) {
                            facts.names.push(Named {
                                kind: NameKind::Class,
                                name: word.to_owned(),
                                range: lines.range(bytes.clone()),
                                written: lines.range(bytes.clone()),
                                bytes,
                            });
                        }
                    }
                    "id" => {
                        if let Some((bytes, word)) = words(text, a.value.clone()).into_iter().next()
                        {
                            facts.names.push(Named {
                                kind: NameKind::Id,
                                name: word.to_owned(),
                                range: lines.range(bytes.clone()),
                                written: lines.range(bytes.clone()),
                                bytes,
                            });
                        }
                    }
                    _ => {}
                }
            }
            if tag_name == "link"
                && value("rel").is_some_and(|rel| {
                    rel.split_ascii_whitespace()
                        .any(|r| r.eq_ignore_ascii_case("stylesheet"))
                })
                && let Some(href) = value("href")
            {
                facts.links.push(href.trim().to_owned());
            }
            continue;
        }
        if node.kind() == "style_element" {
            let parts = children(node);
            let lang = parts
                .iter()
                .find(|c| c.kind() == "start_tag")
                .and_then(|tag| {
                    let attrs = attributes(*tag, text);
                    attrs
                        .iter()
                        .find(|a| a.name == "lang")
                        .and_then(|a| text.get(a.value.clone()))
                        .map(str::to_ascii_lowercase)
                });
            let language = match lang.as_deref() {
                Some("scss") => Some(LanguageId::Scss),
                Some("less") => Some(LanguageId::Less),
                None | Some("css") => Some(LanguageId::Css),
                Some(_) => None,
            };
            if let (Some(language), Some(raw)) =
                (language, parts.iter().find(|c| c.kind() == "raw_text"))
            {
                facts.styles.push((raw.byte_range(), language));
            }
        }
        stack.extend(children(node).into_iter().rev());
    }
    facts
}

/// A selector name as it is compared: without the backslash before an escaped character that is
/// no hexadecimal digit (`a\:b` names `a:b`). Escapes by code point stay as written.
fn unescaped(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(|n| !n.is_ascii_hexdigit()) {
            continue;
        }
        out.push(c);
    }
    out
}

/// The class and id selectors of the stylesheet `text`, in document order; each range starts at
/// the `.` or `#` and ends after the name.
fn selectors(language: LanguageId, text: &str) -> Vec<Named> {
    let Some(tree) = parse(language, text) else {
        return Vec::new();
    };
    let lines = Lines::new(text);
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let wanted = match node.kind() {
            "class_selector" => Some((NameKind::Class, "class_name", '.')),
            "id_selector" => Some((NameKind::Id, "id_name", '#')),
            _ => None,
        };
        if let Some((kind, name_kind, mark)) = wanted
            && let Some(name) = children(node).into_iter().find(|c| c.kind() == name_kind)
            && let Some(written) = text.get(name.byte_range())
        {
            // The mark directly before the name belongs to the place.
            let start = name.start_byte();
            let start = if text[..start].ends_with(mark) {
                start - mark.len_utf8()
            } else {
                start
            };
            out.push(Named {
                kind,
                name: unescaped(written),
                range: lines.range(start..name.end_byte()),
                written: lines.range(name.byte_range()),
                bytes: name.byte_range(),
            });
        }
        stack.extend(children(node).into_iter().rev());
    }
    // A compound selector holds its first class innermost (`.button.primary`).
    out.sort_by_key(|n| n.bytes.start);
    out
}

/// The class and id selectors of the `<style>` block at `bytes` of the HTML `text` in
/// `language`, with their places in the file.
fn block_selectors(
    text: &str,
    lines: &Lines,
    bytes: ByteRange<usize>,
    language: LanguageId,
) -> Vec<Named> {
    let Some(block) = text.get(bytes.clone()) else {
        return Vec::new();
    };
    let (line, character) = lines.position(bytes.start);
    selectors(language, block)
        .into_iter()
        .map(|n| n.moved(bytes.start, line, character))
        .collect()
}

/// Modification time and size of a file when it was read.
type Stamp = (u128, u64);

/// The names of files by path, each with the stamp it was read at.
type Remembered = HashMap<PathBuf, (Stamp, Arc<Vec<Named>>)>;

/// The names of the files read so far (behaviour 32).
static REMEMBERED: Mutex<Option<Remembered>> = Mutex::new(None);

/// `path` without `.` and with `..` resolved by name only; `None` when it would leave its start.
fn normalized(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// `file` relative to `root`, as given or with both made canonical.
fn relative_to(root: &Path, file: &Path) -> Option<PathBuf> {
    if let Ok(rel) = file.strip_prefix(root) {
        return normalized(rel);
    }
    let root = root.canonicalize().ok()?;
    let file = file.canonicalize().ok()?;
    normalized(file.strip_prefix(root).ok()?)
}

/// `%xx` sequences of an address turned into the bytes they stand for; `None` when the result is
/// not UTF-8.
fn percent_decoded(address: &str) -> Option<String> {
    let bytes = address.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The file below the project folder that the address `href` of the HTML file `html` (relative to
/// the project folder) names, relative to the project folder (behaviour 28): an address with a
/// scheme (`https:`, `data:`), with a host (`//cdn…`) or with a backslash names none, a query and
/// a fragment are cut off, an address starting with `/` counts from the project folder, and one
/// that leaves the project folder names none.
fn local_target(html: &Path, href: &str) -> Option<PathBuf> {
    let href = href.trim();
    let path = href.split(['#', '?']).next().unwrap_or_default();
    if path.is_empty() || path.starts_with("//") || path.contains('\\') {
        return None;
    }
    // A scheme: letters, digits, `+`, `-`, `.` before the first `:` that precedes any `/`.
    if let Some(colon) = path.find(':')
        && !path[..colon].contains('/')
    {
        return None;
    }
    let path = percent_decoded(path)?;
    if path.contains('\0') {
        return None;
    }
    match path.strip_prefix('/') {
        Some(from_root) => normalized(Path::new(from_root)),
        None => normalized(&html.parent().unwrap_or(Path::new("")).join(path)),
    }
}

/// The project's own file `relative`, opened through a handle that follows no symbolic link,
/// with its stamp; `None` for a file that is not the project's own, is larger than
/// [`MAX_FILE_BYTES`] or cannot be opened (behaviour 32).
fn open_own(own: &OwnSources, relative: &Path) -> Option<(std::fs::File, Stamp)> {
    if own.not_own(relative).is_some() {
        return None;
    }
    let (file, meta) = open_regular(&own.root().join(relative)).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some((file, (modified, meta.len())))
}

/// The text behind `file` when it is UTF-8 and within [`MAX_FILE_BYTES`].
fn read_bounded(file: std::fs::File) -> Option<String> {
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The names of the project's own file `relative` (the selectors of a stylesheet, the class and
/// id attributes of an HTML file): remembered while the file's modification time and size stay
/// the same, read and parsed again when they change (behaviour 32).
fn names_of(own: &OwnSources, relative: &Path) -> Option<Arc<Vec<Named>>> {
    let (file, stamp) = open_own(own, relative)?;
    let key = own.root().join(relative);
    if let Ok(guard) = REMEMBERED.lock()
        && let Some((known, names)) = guard.as_ref().and_then(|m| m.get(&key))
        && *known == stamp
    {
        return Some(names.clone());
    }
    let text = read_bounded(file)?;
    let names = match LanguageId::detect(relative)? {
        LanguageId::Html => {
            let tree = parse(LanguageId::Html, &text)?;
            html_facts(&text, &tree).names
        }
        language @ (LanguageId::Css | LanguageId::Scss | LanguageId::Less) => {
            selectors(language, &text)
        }
        _ => return None,
    };
    let names = Arc::new(names);
    if let Ok(mut guard) = REMEMBERED.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() >= MAX_REMEMBERED {
            map.clear();
        }
        map.insert(key, (stamp, names.clone()));
    }
    Some(names)
}

/// The places of `names` that name `wanted` exactly, in the file `path`: each the name without
/// its mark (v22 behaviour 28, OBS-0069 G3).
fn places(path: &Path, names: &[Named], kind: NameKind, wanted: &str) -> Vec<Location> {
    names
        .iter()
        .filter(|n| n.kind == kind && n.name == wanted)
        .map(|n| Location {
            path: path.to_path_buf(),
            range: n.written.clone(),
        })
        .collect()
}

/// `range` of a block that starts at `line`/`character` of its file, as a range of the file.
fn shifted(range: &Range, line: u64, character: u64) -> Range {
    let shift = |l: u64, c: u64| {
        if l == 0 {
            (line, character + c)
        } else {
            (line + l, c)
        }
    };
    let (start_line, start_character) = shift(range.start_line, range.start_character);
    let (end_line, end_character) = shift(range.end_line, range.end_character);
    Range {
        start_line,
        start_character,
        end_line,
        end_character,
    }
}

/// Where names of [`each_stylesheet`] come from.
enum Sheet<'a> {
    /// A stylesheet of the project, relative to the project folder.
    File(&'a Path),
    /// The `<style>` blocks of the HTML file itself.
    Blocks,
}

/// Visits the selectors of the stylesheets that count for the HTML file `file` with content
/// `text` (behaviour 28): first the stylesheets it links, in the order of the links, then its
/// `<style>` blocks, then the project's other CSS, SCSS and Less files by path; at most
/// [`MAX_STYLESHEETS`] files are read (behaviour 32).
fn each_stylesheet(
    root: &Path,
    file: &Path,
    text: &str,
    facts: &HtmlFacts,
    mut visit: impl FnMut(Sheet, &[Named]),
) {
    let own = OwnSources::new(root);
    let html = relative_to(root, file);
    let mut read: Vec<PathBuf> = Vec::new();
    if let Some(html) = &html {
        for href in &facts.links {
            let Some(target) = local_target(html, href) else {
                continue;
            };
            if stylesheet_language(&target).is_none() || read.contains(&target) {
                continue;
            }
            if read.len() >= MAX_STYLESHEETS {
                break;
            }
            if let Some(names) = names_of(&own, &target) {
                visit(Sheet::File(&target), &names);
            }
            read.push(target);
        }
    }
    let lines = Lines::new(text);
    let blocks: Vec<Named> = facts
        .styles
        .iter()
        .flat_map(|(bytes, language)| block_selectors(text, &lines, bytes.clone(), *language))
        .collect();
    visit(Sheet::Blocks, &blocks);
    for other in own.files() {
        if read.len() >= MAX_STYLESHEETS {
            break;
        }
        if stylesheet_language(&other).is_none() || read.contains(&other) {
            continue;
        }
        if let Some(names) = names_of(&own, &other) {
            visit(Sheet::File(&other), &names);
        }
        read.push(other);
    }
}

/// The rules that define the class or id `wanted` for the HTML file `file` with content `text`
/// (behaviour 28), in the order of [`each_stylesheet`].
fn rules(
    root: &Path,
    file: &Path,
    text: &str,
    facts: &HtmlFacts,
    kind: NameKind,
    wanted: &str,
) -> Vec<Location> {
    let mut out = Vec::new();
    each_stylesheet(root, file, text, facts, |sheet, names| {
        let path = match sheet {
            Sheet::File(relative) => root.join(relative),
            Sheet::Blocks => file.to_path_buf(),
        };
        out.extend(places(&path, names, kind, wanted));
    });
    out
}

/// The elements with the id `wanted` that the jump link `href` of the HTML file `file` leads to
/// (behaviour 29): in the file itself for `#name`, in another HTML file of the project's own for
/// `other.html#name`.
fn fragment_targets(
    root: &Path,
    file: &Path,
    facts: &HtmlFacts,
    address: &str,
    wanted: &str,
) -> Vec<Location> {
    if address.is_empty() {
        return places(file, &facts.names, NameKind::Id, wanted);
    }
    let Some(target) = relative_to(root, file).and_then(|html| local_target(&html, address)) else {
        return Vec::new();
    };
    if !is_html(&target) {
        return Vec::new();
    }
    let own = OwnSources::new(root);
    names_of(&own, &target)
        .map(|names| places(&root.join(&target), &names, NameKind::Id, wanted))
        .unwrap_or_default()
}

/// The attribute whose value holds byte `at` of the HTML document `tree`: its name node and the
/// bytes of its value. Directly after a value without quotes the caret stands on what follows, so
/// the place before it is looked at as well.
fn attribute_at(tree: &Tree, at: usize) -> Option<(Node<'_>, ByteRange<usize>)> {
    let around = |at: usize| {
        let mut node = tree.root_node().descendant_for_byte_range(at, at)?;
        while node.kind() != "attribute" {
            node = node.parent()?;
        }
        attribute(node).filter(|(_, value)| value.start <= at && at <= value.end)
    };
    around(at).or_else(|| {
        let before = at.checked_sub(1)?;
        around(before).filter(|(_, value)| value.end == at)
    })
}

/// The value of a `class` or `id` attribute that holds byte `at` of the HTML document: what it
/// names and its bytes.
fn name_value_at(tree: &Tree, text: &str, at: usize) -> Option<(NameKind, ByteRange<usize>)> {
    let (name, value) = attribute_at(tree, at)?;
    match text.get(name.byte_range())?.to_ascii_lowercase().as_str() {
        "class" => Some((NameKind::Class, value)),
        "id" => Some((NameKind::Id, value)),
        _ => None,
    }
}

/// What Go to Declaration finds at 0-based `line` and `character` (in chars) of the HTML file
/// `file` with content `text` below the project folder `root`, when the place is one Ferris IDEA
/// answers itself (behaviours 28 and 29): a word of a `class` attribute or the value of an `id`
/// attribute leads to the rules that define it, the fragment of an `href` to the element with
/// that id. `None` for every other place: the language server is asked. `Some` of nothing when
/// the name is defined nowhere.
pub fn declarations(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
) -> Option<Vec<Location>> {
    let tree = parse(LanguageId::Html, text)?;
    let lines = Lines::new(text);
    let at = lines.offset(line, character)?;
    let (name, value) = attribute_at(&tree, at)?;
    let name = text.get(name.byte_range())?.to_ascii_lowercase();
    let facts = html_facts(text, &tree);
    match name.as_str() {
        "class" | "id" => {
            let kind = if name == "class" {
                NameKind::Class
            } else {
                NameKind::Id
            };
            let word = words(text, value)
                .into_iter()
                .find(|(bytes, _)| bytes.start <= at && at <= bytes.end);
            Some(match word {
                Some((_, word)) => rules(root, file, text, &facts, kind, word),
                None => Vec::new(),
            })
        }
        "href" => {
            let content = text.get(value.clone())?;
            let hash = content.find('#')?;
            if at < value.start + hash {
                return None;
            }
            let wanted = percent_decoded(&content[hash + 1..])?;
            if wanted.is_empty() {
                return Some(Vec::new());
            }
            Some(fragment_targets(
                root,
                file,
                &facts,
                content[..hash].trim(),
                &wanted,
            ))
        }
        _ => None,
    }
}

/// The uses in the project's own HTML files of the class or id selector at 0-based `line` and
/// `character` (in chars) of the stylesheet `file` with content `text` (behaviour 30): every
/// word of a `class` attribute, or every `id` value, that equals the name, sorted by path and
/// place. `None` when the place is no class or id selector or the file no stylesheet.
pub fn selector_uses(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
) -> Option<Vec<Location>> {
    let language = stylesheet_language(file)?;
    let wanted = selectors(language, text)
        .into_iter()
        .find(|n| n.holds((line, character)))?;
    Some(html_uses(root, wanted.kind, &wanted.name))
}

/// Every word of a `class` attribute (or every `id` value) of the project's own HTML files that
/// equals `name`, sorted by path and place.
fn html_uses(root: &Path, kind: NameKind, name: &str) -> Vec<Location> {
    let own = OwnSources::new(root);
    let mut out = Vec::new();
    let mut read = 0;
    for html in own.files() {
        if !is_html(&html) {
            continue;
        }
        if read >= MAX_HTML_FILES {
            break;
        }
        read += 1;
        if let Some(names) = names_of(&own, &html) {
            out.extend(places(&root.join(&html), &names, kind, name));
        }
    }
    out
}

/// A class or id name in a `class` or `id` attribute of the HTML file `file` with content `text`,
/// at 0-based `line` and `character` (`SPEC-0042` v26 behaviours 46 and 47, OBS-0087 L5, L6): the
/// name, whether it is a class, the rules of the project's stylesheets that define it (as Go to
/// Declaration finds them) and its uses in the project's own HTML files. `None` elsewhere.
pub fn html_name_uses(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
) -> Option<(String, bool, Vec<Location>, Vec<Location>)> {
    if !is_html(file) {
        return None;
    }
    let tree = parse(LanguageId::Html, text)?;
    let at = Lines::new(text).offset(line, character)?;
    let (kind, value) = name_value_at(&tree, text, at)?;
    let word = words(text, value)
        .into_iter()
        .find(|(bytes, _)| bytes.start <= at && at <= bytes.end)?
        .1
        .to_owned();
    let rules = declarations(root, file, text, line, character).unwrap_or_default();
    let uses = html_uses(root, kind, &word);
    Some((word, kind == NameKind::Class, rules, uses))
}

/// The project's own stylesheets that the HTML file `file` with content `text` links with
/// `<link rel="stylesheet">`, relative to the project folder, in the order of the links and each
/// once (`SPEC-0071` v10 behaviour 17). Addresses outside the project name none (behaviour 28).
pub fn linked_stylesheets(root: &Path, file: &Path, text: &str) -> Vec<PathBuf> {
    let (Some(html), Some(tree)) = (relative_to(root, file), parse(LanguageId::Html, text)) else {
        return Vec::new();
    };
    let own = OwnSources::new(root);
    let mut out: Vec<PathBuf> = Vec::new();
    for href in &html_facts(text, &tree).links {
        let Some(target) = local_target(&html, href) else {
            continue;
        };
        if stylesheet_language(&target).is_some()
            && !out.contains(&target)
            && open_own(&own, &target).is_some()
        {
            out.push(target);
        }
    }
    out
}

/// The project's own HTML files that link the stylesheet `stylesheet`, relative to the project
/// folder and sorted by path (`SPEC-0071` v10 behaviour 17); at most [`MAX_HTML_FILES`] HTML
/// files are read (behaviour 32).
pub fn pages_linking(root: &Path, stylesheet: &Path) -> Vec<PathBuf> {
    let Some(wanted) = relative_to(root, stylesheet).filter(|w| stylesheet_language(w).is_some())
    else {
        return Vec::new();
    };
    let own = OwnSources::new(root);
    let mut out = Vec::new();
    let mut read = 0;
    for html in own.files() {
        if !is_html(&html) {
            continue;
        }
        if read >= MAX_HTML_FILES {
            break;
        }
        read += 1;
        let links = open_own(&own, &html)
            .and_then(|(file, _)| read_bounded(file))
            .and_then(|text| {
                let tree = parse(LanguageId::Html, &text)?;
                Some(html_facts(&text, &tree).links)
            })
            .unwrap_or_default();
        if links
            .iter()
            .any(|href| local_target(&html, href).as_ref() == Some(&wanted))
        {
            out.push(html);
        }
    }
    out
}

/// The class or id name at 0-based `line` and `character` (in chars) of `file` with content
/// `text` that Ferris IDEA renames itself (`SPEC-0042` v22 behaviour 38): a class or id selector of
/// a stylesheet — the caret on the mark, in the name or directly after it — or of a `<style>`
/// block of an HTML file, a word of a `class` value or an `id` value of an HTML file.
fn named_at(file: &Path, text: &str, line: u64, character: u64) -> Option<(NameKind, String)> {
    if let Some(language) = stylesheet_language(file) {
        return selectors(language, text)
            .into_iter()
            .find(|n| n.holds((line, character)))
            .map(|n| (n.kind, n.name));
    }
    if !is_html(file) {
        return None;
    }
    let tree = parse(LanguageId::Html, text)?;
    let lines = Lines::new(text);
    let at = lines.offset(line, character)?;
    if let Some((kind, value)) = name_value_at(&tree, text, at) {
        return words(text, value)
            .into_iter()
            .find(|(bytes, _)| bytes.start <= at && at <= bytes.end)
            .map(|(_, word)| (kind, word.to_owned()));
    }
    html_facts(text, &tree)
        .styles
        .iter()
        .flat_map(|(bytes, language)| block_selectors(text, &lines, bytes.clone(), *language))
        .find(|n| n.holds((line, character)))
        .map(|n| (n.kind, n.name))
}

/// The class or id name a rename at 0-based `line` and `character` of `file` with content `text`
/// starts with, whole and without its mark (`SPEC-0042` v22 behaviour 38: the rename popup);
/// `None` where Ferris IDEA does not rename itself.
pub fn name_at(file: &Path, text: &str, line: u64, character: u64) -> Option<String> {
    named_at(file, text, line, character).map(|(_, name)| name)
}

/// A non-ASCII character CSS Syntax Level 3 allows in an identifier, without white space and the
/// direction mark those ranges hold (`SPEC-0042` v22 behaviour 38b).
fn non_ascii_identifier(c: char) -> bool {
    matches!(
        c as u32,
        0xB7 | 0xC0..=0xD6
            | 0xD8..=0xF6
            | 0xF8..=0x37D
            | 0x37F..=0x1FFF
            | 0x200C
            | 0x200D
            | 0x203F
            | 0x2040
            | 0x2070..=0x218F
            | 0x2C00..=0x2FEF
            | 0x3001..=0xD7FF
            | 0xF900..=0xFDCF
            | 0xFDF0..=0xFFFD
            | 0x10000..
    ) && !c.is_whitespace()
        && c != '\u{061C}'
}

/// Whether `name` is a CSS identifier written without escapes (`SPEC-0042` v22 behaviour 38b): an
/// optional `-`, then a letter, `_`, a non-ASCII identifier character or a second `-`, then
/// letters, digits, `_`, `-` and non-ASCII identifier characters.
pub fn is_css_identifier(name: &str) -> bool {
    let starts = |c: char| c.is_ascii_alphabetic() || c == '_' || non_ascii_identifier(c);
    let mut chars = name.chars();
    let first_ok = match chars.next() {
        Some('-') => chars.next().is_some_and(|c| c == '-' || starts(c)),
        Some(c) => starts(c),
        None => false,
    };
    first_ok && chars.all(|c| starts(c) || c.is_ascii_digit() || c == '-')
}

/// The new name of a class or id as typed into the rename: a `.` before a class's name and a `#`
/// before an id's is the mark and is left off; what remains must be a CSS identifier
/// (`SPEC-0042` v22 behaviour 38b), else the message why it is refused.
fn new_name(kind: NameKind, typed: &str) -> Result<String, String> {
    let (mark, what) = match kind {
        NameKind::Class => ('.', "class name"),
        NameKind::Id => ('#', "id"),
    };
    let name = typed.strip_prefix(mark).unwrap_or(typed);
    if is_css_identifier(name) {
        Ok(name.to_owned())
    } else {
        // Debug form: control and direction characters are shown escaped, never as they are.
        Err(format!(
            "{name:?} is not a valid CSS {what}: it must start with a letter, \"_\" or \"-\" (not \
             \"-\" and a digit) and hold only letters, digits, \"_\" and \"-\". Nothing was renamed."
        ))
    }
}

/// The text of the project's own file `relative` for a rename (`SPEC-0042` v22 behaviour 38c):
/// the text its editor shows when the planning runs with the window's texts (`SPEC-0220` v4
/// behaviour 13), else the saved file within the bounds of behaviour 32.
fn text_for(own: &OwnSources, relative: &Path) -> Option<String> {
    if let Some(shown) = crate::shown::text_of(&own.root().join(relative)) {
        return Some(shown);
    }
    let (file, _) = open_own(own, relative)?;
    read_bounded(file)
}

/// The edits that rename the class or id `old` to `new` in `file` with content `text`
/// (`SPEC-0042` v22 behaviour 38a): its selectors in a stylesheet; the words of `class` values or
/// the `id` values and the selectors of the `<style>` blocks in an HTML file. Each edit replaces
/// the name alone, never its mark.
fn renamed_in(file: &Path, text: &str, kind: NameKind, old: &str, new: &str) -> Vec<Edit> {
    let named = if let Some(language) = stylesheet_language(file) {
        selectors(language, text)
    } else if let Some(tree) = is_html(file)
        .then(|| parse(LanguageId::Html, text))
        .flatten()
    {
        let lines = Lines::new(text);
        let facts = html_facts(text, &tree);
        let mut all = facts.names;
        for (bytes, language) in &facts.styles {
            all.extend(block_selectors(text, &lines, bytes.clone(), *language));
        }
        all
    } else {
        return Vec::new();
    };
    let mut edits: Vec<Edit> = named
        .into_iter()
        .filter(|n| n.kind == kind && n.name == old)
        .filter_map(|n| {
            Some(Edit {
                old_text: text.get(n.bytes.clone())?.to_owned(),
                new_text: new.to_owned(),
                range: n.written,
            })
        })
        .collect();
    edits.sort_by_key(|e| (e.range.start_line, e.range.start_character));
    edits
}

/// Plans renaming the class or id at 0-based `line` and `character` (in chars) of `file` with
/// content `text` below the project folder `root` to `new_name` (`SPEC-0042` v22 behaviour 38):
/// every selector of that name in the project's own stylesheets and `<style>` blocks and every
/// use in its HTML files, the marks kept. `None` where Ferris IDEA does not rename itself (the
/// language server is asked); `Some(Err)` with the reason when the rename is refused: a new name
/// that is no CSS identifier, a project beyond the bounds, a file that is not the project's own.
pub fn rename_plan(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
    new_name: &str,
) -> Option<Result<RenamePlan, String>> {
    let (kind, old) = named_at(file, text, line, character)?;
    Some(plan_rename(root, file, text, kind, &old, new_name))
}

fn plan_rename(
    root: &Path,
    file: &Path,
    text: &str,
    kind: NameKind,
    old: &str,
    typed: &str,
) -> Result<RenamePlan, String> {
    let new = new_name(kind, typed)?;
    let own = OwnSources::new(root);
    let this = relative_to(own.root(), file)
        .filter(|relative| own.not_own(relative).is_none())
        .ok_or_else(|| {
            format!(
                "{} is not among the project's own sources; nothing was renamed.",
                file.display()
            )
        })?;
    let mut plan = RenamePlan {
        old_name: old.to_owned(),
        new_name: new.clone(),
        files: Vec::new(),
    };
    if new == old {
        return Ok(plan);
    }
    let mut todo: Vec<PathBuf> = own
        .files()
        .into_iter()
        .filter(|p| is_stylesheet(p) || is_html(p))
        .collect();
    let sheets = todo.iter().filter(|p| is_stylesheet(p)).count();
    let pages = todo.len() - sheets;
    // A rename never leaves places out without saying so (behaviour 38d).
    for (count, bound, what) in [
        (sheets, MAX_STYLESHEETS, "stylesheets"),
        (pages, MAX_HTML_FILES, "HTML files"),
    ] {
        if count > bound {
            return Err(format!(
                "The project holds {count} {what}, more than the {bound} Ferris IDEA reads for one \
                 rename; nothing was renamed."
            ));
        }
    }
    if !todo.contains(&this) {
        todo.push(this.clone());
    }
    for relative in todo {
        let content = if relative == this {
            Some(text.to_owned())
        } else {
            text_for(&own, &relative)
        };
        // Without the name or an escape in it, the file has no place to change.
        let Some(content) = content.filter(|t| t.contains(old) || t.contains('\\')) else {
            continue;
        };
        let edits = renamed_in(&relative, &content, kind, old, &new);
        if !edits.is_empty() {
            plan.files.push(FileEdits {
                path: own.root().join(&relative),
                edits,
            });
        }
    }
    plan.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(plan)
}

/// The bytes typed of the word of a `class` or `id` value at byte `at` of the HTML document: from
/// the start of the word up to `at`, empty between words and in an empty value (`SPEC-0042` v22
/// behaviour 39).
fn typed_in_value(tree: &Tree, text: &str, at: usize) -> Option<(NameKind, ByteRange<usize>)> {
    let (kind, value) = name_value_at(tree, text, at)?;
    let start = words(text, value)
        .into_iter()
        .find(|(bytes, _)| bytes.start <= at && at <= bytes.end)
        .map_or(at, |(bytes, _)| bytes.start);
    Some((kind, start..at))
}

/// The character where the typed word of a `class` or `id` value starts, on the caret's line,
/// when the caret at 0-based `line` and `character` of the HTML `text` stands in such a value
/// (`SPEC-0042` v22 behaviour 39, `SPEC-0072` v15 behaviour 52); `None` everywhere else.
pub fn value_word(text: &str, line: u64, character: u64) -> Option<u64> {
    let tree = parse(LanguageId::Html, text)?;
    let lines = Lines::new(text);
    let at = lines.offset(line, character)?;
    let (_, typed) = typed_in_value(&tree, text, at)?;
    let (start_line, start) = lines.position(typed.start);
    (start_line == line).then_some(start)
}

/// The suggestions in a `class` or `id` value of the HTML file `file` with content `text` at
/// 0-based `line` and `character` (`SPEC-0042` v22 behaviour 39): the class names, for an `id`
/// the ids, of the stylesheets in the order of behaviour 28, each once, with the file and 1-based
/// line where it is first defined, at most `limit`. `None` outside such a value: the language
/// server is asked.
pub fn completions(
    root: &Path,
    file: &Path,
    text: &str,
    line: u64,
    character: u64,
    limit: usize,
) -> Option<Vec<CompletionItem>> {
    if !is_html(file) {
        return None;
    }
    let tree = parse(LanguageId::Html, text)?;
    let lines = Lines::new(text);
    let at = lines.offset(line, character)?;
    let (kind, typed) = typed_in_value(&tree, text, at)?;
    let facts = html_facts(text, &tree);
    let shown = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let own_name = relative_to(root, file)
        .map(|p| shown(&p))
        .or_else(|| file.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut offered: Vec<(String, String)> = Vec::new();
    each_stylesheet(root, file, text, &facts, |sheet, names| {
        let place = match sheet {
            Sheet::File(relative) => shown(relative),
            Sheet::Blocks => own_name.clone(),
        };
        for n in names.iter().filter(|n| n.kind == kind) {
            if offered.len() < limit && seen.insert(n.name.clone()) {
                offered.push((
                    n.name.clone(),
                    format!("{place}:{}", n.written.start_line + 1),
                ));
            }
        }
    });
    let (start_line, start_character) = lines.position(typed.start);
    let replaced = Range {
        start_line,
        start_character,
        end_line: line,
        end_character: character,
    };
    let kind = match kind {
        NameKind::Class => "class",
        NameKind::Id => "field",
    };
    Some(
        offered
            .into_iter()
            .enumerate()
            .map(|(i, (name, place))| CompletionItem {
                label: name.clone(),
                insert: name,
                kind: Some(kind.to_owned()),
                description: Some(place),
                sort_text: Some(format!("{i:06}")),
                edit_range: Some(replaced.clone()),
                ..Default::default()
            })
            .collect(),
    )
}

/// The CSS server's list without obsolete values (`SPEC-0042` v26 behaviour 45): items it marks as
/// deprecated (LSP tag 1, `deprecated: true`) and values whose documentation says they were
/// standardized under another name (`flexbox`, `-ms-flexbox`) go; where the list holds the values
/// of `display` (`flow-root`) and no `flow`, `flow` (CSS Display Level 3) comes before `flow-root`.
pub fn clean_css_values(items: &mut Vec<CompletionItem>) {
    let deprecated = |item: &CompletionItem| {
        item.raw.as_ref().is_some_and(|raw| {
            raw["deprecated"].as_bool() == Some(true)
                || raw["tags"]
                    .as_array()
                    .is_some_and(|t| t.iter().any(|t| t.as_u64() == Some(1)))
        })
    };
    let renamed = |item: &CompletionItem| {
        let doc = item.documentation.clone().or_else(|| {
            let raw = item.raw.as_ref()?;
            let d = &raw["documentation"];
            d.as_str()
                .map(str::to_owned)
                .or_else(|| d["value"].as_str().map(str::to_owned))
        });
        doc.is_some_and(|d| d.replace('\\', "").contains("Standardized as '"))
    };
    items.retain(|item| !deprecated(item) && !renamed(item));
    if items.iter().any(|i| i.label == "flow") {
        return;
    }
    if let Some(at) = items.iter().position(|i| i.label == "flow-root") {
        let root = &items[at];
        let flow = CompletionItem {
            label: "flow".into(),
            insert: "flow".into(),
            kind: root.kind.clone(),
            sort_text: root.sort_text.clone(),
            documentation: Some(
                "The element lays out its contents using flow layout (block-and-inline layout)."
                    .into(),
            ),
            ..Default::default()
        };
        items.insert(at, flow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0071/AK-13 (v10, behaviour 17): the stylesheets a page links that are files of the
    /// project, each once and in link order, and the pages that link a stylesheet.
    #[test]
    fn linked_stylesheets_and_the_pages_that_link_one() {
        let root = project("linked");
        std::fs::create_dir_all(root.join("pages")).unwrap();
        std::fs::write(root.join("css/print.scss"), ".a {}\n").unwrap();
        let page = "<html><head>\n<link rel=\"stylesheet\" href=\"../css/print.scss\">\n<link rel=\"stylesheet\" href=\"/css/site.css?v=2\">\n<link rel=\"stylesheet\" href=\"../css/print.scss\">\n<link rel=\"stylesheet\" href=\"https://cdn.example/x.css\">\n<link rel=\"stylesheet\" href=\"../css/missing.css\">\n<link rel=\"icon\" href=\"../css/site.css\">\n</head></html>\n";
        std::fs::write(root.join("pages/about.html"), page).unwrap();
        let shown = |list: Vec<PathBuf>| -> Vec<String> {
            list.iter()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect()
        };
        assert_eq!(
            shown(linked_stylesheets(
                &root,
                &root.join("pages/about.html"),
                page
            )),
            ["css/print.scss", "css/site.css"]
        );
        assert_eq!(
            shown(linked_stylesheets(&root, &root.join("index.html"), INDEX)),
            ["css/site.css"]
        );
        assert_eq!(
            shown(pages_linking(&root, &root.join("css/site.css"))),
            ["index.html", "pages/about.html"]
        );
        assert_eq!(
            shown(pages_linking(&root, &root.join("css/print.scss"))),
            ["pages/about.html"]
        );
        // A file that is no stylesheet, and one outside the project, are linked from nowhere.
        assert!(pages_linking(&root, &root.join("index.html")).is_empty());
        assert!(pages_linking(&root, Path::new("/nowhere/site.css")).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The sample of OBS-0039: `index.html` links `css/site.css`, has a jump link to `#prices`
    /// and elements with classes and ids.
    const INDEX: &str = include_str!("../tests/fixtures/web/index.html");
    const SITE: &str = include_str!("../tests/fixtures/web/css/site.css");

    /// A project like the one of OBS-0039 in a fresh folder: `index.html` linking `css/site.css`.
    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ferris-idea-html-css-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("css")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("index.html"), INDEX).unwrap();
        std::fs::write(root.join("css/site.css"), SITE).unwrap();
        root
    }

    /// 0-based line and char of the first `needle` in `text`, moved `into` chars into it.
    fn at(text: &str, needle: &str, into: u64) -> (u64, u64) {
        let byte = text.find(needle).expect(needle);
        let (line, character) = Lines::new(text).position(byte);
        (line, character + into)
    }

    fn found(root: &Path, list: &[Location]) -> Vec<(String, u64, u64)> {
        list.iter()
            .map(|l| {
                (
                    l.path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    l.range.start_line,
                    l.range.start_character,
                )
            })
            .collect()
    }

    /// SPEC-0042/AK-31 (behaviour 31, OBS-0039 W3): nested like the document, named with id and
    /// classes, `href` and `src` as detail, no text, comment or document type.
    #[test]
    fn structure_is_nested_and_named() {
        let rows: Vec<(usize, String, Option<String>)> = structure(INDEX)
            .into_iter()
            .map(|s| {
                assert_eq!(s.kind, ELEMENT_KIND);
                (s.depth, s.name, s.detail)
            })
            .collect();
        let row =
            |d: usize, n: &str, detail: Option<&str>| (d, n.to_owned(), detail.map(str::to_owned));
        assert_eq!(
            rows,
            [
                row(0, "html", None),
                row(1, "head", None),
                row(2, "meta", None),
                row(2, "title", None),
                row(2, "link", Some("css/site.css")),
                row(1, "body", None),
                row(2, "header#masthead.banner", None),
                row(3, "img", Some("logo.png")),
                row(3, "a", Some("#prices")),
                row(2, "main#prices.card", None),
                row(3, "p", None),
                row(3, "button.button.primary", None),
                row(2, "script", Some("app.js")),
            ]
        );
        let symbols = structure(INDEX);
        let main = symbols
            .iter()
            .find(|s| s.name == "main#prices.card")
            .unwrap();
        // The name starts after `<`; the element ends after `</main>`.
        assert_eq!((main.line, main.character), (12, 3));
        assert_eq!((main.end_line, main.end_character), (15, 9));
        assert!(main.contains(14, 6));
        assert!(structure("").is_empty());
        assert!(structure("just text").is_empty());
    }

    /// Broken markup still lists what can be read, and the bound holds.
    #[test]
    fn structure_of_broken_and_huge_documents() {
        let names: Vec<String> =
            structure("<div class=\"a\"><span id=x>t</div><ul><li>1<li>2</ul></p><p>")
                .into_iter()
                .map(|s| format!("{}{}", "-".repeat(s.depth), s.name))
                .collect();
        assert_eq!(names, ["div.a", "-span#x", "ul", "-li", "-li", "p"]);
        let many = "<i></i>".repeat(MAX_ELEMENTS + 10);
        assert_eq!(structure(&many).len(), MAX_ELEMENTS);
        let deep = format!("{}{}", "<div>".repeat(3000), "</div>".repeat(3000));
        assert_eq!(structure(&deep).last().map(|s| s.depth), Some(2999));
    }

    /// SPEC-0042/AK-29 (behaviour 28, OBS-0039 W1): a class of `index.html` leads to its rules in
    /// the linked `css/site.css`, an id to its `#` rule. SPEC-0042/AK-43 (v22, OBS-0069 G3):
    /// each target is the name after the mark.
    #[test]
    fn class_and_id_lead_to_their_rules() {
        let root = project("rules");
        let file = root.join("index.html");
        let (line, character) = at(INDEX, "class=\"card\"", 8);
        assert_eq!((line, character), (12, 28));
        let card = declarations(&root, &file, INDEX, line, character).unwrap();
        // `card` of `.card` on line 20 (0-based 19) and of `.card:hover`, after the dot.
        assert_eq!(
            found(&root, &card),
            [
                ("css/site.css".into(), 19, 1),
                ("css/site.css".into(), 22, 1)
            ]
        );
        assert_eq!(card[0].range.end_character, 5);
        // A value without quotes, with the caret in it and directly after it.
        let bare = "<link rel=stylesheet href=css/site.css>\n<p class=card>x</p>\n";
        for into in [8, 10] {
            let (l, c) = at(bare, "class=card", into);
            let got = declarations(&root, &root.join("bare.html"), bare, l, c);
            assert_eq!(got.map(|g| g.len()), Some(2), "{into}");
        }
        let (l, c) = at(bare, "class=card", 11);
        assert_eq!(
            declarations(&root, &root.join("bare.html"), bare, l, c),
            None
        );
        // The caret directly after the name, and directly before it, still names it.
        for into in [7, 11] {
            let (l, c) = at(INDEX, "class=\"card\"", into);
            assert_eq!(declarations(&root, &file, INDEX, l, c).unwrap().len(), 2);
        }
        // The second word of a class list.
        let (l, c) = at(INDEX, "button primary", 9);
        assert_eq!(
            found(&root, &declarations(&root, &file, INDEX, l, c).unwrap()),
            [("css/site.css".into(), 12, 8)]
        );
        let (l, c) = at(INDEX, "id=\"masthead\"", 5);
        assert_eq!(
            found(&root, &declarations(&root, &file, INDEX, l, c).unwrap()),
            [("css/site.css".into(), 8, 1)]
        );
        // Not on a class, id or fragment: the language server's question.
        let (l, c) = at(INDEX, "<title>", 2);
        assert_eq!(declarations(&root, &file, INDEX, l, c), None);
        let (l, c) = at(INDEX, "css/site.css", 3);
        assert_eq!(declarations(&root, &file, INDEX, l, c), None);
        let (l, c) = at(INDEX, "class=\"card\"", 2);
        assert_eq!(declarations(&root, &file, INDEX, l, c), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Behaviour 28: linked stylesheets first, then `<style>` blocks, then the project's other
    /// stylesheets; exact and case-sensitive; nothing outside the project's own sources.
    #[test]
    fn several_definitions_linked_first() {
        let root = project("order");
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/pkg.css"), ".card {}\n").unwrap();
        std::fs::write(
            root.join("a-other.scss"),
            ".wrap {\n  .card { top: 0; }\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("b-other.less"),
            ".Card {}\n.card-wide {}\n.card {}\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir_all(root.join("ignored")).unwrap();
        std::fs::write(root.join("ignored/x.css"), ".card {}\n").unwrap();
        let outside = root.parent().unwrap().join(format!(
            "ferris-idea-html-css-outside-{}.css",
            std::process::id()
        ));
        std::fs::write(&outside, ".card {}\n").unwrap();
        let html = format!(
            "<link rel=\"stylesheet\" href=\"https://example.org/x.css\">\n\
             <link rel=\"stylesheet\" href=\"//example.org/x.css\">\n\
             <link rel=\"stylesheet\" href=\"../{}\">\n\
             <link rel=\"stylesheet\" href=\"node_modules/pkg/pkg.css\">\n\
             <link rel=\"icon\" href=\"a-other.scss\">\n\
             <LINK REL=\"alternate StyleSheet\" HREF=\"./css/../css/site.css?v=3#top\">\n\
             <style>\n  p {{ }}\n  .card {{ }} </style>\n\
             <div class=\"card\"></div>\n",
            outside.file_name().unwrap().to_string_lossy()
        );
        let file = root.join("page.html");
        std::fs::write(&file, &html).unwrap();
        let (l, c) = at(&html, "class=\"card\"", 8);
        let got = declarations(&root, &file, &html, l, c).unwrap();
        assert_eq!(
            found(&root, &got),
            [
                ("css/site.css".into(), 19, 1),
                ("css/site.css".into(), 22, 1),
                ("page.html".into(), 8, 3),
                ("a-other.scss".into(), 1, 3),
                ("b-other.less".into(), 2, 1),
            ]
        );
        let _ = std::fs::remove_file(outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Behaviour 32: a stylesheet behind a symbolic link or larger than the bound is not read,
    /// and a changed file is read again.
    #[test]
    fn bounds_links_and_changes() {
        let root = project("bounds");
        let file = root.join("index.html");
        let (l, c) = at(INDEX, "class=\"card\"", 8);
        assert_eq!(declarations(&root, &file, INDEX, l, c).unwrap().len(), 2);
        // Changed on disk: the remembered names are not used.
        std::fs::write(root.join("css/site.css"), "\n\n.card { color: red; }\n").unwrap();
        assert_eq!(
            found(&root, &declarations(&root, &file, INDEX, l, c).unwrap()),
            [("css/site.css".into(), 2, 1)]
        );
        // Too large.
        let big = format!(".card {{}}\n{}", " ".repeat(MAX_FILE_BYTES as usize));
        std::fs::write(root.join("css/site.css"), big).unwrap();
        assert!(declarations(&root, &file, INDEX, l, c).unwrap().is_empty());
        #[cfg(unix)]
        {
            let elsewhere = root.parent().unwrap().join(format!(
                "ferris-idea-html-css-linked-{}.css",
                std::process::id()
            ));
            std::fs::write(&elsewhere, ".card {}\n").unwrap();
            std::fs::remove_file(root.join("css/site.css")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, root.join("css/site.css")).unwrap();
            assert!(declarations(&root, &file, INDEX, l, c).unwrap().is_empty());
            let _ = std::fs::remove_file(elsewhere);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0042/AK-30 (behaviour 29, OBS-0039 W2): `#prices` leads to `id="prices"` of the same
    /// file, `other.html#name` to that file's element.
    #[test]
    fn jump_links_lead_to_the_id() {
        let root = project("fragment");
        let file = root.join("index.html");
        let (line, character) = at(INDEX, "href=\"#prices\"", 9);
        assert_eq!(line, 10);
        let got = declarations(&root, &file, INDEX, line, character).unwrap();
        assert_eq!(found(&root, &got), [("index.html".into(), 12, 12)]);
        assert_eq!(got[0].range.end_character, 18);
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/other.html"),
            "<p id=\"Top\">a</p>\n<p id=\"top\">b</p>\n<p class=\"top\">c</p>\n",
        )
        .unwrap();
        let html = "<a href=\"docs/other.html#top\">x</a>\n<a href=\"#none\">y</a>\n\
                    <a href=\"https://example.org/docs/other.html#top\">z</a>\n\
                    <a href=\"../outside.html#top\">o</a>\n<a href=\"docs/other.html\">p</a>\n";
        let page = root.join("page.html");
        std::fs::write(&page, html).unwrap();
        let ask = |needle: &str, into: u64| {
            let (l, c) = at(html, needle, into);
            declarations(&root, &page, html, l, c)
        };
        // On the fragment: the element of the other file, exactly and case-sensitively.
        assert_eq!(
            found(&root, &ask("other.html#top", 12).unwrap()),
            [("docs/other.html".into(), 1, 7)]
        );
        // On the path before the `#`: not ours.
        assert_eq!(ask("docs/other.html#top", 3), None);
        assert_eq!(ask("\"docs/other.html\"", 5), None);
        assert!(ask("#none", 2).unwrap().is_empty());
        assert!(
            ask("https://example.org/docs/other.html#top", 37)
                .unwrap()
                .is_empty()
        );
        assert!(ask("../outside.html#top", 17).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0042/AK-32 (behaviour 30): a class selector of `css/site.css` lists the `class`
    /// attributes that use it in the project's own HTML files.
    #[test]
    fn selector_lists_its_uses_in_html() {
        let root = project("uses");
        std::fs::write(
            root.join("about.html"),
            "<div class=\"wide card\n  Card card-wide\"></div>\n<p id=\"card\"></p>\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/x.html"), "<p class=\"card\">").unwrap();
        let css = root.join("css/site.css");
        let (l, c) = at(SITE, ".card {", 2);
        let uses = selector_uses(&root, &css, SITE, l, c).unwrap();
        assert_eq!(
            found(&root, &uses),
            [("about.html".into(), 0, 17), ("index.html".into(), 12, 27)]
        );
        // An id selector lists `id` attributes, not classes of the same name.
        let (l, c) = at(SITE, "#prices", 0);
        assert_eq!(
            found(&root, &selector_uses(&root, &css, SITE, l, c).unwrap()),
            [("index.html".into(), 12, 12)]
        );
        // A property or a tag selector is no class or id selector.
        let (l, c) = at(SITE, "margin", 2);
        assert_eq!(selector_uses(&root, &css, SITE, l, c), None);
        assert_eq!(selector_uses(&root, &css, SITE, 0, 1), None);
        assert_eq!(
            selector_uses(&root, &root.join("index.html"), INDEX, 0, 0),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn addresses_and_names() {
        let html = Path::new("pages/index.html");
        let target =
            |href: &str| local_target(html, href).map(|p| p.to_string_lossy().replace('\\', "/"));
        assert_eq!(target("site.css").as_deref(), Some("pages/site.css"));
        assert_eq!(
            target("../css/a%20b.css?x#y").as_deref(),
            Some("css/a b.css")
        );
        assert_eq!(target("/css/site.css").as_deref(), Some("css/site.css"));
        for refused in [
            "",
            "#top",
            "http://example.org/site.css",
            "HTTPS://example.org/site.css",
            "//example.org/site.css",
            "data:text/css,.a{}",
            "file:///etc/passwd",
            "../../etc/passwd",
            "/../x.css",
            "..\\x.css",
            "a%00.css",
        ] {
            assert_eq!(target(refused), None, "{refused}");
        }
        let text = format!("ä€😀\n{}<p id=\"ü\">", "é".repeat(100));
        let lines = Lines::new(&text);
        assert_eq!(lines.position(text.find('<').unwrap()), (1, 100));
        assert_eq!(lines.position(text.len()), (1, 110));
        assert_eq!(lines.offset(1, 100), text.find('<'));
        assert_eq!(lines.offset(0, 99), Some("ä€😀".len()));
        assert_eq!(lines.offset(2, 0), None);
        assert_eq!(unescaped("a\\:b"), "a:b");
        assert_eq!(unescaped("md\\31 x"), "md\\31 x");
        let scss = selectors(LanguageId::Scss, ".a { &.b { } #c & { } }\n");
        let names: Vec<&str> = scss.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    /// The edits of a plan as (file, line, character, old text, new text), the files relative to
    /// the project folder.
    fn edits_of(root: &Path, plan: &RenamePlan) -> Vec<(String, u64, u64, String, String)> {
        plan.files
            .iter()
            .flat_map(|f| {
                let file = f
                    .path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                f.edits.iter().map(move |e| {
                    (
                        file.clone(),
                        e.range.start_line,
                        e.range.start_character,
                        e.old_text.clone(),
                        e.new_text.clone(),
                    )
                })
            })
            .collect()
    }

    /// The text of the project's file `relative` after the plan's edits.
    fn after(root: &Path, plan: &RenamePlan, relative: &str) -> String {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path).unwrap();
        match plan.files.iter().find(|f| f.path == path) {
            Some(f) => crate::rename::apply_edits(&text, &f.edits).unwrap(),
            None => text,
        }
    }

    const MORE_SCSS: &str = ".wrap {\n  .card { top: 0; }\n}\n.card-wide {}\n.Card {}\n#card {}\n";
    const PAGE: &str = "<style>\n  .card { }\n</style>\n<div class=\"a card b\" id=\"card\"></div>\n\
                        <p class=\"card-wide Card\"></p>\n";

    /// SPEC-0042/AK-39 (v22, behaviour 38a; OBS-0069 G2): renaming `.card` keeps the dot and
    /// changes the selectors of every stylesheet and `<style>` block and the `class` words of every
    /// page, from whichever place it starts; nothing else.
    #[test]
    fn renaming_a_class_keeps_its_mark() {
        let root = project("rename");
        std::fs::write(root.join("css/more.scss"), MORE_SCSS).unwrap();
        std::fs::write(root.join("page.html"), PAGE).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/x.html"), "<p class=\"card\">").unwrap();
        let site = root.join("css/site.css");
        let page = root.join("page.html");
        let expected = |new: &str| {
            [
                ("css/more.scss", 1, 3),
                ("css/site.css", 19, 1),
                ("css/site.css", 22, 1),
                ("index.html", 12, 27),
                ("page.html", 1, 3),
                ("page.html", 3, 14),
            ]
            .map(|(f, l, c)| (f.to_owned(), l, c, "card".to_owned(), new.to_owned()))
            .to_vec()
        };
        // From the stylesheet: on the dot, in the name, directly after it.
        let (l, c) = at(SITE, ".card {", 0);
        for into in [0, 2, 5] {
            let plan = rename_plan(&root, &site, SITE, l, c + into, "box")
                .unwrap()
                .unwrap();
            assert_eq!(plan.old_name, "card");
            assert_eq!(plan.new_name, "box");
            assert_eq!(edits_of(&root, &plan), expected("box"), "{into}");
        }
        // From a page: a word of a class list, the `<style>` block, the linked page.
        let (l, c) = at(PAGE, "a card b", 3);
        let from_page = rename_plan(&root, &page, PAGE, l, c, "box")
            .unwrap()
            .unwrap();
        assert_eq!(edits_of(&root, &from_page), expected("box"));
        let (l, c) = at(PAGE, ".card", 0);
        let from_block = rename_plan(&root, &page, PAGE, l, c, ".box")
            .unwrap()
            .unwrap();
        assert_eq!(edits_of(&root, &from_block), expected("box"));
        let (l, c) = at(INDEX, "class=\"card\"", 9);
        let from_index = rename_plan(&root, &root.join("index.html"), INDEX, l, c, "box")
            .unwrap()
            .unwrap();
        assert_eq!(edits_of(&root, &from_index), expected("box"));
        // The texts afterwards.
        let css = after(&root, &from_page, "css/site.css");
        assert!(
            css.contains("\n.box {\n") && css.contains("\n.box:hover {\n"),
            "{css}"
        );
        assert!(!css.contains("card"), "{css}");
        let html = after(&root, &from_page, "page.html");
        assert!(html.contains("  .box { }"), "{html}");
        assert!(html.contains("class=\"a box b\" id=\"card\""), "{html}");
        assert!(html.contains("class=\"card-wide Card\""), "{html}");
        let scss = after(&root, &from_page, "css/more.scss");
        assert_eq!(scss, MORE_SCSS.replacen(".card {", ".box {", 1));
        assert!(after(&root, &from_page, "index.html").contains("class=\"box\""));
        // An id selector and its `id` values; `href="#prices"` keeps its fragment.
        let (l, c) = at(SITE, "#prices", 3);
        let id = rename_plan(&root, &site, SITE, l, c, "#offers")
            .unwrap()
            .unwrap();
        assert_eq!(
            edits_of(&root, &id),
            [("css/site.css", 16, 1), ("index.html", 12, 12)].map(|(f, l, c)| (
                f.to_owned(),
                l,
                c,
                "prices".into(),
                "offers".into()
            ))
        );
        let (l, c) = at(PAGE, "id=\"card\"", 5);
        let card_id = rename_plan(&root, &page, PAGE, l, c, "box")
            .unwrap()
            .unwrap();
        assert_eq!(
            edits_of(&root, &card_id),
            [("css/more.scss", 5, 1), ("page.html", 3, 26)].map(|(f, l, c)| (
                f.to_owned(),
                l,
                c,
                "card".into(),
                "box".into()
            ))
        );
        // The popup's name: whole, without the mark.
        let more = root.join("css/more.scss");
        let (l, c) = at(MORE_SCSS, ".card-wide", 6);
        assert_eq!(
            name_at(&more, MORE_SCSS, l, c).as_deref(),
            Some("card-wide")
        );
        let (l, c) = at(SITE, ".card {", 0);
        assert_eq!(name_at(&site, SITE, l, c).as_deref(), Some("card"));
        // The same name renames nothing.
        let same = rename_plan(&root, &site, SITE, l, c, "card")
            .unwrap()
            .unwrap();
        assert!(same.files.is_empty());
        // Not a class or id: the language server's question.
        let (l, c) = at(SITE, "margin", 2);
        assert_eq!(rename_plan(&root, &site, SITE, l, c, "box"), None);
        assert_eq!(rename_plan(&root, &site, SITE, 0, 1, "box"), None);
        let (l, c) = at(INDEX, "css/site.css", 3);
        assert_eq!(
            rename_plan(&root, &root.join("index.html"), INDEX, l, c, "box"),
            None
        );
        assert_eq!(name_at(&site, SITE, 0, 1), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0042/AK-40 (v22, behaviour 38b): a new name must be a CSS identifier; a leading mark
    /// of the right kind is left off, everything else is refused with the reason.
    #[test]
    fn new_names_must_be_css_identifiers() {
        for good in [
            "box",
            "-box",
            "--box",
            "_b1",
            "card-2",
            "café",
            "日本",
            "a\u{200D}b",
        ] {
            assert_eq!(
                new_name(NameKind::Class, good).as_deref(),
                Ok(good),
                "{good}"
            );
            assert!(is_css_identifier(good), "{good}");
        }
        assert_eq!(new_name(NameKind::Class, ".box").as_deref(), Ok("box"));
        assert_eq!(new_name(NameKind::Id, "#main").as_deref(), Ok("main"));
        let refused = [
            "",
            "1box",
            "-1box",
            "-",
            "a b",
            "a.b",
            "a:b",
            "a\\:b",
            "\"a\"",
            "<a>",
            "a&b",
            "#main",
            "\t",
            "a\u{85}",
            "a\u{202E}b",
            "a\u{61C}b",
            "a\u{1680}b",
            "a\u{200F}b",
            ".",
        ];
        for bad in refused {
            let error = new_name(NameKind::Class, bad).unwrap_err();
            assert!(error.contains("is not a valid CSS class name"), "{error}");
            assert!(error.ends_with("Nothing was renamed."), "{error}");
            assert!(
                !error.chars().any(|c| c.is_control() || c == '\u{202E}'),
                "{error:?}"
            );
        }
        let error = new_name(NameKind::Id, ".box").unwrap_err();
        assert!(error.contains("is not a valid CSS id"), "{error}");
        // A refused name leaves every file as it was.
        let root = project("names");
        let site = root.join("css/site.css");
        let (l, c) = at(SITE, ".card {", 2);
        let refusal = rename_plan(&root, &site, SITE, l, c, "1box").unwrap();
        assert!(
            refusal
                .unwrap_err()
                .starts_with("\"1box\" is not a valid CSS class name")
        );
        assert_eq!(std::fs::read_to_string(&site).unwrap(), SITE);
        assert_eq!(
            std::fs::read_to_string(root.join("index.html")).unwrap(),
            INDEX
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0042/AK-41 (v22, behaviour 38c and d): more HTML files than the bound refuse the
    /// rename; a file larger than 1 MiB is left out; the shown text of an open page counts; a
    /// page outside the own sources refuses.
    #[tokio::test]
    async fn rename_bounds_and_shown_texts() {
        let root = project("rename-bounds");
        let site = root.join("css/site.css");
        let (l, c) = at(SITE, ".card {", 2);
        let big = format!(".card {{}}\n{}", " ".repeat(MAX_FILE_BYTES as usize));
        std::fs::write(root.join("css/big.css"), big).unwrap();
        let saved = "<p class=\"card\">x</p>\n";
        std::fs::write(root.join("page.html"), saved).unwrap();
        let shown = "<p class=\"card\">x</p>\n<p class=\"card\">y</p>\n";
        let plan = crate::shown::scope(vec![(root.join("page.html"), shown.into())], async {
            rename_plan(&root, &site, SITE, l, c, "box")
        })
        .await
        .unwrap()
        .unwrap();
        let files: Vec<(String, u64)> = edits_of(&root, &plan)
            .into_iter()
            .map(|(f, l, ..)| (f, l))
            .collect();
        assert_eq!(
            files,
            [
                ("css/site.css".into(), 19),
                ("css/site.css".into(), 22),
                ("index.html".into(), 12),
                ("page.html".into(), 0),
                ("page.html".into(), 1),
            ]
        );
        // Outside the scope the saved page counts.
        let plan = rename_plan(&root, &site, SITE, l, c, "box")
            .unwrap()
            .unwrap();
        assert_eq!(edits_of(&root, &plan).len(), 4);
        // A page that is not the project's own.
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        let foreign = root.join("node_modules/pkg/x.html");
        let text = "<p class=\"card\">";
        std::fs::write(&foreign, text).unwrap();
        let refusal = rename_plan(&root, &foreign, text, 0, 11, "box").unwrap();
        assert!(
            refusal
                .unwrap_err()
                .contains("is not among the project's own sources"),
        );
        // One HTML file more than the bound.
        std::fs::create_dir_all(root.join("many")).unwrap();
        for i in 0..MAX_HTML_FILES {
            std::fs::write(root.join(format!("many/p{i}.html")), "<p></p>\n").unwrap();
        }
        let refusal = rename_plan(&root, &site, SITE, l, c, "box").unwrap();
        let error = refusal.unwrap_err();
        assert!(
            error.contains("502 HTML files, more than the 500"),
            "{error}"
        );
        assert!(error.ends_with("nothing was renamed."), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0042/AK-42 (v22, behaviour 39; OBS-0069 G1): a `class` value offers the stylesheets'
    /// classes, an `id` value their ids, each once with the file and line of its first place.
    #[test]
    fn class_names_and_ids_are_offered() {
        let root = project("offered");
        std::fs::write(root.join("z.css"), ".zeta {}\n.card {}\n#zid {}\n").unwrap();
        let page = "<link rel=\"stylesheet\" href=\"css/site.css\">\n\
                    <style>.local {}</style>\n<div class=\"\" id=\"\"></div>\n\
                    <p class=\"a car-\"></p>\n<p class=\"a  b\"></p>\n";
        let file = root.join("page.html");
        std::fs::write(&file, page).unwrap();
        let offer = |needle: &str, into: u64| {
            let (l, c) = at(page, needle, into);
            completions(&root, &file, page, l, c, 100).map(|items| {
                items
                    .into_iter()
                    .map(|i| {
                        assert_eq!(i.insert, i.label);
                        let r = i.edit_range.unwrap();
                        assert_eq!((r.end_line, r.end_character), (l, c));
                        (
                            i.label,
                            i.kind.unwrap(),
                            i.description.unwrap(),
                            (r.start_line, r.start_character),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        };
        let classes = |start: (u64, u64)| {
            [
                ("banner", "css/site.css:5"),
                ("button", "css/site.css:13"),
                ("primary", "css/site.css:13"),
                ("card", "css/site.css:20"),
                ("local", "page.html:2"),
                ("zeta", "z.css:1"),
            ]
            .map(|(n, p)| (n.to_owned(), "class".to_owned(), p.to_owned(), start))
            .to_vec()
        };
        // An empty value: the caret between the quotes.
        assert_eq!(offer("class=\"\"", 7), Some(classes((2, 12))));
        assert_eq!(
            offer("id=\"\"", 4),
            Some(
                [
                    ("masthead", "css/site.css:9"),
                    ("prices", "css/site.css:17"),
                    ("zid", "z.css:3")
                ]
                .map(|(n, p)| (n.to_owned(), "field".to_owned(), p.to_owned(), (2, 18)))
                .to_vec()
            )
        );
        // A typed word with `-`: replaced from its start.
        assert_eq!(offer("car-\"", 4), Some(classes((3, 12))));
        let (l, c) = at(page, "car-\"", 4);
        assert_eq!(value_word(page, l, c), Some(12));
        // Between two words: nothing typed.
        assert_eq!(offer("a  b", 2), Some(classes((4, 12))));
        assert_eq!(value_word(page, 4, 12), Some(12));
        // Not in a class or id value.
        assert_eq!(offer("<div", 2), None);
        assert_eq!(offer("css/site.css", 2), None);
        assert_eq!(value_word(page, 0, 3), None);
        assert_eq!(
            completions(&root, &root.join("css/site.css"), SITE, 4, 2, 100),
            None
        );
        // The limit.
        let (l, c) = at(page, "class=\"\"", 7);
        assert_eq!(completions(&root, &file, page, l, c, 2).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }
    /// SPEC-0042 AK-49 (v26, behaviour 45): obsolete aliases and tagged items go, `flow` comes
    /// before `flow-root`.
    #[test]
    fn obsolete_css_values_are_left_out() {
        let item = |label: &str, doc: &str| CompletionItem {
            label: label.into(),
            insert: label.into(),
            kind: Some("value".into()),
            documentation: (!doc.is_empty()).then(|| doc.to_owned()),
            ..Default::default()
        };
        let mut tagged = item("-moz-box", "");
        tagged.raw = Some(serde_json::json!({"label": "-moz-box", "tags": [1]}));
        let mut items = vec![
            item("flex", "Flex container. _Widely available_"),
            item("flexbox", "Flow layout\\. Standardized as 'flex'\\."),
            item("-ms-flexbox", "Standardized as 'flex'."),
            item(
                "-ms-inline-flexbox",
                "Inline\\-level. Standardized as 'inline\\-flex'",
            ),
            item("-webkit-flex", "Flow layout."),
            item("flow-root", "Block container."),
            item("inline-flex", "Inline-level flex container."),
            tagged,
        ];
        clean_css_values(&mut items);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            ["flex", "-webkit-flex", "flow", "flow-root", "inline-flex"]
        );
        let mut colors = vec![item("red", ""), item("blue", "")];
        clean_css_values(&mut colors);
        assert_eq!(colors.len(), 2, "no `flow` without the values of `display`");
    }
}
