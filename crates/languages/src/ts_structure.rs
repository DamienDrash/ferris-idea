//! Classes and interfaces of JavaScript and TypeScript (`SPEC-0320`): their syntax, and through the
//! language server their type hierarchy, usage counts and gutter marks. typescript-language-server
//! has no type hierarchy provider, so the syntax tree says where `extends`/`implements` stand and
//! the server says which declaration each name means.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use tree_sitter::Node;

use crate::LanguageId;
use crate::insight::{CallNode, Location, Opened, Range, parse_locations};
use crate::java_structure::TypeDirection;
use crate::lsp::{LspError, file_uri};
use crate::syntax::SyntaxDocument;
use crate::usages::UsageCount;

/// At most this many nodes in a hierarchy (behaviour 4).
const NODE_LIMIT: usize = 500;
/// At most this many declarations counted per file (behaviour 5).
const MAX_DECLARATIONS: usize = 200;

/// Whether `path` is a JavaScript or TypeScript file.
pub fn is_script(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs")
    )
}

/// Whether `path` is a TypeScript file.
pub fn is_typescript(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

/// A name in an `extends` or `implements` clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Heritage {
    /// The name (the last part of `ns.Base`).
    pub name: String,
    /// `ns` of `ns.Base` when it is a plain name (`SPEC-0320` v5 behaviour 21: a namespace import).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    /// 0-based line of the name.
    pub line: u64,
    /// Character of the name in its line.
    pub character: u64,
    /// Whether it stands after `extends`.
    pub extends: bool,
}

/// What a member is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberKind {
    /// A method with or without a body.
    Method,
    /// A field or an interface property.
    Field,
    /// `get x()`.
    Getter,
    /// `set x(v)`.
    Setter,
    /// The constructor.
    Constructor,
}

/// One member of a class or interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The name.
    pub name: String,
    /// What it is.
    pub kind: MemberKind,
    /// `static`.
    pub is_static: bool,
    /// `abstract`.
    pub is_abstract: bool,
    /// `public`, `protected`, `private`; `None` when not written.
    pub visibility: Option<String>,
    /// 0-based line of the name.
    pub line: u64,
    /// Character of the name in its line.
    pub character: u64,
    /// Bytes of the whole member, with doc comment and decorators.
    pub start: usize,
    /// End of the member, with its `;`.
    pub end: usize,
    /// Byte of a written `private`.
    pub private_at: Option<usize>,
    /// Interface signature when everything is annotated; `None` when the server must help.
    pub signature: Option<String>,
    /// `readonly`.
    pub readonly: bool,
    /// Names used as `this.x`.
    pub this_uses: Vec<String>,
    /// Every identifier in the member.
    pub identifiers: Vec<String>,
    /// Whether the member uses `super`.
    pub uses_super: bool,
    /// Byte after `{` of a constructor body.
    pub body_open: Option<usize>,
    /// Names of parameter properties (constructors).
    pub parameter_properties: Vec<String>,
}

impl Member {
    /// Whether the member takes part in overriding (behaviour 8).
    pub fn overridable(&self) -> bool {
        !self.is_static
            && self.kind != MemberKind::Constructor
            && self.visibility.as_deref() != Some("private")
            && !self.name.starts_with('#')
    }

    /// Whether an extract or pull can move it (behaviour 14).
    pub fn movable(&self) -> bool {
        self.kind != MemberKind::Constructor
    }

    /// Whether an interface can declare it (behaviour 12).
    pub fn public_instance(&self) -> bool {
        !self.is_static
            && self.kind != MemberKind::Constructor
            && matches!(self.visibility.as_deref(), None | Some("public"))
            && !self.name.starts_with('#')
    }
}

/// A class or interface declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    /// The name.
    pub name: String,
    /// Interface rather than class.
    pub is_interface: bool,
    /// `abstract class`.
    pub is_abstract: bool,
    /// Under `export`.
    pub exported: bool,
    /// 0-based line of the name.
    pub line: u64,
    /// Character of the name in its line.
    pub character: u64,
    /// Bytes of the declaration, `export` and decorators included.
    pub start: usize,
    /// End of the declaration.
    pub end: usize,
    /// End of the name and its type parameters.
    pub after_name: usize,
    /// Bytes of the `extends` clause of a class, or the type list after `extends` of an interface.
    pub extends_clause: Option<(usize, usize)>,
    /// End of the `implements` clause.
    pub implements_end: Option<usize>,
    /// Byte of the `{` of the body.
    pub body_start: usize,
    /// Byte of the closing `}` of the body.
    pub body_end: usize,
    /// Its names in `extends` and `implements`.
    pub heritage: Vec<Heritage>,
    /// Its members in document order.
    pub members: Vec<Member>,
    /// Names of its type parameters.
    pub type_parameters: Vec<String>,
}

impl TypeDecl {
    /// The member named `name`.
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name)
    }

    /// The name's range.
    fn name_range(&self) -> Range {
        name_range(&self.name, self.line, self.character)
    }

    /// `class` or `interface`.
    pub fn kind_name(&self) -> &'static str {
        if self.is_interface {
            "interface"
        } else {
            "class"
        }
    }
}

fn name_range(name: &str, line: u64, character: u64) -> Range {
    Range {
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character + name.chars().count() as u64,
    }
}

/// A parsed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The text.
    pub text: String,
    /// Its classes and interfaces in document order.
    pub types: Vec<TypeDecl>,
    /// Names bound by its imports.
    pub imports: Vec<String>,
    /// Names declared at file level.
    pub top_level: Vec<String>,
    /// What its imports bind, with their modules (`SPEC-0320` v5 behaviour 21).
    pub bindings: Vec<Binding>,
    /// Its `export … from` statements.
    pub reexports: Vec<ReExport>,
    /// Its `export { a as b }` without a module: exported name and local name.
    pub local_exports: Vec<(String, String)>,
    /// The name its `export default` gives, when it names a declaration or a name.
    pub default_export: Option<String>,
}

/// What an import binds (`SPEC-0320` v5 behaviour 21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Imported {
    /// `import X from`.
    Default,
    /// `import { X }` or `import { X as Y }`: the exported name.
    Named(String),
    /// `import * as ns from`.
    Namespace,
}

/// One name an import binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The name in this file.
    pub local: String,
    /// What it is in the module.
    pub imported: Imported,
    /// The module as written (`./shapes`, `react`).
    pub module: String,
}

/// An `export … from` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReExport {
    /// The module as written.
    pub module: String,
    /// Exported and original names; `None` for `export * from`.
    pub names: Option<Vec<(String, String)>>,
}

impl Parsed {
    /// The innermost type at a place (the name, or anywhere inside).
    pub fn type_at(&self, line: u64, character: u64) -> Option<usize> {
        let offset = offset_of(&self.text, line, character);
        self.types
            .iter()
            .enumerate()
            .filter(|(_, t)| t.start <= offset && offset < t.end)
            .min_by_key(|(_, t)| t.end - t.start)
            .map(|(i, _)| i)
    }

    /// The type whose name is at a place.
    fn type_named_at(&self, line: u64, character: u64) -> Option<usize> {
        self.types.iter().position(|t| {
            t.line == line
                && character >= t.character
                && character <= t.character + t.name.chars().count() as u64
        })
    }

    /// The type with a heritage name at a place.
    fn heritage_owner(&self, line: u64, character: u64) -> Option<usize> {
        self.types.iter().position(|t| {
            t.heritage
                .iter()
                .any(|h| h.line == line && h.character == character)
        })
    }
}

/// Byte offset of a 0-based line and character (Unicode scalar values).
pub fn offset_of(text: &str, line: u64, character: u64) -> usize {
    let mut at = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i as u64 == line {
            return at
                + l.char_indices()
                    .nth(character as usize)
                    .map_or(l.trim_end_matches('\n').len(), |(b, _)| b);
        }
        at += l.len();
    }
    text.len()
}

/// 0-based line and character of byte `offset`.
pub fn place_of(text: &str, offset: usize) -> (u64, u64) {
    let before = &text[..offset];
    let line = before.matches('\n').count() as u64;
    let start = before.rfind('\n').map_or(0, |n| n + 1);
    (line, before[start..].chars().count() as u64)
}

fn place(text: &str, node: Node) -> (u64, u64) {
    place_of(text, node.start_byte())
}

fn slice<'a>(text: &'a str, node: Node) -> &'a str {
    &text[node.byte_range()]
}

fn visit<'a>(node: Node<'a>, f: &mut dyn FnMut(Node<'a>)) {
    f(node);
    let mut c = node.walk();
    for child in node.children(&mut c) {
        visit(child, f);
    }
}

fn has_child(node: Node, kind: &str) -> bool {
    let mut c = node.walk();
    node.children(&mut c).any(|n| n.kind() == kind)
}

