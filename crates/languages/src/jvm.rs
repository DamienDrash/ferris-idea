//! Java and Kotlin in the editor (`SPEC-0122`): run markers for `main` and tests, Optimize
//! Imports, and Generate (constructor, getters, setters, `equals`/`hashCode`, `toString`) — all
//! from the syntax tree, without a language server.

use std::collections::BTreeSet;
use std::ops::Range;

use serde::Serialize;
use tree_sitter::{Node, Parser, Tree};

/// The two languages handled here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Jvm {
    /// `.java`
    Java,
    /// `.kt`
    Kotlin,
}

impl Jvm {
    /// The language of a file name, if it is Java or Kotlin.
    pub fn of(path: &str) -> Option<Self> {
        if path.ends_with(".java") {
            Some(Self::Java)
        } else if path.ends_with(".kt") {
            Some(Self::Kotlin)
        } else {
            None
        }
    }
}

pub(crate) fn parse(lang: Jvm, text: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language = match lang {
        Jvm::Java => tree_sitter_java::LANGUAGE.into(),
        Jvm::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
    };
    parser.set_language(&language).ok()?;
    parser.parse(text, None)
}

pub(crate) fn text_of<'a>(n: Node, text: &'a str) -> &'a str {
    &text[n.byte_range()]
}

pub(crate) fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

// ---------------------------------------------------------------- run markers (behaviour 1)

/// What a run marker starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// A class or file with a `main` function: runs it.
    Main,
    /// A test class: runs all its tests.
    TestClass,
    /// One test method: runs only it.
    TestMethod,
}

/// A ▶ in the gutter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunMarker {
    /// 0-based line of the declaration's name.
    pub line: usize,
    /// What it runs.
    pub kind: MarkerKind,
    /// Simple class name (for Kotlin top-level `main`, the file's class `NameKt` is not needed:
    /// the file runs).
    pub class: String,
    /// Test method name for [`MarkerKind::TestMethod`].
    pub method: Option<String>,
}

/// Annotations that make a method a test (JUnit 4 and 5, kotlin.test, TestNG).
const TEST_ANNOTATIONS: &[&str] = &[
    "Test",
    "ParameterizedTest",
    "RepeatedTest",
    "TestFactory",
    "TestTemplate",
];

/// Last segment of an annotation's name (`org.junit.Test` → `Test`).
fn annotation_names(modifiers: Option<Node>, text: &str) -> Vec<String> {
    let Some(m) = modifiers else {
        return Vec::new();
    };
    children(m)
        .into_iter()
        .filter(|c| c.kind().contains("annotation"))
        .filter_map(|a| {
            let name = a
                .child_by_field_name("name")
                .or_else(|| children(a).into_iter().find(|c| c.kind() == "user_type"))
                .or_else(|| {
                    children(a)
                        .into_iter()
                        .find(|c| c.kind() == "constructor_invocation")
                })?;
            let full = text_of(name, text);
            let full = full.split('(').next().unwrap_or(full);
            Some(full.rsplit('.').next().unwrap_or(full).trim().to_owned())
        })
        .collect()
}

pub(crate) fn modifiers_of(n: Node) -> Option<Node> {
    children(n).into_iter().find(|c| c.kind() == "modifiers")
}

pub(crate) fn has_word(modifiers: Option<Node>, word: &str, text: &str) -> bool {
    modifiers.is_some_and(|m| {
        children(m)
            .iter()
            .any(|c| !c.kind().contains("annotation") && text_of(*c, text) == word)
            || text_of(m, text).split_whitespace().any(|w| w == word)
    })
}

fn name_of<'a>(n: Node, text: &'a str) -> Option<(&'a str, usize)> {
    let name = n
        .child_by_field_name("name")
        .or_else(|| children(n).into_iter().find(|c| c.kind() == "identifier"))?;
    Some((text_of(name, text), name.start_position().row))
}

fn body_of(n: Node) -> Option<Node> {
    n.child_by_field_name("body")
        .or_else(|| children(n).into_iter().find(|c| c.kind() == "class_body"))
}

fn is_test(n: Node, text: &str) -> bool {
    annotation_names(modifiers_of(n), text)
        .iter()
        .any(|a| TEST_ANNOTATIONS.contains(&a.as_str()))
}

/// The ▶ markers of a Java or Kotlin source: a top-level class with `public static void main`
/// (Java) or a top-level `fun main` (Kotlin) gets a run marker on the class or function; a
/// top-level class with test methods gets one for the class and one per test method. Nested
/// classes are left out.
pub fn run_markers(lang: Jvm, text: &str) -> Vec<RunMarker> {
    let Some(tree) = parse(lang, text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for top in children(tree.root_node()) {
        match (lang, top.kind()) {
            (_, "class_declaration") | (Jvm::Kotlin, "object_declaration") => {
                let Some((class, class_line)) = name_of(top, text) else {
                    continue;
                };
                let methods: Vec<Node> = body_of(top)
                    .map(children)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| matches!(m.kind(), "method_declaration" | "function_declaration"))
                    .collect();
                let mains: Vec<usize> = methods
                    .iter()
                    .filter(|m| {
                        lang == Jvm::Java
                            && name_of(**m, text).is_some_and(|(n, _)| n == "main")
                            && has_word(modifiers_of(**m), "static", text)
                            && m.child_by_field_name("type")
                                .is_some_and(|t| t.kind() == "void_type")
                    })
                    .filter_map(|m| name_of(*m, text).map(|(_, l)| l))
                    .collect();
                let tests: Vec<(String, usize)> = methods
                    .iter()
                    .filter(|m| is_test(**m, text))
                    .filter_map(|m| name_of(*m, text).map(|(n, l)| (n.to_owned(), l)))
                    .collect();
                if !mains.is_empty() {
                    out.push(RunMarker {
                        line: class_line,
                        kind: MarkerKind::Main,
                        class: class.to_owned(),
                        method: None,
                    });
                    for line in mains {
                        out.push(RunMarker {
                            line,
                            kind: MarkerKind::Main,
                            class: class.to_owned(),
                            method: None,
                        });
                    }
                } else if !tests.is_empty() {
                    out.push(RunMarker {
                        line: class_line,
                        kind: MarkerKind::TestClass,
                        class: class.to_owned(),
                        method: None,
                    });
                }
                for (method, line) in tests {
                    out.push(RunMarker {
                        line,
                        kind: MarkerKind::TestMethod,
                        class: class.to_owned(),
                        method: Some(method),
                    });
                }
            }
            (Jvm::Kotlin, "function_declaration") => {
                if let Some((name, line)) = name_of(top, text)
                    && name == "main"
                {
                    out.push(RunMarker {
                        line,
                        kind: MarkerKind::Main,
                        class: String::new(),
                        method: None,
                    });
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|m| m.line);
    out.dedup_by_key(|m| m.line);
    out
}

/// The test method around 0-based `line` (for running only it with ⌃⇧R), if any.
pub fn test_method_at(lang: Jvm, text: &str, line: usize) -> Option<RunMarker> {
    let tree = parse(lang, text)?;
    let markers = run_markers(lang, text);
    let mut node = tree.root_node().descendant_for_point_range(
        tree_sitter::Point::new(line, 0),
        tree_sitter::Point::new(line, 0),
    )?;
    loop {
        if matches!(node.kind(), "method_declaration" | "function_declaration") {
            let (_, name_line) = name_of(node, text)?;
            return markers
                .into_iter()
                .find(|m| m.line == name_line && m.kind == MarkerKind::TestMethod);
        }
        node = node.parent()?;
    }
}

// ---------------------------------------------------------------- Optimize Imports (behaviour 2)

/// What Optimize Imports would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportPlan {
    /// Byte range of the import block that is replaced.
    pub range: Range<usize>,
    /// The new import block.
    pub replacement: String,
    /// Imports removed (unused, duplicate, `java.lang` or same package).
    pub removed: Vec<String>,
}

/// Kotlin operator and delegate conventions: imports of these names are used by syntax, not by
/// name (`by lazy`, `a + b`), so they are never removed.
const KOTLIN_CONVENTIONS: &[&str] = &[
    "getValue",
    "setValue",
    "provideDelegate",
    "plus",
    "minus",
    "times",
    "div",
    "rem",
    "rangeTo",
    "rangeUntil",
    "contains",
    "get",
    "set",
    "invoke",
    "compareTo",
    "iterator",
    "next",
    "hasNext",
    "inc",
    "dec",
    "unaryPlus",
    "unaryMinus",
    "not",
    "plusAssign",
    "minusAssign",
    "timesAssign",
    "divAssign",
    "remAssign",
    "equals",
];

struct Import {
    /// Path without `import`, `static`, `;`, alias: `java.util.List`, `org.x.Y.*`.
    path: String,
    is_static: bool,
    alias: Option<String>,
}

impl Import {
    fn line(&self, lang: Jvm) -> String {
        match lang {
            Jvm::Java => format!(
                "import {}{};",
                if self.is_static { "static " } else { "" },
                self.path
            ),
            Jvm::Kotlin => match &self.alias {
                Some(a) => format!("import {} as {a}", self.path),
                None => format!("import {}", self.path),
            },
        }
    }

    fn simple(&self) -> &str {
        self.alias
            .as_deref()
            .unwrap_or_else(|| self.path.rsplit('.').next().unwrap_or(&self.path))
    }
}

/// Words a comment can use a class through: `{@link X}`, `@see X`, `@throws X`, `[X]` (KDoc).
fn comment_refs(comment: &str, out: &mut BTreeSet<String>) {
    let ident = |s: &str| -> String {
        s.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    };
    for tag in [
        "@link ",
        "@linkplain ",
        "@see ",
        "@throws ",
        "@exception ",
        "@value ",
    ] {
        for part in comment.split(tag).skip(1) {
            let word = ident(part.trim_start());
            if !word.is_empty() {
                out.insert(word);
            }
        }
    }
    for part in comment.split('[').skip(1) {
        let word = ident(part);
        if !word.is_empty() {
            out.insert(word);
        }
    }
}

/// Names used outside the package and import declarations.
fn used_names(root: Node, text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "import_declaration" | "import" | "package_declaration" | "package_header" => continue,
            "identifier" | "type_identifier" | "simple_identifier" => {
                out.insert(text_of(n, text).to_owned());
            }
            k if k.contains("comment") => comment_refs(text_of(n, text), &mut out),
            _ => {}
        }
        stack.extend(children(n));
    }
    out
}

