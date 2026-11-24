//! Where the carets of added occurrences stand (`SPEC-0025` v3 behaviour 4a, OBS-0053 C1), in a
//! window of the test harness, by keys only with the macOS keymap: ⌃G, ⌃⇧G and ⌃⌘G from a caret
//! at the start, at the end and inside a word, and from a selection made backwards; ⌃⌘G from an
//! empty caret matches whole words and a rectangle pasted at one caret is indented to its column
//! (`SPEC-0025` v4, OBS-0058 M1 and R1).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const TEXT: &str = "foo one foo\nbar foo two\nfoo three\nabc\ndef\nghi\n";

fn workspace() -> PathBuf {
    workspace_with("carets", TEXT)
}

fn workspace_with(tag: &str, text: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-occurrence-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("caret.txt"), text).unwrap();
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<EditorView>,
    &'static mut VisualTestContext,
) {
    // Settings of its own: the user's settings and keymap are never read or written.
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
    let file = root.join("caret.txt");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench.read_with(cx, |b, _| {
        b.active_editor_entity().expect("caret.txt is open")
    });
    (bench, editor, cx)
}

/// The carets as (anchor, head), in document order.
fn carets(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
    editor.read_with(cx, |e, _| {
        e.state
            .carets
            .carets()
            .iter()
            .map(|c| (c.anchor, c.head))
            .collect()
    })
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

/// SPEC-0025 v3 AK-9: from the caret at offset 0, ⌃G three times selects three `foo` with every
/// caret at the start of its `foo`, ⌃⇧G takes the last away, ⌃⌘G selects all four with the carets
/// at their starts; from the end of a word and from inside it the carets stand at the ends; a
/// selection made backwards gives carets at the starts.
#[gpui::test]
fn carets_keep_their_place_in_the_occurrence(cx: &mut TestAppContext) {
    let root = workspace();
    let (_bench, editor, cx) = window(&root, cx);
    // ⌘↖ (Home) goes to the start of the text, as in step 1 of OBS-0053.
    keys(cx, "right cmd-home");
    assert_eq!(carets(&editor, cx), [(0, 0)]);

    keys(cx, "ctrl-g");
    assert_eq!(
        carets(&editor, cx),
        [(3, 0)],
        "the word at the caret, the caret at its start"
    );
    keys(cx, "ctrl-g ctrl-g");
    assert_eq!(carets(&editor, cx), [(3, 0), (11, 8), (19, 16)]);
    keys(cx, "ctrl-shift-g");
    assert_eq!(carets(&editor, cx), [(3, 0), (11, 8)]);

    keys(cx, "escape cmd-home ctrl-cmd-g");
    assert_eq!(carets(&editor, cx), [(3, 0), (11, 8), (19, 16), (27, 24)]);

    // From the end of the word (⌥→ from the start): every caret at the end of its foo.
    keys(cx, "escape cmd-home alt-right");
    assert_eq!(carets(&editor, cx), [(3, 3)]);
    keys(cx, "ctrl-g ctrl-g");
    assert_eq!(carets(&editor, cx), [(0, 3), (8, 11)]);

    // From inside the word: at the ends (Ferris IDEA's choice, not observed).
    keys(cx, "escape cmd-home right ctrl-cmd-g");
    assert_eq!(carets(&editor, cx), [(0, 3), (8, 11), (16, 19), (24, 27)]);

    // A selection made backwards: the added carets at the starts.
    keys(
        cx,
        "escape cmd-home alt-right shift-left shift-left shift-left",
    );
    assert_eq!(carets(&editor, cx), [(3, 0)]);
    keys(cx, "ctrl-g");
    assert_eq!(carets(&editor, cx), [(3, 0), (11, 8)]);
    let _ = std::fs::remove_dir_all(&root);
}

fn editor_text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

/// SPEC-0025 v4 AK-12 (OBS-0058 M1, R1): in `x xz x` ⌃⌘G from an empty caret gives two carets,
/// not three; a rectangle copied in column mode and pasted at the end of the first line puts its
/// first line after the caret and the others on new lines below, indented to the caret's column,
/// as one undo step.
#[gpui::test]
fn whole_words_and_a_pasted_rectangle(cx: &mut TestAppContext) {
    let root = workspace_with("mc", "x xz x\nab\ncd\nef\n");
    let (_bench, editor, cx) = window(&root, cx);
    keys(cx, "right cmd-home ctrl-cmd-g");
    assert_eq!(carets(&editor, cx), [(1, 0), (6, 5)], "not the x in xz");
    keys(cx, "escape cmd-home ctrl-g ctrl-g ctrl-g");
    assert_eq!(carets(&editor, cx).len(), 2, "⌃G the same way");

    // Column mode (⇧⌘8): `a`, `c`, `e` from three lines, copied.
    keys(
        cx,
        "escape cmd-home down shift-cmd-8 shift-right shift-down shift-down",
    );
    assert_eq!(carets(&editor, cx), [(7, 8), (10, 11), (13, 14)]);
    keys(cx, "cmd-c shift-cmd-8 escape cmd-home cmd-right");
    assert_eq!(carets(&editor, cx), [(6, 6)]);
    keys(cx, "cmd-v");
    assert_eq!(
        editor_text(&editor, cx),
        "x xz xa\n      c\n      e\nab\ncd\nef\n"
    );
    keys(cx, "cmd-z");
    assert_eq!(editor_text(&editor, cx), "x xz x\nab\ncd\nef\n");
    let _ = std::fs::remove_dir_all(&root);
}
