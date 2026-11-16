//! The class a Python member comes from (`SPEC-0072` v10 behaviour 44). pyright and basedpyright
//! do not name it with their completion items, so it is worked out from the server's answers
//! (the type definition of the receiver, the definition of each base class) and from what the
//! classes declare in their sources and stubs, in method resolution order. Where the hierarchy
//! cannot be read, no origin is given.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use tree_sitter::Node;

use crate::LanguageId;
use crate::insight::{CompletionItem, Opened, line_of, parse_locations, to_server};
use crate::lsp::{LspError, file_uri};
use crate::syntax::SyntaxDocument;

/// At most this many classes are read for one hierarchy.
const CLASS_LIMIT: usize = 32;
/// Sources and stubs larger than this are not read.
const MAX_CLASS_FILE_BYTES: u64 = 4 << 20;

/// Methods a class decorator such as `@dataclass` may generate: below a decorated class they get
/// no origin from the classes above it.
const GENERATED: &[&str] = &[
    "__init__",
    "__repr__",
    "__eq__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__hash__",
    "__setattr__",
    "__delattr__",
    "__replace__",
    "__match_args__",
];

/// Bases of `typing` that declare no members of their own.
const EMPTY_BASES: &[&str] = &["Generic", "Protocol"];

/// The character column of the last character of the receiver of a member access: for the line
/// `    item.to` with the caret at `character` 11 that is 7. `None` when the name being completed
/// does not follow `name.`.
pub fn receiver_end(line: &str, character: usize) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut at = character.min(chars.len());
    while at > 0 && ident(chars[at - 1]) {
        at -= 1;
    }
    (at >= 2 && chars[at - 1] == '.' && ident(chars[at - 2])).then(|| at - 2)
}

/// A base class as written in a class statement: the place of its (last) name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// The name, e.g. `Sequence` for `typing.Sequence[str]`.
    pub name: String,
    /// 0-based line of the name.
    pub line: u64,
    /// 0-based character of the name.
    pub character: u64,
}

/// What a `class` statement declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassDecl {
    /// The class name.
    pub name: String,
    /// Methods, nested classes, class-level names and attributes assigned through `self`.
    pub members: HashSet<String>,
    /// The written bases in order; `None` for a base that is no name (a call, a string).
    pub bases: Vec<Option<Base>>,
    /// Whether the class has a decorator.
    pub decorated: bool,
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

/// Byte offset of 0-based `line` and `character` in `text`.
fn byte_at(text: &str, line: u64, character: u64) -> Option<usize> {
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n')? + 1;
    }
    let rest = text[start..].split('\n').next().unwrap_or_default();
    Some(
        start
            + rest
                .char_indices()
                .nth(character as usize)
                .map_or(rest.len(), |(i, _)| i),
    )
}

fn column(n: Node, text: &str) -> u64 {
    let start = n.start_byte();
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..start].chars().count() as u64
}

/// Names a target declares: identifiers, also in tuples.
fn target_names(n: Node, text: &str, out: &mut HashSet<String>) {
    match n.kind() {
        "identifier" => {
            out.insert(word(n, text).to_owned());
        }
        "pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" => {
            for c in children(n) {
                target_names(c, text, out);
            }
        }
        _ => {}
    }
}

/// Attributes assigned through `receiver` (`self.x = …`, `self.x: int = …`) below `n`, without
/// entering nested functions and classes.
fn self_attributes(n: Node, text: &str, receiver: &str, out: &mut HashSet<String>) {
    for c in children(n) {
        match c.kind() {
            "function_definition" | "class_definition" | "lambda" => {}
            "assignment" => {
                if let Some(left) = c.child_by_field_name("left") {
                    let mut lefts = vec![left];
                    while let Some(l) = lefts.pop() {
                        match l.kind() {
                            "attribute" => {
                                let object = l.child_by_field_name("object");
                                let attribute = l.child_by_field_name("attribute");
                                if let (Some(o), Some(a)) = (object, attribute)
                                    && o.kind() == "identifier"
                                    && word(o, text) == receiver
                                {
                                    out.insert(word(a, text).to_owned());
                                }
                            }
                            "pattern_list" | "tuple_pattern" | "list_pattern" => {
                                lefts.extend(children(l));
                            }
                            _ => {}
                        }
                    }
                }
                self_attributes(c, text, receiver, out);
            }
            _ => self_attributes(c, text, receiver, out),
        }
    }
}

