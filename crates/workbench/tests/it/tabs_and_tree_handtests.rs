//! The steps of the hand tests 94, 193, 196, 200 and 201 (feature row F-020) in a window without a
//! screen: real mouse and key events where the hand test names them, the public calls of the
//! workbench where a step has no control to click headless.
//!
//! `SPEC-0112` v5.1 behaviours 6–8 (hand test 94), `SPEC-0275` v12 behaviours 1–7, 10, 17–27 (hand
//! tests 193, 196, 200, 201). What needs eyes on a real window stays in the hand tests: how the
//! menus, the highlight of several rows and the name field are drawn, the trackpad's two-finger
//! click, the native Delete alert and the database tree with a connection. The import a Java
//! language server adds on a move is checked when one is installed.
//!
//! Also here: the two open points of F-020 from OBS-0031 — packages compacted without a build file
//! (T1, `SPEC-0275` v11) and the Switcher in detail (`SPEC-0059` v9.4 behaviour 27) — and file
//! operations with a split editor (`SPEC-0275` behaviours 5, 6, 20, 21 in every group).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::LocalHistory;
use ferris_idea_workbench::editor_layout::MAIN;
use ferris_idea_workbench::{MenuPlace, WorkbenchView, hit_targets};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

fn workspace(tag: &str, files: &[(&str, &str)], folders: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-f020-handtests-{tag}-{}",
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

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    hit_targets::enable();
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

/// Waits until `done` holds, letting background work and timers run.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..3000 {
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

type Bench = Entity<WorkbenchView>;

/// The visible rows of the project tree as `path` (relative, `.` for the project folder).
fn rows(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["path"].as_str().map(str::to_owned))
        .collect()
}

/// The selected rows of the project tree.
fn selected(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["selected"] == true)
        .filter_map(|r| r["path"].as_str().map(str::to_owned))
        .collect()
}

fn tree(bench: &Bench, cx: &mut VisualTestContext, action: &str, path: &str) {
    bench.update_in(cx, |b, _, cx| {
        let at = b.project_root().join(path);
        b.project_tree_call(&serde_json::json!({"action": action, "path": at}), cx)
            .unwrap_or_else(|e| panic!("{action} {path}: {e}"));
    });
    cx.run_until_parked();
}

/// The middle of the tree row named `name` as it was last drawn.
fn row_at(cx: &mut VisualTestContext, name: &str) -> gpui::Point<gpui::Pixels> {
    cx.run_until_parked();
    let r = hit_targets::get(&format!("tree-{name}"))
        .unwrap_or_else(|| panic!("the row {name} is drawn"));
    point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
}

fn click_row(cx: &mut VisualTestContext, name: &str, modifiers: Modifiers) {
    let at = row_at(cx, name);
    cx.simulate_click(at, modifiers);
    cx.run_until_parked();
}

/// A press of the right button on the row, as a mouse and a trackpad's two-finger click send it.
fn right_click_row(cx: &mut VisualTestContext, name: &str) {
    let at = row_at(cx, name);
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
}

fn focus_tree(bench: &Bench, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
}

fn menu(bench: &Bench, cx: &mut VisualTestContext) -> Option<(MenuPlace, Vec<(String, bool)>)> {
    bench.read_with(cx, |b, _| b.context_menu_items())
}

fn enabled(items: &[(String, bool)], label: &str) -> Option<bool> {
    items.iter().find(|(l, _)| l == label).map(|(_, on)| *on)
}

fn choose(bench: &Bench, cx: &mut VisualTestContext, item: &str) {
    let chosen = bench.update_in(cx, |b, window, cx| b.choose_context_item(item, window, cx));
    assert!(chosen, "the menu has the enabled item {item}");
    cx.run_until_parked();
}

fn prompt(bench: &Bench, cx: &mut VisualTestContext) -> Option<(String, Option<String>)> {
    bench.read_with(cx, |b, _| b.name_prompt_state())
}

fn field(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| b.name_field_text(cx))
}

/// Replaces the name field's text, as typing over its selection does.
fn name(bench: &Bench, cx: &mut VisualTestContext, text: &str) {
    bench.update_in(cx, |b, _, cx| b.set_name_field(text, cx));
    cx.run_until_parked();
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn main_tabs(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, _| b.main_tab_titles())
}

