//! The eighth package of actions taken from the keymap, carried out in a window of the test
//! harness: the editor tabs (`SPEC-0059` v9.7 behaviour 29), the overloads of parameter info
//! (`SPEC-0080` v4 behaviour 8), class name completion and inserting with a dot (`SPEC-0072` v13
//! behaviours 48 and 49), Inspect Current File and Run Inspection by Name (`SPEC-0098` v10
//! behaviours 20 and 21), the files of a commit (`SPEC-0086` v12 behaviour 21), the gutter icon
//! tooltip (`SPEC-0294` v5 behaviour 12), Recompile (`SPEC-0100` v5 behaviour 22), Introduce
//! Parameter (`SPEC-0095` v11 behaviour 14), Complete Current Statement and Unwrap/Remove
//! (`SPEC-0056` v5 behaviours 20 and 21) and Quick Evaluate Expression (`SPEC-0091` v19
//! behaviour 44).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_languages::insight::{CompletionItem, Signature, SignatureHelp};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::problems_model::InspectScope;

const JAVA: &str = "class Shop {
    void m(java.util.List<String> list) {
        for (String s : list) {
            if (s.isEmpty()) {
                call(1
            }
        }
    }
}
";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-eighth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {\n    let x = 1;\n}\n").unwrap();
    std::fs::write(root.join("src/Shop.java"), JAVA).unwrap();
    std::fs::write(root.join("src/b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(root.join("notes.txt"), "one\ntwo\n").unwrap();
    // The directory as the workbench names it, so paths compare equal.
    root.canonicalize().unwrap()
}

fn open(
    root: &Path,
    file: &str,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join(file);
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    open_file(&bench, &file, cx);
    (bench, cx)
}

fn open_file(bench: &Entity<WorkbenchView>, file: &Path, cx: &mut VisualTestContext) {
    let file = file.to_path_buf();
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
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

fn text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .unwrap()
            .read(cx)
            .state
            .buffer
            .text()
    })
}

/// Puts the caret of the active editor at 0-based `line` and `column`.
fn caret_at(bench: &Entity<WorkbenchView>, line: usize, column: usize, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().unwrap();
        editor.update(cx, |e, cx| e.go_to(line, column, cx));
    });
    cx.run_until_parked();
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

/// Title and rows of the open chooser.
fn chooser(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> Option<(String, Vec<String>)> {
    let popup = bench.read_with(cx, |b, _| b.compare_popup())?;
    Some(popup.read_with(cx, |v, _| (v.title().to_owned(), v.shown_items())))
}

fn active_name(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| {
        b.active_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()
    })
}

/// SPEC-0059 AK-17 (v9.7): the editor tabs step right and left and wrap at both ends; the key
/// reaches the action from the editor.
#[gpui::test]
fn editor_tabs_step_right_and_left(cx: &mut TestAppContext) {
    let root = workspace("tabs");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    open_file(&bench, &root.join("src/b.rs"), cx);
    open_file(&bench, &root.join("notes.txt"), cx);
    assert_eq!(active_name(&bench, cx), "notes.txt");
    act(&bench, "NextEditorTab", cx);
    assert_eq!(active_name(&bench, cx), "main.rs", "wraps to the first tab");
    act(&bench, "NextEditorTab", cx);
    assert_eq!(active_name(&bench, cx), "b.rs");
    act(&bench, "PreviousEditorTab", cx);
    assert_eq!(active_name(&bench, cx), "main.rs");
    act(&bench, "PreviousEditorTab", cx);
    assert_eq!(
        active_name(&bench, cx),
        "notes.txt",
        "wraps to the last tab"
    );
    press("ctrl-shift-left", cx);
    assert_eq!(active_name(&bench, cx), "b.rs", "⌃⇧← in the editor");
    press("ctrl-shift-right", cx);
    assert_eq!(active_name(&bench, cx), "notes.txt", "⌃⇧→ in the editor");
    let _ = std::fs::remove_dir_all(&root);
}

fn signatures(n: usize) -> SignatureHelp {
    SignatureHelp {
        signatures: (0..n)
            .map(|i| Signature {
                label: format!("call(int a{i})"),
                documentation: None,
                parameters: std::iter::once(5..11).collect(),
                active_parameter: Some(0),
            })
            .collect(),
        active_signature: 0,
    }
}

