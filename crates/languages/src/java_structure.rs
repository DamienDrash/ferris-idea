//! Java class structure (`SPEC-0082` v5, `SPEC-0122` v3): the type hierarchy, the method
//! hierarchy and the super method from the language server's type hierarchy together with the
//! syntax tree of each project file, the methods a class can override or must implement from
//! jdtls, and Surround With on the syntax tree. Kotlin files take the same entry points and are
//! answered by [`crate::kotlin_structure`] (`SPEC-0082` v6, `SPEC-0122` v6).

use std::collections::HashSet;
use std::ops::Range as Bytes;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tree_sitter::Node;

use crate::insight::{
    CallNode, Location, Opened, Range, SYMBOL_KINDS, kind_name, parse_range, uri_to_path,
};
use crate::jvm::{Jvm, children, has_word, modifiers_of, parse, text_of};
use crate::lsp::LspError;

// ------------------------------------------------------------------ the syntax tree of one file

/// What kind of type a declaration is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    /// `class`.
    Class,
    /// `interface` (also `@interface`).
    Interface,
    /// `enum`.
    Enum,
    /// `record`.
    Record,
}

/// A method declared in a type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MethodDecl {
    /// Name.
    pub name: String,
    /// Parameter types as written.
    pub params: Vec<String>,
    /// The return type as written (`SPEC-0122` v7 behaviour 6b); `void` included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// Without a body (`abstract`, or an interface method that is not `default`/`static`).
    pub is_abstract: bool,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column of the name, in characters.
    pub column: usize,
    /// Can be overridden: neither `private` nor `static` (`SPEC-0320` v2).
    #[serde(skip)]
    pub overridable: bool,
}

/// A type declaration of a Java file with its own methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDecl {
    /// Simple name.
    pub name: String,
    /// Class, interface, enum or record.
    pub kind: TypeKind,
    /// Declared `abstract` (interfaces count as abstract).
    pub is_abstract: bool,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column of the name, in characters.
    pub column: usize,
    /// The methods declared in the body (constructors left out).
    pub methods: Vec<MethodDecl>,
}

impl TypeDecl {
    /// The method of this type matching `name` with as many parameters whose simple types match.
    pub fn method(&self, name: &str, params: &[String]) -> Option<&MethodDecl> {
        self.methods
            .iter()
            .find(|m| m.name == name && same_params(&m.params, params))
    }
}

/// Whether two parameter lists are the same up to generic arguments and package names.
fn same_params(a: &[String], b: &[String]) -> bool {
    fn simple(t: &str) -> String {
        let t = t.split('<').next().unwrap_or(t).trim();
        let t = t.rsplit('.').next().unwrap_or(t);
        t.replace("...", "[]").replace(' ', "")
    }
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| simple(x) == simple(y))
}

fn type_kind(kind: &str) -> Option<TypeKind> {
    match kind {
        "class_declaration" => Some(TypeKind::Class),
        "interface_declaration" | "annotation_type_declaration" => Some(TypeKind::Interface),
        "enum_declaration" => Some(TypeKind::Enum),
        "record_declaration" => Some(TypeKind::Record),
        _ => None,
    }
}

fn column(text: &str, n: Node) -> usize {
    let start = n.start_byte();
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..start].chars().count()
}

