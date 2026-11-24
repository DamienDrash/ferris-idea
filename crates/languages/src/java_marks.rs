//! Java usage counts at members and gutter marks (`SPEC-0292` v5, `SPEC-0320` v2): document symbols
//! and references of jdtls for the counts, its type hierarchy with the syntax tree of each project
//! file for what a member overrides or is overridden by.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::insight::{Location, Opened, Range};
use crate::java_structure::{
    TypeDecl, TypeKind, item_key, item_node, node_type, prepare, related, types_in,
};
use crate::lsp::LspError;
use crate::ts_structure::{DeclarationMark, MarkKind, MarkTarget};
use crate::usages::{Counted, Plan, UsageCount, distinct_places};

/// At most this many related types followed from one type.
const MAX_RELATED: usize = 500;

/// Whether `path` is a Java source file.
pub fn is_java(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("java")
}

/// The declarations of a jdtls `documentSymbol` answer that get a count: types, methods,
/// constructors, fields and enum constants. jdtls names a method `area()`; the name stops at `(`.
/// Anonymous classes and initialisers have no usable name and are left out.
pub fn counted_declarations(symbols: &Value) -> Vec<UsageCount> {
    fn kind(k: Option<u64>) -> &'static str {
        match k {
            Some(5) => "class",
            Some(6) => "method",
            Some(8) => "field",
            Some(9) => "constructor",
            Some(10) => "enum",
            Some(11) => "interface",
            Some(22) => "enum member",
            Some(23) => "record",
            _ => "",
        }
    }
    fn walk(list: &Value, out: &mut Vec<UsageCount>) {
        for s in list.as_array().into_iter().flatten() {
            let name = s["name"].as_str().unwrap_or_default();
            let name = name.split('(').next().unwrap_or_default().trim();
            let k = kind(s["kind"].as_u64());
            let at = if s["selectionRange"].is_object() {
                &s["selectionRange"]["start"]
            } else {
                &s["range"]["start"]
            };
            if !k.is_empty()
                && !name.is_empty()
                && !name.contains([' ', '<', '{'])
                && let (Some(line), Some(character)) =
                    (at["line"].as_u64(), at["character"].as_u64())
            {
                out.push(UsageCount::new(name, k, line, character));
            }
            walk(&s["children"], out);
        }
    }
    let mut out = Vec::new();
    walk(symbols, &mut out);
    out.sort_by_key(|u| (u.line, u.character));
    out
}

/// A project type reached through the type hierarchy, with its place.
struct Reached {
    path: PathBuf,
    decl: TypeDecl,
}

/// The project types above (`up`) or below `item`, nearest first. Types outside the project end
/// the walk there.
async fn reach(
    o: &mut Opened,
    item: &Value,
    up: bool,
    project: &Path,
    deadline: Instant,
) -> Result<Vec<Reached>, LspError> {
    let mut seen = HashSet::from([item_key(item)]);
    let mut queue: VecDeque<Value> = related(o, item, up).await?.into();
    let mut out = Vec::new();
    while let Some(next) = queue.pop_front() {
        if Instant::now() >= deadline || out.len() >= MAX_RELATED {
            break;
        }
        if !seen.insert(item_key(&next)) {
            continue;
        }
        let node = item_node(o, &next);
        if !node.location.path.starts_with(project) {
            continue;
        }
        if let Some(decl) = node_type(&node, &|_: &Path| None) {
            out.push(Reached {
                path: node.location.path.clone(),
                decl,
            });
        }
        queue.extend(related(o, &next, up).await?);
    }
    Ok(out)
}

