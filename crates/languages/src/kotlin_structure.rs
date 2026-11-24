//! Kotlin class structure from the syntax tree (`SPEC-0082` v6, `SPEC-0122` v6, `SPEC-0292` v6,
//! `SPEC-0320` v3): the type hierarchy, the method hierarchy, the super member, the members a
//! class can override or must implement, gutter marks and the counts of inheritors and
//! implementations. kotlin-language-server offers neither a type hierarchy nor implementations, so
//! the `.kt` files of the project are parsed and names are resolved within the project: by file,
//! package and import, never into libraries; overloads are told apart by their parameter count.
//! Only the usage counts ask the server (`textDocument/references`).

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;
use tree_sitter::Node;

use crate::insight::{CallNode, Location, Opened, Range};
use crate::java_structure::{OverridableMethod, SuperTarget, TypeDirection, offset_of};
use crate::jvm::{Jvm, parse};
use crate::lsp::LspError;
use crate::rename::{Edit, FileEdits};
use crate::ts_structure::{DeclarationMark, MarkKind, MarkTarget, place_of};
use crate::usages::{Counted, Plan, UsageCount, distinct_places};

/// At most this many nodes in a hierarchy.
const NODE_LIMIT: usize = 500;
/// At most this many Kotlin files read per project.
const MAX_FILES: usize = 5000;
/// Files larger than this are left out.
const MAX_FILE_BYTES: u64 = 1_000_000;
/// Folders never searched for sources.
const SKIPPED_DIRS: &[&str] = &["build", "out", "target", "node_modules", "bin"];

/// Whether `path` is a Kotlin source file.
pub fn is_kotlin(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("kt")
}

// ------------------------------------------------------------------ the syntax tree of one file

/// What kind of type a declaration is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KtKind {
    /// `class` (also `data`, `sealed`, `abstract`, `annotation`).
    Class,
    /// `interface`.
    Interface,
    /// `object` and `companion object`.
    Object,
    /// `enum class`.
    Enum,
}

impl KtKind {
    fn name(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Object => "object",
            Self::Enum => "enum",
        }
    }
}

/// A parameter of a function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KtParam {
    /// Name.
    pub name: String,
    /// Type as written.
    pub ty: String,
    /// `vararg`.
    pub vararg: bool,
}

/// A function or property of a type, or of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KtMember {
    /// Name.
    pub name: String,
    /// A property (`val`/`var`, also in the primary constructor) rather than a function.
    pub property: bool,
    /// Parameters of a function.
    pub params: Vec<KtParam>,
    /// Return type or property type as written; `None` when inferred or `Unit`.
    pub ty: Option<String>,
    /// Type parameters as written (`<T>`).
    pub type_params: Option<String>,
    /// An extension receiver.
    pub receiver: bool,
    /// `var`.
    pub mutable: bool,
    /// `suspend`.
    pub suspend: bool,
    /// Without an implementation: `abstract`, or an interface member without body, initializer
    /// or getter.
    pub is_abstract: bool,
    /// Declared with `override`.
    pub is_override: bool,
    /// Can be overridden: in an interface, or `open`, `abstract` or a not final `override`; never
    /// `private`.
    pub overridable: bool,
    /// `private`.
    pub is_private: bool,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column of the name, in characters.
    pub column: usize,
    /// Bytes of the declaration.
    pub start: usize,
    /// End of the declaration.
    pub end: usize,
}

impl KtMember {
    /// Whether `other` is the same member by kind, name and parameter count.
    pub fn same(&self, other: &KtMember) -> bool {
        self.property == other.property
            && self.name == other.name
            && self.params.len() == other.params.len()
    }

    /// `name(Type, …)` of a function, `name: Type` of a property.
    pub fn label(&self) -> String {
        if self.property {
            match &self.ty {
                Some(t) => format!("{}: {t}", self.name),
                None => self.name.clone(),
            }
        } else {
            let params: Vec<&str> = self.params.iter().map(|p| p.ty.as_str()).collect();
            match &self.ty {
                Some(t) => format!("{}({}): {t}", self.name, params.join(", ")),
                None => format!("{}({})", self.name, params.join(", ")),
            }
        }
    }

    fn name_range(&self) -> Range {
        name_range(self.line, self.column, &self.name)
    }
}

/// A name in the supertype list of a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KtSuper {
    /// Simple name (the last part of `a.Base`).
    pub name: String,
    /// The name as written without type arguments.
    pub written: String,
    /// Written as a constructor call (`Base()`), so it is a class.
    pub call: bool,
    /// Its type arguments as written (`String` of `Store<String>`).
    pub args: Vec<String>,
    /// 0-based line of the simple name.
    pub line: usize,
    /// 0-based column of the simple name.
    pub column: usize,
}

/// A type declaration of a Kotlin file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KtType {
    /// Simple name (`Companion` for an unnamed companion object).
    pub name: String,
    /// Class, interface, object or enum.
    pub kind: KtKind,
    /// Cannot be instantiated as it is: `abstract`, `sealed` or an interface.
    pub is_abstract: bool,
    /// The names of the types around it, outermost first.
    pub outer: Vec<String>,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based column of the name, in characters.
    pub column: usize,
    /// The names of its type parameters.
    pub type_params: Vec<String>,
    /// The supertypes as written.
    pub supertypes: Vec<KtSuper>,
    /// Functions and properties, constructor properties first.
    pub members: Vec<KtMember>,
    /// Enum entries: name, line and column.
    pub entries: Vec<(String, usize, usize)>,
    /// Bytes of the declaration.
    pub start: usize,
    /// End of the declaration.
    pub end: usize,
    /// Bytes of the body with its braces.
    pub body: Option<(usize, usize)>,
}

impl KtType {
    fn name_range(&self) -> Range {
        name_range(self.line, self.column, &self.name)
    }

    /// The member like `m` declared in this type.
    pub fn member(&self, m: &KtMember) -> Option<&KtMember> {
        self.members.iter().find(|x| x.same(m))
    }

    /// `Outer.Name`.
    fn nested_name(&self) -> String {
        let mut parts = self.outer.clone();
        parts.push(self.name.clone());
        parts.join(".")
    }
}

/// A parsed Kotlin file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KtFile {
    /// The file.
    pub path: PathBuf,
    /// Its text.
    pub text: String,
    /// The package (empty for none).
    pub package: String,
    /// Imported names as written (`a.B`, `a.*`).
    pub imports: Vec<String>,
    /// Types in document order, nested ones included.
    pub types: Vec<KtType>,
    /// Functions and properties at file level.
    pub top_level: Vec<KtMember>,
}

fn name_range(line: usize, column: usize, name: &str) -> Range {
    Range {
        start_line: line as u64,
        start_character: column as u64,
        end_line: line as u64,
        end_character: (column + name.chars().count()) as u64,
    }
}

fn kids(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn src<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

fn place(n: Node, text: &str) -> (usize, usize) {
    let (line, column) = place_of(text, n.start_byte());
    (line as usize, column as usize)
}

fn child<'t>(n: Node<'t>, kind: &str) -> Option<Node<'t>> {
    kids(n).into_iter().find(|c| c.kind() == kind)
}

/// The modifier words of a declaration, annotations left out.
fn modifier_words<'a>(n: Node, text: &'a str) -> Vec<&'a str> {
    let Some(m) = child(n, "modifiers") else {
        return Vec::new();
    };
    kids(m)
        .into_iter()
        .filter(|c| !c.kind().contains("annotation"))
        .map(|c| src(c, text).trim())
        .collect()
}

fn is_type_node(n: Node) -> bool {
    n.is_named() && n.kind().ends_with("type") && n.kind() != "type_parameters"
}

