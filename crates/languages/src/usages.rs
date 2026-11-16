//! Usage counts of the types declared in a file (`SPEC-0292`): document symbols, then the server's
//! references of each type without its declaration.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::insight::Opened;
use crate::lsp::LspError;

/// Types counted per file at most (behaviour 1).
pub const MAX_TYPES: usize = 50;

/// A counted declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageCount {
    /// The type's name.
    pub name: String,
    /// `class`, `interface`, `enum` or `struct`.
    pub kind: &'static str,
    /// 0-based line of the declaration.
    pub line: u64,
    /// Character of the name on that line (server units, as the server gave it).
    #[serde(skip)]
    pub character: u64,
    /// Usages found.
    pub count: usize,
    /// Project subtypes of a type (`SPEC-0320` behaviour 6); for a Kotlin member the members of
    /// subtypes that override or implement it (`SPEC-0292` v6 behaviour 9).
    pub inheritors: usize,
}

impl UsageCount {
    /// `1 usage` or `N usages` (behaviour 4).
    pub fn label(&self) -> String {
        let usages = match self.count {
            0 => "no usages".to_owned(),
            1 => "1 usage".to_owned(),
            n => format!("{n} usages"),
        };
        // Below a type are inheritors, below an abstract member implementations, below any
        // other member overrides (`SPEC-0292` v6 behaviour 9).
        let (one, many) = match self.kind {
            "abstract function" | "abstract method" | "abstract property" => {
                ("implementation", "implementations")
            }
            "method" | "function" | "property" => ("override", "overrides"),
            _ => ("inheritor", "inheritors"),
        };
        match self.inheritors {
            0 => usages,
            1 => format!("{usages} · 1 {one}"),
            n => format!("{usages} · {n} {many}"),
        }
    }
}

/// The types among `symbols` (a `documentSymbol` answer), nested ones included, in document order,
/// at most [`MAX_TYPES`] (behaviour 1). Only hierarchical answers carry the name's position; flat
/// ones use the symbol's start.
pub fn types(symbols: &Value) -> Vec<UsageCount> {
    fn walk(list: &Value, out: &mut Vec<UsageCount>) {
        for s in list.as_array().into_iter().flatten() {
            let kind = match s["kind"].as_u64() {
                Some(5) => "class",
                Some(10) => "enum",
                Some(11) => "interface",
                Some(23) => "struct",
                _ => "",
            };
            let at = if s["selectionRange"].is_object() {
                &s["selectionRange"]["start"]
            } else if s["location"].is_object() {
                &s["location"]["range"]["start"]
            } else {
                &s["range"]["start"]
            };
            if !kind.is_empty()
                && let (Some(line), Some(character)) =
                    (at["line"].as_u64(), at["character"].as_u64())
            {
                out.push(UsageCount {
                    name: s["name"].as_str().unwrap_or_default().to_owned(),
                    kind,
                    line,
                    character,
                    count: 0,
                    inheritors: 0,
                });
            }
            walk(&s["children"], out);
        }
    }
    let mut out = Vec::new();
    walk(symbols, &mut out);
    out.sort_by_key(|u| (u.line, u.character));
    out.truncate(MAX_TYPES);
    out
}

/// Places of a `references` answer, each counted once (behaviour 2).
pub fn distinct_places(references: &Value) -> usize {
    references
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| {
            Some((
                l["uri"].as_str()?.to_owned(),
                l["range"]["start"]["line"].as_u64()?,
                l["range"]["start"]["character"].as_u64()?,
            ))
        })
        .collect::<BTreeSet<_>>()
        .len()
}

/// [`distinct_places`] without the declaration itself at `line` and `character` of `uri`: a server
/// may list it although it was asked not to (vscode-css-language-server, behaviour 2).
pub fn distinct_places_besides(references: &Value, uri: &str, line: u64, character: u64) -> usize {
    let own = references.as_array().into_iter().flatten().any(|l| {
        l["uri"].as_str() == Some(uri)
            && l["range"]["start"]["line"].as_u64() == Some(line)
            && l["range"]["start"]["character"].as_u64() == Some(character)
    });
    distinct_places(references) - usize::from(own)
}

