//! The terminal tool window (`SPEC-0062`): draws a `Session`'s screen with the scheme's console colours
//! and sends keys and typed text to the program; search, prompt marks and shell integration
//! (`SPEC-0206`), split and close keys and the end of the shell (`SPEC-0205`).

use crate::semantics::Annotated;
use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    App, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    Font, KeyDownEvent, Pixels, ShapedLine, TextRun, UTF16Selection, Window, canvas, div, fill,
    font, point, prelude::*, px, rgb, size,
};
use idecraft_terminal::{
    Color, Link, LinkTarget, Match, MouseAction, MouseButton, MouseEncoding, MouseInput, MouseMode,
    Selection, SelectionUnit, Session, SpawnOptions, key_bytes, line_links, mouse_bytes,
};

use crate::EditorView;
use crate::tips::{Tipped, declared};

use crate::palette::{Palette, xterm_color};
use crate::workbench_view::c;

const LINE_HEIGHT: f32 = 16.0;
const PADDING: f32 = 8.0;

/// What the terminal asks the workbench to do (`SPEC-0062` v2 behaviour 7).
pub enum TerminalEvent {
    /// Open another tab (⌘T / Ctrl+Shift+T).
    NewTab,
    /// Close this shell (⌘W / Ctrl+Shift+W, or its program ended; `SPEC-0205` behaviours 4, 5).
    Close,
    /// Start a shell next to this one: to the right (true) or below (`SPEC-0205` behaviour 3).
    Split(bool),
    /// Focus the next (true) or previous shell of the tab.
    FocusSibling(bool),
    /// Open a file at a 1-based line and column from a link (`SPEC-0153` behaviour 3).
    OpenFile(PathBuf, Option<usize>, Option<usize>),
    /// A secondary click outside mouse reporting: open the terminal's context menu at this window
    /// position (`SPEC-0313` behaviour 2).
    ContextMenu(gpui::Point<Pixels>),
}

impl gpui::EventEmitter<TerminalEvent> for TerminalView {}

/// A shell in a pseudo terminal, drawn in the tool window.
pub struct TerminalView {
    session: Option<Session>,
    cwd: PathBuf,
    /// The shell's current folder, asked at most once a second (`SPEC-0161`).
    shell_dir: std::cell::RefCell<Option<(std::time::Instant, PathBuf)>>,
    /// Program and arguments instead of the user's shell (an SSH login, SPEC-0121).
    command: Vec<String>,
    focus: FocusHandle,
    size: (u16, u16),
    seen: u64,
    error: Option<String>,
    /// Lines scrolled back into the history; 0 shows the live screen (SPEC-0118 behaviour 3).
    scroll_offset: usize,
    /// Scrollback length at the last paint, to keep a scrolled view still while output arrives.
    seen_scrollback: usize,
    /// Pixels of wheel movement not yet turned into lines.
    wheel_rest: f32,
    selection: Option<Selection>,
    /// A drag selects (left button down outside mouse reporting).
    selecting: bool,
    /// Button held while the program receives reports.
    reported: Option<MouseButton>,
    /// Origin of the first cell and the cell width, from the last paint.
    layout: Option<(gpui::Point<Pixels>, f32)>,
    /// The link under the mouse: its history line and the link (`SPEC-0153` behaviour 4).
    hover_link: Option<(usize, Link)>,
    /// Saved lines to show in front of the shell when it starts (`SPEC-0205` behaviour 6).
    history: Vec<String>,
    /// Add the shell integration (`SPEC-0206` behaviour 5); off for SSH logins.
    integration: bool,
    /// The program ended and the view asked to be closed.
    ended: bool,
    /// How a program that keeps its output ended (an SSH login, a container's shell; hand test
    /// 170: `exit 3` ended it without a word).
    exit_note: Option<String>,
    /// The find field over the terminal, while open (`SPEC-0206` behaviour 1).
    search: Option<TerminalSearch>,
    /// The last query, offered when the field opens again.
    last_query: String,
    /// Workspace and tab name under which the shell is registered for readers (`SPEC-0313`
    /// behaviour 9); set by the workbench, kept in a cell so that saving the tabs can rename it.
    label: std::cell::RefCell<Option<(PathBuf, String)>>,
    /// The shell's registration, from its start until the view goes.
    registration: std::cell::RefCell<Option<idecraft_terminal::Registration>>,
    /// Shared with the AI chat (`SPEC-0313` behaviour 5).
    shared: bool,
    /// Opens the program behind a link instead of a local pseudo terminal when the view first
    /// gets its size (a host terminal of a remote project, `SPEC-0318` v2 detail 2).
    opener: Option<LinkOpener>,
    /// The link's state for the tab ("Reconnecting"), while it has one.
    link_note: Option<std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    /// Close the tab when the linked program exits, as a local shell's tab closes.
    close_on_exit: bool,
    /// Input typed before the program started (it starts when the view is first drawn): a
    /// command IDECraft types into a new tab, such as a sign-in (`SPEC-0160` behaviour 1) or
    /// Insert into Terminal (`SPEC-0313` behaviour 8), goes to the program once it runs.
    pending: Vec<u8>,
}

/// Input kept for a program that has not started yet, at most.
const PENDING_INPUT: usize = 64 * 1024;

/// Opens a [`idecraft_terminal::PtyLink`] for `rows` × `cols`.
pub type LinkOpener = Box<dyn FnOnce(u16, u16) -> std::sync::Arc<dyn idecraft_terminal::PtyLink>>;

/// The terminal's find field and its matches (`SPEC-0206` behaviour 1).
struct TerminalSearch {
    field: Entity<EditorView>,
    match_case: bool,
    matches: Vec<Match>,
    /// Index of the current match in `matches` (top to bottom).
    current: Option<usize>,
    /// History length and output generation the matches were found for.
    searched: (String, bool, u64),
    /// History base the matches were found at.
    base: u64,
    _watch: gpui::Subscription,
}

/// The shell program and variables for the user's shells set with [`set_local_shell`].
type LocalShell = (Option<String>, Vec<(String, String)>);
static LOCAL_SHELL: std::sync::OnceLock<LocalShell> = std::sync::OnceLock::new();

/// Starts the user's shells from now on with `program` (when Settings › Terminal names none,
/// instead of `$SHELL`) and the variables `env` (after those of the shell integration, so they
/// win). The window self-test gives its shells a fixed start-up folder this way, so its steps do
/// not depend on the person's shell configuration. Only the first call counts.
pub fn set_local_shell(program: Option<String>, env: Vec<(String, String)>) {
    let _ = LOCAL_SHELL.set((program, env));
}

fn editor_font() -> Font {
    let mut f = font(crate::fonts::editor_family());
    f.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        if cfg!(target_os = "macos") {
            "Menlo".into()
        } else {
            "DejaVu Sans Mono".into()
        },
    ]));
    f
}

