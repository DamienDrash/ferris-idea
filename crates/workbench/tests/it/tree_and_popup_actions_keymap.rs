//! The fourth package of actions taken from the keymap, carried out in a window of the test
//! harness: Expand All and Collapse All of the Project tree, New in the same place and Open in
//! Right Split (`SPEC-0275` v13 behaviours 33, 35 and 36), the Text tab of Search Everywhere
//! (`SPEC-0067` v8 behaviour 19), the query history (`SPEC-0093` v5 behaviour 16) and the recent
//! searches of the find bar (`SPEC-0065` v5 behaviour 19).

#![cfg(target_os = "macos")]

use ferris_idea_workbench::everywhere_model::Filter;
use ferris_idea_workbench::{MenuPlace, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-fourth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/inner")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/inner/deep.js"), "let deep = 1;\n").unwrap();
    std::fs::write(root.join("src/main.js"), "let a = 1;\nlet b = 2;\n").unwrap();
    std::fs::write(root.join("docs/notes.txt"), "notes\n").unwrap();
    // The directory as the workbench names it, so paths compare equal.
    root.canonicalize().unwrap()
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join("src/main.js");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

fn act(bench: &Entity<WorkbenchView>, id: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

/// The names of the Project tree's visible rows.
fn rows(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    state["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// SPEC-0275 AK-31 (v13): Expand All opens the folders of the Project tree, Collapse All closes
/// them; with the tree in focus the menu bar's folding items do the same.
#[gpui::test]
fn expand_all_and_collapse_all_act_on_the_project_tree(cx: &mut TestAppContext) {
    let root = workspace("all");
    let (bench, cx) = open(&root, cx);
    act(&bench, "CollapseAll", cx);
    let closed = rows(&bench, cx);
    assert!(!closed.contains(&"deep.js".to_owned()), "{closed:?}");
    assert!(!closed.contains(&"main.js".to_owned()), "{closed:?}");

    // The project folder is selected: everything below it opens.
    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(&serde_json::json!({"action": "select", "path": root}), cx)
            .expect("the project folder is in the tree");
    });
    act(&bench, "ExpandAll", cx);
    let opened = rows(&bench, cx);
    for name in ["deep.js", "main.js", "notes.txt"] {
        assert!(opened.contains(&name.to_owned()), "{name}: {opened:?}");
    }

    // With the tree in focus the key the menu bar took for Collapse belongs to the tree.
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
    act(&bench, "CollapseRegion", cx);
    assert_eq!(rows(&bench, cx), closed);
    act(&bench, "ExpandRegion", cx);
    assert_eq!(rows(&bench, cx), opened);

    // Expanding the selected folder with everything below it, as numpad * does.
    act(&bench, "CollapseAll", cx);
    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(
            &serde_json::json!({"action": "select", "path": root.join("src")}),
            cx,
        )
        .expect("src is in the tree");
    });
    act(&bench, "FullyExpandTreeNode", cx);
    let below = rows(&bench, cx);
    assert!(below.contains(&"deep.js".to_owned()), "{below:?}");
    assert!(!below.contains(&"notes.txt".to_owned()), "{below:?}");
    act(&bench, "CollapseTreeNode", cx);
    assert!(!rows(&bench, cx).contains(&"main.js".to_owned()));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0275 AK-32 (v13): New in the same place opens the New menu for the folder of the active
/// file, whatever the tree has selected.
#[gpui::test]
fn new_in_the_same_place_opens_the_new_menu(cx: &mut TestAppContext) {
    let root = workspace("new");
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(
            &serde_json::json!({"action": "select", "path": root.join("docs")}),
            cx,
        )
        .expect("docs is in the tree");
    });
    act(&bench, "NewElementSamePlace", cx);
    let (place, target) = bench.read_with(cx, |b, _| {
        (
            b.context_menu_items().map(|(place, _)| place),
            b.context_menu_target(),
        )
    });
    assert_eq!(place, Some(MenuPlace::New));
    assert_eq!(target, Some(root.join("src")));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0275 AK-32 (v13): Open in Right Split puts the file selected in the tree into a second
