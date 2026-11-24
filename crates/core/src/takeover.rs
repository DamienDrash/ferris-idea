//! Taking over what the former product name left behind (`SPEC-0356`).
//!
//! The product was renamed; its folders, environment variables and stored names change with it.
//! What older versions wrote stays readable: the settings folder and a project's folder are moved
//! to their new names once, old environment variables are read when the new one is not set, and
//! old theme and keymap names are read as today's.
//!
//! Every former name below is a constant on a line marked `rebrand:keep`, so the mechanical rename
//! of the technical names (rename step 2) leaves it as it is; the current names are the constants the
//! rename rewrites. Until that rename both are the same and every function here changes nothing.

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The settings folder's name under the configuration home (`~/.config`).
pub const CONFIG_DIR_NAME: &str = "ferris-idea";
/// The folder of a project's IDE state, at the project root.
pub const PROJECT_DIR: &str = ".ferris-idea";
/// The prefix of the product's environment variables.
pub const ENV_PREFIX: &str = "FERRIS_IDEA_";
/// The product's name in text.
const PRODUCT: &str = "Ferris IDEA";

const FORMER_CONFIG_DIR_NAME: &str = "idecraft"; // rebrand:keep
const FORMER_PROJECT_DIR: &str = ".idecraft"; // rebrand:keep
const FORMER_ENV_PREFIX: &str = "IDECRAFT_"; // rebrand:keep
const FORMER_PRODUCT: &str = "IDECraft"; // rebrand:keep

/// The brand themes (`SPEC-0060` v6) by today's names.
pub const BRAND_DARK_THEME: &str = "Ferris IDEA Dark";
/// See [`BRAND_DARK_THEME`].
pub const BRAND_LIGHT_THEME: &str = "Ferris IDEA Light";
const FORMER_DARK_THEME: &str = "IDECraft Dark"; // rebrand:keep
const FORMER_LIGHT_THEME: &str = "IDECraft Light"; // rebrand:keep

/// The name of the user keymap.
pub const USER_KEYMAP_NAME: &str = "Ferris IDEA";
const FORMER_USER_KEYMAP_NAME: &str = "IDECraft"; // rebrand:keep
const FORMER_KEYMAP_FILE: &str = "keymaps/IDECraft.toml"; // rebrand:keep
const FORMER_LEGACY_KEYMAP_FILE: &str = "keymaps/IDECraft.xml"; // rebrand:keep

/// The note left in a folder that was moved to its new name; while it is there, the folder is
/// never taken over again.
pub const NOTE_FILE: &str = "MOVED.txt";

/// What a takeover did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Takeover {
    /// Nothing to take over: no former folder, the new one exists, it was taken over before, or
    /// the names are the same.
    Nothing,
    /// The former folder was moved to the new name (and the note left in its place).
    Moved {
        /// Where it was.
        from: PathBuf,
        /// Where it is now.
        to: PathBuf,
    },
    /// The move failed; nothing was changed. The text says why, never a file's content.
    Failed(String),
}

impl Takeover {
    /// A line for the user, or `None` when nothing happened.
    pub fn message(&self) -> Option<String> {
        match self {
            Takeover::Nothing => None,
            Takeover::Moved { from, to } => Some(format!(
                "moved {} to {} (the product was renamed)",
                from.display(),
                to.display()
            )),
            Takeover::Failed(why) => Some(format!("could not take over the former folder: {why}")),
        }
    }
}

/// The value of the product's environment variable `name` (as [`std::env::var_os`]); while it is
/// not set, the variable of the same name with the former prefix. The new name wins when both are
/// set (`SPEC-0356` behaviour 6).
pub fn product_var_os(name: &str) -> Option<OsString> {
    var_os_in(name, |n| std::env::var_os(n))
}

/// As [`product_var_os`], with the result of [`std::env::var`].
pub fn product_var(name: &str) -> Result<String, std::env::VarError> {
    match product_var_os(name) {
        None => Err(std::env::VarError::NotPresent),
        Some(v) => v.into_string().map_err(std::env::VarError::NotUnicode),
    }
}

/// [`product_var_os`] over the variables `get` gives.
pub fn var_os_in(name: &str, get: impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    get(name).or_else(|| former_var_name(name).and_then(|old| get(&old)))
}

