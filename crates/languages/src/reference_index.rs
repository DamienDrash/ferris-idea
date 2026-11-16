//! The project-wide reference index of Java names and the global inspection
//! `java.unused-declaration` (`SPEC-0316`): declarations and name uses per file from IDECraft's
//! own syntax trees, refreshed incrementally by modification time and size, kept in memory and in
//! the user cache folder. `SPEC-0316` v3 behaviour 8 / `SPEC-0098` v12 behaviour 26: the editor's
//! step for one file's unsaved text against the totals kept with the in-memory index.
//! `SPEC-0316` v4 behaviours 9 and 10 / `SPEC-0098` v13 behaviour 27: Kotlin declarations from
//! the Kotlin syntax tree, uses across Java and Kotlin both ways (also through the JVM accessor
//! names of properties), the Kotlin entry points and `kotlin.unused-declaration`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::LanguageId;
use crate::inspections::{Finding, Fix, Level, Placed};
use crate::syntax::SyntaxDocument;

/// Version of the saved form; a saved index of another version is ignored (behaviour 2; v3
/// behaviour 8: format 2 adds the visibility; v4 behaviour 9: format 3, Kotlin declarations).
pub const FORMAT: u32 = 3;

/// The inspection id of unused declarations (behaviour 5).
pub const UNUSED_DECLARATION: &str = "java.unused-declaration";

/// The inspection id of unused Kotlin declarations (v4 behaviour 9).
pub const KOTLIN_UNUSED_DECLARATION: &str = "kotlin.unused-declaration";

/// The inspection id of the unused declarations of the file `rel`: Kotlin's for a `.kt` file.
pub fn unused_id(rel: &Path) -> &'static str {
    if is_kotlin(rel) {
        KOTLIN_UNUSED_DECLARATION
    } else {
        UNUSED_DECLARATION
    }
}

/// Files larger than this are not indexed (behaviour 1).
const MAX_FILE_BYTES: u64 = 1 << 20;

/// What a declaration declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A class.
    Class,
    /// An interface.
    Interface,
    /// An enum.
    Enum,
    /// A record.
    Record,
    /// An annotation type.
    AnnotationType,
    /// A method.
    Method,
    /// A field.
    Field,
    /// A Kotlin function (v4 behaviour 9).
    Function,
    /// A Kotlin property.
    Property,
    /// A Kotlin named object.
    Object,
    /// A Kotlin enum class.
    EnumClass,
    /// A Kotlin annotation class.
    AnnotationClass,
}

impl Kind {
    /// The word of the message: `Class`, `Interface`, …
    fn word(self) -> &'static str {
        match self {
            Kind::Class => "Class",
            Kind::Interface => "Interface",
            Kind::Enum => "Enum",
            Kind::Record => "Record",
            Kind::AnnotationType => "Annotation type",
            Kind::Method => "Method",
            Kind::Field => "Field",
            Kind::Function => "Function",
            Kind::Property => "Property",
            Kind::Object => "Object",
            Kind::EnumClass => "Enum class",
            Kind::AnnotationClass => "Annotation class",
        }
    }

    fn is_type(self) -> bool {
        !matches!(
            self,
            Kind::Method | Kind::Field | Kind::Function | Kind::Property
        )
    }
}

/// One declaration of a Java file (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    /// Simple name.
    pub name: String,
    /// What it declares.
    pub kind: Kind,
    /// The simple name of the enclosing type, empty for a top-level type.
    pub container: String,
    /// Byte range of the name.
    pub start: usize,
    /// End of the name.
    pub end: usize,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column of the name, in chars.
    pub column: usize,
    /// Char offset of the name.
    pub offset: usize,
    /// Declared `private`.
    pub private: bool,
    /// An entry point or a member used from outside the source (behaviour 4).
    pub entry: bool,
    /// `public` or `protected`, or a member of an interface or annotation type: part of a
    /// library's API when its file lies in an API folder (v3 behaviour 4).
    pub exported: bool,
}

/// A type with the simple names of its supertypes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TypeInfo {
    name: String,
    supertypes: Vec<String>,
}

/// The indexed facts of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Modification time (nanoseconds since the epoch) and size when read.
    stamp: (u128, u64),
    /// Declarations (Java files only).
    pub declarations: Vec<Declaration>,
    /// Uses per name.
    pub uses: BTreeMap<String, u32>,
    types: Vec<TypeInfo>,
}

/// What a refresh did (behaviour 2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Files in the index.
    pub files: usize,
    /// Java files in the index.
    pub java_files: usize,
    /// Kotlin files in the index (v4 behaviour 9).
    pub kotlin_files: usize,
    /// Files read and parsed by this refresh.
    pub read: usize,
    /// Entries dropped because their file is gone.
    pub removed: usize,
    /// Whether the index came from the memory or disk cache.
    pub cached: bool,
}

/// The project-wide reference index (behaviour 1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceIndex {
    format: u32,
    root: PathBuf,
    files: BTreeMap<PathBuf, FileEntry>,
}

/// Saved index files below the user cache folder.
const CACHE_DIR: &str = "reference-index";

fn is_java(rel: &Path) -> bool {
    rel.extension().is_some_and(|e| e == "java")
}

/// A Kotlin source file, indexed by its syntax tree (v4 behaviour 9; scripts are word files).
fn is_kotlin(rel: &Path) -> bool {
    rel.extension().is_some_and(|e| e == "kt")
}

/// A file whose declarations the index holds.
fn has_declarations(rel: &Path) -> bool {
    is_java(rel) || is_kotlin(rel)
}

/// Whether `rel` is a text file that can name Java classes (behaviour 1).
fn is_other_text(rel: &Path) -> bool {
    let ext = rel
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some("kts" | "groovy" | "gradle" | "xml" | "properties" | "yml" | "yaml" | "conf")
    ) || rel
        .to_string_lossy()
        .replace('\\', "/")
        .contains("META-INF/services/")
}

/// Whether `rel` belongs in the index.
pub fn indexed(rel: &Path) -> bool {
    has_declarations(rel) || is_other_text(rel)
}

fn text_of<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn modifiers(n: Node) -> Option<Node> {
    children(n).into_iter().find(|c| c.kind() == "modifiers")
}

fn has_annotation(n: Node) -> bool {
    modifiers(n).is_some_and(|m| children(m).iter().any(|c| c.kind().contains("annotation")))
}

fn has_modifier(n: Node, text: &str, word: &str) -> bool {
    modifiers(n).is_some_and(|m| {
        children(m)
            .into_iter()
            .filter(|c| !c.kind().contains("annotation"))
            .any(|c| text_of(c, text) == word)
    })
}

fn type_kind(kind: &str) -> Option<Kind> {
    Some(match kind {
        "class_declaration" => Kind::Class,
        "interface_declaration" => Kind::Interface,
        "enum_declaration" => Kind::Enum,
        "record_declaration" => Kind::Record,
        "annotation_type_declaration" => Kind::AnnotationType,
        _ => return None,
    })
}

/// The simple names of the types in a `superclass`, `super_interfaces` or `extends_interfaces`
/// node (generic arguments left out).
fn type_names(n: Node, text: &str, out: &mut Vec<String>) {
    match n.kind() {
        "type_arguments" => {}
        "type_identifier" => out.push(text_of(n, text).to_owned()),
        "scoped_type_identifier" => {
            // `a.b.C`: the last name.
            if let Some(last) = children(n)
                .into_iter()
                .rev()
                .find(|c| c.kind() == "type_identifier")
            {
                out.push(text_of(last, text).to_owned());
            }
        }
        _ => {
            for c in children(n) {
                type_names(c, text, out);
            }
        }
    }
}

fn supertypes(decl: Node, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for c in children(decl) {
        if matches!(
            c.kind(),
            "superclass" | "super_interfaces" | "extends_interfaces"
        ) {
            type_names(c, text, &mut out);
        }
    }
    out
}

/// Methods that are called by the platform rather than by name (behaviour 4).
const PLATFORM_METHODS: &[&str] = &[
    "toString",
    "equals",
    "hashCode",
    "clone",
    "finalize",
    "readObject",
    "writeObject",
    "readResolve",
    "writeReplace",
    "readObjectNoData",
];

/// A launchable `main`: no parameters or one `String[]`/`String...`, static or not, of any
/// visibility (v3 behaviour 4: the launch protocol of Java 25 accepts all of them).
fn is_main(m: Node, text: &str) -> bool {
    m.child_by_field_name("name")
        .is_some_and(|n| text_of(n, text) == "main")
        && m.child_by_field_name("parameters").is_some_and(|p| {
            let params: Vec<Node> = children(p)
                .into_iter()
                .filter(|c| matches!(c.kind(), "formal_parameter" | "spread_parameter"))
                .collect();
            params.is_empty() || (params.len() == 1 && text_of(params[0], text).contains("String"))
        })
}

/// Annotations of `java.lang` that do not make a framework call a type's methods (v3
/// behaviour 4).
const PLAIN_ANNOTATIONS: &[&str] = &["SuppressWarnings", "Deprecated", "FunctionalInterface"];

/// Whether `n` carries an annotation other than the plain ones of `java.lang`.
fn has_framework_annotation(n: Node, text: &str) -> bool {
    modifiers(n).is_some_and(|m| {
        children(m)
            .into_iter()
            .filter(|c| c.kind().contains("annotation"))
            .any(|a| {
                let name = a
                    .child_by_field_name("name")
                    .map_or("", |x| text_of(x, text));
                let simple = name.rsplit('.').next().unwrap_or(name);
                !PLAIN_ANNOTATIONS.contains(&simple)
            })
    })
}

