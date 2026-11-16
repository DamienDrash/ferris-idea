//! Window checks of the icon gaps of OBS-0046 (`SPEC-0061` v17 AK-v17b–d): the Notifications tool
//! window lists the status bar's notes and clears them, the Build stripe button waits for a build,
//! and the toolbar's Run and Debug follow what the current file offers. Nothing is drawn on a
//! screen.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_core::notifications::Kind;
use idecraft_ui::tool_windows::{Side, ToolWindow};
use idecraft_workbench::WorkbenchView;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-f086-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
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
    cx.run_until_parked();
    (bench, cx)
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
    for _ in 0..200 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}

/// Shows `note` in the status bar and lets the window draw it.
fn note(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, note: &str, passing: bool) {
    bench.update(cx, |b, cx| {
        if passing {
            b.hint_status(note.into(), cx);
        } else {
            b.hold_status(note.into(), cx);
        }
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

fn listed(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<(String, Kind, u32)> {
    bench.read_with(cx, |b, _| {
        b.notifications()
            .into_iter()
            .map(|n| (n.text, n.kind, n.count))
            .collect()
    })
}

/// SPEC-0061 AK-v17b: the window on the right stripe lists the status bar's notes newest first
/// with their repeats, leaves passing hints out, and Clear All empties it.
#[gpui::test]
fn a_status_note_is_listed_and_cleared(cx: &mut TestAppContext) {
    let root = workspace("notes", &[("notes.txt", "hello\n")]);
    let (bench, cx) = window(&root, cx);
    let right = bench.read_with(cx, |b, _| b.stripe_windows(Side::Right));
    assert_eq!(right.first(), Some(&ToolWindow::Notifications), "{right:?}");

    // Shown, gone (an empty note is not listed) and shown again: one row with two showings.
    note(&bench, cx, "Saved", false);
    note(&bench, cx, "", false);
    note(&bench, cx, "Saved", false);
    note(&bench, cx, "Push failed: rejected", false);
    note(&bench, cx, "Press ⌥⏎ for the fixes", true);
    assert_eq!(
        listed(&bench, cx),
        [
            ("Push failed: rejected".to_owned(), Kind::Error, 1),
            ("Saved".to_owned(), Kind::Info, 2),
        ]
    );

    action(&bench, cx, "ActivateNotificationsToolWindow");
    assert!(bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Notifications)));
    bench.update(cx, |b, cx| b.clear_notifications(cx));
    assert!(listed(&bench, cx).is_empty());
    action(&bench, cx, "ActivateNotificationsToolWindow");
    assert!(!bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Notifications)));
}

/// SPEC-0061 AK-v17c: no Build button until the project has a Maven or Gradle file or a build
/// ran.
#[gpui::test]
fn the_build_button_waits_for_a_build(cx: &mut TestAppContext) {
    let root = workspace(
        "nobuild",
        &[("go.mod", "module app\n"), ("main.go", "package main\n")],
    );
    let (bench, cx) = window(&root, cx);
    let has_build = |bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext| -> bool {
        bench.read_with(cx, |b, _| {
            b.stripe_windows(Side::Right).contains(&ToolWindow::Build)
        })
    };
    for _ in 0..30 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
    }
    assert!(!has_build(&bench, cx));
    action(&bench, cx, "CompileDirty");
    assert!(has_build(&bench, cx));
}

/// SPEC-0061 AK-v17c: a project with a Maven build file has the Build button from the start.
#[gpui::test]
fn a_maven_project_has_the_build_button(cx: &mut TestAppContext) {
    let root = workspace(
        "maven",
        &[(
            "pom.xml",
            "<project><modelVersion>4.0.0</modelVersion><groupId>a</groupId>\
             <artifactId>b</artifactId><version>1</version></project>\n",
        )],
    );
    let (bench, cx) = window(&root, cx);
    wait(cx, "the build file is read", |cx| {
        bench.read_with(cx, |b, _| {
            b.stripe_windows(Side::Right).contains(&ToolWindow::Build)
        })
    });
}

/// SPEC-0061 AK-v17d: with "Current File", Run and Debug act only for a file that offers
/// something to run; a chosen configuration keeps them active.
#[gpui::test]
fn run_and_debug_follow_the_current_file(cx: &mut TestAppContext) {
    let root = workspace(
        "run",
        &[("notes.txt", "hello\n"), ("app.py", "print('hi')\n")],
    );
    let (bench, cx) = window(&root, cx);
    let enabled = |bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| b.run_buttons_enabled(cx))
    };
    assert_eq!(
        bench.read_with(cx, |b, _| b.selected_run_configuration()),
        None
    );
    assert!(!enabled(&bench, cx), "no editor");
    let open = |bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, name: &str| {
        let file = root.join(name);
        bench.update_in(cx, |b, window, cx| b.open(&file, window, cx));
        cx.run_until_parked();
    };
    open(&bench, cx, "notes.txt");
    assert!(!enabled(&bench, cx), "a text file");
    open(&bench, cx, "app.py");
    assert!(enabled(&bench, cx), "a Python file");
    open(&bench, cx, "notes.txt");
    assert!(!enabled(&bench, cx));
    // A configuration written later and chosen in the run widget.
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    std::fs::write(
        root.join(".idecraft/run.toml"),
        "[[run]]\nname = \"api\"\ncommand = [\"echo\", \"api\"]\n",
    )
    .unwrap();
    bench.update(cx, |b, cx| b.select_run_configuration("api", cx));
    assert_eq!(
        bench.read_with(cx, |b, _| b.selected_run_configuration()),
        Some("api".to_owned())
    );
    assert!(enabled(&bench, cx), "a chosen configuration");
}