fn java_group(i: &Import) -> u8 {
    if i.is_static {
        2
    } else if i.path.starts_with("java.") || i.path.starts_with("javax.") {
        1
    } else {
        0
    }
}

fn kotlin_group(i: &Import) -> u8 {
    if i.alias.is_some() {
        4
    } else if i.path.starts_with("java.") {
        1
    } else if i.path.starts_with("javax.") {
        2
    } else if i.path.starts_with("kotlin.") {
        3
    } else {
        0
    }
}

/// Plans Optimize Imports: unused single-name imports, duplicates, `java.lang` (Java) and imports
/// from the file's own package go; wildcard imports stay; the rest is sorted in the reference's
/// default layout (Java: others, blank line, `javax.*` and `java.*`, blank line, static imports;
/// Kotlin: others, `java.*`, `javax.*`, `kotlin.*`, aliases). `Ok(None)` when nothing changes;
/// an error when comments sit between the imports (they would be lost).
pub fn optimize_imports(lang: Jvm, text: &str) -> Result<Option<ImportPlan>, String> {
    let tree = parse(lang, text).ok_or("the file could not be parsed")?;
    let root = tree.root_node();
    let top = children(root);
    let import_kind = match lang {
        Jvm::Java => "import_declaration",
        Jvm::Kotlin => "import",
    };
    let imports: Vec<Node> = top
        .iter()
        .copied()
        .filter(|n| n.kind() == import_kind)
        .collect();
    let (Some(first), Some(last)) = (imports.first(), imports.last()) else {
        return Ok(None);
    };
    let range = first.start_byte()..last.end_byte();
    if top.iter().any(|n| {
        n.kind() != import_kind && n.start_byte() >= range.start && n.end_byte() <= range.end
    }) {
        return Err("comments or declarations sit between the imports; nothing changed".into());
    }
    let package = top
        .iter()
        .find(|n| matches!(n.kind(), "package_declaration" | "package_header"))
        .map(|n| {
            text_of(*n, text)
                .trim_start_matches("package")
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_owned()
        });
    let parsed: Vec<Import> = imports
        .iter()
        .map(|n| {
            let raw = text_of(*n, text)
                .trim_start_matches("import")
                .trim()
                .trim_end_matches(';')
                .trim();
            let (is_static, raw) = match raw.strip_prefix("static ") {
                Some(r) => (true, r.trim()),
                None => (false, raw),
            };
            let (path, alias) = match raw.split_once(" as ") {
                Some((p, a)) => (p.trim(), Some(a.trim().to_owned())),
                None => (raw, None),
            };
            Import {
                path: path.split_whitespace().collect::<String>(),
                is_static,
                alias,
            }
        })
        .collect();
    let used = used_names(root, text);
    let mut kept: Vec<Import> = Vec::new();
    let mut removed = Vec::new();
    for import in parsed {
        let line = import.line(lang);
        let wildcard = import.path.ends_with(".*");
        let owner = import.path.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
        let redundant = !import.is_static
            && !wildcard
            && import.alias.is_none()
            && ((lang == Jvm::Java && owner == "java.lang") || package.as_deref() == Some(owner));
        let duplicate = kept.iter().any(|k| k.line(lang) == line);
        let unused = !wildcard
            && !used.contains(import.simple())
            && !(lang == Jvm::Kotlin && KOTLIN_CONVENTIONS.contains(&import.simple()));
        if duplicate || redundant || unused {
            removed.push(line);
        } else {
            kept.push(import);
        }
    }
    let group = |i: &Import| match lang {
        Jvm::Java => java_group(i),
        Jvm::Kotlin => kotlin_group(i),
    };
    kept.sort_by(|a, b| {
        group(a)
            .cmp(&group(b))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.alias.cmp(&b.alias))
    });
    let mut replacement = String::new();
    let mut last_group = None;
    for i in &kept {
        let g = group(i);
        if let Some(prev) = last_group {
            replacement.push('\n');
            if lang == Jvm::Java && prev != g {
                replacement.push('\n');
            }
        }
        replacement.push_str(&i.line(lang));
        last_group = Some(g);
    }
    if replacement == text[range.clone()] {
        return Ok(None);
    }
    // With every import gone, the blank line after the block goes too.
    let range = if kept.is_empty() {
        let mut end = range.end;
        let bytes = text.as_bytes();
        while end < bytes.len() && (bytes[end] == b'\n' || bytes[end] == b'\r') {
            end += 1;
        }
        range.start..end
    } else {
        range
    };
    Ok(Some(ImportPlan {
        range,
        replacement,
        removed,
    }))
}

// ---------------------------------------------------------------- Generate (behaviour 3)

/// What Generate inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerateKind {
    /// A constructor taking the fields.
    Constructor,
    /// Getters for fields without one.
    Getters,
    /// Setters for non-final fields without one.
    Setters,
    /// Both.
    GettersAndSetters,
    /// `equals` and `hashCode` with `java.util.Objects`.
    EqualsHashCode,
    /// `toString` listing the fields.
    ToString,
}

impl GenerateKind {
    /// All kinds in menu order.
    pub const ALL: [GenerateKind; 6] = [
        Self::Constructor,
        Self::Getters,
        Self::Setters,
        Self::GettersAndSetters,
        Self::EqualsHashCode,
        Self::ToString,
    ];

    /// Menu label as in the reference.
    pub fn label(self) -> &'static str {
        match self {
            Self::Constructor => "Constructor",
            Self::Getters => "Getter",
            Self::Setters => "Setter",
            Self::GettersAndSetters => "Getter and Setter",
            Self::EqualsHashCode => "equals() and hashCode()",
            Self::ToString => "toString()",
        }
    }
}

/// A template of Generate (`SPEC-0122` v4 behaviour 3b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerateTemplate {
    /// The default of the kind: `getX`/`setX`, `getClass()`, text concatenation.
    Default,
    /// Accessors named after the field; the setter returns `this`.
    Fluent,
    /// `equals` with `instanceof`, which also accepts subclasses.
    Instanceof,
    /// `equals`/`hashCode` through the class's getters.
    Getters,
    /// `instanceof` and the class's getters.
    InstanceofGetters,
    /// `toString` with a `StringBuilder`.
    StringBuilder,
}