/// editor group; on a folder it says what it needs.
#[gpui::test]
fn the_trees_file_opens_in_the_right_split(cx: &mut TestAppContext) {
    let root = workspace("split");
    let (bench, cx) = open(&root, cx);
    assert_eq!(bench.read_with(cx, |b, _| b.group_ids().len()), 1);
    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(
            &serde_json::json!({"action": "select", "path": root.join("docs")}),
            cx,
        )
        .expect("docs is in the tree");
    });
    act(&bench, "OpenInRightSplit", cx);
    assert_eq!(bench.read_with(cx, |b, _| b.group_ids().len()), 1);
    let note = bench.read_with(cx, |b, _| b.status_note().map(str::to_owned));
    assert!(
        note.as_deref().is_some_and(|n| n.contains("Select a file")),
        "{note:?}"
    );

    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(
            &serde_json::json!({"action": "expand", "path": root.join("docs")}),
            cx,
        )
        .expect("docs is in the tree");
        b.project_tree_call(
            &serde_json::json!({"action": "select", "path": root.join("docs/notes.txt")}),
            cx,
        )
        .expect("the file is in the tree");
    });
    act(&bench, "OpenInRightSplit", cx);
    let (groups, titles) = bench.read_with(cx, |b, _| {
        let ids = b.group_ids();
        let titles: Vec<Vec<String>> = ids.iter().map(|g| b.group_titles(*g)).collect();
        (ids.len(), titles)
    });
    assert_eq!(groups, 2, "{titles:?}");
    assert!(
        titles.iter().any(|t| t == &["notes.txt".to_owned()]),
        "{titles:?}"
    );
    assert!(
        titles.iter().any(|t| t == &["main.js".to_owned()]),
        "{titles:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0067 AK-9 (v8): the action opens Search Everywhere on its Text tab.
#[gpui::test]
fn text_search_opens_the_text_tab(cx: &mut TestAppContext) {
    let root = workspace("text");
    let (bench, cx) = open(&root, cx);
    assert!(bench.read_with(cx, |b, _| b.everywhere().is_none()));
    act(&bench, "TextSearchAction", cx);
    let filter = bench.read_with(cx, |b, cx| b.everywhere().map(|v| v.read(cx).filter()));
    assert_eq!(filter, Some(Filter::Text));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0093 AK-12 (v5), SPEC-0065 AK-7 (v5): the query history needs a SQL file and the recent
/// searches an open find bar; with the bar open the action and ⌥↓ in its field show them.
#[gpui::test]
fn history_actions_need_their_place(cx: &mut TestAppContext) {
    let root = workspace("history");
    let (bench, cx) = open(&root, cx);
    let note =
        |cx: &mut VisualTestContext| bench.read_with(cx, |b, _| b.status_note().map(str::to_owned));

    act(&bench, "Console.History.Browse", cx);
    assert!(!bench.read_with(cx, |b, _| b.history_open()));
    let said = note(cx);
    assert!(
        said.as_deref()
            .is_some_and(|n| n.contains("Browse Query History")),
        "{said:?}"
    );

    act(&bench, "ShowSearchHistory", cx);
    assert!(!bench.read_with(cx, |b, _| b.search_history_shown()));
    let said = note(cx);
    assert!(
        said.as_deref()
            .is_some_and(|n| n.contains("Search History")),
        "{said:?}"
    );

    act(&bench, "Find", cx);
    assert!(!bench.read_with(cx, |b, _| b.search_history_shown()));
    act(&bench, "ShowSearchHistory", cx);
    assert!(bench.read_with(cx, |b, _| b.search_history_shown()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!bench.read_with(cx, |b, _| b.search_history_shown()));
    // The key of the action, pressed in the find field.
    cx.simulate_keystrokes("alt-down");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.search_history_shown()));
    let _ = std::fs::remove_dir_all(&root);
}
