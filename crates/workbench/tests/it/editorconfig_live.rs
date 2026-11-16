//! `SPEC-0137` v2 behaviours 2a and 2b (OBS-0067 E1, E2, IMP-0413), in the harness's window: an
//! open file follows an `.editorconfig` that another program created, changed or deleted, without
//! reopening the file: Tab inserts the new indentation, the status bar names it and the right
//! margin moves to `max_line_length`. Reload All from Disk (Synchronize) reads it at once. Until v2
//! the open file kept the values it was opened with, and `max_line_length` drew no margin.

#![cfg(target_os = "macos")]

use gpui::{Entity, TestAppContext, VisualTestContext};
use idecraft_workbench::{EditorView, WorkbenchView};

/// Runs timers and the look at the disk until `done` holds; panics after 200 rounds.
fn wait(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..200 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{what}");
}

/// The open file's text, its status bar indentation and its margin column.
fn state(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, String, Option<u32>) {
    editor.read_with(cx, |e, _| {
        (
            e.state.buffer.text(),
            e.status().indent,
            e.right_margin_column(),
        )
    })
}

/// What Tab inserts on the empty line below `x` and after the `x` in column 1, each typed into
/// the open file; the text is put back to `x` afterwards. Replacing the whole text keeps the file's
/// indentation: the status bar names the same unit before and after.
fn tab_inserts(
    bench: &Entity<WorkbenchView>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (String, String) {
    let indent = state(editor, cx).1;
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    let tab_at = |text: &str, line: usize, column: usize, cx: &mut VisualTestContext| {
        editor.update(cx, |e, cx| {
            e.set_text(text, cx);
            e.go_to(line, column, cx);
        });
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        let typed = editor.read_with(cx, |e, _| e.state.buffer.text());
        typed.strip_prefix(text).unwrap_or(&typed).to_owned()
    };
    let empty_line = tab_at("x\n", 1, 0, cx);
    let after_text = tab_at("x", 0, 1, cx);
    editor.update(cx, |e, cx| e.set_text("x", cx));
    assert_eq!(
        state(editor, cx).1,
        indent,
        "replacing the text keeps the file's indentation"
    );
    (empty_line, after_text)
}

#[gpui::test]
fn an_open_file_follows_its_editorconfig_on_disk(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("idecraft-ec-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let config = root.join(".editorconfig");
    std::fs::write(
        &config,
        "root = true\n\n[*]\nindent_style = space\nindent_size = 2\nmax_line_length = 20\n",
    )
    .unwrap();
    let file = root.join("sub/a.txt");
    std::fs::write(&file, "x").unwrap();
    cx.executor().allow_parking();
    let dir = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&dir, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    bench.update_in(cx, |b, window, cx| b.open(&file, window, cx));
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, cx| b.editor_for(&file, cx))
        .expect("the file is open");

    // As opened: two spaces, the margin after column 20 (behaviour 2a).
    let (_, indent, margin) = state(&editor, cx);
    assert_eq!((indent.as_str(), margin), ("  ", Some(20)));
    // One whole unit on an empty line, the next stop after text (column 1 → 2).
    assert_eq!(tab_inserts(&bench, &editor, cx), ("  ".into(), " ".into()));

    // Changed by another program: the next look at the disk applies it (behaviour 2b).
    std::fs::write(
        &config,
        "root = true\n\n[*]\nindent_style = space\nindent_size = 6\nmax_line_length = 50\n",
    )
    .unwrap();
    wait(
        cx,
        "the changed .editorconfig reaches the open file",
        |cx| {
            let (_, indent, margin) = state(&editor, cx);
            indent == "      " && margin == Some(50)
        },
    );
    assert_eq!(state(&editor, cx).0, "x", "the text is not touched");
    assert_eq!(
        tab_inserts(&bench, &editor, cx),
        ("      ".into(), "     ".into())
    );

    // A nearer file created by another program wins; `off` draws no margin.
    let nearer = root.join("sub/.editorconfig");
    std::fs::write(
        &nearer,
        "[*.txt]\nindent_style = tab\nmax_line_length = off\n",
    )
    .unwrap();
    wait(cx, "a created .editorconfig reaches the open file", |cx| {
        let (_, indent, margin) = state(&editor, cx);
        indent == "\t" && margin.is_none()
    });
    assert_eq!(tab_inserts(&bench, &editor, cx), ("\t".into(), "\t".into()));

    // Deleted again, and read at once by Reload All from Disk (no wait for the look at the disk).
    std::fs::remove_file(&nearer).unwrap();
    bench.update(cx, |b, cx| b.synchronize(cx));
    let (_, indent, margin) = state(&editor, cx);
    assert_eq!((indent.as_str(), margin), ("      ", Some(50)));

    let _ = std::fs::remove_dir_all(&root);
}
