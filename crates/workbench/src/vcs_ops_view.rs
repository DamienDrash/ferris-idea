//! The VCS operations popup (`Vcs.QuickListPopupAction`, `SPEC-0109`), the Push dialog and the
//! Update Project dialog (`SPEC-0144`).

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use idecraft_core::UpdateMethod;
use idecraft_vcs::{Outgoing, PushOptions};

use crate::palette::Palette;
use crate::vcs_ops_model::{OPERATIONS, operation_digit, operation_for_digit, push_creates_branch};
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum VcsOpsEvent {
    /// Run this action.
    Run(&'static str),
    /// Close the popup.
    Close,
}

/// The numbered VCS operations (behaviour 1).
pub struct VcsOpsView {
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<VcsOpsEvent> for VcsOpsView {}

impl VcsOpsView {
    /// The popup with the first operation selected.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the popup.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The rows as shown: the first ten with their digit (v2 behaviour 6).
    pub fn rows(&self) -> Vec<String> {
        OPERATIONS
            .iter()
            .enumerate()
            .map(|(i, (title, _))| match operation_digit(i) {
                Some(d) => format!("{d}. {title}"),
                None => (*title).to_owned(),
            })
            .collect()
    }

    /// Runs row `i`.
    pub fn choose(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some((_, action)) = OPERATIONS.get(i) {
            cx.emit(VcsOpsEvent::Run(action));
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let n = OPERATIONS.len();
        // The first and the last operation (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(&event.keystroke, n) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose(self.selected, cx),
            "escape" => cx.emit(VcsOpsEvent::Close),
            k => match operation_for_digit(k) {
                Some(i) => self.choose(i, cx),
                None => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for VcsOpsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        // Each row names its shortcut from the keymap in use, as the reference does (v3).
        let keymap = crate::keymaps::current();
        let shortcuts: Vec<String> = OPERATIONS
            .iter()
            .map(|(_, action)| {
                crate::context_menu::shortcut_label(action, &keymap, cfg!(target_os = "macos"))
            })
            .collect();
        div()
            .id("vcs-ops-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(300.))
            .py_1()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("VCS Operations"),
            )
            .children(self.rows().into_iter().enumerate().map(|(i, row)| {
                div()
                    .id(("vcs-op", i))
                    // A line between the numbered entries and the rest, as in the reference.
                    .when(i == 10, |d| {
                        d.mt_1().border_t_1().border_color(c(p.separator))
                    })
                    .h(px(26.))
                    .mx_1()
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                    .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .child(div().flex_1().child(row))
                    .child(
                        div()
                            .text_color(c(p.ui_muted))
                            .child(shortcuts.get(i).cloned().unwrap_or_default()),
                    )
                    .on_click(
                        cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.choose(i, cx)),
                    )
            }))
    }
}

/// What the Push dialog asks the workbench to do.
pub enum PushEvent {
    /// Push the shown commits.
    Push,
    /// Close without pushing.
    Close,
}

/// The commits Push would send, the Push button (behaviour 3) and the options of `SPEC-0144`:
/// target branch, push tags, force with lease.
pub struct PushView {
    outgoing: Result<Outgoing, String>,
    target: gpui::Entity<crate::editor_view::EditorView>,
    tags: bool,
    force_with_lease: bool,
    /// Force push and protected branches of Settings › Version Control (`SPEC-0150`).
    vcs: idecraft_core::VcsSettings,
    /// Why a force push was refused.
    refusal: Option<String>,
    /// The commit whose files are shown (v2 behaviour 7).
    selected: usize,
    focus: FocusHandle,
    /// Redraws the title while the target branch is typed.
    _target_watch: gpui::Subscription,
}

impl EventEmitter<PushEvent> for PushView {}

impl PushView {
    /// A dialog for `outgoing`, or the reason nothing can be pushed.
    pub fn new(outgoing: Result<Outgoing, String>, cx: &mut Context<Self>) -> Self {
        let target = cx.new(crate::editor_view::EditorView::field);
        if let Ok(o) = &outgoing {
            target.update(cx, |f, cx| f.set_text(&o.branch, cx));
        }
        let watch = cx.observe(&target, |_, _, cx| cx.notify());
        Self {
            outgoing,
            target,
            tags: false,
            force_with_lease: false,
            vcs: crate::appearance::ide_settings(cx).vcs,
            refusal: None,
            selected: 0,
            focus: cx.focus_handle(),
            _target_watch: watch,
        }
    }

    /// The dialog's title: branch, remote and the target branch as typed (hand test 70: the
    /// title kept the local name after the target was changed).
    pub fn title(&self, cx: &gpui::App) -> String {
        match &self.outgoing {
            Ok(o) => {
                let target = self.target_branch(cx);
                // Only a branch the remote lacks is new (OBS-0022 L24).
                let new = if push_creates_branch(o, &target) {
                    " (new)"
                } else {
                    ""
                };
                // Push All up to Here names the last commit pushed (SPEC-0086 v8).
                let upto = o
                    .up_to
                    .as_deref()
                    .map(|id| format!(" up to {}", &id[..id.len().min(7)]))
                    .unwrap_or_default();
                format!("Push {}{upto} → {}/{target}{new}", o.branch, o.remote)
            }
            Err(_) => "Push".into(),
        }
    }

    /// The branch the push would write to on the remote.
    fn target_branch(&self, cx: &gpui::App) -> String {
        let text = self.target.read(cx).state.buffer.text().trim().to_owned();
        match self.outgoing() {
            Some(o) if text.is_empty() => o.branch.clone(),
            _ => text,
        }
    }

    /// Why force pushing to the target is not allowed, if it is not (`SPEC-0150` behaviour 4).
    fn force_refusal(&self, cx: &gpui::App) -> Option<String> {
        let branch = self.target_branch(cx);
        if !self.vcs.allow_force_push {
            Some("Force push is switched off in Settings › Version Control".into())
        } else if !self.vcs.may_force_push(&branch) {
            Some(format!(
                "{branch} is a protected branch and is never force pushed (Settings › Version Control)"
            ))
        } else {
            None
        }
    }

    /// Why the last force push was refused.
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// What would be pushed.
    pub fn outgoing(&self) -> Option<&Outgoing> {
        self.outgoing.as_ref().ok()
    }

    /// The options as chosen; the upstream is set when the remote branch is new.
    pub fn options(&self, cx: &gpui::App) -> PushOptions {
        let target = self.target.read(cx).state.buffer.text().trim().to_owned();
        let same = self.outgoing().is_some_and(|o| o.branch == target);
        PushOptions {
            set_upstream: self.outgoing().is_some_and(|o| !o.has_upstream) && same,
            target: (!same && !target.is_empty()).then_some(target),
            tags: self.tags,
            force_with_lease: self.force_with_lease,
            branch: self.outgoing().map(|o| o.branch.clone()),
            up_to: self.outgoing().and_then(|o| o.up_to.clone()),
        }
    }

    /// Index of the commit whose files are shown.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Selects commit `i` of the list (v2 behaviour 7).
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        let n = self.outgoing().map_or(0, |o| o.commits.len());
        if n > 0 {
            self.selected = i.min(n - 1);
            cx.notify();
        }
    }

    /// The files of the selected commit, as `status path`.
    pub fn selected_files(&self) -> Vec<String> {
        self.outgoing()
            .and_then(|o| {
                let c = o.commits.get(self.selected)?;
                o.files.get(&c.id)
            })
            .map(|files| {
                files
                    .iter()
                    .map(|f| format!("{}  {}", f.status, f.path))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The target field's text.
    pub fn target_text(&self, cx: &gpui::App) -> String {
        self.target.read(cx).state.buffer.text()
    }

    /// An edit action of the menu bar (⌘A, ⌘C, ⌘V, ⌘X): it goes to the target field, never to
    /// the editor behind the dialog (v2 behaviour 8, OBS-0022 L23).
    pub fn edit_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.outgoing.is_err() {
            return;
        }
        window.focus(&self.target.read(cx).focus());
        self.target.update(cx, |f, cx| {
            f.run_action(id, cx);
            cx.notify();
        });
    }

    /// Sets the target branch field (tests).
    pub fn set_target(&mut self, text: &str, cx: &mut Context<Self>) {
        self.target.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Puts the keyboard focus into the target branch field (as Tab does).
    pub fn focus_target(&self, window: &mut Window, cx: &gpui::App) {
        window.focus(&self.target.read(cx).focus());
    }

    /// Switches "Push tags" (⌥T).
    pub fn toggle_tags(&mut self, cx: &mut Context<Self>) {
        self.tags = !self.tags;
        cx.notify();
    }

    /// Switches "Force push (with lease)" (⌥F).
    pub fn toggle_force(&mut self, cx: &mut Context<Self>) {
        self.refusal = None;
        if !self.force_with_lease
            && let Some(why) = self.force_refusal(cx)
        {
            self.refusal = Some(why);
        } else {
            self.force_with_lease = !self.force_with_lease;
        }
        cx.notify();
    }

    fn push(&mut self, cx: &mut Context<Self>) {
        // The target may have been changed to a protected branch after force was chosen.
        if self.force_with_lease
            && let Some(why) = self.force_refusal(cx)
        {
            self.refusal = Some(why);
            cx.notify();
            return;
        }
        if self.outgoing.is_ok() {
            cx.emit(PushEvent::Push);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let in_field = self.target.read(cx).focus().is_focused(window);
        let n = self.outgoing().map_or(0, |o| o.commits.len());
        // The first and the last outgoing commit (`SPEC-0275` v19 behaviour 47); in the target
        // field Home and End move the caret.
        if let Some(to) =
            crate::list_nav::edge_with(k, n, crate::list_nav::Keys::beside_field(!in_field))
        {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let primary = if cfg!(target_os = "macos") {
            k.modifiers.platform
        } else {
            k.modifiers.control
        };
        match k.key.as_str() {
            // ⌘A selects the target field's text instead of the editor's (OBS-0022 L23).
            "a" if primary => self.edit_action("$SelectAll", window, cx),
            "up" if n > 0 => self.selected = (self.selected + n - 1) % n,
            "down" if n > 0 => self.selected = (self.selected + 1) % n,
            "enter" => self.push(cx),
            "escape" if in_field => window.focus(&self.focus),
            "escape" => cx.emit(PushEvent::Close),
            "tab" if !in_field => {
                let f = self.target.read(cx).focus();
                window.focus(&f);
            }
            "tab" => window.focus(&self.focus),
            "t" if k.modifiers.alt => self.toggle_tags(cx),
            "f" if k.modifiers.alt => self.toggle_force(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for PushView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let title = self.title(cx);
        let selected = self.selected;
        let files = self.selected_files();
        let (rows, note): (Vec<String>, Option<String>) = match &self.outgoing {
            Ok(o) => (
                o.commits
                    .iter()
                    .map(|c| format!("{}  {}", c.short_id, c.summary))
                    .collect(),
                o.commits
                    .is_empty()
                    .then(|| "No commits to push (as of the last fetch)".to_owned()),
            ),
            Err(e) => (Vec::new(), Some(e.clone())),
        };
        div()
            .id("push-dialog")
            .track_focus(&self.focus)
            .occlude()
            // Seen before the target field, so Enter pushes instead of breaking the line
            // (hand test 70, D19 pattern).
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(760.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
            .child(
                // Commits left, the selected commit's files right (v2 behaviour 7).
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("push-commits")
                            .flex_1()
                            .max_h(px(300.))
                            .overflow_y_scroll()
                            .children(rows.into_iter().enumerate().map(|(i, r)| {
                                div()
                                    .id(("push-commit", i))
                                    .h(px(22.))
                                    .px_1()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .when(i == selected, |d| d.bg(c(p.tree_selection)))
                                    .child(r)
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| this.select(i, cx),
                                    ))
                            })),
                    )
                    .when(
                        self.outgoing().is_some_and(|o| !o.commits.is_empty()),
                        |d| {
                            d.child(
                                div()
                                    .id("push-files")
                                    .w(px(300.))
                                    .max_h(px(300.))
                                    .overflow_y_scroll()
                                    .border_l_1()
                                    .border_color(c(p.separator))
                                    .pl_2()
                                    .text_color(c(p.ui_muted))
                                    .children(
                                        files
                                            .into_iter()
                                            .map(|f| div().h(px(22.)).whitespace_nowrap().child(f)),
                                    ),
                            )
                        },
                    ),
            )
            .when_some(note, |d, n| {
                d.child(div().text_color(c(p.ui_muted)).child(n))
            })
            .when_some(self.refusal.clone(), |d, n| {
                d.child(div().text_color(c(p.ui_muted)).child(n))
            })
            .when(self.outgoing.is_ok(), |d| {
                let check = |on: bool| if on { "☑" } else { "☐" };
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_color(c(p.ui_muted)).child("Target branch"))
                        .child(
                            crate::metrics::field_frame()
                                .flex_1()
                                .child(self.target.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(
                            div()
                                .id("push-tags")
                                .child(format!("{} Push tags (⌥T)", check(self.tags)))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.toggle_tags(cx)
                                })),
                        )
                        .child(
                            div()
                                .id("push-force")
                                .child(format!(
                                    "{} Force push, only if unchanged since fetch (⌥F)",
                                    check(self.force_with_lease)
                                ))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.toggle_force(cx)
                                })),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("push-cancel")
                            .px_2()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child("Cancel")
                            .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(PushEvent::Close)
                            })),
                    )
                    .child(
                        div()
                            .id("push-button")
                            .px_2()
                            .rounded(px(4.))
                            .bg(c(p.tree_selection))
                            .child(if self.force_with_lease {
                                "Force Push"
                            } else {
                                "Push"
                            })
                            .on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.push(cx)),
                            ),
                    ),
            )
    }
}