/// The former name of the product's variable `name`; `None` when it has none or the same.
pub fn former_var_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix(ENV_PREFIX)?;
    let old = format!("{FORMER_ENV_PREFIX}{rest}");
    (old != name).then_some(old)
}

/// Today's name for a theme or editor scheme saved under the former name (ignoring case); any
/// other name as it is (`SPEC-0356` behaviour 7).
pub fn current_theme_name(name: &str) -> Cow<'_, str> {
    let trimmed = name.trim();
    for (old, new) in [
        (FORMER_DARK_THEME, BRAND_DARK_THEME),
        (FORMER_LIGHT_THEME, BRAND_LIGHT_THEME),
    ] {
        if old != new && trimmed.eq_ignore_ascii_case(old) {
            return Cow::Borrowed(new);
        }
    }
    Cow::Borrowed(name)
}

/// Today's name for the user keymap saved under the former name; any other name as it is
/// (`SPEC-0356` behaviour 8).
pub fn current_keymap_name(name: &str) -> &str {
    if FORMER_USER_KEYMAP_NAME != USER_KEYMAP_NAME && name == FORMER_USER_KEYMAP_NAME {
        USER_KEYMAP_NAME
    } else {
        name
    }
}

/// The text of the note left in a moved folder.
fn note(to: &Path) -> String {
    format!(
        "{FORMER_PRODUCT} is now called {PRODUCT}.\n\n\
         The contents of this folder were moved to\n  {}\n\
         and are read only from there. Nothing was deleted. While this note is here, the folder is\n\
         not taken over again; you may delete the folder.\n",
        to.display()
    )
}

/// Moves the folder `former` to `current` once (`SPEC-0356` behaviours 1–4): only when `current`
/// does not exist (no file, folder or link of that name), `former` is a folder, and `former` holds
/// no [`NOTE_FILE`]. The move is a rename, so every file keeps its content, owner and mode and no
/// second copy of a key or password file is made. In its place a folder with the former folder's
/// mode is created, holding only the note. A failed rename changes nothing. Nothing is deleted
/// and nothing is overwritten.
pub fn take_over_folder(former: &Path, current: &Path) -> Takeover {
    if former == current {
        return Takeover::Nothing;
    }
    if std::fs::symlink_metadata(current).is_ok() {
        return Takeover::Nothing;
    }
    let Ok(meta) = std::fs::metadata(former) else {
        return Takeover::Nothing;
    };
    if !meta.is_dir() || std::fs::symlink_metadata(former.join(NOTE_FILE)).is_ok() {
        return Takeover::Nothing;
    }
    if let Err(e) = std::fs::rename(former, current) {
        // Another process may have moved it a moment ago.
        if std::fs::symlink_metadata(former).is_err() && current.is_dir() {
            return Takeover::Nothing;
        }
        return Takeover::Failed(format!(
            "{} -> {}: {e}",
            former.display(),
            current.display()
        ));
    }
    leave_note(former, current, &meta);
    Takeover::Moved {
        from: former.to_path_buf(),
        to: current.to_path_buf(),
    }
}

/// Recreates `former` with the mode it had and writes the note into it; failures are ignored,
/// since the data is already in its new place.
fn leave_note(former: &Path, current: &Path, meta: &std::fs::Metadata) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        let mode = meta.permissions().mode() & 0o7777;
        if std::fs::DirBuilder::new()
            .mode(0o700)
            .create(former)
            .is_err()
        {
            return;
        }
        let _ = std::fs::set_permissions(former, std::fs::Permissions::from_mode(mode));
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(former.join(NOTE_FILE))
        {
            use std::io::Write;
            let _ = f.write_all(note(current).as_bytes());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        if std::fs::create_dir(former).is_ok() {
            let _ = std::fs::write(former.join(NOTE_FILE), note(current));
        }
    }
}

/// The configuration home the settings folder lives in: `$XDG_CONFIG_HOME`, else `~/.config`.
pub(crate) fn config_home() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

