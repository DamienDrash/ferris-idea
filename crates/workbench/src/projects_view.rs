//! The project popup of the main toolbar (`SPEC-0094` v5): New Project…, Open…, Clone
//! Repository…, Remote Development…, the projects open in windows and the recent projects; the
//! forms of New Project and Clone Repository and a clone's progress.

use std::path::PathBuf;

use gpui::{Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::EditorView;
use crate::icon_images::icon;
use crate::palette::Palette;
use crate::project_popup_model::{PopupRow, clone_plan, new_project_plan, popup_rows};
use crate::workbench_view::c;

/// What the popup asks the workbench to do.
pub enum ProjectsEvent {
    /// Choose a folder in the system dialog and open it.
    Browse,
    /// Open a folder on an SSH host as a remote project (`SPEC-0317` v3 behaviour 1).
    RemoteDevelopment,
    /// Open this project, or bring its window to the front.
    Open(PathBuf),
    /// Clone the checked address into the new folder (behaviour 13).
    Clone(idecraft_vcs::CloneSource, PathBuf),
    /// Cancel the running clone.
    CancelClone,
    /// Make the empty project folder, with `git init` when true (behaviour 14).
    NewProject(PathBuf, bool),
    /// Close the popup.
    Close,
}

/// Which form the popup shows instead of its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Address and folder of Clone Repository.
    Clone,
    /// Location, name and `git init` of New Project.
    NewProject,
}

/// The popup (behaviour 1).
pub struct ProjectsView {
    rows: Vec<PopupRow>,
    selected: usize,
    focus: FocusHandle,
    form: Option<Form>,
    /// Clone: the address; New Project: the name.
    first: Entity<EditorView>,
    /// The folder the new project or clone goes in.
    location: Entity<EditorView>,
    git_init: bool,
    error: Option<String>,
    /// The running clone's progress text, shown above the list with Cancel.
    cloning: Option<String>,
    default_location: String,
}

impl EventEmitter<ProjectsEvent> for ProjectsView {}

/// "~/…" for paths in the home folder.
fn short(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl ProjectsView {
    /// A popup for the window of `current` with the projects `open` in windows and the `recent`
    /// ones, most recent first.
    pub fn new(
        open: Vec<PathBuf>,
        current: PathBuf,
        recent: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            rows: popup_rows(&open, &current, &recent),
            selected: 0,
            focus: cx.focus_handle(),
            form: None,
            first: cx.new(EditorView::field),
            location: cx.new(EditorView::field),
            git_init: true,
            error: None,
            cloning: None,
            default_location: crate::project_popup_model::default_location(&current),
        }
    }

    /// Focus handle of the popup.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The rows as shown: the actions' titles, then each project's name (an open one's current
    /// window marked "(current)").
    pub fn rows(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|r| match r {
                PopupRow::OpenProject { root, current } if *current => {
                    format!("{} (current)", name_of(root))
                }
                PopupRow::OpenProject { root, .. } | PopupRow::Recent(root) => name_of(root),
                action => action.title().unwrap_or_default().to_owned(),
            })
            .collect()
    }

    /// The form shown, if any.
    pub fn form(&self) -> Option<Form> {
        self.form
    }

    /// Why the form cannot be carried out, after the last try.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The running clone's progress, as shown.
    pub fn cloning(&self) -> Option<&str> {
        self.cloning.as_deref()
    }

    /// Shows the running clone's progress (`None`: no clone runs) and, after a failed one, why.
    pub fn set_cloning(
        &mut self,
        progress: Option<String>,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.cloning = progress;
        if error.is_some() {
            self.error = error;
        }
        cx.notify();
    }

    /// Fills the form's fields (for tests and agents): the address or name, and the folder.
    pub fn fill(&mut self, first: &str, location: &str, git_init: bool, cx: &mut Context<Self>) {
        self.first.update(cx, |f, cx| f.set_text(first, cx));
        self.location.update(cx, |f, cx| f.set_text(location, cx));
        self.git_init = git_init;
        cx.notify();
    }

    fn open_form(&mut self, form: Form, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.form = Some(form);
        self.error = None;
        self.first.update(cx, |f, cx| f.set_text("", cx));
        let location = self.default_location.clone();
        self.location.update(cx, |f, cx| f.set_text(&location, cx));
        if let Some(window) = window {
            window.focus(&self.first.read(cx).focus());
        }
        cx.notify();
    }

    /// Chooses row `i` (behaviour 2); New Project and Clone Repository open their form.
    pub fn choose(&mut self, i: usize, cx: &mut Context<Self>) {
        self.choose_in(i, None, cx);
    }

    fn choose_in(&mut self, i: usize, window: Option<&mut Window>, cx: &mut Context<Self>) {
        match self.rows.get(i).cloned() {
            Some(PopupRow::NewProject) => self.open_form(Form::NewProject, window, cx),
            Some(PopupRow::Clone) => self.open_form(Form::Clone, window, cx),
            Some(PopupRow::Open) => cx.emit(ProjectsEvent::Browse),
            Some(PopupRow::RemoteDevelopment) => cx.emit(ProjectsEvent::RemoteDevelopment),
            Some(PopupRow::OpenProject { root, .. } | PopupRow::Recent(root)) => {
                cx.emit(ProjectsEvent::Open(root))
            }
            None => {}
        }
    }

    /// Carries out the form: clone or make the project, or names why it cannot (behaviours 13
    /// and 14). A second clone waits until the first ends.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let first = self.first.read(cx).state.buffer.text();
        let location = self.location.read(cx).state.buffer.text();
        let result = match self.form {
            Some(Form::Clone) if self.cloning.is_some() => {
                Err("A clone is running; cancel it or wait until it ends.".to_owned())
            }
            Some(Form::Clone) => clone_plan(&first, &location)
                .map(|(source, target)| ProjectsEvent::Clone(source, target)),
            Some(Form::NewProject) => new_project_plan(&location, &first)
                .map(|target| ProjectsEvent::NewProject(target, self.git_init)),
            None => return,
        };
        match result {
            Ok(event) => {
                self.error = None;
                cx.emit(event);
            }
            Err(why) => self.error = Some(why),
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if self.form.is_some() {
            match key {
                "enter" => self.submit(cx),
                "escape" => {
                    self.form = None;
                    self.error = None;
                    window.focus(&self.focus);
                }
                "tab" => {
                    let next = if self.first.read(cx).focus().is_focused(window) {
                        self.location.read(cx).focus()
                    } else {
                        self.first.read(cx).focus()
                    };
                    window.focus(&next);
                }
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let n = self.rows.len();
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(&event.keystroke, n) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match key {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "enter" => self.choose_in(self.selected, Some(window), cx),
            "escape" => cx.emit(ProjectsEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn render_form(&mut self, form: Form, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let field = |label: &'static str, editor: Entity<EditorView>| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .child(
                    div()
                        .w(px(80.))
                        .flex_none()
                        .text_color(c(p.ui_muted))
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(24.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.separator))
                        .child(div().flex_1().min_w_0().h(px(18.)).child(editor)),
                )
        };
        let first = self.first.read(cx).state.buffer.text();
        let location = self.location.read(cx).state.buffer.text();
        let (title, first_label, target, action) = match form {
            Form::Clone => (
                "Clone Repository",
                "URL",
                clone_plan(&first, &location).ok().map(|(_, t)| t),
                "Clone",
            ),
            Form::NewProject => (
                "New Project",
                "Name",
                new_project_plan(&location, &first).ok(),
                "Create",
            ),
        };
        let button = |id: &'static str, text: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded(px(4.))
                .when(primary, |d| d.bg(c(p.accent)).text_color(c(p.accent_text)))
                .when(!primary, |d| d.border_1().border_color(c(p.separator)))
                .child(text)
        };
        div()
            .flex()
            .flex_col()
            .py_1()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(title),
            )
            .child(field(first_label, self.first.clone()))
            .child(field("Location", self.location.clone()))
            .when(form == Form::NewProject, |d| {
                let on = self.git_init;
                d.child(
                    div()
                        .id("project-git-init")
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .child(if on { "☑" } else { "☐" })
                        .child("Create Git repository")
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            this.git_init = !this.git_init;
                            cx.notify();
                        })),
                )
            })
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(match &target {
                        Some(t) => format!("Into {}", short(t)),
                        None => {
                            "Only HTTPS and SSH; no password or token in the address.".to_owned()
                        }
                    }),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(c(p.error_text))
                        .child(e),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(
                        button("project-form-cancel", "Cancel", false).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, window, cx| {
                                this.form = None;
                                this.error = None;
                                window.focus(&this.focus);
                                cx.notify();
                            },
                        )),
                    )
                    .child(button("project-form-ok", action, true).on_click(
                        cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.submit(cx)),
                    )),
            )
            .into_any_element()
    }
}

