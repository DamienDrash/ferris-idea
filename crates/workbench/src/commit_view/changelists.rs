//! Changelists in the Commit tool window (`SPEC-0335`): the lists are read with the status and
//! kept in the workspace, a group per list, and the actions New, Edit, Set Active, Delete and
//! Move with their strip above the Amend switch.

use std::path::Path;

use ferris_idea_core::changelists::{CHANGELISTS_FILE, Changelists, ListError};
use gpui::{Context, Window, div, prelude::*, px};

use super::{CommitEvent, CommitView, button, c, checkbox};
use crate::changelist_model::{self as model, ListOp, ListStrip};
use crate::commit_model::Row;
use crate::palette::Palette;

/// Modification time and size of the state file, to see that another program changed it.
fn stamp(root: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::symlink_metadata(root.join(CHANGELISTS_FILE)).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Writes the lists when there is something to keep, after the generated ignore file lists the
/// state file (`SPEC-0335` behaviour 19).
fn write_lists(root: &Path, lists: &Changelists) -> std::io::Result<()> {
    if !lists.keeps_a_file(root) {
        return Ok(());
    }
    crate::goto_model::refresh_local_state_ignore(root);
    lists.save(root)
}

impl CommitView {
    /// Reads the changelists and brings them in line with the changed files just read
    /// (`SPEC-0335` behaviours 2–4 and 17). A state file that cannot be used is said once and is
    /// not replaced by looking (behaviour 21).
    pub(super) fn read_lists(&mut self, renames: &[ferris_idea_vcs::Rename]) {
        let (mut lists, problem) = Changelists::load(&self.root);
        if problem != self.lists_problem
            && let Some(why) = problem
        {
            self.note = Some((model::lists_unreadable(why), false));
        }
        self.lists_problem = problem;
        let renames: Vec<(String, String)> = renames
            .iter()
            .map(|r| (r.from.clone(), r.to.clone()))
            .collect();
        if lists.sync(&self.list.changed_paths(), &renames)
            && problem.is_none()
            && let Err(e) = write_lists(&self.root, &lists)
        {
            self.note = Some((model::lists_not_saved(&e), false));
        }
        self.lists = lists;
        self.lists_stamp = stamp(&self.root);
        if self
            .current_list
            .as_deref()
            .is_some_and(|l| self.lists.get(l).is_none())
        {
            self.current_list = None;
        }
    }

    /// Whether the state file of the changelists changed since the window read it, as after a
    /// change through the MCP tool (`SPEC-0335` behaviour 17).
    pub fn changelists_file_changed(&self) -> bool {
        self.remote.is_none() && stamp(&self.root) != self.lists_stamp
    }

    /// The changelists as the window shows them.
    pub fn lists(&self) -> &Changelists {
        &self.lists
    }

    /// The selected changelist, if a list and not a file is selected.
    pub fn selected_list(&self) -> Option<&str> {
        self.current_list.as_deref()
    }

    /// Selects a changelist as a click on its row does (`SPEC-0335` behaviour 14).
    pub fn select_list(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.lists.get(name).is_some() {
            self.current_list = Some(name.to_owned());
            self.current = None;
            cx.notify();
        }
    }

    /// Checks all files of a changelist, or unchecks them when all are checked, as its checkbox
    /// does (`SPEC-0335` behaviour 14).
    pub fn toggle_list(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(list) = self.lists.get(name) {
            let paths: Vec<String> = list.files.iter().cloned().collect();
            self.list.toggle_paths(&paths);
            cx.notify();
        }
    }

    /// The open strip of a changelist action.
    pub fn list_strip(&self) -> Option<&ListStrip> {
        self.list_strip.as_ref()
    }

    /// Why the strip's name was refused.
    pub fn list_strip_error(&self) -> Option<&str> {
        self.list_error.as_deref()
    }

    /// The text of the strip's name field.
    pub fn list_name(&self, cx: &gpui::App) -> String {
        self.list_name.read(cx).state.buffer.text()
    }

    /// Replaces the text of the strip's name field.
    pub fn set_list_name(&mut self, text: &str, cx: &mut Context<Self>) {
        self.list_name.update(cx, |f, cx| f.set_text(text, cx));
        cx.notify();
    }

    /// Flips "Set active" of the open strip where it can be changed.
    pub fn toggle_list_strip_active(&mut self, cx: &mut Context<Self>) {
        if let Some(strip) = self.list_strip.as_mut() {
            strip.toggle_set_active();
            cx.notify();
        }
    }

    fn say(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        cx.emit(CommitEvent::Status(text.into()));
        cx.notify();
    }

    /// Keeps `next` as the project's lists; `false`, with the reason in the status bar, when
    /// the state file must stay as it is or cannot be written.
    fn store_lists(&mut self, next: Changelists, cx: &mut Context<Self>) -> bool {
        if let Some(why) = self.lists_problem.filter(|w| w.keeps_the_file()) {
            self.say(model::lists_kept(why), cx);
            return false;
        }
        match write_lists(&self.root, &next) {
            Ok(()) => {
                self.lists = next;
                self.lists_problem = None;
                self.lists_stamp = stamp(&self.root);
                if self
                    .current_list
                    .as_deref()
                    .is_some_and(|l| self.lists.get(l).is_none())
                {
                    self.current_list = None;
                }
                true
            }
            Err(e) => {
                self.say(model::lists_not_saved(&e), cx);
                false
            }
        }
    }

    fn open_list_strip(
        &mut self,
        strip: ListStrip,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // One strip at a time.
        self.rollback = None;
        self.list_error = None;
        // The proposed or current name is selected: typing replaces it (`SPEC-0335` v2).
        self.list_name
            .update(cx, |f, cx| f.set_text_selected(name, cx));
        if strip.has_name() {
            window.focus(&self.list_name.read(cx).focus());
        } else {
            window.focus(&self.focus);
        }
        self.list_strip = Some(strip);
        cx.notify();
    }

    /// Runs a changelist action (`SPEC-0335` behaviours 8–12): at once where nothing is to be
    /// asked, else by opening its strip. Without something to act on the status bar says so.
    pub fn changelist_action(&mut self, op: ListOp, window: &mut Window, cx: &mut Context<Self>) {
        self.lists_menu = false;
        if self.remote.is_some() {
            return self.say(model::NO_REMOTE_CHANGELISTS, cx);
        }
        if let Some(why) = self.lists_problem.filter(|w| w.keeps_the_file()) {
            return self.say(model::lists_kept(why), cx);
        }
        let target = model::target_list(
            &self.lists,
            self.current_list.as_deref(),
            self.current.as_deref(),
        );
        match op {
            ListOp::New => {
                let strip = ListStrip::New {
                    set_active: self.new_list_active,
                };
                let name = self.lists.free_name();
                self.open_list_strip(strip, &name, window, cx);
            }
            ListOp::Edit => {
                let Some(list) = target else {
                    return self.say(model::NO_LIST_SELECTED, cx);
                };
                let active = self.lists.active() == list;
                let strip = ListStrip::Edit {
                    list: list.clone(),
                    set_active: active,
                    fixed: active,
                };
                self.open_list_strip(strip, &list, window, cx);
            }
            ListOp::SetActive => {
                let Some(list) = target else {
                    return self.say(model::NO_LIST_SELECTED, cx);
                };
                let mut next = self.lists.clone();
                match next.set_active(&list) {
                    Ok(true) => {
                        if self.store_lists(next, cx) {
                            self.say(model::activated(&list), cx);
                        }
                    }
                    Ok(false) => self.say(model::already_active(&list), cx),
                    Err(e) => self.say(e.to_string(), cx),
                }
            }
            ListOp::Delete => {
                let Some(list) = self
                    .current_list
                    .clone()
                    .filter(|l| self.lists.get(l).is_some())
                else {
                    return self.say(model::NO_LIST_TO_DELETE, cx);
                };
                match model::delete_strip(&self.lists, &list) {
                    Some(strip) => self.open_list_strip(strip, "", window, cx),
                    None => {
                        let mut next = self.lists.clone();
                        match next.delete(&list, None) {
                            Ok(outcome) => {
                                if self.store_lists(next, cx) {
                                    self.say(model::deleted(&list, &outcome), cx);
                                }
                            }
                            Err(e) => self.say(e.to_string(), cx),
                        }
                    }
                }
            }
            ListOp::Move => {
                let files = model::files_to_move(
                    &self.lists,
                    self.current_list.as_deref(),
                    self.current.as_deref(),
                );
                if files.is_empty() {
                    return self.say(model::NOTHING_TO_MOVE, cx);
                }
                let targets = self.lists.move_targets(&files);
                let strip = ListStrip::Move {
                    files,
                    offered: targets.offered,
                    set_active: self.new_list_active,
                };
                self.open_list_strip(strip, &targets.preselected, window, cx);
            }
        }
    }

    /// What confirming `strip` with `name` makes of `next`, and what the window says then.
    fn apply_strip(
        strip: &ListStrip,
        name: &str,
        next: &mut Changelists,
    ) -> Result<String, ListError> {
        Ok(match strip {
            ListStrip::New { set_active } => {
                let made = next.create(name, *set_active)?;
                model::created(&made, *set_active)
            }
            ListStrip::Edit {
                list,
                set_active,
                fixed,
            } => {
                let renamed = next.rename(list, name)?;
                let now = renamed.clone().unwrap_or_else(|| list.clone());
                let activated = !fixed && *set_active && next.set_active(&now)?;
                model::edited(list, renamed.as_deref(), activated)
            }
            ListStrip::Delete {
                list,
                choose,
                selected,
                ..
            } => {
                let outcome = next.delete(list, choose.get(*selected).map(String::as_str))?;
                model::deleted(list, &outcome)
            }
            ListStrip::Move {
                files, set_active, ..
            } => {
                let outcome = next.move_files(files, name, true)?;
                let to = name.trim();
                if *set_active {
                    next.set_active(to)?;
                }
                model::moved(to, &outcome)
            }
        })
    }

    /// Confirms the open strip, as Enter and its button do. A name the rules refuse keeps the
    /// strip open and says why under the field.
    pub fn confirm_list_strip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(strip) = self.list_strip.clone() else {
            return;
        };
        let name = self.list_name(cx);
        let mut next = self.lists.clone();
        let said = match Self::apply_strip(&strip, &name, &mut next) {
            Ok(said) => said,
            Err(e) => {
                self.list_error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        if let ListStrip::New { set_active } | ListStrip::Move { set_active, .. } = &strip {
            self.new_list_active = *set_active;
        }
        // The selection follows a renamed list.
        let renamed = match &strip {
            ListStrip::Edit { list, .. } if self.current_list.as_deref() == Some(list) => {
                Some(name.trim().to_owned())
            }
            _ => None,
        };
        let stored = self.store_lists(next, cx);
        if stored {
            if let Some(name) = renamed.filter(|n| self.lists.get(n).is_some()) {
                self.current_list = Some(name);
            }
            self.say(said, cx);
        }
        self.list_strip = None;
        self.list_error = None;
        window.focus(&self.focus);
        cx.notify();
    }

    /// Closes the open strip without a change, as Escape and Cancel do; `false` when none was
    /// open.
    pub fn cancel_list_strip(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let open = self.list_strip.take().is_some();
        if open {
            self.list_error = None;
            window.focus(&self.focus);
            cx.notify();
        }
        open
    }

    /// Up and Down in the open strip: the next or previous list to choose, around the ends. In
    /// Move the chosen list's name goes into the field.
    pub fn step_list_strip(&mut self, forward: bool, cx: &mut Context<Self>) {
        let name = self.list_name(cx);
        let Some(strip) = self.list_strip.as_mut() else {
            return;
        };
        let count = strip.choices().len();
        if count == 0 {
            return;
        }
        let step = |at: Option<usize>| match (at, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => (i + count - 1) % count,
        };
        match strip {
            ListStrip::Delete { selected, .. } => *selected = step(Some(*selected)),
            ListStrip::Move { offered, .. } => {
                let at = offered.iter().position(|o| o == name.trim());
                let chosen = offered[step(at)].clone();
                self.list_name
                    .update(cx, |f, cx| f.set_text_selected(&chosen, cx));
            }
            _ => {}
        }
        cx.notify();
    }

    /// Chooses row `index` of the open strip, as a click on it does.
    pub fn choose_in_list_strip(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(strip) = self.list_strip.as_mut() else {
            return;
        };
        match strip {
            ListStrip::Delete {
                selected, choose, ..
            } if index < choose.len() => *selected = index,
            ListStrip::Move { offered, .. } if index < offered.len() => {
                let chosen = offered[index].clone();
                self.list_name
                    .update(cx, |f, cx| f.set_text_selected(&chosen, cx));
            }
            _ => {}
        }
        cx.notify();
    }

    /// The row of a changelist: chevron, checkbox, the name (bold for the active list) and the
    /// number of files; a click selects the list (`SPEC-0335` behaviours 13 and 14).
    pub(super) fn changelist_row(
        &self,
        row: &Row,
        active: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = Palette::current();
        let count = row.paths.len();
        let count_text = format!("{count} file{}", if count == 1 { "" } else { "s" });
        let name = row.label.clone();
        let selected = self.current_list.as_deref() == Some(name.as_str());
        let paths = row.paths.clone();
        div()
            .id(gpui::SharedString::from(format!("changelist-{name}")))
            .flex()
            .items_center()
            .gap_1()
            .h(px(p.tree_row_height))
            .mx_2()
            .pl(px(4.))
            .rounded(px(4.))
            .when(selected, |d| d.bg(c(p.tree_selection)))
            .when(!selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
            .child(self.chevron(&row.key, cx))
            .child(
                div()
                    .id(gpui::SharedString::from(format!("changelist-check-{name}")))
                    .child(checkbox(self.list.paths_checked(&row.paths)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.list.toggle_paths(&paths);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .when(active, |d| d.font_weight(gpui::FontWeight::BOLD))
                    .child(row.label.clone()),
            )
            .child(div().text_color(c(p.ui_dim)).child(count_text))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                window.focus(&this.focus);
                this.select_list(&name, cx);
            }))
            .into_any_element()
    }

    /// The menu of the toolbar's changelists button: the five actions.
    pub(super) fn lists_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        div()
            .id("commit-changelists-menu")
            .mx_2()
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.popup_border))
            .bg(c(p.popup))
            .children(ListOp::ALL.into_iter().enumerate().map(|(i, (op, title))| {
                div()
                    .id(("commit-changelists-menu-item", i))
                    .flex()
                    .items_center()
                    .px_2()
                    .h(px(24.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(title)
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                        this.changelist_action(op, window, cx);
                    }))
            }))
    }

    /// The open strip of a changelist action, above the Amend switch.
    pub(super) fn list_strip_element(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let strip = self.list_strip.as_ref()?;
        let p = Palette::current();
        let name = self.list_name(cx);
        let typed = name.trim();
        let chosen = match strip {
            ListStrip::Delete { selected, .. } => Some(*selected),
            ListStrip::Move { offered, .. } => offered.iter().position(|o| o == typed),
            _ => None,
        };
        // A name no list has makes a new list (`SPEC-0335` behaviour 12).
        let is_new = matches!(strip, ListStrip::Move { .. })
            && !typed.is_empty()
            && self.lists.get(typed).is_none();
        let active = self.lists.active().to_owned();
        let choices: Vec<gpui::Stateful<gpui::Div>> = strip
            .choices()
            .iter()
            .enumerate()
            .map(|(i, list)| {
                div()
                    .id(("changelist-choice", i))
                    .flex_none()
                    .px_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .when(chosen == Some(i), |d| d.bg(c(p.tree_selection)))
                    .when(chosen != Some(i), |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .when(*list == active, |d| d.font_weight(gpui::FontWeight::BOLD))
                    .child(list.clone())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.choose_in_list_strip(i, cx);
                    }))
            })
            .collect();
        Some(
            div()
                .id("commit-changelist-strip")
                .flex_none()
                .mx_2()
                .mb_2()
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .rounded(px(6.))
                .border_1()
                .border_color(c(p.popup_border))
                .bg(c(p.popup))
                .child(strip.title())
                .when(strip.has_name(), |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(24.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(c(p.separator))
                                    .overflow_hidden()
                                    .child(self.list_name.clone()),
                            )
                            .when(is_new, |d| {
                                d.child(div().flex_none().text_color(c(p.ui_muted)).child("New"))
                            }),
                    )
                })
                .when(!choices.is_empty(), |d| {
                    d.child(
                        div()
                            .id("changelist-choices")
                            .max_h(px(24. * 6.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(choices),
                    )
                })
                .when_some(strip.set_active(), |d, (on, free)| {
                    d.child(
                        div()
                            .id("changelist-set-active")
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(!free, |d| d.opacity(0.5))
                            .child(checkbox(Some(on)))
                            .child("Set active")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.toggle_list_strip_active(cx)),
                            ),
                    )
                })
                .when_some(self.list_error.clone(), |d, error| {
                    d.child(div().text_color(c(p.error_text)).child(error))
                })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            button("changelist-ok", strip.confirm_label(), true).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.confirm_list_strip(window, cx)
                                }),
                            ),
                        )
                        .child(
                            button("changelist-cancel", "Cancel", false).on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.cancel_list_strip(window, cx);
                                },
                            )),
                        ),
                ),
        )
    }
}