/// The enclosing type of a node while the tree is walked.
#[derive(Debug, Clone, Default)]
struct Around {
    /// Simple name, empty at the top level and in anonymous bodies.
    name: String,
    /// Inside an anonymous body, an enum constant body or an annotation type.
    hidden: bool,
    /// An interface or annotation type: its members are implicitly public.
    interface: bool,
    /// A type with a framework annotation: its public methods are entry points.
    framework: bool,
}

/// Line, column and char offset of byte `at`, counting from `from` (behaviour 6 places).
struct Placer<'a> {
    text: &'a str,
    last: usize,
    line: usize,
    line_start: usize,
    chars: usize,
}

impl Placer<'_> {
    fn place(&mut self, at: usize) -> (usize, usize, usize) {
        // Declarations come in tree order, which is ascending.
        if at < self.last {
            self.last = 0;
            self.line = 0;
            self.line_start = 0;
            self.chars = 0;
        }
        for (i, ch) in self.text[self.last..at].char_indices() {
            if ch == '\n' {
                self.line += 1;
                self.line_start = self.last + i + 1;
            }
        }
        self.chars += self.text[self.last..at].chars().count();
        self.last = at;
        (
            self.line,
            self.text[self.line_start..at].chars().count(),
            self.chars,
        )
    }
}

/// The declarations, uses and types of a Java syntax tree (behaviour 1).
fn index_java(root: Node, text: &str) -> FileEntry {
    let mut entry = FileEntry::default();
    let mut names: BTreeSet<usize> = BTreeSet::new();
    let mut placer = Placer {
        text,
        last: 0,
        line: 0,
        line_start: 0,
        chars: 0,
    };
    let mut stack: Vec<(Node, Arc<Around>)> = vec![(root, Arc::new(Around::default()))];
    let mut found: Vec<(Node, Declaration)> = Vec::new();
    // `public`/`protected`, or a member of an interface (v3 behaviour 4).
    let exported = |n: Node, around: &Around| {
        around.interface || has_modifier(n, text, "public") || has_modifier(n, text, "protected")
    };
    while let Some((n, around)) = stack.pop() {
        let kind = n.kind();
        let mut inner = around.clone();
        let container = &around.name;
        let hidden = around.hidden;
        if let Some(k) = type_kind(kind) {
            if let Some(name) = n.child_by_field_name("name") {
                let simple = text_of(name, text).to_owned();
                names.insert(name.start_byte());
                entry.types.push(TypeInfo {
                    name: simple.clone(),
                    supertypes: supertypes(n, text),
                });
                found.push((
                    n,
                    Declaration {
                        name: simple.clone(),
                        kind: k,
                        container: container.clone(),
                        start: name.start_byte(),
                        end: name.end_byte(),
                        line: 0,
                        column: 0,
                        offset: 0,
                        private: has_modifier(n, text, "private"),
                        entry: hidden || has_annotation(n),
                        exported: exported(n, &around),
                    },
                ));
                inner = Arc::new(Around {
                    name: simple,
                    hidden: hidden || k == Kind::AnnotationType,
                    interface: matches!(k, Kind::Interface | Kind::AnnotationType),
                    framework: has_framework_annotation(n, text),
                });
            }
        } else if matches!(kind, "method_declaration") {
            if let Some(name) = n.child_by_field_name("name") {
                names.insert(name.start_byte());
                let simple = text_of(name, text).to_owned();
                let public = around.interface || has_modifier(n, text, "public");
                let entry_point = hidden
                    || has_annotation(n)
                    || is_main(n, text)
                    || has_modifier(n, text, "native")
                    || (around.framework && public)
                    || PLATFORM_METHODS.contains(&simple.as_str());
                found.push((
                    n,
                    Declaration {
                        name: simple,
                        kind: Kind::Method,
                        container: container.clone(),
                        start: name.start_byte(),
                        end: name.end_byte(),
                        line: 0,
                        column: 0,
                        offset: 0,
                        private: has_modifier(n, text, "private"),
                        entry: entry_point,
                        exported: exported(n, &around),
                    },
                ));
            }
        } else if matches!(
            kind,
            "constructor_declaration" | "compact_constructor_declaration"
        ) {
            // A constructor's name is the class name, not a use of it.
            if let Some(name) = n.child_by_field_name("name") {
                names.insert(name.start_byte());
            }
        } else if kind == "field_declaration" || kind == "constant_declaration" {
            let private = has_modifier(n, text, "private");
            let annotated = has_annotation(n);
            let is_exported = exported(n, &around);
            for d in children(n)
                .into_iter()
                .filter(|c| c.kind() == "variable_declarator")
            {
                if let Some(name) = d.child_by_field_name("name") {
                    names.insert(name.start_byte());
                    let simple = text_of(name, text).to_owned();
                    let entry_point = hidden
                        || annotated
                        || matches!(
                            simple.as_str(),
                            "serialVersionUID" | "serialPersistentFields"
                        );
                    found.push((
                        n,
                        Declaration {
                            name: simple,
                            kind: Kind::Field,
                            container: container.clone(),
                            start: name.start_byte(),
                            end: name.end_byte(),
                            line: 0,
                            column: 0,
                            offset: 0,
                            private,
                            entry: entry_point,
                            exported: is_exported,
                        },
                    ));
                }
            }
        } else if matches!(kind, "import_declaration" | "package_declaration") {
            continue;
        } else if kind == "class_body"
            && n.parent()
                .is_some_and(|p| matches!(p.kind(), "object_creation_expression" | "enum_constant"))
        {
            // Members of anonymous classes and enum constant bodies (behaviour 4).
            inner = Arc::new(Around {
                hidden: true,
                ..Around::default()
            });
        } else if matches!(kind, "identifier" | "type_identifier")
            && !names.contains(&n.start_byte())
        {
            *entry.uses.entry(text_of(n, text).to_owned()).or_default() += 1;
        }
        for c in children(n).into_iter().rev() {
            stack.push((c, inner.clone()));
        }
    }
    // A type that declares `main`, or an annotated method or field, is an entry point itself.
    let entry_types: BTreeSet<usize> = found
        .iter()
        .filter(|(n, d)| {
            !d.kind.is_type()
                && (has_annotation(*n) || (d.kind == Kind::Method && is_main(*n, text)))
        })
        .filter_map(|(n, _)| {
            std::iter::successors(n.parent(), |x| x.parent())
                .find(|p| type_kind(p.kind()).is_some())
                .map(|p| p.start_byte())
        })
        .collect();
    found.sort_by_key(|(_, d)| d.start);
    for (n, mut d) in found {
        if d.kind.is_type() && entry_types.contains(&n.start_byte()) {
            d.entry = true;
        }
        let (line, column, offset) = placer.place(d.start);
        d.line = line;
        d.column = column;
        d.offset = offset;
        entry.declarations.push(d);
    }
    entry
}

/// The word uses of a text file that can name Java classes (behaviour 1).
fn index_words(text: &str) -> FileEntry {
    let mut entry = FileEntry::default();
    for word in text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
        if word
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        {
            *entry.uses.entry(word.to_owned()).or_default() += 1;
        }
    }
    entry
}

// ------------------------------------------------------------------ Kotlin (v4 behaviours 9, 10)

/// The names that keep a declaration used: its own, and for a Kotlin property the names of its
/// JVM accessors as Java writes them (v4 behaviour 9).
fn names_of(d: &Declaration) -> Vec<String> {
    let mut out = vec![d.name.clone()];
    if d.kind == Kind::Property {
        out.extend(accessor_names(&d.name));
    }
    out
}

/// `name` → `getName`, `setName`; `isOn` → `setOn` (`isOn` is the getter's own name).
fn accessor_names(name: &str) -> Vec<String> {
    let capital = |s: &str| {
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_uppercase().chain(c).collect::<String>())
            .unwrap_or_default()
    };
    match name.strip_prefix("is") {
        Some(rest) if rest.chars().next().is_some_and(char::is_uppercase) => {
            vec![format!("set{rest}")]
        }
        _ if name.is_empty() => Vec::new(),
        _ => vec![
            format!("get{}", capital(name)),
            format!("set{}", capital(name)),
        ],
    }
}

/// The Java accessor names that a Kotlin use of `name` written as a property also reaches:
/// Kotlin reads `getName()` as `name` and `isOn()` as `isOn` (v4 behaviour 9).
fn property_uses(name: &str) -> Vec<String> {
    if !name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        return Vec::new();
    }
    accessor_names(name)
}

/// How a node is written in its parent, carried down the walk instead of asking for parents
/// (a parent is found by walking down from the root): whether an identifier reads as a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// An expression: an identifier here is written as a property.
    Plain,
    /// What a call calls (`f` of `f()`, `a.f` of `a.f()`).
    Callee,
    /// A declared name or a type.
    Name,
}

/// The roles of the `count` children of a node of `kind` whose own role is `role`.
fn child_role(kind: &str, role: Role, index: usize, count: usize) -> Role {
    match kind {
        "call_expression" if index == 0 => Role::Callee,
        // `a.f`: the last part is called when the navigation is.
        "navigation_expression" if index + 1 == count => role,
        "user_type"
        | "parameter"
        | "class_parameter"
        | "variable_declaration"
        | "type_parameter"
        | "enum_entry" => Role::Name,
        _ => Role::Plain,
    }
}

