//! Beans and configuration of Spring (`SPEC-0115`), Jakarta CDI with Quarkus and MicroProfile, and
//! Micronaut (`SPEC-0134`): IDECraft's own model of a project's Java sources, read with tree-sitter —
//! the beans (stereotype and scope annotations, `@Bean`/`@Produces`/factory methods), where they are
//! injected (constructors, `@Autowired`/`@Inject` fields and setters, `@Qualifier`/`@Named`), and the
//! configuration keys (`@Value("${…}")`, `@ConfigProperty`, `@Property`) against
//! `application.properties` / `application.yml`. Not a compiler: types are matched by simple name,
//! and only types declared in the project are checked.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tree_sitter::{Node, Parser};

use crate::inspections::Level;

/// Annotations that make a class a bean, as written (simple names): Spring stereotypes, CDI
/// scopes and EJB session beans (Jakarta EE, Quarkus), JAX-RS resources, Micronaut scopes and
/// factories.
const STEREOTYPES: &[&str] = &[
    "Component",
    "Service",
    "Repository",
    "Controller",
    "RestController",
    "Configuration",
    "SpringBootApplication",
    "ControllerAdvice",
    "RestControllerAdvice",
    "TestConfiguration",
    // Classes bound to configuration (Spring Boot, Micronaut; SPEC-0115 v3 behaviour 9).
    "ConfigurationProperties",
    // CDI and EJB (SPEC-0134).
    "ApplicationScoped",
    "RequestScoped",
    "SessionScoped",
    "ConversationScoped",
    "Dependent",
    "Singleton",
    "Stateless",
    "Stateful",
    "Path",
    // Micronaut.
    "Prototype",
    "Context",
    "Factory",
];

/// Classes whose annotated methods declare beans: Spring configurations and Micronaut factories.
const FACTORIES: &[&str] = &[
    "Configuration",
    "SpringBootApplication",
    "TestConfiguration",
    "Factory",
];

/// Scope annotations that declare a bean on a method of a Micronaut factory, besides `@Bean`.
const FACTORY_SCOPES: &[&str] = &["Singleton", "Prototype"];

/// Annotations that make an interface a bean the framework implements (`SPEC-0115` v3 behaviour
/// 9, `SPEC-0134` v3 behaviour 5): configuration mappings, declarative HTTP clients, and — by
/// [`is_repository_annotation`] — data repositories.
const INTERFACE_BEANS: &[&str] = &[
    "ConfigurationProperties",
    "ConfigMapping",
    "RegisterRestClient",
    "Client",
    "FeignClient",
];

/// Bean kinds whose annotation value is not a bean name (a path, a prefix, a client's address)
/// and whose constructor takes bound values, not beans.
const UNNAMED_KINDS: &[&str] = &[
    "Path",
    "ConfigurationProperties",
    "ConfigMapping",
    "RegisterRestClient",
    "Client",
    "FeignClient",
];

/// Annotations that declare a bean only under a condition: a profile or a requirement.
fn is_condition(name: &str) -> bool {
    name.starts_with("Conditional")
        || matches!(
            name,
            "Profile"
                | "Requires"
                | "IfBuildProfile"
                | "UnlessBuildProfile"
                | "IfBuildProperty"
                | "UnlessBuildProperty"
        )
}

/// Annotations of a bean that is used only when no other bean fits.
const FALLBACKS: &[&str] = &["ConditionalOnMissingBean", "DefaultBean", "Secondary"];

/// Whether an annotation on an interface marks a data repository (`@Repository`,
/// `@JdbcRepository`, `@MongoRepository`, …), which the framework implements.
fn is_repository_annotation(name: &str) -> bool {
    name.ends_with("Repository")
}

/// JVM system properties every application has; never written into the configuration files.
const SYSTEM_KEYS: &[&str] = &[
    "user.home",
    "user.dir",
    "user.name",
    "java.home",
    "java.version",
    "java.io.tmpdir",
    "os.name",
    "os.arch",
    "os.version",
    "file.encoding",
    "file.separator",
    "line.separator",
    "path.separator",
];

/// Annotations of configuration values, which are not bean lookups.
const CONFIG_VALUES: &[&str] = &["Value", "ConfigProperty", "Property"];

/// Generic containers whose injection takes any number of beans, or none.
const CONTAINERS: &[&str] = &[
    "List",
    "Set",
    "Collection",
    "Map",
    "Optional",
    "ObjectProvider",
    "Provider",
];

/// One bean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bean {
    /// Bean name: the annotation's value, else the class name with a small first letter, or the
    /// `@Bean` method's name.
    pub name: String,
    /// Declared type (simple name).
    pub type_name: String,
    /// Stereotype (`Service`, …) or `Bean` for a `@Bean` method.
    pub kind: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line of the class or method name.
    pub line: usize,
    /// Marked `@Primary`.
    pub primary: bool,
    /// Further names of the bean: the other entries of `@Bean(name = {"a", "b"})`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Qualifier annotations of the project's own on the bean (`SPEC-0134` v3 behaviour 5).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub qualifiers: Vec<String>,
    /// Declared under a condition (`@Profile`, `@Conditional…`, `@Requires`, …) of its own or of
    /// its class (`SPEC-0115` v3 behaviour 9).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conditional: bool,
    /// Used only when no other bean fits (`@ConditionalOnMissingBean`, `@DefaultBean`,
    /// `@Secondary`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fallback: bool,
    /// Declared in the test sources (`src/test/`): not a bean of the application (`SPEC-0115` v5
    /// behaviour 15).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub test: bool,
}

/// One place where a bean is injected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Injection {
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// Byte range of the injected type in the file.
    pub start: usize,
    /// End of that range.
    pub end: usize,
    /// Injected type (simple name, generics removed).
    pub type_name: String,
    /// `@Qualifier` value, if any.
    pub qualifier: Option<String>,
    /// Qualifier annotations of the project's own at the injection point.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub qualifiers: Vec<String>,
    /// Indices of the beans that can be injected, after `@Qualifier` and `@Primary`.
    pub candidates: Vec<usize>,
    /// Whether the type is declared in the project (only those are checked).
    pub own_type: bool,
    /// The class the injection point stands in (simple name).
    pub owner: String,
    /// The bean a producer method declares, when the injection point is one of its parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

/// One configuration property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Property {
    /// Full key, e.g. `server.port`.
    pub key: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column (chars) of the key on its line.
    pub column: usize,
    /// Byte range in the file of the key as written on its line (in YAML its last part).
    pub start: usize,
    /// End of that range.
    pub end: usize,
    /// The profile the definition belongs to: the file's (`application-dev.yml`), the YAML
    /// document's (`spring.config.activate.on-profile`) or the key's (`%dev.key`); none for the
    /// default profile (`SPEC-0115` v5 behaviour 13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The value as written, where it is a scalar. Never serialized: a value can be a secret, and
    /// is given out only through [`shown_value`](crate::spring_nav::shown_value).
    #[serde(skip)]
    pub value: Option<String>,
}

/// One `${key}` inside a `@Value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValueRef {
    /// The key.
    pub key: String,
    /// Whether a default follows (`${key:default}`).
    pub has_default: bool,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column (chars) of the key on its line.
    pub column: usize,
    /// Byte range of the key in the file.
    pub start: usize,
    /// End of that range.
    pub end: usize,
    /// Whether the key is bound in relaxed form (`@Value`, `@Property`): a key in kebab case also
    /// finds its camel-case or underscore spelling (`SPEC-0115` v3 behaviour 10).
    #[serde(skip)]
    pub relaxed: bool,
    /// The key is a prefix (`@ConfigurationProperties`, `@ConfigMapping`): it stands for every
    /// key below it (`SPEC-0115` v5 behaviour 12).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub prefix: bool,
    /// Byte range of the whole placeholder `${…}` in the file, for a `@Value` placeholder.
    #[serde(skip)]
    pub span: Option<(usize, usize)>,
}

/// A field a bean is injected into: the annotation that asks for it (`SPEC-0115` v4 behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInjection {
    /// Simple name of the annotation: `Autowired` or `Inject`.
    pub annotation: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line of the annotation.
    pub line: usize,
    /// Byte range of the annotation in the file.
    pub start: usize,
    /// End of that range.
    pub end: usize,
}

/// A finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpringProblem {
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// Byte range in the file.
    pub start: usize,
    /// End of that range.
    pub end: usize,
    /// Message.
    pub message: String,
    /// `warning` for a wiring or configuration mistake, `weak` for a recommendation of the
    /// framework's documentation (`SPEC-0115` v4 behaviour 11).
    pub severity: Level,
}

/// The model of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SpringModel {
    /// Beans, in file order.
    pub beans: Vec<Bean>,
    /// Injection points.
    pub injections: Vec<Injection>,
    /// Configuration properties.
    pub properties: Vec<Property>,
    /// `@Value`, `@ConfigProperty` and `@Property` keys.
    pub values: Vec<ValueRef>,
    /// Frameworks seen in the imports and named by the build files, in [`FRAMEWORKS`] order
    /// (`SPEC-0134` v2 behaviour 1).
    pub frameworks: Vec<String>,
    /// Fields annotated `@Autowired` or `@Inject`, for the recommendation of constructor
    /// injection (`SPEC-0115` v4 behaviour 11); reported through [`SpringModel::problems`].
    #[serde(skip)]
    pub field_injections: Vec<FieldInjection>,
}

/// Frameworks by import prefix, most specific first: a Quarkus project also imports CDI.
pub const FRAMEWORKS: &[(&str, &[&str])] = &[
    ("Quarkus", &["io.quarkus."]),
    ("Micronaut", &["io.micronaut."]),
    ("Spring", &["org.springframework."]),
    (
        "Jakarta EE",
        &[
            "jakarta.enterprise.",
            "javax.enterprise.",
            "jakarta.ejb.",
            "javax.ejb.",
            "jakarta.ws.rs.",
            "org.eclipse.microprofile.",
        ],
    ),
];

/// Frameworks by the group a build file names (`SPEC-0134` v2 behaviour 1): a Quarkus application
/// imports CDI, JAX-RS and MicroProfile like any Jakarta EE application, and only its build names
/// `io.quarkus` (dependencies, the `io.quarkus.platform` BOM, the Maven or Gradle plugin).
const BUILD_MARKERS: &[(&str, &str)] = &[
    ("Quarkus", "io.quarkus"),
    ("Micronaut", "io.micronaut"),
    ("Spring", "org.springframework"),
];

/// Whether `rel` is a Maven or Gradle build file (`SPEC-0134` v2 behaviour 1).
fn is_build_file(rel: &Path) -> bool {
    rel.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        matches!(
            n,
            "pom.xml"
                | "build.gradle"
                | "build.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "gradle.properties"
        )
    })
}

/// The frameworks whose group the build file `src` names as a whole: not after a name character or
/// `.`, and followed by `.`, `:`, a quote, `<`, `}`, whitespace or the end, so
/// `io.quarkus.platform` counts and `io.quarkusx` does not.
fn build_frameworks(src: &str) -> impl Iterator<Item = &'static str> + '_ {
    BUILD_MARKERS.iter().filter_map(move |(name, group)| {
        src.match_indices(group)
            .any(|(at, _)| {
                let before = src[..at].chars().next_back();
                let after = src[at + group.len()..].chars().next();
                let starts = before.is_none_or(|c| !(c.is_alphanumeric() || c == '.' || c == '_'));
                let ends = after.is_none_or(|c| {
                    matches!(c, '.' | ':' | '"' | '\'' | '<' | '}') || c.is_whitespace()
                });
                starts && ends
            })
            .then_some(*name)
    })
}

