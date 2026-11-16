//! F-030 · Detect a file's language: extension → language (SPEC-0004 AK-1…AK-3, SPEC-0006 AK-2/AK-4).

use crate::harness::*;
use serde_json::json;

#[test]
fn extensions_and_special_names_give_lsp_ids() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    for (file, id) in [
        ("a.py", "python"),
        ("a.sql", "sql"),
        ("a.js", "javascript"),
        ("a.ts", "typescript"),
        ("A.java", "java"),
        ("a.rs", "rust"),
        ("Dockerfile", "dockerfile"),
    ] {
        assert_eq!(
            ide.ok("detect_language", json!({"path": file})),
            json!(id),
            "{file}"
        );
    }
    assert_eq!(
        ide.ok("detect_language", json!({"path": "notes.zzz"})),
        json!(null)
    );
}

#[test]
fn files_are_counted_and_filtered_by_language() {
    let p = Project::reference("react-basic");
    let mut ide = Ide::stdio(&p);
    let info = ide.ok("workspace_info", json!({}));
    assert_eq!(info["languages"]["typescript"], 2, "{info}");
    let tsx = ide.ok("list_files", json!({"language": "typescript"}));
    assert_eq!(tsx, json!(["src/StockTable.tsx", "src/main.tsx"]));
}