/// The simple name of a `user_type` (`a.Base<T>` → `Base`).
fn user_type_name<'a>(n: Node, text: &'a str) -> Option<&'a str> {
    children(n)
        .into_iter()
        .rev()
        .find(|c| c.kind() == "identifier")
        .map(|c| text_of(c, text))
}

/// The first `user_type` below `n` in tree order, not inside type or call arguments.
fn first_user_type(n: Node) -> Option<Node> {
    if n.kind() == "user_type" {
        return Some(n);
    }
    children(n)
        .into_iter()
        .filter(|c| !matches!(c.kind(), "type_arguments" | "value_arguments"))
        .find_map(first_user_type)
}

/// The modifier words of a Kotlin declaration (annotations left out) and the simple names of its
/// annotations.
fn kt_modifiers<'a>(n: Node, text: &'a str) -> (Vec<&'a str>, Vec<&'a str>) {
    let Some(m) = modifiers(n) else {
        return (Vec::new(), Vec::new());
    };
    let mut words = Vec::new();
    let mut annotations = Vec::new();
    for c in children(m) {
        if c.kind() == "annotation" {
            annotations.push(
                first_user_type(c)
                    .and_then(|t| user_type_name(t, text))
                    .unwrap_or(""),
            );
        } else {
            words.push(text_of(c, text).trim());
        }
    }
    (words, annotations)
}

/// Annotations that do not make a framework read a class's members (v4 behaviour 10).
const KOTLIN_PLAIN_ANNOTATIONS: &[&str] = &["Suppress", "Deprecated", "OptIn", "JvmInline"];

/// Functions of Java serialization (v4 behaviour 10).
const SERIAL_FUNCTIONS: &[&str] = &[
    "readObject",
    "writeObject",
    "readResolve",
    "writeReplace",
    "readObjectNoData",
];

/// The enclosing Kotlin declaration while the tree is walked.
#[derive(Debug, Clone, Default)]
struct KtAround {
    /// The enclosing type's simple name, empty at the top level.
    name: String,
    /// Inside an annotation class: its members are read through the annotation.
    hidden: bool,
    /// A class or object with an annotation other than the plain ones.
    framework: bool,
    /// A `data`, `value` or annotation class: its constructor properties are used implicitly.
    implicit_properties: bool,
    /// Inside a body, lambda, accessor or object expression: declarations are local.
    local: bool,
    /// Start byte of the enclosing class or object (a companion's is the class around it).
    owner: Option<usize>,
}

/// A launchable Kotlin `main`: no parameters, or one `Array<String>` or `vararg` `String`.
fn is_kotlin_main(f: Node, text: &str) -> bool {
    let Some(params) = children(f)
        .into_iter()
        .find(|c| c.kind() == "function_value_parameters")
    else {
        return false;
    };
    let count = children(params)
        .into_iter()
        .filter(|c| c.kind() == "parameter")
        .count();
    let written = text_of(params, text);
    count == 0
        || (count == 1
            && (written.contains("Array<String>")
                || (written.contains("vararg") && written.contains("String"))))
}

/// The simple names of the supertypes of a Kotlin class, object or companion object.
fn kotlin_supertypes(n: Node, text: &str) -> Vec<String> {
    children(n)
        .into_iter()
        .filter(|c| c.kind() == "delegation_specifiers")
        .flat_map(children)
        .filter(|c| c.kind() == "delegation_specifier")
        .filter_map(first_user_type)
        .filter_map(|t| user_type_name(t, text))
        .map(str::to_owned)
        .collect()
}

/// Counts `word` and the Java accessor names it reaches when written as a property.
fn count_use(uses: &mut BTreeMap<String, u32>, word: &str, as_property: bool) {
    fn add(uses: &mut BTreeMap<String, u32>, word: &str) {
        match uses.get_mut(word) {
            Some(c) => *c += 1,
            None => {
                uses.insert(word.to_owned(), 1);
            }
        }
    }
    add(uses, word);
    if as_property {
        for a in property_uses(word) {
            add(uses, &a);
        }
    }
}

/// The declarations, uses and types of a Kotlin syntax tree (v4 behaviours 9 and 10). With
/// `accessors` a name written as a property also counts for the Java accessors it reaches; the
/// editor's step leaves them out, they only keep Java declarations used and a Kotlin file has
/// none of its own.
fn index_kotlin(root: Node, text: &str, accessors: bool) -> FileEntry {
    let mut entry = FileEntry::default();
    let mut names: BTreeSet<usize> = BTreeSet::new();
    // (declaration, its owner, annotated or `main`)
    let mut found: Vec<(Declaration, Option<usize>, bool)> = Vec::new();
    // (start byte of a class or object, its index in `found`)
    let mut owners: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<(Node, Arc<KtAround>, Role)> =
        vec![(root, Arc::new(KtAround::default()), Role::Plain)];
    let decl = |name: Node, kind: Kind, container: &str, private: bool, entry: bool| Declaration {
        name: text_of(name, text).to_owned(),
        kind,
        container: container.to_owned(),
        start: name.start_byte(),
        end: name.end_byte(),
        line: 0,
        column: 0,
        offset: 0,
        private,
        entry,
        exported: false,
    };
    let local_below = |around: &KtAround| {
        Arc::new(KtAround {
            local: true,
            ..around.clone()
        })
    };
    while let Some((n, around, role)) = stack.pop() {
        let kind = n.kind();
        let mut inner = around.clone();
        let local = around.local;
        match kind {
            "package_header" | "import" | "file_annotation" => continue,
            "class_declaration" | "object_declaration" if !local => {
                if let Some(name) = n.child_by_field_name("name") {
                    let (words, annotations) = kt_modifiers(n, text);
                    let has = |w: &str| words.contains(&w);
                    let k = if kind == "object_declaration" {
                        Kind::Object
                    } else if children(n).iter().any(|c| c.kind() == "interface") {
                        Kind::Interface
                    } else if has("enum") {
                        Kind::EnumClass
                    } else if has("annotation") {
                        Kind::AnnotationClass
                    } else {
                        Kind::Class
                    };
                    names.insert(name.start_byte());
                    let simple = text_of(name, text).to_owned();
                    entry.types.push(TypeInfo {
                        name: simple.clone(),
                        supertypes: kotlin_supertypes(n, text),
                    });
                    let mut d = decl(
                        name,
                        k,
                        &around.name,
                        has("private"),
                        around.hidden || !annotations.is_empty() || has("expect") || has("actual"),
                    );
                    d.exported = !has("private") && !has("internal");
                    owners.push((n.start_byte(), found.len()));
                    found.push((d, around.owner, !annotations.is_empty()));
                    inner = Arc::new(KtAround {
                        name: simple,
                        hidden: around.hidden || k == Kind::AnnotationClass,
                        framework: annotations
                            .iter()
                            .any(|a| !KOTLIN_PLAIN_ANNOTATIONS.contains(a)),
                        implicit_properties: has("data")
                            || has("value")
                            || has("inline")
                            || k == Kind::AnnotationClass,
                        local: false,
                        owner: Some(n.start_byte()),
                    });
                }
            }
            "companion_object" if !local => {
                let name = n
                    .child_by_field_name("name")
                    .or_else(|| children(n).into_iter().find(|c| c.kind() == "identifier"));
                if let Some(name) = name {
                    names.insert(name.start_byte());
                }
                let simple = name.map_or("Companion", |x| text_of(x, text)).to_owned();
                entry.types.push(TypeInfo {
                    name: simple.clone(),
                    supertypes: kotlin_supertypes(n, text),
                });
                let (_, annotations) = kt_modifiers(n, text);
                inner = Arc::new(KtAround {
                    name: simple,
                    hidden: around.hidden,
                    framework: annotations
                        .iter()
                        .any(|a| !KOTLIN_PLAIN_ANNOTATIONS.contains(a)),
                    implicit_properties: false,
                    local: false,
                    owner: around.owner,
                });
            }
            "class_parameter" if !local => {
                let parts = children(n);
                let property = parts.iter().any(|c| matches!(c.kind(), "val" | "var"));
                if property && let Some(name) = parts.iter().find(|c| c.kind() == "identifier") {
                    let (words, annotations) = kt_modifiers(n, text);
                    let has = |w: &str| words.contains(&w);
                    names.insert(name.start_byte());
                    let mut d = decl(
                        *name,
                        Kind::Property,
                        &around.name,
                        has("private"),
                        around.hidden
                            || around.implicit_properties
                            || !annotations.is_empty()
                            || has("override")
                            || has("expect")
                            || has("actual")
                            || (around.framework
                                && !has("private")
                                && !has("internal")
                                && !has("protected")),
                    );
                    d.exported = !has("private") && !has("internal");
                    found.push((d, around.owner, !annotations.is_empty()));
                }
            }
            "property_declaration" if !local => {
                let name = children(n)
                    .into_iter()
                    .find(|c| c.kind() == "variable_declaration")
                    .and_then(|v| children(v).into_iter().find(|c| c.kind() == "identifier"));
                if let Some(name) = name {
                    let (words, annotations) = kt_modifiers(n, text);
                    let has = |w: &str| words.contains(&w);
                    names.insert(name.start_byte());
                    let public = !has("private") && !has("internal") && !has("protected");
                    let simple = text_of(name, text);
                    let mut d = decl(
                        name,
                        Kind::Property,
                        &around.name,
                        has("private"),
                        around.hidden
                            || !annotations.is_empty()
                            || has("override")
                            || has("expect")
                            || has("actual")
                            || (around.framework && public)
                            || matches!(simple, "serialVersionUID" | "serialPersistentFields"),
                    );
                    d.exported = !has("private") && !has("internal");
                    found.push((d, around.owner, !annotations.is_empty()));
                }
                // The initializer, delegate and accessors are local.
                inner = local_below(&around);
            }
            "function_declaration" if !local => {
                if let Some(name) = n.child_by_field_name("name") {
                    let (words, annotations) = kt_modifiers(n, text);
                    let has = |w: &str| words.contains(&w);
                    names.insert(name.start_byte());
                    let public = !has("private") && !has("internal") && !has("protected");
                    let simple = text_of(name, text);
                    // An extension receiver: a type written before the name.
                    let receiver = children(n)
                        .into_iter()
                        .take_while(|c| c.start_byte() < name.start_byte())
                        .filter(|c| !matches!(c.kind(), "modifiers" | "type_parameters"))
                        .find_map(first_user_type)
                        .and_then(|t| user_type_name(t, text));
                    let main = simple == "main" && is_kotlin_main(n, text);
                    let mut d = decl(
                        name,
                        Kind::Function,
                        &around.name,
                        has("private"),
                        around.hidden
                            || !annotations.is_empty()
                            || main
                            || has("override")
                            || has("operator")
                            || has("external")
                            || has("expect")
                            || has("actual")
                            || (around.framework && public)
                            || (receiver == Some("Application") && public)
                            || SERIAL_FUNCTIONS.contains(&simple),
                    );
                    d.exported = !has("private") && !has("internal");
                    found.push((d, around.owner, !annotations.is_empty() || main));
                }
                inner = local_below(&around);
            }
            "enum_entry" => {
                if let Some(name) = children(n).into_iter().find(|c| c.kind() == "identifier") {
                    names.insert(name.start_byte());
                }
                // Members of an entry body are used through the enum class.
                inner = local_below(&around);
            }
            "object_literal"
            | "secondary_constructor"
            | "anonymous_initializer"
            | "getter"
            | "setter"
            | "lambda_literal"
            | "function_body" => inner = local_below(&around),
            "identifier" if !names.contains(&n.start_byte()) => {
                count_use(
                    &mut entry.uses,
                    text_of(n, text),
                    accessors && role == Role::Plain,
                );
            }
            "string_content" => {
                // `$name` in a string template.
                let mut rest = text_of(n, text);
                while let Some(i) = rest.find('$') {
                    rest = &rest[i + 1..];
                    let end = rest
                        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .unwrap_or(rest.len());
                    if end > 0 && !rest.starts_with(|c: char| c.is_ascii_digit()) {
                        count_use(&mut entry.uses, &rest[..end], accessors);
                    }
                }
            }
            _ => {}
        }
        let parts = children(n);
        let count = parts.len();
        for (i, c) in parts.into_iter().enumerate().rev() {
            stack.push((c, inner.clone(), child_role(kind, role, i, count)));
        }
    }
    // A class or object that declares an annotated member or `main` is an entry point itself.
    let entry_owners: BTreeSet<usize> = found
        .iter()
        .filter(|(d, _, marked)| *marked && !d.kind.is_type())
        .filter_map(|(_, owner, _)| *owner)
        .collect();
    for (start, i) in owners {
        if entry_owners.contains(&start) {
            found[i].0.entry = true;
        }
    }
    let mut decls: Vec<Declaration> = found.into_iter().map(|(d, _, _)| d).collect();
    decls.sort_by_key(|d| d.start);
    let mut placer = Placer {
        text,
        last: 0,
        line: 0,
        line_start: 0,
        chars: 0,
    };
    for mut d in decls {
        let (line, column, offset) = placer.place(d.start);
        d.line = line;
        d.column = column;
        d.offset = offset;
        entry.declarations.push(d);
    }
    entry
}

