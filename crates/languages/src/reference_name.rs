//! What Copy Reference copies (`SPEC-0332` v2 behaviours 13a to 13c): the qualified name of the
//! symbol around a place, from the language server's document symbols of the file, and the
//! file's reference name with a line where there is no such symbol. The forms of the names for
//! Java, Kotlin and Python and the reference name of a file follow the open platform's
//! copy-reference code (intellij-community, `docs/legal/PROVENANCE.md`: logic template); the
//! code is our own, and so is the form for Rust.

use std::path::{Path, PathBuf};

use crate::LanguageId;
use crate::insight::Symbol;

/// Symbol kinds that are types.
const TYPE_KINDS: [&str; 5] = ["class", "interface", "enum", "struct", "object"];

/// Symbol kinds that are functions.
const FUNCTION_KINDS: [&str; 3] = ["method", "function", "constructor"];

/// Symbol kinds that are members holding a value.
const VALUE_KINDS: [&str; 4] = ["field", "property", "constant", "enum-member"];

/// Symbol kinds that are modules.
const MODULE_KINDS: [&str; 2] = ["module", "namespace"];

/// How a language writes qualified names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// `package.Type#member`
    Jvm,
    /// `module.Class.function`
    Python,
    /// `crate::module::Type::item`
    Rust,
}

fn form(language: Option<LanguageId>) -> Option<Form> {
    match language? {
        LanguageId::Java | LanguageId::Kotlin => Some(Form::Jvm),
        LanguageId::Python => Some(Form::Python),
        LanguageId::Rust => Some(Form::Rust),
        _ => None,
    }
}

/// Whether symbols of `language` have qualified names here; the others copy file and line.
pub fn has_rules(language: Option<LanguageId>) -> bool {
    form(language).is_some()
}

fn is_type(s: &Symbol) -> bool {
    TYPE_KINDS.contains(&s.kind.as_str())
}

fn is_function(s: &Symbol) -> bool {
    FUNCTION_KINDS.contains(&s.kind.as_str())
}

fn is_value(s: &Symbol) -> bool {
    VALUE_KINDS.contains(&s.kind.as_str())
}

fn is_module(s: &Symbol) -> bool {
    MODULE_KINDS.contains(&s.kind.as_str())
}

/// A class without a name of its own: an anonymous class, an object expression.
fn is_anonymous(s: &Symbol) -> bool {
    let name = s.name.trim();
    name.is_empty() || name.starts_with('<') || crate::context::is_anonymous(name)
}

/// The server's name of a symbol without a parameter list at its end.
fn base(name: &str) -> &str {
    name.find('(').map_or(name, |i| &name[..i]).trim()
}

/// The parameter list at the end of the server's name of a function, with its brackets.
fn parameters(name: &str) -> Option<&str> {
    let name = name.trim_end();
    let open = name.find('(')?;
    name.ends_with(')').then(|| &name[open..])
}

/// The symbols directly below `parent` (the top level for `None`) in the flattened list.
fn children(symbols: &[Symbol], parent: Option<usize>) -> Vec<usize> {
    let (start, depth) = match parent {
        Some(p) => (p + 1, symbols[p].depth + 1),
        None => (0, 0),
    };
    let mut out = Vec::new();
    for (i, s) in symbols.iter().enumerate().skip(start) {
        if s.depth < depth {
            break;
        }
        if s.depth == depth {
            out.push(i);
        }
    }
    out
}

/// The symbols around 0-based `line`/`character`, outermost first. Of several symbols of one
/// level that hold the place (declarations sharing a line) the last one whose name starts at or
/// before it counts, else the first. A symbol on the line of its parent's name holds the place
/// only from its own name on: before it the place belongs to the parent.
fn around(symbols: &[Symbol], line: u64, character: u64) -> Vec<usize> {
    let mut out = Vec::new();
    let mut parent: Option<usize> = None;
    loop {
        let holding: Vec<usize> = children(symbols, parent)
            .into_iter()
            .filter(|&i| {
                let s = &symbols[i];
                s.contains(line, character)
                    && (parent.is_none_or(|p| symbols[p].line != s.line)
                        || (s.line, s.character) <= (line, character))
            })
            .collect();
        let pick = holding
            .iter()
            .rev()
            .find(|&&i| (symbols[i].line, symbols[i].character) <= (line, character))
            .or(holding.first());
        let Some(&i) = pick else {
            return out;
        };
        out.push(i);
        parent = Some(i);
    }
}