impl GenerateTemplate {
    /// The templates of `kind`, the default first.
    pub fn of(kind: GenerateKind) -> &'static [GenerateTemplate] {
        match kind {
            GenerateKind::Constructor => &[Self::Default],
            GenerateKind::Getters | GenerateKind::Setters | GenerateKind::GettersAndSetters => {
                &[Self::Default, Self::Fluent]
            }
            GenerateKind::EqualsHashCode => &[
                Self::Default,
                Self::Instanceof,
                Self::Getters,
                Self::InstanceofGetters,
            ],
            GenerateKind::ToString => &[Self::Default, Self::StringBuilder],
        }
    }

    /// What the template list calls it for `kind`.
    pub fn label(self, kind: GenerateKind) -> &'static str {
        match (self, kind) {
            (Self::Default, GenerateKind::EqualsHashCode) => "getClass() comparison",
            (Self::Default, GenerateKind::ToString) => "String concatenation",
            (Self::Default, _) => "getX() and setX()",
            (Self::Fluent, _) => "x() and x(value) returning this",
            (Self::Instanceof, _) => "instanceof comparison",
            (Self::Getters, _) => "getClass() comparison, through getters",
            (Self::InstanceofGetters, _) => "instanceof comparison, through getters",
            (Self::StringBuilder, _) => "StringBuilder",
        }
    }
}

/// What the chooser of Generate decided (`SPEC-0122` v4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOptions {
    /// The fields to use by name, in the class's order; `None` uses every field.
    pub fields: Option<Vec<String>>,
    /// The template; it must be one of [`GenerateTemplate::of`].
    pub template: GenerateTemplate,
    /// `toString` carries `@Override`.
    pub override_annotation: bool,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            fields: None,
            template: GenerateTemplate::Default,
            override_annotation: true,
        }
    }
}

/// A field of the class at the caret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    /// Name.
    pub name: String,
    /// Type as written.
    pub ty: String,
    /// `static`.
    pub is_static: bool,
    /// `final`.
    pub is_final: bool,
    /// Has an initializer.
    pub initialized: bool,
}

/// A text edit in byte offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextEdit {
    /// Replaced range.
    pub range: Range<usize>,
    /// New text.
    pub text: String,
}

struct ClassAt<'t> {
    name: String,
    fields: Vec<Field>,
    /// Methods: name and parameter types.
    methods: Vec<(String, Vec<String>)>,
    body: Node<'t>,
}

