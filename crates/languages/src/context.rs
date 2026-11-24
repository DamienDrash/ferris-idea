//! Where a place is in the code: its module, package, type and function, and what kind of usage
//! its line holds (`SPEC-0081` v2 behaviour 4, `SPEC-0293` v3 behaviour 14). The module comes from
//! the folders, the package, type and function from the language server's symbols of the file,
//! the usage kind from the line's text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::insight::{Location, Opened, Symbol};
use crate::lsp::LspError;

/// Files whose name marks the folder of a module.
const BUILD_FILES: [&str; 11] = [
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "CMakeLists.txt",
];

/// Symbol kinds that are types.
const TYPE_KINDS: [&str; 5] = ["class", "interface", "enum", "struct", "object"];

/// Symbol kinds that are functions.
const FUNCTION_KINDS: [&str; 3] = ["method", "function", "constructor"];

/// What is known about a place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PlaceContext {
    /// The module's folder name.
    pub module: String,
    /// The package (Java), namespaces, or the folder below the module.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The type around the place with its enclosing types, e.g. `Discounts.Half`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// The kind of that type (`class`, `interface`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_kind: Option<String>,
    /// The innermost function around the place, as the server names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// The server's name of the symbol starting at the place, e.g. `apply(int)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// The kind of that symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<String>,
}

/// What a usage does with the symbol, from its line (`SPEC-0293` v3 behaviour 14), or in
/// JavaScript and TypeScript from the syntax tree (v4 behaviour 14a). The order is the order of the
/// groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    /// An import or include line.
    Import,
    /// A call: `(` follows the name.
    Call,
    /// An assignment to it.
    Write,
    /// Its value is read (v4; syntax trees only).
    Read,
    /// It names a type: an annotation, `extends`, `implements` (v4; syntax trees only).
    Type,
    /// The type of a method's receiver, `func (r Rect) Area()` (v12, Go).
    Receiver,
    /// The type of a composite literal, `Rect{W: 2}`: a struct initialization (v12, Go).
    #[serde(rename = "struct_initialization")]
    StructInit,
    /// It declares something of the same name, such as an overload (v4; syntax trees only).
    Declaration,
    /// Anything else.
    Other,
    /// A mention in a comment, no code usage (v12 behaviour 30, Go).
    Comment,
}

impl UsageKind {
    /// The group row's text.
    pub fn label(self) -> &'static str {
        match self {
            UsageKind::Import => "Imports",
            UsageKind::Call => "Calls",
            UsageKind::Write => "Writes",
            UsageKind::Read => "Reads",
            UsageKind::Type => "Type usages",
            UsageKind::Receiver => "Receivers",
            UsageKind::StructInit => "Struct initializations",
            UsageKind::Declaration => "Declarations",
            UsageKind::Other => "Other usages",
            UsageKind::Comment => "Usages in comments",
        }
    }
}

/// The kind of the usage that starts at char `column` of `line`.
pub fn usage_kind(line: &str, column: usize) -> UsageKind {
    let t = line.trim_start();
    let import = [
        "import ",
        "#include",
        "# include",
        "use ",
        "require ",
        "require(",
        "require_relative ",
    ]
    .iter()
    .any(|p| t.starts_with(p))
        || (t.starts_with("from ") && t.contains(" import "));
    if import {
        return UsageKind::Import;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut i = column.min(chars.len());
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
        i += 1;
    }
    let rest: String = chars[i..].iter().collect();
    let rest = rest.trim_start();
    if rest.starts_with('(') {
        return UsageKind::Call;
    }
    if rest.starts_with("++") || rest.starts_with("--") {
        return UsageKind::Write;
    }
    let compound = [
        "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", ">>>=", "??=", ":=",
    ];
    if compound.iter().any(|op| rest.starts_with(op)) {
        return UsageKind::Write;
    }
    if rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>") {
        return UsageKind::Write;
    }
    // `++x` / `--x` before the name.
    let before: String = chars[..column.min(chars.len())].iter().collect();
    if before.trim_end().ends_with("++") || before.trim_end().ends_with("--") {
        return UsageKind::Write;
    }
    UsageKind::Other
}

