//! A remote project in a project window (`SPEC-0317` v3): Remote Development opens a folder of an
//! SSH host; the window's project tree lists the host folder through `fs.*`, its editors hold the
//! session's documents (`doc.*`: typing applied at once, saving on the host, the host's changes
//! merged in), the title and the status bar name the host, and the connection widget follows the
//! supervisor of [`LiveProject`] through Connected, Reconnecting and Disconnected. After a crash of
//! the window, opening the project again offers the recovery copies.
//!
//! Nothing of the project is read or written on this machine: the window stands for the host
//! folder with a folder of its own state (`<settings>/remote/projects/<key>/<name>`, 0700), which
//! also takes the window's own files (tool window layout, recent files).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::{AnyWindowHandle, App, AppContext, Context, Global, Window, div, prelude::*, px};
use idecraft_core::file_ops::DeletePlan;
use idecraft_remote::{
    ConnState, DocState, DocSync, Follow, LiveProject, Restored, Synced, Timing,
};
use idecraft_session::{ErrorCode, RecoveryCopy, RpcError};
use serde_json::{Value, json};

use super::{WorkbenchView, c, status_widget};
use crate::palette::Palette;
use crate::project_tree::{ProjectTree, RemoteListing};
use crate::tips::Tipped;

/// Where remote projects keep the window's state — stand-in folders, recovery copies, the recent
/// remote projects; the self-test points it into its own folder. Default: `<settings>/remote`.
pub struct RemoteStateDir(pub PathBuf);

impl Global for RemoteStateDir {}

/// The state folder of remote projects.
pub fn state_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<RemoteStateDir>() {
        Some(d) => Some(d.0.clone()),
        None => idecraft_core::user_config_dir().map(|d| d.join("remote")),
    }
}

/// Recovery copies of remote documents (`SPEC-0317` behaviour 13).
pub fn recovery_dir(base: &Path) -> PathBuf {
    base.join("recovery")
}

/// The recent remote projects (behaviour 1, SEC-10: host and folder only, 0600).
pub fn recent_file(base: &Path) -> PathBuf {
    idecraft_remote::recent_remote_file(base)
}

/// The folder of this machine that stands for `folder` on `host` in the window.
pub fn stand_in(base: &Path, host: &str, folder: &str) -> PathBuf {
    let key = idecraft_session::ot::str_hash(&format!("{host}\n{folder}"));
    base.join("projects").join(key).join(folder_name(folder))
}

fn folder_name(folder: &str) -> String {
    folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("/")
        .to_owned()
}

/// What a stop at a file of a remote project needs (`SPEC-0322` v5 behaviour 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionStep {
    /// The frame is outside the project: nothing opens here.
    Nothing,
    /// The editor is there.
    MarkNow,
    /// The document is already on its way; only the line changes.
    MarkOnArrival,
    /// The document has to be opened through the session first.
    OpenThenMark,
}

fn execution_point_step(inside: bool, has_editor: bool, on_its_way: bool) -> ExecutionStep {
    match (inside, has_editor, on_its_way) {
        (false, ..) => ExecutionStep::Nothing,
        (true, true, _) => ExecutionStep::MarkNow,
        (true, false, true) => ExecutionStep::MarkOnArrival,
        (true, false, false) => ExecutionStep::OpenThenMark,
    }
}

/// Drops the waiting execution points of files other than `keep`; true when `keep`'s document is
/// already on its way (with any waiting place, so it is not opened twice).
fn forget_other_execution_points(
    places: &mut HashMap<PathBuf, super::remote_host::PendingPlace>,
    keep: &Path,
) -> bool {
    places.retain(|p, place| {
        p == keep || !matches!(place, super::remote_host::PendingPlace::Execution(_))
    });
    places.contains_key(keep)
}

/// Windows of open remote projects, by `host|folder`.
#[derive(Default)]
struct RemoteWindows(HashMap<String, AnyWindowHandle>);

impl Global for RemoteWindows {}

fn key(host: &str, folder: &str) -> String {
    format!("{host}|{folder}")
}

/// One open document of the remote project.
struct Doc {
    sync: DocSync,
    /// When the editor and the replica have to meet again.
    follow: Follow,
    /// The next save overwrites the host file (after a conflict, or the user was told the host
    /// file changed since).
    overwrite: bool,
    /// Saving asks first: the host file changed since the window's text was based on it.
    confirm: bool,
}

/// The remote project of a window.
pub(super) struct RemoteWindow {
    pub(super) live: LiveProject,
    listing: RemoteListing,
    pub(super) stand_in: PathBuf,
    /// The window's state of the host functions: terminals, runs, ports, trust (`SPEC-0318` v2).
    pub(super) host: super::remote_host::HostState,
    title: String,
    docs: HashMap<PathBuf, Doc>,
    states: HashMap<String, DocState>,
    listing_now: HashSet<PathBuf>,
    /// Folders asked for again while their listing was on its way: that answer may be older
    /// than the change that made the tree ask, so they are listed once more.
    listing_again: HashSet<PathBuf>,
    shown_generation: u64,
}

impl Drop for RemoteWindow {
    fn drop(&mut self) {
        // A window that goes without closing its project still ends the session; recovery copies
        // of unsaved text stay (behaviour 11, 13).
        idecraft_remote::ports::close_all(&self.live.host(), &self.live.folder());
        if self.live.state() != ConnState::Closed {
            let live = self.live.clone();
            std::thread::spawn(move || {
                let _ = live.close(idecraft_remote::Unsaved::Discard);
            });
        }
    }
}

/// Opens `live` in a project window of its own (`SPEC-0317` v3 behaviours 5–6); the project is
/// published for agents (behaviour 20). Returns the window.
pub fn open_live_window(live: LiveProject, cx: &mut App) -> Result<AnyWindowHandle, String> {
    let host = live.host();
    let folder = live.folder();
    if let Some(w) = cx
        .try_global::<RemoteWindows>()
        .and_then(|m| m.0.get(&key(&host, &folder)).copied())
        && w.update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Ok(w);
    }
    let base = state_dir(cx).ok_or("no settings folder for remote projects")?;
    let root = stand_in(&base, &host, &folder);
    #[cfg(unix)]
    for dir in [base.clone(), root.clone()] {
        idecraft_session::reattach::private_dir(&dir).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let title = format!("{} — {host}", folder_name(&folder));
    let options = crate::project_windows::window_options(&title, cx);
    let shown = live.clone();
    let window_title = title.clone();
    let handle = cx
        .open_window(options, move |window, cx| {
            crate::x11_wake::until_first_frame(window);
            window.set_window_title(&window_title);
            let bench =
                cx.new(|cx| WorkbenchView::new_remote(shown.clone(), &root, window_title, cx));
            window.focus(&bench.read(cx).start_focus(cx));
            crate::appearance::watch_os_appearance(window);
            let weak = bench.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                weak.update(cx, |b, cx| b.remote_should_close(window, cx))
                    .unwrap_or(true)
            });
            // Copies left by a window that crashed are offered once the window is there.
            let asking = bench.clone();
            window.defer(cx, move |window, cx| {
                asking.update(cx, |b, cx| b.remote_offer_recovery(window, cx));
            });
            bench
        })
        .map_err(|e| e.to_string())?;
    live.publish();
    cx.default_global::<RemoteWindows>()
        .0
        .insert(key(&host, &folder), handle.into());
    Ok(handle.into())
}

fn rpc_text(e: &RpcError) -> String {
    e.message.clone()
}