fn clipboard(cx: &mut VisualTestContext) -> String {
    cx.read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default()
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

// ------------------------------------------------------------------------------ hand test 94

/// Hand test 94 (`SPEC-0112` behaviours 6–8, AK-4): two files, Split and Move Right, typing in
/// the right group, Move Tab to Opposite Group; then with one tab on the left Split Right and
/// the left tab moved.
#[gpui::test]
fn hand_test_94_a_tab_moves_to_the_other_group(cx: &mut TestAppContext) {
    let root = workspace("94", &[("one.txt", "first\n"), ("two.txt", "beta\n")], &[]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("one.txt"));
    open(&bench, cx, &root.join("two.txt"));
    let action = |bench: &Bench, cx: &mut VisualTestContext, id: &str| {
        bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
        cx.run_until_parked();
    };

    // View › Split and Move Right: the file leaves the main group for a focused second group.
    action(&bench, cx, "SplitAndMoveRight");
    bench.read_with(cx, |b, _| {
        assert_eq!(b.main_tab_titles(), ["one.txt"]);
        assert_eq!(b.split_state(), Some((vec!["two.txt".to_owned()], false)));
        assert!(b.split_active(), "the second group has the focus");
    });

    // Typing there: an unsaved change, marked on the tab.
    cx.simulate_input("x");
    cx.run_until_parked();
    bench.read_with(cx, |b, cx| {
        assert_eq!(b.active_text(cx).as_deref(), Some("xbeta\n"));
        assert_eq!(b.split_tab_labels(cx), ["two.txt *"]);
    });
    assert_eq!(read(&root, "two.txt"), "beta\n", "nothing was saved");

    // View › Move Tab to Opposite Group: back in the main group with the unsaved text, not read
    // again from disk; the emptied second group closes.
    action(&bench, cx, "MoveEditorToOppositeTabGroup");
    bench.read_with(cx, |b, cx| {
        assert_eq!(b.split_state(), None, "the emptied group closed");
        assert_eq!(b.group_ids(), [MAIN]);
        assert_eq!(b.main_tab_titles(), ["one.txt", "two.txt"]);
        assert_eq!(b.active_text(cx).as_deref(), Some("xbeta\n"));
        let editor = b.active_editor_entity().expect("the moved file is active");
        assert!(editor.read(cx).is_modified(), "the change is still unsaved");
    });
    // The undo history came along: undo takes the typed letter away.
    action(&bench, cx, "$Undo");
    bench.read_with(cx, |b, cx| {
        assert_eq!(b.active_text(cx).as_deref(), Some("beta\n"));
    });

    // With one tab on the left: Split Right, then the left tab moves. The left group empties
    // and takes the tabs of the right one; one group is left.
    bench.update_in(cx, |b, window, cx| b.close_all_tabs_unchecked(window, cx));
    cx.run_until_parked();
    open(&bench, cx, &root.join("one.txt"));
    action(&bench, cx, "SplitAndMoveRight");
    assert_eq!(
        status(&bench, cx),
        "Only one tab in the group: use Split Right or Split Down",
        "with one tab Split and Move moves nothing and says so"
    );
    action(&bench, cx, "SplitVertically");
    bench.read_with(cx, |b, _| {
        assert_eq!(b.split_state(), Some((vec!["one.txt".to_owned()], false)));
    });
    cx.simulate_input("y");
    cx.run_until_parked();
    action(&bench, cx, "PrevSplitter");
    bench.read_with(cx, |b, _| assert_eq!(b.focused_group(), MAIN));
    action(&bench, cx, "MoveEditorToOppositeTabGroup");
    bench.read_with(cx, |b, cx| {
        assert_eq!(b.split_state(), None, "the split closed");
        assert_eq!(b.group_ids(), [MAIN]);
        assert_eq!(b.main_tab_titles(), ["one.txt"]);
        assert_eq!(b.active_text(cx).as_deref(), Some("yfirst\n"));
    });

    // Without a split the action only says so.
    action(&bench, cx, "MoveEditorToOppositeTabGroup");
    assert_eq!(status(&bench, cx), "The editor is not split");
    let _ = std::fs::remove_dir_all(&root);
}

// ----------------------------------------------------------------------------- hand test 193

const A_JAVA: &str = "package com.acme;\n\npublic class A {\n}\n";
const B_JAVA: &str = "package com.acme;\n\npublic class B {\n    A a;\n}\n";

/// Hand test 193 (`SPEC-0275` behaviours 1–7 and 10): the three kinds of secondary click on a
/// folder, New › SQL File twice, Rename…, Delete…, ⌘N with HTTP Request and Java Class, the
/// menus of the editor text and of a tab, and the arrow keys in the project tree.
#[gpui::test]
fn hand_test_193_menus_new_rename_delete_and_tree_keys(cx: &mut TestAppContext) {
    let root = workspace(
        "193",
        &[
            ("notes.txt", "notes\n"),
            ("queries/first.sql", "select 1;\n"),
            ("src/main/java/com/acme/A.java", A_JAVA),
        ],
        &[],
    );
    let (bench, cx) = window(&root, cx);

    // Right button (mouse, two-finger click) and Control-click open the folder's menu and
    // select the row.
    right_click_row(cx, "queries");
    let (place, items) = menu(&bench, cx).expect("the right button opens a menu");
    assert_eq!(place, MenuPlace::ProjectTree);
    for item in [
        "New › File",
        "New › Directory",
        "New › Java Class",
        "New › SQL File",
        "New › HTTP Request",
        "Cut",
        "Copy",
        "Rename…",
        "Delete…",
    ] {
        assert_eq!(enabled(&items, item), Some(true), "{item} in {items:?}");
    }
    assert_eq!(
        enabled(&items, "Paste"),
        Some(false),
        "nothing was copied yet"
    );
    assert_eq!(selected(&bench, cx), ["queries"]);
    cx.simulate_keystrokes("escape");
    assert!(menu(&bench, cx).is_none(), "Escape closes the menu");

    let at = row_at(cx, "notes.txt");
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::control());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::control());
    cx.run_until_parked();
    let (place, _) = menu(&bench, cx).expect("a Control-click opens a menu");
    assert_eq!(place, MenuPlace::ProjectTree);
    assert_eq!(selected(&bench, cx), ["notes.txt"]);
    cx.simulate_keystrokes("escape");

    // New › SQL File, `query`, Enter: the file is in the folder and opens in the editor.
    right_click_row(cx, "queries");
    choose(&bench, cx, "New › SQL File");
    assert!(prompt(&bench, cx).is_some(), "the name field opened");
    cx.simulate_input("query");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("queries/query.sql").is_file());
    assert!(prompt(&bench, cx).is_none());
    assert_eq!(main_tabs(&bench, cx), ["query.sql"]);
    assert_eq!(selected(&bench, cx), ["queries/query.sql"]);

    // The same name again: refused, the field stays open with the reason until Escape.
    right_click_row(cx, "queries");
    choose(&bench, cx, "New › SQL File");
    cx.simulate_input("query");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (_, error) = prompt(&bench, cx).expect("the field stays open");
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.contains("already exists")),
        "{error:?}"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none(), "Escape closes the field");

    // Rename…: the field holds the name; Enter renames the file and its tab.
    right_click_row(cx, "query.sql");
    choose(&bench, cx, "Rename…");
    assert_eq!(field(&bench, cx).as_deref(), Some("query.sql"));
    // The part before the extension is selected: typing replaces it.
    cx.simulate_input("report");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("queries/report.sql").is_file());
    assert!(!root.join("queries/query.sql").exists());
    assert_eq!(main_tabs(&bench, cx), ["report.sql"]);

    // Delete… asks first, naming the file; Enter deletes and closes the tab.
    right_click_row(cx, "report.sql");
    choose(&bench, cx, "Delete…");
    assert_eq!(
        bench.read_with(cx, |b, _| b.delete_question()).as_deref(),
        Some("Delete file \"report.sql\"?")
    );
    assert!(root.join("queries/report.sql").is_file(), "asked first");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!root.join("queries/report.sql").exists());
    assert!(main_tabs(&bench, cx).is_empty(), "the tab closed");

    // ⌘N in the project tree shows the kinds of files; HTTP Request and Java Class.
    tree(&bench, cx, "select", "queries");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-n");
    cx.run_until_parked();
    let (place, items) = menu(&bench, cx).expect("⌘N opens New");
    assert_eq!(place, MenuPlace::New);
    let kinds: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "File",
            "Directory",
            "Java Class",
            "Kotlin File",
            "Python File",
            "TypeScript File",
            "JavaScript File",
            "SQL File",
            "HTTP Request"
        ]
    );
    choose(&bench, cx, "HTTP Request");
    cx.simulate_input("ping");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("queries/ping.http").is_file());

    tree(&bench, cx, "select", "src/main/java/com/acme");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-n");
    cx.run_until_parked();
    choose(&bench, cx, "Java Class");
    cx.simulate_input("Order");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let order = read(&root, "src/main/java/com/acme/Order.java");
    assert!(order.starts_with("package com.acme;\n"), "{order}");
    assert!(order.contains("public class Order {"), "{order}");
    assert_eq!(
        bench
            .read_with(cx, |b, _| b.active_path())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .as_deref(),
        Some("Order.java")
    );

    // A secondary click on the editor text: Go To, Find Usages, Refactor › …, Reformat Code.
    let r = hit_targets::get("editor-text").expect("the editor is drawn");
    let at = point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    let (place, items) = menu(&bench, cx).expect("the editor text has a menu");
    assert_eq!(place, MenuPlace::Editor);
    for item in [
        "Go To › Declaration or Usages",
        "Find Usages",
        "Refactor › Rename…",
        "Reformat Code",
        "Paste",
    ] {
        assert!(enabled(&items, item).is_some(), "{item} in {items:?}");
    }
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // A secondary click on an editor tab.
    let r = hit_targets::get("tab-0").expect("the tab is drawn");
    let at = point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    let (place, items) = menu(&bench, cx).expect("a tab has a menu");
    assert!(
        matches!(place, MenuPlace::Tab { index: 0, .. }),
        "{place:?}"
    );
    for item in ["Close", "Close Other Tabs", "Split Right", "Rename File…"] {
        assert!(enabled(&items, item).is_some(), "{item} in {items:?}");
    }
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // ←/→/↑/↓/Enter in the project tree.
    tree(&bench, cx, "collapse", "queries");
    tree(&bench, cx, "select", "queries");
    focus_tree(&bench, cx);
    let expanded = |bench: &Bench, cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, _| b.tree_is_expanded(&b.tree_selected_path()))
    };
    cx.simulate_keystrokes("right");
    assert!(expanded(&bench, cx), "→ expands a collapsed folder");
    assert_eq!(selected(&bench, cx), ["queries"]);
    cx.simulate_keystrokes("right");
    assert_eq!(
        selected(&bench, cx),
        ["queries/first.sql"],
        "→ goes to the first child"
    );
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&bench, cx), ["queries/ping.http"]);
    cx.simulate_keystrokes("up");
    assert_eq!(selected(&bench, cx), ["queries/first.sql"]);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        main_tabs(&bench, cx).contains(&"first.sql".to_owned()),
        "Enter opens a file"
    );
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("left");
    assert_eq!(selected(&bench, cx), ["queries"], "← goes to the parent");
    cx.simulate_keystrokes("left");
    assert!(!expanded(&bench, cx), "← collapses an expanded folder");
    cx.simulate_keystrokes("enter");
    assert!(expanded(&bench, cx), "Enter toggles a folder");
    let _ = std::fs::remove_dir_all(&root);
}

