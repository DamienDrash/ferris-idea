//! The speed search of the Database tool window and the filter of the Spring tool window take the
//! text the platform's text input commits, not the key behind it (`SPEC-0004` v7 behaviour 17a,
//! AK-12): synthetic input with a Unicode string arrives on macOS with key code 0, which is `a`.
//! Both views stand alone in a window of the test harness; no connection is opened.

#![cfg(target_os = "macos")]

use gpui::{Entity, EntityInputHandler, Modifiers, Render, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{DatabaseView, SpringView};

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

/// SPEC-0004 v7 AK-12: a committed letter opens the Database speed search, more extend it (also
/// `ä`); real keys with Shift go on; ⌫ and Escape keep their meaning.
#[gpui::test]
fn committed_text_opens_and_extends_the_database_search(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("idecraft-db-search-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let at = root.clone();
    let window = cx.add_window(move |_, cx| DatabaseView::new(&at, cx));
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(400.), px(600.)));
    view.update_in(cx, |v, window, _| window.focus(&v.focus()));
    cx.run_until_parked();
    let search =
        |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.speed_search().map(str::to_owned));
    assert_eq!(search(cx), None);
    for ch in ["o", "r", "d"] {
        key_a_then_commit(&view, cx, ch);
    }
    assert_eq!(search(cx).as_deref(), Some("ord"));
    key_a_then_commit(&view, cx, "ä");
    assert_eq!(search(cx).as_deref(), Some("ordä"));
    keys(cx, "backspace");
    assert_eq!(search(cx).as_deref(), Some("ord"));
    // Real keys, one by one (`simulate_input` also sends empty keys, which close the search as
    // any other key does).
    keys(cx, "shift-e r");
    assert_eq!(search(cx).as_deref(), Some("ordEr"));
    keys(cx, "escape");
    assert_eq!(search(cx), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0004 v7 AK-12: the Spring window's filter takes committed text; real keys with Shift go
/// on; ⌫ and Escape keep their meaning.
#[gpui::test]
fn committed_text_filters_the_spring_window(cx: &mut TestAppContext) {
    let window = cx.add_window(|_, cx| SpringView::new(cx));
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(400.), px(600.)));
    view.update_in(cx, |v, window, _| window.focus(&v.focus()));
    cx.run_until_parked();
    let text = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.filter().text.clone());
    for ch in ["S", "h", "o", "p"] {
        key_a_then_commit(&view, cx, ch);
    }
    assert_eq!(text(cx), "Shop");
    keys(cx, "shift-s ü");
    assert_eq!(text(cx), "ShopSü");
    keys(cx, "backspace");
    assert_eq!(text(cx), "ShopS");
    keys(cx, "escape");
    assert_eq!(text(cx), "");
}