/// The frameworks whose packages `src` imports.
fn imported_frameworks(src: &str) -> impl Iterator<Item = &'static str> + '_ {
    FRAMEWORKS.iter().filter_map(move |(name, prefixes)| {
        src.lines()
            .map(str::trim_start)
            .filter_map(|l| l.strip_prefix("import "))
            .any(|l| {
                let l = l.trim_start_matches("static ").trim_start();
                prefixes.iter().any(|p| l.starts_with(p))
            })
            .then_some(*name)
    })
}

/// A class as read from the source.
struct ClassInfo {
    name: String,
    supertypes: Vec<String>,
    /// An interface that is not a bean by its own annotations and not marked `@NoRepositoryBean`:
    /// it is a bean when it extends a library repository type. With its file and line.
    repository_candidate: Option<(PathBuf, usize)>,
}

/// What the project's own annotation types add (`SPEC-0115` v3 behaviour 9, `SPEC-0134` v3
/// behaviour 5).
#[derive(Default)]
struct Composed {
    /// Composed annotations: name → the stereotype it carries as a meta-annotation.
    stereotypes: HashMap<String, String>,
    /// Qualifier annotations: annotation types annotated `@Qualifier`.
    qualifiers: HashSet<String>,
}

/// Reads the annotation types the project declares.
fn composed(files: &[(PathBuf, String)]) -> Composed {
    let mut metas: Vec<(String, Vec<String>)> = Vec::new();
    for (rel, src) in files {
        if !rel.extension().is_some_and(|e| e == "java") || !src.contains("@interface") {
            continue;
        }
        let mut parser = Parser::new();
        if parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .is_err()
        {
            continue;
        }
        let Some(tree) = parser.parse(src, None) else {
            continue;
        };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
            if node.kind() == "annotation_type_declaration"
                && let Some(name) = node.child_by_field_name("name")
            {
                let on = annotations(&node, src).into_iter().map(|(a, _)| a);
                metas.push((text(&name, src).to_owned(), on.collect()));
            }
        }
    }
    let mut out = Composed::default();
    for (name, on) in &metas {
        if on.iter().any(|a| a == "Qualifier") {
            out.qualifiers.insert(name.clone());
        }
    }
    // A composed annotation may carry another composed annotation.
    loop {
        let mut grew = false;
        for (name, on) in &metas {
            if out.stereotypes.contains_key(name) {
                continue;
            }
            let kind = on.iter().find_map(|a| {
                STEREOTYPES
                    .contains(&a.as_str())
                    .then(|| a.clone())
                    .or_else(|| out.stereotypes.get(a).cloned())
            });
            if let Some(kind) = kind {
                out.stereotypes.insert(name.clone(), kind);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    out
}

/// The bean names an annotation of `kind` gives: its value written alone or as `value = "x"`, for
/// `@Bean` also `name = "x"` or every entry of `name = {"x", "y"}` (the first is the name, the
/// others are aliases). Other attributes (`destroyMethod = "close"`) and paths are no names.
fn bean_names(kind: &str, args: &str) -> Vec<String> {
    if UNNAMED_KINDS.contains(&kind) {
        return Vec::new();
    }
    let Some(inner) = args
        .trim()
        .strip_prefix('(')
        .and_then(|a| a.strip_suffix(')'))
    else {
        return Vec::new();
    };
    crate::endpoints::split_args(inner)
        .into_iter()
        .map(crate::endpoints::named_arg)
        .find(|(n, _)| match n {
            None | Some("value") => true,
            Some("name") => kind == "Bean",
            Some(_) => false,
        })
        .map(|(_, v)| crate::endpoints::value_strings(v))
        .unwrap_or_default()
        .into_iter()
        .filter(|v| !v.starts_with('/'))
        .collect()
}

/// Whether `@Produces` with these arguments in this file is the CDI producer annotation: it takes
/// no arguments, while the JAX-RS and Micronaut HTTP annotations of the same name list media
/// types (`SPEC-0134` v3 behaviour 5).
fn is_cdi_produces(args: &str, src: &str) -> bool {
    let empty = args
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .is_empty();
    let http = src.contains("ws.rs.Produces") || src.contains("http.annotation.Produces");
    empty && (!http || src.contains("enterprise.inject.Produces"))
}

fn text<'a>(n: &Node, src: &'a str) -> &'a str {
    &src[n.byte_range()]
}

/// Simple name of a type: `java.util.List<Foo>` → `List`, `Foo[]` → `Foo`.
fn simple(ty: &str) -> String {
    let base = ty
        .split('<')
        .next()
        .unwrap_or(ty)
        .trim()
        .trim_end_matches("[]");
    base.rsplit('.').next().unwrap_or(base).trim().to_owned()
}

/// The annotations of a declaration's modifiers: (simple name, argument text).
fn annotations(decl: &Node, src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        if child.kind() != "modifiers" {
            continue;
        }
        let mut c2 = child.walk();
        for a in child.children(&mut c2) {
            if a.kind() == "marker_annotation" || a.kind() == "annotation" {
                let name = a
                    .child_by_field_name("name")
                    .map(|n| simple(text(&n, src)))
                    .unwrap_or_default();
                let args = a
                    .child_by_field_name("arguments")
                    .map(|n| text(&n, src).to_owned())
                    .unwrap_or_default();
                out.push((name, args));
            }
        }
    }
    out
}

/// The first annotation of `decl` with one of the simple `names`: that name and its node.
fn annotation_node<'t>(decl: &Node<'t>, src: &str, names: &[&str]) -> Option<(String, Node<'t>)> {
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        if child.kind() != "modifiers" {
            continue;
        }
        let mut c2 = child.walk();
        for a in child.children(&mut c2) {
            if a.kind() == "marker_annotation" || a.kind() == "annotation" {
                let name = a
                    .child_by_field_name("name")
                    .map(|n| simple(text(&n, src)))
                    .unwrap_or_default();
                if names.contains(&name.as_str()) {
                    return Some((name, a));
                }
            }
        }
    }
    None
}

/// The string value of annotation arguments: `("x")`, `(value = "x")`, `(name = "x")`.
fn string_arg(args: &str) -> Option<String> {
    let start = args.find('"')?;
    let rest = &args[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn decapitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        // Spring keeps names that start with two capitals as they are (`URLParser`).
        Some(c) if !chars.clone().next().is_some_and(char::is_uppercase) => {
            c.to_lowercase().chain(chars).collect()
        }
        _ => name.to_owned(),
    }
}

/// The 0-based column, in chars, of `byte` on its line.
fn column_of(src: &str, byte: usize) -> usize {
    let byte = byte.min(src.len());
    let line_start = src[..byte].rfind('\n').map_or(0, |i| i + 1);
    src[line_start..byte].chars().count()
}

fn line_of(src: &str, byte: usize) -> usize {
    src[..byte].matches('\n').count()
}

/// What one Java file contributes.
#[derive(Default)]
struct FileScan {
    classes: Vec<ClassInfo>,
    beans: Vec<Bean>,
    injections: Vec<Injection>,
    values: Vec<ValueRef>,
    field_injections: Vec<FieldInjection>,
}

fn scan_java(rel: &Path, src: &str, ctx: &Composed) -> FileScan {
    let mut out = FileScan::default();
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .is_err()
    {
        return out;
    }
    let Some(tree) = parser.parse(src, None) else {
        return out;
    };
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children.iter().rev() {
            stack.push(*child);
        }
        match node.kind() {
            "class_declaration" | "interface_declaration" | "record_declaration" => {
                class(rel, src, &node, ctx, &mut out)
            }
            "annotation" => value_refs(rel, src, &node, &mut out),
            _ => {}
        }
    }
    out
}

