//! Window checks of the bookmarks (`SPEC-0331`): the actions through the keymap, the marks of the
//! editor following edits, the popups, the tool window, another writer of the project file and a
//! second window on the same project. No language server takes part; nothing is drawn on a
//! screen.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::bookmarks::{BOOKMARKS_FILE, Store};
use ferris_idea_ui::tool_windows::ToolWindow;
use ferris_idea_workbench::bookmarks_model::Node;
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

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

const FILE: &str = "src/shop/Cart.java";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-bookmarks-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join(FILE), CART).unwrap();
    std::fs::write(root.join("notes.txt"), "one\ntwo\nthree\n").unwrap();
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
    (bench, cx)
}

fn open(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    file: &Path,
) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

fn action(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

/// The project's bookmarks as `file:line` (1-based) with their mnemonic, newest first.
fn places(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, _| {
        b.bookmarks()
            .list()
            .iter()
            .map(|m| match m.mnemonic {
                Some(c) => format!("{} [{c}]", m.place()),
                None => m.place(),
            })
            .collect()
    })
}

/// The 0-based lines the editor marks in its gutter, by line.
fn marks(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<usize> {
    editor.read_with(cx, |e, _| {
        let mut lines: Vec<usize> = e.bookmarks().iter().map(|b| b.line).collect();
        lines.sort_unstable();
        lines
    })
}

fn caret_line(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |e, _| e.status().line - 1)
}

/// The char offset of the start of the 0-based `line` of the editor's text.
fn line_start(editor: &Entity<EditorView>, cx: &mut VisualTestContext, line: usize) -> usize {
    editor.read_with(cx, |e, _| {
        e.state
            .buffer
            .text()
            .split('\n')
            .take(line)
            .map(|l| l.chars().count() + 1)
            .sum()
    })
}