fn project_root(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

fn name_range(line: usize, column: usize, name: &str) -> Range {
    Range {
        start_line: line as u64,
        start_character: column as u64,
        end_line: line as u64,
        end_character: (column + name.chars().count()) as u64,
    }
}

fn type_target(r: &Reached) -> MarkTarget {
    MarkTarget {
        name: r.decl.name.clone(),
        location: Location {
            path: r.path.clone(),
            range: name_range(r.decl.line, r.decl.column, &r.decl.name),
        },
    }
}

fn method_target(r: &Reached, name: &str, params: &[String]) -> Option<MarkTarget> {
    let m = r.decl.method(name, params)?;
    Some(MarkTarget {
        name: r.decl.name.clone(),
        location: Location {
            path: r.path.clone(),
            range: name_range(m.line, m.column, &m.name),
        },
    })
}

/// The gutter marks of a Java file (`SPEC-0320` v2 behaviours 17–18).
pub async fn declaration_marks_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<DeclarationMark>, LspError> {
    // Kotlin: from the syntax trees of the project (`SPEC-0320` v3 behaviour 19).
    if crate::kotlin_structure::is_kotlin(file) {
        return Ok(crate::kotlin_structure::declaration_marks_in(
            root, file, text,
        ));
    }
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = marks(&mut o, &project_root(root), Instant::now() + wait).await;
    o.close().await;
    result
}

async fn marks(
    o: &mut Opened,
    project: &Path,
    deadline: Instant,
) -> Result<Vec<DeclarationMark>, LspError> {
    let mut out = Vec::new();
    for t in types_in(&o.text) {
        if Instant::now() >= deadline {
            break;
        }
        let Some(item) = prepare(o, t.line as u64, t.column as u64).await? else {
            continue;
        };
        let ups = reach(o, &item, true, project, deadline).await?;
        let downs = reach(o, &item, false, project, deadline).await?;
        if !downs.is_empty() {
            out.push(DeclarationMark {
                line: t.line as u64,
                character: t.column as u64,
                name: t.name.clone(),
                kind: MarkKind::Inherited,
                targets: downs.iter().map(type_target).collect(),
            });
        }
        for m in t.methods.iter().filter(|m| m.overridable) {
            let holders = |list: &[Reached]| -> Vec<MarkTarget> {
                list.iter()
                    .filter(|r| {
                        r.decl
                            .method(&m.name, &m.params)
                            .is_some_and(|x| x.overridable)
                    })
                    .filter_map(|r| method_target(r, &m.name, &m.params))
                    .collect()
            };
            let above = holders(&ups);
            if let Some(first) = ups.iter().find(|r| {
                r.decl
                    .method(&m.name, &m.params)
                    .is_some_and(|x| x.overridable)
            }) {
                let top = first.decl.method(&m.name, &m.params);
                let implements =
                    first.decl.kind == TypeKind::Interface || top.is_some_and(|x| x.is_abstract);
                out.push(DeclarationMark {
                    line: m.line as u64,
                    character: m.column as u64,
                    name: m.name.clone(),
                    kind: if implements {
                        MarkKind::Implements
                    } else {
                        MarkKind::Overrides
                    },
                    targets: above,
                });
            }
            let below = holders(&downs);
            if !below.is_empty() {
                out.push(DeclarationMark {
                    line: m.line as u64,
                    character: m.column as u64,
                    name: m.name.clone(),
                    kind: if t.kind == TypeKind::Interface || m.is_abstract {
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
    Ok(out)
}

/// Usage counts of a Java file: types and members, unused ones too, with inheritors at types
/// (`SPEC-0292` v5); those `plan` chooses, in its order, within `wait` (v8 behaviour 12).
pub async fn usage_counts_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
    plan: &Plan,
) -> Result<Counted, LspError> {
    let deadline = Instant::now() + wait;
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = counts(&mut o, &project_root(root), deadline, plan).await;
    o.close().await;
    result
}

async fn counts(
    o: &mut Opened,
    project: &Path,
    deadline: Instant,
    plan: &Plan,
) -> Result<Counted, LspError> {
    let uri = o.uri.clone();
    let symbols = o
        .ask(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .await?;
    let mut found = Vec::new();
    let mut complete = true;
    for mut d in plan.order(counted_declarations(&symbols)) {
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
            // The budget ended within the question: what was counted stands (v8 behaviour 12).
            Err(LspError::Timeout(..)) if !found.is_empty() => {
                complete = false;
                break;
            }
            Err(e) => return Err(e),
        };
        d.count = distinct_places(&references);
        if matches!(d.kind, "class" | "interface" | "enum" | "record")
            && let Some(item) = prepare(o, d.line, d.character).await?
        {
            d.inheritors = reach(o, &item, false, project, deadline).await?.len();
        }
        found.push(d);
    }
    Ok(Counted::new(found, complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0292 AK-11 (v5): members too, jdtls' method names cut at `(`, no anonymous classes.
    #[test]
    fn counted_declarations_of_a_java_file() {
        let sym = |name: &str, kind: u64, line: u64, children: Value| {
            json!({"name": name, "kind": kind,
                "selectionRange": {"start": {"line": line, "character": 4}},
                "range": {"start": {"line": line, "character": 0}},
                "children": children})
        };
        let tree = json!([sym(
            "Shape",
            5,
            2,
            json!([
                sym("radius", 8, 3, json!([])),
                sym("Shape(double)", 9, 4, json!([])),
                sym("area()", 6, 5, json!([])),
                sym("new Runnable() {...}", 5, 6, json!([])),
                sym("Kind", 10, 8, json!([sym("A", 22, 9, json!([]))])),
            ])
        )]);
        let d = counted_declarations(&tree);
        let names: Vec<(&str, &str, u64)> = d
            .iter()
            .map(|u| (u.name.as_str(), u.kind, u.line))
            .collect();
        assert_eq!(
            names,
            [
                ("Shape", "class", 2),
                ("radius", "field", 3),
                ("Shape", "constructor", 4),
                ("area", "method", 5),
                ("Kind", "enum", 8),
                ("A", "enum member", 9),
            ]
        );
    }

    #[test]
    fn java_files() {
        assert!(is_java(Path::new("a/B.java")));
        assert!(!is_java(Path::new("a/B.kt")));
    }
}
