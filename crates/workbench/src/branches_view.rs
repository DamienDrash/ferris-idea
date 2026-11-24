//! The branch popup (`SPEC-0068`): filter, actions, recent, local and remote branches in folders,
//! tags, new branch, checkout and a submenu per branch (v5), favourites with a star (v8).

use std::collections::BTreeSet;
use std::ops::Range;

use ferris_idea_vcs::Branch;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::EditorView;
use crate::branches_model::{
    BranchAction, BranchList, BranchRow, FOOTER, Group, OpAction, PopupData, branch_actions,
    folder_key, initially_collapsed, row_name, short_label,
};
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchesEvent {
    /// Check out a branch; `true` creates it at HEAD first.
    Checkout(String, bool),
    /// Rebase the current branch onto this branch (SPEC-0106 behaviour 8).
    Rebase(String),
    /// Merge this branch into the current one (SPEC-0068 v3 behaviour 12).
    Merge(String),
    /// Continue, abort or skip the waiting operation (v3 behaviour 11).
    Operation(OpAction),
    /// Run a workbench action: Update Project, Commit, Push (v5 behaviour 15).
    Run(&'static str),
    /// Check out a tag or revision as a detached HEAD (v5 behaviour 16).
    CheckoutRevision(String),
    /// Create branch `.0` at `.1` and check it out (v5 behaviour 20).
    CreateFrom(String, String),
    /// Check out branch `.0` and rebase it onto `.1` (v5 behaviour 20).
    CheckoutAndRebase(String, String),
    /// Rename branch `.0` to `.1` (v5 behaviour 20).
    Rename(String, String),
    /// Delete the local branch (v5 behaviour 20).
    Delete(String),
    /// List the commits of `.0` and the current branch that the other lacks (v6 behaviour 23).
    CompareWith(String),
    /// List the files that differ between `.0` and the working tree (v6 behaviour 23).
    DiffWorkingTree(String),
    /// Open the push popup for local branch `.0` (v6 behaviour 23).
    PushBranch(String),
    /// Branch `.0` (remote-tracking when `.1`) was marked (`.2` true) or unmarked as a favourite;
    /// the popup stays open (v8 behaviours 26, 28, 29).
    Favorite(String, bool, bool),
    /// Close the popup.
    Close,
}

/// What the field takes instead of a filter.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Naming {
    /// The name of a new branch, at HEAD or at a start point (behaviour 7, v5 behaviour 20).
    NewBranch(Option<String>),
    /// The new name of a branch (v5 behaviour 20).
    Rename(String),
    /// A tag or revision to check out (v5 behaviour 16).
    Revision,
}

impl Naming {
    /// What the field holds when the naming starts: the old name for a rename.
    fn start_text(&self) -> String {
        match self {
            Naming::Rename(old) => old.clone(),
            _ => String::new(),
        }
    }
}

/// The local branch of a row: what `Git.Rename.Local.Branch` renames (v9 behaviour 33).
pub fn local_branch_of(row: Option<&BranchRow>) -> Option<String> {
    match row {
        Some(BranchRow::Branch(b)) if !b.remote => Some(b.name.clone()),
        _ => None,
    }
}

/// A branch's submenu (v5 behaviour 20).
struct Submenu {
    /// The row it belongs to.
    row: BranchRow,
    items: Vec<(BranchAction, String)>,
    selected: usize,
}

/// Branches of the repository with a filter.
pub struct BranchesView {
    filter: Entity<EditorView>,
    data: PopupData,
    list: BranchList,
    collapsed: BTreeSet<String>,
    naming: Option<Naming>,
    submenu: Option<Submenu>,
    scroll: UniformListScrollHandle,
    _watch: Subscription,
}

impl EventEmitter<BranchesEvent> for BranchesView {}

impl BranchesView {
    /// A popup over `branches` (current first, then local, then remote), with the rows of a
    /// waiting `operation` first.
    pub fn new(
        branches: Vec<Branch>,
        operation: Option<ferris_idea_vcs::Operation>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_data(PopupData::of(&branches, operation), cx)
    }

