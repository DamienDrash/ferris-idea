//! Usage counts of the declarations of a file (`SPEC-0292`): document symbols, then the server's
//! usage lenses or its references of each declaration without the declaration itself; the
//! declarations on the visible lines first, within a budget (v8 behaviours 10–12).

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::insight::Opened;
use crate::lsp::LspError;

/// Types counted per file at most (behaviour 1).
pub const MAX_TYPES: usize = 50;
/// Declarations counted per question at most (`SPEC-0292` v8 behaviours 10 and 12).
pub const MAX_DECLARATIONS: usize = 200;
/// Lines above and below the visible ones that the first question counts too (v8 behaviour 12).
pub const FOCUS_MARGIN: u64 = 20;
/// `source` of a count from the server's references.
pub const FROM_REFERENCES: &str = "references";
/// `source` of a count from a usage lens (v8 behaviour 11).
pub const FROM_CODE_LENS: &str = "code lens";

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
    /// Where the count comes from: [`FROM_REFERENCES`] or [`FROM_CODE_LENS`] (v8 behaviour 17).
    pub source: &'static str,
}

impl UsageCount {
    /// A declaration not counted yet.
    pub fn new(name: &str, kind: &'static str, line: u64, character: u64) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            line,
            character,
            count: 0,
            inheritors: 0,
            source: FROM_REFERENCES,
        }
    }

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
                out.push(UsageCount::new(
                    s["name"].as_str().unwrap_or_default(),
                    kind,
                    line,
                    character,
                ));
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

/// Where a document symbol's name starts: the selection range of a hierarchical answer, the
/// location of a flat one, else the symbol's start.
fn name_start(s: &Value) -> (Option<u64>, Option<u64>) {
    let at = if s["selectionRange"].is_object() {
        &s["selectionRange"]["start"]
    } else if s["location"].is_object() {
        &s["location"]["range"]["start"]
    } else {
        &s["range"]["start"]
    };
    (at["line"].as_u64(), at["character"].as_u64())
}

/// The declarations among `symbols` that get a count in a language without a module of its own
/// (`SPEC-0292` v8 behaviour 10): types, and the methods, functions, constructors, fields,
/// properties and enum members of the file or of a type — nothing inside a function or method, no
/// variables or constants. In document order, not cut: the plan cuts (behaviour 12).
pub fn declarations(symbols: &Value) -> Vec<UsageCount> {
    fn walk(list: &Value, out: &mut Vec<UsageCount>) {
        for s in list.as_array().into_iter().flatten() {
            let (kind, body) = match s["kind"].as_u64() {
                Some(5) => ("class", false),
                Some(10) => ("enum", false),
                Some(11) => ("interface", false),
                Some(23) => ("struct", false),
                Some(6) => ("method", true),
                Some(12) => ("function", true),
                Some(9) => ("constructor", true),
                Some(8) => ("field", false),
                Some(7) => ("property", false),
                Some(22) => ("enum member", false),
                // Variables and constants are not counted, nor what they hold.
                Some(13 | 14) => continue,
                // Modules, namespaces, packages, impl blocks: what is in them.
                _ => ("", false),
            };
            let name = s["name"].as_str().unwrap_or_default();
            if !kind.is_empty()
                && !name.is_empty()
                && let (Some(line), Some(character)) = name_start(s)
            {
                out.push(UsageCount::new(name, kind, line, character));
            }
            // Locals and nested functions of a function or method are not counted.
            if !body {
                walk(&s["children"], out);
            }
        }
    }
    let mut out = Vec::new();
    walk(symbols, &mut out);
    out.sort_by_key(|u| (u.line, u.character));
    out
}

/// Which declarations a question counts, in which order (`SPEC-0292` v8 behaviour 12).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Lines (0-based, inclusive) counted first: the visible ones with their margin.
    pub focus: Option<(u64, u64)>,
    /// Only the declarations within `focus`.
    pub focus_only: bool,
    /// Declarations already counted for this text, by line and character.
    pub skip: Vec<(u64, u64)>,
}

impl Plan {
    /// The editor's first question: the visible lines `first..=last` and their margin only.
    pub fn visible(first: u64, last: u64) -> Self {
        Self {
            focus: Some((first.saturating_sub(FOCUS_MARGIN), last + FOCUS_MARGIN)),
            focus_only: true,
            skip: Vec::new(),
        }
    }