/// Whether a symbol can be the one Copy Reference names; the others are passed over for the
/// symbol around them.
fn nameable(form: Form, s: &Symbol) -> bool {
    if crate::structure::is_lambda(&s.name) {
        return false;
    }
    match form {
        Form::Jvm => is_type(s) || is_function(s) || is_value(s),
        Form::Python => s.kind == "class" || is_function(s),
        // rust-analyzer lists a type alias as a type parameter; generic parameters it does not
        // list.
        Form::Rust => {
            is_type(s)
                || is_function(s)
                || is_value(s)
                || is_module(s)
                || s.kind == "type-parameter"
        }
    }
}

/// The type an `impl` block of Rust is for, as rust-analyzer names the block (`impl Cart`,
/// `impl<T> Display for Cart<T>`): the last name of the type's path, without generic arguments.
fn impl_type(name: &str) -> String {
    let mut rest = name.trim();
    rest = rest.strip_prefix("unsafe ").unwrap_or(rest).trim_start();
    if let Some(after) = rest.strip_prefix("impl") {
        rest = after.trim_start();
        // The generic parameters of the block itself.
        if rest.starts_with('<') {
            let mut depth = 0usize;
            let mut end = rest.len();
            for (i, c) in rest.char_indices() {
                match c {
                    '<' => depth += 1,
                    '>' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            rest = rest[end..].trim_start();
        }
        // `Trait for Type`: the type follows the last ` for ` outside angle brackets.
        let mut depth = 0usize;
        let mut after_for = None;
        for (i, c) in rest.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => depth = depth.saturating_sub(1),
                ' ' if depth == 0 && rest[i..].starts_with(" for ") => after_for = Some(i + 5),
                _ => {}
            }
        }
        if let Some(i) = after_for {
            rest = &rest[i..];
        }
    }
    let mut rest = rest.trim().trim_start_matches('&').trim_start();
    // A lifetime of a reference: `&'a mut T`.
    if rest.starts_with('\'') {
        rest = rest
            .split_once(' ')
            .map_or("", |(_, after)| after)
            .trim_start();
    }
    for prefix in ["mut ", "dyn "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest).trim_start();
    }
    let end = rest.find(['<', ' ', '{']).unwrap_or(rest.len());
    let path = &rest[..end];
    path.rsplit("::").next().unwrap_or(path).to_owned()
}

/// The name of a container or target in a Rust path.
fn rust_name(s: &Symbol) -> String {
    if s.kind == "object" || s.name.trim_start().starts_with("impl ") {
        impl_type(&s.name)
    } else {
        let name = base(&s.name);
        name.find('<')
            .map_or(name, |i| &name[..i])
            .trim()
            .to_owned()
    }
}

/// The qualified name of the symbol around 0-based `line`/`character` (`SPEC-0332` v2 behaviour
/// 13c), from the file's `text` and its `symbols` as the language server lists them. `module` is
/// the name the file's symbols start with: a Python module (`shop.cart`, [`python_module`]) or
/// the Rust modules of the file (`crate::cart`, [`rust_module`]); Java and Kotlin take the
/// package from the text. `None` where no symbol can be named: Copy Reference then copies file
/// and line.
pub fn qualified_name(
    language: Option<LanguageId>,
    module: Option<&str>,
    text: &str,
    symbols: &[Symbol],
    line: u64,
    character: u64,
) -> Option<String> {
    let form = form(language)?;
    // jdtls lists the `package` line as a symbol.
    let symbols = crate::structure::without_packages(symbols.to_vec());
    let mut chain = around(&symbols, line, character);
    while let Some(&last) = chain.last() {
        if nameable(form, &symbols[last]) {
            break;
        }
        chain.pop();
    }
    let (&target, containers) = chain.split_last()?;
    let target = &symbols[target];
    let parent = containers.last().copied();
    let containers: Vec<&Symbol> = containers.iter().map(|&i| &symbols[i]).collect();
    if is_anonymous(target) || base(&target.name).is_empty() {
        return None;
    }
    // Inside a function or an anonymous class nothing has a qualified name; Java and Kotlin
    // refer to a member of such a class by its simple name.
    if containers.iter().any(|c| is_function(c) || is_anonymous(c)) {
        let member = !is_type(target) && containers.last().is_some_and(|c| is_type(c));
        return (form == Form::Jvm && member).then(|| base(&target.name).to_owned());
    }
    match form {
        Form::Jvm => {
            if !containers.iter().all(|c| is_type(c)) {
                return None;
            }
            let mut types: Vec<&str> = Vec::new();
            let package = ferris_idea_core::source_roots::declared_package(text);
            if let Some(p) = package.as_deref() {
                types.push(p);
            }
            types.extend(containers.iter().map(|c| base(&c.name)));
            let name = base(&target.name);
            if is_type(target) || containers.is_empty() {
                types.push(name);
                return Some(types.join("."));
            }
            // A function that shares its name with another member of its type gets the
            // parameter list the server names it with.
            let overloaded = is_function(target)
                && children(&symbols, parent)
                    .into_iter()
                    .filter(|&i| is_function(&symbols[i]) && base(&symbols[i].name) == name)
                    .count()
                    > 1;
            let list = overloaded
                .then(|| parameters(&target.name))
                .flatten()
                .unwrap_or_default();
            Some(format!("{}#{name}{list}", types.join(".")))
        }
        Form::Python => {
            if !containers.iter().all(|c| c.kind == "class") {
                return None;
            }
            let mut parts: Vec<&str> = module.into_iter().collect();
            parts.extend(containers.iter().map(|c| base(&c.name)));
            parts.push(base(&target.name));
            Some(parts.join("."))
        }
        Form::Rust => {
            if !containers.iter().all(|c| is_type(c) || is_module(c)) {
                return None;
            }
            let mut parts: Vec<String> = module.map(str::to_owned).into_iter().collect();
            parts.extend(containers.iter().map(|c| rust_name(c)));
            parts.push(rust_name(target));
            if parts.iter().any(String::is_empty) {
                return None;
            }
            Some(parts.join("::"))
        }
    }
}