fn function_decl(n: Node, text: &str, in_interface: bool) -> Option<KtMember> {
    let name = n.child_by_field_name("name")?;
    let words = modifier_words(n, text);
    let has = |w: &str| words.contains(&w);
    let mut params = Vec::new();
    let mut ty = None;
    let mut receiver = false;
    let mut seen_params = false;
    for c in kids(n) {
        match c.kind() {
            "function_value_parameters" => {
                seen_params = true;
                let mut vararg = false;
                for p in kids(c) {
                    match p.kind() {
                        "parameter_modifiers" => vararg = src(p, text).contains("vararg"),
                        "parameter" => {
                            let parts = kids(p);
                            let pname = parts.iter().find(|x| x.kind() == "identifier");
                            let pty = parts.iter().find(|x| is_type_node(**x));
                            params.push(KtParam {
                                name: pname.map(|x| src(*x, text).to_owned()).unwrap_or_default(),
                                ty: pty.map(|x| src(*x, text).to_owned()).unwrap_or_default(),
                                vararg,
                            });
                            vararg = false;
                        }
                        _ => {}
                    }
                }
            }
            _ if is_type_node(c) && c.start_byte() < name.start_byte() => receiver = true,
            _ if is_type_node(c) && seen_params && ty.is_none() => {
                ty = Some(src(c, text).to_owned());
            }
            _ => {}
        }
    }
    let has_body = child(n, "function_body").is_some();
    let (line, column) = place(name, text);
    let is_private = has("private");
    Some(KtMember {
        name: src(name, text).to_owned(),
        property: false,
        params,
        ty: ty.filter(|t| t != "Unit"),
        type_params: child(n, "type_parameters").map(|t| src(t, text).to_owned()),
        receiver,
        mutable: false,
        suspend: has("suspend"),
        is_abstract: has("abstract") || (in_interface && !has_body),
        is_override: has("override"),
        overridable: !is_private
            && !has("final")
            && (in_interface || has("open") || has("abstract") || has("override")),
        is_private,
        line,
        column,
        start: n.start_byte(),
        end: n.end_byte(),
    })
}

fn property_decl(n: Node, text: &str, in_interface: bool) -> Option<KtMember> {
    let parts = kids(n);
    let var = parts.iter().find(|c| c.kind() == "variable_declaration")?;
    let var_parts = kids(*var);
    let name = var_parts.iter().find(|c| c.kind() == "identifier")?;
    let ty = var_parts.iter().find(|c| is_type_node(**c));
    let words = modifier_words(n, text);
    let has = |w: &str| words.contains(&w);
    // Anything after the name but a setter makes the property concrete: an initializer, a
    // delegate or a getter.
    let implemented = parts
        .iter()
        .any(|c| c.start_byte() > var.end_byte() && c.is_named() && !matches!(c.kind(), "setter"));
    let receiver = parts
        .iter()
        .any(|c| is_type_node(*c) && c.start_byte() < var.start_byte());
    let (line, column) = place(*name, text);
    let is_private = has("private");
    Some(KtMember {
        name: src(*name, text).to_owned(),
        property: true,
        params: Vec::new(),
        ty: ty.map(|t| src(*t, text).to_owned()),
        type_params: None,
        receiver,
        mutable: parts.iter().any(|c| c.kind() == "var"),
        suspend: false,
        is_abstract: has("abstract") || (in_interface && !implemented),
        is_override: has("override"),
        overridable: !is_private
            && !has("final")
            && !has("const")
            && (in_interface || has("open") || has("abstract") || has("override")),
        is_private,
        line,
        column,
        start: n.start_byte(),
        end: n.end_byte(),
    })
}

/// A `val`/`var` parameter of the primary constructor as a property.
fn constructor_property(p: Node, text: &str) -> Option<KtMember> {
    let parts = kids(p);
    let mutable = parts.iter().any(|c| c.kind() == "var");
    if !mutable && !parts.iter().any(|c| c.kind() == "val") {
        return None;
    }
    let name = parts.iter().find(|c| c.kind() == "identifier")?;
    let ty = parts.iter().find(|c| is_type_node(**c));
    let words = modifier_words(p, text);
    let has = |w: &str| words.contains(&w);
    let (line, column) = place(*name, text);
    Some(KtMember {
        name: src(*name, text).to_owned(),
        property: true,
        params: Vec::new(),
        ty: ty.map(|t| src(*t, text).to_owned()),
        type_params: None,
        receiver: false,
        mutable,
        suspend: false,
        is_abstract: false,
        is_override: has("override"),
        overridable: !has("private") && !has("final") && (has("open") || has("override")),
        is_private: has("private"),
        line,
        column,
        start: p.start_byte(),
        end: p.end_byte(),
    })
}

