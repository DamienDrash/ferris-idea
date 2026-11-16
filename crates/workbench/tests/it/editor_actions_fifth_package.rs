//! Window checks of the fifth package of keymap actions: Cut up to Line End, overwrite mode,
//! Auto-Indent Lines and Context Info (`SPEC-0057` v6 AK-25), Fold Code Block and Fold Selection
//! (`SPEC-0083` v7 AK-18). Every key goes through the window's key dispatch
//! (`simulate_keystrokes`); the way from the keyboard to that entry is not run here. No language
//! server takes part. The font size actions change a value of the whole process and have a test
//! program of their own (`tests/font_size_actions.rs`).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{EditorView, WorkbenchView};

/// Regions by line: the class body 2–17, `total` 3–11 with the loop 5–9 and the `if` 6–8 in it,
/// `clear` 13–16.
const CART: &str = "package shop;

public class Cart {
    int total(int[] xs) {
        int sum = 0;
        for (int x : xs) {
            if (x > 0) {
                sum += x;
            }
        }
        return sum;
    }

    void clear() {
        items = 0;
        count = 0;
    }
}
";

const NOTES: &str = "alpha beta\ngamma\ndelta\n";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "idecraft-editor-actions-fifth-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join("src/shop/Cart.java"), CART).unwrap();
    std::fs::write(root.join("notes.txt"), NOTES).unwrap();
    std::fs::write(root.join("other.txt"), "abc\n").unwrap();
    // One method of 200 statements: its line (3) and the class's (2) scroll out of view.
    let mut tall = String::from("package shop;\n\npublic class Tall {\n    int tall() {\n");
    for i in 0..200 {
        tall.push_str(&format!("        int a{i} = {i};\n"));
    }
    tall.push_str("        return 0;\n    }\n}\n");
    std::fs::write(root.join("src/shop/Tall.java"), tall).unwrap();
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    // The window the keys go to is the active one, as on screen.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (bench, cx)
}

/// Waits until `done` holds, letting background work and timers run.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
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

/// Opens `file` (relative to the workspace) and gives its editor the focus.
fn open(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    root: &Path,
    file: &str,
) -> Entity<EditorView> {
    let file = root.join(file);
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

/// Opens a Java class and waits for its syntax tree (Collapse All folds something once it is
/// there).
fn open_class(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    root: &Path,
    name: &str,
) -> Entity<EditorView> {
    let editor = open(bench, cx, root, &format!("src/shop/{name}.java"));
    until(cx, "the syntax tree gives folding regions", |cx| {
        editor.update(cx, |e, cx| {
            e.run_action("CollapseAllRegions", cx);
            !e.folded_regions().is_empty()
        })
    });
    editor.update(cx, |e, cx| {
        e.run_action("ExpandAllRegions", cx);
        e.go_to(0, 0, cx);
    });
    cx.run_until_parked();
    editor
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

fn note(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |e, _| e.status().note)
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn folded(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
    editor.read_with(cx, |e, _| e.folded_regions())
}

/// The primary caret as 0-based line and column.
fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    editor.read_with(cx, |e, _| {
        let p = e
            .state
            .buffer
            .offset_to_position(e.state.carets.primary().head)
            .unwrap();
        (p.line, p.column)
    })
}

/// SPEC-0057 v6 AK-25: ⌃K cuts from the caret to the line end and fills the clipboard; pressed
/// again at the line end it takes the line break and the clipboard holds both cuts; a read-only
/// text and the clipboard stay as they are.
#[gpui::test]
fn ctrl_k_cuts_to_the_line_end(cx: &mut TestAppContext) {
    let root = workspace("cut");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root, "notes.txt");
    editor.update(cx, |e, cx| e.go_to(0, 6, cx));

    keys(cx, "ctrl-k");
    assert_eq!(text(&editor, cx), "alpha \ngamma\ndelta\n");
    assert_eq!(clipboard(cx).as_deref(), Some("beta"));
    assert_eq!(caret(&editor, cx), (0, 6));
    // At the line end the next line joins, and one paste would bring back both cuts.
    keys(cx, "ctrl-k");
    assert_eq!(text(&editor, cx), "alpha gamma\ndelta\n");
    assert_eq!(clipboard(cx).as_deref(), Some("beta\n"));
    // The window was not the active one in between — the clipboard may hold another text by
    // now: the next cut starts afresh.
    cx.deactivate_window();
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    keys(cx, "ctrl-k");
    assert_eq!(text(&editor, cx), "alpha \ndelta\n");
    assert_eq!(clipboard(cx).as_deref(), Some("gamma"));
    // After a caret move the next cut stands alone as well.
    keys(cx, "right");
    assert_eq!(caret(&editor, cx), (1, 0));
    keys(cx, "ctrl-k");
    assert_eq!(text(&editor, cx), "alpha \n\n");
    assert_eq!(clipboard(cx).as_deref(), Some("delta"));
    // One undo step per cut.
    keys(cx, "cmd-z");
    assert_eq!(text(&editor, cx), "alpha \ndelta\n");

    // A read-only text is not cut and the clipboard keeps its text.
    editor.update(cx, |e, cx| {
        e.set_read_only(true);
        e.go_to(0, 0, cx);
    });
    keys(cx, "ctrl-k");
    assert_eq!(text(&editor, cx), "alpha \ndelta\n");
    assert_eq!(clipboard(cx).as_deref(), Some("delta"));
    assert!(
        note(&editor, cx).is_some_and(|n| n.starts_with("Read-only")),
        "the status bar says why"
    );
}

