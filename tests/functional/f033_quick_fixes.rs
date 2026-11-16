//! F-033 · Quick fixes: correction proposals at errors (SPEC-0041 AK-1/AK-3, SPEC-0076, SPEC-0125).

use crate::harness::*;
use serde_json::json;

#[test]
fn a_missing_semicolon_is_fixed_by_the_preferred_action() {
    if !on_path("clangd") {
        skip("clangd not installed");
        return;
    }
    let p = Project::empty();
    p.write("m.c", "int main(void) {\n  int x = 1\n  return x;\n}\n");
    let mut ide = Ide::stdio(&p);
    let diags = ide.call(
        "file_diagnostics",
        json!({"path": "m.c", "timeout_seconds": 120}),
    );
    if lsp_unavailable(&diags) {
        return;
    }
    let d = diags.unwrap()[0].clone();
    let at = json!({"path": "m.c", "start_line": d["start_line"], "start_character": d["start_character"],
                    "end_line": d["end_line"], "end_character": d["end_character"]});
    let actions = ide.ok("code_actions", at.clone());
    let fix = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["preferred"] == true)
        .unwrap_or_else(|| panic!("a preferred fix: {actions}"))
        .clone();
    let mut unknown = at.clone();
    unknown["title"] = json!("no such action");
    let reason = ide.refused("apply_code_action", unknown);
    assert!(
        reason.contains(&text(&fix["title"])),
        "names the available titles: {reason}"
    );
    let mut apply = at;
    apply["title"] = fix["title"].clone();
    apply["dry_run"] = json!(false);
    ide.ok("apply_code_action", apply);
    assert!(p.read("m.c").contains("int x = 1;"), "{}", p.read("m.c"));
}

#[test]
fn an_own_inspection_fix_removes_an_unused_import() {
    let p = Project::reference("java-basic");
    p.write(
        "src/main/java/lab/inventory/U.java",
        "package lab.inventory;\nimport java.util.Map;\nclass U {}\n",
    );
    let mut ide = Ide::stdio(&p);
    let preview = ide.ok(
        "fix_inspection",
        json!({"path": "src/main/java/lab/inventory/U.java", "line": 1, "column": 0, "id": "java.unused-import"}),
    );
    assert!(
        p.read("src/main/java/lab/inventory/U.java")
            .contains("import java.util.Map"),
        "{preview}"
    );
    ide.ok(
        "fix_inspection",
        json!({"path": "src/main/java/lab/inventory/U.java", "line": 1, "column": 0,
               "id": "java.unused-import", "dry_run": false}),
    );
    assert!(
        !p.read("src/main/java/lab/inventory/U.java")
            .contains("import")
    );
}
