//! The gaps of the window checks of 2026-11-18 in a window of the test harness, with keys where
//! the gap was found with keys: a template's places with their name list (`SPEC-0072` v19
//! behaviour 51, OBS-0086 K2, K3), Move (F6) from the Project tree as a step of its history and
//! the checks before the question (`SPEC-0275` v22 behaviours 38 and 49, OBS-0088 U1, U3), the
//! tree following the disk (`SPEC-0026` v4 behaviour 6, OBS-0088 U2) and the file's own
//! indentation (`SPEC-0150` v5 behaviour 7, OBS-0087 W4).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_languages::insight::CompletionItem;
use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)], folders: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-gaps-1118-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for dir in folders {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    cx.executor().allow_parking();
    // Questions are drawn in the window, as in the window self-test.
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

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(250));
    }
    panic!("timed out: {what}");
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// The text the active editor has selected.
fn selected(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        let e = b.active_editor_entity().unwrap();
        let e = e.read(cx);
        e.selection_range()
            .map(|r| {
                e.state
                    .buffer
                    .text()
                    .chars()
                    .skip(r.start)
                    .take(r.len())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn list(bench: &Bench, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    bench.read_with(cx, |b, _| b.completion_items())
}

fn focus_tree(bench: &Bench, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
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

/// A chord as the macOS application presses it: the window first, then the menu bar's action.
fn chord(bench: &Bench, cx: &mut VisualTestContext, keystroke: &str, action: &str) {
    let key = gpui::Keystroke::parse(keystroke).unwrap();
    let taken = cx.update(|window, cx| window.dispatch_keystroke(key, cx));
    cx.run_until_parked();
    if !taken {
        bench.update_in(cx, |b, window, cx| b.menu_action(action, window, cx));
        cx.run_until_parked();
    }
}

fn question(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, _| b.drawn_question().map(|q| q.message.clone()))
}

/// The tree's state as the MCP tool reports it, as text.
fn tree(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.project_tree_state().to_string())
}

/// F6 in the tree on `path`, then `dir` in the folder field and Return; waits for the move.
fn move_in_tree(bench: &Bench, cx: &mut VisualTestContext, path: &str, dir: &str) {
    focus_tree(bench, cx);
    select(bench, cx, path);
    keys(cx, "f6");
    let moving = bench.read_with(cx, |b, _| b.moving().map(Path::to_path_buf));
    let root = bench.read_with(cx, |b, _| b.project_root().to_path_buf());
    assert_eq!(
        moving,
        Some(root.join(path)),
        "F6 moves the tree's selection"
    );
    bench.update_in(cx, |b, _, cx| {
        let g = b.goto_popup().expect("the folder field is open");
        g.update(cx, |g, cx| g.set_field_text(dir, cx));
    });
    cx.run_until_parked();
    keys(cx, "enter");
    let name = Path::new(path).file_name().unwrap().to_owned();
    let target = root.join(dir).join(name);
    until(cx, 30, "the move", |_| target.exists());
}

/// SPEC-0072 AK-40 (v19, behaviour 51; OBS-0086 K2, K3): Tab on the name list of a place takes
/// the name and goes on in one press; ⇧Tab at the first place stays there without unindenting,
/// also with the list open.
#[gpui::test]
fn tab_on_a_places_name_list_goes_on(cx: &mut TestAppContext) {
    let start = "class A {\n    void f(java.util.List<String> names) {\n        \n    }\n}\n";
    let root = workspace(
        "names",
        &[
            ("A.java", start),
            ("B.java", &start.replace("class A", "class B")),
        ],
        &[],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("A.java"));
    keys(cx, "down down end");
    let template = || CompletionItem {
        label: "iter".into(),
        insert: "for (String ${1:name} : ${2:names}) {\n    $0\n}".into(),
        kind: Some("snippet".into()),
        snippet: true,
        ..Default::default()
    };
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![template()], false, cx))
    });
    keys(cx, "enter");
    assert_eq!(selected(&bench, cx), "name");
    let names = list(&bench, cx).expect("the place's name list is open");
    assert!(names.len() > 1, "{names:?}");
    let line = |bench: &Bench, cx: &mut VisualTestContext| {
        text(bench, cx)
            .lines()
            .nth(2)
            .unwrap_or_default()
            .to_owned()
    };
    let inserted = line(&bench, cx);
    assert!(
        inserted.starts_with("        for (String name"),
        "{inserted:?}"
    );

    // K3 with the list open: ⇧Tab at the first place closes the list and stays.
    keys(cx, "shift-tab");
    assert_eq!(list(&bench, cx), None);
    assert_eq!(selected(&bench, cx), "name");
    assert_eq!(line(&bench, cx), inserted, "no unindent");

    // K2: in a second file, the list open; one Tab takes the name and goes on.
    open(&bench, cx, &root.join("B.java"));
    keys(cx, "down down end");
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![template()], false, cx))
    });
    keys(cx, "enter");
    assert!(list(&bench, cx).is_some(), "the name list is open again");
    keys(cx, "tab");
    assert_eq!(
        selected(&bench, cx),
        "names",
        "one Tab goes on to the next place"
    );
    assert_eq!(list(&bench, cx), None);

    // ⇧Tab back, and once more at the first place: it stays.
    keys(cx, "shift-tab");
    assert_eq!(selected(&bench, cx), "name");
    let at_first = line(&bench, cx);
    keys(cx, "shift-tab");
    assert_eq!(
        selected(&bench, cx),
        "name",
        "⇧Tab at the first place stays"
    );
    assert_eq!(line(&bench, cx), at_first, "no unindent");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0275 AK-45 (v22, behaviour 49; OBS-0088 U1): F6 in the tree moves the tree's selection,