/// The name of the first parameter of a function, e.g. `self`.
fn first_parameter<'a>(function: Node, text: &'a str) -> Option<&'a str> {
    let list = function.child_by_field_name("parameters")?;
    let first = children(list).into_iter().find(|c| c.is_named())?;
    match first.kind() {
        "identifier" => Some(word(first, text)),
        "typed_parameter" => children(first)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|c| word(c, text)),
        _ => None,
    }
}

/// What the class body below `n` declares, also inside `if` and `try` blocks.
fn body_members(n: Node, text: &str, out: &mut HashSet<String>) {
    for c in children(n) {
        match c.kind() {
            "function_definition" => {
                if let Some(name) = c.child_by_field_name("name") {
                    out.insert(word(name, text).to_owned());
                }
                let is_static = c.parent().is_some_and(|p| {
                    p.kind() == "decorated_definition"
                        && children(p).iter().any(|d| {
                            d.kind() == "decorator"
                                && matches!(
                                    word(*d, text).trim_start_matches('@').trim(),
                                    "staticmethod" | "classmethod"
                                )
                        })
                });
                if !is_static
                    && let (Some(receiver), Some(body)) =
                        (first_parameter(c, text), c.child_by_field_name("body"))
                {
                    self_attributes(body, text, receiver, out);
                }
            }
            "class_definition" => {
                if let Some(name) = c.child_by_field_name("name") {
                    out.insert(word(name, text).to_owned());
                }
            }
            "lambda" => {}
            "assignment" => {
                if let Some(left) = c.child_by_field_name("left") {
                    target_names(left, text, out);
                }
            }
            _ => body_members(c, text, out),
        }
    }
}

/// The base a superclass expression names: a name, a dotted name, or one with type arguments.
fn base_of(n: Node, text: &str) -> Option<Base> {
    match n.kind() {
        "identifier" => Some(Base {
            name: word(n, text).to_owned(),
            line: n.start_position().row as u64,
            character: column(n, text),
        }),
        "attribute" => base_of(n.child_by_field_name("attribute")?, text),
        "subscript" => base_of(n.child_by_field_name("value")?, text),
        _ => None,
    }
}

/// The `class` statement whose name is at 0-based `line` and `character` of `text`, with what it
/// declares; `None` when there is no class name at that place.
pub fn class_at(text: &str, line: u64, character: u64) -> Option<ClassDecl> {
    let doc = SyntaxDocument::new(LanguageId::Python, false, text).ok()?;
    let at = byte_at(text, line, character)?;
    let name = doc.tree().root_node().descendant_for_byte_range(at, at)?;
    let class = name.parent()?;
    if name.kind() != "identifier"
        || class.kind() != "class_definition"
        || class.child_by_field_name("name") != Some(name)
    {
        return None;
    }
    let mut members = HashSet::new();
    if let Some(body) = class.child_by_field_name("body") {
        body_members(body, text, &mut members);
    }
    let bases = class
        .child_by_field_name("superclasses")
        .map(|list| {
            children(list)
                .into_iter()
                .filter(|c| c.is_named() && !matches!(c.kind(), "keyword_argument" | "comment"))
                .map(|c| base_of(c, text))
                .collect()
        })
        .unwrap_or_default();
    Some(ClassDecl {
        name: word(name, text).to_owned(),
        members,
        bases,
        decorated: class
            .parent()
            .is_some_and(|p| p.kind() == "decorated_definition"),
    })
}

/// One class of a hierarchy: its declaration and where its bases are in the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The class.
    pub decl: ClassDecl,
    /// Its bases as indexes into the hierarchy (the implicit `object` included); `None` for a
    /// base that could not be read.
    pub bases: Vec<Option<usize>>,
}

/// The classes a receiver's members can come from; index 0 is the receiver's class.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hierarchy {
    /// The classes read.
    pub classes: Vec<Entry>,
}

