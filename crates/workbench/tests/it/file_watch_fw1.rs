//! The two defects of window check OBS-0096 in a window of the test harness: the Project tree
//! follows files and folders another program creates, deletes or renames, within a second and
//! without Reload All from Disk, also for a project opened through a symbolic link (FW1,
//! `SPEC-0059` v9.11 behaviour 31); the tooltip of an editor tab goes when the pointer leaves the
//! tab or the window (TT1, `SPEC-0294` v6 behaviour 13).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferris_idea_ui::tooltip;
use ferris_idea_workbench::{WorkbenchView, hit_targets};
use gpui::{Entity, Modifiers, MouseExitEvent, TestAppContext, VisualTestContext, point, px};

type Bench = Entity<WorkbenchView>;

/// A project folder with `src/lib.rs` and `notes.txt`, and a symbolic link to it (as `/tmp`
/// stands for `/private/tmp`); returns (folder, link).
fn workspace(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("ferris-idea-fw1-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let real = base.join("proj");
    std::fs::create_dir_all(real.join("src")).unwrap();
    std::fs::write(real.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(real.join("notes.txt"), "one\n").unwrap();
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    (real, link)
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    hit_targets::enable();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

/// The visible rows of the project tree as paths from the project folder.
fn rows(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["path"].as_str().map(str::to_owned))
        .collect()
}

fn tree(bench: &Bench, cx: &mut VisualTestContext, action: &str, path: &str) {
    bench.update_in(cx, |b, _, cx| {
        let at = b.project_root().join(path);
        b.project_tree_call(&serde_json::json!({"action": action, "path": at}), cx)
            .unwrap_or_else(|e| panic!("{action} {path}: {e}"));
    });
    cx.run_until_parked();
}

/// Waits at most one second of the window's clock, in steps of 50 ms, until `done` holds.
fn within_a_second(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..=20 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("not within a second: {what}");
}

/// SPEC-0059 AK-20 (v9.11): created, renamed and deleted outside, in the project folder and in
/// new subfolders, the tree shows it within a second; expanded folders stay expanded.
#[gpui::test]
fn the_tree_follows_files_and_folders_changed_outside(cx: &mut TestAppContext) {
    let (real, link) = workspace("tree");
    let (bench, cx) = window(&link, cx);
    tree(&bench, cx, "expand", "src");
    let has = |cx: &mut VisualTestContext, path: &str| rows(&bench, cx).iter().any(|r| r == path);
    assert!(has(cx, "src/lib.rs"));

    // `printf … > README.md` in the project folder (OBS-0096 FW1).
    std::fs::write(real.join("README.md"), "# Lab\n").unwrap();
    within_a_second(cx, "README.md", |cx| has(cx, "README.md"));

    // A Java file in new subfolders: the new folder shows; once expanded, a file created in it.
    std::fs::create_dir_all(real.join("src/main/java/lab/app")).unwrap();
    within_a_second(cx, "src/main", |cx| has(cx, "src/main"));
    tree(&bench, cx, "expand", "src/main/java/lab/app");
    std::fs::write(
        real.join("src/main/java/lab/app/Main.java"),
        "class Main {}\n",
    )
    .unwrap();
    within_a_second(cx, "Main.java", |cx| {
        has(cx, "src/main/java/lab/app/Main.java")
    });

    // Renamed and deleted outside.
    std::fs::rename(real.join("notes.txt"), real.join("todo.txt")).unwrap();
    within_a_second(cx, "the rename", |cx| {
        has(cx, "todo.txt") && !has(cx, "notes.txt")
    });
    std::fs::remove_file(real.join("src/lib.rs")).unwrap();
    within_a_second(cx, "the deletion", |cx| !has(cx, "src/lib.rs"));
    // Still expanded where it was.
    assert!(has(cx, "src/main/java/lab/app/Main.java"));
    let _ = std::fs::remove_dir_all(real.parent().unwrap());
}

/// SPEC-0294 AK-13 (v6): the tooltip of the active editor tab goes when the pointer leaves the
/// tab, and when it leaves the window.
#[gpui::test]
fn the_tab_tooltip_goes_with_the_pointer(cx: &mut TestAppContext) {
    let (real, _) = workspace("tip");
    let (bench, cx) = window(&real, cx);
    let file = real.join("notes.txt");
    bench.update_in(cx, |b, window, cx| b.open(&file, window, cx));
    cx.run_until_parked();
    let tab = || {
        let r = hit_targets::get("tab-0").expect("the tab is drawn");
        point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
    };
    // Rests on the tab until its tooltip shows. Hiding by a key press is a flag of the whole
    // process, which the other window tests running beside this one set too; a tooltip they hid
    // is tried again after the pointer left the tab.
    let hover = |cx: &mut VisualTestContext| {
        for _ in 0..50 {
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::none());
            cx.simulate_mouse_move(tab(), None, Modifiers::none());
            // gpui shows a tooltip after the pointer rested half a second.
            cx.executor().advance_clock(Duration::from_millis(600));
            cx.run_until_parked();
            if let Some(shown) = tooltip::shown() {
                assert!(shown.text.contains("notes.txt"), "{}", shown.text);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the tab's tooltip is not shown");
    };

    let gone = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        tooltip::shown().is_none()
    };

    // Left the tab for the empty part of the tab row beside it, which draws nothing on hover.
    hover(cx);
    let r = hit_targets::get("tab-0").expect("the tab is drawn");
    let beside = point(px(r.x + r.w + 300.), px(r.y + r.h / 2.));
    cx.simulate_mouse_move(beside, None, Modifiers::none());
    assert!(gone(cx), "{:?}", tooltip::shown());
    // And for the editor below it.
    hover(cx);
    let r = hit_targets::get("editor-text").expect("the editor is drawn");
    let below = point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_mouse_move(below, None, Modifiers::none());
    assert!(gone(cx), "{:?}", tooltip::shown());

    // Left the window straight from the tab.
    hover(cx);
    cx.simulate_event(MouseExitEvent {
        position: tab(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    assert!(gone(cx), "{:?}", tooltip::shown());
    // The tab shows it again when the pointer comes back and rests.
    cx.simulate_mouse_move(below, None, Modifiers::none());
    cx.run_until_parked();
    hover(cx);
    let _ = std::fs::remove_dir_all(real.parent().unwrap());
}
