//! Re-checks without a screen of the hand tests that hold rows at stage 3 in PARITY.md (F-002,
//! F-012, F-038, F-071, F-080), done on 2026-11-14: the steps of hand tests 21, 53, 56, 57, 72,
//! 77 and 110 with real key and mouse events where the hand test names them, the public calls of
//! the workbench where a step has no control to reach headless (the file dialog of Install Plugin
//! from Disk). Each test names the defect of the hand test it re-checks
//! (docs/testing/DEFECTS-2026-11-05.md).
//!
//! What needs eyes on a real window stays in the hand tests: how the rows, popups and the
//! rectangle are drawn, a real keyboard and trackpad, and the comparison with the reference.
//! Tests with a debugger or a language server are skipped when it is missing.
//!
//! `SPEC-0091` v5 (hand test 21), `SPEC-0126` v2 (53), `SPEC-0129` v2 (56), `SPEC-0130` (57),
//! `SPEC-0146` v2 (72), `SPEC-0151` (77), `SPEC-0185`, `SPEC-0071` v6, `SPEC-0077` v2 (110).

#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, px};
use idecraft_workbench::DebugState;
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::settings_model::Page;
use idecraft_workbench::{EditorView, MenuPlace, WorkbenchView};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "idecraft-handtest-rechecks-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    root.canonicalize().unwrap()
}

/// A window on `root` with a settings file and theme file of its own: the user's settings are
/// never read or written.
fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    idecraft_workbench::hit_targets::enable();
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

