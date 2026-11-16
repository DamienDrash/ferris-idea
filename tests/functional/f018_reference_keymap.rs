//! F-018 · The reference's keymap: same bindings, default on macOS/Windows/Linux (SPEC-0028 AK-2…AK-5).

use crate::harness::*;
use serde_json::json;

fn shortcuts(ide: &mut Ide, keymap: &str, action: &str) -> String {
    text(&ide.ok("keymap_lookup", json!({"keymap": keymap, "action": action})))
}

#[test]
fn default_and_macos_bindings_of_well_known_actions() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    assert!(shortcuts(&mut ide, "default", "SelectNextOccurrence").contains("Alt"));
    assert!(shortcuts(&mut ide, "macos", "SelectNextOccurrence").contains("⌃G"));
    assert!(shortcuts(&mut ide, "macos", "GotoDeclaration").contains("⌘B"));
    assert!(shortcuts(&mut ide, "macos", "$Copy").contains("⌘C"));
    assert!(shortcuts(&mut ide, "macos", "$Undo").contains("⌘Z"));
    assert!(shortcuts(&mut ide, "default", "$Copy").contains("Ctrl"));
}

#[test]
fn a_keystroke_names_its_actions() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "keymap_lookup",
        json!({"keymap": "macos", "keystroke": "meta D"}),
    );
    assert!(text(&r["actions"]).contains("EditorDuplicate"), "{r}");
    let r = ide.ok(
        "keymap_lookup",
        json!({"keymap": "default", "keystroke": "ctrl D"}),
    );
    assert!(text(&r["actions"]).contains("EditorDuplicate"), "{r}");
}