/// Parses a script file; `None` when the file is no script.
pub fn parse_file(path: &Path, text: &str) -> Option<Parsed> {
    if !is_script(path) {
        return None;
    }
    let language = if is_typescript(path) {
        LanguageId::TypeScript
    } else {
        LanguageId::JavaScript
    };
    let tsx = path.extension().is_some_and(|e| e == "tsx");
    let doc = SyntaxDocument::new(language, tsx, text).ok()?;
    let root = doc.tree().root_node();
    let mut parsed = Parsed {
        text: text.to_owned(),
        types: Vec::new(),
        imports: Vec::new(),
        top_level: Vec::new(),
        bindings: Vec::new(),
        reexports: Vec::new(),
        local_exports: Vec::new(),
        default_export: None,
    };
    let mut nodes = Vec::new();
    visit(root, &mut |n| {
        if matches!(
            n.kind(),
            "class_declaration" | "abstract_class_declaration" | "interface_declaration"
        ) {
            nodes.push(n);
        }
        if n.kind() == "import_statement" {
            visit(n, &mut |i| {
                if matches!(i.kind(), "identifier") {
                    parsed.imports.push(slice(text, i).to_owned());
                }
            });
        }
    });
    let mut c = root.walk();
    for child in root.children(&mut c) {
        read_module_links(child, text, &mut parsed);
        let declaration = if child.kind() == "export_statement" {
            child.child_by_field_name("declaration").unwrap_or(child)
        } else {
            child
        };
        if let Some(name) = declaration.child_by_field_name("name") {
            parsed.top_level.push(slice(text, name).to_owned());
        }
        if matches!(
            declaration.kind(),
            "lexical_declaration" | "variable_declaration"
        ) {
            let mut w = declaration.walk();
            for d in declaration.children(&mut w) {
                if let Some(name) = d.child_by_field_name("name") {
                    parsed.top_level.push(slice(text, name).to_owned());
                }
            }
        }
    }
    parsed.top_level.extend(parsed.imports.iter().cloned());
    for n in nodes {
        if let Some(t) = type_decl(n, text) {
            parsed.types.push(t);
        }
    }
    Some(parsed)
}

/// The text of a string literal without its quotes.
fn string_value(node: Node, text: &str) -> String {
    slice(text, node)
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_owned()
}

