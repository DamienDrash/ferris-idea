//! A path to a file opens the file's project and the file in an editor (`SPEC-0094` v7 AK-17).
//! Seen in the window check of 2026-11-15: `idecraft ui <folder>/queries/file.sql` opened a window
//! whose project root was the file, shown in the Project tree as a folder, and no editor.

#![cfg(target_os = "macos")]

use gpui::{Focusable, TestAppContext};
use idecraft_workbench::project_windows::{RecentProjectsFile, open_project, workbenches};

/// A Git repository with `queries/file.sql` and `queries/other.sql`.
fn repository() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-ofp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("queries")).unwrap();
    std::fs::write(root.join("queries/file.sql"), "select 1;\n").unwrap();
    std::fs::write(root.join("queries/other.sql"), "select 2;\n").unwrap();
    root.canonicalize().unwrap()
}

/// The project root, the active tab and whether its editor has the focus, of the only workbench.
fn shown(cx: &mut TestAppContext) -> (std::path::PathBuf, Option<std::path::PathBuf>, bool) {
    let benches = cx.update(|cx| workbenches(cx));
    assert_eq!(benches.len(), 1, "one project window");
    let bench = benches[0].clone();
    let handle = cx.update(|cx| cx.windows()[0]);
    handle
        .update(cx, |_, window, cx| {
            let b = bench.read(cx);
            let focused = b
                .active_editor_entity()
                .is_some_and(|e| e.read(cx).focus_handle(cx).is_focused(window));
            (b.project_root().to_path_buf(), b.active_file(), focused)
        })
        .unwrap()
}

/// SPEC-0094 v7 AK-17 (behaviours 18 and 19): a file opens one window on its repository with the
/// file in the active, focused tab; a second file of the project opens in that window.
#[gpui::test]
fn a_file_opens_its_project_and_itself(cx: &mut TestAppContext) {
    let recent = std::env::temp_dir().join(format!("idecraft-ofp-recent-{}", std::process::id()));
    cx.update(|cx| cx.set_global(RecentProjectsFile(recent.clone())));
    let root = repository();
    let file = root.join("queries/file.sql");

    let opened = cx.update(|cx| open_project(&file, cx)).unwrap();
    assert!(opened, "a new window");
    cx.run_until_parked();
    let (project, active, focused) = shown(cx);
    assert_eq!(project, root, "the repository is the project, not the file");
    assert_eq!(active, Some(file.clone()));
    assert!(focused, "the file's editor has the focus");
    // The recent list holds the project folder, never the file.
    let listed = std::fs::read_to_string(&recent).unwrap();
    assert_eq!(
        listed.lines().next(),
        Some(root.display().to_string().as_str())
    );

    let other = root.join("queries/other.sql");
    let opened = cx.update(|cx| open_project(&other, cx)).unwrap();
    assert!(!opened, "no second window");
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    let (project, active, _) = shown(cx);
    assert_eq!(project, root);
    assert_eq!(active, Some(other));

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&recent);
}
