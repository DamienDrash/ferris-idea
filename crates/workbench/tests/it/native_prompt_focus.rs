//! `SPEC-0281` v3 behaviour 8, AK-7, in a window of the toolkit's test platform: the native alerts
//! IDECraft asks hold the keyboard focus on the answer that changes nothing — the cancel answer,
//! else the default — so Space (with macOS keyboard navigation on) never deletes a file or drops
//! unsaved changes; Return keeps giving the default. Pressing the focused answer is what Space
//! does. Which button the system alert draws with the focus ring needs a screen (AK-8).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::tabs::CloseScope;

/// A project with `notes.txt` and a window of the test platform on it.
fn window(
    tag: &str,
    cx: &mut TestAppContext,
) -> (
    PathBuf,
    Entity<WorkbenchView>,
    &'static mut VisualTestContext,
) {
    let root = std::env::temp_dir().join(format!(
        "idecraft-prompt-focus-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "first line\n").unwrap();
    let root = root.canonicalize().unwrap();
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.clone();
    let handle = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = handle.root(cx).unwrap();
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (root, bench, cx)
}

fn unsaved(file: &Path, cx: &mut VisualTestContext) -> bool {
    let (unsaved, _) = cx.update(|_, cx| WorkbenchView::pending_work(cx));
    unsaved.iter().any(|f| file.ends_with(f))
}

/// Answers the pending question with its focused answer, as Space does; returns that answer.
fn press_focused(cx: &mut VisualTestContext) -> String {
    let focused = cx
        .pending_prompt_focused_answer()
        .expect("the question focuses an answer");
    cx.simulate_prompt_answer(&focused);
    cx.run_until_parked();
    focused
}

#[gpui::test]
fn closing_unsaved_tabs_focuses_cancel(cx: &mut TestAppContext) {
    let (root, bench, cx) = window("close", cx);
    let file = root.join("notes.txt");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    cx.simulate_input("changed ");
    cx.run_until_parked();
    assert!(unsaved(&file, cx), "the edit is unsaved");

    bench.update_in(cx, |b, window, cx| {
        b.close_several(CloseScope::All, window, cx)
    });
    cx.run_until_parked();
    assert!(
        cx.has_pending_prompt(),
        "closing asks about the unsaved file"
    );
    let (message, _) = cx.pending_prompt().unwrap();
    assert!(message.contains("notes.txt"), "{message}");
    assert_eq!(
        press_focused(cx),
        "Cancel",
        "the cancel answer holds the keyboard focus, not Save All or Don't Save"
    );
    assert!(unsaved(&file, cx), "Cancel keeps the tab and the edit");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "first line\n");

    // Leaves no unsaved document behind for the other window tests of this process.
    bench.update_in(cx, |b, window, cx| {
        b.close_several(CloseScope::All, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Don't Save");
    cx.run_until_parked();
    assert!(!unsaved(&file, cx));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn delete_in_the_project_tree_focuses_cancel_and_keeps_the_file(cx: &mut TestAppContext) {
    let (root, bench, cx) = window("delete", cx);
    let file = root.join("notes.txt");
    bench.update_in(cx, |b, window, cx| {
        b.ask_delete(std::slice::from_ref(&file), window, cx)
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "Delete asks first");
    let (message, _) = cx.pending_prompt().unwrap();
    assert!(message.contains("notes.txt"), "{message}");
    assert_eq!(press_focused(cx), "Cancel");
    assert!(!cx.has_pending_prompt());
    assert!(file.exists(), "Space deletes nothing");

    // Return still deletes: the default answer is Delete.
    bench.update_in(cx, |b, window, cx| {
        b.ask_delete(std::slice::from_ref(&file), window, cx)
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    assert!(!file.exists(), "Delete deletes");
    let _ = std::fs::remove_dir_all(&root);
}
