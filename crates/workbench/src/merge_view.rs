//! The merge view (`SPEC-0101`): the files of a stopped merge, rebase or cherry-pick with their
//! conflicts in three columns — ours, the result and theirs. The result column is an editable text
//! (v7): the sides are taken or ignored in the gutters between the columns and what they insert
//! can be edited; Resolve Simple merges simple conflicts word by word, F7 / ⇧F7 step through the
//! changes, and Apply writes exactly the result text and marks the file resolved; after the last
//! file the waiting operation is finished (v3). Nothing is written before Apply (v7 behaviour 21),
//! and Close asks before choices or edits are dropped (behaviour 25). A binary file and a file one
//! side deleted are one conflict that takes a whole side and is never written from text (v6
//! behaviour 12). Each file has its own [`MergeSession`].

use std::cell::Cell;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ferris_idea_editor::Motion;
use ferris_idea_vcs::{ChunkKind, Conflict, Operation, Outcome, Repo, Resolution};
use gpui::{
    ClipboardItem, Context, EventEmitter, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, Window, div, font, point, prelude::*, px,
};

use crate::merge_session::{MergeSession, ResultLine, counter, side_words};
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// Height of one text line.
const LINE: f32 = 20.;
/// Width of a gutter between two columns.
const GUTTER: f32 = 44.;
/// Left padding of the text in a column.
const PAD: f32 = 8.;
/// Columns a tab takes, as the side texts are drawn.
const TAB: usize = 4;

/// What the view asks the workbench to do.
pub enum MergeEvent {
    /// Close the view.
    Close,
    /// `path` was resolved, with this text or (`None`) with a whole side, which may have removed
    /// the file; an open editor takes it over.
    Resolved(PathBuf, Option<String>),
    /// The last file was resolved: close and finish the waiting operation (v3 behaviour 8).
    Finished,
}

/// A question the view shows before it goes on (v7 behaviours 20 and 25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeAsk {
    /// Close although choices or edits are not applied.
    Close,
    /// Apply although the result has conflict markers.
    Markers,
}

/// The merge view.
pub struct MergeView {
    root: PathBuf,
    /// One session per conflicted file, in order (v7 behaviour 19).
    sessions: Vec<MergeSession>,
    file: usize,
    error: Option<String>,
    /// What the last Resolve Simple or step did (v3 behaviour 6, v7 behaviour 24).
    note: Option<String>,
    ask: Option<MergeAsk>,
    focus: FocusHandle,
    /// Headers of the left and right column.
    sides: (String, String),
    scroll: gpui::ScrollHandle,
    /// The row of file tabs, which rolls sideways when the tabs do not fit (v8 behaviour 28).
    tabs_scroll: gpui::ScrollHandle,
    /// Left edge of the result column in the window, from the last frame (for clicks).
    result_x: Rc<Cell<f32>>,
    /// The mouse button went down in the result column and is still held.
    dragging: bool,
    /// A merge of plain texts outside Git (a file changed on an SSH host, `SPEC-0121` v7
    /// behaviour 4a): Apply hands the result to this file's editor through
    /// [`MergeEvent::Resolved`] and writes nothing itself.
    target: Option<PathBuf>,
}

impl EventEmitter<MergeEvent> for MergeView {}

/// The column a char column of `text` is drawn in (a tab takes [`TAB`] columns).
fn drawn_column(text: &str, column: usize) -> usize {
    text.chars()
        .take(column)
        .map(|ch| if ch == '\t' { TAB } else { 1 })
        .sum()
}

/// The char column of `text` nearest to the drawn column `drawn`.
fn char_column(text: &str, drawn: usize) -> usize {
    let mut at = 0;
    for (i, ch) in text.chars().enumerate() {
        let width = if ch == '\t' { TAB } else { 1 };
        if at + width.div_ceil(2) > drawn {
            return i;
        }
        at += width;
    }
    text.chars().count()
}

/// A header text that gives way: its own width while there is room, then cut with "…" on one
/// line (v8 behaviour 28).
fn one_line(d: gpui::Div) -> gpui::Div {
    d.flex_initial()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
}

/// Rows each region takes: its longest column, and one row for a change without lines.
fn heights(session: &MergeSession, lines: &[Vec<ResultLine>]) -> Vec<usize> {
    session
        .chunks()
        .iter()
        .zip(lines)
        .map(|(chunk, result)| {
            let shown = if session.whole_side() {
                chunk
                    .result()
                    .map_or(0, |t| t.split_inclusive('\n').count())
            } else {
                result.len()
            };
            chunk
                .ours
                .len()
                .max(chunk.theirs.len())
                .max(shown)
                .max(usize::from(chunk.kind != ChunkKind::Stable))
        })
        .collect()
}

