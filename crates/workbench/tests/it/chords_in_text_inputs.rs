//! Chords pressed while a text input has the focus (`SPEC-0028` v6 behaviour 11), in a window of
//! the test harness. Every key goes through the window's key dispatch
//! (`simulate_keystrokes`), the entry the macOS platform code calls first for a chord with ⌃
//! while no input method composes; the way from the keyboard to that entry (Cocoa, the toolkit's
//! naming of the key) is not run here. Needs the `git` program to make the repository.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, EntityInputHandler, Focusable, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{CommitView, EditorView, WorkbenchView};

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A repository with one commit, a changed file and a source file; `None` without `git`.
fn workspace(tag: &str) -> Option<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "idecraft-chords-in-text-inputs-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    std::fs::write(root.join("call.txt"), "f(a, b)\n").unwrap();
    if !git(&root, &["init", "-q", "-b", "main"]) {
        eprintln!("skipped: no git");
        return None;
    }
    for args in [
        &["config", "user.name", "T"][..],
        &["config", "user.email", "t@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        &["add", "."],
        &["commit", "-q", "-m", "base"],
    ] {
        assert!(git(&root, args), "git {args:?}");
    }
    Some(root.canonicalize().unwrap())
}

fn open(
    root: &Path,
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<CommitView>,
    &'static mut VisualTestContext,
) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ActivateCommitToolWindow", window, cx)
    });
    cx.run_until_parked();
    let commit = bench
        .read_with(cx, |b, _| b.commit_view())
        .expect("the Commit tool window is open");
    (bench, commit, cx)
}

/// Commits a change of the tracked file from the window, so the history has `message`.
fn commit(root: &Path, view: &Entity<CommitView>, cx: &mut VisualTestContext, message: &str) {
    std::fs::write(root.join("a.txt"), format!("{message}\n")).unwrap();
    view.update(cx, |v, cx| {
        v.refresh();
        v.check_only(&["a.txt"], cx);
        v.set_message(message, cx);
        v.commit_now(cx);
    });
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |v, _| v
            .note()
            .is_some_and(|n| n.starts_with("Committed"))),
        "{message:?}"
    );
}

fn focus_message(view: &Entity<CommitView>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    let field = view.read_with(cx, |v, _| v.message_field());
    field.update_in(cx, |f, window, _| window.focus(&f.focus()));
    cx.run_until_parked();
    field
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn history_open(view: &Entity<CommitView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |v, _| v.message_history().is_some())
}

/// SPEC-0028 AK-12 (v6), the commit message field: ⌃M is Commit Message History before the
/// field's Move Caret to Matching Brace; Return stays a line break; the input method goes first.
#[gpui::test]
fn ctrl_m_in_the_commit_message_field_opens_the_message_history(cx: &mut TestAppContext) {
    let Some(root) = workspace("message") else {
        return;
    };
    let (_bench, view, cx) = open(&root, cx);
    commit(&root, &view, cx, "Add the cart");
    let field = focus_message(&view, cx);
    view.update(cx, |v, cx| v.set_message("half (a) thought", cx));

    keys(cx, "ctrl-m");
    assert_eq!(
        view.read_with(cx, |v, _| v.message_history()),
        Some((vec!["Add the cart".to_owned()], 0))
    );
    assert!(
        field.update_in(cx, |f, window, _| f.focus().is_focused(window)),
        "the focus stays in the message field"
    );
    // The same chord closes the list; the text is as typed.
    keys(cx, "ctrl-m");
    assert!(!history_open(&view, cx));
    assert_eq!(
        view.read_with(cx, |v, cx| v.message(cx)),
        "half (a) thought"
    );

    // Return is another keystroke: a line break, no list.
    view.update(cx, |v, cx| v.set_message("", cx));
    keys(cx, "enter");
    assert!(!history_open(&view, cx));
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), "\n");

    // While the input method composes, the key is not the keymap's.
    view.update(cx, |v, cx| v.set_message("", cx));
    field.update_in(cx, |f, window, cx| {
        f.replace_and_mark_text_in_range(None, "か", None, window, cx);
    });
    assert!(view.read_with(cx, |v, cx| v.composing(cx)));
    let grouped = view.read_with(cx, |v, _| v.group_by().module);
    keys(cx, "ctrl-m");
    assert!(!history_open(&view, cx), "the input method owns the key");
    assert_eq!(view.read_with(cx, |v, _| v.group_by().module), grouped);
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), "か");
    // Once the composition is committed the chord is the field's again.
    field.update_in(cx, |f, window, cx| {
        f.replace_text_in_range(None, "か", window, cx);
    });
    assert!(!view.read_with(cx, |v, cx| v.composing(cx)));
    keys(cx, "ctrl-m");
    assert!(history_open(&view, cx));
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), "か");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0028 AK-12 (v6), the Commit tool window around its fields: in the message field the
/// editing chords of the keymap stay (⌃A, ⌃E, ⌃P), ⌃P is not Group By Directory there, and ⌃O,
/// which the field has no action for, opens the commit options.
#[gpui::test]
fn editing_chords_stay_in_the_commit_message_field(cx: &mut TestAppContext) {
    let Some(root) = workspace("editing") else {
        return;
    };
    let (_bench, view, cx) = open(&root, cx);
    let field = focus_message(&view, cx);
    view.update(cx, |v, cx| v.set_message("one\nlonger two", cx));
    let caret = |cx: &mut VisualTestContext| field.read_with(cx, |f, _| f.caret());
    let grouping = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| (v.group_by().directory, v.group_by().module))
    };
    let before = grouping(cx);

    // The second line: its end, its start.
    keys(cx, "cmd-end ctrl-e");
    assert_eq!(caret(cx), "one\nlonger two".chars().count());
    keys(cx, "ctrl-a");
    assert_eq!(caret(cx), 4);
    // ⌃F and ⌃B: one character forward and back.
    keys(cx, "ctrl-f ctrl-f ctrl-b");
    assert_eq!(caret(cx), 5);
    // ⌃P: one line up, and the list is grouped as before. ⌃N: down again.
    keys(cx, "ctrl-p");
    assert!(caret(cx) <= 3, "the caret is in the first line");
    assert_eq!(grouping(cx), before);
    keys(cx, "ctrl-n");
    assert!(caret(cx) >= 4, "the caret is in the second line");
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), "one\nlonger two");
    assert!(!history_open(&view, cx));

    // A chord nothing takes (⌃Y, ⌃Q) may come back from the text system as a control
    // character to type: it is not inserted. Text and a line break are.
    keys(cx, "ctrl-y ctrl-q");
    field.update_in(cx, |f, window, cx| {
        f.replace_text_in_range(None, "\u{19}", window, cx);
        f.replace_text_in_range(None, "\u{11}", window, cx);
    });
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), "one\nlonger two");
    field.update_in(cx, |f, window, cx| {
        f.replace_text_in_range(None, "!", window, cx);
    });
    assert!(view.read_with(cx, |v, cx| v.message(cx)).contains('!'));

    // ⌃O is no editing chord: the window around the field takes it.
    assert!(!view.read_with(cx, |v, _| v.options_open()));
    keys(cx, "ctrl-o");
    assert!(view.read_with(cx, |v, _| v.options_open()));

    // The author field is another input: ⌃M there is not the history of the message field.
    let author = view.read_with(cx, |v, _| v.author_field());
    author.update_in(cx, |f, window, _| window.focus(&f.focus()));
    cx.run_until_parked();
    keys(cx, "ctrl-m");
    assert!(!history_open(&view, cx));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0028 AK-12 (v6.1), SPEC-0057 v6 behaviour 24: ⌃K in the commit message field cuts from