/// The index entry of one file's text (behaviour 1); `None` for a Java file that cannot be parsed.
pub fn index_text(rel: &Path, text: &str) -> Option<FileEntry> {
    if is_java(rel) {
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).ok()?;
        Some(index_java(doc.tree().root_node(), text))
    } else if is_kotlin(rel) {
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, text).ok()?;
        Some(index_kotlin(doc.tree().root_node(), text, true))
    } else {
        Some(index_words(text))
    }
}

fn stamp_of(path: &Path) -> Option<(u128, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let time = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some((time, m.len()))
}

/// One unused declaration (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unused<'a> {
    /// The file, relative to the project root.
    pub path: &'a Path,
    /// The declaration.
    pub declaration: &'a Declaration,
}

impl Unused<'_> {
    /// The finding's message (behaviour 5).
    pub fn message(&self) -> String {
        let d = self.declaration;
        match d.kind {
            Kind::Method => format!("Method '{}()' is never used", d.name),
            k => format!("{} '{}' is never used", k.word(), d.name),
        }
    }
}

impl ReferenceIndex {
    /// An empty index of the project at `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            format: FORMAT,
            root: root.to_path_buf(),
            files: BTreeMap::new(),
        }
    }

    /// The saved index of `root` in `file`, or an empty one when it is missing, unreadable, of
    /// another format or of another project (behaviour 2).
    pub fn load(file: &Path, root: &Path) -> Self {
        std::fs::read(file)
            .ok()
            .and_then(|b| serde_json::from_slice::<ReferenceIndex>(&b).ok())
            .filter(|i| i.format == FORMAT && i.root == root)
            .unwrap_or_else(|| Self::new(root))
    }

    /// Saves the index to `file` atomically, then removes the saved indexes in the same folder
    /// whose project folder is gone (behaviour 2).
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        let dir = file
            .parent()
            .ok_or_else(|| std::io::Error::other("cache file without a folder"))?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(
            ".{}.{}.tmp",
            file.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            std::process::id()
        ));
        std::fs::write(
            &tmp,
            serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&tmp, file).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        prune(dir);
        Ok(())
    }

    /// The project folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Brings the index up to date with `files` (relative to the root; files that do not belong in
    /// the index are skipped): reads only new and changed files, drops the others (behaviour 2).
    pub fn refresh(&mut self, files: &[PathBuf]) -> Stats {
        let mut stats = Stats::default();
        let wanted: BTreeSet<&PathBuf> = files.iter().filter(|f| indexed(f)).collect();
        let before = self.files.len();
        self.files.retain(|p, _| wanted.contains(p));
        stats.removed = before - self.files.len();
        for rel in wanted {
            let path = self.root.join(rel);
            let Some(stamp) = stamp_of(&path) else {
                self.files.remove(rel);
                continue;
            };
            if self.files.get(rel).is_some_and(|e| e.stamp == stamp) {
                continue;
            }
            let text = (stamp.1 <= MAX_FILE_BYTES)
                .then(|| std::fs::read_to_string(&path).ok())
                .flatten();
            stats.read += 1;
            match text.and_then(|t| index_text(rel, &t)) {
                Some(mut e) => {
                    e.stamp = stamp;
                    self.files.insert(rel.clone(), e);
                }
                // Unreadable or oversized: remembered empty, so it is not read again unchanged.
                None => {
                    self.files.insert(
                        rel.clone(),
                        FileEntry {
                            stamp,
                            ..FileEntry::default()
                        },
                    );
                }
            }
        }
        stats.files = self.files.len();
        stats.java_files = self.files.keys().filter(|p| is_java(p)).count();
        stats.kotlin_files = self.files.keys().filter(|p| is_kotlin(p)).count();
        stats
    }

    /// The files that use `name`, with the number of uses.
    pub fn uses_of(&self, name: &str) -> Vec<(&Path, u32)> {
        self.files
            .iter()
            .filter_map(|(p, e)| e.uses.get(name).map(|n| (p.as_path(), *n)))
            .collect()
    }

    /// The declarations named `name`.
    pub fn declarations_of(&self, name: &str) -> Vec<(&Path, &Declaration)> {
        self.files
            .iter()
            .flat_map(|(p, e)| {
                e.declarations
                    .iter()
                    .filter(move |d| d.name == name)
                    .map(move |d| (p.as_path(), d))
            })
            .collect()
    }

    /// The unused declarations of the files for which `in_scope` holds (behaviour 3), in path and
    /// position order.
    pub fn unused(&self, in_scope: impl Fn(&Path) -> bool) -> Vec<Unused<'_>> {
        let mut used: HashMap<&str, u32> = HashMap::new();
        let mut types: HashMap<&str, Vec<&[String]>> = HashMap::new();
        for e in self.files.values() {
            for (n, c) in &e.uses {
                *used.entry(n.as_str()).or_default() += c;
            }
            for t in &e.types {
                types
                    .entry(t.name.as_str())
                    .or_default()
                    .push(t.supertypes.as_slice());
            }
        }
        let api = api_folders(&self.root);
        let mut out = Vec::new();
        for (path, e) in &self.files {
            if !in_scope(path) {
                continue;
            }
            let in_api = in_api_folder(&api, path);
            for d in &e.declarations {
                if d.private
                    || d.entry
                    || (in_api && d.exported)
                    || names_of(d)
                        .iter()
                        .any(|n| used.get(n.as_str()).is_some_and(|c| *c > 0))
                {
                    continue;
                }
                if d.kind == Kind::Method && library_supertype(&d.container, &types) {
                    continue;
                }
                out.push(Unused {
                    path,
                    declaration: d,
                });
            }
        }
        out
    }
}