    /// The editor's second question: the rest of the file nearest to the visible lines first,
    /// without the declarations `skip` already counted.
    pub fn rest(first: u64, last: u64, skip: Vec<(u64, u64)>) -> Self {
        Self {
            focus: Some((first.saturating_sub(FOCUS_MARGIN), last + FOCUS_MARGIN)),
            focus_only: false,
            skip,
        }
    }

    /// Exactly the lines `first..=last` (MCP `from_line`, `to_line`, v8 behaviour 17).
    pub fn lines(first: u64, last: u64) -> Self {
        Self {
            focus: Some((first, last)),
            focus_only: true,
            skip: Vec::new(),
        }
    }

    /// `decls` in the order they are asked about: those within the focus in document order, then
    /// the others by their distance to it; skipped ones left out; at most [`MAX_DECLARATIONS`].
    pub fn order(&self, mut decls: Vec<UsageCount>) -> Vec<UsageCount> {
        decls.retain(|d| !self.skip.contains(&(d.line, d.character)));
        if let Some((first, last)) = self.focus {
            let distance = |line: u64| {
                if line < first {
                    first - line
                } else {
                    line.saturating_sub(last)
                }
            };
            if self.focus_only {
                decls.retain(|d| distance(d.line) == 0);
            }
            decls.sort_by_key(|d| (distance(d.line), d.line, d.character));
        }
        decls.truncate(MAX_DECLARATIONS);
        decls
    }
}

/// The counts of one question and whether its budget sufficed for every chosen declaration
/// (`SPEC-0292` v8 behaviours 12 and 17).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counted {
    /// The counted declarations in document order.
    pub counts: Vec<UsageCount>,
    /// False when the budget ended before every chosen declaration was asked about.
    pub complete: bool,
}

impl Counted {
    /// `counts` in document order, `complete` as given.
    pub fn new(mut counts: Vec<UsageCount>, complete: bool) -> Self {
        counts.sort_by_key(|c| (c.line, c.character));
        Self { counts, complete }
    }
}

/// A usage lens (`SPEC-0292` v8 behaviour 11): its line, the characters it covers and the places of
/// its command — a command that shows references with the places as its third argument. `None`
/// for any other lens and for one not resolved yet.
pub fn usage_lens(lens: &Value) -> Option<(u64, u64, u64, &Value)> {
    let command = &lens["command"];
    let name = command["command"].as_str()?;
    let shows_references = name == "editor.action.showReferences"
        || name.ends_with(".showReferences")
        || name.ends_with(".show.references");
    let places = &command["arguments"][2];
    if !shows_references || !places.is_array() {
        return None;
    }
    let range = &lens["range"];
    Some((
        range["start"]["line"].as_u64()?,
        range["start"]["character"].as_u64()?,
        range["end"]["character"].as_u64().unwrap_or(u64::MAX),
        places,
    ))
}

/// The usage lens of `d` among `lenses` (line, from, to, places): the lens on its line that covers
/// its name, or the only one on its line.
fn lens_of<'a>(lenses: &'a [(u64, u64, u64, Value)], d: &UsageCount) -> Option<&'a Value> {
    let on_line: Vec<_> = lenses.iter().filter(|l| l.0 == d.line).collect();
    on_line
        .iter()
        .find(|l| l.1 <= d.character && d.character <= l.2)
        .or_else(|| (on_line.len() == 1).then(|| &on_line[0]))
        .map(|l| &l.3)
}

