//! Save All in a window of the test harness (OBS-0065 S1; `SPEC-0058` v4 behaviour 12, AK-7):
//! every changed file of the window is written, in every split group, whichever way the action is
//! chosen — the window's key dispatch (⌘S), the File menu's action, Find Action — with the focus in
//! an editor or in the Project tree. Each file is saved once, as `SaveDocument` saves it.
//!
//! A chord goes through `Window::dispatch_keystroke`; the tests check that the window takes ⌘S
//! itself (one nothing there takes would go on to the menu bar, as the macOS application does
//! it). What needs a real window stays out: the menu bar's key equivalent and the drawn tabs.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::editor_layout::MAIN;
use ferris_idea_workbench::{EditorView, EverywhereEvent, WorkbenchView};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;
type Editor = Entity<EditorView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-save-all-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        std::fs::write(root.join(path), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

/// Opens `file` as the tab in front, the focus in its editor.
fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Editor {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

/// Types `text` at the start of the editor with the focus.
fn type_in_front(bench: &Bench, cx: &mut VisualTestContext, text: &str) -> Editor {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor has the focus");
    editor.update(cx, |e, cx| e.go_to(0, 0, cx));
    cx.simulate_input(text);
    cx.run_until_parked();
    editor
}

/// Opens `file` in front and types `text` at its start.
fn edit(bench: &Bench, cx: &mut VisualTestContext, file: &Path, text: &str) -> Editor {
    open(bench, cx, file);
    type_in_front(bench, cx, text)
}

fn disk(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap()
}

fn modified(editor: &Editor, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |e, _| e.is_modified())
}

fn note(editor: &Editor, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.status().note.unwrap_or_default())
}

/// ⌘S through the window's key dispatch, which takes it.
fn save_all_key(cx: &mut VisualTestContext) {
    let key = gpui::Keystroke::parse("cmd-s").unwrap();
    let taken = cx.update(|window, cx| window.dispatch_keystroke(key, cx));
    cx.run_until_parked();
    assert!(taken, "the window takes ⌘S itself");
}

