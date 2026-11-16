//! The project popup and closing projects with one and with two project windows in one process
//! (`SPEC-0094` v5 AK-14). Seen in the window on 2026-11-14: opening the popup read the window's
//! own workbench while it was being updated, and gpui ended the process.

#![cfg(target_os = "macos")]

use gpui::TestAppContext;
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::project_windows::{RecentProjectsFile, register_window};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-ppw-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

/// Opens a project window for each of `roots` and registers it as `open_project` does.
fn windows(
    roots: &[std::path::PathBuf],
    cx: &mut TestAppContext,
) -> Vec<(gpui::Entity<WorkbenchView>, gpui::AnyWindowHandle)> {
    let mut out = Vec::new();
    for root in roots {
        let shown = root.clone();
        let window = cx.add_window(move |_, cx| WorkbenchView::new(&shown, cx));
        cx.update(|cx| register_window(root, window.into(), cx));
        out.push((
            window.root(cx).unwrap(),
            gpui::AnyWindowHandle::from(window),
        ));
    }
    cx.run_until_parked();
    out
}

fn open_popup(
    bench: &gpui::Entity<WorkbenchView>,
    handle: gpui::AnyWindowHandle,
    cx: &mut TestAppContext,
) -> Vec<String> {
    handle
        .update(cx, |_, window, cx| {
            bench.update(cx, |b, cx| b.open_projects(window, cx))
        })
        .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        bench
            .read(cx)
            .projects_popup()
            .map(|v| v.read(cx).rows())
            .expect("the popup is open")
    })
}

/// SPEC-0094 v5 AK-14: one window with many recent projects — the popup opens and lists its
/// actions, this project as current and the recent ones. 24 others: with the open project the
/// list is at its limit of 25 (`RECENT_PROJECTS_LIMIT`), so none is dropped on any platform.
#[gpui::test]
fn the_popup_opens_in_a_single_window_with_many_recent_projects(cx: &mut TestAppContext) {
    let recent = std::env::temp_dir().join(format!("idecraft-ppw-recent1-{}", std::process::id()));
    let others: Vec<_> = (0..24).map(|i| workspace(&format!("r{i}"))).collect();
    let text: String = others
        .iter()
        .map(|p| format!("{}\n", p.display()))
        .collect();
    std::fs::write(&recent, text).unwrap();
    cx.update(|cx| cx.set_global(RecentProjectsFile(recent.clone())));
    let root = workspace("one");
    let w = windows(std::slice::from_ref(&root), cx);
    let rows = open_popup(&w[0].0, w[0].1, cx);
    assert_eq!(
        rows[..4],
        [
            "New Project…",
            "Open…",
            "Clone Repository…",
            "Remote Development…"
        ]
    );
    assert_eq!(
        rows[4],
        format!("{} (current)", root.file_name().unwrap().to_string_lossy())
    );
    assert_eq!(rows.len(), 5 + 24, "{rows:?}");
    for p in others.iter().chain([&root]) {
        let _ = std::fs::remove_dir_all(p);
    }
    let _ = std::fs::remove_file(&recent);
}

/// SPEC-0094 v5 AK-14: two windows in one process — the popup of each lists both open projects;
/// choosing the other one brings its window to the front; Close Other Projects, used in the
/// first window, closes the second without ending the process.
#[gpui::test]
fn the_popup_and_closing_work_with_two_windows(cx: &mut TestAppContext) {
    let recent = std::env::temp_dir().join(format!("idecraft-ppw-recent2-{}", std::process::id()));
    cx.update(|cx| cx.set_global(RecentProjectsFile(recent.clone())));
    let roots = [workspace("a"), workspace("b")];
    let w = windows(&roots, cx);
    let name = |i: usize| roots[i].file_name().unwrap().to_string_lossy().into_owned();
    let rows = open_popup(&w[0].0, w[0].1, cx);
    assert_eq!(
        rows[4..6],
        [format!("{} (current)", name(0)), name(1)],
        "{rows:?}"
    );
    let rows = open_popup(&w[1].0, w[1].1, cx);
    assert_eq!(
        rows[4..6],
        [name(0), format!("{} (current)", name(1))],
        "{rows:?}"
    );

    // Choosing the other open project: its window comes to the front.
    open_popup(&w[0].0, w[0].1, cx);
    w[0].1
        .update(cx, |_, _, cx| {
            let popup = w[0].0.read(cx).projects_popup().unwrap();
            popup.update(cx, |v, cx| v.choose(5, cx));
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        cx.update(|cx| cx.active_window()) == Some(w[1].1),
        "the other window is in front"
    );
    assert!(cx.read(|cx| w[0].0.read(cx).projects_popup().is_none()));

    // Close Other Projects in the first window: the second goes, the first stays.
    w[0].1
        .update(cx, |_, window, cx| {
            w[0].0
                .update(cx, |b, cx| b.menu_action("CloseOtherProjects", window, cx))
        })
        .unwrap();
    cx.run_until_parked();
    let left = cx.update(|cx| cx.windows());
    assert!(
        left.contains(&w[0].1) && !left.contains(&w[1].1),
        "{} windows left",
        left.len()
    );
    for p in &roots {
        let _ = std::fs::remove_dir_all(p);
    }
    let _ = std::fs::remove_file(&recent);
}