/// Whether a method of type `ty` may implement a library method: one of its supertypes,
/// followed through the project's types, is not declared in the project (behaviour 4).
fn library_supertype(ty: &str, types: &HashMap<&str, Vec<&[String]>>) -> bool {
    let mut seen = BTreeSet::new();
    let mut pending = vec![ty];
    while let Some(t) = pending.pop() {
        if !seen.insert(t) {
            continue;
        }
        let Some(decls) = types.get(t) else {
            // Not a project type: a library type (the start type is always known).
            return t != ty;
        };
        for supers in decls {
            for s in supers.iter() {
                if !types.contains_key(s.as_str()) {
                    return true;
                }
                pending.push(s);
            }
        }
    }
    false
}

/// The file naming the API folders of a library (v3 behaviour 4).
pub const ENTRY_POINTS_FILE: &str = ".idecraft/entry-points.toml";

/// The API folders of the project at `root`, relative to it, from [`ENTRY_POINTS_FILE`]
/// (`api = ["lib/src/main/java"]`; v3 behaviour 4): none without the file or with an unreadable
/// one; a folder that is absolute, holds `..` or lies behind a symbolic link is left out.
pub fn api_folders(root: &Path) -> Vec<PathBuf> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        api: Vec<String>,
    }
    let Some(file) = std::fs::read_to_string(root.join(ENTRY_POINTS_FILE))
        .ok()
        .and_then(|t| toml::from_str::<File>(&t).ok())
    else {
        return Vec::new();
    };
    file.api
        .iter()
        .map(|f| PathBuf::from(f.trim_end_matches('/')))
        .filter(|f| {
            !f.as_os_str().is_empty()
                && f.components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
        })
        .filter(|f| {
            // No component may be a symbolic link (security first).
            let mut at = root.to_path_buf();
            f.components().all(|c| {
                at.push(c);
                std::fs::symlink_metadata(&at).is_ok_and(|m| !m.file_type().is_symlink())
            })
        })
        .collect()
}

fn in_api_folder(api: &[PathBuf], rel: &Path) -> bool {
    api.iter().any(|a| rel.starts_with(a))
}