// ----------------------------------------------------------------------------- hand test 196

/// Hand test 196 (`SPEC-0275` v2 behaviours 17–22): ⌘C/⌘V of a file with the name field, a
/// second copy, Cut and Paste of an open file under another name, a folder into its own
/// sub-folder, a folder with sub-folders, an existing name in the field, foreign text on the
/// clipboard, and the Local History of the old path.
#[gpui::test]
fn hand_test_196_cut_copy_and_paste_of_files(cx: &mut TestAppContext) {
    let root = workspace(
        "196",
        &[
            ("a.txt", "alpha\n"),
            ("src/lib.txt", "lib\n"),
            ("src/inner/deep/leaf.txt", "leaf\n"),
            ("other/keep.txt", "keep\n"),
        ],
        &[],
    );
    let (bench, cx) = window(&root, cx);
    tree(&bench, cx, "expand", "src");

    // a.txt, ⌘C, src, ⌘V, Enter.
    click_row(cx, "a.txt", Modifiers::none());
    cx.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    assert_eq!(clipboard(cx), root.join("a.txt").display().to_string());
    click_row(cx, "src", Modifiers::none());
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    let (title, error) = prompt(&bench, cx).expect("Paste opens the name field");
    assert_eq!(title, "Copy a.txt to src");
    assert_eq!(error, None);
    assert_eq!(field(&bench, cx).as_deref(), Some("a.txt"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(read(&root, "src/a.txt"), "alpha\n");
    assert_eq!(read(&root, "a.txt"), "alpha\n", "a copy leaves the file");
    assert_eq!(selected(&bench, cx), ["src/a.txt"]);

    // ⌘V on src once more: the second copy gets a free name, nothing is overwritten.
    tree(&bench, cx, "select", "src");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert_eq!(field(&bench, cx).as_deref(), Some("a_copy.txt"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(read(&root, "src/a_copy.txt"), "alpha\n");

    // The file open in the editor; its menu shows Cut, Copy, Paste; Cut.
    open(&bench, cx, &root.join("src/a.txt"));
    tree(&bench, cx, "select", "src/a.txt");
    bench.update_in(cx, |b, _, cx| {
        b.open_context_menu(
            MenuPlace::ProjectTree,
            Some(root.join("src/a.txt")),
            point(px(200.), px(200.)),
            cx,
        )
    });
    let (_, items) = menu(&bench, cx).unwrap();
    for item in ["Cut", "Copy", "Paste"] {
        assert_eq!(enabled(&items, item), Some(true), "{item}");
    }
    choose(&bench, cx, "Cut");

    // Paste on another folder under the name b.txt: the file moves, the open tab follows.
    right_click_row(cx, "other");
    choose(&bench, cx, "Paste");
    let (title, _) = prompt(&bench, cx).expect("Paste opens the name field");
    assert_eq!(title, "Move a.txt to other");
    name(&bench, cx, "b.txt");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(read(&root, "other/b.txt"), "alpha\n");
    assert!(!root.join("src/a.txt").exists(), "the file moved");
    assert_eq!(main_tabs(&bench, cx), ["b.txt"]);
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("other/b.txt"))
    );
    assert_eq!(selected(&bench, cx), ["other/b.txt"]);

    // The old path in the Local History: the old content and the label of the move.
    let history = LocalHistory::open(&root).unwrap();
    let old = history.revisions("src/a.txt");
    assert!(!old.is_empty(), "the old path has revisions");
    assert!(
        old.iter().any(|r| history
            .content("src/a.txt", r.id)
            .is_ok_and(|c| c.as_deref() == Some(b"alpha\n".as_slice()))),
        "the old content is kept"
    );
    assert!(
        old.iter()
            .any(|r| r.label.as_deref().is_some_and(|l| l.starts_with("Moved "))),
        "{old:?}"
    );

    // After the move the file clipboard is empty.
    tree(&bench, cx, "select", "src");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none());
    assert_eq!(
        status(&bench, cx),
        "Nothing to paste: Cut or Copy a file in the project tree first"
    );

    // The folder src into a sub-folder of src: refused in the status bar.
    tree(&bench, cx, "select", "src");
    cx.simulate_keystrokes("cmd-c");
    tree(&bench, cx, "select", "src/inner");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none(), "no name field");
    assert!(
        status(&bench, cx).contains("cannot be pasted into itself"),
        "{}",
        status(&bench, cx)
    );
    assert!(!root.join("src/inner/src").exists());

    // A folder with sub-folders into the project folder: copied with everything in it, under a
    // free name beside itself.
    tree(&bench, cx, "select", "src/inner");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-c");
    tree(&bench, cx, "select", ".");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert_eq!(field(&bench, cx).as_deref(), Some("inner"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(read(&root, "inner/deep/leaf.txt"), "leaf\n");
    assert_eq!(read(&root, "src/inner/deep/leaf.txt"), "leaf\n");

    // An existing name typed into the field: refused there, the field stays open; Escape.
    tree(&bench, cx, "select", "a.txt");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-c");
    tree(&bench, cx, "select", "other");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    name(&bench, cx, "keep.txt");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (_, error) = prompt(&bench, cx).expect("the field stays open");
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.contains("already exists")),
        "{error:?}"
    );
    assert_eq!(
        read(&root, "other/keep.txt"),
        "keep\n",
        "nothing overwritten"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none());

    // After Cut, text copied in an editor: ⌘V in the tree pastes no file.
    tree(&bench, cx, "select", "a.txt");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-x");
    cx.run_until_parked();
    open(&bench, cx, &root.join("src/lib.txt"));
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    assert_eq!(clipboard(cx), "lib\n");
    tree(&bench, cx, "select", "other");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none(), "no file is pasted");
    assert!(root.join("a.txt").is_file());
    assert!(!root.join("other/a.txt").exists());
    assert_eq!(
        status(&bench, cx),
        "Nothing to paste: Cut or Copy a file in the project tree first"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ----------------------------------------------------------------------------- hand test 200

/// Hand test 200 (`SPEC-0275` v3 behaviours 24 and 25): ⇧-click, ⌘-click, ⇧↓/⇧↑, ⌘A, a plain
/// click; three files copied and pasted twice; a move onto an existing name; Copy Path of
/// several; Rename with several; Delete of several and of a folder with a file in it; the
/// Local History of a deleted path.
#[gpui::test]
fn hand_test_200_several_selected_rows(cx: &mut TestAppContext) {
    let root = workspace(
        "200",
        &[
            ("a.txt", "a\n"),
            ("b.txt", "b\n"),
            ("c.txt", "c\n"),
            ("d.txt", "d\n"),
            ("e.txt", "e\n"),
            ("f.txt", "f\n"),
            ("box/b.txt", "the other b\n"),
            ("pack/in.txt", "in\n"),
            ("pack/sub/low.txt", "low\n"),
        ],
        &["bin"],
    );
    let (bench, cx) = window(&root, cx);
    assert_eq!(
        rows(&bench, cx),
        [
            ".", "bin", "box", "pack", "a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"
        ]
    );

    // a.txt, then ⇧-click three rows further down: everything between is selected.
    click_row(cx, "a.txt", Modifiers::none());
    click_row(cx, "d.txt", Modifiers::shift());
    assert_eq!(selected(&bench, cx), ["a.txt", "b.txt", "c.txt", "d.txt"]);
    // ⌘-click adds a folder and takes a selected file away.
    click_row(cx, "bin", Modifiers::command());
    assert_eq!(
        selected(&bench, cx),
        ["bin", "a.txt", "b.txt", "c.txt", "d.txt"]
    );
    click_row(cx, "b.txt", Modifiers::command());
    assert_eq!(selected(&bench, cx), ["bin", "a.txt", "c.txt", "d.txt"]);

    // ⇧↓ twice, ⇧↑ three times: the range grows and shrinks from the anchor, the row clicked
    // last (b.txt); the lead was on the selected row nearest to it (a.txt).
    cx.simulate_keystrokes("shift-down");
    assert_eq!(selected(&bench, cx), ["b.txt"]);
    cx.simulate_keystrokes("shift-down");
    assert_eq!(selected(&bench, cx), ["b.txt", "c.txt"]);
    cx.simulate_keystrokes("shift-up shift-up shift-up");
    assert_eq!(selected(&bench, cx), ["pack", "a.txt", "b.txt"]);

    // ⌘A selects every visible row; a plain click one row again.
    cx.simulate_keystrokes("cmd-a");
    cx.run_until_parked();
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    assert!(
        state["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["selected"] == true),
        "{state}"
    );
    click_row(cx, "e.txt", Modifiers::none());
    assert_eq!(selected(&bench, cx), ["e.txt"]);

    // Three files, ⌘C, a folder, ⌘V: no name is asked, the copies are selected.
    click_row(cx, "a.txt", Modifiers::none());
    click_row(cx, "c.txt", Modifiers::shift());
    cx.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    let lines: Vec<String> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|n| root.join(n).display().to_string())
        .collect();
    assert_eq!(clipboard(cx), lines.join("\n"));
    click_row(cx, "bin", Modifiers::none());
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none(), "several items ask no name");
    assert_eq!(
        selected(&bench, cx),
        ["bin/a.txt", "bin/b.txt", "bin/c.txt"]
    );
    // The same three once more into the same folder: `_copy`.
    tree(&bench, cx, "select", "bin");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert_eq!(
        selected(&bench, cx),
        ["bin/a_copy.txt", "bin/b_copy.txt", "bin/c_copy.txt"]
    );
    assert_eq!(read(&root, "bin/a.txt"), "a\n");

    // Two files cut and pasted where one of the names exists: refused as a whole.
    tree(&bench, cx, "collapse", "bin");
    click_row(cx, "a.txt", Modifiers::none());
    click_row(cx, "b.txt", Modifiers::shift());
    cx.simulate_keystrokes("cmd-x");
    cx.run_until_parked();
    click_row(cx, "box", Modifiers::none());
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(
        status(&bench, cx).contains("already exists"),
        "{}",
        status(&bench, cx)
    );
    assert!(root.join("a.txt").is_file(), "nothing was moved");
    assert!(root.join("b.txt").is_file());
    assert!(!root.join("box/a.txt").exists());
    assert_eq!(read(&root, "box/b.txt"), "the other b\n");

    // Copy Path/Reference › Absolute Path on a selected row: one line per item.
    click_row(cx, "c.txt", Modifiers::none());
    click_row(cx, "d.txt", Modifiers::shift());
    right_click_row(cx, "d.txt");
    assert_eq!(
        selected(&bench, cx),
        ["c.txt", "d.txt"],
        "a secondary click on a selected row keeps the selection"
    );
    let (_, items) = menu(&bench, cx).unwrap();
    assert_eq!(
        enabled(&items, "Rename…"),
        Some(false),
        "Rename is off with several selected"
    );
    choose(&bench, cx, "Copy Path/Reference › Absolute Path");
    assert_eq!(
        clipboard(cx),
        format!(
            "{}\n{}",
            root.join("c.txt").display(),
            root.join("d.txt").display()
        )
    );
    open(&bench, cx, &root.join("f.txt"));
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(
        bench
            .read_with(cx, |b, cx| b.active_text(cx))
            .is_some_and(
                |t| t.contains(&format!("{}\n", root.join("c.txt").display()))
                    && t.contains(&root.join("d.txt").display().to_string())
            ),
        "the paths are pasted into the editor"
    );
    bench.update_in(cx, |b, window, cx| b.close_all_tabs_unchecked(window, cx));

    // ⇧F6 with several selected says to select one.
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("shift-f6");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none());
    assert_eq!(status(&bench, cx), "Select one file or folder to rename it");

    // ⌦ asks once for all; Enter deletes; the tab of a deleted file closes.
    open(&bench, cx, &root.join("d.txt"));
    click_row(cx, "c.txt", Modifiers::none());
    click_row(cx, "e.txt", Modifiers::shift());
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(
        bench.read_with(cx, |b, _| b.delete_question()).as_deref(),
        Some("Delete 3 items: 3 files and 0 folders?")
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    for gone in ["c.txt", "d.txt", "e.txt"] {
        assert!(!root.join(gone).exists(), "{gone}");
    }
    assert!(main_tabs(&bench, cx).is_empty(), "the tab of d.txt closed");

    // A folder and a file in it: the file counts once, with the folder.
    tree(&bench, cx, "expand", "pack");
    click_row(cx, "pack", Modifiers::none());
    click_row(cx, "in.txt", Modifiers::command());
    assert_eq!(selected(&bench, cx), ["pack", "pack/in.txt"]);
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    let question = bench
        .read_with(cx, |b, _| b.delete_question())
        .expect("Delete asks");
    assert!(
        question.contains("\"pack\"") && question.contains("2 files and 1 folder"),
        "{question}"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!root.join("pack").exists());

    // A deleted path in the Local History.
    let history = LocalHistory::open(&root).unwrap();
    let old = history.revisions("d.txt");
    assert!(
        old.iter().any(|r| history
            .content("d.txt", r.id)
            .is_ok_and(|c| c.as_deref() == Some(b"d\n".as_slice()))),
        "{old:?}"
    );
    assert!(!history.revisions("pack/sub/low.txt").is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------------- open point T1 (OBS-0031)

/// OBS-0031 finding T1 (`SPEC-0275` v11 behaviour 30, AK-28) in the window: a project without
/// a build file gets its source folder from its files, in the background, and the packages
/// below it stand as the reference showed them: `lab`, `empty.a.b`, `inventory`, `util.deep`,
/// `two`, `x`, `y`; `docs/one/two` outside the sources stays folder by folder.
#[gpui::test]
fn t1_packages_are_compacted_without_a_build_file(cx: &mut TestAppContext) {
    let java = "src/main/java";
    let class = |package: &str, name: &str| {
        (
            format!("{java}/{}/{name}.java", package.replace('.', "/")),
            format!("package {package};\n\npublic class {name} {{\n}}\n"),
        )
    };
    let classes = [
        class("lab.inventory", "Inventory"),
        class("lab.inventory.util.deep", "Leaf"),
        class("lab.two.x", "X"),
        class("lab.two.y", "Y"),
    ];
    let mut files: Vec<(&str, &str)> = classes
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    files.push(("docs/one/two/note.txt", "note\n"));
    let root = workspace("t1", &files, &["src/main/java/lab/empty/a/b"]);
    let (bench, cx) = window(&root, cx);
    let role = |bench: &Bench, cx: &mut VisualTestContext| {
        let state = bench.read_with(cx, |b, _| b.project_tree_state());
        state["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["path"] == java)
            .map(|r| r["role"].clone())
    };
    for dir in ["src", "src/main", "docs", "docs/one"] {
        tree(&bench, cx, "expand", dir);
    }
    until(cx, "the source folder is recognised from the files", |cx| {
        role(&bench, cx) == Some(serde_json::json!("sources"))
    });
    for dir in [
        java,
        "src/main/java/lab",
        "src/main/java/lab/inventory",
        "src/main/java/lab/two",
    ] {
        tree(&bench, cx, "expand", dir);
    }
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    let names: Vec<(u64, &str)> = state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "folder")
        .map(|r| (r["depth"].as_u64().unwrap(), r["name"].as_str().unwrap()))
        .collect();
    let below_java: Vec<(u64, &str)> = names
        .iter()
        .copied()
        .skip_while(|(_, n)| *n != "java")
        .skip(1)
        .take_while(|(d, _)| *d > 3)
        .collect();
    assert_eq!(
        below_java,
        [
            (4, "lab"),
            (5, "empty.a.b"),
            (5, "inventory"),
            (6, "util.deep"),
            (5, "two"),
            (6, "x"),
            (6, "y")
        ],
        "{state}"
    );
    assert!(
        names.contains(&(2, "one")) && names.contains(&(3, "two")),
        "outside the sources folders stay one by one: {names:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------- file operations and split groups

/// The tabs of every group as `group id: [paths relative to the project folder]`.
fn group_tabs(bench: &Bench, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    let state = bench.read_with(cx, |b, cx| b.editor_tabs_state(cx));
    state["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            g["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["path"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .collect()
}

/// `SPEC-0275` behaviours 5, 6, 20 and 21 hold for every editor group (`SPEC-0112`): tabs of a
/// deleted file close in each group, a tab in a second group follows a moved or renamed file,
/// and unsaved changes shown only in a second group keep the file from being renamed or moved.
#[gpui::test]
fn file_operations_reach_the_tabs_of_every_group(cx: &mut TestAppContext) {
    let root = workspace(
        "groups",
        &[
            ("one.txt", "one\n"),
            ("two.txt", "two\n"),
            ("three.txt", "three\n"),
            ("four.txt", "four\n"),
        ],
        &["dir"],
    );
    let (bench, cx) = window(&root, cx);
    let action = |bench: &Bench, cx: &mut VisualTestContext, id: &str| {
        bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
        cx.run_until_parked();
    };
    open(&bench, cx, &root.join("one.txt"));
    open(&bench, cx, &root.join("two.txt"));
    open(&bench, cx, &root.join("three.txt"));
    open(&bench, cx, &root.join("four.txt"));
    // four.txt only in the second group, three.txt in both.
    action(&bench, cx, "SplitAndMoveRight");
    action(&bench, cx, "PrevSplitter");
    action(&bench, cx, "SplitVertically");
    action(&bench, cx, "MoveEditorToOppositeTabGroup");
    assert_eq!(
        group_tabs(&bench, cx),
        [
            vec!["one.txt", "two.txt", "three.txt"],
            vec!["four.txt", "three.txt"]
        ]
    );

    // Cut and Paste of the file shown only in the second group: its tab follows.
    tree(&bench, cx, "select", "four.txt");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-x");
    tree(&bench, cx, "select", "dir");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("dir/four.txt").is_file());
    assert_eq!(
        group_tabs(&bench, cx),
        [
            vec!["one.txt", "two.txt", "three.txt"],
            vec!["three.txt", "dir/four.txt"]
        ],
        "the second group's tab follows the moved file"
    );

    // Rename… of it: the second group's tab follows the new name.
    bench.update_in(cx, |b, window, cx| {
        b.start_rename(root.join("dir/four.txt"), window, cx)
    });
    cx.run_until_parked();
    name(&bench, cx, "fourth.txt");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("dir/fourth.txt").is_file());
    let tabs = group_tabs(&bench, cx);
    assert!(
        tabs.iter().flatten().all(|t| t != "dir/four.txt"),
        "no tab is left for the old name: {tabs:?}"
    );
    assert!(
        tabs.iter().flatten().any(|t| t == "dir/fourth.txt"),
        "{tabs:?}"
    );

    // Unsaved changes shown only in a second group: Rename is refused in the field, Cut and
    // Paste in the status bar.
    let second = bench.read_with(cx, |b, _| b.group_ids()[1]);
    let three = root.join("three.txt");
    let tabs = |bench: &Bench, cx: &mut VisualTestContext, action: &str, group: usize| {
        bench.update_in(cx, |b, window, cx| {
            b.editor_tabs_call(
                &serde_json::json!({"action": action, "group": group, "path": three}),
                window,
                cx,
            )
            .unwrap_or_else(|e| panic!("{action}: {e}"));
        });
        cx.run_until_parked();
    };
    tabs(&bench, cx, "close", MAIN);
    tabs(&bench, cx, "activate", second);
    cx.simulate_input("z");
    cx.run_until_parked();
    assert_eq!(
        bench.read_with(cx, |b, cx| b.active_text(cx)).as_deref(),
        Some("zthree\n")
    );
    bench.update_in(cx, |b, window, cx| {
        b.start_rename(three.clone(), window, cx)
    });
    cx.run_until_parked();
    name(&bench, cx, "third.txt");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (_, error) = prompt(&bench, cx).expect("the field stays open");
    assert_eq!(
        error.as_deref(),
        Some("Save the open file first; it has changes")
    );
    assert!(three.is_file(), "the file was not renamed");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    tree(&bench, cx, "select", "three.txt");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-x");
    tree(&bench, cx, "select", "dir");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert!(prompt(&bench, cx).is_none());
    assert_eq!(
        status(&bench, cx),
        "Save the open file first; it has changes"
    );
    assert!(three.is_file(), "the file was not moved");

    // Delete… of a file open in two groups: both tabs close, and a group left empty with it.
    tabs(&bench, cx, "activate", second);
    action(&bench, cx, "$Undo");
    open(&bench, cx, &three);
    let before = group_tabs(&bench, cx);
    assert!(
        before.iter().all(|g| g.contains(&"three.txt".to_owned())),
        "{before:?}"
    );
    bench.update_in(cx, |b, window, cx| {
        b.ask_delete(std::slice::from_ref(&three), window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!three.exists());
    let after = group_tabs(&bench, cx);
    assert!(
        after.iter().flatten().all(|t| t != "three.txt"),
        "no group keeps a tab of the deleted file: {after:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ----------------------------------------------------------------------------- hand test 201

const K_KT: &str = "package org.x\n\nfun k() = 1\n";

/// The paths of the main group's tabs, relative to the project folder.
fn tab_paths(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    let state = bench.read_with(cx, |b, cx| b.editor_tabs_state(cx));
    state["groups"][0]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["path"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// Hand test 201 (`SPEC-0275` v3 behaviour 26): a cut Java class pasted into another package, a
/// cut Kotlin file, a copied Java file. The package lines follow, open tabs follow, the status
/// bar reports the move and the old path is in the Local History. The import in the other class
/// is the language server's edit: checked only when a Java language server answered.
#[gpui::test]
fn hand_test_201_the_package_follows_a_pasted_class(cx: &mut TestAppContext) {
    let root = workspace(
        "201",
        &[
            ("build.gradle", "plugins { id 'java' }\n"),
            ("src/main/java/com/acme/A.java", A_JAVA),
            ("src/main/java/com/acme/B.java", B_JAVA),
            ("src/main/kotlin/org/x/K.kt", K_KT),
        ],
        &[
            "src/main/java/com/acme/util",
            "src/main/java/com/other",
            "src/main/kotlin/org/x/sub",
        ],
    );
    let (bench, cx) = window(&root, cx);
    let java = "src/main/java/com/acme";
    open(&bench, cx, &root.join(java).join("A.java"));
    open(&bench, cx, &root.join(java).join("B.java"));

    // A.java, ⌘X, the folder util, ⌘V, Enter.
    tree(&bench, cx, "select", &format!("{java}/A.java"));
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-x");
    tree(&bench, cx, "select", &format!("{java}/util"));
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    let (title, _) = prompt(&bench, cx).expect("Paste opens the name field");
    assert_eq!(title, format!("Move A.java to {java}/util"));
    cx.simulate_keystrokes("enter");
    until(cx, "the status bar reports the move", |cx| {
        status(&bench, cx).starts_with("Moved ")
    });
    let report = status(&bench, cx);
    assert!(
        report.starts_with(&format!("Moved to {java}/util/A.java; ")),
        "{report}"
    );
    let a = read(&root, &format!("{java}/util/A.java"));
    assert!(a.starts_with("package com.acme.util;\n"), "{a}");
    assert!(!root.join(java).join("A.java").exists());
    // The open tab followed, and the tree selects the file at its new place.
    let tabs = tab_paths(&bench, cx);
    assert!(tabs.contains(&format!("{java}/util/A.java")), "{tabs:?}");
    assert!(!tabs.contains(&format!("{java}/A.java")), "{tabs:?}");
    assert_eq!(selected(&bench, cx), [format!("{java}/util/A.java")]);
    // The other class imports the moved one: the language server's edit, made in its open
    // editor. Without a Java language server only the package line follows.
    if report.contains("the language server") {
        open(&bench, cx, &root.join(java).join("B.java"));
        let b = bench.read_with(cx, |b, cx| b.active_text(cx)).unwrap();
        assert!(b.contains("import com.acme.util.A;"), "{b}");
        assert!(report.contains("2 file(s) updated"), "{report}");
    } else {
        eprintln!("no Java language server answered: the import in B.java was not checked");
    }
    // The old path in the Local History, with the label of the move.
    let history = LocalHistory::open(&root).unwrap();
    let old = history.revisions(&format!("{java}/A.java"));
    assert!(
        old.iter()
            .any(|r| r.label.as_deref().is_some_and(|l| l.starts_with("Moved "))),
        "{old:?}"
    );

    // A Kotlin file cut and pasted into a sub-package: only its package line changes.
    let kotlin = "src/main/kotlin/org/x";
    tree(&bench, cx, "select", &format!("{kotlin}/K.kt"));
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-x");
    tree(&bench, cx, "select", &format!("{kotlin}/sub"));
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    until(cx, "the status bar reports the Kotlin move", |cx| {
        status(&bench, cx).starts_with(&format!("Moved to {kotlin}/sub/K.kt"))
    });
    assert_eq!(
        read(&root, &format!("{kotlin}/sub/K.kt")),
        "package org.x.sub\n\nfun k() = 1\n"
    );

    // A Java file copied (⌘C) into another package: the copy gets the package of its folder,
    // the original stays as it is.
    tree(&bench, cx, "select", &format!("{java}/B.java"));
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-c");
    tree(&bench, cx, "select", "src/main/java/com/other");
    focus_tree(&bench, cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    let (title, _) = prompt(&bench, cx).expect("Paste opens the name field");
    assert_eq!(title, "Copy B.java to src/main/java/com/other");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let copy = read(&root, "src/main/java/com/other/B.java");
    assert!(copy.starts_with("package com.other;\n"), "{copy}");
    assert!(root.join(java).join("B.java").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------------------------ the Switcher

fn switcher(bench: &Bench, cx: &mut VisualTestContext) -> Option<serde_json::Value> {
    bench.read_with(cx, |b, cx| b.switcher_state(cx))
}

fn labels(state: &serde_json::Value, column: &str) -> Vec<String> {
    state[column]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// `SPEC-0059` v9.4 behaviour 27 (AK-15), the open point "the Switcher in detail" of OBS-0031:
/// ⌃⇥ opens two columns — the tool windows with their characters and Recent Locations, the
/// recently used files — with the previous file selected and its folder below; ⌃⇥ walks,
/// ← and → change the column, releasing ⌃ opens the selection.
#[gpui::test]
fn the_switcher_shows_two_columns_and_follows_the_keys(cx: &mut TestAppContext) {
    let root = workspace(
        "switcher",
        &[
            ("one.txt", "one\n"),
            ("src/a/Main.java", "class Main {}\n"),
            ("src/b/Main.java", "class Main {}\n"),
        ],
        &[],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("one.txt"));
    open(&bench, cx, &root.join("src/a/Main.java"));
    open(&bench, cx, &root.join("src/b/Main.java"));
    assert!(switcher(&bench, cx).is_none());

    cx.simulate_keystrokes("ctrl-tab");
    cx.run_until_parked();
    let state = switcher(&bench, cx).expect("⌃⇥ opens the Switcher");
    // Right: the files, most recent first, equal names with their folder.
    assert_eq!(
        labels(&state, "right"),
        ["Main.java — b", "Main.java — a", "one.txt"]
    );
    assert_eq!(state["selected"], "Main.java — a", "the file used before");
    assert_eq!(state["folder"], "src/a");
    // Left: the tool windows with their characters, then Recent Locations with its shortcut.
    let left = state["left"].as_array().unwrap();
    assert_eq!(left[0]["label"], "Project");
    assert_eq!(left[0]["character"], "1", "⌘1 in the macOS keymap");
    let last = left.last().unwrap();
    assert_eq!(last["label"], "Recent Locations");
    assert_eq!(last["shortcut"], "⇧⌘E");
    assert!(last["character"].is_null());
    let characters: Vec<&str> = left
        .iter()
        .filter_map(|r| r["character"].as_str())
        .collect();
    assert_eq!(
        characters.len(),
        left.len() - 1,
        "every tool window has a character: {left:?}"
    );
    let mut unique = characters.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), characters.len(), "{characters:?}");
    for tool in ["Terminal", "Database", "Structure", "Bookmarks"] {
        assert!(labels(&state, "left").contains(&tool.to_owned()), "{tool}");
    }

    // ⌃⇥ again walks on; a file directly in the project folder names that folder.
    cx.simulate_keystrokes("ctrl-tab");
    let state = switcher(&bench, cx).unwrap();
    assert_eq!(state["selected"], "one.txt");
    assert_eq!(
        state["folder"],
        root.file_name().unwrap().to_string_lossy().as_ref()
    );
    // ⌃⇧⇥ walks back.
    cx.simulate_keystrokes("ctrl-shift-tab");
    assert_eq!(switcher(&bench, cx).unwrap()["selected"], "Main.java — a");

    // ← to the tool windows at the same height (no folder line there), → back to the files.
    cx.simulate_keystrokes("left");
    let state = switcher(&bench, cx).unwrap();
    assert_eq!(state["selected"], "Commit");
    assert_eq!(state["folder"], "");
    cx.simulate_keystrokes("down");
    assert_eq!(switcher(&bench, cx).unwrap()["selected"], "Git");
    cx.simulate_keystrokes("right");
    assert_eq!(switcher(&bench, cx).unwrap()["selected"], "one.txt");
    cx.simulate_keystrokes("up");
    assert_eq!(switcher(&bench, cx).unwrap()["selected"], "Main.java — a");

    // Releasing ⌃ opens the selected file.
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(switcher(&bench, cx).is_none(), "the Switcher closed");
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("src/a/Main.java"))
    );

    // Escape closes without a choice.
    cx.simulate_keystrokes("ctrl-tab");
    assert!(switcher(&bench, cx).is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(switcher(&bench, cx).is_none());
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("src/a/Main.java"))
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0059` v9.4 behaviours 27b, 27e and 27f (AK-15): ⌃ with a tool window's character
/// shows the tool window, also when the menu bar took the key for another action; ⇧Enter
/// opens the selected file in a group on the right; Recent Locations opens its popup.
#[gpui::test]
fn the_switcher_takes_characters_and_opens_in_a_split(cx: &mut TestAppContext) {
    use ferris_idea_ui::tool_windows::ToolWindow;
    let root = workspace(
        "switcher-keys",
        &[
            ("one.txt", "one\n"),
            ("two.txt", "two\n"),
            ("three.txt", "three\n"),
        ],
        &[],
    );
    let (bench, cx) = window(&root, cx);
    for file in ["one.txt", "two.txt", "three.txt"] {
        open(&bench, cx, &root.join(file));
    }
    let shown = |bench: &Bench, cx: &mut VisualTestContext, w: ToolWindow| {
        bench.read_with(cx, |b, _| b.tool_window_shown(w))
    };

    // ⌃6: the Problems tool window (⌘6 in the macOS keymap).
    assert!(!shown(&bench, cx, ToolWindow::Problems));
    cx.simulate_keystrokes("ctrl-tab");
    let state = switcher(&bench, cx).expect("the Switcher is open");
    let problems = state["left"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["label"] == "Problems")
        .expect("Problems is listed")
        .clone();
    assert_eq!(problems["character"], "6");
    cx.simulate_keystrokes("ctrl-6");
    cx.run_until_parked();
    assert!(switcher(&bench, cx).is_none(), "the Switcher closed");
    assert!(shown(&bench, cx, ToolWindow::Problems));
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("three.txt")),
        "no file was switched to"
    );

    // The menu bar took ⌃1 for the action bound to it: the Switcher's row `1` is chosen and
    // the action does not run.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("ctrl-tab");
    assert!(switcher(&bench, cx).is_some());
    let taken = ferris_idea_workbench::keymaps::current()
        .actions()
        .find(|(_, shortcuts)| {
            shortcuts.iter().any(|s| {
                matches!(s, ferris_idea_ui::keymap::Shortcut::Keyboard { first, second: None }
                    if first.key == "1"
                        && first.modifiers == [ferris_idea_ui::keymap::Modifier::Ctrl])
            })
        })
        .map(|(id, _)| id.to_owned())
        .expect("the macOS keymap binds ⌃1");
    bench.update_in(cx, |b, window, cx| b.menu_action(&taken, window, cx));
    cx.run_until_parked();
    assert!(switcher(&bench, cx).is_none(), "the Switcher closed");
    assert!(
        cx.update(|window, cx| bench.read(cx).tree_focus().is_focused(window)),
        "the Project tool window has the focus"
    );

    // ⇧Enter on a file the main group shows: a second group on the right with it, focused.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(switcher(&bench, cx).unwrap()["selected"], "two.txt");
    cx.simulate_keystrokes("shift-enter");
    cx.run_until_parked();
    assert!(switcher(&bench, cx).is_none());
    assert_eq!(
        group_tabs(&bench, cx),
        [vec!["one.txt", "two.txt", "three.txt"], vec!["two.txt"]]
    );
    assert!(bench.read_with(cx, |b, _| b.split_active()));

    // ⇧Enter on a recent file that has no tab: it opens only in the new group.
    bench.update_in(cx, |b, window, cx| {
        b.unsplit_all(window, cx);
        b.editor_tabs_call(
            &serde_json::json!({"action": "close", "group": MAIN, "path": root.join("one.txt")}),
            window,
            cx,
        )
        .unwrap();
    });
    cx.run_until_parked();
    assert_eq!(group_tabs(&bench, cx), [vec!["two.txt", "three.txt"]]);
    cx.simulate_keystrokes("ctrl-tab");
    let state = switcher(&bench, cx).unwrap();
    assert!(
        labels(&state, "right").contains(&"one.txt".to_owned()),
        "a closed recent file is listed: {state}"
    );
    while switcher(&bench, cx).unwrap()["selected"] != "one.txt" {
        cx.simulate_keystrokes("ctrl-tab");
    }
    // With ⌃ still held, as the Switcher is used.
    cx.simulate_keystrokes("ctrl-shift-enter");
    cx.run_until_parked();
    assert_eq!(
        group_tabs(&bench, cx),
        [vec!["two.txt", "three.txt"], vec!["one.txt"]]
    );
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("one.txt"))
    );

    // Recent Locations: the entry closes the Switcher and opens that popup.
    cx.simulate_keystrokes("ctrl-tab");
    let chosen = bench.update_in(cx, |b, window, cx| {
        b.choose_tab_list_row("Recent Locations", window, cx)
    });
    assert!(chosen);
    cx.run_until_parked();
    assert!(switcher(&bench, cx).is_none());
    assert!(
        bench.read_with(cx, |b, _| b.goto_popup().is_some()),
        "Recent Locations is open"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------------ PARITY F-017 gap N1

/// The caret's place in the active editor, 0-based, and the active file's name.
fn place(bench: &Bench, cx: &mut VisualTestContext) -> (String, usize, usize) {
    bench.read_with(cx, |b, cx| {
        let name = b
            .active_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let s = b.active_editor_entity().unwrap().read(cx).status();
        (name, s.line - 1, s.column - 1)
    })
}

fn caret_to(bench: &Bench, cx: &mut VisualTestContext, line: usize, column: usize) {
    bench.update_in(cx, |b, _, cx| {
        b.active_editor_entity()
            .unwrap()
            .update(cx, |e, cx| e.go_to(line, column, cx));
    });
    cx.run_until_parked();
}

/// SPEC-0071 AK-15 (v13, behaviour 4a), gap N1 of PARITY row F-017: a file opened by a double
/// click in the Project tree is a jump — Back returns to the file before it at its caret, Forward
/// to the opened file at its caret; a file opened from Recent Files is a jump too.
#[gpui::test]
fn back_returns_from_a_file_opened_in_the_tree(cx: &mut TestAppContext) {
    let root = workspace(
        "n1",
        &[
            (
                "src/shop/Main.txt",
                "class Main {\n  void run() {\n    go();\n  }\n}\n",
            ),
            (
                "src/shop/Inventory.txt",
                "class Inventory {\n  int count;\n}\n",
            ),
        ],
        &[],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("src/shop/Main.txt"));
    caret_to(&bench, cx, 2, 4);
    tree(&bench, cx, "expand", "src");
    tree(&bench, cx, "expand", "src/shop");

    // A real double click on the row of Inventory.txt.
    let at = row_at(cx, "Inventory.txt");
    for click_count in [1, 2] {
        cx.simulate_event(gpui::MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    cx.run_until_parked();
    assert_eq!(
        place(&bench, cx).0,
        "Inventory.txt",
        "the double click opens the file"
    );
    caret_to(&bench, cx, 1, 2);

    let action = |bench: &Bench, cx: &mut VisualTestContext, id: &str| {
        bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
        cx.run_until_parked();
    };
    action(&bench, cx, "Back");
    assert_eq!(
        place(&bench, cx),
        ("Main.txt".to_owned(), 2, 4),
        "Back returns to the file before and its caret"
    );
    action(&bench, cx, "Forward");
    assert_eq!(
        place(&bench, cx),
        ("Inventory.txt".to_owned(), 1, 2),
        "Forward returns to the opened file and its caret"
    );

    // Recent Files: Enter right away opens the previous file (SPEC-0069 behaviour 4a); Back
    // returns.
    action(&bench, cx, "RecentFiles");
    assert!(
        bench.read_with(cx, |b, _| b.goto_popup().is_some()),
        "the popup is open"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        place(&bench, cx).0,
        "Main.txt",
        "Recent Files opens the previous file"
    );
    action(&bench, cx, "Back");
    assert_eq!(
        place(&bench, cx),
        ("Inventory.txt".to_owned(), 1, 2),
        "Back returns from a file opened from Recent Files"
    );
    let _ = std::fs::remove_dir_all(&root);
}
