//! Optimize Imports for JavaScript, TypeScript, Vue and Svelte files (`SPEC-0123` v4
//! behaviour 6): the language server's organize-imports code actions on the editor's text. ESLint
//! and Prettier of the project are not run here.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use tree_sitter::Node;

use crate::LanguageId;
use crate::actions::action_edits;
use crate::insight::Opened;
use crate::lsp::{LspError, file_uri};
use crate::python_tools::ImportCleanup;
use crate::rename::{apply_edits, file_edits};
use crate::syntax::SyntaxDocument;

/// Removes the unused imports.
const REMOVE_UNUSED: &str = "source.removeUnusedImports";
/// Sorts and combines the imports.
const SORT: &str = "source.sortImports";
/// Everything the server understands by organizing imports.
const ORGANIZE: &str = "source.organizeImports";

/// The language of `file` when Optimize Imports goes through its language server.
fn language(file: &Path) -> Option<LanguageId> {
    LanguageId::detect(file).filter(|l| {
        matches!(
            l,
            LanguageId::JavaScript | LanguageId::TypeScript | LanguageId::Vue | LanguageId::Svelte
        )
    })
}

/// Whether Optimize Imports of `file` is the language server's: `.ts .tsx .mts .cts .js .jsx
/// .mjs .cjs .vue .svelte`.
pub fn applies(file: &Path) -> bool {
    language(file).is_some()
}

/// Whether `kind` is `base` or a kind below it (`source.sortImports.ts` below
/// `source.sortImports`).
fn within(kind: &str, base: &str) -> bool {
    kind.strip_prefix(base)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// The code-action kinds to ask a server with `capabilities` for, in order, each on the result of
/// the one before: removing unused imports and sorting where the server announces both, else its
/// `source.organizeImports` (typescript-language-server 6 only sorts and combines for that kind).
pub fn steps(capabilities: &Value) -> Result<&'static [&'static str], String> {
    let provider = &capabilities["codeActionProvider"];
    if provider.is_null() || provider == &Value::Bool(false) {
        return Err(
            "The language server offers no code actions, so it cannot optimize imports".into(),
        );
    }
    let Some(kinds) = provider["codeActionKinds"].as_array() else {
        return Ok(&[ORGANIZE]);
    };
    let offers = |base: &str| {
        kinds
            .iter()
            .filter_map(Value::as_str)
            .any(|k| within(k, base))
    };
    if offers(REMOVE_UNUSED) && offers(SORT) {
        Ok(&[REMOVE_UNUSED, SORT])
    } else if offers(ORGANIZE) {
        Ok(&[ORGANIZE])
    } else {
        Err("The language server offers no organize-imports action".into())
    }
}

fn word<'a>(n: Node, text: &'a str) -> &'a str {
    text.get(n.byte_range()).unwrap_or_default()
}

/// The names the import statement part `n` binds.
fn bound(n: Node, text: &str, out: &mut Vec<String>) {
    match n.kind() {
        "import_specifier" => {
            if let Some(name) = n
                .child_by_field_name("alias")
                .or_else(|| n.child_by_field_name("name"))
            {
                out.push(word(name, text).to_owned());
            }
        }
        "identifier" => out.push(word(n, text).to_owned()),
        "string" => {}
        _ => {
            let mut cursor = n.walk();
            for c in n.children(&mut cursor) {
                bound(c, text, out);
            }
        }
    }
}

fn statements(n: Node, text: &str, out: &mut Vec<String>) {
    let mut cursor = n.walk();
    for c in n.children(&mut cursor) {
        if c.kind() == "import_statement" {
            bound(c, text, out);
        } else {
            statements(c, text, out);
        }
    }
}

/// The contents of the `<script>` blocks of a Vue or Svelte file.
fn script_blocks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("<script") {
        let Some(start) = rest[open..].find('>').map(|i| open + i + 1) else {
            break;
        };
        let end = rest[start..]
            .find("</script")
            .map_or(rest.len(), |i| start + i);
        out.push(&rest[start..end]);
        rest = &rest[end..];
    }
    out
}