/// SPEC-0080 AK-6 (v4): with three signatures ⌥↓ and ⌥↑ move the highlight and wrap; a refresh
/// with the same count keeps the choice, another count drops it; with one signature the key
/// is Extend Selection again.
#[gpui::test]
fn overloads_are_chosen_in_parameter_info(cx: &mut TestAppContext) {
    let root = workspace("overloads");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    caret_at(&bench, 1, 9, cx);
    let active = |cx: &mut VisualTestContext| bench.read_with(cx, |b, _| b.param_info_active());
    assert!(bench.update_in(cx, |b, window, cx| b.show_param_info(
        signatures(3),
        window,
        cx
    )));
    press("alt-down", cx);
    assert_eq!(active(cx), Some(1));
    act(&bench, "MethodOverloadSwitchDown", cx);
    assert_eq!(active(cx), Some(2));
    press("alt-down", cx);
    assert_eq!(active(cx), Some(0), "wraps to the first");
    press("alt-up", cx);
    assert_eq!(active(cx), Some(2), "wraps to the last");
    // The server answers again with three signatures and its own pick: the choice stays.
    bench.update_in(cx, |b, _, cx| {
        assert!(b.refresh_param_info(signatures(3), cx))
    });
    assert_eq!(active(cx), Some(2));
    bench.update_in(cx, |b, _, cx| {
        assert!(b.refresh_param_info(signatures(2), cx))
    });
    assert_eq!(active(cx), Some(0), "another count: the server's pick");
    // One signature: ⌥↑ is Extend Selection, which selects the word at the caret.
    assert!(bench.update_in(cx, |b, window, cx| b.show_param_info(
        signatures(1),
        window,
        cx
    )));
    press("alt-up", cx);
    let selected = bench.read_with(cx, |b, cx| {
        let e = b.active_editor_entity().unwrap();
        !e.read(cx).state.carets.primary().range().is_empty()
    });
    assert!(selected, "Extend Selection ran");
    let _ = std::fs::remove_dir_all(&root);
}

fn item(label: &str, insert: &str, kind: &str) -> CompletionItem {
    CompletionItem {
        label: label.into(),
        insert: insert.into(),
        kind: Some(kind.into()),
        ..Default::default()
    }
}

/// SPEC-0072 AK-31 (v13): class name completion shows only the types; without one the status
/// bar says so.
#[gpui::test]
fn class_name_completion_keeps_types(cx: &mut TestAppContext) {
    let root = workspace("classes");
    let (bench, cx) = open(&root, "src/Shop.java", cx);
    caret_at(&bench, 4, 16, cx);
    let items = vec![
        item("ArrayList", "ArrayList", "class"),
        item("add", "add", "method"),
        item("List", "List", "interface"),
        item("count", "count", "variable"),
    ];
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(items, true, cx))
    });
    let shown = bench.read_with(cx, |b, _| b.completion_items());
    let shown = shown.expect("the list is open");
    assert!(shown.iter().any(|l| l.contains("ArrayList")), "{shown:?}");
    assert!(shown.iter().any(|l| l.contains("List")), "{shown:?}");
    assert!(
        !shown
            .iter()
            .any(|l| l.contains("add") || l.contains("count")),
        "{shown:?}"
    );
    press("escape", cx);
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![item("add", "add", "method")], true, cx))
    });
    assert_eq!(bench.read_with(cx, |b, _| b.completion_items()), None);
    assert_eq!(status(&bench, cx), "No class names to suggest");
    // In a file without a language the action only says so.
    open_file(&bench, &root.join("notes.txt"), cx);
    act(&bench, "ClassNameCompletion", cx);
    assert_eq!(status(&bench, cx), "No class names to suggest");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0072 AK-32 (v13): ⌃. inserts the suggestion with a dot, after the parentheses of an
