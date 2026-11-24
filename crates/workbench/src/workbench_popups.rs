//! Popups that close on a click outside them or when the window loses the focus (`SPEC-0275` v4
//! behaviour 28). One registry, [`Popup`], names every such popup of the workbench; drawing a popup
//! through [`WorkbenchView::closing_popup`] makes a mouse press outside it close it, and the window
//! closes all of them when it stops being the active window. Dialogs are not registered.

use gpui::{Context, Div, Entity, FocusHandle, Window, div, prelude::*};

use super::WorkbenchView;

/// A popup that closes without a choice on a click outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Popup {
    /// The menu of a ▶ run marker and the other Run/Debug and Generate choices.
    RunMarker,
    /// The Python interpreter choice.
    Python,
    /// The target of a schema comparison.
    CompareTargets,
    /// Run… (⌃⌥R).
    RunChooser,
    /// Go to Line, Recent Files and the Rename name popup.
    Goto,
    /// Search Everywhere.
    Everywhere,
    /// File Structure.
    Structure,
    /// Show Usages.
    Usages,
    /// Branches.
    Branches,
    /// Recent projects.
    Projects,
    /// VCS operations.
    VcsOps,
    /// Stashes.
    Stashes,
    /// Quick Switch Scheme.
    Themes,
    /// Context actions (⌥↩).
    Intentions,
    /// Quick documentation.
    Doc,
    /// The query history of a SQL file's connection (`SPEC-0093` v4 behaviour 15).
    SqlHistory,
    /// The bookmark popups: mnemonic, list, description (`SPEC-0331`).
    Bookmarks,
}

impl Popup {
    /// Every registered popup.
    pub(super) const ALL: [Popup; 17] = [
        Popup::RunMarker,
        Popup::Python,
        Popup::CompareTargets,
        Popup::RunChooser,
        Popup::Goto,
        Popup::Everywhere,
        Popup::Structure,
        Popup::Usages,
        Popup::Branches,
        Popup::Projects,
        Popup::VcsOps,
        Popup::Stashes,
        Popup::Themes,
        Popup::Intentions,
        Popup::Doc,
        Popup::SqlHistory,
        Popup::Bookmarks,
    ];
}

impl WorkbenchView {
    /// Whether `popup` is open.
    pub(super) fn popup_open(&self, popup: Popup) -> bool {
        match popup {
            Popup::RunMarker => self.jvm_popup.is_some(),
            Popup::Python => self.python_popup.is_some(),
            Popup::CompareTargets => self.compare_targets.is_some(),
            Popup::RunChooser => self.run_chooser.is_some(),
            Popup::Goto => self.goto.is_some(),
            Popup::Everywhere => self.everywhere().is_some(),
            Popup::Structure => self.structure.is_some(),
            Popup::Usages => self.usages.is_some(),
            Popup::Branches => self.branches.is_some(),
            Popup::Projects => self.projects.is_some(),
            Popup::VcsOps => self.vcs_ops.is_some(),
            Popup::Stashes => self.stashes.is_some(),
            Popup::Themes => self.themes.is_some(),
            Popup::Intentions => self.intentions.is_some(),
            Popup::Doc => self.doc.is_some(),
            Popup::SqlHistory => self.history_open(),
            Popup::Bookmarks => self.bookmarks_popup.is_some(),
        }
    }

    /// Whether a popup other than the quick documentation, or a context menu, is open: what a
    /// tooltip or a hover card must not lie over (`SPEC-0294` v3 behaviour 11).
    pub(super) fn popup_over_editor(&self) -> bool {
        self.context_menu.is_some()
            || Popup::ALL
                .iter()
                .any(|p| *p != Popup::Doc && self.popup_open(*p))
    }

