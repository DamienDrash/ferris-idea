//! F-012 · Multiple carets and column selection: several carets edit at once (SPEC-0025 AK-5/AK-7,
//! SPEC-0130).

use crate::harness::*;
use serde_json::json;

#[test]
fn all_occurrences_get_a_caret_and_the_file_stays_unchanged() {
    let p = Project::reference("python-basic");
    let before = p.read("src/inventory/model.py");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "preview_multi_caret_edit",
        json!({"path": "src/inventory/model.py", "occurrence": "qty", "replacement": "quantity"}),
    );
    assert_eq!(r["carets"], 3);
    let t = text(&r["text"]);
    assert_eq!(t.matches("quantity").count(), 3);
    assert!(!t.contains("qty"));
    assert_eq!(p.read("src/inventory/model.py"), before);
}

#[test]
fn typing_at_three_carets_and_one_undo() {
    let p = Project::empty();
    p.write("a.txt", "a\nb\nc\n");
    let mut ide = Ide::stdio(&p);
    let carets =
        json!([{"line": 1, "column": 2}, {"line": 2, "column": 2}, {"line": 3, "column": 2}]);
    let r = ide.ok(
        "editor_keystrokes",
        json!({"path": "a.txt", "keymap": "macos", "carets": carets, "steps": [{"text": "!"}]}),
    );
    assert_eq!(r["text"], "a!\nb!\nc!\n");
    assert_eq!(r["carets"].as_array().unwrap().len(), 3);
    let r = ide.ok(
        "editor_keystrokes",
        json!({"path": "a.txt", "keymap": "macos", "carets": carets,
               "steps": [{"text": "!"}, {"keys": "meta Z"}]}),
    );
    assert_eq!(r["text"], "a\nb\nc\n");
}

#[test]
fn column_selection_replaces_the_same_columns_on_every_line() {
    let p = Project::empty();
    p.write("cols.txt", "abc\ndefg\nh\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "preview_column_edit",
        json!({"path": "cols.txt", "from_line": 0, "from_column": 1, "to_line": 2, "to_column": 2,
               "replacement": "X"}),
    );
    // A line shorter than the rectangle gets the text at its end.
    assert_eq!(r["text"], "aXc\ndXfg\nhX\n");
}
