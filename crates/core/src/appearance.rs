//! The chosen theme (`SPEC-0060` v5) and where the user's keymap lives (`SPEC-0105`).
//!
//! Since `SPEC-0303` the theme is `appearance.theme` in the IDE settings; the `theme` file of
//! older versions (its name on one line) is read while the settings name none, and retired to
//! `theme.bak` when a theme is chosen. The keymap is `keymaps/FerrisIDEA.toml`; older versions kept
//! `keymaps/FerrisIDEA.xml`.

use std::path::{Path, PathBuf};

use crate::user_config_dir;

/// Where older versions kept the choice: `theme` in [`user_config_dir`].
pub fn theme_file() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join("theme"))
}

/// The user's keymap file inside [`user_config_dir`] (`SPEC-0303` behaviour 14).
pub(crate) const KEYMAP_FILE: &str = "keymaps/FerrisIDEA.toml";

/// Where the user's keymap is kept: `keymaps/FerrisIDEA.toml` in [`user_config_dir`]
/// (`SPEC-0303` behaviour 14).
pub fn keymap_file() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join(KEYMAP_FILE))
}

/// The keymap file older versions wrote beside `file`: the same name with `.xml`, in the
/// reference's keymap format.
pub fn legacy_keymap_file(file: &Path) -> PathBuf {
    file.with_extension("xml")
}

/// The theme name saved in the old-style `file`, if any.
pub fn load_theme(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let name = text.trim();
    // A name saved under the product's former name reads as today's (`SPEC-0356` behaviour 7).
    (!name.is_empty()).then(|| crate::takeover::current_theme_name(name).into_owned())
}

/// The chosen theme: `appearance.theme` of the settings file, else the old `legacy` theme file
/// (`SPEC-0303` behaviour 14).
pub fn chosen_theme(settings_file: &Path, legacy: Option<&Path>) -> Option<String> {
    crate::load_ide_settings(settings_file)
        .appearance
        .theme
        .filter(|t| !t.trim().is_empty())
        .or_else(|| legacy.and_then(load_theme))
}

/// Saves `name` as `appearance.theme` in the settings file and retires the old theme file to
/// `theme.bak` (once).
pub fn save_chosen_theme(
    settings_file: &Path,
    legacy: Option<&Path>,
    name: &str,
) -> std::io::Result<()> {
    let mut settings = crate::load_ide_settings(settings_file);
    settings.appearance.theme = Some(name.to_owned());
    crate::save_ide_settings(settings_file, &settings)?;
    match legacy {
        Some(old) => crate::format::retire(old),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0303 AK-5: the old theme file is read until a theme is chosen, then kept as `.bak`.
    #[test]
    fn the_theme_moves_into_the_settings() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let settings = dir.join("settings.toml");
        let old = dir.join("theme");
        assert_eq!(chosen_theme(&settings, Some(&old)), None);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&old, "Light\n").unwrap();
        assert_eq!(
            chosen_theme(&settings, Some(&old)).as_deref(),
            Some("Light")
        );
        std::fs::write(&old, "  \n").unwrap();
        assert_eq!(chosen_theme(&settings, Some(&old)), None);
        std::fs::write(&old, "Light\n").unwrap();
        save_chosen_theme(&settings, Some(&old), "Ferris IDEA Light").unwrap();
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("theme.bak")).unwrap(),
            "Light\n"
        );
        assert_eq!(
            chosen_theme(&settings, Some(&old)).as_deref(),
            Some("Ferris IDEA Light")
        );
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(
            text.starts_with("[ferris-idea]\nkind = \"ide-settings\"\nformat = 1\n"),
            "{text}"
        );
        assert!(text.contains("theme = \"Ferris IDEA Light\""));
        let _ = std::fs::remove_dir_all(dir);
    }
}