fn class(rel: &Path, src: &str, node: &Node, ctx: &Composed, out: &mut FileScan) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(&name_node, src).to_owned();
    let mut supertypes = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "superclass" | "super_interfaces" | "extends_interfaces"
        ) {
            let mut c2 = child.walk();
            let mut todo = vec![child];
            while let Some(n) = todo.pop() {
                if matches!(n.kind(), "type_identifier" | "scoped_type_identifier")
                    && n.parent().is_some_and(|p| p.kind() != "type_arguments")
                {
                    supertypes.push(simple(text(&n, src)));
                    continue;
                }
                for k in n.children(&mut c2) {
                    todo.push(k);
                }
                c2 = n.walk();
            }
        }
    }
    let annots = annotations(node, src);
    let interface = node.kind() == "interface_declaration";
    let line = line_of(src, name_node.start_byte());
    // The annotation that makes the declaration a bean: (kind, arguments). An interface is one
    // only where the framework implements it; a composed annotation gives its stereotype's kind.
    let stereotype: Option<(String, String)> = if interface {
        annots
            .iter()
            .find(|(a, _)| INTERFACE_BEANS.contains(&a.as_str()) || is_repository_annotation(a))
            .cloned()
    } else {
        annots
            .iter()
            .find(|(a, _)| STEREOTYPES.contains(&a.as_str()))
            .cloned()
            .or_else(|| {
                annots
                    .iter()
                    .find_map(|(a, _)| ctx.stereotypes.get(a))
                    .map(|kind| (kind.clone(), String::new()))
            })
    };
    out.classes.push(ClassInfo {
        name: name.clone(),
        supertypes,
        repository_candidate: (interface
            && stereotype.is_none()
            && !annots.iter().any(|(a, _)| a == "NoRepositoryBean"))
        .then(|| (rel.to_path_buf(), line)),
    });
    let class_conditional = annots.iter().any(|(a, _)| is_condition(a));
    let own_qualifiers = |annots: &[(String, String)]| -> Vec<String> {
        annots
            .iter()
            .filter(|(a, _)| ctx.qualifiers.contains(a))
            .map(|(a, _)| a.clone())
            .collect()
    };
    let is_fallback =
        |annots: &[(String, String)]| annots.iter().any(|(a, _)| FALLBACKS.contains(&a.as_str()));
    let configuration = annots.iter().any(|(a, _)| FACTORIES.contains(&a.as_str()));
    if let Some((kind, args)) = &stereotype {
        out.beans.push(Bean {
            // `@Named("x")` names a CDI or Micronaut bean; a Spring stereotype names it with its
            // value, while a JAX-RS or Micronaut path (`@Path("/a")`, `@Controller("/a")`) does not.
            name: annots
                .iter()
                .find(|(a, _)| a == "Named")
                .and_then(|(_, args)| string_arg(args))
                .or_else(|| bean_names(kind, args).into_iter().next())
                .unwrap_or_else(|| decapitalize(&name)),
            type_name: name.clone(),
            kind: kind.clone(),
            file: rel.to_path_buf(),
            line,
            primary: is_primary(&annots),
            aliases: Vec::new(),
            qualifiers: own_qualifiers(&annots),
            conditional: class_conditional,
            fallback: is_fallback(&annots),
            test: false,
        });
    }
    if interface {
        return;
    }
    let is_bean = stereotype.is_some();
    // A class bound to configuration takes values, not beans, in its constructor.
    let bound = stereotype
        .as_ref()
        .is_some_and(|(kind, _)| UNNAMED_KINDS.contains(&kind.as_str()) && kind != "Path");
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut constructors = Vec::new();
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        let member_annots = annotations(&member, src);
        let autowired = member_annots
            .iter()
            .any(|(a, _)| a == "Autowired" || a == "Inject");
        let named = member_annots
            .iter()
            .find(|(a, _)| a == "Named")
            .and_then(|(_, args)| string_arg(args));
        // What declares a bean on a member: `@Bean` in any Spring bean class or factory, a scope
        // in a Micronaut factory, CDI's `@Produces` in any bean class.
        let producer = member_annots.iter().find(|(a, args)| match a.as_str() {
            "Produces" => is_bean && is_cdi_produces(args, src),
            "Bean" => is_bean,
            a => configuration && FACTORY_SCOPES.contains(&a),
        });
        let mut produced = |ty: &Node, name_node: &Node, args: &str| {
            let mut names = bean_names("Bean", args).into_iter();
            out.beans.push(Bean {
                name: named
                    .clone()
                    .or_else(|| names.next())
                    .unwrap_or_else(|| text(name_node, src).to_owned()),
                aliases: names.collect(),
                type_name: simple(text(ty, src)),
                kind: "Bean".into(),
                file: rel.to_path_buf(),
                line: line_of(src, name_node.start_byte()),
                primary: is_primary(&member_annots),
                qualifiers: own_qualifiers(&member_annots),
                conditional: class_conditional
                    || member_annots.iter().any(|(a, _)| is_condition(a)),
                fallback: is_fallback(&member_annots),
                test: false,
            });
        };
        match member.kind() {
            "constructor_declaration" => constructors.push((member, autowired)),
            "field_declaration" => {
                if let Some((annotation, at)) =
                    annotation_node(&member, src, &["Autowired", "Inject"])
                {
                    out.field_injections.push(FieldInjection {
                        annotation,
                        file: rel.to_path_buf(),
                        line: line_of(src, at.start_byte()),
                        start: at.start_byte(),
                        end: at.end_byte(),
                    });
                }
                let Some(ty) = member.child_by_field_name("type") else {
                    continue;
                };
                if let Some((a, args)) = producer
                    && a == "Produces"
                {
                    // A producer field.
                    if let Some(n) = member
                        .child_by_field_name("declarator")
                        .and_then(|d| d.child_by_field_name("name"))
                    {
                        produced(&ty, &n, args);
                    }
                } else if autowired
                    && is_bean
                    && !member_annots
                        .iter()
                        .any(|(a, _)| CONFIG_VALUES.contains(&a.as_str()))
                {
                    injection(rel, src, &ty, &member_annots, ctx, (&name, None), out);
                }
            }
            "method_declaration" => {
                let mut made = None;
                if let Some((_, args)) = producer
                    && let (Some(ty), Some(m)) = (
                        member.child_by_field_name("type"),
                        member.child_by_field_name("name"),
                    )
                {
                    produced(&ty, &m, args);
                    made = out.beans.last().map(|b| b.name.clone());
                }
                // Producer method parameters and @Autowired/@Inject setters are injection points.
                if producer.is_some() || (autowired && is_bean) {
                    parameters(rel, src, &member, ctx, (&name, made.as_deref()), out);
                }
            }
            _ => {}
        }
    }
    if is_bean && !bound {
        // The constructor the framework uses: the @Autowired/@Inject one, else the only one.
        let chosen = constructors
            .iter()
            .find(|(_, a)| *a)
            .or_else(|| (constructors.len() == 1).then(|| &constructors[0]));
        if let Some((c, _)) = chosen {
            parameters(rel, src, c, ctx, (&name, None), out);
        }
    }
}

fn parameters(
    rel: &Path,
    src: &str,
    decl: &Node,
    ctx: &Composed,
    owner: (&str, Option<&str>),
    out: &mut FileScan,
) {
    let Some(params) = decl.child_by_field_name("parameters") else {
        return;
    };
    let mut cursor = params.walk();
    for p in params.children(&mut cursor) {
        if p.kind() == "formal_parameter"
            && let Some(ty) = p.child_by_field_name("type")
        {
            let annots = annotations(&p, src);
            if annots
                .iter()
                .any(|(a, _)| CONFIG_VALUES.contains(&a.as_str()))
            {
                continue;
            }
            injection(rel, src, &ty, &annots, ctx, owner, out);
        }
    }
}

fn injection(
    rel: &Path,
    src: &str,
    ty: &Node,
    annots: &[(String, String)],
    ctx: &Composed,
    owner: (&str, Option<&str>),
    out: &mut FileScan,
) {
    let written = text(ty, src);
    let outer = simple(written);
    // Containers and arrays take any number of beans; a primitive is a value.
    if CONTAINERS.contains(&outer.as_str())
        || matches!(
            ty.kind(),
            "integral_type" | "floating_point_type" | "boolean_type" | "array_type"
        )
    {
        return;
    }
    out.injections.push(Injection {
        file: rel.to_path_buf(),
        line: line_of(src, ty.start_byte()),
        start: ty.start_byte(),
        end: ty.end_byte(),
        type_name: outer,
        qualifier: annots
            .iter()
            .find(|(a, _)| a == "Qualifier" || a == "Named")
            .and_then(|(_, args)| string_arg(args)),
        qualifiers: annots
            .iter()
            .filter(|(a, _)| ctx.qualifiers.contains(a))
            .map(|(a, _)| a.clone())
            .collect(),
        candidates: Vec::new(),
        own_type: false,
        owner: owner.0.to_owned(),
        producer: owner.1.map(str::to_owned),
    });
}

/// Whether a bean wins among several: Spring's and Micronaut's `@Primary`, or a CDI
/// `@Alternative` enabled with `@Priority`.
fn is_primary(annots: &[(String, String)]) -> bool {
    let has = |n: &str| annots.iter().any(|(a, _)| a == n);
    has("Primary") || (has("Alternative") && has("Priority"))
}

fn value_refs(rel: &Path, src: &str, node: &Node, out: &mut FileScan) {
    let name = node
        .child_by_field_name("name")
        .map(|n| simple(text(&n, src)))
        .unwrap_or_default();
    // MicroProfile `@ConfigProperty(name = "k", defaultValue = "…")` and Micronaut
    // `@Property(name = "k")` name the key directly (SPEC-0134 behaviour 3).
    if name == "ConfigProperty" || name == "Property" {
        let Some(args) = node.child_by_field_name("arguments") else {
            return;
        };
        let body = text(&args, src);
        let Some(at) = body.find("name") else {
            return;
        };
        let Some(open) = body[at..].find('"').map(|i| at + i + 1) else {
            return;
        };
        let Some(close) = body[open..].find('"').map(|i| open + i) else {
            return;
        };
        let start = args.start_byte() + open;
        // An `Optional` value is empty, not an error, when the key is missing.
        let optional = node
            .parent()
            .and_then(|m| m.parent())
            .and_then(|d| d.child_by_field_name("type"))
            .is_some_and(|ty| simple(text(&ty, src)).starts_with("Optional"));
        out.values.push(ValueRef {
            relaxed: name == "Property",
            prefix: false,
            span: None,
            key: body[open..close].to_owned(),
            has_default: optional || body.contains("defaultValue"),
            file: rel.to_path_buf(),
            line: line_of(src, start),
            column: column_of(src, start),
            start,
            end: args.start_byte() + close,
        });
        return;
    }
    // The prefix of a class bound to configuration: `@ConfigurationProperties("shop")`,
    // `(prefix = "shop")`, `(value = "shop")`, `@ConfigMapping(prefix = "greeting")`.
    if name == "ConfigurationProperties" || name == "ConfigMapping" {
        let Some(args) = node.child_by_field_name("arguments") else {
            return;
        };
        let body = text(&args, src);
        let from = body.find("prefix").unwrap_or(0);
        let Some(open) = body[from..].find('"').map(|i| from + i + 1) else {
            return;
        };
        let Some(close) = body[open..].find('"').map(|i| open + i) else {
            return;
        };
        if open == close {
            return;
        }
        let start = args.start_byte() + open;
        out.values.push(ValueRef {
            relaxed: true,
            prefix: true,
            span: None,
            key: body[open..close].to_owned(),
            has_default: true,
            file: rel.to_path_buf(),
            line: line_of(src, start),
            column: column_of(src, start),
            start,
            end: args.start_byte() + close,
        });
        return;
    }
    let Some(args) = node
        .child_by_field_name("arguments")
        .filter(|_| name == "Value")
    else {
        return;
    };
    let base = args.start_byte();
    let body = text(&args, src);
    let mut from = 0;
    while let Some(open) = body[from..].find("${").map(|i| from + i + 2) {
        let Some(close) = body[open..].find('}').map(|i| open + i) else {
            break;
        };
        let inner = &body[open..close];
        let (key, has_default) = match inner.find(':') {
            Some(c) => (&inner[..c], true),
            None => (inner, false),
        };
        out.values.push(ValueRef {
            relaxed: true,
            prefix: false,
            span: Some((base + open - 2, base + close + 1)),
            key: key.trim().to_owned(),
            has_default,
            file: rel.to_path_buf(),
            line: line_of(src, base + open),
            column: column_of(src, base + open),
            start: base + open,
            end: base + open + key.len(),
        });
        from = close;
    }
}

/// One key of a configuration file: where it is written and, for a scalar, its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    /// Full key.
    pub key: String,
    /// 0-based line.
    pub line: usize,
    /// Byte range in the file of the key as written on its line (in YAML its last part; for a
    /// scalar list item the item).
    pub start: usize,
    /// End of that range.
    pub end: usize,
    /// The value as written, without quotes; none for a block scalar or a key without a value.
    pub value: Option<String>,
    /// 0-based index of the YAML document the key is in; 0 in a `.properties` file.
    pub document: usize,
}

/// The lines of `text` with the byte each starts at, without their line ends.
fn lines_with_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split_inclusive('\n').map(move |raw| {
        let start = at;
        at += raw.len();
        (start, raw.trim_end_matches(['\n', '\r']))
    })
}

