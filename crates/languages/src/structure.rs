//! The order, filters and inherited members of the File Structure popup (`SPEC-0079` v4
//! behaviours 7 and 9), free of the UI toolkit. The kind order follows the popup order of the
//! open-source Java structure view of `intellij-community` (logic template, see
//! `docs/legal/PROVENANCE.md`); the code is our own.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::context::is_anonymous;
use crate::insight::{Opened, Symbol, parse_range, uri_to_path};
use crate::lsp::LspError;

/// Most supertypes looked at per file (behaviour 9).
const SUPERTYPE_LIMIT: usize = 20;

/// Where a symbol goes among its siblings: smaller first (behaviour 7).
pub fn kind_weight(s: &Symbol) -> u8 {
    if is_anonymous(&s.name) {
        return 55;
    }
    match s.kind.as_str() {
        "constructor" => 30,
        "method" | "function" | "operator" => 35,
        "property" => 40,
        "field" | "constant" | "enum-member" | "variable" | "key" => 50,
        "class" | "interface" | "enum" | "struct" | "object" | "type-parameter" => 53,
        _ => 60,
    }
}

/// `symbols` without package symbols (jdtls lists the `package` clause); the symbols nested in a
/// package move up one level (`SPEC-0079` v3 behaviour 6).
pub fn without_packages(symbols: Vec<Symbol>) -> Vec<Symbol> {
    // Depths of the packages that enclose the current symbol.
    let mut open: Vec<usize> = Vec::new();
    let mut out = Vec::with_capacity(symbols.len());
    for mut s in symbols {
        while open.last().is_some_and(|&d| d >= s.depth) {
            open.pop();
        }
        if s.kind == "package" {
            open.push(s.depth);
            continue;
        }
        s.depth -= open.len().min(s.depth);
        out.push(s);
    }
    out
}

/// Whether a symbol stands for a lambda.
pub fn is_lambda(name: &str) -> bool {
    name.contains("->") || name.starts_with("lambda") || name.starts_with("<lambda")
}

/// Whether a symbol is a type that can be collapsed (behaviour 8).
pub fn is_type(s: &Symbol) -> bool {
    kind_weight(s) == 53 || kind_weight(s) == 55
}

/// `symbols` (document order, with depths) ordered below each parent by kind, then by name
/// ignoring case, each symbol keeping its children (behaviour 7).
pub fn ordered(symbols: Vec<Symbol>) -> Vec<Symbol> {
    fn order(slice: &[Symbol]) -> Vec<Symbol> {
        let Some(first) = slice.first() else {
            return Vec::new();
        };
        let base = first.depth;
        let mut groups: Vec<&[Symbol]> = Vec::new();
        let mut start = 0;
        for i in 1..=slice.len() {
            if i == slice.len() || slice[i].depth <= base {
                groups.push(&slice[start..i]);
                start = i;
            }
        }
        // The elements of an HTML file keep the order of the document (v7 behaviour 14): the
        // sort is stable and they share one key.
        groups.sort_by_cached_key(|g| {
            let element = g[0].kind == crate::html_css::ELEMENT_KIND;
            (
                kind_weight(&g[0]),
                (!element).then(|| g[0].name.to_lowercase()),
            )
        });
        let mut out = Vec::with_capacity(slice.len());
        for g in groups {
            out.push(g[0].clone());
            out.extend(order(&g[1..]));
        }
        out
    }
    order(&symbols)
}

/// `symbols` without anonymous classes (unless `anonymous`) and lambdas (unless `lambdas`), each
/// with everything below it (behaviour 9).
pub fn filtered(symbols: Vec<Symbol>, anonymous: bool, lambdas: bool) -> Vec<Symbol> {
    let mut out = Vec::with_capacity(symbols.len());
    let mut hidden_below: Option<usize> = None;
    for s in symbols {
        if let Some(d) = hidden_below {
            if s.depth > d {
                continue;
            }
            hidden_below = None;
        }
        if (!anonymous && is_anonymous(&s.name)) || (!lambdas && is_lambda(&s.name)) {
            hidden_below = Some(s.depth);
            continue;
        }
        out.push(s);
    }
    out
}

