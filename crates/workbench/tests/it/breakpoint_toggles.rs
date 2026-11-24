//! F-071 without a screen (2026-11-16), in a window of the test harness: Toggle Breakpoint
//! Enabled, ⌥-click on a line number and Remove All Breakpoints (in File) (`SPEC-0127` v15
//! behaviours 31, 32); with a real debugpy a condition that cannot be evaluated is told in the
//! console (`SPEC-0039` v6) and the expression field's history goes back and forward with ↑ and
//! ↓ and stays for the next session (`SPEC-0091` v21 behaviour 46). The live test is skipped
//! without debugpy.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::{DebugState, EditorView, WorkbenchView};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-bp-toggles-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        std::fs::write(root.join(path), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    bench.read_with(cx, |b, _| {
        b.active_editor_entity().expect("the file is open")
    })
}

fn act(bench: &Bench, id: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

fn saved(root: &Path) -> String {
    std::fs::read_to_string(root.join(".ferris-idea/breakpoints.toml")).unwrap_or_default()
}

/// Whether the saved breakpoint on line 1 of `file` is switched off; `None` without one.
fn off(root: &Path, file: &Path) -> Option<bool> {
    let (lines, props) = ferris_idea_workbench::debug_model::load_all(root);
    lines.get(file)?.contains(&0).then(|| {
        props
            .get(file)
            .and_then(|m| m.get(&0))
            .is_some_and(|p| p.disabled)
    })
}

/// A click on the line number of the 0-based `line` (its line starts at char `offset`), with
/// `modifiers` held.
fn click_line_number(
    editor: &Entity<EditorView>,
    offset: usize,
    modifiers: Modifiers,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| window.draw(cx).clear());
    let (bounds, below) = editor.read_with(cx, |e, _| {
        (
            e.painted_bounds(),
            e.point_below(offset).expect("the line is drawn"),
        )
    });
    let at = gpui::point(bounds.origin.x + px(8.), below.y - px(4.));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, modifiers);
    cx.run_until_parked();
}

const LINES: &str = "a = 1\nb = 2\nc = 3\n";