impl MergeView {
    /// A view on `conflicts` (not empty) of the repository at `root`, on the first file. In a
    /// stopped rebase the replayed branch is "Yours" on the left and the target "Theirs", although
    /// Git's HEAD is the target then (v2 behaviour 2, OBS-0009).
    pub fn new(root: &Path, conflicts: Vec<Conflict>, cx: &mut Context<Self>) -> Self {
        let repo = Repo::discover(root).ok();
        let rebasing = repo
            .as_ref()
            .is_some_and(|r| r.operation_in_progress() == Some(Operation::Rebase));
        let sides = if rebasing {
            let branch = repo.as_ref().and_then(|r| r.rebasing_branch());
            (
                branch.map_or("Yours (rebased)".into(), |b| format!("Yours ({b})")),
                "Theirs (rebase target)".into(),
            )
        } else {
            ("Yours (HEAD)".into(), "Theirs".into())
        };
        Self {
            root: root.to_path_buf(),
            sessions: conflicts
                .iter()
                .map(|c| MergeSession::new(c, rebasing))
                .collect(),
            file: 0,
            error: None,
            note: None,
            ask: None,
            focus: cx.focus_handle(),
            sides,
            scroll: gpui::ScrollHandle::new(),
            tabs_scroll: gpui::ScrollHandle::new(),
            result_x: Rc::new(Cell::new(0.)),
            dragging: false,
            target: None,
        }
    }

