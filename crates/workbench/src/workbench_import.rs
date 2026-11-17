//! Exchange files in the workbench (`SPEC-0303` behaviours 11, 17, 18): the import preview for a
//! file that was opened, dropped or sent by `idecraft open`, the import after confirmation, and
//! Share, which opens a prefilled GitHub issue in the browser.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gpui::{App, AppContext, Context, Window};

use super::WorkbenchView;
use crate::{ImportPreview, ImportPreviewEvent};

/// The name Export Settings suggests (`SPEC-0105` v9 behaviour 14).
pub const SETTINGS_EXPORT_NAME: &str = "settings.idcsettings";

/// Files the system asked IDECraft to open (Finder, `open`), waiting for a project window.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Notes files to open; the app's loop hands them to [`WorkbenchView::show_import_in_front`].
pub fn queue_opened(paths: Vec<PathBuf>) {
    OPENED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(paths);
}

/// The files noted by [`queue_opened`] since the last call.
pub fn take_opened() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(|e| e.into_inner()))
}

impl WorkbenchView {
    /// Where an import of this window writes: its project and the window's user files.
    fn import_targets(&self, cx: &App) -> idecraft_formats::Targets {
        idecraft_formats::Targets {
            root: self.root.clone(),
            keymap_file: crate::keymaps::keymap_file(cx),
            settings_file: crate::appearance::ide_settings_file(cx),
            legacy_theme_file: crate::appearance::theme_file(cx),
        }
    }

    /// Shows the import preview of the exchange file at `path` (behaviour 11).
    pub fn open_import(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.import_targets(cx);
        let preview = idecraft_formats::preview_file(path, &targets);
        let view = cx.new(|cx| ImportPreview::new(preview, cx));
        self._import_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, view, event: &ImportPreviewEvent, window, cx| match event {
                ImportPreviewEvent::Share => {
                    let p = view.read(cx).preview().clone();
                    this.share_exchange(&p, cx);
                }
                ImportPreviewEvent::Import => {
                    let p = view.read(cx).preview().clone();
                    this.import_preview = None;
                    this.apply_import(&p, cx);
                    this.focus_active(window, cx);
                    cx.notify();
                }
                ImportPreviewEvent::Close => {
                    this.import_preview = None;
                    this.status = Some("Import cancelled; nothing changed.".into());
                    this.focus_active(window, cx);
                    cx.notify();
                }
            },
        ));
        window.focus(&view.read(cx).focus());
        self.import_preview = Some(view);
        cx.notify();
    }

    /// File › Manage IDE Settings › Export Settings… (`SPEC-0105` v9 behaviour 14): asks for the
    /// file in the system's save dialog, starting in the project's folder.
    pub(crate) fn export_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_new_path(&self.root, Some(SETTINGS_EXPORT_NAME));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(target))) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.export_settings_to(&target, cx));
        })
        .detach();
    }

    /// Writes the `settings` exchange file of the IDE's shared settings and the user keymap to
    /// `target` (`SPEC-0303` behaviour 13) and says in the status bar what was written and what
    /// was left out; returns whether it was written.
    pub fn export_settings_to(&mut self, target: &Path, cx: &mut Context<Self>) -> bool {
        let targets = self.import_targets(cx);
        let written = idecraft_formats::export(
            "settings",
            &idecraft_formats::Selection::default(),
            &targets,
        )
        .and_then(|e| {
            std::fs::write(target, e.text.as_bytes())
                .map(|()| e.left_out)
                .map_err(|err| err.to_string())
        });
        self.status = Some(match &written {
            Ok(left_out) => {
                let sections: Vec<&str> = left_out
                    .iter()
                    .filter_map(|l| l.strip_prefix("settings."))
                    .filter_map(|l| l.split(':').next())
                    .collect();
                if sections.is_empty() {
                    format!("Exported the settings to {}", target.display())
                } else {
                    format!(
                        "Exported the settings to {}; not exported: {}",
                        target.display(),
                        sections.join(", ")
                    )
                }
            }
            Err(e) => format!("Settings not exported to {}: {e}", target.display()),
        });
        if written.is_ok() && target.starts_with(&self.root) {
            self.tree = self.tree.reloaded();
        }
        cx.notify();
        written.is_ok()
    }

    /// File › Manage IDE Settings › Import Settings… (`SPEC-0105` v9 behaviour 14): asks for a
    /// file in the system's open dialog and shows its import preview; nothing changes before the
    /// user confirms there.
    pub(crate) fn import_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| this.open_import(&path, window, cx));
            }
        })
        .detach();
    }

    /// The import preview, if it is shown.
    pub fn import_preview(&self) -> Option<gpui::Entity<ImportPreview>> {
        self.import_preview.clone()
    }

    /// Imports a confirmed preview and uses what changed at once (behaviour 12).
    fn apply_import(&mut self, preview: &idecraft_formats::Preview, cx: &mut Context<Self>) {
        let targets = self.import_targets(cx);
        match idecraft_formats::apply(preview, &targets) {
            Ok(written) => {
                crate::keymaps::load(cx);
                crate::appearance::apply_saved_theme(cx);
                crate::appearance::apply_saved_settings(cx);
                cx.refresh_windows();
                self.status = Some(format!(
                    "Imported {}: {} changed.",
                    preview.file,
                    written.join(", ")
                ));
            }
            Err(e) => self.status = Some(format!("Nothing imported: {e}")),
        }
    }

    /// Opens the prefilled issue for `preview`'s file in the browser (behaviour 18).
    fn share_exchange(&mut self, preview: &idecraft_formats::Preview, cx: &mut Context<Self>) {
        let kind = preview.kind.clone().unwrap_or_else(|| "file".into());
        let name = preview
            .envelope
            .as_ref()
            .and_then(|e| e.name.clone())
            .unwrap_or_else(|| preview.file.clone());
        let (url, whole) = idecraft_formats::share_url(&kind, &name, &preview.text);
        cx.open_url(&url);
        self.status = Some(if whole {
            "Opened a prefilled issue in the browser; nothing is sent until you submit it.".into()
        } else {
            format!(
                "Opened an issue in the browser; {} is too large for the link, attach it there.",
                preview.file
            )
        });
        cx.notify();
    }

    /// Shows the preview of `path` in the front project window (the last one opened when none is
    /// in front); returns that window's project (behaviour 17).
    pub fn show_import_in_front(path: &Path, cx: &mut App) -> Result<String, String> {
        let benches = crate::project_windows::workbenches(cx);
        let active = cx.active_window();
        let bench = benches
            .iter()
            .find(|b| {
                let root = b.read(cx).root.clone();
                active.is_some() && crate::project_windows::project_window(&root, cx) == active
            })
            .or(benches.last())
            .cloned()
            .ok_or("no project window is open")?;
        let root = bench.read(cx).root.clone();
        let handle = crate::project_windows::project_window(&root, cx)
            .ok_or("the project window is not registered")?;
        let path = path.to_path_buf();
        handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                bench.update(cx, |b, cx| b.open_import(&path, window, cx));
            })
            .map_err(|e| e.to_string())?;
        Ok(root.display().to_string())
    }
}
