//! Gaps of feature row F-012 closed after the check without a screen (2026-11-16), in a window of
//! the test harness with the macOS keymap and real key, modifier and mouse events: Clone Caret
//! Above/Below by ⌥ pressed twice and held (`SPEC-0025` v5 behaviour 9), a suggestion accepted at
//! several carets (`SPEC-0025` v5 behaviour 10) and the column selection with the middle button
//! (`SPEC-0130` v3 behaviour 6).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_languages::insight::CompletionItem;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, text: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-multi-caret-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("carets.txt"), text).unwrap();
    root.canonicalize().unwrap()
}

/// A window on `root` with `carets.txt` open and focused; settings of its own, so the user's
/// settings and keymap are never read or written.
fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Bench, Entity<EditorView>, &'static mut VisualTestContext) {
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
    let file = root.join("carets.txt");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    // One frame so the editor has painted lines to aim the pointer at.
    cx.update(|window, cx| window.draw(cx).clear());
    let editor = bench.read_with(cx, |b, _| {
        b.active_editor_entity().expect("carets.txt is open")
    });
    (bench, editor, cx)
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

/// The ranges of the carets, in document order.
fn parts(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<std::ops::Range<usize>> {
    editor.read_with(cx, |e, _| {
        e.state.carets.carets().iter().map(|c| c.range()).collect()
    })
}

/// A single caret or selection from `start` to `end`.
fn one(start: usize, end: usize) -> Vec<std::ops::Range<usize>> {
    vec![std::ops::Range { start, end }]
}

fn caret_at(editor: &Entity<EditorView>, cx: &mut VisualTestContext, offset: usize) {
    editor.update(cx, |e, cx| e.select_range(offset..offset, cx));
    cx.run_until_parked();
}

fn option(held: bool) -> Modifiers {
    Modifiers {
        alt: held,
        ..Default::default()
    }
}

/// The window position at the left edge of char `offset`, as last drawn.
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

const LINES: &str = "one\ntwo\nsix\nten\n";

/// SPEC-0025 AK-15 (v5, behaviour 9): ⌥ pressed twice and held, ↓ ↓ clones the caret onto the two
/// lines below; typing goes in at all three and one ⌘Z takes it back with the carets. After ⌥
/// was released, ⌥↑ is Extend Selection again; a single ⌥ press clones nothing.
#[gpui::test]
fn clone_caret_with_the_doubled_option_key(cx: &mut TestAppContext) {
    let root = workspace("clone", LINES);
    let (bench, editor, cx) = window(&root, cx);
    caret_at(&editor, cx, 1);

    // ⌥ down, up, down — held — then ↓ twice.
    cx.simulate_modifiers_change(option(true));
    cx.simulate_modifiers_change(option(false));
    cx.simulate_modifiers_change(option(true));
    keys(cx, "alt-down alt-down");
    assert_eq!(
        parts(&editor, cx),
        [1..1, 5..5, 9..9],
        "a caret on each of the next two lines, same column"
    );
    keys(cx, "X");
    assert_eq!(text(&bench, cx), "oXne\ntXwo\nsXix\nten\n");
    keys(cx, "cmd-z");
    assert_eq!(text(&bench, cx), LINES);
    assert_eq!(parts(&editor, cx).len(), 3, "undo keeps the three carets");

    // Released: ⌥↑ is Extend Selection, it selects the word at the caret and clones nothing.
    keys(cx, "escape");
    cx.simulate_modifiers_change(option(false));
    caret_at(&editor, cx, 13);
    cx.simulate_modifiers_change(option(true));
    keys(cx, "alt-up");
    assert_eq!(parts(&editor, cx), one(12, 15), "Extend Selection");

    // Pressed twice and held again: ↑ clones above, and the plain ⌥ press after it no longer.
    cx.simulate_modifiers_change(option(false));
    caret_at(&editor, cx, 13);
    cx.simulate_modifiers_change(option(true));
    cx.simulate_modifiers_change(option(false));
    cx.simulate_modifiers_change(option(true));
    keys(cx, "alt-up");
    assert_eq!(parts(&editor, cx), [9..9, 13..13]);
    cx.simulate_modifiers_change(option(false));
    keys(cx, "escape");
    cx.simulate_modifiers_change(option(true));
    keys(cx, "alt-down");
    assert_eq!(parts(&editor, cx).len(), 1, "one press is no gesture");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0025 AK-15 (v5, behaviour 10): with three carets after `ar` the suggestion `area`
/// accepted with Enter goes in at all three; the carets stand after it; one ⌘Z takes it back.
#[gpui::test]
fn a_suggestion_goes_in_at_every_caret(cx: &mut TestAppContext) {
    let text0 = "ar = 1\nar = 2\nar = 3\n";
    let root = workspace("completion", text0);
    let (bench, editor, cx) = window(&root, cx);
    editor.update(cx, |e, cx| e.select_ranges(&[2..2, 9..9, 16..16], cx));
    cx.run_until_parked();
    let item = CompletionItem {
        label: "area".into(),
        insert: "area".into(),
        kind: Some("variable".into()),
        ..Default::default()
    };
    bench.update_in(cx, |b, _, cx| {
        assert!(b.show_completion_items(vec![item], false, cx))
    });
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.completion_items()).is_some(),
        "the list is open"
    );
    keys(cx, "enter");
    assert_eq!(text(&bench, cx), "area = 1\narea = 2\narea = 3\n");
    assert_eq!(parts(&editor, cx), [4..4, 13..13, 22..22]);
    keys(cx, "cmd-z");
    assert_eq!(text(&bench, cx), text0, "one undo step");
    let _ = std::fs::remove_dir_all(&root);
}