/// The folder name of the module `path` belongs to: the nearest folder up to `root` holding a
/// build file, else `root` itself.
pub fn module_of(root: &Path, path: &Path) -> String {
    module_dir(root, path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folder of the module `path` belongs to: the nearest folder up to `root` holding a build
/// file, else `root` itself (`SPEC-0334` behaviour 16).
pub fn module_folder(root: &Path, path: &Path) -> PathBuf {
    module_dir(root, path)
}

fn module_dir(root: &Path, path: &Path) -> PathBuf {
    // Paths from a server are canonical; the project root may be given through a link.
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root = if path.starts_with(&canonical) {
        canonical.as_path()
    } else {
        root
    };
    let mut dir = path.parent();
    while let Some(d) = dir {
        if !d.starts_with(root) {
            break;
        }
        if BUILD_FILES.iter().any(|f| d.join(f).is_file()) {
            return d.to_path_buf();
        }
        if d == root {
            break;
        }
        dir = d.parent();
    }
    root.to_path_buf()
}

/// The symbols of `symbols` around 0-based `line`/`character`, outermost first.
fn chain(symbols: &[Symbol], line: u64, character: u64) -> Vec<&Symbol> {
    let mut out: Vec<&Symbol> = Vec::new();
    for s in symbols {
        // Only symbols nested in the last one found (or top level) can follow.
        if s.depth != out.len() {
            continue;
        }
        if s.contains(line, character) {
            out.push(s);
        }
    }
    out
}

/// What is known about `line`/`character` of `path` from the file's `symbols` (none when the
/// server gave none).
pub fn context_of(
    root: &Path,
    path: &Path,
    symbols: Option<&[Symbol]>,
    line: u64,
    character: u64,
) -> PlaceContext {
    let mut c = PlaceContext {
        module: module_of(root, path),
        ..PlaceContext::default()
    };
    let symbols = symbols.unwrap_or_default();
    let around = chain(symbols, line, character);
    let types: Vec<&Symbol> = around
        .iter()
        .copied()
        .filter(|s| TYPE_KINDS.contains(&s.kind.as_str()) && !is_anonymous(&s.name))
        .collect();
    if let Some(last) = types.last() {
        c.container = Some(
            types
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join("."),
        );
        c.container_kind = Some(last.kind.clone());
    }
    c.method = around
        .iter()
        .rev()
        .find(|s| FUNCTION_KINDS.contains(&s.kind.as_str()))
        .map(|s| s.name.clone());
    c.symbol = around
        .last()
        .filter(|s| s.line == line && s.character == character)
        .map(|s| s.name.clone());
    c.symbol_kind = around
        .last()
        .filter(|s| s.line == line && s.character == character)
        .map(|s| s.kind.clone());
    let namespaces: Vec<&str> = around
        .iter()
        .filter(|s| s.kind == "namespace")
        .map(|s| s.name.as_str())
        .collect();
    c.package = symbols
        .iter()
        .find(|s| s.kind == "package")
        .map(|s| s.name.clone())
        .or_else(|| (!namespaces.is_empty()).then(|| namespaces.join("::")))
        .or_else(|| {
            let dir = path.parent()?;
            let rel = dir.strip_prefix(module_dir(root, path)).ok()?;
            let rel = rel.to_string_lossy().replace('\\', "/");
            (!rel.is_empty()).then_some(rel)
        });
    c
}

/// Whether a symbol's name is that of an anonymous class (`new Runnable() {...}`).
pub fn is_anonymous(name: &str) -> bool {
    name.starts_with("new ") || name.contains("{...}") || name.starts_with("<anonymous")
}

/// The symbols of each distinct file of `places` (at most `limit` files), asked through `o` until
/// `deadline`; files whose symbols do not arrive are left out.
pub(crate) async fn symbols_by_file(
    o: &mut Opened,
    places: &[Location],
    deadline: Instant,
    limit: usize,
) -> HashMap<PathBuf, Vec<Symbol>> {
    let mut out: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();
    let mut files: Vec<&PathBuf> = places.iter().map(|l| &l.path).collect();
    files.sort();
    files.dedup();
    for file in files.into_iter().take(limit) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        if let Ok(symbols) = o.symbols_of(file, left).await {
            out.insert(file.clone(), symbols);
        }
    }
    out
}

/// The context of each of `places`, with the symbols of their files asked of the server of `file`
/// (whose content is `text`, else the saved file) within `wait` (`SPEC-0081` v2 behaviour 4).
pub async fn contexts_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    places: &[Location],
    wait: Duration,
) -> Result<Vec<PlaceContext>, LspError> {
    let started = Instant::now();
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let symbols = symbols_by_file(&mut o, places, started + wait, 100).await;
    o.close().await;
    Ok(places
        .iter()
        .map(|l| {
            let mut c = context_of(
                root,
                &l.path,
                symbols.get(&l.path).map(Vec::as_slice),
                l.range.start_line,
                l.range.start_character,
            );
            if c.container.is_none() {
                java_owner(&mut c, l);
            }
            c
        })
        .collect())
}

