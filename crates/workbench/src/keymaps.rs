//! The keymap in use for the whole application (`SPEC-0105`): the platform's bundled keymap with the
//! user's changes from their keymap file over it.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use ferris_idea_ui::keymap::Keystroke;
use ferris_idea_ui::keymap::{Keymap, KeymapFile, Shortcut};
use gpui::{App, Global};

/// The user keymap file in use; the self-test points it into its own folder.
pub struct UserKeymapFile(pub PathBuf);

impl Global for UserKeymapFile {}

/// The user keymap file: the one set as [`UserKeymapFile`], else the user's.
pub fn keymap_file(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<UserKeymapFile>() {
        Some(f) => Some(f.0.clone()),
        None => ferris_idea_core::keymap_file(),
    }
}

static CURRENT: RwLock<Option<Arc<Keymap>>> = RwLock::new(None);

fn install(keymap: Keymap) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(keymap));
}

/// The keymap the views use.
pub fn current() -> Arc<Keymap> {
    if let Some(k) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return k.clone();
    }
    install(Keymap::bundled_for_platform());
    current()
}

/// The first shortcut of `action` in the keymap in use, written for this system: `⌥F7` on
/// macOS, `Alt+F7` elsewhere; empty when the action has none (`SPEC-0028` v9 behaviour 14).
pub fn label(action: &str) -> String {
    current().shortcut_label(action, cfg!(target_os = "macos"))
}

/// A key in parentheses after a space, ` (⌥F7)`, or nothing for an empty key (`SPEC-0028` v9
/// behaviour 14).
pub fn in_parens(key: &str) -> String {
    if key.is_empty() {
        String::new()
    } else {
        format!(" ({key})")
    }
}

/// The user's changes: the keymap file, or an empty user keymap when there is none.
pub fn user_keymap(cx: &App) -> KeymapFile {
    KeymapFile::read_user(keymap_file(cx).as_deref())
}

/// Reads the user keymap file and uses it over the bundled keymap.
pub fn load(cx: &App) {
    install(Keymap::bundled_for_platform().with_overrides(&user_keymap(cx)));
}

/// Gives `action` exactly `shortcuts` (none removes its shortcuts), or with `None` restores the
/// bundled ones; saves the user keymap and uses it at once (behaviour 4).
pub fn set_shortcuts(
    action: &str,
    shortcuts: Option<Vec<Shortcut>>,
    cx: &App,
) -> std::io::Result<()> {
    let mut user = user_keymap(cx);
    user.set_action(action, shortcuts);
    if let Some(file) = keymap_file(cx) {
        user.write(&file)?;
    }
    install(Keymap::bundled_for_platform().with_overrides(&user));
    Ok(())
}

/// The keymap actions of a key pressed in a tree (`SPEC-0275` v13 behaviour 32): the actions of
/// the keystroke and, since the toolkit does not tell the numeric keypad from the main block, of
/// the keypad's key with the same sign (`-` is also `SUBTRACT`, `*` is `MULTIPLY`). `key` as the
/// toolkit names it; `meta` is ⌘ on macOS.
pub fn tree_actions_in(
    keymap: &Keymap,
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> Vec<String> {
    let mut strokes: Vec<Keystroke> = Keystroke::from_platform(key, ctrl, alt, shift, meta)
        .into_iter()
        .collect();
    let keypad = match key {
        "-" => Some("SUBTRACT"),
        "*" => Some("MULTIPLY"),
        _ => None,
    };
    // The modifiers in the keymap's order, taken from a keystroke with a key that always has a
    // name.
    if let (Some(keypad), Some(mut twin)) = (
        keypad,
        Keystroke::from_platform("a", ctrl, alt, shift, meta),
    ) {
        twin.key = keypad.to_owned();
        strokes.push(twin);
    }
    let mut out: Vec<String> = Vec::new();
    for stroke in &strokes {
        for id in keymap.actions_for(stroke, None) {
            if !out.iter().any(|o| o == id) {
                out.push(id.to_owned());
            }
        }
    }
    out
}

/// The tree key the keymap in use gives a key event: expanding, collapsing and expanding fully
/// (`SPEC-0275` v13 behaviour 32); `None` for every other key.
pub fn tree_action_key(k: &gpui::Keystroke) -> Option<crate::tree_nav::TreeKey> {
    let m = &k.modifiers;
    tree_actions_in(&current(), &k.key, m.control, m.alt, m.shift, m.platform)
        .iter()
        .find_map(|id| crate::tree_nav::action_key(id))
}

/// The actions whose mouse shortcut in the keymap in use is a single click of the primary button
/// with the modifiers `m` (`SPEC-0130` v2 behaviour 5).
pub fn primary_click_actions(m: &gpui::Modifiers) -> Vec<String> {
    click_actions(1, m)
}

/// The actions whose mouse shortcut in the keymap in use is a single click of `button` (1 the
/// primary, 2 the middle one) with the modifiers `m` (`SPEC-0130` v3 behaviour 6).
pub fn click_actions(button: u8, m: &gpui::Modifiers) -> Vec<String> {
    click_actions_in(&current(), button, m)
}

fn click_actions_in(keymap: &Keymap, button: u8, m: &gpui::Modifiers) -> Vec<String> {
    let held = ferris_idea_ui::keymap::MouseModifiers {
        ctrl: m.control,
        alt: m.alt,
        shift: m.shift,
        meta: m.platform,
    };
    keymap
        .mouse_actions(button, false, held)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0130 AK-6 (v3): the middle button alone means the column selection by dragging, ⌥⇧
    /// with it the rectangle from the caret, in the bundled macOS and default keymaps.
    #[test]
    fn middle_click_actions() {
        const DRAG: &str = "EditorCreateRectangularSelectionOnMouseDrag";
        const FROM_CARET: &str = "EditorCreateRectangularSelection";
        let alt_shift = gpui::Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let plain = click_actions_in(&keymap, 2, &gpui::Modifiers::default());
            assert!(plain.iter().any(|a| a == DRAG), "{plain:?}");
            assert!(!plain.iter().any(|a| a == FROM_CARET), "{plain:?}");
            let both = click_actions_in(&keymap, 2, &alt_shift);
            assert!(both.iter().any(|a| a == FROM_CARET), "{both:?}");
            assert!(!both.iter().any(|a| a == DRAG), "{both:?}");
            let primary = click_actions_in(&keymap, 1, &gpui::Modifiers::default());
            assert!(!primary.iter().any(|a| a == DRAG || a == FROM_CARET));
        }
    }
}
