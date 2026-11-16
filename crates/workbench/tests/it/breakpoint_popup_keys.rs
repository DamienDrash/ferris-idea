//! OBS-0054 B1–B5 in a window of the test harness with real keys: ⌘F8 on a Java method header
//! sets a method breakpoint (`SPEC-0146` v6 AK-v6-3), ⇧⌘F8 edits the breakpoint at the caret in
//! the small popup and "More" opens the dialog (`SPEC-0127` v10 AK-15), the dialog's row check
//! boxes, Suspend All/Thread, the dependency chosen from the breakpoints and the exception
//! breakpoints per language; the dialog's switches as a list with All chosen and the dependency
//! chosen by clicks (`SPEC-0127` v12, OBS-0056 D1, D2, B4); the popup and the dialog showing one
//! state of a breakpoint (`SPEC-0127` v13 AK-18). No debugger is started.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_debugger::Suspend;
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::breakpoint_dialog::Row;
use idecraft_workbench::breakpoint_popup::{RowKind, Target, title_of};
use idecraft_workbench::debug_model::BreakpointLook;
use idecraft_workbench::{EditorView, WorkbenchView, semantics};

type Bench = Entity<WorkbenchView>;

const CART: &str = "package shop;\n\npublic class Cart {\n    int total(int n) {\n        return n * 2;\n    }\n}\n";
const LOOP_PY: &str = "total = 0\nfor i in range(5):\n    total += i\nprint(total)\n";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "idecraft-breakpoint-popup-keys-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::write(root.join("src/shop/Cart.java"), CART).unwrap();
    std::fs::write(root.join("loop.py"), LOOP_PY).unwrap();
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open")
}

fn special(root: &Path) -> idecraft_debugger::SpecialBreakpoints {
    idecraft_debugger::saved::load_special_breakpoints(root)
}

fn popup_open(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, _| b.breakpoint_popup().is_some())
}

fn dialog_open(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, _| b.breakpoint_dialog().is_some())
}

