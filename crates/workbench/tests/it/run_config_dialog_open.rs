//! Window check of #485 (D1): "Edit Configurations…" in the run widget's list opens the
//! configuration editor, drawn and holding the keyboard focus (`SPEC-0327` behaviour 1).

#![cfg(target_os = "macos")]

use ferris_idea_workbench::{RunChooserEvent, WorkbenchView};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-d1-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let mut text = String::new();
    for i in 0..8 {
        text.push_str(&format!(
            "[[run]]\nname = \"c{i}\"\ncommand = [\"echo\", \"{i}\"]\n\n"
        ));
    }
    std::fs::write(root.join(".ferris-idea/run.toml"), text).unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\"name\": \"x\", \"scripts\": {\"dev\": \"echo dev\", \"build\": \"echo b\"}}\n",
    )
    .unwrap();
    std::fs::write(root.join("main.js"), "console.log(1)\n").unwrap();
    root
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join("main.js");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    // As in the window check: 1372 × 860, a file open, the Run window shown.
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    // A temporary configuration made by Run Current File is the selected one, as in the check.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("RunClass", window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

/// The editor is open, drawn (its focus is in the frame's dispatch tree) and holds the focus.
fn editor_open_with_focus(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) {
    let dialog = bench
        .read_with(cx, |b, _| b.run_config_dialog())
        .expect("the editor is open");
    cx.run_until_parked();
    let (focused, available) = cx.update(|window, cx| {
        (
            dialog.read(cx).focus().contains_focused(window, cx),
            window.focused(cx).is_some(),
        )
    });
    assert!(available, "an element holds the keyboard focus");
    assert!(focused, "the editor holds the keyboard focus");
    // The menu items stay enabled: the focused element is in the drawn frame.
    let enabled = cx.update(|window, cx| {
        window.is_action_available(
            &ferris_idea_workbench::menu_bar::IdeAction {
                id: "editRunConfigurations".into(),
            },
            cx,
        )
    });
    assert!(enabled, "menu actions are available");
}

#[gpui::test]
fn the_edit_event_opens_the_editor(cx: &mut TestAppContext) {
    let root = workspace("event");
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| b.open_run_chooser_as(true, window, cx));
    cx.run_until_parked();
    let chooser = bench
        .read_with(cx, |b, _| b.run_chooser())
        .expect("the list is open");
    chooser.update(cx, |_, cx| cx.emit(RunChooserEvent::Edit(None)));
    cx.run_until_parked();
    editor_open_with_focus(&bench, cx);
    let _ = std::fs::remove_dir_all(&root);
}

/// A real click on the last row, "Edit Configurations…", of the run widget's list and of the
/// chooser.
#[gpui::test]
fn clicking_edit_configurations_opens_the_editor(cx: &mut TestAppContext) {
    ferris_idea_workbench::hit_targets::enable();
    let root = workspace("click");
    let (bench, cx) = open(&root, cx);
    let center = |name: &str| {
        let r = ferris_idea_workbench::hit_targets::get(name).expect("drawn");
        point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
    };
    for select_only in [true, false] {
        if select_only {
            // The run widget's list, opened by a click on the widget as by hand.
            cx.simulate_mouse_move(center("run-widget"), None, Modifiers::none());
            cx.simulate_click(center("run-widget"), Modifiers::none());
        } else {
            bench.update_in(cx, |b, window, cx| b.open_run_chooser(window, cx));
        }
        cx.run_until_parked();
        let rows = bench
            .read_with(cx, |b, cx| b.run_chooser().map(|c| c.read(cx).rows().len()))
            .expect("the list is open");
        // The pointer passes over the rows on its way down, as a hand does.
        for i in 0..rows {
            cx.simulate_mouse_move(center(&format!("run-choice-{i}")), None, Modifiers::none());
            cx.run_until_parked();
        }
        // Mouse-down and mouse-up as two events with everything settled in between, as a hand
        // clicks: the up must not reach a button of the editor drawn meanwhile.
        let at = center(&format!("run-choice-{}", rows - 1));
        cx.simulate_mouse_down(at, gpui::MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        cx.simulate_mouse_up(at, gpui::MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        // And a further click at the same place lands on the editor without closing it.
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        assert!(bench.read_with(cx, |b, _| b.run_chooser().is_none()));
        editor_open_with_focus(&bench, cx);
        // Close it again for the next round.
        bench.update_in(cx, |b, window, cx| {
            if let Some(d) = b.run_config_dialog() {
                d.update(cx, |d, cx| d.cancel(cx));
            }
            let _ = window;
        });
        cx.run_until_parked();
        assert!(bench.read_with(cx, |b, _| b.run_config_dialog().is_none()));
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0087 v7 AK-11: typing in the list filters its rows by name; Backspace edits the text,
/// Escape first clears it and then closes the list, Enter chooses the highlighted row, and a
/// digit picks its row only while nothing is typed.
#[gpui::test]
fn typing_filters_the_run_list(cx: &mut TestAppContext) {
    let root = workspace("filter");
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| b.open_run_chooser_as(true, window, cx));
    cx.run_until_parked();
    let state = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.run_chooser().map(|c| {
                let c = c.read(cx);
                let labels: Vec<String> = c.rows().iter().map(|r| r.label().to_owned()).collect();
                (c.filter().to_owned(), labels)
            })
        })
    };
    let (_, all) = state(cx).expect("the list is open");
    assert!(all.len() > 8, "{all:?}");
    // A digit is part of the name once a letter is typed: "c3" leaves that configuration alone.
    cx.simulate_keystrokes("c 3");
    cx.run_until_parked();
    assert_eq!(state(cx), Some(("c3".to_owned(), vec!["c3".to_owned()])));
    // Backspace widens the rows again: every name with a "c", the fixed rows included.
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    let (text, rows) = state(cx).expect("the list is open");
    assert_eq!(text, "c");
    for i in 0..8 {
        assert!(rows.contains(&format!("c{i}")), "{rows:?}");
    }
    assert!(rows.contains(&"Current File".to_owned()), "{rows:?}");
    assert!(
        rows.contains(&"Edit Configurations…".to_owned()),
        "{rows:?}"
    );
    // Text that matches nothing leaves no row, and Enter does nothing.
    cx.simulate_keystrokes("z z enter");
    cx.run_until_parked();
    assert_eq!(state(cx), Some(("czz".to_owned(), Vec::new())));
    // Escape clears the text and the list stays open with all its rows.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(state(cx), Some((String::new(), all)));
    // Enter chooses the first row the text leaves.
    cx.simulate_keystrokes("c 5 enter");
    cx.run_until_parked();
    assert!(state(cx).is_none(), "the list closed");
    assert_eq!(
        bench.read_with(cx, |b, _| b.selected_run_configuration()),
        Some("c5".to_owned())
    );
    // With nothing typed a digit picks its row at once.
    bench.update_in(cx, |b, window, cx| b.open_run_chooser_as(true, window, cx));
    cx.run_until_parked();
    let second = bench
        .read_with(cx, |b, cx| {
            b.run_chooser()
                .and_then(|c| c.read(cx).rows().get(1).map(|r| r.label().to_owned()))
        })
        .expect("a second row");
    cx.simulate_keystrokes("2");
    cx.run_until_parked();
    assert!(state(cx).is_none(), "the digit chose a row");
    assert_eq!(
        bench.read_with(cx, |b, _| b.selected_run_configuration()),
        Some(second)
    );
    // And Escape closes the list.
    bench.update_in(cx, |b, window, cx| b.open_run_chooser_as(true, window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(state(cx).is_none(), "Escape closed the list");
    let _ = std::fs::remove_dir_all(&root);
}
