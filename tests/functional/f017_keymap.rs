//! F-017 · Keymap: standard bindings, changeable by the user (SPEC-0028 AK-4/AK-5, SPEC-0129).

use crate::harness::*;
use serde_json::json;

#[test]
fn a_user_shortcut_replaces_the_default_and_can_be_reset() {
    // Shortcuts are shown as the platform writes them (SPEC-0028): symbols on macOS, words
    // elsewhere; the default of `EditorDuplicate` is ⌘D on macOS and Ctrl+D elsewhere.
    let (custom, default) = if cfg!(target_os = "macos") {
        ("⌥⌘D", "⌘D")
    } else {
        ("Alt+Meta+D", "Ctrl+D")
    };
    let p = Project::empty();
    // The user's own settings are changed only with the `settings` right (SPEC-0053 v8).
    p.grant(&["settings"]);
    let mut ide = Ide::stdio(&p);
    ide.ok(
        "set_shortcut",
        json!({"action": "EditorDuplicate", "shortcuts": ["meta alt D"]}),
    );
    let user = ide.ok(
        "keymap_lookup",
        json!({"keymap": "user", "action": "EditorDuplicate"}),
    );
    assert!(text(&user).contains(custom), "{user}");
    // Stored in the user's settings folder, never in the project: the project gets the line of
    // the agent log (SPEC-0053 v7 behaviour 11) with the `.gitignore` that keeps it out of
    // commits, and nothing else.
    assert!(std::fs::read_dir(&p.config).unwrap().count() > 0);
    let in_project: Vec<String> = std::fs::read_dir(p.path(".idecraft"))
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        in_project
            .iter()
            .all(|name| name == "agent-log.jsonl" || name == ".gitignore"),
        "{in_project:?}"
    );
    ide.ok(
        "set_shortcut",
        json!({"action": "EditorDuplicate", "reset": true}),
    );
    let user = ide.ok(
        "keymap_lookup",
        json!({"keymap": "user", "action": "EditorDuplicate"}),
    );
    assert!(
        text(&user).contains(default) && !text(&user).contains(custom),
        "{user}"
    );
}

#[test]
fn an_unknown_keymap_is_refused() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    ide.refused(
        "keymap_lookup",
        json!({"keymap": "emacs-from-mars", "action": "EditorDuplicate"}),
    );
}
