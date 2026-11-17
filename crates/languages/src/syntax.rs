//! Syntax highlighting with Tree-sitter (`SPEC-0027`).
//!
//! Also folding regions (`SPEC-0030`). A [`SyntaxDocument`] keeps the text and its syntax tree; edits re-parse
//! incrementally and report which ranges changed. Highlighting runs the
//! grammar's `highlights.scm` query over a byte range and resolves overlaps:
//! the innermost node wins, for the same node the earlier pattern wins.

use std::ops::Range;
use std::sync::OnceLock;

use serde::Serialize;
use tree_sitter::{
    InputEdit, Language, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

use crate::LanguageId;

/// A coloured run of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// Start char offset.
    pub start: usize,
    /// End char offset (exclusive).
    pub end: usize,
    /// Capture name, e.g. `keyword`.
    pub kind: String,
}

/// Why a document cannot be highlighted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyntaxError {
    /// No grammar for the language yet.
    #[error("no syntax highlighting for {0} yet")]
    Unsupported(String),
    /// The edit range lies outside the text.
    #[error("edit range {0:?} is outside the text")]
    OutOfBounds(Range<usize>),
}

/// Grammar and query of one language.
struct Grammar {
    language: Language,
    query: Query,
    /// The query's source, for [`Grammar::comments`].
    source: String,
    comments: OnceLock<Query>,
}

impl Grammar {
    /// The highlight query with only the patterns that capture a comment (`comment…`) enabled:
    /// the same comments as the full query, found in a fraction of the time.
    fn comments(&self) -> &Query {
        self.comments.get_or_init(|| {
            let mut query =
                Query::new(&self.language, &self.source).expect("bundled highlight query compiles");
            let comment: Vec<usize> = query
                .capture_names()
                .iter()
                .enumerate()
                .filter(|(_, n)| n.starts_with("comment"))
                .map(|(i, _)| i)
                .collect();
            for pattern in 0..query.pattern_count() {
                let quantifiers = query.capture_quantifiers(pattern);
                let captures_comment = comment.iter().any(|&c| {
                    quantifiers
                        .get(c)
                        .is_some_and(|q| *q != tree_sitter::CaptureQuantifier::Zero)
                });
                if !captures_comment {
                    query.disable_pattern(pattern);
                }
            }
            query
        })
    }
}

/// A variable declared in SCSS (`$farbe: …`) or Less (`@farbe: …`) is a variable, not a property.
const STYLE_VARIABLES: &str = "((property_name) @variable (#match? @variable \"^[$@]\"))";

/// IDECraft's colour roles per language (`SPEC-0027` v6), placed before the grammar's query.
const PYTHON_ROLES: &str = include_str!("../queries/python-roles.scm");
const JAVASCRIPT_ROLES: &str = include_str!("../queries/javascript-roles.scm");
const TYPESCRIPT_ROLES: &str = include_str!("../queries/typescript-roles.scm");
const SQL_ROLES: &str = include_str!("../queries/sql-roles.scm");
/// Roles of languages whose bundled query names every identifier or scalar first (`SPEC-0027`
/// v13).
const C_ROLES: &str = include_str!("../queries/c-roles.scm");
const CPP_ROLES: &str = include_str!("../queries/cpp-roles.scm");
const YAML_ROLES: &str = include_str!("../queries/yaml-roles.scm");
const RUBY_ROLES: &str = include_str!("../queries/ruby-roles.scm");
const LUA_ROLES: &str = include_str!("../queries/lua-roles.scm");
const SWIFT_ROLES: &str = include_str!("../queries/swift-roles.scm");
const R_ROLES: &str = include_str!("../queries/r-roles.scm");

/// Supported languages (`SPEC-0027` behaviour 1); `tsx` selects TSX for TypeScript files.
fn grammar(language: LanguageId, tsx: bool) -> Option<&'static Grammar> {
    macro_rules! cached {
        ($lang:expr, $($query:expr),+) => {{
            static CELL: OnceLock<Grammar> = OnceLock::new();
            CELL.get_or_init(|| {
                let language: Language = $lang.into();
                let source = [$($query),+].join("\n");
                let query = Query::new(&language, &source).expect("bundled highlight query compiles");
                Grammar { language, query, source, comments: OnceLock::new() }
            })
        }};
    }
    Some(match language {
        // IDECraft's colour roles come first (`SPEC-0027` v6): for the same node the earlier
        // pattern wins.
        LanguageId::Python => cached!(
            tree_sitter_python::LANGUAGE,
            PYTHON_ROLES,
            tree_sitter_python::HIGHLIGHTS_QUERY
        ),
        LanguageId::JavaScript => cached!(
            tree_sitter_javascript::LANGUAGE,
            JAVASCRIPT_ROLES,
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        ),
        LanguageId::TypeScript if tsx => cached!(
            tree_sitter_typescript::LANGUAGE_TSX,
            TYPESCRIPT_ROLES,
            JAVASCRIPT_ROLES,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        ),
        LanguageId::TypeScript => cached!(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            TYPESCRIPT_ROLES,
            JAVASCRIPT_ROLES,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            tree_sitter_javascript::HIGHLIGHT_QUERY
        ),
        LanguageId::Java => cached!(
            tree_sitter_java::LANGUAGE,
            tree_sitter_java::HIGHLIGHTS_QUERY
        ),
        LanguageId::Rust => cached!(
            tree_sitter_rust::LANGUAGE,
            tree_sitter_rust::HIGHLIGHTS_QUERY
        ),
        LanguageId::Go => cached!(tree_sitter_go::LANGUAGE, tree_sitter_go::HIGHLIGHTS_QUERY),
        LanguageId::C => cached!(
            tree_sitter_c::LANGUAGE,
            C_ROLES,
            tree_sitter_c::HIGHLIGHT_QUERY
        ),
        // The C++ grammar extends C's; its highlight query adds to C's.
        LanguageId::Cpp => cached!(
            tree_sitter_cpp::LANGUAGE,
            C_ROLES,
            CPP_ROLES,
            tree_sitter_c::HIGHLIGHT_QUERY,
            tree_sitter_cpp::HIGHLIGHT_QUERY
        ),
        LanguageId::Sql => cached!(
            tree_sitter_sequel::LANGUAGE,
            SQL_ROLES,
            tree_sitter_sequel::HIGHLIGHTS_QUERY
        ),
        LanguageId::Json => cached!(
            tree_sitter_json::LANGUAGE,
            tree_sitter_json::HIGHLIGHTS_QUERY
        ),
        LanguageId::Html => cached!(
            tree_sitter_html::LANGUAGE,
            tree_sitter_html::HIGHLIGHTS_QUERY
        ),
        LanguageId::Css => cached!(tree_sitter_css::LANGUAGE, tree_sitter_css::HIGHLIGHTS_QUERY),
        LanguageId::Xml => cached!(
            tree_sitter_xml::LANGUAGE_XML,
            tree_sitter_xml::XML_HIGHLIGHT_QUERY
        ),
        LanguageId::Yaml => cached!(
            tree_sitter_yaml::LANGUAGE,
            YAML_ROLES,
            tree_sitter_yaml::HIGHLIGHTS_QUERY
        ),
        // Block structure only (headings, lists, code fences); inline emphasis needs the inline
        // grammar as an injection.
        LanguageId::Markdown => cached!(
            tree_sitter_md::LANGUAGE,
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK
        ),
        LanguageId::Toml => cached!(
            tree_sitter_toml_ng::LANGUAGE,
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY
        ),
        LanguageId::Shell => cached!(
            tree_sitter_bash::LANGUAGE,
            tree_sitter_bash::HIGHLIGHT_QUERY
        ),
        LanguageId::Dockerfile => cached!(
            tree_sitter_containerfile::LANGUAGE,
            tree_sitter_containerfile::HIGHLIGHTS_QUERY
        ),
        LanguageId::Groovy => cached!(
            tree_sitter_groovy::LANGUAGE,
            include_str!("../queries/groovy-highlights.scm")
        ),
        LanguageId::Kotlin => cached!(
            tree_sitter_kotlin_ng::LANGUAGE,
            include_str!("../queries/kotlin-highlights.scm")
        ),
        LanguageId::Scala => cached!(
            tree_sitter_scala::LANGUAGE,
            tree_sitter_scala::HIGHLIGHTS_QUERY
        ),
        // The SCSS and Less queries only add to CSS's (selectors, properties, colours, numbers
        // come from the CSS query; hand test 95).
        LanguageId::Scss => cached!(
            tree_sitter_scss::language(),
            STYLE_VARIABLES,
            tree_sitter_scss::HIGHLIGHTS_QUERY,
            tree_sitter_css::HIGHLIGHTS_QUERY
        ),
        LanguageId::Less => cached!(
            tree_sitter_less::language(),
            STYLE_VARIABLES,
            tree_sitter_less::HIGHLIGHTS_QUERY,
            tree_sitter_css::HIGHLIGHTS_QUERY
        ),
        LanguageId::Swift => cached!(
            tree_sitter_swift::LANGUAGE,
            SWIFT_ROLES,
            tree_sitter_swift::HIGHLIGHTS_QUERY
        ),
        LanguageId::Lua => cached!(
            tree_sitter_lua::LANGUAGE,
            LUA_ROLES,
            tree_sitter_lua::HIGHLIGHTS_QUERY
        ),
        LanguageId::R => cached!(
            tree_sitter_r::LANGUAGE,
            R_ROLES,
            tree_sitter_r::HIGHLIGHTS_QUERY
        ),
        // SPEC-0027 v5.
        LanguageId::Ruby => cached!(
            tree_sitter_ruby::LANGUAGE,
            RUBY_ROLES,
            tree_sitter_ruby::HIGHLIGHTS_QUERY
        ),
        LanguageId::Php => cached!(
            tree_sitter_php::LANGUAGE_PHP,
            tree_sitter_php::HIGHLIGHTS_QUERY
        ),
        // Vue and Svelte components are HTML with their own syntax inside (`SPEC-0141`).
        LanguageId::Vue | LanguageId::Svelte => cached!(
            tree_sitter_html::LANGUAGE,
            tree_sitter_html::HIGHLIGHTS_QUERY
        ),
    })
}

/// Texts larger than this many bytes are parsed on a background thread when opened, so opening
/// never waits for the tree (`SPEC-0260` behaviour 4).
pub const BACKGROUND_PARSE_BYTES: usize = 1024 * 1024;

/// Texts larger than this many bytes get no syntax tree: a tree takes up to ~45 bytes per byte of
/// text (a 50 MB SQL dump: 2.2 GB), so the editor shows them as plain text (`SPEC-0260`
/// behaviour 4).
pub const MAX_SYNTAX_BYTES: usize = 8 * 1024 * 1024;

/// A syntax tree with the text it was parsed from, taken from a [`SyntaxDocument`].
#[derive(Clone)]
pub struct TreeSnapshot {
    tree: Tree,
    text: String,
}

impl TreeSnapshot {
    /// The text of the tree.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn tree(&self) -> &Tree {
        &self.tree
    }
}

/// Text plus syntax tree of one document.
pub struct SyntaxDocument {
    language: LanguageId,
    tsx: bool,
    grammar: &'static Grammar,
    parser: Parser,
    tree: Tree,
    text: String,
    /// The contents of `<script>` and `<style>` elements of HTML, Vue and Svelte, with their own
    /// grammar (`SPEC-0141` v2 behaviour 6).
    embedded: Vec<Embedded>,
}

/// A block of another language inside a document.
struct Embedded {
    /// Byte range in the outer text.
    range: Range<usize>,
    doc: SyntaxDocument,
}

/// The language of a `<script>` or `<style>` element from its `lang` or `type` attribute: script
/// is JavaScript unless `ts`/`tsx`/`typescript` (JSON for `application/json` and similar); style
/// is CSS unless `scss` or `less`. `None` for other types (templates).
fn embedded_language(element: &str, attributes: &[(String, String)]) -> Option<(LanguageId, bool)> {
    let value = |name: &str| {
        attributes
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.to_ascii_lowercase())
    };
    if element == "style_element" {
        return Some(match value("lang").as_deref() {
            Some("scss") => (LanguageId::Scss, false),
            Some("less") => (LanguageId::Less, false),
            None | Some("css" | "postcss") => (LanguageId::Css, false),
            Some(_) => return None,
        });
    }
    match (value("lang").as_deref(), value("type").as_deref()) {
        (Some("ts" | "typescript"), _) => Some((LanguageId::TypeScript, false)),
        (Some("tsx"), _) => Some((LanguageId::TypeScript, true)),
        (Some("js" | "javascript" | "jsx"), _) | (None, None) => {
            Some((LanguageId::JavaScript, false))
        }
        (None, Some(t)) if t.contains("typescript") => Some((LanguageId::TypeScript, false)),
        (None, Some(t)) if t.ends_with("json") => Some((LanguageId::Json, false)),
        (None, Some(t)) if t.contains("javascript") || t == "module" => {
            Some((LanguageId::JavaScript, false))
        }
        _ => None,
    }
}

/// The embedded blocks of an HTML-like `tree` over `text`.
fn embedded_blocks(language: LanguageId, tree: &Tree, text: &str) -> Vec<Embedded> {
    if !matches!(
        language,
        LanguageId::Html | LanguageId::Vue | LanguageId::Svelte
    ) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if kind != "script_element" && kind != "style_element" {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        let mut attributes = Vec::new();
        if let Some(tag) = children.iter().find(|c| c.kind() == "start_tag") {
            let mut c = tag.walk();
            for attr in tag.children(&mut c).filter(|a| a.kind() == "attribute") {
                let mut ac = attr.walk();
                let parts: Vec<_> = attr.children(&mut ac).collect();
                let name = parts
                    .iter()
                    .find(|p| p.kind() == "attribute_name")
                    .map(|p| text[p.byte_range()].to_owned())
                    .unwrap_or_default();
                let value = parts
                    .iter()
                    .find(|p| p.kind() != "attribute_name" && p.kind() != "=")
                    .map(|p| text[p.byte_range()].trim_matches(['"', '\'']).to_owned())
                    .unwrap_or_default();
                attributes.push((name, value));
            }
        }
        let Some(raw) = children.iter().find(|c| c.kind() == "raw_text") else {
            continue;
        };
        let Some((lang, tsx)) = embedded_language(kind, &attributes) else {
            continue;
        };
        let range = raw.byte_range();
        if let Ok(doc) = SyntaxDocument::new(lang, tsx, &text[range.clone()]) {
            out.push(Embedded { range, doc });
        }
    }
    out.sort_by_key(|e| e.range.start);
    out
}

fn point_at(text: &str, byte: usize) -> Point {
    let before = &text[..byte];
    let row = before.matches('\n').count();
    let column = before.rfind('\n').map_or(byte, |nl| byte - nl - 1);
    Point { row, column }
}

fn byte_of(text: &str, chars: usize) -> Option<usize> {
    // A byte starts a char unless it is a continuation byte (0b10xx_xxxx).
    let starts_char = |b: &u8| (*b as i8) >= -0x40;
    let bytes = text.as_bytes();
    let (mut seen, mut i) = (0, 0);
    // Whole chunks first: counting in chunks vectorises, unlike decoding char by char.
    while let Some(chunk) = bytes.get(i..i + 64) {
        let n = chunk.iter().filter(|b| starts_char(b)).count();
        if seen + n > chars {
            break;
        }
        seen += n;
        i += 64;
    }
    for (j, b) in bytes[i..].iter().enumerate() {
        if starts_char(b) {
            if seen == chars {
                return Some(i + j);
            }
            seen += 1;
        }
    }
    (seen == chars).then_some(bytes.len())
}