/// Waits up to `secs` seconds of real time until `done` holds, letting background work and timers
/// run.
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

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    // One frame so the editor has painted lines to aim the pointer at.
    cx.update(|window, cx| window.draw(cx).clear());
    bench.read_with(cx, |b, _| {
        b.active_editor_entity().expect("the file is open")
    })
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// The ranges of the carets, in order.
fn parts(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<std::ops::Range<usize>> {
    editor.read_with(cx, |e, _| {
        e.state.carets.carets().iter().map(|c| c.range()).collect()
    })
}

/// The window position at the left edge of char `offset` (between it and the char before), in
/// its line, as last drawn: the place a column selection's corner snaps to.
fn at_char(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    offset: usize,
) -> gpui::Point<gpui::Pixels> {
    cx.update(|window, cx| window.draw(cx).clear());
    let below = editor
        .read_with(cx, |e, _| e.point_below(offset))
        .unwrap_or_else(|| panic!("char {offset} is drawn"));
    gpui::point(below.x, below.y - px(4.))
}

/// A drag with the primary button from the left edge of char `from` to that of char `to`, with
/// `modifiers` held.
fn drag(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    from: usize,
    to: usize,
    modifiers: Modifiers,
) {
    let start = at_char(editor, cx, from);
    let end = at_char(editor, cx, to);
    cx.simulate_mouse_down(start, MouseButton::Left, modifiers);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), modifiers);
    cx.simulate_mouse_up(end, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

// ------------------------------------------------------------------- hand tests 57 and 77

const COLUMNS: &str = "alpha one\nbe\ngamma two\n";

/// Hand test 57 (`SPEC-0130` behaviours 1, 2): ⌥ held, a drag over three lines selects the same
/// columns on each; the short line gets a caret at its end; typing and ⌫ act on every line;
/// a rectangle copied with ⌘C goes into another rectangle of as many lines with ⌘V, one line each.
#[gpui::test]
fn hand_test_57_column_selection_with_the_mouse(cx: &mut TestAppContext) {
    let root = workspace("57", &[("columns.txt", COLUMNS)]);
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("columns.txt"));

    // ⌥-drag from "one" (line 0, column 6) to the end of "two" (line 2, column 9).
    drag(&editor, cx, 6, 22, Modifiers::alt());
    assert_eq!(
        parts(&editor, cx),
        [6..9, 12..12, 19..22],
        "one part per line, a caret at the end of the short line"
    );
    keys(cx, "X");
    assert_eq!(text(&bench, cx), "alpha X\nbeX\ngamma X\n");
    keys(cx, "backspace");
    assert_eq!(
        text(&bench, cx),
        "alpha \nbe\ngamma \n",
        "⌫ deletes at every caret"
    );
    keys(cx, "cmd-z cmd-z");
    assert_eq!(text(&bench, cx), COLUMNS);

    // ⌥-drag again over "alpha"/"be"/"gamma" (columns 0–3), ⌘C, then into the rectangle of
    // columns 6–9: each line gets its own line of the copy.
    drag(&editor, cx, 0, 16, Modifiers::alt());
    assert_eq!(parts(&editor, cx), [0..3, 10..12, 13..16]);
    keys(cx, "cmd-c");
    let copied = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert_eq!(copied, "alp\nbe\ngam");
    drag(&editor, cx, 6, 22, Modifiers::alt());
    keys(cx, "cmd-v");
    assert_eq!(text(&bench, cx), "alpha alp\nbebe\ngamma gam\n");

    // Without ⌥ the drag selects as before: one selection from the start to the end.
    keys(cx, "cmd-z");
    drag(&editor, cx, 6, 22, Modifiers::none());
    assert_eq!(
        parts(&editor, cx).len(),
        1,
        "a plain drag makes one selection"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 77, the mouse part (`SPEC-0151` behaviours 1, 2): with column selection mode on a
/// plain drag selects a rectangle over the short line; ⇧⌘8 switches it off and a plain drag
/// selects normally again.
#[gpui::test]
fn hand_test_77_column_mode_with_the_mouse(cx: &mut TestAppContext) {
    let root = workspace("77", &[("colmode.txt", COLUMNS)]);
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("colmode.txt"));
    keys(cx, "cmd-shift-8");
    assert!(
        editor.read_with(cx, |e, _| e.column_mode()),
        "⇧⌘8 switches the mode on"
    );

    drag(&editor, cx, 6, 22, Modifiers::none());
    assert_eq!(
        parts(&editor, cx),
        [6..9, 12..12, 19..22],
        "a plain drag in column mode selects the rectangle"
    );
    keys(cx, "X");
    assert_eq!(text(&bench, cx), "alpha X\nbeX\ngamma X\n");

    keys(cx, "cmd-shift-8");
    assert!(
        !editor.read_with(cx, |e, _| e.column_mode()),
        "⇧⌘8 switches it off"
    );
    drag(&editor, cx, 0, 13, Modifiers::none());
    assert_eq!(
        parts(&editor, cx),
        vec![std::ops::Range { start: 0, end: 13 }],
        "without column mode a drag selects from the start to the pointer"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 56

/// Hand test 56 (`SPEC-0129` v2 behaviours 3, 4, 6; D19): Editor › General switches both save
/// options; Terminal: `/bin/bash`, Enter applies it without adding a line, Escape leaves the
/// field and a second Escape closes Settings; saving a file strips trailing spaces except on the
/// caret line and adds the final line break; what was set is in the settings file a restart
/// reads. The font size is one value for the whole process: its keys are checked in
/// `tests/font_size_actions.rs` and in the window self-test (`ide_settings_steps`).
#[gpui::test]
fn hand_test_56_settings_pages_apply_and_are_saved(cx: &mut TestAppContext) {
    let root = workspace("56", &[("tidy.txt", "a  \nb\t\nc  ")]);
    let file = root.join(".test-settings/settings.toml");
    let (bench, cx) = window(&root, cx);
    let dialog = |bench: &Bench, cx: &mut VisualTestContext| {
        bench
            .read_with(cx, |b, _| b.settings_dialog())
            .expect("Settings is open")
    };

    // Editor › General: both options on.
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Editor, window, cx)
    });
    cx.run_until_parked();
    keys(cx, "enter down space");
    let saved = idecraft_core::load_ide_settings(&file);
    assert!(
        saved.editor.strip_trailing_spaces && saved.editor.ensure_line_break_at_end,
        "{:?}",
        saved.editor
    );

    // Terminal: the shell path, Enter (D19: Enter added a line and nothing was saved).
    let view = dialog(&bench, cx);
    view.update_in(cx, |v, window, cx| {
        v.show(Page::Terminal, cx);
        v.set_shell_text("/bin/bash", cx);
        v.focus_shell(window, cx);
    });
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(view.read_with(cx, |v, cx| v.shell_text(cx)), "/bin/bash");
    assert_eq!(
        idecraft_core::load_ide_settings(&file)
            .terminal
            .shell
            .as_deref(),
        Some("/bin/bash")
    );
    // D19: Escape was swallowed by the field. Now it leaves the field, a second one closes.
    keys(cx, "escape");
    assert!(
        bench.read_with(cx, |b, _| b.settings_dialog().is_some()),
        "the first Escape leaves the field"
    );
    keys(cx, "escape");
    assert!(
        bench.read_with(cx, |b, _| b.settings_dialog().is_none()),
        "the second Escape closes Settings"
    );

    // Saving strips the spaces at the line ends but the caret's and ends with a line break.
    let editor = open(&bench, cx, &root.join("tidy.txt"));
    editor.update(cx, |e, cx| e.select_range(1..1, cx));
    keys(cx, "cmd-s");
    assert_eq!(
        std::fs::read_to_string(root.join("tidy.txt")).unwrap(),
        "a  \nb\nc\n"
    );

    // After a restart: the settings file holds every value set here.
    let again = idecraft_core::load_ide_settings(&file);
    assert!(again.editor.strip_trailing_spaces && again.editor.ensure_line_break_at_end);
    assert_eq!(again.terminal.shell.as_deref(), Some("/bin/bash"));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 53

/// The status bar sample of `tests/plugins` (SPEC-0210): a plugin with a name and a version.
const SAMPLE_PLUGIN: &[u8] = include_bytes!("../../../../tests/plugins/status.wasm");

/// Hand test 53 (`SPEC-0126` v2, `SPEC-0211`; D18): Install from Disk shows the plugin for review
/// with name and version, Enter installs it; Enter disables it (it is no longer loaded) and
/// enables it again; the Mac's ⌫ — not only the forward Delete — asks first and then removes it.
#[gpui::test]
fn hand_test_53_plugins_are_installed_switched_and_removed(cx: &mut TestAppContext) {
    let root = workspace("53", &[("notes.txt", "notes\n")]);
    let plugins = root.join(".plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let source = root.join("downloads/lines.wasm");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, SAMPLE_PLUGIN).unwrap();
    let (bench, cx) = window(&root, cx);
    bench.update(cx, |b, cx| {
        b.set_plugins_dir(plugins.clone());
        b.reload_plugins(cx);
    });
    assert_eq!(bench.read_with(cx, |b, _| b.plugin_count()), 0);
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Plugins, window, cx)
    });
    cx.run_until_parked();
    let view = bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("Settings is open");

    // Install Plugin from Disk… with the file the dialog would return: review first.
    view.update(cx, |v, cx| v.install_plugin(&source, cx));
    cx.run_until_parked();
    let headline = view.read_with(cx, |v, _| v.plugin_review().map(|r| r.headline()));
    assert_eq!(headline.as_deref(), Some("Install Line Count 1.0"));
    assert!(
        view.read_with(cx, |v, _| v.plugins().is_empty()),
        "nothing installed yet"
    );
    keys(cx, "enter");
    let listed = view.read_with(cx, |v, _| {
        v.plugins()
            .iter()
            .map(|p| {
                let m = p.manifest.as_ref();
                (
                    p.file.clone(),
                    m.map(|m| m.name.clone()),
                    m.map(|m| m.version.clone()),
                    p.enabled,
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        listed,
        [(
            "lines".to_owned(),
            Some("Line Count".to_owned()),
            Some("1.0".to_owned()),
            true
        )]
    );
    assert_eq!(bench.read_with(cx, |b, _| b.plugin_count()), 1, "loaded");

    // Enter switches it off and on.
    keys(cx, "enter");
    assert!(!view.read_with(cx, |v, _| v.plugins()[0].enabled));
    assert_eq!(bench.read_with(cx, |b, _| b.plugin_count()), 0, "disabled");
    keys(cx, "enter");
    assert!(view.read_with(cx, |v, _| v.plugins()[0].enabled));
    assert_eq!(bench.read_with(cx, |b, _| b.plugin_count()), 1, "enabled");

    // D18: the Mac's ⌫ (backspace) with an empty search asks first, then removes.
    keys(cx, "backspace");
    assert_eq!(
        view.read_with(cx, |v, _| v.plugins().len()),
        1,
        "the first ⌫ only asks"
    );
    keys(cx, "backspace");
    assert!(
        view.read_with(cx, |v, _| v.plugins().is_empty()),
        "the second ⌫ removes it"
    );
    assert_eq!(bench.read_with(cx, |b, _| b.plugin_count()), 0);
    assert!(!plugins.join("lines.wasm").exists(), "the file is gone");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 21

const COUNT_PY: &str = "def zaehle(woerter):\n    anzahl = {}\n    for wort in woerter:\n        anzahl[wort] = anzahl.get(wort, 0) + 1\n    return anzahl\n\n\nprint(zaehle([\"a\", \"b\", \"a\"]))\n";

fn debugpy_installed() -> bool {
    idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some())
}

fn paused(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Paused(_)))
    })
}

