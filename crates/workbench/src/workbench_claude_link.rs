//! The window's side of the Claude Code link (`SPEC-0358` behaviours 12–23): the selection, the
//! open editors and their problems for Claude Code, files it opens, and its proposed changes shown
//! in the diff viewer with Accept and Reject. The link itself (`ferris-idea-claude-link`) is
//! toolkit-free; the app hands its questions to the functions here on the window's thread.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use ferris_idea_claude_link::{
    Diagnostic, DiffDecision, DiffProposal, OpenEditor, OpenRequest, Position, Selection, Severity,
};
use ferris_idea_editor::documents::DocumentRegistry;
use gpui::{App, Context, Entity, Global, WeakEntity, Window, div, prelude::*};

use super::WorkbenchView;
use crate::diff_view::{DiffDecided, DiffEvent, DiffView};
use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, Role, Semantics};

/// A proposed change waiting for the user, and the viewer that shows it.
struct Waiting {
    proposal: DiffProposal,
    reply: Sender<DiffDecision>,
    shown: Option<WeakEntity<DiffView>>,
    _subscriptions: Vec<gpui::Subscription>,
}

/// Claude Code's proposed changes per project, the first one shown (behaviour 17).
#[derive(Default)]
struct ClaudeDiffs(HashMap<PathBuf, Vec<Waiting>>);

impl Global for ClaudeDiffs {}

/// The projects whose link has Claude Code connected, for the status bar (behaviour 23).
#[derive(Default)]
struct ClaudeConnected(HashSet<PathBuf>);

impl Global for ClaudeConnected {}

fn same(a: &Path, b: &Path) -> bool {
    a == b || DocumentRegistry::key(a) == DocumentRegistry::key(b)
}

/// The workbench of the project window of `root`.
fn bench_of(root: &Path, cx: &App) -> Option<Entity<WorkbenchView>> {
    crate::project_windows::workbenches(cx)
        .into_iter()
        .find(|b| same(&b.read(cx).root, root))
}

fn severity(s: crate::diagnostics_model::Severity, unused: bool) -> Severity {
    use crate::diagnostics_model::Severity as S;
    match s {
        S::Error => Severity::Error,
        S::Warning => Severity::Warning,
        S::Weak if unused => Severity::Hint,
        S::Weak => Severity::Information,
    }
}

/// Char offsets of the selection `request` names in `text` (behaviour 13).
pub(crate) fn selection_range(text: &str, request: &OpenRequest) -> Option<std::ops::Range<usize>> {
    let chars = |byte: usize| text[..byte].chars().count();
    let line_end = |from: usize| text[from..].find('\n').map_or(text.len(), |i| from + i);
    if let Some(start_text) = &request.start_text {
        let start = text.find(start_text.as_str())?;
        let mut end = start + start_text.len();
        if let Some(end_text) = &request.end_text
            && let Some(i) = text[start..].find(end_text.as_str())
        {
            end = start + i + end_text.len();
            if request.select_to_end_of_line {
                end = line_end(end);
            }
        }
        return Some(chars(start)..chars(end));
    }
    let first = request.start_line?.checked_sub(1)?;
    let last = request.end_line.unwrap_or(first + 1).max(first + 1) - 1;
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let start = *starts.get(first)?;
    let end = line_end(*starts.get(last).unwrap_or(starts.last()?));
    Some(chars(start)..chars(end))
}

impl WorkbenchView {
    /// The editor of `path` in any tab group of this window.
    fn claude_editor_of(&self, path: &Path) -> Option<Entity<EditorView>> {
        self.editors
            .iter()
            .chain(self.groups.iter().flat_map(|s| s.editors.iter()))
            .find(|(p, _)| same(p, path))
            .map(|(_, e)| e.clone())
    }

    /// The selection of the active editor of the project window of `root` (behaviour 14).
    pub fn claude_selection_in(root: &Path, cx: &mut App) -> Option<Selection> {
        let s = Self::editor_selection_in(root, cx).ok().flatten()?;
        Some(Selection {
            path: s.path,
            start: Position {
                line: s.start_line,
                character: s.start_character,
            },
            end: Position {
                line: s.end_line,
                character: s.end_character,
            },
            text: s.text,
        })
    }