fn char_of(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

impl SyntaxDocument {
    /// Parses `text` as `language`; `tsx` selects TSX for TypeScript.
    pub fn new(language: LanguageId, tsx: bool, text: &str) -> Result<Self, SyntaxError> {
        let grammar = grammar(language, tsx)
            .ok_or_else(|| SyntaxError::Unsupported(language.lsp_id().to_owned()))?;
        let mut parser = Parser::new();
        parser
            .set_language(&grammar.language)
            .expect("grammar matches the tree-sitter version");
        let tree = parser
            .parse(text, None)
            .expect("no timeout or cancellation set");
        let embedded = embedded_blocks(language, &tree, text);
        Ok(Self {
            language,
            tsx,
            grammar,
            parser,
            tree,
            text: text.to_owned(),
            embedded,
        })
    }

    /// The language of the document.
    pub fn language(&self) -> LanguageId {
        self.language
    }

    pub(crate) fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The tree and the text as they are now, to read on another thread; the tree is shared,
    /// not copied (`SPEC-0098` v12 behaviour 26).
    pub fn snapshot(&self) -> TreeSnapshot {
        TreeSnapshot {
            tree: self.tree.clone(),
            text: self.text.clone(),
        }
    }

    pub(crate) fn ts_language(&self) -> &Language {
        &self.grammar.language
    }

    /// Current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the chars in `range` with `new_text`, re-parses incrementally
    /// and returns the char ranges whose syntax changed.
    pub fn edit(
        &mut self,
        range: Range<usize>,
        new_text: &str,
    ) -> Result<Vec<Range<usize>>, SyntaxError> {
        let (Some(start), Some(old_end)) = (
            byte_of(&self.text, range.start),
            byte_of(&self.text, range.end),
        ) else {
            return Err(SyntaxError::OutOfBounds(range));
        };
        if start > old_end {
            return Err(SyntaxError::OutOfBounds(range));
        }
        let start_position = point_at(&self.text, start);
        let old_end_position = point_at(&self.text, old_end);
        self.text.replace_range(start..old_end, new_text);
        let new_end = start + new_text.len();
        self.tree.edit(&InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            start_position,
            old_end_position,
            new_end_position: point_at(&self.text, new_end),
        });
        let tree = self
            .parser
            .parse(&self.text, Some(&self.tree))
            .expect("no timeout or cancellation set");
        let mut changed: Vec<Range<usize>> = self
            .tree
            .changed_ranges(&tree)
            .map(|r| r.start_byte..r.end_byte)
            .collect();
        // The edited text itself always needs repainting.
        changed.push(start..new_end);
        // Embedded blocks are parsed anew; a changed one is repainted whole.
        let embedded = embedded_blocks(self.language, &tree, &self.text);
        for block in &embedded {
            let same = self.embedded.iter().any(|old| {
                old.doc.text == block.doc.text && old.doc.language == block.doc.language
            });
            if !same {
                changed.push(block.range.clone());
            }
        }
        self.embedded = embedded;
        changed.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in changed {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        self.tree = tree;
        Ok(merged
            .into_iter()
            .map(|r| char_of(&self.text, r.start)..char_of(&self.text, r.end.min(self.text.len())))
            .collect())
    }

    /// Spans of the whole document.
    pub fn highlight(&self) -> Vec<Span> {
        self.highlight_range(0..self.text.chars().count())
    }

    /// Spans intersecting the char range `range`.
    pub fn highlight_range(&self, range: Range<usize>) -> Vec<Span> {
        let start = byte_of(&self.text, range.start).unwrap_or(self.text.len());
        // The end is counted on from the start, not from the beginning of the text.
        let end = byte_of(&self.text[start..], range.end.saturating_sub(range.start))
            .map_or(self.text.len(), |b| start + b);
        let query = &self.grammar.query;
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start..end);
        // (byte range, pattern index, capture index)
        let mut captures: Vec<(Range<usize>, usize, u32)> = Vec::new();
        let refined_base = names.len() as u32;
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(m) = matches.next() {
            for c in m.captures {
                let r = c.node.byte_range();
                let name = names[c.index as usize];
                // `spell`/`nospell` only mark text for a spell checker, they are no colour.
                if r.start < r.end && !name.starts_with('_') && !name.ends_with("spell") {
                    // A documentation comment is a comment of its own role (`SPEC-0027` v10).
                    let capture = if name.starts_with("comment")
                        && doc_comment(self.language, &self.text[r.clone()])
                    {
                        refined_base + DOC_COMMENT_ROLE
                    } else {
                        c.index
                    };
                    captures.push((r, m.pattern_index, capture));
                }
            }
        }
        // Roles a query cannot decide alone (`SPEC-0027` v6); appended last with pattern 0, so they
        // win over every other capture of the same node.
        for (range, role) in self.refined_roles(start, end) {
            captures.push((range, 0, refined_base + role));
        }
        let kind_of = |capture: u32| -> &str {
            let name = names
                .get(capture as usize)
                .copied()
                .unwrap_or_else(|| REFINED_ROLES[(capture - refined_base) as usize]);
            language_role(self.language, name)
        };
        // Paint outer nodes first and, for the same node, later patterns first,
        // so inner nodes and earlier patterns end up on top.
        captures.sort_by(|a, b| {
            (b.0.len(), b.1)
                .cmp(&(a.0.len(), a.1))
                .then(a.0.start.cmp(&b.0.start))
        });
        let lo = start;
        let hi = end.max(start);
        let mut paint: Vec<Option<u32>> = vec![None; hi - lo];
        for (r, _, capture) in &captures {
            let (s, e) = (r.start.max(lo), r.end.min(hi));
            for slot in paint
                .iter_mut()
                .take(e.saturating_sub(lo))
                .skip(s.saturating_sub(lo))
            {
                *slot = Some(*capture);
            }
        }
        let mut spans: Vec<Span> = Vec::new();
        // Spans come in text order, so char offsets are counted on from the previous one instead
        // of from the start of the text (linear instead of quadratic in the document size).
        let first_char = if lo < self.text.len() {
            range.start
        } else {
            char_of(&self.text, lo)
        };
        let (mut at_byte, mut at_char) = (lo, first_char);
        let mut char_at = |byte: usize| {
            at_char += self.text[at_byte..byte].chars().count();
            at_byte = byte;
            at_char
        };
        let mut i = 0;
        while i < paint.len() {
            let Some(capture) = paint[i] else {
                i += 1;
                continue;
            };
            let mut j = i;
            while j < paint.len() && paint[j] == Some(capture) {
                j += 1;
            }
            let (b0, b1) = (lo + i, lo + j);
            if self.text.is_char_boundary(b0) && self.text.is_char_boundary(b1) {
                spans.push(Span {
                    start: char_at(b0),
                    end: char_at(b1),
                    kind: kind_of(capture).to_owned(),
                });
            }
            i = j;
        }
        // Embedded blocks in their own colours (`SPEC-0141` v2 behaviour 6).
        let mut added = false;
        for block in &self.embedded {
            let (s, e) = (block.range.start.max(lo), block.range.end.min(hi));
            if s >= e {
                continue;
            }
            let base = char_of(&self.text, block.range.start);
            let inner = &block.doc.text;
            let from = char_of(inner, s - block.range.start);
            let to = char_of(inner, e - block.range.start);
            for span in block.doc.highlight_range(from..to) {
                spans.push(Span {
                    start: span.start + base,
                    end: span.end + base,
                    kind: span.kind,
                });
            }
            added = true;
        }
        if added {
            spans.sort_by_key(|s| s.start);
        }
        spans
    }

    /// Byte ranges of the comments the highlight query captures (`comment…`), embedded blocks
    /// included, in text order; a range overlapping an earlier one is dropped. Much cheaper than
    /// filtering [`Self::highlight`] of the whole document: no paint pass, no char offsets.
    pub fn comment_ranges(&self) -> Vec<Range<usize>> {
        let query = self.grammar.comments();
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut ranges = Vec::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(m) = matches.next() {
            for c in m.captures {
                let r = c.node.byte_range();
                if r.start < r.end && names[c.index as usize].starts_with("comment") {
                    ranges.push(r);
                }
            }
        }
        for block in &self.embedded {
            let base = block.range.start;
            ranges.extend(
                block
                    .doc
                    .comment_ranges()
                    .into_iter()
                    .map(|r| r.start + base..r.end + base),
            );
        }
        ranges.sort_by_key(|r| r.start);
        ranges.dedup_by(|b, a| b.start < a.end);
        ranges
    }
}

impl SyntaxDocument {
    /// Char ranges a selection around the char range `at` can grow to (`SPEC-0057` v3 behaviour
    /// 13), smallest first: every enclosing node (`…declarator` nodes left out), the text of a
    /// string literal without its quotes, the whole lines of a node that fills its lines, and the
    /// lines between the braces of a `{…}` node.
    pub fn selection_ranges(&self, at: Range<usize>) -> Vec<Range<usize>> {
        let text = self.text.as_str();
        let (Some(start), Some(end)) = (byte_of(text, at.start), byte_of(text, at.end)) else {
            return Vec::new();
        };
        let bytes = text.as_bytes();
        let line_start = |b: usize| text[..b].rfind('\n').map_or(0, |i| i + 1);
        // After the line break of the line holding byte `b` (or the text end).
        let line_after = |b: usize| text[b..].find('\n').map_or(text.len(), |i| b + i + 1);
        let blank = |r: Range<usize>| text[r].trim().is_empty();
        let mut out: Vec<Range<usize>> = Vec::new();
        let mut node = self.tree.root_node().descendant_for_byte_range(start, end);
        while let Some(n) = node {
            let r = n.byte_range();
            node = n.parent();
            if r.start >= r.end || n.kind().ends_with("declarator") {
                continue;
            }
            out.push(r.clone());
            let first = bytes[r.start];
            let last = bytes[r.end - 1];
            if n.kind().contains("string")
                && r.end - r.start >= 2
                && matches!(first, b'"' | b'\'' | b'`')
                && first == last
            {
                out.push(r.start + 1..r.end - 1);
            }
            let ls = line_start(r.start);
            let le = if bytes[r.end - 1] == b'\n' {
                r.end
            } else {
                line_after(r.end)
            };
            if blank(ls..r.start) && blank(r.end..le) {
                out.push(ls..le);
            }
            if first == b'{' && last == b'}' {
                let inner_start = line_after(r.start);
                let close_line = line_start(r.end - 1);
                if blank(r.start + 1..inner_start)
                    && blank(close_line..r.end - 1)
                    && inner_start < close_line
                {
                    out.push(inner_start..close_line);
                }
            }
        }
        let chars = |b: usize| text[..b].chars().count();
        let mut ranges: Vec<Range<usize>> = out
            .into_iter()
            .filter(|r| r.start <= start && r.end >= end)
            .map(|r| chars(r.start)..chars(r.end))
            .collect();
        ranges.sort_by_key(|r| (r.len(), r.start));
        ranges.dedup();
        ranges
    }
}

/// A definition found by the grammar's tags query (`SPEC-0031`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Definition {
    /// Name of the defined symbol.
    pub name: String,
    /// `class`, `function`, `method`, `interface`, `type`, `module`, `constant`, `macro`.
    pub kind: String,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column (chars) of the name.
    pub character: usize,
}

/// Context Info (`SPEC-0057` v6 behaviour 27): of the declarations `around` the caret, outermost
/// first as [`SyntaxDocument::definitions_around`] gives them, the one to show when the rows from
/// the 0-based line `first_visible` on are in view. That is the innermost one above that line;
/// with `shown`, the line of the declaration shown now, the next such one outwards from it. `None`
/// when all (that are left) are in view.
pub fn context_declaration(
    around: &[Definition],
    first_visible: usize,
    shown: Option<usize>,
) -> Option<&Definition> {
    let mut outwards = around.iter().rev();
    if let Some(line) = shown.filter(|l| around.iter().any(|d| d.line == *l)) {
        outwards.by_ref().find(|d| d.line == line);
    }
    outwards.find(|d| d.line < first_visible)
}

/// Where a search may look in a file (`SPEC-0065` v4 behaviour 13, `SPEC-0066` v3 behaviour 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SearchContext {
    /// Everywhere.
    #[default]
    Anywhere,
    /// Only in comments.
    InComments,
    /// Only in string literals.
    InStrings,
    /// Everywhere but in comments.
    ExceptComments,
    /// Everywhere but in string literals.
    ExceptStrings,
    /// Neither in comments nor in string literals.
    ExceptCommentsAndStrings,
}

impl SearchContext {
    /// Every context, in menu order.
    pub const ALL: [SearchContext; 6] = [
        SearchContext::Anywhere,
        SearchContext::InComments,
        SearchContext::InStrings,
        SearchContext::ExceptComments,
        SearchContext::ExceptStrings,
        SearchContext::ExceptCommentsAndStrings,
    ];

    /// The id of this context on the wire (`SPEC-0318` v5 behaviour 40).
    pub fn id(self) -> &'static str {
        match self {
            SearchContext::Anywhere => "anywhere",
            SearchContext::InComments => "in_comments",
            SearchContext::InStrings => "in_strings",
            SearchContext::ExceptComments => "except_comments",
            SearchContext::ExceptStrings => "except_strings",
            SearchContext::ExceptCommentsAndStrings => "except_comments_and_strings",
        }
    }

    /// The context an id names.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            SearchContext::Anywhere => "Anywhere",
            SearchContext::InComments => "In Comments",
            SearchContext::InStrings => "In String Literals",
            SearchContext::ExceptComments => "Except Comments",
            SearchContext::ExceptStrings => "Except String Literals",
            SearchContext::ExceptCommentsAndStrings => "Except Comments and String Literals",
        }
    }

    /// Keeps the char ranges of `matches` in `text` (the content of `path`) that lie where this
    /// context allows; a file without a grammar has no comments or strings.
    pub fn filter(
        self,
        path: &std::path::Path,
        text: &str,
        matches: Vec<Range<usize>>,
    ) -> Vec<Range<usize>> {
        if self == SearchContext::Anywhere || matches.is_empty() {
            return matches;
        }
        let (comments, strings) = comment_and_string_ranges(path, text);
        let inside = |set: &[Range<usize>], m: &Range<usize>| {
            set.iter().any(|r| r.start <= m.start && m.end <= r.end)
        };
        matches
            .into_iter()
            .filter(|m| {
                let (c, s) = (inside(&comments, m), inside(&strings, m));
                match self {
                    SearchContext::Anywhere => true,
                    SearchContext::InComments => c,
                    SearchContext::InStrings => s,
                    SearchContext::ExceptComments => !c,
                    SearchContext::ExceptStrings => !s,
                    SearchContext::ExceptCommentsAndStrings => !c && !s,
                }
            })
            .collect()
    }
}

/// The char ranges of comments and of string literals in `text`, the content of `path`, from its
/// grammar's highlighting; empty for files without a grammar.
pub fn comment_and_string_ranges(
    path: &std::path::Path,
    text: &str,
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let Some(language) = LanguageId::detect(path) else {
        return (Vec::new(), Vec::new());
    };
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    let Ok(doc) = SyntaxDocument::new(language, tsx, text) else {
        return (Vec::new(), Vec::new());
    };
    let (mut comments, mut strings) = (Vec::new(), Vec::new());
    for s in doc.highlight() {
        if s.kind.starts_with("comment") {
            comments.push(s.start..s.end);
        } else if s.kind.starts_with("string") || s.kind.starts_with("character") {
            strings.push(s.start..s.end);
        }
    }
    (comments, strings)
}

/// The byte ranges of the comments of `text`, the content of `path`, sorted and not overlapping
/// (`SPEC-0334` behaviour 3): what the grammar of the file's language captures as a comment,
/// embedded languages included. `None` for a file whose language is unknown or has no grammar,
/// and for a text too large for a tree.
pub fn comment_ranges_of(path: &std::path::Path, text: &str) -> Option<Vec<Range<usize>>> {
    if text.len() > MAX_SYNTAX_BYTES {
        return None;
    }
    let language = LanguageId::detect(path)?;
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    let doc = SyntaxDocument::new(language, tsx, text).ok()?;
    Some(doc.comment_ranges())
}