/// Takes over the settings folder of the former name (`SPEC-0356` behaviour 1), then the user
/// keymap files inside it (behaviour 5). A folder named by the settings-folder variable is the
/// user's choice and is not moved. Run once at start-up, before anything reads or writes settings.
pub fn take_over_user_config() -> Takeover {
    // Builds run by Cargo (tests, `cargo run`) and the repository's test scripts set this, so no
    // test moves the user's own settings folder.
    if product_var_os(&format!("{ENV_PREFIX}NO_TAKEOVER")).is_some() {
        return Takeover::Nothing;
    }
    if product_var_os(&format!("{ENV_PREFIX}CONFIG_DIR")).is_some() {
        if let Some(dir) = crate::user_config_dir() {
            take_over_keymap_files(&dir);
        }
        return Takeover::Nothing;
    }
    let Some(home) = config_home() else {
        return Takeover::Nothing;
    };
    let current = home.join(CONFIG_DIR_NAME);
    let done = take_over_folder(&home.join(FORMER_CONFIG_DIR_NAME), &current);
    take_over_keymap_files(&current);
    done
}

/// Takes over the project folder of the former name at `root` (`SPEC-0356` behaviour 2). Run when
/// a project is opened, before anything reads or writes the project's IDE state.
pub fn take_over_project_folder(root: &Path) -> Takeover {
    take_over_folder(&root.join(FORMER_PROJECT_DIR), &root.join(PROJECT_DIR))
}

/// Takes over a folder named after the product inside `parent`, as the product keeps one in a Git
/// repository's own folder (`SPEC-0356` behaviour 2).
pub fn take_over_named_folder(parent: &Path) -> Takeover {
    take_over_folder(
        &parent.join(FORMER_CONFIG_DIR_NAME),
        &parent.join(CONFIG_DIR_NAME),
    )
}