/// The methods of a type body (an enum's methods sit in its body declarations).
fn methods_of(body: Node, text: &str, interface: bool) -> Vec<MethodDecl> {
    let mut members = children(body);
    if let Some(decls) = members
        .iter()
        .find(|m| m.kind() == "enum_body_declarations")
        .copied()
    {
        members = children(decls);
    }
    members
        .into_iter()
        .filter(|m| m.kind() == "method_declaration")
        .filter_map(|m| {
            let name = m.child_by_field_name("name")?;
            let params = m
                .child_by_field_name("parameters")
                .map(|ps| {
                    children(ps)
                        .into_iter()
                        .filter(|p| matches!(p.kind(), "formal_parameter" | "spread_parameter"))
                        .map(|p| {
                            let ty = p
                                .child_by_field_name("type")
                                .or_else(|| {
                                    children(p).into_iter().find(|c| c.kind().ends_with("type"))
                                })
                                .map(|t| text_of(t, text).to_owned())
                                .unwrap_or_default();
                            if p.kind() == "spread_parameter" {
                                format!("{ty}...")
                            } else {
                                ty
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mods = modifiers_of(m);
            let no_body = m.child_by_field_name("body").is_none();
            let is_abstract = has_word(mods, "abstract", text)
                || (interface
                    && no_body
                    && !has_word(mods, "default", text)
                    && !has_word(mods, "static", text));
            let return_type = m.child_by_field_name("type").map(|t| {
                text_of(t, text)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            Some(MethodDecl {
                name: text_of(name, text).to_owned(),
                params,
                return_type,
                is_abstract,
                line: name.start_position().row,
                column: column(text, name),
                overridable: !has_word(mods, "private", text) && !has_word(mods, "static", text),
            })
        })
        .collect()
}

fn type_decl(n: Node, text: &str) -> Option<TypeDecl> {
    let kind = type_kind(n.kind())?;
    let name = n.child_by_field_name("name")?;
    let body = n.child_by_field_name("body")?;
    let interface = kind == TypeKind::Interface;
    Some(TypeDecl {
        name: text_of(name, text).to_owned(),
        kind,
        is_abstract: interface || has_word(modifiers_of(n), "abstract", text),
        line: name.start_position().row,
        column: column(text, name),
        methods: methods_of(body, text, interface),
    })
}

/// Every type declared in `text`, nested ones included, in document order.
pub fn types_in(text: &str) -> Vec<TypeDecl> {
    fn visit(n: Node, text: &str, out: &mut Vec<TypeDecl>) {
        if type_kind(n.kind()).is_some()
            && let Some(t) = type_decl(n, text)
        {
            out.push(t);
        }
        let mut cursor = n.walk();
        for child in n.named_children(&mut cursor) {
            visit(child, text, out);
        }
    }
    let mut out = Vec::new();
    if let Some(tree) = parse(Jvm::Java, text) {
        visit(tree.root_node(), text, &mut out);
    }
    out
}

/// Byte offset of 0-based `line` and `column` (characters) in `text`, clamped to the line.
pub fn offset_of(text: &str, line: usize, column: usize) -> usize {
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    text[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(i, _)| start + i)
}

/// The innermost type declaration around `offset` (its name, header or body).
pub fn type_at(text: &str, offset: usize) -> Option<TypeDecl> {
    let tree = parse(Jvm::Java, text)?;
    let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;
    loop {
        if type_kind(node.kind()).is_some() {
            return type_decl(node, text);
        }
        node = node.parent()?;
    }
}

/// The type whose name starts at 0-based `line` and `column` (a type hierarchy item's place).
pub fn type_named_at(text: &str, line: usize, column: usize) -> Option<TypeDecl> {
    type_at(text, offset_of(text, line, column))
}

/// The method whose name stands on 0-based `line`, named for a method breakpoint as
/// `pkg.Type.method`: the file's package and the simple name of the declaring type (`SPEC-0146` v6
/// behaviour 12). `None` on other lines; constructors are no methods here.
pub fn method_named_on_line(text: &str, line: usize) -> Option<String> {
    let types = types_in(text);
    let (owner, method) = types
        .iter()
        .find_map(|t| t.methods.iter().find(|m| m.line == line).map(|m| (t, m)))?;
    Some(match idecraft_core::source_roots::declared_package(text) {
        Some(pkg) => format!("{pkg}.{}.{}", owner.name, method.name),
        None => format!("{}.{}", owner.name, method.name),
    })
}

/// The method declaration around `offset` (name, parameters or body) and the type it is in.
pub fn method_at(text: &str, offset: usize) -> Option<(TypeDecl, MethodDecl)> {
    let tree = parse(Jvm::Java, text)?;
    let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;
    loop {
        if node.kind() == "method_declaration" {
            let name = node.child_by_field_name("name")?;
            let (line, col) = (name.start_position().row, column(text, name));
            let owner = type_at(text, node.start_byte())?;
            let method = owner
                .methods
                .iter()
                .find(|m| m.line == line && m.column == col)?
                .clone();
            return Some((owner, method));
        }
        if type_kind(node.kind()).is_some() {
            return None;
        }
        node = node.parent()?;
    }
}

// ------------------------------------------------------------ type hierarchy (language server)

/// Which part of a type hierarchy to show (`SPEC-0082` v5 behaviour 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeDirection {
    /// The chain of superclasses down to the type, and every subtype below it.
    Class,
    /// The supertypes (classes and interfaces), the type at the root.
    Supertypes,
    /// The subtypes, the type at the root.
    Subtypes,
}

/// At most this many nodes per hierarchy.
const TYPE_NODE_LIMIT: usize = 500;

pub(crate) fn item_node(o: &mut Opened, item: &Value) -> CallNode {
    let uri = item["uri"].as_str().unwrap_or_default().to_owned();
    let range = item
        .get("selectionRange")
        .and_then(|r| parse_range(r, &uri, &mut o.files))
        .unwrap_or(Range {
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 0,
        });
    CallNode {
        name: item["name"].as_str().unwrap_or_default().to_owned(),
        kind: kind_name(&SYMBOL_KINDS, item.get("kind")).unwrap_or_else(|| "class".into()),
        detail: item["detail"].as_str().map(str::to_owned),
        location: Location {
            path: uri_to_path(&uri).unwrap_or_else(|| PathBuf::from(&uri)),
            range,
        },
        call_sites: Vec::new(),
        children: Vec::new(),
        recursive: false,
        mark: None,
    }
}

pub(crate) fn item_key(item: &Value) -> String {
    format!(
        "{}#{}",
        item["uri"].as_str().unwrap_or_default(),
        item["name"].as_str().unwrap_or_default()
    )
}

/// The type hierarchy item of the type at the place, or of the type around it.
pub(crate) async fn prepare(
    o: &mut Opened,
    line: u64,
    character: u64,
) -> Result<Option<Value>, LspError> {
    let at = o.position(line, character);
    let items = o.ask("textDocument/prepareTypeHierarchy", at).await?;
    if let Some(item) = items.as_array().and_then(|a| a.first()) {
        return Ok(Some(item.clone()));
    }
    // Inside a body: ask at the name of the type around the place.
    let offset = offset_of(&o.text, line as usize, character as usize);
    let Some(t) = type_at(&o.text, offset) else {
        return Ok(None);
    };
    let at = o.position(t.line as u64, t.column as u64);
    let items = o.ask("textDocument/prepareTypeHierarchy", at).await?;
    Ok(items.as_array().and_then(|a| a.first()).cloned())
}

pub(crate) async fn related(o: &Opened, item: &Value, up: bool) -> Result<Vec<Value>, LspError> {
    let method = if up {
        "typeHierarchy/supertypes"
    } else {
        "typeHierarchy/subtypes"
    };
    let v = o.ask(method, json!({"item": item})).await?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

/// Builds the subtree of `item` going `up` (supertypes) or down (subtypes).
fn expand<'a>(
    o: &'a mut Opened,
    item: Value,
    up: bool,
    seen: &'a mut HashSet<String>,
    budget: &'a mut usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<CallNode, LspError>> + Send + 'a>> {
    Box::pin(async move {
        *budget = budget.saturating_sub(1);
        let mut node = item_node(o, &item);
        if !seen.insert(item_key(&item)) {
            node.recursive = true;
            return Ok(node);
        }
        if *budget == 0 {
            return Ok(node);
        }
        for next in related(o, &item, up).await? {
            if *budget == 0 {
                break;
            }
            let child = expand(o, next, up, seen, budget).await?;
            node.children.push(child);
        }
        Ok(node)
    })
}

/// The superclass of `item`: the first supertype that is a class, else none.
async fn superclass(o: &Opened, item: &Value) -> Result<Option<Value>, LspError> {
    let supers = related(o, item, true).await?;
    // Symbol kind 5 is a class (LSP); interfaces (11) are not part of the class chain.
    Ok(supers.into_iter().find(|s| s["kind"].as_u64() == Some(5)))
}

/// The type hierarchy of the type at (or around) a place of `file` (`SPEC-0082` v5 behaviour 13):
/// for [`TypeDirection::Class`] the root is the topmost superclass and the chain leads down to the
/// type (marked `*`) with all its subtypes below; the other directions start at the type. `None`
/// when no type is there.
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
    // Kotlin has no server hierarchy; the syntax trees of the project answer (behaviour 17).
    if crate::kotlin_structure::is_kotlin(file) {
        return Ok(crate::kotlin_structure::type_hierarchy_in(
            root, file, text, line, character, direction,
        ));
    }
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let Some(item) = prepare(&mut o, line, character).await? else {
            return Ok(None);
        };
        let mut seen = HashSet::new();
        let mut budget = TYPE_NODE_LIMIT;
        match direction {
            TypeDirection::Supertypes | TypeDirection::Subtypes => {
                let up = direction == TypeDirection::Supertypes;
                let mut tree = expand(&mut o, item, up, &mut seen, &mut budget).await?;
                tree.mark = Some("*".into());
                Ok(Some(tree))
            }
            TypeDirection::Class => {
                let mut tree = expand(&mut o, item.clone(), false, &mut seen, &mut budget).await?;
                tree.mark = Some("*".into());
                let mut at = item;
                let mut chain = HashSet::from([item_key(&at)]);
                while let Some(up) = superclass(&o, &at).await? {
                    if !chain.insert(item_key(&up)) {
                        break;
                    }
                    let mut parent = item_node(&mut o, &up);
                    parent.children.push(tree);
                    tree = parent;
                    at = up;
                }
                Ok(Some(tree))
            }
        }
    }
    .await;
    o.close().await;
    result
}

/// The text of the file of a hierarchy node: open editors' text from `texts`, else the disk.
fn file_text(path: &Path, texts: &dyn Fn(&Path) -> Option<String>) -> Option<String> {
    texts(path).or_else(|| std::fs::read_to_string(path).ok())
}

/// The type declared at a hierarchy node, read from the project's source.
pub(crate) fn node_type(n: &CallNode, texts: &dyn Fn(&Path) -> Option<String>) -> Option<TypeDecl> {
    let text = file_text(&n.location.path, texts)?;
    type_named_at(
        &text,
        n.location.range.start_line as usize,
        n.location.range.start_character as usize,
    )
}

/// Marks the method `name(params)` in every type of a subtype tree (`SPEC-0082` v5 behaviour 14):
/// `+` defined there, `!` not defined although the type is concrete and no supertype in the tree
/// defines it concretely, `-` inherited.
fn mark_methods(
    n: &mut CallNode,
    name: &str,
    params: &[String],
    inherited_body: bool,
    texts: &dyn Fn(&Path) -> Option<String>,
) {
    let t = node_type(n, texts);
    let own = t.as_ref().and_then(|t| t.method(name, params));
    let concrete_here = own.is_some_and(|m| !m.is_abstract);
    n.mark = Some(
        match (own, &t) {
            (Some(_), _) => "+",
            (None, Some(t)) if !t.is_abstract && !inherited_body => "!",
            _ => "-",
        }
        .into(),
    );
    let body = inherited_body || concrete_here;
    for c in &mut n.children {
        mark_methods(c, name, params, body, texts);
    }
}

/// The method hierarchy of the method at a place of `file` (`SPEC-0082` v5 behaviour 14): the
/// types from the topmost project supertype that declares the method down through all subtypes,
/// each marked `+`, `-` or `!`. `texts` gives the unsaved text of open files. `None` outside a
/// method.
#[allow(clippy::too_many_arguments)]
pub async fn method_hierarchy_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    texts: &(dyn Fn(&Path) -> Option<String> + Sync),
    wait: Duration,
) -> Result<Option<(String, CallNode)>, LspError> {
    if crate::kotlin_structure::is_kotlin(file) {
        return Ok(crate::kotlin_structure::method_hierarchy_in(
            root, file, text, line, character, texts,
        ));
    }
    let source = match text {
        Some(t) => t.to_owned(),
        None => std::fs::read_to_string(file).map_err(|e| LspError::Io(e.to_string()))?,
    };
    let offset = offset_of(&source, line as usize, character as usize);
    let Some((owner, method)) = method_at(&source, offset) else {
        return Ok(None);
    };
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let Some(mut item) = prepare(&mut o, owner.line as u64, owner.column as u64).await? else {
            return Ok(None);
        };
        // Up while a project supertype declares the method too.
        let mut seen = HashSet::from([item_key(&item)]);
        'up: loop {
            for s in related(&o, &item, true).await? {
                let n = item_node(&mut o, &s);
                if node_type(&n, texts)
                    .is_some_and(|t| t.method(&method.name, &method.params).is_some())
                    && seen.insert(item_key(&s))
                {
                    item = s;
                    continue 'up;
                }
            }
            break;
        }
        let mut seen = HashSet::new();
        let mut budget = TYPE_NODE_LIMIT;
        let mut tree = expand(&mut o, item, false, &mut seen, &mut budget).await?;
        mark_methods(&mut tree, &method.name, &method.params, false, texts);
        Ok(Some((method.name.clone(), tree)))
    }
    .await;
    o.close().await;
    result
}

/// What Go to Super Method found (`SPEC-0082` v5 behaviour 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuperTarget {
    /// Type and member, e.g. `Animal.sound()` or `Animal` for a superclass.
    pub name: String,
    /// Where it is declared; `None` for a type outside the project's sources.
    pub location: Option<Location>,
}

/// The method the method at a place overrides or implements, searched breadth first through the
/// project's supertypes; on a type outside a method, its superclass (`SPEC-0082` v5 behaviour 15).
/// `None` when there is nothing above.
pub async fn super_target_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    texts: &(dyn Fn(&Path) -> Option<String> + Sync),
    wait: Duration,
) -> Result<Option<SuperTarget>, LspError> {
    if crate::kotlin_structure::is_kotlin(file) {
        return Ok(crate::kotlin_structure::super_target_in(
            root, file, text, line, character, texts,
        ));
    }
    let source = match text {
        Some(t) => t.to_owned(),
        None => std::fs::read_to_string(file).map_err(|e| LspError::Io(e.to_string()))?,
    };
    let offset = offset_of(&source, line as usize, character as usize);
    let method = method_at(&source, offset);
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let (tl, tc) = match &method {
            Some((owner, _)) => (owner.line as u64, owner.column as u64),
            None => (line, character),
        };
        let Some(item) = prepare(&mut o, tl, tc).await? else {
            return Ok(None);
        };
        let Some((_, m)) = method else {
            // On the type: its superclass, else its first supertype.
            let supers = related(&o, &item, true).await?;
            let up = supers
                .iter()
                .find(|s| s["kind"].as_u64() == Some(5))
                .or(supers.first());
            return Ok(up.map(|s| {
                let n = item_node(&mut o, s);
                let local = n.location.path.is_file();
                SuperTarget {
                    name: n.name.clone(),
                    location: local.then_some(n.location),
                }
            }));
        };
        let mut queue = related(&o, &item, true).await?;
        let mut seen = HashSet::new();
        while !queue.is_empty() {
            let s = queue.remove(0);
            if !seen.insert(item_key(&s)) {
                continue;
            }
            let n = item_node(&mut o, &s);
            if let Some(t) = node_type(&n, texts)
                && let Some(found) = t.method(&m.name, &m.params)
            {
                let range = Range {
                    start_line: found.line as u64,
                    start_character: found.column as u64,
                    end_line: found.line as u64,
                    end_character: (found.column + found.name.chars().count()) as u64,
                };
                return Ok(Some(SuperTarget {
                    name: format!("{}.{}()", t.name, found.name),
                    location: Some(Location {
                        path: n.location.path,
                        range,
                    }),
                }));
            }
            queue.extend(related(&o, &s, true).await?);
        }
        Ok(None)
    }
    .await;
    o.close().await;
    result
}