    /// Closes `popup` without a choice, as Escape does; returns whether it was open.
    pub(super) fn close_popup(&mut self, popup: Popup) -> bool {
        if !self.popup_open(popup) {
            return false;
        }
        match popup {
            Popup::RunMarker => self.jvm_popup = None,
            Popup::Python => self.python_popup = None,
            Popup::CompareTargets => self.compare_targets = None,
            Popup::RunChooser => self.run_chooser = None,
            Popup::Goto => {
                self.goto = None;
                self.rename_at = None;
                self.introduced_name = None;
                // The name popup after an extraction closed without a new name (SPEC-0095 v5
                // behaviour 8a): the prompt in the status line no longer holds.
                if let Some(name) = self.extract_name_prompt.take() {
                    self.status = Some(ferris_idea_languages::actions::kept_name_message(&name));
                }
            }
            Popup::Everywhere => self.show_everywhere = false,
            Popup::Structure => self.structure = None,
            Popup::Usages => self.usages = None,
            Popup::Branches => self.branches = None,
            Popup::Projects => self.projects = None,
            Popup::VcsOps => self.vcs_ops = None,
            Popup::Stashes => self.stashes = None,
            Popup::Themes => self.themes = None,
            Popup::Intentions => {
                self.intentions = None;
                self.refactor_targets.clear();
                self.refactor_own.clear();
                self.own_fixes.clear();
            }
            Popup::Doc => self.doc = None,
            Popup::SqlHistory => self.close_history(),
            Popup::Bookmarks => {
                self.bookmarks_popup = None;
                self.bookmark_popup_line = None;
            }
        }
        true
    }

    /// Closes every open popup; returns whether one was open.
    pub(super) fn close_popups(&mut self) -> bool {
        let mut closed = false;
        for p in Popup::ALL {
            closed |= self.close_popup(p);
        }
        closed
    }

    /// `content` drawn as `popup`: a mouse press outside its bounds closes it and goes on to what
    /// it hit. When that takes no focus, the editor gets it back. A press, a release or the wheel
    /// inside its bounds stays in the popup and never reaches what lies under it (`SPEC-0275` v14
    /// behaviour 28b): the popup's own views need not see to that themselves.
    pub(super) fn closing_popup(
        &self,
        popup: Popup,
        content: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .occlude()
            .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
                if this.close_popup(popup) {
                    this.refocus_after_popup(window, cx);
                    cx.notify();
                }
            }))
            .child(content)
    }

    /// After a popup closed by a click: if the click gave nobody the focus, the active editor
    /// (or the project tree) takes it, so the keyboard keeps working.
    fn refocus_after_popup(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.defer_in(window, |this, window, cx| {
            if window.focused(cx).is_none() {
                this.focus_active(window, cx);
            }
        });
    }

    /// Closes the popup `view` when the keyboard focus leaves `focus`, unless another popup of
    /// the same kind replaced it meanwhile.
    pub(super) fn close_on_focus_out<V: 'static>(
        &mut self,
        popup: Popup,
        view: &Entity<V>,
        focus: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = view.entity_id();
        self._popup_focus = Some(cx.on_focus_out(focus, window, move |this, _, _, cx| {
            let same = match popup {
                Popup::RunMarker => this.jvm_popup.as_ref().map(|v| v.entity_id()),
                Popup::Python => this.python_popup.as_ref().map(|v| v.entity_id()),
                Popup::CompareTargets => this.compare_targets.as_ref().map(|v| v.entity_id()),
                _ => None,
            } == Some(id);
            if same && this.close_popup(popup) {
                cx.notify();
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::Popup;

    #[test]
    fn every_popup_is_registered_once() {
        // The popups of SPEC-0275 v4 behaviour 28.
        let named = [
            "RunMarker",
            "Python",
            "CompareTargets",
            "RunChooser",
            "Goto",
            "Everywhere",
            "Structure",
            "Usages",
            "Branches",
            "Projects",
            "VcsOps",
            "Stashes",
            "Themes",
            "Intentions",
            "Doc",
            "SqlHistory",
            "Bookmarks",
        ];
        let registered: Vec<String> = Popup::ALL.iter().map(|p| format!("{p:?}")).collect();
        assert_eq!(registered, named);
        for (i, p) in Popup::ALL.iter().enumerate() {
            assert!(!Popup::ALL[i + 1..].contains(p), "{p:?} twice");
        }
    }
}
