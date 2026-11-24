//! The project popup's choices in the workbench (`SPEC-0094` v5 behaviours 1, 2, 12 to 14):
//! switching to an open project's window, Clone Repository in the background with progress and
//! cancelling, New Project.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{AppContext, Context, Window};

use super::WorkbenchView;
use crate::project_popup_model::progress_text;

/// How long a clone may run before it is ended.
const CLONE_LIMIT: Duration = Duration::from_secs(3600);

/// A clone running on a thread of its own (behaviour 13): set `cancel` to end it; `progress`
/// holds git's last progress line and `done` the new folder or why it failed, once it ended.
pub(crate) struct CloneThread {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<Mutex<Option<ferris_idea_vcs::CloneProgress>>>,
    pub done: Arc<Mutex<Option<Result<PathBuf, String>>>>,
}

impl CloneThread {
    /// The last progress and, once the clone ended, its result (taken).
    pub fn poll(
        &self,
    ) -> (
        Option<ferris_idea_vcs::CloneProgress>,
        Option<Result<PathBuf, String>>,
    ) {
        let finished = self.done.lock().unwrap_or_else(|e| e.into_inner()).take();
        let step = self
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        (step, finished)
    }
}

/// Starts cloning `source` into `target` with the rules of behaviour 13 and the one-hour limit.
pub(crate) fn spawn_clone(source: ferris_idea_vcs::CloneSource, target: PathBuf) -> CloneThread {
    let job = CloneThread {
        cancel: Arc::new(AtomicBool::new(false)),
        progress: Arc::new(Mutex::new(None)),
        done: Arc::new(Mutex::new(None)),
    };
    let (progress, done, cancel) = (job.progress.clone(), job.done.clone(), job.cancel.clone());
    std::thread::spawn(move || {
        let result =
            ferris_idea_vcs::clone_repository(&source, &target, &cancel, CLONE_LIMIT, |p| {
                *progress.lock().unwrap_or_else(|e| e.into_inner()) = Some(p)
            })
            .map(|o| o.folder)
            .map_err(|e| e.to_string());
        *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    });
    job
}

impl WorkbenchView {
    /// Opens the project popup: the actions, the projects open in windows and the recent ones
    /// (behaviours 1 and 12).
    pub fn open_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // This workbench is being updated: it is read directly, the others through their entity.
        let open: Vec<PathBuf> =
            Self::read_benches(Some((cx.entity_id(), self)), cx, |b, _| b.root.clone())
                .into_iter()
                .map(|(root, _)| root)
                .collect();
        let current = self.root.clone();
        let recent = crate::project_windows::recent_projects(&self.root, cx);
        let cloning = self.clone_job.as_ref().map(|(_, p)| p.clone());
        let view = cx.new(|cx| {
            let mut v = crate::ProjectsView::new(open, current, recent, cx);
            v.set_cloning(cloning, None, cx);
            v
        });
        self._projects_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, _, event: &crate::ProjectsEvent, window, cx| {
                match event {
                    crate::ProjectsEvent::Browse => this.browse_project(cx),
                    crate::ProjectsEvent::RemoteDevelopment => {
                        this.projects = None;
                        this.open_remote_development(window, cx);
                        cx.notify();
                        return;
                    }
                    crate::ProjectsEvent::Open(path) => this.open_project(path, cx),
                    crate::ProjectsEvent::Clone(source, target) => {
                        // The popup stays open with the progress.
                        this.start_clone(source.clone(), target.clone(), cx);
                        return;
                    }
                    crate::ProjectsEvent::CancelClone => {
                        if let Some((cancel, _)) = &this.clone_job {
                            cancel.store(true, Ordering::SeqCst);
                        }
                        return;
                    }
                    crate::ProjectsEvent::NewProject(target, git) => {
                        match ferris_idea_vcs::new_project(target, *git) {
                            Ok(()) => {
                                // Made by Ferris IDEA a moment ago: no trust question (A16, NP2).
                                if let Err(e) = crate::project_popup_model::trust_new_project(
                                    crate::project_popup_model::new_project_trust_file().as_deref(),
                                    target,
                                ) {
                                    this.status = Some(format!("Not trusted: {e}"));
                                }
                                this.open_project(target, cx)
                            }
                            Err(e) => {
                                if let Some(v) = &this.projects {
                                    v.update(cx, |v, cx| {
                                        v.set_cloning(None, Some(e.to_string()), cx)
                                    });
                                }
                                return;
                            }
                        }
                    }
                    crate::ProjectsEvent::Close => {}
                }
                this.projects = None;
                this.focus_active(window, cx);
                cx.notify();
            },
        ));
        window.focus(&view.read(cx).focus());
        self.projects = Some(view);
        cx.notify();
    }

    /// Clones `source` into `target` on a thread of its own; the popup (while open) and the status
    /// bar show the progress; the new folder opens in a window of its own, which asks whether to
    /// trust it like any new folder (behaviour 13, `SPEC-0278`).
    fn start_clone(
        &mut self,
        source: ferris_idea_vcs::CloneSource,
        target: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.clone_job.is_some() {
            return;
        }
        let name = source.folder_name.clone();
        let job = spawn_clone(source, target);
        let first = progress_text(&name, None);
        self.clone_job = Some((job.cancel.clone(), first.clone()));
        self.status = Some(first.clone());
        if let Some(v) = &self.projects {
            v.update(cx, |v, cx| v.set_cloning(Some(first), None, cx));
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let (step, finished) = job.poll();
                let alive = this.update(cx, |this, cx| {
                    match &finished {
                        None => {
                            let text = progress_text(&name, step.as_ref());
                            if let Some(job) = &mut this.clone_job {
                                job.1 = text.clone();
                            }
                            this.status = Some(text.clone());
                            if let Some(v) = &this.projects {
                                v.update(cx, |v, cx| v.set_cloning(Some(text), None, cx));
                            }
                        }
                        Some(Ok(folder)) => {
                            this.clone_job = None;
                            this.status = Some(format!("Cloned {name}"));
                            this.projects = None;
                            this.open_project(folder, cx);
                        }
                        Some(Err(e)) => {
                            this.clone_job = None;
                            let first = e.lines().next().unwrap_or_default().to_owned();
                            this.status = Some(first);
                            if let Some(v) = &this.projects {
                                v.update(cx, |v, cx| v.set_cloning(None, Some(e.clone()), cx));
                            }
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() || finished.is_some() {
                    break;
                }
            }
        })
        .detach();
    }
}