// ------------------------------------------------- override and implement (jdtls, SPEC-0122 v3)

/// A method a class can override or must implement, as jdtls lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverridableMethod {
    /// Name.
    pub name: String,
    /// Parameter types.
    pub parameters: Vec<String>,
    /// Abstract and not implemented yet (Implement Methods preselects these).
    pub unimplemented: bool,
    /// The type that declares it, e.g. `shop.Animal`.
    pub declaring_class: String,
    /// `class` or `interface`.
    pub declaring_class_type: String,
    /// The return type, read from the declaring type's source in the project; `None` for types
    /// without a source there (jdtls names none, `SPEC-0122` v7 behaviour 6b).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// The server's item, sent back to add the method.
    #[serde(skip)]
    pub raw: Value,
}

impl OverridableMethod {
    /// The row label: `describe(): String` without return type when jdtls gives none. A Kotlin
    /// member brings its own (`owner: String` for a property).
    pub fn label(&self) -> String {
        if let Some(label) = self.raw["label"].as_str() {
            return label.to_owned();
        }
        let call = format!("{}({})", self.name, self.parameters.join(", "));
        match &self.return_type {
            Some(ty) => format!("{call}: {ty}"),
            None => call,
        }
    }

    /// A `default` method of a Java interface: listed by Implement Methods, not checked
    /// (`SPEC-0122` v7 behaviour 6a). Kotlin members (with their own stub) are never one.
    pub fn is_default_method(&self) -> bool {
        self.declaring_class_type == "interface"
            && !self.unimplemented
            && self.raw.get("kotlin").is_none()
    }
}