/// What the Update Project dialog asks the workbench to do.
pub enum UpdateEvent {
    /// Update with this method.
    Run(UpdateMethod),
    /// Reset the branch to its upstream (`SPEC-0144` v4 behaviour 9); never remembered.
    Reset,
    /// Close without updating.
    Close,
}

/// Update Project: merge or rebase, the last choice preselected (`SPEC-0144`), or a reset to the
/// upstream (v4 behaviour 9).
pub struct UpdateView {
    method: UpdateMethod,
    /// The third choice, reset to the upstream, is selected.
    reset: bool,
    /// Commits of the branch the upstream lacks as of the last fetch.
    ahead: usize,
    focus: FocusHandle,
}

impl EventEmitter<UpdateEvent> for UpdateView {}

impl UpdateView {
    /// The dialog with `method` selected.
    pub fn new(method: UpdateMethod, cx: &mut Context<Self>) -> Self {
        Self {
            method,
            reset: false,
            ahead: 0,
            focus: cx.focus_handle(),
        }
    }

    /// The dialog for a branch `ahead` commits ahead of its upstream: the reset choice names
    /// what it gives up (IMP-0322).
    pub fn with_ahead(mut self, ahead: usize) -> Self {
        self.ahead = ahead;
        self
    }

    /// Whether the reset to the upstream is selected.
    pub fn reset_selected(&self) -> bool {
        self.reset
    }