/// A YAML scalar without its quotes.
fn unquoted(value: &str) -> &str {
    let v = value.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

/// Keys of a `.properties` file with their lines: the key ends at `=`, `:` or white space; a line
/// ending in `\` continues on the next one, which is no key.
pub fn properties_keys(text: &str) -> Vec<(String, usize)> {
    properties_entries(text)
        .into_iter()
        .map(|e| (e.key, e.line))
        .collect()
}

/// The keys of a `.properties` file with their places and values (`SPEC-0115` v5 behaviour 13):
/// the value follows the separator, and a continued line adds the next line without its leading
/// white space.
pub fn properties_entries(text: &str) -> Vec<ConfigEntry> {
    let mut out: Vec<ConfigEntry> = Vec::new();
    let mut continued = false;
    for (i, (offset, l)) in lines_with_offsets(text).enumerate() {
        let continuation = continued;
        let t = l.trim_start();
        let comment = !continuation && (t.starts_with('#') || t.starts_with('!'));
        let slashes = l.len() - l.trim_end_matches('\\').len();
        continued = !comment && slashes % 2 == 1;
        let body = if continued { &t[..t.len() - 1] } else { t };
        if continuation {
            if let Some(value) = out.last_mut().and_then(|e| e.value.as_mut()) {
                value.push_str(body);
            }
            continue;
        }
        if comment || t.is_empty() {
            continue;
        }
        let end = t
            .find(|c: char| c == '=' || c == ':' || c.is_whitespace())
            .unwrap_or(t.len());
        if end == 0 {
            continue;
        }
        let rest = body.get(end..).unwrap_or("").trim_start();
        let rest = rest.strip_prefix(['=', ':']).map_or(rest, str::trim_start);
        let start = offset + (l.len() - t.len());
        out.push(ConfigEntry {
            key: t[..end].to_owned(),
            line: i,
            start,
            end: start + end,
            value: Some(rest.to_owned()),
            document: 0,
        });
    }
    for e in &mut out {
        if let Some(v) = &mut e.value {
            *v = v.trim_end().to_owned();
        }
    }
    out
}

/// Keys of a YAML file as the frameworks flatten them: the keys that carry a value, their parents
/// joined with dots by indentation, list items with their index (`a.b[0].c`); every document of
/// a multi-document file.
pub fn yaml_keys(text: &str) -> Vec<(String, usize)> {
    yaml_entries(text)
        .into_iter()
        .map(|e| (e.key, e.line))
        .collect()
}

/// The keys of a YAML file with their places, scalar values and documents (`SPEC-0115` v5
/// behaviour 13). Values are taken as written: quotes removed, nothing else parsed.
pub fn yaml_entries(text: &str) -> Vec<ConfigEntry> {
    /// One open level: its indentation, its part of the key, the index of its next list item.
    struct Level {
        indent: usize,
        part: String,
        item: bool,
        next: usize,
    }
    fn path(stack: &[Level]) -> String {
        let mut out = String::new();
        for l in stack {
            if !l.item && !out.is_empty() {
                out.push('.');
            }
            out.push_str(&l.part);
        }
        out
    }
    let mut stack: Vec<Level> = Vec::new();
    let mut out = Vec::new();
    let mut document = 0;
    // A key without a value: a property only when no deeper line follows it.
    let mut open: Option<(ConfigEntry, usize)> = None;
    // Inside a block scalar (`key: |`): deeper lines are text.
    let mut block: Option<usize> = None;
    for (i, (offset, line)) in lines_with_offsets(text).enumerate() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut indent = line.len() - t.len();
        if let Some(b) = block {
            if indent > b {
                continue;
            }
            block = None;
        }
        if t.starts_with("---") || t.starts_with("...") {
            out.extend(open.take().map(|(e, _)| e));
            stack.clear();
            if t.starts_with("---") && (i > 0 || !out.is_empty()) {
                document += 1;
            }
            continue;
        }
        let mut content = t;
        if t == "-" || t.starts_with("- ") {
            // A list item of the key above it, at its indentation or deeper.
            while stack
                .last()
                .is_some_and(|l| l.indent > indent || (l.indent == indent && l.item))
            {
                stack.pop();
            }
            open = None;
            let Some(parent) = stack.last_mut() else {
                continue;
            };
            let index = parent.next;
            parent.next += 1;
            stack.push(Level {
                indent,
                part: format!("[{index}]"),
                item: true,
                next: 0,
            });
            let rest = t[1..].trim_start();
            if rest.is_empty() {
                continue;
            }
            indent += t.len() - rest.len();
            content = rest;
            if key_end(content).is_none() {
                // A scalar item.
                let start = offset + indent;
                let value = content.split(" #").next().unwrap_or(content).trim();
                out.push(ConfigEntry {
                    key: path(&stack),
                    line: i,
                    start,
                    end: start + value.len(),
                    value: Some(unquoted(value).to_owned()),
                    document,
                });
                continue;
            }
        }
        let Some(colon) = key_end(content) else {
            continue;
        };
        if let Some((entry, depth)) = open.take()
            && indent <= depth
        {
            out.push(entry);
        }
        let raw = &content[..colon];
        let trimmed = raw.trim();
        let key = trimmed.trim_matches(['"', '\'']);
        let start = offset
            + indent
            + (raw.len() - raw.trim_start().len())
            + (trimmed.len() - trimmed.trim_start_matches(['"', '\'']).len());
        let value = content[colon + 1..].trim();
        let value = value.split(" #").next().unwrap_or(value).trim();
        while stack.last().is_some_and(|l| l.indent >= indent) {
            stack.pop();
        }
        stack.push(Level {
            indent,
            part: key.to_owned(),
            item: false,
            next: 0,
        });
        let mut entry = ConfigEntry {
            key: path(&stack),
            line: i,
            start,
            end: start + key.len(),
            value: None,
            document,
        };
        if value.is_empty() {
            open = Some((entry, indent));
        } else {
            if value.starts_with(['|', '>']) {
                block = Some(indent);
            } else {
                entry.value = Some(unquoted(value).to_owned());
            }
            out.push(entry);
        }
    }
    out.extend(open.map(|(e, _)| e));
    out
}

