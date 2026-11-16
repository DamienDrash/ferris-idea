//! Select All, Copy, Cut, Paste, Undo and Redo in the Settings dialog (`SPEC-0105` v8
//! behaviour 10): they act on the field that has the focus, whether they come from their keys or
//! from the menu bar, and never on the editor behind the dialog. The search has no caret of its
//! own; it takes them through [`edit_search`].

use gpui::{App, Context, Entity, KeyDownEvent, Window};

use super::SettingsView;
use crate::editor_view::EditorView;

/// The edit actions the dialog takes for its fields.
pub(crate) const EDIT_ACTIONS: [&str; 6] =
    ["$SelectAll", "$Copy", "$Cut", "$Paste", "$Undo", "$Redo"];

/// The search after an edit action: its text, whether all of it is marked, and what goes to the
/// clipboard.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SearchEdit {
    pub query: String,
    pub marked: bool,
    pub copied: Option<String>,
}

/// What edit action `id` does to the search `query`, all of it `marked` or not; `pasted` is the
/// clipboard's text for Paste. `None` when `id` is no edit action.
pub(crate) fn edit_search(
    query: &str,
    marked: bool,
    id: &str,
    pasted: Option<&str>,
) -> Option<SearchEdit> {
    let marked = marked && !query.is_empty();
    let keep = |copied: Option<String>| SearchEdit {
        query: query.to_owned(),
        marked,
        copied,
    };
    Some(match id {
        "$SelectAll" => SearchEdit {
            query: query.to_owned(),
            marked: !query.is_empty(),
            copied: None,
        },
        "$Copy" => keep(marked.then(|| query.to_owned())),
        "$Cut" if marked => SearchEdit {
            query: String::new(),
            marked: false,
            copied: Some(query.to_owned()),
        },
        "$Cut" | "$Undo" | "$Redo" => keep(None),
        "$Paste" => {
            // The search is one line: the clipboard's first, without control characters.
            let text: String = pasted
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            if text.is_empty() {
                keep(None)
            } else {
                SearchEdit {
                    query: if marked {
                        text
                    } else {
                        format!("{query}{text}")
                    },
                    marked: false,
                    copied: None,
                }
            }
        }
        _ => return None,
    })
}

/// The edit action key `event` stands for in the keymap in use, if it is one with the primary
/// modifier (⌘ on macOS, Ctrl elsewhere): ⇧Delete, also bound to Cut, keeps restoring shortcuts
/// on the keymap page (behaviour 4).
pub(super) fn edit_action_of(event: &KeyDownEvent) -> Option<String> {
    let m = &event.keystroke.modifiers;
    let primary = if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    };
    if !primary {
        return None;
    }
    let stroke = idecraft_ui::keymap::Keystroke::from_platform(
        &event.keystroke.key,
        m.control,
        m.alt,
        m.shift,
        m.platform,
    )?;
    let keymap = crate::keymaps::current();
    keymap
        .actions_for(&stroke, None)
        .into_iter()
        .find(|a| EDIT_ACTIONS.contains(a))
        .map(str::to_owned)
}

impl SettingsView {
    /// The text field of the dialog that has the focus, if one has it (the search is none).
    fn focused_field(&self, window: &Window, cx: &App) -> Option<Entity<EditorView>> {
        [
            &self.shell,
            &self.protected,
            &self.font_preview,
            &self.advanced_search,
            &self.advanced_field,
        ]
        .into_iter()
        .chain(self.font_fields.iter())
        .chain(self.ai_fields.iter())
        .find(|f| f.read(cx).focus().is_focused(window))
        .cloned()
    }

    /// Carries out edit action `id` (`$SelectAll`, `$Copy`, …) on the field that has the focus,
    /// else on the search; `false` when `id` is no edit action (`SPEC-0105` v8 behaviour 10).
    pub fn edit_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !EDIT_ACTIONS.contains(&id) {
            return false;
        }
        if let Some(field) = self.focused_field(window, cx) {
            field.update(cx, |f, cx| {
                f.run_action(id, cx);
                cx.notify();
            });
            return true;
        }
        let pasted = (id == "$Paste")
            .then(|| cx.read_from_clipboard().and_then(|c| c.text()))
            .flatten();
        if let Some(edit) = edit_search(&self.query, self.query_marked, id, pasted.as_deref()) {
            if let Some(text) = edit.copied {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            self.query_marked = edit.marked;
            if edit.query != self.query {
                self.query = edit.query;
                self.after_query(cx);
            }
            cx.notify();
        }
        true
    }

    /// Whether the whole search is marked (Select All).
    pub fn search_marked(&self) -> bool {
        self.query_marked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(
        query: &str,
        marked: bool,
        id: &str,
        pasted: Option<&str>,
    ) -> (String, bool, Option<String>) {
        let e = edit_search(query, marked, id, pasted).expect("an edit action");
        (e.query, e.marked, e.copied)
    }

    /// SPEC-0105 v8 AK-13 (OBS-0062 S6): Select All marks the search, typed or pasted text
    /// replaces the mark, Cut empties onto the clipboard, Copy copies only a mark, Undo and Redo
    /// change nothing.
    #[test]
    fn the_search_takes_select_all_paste_and_cut() {
        assert_eq!(
            edit("font", false, "$SelectAll", None),
            ("font".into(), true, None)
        );
        assert_eq!(
            edit("", false, "$SelectAll", None),
            ("".into(), false, None),
            "nothing to mark"
        );
        assert_eq!(
            edit("font", true, "$Copy", None),
            ("font".into(), true, Some("font".into()))
        );
        assert_eq!(
            edit("font", false, "$Copy", None),
            ("font".into(), false, None)
        );
        assert_eq!(
            edit("font", true, "$Cut", None),
            ("".into(), false, Some("font".into()))
        );
        assert_eq!(
            edit("font", false, "$Cut", None),
            ("font".into(), false, None)
        );
        assert_eq!(
            edit("font", true, "$Paste", Some("keymap\nsecond line")),
            ("keymap".into(), false, None)
        );
        assert_eq!(
            edit("font", false, "$Paste", Some(" size")),
            ("font size".into(), false, None)
        );
        assert_eq!(
            edit("font", true, "$Paste", Some("\t")),
            ("font".into(), true, None)
        );
        assert_eq!(
            edit("font", true, "$Undo", None),
            ("font".into(), true, None)
        );
        assert_eq!(
            edit("font", true, "$Redo", None),
            ("font".into(), true, None)
        );
        assert!(edit_search("font", true, "SaveAll", None).is_none());
        assert!(
            EDIT_ACTIONS
                .iter()
                .all(|id| edit_search("", false, id, None).is_some())
        );
    }
}
