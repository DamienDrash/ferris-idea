//! Undo and redo of the project tree's file operations in a window of the test harness (feature
//! row F-011, OBS-0042 point 3): `SPEC-0275` v16 behaviours 37–43 and `SPEC-0057` v8 behaviour
//! 29. Chords go through the window's key dispatch; a chord nothing in the window takes goes on
//! to the menu bar's action, as the macOS application does it. File operations are started by
//! the calls the tree's menu makes (the name field, the Delete question, ⌘X and ⌘V in the tree).
//! What needs eyes stays a hand test: how the question is drawn and what the Edit menu shows in
//! the real menu bar.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::file_ops::NewKind;
use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)], folders: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-undo-file-operations-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for dir in folders {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    // The Delete question is drawn in the window, as in the window self-test.
    cx.update(|cx| cx.set_global(ferris_idea_workbench::dialogs::DrawnConfirmations));
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
}

fn active_text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn text_of(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Option<String> {
    bench.read_with(cx, |b, cx| {
        b.editor_for(file, cx)
            .map(|e| e.read(cx).state.buffer.text())
    })
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn tabs(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, _| b.main_tab_titles())
}

fn focus_tree(bench: &Bench, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
}

fn focus_editor(bench: &Bench, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
}

fn select(bench: &Bench, cx: &mut VisualTestContext, path: &str) {
    bench.update_in(cx, |b, _, cx| {
        let at = b.project_root().join(path);
        b.project_tree_call(&serde_json::json!({"action": "select", "path": at}), cx)
            .unwrap_or_else(|e| panic!("select {path}: {e}"));
    });
    cx.run_until_parked();
}

/// Presses a chord as the macOS application does: the window's key dispatch first; a chord
/// nothing there took goes to the menu bar, whose key equivalent carries out `action`.
fn chord(bench: &Bench, cx: &mut VisualTestContext, keystroke: &str, action: &str) {
    let key = gpui::Keystroke::parse(keystroke).unwrap();
    let taken = cx.update(|window, cx| window.dispatch_keystroke(key, cx));
    cx.run_until_parked();
    if !taken {
        bench.update_in(cx, |b, window, cx| b.menu_action(action, window, cx));
        cx.run_until_parked();
    }
}

fn undo(bench: &Bench, cx: &mut VisualTestContext) {
    chord(bench, cx, "cmd-z", "$Undo");
}

fn redo(bench: &Bench, cx: &mut VisualTestContext) {
    chord(bench, cx, "cmd-shift-z", "$Redo");
}

/// The question drawn in the window: its headline, its detail and its buttons.
fn question(bench: &Bench, cx: &mut VisualTestContext) -> Option<(String, String, Vec<String>)> {
    bench.read_with(cx, |b, _| {
        b.drawn_question()
            .map(|q| (q.message.clone(), q.detail.clone(), q.buttons.clone()))
    })
}

fn key(cx: &mut VisualTestContext, keystroke: &str) {
    cx.simulate_keystrokes(keystroke);
    cx.run_until_parked();
}

/// What Edit › Undo and Edit › Redo would be called now.
fn titles(bench: &Bench, cx: &mut VisualTestContext) -> (Option<String>, Option<String>) {
    bench.update_in(cx, |b, window, _| {
        let t = b.file_undo_titles(window);
        (t.undo, t.redo)
    })
}

/// New from the tree: the name field for `kind` in the project folder, `name`, Return.
fn new_item(bench: &Bench, cx: &mut VisualTestContext, kind: NewKind, name: &str) {
    bench.update_in(cx, |b, window, cx| {
        let dir = b.project_root().to_path_buf();
        b.start_new(kind, dir, window, cx);
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, _, cx| b.set_name_field(name, cx));
    cx.run_until_parked();
    key(cx, "enter");
    assert_eq!(
        bench.read_with(cx, |b, _| b.name_prompt_state()),
        None,
        "the name field closed"
    );
}

fn rename(bench: &Bench, cx: &mut VisualTestContext, file: &Path, name: &str) {
    let file = file.to_path_buf();
    bench.update_in(cx, |b, window, cx| b.start_rename(file, window, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, _, cx| b.set_name_field(name, cx));
    cx.run_until_parked();
    key(cx, "enter");
}

fn delete(bench: &Bench, cx: &mut VisualTestContext, paths: &[PathBuf]) {
    bench.update_in(cx, |b, window, cx| b.ask_delete(paths, window, cx));
    cx.run_until_parked();
    key(cx, "enter");
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `SPEC-0057` v8 AK-28 (behaviour 29): with the focus in the project tree ⌘Z and ⇧⌘Z never
/// change the editor's text, also when typing was the last thing done; the status bar says that
/// the tree has nothing to undo.
#[gpui::test]
fn undo_in_the_tree_leaves_the_editor_alone(cx: &mut TestAppContext) {
    let root = workspace("alone", &[("a.txt", "one\n")], &[]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("a.txt"));
    cx.simulate_input("zz");
    cx.run_until_parked();
    assert_eq!(active_text(&bench, cx), "zzone\n");

    focus_tree(&bench, cx);
    undo(&bench, cx);
    assert_eq!(active_text(&bench, cx), "zzone\n", "the typing stays");
    assert_eq!(status(&bench, cx), "Nothing to undo in the project tree");
    assert_eq!(question(&bench, cx), None);
    redo(&bench, cx);
    assert_eq!(active_text(&bench, cx), "zzone\n");
    assert_eq!(status(&bench, cx), "Nothing to redo in the project tree");
    assert_eq!(titles(&bench, cx), (None, None));

    // Back in the editor ⌘Z is the editor's.
    focus_editor(&bench, cx);
    undo(&bench, cx);
    assert_eq!(active_text(&bench, cx), "one\n");
    redo(&bench, cx);
    assert_eq!(active_text(&bench, cx), "zzone\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0275` v16 AK-36 (behaviours 38, 39, 42): New File is taken back from the tree after a
/// question, Cancel changes nothing, redo puts the file back; typing in another file in between
/// does not come first; the Edit menu's titles name the step while the tree has the focus.
#[gpui::test]
fn a_new_file_is_undone_from_the_tree(cx: &mut TestAppContext) {
    let root = workspace("new", &[("a.txt", "one\n")], &[]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("a.txt"));
    new_item(&bench, cx, NewKind::File, "notes/new.txt");
    let new = root.join("notes/new.txt");
    assert!(new.is_file());
    assert!(tabs(&bench, cx).iter().any(|t| t == "new.txt"), "it opened");

    // Typing in the other file is the last thing done.
    open(&bench, cx, &root.join("a.txt"));
    cx.simulate_input("zz");
    cx.run_until_parked();
    assert_eq!(active_text(&bench, cx), "zzone\n");
    assert_eq!(titles(&bench, cx), (None, None), "plain in the editor");

    focus_tree(&bench, cx);
    assert_eq!(
        titles(&bench, cx),
        (Some("New File new.txt".to_owned()), None)
    );
    undo(&bench, cx);
    let (message, detail, buttons) = question(&bench, cx).expect("the tree asks first");
    assert_eq!(message, "Undo New File \"notes/new.txt\"?");
    assert!(detail.contains("taken out of the project"), "{detail}");
    assert_eq!(buttons, ["Undo", "Cancel"]);
    assert!(new.is_file(), "nothing happens before the answer");
    assert_eq!(active_text(&bench, cx), "zzone\n", "not the typing");

    // Cancel: file, tab and step stay.
    key(cx, "escape");
    assert_eq!(question(&bench, cx), None);
    assert!(new.is_file());
    assert!(tabs(&bench, cx).iter().any(|t| t == "new.txt"));
    assert_eq!(
        titles(&bench, cx),
        (Some("New File new.txt".to_owned()), None)
    );

    // The action: the file and the folder made on its way go, its tab closes.
    undo(&bench, cx);
    key(cx, "enter");
    assert_eq!(question(&bench, cx), None);
    assert!(!new.exists());
    assert!(!root.join("notes").exists());
    assert_eq!(tabs(&bench, cx), ["a.txt"]);
    assert_eq!(status(&bench, cx), "Undone: New File new.txt");
    assert_eq!(
        text_of(&bench, cx, &root.join("a.txt")).as_deref(),
        Some("zzone\n")
    );
    assert_eq!(
        titles(&bench, cx),
        (None, Some("New File new.txt".to_owned())),
        "the tree has the focus again"
    );
    undo(&bench, cx);
    assert_eq!(status(&bench, cx), "Nothing to undo in the project tree");

    // Redo asks too and puts the file back; it is not opened by itself.
    redo(&bench, cx);
    let (message, _, buttons) = question(&bench, cx).expect("redo asks");
    assert_eq!(message, "Redo New File \"notes/new.txt\"?");
    assert_eq!(buttons, ["Redo", "Cancel"]);
    assert!(!new.exists());
    key(cx, "enter");
    assert_eq!(read(&root, "notes/new.txt"), "");
    assert_eq!(tabs(&bench, cx), ["a.txt"]);
    assert_eq!(status(&bench, cx), "Redone: New File new.txt");
    assert_eq!(
        text_of(&bench, cx, &root.join("a.txt")).as_deref(),
        Some("zzone\n")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0275` v16 AK-36 (behaviour 39): Rename, a cut and pasted file and Delete — of one file
/// and of two — are taken back and made again from the tree; tabs follow, deleted content is
/// back.
#[gpui::test]
fn rename_move_and_delete_are_undone_from_the_tree(cx: &mut TestAppContext) {
    let root = workspace(
        "ops",
        &[
            ("a.txt", "alpha\n"),
            ("b.txt", "beta\n"),
            ("c.txt", "gamma\n"),
        ],
        &["dir"],
    );
    let (bench, cx) = window(&root, cx);
    let take_back = |bench: &Bench, cx: &mut VisualTestContext| {
        focus_tree(bench, cx);
        undo(bench, cx);
        assert!(question(bench, cx).is_some(), "undo asks");
        key(cx, "enter");
    };
    let make_again = |bench: &Bench, cx: &mut VisualTestContext| {
        focus_tree(bench, cx);
        redo(bench, cx);
        assert!(question(bench, cx).is_some(), "redo asks");
        key(cx, "enter");
    };

    // Rename: the tab follows the name back and forth.
    open(&bench, cx, &root.join("a.txt"));
    rename(&bench, cx, &root.join("a.txt"), "z.txt");
    assert!(root.join("z.txt").is_file() && !root.join("a.txt").exists());
    assert_eq!(tabs(&bench, cx), ["z.txt"]);
    focus_tree(&bench, cx);
    undo(&bench, cx);
    assert_eq!(
        question(&bench, cx).map(|q| q.0).as_deref(),
        Some("Undo Rename \"a.txt\" to \"z.txt\"?")
    );
    key(cx, "enter");
    assert!(root.join("a.txt").is_file() && !root.join("z.txt").exists());
    assert_eq!(tabs(&bench, cx), ["a.txt"]);
    assert_eq!(status(&bench, cx), "Undone: Rename a.txt to z.txt");
    make_again(&bench, cx);
    assert!(root.join("z.txt").is_file() && !root.join("a.txt").exists());
    assert_eq!(tabs(&bench, cx), ["z.txt"]);

    // Cut and Paste into a folder.
    select(&bench, cx, "z.txt");
    focus_tree(&bench, cx);
    key(cx, "cmd-x");
    select(&bench, cx, "dir");
    focus_tree(&bench, cx);
    key(cx, "cmd-v");
    key(cx, "enter");
    assert_eq!(read(&root, "dir/z.txt"), "alpha\n");
    assert!(!root.join("z.txt").exists());
    assert_eq!(
        titles(&bench, cx).0.as_deref(),
        Some("Move z.txt to dir"),
        "{}",
        status(&bench, cx)
    );
    take_back(&bench, cx);
    assert_eq!(read(&root, "z.txt"), "alpha\n");
    assert!(!root.join("dir/z.txt").exists());
    assert_eq!(tabs(&bench, cx), ["z.txt"]);
    make_again(&bench, cx);
    assert_eq!(read(&root, "dir/z.txt"), "alpha\n");
    assert!(!root.join("z.txt").exists());

    // Delete of one file: its content is back.
    delete(&bench, cx, &[root.join("b.txt")]);
    assert!(!root.join("b.txt").exists());
    assert_eq!(status(&bench, cx), "Deleted b.txt (kept in Local History)");
    assert_eq!(titles(&bench, cx).0.as_deref(), Some("Delete b.txt"));
    take_back(&bench, cx);
    assert_eq!(read(&root, "b.txt"), "beta\n");
    assert_eq!(status(&bench, cx), "Undone: Delete b.txt");
    make_again(&bench, cx);
    assert!(!root.join("b.txt").exists());
    take_back(&bench, cx);
    assert_eq!(read(&root, "b.txt"), "beta\n");

    // Delete of two files is one step.
    delete(&bench, cx, &[root.join("b.txt"), root.join("c.txt")]);
    assert!(!root.join("b.txt").exists() && !root.join("c.txt").exists());
    assert_eq!(titles(&bench, cx).0.as_deref(), Some("Delete 2 items"));
    focus_tree(&bench, cx);
    undo(&bench, cx);
    let (message, detail, _) = question(&bench, cx).expect("undo asks");
    assert_eq!(message, "Undo Delete of 2 items?");
    assert!(
        detail.contains("b.txt") && detail.contains("c.txt"),
        "{detail}"
    );
    key(cx, "enter");
    assert_eq!(read(&root, "b.txt"), "beta\n");
    assert_eq!(read(&root, "c.txt"), "gamma\n");
    // The step before it is the next one (the single Delete was undone, so the new Delete
    // dropped it), and the two files can be deleted again.
    assert_eq!(
        titles(&bench, cx),
        (
            Some("Move z.txt to dir".to_owned()),
            Some("Delete 2 items".to_owned())
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0275` v16 AK-36 (behaviour 40): a new file that was edited is not taken out of the
/// project — refused while its editor has unsaved changes, and refused after the save because
/// the file is no longer what New left; since v22 (behaviour 38) without a question first.
#[gpui::test]
fn a_changed_file_is_not_taken_back(cx: &mut TestAppContext) {
    let root = workspace("changed", &[("a.txt", "one\n")], &[]);
    let (bench, cx) = window(&root, cx);
    new_item(&bench, cx, NewKind::File, "n.txt");
    let new = root.join("n.txt");
    focus_editor(&bench, cx);
    cx.simulate_input("hello");
    cx.run_until_parked();
    assert_eq!(active_text(&bench, cx), "hello");

    focus_tree(&bench, cx);
    undo(&bench, cx);
    // A step that would be refused asks nothing (v22 behaviour 38).
    assert_eq!(question(&bench, cx), None);
    assert_eq!(
        status(&bench, cx),
        "Undo refused: save n.txt first; it has unsaved changes"
    );
    assert!(new.is_file());
    assert_eq!(tabs(&bench, cx), ["n.txt"]);

    focus_editor(&bench, cx);
    bench.update_in(cx, |b, window, cx| b.menu_action("SaveAll", window, cx));
    cx.run_until_parked();
    assert_eq!(read(&root, "n.txt"), "hello");
    focus_tree(&bench, cx);
    undo(&bench, cx);
    // A step that would be refused asks nothing (v22 behaviour 38).
    assert_eq!(question(&bench, cx), None);
    assert_eq!(
        status(&bench, cx),
        "Undo refused: n.txt was changed since the operation; nothing was touched"
    );
    assert_eq!(read(&root, "n.txt"), "hello");
    assert_eq!(tabs(&bench, cx), ["n.txt"]);
    assert_eq!(
        titles(&bench, cx),
        (Some("New File n.txt".to_owned()), None),
        "the step stays"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0275` v16 AK-37 (behaviour 43): the call of the MCP tool lists the steps and takes one
/// back without a question, under the same checks.
#[gpui::test]
fn the_agent_tool_takes_a_step_back_without_a_question(cx: &mut TestAppContext) {
    let root = workspace("tool", &[("a.txt", "one\n")], &[]);
    let (bench, cx) = window(&root, cx);
    new_item(&bench, cx, NewKind::File, "m.txt");
    let call = |bench: &Bench, cx: &mut VisualTestContext, action: &str| {
        let answer = bench.update_in(cx, |b, window, cx| {
            b.file_undo_call(&serde_json::json!({"action": action}), window, cx)
        });
        cx.run_until_parked();
        answer
    };
    let listed = call(&bench, cx, "list").unwrap();
    assert_eq!(listed["undo"][0]["label"], "New File m.txt");
    assert_eq!(listed["undo"][0]["kind"], "create");
    assert_eq!(listed["undo"][0]["paths"][0], "m.txt");
    assert_eq!(listed["redo"].as_array().map(Vec::len), Some(0));
    assert!(listed["done"].is_null());

    let undone = call(&bench, cx, "undo").unwrap();
    assert_eq!(undone["done"], "Undone: New File m.txt");
    assert_eq!(undone["redo"][0]["label"], "New File m.txt");
    assert_eq!(undone["undo"].as_array().map(Vec::len), Some(0));
    assert_eq!(question(&bench, cx), None, "no question for the agent");
    assert!(!root.join("m.txt").exists());
    assert_eq!(tabs(&bench, cx), Vec::<String>::new(), "its tab closed");
    assert_eq!(
        call(&bench, cx, "undo").unwrap_err(),
        "Nothing to undo in the project tree"
    );

    // Something took the file's place: redo is refused and overwrites nothing.
    std::fs::write(root.join("m.txt"), "someone else's").unwrap();
    let refused = call(&bench, cx, "redo").unwrap_err();
    assert_eq!(
        refused,
        "Redo refused: m.txt already exists; nothing was touched"
    );
    assert_eq!(read(&root, "m.txt"), "someone else's");
    std::fs::remove_file(root.join("m.txt")).unwrap();
    let redone = call(&bench, cx, "redo").unwrap();
    assert_eq!(redone["done"], "Redone: New File m.txt");
    assert_eq!(read(&root, "m.txt"), "");
    assert!(
        call(&bench, cx, "forget")
            .unwrap_err()
            .contains("unknown action")
    );
    let _ = std::fs::remove_dir_all(&root);
}
