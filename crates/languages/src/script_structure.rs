//! The structure of a JavaScript or TypeScript file (`SPEC-0079` v6 behaviour 13): the language
//! server's symbols without the local variables of functions, an arrow function bound to a name
//! as a function, each function with its parameter list and annotated return type, each member
//! and variable with its annotated type. typescript-language-server reports locals as children
//! of a function and no detail; signatures and types are read from the syntax tree.

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::Node;

use crate::LanguageId;
use crate::insight::Symbol;
use crate::python_structure::without_locals;
use crate::syntax::SyntaxDocument;

/// A destructuring pattern longer than this many characters is cut.
const PATTERN_LIMIT: usize = 20;
/// A type longer than this many characters is cut.
const TYPE_LIMIT: usize = 40;

/// What the text declares at a name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Declaration {
    /// A function: what it declares locally is left out.
    function: bool,
    /// A variable or constant holding a function, reported with kind `function`.
    bound: bool,
    /// The parameter list with the return type, or the type of a member or variable.
    detail: Option<String>,
}

fn is_function(s: &Symbol) -> bool {
    matches!(s.kind.as_str(), "function" | "method" | "constructor")
}

fn children(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.children(&mut cursor).collect()
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

/// `text` with runs of white space folded to one space, cut after `limit` characters.
fn short(text: &str, limit: usize) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() > limit {
        let mut cut: String = folded.chars().take(limit).collect();
        cut.push('…');
        cut
    } else {
        folded
    }
}

/// An annotation (`: number`, `: x is string`) as it is shown, empty without one.
fn annotation(node: Option<Node>, text: &str) -> String {
    node.map(|n| word(n, text).trim_start_matches(':').trim())
        .filter(|t| !t.is_empty())
        .map(|t| format!(": {}", short(t, TYPE_LIMIT)))
        .unwrap_or_default()
}

/// The name of a parameter: an identifier, `...rest`, or a destructuring pattern.
fn parameter_name(pattern: Node, text: &str) -> String {
    match pattern.kind() {
        "rest_pattern" => {
            let inner = children(pattern)
                .into_iter()
                .rfind(|c| c.is_named())
                .map(|c| parameter_name(c, text))
                .unwrap_or_default();
            format!("...{inner}")
        }
        "identifier" | "this" => word(pattern, text).to_owned(),
        _ => short(word(pattern, text), PATTERN_LIMIT),
    }
}

/// One parameter as it is shown: `a: number`, `b?`, `...rest: string[]`.
fn parameter(p: Node, text: &str) -> Option<String> {
    if !p.is_named() {
        return None;
    }
    match p.kind() {
        "comment" | "decorator" | "ERROR" => None,
        // TypeScript.
        "required_parameter" | "optional_parameter" => {
            let name = parameter_name(p.child_by_field_name("pattern")?, text);
            let optional =
                p.kind() == "optional_parameter" || p.child_by_field_name("value").is_some();
            let mark = if optional { "?" } else { "" };
            let ty = annotation(p.child_by_field_name("type"), text);
            Some(format!("{name}{mark}{ty}"))
        }
        // JavaScript: `b = 2`.
        "assignment_pattern" => Some(format!(
            "{}?",
            parameter_name(p.child_by_field_name("left")?, text)
        )),
        _ => Some(parameter_name(p, text)),
    }
}

/// The parameter list of a function with its annotated return type: `(a: number, b?): number`.
fn signature(function: Node, text: &str) -> Option<String> {
    let list = match function.child_by_field_name("parameters") {
        Some(list) => children(list)
            .into_iter()
            .filter_map(|p| parameter(p, text))
            .collect::<Vec<_>>()
            .join(", "),
        // `q => q + 1`
        None => parameter_name(function.child_by_field_name("parameter")?, text),
    };
    let returns = annotation(function.child_by_field_name("return_type"), text);
    Some(format!("({list}){returns}"))
}

fn some(detail: String) -> Option<String> {
    Some(detail).filter(|d| !d.is_empty())
}

