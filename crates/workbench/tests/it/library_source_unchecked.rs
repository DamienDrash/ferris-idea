//! `SPEC-0149` v5 AK-10 (behaviour 11, beta finding B2 of hand test 209): a library source is read,
//! not checked. An editor that is not checked shows none of Ferris IDEA's own findings, keeps none
//! from later diagnostics, and the Problems window's File tab lists nothing for it. No language
//! server takes part; the library-source path itself is checked in hand test 209 of
//! `workflow_a_beta_handtests.rs`.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

/// A bare `except:` is one of Ferris IDEA's own findings in Python (`SPEC-0098`).
const SAFE: &str = "def safe():\n    try:\n        return 1\n    except:\n        return 0\n";

fn workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-unchecked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("safe.py"), SAFE).unwrap();
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
    (bench, cx)
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn problems(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |e, _| e.problems().len())
}

#[gpui::test]
fn a_library_source_shows_no_problems(cx: &mut TestAppContext) {
    let root = workspace();
    let (bench, cx) = window(&root, cx);
    let file = root.join("safe.py");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    until(cx, "the bare except is marked", |cx| {
        problems(&editor, cx) > 0
    });
    let listed = bench.read_with(cx, |b, cx| b.problems_list(cx).len());
    assert!(
        listed > 0,
        "the File tab lists the finding of a checked file"
    );

    // Not checked, as a library source is: nothing in the editor, nothing in the File tab.
    editor.update(cx, |e, _| e.set_unchecked(true));
    cx.run_until_parked();
    assert_eq!(problems(&editor, cx), 0, "no own findings");
    assert_eq!(bench.read_with(cx, |b, cx| b.problems_list(cx).len()), 0);
    // An edit runs the inspections again: still nothing.
    editor.update(cx, |e, cx| e.replace_ranges(&[(0..0, "\n".into())], cx));
    for _ in 0..20 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    assert_eq!(problems(&editor, cx), 0, "nothing after the text changed");
    let _ = std::fs::remove_dir_all(&root);
}