/// `path` below `base` with `/` between the names; `None` outside it and for `base` itself. The
/// two may differ by a symbolic link.
pub fn below(path: &Path, base: &Path) -> Option<String> {
    let direct = |path: &Path, base: &Path| {
        let rel = path.strip_prefix(base).ok()?;
        let text = rel.to_string_lossy().replace('\\', "/");
        (!text.is_empty()).then_some(text)
    };
    direct(path, base).or_else(|| {
        let (path, base) = (path.canonicalize().ok()?, base.canonicalize().ok()?);
        direct(&path, &base)
    })
}

/// A file's reference name (`SPEC-0332` v2 behaviour 13a): its path below the innermost of the
/// source folders `source_roots` that holds it, else below the project folder `root`, else its
/// absolute path.
pub fn file_name(path: &Path, root: &Path, source_roots: &[PathBuf]) -> String {
    ferris_idea_core::source_roots::containing(source_roots, path)
        .and_then(|source| below(path, source))
        .or_else(|| below(path, root))
        .unwrap_or_else(|| path.display().to_string())
}

/// A reference name with the 1-based number of 0-based `line`.
pub fn with_line(name: &str, line: usize) -> String {
    format!("{name}:{}", line + 1)
}

/// The Python module of a file by its path below a source folder or the project folder:
/// `shop/cart.py` is `shop.cart`, `shop/__init__.py` is `shop`. `None` when nothing is left.
pub fn python_module(relative: &str) -> Option<String> {
    let mut names: Vec<&str> = relative.split('/').filter(|n| !n.is_empty()).collect();
    let last = names.pop()?;
    let stem = last
        .strip_suffix(".pyi")
        .or_else(|| last.strip_suffix(".py"))
        .unwrap_or(last);
    if stem != "__init__" {
        names.push(stem);
    }
    (!names.is_empty()).then(|| names.join("."))
}

/// The modules of a Rust file by its path below the `src` folder of its package: `cart/line.rs`
/// is `crate::cart::line`; `lib.rs`, `main.rs` and `mod.rs` name no module of their own, and a
/// file directly in `bin` is a crate root.
pub fn rust_module_below_src(relative: &str) -> String {
    let mut names: Vec<&str> = relative.split('/').filter(|n| !n.is_empty()).collect();
    let mut out = vec!["crate"];
    if names.len() == 2 && names[0] == "bin" {
        return out.join("::");
    }
    if let Some(last) = names.pop() {
        let stem = last.strip_suffix(".rs").unwrap_or(last);
        if !matches!(stem, "lib" | "main" | "mod") {
            names.push(stem);
        }
    }
    out.extend(names);
    out.join("::")
}