/// The name without parameters, for comparing members (`apply(int)` → `apply`).
fn bare(name: &str) -> &str {
    name.split('(').next().unwrap_or(name).trim()
}

/// `symbols` of `file` (content `text`, else the saved file) with the inherited members of each
/// type inserted below it, from the server's supertypes in project files (behaviour 9).
pub async fn with_inherited_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    symbols: Vec<Symbol>,
    wait: Duration,
) -> Result<Vec<Symbol>, LspError> {
    let started = Instant::now();
    let left = || wait.saturating_sub(started.elapsed());
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // Inherited rows go after the whole subtree of their type: (last index, type depth, rows).
    let mut inserts: Vec<(usize, usize, Vec<Symbol>)> = Vec::new();
    let mut failed: Option<LspError> = None;
    for (i, s) in symbols.iter().enumerate() {
        if !is_type(s) || is_anonymous(&s.name) || left().is_zero() {
            continue;
        }
        let end = (i + 1..symbols.len())
            .find(|&j| symbols[j].depth <= s.depth)
            .unwrap_or(symbols.len());
        let own: Vec<String> = symbols[i + 1..end]
            .iter()
            .filter(|c| c.depth == s.depth + 1)
            .map(|c| bare(&c.name).to_owned())
            .collect();
        match inherited(&mut o, &root_canonical, s, &own, left()).await {
            Ok(members) if !members.is_empty() => inserts.push((end - 1, s.depth, members)),
            Ok(_) => {}
            Err(e) => {
                failed.get_or_insert(e);
            }
        }
    }
    o.close().await;
    if inserts.is_empty()
        && let Some(e) = failed
    {
        return Err(e);
    }
    // Deeper types first where subtrees end together, so the depths stay a tree.
    inserts.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out = Vec::with_capacity(symbols.len());
    let mut pending = inserts.into_iter().peekable();
    for (i, s) in symbols.into_iter().enumerate() {
        out.push(s);
        while let Some((_, _, rows)) = pending.next_if(|(at, _, _)| *at == i) {
            out.extend(rows);
        }
    }
    Ok(out)
}

