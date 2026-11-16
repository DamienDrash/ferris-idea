//! F-010 · Text editor basics: insert, delete, replace, Unicode-safe; editing commands
//! (SPEC-0003 AK-1/AK-2, SPEC-0056, SPEC-0057).

use crate::harness::*;
use serde_json::json;

fn project() -> Project {
    let p = Project::empty();
    p.write("u.py", "a = \"Grüße €\"\nb = 2\n");
    p
}

#[test]
fn typing_and_backspace_count_characters_not_bytes() {
    let p = project();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "editor_keystrokes",
        json!({"path": "u.py", "carets": [{"line": 1, "column": 11}],
               "steps": [{"keys": "BACK_SPACE"}, {"text": "ü"}]}),
    );
    // Column 11 is after `e` of `Grüße`: one character goes, one comes.
    assert_eq!(r["text"], "a = \"Grüßü €\"\nb = 2\n");
    assert_eq!(r["carets"][0]["head"], json!({"line": 1, "column": 11}));
    // The file itself is untouched.
    assert_eq!(p.read("u.py"), "a = \"Grüße €\"\nb = 2\n");
}

#[test]
fn line_commands_duplicate_delete_and_move() {
    let p = project();
    let mut ide = Ide::stdio(&p);
    let carets = json!([{"line": 1, "column": 1}]);
    let dup = ide.ok(
        "editor_action",
        json!({"path": "u.py", "action": "duplicate", "carets": carets}),
    );
    assert_eq!(dup["text"], "a = \"Grüße €\"\na = \"Grüße €\"\nb = 2\n");
    let del = ide.ok(
        "editor_action",
        json!({"path": "u.py", "action": "delete_lines", "carets": carets}),
    );
    assert_eq!(del["text"], "b = 2\n");
    let mv = ide.ok(
        "editor_action",
        json!({"path": "u.py", "action": "move_lines_down", "carets": carets}),
    );
    assert_eq!(mv["text"], "b = 2\na = \"Grüße €\"\n");
    let comment = ide.ok(
        "editor_action",
        json!({"path": "u.py", "action": "toggle_comment", "carets": carets}),
    );
    // The line comment token goes into the first column.
    assert!(text(&comment["text"]).starts_with("#a = "), "{comment}");
}

#[test]
fn replacing_a_selection_by_typing() {
    let p = project();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "editor_keystrokes",
        json!({"path": "u.py",
               "carets": [{"line": 2, "column": 6, "anchor_line": 2, "anchor_column": 5}],
               "steps": [{"text": "42"}]}),
    );
    assert_eq!(r["text"], "a = \"Grüße €\"\nb = 42\n");
}