/// SPEC-0331 AK-6, AK-1 and AK-10 (window part): the keys of the keymap set, move and remove
/// bookmarks; the gutter and the project file follow edits.
#[gpui::test]
fn the_keymap_toggles_bookmarks_and_edits_move_them(cx: &mut TestAppContext) {
    let root = workspace("keys");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join(FILE));

    // F3 on lines 7 and 12, ⌃⇧1 on line 16 (OBS-0040's run).
    editor.update(cx, |e, cx| e.go_to(6, 4, cx));
    cx.simulate_keystrokes("f3");
    editor.update(cx, |e, cx| e.go_to(11, 0, cx));
    cx.simulate_keystrokes("f3");
    editor.update(cx, |e, cx| e.go_to(15, 0, cx));
    cx.simulate_keystrokes("ctrl-shift-1");
    cx.run_until_parked();
    assert_eq!(
        places(&bench, cx),
        [
            "src/shop/Cart.java:16 [1]",
            "src/shop/Cart.java:12",
            "src/shop/Cart.java:7"
        ]
    );
    assert_eq!(marks(&editor, cx), [6, 11, 15]);
    let saved = std::fs::read_to_string(root.join(BOOKMARKS_FILE)).unwrap();
    assert!(saved.contains("kind = \"bookmarks\""));
    assert!(saved.contains("line = 16\nmnemonic = \"1\""));

    // Two lines typed above line 12: the bookmarks below follow, in the editor and in the file.
    let at = line_start(&editor, cx, 9);
    editor.update(cx, |e, cx| {
        e.replace_ranges(&[(at..at, "        // a\n        // b\n".into())], cx)
    });
    cx.run_until_parked();
    assert_eq!(marks(&editor, cx), [6, 13, 17]);
    assert_eq!(
        places(&bench, cx),
        [
            "src/shop/Cart.java:18 [1]",
            "src/shop/Cart.java:14",
            "src/shop/Cart.java:7"
        ]
    );
    // Line 7 deleted as a whole: its bookmark goes with it.
    let (from, to) = (line_start(&editor, cx, 6), line_start(&editor, cx, 7));
    editor.update(cx, |e, cx| {
        e.replace_ranges(&[(from..to, String::new())], cx)
    });
    cx.run_until_parked();
    assert_eq!(marks(&editor, cx), [12, 16]);
    assert_eq!(
        places(&bench, cx),
        ["src/shop/Cart.java:17 [1]", "src/shop/Cart.java:13"]
    );
    assert_eq!(Store::open(&root).bookmarks().list().len(), 2);

    // ⌃1 from another file comes back to the bookmark; the next and previous bookmark wrap.
    open(&bench, cx, &root.join("notes.txt"));
    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_file()),
        Some(root.join(FILE))
    );
    assert_eq!(caret_line(&editor, cx), 16);
    action(&bench, cx, "GotoNextBookmark");
    assert_eq!(
        caret_line(&editor, cx),
        12,
        "from the last one to the first"
    );
    action(&bench, cx, "GotoPreviousBookmark");
    assert_eq!(caret_line(&editor, cx), 16);
    cx.simulate_keystrokes("ctrl-7");
    cx.run_until_parked();
    assert_eq!(caret_line(&editor, cx), 16, "no bookmark has 7");

    // ⌃⇧1 on another line takes the mnemonic over; again on that line removes the bookmark; F3
    // on a bookmarked line removes it.
    // The first as a keyboard sends it: the character of ⇧1, without Shift (SPEC-0028 v5 AK-9).
    editor.update(cx, |e, cx| e.go_to(12, 0, cx));
    cx.simulate_keystrokes("ctrl-!");
    cx.run_until_parked();
    assert_eq!(
        places(&bench, cx),
        ["src/shop/Cart.java:17", "src/shop/Cart.java:13 [1]"]
    );
    cx.simulate_keystrokes("ctrl-shift-1");
    cx.run_until_parked();
    assert_eq!(places(&bench, cx), ["src/shop/Cart.java:17"]);
    editor.update(cx, |e, cx| e.go_to(16, 0, cx));
    cx.simulate_keystrokes("f3");
    cx.run_until_parked();
    assert!(places(&bench, cx).is_empty());
    assert!(marks(&editor, cx).is_empty());
    assert!(!root.join(BOOKMARKS_FILE).exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0331 AK-10 (window part): the mnemonic popup, the list popup and the tool window through
/// their keys; behaviours 16, 20, 21, 22–24.
#[gpui::test]
fn popups_and_the_tool_window(cx: &mut TestAppContext) {
    let root = workspace("popups");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join(FILE));
    editor.update(cx, |e, cx| e.go_to(6, 0, cx));
    cx.simulate_keystrokes("f3");
    editor.update(cx, |e, cx| e.go_to(11, 0, cx));
    cx.simulate_keystrokes("ctrl-shift-4");

    // ⌥F3 on line 16: typing a character sets the bookmark with it.
    editor.update(cx, |e, cx| e.go_to(15, 0, cx));
    cx.simulate_keystrokes("alt-f3");
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.bookmarks_popup())
        .expect("the mnemonic popup");
    assert!(!popup.read_with(cx, |p, _| p.is_list()));
    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.bookmarks_popup()).is_none());
    assert_eq!(places(&bench, cx)[0], "src/shop/Cart.java:16 [B]");
    // Again on the same line: its character is selected; → and Enter give it the next one, and
    // a character another bookmark has is taken from it.
    cx.simulate_keystrokes("alt-f3");
    cx.run_until_parked();
    let popup = bench.read_with(cx, |b, _| b.bookmarks_popup()).unwrap();
    assert_eq!(popup.read_with(cx, |p, _| p.selected()), Some(11));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.bookmarks_popup()).is_none());
    assert_eq!(places(&bench, cx)[0], "src/shop/Cart.java:16 [B]");
    cx.simulate_keystrokes("alt-f3");
    cx.simulate_keystrokes("4");
    cx.run_until_parked();
    assert_eq!(
        places(&bench, cx),
        [
            "src/shop/Cart.java:16 [4]",
            "src/shop/Cart.java:12",
            "src/shop/Cart.java:7"
        ]
    );

    // ⌘F3: the list, newest first, with the lines' texts; ↓ and Enter jump.
    cx.simulate_keystrokes("cmd-f3");
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.bookmarks_popup())
        .expect("the list");
    let rows: Vec<String> = popup.read_with(cx, |p, _| {
        p.rows()
            .iter()
            .map(|r| format!("{}:{} {}", r.name, r.line + 1, r.label))
            .collect()
    });
    assert_eq!(
        rows,
        [
            "Cart.java:16 count = 0;",
            "Cart.java:12 }",
            "Cart.java:7 if (x > 0) {"
        ]
    );
    cx.simulate_keystrokes("down down");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.bookmarks_popup()).is_none());
    assert_eq!(caret_line(&editor, cx), 6);
    // A typed mnemonic jumps; Delete removes the selected bookmark and keeps the list open.
    cx.simulate_keystrokes("cmd-f3");
    cx.simulate_keystrokes("4");
    cx.run_until_parked();
    assert_eq!(caret_line(&editor, cx), 15);
    cx.simulate_keystrokes("cmd-f3");
    cx.simulate_keystrokes("down backspace");
    cx.run_until_parked();
    let popup = bench.read_with(cx, |b, _| b.bookmarks_popup()).unwrap();
    assert_eq!(popup.read_with(cx, |p, _| p.rows().len()), 2);
    assert_eq!(marks(&editor, cx), [6, 15]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // ⌘2: the tool window with project, file and bookmarks; it is not on the sidebar by default.
    cx.simulate_keystrokes("cmd-2");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Bookmarks)));
    assert!(!ToolWindow::Bookmarks.on_sidebar_by_default());
    let tool = bench
        .read_with(cx, |b, _| b.bookmarks_tool_view())
        .expect("the tree");
    let labels = |cx: &mut VisualTestContext| -> Vec<String> {
        tool.read_with(cx, |t, _| {
            t.nodes()
                .iter()
                .map(|n| match n {
                    Node::Project { .. } => "project".to_owned(),
                    Node::File { name, folder, .. } => format!("{name} ({folder})"),
                    Node::Bookmark(r) => match r.mnemonic {
                        Some(m) => format!("[{m}] {}", r.line_label()),
                        None => r.line_label(),
                    },
                })
                .collect()
        })
    };
    assert_eq!(
        labels(cx),
        [
            "project",
            "Cart.java (src/shop)",
            "[4] 16: count = 0;",
            "7: if (x > 0) {"
        ]
    );
    // ↓ ↓ ↓ Enter jumps to the last bookmark; the tree keeps showing.
    cx.simulate_keystrokes("down down down enter");
    cx.run_until_parked();
    assert_eq!(caret_line(&editor, cx), 6);
    // F2 on a bookmark asks for its description; it shows in place of the line's text.
    cx.simulate_keystrokes("cmd-2");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Bookmarks)));
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.bookmarks_popup()).is_some());
    cx.simulate_input("only positive ones");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.bookmarks_popup()).is_none());
    assert_eq!(labels(cx)[3], "7: only positive ones");
    // Delete on the bookmark removes it; on the file node it does nothing.
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(
        labels(cx),
        ["project", "Cart.java (src/shop)", "[4] 16: count = 0;"]
    );
    cx.simulate_keystrokes("up backspace");
    cx.run_until_parked();
    assert_eq!(labels(cx).len(), 3);
    assert_eq!(marks(&editor, cx), [15]);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0331 AK-4 and behaviours 9, 12, 13 in the window: another writer of the project file
