//! Code insight over LSP: hover, definition, references, completion, symbols (`SPEC-0024`).
//!
//! Positions in this API count Unicode scalar values; they are converted to
//! and from the position encoding the server chose.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::LanguageId;
use crate::lsp::{LspClient, LspError, file_uri};

/// Position encoding negotiated with the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Bytes of UTF-8.
    Utf8,
    /// UTF-16 code units (the LSP default).
    Utf16,
    /// Unicode scalar values.
    Utf32,
}

impl Encoding {
    /// The encoding named in the server capabilities (`utf-16` if none).
    pub fn from_capabilities(capabilities: &Value) -> Self {
        match capabilities.get("positionEncoding").and_then(Value::as_str) {
            Some("utf-8") => Self::Utf8,
            Some("utf-32") => Self::Utf32,
            _ => Self::Utf16,
        }
    }

    fn units(self, c: char) -> u64 {
        match self {
            Self::Utf8 => c.len_utf8() as u64,
            Self::Utf16 => c.len_utf16() as u64,
            Self::Utf32 => 1,
        }
    }
}

/// Characters → server units in `line`, clamped to the line end.
pub fn to_server(line: &str, character: u64, encoding: Encoding) -> u64 {
    line.chars()
        .take(character as usize)
        .map(|c| encoding.units(c))
        .sum()
}

/// Server units → characters in `line`, clamped to the line end.
pub fn from_server(line: &str, units: u64, encoding: Encoding) -> u64 {
    let mut used = 0;
    let mut chars = 0;
    for c in line.chars() {
        let next = used + encoding.units(c);
        if next > units {
            break;
        }
        used = next;
        chars += 1;
    }
    chars
}

pub(crate) fn line_of(text: &str, n: u64) -> &str {
    text.split('\n')
        .nth(n as usize)
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or("")
}

/// A range in characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Range {
    /// Start line.
    pub start_line: u64,
    /// Start character.
    pub start_character: u64,
    /// End line.
    pub end_line: u64,
    /// End character.
    pub end_character: u64,
}

/// Hover text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Hover {
    /// Text (Markdown or plain, as the server sent it).
    pub text: String,
    /// Range the hover applies to.
    pub range: Option<Range>,
}

/// One signature of a call (`SPEC-0080`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Signature {
    /// The whole signature, e.g. `int add(int a, int b)`.
    pub label: String,
    /// Documentation, if the server sends any.
    pub documentation: Option<String>,
    /// Each parameter as a char range of `label`.
    pub parameters: Vec<std::ops::Range<usize>>,
    /// The parameter the caret is in, if known.
    pub active_parameter: Option<usize>,
}

/// The signatures of the call around a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SignatureHelp {
    /// All overloads.
    pub signatures: Vec<Signature>,
    /// The overload the server thinks fits best.
    pub active_signature: usize,
}

/// A place in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    /// File path.
    pub path: PathBuf,
    /// Range in the file.
    pub range: Range,
}

/// One function in a call hierarchy with the functions that call it, or that it calls
/// (`SPEC-0082`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallNode {
    /// Function name.
    pub name: String,
    /// Symbol kind, e.g. `function` or `method`.
    pub kind: String,
    /// Extra detail, e.g. the containing class or a signature.
    pub detail: Option<String>,
    /// Where the function's name is.
    pub location: Location,
    /// Where the calls between this node and its parent are written (in the caller's file).
    pub call_sites: Vec<Range>,
    /// Callers (incoming) or callees (outgoing), expanded up to the requested depth.
    pub children: Vec<CallNode>,
    /// Whether this node repeats one of its ancestors (recursion); it is not expanded again.
    pub recursive: bool,
    /// A mark before the name: in a method hierarchy `+` (the type defines the method), `-` (it
    /// inherits it) or `!` (it must define it); `*` for the type a hierarchy was asked for
    /// (`SPEC-0082` v5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
}

/// Which way a call hierarchy goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDirection {
    /// Who calls the function (`callHierarchy/incomingCalls`).
    Callers,
    /// What the function calls (`callHierarchy/outgoingCalls`).
    Callees,
}

/// At most this many nodes are collected for one hierarchy.
const CALL_NODE_LIMIT: usize = 500;

/// A completion proposal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CompletionItem {
    /// Label shown in the list.
    pub label: String,
    /// Kind name, e.g. `function`.
    pub kind: Option<String>,
    /// Extra detail, e.g. a signature.
    pub detail: Option<String>,
    /// Text that would be inserted.
    pub insert: String,
    /// The part shown right after the label, e.g. `(E... elements)` (LSP `labelDetails.detail`,
    /// `SPEC-0072` v5 behaviour 10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_detail: Option<String>,
    /// The type shown at the right edge, e.g. `List<E>` (LSP `labelDetails.description`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The server's ranking key (behaviour 12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    /// Whether the server preselects this item (behaviour 12).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub preselect: bool,
    /// Documentation as Markdown, when the server sent it with the list (behaviour 13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    /// The server's item as sent, for `completionItem/resolve`.
    #[serde(skip)]
    pub raw: Option<Value>,
    /// Whether [`CompletionItem::insert`] is a snippet with tab stops (LSP `insertTextFormat` 2,
    /// `SPEC-0072` v7 behaviour 18).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub snippet: bool,
    /// The range the server replaces with [`CompletionItem::insert`] (LSP `textEdit`; for an
    /// `InsertReplaceEdit` its insert range), in characters (`SPEC-0072` v7 behaviour 19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_range: Option<Range>,
    /// Further edits elsewhere in the file, e.g. an import (LSP `additionalTextEdits`,
    /// `SPEC-0072` v7 behaviour 20).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additional_edits: Vec<CompletionEdit>,
    /// The class a member comes from, e.g. `Item` or `object` (Python, `SPEC-0072` v10
    /// behaviour 44).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// An edit that comes with a completion item (`SPEC-0072` v7 behaviour 20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CompletionEdit {
    /// The replaced range, in characters.
    pub range: Range,
    /// The new text.
    pub text: String,
}

/// The ranges of `item` in characters: its own edit range and its additional edits.
fn completion_ranges(
    item: &Value,
    uri: &str,
    conv: &mut dyn Converter,
) -> (Option<Range>, Vec<CompletionEdit>) {
    let edit = item.get("textEdit").and_then(|e| {
        e.get("range")
            .or_else(|| e.get("insert"))
            .and_then(|r| parse_range(r, uri, conv))
    });
    (edit, additional_edits(item, uri, conv))
}

