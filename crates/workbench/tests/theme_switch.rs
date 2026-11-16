//! Switching themes at application level (`SPEC-0060` v10 AK-16, `SPEC-0131` v4 AK-8): every
//! bundled theme is chosen, drawn with at once, saved and found again at the next start; Sync with
//! OS keeps one theme per kind and gives way to the chosen theme when it is switched off; a theme
//! whose files lack a colour is noted in the theme log once. The chosen theme is one value for the
//! whole process, so this test has a process of its own.
//!
//! Window tests need gpui's test support, which the workspace enables on macOS only.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::TestAppContext;
use idecraft_workbench::appearance::{
    self, IdeSettingsFile, THEME_LOG, ThemeFile, ThemeLog, ide_settings, os_is_dark, os_theme,
    set_ide_settings, theme_for,
};
use idecraft_workbench::palette::{Palette, ThemeId, current_theme, set_current_theme};

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idecraft-theme-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn saved(dir: &Path) -> Option<String> {
    idecraft_core::chosen_theme(&dir.join("settings.toml"), Some(&dir.join("theme")))
}

/// What a start of the application does with the saved files.
fn restart(cx: &mut gpui::App) {
    set_current_theme(ThemeId::DEFAULT);
    appearance::apply_saved_theme(cx);
    appearance::apply_saved_settings(cx);
}

#[gpui::test]
fn themes_switch_at_once_and_survive_a_restart(cx: &mut TestAppContext) {
    let dir = folder();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(ThemeLog(dir.join(THEME_LOG)));
        restart(cx);
        assert_eq!(current_theme(), ThemeId::IdecraftDark, "a new install");
        assert_eq!(saved(&dir), None);

        // Every bundled theme: drawn with at once, saved, found again at the next start.
        for theme in ThemeId::ALL {
            appearance::choose_theme(theme, cx).unwrap();
            assert_eq!(current_theme(), theme);
            let p = Palette::current();
            assert!(
                std::ptr::eq(p, Palette::for_theme(theme)),
                "{theme:?}: the views draw with its palette"
            );
            assert_eq!(p.dark, theme.is_dark(), "{theme:?}");
            assert_eq!(saved(&dir).as_deref(), Some(theme.name()));
            assert_eq!(
                ide_settings(cx).appearance.theme.as_deref(),
                Some(theme.name())
            );
            restart(cx);
            assert_eq!(current_theme(), theme, "{theme:?} after a restart");
        }
        // The loop ends with the last theme of the list.
        let chosen = ThemeId::Light;
        assert_eq!(current_theme(), chosen);

        // Sync with OS: the theme for the system's appearance, at once.
        let dark = os_is_dark(cx);
        let mut s = ide_settings(cx);
        s.appearance.sync_with_os = true;
        set_ide_settings(s, cx).unwrap();
        let of_kind = |dark: bool| {
            if dark {
                ThemeId::IdecraftDark
            } else {
                ThemeId::IdecraftLight
            }
        };
        assert_eq!(current_theme(), of_kind(dark));
        // Both appearances of the system have their theme.
        let s = ide_settings(cx);
        assert_eq!(os_theme(&s, true), Some(ThemeId::IdecraftDark));
        assert_eq!(os_theme(&s, false), Some(ThemeId::IdecraftLight));

        // A theme chosen while syncing becomes the theme of its kind; the other kind keeps its
        // own, and the theme chosen without syncing stays saved.
        appearance::choose_theme(ThemeId::IslandsDark, cx).unwrap();
        let s = ide_settings(cx);
        assert_eq!(s.appearance.dark_theme, "Islands Dark");
        assert_eq!(s.appearance.light_theme, "IDECraft Light");
        assert_eq!(os_theme(&s, true), Some(ThemeId::IslandsDark));
        assert_eq!(os_theme(&s, false), Some(ThemeId::IdecraftLight));
        appearance::choose_theme(ThemeId::IslandsLight, cx).unwrap();
        let s = ide_settings(cx);
        assert_eq!(s.appearance.dark_theme, "Islands Dark", "kept");
        assert_eq!(s.appearance.light_theme, "Islands Light");
        assert_eq!(s.appearance.theme.as_deref(), Some(chosen.name()));
        // What is shown is the system's choice between the two, whichever was chosen last.
        let shown = if dark {
            ThemeId::IslandsDark
        } else {
            ThemeId::IslandsLight
        };
        assert_eq!(current_theme(), shown);
        assert_eq!(theme_for(&s, true), Some(ThemeId::IslandsDark));
        assert_eq!(theme_for(&s, false), Some(ThemeId::IslandsLight));

        // The next start shows the same.
        restart(cx);
        assert_eq!(current_theme(), shown, "syncing after a restart");
        let s = ide_settings(cx);
        assert!(s.appearance.sync_with_os);
        assert_eq!(s.appearance.dark_theme, "Islands Dark");
        assert_eq!(s.appearance.light_theme, "Islands Light");

        // A name that is no theme gives the kind's default instead of a broken window.
        let mut odd = s.clone();
        odd.appearance.dark_theme = "Solarized".into();
        odd.appearance.light_theme = String::new();
        assert_eq!(os_theme(&odd, true), Some(ThemeId::IdecraftDark));
        assert_eq!(os_theme(&odd, false), Some(ThemeId::IdecraftLight));

        // Sync with OS switched off: the theme chosen without it is shown again at once, the same
        // one the next start shows.
        let mut s = ide_settings(cx);
        s.appearance.sync_with_os = false;
        assert_eq!(theme_for(&s, dark), Some(chosen));
        set_ide_settings(s, cx).unwrap();
        assert_eq!(current_theme(), chosen, "at once");
        restart(cx);
        assert_eq!(current_theme(), chosen, "and after a restart");
        // The themes of the two kinds are kept for the next time.
        let s = ide_settings(cx);
        assert_eq!(s.appearance.dark_theme, "Islands Dark");
        assert_eq!(s.appearance.light_theme, "Islands Light");

        // Another setting changed with a theme chosen does not touch the theme.
        let mut s = ide_settings(cx);
        s.editor.font_size = 16.0;
        set_ide_settings(s, cx).unwrap();
        assert_eq!(current_theme(), chosen);
        assert_eq!(saved(&dir).as_deref(), Some(chosen.name()));

        // A saved name that is no theme: the default, and the window draws.
        std::fs::write(
            dir.join("settings.toml"),
            "[appearance]\ntheme = \"Solarized\"\n",
        )
        .unwrap();
        restart(cx);
        assert_eq!(current_theme(), ThemeId::DEFAULT);
    });

    // The theme log: one line per colour a shown theme's files lack, written once however often
    // the theme was shown; complete themes write nothing.
    let log = std::fs::read_to_string(dir.join(THEME_LOG)).unwrap_or_default();
    let mut expected = 0;
    for theme in ThemeId::ALL {
        let notes = Palette::for_theme(theme).notes();
        expected += notes.len();
        for note in notes {
            assert_eq!(log.matches(note.as_str()).count(), 1, "{note} in:\n{log}");
        }
    }
    assert_eq!(log.lines().count(), expected, "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}
