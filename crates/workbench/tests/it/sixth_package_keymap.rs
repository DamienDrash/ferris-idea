//! The sixth package of actions taken from the keymap, carried out in a window of the test
//! harness: the filters, groupings, Exclude, Include and the export of the Find tool window
//! (`SPEC-0293` v9 behaviours 22–26), the group steps of Search Everywhere (`SPEC-0067` v11
//! behaviour 22), Next and Prev Parameter (`SPEC-0080` v3 behaviour 7), Activate Next and
//! Previous Window (`SPEC-0094` v6 behaviour 17), Show Source and Copy… (`SPEC-0275` v18
//! behaviours 45 and 46), the list of a tool window's tabs (`SPEC-0167` v7 behaviour 19), Go to
//! Mnemonic (`SPEC-0331` v3 behaviour 33), New Scratch File (`SPEC-0059` v9.6 behaviour 28) and
//! the find bar's filter menu (`SPEC-0065` v6 behaviour 20).

#![cfg(target_os = "macos")]

use ferris_idea_languages::context::{PlaceContext, UsageKind};
use ferris_idea_languages::insight::{Signature, SignatureHelp};
use ferris_idea_ui::tool_windows::ToolWindow;
use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::find_usages_model::{Hit, UsageTree};
use ferris_idea_workbench::nav_model::Place;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-sixth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/inner")).unwrap();
    std::fs::write(root.join("src/inner/deep.js"), "let deep = 1;\n").unwrap();
    std::fs::write(
        root.join("src/main.js"),
        "import { count } from './inner/deep.js';\nlet a = count;\ncount = 2;\ncall(count, a, 3);\n",
    )
    .unwrap();
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

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// The usages of `count` in `src/main.js`: an import, a read, a write and a call.
fn usages(root: &std::path::Path) -> UsageTree {
    let file = root.join("src/main.js");
    let hit = |line: usize, column: usize, text: &str, kind: UsageKind| Hit {
        place: Place {
            path: file.clone(),
            line,
            column,
        },
        shown_path: "src/main.js".into(),
        text: text.into(),
        mark: column..column + 5,
        kind: Some(kind),
        context: Some(PlaceContext {
            module: "shop.web".into(),
            ..PlaceContext::default()
        }),
    };
    UsageTree::new(
        "count",
        "src/inner/deep.js:1".into(),
        false,
        vec![
            hit(
                0,
                9,
                "import { count } from './inner/deep.js';",
                UsageKind::Import,
            ),
            hit(1, 8, "let a = count;", UsageKind::Read),
            hit(2, 0, "count = 2;", UsageKind::Write),
            hit(3, 5, "call(count, a, 3);", UsageKind::Call),
        ],
    )
}

/// The labels of the Find tool window's rows, indented by two blanks a level, an excluded row
/// with `~` in front.
fn find_rows(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        let view = b.find_tool_window().expect("the Find tool window");
        let view = view.read(cx);
        view.tree()
            .expect("a result")
            .rows()
            .into_iter()
            .map(|r| {
                format!(
                    "{}{}{}",
                    if r.excluded { "~" } else { "" },
                    "  ".repeat(r.depth),
                    r.label
                )
            })
            .collect()
    })
}