/// SPEC-0057 v6 AK-25: Insert switches overwrite mode for this editor alone; typing replaces the
/// character under the caret and inserts at the line end; the status and the caret follow.
#[gpui::test]
fn insert_switches_overwrite_mode(cx: &mut TestAppContext) {
    let root = workspace("overwrite");
    let (bench, cx) = window(&root, cx);
    let other = open(&bench, cx, &root, "other.txt");
    let editor = open(&bench, cx, &root, "notes.txt");
    editor.update(cx, |e, cx| e.go_to(1, 3, cx));
    cx.run_until_parked();
    let bar = editor.read_with(cx, |e, _| e.painted_caret_widths().to_vec());
    assert_eq!(bar, [2.0], "a bar in insert mode");
    assert!(!editor.read_with(cx, |e, _| e.status().overwrite));

    keys(cx, "insert");
    assert!(editor.read_with(cx, |e, _| e.overwrite() && e.status().overwrite));
    assert!(
        !other.read_with(cx, |e, _| e.overwrite()),
        "the other editor keeps its mode"
    );
    let block = editor.read_with(cx, |e, _| e.painted_caret_widths().to_vec());
    assert!(
        block.len() == 1 && block[0] > 4.0,
        "a block as wide as a character: {block:?}"
    );

    // `gamma`: the caret is before the second `m`.
    cx.simulate_input("XY");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "alpha beta\ngamXY\ndelta\n");
    // At the line end typing inserts; the line break stays.
    cx.simulate_input("Z");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "alpha beta\ngamXYZ\ndelta\n");
    assert_eq!(caret(&editor, cx), (1, 6));
    let at_end = editor.read_with(cx, |e, _| e.painted_caret_widths().to_vec());
    assert!(
        at_end.len() == 1 && at_end[0] > 4.0,
        "one cell at the line end: {at_end:?}"
    );
    // The typed run is one undo step.
    keys(cx, "cmd-z");
    assert_eq!(text(&editor, cx), NOTES);

    // Insert again: back to insert mode.
    keys(cx, "insert");
    assert!(!editor.read_with(cx, |e, _| e.overwrite() || e.status().overwrite));
    editor.update(cx, |e, cx| e.go_to(1, 0, cx));
    cx.simulate_input("x");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "alpha beta\nxgamma\ndelta\n");
    let bar = editor.read_with(cx, |e, _| e.painted_caret_widths().to_vec());
    assert_eq!(bar, [2.0]);
}