/// Usage counts of the types declared in `file` whose content is `text` or the disk content; types
/// without usages are left out (behaviour 2).
pub async fn usage_counts_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<UsageCount>, LspError> {
    if crate::ts_structure::is_script(file) {
        return crate::ts_structure::usage_counts_in(root, file, text, wait).await;
    }
    if crate::java_marks::is_java(file) {
        return crate::java_marks::usage_counts_in(root, file, text, wait).await;
    }
    if crate::kotlin_structure::is_kotlin(file) {
        return crate::kotlin_structure::usage_counts_in(root, file, text, wait).await;
    }
    let o = Opened::open_text(root, file, text, wait).await?;
    let uri = o.uri.clone();
    let result = async {
        let symbols = o
            .ask(
                "textDocument/documentSymbol",
                json!({"textDocument": {"uri": uri}}),
            )
            .await?;
        let mut found = Vec::new();
        for mut t in types(&symbols) {
            let references = o
                .ask(
                    "textDocument/references",
                    json!({"textDocument": {"uri": uri},
                        "position": {"line": t.line, "character": t.character},
                        "context": {"includeDeclaration": false}}),
                )
                .await?;
            t.count = distinct_places_besides(&references, &uri, t.line, t.character);
            if t.count > 0 {
                found.push(t);
            }
        }
        Ok::<_, LspError>(found)
    }
    .await;
    o.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, kind: u64, line: u64, children: Value) -> Value {
        json!({"name": name, "kind": kind,
            "range": {"start": {"line": line, "character": 0}, "end": {"line": line + 3, "character": 1}},
            "selectionRange": {"start": {"line": line, "character": 7}, "end": {"line": line, "character": 9}},
            "children": children})
    }

    /// SPEC-0292 AK-13: a declaration the server lists among its own usages is not counted.
    #[test]
    fn the_declaration_itself_is_no_usage() {
        let at = |uri: &str, line: u64, character: u64| {
            json!({"uri": uri, "range": {
            "start": {"line": line, "character": character},
            "end": {"line": line, "character": character + 4}}})
        };
        let found = json!([
            at("file:///a.css", 4, 0),
            at("file:///a.css", 9, 0),
            at("file:///b.css", 4, 0)
        ]);
        assert_eq!(distinct_places(&found), 3);
        assert_eq!(distinct_places_besides(&found, "file:///a.css", 4, 0), 2);
        assert_eq!(distinct_places_besides(&found, "file:///a.css", 5, 0), 3);
        assert_eq!(
            distinct_places_besides(&json!(null), "file:///a.css", 4, 0),
            0
        );
    }

    /// SPEC-0292 AK-1: only types are counted, nested ones too, in document order.
    #[test]
    fn chooses_types_from_the_symbol_tree() {
        let tree = json!([sym(
            "Inventory",
            5,
            4,
            json!([
                sym(
                    "Item",
                    23,
                    5,
                    json!([
                        sym("sku", 8, 5, json!([])),
                        sym("totalCents", 6, 6, json!([]))
                    ])
                ),
                sym("Kind", 10, 12, json!([sym("A", 22, 13, json!([]))])),
                sym("Named", 11, 15, json!([])),
                sym("main", 6, 18, json!([])),
            ])
        )]);
        let t = types(&tree);
        let names: Vec<(&str, &str, u64)> = t
            .iter()
            .map(|u| (u.name.as_str(), u.kind, u.line))
            .collect();
        assert_eq!(
            names,
            [
                ("Inventory", "class", 4),
                ("Item", "struct", 5),
                ("Kind", "enum", 12),
                ("Named", "interface", 15)
            ]
        );
        assert_eq!(t[1].character, 7);
        // A flat answer (SymbolInformation) uses the location's start.
        let flat = json!([{"name": "A", "kind": 5, "location": {"uri": "file:///a",
            "range": {"start": {"line": 2, "character": 0}, "end": {"line": 3, "character": 0}}}}]);
        assert_eq!(types(&flat)[0].line, 2);
    }

    /// SPEC-0292 AK-1: a place named twice counts once; the label has the right number.
    #[test]
    fn counts_places_once_and_labels() {
        let at = |uri: &str, line: u64, c: u64| json!({"uri": uri, "range": {"start": {"line": line, "character": c}, "end": {"line": line, "character": c + 4}}});
        let refs = json!([
            at("file:///a", 1, 2),
            at("file:///a", 1, 2),
            at("file:///a", 3, 2),
            at("file:///b", 1, 2)
        ]);
        assert_eq!(distinct_places(&refs), 3);
        assert_eq!(distinct_places(&Value::Null), 0);
        let mut u = UsageCount {
            name: "Item".into(),
            kind: "struct",
            line: 0,
            character: 0,
            count: 1,
            inheritors: 0,
        };
        assert_eq!(u.label(), "1 usage");
        u.count = 3;
        assert_eq!(u.label(), "3 usages");
        u.count = 0;
        u.inheritors = 2;
        assert_eq!(u.label(), "no usages · 2 inheritors");
        u.count = 1;
        u.inheritors = 1;
        assert_eq!(u.label(), "1 usage · 1 inheritor");
        // SPEC-0292 AK-12 (v6): what is below a member is named by what the member is.
        u.kind = "abstract function";
        assert_eq!(u.label(), "1 usage · 1 implementation");
        u.inheritors = 3;
        assert_eq!(u.label(), "1 usage · 3 implementations");
        u.kind = "method";
        u.count = 0;
        assert_eq!(u.label(), "no usages · 3 overrides");
    }
}