fn code_action_params(o: &Opened, line: u64, character: u64) -> Value {
    let p = o.position(line, character);
    json!({
        "textDocument": p["textDocument"],
        "range": {"start": p["position"], "end": p["position"]},
        "context": {"diagnostics": []},
    })
}

/// The methods the class at a place can override (`java/listOverridableMethods`): its type name
/// and the methods grouped by declaring type in the server's order.
pub async fn overridable_methods_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<(String, Vec<OverridableMethod>), LspError> {
    // Kotlin: from the syntax trees of the project (`SPEC-0122` v6 behaviour 15).
    if crate::kotlin_structure::is_kotlin(file) {
        return Ok(crate::kotlin_structure::overridable_methods_in(
            root, file, text, line, character,
        ));
    }
    let o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let v = o
            .ask(
                "java/listOverridableMethods",
                code_action_params(&o, line, character),
            )
            .await?;
        let (name, mut methods) = parse_overridable(&v);
        add_return_types(root, &o.text, file, &mut methods);
        Ok((name, methods))
    }
    .await;
    o.close().await;
    result
}

/// Folders that hold no project sources.
const SKIPPED: &[&str] = &["target", "build", "out", "bin", "node_modules"];

/// The `package` of a Java source, `""` for the default package.
fn package_of(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("package "))
        .map(|p| p.trim_end_matches(';').trim().to_owned())
        .unwrap_or_default()
}