/// empty call; without a list the action is not available.
#[gpui::test]
fn a_suggestion_is_inserted_with_a_dot(cx: &mut TestAppContext) {
    let root = workspace("dot");
    std::fs::write(
        root.join("src/Dot.java"),
        "class Dot {\n    int m() { return li }\n}\n",
    )
    .unwrap();
    let (bench, cx) = open(&root, "src/Dot.java", cx);
    caret_at(&bench, 1, 23, cx);
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![item("list", "list", "variable")], false, cx))
    });
    press("ctrl-.", cx);
    assert_eq!(
        text(&bench, cx),
        "class Dot {\n    int m() { return list. }\n}\n"
    );
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![item("size", "size()", "method")], false, cx))
    });
    act(&bench, "EditorChooseLookupItemDot", cx);
    assert_eq!(
        text(&bench, cx),
        "class Dot {\n    int m() { return list.size(). }\n}\n"
    );
    act(&bench, "EditorChooseLookupItemDot", cx);
    assert!(
        status(&bench, cx).contains("not available"),
        "{}",
        status(&bench, cx)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0098 AK-13 (v10): ⌥⇧I inspects the file without a dialog; Run Inspection by Name lists
/// the catalogue under headings, narrows it by typing and opens the scope dialog for the chosen
/// one; without a file the status bar says so.
#[gpui::test]
fn inspections_run_on_the_file_and_by_name(cx: &mut TestAppContext) {
    let root = workspace("inspect");
    let (bench, cx) = open(&root, "src/Shop.java", cx);
    act(&bench, "CodeInspection.OnEditor", cx);
    until(cx, "the inspection ends", |cx| {
        bench.read_with(cx, |b, _| !b.inspecting())
    });
    let scope = bench.read_with(cx, |b, _| b.inspect_scope());
    assert_eq!(scope, InspectScope::File(root.join("src/Shop.java")));
    assert_eq!(bench.read_with(cx, |b, _| b.inspect_dialog()), None);

    act(&bench, "RunInspection", cx);
    let (title, rows) = chooser(&bench, cx).expect("the list of inspections");
    assert_eq!(title, "Run Inspection by Name");
    assert!(rows.iter().any(|r| r == "Java"), "a language heading");
    let popup = bench.read_with(cx, |b, _| b.compare_popup()).unwrap();
    popup.update(cx, |v, _| v.set_filter("unused"));
    cx.run_until_parked();
    let (_, narrowed) = chooser(&bench, cx).unwrap();
    assert!(narrowed.len() < rows.len(), "{narrowed:?}");
    assert!(
        narrowed
            .iter()
            .all(|r| r.to_lowercase().contains("unused") || !r.contains(' ') || r.len() < 30),
        "{narrowed:?}"
    );
    press("enter", cx);
    let (choices, _) = bench
        .read_with(cx, |b, _| b.inspect_dialog())
        .expect("the scope dialog");
    assert_eq!(choices[0], "Whole project");
    let only = bench.read_with(cx, |b, _| b.inspect_only());
    assert!(only.is_some_and(|id| id.contains("unused")), "{only:?}");
    press("escape", cx);

    // Without an open file.
    act(&bench, "CloseAllEditors", cx);
    act(&bench, "CodeInspection.OnEditor", cx);
    assert_eq!(
        status(&bench, cx),
        "Inspect Current File needs a file open in the editor."
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// SPEC-0086 AK-34 (v12): with the log filtered to one file, the list holds every file of the
/// selected commit; Enter opens the diff of the chosen one.
#[gpui::test]
fn all_files_of_a_commit_are_listed(cx: &mut TestAppContext) {
    let root = workspace("affected");
    std::fs::write(root.join(".gitignore"), "/.*\n/target\n").unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test Author"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["add", "-A", "-f"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    std::fs::write(root.join("notes.txt"), "one\nTWO\n").unwrap();
    std::fs::write(root.join("src/b.rs"), "fn b() { }\n").unwrap();
    git(&root, &["commit", "-q", "-am", "second"]);
    let (bench, cx) = open(&root, "notes.txt", cx);
    act(&bench, "Vcs.ShowTabbedFileHistory", cx);
    let log = bench
        .read_with(cx, |b, _| b.git_log())
        .expect("the Git log");
    until(cx, "the history is read", |cx| {
        log.read_with(cx, |l, _| !l.loading() && !l.subjects().is_empty())
    });
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    log.update(cx, |l, cx| l.select_commit(&head, cx));
    cx.run_until_parked();
    act(&bench, "VcsHistory.ShowAllAffected", cx);
    let (title, rows) = chooser(&bench, cx).expect("the files of the commit");
    assert!(title.ends_with(": second"), "{title}");
    assert_eq!(rows, ["M  notes.txt", "M  src/b.rs"]);
    press("down", cx);
    press("enter", cx);
    until(cx, "the diff opens", |cx| {
        bench.read_with(cx, |b, _| b.diff_view().is_some())
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0294 AK-12 (v5): with a bookmark on the caret's line the hint shows its tooltip; a
/// line without icons says so.
#[gpui::test]
fn the_gutter_tooltip_is_shown_from_the_keyboard(cx: &mut TestAppContext) {
    let root = workspace("gutter");
    let (bench, cx) = open(&root, "notes.txt", cx);
    act(&bench, "EditorShowGutterIconTooltip", cx);
    assert_eq!(status(&bench, cx), "No gutter icons on this line");
    act(&bench, "ToggleBookmark", cx);
    // The gutter is drawn again with the bookmark.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    act(&bench, "EditorShowGutterIconTooltip", cx);
    let blocks = bench
        .read_with(cx, |b, _| b.doc_blocks())
        .expect("the hint below the caret");
    assert!(format!("{blocks:?}").contains("Bookmark"), "{blocks:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0100 AK-20 (v5): Recompile starts the build of the file's crate in its Run tab; a file
/// outside every module says so.
#[gpui::test]
fn recompile_builds_the_module_of_the_file(cx: &mut TestAppContext) {
    let root = workspace("recompile");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    act(&bench, "Compile", cx);
    assert_eq!(status(&bench, cx), "Recompile: shop [build]");
    act(&bench, "Stop", cx);
    let elsewhere =
        std::env::temp_dir().join(format!("idecraft-eighth-loose-{}.txt", std::process::id()));
    std::fs::write(&elsewhere, "x\n").unwrap();
    open_file(&bench, &elsewhere, cx);
    act(&bench, "Compile", cx);
    assert_eq!(
        status(&bench, cx),
        "Nothing to compile: the file is in no Maven, Gradle, Cargo or Go module"
    );
    let _ = std::fs::remove_file(&elsewhere);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0095 AK-19 (v11): Introduce Parameter is a refactoring shortcut; in a file without a
/// language it says what is missing.
#[gpui::test]
fn introduce_parameter_is_a_refactoring_shortcut(cx: &mut TestAppContext) {
    let root = workspace("parameter");
    let (bench, cx) = open(&root, "notes.txt", cx);
    act(&bench, "IntroduceParameter", cx);
    assert_eq!(
        status(&bench, cx),
        "Introduce Parameter needs a language server; this file has no language."
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0056 AK-15 and AK-16 (v5): ⇧⌘↩ completes a call and opens a block after an `if`;
/// ⇧⌘⌦ lists the blocks, Enter unwraps the chosen one and Undo restores it.
#[gpui::test]
fn statements_are_completed_and_blocks_unwrapped(cx: &mut TestAppContext) {
    let root = workspace("statement");
    let (bench, cx) = open(&root, "src/Shop.java", cx);
    caret_at(&bench, 4, 22, cx);
    press("cmd-shift-enter", cx);
    assert_eq!(
        text(&bench, cx),
        JAVA.replace("call(1\n", "call(1);\n                \n")
    );
    act(&bench, "$Undo", cx);
    assert_eq!(text(&bench, cx), JAVA);

    caret_at(&bench, 4, 16, cx);
    press("cmd-shift-delete", cx);
    let (title, rows) = chooser(&bench, cx).expect("the blocks around the caret");
    assert_eq!(title, "Unwrap/Remove");
    assert_eq!(rows, ["Unwrap 'if…'", "Unwrap 'for…'"]);
    press("down", cx);
    press("enter", cx);
    assert_eq!(
        text(&bench, cx),
        "class Shop {
    void m(java.util.List<String> list) {
        if (s.isEmpty()) {
            call(1
        }
    }
}
"
    );
    act(&bench, "$Undo", cx);
    assert_eq!(text(&bench, cx), JAVA);
    caret_at(&bench, 0, 0, cx);
    act(&bench, "Unwrap", cx);
    assert_eq!(status(&bench, cx), "Nothing to unwrap here");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0091 AK-24 (v19): without a debug session Quick Evaluate Expression is not available
/// and the Debug window stays closed.
#[gpui::test]
fn quick_evaluate_needs_a_paused_session(cx: &mut TestAppContext) {
    let root = workspace("quick");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    caret_at(&bench, 1, 9, cx);
    act(&bench, "QuickEvaluateExpression", cx);
    assert!(
        status(&bench, cx).contains("not available"),
        "{}",
        status(&bench, cx)
    );
    assert!(bench.read_with(cx, |b, _| b.debug_view()).is_none());
    let _ = std::fs::remove_dir_all(&root);
}
