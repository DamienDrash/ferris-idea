//! Commit Message History in a window of the test harness (`SPEC-0063` v10 behaviour 22): a
//! commit from the Commit tool window remembers its message in the project, the action lists the
//! messages newest first, choosing one replaces the field's text as one undo step, and a broken
//! history file is said in the status bar. Needs the `git` program to make the repository.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::commit_messages::{COMMIT_MESSAGES_FILE, CommitMessages};
use ferris_idea_workbench::commit_model::NO_MESSAGE_HISTORY;
use ferris_idea_workbench::keymap_actions::MESSAGE_HISTORY;
use ferris_idea_workbench::{CommitView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A repository with one commit and one changed file; `None` without `git`.
fn workspace(tag: &str) -> Option<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-commit-message-history-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
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

fn action(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

/// Changes the tracked file and commits it from the window with `message`.
fn commit(
    root: &Path,
    commit: &Entity<CommitView>,
    cx: &mut VisualTestContext,
    n: usize,
    message: &str,
) {
    std::fs::write(root.join("a.txt"), format!("change {n}\n")).unwrap();
    commit.update(cx, |v, cx| {
        v.refresh();
        v.check_only(&["a.txt"], cx);
        v.set_message(message, cx);
        v.commit_now(cx);
    });
    cx.run_until_parked();
    assert!(
        commit.read_with(cx, |v, _| v
            .note()
            .is_some_and(|n| n.starts_with("Committed"))),
        "{message:?}: {:?}",
        commit.read_with(cx, |v, _| v.note().map(str::to_owned))
    );
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// SPEC-0063 AK-12 (v10): commits remember their messages; the action lists them newest first;
/// Down and Enter choose; one undo brings the typed text back.
#[gpui::test]
fn committed_messages_are_listed_and_a_chosen_one_replaces_the_text(cx: &mut TestAppContext) {
    let Some(root) = workspace("choose") else {
        return;
    };
    let (bench, view, cx) = open(&root, cx);

    // Nothing remembered yet: no list, and the status bar says so.
    action(&bench, cx, MESSAGE_HISTORY);
    assert_eq!(view.read_with(cx, |v, _| v.message_history()), None);
    assert_eq!(status(&bench, cx), NO_MESSAGE_HISTORY);
    assert!(!root.join(COMMIT_MESSAGES_FILE).exists());

    let body = "Fix the total\n\nIt counted the first item twice.";
    commit(&root, &view, cx, 1, "Add the cart");
    commit(&root, &view, cx, 2, body);
    commit(&root, &view, cx, 3, "Add the cart");
    // The file holds what was typed, newest first, each once, and Git is told to ignore it.
    let (kept, problem) = CommitMessages::load(&root);
    assert_eq!(problem, None);
    assert_eq!(kept.newest_first(), ["Add the cart", body]);
    let ignore = std::fs::read_to_string(root.join(".ferris-idea/.gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == "commit-messages.toml"));

    // The field is empty after a commit; something is typed, then the history is asked for.
    view.update(cx, |v, cx| v.set_message("half a thought", cx));
    action(&bench, cx, MESSAGE_HISTORY);
    assert_eq!(
        view.read_with(cx, |v, _| v.message_history()),
        Some((
            vec![
                "Add the cart".to_owned(),
                "Fix the total↵↵It counted the first item twice.".to_owned()
            ],
            0
        ))
    );
    assert!(
        view.update_in(cx, |v, window, cx| v.focus(cx).is_focused(window)),
        "the message field has the focus"
    );
    // Down selects the second row, Enter takes it: the message itself, with its line breaks.
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.message_history()), None);
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), body);
    // One undo step.
    view.update(cx, |v, cx| v.undo_message(false, cx));
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), "half a thought");
    view.update(cx, |v, cx| v.undo_message(true, cx));
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), body);

    // Escape closes the list and leaves the text; the action closes an open list too.
    action(&bench, cx, MESSAGE_HISTORY);
    assert!(view.read_with(cx, |v, _| v.message_history()).is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.message_history()), None);
    assert_eq!(view.read_with(cx, |v, cx| v.message(cx)), body);
    action(&bench, cx, MESSAGE_HISTORY);
    action(&bench, cx, MESSAGE_HISTORY);
    assert_eq!(view.read_with(cx, |v, _| v.message_history()), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0063 AK-12 (v10): a history file that cannot be used opens no list; the status bar
/// names the file and the reason, never its content; the next commit starts a good file.
#[gpui::test]
fn a_broken_history_file_is_said_and_replaced_by_the_next_commit(cx: &mut TestAppContext) {
    let Some(root) = workspace("broken") else {
        return;
    };
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    std::fs::write(
        root.join(COMMIT_MESSAGES_FILE),
        "version = 1\nmessage = [\"s3cr3t-token\"\n",
    )
    .unwrap();
    let (bench, view, cx) = open(&root, cx);
    action(&bench, cx, MESSAGE_HISTORY);
    assert_eq!(view.read_with(cx, |v, _| v.message_history()), None);
    let said = status(&bench, cx);
    assert!(said.contains(COMMIT_MESSAGES_FILE), "{said}");
    assert!(said.contains("was not read"), "{said}");
    assert!(!said.contains("s3cr3t"), "{said}");

    commit(&root, &view, cx, 1, "After the break");
    action(&bench, cx, MESSAGE_HISTORY);
    assert_eq!(
        view.read_with(cx, |v, _| v.message_history()),
        Some((vec!["After the break".to_owned()], 0))
    );
    let _ = std::fs::remove_dir_all(&root);
}
