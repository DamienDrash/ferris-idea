//! The project tree's option "Compact Middle Packages" in the workbench and the window side of the
//! MCP tool `project_tree` (`SPEC-0275` v10 behaviours 30 and 31).

use std::path::{Path, PathBuf};

use gpui::Context;

use super::WorkbenchView;

impl WorkbenchView {
    /// Whether the project tree compacts packages.
    pub fn compact_middle_packages(&self) -> bool {
        self.tree.compact()
    }

    /// Switches "Compact Middle Packages" for every window and keeps it in the IDE settings
    /// (`project_view.compact_middle_packages`).
    pub fn set_compact_middle_packages(&mut self, on: bool, cx: &mut Context<Self>) {
        let mut settings = crate::appearance::ide_settings(cx);
        if settings.project_view.compact_middle_packages != on {
            settings.project_view.compact_middle_packages = on;
            if let Err(e) = crate::appearance::set_ide_settings(settings, cx) {
                self.status = Some(format!("The settings could not be saved: {e}"));
            }
        }
        self.follow_compact_setting(cx);
        cx.notify();
    }

    /// Takes the option from the IDE settings in effect (each frame: another window or the
    /// Settings may have changed it); the selection stays in view.
    pub(super) fn follow_compact_setting(&mut self, cx: &gpui::App) {
        let on = crate::appearance::ide_settings(cx)
            .project_view
            .compact_middle_packages;
        if self.tree.compact() != on {
            self.tree.set_compact(on);
            self.scroll_tree_to_selection();
        }
    }

    /// The project tree as the MCP tool `project_tree` reports it.
    pub fn project_tree_state(&self) -> serde_json::Value {
        self.tree.state()
    }

    /// One call of the MCP tool `project_tree`: `request` holds `action` (`list`, `expand`,
    /// `collapse`, `select`, `compact`), `path` and `on`; the reply is the tree after it.
    pub fn project_tree_call(
        &mut self,
        request: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        let action = request["action"].as_str().unwrap_or("list");
        match action {
            "list" => {}
            "compact" => {
                let on = request["on"]
                    .as_bool()
                    .ok_or("`compact` needs `on`: true or false")?;
                self.set_compact_middle_packages(on, cx);
            }
            // Expand All and Collapse All of the header (SPEC-0275 v13 behaviour 32).
            "collapse_all" => {
                self.tree.collapse_all();
                self.scroll_tree_to_selection();
                cx.notify();
            }
            "expand_all" if request["path"].is_null() => {
                self.tree.expand_all();
                cx.notify();
            }
            _ => {
                let path = request["path"]
                    .as_str()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("`{action}` needs `path`"))?;
                self.tree.act(action, &path)?;
                if action == "select" {
                    self.scroll_tree_to_selection();
                }
                cx.notify();
            }
        }
        Ok(self.tree.state())
    }

    /// `project_tree` for the project window of `workspace` (MCP over HTTP).
    pub fn project_tree_in(
        workspace: &Path,
        request: &serde_json::Value,
        cx: &mut gpui::App,
    ) -> Result<serde_json::Value, String> {
        let wanted = workspace.canonicalize().unwrap_or(workspace.to_path_buf());
        let bench = crate::project_windows::workbenches(cx)
            .into_iter()
            .find(|b| {
                let root = b.read(cx).root.clone();
                root.canonicalize().unwrap_or(root) == wanted
            })
            .ok_or("no project window is open for this workspace")?;
        bench.update(cx, |b, cx| {
            b.follow_compact_setting(cx);
            b.project_tree_call(request, cx)
        })
    }
}