/// not the editor's file; ⌘Z in the tree asks and puts it back, ⇧⌘Z moves it again; a folder
/// too; a new file at the old place refuses the undo without asking (AK-46).
#[gpui::test]
fn a_move_from_the_tree_is_undone_from_the_tree(cx: &mut TestAppContext) {
    let root = workspace(
        "move",
        &[
            ("notes.txt", "n\n"),
            ("open.txt", "o\n"),
            ("box/in.txt", "i\n"),
        ],
        &["docs"],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("open.txt"));

    move_in_tree(&bench, cx, "notes.txt", "docs");
    assert!(!root.join("notes.txt").exists());
    assert!(root.join("open.txt").exists(), "the editor's file stays");

    focus_tree(&bench, cx);
    chord(&bench, cx, "cmd-z", "$Undo");
    let asked = question(&bench, cx).expect("undo asks");
    assert!(asked.contains("notes.txt"), "{asked}");
    keys(cx, "enter");
    assert!(root.join("notes.txt").is_file() && !root.join("docs/notes.txt").exists());
    focus_tree(&bench, cx);
    chord(&bench, cx, "cmd-shift-z", "$Redo");
    assert!(question(&bench, cx).is_some(), "redo asks");
    keys(cx, "enter");
    assert!(root.join("docs/notes.txt").is_file() && !root.join("notes.txt").exists());

    // A folder with its file.
    move_in_tree(&bench, cx, "box", "docs");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/box/in.txt")).unwrap(),
        "i\n"
    );
    assert!(
        tree(&bench, cx).contains("docs/box"),
        "{}",
        tree(&bench, cx)
    );
    focus_tree(&bench, cx);
    chord(&bench, cx, "cmd-z", "$Undo");
    assert!(question(&bench, cx).is_some_and(|q| q.contains("box")));
    keys(cx, "enter");
    assert!(root.join("box/in.txt").is_file() && !root.join("docs/box").exists());

    // The step before: `notes.txt` into `docs`, now with a new `notes.txt` at the old place.
    std::fs::write(root.join("notes.txt"), "someone else's\n").unwrap();
    focus_tree(&bench, cx);
    chord(&bench, cx, "cmd-z", "$Undo");
    assert_eq!(question(&bench, cx), None, "a refused step asks nothing");
    let said = status(&bench, cx);
    assert!(
        said.contains("refused") && said.contains("notes.txt"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "someone else's\n"
    );
    assert!(root.join("docs/notes.txt").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0275 AK-46 (v22, behaviour 38; OBS-0088 U3): after a rename and a new file at the old
/// name, ⌘Z in the tree shows no question; the status bar names the file; the step stays.
#[gpui::test]
fn a_refused_step_asks_nothing(cx: &mut TestAppContext) {
    let root = workspace("refused", &[("README.txt", "r\n")], &[]);
    let (bench, cx) = window(&root, cx);
    let file = root.join("README.txt");
    bench.update_in(cx, |b, window, cx| b.start_rename(file.clone(), window, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, _, cx| b.set_name_field("README2.txt", cx));
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(root.join("README2.txt").is_file());
    std::fs::write(&file, "new\n").unwrap();
    focus_tree(&bench, cx);
    chord(&bench, cx, "cmd-z", "$Undo");
    assert_eq!(question(&bench, cx), None);
    let said = status(&bench, cx);
    assert!(said.contains("README.txt"), "{said}");
    let next = bench.read_with(cx, |b, _| b.file_undo_state());
    assert_eq!(next["undo"].as_array().map(Vec::len), Some(1), "{next}");
    std::fs::remove_file(&file).unwrap();
    chord(&bench, cx, "cmd-z", "$Undo");
    assert!(
        question(&bench, cx).is_some(),
        "asks once the reason is gone"
    );
    keys(cx, "enter");
    assert!(file.is_file() && !root.join("README2.txt").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0026 AK-6 (v4, behaviour 6; OBS-0088 U2): a file another program writes into a shown
/// folder appears in the tree at the next look, without activating the window, and goes again.
#[gpui::test]
fn a_file_created_outside_shows_in_the_tree(cx: &mut TestAppContext) {
    let root = workspace("outside", &[("a.txt", "a\n")], &[]);
    let (bench, cx) = window(&root, cx);
    // A first look sets the baseline.
    for _ in 0..12 {
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
    }
    assert!(!tree(&bench, cx).contains("from-outside.txt"));
    std::fs::write(root.join("from-outside.txt"), "x\n").unwrap();
    until(cx, 20, "the new file shows", |cx| {
        tree(&bench, cx).contains("from-outside.txt")
    });
    std::fs::remove_file(root.join("from-outside.txt")).unwrap();
    until(cx, 20, "the deleted file goes", |cx| {
        !tree(&bench, cx).contains("from-outside.txt")
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0150 AK-7 (v5, behaviour 7; OBS-0087 W4): a CSS file indented with two spaces is
/// indented with two, although Code Style says four for CSS; an `.editorconfig` wins.
#[gpui::test]
fn a_files_own_indentation_wins_over_code_style(cx: &mut TestAppContext) {
    let css = ".card {\n  display: flex;\n  color: red;\n}\n";
    let root = workspace(
        "indent",
        &[("web/style.css", css), ("cfg/style.css", css)],
        &[],
    );
    std::fs::write(
        root.join("cfg/.editorconfig"),
        "root = true\n\n[*]\nindent_style = space\nindent_size = 4\n",
    )
    .unwrap();
    let (bench, cx) = window(&root, cx);
    let unit = |bench: &Bench, cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.active_editor_entity()
                .map(|e| e.read(cx).indent_unit().to_owned())
                .unwrap_or_default()
        })
    };
    open(&bench, cx, &root.join("web/style.css"));
    assert_eq!(unit(&bench, cx), "  ");
    bench.update_in(cx, |b, _, cx| {
        let e = b.active_editor_entity().unwrap();
        let end = e.read(cx).state.buffer.len_chars();
        e.update(cx, |e, cx| e.select_range(end..end, cx));
    });
    keys(cx, "tab");
    assert!(text(&bench, cx).ends_with("\n  "), "{:?}", text(&bench, cx));
    open(&bench, cx, &root.join("cfg/style.css"));
    assert_eq!(unit(&bench, cx), "    ", "the .editorconfig wins");
    let _ = std::fs::remove_dir_all(&root);
}
