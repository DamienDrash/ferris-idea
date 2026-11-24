//! Window check A12 of 2026-11-18 (feature row F-011) in a window of the test harness: Move (F6)
//! of a Java class updates the import of another file (`SPEC-0114` v5 behaviour 9) and is one undo
//! group, taken back and made again by ⌘Z/⇧⌘Z in its editor and in the Project tree, refused when
//! a touched file changed since (`SPEC-0114` v5 behaviour 10, `SPEC-0193` v7 behaviour 12); ⇧F6 on
//! a keyword offers no rename (`SPEC-0074` v2 behaviour 5). Works with or without jdtls: where the
//! server gives no edits, Ferris IDEA's own rewriting does. Nothing here reads or writes the user's
//! settings: the window keeps its undo groups in memory, the local history lives in the project.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;
type Editor = Entity<EditorView>;

const INVENTORY: &str = "package lab.inventory;\n\npublic class Inventory {\n    int count;\n}\n";
const MAIN: &str = "package lab.app;\n\nimport lab.inventory.Inventory;\n\npublic class Main {\n    Inventory i = new Inventory();\n}\n";
const OLD: &str = "src/main/java/lab/inventory/Inventory.java";
const NEW: &str = "src/main/java/lab/stock/Inventory.java";
const USER: &str = "src/main/java/lab/app/Main.java";

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-move-undo-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Editor {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

fn text(editor: &Editor, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// A chord through the window's key dispatch; one nothing there took goes to the menu bar's
/// action, as the macOS application does it.
fn chord(bench: &Bench, cx: &mut VisualTestContext, keystroke: &str, action: &str) {
    let key = gpui::Keystroke::parse(keystroke).unwrap();
    let taken = cx.update(|window, cx| window.dispatch_keystroke(key, cx));
    cx.run_until_parked();
    if !taken {
        bench.update_in(cx, |b, window, cx| b.menu_action(action, window, cx));
        cx.run_until_parked();
    }
}

/// Waits until `done` holds, letting background work and timers run.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1200 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// `SPEC-0114` v5 AK-9 and AK-10, `SPEC-0193` v7 AK-11 (window check A12 V1, V2).
#[gpui::test]
fn a_move_updates_imports_and_is_one_undo_group(cx: &mut TestAppContext) {
    let root = workspace("group", &[(OLD, INVENTORY), (USER, MAIN)]);
    let (bench, cx) = window(&root, cx);
    let moved = open(&bench, cx, &root.join(OLD));

    // F6: the field holds the folder; the new package's folder, Enter.
    bench.update_in(cx, |b, window, cx| b.menu_action("Move", window, cx));
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.goto_popup())
        .expect("the Move popup is open");
    popup.update(cx, |g, cx| g.set_field_text("src/main/java/lab/stock", cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    until(cx, "the file is moved", |cx| {
        status(&bench, cx).starts_with("Moved to")
    });
    assert!(!root.join(OLD).exists());
    assert!(root.join(NEW).is_file());
    // V1: the import of the other file follows, the moved file has its new package.
    let main_after = read(&root, USER);
    assert!(
        main_after.contains("import lab.stock.Inventory;"),
        "{main_after}"
    );
    assert!(text(&moved, cx).starts_with("package lab.stock;\n"));
    let name = "Move Inventory.java to src/main/java/lab/stock";

    // V2: one ⌘Z in the moved file's editor takes it all back.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    chord(&bench, cx, "cmd-z", "$Undo");
    assert_eq!(status(&bench, cx), format!("Undone: {name} (2 files)"));
    assert!(root.join(OLD).is_file() && !root.join(NEW).exists());
    assert_eq!(read(&root, USER), MAIN);
    let editor = bench
        .read_with(cx, |b, cx| b.editor_for(&root.join(OLD), cx))
        .expect("the editor followed the file back");
    assert_eq!(text(&editor, cx), INVENTORY);

    // ⇧⌘Z moves it again.
    chord(&bench, cx, "cmd-shift-z", "$Redo");
    assert_eq!(status(&bench, cx), format!("Redone: {name} (2 files)"));
    assert!(root.join(NEW).is_file() && !root.join(OLD).exists());
    assert_eq!(read(&root, USER), main_after);

    // ⌘Z and ⇧⌘Z in the Project tree take the same group.
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
    chord(&bench, cx, "cmd-z", "$Undo");
    // From the tree it is asked first (SPEC-0193 v8 behaviour 12); Return takes it back.
    assert!(root.join(NEW).is_file(), "nothing moves before the answer");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(status(&bench, cx), format!("Undone: {name} (2 files)"));
    assert!(root.join(OLD).is_file() && !root.join(NEW).exists());
    assert_eq!(read(&root, USER), MAIN);
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
    chord(&bench, cx, "cmd-shift-z", "$Redo");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(status(&bench, cx), format!("Redone: {name} (2 files)"));
    assert!(root.join(NEW).is_file());

    // Main.java changed on disk since: refused, nothing moves.
    std::fs::write(root.join(USER), format!("{main_after}// later\n")).unwrap();
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    chord(&bench, cx, "cmd-z", "$Undo");
    let refused = status(&bench, cx);
    assert!(
        refused.starts_with(&format!("Cannot undo {name}: ")),
        "{refused}"
    );
    assert!(root.join(NEW).is_file() && !root.join(OLD).exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0074` v2 AK-4 (window check A12 R1): ⇧F6 on the keyword `record` opens no rename popup
/// and says why; no language server is asked.
#[gpui::test]
fn rename_is_refused_on_a_keyword(cx: &mut TestAppContext) {
    let code = "package lab;\n\npublic record Item(String name) {}\n";
    let root = workspace("keyword", &[("src/main/java/lab/Item.java", code)]);
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("src/main/java/lab/Item.java"));
    let at = code.find("record").unwrap() + 2;
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("RenameElement", window, cx)
    });
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.goto_popup().is_none()));
    assert_eq!(
        status(&bench, cx),
        "Cannot rename this element: the caret should be on an identifier"
    );
    let _ = std::fs::remove_dir_all(&root);
}