/// The `additionalTextEdits` of an item, empty ones left out.
fn additional_edits(item: &Value, uri: &str, conv: &mut dyn Converter) -> Vec<CompletionEdit> {
    item.get("additionalTextEdits")
        .and_then(Value::as_array)
        .map(|edits| {
            edits
                .iter()
                .filter_map(|e| {
                    let range = parse_range(e.get("range")?, uri, conv)?;
                    let text = e.get("newText")?.as_str()?.to_owned();
                    let empty = text.is_empty()
                        && range.start_line == range.end_line
                        && range.start_character == range.end_character;
                    (!empty).then_some(CompletionEdit { range, text })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Documentation of an LSP item: a string or `MarkupContent`, as Markdown; `None` when empty.
fn documentation_text(v: Option<&Value>) -> Option<String> {
    let text = match v? {
        Value::String(s) => s.clone(),
        other => other.get("value")?.as_str()?.to_owned(),
    };
    (!text.trim().is_empty()).then_some(text)
}

/// A symbol of a document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Symbol {
    /// Name.
    pub name: String,
    /// Kind name, e.g. `function`.
    pub kind: String,
    /// Extra detail.
    pub detail: Option<String>,
    /// Start line.
    pub line: u64,
    /// Start character.
    pub character: u64,
    /// Nesting depth (0 = top level).
    pub depth: usize,
    /// Last line of the whole symbol, body included (`SPEC-0081` v2: what contains a place).
    pub end_line: u64,
    /// Character after the symbol's end on `end_line`.
    pub end_character: u64,
    /// The type a member is inherited from (`SPEC-0079` v4 behaviour 9), for inherited rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_from: Option<String>,
    /// The file of an inherited member, when it is not the file asked about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Symbol {
    /// Whether the symbol, from the start of its name's line to the end of its body, covers
    /// 0-based `line` and `character` (modifiers before the name on its line count).
    pub fn contains(&self, line: u64, character: u64) -> bool {
        (self.line, 0) <= (line, character)
            && (line, character) <= (self.end_line, self.end_character)
    }
}

/// Whether to look for the definition or for all references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationKind {
    /// `textDocument/definition`
    Definition,
    /// `textDocument/references` (including the declaration)
    References,
    /// `textDocument/typeDefinition`: the declaration of the symbol's type (`SPEC-0081`)
    TypeDefinition,
    /// `textDocument/implementation`: implementations and overrides (`SPEC-0081`)
    Implementation,
}

const COMPLETION_KINDS: [&str; 25] = [
    "text",
    "method",
    "function",
    "constructor",
    "field",
    "variable",
    "class",
    "interface",
    "module",
    "property",
    "unit",
    "value",
    "enum",
    "keyword",
    "snippet",
    "color",
    "file",
    "reference",
    "folder",
    "enum-member",
    "constant",
    "struct",
    "event",
    "operator",
    "type-parameter",
];

pub(crate) const SYMBOL_KINDS: [&str; 26] = [
    "file",
    "module",
    "namespace",
    "package",
    "class",
    "method",
    "property",
    "field",
    "constructor",
    "enum",
    "interface",
    "function",
    "variable",
    "constant",
    "string",
    "number",
    "boolean",
    "array",
    "object",
    "key",
    "null",
    "enum-member",
    "struct",
    "event",
    "operator",
    "type-parameter",
];

pub(crate) fn kind_name(table: &[&str], v: Option<&Value>) -> Option<String> {
    let n = v?.as_u64()? as usize;
    table.get(n.checked_sub(1)?).map(|s| s.to_string())
}

/// `file://` URI → path, percent-decoded; repeated separators and `.` components are dropped, so
/// one file is one path (ruby-lsp names indexed files `root//file.rb`, hand test 110).
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = rest
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
    String::from_utf8(out)
        .ok()
        .map(|p| PathBuf::from(p).components().collect())
}

/// Converts server positions of `uri` into characters.
pub trait Converter {
    /// (line, server units) in the file at `uri` → characters.
    fn character(&mut self, uri: &str, line: u64, units: u64) -> u64;
}

/// Reads an LSP range of `uri`, converting characters.
pub(crate) fn parse_range(v: &Value, uri: &str, conv: &mut dyn Converter) -> Option<Range> {
    let n = |p: &str| v.pointer(p).and_then(Value::as_u64);
    let (sl, sc, el, ec) = (
        n("/start/line")?,
        n("/start/character")?,
        n("/end/line")?,
        n("/end/character")?,
    );
    Some(Range {
        start_line: sl,
        start_character: conv.character(uri, sl, sc),
        end_line: el,
        end_character: conv.character(uri, el, ec),
    })
}

/// Reads a hover answer.
pub fn parse_hover(v: &Value, uri: &str, conv: &mut dyn Converter) -> Option<Hover> {
    let marked = |m: &Value| match m {
        Value::String(s) => s.clone(),
        other => other
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    let contents = v.get("contents")?;
    let text = match contents {
        Value::Array(parts) => parts.iter().map(marked).collect::<Vec<_>>().join("\n\n"),
        other => marked(other),
    };
    if text.trim().is_empty() {
        return None;
    }
    Some(Hover {
        text,
        range: v.get("range").and_then(|r| parse_range(r, uri, conv)),
    })
}

/// Char offset in `label` of a UTF-16 offset.
fn char_of_utf16_in(label: &str, units: u64) -> usize {
    let mut seen = 0u64;
    for (i, c) in label.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16() as u64;
    }
    label.chars().count()
}

/// Reads a `SignatureHelp` answer; `None` for null or no signatures.
pub fn parse_signature_help(v: &Value) -> Option<SignatureHelp> {
    let list = v.get("signatures")?.as_array()?;
    if list.is_empty() {
        return None;
    }
    let top_active = v.get("activeParameter").and_then(Value::as_u64);
    let text = |d: &Value| match d {
        Value::String(s) => Some(s.clone()),
        Value::Object(_) => d.get("value").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    };
    let signatures = list
        .iter()
        .map(|s| {
            let label = s["label"].as_str().unwrap_or_default().to_owned();
            let mut from = 0usize;
            let parameters = s["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| match &p["label"] {
                    Value::Array(pair) => {
                        let a = char_of_utf16_in(&label, pair.first()?.as_u64()?);
                        let b = char_of_utf16_in(&label, pair.get(1)?.as_u64()?);
                        Some(a..b)
                    }
                    Value::String(name) => {
                        // Plain labels are found in order after the previous parameter.
                        let chars: Vec<char> = label.chars().collect();
                        let needle: Vec<char> = name.chars().collect();
                        let at = (from..=chars.len().checked_sub(needle.len())?)
                            .find(|&i| chars[i..i + needle.len()] == needle[..])?;
                        from = at + needle.len();
                        Some(at..at + needle.len())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let active = s
                .get("activeParameter")
                .and_then(Value::as_u64)
                .or(top_active)
                .map(|a| a as usize)
                .filter(|&a| a < parameters.len());
            Signature {
                label,
                documentation: s.get("documentation").and_then(text),
                parameters,
                active_parameter: active,
            }
        })
        .collect::<Vec<_>>();
    let active_signature = v
        .get("activeSignature")
        .and_then(Value::as_u64)
        .map(|a| a as usize)
        .filter(|&a| a < signatures.len())
        .unwrap_or(0);
    Some(SignatureHelp {
        signatures,
        active_signature,
    })
}

/// Reads `Location`, `Location[]` or `LocationLink[]`.
pub fn parse_locations(v: &Value, conv: &mut dyn Converter) -> Vec<Location> {
    let one = |l: &Value, conv: &mut dyn Converter| {
        let (uri, r) = match (l.get("targetUri"), l.get("uri")) {
            (Some(u), _) => (
                u.as_str()?,
                l.get("targetSelectionRange").or(l.get("targetRange"))?,
            ),
            (None, Some(u)) => (u.as_str()?, l.get("range")?),
            _ => return None,
        };
        Some(Location {
            path: uri_to_path(uri)?,
            range: parse_range(r, uri, conv)?,
        })
    };
    let mut found: Vec<Location> = match v {
        Value::Array(list) => list.iter().filter_map(|l| one(l, conv)).collect(),
        Value::Null => Vec::new(),
        other => one(other, conv).into_iter().collect(),
    };
    // One place named twice is one location (ruby-lsp with a file indexed under two URIs).
    let mut i = 0;
    while i < found.len() {
        if found[..i].contains(&found[i]) {
            found.remove(i);
        } else {
            i += 1;
        }
    }
    found
}

/// Readable names of the locations in `v` that are no files (`SPEC-0042` v7 behaviour 16): jdtls
/// answers `jdt://contents/java.base/java.lang/String.class?…` for a class of the JDK or a library,
/// named `java.lang.String`; other addresses are given up to their query.
pub fn outside_locations(v: &Value) -> Vec<String> {
    let uri_of = |l: &Value| {
        l.get("targetUri")
            .or(l.get("uri"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let uris: Vec<String> = match v {
        Value::Array(list) => list.iter().filter_map(uri_of).collect(),
        Value::Null => Vec::new(),
        other => uri_of(other).into_iter().collect(),
    };
    let mut names: Vec<String> = uris
        .iter()
        .filter(|u| uri_to_path(u).is_none())
        .map(|u| library_name(u))
        .collect();
    names.dedup();
    names
}

/// The addresses of the locations in `v` that are no files, with their 0-based start lines.
pub fn outside_places(v: &Value) -> Vec<(String, u64)> {
    let place = |l: &Value| {
        let uri = l.get("targetUri").or(l.get("uri"))?.as_str()?.to_owned();
        let line = l
            .pointer("/targetSelectionRange/start/line")
            .or(l.pointer("/range/start/line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        Some((uri, line))
    };
    let all: Vec<(String, u64)> = match v {
        Value::Array(list) => list.iter().filter_map(place).collect(),
        Value::Null => Vec::new(),
        other => place(other).into_iter().collect(),
    };
    all.into_iter()
        .filter(|(u, _)| uri_to_path(u).is_none())
        .collect()
}

/// `java.lang.String` for a `jdt://contents/<module or jar>/<package>/<Class>.class?…` address
/// (`.java` when the library's sources are there).
fn library_name(uri: &str) -> String {
    let bare = uri.split('?').next().unwrap_or(uri);
    if let Some(rest) = bare.strip_prefix("jdt://contents/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if let [.., package, class] = parts.as_slice()
            && let Some(class) = class
                .strip_suffix(".class")
                .or_else(|| class.strip_suffix(".java"))
        {
            let class = class.split('$').next().unwrap_or(class);
            return if package.is_empty() || package.contains(".jar") {
                class.to_owned()
            } else {
                format!("{package}.{class}")
            };
        }
    }
    bare.to_owned()
}

/// What a location request found: places in files, and the names of places that are no files
/// (library classes, `SPEC-0042` v7 behaviour 16).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Found {
    /// Places in files.
    pub locations: Vec<Location>,
    /// Readable names of the places outside files.
    pub outside: Vec<String>,
    /// The addresses of the places outside files with their 0-based start lines, for opening a
    /// library class's source (`SPEC-0149` v3 behaviour 11).
    #[serde(skip)]
    pub outside_places: Vec<(String, u64)>,
}

/// Reads a `CompletionList` or `CompletionItem[]`, keeping at most `limit` items: the best ones in
/// the server's ranking, in that order — preselected first, then by sort text ignoring case first,
/// items without sort text last, the server's order where nothing decides (`SPEC-0072` v17
/// behaviour 54; rust-analyzer and intelephense hand out their lists unsorted).
pub fn parse_completion(v: &Value, limit: usize) -> Vec<CompletionItem> {
    let items = match v {
        Value::Array(list) => list.as_slice(),
        other => other
            .get("items")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice),
    };
    let rank = |i: &Value| {
        let sort = i.get("sortText").and_then(Value::as_str);
        (
            i.get("preselect").and_then(Value::as_bool) != Some(true),
            sort.is_none(),
            sort.map(str::to_lowercase),
            sort.map(str::to_owned),
        )
    };
    let mut ranked: Vec<&Value> = items.iter().collect();
    ranked.sort_by_cached_key(|i| rank(i));
    ranked
        .into_iter()
        .take(limit)
        .map(|i| {
            let s = |k: &str| i.get(k).and_then(Value::as_str).map(str::to_owned);
            let label = s("label").unwrap_or_default();
            CompletionItem {
                insert: i
                    .pointer("/textEdit/newText")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| s("insertText"))
                    .unwrap_or_else(|| label.trim().to_owned()),
                kind: kind_name(&COMPLETION_KINDS, i.get("kind")),
                detail: s("detail"),
                label: label.trim().to_owned(),
                label_detail: i
                    .pointer("/labelDetails/detail")
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned),
                description: i
                    .pointer("/labelDetails/description")
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned),
                sort_text: s("sortText"),
                preselect: i.get("preselect").and_then(Value::as_bool) == Some(true),
                documentation: documentation_text(i.get("documentation")),
                snippet: i.get("insertTextFormat").and_then(Value::as_u64) == Some(2),
                raw: Some(i.clone()),
                ..Default::default()
            }
        })
        .collect()
}

/// Reads `DocumentSymbol[]` (flattened depth-first) or `SymbolInformation[]`.
pub fn parse_symbols(v: &Value, uri: &str, conv: &mut dyn Converter) -> Vec<Symbol> {
    fn walk(
        list: &[Value],
        depth: usize,
        uri: &str,
        conv: &mut dyn Converter,
        out: &mut Vec<Symbol>,
    ) {
        for s in list {
            let r = s
                .get("selectionRange")
                .or_else(|| s.get("range"))
                .or_else(|| s.pointer("/location/range"));
            let Some(r) = r.and_then(|r| parse_range(r, uri, conv)) else {
                continue;
            };
            let whole = s
                .get("range")
                .or_else(|| s.pointer("/location/range"))
                .and_then(|r| parse_range(r, uri, conv))
                .unwrap_or_else(|| r.clone());
            out.push(Symbol {
                end_line: whole.end_line.max(r.end_line),
                end_character: if whole.end_line >= r.end_line {
                    whole.end_character
                } else {
                    r.end_character
                },
                inherited_from: None,
                path: None,
                name: s
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                kind: kind_name(&SYMBOL_KINDS, s.get("kind")).unwrap_or_else(|| "unknown".into()),
                detail: s.get("detail").and_then(Value::as_str).map(str::to_owned),
                line: r.start_line,
                character: r.start_character,
                depth,
            });
            if let Some(children) = s.get("children").and_then(Value::as_array) {
                walk(children, depth + 1, uri, conv, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(list) = v.as_array() {
        walk(list, 0, uri, conv, &mut out);
    }
    out
}

/// Converts with file texts read from disk (the opened file from memory, and the texts the window
/// shows for open files while a change is planned, `SPEC-0220` v4 behaviour 13).
pub(crate) struct Files {
    pub(crate) encoding: Encoding,
    pub(crate) texts: HashMap<String, String>,
    /// The shown texts of the planning scope ([`crate::shown::scope`]) when the file was opened.
    pub(crate) shown: crate::shown::Texts,
}

impl Files {
    /// The content of `path`: its shown text when the planning scope gave it, else the disk.
    pub(crate) fn read(&self, path: &Path) -> std::io::Result<String> {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        match self.shown.get(&key) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(path),
        }
    }
}

impl Converter for Files {
    fn character(&mut self, uri: &str, line: u64, units: u64) -> u64 {
        if !self.texts.contains_key(uri) {
            let text = uri_to_path(uri)
                .and_then(|p| self.read(&p).ok())
                .unwrap_or_default();
            self.texts.insert(uri.to_owned(), text);
        }
        let text = &self.texts[uri];
        from_server(line_of(text, line), units, self.encoding)
    }
}

/// A file opened in a freshly started server.
/// How long a question waits at most for a server's refined diagnostics after a list from its build
/// (jdtls; `SPEC-0076` v8 behaviour 10). It waits only when the list is one from the build
/// ([`from_the_build`]); a server under load publishes the refined list many seconds later.
pub(crate) const REFINE_WAIT: Duration = Duration::from_secs(30);

/// How long a question waits for the next list of a server that publishes its diagnostics in
/// steps (`SPEC-0042` v23 behaviour 41): typescript-language-server sends the semantic check's
/// list some hundred milliseconds after the syntax check's, longer for a server just started.
pub(crate) const STEP_QUIET: Duration = Duration::from_secs(2);

/// Whether `list` is jdtls's list from its build of the saved file: every problem named by its line
/// only, at character 0 with no width. Its refined list names them exactly; when jdtls has the build
/// already it publishes only the refined one (`SPEC-0076` v8 behaviour 10).
pub(crate) fn from_the_build(list: &[Value]) -> bool {
    !list.is_empty()
        && list.iter().all(|d| {
            let r = &d["range"];
            r["start"]["character"] == 0
                && r["end"]["character"] == 0
                && r["start"]["line"] == r["end"]["line"]
        })
}

/// The files of `shown` other than `file` that the server at `command` serves for the workspace at
/// `root`, with their language and text (`SPEC-0220` v4 behaviour 13).
fn shown_for_server(
    root: &Path,
    file: &Path,
    command: &Path,
    shown: &crate::shown::Texts,
) -> Vec<(PathBuf, LanguageId, String)> {
    let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut same: HashMap<LanguageId, bool> = HashMap::new();
    let mut out: Vec<(PathBuf, LanguageId, String)> = shown
        .iter()
        .filter(|(path, _)| **path != key)
        .filter_map(|(path, text)| {
            let language = LanguageId::detect(path)?;
            let served = *same.entry(language).or_insert_with(|| {
                crate::servers::resolve_for(language, root).is_ok_and(|r| r.command == command)
            });
            served.then(|| (path.clone(), language, text.clone()))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub(crate) struct Opened {
    client: Client,
    pub(crate) files: Files,
    pub(crate) uri: String,
    pub(crate) text: String,
    pub(crate) capabilities: Value,
    /// Other files sent to the server for their symbols (`SPEC-0081` v2).
    others: Vec<String>,
    /// The server's diagnostics are pulled, not waited for (`SPEC-0141` v4 behaviour 9).
    pulls_diagnostics: bool,
    /// The server publishes the diagnostics of the build first and refines them after; the refined
    /// list is the one asked about (`SPEC-0076` v6 behaviour 10).
    refines_diagnostics: bool,
    /// The server publishes an opened file's diagnostics twice; the second list is the answer
    /// (`SPEC-0042` v20 behaviour 36).
    publishes_twice: bool,
    /// The server publishes an opened file's diagnostics in steps; the last list is the answer
    /// (`SPEC-0042` v23 behaviour 41).
    publishes_in_steps: bool,
    /// The document version last sent to a server of its own.
    version: i64,
}

/// The server an [`Opened`] file talks to: started for this question, or a running one of the pool
/// (`SPEC-0090`).
enum Client {
    Own(Box<LspClient>),
    Pooled(Arc<crate::pool::Server>),
}

impl Opened {
    /// Opens `file` with `text` as its content (the editor's unsaved text), or the disk content.
    pub(crate) async fn open_text(
        root: &Path,
        file: &Path,
        text: Option<&str>,
        wait: Duration,
    ) -> Result<Self, LspError> {
        let language = LanguageId::detect(file)
            .ok_or_else(|| LspError::NoServer(file.display().to_string()))?;
        let server = crate::servers::resolve_for(language, root)?;
        let pulls_diagnostics = server.candidate.pulls_diagnostics;
        let publishes_twice = server.candidate.publishes_twice;
        let publishes_in_steps = server.candidate.publishes_in_steps;
        // The newest list is the answer also after bash-language-server got its text again.
        let refines_diagnostics =
            server.candidate.refines_diagnostics || server.candidate.rereads_saved_files;
        let shown = crate::shown::current();
        let text = match text {
            Some(t) => t.to_owned(),
            None => crate::shown::read(file).map_err(|e| LspError::Io(e.to_string()))?,
        };
        // The other open files with unsaved changes this server serves (SPEC-0220 v4 behaviour 13).
        let others = shown_for_server(root, file, &server.command, &shown);
        if let Some(pool) = crate::pool::pool() {
            let s = pool.server(root, server, wait).await?;
            let uri = file_uri(file);
            pool.sync(&s, file, language, &text, wait).await?;
            let mut texts = HashMap::from([(uri.clone(), text.clone())]);
            let mut sent = Vec::new();
            for (path, other_language, other_text) in &others {
                // Opened first as the editor opens it: a server that diagnoses first has read it
                // before the question that follows.
                pool.sync(&s, path, *other_language, other_text, wait)
                    .await?;
                let other_uri = file_uri(path);
                texts.insert(other_uri.clone(), other_text.clone());
                sent.push(other_uri);
            }
            let capabilities = s.capabilities.clone();
            return Ok(Self {
                files: Files {
                    encoding: Encoding::from_capabilities(&capabilities),
                    texts,
                    shown,
                },
                client: Client::Pooled(s),
                uri,
                text,
                capabilities,
                others: sent,
                pulls_diagnostics,
                refines_diagnostics,
                publishes_twice,
                publishes_in_steps,
                version: 2,
            });
        }
        let mut client = server.spawn(root)?;
        client.timeout = wait;
        client.init_options = server.initialization_options(root);
        client.set_settings(server.settings(root));
        let opened = async {
            let capabilities = client.initialize(root).await?;
            let uri = file_uri(file);
            client
                .notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {"uri": uri, "languageId": language.lsp_id_for(&uri), "version": 1, "text": text}}),
                )
                .await?;
            if server.candidate.resend_on_open {
                client
                    .notify(
                        "textDocument/didChange",
                        json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": text}]}),
                    )
                    .await?;
            }
            // The other open files with unsaved changes it serves (SPEC-0220 v4 behaviour 13).
            for (path, other_language, other_text) in &others {
                let other_uri = file_uri(path);
                client
                    .notify(
                        "textDocument/didOpen",
                        json!({"textDocument": {"uri": other_uri, "languageId": other_language.lsp_id_for(&other_uri), "version": 1, "text": other_text}}),
                    )
                    .await?;
            }
            // A server that reports its state answers only after loading the workspace; others
            // answer at once (SPEC-0042 v2).
            if server.candidate.reports_status {
                let settle = server
                    .candidate
                    .skips_indexing_when_cached
                    .then_some(crate::lsp::INDEXING_SETTLE);
                client.wait_until_ready(wait, settle).await;
            }
            if let Some(loading) = server.candidate.loading_text {
                let deadline = tokio::time::Instant::now() + wait;
                while tokio::time::Instant::now() < deadline {
                    let probe = client
                        .request(
                            "textDocument/hover",
                            json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}}),
                        )
                        .await?;
                    if !probe.to_string().contains(loading) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
            // pyright lists the project's files before it publishes the first diagnostics; asked
            // earlier, it answers references and rename from the opened file alone (SPEC-0042 v14
            // behaviour 22).
            if server.candidate.diagnoses_first || server.candidate.lists_files_first {
                let published = client.wait_for_diagnostics(&uri, wait).await;
                // The other files given (SPEC-0220 v4 behaviour 13) are read when their
                // diagnostics come, so the question finds them in the server's index.
                if server.candidate.diagnoses_first {
                    for (path, ..) in &others {
                        let other_uri = file_uri(path);
                        if client.latest_held_diagnostics(&other_uri).is_none() {
                            client.wait_for_diagnostics(&other_uri, wait).await;
                        }
                    }
                }
                // jdtls: the build's list comes first, the one with exact ranges right after it
                // (SPEC-0076 v6 behaviour 10).
                if published
                    && refines_diagnostics
                    && client
                        .latest_held_diagnostics(&uri)
                        .is_some_and(|list| from_the_build(&list))
                {
                    client.wait_for_diagnostics(&uri, REFINE_WAIT.min(wait)).await;
                }
                // bash-language-server has meanwhile read the saved file over the opened text
                // (SPEC-0042 v18 behaviour 34).
                if published
                    && server.candidate.rereads_saved_files
                    && std::fs::read_to_string(file).ok().as_deref() != Some(text.as_str())
                {
                    client
                        .notify(
                            "textDocument/didChange",
                            json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": text}]}),
                        )
                        .await?;
                    client.wait_for_diagnostics(&uri, wait).await;
                }
            }
            Ok::<_, LspError>((capabilities, uri))
        }
        .await;
        match opened {
            Ok((capabilities, uri)) => Ok(Self {
                files: Files {
                    encoding: Encoding::from_capabilities(&capabilities),
                    texts: others
                        .iter()
                        .map(|(p, _, t)| (file_uri(p), t.clone()))
                        .chain([(uri.clone(), text.clone())])
                        .collect(),
                    shown,
                },
                client: Client::Own(Box::new(client)),
                uri,
                text,
                capabilities,
                others: others.iter().map(|(p, ..)| file_uri(p)).collect(),
                pulls_diagnostics,
                refines_diagnostics,
                publishes_twice,
                publishes_in_steps,
                version: 2,
            }),
            Err(e) => {
                client.shutdown().await;
                Err(e)
            }
        }
    }

    /// Replaces the text of the opened `file` on the server, so a further question is answered for
    /// `text` (`SPEC-0123` v4 behaviour 6: the second code action works on the first one's result).
    pub(crate) async fn set_text(
        &mut self,
        file: &Path,
        text: &str,
        wait: Duration,
    ) -> Result<(), LspError> {
        match &self.client {
            Client::Own(c) => {
                self.version += 1;
                c.notify(
                    "textDocument/didChange",
                    json!({"textDocument": {"uri": self.uri, "version": self.version}, "contentChanges": [{"text": text}]}),
                )
                .await?;
            }
            Client::Pooled(s) => {
                let language = LanguageId::detect(file)
                    .ok_or_else(|| LspError::NoServer(file.display().to_string()))?;
                let pool = crate::pool::pool().expect("a pooled server implies the pool");
                pool.sync(s, file, language, text, wait).await?;
            }
        }
        self.text = text.to_owned();
        self.files.texts.insert(self.uri.clone(), text.to_owned());
        Ok(())
    }

    pub(crate) fn position(&self, line: u64, character: u64) -> Value {
        let units = to_server(line_of(&self.text, line), character, self.files.encoding);
        json!({"textDocument": {"uri": self.uri}, "position": {"line": line, "character": units}})
    }

    pub(crate) async fn ask(&self, method: &str, params: Value) -> Result<Value, LspError> {
        match &self.client {
            Client::Own(c) => c.request(method, params).await,
            Client::Pooled(s) => {
                let (s, method) = (Arc::clone(s), method.to_owned());
                let pool = crate::pool::pool().expect("a pooled server implies the pool");
                pool.run(async move { s.client.request(&method, params).await })
                    .await?
            }
        }
    }

    /// Like [`Self::ask`], waiting at most `wait` or until `stop` is set, the request withdrawn when
    /// given up (`SPEC-0293` behaviour 2).
    pub(crate) async fn ask_bounded(
        &self,
        method: &str,
        params: Value,
        wait: Duration,
        stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Value, LspError> {
        match &self.client {
            Client::Own(c) => c.request_bounded(method, params, wait, stop).await,
            Client::Pooled(s) => {
                let (s, method) = (Arc::clone(s), method.to_owned());
                let pool = crate::pool::pool().expect("a pooled server implies the pool");
                pool.run(async move { s.client.request_bounded(&method, params, wait, stop).await })
                    .await?
            }
        }
    }

    /// Sends another `file` of the project (its disk content) to the server, once.
    pub(crate) async fn open_other(&mut self, file: &Path, wait: Duration) -> Result<(), LspError> {
        let uri = file_uri(file);
        if uri == self.uri || self.others.contains(&uri) {
            return Ok(());
        }
        let language = LanguageId::detect(file)
            .ok_or_else(|| LspError::NoServer(file.display().to_string()))?;
        let text = self
            .files
            .read(file)
            .map_err(|e| LspError::Io(e.to_string()))?;
        match &self.client {
            Client::Own(c) => {
                c.notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {"uri": uri, "languageId": language.lsp_id_for(&uri), "version": 1, "text": text}}),
                )
                .await?;
            }
            Client::Pooled(s) => {
                let pool = crate::pool::pool().expect("a pooled server implies the pool");
                pool.sync(s, file, language, &text, wait).await?;
            }
        }
        self.files.texts.insert(uri.clone(), text);
        self.others.push(uri);
        Ok(())
    }

    /// The symbols of another `file` of the project (its disk content), or of the opened one,
    /// asked within `wait` (`SPEC-0081` v2 behaviour 4, `SPEC-0293` v3 behaviour 14).
    pub(crate) async fn symbols_of(
        &mut self,
        file: &Path,
        wait: Duration,
    ) -> Result<Vec<Symbol>, LspError> {
        let uri = file_uri(file);
        self.open_other(file, wait).await?;
        let v = self
            .ask_bounded(
                "textDocument/documentSymbol",
                json!({"textDocument": {"uri": uri}}),
                wait,
                None,
            )
            .await?;
        Ok(parse_symbols(&v, &uri, &mut self.files))
    }

    /// Whether the server was started for this question or less than 30 seconds ago.
    pub(crate) fn just_started(&self) -> bool {
        match &self.client {
            Client::Own(_) => true,
            Client::Pooled(s) => s.started.elapsed() < Duration::from_secs(30),
        }
    }

    /// The error for a feature the server says it does not offer (`SPEC-0023` v3 behaviour 11).
    pub(crate) fn unsupported(&self, method: &str) -> LspError {
        LspError::Unsupported {
            server: self.lsp().name.clone(),
            method: method.to_owned(),
        }
    }

    /// The connection, e.g. for the edits the server applied.
    pub(crate) fn lsp(&self) -> &LspClient {
        match &self.client {
            Client::Own(c) => c,
            Client::Pooled(s) => &s.client,
        }
    }

    /// The raw diagnostics the server publishes for the opened text, waiting at most `wait`; a
    /// pooled server that already had this text answers with what it published before.
    pub(crate) async fn diagnostics(&mut self, wait: Duration) -> Vec<Value> {
        // A server that publishes in steps answers a pull with the complete list; without an
        // answer the publications are read as for any server (`SPEC-0141` v4 behaviour 9).
        if self.pulls_diagnostics {
            let pulled = self
                .ask_bounded(
                    "textDocument/diagnostic",
                    json!({"textDocument": {"uri": self.uri}}),
                    wait,
                    None,
                )
                .await;
            if let Ok(report) = pulled
                && let Some(items) = report["items"].as_array()
            {
                return items.clone();
            }
        }
        match &mut self.client {
            Client::Own(c) => {
                let deadline = tokio::time::Instant::now() + wait;
                // A server that counted as ready after its silence may still begin indexing
                // (intelephense under load): what it publishes until the indexing ends is about a
                // project it has not read, and it publishes again after (`SPEC-0042` v20
                // behaviour 36).
                let mut indexing = false;
                // A server that publishes twice: its first list, kept while the second is awaited.
                let mut first: Option<Vec<Value>> = None;
                let mut until = deadline;
                loop {
                    let left = until.saturating_duration_since(tokio::time::Instant::now());
                    let Some(n) = c.next_notification(left).await else {
                        return first.unwrap_or_default();
                    };
                    if n.method == crate::lsp::INDEXING_STARTED {
                        indexing = true;
                        until = deadline;
                    } else if n.method == crate::lsp::INDEXING_ENDED {
                        indexing = false;
                    } else if !indexing
                        && n.method == "textDocument/publishDiagnostics"
                        && n.params["uri"].as_str() == Some(self.uri.as_str())
                    {
                        // A later list already held refines this one (SPEC-0076 v6 behaviour 10).
                        if (self.refines_diagnostics || self.publishes_twice)
                            && let Some(later) = c.latest_held_diagnostics(&self.uri)
                        {
                            return later;
                        }
                        let list = n.params["diagnostics"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        // In steps: the next list follows soon, or this one was the last.
                        if self.publishes_in_steps
                            && c.publications(&self.uri) < crate::servers::STEPS
                        {
                            first = Some(list);
                            until = deadline.min(tokio::time::Instant::now() + STEP_QUIET);
                            continue;
                        }
                        if self.publishes_twice && first.is_none() && c.publications(&self.uri) < 2
                        {
                            first = Some(list);
                            until = deadline.min(tokio::time::Instant::now() + REFINE_WAIT);
                            continue;
                        }
                        return list;
                    }
                }
            }
            Client::Pooled(s) => {
                // Whoever sent the current text (this question or the editor), the diagnostics
                // published after that are the ones for it.
                let (s, uri) = (Arc::clone(s), self.uri.clone());
                let pool = crate::pool::pool().expect("a pooled server implies the pool");
                pool.run(async move {
                    let sent_at = s.sent_at(&uri).await;
                    s.diagnostics_after(&uri, sent_at, wait).await
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
            }
        }
    }

    /// `file` with `text` as opened over `client`, a connection to a scripted server.
    #[cfg(test)]
    pub(crate) fn over(client: LspClient, file: &Path, text: &str) -> Self {
        let uri = file_uri(file);
        Self {
            files: Files {
                encoding: Encoding::from_capabilities(&Value::Null),
                texts: HashMap::from([(uri.clone(), text.to_owned())]),
                shown: Default::default(),
            },
            client: Client::Own(Box::new(client)),
            uri,
            text: text.to_owned(),
            capabilities: Value::Null,
            others: Vec::new(),
            pulls_diagnostics: false,
            refines_diagnostics: false,
            publishes_twice: false,
            publishes_in_steps: false,
            version: 2,
        }
    }

    /// Ends the question: a server started for it is shut down, a pooled one keeps running.
    pub(crate) async fn close(self) {
        if let Client::Own(c) = self.client {
            c.shutdown().await;
        }
    }
}

/// Hover at a position of `file`.
pub async fn hover(
    root: &Path,
    file: &Path,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Option<Hover>, LspError> {
    hover_in(root, file, None, line, character, wait).await
}

/// Hover at a position of `file` whose content is `text` (the editor's unsaved text), or the disk
/// content (`SPEC-0077`).
pub async fn hover_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Option<Hover>, LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let answer = o
        .ask("textDocument/hover", o.position(line, character))
        .await;
    let uri = o.uri.clone();
    let mut result = answer.map(|v| parse_hover(&v, &uri, &mut o.files));
    // On the first character of a template name the Vue server describes its generated context
    // instead; one character further right it names the symbol (`SPEC-0141` v4 behaviour 10).
    if LanguageId::detect(file) == Some(LanguageId::Vue)
        && matches!(&result, Ok(Some(h)) if vue_internal(&h.text))
        && next_in_word(line_of(&o.text, line), character)
        && let Ok(again) = o
            .ask("textDocument/hover", o.position(line, character + 1))
            .await
        && let Some(h) = parse_hover(&again, &uri, &mut o.files)
    {
        result = Ok(Some(h));
    }
    o.close().await;
    result
}

/// Whether a Vue hover describes one of the server's generated `__VLS_` declarations (its first
/// code line names one before the type) rather than a symbol of the file (`SPEC-0141` v4).
fn vue_internal(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))
        .is_some_and(|l| l.split(':').next().unwrap_or(l).contains("__VLS_"))
}

/// Whether the characters at `character` and right after it in `line` both belong to a name.
fn next_in_word(line: &str, character: u64) -> bool {
    let name = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut chars = line
        .chars()
        .skip(usize::try_from(character).unwrap_or(usize::MAX));
    matches!((chars.next(), chars.next()), (Some(a), Some(b)) if name(a) && name(b))
}

/// The signatures of the call around a position of `file` whose content is `text` (the editor's
/// unsaved text), or the disk content (`SPEC-0080`).
pub async fn signature_help_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Option<SignatureHelp>, LspError> {
    let o = Opened::open_text(root, file, text, wait).await?;
    let answer = o
        .ask("textDocument/signatureHelp", o.position(line, character))
        .await;
    o.close().await;
    answer.map(|v| parse_signature_help(&v))
}

/// The call hierarchy of the function at a position of `file` whose content is `text` (the editor's
/// unsaved text), or the disk content: callers or callees, `depth` levels deep (`SPEC-0082`).
/// `None` when there is no function at the position.
#[allow(clippy::too_many_arguments)]
pub async fn call_hierarchy_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    direction: CallDirection,
    depth: usize,
    wait: Duration,
) -> Result<Option<CallNode>, LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let items = o
            .ask(
                "textDocument/prepareCallHierarchy",
                o.position(line, character),
            )
            .await?;
        let Some(item) = items.as_array().and_then(|a| a.first()).cloned() else {
            return Ok(None);
        };
        let mut budget = CALL_NODE_LIMIT;
        let mut ancestors = Vec::new();
        expand_calls(
            &mut o,
            item,
            Vec::new(),
            direction,
            depth,
            &mut ancestors,
            &mut budget,
        )
        .await
        .map(Some)
    }
    .await;
    o.close().await;
    result
}

/// Identity of a hierarchy item: file and start of its name.
fn item_key(item: &Value) -> (String, u64, u64) {
    (
        item["uri"].as_str().unwrap_or_default().to_owned(),
        item.pointer("/selectionRange/start/line")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        item.pointer("/selectionRange/start/character")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
}

/// Builds the node of `item` and, while `depth` and `budget` last, its children.
fn expand_calls<'a>(
    o: &'a mut Opened,
    item: Value,
    call_sites: Vec<Range>,
    direction: CallDirection,
    depth: usize,
    ancestors: &'a mut Vec<(String, u64, u64)>,
    budget: &'a mut usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<CallNode, LspError>> + Send + 'a>> {
    Box::pin(async move {
        *budget = budget.saturating_sub(1);
        let uri = item["uri"].as_str().unwrap_or_default().to_owned();
        let key = item_key(&item);
        let recursive = ancestors.contains(&key);
        let range = item
            .get("selectionRange")
            .and_then(|r| parse_range(r, &uri, &mut o.files))
            .unwrap_or(Range {
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 0,
            });
        let mut node = CallNode {
            name: item["name"].as_str().unwrap_or_default().to_owned(),
            kind: kind_name(&SYMBOL_KINDS, item.get("kind")).unwrap_or_else(|| "unknown".into()),
            detail: item["detail"].as_str().map(str::to_owned),
            location: Location {
                path: uri_to_path(&uri).unwrap_or_default(),
                range,
            },
            call_sites,
            children: Vec::new(),
            recursive,
            mark: None,
        };
        if depth == 0 || recursive || *budget == 0 {
            return Ok(node);
        }
        let (method, other) = match direction {
            CallDirection::Callers => ("callHierarchy/incomingCalls", "from"),
            CallDirection::Callees => ("callHierarchy/outgoingCalls", "to"),
        };
        let calls = o.ask(method, json!({"item": item})).await?;
        ancestors.push(key);
        for call in calls.as_array().into_iter().flatten() {
            if *budget == 0 {
                break;
            }
            let next = call[other].clone();
            // Call sites are written in the caller: the child for callers, this node for callees.
            let site_uri = match direction {
                CallDirection::Callers => next["uri"].as_str().unwrap_or_default().to_owned(),
                CallDirection::Callees => uri.clone(),
            };
            let sites = call["fromRanges"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| parse_range(r, &site_uri, &mut o.files))
                .collect();
            let child =
                expand_calls(o, next, sites, direction, depth - 1, ancestors, budget).await?;
            node.children.push(child);
        }
        ancestors.pop();
        Ok(node)
    })
}

/// Definition or references of the symbol at a position of `file`.
pub async fn locations(
    root: &Path,
    file: &Path,
    line: u64,
    character: u64,
    kind: LocationKind,
    wait: Duration,
) -> Result<Vec<Location>, LspError> {
    locations_in(root, file, None, line, character, kind, wait).await
}

/// Like [`locations`], on `text` instead of the saved file when given (unsaved editor text).
pub async fn locations_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    kind: LocationKind,
    wait: Duration,
) -> Result<Vec<Location>, LspError> {
    found_in(root, file, text, line, character, kind, wait)
        .await
        .map(|f| f.locations)
}

/// Like [`locations_in`], with the names of places outside files (library classes) as well.
pub async fn found_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    kind: LocationKind,
    wait: Duration,
) -> Result<Found, LspError> {
    // A class, an id and a jump link of an HTML file are answered from IDECraft's own syntax
    // trees, without a server (`SPEC-0042` v17 behaviours 28, 29 and 33).
    if kind == LocationKind::Definition && crate::html_css::is_html(file) {
        let own = match text {
            Some(t) => crate::html_css::declarations(root, file, t, line, character),
            None => std::fs::read_to_string(file)
                .ok()
                .and_then(|t| crate::html_css::declarations(root, file, &t, line, character)),
        };
        if let Some(locations) = own {
            return Ok(Found {
                locations,
                ..Found::default()
            });
        }
    }
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let mut params = o.position(line, character);
    let method = match kind {
        LocationKind::Definition => "textDocument/definition",
        LocationKind::References => {
            params["context"] = json!({"includeDeclaration": true});
            "textDocument/references"
        }
        LocationKind::TypeDefinition => "textDocument/typeDefinition",
        LocationKind::Implementation => "textDocument/implementation",
    };
    // The question has the caller's `wait` too, not the client's fixed limit: Go to Declaration
    // waits 30 s, an agent's tool 60 s, a test what it needs (`SPEC-0042` v21 behaviour 37,
    // `SPEC-0071` v11 behaviour 1). Until then `wait` bounded only the opening.
    let result = o
        .ask_bounded(method, params, wait, None)
        .await
        .map(|v| Found {
            locations: parse_locations(&v, &mut o.files),
            outside: outside_locations(&v),
            outside_places: outside_places(&v),
        });
    o.close().await;
    result
}

/// Completion proposals at a position of `file`.
pub async fn complete(
    root: &Path,
    file: &Path,
    line: u64,
    character: u64,
    limit: usize,
    wait: Duration,
) -> Result<Vec<CompletionItem>, LspError> {
    complete_in(root, file, None, line, character, limit, wait).await
}

/// Like [`complete`], on `text` instead of the saved file when given (unsaved editor text).
pub async fn complete_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    limit: usize,
    wait: Duration,
) -> Result<Vec<CompletionItem>, LspError> {
    // In a `class` or `id` value of an HTML file: the stylesheets' names, without a server
    // (`SPEC-0042` v22 behaviour 39).
    if crate::html_css::is_html(file) {
        let disk;
        let content = match text {
            Some(t) => Some(t),
            None => {
                disk = std::fs::read_to_string(file).ok();
                disk.as_deref()
            }
        };
        if let Some(items) = content
            .and_then(|t| crate::html_css::completions(root, file, t, line, character, limit))
        {
            return Ok(items);
        }
    }
    // Configuration keys come from the bean model, not from a language server (`SPEC-0115` v7
    // behaviour 19, `SPEC-0134` v6 behaviour 8).
    {
        let rel = file.strip_prefix(root).unwrap_or(file);
        let disk;
        let source = match text {
            Some(t) => Some(t),
            None if crate::config_completion::is_config_path(rel)
                || rel.extension().is_some_and(|e| e == "java") =>
            {
                disk = std::fs::read_to_string(file).ok();
                disk.as_deref()
            }
            None => None,
        };
        if let Some(source) = source
            && crate::config_completion::is_key_site(rel, source, line, character)
        {
            let model = crate::spring::project_model(root);
            if let Some(items) = crate::config_completion::key_completions(
                &model, rel, source, line, character, limit,
            ) {
                return Ok(items);
            }
        }
    }
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let language = LanguageId::detect(file);
    let script = matches!(
        language,
        Some(LanguageId::TypeScript | LanguageId::JavaScript | LanguageId::Python)
    );
    let mut result = o
        .ask("textDocument/completion", o.position(line, character))
        .await
        .map(|v| parse_completion(&v, limit));
    // typescript-language-server answers with an empty list while tsserver still loads the
    // project after a start, pyright while it still analyses the file; asked again for up to ten
    // seconds (`SPEC-0072` v8, headless calls; v9 behaviour 36: the first list after a start).
    let mut tries = 0;
    while script && tries < 20 && o.just_started() && result.as_ref().is_ok_and(Vec::is_empty) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        tries += 1;
        result = o
            .ask("textDocument/completion", o.position(line, character))
            .await
            .map(|v| parse_completion(&v, limit));
    }
    let result = result.map(|mut items| {
        let uri = o.uri.clone();
        for item in &mut items {
            if let Some(raw) = &item.raw {
                (item.edit_range, item.additional_edits) =
                    completion_ranges(raw, &uri, &mut o.files);
            }
        }
        // TypeScript and JavaScript: call parentheses and IDECraft's postfix templates
        // (`SPEC-0072` v8 behaviours 29–30).
        if matches!(
            LanguageId::detect(file),
            Some(LanguageId::TypeScript | LanguageId::JavaScript)
        ) {
            let line_text = line_of(&o.text, line).to_owned();
            crate::script_completion::with_call_parentheses(
                &mut items,
                &line_text,
                character as usize,
            );
            items.extend(crate::script_completion::postfix_items(
                &line_text,
                line,
                character as usize,
            ));
            items.truncate(limit);
        }
        // Python: call parentheses, IDECraft's postfix and live templates and the standard
        // library names the server does not offer (`SPEC-0072` v9 behaviours 37–41).
        if language == Some(LanguageId::Python) {
            let line_text = line_of(&o.text, line).to_owned();
            let at = character as usize;
            use crate::python_completion as py;
            py::with_call_parentheses(&mut items, &line_text, at);
            let std = py::stdlib_items(py::installed_stdlib(), &o.text, line, at, &items, 50);
            items.extend(std);
            items.extend(py::postfix_items(&line_text, line, at));
            items.extend(py::live_items(&line_text, at));
            items.truncate(limit);
        }
        items
    });
    o.close().await;
    result
}

