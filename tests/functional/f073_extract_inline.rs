//! F-073 · Extract method/variable, inline: server refactorings with a preview (SPEC-0041 v2 AK-1,
//! AK-3, SPEC-0076 v3, IMP-0018).

use crate::harness::*;
use serde_json::json;

#[test]
fn extract_an_expression_into_a_variable() {
    let p = Project::empty();
    p.write(
        "m.c",
        "int add(int a, int b) { return a + b; }\nint main(void) {\n  return add(1, 2) * 3;\n}\n",
    );
    let mut ide = Ide::stdio(&p);
    // Select `add(1, 2)` on line 2 (0-based).
    let at = json!({"path": "m.c", "start_line": 2, "start_character": 9, "end_line": 2, "end_character": 18});
    let actions = ide.call("code_actions", at.clone());
    if lsp_unavailable(&actions) {
        return;
    }
    let actions = actions.unwrap();
    let Some(extract) = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| text(&a["title"]).to_lowercase().contains("extract"))
        .cloned()
    else {
        panic!("an extract refactoring: {actions}");
    };
    let mut apply = at;
    apply["title"] = extract["title"].clone();
    let preview = ide.ok("apply_code_action", apply.clone());
    assert!(
        text(&preview).contains("m.c"),
        "the preview lists the edits: {preview}"
    );
    apply["dry_run"] = json!(false);
    ide.ok("apply_code_action", apply);
    let after = p.read("m.c");
    assert!(after.contains("= add(1, 2)"), "{after}");
}
