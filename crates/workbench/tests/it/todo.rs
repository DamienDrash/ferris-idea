//! Window checks of the TODO tool window (`SPEC-0334` AK-9): the action that shows it, the tree
//! after the scan in the background, the jump to an item, the occurrence actions, the scopes,
//! the grouping keys that belong to the window's focus, and the tree following edits, saves and
//! changes on disk. No language server takes part; nothing is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_ui::tool_windows::{Side, ToolWindow};
use ferris_idea_workbench::todo_model::{Filter, Node, Scope, count_label, totals_label};
use ferris_idea_workbench::{EditorView, TodoToolView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const CART: &str = "package shop;

public class Cart {
    // TODO total with tax
    int total() { return 0; }

    /* FIXME clear the cart */
    void clear() {}
}
";

const FILE: &str = "app/src/main/java/shop/Cart.java";

fn workspace(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-todo-win-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app/src/main/java/shop")).unwrap();
    std::fs::write(root.join(FILE), CART).unwrap();
    // A module by its build file (one that starts no tooling when a Java file opens).
    std::fs::write(root.join("app/go.mod"), "module app\n").unwrap();
    std::fs::write(
        root.join("app/settings.yaml"),
        "# TODO version\nname: app\n",
    )
    .unwrap();
    std::fs::write(root.join("notes.txt"), "TODO buy milk\n").unwrap();
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

/// Lets the window's clock and the background work run until `done`.
fn wait(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..400 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}

/// Shows the TODO window and waits for the first scan.
fn show(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<TodoToolView> {
    action(bench, cx, "ActivateTODOToolWindow");
    let tool = bench
        .read_with(cx, |b, _| b.todo_tool_view())
        .expect("the TODO window");
    wait(cx, "the first scan", |cx| {
        tool.read_with(cx, |t, _| t.scanned())
    });
    tool
}

/// The visible rows as indented lines.
fn rows(tool: &Entity<TodoToolView>, cx: &mut VisualTestContext) -> Vec<String> {
    tool.read_with(cx, |t, _| {
        t.nodes()
            .iter()
            .map(|n| {
                let text = match n {
                    Node::Project { items, files, .. } => {
                        format!("project ({})", totals_label(*items, *files))
                    }
                    Node::Module { name, items, .. } => {
                        format!("[{name}] ({})", count_label(*items))
                    }
                    Node::Folder { name, items, .. } => format!("{name}/ ({items})"),
                    Node::File {
                        name,
                        folder,
                        items,
                        ..
                    } if folder.is_empty() => format!("{name} ({items})"),
                    Node::File {
                        name,
                        folder,
                        items,
                        ..
                    } => format!("{name} <{folder}> ({items})"),
                    Node::Item { line, text, .. } => format!("{} {text}", line + 1),
                };
                format!("{}{text}", "  ".repeat(n.depth()))
            })
            .collect()
    })
}

/// Items shown in the Project scope, from the top row.
fn total(tool: &Entity<TodoToolView>, cx: &mut VisualTestContext) -> usize {
    tool.read_with(cx, |t, _| match t.nodes().first() {
        Some(Node::Project { items, .. }) => *items,
        _ => 0,
    })
}

fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    editor.read_with(cx, |e, _| (e.status().line - 1, e.status().column - 1))
}

/// SPEC-0334 AK-9: the action shows the window at the bottom with its tree; Enter opens an
/// item; the occurrence actions step through the items; the scopes and the filter.
#[gpui::test]
fn the_todo_window_lists_and_opens_items(cx: &mut TestAppContext) {
    let root = workspace("list");
    let (bench, cx) = window(&root, cx);
    let tool = show(&bench, cx);
    assert!(bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Todo)));
    assert_eq!(
        bench.read_with(cx, |b, _| b.tool_layout().placement(ToolWindow::Todo).side),
        Side::Bottom
    );
    assert!(!ToolWindow::Todo.on_sidebar_by_default());
    assert!(tool.read_with(cx, |t, _| t.preview_on() && t.message().is_none()));
    assert_eq!(
        rows(&tool, cx),
        [
            "project (4 items in 3 files)",
            "  app/ (3)",
            "    src/main/java/shop/ (2)",
            "      Cart.java (2)",
            "        4 // TODO total with tax",
            "        7 /* FIXME clear the cart */",
            "    settings.yaml (1)",
            "      1 # TODO version",
            "  notes.txt (1)",
            "    1 TODO buy milk",
        ]
    );

    // ↓ to the first item, Enter: the file opens with the caret at the word.
    cx.simulate_keystrokes("down down down down enter");
    cx.run_until_parked();
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join(FILE))
    );
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open");
    assert_eq!(caret(&editor, cx), (3, 7));

    // Next and previous occurrence go through the items and stop at the ends.
    action(&bench, cx, "NextOccurence");
    assert_eq!(caret(&editor, cx), (6, 7));
    action(&bench, cx, "PreviousOccurence");
    assert_eq!(caret(&editor, cx), (3, 7));
    action(&bench, cx, "PreviousOccurence");
    assert_eq!(caret(&editor, cx), (3, 7));
    action(&bench, cx, "NextOccurence");
    action(&bench, cx, "NextOccurence");
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("app/settings.yaml"))
    );
    assert!(bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Todo)));

    // A closed node hides its rows; the next occurrence opens what it needs.
    cx.simulate_keystrokes("up left");
    cx.run_until_parked();
    assert_eq!(
        rows(&tool, cx)[6..8],
        ["    settings.yaml (1)", "  notes.txt (1)"]
    );
    action(&bench, cx, "NextOccurence");
    assert_eq!(
        rows(&tool, cx)[6..8],
        ["    settings.yaml (1)", "      1 # TODO version"]
    );
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("app/settings.yaml"))
    );
    action(&bench, cx, "NextOccurence");
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("notes.txt"))
    );
    // At the last item nothing further happens.
    action(&bench, cx, "NextOccurence");
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("notes.txt"))
    );

    // The filter keeps one pattern.
    tool.update(cx, |t, cx| t.press("filter-fixme", cx));
    assert_eq!(tool.read_with(cx, |t, _| t.filter()), Filter::ALL[2]);
    assert_eq!(
        rows(&tool, cx),
        [
            "project (1 item in 1 file)",
            "  app/src/main/java/shop/ (1)",
            "    Cart.java (1)",
            "      7 /* FIXME clear the cart */",
        ]
    );
    tool.update(cx, |t, cx| t.press("filter-all", cx));

    // Current File follows the active editor.
    bench.update(cx, |b, cx| b.set_todo_scope(Scope::File, cx));
    wait(cx, "the current file's items", |cx| {
        rows(&tool, cx).len() == 2
    });
    assert_eq!(rows(&tool, cx), ["notes.txt (1)", "  1 TODO buy milk"]);
    open(&bench, cx, &root.join(FILE));
    wait(cx, "the items of the other file", |cx| {
        rows(&tool, cx).len() == 3
    });
    assert_eq!(
        rows(&tool, cx),
        [
            "Cart.java <app/src/main/java/shop> (2)",
            "  4 // TODO total with tax",
            "  7 /* FIXME clear the cart */",
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0334 AK-9 and behaviour 25: the chords of the grouping actions act while the TODO
/// window has the focus and not in the editor; by id the actions flip the toggles anywhere.
#[gpui::test]
fn grouping_keys_belong_to_the_todo_window(cx: &mut TestAppContext) {
    let root = workspace("keys");
    let (bench, cx) = window(&root, cx);
    // Before the window was ever shown the action only says so.
    action(&bench, cx, "TodoViewGroupByShowModules");
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)),
        Some("The TODO tool window is not open".to_owned())
    );
    open(&bench, cx, &root.join(FILE));
    let tool = show(&bench, cx);
    let grouping = |cx: &mut VisualTestContext| {
        tool.read_with(cx, |t, _| {
            let g = t.grouping();
            (g.modules, g.packages, g.flatten)
        })
    };
    assert_eq!(grouping(cx), (false, true, false));

    // ⌃M: modules as a level.
    cx.simulate_keystrokes("ctrl-m");
    cx.run_until_parked();
    assert_eq!(grouping(cx), (true, true, false));
    let shown = rows(&tool, cx);
    assert_eq!(shown[1], "  [app] (3 items)");
    assert!(
        shown.contains(&"    src/main/java/shop/ (2)".to_owned()),
        "{shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|r| r.starts_with("  [ferris-idea-todo-win-keys")),
        "{shown:?}"
    );
    // ⌃F flattens the folders; ⌃P takes them away, and then ⌃F does nothing.
    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    assert_eq!(grouping(cx), (true, true, true));
    cx.simulate_keystrokes("ctrl-f ctrl-p");
    cx.run_until_parked();
    assert_eq!(grouping(cx), (true, false, false));
    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    assert_eq!(grouping(cx), (true, false, false));
    assert!(rows(&tool, cx).contains(&"    Cart.java <src/main/java/shop> (2)".to_owned()));

    // In the editor the chords are not the TODO window's.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-m ctrl-p");
    cx.run_until_parked();
    assert_eq!(grouping(cx), (true, false, false));
    // By id they act wherever the focus is.
    action(&bench, cx, "TodoViewGroupByShowPackages");
    action(&bench, cx, "TodoViewGroupByShowModules");
    assert_eq!(grouping(cx), (false, true, false));
    action(&bench, cx, "TodoViewGroupByFlattenPackage");
    assert_eq!(grouping(cx), (false, true, true));
    assert_eq!(
        rows(&tool, cx)[1..4],
        [
            "  app/ (1)",
            "    settings.yaml (1)",
            "      1 # TODO version"
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0334 AK-9 and behaviours 9, 12: typed text shows in both scopes before it is saved, a
/// save and a change on disk show without a key press, and a hidden window scans nothing.
#[gpui::test]
fn the_tree_follows_saves_and_the_current_file(cx: &mut TestAppContext) {
    let root = workspace("refresh");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join(FILE));
    let tool = show(&bench, cx);
    assert_eq!(total(&tool, cx), 4);

    // Typed, not saved: the current file and the project show the editor's text.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    editor.update(cx, |e, cx| e.go_to(0, 0, cx));
    cx.simulate_input("// todo typed first\n");
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    bench.update(cx, |b, cx| b.set_todo_scope(Scope::File, cx));
    wait(cx, "the typed item in the current file", |cx| {
        rows(&tool, cx).len() == 4
    });
    assert_eq!(
        rows(&tool, cx)[..2],
        [
            "Cart.java <app/src/main/java/shop> (3)",
            "  1 // todo typed first"
        ]
    );
    bench.update(cx, |b, cx| b.set_todo_scope(Scope::Project, cx));
    wait(cx, "the typed item in the project", |cx| {
        total(&tool, cx) == 5
    });
    assert!(rows(&tool, cx).contains(&"        5 // TODO total with tax".to_owned()));
    assert_eq!(std::fs::read_to_string(root.join(FILE)).unwrap(), CART);

    // Saved: the same items, now from the file.
    cx.simulate_keystrokes("cmd-s");
    wait(cx, "the save", |cx| {
        !editor.read_with(cx, |e, _| e.is_modified())
    });
    assert!(
        std::fs::read_to_string(root.join(FILE))
            .unwrap()
            .starts_with("// todo typed first\n")
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    assert_eq!(total(&tool, cx), 5);

    // Another program changes a file: it shows without a key press.
    std::fs::write(root.join("notes.txt"), "TODO buy milk\nFIXME and bread\n").unwrap();
    wait(cx, "the change on disk", |cx| total(&tool, cx) == 6);
    assert!(rows(&tool, cx).contains(&"    2 FIXME and bread".to_owned()));

    // Hidden: nothing is scanned; shown again: the tree is up to date.
    bench.update_in(cx, |b, window, cx| {
        window.focus(&tool.read(cx).focus());
        b.menu_action("ActivateTODOToolWindow", window, cx);
    });
    cx.run_until_parked();
    assert!(!bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Todo)));
    std::fs::write(root.join("notes.txt"), "nothing left\n").unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(total(&tool, cx), 6);
    action(&bench, cx, "ActivateTODOToolWindow");
    assert!(bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Todo)));
    wait(cx, "the scan after showing again", |cx| {
        total(&tool, cx) == 4
    });
    let _ = std::fs::remove_dir_all(&root);
}