/// What resolving a completion item added: its detail and documentation (`SPEC-0072` v8
/// behaviour 28).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Resolved {
    /// The detail, e.g. `(property) Item.qty: number`.
    pub detail: Option<String>,
    /// The documentation as Markdown (Java: repaired, behaviour 27).
    pub documentation: Option<String>,
}

/// Resolves several completion `items` of `file` whose content is `text` in one server question
/// (`SPEC-0072` v8 behaviour 28): one entry per item, `None` for items the server does not resolve.
pub async fn resolve_details_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    items: &[CompletionItem],
    wait: Duration,
) -> Result<Vec<Option<Resolved>>, LspError> {
    resolve_details_within(root, file, text, items, wait, None).await
}

/// [`resolve_details_in`] that stops asking once `budget` is used up (`SPEC-0072` v10 behaviour
/// 46): the items are resolved in order, those the time did not reach get `None`.
pub async fn resolve_details_within(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    items: &[CompletionItem],
    wait: Duration,
    budget: Option<Duration>,
) -> Result<Vec<Option<Resolved>>, LspError> {
    if items.iter().all(|i| i.raw.is_none()) {
        return Ok(vec![None; items.len()]);
    }
    let o = Opened::open_text(root, file, text, wait).await?;
    // The budget counts the questions, not the start of a server.
    let started = std::time::Instant::now();
    let spent = || budget.is_some_and(|b| started.elapsed() >= b);
    let resolves = o.capabilities["completionProvider"]["resolveProvider"] == true;
    let java = LanguageId::detect(file) == Some(LanguageId::Java);
    let python = LanguageId::detect(file) == Some(LanguageId::Python);
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(raw) = item.raw.clone().filter(|_| resolves && !spent()) else {
            out.push(None);
            continue;
        };
        let asked = match budget {
            Some(b) => {
                let left = b.saturating_sub(started.elapsed());
                o.ask_bounded("completionItem/resolve", raw, left, None)
                    .await
            }
            None => o.ask("completionItem/resolve", raw).await,
        };
        match asked {
            Ok(v) => {
                let raw_doc = documentation_text(v.get("documentation"));
                let mut detail = v
                    .get("detail")
                    .and_then(Value::as_str)
                    .filter(|d| !d.trim().is_empty())
                    .map(str::to_owned);
                // pyright puts the declaration only into the documentation's code block
                // (`SPEC-0072` v9 behaviour 34); classes keep no detail (their bases are no
                // signature), and neither do the methods a dataclass generates, for which
                // pyright writes `undefined` (v10 behaviour 44).
                if python && detail.as_deref().is_none_or(|d| d == "Auto-import") {
                    detail = raw_doc
                        .as_deref()
                        .and_then(crate::python_completion::declaration)
                        .filter(|d| !d.starts_with("class ") && d != "undefined")
                        .or(detail);
                }
                let documentation = raw_doc.map(|d| {
                    if java {
                        crate::javadoc::repair(item, &d, Some(root))
                    } else if python {
                        crate::python_completion::tidy_documentation(&d)
                    } else {
                        d
                    }
                });
                out.push(Some(Resolved {
                    detail,
                    documentation,
                }));
            }
            Err(_) => out.push(None),
        }
    }
    o.close().await;
    Ok(out)
}