/// The declarations of `text`, the content of `path`, parsed with the language of its name; empty
/// for an unknown language or a text too large for a tree (`SPEC-0292` v4 behaviour 7a).
pub fn declaration_spans_of(
    path: &std::path::Path,
    text: &str,
    limit: usize,
) -> Vec<DeclarationSpan> {
    let Some(language) = LanguageId::detect(path) else {
        return Vec::new();
    };
    if text.len() > MAX_SYNTAX_BYTES {
        return Vec::new();
    }
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    SyntaxDocument::new(language, tsx, text)
        .map(|d| d.declaration_spans(limit))
        .unwrap_or_default()
}

/// A declaration with the lines its whole node covers (`SPEC-0292` v4 behaviour 7a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationSpan {
    /// Name of the declared symbol.
    pub name: String,
    /// `class`, `interface`, `method` or `function`.
    pub kind: String,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based first line of the node (annotations and modifiers included).
    pub start_line: usize,
    /// 0-based last line of the node.
    pub end_line: usize,
}

/// Tags query of `language`, if the grammar ships one.
fn tags_query(language: LanguageId, tsx: bool) -> Option<&'static Query> {
    macro_rules! cached {
        ($($query:expr),+) => {{
            static CELL: OnceLock<Query> = OnceLock::new();
            let grammar = grammar(language, tsx)?;
            CELL.get_or_init(|| {
                Query::new(&grammar.language, &[$($query),+].join("\n"))
                    .expect("bundled tags query compiles")
            })
        }};
    }
    Some(match language {
        LanguageId::C => cached!(tree_sitter_c::TAGS_QUERY),
        LanguageId::Cpp => cached!(tree_sitter_cpp::TAGS_QUERY),
        LanguageId::Go => cached!(tree_sitter_go::TAGS_QUERY),
        LanguageId::Java => cached!(tree_sitter_java::TAGS_QUERY),
        LanguageId::JavaScript => cached!(tree_sitter_javascript::TAGS_QUERY),
        LanguageId::Python => cached!(tree_sitter_python::TAGS_QUERY),
        LanguageId::Rust => cached!(tree_sitter_rust::TAGS_QUERY),
        LanguageId::Swift => cached!(tree_sitter_swift::TAGS_QUERY),
        LanguageId::Lua => cached!(tree_sitter_lua::TAGS_QUERY),
        LanguageId::R => cached!(tree_sitter_r::TAGS_QUERY),
        LanguageId::TypeScript if tsx => {
            cached!(
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            )
        }
        LanguageId::TypeScript => {
            cached!(
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            )
        }
        _ => return None,
    })
}

/// A Java method body of one statement, folded when the file opens (`SPEC-0083` v2 behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OneLineBody {
    /// Line of the opening brace (stays visible).
    pub start_line: usize,
    /// Line of the closing brace.
    pub end_line: usize,
    /// Text shown after the opening brace while folded: ` statement }`.
    pub placeholder: String,
    /// Columns of the folded line: the brace line up to `{` and the placeholder (`SPEC-0083` v10
    /// behaviour 7a); the editor folds only bodies within the right margin.
    pub folded_width: usize,
}

/// Whether a node is of some kind (a comment, an import).
type NodeTest<'a> = dyn Fn(&tree_sitter::Node) -> bool + 'a;

/// A collapsible region (`SPEC-0030`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoldRegion {
    /// 0-based first line (stays visible).
    pub start_line: usize,
    /// 0-based last line (hidden when folded).
    pub end_line: usize,
    /// `block`, `comment`, `imports` or `region` (a custom region between comment markers,
    /// `SPEC-0083` v8 behaviour 19).
    pub kind: String,
    /// A documentation comment (`SPEC-0083` v8 behaviour 20).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub doc: bool,
    /// The outermost container of the file, which Collapse All leaves open (`SPEC-0083` v8
    /// behaviour 18).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub outermost: bool,
    /// How many chars of the first line stay in front of the placeholder while the region is
    /// folded; without it the whole line stays (`SPEC-0083` v8 behaviour 21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<usize>,
    /// The placeholder shown for the hidden text while the region is folded.
    pub collapsed_text: String,
}

impl FoldRegion {
    fn new(start_line: usize, end_line: usize, kind: &str) -> Self {
        Self {
            start_line,
            end_line,
            kind: kind.into(),
            doc: false,
            outermost: false,
            keep: None,
            collapsed_text: crate::folding::ELLIPSIS.into(),
        }
    }
}

fn is_comment(kind: &str) -> bool {
    kind.contains("comment")
}

fn is_import(kind: &str) -> bool {
    matches!(
        kind,
        "use_declaration"
            | "import_statement"
            | "import_from_statement"
            | "import_declaration"
            | "preproc_include"
            | "future_import_statement"
    )
}

fn is_comment_node(node: &tree_sitter::Node) -> bool {
    is_comment(node.kind())
}

/// An import statement; Kotlin's grammar names the statement `import`, like the keyword in it.
fn is_import_node(node: &tree_sitter::Node) -> bool {
    is_import(node.kind()) || (node.is_named() && node.kind() == "import")
}

/// The byte ranges of the elements of `node` when it is a comma-separated list: its children are
/// separated by `,` tokens, with exactly one named child that is no comment between two commas
/// and nothing after a trailing comma (`SPEC-0056` v4 behaviour 19).
fn comma_list<'t>(
    node: &tree_sitter::Node<'t>,
    cursor: &mut tree_sitter::TreeCursor<'t>,
) -> Option<Vec<Range<usize>>> {
    if node.is_error() {
        return None;
    }
    let mut groups: Vec<Vec<Range<usize>>> = vec![Vec::new()];
    for child in node.children(cursor) {
        if child.kind() == "," {
            groups.push(Vec::new());
        } else if child.is_named() && !is_comment(child.kind()) {
            groups.last_mut()?.push(child.byte_range());
        }
    }
    if groups.last().is_some_and(Vec::is_empty) {
        groups.pop();
    }
    if groups.len() < 2 || groups.iter().any(|g| g.len() != 1 || g[0].is_empty()) {
        return None;
    }
    Some(groups.into_iter().flatten().collect())
}

/// What a Kotlin name declares, see [`SyntaxDocument::kotlin_declared`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KotlinDeclared {
    /// A `val`/`var` inside a function body, a lambda or an initializer.
    Local,
    /// A `val`/`var` parameter of a primary constructor.
    ConstructorProperty,
}

