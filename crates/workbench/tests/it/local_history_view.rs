//! `SPEC-0085` v2 AK-5 in a window of the test harness, by keys and by clicks on the controls
//! found through the semantic tree (`SPEC-0319`): the local history is titled with the file, an
//! entry shows the state before its change, typed text narrows the list, Create Patch puts the
//! patch on the clipboard, Next Difference steps, Revert puts the state before back.

#![cfg(target_os = "macos")]

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_workbench::{HistoryView, WorkbenchView, semantics};

fn workspace() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-local-history-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    // The workbench compares paths below its canonical root (`/private/var/…` on macOS).
    let root = root.canonicalize().unwrap();
    // Three saved states, an hour apart; the last is the current text.
    let mut history = idecraft_core::LocalHistory::open(&root).unwrap();
    let file = root.join("src/Inventory.java");
    let states = [
        "int a;\nint q;\nint b;\n",
        "int a;\nint b;\n",
        "int a;\nint b;\nint c;\n",
    ];
    for (k, text) in states.iter().enumerate() {
        std::fs::write(&file, text).unwrap();
        let time = 1_763_200_000_000 + k as u64 * 3_600_000;
        assert!(history.record_at("src/Inventory.java", time).unwrap());
    }
    root
}

fn node_centre(id: &str, cx: &mut VisualTestContext) -> Option<gpui::Point<gpui::Pixels>> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let node = tree.iter().find(|n| n.test_id() == Some(id))?;
        let r = node.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
        Some(point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)))
    })
}

fn has(id: &str, cx: &mut VisualTestContext) -> bool {
    node_centre(id, cx).is_some()
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let at = node_centre(id, cx).unwrap_or_else(|| panic!("no control {id} in view"));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn view(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<HistoryView> {
    bench
        .read_with(cx, |b, _| b.history_view())
        .expect("the local history is open")
}

fn editor_text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .expect("the file is open")
            .read(cx)
            .state
            .buffer
            .text()
    })
}

/// AK-5: title, entries with the state before them, search, patch, stepping, Revert.
#[gpui::test]
fn history_shows_changes_before_and_searches(cx: &mut TestAppContext) {
    let root = workspace();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let file = root.join("src/Inventory.java");
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.show_history(window, cx);
    });
    cx.run_until_parked();
    let history = view(&bench, cx);

    // H1: titled with the file.
    assert_eq!(
        history.read_with(cx, |v, _| v.heading()),
        "Local History: Inventory.java"
    );
    assert!(has("history-title", cx));
    // H2: each row names the change and its date and time, not its age.
    let rows = history.read_with(cx, |v, _| v.rows());
    assert_eq!(rows.len(), 3, "{rows:?}");
    for row in &rows {
        let (what, when) = row.split_once(" · ").expect("kind · time");
        assert_eq!(what, "Change", "{row}");
        assert!(
            when.len() == 16 && when.as_bytes()[4] == b'-' && when.as_bytes()[13] == b':',
            "a date with the time: {row}"
        );
    }
    // H3: the newest entry shows the last change: the state before it against the current text.
    let diff = history.read_with(cx, |v, _| v.diff());
    assert_eq!(diff.read_with(cx, |d, _| d.differences()), 1);
    let (left, right) = diff.read_with(cx, |d, _| {
        let (l, r) = d.side_titles();
        (l.to_owned(), r.to_owned())
    });
    assert!(left.starts_with("Before "), "{left}");
    assert_eq!(right, "Current");
    // H5: both sides are marked in this view.
    assert!(diff.read_with(cx, |d, _| d.filler_marks()));

    // H4: the search narrows the list to the entries whose state before contains the text.
    keys(cx, "q");
    assert_eq!(history.read_with(cx, |v, _| v.query().to_owned()), "q");
    assert!(has("history-search", cx));
    assert!(
        !has("history-row-0", cx),
        "before the newest entry there was no `q`"
    );
    assert!(has("history-row-1", cx) && has("history-row-2", cx));
    assert_eq!(
        history.read_with(cx, |v, _| v.selected()),
        1,
        "the first one shown"
    );
    keys(cx, "x");
    assert!(has("history-none", cx), "no entry contains `qx`");
    keys(cx, "backspace");
    assert!(has("history-row-1", cx));
    keys(cx, "escape");
    assert_eq!(history.read_with(cx, |v, _| v.query().to_owned()), "");
    assert!(has("history-row-0", cx), "Escape clears the search first");
    assert!(
        bench.read_with(cx, |b, _| b.history_view()).is_some(),
        "and keeps the view open"
    );

    // H4: Create Patch puts a unified patch from the state before to the current text on the
    // clipboard.
    click("history-patch", cx);
    let patch = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("a patch on the clipboard");
    assert!(
        patch.starts_with("--- a/src/Inventory.java\n+++ b/src/Inventory.java\n@@"),
        "{patch}"
    );
    assert!(
        patch.contains("-int q;\n") && patch.contains("+int c;\n"),
        "{patch}"
    );
    assert_eq!(
        bench.read_with(cx, |b, _| b.status_note().map(str::to_owned)),
        Some("Patch copied to the clipboard".to_owned())
    );

    // H4: Previous/Next Difference are buttons of the comparison's header.
    let diff = history.read_with(cx, |v, _| v.diff());
    assert_eq!(diff.read_with(cx, |d, _| d.differences()), 2);
    assert!(has("diff-previous", cx) && has("diff-next", cx) && has("diff-count", cx));
    assert_eq!(diff.read_with(cx, |d, _| d.current()), Some(0));
    click("diff-next", cx);
    assert_eq!(diff.read_with(cx, |d, _| d.current()), Some(1));
    click("diff-previous", cx);
    assert_eq!(diff.read_with(cx, |d, _| d.current()), Some(0));

    // The oldest entry, chosen by a click, shows its own state.
    click("history-row-2", cx);
    assert_eq!(history.read_with(cx, |v, _| v.selected()), 2);
    let diff = history.read_with(cx, |v, _| v.diff());
    assert_eq!(diff.read_with(cx, |d, _| d.differences()), 2);

    // Revert puts back the state before the chosen change, as one undo step.
    click("history-row-0", cx);
    click("history-revert", cx);
    assert_eq!(editor_text(&bench, cx), "int a;\nint b;\n");
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().expect("the file is open");
        editor.update(cx, |e, cx| e.run_action("$Undo", cx));
    });
    cx.run_until_parked();
    assert_eq!(editor_text(&bench, cx), "int a;\nint b;\nint c;\n");
    let _ = std::fs::remove_dir_all(&root);
}
