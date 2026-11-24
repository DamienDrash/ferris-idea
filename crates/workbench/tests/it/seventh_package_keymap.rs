//! The seventh package of actions taken from the keymap, carried out in a window of the test
//! harness: the first and last row of a list and the selection extended to them (`SPEC-0275`
//! v19 behaviours 47 and 48), the chooser of the file's custom regions (`SPEC-0083` v9
//! behaviour 24), the type of the expression at the caret (`SPEC-0077` v7 behaviour 6), the end
//! of input for the program of the Run tool window (`SPEC-0087` v10 behaviour 18) and the list
//! of the background tasks that can be stopped (`SPEC-0061` v16 behaviour 62).

#![cfg(target_os = "macos")]

use std::sync::atomic::Ordering;

use ferris_idea_languages::context::UsageKind;
use ferris_idea_ui::tool_windows::ToolWindow;
use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::doc_model::DocBlock;
use ferris_idea_workbench::find_usages_model::{Hit, UsageTree};
use ferris_idea_workbench::keymap_actions::BackgroundTask;
use ferris_idea_workbench::nav_model::Place;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

/// A Java file with three custom regions: `Fields` 1–4, `Inner` 2–3 inside it and one without
/// a name 6–8; a marker without a partner on line 9.
const REGIONS: &str = "class Shop {
    // region Fields
    // region Inner
    int count;
    // endregion
    // endregion
    int price;
    // region
    int rest;
    // endregion
    // region Never closed
}
";

fn workspace(tag: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-seventh-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    let total = vec![1u8];\n    total.len();\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/Shop.java"), REGIONS).unwrap();
    std::fs::write(root.join("src/plain.rs"), "fn a() {\n    let x = 1;\n}\n").unwrap();
    std::fs::write(root.join("notes.txt"), "one\ntwo\n").unwrap();
    // A program that reads its input to the end between two lines of output.
    std::fs::write(
        root.join(".ferris-idea/run.toml"),
        "[[run]]\nname = \"reader\"\ncommand = [\"/bin/sh\", \"-c\", \"echo before; cat; echo after\"]\n",
    )
    .unwrap();
    // The directory as the workbench names it, so paths compare equal.
    root.canonicalize().unwrap()
}