/// the caret to the end of the line and fills the clipboard; at a line end it takes the line
/// break; at the end of the text it changes nothing.
#[gpui::test]
fn ctrl_k_in_the_commit_message_field_cuts_to_the_line_end(cx: &mut TestAppContext) {
    let Some(root) = workspace("cut") else {
        return;
    };
    let (_bench, view, cx) = open(&root, cx);
    let field = focus_message(&view, cx);
    view.update(cx, |v, cx| v.set_message("one two\nthree", cx));
    let message = |cx: &mut VisualTestContext| view.read_with(cx, |v, cx| v.message(cx));
    let clipboard = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|i| i.text());

    // The start of the text, then four characters on: before `two`.
    keys(cx, "cmd-home ctrl-f ctrl-f ctrl-f ctrl-f");
    assert_eq!(field.read_with(cx, |f, _| f.caret()), 4);
    keys(cx, "ctrl-k");
    assert_eq!(message(cx), "one \nthree");
    assert_eq!(clipboard(cx).as_deref(), Some("two"));
    assert_eq!(field.read_with(cx, |f, _| f.caret()), 4);
    assert!(!history_open(&view, cx));
    // Only the line break is left of the line: it goes, and the next line joins.
    keys(cx, "ctrl-k");
    assert_eq!(message(cx), "one three");
    // At the end of the text there is nothing to cut.
    keys(cx, "cmd-end");
    let before = clipboard(cx);
    keys(cx, "ctrl-k");
    assert_eq!(message(cx), "one three");
    assert_eq!(clipboard(cx), before);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0028 AK-12 (v6), the rest of the Commit tool window: with the focus in the file list ⌃M
/// and ⌃P group by module and directory, and no history opens (SPEC-0063 behaviours 21 and 22).
#[gpui::test]
fn ctrl_m_in_the_commit_file_list_groups_by_module(cx: &mut TestAppContext) {
    let Some(root) = workspace("list") else {
        return;
    };
    let (_bench, view, cx) = open(&root, cx);
    commit(&root, &view, cx, "Add the cart");
    view.update_in(cx, |v, window, cx| window.focus(&v.focus_handle(cx)));
    cx.run_until_parked();
    let grouping = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| (v.group_by().directory, v.group_by().module))
    };
    let (directory, module) = grouping(cx);

    keys(cx, "ctrl-m");
    assert_eq!(grouping(cx), (directory, !module));
    assert!(!history_open(&view, cx));
    keys(cx, "ctrl-p");
    assert_eq!(grouping(cx), (!directory, !module));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0028 AK-12 (v6), the editor: ⌃M is Move Caret to Matching Brace, also while the Commit
/// tool window is shown; no history opens.
#[gpui::test]
fn ctrl_m_in_the_editor_goes_to_the_matching_brace(cx: &mut TestAppContext) {
    let Some(root) = workspace("editor") else {
        return;
    };
    let (bench, view, cx) = open(&root, cx);
    commit(&root, &view, cx, "Add the cart");
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("call.txt"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    // The caret after `f`, before the opening parenthesis.
    keys(cx, "cmd-home right");
    assert_eq!(editor.read_with(cx, |e, _| e.caret()), 1);
    keys(cx, "ctrl-m");
    let there = editor.read_with(cx, |e, _| e.caret());
    assert!(there > 1, "the caret went to the closing parenthesis");
    keys(cx, "ctrl-m");
    assert!(editor.read_with(cx, |e, _| e.caret()) < there);
    assert!(!history_open(&view, cx));
    let _ = std::fs::remove_dir_all(&root);
}