/// Gives each method the return type its declaring type writes, when that type's source is in the
/// project: the file being edited (`text`, the editor's), else a `.java` file below `root` whose
/// package and type match the qualified name (`shop.Pet`; `shop.Outer.Inner` in `Outer.java`).
/// Library types keep none (`SPEC-0122` v7 behaviour 6b).
pub fn add_return_types(root: &Path, text: &str, file: &Path, methods: &mut [OverridableMethod]) {
    let mut sources: Vec<(String, String)> = vec![(package_of(text), text.to_owned())];
    let mut walked = false;
    let mut cache: std::collections::HashMap<String, Option<TypeDecl>> =
        std::collections::HashMap::new();
    for m in methods.iter_mut() {
        let class = m
            .declaring_class
            .split('<')
            .next()
            .unwrap_or_default()
            .to_owned();
        if !cache.contains_key(&class) {
            let mut found = find_type(&sources, &class);
            if found.is_none() && !walked {
                walked = true;
                sources.extend(java_sources(root, file));
                found = find_type(&sources, &class);
            }
            cache.insert(class.clone(), found);
        }
        if let Some(Some(decl)) = cache.get(&class) {
            m.return_type = decl
                .method(&m.name, &m.parameters)
                .and_then(|d| d.return_type.clone());
        }
    }
}

/// The type named by `qualified` among `sources` (package, text).
fn find_type(sources: &[(String, String)], qualified: &str) -> Option<TypeDecl> {
    let parts: Vec<&str> = qualified.split('.').collect();
    // `a.b.C` is `C` in package `a.b`; `a.b.Outer.Inner` is `Inner` in `Outer` of `a.b`.
    for split in (0..parts.len()).rev() {
        let package = parts[..split].join(".");
        let name = parts.last()?;
        for (p, text) in sources {
            if *p != package {
                continue;
            }
            let outer = parts.get(split).copied().unwrap_or(name);
            if !text.contains(outer) {
                continue;
            }
            if let Some(t) = types_in(text).into_iter().find(|t| t.name == *name) {
                return Some(t);
            }
        }
    }
    None
}

/// The project's Java sources other than `file`, at most 5000, without build output and hidden
/// folders.
fn java_sources(root: &Path, file: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    let skip = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut count = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                    dirs.push(path);
                }
            } else if name.ends_with(".java") && count < 5000 {
                count += 1;
                if path.canonicalize().is_ok_and(|p| p == skip) {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((package_of(&text), text));
                }
            }
        }
    }
    out
}

/// Reads jdtls's answer to `java/listOverridableMethods`.
pub fn parse_overridable(v: &Value) -> (String, Vec<OverridableMethod>) {
    let methods = v["methods"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| OverridableMethod {
            name: m["name"].as_str().unwrap_or_default().to_owned(),
            parameters: m["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(str::to_owned))
                .collect(),
            unimplemented: m["unimplemented"].as_bool().unwrap_or(false),
            declaring_class: m["declaringClass"].as_str().unwrap_or_default().to_owned(),
            declaring_class_type: m["declaringClassType"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            return_type: None,
            raw: m.clone(),
        })
        .collect();
    (v["type"].as_str().unwrap_or_default().to_owned(), methods)
}

/// Adds `methods` (items of [`overridable_methods_in`]) to the class at a place
/// (`java/addOverridableMethods`): the edits, with `@Override` and a default body.
pub async fn add_overridable_methods_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    methods: &[OverridableMethod],
    wait: Duration,
) -> Result<Vec<crate::rename::FileEdits>, LspError> {
    if crate::kotlin_structure::is_kotlin(file) {
        return crate::kotlin_structure::add_overridable_methods_in(
            root, file, text, line, character, methods,
        )
        .map_err(LspError::Protocol);
    }
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let raw: Vec<Value> = methods.iter().map(|m| m.raw.clone()).collect();
        let edit = o
            .ask(
                "java/addOverridableMethods",
                json!({
                    "context": code_action_params(&o, line, character),
                    "overridableMethods": raw,
                }),
            )
            .await?;
        if edit.is_null() {
            return Err(LspError::Protocol("the server added nothing".into()));
        }
        crate::rename::file_edits(&mut o, &[edit])
    }
    .await;
    o.close().await;
    result
}

// --------------------------------------------------------- Surround With (SPEC-0122 v3 beh. 7)

/// A statement wrapper of Surround With.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurroundKind {
    /// `if (…) { … }`
    If,
    /// `if (…) { … } else { }`
    IfElse,
    /// `while (…) { … }`
    While,
    /// `do { … } while (…);`
    DoWhile,
    /// `for (…) { … }`
    For,
    /// `try { … } catch (Exception e) { … }`
    TryCatch,
    /// `try { … } finally { }`
    TryFinally,
    /// `try { … } catch … finally { }`
    TryCatchFinally,
    /// `synchronized (…) { … }`
    Synchronized,
    /// `Runnable runnable = new Runnable() { public void run() { … } };`
    Runnable,
    /// `{ … }`
    Braces,
}

impl SurroundKind {
    /// All wrappers in menu order.
    pub const ALL: [SurroundKind; 11] = [
        Self::If,
        Self::IfElse,
        Self::While,
        Self::DoWhile,
        Self::For,
        Self::TryCatch,
        Self::TryFinally,
        Self::TryCatchFinally,
        Self::Synchronized,
        Self::Runnable,
        Self::Braces,
    ];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::If => "if",
            Self::IfElse => "if / else",
            Self::While => "while",
            Self::DoWhile => "do / while",
            Self::For => "for",
            Self::TryCatch => "try / catch",
            Self::TryFinally => "try / finally",
            Self::TryCatchFinally => "try / catch / finally",
            Self::Synchronized => "synchronized",
            Self::Runnable => "Runnable",
            Self::Braces => "{ }",
        }
    }

    /// The kind with `label` or its snake-case name (MCP).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| {
            k.label() == name
                || serde_json::to_value(k)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    == Some(name.to_owned())
        })
    }
}

/// Surround With as one replacement: `range` of the old text becomes `text`; the caret goes to
/// `caret` (a byte offset in the new file text), e.g. into the empty condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Surrounded {
    /// Replaced bytes of the old text (whole lines).
    pub range: Bytes<usize>,
    /// The new text of those lines.
    pub text: String,
    /// Caret in the new file text.
    pub caret: usize,
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// Whether `word` occurs as an identifier in `text`.
fn uses(text: &str, word: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(word).any(|(i, _)| {
        !text[..i].chars().next_back().is_some_and(is_ident)
            && !text[i + word.len()..].chars().next().is_some_and(is_ident)
    })
}