/// The usage lenses of the opened file on the lines of `decls`, resolved where the server leaves
/// that for later, within `deadline` (`SPEC-0292` v8 behaviour 11). Empty when the server has no
/// lenses.
async fn usage_lenses(
    o: &Opened,
    decls: &[UsageCount],
    deadline: Instant,
) -> Vec<(u64, u64, u64, Value)> {
    let provider = &o.capabilities["codeLensProvider"];
    if provider.is_null() || provider == &Value::Bool(false) {
        return Vec::new();
    }
    let resolves = provider["resolveProvider"] == true;
    let left = deadline.saturating_duration_since(Instant::now());
    let Ok(lenses) = o
        .ask_bounded(
            "textDocument/codeLens",
            json!({"textDocument": {"uri": o.uri}}),
            left,
            None,
        )
        .await
    else {
        return Vec::new();
    };
    let lines: BTreeSet<u64> = decls.iter().map(|d| d.line).collect();
    let mut wanted: Vec<&Value> = lenses
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| {
            l["range"]["start"]["line"]
                .as_u64()
                .is_some_and(|line| lines.contains(&line))
        })
        .collect();
    // In the plan's order: the lens of the first declaration first.
    let rank = |l: &Value| {
        let line = l["range"]["start"]["line"].as_u64().unwrap_or(u64::MAX);
        decls
            .iter()
            .position(|d| d.line == line)
            .unwrap_or(usize::MAX)
    };
    wanted.sort_by_key(|l| rank(l));
    let mut out = Vec::new();
    for lens in wanted {
        let resolved;
        let lens = if lens["command"].is_object() || !resolves {
            lens
        } else {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match o
                .ask_bounded("codeLens/resolve", lens.clone(), left, None)
                .await
            {
                Ok(r) => {
                    resolved = r;
                    &resolved
                }
                Err(_) => break,
            }
        };
        if let Some((line, from, to, places)) = usage_lens(lens) {
            out.push((line, from, to, places.clone()));
        }
    }
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

/// Usage counts of the declarations of `file` whose content is `text` or the disk content, all of
/// them as far as `wait` allows; declarations without usages are left out where behaviour 2 says so.
pub async fn usage_counts_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<UsageCount>, LspError> {
    Ok(
        usage_counts_planned(root, file, text, wait, &Plan::default())
            .await?
            .counts,
    )
}