/// `SPEC-0127` AK-22 (v15, behaviour 31): Toggle Breakpoint Enabled switches the caret line's
/// breakpoint off and on, keeping its condition, and does nothing on a line without one; ⌥-click
/// on a line number switches a breakpoint off and on, and on a free line sets a temporary one; a
/// plain click still removes.
#[gpui::test]
fn enabled_is_toggled_by_the_action_and_by_alt_click(cx: &mut TestAppContext) {
    let root = workspace("enabled", &[("main.py", LINES)]);
    let file = root.join("main.py");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &file);
    let lines = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.breakpoints().to_vec());

    // Without a breakpoint the action does nothing.
    act(&bench, "ToggleBreakpointEnabled", cx);
    assert!(lines(cx).is_empty());

    act(&bench, "ToggleLineBreakpoint", cx);
    bench.update(cx, |b, cx| {
        b.set_breakpoint_props(
            &file,
            0,
            ferris_idea_workbench::debug_model::BreakpointProps {
                condition: Some("a > 0".into()),
                ..Default::default()
            },
            cx,
        )
    });
    act(&bench, "ToggleBreakpointEnabled", cx);
    let text = saved(&root);
    assert_eq!(off(&root, &file), Some(true));
    assert!(text.contains("a > 0"), "{text}");
    assert_eq!(lines(cx), [0], "switched off, not removed");
    act(&bench, "ToggleBreakpointEnabled", cx);
    let text = saved(&root);
    assert_eq!(off(&root, &file), Some(false));
    assert!(text.contains("a > 0"), "{text}");

    // ⌥-click on the breakpoint of line 1: off, on again.
    click_line_number(&editor, 0, Modifiers::alt(), cx);
    assert_eq!(off(&root, &file), Some(true));
    click_line_number(&editor, 0, Modifiers::alt(), cx);
    assert_eq!(off(&root, &file), Some(false));
    assert_eq!(lines(cx), [0]);

    // ⌥-click on line 3 without one: a temporary breakpoint.
    let third = LINES.find("c =").unwrap();
    click_line_number(&editor, third, Modifiers::alt(), cx);
    assert_eq!(lines(cx), [0, 2]);
    assert!(
        saved(&root).contains("remove_once_hit = true"),
        "{}",
        saved(&root)
    );
    // A plain click removes it as before.
    click_line_number(&editor, third, Modifiers::none(), cx);
    assert_eq!(lines(cx), [0]);
    assert!(!saved(&root).contains("remove_once_hit"));
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0127` AK-22 (v15, behaviour 32): Remove All Breakpoints in File removes the active
/// file's breakpoints and keeps the other file's; Remove All Breakpoints removes the rest, also of
/// the file without an editor; the status bar counts them; both are found by Search Everywhere's
/// titles.
#[gpui::test]
fn all_breakpoints_are_removed_in_the_file_and_everywhere(cx: &mut TestAppContext) {
    let root = workspace("remove-all", &[("main.py", LINES), ("other.py", LINES)]);
    // other.py has a saved breakpoint and no editor.
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let other = root.join("other.py");
    let lines_of_other = std::collections::BTreeMap::from([(other.clone(), vec![1])]);
    ferris_idea_workbench::debug_model::save_all(&root, &lines_of_other, &Default::default());
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("main.py"));
    act(&bench, "ToggleLineBreakpoint", cx);
    editor.update(cx, |e, cx| e.toggle_breakpoint(2, cx));
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoints().to_vec()),
        [0, 2]
    );
    assert!(saved(&root).contains("other.py"), "{}", saved(&root));

    act(&bench, "Debugger.RemoveAllBreakpointsInFile", cx);
    assert!(editor.read_with(cx, |e, _| e.breakpoints().is_empty()));
    let text = saved(&root);
    assert!(
        text.contains("other.py") && !text.contains("main.py"),
        "{text}"
    );
    let note = bench.read_with(cx, |b, cx| b.status_message(cx));
    assert_eq!(note.as_deref(), Some("2 breakpoints removed in this file"));

    act(&bench, "Debugger.RemoveAllBreakpoints", cx);
    assert!(!saved(&root).contains("other.py"), "{}", saved(&root));
    let note = bench.read_with(cx, |b, cx| b.status_message(cx));
    assert_eq!(note.as_deref(), Some("1 breakpoint removed"));
    act(&bench, "Debugger.RemoveAllBreakpoints", cx);
    let note = bench.read_with(cx, |b, cx| b.status_message(cx));
    assert_eq!(note.as_deref(), Some("No breakpoints to remove"));

    for (id, title) in [
        ("Debugger.RemoveAllBreakpoints", "Remove All Breakpoints"),
        (
            "Debugger.RemoveAllBreakpointsInFile",
            "Remove All Breakpoints in File",
        ),
        ("ToggleBreakpointEnabled", "Toggle Breakpoint Enabled"),
    ] {
        assert_eq!(ferris_idea_languages::everywhere::action_title(id).0, title);
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------------------ with real debugpy

const LOOP_PY: &str = "def main():\n    total = 0\n    for i in range(4):\n        total += i\n    print('total', total)\n\n\nmain()\n";

fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn paused(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Paused(_)))
    })
}

/// `SPEC-0039` AK (v6) in the window and `SPEC-0091` AK-26 (v21, behaviour 46), with a real
/// debugpy: a condition naming an unknown variable stops at the breakpoint and the console says
/// it could not be evaluated; two expressions evaluated in the field come back with ↑ (newest
/// first) and ↓ (back to the empty field), and the history is still there in the next session.
#[gpui::test]
fn a_failed_condition_is_told_and_the_field_keeps_a_history(cx: &mut TestAppContext) {
    if !ferris_idea_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some())
    {
        eprintln!("skipped: debugpy is missing");
        return;
    }
    let root = workspace("live", &[("loop.py", LOOP_PY)]);
    ferris_idea_core::trust::trust_for_process(&root);
    let file = root.join("loop.py");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &file);
    editor.update(cx, |e, cx| e.toggle_breakpoint(3, cx));
    cx.run_until_parked();
    bench.update(cx, |b, cx| {
        b.set_breakpoint_props(
            &file,
            3,
            ferris_idea_workbench::debug_model::BreakpointProps {
                condition: Some("nosuch == 3".into()),
                ..Default::default()
            },
            cx,
        )
    });
    cx.simulate_keystrokes("ctrl-shift-d");
    until(cx, 120, "the program stops at the failed condition", |cx| {
        paused(&bench, cx)
    });
    let view = bench.read_with(cx, |b, _| b.debug_view().expect("the debug window"));
    until(cx, 30, "the console tells the failed condition", |cx| {
        view.read_with(cx, |v, _| {
            // debugpy's own report, as in `condition_errors.rs`.
            v.console().contains("nosuch == 3") && v.console().contains("NameError")
        })
    });

    // ⌥F8 puts the keyboard into the field; two expressions are evaluated with Enter.
    act(&bench, "EvaluateExpression", cx);
    for (expression, shown) in [("total", "total = 0"), ("i * 10", "i * 10 = 0")] {
        view.update(cx, |v, cx| v.set_expression(expression, cx));
        cx.simulate_keystrokes("enter");
        until(cx, 30, "the value is shown", |cx| {
            view.read_with(cx, |v, _| v.result_texts().iter().any(|r| r == shown))
        });
    }
    let field = |cx: &mut VisualTestContext| view.read_with(cx, |v, cx| v.expression_text(cx));
    view.update(cx, |v, cx| v.set_expression("", cx));
    for (key, text) in [
        ("up", "i * 10"),
        ("up", "total"),
        ("up", "total"),
        ("down", "i * 10"),
        ("down", ""),
    ] {
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        assert_eq!(field(cx), text, "after {key}");
    }
    assert_eq!(
        view.read_with(cx, |v, _| v.eval_history()),
        ["i * 10", "total"]
    );

    // The next session keeps the history.
    act(&bench, "Stop", cx);
    until(cx, 30, "the session ends", |cx| !paused(&bench, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("ctrl-shift-d");
    until(cx, 120, "the program stops again", |cx| paused(&bench, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.eval_history()),
        ["i * 10", "total"]
    );
    act(&bench, "Stop", cx);
    let _ = std::fs::remove_dir_all(&root);
}