/// SPEC-0293 AK-14 (v9), SPEC-0275 AK-40 (v18): the filters, the nested folders, the module
/// groups, Exclude and Include change the tree the window shows; Exclude has the Delete key
/// while the window has the focus; Show Source opens the selected usage and the window keeps
/// the focus.
#[gpui::test]
fn find_window_filters_groups_and_excludes(cx: &mut TestAppContext) {
    let root = workspace("find");
    let (bench, cx) = open(&root, cx);
    // Without a result each action says so and changes nothing.
    act(&bench, "UsageFiltering.ReadAccess", cx);
    assert_eq!(status(&bench, cx), "No Find Usages result to group");
    act(&bench, "ExportToTextFile", cx);
    assert_eq!(status(&bench, cx), "No Find Usages result to export");

    let tree = usages(&root);
    bench.update_in(cx, |b, window, cx| b.show_usage_tree(tree, window, cx));
    cx.run_until_parked();
    let all = find_rows(&bench, cx);
    let has = |rows: &[String], text: &str| rows.iter().any(|r| r.contains(text));
    assert!(
        has(&all, "let a = count;") && has(&all, "count = 2;"),
        "{all:?}"
    );

    // The filters, one after the other.
    act(&bench, "UsageFiltering.ReadAccess", cx);
    let rows = find_rows(&bench, cx);
    assert!(!has(&rows, "let a = count;"), "{rows:?}");
    assert!(
        has(&rows, "count = 2;") && has(&rows, "call(count"),
        "{rows:?}"
    );
    act(&bench, "UsageFiltering.WriteAccess", cx);
    assert!(!has(&find_rows(&bench, cx), "count = 2;"));
    act(&bench, "UsageFiltering.Imports", cx);
    let rows = find_rows(&bench, cx);
    assert!(!has(&rows, "import {"), "{rows:?}");
    assert!(
        has(&rows, "call(count"),
        "a call is never filtered: {rows:?}"
    );
    for id in [
        "UsageFiltering.ReadAccess",
        "UsageFiltering.WriteAccess",
        "UsageFiltering.Imports",
    ] {
        act(&bench, id, cx);
    }
    assert_eq!(find_rows(&bench, cx), all);

    // Directory Structure: one row for the folder `src`.
    assert!(!all.iter().any(|r| r.trim() == "src"), "{all:?}");
    act(&bench, "UsageGrouping.DirectoryStructure", cx);
    let rows = find_rows(&bench, cx);
    assert!(rows.iter().any(|r| r.trim() == "src"), "{rows:?}");
    act(&bench, "UsageGrouping.DirectoryStructure", cx);
    assert_eq!(find_rows(&bench, cx), all);

    // Flatten Modules off: the module `shop.web` below the group `shop`.
    assert!(!all.iter().any(|r| r.trim() == "shop"), "{all:?}");
    act(&bench, "UsageGrouping.FlattenModules", cx);
    let rows = find_rows(&bench, cx);
    assert!(rows.iter().any(|r| r.trim() == "shop"), "{rows:?}");
    act(&bench, "UsageGrouping.FlattenModules", cx);
    assert_eq!(find_rows(&bench, cx), all);

    // Exclude by its key while the window has the focus: the selected usage, the first one.
    let included = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            let view = b.find_tool_window().unwrap();
            view.read(cx).tree().unwrap().included().len()
        })
    };
    assert_eq!(included(cx), 4);
    bench.update_in(cx, |b, _, cx| {
        let view = b.find_tool_window().unwrap();
        let first = view
            .read(cx)
            .tree()
            .unwrap()
            .rows()
            .iter()
            .position(|r| r.label.contains("import {"))
            .expect("the row of the import");
        view.update(cx, |v, cx| v.select_row(first, cx));
    });
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(included(cx), 3, "{:?}", find_rows(&bench, cx));
    // The import and the groups that hold nothing else are marked, the other usages are not.
    let rows = find_rows(&bench, cx);
    let marked = |text: &str| {
        rows.iter()
            .find(|r| r.contains(text))
            .is_some_and(|r| r.starts_with('~'))
    };
    assert!(marked("import {") && marked("Imports"), "{rows:?}");
    assert!(!marked("count = 2;") && !marked("Usages in"), "{rows:?}");
    // The next row is selected: excluding again takes what it stands for, one usage more.
    act(&bench, "UsageView.Exclude", cx);
    assert_eq!(included(cx), 2);
    // Include on the group row brings both back; a second Include has nothing to do.
    bench.update_in(cx, |b, _, cx| {
        let view = b.find_tool_window().unwrap();
        view.update(cx, |v, cx| v.select_row(1, cx));
    });
    cx.simulate_keystrokes("shift-backspace");
    cx.run_until_parked();
    assert_eq!(included(cx), 4);
    act(&bench, "UsageView.Include", cx);
    assert_eq!(status(&bench, cx), "Nothing excluded in the selected row");

    // Show Source on a usage: its file is the active one and the window keeps the focus.
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("src/inner/deep.js"), window, cx);
        let view = b.find_tool_window().unwrap();
        let last = view.read(cx).tree().unwrap().rows().len() - 1;
        view.update(cx, |v, cx| v.select_row(last, cx));
        window.focus(&view.read(cx).focus());
    });
    cx.run_until_parked();
    act(&bench, "ViewSource", cx);
    let (active, focused) = bench.update_in(cx, |b, window, cx| {
        (
            b.active_path(),
            b.tool_window_focused(ToolWindow::Find, window, cx),
        )
    });
    assert_eq!(active, Some(root.join("src/main.js")));
    assert!(focused, "the Find tool window keeps the focus");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0293 AK-15 (v9): the usages are written to the chosen file, without what is excluded.
