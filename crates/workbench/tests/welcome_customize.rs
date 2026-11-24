//! Customize, Plugins and the gear menu of the Welcome window (`SPEC-0355` AK-4): the theme, Sync
//! with OS, the editor scheme, the font size and compact mode change the settings in effect and
//! this test's own settings file; Configure…, Manage Plugins… and the gear open the Settings dialog
//! on their page. The chosen theme is one value for the whole process, so this test has a process
//! of its own; the user's settings are never read or written.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use ferris_idea_workbench::appearance::{IdeSettingsFile, ThemeFile, ide_settings};
use ferris_idea_workbench::keymaps::UserKeymapFile;
use ferris_idea_workbench::palette::{ThemeId, current_theme};
use ferris_idea_workbench::project_windows::{QuitsInHarness, RecentProjectsFile};
use ferris_idea_workbench::settings_model::Page;
use ferris_idea_workbench::welcome_model::{GearItem, Section};
use ferris_idea_workbench::welcome_view::{WelcomePluginsDir, show, welcome_window};
use gpui::TestAppContext;

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-welcome-customize-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    dir.canonicalize().unwrap()
}

/// SPEC-0355 AK-4 (behaviours 14 to 18, 21).
#[gpui::test]
fn customize_changes_the_settings_in_effect_and_their_file(cx: &mut TestAppContext) {
    let dir = folder();
    let file = dir.join("settings.toml");
    {
        let dir = dir.clone();
        let file = file.clone();
        cx.update(move |cx| {
            cx.set_global(RecentProjectsFile(dir.join("recent-projects")));
            cx.set_global(IdeSettingsFile(file));
            cx.set_global(ThemeFile(dir.join("theme")));
            cx.set_global(UserKeymapFile(dir.join("keymap.toml")));
            cx.set_global(WelcomePluginsDir(dir.join("plugins")));
            cx.set_global(QuitsInHarness::default());
        });
    }
    cx.update(show);
    cx.run_until_parked();
    let handle = cx
        .update(|cx| welcome_window(cx))
        .expect("the Welcome window");
    handle
        .update(cx, |view, _, cx| view.set_section(Section::Customize, cx))
        .unwrap();

    // The theme: chosen for every window at once and saved.
    handle
        .update(cx, |view, _, cx| {
            view.choose_theme(ThemeId::IslandsLight, cx)
        })
        .unwrap();
    assert_eq!(current_theme(), ThemeId::IslandsLight);
    let saved = ferris_idea_core::load_ide_settings(&file);
    assert_eq!(saved.appearance.theme.as_deref(), Some("Islands Light"));

    // Sync with OS, the editor scheme, the font size, compact mode.
    let before = cx.update(|cx| ide_settings(cx)).editor.font_size;
    handle
        .update(cx, |view, _, cx| {
            view.toggle_sync(cx);
            view.set_scheme("Dark", cx);
            view.step_font_size(true, cx);
            view.toggle_compact(cx);
        })
        .unwrap();
    let in_effect = cx.update(|cx| ide_settings(cx));
    assert!(in_effect.appearance.sync_with_os);
    assert_eq!(in_effect.appearance.editor_scheme, "Dark");
    assert_eq!(in_effect.editor.font_size, before + 1.0);
    assert!(in_effect.appearance.compact_mode);
    let saved = ferris_idea_core::load_ide_settings(&file);
    assert_eq!(
        saved, in_effect,
        "the same settings, in the test's own file"
    );
    handle
        .update(cx, |view, _, cx| {
            view.toggle_sync(cx);
            view.set_scheme("", cx);
            view.step_font_size(false, cx);
            view.toggle_compact(cx);
        })
        .unwrap();
    let back = cx.update(|cx| ide_settings(cx));
    assert!(!back.appearance.sync_with_os && !back.appearance.compact_mode);
    assert_eq!(back.editor.font_size, before);

    // Configure… opens Settings › Keymap.
    handle
        .update(cx, |view, window, cx| {
            view.open_settings(Page::Keymap, window, cx)
        })
        .unwrap();
    let page = handle
        .update(cx, |view, _, cx| {
            view.settings_dialog().map(|d| d.read(cx).page())
        })
        .unwrap();
    assert_eq!(page, Some(Page::Keymap));

    // The gear: All Settings… opens the dialog on Appearance, Plugins… on Plugins.
    for (item, wanted) in [
        (GearItem::AllSettings, Page::Appearance),
        (GearItem::Plugins, Page::Plugins),
    ] {
        handle
            .update(cx, |view, window, cx| view.gear_choose(item, window, cx))
            .unwrap();
        let page = handle
            .update(cx, |view, _, cx| {
                view.settings_dialog().map(|d| d.read(cx).page())
            })
            .unwrap();
        assert_eq!(page, Some(wanted), "{item:?}");
    }
    // Plugins: the test's empty folder, no plugin of the user's.
    let plugins = handle
        .update(cx, |view, _, _| view.plugins().len())
        .unwrap();
    assert_eq!(plugins, 0);
    let _ = std::fs::remove_dir_all(&dir);
}