impl Hierarchy {
    /// Whether every class reachable from `from` could be read.
    fn complete(&self, from: usize) -> bool {
        let mut seen = HashSet::new();
        let mut todo = vec![from];
        while let Some(i) = todo.pop() {
            if !seen.insert(i) {
                continue;
            }
            for b in &self.classes[i].bases {
                match b {
                    Some(b) => todo.push(*b),
                    None => return false,
                }
            }
        }
        true
    }

    /// The C3 linearisation of the class `of`; `None` when the bases have no consistent order
    /// or refer to each other in a circle.
    fn c3(&self, of: usize, depth: usize) -> Option<Vec<usize>> {
        if depth > self.classes.len() {
            return None;
        }
        let bases: Vec<usize> = self.classes[of].bases.iter().flatten().copied().collect();
        let mut lists: Vec<Vec<usize>> = Vec::new();
        for b in &bases {
            lists.push(self.c3(*b, depth + 1)?);
        }
        lists.push(bases);
        let mut out = vec![of];
        loop {
            lists.retain(|l| !l.is_empty());
            if lists.is_empty() {
                return Some(out);
            }
            let head = lists
                .iter()
                .map(|l| l[0])
                .find(|h| !lists.iter().any(|l| l[1..].contains(h)))?;
            out.push(head);
            for l in &mut lists {
                if l[0] == head {
                    l.remove(0);
                }
            }
        }
    }

    /// The class that declares the member `name`: the first in method resolution order
    /// (behaviour 44). `None` when no class read declares it or the search meets a base that
    /// could not be read first.
    pub fn origin(&self, name: &str) -> Option<&str> {
        let mut at = 0;
        for _ in 0..=self.classes.len() {
            let class = self.classes.get(at)?;
            if class.decl.members.contains(name) {
                return Some(&class.decl.name);
            }
            if class.decl.decorated && GENERATED.contains(&name) {
                return None;
            }
            match class.bases.as_slice() {
                [Some(base)] => at = *base,
                [] | [None] => return None,
                _ => {
                    if !self.complete(at) {
                        return None;
                    }
                    for i in self.c3(at, 0)?.into_iter().skip(1) {
                        let class = &self.classes[i];
                        if class.decl.members.contains(name) {
                            return Some(&class.decl.name);
                        }
                        if class.decl.decorated && GENERATED.contains(&name) {
                            return None;
                        }
                    }
                    return None;
                }
            }
        }
        None
    }
}

/// `class object` of the typeshed stubs beside the Python server `command`: the stub's path, its
/// text and the place of the name.
fn object_stub(command: &Path) -> Option<(PathBuf, String, u64, u64)> {
    let path = crate::python_completion::typeshed_beside(command)?.join("builtins.pyi");
    let text = std::fs::read_to_string(&path).ok()?;
    let line = text
        .lines()
        .position(|l| l.starts_with("class object:") || l.starts_with("class object("))?;
    Some((path, text, line as u64, 6))
}

