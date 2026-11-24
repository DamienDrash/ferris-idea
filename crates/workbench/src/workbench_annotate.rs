//! Annotate with Git Blame in the workbench (`SPEC-0277`): running the blame of an editor view's
//! file off the window's thread (per view, also in a split: v2 behaviour 13), refreshing it when
//! HEAD changes, the reasons a file cannot be annotated, and the way from an annotation to its
//! commit in the Git log.
//!
//! What the column shows and how it follows edits is in [`crate::blame_model`] and the editor view.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferris_idea_vcs::{GitError, HeadBlame, Repo};
use gpui::{Context, Entity, WeakEntity, Window};

use super::WorkbenchView;
use crate::EditorView;
use crate::blame_model;

/// Reads the blame of `path` at HEAD (behaviours 7 and 9).
pub(super) fn blame_of(path: &Path) -> Result<HeadBlame, GitError> {
    let dir = path.parent().unwrap_or(path);
    let repo = Repo::discover(dir)?;
    let workdir = repo
        .workdir()
        .and_then(|w| w.canonicalize().ok())
        .ok_or_else(|| GitError::NotARepository(path.display().to_string()))?;
    let canonical = path.canonicalize().unwrap_or(path.to_path_buf());
    let relative = canonical
        .strip_prefix(&workdir)
        .map_err(|_| GitError::NotARepository(path.display().to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    repo.blame_head(&relative)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl WorkbenchView {
    /// `Annotate`: shows the annotations of the editor view with the focus, or closes them while
    /// shown (behaviours 1 and 13).
    pub fn toggle_annotations(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) else {
            self.status = Some(blame_model::NO_EDITOR.into());
            cx.notify();
            return;
        };
        if editor.read(cx).annotated() {
            Self::close_annotations_in(&editor, cx);
            cx.notify();
        } else {
            self.annotate_view(path, &editor, cx);
        }
    }

    /// Whether the main group's editor of `path` shows annotations (its tab menu).
    pub fn annotated(&self, path: &Path, cx: &gpui::App) -> bool {
        self.editors
            .get(path)
            .is_some_and(|e| e.read(cx).annotated())
    }

    /// Close Annotations on the editor view with the focus (behaviours 6 and 13).
    pub fn close_annotations(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor() {
            Self::close_annotations_in(&editor, cx);
        }
        cx.notify();
    }

    fn close_annotations_in(editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        editor.update(cx, |e, cx| e.set_annotations(None, cx));
    }

    /// Annotates the editor view with the focus (behaviour 1).
    pub fn annotate(&mut self, cx: &mut Context<Self>) {
        if let (Some(path), Some(editor)) = (self.active_path(), self.active_editor()) {
            self.annotate_view(path, &editor, cx);
        }
    }

    /// Reads the blame of `path` on a background thread and shows it in `editor`; says in the
    /// status bar why it cannot (behaviours 9, 10 and 13).
    pub(super) fn annotate_view(
        &mut self,
        path: PathBuf,
        editor: &Entity<EditorView>,
        cx: &mut Context<Self>,
    ) {
        self.blame_in_background(path, vec![editor.downgrade()], true, cx);
    }

    /// Every editor view of this window with its file: the main group's, then the second group's.
    fn editor_views(&self) -> Vec<(PathBuf, Entity<EditorView>)> {
        self.editors
            .iter()
            .chain(self.groups.iter().flat_map(|s| s.editors.iter()))
            .map(|(p, e)| (p.clone(), e.clone()))
            .collect()
    }

    /// One read per file serves every view that asked while it runs (behaviours 10 and 13).
    fn blame_in_background(
        &mut self,
        path: PathBuf,
        views: Vec<WeakEntity<EditorView>>,
        announce: bool,
        cx: &mut Context<Self>,
    ) {
        let mut start = false;
        for view in views {
            start |= self.annotating.ask(&path, view);
        }
        if !start {
            return;
        }
        let name = file_name(&path);
        if announce {
            self.status = Some(format!("Annotating {name}…"));
            cx.notify();
        }
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { blame_of(&path) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let views = this.annotating.finish(&path);
                match read {
                    Ok(blame) => {
                        let blame = Arc::new(blame);
                        // A view closed meanwhile is skipped.
                        for view in views.iter().filter_map(WeakEntity::upgrade) {
                            let blame = blame.clone();
                            view.update(cx, |e, cx| e.set_annotations(Some(blame), cx));
                        }
                        if announce {
                            this.status = None;
                        }
                    }
                    Err(e) => {
                        // A refresh of shown annotations keeps them; a request says why not.
                        this.status = Some(blame_model::reason(&e, &name));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether a blame is being read (tests).
    pub fn annotating(&self) -> bool {
        self.annotating.running()
    }

    /// Reads the blame again for annotated editor views, in both groups of a split, whose HEAD
    /// text is no longer the one blamed: after a commit, checkout or pull (behaviours 8 and 13).
    /// `heads` holds the HEAD text per path as the gutter markers just read it; `None` for all
    /// means: after a commit in Ferris IDEA, refresh every annotated view.
    pub(super) fn refresh_annotations(
        &mut self,
        heads: Option<&[(PathBuf, Option<String>)]>,
        cx: &mut Context<Self>,
    ) {
        let annotated: Vec<(WeakEntity<EditorView>, PathBuf, String)> = self
            .editor_views()
            .into_iter()
            .filter_map(|(p, e)| {
                let blamed = e.read(cx).annotations()?.blame().head_text.clone();
                Some((e.downgrade(), p, blamed))
            })
            .collect();
        let mut per_file: Vec<(PathBuf, Vec<WeakEntity<EditorView>>)> = Vec::new();
        for (path, view) in blame_model::stale_views(&annotated, heads) {
            match per_file.iter_mut().find(|(p, _)| *p == path) {
                Some((_, views)) => views.push(view),
                None => per_file.push((path, vec![view])),
            }
        }
        for (path, views) in per_file {
            self.blame_in_background(path, views, false, cx);
        }
    }

    /// Opens the Git log with `commit` (full id) selected (behaviour 5).
    pub fn show_commit_in_log(
        &mut self,
        commit: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_log.is_none() {
            self.toggle_git_log(window, cx);
        }
        if let Some(log) = self.git_log.clone() {
            log.update(cx, |l, cx| l.select_commit(commit, cx));
            window.focus(&log.read(cx).focus());
        }
        cx.notify();
    }

    /// The items of the annotation column's menu for 0-based `line` of the editor view with the
    /// focus, which the menu was opened on (behaviours 6 and 13).
    pub(super) fn annotation_item(
        &mut self,
        action: &str,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commit = self.active_editor().and_then(|e| {
            let e = e.read(cx);
            e.annotations()?.commit_at(line?).map(|c| c.info.id.clone())
        });
        match action {
            blame_model::CLOSE_ANNOTATIONS => self.close_annotations(cx),
            blame_model::ANNOTATE => self.toggle_annotations(cx),
            blame_model::SHOW_IN_GIT_LOG => {
                if let Some(id) = commit {
                    self.show_commit_in_log(&id, window, cx);
                }
            }
            blame_model::COPY_REVISION => {
                if let Some(id) = commit {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(id));
                }
            }
            other => self.menu_action(other, window, cx),
        }
    }
}