/// Where the key of a YAML mapping line ends: the first `:` followed by a space or the line end.
fn key_end(content: &str) -> Option<usize> {
    content.match_indices(':').map(|(i, _)| i).find(|&i| {
        content[i + 1..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
    })
}

/// The profile a key names itself: `dev` for `%dev.key` (Quarkus, MicroProfile).
fn key_profile(key: &str) -> Option<&str> {
    key.strip_prefix('%')?.split_once('.').map(|(p, _)| p)
}

/// The profile a configuration file is for: `dev` for `application-dev.yml`.
fn file_profile(rel: &Path) -> Option<String> {
    let stem = rel.file_stem()?.to_str()?;
    let (_, profile) = stem.split_once('-')?;
    (!profile.is_empty()).then(|| profile.to_owned())
}

/// A configuration key without its profile: Quarkus and MicroProfile write `%dev.key` for a key
/// of the profile `dev`.
pub(crate) fn without_profile(key: &str) -> &str {
    match key.strip_prefix('%') {
        Some(rest) => rest.split_once('.').map_or(key, |(_, k)| k),
        None => key,
    }
}

/// A key as relaxed binding compares it: no `-` or `_`, lower case.
pub(crate) fn canonical(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether the configuration key `defined` gives `wanted` its value (`SPEC-0115` v3 behaviour
/// 10): the same key, for any profile, and for `@Value`/`@Property` a key written in lower-case
/// kebab form also in its camel-case or underscore spelling.
pub(crate) fn defines(defined: &str, wanted: &ValueRef) -> bool {
    if wanted.prefix {
        // Every key below the prefix.
        let (key, prefix) = (canonical(without_profile(defined)), canonical(&wanted.key));
        return key
            .strip_prefix(&prefix)
            .is_some_and(|rest| rest.starts_with(['.', '[']));
    }
    let defined = without_profile(defined);
    defined == wanted.key
        || (wanted.relaxed
            && !wanted.key.chars().any(|c| c.is_uppercase() || c == '_')
            && canonical(defined) == canonical(&wanted.key))
}

/// Keys the environment gives every application, so no configuration file needs them: JVM system
/// properties, Spring Boot's random values, and names in the form of environment variables.
pub(crate) fn is_external_key(key: &str) -> bool {
    SYSTEM_KEYS.contains(&key)
        || key.starts_with("random.")
        || (key.chars().any(|c| c.is_ascii_uppercase())
            && key
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

/// Whether the file is a test source of the usual build layout.
pub(crate) fn is_test_source(rel: &Path) -> bool {
    rel.to_string_lossy()
        .replace('\\', "/")
        .contains("src/test/")
}

pub(crate) fn is_config_file(rel: &Path) -> Option<bool> {
    let name = rel.file_name()?.to_str()?;
    if !name.starts_with("application") && !name.starts_with("bootstrap") {
        return None;
    }
    if name.ends_with(".properties") {
        Some(false)
    } else if name.ends_with(".yml") || name.ends_with(".yaml") {
        Some(true)
    } else {
        None
    }
}

/// The model of the given files (workspace-relative path and text).
pub fn model_of(files: &[(PathBuf, String)]) -> SpringModel {
    let mut model = SpringModel::default();
    let mut classes: HashMap<String, Vec<String>> = HashMap::new();
    let ctx = composed(files);
    let mut repositories: Vec<(String, PathBuf, usize)> = Vec::new();
    for (rel, text) in files {
        if rel.extension().is_some_and(|e| e == "java") {
            let scan = scan_java(rel, text, &ctx);
            for f in imported_frameworks(text) {
                if !model.frameworks.iter().any(|x| x == f) {
                    model.frameworks.push(f.to_owned());
                }
            }
            for c in scan.classes {
                if let Some((file, line)) = c.repository_candidate {
                    repositories.push((c.name.clone(), file, line));
                }
                classes.entry(c.name).or_default().extend(c.supertypes);
            }
            model.beans.extend(scan.beans);
            model.injections.extend(scan.injections);
            model.values.extend(scan.values);
            model.field_injections.extend(scan.field_injections);
        } else if is_build_file(rel) {
            for f in build_frameworks(text) {
                if !model.frameworks.iter().any(|x| x == f) {
                    model.frameworks.push(f.to_owned());
                }
            }
        } else if let Some(yaml) = is_config_file(rel) {
            let entries = if yaml {
                yaml_entries(text)
            } else {
                properties_entries(text)
            };
            // The profile of each YAML document, else the file's.
            let of_file = file_profile(rel);
            let of_document = |document: usize| -> Option<String> {
                entries
                    .iter()
                    .filter(|e| e.document == document)
                    .find(|e| {
                        e.key == "spring.config.activate.on-profile" || e.key == "spring.profiles"
                    })
                    .and_then(|e| e.value.clone())
            };
            let profiles: Vec<Option<String>> = entries
                .iter()
                .map(|e| {
                    key_profile(&e.key)
                        .map(str::to_owned)
                        .or_else(|| of_document(e.document))
                        .or_else(|| of_file.clone())
                })
                .collect();
            model
                .properties
                .extend(
                    entries
                        .into_iter()
                        .zip(profiles)
                        .map(|(e, profile)| Property {
                            key: e.key,
                            file: rel.clone(),
                            line: e.line,
                            column: column_of(text, e.start),
                            start: e.start,
                            end: e.end,
                            profile,
                            value: e.value,
                        }),
                );
        }
    }
    // All types a bean can be injected as: its type and, transitively, the project's supertypes.
    let assignable = |ty: &str| -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut todo = vec![ty.to_owned()];
        while let Some(t) = todo.pop() {
            if seen.insert(t.clone())
                && let Some(sup) = classes.get(&t)
            {
                todo.extend(sup.iter().cloned());
            }
        }
        seen
    };
    // An interface that extends a repository type of a library (Spring Data's `Repository`,
    // `CrudRepository`, `JpaRepository`, …) is a bean the framework implements.
    for (name, file, line) in repositories {
        if assignable(&name)
            .iter()
            .any(|t| !classes.contains_key(t) && t.ends_with("Repository"))
        {
            model.beans.push(Bean {
                name: decapitalize(&name),
                type_name: name,
                kind: "Repository".into(),
                file,
                line,
                primary: false,
                aliases: Vec::new(),
                qualifiers: Vec::new(),
                conditional: false,
                fallback: false,
                test: false,
            });
        }
    }
    for b in &mut model.beans {
        b.test = is_test_source(&b.file);
    }
    let rank = |f: &String| FRAMEWORKS.iter().position(|(n, _)| n == f);
    model.frameworks.sort_by_key(rank);
    // In CDI a bean with a qualifier of its own is not a `@Default` bean.
    let cdi = matches!(model.label(), "Quarkus" | "Jakarta EE");
    let bean_types: Vec<HashSet<String>> = model
        .beans
        .iter()
        .map(|b| assignable(&b.type_name))
        .collect();
    for inj in &mut model.injections {
        inj.own_type = classes.contains_key(&inj.type_name);
        let in_test = is_test_source(&inj.file);
        let mut found: Vec<usize> = (0..model.beans.len())
            .filter(|&i| bean_types[i].contains(&inj.type_name))
            // A bean of the test sources is not on the application's class path.
            .filter(|&i| in_test || !is_test_source(&model.beans[i].file))
            .collect();
        if let Some(q) = &inj.qualifier {
            found.retain(|&i| model.beans[i].name == *q || model.beans[i].aliases.contains(q));
        }
        if !inj.qualifiers.is_empty() {
            found.retain(|&i| {
                inj.qualifiers
                    .iter()
                    .all(|q| model.beans[i].qualifiers.contains(q))
            });
        } else if cdi && inj.qualifier.is_none() {
            found.retain(|&i| model.beans[i].qualifiers.is_empty());
        }
        if found.len() > 1 {
            let first: Vec<usize> = found
                .iter()
                .copied()
                .filter(|&i| !model.beans[i].fallback)
                .collect();
            if !first.is_empty() {
                found = first;
            }
        }
        if found.len() > 1 {
            let primary: Vec<usize> = found
                .iter()
                .copied()
                .filter(|&i| model.beans[i].primary)
                .collect();
            if primary.len() == 1 {
                found = primary;
            }
        }
        inj.candidates = found;
    }
    model
}

/// Reads the project's Java, configuration and build files and builds the model.
pub fn model(root: &Path, files: &[PathBuf]) -> SpringModel {
    let read: Vec<(PathBuf, String)> = files
        .iter()
        .filter(|rel| {
            rel.extension().is_some_and(|e| e == "java")
                || is_config_file(rel).is_some()
                || is_build_file(rel)
        })
        .filter_map(|rel| {
            let text = std::fs::read_to_string(root.join(rel)).ok()?;
            Some((rel.clone(), text))
        })
        .collect();
    model_of(&read)
}

/// The model of the project at `root` over its own sources (v2 behaviour 8): classes and
/// configuration files in a virtual environment, installed dependencies, build output or ignored
/// files are not the project's beans and keys.
pub fn project_model(root: &Path) -> SpringModel {
    model(root, &idecraft_core::OwnSources::new(root).files())
}

/// The recommendation on an injected field (`SPEC-0115` v4 behaviour 11).
pub const FIELD_INJECTION: &str = "Field injection: constructor injection is recommended";

impl SpringModel {
    /// Whether the project looks like a Spring project (it declares beans).
    pub fn is_spring(&self) -> bool {
        !self.beans.is_empty()
    }

    /// The framework findings are labelled with: the most specific one imported, Spring when the
    /// sources import none of them.
    pub fn label(&self) -> &str {
        self.frameworks.first().map_or("Spring", String::as_str)
    }

    /// Problems: an own type with no bean or several, `@Value` keys defined nowhere and without
    /// default (behaviour 4), and as weak warnings the injected fields (v4 behaviour 11).
    pub fn problems(&self) -> Vec<SpringProblem> {
        if !self.is_spring() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for inj in self.injections.iter().filter(|i| i.own_type) {
            let message = match inj.candidates.len() {
                0 => match &inj.qualifier {
                    Some(q) => format!("No bean named '{q}' of type {}", inj.type_name),
                    None => format!("No bean of type {} found", inj.type_name),
                },
                1 => continue,
                // Beans that each have a condition are alternatives the profile or the
                // environment chooses between.
                _ if inj.candidates.iter().all(|&i| self.beans[i].conditional) => continue,
                n => {
                    let names: Vec<&str> = inj
                        .candidates
                        .iter()
                        .map(|&i| self.beans[i].name.as_str())
                        .collect();
                    // How the framework picks one (SPEC-0134 behaviour 2).
                    let fix = match self.label() {
                        "Quarkus" | "Jakarta EE" => "mark one @Alternative @Priority or add @Named",
                        "Micronaut" => "mark one @Primary or add @Named",
                        _ => "mark one @Primary or add @Qualifier",
                    };
                    format!(
                        "{n} beans of type {} ({}); {fix}",
                        inj.type_name,
                        names.join(", ")
                    )
                }
            };
            out.push(SpringProblem {
                file: inj.file.clone(),
                line: inj.line,
                start: inj.start,
                end: inj.end,
                message,
                severity: Level::Warning,
            });
        }
        for v in &self.values {
            if !v.has_default
                && !v.prefix
                && !is_external_key(&v.key)
                && !self.properties.iter().any(|p| defines(&p.key, v))
            {
                out.push(SpringProblem {
                    file: v.file.clone(),
                    line: v.line,
                    start: v.start,
                    end: v.end,
                    message: format!(
                        "Property '{}' is not defined in the application configuration",
                        v.key
                    ),
                    severity: Level::Warning,
                });
            }
        }
        // Field injection, where the framework's own documentation recommends the constructor
        // (SPEC-0115 v4 behaviour 11): Spring's `@Autowired`, Micronaut's `@Inject`. Jakarta CDI
        // and Quarkus document field injection as a usual form. Not in test sources, where the
        // frameworks' test support injects into fields.
        let annotation = match self.label() {
            "Spring" => "Autowired",
            "Micronaut" => "Inject",
            _ => "",
        };
        for f in &self.field_injections {
            if f.annotation == annotation && !is_test_source(&f.file) {
                out.push(SpringProblem {
                    file: f.file.clone(),
                    line: f.line,
                    start: f.start,
                    end: f.end,
                    message: FIELD_INJECTION.into(),
                    severity: Level::Weak,
                });
            }
        }
        out
    }

    /// Where the bean or property at byte `at` of `file` is declared: the candidate beans of an
    /// injection point, or the definitions of a `@Value` key (behaviour 5).
    pub fn targets_at(&self, file: &Path, at: usize) -> Vec<(PathBuf, usize)> {
        if let Some(inj) = self
            .injections
            .iter()
            .find(|i| i.file == file && i.start <= at && at <= i.end)
        {
            return inj
                .candidates
                .iter()
                .map(|&i| (self.beans[i].file.clone(), self.beans[i].line))
                .collect();
        }
        if let Some(v) = self
            .values
            .iter()
            .find(|v| v.file == file && v.start <= at && at <= v.end)
        {
            return self
                .properties
                .iter()
                .filter(|p| defines(&p.key, v))
                .map(|p| (p.file.clone(), p.line))
                .collect();
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<(PathBuf, String)> {
        list.iter()
            .map(|(p, t)| (PathBuf::from(p), (*t).to_owned()))
            .collect()
    }

    const REPO: &str = "package a;\n@Repository\npublic class StockRepository implements Store {\n  private final JdbcClient jdbc;\n  public StockRepository(JdbcClient jdbc) { this.jdbc = jdbc; }\n}\n";
    const STORE: &str = "package a;\npublic interface Store {}\n";
    const CONTROLLER: &str = "package a;\n@RestController\nclass StockController {\n  @Autowired private Store store;\n  @Value(\"${server.port}\") int port;\n  StockController(StockRepository repo, @Value(\"${app.name:x}\") String name, List<Store> all) {}\n}\n";

    #[test]
    fn beans_and_injections_resolve_by_type_and_supertype() {
        let m = model_of(&files(&[
            ("src/StockRepository.java", REPO),
            ("src/Store.java", STORE),
            ("src/StockController.java", CONTROLLER),
            (
                "src/main/resources/application.properties",
                "server.port=8089\n# x\nspring.application.name: inv\n",
            ),
        ]));
        let names: Vec<(&str, &str, &str)> = m
            .beans
            .iter()
            .map(|b| (b.name.as_str(), b.type_name.as_str(), b.kind.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("stockRepository", "StockRepository", "Repository"),
                ("stockController", "StockController", "RestController")
            ]
        );
        let by: HashMap<&str, &Injection> = m
            .injections
            .iter()
            .map(|i| (i.type_name.as_str(), i))
            .collect();
        assert_eq!(
            by["Store"].candidates,
            [0],
            "an interface finds its implementation"
        );
        assert_eq!(by["StockRepository"].candidates, [0]);
        assert!(!by["JdbcClient"].own_type, "library types are not checked");
        assert!(!by.contains_key("List"), "containers take any number");
        assert!(
            !by.contains_key("String"),
            "@Value parameters are not beans"
        );
        assert!(messages(&m).is_empty(), "{:?}", m.problems());
        let keys: Vec<&str> = m.properties.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["server.port", "spring.application.name"]);
        assert_eq!(m.values.len(), 2);
        let at = CONTROLLER.find("server.port").unwrap() + 2;
        assert_eq!(
            m.targets_at(Path::new("src/StockController.java"), at),
            [(
                PathBuf::from("src/main/resources/application.properties"),
                0
            )]
        );
        let at = CONTROLLER.find("Store store").unwrap();
        assert_eq!(
            m.targets_at(Path::new("src/StockController.java"), at),
            [(PathBuf::from("src/StockRepository.java"), 2)]
        );
    }

    #[test]
    fn missing_ambiguous_qualified_and_primary() {
        let config = "@Configuration\nclass Config {\n  @Bean Store fast() { return null; }\n  @Bean(\"slow\") Store other(Clock clock) { return null; }\n}\n";
        let user = "@Service\nclass User {\n  User(Store s, @Qualifier(\"slow\") Store q, @Qualifier(\"none\") Store n, Clock c) {}\n  @Value(\"${missing.key}\") String v;\n}\n";
        let clock = "public class Clock {}\n";
        let m = model_of(&files(&[
            ("Store.java", STORE),
            ("Config.java", config),
            ("User.java", user),
            ("Clock.java", clock),
        ]));
        let messages: Vec<String> = m.problems().into_iter().map(|p| p.message).collect();
        assert_eq!(
            messages,
            [
                "No bean of type Clock found",
                "2 beans of type Store (fast, slow); mark one @Primary or add @Qualifier",
                "No bean named 'none' of type Store",
                "No bean of type Clock found",
                "Property 'missing.key' is not defined in the application configuration",
            ]
        );
        let primary = config.replace("@Bean Store fast", "@Primary @Bean Store fast");
        let m = model_of(&files(&[
            ("Store.java", STORE),
            ("Config.java", &primary),
            ("User.java", user),
        ]));
        let first = m
            .injections
            .iter()
            .find(|i| i.type_name == "Store")
            .unwrap();
        assert_eq!(first.candidates.len(), 1, "@Primary wins");
    }

    #[test]
    fn yaml_keys_flatten_by_indentation() {
        let yaml = "spring:\n  application:\n    name: inv\n  datasource:\n    url: x\n# c\nserver:\n  port: 8080\n";
        let keys: Vec<String> = yaml_keys(yaml).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            [
                "spring.application.name",
                "spring.datasource.url",
                "server.port"
            ]
        );
        assert_eq!(decapitalize("URLParser"), "URLParser");
        assert_eq!(decapitalize("Stock"), "stock");
        assert_eq!(simple("java.util.List<Foo>"), "List");
    }

    #[test]
    fn cdi_quarkus_beans_producers_and_config_properties() {
        let m = model_of(&files(&[
            (
                "src/Clock.java",
                "package a;\nimport jakarta.enterprise.context.ApplicationScoped;\nimport io.quarkus.runtime.Startup;\npublic interface Clock {}\n",
            ),
            (
                "src/SystemClock.java",
                "package a;\n@ApplicationScoped\npublic class SystemClock implements Clock {}\n",
            ),
            (
                "src/TestClock.java",
                "package a;\n@Alternative @Priority(1) @ApplicationScoped\npublic class TestClock implements Clock {}\n",
            ),
            (
                "src/Producers.java",
                "package a;\n@ApplicationScoped\nclass Producers {\n  @Produces @Named(\"utc\") Zone utc(Clock clock) { return null; }\n}\n",
            ),
            (
                "src/Greeter.java",
                "package a;\nimport org.eclipse.microprofile.config.inject.ConfigProperty;\n@Path(\"/hello\")\nclass Greeter {\n  @Inject Clock clock;\n  @Inject @Named(\"utc\") Zone zone;\n  @Inject @Named(\"local\") Zone other;\n  @ConfigProperty(name = \"greeting.text\") String text;\n  @ConfigProperty(name = \"greeting.count\", defaultValue = \"1\") int count;\n  @Inject Greeter(@ConfigProperty(name = \"greeting.name\") String name, Missing m) {}\n}\n",
            ),
            ("src/Zone.java", "package a;\npublic class Zone {}\n"),
            ("src/Missing.java", "package a;\npublic class Missing {}\n"),
            (
                "src/main/resources/application.properties",
                "greeting.name=x\n",
            ),
        ]));
        assert_eq!(m.frameworks, ["Quarkus", "Jakarta EE"]);
        assert_eq!(m.label(), "Quarkus");
        let beans: Vec<(&str, &str)> = m
            .beans
            .iter()
            .map(|b| (b.name.as_str(), b.kind.as_str()))
            .collect();
        assert!(
            beans.contains(&("systemClock", "ApplicationScoped")),
            "{beans:?}"
        );
        assert!(beans.contains(&("utc", "Bean")), "{beans:?}");
        assert!(beans.contains(&("greeter", "Path")), "{beans:?}");
        // The @Alternative @Priority bean wins; @Named picks the producer; config values and
        // the producer's own parameter are no bean lookups.
        let clock = m
            .injections
            .iter()
            .find(|i| {
                i.type_name == "Clock" && i.qualifier.is_none() && i.file.ends_with("Greeter.java")
            })
            .unwrap();
        assert_eq!(m.beans[clock.candidates[0]].name, "testClock");
        assert_eq!(clock.candidates.len(), 1);
        assert!(
            m.injections
                .iter()
                .all(|i| i.type_name != "String" && i.type_name != "int")
        );
        let mut problems: Vec<String> = m.problems().into_iter().map(|p| p.message).collect();
        problems.sort();
        assert_eq!(
            problems,
            [
                "No bean named 'local' of type Zone",
                "No bean of type Missing found",
                "Property 'greeting.text' is not defined in the application configuration",
            ]
        );
    }

    /// SPEC-0134/AK-6 (v2): the project of hand test Nr. 61 — code.quarkus.io's `pom.xml` with the
    /// BOM through properties, sources importing only CDI and MicroProfile.
    #[test]
    fn quarkus_is_read_from_the_build_file() {
        let sources = [
            (
                "src/main/java/dev/ht/Greeter.java",
                "package dev.ht;\npublic interface Greeter { String greet(String n); }\n",
            ),
            (
                "src/main/java/dev/ht/PlainGreeter.java",
                "package dev.ht;\nimport jakarta.enterprise.context.ApplicationScoped;\n@ApplicationScoped\npublic class PlainGreeter implements Greeter {}\n",
            ),
            (
                "src/main/java/dev/ht/LoudGreeter.java",
                "package dev.ht;\nimport jakarta.enterprise.context.ApplicationScoped;\n@ApplicationScoped\npublic class LoudGreeter implements Greeter {}\n",
            ),
            (
                "src/main/java/dev/ht/GreetingService.java",
                "package dev.ht;\nimport jakarta.enterprise.context.ApplicationScoped;\nimport jakarta.inject.Inject;\nimport org.eclipse.microprofile.config.inject.ConfigProperty;\n@ApplicationScoped\npublic class GreetingService {\n  @Inject Greeter greeter;\n  @ConfigProperty(name = \"greeting.suffix\") String suffix;\n}\n",
            ),
            ("src/main/resources/application.properties", "\n"),
        ];
        let jakarta_only = model_of(&files(&sources));
        assert_eq!(jakarta_only.frameworks, ["Jakarta EE"]);

        let pom = "<project>\n  <properties>\n    <quarkus.platform.artifact-id>quarkus-bom</quarkus.platform.artifact-id>\n    <quarkus.platform.group-id>io.quarkus.platform</quarkus.platform.group-id>\n  </properties>\n  <dependencies>\n    <dependency>\n      <groupId>io.quarkus</groupId>\n      <artifactId>quarkus-rest</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n";
        let mut with_pom = sources.to_vec();
        with_pom.push(("pom.xml", pom));
        let m = model_of(&files(&with_pom));
        assert_eq!(m.frameworks, ["Quarkus", "Jakarta EE"]);
        assert_eq!(m.label(), "Quarkus");
        let mut problems: Vec<String> = m
            .problems()
            .into_iter()
            .map(|p| format!("{}: {}", m.label(), p.message))
            .collect();
        problems.sort();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].starts_with("Quarkus: 2 beans of type Greeter"),
            "{problems:?}"
        );
        assert_eq!(
            problems[1],
            "Quarkus: Property 'greeting.suffix' is not defined in the application configuration"
        );

        // Gradle: the plugin id, or the platform group in gradle.properties; a module's pom.
        for (file, text) in [
            (
                "build.gradle.kts",
                "plugins {\n    java\n    id(\"io.quarkus\")\n}\n",
            ),
            ("build.gradle", "plugins {\n    id 'io.quarkus'\n}\n"),
            (
                "gradle.properties",
                "quarkusPlatformGroupId=io.quarkus.platform\n",
            ),
            ("app/pom.xml", "<groupId>io.quarkus</groupId>"),
        ] {
            let mut build = sources.to_vec();
            build.push((file, text));
            assert_eq!(model_of(&files(&build)).label(), "Quarkus", "{file}");
        }

        // A Jakarta EE application's own build stays Jakarta EE.
        let mut jee = sources.to_vec();
        jee.push((
            "pom.xml",
            "<dependency>\n  <groupId>jakarta.platform</groupId>\n  <artifactId>jakarta.jakartaee-api</artifactId>\n</dependency>\n",
        ));
        assert_eq!(model_of(&files(&jee)).frameworks, ["Jakarta EE"]);
    }

    /// SPEC-0134/AK-6 (v2): only whole groups count, and only in build files.
    #[test]
    fn build_markers_are_whole_groups() {
        let found = |src: &str| build_frameworks(src).collect::<Vec<_>>();
        assert_eq!(found("<groupId>io.quarkus</groupId>"), ["Quarkus"]);
        assert_eq!(
            found("implementation(\"io.quarkus:quarkus-arc\")"),
            ["Quarkus"]
        );
        assert_eq!(found("group=io.quarkus.platform\n"), ["Quarkus"]);
        assert_eq!(found("group=io.quarkus"), ["Quarkus"]);
        assert!(found("<groupId>io.quarkusx</groupId>").is_empty());
        assert!(found("<groupId>dev.io.quarkus</groupId>").is_empty());
        assert_eq!(
            found("<parent><groupId>io.micronaut.platform</groupId></parent>"),
            ["Micronaut"]
        );
        assert_eq!(
            found("<groupId>org.springframework.boot</groupId>"),
            ["Spring"]
        );
        assert!(is_build_file(Path::new("a/pom.xml")));
        assert!(is_build_file(Path::new("settings.gradle.kts")));
        assert!(!is_build_file(Path::new("docs/pom.xml.txt")));
        // The name in a Java file is no build marker.
        let m = model_of(&files(&[(
            "src/A.java",
            "package a;\n// built with io.quarkus\nimport jakarta.inject.Singleton;\n@Singleton class A {}\n",
        )]));
        assert!(!m.frameworks.iter().any(|f| f == "Quarkus"));
    }

    #[test]
    fn micronaut_factory_beans_and_property_keys() {
        let m = model_of(&files(&[
            (
                "src/Mailer.java",
                "package a;\nimport io.micronaut.context.annotation.Factory;\n@Factory\nclass Mailers {\n  @Singleton Mailer smtp() { return null; }\n  @Bean Mailer log() { return null; }\n}\n",
            ),
            (
                "src/MailerT.java",
                "package a;\npublic interface Mailer {}\n",
            ),
            (
                "src/Signup.java",
                "package a;\n@Controller(\"/signup\")\nclass Signup {\n  @Inject Mailer mailer;\n  @Property(name = \"mail.from\") String from;\n}\n",
            ),
            ("src/application.yml", "mail:\n  from: a@b\n"),
        ]));
        assert_eq!(m.label(), "Micronaut");
        let problems = messages(&m);
        assert_eq!(
            problems,
            ["2 beans of type Mailer (smtp, log); mark one @Primary or add @Named"]
        );
        let at = m.values[0].start;
        assert_eq!(m.targets_at(Path::new("src/Signup.java"), at).len(), 1);
    }

    /// The messages of the warnings: wiring and configuration mistakes.
    fn messages(m: &SpringModel) -> Vec<String> {
        m.problems()
            .into_iter()
            .filter(|p| p.severity == Level::Warning)
            .map(|p| p.message)
            .collect()
    }

    /// The weak warnings as `file:line` (1-based line) with the text their range covers.
    fn weak(m: &SpringModel, sources: &[(PathBuf, String)]) -> Vec<String> {
        m.problems()
            .into_iter()
            .filter(|p| p.severity == Level::Weak)
            .map(|p| {
                assert_eq!(p.message, FIELD_INJECTION);
                let text = &sources.iter().find(|(f, _)| *f == p.file).unwrap().1;
                format!(
                    "{}:{} {}",
                    p.file.display(),
                    p.line + 1,
                    &text[p.start..p.end]
                )
            })
            .collect()
    }

    // SPEC-0115/AK-10 (v4, behaviour 11).
    #[test]
    fn field_injection_is_a_weak_warning() {
        // Spring: every `@Autowired` field, also with a qualifier, a container type or a library
        // type; not constructors, setters, `@Value` fields, and not the test sources.
        let spring = files(&[
            (
                "src/main/java/a/Orders.java",
                "package a;\nimport org.springframework.beans.factory.annotation.Autowired;\n@Service\nclass Orders {\n  @Autowired\n  private Store store;\n  @Autowired @Qualifier(\"store\") private Store named;\n  @Autowired(required = false)\n  List<Store> all;\n  @Autowired JdbcClient jdbc;\n  @Value(\"${a.b:1}\") int n;\n  @Inject Store jsr;\n  private Store plain;\n  @Autowired\n  void setStore(Store s) {}\n}\n",
            ),
            (
                "src/main/java/a/Store.java",
                "package a;\n@Component\nclass Store {}\n",
            ),
            (
                "src/main/java/a/Ctor.java",
                "package a;\n@Service\nclass Ctor {\n  private final Store store;\n  @Autowired\n  Ctor(Store store) { this.store = store; }\n}\n",
            ),
            (
                "src/test/java/a/OrdersTest.java",
                "package a;\nimport org.springframework.beans.factory.annotation.Autowired;\n@SpringBootTest\nclass OrdersTest {\n  @Autowired\n  Orders orders;\n}\n",
            ),
        ]);
        let m = model_of(&spring);
        assert_eq!(m.label(), "Spring");
        assert!(messages(&m).is_empty(), "{:?}", m.problems());
        assert_eq!(
            weak(&m, &spring),
            [
                "src/main/java/a/Orders.java:5 @Autowired",
                "src/main/java/a/Orders.java:7 @Autowired",
                "src/main/java/a/Orders.java:8 @Autowired(required = false)",
                "src/main/java/a/Orders.java:10 @Autowired",
            ]
        );
        // Micronaut: its documentation prefers the constructor too; the annotation is `@Inject`.
        let micronaut = files(&[(
            "src/main/java/a/Signup.java",
            "package a;\nimport io.micronaut.http.annotation.Controller;\nimport jakarta.inject.Inject;\n@Controller(\"/signup\")\nclass Signup {\n  @Inject\n  Mailer mailer;\n  @Property(name = \"mail.from\") String from;\n}\n",
        )]);
        let m = model_of(&micronaut);
        assert_eq!(m.label(), "Micronaut");
        assert_eq!(
            weak(&m, &micronaut),
            ["src/main/java/a/Signup.java:6 @Inject"]
        );
        // Jakarta CDI and Quarkus document field injection as a usual form: no finding.
        for (label, import) in [
            ("Jakarta EE", "jakarta.enterprise.context.ApplicationScoped"),
            ("Quarkus", "io.quarkus.runtime.Startup"),
        ] {
            let cdi = files(&[(
                "src/main/java/a/Greeter.java",
                &format!(
                    "package a;\nimport {import};\nimport jakarta.inject.Inject;\n@ApplicationScoped\nclass Greeter {{\n  @Inject\n  Greeter other;\n  @Autowired Greeter spring;\n}}\n"
                ),
            )]);
            let m = model_of(&cdi);
            assert_eq!(m.label(), label);
            assert!(weak(&m, &cdi).is_empty(), "{label}: {:?}", m.problems());
        }
        // As JSON a problem names its severity.
        let json = serde_json::to_value(model_of(&micronaut).problems()).unwrap();
        assert_eq!(json[0]["severity"], "warning", "the undefined key");
        assert_eq!(json[1]["severity"], "weak");
    }

    fn kinds(m: &SpringModel) -> Vec<(&str, &str, &str)> {
        m.beans
            .iter()
            .map(|b| (b.name.as_str(), b.type_name.as_str(), b.kind.as_str()))
            .collect()
    }

    fn candidates<'a>(m: &'a SpringModel, file: &str, ty: &str) -> Vec<Vec<&'a str>> {
        m.injections
            .iter()
            .filter(|i| i.file.ends_with(file) && i.type_name == ty)
            .map(|i| {
                i.candidates
                    .iter()
                    .map(|&c| m.beans[c].name.as_str())
                    .collect()
            })
            .collect()
    }

    /// SPEC-0115/AK-7 (v3, behaviour 9).
    #[test]
    fn composed_stereotypes_and_bean_names() {
        let m = model_of(&files(&[
            (
                "UseCase.java",
                "package a;\nimport org.springframework.stereotype.Service;\n@Service\npublic @interface UseCase {}\n",
            ),
            ("Flow.java", "@UseCase\npublic @interface Flow {}\n"),
            (
                "Checkout.java",
                "@UseCase\nclass Checkout {\n  Checkout(Printer printer, @Qualifier(\"receiptPrinter\") Printer alias, Pool pool) {}\n}\n",
            ),
            ("Refund.java", "@Flow\nclass Refund {}\n"),
            ("Printer.java", "public class Printer {}\n"),
            ("Pool.java", "public class Pool {}\n"),
            (
                "Tools.java",
                "@Component(value = \"beans\")\nclass Tools {\n  @Bean(name = {\"receipts\", \"receiptPrinter\"}) Printer printer() { return null; }\n  @Bean(destroyMethod = \"shutdown\") Pool workers() { return null; }\n}\n",
            ),
        ]));
        let beans = kinds(&m);
        for expected in [
            ("checkout", "Checkout", "Service"),
            ("refund", "Refund", "Service"),
            ("beans", "Tools", "Component"),
            ("receipts", "Printer", "Bean"),
            ("workers", "Pool", "Bean"),
        ] {
            assert!(beans.contains(&expected), "{expected:?} in {beans:?}");
        }
        assert_eq!(beans.len(), 5, "{beans:?}");
        assert_eq!(
            candidates(&m, "Checkout.java", "Printer"),
            [["receipts"], ["receipts"]],
            "the alias finds the bean"
        );
        assert!(m.problems().is_empty(), "{:?}", m.problems());
    }

    /// SPEC-0115/AK-7 (v3, behaviour 9) and SPEC-0134/AK-8 (v3, behaviour 5).
    #[test]
    fn framework_provided_beans() {
        let m = model_of(&files(&[
            ("Order.java", "public class Order {}\n"),
            (
                "Orders.java",
                "import org.springframework.data.jpa.repository.JpaRepository;\npublic interface Orders extends JpaRepository<Order, Long> {}\n",
            ),
            (
                "Base.java",
                "@NoRepositoryBean\npublic interface Base<T> extends CrudRepository<T, Long> {}\n",
            ),
            (
                "Items.java",
                "public interface Items extends Base<Order> {}\n",
            ),
            ("Plain.java", "public interface Plain {}\n"),
            (
                "Notes.java",
                "@JdbcRepository(dialect = Dialect.H2)\npublic interface Notes {}\n",
            ),
            (
                "Settings.java",
                "@ConfigurationProperties(prefix = \"shop\")\npublic class Settings {\n  Settings(Plain bound) {}\n}\n",
            ),
            (
                "Mapping.java",
                "@ConfigMapping(prefix = \"greeting\")\npublic interface Mapping {}\n",
            ),
            (
                "Countries.java",
                "@RegisterRestClient(configKey = \"countries\")\npublic interface Countries {}\n",
            ),
            (
                "Spell.java",
                "@Client(\"https://spell.example\")\npublic interface Spell {}\n",
            ),
            (
                "Stock.java",
                "@FeignClient(name = \"stock\")\npublic interface Stock {}\n",
            ),
            (
                "User.java",
                "@Service\nclass User {\n  User(Orders a, Items b, Notes c, Settings d, Mapping e, Countries f, Spell g, Stock h, Base<Order> i, Plain j, Order[] all, boolean flag) {}\n}\n",
            ),
        ]));
        let beans = kinds(&m);
        for expected in [
            ("orders", "Orders", "Repository"),
            ("items", "Items", "Repository"),
            ("notes", "Notes", "JdbcRepository"),
            ("settings", "Settings", "ConfigurationProperties"),
            ("mapping", "Mapping", "ConfigMapping"),
            ("countries", "Countries", "RegisterRestClient"),
            ("spell", "Spell", "Client"),
            ("stock", "Stock", "FeignClient"),
        ] {
            assert!(beans.contains(&expected), "{expected:?} in {beans:?}");
        }
        assert!(
            !beans
                .iter()
                .any(|(_, ty, _)| *ty == "Base" || *ty == "Plain"),
            "{beans:?}"
        );
        // An array and a primitive are no lookups; the bound constructor of the configuration
        // class is no injection point, so `Plain` is asked for once.
        assert!(
            !m.injections
                .iter()
                .any(|i| i.type_name == "Order" || i.type_name == "boolean")
        );
        assert_eq!(candidates(&m, "User.java", "Base"), [["items"]]);
        assert_eq!(messages(&m), ["No bean of type Plain found"]);
    }

    /// SPEC-0115/AK-7 (v3, behaviour 9).
    #[test]
    fn conditional_and_test_beans() {
        let config = "@Configuration\nclass Config {\n  @Bean @Profile(\"prod\") Tax flat() { return null; }\n  @Bean @Profile(\"!prod\") Tax none() { return null; }\n  @Bean Cache simple() { return null; }\n}\n";
        let sources = |config: &str| {
            model_of(&files(&[
                ("src/main/java/Tax.java", "public interface Tax {}\n"),
                ("src/main/java/Cache.java", "public interface Cache {}\n"),
                (
                    "src/main/java/Gateway.java",
                    "public interface Gateway {}\n",
                ),
                ("src/main/java/Config.java", config),
                (
                    "src/main/java/Defaults.java",
                    "@Configuration\nclass Defaults {\n  @Bean @ConditionalOnMissingBean Cache fallback() { return null; }\n}\n",
                ),
                (
                    "src/main/java/Real.java",
                    "@Component\nclass Real implements Gateway {}\n",
                ),
                (
                    "src/main/java/Shop.java",
                    "@Service\nclass Shop {\n  Shop(Tax tax, Cache cache, Gateway gateway) {}\n}\n",
                ),
                (
                    "src/test/java/Fakes.java",
                    "@TestConfiguration\nclass Fakes {\n  @Bean Gateway fake() { return null; }\n}\n",
                ),
                (
                    "src/test/java/Probe.java",
                    "@Component\nclass Probe {\n  Probe(Gateway g) {}\n}\n",
                ),
            ]))
        };
        let m = sources(config);
        assert_eq!(candidates(&m, "Shop.java", "Tax"), [["flat", "none"]]);
        assert_eq!(candidates(&m, "Shop.java", "Cache"), [["simple"]]);
        assert_eq!(candidates(&m, "Shop.java", "Gateway"), [["real"]]);
        // The test sees the application's bean and its own.
        assert_eq!(
            messages(&m),
            ["2 beans of type Gateway (real, fake); mark one @Primary or add @Qualifier"]
        );
        // One bean without a condition beside a conditional one: both can be there.
        let m = sources(&config.replace("@Bean @Profile(\"!prod\") Tax none", "@Bean Tax none"));
        assert!(
            messages(&m).contains(
                &"2 beans of type Tax (flat, none); mark one @Primary or add @Qualifier".to_owned()
            ),
            "{:?}",
            messages(&m)
        );
    }

    /// SPEC-0134/AK-8 (v3, behaviour 5).
    #[test]
    fn cdi_producers_and_own_qualifiers() {
        let express = (
            "Express.java",
            "import jakarta.inject.Qualifier;\n@Qualifier\npublic @interface Express {}\n",
        );
        let m = model_of(&files(&[
            express,
            ("Courier.java", "public interface Courier {}\n"),
            (
                "Standard.java",
                "import jakarta.enterprise.context.ApplicationScoped;\n@ApplicationScoped\nclass Standard implements Courier {}\n",
            ),
            (
                "Fast.java",
                "@Express @ApplicationScoped\nclass Fast implements Courier {}\n",
            ),
            ("Book.java", "public class Book {}\n"),
            ("Shelf.java", "public class Shelf {}\n"),
            ("Greeter.java", "public interface Greeter {}\n"),
            (
                "Quiet.java",
                "@DefaultBean @ApplicationScoped\nclass Quiet implements Greeter {}\n",
            ),
            (
                "Loud.java",
                "@ApplicationScoped\nclass Loud implements Greeter {}\n",
            ),
            (
                "Producers.java",
                "import jakarta.enterprise.inject.Produces;\n@ApplicationScoped\nclass Producers {\n  @Produces @Named(\"title\") String title = \"x\";\n  @Produces Shelf shelf() { return null; }\n}\n",
            ),
            (
                "Books.java",
                "import jakarta.ws.rs.Produces;\n@Path(\"/books\")\nclass Books {\n  @Inject @Express Courier fast;\n  @Inject Courier normal;\n  @Inject Shelf shelf;\n  @Inject Greeter greeter;\n  @GET @Produces(MediaType.APPLICATION_JSON) public Book get(Book template) { return null; }\n  @POST @Produces(\"application/json\") public Book add(Book book) { return book; }\n  @GET @Produces public Book any(Book book) { return book; }\n}\n",
            ),
        ]));
        assert_eq!(m.label(), "Jakarta EE");
        let beans = kinds(&m);
        assert!(beans.contains(&("title", "String", "Bean")), "{beans:?}");
        assert!(beans.contains(&("shelf", "Shelf", "Bean")), "{beans:?}");
        assert!(
            !beans.iter().any(|(_, ty, _)| *ty == "Book"),
            "media types declare no bean: {beans:?}"
        );
        assert!(
            !m.injections.iter().any(|i| i.type_name == "Book"),
            "the parameters of a resource method are no injection points"
        );
        assert_eq!(
            candidates(&m, "Books.java", "Courier"),
            [["fast"], ["standard"]],
            "the qualifier picks its bean; without one the @Default bean is taken"
        );
        assert_eq!(candidates(&m, "Books.java", "Greeter"), [["loud"]]);
        assert!(m.problems().is_empty(), "{:?}", m.problems());

        // Micronaut has no @Default rule: without a qualifier both beans fit.
        let m = model_of(&files(&[
            express,
            ("Courier.java", "public interface Courier {}\n"),
            (
                "Standard.java",
                "import io.micronaut.context.annotation.Prototype;\n@Singleton\nclass Standard implements Courier {}\n",
            ),
            (
                "Fast.java",
                "@Express @Singleton\nclass Fast implements Courier {}\n",
            ),
            (
                "User.java",
                "@Singleton\nclass User {\n  @Inject Courier any;\n  @Inject @Express Courier fast;\n}\n",
            ),
        ]));
        assert_eq!(m.label(), "Micronaut");
        assert_eq!(
            messages(&m),
            ["2 beans of type Courier (standard, fast); mark one @Primary or add @Named"]
        );
    }

    /// SPEC-0115/AK-8 (v3, behaviour 10).
    #[test]
    fn configuration_file_keys() {
        assert_eq!(
            properties_keys(
                "# c\nname = Corner\ngreeting : Welcome \\\n    to the shop\nplain value\n! other\nlast=1\\\\\nnext=2\n"
            ),
            [
                ("name".to_owned(), 1),
                ("greeting".to_owned(), 2),
                ("plain".to_owned(), 4),
                ("last".to_owned(), 6),
                ("next".to_owned(), 7),
            ]
        );
        let yaml = "shop:\n  currency: EUR   # main\n  payment:\n    providers:\n      - name: stripe\n        url: https://stripe.example\n      - name: paypal\n    tags:\n    - a\n    - b\n  banner: |\n    key: not a key\n  empty:\n---\nshop:\n  currency: USD\n\"%dev\":\n  debug: true\n";
        let keys = yaml_keys(yaml);
        let keys: Vec<(&str, usize)> = keys.iter().map(|(k, l)| (k.as_str(), *l)).collect();
        assert_eq!(
            keys,
            [
                ("shop.currency", 1),
                ("shop.payment.providers[0].name", 4),
                ("shop.payment.providers[0].url", 5),
                ("shop.payment.providers[1].name", 6),
                ("shop.payment.tags[0]", 8),
                ("shop.payment.tags[1]", 9),
                ("shop.banner", 10),
                ("shop.empty", 12),
                ("shop.currency", 15),
                ("%dev.debug", 17),
            ]
        );
        // A key at the indentation of the dashes is a sibling of the list's key.
        let keys = yaml_keys("providers:\n- name: a\n  url: b\nother: c\n");
        let keys: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["providers[0].name", "providers[0].url", "other"]);
    }

    /// SPEC-0115/AK-8 (v3, behaviour 10) and SPEC-0134/AK-8 (v3, behaviour 5).
    #[test]
    fn relaxed_and_external_keys() {
        let m = model_of(&files(&[
            (
                "src/S.java",
                "@Service\nclass S {\n  @Value(\"${shop.max-items}\") int a;\n  @Value(\"${shop.maxItems}\") int b;\n  @Value(\"${shop.item-price}\") int c;\n  @Value(\"${shop.itemPrice}\") int d;\n  @Value(\"${user.home}\") String e;\n  @Value(\"${random.uuid}\") String f;\n  @Value(\"${DATABASE_URL}\") String g;\n  @Value(\"${shop.debug}\") String h;\n  @Value(\"${user.service}\") String i;\n}\n",
            ),
            (
                "src/main/resources/application.yml",
                "shop:\n  maxItems: 20\n  item-price: 5\n",
            ),
            (
                "src/main/resources/application-dev.yml",
                "shop:\n  debug: true\n",
            ),
        ]));
        assert_eq!(
            messages(&m),
            [
                "Property 'shop.itemPrice' is not defined in the application configuration",
                "Property 'user.service' is not defined in the application configuration",
            ]
        );
        let kebab = m.values[0].start;
        assert_eq!(
            m.targets_at(Path::new("src/S.java"), kebab),
            [(PathBuf::from("src/main/resources/application.yml"), 1)],
            "Go to Bean on the kebab-case key opens the camel-case definition"
        );

        // MicroProfile Config: exact keys, `Optional` values, `%profile.` keys.
        let m = model_of(&files(&[
            (
                "src/G.java",
                "import org.eclipse.microprofile.config.inject.ConfigProperty;\n@ApplicationScoped\nclass G {\n  @ConfigProperty(name = \"greeting.name\") Optional<String> name;\n  @ConfigProperty(name = \"greeting.debug\") boolean debug;\n  @ConfigProperty(name = \"greeting.max-size\") int size;\n  G(@ConfigProperty(name = \"greeting.count\") OptionalInt count) {}\n}\n",
            ),
            (
                "src/main/resources/application.properties",
                "%dev.greeting.debug=true\ngreeting.maxSize=3\n",
            ),
        ]));
        assert_eq!(
            messages(&m),
            ["Property 'greeting.max-size' is not defined in the application configuration"]
        );

        // Micronaut normalises property names: the kebab-case key finds the camel-case one.
        let m = model_of(&files(&[
            (
                "src/N.java",
                "import io.micronaut.context.annotation.Property;\n@Singleton\nclass N {\n  @Property(name = \"notes.page-size\") int size;\n}\n",
            ),
            (
                "src/main/resources/application.yml",
                "notes:\n  pageSize: 25\n",
            ),
        ]));
        assert!(m.problems().is_empty(), "{:?}", m.problems());
    }

    /// SPEC-0115 AK-13 (v5): the values of `.properties` and YAML files as written, the place
    /// of each key, the documents of a YAML file and the profile of each definition.
    #[test]
    fn configuration_values() {
        let text = "# c\nshop.name = Corner Shop  \nshop.greeting : Welcome \\\n    to the shop\nport=8085\nempty=\nflag\n";
        let entries: Vec<(String, usize, Option<String>)> = properties_entries(text)
            .into_iter()
            .map(|e| (text[e.start..e.end].to_owned(), e.line, e.value))
            .collect();
        let row = |k: &str, l: usize, v: &str| (k.to_owned(), l, Some(v.to_owned()));
        assert_eq!(
            entries,
            [
                row("shop.name", 1, "Corner Shop"),
                row("shop.greeting", 2, "Welcome to the shop"),
                row("port", 4, "8085"),
                row("empty", 5, ""),
                row("flag", 6, ""),
            ]
        );
        let yaml = "shop:\n  currency: EUR # default\n  \"title\": 'My shop'\n  text: |\n    line\n  tags:\n    - a\n    - \"b\"\n---\nspring:\n  config:\n    activate:\n      on-profile: prod\nshop:\n  currency: USD\n";
        let entries: Vec<(String, String, Option<String>, usize)> = yaml_entries(yaml)
            .into_iter()
            .map(|e| (e.key, yaml[e.start..e.end].to_owned(), e.value, e.document))
            .collect();
        let row = |k: &str, w: &str, v: Option<&str>, d: usize| {
            (k.to_owned(), w.to_owned(), v.map(str::to_owned), d)
        };
        assert_eq!(
            entries,
            [
                row("shop.currency", "currency", Some("EUR"), 0),
                row("shop.title", "title", Some("My shop"), 0),
                // A block scalar has no value to show.
                row("shop.text", "text", None, 0),
                row("shop.tags[0]", "a", Some("a"), 0),
                row("shop.tags[1]", "\"b\"", Some("b"), 0),
                row(
                    "spring.config.activate.on-profile",
                    "on-profile",
                    Some("prod"),
                    1
                ),
                row("shop.currency", "currency", Some("USD"), 1),
            ]
        );
        // The profile of a definition: the document's, the file's, the key's; none by default.
        let m = model_of(&files(&[
            ("src/main/resources/application.yml", yaml),
            (
                "src/main/resources/application-dev.properties",
                "shop.banner=DEV\n",
            ),
            (
                "src/main/resources/application.properties",
                "%test.shop.banner=T\nshop.name=x\n",
            ),
        ]));
        let profiles: Vec<(&str, Option<&str>, usize)> = m
            .properties
            .iter()
            .filter(|p| !p.key.starts_with("shop.t") && !p.key.starts_with("spring"))
            .map(|p| (p.key.as_str(), p.profile.as_deref(), p.column))
            .collect();
        assert_eq!(
            profiles,
            [
                ("shop.currency", None, 2),
                ("shop.currency", Some("prod"), 2),
                ("shop.banner", Some("dev"), 0),
                ("%test.shop.banner", Some("test"), 0),
                ("shop.name", None, 0),
            ]
        );
        // The model never serializes a value.
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("EUR") && !json.contains("DEV"), "{json}");
    }
}