/// Removes the saved indexes in `dir` whose project folder no longer exists (behaviour 2).
fn prune(dir: &Path) {
    #[derive(Deserialize)]
    struct Head {
        root: PathBuf,
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let gone = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Head>(&b).ok())
            .is_some_and(|h| !h.root.is_dir());
        if gone {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// The saved index file of the project at `root` in the user cache folder (behaviour 2).
pub fn cache_file(root: &Path) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let hex: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    Some(
        idecraft_core::user_cache_dir()?
            .join(CACHE_DIR)
            .join(format!("{hex}.json")),
    )
}

/// What the editor's step needs of an index besides its entries (v3 behaviour 8): the uses of
/// every name summed over the files, and the project's types with the file that declares them.
#[derive(Debug, Default)]
pub struct Totals {
    uses: HashMap<String, u64>,
    /// Type name → (file number, simple names of its supertypes).
    types: HashMap<String, Vec<(usize, Vec<String>)>>,
    /// File → its number in `types`.
    file_no: HashMap<PathBuf, usize>,
}

impl Totals {
    /// The totals of `index`.
    pub fn of(index: &ReferenceIndex) -> Self {
        let mut t = Totals::default();
        for (no, (path, e)) in index.files.iter().enumerate() {
            t.file_no.insert(path.clone(), no);
            for (n, c) in &e.uses {
                *t.uses.entry(n.clone()).or_default() += u64::from(*c);
            }
            for ty in &e.types {
                t.types
                    .entry(ty.name.clone())
                    .or_default()
                    .push((no, ty.supertypes.clone()));
            }
        }
        t
    }
}

/// An index in memory with its totals: `index` is `None` until it was first loaded, `totals` is
/// `None` until a refresh has ended (not ready).
#[derive(Default)]
struct Held {
    index: Option<ReferenceIndex>,
    totals: Option<Arc<Totals>>,
}

/// The indexes of this process by project folder, each behind its own lock, so that a long
/// refresh of one project makes only that project busy; the map's lock is held only to look up.
static MEMORY: Mutex<Option<HashMap<PathBuf, Arc<Mutex<Held>>>>> = Mutex::new(None);

/// The slot of the project at `root`, created empty when missing.
fn slot(root: &Path) -> Arc<Mutex<Held>> {
    let mut guard = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(HashMap::new)
        .entry(root.to_path_buf())
        .or_default()
        .clone()
}

/// Puts `index` in memory as the ready index of its project, with its totals, as a refresh does
/// (for measurements and checks that build an index without the user cache).
pub fn hold(index: ReferenceIndex) {
    let slot = slot(&index.root.clone());
    let mut held = slot.lock().unwrap_or_else(|e| e.into_inner());
    held.totals = Some(Arc::new(Totals::of(&index)));
    held.index = Some(index);
}

/// Whether the index of the project at `root` is ready in this process: a refresh has ended
/// (v3 behaviour 8). `false` while a refresh of that project holds it.
pub fn is_ready(root: &Path) -> bool {
    slot(root)
        .try_lock()
        .is_ok_and(|h| h.index.is_some() && h.totals.is_some())
}

/// The files the index of the project at `root` is built over, relative to it (v2 behaviour 7):
/// the project's own sources, and the files left out of them only because an ignore rule of Git
/// matches, since a generated source of the project is not committed but its uses are real.
/// Virtual environments, installed dependencies and build output are not read: a use there must
/// not keep a declaration "used".
pub fn project_files(root: &Path) -> Vec<PathBuf> {
    idecraft_core::OwnSources::new(root).sources_and_ignored()
}

/// Runs `f` on the refreshed index of the project at `root` over its `files` (relative): from
/// memory, else from the user cache, else new; saved after a refresh that changed it
/// (behaviour 2).
pub fn with_project_index<T>(
    root: &Path,
    files: &[PathBuf],
    f: impl FnOnce(&ReferenceIndex, &Stats) -> T,
) -> T {
    let slot = slot(root);
    let mut held = slot.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache_file(root);
    let mut cached = true;
    let index = held.index.get_or_insert_with(|| {
        let loaded = cache.as_deref().map_or_else(
            || ReferenceIndex::new(root),
            |c| ReferenceIndex::load(c, root),
        );
        cached = !loaded.files.is_empty();
        loaded
    });
    let mut stats = index.refresh(files);
    stats.cached = cached;
    let changed = stats.read > 0 || stats.removed > 0;
    if changed && let Some(c) = cache.as_deref() {
        let _ = index.save(c);
    }
    let totals = (changed || held.totals.is_none())
        .then(|| Arc::new(Totals::of(held.index.as_ref().expect("loaded above"))));
    if let Some(t) = totals {
        held.totals = Some(t);
    }
    f(held.index.as_ref().expect("loaded above"), &stats)
}

/// The id of the action Safe Delete, which the editor's fix of an unused declaration runs
/// (`SPEC-0098` v12 behaviour 26).
pub const SAFE_DELETE: &str = "SafeDelete";

/// [`editor_unused`] on a snapshot of the editor's tree.
pub fn editor_unused_of(
    root: &Path,
    rel: &Path,
    snapshot: &crate::syntax::TreeSnapshot,
    cancel: &AtomicBool,
) -> Option<Vec<Finding>> {
    editor_unused(
        root,
        rel,
        snapshot.tree().root_node(),
        snapshot.text(),
        cancel,
    )
}

/// The editor's step for the unused declarations of the file `rel` (relative to `root`) whose
/// current text is `text` with the syntax tree `tree` (`SPEC-0316` v3 behaviour 8, `SPEC-0098`
/// v12 behaviour 26): its declarations and uses from the tree, every other file's from the ready
/// index. Top-level types are left out. Each finding carries the fix "Safe delete 'name'"; the
/// suppressions and the project's profile are applied. `None` when the index is not ready (or a
/// refresh holds it) and when `cancel` was set meanwhile.
pub(crate) fn editor_unused(
    root: &Path,
    rel: &Path,
    tree: Node,
    text: &str,
    cancel: &AtomicBool,
) -> Option<Vec<Finding>> {
    let cancelled = || cancel.load(Ordering::Relaxed);
    if cancelled() || !has_declarations(rel) || !is_ready(root) {
        return None;
    }
    // v4 behaviour 9 / SPEC-0098 v13 behaviour 27: a Kotlin file from its Kotlin tree.
    let here = if is_kotlin(rel) {
        index_kotlin(tree, text, false)
    } else {
        index_java(tree, text)
    };
    if cancelled() {
        return None;
    }
    let api = api_folders(root);
    let in_api = in_api_folder(&api, rel);
    let candidates: Vec<&Declaration> = here
        .declarations
        .iter()
        .filter(|d| !d.private && !d.entry && !(in_api && d.exported))
        // A top-level type is the file itself (v12 behaviour 26).
        .filter(|d| !(d.kind.is_type() && d.container.is_empty()))
        .filter(|d| {
            !names_of(d)
                .iter()
                .any(|n| here.uses.get(n).is_some_and(|c| *c > 0))
        })
        .collect();
    if candidates.is_empty() {
        return Some(Vec::new());
    }
    // Briefly under the lock: the totals and this file's indexed uses of the candidates' names.
    let (totals, before) = {
        let slot = slot(root);
        let held = slot.try_lock().ok()?;
        let totals = held.totals.clone()?;
        let entry = held.index.as_ref()?.files.get(rel);
        let before: HashMap<String, u64> = candidates
            .iter()
            .flat_map(|d| names_of(d))
            .map(|name| {
                let n = entry.and_then(|e| e.uses.get(&name)).copied().unwrap_or(0);
                (name, u64::from(n))
            })
            .collect();
        (totals, before)
    };
    if cancelled() {
        return None;
    }
    let elsewhere = |name: &str| {
        totals
            .uses
            .get(name)
            .copied()
            .unwrap_or(0)
            .saturating_sub(before.get(name).copied().unwrap_or(0))
    };
    let left: Vec<&Declaration> = candidates
        .into_iter()
        .filter(|d| names_of(d).iter().all(|n| elsewhere(n) == 0))
        .collect();
    let mut types: HashMap<&str, Vec<&[String]>> = HashMap::new();
    if left.iter().any(|d| d.kind == Kind::Method) {
        let own = totals.file_no.get(rel).copied();
        for (name, decls) in &totals.types {
            for (no, supers) in decls {
                if Some(*no) != own {
                    types
                        .entry(name.as_str())
                        .or_default()
                        .push(supers.as_slice());
                }
            }
        }
        for t in &here.types {
            types
                .entry(t.name.as_str())
                .or_default()
                .push(t.supertypes.as_slice());
        }
    }
    let mut found = Vec::new();
    for d in left {
        if d.kind == Kind::Method && library_supertype(&d.container, &types) {
            continue;
        }
        let message = Unused {
            path: rel,
            declaration: d,
        }
        .message();
        let f = Finding {
            id: unused_id(rel),
            start: d.start,
            end: d.end,
            level: Level::Warning,
            message,
            fix: Some(Fix {
                title: format!("Safe delete '{}'", d.name),
                edits: Vec::new(),
                learn: None,
                keep: None,
                action: Some(SAFE_DELETE),
            }),
        };
        if suppressed_in(rel, tree, text, &f) {
            continue;
        }
        found.push(f);
    }
    if cancelled() {
        return None;
    }
    Some(crate::inspection_profile::filter(&root.join(rel), found))
}

/// Whether finding `f` of the file `rel` is suppressed in its tree: `@SuppressWarnings` and
/// `//noinspection` in Java, `@Suppress` in Kotlin (`SPEC-0098` v13 behaviour 28).
fn suppressed_in(rel: &Path, tree: Node, text: &str, f: &Finding) -> bool {
    if is_kotlin(rel) {
        crate::kotlin_inspections::suppressed(tree, text, f)
    } else {
        crate::java_inspections::suppressed(tree, text, f)
    }
}

/// The findings of `java.unused-declaration` and `kotlin.unused-declaration` (v4 behaviour 9)
/// for the files in `scope` (relative to `root`), with the index refreshed over `all` project
/// files (behaviour 5): placed, before the profile, suppressions applied.
pub fn unused_findings(root: &Path, all: &[PathBuf], scope: &[PathBuf]) -> Vec<Placed> {
    let in_scope: BTreeSet<&PathBuf> = scope.iter().filter(|p| has_declarations(p)).collect();
    if in_scope.is_empty() {
        return Vec::new();
    }
    let found: Vec<(PathBuf, Declaration, String)> = with_project_index(root, all, |index, _| {
        index
            .unused(|p| in_scope.contains(&p.to_path_buf()))
            .into_iter()
            .map(|u| (u.path.to_path_buf(), u.declaration.clone(), u.message()))
            .collect()
    });
    let mut out = Vec::new();
    let mut by_file: BTreeMap<PathBuf, Vec<(Declaration, String)>> = BTreeMap::new();
    for (p, d, m) in found {
        by_file.entry(p).or_default().push((d, m));
    }
    for (rel, decls) in by_file {
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        let language = if is_kotlin(&rel) {
            LanguageId::Kotlin
        } else {
            LanguageId::Java
        };
        let Ok(doc) = SyntaxDocument::new(language, false, &text) else {
            continue;
        };
        let tree_root = doc.tree().root_node();
        let id = unused_id(&rel);
        for (d, message) in decls {
            // The file may have changed since it was indexed.
            if text.get(d.start..d.end) != Some(d.name.as_str()) {
                continue;
            }
            let f = Finding {
                id,
                start: d.start,
                end: d.end,
                level: Level::Warning,
                message: message.clone(),
                fix: None,
            };
            if suppressed_in(&rel, tree_root, &text, &f) {
                continue;
            }
            out.push(Placed {
                path: rel.clone(),
                line: d.line,
                column: d.column,
                offset: d.offset,
                id,
                name: crate::inspections::name_of(id),
                level: Level::Warning,
                message,
                fix: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> (PathBuf, Vec<PathBuf>) {
        let dir = std::env::temp_dir().join(format!(
            "idecraft-refindex-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut rels = Vec::new();
        for (rel, text) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            rels.push(PathBuf::from(rel));
        }
        (dir, rels)
    }

    fn unused_names(index: &ReferenceIndex) -> Vec<String> {
        let mut v: Vec<String> = index
            .unused(|_| true)
            .iter()
            .map(|u| format!("{}:{}", u.declaration.container, u.declaration.name))
            .collect();
        v.sort();
        v
    }

    // SPEC-0316/AK-1
    #[test]
    fn declarations_uses_and_unused_members() {
        let a = "package p;\nimport q.Lib;\npublic class Service {\n    public static final int LIMIT = 3;\n    public int count;\n    public void used() {}\n    public void lonely() {}\n    void helper() { used(); }\n    @Deprecated public void annotated() {}\n    private void hidden() {}\n    public String toString() { return \"s\"; }\n    public Service() {}\n    public Runnable r = new Runnable() { public void run() {} public void extra() {} };\n}\n";
        let b = "package p;\npublic class Main {\n    public static void main(String[] args) { new Service().helper(); System.out.println(Service.LIMIT); }\n}\n";
        let c = "package p;\nclass Unused {\n    void m() {}\n}\n";
        let d = "package p;\nclass Task extends java.util.TimerTask {\n    public void run() {}\n    public void cancelLater() {}\n}\nclass Loose implements Shape {\n    public void area() {}\n}\ninterface Shape { }\n";
        let e = "package p;\nclass FooTest {\n    @org.junit.Test void works() {}\n}\n@SuppressWarnings(\"unused\")\nclass Quiet {\n    public void nobody() {}\n}\nclass Named {}\n";
        let xml = "<bean class=\"p.Named\"/>\n";
        let (dir, files) = project(&[
            ("src/p/Service.java", a),
            ("src/p/Main.java", b),
            ("src/p/Unused.java", c),
            ("src/p/Task.java", d),
            ("src/p/FooTest.java", e),
            ("src/beans.xml", xml),
        ]);
        let mut index = ReferenceIndex::new(&dir);
        let stats = index.refresh(&files);
        assert_eq!((stats.files, stats.java_files, stats.read), (6, 5, 6));
        let service = &index.files[&PathBuf::from("src/p/Service.java")];
        // The import and the constructor's name are no uses; `used` is used once by `helper`.
        assert_eq!(service.uses.get("Lib"), None);
        assert_eq!(service.uses.get("used"), Some(&1));
        assert!(!service.uses.contains_key("Service"), "{:?}", service.uses);
        let lonely = service
            .declarations
            .iter()
            .find(|d| d.name == "lonely")
            .unwrap();
        assert_eq!((lonely.line, lonely.column), (6, 16));
        assert_eq!(
            unused_names(&index),
            [
                ":Loose",
                ":Task",
                ":Unused",
                "Loose:area",
                "Quiet:nobody",
                "Service:count",
                "Service:lonely",
                "Service:r",
                "Unused:m",
            ]
        );
        let msgs: Vec<String> = index.unused(|_| true).iter().map(Unused::message).collect();
        assert!(msgs.contains(&"Method 'lonely()' is never used".to_owned()));
        assert!(msgs.contains(&"Class 'Unused' is never used".to_owned()));
        assert!(msgs.contains(&"Field 'count' is never used".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0316/AK-1: the class-level suppression keeps its members out of the findings.
    #[test]
    fn findings_follow_suppression_and_scope() {
        let (dir, files) = project(&[
            (
                "src/Quiet.java",
                "@SuppressWarnings(\"unused\")\nclass Quiet {\n    public void nobody() {}\n}\n",
            ),
            (
                "src/Loud.java",
                "class Loud {\n    //noinspection unused\n    public void hushed() {}\n    public void alone() {}\n}\n",
            ),
            ("src/User.java", "class User { Loud l; Quiet q; }\n"),
        ]);
        let scope = [
            PathBuf::from("src/Loud.java"),
            PathBuf::from("src/Quiet.java"),
        ];
        let found = unused_findings(&dir, &files, &scope);
        let found: Vec<_> = found
            .iter()
            .map(|p| {
                (
                    p.path.to_string_lossy().into_owned(),
                    p.line,
                    p.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [(
                "src/Loud.java".to_owned(),
                3,
                "Method 'alone()' is never used"
            )]
        );
        assert!(unused_findings(&dir, &files, &[PathBuf::from("README.md")]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0316/AK-8 (v3 behaviour 4)
    #[test]
    fn more_entry_points() {
        let a = "package p;\nclass Launch {\n    void main() {}\n}\nclass Native {\n    native void call();\n    public void plain() {}\n}\nclass Ser implements java.io.Serializable {\n    static final Object[] serialPersistentFields = {};\n}\n@org.testng.annotations.Test\npublic class NgTest {\n    public void works() {}\n    void notTest() {}\n}\n@SuppressWarnings(\"x\")\n@Deprecated\nclass Plain {\n    public void quiet() {}\n}\nclass User { Native n; Ser s; Plain q; }\n";
        let (dir, files) = project(&[("src/p/A.java", a)]);
        let mut index = ReferenceIndex::new(&dir);
        index.refresh(&files);
        // The instance `main` keeps its class; `native`, `serialPersistentFields` and the public
        // method of the TestNG class are kept, the package-private one and the method of a class
        // with only plain annotations are not.
        assert_eq!(
            unused_names(&index),
            [
                ":User",
                "Native:plain",
                "NgTest:notTest",
                "Plain:quiet",
                "User:n",
                "User:q",
                "User:s"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0316/AK-8 (v3 behaviour 4): the API folders of a library.
    #[test]
    fn api_folders_of_a_library() {
        let lib = "package l;\npublic class Api {\n    public void call() {}\n    protected int hook;\n    void inner() {}\n}\ninterface Port { void open(); }\n";
        let app = "package a;\npublic class App {\n    public void call2() {}\n}\n";
        let (dir, files) = project(&[("lib/src/l/Api.java", lib), ("app/src/a/App.java", app)]);
        let mut index = ReferenceIndex::new(&dir);
        index.refresh(&files);
        let all = [
            ":Api",
            ":App",
            ":Port",
            "Api:call",
            "Api:hook",
            "Api:inner",
            "App:call2",
            "Port:open",
        ];
        assert_eq!(unused_names(&index), all);
        std::fs::create_dir_all(dir.join(".idecraft")).unwrap();
        let config = dir.join(ENTRY_POINTS_FILE);
        std::fs::write(&config, "api = [\"lib/src/\"]\n").unwrap();
        assert_eq!(api_folders(&dir), [PathBuf::from("lib/src")]);
        // Public and protected declarations and interface members there are kept; the
        // package-private type `Port` and method `inner` are not.
        assert_eq!(
            unused_names(&index),
            [":App", ":Port", "Api:inner", "App:call2"]
        );
        // Absolute and `..` folders are ignored, an unreadable file means none.
        std::fs::write(&config, "api = [\"/tmp\", \"../x\", \"lib/../app\"]\n").unwrap();
        assert!(api_folders(&dir).is_empty());
        std::fs::write(&config, "api = 3").unwrap();
        assert!(api_folders(&dir).is_empty());
        assert_eq!(unused_names(&index), all);
        // A folder behind a symbolic link is ignored.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("lib"), dir.join("linked")).unwrap();
            std::fs::write(&config, "api = [\"linked/src\"]\n").unwrap();
            assert!(api_folders(&dir).is_empty());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn editor_names(root: &Path, rel: &str, text: &str) -> Option<Vec<String>> {
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let cancel = AtomicBool::new(false);
        editor_unused(root, Path::new(rel), doc.tree().root_node(), text, &cancel).map(|v| {
            v.into_iter()
                .map(|f| text[f.start..f.end].to_owned())
                .collect()
        })
    }

    // SPEC-0098/AK-16, SPEC-0316/AK-9
    #[test]
    fn editor_findings_of_the_unsaved_text() {
        let fold = "public class Fold {\n    private int count;\n    public int getCount() { return count; }\n    public void add() { count++; }\n    Runnable task = null;\n    static class Inner {\n        java.util.Map<String, Integer> sizes;\n    }\n    public String toString() { return \"\"; }\n    @Override public int hashCode() { return 1; }\n}\n";
        let other = "class Other {\n    void use(Fold f) { f.add(); }\n    public static void main(String[] a) {}\n}\n";
        let (dir, files) = project(&[("src/Fold.java", fold), ("src/Other.java", other)]);
        let mut index = ReferenceIndex::new(&dir);
        index.refresh(&files);
        hold(index);
        // `Fold` is a top-level type; `add` is used by `Other`; `count` is private.
        assert_eq!(
            editor_names(&dir, "src/Fold.java", fold).unwrap(),
            ["getCount", "task", "Inner", "sizes"]
        );
        // The unsaved text counts: a use typed into the file, and a declaration removed.
        let typed = fold.replace("count++;", "count++; getCount(); new Inner();");
        assert_eq!(
            editor_names(&dir, "src/Fold.java", &typed).unwrap(),
            ["task", "sizes"]
        );
        // A use the file has on disk and lost in the editor no longer counts.
        std::fs::write(dir.join("src/Fold.java"), &typed).unwrap();
        let mut index = ReferenceIndex::new(&dir);
        index.refresh(&files);
        hold(index);
        assert_eq!(
            editor_names(&dir, "src/Fold.java", &typed).unwrap(),
            ["task", "sizes"]
        );
        assert_eq!(
            editor_names(&dir, "src/Fold.java", fold).unwrap(),
            ["getCount", "task", "Inner", "sizes"]
        );
        // Another file's declarations: the package-private `use`; `main` is an entry point.
        assert_eq!(
            editor_names(&dir, "src/Other.java", other).unwrap(),
            ["use"]
        );
        // Each finding has the Safe Delete fix; a suppression and the profile apply.
        let doc = SyntaxDocument::new(LanguageId::Java, false, fold).unwrap();
        let found = editor_unused(
            &dir,
            Path::new("src/Fold.java"),
            doc.tree().root_node(),
            fold,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(found[0].message, "Method 'getCount()' is never used");
        let fix = found[0].fix.as_ref().unwrap();
        assert_eq!(
            (fix.title.as_str(), fix.action),
            ("Safe delete 'getCount'", Some(SAFE_DELETE))
        );
        let hushed = fold.replace(
            "    Runnable task",
            "    //noinspection unused\n    Runnable task",
        );
        assert_eq!(
            editor_names(&dir, "src/Fold.java", &hushed).unwrap(),
            ["getCount", "Inner", "sizes"]
        );
        let mut profile = crate::inspection_profile::Profile::default();
        profile.set(UNUSED_DECLARATION, Some(false), None);
        crate::inspection_profile::save(&dir, &profile).unwrap();
        assert_eq!(
            editor_names(&dir, "src/Fold.java", fold).unwrap(),
            Vec::<String>::new()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0098/AK-16: nothing before the index is ready, nothing once cancelled.
    #[test]
    fn editor_findings_wait_for_the_index_and_stop_when_cancelled() {
        let text = "class A {\n    void lonely() {}\n}\n";
        let (dir, files) = project(&[("src/A.java", text)]);
        assert!(!is_ready(&dir));
        assert_eq!(editor_names(&dir, "src/A.java", text), None);
        with_project_index_uncached(&dir, &files);
        assert!(is_ready(&dir));
        assert_eq!(editor_names(&dir, "src/A.java", text).unwrap(), ["lonely"]);
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let cancel = AtomicBool::new(true);
        assert_eq!(
            editor_unused(
                &dir,
                Path::new("src/A.java"),
                doc.tree().root_node(),
                text,
                &cancel
            ),
            None
        );
        // Neither a Java nor a Kotlin file: none.
        assert_eq!(editor_names(&dir, "src/A.groovy", text), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refresh as the window's, without writing to the user cache.
    fn with_project_index_uncached(root: &Path, files: &[PathBuf]) {
        let mut index = ReferenceIndex::new(root);
        index.refresh(files);
        hold(index);
    }

    const SERVICE_KT: &str = r#"package zoo

import java.io.Serializable

class Service(val name: String, private val secret: Int, val spare: Int) : Serializable {
    val size: Int = 0
    var isOn: Boolean = false
    fun helper() = lonely() + secret
    fun lonely() = 1
    fun alone() {}
    internal fun inner() {}
    override fun toString() = name
    operator fun plus(o: Service) = this
    infix fun shl(o: Int) = o
    @Test fun works() {}
    fun readResolve(): Any = this
    companion object {
        const val LIMIT = 3
        fun create() = Service("a", 1, 2)
        private const val serialVersionUID = 1L
    }
    object Registry {
        fun all() {}
    }
    class Nested
    enum class Mode {
        A,
        B {
            override fun toString() = "b"
        },
    }
    val task = object : Runnable {
        override fun run() {}
        fun extra() {}
    }
}

fun main() { Service("x", 1, 2).helper() }
fun Application.module() {}
fun topUnused() { fun local() {}; local() }
val topProp = 1
data class Point(val x: Int, val y: Int)
annotation class Tag(val value: String)
@Serializable class Dto(val id: Int) {
    val extra = 1
}
expect fun platform(): String
interface Port {
    fun send()
}
object Single
private fun hidden() {}
"#;

    const USE_JAVA: &str = "class UseKt {\n    void f(zoo.Service s) { s.getSize(); s.setOn(true); int n = zoo.Service.LIMIT; }\n}\n";

    // SPEC-0316/AK-10 (v4 behaviours 9 and 10)
    #[test]
    fn kotlin_declarations_and_entry_points() {
        let (dir, files) = project(&[
            ("src/zoo/Service.kt", SERVICE_KT),
            ("src/UseKt.java", USE_JAVA),
            ("build.gradle.kts", "plugins { kotlin(\"jvm\") }\n"),
        ]);
        let mut index = ReferenceIndex::new(&dir);
        let stats = index.refresh(&files);
        assert_eq!(
            (stats.files, stats.java_files, stats.kotlin_files),
            (3, 1, 1)
        );
        let kt = &index.files[&PathBuf::from("src/zoo/Service.kt")];
        let kind = |name: &str| {
            kt.declarations
                .iter()
                .find(|d| d.name == name)
                .map(|d| (d.kind, d.container.as_str()))
        };
        assert_eq!(kind("Service"), Some((Kind::Class, "")));
        assert_eq!(kind("spare"), Some((Kind::Property, "Service")));
        assert_eq!(kind("create"), Some((Kind::Function, "Companion")));
        assert_eq!(kind("Registry"), Some((Kind::Object, "Service")));
        assert_eq!(kind("Mode"), Some((Kind::EnumClass, "Service")));
        assert_eq!(kind("Tag"), Some((Kind::AnnotationClass, "")));
        assert_eq!(kind("Port"), Some((Kind::Interface, "")));
        // Locals, enum entries and the members of object expressions are not declarations.
        assert_eq!(kind("local"), None);
        assert_eq!(kind("A"), None);
        assert_eq!(kind("extra").map(|k| k.1), Some("Dto"));
        assert!(kt.declarations.iter().all(|d| d.name != "run"));
        // Imports and the package are no uses; a type in the supertype list is.
        assert!(!kt.uses.contains_key("io"));
        assert!(kt.uses.contains_key("Serializable"));
        let service = kt.declarations.iter().find(|d| d.name == "spare").unwrap();
        assert_eq!((service.line, service.column), (4, 61));
        assert_eq!(
            unused_names(&index),
            [
                ":Point",
                ":Port",
                ":Single",
                ":Tag",
                ":UseKt",
                ":topProp",
                ":topUnused",
                "Companion:create",
                "Port:send",
                "Registry:all",
                "Service:Mode",
                "Service:Nested",
                "Service:Registry",
                "Service:alone",
                "Service:inner",
                "Service:shl",
                "Service:spare",
                "Service:task",
                "UseKt:f",
            ]
        );
        let msgs: Vec<String> = index.unused(|_| true).iter().map(Unused::message).collect();
        for m in [
            "Function 'alone' is never used",
            "Property 'spare' is never used",
            "Object 'Registry' is never used",
            "Enum class 'Mode' is never used",
            "Annotation class 'Tag' is never used",
            "Interface 'Port' is never used",
        ] {
            assert!(msgs.contains(&m.to_owned()), "{m}: {msgs:?}");
        }
        // Inspect Code: the Kotlin id, `@Suppress` on a class keeps its members out.
        let quiet = SERVICE_KT.replace(
            "    object Registry",
            "    @Suppress(\"unused\")\n    object Registry",
        );
        std::fs::write(dir.join("src/zoo/Service.kt"), quiet).unwrap();
        let found = unused_findings(&dir, &files, &[PathBuf::from("src/zoo/Service.kt")]);
        assert!(found.iter().all(|p| p.id == KOTLIN_UNUSED_DECLARATION));
        assert!(
            found
                .iter()
                .any(|p| p.message == "Function 'alone' is never used")
        );
        assert!(
            !found
                .iter()
                .any(|p| p.message.contains("Registry")
                    || p.message == "Function 'all' is never used")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0316/AK-10 (v4 behaviour 9): uses across the languages, both ways.
    #[test]
    fn uses_across_java_and_kotlin() {
        let person = "public class Person {\n    public String getTitle() { return \"\"; }\n    public void setTitle(String t) {}\n    public boolean isActive() { return true; }\n    public void setActive(boolean a) {}\n    public void calledFromKotlin() {}\n    public void lonelyJava() {}\n}\n";
        let caller = "public class Caller {\n    public static void main(String[] a) { Util.INSTANCE.fromJava(); UtilKt.helperKt(); new Box().getLabel(); }\n}\n";
        let util = "object Util {\n    fun fromJava() {}\n    fun notFromJava() {}\n}\nfun helperKt() {}\nclass Box { val label = \"\"; val hidden = 0 }\nfun use(p: Person) {\n    println(\"${p.title} $p\")\n    p.calledFromKotlin()\n    p.isActive = true\n}\n";
        let (dir, files) = project(&[
            ("src/Person.java", person),
            ("src/Caller.java", caller),
            ("src/Util.kt", util),
        ]);
        let mut index = ReferenceIndex::new(&dir);
        index.refresh(&files);
        assert_eq!(
            unused_names(&index),
            [
                ":use",
                "Box:hidden",
                "Person:lonelyJava",
                "Util:notFromJava"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn kotlin_editor_names(root: &Path, rel: &str, text: &str) -> Option<Vec<String>> {
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, text).unwrap();
        let cancel = AtomicBool::new(false);
        editor_unused(root, Path::new(rel), doc.tree().root_node(), text, &cancel).map(|v| {
            v.into_iter()
                .map(|f| text[f.start..f.end].to_owned())
                .collect()
        })
    }

    // SPEC-0098/AK-17 (v13 behaviour 27)
    #[test]
    fn kotlin_editor_findings() {
        let (dir, files) = project(&[
            ("src/zoo/Service.kt", SERVICE_KT),
            ("src/UseKt.java", USE_JAVA),
        ]);
        let mut index = ReferenceIndex::new(&dir);
        index.refresh(&files);
        hold(index);
        // Top-level classes, interfaces and objects are left out; top-level functions and
        // properties are not.
        assert_eq!(
            kotlin_editor_names(&dir, "src/zoo/Service.kt", SERVICE_KT).unwrap(),
            [
                "spare",
                "alone",
                "inner",
                "shl",
                "create",
                "Registry",
                "all",
                "Nested",
                "Mode",
                "task",
                "topUnused",
                "topProp",
                "send"
            ]
        );
        // The unsaved text counts; the Java file's uses of the saved state count.
        let typed = SERVICE_KT.replace("fun main() {", "fun main() { Service.create(); alone();");
        assert_eq!(
            kotlin_editor_names(&dir, "src/zoo/Service.kt", &typed).unwrap(),
            [
                "spare",
                "inner",
                "shl",
                "Registry",
                "all",
                "Nested",
                "Mode",
                "task",
                "topUnused",
                "topProp",
                "send"
            ]
        );
        let without_java_use = SERVICE_KT.replace("val size", "val size2");
        assert!(
            kotlin_editor_names(&dir, "src/zoo/Service.kt", &without_java_use)
                .unwrap()
                .contains(&"size2".to_owned())
        );
        // The Kotlin id, the Safe Delete fix, `@Suppress` and `@file:Suppress`.
        let doc = SyntaxDocument::new(LanguageId::Kotlin, false, SERVICE_KT).unwrap();
        let found = editor_unused(
            &dir,
            Path::new("src/zoo/Service.kt"),
            doc.tree().root_node(),
            SERVICE_KT,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(found[0].id, KOTLIN_UNUSED_DECLARATION);
        assert_eq!(found[0].message, "Property 'spare' is never used");
        assert_eq!(
            found[1].fix.as_ref().map(|f| (f.title.as_str(), f.action)),
            Some(("Safe delete 'alone'", Some(SAFE_DELETE)))
        );
        let hushed = SERVICE_KT.replace("    fun alone()", "    @Suppress(\"unused\") fun alone()");
        assert!(
            !kotlin_editor_names(&dir, "src/zoo/Service.kt", &hushed)
                .unwrap()
                .contains(&"alone".to_owned())
        );
        let file_wide = format!("@file:Suppress(\"unused\")\n{SERVICE_KT}");
        assert_eq!(
            kotlin_editor_names(&dir, "src/zoo/Service.kt", &file_wide).unwrap(),
            Vec::<String>::new()
        );
        // The Java file's own declarations against the Kotlin file's uses.
        assert_eq!(
            editor_names(&dir, "src/UseKt.java", USE_JAVA).unwrap(),
            ["f"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SPEC-0316/AK-2
    #[test]
    fn refresh_is_incremental_and_cached() {
        let (dir, mut files) = project(&[
            ("src/A.java", "class A { void f() { new B().g(); } }\n"),
            ("src/B.java", "class B { void g() {} }\n"),
        ]);
        let mut index = ReferenceIndex::new(&dir);
        assert_eq!(index.refresh(&files).read, 2);
        let again = index.refresh(&files);
        assert_eq!((again.read, again.removed, again.files), (0, 0, 2));
        // A changed file (other size) is read again, a removed one dropped, a new one added.
        std::fs::write(
            dir.join("src/B.java"),
            "class B { void g() {} void h() {} }\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/C.java"), "class C {}\n").unwrap();
        files.push(PathBuf::from("src/C.java"));
        files.retain(|f| f != Path::new("src/A.java"));
        let changed = index.refresh(&files);
        assert_eq!((changed.read, changed.removed, changed.files), (2, 1, 2));
        // Saved and loaded: nothing to read.
        let cache = dir.join("cache/index.json");
        index.save(&cache).unwrap();
        let mut loaded = ReferenceIndex::load(&cache, &dir);
        assert_eq!(loaded, index);
        assert_eq!(loaded.refresh(&files).read, 0);
        // Another project or format: empty.
        assert!(
            ReferenceIndex::load(&cache, Path::new("/elsewhere"))
                .files
                .is_empty()
        );
        let mut old = index.clone();
        old.format = FORMAT + 1;
        std::fs::write(&cache, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(ReferenceIndex::load(&cache, &dir).files.is_empty());
        // Saving removes the index of a project folder that is gone.
        let gone = ReferenceIndex::new(&dir.join("gone"));
        let gone_file = dir.join("cache/gone.json");
        std::fs::write(&gone_file, serde_json::to_vec(&gone).unwrap()).unwrap();
        index.save(&cache).unwrap();
        assert!(!gone_file.exists());
        assert!(cache.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