/// Renames the user keymap files of the former name inside the settings folder `dir` to today's
/// names, each only when today's file does not exist (`SPEC-0356` behaviour 5).
pub fn take_over_keymap_files(dir: &Path) {
    let current = dir.join(crate::appearance::KEYMAP_FILE);
    let legacy = crate::legacy_keymap_file(&current);
    for (old, new) in [
        (dir.join(FORMER_KEYMAP_FILE), current),
        (dir.join(FORMER_LEGACY_KEYMAP_FILE), legacy),
    ] {
        if old == new || std::fs::symlink_metadata(&new).is_ok() {
            continue;
        }
        if std::fs::symlink_metadata(&old).is_ok_and(|m| m.is_file()) {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let d = std::env::temp_dir().join(format!(
                "takeover-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Self(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn set_mode(p: &Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    }

    /// SPEC-0356 AK-1: the former folder moves once, keeps every file and mode, and leaves the
    /// note in a folder of the same mode.
    #[cfg(unix)]
    #[test]
    fn the_former_folder_moves_once_with_its_modes() {
        let d = Dir::new("move");
        let old = d.0.join("old-name");
        let new = d.0.join("new-name");
        std::fs::create_dir_all(old.join("remote")).unwrap();
        std::fs::write(old.join("settings.toml"), "a = 1\n").unwrap();
        std::fs::write(old.join("remote/key"), "not-a-real-key").unwrap();
        set_mode(&old.join("remote/key"), 0o600);
        set_mode(&old.join("remote"), 0o700);
        set_mode(&old, 0o750);

        let done = take_over_folder(&old, &new);
        assert_eq!(
            done,
            Takeover::Moved {
                from: old.clone(),
                to: new.clone()
            }
        );
        assert!(done.message().unwrap().contains("new-name"));
        assert_eq!(
            std::fs::read_to_string(new.join("settings.toml")).unwrap(),
            "a = 1\n"
        );
        assert_eq!(mode(&new.join("remote/key")), 0o600);
        assert_eq!(mode(&new.join("remote")), 0o700);
        assert_eq!(mode(&new), 0o750);
        // The old place holds only the note, in a folder of the old mode.
        let left: Vec<_> = std::fs::read_dir(&old)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from(NOTE_FILE)]);
        assert_eq!(mode(&old), 0o750);
        assert_eq!(mode(&old.join(NOTE_FILE)), 0o600);
        let text = std::fs::read_to_string(old.join(NOTE_FILE)).unwrap();
        assert!(text.contains(&new.display().to_string()), "{text}");
        assert!(text.contains("Nothing was deleted"), "{text}");

        // Once: with the note there, a later run changes nothing, even when the new folder is
        // gone again.
        std::fs::rename(&new, d.0.join("elsewhere")).unwrap();
        std::fs::write(old.join("later"), "x").unwrap();
        assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        assert!(!new.exists());
        assert!(old.join("later").exists());
    }

    /// SPEC-0356 AK-2: an existing new folder (or file, or link) is never replaced, and the
    /// former folder stays as it was.
    #[test]
    fn an_existing_new_folder_is_never_overwritten() {
        let d = Dir::new("keep");
        let old = d.0.join("old");
        let new = d.0.join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("a"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("a"), "new").unwrap();
        assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        assert_eq!(std::fs::read_to_string(new.join("a")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old.join("a")).unwrap(), "old");
        assert!(!old.join(NOTE_FILE).exists());

        // An empty new folder counts as existing too.
        std::fs::remove_file(new.join("a")).unwrap();
        assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        // A file of the new name.
        std::fs::remove_dir(&new).unwrap();
        std::fs::write(&new, "file").unwrap();
        assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "file");
        // A dangling link of the new name.
        #[cfg(unix)]
        {
            std::fs::remove_file(&new).unwrap();
            std::os::unix::fs::symlink(d.0.join("nowhere"), &new).unwrap();
            assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        }
        assert_eq!(std::fs::read_to_string(old.join("a")).unwrap(), "old");
    }

    /// SPEC-0356 AK-2: no former folder, a former file, or the same name: nothing happens.
    #[test]
    fn nothing_to_take_over() {
        let d = Dir::new("nothing");
        let old = d.0.join("old");
        let new = d.0.join("new");
        assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        std::fs::write(&old, "a file").unwrap();
        assert_eq!(take_over_folder(&old, &new), Takeover::Nothing);
        assert!(!new.exists());
        std::fs::remove_file(&old).unwrap();
        std::fs::create_dir(&old).unwrap();
        assert_eq!(take_over_folder(&old, &old), Takeover::Nothing);
        assert!(!old.join(NOTE_FILE).exists());
    }

    /// SPEC-0356 AK-3: a former folder that is a link moves as the link; the target is untouched.
    #[cfg(unix)]
    #[test]
    fn a_linked_former_folder_moves_as_the_link() {
        let d = Dir::new("link");
        let target = d.0.join("dotfiles");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("settings.toml"), "b = 2\n").unwrap();
        let old = d.0.join("old");
        let new = d.0.join("new");
        std::os::unix::fs::symlink(&target, &old).unwrap();
        assert!(matches!(
            take_over_folder(&old, &new),
            Takeover::Moved { .. }
        ));
        assert!(
            std::fs::symlink_metadata(&new)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(new.join("settings.toml")).unwrap(),
            "b = 2\n"
        );
        assert!(old.join(NOTE_FILE).is_file());
        assert!(!target.join(NOTE_FILE).exists());
    }

    /// SPEC-0356 AK-1: the project folder of the former name is taken over at the project root.
    #[test]
    fn the_project_folder_is_taken_over() {
        let d = Dir::new("project");
        let old = d.0.join(FORMER_PROJECT_DIR);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("run.toml"), "[[run]]\n").unwrap();
        let done = take_over_project_folder(&d.0);
        if FORMER_PROJECT_DIR == PROJECT_DIR {
            assert_eq!(done, Takeover::Nothing);
            assert!(!old.join(NOTE_FILE).exists());
        } else {
            assert!(matches!(done, Takeover::Moved { .. }));
            assert!(old.join(NOTE_FILE).is_file());
        }
        assert!(d.0.join(PROJECT_DIR).join("run.toml").is_file());
    }

    /// SPEC-0356 AK-4: keymap files of the former name are renamed inside the settings folder,
    /// never over today's file.
    #[test]
    fn keymap_files_take_todays_names() {
        let d = Dir::new("keymaps");
        std::fs::create_dir_all(d.0.join("keymaps")).unwrap();
        std::fs::write(d.0.join(FORMER_KEYMAP_FILE), "old toml").unwrap();
        std::fs::write(d.0.join(FORMER_LEGACY_KEYMAP_FILE), "old xml").unwrap();
        take_over_keymap_files(&d.0);
        let current = d.0.join(crate::appearance::KEYMAP_FILE);
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "old toml");
        assert_eq!(
            std::fs::read_to_string(crate::legacy_keymap_file(&current)).unwrap(),
            "old xml"
        );
        if FORMER_KEYMAP_FILE != crate::appearance::KEYMAP_FILE {
            // Today's file wins over a former one written later.
            std::fs::write(d.0.join(FORMER_KEYMAP_FILE), "older").unwrap();
            take_over_keymap_files(&d.0);
            assert_eq!(std::fs::read_to_string(&current).unwrap(), "old toml");
            assert!(d.0.join(FORMER_KEYMAP_FILE).exists());
        }
    }

    /// SPEC-0356 AK-5: a former variable name maps to the same suffix; other names have none.
    #[test]
    fn former_variable_names() {
        let name = format!("{ENV_PREFIX}CONFIG_DIR");
        let former = former_var_name(&name);
        if ENV_PREFIX == FORMER_ENV_PREFIX {
            assert_eq!(former, None);
        } else {
            assert_eq!(former, Some(format!("{FORMER_ENV_PREFIX}CONFIG_DIR")));
        }
        assert_eq!(former_var_name("HOME"), None);
    }

    /// SPEC-0356 AK-5: the new variable wins; the former one is read while the new one is unset.
    #[test]
    fn the_new_variable_wins_over_the_former_one() {
        let new = format!("{ENV_PREFIX}PLUGINS");
        let old = format!("{FORMER_ENV_PREFIX}PLUGINS");
        let env = |pairs: &[(&str, &str)]| {
            let map: std::collections::HashMap<String, OsString> = pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
                .collect();
            move |n: &str| map.get(n).cloned()
        };
        assert_eq!(var_os_in(&new, env(&[(&old, "old")])), Some("old".into()));
        assert_eq!(var_os_in(&new, env(&[(&new, "new")])), Some("new".into()));
        // Both set: the new one wins (the map keeps the last pair for one name).
        assert_eq!(
            var_os_in(&new, env(&[(&old, "old"), (&new, "new")])),
            Some("new".into())
        );
        assert_eq!(var_os_in(&new, env(&[])), None);
        assert_eq!(var_os_in("HOME", env(&[(&old, "old")])), None);
    }

    /// SPEC-0356 AK-6: IDE settings saved with the brand themes' former names load with today's.
    #[test]
    fn saved_theme_names_load_as_todays() {
        let d = Dir::new("ide-settings");
        let file = d.0.join("ide-settings.toml");
        std::fs::write(
            &file,
            format!(
                "[appearance]\ntheme = \"{FORMER_DARK_THEME}\"\nsync_with_os = true\n\
                 dark_theme = \"{FORMER_DARK_THEME}\"\nlight_theme = \"{FORMER_LIGHT_THEME}\"\n\
                 editor_scheme = \"{FORMER_LIGHT_THEME}\"\n"
            ),
        )
        .unwrap();
        let a = crate::load_ide_settings(&file).appearance;
        assert_eq!(a.theme.as_deref(), Some(BRAND_DARK_THEME));
        assert_eq!(a.dark_theme, BRAND_DARK_THEME);
        assert_eq!(a.light_theme, BRAND_LIGHT_THEME);
        assert_eq!(a.editor_scheme, BRAND_LIGHT_THEME);
        // The theme file of older versions too.
        std::fs::write(d.0.join("theme"), FORMER_LIGHT_THEME).unwrap();
        assert_eq!(
            crate::load_theme(&d.0.join("theme")).as_deref(),
            Some(BRAND_LIGHT_THEME)
        );
    }

    /// SPEC-0356 AK-6: theme and keymap names saved under the former name read as today's.
    #[test]
    fn former_theme_and_keymap_names_read_as_todays() {
        assert_eq!(current_theme_name(FORMER_DARK_THEME), BRAND_DARK_THEME);
        assert_eq!(
            current_theme_name(&FORMER_LIGHT_THEME.to_lowercase()).to_lowercase(),
            BRAND_LIGHT_THEME.to_lowercase()
        );
        assert_eq!(current_theme_name("Islands Dark"), "Islands Dark");
        assert_eq!(
            current_keymap_name(FORMER_USER_KEYMAP_NAME),
            USER_KEYMAP_NAME
        );
        assert_eq!(current_keymap_name("macOS"), "macOS");
    }
}