    /// A merge of three texts outside Git (`SPEC-0121` v7 behaviour 4a): `base` in the middle,
    /// `ours` on the left under `sides.0`, `theirs` on the right under `sides.1`; `name` is the
    /// file tab. Apply emits [`MergeEvent::Resolved`] with `target` and the result, then
    /// [`MergeEvent::Finished`]; nothing is written by the view.
    pub fn for_texts(
        target: PathBuf,
        name: String,
        texts: [String; 3],
        sides: (String, String),
        cx: &mut Context<Self>,
    ) -> Self {
        let [base, ours, theirs] = texts;
        let conflict = Conflict {
            path: name,
            base: Some(base),
            ours: Some(ours),
            theirs: Some(theirs),
            has_base: true,
            has_ours: true,
            has_theirs: true,
            binary: false,
        };
        Self {
            root: target.parent().map(Path::to_path_buf).unwrap_or_default(),
            sessions: vec![MergeSession::new(&conflict, false)],
            file: 0,
            error: None,
            note: None,
            ask: None,
            focus: cx.focus_handle(),
            sides,
            scroll: gpui::ScrollHandle::new(),
            tabs_scroll: gpui::ScrollHandle::new(),
            result_x: Rc::new(Cell::new(0.)),
            dragging: false,
            target: Some(target),
        }
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Paths of the conflicted files, in order.
    pub fn file_names(&self) -> Vec<String> {
        self.sessions.iter().map(|s| s.path().to_owned()).collect()
    }

    /// Index of the shown file.
    pub fn file(&self) -> usize {
        self.file
    }

    /// The session of the shown file.
    pub fn session(&self) -> Option<&MergeSession> {
        self.sessions.get(self.file)
    }

    /// Shows file `i` (behaviour 1); the other files keep their choices and edits (v7 behaviour
    /// 19).
    pub fn select_file(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.sessions.len() {
            self.file = i;
            self.error = None;
            self.note = None;
            self.ask = None;
            self.scroll.set_offset(point(px(0.), px(0.)));
            self.tabs_scroll.scroll_to_item(i);
            cx.notify();
        }
    }

    /// Conflicts of the shown file still open; a file that takes a whole side is open until a
    /// side is taken (v6 behaviour 12).
    pub fn open_conflicts(&self) -> usize {
        self.session().map_or(0, MergeSession::open_conflicts)
    }

    /// Whether the shown file takes a whole side instead of a merge region by region.
    pub fn whole_side(&self) -> bool {
        self.session().is_some_and(MergeSession::whole_side)
    }

    /// Number of conflicts of the shown file.
    pub fn conflict_count(&self) -> usize {
        self.session().map_or(0, MergeSession::conflict_count)
    }

    /// The question the view shows, if any.
    pub fn asking(&self) -> Option<MergeAsk> {
        self.ask
    }

    /// Runs `change` on the shown file's session and shows the caret.
    fn change<R: Default>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut MergeSession) -> R,
    ) -> R {
        let Some(session) = self.sessions.get_mut(self.file) else {
            return R::default();
        };
        let out = change(session);
        self.error = None;
        self.reveal_caret();
        cx.notify();
        out
    }

    /// Resolves conflict `n` (0-based among the conflicts) of the shown file with `resolution`.
    pub fn resolve(&mut self, n: usize, resolution: Resolution, cx: &mut Context<Self>) {
        self.change(cx, |s| s.resolve(n, resolution));
    }

    /// Takes the left (`true`) or right side of region `index` (v7 behaviour 16).
    pub fn accept(&mut self, index: usize, left: bool, cx: &mut Context<Self>) -> bool {
        self.change(cx, |s| s.accept(index, left))
    }

    /// Ignores the left (`true`) or right side of region `index` (v7 behaviour 16).
    pub fn ignore(&mut self, index: usize, left: bool, cx: &mut Context<Self>) -> bool {
        self.change(cx, |s| s.ignore(index, left))
    }

    /// Takes one side for every open conflict (Accept Left / Accept Right, behaviour 3).
    pub fn accept_all(&mut self, ours: bool, cx: &mut Context<Self>) {
        self.change(cx, |s| s.accept_all(ours));
    }

    /// Resolves the simple conflicts of the shown file word by word (v3 behaviour 6); returns how
    /// many.
    pub fn resolve_simple(&mut self, cx: &mut Context<Self>) -> usize {
        let n = self.change(cx, MergeSession::resolve_simple);
        self.note = Some(match n {
            0 => "No simple conflicts".to_owned(),
            1 => "1 simple conflict resolved".to_owned(),
            n => format!("{n} simple conflicts resolved"),
        });
        n
    }

    /// Puts the caret of the result column on `line` at `column` (a click there).
    pub fn set_caret(&mut self, line: usize, column: usize, extend: bool, cx: &mut Context<Self>) {
        self.change(cx, |s| s.set_caret(line, column, extend));
    }

    /// F7 / ⇧F7: the next or previous change or conflict (v7 behaviour 24).
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let moved = self.change(cx, |s| s.step(forward));
        self.note = (!moved).then(|| {
            if forward {
                "No change after this one".to_owned()
            } else {
                "No change before this one".to_owned()
            }
        });
        moved
    }

    /// The result of the shown file as it stands (v7 behaviour 15).
    pub fn result_text(&self) -> String {
        self.session().map(MergeSession::text).unwrap_or_default()
    }

    /// Close or Escape: asks first when choices or edits of a file are not applied (v7 behaviour
    /// 25), else closes.
    pub fn request_close(&mut self, cx: &mut Context<Self>) {
        if self.sessions.iter().any(MergeSession::is_dirty) {
            self.ask = Some(MergeAsk::Close);
            cx.notify();
        } else {
            cx.emit(MergeEvent::Close);
        }
    }

    /// Answers the shown question: `yes` discards and closes, or applies with the markers; `no`
    /// goes back to resolving.
    pub fn answer(&mut self, yes: bool, cx: &mut Context<Self>) {
        let ask = self.ask.take();
        cx.notify();
        match ask {
            Some(MergeAsk::Close) if yes => cx.emit(MergeEvent::Close),
            Some(MergeAsk::Markers) if yes => {
                self.write(cx);
            }
            _ => {}
        }
    }

    /// Writes the result of the shown file and marks it resolved (behaviour 4); goes on with the
    /// next file, or closes when none is left. Refused while conflicts are open; a result with
    /// conflict markers is asked about first (v7 behaviour 20).
    pub fn apply(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.open_conflicts();
        if open > 0 {
            self.error = Some(format!("{open} conflict(s) still open"));
            cx.notify();
            return false;
        }
        if self.session().is_some_and(MergeSession::has_markers) {
            self.ask = Some(MergeAsk::Markers);
            cx.notify();
            return false;
        }
        self.write(cx)
    }

    fn write(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(session) = self.sessions.get(self.file) else {
            return false;
        };
        let path = session.path().to_owned();
        // The text of the result column, or the side a whole-side file takes: never a text built
        // from a binary or deleted side (v6 behaviour 12).
        let Some(outcome) = session.outcome() else {
            self.error = Some("1 conflict(s) still open".to_owned());
            cx.notify();
            return false;
        };
        let text = match &outcome {
            Outcome::Text(text) => Some(text.clone()),
            Outcome::Side(_) => None,
        };
        if let Some(target) = self.target.clone() {
            // Plain texts: the workbench takes the result over; nothing is written here.
            let Some(text) = text else {
                self.error = Some("Take a side or edit the result first".to_owned());
                cx.notify();
                return false;
            };
            cx.emit(MergeEvent::Resolved(target, Some(text)));
            self.sessions.clear();
            cx.emit(MergeEvent::Finished);
            return true;
        }
        let done = ferris_idea_vcs::Repo::discover(&self.root)
            .and_then(|repo| {
                repo.apply_resolution(&path, &outcome)?;
                Ok(repo.workdir().map(|w| w.join(&path)))
            })
            .map_err(|e| e.to_string());
        match done {
            Ok(full) => {
                if let Some(full) = full {
                    cx.emit(MergeEvent::Resolved(full, text));
                }
                self.sessions.remove(self.file);
                if self.sessions.is_empty() {
                    cx.emit(MergeEvent::Finished);
                } else {
                    let next = self.file.min(self.sessions.len() - 1);
                    self.select_file(next, cx);
                }
                true
            }
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                false
            }
        }
    }

    /// Scrolls so that the caret's line is in view.
    fn reveal_caret(&mut self) {
        let Some(session) = self.session() else {
            return;
        };
        let lines = session.result_lines();
        let caret_line = session.caret_position().line;
        let region = session.caret_region();
        let rows = heights(session, &lines);
        let within = lines
            .get(region)
            .and_then(|l| l.iter().position(|r| r.line == caret_line))
            .unwrap_or(0);
        let top = (rows[..region.min(rows.len())].iter().sum::<usize>() + within) as f32 * LINE;
        let height = f32::from(self.scroll.bounds().size.height);
        if height <= 0. {
            return;
        }
        let offset = self.scroll.offset();
        let shown = -f32::from(offset.y);
        let to = if top < shown {
            (top - 2. * LINE).max(0.)
        } else if top + LINE > shown + height {
            top + 3. * LINE - height
        } else {
            return;
        };
        self.scroll.set_offset(point(offset.x, px(-to)));
    }

    /// Keys of the view: the answers of a shown question; else Escape, F7 / ⇧F7 and the keys of
    /// the result column (v7 behaviours 15, 18, 24, 25).
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        if let Some(ask) = self.ask {
            let plain = !(m.control || m.alt || m.platform);
            match (k.key.as_str(), ask) {
                ("escape" | "enter", _) => self.answer(false, cx),
                ("d", MergeAsk::Close) | ("a", MergeAsk::Markers) if plain => self.answer(true, cx),
                _ => {}
            }
            cx.stop_propagation();
            return;
        }
        let mac = cfg!(target_os = "macos");
        let command = if mac {
            m.platform && !m.control && !m.alt
        } else {
            m.control && !m.alt && !m.platform
        };
        let word = if mac { m.alt } else { m.control };
        let plain = !(m.control || m.platform || (m.alt && !mac));
        let shift = m.shift;
        let moved = |motion: Motion| -> Box<dyn FnOnce(&mut MergeSession)> {
            Box::new(move |s: &mut MergeSession| s.move_caret(motion, shift))
        };
        let act: Box<dyn FnOnce(&mut MergeSession)> = match k.key.as_str() {
            "escape" if plain => {
                self.request_close(cx);
                cx.stop_propagation();
                return;
            }
            "f7" if plain && !m.alt => {
                self.step(!shift, cx);
                cx.stop_propagation();
                return;
            }
            "z" if command && shift => Box::new(|s| {
                s.redo();
            }),
            "z" if command => Box::new(|s| {
                s.undo();
            }),
            "y" if command && !mac => Box::new(|s| {
                s.redo();
            }),
            "a" if command => Box::new(MergeSession::select_all),
            "c" if command => {
                if let Some(text) = self.session().and_then(MergeSession::copy) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                cx.stop_propagation();
                return;
            }
            "x" if command => {
                let cut = self.change(cx, MergeSession::cut);
                if let Some(text) = cut {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                cx.stop_propagation();
                return;
            }
            "v" if command => {
                let text = cx.read_from_clipboard().and_then(|i| i.text());
                match text {
                    Some(text) => Box::new(move |s| {
                        s.paste(&text);
                    }),
                    None => return,
                }
            }
            "left" if mac && m.platform => moved(Motion::LineStart),
            "right" if mac && m.platform => moved(Motion::LineEnd),
            "up" if mac && m.platform => moved(Motion::DocumentStart),
            "down" if mac && m.platform => moved(Motion::DocumentEnd),
            "left" if word => moved(Motion::WordLeft),
            "right" if word => moved(Motion::WordRight),
            "left" if plain => moved(Motion::Left),
            "right" if plain => moved(Motion::Right),
            "up" if plain => moved(Motion::Up),
            "down" if plain => moved(Motion::Down),
            "home" if !mac && m.control => moved(Motion::DocumentStart),
            "end" if !mac && m.control => moved(Motion::DocumentEnd),
            "home" if plain => moved(Motion::LineStart),
            "end" if plain => moved(Motion::LineEnd),
            "backspace" if plain => Box::new(|s| {
                s.backspace();
            }),
            "delete" if plain => Box::new(|s| {
                s.delete();
            }),
            "enter" if plain && !m.alt => Box::new(|s| {
                s.newline();
            }),
            "tab" if plain && !m.alt && !shift => Box::new(|s| {
                s.type_text("\t");
            }),
            // Typed text arrives through the platform's text input, not as the key behind it
            // (`SPEC-0004` v8 behaviour 17b); anything else is the workbench's (its shortcuts go
            // on working).
            _ => return,
        };
        self.note = None;
        self.change(cx, act);
        cx.stop_propagation();
    }

    /// Takes text the platform's text input committed: it is typed into the result column
    /// (`SPEC-0004` v8 behaviour 17b); not while a question is open.
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        if self.ask.is_some() {
            return;
        }
        self.note = None;
        let text = committed.to_owned();
        self.change(cx, move |s| {
            s.type_text(&text);
        });
    }

    /// The line and column of the result column under window position `x` on `line`.
    fn column_at(&self, line: &ResultLine, x: gpui::Pixels) -> usize {
        let advance = crate::fonts::editor_advance().max(1.);
        let drawn = ((f32::from(x) - self.result_x.get() - PAD) / advance)
            .round()
            .max(0.);
        char_column(&line.text, drawn as usize)
    }
}