    /// Selects the reset to the upstream (⌥E).
    pub fn select_reset(&mut self, cx: &mut Context<Self>) {
        self.reset = true;
        cx.notify();
    }

    /// Focus handle of the dialog.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The selected method.
    pub fn method(&self) -> UpdateMethod {
        self.method
    }

    /// Selects `method`.
    pub fn select(&mut self, method: UpdateMethod, cx: &mut Context<Self>) {
        self.method = method;
        self.reset = false;
        cx.notify();
    }

    fn run(&self, cx: &mut Context<Self>) {
        cx.emit(if self.reset {
            UpdateEvent::Reset
        } else {
            UpdateEvent::Run(self.method)
        });
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            // Merge → Rebase → Reset → Merge (v4 behaviour 9).
            "down" => {
                (self.method, self.reset) = match (self.method, self.reset) {
                    (_, true) => (UpdateMethod::Merge, false),
                    (UpdateMethod::Merge, false) => (UpdateMethod::Rebase, false),
                    (UpdateMethod::Rebase, false) => (UpdateMethod::Rebase, true),
                }
            }
            "up" => {
                (self.method, self.reset) = match (self.method, self.reset) {
                    (_, true) => (UpdateMethod::Rebase, false),
                    (UpdateMethod::Rebase, false) => (UpdateMethod::Merge, false),
                    (UpdateMethod::Merge, false) => (UpdateMethod::Rebase, true),
                }
            }
            "m" if event.keystroke.modifiers.alt => {
                (self.method, self.reset) = (UpdateMethod::Merge, false)
            }
            "r" if event.keystroke.modifiers.alt => {
                (self.method, self.reset) = (UpdateMethod::Rebase, false)
            }
            "e" if event.keystroke.modifiers.alt => self.reset = true,
            "enter" => self.run(cx),
            "escape" => cx.emit(UpdateEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for UpdateView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let choices = [
            (
                UpdateMethod::Merge,
                "Merge incoming changes into the current branch (⌥M)",
            ),
            (
                UpdateMethod::Rebase,
                "Rebase the current branch on top of incoming changes (⌥R)",
            ),
        ];
        div()
            .id("update-dialog")
            .track_focus(&self.focus)
            .occlude()
            .on_key_down(cx.listener(Self::on_key))
            .w(px(460.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Update Project"),
            )
            .children(choices.into_iter().enumerate().map(|(i, (m, label))| {
                let on = m == self.method && !self.reset;
                div()
                    .id(("update-method", i))
                    .px_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.))
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .child(if on { "◉" } else { "○" })
                    .child(label)
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.select(m, cx)
                    }))
            }))
            .child(
                div()
                    .id("update-reset")
                    .px_2()
                    .py_1()
                    .min_h(px(24.))
                    .flex()
                    .items_start()
                    .gap_2()
                    .rounded(px(4.))
                    .when(self.reset, |d| d.bg(c(p.tree_selection)))
                    .child(if self.reset { "◉" } else { "○" })
                    // The long label wraps inside the popup (it ran past its edge, OBS-0022
                    // re-check).
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .child(crate::vcs_ops_model::reset_choice_label(self.ahead)),
                    )
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.select_reset(cx)
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child("Local changes are stashed and restored around a rebase or reset. Enter updates, Escape cancels."),
            )
    }
}
