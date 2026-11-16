//! F-011 · Undo/redo with groups: a refactoring is undone as one step (SPEC-0003 AK-5/AK-6,
//! SPEC-0025 AK-5, SPEC-0193).

use crate::harness::*;
use serde_json::json;

#[test]
fn typed_text_and_its_undo_and_redo() {
    let p = Project::empty();
    p.write("a.txt", "one\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "editor_keystrokes",
        json!({"path": "a.txt", "keymap": "macos", "carets": [{"line": 1, "column": 4}],
               "steps": [{"text": " two"}, {"keys": "meta Z"}]}),
    );
    assert_eq!(r["text"], "one\n");
    let r = ide.ok(
        "editor_keystrokes",
        json!({"path": "a.txt", "keymap": "macos", "carets": [{"line": 1, "column": 4}],
               "steps": [{"text": " two"}, {"keys": "meta Z"}, {"keys": "meta shift Z"}]}),
    );
    assert_eq!(r["text"], "one two\n");
}

#[test]
fn a_rename_over_several_files_is_undone_in_one_step() {
    let p = Project::reference("python-basic");
    let model = p.read("src/inventory/model.py");
    let test = p.read("test_model.py");
    let mut ide = Ide::stdio(&p);
    let renamed = ide.call(
        "rename_symbol",
        json!({"path": "src/inventory/model.py", "line": 7, "character": 4,
               "new_name": "quantity", "dry_run": false}),
    );
    if lsp_unavailable(&renamed) {
        return;
    }
    renamed.unwrap();
    assert!(p.read("src/inventory/model.py").contains("quantity: int"));
    assert!(p.read("test_model.py").contains(".quantity == 5"));
    ide.ok("undo_refactoring", json!({}));
    assert_eq!(p.read("src/inventory/model.py"), model);
    assert_eq!(p.read("test_model.py"), test);
}