/// SPEC-0057 v6 AK-25: ⌃⌥I gives the selected lines of a Java file their indentation and moves a
/// caret without selection one line down; a text file has no rule and says so.
#[gpui::test]
fn auto_indent_lines_in_the_window(cx: &mut TestAppContext) {
    let root = workspace("indent");
    std::fs::write(
        root.join("src/shop/Flat.java"),
        "class Flat {\nint a;\n        void f() {\nrun();\n}\n}\n",
    )
    .unwrap();
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root, "src/shop/Flat.java");

    // The caret's line, then the caret is on the next one.
    editor.update(cx, |e, cx| e.go_to(1, 2, cx));
    keys(cx, "ctrl-alt-i");
    assert_eq!(
        text(&editor, cx),
        "class Flat {\n    int a;\n        void f() {\nrun();\n}\n}\n"
    );
    assert_eq!(caret(&editor, cx), (2, 2));
    // The lines 2 to 4 selected (the selection ends at the start of line 5).
    editor.update(cx, |e, cx| {
        let from = e.state.buffer.text().find("        void").unwrap();
        let to = e.state.buffer.text().rfind("}\n").unwrap();
        e.select_range(from..to, cx);
    });
    keys(cx, "ctrl-alt-i");
    let indented = "class Flat {\n    int a;\n    void f() {\n        run();\n    }\n}\n";
    assert_eq!(text(&editor, cx), indented);
    assert_eq!(note(&editor, cx), None);
    // One undo step.
    keys(cx, "cmd-z");
    assert_eq!(
        text(&editor, cx),
        "class Flat {\n    int a;\n        void f() {\nrun();\n}\n}\n"
    );

    // No rule for plain text: nothing changes, the status bar says so.
    let notes = open(&bench, cx, &root, "notes.txt");
    notes.update(cx, |e, cx| e.go_to(1, 2, cx));
    keys(cx, "ctrl-alt-i");
    assert_eq!(text(&notes, cx), NOTES);
    assert_eq!(caret(&notes, cx), (1, 2));
    assert!(
        note(&notes, cx).is_some_and(|n| n.contains("no indentation rule")),
        "{:?}",
        note(&notes, cx)
    );
}

/// SPEC-0057 v6 AK-25: ⌃⇧Q shows the line of the method the caret is in when that line is
/// scrolled out of view, the class's line on the second press and nothing on the third; the
/// strip goes with a caret move; with everything in view nothing is shown.
#[gpui::test]
fn context_info_shows_the_hidden_declaration(cx: &mut TestAppContext) {
    let root = workspace("context");
    let (bench, cx) = window(&root, cx);
    let editor = open_class(&bench, cx, &root, "Tall");
    let shown = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.shown_context());

    // Near the top everything is in view.
    editor.update(cx, |e, cx| e.go_to(6, 8, cx));
    cx.run_until_parked();
    keys(cx, "ctrl-shift-q");
    assert_eq!(shown(cx), None);
    assert_eq!(note(&editor, cx), None);

    // Far down in the method: its line and the class's are above the view.
    editor.update(cx, |e, cx| e.go_to(150, 8, cx));
    cx.run_until_parked();
    let (scrolled, _) = editor.read_with(cx, |e, _| e.scrolled_rows());
    assert!(scrolled > 4.0, "the view is scrolled: {scrolled}");
    keys(cx, "ctrl-shift-q");
    assert_eq!(shown(cx), Some((3, "    int tall() {".to_owned())));
    assert_eq!(caret(&editor, cx), (150, 8), "the caret stays");
    assert_eq!(editor.read_with(cx, |e, _| e.scrolled_rows()).0, scrolled);
    keys(cx, "ctrl-shift-q");
    assert_eq!(shown(cx), Some((2, "public class Tall {".to_owned())));
    keys(cx, "ctrl-shift-q");
    assert_eq!(shown(cx), None, "after the outermost the strip goes");
    // Shown again, it goes with a caret move and with Escape.
    keys(cx, "ctrl-shift-q");
    assert_eq!(shown(cx).map(|(line, _)| line), Some(3));
    keys(cx, "right");
    assert_eq!(shown(cx), None);
    keys(cx, "ctrl-shift-q");
    assert_eq!(shown(cx).map(|(line, _)| line), Some(3));
    keys(cx, "escape");
    assert_eq!(shown(cx), None);
    assert_eq!(caret(&editor, cx), (150, 9));

    // A text file has no declarations: the status bar says so.
    let notes = open(&bench, cx, &root, "notes.txt");
    keys(cx, "ctrl-shift-q");
    assert_eq!(notes.read_with(cx, |e, _| e.shown_context()), None);
    assert!(
        note(&notes, cx).is_some_and(|n| n.starts_with("Context Info")),
        "{:?}",
        note(&notes, cx)
    );
}

