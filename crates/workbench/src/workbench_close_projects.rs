//! Close Project, Close All Projects and Close Other Projects (`SPEC-0094` v3 behaviours 6 to 9,
//! `SPEC-0261` v6 behaviour 19). The decisions without a window are in [`crate::close_model`].

use std::path::{Path, PathBuf};

use gpui::{App, Context, Entity};

use super::{DrawnFor, WorkbenchView};
use crate::close_model::{CloseScope, project_name, unsaved_in};
use crate::quit_model::QuitChoice;

/// What closing would close and lose: the project roots (current last), their unsaved files named
/// as the question names them, and their running work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClosePlan {
    pub projects: Vec<PathBuf>,
    pub unsaved: Vec<String>,
    pub running: Vec<String>,
}

impl WorkbenchView {
    /// The open project windows' workbenches with their roots, oldest first.
    fn open_benches(cx: &App) -> Vec<(PathBuf, Entity<WorkbenchView>)> {
        Self::read_benches(None, cx, |b, _| b.root.clone())
    }

    /// `read` of every open workbench, oldest first. `me` is the workbench being updated: it is
    /// read directly, never through its entity, which gpui refuses while the update runs (the
    /// crash of 2026-11-14 when the project popup opened).
    pub(super) fn read_benches<T>(
        me: Option<(gpui::EntityId, &WorkbenchView)>,
        cx: &App,
        read: impl Fn(&WorkbenchView, &App) -> T,
    ) -> Vec<(T, Entity<WorkbenchView>)> {
        crate::project_windows::workbenches(cx)
            .into_iter()
            .map(|b| {
                let value = match me {
                    Some((id, me)) if id == b.entity_id() => read(me, cx),
                    _ => read(b.read(cx), cx),
                };
                (value, b)
            })
            .collect()
    }

    /// What `scope` would close when used in the window of `current` (behaviours 6 to 8).
    pub fn close_plan(scope: CloseScope, current: &Path, cx: &App) -> ClosePlan {
        Self::close_plan_with(scope, current, None, cx)
    }

    /// [`Self::close_plan`] from inside the update of workbench `me`.
    fn close_plan_with(
        scope: CloseScope,
        current: &Path,
        me: Option<(gpui::EntityId, &WorkbenchView)>,
        cx: &App,
    ) -> ClosePlan {
        let benches = Self::read_benches(me, cx, |b, cx| {
            (
                b.root.clone(),
                b.remote_unsaved_names(cx),
                b.running_work(cx),
            )
        });
        let open: Vec<PathBuf> = benches.iter().map(|((r, _, _), _)| r.clone()).collect();
        let projects = scope.targets(&open, current);
        let modified: Vec<PathBuf> = idecraft_editor::documents::DocumentRegistry::global()
            .list()
            .into_iter()
            .filter(|d| d.modified)
            .map(|d| d.path)
            .collect();
        let mut unsaved = crate::quit_model::names(&unsaved_in(&modified, &projects), &open);
        let mut running = Vec::new();
        for ((root, remote, work), _) in benches {
            if projects.contains(&root) {
                unsaved.extend(remote);
                running.extend(work);
            }
        }
        ClosePlan {
            projects,
            unsaved,
            running,
        }
    }

    /// What `scope` (`this`, `all`, `others`) would close seen from the project window of
    /// `workspace`, for the MCP tool `close_project` (behaviour 10).
    pub fn close_plan_in(workspace: &Path, scope: &str, cx: &App) -> Result<ClosePlan, String> {
        let scope = CloseScope::of_name(scope).ok_or(format!("unknown scope `{scope}`"))?;
        let wanted = workspace.canonicalize().unwrap_or(workspace.to_path_buf());
        let root = Self::open_benches(cx)
            .into_iter()
            .map(|(r, _)| r)
            .find(|r| r.canonicalize().unwrap_or(r.clone()) == wanted)
            .ok_or("no project window is open for this workspace")?;
        Ok(Self::close_plan(scope, &root, cx))
    }

    /// `CloseProject`, `CloseAllProjects`, `CloseOtherProjects` used in this window: closes at
    /// once when nothing is pending, else asks in this window (behaviour 7). False when the
    /// action is not offered now (behaviour 6).
    pub(super) fn close_projects(&mut self, scope: CloseScope, cx: &mut Context<Self>) -> bool {
        let root = self.root.clone();
        let plan = Self::close_plan_with(scope, &root, Some((cx.entity_id(), self)), cx);
        if plan.projects.is_empty() {
            return false;
        }
        let names: Vec<String> = plan.projects.iter().map(|p| project_name(p)).collect();
        match crate::close_model::question(&names, &plan.unsaved, &plan.running) {
            None => {
                // After this action: the windows are not closed from inside this one's update.
                cx.defer(move |cx| Self::close_now(&plan.projects, cx));
            }
            Some(q) => {
                let choices = q.answers.iter().map(|(_, c)| *c).collect();
                self.drawn_question = Some((
                    crate::dialogs::DrawnQuestion::quit(&q),
                    DrawnFor::Close(plan.projects, choices),
                ));
                cx.notify();
            }
        }
        true
    }