/// `SPEC-0146` v6 AK-v6-3 and the method part of `SPEC-0127` v10 AK-15.
#[gpui::test]
fn cmd_f8_on_a_method_header_sets_a_method_breakpoint(cx: &mut TestAppContext) {
    let root = workspace("method");
    let (bench, cx) = window(&root, cx);
    let cart = root.join("src/shop/Cart.java");
    let editor = open(&bench, cx, &cart);

    // ⌘F8 on `int total(int n) {` (line 4).
    keys(cx, "down down down");
    keys(cx, "cmd-f8");
    let s = special(&root);
    assert_eq!(s.functions.len(), 1, "{s:?}");
    assert_eq!(s.functions[0].name, "shop.Cart.total");
    assert_eq!(s.functions[0].file.as_deref(), Some(cart.as_path()));
    assert_eq!(s.functions[0].line, Some(3));
    assert!(editor.read_with(cx, |e, _| e.breakpoints().is_empty()));
    assert_eq!(
        editor.read_with(cx, |e, _| e.method_breakpoint(3)),
        Some(true)
    );

    // On the body line a line breakpoint.
    keys(cx, "down");
    keys(cx, "cmd-f8");
    assert_eq!(editor.read_with(cx, |e, _| e.breakpoints().to_vec()), [4]);
    assert_eq!(special(&root).functions.len(), 1);

    // ⇧⌘F8 on the header: the popup with entry and exit; exit on, entry off, both off refused.
    keys(cx, "up");
    keys(cx, "cmd-shift-f8");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the popup is open");
    assert!(matches!(
        popup.read_with(cx, |p, _| p.target().clone()),
        Target::Method(0, _)
    ));
    keys(cx, "alt-x");
    keys(cx, "alt-n");
    keys(cx, "alt-x");
    assert_eq!(
        popup.read_with(cx, |p, _| p.state().method),
        Some((false, true)),
        "exit stays on while entry is off"
    );
    keys(cx, "enter");
    assert!(!popup_open(&bench, cx));
    let f = special(&root).functions[0].clone();
    assert!(!f.entry && f.exit, "{f:?}");

    // ⌥E in the popup switches it off; the gutter draws the outline diamond (SPEC-0146 v7).
    keys(cx, "cmd-shift-f8");
    keys(cx, "alt-e");
    keys(cx, "enter");
    assert!(special(&root).functions[0].disabled);
    assert_eq!(
        editor.read_with(cx, |e, _| e.method_breakpoint(3)),
        Some(false)
    );

    // ⌘F8 on the header again removes it.
    keys(cx, "cmd-f8");
    assert!(special(&root).functions.is_empty());
    assert_eq!(editor.read_with(cx, |e, _| e.method_breakpoint(3)), None);
    assert_eq!(editor.read_with(cx, |e, _| e.breakpoints().to_vec()), [4]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0127` v10 AK-15, the line breakpoint part.
#[gpui::test]
fn the_popup_edits_a_breakpoint_and_more_opens_the_dialog(cx: &mut TestAppContext) {
    let root = workspace("popup");
    let (bench, cx) = window(&root, cx);
    let file = root.join("loop.py");
    let editor = open(&bench, cx, &file);
    keys(cx, "down");
    keys(cx, "cmd-f8");
    assert_eq!(editor.read_with(cx, |e, _| e.breakpoints().to_vec()), [1]);

    // ⇧⌘F8: the popup, the focus in Condition.
    keys(cx, "cmd-shift-f8");
    assert!(!dialog_open(&bench, cx), "not the dialog");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the popup is open");
    let focused = cx.update(|window, cx| popup.read(cx).focus(cx).is_focused(window));
    assert!(focused, "the focus is in the Condition field");
    cx.simulate_input("i > 2");
    cx.run_until_parked();
    keys(cx, "alt-s");
    assert_eq!(
        popup.read_with(cx, |p, _| p.state().suspend),
        Some(Suspend::None)
    );
    keys(cx, "alt-s");
    keys(cx, "alt-t");
    keys(cx, "enter");
    assert!(!popup_open(&bench, cx), "Enter applies and closes");
    let props = |bench: &Bench, cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| b.open_breakpoints(None, window, cx));
        let p = bench.read_with(cx, |b, cx| {
            b.breakpoint_dialog()
                .and_then(|d| d.read(cx).selected().map(|e| e.props.clone()))
        });
        keys(cx, "escape");
        p.expect("the breakpoint is listed")
    };
    let p = props(&bench, cx);
    assert_eq!(p.condition.as_deref(), Some("i > 2"));
    assert_eq!(p.rules.suspend, Some(Suspend::Thread));
    assert!(
        std::fs::read_to_string(root.join(".idecraft/breakpoints.toml"))
            .unwrap_or_default()
            .contains("i > 2"),
        "saved"
    );

    // ⌥E, Escape: nothing changes.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    keys(cx, "alt-e");
    keys(cx, "escape");
    assert!(!popup_open(&bench, cx));
    assert_ne!(
        editor.read_with(cx, |e, _| e.breakpoint_look(1)),
        BreakpointLook::Disabled
    );

    // ⇧⌘F8 twice: More opens the dialog with the breakpoint selected.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    keys(cx, "cmd-shift-f8");
    assert!(!popup_open(&bench, cx));
    let selected = bench.read_with(cx, |b, cx| {
        b.breakpoint_dialog()
            .and_then(|d| d.read(cx).selected().map(|e| (e.path.clone(), e.line)))
    });
    assert_eq!(selected, Some((file.clone(), 1)));
    keys(cx, "escape");

    // On a line without a breakpoint ⇧⌘F8 opens the dialog.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "down");
    keys(cx, "cmd-shift-f8");
    assert!(dialog_open(&bench, cx));
    assert!(!popup_open(&bench, cx));
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0127` v10 AK-15 and `SPEC-0146` v6 AK-v6-3, the dialog part.
#[gpui::test]
fn exception_breakpoints_per_language_and_row_checks(cx: &mut TestAppContext) {
    let root = workspace("rows");
    let (bench, cx) = window(&root, cx);
    let file = root.join("loop.py");
    let editor = open(&bench, cx, &file);
    keys(cx, "cmd-f8");
    keys(cx, "down down");
    keys(cx, "cmd-f8");
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoints().to_vec()),
        [0, 2]
    );

    // On a line without a breakpoint ⇧⌘F8 opens the dialog; the first breakpoint is selected.
    keys(cx, "down");
    keys(cx, "cmd-shift-f8");
    let dialog = bench
        .read_with(cx, |b, _| b.breakpoint_dialog())
        .expect("the dialog is open");
    assert_eq!(dialog.read_with(cx, |d, _| d.row()), Row::Line(0));

    // ⌥E switches the row off and on at once.
    keys(cx, "alt-e");
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoint_look(0)),
        BreakpointLook::Disabled
    );
    keys(cx, "alt-e");
    assert_ne!(
        editor.read_with(cx, |e, _| e.breakpoint_look(0)),
        BreakpointLook::Disabled
    );

    // ⌥D chooses the other breakpoint, ⌥K leaves it enabled, ⌥A suspends all threads.
    keys(cx, "alt-d");
    keys(cx, "alt-k");
    keys(cx, "alt-a");
    let r = dialog.read_with(cx, |d, cx| d.props(cx).rules);
    let other = format!("{}:3", file.display());
    assert_eq!(r.depends_on.as_deref(), Some(other.as_str()));
    assert!(r.depends_keep);
    assert_eq!(r.suspend, Some(Suspend::All));

    // Down to the exception rows: Java, then Python; ⌥U sets uncaught for Python only.
    keys(cx, "down down");
    assert_eq!(dialog.read_with(cx, |d, _| d.row()), Row::Exceptions(0));
    keys(cx, "down");
    assert_eq!(dialog.read_with(cx, |d, _| d.row()), Row::Exceptions(1));
    keys(cx, "alt-u");
    keys(cx, "enter");
    assert!(!dialog_open(&bench, cx));
    let s = special(&root);
    assert!(s.exceptions_of("python").uncaught, "{s:?}");
    assert!(!s.exceptions_of("java").uncaught);
    assert!(!s.uncaught, "the shared setting stays");
    let text =
        std::fs::read_to_string(root.join(idecraft_debugger::saved::SPECIAL_BREAKPOINTS_FILE))
            .unwrap_or_default();
    assert!(text.contains("\"languages\""), "{text}");

    // The dependency, its switch and Suspend were saved with the first breakpoint.
    bench.update_in(cx, |b, window, cx| b.open_breakpoints(None, window, cx));
    let rules = bench.read_with(cx, |b, cx| {
        b.breakpoint_dialog()
            .map(|d| d.read(cx).list()[0].props.rules.clone())
    });
    let rules = rules.expect("the dialog is open");
    assert_eq!(rules.depends_on.as_deref(), Some(other.as_str()));
    assert!(rules.depends_keep);
    assert_eq!(rules.suspend, Some(Suspend::All));
    assert_eq!(
        idecraft_workbench::breakpoint_dialog::exception_headings(),
        [
            "Java Exception Breakpoints",
            "Python Exception Breakpoints",
            "JavaScript Exception Breakpoints",
            "Native Exception Breakpoints",
            "Go Exception Breakpoints",
        ]
    );
    // The Python row's check box is on.
    let on = bench.read_with(cx, |b, cx| {
        b.breakpoint_dialog().map(|d| d.read(cx).exceptions(1).on())
    });
    assert_eq!(on, Some(true));
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0127` v11 AK-16 (OBS-0056 P2–P6): the popup is headed by the kind of breakpoint, shows
/// check boxes with All chosen; a condition applied comes back with ↑ in the next popup; ⇧↩
/// expands the field, ↩ adds a line there and ⌘↩ applies; a method breakpoint's popup shows Watch.
#[gpui::test]
fn the_popup_shows_kind_rows_and_history(cx: &mut TestAppContext) {
    let root = workspace("look");
    let (bench, cx) = window(&root, cx);
    let file = root.join("loop.py");
    open(&bench, cx, &file);
    keys(cx, "down");
    keys(cx, "cmd-f8");
    keys(cx, "cmd-shift-f8");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the popup is open");
    let (kind, place) = popup.read_with(cx, |p, _| title_of(p.target()));
    assert_eq!((kind, place.as_str()), ("Line breakpoint", "loop.py:2"));
    let rows = popup.read_with(cx, |p, _| p.state().rows());
    let all = rows.iter().find(|r| r.label == "All").expect("All");
    assert!(
        all.on && all.kind == RowKind::Option,
        "All is chosen: {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.label == "Suspend" && r.on && r.kind == RowKind::Check)
    );
    // Escape: nothing saved, the breakpoint keeps no choice of threads.
    keys(cx, "escape");
    let saved =
        || std::fs::read_to_string(root.join(".idecraft/breakpoints.toml")).unwrap_or_default();
    assert!(!saved().contains("suspend"), "{}", saved());

    // A condition applied comes back with ↑ in the next popup; All is saved with it.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    cx.simulate_input("i > 2");
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(saved().contains("i > 2"), "{}", saved());
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the popup is open again");
    // Clear the field, then ↑ brings the recent condition back and ↓ the typed (empty) text.
    keys(cx, "cmd-a backspace");
    keys(cx, "up");
    assert_eq!(popup.read_with(cx, |p, cx| p.condition(cx)), "i > 2");
    keys(cx, "down");
    assert_eq!(popup.read_with(cx, |p, cx| p.condition(cx)), "");

    // ⇧↩ expands; ↩ adds a line; ⌘↩ applies the two-line condition.
    keys(cx, "shift-enter");
    assert!(popup.read_with(cx, |p, _| p.expanded()));
    cx.simulate_input("i > 3");
    cx.run_until_parked();
    keys(cx, "enter");
    assert!(popup_open(&bench, cx), "↩ starts a new line while expanded");
    cx.simulate_input("and i < 5");
    cx.run_until_parked();
    keys(cx, "cmd-enter");
    assert!(!popup_open(&bench, cx), "⌘↩ applies and closes");
    let text = saved();
    assert!(
        text.contains("i > 3") && text.contains("and i < 5"),
        "{text}"
    );

    // A method breakpoint: "Method breakpoint" and the Watch section.
    let cart = root.join("src/shop/Cart.java");
    open(&bench, cx, &cart);
    keys(cx, "down down down");
    keys(cx, "cmd-f8");
    keys(cx, "cmd-shift-f8");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the method popup is open");
    let (kind, place) = popup.read_with(cx, |p, _| title_of(p.target()));
    assert_eq!(
        (kind, place.as_str()),
        ("Method breakpoint", "shop.Cart.total")
    );
    let labels: Vec<_> = popup.read_with(cx, |p, _| {
        p.state().rows().into_iter().map(|r| r.label).collect()
    });
    assert_eq!(labels, ["Enabled", "Watch", "Method entry", "Method exit"]);
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// A node of the semantic tree by its test id: its left edge at mid height, whether it is
/// checked, its value.
fn node(
    id: &str,
    cx: &mut VisualTestContext,
) -> Option<(gpui::Point<gpui::Pixels>, Option<bool>, Option<String>)> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let n = tree.iter().find(|n| n.test_id() == Some(id))?;
        let r = n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
        Some((
            gpui::point(px(r.x), px(r.y + r.h / 2.)),
            n.checked(),
            n.raw_value().map(str::to_owned),
        ))
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let (at, _, _) = node(id, cx).unwrap_or_else(|| panic!("no control {id} in view"));
    let at = gpui::point(at.x + px(4.), at.y);
    cx.simulate_mouse_move(at, None, gpui::Modifiers::none());
    cx.simulate_click(at, gpui::Modifiers::none());
    cx.run_until_parked();
}

/// `SPEC-0127` v12 AK-17 (OBS-0056 D1, D2 and B4): in the dialog All is chosen while Suspend is
/// on, the switches stand one under the other with All and Thread indented under Suspend, a click
/// chooses Thread, the keys still work, and "Disable until hitting" is chosen by clicks from the
/// other breakpoints and kept with Done.
#[gpui::test]
fn the_dialogs_switches_are_a_list_and_the_dependency_is_chosen(cx: &mut TestAppContext) {
    let root = workspace("switches");
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let (bench, cx) = window(&root, cx);
    let file = root.join("loop.py");
    let editor = open(&bench, cx, &file);
    keys(cx, "cmd-f8");
    keys(cx, "down down");
    keys(cx, "cmd-f8");
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoints().to_vec()),
        [0, 2]
    );
    keys(cx, "down");
    keys(cx, "cmd-shift-f8");
    let dialog = bench
        .read_with(cx, |b, _| b.breakpoint_dialog())
        .expect("the dialog is open");
    assert_eq!(dialog.read_with(cx, |d, _| d.row()), Row::Line(0));

    // D1: All is chosen, as in the popup.
    assert_eq!(
        dialog.read_with(cx, |d, _| d.shown_suspend()),
        Some(Suspend::All)
    );
    let ids = [
        "breakpoint-enabled",
        "breakpoint-suspend",
        "breakpoint-suspend-all",
        "breakpoint-suspend-thread",
        "breakpoint-log-hit",
        "breakpoint-log-stack",
        "breakpoint-remove-once",
    ];
    let nodes: Vec<_> = ids
        .iter()
        .map(|id| node(id, cx).unwrap_or_else(|| panic!("no {id}")))
        .collect();
    let checked: Vec<Option<bool>> = nodes.iter().map(|n| n.1).collect();
    assert_eq!(
        checked,
        [
            Some(true),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(false),
            Some(false)
        ]
    );
    // D2: one under the other; All and Thread indented under Suspend, the checks in one column.
    for pair in nodes.windows(2) {
        assert!(pair[1].0.y > pair[0].0.y, "a vertical list");
    }
    let x = |i: usize| nodes[i].0.x;
    assert_eq!(x(0), x(1));
    assert_eq!(x(1), x(4));
    assert_eq!(x(4), x(6));
    assert!(
        x(2) > x(1) && x(2) == x(3),
        "options indented under Suspend"
    );

    // A click chooses Thread; ⌥A, the key, goes back to All.
    click("breakpoint-suspend-thread", cx);
    assert_eq!(
        dialog.read_with(cx, |d, _| d.shown_suspend()),
        Some(Suspend::Thread)
    );
    keys(cx, "alt-a");
    assert_eq!(
        dialog.read_with(cx, |d, _| d.shown_suspend()),
        Some(Suspend::All)
    );
    click("breakpoint-suspend-thread", cx);
    click("breakpoint-log-hit", cx);
    assert_eq!(node("breakpoint-log-hit", cx).unwrap().1, Some(true));

    // B4: "Disable until hitting" from the other breakpoints, by clicks; the form is scrolled to
    // it with the wheel.
    let (over, _, _) = node("breakpoint-remove-once", cx).unwrap();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(over.x + px(20.), over.y),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-600.))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(
        node("breakpoint-depends", cx).unwrap().2.as_deref(),
        Some("None")
    );
    click("breakpoint-depends", cx);
    let chosen = node("breakpoint-depends", cx).unwrap().2.unwrap();
    assert!(
        chosen.contains("loop.py:3"),
        "the other breakpoint: {chosen}"
    );
    click("breakpoint-depends", cx);
    assert_eq!(
        node("breakpoint-depends", cx).unwrap().2.as_deref(),
        Some("None"),
        "and back to none"
    );
    click("breakpoint-depends", cx);
    assert_eq!(
        node("breakpoint-depends-keep", cx).unwrap().2.as_deref(),
        Some("Disable again")
    );
    click("breakpoint-depends-keep", cx);
    assert_eq!(
        node("breakpoint-depends-keep", cx).unwrap().2.as_deref(),
        Some("Leave enabled")
    );

    // Done keeps it all.
    keys(cx, "enter");
    assert!(!dialog_open(&bench, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    let dialog = bench
        .read_with(cx, |b, _| b.breakpoint_dialog())
        .expect("the dialog is open again");
    let props = dialog.read_with(cx, |d, _| d.selected().map(|e| e.props.clone()));
    let props = props.expect("the first breakpoint");
    assert_eq!(props.rules.suspend, Some(Suspend::Thread));
    assert!(props.rules.log_hit);
    assert!(props.rules.depends_keep);
    assert!(
        props
            .rules
            .depends_on
            .as_deref()
            .is_some_and(|d| d.ends_with("loop.py:3")),
        "{props:?}"
    );
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// The switches a view shares with the other one, by its test id prefix (`breakpoint-popup-` or
/// `breakpoint-`): for Enabled, Suspend, All and Thread their role and checked state, `None` for
/// one that is not in view.
fn shared(prefix: &str, cx: &mut VisualTestContext) -> Vec<Option<(semantics::Role, bool)>> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window));
        ["enabled", "suspend", "suspend-all", "suspend-thread"]
            .iter()
            .map(|id| {
                let id = format!("{prefix}{id}");
                let n = tree
                    .as_ref()?
                    .iter()
                    .find(|n| n.test_id() == Some(id.as_str()))?;
                n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
                Some((n.role(), n.checked()?))
            })
            .collect()
    })
}

/// The checked states of `shared`, `None` for a switch not in view.
fn checks(prefix: &str, cx: &mut VisualTestContext) -> Vec<Option<bool>> {
    shared(prefix, cx)
        .into_iter()
        .map(|s| s.map(|(_, on)| on))
        .collect()
}

const POPUP: &str = "breakpoint-popup-";
const DIALOG: &str = "breakpoint-";

/// `SPEC-0127` v13 AK-18 (OBS-0056 D1, D2): the popup and the dialog show one state of a line
/// breakpoint. The dialog is opened as the recheck opened it, with More from the popup; All is
/// chosen there, the switches are a vertical list of check boxes with All and Thread as options,
/// and what one view applies the other shows.
#[gpui::test]
fn the_dialog_and_the_popup_show_one_state(cx: &mut TestAppContext) {
    use semantics::Role;
    let root = workspace("one-state");
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let (bench, cx) = window(&root, cx);
    let file = root.join("loop.py");
    let editor = open(&bench, cx, &file);
    keys(cx, "down");
    keys(cx, "cmd-f8");
    assert_eq!(editor.read_with(cx, |e, _| e.breakpoints().to_vec()), [1]);
    let saved =
        || std::fs::read_to_string(root.join(".idecraft/breakpoints.toml")).unwrap_or_default();

    // ⇧⌘F8: the popup with All chosen; ⇧⌘F8 there (More) opens the dialog.
    keys(cx, "cmd-shift-f8");
    assert!(popup_open(&bench, cx));
    let on_all = [Some(true), Some(true), Some(true), Some(false)];
    assert_eq!(checks(POPUP, cx), on_all, "the popup shows All chosen");
    keys(cx, "cmd-shift-f8");
    assert!(dialog_open(&bench, cx) && !popup_open(&bench, cx));
    let dialog = bench
        .read_with(cx, |b, _| b.breakpoint_dialog())
        .expect("the dialog is open");
    assert_eq!(dialog.read_with(cx, |d, _| d.row()), Row::Line(0));

    // D1: All chosen, Thread not, as in the popup; nothing was saved, so the breakpoint keeps
    // having no choice of threads.
    assert_eq!(checks(DIALOG, cx), on_all, "the dialog shows All chosen");
    assert_eq!(
        dialog.read_with(cx, |d, _| d.shown_suspend()),
        Some(Suspend::All)
    );
    assert!(!saved().contains("suspend"), "{}", saved());

    // D2: check boxes one under the other, All and Thread options under Suspend, then the
    // dialog's own switches.
    let roles: Vec<_> = shared(DIALOG, cx)
        .into_iter()
        .map(|s| s.expect("in view").0)
        .collect();
    assert_eq!(
        roles,
        [
            Role::CheckBox,
            Role::CheckBox,
            Role::RadioButton,
            Role::RadioButton
        ]
    );
    let ids = [
        "breakpoint-enabled",
        "breakpoint-suspend",
        "breakpoint-suspend-all",
        "breakpoint-suspend-thread",
        "breakpoint-log-hit",
        "breakpoint-log-stack",
        "breakpoint-remove-once",
    ];
    let at: Vec<_> = ids
        .iter()
        .map(|id| node(id, cx).unwrap_or_else(|| panic!("no {id}")).0)
        .collect();
    for pair in at.windows(2) {
        assert!(pair[1].y > pair[0].y, "a vertical list: {at:?}");
    }
    assert!(
        at[2].x > at[1].x && at[2].x == at[3].x,
        "options under Suspend"
    );
    assert!(at[0].x == at[1].x && at[1].x == at[4].x && at[4].x == at[6].x);

    // A click on Suspend switches it off: All and Thread go. Done saves it.
    click("breakpoint-suspend", cx);
    assert_eq!(checks(DIALOG, cx), [Some(true), Some(false), None, None]);
    keys(cx, "enter");
    assert!(!dialog_open(&bench, cx));
    assert!(saved().contains("suspend = \"none\""), "{}", saved());

    // The popup shows the same: Suspend off, neither All nor Thread.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    let popup = bench
        .read_with(cx, |b, _| b.breakpoint_popup())
        .expect("the popup is open");
    assert_eq!(checks(POPUP, cx), [Some(true), Some(false), None, None]);
    assert_eq!(
        popup.read_with(cx, |p, _| p.state().suspend),
        Some(Suspend::None)
    );

    // In the popup ⌥S and ⌥T, then More: the dialog shows Thread chosen.
    keys(cx, "alt-s");
    keys(cx, "alt-t");
    let on_thread = [Some(true), Some(true), Some(false), Some(true)];
    assert_eq!(checks(POPUP, cx), on_thread);
    keys(cx, "cmd-shift-f8");
    assert!(dialog_open(&bench, cx) && !popup_open(&bench, cx));
    assert_eq!(
        checks(DIALOG, cx),
        on_thread,
        "the dialog shows the popup's Thread"
    );
    assert!(saved().contains("suspend = \"thread\""), "{}", saved());

    // A click on Enabled and Done: the popup shows Enabled off and Thread chosen.
    click("breakpoint-enabled", cx);
    assert_eq!(checks(DIALOG, cx)[0], Some(false));
    keys(cx, "enter");
    assert_eq!(
        editor.read_with(cx, |e, _| e.breakpoint_look(1)),
        BreakpointLook::Disabled
    );
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-shift-f8");
    assert!(popup_open(&bench, cx));
    assert_eq!(
        checks(POPUP, cx),
        [Some(false), Some(true), Some(false), Some(true)]
    );
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}