fn class_at<'t>(tree: &'t Tree, text: &str, offset: usize) -> Option<ClassAt<'t>> {
    let mut node = tree.root_node().descendant_for_byte_range(offset, offset)?;
    loop {
        if matches!(
            node.kind(),
            "class_declaration" | "record_declaration" | "enum_declaration"
        ) {
            break;
        }
        node = node.parent()?;
    }
    let name = text_of(node.child_by_field_name("name")?, text).to_owned();
    let body = node.child_by_field_name("body")?;
    let mut fields = Vec::new();
    let mut methods = Vec::new();
    for member in children(body) {
        match member.kind() {
            "field_declaration" => {
                let m = modifiers_of(member);
                let ty = member
                    .child_by_field_name("type")
                    .map(|t| text_of(t, text).to_owned())
                    .unwrap_or_default();
                let mut cursor = member.walk();
                for d in member.children_by_field_name("declarator", &mut cursor) {
                    let Some(n) = d.child_by_field_name("name") else {
                        continue;
                    };
                    // `int a[]` puts the brackets on the declarator.
                    let dims = d
                        .child_by_field_name("dimensions")
                        .map(|x| text_of(x, text))
                        .unwrap_or("");
                    fields.push(Field {
                        name: text_of(n, text).to_owned(),
                        ty: format!("{ty}{dims}"),
                        is_static: has_word(m, "static", text),
                        is_final: has_word(m, "final", text),
                        initialized: d.child_by_field_name("value").is_some(),
                    });
                }
            }
            "method_declaration" | "constructor_declaration" => {
                let n = if member.kind() == "constructor_declaration" {
                    "<init>".to_owned()
                } else {
                    member
                        .child_by_field_name("name")
                        .map(|n| text_of(n, text).to_owned())
                        .unwrap_or_default()
                };
                let params = member
                    .child_by_field_name("parameters")
                    .map(|ps| {
                        children(ps)
                            .into_iter()
                            .filter(|p| p.kind() == "formal_parameter")
                            .filter_map(|p| p.child_by_field_name("type"))
                            .map(|t| text_of(t, text).to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                methods.push((n, params));
            }
            _ => {}
        }
    }
    Some(ClassAt {
        name,
        fields,
        methods,
        body,
    })
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn getter_name(f: &Field) -> String {
    let prefix = if f.ty == "boolean" { "is" } else { "get" };
    // A boolean field already named `isX` keeps its name as the getter.
    if f.ty == "boolean"
        && f.name.len() > 2
        && f.name.starts_with("is")
        && f.name[2..].starts_with(|c: char| c.is_uppercase())
    {
        return f.name.clone();
    }
    format!("{prefix}{}", capitalize(&f.name))
}

fn line_indent(text: &str, at: usize) -> String {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    text[start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Member indentation: that of the first member, else the class's own plus one level.
fn member_indent(text: &str, body: Node) -> String {
    match children(body)
        .into_iter()
        .find(|c| !matches!(c.kind(), "{" | "}") && !c.kind().contains("comment"))
    {
        Some(m) => line_indent(text, m.start_byte()),
        None => {
            let unit = if text.contains("\n\t") { "\t" } else { "    " };
            format!("{}{unit}", line_indent(text, body.start_byte()))
        }
    }
}

fn has_import(text: &str, class: &str) -> bool {
    let package = class.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
    text.lines()
        .map(str::trim)
        .any(|l| l == format!("import {class};") || l == format!("import {package}.*;"))
}

/// Edit that adds `import` lines for `classes` after the last import (or the package line).
fn add_imports(text: &str, classes: &[&str]) -> Option<TextEdit> {
    let missing: Vec<&&str> = classes.iter().filter(|c| !has_import(text, c)).collect();
    if missing.is_empty() {
        return None;
    }
    let mut at = None;
    let mut after_package = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("import ") || t.starts_with("package ") {
            at = Some(offset + line.len());
            after_package = t.starts_with("package ");
        } else if !t.trim().is_empty()
            && !t.starts_with("//")
            && !t.starts_with("/*")
            && !t.starts_with('*')
        {
            break;
        }
        offset += line.len();
    }
    let at = at.unwrap_or(0);
    let mut lines = String::new();
    if after_package {
        lines.push('\n');
    }
    for c in missing {
        lines.push_str(&format!("import {c};\n"));
    }
    if at == 0 {
        lines.push('\n');
    }
    Some(TextEdit {
        range: at..at,
        text: lines,
    })
}

/// The fields Generate offers for `kind` in the class around `offset`, and the class name;
/// `None` outside a Java class.
pub fn generate_fields(
    text: &str,
    offset: usize,
    kind: GenerateKind,
) -> Option<(String, Vec<Field>)> {
    let tree = parse(Jvm::Java, text)?;
    let class = class_at(&tree, text, offset)?;
    let fields = class
        .fields
        .into_iter()
        .filter(|f| !f.is_static)
        .filter(|f| match kind {
            GenerateKind::Constructor => !(f.is_final && f.initialized),
            GenerateKind::Setters => !f.is_final,
            _ => true,
        })
        .collect();
    Some((class.name, fields))
}

/// The fields the chooser of Generate lists for `kind` and which of them start checked
/// (`SPEC-0122` v4 behaviour 3b): accessors only for fields still lacking one; the constructor
/// checks only the final fields (they must be set), everything else checks every field.
pub fn chooser_fields(text: &str, offset: usize, kind: GenerateKind) -> Vec<(Field, bool)> {
    let Some((_, fields)) = generate_fields(text, offset, kind) else {
        return Vec::new();
    };
    let tree = parse(Jvm::Java, text);
    let existing: Vec<String> = tree
        .as_ref()
        .and_then(|t| class_at(t, text, offset))
        .map(|c| c.methods.into_iter().map(|(n, _)| n).collect())
        .unwrap_or_default();
    let lacks_getter = |f: &Field| !existing.contains(&getter_name(f));
    let lacks_setter =
        |f: &Field| !f.is_final && !existing.contains(&format!("set{}", capitalize(&f.name)));
    fields
        .into_iter()
        .filter(|f| match kind {
            GenerateKind::Getters => lacks_getter(f),
            GenerateKind::Setters => lacks_setter(f),
            GenerateKind::GettersAndSetters => lacks_getter(f) || lacks_setter(f),
            _ => true,
        })
        .map(|f| {
            let checked = kind != GenerateKind::Constructor || f.is_final;
            (f, checked)
        })
        .collect()
}

/// The Generate entries that apply in the Java class around `offset` (`SPEC-0122` v3 behaviour
/// 3a): Constructor and `toString()` always; `equals()`/`hashCode()` when the class has instance
/// fields; Getter, Setter and both only when a field still lacks them (a final field gets no
/// setter).
pub fn applicable_kinds(text: &str, offset: usize) -> Vec<GenerateKind> {
    let has_fields =
        generate_fields(text, offset, GenerateKind::Getters).is_some_and(|(_, f)| !f.is_empty());
    let applies = |k: GenerateKind| {
        has_fields
            && generate_fields(text, offset, k).is_some_and(|(_, f)| !f.is_empty())
            && generate(text, offset, k).is_ok_and(|e| !e.is_empty())
    };
    GenerateKind::ALL
        .into_iter()
        .filter(|k| match k {
            GenerateKind::Constructor | GenerateKind::ToString => true,
            GenerateKind::EqualsHashCode => has_fields,
            // Both only when setters are possible as well.
            GenerateKind::GettersAndSetters => {
                applies(GenerateKind::Setters) && generate(text, offset, *k).is_ok()
            }
            k => applies(*k),
        })
        .collect()
}

/// Where Generate puts its methods for a caret at `offset` in the class `body`: the start of the
/// line after the member the caret is in, or else after the last member before the caret; `None`
/// when no member comes before the caret (then they go before the closing brace).
fn member_insertion(text: &str, body: Node, offset: usize) -> Option<usize> {
    let close = body.end_byte().saturating_sub(1);
    if offset <= body.start_byte() || offset > close {
        return None;
    }
    let members: Vec<Node> = children(body)
        .into_iter()
        .filter(|m| m.is_named() && !m.kind().ends_with("comment"))
        .collect();
    let at = members
        .iter()
        .find(|m| m.start_byte() < offset && offset < m.end_byte())
        .or_else(|| members.iter().rfind(|m| m.end_byte() <= offset))?;
    let end = at.end_byte();
    let next = end + text[end..].find('\n')? + 1;
    (next <= close).then_some(next)
}

/// Plans Generate in the Java class around `offset` for its non-static fields (behaviour 3):
/// the methods go after the member at or before the caret, else before the class's closing
/// brace, with the members' indentation, existing
/// methods of the same name (and constructors with the same parameters) are not made twice, and
/// `java.util.Objects`/`Arrays` are imported when needed. Edits are in ascending order.
pub fn generate(text: &str, offset: usize, kind: GenerateKind) -> Result<Vec<TextEdit>, String> {
    generate_with(text, offset, kind, &GenerateOptions::default())
}

/// [`generate`] with the fields and the template of the chooser (`SPEC-0122` v4 behaviour 3b).
pub fn generate_with(
    text: &str,
    offset: usize,
    kind: GenerateKind,
    options: &GenerateOptions,
) -> Result<Vec<TextEdit>, String> {
    if !GenerateTemplate::of(kind).contains(&options.template) {
        return Err(format!(
            "`{}` has no template {:?}",
            kind.label(),
            options.template
        ));
    }
    let tree = parse(Jvm::Java, text).ok_or("the file could not be parsed")?;
    let class = class_at(&tree, text, offset).ok_or("the caret is not inside a Java class")?;
    let indent = member_indent(text, class.body);
    let unit = if indent.contains('\t') { "\t" } else { "    " };
    let inner = format!("{indent}{unit}");
    let mut fields: Vec<&Field> = class.fields.iter().filter(|f| !f.is_static).collect();
    if let Some(wanted) = &options.fields {
        if let Some(unknown) = wanted
            .iter()
            .find(|w| !fields.iter().any(|f| f.name == **w))
        {
            return Err(format!("`{unknown}` is not an instance field of the class"));
        }
        fields.retain(|f| wanted.contains(&f.name));
    }
    let template = options.template;
    let has_method = |name: &str| class.methods.iter().any(|(n, _)| n == name);
    let has_signature = |name: &str, params: &[&str]| {
        class.methods.iter().any(|(n, p)| {
            n == name && p.len() == params.len() && p.iter().zip(params).all(|(a, b)| a == b)
        })
    };
    let mut blocks: Vec<String> = Vec::new();
    let mut imports: Vec<&str> = Vec::new();
    let name = &class.name;
    match kind {
        GenerateKind::Constructor => {
            let params: Vec<&&Field> = fields
                .iter()
                .filter(|f| !(f.is_final && f.initialized))
                .collect();
            let types: Vec<String> = params.iter().map(|f| f.ty.clone()).collect();
            if class
                .methods
                .iter()
                .any(|(n, p)| n == "<init>" && *p == types)
            {
                return Err(format!("{name} already has this constructor"));
            }
            let list = params
                .iter()
                .map(|f| format!("{} {}", f.ty, f.name))
                .collect::<Vec<_>>()
                .join(", ");
            let mut b = format!("{indent}public {name}({list}) {{\n");
            for f in &params {
                b.push_str(&format!("{inner}this.{0} = {0};\n", f.name));
            }
            b.push_str(&format!("{indent}}}"));
            blocks.push(b);
        }
        GenerateKind::Getters | GenerateKind::Setters | GenerateKind::GettersAndSetters => {
            let fluent = template == GenerateTemplate::Fluent;
            for f in &fields {
                if kind != GenerateKind::Setters {
                    let g = if fluent {
                        f.name.clone()
                    } else {
                        getter_name(f)
                    };
                    if !has_signature(&g, &[]) {
                        blocks.push(format!(
                            "{indent}public {} {g}() {{\n{inner}return {};\n{indent}}}",
                            f.ty, f.name
                        ));
                    }
                }
                if kind != GenerateKind::Getters && !f.is_final {
                    let s = if fluent {
                        f.name.clone()
                    } else {
                        format!("set{}", capitalize(&f.name))
                    };
                    if has_signature(&s, &[f.ty.as_str()]) || (!fluent && has_method(&s)) {
                        continue;
                    }
                    if fluent {
                        blocks.push(format!(
                            "{indent}public {name} {s}({} {1}) {{\n{inner}this.{1} = {1};\n{inner}return this;\n{indent}}}",
                            f.ty, f.name
                        ));
                    } else {
                        blocks.push(format!(
                            "{indent}public void {s}({} {1}) {{\n{inner}this.{1} = {1};\n{indent}}}",
                            f.ty, f.name
                        ));
                    }
                }
            }
        }
        GenerateKind::EqualsHashCode => {
            if has_method("equals") || has_method("hashCode") {
                return Err(format!("{name} already has equals() or hashCode()"));
            }
            let mut that = {
                let mut c = name.chars();
                c.next()
                    .map(|f| f.to_lowercase().chain(c).collect::<String>())
                    .unwrap_or_default()
            };
            if that == "o" || fields.iter().any(|f| f.name == that) {
                that = "that".into();
            }
            let through_getters = matches!(
                template,
                GenerateTemplate::Getters | GenerateTemplate::InstanceofGetters
            );
            // A field read through its getter when the template asks for it and the class has one.
            let read = |f: &Field| {
                let g = getter_name(f);
                if through_getters && has_signature(&g, &[]) {
                    format!("{g}()")
                } else {
                    f.name.clone()
                }
            };
            let compare: Vec<String> = fields
                .iter()
                .map(|f| {
                    let v = read(f);
                    match f.ty.as_str() {
                        "float" => format!("Float.compare({v}, {that}.{v}) == 0"),
                        "double" => format!("Double.compare({v}, {that}.{v}) == 0"),
                        "int" | "long" | "short" | "byte" | "char" | "boolean" => {
                            format!("{v} == {that}.{v}")
                        }
                        t if t.ends_with("[]") => {
                            imports.push("java.util.Arrays");
                            format!("Arrays.equals({v}, {that}.{v})")
                        }
                        _ => {
                            imports.push("java.util.Objects");
                            format!("Objects.equals({v}, {that}.{v})")
                        }
                    }
                })
                .collect();
            let result = if compare.is_empty() {
                "true".to_owned()
            } else {
                compare.join(" && ")
            };
            let guard = if matches!(
                template,
                GenerateTemplate::Instanceof | GenerateTemplate::InstanceofGetters
            ) {
                format!("if (!(o instanceof {name})) return false;")
            } else {
                "if (o == null || getClass() != o.getClass()) return false;".to_owned()
            };
            blocks.push(format!(
                "{indent}@Override\n{indent}public boolean equals(Object o) {{\n\
                 {inner}{guard}\n\
                 {inner}{name} {that} = ({name}) o;\n\
                 {inner}return {result};\n{indent}}}"
            ));
            let (arrays, plain): (Vec<&&Field>, Vec<&&Field>) =
                fields.iter().partition(|f| f.ty.ends_with("[]"));
            let hash = plain.iter().map(|f| read(f)).collect::<Vec<_>>().join(", ");
            let body = if arrays.is_empty() {
                imports.push("java.util.Objects");
                format!("{inner}return Objects.hash({hash});\n")
            } else {
                let mut b = if plain.is_empty() {
                    format!("{inner}int result = 0;\n")
                } else {
                    imports.push("java.util.Objects");
                    format!("{inner}int result = Objects.hash({hash});\n")
                };
                for f in arrays {
                    b.push_str(&format!(
                        "{inner}result = 31 * result + Arrays.hashCode({});\n",
                        read(f)
                    ));
                }
                b.push_str(&format!("{inner}return result;\n"));
                b
            };
            blocks.push(format!(
                "{indent}@Override\n{indent}public int hashCode() {{\n{body}{indent}}}"
            ));
        }
        GenerateKind::ToString => {
            if has_method("toString") {
                return Err(format!("{name} already has toString()"));
            }
            let annotation = if options.override_annotation {
                format!("{indent}@Override\n")
            } else {
                String::new()
            };
            let value = |f: &Field, imports: &mut Vec<&str>| {
                if f.ty.ends_with("[]") {
                    imports.push("java.util.Arrays");
                    format!("Arrays.toString({})", f.name)
                } else {
                    f.name.clone()
                }
            };
            if template == GenerateTemplate::StringBuilder {
                let mut b = format!(
                    "{annotation}{indent}public String toString() {{\n{inner}StringBuilder sb = new StringBuilder(\"{name}{{\");\n"
                );
                for (i, f) in fields.iter().enumerate() {
                    let sep = if i == 0 { "" } else { ", " };
                    if f.ty == "String" {
                        b.push_str(&format!(
                            "{inner}sb.append(\"{sep}{0}='\").append({0}).append('\\'');\n",
                            f.name
                        ));
                    } else {
                        b.push_str(&format!(
                            "{inner}sb.append(\"{sep}{}=\").append({});\n",
                            f.name,
                            value(f, &mut imports)
                        ));
                    }
                }
                b.push_str(&format!(
                    "{inner}sb.append('}}');\n{inner}return sb.toString();\n{indent}}}"
                ));
                blocks.push(b);
            } else {
                let cont = format!("{inner}{unit}{unit}");
                let mut b = format!(
                    "{annotation}{indent}public String toString() {{\n{inner}return \"{name}{{\" +\n"
                );
                for (i, f) in fields.iter().enumerate() {
                    let sep = if i == 0 { "" } else { ", " };
                    let line = if f.ty == "String" {
                        format!("{cont}\"{sep}{0}='\" + {0} + '\\'' +\n", f.name)
                    } else {
                        format!(
                            "{cont}\"{sep}{}=\" + {} +\n",
                            f.name,
                            value(f, &mut imports)
                        )
                    };
                    b.push_str(&line);
                }
                b.push_str(&format!("{cont}'}}';\n{indent}}}"));
                blocks.push(b);
            }
        }
    }
    if blocks.is_empty() {
        return Err("nothing to generate: every field has its methods already".into());
    }
    let close = class.body.end_byte().saturating_sub(1);
    let (at, mut insert, before_close) = match member_insertion(text, class.body, offset) {
        // After the member at or before the caret (SPEC-0122 v2 behaviour 3, hand test 49).
        Some(at) => (at, String::new(), false),
        None => {
            // Before the line of the closing brace.
            let line_start = text[..close].rfind('\n').map_or(0, |i| i + 1);
            if text[line_start..close].trim().is_empty() {
                (line_start, String::new(), false)
            } else {
                (close, "\n".to_owned(), true)
            }
        }
    };
    for b in &blocks {
        insert.push('\n');
        insert.push_str(b);
        insert.push('\n');
    }
    if before_close {
        insert.push_str(&line_indent(text, close));
    } else if at < close && !text[at..].lines().next().unwrap_or("").trim().is_empty() {
        // A member follows directly: a blank line before it.
        insert.push('\n');
    }
    let mut edits: Vec<TextEdit> = Vec::new();
    imports.sort();
    imports.dedup();
    if let Some(e) = add_imports(text, &imports) {
        edits.push(e);
    }
    edits.push(TextEdit {
        range: at..at,
        text: insert,
    });
    edits.sort_by_key(|e| e.range.start);
    Ok(edits)
}

/// The kinds Generate offers in a Kotlin class (the reference's Kotlin menu): a secondary
/// constructor, `equals()`/`hashCode()` and `toString()`.
pub const KOTLIN_KINDS: [GenerateKind; 3] = [
    GenerateKind::Constructor,
    GenerateKind::EqualsHashCode,
    GenerateKind::ToString,
];

/// A Kotlin property: name and type as written (`None` when inferred).
fn kotlin_properties<'t>(class: Node<'t>, text: &str) -> Vec<(String, Option<String>)> {
    let src = |n: Node| text[n.byte_range()].to_owned();
    let mut out = Vec::new();
    let mut walk = class.walk();
    for part in class.named_children(&mut walk) {
        match part.kind() {
            "primary_constructor" => {
                let mut w = part.walk();
                for params in part.named_children(&mut w) {
                    let mut w2 = params.walk();
                    for p in params.named_children(&mut w2) {
                        // Only `val`/`var` parameters are properties.
                        let t = src(p);
                        let t = t.trim_start();
                        let declares = t.split_whitespace().any(|w| w == "val" || w == "var");
                        if p.kind() == "class_parameter" && declares {
                            let mut w3 = p.walk();
                            let named: Vec<Node> = p.named_children(&mut w3).collect();
                            let name = named.iter().find(|n| n.kind() == "identifier");
                            let ty = named.iter().rev().find(|n| n.kind().ends_with("_type"));
                            if let Some(name) = name {
                                out.push((src(*name), ty.map(|t| src(*t))));
                            }
                        }
                    }
                }
            }
            "class_body" => {
                let mut w = part.walk();
                for m in part.named_children(&mut w) {
                    if m.kind() != "property_declaration" {
                        continue;
                    }
                    let mut w2 = m.walk();
                    if let Some(v) = m
                        .named_children(&mut w2)
                        .find(|n| n.kind() == "variable_declaration")
                    {
                        let mut w3 = v.walk();
                        let named: Vec<Node> = v.named_children(&mut w3).collect();
                        if let Some(name) = named.iter().find(|n| n.kind() == "identifier") {
                            let ty = named.iter().find(|n| n.kind().ends_with("_type"));
                            out.push((src(*name), ty.map(|t| src(*t))));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Names of the functions declared in a Kotlin class body.
fn kotlin_functions(class: Node, text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut walk = class.walk();
    for part in class.named_children(&mut walk) {
        if part.kind() != "class_body" {
            continue;
        }
        let mut w = part.walk();
        for m in part.named_children(&mut w) {
            if m.kind() == "function_declaration"
                && let Some(n) = m.child_by_field_name("name")
            {
                names.push(text[n.byte_range()].to_owned());
            }
            if m.kind() == "secondary_constructor" {
                names.push("constructor".into());
            }
        }
    }
    names
}

/// Plans Generate in the Kotlin class around `offset` (`SPEC-0132`): `equals()`/`hashCode()`
/// and `toString()` over its properties (`val`/`var` constructor parameters and body properties),
/// or a secondary constructor taking the body properties when the class has no primary
/// constructor parameters. A data class has these already and is refused.
pub fn generate_kotlin(
    text: &str,
    offset: usize,
    kind: GenerateKind,
) -> Result<Vec<TextEdit>, String> {
    let tree = parse(Jvm::Kotlin, text).ok_or("the file could not be parsed")?;
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset, offset)
        .ok_or("the caret is not inside a Kotlin class")?;
    let class = loop {
        if node.kind() == "class_declaration" {
            break node;
        }
        node = node
            .parent()
            .ok_or("the caret is not inside a Kotlin class")?;
    };
    let name = class
        .child_by_field_name("name")
        .map(|n| text[n.byte_range()].to_owned())
        .unwrap_or_default();
    let head = &text[class.start_byte()
        ..class
            .child_by_field_name("name")
            .map_or(class.start_byte(), |n| n.start_byte())];
    if head.split_whitespace().any(|w| w == "data") && kind != GenerateKind::Constructor {
        return Err(format!("data class {name} has {} already", kind.label()));
    }
    let props = kotlin_properties(class, text);
    let funs = kotlin_functions(class, text);
    let mut walk = class.walk();
    let body = class
        .named_children(&mut walk)
        .find(|n| n.kind() == "class_body");
    let indent = match body {
        Some(b) => member_indent(text, b),
        None => format!("{}    ", line_indent(text, class.start_byte())),
    };
    let unit = if indent.contains('\t') { "\t" } else { "    " };
    let inner = format!("{indent}{unit}");
    let is_array = |t: &Option<String>| {
        t.as_deref()
            .is_some_and(|t| t.trim_end_matches('?').ends_with("Array") || t.starts_with("Array<"))
    };
    let nullable = |t: &Option<String>| t.as_deref().is_some_and(|t| t.ends_with('?'));
    let block = match kind {
        GenerateKind::EqualsHashCode => {
            if funs.iter().any(|f| f == "equals" || f == "hashCode") {
                return Err(format!("{name} already has equals() or hashCode()"));
            }
            let mut eq = format!(
                "{indent}override fun equals(other: Any?): Boolean {{\n\
                 {inner}if (this === other) return true\n\
                 {inner}if (javaClass != other?.javaClass) return false\n"
            );
            if !props.is_empty() {
                eq.push_str(&format!("\n{inner}other as {name}\n\n"));
                for (p, t) in &props {
                    if is_array(t) {
                        eq.push_str(&format!(
                            "{inner}if (!{p}.contentEquals(other.{p})) return false\n"
                        ));
                    } else {
                        eq.push_str(&format!("{inner}if ({p} != other.{p}) return false\n"));
                    }
                }
                eq.push('\n');
            }
            eq.push_str(&format!("{inner}return true\n{indent}}}\n\n"));
            let hash = |(p, t): &(String, Option<String>)| {
                if is_array(t) {
                    if nullable(t) {
                        format!("({p}?.contentHashCode() ?: 0)")
                    } else {
                        format!("{p}.contentHashCode()")
                    }
                } else if t.as_deref() == Some("Int") {
                    p.clone()
                } else if nullable(t) {
                    format!("({p}?.hashCode() ?: 0)")
                } else {
                    format!("{p}.hashCode()")
                }
            };
            let mut hc = format!("{indent}override fun hashCode(): Int {{\n");
            match props.split_first() {
                None => hc.push_str(&format!("{inner}return javaClass.hashCode()\n")),
                Some((first, [])) => hc.push_str(&format!("{inner}return {}\n", hash(first))),
                Some((first, rest)) => {
                    hc.push_str(&format!("{inner}var result = {}\n", hash(first)));
                    for p in rest {
                        hc.push_str(&format!("{inner}result = 31 * result + {}\n", hash(p)));
                    }
                    hc.push_str(&format!("{inner}return result\n"));
                }
            }
            hc.push_str(&format!("{indent}}}"));
            eq + &hc
        }
        GenerateKind::ToString => {
            if funs.iter().any(|f| f == "toString") {
                return Err(format!("{name} already has toString()"));
            }
            let parts: Vec<String> = props
                .iter()
                .map(|(p, t)| match t.as_deref() {
                    Some("String") | Some("String?") => format!("{p}='${p}'"),
                    _ if is_array(t) => format!("{p}=${{{p}.contentToString()}}"),
                    _ => format!("{p}=${p}"),
                })
                .collect();
            format!(
                "{indent}override fun toString(): String {{\n{inner}return \"{name}({})\"\n{indent}}}",
                parts.join(", ")
            )
        }
        GenerateKind::Constructor => {
            let mut w = class.walk();
            let primary_params = class
                .named_children(&mut w)
                .find(|n| n.kind() == "primary_constructor")
                .is_some_and(|p| text[p.byte_range()].contains(':'));
            if primary_params {
                return Err(format!(
                    "{name} has a primary constructor with parameters; a secondary one must call it"
                ));
            }
            let typed: Vec<&(String, Option<String>)> =
                props.iter().filter(|(_, t)| t.is_some()).collect();
            if typed.is_empty() {
                return Err(format!("{name} has no properties with a declared type"));
            }
            let list = typed
                .iter()
                .map(|(p, t)| format!("{p}: {}", t.as_deref().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(", ");
            let mut b = format!("{indent}constructor({list}) {{\n");
            for (p, _) in &typed {
                b.push_str(&format!("{inner}this.{p} = {p}\n"));
            }
            b.push_str(&format!("{indent}}}"));
            b
        }
        _ => {
            return Err(format!(
                "{} is not offered for Kotlin: properties have their accessors",
                kind.label()
            ));
        }
    };
    // Before the class body's closing brace, or as a new body.
    let Some(body) = body else {
        let at = class.end_byte();
        return Ok(vec![TextEdit {
            range: at..at,
            text: format!(" {{\n{block}\n{}}}", line_indent(text, class.start_byte())),
        }]);
    };
    let close = body.end_byte().saturating_sub(1);
    let line_start = text[..close].rfind('\n').map_or(0, |i| i + 1);
    let only_space = text[line_start..close].trim().is_empty();
    let (at, mut insert) = if only_space {
        (line_start, String::new())
    } else {
        (close, "\n".to_owned())
    };
    insert.push('\n');
    insert.push_str(&block);
    insert.push('\n');
    if !only_space {
        insert.push_str(&line_indent(text, close));
    }
    Ok(vec![TextEdit {
        range: at..at,
        text: insert,
    }])
}

/// `text` with `edits` (ascending, not overlapping) applied.
pub fn apply(text: &str, edits: &[TextEdit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for e in edits {
        out.push_str(&text[at..e.range.start]);
        out.push_str(&e.text);
        at = e.range.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA_TEST: &str = "package calc;\n\nimport org.junit.jupiter.api.Test;\n\nclass CalcTest {\n    @Test\n    void adds() {\n        int x = 1;\n    }\n\n    @org.junit.jupiter.params.ParameterizedTest\n    void many(int a) {}\n\n    void helper() {}\n}\n";

    #[test]
    fn markers_for_main_and_tests() {
        let m = run_markers(Jvm::Java, JAVA_TEST);
        let lines: Vec<(usize, MarkerKind, Option<&str>)> = m
            .iter()
            .map(|m| (m.line, m.kind, m.method.as_deref()))
            .collect();
        assert_eq!(
            lines,
            [
                (4, MarkerKind::TestClass, None),
                (6, MarkerKind::TestMethod, Some("adds")),
                (11, MarkerKind::TestMethod, Some("many")),
            ]
        );
        let main = "public class App {\n  static int f() { return 1; }\n  public static void main(String[] args) {}\n  class Inner { public static void main(String[] a) {} }\n}\n";
        let m = run_markers(Jvm::Java, main);
        assert_eq!(
            m.iter().map(|m| (m.line, m.kind)).collect::<Vec<_>>(),
            [(0, MarkerKind::Main), (2, MarkerKind::Main)]
        );
        assert_eq!(m[0].class, "App");
        let kt = "package a\nimport org.junit.jupiter.api.Test\nclass CalcTest {\n    @Test\n    fun adds() {}\n    fun helper() {}\n}\nfun main() {\n}\n";
        let m = run_markers(Jvm::Kotlin, kt);
        assert_eq!(
            m.iter()
                .map(|m| (m.line, m.kind, m.method.clone()))
                .collect::<Vec<_>>(),
            [
                (2, MarkerKind::TestClass, None),
                (4, MarkerKind::TestMethod, Some("adds".into())),
                (7, MarkerKind::Main, None),
            ]
        );
        assert!(run_markers(Jvm::Java, "class A { void f() {} }").is_empty());
    }

    #[test]
    fn test_method_around_a_line() {
        assert_eq!(
            test_method_at(Jvm::Java, JAVA_TEST, 7).and_then(|m| m.method),
            Some("adds".into())
        );
        assert!(
            test_method_at(Jvm::Java, JAVA_TEST, 13).is_none(),
            "helper is no test"
        );
        assert!(test_method_at(Jvm::Java, JAVA_TEST, 2).is_none());
    }

    #[test]
    fn java_imports_are_cleaned_and_laid_out() {
        let src = "package shop;\n\nimport static org.junit.Assert.assertEquals;\nimport java.util.List;\nimport shop.Cart;\nimport java.lang.String;\nimport java.util.Map;\nimport javax.inject.Inject;\nimport com.acme.Unused;\nimport com.acme.Used;\nimport org.x.*;\nimport java.util.List;\n\n/** See {@link Map}. */\nclass A {\n    @Inject Used u;\n    List<String> l;\n    void t() { assertEquals(1, 1); }\n}\n";
        let plan = optimize_imports(Jvm::Java, src).unwrap().unwrap();
        assert_eq!(
            plan.replacement,
            "import com.acme.Used;\nimport org.x.*;\n\nimport java.util.List;\nimport java.util.Map;\nimport javax.inject.Inject;\n\nimport static org.junit.Assert.assertEquals;"
        );
        assert_eq!(
            plan.removed,
            [
                "import shop.Cart;",
                "import java.lang.String;",
                "import com.acme.Unused;",
                "import java.util.List;"
            ]
        );
        let after = format!(
            "{}{}{}",
            &src[..plan.range.start],
            plan.replacement,
            &src[plan.range.end..]
        );
        assert_eq!(
            optimize_imports(Jvm::Java, &after).unwrap(),
            None,
            "a second run changes nothing"
        );
        // All imports unused: the block and its blank line go.
        let lone = "package a;\n\nimport java.util.List;\n\nclass A {}\n";
        let p = optimize_imports(Jvm::Java, lone).unwrap().unwrap();
        assert_eq!(
            format!(
                "{}{}{}",
                &lone[..p.range.start],
                p.replacement,
                &lone[p.range.end..]
            ),
            "package a;\n\nclass A {}\n"
        );
        assert!(
            optimize_imports(
                Jvm::Java,
                "import a.B;\n// note\nimport c.D;\nclass X { B b; D d; }"
            )
            .is_err()
        );
        assert_eq!(optimize_imports(Jvm::Java, "class X {}").unwrap(), None);
    }

    #[test]
    fn kotlin_imports_keep_conventions_and_aliases() {
        let src = "package a\n\nimport kotlin.math.max as mx\nimport androidx.compose.runtime.getValue\nimport java.io.File\nimport com.acme.Gone\nimport com.acme.Here\nimport kotlin.math.min\n\nfun f(h: Here) { val x by lazy { 1 }; println(mx(1, 2) + min(1, 2)); File(\"a\") }\n";
        let plan = optimize_imports(Jvm::Kotlin, src).unwrap().unwrap();
        assert_eq!(
            plan.replacement,
            "import androidx.compose.runtime.getValue\nimport com.acme.Here\nimport java.io.File\nimport kotlin.math.min\nimport kotlin.math.max as mx"
        );
        assert_eq!(plan.removed, ["import com.acme.Gone"]);
    }

    const POINT: &str = "package geo;\n\nimport java.util.List;\n\npublic class Point {\n    private final int x;\n    private String name;\n    private boolean visible;\n    private double[] weights;\n    private static int COUNT = 0;\n    private final List<String> tags = List.of();\n\n    public int getX() {\n        return x;\n    }\n}\n";

    /// With the caret after the last member: the methods go at the class end.
    fn generated(kind: GenerateKind) -> String {
        let at = POINT.rfind("    }\n}").unwrap() + 6;
        apply(POINT, &generate(POINT, at, kind).unwrap())
    }

    /// Hand test 49: the methods go where the caret is, after the member at or before it.
    /// SPEC-0122 v3 behaviour 3a (OBS-0027): only what applies is offered.
    const CHOOSER: &str = "class Person {\n    private final String name;\n    private int age;\n    private boolean active;\n\n    public int getAge() {\n        return age;\n    }\n}\n";

    #[test]
    fn chooser_lists_and_checks_the_fields() {
        let at = CHOOSER.find("private int").unwrap();
        let rows = |k| -> Vec<(String, bool)> {
            chooser_fields(CHOOSER, at, k)
                .into_iter()
                .map(|(f, c)| (f.name, c))
                .collect()
        };
        assert_eq!(
            rows(GenerateKind::Constructor),
            [
                ("name".to_owned(), true),
                ("age".to_owned(), false),
                ("active".to_owned(), false)
            ]
        );
        // `age` has its getter; `name` is final and gets no setter.
        assert_eq!(
            rows(GenerateKind::Getters),
            [("name".to_owned(), true), ("active".to_owned(), true)]
        );
        assert_eq!(
            rows(GenerateKind::Setters),
            [("age".to_owned(), true), ("active".to_owned(), true)]
        );
        assert_eq!(rows(GenerateKind::ToString).len(), 3);
    }

    fn with(kind: GenerateKind, options: &GenerateOptions) -> Result<String, String> {
        let at = CHOOSER.find("private int").unwrap();
        generate_with(CHOOSER, at, kind, options).map(|e| apply(CHOOSER, &e))
    }

    #[test]
    fn generate_uses_only_the_chosen_fields() {
        let fields = |n: &[&str]| GenerateOptions {
            fields: Some(n.iter().map(|s| (*s).to_owned()).collect()),
            ..Default::default()
        };
        let t = with(GenerateKind::Constructor, &fields(&["name"])).unwrap();
        assert!(t.contains("public Person(String name) {\n        this.name = name;\n    }"));
        // None chosen: the no-argument constructor.
        let t = with(GenerateKind::Constructor, &fields(&[])).unwrap();
        assert!(t.contains("public Person() {\n    }"), "{t}");
        let t = with(GenerateKind::ToString, &fields(&["age"])).unwrap();
        assert!(
            t.contains("\"age=\" + age +") && !t.contains("name='"),
            "{t}"
        );
        let t = with(GenerateKind::Getters, &fields(&["active"])).unwrap();
        assert!(t.contains("isActive") && !t.contains("getName"), "{t}");
        let e = with(GenerateKind::Getters, &fields(&["ghost"])).unwrap_err();
        assert!(e.contains("ghost"), "{e}");
    }

    #[test]
    fn generate_templates() {
        let opt = |template, override_annotation| GenerateOptions {
            fields: None,
            template,
            override_annotation,
        };
        let t = with(GenerateKind::Setters, &opt(GenerateTemplate::Fluent, true)).unwrap();
        assert!(
            t.contains(
                "public Person age(int age) {\n        this.age = age;\n        return this;\n    }"
            ),
            "{t}"
        );
        let t = with(GenerateKind::Getters, &opt(GenerateTemplate::Fluent, true)).unwrap();
        assert!(t.contains("public String name() {"), "{t}");
        let t = with(
            GenerateKind::EqualsHashCode,
            &opt(GenerateTemplate::Instanceof, true),
        )
        .unwrap();
        assert!(
            t.contains("if (!(o instanceof Person)) return false;"),
            "{t}"
        );
        assert!(!t.contains("getClass()"), "{t}");
        let t = with(
            GenerateKind::EqualsHashCode,
            &opt(GenerateTemplate::Getters, true),
        )
        .unwrap();
        // `age` has a getter, the others are read directly.
        assert!(t.contains("getAge() == person.getAge()"), "{t}");
        assert!(t.contains("Objects.hash(name, getAge(), active)"), "{t}");
        let t = with(
            GenerateKind::ToString,
            &opt(GenerateTemplate::StringBuilder, false),
        )
        .unwrap();
        assert!(t.contains("new StringBuilder(\"Person{\")"), "{t}");
        assert!(t.contains("sb.append(\", age=\").append(age);"), "{t}");
        assert!(
            t.contains("sb.append(\"name='\").append(name).append('\\'');"),
            "{t}"
        );
        assert!(!t.contains("@Override"), "{t}");
        let e = with(
            GenerateKind::Constructor,
            &opt(GenerateTemplate::Fluent, true),
        )
        .unwrap_err();
        assert!(e.contains("template"), "{e}");
    }

    #[test]
    fn generate_offers_what_applies() {
        use GenerateKind::*;
        let person = "class P {\n    private final String name;\n    private int age;\n\n}\n";
        let at = person.find("\n\n}").unwrap() + 1;
        assert_eq!(applicable_kinds(person, at), GenerateKind::ALL.to_vec());
        let basket = "class B {\n    private final java.util.List<String> items = new java.util.ArrayList<>();\n\n}\n";
        let at = basket.find("\n\n}").unwrap() + 1;
        assert_eq!(
            applicable_kinds(basket, at),
            [Constructor, Getters, EqualsHashCode, ToString]
        );
        let main = "class M {\n    static void main(String[] a) {\n        int x = 1;\n    }\n}\n";
        let at = main.find("int x").unwrap();
        assert_eq!(applicable_kinds(main, at), [Constructor, ToString]);
    }

    #[test]
    fn generated_methods_go_to_the_caret() {
        let caret_in = |needle: &str, kind| {
            let at = POINT.find(needle).unwrap() + 2;
            apply(POINT, &generate(POINT, at, kind).unwrap())
        };
        // Caret in the field `name`: right after its line, a blank line before the next field.
        let g = caret_in("private String", GenerateKind::Getters);
        assert!(
            g.contains("    private String name;\n\n    public String getName() {\n        return name;\n    }\n"),
            "{g}"
        );
        assert!(
            g.contains("    public List<String> getTags() {\n        return tags;\n    }\n\n    private boolean visible;"),
            "{g}"
        );
        // Caret on the blank line after the fields: before `getX`.
        let blank = POINT.find("\n\n    public int getX").unwrap() + 1;
        let t = apply(
            POINT,
            &generate(POINT, blank, GenerateKind::ToString).unwrap(),
        );
        let (to_string, get_x) = (t.find("toString()").unwrap(), t.find("getX()").unwrap());
        assert!(to_string < get_x, "{t}");
        // Caret inside `getX`: after it, which is the class end here.
        let inside = caret_in("return x;", GenerateKind::ToString);
        assert!(inside.find("getX()").unwrap() < inside.find("toString()").unwrap());
        assert!(inside.ends_with("    }\n}\n"), "{inside}");
    }

    #[test]
    fn constructor_getters_setters() {
        let c = generated(GenerateKind::Constructor);
        assert!(c.contains("    public Point(int x, String name, boolean visible, double[] weights) {\n        this.x = x;\n        this.name = name;\n        this.visible = visible;\n        this.weights = weights;\n    }\n}\n"), "{c}");
        let g = generated(GenerateKind::GettersAndSetters);
        assert!(
            !g.contains("public void setX("),
            "final field has no setter"
        );
        assert_eq!(
            g.matches("public int getX()").count(),
            1,
            "existing getter not repeated"
        );
        assert!(g.contains("    public boolean isVisible() {\n        return visible;\n    }"));
        assert!(
            g.contains("    public void setName(String name) {\n        this.name = name;\n    }")
        );
        assert!(g.contains("public List<String> getTags()"));
        assert!(!g.contains("getCOUNT"), "static fields are left out");
        assert!(g.ends_with("    }\n}\n"));
        let at = POINT.find("private String").unwrap();
        let (name, fields) = generate_fields(POINT, at, GenerateKind::Setters).unwrap();
        assert_eq!((name.as_str(), fields.len()), ("Point", 3));
    }

    #[test]
    fn equals_hash_code_and_to_string() {
        let e = generated(GenerateKind::EqualsHashCode);
        assert!(
            e.contains(
                "import java.util.List;\nimport java.util.Arrays;\nimport java.util.Objects;\n"
            ),
            "{e}"
        );
        assert!(e.contains("        if (o == null || getClass() != o.getClass()) return false;\n        Point point = (Point) o;\n        return x == point.x && Objects.equals(name, point.name) && visible == point.visible && Arrays.equals(weights, point.weights) && Objects.equals(tags, point.tags);\n"), "{e}");
        assert!(e.contains("        int result = Objects.hash(x, name, visible, tags);\n        result = 31 * result + Arrays.hashCode(weights);\n        return result;\n"));
        let t = generated(GenerateKind::ToString);
        assert!(t.contains("        return \"Point{\" +\n                \"x=\" + x +\n                \", name='\" + name + '\\'' +\n                \", visible=\" + visible +\n                \", weights=\" + Arrays.toString(weights) +\n                \", tags=\" + tags +\n                '}';\n"), "{t}");
        let twice = apply(&t, &[]);
        let at = twice.find("private String").unwrap();
        assert!(
            generate(&twice, at, GenerateKind::ToString)
                .unwrap_err()
                .contains("already has toString")
        );
        assert!(generate("class A { }", 100, GenerateKind::Getters).is_err());
        assert!(generate("int x;", 0, GenerateKind::Getters).is_err());
    }

    #[test]
    fn kotlin_generate_equals_hash_code_to_string_and_constructor() {
        let src = "class Point(val x: Int, var name: String?, other: Long) {\n    val ids: IntArray = IntArray(0)\n    fun f() {}\n}\n";
        let at = src.find("fun f").unwrap();
        let eq = apply(
            src,
            &generate_kotlin(src, at, GenerateKind::EqualsHashCode).unwrap(),
        );
        assert!(eq.contains("    override fun equals(other: Any?): Boolean {\n        if (this === other) return true\n        if (javaClass != other?.javaClass) return false\n\n        other as Point\n\n        if (x != other.x) return false\n        if (name != other.name) return false\n        if (!ids.contentEquals(other.ids)) return false\n\n        return true\n    }"), "{eq}");
        assert!(eq.contains("        var result = x\n        result = 31 * result + (name?.hashCode() ?: 0)\n        result = 31 * result + ids.contentHashCode()\n        return result\n"), "{eq}");
        assert!(
            !eq.contains("other.other"),
            "a plain parameter is not a property"
        );
        let ts = apply(
            src,
            &generate_kotlin(src, at, GenerateKind::ToString).unwrap(),
        );
        assert!(
            ts.contains("return \"Point(x=$x, name='$name', ids=${ids.contentToString()})\""),
            "{ts}"
        );
        assert!(generate_kotlin(&eq, at, GenerateKind::EqualsHashCode).is_err());
        assert!(
            generate_kotlin(src, at, GenerateKind::Constructor).is_err(),
            "primary parameters"
        );
        assert!(generate_kotlin(src, at, GenerateKind::Getters).is_err());

        let plain = "class Box {\n    var w: Int = 0\n    var label: String = \"\"\n}\n";
        let c = apply(
            plain,
            &generate_kotlin(plain, 12, GenerateKind::Constructor).unwrap(),
        );
        assert!(c.contains("    constructor(w: Int, label: String) {\n        this.w = w\n        this.label = label\n    }\n}"), "{c}");
        let data = "data class D(val a: Int)\n";
        assert!(generate_kotlin(data, 15, GenerateKind::ToString).is_err());
        let bare = "class E(val a: Int)\n";
        let t = apply(
            bare,
            &generate_kotlin(bare, 8, GenerateKind::ToString).unwrap(),
        );
        assert!(t.starts_with("class E(val a: Int) {\n    override fun toString(): String {\n        return \"E(a=$a)\"\n    }\n}"), "{t}");
    }
}