    /// The window's close button (`SPEC-0094` v4 behaviour 11): Close Project, so the window goes
    /// through [`Self::close_now`] or stays after Cancel. Returns whether the platform may close
    /// the window itself: only when it is no registered project window.
    pub fn close_button(&mut self, cx: &mut Context<Self>) -> bool {
        !self.close_projects(CloseScope::of_close_button(), cx)
    }

    /// An answer to the question of [`Self::close_projects`].
    pub(super) fn answer_close(
        &mut self,
        projects: Vec<PathBuf>,
        choice: Option<QuitChoice>,
        cx: &mut Context<Self>,
    ) {
        let Some(choice) = choice else {
            return;
        };
        let me = cx.entity();
        // The save step updates the workbenches that close, this one too, so it runs after this
        // update.
        cx.defer(move |cx| match Self::prepare_close(&projects, choice, cx) {
            Ok(()) => Self::close_now(&projects, cx),
            Err(why) => me.update(cx, |b, cx| {
                b.drawn_question = Some((
                    crate::dialogs::DrawnQuestion::notice("Nothing was closed", why),
                    DrawnFor::Notice,
                ));
                cx.notify();
            }),
        });
    }

    /// The step before the `projects` close, as [`Self::prepare_quit`] for quitting: `Ok` closes
    /// them, `Err` names what keeps them open (behaviour 7).
    pub fn prepare_close(
        projects: &[PathBuf],
        choice: QuitChoice,
        cx: &mut App,
    ) -> Result<(), String> {
        let benches: Vec<_> = Self::open_benches(cx)
            .into_iter()
            .filter(|(r, _)| projects.contains(r))
            .collect();
        let pending = |cx: &App| {
            let open: Vec<PathBuf> = Self::open_benches(cx).into_iter().map(|(r, _)| r).collect();
            let modified: Vec<PathBuf> = idecraft_editor::documents::DocumentRegistry::global()
                .list()
                .into_iter()
                .filter(|d| d.modified)
                .map(|d| d.path)
                .collect();
            let mut left = crate::quit_model::names(&unsaved_in(&modified, projects), &open);
            for (_, b) in &benches {
                left.extend(b.read(cx).remote_unsaved_names(cx));
            }
            left
        };
        match choice {
            QuitChoice::Discard => Ok(()),
            QuitChoice::IfNothingPending => {
                let mut left = pending(cx);
                for (_, b) in &benches {
                    left.extend(b.read(cx).running_work(cx));
                }
                if left.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "work became pending meanwhile: {}",
                        left.join(", ")
                    ))
                }
            }
            QuitChoice::SaveAll => {
                for (_, b) in &benches {
                    b.update(cx, |b, cx| b.save_all_editors(cx));
                }
                let left = pending(cx);
                if left.is_empty() {
                    Ok(())
                } else {
                    Err(format!("not saved: {}", left.join(", ")))
                }
            }
        }
    }

    /// Closes the windows of `projects` in that order (the current one last, behaviour 8): stops
    /// their runs and debug sessions, notes them closed and removes the windows. When no project
    /// window is left the app offers its folder chooser (behaviour 9).
    pub fn close_now(projects: &[PathBuf], cx: &mut App) {
        let benches = Self::open_benches(cx);
        for root in projects {
            if let Some((_, bench)) = benches.iter().find(|(r, _)| r == root) {
                bench.update(cx, |b, cx| b.stop_everything(cx));
            }
            let Some(window) = crate::project_windows::unregister_window(root, cx) else {
                continue;
            };
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
        if crate::project_windows::opened_windows(cx).is_empty()
            && let Some(hook) = cx
                .try_global::<crate::project_windows::OnLastProjectClosed>()
                .map(|h| h.0.clone())
        {
            // After the windows are gone, so the chooser is the only thing on screen.
            cx.defer(move |cx| hook(cx));
        }
    }

    /// Stops the runs, tests and debug session of this window (behaviour 7).
    fn stop_everything(&mut self, cx: &mut Context<Self>) {
        self.debug_command(crate::debug_model::Command::Stop, cx);
        if let Some(v) = &self.run_view {
            v.update(cx, |v, cx| {
                v.stop_all();
                cx.notify();
            });
        }
    }
}