/// The names the import statements of `text` (the content of `file`) bind, in document order:
/// default and namespace imports, named imports by their alias if they have one, `import x =
/// require(…)`; for Vue and Svelte files those of the script blocks.
pub fn import_names(file: &Path, text: &str) -> Vec<String> {
    let lower = file.to_string_lossy().to_ascii_lowercase();
    let (grammar, tsx, parts) = match language(file) {
        Some(LanguageId::JavaScript) => (LanguageId::JavaScript, false, vec![text]),
        Some(LanguageId::TypeScript) => {
            (LanguageId::TypeScript, lower.ends_with(".tsx"), vec![text])
        }
        Some(_) => (LanguageId::TypeScript, false, script_blocks(text)),
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for part in parts {
        if let Ok(doc) = SyntaxDocument::new(grammar, tsx, part) {
            statements(doc.tree().root_node(), part, &mut out);
        }
    }
    out
}

/// The names of `before` that `after` no longer imports, in the order of `before`.
fn names_removed(file: &Path, before: &str, after: &str) -> Vec<String> {
    let mut kept = import_names(file, after);
    import_names(file, before)
        .into_iter()
        .filter(|name| match kept.iter().position(|k| k == name) {
            Some(i) => {
                kept.swap_remove(i);
                false
            }
            None => true,
        })
        .collect()
}

/// One line that says what the clean-up did (`changed`: whether the text differs), for the status
/// bar.
pub fn summary(done: &ImportCleanup, changed: bool) -> String {
    match (done.removed.len(), changed) {
        (0, false) => "Imports are already optimized".to_owned(),
        (0, true) => format!("Imports sorted ({})", done.by),
        (1, _) => format!(
            "Removed 1 unused import and sorted the imports ({})",
            done.by
        ),
        (n, _) => format!(
            "Removed {n} unused imports and sorted the imports ({})",
            done.by
        ),
    }
}

/// Asks for the action of `kind` over the whole opened text and returns the text it makes;
/// `None` when the server has no such action now or the action changes nothing.
async fn step(o: &mut Opened, kind: &str) -> Result<Option<String>, LspError> {
    let last_line = o.text.matches('\n').count() as u64;
    let last_character = o
        .text
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count() as u64;
    let end = o.position(last_line, last_character)["position"].clone();
    let answer = o
        .ask(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": o.uri},
                "range": {"start": {"line": 0, "character": 0}, "end": end},
                "context": {"diagnostics": [], "only": [kind]},
            }),
        )
        .await?;
    let Some(action) = answer
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["kind"].as_str().is_some_and(|k| within(k, kind)))
    else {
        return Ok(None);
    };
    let title = action["title"].as_str().unwrap_or(kind).to_owned();
    if let Some(reason) = action["disabled"]["reason"].as_str() {
        return Err(LspError::Protocol(format!("{title} is disabled: {reason}")));
    }
    let edits = action_edits(o, action, &title).await?;
    if edits.is_empty() {
        return Ok(None);
    }
    let mut text = None;
    for file in file_edits(o, &edits)? {
        if file_uri(&file.path) != o.uri {
            return Err(LspError::Protocol(format!(
                "{title} would change another file ({}); nothing was changed",
                file.path.display()
            )));
        }
        text = Some(apply_edits(&o.text, &file.edits).map_err(LspError::Protocol)?);
    }
    Ok(text)
}

/// The text after every step of the opened file's server.
async fn organized(o: &mut Opened, file: &Path, wait: Duration) -> Result<String, LspError> {
    let kinds = steps(&o.capabilities).map_err(LspError::Protocol)?;
    let mut text = o.text.clone();
    for kind in kinds {
        if text != o.text {
            o.set_text(file, &text, wait).await?;
        }
        if let Some(new) = step(o, kind).await? {
            text = new;
        }
    }
    Ok(text)
}