crate::committed_text::committed_text_input!(MergeView, take_committed);

impl Render for MergeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let inserted = p.diff_inserted;
        let modified = p.diff_modified;
        let conflict = p.diff_conflict;
        let mut mono = font(crate::fonts::editor_family());
        mono.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
            if cfg!(target_os = "macos") {
                "Menlo".into()
            } else {
                "DejaVu Sans Mono".into()
            },
        ]));
        let open = self.open_conflicts();
        let whole = self.whole_side();
        let focused = self.focus.is_focused(window);
        let advance = crate::fonts::editor_advance();
        let button = |id: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .flex_none()
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .when(!on, |d| d.opacity(0.4))
                .when(on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(label)
                // The UI driver finds the button by its id (SPEC-0319 behaviour 44). Apply
                // writes the file and the index; so may the answer to a question.
                .relative()
                .semantics(
                    Semantics::new(Role::Button)
                        .name(label)
                        .test_id(id)
                        .enabled(on)
                        .action(AxAction::Click)
                        .effects(if matches!(id, "merge-apply" | "merge-ask-yes") {
                            &[Effect::Write, Effect::VcsWrite]
                        } else {
                            &[Effect::None]
                        }),
                )
        };
        // A side column: its lines, changed words in a stronger shade (v7 behaviour 23).
        let side =
            |text: &[String], words: &[Vec<Range<usize>>], bg: Option<u32>, height: usize| {
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(height as f32 * LINE))
                    .overflow_hidden()
                    .px(px(PAD))
                    .when_some(bg, |d, bg| d.bg(gpui::rgb(bg)))
                    .children(text.iter().enumerate().map(|(i, l)| {
                        let marked = words.get(i).map_or(&[][..], Vec::as_slice);
                        div()
                            .h(px(LINE))
                            .whitespace_nowrap()
                            .child(crate::diff_view::side_text(
                                l.trim_end_matches(['\n', '\r']),
                                &[],
                                marked,
                                crate::diff_view::stronger(bg.unwrap_or(modified)),
                            ))
                    }))
            };
        let (lines, rows_high, caret, selection) = match self.session() {
            Some(s) => {
                let lines = s.result_lines();
                let high = heights(s, &lines);
                let caret = s.caret();
                let range = caret.range();
                (
                    lines,
                    high,
                    s.caret_position(),
                    (!caret.is_empty()).then(|| (s.position(range.start), s.position(range.end))),
                )
            }
            None => (
                Vec::new(),
                Vec::new(),
                ferris_idea_editor::Position::new(0, 0),
                None,
            ),
        };
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        if let Some(session) = self.session() {
            for (k, chunk) in session.chunks().iter().enumerate() {
                let height = rows_high[k];
                let is_open = session.is_open(k);
                let is_conflict = chunk.kind == ChunkKind::Conflict;
                let (left_bg, right_bg) = match chunk.kind {
                    ChunkKind::Stable => (None, None),
                    ChunkKind::Ours => (Some(modified), None),
                    ChunkKind::Theirs => (None, Some(inserted)),
                    ChunkKind::Both => (Some(modified), Some(modified)),
                    ChunkKind::Conflict => (Some(conflict), Some(conflict)),
                };
                // The result: an open conflict in the conflict colour, an applied change in its
                // side's colour, an ignored one plain.
                let mid_bg = match chunk.kind {
                    ChunkKind::Stable => None,
                    ChunkKind::Conflict if is_open => Some(conflict),
                    ChunkKind::Conflict => Some(modified),
                    ChunkKind::Theirs if !session.can_accept(k, false) => Some(inserted),
                    ChunkKind::Ours | ChunkKind::Both if !session.can_accept(k, true) => {
                        Some(modified)
                    }
                    _ => None,
                };
                let (left_words, right_words) = if whole {
                    (Vec::new(), Vec::new())
                } else {
                    side_words(chunk)
                };
                let gutter = |left: bool, cx: &mut Context<Self>| {
                    let take = session.can_accept(k, left);
                    let skip = session.can_ignore(k, left);
                    let control = |name: &'static str, label: &'static str, says: &str| {
                        div()
                            .id((name, k))
                            .relative()
                            .w(px(18.))
                            .h(px(LINE - 2.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(label)
                            // Changes the result in memory only (v7 behaviour 21).
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name(format!("{says} of region {}", k + 1))
                                    .test_id(format!("{name}-{k}"))
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                    };
                    let accept = take.then(|| {
                        control(
                            if left {
                                "merge-take-left"
                            } else {
                                "merge-take-right"
                            },
                            if left { "»" } else { "«" },
                            if left {
                                "Take the left side"
                            } else {
                                "Take the right side"
                            },
                        )
                        .on_click(cx.listener(
                            move |this, _: &gpui::ClickEvent, window, cx| {
                                window.focus(&this.focus);
                                this.accept(k, left, cx);
                            },
                        ))
                    });
                    let ignore = skip.then(|| {
                        control(
                            if left {
                                "merge-ignore-left"
                            } else {
                                "merge-ignore-right"
                            },
                            "×",
                            if left {
                                "Ignore the left side"
                            } else {
                                "Ignore the right side"
                            },
                        )
                        .on_click(cx.listener(
                            move |this, _: &gpui::ClickEvent, window, cx| {
                                window.focus(&this.focus);
                                this.ignore(k, left, cx);
                            },
                        ))
                    });
                    let bg = if left { left_bg } else { right_bg };
                    let strip = div()
                        .w(px(GUTTER))
                        .flex_none()
                        .h(px(height as f32 * LINE))
                        .flex()
                        .justify_center()
                        .gap(px(2.))
                        .border_l_1()
                        .border_r_1()
                        .border_color(c(p.separator))
                        .text_color(c(p.ui_muted))
                        .when_some(bg.filter(|_| take || skip), |d, bg| d.bg(gpui::rgb(bg)));
                    // The take control stands next to the result, the ignore control outside.
                    if left {
                        strip.children(ignore).children(accept)
                    } else {
                        strip.children(accept).children(ignore)
                    }
                };
                let middle = if whole {
                    // A whole-side file shows the side it takes; it has no text to edit.
                    let shown: Vec<String> = chunk
                        .result()
                        .map(|t| t.split_inclusive('\n').map(str::to_owned).collect())
                        .unwrap_or_default();
                    side(
                        &shown,
                        &[],
                        Some(if is_open { conflict } else { modified }),
                        height,
                    )
                    .into_any_element()
                } else {
                    let result_x = self.result_x.clone();
                    div()
                        .id(("merge-result", k))
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h(px(height as f32 * LINE))
                        .overflow_hidden()
                        .cursor_text()
                        .when_some(mid_bg, |d, bg| d.bg(gpui::rgb(bg)))
                        .child(
                            gpui::canvas(
                                move |bounds, _, _| result_x.set(f32::from(bounds.origin.x)),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .children(lines[k].iter().map(|line| {
                            let chars = line.text.chars().count();
                            let selected: Vec<Range<usize>> = selection
                                .filter(|(from, to)| from.line <= line.line && line.line <= to.line)
                                .map(|(from, to)| {
                                    let start = if from.line == line.line {
                                        from.column
                                    } else {
                                        0
                                    };
                                    let end = if to.line == line.line {
                                        to.column
                                    } else {
                                        chars
                                    };
                                    start..end
                                })
                                .filter(|r| !r.is_empty())
                                .into_iter()
                                .collect();
                            let here = line.clone();
                            let dragged = line.clone();
                            div()
                                .relative()
                                .h(px(LINE))
                                .px(px(PAD))
                                .whitespace_nowrap()
                                .child(crate::diff_view::side_text(
                                    &line.text,
                                    &[],
                                    &selected,
                                    p.selection,
                                ))
                                .when(focused && caret.line == line.line, |d| {
                                    let x = PAD
                                        + drawn_column(&line.text, caret.column) as f32 * advance;
                                    d.child(
                                        div()
                                            .absolute()
                                            .top(px(2.))
                                            .left(px(x))
                                            .w(px(1.5))
                                            .h(px(LINE - 4.))
                                            .bg(gpui::rgb(p.caret)),
                                    )
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                        window.focus(&this.focus);
                                        let column = this.column_at(&here, e.position.x);
                                        this.dragging = true;
                                        this.note = None;
                                        this.set_caret(here.line, column, e.modifiers.shift, cx);
                                    }),
                                )
                                .on_mouse_move(cx.listener(
                                    move |this, e: &MouseMoveEvent, _, cx| {
                                        if this.dragging
                                            && e.pressed_button == Some(MouseButton::Left)
                                        {
                                            let column = this.column_at(&dragged, e.position.x);
                                            this.set_caret(dragged.line, column, true, cx);
                                        }
                                    },
                                ))
                        }))
                        .into_any_element()
                };
                rows.push(
                    div()
                        .id(("merge-chunk", k))
                        .flex()
                        .w_full()
                        .when(chunk.kind != ChunkKind::Stable || is_conflict, |d| {
                            d.border_b_1().border_color(c(p.separator))
                        })
                        .child(side(&chunk.ours, &left_words, left_bg, height))
                        .child(gutter(true, cx))
                        .child(middle)
                        .child(gutter(false, cx))
                        .child(side(&chunk.theirs, &right_words, right_bg, height))
                        .into_any_element(),
                );
            }
        }
        let files = self.file_names();
        let shown = self.file;
        let dirty: Vec<bool> = self.sessions.iter().map(MergeSession::is_dirty).collect();
        let count = self
            .session()
            .map(|s| counter(s.counts()))
            .unwrap_or_default();
        // A file with choices or edits not applied carries a mark.
        let names: Vec<String> = files
            .into_iter()
            .enumerate()
            .map(|(i, f)| {
                if dirty.get(i) == Some(&true) {
                    format!("{f} *")
                } else {
                    f
                }
            })
            .collect();
        // The tab row is never narrower than the shown tab, which it rolls into view (v8
        // behaviour 28): its name as drawn and the tab's padding.
        let shown_tab = names.get(shown).map_or(0., |name| {
            let style = window.text_style();
            let size = style.font_size.to_pixels(window.rem_size());
            let line = window.text_system().shape_line(
                name.clone().into(),
                size,
                &[style.to_run(name.len())],
                None,
            );
            line.width / px(1.) + 2. * 8. + 1.
        });
        let question = self.ask.map(|ask| match ask {
            MergeAsk::Close => {
                let n = dirty.iter().filter(|d| **d).count();
                (
                    if n == 1 {
                        "Choices or edits of 1 file are not applied. Close and discard them?"
                            .to_owned()
                    } else {
                        format!(
                            "Choices or edits of {n} files are not applied. Close and discard them?"
                        )
                    },
                    "Discard and Close (D)",
                )
            }
            MergeAsk::Markers => (
                "The result still has conflict markers (<<<<<<<, =======, >>>>>>>). Apply it as it is?"
                    .to_owned(),
                "Apply Anyway (A)",
            ),
        });
        div()
            .id("merge-view")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v8 behaviour 17b).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, _| this.dragging = false),
            )
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Merge Conflicts"),
                    )
                    // The file tabs give way, so the count and the buttons (Apply, Close) always
                    // stay in the header, also next to an open side window (SPEC-0101 v5
                    // behaviour 10, OBS-0022 L3). They give way as a row that rolls; each tab
                    // keeps the width of its name (v8 behaviour 28).
                    .child(
                        div()
                            .id("merge-files")
                            .flex_initial()
                            .min_w(px(shown_tab))
                            .overflow_x_scroll()
                            .track_scroll(&self.tabs_scroll)
                            .flex()
                            .items_center()
                            .gap_2()
                            .children(names.into_iter().enumerate().map(|(i, name)| {
                                div()
                                    .id(("merge-file", i))
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .when(i == shown, |d| d.bg(c(p.tree_selection)))
                                    .when(i != shown, |d| {
                                        d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
                                    })
                                    .child(name.clone())
                                    .relative()
                                    .semantics(
                                        Semantics::new(Role::Tab)
                                            .name(name)
                                            .test_id(format!("merge-file-{i}"))
                                            .selected(i == shown)
                                            .action(AxAction::Click)
                                            .effects(&[Effect::None]),
                                    )
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.select_file(i, cx)
                                        },
                                    ))
                            })),
                    )
                    // The note and the count share the room the tabs leave, each on one line and
                    // cut with "…" (v8 behaviour 28); what is left stays empty before the buttons.
                    .when_some(self.note.clone(), |d, note| {
                        d.child(
                            one_line(div())
                                .text_color(c(p.ui_muted))
                                .child(note.clone())
                                .relative()
                                .semantics(
                                    Semantics::new(Role::Status)
                                        .name(note)
                                        .test_id("merge-note"),
                                ),
                        )
                    })
                    .child(one_line(div()).text_color(c(p.ui_muted)).child(count))
                    .child(div().flex_1())
                    .child(
                        button("merge-resolve-simple", "Resolve Simple", open > 0 && !whole)
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.resolve_simple(cx);
                            })),
                    )
                    .child(
                        button("merge-accept-left", "Accept Left", open > 0).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, _, cx| this.accept_all(true, cx),
                        )),
                    )
                    .child(
                        button("merge-accept-right", "Accept Right", open > 0).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.accept_all(false, cx)
                            }),
                        ),
                    )
                    .child(
                        button("merge-apply", "Apply", open == 0).on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, _, cx| {
                                this.apply(cx);
                            },
                        )),
                    )
                    .child(button("merge-close", "Close", true).on_click(
                        cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.request_close(cx)),
                    )),
            )
            .when_some(question, |d, (text, yes)| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .border_b_1()
                        .border_color(c(p.separator))
                        .bg(gpui::rgb(conflict))
                        .child(div().flex_1().min_w_0().child(text))
                        .child(button("merge-ask-yes", yes, true).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.answer(true, cx)),
                        ))
                        .child(button("merge-ask-no", "Keep Resolving", true).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.answer(false, cx)),
                        )),
                )
            })
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_color(gpui::rgb(p.console_ansi_text[1]))
                        .child(e),
                )
            })
            .child(
                div()
                    .h(px(24.))
                    .flex_none()
                    .flex()
                    .text_color(c(p.ui_muted))
                    .child(div().flex_1().px_2().child(self.sides.0.clone()))
                    .child(div().w(px(GUTTER)).flex_none())
                    .child(div().flex_1().px_2().child("Result"))
                    .child(div().w(px(GUTTER)).flex_none())
                    .child(div().flex_1().px_2().child(self.sides.1.clone())),
            )
            .child(
                div()
                    .id("merge-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .font(mono)
                    .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                    .children(rows),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPEC-0101 v7 behaviour 15: a click lands on the char whose middle is passed, tabs as drawn.
    #[test]
    fn columns_follow_tabs_as_drawn() {
        assert_eq!(drawn_column("\tab", 0), 0);
        assert_eq!(drawn_column("\tab", 1), 4);
        assert_eq!(drawn_column("\tab", 3), 6);
        assert_eq!(char_column("\tab", 0), 0);
        assert_eq!(char_column("\tab", 1), 0);
        assert_eq!(char_column("\tab", 2), 1);
        assert_eq!(char_column("\tab", 4), 1);
        assert_eq!(char_column("\tab", 5), 2);
        assert_eq!(
            char_column("\tab", 40),
            3,
            "past the end: the end of the line"
        );
        assert_eq!(char_column("", 3), 0);
    }
}