fn watches(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .map(|v| v.read(cx).watch_texts())
            .unwrap_or_default()
    })
}

/// Hand test 21 (`SPEC-0091` v5 and v7; D2): Debug Current File stops at the breakpoint; the
/// threads are listed; the values stand after the lines; a name selected in the editor goes into
/// the watches through the editor's menu ("Add to Watches", missing in the hand test), shows its
/// value and follows a step. Skipped without debugpy.
#[gpui::test]
fn hand_test_21_add_to_watches_from_the_editor(cx: &mut TestAppContext) {
    if !debugpy_installed() {
        eprintln!("skipped: debugpy is missing");
        return;
    }
    let root = workspace("21", &[("debug/zaehlen.py", COUNT_PY)]);
    idecraft_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("debug/zaehlen.py"));
    editor.update(cx, |e, cx| e.toggle_breakpoint(3, cx));
    cx.run_until_parked();

    // Run › Debug Current File (⌃⇧D).
    keys(cx, "ctrl-shift-d");
    until(cx, 120, "the program stops at the breakpoint", |cx| {
        paused(&bench, cx)
    });
    // The thread list is asked for after the stop is shown (and after the fields of the frame),
    // so it arrives a moment later; under load that moment is long enough to read it empty.
    until(cx, 30, "the threads are listed with MainThread", |cx| {
        bench.read_with(cx, |b, cx| {
            b.debug_view()
                .map(|v| v.read(cx).thread_names())
                .unwrap_or_default()
                .iter()
                .any(|t| t.contains("MainThread"))
        })
    });
    until(cx, 30, "the values stand after the lines", |cx| {
        editor.read_with(cx, |e, _| {
            e.inline_values()
                .iter()
                .any(|(_, v)| v.starts_with("woerter"))
        })
    });

    // `anzahl` on line 4 selected, the editor's menu: the debugger's items are there.
    let line = COUNT_PY.find("anzahl[wort]").unwrap();
    editor.update(cx, |e, cx| e.select_range(line..line + 6, cx));
    bench.update(cx, |b, cx| {
        b.open_context_menu(
            MenuPlace::Editor,
            b.active_path(),
            gpui::point(px(300.), px(200.)),
            cx,
        )
    });
    cx.run_until_parked();
    let (place, items) = bench
        .read_with(cx, |b, _| b.context_menu_items())
        .expect("the editor menu is open");
    assert_eq!(place, MenuPlace::Editor);
    for item in ["Evaluate Expression…", "Add to Watches", "Run to Cursor"] {
        assert!(
            items.iter().any(|(l, on)| l == item && *on),
            "{item} in {items:?}"
        );
    }
    let chosen = bench.update_in(cx, |b, window, cx| {
        b.choose_context_item("Add to Watches", window, cx)
    });
    assert!(chosen);
    until(cx, 30, "the watch shows its value", |cx| {
        watches(&bench, cx).iter().any(|w| w == "anzahl = {}")
    });

    // Step Over twice: the first word is counted and the watch follows.
    // The view stays paused until the adapter reports the step running, so "paused" alone holds
    // right after the step was asked for; each step ends where the stack differs (line 4, 3, 4).
    let stack = |bench: &Bench, cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.debug_view()
                .map(|v| v.read(cx).stack_text())
                .unwrap_or_default()
        })
    };
    for _ in 0..2 {
        let before = stack(&bench, cx);
        bench.update_in(cx, |b, window, cx| b.menu_action("StepOver", window, cx));
        cx.run_until_parked();
        until(cx, 30, "the step ends on another line", |cx| {
            paused(&bench, cx) && stack(&bench, cx) != before
        });
    }
    until(cx, 30, "the watch follows the step", |cx| {
        watches(&bench, cx)
            .iter()
            .any(|w| w.starts_with("anzahl = ") && w.contains("'a': 1"))
    });
    bench.update_in(cx, |b, window, cx| b.menu_action("Stop", window, cx));
    cx.run_until_parked();
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 72

