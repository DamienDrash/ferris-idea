//! The chosen theme for the whole application (`SPEC-0060` v5): read at start-up, switched from
//! the theme popup, saved for the next start.

use std::path::PathBuf;

use gpui::{App, Global};
use idecraft_ui::theme::ThemeId;

use crate::palette::set_current_theme;

/// The theme file in use; the self-test points it into its own folder.
pub struct ThemeFile(pub PathBuf);

impl Global for ThemeFile {}

/// The theme file: the one set as [`ThemeFile`], else the user's.
pub fn theme_file(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<ThemeFile>() {
        Some(f) => Some(f.0.clone()),
        None => idecraft_core::theme_file(),
    }
}

/// Draws with the saved theme; an unknown or missing name gives the default, `IDECraft Dark`
/// (`SPEC-0060` v6). A saved choice is never replaced.
pub fn apply_saved_theme(cx: &App) {
    // `appearance.theme` of the IDE settings, else the theme file of older versions (SPEC-0303
    // behaviour 14).
    let legacy = theme_file(cx);
    let saved = match ide_settings_file(cx) {
        Some(f) => idecraft_core::chosen_theme(&f, legacy.as_deref()),
        None => legacy.and_then(|f| idecraft_core::load_theme(&f)),
    };
    show_theme(ThemeId::from_saved(saved.as_deref()), cx);
}

/// The log that takes the notes of themes (`SPEC-0060` v10 behaviour 15). Only a process that
/// sets it writes the file: the application at start-up; tests and tools keep the notes in
/// [`crate::palette::Palette::notes`].
pub struct ThemeLog(pub PathBuf);

impl Global for ThemeLog {}

/// File name of the theme log in the log folder.
pub const THEME_LOG: &str = "theme.log";

/// Makes `theme` the one the views draw with and, the first time it is shown in this process,
/// writes what its files left out or spelled wrongly to the error output and the theme log
/// (behaviour 15). The window draws either way.
fn show_theme(theme: ThemeId, cx: &App) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NOTED: AtomicU32 = AtomicU32::new(0);
    set_current_theme(theme);
    let bit = 1u32 << ThemeId::ALL.iter().position(|t| *t == theme).unwrap_or(0);
    if NOTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    let notes = crate::palette::Palette::for_theme(theme).notes();
    let log = cx.try_global::<ThemeLog>().map(|l| l.0.clone());
    for line in theme_log_lines(notes, std::time::SystemTime::now()) {
        eprintln!(
            "note: {}",
            line.split_once(' ').map_or(line.as_str(), |(_, l)| l)
        );
        if let Some(log) = &log {
            let _ = idecraft_core::logs::append_line(log, &line);
        }
    }
}

/// The lines the theme log gets for `notes`: each with the time in front.
pub fn theme_log_lines(notes: &[String], time: std::time::SystemTime) -> Vec<String> {
    let stamp = idecraft_core::logs::utc_stamp(time);
    notes.iter().map(|n| format!("{stamp} {n}")).collect()
}

/// Switches every window to `theme` and saves the choice (behaviour 10). While the theme follows
/// the system it becomes the theme for the system's light or dark appearance, whichever it is
/// (`SPEC-0131` behaviour 3).
pub fn choose_theme(theme: ThemeId, cx: &mut App) -> std::io::Result<()> {
    let mut settings = ide_settings(cx);
    if settings.appearance.sync_with_os {
        if theme.is_dark() {
            settings.appearance.dark_theme = theme.name().into();
        } else {
            settings.appearance.light_theme = theme.name().into();
        }
        // The system's appearance decides which of the two is shown.
        return set_ide_settings(settings, cx);
    }
    show_theme(theme, cx);
    cx.refresh_windows();
    // Kept as `appearance.theme` in the IDE settings; the old theme file goes to `theme.bak`
    // (SPEC-0303 behaviour 14).
    settings.appearance.theme = Some(theme.name().into());
    cx.set_global(CurrentIdeSettings(settings.clone()));
    match ide_settings_file(cx) {
        Some(f) => {
            idecraft_core::save_ide_settings(&f, &settings)?;
            match theme_file(cx) {
                Some(old) => idecraft_core::format::retire(&old),
                None => Ok(()),
            }
        }
        None => Ok(()),
    }
}

/// The IDE settings file in use (`SPEC-0129`); the self-test points it into its own folder.
pub struct IdeSettingsFile(pub PathBuf);

impl Global for IdeSettingsFile {}

/// The IDE settings in effect.
pub struct CurrentIdeSettings(pub idecraft_core::IdeSettings);

impl Global for CurrentIdeSettings {}

/// The IDE settings file: the one set as [`IdeSettingsFile`], else the user's.
pub fn ide_settings_file(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<IdeSettingsFile>() {
        Some(f) => Some(f.0.clone()),
        None => idecraft_core::ide_settings_file(),
    }
}

/// The IDE settings in effect (the defaults before they were read).
pub fn ide_settings(cx: &App) -> idecraft_core::IdeSettings {
    cx.try_global::<CurrentIdeSettings>()
        .map(|s| s.0.clone())
        .unwrap_or_default()
}