impl WorkbenchView {
    /// A project window on a remote project: the tree and the editors go through `live`.
    pub(super) fn new_remote(
        live: LiveProject,
        root: &Path,
        title: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut b = Self::new(root, cx);
        // Nothing of the project runs on this machine; its stand-in folder holds no code.
        b.safe_mode = false;
        let listing = RemoteListing::default();
        b.tree = ProjectTree::remote(root, listing.clone());
        b.remote_project = Some(RemoteWindow {
            live,
            listing,
            stand_in: root.to_path_buf(),
            title,
            docs: HashMap::new(),
            states: HashMap::new(),
            listing_now: HashSet::new(),
            listing_again: HashSet::new(),
            shown_generation: u64::MAX,
            host: super::remote_host::HostState::default(),
        });
        // Host terminals live with their session (`SPEC-0318` v2 detail 2): no saved tabs.
        b.terminals_restored = true;
        if let Some(r) = &b.remote_project {
            r.live.set_window_root(root);
        }
        cx.on_app_quit(|this, _| {
            this.remote_bye();
            async {}
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                if this.update(cx, |b, cx| b.remote_tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        b
    }

    /// The remote project of this window, if it is one (host label and folder on the host).
    pub fn remote_project(&self) -> Option<(String, String)> {
        self.remote_project
            .as_ref()
            .map(|r| (r.live.host(), r.live.folder()))
    }

    /// The live project of this window (self-test, agents).
    pub fn remote_live(&self) -> Option<LiveProject> {
        self.remote_project.as_ref().map(|r| r.live.clone())
    }

    /// The window title of a remote project: folder name and host (behaviour 6).
    pub fn remote_title(&self) -> Option<String> {
        self.remote_project.as_ref().map(|r| r.title.clone())
    }

    /// The connection widget's text and tooltip, as the status bar shows them (behaviour 6).
    pub fn remote_widget_text(&self) -> Option<(String, String)> {
        let r = self.remote_project.as_ref()?;
        let host = r.live.host();
        let state = r.live.state();
        let pending: usize = r.states.values().map(|s| s.pending).sum();
        let waiting = r.states.values().any(|s| s.save_requested);
        let mut text = state.widget_text(&host);
        let mut tip = state.tooltip(&host, pending);
        if waiting {
            text.push_str(" · save pending");
            tip.push_str("\nA save waits for the connection and is carried out when it is back");
        }
        Some((text, tip))
    }

    /// What the project tree shows beside the project name (self-test, `SPEC-0317` v6 detail 11).
    pub fn tree_root_label(&self) -> String {
        crate::chrome::root_label(
            &self.root,
            std::env::var_os("HOME").map(PathBuf::from).as_deref(),
            self.remote_folders().as_ref(),
        )
    }

    /// Marks 0-based `line` of `path` as a stop of the debugged program (self-test).
    pub fn selftest_show_execution_point(
        &mut self,
        path: &Path,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_execution_point(path, line, window, cx);
    }

    /// The role the project tree shows for the folder at `path` (tests; `SPEC-0049` v2).
    pub fn tree_folder_info(&self, path: &Path) -> Option<idecraft_core::buildtools::FolderInfo> {
        self.tree.role_of(path).cloned()
    }

    /// The project's source folders as the tree knows them (tests; `SPEC-0332` v2 behaviour 13a).
    pub fn tree_source_roots(&self) -> Vec<PathBuf> {
        self.tree.source_roots()
    }

    /// The names of the project tree's visible rows (self-test).
    pub fn tree_row_names(&self) -> Vec<String> {
        self.tree.rows().into_iter().map(|r| r.name).collect()
    }

    /// The local path in the window of `rel` (a path relative to the host folder).
    pub fn remote_local_path(&self, rel: &str) -> Option<PathBuf> {
        let r = self.remote_project.as_ref()?;
        Some(if rel == "." || rel.is_empty() {
            r.stand_in.clone()
        } else {
            r.stand_in.join(rel)
        })
    }

    /// The stand-in folder and the folder on the host it stands for: popups show the host's
    /// paths, never the stand-in's (`SPEC-0317` v5 window detail 10).
    pub(super) fn remote_folders(&self) -> Option<(PathBuf, String)> {
        let r = self.remote_project.as_ref()?;
        Some((r.stand_in.clone(), r.live.folder()))
    }

    pub(super) fn remote_rel(&self, path: &Path) -> Option<String> {
        let r = self.remote_project.as_ref()?;
        let rel = path.strip_prefix(&r.stand_in).ok()?;
        let s = rel.to_string_lossy().replace('\\', "/");
        Some(if s.is_empty() { ".".into() } else { s })
    }

    /// Whether `path` is a remote document with unsaved changes or edits not acknowledged yet.
    pub(super) fn remote_unsaved(&self, path: &Path) -> bool {
        let Some(r) = &self.remote_project else {
            return false;
        };
        r.docs.get(path).is_some_and(|d| {
            r.states
                .get(&d.sync.doc)
                .is_some_and(|s| s.modified || s.pending > 0)
        })
    }

    /// Unsaved files of this window's remote project as `host:folder/file` (quitting and closing
    /// name them, behaviour 11).
    pub fn remote_unsaved_names(&self, cx: &gpui::App) -> Vec<String> {
        let Some(r) = &self.remote_project else {
            return Vec::new();
        };
        let (host, folder) = (r.live.host(), r.live.folder());
        let mut out: Vec<String> = r
            .docs
            .iter()
            .filter(|(p, d)| {
                self.editors
                    .get(*p)
                    .is_some_and(|e| e.read(cx).is_modified())
                    || r.states
                        .get(&d.sync.doc)
                        .is_some_and(|s| s.modified || s.pending > 0)
            })
            .map(|(_, d)| format!("{host}:{}/{}", folder.trim_end_matches('/'), d.sync.doc))
            .collect();
        out.sort();
        out
    }

    /// Remote Development… (`Remote.OpenProject`, the project popup; `SPEC-0317` v3 behaviour 1):
    /// the Remote Host window chooses a host and a folder and lists the recent remote projects.
    pub fn open_remote_development(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let recent = state_dir(cx)
            .map(|b| idecraft_remote::load_recent_remote(&recent_file(&b)))
            .unwrap_or_default();
        let view = self.ensure_remote(window, cx);
        view.update(cx, |v, cx| v.set_project_mode(true, recent, cx));
        self.show_remote = true;
        window.focus(&view.read(cx).field_focus(cx));
        cx.notify();
    }

    /// Opens `folder` of `host` as a remote project (behaviours 2–5): connect, find or bring the
    /// backend, start it, open the session — then a project window of its own.
    pub fn remote_open_project(&mut self, host: String, folder: String, cx: &mut Context<Self>) {
        self.remote_open_project_then(host, folder, |_, _| {}, cx);
    }

    /// As [`Self::remote_open_project`], then `done` with the project's state or the reason.
    pub fn remote_open_project_then(
        &mut self,
        host: String,
        folder: String,
        done: impl FnOnce(Result<Value, String>, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        if !idecraft_remote::bootstrap::absolute_folder(&folder) {
            done(
                Err("the folder must be an absolute path on the host".into()),
                cx,
            );
            return;
        }
        let spec = match idecraft_remote::HostSpec::parse(&host) {
            Ok(s) => s,
            Err(e) => {
                done(Err(e.to_string()), cx);
                return;
            }
        };
        // A first pairing runs before anything opens; the code goes no further than this.
        let code = idecraft_remote::HostSpec::pairing_code(&host);
        if let (idecraft_remote::HostSpec::Paired { address }, Some(code)) = (&spec, code) {
            let (address, label) = (address.clone(), spec.label());
            self.remote_pair_then(
                address,
                code,
                move |this, paired, cx| match paired {
                    Ok(_) => this.remote_open_project_then(label, folder, done, cx),
                    Err(e) => done(Err(e), cx),
                },
                cx,
            );
            return;
        }
        let host = spec.label();
        let found_as = match &spec {
            idecraft_remote::HostSpec::Ssh(_) => self.remote_target(&host).host,
            _ => host.clone(),
        };
        if let Some(open) = idecraft_remote::live::find(&found_as, &folder) {
            let shown = open_live_window(open.clone(), cx).map(|_| open.describe());
            done(shown, cx);
            return;
        }
        let Some(base) = state_dir(cx) else {
            done(Err("no settings folder for remote projects".into()), cx);
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        self.remote_opening = Some(stop.clone());
        if let Some(v) = &self.remote_view {
            let text = format!(
                "Opening {folder} on {host}: connecting, checking the backend, starting it, opening the session… Escape stops"
            );
            v.update(cx, |v, cx| v.show_busy(text, cx));
        }
        let opts = idecraft_remote::ServeOptions {
            backend_dir: self.remote_backend_dir.clone(),
            ..idecraft_remote::ServeOptions::default()
        };
        let recovery = recovery_dir(&base);
        let wanted = folder.clone();
        let extra = self.remote_extra.clone();
        let task = cx
            .background_executor()
            .spawn(async move { open_link(spec, &wanted, opts, recovery, &extra) });
        cx.spawn(async move |this, cx| {
            let opened = task.await;
            let _ = this.update(cx, |this, cx| {
                this.remote_opening = None;
                let view = this.remote_view.clone();
                let stopped = stop.load(Ordering::SeqCst);
                match opened {
                    Ok(p) if stopped => {
                        // Whatever was started on the host ends (behaviour 3).
                        std::thread::spawn(move || {
                            let mut p = p;
                            let _ = p.close(idecraft_remote::Unsaved::Discard);
                        });
                        done(Err("stopped".into()), cx);
                    }
                    Ok(p) => {
                        let _ =
                            idecraft_remote::remember_remote(&recent_file(&base), &host, &folder);
                        let live = LiveProject::start(p, Timing::default());
                        let shown = open_live_window(live.clone(), cx);
                        if let Some(v) = &view {
                            v.update(cx, |v, cx| v.clear_busy(cx));
                        }
                        this.status = Some(match &shown {
                            Ok(_) => format!("Opened {folder} on {host} as a remote project"),
                            Err(e) => format!("Cannot open the window: {e}"),
                        });
                        done(shown.map(|_| live.describe()), cx);
                    }
                    Err(e) => {
                        let text = e.to_string();
                        if let Some(v) = &view {
                            let shown = text.clone();
                            v.update(cx, |v, cx| v.show_error(shown, cx));
                        }
                        done(Err(text), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Pairs with the listener at `address` (`SPEC-0321` behaviour 17): the handshake in the
    /// background, then the six digits in a dialog. Only a click pins the host's key; declining
    /// or closing the dialog stores nothing on either side.
    pub fn remote_pair_then(
        &mut self,
        address: String,
        code: String,
        done: impl FnOnce(&mut Self, Result<String, String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(v) = &self.remote_view {
            let text = format!("Pairing with {address}…");
            v.update(cx, |v, cx| v.show_busy(text, cx));
        }
        let target = address.clone();
        let task = cx
            .background_executor()
            .spawn(async move { idecraft_remote::paired::begin_pairing(&target, &code) });
        cx.spawn(async move |this, cx| {
            let pending = match task.await {
                Ok(p) => p,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(v) = &this.remote_view {
                            let text = e.to_string();
                            v.update(cx, |v, cx| v.show_error(text, cx));
                        }
                        done(this, Err(e.to_string()), cx);
                    });
                    return;
                }
            };
            let digits = pending.digits().to_owned();
            let fingerprint = pending.host_fingerprint();
            let answer = cx.update(|cx| {
                let target = cx.active_window().or_else(|| cx.windows().into_iter().next())?;
                target
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        window.prompt(
                            gpui::PromptLevel::Warning,
                            &format!("Pair with {address}?"),
                            Some(&format!(
                                "Compare these digits with the ones the host shows: {digits}\n\nHost key {fingerprint}. Confirm only if both match."
                            )),
                            &pairing_buttons(),
                            cx,
                        )
                    })
                    .ok()
            });
            let confirmed = match answer {
                Ok(Some(prompt)) => matches!(prompt.await, Ok(0)),
                _ => false,
            };
            let outcome = if confirmed {
                cx.background_executor()
                    .spawn(async move {
                        idecraft_remote::paired::finish_pairing(pending).map_err(|e| e.to_string())
                    })
                    .await
            } else {
                pending.decline();
                Err("the pairing was declined; nothing was stored".to_owned())
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(v) = &this.remote_view {
                    match &outcome {
                        Ok(_) => v.update(cx, |v, cx| v.clear_busy(cx)),
                        Err(e) => {
                            let text = e.clone();
                            v.update(cx, |v, cx| v.show_error(text, cx));
                        }
                    }
                }
                done(this, outcome, cx);
            });
        })
        .detach();
    }

    /// Escape while a remote project opens (behaviour 3).
    pub fn remote_cancel_open(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.remote_opening.take() {
            stop.store(true, Ordering::SeqCst);
            if let Some(v) = &self.remote_view {
                v.update(cx, |v, cx| {
                    v.show_error(
                        "Opening stopped; whatever was started on the host is ended".into(),
                        cx,
                    )
                });
            }
        }
        cx.notify();
    }

    /// Opens the remote document at `path` (behaviour 8) in the background, then its editor.
    pub(super) fn remote_open_doc(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(rel), Some(r)) = (self.remote_rel(path), &self.remote_project) else {
            return;
        };
        if let Some(d) = r.docs.get(path)
            && let Some(e) = self.editors.get(path).cloned()
        {
            let _ = d;
            window.focus(&e.read(cx).focus());
            return;
        }
        let live = r.live.clone();
        let host = live.host();
        self.status = Some(format!("Opening {rel} on {host}…"));
        let task = cx
            .background_executor()
            .spawn(async move { live.open_doc(&rel).map_err(|e| (rel, e)) });
        let handle = window.window_handle();
        let local = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let opened = task.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    match opened {
                        Ok((_, text, _, sync)) => {
                            if let Some(r) = this.remote_project.as_mut() {
                                r.docs.insert(
                                    local.clone(),
                                    Doc {
                                        follow: Follow::new(0),
                                        sync,
                                        overwrite: false,
                                        confirm: false,
                                    },
                                );
                            }
                            this.open_with_text(&local, text, window, cx);
                            // A search result opened it: go to its place now.
                            let place = this
                                .remote_project
                                .as_mut()
                                .and_then(|r| r.host.pending_place.remove(&local));
                            if let (Some(place), Some(e)) = (place, this.editors.get(&local)) {
                                e.update(cx, |e, cx| place.apply(e, cx));
                            }
                            let rev = this.editors.get(&local).map(|e| e.read(cx).revision());
                            if let (Some(r), Some(rev)) = (this.remote_project.as_mut(), rev)
                                && let Some(d) = r.docs.get_mut(&local)
                            {
                                d.follow = Follow::new(rev);
                            }
                        }
                        Err((rel, e)) => {
                            if let Some(r) = this.remote_project.as_mut() {
                                r.host.pending_place.remove(&local);
                            }
                            this.status =
                                Some(format!("Cannot open {rel} on {host}: {}", rpc_text(&e)));
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// Opens the remote document at `path` and puts the caret on 0-based `line` and `column` once
    /// it is there (a jump from ⌘B to another remote file).
    pub(super) fn remote_open_at(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remote_pend(
            path,
            super::remote_host::PendingPlace::GoTo(line, Some(column)),
        );
        self.remote_open_doc(path, window, cx);
    }

    /// Shows where the program stopped in `path` of this remote project (`SPEC-0322` v5 behaviour
    /// 14): at once in an open editor, else when the document has arrived. A place of another file
    /// that was still on its way is out of date. A frame outside the project opens nothing.
    pub(super) fn remote_show_execution_point(
        &mut self,
        path: &Path,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inside = self.remote_rel(path).is_some_and(|rel| rel != ".");
        let has_editor = self.editors.contains_key(path);
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let waiting = forget_other_execution_points(&mut r.host.pending_place, path);
        match execution_point_step(inside, has_editor, waiting) {
            ExecutionStep::Nothing => {}
            ExecutionStep::MarkNow => {
                self.open(path, window, cx);
                if let Some(editor) = self.editors.get(path) {
                    editor.update(cx, |e, cx| {
                        e.go_to_line(line, cx);
                        e.set_execution_line(Some(line), cx);
                    });
                }
            }
            ExecutionStep::MarkOnArrival => {
                self.remote_pend(path, super::remote_host::PendingPlace::Execution(line));
            }
            ExecutionStep::OpenThenMark => {
                self.remote_pend(path, super::remote_host::PendingPlace::Execution(line));
                self.remote_open_doc(path, window, cx);
            }
        }
    }

    /// The debug session ended: a document still on its way gets no execution point.
    pub(super) fn remote_forget_execution_point(&mut self) {
        if let Some(r) = self.remote_project.as_mut() {
            r.host
                .pending_place
                .retain(|_, p| !matches!(p, super::remote_host::PendingPlace::Execution(_)));
        }
    }

    /// The user typed into a remote document: its edit goes to the session at once.
    pub(super) fn remote_edited(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.remote_sync(path, cx);
    }

    /// Meets the editor of `path` with the session's replica (behaviour 9); never waits.
    fn remote_sync(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(editor) = self.editors.get(path).cloned() else {
            return;
        };
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let Some(d) = r.docs.get_mut(path) else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let outcome = r.live.sync_doc(&mut d.sync, &text);
        match &outcome {
            Ok(Synced::Apply(ops)) => {
                let ranges: Vec<(std::ops::Range<usize>, String)> = ops
                    .iter()
                    .map(|o| (o.at..o.at + o.delete, o.insert.clone()))
                    .collect();
                editor.update(cx, |e, cx| e.merge_ranges(&ranges, cx));
                if editor.read(cx).state.buffer.text() != d.sync.synced {
                    // The editor could not take the change (it refused an edit): it shows the
                    // session's text, which holds both sides.
                    let synced = d.sync.synced.clone();
                    editor.update(cx, |e, cx| {
                        let caret = e.caret();
                        e.set_text(&synced, cx);
                        e.select_range(caret..caret, cx);
                    });
                }
            }
            Ok(Synced::Same | Synced::Busy) => {}
            Err(e) => {
                let name = d.sync.doc.clone();
                self.status = Some(format!("{name}: {}", rpc_text(e)));
            }
        }
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let Some(d) = r.docs.get_mut(path) else {
            return;
        };
        let state = r.states.get(&d.sync.doc).copied();
        d.follow.met(
            &outcome,
            &mut d.sync,
            editor.read(cx).revision(),
            state.as_ref(),
        );
        // Saved on the host (by this window, after a resume, or by another client) and the editor
        // shows exactly that text: the tab is no longer marked.
        if !d.sync.modified
            && editor.read(cx).is_modified()
            && editor.read(cx).state.buffer.text() == d.sync.synced
        {
            editor.update(cx, |e, cx| e.mark_saved(cx));
        }
    }

    /// One look of the window at its remote project: folder listings, file tree events, the
    /// documents' states, the editors' text and the connection widget.
    fn remote_tick(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if r.live.state() == ConnState::Closed {
            // Closed by an agent (`remote_project_close`) or another way: the window goes.
            let k = key(&r.live.host(), &r.live.folder());
            let handle = cx
                .try_global::<RemoteWindows>()
                .and_then(|m| m.0.get(&k).copied());
            {
                let m = cx.default_global::<RemoteWindows>();
                m.0.remove(&k);
            }
            if let Some(h) = handle {
                cx.defer(move |cx| {
                    let _ = h.update(cx, |_, window, _| window.remove_window());
                });
            }
            return;
        }
        // What the host's functions announced (SPEC-0318): diagnostics of the documents, Git moves.
        let events = r.live.take_host_events();
        if !events.is_empty() {
            self.remote_host_events(events, cx);
        }
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        // Folders the tree asked for (behaviour 7).
        for dir in r.listing.take_wanted() {
            if !r.listing_now.insert(dir.clone()) {
                r.listing_again.insert(dir);
                continue;
            }
            let rel = dir
                .strip_prefix(&r.stand_in)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let rel = if rel.is_empty() { ".".to_owned() } else { rel };
            let live = r.live.clone();
            let task = cx
                .background_executor()
                .spawn(async move { live.list(&rel) });
            cx.spawn(async move |this, cx| {
                let listed = task.await;
                let _ = this.update(cx, |this, cx| {
                    let Some(r) = this.remote_project.as_mut() else {
                        return;
                    };
                    r.listing_now.remove(&dir);
                    match listed {
                        Ok(entries) => {
                            let entries = entries
                                .iter()
                                .filter_map(|e| {
                                    Some((e["kind"] == "dir", e["name"].as_str()?.to_owned()))
                                })
                                .collect();
                            r.listing.put(dir.clone(), entries);
                            if r.listing_again.remove(&dir) {
                                r.listing.forget(&dir);
                            }
                            this.tree = this.tree.reloaded();
                        }
                        // Disconnected: asked again once the connection is back.
                        Err(e) if e.code == ErrorCode::Unavailable => {}
                        Err(e) => {
                            this.status = Some(format!("Cannot list a folder: {}", e.message))
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        // Files created, changed, renamed or deleted on the host (behaviour 7).
        if let Some((events, stale)) = r.live.take_tree_events() {
            let mut reload = stale;
            if stale && let Ok(mut d) = r.listing.dirs.lock() {
                d.clear();
            }
            for e in &events {
                for k in ["path", "from"] {
                    let Some(p) = e.get(k).and_then(Value::as_str) else {
                        continue;
                    };
                    let local = r.stand_in.join(p);
                    let parent = local.parent().unwrap_or(&r.stand_in).to_path_buf();
                    r.listing.forget(&parent);
                    r.listing.forget(&local);
                    reload = true;
                }
            }
            if reload {
                self.tree = self.tree.reloaded();
                cx.notify();
            }
        }
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        if let Some(states) = r.live.doc_states() {
            r.states = states;
        }
        // Documents whose editor was closed are closed in the session too.
        let gone: Vec<PathBuf> = r
            .docs
            .keys()
            .filter(|p| !self.editors.contains_key(*p))
            .cloned()
            .collect();
        for p in gone {
            if let Some(d) = r.docs.remove(&p) {
                let live = r.live.clone();
                let unsaved = r
                    .states
                    .get(&d.sync.doc)
                    .is_some_and(|s| s.modified || s.pending > 0);
                if !unsaved {
                    std::thread::spawn(move || {
                        let _ = live.with(|l| l.session().close_doc(&d.sync.doc));
                    });
                }
            }
        }
        let due: Vec<PathBuf> = r
            .docs
            .iter()
            .filter(|(p, d)| {
                let rev = self.editors.get(*p).map(|e| e.read(cx).revision());
                let state = r.states.get(&d.sync.doc);
                d.follow.due(&d.sync, rev, state)
            })
            .map(|(p, _)| p.clone())
            .collect();
        for p in due {
            self.remote_sync(&p, cx);
        }
        self.remote_host_tick(cx);
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let generation = r.live.generation();
        if generation != r.shown_generation {
            r.shown_generation = generation;
            // Reconnected: folders not listed during the break are asked for again.
            if matches!(r.live.state(), ConnState::Connected { .. }) {
                // The tree follows the host's files (behaviour 7): subscribed when the window
                // opens and again after a new backend (a resumed session keeps it; the call is
                // idempotent).
                let live = r.live.clone();
                cx.background_executor()
                    .spawn(async move {
                        let _ = live.call("fs.watch", json!({ "exclude": [] }));
                    })
                    .detach();
                self.tree = self.tree.reloaded();
            }
            cx.notify();
        }
    }

    /// ⌘S on a remote document (behaviour 10): saved on the host; a conflict or a host file that
    /// changed since asks for a second save; while disconnected the save waits and says so.
    pub(super) fn remote_save(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.remote_sync(path, cx);
        let Some(r) = self.remote_project.as_mut() else {
            return;
        };
        let host = r.live.host();
        let Some(d) = r.docs.get_mut(path) else {
            return;
        };
        let id = d.sync.doc.clone();
        let changed = r.states.get(&id).is_some_and(|s| s.disk_changed) || d.confirm;
        if changed && !d.overwrite {
            d.overwrite = true;
            self.status = Some(format!(
                "{id} changed on {host} since this text was based on it: save again to overwrite it (the host keeps its version in the local history)"
            ));
            cx.notify();
            return;
        }
        let overwrite = d.overwrite;
        let live = r.live.clone();
        self.status = Some(format!("Saving {id} on {host}…"));
        let task = cx.background_executor().spawn({
            let id = id.clone();
            async move { live.save(&id, overwrite) }
        });
        let local = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            let _ = this.update(cx, |this, cx| {
                let doc = this
                    .remote_project
                    .as_mut()
                    .and_then(|r| r.docs.get_mut(&local));
                this.status = Some(match saved {
                    Ok(_) => {
                        if let Some(d) = doc {
                            d.overwrite = false;
                            d.confirm = false;
                        }
                        "Saved on host".to_owned()
                    }
                    Err(e) if e.code == ErrorCode::Conflict => {
                        if let Some(d) = doc {
                            d.overwrite = true;
                        }
                        format!(
                            "{id} changed on {host} since it was loaded: nothing was written. Save again to overwrite it"
                        )
                    }
                    Err(e) if e.code == ErrorCode::Unavailable => e.message,
                    Err(e) => format!("Could not save {id} on {host}: {}", e.message),
                });
                this.remote_sync(&local, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves every remote document now, waiting for the host (quitting with Save All). Files that
    /// stay unsaved are named by [`Self::remote_unsaved_names`].
    pub(super) fn remote_save_all_now(&mut self, cx: &mut Context<Self>) -> Vec<PathBuf> {
        let paths: Vec<PathBuf> = self
            .remote_project
            .as_ref()
            .map(|r| r.docs.keys().cloned().collect())
            .unwrap_or_default();
        for p in paths {
            self.remote_sync(&p, cx);
            let Some(r) = self.remote_project.as_mut() else {
                break;
            };
            let Some(d) = r.docs.get(&p) else {
                continue;
            };
            let unsaved = r
                .states
                .get(&d.sync.doc)
                .is_none_or(|s| s.modified || s.pending > 0);
            if unsaved && r.live.save(&d.sync.doc, d.overwrite).is_ok() {
                if let Some(s) = r.live.doc_states() {
                    r.states = s;
                }
                if let Some(e) = self.editors.get(&p).cloned() {
                    e.update(cx, |e, cx| e.mark_saved(cx));
                }
            }
        }
        Vec::new()
    }

    /// Closing a remote project's window (behaviour 11): asks about unsaved files in a question
    /// drawn in the window (`SPEC-0281` v2 behaviour 6), then ends the session. Returns whether the
    /// window may close now.
    fn remote_should_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unsaved = self.remote_unsaved_names(cx);
        if unsaved.is_empty() {
            self.remote_bye();
            return true;
        }
        self.drawn_question = Some((
            crate::dialogs::DrawnQuestion::remote_close(&unsaved),
            super::DrawnFor::RemoteClose,
        ));
        cx.notify();
        false
    }

    /// An answer to the question of [`Self::remote_should_close`]: 0 Save All, 1 Don't Save, else
    /// Cancel.
    pub(super) fn answer_remote_close(
        &mut self,
        answer: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match answer {
            0 => {
                self.remote_save_all_now(cx);
                if self.remote_unsaved_names(cx).is_empty() {
                    self.remote_bye();
                    window.remove_window();
                } else {
                    self.status = Some("Some files could not be saved on the host".into());
                    cx.notify();
                }
            }
            1 => {
                self.remote_bye();
                window.remove_window();
            }
            _ => {}
        }
    }

    /// Ends the session (`session.bye`), waiting at most 2 s for the host.
    fn remote_bye(&mut self) {
        let Some(r) = &self.remote_project else {
            return;
        };
        if r.live.state() == ConnState::Closed {
            return;
        }
        let live = r.live.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(live.close(idecraft_remote::Unsaved::Discard));
        });
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }

    /// The connection widget of the status bar (behaviour 6): host, state, and a menu.
    pub(super) fn remote_widget(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let r = self.remote_project.as_ref()?;
        let (text, tip) = self.remote_widget_text()?;
        let p = Palette::current();
        let dot = match r.live.state() {
            ConnState::Connected { .. } => p.success_text,
            ConnState::Reconnecting { .. } => p.accent,
            ConnState::Disconnected { .. } | ConnState::Closed => p.error_text,
        };
        Some(
            div()
                .id("remote-connection-widget")
                .tip_text(
                    "remote-connection-widget",
                    idecraft_ui::tooltip::Tip::plain(tip),
                )
                .map(status_widget)
                .on_click(cx.listener(|this, _, window, cx| this.remote_widget_menu(window, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().size(px(7.)).rounded_full().bg(c(dot)))
                        .child(text),
                )
                .into_any_element(),
        )
    }

    /// The widget's menu: Reconnect, Disconnect, Show Connection Log, Backend Info (never the
    /// resume token).
    pub fn remote_widget_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = &self.remote_project else {
            return;
        };
        let host = r.live.host();
        let items = [
            "Reconnect",
            "Disconnect",
            "Show Connection Log",
            "Backend Info",
            "Ports…",
        ]
        .map(str::to_owned)
        .to_vec();
        self.show_jvm_popup(
            host.clone(),
            items,
            window,
            cx,
            move |this, i, window, cx| {
                this.remote_widget_action(i, window, cx);
            },
        );
    }

    /// Item `i` of the widget's menu.
    pub fn remote_widget_action(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = &self.remote_project else {
            return;
        };
        let live = r.live.clone();
        let host = live.host();
        match i {
            0 => {
                live.reconnect_now();
                self.status = Some(format!("Reconnecting to {host}…"));
            }
            1 => {
                std::thread::spawn(move || live.disconnect());
                self.status = Some(format!(
                    "Disconnected from {host}; typing goes on, Reconnect connects again"
                ));
            }
            2 => {
                let mut lines = live.log();
                if lines.is_empty() {
                    lines.push("nothing yet".into());
                }
                let lines = lines.split_off(lines.len().saturating_sub(40));
                self.show_jvm_popup(
                    format!("Connection log — {host}"),
                    lines,
                    window,
                    cx,
                    |_, _, _, _| {},
                );
            }
            3 => {
                let info = live.backend_info();
                self.show_jvm_popup(
                    format!("Backend — {host}"),
                    info,
                    window,
                    cx,
                    |_, _, _, _| {},
                );
            }
            4 => {
                // Opened after the menu's own event is handled.
                cx.defer_in(window, |this, window, cx| {
                    this.remote_show_ports(window, cx)
                });
            }
            _ => {}
        }
        cx.notify();
    }

    /// Offers the recovery copies a crashed window left for this project (behaviour 13).
    pub(super) fn remote_offer_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = &self.remote_project else {
            return;
        };
        let copies = r.live.recovery_copies();
        if copies.is_empty() {
            return;
        }
        let n = copies.len();
        let files = copies
            .iter()
            .map(|c| c.path.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let title = format!(
            "{n} file{} had unsaved changes when IDECraft last closed: {files}",
            if n == 1 { "" } else { "s" }
        );
        let items = vec![
            format!("Restore {n} unsaved file{}", if n == 1 { "" } else { "s" }),
            "Discard the copies".to_owned(),
            "Decide later".to_owned(),
        ];
        self.show_jvm_popup(title, items, window, cx, move |this, i, window, cx| {
            this.remote_recovery_choice(i, copies.clone(), window, cx);
        });
    }

    /// The answer to the recovery question: 0 restores, 1 discards, else nothing.
    pub fn remote_recovery_choice(
        &mut self,
        choice: usize,
        copies: Vec<RecoveryCopy>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(r) = &self.remote_project else {
            return;
        };
        let live = r.live.clone();
        match choice {
            0 => {
                let task = cx.background_executor().spawn({
                    let live = live.clone();
                    async move { live.restore(&copies) }
                });
                let handle = window.window_handle();
                cx.spawn(async move |this, cx| {
                    let restored = task.await;
                    let _ = cx.update_window(handle, |_, window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            let mut notes = Vec::new();
                            for (doc, outcome) in &restored {
                                let Some(local) = this.remote_local_path(doc) else {
                                    continue;
                                };
                                match outcome {
                                    Restored::Failed(why) => notes.push(format!("{doc}: {why}")),
                                    Restored::HostChanged => {
                                        notes.push(format!(
                                            "{doc}: changed on the host since; saving asks first"
                                        ));
                                        this.remote_open_doc_confirming(&local, window, cx);
                                    }
                                    Restored::Followed | Restored::Unchanged => {
                                        this.remote_open_doc(&local, window, cx);
                                    }
                                }
                            }
                            this.status = Some(if notes.is_empty() {
                                format!("Restored {} file(s)", restored.len())
                            } else {
                                format!("Restored; {}", notes.join("; "))
                            });
                            cx.notify();
                        });
                    });
                })
                .detach();
            }
            1 => {
                live.discard_copies(&copies);
                self.status = Some("The recovery copies were discarded".into());
            }
            _ => {}
        }
        cx.notify();
    }

    fn remote_open_doc_confirming(
        &mut self,
        local: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remote_open_doc(local, window, cx);
        // The document opens in the background; the flag waits for it.
        let path = local.to_path_buf();
        cx.spawn(async move |this, cx| {
            for _ in 0..200 {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let set = this
                    .update(cx, |this, _| {
                        this.remote_project
                            .as_mut()
                            .and_then(|r| r.docs.get_mut(&path))
                            .map(|d| d.confirm = true)
                            .is_some()
                    })
                    .unwrap_or(true);
                if set {
                    break;
                }
            }
        })
        .detach();
    }

    /// New file or folder `name` in `dir` on the host (the tree's New, behaviour 7).
    pub(super) fn remote_project_create(
        &mut self,
        dir: &Path,
        name: &str,
        folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a name".into());
        }
        let rel_dir = self.remote_rel(dir).ok_or("not in the project")?;
        let rel = if rel_dir == "." {
            name.to_owned()
        } else {
            format!("{rel_dir}/{name}")
        };
        let r = self.remote_project.as_ref().ok_or("no remote project")?;
        r.live
            .try_call(
                "fs.create",
                json!({"path": rel, "kind": if folder { "dir" } else { "file" }}),
            )
            .map_err(|e| rpc_text(&e))?;
        r.listing.forget(dir);
        self.tree = self.tree.reloaded();
        let local = self.remote_local_path(&rel).ok_or("not in the project")?;
        if !folder {
            self.remote_open_doc(&local, window, cx);
        }
        self.status = Some(format!("Created {rel} on {}", r_host(self)));
        Ok(())
    }

    /// Renames `path` to `name` on the host (the tree's Rename).
    pub(super) fn remote_project_rename(
        &mut self,
        path: &Path,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.contains('/') {
            return Err("Enter a new name without /".into());
        }
        let from = self.remote_rel(path).ok_or("not in the project")?;
        if from == "." {
            return Err("The project folder itself cannot be renamed".into());
        }
        let to = match from.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{name}"),
            None => name.to_owned(),
        };
        let r = self.remote_project.as_ref().ok_or("no remote project")?;
        r.live
            .try_call("fs.rename", json!({"from": from, "to": to}))
            .map_err(|e| rpc_text(&e))?;
        if let Some(parent) = path.parent() {
            r.listing.forget(parent);
        }
        self.tree = self.tree.reloaded();
        self.status = Some(format!("Renamed {from} to {to} on {}", r_host(self)));
        cx.notify();
        Ok(())
    }

    /// What Delete would remove on the host (no count of what a folder holds: the host is not
    /// walked for the question).
    pub(super) fn remote_delete_plans(&self, paths: &[PathBuf]) -> Vec<DeletePlan> {
        let Some(r) = &self.remote_project else {
            return Vec::new();
        };
        paths
            .iter()
            .filter(|p| **p != r.stand_in)
            .map(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dir = p
                    .parent()
                    .and_then(|parent| {
                        r.listing
                            .dirs
                            .lock()
                            .ok()?
                            .get(parent)?
                            .iter()
                            .find(|(_, n)| *n == name)
                            .map(|(d, _)| *d)
                    })
                    .unwrap_or(false);
                DeletePlan {
                    path: p.clone(),
                    dir,
                    files: usize::from(!dir),
                    dirs: 0,
                }
            })
            .collect()
    }

    /// Deletes `plan` on the host (the tree's Delete).
    pub(super) fn remote_project_delete(&mut self, plan: &DeletePlan) -> Result<PathBuf, String> {
        let rel = self.remote_rel(&plan.path).ok_or("not in the project")?;
        let r = self.remote_project.as_ref().ok_or("no remote project")?;
        r.live
            .try_call("fs.delete", json!({"path": rel, "recursive": plan.dir}))
            .map_err(|e| rpc_text(&e))?;
        if let Some(parent) = plan.path.parent() {
            r.listing.forget(parent);
        }
        Ok(plan.path.clone())
    }

    /// Opens a remote project for an agent in the front window (`remote_project_open`,
    /// behaviour 20); `done` gets the project's state or the reason.
    pub fn pair_remote_host_in_front(
        address: String,
        code: String,
        done: impl FnOnce(Result<String, String>, &mut App) + 'static,
        cx: &mut App,
    ) {
        let Some(bench) = crate::project_windows::workbenches(cx).into_iter().next() else {
            done(Err("no project window is open".into()), cx);
            return;
        };
        bench.update(cx, |b, cx| {
            b.remote_pair_then(address, code, move |_, r, cx| done(r, cx), cx)
        });
    }

    /// Opens `folder` of `host` as a remote project in the window in front (agents).
    pub fn open_remote_project_in_front(
        host: String,
        folder: String,
        done: impl FnOnce(Result<Value, String>, &mut App) + 'static,
        cx: &mut App,
    ) {
        let Some(bench) = crate::project_windows::workbenches(cx).into_iter().next() else {
            done(Err("no project window is open".into()), cx);
            return;
        };
        bench.update(cx, |b, cx| {
            b.remote_open_project_then(host, folder, done, cx)
        });
    }
}

/// Connects a project over the route of `spec` (`SPEC-0321`); runs in the background.
fn open_link(
    spec: idecraft_remote::HostSpec,
    folder: &str,
    opts: idecraft_remote::ServeOptions,
    recovery: PathBuf,
    extra: &[String],
) -> Result<Box<dyn idecraft_remote::ProjectLink>, idecraft_remote::RemoteError> {
    if let idecraft_remote::HostSpec::Paired { address } = &spec {
        return Ok(Box::new(idecraft_remote::PairedProject::open(
            address,
            folder,
            Some(recovery),
        )?));
    }
    let binary =
        std::env::current_exe().map_err(|e| idecraft_remote::RemoteError::Failed(e.to_string()))?;
    let route = idecraft_remote::HostRoute::for_spec(&spec, extra)?;
    Ok(Box::new(idecraft_remote::RemoteProject::open_route(
        route,
        folder,
        &binary,
        opts,
        Some(recovery),
    )?))
}

/// How long a host question waits for the window's own edits of the document to be acknowledged
/// (SPEC-0318 behaviour 4); typing never waits for it.
const SETTLE: Duration = Duration::from_secs(2);

/// How long a host question waits for its answer (the first start of a server included).
const HOST_LIMIT: Duration = Duration::from_secs(60);

/// A question about a remote document to the host's language servers (SPEC-0318 behaviours 10
/// and 11), asked on the backend's text at the revision the window's edits reached.
#[derive(Clone)]
pub(crate) struct RemoteAsk {
    live: LiveProject,
    doc: String,
    stand_in: PathBuf,
}

impl RemoteAsk {
    /// `method` with `params` plus the document and its settled revision; a revision that moved
    /// on meanwhile is asked once more.
    fn ask(&self, method: &str, mut params: Value) -> Result<Value, String> {
        let mut again = true;
        loop {
            let revision = self
                .live
                .settled_revision(&self.doc, SETTLE)
                .ok_or_else(|| format!("{} is not open on {} yet", self.doc, self.live.host()))?;
            params["doc"] = json!(self.doc);
            params["revision"] = json!(revision);
            match self.live.host_call(method, params.clone(), HOST_LIMIT) {
                Err(e) if e.code == ErrorCode::StaleRevision && again => again = false,
                Err(e) => return Err(rpc_text(&e)),
                Ok(v) => return Ok(v),
            }
        }
    }

    /// Signature help at a position (`lsp.signature`, SPEC-0080 on the host).
    pub(crate) fn signature(
        &self,
        line: usize,
        character: usize,
    ) -> Result<Option<idecraft_languages::insight::SignatureHelp>, String> {
        let v = self.ask(
            "lsp.signature",
            json!({ "line": line, "character": character }),
        )?;
        Ok(serde_json::from_value(v["signature"].clone()).unwrap_or(None))
    }

    /// Semantic tokens of the document (`lsp.semantic`, SPEC-0290 on the host); the colour-scheme
    /// keys are computed here from the token types (v2 detail 9).
    pub(crate) fn semantic(
        &self,
    ) -> Result<Vec<idecraft_languages::semantic::SemanticToken>, String> {
        let v = self.ask("lsp.semantic", json!({}))?;
        let family = idecraft_ui::theme::KeyFamily::of_path(&self.doc);
        Ok(v["tokens"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let token_type = t["type"].as_str()?.to_owned();
                let modifiers: Vec<String> = t["modifiers"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|m| m.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let refs: Vec<&str> = modifiers.iter().map(String::as_str).collect();
                let key = idecraft_ui::theme::semantic_key_in(&token_type, &refs, family);
                Some(idecraft_languages::semantic::SemanticToken {
                    line: t["line"].as_u64()?,
                    start: t["start"].as_u64()?,
                    end: t["end"].as_u64()?,
                    token_type,
                    modifiers,
                    key,
                })
            })
            .collect())
    }

    /// IDECraft's inspections of the document on the host (`lsp.inspect`, behaviour 9): id, level,
    /// message, start and end.
    pub(crate) fn inspect(&self) -> Result<Vec<crate::editor_view::HostFinding>, String> {
        let v = self.ask("lsp.inspect", json!({}))?;
        let n = |i: &Value, k: &str| i[k].as_u64().unwrap_or(0) as usize;
        Ok(v["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| {
                (
                    i["id"].as_str().unwrap_or_default().to_owned(),
                    i["level"].as_str().unwrap_or_default().to_owned(),
                    i["message"].as_str().unwrap_or_default().to_owned(),
                    (n(i, "start_line"), n(i, "start_character")),
                    (n(i, "end_line"), n(i, "end_character")),
                )
            })
            .collect())
    }

    /// The host server's code actions for `range` (`lsp.code_actions`, SPEC-0076).
    pub(crate) fn code_actions(
        &self,
        range: &idecraft_languages::insight::Range,
    ) -> Result<Vec<idecraft_languages::actions::CodeAction>, String> {
        let v = self.ask("lsp.code_actions", json!({ "range": range }))?;
        Ok(serde_json::from_value(v["actions"].clone()).unwrap_or_default())
    }

    /// Plans (`apply: false`) or applies the action titled `title` on the host
    /// (`lsp.apply_action`, behaviour 12): the files it changes, and with `apply` the changed ones.
    pub(crate) fn apply_action(
        &self,
        range: &idecraft_languages::insight::Range,
        title: &str,
        apply: bool,
    ) -> Result<Value, String> {
        self.ask(
            "lsp.apply_action",
            json!({ "range": range, "title": title, "apply": apply }),
        )
    }

    /// Renames the symbol at a position on the host (`lsp.rename` with `apply: true`, behaviour
    /// 12): old and new name, the planned and the changed files.
    pub(crate) fn rename(
        &self,
        line: usize,
        character: usize,
        new_name: &str,
    ) -> Result<Value, String> {
        self.ask(
            "lsp.rename",
            json!({ "line": line, "character": character, "new_name": new_name, "apply": true }),
        )
    }

    /// The window's path of a relative path from the host; paths that leave the project are
    /// refused (SEC-14).
    fn local(&self, doc: &str) -> Option<PathBuf> {
        idecraft_session::paths::check_relative(doc)
            .ok()
            .filter(|d| d != ".")
            .map(|d| self.stand_in.join(d))
    }

    /// Completion items at a position (SPEC-0072 on the host).
    pub(crate) fn complete(
        &self,
        line: usize,
        character: usize,
        limit: usize,
    ) -> Option<Vec<idecraft_languages::insight::CompletionItem>> {
        let v = self
            .ask(
                "lsp.complete",
                json!({ "line": line, "character": character, "limit": limit }),
            )
            .ok()?;
        serde_json::from_value(v["items"].clone()).ok()
    }

    /// Hover text at a position (SPEC-0077 on the host).
    pub(crate) fn hover(
        &self,
        line: usize,
        character: usize,
    ) -> Result<Option<idecraft_languages::insight::Hover>, String> {
        let v = self.ask("lsp.hover", json!({ "line": line, "character": character }))?;
        Ok(serde_json::from_value(v["hover"].clone()).unwrap_or(None))
    }

    /// Definition, type, implementations or references at a position; targets inside the
    /// project become remote documents of this window, others are names only.
    pub(crate) fn found(
        &self,
        line: usize,
        character: usize,
        kind: idecraft_languages::insight::LocationKind,
    ) -> Result<idecraft_languages::insight::Found, String> {
        use idecraft_languages::insight::{Found, Location, LocationKind};
        let kind = match kind {
            LocationKind::Definition => "definition",
            LocationKind::References => "references",
            LocationKind::TypeDefinition => "type_definition",
            LocationKind::Implementation => "implementation",
        };
        let v = self.ask(
            "lsp.locations",
            json!({ "line": line, "character": character, "kind": kind }),
        )?;
        let locations = v["locations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let path = self.local(l["doc"].as_str()?)?;
                let range = serde_json::from_value(l["range"].clone()).ok()?;
                Some(Location { path, range })
            })
            .collect();
        let outside = v["outside"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect();
        Ok(Found {
            locations,
            outside,
            outside_places: Vec::new(),
        })
    }
}

impl WorkbenchView {
    /// A host question about `path`, when it is a document of this window's remote project.
    pub(crate) fn remote_ask(&self, path: &Path) -> Option<RemoteAsk> {
        let r = self.remote_project.as_ref()?;
        let rel = path
            .strip_prefix(&r.stand_in)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        (!rel.is_empty()).then(|| RemoteAsk {
            live: r.live.clone(),
            doc: rel,
            stand_in: r.stand_in.clone(),
        })
    }

    /// Applies what the host announced: diagnostics to the editors of their documents (SPEC-0318
    /// behaviour 8), Git moves to the change markers (behaviour 16).
    fn remote_host_events(&mut self, events: Vec<(String, Value)>, cx: &mut Context<Self>) {
        let Some(stand_in) = self.remote_project.as_ref().map(|r| r.stand_in.clone()) else {
            return;
        };
        let mut git_moved = false;
        for (method, p) in events {
            match method.as_str() {
                "lsp.diagnostics" => {
                    let Some(doc) = p["doc"]
                        .as_str()
                        .and_then(|d| idecraft_session::paths::check_relative(d).ok())
                    else {
                        continue;
                    };
                    let path = stand_in.join(&doc);
                    let found: Vec<idecraft_languages::lsp::Diagnostic> =
                        serde_json::from_value(p["items"].clone()).unwrap_or_default();
                    if let Some(editor) = self.editors.get(&path) {
                        editor.update(cx, |e, cx| e.set_diagnostics(&found, cx));
                    }
                    for twin in self.twins(&path) {
                        twin.update(cx, |e, cx| e.set_diagnostics(&found, cx));
                    }
                }
                "git.changed" => {
                    git_moved = true;
                    if let Some(r) = self.remote_project.as_mut() {
                        r.host.branch = p["branch"].as_str().map(str::to_owned);
                    }
                }
                "port.detected" | "port.gone" => {
                    self.remote_port_event(&method, &p);
                    self.remote_refresh_services_ports(cx);
                }
                _ => {}
            }
        }
        if git_moved {
            let open: Vec<PathBuf> = self.editors.keys().cloned().collect();
            self.remote_load_bases(open, cx);
            self.remote_git_moved(cx);
        }
        cx.notify();
    }

    /// The change markers of remote documents: their text at HEAD from the host (`git.base`).
    pub(super) fn remote_load_bases(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let Some(ask) = self.remote_ask(&path) else {
                continue;
            };
            let task = cx.background_executor().spawn(async move {
                ask.live
                    .host_call("git.base", json!({ "path": ask.doc }), HOST_LIMIT)
                    .ok()
                    .and_then(|v| v["text"].as_str().map(str::to_owned))
            });
            cx.spawn(async move |this, cx| {
                let base = task.await;
                let _ = this.update(cx, |this, cx| {
                    for editor in this.views_of(&path) {
                        editor.update(cx, |e, cx| e.set_base(base.clone(), cx));
                    }
                });
            })
            .detach();
        }
    }
}

/// The pairing question's buttons: Digits Match (the default, Return, answer 0) and Cancel
/// (Escape), which holds the keyboard focus, so a stray Space declines the pairing
/// (`SPEC-0281` v3 behaviour 8).
fn pairing_buttons() -> [gpui::PromptButton; 2] {
    [
        gpui::PromptButton::new("Digits Match"),
        gpui::PromptButton::cancel("Cancel"),
    ]
}

fn r_host(b: &WorkbenchView) -> String {
    b.remote_project
        .as_ref()
        .map(|r| r.live.host())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SPEC-0281` v3 behaviour 8, AK-7: Return confirms the pairing (answer 0), the keyboard focus
    /// and so Space sit on Cancel.
    #[test]
    fn the_pairing_question_focuses_cancel() {
        let buttons = pairing_buttons();
        assert_eq!(buttons[0].label().as_ref(), "Digits Match");
        let focused = gpui::prompt_focused_answer(&buttons).unwrap();
        assert_eq!(buttons[focused].label().as_ref(), "Cancel");
        assert_ne!(focused, 0, "Space must not confirm the pairing");
    }

    #[test]
    fn a_stop_in_a_remote_file_marks_it_now_or_when_it_has_arrived() {
        use ExecutionStep::*;
        // No file is on this computer: whether the editor exists decides, never the local disk.
        assert_eq!(execution_point_step(true, true, false), MarkNow);
        assert_eq!(execution_point_step(true, false, false), OpenThenMark);
        // A step while the document is on its way moves the line and opens nothing again.
        assert_eq!(execution_point_step(true, false, true), MarkOnArrival);
        assert_eq!(execution_point_step(false, true, true), Nothing, "outside");
        assert_eq!(execution_point_step(false, false, false), Nothing);
    }

    #[test]
    fn a_new_stop_forgets_the_waiting_point_of_another_file_only() {
        use super::super::remote_host::PendingPlace::{Execution, GoTo};
        let (a, b, c) = (Path::new("/s/a"), Path::new("/s/b"), Path::new("/s/c"));
        let mut places = HashMap::new();
        places.insert(a.to_path_buf(), Execution(1));
        places.insert(c.to_path_buf(), GoTo(3, None));
        assert!(!forget_other_execution_points(&mut places, b));
        assert!(!places.contains_key(a), "out of date");
        assert!(
            places.contains_key(c),
            "a search result still goes to its place"
        );
        places.insert(a.to_path_buf(), Execution(2));
        assert!(
            forget_other_execution_points(&mut places, a),
            "already on its way"
        );
    }

    #[test]
    fn the_stand_in_folder_is_named_like_the_host_folder_and_keyed_by_host() {
        let base = Path::new("/state/remote");
        let a = stand_in(base, "build", "/srv/app/");
        let b = stand_in(base, "other", "/srv/app");
        assert_eq!(a.file_name().unwrap(), "app");
        assert_ne!(a, b);
        assert!(a.starts_with("/state/remote/projects"));
        assert_eq!(folder_name("/"), "/");
        assert_eq!(recovery_dir(base), Path::new("/state/remote/recovery"));
    }
}
