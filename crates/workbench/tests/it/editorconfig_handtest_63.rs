//! Hand test 63 (`SPEC-0137` v3 AK-7) in a window of the test harness with real keys: a project
//! whose `.editorconfig` asks for four spaces in `*.py`, a tab of width 8 in `Makefile`, CRLF in
//! `*.bat` and, for every file, trimmed line ends and a final line break. Tab and typing in each
//! file and ⌘S give the files byte for byte; a changed `.editorconfig`, saved, switches the open
//! editors at once. The comparison with the reference stays in `docs/research/PLAN-F081.md`.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{EditorView, WorkbenchView};

const EDITORCONFIG: &str = "root = true\n\n[*]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n\n[*.py]\nindent_style = space\nindent_size = 4\n\n[Makefile]\nindent_style = tab\ntab_width = 8\n\n[*.bat]\nend_of_line = crlf\n";

fn workspace() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("idecraft-editorconfig-63-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(".editorconfig"), EDITORCONFIG).unwrap();
    std::fs::write(root.join("main.py"), "def f():\n").unwrap();
    std::fs::write(root.join("Makefile"), "all:\n").unwrap();
    std::fs::write(root.join("run.bat"), "echo a  \necho b").unwrap();
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    // Settings of its own: the user's settings are never read or written; the IDE's own save
    // options are off, so what happens on saving comes from `.editorconfig`.
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

fn type_text(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn open(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    file: &Path,
) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open")
}

/// The caret to the end of the text (the last line, past its last character).
fn to_end(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    editor.update(cx, |e, cx| e.go_to(usize::MAX, usize::MAX, cx));
    cx.run_until_parked();
}

fn saved(root: &Path, name: &str) -> String {
    String::from_utf8(std::fs::read(root.join(name)).unwrap()).unwrap()
}

fn indent(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, usize) {
    editor.read_with(cx, |e, _| (e.state.indent.clone(), e.tab_width()))
}

#[gpui::test]
fn hand_test_63_editorconfig_in_the_window(cx: &mut TestAppContext) {
    let root = workspace();
    let (bench, cx) = window(&root, cx);

    // `*.py`: Tab gives four spaces; the trailing spaces of a line the caret left go on saving.
    let py = open(&bench, cx, &root.join("main.py"));
    assert_eq!(indent(&py, cx), ("    ".to_owned(), 4));
    to_end(&py, cx);
    keys(cx, "tab");
    type_text(cx, "x = 1  ");
    keys(cx, "enter");
    type_text(cx, "# end");
    keys(cx, "cmd-s");
    assert_eq!(saved(&root, "main.py"), "def f():\n    x = 1\n    # end\n");

    // `Makefile`: Tab is a tab, a tab is 8 columns wide; the final line break is added.
    let make = open(&bench, cx, &root.join("Makefile"));
    assert_eq!(indent(&make, cx), ("\t".to_owned(), 8));
    to_end(&make, cx);
    keys(cx, "tab");
    type_text(cx, "echo hi");
    keys(cx, "cmd-s");
    assert_eq!(saved(&root, "Makefile"), "all:\n\techo hi\n");

    // `*.bat`: every line break becomes CRLF, trailing spaces go, a final line break is added.
    let bat = open(&bench, cx, &root.join("run.bat"));
    to_end(&bat, cx);
    type_text(cx, "!");
    keys(cx, "cmd-s");
    assert_eq!(saved(&root, "run.bat"), "echo a\r\necho b!\r\n");

    // A changed `.editorconfig`, saved, switches the open Python editor at once.
    let ec = open(&bench, cx, &root.join(".editorconfig"));
    ec.update(cx, |e, cx| {
        e.set_text(
            &EDITORCONFIG.replace("indent_size = 4", "indent_size = 2"),
            cx,
        )
    });
    keys(cx, "cmd-s");
    assert_eq!(indent(&py, cx), ("  ".to_owned(), 2));
    let py = open(&bench, cx, &root.join("main.py"));
    to_end(&py, cx);
    keys(cx, "tab");
    type_text(cx, "x");
    keys(cx, "cmd-s");
    assert_eq!(
        saved(&root, "main.py"),
        "def f():\n    x = 1\n    # end\n  x\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
