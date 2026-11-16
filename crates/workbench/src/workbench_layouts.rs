//! Named tool window layouts in the window (`SPEC-0333`): saving the current arrangement under a
//! name, applying and restoring a saved layout, renaming and deleting one, and the Layouts
//! submenu of the Window menu.
//!
//! The rules, the names and the user's file are [`idecraft_ui::window_layouts`], the same code as
//! the MCP tool `window_layouts`. The file is read again before every action, so two windows do
//! not overwrite each other's layouts (behaviour 15).

use std::path::PathBuf;

use gpui::{Context, Window};
use idecraft_ui::tool_windows::ToolWindow;
use idecraft_ui::window_layouts::{self as model, Applied, Arrangement, Layouts};

use super::{DrawnFor, WorkbenchView};

impl WorkbenchView {
    /// Keeps the saved layouts in `file` instead of the user's settings folder (tests).
    pub fn set_window_layouts_file(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.window_layouts_file = Some(file);
        let layouts = self.window_layouts();
        Self::show_layout_menu(&layouts, cx);
    }

    /// The saved layouts as the file holds them now; none without a settings folder.
    pub fn window_layouts(&self) -> Layouts {
        self.window_layouts_file
            .as_deref()
            .map(Layouts::load)
            .unwrap_or_default()
    }

    /// The window's arrangement as a layout would save it (behaviour 1): the layout, the shown
    /// tool windows — the tree also while Commit covers it — and the focused one.
    pub fn arrangement(&self, window: &Window, cx: &gpui::App) -> Arrangement {
        let mut shown = self.shown_tool_windows();
        if self.show_tree && !shown.contains(&ToolWindow::Project) {
            shown.insert(0, ToolWindow::Project);
        }
        Arrangement::new(
            &self.tool_layout,
            &shown,
            self.focused_tool_window(window, cx),
        )
    }

    /// The Layouts submenu shows `layouts` (behaviour 12).
    fn show_layout_menu(layouts: &Layouts, cx: &mut Context<Self>) {
        cx.set_global(crate::menu_bar::LayoutMenu(layouts.menu()));
        crate::menu_bar::install(cx);
    }

    /// The file and its layouts for an action that changes them; `None`, with the reason in the
    /// status line, without a settings folder.
    fn layouts_to_change(&mut self) -> Option<(PathBuf, Layouts)> {
        let Some(file) = self.window_layouts_file.clone() else {
            self.status = Some("Layouts need a settings folder (HOME is not set)".into());
            return None;
        };
        let layouts = Layouts::load(&file);
        Some((file, layouts))
    }

    /// Writes `layouts`; a failure is the status line's text and the error returned.
    fn store_layouts(
        &mut self,
        file: &std::path::Path,
        layouts: &Layouts,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        layouts
            .save(file)
            .map_err(|e| format!("Layouts not saved: {e}"))?;
        // The file is whole again; the menu shows what it holds.
        Self::show_layout_menu(&Layouts::load(file), cx);
        Ok(())
    }