/// The modules of the Rust file `path` in the project at `root`: its place below the `src`
/// folder next to the nearest `Cargo.toml` at or below `root`. `None` for a file outside such a
/// folder (tests, examples, benches, a build script).
pub fn rust_module(root: &Path, path: &Path) -> Option<String> {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.join("Cargo.toml").is_file() {
            return below(path, &d.join("src")).map(|rel| rust_module_below_src(&rel));
        }
        if d == root || !d.starts_with(root) {
            break;
        }
        dir = d.parent();
    }
    None
}

/// The name the symbols of `path` start with, for [`qualified_name`]: the Python module below
/// the file's source folder or the project folder, the Rust modules of the file; none for the
/// other languages.
pub fn module_of(
    language: Option<LanguageId>,
    root: &Path,
    path: &Path,
    source_roots: &[PathBuf],
) -> Option<String> {
    match language? {
        LanguageId::Python => python_module(
            &ferris_idea_core::source_roots::containing(source_roots, path)
                .and_then(|source| below(path, source))
                .or_else(|| below(path, root))?,
        ),
        LanguageId::Rust => rust_module(root, path),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A symbol named `name` of `kind` at `depth`, from the start of its `line` (its name at
    /// column `character`) to the end of line `end`.
    fn sym(name: &str, kind: &str, depth: usize, line: u64, character: u64, end: u64) -> Symbol {
        Symbol {
            name: name.into(),
            kind: kind.into(),
            depth,
            line,
            character,
            end_line: end,
            end_character: 200,
            ..Symbol::default()
        }
    }

    fn java(text: &str, symbols: &[Symbol], line: u64, character: u64) -> Option<String> {
        qualified_name(Some(LanguageId::Java), None, text, symbols, line, character)
    }

    /// `Cart` as jdtls lists it: the package line, the class, its members with their parameter
    /// lists, a nested class.
    fn cart() -> Vec<Symbol> {
        vec![
            sym("shop", "package", 0, 0, 8, 0),
            sym("Cart", "class", 0, 2, 13, 30),
            sym("items", "field", 1, 3, 8, 3),
            sym("Cart()", "constructor", 1, 5, 4, 7),
            sym("total(int[])", "method", 1, 9, 8, 14),
            sym("add(int)", "method", 1, 16, 9, 18),
            sym("add(int, String)", "method", 1, 20, 9, 22),
            sym("Line", "class", 1, 24, 17, 29),
            sym("price", "field", 2, 25, 12, 25),
            sym("net()", "method", 2, 27, 12, 28),
        ]
    }

    const PACKAGE: &str = "package shop;\n\npublic class Cart {\n}\n";

    #[test]
    fn java_types_and_members() {
        let s = cart();
        assert_eq!(java(PACKAGE, &s, 2, 14).as_deref(), Some("shop.Cart"));
        // Before the name on its line: modifiers count.
        assert_eq!(java(PACKAGE, &s, 2, 0).as_deref(), Some("shop.Cart"));
        assert_eq!(java(PACKAGE, &s, 3, 9).as_deref(), Some("shop.Cart#items"));
        assert_eq!(java(PACKAGE, &s, 6, 2).as_deref(), Some("shop.Cart#Cart"));
        // Inside the body of a method, not only on its name.
        assert_eq!(
            java(PACKAGE, &s, 12, 20).as_deref(),
            Some("shop.Cart#total")
        );
        assert_eq!(java(PACKAGE, &s, 24, 20).as_deref(), Some("shop.Cart.Line"));
        assert_eq!(
            java(PACKAGE, &s, 25, 12).as_deref(),
            Some("shop.Cart.Line#price")
        );
        assert_eq!(
            java(PACKAGE, &s, 27, 13).as_deref(),
            Some("shop.Cart.Line#net")
        );
        // Between two members: the class.
        assert_eq!(java(PACKAGE, &s, 15, 0).as_deref(), Some("shop.Cart"));
        // An enum's constants on one line: the one the caret is on.
        let colours = [
            sym("Colour", "enum", 0, 0, 5, 3),
            sym("RED", "enum-member", 1, 1, 4, 1),
            sym("GREEN", "enum-member", 1, 1, 9, 1),
            sym("BLUE", "enum-member", 1, 1, 16, 1),
        ];
        assert_eq!(java("", &colours, 1, 11).as_deref(), Some("Colour#GREEN"));
        assert_eq!(java("", &colours, 1, 20).as_deref(), Some("Colour#BLUE"));
        assert_eq!(java("", &colours, 1, 0).as_deref(), Some("Colour#RED"));
    }

    #[test]
    fn java_overloads_take_the_servers_parameter_list() {
        let s = cart();
        assert_eq!(
            java(PACKAGE, &s, 17, 0).as_deref(),
            Some("shop.Cart#add(int)")
        );
        assert_eq!(
            java(PACKAGE, &s, 20, 9).as_deref(),
            Some("shop.Cart#add(int, String)")
        );
        // One method of that name: no list.
        assert_eq!(java(PACKAGE, &s, 9, 8).as_deref(), Some("shop.Cart#total"));
        // A server that names functions without a list adds none.
        let plain = [
            sym("Cart", "class", 0, 0, 6, 9),
            sym("add", "method", 1, 1, 4, 2),
            sym("add", "method", 1, 4, 4, 5),
        ];
        assert_eq!(java("", &plain, 4, 4).as_deref(), Some("Cart#add"));
        // A field named like a method is no overload.
        let mixed = [
            sym("Cart", "class", 0, 0, 6, 9),
            sym("size", "field", 1, 1, 4, 1),
            sym("size()", "method", 1, 3, 4, 5),
        ];
        assert_eq!(java("", &mixed, 4, 0).as_deref(), Some("Cart#size"));
    }

    #[test]
    fn java_anonymous_and_local_classes() {
        let s = [
            sym("Jobs", "class", 0, 2, 6, 30),
            sym("start()", "method", 1, 3, 9, 20),
            sym("new Runnable() {...}", "class", 2, 4, 25, 9),
            sym("run()", "method", 3, 6, 24, 8),
            sym("Local", "class", 2, 11, 14, 16),
            sym("count", "field", 3, 12, 16, 12),
            sym("Inner", "class", 3, 13, 18, 15),
            sym("later", "field", 1, 22, 13, 27),
            sym("new Runnable() {...}", "class", 2, 22, 25, 27),
            sym("run()", "method", 3, 24, 20, 26),
        ];
        let text = "package jobs;\n";
        // A member of an anonymous class: its simple name.
        assert_eq!(java(text, &s, 7, 10).as_deref(), Some("run"));
        assert_eq!(java(text, &s, 25, 10).as_deref(), Some("run"));
        // The anonymous class itself and a class inside a method have no name.
        assert_eq!(java(text, &s, 5, 0), None);
        assert_eq!(java(text, &s, 11, 14), None);
        assert_eq!(java(text, &s, 13, 18), None);
        // A member of a class inside a method: its simple name.
        assert_eq!(java(text, &s, 12, 16).as_deref(), Some("count"));
        // The method around them keeps its name.
        assert_eq!(java(text, &s, 18, 0).as_deref(), Some("jobs.Jobs#start"));
        // The field an anonymous class is assigned to.
        assert_eq!(java(text, &s, 22, 13).as_deref(), Some("jobs.Jobs#later"));
    }

    #[test]
    fn java_file_without_a_package() {
        let s = [
            sym("Main", "class", 0, 0, 13, 5),
            sym("main(String[])", "method", 1, 1, 23, 3),
        ];
        let text = "public class Main {\n}\n";
        assert_eq!(java(text, &s, 0, 13).as_deref(), Some("Main"));
        assert_eq!(java(text, &s, 2, 0).as_deref(), Some("Main#main"));
    }

    #[test]
    fn kotlin_top_level_and_members() {
        let kotlin = |s: &[Symbol], line| {
            qualified_name(
                Some(LanguageId::Kotlin),
                None,
                "package shop\n\nfun total() = 0\n",
                s,
                line,
                4,
            )
        };
        let s = [
            sym("total", "function", 0, 2, 4, 2),
            sym("limit", "property", 0, 3, 4, 3),
            sym("Cart", "class", 0, 5, 6, 12),
            sym("size", "property", 1, 6, 8, 6),
            sym("add(Int)", "function", 1, 7, 8, 8),
            sym("add(Int, String)", "function", 1, 9, 8, 10),
            sym("Companion", "object", 1, 11, 21, 12),
            sym("create", "function", 2, 12, 12, 12),
        ];
        assert_eq!(kotlin(&s, 2).as_deref(), Some("shop.total"));
        assert_eq!(kotlin(&s, 3).as_deref(), Some("shop.limit"));
        assert_eq!(kotlin(&s, 5).as_deref(), Some("shop.Cart"));
        assert_eq!(kotlin(&s, 6).as_deref(), Some("shop.Cart#size"));
        assert_eq!(kotlin(&s, 9).as_deref(), Some("shop.Cart#add(Int, String)"));
        assert_eq!(kotlin(&s, 11).as_deref(), Some("shop.Cart.Companion"));
        assert_eq!(
            kotlin(&s, 12).as_deref(),
            Some("shop.Cart.Companion#create")
        );
    }

    #[test]
    fn python_names() {
        let python = |s: &[Symbol], line, character| {
            qualified_name(
                Some(LanguageId::Python),
                Some("shop.cart"),
                "",
                s,
                line,
                character,
            )
        };
        let s = [
            sym("LIMIT", "constant", 0, 0, 0, 0),
            sym("Cart", "class", 0, 2, 6, 14),
            sym("size", "variable", 1, 3, 4, 3),
            sym("total", "method", 1, 5, 8, 9),
            sym("helper", "function", 2, 6, 12, 7),
            sym("Line", "class", 1, 11, 10, 14),
            sym("net", "method", 2, 12, 12, 13),
            sym("main", "function", 0, 16, 4, 20),
            sym("Local", "class", 1, 17, 10, 19),
            sym("run", "method", 2, 18, 12, 19),
        ];
        assert_eq!(python(&s, 2, 6).as_deref(), Some("shop.cart.Cart"));
        assert_eq!(python(&s, 5, 8).as_deref(), Some("shop.cart.Cart.total"));
        assert_eq!(python(&s, 9, 0).as_deref(), Some("shop.cart.Cart.total"));
        assert_eq!(python(&s, 11, 10).as_deref(), Some("shop.cart.Cart.Line"));
        assert_eq!(
            python(&s, 12, 12).as_deref(),
            Some("shop.cart.Cart.Line.net")
        );
        assert_eq!(python(&s, 16, 4).as_deref(), Some("shop.cart.main"));
        // A class attribute is passed over for its class, a module variable has no name.
        assert_eq!(python(&s, 3, 4).as_deref(), Some("shop.cart.Cart"));
        assert_eq!(python(&s, 0, 0), None);
        // Nothing inside a function has a name.
        assert_eq!(python(&s, 6, 12), None);
        assert_eq!(python(&s, 17, 10), None);
        assert_eq!(python(&s, 18, 12), None);
        // Without a module the names start at the file.
        assert_eq!(
            qualified_name(Some(LanguageId::Python), None, "", &s, 5, 8).as_deref(),
            Some("Cart.total")
        );
    }

    #[test]
    fn python_modules() {
        assert_eq!(python_module("shop/cart.py").as_deref(), Some("shop.cart"));
        assert_eq!(python_module("shop/__init__.py").as_deref(), Some("shop"));
        assert_eq!(
            python_module("shop/types.pyi").as_deref(),
            Some("shop.types")
        );
        assert_eq!(python_module("main.py").as_deref(), Some("main"));
        assert_eq!(python_module("__init__.py"), None);
        // Below a source folder, else below the project folder.
        let root = Path::new("/work/shop");
        let roots = [PathBuf::from("/work/shop/src")];
        let module =
            |path: &str| module_of(Some(LanguageId::Python), root, Path::new(path), &roots);
        assert_eq!(
            module("/work/shop/src/shop/cart.py").as_deref(),
            Some("shop.cart")
        );
        assert_eq!(
            module("/work/shop/tools/gen.py").as_deref(),
            Some("tools.gen")
        );
        assert_eq!(module("/elsewhere/gen.py"), None);
        assert_eq!(
            module_of(
                Some(LanguageId::Java),
                root,
                Path::new("/work/shop/A.java"),
                &roots
            ),
            None
        );
    }

    #[test]
    fn rust_names() {
        let rust = |module: Option<&str>, s: &[Symbol], line, character| {
            qualified_name(Some(LanguageId::Rust), module, "", s, line, character)
        };
        // As rust-analyzer lists a file: an `impl` block is an object named after its header.
        let s = [
            sym("LIMIT", "constant", 0, 0, 6, 0),
            sym("Cart", "struct", 0, 2, 11, 5),
            sym("items", "field", 1, 3, 4, 3),
            sym("impl Cart", "object", 0, 7, 5, 14),
            sym("total", "method", 1, 8, 11, 12),
            sym("inner", "function", 2, 9, 11, 10),
            sym(
                "impl<T: Clone> fmt::Display for Cart<T>",
                "object",
                0,
                16,
                5,
                20,
            ),
            sym("fmt", "method", 1, 17, 7, 19),
            sym("Colour", "enum", 0, 22, 9, 25),
            sym("Red", "enum-member", 1, 23, 4, 23),
            sym("tests", "module", 0, 27, 4, 33),
            sym("Price", "interface", 1, 28, 14, 30),
            sym("net", "method", 2, 29, 11, 29),
            sym("totals", "function", 1, 32, 7, 32),
            sym("impl<'a> Price for &'a mut dyn Tax", "object", 0, 35, 5, 37),
            sym("net", "method", 1, 36, 7, 36),
            sym("Basket", "type-parameter", 0, 39, 9, 39),
        ];
        let m = Some("crate::cart");
        assert_eq!(rust(m, &s, 0, 6).as_deref(), Some("crate::cart::LIMIT"));
        assert_eq!(rust(m, &s, 2, 11).as_deref(), Some("crate::cart::Cart"));
        assert_eq!(
            rust(m, &s, 3, 4).as_deref(),
            Some("crate::cart::Cart::items")
        );
        assert_eq!(rust(m, &s, 7, 5).as_deref(), Some("crate::cart::Cart"));
        assert_eq!(
            rust(m, &s, 11, 0).as_deref(),
            Some("crate::cart::Cart::total")
        );
        assert_eq!(
            rust(m, &s, 18, 0).as_deref(),
            Some("crate::cart::Cart::fmt")
        );
        assert_eq!(
            rust(m, &s, 23, 4).as_deref(),
            Some("crate::cart::Colour::Red")
        );
        assert_eq!(rust(m, &s, 27, 4).as_deref(), Some("crate::cart::tests"));
        assert_eq!(
            rust(m, &s, 29, 11).as_deref(),
            Some("crate::cart::tests::Price::net")
        );
        assert_eq!(
            rust(m, &s, 32, 7).as_deref(),
            Some("crate::cart::tests::totals")
        );
        assert_eq!(rust(m, &s, 36, 7).as_deref(), Some("crate::cart::Tax::net"));
        // A type alias, which rust-analyzer lists as a type parameter.
        assert_eq!(rust(m, &s, 39, 9).as_deref(), Some("crate::cart::Basket"));
        // Nothing inside a function has a name.
        assert_eq!(rust(m, &s, 9, 11), None);
        // A file outside `src`: no `crate`, no modules.
        assert_eq!(rust(None, &s, 11, 0).as_deref(), Some("Cart::total"));
        assert_eq!(impl_type("impl Cart"), "Cart");
        assert_eq!(impl_type("impl<T> From<Vec<T>> for Cart<T>"), "Cart");
        assert_eq!(impl_type("unsafe impl Send for shop::Cart"), "Cart");
        assert_eq!(impl_type("impl Iterator for Lines<'_>"), "Lines");
    }

    #[test]
    fn rust_modules_of_a_file() {
        assert_eq!(rust_module_below_src("lib.rs"), "crate");
        assert_eq!(rust_module_below_src("main.rs"), "crate");
        assert_eq!(rust_module_below_src("cart.rs"), "crate::cart");
        assert_eq!(rust_module_below_src("cart/mod.rs"), "crate::cart");
        assert_eq!(rust_module_below_src("cart/line.rs"), "crate::cart::line");
        assert_eq!(rust_module_below_src("bin/tool.rs"), "crate");

        let root =
            std::env::temp_dir().join(format!("ferris-idea-reference-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/shop/src/cart")).unwrap();
        std::fs::create_dir_all(root.join("crates/shop/tests")).unwrap();
        std::fs::write(root.join("crates/shop/Cargo.toml"), "[package]\n").unwrap();
        let file = |rel: &str| {
            let path = root.join(rel);
            std::fs::write(&path, "").unwrap();
            path
        };
        assert_eq!(
            rust_module(&root, &file("crates/shop/src/cart/line.rs")).as_deref(),
            Some("crate::cart::line")
        );
        assert_eq!(
            rust_module(&root, &file("crates/shop/src/lib.rs")).as_deref(),
            Some("crate")
        );
        assert_eq!(rust_module(&root, &file("crates/shop/tests/it.rs")), None);
        assert_eq!(rust_module(&root, &file("crates/shop/build.rs")), None);
        // No Cargo.toml up to the project folder.
        assert_eq!(rust_module(&root, &file("loose.rs")), None);
        assert_eq!(
            module_of(
                Some(LanguageId::Rust),
                &root,
                &root.join("crates/shop/src/cart/line.rs"),
                &[]
            )
            .as_deref(),
            Some("crate::cart::line")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn names_outside_ascii() {
        let s = [
            sym("Größe", "class", 0, 2, 6, 8),
            sym("länge()", "method", 1, 3, 9, 5),
            sym("価格", "field", 1, 6, 8, 6),
        ];
        let text = "package größen.maße;\n";
        assert_eq!(java(text, &s, 2, 6).as_deref(), Some("größen.maße.Größe"));
        assert_eq!(
            java(text, &s, 4, 0).as_deref(),
            Some("größen.maße.Größe#länge")
        );
        assert_eq!(
            java(text, &s, 6, 8).as_deref(),
            Some("größen.maße.Größe#価格")
        );
        let py = [
            sym("Übung", "class", 0, 0, 6, 3),
            sym("löse", "method", 1, 1, 8, 2),
        ];
        assert_eq!(
            qualified_name(
                Some(LanguageId::Python),
                Some("kurs.übungen"),
                "",
                &py,
                1,
                8
            )
            .as_deref(),
            Some("kurs.übungen.Übung.löse")
        );
        assert_eq!(
            python_module("kurs/übungen.py").as_deref(),
            Some("kurs.übungen")
        );
        let rs = [
            sym("Maß", "struct", 0, 0, 7, 2),
            sym("länge", "field", 1, 1, 4, 1),
        ];
        assert_eq!(
            qualified_name(Some(LanguageId::Rust), Some("crate::maße"), "", &rs, 1, 4).as_deref(),
            Some("crate::maße::Maß::länge")
        );
    }

    #[test]
    fn caret_outside_every_symbol_and_on_a_local() {
        let s = cart();
        // The package line, the blank line after it, a line after the class.
        assert_eq!(java(PACKAGE, &s, 0, 8), None);
        assert_eq!(java(PACKAGE, &s, 1, 0), None);
        assert_eq!(java(PACKAGE, &s, 40, 0), None);
        assert_eq!(java(PACKAGE, &[], 3, 0), None);
        // A local variable, a parameter and a lambda are passed over for the method.
        let locals = [
            sym("Cart", "class", 0, 2, 13, 20),
            sym("total(int[])", "method", 1, 3, 8, 12),
            sym("sum", "variable", 2, 4, 12, 4),
            sym("xs", "type-parameter", 2, 3, 20, 3),
            sym("lambda$0", "function", 2, 6, 20, 8),
            sym("() -> {...}", "function", 2, 9, 20, 10),
        ];
        assert_eq!(
            java(PACKAGE, &locals, 4, 12).as_deref(),
            Some("shop.Cart#total")
        );
        assert_eq!(
            java(PACKAGE, &locals, 7, 0).as_deref(),
            Some("shop.Cart#total")
        );
        assert_eq!(
            java(PACKAGE, &locals, 9, 22).as_deref(),
            Some("shop.Cart#total")
        );
    }

    #[test]
    fn languages_without_rules() {
        let s = [sym("main", "function", 0, 0, 5, 3)];
        for language in [
            None,
            Some(LanguageId::Go),
            Some(LanguageId::TypeScript),
            Some(LanguageId::Json),
            Some(LanguageId::Markdown),
        ] {
            assert!(!has_rules(language), "{language:?}");
            assert_eq!(
                qualified_name(language, None, "", &s, 1, 0),
                None,
                "{language:?}"
            );
        }
        for language in [
            LanguageId::Java,
            LanguageId::Kotlin,
            LanguageId::Python,
            LanguageId::Rust,
        ] {
            assert!(has_rules(Some(language)), "{language:?}");
        }
    }

    #[test]
    fn reference_name_of_a_file() {
        let root = Path::new("/work/shop");
        let roots = [
            PathBuf::from("/work/shop/src/main/java"),
            PathBuf::from("/work/shop/src/main/java/gen"),
        ];
        let name = |path: &str| file_name(Path::new(path), root, &roots);
        assert_eq!(
            name("/work/shop/src/main/java/shop/Cart.java"),
            "shop/Cart.java"
        );
        assert_eq!(name("/work/shop/src/main/java/gen/a/B.java"), "a/B.java");
        assert_eq!(name("/work/shop/docs/a b/ü.md"), "docs/a b/ü.md");
        assert_eq!(name("/usr/lib/jdk/String.java"), "/usr/lib/jdk/String.java");
        assert_eq!(with_line("shop/Cart.java", 0), "shop/Cart.java:1");
        assert_eq!(with_line(&name("/work/shop/notes.txt"), 11), "notes.txt:12");
        assert_eq!(below(Path::new("/work/shop"), root), None);
        assert_eq!(below(Path::new("/work/shop-old/a"), root), None);
    }
}