    /// The open editor tabs of the project window of `root` (behaviour 15).
    pub fn claude_open_editors_in(root: &Path, cx: &App) -> Vec<OpenEditor> {
        let Some(bench) = bench_of(root, cx) else {
            return Vec::new();
        };
        let b = bench.read(cx);
        let active = b.active_path();
        let mut paths: Vec<PathBuf> = b.tabs.paths().to_vec();
        for p in b
            .editors
            .keys()
            .chain(b.groups.iter().flat_map(|g| g.editors.keys()))
        {
            if !paths.contains(p) {
                paths.push(p.clone());
            }
        }
        paths
            .into_iter()
            .map(|path| OpenEditor {
                active: active.as_ref() == Some(&path),
                dirty: b
                    .claude_editor_of(&path)
                    .is_some_and(|e| e.read(cx).is_modified()),
                path,
            })
            .collect()
    }

    /// The problems of `file`, or of every open file, of the project window of `root`
    /// (behaviour 16).
    pub fn claude_diagnostics_in(root: &Path, file: Option<&Path>, cx: &App) -> Vec<Diagnostic> {
        let Some(bench) = bench_of(root, cx) else {
            return Vec::new();
        };
        let b = bench.read(cx);
        let mut out = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for (path, editor) in b
            .editors
            .iter()
            .chain(b.groups.iter().flat_map(|g| g.editors.iter()))
        {
            if file.is_some_and(|f| !same(f, path)) || seen.contains(path) {
                continue;
            }
            seen.push(path.clone());
            for p in editor.read(cx).problem_spans() {
                out.push(Diagnostic {
                    path: path.clone(),
                    start: Position {
                        line: p.start.0,
                        character: p.start.1,
                    },
                    end: Position {
                        line: p.end.0,
                        character: p.end.1,
                    },
                    severity: severity(p.severity, p.unused),
                    message: p.message,
                    source: None,
                });
            }
        }
        out
    }

    /// Opens a file for Claude Code and selects what it names (behaviour 13).
    pub fn claude_open_file_in(
        root: &Path,
        request: &OpenRequest,
        cx: &mut App,
    ) -> Result<(), String> {
        let bench = bench_of(root, cx).ok_or("no project window is open for this project")?;
        let handle = crate::project_windows::project_window(&bench.read(cx).root.clone(), cx)
            .ok_or("the project window is not registered")?;
        handle
            .update(cx, |_, window, cx| {
                if request.focus {
                    window.activate_window();
                }
                bench.update(cx, |b, cx| {
                    b.open(&request.path, window, cx);
                    let Some(editor) = b.claude_editor_of(&request.path) else {
                        return;
                    };
                    editor.update(cx, |e, cx| {
                        let text = e.state.buffer.text();
                        if let Some(range) = selection_range(&text, request) {
                            e.select_range(range, cx);
                        }
                    });
                    if request.focus {
                        b.focus_active(window, cx);
                    }
                    cx.notify();
                });
            })
            .map_err(|e| e.to_string())
    }

    /// Shows Claude Code's proposed change in the project window of `root`, after the ones
    /// before it (behaviour 17). Without a window the change is rejected.
    pub fn claude_show_diff_in(
        root: &Path,
        proposal: DiffProposal,
        reply: Sender<DiffDecision>,
        cx: &mut App,
    ) {
        let Some(bench) = bench_of(root, cx) else {
            let _ = reply.send(DiffDecision::Rejected);
            return;
        };
        let key = bench.read(cx).root.clone();
        let diffs = cx.default_global::<ClaudeDiffs>();
        let list = diffs.0.entry(key.clone()).or_default();
        // A proposal of the same name replaces the earlier one.
        if let Some(i) = list
            .iter()
            .position(|w| w.proposal.tab_name == proposal.tab_name)
        {
            let old = list.remove(i);
            let _ = old.reply.send(DiffDecision::Rejected);
        }
        list.push(Waiting {
            proposal,
            reply,
            shown: None,
            _subscriptions: Vec::new(),
        });
        Self::claude_show_next(&key, cx);
    }