/// A Java place whose symbols did not arrive in time gets its type from the syntax tree, so the
/// rows of one chooser look alike (OBS-0027, `SPEC-0081` v3 behaviour 4a).
fn java_owner(c: &mut PlaceContext, l: &Location) {
    if l.path.extension().and_then(|e| e.to_str()) != Some("java") {
        return;
    }
    let Ok(text) = std::fs::read_to_string(&l.path) else {
        return;
    };
    let at = crate::java_structure::offset_of(
        &text,
        l.range.start_line as usize,
        l.range.start_character as usize,
    );
    let Some(t) = crate::java_structure::type_at(&text, at) else {
        return;
    };
    c.container = Some(t.name);
    c.container_kind = Some(
        match t.kind {
            crate::java_structure::TypeKind::Interface => "interface",
            crate::java_structure::TypeKind::Enum => "enum",
            _ => "class",
        }
        .into(),
    );
    if c.package.is_none() {
        c.package = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("package "))
            .map(|p| p.trim_end_matches(';').trim().to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, kind: &str, depth: usize, lines: (u64, u64)) -> Symbol {
        Symbol {
            name: name.into(),
            kind: kind.into(),
            line: lines.0,
            character: 4,
            depth,
            end_line: lines.1,
            end_character: 5,
            ..Symbol::default()
        }
    }

    /// OBS-0027: a Java place without symbols gets its type and package from the syntax tree.
    #[test]
    fn java_owner_from_the_syntax_tree() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Dog.java");
        std::fs::write(
            &path,
            "package shop;\n\nclass Dog {\n    String sound() { return \"w\"; }\n}\n",
        )
        .unwrap();
        let l = Location {
            path: path.clone(),
            range: crate::insight::Range {
                start_line: 3,
                start_character: 11,
                end_line: 3,
                end_character: 16,
            },
        };
        let mut c = PlaceContext::default();
        java_owner(&mut c, &l);
        assert_eq!(c.container.as_deref(), Some("Dog"));
        assert_eq!(c.container_kind.as_deref(), Some("class"));
        assert_eq!(c.package.as_deref(), Some("shop"));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn discounts() -> Vec<Symbol> {
        vec![
            sym("lab.inventory", "package", 0, (0, 0)),
            sym("Discounts", "class", 0, (2, 23)),
            sym("None", "class", 1, (3, 7)),
            sym("apply(int)", "method", 2, (4, 6)),
            sym("Half", "class", 1, (9, 13)),
            sym("apply(int)", "method", 2, (10, 12)),
            sym("priceOf(Item, Discount)", "method", 1, (15, 17)),
            sym("new Runnable() {...}", "class", 2, (16, 16)),
        ]
    }

    /// SPEC-0081 v2 AK-4.
    #[test]
    fn the_innermost_type_names_the_place() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-ctx-{}", std::process::id()));
        let file = dir.join("src/lab/inventory/Discounts.java");
        let s = discounts();
        let c = context_of(&dir, &file, Some(&s), 10, 4);
        assert_eq!(c.container.as_deref(), Some("Discounts.Half"));
        assert_eq!(c.container_kind.as_deref(), Some("class"));
        assert_eq!(c.method.as_deref(), Some("apply(int)"));
        assert_eq!(c.symbol.as_deref(), Some("apply(int)"));
        assert_eq!(c.package.as_deref(), Some("lab.inventory"));
        // A call inside priceOf; the anonymous class is not a container.
        let c = context_of(&dir, &file, Some(&s), 16, 20);
        assert_eq!(c.container.as_deref(), Some("Discounts"));
        assert_eq!(c.method.as_deref(), Some("priceOf(Item, Discount)"));
        assert_eq!(c.symbol, None);
        // Without symbols the folder below the module is the package.
        let c = context_of(&dir, &file, None, 10, 4);
        assert_eq!(c.container, None);
        assert_eq!(c.package.as_deref(), Some("src/lab/inventory"));
    }

    /// SPEC-0081 v2 AK-4.
    #[test]
    fn the_module_is_the_nearest_build_folder() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-mod-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("shop/src/main/java/a")).unwrap();
        std::fs::write(dir.join("shop/pom.xml"), "<project/>").unwrap();
        let file = dir.join("shop/src/main/java/a/A.java");
        assert_eq!(module_of(&dir, &file), "shop");
        let loose = dir.join("other/B.java");
        assert_eq!(
            module_of(&dir, &loose),
            dir.file_name().unwrap().to_string_lossy()
        );
        let c = context_of(&dir, &file, None, 0, 0);
        assert_eq!(c.package.as_deref(), Some("src/main/java/a"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0293 v3 AK-8.
    #[test]
    fn usage_kinds_from_the_line() {
        assert_eq!(
            usage_kind("import lab.inventory.Discount;", 21),
            UsageKind::Import
        );
        assert_eq!(usage_kind("#include \"helper.h\"", 10), UsageKind::Import);
        assert_eq!(usage_kind("  return rule.apply(x);", 14), UsageKind::Call);
        assert_eq!(usage_kind("  int a = helper ();", 10), UsageKind::Call);
        assert_eq!(usage_kind("  count = 3;", 2), UsageKind::Write);
        assert_eq!(usage_kind("  count += 3;", 2), UsageKind::Write);
        assert_eq!(usage_kind("  count++;", 2), UsageKind::Write);
        assert_eq!(usage_kind("  ++count;", 4), UsageKind::Write);
        assert_eq!(usage_kind("  if (count == 3)", 6), UsageKind::Other);
        assert_eq!(usage_kind("  Discount rule = x;", 2), UsageKind::Other);
        assert_eq!(UsageKind::Call.label(), "Calls");
    }
}
