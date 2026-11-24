//! The Welcome window in the test harness (`SPEC-0355` AK-3): one window of 800 × 650 pt, the
//! recent projects with a missing one marked, search, opening and removing, the window going when
//! a project opens and coming back after the last project window, and quitting when it is closed
//! without a project. The test has its own recent list, settings and plugins folder; the user's
//! are never read or written. Nothing is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use ferris_idea_workbench::WelcomeView;
use ferris_idea_workbench::appearance::{IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::keymaps::UserKeymapFile;
use ferris_idea_workbench::project_windows::{
    OnLastProjectClosed, QuitsInHarness, RecentProjectsFile, opened_windows,
};
use ferris_idea_workbench::welcome_model::Section;
use ferris_idea_workbench::welcome_view::{WelcomePluginsDir, show, welcome_window};
use gpui::{AnyWindowHandle, TestAppContext, WindowHandle};

fn folder(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-welcome-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// This test's own files, set before anything reads them.
fn isolate(cx: &mut TestAppContext, dir: &std::path::Path) -> QuitsInHarness {
    let quits = QuitsInHarness::default();
    let q = quits.clone();
    let dir = dir.to_path_buf();
    cx.update(move |cx| {
        cx.set_global(RecentProjectsFile(dir.join("recent-projects")));
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(UserKeymapFile(dir.join("keymap.toml")));
        cx.set_global(WelcomePluginsDir(dir.join("plugins")));
        cx.set_global(q);
        cx.set_global(OnLastProjectClosed(std::rc::Rc::new(show)));
    });
    quits
}

fn welcome(cx: &mut TestAppContext) -> Option<WindowHandle<WelcomeView>> {
    cx.update(|cx| welcome_window(cx))
}

fn names(handle: WindowHandle<WelcomeView>, cx: &mut TestAppContext) -> Vec<String> {
    handle.update(cx, |view, _, _| view.shown_names()).unwrap()
}

/// SPEC-0355 AK-3 (behaviours 1 to 5, 7 to 11).
#[gpui::test]
fn the_welcome_window_lists_opens_removes_and_comes_back(cx: &mut TestAppContext) {
    let dir = folder("ak3");
    let quits = isolate(cx, &dir);
    let shop = dir.join("shop");
    std::fs::create_dir_all(shop.join(".git")).unwrap();
    std::fs::write(shop.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let gone = dir.join("gone");
    std::fs::write(
        dir.join("recent-projects"),
        format!("{}\n{}\n", shop.display(), gone.display()),
    )
    .unwrap();

    // One window of 800 × 650 pt; showing it again opens no second one.
    cx.update(show);
    cx.run_until_parked();
    let handle = welcome(cx).expect("the Welcome window is open");
    let size = handle
        .update(cx, |_, window, _| window.bounds().size)
        .unwrap();
    assert_eq!(
        (f32::from(size.width), f32::from(size.height)),
        (800.0, 650.0)
    );
    cx.update(show);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);

    // The list: the project with its branch, the missing one marked.
    assert_eq!(names(handle, cx), ["shop", "gone (missing)"]);
    let branch = handle
        .update(cx, |view, _, _| view.rows()[0].branch.clone())
        .unwrap();
    assert_eq!(branch.as_deref(), Some("main"));

    // Keys: ↓ selects the missing one, Enter only says it is gone.
    let any: AnyWindowHandle = handle.into();
    cx.simulate_keystrokes(any, "down enter");
    let (selected, note) = handle
        .update(cx, |view, _, _| {
            (view.selected(), view.note().map(str::to_owned))
        })
        .unwrap();
    assert_eq!(selected, 1);
    assert!(note.unwrap().contains("no longer exists"));
    assert_eq!(cx.update(|cx| cx.windows().len()), 1, "nothing opened");

    // ⌘⌫ removes it from the list file; the search filters by typing.
    cx.simulate_keystrokes(any, "cmd-backspace");
    assert_eq!(names(handle, cx), ["shop"]);
    assert_eq!(
        std::fs::read_to_string(dir.join("recent-projects")).unwrap(),
        format!("{}\n", shop.display())
    );
    cx.simulate_keystrokes(any, "z z");
    assert!(names(handle, cx).is_empty(), "no match");
    cx.simulate_keystrokes(any, "escape s h");
    assert_eq!(names(handle, cx), ["shop"]);

    // ⌘3 shows Customize (⌘2 is SSH since v3), ⌘1 Projects again.
    cx.simulate_keystrokes(any, "cmd-3");
    let section = handle.update(cx, |view, _, _| view.section()).unwrap();
    assert_eq!(section, Section::Customize);
    cx.simulate_keystrokes(any, "cmd-1");

    // Enter opens the project in a window of its own; the Welcome window goes.
    cx.simulate_keystrokes(any, "enter");
    cx.run_until_parked();
    assert!(welcome(cx).is_none(), "the Welcome window closed");
    let projects = cx.update(opened_windows);
    assert_eq!(projects.len(), 1, "one project window");

    // Closing the last project window shows the Welcome window again.
    cx.update(|cx| {
        ferris_idea_workbench::WorkbenchView::close_now(std::slice::from_ref(&shop), cx)
    });
    cx.run_until_parked();
    let handle = welcome(cx).expect("the Welcome window is back");
    assert_eq!(names(handle, cx), ["shop"]);

    // ⌘W without a project window quits (counted in the harness).
    assert_eq!(quits.0.get(), 0);
    cx.simulate_keystrokes(handle.into(), "cmd-w");
    cx.run_until_parked();
    assert!(welcome(cx).is_none());
    assert_eq!(quits.0.get(), 1, "closing it without a project quits");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0355 AK-3 (behaviour 7): New Project in the Welcome window makes the folder with Git and
/// opens it; its Location starts beside the most recent project.
#[gpui::test]
fn new_project_from_the_welcome_window(cx: &mut TestAppContext) {
    let dir = folder("new");
    isolate(cx, &dir);
    let old = dir.join("old");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(dir.join("recent-projects"), format!("{}\n", old.display())).unwrap();
    cx.update(show);
    cx.run_until_parked();
    let handle = welcome(cx).unwrap();
    cx.simulate_keystrokes(handle.into(), "cmd-n");
    let form = handle
        .update(cx, |view, _, _| view.form())
        .unwrap()
        .expect("the New Project form");
    // An error about the location goes when the location is edited (window check A16, CL1).
    let missing = dir.join("no-such-folder").display().to_string();
    handle
        .update(cx, |_, _, cx| {
            form.update(cx, |f, cx| {
                f.fill("fresh", &missing, true, cx);
                f.submit(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    let error = handle
        .update(cx, |_, _, cx| form.read(cx).error().map(str::to_owned))
        .unwrap();
    assert!(error.is_some(), "a location that does not exist is refused");
    handle
        .update(cx, |_, _, cx| {
            form.update(cx, |f, cx| {
                f.fill("fresh", &dir.display().to_string(), true, cx)
            })
        })
        .unwrap();
    cx.run_until_parked();
    let error = handle
        .update(cx, |_, _, cx| form.read(cx).error().map(str::to_owned))
        .unwrap();
    assert_eq!(error, None, "edited: the error is gone");
    handle
        .update(cx, |_, _, cx| {
            form.update(cx, |f, cx| {
                f.fill("fresh", &dir.display().to_string(), true, cx);
                f.submit(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    assert!(dir.join("fresh/.git").is_dir(), "made with git init");
    assert!(
        welcome(cx).is_none(),
        "the project opened, the Welcome window went"
    );
    assert_eq!(cx.update(opened_windows).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
