//! `SPEC-0006` v56 behaviour 11 (AK-11): the window's answer to the MCP tool `editor_selection`,
//! from a real workbench window of the harness with a selection made by keys.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use ferris_idea_workbench::WorkbenchView;
use gpui::{TestAppContext, VisualTestContext, px};

fn workspace() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-mcp-selection-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.py"), "a = 1\nname = \"shop\"\n").unwrap();
    root.canonicalize().unwrap()
}

#[gpui::test]
fn the_window_reports_the_active_editors_selection(cx: &mut TestAppContext) {
    let root = workspace();
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.update(|_, cx| {
        ferris_idea_workbench::project_windows::register_workbench(bench.downgrade(), cx)
    });
    cx.run_until_parked();
    let ask = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| WorkbenchView::editor_selection_in(&root, cx))
            .expect("the project window answers")
    };
    assert_eq!(ask(cx), None, "no editor yet");

    let file = root.join("src/app.py");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    // The caret alone: an empty selection at the start.
    let caret = ask(cx).expect("an editor is active");
    assert_eq!(
        (
            caret.start_line,
            caret.start_character,
            caret.end_line,
            caret.end_character
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(caret.text, "");
    // Selected by keys: from the second line's start over `name`.
    cx.simulate_keystrokes("down shift-right shift-right shift-right shift-right");
    cx.run_until_parked();
    let s = ask(cx).unwrap();
    assert!(s.path.ends_with("src/app.py"), "{:?}", s.path);
    assert_eq!(
        (s.start_line, s.start_character, s.end_line, s.end_character),
        (1, 0, 1, 4)
    );
    assert_eq!(s.text, "name");
    assert_eq!(s.carets, 1);
    // Another workspace has no window here.
    let other = cx.update(|_, cx| {
        WorkbenchView::editor_selection_in(&std::env::temp_dir().join("elsewhere"), cx)
    });
    assert!(other.is_err());
    let _ = std::fs::remove_dir_all(&root);
}
