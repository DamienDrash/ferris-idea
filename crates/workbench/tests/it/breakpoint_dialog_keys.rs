//! The rest of hand test 54 (`SPEC-0127` v9 AK-14) in a window of the test harness with real keys
//! and real debugpy: the condition typed in the Breakpoints dialog, a line typed above the
//! breakpoints, the stop on the pass where the condition holds, and a failed condition told in
//! the debug console (`SPEC-0039` v6 behaviour 3a). Skipped without debugpy.
//!
//! The agent run of 2026-11-13 (PARITY.md, F-071) checked the dialog's list, the stop with
//! `i == 3`, the log message and the switched-off breakpoint in the window; it left out the
//! inserted line and the failed condition. What needs eyes stays in the hand test: the marks as
//! drawn, the dialog next to the reference, a C program.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::DebugState;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::debug_model::BreakpointLook;
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

const LOOP_PY: &str = "total = 0\nfor i in range(5):\n    inner = i * 10\n    total += inner\nprint(\"total\", total)\n";

fn debugpy_installed() -> bool {
    ferris_idea_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some())
}

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-breakpoint-dialog-keys-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("loop.py"), LOOP_PY).unwrap();
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

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

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn state(bench: &Bench, cx: &mut VisualTestContext) -> Option<DebugState> {
    bench.read_with(cx, |b, cx| {
        b.debug_view().map(|v| v.read(cx).state().clone())
    })
}

fn paused(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    matches!(state(bench, cx), Some(DebugState::Paused(_)))
}

fn console(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .map(|v| v.read(cx).console().to_owned())
            .unwrap_or_default()
    })
}

fn variables(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .map(|v| v.read(cx).variable_texts())
            .unwrap_or_default()
    })
}

fn look(editor: &Entity<EditorView>, cx: &mut VisualTestContext, line: usize) -> BreakpointLook {
    editor.read_with(cx, |e, _| e.breakpoint_look(line))
}

fn saved(root: &Path) -> String {
    std::fs::read_to_string(root.join(".ferris-idea/breakpoints.toml")).unwrap_or_default()
}

/// ⇧⌘F8 on the caret's breakpoint, `condition` typed over the Condition field, Enter. Since
/// `SPEC-0127` v10 the key opens the small popup beside the line (behaviour 19).
fn type_condition(bench: &Bench, cx: &mut VisualTestContext, condition: &str) {
    keys(cx, "cmd-shift-f8");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the breakpoint popup is open");
    let focused = cx.update(|window, cx| popup.read(cx).focus(cx).is_focused(window));
    assert!(focused, "the focus is in the Condition field");
    keys(cx, "cmd-a");
    cx.simulate_input(condition);
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(
        bench.read_with(cx, |b, _| b.breakpoint_popup()).is_none(),
        "Enter applies and closes"
    );
}

/// `SPEC-0127` v9 AK-14 (hand test 54, the steps left out on 2026-11-13).
#[gpui::test]
fn hand_test_54_condition_moves_with_an_inserted_line_and_a_failed_condition_is_told(
    cx: &mut TestAppContext,
) {
    if !debugpy_installed() {
        eprintln!("skipped: debugpy is missing");
        return;
    }
    let root = workspace("54");
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.open(&root.join("loop.py"), window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("loop.py is open");

    // ⌘F8 on `inner = …` (line 3) and on `total += inner` (line 4); the caret ends on line 4.
    keys(cx, "down down");
    keys(cx, "cmd-f8");
    keys(cx, "down");
    keys(cx, "cmd-f8");
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoints().to_vec()),
        [2, 3]
    );
    // Back on line 3, its breakpoint gets the condition.
    keys(cx, "up");
    type_condition(&bench, cx, "i == 3");
    assert_eq!(look(&editor, cx, 2), BreakpointLook::Conditional);
    assert_eq!(look(&editor, cx, 3), BreakpointLook::Plain);
    until(cx, 10, "the condition is saved", |_| {
        saved(&root).contains("i == 3")
    });

    // A line typed above both moves them and the condition one line down.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-home");
    cx.simulate_input("# counts\n");
    cx.run_until_parked();
    assert!(
        editor
            .read_with(cx, |e, _| e.state.buffer.text())
            .starts_with("# counts\ntotal = 0\n")
    );
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoints().to_vec()),
        [3, 4]
    );
    assert_eq!(look(&editor, cx, 3), BreakpointLook::Conditional);
    assert_eq!(look(&editor, cx, 4), BreakpointLook::Plain);
    keys(cx, "cmd-s");
    until(cx, 10, "the moved condition is saved with its line", |_| {
        let t = saved(&root);
        t.contains("i == 3") && t.contains("line = 4")
    });
    // The plain breakpoint below is taken away, so only the condition decides the stop.
    keys(cx, "cmd-end");
    keys(cx, "up up");
    keys(cx, "cmd-f8");
    assert_eq!(editor.read_with(cx, |e, _| e.breakpoints().to_vec()), [3]);

    // Debug: one stop, on the pass where `i == 3`; Resume runs to the end.
    keys(cx, "ctrl-shift-d");
    until(
        cx,
        120,
        "the program stops at the conditional breakpoint",
        |cx| paused(&bench, cx),
    );
    until(cx, 30, "the locals are shown", |cx| {
        variables(&bench, cx).iter().any(|v| v == "i = 3")
    });
    keys(cx, "cmd-alt-r");
    until(cx, 60, "the program runs to its end", |cx| {
        matches!(state(&bench, cx), Some(DebugState::Ended(_)))
    });
    assert!(
        console(&bench, cx).contains("total 100"),
        "{}",
        console(&bench, cx)
    );

    // A condition with an unknown name: the program stops at the first hit and the console says
    // why.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-home");
    keys(cx, "down down down");
    type_condition(&bench, cx, "nope == 3");
    until(cx, 10, "the new condition is saved", |_| {
        saved(&root).contains("nope == 3")
    });
    keys(cx, "ctrl-shift-d");
    until(cx, 120, "a failed condition stops the program", |cx| {
        paused(&bench, cx)
    });
    until(cx, 30, "the console names the error", |cx| {
        console(&bench, cx).contains("NameError")
    });
    until(cx, 30, "it stopped at the first hit", |cx| {
        variables(&bench, cx).iter().any(|v| v == "i = 0")
    });
    bench.update_in(cx, |b, window, cx| b.menu_action("Stop", window, cx));
    cx.run_until_parked();
    let _ = std::fs::remove_dir_all(&root);
}