impl SyntaxDocument {
    /// Bodies of Java methods and constructors that hold one statement on one line, with the opening
    /// brace at the end of a line that has more on it and the closing brace on the line after the
    /// statement (`SPEC-0083` v2 behaviour 7): the line of `{`, the line of `}` and the text shown
    /// in the fold's place, ` statement }`.
    pub fn one_line_bodies(&self) -> Vec<OneLineBody> {
        if self.language != LanguageId::Java {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut stack = vec![self.tree.root_node()];
        while let Some(node) = stack.pop() {
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
            if !matches!(
                node.kind(),
                "method_declaration" | "constructor_declaration"
            ) {
                continue;
            }
            let Some(body) = node.child_by_field_name("body") else {
                continue;
            };
            // The name and the end of the parameter list on one line (`SPEC-0083` v10 behaviour
            // 7a, as the open platform's folding of one-line methods asks).
            let same_line = match (
                node.child_by_field_name("name"),
                node.child_by_field_name("parameters"),
            ) {
                (Some(name), Some(params)) => {
                    name.start_position().row == params.end_position().row
                }
                _ => false,
            };
            if !same_line {
                continue;
            }
            let open = body.start_position().row;
            let close = body.end_position().row;
            if close != open + 2 {
                continue;
            }
            let mut cursor = body.walk();
            let inner: Vec<_> = body.named_children(&mut cursor).collect();
            let [statement] = inner.as_slice() else {
                continue;
            };
            if statement.start_position().row != open + 1
                || statement.end_position().row != open + 1
                || statement.kind().contains("comment")
            {
                continue;
            }
            let line_start = self.text[..body.start_byte()]
                .rfind('\n')
                .map_or(0, |i| i + 1);
            if self.text[line_start..body.start_byte()].trim().is_empty()
                || !self.text[body.start_byte() + 1..]
                    .split('\n')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            {
                continue;
            }
            let statement = self.text[statement.start_byte()..statement.end_byte()].trim();
            let placeholder = format!(" {statement} }}");
            let brace_line = self.text[line_start..=body.start_byte()].chars().count();
            out.push(OneLineBody {
                start_line: open,
                end_line: close,
                folded_width: brace_line + placeholder.chars().count(),
                placeholder,
            });
        }
        out.sort_by_key(|b| b.start_line);
        out
    }

    /// The import block of a Java or Kotlin file with more than one import (`SPEC-0083` v4
    /// behaviour 9): the line of the first import and the line of the last, with the blank lines
    /// and comments between them. `None` for one import, for imports all on one line and for
    /// other languages.
    pub fn import_block(&self) -> Option<(usize, usize)> {
        let kind = match self.language {
            LanguageId::Java => "import_declaration",
            LanguageId::Kotlin => "import",
            _ => return None,
        };
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        let imports: Vec<_> = root
            .named_children(&mut cursor)
            .filter(|n| n.kind() == kind)
            .collect();
        let (first, last) = (imports.first()?, imports.last()?);
        let (start, end) = (first.start_position().row, last.end_position().row);
        (imports.len() > 1 && end > start).then_some((start, end))
    }

    /// What the Kotlin name starting at char `offset` declares, where the syntax tree knows it
    /// better than kotlin-language-server's token (`SPEC-0290` v3 behaviour 3a): a `val`/`var` in a
    /// body is a local variable, a `val`/`var` of a primary constructor a property.
    pub fn kotlin_declared(&self, offset: usize) -> Option<KotlinDeclared> {
        if self.language != LanguageId::Kotlin {
            return None;
        }
        let byte = byte_of(&self.text, offset)?;
        let name = self
            .tree
            .root_node()
            .named_descendant_for_byte_range(byte, byte)?;
        if name.kind() != "identifier" {
            return None;
        }
        let parent = name.parent()?;
        match parent.kind() {
            "class_parameter" => {
                let mut cursor = parent.walk();
                parent
                    .children(&mut cursor)
                    .any(|c| matches!(c.kind(), "val" | "var"))
                    .then_some(KotlinDeclared::ConstructorProperty)
            }
            "variable_declaration" => {
                // The declaration, or the destructuring it is part of.
                let mut declaration = parent.parent()?;
                if declaration.kind() == "multi_variable_declaration" {
                    declaration = declaration.parent()?;
                }
                if declaration.kind() != "property_declaration" {
                    return None;
                }
                let owner = declaration.parent()?.kind();
                (!matches!(owner, "source_file" | "class_body" | "enum_class_body"))
                    .then_some(KotlinDeclared::Local)
            }
            _ => None,
        }
    }

    /// Definitions in document order (`SPEC-0031` behaviour 5); empty without a tags query.
    pub fn definitions(&self) -> Vec<Definition> {
        let Some(query) = tags_query(self.language, self.tsx) else {
            return Vec::new();
        };
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut out: Vec<Definition> = Vec::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(m) = matches.next() {
            let kind = m
                .captures
                .iter()
                .find_map(|c| names[c.index as usize].strip_prefix("definition."));
            let name = m
                .captures
                .iter()
                .find(|c| names[c.index as usize] == "name");
            if let (Some(kind), Some(name)) = (kind, name) {
                out.push(self.definition_at(name.node, kind));
            }
        }
        // Java's tags query names neither records nor enums; they are classes all the same, nested
        // ones too (`SPEC-0031` v6 behaviour 8a).
        if self.language == LanguageId::Java {
            let mut stack = vec![self.tree.root_node()];
            while let Some(node) = stack.pop() {
                let kind = match node.kind() {
                    "record_declaration" => Some("class"),
                    "enum_declaration" => Some("enum"),
                    _ => None,
                };
                if let (Some(kind), Some(name)) = (kind, node.child_by_field_name("name")) {
                    out.push(self.definition_at(name, kind));
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
        }
        out.sort_by_key(|d| (d.line, d.character));
        out.dedup_by(|a, b| a.line == b.line && a.character == b.character);
        out
    }

    /// The definition of kind `kind` whose name is the node `name`.
    fn definition_at(&self, name: tree_sitter::Node<'_>, kind: &str) -> Definition {
        let p = name.start_position();
        let line_start = name.start_byte() - p.column;
        Definition {
            name: self.text[name.byte_range()].to_owned(),
            kind: kind.to_owned(),
            line: p.row,
            character: self.text[line_start..name.start_byte()].chars().count(),
        }
    }

    /// Classes, interfaces, methods and functions with the lines of their whole nodes, in
    /// document order, at most `limit` (`SPEC-0292` v4 behaviour 7a: the authors of
    /// declarations); empty without a tags query.
    pub fn declaration_spans(&self, limit: usize) -> Vec<DeclarationSpan> {
        let Some(query) = tags_query(self.language, self.tsx) else {
            return Vec::new();
        };
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut out: Vec<DeclarationSpan> = Vec::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(m) = matches.next() {
            let def = m.captures.iter().find_map(|c| {
                names[c.index as usize]
                    .strip_prefix("definition.")
                    .map(|kind| (kind, c.node))
            });
            let name = m
                .captures
                .iter()
                .find(|c| names[c.index as usize] == "name");
            let (Some((kind, node)), Some(name)) = (def, name) else {
                continue;
            };
            if !matches!(kind, "class" | "interface" | "method" | "function") {
                continue;
            }
            out.push(DeclarationSpan {
                name: self.text[name.node.byte_range()].to_owned(),
                kind: kind.to_owned(),
                line: name.node.start_position().row,
                start_line: node.start_position().row,
                end_line: node.end_position().row,
            });
        }
        // Java's tags query names neither records, enums nor constructors; they are declarations
        // the author hint stands at as well.
        if self.language == LanguageId::Java {
            let mut stack = vec![self.tree.root_node()];
            while let Some(node) = stack.pop() {
                let kind = match node.kind() {
                    "record_declaration" | "enum_declaration" => Some("class"),
                    "constructor_declaration" | "compact_constructor_declaration" => Some("method"),
                    _ => None,
                };
                if let (Some(kind), Some(name)) = (kind, node.child_by_field_name("name")) {
                    out.push(DeclarationSpan {
                        name: self.text[name.byte_range()].to_owned(),
                        kind: kind.to_owned(),
                        line: name.start_position().row,
                        start_line: node.start_position().row,
                        end_line: node.end_position().row,
                    });
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
        }
        out.sort_by_key(|d| (d.line, d.start_line, std::cmp::Reverse(d.end_line)));
        out.dedup_by(|a, b| a.line == b.line && a.name == b.name);
        out.truncate(limit);
        out
    }

    /// The definitions whose whole node contains byte `offset`, outermost first (`SPEC-0061` v3
    /// behaviour 5a: the symbol path of the breadcrumbs); empty without a tags query.
    pub fn definitions_around(&self, offset: usize) -> Vec<Definition> {
        let Some(query) = tags_query(self.language, self.tsx) else {
            return Vec::new();
        };
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut found: Vec<(usize, usize, Definition)> = Vec::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), self.text.as_bytes());
        while let Some(m) = matches.next() {
            let def = m.captures.iter().find_map(|c| {
                names[c.index as usize]
                    .strip_prefix("definition.")
                    .map(|kind| (kind, c.node))
            });
            let name = m
                .captures
                .iter()
                .find(|c| names[c.index as usize] == "name");
            let (Some((kind, node)), Some(name)) = (def, name) else {
                continue;
            };
            if !(node.start_byte() <= offset && offset <= node.end_byte()) {
                continue;
            }
            let p = name.node.start_position();
            let line_start = name.node.start_byte() - p.column;
            found.push((
                node.start_byte(),
                node.end_byte(),
                Definition {
                    name: self.text[name.node.byte_range()].to_owned(),
                    kind: kind.to_owned(),
                    line: p.row,
                    character: self.text[line_start..name.node.start_byte()]
                        .chars()
                        .count(),
                },
            ));
        }
        found.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
        found.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        found.into_iter().map(|(_, _, d)| d).collect()
    }

    /// Char offsets of the names of the method and function declarations, in document order
    /// (`SPEC-0071` v9 behaviour 12); empty without a tags query.
    pub fn method_offsets(&self) -> Vec<usize> {
        let found: Vec<Definition> = self
            .definitions()
            .into_iter()
            .filter(|d| matches!(d.kind.as_str(), "method" | "function"))
            .collect();
        if found.is_empty() {
            return Vec::new();
        }
        // Char offset of every line start.
        let mut starts = vec![0usize];
        let mut n = 0;
        for c in self.text.chars() {
            n += 1;
            if c == '\n' {
                starts.push(n);
            }
        }
        found
            .iter()
            .filter_map(|d| starts.get(d.line).map(|s| s + d.character))
            .collect()
    }

    /// The elements of the comma-separated list around the char offset `at`, as char ranges in
    /// order (`SPEC-0056` v4 behaviour 19): the innermost node whose children are separated by `,`
    /// with one element between two commas and `at` inside or at an edge of an element. Empty
    /// when there is none.
    pub fn list_elements(&self, at: usize) -> Vec<Range<usize>> {
        let text = self.text.as_str();
        let Some(byte) = byte_of(text, at) else {
            return Vec::new();
        };
        let mut cursor = self.tree.walk();
        let mut node = self.tree.root_node().descendant_for_byte_range(byte, byte);
        while let Some(n) = node {
            node = n.parent();
            if let Some(elements) = comma_list(&n, &mut cursor)
                && elements.iter().any(|r| r.start <= byte && byte <= r.end)
            {
                let chars = |b: usize| text[..b].chars().count();
                return elements
                    .into_iter()
                    .map(|r| chars(r.start)..chars(r.end))
                    .collect();
            }
        }
        Vec::new()
    }

    /// The code blocks around the char offset `at` for Fold Code Block, innermost first, each as
    /// the 0-based lines of its `{` and its `}` (`SPEC-0083` v7 behaviour 15): in Java the `{…}`
    /// blocks of method, constructor, lambda and initialiser bodies, of statements and of a
    /// `switch` — not the body of a type. A block is around the offset from before its `{` to
    /// behind its `}`. Empty for other languages.
    pub fn code_blocks_around(&self, at: usize) -> Vec<(usize, usize)> {
        if self.language != LanguageId::Java {
            return Vec::new();
        }
        let Some(byte) = byte_of(&self.text, at) else {
            return Vec::new();
        };
        let mut found: Vec<(usize, (usize, usize))> = Vec::new();
        let mut cursor = self.tree.walk();
        let mut stack = vec![self.tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.start_byte() > byte || node.end_byte() < byte {
                continue;
            }
            if matches!(node.kind(), "block" | "constructor_body" | "switch_block") {
                found.push((
                    node.end_byte() - node.start_byte(),
                    (node.start_position().row, node.end_position().row),
                ));
            }
            stack.extend(node.children(&mut cursor));
        }
        found.sort_by_key(|(len, _)| *len);
        found.into_iter().map(|(_, lines)| lines).collect()
    }

    /// The region marker a comment node is (`SPEC-0083` v8 behaviour 19): a comment on one
    /// line with nothing else on that line.
    fn region_marker_of(
        &self,
        node: &tree_sitter::Node,
        lines: &[&str],
    ) -> Option<crate::folding::RegionMarker> {
        let (start, end) = (node.start_position(), node.end_position());
        if !is_comment(node.kind()) || start.row != end.row {
            return None;
        }
        let line = lines.get(start.row)?;
        let alone = line.get(..start.column)?.trim().is_empty()
            && line.get(end.column.min(line.len())..)?.trim().is_empty();
        if !alone {
            return None;
        }
        crate::folding::region_marker(self.text.get(node.byte_range())?)
    }

    /// The body of the outermost container of the file as first and last line (`SPEC-0083` v8
    /// behaviour 18): the body of the only type declared at the top level of a Java or Kotlin
    /// file that declares nothing else there, the root value of a JSON file, the root element
    /// of an HTML or XML file. Other languages have none.
    fn outermost_container(&self) -> Option<(usize, usize)> {
        let root = self.tree.root_node();
        let mut cursor = root.walk();
        let rows = |n: tree_sitter::Node| (n.start_position().row, n.end_position().row);
        match self.language {
            LanguageId::Java | LanguageId::Kotlin => {
                let mut declarations = root.named_children(&mut cursor).filter(|n| {
                    !is_comment(n.kind())
                        && !is_import_node(n)
                        && !matches!(
                            n.kind(),
                            "package_declaration"
                                | "package_header"
                                | "import_list"
                                | "import_header"
                                | "file_annotation"
                                | "shebang_line"
                        )
                });
                let only = declarations.next()?;
                if declarations.next().is_some() || !only.kind().ends_with("_declaration") {
                    return None;
                }
                let mut inner = only.walk();
                let body = only
                    .named_children(&mut inner)
                    .find(|n| n.kind().ends_with("_body") && n.kind() != "function_body")?;
                let is_type = !matches!(
                    only.kind(),
                    "function_declaration" | "property_declaration" | "method_declaration"
                );
                is_type.then(|| rows(body))
            }
            LanguageId::Json => {
                let mut values = root
                    .named_children(&mut cursor)
                    .filter(|n| !is_comment(n.kind()));
                let only = values.next()?;
                values.next().is_none().then(|| rows(only))
            }
            LanguageId::Html | LanguageId::Xml => {
                let mut elements = root
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() == "element");
                let only = elements.next()?;
                elements.next().is_none().then(|| rows(only))
            }
            _ => None,
        }
    }

    /// Folding regions (`SPEC-0030`), sorted by first line and one per first line: bracketed
    /// and indented blocks, comments over several lines, runs of line comments and of imports,
    /// and custom regions between comment markers. Each has the text shown in its place while
    /// it is folded (`SPEC-0083` v8 behaviours 18–21).
    pub fn folding(&self) -> Vec<FoldRegion> {
        use crate::folding::{RegionMarker, block_placeholder, comment_look, comment_run_look};
        let lines: Vec<&str> = self.text.split('\n').collect();
        let line = |row: usize| lines.get(row).copied().unwrap_or_default();
        // Char column of byte column `column` in line `row`.
        let chars = |row: usize, column: usize| {
            line(row)
                .get(..column)
                .map_or(column, |before| before.chars().count())
        };
        let mut regions = Vec::new();
        let mut markers: Vec<(usize, usize, RegionMarker)> = Vec::new();
        let mut cursor = self.tree.walk();
        // Each node with its parent's first row: `Node::parent` searches down from the root, which
        // is quadratic for a root with many children.
        let mut stack = vec![(self.tree.root_node(), None)];
        while let Some((node, parent_row)) = stack.pop() {
            let end = node.end_position().row;
            // An indentation block starts below its header (`class A:`); fold from the header.
            // So does a block whose `{` stands alone on its line; one behind other text
            // (`} else {`, a signature under its annotation) starts on its own line.
            let first = node.child(0).map(|c| c.kind());
            let position = node.start_position();
            let own_line = first == Some("{")
                && line(position.row)
                    .get(..position.column)
                    .is_some_and(|before| !before.trim().is_empty());
            let start = match parent_row {
                Some(row) if node.kind() == "block" && !own_line => row,
                _ => position.row,
            };
            if is_comment(node.kind())
                && let Some(marker) = self.region_marker_of(&node, &lines)
                && markers
                    .last()
                    .is_none_or(|(row, _, _)| *row != position.row)
            {
                markers.push((position.row, chars(position.row, position.column), marker));
            }
            if node.is_named() && end > start && parent_row.is_some() {
                let kind = node.kind();
                let count = node.child_count();
                let last = count
                    .checked_sub(1)
                    .and_then(|i| node.child(i as u32))
                    .map(|c| c.kind());
                let bracketed = matches!(
                    (first, last),
                    (Some("{"), Some("}")) | (Some("["), Some("]")) | (Some("("), Some(")"))
                );
                if is_comment(kind) {
                    let mut region = FoldRegion::new(start, end, "comment");
                    let position = node.start_position();
                    let text = self.text.get(node.byte_range()).unwrap_or_default();
                    (region.doc, region.keep, region.collapsed_text) =
                        comment_look(text, chars(position.row, position.column));
                    regions.push(region);
                } else if bracketed || kind == "block" || (kind == "element" && count > 2) {
                    regions.push(FoldRegion::new(start, end, "block"));
                }
            }
            // Most nodes are leaves: nothing to look through.
            if node.child_count() == 0 {
                continue;
            }
            // Runs of single-line comments and of imports among the children. A region marker
            // is no part of a run.
            let children: Vec<_> = node.children(&mut cursor).collect();
            let plain_comment = |n: &tree_sitter::Node| {
                is_comment_node(n) && self.region_marker_of(n, &lines).is_none()
            };
            let predicates: [(&NodeTest, &str); 2] =
                [(&plain_comment, "comment"), (&is_import_node, "imports")];
            for (predicate, kind) in predicates {
                let mut i = 0;
                while i < children.len() {
                    if !predicate(&children[i]) {
                        i += 1;
                        continue;
                    }
                    let mut j = i;
                    while j + 1 < children.len()
                        && predicate(&children[j + 1])
                        && children[j + 1].start_position().row
                            <= children[j].end_position().row + 1
                    {
                        j += 1;
                    }
                    let (s, e) = (
                        children[i].start_position().row,
                        children[j].end_position().row,
                    );
                    if j > i && e > s {
                        let mut region = FoldRegion::new(s, e, kind);
                        let column = chars(s, children[i].start_position().column);
                        let text =
                            |n: &tree_sitter::Node| self.text.get(n.byte_range()).unwrap_or("");
                        if kind == "comment" {
                            let run: Vec<&str> = children[i..=j].iter().map(text).collect();
                            (region.doc, region.keep, region.collapsed_text) =
                                comment_run_look(&run, column);
                        } else {
                            // The keyword stays: `import ...`.
                            let keyword = text(&children[i])
                                .split_whitespace()
                                .next()
                                .map_or(0, |word| word.chars().count());
                            region.keep = Some(column + keyword + 1);
                        }
                        regions.push(region);
                    }
                    i = j + 1;
                }
            }
            let row = Some(node.start_position().row);
            stack.extend(children.into_iter().rev().map(|c| (c, row)));
        }
        // The import block of a Java or Kotlin file is one region with the blank lines and
        // comments between its groups (`SPEC-0083` v4 behaviour 9).
        if let Some((start, end)) = self.import_block() {
            let text = line(start);
            let indent = text.chars().take_while(|c| c.is_whitespace()).count();
            let keyword = text
                .split_whitespace()
                .next()
                .map_or(0, |word| word.chars().count());
            regions
                .retain(|r| !(r.kind == "imports" && r.start_line >= start && r.end_line <= end));
            let mut region = FoldRegion::new(start, end, "imports");
            region.keep = Some(indent + keyword + 1);
            regions.push(region);
        }
        // A block that ends on the line where the next one starts (`} else {`) ends on the line
        // before, so both can be folded (`SPEC-0083` v8 behaviour 21).
        let starts: std::collections::HashSet<usize> = regions
            .iter()
            .filter(|r| r.kind == "block")
            .map(|r| r.start_line)
            .collect();
        for region in &mut regions {
            if region.kind != "block" {
                continue;
            }
            if starts.contains(&region.end_line) {
                // Its closing bracket stays visible on the next line.
                region.end_line -= 1;
            } else {
                region.collapsed_text =
                    block_placeholder(line(region.start_line), line(region.end_line));
            }
        }
        regions.retain(|r| r.end_line > r.start_line);
        // Custom regions; one that crosses another region — neither inside the other — folds
        // nothing.
        markers.sort_by_key(|(row, _, _)| *row);
        let columns: std::collections::HashMap<usize, usize> = markers
            .iter()
            .map(|(row, column, _)| (*row, *column))
            .collect();
        let marked: Vec<(usize, RegionMarker)> =
            markers.into_iter().map(|(row, _, m)| (row, m)).collect();
        for (start, end, name) in crate::folding::pair_markers(&marked) {
            let crosses = regions.iter().any(|r| {
                let shares = r.start_line <= end && start <= r.end_line;
                let inside = r.start_line >= start && r.end_line <= end;
                let around = r.start_line <= start && r.end_line >= end;
                shares && !inside && !around
            });
            if crosses {
                continue;
            }
            let mut region = FoldRegion::new(start, end, "region");
            region.keep = columns.get(&start).copied();
            if !name.is_empty() {
                region.collapsed_text = name;
            }
            regions.push(region);
        }
        // One region per first line: a custom region before the others, else the longest.
        regions.sort_by(|a, b| {
            a.start_line
                .cmp(&b.start_line)
                .then((a.kind != "region").cmp(&(b.kind != "region")))
                .then(b.end_line.cmp(&a.end_line))
        });
        regions.dedup_by(|later, kept| later.start_line == kept.start_line);
        if let Some((start, end)) = self.outermost_container()
            && let Some(region) = regions
                .iter_mut()
                .find(|r| (r.start_line, r.end_line) == (start, end) && r.kind == "block")
        {
            region.outermost = true;
        }
        regions
    }
}

/// Capture names of the roles [`SyntaxDocument::refined_roles`] finds (`SPEC-0027` v6), by index.
const REFINED_ROLES: [&str; 8] = [
    "type.parameter",
    "variable.builtin.self.python",
    "alias.sql",
    "comment.documentation",
    "property",
    "property.static",
    "variable.reassigned",
    "variable.parameter.reassigned",
];

/// Index of the doc-comment role in [`REFINED_ROLES`] (`SPEC-0027` v10).
const DOC_COMMENT_ROLE: u32 = 3;
/// Index of the field role in [`REFINED_ROLES`] (`SPEC-0027` v10).
const FIELD_ROLE: u32 = 4;
/// Index of the static-field role in [`REFINED_ROLES`] (`SPEC-0027` v10).
const STATIC_FIELD_ROLE: u32 = 5;
/// Index of the reassigned-local role in [`REFINED_ROLES`] (`SPEC-0027` v11).
const REASSIGNED_LOCAL_ROLE: u32 = 6;
/// Index of the reassigned-parameter role in [`REFINED_ROLES`] (`SPEC-0027` v11).
const REASSIGNED_PARAMETER_ROLE: u32 = 7;

/// Whether the comment `text` of `language` is a documentation comment (`SPEC-0027` v10):
/// `/** … */` in Java, Kotlin, JavaScript, TypeScript, Groovy, Scala and PHP (not the empty
/// `/**/`); in Rust also `///` (not `////`), `//!` and `/*! … */`.
fn doc_comment(language: LanguageId, text: &str) -> bool {
    let block = text.starts_with("/**") && !text.starts_with("/**/");
    match language {
        LanguageId::Java
        | LanguageId::Kotlin
        | LanguageId::JavaScript
        | LanguageId::TypeScript
        | LanguageId::Groovy
        | LanguageId::Scala
        | LanguageId::Php => block,
        LanguageId::Rust => {
            block
                || (text.starts_with("///") && !text.starts_with("////"))
                || text.starts_with("//!")
                || text.starts_with("/*!")
        }
        _ => false,
    }
}

/// The capture name a span of `language` shows: SQL data types, `NULL`, `DEFAULT`, `ASC`/`DESC`
/// and the other keywords the grammar's query marks as types or attributes are keywords, its
/// floats numbers (`SPEC-0027` v6).
fn language_role(language: LanguageId, name: &str) -> &str {
    match (language, name) {
        (
            LanguageId::Sql,
            "type.builtin" | "attribute" | "type.qualifier" | "storageclass" | "conditional",
        ) => "keyword",
        (LanguageId::Sql, "float") => "number",
        _ => name,
    }
}

impl SyntaxDocument {
    /// Byte ranges and [`REFINED_ROLES`] indices of the roles a query cannot decide alone, for the
    /// nodes overlapping `lo..hi` (`SPEC-0027` v6): uses of a TypeScript type parameter inside its
    /// declaration; `self`/`cls` as first parameter of a Python method, and its uses; SQL aliases
    /// where they are used; v11: reassigned Java locals and parameters.
    fn refined_roles(&self, lo: usize, hi: usize) -> Vec<(Range<usize>, u32)> {
        if !matches!(
            self.language,
            LanguageId::TypeScript | LanguageId::Python | LanguageId::Sql | LanguageId::Java
        ) {
            return Vec::new();
        }
        let mut out = scoped_walk(self.language, &self.text, self.tree.root_node(), lo, hi);
        // Reassigned locals and parameters (`SPEC-0027` v11).
        if self.language == LanguageId::Java {
            out.extend(
                crate::java_reassigned::reassigned(self.tree.root_node(), &self.text, lo, hi)
                    .into_iter()
                    .map(|(r, parameter)| {
                        let role = if parameter {
                            REASSIGNED_PARAMETER_ROLE
                        } else {
                            REASSIGNED_LOCAL_ROLE
                        };
                        (r, role)
                    }),
            );
        }
        out
    }
}

/// Prefixes of the names a Java scope declares (`SPEC-0027` v10): a field, a static field, and a
/// parameter or local variable, which hides a field of the same name.
const JAVA_FIELD: &str = "f:";
const JAVA_STATIC_FIELD: &str = "s:";
const JAVA_LOCAL: &str = "l:";

/// Whether a Java declaration (field, method, class) carries the modifier `static`.
fn java_static(declaration: tree_sitter::Node, text: &str) -> bool {
    let mut c = declaration.walk();
    declaration
        .children(&mut c)
        .filter(|m| m.kind() == "modifiers")
        .any(|m| {
            let mut mc = m.walk();
            m.children(&mut mc)
                .any(|k| &text[k.byte_range()] == "static")
        })
}

/// The names of the variable declarators of a Java field or local variable declaration.
fn java_declarators(declaration: tree_sitter::Node, text: &str) -> Vec<String> {
    let mut c = declaration.walk();
    declaration
        .children_by_field_name("declarator", &mut c)
        .filter_map(|d| d.child_by_field_name("name"))
        .map(|n| text[n.byte_range()].to_owned())
        .collect()
}

/// The names a Java node declares for its subtree (`SPEC-0027` v10): a class, interface, enum or
/// record body its fields (interface fields and enum constants are static), a record its
/// components; a method, constructor, lambda, block, loop, `catch` or `try` its parameters and
/// local variables, which hide fields of the same name.
fn java_declared_names(node: tree_sitter::Node, text: &str) -> Vec<String> {
    let name_of = |n: tree_sitter::Node| text[n.byte_range()].to_owned();
    let mut out = Vec::new();
    let mut c = node.walk();
    match node.kind() {
        "class_body" | "enum_body" | "enum_body_declarations" | "interface_body" => {
            for member in node.named_children(&mut c) {
                match member.kind() {
                    "field_declaration" => {
                        let prefix = if java_static(member, text) {
                            JAVA_STATIC_FIELD
                        } else {
                            JAVA_FIELD
                        };
                        out.extend(
                            java_declarators(member, text)
                                .into_iter()
                                .map(|n| format!("{prefix}{n}")),
                        );
                    }
                    "constant_declaration" => out.extend(
                        java_declarators(member, text)
                            .into_iter()
                            .map(|n| format!("{JAVA_STATIC_FIELD}{n}")),
                    ),
                    "enum_constant" => {
                        if let Some(n) = member.child_by_field_name("name") {
                            out.push(format!("{JAVA_STATIC_FIELD}{}", name_of(n)));
                        }
                    }
                    _ => {}
                }
            }
        }
        "record_declaration" => {
            if let Some(params) = node.child_by_field_name("parameters") {
                let mut pc = params.walk();
                out.extend(
                    params
                        .named_children(&mut pc)
                        .filter_map(|p| p.child_by_field_name("name"))
                        .map(|n| format!("{JAVA_FIELD}{}", name_of(n))),
                );
            }
        }
        "method_declaration"
        | "constructor_declaration"
        | "compact_constructor_declaration"
        | "lambda_expression" => {
            let params = node.child_by_field_name("parameters");
            if let Some(params) = params {
                if params.kind() == "identifier" {
                    // `x -> …`
                    out.push(format!("{JAVA_LOCAL}{}", name_of(params)));
                } else {
                    let mut pc = params.walk();
                    for p in params.named_children(&mut pc) {
                        let name = match p.kind() {
                            "identifier" => Some(p),
                            _ => p.child_by_field_name("name").or_else(|| {
                                let mut vc = p.walk();
                                p.named_children(&mut vc)
                                    .find(|v| v.kind() == "variable_declarator")
                                    .and_then(|v| v.child_by_field_name("name"))
                            }),
                        };
                        if let Some(n) = name {
                            out.push(format!("{JAVA_LOCAL}{}", name_of(n)));
                        }
                    }
                }
            }
        }
        "enhanced_for_statement" => {
            if let Some(n) = node.child_by_field_name("name") {
                out.push(format!("{JAVA_LOCAL}{}", name_of(n)));
            }
        }
        "catch_clause" => {
            for p in node.named_children(&mut c) {
                if p.kind() == "catch_formal_parameter"
                    && let Some(n) = p.child_by_field_name("name")
                {
                    out.push(format!("{JAVA_LOCAL}{}", name_of(n)));
                }
            }
        }
        _ => {}
    }
    // Local variables declared directly in a block, a loop header, a `switch` group or a `try`
    // resource list hide fields in the whole of it.
    if matches!(
        node.kind(),
        "block"
            | "constructor_body"
            | "for_statement"
            | "switch_block_statement_group"
            | "resource_specification"
    ) {
        let mut c = node.walk();
        for child in node.named_children(&mut c) {
            match child.kind() {
                "local_variable_declaration" => out.extend(
                    java_declarators(child, text)
                        .into_iter()
                        .map(|n| format!("{JAVA_LOCAL}{n}")),
                ),
                "resource" => {
                    if let Some(n) = child.child_by_field_name("name") {
                        out.push(format!("{JAVA_LOCAL}{}", name_of(n)));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The field role of a Java identifier (`SPEC-0027` v10): the name of a field or enum constant
/// where it is declared; the name after the dot of a field access (`this.count`, `p.x`; a name in
/// capitals such as `Color.RED` is static); a name the enclosing classes declare as a field and no
/// enclosing method, lambda or block hides. Method names, types, labels and the names of locals
/// and parameters keep their colour.
fn java_field_role(node: tree_sitter::Node, names: &[String], text: &str) -> Option<u32> {
    if node.kind() != "identifier" {
        return None;
    }
    let name = &text[node.byte_range()];
    let parent = node.parent()?;
    let is = |field: &str| parent.child_by_field_name(field) == Some(node);
    let role = |is_static: bool| {
        if is_static {
            STATIC_FIELD_ROLE
        } else {
            FIELD_ROLE
        }
    };
    let capitals = |n: &str| {
        n.chars().any(|c| c.is_ascii_uppercase())
            && n.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    match parent.kind() {
        // Where a field is declared.
        "variable_declarator" if is("name") => {
            let declaration = parent.parent()?;
            match declaration.kind() {
                "field_declaration" => Some(role(java_static(declaration, text))),
                "constant_declaration" => Some(STATIC_FIELD_ROLE),
                _ => None,
            }
        }
        "enum_constant" if is("name") => Some(STATIC_FIELD_ROLE),
        // A record component where it is declared.
        "formal_parameter"
            if is("name")
                && parent
                    .parent()
                    .and_then(|p| p.parent())
                    .is_some_and(|r| r.kind() == "record_declaration") =>
        {
            Some(FIELD_ROLE)
        }
        // `this.count`, `p.x`, `Color.RED`; `a.B` with a capital first letter is a type.
        "field_access" if is("field") => {
            if capitals(name) {
                Some(STATIC_FIELD_ROLE)
            } else if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                None
            } else {
                Some(FIELD_ROLE)
            }
        }
        // Names that are no variable use.
        "method_invocation" if is("name") => None,
        "method_declaration"
        | "constructor_declaration"
        | "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration"
        | "labeled_statement"
        | "break_statement"
        | "continue_statement"
        | "scoped_identifier"
        | "package_declaration"
        | "import_declaration"
        | "marker_annotation"
        | "annotation"
        | "method_reference" => None,
        "variable_declarator"
        | "formal_parameter"
        | "catch_formal_parameter"
        | "enhanced_for_statement"
        | "resource"
        | "spread_parameter"
            if is("name") =>
        {
            None
        }
        "lambda_expression" if is("parameters") => None,
        "inferred_parameters" => None,
        // `obj.method()`: `obj` is a use; `a.b` where `a` is the object: a use too.
        _ => {
            let found = names.iter().rev().find(|n| {
                n.get(2..) == Some(name)
                    && [JAVA_FIELD, JAVA_STATIC_FIELD, JAVA_LOCAL]
                        .iter()
                        .any(|p| n.starts_with(p))
            })?;
            if found.starts_with(JAVA_LOCAL) {
                None
            } else {
                Some(role(found.starts_with(JAVA_STATIC_FIELD)))
            }
        }
    }
}

/// The names `node` declares for its subtree (`SPEC-0027` v6): a TypeScript declaration its type
/// parameters, a Python method its `self`/`cls`, a SQL statement its aliases; v10: a Java class
/// its fields, a Java method or block its parameters and locals.
fn declared_names(language: LanguageId, node: tree_sitter::Node, text: &str) -> Vec<String> {
    match language {
        LanguageId::Java => java_declared_names(node, text),
        LanguageId::TypeScript => {
            let Some(params) = node.child_by_field_name("type_parameters") else {
                return Vec::new();
            };
            let mut c = params.walk();
            params
                .named_children(&mut c)
                .filter(|p| p.kind() == "type_parameter")
                .filter_map(|p| p.child_by_field_name("name"))
                .map(|name| text[name.byte_range()].to_owned())
                .collect()
        }
        LanguageId::Python => python_self(node, text).into_iter().collect(),
        LanguageId::Sql if node.kind() == "statement" => sql_aliases(node, text),
        _ => Vec::new(),
    }
}

/// The [`REFINED_ROLES`] index of `node` when it uses one of the declared `names` (or, in Java,
/// declares or accesses a field).
fn name_role(
    language: LanguageId,
    node: tree_sitter::Node,
    names: &[String],
    text: &str,
) -> Option<u32> {
    match language {
        LanguageId::TypeScript => uses_name(language, node, names, text).then_some(0),
        LanguageId::Python => uses_name(language, node, names, text).then_some(1),
        LanguageId::Sql => uses_name(language, node, names, text).then_some(2),
        LanguageId::Java => java_field_role(node, names, text),
        _ => None,
    }
}

/// Whether `node` uses one of the declared `names`.
fn uses_name(language: LanguageId, node: tree_sitter::Node, names: &[String], text: &str) -> bool {
    let name = &text[node.byte_range()];
    match language {
        LanguageId::TypeScript => {
            node.kind() == "type_identifier" && names.iter().any(|x| x == name)
        }
        LanguageId::Python => {
            // `p.self` names an attribute, not the parameter.
            let attribute_name = node.parent().is_some_and(|p| {
                p.kind() == "attribute" && p.child_by_field_name("attribute") == Some(node)
            });
            node.kind() == "identifier" && !attribute_name && names.iter().any(|x| x == name)
        }
        LanguageId::Sql => {
            if node.kind() != "identifier" || !names.iter().any(|x| x.eq_ignore_ascii_case(name)) {
                return false;
            }
            let Some(parent) = node.parent() else {
                return false;
            };
            match parent.kind() {
                // `ORDER BY n`: a field without a table in front.
                "field" => {
                    parent.child_by_field_name("name") == Some(node)
                        && !has_child(parent, "object_reference")
                }
                // `p.name`: the table alias in front of a field.
                "object_reference" => parent.parent().is_some_and(|f| f.kind() == "field"),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Whether `node` has a direct child of `kind`.
fn has_child(node: tree_sitter::Node, kind: &str) -> bool {
    let mut c = node.walk();
    node.children(&mut c).any(|ch| ch.kind() == kind)
}

/// Walks the nodes overlapping `lo..hi` (iteratively: trees can be very deep) with the names the
/// enclosing nodes declare, and returns the byte ranges and [`REFINED_ROLES`] indices of the
/// nodes that use one of them (in Java also of fields declared or accessed).
fn scoped_walk(
    language: LanguageId,
    text: &str,
    root: tree_sitter::Node,
    lo: usize,
    hi: usize,
) -> Vec<(Range<usize>, u32)> {
    let mut out = Vec::new();
    let mut names: Vec<String> = Vec::new();
    // The length of `names` before each entered node.
    let mut marks: Vec<usize> = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        marks.push(names.len());
        names.extend(declared_names(language, node, text));
        if (!names.is_empty() || language == LanguageId::Java)
            && let Some(role) = name_role(language, node, &names, text)
        {
            out.push((node.byte_range(), role));
        }
        if cursor.goto_first_child_for_byte(lo).is_some() {
            if cursor.node().start_byte() < hi {
                continue;
            }
            cursor.goto_parent();
        }
        loop {
            if let Some(mark) = marks.pop() {
                names.truncate(mark);
            }
            if cursor.goto_next_sibling() && cursor.node().start_byte() < hi {
                break;
            }
            if marks.is_empty() || !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// The name of the first parameter of a Python method when it is `self` or `cls` (not for static
/// methods, which have none).
fn python_self(node: tree_sitter::Node, text: &str) -> Option<String> {
    if node.kind() != "function_definition" {
        return None;
    }
    let mut owner = node.parent()?;
    if owner.kind() == "decorated_definition" {
        let mut c = owner.walk();
        let is_static = owner
            .children(&mut c)
            .any(|d| d.kind() == "decorator" && text[d.byte_range()].trim() == "@staticmethod");
        if is_static {
            return None;
        }
        owner = owner.parent()?;
    }
    if owner.kind() != "block" || owner.parent()?.kind() != "class_definition" {
        return None;
    }
    let params = node.child_by_field_name("parameters")?;
    let mut c = params.walk();
    let first = params
        .named_children(&mut c)
        .find(|p| p.kind() != "comment")?;
    let name = match first.kind() {
        "identifier" => first,
        "typed_parameter" => first.named_child(0)?,
        "default_parameter" | "typed_default_parameter" => first.child_by_field_name("name")?,
        _ => return None,
    };
    let name = &text[name.byte_range()];
    matches!(name, "self" | "cls").then(|| name.to_owned())
}

/// The aliases a SQL statement defines (`AS n`, `FROM person p`).
fn sql_aliases(statement: tree_sitter::Node, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![statement];
    while let Some(n) = stack.pop() {
        if matches!(n.kind(), "term" | "relation")
            && let Some(alias) = n.child_by_field_name("alias")
        {
            out.push(text[alias.byte_range()].to_owned());
        }
        let mut c = n.walk();
        stack.extend(n.named_children(&mut c));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0065 v4 behaviour 13: matches in comments and string literals told apart.
    #[test]
    fn search_contexts_tell_comments_and_strings_apart() {
        let text = "// total\nlong total = 0;\nString s = \"total\";\n";
        let path = std::path::Path::new("A.java");
        let all: Vec<Range<usize>> = {
            let mut v = Vec::new();
            let mut from = 0;
            while let Some(i) = text[from..].find("total") {
                let s = text[..from + i].chars().count();
                v.push(s..s + 5);
                from += i + 5;
            }
            v
        };
        assert_eq!(all.len(), 3);
        let keep = |c: SearchContext| c.filter(path, text, all.clone()).len();
        assert_eq!(keep(SearchContext::Anywhere), 3);
        assert_eq!(keep(SearchContext::InComments), 1);
        assert_eq!(keep(SearchContext::InStrings), 1);
        assert_eq!(keep(SearchContext::ExceptComments), 2);
        assert_eq!(keep(SearchContext::ExceptStrings), 2);
        assert_eq!(keep(SearchContext::ExceptCommentsAndStrings), 1);
        assert_eq!(
            SearchContext::InComments
                .filter(std::path::Path::new("a.unknown"), text, all.clone())
                .len(),
            0
        );
    }

    /// SPEC-0056 v4 AK-12.
    #[test]
    fn list_elements_around_an_offset() {
        let shown = |language: LanguageId, text: &str, at: &str| -> Vec<String> {
            let doc = SyntaxDocument::new(language, false, text).expect("grammar");
            let at = text[..text.find(at).expect("marker")].chars().count();
            doc.list_elements(at)
                .into_iter()
                .map(|r| text.chars().skip(r.start).take(r.len()).collect())
                .collect()
        };
        let java = "class A {\n    int x = 1, y = 2;\n    void m() {\n        call(inner(ä, b), c);\n    }\n}\n";
        assert_eq!(shown(LanguageId::Java, java, "ä, b"), ["ä", "b"]);
        assert_eq!(
            shown(LanguageId::Java, java, ", b)"),
            ["ä", "b"],
            "at an edge"
        );
        assert_eq!(
            shown(LanguageId::Java, java, "inner"),
            ["inner(ä, b)", "c"],
            "on the name of the inner call the outer list is meant"
        );
        assert_eq!(shown(LanguageId::Java, java, "c);"), ["inner(ä, b)", "c"]);
        // A declaration with its type and two declarators is no list; a statement neither.
        assert!(shown(LanguageId::Java, java, "x = 1").is_empty());
        assert!(shown(LanguageId::Java, java, "call(").is_empty());
        let python = "def f(a, b=1, *c):\n    return [a, b,]\n";
        assert_eq!(shown(LanguageId::Python, python, "b=1"), ["a", "b=1", "*c"]);
        assert_eq!(
            shown(LanguageId::Python, python, "a, b,]"),
            ["a", "b"],
            "nothing after the trailing comma"
        );
        let rust = "fn main() {\n    let v = [1, 22, 3];\n}\n";
        assert_eq!(shown(LanguageId::Rust, rust, "22"), ["1", "22", "3"]);
        let json = "{\"a\": 1, \"b\": [true, false]}\n";
        assert_eq!(
            shown(LanguageId::Json, json, "\"a\""),
            ["\"a\": 1", "\"b\": [true, false]"]
        );
        assert_eq!(shown(LanguageId::Json, json, "false"), ["true", "false"]);
    }

    /// SPEC-0071 v9 AK-8.
    #[test]
    fn method_offsets() {
        let names = |language: LanguageId, text: &str| -> Vec<String> {
            let doc = SyntaxDocument::new(language, false, text).expect("grammar");
            doc.method_offsets()
                .into_iter()
                .map(|o| {
                    text.chars()
                        .skip(o)
                        .take_while(|c| c.is_alphanumeric())
                        .collect()
                })
                .collect()
        };
        let java = "class Größe {\n    int feld = 1;\n    int eins() { return 1; }\n\n    void zwei() {}\n}\n";
        assert_eq!(names(LanguageId::Java, java), ["eins", "zwei"]);
        let python = "class A:\n    def f(self):\n        pass\n\ndef g():\n    pass\n";
        assert_eq!(names(LanguageId::Python, python), ["f", "g"]);
        assert!(names(LanguageId::Json, "{\"a\": 1}\n").is_empty());
    }

    /// SPEC-0057 v3 behaviour 13 (AK-10): the stages of OBS-0018 item 1.
    #[test]
    fn syntax_selection_stages() {
        let text = "class Edit {\n    String greet(String name) {\n        String a = \"Hello, \" + name;\n        String b = a.trim();\n        return b;\n    }\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).expect("java");
        let at = text.find("Hello").unwrap() + 2;
        let ranges = doc.selection_ranges(at..at);
        let shown: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
        let want = [
            "Hello, ",
            "\"Hello, \"",
            "\"Hello, \" + name",
            "String a = \"Hello, \" + name;",
            "        String a = \"Hello, \" + name;\n",
            "        String a = \"Hello, \" + name;\n        String b = a.trim();\n        return b;\n",
        ];
        for (i, w) in want.iter().enumerate() {
            assert!(shown.get(i) == Some(w), "stage {i}: {:?}", shown.get(i));
        }
        assert!(
            shown
                .iter()
                .any(|s| s.starts_with("{\n") && s.ends_with("    }"))
        );
        assert!(shown.iter().any(|s| s.starts_with("String greet")));
        assert!(
            !shown.contains(&"a = \"Hello, \" + name"),
            "declarator left out"
        );
    }

    #[test]
    fn comment_ranges_match_the_comment_highlights() {
        let samples = [
            (
                LanguageId::Rust,
                "// a\nfn f() { /* b */ let x = 1; } /// doc\n",
            ),
            (
                LanguageId::Java,
                "/** Doc. */\nclass A { // c\n int x; /* d */ }\n",
            ),
            (LanguageId::Python, "# a\ndef f():\n    # b\n    return 1\n"),
            (LanguageId::Sql, "-- a\nSELECT 1; /* b */\n"),
            (
                LanguageId::Html,
                "<!-- a --><script>// b\nlet x;</script><style>/* c */</style>",
            ),
            (LanguageId::Go, "package a\n// b\nfunc f() {} /* c */\n"),
        ];
        for (language, text) in samples {
            let doc = SyntaxDocument::new(language, false, text).unwrap();
            let byte = |c: usize| byte_of(text, c).unwrap();
            let mut expected: Vec<Range<usize>> = doc
                .highlight()
                .into_iter()
                .filter(|s| s.kind.starts_with("comment"))
                .map(|s| byte(s.start)..byte(s.end))
                .collect();
            expected.sort_by_key(|r| r.start);
            expected.dedup_by(|b, a| b.start < a.end);
            assert!(!expected.is_empty(), "{language:?}");
            let found = doc.comment_ranges();
            // Every comment the highlighting shows is found; SQL line comments are captured as
            // `@comment @spell`, so highlighting paints them `spell` but they are comments too.
            assert!(
                expected.iter().all(|r| found.contains(r)),
                "{language:?}: {found:?} vs {expected:?}"
            );
            if language == LanguageId::Sql {
                assert_eq!(found, [0..4, 15..22]);
            } else {
                assert_eq!(found, expected, "{language:?}");
            }
        }
    }

    #[test]
    fn byte_of_counts_chars_across_chunks() {
        let text = format!("{}ü{}é", "a".repeat(70), "ß".repeat(40));
        for chars in 0..=text.chars().count() {
            let expected = text
                .char_indices()
                .map(|(b, _)| b)
                .chain([text.len()])
                .nth(chars);
            assert_eq!(byte_of(&text, chars), expected, "char {chars}");
        }
        assert_eq!(byte_of(&text, text.chars().count() + 1), None);
        assert_eq!(byte_of("", 0), Some(0));
    }

    fn kinds_of<'a>(text: &'a str, spans: &'a [Span]) -> Vec<(&'a str, &'a str)> {
        spans
            .iter()
            .map(|s| {
                let a = text
                    .char_indices()
                    .nth(s.start)
                    .map_or(text.len(), |(b, _)| b);
                let b = text
                    .char_indices()
                    .nth(s.end)
                    .map_or(text.len(), |(b, _)| b);
                (&text[a..b], s.kind.as_str())
            })
            .collect()
    }

    #[test]
    fn rust_spans_at_char_offsets() {
        let text = "// Größe\nfn main() { let x = \"hé\"; }\n";
        let doc = SyntaxDocument::new(LanguageId::Rust, false, text).unwrap();
        let spans = doc.highlight();
        let kinds = kinds_of(text, &spans);
        assert!(kinds.contains(&("// Größe", "comment")), "{kinds:?}");
        assert!(kinds.contains(&("fn", "keyword")), "{kinds:?}");
        assert!(kinds.contains(&("let", "keyword")), "{kinds:?}");
        assert!(kinds.contains(&("main", "function")), "{kinds:?}");
        assert!(
            kinds
                .iter()
                .any(|(t, k)| t.contains("hé") && *k == "string"),
            "{kinds:?}"
        );
        assert!(
            spans.windows(2).all(|w| w[0].end <= w[1].start),
            "sorted, no overlap"
        );
    }

    #[test]
    fn every_language_highlights_a_sample() {
        let samples = [
            (LanguageId::Python, false, "def f(x):\n    return 'a'\n"),
            (
                LanguageId::JavaScript,
                false,
                "const a = <b>{'c'}</b>; function f() { return 1; }",
            ),
            (
                LanguageId::TypeScript,
                false,
                "interface A { x: string }\nconst y: number = 1;",
            ),
            (
                LanguageId::TypeScript,
                true,
                "const e = <div className=\"x\">{1}</div>;",
            ),
            (LanguageId::Java, false, "class A { String s = \"x\"; }"),
            (
                LanguageId::Go,
                false,
                "package main\nfunc main() { s := \"x\" }",
            ),
            (LanguageId::C, false, "int main(void) { return 0; } /* c */"),
            (
                LanguageId::Cpp,
                false,
                "struct S { virtual int f() const = 0; }; // c++",
            ),
            (LanguageId::Sql, false, "SELECT a FROM t WHERE b = 'x';"),
            (LanguageId::Json, false, "{\"a\": [1, true, null]}"),
            (LanguageId::Html, false, "<p class=\"x\">hi</p><!-- c -->"),
            (LanguageId::Css, false, "a { color: red; } /* c */"),
            (
                LanguageId::Xml,
                false,
                "<project a=\"x\"><!-- c --></project>",
            ),
            (
                LanguageId::Yaml,
                false,
                "name: \"x\" # c\nlist: [1, true]\n",
            ),
            (LanguageId::Markdown, false, "# Title\n\n```\ncode\n```\n"),
            (LanguageId::Toml, false, "[package]\nname = \"x\" # c\n"),
            (
                LanguageId::Shell,
                false,
                "if true; then echo \"x\"; fi # c\n",
            ),
            (
                LanguageId::Dockerfile,
                false,
                "FROM alpine\nRUN echo \"x\" # c\n",
            ),
            (
                LanguageId::Groovy,
                false,
                "class A { def s = \"x\" } // c\n",
            ),
            (
                LanguageId::Scala,
                false,
                "object A { val s = \"x\" } // c\n",
            ),
            (
                LanguageId::Scss,
                false,
                "$a: red;\n.b { color: $a; } // c\n",
            ),
            (
                LanguageId::Less,
                false,
                "@a: red;\n.b { color: @a; } // c\n",
            ),
            (
                LanguageId::Swift,
                false,
                "func f() -> String { return \"x\" } // c\n",
            ),
            (
                LanguageId::Lua,
                false,
                "local function f() return \"x\" end -- c\n",
            ),
            (LanguageId::R, false, "f <- function(x) { \"x\" } # c\n"),
            (
                LanguageId::Ruby,
                false,
                "class A\n  def f(x) = \"x#{x}\" # c\nend\n",
            ),
            (
                LanguageId::Php,
                false,
                "<?php\nfunction f($x) { return \"x\"; } // c\n",
            ),
            (
                LanguageId::Vue,
                false,
                "<template><p class=\"x\">{{ a }}</p></template><!-- c -->\n",
            ),
            (
                LanguageId::Svelte,
                false,
                "<script>let a = 1</script><p class=\"x\">{a}</p><!-- c -->\n",
            ),
            (
                LanguageId::Kotlin,
                false,
                "fun main() { val s = \"x\" } // c\n",
            ),
        ];
        for (language, tsx, text) in samples {
            let spans = SyntaxDocument::new(language, tsx, text)
                .unwrap()
                .highlight();
            assert!(
                spans.iter().any(|s| [
                    "keyword",
                    "string",
                    "tag",
                    "property",
                    "constant.builtin",
                    "comment",
                    "text.title"
                ]
                .iter()
                .any(|k| s.kind.starts_with(k))),
                "{language:?}: {spans:?}"
            );
            assert!(
                spans.windows(2).all(|w| w[0].end <= w[1].start),
                "{language:?}"
            );
        }
    }

    /// SPEC-0141 v3 (hand test 95): SCSS and Less colour selectors, properties, values and
    /// variables like CSS, also inside a Vue style block.
    #[test]
    fn scss_and_less_are_coloured_like_css() {
        let scss = "$farbe: #3366ff;\n.karte {\n  color: darken($farbe, 10%);\n  .titel { font-weight: bold; }\n}\n";
        let want = [
            ("$farbe", "variable"),
            ("3366ff", "string.special"),
            ("karte", "property"),
            ("color", "property"),
            ("darken", "function"),
            ("10", "number"),
            ("titel", "property"),
        ];
        let doc = SyntaxDocument::new(LanguageId::Scss, false, scss).unwrap();
        let spans = doc.highlight();
        let kinds = kinds_of(scss, &spans);
        for w in want {
            assert!(kinds.contains(&w), "{w:?} in {kinds:?}");
        }
        assert!(!kinds.contains(&("$farbe", "property")), "{kinds:?}");
        let less = scss.replace('$', "@");
        let doc = SyntaxDocument::new(LanguageId::Less, false, &less).unwrap();
        let spans = doc.highlight();
        let kinds = kinds_of(&less, &spans);
        for (t, k) in want {
            let t = t.replace('$', "@");
            assert!(kinds.contains(&(t.as_str(), k)), "{t} {k} in {kinds:?}");
        }
        let vue = format!(
            "<template><div></div></template>\n<style lang=\"scss\" scoped>\n{scss}</style>\n"
        );
        let doc = SyntaxDocument::new(LanguageId::Vue, false, &vue).unwrap();
        let spans = doc.highlight();
        let kinds = kinds_of(&vue, &spans);
        for w in want {
            assert!(kinds.contains(&w), "vue: {w:?} in {kinds:?}");
        }
    }

    /// SPEC-0141 v2 AK-6: script and style blocks of Vue, Svelte and HTML in their own colours.
    #[test]
    fn embedded_script_and_style_blocks() {
        let text = "<template><p>{{ n }}</p></template>\n<script setup lang=\"ts\">\nconst n: number = 1;\n</script>\n<style lang=\"scss\">\n$c: red;\n.a { color: $c; }\n</style>\n";
        let doc = SyntaxDocument::new(LanguageId::Vue, false, text).unwrap();
        let spans = doc.highlight();
        let kinds = kinds_of(text, &spans);
        assert!(kinds.contains(&("const", "keyword")), "{kinds:?}");
        assert!(kinds.contains(&("number", "type.builtin")), "{kinds:?}");
        assert!(kinds.contains(&("template", "tag")), "{kinds:?}");
        assert!(kinds.iter().any(|(t, _)| *t == "$c"), "{kinds:?}");
        assert!(
            spans.windows(2).all(|w| w[0].end <= w[1].start),
            "{kinds:?}"
        );
        // Only part of the document: spans of the block stay inside the range.
        let start = text.find("const").unwrap();
        let part = doc.highlight_range(start..start + 5);
        assert_eq!(kinds_of(text, &part), [("const", "keyword")]);

        let svelte = "<script>\nlet a = 'x';\n</script>\n<p>{a}</p>\n";
        let kinds_svelte = kinds_of(
            svelte,
            &SyntaxDocument::new(LanguageId::Svelte, false, svelte)
                .unwrap()
                .highlight(),
        )
        .into_iter()
        .map(|(t, k)| (t.to_owned(), k.to_owned()))
        .collect::<Vec<_>>();
        assert!(
            kinds_svelte.contains(&("let".into(), "keyword".into())),
            "{kinds_svelte:?}"
        );
        let html =
            "<script type=\"text/template\"><b>x</b></script><style>a { color: red; }</style>";
        let html_spans = SyntaxDocument::new(LanguageId::Html, false, html)
            .unwrap()
            .highlight();
        let kinds = kinds_of(html, &html_spans);
        assert!(kinds.iter().any(|(t, _)| *t == "color"), "{kinds:?}");
        assert!(
            !kinds.iter().any(|(t, _)| *t == "b"),
            "templates stay plain: {kinds:?}"
        );

        // An edit inside the block repaints the whole block and keeps the colours right.
        let mut doc = SyntaxDocument::new(LanguageId::Vue, false, text).unwrap();
        let at = text.find("1;").unwrap();
        let changed = doc.edit(at..at + 1, "'s'").unwrap();
        let block = text.find("\nconst").unwrap();
        assert!(
            changed.iter().any(|r| r.start <= block + 1 && r.end > at),
            "{changed:?}"
        );
        let new_text = doc.text().to_owned();
        let kinds = kinds_of(&new_text, &doc.highlight())
            .into_iter()
            .map(|(t, k)| (t.to_owned(), k.to_owned()))
            .collect::<Vec<_>>();
        assert!(
            kinds.contains(&("'s'".into(), "string".into())),
            "{kinds:?}"
        );
    }

    #[test]
    fn incremental_edits_match_a_fresh_parse() {
        let mut doc = SyntaxDocument::new(LanguageId::Python, false, "x = 1\n").unwrap();
        let changed = doc.edit(4..5, "'ü'").unwrap();
        assert!(
            changed.iter().any(|r| r.start <= 4 && r.end >= 7),
            "{changed:?}"
        );
        // Deterministic pseudo-random edits.
        let pieces = [
            "def ", "(", ")", ":\n", "  ", "'s'", "# c\n", "1", "if x", "\n", "é", "\"",
        ];
        let mut seed: u64 = 7;
        for _ in 0..200 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = doc.text().chars().count();
            let a = (seed >> 33) as usize % (len + 1);
            let b = (a + ((seed >> 20) as usize % 3)).min(len);
            let piece = pieces[(seed >> 40) as usize % pieces.len()];
            doc.edit(a..b, piece).unwrap();
        }
        let fresh = SyntaxDocument::new(LanguageId::Python, false, doc.text()).unwrap();
        assert_eq!(doc.highlight(), fresh.highlight());
        assert!(doc.edit(0..999, "x").is_err());
    }

    fn folds(language: LanguageId, text: &str) -> Vec<(usize, usize, String)> {
        SyntaxDocument::new(language, false, text)
            .unwrap()
            .folding()
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.kind))
            .collect()
    }

    // SPEC-0061 v3 AK-1a.
    #[test]
    fn definitions_around_a_position() {
        let around = |l, text: &str, needle: &str| -> Vec<String> {
            let doc = SyntaxDocument::new(l, false, text).unwrap();
            doc.definitions_around(text.find(needle).unwrap())
                .into_iter()
                .map(|d| format!("{}:{}", d.kind, d.name))
                .collect()
        };
        let java = "package lab;\n\npublic class Inventory {\n    int n;\n    public static void main(String[] args) {\n        int total = 0;\n    }\n}\n";
        assert_eq!(
            around(LanguageId::Java, java, "total"),
            ["class:Inventory", "method:main"]
        );
        assert_eq!(around(LanguageId::Java, java, "int n"), ["class:Inventory"]);
        assert!(around(LanguageId::Java, java, "package").is_empty());
        let py = "class A:\n    def run(self):\n        x = 1\n";
        assert_eq!(
            around(LanguageId::Python, py, "x = 1"),
            ["class:A", "function:run"]
        );
        let rs = "fn main() {\n    let a = 1;\n}\n";
        assert_eq!(around(LanguageId::Rust, rs, "let a"), ["function:main"]);
        let json = SyntaxDocument::new(LanguageId::Json, false, "{\"a\": 1}").unwrap();
        assert!(json.definitions_around(2).is_empty(), "no tags query");
    }

    // SPEC-0292 v4 AK-8.
    #[test]
    fn declaration_spans_cover_their_nodes() {
        let java = "package lab;\n\npublic class Inventory {\n    int n;\n\n    @Override\n    public String toString() {\n        return \"x\";\n    }\n\n    record Item(int q) {}\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Java, false, java).unwrap();
        let spans: Vec<(String, String, usize, usize, usize)> = doc
            .declaration_spans(200)
            .into_iter()
            .map(|d| (d.name, d.kind, d.line, d.start_line, d.end_line))
            .collect();
        assert!(
            spans.contains(&("Inventory".into(), "class".into(), 2, 2, 11)),
            "{spans:?}"
        );
        assert!(
            spans.contains(&("toString".into(), "method".into(), 6, 5, 8)),
            "{spans:?}"
        );
        assert!(spans.iter().all(|s| s.0 != "n"), "no fields: {spans:?}");
        assert!(
            spans.contains(&("Item".into(), "class".into(), 10, 10, 10)),
            "records too: {spans:?}"
        );
        assert_eq!(doc.declaration_spans(1).len(), 1);
        let json = SyntaxDocument::new(LanguageId::Json, false, "{\"a\": 1}").unwrap();
        assert!(json.declaration_spans(200).is_empty());
    }

    #[test]
    fn definitions_from_tags_queries() {
        let defs = |l, text: &str| -> Vec<(String, String, usize)> {
            SyntaxDocument::new(l, false, text)
                .unwrap()
                .definitions()
                .into_iter()
                .map(|d| (d.name, d.kind, d.line))
                .collect()
        };
        assert_eq!(
            defs(
                LanguageId::Rust,
                "struct Point;\ntrait Shape {}\nimpl Point {\n    fn area(&self) {}\n}\nfn main() {}\n"
            ),
            [
                ("Point".into(), "class".into(), 0),
                ("Shape".into(), "interface".into(), 1),
                ("area".into(), "method".into(), 3),
                ("main".into(), "function".into(), 5),
            ]
        );
        assert_eq!(
            defs(
                LanguageId::Python,
                "class Ä:\n    def run(self):\n        pass\n"
            ),
            [
                ("Ä".into(), "class".into(), 0),
                ("run".into(), "function".into(), 1)
            ]
        );
        let ts = defs(
            LanguageId::TypeScript,
            "interface Shape { area(): number }\nfunction make(): void {}\nclass Box {}\n",
        );
        assert!(
            ts.contains(&("Shape".into(), "interface".into(), 0)),
            "{ts:?}"
        );
        assert!(
            ts.contains(&("make".into(), "function".into(), 1)),
            "{ts:?}"
        );
        assert!(ts.contains(&("Box".into(), "class".into(), 2)), "{ts:?}");
        assert!(defs(LanguageId::Json, "{}").is_empty());
    }

    /// SPEC-0083 v2 AK-4: one-statement bodies on their own line are found with their placeholder;
    /// longer bodies, bodies on the signature's line, braces alone on a line and other languages
    /// are not.
    #[test]
    fn java_one_line_bodies() {
        let text = "class A {\n    int total() {\n        return qty * price;\n    }\n    A(int q) {\n        this.q = q;\n    }\n    void two() {\n        a();\n        b();\n    }\n    void inline() { c(); }\n    void allman()\n    {\n        d();\n    }\n    void commented() {\n        // nothing\n    }\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let bodies = doc.one_line_bodies();
        let found: Vec<(usize, usize, &str)> = bodies
            .iter()
            .map(|b| (b.start_line, b.end_line, b.placeholder.as_str()))
            .collect();
        assert_eq!(
            found,
            [(1, 3, " return qty * price; }"), (4, 6, " this.q = q; }")]
        );
        let rust = SyntaxDocument::new(LanguageId::Rust, false, "fn f() {\n    g();\n}\n").unwrap();
        assert!(rust.one_line_bodies().is_empty());
    }

    /// SPEC-0083 AK-31 (v10, behaviour 7a): the conditions of the open platform's default — the
    /// name and the parameter list's end on one line, and the width of the folded line for the
    /// right margin; the method of OBS-0056 folds within the default margin.
    #[test]
    fn one_line_bodies_follow_the_open_conditions() {
        let text = "record Item(int qty, int unitPriceCents) {\n    int totalCents() {\n        return qty * unitPriceCents;\n    }\n    int wrapped(int a,\n                int b) {\n        return a + b;\n    }\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let bodies = doc.one_line_bodies();
        assert_eq!(
            bodies.len(),
            1,
            "the wrapped parameter list stays open: {bodies:?}"
        );
        let b = &bodies[0];
        assert_eq!((b.start_line, b.end_line), (1, 3));
        let folded = "    int totalCents() { return qty * unitPriceCents; }";
        assert_eq!(b.folded_width, folded.chars().count());
        assert!(b.folded_width <= 120, "within the default right margin");
        assert!(
            idecraft_core::IdeSettings::default()
                .editor
                .fold_one_line_methods
        );
    }

    /// SPEC-0027 AK-16 (v13): in the languages whose bundled query names every identifier (or
    /// scalar) first, declared and called functions, YAML keys, C constants and Lua and R
    /// parameters get their roles; the rest keeps the bundled query's kinds.
    #[test]
    fn roles_before_the_bundled_queries() {
        let roles = |language: LanguageId, text: &str| -> Vec<(String, String)> {
            let doc = SyntaxDocument::new(language, false, text).unwrap();
            let chars: Vec<char> = text.chars().collect();
            doc.highlight()
                .iter()
                .filter(|s| {
                    s.kind.starts_with("function")
                        || s.kind.starts_with("property")
                        || s.kind.starts_with("constant")
                        || s.kind == "variable.parameter"
                        || s.kind == "keyword"
                            && chars[s.start].is_lowercase()
                            && s.end - s.start > 6
                })
                .map(|s| (chars[s.start..s.end].iter().collect(), s.kind.clone()))
                .collect()
        };
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(t, k)| ((*t).to_owned(), (*k).to_owned()))
                .collect()
        };
        assert_eq!(
            roles(
                LanguageId::C,
                "#define SQ(x) ((x)*(x))\nstatic int greet(int n) { return n + MAX; }\nint main(void) { printf(\"%d\", greet(1)); ops.run(); p->stop(); }\n"
            ),
            pairs(&[
                ("SQ", "function.macro"),
                ("greet", "function"),
                ("MAX", "constant"),
                ("main", "function"),
                ("printf", "function.call"),
                ("greet", "function.call"),
                ("run", "function.call"),
                ("stop", "function.call"),
            ])
        );
        let cpp = roles(
            LanguageId::Cpp,
            "class G { public: int greet(int n) { return n; } };\nint G::other() { return std::max(1, 2); }\nint main() { G g; g.greet(1); }\n",
        );
        assert_eq!(
            cpp,
            pairs(&[
                ("greet", "function"),
                ("other", "function"),
                ("max", "function.call"),
                ("main", "function"),
                ("greet", "function.call"),
            ]),
            "{cpp:?}"
        );
        assert_eq!(
            roles(
                LanguageId::Yaml,
                "name: shop\n\"quoted\": x\nitems:\n  - a\nmap: {k: v, 'q': w}\n"
            ),
            pairs(&[
                ("name", "property"),
                ("\"quoted\"", "property"),
                ("items", "property"),
                ("map", "property"),
                ("k", "property"),
                ("'q'", "property"),
            ])
        );
        assert_eq!(
            roles(
                LanguageId::Ruby,
                "class G\n  private\n  def greet(n)\n    n\n  end\n  def self.make; end\nend\ng.greet(1)\nputs \"a\"\n"
            ),
            pairs(&[
                ("private", "keyword"),
                ("greet", "function"),
                ("make", "function"),
                ("greet", "function.call"),
                ("puts", "function.call"),
            ])
        );
        assert_eq!(
            roles(
                LanguageId::Lua,
                "local function greet(n) return n end\nfunction M.run(a) end\nprint(greet(1))\nobj:stop()\n"
            ),
            pairs(&[
                ("greet", "function"),
                ("n", "variable.parameter"),
                ("run", "function"),
                ("a", "variable.parameter"),
                ("print", "function.call"),
                ("greet", "function.call"),
                ("stop", "function.call"),
            ])
        );
        assert_eq!(
            roles(
                LanguageId::Swift,
                "func greet(_ n: String) -> String { return n }\nprint(greet(\"a\"))\nlist.append(1)\n"
            ),
            pairs(&[
                ("greet", "function"),
                ("print", "function.call"),
                ("greet", "function.call"),
                ("append", "function.call"),
            ])
        );
        assert_eq!(
            roles(
                LanguageId::R,
                "greet <- function(n) paste(\"Hi\", n)\nshout = function(x) x\nprint(greet(\"a\"))\n"
            ),
            pairs(&[
                ("greet", "function"),
                ("n", "variable.parameter"),
                ("paste", "function.call"),
                ("shout", "function"),
                ("x", "variable.parameter"),
                ("print", "function.call"),
                ("greet", "function.call"),
            ])
        );
    }

    /// SPEC-0027 AK-10 (v8): Kotlin names take the role of what they are — functions declared and
    /// called, properties declared and read through a receiver; parameters and local variables
    /// keep the text colour.
    #[test]
    fn kotlin_colour_roles() {
        let text = "package zoo\n\nclass Cat(val name: String, var age: Int, plain: Int) : Animal(plain) {\n    val loud: Boolean = true\n    init { val seen = 1 }\n    fun describe(times: Int): String {\n        val text = name.length + sound().length\n        listOf(1).forEach { val inner = it; println(inner) }\n        return this.owner.name + Shape.Circle(2.0).radius + Math.PI\n    }\n}\nval top = 2\nfun Cat.twice() = top\n";
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, text).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let spans = doc.highlight();
        let roles: Vec<(String, &str)> = spans
            .iter()
            .filter(|s| !matches!(s.kind.as_str(), "keyword" | "type" | "number" | "string"))
            .map(|s| (chars[s.start..s.end].iter().collect(), s.kind.as_str()))
            .collect();
        let expected = [
            ("name", "property"),
            ("age", "property"),
            ("loud", "property"),
            ("describe", "function"),
            ("length", "property"),
            ("sound", "function.call"),
            ("length", "property"),
            ("listOf", "function.call"),
            ("forEach", "function.call"),
            ("println", "function.call"),
            ("owner", "property"),
            ("name", "property"),
            ("Circle", "function.call"),
            ("radius", "property"),
            ("top", "property"),
            ("twice", "function"),
        ];
        let expected: Vec<(String, &str)> = expected
            .iter()
            .map(|(t, k)| ((*t).to_owned(), *k))
            .collect();
        assert_eq!(roles, expected);
        // What the tree knows better than the server's token (SPEC-0290 v3 behaviour 3a).
        let at = |needle: &str| text[..text.find(needle).unwrap()].chars().count();
        assert_eq!(
            doc.kotlin_declared(at("name: String")),
            Some(KotlinDeclared::ConstructorProperty)
        );
        assert_eq!(doc.kotlin_declared(at("plain: Int")), None);
        assert_eq!(doc.kotlin_declared(at("loud:")), None);
        assert_eq!(
            doc.kotlin_declared(at("seen =")),
            Some(KotlinDeclared::Local)
        );
        assert_eq!(
            doc.kotlin_declared(at("text =")),
            Some(KotlinDeclared::Local)
        );
        assert_eq!(
            doc.kotlin_declared(at("inner =")),
            Some(KotlinDeclared::Local)
        );
        assert_eq!(doc.kotlin_declared(at("top =")), None);
        assert_eq!(doc.kotlin_declared(at("describe(")), None);
    }

    /// The roles of `kind` in the highlighting of `text`: (text of the span, kind).
    fn spans_of(language: LanguageId, text: &str, kinds: &[&str]) -> Vec<(String, String)> {
        let doc = SyntaxDocument::new(language, false, text).unwrap();
        let chars: Vec<char> = text.chars().collect();
        doc.highlight()
            .into_iter()
            .filter(|s| kinds.iter().any(|k| s.kind == *k))
            .map(|s| (chars[s.start..s.end].iter().collect(), s.kind))
            .collect()
    }

    /// SPEC-0027 AK-12 (v10): Java fields by syntax — declared, accessed after a dot and used by
    /// name where no parameter or local hides them; static fields, interface fields and enum
    /// constants as static fields; record components as fields.
    #[test]
    fn java_field_roles() {
        let text = "class A {\n  private int count = 0;\n  static String NAME = \"a\";\n  int get(int x) { int y = count; return count + this.count + NAME.length() + x + y; }\n  void set(int count) { this.count = count; }\n  void each() { for (int i : list) { use(i, total); } }\n  int total;\n}\nenum E { RED, GREEN; static E f() { return RED; } }\ninterface I { int MAX = 1; }\nrecord R(int a) { int twice() { return a * 2 + p.x + Color.BLUE; } }\n";
        let got = spans_of(LanguageId::Java, text, &["property", "property.static"]);
        let expected = [
            ("count", "property"),
            ("NAME", "property.static"),
            ("count", "property"),
            ("count", "property"),
            ("count", "property"),
            ("NAME", "property.static"),
            // `set(int count)`: the parameter hides the field; `this.count` is the field.
            ("count", "property"),
            ("total", "property"),
            ("total", "property"),
            ("RED", "property.static"),
            ("GREEN", "property.static"),
            ("RED", "property.static"),
            ("MAX", "property.static"),
            ("a", "property"),
            ("a", "property"),
            ("x", "property"),
            ("BLUE", "property.static"),
        ];
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|(t, k)| ((*t).to_owned(), (*k).to_owned()))
            .collect();
        assert_eq!(got, expected);
        // Parameters, locals, methods and loop variables keep their colour.
        let plain = spans_of(LanguageId::Java, text, &["variable"]);
        for name in ["x", "y", "i", "get", "set", "use", "length"] {
            assert!(
                plain.iter().any(|(t, _)| t == name),
                "`{name}` stays a variable: {plain:?}"
            );
        }
    }

    /// SPEC-0027 AK-14 (v11): reassigned locals and parameters at their declarations and uses;
    /// branches of one `if`, a local class's field and fields stay quiet.
    #[test]
    fn java_reassigned_roles() {
        let text = "class A {
  int count;
  void add(String name, int p) {
    for (int i = 0; i < 2; i++) { use(name + i); }
    p = p * 2;
    int a = 1;
    a += 2;
    int b;
    if (p > 0) { b = 1; } else { b = 2; }
    int c;
    c = 1;
    c = 2;
    int d;
    while (true) { d = 3; }
    int e = 5;
    Runnable r = new Runnable() { int e; public void run() { e = 1; this.e++; } };
    count++;
    this.count = e + b + c + d;
  }
}
";
        let got = spans_of(
            LanguageId::Java,
            text,
            &["variable.reassigned", "variable.parameter.reassigned"],
        );
        let at = |(t, k): &(String, String)| format!("{t} {k}");
        let got: Vec<String> = got.iter().map(at).collect();
        let l = |n: &str| format!("{n} variable.reassigned");
        let p = |n: &str| format!("{n} variable.parameter.reassigned");
        let mut expected = vec![p("p")];
        expected.extend(["i", "i", "i", "i"].map(l));
        expected.extend([p("p"), p("p")]);
        expected.extend(["a", "a"].map(l));
        expected.push(p("p"));
        expected.extend(["c", "c", "c"].map(l));
        expected.extend(["d", "d"].map(l));
        expected.extend(["c", "d"].map(l));
        assert_eq!(got, expected);
    }

    /// SPEC-0027 AK-13 (v10): `/** … */` is a doc comment in Java, Kotlin, JavaScript, TypeScript,
    /// Groovy, Scala and PHP, and `///`, `//!`, `/** */`, `/*! */` in Rust; other comments, `/**/`
    /// and `////` stay comments, and C keeps plain comments.
    #[test]
    fn doc_comments_have_their_own_role() {
        let kinds = ["comment", "comment.documentation"];
        type Case = (
            LanguageId,
            &'static str,
            &'static [(&'static str, &'static str)],
        );
        let cases: &[Case] = &[
            (
                LanguageId::Java,
                "/** Doc. */\nclass A {\n  /**/\n  // line\n  /* block */\n  /** f */ int f;\n}\n",
                &[
                    ("/** Doc. */", "comment.documentation"),
                    ("/**/", "comment"),
                    ("// line", "comment"),
                    ("/* block */", "comment"),
                    ("/** f */", "comment.documentation"),
                ],
            ),
            (
                LanguageId::Kotlin,
                "/** KDoc */\nclass A\n// line\n",
                &[
                    ("/** KDoc */", "comment.documentation"),
                    ("// line", "comment"),
                ],
            ),
            (
                LanguageId::JavaScript,
                "/** JSDoc */\nfunction f() {}\n/* block */\n",
                &[
                    ("/** JSDoc */", "comment.documentation"),
                    ("/* block */", "comment"),
                ],
            ),
            (
                LanguageId::TypeScript,
                "/** TSDoc */\nclass A {}\n",
                &[("/** TSDoc */", "comment.documentation")],
            ),
            (
                LanguageId::Groovy,
                "/** GroovyDoc */\nclass A {}\n",
                &[("/** GroovyDoc */", "comment.documentation")],
            ),
            (
                LanguageId::Scala,
                "/** ScalaDoc */\nclass A\n",
                &[("/** ScalaDoc */", "comment.documentation")],
            ),
            (
                LanguageId::Php,
                "<?php\n/** PHPDoc */\nclass A {}\n",
                &[("/** PHPDoc */", "comment.documentation")],
            ),
            (
                LanguageId::Rust,
                "/// doc\nfn f() {}\n//! inner\n//// four\n// line\n/*! block */\nfn g() {}\n",
                &[
                    // The grammar's doc comment nodes end after the line break.
                    ("/// doc\n", "comment.documentation"),
                    ("//! inner\n", "comment.documentation"),
                    ("//// four", "comment"),
                    ("// line", "comment"),
                    ("/*! block */", "comment.documentation"),
                ],
            ),
            (
                LanguageId::C,
                "/** doc */\nint x;\n",
                &[("/** doc */", "comment")],
            ),
        ];
        for (language, text, expected) in cases {
            let got = spans_of(*language, text, &kinds);
            let expected: Vec<(String, String)> = expected
                .iter()
                .map(|(t, k)| ((*t).to_owned(), (*k).to_owned()))
                .collect();
            assert_eq!(got, expected, "{language:?}");
        }
    }

    /// SPEC-0083 AK-8 (v4): the import block of a Java or Kotlin file spans its groups; one
    /// import, or another language, has none. Kotlin imports are a folding region (SPEC-0030).
    #[test]
    fn java_and_kotlin_import_blocks() {
        let block = |language, text: &str| {
            SyntaxDocument::new(language, false, text)
                .unwrap()
                .import_block()
        };
        let java = "package a;\n\nimport b.C;\nimport b.D;\n\n// the JDK\nimport java.util.List;\nimport static java.util.Map.entry;\n\nclass A {}\n";
        assert_eq!(block(LanguageId::Java, java), Some((2, 7)));
        assert_eq!(block(LanguageId::Java, "import b.C;\n\nclass A {}\n"), None);
        assert_eq!(block(LanguageId::Java, "class A {}\n"), None);
        assert_eq!(
            block(LanguageId::Java, "import b.C; import b.D;\nclass A {}\n"),
            None
        );
        let kotlin =
            "package a\n\nimport b.C\nimport b.D as E\n\nimport java.util.List\n\nfun main() {}\n";
        assert_eq!(block(LanguageId::Kotlin, kotlin), Some((2, 5)));
        assert_eq!(
            block(LanguageId::Kotlin, "import b.C\n\nfun main() {}\n"),
            None
        );
        assert_eq!(block(LanguageId::Rust, "use a::b;\nuse c::d;\n"), None);
        // The folding regions give the whole block (SPEC-0083 v8).
        assert_eq!(
            folds(LanguageId::Kotlin, kotlin),
            [(2, 5, "imports".to_owned())]
        );
        // The keyword `import` of other grammars is no import statement of its own.
        assert_eq!(
            folds(LanguageId::Python, "import a\nimport b\n\nx = 1\n"),
            [(0, 1, "imports".to_owned())]
        );
    }

    #[test]
    fn rust_folding() {
        let text = "use a::b;\nuse c::d;\n\n/* one\n   two */\nstruct S {\n    x: i32,\n}\n\nfn f() {\n    let v = [\n        1,\n    ];\n}\nfn g() { 1 }\n";
        assert_eq!(
            folds(LanguageId::Rust, text),
            [
                (0, 1, "imports".into()),
                (3, 4, "comment".into()),
                (5, 7, "block".into()),
                (9, 13, "block".into()),
                (10, 12, "block".into()),
            ]
        );
    }

    #[test]
    fn cpp_folding_and_tags() {
        let text = "struct Square {\n    int side = 2;\n    int area() const {\n        return side;\n    }\n};\n\nint main() {\n    return 0;\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Cpp, false, text).unwrap();
        let regions: Vec<(usize, usize)> = doc
            .folding()
            .iter()
            .map(|r| (r.start_line, r.end_line))
            .collect();
        assert!(regions.contains(&(0, 5)), "{regions:?}");
        assert!(regions.contains(&(2, 4)), "{regions:?}");
        assert!(regions.contains(&(7, 9)), "{regions:?}");
        let names: Vec<String> = doc.definitions().into_iter().map(|d| d.name).collect();
        assert!(
            names.contains(&"Square".to_owned()) && names.contains(&"main".to_owned()),
            "{names:?}"
        );
    }

    #[test]
    fn python_json_html_folding() {
        let py = "import os\nimport sys\n# a\n# b\nclass A:\n    def f(self):\n        return 1\n";
        assert_eq!(
            folds(LanguageId::Python, py),
            [
                (0, 1, "imports".into()),
                (2, 3, "comment".into()),
                (4, 6, "block".into()),
                (5, 6, "block".into()),
            ]
        );
        let json = "{\n  \"a\": [\n    1\n  ]\n}";
        assert_eq!(
            folds(LanguageId::Json, json),
            [(0, 4, "block".into()), (1, 3, "block".into())]
        );
        let html = "<ul>\n  <li>x</li>\n</ul>";
        assert_eq!(folds(LanguageId::Html, html), [(0, 2, "block".into())]);
    }

    #[test]
    fn folding_after_edits_matches_a_fresh_document() {
        let mut doc = SyntaxDocument::new(LanguageId::Rust, false, "fn f() {}\n").unwrap();
        doc.edit(8..8, "\n    let x = 1;\n").unwrap();
        let fresh = SyntaxDocument::new(LanguageId::Rust, false, doc.text()).unwrap();
        assert_eq!(doc.folding(), fresh.folding());
        assert_eq!(doc.folding().len(), 1);
    }

    #[test]
    fn broken_code_still_highlights() {
        let text = "fn broken( { let\nfn ok() { \"s\" }";
        let spans = SyntaxDocument::new(LanguageId::Rust, false, text)
            .unwrap()
            .highlight();
        let kinds = kinds_of(text, &spans);
        assert!(
            kinds.iter().any(|(t, k)| *t == "\"s\"" && *k == "string"),
            "{kinds:?}"
        );
        let partial = SyntaxDocument::new(LanguageId::Rust, false, text).unwrap();
        let ranged = partial.highlight_range(17..text.chars().count());
        assert!(ranged.iter().all(|s| s.end > 17));
    }

    const SHOP: &str = "package shop;\n\npublic class Cart {\n    Cart() {\n        reset();\n    }\n\n    int total(int[] xs) {\n        int sum = 0;\n        for (int x : xs) {\n            if (x > 0) { sum += x; }\n            switch (x) {\n                case 1:\n                    break;\n            }\n        }\n        Runnable r = () -> {\n            sum();\n        };\n        return sum;\n    }\n}\n";

    /// SPEC-0083 v7 AK-16: the code blocks around an offset, innermost first.
    #[test]
    fn java_code_blocks_around() {
        let doc = SyntaxDocument::new(LanguageId::Java, false, SHOP).expect("grammar");
        let at = |marker: &str| SHOP[..SHOP.find(marker).expect("marker")].chars().count();
        // In the loop: the loop's block, then the method's; the class body is none.
        assert_eq!(doc.code_blocks_around(at("if (x")), [(9, 15), (7, 20)]);
        // In the one-line block of the `if`: it comes first, on one line.
        assert_eq!(
            doc.code_blocks_around(at("sum += x")),
            [(10, 10), (9, 15), (7, 20)]
        );
        // Behind its closing brace it still counts, before the `{` as well.
        assert_eq!(
            doc.code_blocks_around(at("sum += x; }") + "sum += x; }".len()),
            [(10, 10), (9, 15), (7, 20)]
        );
        assert_eq!(
            doc.code_blocks_around(at("{ sum += x")),
            [(10, 10), (9, 15), (7, 20)]
        );
        // The body of a `switch`, of a lambda and of a constructor.
        assert_eq!(
            doc.code_blocks_around(at("break")),
            [(11, 14), (9, 15), (7, 20)]
        );
        assert_eq!(doc.code_blocks_around(at("sum();")), [(16, 18), (7, 20)]);
        assert_eq!(doc.code_blocks_around(at("reset")), [(3, 5)]);
        // Between two members and in the package line: none.
        assert!(doc.code_blocks_around(at("int total")).is_empty());
        assert!(doc.code_blocks_around(0).is_empty());
        // Other languages have no code blocks here.
        let python = "def f():\n    if x:\n        return 1\n";
        let doc = SyntaxDocument::new(LanguageId::Python, false, python).expect("grammar");
        assert!(doc.code_blocks_around(20).is_empty());
        let rust = "fn f() {\n    let x = 1;\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Rust, false, rust).expect("grammar");
        assert!(doc.code_blocks_around(12).is_empty());
    }

    /// SPEC-0057 v6 AK-22: the innermost declaration above the first visible line, then outwards.
    #[test]
    fn context_declaration_walks_outwards() {
        let doc = SyntaxDocument::new(LanguageId::Java, false, SHOP).expect("grammar");
        let around = doc.definitions_around(SHOP.find("sum += x").expect("marker"));
        let lines: Vec<usize> = around.iter().map(|d| d.line).collect();
        assert_eq!(lines, [2, 7], "the class, then the method");
        let shown = |first_visible: usize, shown: Option<usize>| {
            context_declaration(&around, first_visible, shown).map(|d| (d.name.as_str(), d.line))
        };
        // The method's line is scrolled out of view: it is the one to show.
        assert_eq!(shown(9, None), Some(("total", 7)));
        // Used again: the class; once more: nothing is left.
        assert_eq!(shown(9, Some(7)), Some(("Cart", 2)));
        assert_eq!(shown(9, Some(2)), None);
        // The method's line is the first visible one: the class is shown at once.
        assert_eq!(shown(7, None), Some(("Cart", 2)));
        assert_eq!(shown(7, Some(2)), None);
        // Everything in view: nothing.
        assert_eq!(shown(2, None), None);
        assert_eq!(shown(0, None), None);
        // A line that is no declaration of these starts afresh.
        assert_eq!(shown(9, Some(4)), Some(("total", 7)));
        // No declarations around the caret.
        assert_eq!(context_declaration(&[], 9, None), None);
    }

    /// `SPEC-0031` v6 AK-13: Java records and enums are definitions, nested ones too.
    #[test]
    fn java_records_and_enums_are_definitions() {
        let text = "package lab;\n\npublic final class Inventory {\n    public record Item(String sku, int qty) {\n        int total() { return qty; }\n    }\n}\n\nenum Unit { PIECE, BOX }\n\nrecord Pair(int a, int b) {}\n";
        let defs: Vec<(String, String, usize, usize)> =
            SyntaxDocument::new(LanguageId::Java, false, text)
                .unwrap()
                .definitions()
                .into_iter()
                .map(|d| (d.name, d.kind, d.line, d.character))
                .collect();
        for expected in [
            ("Inventory", "class", 2, 19),
            ("Item", "class", 3, 18),
            ("total", "method", 4, 12),
            ("Unit", "enum", 8, 5),
            ("Pair", "class", 10, 7),
        ] {
            assert!(
                defs.iter()
                    .any(|(n, k, l, c)| (n.as_str(), k.as_str(), *l, *c) == expected),
                "{expected:?} in {defs:?}"
            );
        }
        // Each declaration once, in document order.
        let lines: Vec<usize> = defs.iter().map(|d| d.2).collect();
        let mut sorted = lines.clone();
        sorted.sort_unstable();
        assert_eq!(lines, sorted);
        assert_eq!(defs.iter().filter(|d| d.0 == "Item").count(), 1, "{defs:?}");
    }
}