    /// A popup with tracking, recent branches and tags (v5).
    pub fn with_data(data: PopupData, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(EditorView::field);
        let watch = cx.observe(&filter, |this, _, cx| this.refilter(cx));
        let collapsed = initially_collapsed();
        let list = BranchList::build_from(&data, "", &collapsed);
        Self {
            filter,
            data,
            list,
            collapsed,
            naming: None,
            submenu: None,
            scroll: UniformListScrollHandle::new(),
            _watch: watch,
        }
    }

    /// Focus handle of the filter field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.filter.read(cx).focus()
    }

    /// The rows shown.
    pub fn list(&self) -> &BranchList {
        &self.list
    }

    /// Whether the field takes a name or revision instead of a filter.
    pub fn naming(&self) -> bool {
        self.naming.is_some()
    }

    /// The labels of the open submenu, if any.
    pub fn submenu(&self) -> Option<Vec<String>> {
        self.submenu
            .as_ref()
            .map(|s| s.items.iter().map(|(_, l)| l.clone()).collect())
    }

    /// Types `text` into the field (self-test).
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_text(text, cx));
        self.refilter(cx);
    }

    fn text(&self, cx: &gpui::App) -> String {
        self.filter.read(cx).state.buffer.text()
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let list = match &self.naming {
            Some(Naming::NewBranch(_)) => BranchList::naming(&self.data.branches, &text),
            Some(Naming::Rename(old)) if text.trim() == old => BranchList::default(),
            Some(Naming::Rename(_)) => BranchList::naming(&self.data.branches, &text),
            Some(Naming::Revision) => BranchList::revision(&text),
            None => BranchList::build_from(&self.data, &text, &self.collapsed),
        };
        if list != self.list {
            self.list = list;
            cx.notify();
        }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.submenu = None;
        self.rebuild(cx);
    }

    /// `Git.CreateNewBranch`: the field takes the name of a new branch at HEAD, as the
    /// "New Branch…" row does (v9 behaviour 32).
    pub fn name_new_branch(&mut self, cx: &mut Context<Self>) {
        self.start_naming(Naming::NewBranch(None), cx);
    }

    /// `Git.Rename.Local.Branch`: the field takes the new name of local branch `name`, as
    /// Rename… of its submenu does (v9 behaviour 33).
    pub fn rename_branch(&mut self, name: String, cx: &mut Context<Self>) {
        self.start_naming(Naming::Rename(name), cx);
    }

    /// The highlighted row's local branch, if it is one.
    pub fn highlighted_local(&self) -> Option<String> {
        local_branch_of(self.list.selected_row())
    }

    fn start_naming(&mut self, naming: Naming, cx: &mut Context<Self>) {
        let text = naming.start_text();
        self.naming = Some(naming);
        self.submenu = None;
        self.filter.update(cx, |f, cx| f.set_text(&text, cx));
        self.rebuild(cx);
    }

    fn stop_naming(&mut self, cx: &mut Context<Self>) {
        self.naming = None;
        self.filter.update(cx, |f, cx| f.set_text("", cx));
        self.rebuild(cx);
    }

    fn toggle_folder(&mut self, group: Group, prefix: &str, open: bool, cx: &mut Context<Self>) {
        let key = folder_key(group, prefix);
        if open {
            self.collapsed.insert(key);
        } else {
            self.collapsed.remove(&key);
        }
        let row = BranchRow::Folder(group, prefix.to_owned(), !open);
        self.rebuild(cx);
        self.list.selected = self.list.position(&row).or(self.list.selected);
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        if let Some(naming) = self.naming.clone() {
            let Some(BranchRow::New(text)) = self.list.selected_row().cloned() else {
                return;
            };
            cx.emit(match naming {
                Naming::NewBranch(None) => BranchesEvent::Checkout(text, true),
                Naming::NewBranch(Some(start)) => BranchesEvent::CreateFrom(text, start),
                Naming::Rename(old) => BranchesEvent::Rename(old, text),
                Naming::Revision => BranchesEvent::CheckoutRevision(text),
            });
            return;
        }
        match self.list.selected_row().cloned() {
            // "New Branch…": the field now takes the name (behaviour 7).
            Some(BranchRow::New(name)) if name.is_empty() => {
                self.start_naming(Naming::NewBranch(None), cx)
            }
            Some(BranchRow::New(name)) => cx.emit(BranchesEvent::Checkout(name, true)),
            Some(BranchRow::Branch(b)) if !b.current => {
                cx.emit(BranchesEvent::Checkout(b.name, false))
            }
            Some(BranchRow::Branch(_)) => cx.emit(BranchesEvent::Close),
            Some(BranchRow::Op(action, _)) => cx.emit(BranchesEvent::Operation(action)),
            Some(BranchRow::Action(a)) => match a.action() {
                Some(id) => cx.emit(BranchesEvent::Run(id)),
                None => self.start_naming(Naming::Revision, cx),
            },
            Some(BranchRow::Folder(group, prefix, open)) => {
                self.toggle_folder(group, &prefix, open, cx)
            }
            Some(BranchRow::Tag(t)) => cx.emit(BranchesEvent::CheckoutRevision(t.name)),
            Some(BranchRow::Header(_)) | None => {}
        }
    }

    /// Selects row `i` and acts on it as Enter does (for the self-test and clicks).
    pub fn choose_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.list.rows.len() {
            self.list.selected = Some(i);
            self.submenu = None;
            self.choose(cx);
            cx.notify();
        }
    }

    /// Selects the first row equal to `row`, if listed (self-test).
    pub fn select_row(&mut self, row: &BranchRow, cx: &mut Context<Self>) {
        if let Some(i) = self.list.position(row) {
            self.list.selected = Some(i);
            self.submenu = None;
            cx.notify();
        }
    }

    /// Selects the row of branch `name`, if listed.
    pub fn select_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(i) = self
            .list
            .rows
            .iter()
            .position(|r| matches!(r, BranchRow::Branch(b) if b.name == name))
        {
            self.list.selected = Some(i);
            cx.notify();
        }
    }

    /// Whether branch `name` (remote-tracking when `remote`) is a favourite (v8 behaviour 25).
    pub fn is_favorite(&self, name: &str, remote: bool) -> bool {
        self.data.favorites.is_favorite(name, remote)
    }

    /// Marks or unmarks the branch of row `i` as a favourite; the same branch stays selected and
    /// the popup open (v8 behaviours 26, 28, 29).
    pub fn toggle_favorite(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(BranchRow::Branch(b)) = self.list.rows.get(i).cloned() else {
            return;
        };
        let favorite = !self.data.favorite(&b);
        self.data.favorites.set(&b.name, b.remote, favorite);
        self.submenu = None;
        self.rebuild(cx);
        // The row may move; keep the same branch selected.
        let same = self.list.rows.get(i) == Some(&BranchRow::Branch(b.clone()));
        if !same {
            self.select_row(&BranchRow::Branch(b.clone()), cx);
        } else {
            self.list.selected = Some(i);
        }
        cx.emit(BranchesEvent::Favorite(b.name, b.remote, favorite));
        cx.notify();
    }

    /// Marks or unmarks the selected branch (Space, v8 behaviour 28); `false` when no branch is
    /// selected.
    pub fn toggle_selected_favorite(&mut self, cx: &mut Context<Self>) -> bool {
        match self.list.selected {
            Some(i) if matches!(self.list.rows.get(i), Some(BranchRow::Branch(_))) => {
                self.toggle_favorite(i, cx);
                true
            }
            _ => false,
        }
    }

    /// Merges the selected branch into the current one, as ⌘↩ does (v3 behaviour 12).
    pub fn merge_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(BranchRow::Branch(b)) = self.list.selected_row()
            && !b.current
        {
            cx.emit(BranchesEvent::Merge(b.name.clone()));
        }
    }

    /// Opens the submenu of the selected branch or tag (→, v5 behaviour 20); on a closed folder
    /// it opens the folder.
    pub fn open_submenu(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.list.selected_row().cloned() else {
            return;
        };
        if let BranchRow::Folder(group, prefix, false) = &row {
            self.toggle_folder(*group, prefix, false, cx);
            return;
        }
        let items = branch_actions(&row, self.data.current(), self.data.row_favorite(&row));
        if !items.is_empty() {
            self.submenu = Some(Submenu {
                row,
                items,
                selected: 0,
            });
            cx.notify();
        }
    }

    /// Runs entry `i` of the open submenu.
    pub fn choose_submenu(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.submenu.take() else {
            return;
        };
        let Some(action) = menu.items.get(i).map(|(a, _)| *a) else {
            return;
        };
        let Some(name) = row_name(&menu.row).map(str::to_owned) else {
            return;
        };
        let current = self.data.current().unwrap_or("HEAD").to_owned();
        match action {
            BranchAction::Checkout => match &menu.row {
                BranchRow::Tag(_) => cx.emit(BranchesEvent::CheckoutRevision(name)),
                _ => cx.emit(BranchesEvent::Checkout(name, false)),
            },
            BranchAction::NewBranchFrom => self.start_naming(Naming::NewBranch(Some(name)), cx),
            BranchAction::CheckoutAndRebase => {
                cx.emit(BranchesEvent::CheckoutAndRebase(name, current))
            }
            BranchAction::RebaseOnto => cx.emit(BranchesEvent::Rebase(name)),
            BranchAction::MergeInto => cx.emit(BranchesEvent::Merge(name)),
            BranchAction::Push if name == current => cx.emit(BranchesEvent::Run("Vcs.Push")),
            BranchAction::Push => cx.emit(BranchesEvent::PushBranch(name)),
            BranchAction::CompareWith => cx.emit(BranchesEvent::CompareWith(name)),
            BranchAction::DiffWorkingTree => cx.emit(BranchesEvent::DiffWorkingTree(name)),
            BranchAction::Rename => self.start_naming(Naming::Rename(name), cx),
            BranchAction::Delete => cx.emit(BranchesEvent::Delete(name)),
            BranchAction::Favorite => {
                let at = self
                    .list
                    .selected
                    .filter(|&i| self.list.rows.get(i) == Some(&menu.row));
                match at.or_else(|| self.list.position(&menu.row)) {
                    Some(i) => self.toggle_favorite(i, cx),
                    None => return,
                }
            }
        }
        cx.notify();
    }

    /// Keys of an open submenu: Up/Down, Enter or Right runs, Left or Escape closes.
    fn submenu_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.submenu.as_mut() else {
            return false;
        };
        let n = menu.items.len();
        match key {
            "up" => menu.selected = (menu.selected + n - 1) % n,
            "down" => menu.selected = (menu.selected + 1) % n,
            "enter" | "right" => {
                let i = menu.selected;
                self.choose_submenu(i, cx);
            }
            "left" | "escape" => self.submenu = None,
            _ => return false,
        }
        true
    }

    /// Up/Down select, Enter checks out, Right opens the submenu or a folder, Left closes a
    /// folder, Alt+Enter rebases onto the branch, Cmd+Enter merges it, Escape closes (behaviours
    /// 3–5, v3 behaviour 12, v5 behaviour 20; SPEC-0106 behaviour 8).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let plain = !(m.control || m.alt || m.platform || m.shift);
        if plain && self.submenu_key(k.key.as_str(), cx) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.key == "enter" && m.platform && !m.control && !m.alt && !m.shift {
            self.merge_selected(cx);
            cx.stop_propagation();
            return;
        }
        if k.key == "enter" && m.alt && !m.control && !m.platform && !m.shift {
            if let Some(BranchRow::Branch(b)) = self.list.selected_row()
                && !b.current
            {
                cx.emit(BranchesEvent::Rebase(b.name.clone()));
            }
            cx.stop_propagation();
            return;
        }
        // Right and Left move the caret while a name is typed or the filter is not empty.
        let browsing = self.naming.is_none() && self.text(cx).is_empty();
        // The first and the last row that is no heading (`SPEC-0275` v19 behaviour 47); Home
        // and End stay the field's while it holds text.
        if self.submenu.is_none()
            && let Some(to) = crate::list_nav::edge_among(
                k,
                self.list.rows.len(),
                crate::list_nav::Keys::beside_field(browsing),
                |i| !matches!(self.list.rows[i], BranchRow::Header(_)),
            )
        {
            self.list.selected = Some(to);
            self.scroll.scroll_to_item(to, gpui::ScrollStrategy::Center);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if !plain {
            return;
        }
        match k.key.as_str() {
            "up" | "down" => {
                self.list.step(k.key == "down");
                if let Some(i) = self.list.selected {
                    self.scroll.scroll_to_item(i, gpui::ScrollStrategy::Center);
                }
            }
            "right" if browsing => self.open_submenu(cx),
            "left" if browsing => {
                let Some(i) = self.list.selected else {
                    return;
                };
                let folder = match &self.list.rows[i] {
                    BranchRow::Folder(..) => Some(i),
                    _ => self.list.parent_folder(i),
                };
                match folder.map(|f| self.list.rows[f].clone()) {
                    Some(BranchRow::Folder(group, prefix, true)) => {
                        self.toggle_folder(group, &prefix, true, cx)
                    }
                    _ => return,
                }
            }
            "enter" => self.choose(cx),
            // Space marks or unmarks the selected branch; branch names have no spaces, so the
            // filter never needs one (v8 behaviour 28). While a name is typed it is typed.
            "space" if self.naming.is_none() => {
                if !self.toggle_selected_favorite(cx) {
                    return;
                }
            }
            // Escape while naming goes back to the branch list.
            "escape" if self.naming.is_some() => self.stop_naming(cx),
            "escape" => cx.emit(BranchesEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn placeholder(&self) -> String {
        match &self.naming {
            Some(Naming::NewBranch(None)) => "Name of the new branch, then Enter".into(),
            Some(Naming::NewBranch(Some(from))) => {
                format!("Name of the new branch from '{from}', then Enter")
            }
            Some(Naming::Rename(old)) => format!("New name of '{old}', then Enter"),
            Some(Naming::Revision) => "Tag, branch or commit to check out, then Enter".into(),
            None => "Filter branches".into(),
        }
    }

    fn confirm_label(&self, text: &str) -> String {
        match &self.naming {
            Some(Naming::NewBranch(Some(from))) => format!("New Branch '{text}' from '{from}'"),
            Some(Naming::Rename(old)) => format!("Rename '{old}' to '{text}'"),
            Some(Naming::Revision) => format!("Checkout '{text}' (detached HEAD)"),
            _ if text.is_empty() => "New Branch…".to_owned(),
            _ => format!("New Branch '{text}'"),
        }
    }
}

impl Render for BranchesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let rows = self.list.rows.clone();
        let depth = self.list.depth.clone();
        let selected = self.list.selected;
        let height = (rows.len().clamp(1, 16) as f32) * 26.0;
        let empty = self.filter.read(cx).state.buffer.is_empty();
        let placeholder = self.placeholder();
        let labels: Vec<String> = rows
            .iter()
            .map(|r| match r {
                BranchRow::New(t) => self.confirm_label(t),
                _ => String::new(),
            })
            .collect();
        let hints: Vec<Option<String>> = rows
            .iter()
            .map(|r| match r {
                BranchRow::Branch(b) if !b.remote => self.data.tracking_hint(&b.name),
                _ => None,
            })
            .collect();
        let tag_count = self.data.tags.len();
        let favorites: Vec<bool> = rows.iter().map(|r| self.data.row_favorite(r)).collect();
        let submenu_for = self.submenu.as_ref().map(|s| s.row.clone());
        let list = uniform_list(
            "branches",
            rows.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let on = Some(i) == selected;
                        // Hints stay readable on the selection (hand test 2026-11-01).
                        let hint = c(if on { p.ui_text } else { p.ui_muted });
                        let indent = px(12. + 16. * f32::from(depth.get(i).copied().unwrap_or(0)));
                        let row = div()
                            .id(("br", i))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(26.))
                            .pl(indent)
                            .pr_3()
                            .overflow_hidden()
                            .whitespace_nowrap();
                        let pick = |d: gpui::Stateful<gpui::Div>| {
                            d.mx_1()
                                .rounded(px(4.))
                                .when(on, |d| d.bg(c(p.tree_selection)))
                                .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                .on_click(cx.listener(
                                    move |this, _: &gpui::ClickEvent, window, cx| {
                                        this.list.selected = Some(i);
                                        this.submenu = None;
                                        this.choose(cx);
                                        // A click must not take the keys from the field, which
                                        // now may take the new branch's name (hand test).
                                        window.focus(&this.filter.read(cx).focus());
                                        cx.notify();
                                    },
                                ))
                        };
                        // The chevron of a row with a submenu opens it (v5 behaviour 20).
                        let more = |row: &BranchRow| {
                            let open = submenu_for.as_ref() == Some(row);
                            div()
                                .id(("br-more", i))
                                .flex_none()
                                .rounded(px(3.))
                                .when(open, |d| d.bg(c(p.toolbar_hover)))
                                .hover(|s| s.bg(c(p.toolbar_hover)))
                                .child(icon("general/chevronRight"))
                                .on_click(cx.listener(
                                    move |this, _: &gpui::ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        this.list.selected = Some(i);
                                        this.open_submenu(cx);
                                        window.focus(&this.filter.read(cx).focus());
                                    },
                                ))
                        };
                        match &rows[i] {
                            BranchRow::Header(title) => row
                                .mx_1()
                                .mt_1()
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(c(p.ui_muted))
                                .child(*title),
                            BranchRow::New(_) => pick(
                                row.child(div().w(px(16.)).flex_none().child(icon("general/add")))
                                    .child(labels[i].clone()),
                            ),
                            BranchRow::Op(_, label) => pick(
                                row.child(div().w(px(16.)).flex_none().text_color(hint).child("›"))
                                    .child(label.clone()),
                            ),
                            BranchRow::Action(a) => {
                                // The action's shortcut on the right, as in the reference (v7).
                                let shortcut = a
                                    .action()
                                    .map(|id| {
                                        crate::context_menu::shortcut_label(
                                            id,
                                            &crate::keymaps::current(),
                                            cfg!(target_os = "macos"),
                                        )
                                    })
                                    .unwrap_or_default();
                                pick(
                                    row.child(icon(a.icon()))
                                        .child(div().flex_1().child(a.label()))
                                        .child(div().text_color(hint).child(shortcut)),
                                )
                            }
                            BranchRow::Folder(group, prefix, open) => pick(
                                row.child(icon(if *open {
                                    "general/chevronDown"
                                } else {
                                    "general/chevronRight"
                                }))
                                .child(icon(if *group == Group::Tags {
                                    "ferris-idea/tag"
                                } else {
                                    "nodes/folder"
                                }))
                                .child(
                                    if *group == Group::Tags {
                                        format!("Tags ({tag_count})")
                                    } else {
                                        prefix.clone()
                                    },
                                ),
                            ),
                            BranchRow::Tag(t) => {
                                let r = rows[i].clone();
                                pick(
                                    row.child(icon("ferris-idea/tag"))
                                        .child(div().flex_none().child(t.name.clone()))
                                        .child(div().flex_1())
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(hint)
                                                .child(t.target.clone()),
                                        )
                                        .child(more(&r)),
                                )
                            }
                            BranchRow::Branch(b) => {
                                let r = rows[i].clone();
                                let d = depth.get(i).copied().unwrap_or(0);
                                // The star: filled on a favourite, an outline on the selected or
                                // hovered row, else an empty slot (v8 behaviour 26).
                                let fav = favorites[i];
                                let group = format!("br-row-{i}");
                                let star = div()
                                    .id(("br-star", i))
                                    .flex_none()
                                    .size(px(16.))
                                    .when(fav, |d| d.child(icon("ferris-idea/star")))
                                    .when(!fav, |d| {
                                        d.when(!on, |d| {
                                            d.opacity(0.)
                                                .group_hover(group.clone(), |s| s.opacity(1.))
                                        })
                                        .child(icon("ferris-idea/starOutline"))
                                    })
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.toggle_favorite(i, cx);
                                            window.focus(&this.filter.read(cx).focus());
                                        },
                                    ));
                                pick(
                                    row.group(group)
                                        .child(star)
                                        .child(icon("general/vcs"))
                                        .child(
                                            div()
                                                .flex_none()
                                                .when(b.current, |d| {
                                                    d.font_weight(gpui::FontWeight::SEMIBOLD)
                                                })
                                                .child(short_label(&b.name, d).to_owned()),
                                        )
                                        .child(div().flex_1())
                                        .when_some(hints[i].clone(), |d, h| {
                                            d.child(div().flex_none().text_color(hint).child(h))
                                        })
                                        .when(b.current, |d| {
                                            d.child(
                                                div().flex_none().text_color(hint).child("current"),
                                            )
                                        })
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(hint)
                                                .child(b.target.clone()),
                                        )
                                        .child(more(&r)),
                                )
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .h(px(height));
        let main = div()
            .w(px(440.))
            .flex()
            .flex_col()
            .child(
                div()
                    .m_2()
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c(p.separator))
                    .gap_1()
                    .when(self.naming.is_none(), |d| d.child(icon("general/search")))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(18.))
                            .overflow_hidden()
                            .child(self.filter.clone())
                            .when(empty, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left(px(2.))
                                        .h(px(18.))
                                        .flex()
                                        .items_center()
                                        .text_color(c(p.ui_dim))
                                        .child(placeholder),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .py_1()
                    .child(list),
            )
            .when(self.naming.is_none(), |d| {
                d.child(
                    div()
                        .border_t_1()
                        .border_color(c(p.separator))
                        .px_3()
                        .py_1()
                        .text_size(px(11.))
                        .text_color(c(p.ui_muted))
                        .child(FOOTER),
                )
            });
        let submenu = self.submenu.as_ref().map(|menu| {
            let chosen = menu.selected;
            div()
                .id("branches-submenu")
                .w(px(300.))
                .py_1()
                .border_l_1()
                .border_color(c(p.separator))
                .flex()
                .flex_col()
                .children(menu.items.iter().enumerate().map(|(i, (_, label))| {
                    div()
                        .id(("br-sub", i))
                        .h(px(26.))
                        .mx_1()
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when(i == chosen, |d| d.bg(c(p.tree_selection)))
                        .when(i != chosen, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(label.clone())
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                            this.choose_submenu(i, cx);
                            window.focus(&this.filter.read(cx).focus());
                        }))
                }))
        });
        div()
            .id("branches-popup")
            .occlude()
            .flex()
            .flex_row()
            .bg(c(p.popup))
            .border_1()
            .border_color(c(p.popup_border))
            .rounded(px(8.))
            .shadow_lg()
            .overflow_hidden()
            .capture_key_down(cx.listener(Self::on_key))
            .child(main)
            .children(submenu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branches_model::PopupAction;

    #[test]
    fn events_are_comparable() {
        assert_ne!(
            BranchesEvent::Delete("a".into()),
            BranchesEvent::Rename("a".into(), "b".into())
        );
        assert_eq!(PopupAction::Push.label(), "Push…");
    }

    /// SPEC-0068 AK-19 (v9).
    #[test]
    fn naming_started_by_the_actions() {
        let branch = |name: &str, remote: bool| {
            BranchRow::Branch(ferris_idea_vcs::Branch {
                name: name.into(),
                remote,
                current: false,
                target: "abc1234".into(),
            })
        };
        // Rename takes the highlighted local branch and nothing else.
        assert_eq!(
            local_branch_of(Some(&branch("feature/x", false))),
            Some("feature/x".to_owned())
        );
        assert_eq!(local_branch_of(Some(&branch("origin/main", true))), None);
        assert_eq!(local_branch_of(Some(&BranchRow::Header("Local"))), None);
        assert_eq!(local_branch_of(None), None);
        // The field starts with the old name for a rename and empty for a new branch.
        assert_eq!(Naming::Rename("feature/x".into()).start_text(), "feature/x");
        assert_eq!(Naming::NewBranch(None).start_text(), "");
    }
}