/// The additional edits of a completion `item` on `file` whose content is `text` (`SPEC-0072` v7
/// behaviour 20): those the item brought, or else those `completionItem/resolve` adds (servers
/// that compute imports only for the chosen item, e.g. TypeScript).
pub async fn resolve_completion_edits_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    item: &CompletionItem,
    wait: Duration,
) -> Result<Vec<CompletionEdit>, LspError> {
    if !item.additional_edits.is_empty() {
        return Ok(item.additional_edits.clone());
    }
    let Some(raw) = item.raw.clone() else {
        return Ok(Vec::new());
    };
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let resolves = o.capabilities["completionProvider"]["resolveProvider"] == true;
    let result = if resolves {
        match o.ask("completionItem/resolve", raw).await {
            Ok(v) => {
                let uri = o.uri.clone();
                Ok(additional_edits(&v, &uri, &mut o.files))
            }
            Err(e) => Err(e),
        }
    } else {
        Ok(Vec::new())
    };
    o.close().await;
    result
}

/// The documentation of a completion `item` that the server left out of the list, asked for with
/// `completionItem/resolve` on `file` whose content is `text` (`SPEC-0072` v5 behaviour 13).
/// `None` when the server does not resolve items or has none.
pub async fn resolve_completion_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    item: &CompletionItem,
    wait: Duration,
) -> Result<Option<String>, LspError> {
    // jdtls garbles some Javadoc; it is repaired here (`SPEC-0072` v8 behaviour 27).
    // Python overloads are collapsed to the first (`SPEC-0072` v9 behaviour 42).
    let repair = |d: String| match LanguageId::detect(file) {
        Some(LanguageId::Java) => crate::javadoc::repair(item, &d, Some(root)),
        Some(LanguageId::Python) => crate::python_completion::tidy_documentation(&d),
        _ => d,
    };
    if let Some(d) = &item.documentation {
        return Ok(Some(repair(d.clone())));
    }
    let Some(raw) = item.raw.clone() else {
        return Ok(None);
    };
    let o = Opened::open_text(root, file, text, wait).await?;
    let resolves = o.capabilities["completionProvider"]["resolveProvider"] == true;
    let result = if resolves {
        o.ask("completionItem/resolve", raw)
            .await
            .map(|v| documentation_text(v.get("documentation")).map(repair))
    } else {
        Ok(None)
    };
    o.close().await;
    result
}