impl TerminalView {
    /// A terminal that starts the user's shell in `cwd` when first drawn.
    pub fn new(cwd: PathBuf, cx: &mut Context<Self>) -> Self {
        // Redraw when the program wrote something (behaviour 1); output arrives on a reader thread.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    let generation = this.session.as_ref().map_or(0, Session::generation);
                    if generation != this.seen {
                        this.seen = generation;
                        this.refresh_search(cx);
                        cx.notify();
                    }
                    // The user's shell ended: it closes (`SPEC-0205` behaviour 5); SSH logins and
                    // remote runs keep their output.
                    if (this.integration || this.close_on_exit)
                        && !this.ended
                        && let Some(s) = this.session.as_mut()
                        && s.exited().is_some()
                    {
                        this.ended = true;
                        cx.emit(TerminalEvent::Close);
                    }
                    // A host terminal that ended with its session keeps its text with a note.
                    if this.exit_note.is_none()
                        && let Some(note) = this.session.as_ref().and_then(Session::end_note)
                        && this.session.as_mut().and_then(Session::exited).is_none()
                    {
                        this.exit_note = Some(note);
                        cx.notify();
                    }
                    if !this.integration
                        && this.exit_note.is_none()
                        && let Some(code) = this.session.as_mut().and_then(Session::exited)
                    {
                        this.exit_note = Some(exit_note(code));
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            session: None,
            cwd,
            shell_dir: Default::default(),
            // The shell chosen in Settings › Terminal, else `$SHELL` (SPEC-0129 behaviour 6).
            command: crate::appearance::ide_settings(cx)
                .terminal
                .shell
                .or_else(|| LOCAL_SHELL.get().and_then(|(p, _)| p.clone()))
                .map(|s| vec![s])
                .unwrap_or_default(),
            focus: cx.focus_handle(),
            size: (0, 0),
            seen: 0,
            error: None,
            scroll_offset: 0,
            seen_scrollback: 0,
            wheel_rest: 0.0,
            selection: None,
            selecting: false,
            reported: None,
            layout: None,
            hover_link: None,
            history: Vec::new(),
            integration: true,
            ended: false,
            exit_note: None,
            search: None,
            last_query: String::new(),
            label: Default::default(),
            registration: Default::default(),
            shared: false,
            opener: None,
            link_note: None,
            close_on_exit: false,
            pending: Vec::new(),
        }
    }

    /// The program the terminal starts: the shell of Settings › Terminal, else `$SHELL`
    /// (`SPEC-0129` behaviour 6); empty for the system's default.
    pub fn command(&self) -> &[String] {
        &self.command
    }