/// Whether the folding arrows of open regions are drawn all the time (`SPEC-0083` v8 behaviour
/// 22); by default only while the pointer is over the editor.
pub fn fold_arrows_always(cx: &App) -> bool {
    cx.try_global::<CurrentIdeSettings>()
        .is_some_and(|s| s.0.editor.folding_arrows == idecraft_core::FoldingArrows::Always)
}

/// Reads the saved IDE settings and applies them (start-up).
pub fn apply_saved_settings(cx: &mut App) {
    let settings = ide_settings_file(cx)
        .map(|f| idecraft_core::load_ide_settings(&f))
        .unwrap_or_default();
    crate::fonts::set_editor_font(&settings);
    idecraft_languages::grazie::choose_dictionary(&settings.editor.spelling_dictionary);
    crate::palette::set_compact_mode(settings.appearance.compact_mode);
    // The editor colour scheme apart from the theme (SPEC-0337 behaviour 4).
    crate::palette::set_editor_scheme(&settings.appearance.editor_scheme);
    // Git over SSH to Azure DevOps and own ProxyCommands (ADR-0025 section 1a).
    idecraft_remote::set_azure_classic_kex(settings.git.ssh.allow_classic_kex_for_azure_devops);
    idecraft_remote::set_own_proxy_command(settings.ssh.allow_own_proxy_command);
    cx.set_global(CurrentIdeSettings(settings));
    follow_os(cx);
}

/// The theme for the system's appearance when the theme follows the system (`SPEC-0131`
/// behaviour 1): the chosen dark theme while the system is dark, the light one otherwise.
pub fn os_theme(settings: &idecraft_core::IdeSettings, dark: bool) -> Option<ThemeId> {
    let a = &settings.appearance;
    if !a.sync_with_os {
        return None;
    }
    let (name, fallback) = if dark {
        (&a.dark_theme, ThemeId::IdecraftDark)
    } else {
        (&a.light_theme, ThemeId::IdecraftLight)
    };
    Some(ThemeId::from_name(name).unwrap_or(fallback))
}

/// Whether the system shows a dark appearance.
pub fn os_is_dark(cx: &App) -> bool {
    matches!(
        cx.window_appearance(),
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    )
}

/// Switches to the theme for the system's appearance if the theme follows the system.
pub fn follow_os(cx: &mut App) {
    if let Some(t) = os_theme(&ide_settings(cx), os_is_dark(cx))
        && t != crate::palette::current_theme()
    {
        show_theme(t, cx);
        cx.refresh_windows();
    }
}

/// The theme the views show for `settings`: with Sync with OS the theme for the system's
/// appearance (`dark`), else the chosen one; none when nothing was chosen yet. Switching Sync with
/// OS off therefore shows the theme chosen without it again, at once, as the next start would
/// (`SPEC-0131` v4 behaviour 3a).
pub fn theme_for(settings: &idecraft_core::IdeSettings, dark: bool) -> Option<ThemeId> {
    os_theme(settings, dark).or_else(|| {
        let saved = settings.appearance.theme.as_deref()?;
        (!saved.trim().is_empty()).then(|| ThemeId::from_saved(Some(saved)))
    })
}

/// Follows the system's appearance while `window` is open (behaviour 2).
pub fn watch_os_appearance(window: &gpui::Window) {
    window
        .observe_window_appearance(|_, cx| follow_os(cx))
        .detach();
}

/// Applies `settings` to every window at once and saves them (`SPEC-0129` behaviour 1).
pub fn set_ide_settings(settings: idecraft_core::IdeSettings, cx: &mut App) -> std::io::Result<()> {
    let settings = settings.clamped();
    crate::fonts::set_editor_font(&settings);
    idecraft_languages::grazie::choose_dictionary(&settings.editor.spelling_dictionary);
    crate::palette::set_compact_mode(settings.appearance.compact_mode);
    crate::palette::set_editor_scheme(&settings.appearance.editor_scheme);
    idecraft_remote::set_azure_classic_kex(settings.git.ssh.allow_classic_kex_for_azure_devops);
    idecraft_remote::set_own_proxy_command(settings.ssh.allow_own_proxy_command);
    cx.set_global(CurrentIdeSettings(settings.clone()));
    // The theme for the system's appearance, or without syncing the chosen one (behaviour 3a).
    // A theme chosen in an older version is still in its own file (SPEC-0303 behaviour 14).
    let older = || {
        let name = theme_file(cx).and_then(|f| idecraft_core::load_theme(&f))?;
        (!settings.appearance.sync_with_os).then(|| ThemeId::from_saved(Some(&name)))
    };
    if let Some(t) = theme_for(&settings, os_is_dark(cx)).or_else(older)
        && t != crate::palette::current_theme()
    {
        show_theme(t, cx);
    }
    cx.refresh_windows();
    match ide_settings_file(cx) {
        Some(f) => idecraft_core::save_ide_settings(&f, &settings),
        None => Ok(()),
    }
}