/// Wraps the whole lines of `selection` (bytes; an empty selection takes the caret's line) in
/// `kind` (`SPEC-0122` v3 behaviour 7). The lines are indented one step deeper. A local variable
/// declared in the lines and used after them in the same block is declared before the construct
/// and only assigned inside, so the code still compiles. Lines that are not whole statements of
/// one block are refused.
pub fn surround(
    text: &str,
    selection: Bytes<usize>,
    kind: SurroundKind,
) -> Result<Surrounded, String> {
    let sel_end = if selection.end > selection.start && text[..selection.end].ends_with('\n') {
        selection.end - 1
    } else {
        selection.end
    };
    let start = line_start(text, selection.start.min(text.len()));
    let end = line_end(text, sel_end.max(selection.start).min(text.len()));
    if text[start..end].trim().is_empty() {
        return Err("Select the statements to surround".into());
    }
    let tree = parse(Jvm::Java, text).ok_or("the file could not be parsed")?;
    let first = text[start..end]
        .find(|c: char| !c.is_whitespace())
        .map_or(start, |i| start + i);
    let last = text[start..end]
        .rfind(|c: char| !c.is_whitespace())
        .map_or(end, |i| start + i + 1);
    // The statements must be children of one block.
    let node = tree
        .root_node()
        .descendant_for_byte_range(first, last.saturating_sub(1).max(first))
        .ok_or("no statement selected")?;
    let mut block = node;
    while !(block.kind() == "block" && block.start_byte() < first && block.end_byte() > last) {
        block = block
            .parent()
            .ok_or("Surround With works on statements inside a method")?;
    }
    let statements: Vec<Node> = children(block)
        .into_iter()
        .filter(|s| s.is_named() && s.start_byte() < last && s.end_byte() > first)
        .collect();
    if statements.is_empty()
        || statements.first().is_some_and(|s| s.start_byte() < first)
        || statements.last().is_some_and(|s| s.end_byte() > last)
    {
        return Err("Select whole statements of one block".into());
    }
    let indent: String = text[start..first].to_owned();
    let unit = if indent.contains('\t') { "\t" } else { "    " };
    let after = &text[last..block.end_byte()];
    // Declarations used after the lines move before the construct.
    let mut hoisted = String::new();
    let mut body = String::new();
    let mut cursor = start;
    let mut removed = false;
    for s in &statements {
        if s.kind() != "local_variable_declaration" {
            continue;
        }
        let decls: Vec<Node> = {
            let mut c = s.walk();
            s.children_by_field_name("declarator", &mut c).collect()
        };
        let [d] = decls.as_slice() else {
            continue;
        };
        let Some(name) = d.child_by_field_name("name") else {
            continue;
        };
        let name = text_of(name, text);
        if !uses(after, name) {
            continue;
        }
        let ty = s
            .child_by_field_name("type")
            .map(|t| text_of(t, text))
            .unwrap_or("var");
        if ty == "var" {
            return Err(format!(
                "`{name}` is used after the selection; give it a type instead of `var` first"
            ));
        }
        let mods = modifiers_of(*s)
            .map(|m| text_of(m, text).replace("final", ""))
            .unwrap_or_default();
        let mods = mods.trim();
        hoisted.push_str(&format!(
            "{indent}{}{ty} {name};\n",
            if mods.is_empty() {
                String::new()
            } else {
                format!("{mods} ")
            }
        ));
        body.push_str(&text[cursor..s.start_byte()]);
        match d.child_by_field_name("value") {
            Some(v) => body.push_str(&format!("{name} = {};", text_of(v, text))),
            // Nothing is left of the statement; its blank line goes below.
            None => removed = true,
        }
        cursor = s.end_byte();
    }
    body.push_str(&text[cursor..end]);
    let lines: Vec<String> = body
        .split('\n')
        .filter(|l| !(removed && l.trim().is_empty()))
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{unit}{l}")
            }
        })
        .collect();
    let mut inner = lines.join("\n");
    if kind == SurroundKind::Runnable {
        inner = inner
            .split('\n')
            .map(|l| {
                if l.is_empty() {
                    String::new()
                } else {
                    format!("{unit}{l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let (i, u) = (&indent, unit);
    let (head, foot, caret_in_head): (String, String, Option<usize>) = match kind {
        SurroundKind::If => (format!("{i}if () {{"), format!("{i}}}"), Some(4)),
        SurroundKind::IfElse => (
            format!("{i}if () {{"),
            format!("{i}}} else {{\n{i}{u}\n{i}}}"),
            Some(4),
        ),
        SurroundKind::While => (format!("{i}while () {{"), format!("{i}}}"), Some(7)),
        SurroundKind::DoWhile => (format!("{i}do {{"), format!("{i}}} while ();"), None),
        SurroundKind::For => (format!("{i}for () {{"), format!("{i}}}"), Some(5)),
        SurroundKind::TryCatch => (
            format!("{i}try {{"),
            format!("{i}}} catch (Exception e) {{\n{i}{u}throw new RuntimeException(e);\n{i}}}"),
            None,
        ),
        SurroundKind::TryFinally => (
            format!("{i}try {{"),
            format!("{i}}} finally {{\n{i}{u}\n{i}}}"),
            None,
        ),
        SurroundKind::TryCatchFinally => (
            format!("{i}try {{"),
            format!(
                "{i}}} catch (Exception e) {{\n{i}{u}throw new RuntimeException(e);\n{i}}} finally {{\n{i}{u}\n{i}}}"
            ),
            None,
        ),
        SurroundKind::Synchronized => {
            (format!("{i}synchronized () {{"), format!("{i}}}"), Some(14))
        }
        SurroundKind::Runnable => (
            format!("{i}Runnable runnable = new Runnable() {{\n{i}{u}public void run() {{"),
            format!("{i}{u}}}\n{i}}};"),
            None,
        ),
        SurroundKind::Braces => (format!("{i}{{"), format!("{i}}}"), None),
    };
    // `inner` lines carry the original indentation plus one step.
    let new = format!("{hoisted}{head}\n{inner}\n{foot}");
    let caret_rel = match (kind, caret_in_head) {
        (_, Some(k)) => hoisted.len() + i.len() + k,
        (SurroundKind::DoWhile, _) => new.len() - 2,
        (SurroundKind::TryFinally, _) => new.len() - i.len() - 2,
        (SurroundKind::TryCatchFinally, _) => new.len() - i.len() - 2,
        (SurroundKind::TryCatch, _) => {
            hoisted.len() + head.len() + 1 + inner.len() + 1 + i.len() + "} catch (".len()
        }
        _ => hoisted.len() + head.len() + 1 + inner.len(),
    };
    Ok(Surrounded {
        range: start..end,
        text: new,
        caret: start + caret_rel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0146 v6 AK-v6-1.
    #[test]
    fn method_named_on_line() {
        let text = "package shop;\n\npublic class Cart {\n    Cart() {}\n    int total(int n) {\n        return n;\n    }\n    static class Line {\n        void add() {}\n    }\n}\n";
        assert_eq!(
            super::method_named_on_line(text, 4).as_deref(),
            Some("shop.Cart.total")
        );
        assert_eq!(
            super::method_named_on_line(text, 8).as_deref(),
            Some("shop.Line.add")
        );
        // A body line, a constructor, the class header.
        for line in [5, 3, 2] {
            assert_eq!(super::method_named_on_line(text, line), None, "{line}");
        }
        assert_eq!(
            super::method_named_on_line("class A { void f() {} }", 0).as_deref(),
            Some("A.f")
        );
    }

    const ZOO: &str = "package shop;\n\npublic abstract class Animal {\n    public abstract String sound();\n\n    public String describe() {\n        return \"x\" + sound();\n    }\n}\n\ninterface Pet {\n    String owner();\n    default boolean friendly() { return true; }\n}\n";

    #[test]
    fn types_and_methods_from_the_syntax_tree() {
        let t = type_named_at(ZOO, 2, 22).unwrap();
        assert_eq!(
            (t.name.as_str(), t.kind, t.is_abstract),
            ("Animal", TypeKind::Class, true)
        );
        let names: Vec<(&str, bool)> = t
            .methods
            .iter()
            .map(|m| (m.name.as_str(), m.is_abstract))
            .collect();
        assert_eq!(names, [("sound", true), ("describe", false)]);
        let pet = type_named_at(ZOO, 10, 10).unwrap();
        assert_eq!(pet.kind, TypeKind::Interface);
        let names: Vec<(&str, bool)> = pet
            .methods
            .iter()
            .map(|m| (m.name.as_str(), m.is_abstract))
            .collect();
        assert_eq!(names, [("owner", true), ("friendly", false)]);
        // A place inside a method body finds the method and its type.
        let (owner, m) = method_at(ZOO, offset_of(ZOO, 6, 20)).unwrap();
        assert_eq!(
            (owner.name.as_str(), m.name.as_str(), m.line, m.column),
            ("Animal", "describe", 5, 18)
        );
        assert!(method_at(ZOO, offset_of(ZOO, 2, 5)).is_none());
        assert!(t.method("describe", &[]).is_some());
        assert!(t.method("describe", &["int".into()]).is_none());
        assert!(same_params(
            &["java.util.List<String>".into()],
            &["List".into()]
        ));
    }

    #[test]
    fn overridable_methods_are_read() {
        let v = json!({"type": "Dog", "methods": [
            {"name": "owner", "parameters": [], "unimplemented": true, "declaringClass": "shop.Pet", "declaringClassType": "interface", "bindingKey": "k1"},
            {"name": "equals", "parameters": ["Object"], "unimplemented": false, "declaringClass": "java.lang.Object", "declaringClassType": "class"},
        ]});
        let (ty, ms) = parse_overridable(&v);
        assert_eq!(ty, "Dog");
        assert_eq!(ms.len(), 2);
        assert!(ms[0].unimplemented);
        assert_eq!(ms[1].label(), "equals(Object)");
        assert_eq!(ms[0].raw["bindingKey"], "k1");
    }

    #[test]
    fn method_marks_follow_the_types() {
        let dir = std::env::temp_dir().join(format!("idecraft-mh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, body: &str| {
            let p = dir.join(format!("{name}.java"));
            std::fs::write(&p, body).unwrap();
            p
        };
        let a = file("A", "abstract class A {\n    abstract String sound();\n}\n");
        let b = file(
            "B",
            "class B extends A {\n    String sound() { return \"b\"; }\n}\n",
        );
        let c = file("C", "class C extends B {\n}\n");
        let d = file("D", "abstract class D extends A {\n}\n");
        let e = file("E", "class E extends D {\n}\n");
        let node = |p: &Path, line: u64, col: u64, kids: Vec<CallNode>| CallNode {
            name: p.file_stem().unwrap().to_string_lossy().into_owned(),
            kind: "class".into(),
            detail: None,
            location: Location {
                path: p.to_path_buf(),
                range: Range {
                    start_line: line,
                    start_character: col,
                    end_line: line,
                    end_character: col + 1,
                },
            },
            call_sites: Vec::new(),
            children: kids,
            recursive: false,
            mark: None,
        };
        let mut tree = node(
            &a,
            0,
            15,
            vec![
                node(&b, 0, 6, vec![node(&c, 0, 6, vec![])]),
                node(&d, 0, 15, vec![node(&e, 0, 6, vec![])]),
            ],
        );
        mark_methods(&mut tree, "sound", &[], false, &|_| None);
        let marks = |n: &CallNode| n.mark.clone().unwrap();
        assert_eq!(marks(&tree), "+");
        assert_eq!(marks(&tree.children[0]), "+");
        assert_eq!(marks(&tree.children[0].children[0]), "-");
        assert_eq!(marks(&tree.children[1]), "-");
        assert_eq!(marks(&tree.children[1].children[0]), "!");
        std::fs::remove_dir_all(&dir).ok();
    }

    const MAIN: &str = "class M {\n    void m() {\n        int ratio = 7 / 2;\n        System.out.println(ratio);\n    }\n}\n";

    #[test]
    fn surround_with_if_keeps_a_used_declaration_outside() {
        let line = offset_of(MAIN, 2, 0)..offset_of(MAIN, 2, 0);
        let s = surround(MAIN, line, SurroundKind::If).unwrap();
        let mut out = MAIN.to_owned();
        out.replace_range(s.range.clone(), &s.text);
        assert_eq!(
            out,
            "class M {\n    void m() {\n        int ratio;\n        if () {\n            ratio = 7 / 2;\n        }\n        System.out.println(ratio);\n    }\n}\n"
        );
        assert_eq!(&out[s.caret - 4..s.caret], "if (");
    }

    #[test]
    fn surround_two_statements_with_try_catch_and_braces() {
        let sel = offset_of(MAIN, 2, 0)..offset_of(MAIN, 4, 0);
        let s = surround(MAIN, sel.clone(), SurroundKind::TryCatch).unwrap();
        let mut out = MAIN.to_owned();
        out.replace_range(s.range.clone(), &s.text);
        assert_eq!(
            out,
            "class M {\n    void m() {\n        try {\n            int ratio = 7 / 2;\n            System.out.println(ratio);\n        } catch (Exception e) {\n            throw new RuntimeException(e);\n        }\n    }\n}\n"
        );
        let s = surround(MAIN, sel, SurroundKind::Runnable).unwrap();
        let mut out = MAIN.to_owned();
        out.replace_range(s.range.clone(), &s.text);
        assert!(out.contains("        Runnable runnable = new Runnable() {\n            public void run() {\n                int ratio = 7 / 2;\n"), "{out}");
        assert!(out.contains("            }\n        };\n    }"), "{out}");
    }

    #[test]
    fn surround_refuses_what_is_not_a_statement() {
        let class_line = offset_of(MAIN, 1, 0)..offset_of(MAIN, 1, 0);
        assert!(surround(MAIN, class_line, SurroundKind::If).is_err());
        let half = offset_of(MAIN, 2, 0)..offset_of(MAIN, 5, 0);
        assert!(surround(MAIN, half, SurroundKind::Braces).is_err());
        for k in SurroundKind::ALL {
            assert_eq!(SurroundKind::parse(k.label()), Some(k));
        }
        assert_eq!(
            SurroundKind::parse("try_catch"),
            Some(SurroundKind::TryCatch)
        );
    }

    /// SPEC-0122 v7 AK (behaviour 6b): return types from the declaring types' sources in the
    /// project — the open file's text first, other files by package and name, a nested type in its
    /// outer file; a library type keeps none.
    #[test]
    fn return_types_come_from_the_project_sources() {
        let root = std::env::temp_dir().join(format!("idecraft-java-ret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/shop")).unwrap();
        std::fs::create_dir_all(root.join("target/classes/shop")).unwrap();
        std::fs::write(
            root.join("src/shop/Pet.java"),
            "package shop;\n\npublic interface Pet {\n    String owner();\n    default java.util.List<String> friends(int max) { return null; }\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/shop/Outer.java"),
            "package shop;\n\npublic class Outer {\n    public static abstract class Inner {\n        abstract int size();\n    }\n}\n",
        )
        .unwrap();
        // Build output is no source.
        std::fs::write(
            root.join("target/classes/shop/Base.java"),
            "package shop;\nclass Base { long id() { return 0; } }\n",
        )
        .unwrap();
        let open = "package shop;\n\nabstract class Animal {\n    abstract   void\n  sound();\n}\n\nclass Dog extends Animal implements Pet {}\n";
        let file = root.join("src/shop/Dog.java");
        let m = |name: &str, params: &[&str], class: &str| OverridableMethod {
            name: name.into(),
            parameters: params.iter().map(|p| (*p).to_owned()).collect(),
            unimplemented: false,
            declaring_class: class.into(),
            declaring_class_type: "class".into(),
            return_type: None,
            raw: Value::Null,
        };
        let mut methods = vec![
            m("sound", &[], "shop.Animal"),
            m("owner", &[], "shop.Pet"),
            m("friends", &["int"], "shop.Pet"),
            m("size", &[], "shop.Outer.Inner"),
            m("id", &[], "shop.Base"),
            m("toString", &[], "java.lang.Object"),
            m("friends", &["String"], "shop.Pet"),
        ];
        add_return_types(&root, open, &file, &mut methods);
        let types: Vec<Option<&str>> = methods.iter().map(|m| m.return_type.as_deref()).collect();
        assert_eq!(
            types,
            [
                Some("void"),
                Some("String"),
                Some("java.util.List<String>"),
                Some("int"),
                None,
                None,
                None
            ]
        );
        assert_eq!(methods[2].label(), "friends(int): java.util.List<String>");
        assert_eq!(methods[5].label(), "toString()");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0122 v7 behaviour 6a: which items are default methods of a Java interface.
    #[test]
    fn default_methods_of_java_interfaces() {
        let mut d = OverridableMethod {
            name: "friendly".into(),
            parameters: Vec::new(),
            unimplemented: false,
            declaring_class: "shop.Pet".into(),
            declaring_class_type: "interface".into(),
            return_type: None,
            raw: Value::Null,
        };
        assert!(d.is_default_method());
        d.unimplemented = true;
        assert!(!d.is_default_method(), "still to implement");
        d.unimplemented = false;
        d.raw = serde_json::json!({"label": "owner: String", "kotlin": "override val owner"});
        assert!(!d.is_default_method(), "a Kotlin member");
    }
}