    /// A terminal whose program runs behind a link `opener` makes when the view first gets its
    /// size (a host terminal); `note` tells the tab the link's state.
    pub fn linked(
        cwd: PathBuf,
        opener: LinkOpener,
        note: std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            command: Vec::new(),
            integration: false,
            opener: Some(opener),
            link_note: Some(note),
            close_on_exit: true,
            ..Self::new(cwd, cx)
        }
    }

    /// The link's state for the tab name ("Reconnecting"), for a linked terminal.
    pub fn link_note(&self) -> Option<String> {
        self.link_note.as_ref().and_then(|n| n())
    }

    /// Whether the program runs behind a link (a host terminal).
    pub fn is_linked(&self) -> bool {
        self.link_note.is_some()
    }

    /// A terminal that starts `command` instead of the shell (an SSH login on a host).
    pub fn with_command(cwd: PathBuf, command: Vec<String>, cx: &mut Context<Self>) -> Self {
        Self {
            command,
            integration: false,
            ..Self::new(cwd, cx)
        }
    }

    /// Shows saved `lines` in front of the shell's first prompt (`SPEC-0205` behaviour 6).
    pub fn with_history(mut self, lines: Vec<String>) -> Self {
        self.history = lines;
        self
    }

    /// Whether this terminal runs the user's shell (saved between runs), not an SSH login.
    pub fn is_local(&self) -> bool {
        self.integration && self.link_note.is_none()
    }

    /// The last up to `max` history lines (`SPEC-0205` behaviour 6).
    pub fn history_lines(&self, max: usize) -> Vec<String> {
        self.session
            .as_ref()
            .map(|s| s.with_terminal(|t| t.screen.history_lines(max)))
            .unwrap_or_else(|| self.history.clone())
    }

    /// Names the shell for readers of the registry: workspace and tab name (`SPEC-0313`
    /// behaviour 9). A running shell is registered at once, else when it starts.
    pub fn set_label(&self, root: &std::path::Path, name: &str) {
        let same = self
            .label
            .borrow()
            .as_ref()
            .is_some_and(|(r, n)| r == root && n == name);
        if same {
            return;
        }
        *self.label.borrow_mut() = Some((root.to_path_buf(), name.to_owned()));
        let renamed = self
            .registration
            .borrow()
            .as_ref()
            .map(|r| r.set_name(name));
        if renamed.is_none() {
            self.register();
        }
    }

    /// The tab name the workbench gave the shell.
    pub fn label(&self) -> Option<String> {
        self.label.borrow().as_ref().map(|(_, n)| n.clone())
    }

    fn register(&self) {
        let (Some(session), Some((root, name))) = (&self.session, self.label.borrow().clone())
        else {
            return;
        };
        let r = idecraft_terminal::TerminalRegistry::global().register(
            &root,
            &name,
            session.terminal_handle(),
        );
        r.set_shared(self.shared);
        *self.registration.borrow_mut() = Some(r);
    }

    /// Whether the tab is shared with the AI chat (`SPEC-0313` behaviour 5).
    pub fn shared(&self) -> bool {
        self.shared
    }

    /// Shares the tab with the AI chat or stops sharing it.
    pub fn set_shared(&mut self, shared: bool, cx: &mut Context<Self>) {
        self.shared = shared;
        if let Some(r) = self.registration.borrow().as_ref() {
            r.set_shared(shared);
        }
        cx.notify();
    }

    /// Whether the program turned bracketed paste on (`SPEC-0313` behaviour 8).
    pub fn bracketed_paste(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.with_terminal(|t| t.screen.bracketed_paste()))
    }

    /// The last command the shell marked, with its output and exit status (`SPEC-0313`
    /// behaviour 2).
    pub fn last_command(&self) -> Option<idecraft_ai::context::CommandText> {
        let c = self
            .session
            .as_ref()?
            .with_terminal(|t| t.screen.last_command())?;
        Some(idecraft_ai::context::CommandText {
            command: c.command,
            output: c.output.join("\n"),
            exit: c.exit,
        })
    }

    /// The attachment of `kind` for the chat: the selection, the last command (the last `lines`
    /// lines without marks) or the last lines, masked and within `budget` (`SPEC-0313`
    /// behaviours 2, 6, 7); `None` for an empty selection or an empty terminal.
    pub fn ai_attachment(
        &self,
        kind: idecraft_ai::context::Source,
        lines: usize,
        budget: usize,
    ) -> Option<idecraft_ai::Attachment> {
        use idecraft_ai::context::{Source, attachment, terminal_attachment};
        let name = self.label().unwrap_or_else(|| "Local".to_owned());
        match kind {
            Source::TerminalSelection => {
                let text = self.selection_text().filter(|t| !t.trim().is_empty())?;
                Some(attachment(
                    Source::TerminalSelection,
                    format!("Terminal \"{name}\" · selection"),
                    "console",
                    &text,
                    budget,
                ))
            }
            Source::TerminalCommand => {
                let command = self.last_command();
                let history = self.history_lines(lines);
                (command.is_some() || !history.is_empty())
                    .then(|| terminal_attachment(&name, command.as_ref(), &history, budget))
            }
            _ => {
                let history = self.history_lines(lines);
                (!history.is_empty()).then(|| terminal_attachment(&name, None, &history, budget))
            }
        }
    }

    /// The prompts the shell marked: stable line, command and exit status (`SPEC-0206`).
    pub fn prompts(&self) -> Vec<idecraft_terminal::Prompt> {
        self.session
            .as_ref()
            .map(|s| s.with_terminal(|t| t.screen.prompts().to_vec()))
            .unwrap_or_default()
    }

    /// Opens the find field with the selected text, else the last query (`SPEC-0206`
    /// behaviour 1).
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self
            .selection_text()
            .filter(|t| !t.contains('\n'))
            .unwrap_or_else(|| self.last_query.clone());
        let field = match &self.search {
            Some(s) => s.field.clone(),
            None => {
                let field = cx.new(EditorView::field);
                let watch = cx.observe(&field, |this, _, cx| {
                    this.refresh_search(cx);
                    this.show_current_match();
                    cx.notify();
                });
                self.search = Some(TerminalSearch {
                    field: field.clone(),
                    match_case: false,
                    matches: Vec::new(),
                    current: None,
                    searched: (String::new(), false, u64::MAX),
                    base: 0,
                    _watch: watch,
                });
                field
            }
        };
        field.update(cx, |f, cx| {
            f.set_text_selected(&query, cx);
        });
        window.focus(&field.read(cx).focus());
        self.refresh_search(cx);
        self.show_current_match();
        cx.notify();
    }

    /// Closes the find field and returns to the live screen and the shell.
    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.search.take() {
            self.last_query = s.field.read(cx).state.buffer.text();
        }
        self.scroll_offset = 0;
        window.focus(&self.focus);
        cx.notify();
    }

    /// Sets the query (for tests and MCP-driven steps).
    pub fn set_search_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if let Some(s) = &self.search {
            s.field.update(cx, |f, cx| f.set_text(query, cx));
        }
        self.refresh_search(cx);
        self.show_current_match();
        cx.notify();
    }

    /// Turns "Cc" (match case) on or off.
    pub fn toggle_search_case(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.search {
            s.match_case = !s.match_case;
        }
        self.refresh_search(cx);
        self.show_current_match();
        cx.notify();
    }

    /// Re-finds the query when it, the case option or the output changed; the current match
    /// stays on the same text where it can, else it is the bottom one.
    fn refresh_search(&mut self, cx: &App) {
        let (Some(search), Some(session)) = (&mut self.search, &self.session) else {
            return;
        };
        let query = search.field.read(cx).state.buffer.text();
        let key = (query, search.match_case, session.generation());
        if key == search.searched {
            return;
        }
        let old = search.current.and_then(|i| search.matches.get(i).copied());
        let (base, matches) = session.with_terminal(|t| {
            (
                t.screen.history_base(),
                idecraft_terminal::search(&t.screen, &key.0, key.1),
            )
        });
        // Lines that left the history move the others up.
        let shift = usize::try_from(base.saturating_sub(search.base)).unwrap_or(usize::MAX);
        search.matches = matches;
        search.base = base;
        search.current = old
            .and_then(|m| {
                let line = m.line.checked_sub(shift)?;
                search
                    .matches
                    .iter()
                    .position(|n| n.line == line && n.start == m.start)
            })
            .or(search.matches.len().checked_sub(1));
        search.searched = key;
    }

    /// The find field's counter: "n/m" or "0 results".
    pub fn search_label(&self) -> Option<String> {
        let s = self.search.as_ref()?;
        Some(match s.current {
            Some(i) if !s.matches.is_empty() => format!("{}/{}", i + 1, s.matches.len()),
            _ => "0 results".to_owned(),
        })
    }

    /// The current match (for tests).
    pub fn current_match(&self) -> Option<Match> {
        let s = self.search.as_ref()?;
        s.matches.get(s.current?).copied()
    }

    /// Goes to the next match upwards (`forward`) or downwards, wrapping around.
    pub fn search_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.refresh_search(cx);
        if let Some(s) = &mut self.search {
            let n = s.matches.len();
            if n > 0 {
                let i = s.current.unwrap_or(0);
                s.current = Some(if forward {
                    (i + n - 1) % n
                } else {
                    (i + 1) % n
                });
            }
        }
        self.show_current_match();
        cx.notify();
    }

    /// Scrolls so that the current match is shown.
    fn show_current_match(&mut self) {
        if let Some(m) = self.current_match() {
            self.show_line(m.line, true);
        }
    }

    /// Scrolls so that history line `line` is shown: in the middle when `center`, else at the top.
    fn show_line(&mut self, line: usize, center: bool) {
        let Some(session) = &self.session else {
            return;
        };
        let (history, rows) =
            session.with_terminal(|t| (t.screen.scrollback_len(), t.screen.size().0));
        let first = history.saturating_sub(self.scroll_offset);
        if center && line >= first && line < first + rows {
            return;
        }
        let top = if center {
            line.saturating_sub(rows / 2)
        } else {
            line
        };
        self.scroll_offset = history.saturating_sub(top.min(history));
    }

    /// Scrolls to the previous (`up`) or next prompt start (`SPEC-0206` behaviour 4).
    pub fn jump_prompt(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let (base, lines) = session.with_terminal(|t| {
            (
                t.screen.history_base(),
                t.screen
                    .prompts()
                    .iter()
                    .map(|p| p.line)
                    .collect::<Vec<_>>(),
            )
        });
        let first = self.first_line();
        let target = lines
            .into_iter()
            .filter_map(|l| usize::try_from(l - base).ok());
        let target = if up {
            target.filter(|&l| l < first).max()
        } else {
            target.filter(|&l| l > first).min()
        };
        let scrollback = session.with_terminal(|t| t.screen.scrollback_len());
        if let Some(line) = target {
            self.show_line(line, false);
            self.seen_scrollback = scrollback;
        } else if !up {
            self.scroll_offset = 0;
        }
        cx.notify();
    }

    /// Mouse mode and encoding the program asked for, and whether the alternate screen is shown.
    fn modes(&self) -> (MouseMode, MouseEncoding, bool) {
        self.session
            .as_ref()
            .map_or((MouseMode::Off, MouseEncoding::Default, false), |s| {
                s.with_terminal(|t| {
                    (
                        t.screen.mouse_mode(),
                        t.screen.mouse_encoding(),
                        t.screen.alternate(),
                    )
                })
            })
    }

    /// Mouse mode the program asked for (for tests).
    pub fn mouse_mode(&self) -> MouseMode {
        self.modes().0
    }

    /// Lines scrolled back into the history.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// First history line shown (scrollback lines come first).
    fn first_line(&self) -> usize {
        self.session
            .as_ref()
            .map_or(0, |s| s.with_terminal(|t| t.screen.scrollback_len()))
            .saturating_sub(self.scroll_offset)
    }

    /// The rows shown as text, top to bottom, including scrolled-back history (for tests).
    pub fn shown_text(&self) -> String {
        let first = self.first_line();
        self.lines_text(first, None)
    }

    /// The whole history and screen as text, independent of how many rows the window shows (for
    /// tests).
    pub fn history_text(&self) -> String {
        self.lines_text(0, Some(usize::MAX))
    }

    /// Rows from `first` on: `count` of them, or a screen's worth.
    fn lines_text(&self, first: usize, count: Option<usize>) -> String {
        self.session
            .as_ref()
            .map(|s| {
                s.with_terminal(|t| {
                    let rows = count.unwrap_or(t.screen.size().0);
                    (first..first.saturating_add(rows))
                        .map_while(|i| t.screen.line(i))
                        .map(|l| {
                            l.iter()
                                .filter(|c| c.width > 0)
                                .map(|c| c.ch)
                                .collect::<String>()
                                .trim_end()
                                .to_owned()
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
            .unwrap_or_default()
    }

    /// The selected text, if anything is selected.
    pub fn selection_text(&self) -> Option<String> {
        let sel = self.selection.filter(|s| !s.is_empty())?;
        self.session
            .as_ref()
            .map(|s| s.with_terminal(|t| sel.text(&t.screen)))
    }

    /// A button went down on screen cell (`row`, `col`) (behaviours 1, 4): reported to the program
    /// when it asked for the mouse and Shift is not held, else it starts a selection (two clicks a
    /// word, three a line).
    pub fn press(
        &mut self,
        row: usize,
        col: usize,
        button: MouseButton,
        clicks: usize,
        mods: (bool, bool, bool),
        cx: &mut Context<Self>,
    ) {
        let (mode, encoding, _) = self.modes();
        if mode != MouseMode::Off && !mods.0 && self.scroll_offset == 0 {
            let input = mouse_input(MouseAction::Press, button, row, col, mods);
            if let Some(bytes) = mouse_bytes(&input, mode, encoding) {
                self.write(&bytes);
            }
            self.reported = Some(button);
            self.selection = None;
            cx.notify();
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        let unit = match clicks {
            0 | 1 => SelectionUnit::Char,
            2 => SelectionUnit::Word,
            _ => SelectionUnit::Line,
        };
        self.selection = Some(Selection::new((self.first_line() + row, col), unit));
        self.selecting = true;
        cx.notify();
    }

    /// The mouse moved to (`row`, `col`) with `held` down (behaviours 1, 4).
    pub fn drag(
        &mut self,
        row: usize,
        col: usize,
        held: Option<MouseButton>,
        mods: (bool, bool, bool),
        cx: &mut Context<Self>,
    ) {
        if self.selecting {
            let line = self.first_line() + row;
            if let Some(sel) = &mut self.selection {
                sel.head = (line, col);
            }
            cx.notify();
            return;
        }
        let (mode, encoding, _) = self.modes();
        if matches!(mode, MouseMode::Drag | MouseMode::Motion) && self.scroll_offset == 0 {
            let button = held.or(self.reported).unwrap_or(MouseButton::None);
            let input = mouse_input(MouseAction::Move, button, row, col, mods);
            if let Some(bytes) = mouse_bytes(&input, mode, encoding) {
                self.write(&bytes);
            }
        }
    }

    /// A button went up on (`row`, `col`).
    pub fn release(
        &mut self,
        row: usize,
        col: usize,
        button: MouseButton,
        mods: (bool, bool, bool),
        cx: &mut Context<Self>,
    ) {
        if self.selecting {
            self.selecting = false;
            if self.selection.is_some_and(|s| s.is_empty()) {
                self.selection = None;
            }
            cx.notify();
            return;
        }
        if self.reported.take().is_some() {
            let (mode, encoding, _) = self.modes();
            let input = mouse_input(MouseAction::Release, button, row, col, mods);
            if let Some(bytes) = mouse_bytes(&input, mode, encoding) {
                self.write(&bytes);
            }
        }
    }

    /// The wheel turned by `lines` (positive: away from the user) over (`row`, `col`)
    /// (behaviour 2): reported to a program that asked for the mouse, arrow keys in a full-screen
    /// program, else the view scrolls through the history.
    pub fn wheel(
        &mut self,
        lines: i32,
        row: usize,
        col: usize,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        if lines == 0 {
            return;
        }
        let (mode, encoding, alternate) = self.modes();
        let up = lines > 0;
        let count = lines.unsigned_abs().min(10) as usize;
        if mode != MouseMode::Off && !shift && self.scroll_offset == 0 {
            let button = if up {
                MouseButton::WheelUp
            } else {
                MouseButton::WheelDown
            };
            let input = mouse_input(MouseAction::Press, button, row, col, (false, false, false));
            if let Some(bytes) = mouse_bytes(&input, mode, encoding) {
                for _ in 0..count {
                    self.write(&bytes);
                }
            }
            return;
        }
        if alternate {
            let app_cursor = self
                .session
                .as_ref()
                .is_some_and(|s| s.with_terminal(|t| t.screen.app_cursor()));
            let key = if up { "up" } else { "down" };
            if let Some(bytes) = key_bytes(key, false, false, false, app_cursor) {
                for _ in 0..count {
                    self.write(&bytes);
                }
            }
            return;
        }
        let history = self
            .session
            .as_ref()
            .map_or(0, |s| s.with_terminal(|t| t.screen.scrollback_len()));
        self.scroll_offset = if up {
            (self.scroll_offset + count).min(history)
        } else {
            self.scroll_offset.saturating_sub(count)
        };
        cx.notify();
    }

    /// The link on screen cell (`row`, `col`) whose target can be opened: any web address, a file
    /// that exists (relative to the terminal's folder, absolute or under `~`) (`SPEC-0153`
    /// behaviours 1–3).
    pub fn link_at(&self, row: usize, col: usize) -> Option<(usize, Link)> {
        let line = self.first_line() + row;
        let link = self.session.as_ref()?.with_terminal(|t| {
            line_links(&t.screen, t.screen.line(line)?)
                .into_iter()
                .find(|l| l.start <= col && col < l.end)
        })?;
        match &link.target {
            LinkTarget::Url { .. } => Some((line, link)),
            LinkTarget::File { path, .. } => self
                .resolve_file(path)
                .filter(|p| p.is_file())
                .map(|_| (line, link)),
        }
    }

    fn resolve_file(&self, path: &str) -> Option<PathBuf> {
        if let Some(rest) = path.strip_prefix("~/") {
            return std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest));
        }
        let p = PathBuf::from(path);
        if p.is_absolute() {
            return Some(p);
        }
        // Relative to the shell's current folder, then to the folder it started in (`SPEC-0161`).
        let here = self.current_dir();
        let in_shell = here.join(&p);
        Some(if in_shell.exists() {
            in_shell
        } else {
            self.cwd.join(p)
        })
    }

    /// The shell's current folder: what it reported with OSC 7 or its process's folder, else the
    /// folder it started in (`SPEC-0161` behaviour 1).
    pub fn current_dir(&self) -> PathBuf {
        if let Some((at, d)) = self.shell_dir.borrow().as_ref()
            && at.elapsed() < std::time::Duration::from_secs(1)
        {
            return d.clone();
        }
        let d = self
            .session
            .as_ref()
            .and_then(Session::current_dir)
            .unwrap_or_else(|| self.cwd.clone());
        *self.shell_dir.borrow_mut() = Some((std::time::Instant::now(), d.clone()));
        d
    }

    /// Opens the link on (`row`, `col`): a web address in the browser, a file in the editor at its
    /// line. Returns whether there was one (`SPEC-0153` behaviour 3).
    pub fn open_link(&mut self, row: usize, col: usize, cx: &mut Context<Self>) -> bool {
        let Some((_, link)) = self.link_at(row, col) else {
            return false;
        };
        match link.target {
            LinkTarget::Url { url } => cx.open_url(&url),
            LinkTarget::File { path, line, column } => {
                if let Some(p) = self.resolve_file(&path) {
                    cx.emit(TerminalEvent::OpenFile(p, line, column));
                }
            }
        }
        true
    }

    /// The link under the mouse, underlined (for tests).
    pub fn hovered_link(&self) -> Option<&Link> {
        self.hover_link.as_ref().map(|(_, l)| l)
    }

    /// The mouse is over (`row`, `col`) with no button down: underline the link there.
    pub fn hover(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        let found = self.link_at(row, col);
        if found != self.hover_link {
            self.hover_link = found;
            cx.notify();
        }
    }

    /// Screen cell under a window position, clamped to the screen.
    fn cell_at(&self, position: gpui::Point<Pixels>) -> Option<(usize, usize)> {
        let (origin, cell_width) = self.layout?;
        let (rows, cols) = (self.size.0.max(1) as usize, self.size.1.max(1) as usize);
        let x = (f32::from(position.x - origin.x) / cell_width)
            .floor()
            .max(0.0) as usize;
        let y = (f32::from(position.y - origin.y) / LINE_HEIGHT)
            .floor()
            .max(0.0) as usize;
        Some((y.min(rows - 1), x.min(cols - 1)))
    }

    /// Focus handle of the terminal.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// How the program ended, for terminals that keep their output.
    pub fn exit_note(&self) -> Option<&str> {
        self.exit_note.as_deref()
    }

    /// Visible text, for tests.
    pub fn text(&self) -> String {
        self.session.as_ref().map(Session::text).unwrap_or_default()
    }

    /// Sends `text` to the program as if typed (for tests).
    pub fn send_text(&mut self, text: &str) {
        self.type_bytes(text.as_bytes());
    }

    /// Sends `text` as pasted: line ends as Return, wrapped in bracketed-paste marks when the
    /// program turned that mode on.
    pub fn paste(&mut self, text: &str) {
        let bracketed = self
            .session
            .as_ref()
            .is_some_and(|s| s.with_terminal(|t| t.screen.bracketed_paste()));
        self.type_bytes(&idecraft_terminal::paste_bytes(text, bracketed));
    }

    /// Sends typed input: the view returns to the live screen and the selection goes.
    fn type_bytes(&mut self, bytes: &[u8]) {
        self.scroll_offset = 0;
        self.selection = None;
        self.write(bytes);
    }

    fn write(&mut self, bytes: &[u8]) {
        match &self.session {
            Some(s) => {
                if let Err(e) = s.write(bytes) {
                    self.error = Some(e.to_string());
                }
            }
            None if !self.ended && self.pending.len() + bytes.len() <= PENDING_INPUT => {
                self.pending.extend_from_slice(bytes);
            }
            None => {}
        }
    }

    /// Sends the input typed before the program started.
    fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            let bytes = std::mem::take(&mut self.pending);
            self.write(&bytes);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        // Find, prompts, splits (SPEC-0206 behaviours 1 and 4, SPEC-0205 behaviour 3): ⌘ on
        // macOS, Ctrl+Shift (Ctrl+Alt to move between shells) elsewhere.
        let mac = cfg!(target_os = "macos");
        let cmd = if mac {
            m.platform && !m.control
        } else {
            m.control && m.shift
        };
        let handled = match key {
            "f" if cmd && !m.alt && (!mac || !m.shift) => {
                self.open_search(window, cx);
                true
            }
            "g" if mac && cmd && !m.alt && self.search.is_some() => {
                self.search_step(!m.shift, cx);
                true
            }
            "up" | "down" if cmd && !m.alt && (!mac || !m.shift) => {
                self.jump_prompt(key == "up", cx);
                true
            }
            "d" if cmd => {
                // ⌘D right, ⇧⌘D below; Ctrl+Shift+D and Ctrl+Shift+Alt+D elsewhere.
                let below = if mac { m.shift } else { m.alt };
                cx.emit(TerminalEvent::Split(!below));
                true
            }
            "right" | "left"
                if (mac && m.platform && m.alt && !m.control)
                    || (!mac && m.control && m.alt && !m.shift) =>
            {
                cx.emit(TerminalEvent::FocusSibling(key == "right"));
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            return;
        }
        // New and close tab (behaviour 7): ⌘T/⌘W on macOS, Ctrl+Shift+T/W elsewhere.
        let tab_key = if cfg!(target_os = "macos") {
            m.platform && !m.shift && !m.control && !m.alt
        } else {
            m.control && m.shift && !m.alt
        };
        if tab_key && matches!(event.keystroke.key.as_str(), "t" | "w") {
            cx.emit(if event.keystroke.key == "t" {
                TerminalEvent::NewTab
            } else {
                TerminalEvent::Close
            });
            cx.stop_propagation();
            return;
        }
        // Copy the selection (SPEC-0118 behaviour 4): ⌘C on macOS, Ctrl+Shift+C elsewhere.
        let copy_key = if cfg!(target_os = "macos") {
            m.platform && !m.shift && !m.control
        } else {
            m.control && m.shift
        };
        if copy_key
            && event.keystroke.key == "c"
            && let Some(text) = self.selection_text()
        {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            cx.stop_propagation();
            return;
        }
        // Paste (SPEC-0118 behaviour 4): ⌘V on macOS, Ctrl+Shift+V elsewhere; bracketed when the
        // program asked for it (D1: ⌘V pasted nothing).
        if copy_key && event.keystroke.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.paste(&text);
            }
            cx.stop_propagation();
            return;
        }
        // Command shortcuts belong to the IDE (behaviour 3).
        if m.platform {
            return;
        }
        let app_cursor = self
            .session
            .as_ref()
            .is_some_and(|s| s.with_terminal(|t| t.screen.app_cursor()));
        if let Some(bytes) = key_bytes(&event.keystroke.key, m.control, m.alt, m.shift, app_cursor)
        {
            self.type_bytes(&bytes);
            cx.stop_propagation();
        }
    }

    /// Starts the shell or resizes it to the space it has (behaviour 4).
    fn fit(&mut self, rows: u16, cols: u16) {
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        match &self.session {
            Some(s) => {
                let _ = s.resize(rows, cols);
            }
            None if self.opener.is_some() => {
                if let Some(open) = self.opener.take() {
                    let link = open(rows, cols);
                    let history = std::mem::take(&mut self.history);
                    self.session = Some(Session::over_link(link, rows, cols, &history));
                    self.register();
                    self.flush_pending();
                }
            }
            None => {
                let options = SpawnOptions {
                    env: if self.integration {
                        LOCAL_SHELL
                            .get()
                            .map(|(_, e)| e.clone())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    },
                    history: std::mem::take(&mut self.history),
                    integration: self
                        .integration
                        .then(idecraft_terminal::integration::default_dir),
                };
                match Session::spawn_with(&self.command, &self.cwd, rows, cols, &options) {
                    Ok(s) => {
                        self.session = Some(s);
                        self.register();
                        self.flush_pending();
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> Vec<RowPaint> {
        let p = Palette::current();
        let text_system = window.text_system().clone();
        let base = editor_font();
        let cell_width = text_system
            .shape_line(
                "0".into(),
                px(crate::fonts::EDITOR_FONT_SIZE),
                &[run(1, &base, p.console_text, false)],
                None,
            )
            .width;
        let width = f32::from(bounds.size.width) - 2.0 * PADDING;
        let height = f32::from(bounds.size.height) - 2.0 * PADDING;
        let cols = (width / f32::from(cell_width)).floor().max(2.0) as u16;
        let rows = (height / LINE_HEIGHT).floor().max(1.0) as u16;
        self.fit(rows, cols);
        self.layout = Some((
            point(bounds.origin.x + px(PADDING), bounds.origin.y + px(PADDING)),
            f32::from(cell_width),
        ));
        let Some(session) = &self.session else {
            return Vec::new();
        };
        let history = session.with_terminal(|t| {
            if t.screen.alternate() {
                0
            } else {
                t.screen.scrollback_len()
            }
        });
        // A scrolled view stays on the same lines while new output pushes lines into the history.
        if self.scroll_offset > 0 {
            self.scroll_offset += history.saturating_sub(self.seen_scrollback);
        }
        self.scroll_offset = self.scroll_offset.min(history);
        self.seen_scrollback = history;
        let offset = self.scroll_offset;
        let selection = self.selection;
        let hover = self
            .hover_link
            .as_ref()
            .map(|(line, l)| (*line, l.start..l.end));
        // Search matches and the current one (SPEC-0206 behaviour 1).
        let (matches, current) = self.search.as_ref().map_or((Vec::new(), None), |s| {
            (
                s.matches.clone(),
                s.current.and_then(|i| s.matches.get(i).copied()),
            )
        });
        session.with_terminal(|t| {
            let screen = &t.screen;
            let (rows, cols) = screen.size();
            let (cursor_row, cursor_col) = if offset == 0 {
                screen.cursor()
            } else {
                (usize::MAX, usize::MAX)
            };
            let first = screen.scrollback_len() - offset;
            // Prompt marks by history line: green, red, or grey while running (behaviour 3).
            let history_base = screen.history_base();
            let marks: std::collections::HashMap<usize, u32> = screen
                .prompts()
                .iter()
                .filter_map(|pr| {
                    let line = usize::try_from(pr.line - history_base).ok()?;
                    let color = match (pr.command.is_some(), pr.exit) {
                        (_, Some(0)) => p.console_ansi[2],
                        (_, Some(_)) => p.console_ansi[1],
                        (true, None) => p.console_ansi[8],
                        (false, None) => return None,
                    };
                    Some((line, color))
                })
                .collect();
            let mut out = Vec::with_capacity(rows);
            for r in 0..rows {
                let line = screen.line(first + r).unwrap_or(&[]);
                let mut text = String::new();
                let mut runs: Vec<TextRun> = Vec::new();
                let mut backgrounds = Vec::new();
                let mut cursor_byte = None;
                // Byte offset where each column starts, for the selection.
                let mut col_byte = Vec::with_capacity(cols + 1);
                for c in 0..cols {
                    col_byte.push(text.len());
                    let Some(cell) = line.get(c) else { break };
                    if cell.width == 0 {
                        continue;
                    }
                    if r == cursor_row && c == cursor_col {
                        cursor_byte = Some(text.len());
                    }
                    let start = text.len();
                    text.push(cell.ch);
                    text.push_str(&cell.combining);
                    let len = text.len() - start;
                    let st = cell.style;
                    let (mut fg, mut bg) = (
                        resolve(st.fg, p.console_text, p),
                        resolve(st.bg, p.console_background, p),
                    );
                    if st.inverse {
                        std::mem::swap(&mut fg, &mut bg);
                    } else if let Some(readable) = text_colour(st.fg, st.bg, p) {
                        fg = readable;
                    }
                    if bg != p.console_background {
                        backgrounds.push((start..start + len, bg));
                    }
                    // SGR underline, and the link under the mouse (SPEC-0153 behaviour 4).
                    let underlined = st.underline
                        || hover
                            .as_ref()
                            .is_some_and(|(line, cols)| *line == first + r && cols.contains(&c));
                    let underline = underlined.then(|| gpui::UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(rgb(fg).into()),
                        wavy: false,
                    });
                    match runs.last_mut() {
                        Some(last)
                            if last.color == rgb(fg).into()
                                && last.font.weight == weight(st.bold)
                                && last.underline == underline =>
                        {
                            last.len += len;
                        }
                        _ => runs.push(TextRun {
                            underline,
                            ..run(len, &base, fg, st.bold)
                        }),
                    }
                }
                if r == cursor_row && cursor_byte.is_none() {
                    cursor_byte = Some(text.len());
                }
                let selected = selection
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.columns(screen, first + r))
                    .map(|(from, to)| {
                        let at = |c: usize| col_byte.get(c).copied().unwrap_or(text.len());
                        (at(from), at(to.saturating_add(1)))
                    });
                let shaped = text_system.shape_line(
                    text.into(),
                    px(crate::fonts::EDITOR_FONT_SIZE),
                    &runs,
                    None,
                );
                let origin = point(
                    bounds.origin.x + px(PADDING),
                    bounds.origin.y + px(PADDING + r as f32 * LINE_HEIGHT),
                );
                let cursor = cursor_byte
                    .filter(|_| screen.cursor_visible())
                    .map(|b| (shaped.x_for_index(b), cell_width));
                let at = |c: usize| {
                    shaped.x_for_index(col_byte.get(c).copied().unwrap_or(shaped.text.len()))
                };
                let found = matches
                    .iter()
                    .filter(|m| m.line == first + r)
                    .map(|m| (at(m.start), at(m.end), Some(*m) == current))
                    .collect();
                out.push(RowPaint {
                    mark: marks.get(&(first + r)).copied(),
                    found,
                    selection: selected
                        .filter(|(a, b)| b > a)
                        .map(|(a, b)| (shaped.x_for_index(a), shaped.x_for_index(b))),
                    origin,
                    backgrounds: backgrounds
                        .into_iter()
                        .map(|(range, color)| {
                            (
                                shaped.x_for_index(range.start),
                                shaped.x_for_index(range.end),
                                color,
                            )
                        })
                        .collect(),
                    shaped,
                    cursor,
                });
            }
            out
        })
    }
}

fn weight(bold: bool) -> gpui::FontWeight {
    if bold {
        gpui::FontWeight::BOLD
    } else {
        gpui::FontWeight::NORMAL
    }
}

fn run(len: usize, base: &Font, color: u32, bold: bool) -> TextRun {
    TextRun {
        len,
        font: Font {
            weight: weight(bold),
            ..base.clone()
        },
        color: rgb(color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// One of the 16 terminal colours as text on the terminal's own background: the variant that
/// reads there (`SPEC-0060` v10 behaviour 16). Text on a coloured background, the 256-colour
/// cube and direct colours stay as the program sent them.
fn text_colour(fg: Color, bg: Color, p: &Palette) -> Option<u32> {
    match (fg, bg) {
        (Color::Indexed(i @ 0..=15), Color::Default) => Some(p.console_ansi_text[usize::from(i)]),
        _ => None,
    }
}

fn resolve(c: Color, default: u32, p: &Palette) -> u32 {
    match c {
        Color::Default => default,
        Color::Indexed(i) => xterm_color(i, &p.console_ansi),
        Color::Rgb(r, g, b) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
    }
}

/// One terminal row to paint.
struct RowPaint {
    /// Prompt mark colour at the left edge (`SPEC-0206` behaviour 3).
    mark: Option<u32>,
    /// Search matches: from, to, and whether it is the current one.
    found: Vec<(Pixels, Pixels, bool)>,
    origin: gpui::Point<Pixels>,
    shaped: ShapedLine,
    backgrounds: Vec<(Pixels, Pixels, u32)>,
    cursor: Option<(Pixels, Pixels)>,
    selection: Option<(Pixels, Pixels)>,
}

fn mouse_input(
    action: MouseAction,
    button: MouseButton,
    row: usize,
    col: usize,
    (shift, alt, ctrl): (bool, bool, bool),
) -> MouseInput {
    MouseInput {
        action,
        button,
        row,
        col,
        shift,
        alt,
        ctrl,
    }
}

fn button_of(b: gpui::MouseButton) -> MouseButton {
    match b {
        gpui::MouseButton::Right => MouseButton::Right,
        gpui::MouseButton::Middle => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

fn mods(m: &gpui::Modifiers) -> (bool, bool, bool) {
    (m.shift, m.alt, m.control)
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _actual: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Typed and committed text goes to the program as UTF-8 (behaviour 3).
        self.type_bytes(text.as_bytes());
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        _text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Composition is shown by the input method's own window; only the commit is sent.
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The line over a program's kept output once it ended.
fn exit_note(code: u32) -> String {
    format!("Process finished with exit code {code}")
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let input = entity.clone();
        let focus = self.focus.clone();
        let error = self.error.clone();
        let search_bar = self.search_bar(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .semantics(
                crate::semantics::Semantics::new(crate::semantics::Role::Terminal)
                    .name("Terminal")
                    .test_id("terminal-view")
                    .action(crate::semantics::AxAction::Click)
                    .effects(&[crate::semantics::Effect::None]),
            )
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .when(self.hover_link.is_some(), |d| d.cursor_pointer())
            .on_any_mouse_down(cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| {
                window.focus(&this.focus);
                // ⌘-click (Ctrl+click elsewhere) opens a link (SPEC-0153 behaviour 3).
                let open = if cfg!(target_os = "macos") {
                    e.modifiers.platform
                } else {
                    e.modifiers.control
                };
                if open
                    && e.button == gpui::MouseButton::Left
                    && let Some((row, col)) = this.cell_at(e.position)
                    && this.open_link(row, col, cx)
                {
                    return;
                }
                // A secondary click opens the context menu unless the program reports the mouse
                // (Shift still opens it then), `SPEC-0313` behaviour 2.
                let secondary = crate::context_menu::is_secondary_click(
                    match e.button {
                        gpui::MouseButton::Left => crate::context_menu::Button::Primary,
                        gpui::MouseButton::Right => crate::context_menu::Button::Secondary,
                        _ => crate::context_menu::Button::Other,
                    },
                    e.modifiers.control,
                    cfg!(target_os = "macos"),
                );
                if secondary && (this.mouse_mode() == MouseMode::Off || e.modifiers.shift) {
                    cx.emit(TerminalEvent::ContextMenu(e.position));
                    return;
                }
                if let Some((row, col)) = this.cell_at(e.position) {
                    this.press(
                        row,
                        col,
                        button_of(e.button),
                        e.click_count,
                        mods(&e.modifiers),
                        cx,
                    );
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                if e.pressed_button.is_none()
                    && let Some((row, col)) = this.cell_at(e.position)
                {
                    this.hover(row, col, cx);
                }
                if let Some((row, col)) = this.cell_at(e.position) {
                    this.drag(
                        row,
                        col,
                        e.pressed_button.map(button_of),
                        mods(&e.modifiers),
                        cx,
                    );
                }
            }))
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseUpEvent, _, cx| {
                    if let Some((row, col)) = this.cell_at(e.position) {
                        this.release(row, col, button_of(e.button), mods(&e.modifiers), cx);
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseUpEvent, _, cx| {
                    if let Some((row, col)) = this.cell_at(e.position) {
                        this.release(row, col, button_of(e.button), mods(&e.modifiers), cx);
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(|this, e: &gpui::MouseUpEvent, _, cx| {
                    if let Some((row, col)) = this.cell_at(e.position) {
                        this.release(row, col, MouseButton::Right, mods(&e.modifiers), cx);
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Middle,
                cx.listener(|this, e: &gpui::MouseUpEvent, _, cx| {
                    if let Some((row, col)) = this.cell_at(e.position) {
                        this.release(row, col, MouseButton::Middle, mods(&e.modifiers), cx);
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, e: &gpui::ScrollWheelEvent, _, cx| {
                this.wheel_rest += f32::from(e.delta.pixel_delta(px(LINE_HEIGHT)).y);
                let lines = (this.wheel_rest / LINE_HEIGHT).trunc();
                this.wheel_rest -= lines * LINE_HEIGHT;
                let (row, col) = this.cell_at(e.position).unwrap_or((0, 0));
                this.wheel(lines as i32, row, col, e.modifiers.shift, cx);
            }))
            .when_some(error, |d, e| d.child(e))
            .when_some(self.exit_note.clone(), |d, note| {
                let p = Palette::current();
                d.child(div().px_2().text_xs().text_color(c(p.ui_muted)).child(note))
            })
            .children(search_bar)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        entity.update(cx, |this, _| this.prepare(bounds, window))
                    },
                    move |bounds, rows, window, cx| {
                        let p = Palette::current();
                        window.paint_quad(fill(bounds, rgb(p.console_background)));
                        let focused = focus.is_focused(window);
                        let mut selected: gpui::Hsla = rgb(p.caret).into();
                        selected.a = 0.3;
                        for row in rows {
                            for (from, to, color) in &row.backgrounds {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(row.origin.x + *from, row.origin.y),
                                        size(*to - *from, px(LINE_HEIGHT)),
                                    ),
                                    rgb(*color),
                                ));
                            }
                            let hit = p
                                .scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
                                .unwrap_or(p.selection);
                            for (from, to, current) in &row.found {
                                let b = Bounds::new(
                                    point(row.origin.x + *from, row.origin.y),
                                    size(*to - *from, px(LINE_HEIGHT)),
                                );
                                window.paint_quad(fill(b, rgb(hit)));
                                if *current {
                                    window.paint_quad(gpui::outline(
                                        b,
                                        rgb(p.caret),
                                        gpui::BorderStyle::Solid,
                                    ));
                                }
                            }
                            if let Some(color) = row.mark {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(bounds.origin.x + px(2.), row.origin.y + px(1.)),
                                        size(px(3.), px(LINE_HEIGHT - 2.)),
                                    ),
                                    rgb(color),
                                ));
                            }
                            if let Some((from, to)) = row.selection {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(row.origin.x + from, row.origin.y),
                                        size(to - from, px(LINE_HEIGHT)),
                                    ),
                                    selected,
                                ));
                            }
                            let _ = row.shaped.paint(row.origin, px(LINE_HEIGHT), window, cx);
                            if let Some((x, w)) = row.cursor {
                                let b = Bounds::new(
                                    point(row.origin.x + x, row.origin.y),
                                    size(if focused { w } else { px(1.5) }, px(LINE_HEIGHT)),
                                );
                                let mut c: gpui::Hsla = rgb(p.caret).into();
                                c.a = if focused { 0.6 } else { 0.8 };
                                window.paint_quad(fill(b, c));
                            }
                        }
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, input.clone()),
                            cx,
                        );
                    },
                )
                .flex_1()
                .min_h_0()
                .w_full(),
            )
    }
}

impl TerminalView {
    /// The find field above the terminal: query, "Cc", counter, ↑/↓ and × (`SPEC-0206`
    /// behaviour 1).
    fn search_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let search = self.search.as_ref()?;
        let p = Palette::current();
        let label = self.search_label().unwrap_or_default();
        let nav = |control: &'static idecraft_ui::tooltip::Control,
                   text: &'static str,
                   forward: bool,
                   cx: &mut Context<Self>| {
            div()
                .id(control.name)
                .tip(control)
                .px_1()
                .rounded(px(4.))
                .hover(|s| s.bg(crate::workbench_view::c(p.toolbar_hover)))
                .child(text)
                .on_click(cx.listener(move |this, _, _, cx| this.search_step(forward, cx)))
        };
        Some(
            div()
                .id("terminal-search")
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .bg(crate::workbench_view::c(p.island))
                .border_b_1()
                .border_color(crate::workbench_view::c(p.separator))
                .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    let k = &e.keystroke;
                    let plain = !k.modifiers.control && !k.modifiers.alt && !k.modifiers.platform;
                    match k.key.as_str() {
                        "enter" if plain => this.search_step(!k.modifiers.shift, cx),
                        "up" if plain => this.search_step(true, cx),
                        "down" if plain => this.search_step(false, cx),
                        "escape" if plain => this.close_search(window, cx),
                        "g" if k.modifiers.platform => this.search_step(!k.modifiers.shift, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .child(div().w(px(220.)).child(search.field.clone()))
                .child(crate::find_bar::toggle_button(
                    crate::tips::declared!(TERMINAL_TOOLBAR, "terminal-search-case"),
                    "Cc",
                    search.match_case,
                    cx,
                    |this: &mut Self, cx| this.toggle_search_case(cx),
                ))
                .child(
                    div()
                        .min_w(px(70.))
                        .text_color(crate::workbench_view::c(p.ui_muted))
                        .child(label),
                )
                .child(nav(
                    declared!(TERMINAL_TOOLBAR, "terminal-search-prev"),
                    "↑",
                    true,
                    cx,
                ))
                .child(nav(
                    declared!(TERMINAL_TOOLBAR, "terminal-search-next"),
                    "↓",
                    false,
                    cx,
                ))
                .child(div().flex_1())
                .child(
                    div()
                        .id("terminal-search-close")
                        .tip(declared!(TERMINAL_TOOLBAR, "terminal-search-close"))
                        .size(px(16.))
                        .child(crate::icon_images::icon("general/closeSmall"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx))),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    // Hand test 170: a container's shell that ends says how.
    #[test]
    fn an_ended_program_says_its_exit_code() {
        assert_eq!(super::exit_note(3), "Process finished with exit code 3");
    }
}