/// What `node` declares, with the node of the declared name.
fn declaration<'a>(node: Node<'a>, text: &str) -> Option<(Node<'a>, Declaration)> {
    let function = |detail| Declaration {
        function: true,
        bound: false,
        detail,
    };
    match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_signature"
        | "method_signature"
        | "abstract_method_signature" => Some((
            node.child_by_field_name("name")?,
            function(signature(node, text)),
        )),
        "method_definition" => {
            let name = node.child_by_field_name("name")?;
            // A getter reads like a member: its type alone.
            let getter = children(node)
                .into_iter()
                .take_while(|c| c.id() != name.id())
                .any(|c| c.kind() == "get");
            let detail = if getter {
                some(annotation(node.child_by_field_name("return_type"), text))
            } else {
                signature(node, text)
            };
            Some((name, function(detail)))
        }
        "variable_declarator"
        | "public_field_definition"
        | "field_definition"
        | "property_signature"
        | "pair" => {
            let name = ["name", "property", "key"]
                .into_iter()
                .find_map(|f| node.child_by_field_name(f))?;
            let value = node.child_by_field_name("value").filter(|v| {
                matches!(
                    v.kind(),
                    "arrow_function" | "function_expression" | "generator_function"
                )
            });
            match value {
                Some(v) => Some((
                    name,
                    Declaration {
                        function: true,
                        bound: node.kind() == "variable_declarator",
                        detail: signature(v, text),
                    },
                )),
                None if node.kind() == "pair" => None,
                None => Some((
                    name,
                    Declaration {
                        function: false,
                        bound: false,
                        detail: some(annotation(node.child_by_field_name("type"), text)),
                    },
                )),
            }
        }
        _ => None,
    }
}

/// The declarations of `text`, by the 0-based line and character (in characters) of their names.
fn declarations(file: &Path, text: &str) -> HashMap<(u64, u64), Declaration> {
    fn visit(n: Node, text: &str, lines: &[&str], out: &mut HashMap<(u64, u64), Declaration>) {
        for c in children(n) {
            if let Some((name, found)) = declaration(c, text) {
                let at = name.start_position();
                let character = lines
                    .get(at.row)
                    .and_then(|l| l.get(..at.column))
                    .map_or(at.column, |before| before.chars().count());
                out.insert((at.row as u64, character as u64), found);
            }
            visit(c, text, lines, out);
        }
    }
    let mut out = HashMap::new();
    let Some(language) = LanguageId::detect(file) else {
        return out;
    };
    let tsx = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tsx"));
    if let Ok(doc) = SyntaxDocument::new(language, tsx, text) {
        let lines: Vec<&str> = text.split('\n').collect();
        visit(doc.tree().root_node(), text, &lines, &mut out);
    }
    out
}

/// Whether `file` is a JavaScript or TypeScript file.
pub fn applies(file: &Path) -> bool {
    matches!(
        LanguageId::detect(file),
        Some(LanguageId::JavaScript | LanguageId::TypeScript)
    )
}