impl Render for ProjectsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let row = |i: usize| {
            div()
                .id(("project-row", i))
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.))
                .mx_1()
                .px_2()
                .rounded(px(4.))
                .when(i == selected, |d| d.bg(c(p.tree_selection)))
                .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
        };
        let heading = |text: &'static str| {
            div()
                .px_3()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(c(p.ui_dim))
                .child(text)
        };
        let popup = div()
            .id("projects-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(420.))
            .py_1()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg();
        let popup = popup.when_some(self.cloning.clone(), |d, text| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(div().flex_1().min_w_0().child(text))
                    .child(
                        div()
                            .id("project-clone-cancel")
                            .px_2()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(p.separator))
                            .child("Cancel")
                            .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(ProjectsEvent::CancelClone)
                            })),
                    ),
            )
        });
        if let Some(form) = self.form {
            return popup.child(self.render_form(form, cx));
        }
        let mut popup = popup;
        let mut open_heading = false;
        let mut recent_heading = false;
        for (i, r) in self.rows.clone().into_iter().enumerate() {
            let choose = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.choose_in(i, Some(window), cx)
            });
            match r {
                PopupRow::OpenProject { .. } if !open_heading => {
                    open_heading = true;
                    popup = popup.child(heading("Open Projects"));
                }
                PopupRow::Recent(_) if !recent_heading => {
                    recent_heading = true;
                    popup = popup.child(heading("Recent Projects"));
                }
                _ => {}
            }
            popup = popup.child(match &r {
                PopupRow::OpenProject { root, .. } | PopupRow::Recent(root) => {
                    let name = name_of(root);
                    let current = matches!(r, PopupRow::OpenProject { current: true, .. });
                    row(i)
                        .child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .rounded(px(4.))
                                .bg(c(p.accent))
                                .text_color(c(p.accent_text))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(crate::chrome::project_initials(&name)),
                        )
                        .child(div().flex_none().child(name))
                        .when(current, |d| {
                            d.child(div().flex_none().text_color(c(p.ui_dim)).child("current"))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(c(p.ui_dim))
                                .child(short(root)),
                        )
                        .on_click(choose)
                }
                action => {
                    let glyph = match action {
                        PopupRow::NewProject => "general/add",
                        PopupRow::Clone => "toolwindows/commit",
                        PopupRow::RemoteDevelopment => "idecraft/remote",
                        _ => "nodes/folder",
                    };
                    row(i)
                        .child(icon(glyph))
                        .child(action.title().unwrap_or_default())
                        .on_click(choose)
                }
            });
        }
        if !recent_heading {
            popup = popup.child(heading("No recent projects"));
        }
        popup
    }
}