/// Usage counts of the declarations of `file` that `plan` chooses, in its order, within the budget
/// `wait` (`SPEC-0292` v8 behaviour 12).
pub async fn usage_counts_planned(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
    plan: &Plan,
) -> Result<Counted, LspError> {
    if crate::ts_structure::is_script(file) {
        return crate::ts_structure::usage_counts_in(root, file, text, wait, plan).await;
    }
    if crate::java_marks::is_java(file) {
        return crate::java_marks::usage_counts_in(root, file, text, wait, plan).await;
    }
    if crate::kotlin_structure::is_kotlin(file) {
        return crate::kotlin_structure::usage_counts_in(root, file, text, wait, plan).await;
    }
    let deadline = Instant::now() + wait;
    let o = Opened::open_text(root, file, text, wait).await?;
    let uri = o.uri.clone();
    let result = async {
        let left = deadline.saturating_duration_since(Instant::now());
        let symbols = o
            .ask_bounded(
                "textDocument/documentSymbol",
                json!({"textDocument": {"uri": uri}}),
                left,
                None,
            )
            .await?;
        let chosen = plan.order(declarations(&symbols));
        let lenses = usage_lenses(&o, &chosen, deadline).await;
        let mut found = Vec::new();
        let mut complete = true;
        let mut asked = 0;
        for mut d in chosen {
            if let Some(places) = lens_of(&lenses, &d) {
                d.count = distinct_places_besides(places, &uri, d.line, d.character);
                d.source = FROM_CODE_LENS;
            } else {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    complete = false;
                    break;
                }
                let references = match o
                    .ask_bounded(
                        "textDocument/references",
                        json!({"textDocument": {"uri": uri},
                            "position": {"line": d.line, "character": d.character},
                            "context": {"includeDeclaration": false}}),
                        left,
                        None,
                    )
                    .await
                {
                    Ok(r) => r,
                    // The budget ended within the question: what was counted stands.
                    Err(LspError::Timeout(..)) if asked > 0 => {
                        complete = false;
                        break;
                    }
                    Err(e) => return Err(e),
                };
                d.count = distinct_places_besides(&references, &uri, d.line, d.character);
                asked += 1;
            }
            if d.count > 0 {
                found.push(d);
            }
        }
        Ok::<_, LspError>(Counted::new(found, complete))
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
        let mut u = UsageCount::new("Item", "struct", 0, 0);
        u.count = 1;
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

    /// SPEC-0292 AK-14 (v8): types and their members and the file's functions; nothing inside a
    /// function, no variables or constants.
    #[test]
    fn members_of_every_language() {
        let tree = json!([
            sym(
                "Shape",
                5,
                0,
                json!([
                    sym("area", 6, 1, json!([sym("tmp", 13, 2, json!([]))])),
                    sym("radius", 8, 4, json!([])),
                    sym("new", 9, 5, json!([])),
                ])
            ),
            sym("main", 12, 8, json!([sym("helper", 12, 9, json!([]))])),
            sym("LIMIT", 14, 12, json!([])),
            sym("cache", 13, 13, json!([sym("inner", 12, 13, json!([]))])),
            // An impl block (kind object) holds methods.
            sym("impl Shape", 19, 15, json!([sym("grow", 6, 16, json!([]))])),
        ]);
        let chosen = declarations(&tree);
        let found: Vec<(&str, &str)> = chosen.iter().map(|d| (d.name.as_str(), d.kind)).collect();
        assert_eq!(
            found,
            [
                ("Shape", "class"),
                ("area", "method"),
                ("radius", "field"),
                ("new", "constructor"),
                ("main", "function"),
                ("grow", "method")
            ]
        );
    }

    /// SPEC-0292 AK-15 (v8): the visible lines and their margin first, then by distance; counted
    /// ones skipped; visible only drops the rest; at most 200.
    #[test]
    fn visible_declarations_come_first() {
        let decls = |lines: &[u64]| -> Vec<UsageCount> {
            lines
                .iter()
                .map(|&l| UsageCount::new(&format!("d{l}"), "method", l, 4))
                .collect()
        };
        let lines = |v: Vec<UsageCount>| v.iter().map(|d| d.line).collect::<Vec<_>>();
        let all = decls(&[0, 10, 50, 100, 130, 200, 400]);
        // Visible 100..=110, margin 20: 80..=130.
        assert_eq!(
            lines(Plan::visible(100, 110).order(all.clone())),
            [100, 130]
        );
        assert_eq!(
            lines(Plan::rest(100, 110, vec![(130, 4)]).order(all.clone())),
            [100, 50, 10, 200, 0, 400]
        );
        assert_eq!(lines(Plan::lines(10, 50).order(all.clone())), [10, 50]);
        // Near the top the margin stops at line 0.
        assert_eq!(lines(Plan::visible(5, 9).order(all.clone())), [0, 10]);
        assert_eq!(
            lines(Plan::default().order(all)),
            [0, 10, 50, 100, 130, 200, 400]
        );
        let many: Vec<u64> = (0..300).collect();
        let cut = Plan::rest(250, 260, Vec::new()).order(decls(&many));
        assert_eq!(cut.len(), MAX_DECLARATIONS);
        assert_eq!(cut[0].line, 230);
        assert!(cut.iter().any(|d| d.line == 299));
    }

    /// SPEC-0292 AK-16 (v8): a lens that shows references gives its places; other lenses none.
    #[test]
    fn reference_lenses_give_counts() {
        let place = |line: u64| {
            json!({"uri": "file:///a.rs", "range": {
            "start": {"line": line, "character": 4}, "end": {"line": line, "character": 9}}})
        };
        let lens = |command: &str, places: Value| {
            json!({
            "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}},
            "command": {"title": "x", "command": command,
                "arguments": ["file:///a.rs", {"line": 3, "character": 7}, places]}})
        };
        let refs = json!([place(8), place(8), place(11)]);
        let shown = lens("rust-analyzer.showReferences", refs.clone());
        let (line, from, to, places) = usage_lens(&shown).unwrap();
        assert_eq!((line, from, to), (3, 7, 12));
        assert_eq!(distinct_places(places), 2);
        assert!(usage_lens(&lens("java.show.references", refs.clone())).is_some());
        assert!(usage_lens(&lens("editor.action.showReferences", refs.clone())).is_some());
        assert!(usage_lens(&lens("rust-analyzer.runSingle", refs.clone())).is_none());
        assert!(usage_lens(&lens("rust-analyzer.showReferences", json!(null))).is_none());
        // Not resolved yet: no command.
        assert!(usage_lens(&json!({"range": {"start": {"line": 3, "character": 7}}})).is_none());
        // The lens of a declaration: the one covering its name, else the only one on its line.
        let lenses = vec![
            (3, 7, 12, json!([1])),
            (5, 0, 3, json!([2])),
            (5, 10, 14, json!([3])),
        ];
        let at =
            |line, character| lens_of(&lenses, &UsageCount::new("x", "struct", line, character));
        assert_eq!(at(3, 30), Some(&json!([1])));
        assert_eq!(at(5, 11), Some(&json!([3])));
        assert_eq!(at(5, 6), None);
        assert_eq!(at(4, 0), None);
    }
}