/// Symbols of `file` in document order.
pub async fn symbols(root: &Path, file: &Path, wait: Duration) -> Result<Vec<Symbol>, LspError> {
    symbols_in(root, file, None, wait).await
}

/// Symbols of `file` whose content is `text` (the editor's unsaved text), or the disk content
/// (`SPEC-0079`).
pub async fn symbols_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<Symbol>, LspError> {
    // The elements of an HTML file, nested, from IDECraft's own syntax tree and without a server
    // (`SPEC-0042` v17 behaviours 31 and 33).
    if crate::html_css::is_html(file) {
        return match text {
            Some(t) => Ok(crate::html_css::structure(t)),
            None => std::fs::read_to_string(file)
                .map(|t| crate::html_css::structure(&t))
                .map_err(|e| LspError::Io(format!("{}: {e}", file.display()))),
        };
    }
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let uri = o.uri.clone();
    let result = o
        .ask(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .await
        .map(|v| parse_symbols(&v, &uri, &mut o.files))
        .map(|symbols| {
            // `SPEC-0079` v5 behaviour 12 and v6 behaviour 13: no parameters and locals,
            // functions with their parameter list.
            if LanguageId::detect(file) == Some(LanguageId::Python) {
                crate::python_structure::structure(&o.text, symbols)
            } else if crate::script_structure::applies(file) {
                crate::script_structure::structure(file, &o.text, symbols)
            } else if LanguageId::detect(file) == Some(LanguageId::Php) {
                // intelephense lists a function's parameters below it (`SPEC-0079` v8
                // behaviour 15).
                php_structure(symbols)
            } else {
                symbols
            }
        });
    o.close().await;
    result
}

/// The symbols of a PHP file as the structure shows them: below a function or method only nested
/// functions and types, no parameters or variables (`SPEC-0079` v8 behaviour 15).
fn php_structure(symbols: Vec<Symbol>) -> Vec<Symbol> {
    crate::python_structure::without_locals(
        symbols,
        |s| matches!(s.kind.as_str(), "function" | "method" | "constructor"),
        |s| matches!(s.kind.as_str(), "class" | "interface" | "enum" | "struct"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0079 v8 AK-32: a PHP function's parameters and variables are left out; a method's
    /// too; properties, constants and nested types stay.
    #[test]
    fn php_structure_without_parameters() {
        let s = |name: &str, kind: &str, depth: usize| Symbol {
            name: name.into(),
            kind: kind.into(),
            depth,
            ..Default::default()
        };
        let symbols = vec![
            s("greet", "function", 0),
            s("$name", "variable", 1),
            s("Greeter", "class", 0),
            s("$prefix", "property", 1),
            s("LIMIT", "constant", 1),
            s("hello", "method", 1),
            s("$who", "variable", 2),
            s("$x", "variable", 0),
        ];
        let names: Vec<String> = php_structure(symbols).into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            ["greet", "Greeter", "$prefix", "LIMIT", "hello", "$x"]
        );
    }

    /// SPEC-0076 v8 AK-9: jdtls's list from its build (problems at character 0 with no width)
    /// is waited past; its refined list, an empty list and a list with one exact range are not.
    #[test]
    fn the_build_list_of_jdtls_is_told_by_its_ranges() {
        let at = |line: u64, start: u64, end: u64| {
            json!({"range": {"start": {"line": line, "character": start},
                             "end": {"line": line, "character": end}}, "message": "m"})
        };
        assert!(from_the_build(&[at(4, 0, 0), at(8, 0, 0)]));
        assert!(!from_the_build(&[at(4, 12, 18), at(8, 22, 23)]));
        assert!(!from_the_build(&[]));
        assert!(!from_the_build(&[at(0, 0, 0), at(8, 22, 23)]));
    }

    /// SPEC-0042 v20 AK-37: what a scripted server sends, in order; `None` a pause of a minute.
    async fn diagnostics_after(publishes_twice: bool, script: &[Option<Value>]) -> Vec<Value> {
        diagnostics_scripted(publishes_twice, false, script).await
    }

    /// [`diagnostics_after`] for a server that publishes in steps (`in_steps`) or twice.
    async fn diagnostics_scripted(
        publishes_twice: bool,
        in_steps: bool,
        script: &[Option<Value>],
    ) -> Vec<Value> {
        use tokio::io::{AsyncWriteExt, duplex};
        let (client_out, _server_in) = duplex(1 << 16);
        let (mut server_out, client_in) = duplex(1 << 16);
        let client = LspClient::new(client_in, client_out, None::<tokio::io::DuplexStream>);
        let mut o = Opened::over(client, Path::new("/p/src/Report.php"), "<?php\n");
        o.publishes_twice = publishes_twice;
        o.publishes_in_steps = in_steps;
        let script = script.to_vec();
        tokio::spawn(async move {
            for step in script {
                match step {
                    Some(n) => {
                        let body =
                            json!({"jsonrpc": "2.0", "method": n["method"], "params": n["params"]})
                                .to_string();
                        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
                        server_out.write_all(framed.as_bytes()).await.unwrap();
                    }
                    None => tokio::time::sleep(Duration::from_secs(60)).await,
                }
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        });
        o.diagnostics(Duration::from_secs(120)).await
    }

    fn published(problems: &[&str]) -> Option<Value> {
        let list: Vec<Value> = problems.iter().map(|m| json!({"message": m})).collect();
        Some(json!({"method": "textDocument/publishDiagnostics",
                    "params": {"uri": "file:///p/src/Report.php", "diagnostics": list}}))
    }

    fn said(method: &str) -> Option<Value> {
        Some(json!({"method": method, "params": {}}))
    }

    fn messages(list: &[Value]) -> Vec<&str> {
        list.iter().filter_map(|d| d["message"].as_str()).collect()
    }

    /// SPEC-0042 v20 AK-37: intelephense's second list is the answer, also when the first was
    /// empty; without a second the first after the wait; a list during an indexing is none; a
    /// server that publishes once is answered by its first list.
    #[tokio::test(start_paused = true)]
    async fn the_second_list_of_a_server_that_publishes_twice_is_the_answer() {
        let checked = ["Undefined method 'missingMethod'."];
        let twice = diagnostics_after(true, &[published(&[]), published(&checked)]).await;
        assert_eq!(messages(&twice), checked);
        let once = diagnostics_after(true, &[published(&checked), None]).await;
        assert_eq!(messages(&once), checked);
        let indexing = diagnostics_after(
            true,
            &[
                said("indexingStarted"),
                published(&[]),
                said("indexingEnded"),
                published(&checked),
            ],
        )
        .await;
        assert_eq!(messages(&indexing), checked);
        let late = diagnostics_after(
            true,
            &[
                published(&[]),
                said("indexingStarted"),
                None,
                said("indexingEnded"),
                published(&checked),
            ],
        )
        .await;
        assert_eq!(messages(&late), checked);
        let other = diagnostics_after(false, &[published(&[]), published(&checked)]).await;
        assert!(other.is_empty());
    }

    /// SPEC-0042 v23 AK-45: typescript-language-server's last list of up to three is the
    /// answer; a list followed by a long pause is the answer; three lists end the wait at once.
    #[tokio::test(start_paused = true)]
    async fn the_last_step_of_a_server_that_publishes_in_steps_is_the_answer() {
        let semantic = ["Cannot find name 'helper'."];
        let all = [
            "Cannot find name 'helper'.",
            "'unused' is declared but never read.",
        ];
        let steps = diagnostics_scripted(
            false,
            true,
            &[published(&[]), published(&semantic), published(&all)],
        )
        .await;
        assert_eq!(messages(&steps), all);
        // Two kinds merged into one list: the second list, after the quiet time.
        let merged = diagnostics_scripted(false, true, &[published(&[]), published(&all)]).await;
        assert_eq!(messages(&merged), all);
        // Nothing more within the quiet time: the list so far.
        let paused =
            diagnostics_scripted(false, true, &[published(&semantic), None, published(&all)]).await;
        assert_eq!(messages(&paused), semantic);
        // Without the property the first list stays the answer.
        let once = diagnostics_scripted(false, false, &[published(&[]), published(&all)]).await;
        assert!(once.is_empty());
    }

    /// SPEC-0141/AK-9 (v4): the Vue server's generated context is recognised, a symbol whose type
    /// mentions a generated name is not; the next character must still be part of the name.
    #[test]
    fn vue_internal_hovers_are_recognised() {
        assert!(vue_internal(
            "```typescript\nconst __VLS_ctx: CreateComponentPublicInstance<{}>\n```"
        ));
        assert!(vue_internal(
            "```typescript\nconst __VLS_ctx: any\n```\n\n---\n\n```typescript\nany\n```"
        ));
        assert!(!vue_internal(
            "```typescript\nconst defineProps: <__VLS_Props>() => DefineProps<__VLS_Props>\n```"
        ));
        assert!(!vue_internal("```typescript\n(property) msg: string\n```"));
        assert!(!vue_internal(""));
        let line = "    <h1 class=\"green\">{{ msg }}</h1>";
        assert!(next_in_word(line, 25));
        assert!(next_in_word(line, 26));
        assert!(!next_in_word(line, 27), "g is the last letter");
        assert!(!next_in_word(line, 24), "a space");
        assert!(!next_in_word(line, 200));
    }

    #[test]
    fn places_outside_files_are_named() {
        let range =
            json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}});
        let v = json!([
            {"uri": "jdt://contents/java.base/java.lang/String.class?=jre/%3Cjava.lang(String.class", "range": range},
            {"uri": "jdt://contents/guava-33.jar/com.google.common.base/Strings$Inner.class?x", "range": range},
            {"uri": "jdt://contents/lib.jar//Top.class", "range": range},
            {"uri": "jdt://contents/java.base/java.util/List.java", "range": range},
            {"uri": "file:///tmp/A.java", "range": range},
        ]);
        assert_eq!(
            outside_locations(&v),
            [
                "java.lang.String",
                "com.google.common.base.Strings",
                "Top",
                "java.util.List"
            ]
        );
        let one = json!({"targetUri": "zipfile:///x.zip::A.java", "targetRange": range});
        assert_eq!(outside_locations(&one), ["zipfile:///x.zip::A.java"]);
        assert!(outside_locations(&Value::Null).is_empty());
    }

    #[test]
    fn parses_signature_help_with_offsets_and_names() {
        let v = json!({
            "signatures": [
                {"label": "int add(int a, int b)", "parameters": [
                    {"label": [8, 13]}, {"label": [15, 20]}
                ], "documentation": {"kind": "plaintext", "value": "Adds."}},
                {"label": "f(x, xx)", "parameters": [{"label": "x"}, {"label": "xx"}],
                 "activeParameter": 1}
            ],
            "activeSignature": 0,
            "activeParameter": 1
        });
        let h = parse_signature_help(&v).unwrap();
        assert_eq!(h.active_signature, 0);
        let add = &h.signatures[0];
        assert_eq!(add.parameters, [8..13, 15..20]);
        assert_eq!(add.active_parameter, Some(1));
        assert_eq!(add.documentation.as_deref(), Some("Adds."));
        let f = &h.signatures[1];
        assert_eq!(f.parameters, [2..3, 5..7]);
        assert_eq!(f.active_parameter, Some(1));
        assert!(parse_signature_help(&json!({"signatures": []})).is_none());
        assert!(parse_signature_help(&Value::Null).is_none());
    }

    #[test]
    fn position_conversion() {
        let line = "aü€😀b";
        for (enc, units) in [
            (Encoding::Utf8, [0, 1, 3, 6, 10, 11]),
            (Encoding::Utf16, [0, 1, 2, 3, 5, 6]),
            (Encoding::Utf32, [0, 1, 2, 3, 4, 5]),
        ] {
            for (chars, u) in units.iter().enumerate() {
                assert_eq!(to_server(line, chars as u64, enc), *u, "{enc:?} {chars}");
                assert_eq!(from_server(line, *u, enc), chars as u64, "{enc:?} {u}");
            }
            assert_eq!(to_server(line, 99, enc), units[5], "clamped");
            assert_eq!(from_server(line, 999, enc), 5, "clamped");
        }
        // Inside a multi-unit character: rounds down to its start.
        assert_eq!(from_server(line, 4, Encoding::Utf16), 3);
        assert_eq!(
            Encoding::from_capabilities(&json!({"positionEncoding": "utf-8"})),
            Encoding::Utf8
        );
        assert_eq!(Encoding::from_capabilities(&json!({})), Encoding::Utf16);
        assert_eq!(line_of("a\r\nb€\nc", 1), "b€");
    }

    /// Converts as if every file were the one line `x€y`, in UTF-16.
    struct Fixed;
    impl Converter for Fixed {
        fn character(&mut self, _uri: &str, _line: u64, units: u64) -> u64 {
            from_server("x€y", units, Encoding::Utf8)
        }
    }

    fn r(sl: u64, sc: u64, el: u64, ec: u64) -> Value {
        json!({"start": {"line": sl, "character": sc}, "end": {"line": el, "character": ec}})
    }

    #[test]
    fn parses_hover_and_locations() {
        let h = parse_hover(
            &json!({"contents": {"kind": "markdown", "value": "### add"}, "range": r(1, 0, 1, 4)}),
            "file:///a.c",
            &mut Fixed,
        )
        .unwrap();
        assert_eq!(h.text, "### add");
        assert_eq!(h.range.unwrap().end_character, 2, "byte 4 = after €");
        let h = parse_hover(
            &json!({"contents": ["int add()", {"language": "c", "value": "x"}]}),
            "u",
            &mut Fixed,
        )
        .unwrap();
        assert_eq!(h.text, "int add()\n\nx");
        assert!(parse_hover(&json!({"contents": ""}), "u", &mut Fixed).is_none());

        let single = parse_locations(
            &json!({"uri": "file:///a%20b.c", "range": r(0, 0, 0, 1)}),
            &mut Fixed,
        );
        assert_eq!(single[0].path, PathBuf::from("/a b.c"));
        let links = parse_locations(
            &json!([{"targetUri": "file:///x.c", "targetRange": r(0, 0, 3, 0), "targetSelectionRange": r(1, 1, 1, 2)}]),
            &mut Fixed,
        );
        assert_eq!(links[0].range.start_line, 1);
        assert_eq!(parse_locations(&Value::Null, &mut Fixed), []);
        // Hand test 110 (D61): ruby-lsp names one file `root//greeter.rb` and `root/greeter.rb`.
        let twice = parse_locations(
            &json!([{"uri": "file:///w//greeter.rb", "range": r(0, 6, 0, 13)},
                    {"uri": "file:///w/./greeter.rb", "range": r(0, 6, 0, 13)},
                    {"uri": "file:///w/greeter.rb", "range": r(4, 0, 4, 1)}]),
            &mut Fixed,
        );
        assert_eq!(twice.len(), 2, "{twice:?}");
        assert_eq!(twice[0].path, PathBuf::from("/w/greeter.rb"));
    }

    /// SPEC-0072 v17 behaviour 54: an unsorted list is ranked before the limit cuts it.
    #[test]
    fn completion_is_ranked_before_the_limit() {
        // rust-analyzer after `r.`: trait methods and snippets before the type's own members.
        let list = json!([
            {"label": "into()", "sortText": "7ffffffb"},
            {"label": "try_into()", "sortText": "7ffffffb"},
            {"label": "area()", "sortText": "7ffffff6", "preselect": true},
            {"label": "plain"},
            {"label": "w", "sortText": "7ffffff6", "preselect": true},
            // Case is ignored first: `7FFFFFFC` comes after `7ffffffb`.
            {"label": "Box", "sortText": "7FFFFFFC"},
        ]);
        let labels = |n| -> Vec<String> {
            parse_completion(&list, n)
                .into_iter()
                .map(|i| i.label)
                .collect()
        };
        assert_eq!(
            labels(9),
            ["area()", "w", "into()", "try_into()", "Box", "plain"]
        );
        // intelephense: the project's `greet` among a hundred stub functions is kept.
        let mut many: Vec<Value> = (0..150)
            .map(|n| json!({"label": format!("grapheme_{n:03}"), "sortText": format!("0002grapheme_{n:03}")}))
            .collect();
        many.push(json!({"label": "greet", "sortText": "0001greet"}));
        let kept = parse_completion(&Value::Array(many), 100);
        assert_eq!(kept.len(), 100);
        assert_eq!(kept[0].label, "greet");
    }

    #[test]
    fn parses_completion_and_symbols() {
        let list = json!({"isIncomplete": false, "items": [
            {"label": " add", "kind": 3, "detail": "int", "textEdit": {"newText": "add(", "range": r(0, 0, 0, 2)}},
            {"label": "auto", "kind": 14},
            {"label": "x", "insertText": "x_"}]});
        let items = parse_completion(&list, 2);
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0],
            CompletionItem {
                label: "add".into(),
                kind: Some("function".into()),
                detail: Some("int".into()),
                insert: "add(".into(),
                raw: items[0].raw.clone(),
                ..Default::default()
            }
        );
        // Label details, ranking and documentation (SPEC-0072 v5 behaviours 10, 12, 13).
        let rich = parse_completion(
            &json!({"items": [{"label": "of", "kind": 2, "sortText": "0001", "preselect": true,
                "labelDetails": {"detail": "(E... elements)", "description": "List<E>"},
                "documentation": {"kind": "markdown", "value": "Returns a list."}},
                {"label": "x", "documentation": " "}]}),
            9,
        );
        assert_eq!(rich[0].label_detail.as_deref(), Some("(E... elements)"));
        assert_eq!(rich[0].description.as_deref(), Some("List<E>"));
        assert_eq!(rich[0].sort_text.as_deref(), Some("0001"));
        assert!(rich[0].preselect && !rich[1].preselect);
        assert_eq!(rich[0].documentation.as_deref(), Some("Returns a list."));
        assert_eq!(rich[1].documentation, None);
        assert!(rich[0].raw.is_some());
        assert_eq!(items[1].insert, "auto");
        assert_eq!(
            parse_completion(&json!([{"label": "x", "insertText": "x_"}]), 9)[0].insert,
            "x_"
        );
        // Snippets, edit range and additional edits (SPEC-0072 v7 behaviours 18-20).
        let postfix = json!({"label": "var", "insertTextFormat": 2,
            "textEdit": {"newText": "int ${1:n} = s.length();", "range": r(4, 0, 4, 4)},
            "additionalTextEdits": [{"newText": "", "range": r(0, 0, 0, 0)},
                {"newText": "import a.B;\n", "range": r(1, 0, 1, 0)}]});
        let parsed = parse_completion(&json!([postfix.clone()]), 9);
        assert!(parsed[0].snippet);
        assert_eq!(parsed[0].insert, "int ${1:n} = s.length();");
        let (edit, more) = completion_ranges(&postfix, "u", &mut Fixed);
        assert_eq!(
            edit.map(|e| (e.start_character, e.end_character)),
            Some((0, 2)),
            "bytes of `x€` in the fixed converter"
        );
        assert_eq!(more.len(), 1, "the empty edit is left out");
        assert_eq!(more[0].text, "import a.B;\n");
        let replace = json!({"textEdit": {"newText": "x", "insert": r(0, 0, 0, 1), "replace": r(0, 0, 0, 4)}});
        assert_eq!(
            completion_ranges(&replace, "u", &mut Fixed)
                .0
                .map(|e| e.end_character),
            Some(1)
        );

        let tree = json!([{"name": "S", "kind": 23, "range": r(0, 0, 3, 1), "selectionRange": r(0, 7, 0, 8),
            "children": [{"name": "f", "kind": 8, "range": r(1, 2, 1, 9), "selectionRange": r(1, 6, 1, 7)}]},
            {"name": "main", "kind": 12, "detail": "int ()", "range": r(4, 0, 4, 9), "selectionRange": r(4, 4, 4, 8)}]);
        let symbols = parse_symbols(&tree, "u", &mut Fixed);
        let flat: Vec<(&str, &str, usize)> = symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind.as_str(), s.depth))
            .collect();
        assert_eq!(
            flat,
            [
                ("S", "struct", 0),
                ("f", "field", 1),
                ("main", "function", 0)
            ]
        );
        let info = parse_symbols(
            &json!([{"name": "g", "kind": 13, "location": {"uri": "u", "range": r(2, 0, 2, 1)}}]),
            "u",
            &mut Fixed,
        );
        assert_eq!((info[0].kind.as_str(), info[0].line), ("variable", 2));
    }
}