const COLUMNS: &str = "alpha one\nbe\ngamma two\n";

/// SPEC-0130 AK-6 (v3, behaviour 6): a drag with the middle button selects the rectangle as an
/// ⌥-drag does; ⌥⇧ with a middle click selects the rectangle from the caret to the click.
#[gpui::test]
fn column_selection_with_the_middle_button(cx: &mut TestAppContext) {
    let root = workspace("middle", COLUMNS);
    let (bench, editor, cx) = window(&root, cx);

    // From "one" (line 0, column 6) to the end of "two" (line 2, column 9).
    let start = at_char(&editor, cx, 6);
    let end = at_char(&editor, cx, 22);
    cx.simulate_mouse_down(start, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Middle), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Middle, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        parts(&editor, cx),
        [6..9, 12..12, 19..22],
        "one part per line, a caret at the end of the short line"
    );
    keys(cx, "X");
    assert_eq!(text(&bench, cx), "alpha X\nbeX\ngamma X\n");
    keys(cx, "cmd-z");
    assert_eq!(text(&bench, cx), COLUMNS);

    // The caret at line 0 column 6, ⌥⇧ and a middle click at line 2 column 9.
    caret_at(&editor, cx, 6);
    let end = at_char(&editor, cx, 22);
    let alt_shift = Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    };
    cx.simulate_mouse_down(end, MouseButton::Middle, alt_shift);
    cx.simulate_mouse_up(end, MouseButton::Middle, alt_shift);
    cx.run_until_parked();
    assert_eq!(parts(&editor, cx), [6..9, 12..12, 19..22]);

    // Moving the pointer afterwards without a button changes nothing; a plain click leaves one
    // caret.
    let elsewhere = at_char(&editor, cx, 2);
    cx.simulate_mouse_move(elsewhere, None, Modifiers::none());
    assert_eq!(parts(&editor, cx).len(), 3);
    cx.simulate_mouse_down(elsewhere, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(elsewhere, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(parts(&editor, cx), one(2, 2));
    let _ = std::fs::remove_dir_all(&root);
}
