//! The font size actions in a window (`SPEC-0129` v4 AK-7): ⌃⇧. and ⌃⇧, through the window's key
//! dispatch change the editor font size of the settings in effect, of the editors and of the
//! settings file, and the status bar names the size or the limit. The editor font size is one
//! value for the whole process and the action writes the settings file, so this test has a
//! process and a settings file of its own; the user's settings are never read or written.
//!
//! Window tests need gpui's test support, which the workspace enables on macOS only.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::{
    self, IdeSettingsFile, THEME_LOG, ThemeFile, ThemeLog, ide_settings, set_ide_settings,
};
use idecraft_workbench::fonts::editor_font_size;

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "idecraft-font-size-actions-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

/// The size in the settings in effect, in the editors and in the settings file.
fn sizes(cx: &mut VisualTestContext, file: &Path) -> (f32, f32, Option<f32>) {
    let saved = file
        .exists()
        .then(|| idecraft_core::load_ide_settings(file).editor.font_size);
    (
        cx.update(|_, cx| ide_settings(cx).editor.font_size),
        editor_font_size(),
        saved,
    )
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| b.status_message(cx))
}

#[gpui::test]
fn font_size_keys_change_the_setting(cx: &mut TestAppContext) {
    let settings = folder("settings");
    let file = settings.join("settings.toml");
    let root = folder("project");
    std::fs::write(root.join("notes.txt"), "alpha\nbeta\n").unwrap();
    // The settings, the theme and the theme log of this test, before anything reads them.
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(ThemeLog(settings.join(THEME_LOG)));
        appearance::apply_saved_settings(cx);
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    assert_eq!(sizes(cx, &file), (13.0, 13.0, None), "the defaults");

    // Without an editor the action is not carried out.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("EditorIncreaseFontSizeGlobal", window, cx);
    });
    cx.run_until_parked();
    assert_eq!(sizes(cx, &file), (13.0, 13.0, None));

    let notes = root.join("notes.txt");
    bench.update_in(cx, |b, window, cx| {
        b.open(&notes, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();

    // One step up: in effect, in the editors, in the file, and named in the status bar.
    keys(cx, "ctrl-shift-.");
    assert_eq!(sizes(cx, &file), (14.0, 14.0, Some(14.0)));
    assert_eq!(status(&bench, cx).as_deref(), Some("Editor font size: 14"));
    // The key as a keyboard sends it: the character Shift types there.
    keys(cx, "ctrl->");
    assert_eq!(sizes(cx, &file), (15.0, 15.0, Some(15.0)));
    // And down again.
    keys(cx, "ctrl-shift-,");
    assert_eq!(sizes(cx, &file), (14.0, 14.0, Some(14.0)));
    assert_eq!(status(&bench, cx).as_deref(), Some("Editor font size: 14"));
    keys(cx, "ctrl-<");
    assert_eq!(sizes(cx, &file), (13.0, 13.0, Some(13.0)));
    // The text is as it was: the keys typed nothing.
    let text = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
        .read_with(cx, |e, _| e.state.buffer.text());
    assert_eq!(text, "alpha\nbeta\n");

    // At the limits nothing changes and the status bar names the limit.
    for (limit, key, word) in [
        (40.0, "ctrl-shift-.", "largest"),
        (8.0, "ctrl-shift-,", "smallest"),
    ] {
        cx.update(|_, cx| {
            let mut s = ide_settings(cx);
            s.editor.font_size = limit;
            set_ide_settings(s, cx).unwrap();
        });
        keys(cx, key);
        assert_eq!(sizes(cx, &file), (limit, limit, Some(limit)), "{key}");
        assert_eq!(
            status(&bench, cx),
            Some(format!("Editor font size: {limit} is the {word}")),
            "{key}"
        );
    }
    // One step back from a limit works.
    keys(cx, "ctrl-shift-.");
    assert_eq!(sizes(cx, &file), (9.0, 9.0, Some(9.0)));
}
