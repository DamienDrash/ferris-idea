//! Clicks inside a popup stay in the popup (`SPEC-0275` v14 behaviour 28b), checked in a window
//! of the test harness with Quick Switch Scheme over an open editor. Choosing a theme changes
//! the theme of the whole process and writes the theme log once per process, so this test has a
//! process of its own, apart from `theme_switch.rs` and the other window tests.

#![cfg(target_os = "macos")]

use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::{IdeSettingsFile, THEME_LOG, ThemeFile, ThemeLog};
use idecraft_workbench::palette::{ThemeId, current_theme, set_current_theme};

/// SPEC-0275 AK-33 (v14, behaviour 28b): a click on a row of the Quick Switch Scheme popup
/// chooses the theme and does not reach the editor under the popup; a click on the popup beside
/// its rows does nothing at all. Seen in the window on 2026-11-14: the caret went to the text
/// under the clicked row.
#[gpui::test]
fn a_click_in_the_theme_popup_does_not_reach_the_editor(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("idecraft-popup-clicks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    // Text under every place of the popup, so a press that reached the editor moves the caret.
    let line = "word ".repeat(60);
    let text = format!("{line}\n").repeat(80);
    std::fs::write(root.join("long.txt"), &text).unwrap();
    let root = root.canonicalize().unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(ThemeLog(dir.join(THEME_LOG)));
    });
    let first = ThemeId::ALL[0];
    set_current_theme(first);
    let file = root.join("long.txt");
    let window = {
        let root = root.clone();
        cx.add_window(move |_, cx| WorkbenchView::new(&root, cx))
    };
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let width = 1372.;
    cx.simulate_resize(gpui::size(px(width), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open");
    let caret = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.caret());
    let open = |cx: &mut VisualTestContext| bench.read_with(cx, |b, _| b.themes_popup().is_some());

    // The press does reach the editor where no popup lies: the check below can fail.
    cx.simulate_click(point(px(width / 2.), px(400.)), Modifiers::none());
    cx.run_until_parked();
    assert_ne!(caret(cx), 0, "a click on the text places the caret");

    // The popup is drawn in the middle below the toolbar; its place is not asked for here, the
    // column through its middle is clicked from top to bottom instead.
    let (mut chose, mut beside) = (0, 0);
    for y in (40..520).step_by(7) {
        set_current_theme(first);
        bench.update_in(cx, |b, window, cx| {
            if b.themes_popup().is_none() {
                b.menu_action("QuickChangeScheme", window, cx);
            }
        });
        cx.run_until_parked();
        assert!(open(cx));
        let before = caret(cx);
        cx.simulate_click(point(px(width / 2.), px(y as f32)), Modifiers::none());
        cx.run_until_parked();
        let chosen = current_theme() != first;
        if chosen {
            // On a row (of another theme than the current one).
            assert!(!open(cx), "y {y}: choosing closes the popup");
            assert_eq!(caret(cx), before, "y {y}: the click chose a theme only");
            chose += 1;
        } else if open(cx) {
            // On the popup beside its rows.
            assert_eq!(caret(cx), before, "y {y}: the click stayed in the popup");
            beside += 1;
        }
        // Else outside the popup (it closed without a choice and the click went on, behaviour
        // 28), or on the row of the current theme.
    }
    assert!(
        chose >= 3,
        "{chose} clicks chose a theme, {beside} fell beside the rows"
    );
    set_current_theme(ThemeId::DEFAULT);
    let _ = std::fs::remove_dir_all(&dir);
}