    /// The layout actions by id (behaviours 4–11); false for another id.
    pub(super) fn run_layout_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            "RestoreFactoryDefaultLayout" => self.select_layout(None, window, cx),
            "RestoreDefaultLayout" => self.restore_current_layout(window, cx),
            "StoreDefaultLayout" => self.save_layout_changes(window, cx),
            "StoreNewLayout" => {
                let arrangement = self.arrangement(window, cx);
                self.start_new_layout_prompt(arrangement, window, cx);
            }
            _ => {
                if let Some(name) = id.strip_prefix(model::APPLY) {
                    self.select_layout(Some(name), window, cx);
                } else if let Some(name) = id.strip_prefix(model::RENAME) {
                    match self.window_layouts().get(name).map(|(n, _)| n.to_owned()) {
                        Some(saved) => self.start_rename_layout_prompt(saved, window, cx),
                        None => self.status = Some(format!("There is no layout named \"{name}\"")),
                    }
                } else if let Some(name) = id.strip_prefix(model::DELETE) {
                    self.ask_delete_layout(name);
                } else {
                    return false;
                }
            }
        }
        cx.notify();
        true
    }

    /// Default and Apply: selects the layout `name` (`None`: the default one) and arranges the
    /// window by it (behaviours 6 and 7).
    pub fn select_layout(
        &mut self,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((file, mut layouts)) = self.layouts_to_change() else {
            return;
        };
        let before = layouts.selected().map(str::to_owned);
        let applied = match layouts.select(name) {
            Ok(applied) => applied,
            Err(e) => {
                self.status = Some(e.to_string());
                return;
            }
        };
        let (changed, unsaved) = self.apply_arrangement(&applied, window, cx);
        let mut status = match (&applied.name, changed) {
            (Some(name), _) => format!("Layout \"{name}\" applied"),
            (None, true) => "Tool windows are back in the default layout".into(),
            (None, false) => "Tool windows are in the default layout".into(),
        };
        // The selection is the only thing to write; a file that cannot be read stays as it is.
        if let Some(problem) = &layouts.problem {
            status = problem.message();
        } else if before.as_deref() != layouts.selected()
            && let Err(e) = self.store_layouts(&file, &layouts, cx)
        {
            status = e;
        }
        self.status = Some(unsaved.unwrap_or(status));
        cx.notify();
    }

    /// `RestoreDefaultLayout`: the selected layout as it was saved (behaviour 8).
    pub fn restore_current_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layouts = self.window_layouts();
        let applied = layouts.current();
        let (changed, unsaved) = self.apply_arrangement(&applied, window, cx);
        let mut status = match (&applied.name, changed) {
            (Some(name), _) => format!("Layout \"{name}\" restored"),
            (None, true) => "Tool windows are back in the default layout".into(),
            (None, false) => "Tool windows are in the default layout".into(),
        };
        if let Some(problem) = &layouts.problem {
            status = problem.message();
        }
        self.status = Some(unsaved.unwrap_or(status));
        cx.notify();
    }

    /// `StoreDefaultLayout`: the window's arrangement into the selected layout (behaviour 9).
    pub fn save_layout_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let arrangement = self.arrangement(window, cx);
        let Some((file, mut layouts)) = self.layouts_to_change() else {
            return;
        };
        let saved = layouts
            .save_changes(arrangement)
            .map_err(|e| e.to_string())
            .and_then(|name| self.store_layouts(&file, &layouts, cx).map(|()| name));
        self.status = Some(match saved {
            Ok(name) => format!("Layout \"{name}\" saved"),
            Err(e) => e,
        });
        cx.notify();
    }

    /// Enter in the name field of Save Current Layout as New…: saves `arrangement` as `name`
    /// and selects it (behaviours 4 and 5). The error keeps the field open.
    pub(super) fn save_new_layout(
        &mut self,
        name: &str,
        arrangement: Arrangement,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some((file, mut layouts)) = self.layouts_to_change() else {
            return Err(self.status.clone().unwrap_or_default());
        };
        let name = layouts
            .save_new(name, arrangement)
            .map_err(|e| e.to_string())?;
        self.store_layouts(&file, &layouts, cx)?;
        self.status = Some(format!("Layout \"{name}\" saved"));
        Ok(())
    }

    /// Enter in the name field of Rename… (behaviour 10).
    pub(super) fn rename_layout(
        &mut self,
        old: &str,
        new: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some((file, mut layouts)) = self.layouts_to_change() else {
            return Err(self.status.clone().unwrap_or_default());
        };
        if let Some(new) = layouts.rename(old, new).map_err(|e| e.to_string())? {
            self.store_layouts(&file, &layouts, cx)?;
            self.status = Some(format!("Layout \"{old}\" renamed to \"{new}\""));
        }
        Ok(())
    }

    /// Delete…: asks first; the selected layout is refused with the reason (behaviour 11).
    pub fn ask_delete_layout(&mut self, name: &str) {
        match self.window_layouts().deletable(name) {
            Ok(saved) => {
                self.drawn_question = Some((
                    crate::dialogs::DrawnQuestion {
                        message: format!("Delete layout \"{saved}\"?"),
                        detail: "The tool windows stay as they are.".into(),
                        buttons: vec!["Delete".into(), "Cancel".into()],
                        default: 1,
                        cancel: Some(1),
                    },
                    DrawnFor::DeleteLayout(saved),
                ));
            }
            Err(e) => self.status = Some(e.to_string()),
        }
    }

    /// The Delete answer of the question (behaviour 11).
    pub(super) fn delete_layout(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some((file, mut layouts)) = self.layouts_to_change() else {
            return;
        };
        let deleted = layouts
            .delete(name)
            .map_err(|e| e.to_string())
            .and_then(|name| self.store_layouts(&file, &layouts, cx).map(|()| name));
        self.status = Some(match deleted {
            Ok(name) => format!("Layout \"{name}\" deleted"),
            Err(e) => e,
        });
        cx.notify();
    }

    /// Arranges the window by `applied` and saves the project's layout file; returns whether the
    /// layout of the tool windows changed, and why the project's file could not be saved if it
    /// could not (behaviours 6 and 7).
    fn apply_arrangement(
        &mut self,
        applied: &Applied,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (bool, Option<String>) {
        let before = Arrangement::new(&self.tool_layout, &[], None).layout;
        let changed = before != applied.layout;
        self.tool_layout = applied.layout.clone();
        self.maximized = None;
        match &applied.shown {
            // The default layout keeps the shown windows, one per placement (SPEC-0167 v5
            // behaviour 17).
            None => {
                let mut kept: Vec<ToolWindow> = Vec::new();
                for w in self.shown_tool_windows() {
                    let displaced = kept.iter().any(|k| {
                        !(*k == ToolWindow::Commit && w == ToolWindow::Project)
                            && !(*k == ToolWindow::Project && w == ToolWindow::Commit)
                            && self.tool_layout.displaced_by(*k, &[w]).contains(&w)
                    });
                    if displaced {
                        self.hide_tool_window(w);
                    } else {
                        kept.push(w);
                    }
                }
            }
            Some(shown) => {
                let mut now = self.shown_tool_windows();
                if self.show_tree && !now.contains(&ToolWindow::Project) {
                    now.push(ToolWindow::Project);
                }
                for w in now {
                    if !shown.contains(&w) {
                        self.hide_tool_window(w);
                    }
                }
                for w in shown {
                    if *w == ToolWindow::Project {
                        // The tree needs no view of its own; Commit may cover it.
                        self.show_tree = true;
                    } else if self.tool_window_exists(*w) && !self.tool_window_shown(*w) {
                        self.toggle_tool_window(*w, window, cx);
                    }
                }
                let focus = applied
                    .active
                    .filter(|w| self.tool_window_shown(*w))
                    .and_then(|w| self.tool_window_focus(w, cx));
                match focus {
                    Some(focus) => window.focus(&focus),
                    None => self.focus_active(window, cx),
                }
            }
        }
        let unsaved = self
            .tool_layout
            .save(&self.root)
            .err()
            .map(|e| format!("Tool window layout not saved: {e}"));
        (changed, unsaved)
    }
}