/// The members of the supertypes of `ty` in project files, those `own` declares left out.
async fn inherited(
    o: &mut Opened,
    root: &Path,
    ty: &Symbol,
    own: &[String],
    wait: Duration,
) -> Result<Vec<Symbol>, LspError> {
    let started = Instant::now();
    let left = || wait.saturating_sub(started.elapsed());
    let items = o
        .ask_bounded(
            "textDocument/prepareTypeHierarchy",
            o.position(ty.line, ty.character),
            left(),
            None,
        )
        .await?;
    let Some(item) = items.as_array().and_then(|a| a.first()).cloned() else {
        return Ok(Vec::new());
    };
    let mut queue: Vec<Value> = vec![item];
    let mut seen: Vec<String> = Vec::new();
    let mut taken: Vec<String> = own.to_vec();
    let mut out = Vec::new();
    while let Some(next) = queue.pop() {
        if seen.len() > SUPERTYPE_LIMIT || left().is_zero() {
            break;
        }
        let supers = o
            .ask_bounded(
                "typeHierarchy/supertypes",
                json!({"item": next}),
                left(),
                None,
            )
            .await?;
        for sup in supers.as_array().into_iter().flatten() {
            let uri = sup["uri"].as_str().unwrap_or_default().to_owned();
            let key = format!(
                "{uri}#{}",
                sup.pointer("/selectionRange/start/line")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            );
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let Some(path) = uri_to_path(&uri) else {
                continue;
            };
            let path = path.canonicalize().unwrap_or(path);
            if !path.starts_with(root) {
                continue;
            }
            queue.push(sup.clone());
            let Ok(symbols) = o.symbols_of(&path, left()).await else {
                continue;
            };
            let Some(at) = sup
                .get("selectionRange")
                .and_then(|r| parse_range(r, &uri, &mut o.files))
            else {
                continue;
            };
            let name = sup["name"].as_str().unwrap_or_default().to_owned();
            let Some(k) = symbols
                .iter()
                .position(|s| s.line == at.start_line && s.character == at.start_character)
            else {
                continue;
            };
            let depth = symbols[k].depth;
            for m in symbols[k + 1..]
                .iter()
                .take_while(|m| m.depth > depth)
                .filter(|m| m.depth == depth + 1 && !is_type(m) && m.kind != "constructor")
            {
                let b = bare(&m.name).to_owned();
                if taken.contains(&b) {
                    continue;
                }
                taken.push(b);
                let mut m = m.clone();
                m.depth = ty.depth + 1;
                m.inherited_from = Some(name.clone());
                m.path = Some(path.clone());
                out.push(m);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, kind: &str, depth: usize) -> Symbol {
        Symbol {
            name: name.into(),
            kind: kind.into(),
            depth,
            ..Symbol::default()
        }
    }

    fn names(s: &[Symbol]) -> Vec<(String, usize)> {
        s.iter().map(|s| (s.name.clone(), s.depth)).collect()
    }

    /// SPEC-0079 v4 AK-6: methods before nested types, each by name; children stay below.
    #[test]
    fn members_before_nested_types() {
        let doc = vec![
            sym("Discounts", "class", 0),
            sym("None", "class", 1),
            sym("apply(int)", "method", 2),
            sym("Half", "class", 1),
            sym("apply(int)", "method", 2),
            sym("priceOf(Item, Discount)", "method", 1),
            sym("bothPrices(Item)", "method", 1),
            sym("count", "field", 1),
            sym("Discounts()", "constructor", 1),
        ];
        let shown: Vec<(String, usize)> = names(&ordered(doc));
        let want: Vec<(String, usize)> = [
            ("Discounts", 0),
            ("Discounts()", 1),
            ("bothPrices(Item)", 1),
            ("priceOf(Item, Discount)", 1),
            ("count", 1),
            ("Half", 1),
            ("apply(int)", 2),
            ("None", 1),
            ("apply(int)", 2),
        ]
        .iter()
        .map(|(n, d)| (n.to_string(), *d))
        .collect();
        assert_eq!(shown, want);
        // C functions by name.
        let c = ordered(vec![sym("main", "function", 0), sym("calc", "function", 0)]);
        assert_eq!(names(&c)[0].0, "calc");
    }

    /// SPEC-0079 v7 AK-9 (SPEC-0042 v17 behaviour 31): the elements of an HTML file stay in the
    /// order of the document, below each parent.
    #[test]
    fn elements_keep_document_order() {
        let doc = crate::html_css::structure(
            "<html><head><title>t</title><meta></head>\
             <body><main id=\"z\"><p>a</p><b>b</b></main><div class=\"a\"></div></body></html>",
        );
        let before = names(&doc);
        assert_eq!(
            before
                .iter()
                .map(|(n, d)| format!("{d}{n}"))
                .collect::<Vec<_>>(),
            [
                "0html", "1head", "2title", "2meta", "1body", "2main#z", "3p", "3b", "2div.a"
            ]
        );
        assert_eq!(names(&ordered(doc.clone())), before);
        assert_eq!(
            names(&filtered(without_packages(doc), false, false)),
            before
        );
    }

    /// SPEC-0079 v4 AK-6.
    #[test]
    fn anonymous_and_lambdas_are_filtered() {
        let doc = vec![
            sym("Extra", "class", 0),
            sym("task", "field", 1),
            sym("new Runnable() {...}", "class", 2),
            sym("run()", "method", 3),
            sym("twice", "field", 1),
            sym("x -> x * 2", "function", 2),
            sym("work()", "method", 1),
        ];
        let hidden = names(&filtered(doc.clone(), false, false));
        assert_eq!(
            hidden.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["Extra", "task", "twice", "work()"]
        );
        let all = filtered(doc.clone(), true, true);
        assert_eq!(all.len(), doc.len());
        let anonymous = names(&filtered(doc, true, false));
        assert!(anonymous.iter().any(|(n, _)| n == "run()"));
        assert!(!anonymous.iter().any(|(n, _)| n.contains("->")));
        assert_eq!(bare("apply(int)"), "apply");
    }
}