#[gpui::test]
fn usages_are_exported_to_a_text_file(cx: &mut TestAppContext) {
    let root = workspace("export");
    let (bench, cx) = open(&root, cx);
    let target = root.join("usages.txt");
    // Without a result nothing is written.
    let written = bench.update_in(cx, |b, _, cx| b.export_usages_to(&target, cx));
    assert!(!written && !target.exists());

    let tree = usages(&root);
    bench.update_in(cx, |b, window, cx| b.show_usage_tree(tree, window, cx));
    cx.run_until_parked();
    act(&bench, "UsageView.Exclude", cx);
    let written = bench.update_in(cx, |b, _, cx| b.export_usages_to(&target, cx));
    assert!(written);
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.starts_with("count  (src/inner/deep.js:1)\n"), "{text}");
    assert!(
        text.contains("Usages in Project Files  (3 usages)\n"),
        "{text}"
    );
    assert_eq!(text.matches("count = 2;").count(), 1, "{text}");
    // The selected usage, the import, was excluded: it is not written.
    assert!(!text.contains("import {"), "{text}");
    assert!(text.contains("call(count, a, 3);"), "{text}");
    assert_eq!(
        status(&bench, cx),
        format!("Exported 3 usages to {}", target.display())
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0067 AK-12 (v11): the group actions move the selection of the open popup to the last
/// and the first result of a list without headings, and are nothing without a popup.
#[gpui::test]
fn search_everywhere_steps_by_group(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = workspace("groups");
    let (bench, cx) = open(&root, cx);
    act(&bench, "SearchEverywhere.NavigateToNextGroup", cx);
    assert!(bench.read_with(cx, |b, _| b.everywhere().is_none()));

    act(&bench, "GotoAction", cx);
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("the popup");
    popup.update_in(cx, |v, _, cx| v.set_query("bookmark", cx));
    let rows = |cx: &mut VisualTestContext| popup.read_with(cx, |v, _| v.results().rows.len());
    for _ in 0..400 {
        cx.run_until_parked();
        if rows(cx) > 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let n = rows(cx);
    assert!(n > 2, "the actions with bookmark in their titles: {n}");
    let selected = |cx: &mut VisualTestContext| popup.read_with(cx, |v, _| v.results().selected);
    assert_eq!(selected(cx), Some(0));
    act(&bench, "SearchEverywhere.NavigateToNextGroup", cx);
    assert_eq!(selected(cx), Some(n - 1));
    act(&bench, "SearchEverywhere.NavigateToPrevGroup", cx);
    assert_eq!(selected(cx), Some(0));
    // The key of the keymap does the same while the popup has the focus.
    cx.simulate_keystrokes("pagedown");
    cx.run_until_parked();
    assert_eq!(selected(cx), Some(n - 1));
    let _ = std::fs::remove_dir_all(&root);
}

fn help() -> SignatureHelp {
    SignatureHelp {
        signatures: vec![Signature {
            label: "call(x, y, z)".into(),
            documentation: None,
            parameters: vec![5..6, 8..9, 11..12],
            active_parameter: Some(0),
        }],
        active_signature: 0,
    }
}

/// SPEC-0080 AK-5 (v3): with parameter info shown Tab and ⇧Tab move the caret from argument
/// to argument; at the ends and without parameter info the actions are none.
#[gpui::test]
fn next_and_prev_parameter_move_between_arguments(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = workspace("params");
    let (bench, cx) = open(&root, cx);
    // Line 4 is `call(count, a, 3);`; the caret goes behind `call(`.
    let line_start = "import { count } from './inner/deep.js';\nlet a = count;\ncount = 2;\n"
        .chars()
        .count();
    let caret = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.active_editor_entity().unwrap().read(cx).caret()
        }) - line_start
    };
    let put = |at: usize, cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, _, cx| {
            let editor = b.active_editor_entity().unwrap();
            let to = line_start + at;
            editor.update(cx, |e, cx| {
                e.select_ranges(std::slice::from_ref(&(to..to)), cx)
            });
        });
        cx.run_until_parked();
    };
    // The popup is set anew before each step: without a language server its refresh after a
    // caret move closes it.
    let show = |cx: &mut VisualTestContext| {
        assert!(bench.update_in(cx, |b, window, cx| b.show_param_info(help(), window, cx)));
    };
    put(5, cx);
    // Without parameter info the action is none and the caret stays.
    act(&bench, "NextParameter", cx);
    assert_eq!(caret(cx), 5);

    show(cx);
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(caret(cx), 12, "the start of `a`");
    show(cx);
    act(&bench, "NextParameter", cx);
    assert_eq!(caret(cx), 15, "the start of `3`");
    show(cx);
    act(&bench, "NextParameter", cx);
    assert_eq!(caret(cx), 15, "no further argument");
    show(cx);
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
    assert_eq!(caret(cx), 12);
    show(cx);
    act(&bench, "PrevParameter", cx);
    assert_eq!(caret(cx), 5);
    // The text was never changed: Tab did not indent.
    let text = bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .unwrap()
            .read(cx)
            .state
            .buffer
            .text()
    });
    assert!(text.ends_with("call(count, a, 3);\n"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0094 AK-15 (v6, behaviour 17): Activate Next and Previous Window step through the windows
/// of the application and wrap; with one window there is nothing to do.
#[gpui::test]
fn next_and_previous_window_step(cx: &mut TestAppContext) {
    let roots: Vec<_> = ["w1", "w2", "w3"].iter().map(|t| workspace(t)).collect();
    let mut windows = Vec::new();
    for root in &roots {
        let dir = root.clone();
        let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
        cx.run_until_parked();
        windows.push(window);
    }
    let active = |cx: &mut TestAppContext| {
        let front = cx.update(|cx| cx.active_window());
        windows.iter().position(|w| Some((*w).into()) == front)
    };
    let step = |from: usize, id: &str, cx: &mut TestAppContext| {
        windows[from]
            .update(cx, |b, window, cx| {
                window.activate_window();
                b.menu_action(id, window, cx);
            })
            .unwrap();
        cx.run_until_parked();
    };
    step(0, "NextWindow", cx);
    assert_eq!(active(cx), Some(1));
    step(1, "NextWindow", cx);
    assert_eq!(active(cx), Some(2));
    step(2, "NextWindow", cx);
    assert_eq!(active(cx), Some(0), "wraps");
    step(0, "PreviousWindow", cx);
    assert_eq!(active(cx), Some(2));
    step(2, "PreviousWindow", cx);
    assert_eq!(active(cx), Some(1));
    for root in &roots {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// SPEC-0275 AK-40 (v18): Show Source opens the file selected in the Project tree and the
/// tree keeps the focus; on a folder the status bar says so; in the editor it is no action.
#[gpui::test]
fn show_source_keeps_the_focus_in_the_tree(cx: &mut TestAppContext) {
    let root = workspace("source");
    let (bench, cx) = open(&root, cx);
    let deep = root.join("src/inner/deep.js");
    // With the editor in focus nothing opens.
    act(&bench, "ViewSource", cx);
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(root.join("src/main.js"))
    );

    bench.update_in(cx, |b, window, cx| {
        b.focus_tree(window, cx);
        b.project_tree_call(&serde_json::json!({"action": "select", "path": deep}), cx)
            .expect("the file is in the tree");
    });
    cx.run_until_parked();
    act(&bench, "ViewSource", cx);
    let (active, in_tree) = bench.update_in(cx, |b, window, cx| {
        (
            b.active_path(),
            b.tool_window_focused(ToolWindow::Project, window, cx),
        )
    });
    assert_eq!(active, Some(deep.clone()));
    assert!(in_tree, "the Project tree keeps the focus");

    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(
            &serde_json::json!({"action": "select", "path": root.join("src")}),
            cx,
        )
        .expect("src is in the tree");
    });
    act(&bench, "ViewSource", cx);
    assert_eq!(
        status(&bench, cx),
        "Select a file or a usage to show its source"
    );
    assert_eq!(bench.read_with(cx, |b, _| b.active_path()), Some(deep));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0275 AK-41 (v18): Copy… opens the name field with a free name; Enter makes the copy
/// beside the original; the project folder is refused.
#[gpui::test]
fn copy_makes_a_copy_beside_the_original(cx: &mut TestAppContext) {
    let root = workspace("copy");
    let (bench, cx) = open(&root, cx);
    let deep = root.join("src/inner/deep.js");
    bench.update_in(cx, |b, window, cx| {
        b.focus_tree(window, cx);
        b.project_tree_call(&serde_json::json!({"action": "select", "path": root}), cx)
            .expect("the project folder is in the tree");
    });
    cx.run_until_parked();
    act(&bench, "CopyElement", cx);
    assert!(bench.read_with(cx, |b, _| b.name_prompt_state().is_none()));
    assert_eq!(status(&bench, cx), "Select one file or folder to copy it");

    bench.update_in(cx, |b, _, cx| {
        b.project_tree_call(&serde_json::json!({"action": "select", "path": deep}), cx)
            .expect("the file is in the tree");
    });
    act(&bench, "CopyElement", cx);
    let (title, error) = bench
        .read_with(cx, |b, _| b.name_prompt_state())
        .expect("the name field");
    assert!(title.contains("deep.js"), "{title}");
    assert_eq!(error, None);
    bench.update_in(cx, |b, window, cx| b.finish_name_prompt(true, window, cx));
    cx.run_until_parked();
    let names: Vec<String> = std::fs::read_dir(root.join("src/inner"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let copy = names.iter().find(|n| *n != "deep.js").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("src/inner").join(copy)).unwrap(),
        "let deep = 1;\n"
    );
    assert_eq!(
        std::fs::read_to_string(&deep).unwrap(),
        "let deep = 1;\n",
        "the original stays"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0331 AK-11 (v3): Go to Mnemonic lists the bookmarks with a mnemonic and opens the
/// chosen one; without one the status bar says so.
#[gpui::test]
fn go_to_mnemonic_lists_and_opens(cx: &mut TestAppContext) {
    let root = workspace("mnemonic");
    let (bench, cx) = open(&root, cx);
    act(&bench, "ShowTypeBookmarks", cx);
    assert!(bench.read_with(cx, |b, _| b.compare_popup().is_none()));
    assert_eq!(status(&bench, cx), "No bookmark has a mnemonic");

    // Mnemonic 3 on line 3 of main.js, a bookmark without one on line 1 of deep.js.
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().unwrap();
        editor.update(cx, |e, cx| e.go_to(2, 0, cx));
    });
    act(&bench, "ToggleBookmark3", cx);
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("src/inner/deep.js"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    act(&bench, "ToggleBookmark", cx);

    act(&bench, "ShowTypeBookmarks", cx);
    let popup = bench
        .read_with(cx, |b, _| b.compare_popup())
        .expect("the list");
    let (title, items) = popup.read_with(cx, |v, _| (v.title().to_owned(), v.items().to_vec()));
    assert_eq!(title, "Go to Mnemonic");
    assert_eq!(items, ["3   main.js:3   count = 2;"]);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (active, line) = bench.read_with(cx, |b, cx| {
        let editor = b.active_editor_entity().unwrap();
        let e = editor.read(cx);
        let before: String = e.state.buffer.text().chars().take(e.caret()).collect();
        (b.active_path(), before.matches('\n').count())
    });
    assert_eq!(active, Some(root.join("src/main.js")));
    assert_eq!(line, 2);
    assert!(bench.read_with(cx, |b, _| b.compare_popup().is_none()));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0059 AK-16 (v9.6): New Scratch File lists the kinds with the active file's selected; a
/// scratch file is created empty, opened, and the next one gets the next name.
#[gpui::test]
fn a_scratch_file_is_created_and_opened(cx: &mut TestAppContext) {
    let root = workspace("scratch");
    let (bench, cx) = open(&root, cx);
    act(&bench, "NewScratchFile", cx);
    let popup = bench
        .read_with(cx, |b, _| b.compare_popup())
        .expect("the list of kinds");
    let (title, items, selected) = popup.read_with(cx, |v, _| {
        (v.title().to_owned(), v.items().to_vec(), v.selected())
    });
    assert_eq!(title, "New Scratch File");
    assert_eq!(items[0], "Plain text");
    assert_eq!(items[selected], "JavaScript", "the kind of main.js");
    // Escape closes the list and creates nothing (the user's own folder is not written here).
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.compare_popup().is_none()));

    let dir = root.join("scratches-of-the-test");
    let first = bench
        .update_in(cx, |b, window, cx| {
            b.create_scratch_in(&dir, "py", window, cx)
        })
        .expect("a scratch file");
    cx.run_until_parked();
    assert_eq!(first, dir.join("scratch.py"));
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "");
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(first.clone())
    );
    assert_eq!(
        status(&bench, cx),
        format!("Created scratch file {}", first.display())
    );
    let second = bench
        .update_in(cx, |b, window, cx| {
            b.create_scratch_in(&dir, "py", window, cx)
        })
        .expect("a second scratch file");
    assert_eq!(second, dir.join("scratch_1.py"));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0167 AK-9 (v7): Show List of Tabs lists the tabs of the focused Terminal and selects
/// the chosen one; with one tab and outside a tool window it is no action.
#[gpui::test]
fn show_content_lists_the_tabs_of_the_terminal(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = workspace("tabs");
    let (bench, cx) = open(&root, cx);
    // The editor has the focus: no list.
    act(&bench, "ShowContent", cx);
    assert!(bench.read_with(cx, |b, _| b.compare_popup().is_none()));

    bench.update_in(cx, |b, window, cx| b.new_terminal_tab(window, cx));
    cx.run_until_parked();
    assert!(bench.update_in(cx, |b, window, cx| {
        b.tool_window_focused(ToolWindow::Terminal, window, cx)
    }));
    let tabs = bench.read_with(cx, |b, _| b.terminal_tabs());
    if tabs.len() == 1 {
        // One tab: nothing to list.
        act(&bench, "ShowContent", cx);
        assert!(bench.read_with(cx, |b, _| b.compare_popup().is_none()));
        bench.update_in(cx, |b, window, cx| b.new_terminal_tab(window, cx));
        cx.run_until_parked();
    }
    let tabs = bench.read_with(cx, |b, _| b.terminal_tabs());
    assert!(tabs.len() >= 2, "{tabs:?}");
    let current = bench.read_with(cx, |b, _| b.terminal_tab());
    act(&bench, "ShowContent", cx);
    let popup = bench
        .read_with(cx, |b, _| b.compare_popup())
        .expect("the list of tabs");
    let (title, items, selected) = popup.read_with(cx, |v, _| {
        (v.title().to_owned(), v.items().to_vec(), v.selected())
    });
    assert_eq!(title, "Tabs of Terminal");
    assert_eq!(items, tabs);
    assert_eq!(selected, current);
    // The first tab is another one than the selected (the newest).
    assert_ne!(current, 0);
    popup.update_in(cx, |v, _, cx| {
        v.select(0);
        cx.notify();
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(bench.read_with(cx, |b, _| b.terminal_tab()), 0);
    assert!(bench.read_with(cx, |b, _| b.compare_popup().is_none()));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0065 AK-8 (v6): the filter menu of the find bar opens by its action; without a find
/// bar the action is none.
#[gpui::test]
fn the_find_bar_filter_menu_opens_by_action(cx: &mut TestAppContext) {
    let root = workspace("filter");
    let (bench, cx) = open(&root, cx);
    act(&bench, "ShowFilterPopup", cx);
    assert!(!bench.read_with(cx, |b, _| b.find_filter_shown()));
    act(&bench, "Find", cx);
    act(&bench, "ShowFilterPopup", cx);
    assert!(bench.read_with(cx, |b, _| b.find_filter_shown()));
    let _ = std::fs::remove_dir_all(&root);
}