const CRASH_JAVA: &str = "public class Crash {\n    static int area(int w, int h) {\n        return w * h;\n    }\n    public static void main(String[] args) {\n        System.out.println(\"area \" + area(2, 3));\n        String s = null;\n        System.out.println(s.length());\n    }\n}\n";

/// Hand test 72, the Java half (`SPEC-0146` v2 behaviours 2, 3; D30): with real jdtls and
/// java-debug a method breakpoint `Crash.area` stops on entry of the method, and an exception
/// breakpoint limited to `java.lang.NullPointerException` stops where it is thrown; limited to
/// another type it does not stop, and nothing is refused in the output. Skipped without jdtls or
/// java-debug.
#[test]
fn hand_test_72_java_method_and_typed_exception_breakpoints() {
    use idecraft_debugger::{FunctionBreakpoint, SpecialBreakpoints};
    use idecraft_workbench::debug_model::{Command, Controller, Kind, Launch, Update};
    if !idecraft_languages::java_debug::java_debug_usable()
        || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
    {
        eprintln!("skipped: jdtls or java-debug is missing");
        return;
    }
    let root = workspace("72", &[("Crash.java", CRASH_JAVA)]);
    idecraft_core::trust::trust_for_process(&root);
    let launch = |types: &str| Launch {
        kind: Kind::Java,
        adapter: Vec::new(),
        interpreter: None,
        program: PathBuf::from("Crash"),
        args: Vec::new(),
        cwd: root.clone(),
        env: BTreeMap::new(),
        breakpoints: BTreeMap::new(),
        prepare: String::new(),
        special: SpecialBreakpoints {
            uncaught: true,
            exception_condition: types.to_owned(),
            functions: vec![FunctionBreakpoint {
                name: "Crash.area".into(),
                condition: None,
                ..Default::default()
            }],
            ..SpecialBreakpoints::default()
        },
        attach: None,
        browser: None,
        remote: None,
    };
    // Every update until `pick` accepts one, within `secs`.
    fn wait<T>(
        c: &Controller,
        seen: &mut Vec<Update>,
        secs: u64,
        what: &str,
        mut pick: impl FnMut(&Update) -> Option<T>,
    ) -> T {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            for u in c.poll() {
                if let Update::Failed(e) = &u {
                    panic!("{what}: failed: {e}\n{seen:#?}");
                }
                let found = pick(&u);
                seen.push(u);
                if let Some(t) = found {
                    return t;
                }
            }
            assert!(Instant::now() < end, "{what}: not seen\n{seen:#?}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let stop = |u: &Update| match u {
        Update::Stopped { reason, frames, .. } => frames
            .first()
            .map(|f| (reason.clone(), f.name.clone(), f.line)),
        _ => None,
    };
    let refused = |seen: &[Update]| {
        seen.iter().any(
            |u| matches!(u, Update::Output(t) if t.contains("[IDECraft]") && t.contains("cannot")),
        )
    };

    let c = Controller::start(launch("java.lang.NullPointerException"));
    let mut seen = Vec::new();
    let (reason, frame, line) = wait(&c, &mut seen, 300, "the method breakpoint", stop);
    assert!(frame.contains("area"), "{reason} {frame}:{line}\n{seen:#?}");
    assert!(reason.contains("function"), "{reason}");
    c.send(Command::Continue);
    let (reason, frame, line) = wait(&c, &mut seen, 120, "the exception breakpoint", stop);
    assert!(reason.starts_with("exception"), "{reason} {frame}:{line}");
    assert!(frame.contains("main"), "{frame}");
    assert_eq!(line, 8, "the line that throws");
    assert!(!refused(&seen), "nothing is refused: {seen:#?}");
    c.send(Command::Stop);
    wait(&c, &mut seen, 60, "the end", |u| {
        matches!(u, Update::Exited(_)).then_some(())
    });

    // Limited to another type the NullPointerException does not stop the program.
    let c = Controller::start(launch("java.lang.IllegalStateException"));
    let mut seen = Vec::new();
    wait(&c, &mut seen, 300, "the method breakpoint", stop);
    c.send(Command::Continue);
    wait(&c, &mut seen, 120, "the end without a stop", |u| match u {
        Update::Exited(_) => Some(()),
        Update::Stopped { reason, .. } => panic!("stopped: {reason}"),
        _ => None,
    });
    assert!(!refused(&seen), "nothing is refused: {seen:#?}");
    let _ = std::fs::remove_dir_all(&root);
}

// --------------------------------------------------------------------------- hand test 110

const GREETER_RB: &str = "class Greeter\n  # Greets someone.\n  def greet(name)\n    \"Hello #{name}\"\n  end\nend\n\ngreeter = Greeter.new\ngreeter.greet(\"you\")\n";

/// Hand test 110 (`SPEC-0185`, `SPEC-0071` v6, `SPEC-0077` v2; D61) in the window with the real
/// Ruby language server: ⌘B on `Greeter` of `Greeter.new` goes to the class; F1 shows the class
/// with links as their text and the comment once. Skipped when no Ruby language server resolves
/// (start the test with the Ruby of `~/.local/ruby/bin` on `PATH`).
#[gpui::test]
fn hand_test_110_ruby_declaration_and_documentation(cx: &mut TestAppContext) {
    let root = workspace("110", &[("greeter.rb", GREETER_RB)]);
    if idecraft_languages::servers::resolve_for(idecraft_languages::LanguageId::Ruby, &root)
        .is_err()
    {
        eprintln!("skipped: no Ruby language server");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    idecraft_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("greeter.rb"));
    let at = GREETER_RB.find("Greeter.new").unwrap() + 2;

    // F1: the class, without raw Markdown links and with the comment once.
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("QuickJavaDoc", window, cx)
    });
    until(cx, 180, "the documentation opens", |cx| {
        bench
            .read_with(cx, |b, _| b.doc_blocks())
            .is_some_and(|d| !d.is_empty())
    });
    let shown = format!("{:?}", bench.read_with(cx, |b, _| b.doc_blocks()).unwrap());
    assert!(shown.contains("Greeter"), "{shown}");
    assert!(!shown.contains("file://"), "no raw link: {shown}");
    assert!(!shown.contains("]("), "no Markdown link syntax: {shown}");
    assert!(
        shown.matches("Greets someone.").count() <= 1,
        "the comment once: {shown}"
    );
    keys(cx, "escape");

    // ⌘B: to the class on line 1.
    editor.update(cx, |e, cx| e.select_range(at..at, cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("GotoDeclaration", window, cx)
    });
    until(cx, 180, "⌘B goes to the class", |cx| {
        editor.read_with(cx, |e, _| e.caret()) < GREETER_RB.find('\n').unwrap()
    });
    assert!(
        !status(&bench, cx).contains("Cannot find"),
        "{}",
        status(&bench, cx)
    );
    let _ = std::fs::remove_dir_all(&root);
}