    /// Shows the first waiting change of `root` when none is shown.
    fn claude_show_next(root: &Path, cx: &mut App) {
        let Some(bench) = bench_of(root, cx) else {
            return;
        };
        let first = cx
            .try_global::<ClaudeDiffs>()
            .and_then(|d| d.0.get(root))
            .and_then(|l| l.first())
            .map(|w| (w.shown.is_some(), w.proposal.clone()));
        let Some((false, proposal)) = first else {
            return;
        };
        let Some(handle) = crate::project_windows::project_window(root, cx) else {
            return;
        };
        let _ = handle.update(cx, |_, window, cx| {
            window.activate_window();
            let (view, subscriptions) =
                bench.update(cx, |b, cx| b.show_claude_diff(&proposal, window, cx));
            if let Some(w) = cx
                .default_global::<ClaudeDiffs>()
                .0
                .get_mut(root)
                .and_then(|l| l.first_mut())
            {
                w.shown = Some(view.downgrade());
                w._subscriptions = subscriptions;
            }
        });
    }

    /// The diff viewer of a proposed change, with Accept and Reject (behaviour 18).
    fn show_claude_diff(
        &mut self,
        p: &DiffProposal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<DiffView>, Vec<gpui::Subscription>) {
        let rows = ferris_idea_vcs::side_by_side(&p.old_text, &p.new_text);
        let name = p
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let left = if p.new_file { "New file" } else { "Current" };
        let view = cx.new(|cx| {
            DiffView::new(format!("Claude Code proposes a change to {name}"), rows, cx)
                .with_sides(left, "Proposed by Claude Code")
                .with_texts(&p.path, &p.old_text, &p.new_text)
                .with_decision()
        });
        let unified = self.diff_unified;
        view.update(cx, |v, _| v.set_unified(unified));
        let decided = cx.subscribe_in(
            &view,
            window,
            |this, view, event: &DiffDecided, window, cx| {
                this.claude_decided(view, event.0, window, cx);
            },
        );
        let events = cx.subscribe_in(
            &view,
            window,
            |this, view, event: &DiffEvent, window, cx| match event {
                DiffEvent::Unified(u) => this.diff_unified = *u,
                // Escape closes the viewer: the change is rejected.
                DiffEvent::Close => this.claude_decided(view, false, window, cx),
                DiffEvent::Revert(_) | DiffEvent::Stepped { .. } => {}
            },
        );
        self._diff_events = None;
        self.diff_file = None;
        self.diff_sides = None;
        self.diff_asked = None;
        window.focus(&view.read(cx).focus());
        self.diff = Some(view.clone());
        cx.notify();
        (view, vec![decided, events])
    }

    /// The user accepted or rejected the change `view` shows (behaviour 18).
    fn claude_decided(
        &mut self,
        view: &Entity<DiffView>,
        accept: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.root.clone();
        let taken = cx
            .default_global::<ClaudeDiffs>()
            .0
            .get_mut(&root)
            .and_then(|l| {
                let i = l.iter().position(|w| {
                    w.shown
                        .as_ref()
                        .and_then(WeakEntity::upgrade)
                        .is_some_and(|v| v == *view)
                })?;
                Some(l.remove(i))
            });
        if let Some(w) = taken {
            let _ = w.reply.send(if accept {
                DiffDecision::Accepted(w.proposal.new_text.clone())
            } else {
                DiffDecision::Rejected
            });
        }
        if self.diff.as_ref() == Some(view) {
            self.diff = None;
            self.focus_active(window, cx);
        }
        cx.notify();
        let root2 = root.clone();
        cx.defer(move |cx| Self::claude_show_next(&root2, cx));
    }

    /// Closes the change `tab_name` of `root` if it waits (behaviour 19): Claude Code closed it, or
    /// its call ended.
    pub fn claude_close_diff_in(root: &Path, tab_name: &str, cx: &mut App) {
        let Some(bench) = bench_of(root, cx) else {
            return;
        };
        let key = bench.read(cx).root.clone();
        let taken = cx
            .default_global::<ClaudeDiffs>()
            .0
            .get_mut(&key)
            .and_then(|l| {
                let i = l.iter().position(|w| w.proposal.tab_name == tab_name)?;
                Some(l.remove(i))
            });
        let Some(w) = taken else {
            return;
        };
        let _ = w.reply.send(DiffDecision::Rejected);
        let shown = w.shown.as_ref().and_then(WeakEntity::upgrade);
        if let Some(view) = shown
            && let Some(handle) = crate::project_windows::project_window(&key, cx)
        {
            let _ = handle.update(cx, |_, window, cx| {
                bench.update(cx, |b, cx| {
                    if b.diff.as_ref() == Some(&view) {
                        b.diff = None;
                        b.focus_active(window, cx);
                        cx.notify();
                    }
                });
            });
        }
        Self::claude_show_next(&key, cx);
    }