fn class_file_text(path: &Path, opened: &Path, opened_text: &str) -> Option<String> {
    if path == opened {
        return Some(opened_text.to_owned());
    }
    if std::fs::metadata(path).map_or(true, |m| m.len() > MAX_CLASS_FILE_BYTES) {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The class hierarchy of the receiver of the member access at `line` and `character` (the caret,
/// after the name typed so far) of the Python `file` whose content is `text`, from pyright or
/// basedpyright (behaviour 44). `None` when there is no member access after a name, the server
/// is another one, or the receiver has not exactly one class. All questions together take at
/// most `wait`.
pub async fn member_origins_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Option<Hierarchy>, LspError> {
    if LanguageId::detect(file) != Some(LanguageId::Python) {
        return Ok(None);
    }
    let server = crate::servers::resolve_for(LanguageId::Python, root)?;
    if !crate::python_server::is_python_server(server.candidate.program) {
        return Ok(None);
    }
    let deadline = tokio::time::Instant::now() + wait;
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let found = hierarchy(&mut o, file, &server.command, line, character, deadline).await;
    o.close().await;
    Ok(found)
}

async fn hierarchy(
    o: &mut Opened,
    file: &Path,
    command: &Path,
    line: u64,
    character: u64,
    deadline: tokio::time::Instant,
) -> Option<Hierarchy> {
    let left = || deadline.saturating_duration_since(tokio::time::Instant::now());
    let column = receiver_end(line_of(&o.text, line), character as usize)?;
    let answer = o
        .ask_bounded(
            "textDocument/typeDefinition",
            o.position(line, column as u64),
            left(),
            None,
        )
        .await
        .ok()?;
    let mut places = parse_locations(&answer, &mut o.files);
    places.dedup();
    let [place] = places.as_slice() else {
        return None;
    };
    let opened_text = o.text.clone();
    let object = object_stub(command);
    // (file, line, character of the class name) → index.
    let mut known: HashMap<(PathBuf, u64, u64), usize> = HashMap::new();
    // Per class: its file and text, for the questions about its bases.
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    let mut out = Hierarchy::default();
    let first_text = class_file_text(&place.path, file, &opened_text)?;
    let first = class_at(
        &first_text,
        place.range.start_line,
        place.range.start_character,
    )?;
    known.insert(
        (
            place.path.clone(),
            place.range.start_line,
            place.range.start_character,
        ),
        0,
    );
    out.classes.push(Entry {
        decl: first,
        bases: Vec::new(),
    });
    sources.push((place.path.clone(), first_text));
    let mut next = 0;
    while next < out.classes.len() {
        let (path, text) = sources[next].clone();
        let written = out.classes[next].decl.bases.clone();
        let mut bases: Vec<Option<usize>> = Vec::new();
        for base in written {
            // (file, text, line, character) of the base's class statement, if it can be read.
            let mut target = None;
            let mut empty = false;
            if let Some(base) = &base {
                let units = to_server(line_of(&text, base.line), base.character, o.files.encoding);
                let params = json!({
                    "textDocument": {"uri": file_uri(&path)},
                    "position": {"line": base.line, "character": units},
                });
                if let Ok(v) = o
                    .ask_bounded("textDocument/definition", params, left(), None)
                    .await
                {
                    let mut places = parse_locations(&v, &mut o.files);
                    places.dedup();
                    if let [p] = places.as_slice()
                        && let Some(t) = class_file_text(&p.path, file, &opened_text)
                    {
                        if class_at(&t, p.range.start_line, p.range.start_character).is_some() {
                            target = Some((
                                p.path.clone(),
                                t,
                                p.range.start_line,
                                p.range.start_character,
                            ));
                        } else {
                            empty = EMPTY_BASES.contains(&base.name.as_str());
                        }
                    }
                }
            }
            if empty {
                continue;
            }
            bases.push(target.and_then(|t| add_class(&mut out, &mut known, &mut sources, t)));
        }
        let is_object = out.classes[next].decl.name == "object"
            && path.file_name().is_some_and(|n| n == "builtins.pyi");
        if bases.is_empty() && !is_object {
            // A class without bases derives from `object`.
            bases.push(
                object
                    .clone()
                    .and_then(|t| add_class(&mut out, &mut known, &mut sources, t)),
            );
        }
        out.classes[next].bases = bases;
        next += 1;
    }
    Some(out)
}

/// The index of the class at `target` (file, text, line, character) in `out`, read and added when
/// new; `None` when it is no class or the limit is reached.
fn add_class(
    out: &mut Hierarchy,
    known: &mut HashMap<(PathBuf, u64, u64), usize>,
    sources: &mut Vec<(PathBuf, String)>,
    target: (PathBuf, String, u64, u64),
) -> Option<usize> {
    let (path, text, line, character) = target;
    let key = (path.clone(), line, character);
    if let Some(i) = known.get(&key) {
        return Some(*i);
    }
    if out.classes.len() >= CLASS_LIMIT {
        return None;
    }
    let decl = class_at(&text, line, character)?;
    out.classes.push(Entry {
        decl,
        bases: Vec::new(),
    });
    sources.push((path, text));
    known.insert(key, out.classes.len() - 1);
    Some(out.classes.len() - 1)
}

/// Whether `item` is one of the server's own members, which can have an origin: not a template,
/// a keyword or a name IDECraft added.
pub fn takes_origin(item: &CompletionItem) -> bool {
    item.raw.is_some() && !matches!(item.kind.as_deref(), Some("snippet" | "keyword" | "text"))
}

/// Sets [`CompletionItem::origin`] of the server's `items` from `hierarchy`.
pub fn set_origins(hierarchy: &Hierarchy, items: &mut [CompletionItem]) {
    for item in items.iter_mut().filter(|i| takes_origin(i)) {
        item.origin = hierarchy.origin(&item.label).map(str::to_owned);
    }
}

/// Adds the origin to the completion `items` asked for at `line` and `character` of `file`
/// (behaviour 46); a list that has none stays as it is.
pub async fn add_origins_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    items: &mut [CompletionItem],
    wait: Duration,
) -> Result<(), LspError> {
    if let Some(h) = member_origins_in(root, file, text, line, character, wait).await? {
        set_origins(&h, items);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_before_the_dot() {
        assert_eq!(receiver_end("    item.", 9), Some(7));
        assert_eq!(receiver_end("    item.to", 11), Some(7));
        assert_eq!(receiver_end("x = self.items.na", 17), Some(13));
        assert_eq!(receiver_end("    f().", 8), None);
        assert_eq!(receiver_end("    xs[0].a", 11), None);
        assert_eq!(receiver_end("    \"\".jo", 9), None);
        assert_eq!(receiver_end("    item", 8), None);
        assert_eq!(receiver_end(".x", 2), None);
        assert_eq!(receiver_end("    größe.", 10), Some(8));
    }

    const SOURCE: &str = "\
import typing
from dataclasses import dataclass


@dataclass
class Item(Base, typing.Sequence[str], metaclass=Meta):
    name: str
    qty: int = 1
    a, b = 1, 2
    if True:
        late = 3
    else:
        other = 4

    class Inner:
        hidden = 1

    def total(self) -> float:
        self.cached = 1.0
        self.pair, self.rest = 1, 2
        local = 2

        def helper(self):
            self.not_mine = 1

        return self.cached

    @staticmethod
    def make(x):
        x.nope = 1

    async def load(this: \"Item\"):
        this.loaded: bool = True


class Plain:
    pass


class Odd(make_base()):
    pass
";

    #[test]
    fn what_a_class_declares() {
        let item = class_at(SOURCE, 5, 6).expect("class Item");
        assert_eq!(item.name, "Item");
        assert!(item.decorated);
        let mut members: Vec<&str> = item.members.iter().map(String::as_str).collect();
        members.sort_unstable();
        assert_eq!(
            members,
            [
                "Inner", "a", "b", "cached", "late", "load", "loaded", "make", "name", "other",
                "pair", "qty", "rest", "total"
            ]
        );
        assert_eq!(
            item.bases,
            [
                Some(Base {
                    name: "Base".into(),
                    line: 5,
                    character: 11
                }),
                Some(Base {
                    name: "Sequence".into(),
                    line: 5,
                    character: 24
                }),
            ]
        );
        let plain = class_at(SOURCE, 35, 8).expect("class Plain");
        assert!(!plain.decorated && plain.bases.is_empty() && plain.members.is_empty());
        assert_eq!(class_at(SOURCE, 39, 6).expect("class Odd").bases, [None]);
        // No class name at these places: a method name, a base, the keyword.
        assert_eq!(class_at(SOURCE, 17, 8), None);
        assert_eq!(class_at(SOURCE, 5, 12), None);
        assert_eq!(class_at(SOURCE, 5, 2), None);
    }

    fn class(name: &str, members: &[&str], bases: &[Option<usize>]) -> Entry {
        Entry {
            decl: ClassDecl {
                name: name.to_owned(),
                members: members.iter().map(|m| (*m).to_owned()).collect(),
                bases: Vec::new(),
                decorated: false,
            },
            bases: bases.to_vec(),
        }
    }

    #[test]
    fn origin_along_a_chain() {
        let h = Hierarchy {
            classes: vec![
                class("C", &["own", "shared"], &[Some(1)]),
                class("B", &["shared", "mid"], &[Some(2)]),
                class("object", &["__class__", "__eq__"], &[]),
            ],
        };
        assert_eq!(h.origin("own"), Some("C"));
        assert_eq!(h.origin("shared"), Some("C"));
        assert_eq!(h.origin("mid"), Some("B"));
        assert_eq!(h.origin("__class__"), Some("object"));
        assert_eq!(h.origin("added_at_run_time"), None);
    }

    #[test]
    fn origin_stops_at_a_base_that_could_not_be_read() {
        let h = Hierarchy {
            classes: vec![
                class("C", &["own"], &[Some(1)]),
                class("B", &["mid"], &[None]),
            ],
        };
        assert_eq!(h.origin("own"), Some("C"));
        assert_eq!(h.origin("mid"), Some("B"));
        assert_eq!(h.origin("__class__"), None);
        // With several bases nothing above the class is trusted.
        let h = Hierarchy {
            classes: vec![
                class("C", &["own"], &[Some(1), None]),
                class("A", &["a"], &[Some(2)]),
                class("object", &["__class__"], &[]),
            ],
        };
        assert_eq!(h.origin("own"), Some("C"));
        assert_eq!(h.origin("a"), None);
        assert_eq!(h.origin("__class__"), None);
    }

    #[test]
    fn several_bases_in_c3_order() {
        // class D(B, C); class B(A); class C(A); class A — `who` is declared by C and A, and C
        // comes before A although A is reached first through B.
        let h = Hierarchy {
            classes: vec![
                class("D", &[], &[Some(1), Some(2)]),
                class("B", &["b"], &[Some(3)]),
                class("C", &["who", "c"], &[Some(3)]),
                class("A", &["who", "a"], &[Some(4)]),
                class("object", &["__class__"], &[]),
            ],
        };
        assert_eq!(h.c3(0, 0), Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(h.origin("who"), Some("C"));
        assert_eq!(h.origin("a"), Some("A"));
        assert_eq!(h.origin("b"), Some("B"));
        assert_eq!(h.origin("__class__"), Some("object"));
        // An order that cannot exist: class X(A, B) with B before A elsewhere.
        let h = Hierarchy {
            classes: vec![
                class("X", &[], &[Some(1), Some(2)]),
                class("P", &["p"], &[Some(3), Some(4)]),
                class("Q", &["q"], &[Some(4), Some(3)]),
                class("A", &[], &[]),
                class("B", &[], &[]),
            ],
        };
        assert_eq!(h.c3(0, 0), None);
        assert_eq!(h.origin("p"), None);
    }

    #[test]
    fn generated_methods_of_a_decorated_class() {
        let mut item = class("Item", &["name", "__repr__"], &[Some(1)]);
        item.decl.decorated = true;
        let h = Hierarchy {
            classes: vec![
                item,
                class(
                    "object",
                    &["__eq__", "__repr__", "__class__", "__init__"],
                    &[],
                ),
            ],
        };
        assert_eq!(h.origin("__eq__"), None);
        assert_eq!(h.origin("__init__"), None);
        assert_eq!(h.origin("__repr__"), Some("Item"));
        assert_eq!(h.origin("__class__"), Some("object"));
    }

    #[test]
    fn only_server_items_take_an_origin() {
        let h = Hierarchy {
            classes: vec![class("Item", &["name", "if"], &[])],
        };
        let raw = Some(json!({"label": "x"}));
        let mut items = vec![
            CompletionItem {
                label: "name".into(),
                kind: Some("variable".into()),
                raw: raw.clone(),
                ..Default::default()
            },
            CompletionItem {
                label: "if".into(),
                kind: Some("snippet".into()),
                raw: raw.clone(),
                ..Default::default()
            },
            CompletionItem {
                label: "name".into(),
                ..Default::default()
            },
            CompletionItem {
                label: "unknown".into(),
                raw,
                ..Default::default()
            },
        ];
        set_origins(&h, &mut items);
        let origins: Vec<Option<&str>> = items.iter().map(|i| i.origin.as_deref()).collect();
        assert_eq!(origins, [Some("Item"), None, None, None]);
    }

    #[test]
    fn a_cycle_of_bases_ends() {
        let h = Hierarchy {
            classes: vec![class("A", &[], &[Some(1)]), class("B", &[], &[Some(0)])],
        };
        assert_eq!(h.origin("x"), None);
        let h = Hierarchy {
            classes: vec![
                class("A", &[], &[Some(1), Some(1)]),
                class("B", &[], &[Some(0)]),
            ],
        };
        assert_eq!(h.origin("x"), None);
    }
}