/// The server's `symbols` of the JavaScript or TypeScript `file` with content `text` as the
/// structure shows them: below a function only nested functions and types, each function with
/// its parameter list, members and variables with their annotated type.
pub fn structure(file: &Path, text: &str, mut symbols: Vec<Symbol>) -> Vec<Symbol> {
    let found = declarations(file, text);
    for s in &mut symbols {
        let Some(d) = found.get(&(s.line, s.character)) else {
            continue;
        };
        if d.bound && matches!(s.kind.as_str(), "variable" | "constant") {
            s.kind = "function".to_owned();
        }
        if let Some(detail) = &d.detail {
            s.detail = Some(detail.clone());
        }
    }
    without_locals(
        symbols,
        |s| {
            is_function(s)
                || found
                    .get(&(s.line, s.character))
                    .is_some_and(|d| d.function)
        },
        |s| matches!(s.kind.as_str(), "class" | "interface" | "enum"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, kind: &str, line: u64, character: u64, depth: usize) -> Symbol {
        Symbol {
            name: name.to_owned(),
            kind: kind.to_owned(),
            detail: Some(String::new()),
            line,
            character,
            depth,
            ..Symbol::default()
        }
    }

    fn rows(symbols: &[Symbol]) -> Vec<String> {
        symbols
            .iter()
            .map(|s| {
                format!(
                    "{}{} {}{}",
                    "  ".repeat(s.depth),
                    s.kind,
                    s.name,
                    s.detail.as_deref().unwrap_or_default()
                )
            })
            .collect()
    }

    /// The detail the text gives the declaration whose name starts at the first occurrence of
    /// `name`.
    fn detail(file: &str, text: &str, name: &str) -> Option<String> {
        let (line, column) = text
            .lines()
            .enumerate()
            .find_map(|(i, l)| l.find(name).map(|c| (i, l[..c].chars().count())))
            .unwrap_or_else(|| panic!("{name} in the text"));
        declarations(Path::new(file), text)
            .get(&(line as u64, column as u64))
            .unwrap_or_else(|| panic!("a declaration at {name}"))
            .detail
            .clone()
    }

    /// SPEC-0079/AK-8 (OBS-0028 T1): locals are left out, nested functions and classes stay, an
    /// arrow function bound to a name is a function.
    #[test]
    fn locals_are_left_out_and_arrow_functions_are_functions() {
        let text = "\
export function  messy( a:number,b = 2 ,...rest:string[] ){
  const x=a+b*2
  const helper = (v: number) => { const t = v; return t }
  function inner(q) { let z = q; return z }
  class Local { tag = 1; m() { const v = 1 } }
  const o = { k: 1 }
  return x
}
export class  Shape {
    sides=0
    area(): number { return 0.0 }
    name( ) {
        return 'shape'
    }
    handler = (e: Event) => { const seen = e }
}
export const lam = (q:number)=>q+1
let count: number = 0
";
        // As typescript-language-server lists them.
        let symbols = vec![
            symbol("messy", "function", 0, 17, 0),
            symbol("x", "constant", 1, 8, 1),
            symbol("helper", "constant", 2, 8, 1),
            symbol("t", "constant", 2, 40, 2),
            symbol("inner", "function", 3, 11, 1),
            symbol("z", "variable", 3, 26, 2),
            symbol("Local", "class", 4, 8, 1),
            symbol("tag", "property", 4, 16, 2),
            symbol("m", "method", 4, 25, 2),
            symbol("v", "constant", 4, 37, 3),
            symbol("o", "constant", 5, 8, 1),
            symbol("k", "property", 5, 14, 2),
            symbol("Shape", "class", 8, 14, 0),
            symbol("sides", "property", 9, 4, 1),
            symbol("area", "method", 10, 4, 1),
            symbol("name", "method", 11, 4, 1),
            symbol("handler", "property", 14, 4, 1),
            symbol("seen", "constant", 14, 36, 2),
            symbol("lam", "constant", 16, 13, 0),
            symbol("count", "variable", 17, 4, 0),
        ];
        assert_eq!(
            rows(&structure(Path::new("fmt.ts"), text, symbols)),
            [
                "function messy(a: number, b?, ...rest: string[])",
                "  function helper(v: number)",
                "  function inner(q)",
                "  class Local",
                "    property tag",
                "    method m()",
                "class Shape",
                "  property sides",
                "  method area(): number",
                "  method name()",
                "  property handler(e: Event)",
                "function lam(q: number)",
                "variable count: number",
            ]
        );
    }

    /// SPEC-0079/AK-8: the forms of a parameter list, return types and member types.
    #[test]
    fn parameter_lists_and_types_as_shown() {
        let text = "\
function plain(a: string, b?: number, c: boolean = true, { d, e }: Opts, [f]: number[]): void {}
function guard(x: unknown): x is string { return true }
function long({ first, second, third, fourth }, m: Map<string, Array<Record<string, number>>>) {}
function spread(...rest) {}
function generic<T extends object>(value: T, cb: (v: T) => void): Promise<T> {}
function over(a: string): string;
async function* gen(n: number): AsyncGenerator<number> {}
abstract class Box {
  constructor(private readonly size: number, public label = 'x') {}
  static of(this: void, n: number): Box { return null! }
  get width(): number { return 1 }
  set width(w: number) {}
  get loose() { return 1 }
  abstract area(scale: number): number;
  items: Array<string> = [];
  private note?: string;
  untyped = 1;
}
interface Shape { sides: number; area(unit?: string): number; readonly id: string }
const api = { load: async (id: string): Promise<void> => {}, plain: 1 };
const one = q => q;
const fn = function (a, b = 1) {};
const ä = 'ö'; const größe = (maß: number): number => maß;
";
        let d = |name: &str| detail("a.ts", text, name).unwrap_or_default();
        assert_eq!(
            d("plain"),
            "(a: string, b?: number, c?: boolean, { d, e }: Opts, [f]: number[]): void"
        );
        assert_eq!(d("guard"), "(x: unknown): x is string");
        assert_eq!(
            d("long"),
            "({ first, second, thi…, m: Map<string, Array<Record<string, number>…)"
        );
        assert_eq!(d("spread"), "(...rest)");
        assert_eq!(d("generic"), "(value: T, cb: (v: T) => void): Promise<T>");
        assert_eq!(d("over"), "(a: string): string");
        assert_eq!(d("gen("), "(n: number): AsyncGenerator<number>");
        assert_eq!(d("constructor"), "(size: number, label?)");
        assert_eq!(d("of("), "(this: void, n: number): Box");
        assert_eq!(d("width(): number"), ": number");
        assert_eq!(d("width(w"), "(w: number)");
        assert_eq!(d("loose"), "");
        assert_eq!(d("area(scale"), "(scale: number): number");
        assert_eq!(d("items"), ": Array<string>");
        assert_eq!(d("note"), ": string");
        assert_eq!(d("untyped"), "");
        assert_eq!(d("sides"), ": number");
        assert_eq!(d("area(unit"), "(unit?: string): number");
        assert_eq!(d("id: string"), ": string");
        assert_eq!(d("load"), "(id: string): Promise<void>");
        assert_eq!(d("one"), "(q)");
        assert_eq!(d("fn ="), "(a, b?)");
        // Characters, not bytes, before the name.
        assert_eq!(d("größe"), "(maß: number): number");
        let found = declarations(Path::new("a.ts"), text);
        assert!(!found.values().any(|v| v.detail.as_deref() == Some("")));
    }

    /// SPEC-0079/AK-8: JavaScript and TSX go through their own grammars.
    #[test]
    fn javascript_and_tsx() {
        let js = "\
export function messy(a, b = 2, ...rest) { const x = 1 }
class Shape { sides = 0; #secret = 1; area() { return 0 } static make = () => new Shape() }
const lam = (q) => q + 1
module.exports = { run(task, { retries } = {}) {} }
";
        let d = |name: &str| detail("a.js", js, name);
        assert_eq!(d("messy").as_deref(), Some("(a, b?, ...rest)"));
        assert_eq!(d("sides"), None);
        assert_eq!(d("area").as_deref(), Some("()"));
        assert_eq!(d("make").as_deref(), Some("()"));
        assert_eq!(d("lam").as_deref(), Some("(q)"));
        assert_eq!(d("run").as_deref(), Some("(task, { retries }?)"));
        let symbols = vec![
            symbol("messy", "function", 0, 16, 0),
            symbol("x", "constant", 0, 49, 1),
            symbol("lam", "constant", 2, 6, 0),
        ];
        assert_eq!(
            rows(&structure(Path::new("a.mjs"), js, symbols)),
            ["function messy(a, b?, ...rest)", "function lam(q)"]
        );

        let tsx = "\
export const App = ({ title }: Props): JSX.Element => { const n = 1; return <h1>{title}</h1> }
function Item(props: { id: number }) { return <li>{props.id}</li> }
";
        let d = |name: &str| detail("App.tsx", tsx, name);
        assert_eq!(d("App").as_deref(), Some("({ title }: Props): JSX.Element"));
        assert_eq!(d("Item").as_deref(), Some("(props: { id: number })"));
        let symbols = vec![
            symbol("App", "constant", 0, 13, 0),
            symbol("n", "constant", 0, 62, 1),
            symbol("Item", "function", 1, 9, 0),
        ];
        assert_eq!(
            rows(&structure(Path::new("App.tsx"), tsx, symbols)),
            [
                "function App({ title }: Props): JSX.Element",
                "function Item(props: { id: number })"
            ]
        );
    }

    /// SPEC-0079/AK-8: a symbol the text has no declaration for keeps the server's kind and
    /// detail, and its place in the tree.
    #[test]
    fn a_symbol_without_a_declaration_in_the_text_keeps_its_detail() {
        let text = "function f(a: number) {}\n";
        let mut stale = symbol("g", "constant", 5, 4, 0);
        stale.detail = Some("from the server".to_owned());
        let symbols = vec![
            symbol("f", "function", 0, 9, 0),
            symbol("items.map() callback", "function", 0, 30, 1),
            symbol("local", "variable", 0, 40, 2),
            stale,
        ];
        assert_eq!(
            rows(&structure(Path::new("a.ts"), text, symbols)),
            [
                "function f(a: number)",
                "  function items.map() callback",
                "constant gfrom the server",
            ]
        );
        assert!(applies(Path::new("a.cts")) && applies(Path::new("a.jsx")));
        assert!(!applies(Path::new("App.vue")) && !applies(Path::new("a.py")));
    }
}
