//! Typed filters outside editors take the text the platform's text input commits, not the key
//! behind it (`SPEC-0004` v8 behaviour 17b, AK-13): on macOS synthetic input with a Unicode string
//! arrives with key code 0, which is `a`. Each view stands alone in a window of the test harness:
//! the Git log, the Structure popup, the intentions list, the tab list, the profiler's process
//! list and the merge window.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::{
    GitLogView, IntentionsView, MergeView, ProfilerView, StructureView, TabListView,
};
use gpui::{
    Entity, EntityInputHandler, FocusHandle, Modifiers, Render, TestAppContext, VisualTestContext,
    px,
};

/// A window holding the view `build` makes, focused through `focus`.
fn window<V: Render + 'static>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut gpui::Context<V>) -> V + 'static,
    focus: impl Fn(&V) -> FocusHandle,
) -> (Entity<V>, &'static mut VisualTestContext) {
    let window = cx.add_window(move |_, cx| build(cx));
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(800.), px(600.)));
    let handle = view.read_with(cx, |v, _| focus(v));
    cx.update(|window, _| window.focus(&handle));
    cx.run_until_parked();
    (view, cx)
}

/// A key event of the key `a` (key code 0 on macOS), then the text the text input commits.
fn key_a_then_commit<V: EntityInputHandler + Render>(
    view: &Entity<V>,
    cx: &mut VisualTestContext,
    committed: &str,
) {
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke {
            modifiers: Modifiers::none(),
            key: "a".into(),
            key_char: Some("a".into()),
        },
        is_held: false,
    });
    view.update_in(cx, |v, window, cx| {
        v.replace_text_in_range(None, committed, window, cx)
    });
    cx.run_until_parked();
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn temp(tag: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-filters-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// SPEC-0004 v8 AK-13: the Git log's commit filter and the branches pane's filter.
#[gpui::test]
fn git_log_filters_take_committed_text(cx: &mut TestAppContext) {
    let root = temp("log");
    let at = root.clone();
    let (view, cx) = window(cx, move |cx| GitLogView::new(&at, cx), GitLogView::focus);
    let filter = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.filter().to_owned());
    for ch in ["f", "i", "x"] {
        key_a_then_commit(&view, cx, ch);
    }
    key_a_then_commit(&view, cx, "ü");
    assert_eq!(filter(cx), "fixü");
    keys(cx, "shift-q");
    assert_eq!(filter(cx), "fixüQ");
    keys(cx, "backspace");
    assert_eq!(filter(cx), "fixü");
    keys(cx, "escape");
    assert_eq!(filter(cx), "", "Escape clears the filter");
    // Tab goes to the branches pane: its own filter takes the text.
    keys(cx, "tab");
    let tree =
        |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.branch_tree_filter().to_owned());
    key_a_then_commit(&view, cx, "m");
    key_a_then_commit(&view, cx, "ä");
    assert_eq!(tree(cx), "mä");
    assert_eq!(filter(cx), "", "the commit filter stays");
    keys(cx, "backspace");
    assert_eq!(tree(cx), "m");
    keys(cx, "escape");
    assert_eq!(tree(cx), "");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0004 v8 AK-13: the Structure popup's speed search.
#[gpui::test]
fn structure_takes_committed_text(cx: &mut TestAppContext) {
    let (view, cx) = window(
        cx,
        |cx| StructureView::new("Shop.java", Vec::new(), (0, 0), cx),
        StructureView::focus,
    );
    let query = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.query().to_owned());
    key_a_then_commit(&view, cx, "g");
    key_a_then_commit(&view, cx, "ü");
    keys(cx, "shift-t");
    assert_eq!(query(cx), "güT");
    keys(cx, "backspace");
    assert_eq!(query(cx), "gü");
}

/// SPEC-0004 v8 AK-13: the speed search of the intentions list.
#[gpui::test]
fn intentions_take_committed_text(cx: &mut TestAppContext) {
    let (view, cx) = window(
        cx,
        |cx| IntentionsView::new(Vec::new(), cx),
        IntentionsView::focus,
    );
    let query = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.query().to_owned());
    key_a_then_commit(&view, cx, "i");
    key_a_then_commit(&view, cx, "ä");
    keys(cx, "shift-n");
    assert_eq!(query(cx), "iäN");
    keys(cx, "backspace");
    assert_eq!(query(cx), "iä");
}

/// SPEC-0004 v8 AK-13: the filter of Recent Files and the tab list.
#[gpui::test]
fn tab_list_takes_committed_text(cx: &mut TestAppContext) {
    let (view, cx) = window(
        cx,
        |cx| TabListView::new("Recent Files".into(), Vec::new(), None, false, cx),
        TabListView::focus,
    );
    let text = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.filter_text().to_owned());
    key_a_then_commit(&view, cx, "c");
    key_a_then_commit(&view, cx, "ü");
    keys(cx, "shift-r");
    assert_eq!(text(cx), "cüR");
    keys(cx, "backspace");
    assert_eq!(text(cx), "cü");
}

/// SPEC-0004 v8 AK-13: the profiler's process list; Escape closes it.
#[gpui::test]
fn profiler_processes_take_committed_text(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, ProfilerView::new, ProfilerView::focus);
    view.update(cx, |v, cx| {
        v.show_processes(
            vec![ferris_idea_profiler::ProcessInfo {
                pid: 41,
                command: "java -jar shop.jar".into(),
                refused: None,
            }],
            cx,
        )
    });
    cx.run_until_parked();
    let filter = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| v.picker_filter().map(str::to_owned))
    };
    assert_eq!(filter(cx).as_deref(), Some(""));
    key_a_then_commit(&view, cx, "sh");
    key_a_then_commit(&view, cx, "ö");
    keys(cx, "shift-p");
    assert_eq!(filter(cx).as_deref(), Some("shöP"));
    keys(cx, "backspace");
    assert_eq!(filter(cx).as_deref(), Some("shö"));
    keys(cx, "escape");
    assert_eq!(filter(cx), None, "Escape closes the process list");
}

/// SPEC-0004 v8 AK-13: the merge window's result column takes committed text.
#[gpui::test]
fn merge_result_takes_committed_text(cx: &mut TestAppContext) {
    let root = temp("merge");
    std::fs::write(root.join("a.txt"), "x\n").unwrap();
    let conflict = ferris_idea_vcs::Conflict {
        path: "a.txt".into(),
        base: Some("x\n".into()),
        ours: Some("x1\n".into()),
        theirs: Some("x2\n".into()),
        has_base: true,
        has_ours: true,
        has_theirs: true,
        binary: false,
    };
    let at = root.clone();
    let (view, cx) = window(
        cx,
        move |cx| MergeView::new(&at, vec![conflict], cx),
        MergeView::focus,
    );
    let result = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.result_text());
    let before = result(cx);
    key_a_then_commit(&view, cx, "ü");
    keys(cx, "shift-z");
    let typed = result(cx);
    assert!(typed.contains("üZ"), "{before:?} → {typed:?}");
    assert!(
        !typed.contains("aa"),
        "the key behind the text is not typed: {typed:?}"
    );
    keys(cx, "backspace");
    let back = result(cx);
    assert!(back.contains('ü') && !back.contains("üZ"), "{back:?}");
    let _ = std::fs::remove_dir_all(&root);
}