    /// Changes shown for Claude Code whose viewer went away (another diff took its place, the
    /// window closed) are rejected; the next waiting one is shown (behaviour 19).
    pub fn claude_diffs_tick(cx: &mut App) {
        let roots: Vec<PathBuf> = match cx.try_global::<ClaudeDiffs>() {
            Some(d) => d.0.keys().cloned().collect(),
            None => return,
        };
        for root in roots {
            let bench = bench_of(&root, cx);
            let current = bench.as_ref().and_then(|b| b.read(cx).diff.clone());
            let gone: Vec<Waiting> = {
                let diffs = cx.default_global::<ClaudeDiffs>();
                let Some(list) = diffs.0.get_mut(&root) else {
                    continue;
                };
                let mut gone = Vec::new();
                let mut i = 0;
                while i < list.len() {
                    let lost = bench.is_none()
                        || list[i].shown.as_ref().is_some_and(|w| {
                            w.upgrade().is_none_or(|v| current.as_ref() != Some(&v))
                        });
                    if lost {
                        gone.push(list.remove(i));
                    } else {
                        i += 1;
                    }
                }
                if list.is_empty() {
                    diffs.0.remove(&root);
                }
                gone
            };
            for w in gone {
                let _ = w.reply.send(DiffDecision::Rejected);
            }
            if bench.is_some() {
                Self::claude_show_next(&root, cx);
            }
        }
    }

    /// Records which projects have Claude Code connected; returns whether that changed
    /// (behaviour 23).
    pub fn set_claude_connected(roots: HashSet<PathBuf>, cx: &mut App) -> bool {
        let state = cx.default_global::<ClaudeConnected>();
        if state.0 == roots {
            return false;
        }
        state.0 = roots;
        true
    }

    /// "Claude Code connected" in the status bar while Claude Code is linked to this project
    /// (behaviour 23).
    pub(super) fn claude_link_widget(&self, cx: &App) -> Option<gpui::AnyElement> {
        let connected = cx
            .try_global::<ClaudeConnected>()
            .is_some_and(|c| c.0.contains(&self.root));
        if !connected {
            return None;
        }
        let p = Palette::current();
        Some(
            div()
                .id("claude-code-link-widget")
                .map(super::status_widget)
                .text_color(super::c(p.ui_text))
                .child("Claude Code connected")
                .semantics(
                    Semantics::new(Role::Status)
                        .name("Claude Code connected")
                        .test_id("claude-code-link-widget"),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(start: Option<&str>, end: Option<&str>, eol: bool) -> OpenRequest {
        OpenRequest {
            start_text: start.map(str::to_owned),
            end_text: end.map(str::to_owned),
            select_to_end_of_line: eol,
            ..OpenRequest::default()
        }
    }

    /// SPEC-0358 AK-9: the selection `openFile` names, by text and by lines.
    #[test]
    fn open_file_selections() {
        let text = "fn main() {\n    println!(\"ä\");\n}\n";
        assert_eq!(
            selection_range(text, &request(Some("main"), None, false)),
            Some(3..7)
        );
        assert_eq!(
            selection_range(text, &request(Some("println"), Some(")"), false)),
            Some(16..29)
        );
        assert_eq!(
            selection_range(text, &request(Some("println"), Some("("), true)),
            Some(16..30)
        );
        assert_eq!(
            selection_range(text, &request(Some("nope"), None, false)),
            None
        );
        let lines = OpenRequest {
            start_line: Some(2),
            end_line: Some(3),
            ..OpenRequest::default()
        };
        assert_eq!(selection_range(text, &lines), Some(12..32));
        let one = OpenRequest {
            start_line: Some(1),
            ..OpenRequest::default()
        };
        assert_eq!(selection_range(text, &one), Some(0..11));
    }
}
