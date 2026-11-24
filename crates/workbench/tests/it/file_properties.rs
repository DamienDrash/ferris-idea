//! The file property actions in a window of the test harness (`SPEC-0336` AK-8): converting
//! line separators as one undo step, the questions before a reload and a lossy conversion with
//! Cancel as their default, the editor following the read-only attribute, and what the status bar
//! shows.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::file_properties::{Charset, is_read_only};
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

/// A project with `files` (name, bytes) in a window; the first file is open.
fn open(
    tag: &str,
    files: &[(&str, &[u8])],
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<EditorView>,
    PathBuf,
    &'static mut VisualTestContext,
) {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-file-properties-window-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, bytes) in files {
        std::fs::write(root.join(name), bytes).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let file = root.join(files[0].0);
    let window = cx.add_window({
        let root = root.clone();
        move |_, cx| WorkbenchView::new(&root, cx)
    });
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open");
    (bench, editor, root, cx)
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

fn act(bench: &Entity<WorkbenchView>, id: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

fn note(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn answer(bench: &Entity<WorkbenchView>, i: usize, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.answer_drawn_question(i, window, cx));
    cx.run_until_parked();
}

fn buttons(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> (Vec<String>, usize) {
    bench.read_with(cx, |b, _| {
        let q = b.drawn_question().expect("a question is shown");
        (q.buttons.clone(), q.default)
    })
}

fn choose(bench: &Entity<WorkbenchView>, file: &Path, to: Charset, cx: &mut VisualTestContext) {
    bench.update(cx, |b, cx| b.change_encoding(file, to, cx));
    cx.run_until_parked();
}

/// SPEC-0336 AK-8, behaviours 7 to 9: an open editor's text is converted as one undo step and
/// not written; a file selected in the Project tree that is not open is rewritten on disk; a file
/// outside the project is refused.
#[gpui::test]
fn converting_separators_in_an_editor_is_one_undo_step(cx: &mut TestAppContext) {
    let (bench, editor, root, cx) = open(
        "separators",
        &[
            ("mixed.txt", b"one\r\ntwo\nthree\r\nlast"),
            ("closed.txt", b"a\nb\n"),
        ],
        cx,
    );
    let file = root.join("mixed.txt");
    assert_eq!(editor.read_with(cx, |e, _| e.status().line_ending), "CRLF");
    act(&bench, "ConvertToUnixLineSeparators", cx);
    assert_eq!(text(&editor, cx), "one\ntwo\nthree\nlast");
    assert_eq!(
        note(&bench, cx),
        "Line separators changed to LF: 2 line breaks in mixed.txt"
    );
    assert_eq!(editor.read_with(cx, |e, _| e.status().line_ending), "LF");
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"one\r\ntwo\nthree\r\nlast",
        "the file is written by the next save, not by the conversion"
    );
    // Once more: nothing to change.
    act(&bench, "ConvertToUnixLineSeparators", cx);
    assert_eq!(note(&bench, cx), "mixed.txt already has LF line separators");
    // One undo brings the mixed text back.
    editor.update(cx, |e, cx| {
        e.run_action("$Undo", cx);
    });
    assert_eq!(text(&editor, cx), "one\r\ntwo\nthree\r\nlast");
    // Classic CR is shown as CR.
    act(&bench, "ConvertToMacLineSeparators", cx);
    assert_eq!(text(&editor, cx), "one\rtwo\rthree\rlast");
    assert_eq!(editor.read_with(cx, |e, _| e.status().line_ending), "CR");
    assert_eq!(
        note(&bench, cx),
        "Line separators changed to CR: 3 line breaks in mixed.txt"
    );

    // The Project tree's selection: a file that is not open is rewritten on disk.
    let closed = root.join("closed.txt");
    bench.update_in(cx, |b, window, cx| {
        assert!(b.reveal_in_tree(&closed, cx));
        b.focus_tree(window, cx);
    });
    cx.run_until_parked();
    act(&bench, "ConvertToWindowsLineSeparators", cx);
    assert_eq!(
        note(&bench, cx),
        "Line separators changed to CRLF: 2 line breaks in closed.txt"
    );
    assert_eq!(std::fs::read(&closed).unwrap(), b"a\r\nb\r\n");
    assert_eq!(
        text(&editor, cx),
        "one\rtwo\rthree\rlast",
        "the editor's file is not the target"
    );

    // A file outside the project is not converted, open or not.
    let outside = root.with_extension("outside.txt");
    std::fs::write(&outside, "x\r\ny\r\n").unwrap();
    let outside = outside.canonicalize().unwrap();
    let name = outside.file_name().unwrap().to_string_lossy().into_owned();
    bench.update_in(cx, |b, window, cx| {
        b.open(&outside, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    act(&bench, "ConvertToUnixLineSeparators", cx);
    assert_eq!(
        note(&bench, cx),
        format!("Line separators of {name} were not changed: it is outside the project")
    );
    let other = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    assert_eq!(text(&other, cx), "x\r\ny\r\n");
    act(&bench, "ToggleReadOnlyAttribute", cx);
    assert_eq!(
        note(&bench, cx),
        format!("{name} was not changed: it is outside the project")
    );
    assert!(!is_read_only(&outside).unwrap());
    let _ = std::fs::remove_file(&outside);
}

/// SPEC-0336 AK-8, behaviours 12 and 13: the question offers Reload, Convert and Cancel with
/// Cancel as the default; a reload over unsaved changes asks again, and cancelling keeps them.
#[gpui::test]
fn reload_with_unsaved_changes_asks_and_cancel_keeps_them(cx: &mut TestAppContext) {
    let (bench, editor, root, cx) = open(
        "reload",
        &[("utf8.txt", "é\n".as_bytes()), ("plain.txt", b"abc\n")],
        cx,
    );
    let file = root.join("utf8.txt");
    assert_eq!(editor.read_with(cx, |e, _| e.status().encoding), "UTF-8");
    editor.update(cx, |e, cx| {
        assert!(e.replace_text("é!\n", cx));
    });
    choose(&bench, &file, Charset::Iso8859_1, cx);
    assert_eq!(
        buttons(&bench, cx),
        (vec!["Reload".into(), "Convert".into(), "Cancel".into()], 2)
    );
    // Reload: the unsaved "!" would be lost, so a second question; its default cancels.
    answer(&bench, 0, cx);
    assert_eq!(
        buttons(&bench, cx),
        (
            vec!["Reload and Discard Changes".into(), "Cancel".into()],
            1
        )
    );
    let on_return = bench.read_with(cx, |b, _| {
        b.drawn_question()
            .and_then(|q| q.answer_for_key("enter"))
            .expect("Return answers the question")
    });
    answer(&bench, on_return, cx);
    assert!(bench.read_with(cx, |b, _| b.drawn_question().is_none()));
    assert_eq!(text(&editor, cx), "é!\n");
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    assert_eq!(editor.read_with(cx, |e, _| e.status().encoding), "UTF-8");
    // Again, and this time through: the bytes on disk are read as ISO-8859-1.
    choose(&bench, &file, Charset::Iso8859_1, cx);
    answer(&bench, 0, cx);
    answer(&bench, 0, cx);
    assert_eq!(text(&editor, cx), "Ã©\n");
    assert!(!editor.read_with(cx, |e, _| e.is_modified()));
    assert_eq!(
        editor.read_with(cx, |e, _| e.status().encoding),
        "ISO-8859-1"
    );
    assert_eq!(note(&bench, cx), "utf8.txt was reloaded in ISO-8859-1");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        "é\n".as_bytes(),
        "nothing is written"
    );
    // And back: UTF-8 reads the same bytes as "é" again.
    choose(&bench, &file, Charset::Utf8, cx);
    answer(&bench, 0, cx);
    assert_eq!(text(&editor, cx), "é\n");

    // ASCII reads and writes the same in ISO-8859-1: set at once, no question.
    let plain = root.join("plain.txt");
    bench.update_in(cx, |b, window, cx| b.open(&plain, window, cx));
    cx.run_until_parked();
    choose(&bench, &plain, Charset::Iso8859_1, cx);
    assert!(bench.read_with(cx, |b, _| b.drawn_question().is_none()));
    assert_eq!(
        note(&bench, cx),
        "plain.txt is read and saved as ISO-8859-1 from now on"
    );
    let plain_editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    assert_eq!(
        plain_editor.read_with(cx, |e, _| e.status().encoding),
        "ISO-8859-1"
    );
    assert_eq!(std::fs::read(&plain).unwrap(), b"abc\n");
}

/// SPEC-0336 AK-8, behaviour 14: a conversion that loses characters asks a second time, Cancel
/// is the default, and without Convert Anyway nothing is written; with it the editor shows what
/// the file holds, and one undo brings the characters back.
#[gpui::test]
fn a_lossy_conversion_asks_twice_and_cancel_is_the_default(cx: &mut TestAppContext) {
    let (bench, editor, root, cx) = open("lossy", &[("words.txt", "naïve €\n".as_bytes())], cx);
    let file = root.join("words.txt");
    choose(&bench, &file, Charset::UsAscii, cx);
    // The bytes are not ASCII: no Reload.
    assert_eq!(
        buttons(&bench, cx),
        (vec!["Convert".into(), "Cancel".into()], 1)
    );
    answer(&bench, 0, cx);
    let (asked, default) = buttons(&bench, cx);
    assert_eq!(asked, ["Convert Anyway", "Cancel"]);
    assert_eq!(default, 1);
    let detail = bench.read_with(cx, |b, _| b.drawn_question().unwrap().detail.clone());
    assert!(
        detail.starts_with("2 characters cannot be written in US-ASCII (the first is 'ï', U+00EF)"),
        "{detail}"
    );
    answer(&bench, 1, cx);
    assert_eq!(std::fs::read(&file).unwrap(), "naïve €\n".as_bytes());
    assert_eq!(text(&editor, cx), "naïve €\n");
    assert_eq!(editor.read_with(cx, |e, _| e.status().encoding), "UTF-8");

    // Convert Anyway.
    choose(&bench, &file, Charset::UsAscii, cx);
    answer(&bench, 0, cx);
    answer(&bench, 0, cx);
    assert_eq!(std::fs::read(&file).unwrap(), b"na?ve ?\n");
    assert_eq!(text(&editor, cx), "na?ve ?\n");
    assert!(!editor.read_with(cx, |e, _| e.is_modified()));
    assert_eq!(editor.read_with(cx, |e, _| e.status().encoding), "US-ASCII");
    assert_eq!(
        note(&bench, cx),
        "words.txt was converted to US-ASCII: 2 characters were written as ?"
    );
    // One undo: the characters are back in the editor; saving them is refused, not lossy.
    editor.update(cx, |e, cx| {
        e.run_action("$Undo", cx);
    });
    assert_eq!(text(&editor, cx), "naïve €\n");
    editor.update(cx, |e, cx| {
        e.run_action("SaveAll", cx);
    });
    let said = editor.read_with(cx, |e, _| e.status().note.unwrap_or_default());
    assert!(
        said.contains("2 characters cannot be written in US-ASCII"),
        "{said}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"na?ve ?\n");
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
}

/// SPEC-0336 AK-8, behaviours 16 to 18: the toggle changes the file, saves a modified editor
/// first, and the editor takes no edit while its file is read-only.
#[gpui::test]
fn the_editor_follows_the_read_only_attribute(cx: &mut TestAppContext) {
    let (bench, editor, root, cx) = open(
        "read-only",
        &[("a.txt", b"one\n"), ("locked.txt", b"kept\n")],
        cx,
    );
    let file = root.join("a.txt");
    editor.update(cx, |e, cx| {
        assert!(e.replace_text("one two\n", cx));
    });
    act(&bench, "ToggleReadOnlyAttribute", cx);
    assert_eq!(note(&bench, cx), "a.txt is now read-only");
    assert!(is_read_only(&file).unwrap());
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"one two\n",
        "the unsaved change was saved first"
    );
    assert!(editor.read_with(cx, |e, _| e.is_read_only()));
    assert!(!editor.read_with(cx, |e, _| e.is_modified()));
    editor.update(cx, |e, cx| {
        assert!(!e.replace_text("changed\n", cx), "no edit is taken");
    });
    assert_eq!(text(&editor, cx), "one two\n");
    // A read-only file's separators are not converted.
    act(&bench, "ConvertToWindowsLineSeparators", cx);
    assert_eq!(
        note(&bench, cx),
        "Line separators of a.txt were not changed: it is read-only"
    );
    act(&bench, "ToggleReadOnlyAttribute", cx);
    assert_eq!(note(&bench, cx), "a.txt is now writable");
    assert!(!is_read_only(&file).unwrap());
    assert!(!editor.read_with(cx, |e, _| e.is_read_only()));
    editor.update(cx, |e, cx| {
        assert!(e.replace_text("changed\n", cx));
    });

    // A file that is read-only when it is opened: its editor is read-only from the start, and
    // follows a change made elsewhere when it looks at the disk.
    let locked = root.join("locked.txt");
    ferris_idea_core::file_properties::set_read_only(&root, &locked, true, false).unwrap();
    bench.update_in(cx, |b, window, cx| b.open(&locked, window, cx));
    cx.run_until_parked();
    let locked_editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    assert!(locked_editor.read_with(cx, |e, _| e.is_read_only()));
    ferris_idea_core::file_properties::set_read_only(&root, &locked, false, false).unwrap();
    locked_editor.update(cx, |e, cx| e.check_disk(cx));
    assert!(!locked_editor.read_with(cx, |e, _| e.is_read_only()));
}

/// SPEC-0336 AK-8, behaviours 6, 11, 12 and 15: the status bar's separator and encoding, the
/// list of encodings with the current one selected, and adding and removing the byte order mark.
#[gpui::test]
fn the_status_bar_shows_separator_and_encoding(cx: &mut TestAppContext) {
    let (bench, editor, root, cx) = open(
        "status",
        &[
            ("marked.txt", b"\xEF\xBB\xBFone\r\ntwo\r\n"),
            ("legacy.txt", &[b'c', b'a', b'f', 0xE9, b'\n']),
            ("image.bin", &[0x89, b'P', 0, 1]),
        ],
        cx,
    );
    let file = root.join("marked.txt");
    let status = editor.read_with(cx, |e, _| e.status());
    assert_eq!(
        (status.line_ending.as_str(), status.encoding.as_str()),
        ("CRLF", "UTF-8 BOM")
    );
    assert_eq!(
        text(&editor, cx),
        "one\r\ntwo\r\n",
        "the mark is not in the text"
    );
    // The list: Remove BOM first, then the encodings, UTF-8 selected.
    act(&bench, "ChangeFileEncodingAction", cx);
    let popup = bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the list of encodings");
    popup.read_with(cx, |p, _| {
        assert_eq!(p.title(), "File Encoding");
        assert_eq!(
            p.items(),
            [
                "Remove BOM",
                "UTF-8",
                "UTF-16LE",
                "UTF-16BE",
                "ISO-8859-1",
                "windows-1252",
                "US-ASCII"
            ]
        );
        assert_eq!(p.selected(), 1);
    });
    act(&bench, "RemoveBom", cx);
    assert_eq!(std::fs::read(&file).unwrap(), b"one\r\ntwo\r\n");
    assert_eq!(editor.read_with(cx, |e, _| e.status().encoding), "UTF-8");
    assert_eq!(note(&bench, cx), "marked.txt was converted to UTF-8");
    act(&bench, "RemoveBom", cx);
    assert_eq!(
        note(&bench, cx),
        "No byte order mark was removed from marked.txt: the file has no byte order mark"
    );
    act(&bench, "AddBom", cx);
    assert_eq!(std::fs::read(&file).unwrap(), b"\xEF\xBB\xBFone\r\ntwo\r\n");
    assert_eq!(
        editor.read_with(cx, |e, _| e.status().encoding),
        "UTF-8 BOM"
    );

    // A file that is not UTF-8 opens in windows-1252; a binary file does not open.
    let legacy = root.join("legacy.txt");
    bench.update_in(cx, |b, window, cx| b.open(&legacy, window, cx));
    cx.run_until_parked();
    let legacy_editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    assert_eq!(text(&legacy_editor, cx), "café\n");
    assert_eq!(
        legacy_editor.read_with(cx, |e, _| e.status().encoding),
        "windows-1252"
    );
    let binary = root.join("image.bin");
    bench.update_in(cx, |b, window, cx| b.open(&binary, window, cx));
    cx.run_until_parked();
    assert_eq!(
        note(&bench, cx),
        format!("{} was not opened: it is not a text file", binary.display())
    );
    assert_eq!(bench.read_with(cx, |b, _| b.active_path()), Some(legacy));
}
