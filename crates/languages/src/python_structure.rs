//! The structure of a Python file (`SPEC-0079` v5 behaviour 12): the language server's symbols
//! without the parameters and local variables of functions, and each function with its parameter
//! list as detail. pyright and basedpyright report parameters and locals as children of a
//! function and give no signature; the parameter list is read from the syntax tree.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::LanguageId;
use crate::insight::Symbol;
use crate::syntax::SyntaxDocument;

/// A default value longer than this many characters is cut.
const DEFAULT_LIMIT: usize = 20;

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

/// `text` with runs of white space folded to one space, cut after [`DEFAULT_LIMIT`] characters.
fn short(text: &str) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() > DEFAULT_LIMIT {
        let mut cut: String = folded.chars().take(DEFAULT_LIMIT).collect();
        cut.push('…');
        cut
    } else {
        folded
    }
}

/// The name of a parameter with its stars, without annotation: `a`, `*args`, `**kw`.
fn starred_name(p: Node, text: &str) -> Option<String> {
    match p.kind() {
        "identifier" => Some(word(p, text).to_owned()),
        "list_splat_pattern" | "dictionary_splat_pattern" | "typed_parameter" => {
            let stars = match p.kind() {
                "list_splat_pattern" => "*",
                "dictionary_splat_pattern" => "**",
                _ => "",
            };
            let inner = children(p).into_iter().find(|c| {
                matches!(
                    c.kind(),
                    "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                )
            })?;
            Some(format!("{stars}{}", starred_name(inner, text)?))
        }
        _ => None,
    }
}

/// The parameter list of a function as it is shown: `(a, b=2, *args, **kw)`.
fn parameter_list(function: Node, text: &str) -> Option<String> {
    let list = function.child_by_field_name("parameters")?;
    let mut parts = Vec::new();
    for p in children(list) {
        let part = match p.kind() {
            "default_parameter" | "typed_default_parameter" => {
                let name = word(p.child_by_field_name("name")?, text);
                match p.child_by_field_name("value") {
                    Some(v) => format!("{name}={}", short(word(v, text))),
                    None => name.to_owned(),
                }
            }
            "keyword_separator" => "*".to_owned(),
            "positional_separator" => "/".to_owned(),
            "(" | ")" | "," | "comment" => continue,
            _ => match starred_name(p, text) {
                Some(n) => n,
                None => continue,
            },
        };
        parts.push(part);
    }
    Some(format!("({})", parts.join(", ")))
}

/// The parameter lists of the functions of `text`, by the 0-based line and character (in
/// characters) of their names.
fn parameter_lists(text: &str) -> HashMap<(u64, u64), String> {
    fn visit(n: Node, text: &str, lines: &[&str], out: &mut HashMap<(u64, u64), String>) {
        for c in children(n) {
            if c.kind() == "function_definition"
                && let Some(name) = c.child_by_field_name("name")
                && let Some(list) = parameter_list(c, text)
            {
                let at = name.start_position();
                let character = lines
                    .get(at.row)
                    .and_then(|l| l.get(..at.column))
                    .map_or(at.column, |before| before.chars().count());
                out.insert((at.row as u64, character as u64), list);
            }
            visit(c, text, lines, out);
        }
    }
    let mut out = HashMap::new();
    if let Ok(doc) = SyntaxDocument::new(LanguageId::Python, false, text) {
        let lines: Vec<&str> = text.split('\n').collect();
        visit(doc.tree().root_node(), text, &lines, &mut out);
    }
    out
}

/// `symbols` without what is below a function and is neither a function (`is_function`) nor a
/// type (`is_type`): parameters, local variables and what is below those.
pub(crate) fn without_locals(
    symbols: Vec<Symbol>,
    is_function: impl Fn(&Symbol) -> bool,
    is_type: impl Fn(&Symbol) -> bool,
) -> Vec<Symbol> {
    // The enclosing symbols of the current one: (depth, is a function).
    let mut open: Vec<(usize, bool)> = Vec::new();
    // Everything deeper than this depth belongs to a dropped symbol.
    let mut dropped_below: Option<usize> = None;
    let mut out = Vec::with_capacity(symbols.len());
    for s in symbols {
        if let Some(d) = dropped_below {
            if s.depth > d {
                continue;
            }
            dropped_below = None;
        }
        while open.last().is_some_and(|&(d, _)| d >= s.depth) {
            open.pop();
        }
        let in_function = open.last().is_some_and(|&(_, function)| function);
        let function = is_function(&s);
        if in_function && !function && !is_type(&s) {
            dropped_below = Some(s.depth);
            continue;
        }
        open.push((s.depth, function));
        out.push(s);
    }
    out
}

/// The server's `symbols` of the Python file with content `text` as the structure shows them:
/// below a function only nested functions and classes, each function with its parameter list.
pub fn structure(text: &str, mut symbols: Vec<Symbol>) -> Vec<Symbol> {
    let lists = parameter_lists(text);
    for s in &mut symbols {
        if is_function(s)
            && let Some(list) = lists.get(&(s.line, s.character))
        {
            s.detail = Some(list.clone());
        }
    }
    without_locals(symbols, is_function, |s| s.kind == "class")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, kind: &str, line: u64, character: u64, depth: usize) -> Symbol {
        Symbol {
            name: name.to_owned(),
            kind: kind.to_owned(),
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
                    "{}{}{}",
                    "  ".repeat(s.depth),
                    s.name,
                    s.detail.as_deref().unwrap_or_default()
                )
            })
            .collect()
    }

    /// SPEC-0079/AK-7: parameters and locals are left out, nested functions and classes stay,
    /// module variables and class fields stay.
    #[test]
    fn parameters_and_locals_are_left_out() {
        let text = "\
def messy( a,b = 2,*args,**kw ):
  x=a+b
  def inner(q):
    z = q
    return z
  class Local:
    tag = 1
    def m(self):
      v = 1
  return x
class Shape :
  sides=4
  def area( self ):
    self.cache = 0
    return 0
lam = lambda q:q*2
";
        let symbols = vec![
            symbol("messy", "function", 0, 4, 0),
            symbol("a", "variable", 0, 11, 1),
            symbol("b", "variable", 0, 13, 1),
            symbol("args", "variable", 0, 20, 1),
            symbol("kw", "variable", 0, 27, 1),
            symbol("x", "variable", 1, 2, 1),
            symbol("inner", "function", 2, 6, 1),
            symbol("q", "variable", 2, 12, 2),
            symbol("z", "variable", 3, 4, 2),
            symbol("Local", "class", 5, 8, 1),
            symbol("tag", "variable", 6, 4, 2),
            symbol("m", "method", 7, 8, 2),
            symbol("self", "variable", 7, 10, 3),
            symbol("v", "variable", 8, 6, 3),
            symbol("Shape", "class", 10, 6, 0),
            symbol("sides", "variable", 11, 2, 1),
            symbol("area", "method", 12, 6, 1),
            symbol("self", "variable", 12, 12, 2),
            symbol("cache", "variable", 13, 9, 2),
            symbol("lam", "variable", 15, 0, 0),
        ];
        assert_eq!(
            rows(&structure(text, symbols)),
            [
                "messy(a, b=2, *args, **kw)",
                "  inner(q)",
                "  Local",
                "    tag",
                "    m(self)",
                "Shape",
                "  sides",
                "  area(self)",
                "lam",
            ]
        );
    }

    /// SPEC-0079/AK-7: the forms of a parameter list.
    #[test]
    fn parameter_lists_as_shown() {
        let text = "\
def f(a: int, b: str = 'x', /, c=None, *, d, e: int = 1) -> int: ...
def g(*args: int, **kw: str): ...
def h(big = {'one': 1, 'two': 2, 'three': 3},
      spaced = ( 1 ,
                 2 )): ...
def none(): ...
ä = 'ö'; ü = 1
class Ö:
    async def größe(self, maß=1): ...
";
        let lists = parameter_lists(text);
        let at = |line, character| lists.get(&(line, character)).map(String::as_str);
        assert_eq!(at(0, 4), Some("(a, b='x', /, c=None, *, d, e=1)"));
        assert_eq!(at(1, 4), Some("(*args, **kw)"));
        assert_eq!(
            at(2, 4),
            Some("(big={'one': 1, 'two': 2,…, spaced=( 1 , 2 ))")
        );
        assert_eq!(at(5, 4), Some("()"));
        // Characters, not bytes, on a line with non-ASCII text.
        assert_eq!(at(8, 14), Some("(self, maß=1)"));
        assert_eq!(lists.len(), 5);
    }

    /// SPEC-0079/AK-7: a function the text does not have at the symbol's place keeps the
    /// server's detail.
    #[test]
    fn a_symbol_without_a_function_in_the_text_keeps_its_detail() {
        let mut stale = symbol("gone", "function", 7, 4, 0);
        stale.detail = Some("from the server".to_owned());
        let out = structure("def here(a):\n    pass\n", vec![stale.clone()]);
        assert_eq!(out, [stale]);
        // Parameters go also where the text gives no parameter list.
        let out = structure(
            "",
            vec![
                symbol("ok", "function", 0, 4, 0),
                symbol("a", "variable", 0, 7, 1),
            ],
        );
        assert_eq!(rows(&out), ["ok"]);
    }
}