/// Imports, re-exports and the default export of a top-level statement (`SPEC-0320` v5
/// behaviour 21).
fn read_module_links(node: Node, text: &str, parsed: &mut Parsed) {
    match node.kind() {
        "import_statement" => {
            let Some(source) = node.child_by_field_name("source") else {
                return;
            };
            let module = string_value(source, text);
            let mut c = node.walk();
            for clause in node
                .named_children(&mut c)
                .filter(|n| n.kind() == "import_clause")
            {
                let mut w = clause.walk();
                for part in clause.named_children(&mut w) {
                    match part.kind() {
                        "identifier" => parsed.bindings.push(Binding {
                            local: slice(text, part).to_owned(),
                            imported: Imported::Default,
                            module: module.clone(),
                        }),
                        "namespace_import" => {
                            let mut n = part.walk();
                            if let Some(id) = part
                                .named_children(&mut n)
                                .find(|i| i.kind() == "identifier")
                            {
                                parsed.bindings.push(Binding {
                                    local: slice(text, id).to_owned(),
                                    imported: Imported::Namespace,
                                    module: module.clone(),
                                });
                            }
                        }
                        "named_imports" => {
                            let mut n = part.walk();
                            for spec in part
                                .named_children(&mut n)
                                .filter(|s| s.kind() == "import_specifier")
                            {
                                let Some(name) = spec.child_by_field_name("name") else {
                                    continue;
                                };
                                let local = spec.child_by_field_name("alias").unwrap_or(name);
                                parsed.bindings.push(Binding {
                                    local: slice(text, local).to_owned(),
                                    imported: Imported::Named(string_value(name, text)),
                                    module: module.clone(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "export_statement" => {
            let mut names = Vec::new();
            let mut c = node.walk();
            let clause = node
                .named_children(&mut c)
                .find(|n| n.kind() == "export_clause");
            if let Some(clause) = clause {
                let mut w = clause.walk();
                for spec in clause
                    .named_children(&mut w)
                    .filter(|s| s.kind() == "export_specifier")
                {
                    let Some(name) = spec.child_by_field_name("name") else {
                        continue;
                    };
                    let exported = spec.child_by_field_name("alias").unwrap_or(name);
                    names.push((string_value(exported, text), string_value(name, text)));
                }
            }
            if let Some(source) = node.child_by_field_name("source") {
                let mut c = node.walk();
                let namespace = node
                    .named_children(&mut c)
                    .any(|n| n.kind() == "namespace_export");
                if !namespace {
                    parsed.reexports.push(ReExport {
                        module: string_value(source, text),
                        names: clause.is_some().then_some(names),
                    });
                }
                return;
            }
            parsed.local_exports.extend(names);
            let mut c = node.walk();
            let is_default = node.children(&mut c).any(|n| n.kind() == "default");
            if is_default {
                let named = node
                    .child_by_field_name("declaration")
                    .and_then(|d| d.child_by_field_name("name"))
                    .or_else(|| {
                        node.child_by_field_name("value")
                            .filter(|v| v.kind() == "identifier")
                    });
                parsed.default_export = named.map(|n| slice(text, n).to_owned());
            }
        }
        _ => {}
    }
}

fn type_decl(node: Node, text: &str) -> Option<TypeDecl> {
    let name = node.child_by_field_name("name")?;
    let body = node.child_by_field_name("body")?;
    let is_interface = node.kind() == "interface_declaration";
    let mut start_node = node;
    let exported = node
        .parent()
        .is_some_and(|p| p.kind() == "export_statement");
    if exported {
        start_node = node.parent()?;
    }
    let (line, character) = place(text, name);
    let mut type_parameters = Vec::new();
    let tp = node.child_by_field_name("type_parameters");
    if let Some(tp) = tp {
        let mut c = tp.walk();
        for p in tp.named_children(&mut c) {
            if let Some(n) = p.child_by_field_name("name") {
                type_parameters.push(slice(text, n).to_owned());
            }
        }
    }
    let mut t = TypeDecl {
        name: slice(text, name).to_owned(),
        is_interface,
        is_abstract: node.kind() == "abstract_class_declaration",
        exported,
        line,
        character,
        start: start_node.start_byte(),
        end: start_node.end_byte(),
        after_name: tp.map_or(name.end_byte(), |p| p.end_byte()),
        extends_clause: None,
        implements_end: None,
        body_start: body.start_byte(),
        body_end: body.end_byte().saturating_sub(1),
        heritage: Vec::new(),
        members: Vec::new(),
        type_parameters,
    };
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "class_heritage" => {
                let mut w = child.walk();
                for part in child.children(&mut w) {
                    match part.kind() {
                        "extends_clause" => {
                            t.extends_clause = Some((part.start_byte(), part.end_byte()));
                            if let Some(v) = part.child_by_field_name("value") {
                                add_heritage(&mut t.heritage, v, text, true);
                            }
                        }
                        "implements_clause" => {
                            t.implements_end = Some(part.end_byte());
                            let mut p = part.walk();
                            for ty in part.named_children(&mut p) {
                                add_heritage(&mut t.heritage, ty, text, false);
                            }
                        }
                        // JavaScript: `extends` then the expression.
                        "extends" => {}
                        _ if part.is_named() => {
                            t.extends_clause = Some((child.start_byte(), child.end_byte()));
                            add_heritage(&mut t.heritage, part, text, true);
                        }
                        _ => {}
                    }
                }
            }
            "extends_type_clause" => {
                t.extends_clause = Some((child.start_byte(), child.end_byte()));
                let mut p = child.walk();
                for ty in child.named_children(&mut p) {
                    add_heritage(&mut t.heritage, ty, text, true);
                }
            }
            _ => {}
        }
    }
    let mut w = body.walk();
    let children: Vec<Node> = body.children(&mut w).collect();
    for (i, child) in children.iter().enumerate() {
        if let Some(m) = member(*child, &children, i, text) {
            t.members.push(m);
        }
    }
    Some(t)
}

fn add_heritage(out: &mut Vec<Heritage>, node: Node, text: &str, extends: bool) {
    // `Base<T>`, `ns.Base`, `ns.Base<T>`: the name is the last identifier before the arguments.
    let target = match node.kind() {
        "generic_type" => node.child_by_field_name("name").unwrap_or(node),
        "call_expression" => return,
        _ => node,
    };
    let (leaf, qualifier) = match target.kind() {
        "nested_type_identifier" => (
            target.child_by_field_name("name").unwrap_or(target),
            target.child_by_field_name("module"),
        ),
        "member_expression" => (
            target.child_by_field_name("property").unwrap_or(target),
            target.child_by_field_name("object"),
        ),
        _ => (target, None),
    };
    let qualifier = qualifier
        .filter(|q| q.kind() == "identifier")
        .map(|q| slice(text, q).to_owned());
    if !matches!(
        leaf.kind(),
        "identifier" | "type_identifier" | "property_identifier"
    ) {
        return;
    }
    let (line, character) = place(text, leaf);
    out.push(Heritage {
        name: slice(text, leaf).to_owned(),
        qualifier,
        line,
        character,
        extends,
    });
}

fn member(node: Node, siblings: &[Node], at: usize, text: &str) -> Option<Member> {
    let is_field = matches!(
        node.kind(),
        "public_field_definition" | "field_definition" | "property_signature"
    );
    let is_method = matches!(
        node.kind(),
        "method_definition" | "abstract_method_signature" | "method_signature"
    );
    if !is_field && !is_method {
        return None;
    }
    let name = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("property"))?;
    if !matches!(
        name.kind(),
        "property_identifier" | "private_property_identifier"
    ) {
        return None;
    }
    let name_text = slice(text, name).to_owned();
    let mut is_static = false;
    let mut visibility = None;
    let mut private_at = None;
    let mut readonly = false;
    let mut getter = false;
    let mut setter = false;
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "static" => is_static = true,
            "readonly" => readonly = true,
            "get" => getter = true,
            "set" => setter = true,
            "accessibility_modifier" => {
                let word = slice(text, child).to_owned();
                if word == "private" {
                    private_at = Some(child.start_byte());
                }
                visibility = Some(word);
            }
            _ => {}
        }
    }
    let is_abstract = node.kind() == "abstract_method_signature" || has_child(node, "abstract");
    let kind = if is_field {
        MemberKind::Field
    } else if name_text == "constructor" {
        MemberKind::Constructor
    } else if getter {
        MemberKind::Getter
    } else if setter {
        MemberKind::Setter
    } else {
        MemberKind::Method
    };
    let (line, character) = place(text, name);
    let (start, end) = member_extent(node, siblings, at, text);
    let mut this_uses = Vec::new();
    let mut identifiers = Vec::new();
    let mut uses_super = false;
    visit(node, &mut |n| match n.kind() {
        "member_expression" => {
            if n.child_by_field_name("object")
                .is_some_and(|o| o.kind() == "this")
                && let Some(p) = n.child_by_field_name("property")
            {
                this_uses.push(slice(text, p).to_owned());
            }
        }
        "identifier" | "type_identifier" => identifiers.push(slice(text, n).to_owned()),
        "super" => uses_super = true,
        _ => {}
    });
    let parameters = node.child_by_field_name("parameters");
    let mut parameter_properties = Vec::new();
    if kind == MemberKind::Constructor
        && let Some(params) = parameters
    {
        let mut w = params.walk();
        for p in params.named_children(&mut w) {
            if (has_child(p, "accessibility_modifier") || has_child(p, "readonly"))
                && let Some(n) = p.child_by_field_name("pattern")
            {
                parameter_properties.push(slice(text, n).to_owned());
            }
        }
    }
    let body_open = (kind == MemberKind::Constructor)
        .then(|| node.child_by_field_name("body"))
        .flatten()
        .map(|b| b.start_byte() + 1);
    let optional = has_child(node, "?");
    let signature = signature_of(node, &name_text, kind, optional, readonly, parameters, text);
    Some(Member {
        name: name_text,
        kind,
        is_static,
        is_abstract,
        visibility,
        line,
        character,
        start,
        end,
        private_at,
        signature,
        readonly,
        this_uses,
        identifiers,
        uses_super,
        body_open,
        parameter_properties,
    })
}

/// The bytes of a member with its doc comment, its decorators and its `;`.
fn member_extent(node: Node, siblings: &[Node], at: usize, text: &str) -> (usize, usize) {
    let mut start = node.start_byte();
    let mut i = at;
    while i > 0 {
        let previous = siblings[i - 1];
        let adjacent = text[previous.end_byte()..start].matches('\n').count() <= 1;
        if adjacent && matches!(previous.kind(), "decorator" | "comment") {
            start = previous.start_byte();
            i -= 1;
        } else {
            break;
        }
    }
    let mut end = node.end_byte();
    if let Some(next) = siblings.get(at + 1)
        && next.kind() == ";"
    {
        end = next.end_byte();
    }
    (start, end)
}

/// The text of a parameter for an interface; `None` when its type is not written.
fn parameter_signature(p: Node, text: &str) -> Option<String> {
    if p.kind() == "this_type" {
        return None;
    }
    let pattern = p.child_by_field_name("pattern")?;
    let ty = p.child_by_field_name("type")?;
    let optional = p.kind() == "optional_parameter" || p.child_by_field_name("value").is_some();
    Some(format!(
        "{}{}{}",
        slice(text, pattern),
        if optional { "?" } else { "" },
        slice(text, ty)
    ))
}

/// The interface signature of a member when all its types are written (behaviour 12).
fn signature_of(
    node: Node,
    name: &str,
    kind: MemberKind,
    optional: bool,
    readonly: bool,
    parameters: Option<Node>,
    text: &str,
) -> Option<String> {
    let q = if optional { "?" } else { "" };
    match kind {
        MemberKind::Constructor => None,
        MemberKind::Field => {
            let ty = node.child_by_field_name("type")?;
            Some(format!(
                "{}{name}{q}{}",
                if readonly { "readonly " } else { "" },
                slice(text, ty)
            ))
        }
        MemberKind::Getter => {
            let ty = node.child_by_field_name("return_type")?;
            Some(format!("readonly {name}{}", slice(text, ty)))
        }
        MemberKind::Setter => {
            let params = parameters?;
            let mut c = params.walk();
            let first = params.named_children(&mut c).next()?;
            let ty = first.child_by_field_name("type")?;
            Some(format!("{name}{}", slice(text, ty)))
        }
        MemberKind::Method => {
            let params = parameters?;
            let mut parts = Vec::new();
            let mut c = params.walk();
            for p in params.named_children(&mut c) {
                parts.push(parameter_signature(p, text)?);
            }
            let ret = node.child_by_field_name("return_type")?;
            let tp = node
                .child_by_field_name("type_parameters")
                .map_or("", |t| slice(text, t));
            Some(format!(
                "{name}{q}{tp}({}){}",
                parts.join(", "),
                slice(text, ret)
            ))
        }
    }
}

/// The part of a server's hover about `name`: `name…` without the leading kind and class, on one
/// line (`(method) A.area(x: number): number` → `area(x: number): number`).
pub fn hover_signature(hover: &str, name: &str) -> Option<String> {
    let code: Vec<&str> = if hover.contains("```") {
        hover
            .lines()
            .skip_while(|l| !l.trim_start().starts_with("```"))
            .skip(1)
            .take_while(|l| !l.trim_start().starts_with("```"))
            .collect()
    } else {
        hover.lines().take_while(|l| !l.trim().is_empty()).collect()
    };
    let code = code.join(" ");
    let after_kind = code.rsplit_once(") ").map_or(code.as_str(), |(_, r)| r);
    let code_after_kind = if code.starts_with('(') {
        after_kind
    } else {
        &code
    };
    let mut from = code_after_kind;
    if let Some(i) = code_after_kind.find(&format!(".{name}")) {
        from = &code_after_kind[i + 1..];
    }
    from.starts_with(name).then(|| from.trim().to_owned())
}

/// The kinds of `SPEC-0320` behaviour 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkKind {
    /// Overrides a concrete member above.
    Overrides,
    /// Implements an abstract or interface member above.
    Implements,
    /// Overridden below.
    Overridden,
    /// Implemented below.
    Implemented,
    /// The type has subtypes.
    Inherited,
}

impl MarkKind {
    /// Whether the mark points upward.
    pub fn up(self) -> bool {
        matches!(self, Self::Overrides | Self::Implements)
    }
}

/// A place a mark leads to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarkTarget {
    /// The type holding it.
    pub name: String,
    /// Where it is.
    pub location: Location,
}

/// A gutter mark of a declaration (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclarationMark {
    /// 0-based line of the name.
    pub line: u64,
    /// Character of the name.
    pub character: u64,
    /// The declaration.
    pub name: String,
    /// What it means.
    pub kind: MarkKind,
    /// Where it leads.
    pub targets: Vec<MarkTarget>,
}

type TypeRef = (PathBuf, usize);

/// A supertype: in the project, or outside it (a leaf).
#[derive(Debug, Clone)]
enum Up {
    Project(TypeRef),
    Outside(String, Location),
}

/// The server and the files read for one question.
pub(crate) struct Session {
    /// The language server; `None` when the answers come from the syntax trees of the project
    /// alone (`SPEC-0320` v5 behaviour 21).
    o: Option<Opened>,
    /// The project's script files, read once when names are resolved from the syntax trees.
    files: Option<Vec<PathBuf>>,
    /// Supertypes and subtypes come from the syntax trees even with a server: no `tsconfig.json`
    /// or `jsconfig.json` between the file and the project folder, so the server knows only the
    /// open files (`SPEC-0320` v6 behaviour 21).
    tree_structure: bool,
    root: PathBuf,
    file: PathBuf,
    parsed: HashMap<PathBuf, Arc<Parsed>>,
    started: Instant,
    wait: Duration,
    ups: HashMap<TypeRef, Vec<Up>>,
    downs: HashMap<TypeRef, Vec<TypeRef>>,
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

impl Session {
    pub(crate) async fn open(
        root: &Path,
        file: &Path,
        text: Option<&str>,
        wait: Duration,
    ) -> Result<Self, LspError> {
        let o = Opened::open_text(root, file, text, wait).await?;
        let mut parsed = HashMap::new();
        let file = canonical(file);
        if let Some(p) = parse_file(&file, &o.text) {
            parsed.insert(file.clone(), Arc::new(p));
        }
        Ok(Self {
            o: Some(o),
            files: None,
            tree_structure: !has_project_config(root, &file),
            root: canonical(root),
            file,
            parsed,
            started: Instant::now(),
            wait,
            ups: HashMap::new(),
            downs: HashMap::new(),
        })
    }

    /// A session without a language server: names are resolved through the files, imports and
    /// re-exports of the project (`SPEC-0320` v5 behaviour 21).
    pub(crate) fn without_server(
        root: &Path,
        file: &Path,
        text: Option<&str>,
        wait: Duration,
    ) -> Self {
        let mut parsed = HashMap::new();
        let file = canonical(file);
        let text = match text {
            Some(t) => Some(t.to_owned()),
            None => std::fs::read_to_string(&file).ok(),
        };
        if let Some(p) = text.and_then(|t| parse_file(&file, &t)) {
            parsed.insert(file.clone(), Arc::new(p));
        }
        Self {
            o: None,
            files: None,
            tree_structure: true,
            root: canonical(root),
            file,
            parsed,
            started: Instant::now(),
            wait,
            ups: HashMap::new(),
            downs: HashMap::new(),
        }
    }

    /// The language server, or why there is none.
    pub(crate) fn server(&mut self) -> Result<&mut Opened, LspError> {
        self.o
            .as_mut()
            .ok_or_else(|| LspError::NoServer("no language server for this question".to_owned()))
    }

    pub(crate) fn left(&self) -> Duration {
        self.wait.saturating_sub(self.started.elapsed())
    }

    pub(crate) fn in_project(&self, path: &Path) -> bool {
        path.starts_with(&self.root) && !path.components().any(|c| c.as_os_str() == "node_modules")
    }

    pub(crate) fn parsed(&mut self, path: &Path) -> Option<Arc<Parsed>> {
        let key = canonical(path);
        if let Some(p) = self.parsed.get(&key) {
            return Some(Arc::clone(p));
        }
        let text = std::fs::read_to_string(&key).ok()?;
        let p = Arc::new(parse_file(&key, &text)?);
        self.parsed.insert(key, Arc::clone(&p));
        Some(p)
    }

    pub(crate) fn main(&mut self) -> Option<Arc<Parsed>> {
        let file = self.file.clone();
        self.parsed(&file)
    }

    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// The request parameters for a place of `path`, the file sent to the server first.
    pub(crate) async fn position_in(
        &mut self,
        path: &Path,
        line: u64,
        character: u64,
    ) -> Result<Value, LspError> {
        let wait = self.left();
        self.server()?.open_other(path, wait).await?;
        let text = self
            .parsed(path)
            .map(|p| p.text.clone())
            .unwrap_or_default();
        let units = crate::insight::to_server(
            crate::insight::line_of(&text, line),
            character,
            self.server()?.files.encoding,
        );
        Ok(
            json!({"textDocument": {"uri": file_uri(path)}, "position": {"line": line, "character": units}}),
        )
    }

    /// The direct supertypes of a project type (behaviour 2).
    async fn supers(&mut self, of: &TypeRef) -> Result<Vec<Up>, LspError> {
        if let Some(known) = self.ups.get(of) {
            return Ok(known.clone());
        }
        let Some(parsed) = self.parsed(&of.0) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for h in parsed.types[of.1].heritage.clone() {
            if let Some(up) = self.resolve(&of.0, &h).await? {
                out.push(up);
            }
        }
        self.ups.insert(of.clone(), out.clone());
        Ok(out)
    }

    async fn resolve(&mut self, path: &Path, h: &Heritage) -> Result<Option<Up>, LspError> {
        if self.tree_structure {
            return Ok(self.tree_resolve(path, h));
        }
        let at = self.position_in(path, h.line, h.character).await?;
        let left = self.left();
        let o = self.server()?;
        let v = o
            .ask_bounded("textDocument/definition", at, left, None)
            .await?;
        let found = parse_locations(&v, &mut o.files);
        let Some(first) = found.first().cloned() else {
            return Ok(None);
        };
        for l in &found {
            let p = canonical(&l.path);
            if !self.in_project(&p) {
                continue;
            }
            if let Some(i) = self
                .parsed(&p)
                .and_then(|f| f.type_named_at(l.range.start_line, l.range.start_character))
            {
                return Ok(Some(Up::Project((p, i))));
            }
        }
        Ok(Some(Up::Outside(h.name.clone(), first)))
    }

    /// The direct subtypes of a project type (behaviour 3).
    async fn subs(&mut self, of: &TypeRef) -> Result<Vec<TypeRef>, LspError> {
        if let Some(known) = self.downs.get(of) {
            return Ok(known.clone());
        }
        let Some(parsed) = self.parsed(&of.0) else {
            return Ok(Vec::new());
        };
        if self.tree_structure {
            let out = self.tree_subs(of);
            self.downs.insert(of.clone(), out.clone());
            return Ok(out);
        }
        let t = &parsed.types[of.1];
        let (line, character) = (t.line, t.character);
        let mut at = self.position_in(&of.0, line, character).await?;
        at["context"] = json!({"includeDeclaration": false});
        let left = self.left();
        let o = self.server()?;
        let v = o
            .ask_bounded("textDocument/references", at, left, None)
            .await?;
        let mut out: Vec<TypeRef> = Vec::new();
        for l in parse_locations(&v, &mut o.files) {
            let p = canonical(&l.path);
            if !self.in_project(&p) {
                continue;
            }
            if let Some(i) = self
                .parsed(&p)
                .and_then(|f| f.heritage_owner(l.range.start_line, l.range.start_character))
                && !out.contains(&(p.clone(), i))
            {
                out.push((p, i));
            }
        }
        out.sort();
        self.downs.insert(of.clone(), out.clone());
        Ok(out)
    }

    fn node(&mut self, of: &TypeRef) -> Option<CallNode> {
        let parsed = self.parsed(&of.0)?;
        let t = &parsed.types[of.1];
        Some(CallNode {
            name: t.name.clone(),
            kind: t.kind_name().to_owned(),
            detail: of
                .0
                .strip_prefix(&self.root)
                .ok()
                .map(|p| p.display().to_string()),
            location: Location {
                path: of.0.clone(),
                range: t.name_range(),
            },
            call_sites: Vec::new(),
            children: Vec::new(),
            recursive: false,
            mark: None,
        })
    }

    fn expand<'a>(
        &'a mut self,
        of: TypeRef,
        up: bool,
        seen: &'a mut HashSet<TypeRef>,
        budget: &'a mut usize,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<CallNode>, LspError>> + Send + 'a>,
    > {
        Box::pin(async move {
            *budget = budget.saturating_sub(1);
            let Some(mut node) = self.node(&of) else {
                return Ok(None);
            };
            if !seen.insert(of.clone()) {
                node.recursive = true;
                return Ok(Some(node));
            }
            if *budget == 0 || self.left().is_zero() {
                return Ok(Some(node));
            }
            if up {
                for next in self.supers(&of).await? {
                    if *budget == 0 {
                        break;
                    }
                    match next {
                        Up::Project(t) => {
                            if let Some(n) = self.expand(t, true, seen, budget).await? {
                                node.children.push(n);
                            }
                        }
                        Up::Outside(name, location) => {
                            *budget = budget.saturating_sub(1);
                            node.children.push(outside_node(name, location));
                        }
                    }
                }
            } else {
                for next in self.subs(&of).await? {
                    if *budget == 0 {
                        break;
                    }
                    if let Some(n) = self.expand(next, false, seen, budget).await? {
                        node.children.push(n);
                    }
                }
            }
            Ok(Some(node))
        })
    }

    /// The superclass of a class: what its `extends` names (behaviour 4).
    async fn superclass(&mut self, of: &TypeRef) -> Result<Option<Up>, LspError> {
        let Some(parsed) = self.parsed(&of.0) else {
            return Ok(None);
        };
        let t = &parsed.types[of.1];
        if t.is_interface {
            return Ok(None);
        }
        let Some(h) = t.heritage.iter().find(|h| h.extends).cloned() else {
            return Ok(None);
        };
        self.resolve(&of.0, &h).await
    }

    /// All project supertypes, nearest first.
    async fn all_supers(&mut self, of: &TypeRef) -> Result<Vec<TypeRef>, LspError> {
        let mut out: Vec<TypeRef> = Vec::new();
        let mut queue = vec![of.clone()];
        while let Some(next) = queue.pop() {
            if self.left().is_zero() {
                break;
            }
            for up in self.supers(&next).await? {
                if let Up::Project(t) = up
                    && t != *of
                    && !out.contains(&t)
                {
                    queue.insert(0, t.clone());
                    out.push(t);
                }
            }
        }
        Ok(out)
    }

    /// All project subtypes.
    async fn all_subs(&mut self, of: &TypeRef) -> Result<Vec<TypeRef>, LspError> {
        let mut out: Vec<TypeRef> = Vec::new();
        let mut queue = vec![of.clone()];
        while let Some(next) = queue.pop() {
            if self.left().is_zero() {
                break;
            }
            for t in self.subs(&next).await? {
                if t != *of && !out.contains(&t) {
                    queue.push(t.clone());
                    out.push(t);
                }
            }
        }
        Ok(out)
    }

    /// The direct project supertypes of the type `index` of the main file, with their files.
    pub(crate) async fn direct_project_supers(
        &mut self,
        index: usize,
    ) -> Result<Vec<TypeRef>, LspError> {
        let of = (self.file.clone(), index);
        Ok(self
            .supers(&of)
            .await?
            .into_iter()
            .filter_map(|u| match u {
                Up::Project(t) => Some(t),
                Up::Outside(..) => None,
            })
            .collect())
    }

    pub(crate) async fn close(self) {
        if let Some(o) = self.o {
            o.close().await;
        }
    }

    /// What a heritage name of `path` means, read from the project's syntax trees (`SPEC-0320` v5
    /// behaviour 21): a type of the same file, else the import that binds the name (`ns.Base`
    /// through `import * as ns`), followed through relative modules and their re-exports. A name
    /// from a package or a global name is a leaf where it is written; a relative module that is
    /// not found leaves the name out.
    fn tree_resolve(&mut self, path: &Path, h: &Heritage) -> Option<Up> {
        let path = canonical(path);
        let parsed = self.parsed(&path)?;
        let here = Up::Outside(
            h.name.clone(),
            Location {
                path: path.clone(),
                range: name_range(&h.name, h.line, h.character),
            },
        );
        if let Some(q) = &h.qualifier {
            let b = parsed
                .bindings
                .iter()
                .find(|b| b.local == *q && b.imported == Imported::Namespace)?;
            let module = b.module.clone();
            return self.via_module(&path, &module, &Imported::Named(h.name.clone()), here);
        }
        if let Some(i) = parsed.types.iter().position(|t| t.name == h.name) {
            return Some(Up::Project((path, i)));
        }
        match parsed.bindings.iter().find(|b| b.local == h.name) {
            Some(b) if b.imported != Imported::Namespace => {
                let (module, imported) = (b.module.clone(), b.imported.clone());
                self.via_module(&path, &module, &imported, here)
            }
            Some(_) => None,
            None => Some(here),
        }
    }

    fn via_module(&mut self, from: &Path, module: &str, wanted: &Imported, here: Up) -> Option<Up> {
        if !module.starts_with('.') {
            return Some(here);
        }
        let file = resolve_module(from.parent()?, module)?;
        if !self.in_project(&file) {
            return Some(here);
        }
        self.exported_type(&file, wanted, 0).map(Up::Project)
    }

    /// The project type `file` exports as `wanted`, through at most eight re-exports.
    fn exported_type(&mut self, file: &Path, wanted: &Imported, depth: usize) -> Option<TypeRef> {
        if depth > 8 {
            return None;
        }
        let parsed = self.parsed(file)?;
        let local = match wanted {
            Imported::Default => parsed.default_export.clone(),
            Imported::Named(n) => parsed
                .local_exports
                .iter()
                .find(|(exported, _)| exported == n)
                .map(|(_, local)| local.clone())
                .or_else(|| {
                    parsed
                        .types
                        .iter()
                        .any(|t| t.exported && t.name == *n)
                        .then(|| n.clone())
                }),
            Imported::Namespace => None,
        };
        if let Some(local) = local {
            if let Some(i) = parsed.types.iter().position(|t| t.name == local) {
                return Some((canonical(file), i));
            }
            // Exported under another name after an import of its own.
            if let Some(b) = parsed.bindings.iter().find(|b| b.local == local) {
                let next = resolve_module(file.parent()?, &b.module)?;
                return self.exported_type(&next, &b.imported, depth + 1);
            }
            return None;
        }
        let Imported::Named(name) = wanted else {
            return None;
        };
        for r in parsed.reexports.clone() {
            if !r.module.starts_with('.') {
                continue;
            }
            let original = match &r.names {
                Some(names) => match names.iter().find(|(exported, _)| exported == name) {
                    Some((_, original)) => original.clone(),
                    None => continue,
                },
                None => name.clone(),
            };
            let Some(next) = file.parent().and_then(|d| resolve_module(d, &r.module)) else {
                continue;
            };
            let imported = if original == "default" {
                Imported::Default
            } else {
                Imported::Named(original)
            };
            if let Some(found) = self.exported_type(&next, &imported, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    /// The project's script files: below the root, not in `node_modules`, build output or
    /// hidden folders, at most [`MAX_PROJECT_FILES`].
    fn project_files(&mut self) -> Vec<PathBuf> {
        if let Some(f) = &self.files {
            return f.clone();
        }
        let mut out = Vec::new();
        let mut dirs = vec![self.root.clone()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for path in entries {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if path.is_dir() {
                    if !name.starts_with('.') && !SKIPPED_FOLDERS.contains(&name.as_ref()) {
                        dirs.push(path);
                    }
                } else if is_script(&path) && out.len() < MAX_PROJECT_FILES {
                    out.push(canonical(&path));
                }
            }
        }
        out.sort();
        self.files = Some(out.clone());
        out
    }

    /// The direct subtypes of a project type from the syntax trees: the types of project files
    /// whose `extends`/`implements` names resolve to it (`SPEC-0320` v5 behaviour 21).
    fn tree_subs(&mut self, of: &TypeRef) -> Vec<TypeRef> {
        let Some(name) = self.parsed(&of.0).map(|p| p.types[of.1].name.clone()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for file in self.project_files() {
            if self.left().is_zero() {
                break;
            }
            let Some(parsed) = self.parsed(&file) else {
                continue;
            };
            // Only files that mention the name can mean it (an alias still names it in the
            // import), or that import a default export under a name of their own; which type a
            // name means is decided by resolving it.
            if !parsed.text.contains(name.as_str())
                && !parsed
                    .bindings
                    .iter()
                    .any(|b| b.imported == Imported::Default)
            {
                continue;
            }
            for (i, t) in parsed.types.iter().enumerate() {
                for h in &t.heritage {
                    if let Some(Up::Project(up)) = self.tree_resolve(&file, h)
                        && up == *of
                        && !out.contains(&(file.clone(), i))
                    {
                        out.push((file.clone(), i));
                    }
                }
            }
        }
        out.sort();
        out
    }
}

/// Whether a `tsconfig.json` or `jsconfig.json` lies in the folder of `file` or above it, up to
/// `root`: only then does the language server know the project's other files (`SPEC-0320` v6
/// behaviour 21).
pub fn has_project_config(root: &Path, file: &Path) -> bool {
    let root = canonical(root);
    let file = canonical(file);
    file.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(&root))
        .any(|dir| dir.join("tsconfig.json").is_file() || dir.join("jsconfig.json").is_file())
}

/// Folders that hold no project sources.
const SKIPPED_FOLDERS: &[&str] = &["node_modules", "dist", "build", "out", "coverage", "target"];

/// The most script files read for one answer without a server.
const MAX_PROJECT_FILES: usize = 5000;

/// The file a relative module names: as written, with a script extension, a `.js` written for
/// a `.ts` file, or the folder's `index` (`SPEC-0320` v5 behaviour 21).
pub fn resolve_module(dir: &Path, module: &str) -> Option<PathBuf> {
    const EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "mts", "cts", "js", "jsx", "mjs", "cjs"];
    let base = dir.join(module);
    let mut candidates = vec![base.clone()];
    let written = base.to_string_lossy().into_owned();
    for (js, ts) in [
        (".js", ".ts"),
        (".js", ".tsx"),
        (".mjs", ".mts"),
        (".cjs", ".cts"),
    ] {
        if let Some(stem) = written.strip_suffix(js) {
            candidates.push(PathBuf::from(format!("{stem}{ts}")));
        }
    }
    for e in EXTENSIONS {
        candidates.push(PathBuf::from(format!("{written}.{e}")));
    }
    for e in ["ts", "tsx", "js", "jsx"] {
        candidates.push(base.join(format!("index.{e}")));
    }
    candidates
        .into_iter()
        .find(|c| c.is_file() && is_script(c))
        .map(|c| canonical(&c))
}

fn outside_node(name: String, location: Location) -> CallNode {
    CallNode {
        name,
        kind: "class".into(),
        detail: Some("library".into()),
        location,
        call_sites: Vec::new(),
        children: Vec::new(),
        recursive: false,
        mark: None,
    }
}

/// The type hierarchy of the type at (or around) a place of a script file (behaviours 1–4).
#[allow(clippy::too_many_arguments)]
pub async fn type_hierarchy_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    direction: TypeDirection,
    wait: Duration,
) -> Result<Option<CallNode>, LspError> {
    // Without a language server, or when it fails, from the syntax trees (v5 behaviour 21).
    if let Ok(mut s) = Session::open(root, file, text, wait).await {
        let result = hierarchy(&mut s, line, character, direction).await;
        s.close().await;
        if result.is_ok() {
            return result;
        }
    }
    let mut s = Session::without_server(root, file, text, wait);
    hierarchy(&mut s, line, character, direction).await
}

async fn hierarchy(
    s: &mut Session,
    line: u64,
    character: u64,
    direction: TypeDirection,
) -> Result<Option<CallNode>, LspError> {
    let Some(index) = s.main().and_then(|p| p.type_at(line, character)) else {
        return Ok(None);
    };
    let of = (s.file.clone(), index);
    let mut seen = HashSet::new();
    let mut budget = NODE_LIMIT;
    match direction {
        TypeDirection::Supertypes | TypeDirection::Subtypes => {
            let up = direction == TypeDirection::Supertypes;
            let mut tree = s.expand(of, up, &mut seen, &mut budget).await?;
            if let Some(t) = &mut tree {
                t.mark = Some("*".into());
            }
            Ok(tree)
        }
        TypeDirection::Class => {
            let Some(mut tree) = s.expand(of.clone(), false, &mut seen, &mut budget).await? else {
                return Ok(None);
            };
            tree.mark = Some("*".into());
            let mut at = of.clone();
            let mut chain = HashSet::from([of]);
            while let Some(up) = s.superclass(&at).await? {
                match up {
                    Up::Project(t) => {
                        if !chain.insert(t.clone()) {
                            break;
                        }
                        let Some(mut parent) = s.node(&t) else {
                            break;
                        };
                        parent.children.push(tree);
                        tree = parent;
                        at = t;
                    }
                    Up::Outside(name, location) => {
                        let mut parent = outside_node(name, location);
                        parent.children.push(tree);
                        tree = parent;
                        break;
                    }
                }
            }
            Ok(Some(tree))
        }
    }
}

/// The gutter marks of a script file (behaviours 8 and 10).
pub async fn declaration_marks_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<DeclarationMark>, LspError> {
    // Without a language server, or when it fails, from the syntax trees (v5 behaviour 21).
    if let Ok(mut s) = Session::open(root, file, text, wait).await {
        let result = marks(&mut s).await;
        s.close().await;
        if result.is_ok() {
            return result;
        }
    }
    let mut s = Session::without_server(root, file, text, wait);
    marks(&mut s).await
}

fn target(s: &mut Session, of: &TypeRef, member: Option<&str>) -> Option<MarkTarget> {
    let parsed = s.parsed(&of.0)?;
    let t = &parsed.types[of.1];
    let range = match member.and_then(|m| t.member(m)) {
        Some(m) => name_range(&m.name, m.line, m.character),
        None => t.name_range(),
    };
    Some(MarkTarget {
        name: t.name.clone(),
        location: Location {
            path: of.0.clone(),
            range,
        },
    })
}

async fn marks(s: &mut Session) -> Result<Vec<DeclarationMark>, LspError> {
    let Some(main) = s.main() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (i, t) in main.types.iter().enumerate() {
        if s.left().is_zero() {
            break;
        }
        let of = (s.file.clone(), i);
        let ups = s.all_supers(&of).await?;
        let downs = s.all_subs(&of).await?;
        if !downs.is_empty() {
            let targets = downs.iter().filter_map(|d| target(s, d, None)).collect();
            out.push(DeclarationMark {
                line: t.line,
                character: t.character,
                name: t.name.clone(),
                kind: MarkKind::Inherited,
                targets,
            });
        }
        for m in t.members.iter().filter(|m| m.overridable()) {
            let mut up_targets = Vec::new();
            let mut implements = false;
            for u in &ups {
                let Some(file) = s.parsed(&u.0) else { continue };
                let holder = &file.types[u.1];
                let Some(sm) = holder.member(&m.name).filter(|x| x.overridable()) else {
                    continue;
                };
                if up_targets.is_empty() {
                    implements = holder.is_interface || sm.is_abstract;
                }
                up_targets.extend(target(s, u, Some(&m.name)));
            }
            if !up_targets.is_empty() {
                out.push(DeclarationMark {
                    line: m.line,
                    character: m.character,
                    name: m.name.clone(),
                    kind: if implements {
                        MarkKind::Implements
                    } else {
                        MarkKind::Overrides
                    },
                    targets: up_targets,
                });
            }
            let mut down_targets = Vec::new();
            for d in &downs {
                let Some(file) = s.parsed(&d.0) else { continue };
                if file.types[d.1]
                    .member(&m.name)
                    .is_some_and(Member::overridable)
                {
                    down_targets.extend(target(s, d, Some(&m.name)));
                }
            }
            if !down_targets.is_empty() {
                out.push(DeclarationMark {
                    line: m.line,
                    character: m.character,
                    name: m.name.clone(),
                    kind: if t.is_interface || m.is_abstract {
                        MarkKind::Implemented
                    } else {
                        MarkKind::Overridden
                    },
                    targets: down_targets,
                });
            }
        }
    }
    out.sort_by_key(|m| (m.line, m.character));
    Ok(out)
}

/// The declarations of a `documentSymbol` answer that get a count (behaviour 5).
pub fn counted_declarations(symbols: &Value) -> Vec<UsageCount> {
    fn kind(k: Option<u64>) -> &'static str {
        match k {
            Some(5) => "class",
            Some(6) => "method",
            Some(7 | 8) => "property",
            Some(10) => "enum",
            Some(11) => "interface",
            Some(12) => "function",
            Some(13) => "variable",
            Some(14) => "constant",
            Some(22) => "enum member",
            Some(23) => "struct",
            Some(26) => "type",
            _ => "",
        }
    }
    fn walk(list: &Value, member_level: bool, out: &mut Vec<UsageCount>) {
        for s in list.as_array().into_iter().flatten() {
            let name = s["name"].as_str().unwrap_or_default();
            let k = s["kind"].as_u64();
            let is_type = matches!(k, Some(5 | 10 | 11));
            let is_namespace = matches!(k, Some(2 | 3));
            if is_namespace {
                walk(&s["children"], false, out);
                continue;
            }
            let wanted = if member_level {
                matches!(k, Some(6 | 7 | 8 | 22))
            } else {
                !kind(k).is_empty() && !matches!(k, Some(6 | 7 | 8 | 22))
            };
            let named =
                !name.is_empty() && !name.contains(['<', '(', ' ']) && name != "constructor";
            let at = if s["selectionRange"].is_object() {
                &s["selectionRange"]["start"]
            } else if s["location"].is_object() {
                &s["location"]["range"]["start"]
            } else {
                &s["range"]["start"]
            };
            if wanted
                && named
                && let (Some(line), Some(character)) =
                    (at["line"].as_u64(), at["character"].as_u64())
            {
                out.push(UsageCount {
                    name: name.to_owned(),
                    kind: kind(k),
                    line,
                    character,
                    count: 0,
                    inheritors: 0,
                });
            }
            if is_type {
                walk(&s["children"], true, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(symbols, false, &mut out);
    out.sort_by_key(|u| (u.line, u.character));
    out.truncate(MAX_DECLARATIONS);
    out
}

/// Usage counts of a script file: members too, unused ones too, types with inheritors
/// (behaviours 5–7).
pub async fn usage_counts_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<UsageCount>, LspError> {
    let mut s = Session::open(root, file, text, wait).await?;
    let result = counts(&mut s).await;
    s.close().await;
    result
}

/// For a member that can be overridden, the members of project subtypes that declare it again:
/// implementations below an interface member or an abstract one, overrides below another
/// (`SPEC-0320` v5 behaviour 22, as `SPEC-0292` v6 behaviour 9 for Kotlin).
async fn below_member(s: &mut Session, d: &mut UsageCount) -> Result<(), LspError> {
    let Some(main) = s.main() else {
        return Ok(());
    };
    let owner = main.types.iter().enumerate().find_map(|(i, t)| {
        t.members
            .iter()
            .find(|m| m.line == d.line && m.name == d.name && m.overridable())
            .map(|m| (i, t.is_interface || m.is_abstract, m.kind))
    });
    let Some((i, is_abstract, kind)) = owner else {
        return Ok(());
    };
    let of = (s.file.clone(), i);
    let mut below = 0;
    for sub in s.all_subs(&of).await? {
        if s.parsed(&sub.0).is_some_and(|f| {
            f.types[sub.1]
                .member(&d.name)
                .is_some_and(Member::overridable)
        }) {
            below += 1;
        }
    }
    d.inheritors = below;
    if is_abstract {
        d.kind = if kind == MemberKind::Method {
            "abstract method"
        } else {
            "abstract property"
        };
    }
    Ok(())
}

async fn counts(s: &mut Session) -> Result<Vec<UsageCount>, LspError> {
    let uri = s.server()?.uri.clone();
    let symbols = s
        .server()?
        .ask(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .await?;
    let main = s.main();
    let mut found = Vec::new();
    for mut d in counted_declarations(&symbols) {
        if s.left().is_zero() {
            break;
        }
        let left = s.left();
        let references = s
            .server()?
            .ask_bounded(
                "textDocument/references",
                json!({"textDocument": {"uri": uri},
                    "position": {"line": d.line, "character": d.character},
                    "context": {"includeDeclaration": false}}),
                left,
                None,
            )
            .await?;
        // The places Find Usages lists, so the count and the list agree (`SPEC-0320` v8
        // behaviour 5a): not the declarations of members that implement or override this one.
        let declaration = crate::insight::Location {
            path: s.file.clone(),
            range: crate::insight::Range {
                start_line: d.line,
                start_character: d.character,
                end_line: d.line,
                end_character: d.character,
            },
        };
        let (root, file) = (s.root.clone(), s.file.clone());
        let o = s.server()?;
        let places = crate::insight::parse_locations(&references, &mut o.files);
        let content = o.text.clone();
        d.count =
            crate::find_usages::usages_of(&root, &file, Some(&content), places, Some(&declaration))
                .len();
        if matches!(d.kind, "class" | "interface")
            && let Some(i) = main
                .as_ref()
                .and_then(|p| p.type_named_at(d.line, d.character))
        {
            let of = (s.file.clone(), i);
            d.inheritors = s.all_subs(&of).await?.len();
        }
        if matches!(d.kind, "method" | "property") {
            below_member(s, &mut d).await?;
        }
        found.push(d);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Parsed {
        parse_file(Path::new("a.ts"), text).unwrap()
    }

    /// SPEC-0320 behaviour 1: declarations, heritage and members from the syntax tree.
    #[test]
    fn reads_classes_and_heritage() {
        let p = parse(
            "import { X as Y } from './x';\nexport abstract class A<T> extends ns.B<T> implements I, J<T> {\n  /** doc */\n  private a = 1;\n  static s: number;\n  abstract m(x: number, y = 2): void;\n  get v(): string { return this.a + ''; }\n  constructor(private z: number) { super(); }\n}\ninterface K extends L { p?: string; q(): void }\nconst u = 1;\nfunction f() {}\n",
        );
        let a = &p.types[0];
        assert_eq!(a.name, "A");
        assert!(a.exported && a.is_abstract);
        let h: Vec<_> = a
            .heritage
            .iter()
            .map(|h| (h.name.as_str(), h.extends))
            .collect();
        assert_eq!(h, [("B", true), ("I", false), ("J", false)]);
        assert_eq!(a.type_parameters, ["T"]);
        let names: Vec<_> = a
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("a", MemberKind::Field),
                ("s", MemberKind::Field),
                ("m", MemberKind::Method),
                ("v", MemberKind::Getter),
                ("constructor", MemberKind::Constructor)
            ]
        );
        assert!(a.members[0].visibility.as_deref() == Some("private"));
        assert!(a.members[1].is_static);
        assert!(a.members[2].is_abstract);
        assert_eq!(a.members[3].this_uses, ["a"]);
        assert_eq!(a.members[4].parameter_properties, ["z"]);
        assert!(a.members[4].uses_super);
        assert!(p.text[a.members[0].start..].starts_with("/** doc */"));
        assert!(p.text[..a.members[0].end].ends_with("= 1;"));
        let k = &p.types[1];
        assert!(k.is_interface);
        assert_eq!(k.heritage[0].name, "L");
        assert_eq!(k.members.len(), 2);
        for name in ["Y", "A", "K", "u", "f"] {
            assert!(p.top_level.iter().any(|n| n == name), "{name}");
        }
        assert_eq!(p.imports.last().map(String::as_str), Some("Y"));
    }

    /// SPEC-0320 behaviour 1: the type around a place, the inner one when nested.
    #[test]
    fn finds_type_around_a_place() {
        let p = parse("class A {\n  m() {\n    class B {}\n  }\n}\n");
        assert_eq!(p.types[p.type_at(1, 3).unwrap()].name, "A");
        assert_eq!(p.types[p.type_at(2, 12).unwrap()].name, "B");
        assert_eq!(p.type_at(4, 5), None);
    }

    /// SPEC-0320 JavaScript has classes with `extends`, fields and accessors.
    #[test]
    fn reads_javascript() {
        let p = parse_file(
            Path::new("a.js"),
            "class A extends B {\n  x = 1;\n  m(a) { return this.x; }\n}\n",
        )
        .unwrap();
        let t = &p.types[0];
        assert_eq!(t.heritage[0].name, "B");
        assert!(t.heritage[0].extends);
        assert_eq!(t.members[0].name, "x");
        assert_eq!(t.members[1].this_uses, ["x"]);
        assert_eq!(t.members[1].signature, None);
    }

    /// SPEC-0320 behaviour 12: signatures with their types; missing ones ask the server.
    #[test]
    fn builds_signatures() {
        let p = parse(
            "class A {\n  readonly r: number = 1;\n  o?: string;\n  m<T>(a: T, b: number = 2, private c?: string): T { return a; }\n  n(a) {}\n  get g(): string { return ''; }\n  set w(v: boolean) {}\n}\n",
        );
        let sig = |n: &str| p.types[0].member(n).unwrap().signature.clone();
        assert_eq!(sig("r").unwrap(), "readonly r: number");
        assert_eq!(sig("o").unwrap(), "o?: string");
        assert_eq!(sig("m").unwrap(), "m<T>(a: T, b?: number, c?: string): T");
        assert_eq!(sig("n"), None);
        assert_eq!(sig("g").unwrap(), "readonly g: string");
        assert_eq!(sig("w").unwrap(), "w: boolean");
    }

    /// SPEC-0320 behaviour 12: the signature part of a hover.
    #[test]
    fn reads_hover_signatures() {
        let h = "```typescript\n(method) Circle.area(x: number): number\n```\ndoc";
        assert_eq!(
            hover_signature(h, "area").unwrap(),
            "area(x: number): number"
        );
        assert_eq!(
            hover_signature("(property) A.r: number", "r").unwrap(),
            "r: number"
        );
        assert_eq!(hover_signature("(alias) other", "r"), None);
    }

    /// SPEC-0320 behaviours 5 and 6: members and file-level declarations are counted, not callbacks.
    #[test]
    fn chooses_declarations() {
        let at = |line: u64| json!({"start": {"line": line, "character": 0}, "end": {"line": line, "character": 1}});
        let sym = |name: &str, kind: u64, line: u64, children: Value| json!({"name": name, "kind": kind, "range": at(line), "selectionRange": at(line), "children": children});
        let tree = json!([
            sym(
                "A",
                5,
                0,
                json!([
                    sym("constructor", 9, 1, json!([])),
                    sym("m", 6, 2, json!([sym("local", 13, 3, json!([]))])),
                    sym("x", 7, 4, json!([])),
                ])
            ),
            sym("f", 12, 5, json!([sym("inner", 12, 6, json!([]))])),
            sym("describe() callback", 12, 7, json!([])),
            sym("N", 2, 8, json!([sym("g", 13, 9, json!([]))])),
        ]);
        let names: Vec<_> = counted_declarations(&tree)
            .iter()
            .map(|d| d.name.clone())
            .collect();
        assert_eq!(names, ["A", "m", "x", "f", "g"]);
    }

    /// SPEC-0320: which members take part in overriding, moving and extracting.
    #[test]
    fn classifies_members() {
        let p = parse(
            "class A {\n  a() {}\n  private b() {}\n  static c() {}\n  #d = 1;\n  protected e = 1;\n  constructor() {}\n}\n",
        );
        let m = |n: &str| p.types[0].member(n).unwrap().clone();
        assert!(m("a").overridable() && m("a").public_instance());
        assert!(!m("b").overridable());
        assert!(!m("c").overridable() && !m("c").public_instance());
        assert!(!m("#d").overridable() && !m("#d").public_instance());
        assert!(m("e").overridable() && !m("e").public_instance());
        assert!(!m("constructor").movable() && m("c").movable());
    }

    /// A small project for the answers without a language server (SPEC-0320 v5 AK-14).
    fn tree_project(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("idecraft-ts-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            (
                "shapes/base.ts",
                "export abstract class Base {\n  abstract area(): number;\n  describe(): string { return ''; }\n  private secret(): void {}\n  static make(): void {}\n}\n",
            ),
            (
                "shapes/shape.ts",
                "export interface Shape {\n  area(): number;\n}\n",
            ),
            (
                "shapes/index.ts",
                "export { Base } from './base';\nexport * from './shape';\n",
            ),
            (
                "shapes/circle.ts",
                "import { Base } from './base';\nimport type { Shape } from './shape.js';\nexport class Circle extends Base implements Shape {\n  area() { return 1; }\n  describe() { return 'c'; }\n  secret() {}\n}\n",
            ),
            (
                "square.ts",
                "import * as s from './shapes';\nexport default class Square extends s.Base {\n  area() { return 2; }\n}\n",
            ),
            (
                "tile.tsx",
                "import Sq from './square';\nexport class Tile extends Sq {}\n",
            ),
            (
                "lib.ts",
                "import { Component } from 'react';\nclass View extends Component {}\nclass Failure extends Error {}\nclass Lost extends Missing {}\nimport { Gone } from './nowhere';\nclass Away extends Gone {}\n",
            ),
            (
                "node_modules/x/index.ts",
                "import { Base } from '../../shapes/base';\nexport class Fake extends Base { area() { return 0; } }\n",
            ),
            (
                "dist/out.js",
                "import { Base } from '../shapes/base';\nexport class Built extends Base {}\n",
            ),
        ];
        for (rel, text) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root.canonicalize().unwrap()
    }

    fn tree_session(root: &Path, rel: &str) -> Session {
        Session::without_server(root, &root.join(rel), None, Duration::from_secs(10))
    }

    fn names(s: &mut Session, list: &[TypeRef]) -> Vec<String> {
        list.iter()
            .map(|t| s.parsed(&t.0).unwrap().types[t.1].name.clone())
            .collect()
    }

    /// SPEC-0320 v5 behaviour 21: imports, aliases, namespace imports, default exports and
    /// re-exports are followed; packages and globals are leaves; node_modules and build output
    /// are no project files; a module that is not found leaves the name out.
    #[tokio::test]
    async fn without_a_server_names_are_resolved_through_the_project() {
        let root = tree_project("resolve");
        let mut s = tree_session(&root, "shapes/base.ts");
        let base = (root.join("shapes/base.ts"), 0);
        let direct = s.subs(&base).await.unwrap();
        assert_eq!(names(&mut s, &direct), ["Circle", "Square"]);
        let all = s.all_subs(&base).await.unwrap();
        assert_eq!(names(&mut s, &all), ["Circle", "Square", "Tile"]);

        let circle = (root.join("shapes/circle.ts"), 0);
        let ups = s.all_supers(&circle).await.unwrap();
        assert_eq!(names(&mut s, &ups), ["Base", "Shape"]);
        let tile = (root.join("tile.tsx"), 0);
        let ups = s.all_supers(&tile).await.unwrap();
        assert_eq!(names(&mut s, &ups), ["Square", "Base"]);

        let lib = root.join("lib.ts");
        let leaves: Vec<String> = (0..4)
            .map(|i| {
                let parsed = s.parsed(&lib).unwrap();
                let h = parsed.types[i].heritage[0].clone();
                match s.tree_resolve(&lib, &h) {
                    Some(Up::Outside(name, at)) => {
                        assert_eq!(at.path, lib);
                        format!("leaf {name}")
                    }
                    Some(Up::Project(_)) => "project".into(),
                    None => "left out".into(),
                }
            })
            .collect();
        assert_eq!(
            leaves,
            ["leaf Component", "leaf Error", "leaf Missing", "left out"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0320 v5 behaviours 21 and 1: the hierarchy window's three directions without a server.
    #[tokio::test]
    async fn without_a_server_the_type_hierarchy_has_all_directions() {
        let root = tree_project("hierarchy");
        let mut s = tree_session(&root, "shapes/circle.ts");
        let up = hierarchy(&mut s, 2, 15, TypeDirection::Supertypes)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(up.name, "Circle");
        assert_eq!(up.mark.as_deref(), Some("*"));
        let ups: Vec<_> = up.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(ups, ["Base", "Shape"]);
        let mut s = tree_session(&root, "shapes/base.ts");
        let down = hierarchy(&mut s, 0, 25, TypeDirection::Subtypes)
            .await
            .unwrap()
            .unwrap();
        let downs: Vec<_> = down.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(downs, ["Circle", "Square"]);
        assert_eq!(down.children[1].children[0].name, "Tile");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0320 v5 behaviours 21 and 8: gutter marks without a server.
    #[tokio::test]
    async fn without_a_server_the_gutter_marks_are_found() {
        let root = tree_project("marks");
        let mut s = tree_session(&root, "shapes/base.ts");
        let found: Vec<_> = marks(&mut s)
            .await
            .unwrap()
            .into_iter()
            .map(|m| {
                let targets: Vec<_> = m.targets.iter().map(|t| t.name.clone()).collect();
                (m.name, m.kind, targets)
            })
            .collect();
        assert_eq!(
            found,
            [
                (
                    "Base".to_owned(),
                    MarkKind::Inherited,
                    vec!["Circle".to_owned(), "Square".to_owned(), "Tile".to_owned()]
                ),
                (
                    "area".to_owned(),
                    MarkKind::Implemented,
                    vec!["Circle".to_owned(), "Square".to_owned()]
                ),
                (
                    "describe".to_owned(),
                    MarkKind::Overridden,
                    vec!["Circle".to_owned()]
                ),
            ]
        );
        let mut s = tree_session(&root, "shapes/circle.ts");
        let kinds: Vec<_> = marks(&mut s)
            .await
            .unwrap()
            .into_iter()
            .map(|m| (m.name, m.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("area".to_owned(), MarkKind::Implements),
                ("describe".to_owned(), MarkKind::Overrides),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0320 v5 behaviour 22: what is below a member, and its label.
    #[tokio::test]
    async fn members_count_their_implementations_and_overrides() {
        let root = tree_project("members");
        let mut s = tree_session(&root, "shapes/base.ts");
        let count = |name: &str, line: u64| UsageCount {
            name: name.to_owned(),
            kind: "method",
            line,
            character: 2,
            count: 0,
            inheritors: 0,
        };
        let mut area = count("area", 1);
        below_member(&mut s, &mut area).await.unwrap();
        assert_eq!((area.kind, area.inheritors), ("abstract method", 2));
        assert_eq!(area.label(), "no usages · 2 implementations");
        let mut describe = count("describe", 2);
        below_member(&mut s, &mut describe).await.unwrap();
        assert_eq!((describe.kind, describe.inheritors), ("method", 1));
        assert_eq!(describe.label(), "no usages · 1 override");
        // Private and static members have nothing below them.
        for (name, line) in [("secret", 3), ("make", 4)] {
            let mut d = count(name, line);
            below_member(&mut s, &mut d).await.unwrap();
            assert_eq!(d.inheritors, 0, "{name}");
        }
        let mut s = tree_session(&root, "shapes/shape.ts");
        let mut d = count("area", 1);
        below_member(&mut s, &mut d).await.unwrap();
        assert_eq!((d.kind, d.inheritors), ("abstract method", 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0320 v6 behaviour 21: a project config between the file and the project folder.
    #[test]
    fn project_config_is_looked_for_up_to_the_root() {
        let root = tree_project("config");
        let circle = root.join("shapes/circle.ts");
        assert!(!has_project_config(&root, &circle));
        std::fs::write(root.join("jsconfig.json"), "{}").unwrap();
        assert!(has_project_config(&root, &circle));
        std::fs::remove_file(root.join("jsconfig.json")).unwrap();
        std::fs::write(root.join("shapes/tsconfig.json"), "{}").unwrap();
        assert!(has_project_config(&root, &circle));
        assert!(!has_project_config(&root, &root.join("square.ts")));
        // A config above the project folder does not count.
        let inner = root.join("shapes");
        std::fs::remove_file(inner.join("tsconfig.json")).unwrap();
        std::fs::write(root.join("tsconfig.json"), "{}").unwrap();
        assert!(!has_project_config(&inner, &circle));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0320 v5 behaviour 21: the module links a file declares.
    #[test]
    fn reads_imports_and_exports() {
        let p = parse(
            "import D, { A, B as C } from './x';\nimport * as ns from \"../y\";\nexport { E as F } from './e';\nexport * from './all';\nexport * as group from './g';\nconst G = 1;\nexport { G as H };\nexport default class K extends ns.Base {}\n",
        );
        let b: Vec<_> = p
            .bindings
            .iter()
            .map(|b| (b.local.as_str(), b.imported.clone(), b.module.as_str()))
            .collect();
        assert_eq!(
            b,
            [
                ("D", Imported::Default, "./x"),
                ("A", Imported::Named("A".into()), "./x"),
                ("C", Imported::Named("B".into()), "./x"),
                ("ns", Imported::Namespace, "../y"),
            ]
        );
        assert_eq!(
            p.reexports,
            [
                ReExport {
                    module: "./e".into(),
                    names: Some(vec![("F".into(), "E".into())]),
                },
                ReExport {
                    module: "./all".into(),
                    names: None,
                },
            ]
        );
        assert_eq!(p.local_exports, [("H".to_owned(), "G".to_owned())]);
        assert_eq!(p.default_export.as_deref(), Some("K"));
        assert_eq!(p.types[0].heritage[0].qualifier.as_deref(), Some("ns"));
        let p = parse("const x = 1;\nexport default x;\n");
        assert_eq!(p.default_export.as_deref(), Some("x"));
    }
}