/// SPEC-0083 v7 AK-18: ⇧⌘. folds the innermost code block around the caret and, pressed again,
/// the next ones outwards; ⌘. folds the selected lines and opens them again; a text file has no
/// code blocks.
#[gpui::test]
fn folding_a_block_and_a_selection(cx: &mut TestAppContext) {
    let root = workspace("fold");
    let (bench, cx) = window(&root, cx);
    let editor = open_class(&bench, cx, &root, "Cart");

    // In `sum += x;`: the `if`, then the loop, then the method; the class body is no code block.
    editor.update(cx, |e, cx| e.go_to(7, 16, cx));
    keys(cx, "cmd-shift-.");
    assert_eq!(folded(&editor, cx), [(6, 8)]);
    assert_eq!(
        caret(&editor, cx),
        (6, 24),
        "the end of the fold's first line"
    );
    keys(cx, "cmd-shift-.");
    assert_eq!(folded(&editor, cx), [(5, 9)]);
    // The key as a keyboard sends it: the character Shift types there.
    keys(cx, "cmd->");
    assert_eq!(folded(&editor, cx), [(3, 11)]);
    keys(cx, "cmd-shift-.");
    assert_eq!(folded(&editor, cx), [(3, 11)], "nothing is left to fold");
    assert_eq!(text(&editor, cx), CART, "folding changes no text");
    editor.update(cx, |e, cx| {
        e.run_action("ExpandAllRegions", cx);
    });

    // Three selected lines of `clear`, ending at the start of the fourth: a region of its own.
    editor.update(cx, |e, cx| {
        let text = e.state.buffer.text();
        let from = text.find("    void clear").unwrap() + 4;
        let to = text.find("    }\n}").unwrap();
        e.select_range(from..to, cx);
    });
    keys(cx, "cmd-.");
    assert_eq!(folded(&editor, cx), [(13, 15)]);
    assert_eq!(caret(&editor, cx), (13, 18));
    assert_eq!(note(&editor, cx), None);
    // Without a selection, on the fold's line: it is opened and gone.
    keys(cx, "cmd-.");
    assert!(folded(&editor, cx).is_empty());
    // Once more without a selection: the innermost region around the caret, the method's body.
    keys(cx, "cmd-.");
    assert_eq!(folded(&editor, cx), [(13, 16)]);
    keys(cx, "cmd-.");
    assert!(folded(&editor, cx).is_empty());
    // A selection within one line is not folded.
    editor.update(cx, |e, cx| e.select_range(0..7, cx));
    keys(cx, "cmd-.");
    assert!(folded(&editor, cx).is_empty());
    assert!(
        note(&editor, cx).is_some_and(|n| n.contains("within one line")),
        "{:?}",
        note(&editor, cx)
    );

    // A text file has no code blocks.
    let notes = open(&bench, cx, &root, "notes.txt");
    keys(cx, "cmd-shift-.");
    assert!(folded(&notes, cx).is_empty());
    assert!(
        note(&notes, cx).is_some_and(|n| n.contains("Java")),
        "{:?}",
        note(&notes, cx)
    );
}