/// An action chosen in the menu bar.
fn menu(bench: &Bench, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

/// An action chosen in Find Action.
fn find_action(bench: &Bench, cx: &mut VisualTestContext, id: &str) {
    menu(bench, cx, "GotoAction");
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("Find Action is open");
    popup.update(cx, |_, cx| cx.emit(EverywhereEvent::Action(id.into())));
    cx.run_until_parked();
}

fn focus_tree(bench: &Bench, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
}

/// AK-7: two files changed in two tabs are both written by ⌘S, the File menu and Find Action, with
/// the focus in the editor in front and in the Project tree; the file behind gets the
/// EditorConfig on-save edits as the one in front does (behaviour 12a).
#[gpui::test]
fn save_all_writes_every_changed_tab_by_key_menu_and_find_action(cx: &mut TestAppContext) {
    let root = workspace(
        "ways",
        &[
            (
                ".editorconfig",
                "root = true\n\n[*.txt]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
            ),
            ("a.txt", "alpha\nomega"),
            ("b.txt", "beta\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));

    // ⌘S in the editor in front: the tab behind is written too, tidied as on save — the trailing
    // spaces off a line without the caret, a final line break added.
    let ea = open(&bench, cx, &a);
    ea.update(cx, |e, cx| e.go_to(0, 5, cx));
    cx.simulate_input("  ");
    cx.run_until_parked();
    ea.update(cx, |e, cx| e.go_to(1, 0, cx));
    let eb = edit(&bench, cx, &b, "1");
    assert!(modified(&ea, cx) && modified(&eb, cx));
    assert_eq!(
        disk(&a),
        "alpha\nomega",
        "nothing is written before Save All"
    );
    save_all_key(cx);
    assert_eq!(
        disk(&a),
        "alpha\nomega\n",
        "the tab behind is written, tidied"
    );
    assert_eq!(disk(&b), "1beta\n");
    assert!(!modified(&ea, cx) && !modified(&eb, cx));

    // File › Save All with the focus in the editor in front.
    edit(&bench, cx, &a, "2");
    edit(&bench, cx, &b, "2");
    menu(&bench, cx, "SaveAll");
    assert_eq!(
        (disk(&a), disk(&b)),
        ("2alpha\nomega\n".into(), "21beta\n".into())
    );

    // Find Action with the focus in the editor in front.
    edit(&bench, cx, &a, "3");
    edit(&bench, cx, &b, "3");
    find_action(&bench, cx, "SaveAll");
    assert_eq!(
        (disk(&a), disk(&b)),
        ("32alpha\nomega\n".into(), "321beta\n".into())
    );

    // ⌘S with the focus in the Project tree.
    edit(&bench, cx, &a, "4");
    edit(&bench, cx, &b, "4");
    focus_tree(&bench, cx);
    save_all_key(cx);
    assert_eq!(
        (disk(&a), disk(&b)),
        ("432alpha\nomega\n".into(), "4321beta\n".into()),
        "⌘S in the Project tree saves every editor"
    );

    // Find Action chosen with the focus in the Project tree.
    edit(&bench, cx, &a, "5");
    edit(&bench, cx, &b, "5");
    focus_tree(&bench, cx);
    find_action(&bench, cx, "SaveAll");
    assert_eq!(
        (disk(&a), disk(&b)),
        ("5432alpha\nomega\n".into(), "54321beta\n".into())
    );
    assert!(!modified(&ea, cx) && !modified(&eb, cx));
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-7: a file in each split group, and a file shown in both groups, are all written by one
/// Save All from either group; the file shown twice is saved once, so neither view says that it
/// changed on disk (behaviour 12a).
#[gpui::test]
fn save_all_writes_both_split_groups_once_each(cx: &mut TestAppContext) {
    let root = workspace(
        "split",
        &[
            ("one.txt", "one\n"),
            ("two.txt", "two\n"),
            ("three.txt", "three\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let (one, two, three) = (
        root.join("one.txt"),
        root.join("two.txt"),
        root.join("three.txt"),
    );
    open(&bench, cx, &one);
    open(&bench, cx, &two);
    // two.txt goes to a group of its own.
    menu(&bench, cx, "SplitAndMoveRight");
    let groups = bench.read_with(cx, |b, _| b.group_ids());
    assert_eq!(groups.len(), 2, "the editor is split: {groups:?}");
    let main = MAIN;
    let right = groups
        .into_iter()
        .find(|g| *g != MAIN)
        .expect("a second group");
    let focus = |gid, cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| b.focus_group(gid, window, cx));
        cx.run_until_parked();
    };

    // Changed in each group, ⌘S in the right one: both are written.
    focus(main, cx);
    let e_one = type_in_front(&bench, cx, "L");
    focus(right, cx);
    let e_two = type_in_front(&bench, cx, "R");
    assert_ne!(e_one.entity_id(), e_two.entity_id());
    save_all_key(cx);
    assert_eq!((disk(&one), disk(&two)), ("Lone\n".into(), "Rtwo\n".into()));
    assert!(!modified(&e_one, cx) && !modified(&e_two, cx));

    // And with the focus in the main group, by the menu.
    focus(right, cx);
    type_in_front(&bench, cx, "r");
    focus(main, cx);
    type_in_front(&bench, cx, "l");
    menu(&bench, cx, "SaveAll");
    assert_eq!(
        (disk(&one), disk(&two)),
        ("lLone\n".into(), "rRtwo\n".into())
    );

    // three.txt in both groups: typed in its second view, saved once from there.
    open(&bench, cx, &three);
    let first = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("three.txt is open");
    menu(&bench, cx, "SplitVertically");
    let second = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the second view of three.txt");
    assert_ne!(first.entity_id(), second.entity_id());
    let second = type_in_front(&bench, cx, "T");
    focus(right, cx);
    type_in_front(&bench, cx, "x");
    assert!(modified(&first, cx) && modified(&second, cx));
    save_all_key(cx);
    assert_eq!(disk(&three), "Tthree\n");
    assert_eq!(disk(&two), "xrRtwo\n");
    for view in [&first, &second] {
        assert!(!modified(view, cx), "both views of three.txt are saved");
        assert_eq!(note(view, cx), "", "three.txt was written once");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-7 (behaviour 12c): a file that changed on disk stays modified with its editor's note while
/// the other files are written; the next Save All writes it.
#[gpui::test]
fn a_file_changed_on_disk_stays_unsaved_and_the_others_are_written(cx: &mut TestAppContext) {
    let root = workspace("changed", &[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    let (bench, cx) = window(&root, cx);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let ea = edit(&bench, cx, &a, "x");
    let eb = edit(&bench, cx, &b, "y");
    std::fs::write(&a, "changed elsewhere\n").unwrap();

    save_all_key(cx);
    assert_eq!(disk(&b), "ybeta\n", "the other file is written");
    assert_eq!(
        disk(&a),
        "changed elsewhere\n",
        "the first save does not overwrite another program's change"
    );
    assert!(modified(&ea, cx) && !modified(&eb, cx));
    assert_eq!(
        note(&ea, cx),
        "The file changed on disk since it was opened: save again to overwrite it."
    );

    save_all_key(cx);
    assert_eq!(disk(&a), "xalpha\n", "the second Save All overwrites it");
    assert!(!modified(&ea, cx));
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-7 (behaviour 12b): a read-only file in front is not written and does not keep the file
/// behind it from being saved.
#[gpui::test]
fn a_read_only_editor_in_front_does_not_stop_save_all(cx: &mut TestAppContext) {
    let root = workspace(
        "read-only",
        &[("locked.txt", "locked\n"), ("b.txt", "beta\n")],
    );
    let locked = root.join("locked.txt");
    let set_read_only = |on: bool| {
        let mut p = std::fs::metadata(&locked).unwrap().permissions();
        p.set_readonly(on);
        std::fs::set_permissions(&locked, p).unwrap();
    };
    set_read_only(true);
    let (bench, cx) = window(&root, cx);
    let b = root.join("b.txt");
    let eb = edit(&bench, cx, &b, "y");
    let front = open(&bench, cx, &locked);
    assert!(front.read_with(cx, |e, _| e.is_read_only()));

    save_all_key(cx);
    assert_eq!(disk(&b), "ybeta\n");
    assert_eq!(disk(&locked), "locked\n");
    assert!(!modified(&eb, cx));

    // The menu too.
    edit(&bench, cx, &b, "z");
    open(&bench, cx, &locked);
    menu(&bench, cx, "SaveAll");
    assert_eq!(disk(&b), "zybeta\n");
    set_read_only(false);
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-7 (behaviour 12e): `SaveDocument` writes only its own editor's file.
#[gpui::test]
fn save_document_writes_only_its_own_file(cx: &mut TestAppContext) {
    let root = workspace("one", &[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
    let (bench, cx) = window(&root, cx);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let ea = edit(&bench, cx, &a, "x");
    let eb = edit(&bench, cx, &b, "y");
    eb.update(cx, |e, cx| {
        assert!(e.run_action("SaveDocument", cx));
    });
    cx.run_until_parked();
    assert_eq!(disk(&b), "ybeta\n");
    assert_eq!(disk(&a), "alpha\n", "the other file is left alone");
    assert!(modified(&ea, cx) && !modified(&eb, cx));
    let _ = std::fs::remove_dir_all(&root);
}

/// Behaviour 12e: an editor outside a project window (`ferris-idea edit`) saves its own file on
/// `SaveAll`.
#[gpui::test]
fn an_editor_of_its_own_saves_its_file_on_save_all(cx: &mut TestAppContext) {
    let root = workspace("alone", &[("a.txt", "alpha\n")]);
    let file = root.join("a.txt");
    let editor = cx.new(|cx| EditorView::new("alpha\n", Some(file.clone()), cx));
    editor.update(cx, |e, cx| {
        e.replace_ranges(&[(0..0, "x".into())], cx);
        assert!(e.run_action("SaveAll", cx));
        assert!(!e.is_modified());
    });
    assert_eq!(disk(&file), "xalpha\n");
    let _ = std::fs::remove_dir_all(&root);
}