fn supertypes(n: Node, text: &str) -> Vec<KtSuper> {
    let Some(list) = child(n, "delegation_specifiers") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for spec in kids(list) {
        if spec.kind() != "delegation_specifier" {
            continue;
        }
        // `Base(…)`, `Base` or `Base by x`: the first user type below says which type.
        let mut call = false;
        let mut user = None;
        let mut stack = vec![spec];
        while let Some(x) = stack.pop() {
            if x.kind() == "user_type" {
                user = Some(x);
                break;
            }
            if x.kind() == "constructor_invocation" {
                call = true;
            }
            let mut below = kids(x);
            below.reverse();
            stack.extend(below);
        }
        let Some(user) = user else {
            continue;
        };
        let names: Vec<Node> = kids(user)
            .into_iter()
            .filter(|c| c.kind() == "identifier")
            .collect();
        let Some(last) = names.last() else {
            continue;
        };
        let (line, column) = place(*last, text);
        out.push(KtSuper {
            name: src(*last, text).to_owned(),
            written: names
                .iter()
                .map(|x| src(*x, text))
                .collect::<Vec<_>>()
                .join("."),
            call,
            args: child(user, "type_arguments")
                .map(|list| {
                    kids(list)
                        .into_iter()
                        .filter(|c| c.is_named())
                        .map(|c| src(c, text).to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            line,
            column,
        });
    }
    out
}

fn type_decl(n: Node, text: &str, outer: &[String]) -> Option<KtType> {
    let parts = kids(n);
    let words = modifier_words(n, text);
    let has = |w: &str| words.contains(&w);
    let kind = match n.kind() {
        "class_declaration" if parts.iter().any(|c| c.kind() == "interface") => KtKind::Interface,
        "class_declaration" if has("enum") => KtKind::Enum,
        "class_declaration" => KtKind::Class,
        "object_declaration" | "companion_object" => KtKind::Object,
        _ => return None,
    };
    let name_node = n
        .child_by_field_name("name")
        .or_else(|| parts.iter().find(|c| c.kind() == "identifier").copied());
    let (name, line, column) = match name_node {
        Some(node) => {
            let (line, column) = place(node, text);
            (src(node, text).to_owned(), line, column)
        }
        None if n.kind() == "companion_object" => {
            // The word `object` stands for the name.
            let at = parts.iter().find(|c| c.kind() == "object").copied()?;
            let (line, column) = place(at, text);
            ("Companion".to_owned(), line, column)
        }
        None => return None,
    };
    let in_interface = kind == KtKind::Interface;
    let mut members = Vec::new();
    let mut entries = Vec::new();
    let mut body = None;
    for part in &parts {
        match part.kind() {
            "primary_constructor" => {
                for list in kids(*part) {
                    for p in kids(list) {
                        if p.kind() == "class_parameter"
                            && let Some(m) = constructor_property(p, text)
                        {
                            members.push(m);
                        }
                    }
                }
            }
            "class_body" | "enum_class_body" => {
                body = Some((part.start_byte(), part.end_byte()));
                for m in kids(*part) {
                    match m.kind() {
                        "function_declaration" => {
                            members.extend(function_decl(m, text, in_interface));
                        }
                        "property_declaration" => {
                            members.extend(property_decl(m, text, in_interface));
                        }
                        "enum_entry" => {
                            if let Some(id) = child(m, "identifier") {
                                let (l, c) = place(id, text);
                                entries.push((src(id, text).to_owned(), l, c));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Some(KtType {
        name,
        kind,
        is_abstract: in_interface || has("abstract") || has("sealed"),
        outer: outer.to_vec(),
        line,
        column,
        type_params: child(n, "type_parameters")
            .map(|list| {
                kids(list)
                    .into_iter()
                    .filter(|c| c.kind() == "type_parameter")
                    .filter_map(|c| child(c, "identifier"))
                    .map(|c| src(c, text).to_owned())
                    .collect()
            })
            .unwrap_or_default(),
        supertypes: supertypes(n, text),
        members,
        entries,
        start: n.start_byte(),
        end: n.end_byte(),
        body,
    })
}

/// Parses a Kotlin file.
pub fn parse_file(path: &Path, text: &str) -> Option<KtFile> {
    fn visit(
        n: Node,
        text: &str,
        outer: &mut Vec<String>,
        top: bool,
        file: &mut KtFile,
        depth: usize,
    ) {
        if depth > 200 {
            return;
        }
        let mut pushed = false;
        match n.kind() {
            "class_declaration" | "object_declaration" | "companion_object" => {
                if let Some(t) = type_decl(n, text, outer) {
                    outer.push(t.name.clone());
                    pushed = true;
                    file.types.push(t);
                }
            }
            "function_declaration" if top => {
                file.top_level.extend(function_decl(n, text, false));
            }
            "property_declaration" if top => {
                file.top_level.extend(property_decl(n, text, false));
            }
            "package_header" => {
                if let Some(q) = child(n, "qualified_identifier") {
                    file.package = src(q, text).split_whitespace().collect();
                }
            }
            "import" => {
                let written = src(n, text).trim_start_matches("import").trim();
                let written = written.split(" as ").next().unwrap_or(written);
                if !written.is_empty() {
                    file.imports.push(written.split_whitespace().collect());
                }
            }
            _ => {}
        }
        let below_top = top && n.kind() == "source_file";
        for c in kids(n) {
            if c.is_named() {
                visit(c, text, outer, below_top, file, depth + 1);
            }
        }
        if pushed {
            outer.pop();
        }
    }
    let tree = parse(Jvm::Kotlin, text)?;
    let mut file = KtFile {
        path: path.to_path_buf(),
        text: text.to_owned(),
        package: String::new(),
        imports: Vec::new(),
        types: Vec::new(),
        top_level: Vec::new(),
    };
    visit(tree.root_node(), text, &mut Vec::new(), true, &mut file, 0);
    Some(file)
}

impl KtFile {
    /// The innermost type around byte `offset` (its header or body).
    pub fn type_at(&self, offset: usize) -> Option<usize> {
        self.types
            .iter()
            .enumerate()
            .filter(|(_, t)| t.start <= offset && offset <= t.end)
            .min_by_key(|(_, t)| t.end - t.start)
            .map(|(i, _)| i)
    }

    /// The member of the innermost type around `offset` that `offset` lies in.
    pub fn member_at(&self, offset: usize) -> Option<(usize, usize)> {
        let t = self.type_at(offset)?;
        let m = self.types[t]
            .members
            .iter()
            .position(|m| m.start <= offset && offset <= m.end)?;
        Some((t, m))
    }
}

// ------------------------------------------------------------------------------- the project

/// A type of the project: file and type index.
type TypeRef = (usize, usize);

/// The Kotlin files of a project.
#[derive(Debug, Clone, Default)]
pub struct KtProject {
    /// The parsed files.
    pub files: Vec<Arc<KtFile>>,
}

/// Files parsed from disk, by path, with the modification time and length they had: a hierarchy,
/// the gutter marks and the counts are asked for on every save, and only changed files are read
/// again.
type Parsed = std::collections::HashMap<PathBuf, (SystemTime, u64, Arc<KtFile>)>;

fn parsed() -> &'static Mutex<Parsed> {
    static PARSED: OnceLock<Mutex<Parsed>> = OnceLock::new();
    PARSED.get_or_init(Default::default)
}

/// The file at `path` as it is on disk, parsed, from the cache while it has not changed.
fn from_disk(path: &Path) -> Option<Arc<KtFile>> {
    let meta = std::fs::metadata(path).ok()?;
    let stamp = (meta.modified().ok()?, meta.len());
    {
        let cache = parsed().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((modified, len, file)) = cache.get(path)
            && (*modified, *len) == stamp
        {
            return Some(Arc::clone(file));
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    let file = Arc::new(parse_file(path, &text)?);
    let mut cache = parsed().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 4 * MAX_FILES {
        cache.clear();
    }
    cache.insert(path.to_path_buf(), (stamp.0, stamp.1, Arc::clone(&file)));
    Some(file)
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn kotlin_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 40 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let Ok(kind) = e.file_type() else {
            continue;
        };
        let name = e.file_name();
        let name = name.to_string_lossy();
        let path = e.path();
        if kind.is_dir() {
            if !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_ref()) {
                kotlin_files(&path, out, depth + 1);
            }
        } else if kind.is_file()
            && is_kotlin(&path)
            && e.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
        {
            if out.len() >= MAX_FILES {
                return;
            }
            out.push(path);
        }
    }
}

impl KtProject {
    /// Reads the `.kt` files below `root` (hidden and build folders left out); `texts` gives the
    /// unsaved text of open files, and `file` with `text` is read as given even outside `root`.
    pub fn load(
        root: &Path,
        file: &Path,
        text: Option<&str>,
        texts: &dyn Fn(&Path) -> Option<String>,
    ) -> Self {
        let mut paths = Vec::new();
        kotlin_files(&canonical(root), &mut paths, 0);
        let file = canonical(file);
        if !paths.contains(&file) {
            paths.push(file.clone());
        }
        let files = paths
            .into_iter()
            .filter_map(|p| {
                let unsaved = if p == file && text.is_some() {
                    text.map(str::to_owned)
                } else {
                    texts(&p)
                };
                match unsaved {
                    Some(content) => parse_file(&p, &content).map(Arc::new),
                    None => from_disk(&p),
                }
            })
            .collect();
        Self { files }
    }

    /// A project of the given files (tests).
    pub fn of(files: &[(&str, &str)]) -> Self {
        Self {
            files: files
                .iter()
                .filter_map(|(p, t)| parse_file(Path::new(p), t))
                .map(Arc::new)
                .collect(),
        }
    }

    /// The index of the file at `path`.
    pub fn file(&self, path: &Path) -> Option<usize> {
        let path = canonical(path);
        self.files.iter().position(|f| f.path == path)
    }

    fn ty(&self, r: TypeRef) -> &KtType {
        &self.files[r.0].types[r.1]
    }

    fn qualified(&self, r: TypeRef) -> String {
        let (f, t) = (&self.files[r.0], self.ty(r));
        if f.package.is_empty() {
            t.nested_name()
        } else {
            format!("{}.{}", f.package, t.nested_name())
        }
    }

    /// The project type the supertype `s` written in file `from` means: one of the same file, of
    /// the same package or an imported one. `None` for anything else (a library type).
    fn resolve(&self, from: usize, s: &KtSuper) -> Option<TypeRef> {
        let here = &self.files[from];
        let qualified = s.written.contains('.');
        let first = s.written.split('.').next().unwrap_or(&s.written);
        let mut same_package = None;
        let mut imported = None;
        for (fi, f) in self.files.iter().enumerate() {
            for (ti, t) in f.types.iter().enumerate() {
                if t.name != s.name {
                    continue;
                }
                let full = self.qualified((fi, ti));
                // `Outer.Inner` or `pkg.Name` must end the type's full name.
                if qualified && full != s.written && !full.ends_with(&format!(".{}", s.written)) {
                    continue;
                }
                if fi == from {
                    return Some((fi, ti));
                }
                if f.package == here.package && (t.outer.is_empty() || qualified) {
                    same_package.get_or_insert((fi, ti));
                    continue;
                }
                let by_import = here.imports.iter().any(|i| {
                    *i == full
                        || (qualified
                            && i.ends_with(&format!(".{first}"))
                            && full.starts_with(&format!("{i}.")))
                        || i.strip_suffix(".*").is_some_and(|p| {
                            full.strip_prefix(p)
                                .and_then(|rest| rest.strip_prefix('.'))
                                .is_some_and(|rest| rest == s.written)
                        })
                });
                if by_import || full == s.written {
                    imported.get_or_insert((fi, ti));
                }
            }
        }
        same_package.or(imported)
    }

    /// The direct supertypes of `r` with the project type each means.
    fn supers(&self, r: TypeRef) -> Vec<(&KtSuper, Option<TypeRef>)> {
        self.ty(r)
            .supertypes
            .iter()
            .map(|s| (s, self.resolve(r.0, s).filter(|x| *x != r)))
            .collect()
    }

    /// The direct subtypes of `r` in the project, in file and document order.
    fn subs(&self, r: TypeRef) -> Vec<TypeRef> {
        let name = &self.ty(r).name;
        let mut out = Vec::new();
        for (fi, f) in self.files.iter().enumerate() {
            for (ti, t) in f.types.iter().enumerate() {
                if (fi, ti) != r
                    && t.supertypes
                        .iter()
                        .any(|s| s.name == *name && self.resolve(fi, s) == Some(r))
                {
                    out.push((fi, ti));
                }
            }
        }
        out
    }

    /// Every project type above (`up`) or below `r`, nearest first.
    fn reach(&self, r: TypeRef, up: bool) -> Vec<TypeRef> {
        let mut seen = HashSet::from([r]);
        let mut queue = VecDeque::from([r]);
        let mut out = Vec::new();
        while let Some(at) = queue.pop_front() {
            let next: Vec<TypeRef> = if up {
                self.supers(at).into_iter().filter_map(|(_, x)| x).collect()
            } else {
                self.subs(at)
            };
            for n in next {
                if seen.insert(n) && out.len() < NODE_LIMIT {
                    out.push(n);
                    queue.push_back(n);
                }
            }
        }
        out
    }

    /// Every project type above `r`, nearest first, each with what its type parameters stand
    /// for as `r` sees them (empty when the arguments are not written out).
    fn reach_typed(&self, r: TypeRef) -> Vec<(TypeRef, Vec<(String, String)>)> {
        let mut seen = HashSet::from([r]);
        let mut queue = VecDeque::from([(r, Vec::new())]);
        let mut out: Vec<(TypeRef, Vec<(String, String)>)> = Vec::new();
        while let Some((at, known)) = queue.pop_front() {
            for (s, target) in self.supers(at) {
                let Some(t) = target else {
                    continue;
                };
                if !seen.insert(t) || out.len() >= NODE_LIMIT {
                    continue;
                }
                let params = &self.ty(t).type_params;
                let arguments: Vec<(String, String)> = if params.len() == s.args.len() {
                    params
                        .iter()
                        .zip(&s.args)
                        .map(|(p, a)| (p.clone(), replace_names(a, &known)))
                        .collect()
                } else {
                    Vec::new()
                };
                out.push((t, arguments.clone()));
                queue.push_back((t, arguments));
            }
        }
        out
    }

    fn location(&self, r: TypeRef) -> Location {
        Location {
            path: self.files[r.0].path.clone(),
            range: self.ty(r).name_range(),
        }
    }

    fn node(&self, r: TypeRef) -> CallNode {
        let package = &self.files[r.0].package;
        CallNode {
            name: self.ty(r).name.clone(),
            kind: self.ty(r).kind.name().into(),
            detail: (!package.is_empty()).then(|| package.clone()),
            location: self.location(r),
            call_sites: Vec::new(),
            children: Vec::new(),
            recursive: false,
            mark: None,
        }
    }

    /// A supertype outside the project: a leaf that leads to where it is named.
    fn outside_node(&self, from: usize, s: &KtSuper) -> CallNode {
        CallNode {
            name: s.name.clone(),
            kind: if s.call { "class" } else { "interface" }.into(),
            detail: None,
            location: Location {
                path: self.files[from].path.clone(),
                range: name_range(s.line, s.column, &s.name),
            },
            call_sites: Vec::new(),
            children: Vec::new(),
            recursive: false,
            mark: None,
        }
    }

    fn expand(
        &self,
        r: TypeRef,
        up: bool,
        path: &mut Vec<TypeRef>,
        budget: &mut usize,
    ) -> CallNode {
        *budget = budget.saturating_sub(1);
        let mut node = self.node(r);
        if path.contains(&r) {
            node.recursive = true;
            return node;
        }
        path.push(r);
        if up {
            for (s, target) in self.supers(r) {
                if *budget == 0 {
                    break;
                }
                node.children.push(match target {
                    Some(t) => self.expand(t, up, path, budget),
                    None => {
                        *budget = budget.saturating_sub(1);
                        self.outside_node(r.0, s)
                    }
                });
            }
        } else {
            for t in self.subs(r) {
                if *budget == 0 {
                    break;
                }
                node.children.push(self.expand(t, up, path, budget));
            }
        }
        path.pop();
        node
    }

    /// The type whose name is at a place of file `f`, else the innermost type around it.
    fn type_ref_at(&self, f: usize, line: u64, character: u64) -> Option<TypeRef> {
        let file = &self.files[f];
        let offset = offset_of(&file.text, line as usize, character as usize);
        file.type_at(offset).map(|t| (f, t))
    }

    /// The type hierarchy of the type at (or around) a place (`SPEC-0082` v6 behaviour 17).
    pub fn type_hierarchy(
        &self,
        f: usize,
        line: u64,
        character: u64,
        direction: TypeDirection,
    ) -> Option<CallNode> {
        let r = self.type_ref_at(f, line, character)?;
        let mut budget = NODE_LIMIT;
        let up = direction == TypeDirection::Supertypes;
        let mut tree = self.expand(r, up, &mut Vec::new(), &mut budget);
        tree.mark = Some("*".into());
        if direction != TypeDirection::Class {
            return Some(tree);
        }
        // The chain of superclasses above: project classes, then at most one class outside.
        let mut at = r;
        let mut chain = HashSet::from([r]);
        let mut open_end = true;
        loop {
            let supers = self.supers(at);
            let class = supers.iter().find(|(s, target)| match target {
                Some(t) => self.ty(*t).kind == KtKind::Class,
                None => s.call,
            });
            match class {
                Some((_, Some(t))) if chain.insert(*t) => {
                    let mut parent = self.node(*t);
                    parent.children.push(tree);
                    tree = parent;
                    at = *t;
                }
                Some((s, None)) => {
                    let mut parent = self.outside_node(at.0, s);
                    parent.children.push(tree);
                    tree = parent;
                    open_end = false;
                    break;
                }
                _ => break,
            }
        }
        // Every class ends at `Any`; an interface has no class above.
        if open_end && self.ty(at).kind != KtKind::Interface {
            let mut any = self.node(at);
            any.name = "Any".into();
            any.kind = "class".into();
            any.detail = Some("kotlin".into());
            any.children.push(tree);
            tree = any;
        }
        Some(tree)
    }

    fn mark_members(
        &self,
        r: TypeRef,
        member: &KtMember,
        inherited_body: bool,
        path: &mut Vec<TypeRef>,
        budget: &mut usize,
    ) -> CallNode {
        *budget = budget.saturating_sub(1);
        let t = self.ty(r);
        let own = t.member(member);
        let mut node = self.node(r);
        node.mark = Some(
            match own {
                Some(_) => "+",
                None if !t.is_abstract && !inherited_body => "!",
                None => "-",
            }
            .into(),
        );
        if path.contains(&r) {
            node.recursive = true;
            return node;
        }
        path.push(r);
        let body = inherited_body || own.is_some_and(|m| !m.is_abstract);
        for sub in self.subs(r) {
            if *budget == 0 {
                break;
            }
            node.children
                .push(self.mark_members(sub, member, body, path, budget));
        }
        path.pop();
        node
    }

    /// The method hierarchy of the member at a place (`SPEC-0082` v6 behaviour 17): from the
    /// farthest project supertype that declares it down through every subtype, each marked `+`,
    /// `-` or `!`. `None` outside a member.
    pub fn method_hierarchy(
        &self,
        f: usize,
        line: u64,
        character: u64,
    ) -> Option<(String, CallNode)> {
        let file = &self.files[f];
        let offset = offset_of(&file.text, line as usize, character as usize);
        let (t, m) = file.member_at(offset)?;
        let member = &file.types[t].members[m];
        let top = self
            .reach((f, t), true)
            .into_iter()
            .rfind(|s| self.ty(*s).member(member).is_some())
            .unwrap_or((f, t));
        let mut budget = NODE_LIMIT;
        let tree = self.mark_members(top, member, false, &mut Vec::new(), &mut budget);
        Some((member.name.clone(), tree))
    }

    /// The member the one at a place overrides or implements, or on a type outside a member its
    /// superclass, else its first supertype (`SPEC-0082` v6 behaviour 17).
    pub fn super_target(&self, f: usize, line: u64, character: u64) -> Option<SuperTarget> {
        let file = &self.files[f];
        let offset = offset_of(&file.text, line as usize, character as usize);
        if let Some((t, m)) = file.member_at(offset) {
            let member = &file.types[t].members[m];
            return self.reach((f, t), true).into_iter().find_map(|s| {
                let found = self.ty(s).member(member)?;
                Some(SuperTarget {
                    name: if found.property {
                        format!("{}.{}", self.ty(s).name, found.name)
                    } else {
                        format!("{}.{}()", self.ty(s).name, found.name)
                    },
                    location: Some(Location {
                        path: self.files[s.0].path.clone(),
                        range: found.name_range(),
                    }),
                })
            });
        }
        let r = (f, file.type_at(offset)?);
        let supers = self.supers(r);
        let (s, target) = supers
            .iter()
            .find(|(s, target)| match target {
                Some(t) => self.ty(*t).kind == KtKind::Class,
                None => s.call,
            })
            .or(supers.first())?;
        Some(SuperTarget {
            name: s.name.clone(),
            location: target.map(|t| self.location(t)),
        })
    }

    /// The members the type at a place can override, nearest supertype first, then those of
    /// `kotlin.Any` (`SPEC-0122` v6 behaviour 15); `unimplemented` marks the abstract ones no
    /// supertype implements. The type's qualified name comes first.
    pub fn overridable(
        &self,
        f: usize,
        line: u64,
        character: u64,
    ) -> (String, Vec<OverridableMethod>) {
        let Some(r) = self.type_ref_at(f, line, character) else {
            return (String::new(), Vec::new());
        };
        let own = self.ty(r);
        let typed = self.reach_typed(r);
        let above: Vec<TypeRef> = typed.iter().map(|(t, _)| *t).collect();
        let mut listed: Vec<&KtMember> = Vec::new();
        let mut out = Vec::new();
        for (s, arguments) in &typed {
            let ty = self.ty(*s);
            for m in ty.members.iter().filter(|m| m.overridable && !m.receiver) {
                if own.member(m).is_some() || listed.iter().any(|x| x.same(m)) {
                    continue;
                }
                listed.push(m);
                let implemented = above
                    .iter()
                    .any(|x| self.ty(*x).member(m).is_some_and(|x| !x.is_abstract));
                let unimplemented = m.is_abstract && !implemented;
                // As the type sees it: `T` of `Store<T>` is `String` in `Store<String>`.
                let seen = with_arguments(m, arguments);
                out.push(OverridableMethod {
                    name: m.name.clone(),
                    parameters: seen.params.iter().map(|p| p.ty.clone()).collect(),
                    unimplemented,
                    declaring_class: self.qualified(*s),
                    declaring_class_type: ty.kind.name().into(),
                    return_type: None,
                    raw: json!({"label": seen.label(), "kotlin": stub(&seen, !implemented)}),
                });
            }
        }
        if own.kind != KtKind::Interface {
            for (name, params, ty, body) in [
                (
                    "equals",
                    vec![("other", "Any?")],
                    "Boolean",
                    "return super.equals(other)",
                ),
                ("hashCode", vec![], "Int", "return super.hashCode()"),
                ("toString", vec![], "String", "return super.toString()"),
            ] {
                let declared = |t: &KtType| {
                    t.members
                        .iter()
                        .any(|m| !m.property && m.name == name && m.params.len() == params.len())
                };
                if declared(own) || listed.iter().any(|m| !m.property && m.name == name) {
                    continue;
                }
                let list: Vec<String> = params.iter().map(|(n, t)| format!("{n}: {t}")).collect();
                let types: Vec<String> = params.iter().map(|(_, t)| (*t).to_owned()).collect();
                out.push(OverridableMethod {
                    name: name.into(),
                    parameters: types.clone(),
                    unimplemented: false,
                    declaring_class: "kotlin.Any".into(),
                    declaring_class_type: "class".into(),
                    return_type: None,
                    raw: json!({
                        "label": format!("{name}({}): {ty}", types.join(", ")),
                        "kotlin": format!(
                            "override fun {name}({}): {ty} {{\n    {body}\n}}",
                            list.join(", ")
                        ),
                    }),
                });
            }
        }
        (self.qualified(r), out)
    }

    /// The edits that add `methods` (items of [`Self::overridable`]) at the end of the body of the
    /// type at a place, creating the body when the type has none.
    pub fn add_overridable(
        &self,
        f: usize,
        line: u64,
        character: u64,
        methods: &[OverridableMethod],
    ) -> Result<Vec<FileEdits>, String> {
        let r = self
            .type_ref_at(f, line, character)
            .ok_or("no class at this place")?;
        let (file, t) = (&self.files[f], self.ty(r));
        let stubs: Vec<&str> = methods
            .iter()
            .filter_map(|m| m.raw["kotlin"].as_str())
            .collect();
        if stubs.is_empty() {
            return Err("nothing to add".into());
        }
        let text = &file.text;
        let line_start = text[..t.start].rfind('\n').map_or(0, |i| i + 1);
        let class_indent: String = text[line_start..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let indent = format!("{class_indent}    ");
        let body = stubs
            .iter()
            .map(|s| {
                s.lines()
                    .map(|l| format!("{indent}{l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let (from, to, new_text) = match t.body {
            Some((open, close)) => {
                let inner = &text[open + 1..close - 1];
                let kept = inner.trim_end();
                let gap = if kept.trim().is_empty() { "\n" } else { "\n\n" };
                (
                    open + 1 + kept.len(),
                    close - 1,
                    format!("{gap}{body}\n{class_indent}"),
                )
            }
            None => (t.end, t.end, format!(" {{\n{body}\n{class_indent}}}")),
        };
        let (start_line, start_character) = place_of(text, from);
        let (end_line, end_character) = place_of(text, to);
        Ok(vec![FileEdits {
            path: file.path.clone(),
            edits: vec![Edit {
                range: Range {
                    start_line,
                    start_character,
                    end_line,
                    end_character,
                },
                old_text: text[from..to].to_owned(),
                new_text,
            }],
        }])
    }

    fn member_target(&self, r: TypeRef, m: &KtMember) -> Option<MarkTarget> {
        let found = self.ty(r).member(m)?;
        Some(MarkTarget {
            name: self.ty(r).name.clone(),
            location: Location {
                path: self.files[r.0].path.clone(),
                range: found.name_range(),
            },
        })
    }

    /// The gutter marks of file `f` (`SPEC-0320` v3 behaviour 19).
    pub fn declaration_marks(&self, f: usize) -> Vec<DeclarationMark> {
        let mut out = Vec::new();
        for (ti, t) in self.files[f].types.iter().enumerate() {
            let r = (f, ti);
            let ups = self.reach(r, true);
            let downs = self.reach(r, false);
            if !downs.is_empty() {
                out.push(DeclarationMark {
                    line: t.line as u64,
                    character: t.column as u64,
                    name: t.name.clone(),
                    kind: MarkKind::Inherited,
                    targets: downs
                        .iter()
                        .map(|d| MarkTarget {
                            name: self.ty(*d).name.clone(),
                            location: self.location(*d),
                        })
                        .collect(),
                });
            }
            for m in t.members.iter().filter(|m| !m.is_private) {
                let holders = |list: &[TypeRef]| -> Vec<MarkTarget> {
                    list.iter()
                        .filter_map(|x| self.member_target(*x, m))
                        .collect()
                };
                let first = ups.iter().find_map(|x| {
                    let found = self.ty(*x).member(m)?;
                    Some((self.ty(*x).kind, found.is_abstract))
                });
                if let Some((kind, is_abstract)) = first {
                    out.push(DeclarationMark {
                        line: m.line as u64,
                        character: m.column as u64,
                        name: m.name.clone(),
                        kind: if kind == KtKind::Interface || is_abstract {
                            MarkKind::Implements
                        } else {
                            MarkKind::Overrides
                        },
                        targets: holders(&ups),
                    });
                }
                let below = if m.overridable {
                    holders(&downs)
                } else {
                    Vec::new()
                };
                if !below.is_empty() {
                    out.push(DeclarationMark {
                        line: m.line as u64,
                        character: m.column as u64,
                        name: m.name.clone(),
                        kind: if m.is_abstract {
                            MarkKind::Implemented
                        } else {
                            MarkKind::Overridden
                        },
                        targets: below,
                    });
                }
            }
        }
        out.sort_by_key(|m| (m.line, m.character));
        out
    }

    /// The declarations of file `f` that get a count, with the inheritors of types and the
    /// overriding or implementing members of members, all usage counts zero (`SPEC-0292` v6
    /// behaviour 9).
    pub fn counted_declarations(&self, f: usize) -> Vec<UsageCount> {
        let file = &self.files[f];
        let mut out = Vec::new();
        let entry =
            |name: &str, kind: &'static str, line: usize, column: usize, below: usize| UsageCount {
                inheritors: below,
                ..UsageCount::new(name, kind, line as u64, column as u64)
            };
        let member_kind = |m: &KtMember, in_type: bool| match (m.property, m.is_abstract) {
            (true, true) => "abstract property",
            (true, false) => "property",
            (false, true) => "abstract function",
            (false, false) if in_type => "method",
            (false, false) => "function",
        };
        for (ti, t) in file.types.iter().enumerate() {
            let downs = self.reach((f, ti), false);
            out.push(entry(&t.name, t.kind.name(), t.line, t.column, downs.len()));
            for m in &t.members {
                let below = if m.overridable {
                    downs
                        .iter()
                        .filter(|d| self.ty(**d).member(m).is_some())
                        .count()
                } else {
                    0
                };
                out.push(entry(
                    &m.name,
                    member_kind(m, true),
                    m.line,
                    m.column,
                    below,
                ));
            }
            for (name, line, column) in &t.entries {
                out.push(entry(name, "enum member", *line, *column, 0));
            }
        }
        for m in &file.top_level {
            out.push(entry(&m.name, member_kind(m, false), m.line, m.column, 0));
        }
        // The plan cuts to at most 200 (`SPEC-0292` v8 behaviour 12).
        out.sort_by_key(|u| (u.line, u.character));
        out
    }
}

/// `text` with every whole name that `names` knows replaced.
fn replace_names(text: &str, names: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        match names.iter().find(|(from, _)| from == word) {
            Some((_, to)) => out.push_str(to),
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// `m` with the type parameters of its declaring type replaced by `arguments`; names the member
/// declares as its own type parameters stay.
fn with_arguments(m: &KtMember, arguments: &[(String, String)]) -> KtMember {
    let own = m.type_params.as_deref().unwrap_or_default();
    let arguments: Vec<(String, String)> = arguments
        .iter()
        .filter(|(name, _)| {
            !own.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|w| w == name)
        })
        .cloned()
        .collect();
    let mut seen = m.clone();
    if arguments.is_empty() {
        return seen;
    }
    seen.ty = seen.ty.map(|t| replace_names(&t, &arguments));
    for p in &mut seen.params {
        p.ty = replace_names(&p.ty, &arguments);
    }
    seen
}

/// The text of an overriding member, unindented, its body four spaces in: `TODO()` when nothing
/// above implements it (`todo`), else a call of the supertype's.
fn stub(m: &KtMember, todo: bool) -> String {
    const TODO: &str = "TODO(\"Not yet implemented\")";
    if m.property {
        let keyword = if m.mutable { "var" } else { "val" };
        let ty = m.ty.as_ref().map(|t| format!(": {t}")).unwrap_or_default();
        let value = if todo {
            TODO.to_owned()
        } else {
            format!("super.{}", m.name)
        };
        let mut out = format!("override {keyword} {}{ty}\n    get() = {value}", m.name);
        if m.mutable {
            out.push_str(if todo {
                "\n    set(value) {}"
            } else {
                "\n    set(value) {\n        super."
            });
            if !todo {
                out.push_str(&format!("{} = value\n    }}", m.name));
            }
        }
        return out;
    }
    let params: Vec<String> = m
        .params
        .iter()
        .map(|p| {
            let vararg = if p.vararg { "vararg " } else { "" };
            format!("{vararg}{}: {}", p.name, p.ty)
        })
        .collect();
    let args: Vec<String> = m
        .params
        .iter()
        .map(|p| {
            if p.vararg {
                format!("*{}", p.name)
            } else {
                p.name.clone()
            }
        })
        .collect();
    let suspend = if m.suspend { "suspend " } else { "" };
    let generics = m
        .type_params
        .as_ref()
        .map(|t| format!("{t} "))
        .unwrap_or_default();
    let ty = m.ty.as_ref().map(|t| format!(": {t}")).unwrap_or_default();
    let body = if todo {
        TODO.to_owned()
    } else if m.ty.is_some() {
        format!("return super.{}({})", m.name, args.join(", "))
    } else {
        format!("super.{}({})", m.name, args.join(", "))
    };
    format!(
        "override {suspend}fun {generics}{}({}){ty} {{\n    {body}\n}}",
        m.name,
        params.join(", ")
    )
}

// --------------------------------------------------------------------------- entry points

fn no_texts(_: &Path) -> Option<String> {
    None
}

/// The type hierarchy of the type at a place of a Kotlin `file` (`SPEC-0082` v6 behaviour 17).
pub fn type_hierarchy_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    direction: TypeDirection,
) -> Option<CallNode> {
    let project = KtProject::load(root, file, text, &no_texts);
    project.type_hierarchy(project.file(file)?, line, character, direction)
}

/// The method hierarchy of the member at a place of a Kotlin `file`.
pub fn method_hierarchy_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    texts: &dyn Fn(&Path) -> Option<String>,
) -> Option<(String, CallNode)> {
    let project = KtProject::load(root, file, text, texts);
    project.method_hierarchy(project.file(file)?, line, character)
}

/// Go to Super Method at a place of a Kotlin `file`.
pub fn super_target_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    texts: &dyn Fn(&Path) -> Option<String>,
) -> Option<SuperTarget> {
    let project = KtProject::load(root, file, text, texts);
    project.super_target(project.file(file)?, line, character)
}

/// The members the Kotlin class at a place can override.
pub fn overridable_methods_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
) -> (String, Vec<OverridableMethod>) {
    let project = KtProject::load(root, file, text, &no_texts);
    match project.file(file) {
        Some(f) => project.overridable(f, line, character),
        None => (String::new(), Vec::new()),
    }
}

/// The edits adding `methods` to the Kotlin class at a place.
pub fn add_overridable_methods_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    methods: &[OverridableMethod],
) -> Result<Vec<FileEdits>, String> {
    let project = KtProject::load(root, file, text, &no_texts);
    let f = project.file(file).ok_or("the file could not be parsed")?;
    project.add_overridable(f, line, character, methods)
}

/// The gutter marks of a Kotlin `file`.
pub fn declaration_marks_in(root: &Path, file: &Path, text: Option<&str>) -> Vec<DeclarationMark> {
    let project = KtProject::load(root, file, text, &no_texts);
    project
        .file(file)
        .map(|f| project.declaration_marks(f))
        .unwrap_or_default()
}

/// Usage counts of a Kotlin file (`SPEC-0292` v6 behaviour 9): the declarations and what is below
/// them from the syntax tree, the usages from the server's references. Declarations with neither
/// usages nor anything below are left out; so are those the server was not asked about in time
/// or failed on. Without a server there are no counts.
///
/// `plan` chooses the declarations and their order (`SPEC-0292` v8 behaviour 12).
pub async fn usage_counts_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
    plan: &Plan,
) -> Result<Counted, LspError> {
    let project = KtProject::load(root, file, text, &no_texts);
    let Some(f) = project.file(file) else {
        return Ok(Counted::new(Vec::new(), true));
    };
    let mut found = plan.order(project.counted_declarations(f));
    let deadline = Instant::now() + wait;
    let o = Opened::open_text(root, file, text, wait).await?;
    let mut asked = 0;
    for d in &mut found {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut params = o.position(d.line, d.character);
        params["context"] = json!({"includeDeclaration": false});
        let Ok(references) = o
            .ask_bounded("textDocument/references", params, left, None)
            .await
        else {
            break;
        };
        d.count = distinct_places(&references);
        asked += 1;
    }
    o.close().await;
    let complete = asked == found.len();
    found.truncate(asked);
    found.retain(|d| d.count > 0 || d.inheritors > 0);
    Ok(Counted::new(found, complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANIMALS: &str = "package zoo

abstract class Animal(val name: String) {
    abstract fun sound(): String

    open fun describe(): String = \"$name says ${sound()}\"
}

interface Pet {
    val owner: String
    fun friendly(): Boolean = true
}

class Dog(name: String) : Animal(name), Pet {
    override fun sound(): String = \"Woof\"
}

class Cat(name: String, override val owner: String) : Animal(name), Pet {
    override fun sound(): String = \"Meow\"
}

sealed class Shape {
    data class Circle(val radius: Double) : Shape()
    data class Square(val side: Double) : Shape()
}

class Person(val name: String, val age: Int)
";

    const OTHER: &str = "package farm

import zoo.Animal
import zoo.Pet

open class Cow : Animal(\"cow\"), Comparable<Cow>, Pet {
    override fun sound(): String = \"Moo\"
    override val owner: String get() = \"farmer\"
    override fun compareTo(other: Cow): Int = 0
}

class Calf : Cow()

class Stranger : Runnable {
    override fun run() {}
}
";

    fn project() -> KtProject {
        KtProject::of(&[("/p/zoo/Animals.kt", ANIMALS), ("/p/farm/Cow.kt", OTHER)])
    }

    fn names(n: &CallNode) -> String {
        let mark = n.mark.clone().unwrap_or_default();
        if n.children.is_empty() {
            format!("{mark}{}", n.name)
        } else {
            let below: Vec<String> = n.children.iter().map(names).collect();
            format!("{mark}{}[{}]", n.name, below.join(" "))
        }
    }

    /// SPEC-0082 AK-12: declarations, modifiers, supertypes and members of a Kotlin file.
    #[test]
    fn parses_types_and_members() {
        let f = parse_file(Path::new("/p/Animals.kt"), ANIMALS).unwrap();
        assert_eq!(f.package, "zoo");
        let names: Vec<&str> = f.types.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Animal", "Pet", "Dog", "Cat", "Shape", "Circle", "Square", "Person"
            ]
        );
        let animal = &f.types[0];
        assert!(animal.is_abstract);
        assert_eq!((animal.line, animal.column), (2, 15));
        let members: Vec<(&str, bool, bool, bool)> = animal
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.property, m.is_abstract, m.overridable))
            .collect();
        assert_eq!(
            members,
            [
                ("name", true, false, false),
                ("sound", false, true, true),
                ("describe", false, false, true)
            ]
        );
        let pet = &f.types[1];
        assert_eq!(pet.kind, KtKind::Interface);
        assert!(pet.members[0].is_abstract && pet.members[0].property);
        assert!(!pet.members[1].is_abstract && pet.members[1].overridable);
        let dog = &f.types[2];
        let supers: Vec<(&str, bool)> = dog
            .supertypes
            .iter()
            .map(|s| (s.name.as_str(), s.call))
            .collect();
        assert_eq!(supers, [("Animal", true), ("Pet", false)]);
        let cat = &f.types[3];
        assert!(cat.members[0].is_override && cat.members[0].name == "owner");
        assert_eq!(f.types[5].outer, ["Shape"]);
        assert!(f.types[4].is_abstract, "sealed");
        assert_eq!(f.types[7].body, None);
    }

    /// SPEC-0082 AK-12: the three directions; other packages need an import, libraries are leaves.
    #[test]
    fn type_hierarchy_of_kotlin_types() {
        let p = project();
        let tree = p.type_hierarchy(0, 2, 16, TypeDirection::Class).unwrap();
        assert_eq!(names(&tree), "Any[*Animal[Dog Cat Cow[Calf]]]");
        assert_eq!(tree.detail.as_deref(), Some("kotlin"));
        assert_eq!(tree.children[0].detail.as_deref(), Some("zoo"));
        assert_eq!(tree.children[0].children[2].detail.as_deref(), Some("farm"));
        // From inside a body, and a class in the middle of the chain.
        let tree = p.type_hierarchy(1, 6, 20, TypeDirection::Class).unwrap();
        assert_eq!(names(&tree), "Any[Animal[*Cow[Calf]]]");
        let up = p
            .type_hierarchy(1, 5, 12, TypeDirection::Supertypes)
            .unwrap();
        assert_eq!(names(&up), "*Cow[Animal Comparable Pet]");
        assert_eq!(up.children[1].kind, "interface");
        assert_eq!(up.children[1].location.range.start_line, 5);
        let down = p.type_hierarchy(0, 8, 11, TypeDirection::Subtypes).unwrap();
        assert_eq!(names(&down), "*Pet[Dog Cat Cow[Calf]]");
        let sealed = p.type_hierarchy(0, 21, 14, TypeDirection::Class).unwrap();
        assert_eq!(names(&sealed), "Any[*Shape[Circle Square]]");
        // A library class above ends the chain; an interface has no class above.
        let stranger = p.type_hierarchy(1, 13, 8, TypeDirection::Class).unwrap();
        assert_eq!(names(&stranger), "Any[*Stranger]");
        let pet = p.type_hierarchy(0, 8, 11, TypeDirection::Class).unwrap();
        assert_eq!(names(&pet), "*Pet[Dog Cat Cow[Calf]]");
        assert!(p.type_hierarchy(0, 0, 0, TypeDirection::Class).is_none());
    }

    /// SPEC-0082 AK-12: a type of another package without an import is not the project's type.
    #[test]
    fn resolution_needs_package_or_import() {
        let p = KtProject::of(&[
            (
                "/p/a/A.kt",
                "package a\nopen class Base\ninterface Runnable\n",
            ),
            (
                "/p/b/B.kt",
                "package b\nclass X : Base()\nclass Y : Runnable\n",
            ),
            (
                "/p/c/C.kt",
                "package c\nimport a.*\nclass Z : Base()\nclass W : a.Base()\n",
            ),
        ]);
        let x = p
            .type_hierarchy(1, 1, 6, TypeDirection::Supertypes)
            .unwrap();
        assert_eq!(x.children[0].location.path, Path::new("/p/b/B.kt"));
        let base = p.type_hierarchy(0, 1, 11, TypeDirection::Subtypes).unwrap();
        assert_eq!(names(&base), "*Base[Z W]");
    }

    /// SPEC-0082 AK-12: method hierarchy marks and the super member.
    #[test]
    fn method_hierarchy_and_super_member() {
        let p = project();
        // From Dog.sound(): the root is Animal, which declares it.
        let (name, tree) = p.method_hierarchy(0, 14, 20).unwrap();
        assert_eq!(name, "sound");
        assert_eq!(names(&tree), "+Animal[+Dog +Cat +Cow[-Calf]]");
        // Pet.owner: Dog must define it, Cat does in its constructor, Calf inherits Cow's.
        let (name, tree) = p.method_hierarchy(0, 9, 9).unwrap();
        assert_eq!(name, "owner");
        assert_eq!(names(&tree), "+Pet[!Dog +Cat +Cow[-Calf]]");
        assert!(
            p.method_hierarchy(0, 13, 7).is_none(),
            "on the class header"
        );

        let s = p.super_target(0, 18, 20).unwrap();
        assert_eq!(s.name, "Animal.sound()");
        assert_eq!(s.location.unwrap().range.start_line, 3);
        let s = p.super_target(0, 17, 40).unwrap();
        assert_eq!(s.name, "Pet.owner");
        let s = p.super_target(0, 13, 7).unwrap();
        assert_eq!(s.name, "Animal");
        assert_eq!(s.location.unwrap().range.start_line, 2);
        // A library supertype has no place; a method overriding a library method finds nothing.
        let s = p.super_target(1, 13, 8).unwrap();
        assert_eq!((s.name.as_str(), s.location), ("Runnable", None));
        assert!(p.super_target(1, 14, 18).is_none());
        assert!(
            p.super_target(0, 3, 20).is_none(),
            "nothing above Animal.sound"
        );
    }

    /// SPEC-0122 AK-19: what Dog can override and must implement, and the text added.
    #[test]
    fn override_and_implement_members() {
        let p = project();
        let (ty, methods) = p.overridable(0, 13, 7);
        assert_eq!(ty, "zoo.Dog");
        let rows: Vec<(String, &str, bool)> = methods
            .iter()
            .map(|m| (m.label(), m.declaring_class.as_str(), m.unimplemented))
            .collect();
        assert_eq!(
            rows,
            [
                ("describe(): String".to_owned(), "zoo.Animal", false),
                ("owner: String".to_owned(), "zoo.Pet", true),
                ("friendly(): Boolean".to_owned(), "zoo.Pet", false),
                ("equals(Any?): Boolean".to_owned(), "kotlin.Any", false),
                ("hashCode(): Int".to_owned(), "kotlin.Any", false),
                ("toString(): String".to_owned(), "kotlin.Any", false),
            ]
        );
        assert_eq!(methods[1].declaring_class_type, "interface");
        let chosen = [methods[1].clone(), methods[0].clone()];
        let edits = p.add_overridable(0, 13, 7, &chosen).unwrap();
        let new = crate::rename::apply_edits(ANIMALS, &edits[0].edits).unwrap();
        assert!(
            new.contains(
                "class Dog(name: String) : Animal(name), Pet {
    override fun sound(): String = \"Woof\"

    override val owner: String
        get() = TODO(\"Not yet implemented\")

    override fun describe(): String {
        return super.describe()
    }
}
"
            ),
            "{new}"
        );
        // Cat implements everything; nothing is left to implement.
        let (_, cat) = p.overridable(0, 17, 7);
        assert!(cat.iter().all(|m| !m.unimplemented));
        assert!(!cat.iter().any(|m| m.name == "owner"));
    }

    /// SPEC-0122 AK-19: a class without a body gets one; an empty body is filled; `var`,
    /// `vararg`, `suspend`, type parameters and `Unit` functions keep their shape.
    #[test]
    fn stubs_and_bodies() {
        let text = "interface Store<T> {
    var size: Int
    suspend fun <R> put(key: String, vararg items: T): R
    fun clear()
    fun ready(): Boolean = true
}

class Memory : Store<String>

class Disk : Store<String> {}

object Net : Store<String> {
    override fun ready(): Boolean = false
    override fun toString(): String = \"net\"
}
";
        let p = KtProject::of(&[("/p/Store.kt", text)]);
        let (_, methods) = p.overridable(0, 7, 7);
        let todo: Vec<_> = methods
            .iter()
            .filter(|m| m.unimplemented)
            .cloned()
            .collect();
        assert_eq!(todo.len(), 3);
        let edits = p.add_overridable(0, 7, 7, &todo).unwrap();
        let new = crate::rename::apply_edits(text, &edits[0].edits).unwrap();
        assert!(
            new.contains(
                "class Memory : Store<String> {
    override var size: Int
        get() = TODO(\"Not yet implemented\")
        set(value) {}

    override suspend fun <R> put(key: String, vararg items: String): R {
        TODO(\"Not yet implemented\")
    }

    override fun clear() {
        TODO(\"Not yet implemented\")
    }
}

class Disk"
            ),
            "{new}"
        );
        let (_, disk) = p.overridable(0, 9, 7);
        let ready: Vec<_> = disk.iter().filter(|m| m.name == "ready").cloned().collect();
        let edits = p.add_overridable(0, 9, 7, &ready).unwrap();
        let new = crate::rename::apply_edits(text, &edits[0].edits).unwrap();
        assert!(
            new.contains(
                "class Disk : Store<String> {
    override fun ready(): Boolean {
        return super.ready()
    }
}
"
            ),
            "{new}"
        );
        // Type arguments reach through a generic class in between; a member's own type
        // parameter of the same name stays.
        let generic = "interface Source<T> {
    fun next(): T
    fun <T> map(value: T): T
}
abstract class Cached<K, V> : Source<Map<K, V>>
class Names : Cached<Int, String>()
";
        let g = KtProject::of(&[("/p/Source.kt", generic)]);
        let (_, methods) = g.overridable(0, 5, 7);
        let labels: Vec<String> = methods.iter().take(2).map(|m| m.label()).collect();
        assert_eq!(labels, ["next(): Map<Int, String>", "map(T): T"]);
        // What the object declares already is not offered again.
        let (_, net) = p.overridable(0, 12, 4);
        let offered: Vec<&str> = net.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(offered, ["size", "put", "clear", "equals", "hashCode"]);
        assert!(p.add_overridable(0, 0, 0, &[]).is_err());
    }

    /// SPEC-0320 AK-11: gutter marks of the fixture.
    #[test]
    fn gutter_marks() {
        let p = project();
        let all = p.declaration_marks(0);
        let line = |l: u64| -> Vec<(MarkKind, Vec<&str>)> {
            all.iter()
                .filter(|m| m.line == l)
                .map(|m| (m.kind, m.targets.iter().map(|t| t.name.as_str()).collect()))
                .collect()
        };
        assert_eq!(all.len(), 8);
        assert_eq!(
            line(2),
            [(MarkKind::Inherited, vec!["Dog", "Cat", "Cow", "Calf"])]
        );
        assert_eq!(
            line(3),
            [(MarkKind::Implemented, vec!["Dog", "Cat", "Cow"])]
        );
        assert_eq!(line(5), [], "describe is not overridden anywhere");
        assert_eq!(line(9), [(MarkKind::Implemented, vec!["Cat", "Cow"])]);
        assert_eq!(line(14), [(MarkKind::Implements, vec!["Animal"])]);
        assert_eq!(line(17), [(MarkKind::Implements, vec!["Pet"]),]);
        assert_eq!(line(18), [(MarkKind::Implements, vec!["Animal"])]);
        assert_eq!(line(21), [(MarkKind::Inherited, vec!["Circle", "Square"])]);
        // Calf has nothing of its own; Cow.sound implements Animal's.
        let cow = p.declaration_marks(1);
        assert!(
            cow.iter()
                .any(|m| m.line == 6 && m.kind == MarkKind::Implements)
        );
        assert!(
            cow.iter()
                .any(|m| m.line == 5 && m.kind == MarkKind::Inherited)
        );
    }

    /// SPEC-0292 AK-12: what is counted in a Kotlin file and what is below each declaration.
    #[test]
    fn counted_declarations_with_inheritors() {
        let p = project();
        let counted = p.counted_declarations(0);
        let row = |name: &str, line: u64| {
            counted
                .iter()
                .find(|u| u.name == name && u.line == line)
                .map(|u| (u.kind, u.inheritors))
        };
        assert_eq!(row("Animal", 2), Some(("class", 4)));
        assert_eq!(row("sound", 3), Some(("abstract function", 3)));
        assert_eq!(row("describe", 5), Some(("method", 0)));
        assert_eq!(row("Pet", 8), Some(("interface", 4)));
        assert_eq!(row("owner", 9), Some(("abstract property", 2)));
        assert_eq!(row("Shape", 21), Some(("class", 2)));
        assert_eq!(row("radius", 22), Some(("property", 0)));
        let main = KtProject::of(&[(
            "/p/Main.kt",
            "fun total(): Int = 1\nval limit = 3\nenum class Color { RED }\n",
        )]);
        let counted = main.counted_declarations(0);
        let kinds: Vec<(&str, &str)> = counted.iter().map(|u| (u.name.as_str(), u.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("total", "function"),
                ("limit", "property"),
                ("Color", "enum"),
                ("RED", "enum member")
            ]
        );
    }

    #[test]
    fn kotlin_files() {
        assert!(is_kotlin(Path::new("a/B.kt")));
        assert!(!is_kotlin(Path::new("a/B.java")));
        assert!(!is_kotlin(Path::new("build.gradle.kts")));
    }
}