/// shows within a second, a second window on the project has the same bookmarks, and a file
/// renamed through the file operations keeps them.
#[gpui::test]
fn another_writer_a_restart_and_a_renamed_file(cx: &mut TestAppContext) {
    let root = workspace("restart");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join(FILE));
    editor.update(cx, |e, cx| e.go_to(3, 0, cx));
    cx.simulate_keystrokes("f3");
    cx.run_until_parked();
    assert_eq!(marks(&editor, cx), [3]);

    // An agent sets one through the store while the window is open.
    Store::open(&root).change(|b| {
        b.set(
            FILE,
            13,
            ferris_idea_core::bookmarks::Change::Set('z'),
            ferris_idea_core::bookmarks::Change::Set("clears the cart".into()),
        )
    });
    for _ in 0..40 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        if marks(&editor, cx).len() == 2 {
            break;
        }
    }
    assert_eq!(marks(&editor, cx), [3, 13]);
    assert_eq!(
        places(&bench, cx),
        ["src/shop/Cart.java:14 [Z]", "src/shop/Cart.java:4"]
    );

    // The file renamed through the file operations: the bookmarks are under the new name.
    ferris_idea_core::file_ops::rename(&root, &root.join(FILE), "Basket.java").unwrap();
    for _ in 0..40 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        if places(&bench, cx)[0].contains("Basket") {
            break;
        }
    }
    assert_eq!(
        places(&bench, cx),
        ["src/shop/Basket.java:14 [Z]", "src/shop/Basket.java:4"]
    );

    // A window opened on the project afterwards has them, with mnemonic and description.
    let saved = bench.read_with(cx, |b, _| b.bookmarks().clone());
    let (again, cx) = window(&root, cx);
    assert_eq!(again.read_with(cx, |b, _| b.bookmarks().clone()), saved);
    let basket = open(&again, cx, &root.join("src/shop/Basket.java"));
    assert_eq!(marks(&basket, cx), [3, 13]);
    assert_eq!(
        again.read_with(cx, |b, _| b.bookmarks().list()[0].description.clone()),
        Some("clears the cart".to_owned())
    );
    let _ = std::fs::remove_dir_all(&root);
}
