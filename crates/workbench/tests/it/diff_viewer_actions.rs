//! The diff viewer's own actions, carried out in a window of the test harness (`SPEC-0084` v5
//! AK-15): Accept Left Side and Accept Right Side with what may be written, the current pane,
//! the next and previous file with F7's two presses, and the settings popup.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::diff_actions::{self, Pane};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

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

fn folder(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-diffact-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // The directory as the workbench names it, so paths compare equal.
    root.canonicalize().unwrap()
}

/// A repository with `a.txt`, `b.txt` and `c.txt` committed and then changed: one difference in
/// `a.txt` and `c.txt`, two in `b.txt`.
fn repo(tag: &str) -> PathBuf {
    let root = folder(tag);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test Author"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(name), "one\ntwo\nthree\nfour\nfive\n").unwrap();
    }
    // What the workbench keeps in dot folders of the project is not part of the list of files.
    std::fs::write(root.join(".gitignore"), "/.*\n").unwrap();
    git(&root, &["add", "-A", "-f"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\nfour\nfive\n").unwrap();
    std::fs::write(root.join("b.txt"), "one\nTWO\nthree\nfour\nFIVE\n").unwrap();
    std::fs::write(root.join("c.txt"), "one\ntwo\nthree\nfour\nfive\nsix\n").unwrap();
    root
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

fn show_diff(bench: &Entity<WorkbenchView>, path: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.show_diff(path, window, cx));
    cx.run_until_parked();
}

fn compare(bench: &Entity<WorkbenchView>, left: &Path, right: &Path, cx: &mut VisualTestContext) {
    let (left, right) = (left.to_path_buf(), right.to_path_buf());
    bench.update_in(cx, |b, window, cx| b.compare_files(left, right, window, cx));
    cx.run_until_parked();
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// Title, number of differences and index of the current one of the open viewer.
fn viewer(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> (String, usize, Option<usize>) {
    bench.read_with(cx, |b, cx| {
        let view = b.diff_view().expect("a diff viewer is open");
        let view = view.read(cx);
        (view.title().to_owned(), view.differences(), view.current())
    })
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// SPEC-0084 AK-15 (v5): against HEAD, Accept Right Side is refused and Accept Left Side puts
/// the HEAD lines into the open editor as one undo step, without saving.
#[gpui::test]
fn accept_left_side_changes_the_working_copy_as_one_undo_step(cx: &mut TestAppContext) {
    let root = repo("head");
    let (bench, cx) = open(&root, "b.txt", cx);
    let changed = "one\nTWO\nthree\nfour\nFIVE\n";
    let editor_text =
        |cx: &mut VisualTestContext| bench.read_with(cx, |b, cx| b.active_text(cx)).unwrap();
    show_diff(&bench, "b.txt", cx);
    assert_eq!(viewer(&bench, cx), ("b.txt".to_owned(), 2, Some(0)));

    // The left side is HEAD: nothing is taken into it.
    act(&bench, "Diff.ApplyRightSide", cx);
    assert!(
        status(&bench, cx).contains("is not a file of the project"),
        "{}",
        status(&bench, cx)
    );
    assert_eq!(editor_text(cx), changed);
    assert_eq!(viewer(&bench, cx).1, 2);

    // The first difference goes back to its HEAD line in the editor; the file is not saved.
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(editor_text(cx), "one\ntwo\nthree\nfour\nFIVE\n");
    assert_eq!(read(&root.join("b.txt")), changed);
    assert_eq!(viewer(&bench, cx), ("b.txt".to_owned(), 1, Some(0)));
    assert!(
        status(&bench, cx).contains("not saved"),
        "{}",
        status(&bench, cx)
    );
    // The viewer stays at the same place: the key of the macOS keymap, pressed in the viewer,
    // takes the difference that is left.
    cx.simulate_keystrokes("ctrl-cmd-right");
    cx.run_until_parked();
    assert_eq!(editor_text(cx), "one\ntwo\nthree\nfour\nfive\n");
    assert_eq!(viewer(&bench, cx), ("b.txt".to_owned(), 0, None));
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(status(&bench, cx), diff_actions::NO_DIFFERENCE);

    // Each taking is one undo step of the editor.
    let undo = |cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, _, cx| {
            let editor = b.active_editor_entity().expect("b.txt is open");
            editor.update(cx, |e, cx| e.run_action("$Undo", cx));
        });
        cx.run_until_parked();
    };
    undo(cx);
    assert_eq!(editor_text(cx), "one\ntwo\nthree\nfour\nFIVE\n");
    undo(cx);
    assert_eq!(editor_text(cx), changed);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0084 AK-15 (v5): between two files both directions write the other file; a read-only
/// file stays as it is, and so does a file that changed since the viewer read it.
#[gpui::test]
fn both_directions_between_two_files_and_what_is_refused(cx: &mut TestAppContext) {
    let root = folder("files");
    std::fs::write(root.join("main.txt"), "main\n").unwrap();
    let (left, right) = (root.join("l.txt"), root.join("r.txt"));
    std::fs::write(&left, "a\r\nb\r\nc\r\n").unwrap();
    std::fs::write(&right, "a\r\nB\r\nc\r\nd\r\n").unwrap();
    let (bench, cx) = open(&root, "main.txt", cx);
    compare(&bench, &left, &right, cx);
    assert_eq!(viewer(&bench, cx).1, 2);

    // The changed line from the left file into the right one, with the file's line breaks.
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(read(&right), "a\r\nb\r\nc\r\nd\r\n");
    assert_eq!(read(&left), "a\r\nb\r\nc\r\n");
    assert!(
        status(&bench, cx).starts_with("Accepted the left side: r.txt changed"),
        "{}",
        status(&bench, cx)
    );
    // The added line from the right file into the left one: the files are equal.
    assert_eq!(viewer(&bench, cx).1, 1);
    act(&bench, "Diff.ApplyRightSide", cx);
    assert_eq!(read(&left), "a\r\nb\r\nc\r\nd\r\n");
    assert_eq!(viewer(&bench, cx).1, 0);

    // A read-only file is not written, the other one is.
    let (plain, locked) = (root.join("plain.txt"), root.join("locked.txt"));
    std::fs::write(&plain, "x\n").unwrap();
    std::fs::write(&locked, "y\n").unwrap();
    let mut perms = std::fs::metadata(&locked).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&locked, perms.clone()).unwrap();
    compare(&bench, &plain, &locked, cx);
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(
        status(&bench, cx),
        "locked.txt is read-only: nothing was changed"
    );
    assert_eq!(read(&locked), "y\n");
    assert_eq!(viewer(&bench, cx).1, 1);
    act(&bench, "Diff.ApplyRightSide", cx);
    assert_eq!(read(&plain), "y\n");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&locked, perms).unwrap();

    // A file that changed behind the viewer is not written: the viewer shows it as it is now.
    let (first, second) = (root.join("first.txt"), root.join("second.txt"));
    std::fs::write(&first, "p\n").unwrap();
    std::fs::write(&second, "q\n").unwrap();
    compare(&bench, &first, &second, cx);
    std::fs::write(&second, "q\nmore\n").unwrap();
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(status(&bench, cx), diff_actions::SIDES_CHANGED);
    assert_eq!(read(&second), "q\nmore\n");
    let shown = bench.read_with(cx, |b, cx| b.diff_view().unwrap().read(cx).rows().len());
    assert_eq!(shown, 2);
    // Now the viewer and the files agree again.
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(read(&second), "p\n");

    // A file that is gone is not created again.
    compare(&bench, &plain, &second, cx);
    std::fs::remove_file(&second).unwrap();
    act(&bench, "Diff.ApplyLeftSide", cx);
    assert_eq!(
        status(&bench, cx),
        "second.txt no longer exists: nothing was changed"
    );
    assert!(!second.exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0084 AK-15 (v5): Select Opposite Diff Pane switches the current pane and names its
/// line; the unified view has one pane.
#[gpui::test]
fn the_current_pane_switches(cx: &mut TestAppContext) {
    let root = folder("pane");
    std::fs::write(root.join("main.txt"), "main\n").unwrap();
    let (left, right) = (root.join("l.txt"), root.join("r.txt"));
    std::fs::write(&left, "a\nb\n").unwrap();
    std::fs::write(&right, "a\nnew\nb\n").unwrap();
    let (bench, cx) = open(&root, "main.txt", cx);
    compare(&bench, &left, &right, cx);
    let pane = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| b.diff_view().unwrap().read(cx).pane())
    };
    assert_eq!(pane(cx), Pane::Right);
    act(&bench, "Diff.FocusOppositePane", cx);
    assert_eq!(pane(cx), Pane::Left);
    // Its key is pressed in the viewer below; here the action by its id.
    // The left pane has no line at the added row: the line above it.
    assert_eq!(status(&bench, cx), "Left pane: l.txt, line 1");
    assert_eq!(viewer(&bench, cx).2, Some(0));
    cx.simulate_keystrokes("ctrl-shift-tab");
    cx.run_until_parked();
    assert_eq!(pane(cx), Pane::Right);
    assert_eq!(status(&bench, cx), "Right pane: r.txt, line 2");

    bench.update_in(cx, |b, _, cx| {
        b.diff_view()
            .unwrap()
            .update(cx, |d, _| d.set_unified(true));
    });
    act(&bench, "Diff.FocusOppositePane", cx);
    assert_eq!(status(&bench, cx), diff_actions::ONE_PANE);
    assert_eq!(pane(cx), Pane::Right);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0084 AK-15 (v5): Compare Next File and Compare Previous File go through the changed
/// files without wrapping; F7 at the last difference asks for a second press and then shows the
/// next file, ⇧F7 the previous file at its last difference.
#[gpui::test]
fn the_next_and_previous_file_and_f7_across_files(cx: &mut TestAppContext) {
    let root = repo("chain");
    let (bench, cx) = open(&root, "a.txt", cx);
    show_diff(&bench, "a.txt", cx);
    let title = |cx: &mut VisualTestContext| viewer(&bench, cx).0;

    act(&bench, "Diff.PrevChange", cx);
    assert_eq!(status(&bench, cx), diff_actions::no_further_file(false));
    assert_eq!(title(cx), "a.txt");
    act(&bench, "Diff.NextChange", cx);
    assert_eq!(viewer(&bench, cx), ("b.txt".to_owned(), 2, Some(0)));

    // Within b.txt, then at its last difference: the first press only says so.
    act(&bench, "NextDiff", cx);
    assert_eq!(viewer(&bench, cx).2, Some(1));
    act(&bench, "NextDiff", cx);
    assert_eq!(viewer(&bench, cx), ("b.txt".to_owned(), 2, Some(1)));
    assert_eq!(status(&bench, cx), diff_actions::press_again(true, "c.txt"));
    act(&bench, "NextDiff", cx);
    assert_eq!(viewer(&bench, cx), ("c.txt".to_owned(), 1, Some(0)));

    // c.txt is the last file: F7 stops there. ⇧F7 twice shows b.txt at its last difference.
    act(&bench, "NextDiff", cx);
    assert_eq!(title(cx), "c.txt");
    act(&bench, "PreviousDiff", cx);
    assert_eq!(title(cx), "c.txt");
    assert_eq!(
        status(&bench, cx),
        diff_actions::press_again(false, "b.txt")
    );
    // A press in the other direction forgets the first one.
    act(&bench, "NextDiff", cx);
    act(&bench, "PreviousDiff", cx);
    assert_eq!(title(cx), "c.txt");
    act(&bench, "PreviousDiff", cx);
    assert_eq!(viewer(&bench, cx), ("b.txt".to_owned(), 2, Some(1)));

    // The keys themselves, pressed in the viewer.
    cx.simulate_keystrokes("f7");
    cx.run_until_parked();
    assert_eq!(title(cx), "b.txt");
    cx.simulate_keystrokes("f7");
    cx.run_until_parked();
    assert_eq!(viewer(&bench, cx), ("c.txt".to_owned(), 1, Some(0)));
    act(&bench, "Diff.NextChange", cx);
    assert_eq!(status(&bench, cx), diff_actions::no_further_file(true));
    assert_eq!(title(cx), "c.txt");

    // A comparison of two files has no list.
    compare(&bench, &root.join("a.txt"), &root.join("b.txt"), cx);
    act(&bench, "Diff.NextChange", cx);
    assert_eq!(status(&bench, cx), diff_actions::NO_FILE_LIST);
    act(&bench, "NextDiff", cx);
    act(&bench, "NextDiff", cx);
    act(&bench, "NextDiff", cx);
    assert_eq!(title(cx), "a.txt vs b.txt");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0084 AK-15 (v5): the settings popup lists the two views with the current one marked;
/// choosing the other one switches the viewer, which has the focus again.
#[gpui::test]
fn the_settings_popup_switches_the_view(cx: &mut TestAppContext) {
    let root = repo("settings");
    let (bench, cx) = open(&root, "a.txt", cx);
    show_diff(&bench, "a.txt", cx);
    act(&bench, "Diff.ShowSettingsPopup", cx);
    let popup = bench
        .read_with(cx, |b, _| b.compare_popup())
        .expect("the settings popup is open");
    assert_eq!(
        popup.read_with(cx, |p, _| p.shown_items()),
        diff_actions::settings_rows(false)
    );
    popup.update(cx, |p, cx| {
        p.select(1);
        let chosen = p.accept();
        cx.emit(chosen);
    });
    cx.run_until_parked();
    let (unified, focused) = bench.update_in(cx, |b, window, cx| {
        let view = b.diff_view().expect("the viewer stays open");
        let view = view.read(cx);
        (view.unified(), view.focus().is_focused(window))
    });
    assert!(unified, "the unified view was chosen");
    assert!(focused, "the viewer has the focus again");
    assert!(bench.read_with(cx, |b, _| b.compare_popup()).is_none());
    // The popup now marks the unified view.
    act(&bench, "Diff.ShowSettingsPopup", cx);
    let popup = bench.read_with(cx, |b, _| b.compare_popup()).unwrap();
    assert_eq!(
        popup.read_with(cx, |p, _| p.shown_items()),
        diff_actions::settings_rows(true)
    );
    let _ = std::fs::remove_dir_all(&root);
}