/// Optimize Imports of `file` with content `text` (the editor's text, saved or not) through its
/// language server: unused imports removed, the rest sorted and combined the way the server does
/// it. Nothing is written. Without a server the error says which one is missing.
pub async fn organize_imports_in(
    root: &Path,
    file: &Path,
    text: &str,
    wait: Duration,
) -> Result<ImportCleanup, String> {
    let language = language(file).ok_or_else(|| {
        "Optimize Imports through the language server works in JavaScript, TypeScript, Vue and Svelte files"
            .to_owned()
    })?;
    let message = |e: LspError| match e {
        LspError::NoServer(why) => crate::servers::no_server_message(language, &why),
        LspError::Protocol(m) => m,
        e => format!("Optimize Imports needs the language server, which failed: {e}"),
    };
    let mut o = Opened::open_text(root, file, Some(text), wait)
        .await
        .map_err(message)?;
    let result = organized(&mut o, file, wait).await;
    // A running server keeps the text it was asked about: applying the result is the caller's.
    if o.text != text {
        let _ = o.set_text(file, text, wait).await;
    }
    o.close().await;
    let new = result.map_err(message)?;
    Ok(ImportCleanup {
        removed: names_removed(file, text, &new),
        left: Vec::new(),
        by: "language server",
        sorted: true,
        text: new,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0123/AK-9: remove-then-sort where both kinds are announced (also below the kind, as
    /// typescript-language-server names them), else organize; a server without code actions or
    /// without such a kind is said.
    #[test]
    fn steps_follow_the_announced_kinds() {
        let kinds = |k: &[&str]| json!({"codeActionProvider": {"codeActionKinds": k}});
        // typescript-language-server 6.0.2.
        let ts = kinds(&[
            "source.fixAll.ts",
            "source.removeUnused.ts",
            "source.addMissingImports.ts",
            "quickfix",
            "source.organizeImports.ts",
            "source.removeUnusedImports.ts",
            "source.sortImports.ts",
            "refactor",
        ]);
        assert_eq!(steps(&ts).unwrap(), [REMOVE_UNUSED, SORT]);
        // svelte-language-server 0.18.6.
        let svelte = kinds(&[
            "quickfix",
            "source.organizeImports",
            "source.sortImports",
            "source.addMissingImports",
            "source.removeUnusedImports",
            "refactor",
        ]);
        assert_eq!(steps(&svelte).unwrap(), [REMOVE_UNUSED, SORT]);
        // @vue/language-server 2.2.12.
        let vue = kinds(&[
            "",
            "quickfix",
            "refactor",
            "source",
            "source.organizeImports",
        ]);
        assert_eq!(steps(&vue).unwrap(), [ORGANIZE]);
        // `source.removeUnused.ts` is not below `source.removeUnusedImports`, nor the other way.
        assert_eq!(
            steps(&kinds(&[
                "source.removeUnused.ts",
                "source.sortImports",
                "source.organizeImports"
            ]))
            .unwrap(),
            [ORGANIZE]
        );
        assert_eq!(
            steps(&json!({"codeActionProvider": true})).unwrap(),
            [ORGANIZE]
        );
        assert!(steps(&kinds(&["quickfix", "refactor"])).is_err());
        assert!(steps(&json!({})).is_err());
        assert!(steps(&json!({"codeActionProvider": false})).is_err());
    }

    /// SPEC-0123/AK-9: every form of import binds its names; what is gone between two texts.
    #[test]
    fn removed_names_between_two_texts() {
        let ts = Path::new("a.ts");
        let before = "import def, {a, b as c, type T} from \"./m\"\nimport * as ns from 'n'\nimport type {U} from \"./u\"\nimport fs = require(\"fs\")\nimport \"./side\"\nexport const x = a\n";
        assert_eq!(
            import_names(ts, before),
            ["def", "a", "c", "T", "ns", "U", "fs"]
        );
        let after = "import { a } from \"./m\"\nimport \"./side\"\nexport const x = a\n";
        assert_eq!(
            names_removed(ts, before, after),
            ["def", "c", "T", "ns", "U", "fs"]
        );
        assert!(names_removed(ts, after, after).is_empty());
        // The same name imported twice and removed once.
        assert_eq!(
            names_removed(
                ts,
                "import {a} from \"x\"\nimport {a} from \"y\"\n",
                "import { a } from \"x\"\n"
            ),
            ["a"]
        );
        // JSX in a `.tsx` and a `.jsx` file does not hide the imports.
        let jsx = "import React, {useState} from \"react\"\nexport const A = () => <div>{useState(0)[0]}</div>\n";
        assert_eq!(import_names(Path::new("A.tsx"), jsx), ["React", "useState"]);
        assert_eq!(import_names(Path::new("A.jsx"), jsx), ["React", "useState"]);
        assert_eq!(
            import_names(Path::new("a.mjs"), "import {b} from \"./b.mjs\"\n"),
            ["b"]
        );
        assert!(import_names(Path::new("a.py"), "import os\n").is_empty());
    }

    /// SPEC-0123/AK-9: Vue and Svelte files give the imports of their script blocks.
    #[test]
    fn names_of_script_blocks() {
        let vue = "<script lang=\"ts\">\nimport {x} from \"./x\"\n</script>\n<script setup lang=\"ts\">\nimport {c, b} from \"./util\"\nconst n = b\n</script>\n\n<template>\n  <h1>import {no} from \"./no\"</h1>\n</template>\n";
        assert_eq!(import_names(Path::new("A.vue"), vue), ["x", "c", "b"]);
        let svelte =
            "<script lang=\"ts\">\n  import {a} from \"./util\"\n</script>\n\n<h1>{a}</h1>\n";
        assert_eq!(import_names(Path::new("A.svelte"), svelte), ["a"]);
        assert_eq!(
            names_removed(
                Path::new("A.vue"),
                vue,
                &vue.replace("import {c, b}", "import { b }")
            ),
            ["c"]
        );
    }

    /// SPEC-0123/AK-9: the status lines.
    #[test]
    fn summaries() {
        let done = |removed: &[&str]| ImportCleanup {
            text: String::new(),
            removed: removed.iter().map(|s| (*s).to_owned()).collect(),
            left: Vec::new(),
            by: "language server",
            sorted: true,
        };
        assert_eq!(summary(&done(&[]), false), "Imports are already optimized");
        assert_eq!(
            summary(&done(&[]), true),
            "Imports sorted (language server)"
        );
        assert_eq!(
            summary(&done(&["a"]), true),
            "Removed 1 unused import and sorted the imports (language server)"
        );
        assert_eq!(
            summary(&done(&["a", "b"]), true),
            "Removed 2 unused imports and sorted the imports (language server)"
        );
    }

    #[test]
    fn files_it_applies_to() {
        for name in [
            "a.ts", "a.tsx", "a.mts", "a.cts", "a.js", "a.jsx", "a.mjs", "a.cjs", "A.vue",
            "A.svelte",
        ] {
            assert!(applies(Path::new(name)), "{name}");
        }
        for name in ["a.py", "A.java", "a.kt", "a.json", "a.html"] {
            assert!(!applies(Path::new(name)), "{name}");
        }
    }
}