fn open(
    root: &std::path::Path,
    file: &str,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join(file);
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

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
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

/// Title, rows and selected row of the open chooser.
fn chooser(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> Option<(String, Vec<String>, usize)> {
    let popup = bench.read_with(cx, |b, _| b.compare_popup())?;
    Some(popup.read_with(cx, |v, _| {
        (v.title().to_owned(), v.items().to_vec(), v.selected())
    }))
}

/// 0-based line and column of the caret of the active editor.
fn caret(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> (usize, usize) {
    bench.read_with(cx, |b, cx| {
        let s = b.active_editor_entity().unwrap().read(cx).status();
        (s.line - 1, s.column - 1)
    })
}

/// SPEC-0275 AK-42 (v19): in a popup list Home and End, and the chords of the keymap (⌥↑ and
/// ⌥↓ in the bundled macOS keymap), select the first and the last row; the variants that extend
/// a selection select the same rows where one row is selected; a tool window's list takes the
/// same keys.
#[gpui::test]
fn lists_go_to_their_first_and_last_row(cx: &mut TestAppContext) {
    let root = workspace("lists");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    act(&bench, "NewScratchFile", cx);
    let (_, items, first) = chooser(&bench, cx).expect("the list of kinds");
    let last = items.len() - 1;
    assert!(first != 0 && first != last, "Rust stands in the middle");
    let selected = |cx: &mut VisualTestContext| chooser(&bench, cx).expect("still open").2;

    press("end", cx);
    assert_eq!(selected(cx), last);
    press("home", cx);
    assert_eq!(selected(cx), 0);
    press("alt-down", cx);
    assert_eq!(selected(cx), last, "List-selectLastRow");
    press("alt-up", cx);
    assert_eq!(selected(cx), 0, "List-selectFirstRow");
    // One row is selected: there is nothing to extend, the keys of the extending actions
    // move the selection to the same rows.
    press("down", cx);
    assert_eq!(selected(cx), 1);
    press("shift-end", cx);
    assert_eq!(selected(cx), last);
    press("alt-shift-up", cx);
    assert_eq!(selected(cx), 0);
    press("alt-shift-down", cx);
    assert_eq!(selected(cx), last);
    press("shift-home", cx);
    assert_eq!(selected(cx), 0);
    // The arrow keys are as before.
    press("down", cx);
    assert_eq!(selected(cx), 1);
    press("up", cx);
    assert_eq!(selected(cx), 0);
    press("escape", cx);
    assert!(chooser(&bench, cx).is_none());

    // The Find tool window: its rows are the searched symbol, the groups and three usages.
    let file = root.join("src/main.rs");
    let hit = |line: usize, text: &str| Hit {
        place: Place {
            path: file.clone(),
            line,
            column: 4,
        },
        shown_path: "src/main.rs".into(),
        text: text.into(),
        mark: 4..9,
        kind: Some(UsageKind::Read),
        context: None,
    };
    let tree = UsageTree::new(
        "total",
        "src/main.rs:2".into(),
        false,
        vec![
            hit(1, "let total = vec![1u8];"),
            hit(2, "total.len();"),
            hit(3, "}"),
        ],
    );
    bench.update_in(cx, |b, window, cx| b.show_usage_tree(tree, window, cx));
    cx.run_until_parked();
    let view = bench
        .read_with(cx, |b, _| b.find_tool_window())
        .expect("the Find tool window");
    bench.update_in(cx, |_, window, cx| {
        let focus = view.read(cx).focus();
        window.focus(&focus);
    });
    cx.run_until_parked();
    let place =
        |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.selected_place().map(|p| p.line));
    press("end", cx);
    assert_eq!(place(cx), Some(3), "the last usage");
    press("alt-up", cx);
    press("alt-down", cx);
    assert_eq!(place(cx), Some(3));
    press("home", cx);
    press("end", cx);
    assert_eq!(place(cx), Some(3));
    press("up", cx);
    assert_eq!(place(cx), Some(2), "one row up from the last");
    let _ = std::fs::remove_dir_all(&root);
}

/// The rows of the Project tree that are selected, by name.
fn selected_rows(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, _| {
        b.project_tree_state()["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter(|r| r["selected"] == true)
            .map(|r| r["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    })
}

fn all_rows(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, _| {
        b.project_tree_state()["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| r["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    })
}

/// SPEC-0275 AK-43 (v19): in the Project tree, where several rows can be selected, ⇧End and
/// ⇧Home and the keymap's ⌥⇧↓ and ⌥⇧↑ extend the selection from its anchor to the last and the
/// first row; ⌥↑ and ⌥↓ select the first and the last row alone.
#[gpui::test]
fn the_project_tree_extends_its_selection_to_the_ends(cx: &mut TestAppContext) {
    let root = workspace("tree");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    bench.update_in(cx, |b, window, cx| {
        b.focus_tree(window, cx);
        b.project_tree_call(
            &serde_json::json!({"action": "select", "path": root.join("src/Shop.java")}),
            cx,
        )
        .expect("the file is in the tree");
    });
    cx.run_until_parked();
    let rows = all_rows(&bench, cx);
    let at = rows.iter().position(|r| r == "Shop.java").expect("the row");
    assert!(at > 0 && at + 1 < rows.len(), "{rows:?}");
    assert_eq!(selected_rows(&bench, cx), ["Shop.java"]);

    press("shift-end", cx);
    assert_eq!(selected_rows(&bench, cx), rows[at..], "to the last row");
    // The anchor stays: the other end replaces the first range.
    press("shift-home", cx);
    assert_eq!(selected_rows(&bench, cx), rows[..=at], "to the first row");
    press("alt-shift-down", cx);
    assert_eq!(selected_rows(&bench, cx), rows[at..]);
    press("alt-shift-up", cx);
    assert_eq!(selected_rows(&bench, cx), rows[..=at]);
    // Without Shift one row is selected again.
    press("alt-down", cx);
    assert_eq!(selected_rows(&bench, cx), [rows[rows.len() - 1].clone()]);
    press("alt-up", cx);
    assert_eq!(selected_rows(&bench, cx), [rows[0].clone()]);
    press("end", cx);
    assert_eq!(selected_rows(&bench, cx), [rows[rows.len() - 1].clone()]);
    let in_tree = bench.update_in(cx, |b, window, cx| {
        b.tool_window_focused(ToolWindow::Project, window, cx)
    });
    assert!(in_tree, "the tree keeps the focus");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0083 AK-30 (v9): Custom Folding… lists the custom regions of the file with the one
/// around the caret selected; Enter puts the caret on the first line of the chosen region and
/// Back returns; a file without regions says so and opens no list.
#[gpui::test]
fn custom_regions_are_chosen_from_a_list(cx: &mut TestAppContext) {
    let root = workspace("regions");
    let (bench, cx) = open(&root, "src/Shop.java", cx);
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    until(cx, "the syntax tree gives the regions", |cx| {
        editor.read_with(cx, |e, _| e.custom_regions().len() == 3)
    });
    editor.update(cx, |e, cx| e.go_to(8, 6, cx));
    cx.run_until_parked();

    act(&bench, "GotoCustomRegion", cx);
    let (title, items, selected) = chooser(&bench, cx).expect("the list of regions");
    assert_eq!(title, "Custom Folding");
    assert_eq!(
        items,
        [
            "Fields  (line 2)",
            "Inner  (line 3)",
            "Unnamed region  (line 8)"
        ]
    );
    assert_eq!(selected, 2, "the region around the caret");
    // The first and last row keys of the list (SPEC-0275 v19 behaviour 47), then one down.
    press("home", cx);
    press("down", cx);
    press("enter", cx);
    assert!(chooser(&bench, cx).is_none());
    assert_eq!(caret(&bench, cx), (2, 4), "in front of the marker of Inner");
    let in_editor = bench.update_in(cx, |b, window, cx| {
        b.active_editor_entity()
            .is_some_and(|e| e.read(cx).focus().is_focused(window))
    });
    assert!(in_editor, "the editor has the focus again");
    act(&bench, "Back", cx);
    assert_eq!(caret(&bench, cx), (8, 6), "Back returns to the place left");

    // Escape closes the list and moves nothing.
    act(&bench, "GotoCustomRegion", cx);
    press("escape", cx);
    assert!(chooser(&bench, cx).is_none());
    assert_eq!(caret(&bench, cx), (8, 6));

    // A file without regions.
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("src/plain.rs"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    act(&bench, "GotoCustomRegion", cx);
    assert!(chooser(&bench, cx).is_none());
    assert_eq!(status(&bench, cx), "No custom folding regions in this file");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0077 AK-12 (v7): Type Info shows the type the language server's hover names for the
/// word at the caret, or for a selection, below the caret; without an answer the status bar
/// says so; in a file of no language the action is none.
#[gpui::test]
fn type_info_shows_the_type_at_the_caret(cx: &mut TestAppContext) {
    let root = workspace("type");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    // The caret behind `total` on line 2.
    editor.update(cx, |e, cx| e.go_to(1, 13, cx));
    cx.run_until_parked();
    let shown = |hover: Option<&str>, cx: &mut VisualTestContext| {
        let ok = bench.update_in(cx, |b, _, cx| b.show_type_info_of(hover, cx));
        cx.run_until_parked();
        (ok, bench.read_with(cx, |b, _| b.doc_blocks()))
    };
    assert_eq!(
        shown(Some("```rust\nlet total: Vec<u8>\n```"), cx),
        (true, Some(vec![DocBlock::Code("Vec<u8>".into())]))
    );
    // Escape closes it like the documentation popup.
    press("escape", cx);
    assert_eq!(bench.read_with(cx, |b, _| b.doc_blocks()), None);

    // A selection that is no single word: the type the server answers with alone.
    editor.update(cx, |e, cx| {
        let start = e.state.buffer.text().find("total.len()").unwrap();
        e.select_range(start..start + "total.len()".len(), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        shown(Some("```rust\nusize\n```"), cx),
        (true, Some(vec![DocBlock::Code("usize".into())]))
    );
    press("escape", cx);

    // No answer, and an answer without a type.
    assert_eq!(shown(None, cx), (false, None));
    assert_eq!(status(&bench, cx), "No type information for this place");
    assert!(!shown(Some("```rust\n```"), cx).0);

    // A text file has no language server: the action is not available.
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("notes.txt"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    act(&bench, "ExpressionTypeInfo", cx);
    let note = status(&bench, cx);
    assert!(
        note.starts_with("Type Info") && note.ends_with("is not available here"),
        "{note}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0087 AK-12 (v10): with the Run tool window in focus the key of Send EOF (⌘D in the
/// bundled macOS keymap) ends the input of the tab's program, which goes on; in the editor the
/// same key duplicates the line; without a running program the status bar says so.
#[gpui::test]
fn send_eof_lets_the_program_of_the_run_tab_go_on(cx: &mut TestAppContext) {
    let root = workspace("eof");
    let (bench, cx) = open(&root, "notes.txt", cx);
    // Without a Run tool window the action is none.
    act(&bench, "SendEOF", cx);
    let note = status(&bench, cx);
    assert!(
        note.starts_with("Send EOF") && note.ends_with("is not available here"),
        "{note}"
    );

    bench.update_in(cx, |b, window, cx| {
        b.select_run_configuration("reader", cx);
        b.run_selected(window, cx);
    });
    let run = bench
        .read_with(cx, |b, _| b.run_view())
        .expect("the Run tool window");
    // The lines the program wrote; the console also shows its command line, which holds
    // both words.
    let wrote = |word: &str, cx: &mut VisualTestContext| {
        run.read_with(cx, |r, _| {
            r.output_of("reader")
                .unwrap_or_default()
                .lines()
                .any(|l| l.trim() == word)
        })
    };
    until(cx, "the program wrote its first line", |cx| {
        wrote("before", cx)
    });
    assert!(!wrote("after", cx), "it waits for its input");

    // In the editor ⌘D is Duplicate Line and the program keeps waiting.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    press("cmd-d", cx);
    let text = bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .unwrap()
            .read(cx)
            .state
            .buffer
            .text()
    });
    assert_eq!(text, "one\none\ntwo\n");
    assert!(run.read_with(cx, |r, _| r.running()));

    // In the Run tool window it is the end of input.
    bench.update_in(cx, |_, window, cx| {
        let focus = run.read(cx).focus();
        window.focus(&focus);
    });
    cx.run_until_parked();
    press("cmd-d", cx);
    assert_eq!(status(&bench, cx), "End of input sent to the program");
    until(cx, "the program went on and ended", |cx| {
        !run.read_with(cx, |r, _| r.running())
    });
    assert!(wrote("before", cx) && wrote("after", cx));
    assert_eq!(
        run.read_with(cx, |r, _| r.tab_states()[0].2),
        Some(0),
        "ended by itself, not stopped"
    );
    // Nothing runs any more.
    press("cmd-d", cx);
    assert_eq!(
        status(&bench, cx),
        "No program is running in the selected Run tab"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0061 AK-v16: Stop Background Processes… lists the running tasks that can be stopped;
/// Enter on a row stops that task and no other; without one the status bar says so.
#[gpui::test]
fn background_tasks_are_listed_and_stopped_by_row(cx: &mut TestAppContext) {
    let root = workspace("tasks");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    act(&bench, "StopBackgroundProcesses", cx);
    assert!(chooser(&bench, cx).is_none());
    assert_eq!(
        status(&bench, cx),
        "No background task is running that can be stopped"
    );

    let hold = |task: BackgroundTask, cx: &mut VisualTestContext| {
        bench
            .update_in(cx, |b, _, cx| b.hold_background_task(task, cx))
            .expect("a task with a stop flag")
    };
    let cloning = hold(BackgroundTask::CloneRepository, cx);
    let finding = hold(BackgroundTask::FindUsages, cx);
    let scanning = hold(BackgroundTask::TodoScan, cx);
    let opening = hold(BackgroundTask::RemoteOpen, cx);

    act(&bench, "StopBackgroundProcesses", cx);
    let (title, items, selected) = chooser(&bench, cx).expect("the list of tasks");
    assert_eq!(title, "Stop Background Processes");
    assert_eq!(
        items,
        [
            "Cloning a repository: Receiving objects: 12 %",
            "Opening a remote project",
            "Finding usages",
            "Scanning the project for TODO items"
        ]
    );
    assert_eq!(selected, 0);
    // The third row.
    press("down", cx);
    press("down", cx);
    press("enter", cx);
    assert!(chooser(&bench, cx).is_none());
    assert!(
        finding.load(Ordering::SeqCst),
        "Find Usages was told to stop"
    );
    for (flag, name) in [(&cloning, "clone"), (&scanning, "scan"), (&opening, "open")] {
        assert!(!flag.load(Ordering::SeqCst), "{name} goes on");
    }
    assert_eq!(status(&bench, cx), "Stopped: Finding usages");
    let left: Vec<BackgroundTask> = bench.read_with(cx, |b, cx| {
        b.background_tasks(cx).into_iter().map(|t| t.0).collect()
    });
    assert_eq!(
        left,
        [
            BackgroundTask::CloneRepository,
            BackgroundTask::RemoteOpen,
            BackgroundTask::TodoScan
        ]
    );

    // The last row with the key of the list, then the two that are left.
    act(&bench, "StopBackgroundProcesses", cx);
    press("end", cx);
    press("enter", cx);
    assert!(scanning.load(Ordering::Relaxed));
    assert_eq!(
        status(&bench, cx),
        "Stopped: Scanning the project for TODO items"
    );
    for task in [BackgroundTask::CloneRepository, BackgroundTask::RemoteOpen] {
        assert!(bench.update_in(cx, |b, _, cx| b.stop_background_task(task, cx)));
    }
    assert!(cloning.load(Ordering::SeqCst) && opening.load(Ordering::SeqCst));
    // A task that does not run is not stopped.
    assert!(!bench.update_in(cx, |b, _, cx| {
        b.stop_background_task(BackgroundTask::FindUsages, cx)
    }));
    assert!(!bench.update_in(cx, |b, _, cx| {
        b.stop_background_task(BackgroundTask::AssistantAnswer, cx)
    }));
    let _ = std::fs::remove_dir_all(&root);
}
