//! The AI Chat tool window (`SPEC-0138`): a conversation with the endpoint named in Settings ›
//! Tools › AI Assistant. Code from the editor, terminal and run output, failed tests, problems and
//! the Git diff are attached visibly as chips before they are sent; terminal tabs can be shared
//! with the chat, and commands in answers typed into a terminal (`SPEC-0313`). Nothing is sent
//! while the assistant is off.
//!
//! v4 (OBS-0063): answers are drawn as Markdown (`crate::ai_markdown`), code blocks in the
//! editors' colours with Copy, Insert at Caret, Replace Selection and Apply (a diff preview first,
//! one undo step after); the current file is offered as a context chip that goes only when
//! clicked; the question field takes several lines and has Send and Stop.

use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use ferris_idea_ai::context::SharedTab;
use ferris_idea_ai::{Attachment, Message};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Entity, EventEmitter, FocusHandle, FontWeight,
    KeyDownEvent, Stateful, WeakEntity, Window, div, prelude::*, px,
};

use crate::ai_markdown::{self as md, Block, Run};
use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::terminal_view::TerminalView;
use crate::tips::Tipped;
use crate::workbench_view::c;

/// What "+" attaches (`SPEC-0138` v4 behaviour 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachKind {
    /// The whole file of the active editor.
    File,
    /// The active editor's selection.
    Selection,
    /// The Problems tool window's selected problem, else every problem.
    Problems,
    /// The active terminal's last command and its output.
    Terminal,
    /// The working tree's diff against `HEAD`.
    GitDiff,
}

impl AttachKind {
    const ALL: [AttachKind; 5] = [
        AttachKind::File,
        AttachKind::Selection,
        AttachKind::Problems,
        AttachKind::Terminal,
        AttachKind::GitDiff,
    ];

    fn label(self) -> &'static str {
        match self {
            AttachKind::File => "Current File",
            AttachKind::Selection => "Selection",
            AttachKind::Problems => "Problems",
            AttachKind::Terminal => "Terminal (last command)",
            AttachKind::GitDiff => "Git Diff",
        }
    }

    fn id(self) -> &'static str {
        match self {
            AttachKind::File => "file",
            AttachKind::Selection => "selection",
            AttachKind::Problems => "problems",
            AttachKind::Terminal => "terminal",
            AttachKind::GitDiff => "git-diff",
        }
    }
}

/// A change to a file the chat shows before it is made (`SPEC-0138` v4 behaviour 8): the file's
/// text when the preview was made, the char range replaced and the text put there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyPreview {
    /// The file.
    pub path: PathBuf,
    /// The workspace path shown, with ` (selection)` when only the selection is replaced.
    pub label: String,
    /// The file's text when the preview was made; Apply refuses when it changed since.
    pub before: String,
    /// The chars replaced.
    pub range: Range<usize>,
    /// What replaces them.
    pub replacement: String,
}

impl ApplyPreview {
    /// The file's text after the change.
    pub fn after(&self) -> String {
        let chars: Vec<char> = self.before.chars().collect();
        let start = self.range.start.min(chars.len());
        let end = self.range.end.min(chars.len()).max(start);
        let mut out: String = chars[..start].iter().collect();
        out.push_str(&self.replacement);
        out.extend(&chars[end..]);
        out
    }
}

/// What the window asks the workbench to do.
pub enum AiChatEvent {
    /// Hide the window.
    Hide,
    /// Type this command into a terminal without pressing Enter (`SPEC-0313` behaviour 8).
    InsertCommand(String),
    /// Insert the code at the active editor's caret (`SPEC-0138` v4 behaviour 8).
    InsertAtCaret(String),
    /// Replace the active editor's selection with the code.
    ReplaceSelection(String),
    /// Make the preview of applying code block `usize` (its text) to the active file.
    PreviewApply(usize, String),
    /// Make the change the person confirmed in the preview.
    Apply(ApplyPreview),
    /// Attach this to the next question ("+", `SPEC-0138` v4 behaviour 9).
    Attach(AttachKind),
    /// Open this address in the browser; the person confirmed it (behaviour 7).
    OpenLink(String),
    /// Open this workspace file in the editor (`SPEC-0357` behaviour 11).
    OpenFile(PathBuf),
    /// Open Settings › Tools › AI Assistant on the Anthropic API template (`SPEC-0160` v2
    /// behaviour 6).
    UseAnthropicApi,
    /// Open a terminal tab of the project running the user's own Claude Code (`SPEC-0160` v2
    /// behaviour 7).
    OpenClaudeCode,
}

/// A terminal tab shared with the chat (`SPEC-0313` behaviour 5).
struct Shared {
    view: WeakEntity<TerminalView>,
    last: SharedTab,
}

/// The label of the offered context chip, read on every frame.
pub type LabelFn = dyn Fn(&App) -> Option<String>;
/// The offered context as an attachment, read when the question goes.
pub type AttachmentFn = dyn Fn(&App) -> Option<Attachment>;
/// The editors' colours of a code block's lines.
type Colours = Vec<Vec<(Range<usize>, String)>>;

/// What the workbench offers as context (`SPEC-0138` v4 behaviour 9): the label of the active
/// file or selection, and the attachment made from it when the person turned the chip on.
#[derive(Clone)]
pub struct ContextSource {
    /// The chip's label now, `None` without an open file.
    pub label: Rc<LabelFn>,
    /// The attachment, made only when the question is sent with the chip on.
    pub attachment: Rc<AttachmentFn>,
}

/// How a user message is shown: its chips and the question as typed.
#[derive(Clone)]
struct Asked {
    chips: Vec<String>,
    question: String,
}

/// The window.
pub struct AiChatView {
    /// The workspace, under which the chat's state is published for MCP.
    root: PathBuf,
    /// The conversation so far (user and assistant messages).
    messages: Vec<Message>,
    /// The chips and question of each user message by its index, for drawing.
    asked: HashMap<usize, Asked>,
    /// Chips waiting for the next question (`SPEC-0313` behaviour 1).
    attachments: Vec<Attachment>,
    /// Terminal tabs shared with this chat.
    shared: Vec<Shared>,
    /// The terminal the newest terminal chip came from (`SPEC-0313` behaviour 8).
    last_terminal: Option<WeakEntity<TerminalView>>,
    /// Waiting for an answer, since when.
    busy: bool,
    started: Option<Instant>,
    /// Why the last question got no answer, or why a chip or a command was refused.
    error: Option<String>,
    /// A note that is no error ("Copied", "Applied").
    info: Option<String>,
    /// Set to stop the answer that is arriving (`SPEC-0157` behaviour 2).
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    input: Entity<EditorView>,
    /// The conversation's scroll position: it follows the newest message, so the answer and its
    /// Insert into Terminal actions are in view (hand test 214).
    scroll: gpui::ScrollHandle,
    focus: FocusHandle,
    /// The workbench's offer of the current file.
    source: Option<ContextSource>,
    /// The offered chip's label when last drawn; another file resets the chip to suggested.
    suggestion: Option<String>,
    /// The offered chip is switched on: it goes with the next question.
    context_on: bool,
    /// The "+" list is open.
    adding: bool,
    /// A link waiting for the person's confirmation.
    pending_link: Option<String>,
    /// The preview of Apply and the code block it belongs to.
    preview: Option<(usize, ApplyPreview)>,
    /// The code block whose Copy was pressed last.
    copied: Option<usize>,
    /// The sideways scroll of each code block and its overflow when last drawn (C3).
    code_scroll: HashMap<usize, (gpui::ScrollHandle, f32)>,
    /// The editors' colours of code blocks by language and text.
    colours: HashMap<(String, String), Rc<Colours>>,
    /// The tab shown: the chat or the agents (`SPEC-0357` behaviour 1).
    tab: AiTab,
    /// The Agents tab, made on first use.
    agents: Option<Entity<crate::AgentsView>>,
    _agents_events: Option<gpui::Subscription>,
}

/// The tabs of the AI Chat tool window (`SPEC-0357` behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTab {
    /// The conversation with the assistant (`SPEC-0138`).
    Chat,
    /// The hosted agents (`SPEC-0357`).
    Agents,
}

impl EventEmitter<AiChatEvent> for AiChatView {}

/// The most lines the question field grows to before it scrolls.
const FIELD_LINES: usize = 8;

impl AiChatView {
    /// An empty conversation of the workspace at `root`.
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            root,
            messages: Vec::new(),
            asked: HashMap::new(),
            attachments: Vec::new(),
            shared: Vec::new(),
            last_terminal: None,
            busy: false,
            started: None,
            error: None,
            info: None,
            stop: Default::default(),
            input: cx.new(EditorView::field),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
            source: None,
            suggestion: None,
            context_on: false,
            adding: false,
            pending_link: None,
            preview: None,
            copied: None,
            code_scroll: HashMap::new(),
            colours: HashMap::new(),
            tab: AiTab::Chat,
            agents: None,
            _agents_events: None,
        }
    }

    /// The tab shown.
    pub fn tab(&self) -> AiTab {
        self.tab
    }

    /// Shows `tab`; the Agents tab is made on first use (`SPEC-0357` behaviour 1).
    pub fn show_tab(&mut self, tab: AiTab, cx: &mut Context<Self>) {
        self.tab = tab;
        if tab == AiTab::Agents {
            let view = self.ensure_agents(cx);
            view.update(cx, |v, _| v.reload());
        }
        cx.notify();
    }

    /// The Agents tab, made when missing.
    pub fn ensure_agents(&mut self, cx: &mut Context<Self>) -> Entity<crate::AgentsView> {
        if let Some(v) = &self.agents {
            return v.clone();
        }
        let root = self.root.clone();
        let view = cx.new(|cx| crate::AgentsView::new(root, cx));
        self._agents_events = Some(cx.subscribe(
            &view,
            |_, _, event: &crate::agents_view::AgentsEvent, cx| match event {
                crate::agents_view::AgentsEvent::OpenFile(path) => {
                    cx.emit(AiChatEvent::OpenFile(path.clone()))
                }
            },
        ));
        self.agents = Some(view.clone());
        view
    }

    /// The Agents tab, once made.
    pub fn agents_view(&self) -> Option<Entity<crate::AgentsView>> {
        self.agents.clone()
    }

    /// The focus handle around the whole view: the input, the messages and the buttons.
    pub fn scope(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Focus handle of the question field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.input.read(cx).focus()
    }

    /// Sets what the workbench offers as context (`SPEC-0138` v4 behaviour 9).
    pub fn set_context_source(&mut self, source: ContextSource) {
        self.source = Some(source);
    }

    fn budget(cx: &gpui::App) -> usize {
        ferris_idea_ai::context::attachment_chars(
            crate::appearance::ide_settings(cx).ai.attachment_chars,
        )
    }

    /// Publishes the waiting chips and shared tabs for `ai_assistant` (`SPEC-0313` behaviour 9).
    fn publish(&self, cx: &gpui::App) {
        ferris_idea_ai::live::Live::global().set_chat(
            &self.root,
            ferris_idea_ai::live::ChatState {
                attachments: self.attachments.iter().map(Into::into).collect(),
                shared: self.shared_names(cx),
            },
        );
    }

    /// Adds a chip for the next question (`SPEC-0138` behaviour 3, `SPEC-0313` behaviour 1); a
    /// chip beyond the limits is refused with a note.
    pub fn attach(&mut self, attachment: Attachment, cx: &mut Context<Self>) {
        match ferris_idea_ai::context::may_add(&self.attachments, &attachment, Self::budget(cx)) {
            Ok(()) => {
                self.attachments.push(attachment);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.publish(cx);
        cx.notify();
    }

    /// Adds a chip from the terminal `view`, which "Insert into Terminal" then types into.
    pub fn attach_from_terminal(
        &mut self,
        attachment: Attachment,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) {
        self.last_terminal = Some(view.downgrade());
        self.attach(attachment, cx);
    }

    /// Removes chip `i` (its ×).
    pub fn remove_attachment(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.attachments.len() {
            self.attachments.remove(i);
            self.publish(cx);
            cx.notify();
        }
    }

    /// The waiting chips as their texts (tests).
    pub fn chips(&self) -> Vec<String> {
        self.attachments.iter().map(Attachment::chip).collect()
    }

    /// The offered context chip: its label and whether it is on (tests, `SPEC-0138` v4
    /// behaviour 9).
    pub fn context_chip(&self) -> Option<(String, bool)> {
        self.suggestion.clone().map(|s| (s, self.context_on))
    }

    /// Turns the offered context chip on or off.
    pub fn toggle_context(&mut self, cx: &mut Context<Self>) {
        if self.suggestion.is_some() {
            self.context_on = !self.context_on;
            cx.notify();
        }
    }

    /// Puts `question` into the field without sending it (`SPEC-0313` behaviour 4).
    pub fn prefill(&mut self, question: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |f, cx| f.set_text(question, cx));
    }

    /// The question field's text.
    pub fn question(&self, cx: &gpui::App) -> String {
        self.input.read(cx).state.buffer.text()
    }

    /// Shares the terminal `view` with the chat, or stops sharing it (`SPEC-0313` behaviour 5).
    pub fn share(&mut self, view: &Entity<TerminalView>, on: bool, cx: &mut Context<Self>) {
        let known = self.shared.iter().position(|s| s.view == view.downgrade());
        match (on, known) {
            (true, None) => self.shared.push(Shared {
                view: view.downgrade(),
                last: SharedTab::default(),
            }),
            (false, Some(i)) => {
                self.shared.remove(i);
            }
            _ => {}
        }
        view.update(cx, |v, cx| v.set_shared(on, cx));
        self.publish(cx);
        cx.notify();
    }

    /// Stops sharing every tab (New Chat, the assistant switched off).
    pub fn unshare_all(&mut self, cx: &mut Context<Self>) {
        for s in std::mem::take(&mut self.shared) {
            if let Some(v) = s.view.upgrade() {
                v.update(cx, |v, cx| v.set_shared(false, cx));
            }
        }
        self.publish(cx);
        cx.notify();
    }

    /// Names of the shared tabs that still exist.
    pub fn shared_names(&self, cx: &gpui::App) -> Vec<String> {
        self.shared
            .iter()
            .filter_map(|s| s.view.upgrade())
            .map(|v| v.read(cx).label().unwrap_or_else(|| "Local".to_owned()))
            .collect()
    }

    /// The terminal "Insert into Terminal" types into: the shared tab, else the one of the newest
    /// terminal chip (`SPEC-0313` behaviour 8).
    pub fn command_target(&self) -> Option<Entity<TerminalView>> {
        self.shared
            .iter()
            .rev()
            .find_map(|s| s.view.upgrade())
            .or_else(|| self.last_terminal.as_ref().and_then(WeakEntity::upgrade))
    }

    /// Says why a command or a code block was not inserted.
    pub fn note(&mut self, text: String, cx: &mut Context<Self>) {
        self.error = Some(text);
        self.info = None;
        cx.notify();
    }

    /// Says what was done ("Applied to …").
    pub fn inform(&mut self, text: String, cx: &mut Context<Self>) {
        self.info = Some(text);
        self.error = None;
        cx.notify();
    }

    /// The note shown that is no error (tests).
    pub fn info(&self) -> Option<&str> {
        self.info.as_deref()
    }

    /// Whether an answer is awaited.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Sets the question field (tests).
    pub fn set_question(&mut self, text: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// The commands of the assistant's answers, newest last (`SPEC-0313` behaviour 8).
    pub fn commands(&self) -> Vec<String> {
        self.messages
            .iter()
            .filter(|m| m.role == "assistant")
            .flat_map(|m| ferris_idea_ai::context::command_blocks(&m.content))
            .collect()
    }

    /// The code blocks of the assistant's answers in the order drawn: language and text.
    pub fn code_blocks(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .filter(|m| m.role == "assistant")
            .flat_map(|m| md::parse(&m.content))
            .filter_map(|b| match b {
                Block::Code(lang, code) => Some((lang, code)),
                _ => None,
            })
            .collect()
    }

    /// The preview of Apply shown, if any (tests).
    pub fn apply_preview(&self) -> Option<&ApplyPreview> {
        self.preview.as_ref().map(|(_, p)| p)
    }

    /// Shows the preview of applying code block `block` (the workbench made it).
    pub fn show_apply_preview(
        &mut self,
        block: usize,
        preview: ApplyPreview,
        cx: &mut Context<Self>,
    ) {
        self.preview = Some((block, preview));
        self.error = None;
        self.info = None;
        cx.notify();
    }

    /// The link waiting for confirmation (tests).
    pub fn pending_link(&self) -> Option<&str> {
        self.pending_link.as_deref()
    }

    /// The conversation as text (tests): `You: …`, `AI: …`, `Attached: chip`, `Sharing: tab`,
    /// `Error: …`.
    pub fn texts(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .messages
            .iter()
            .map(|m| {
                let who = if m.role == "user" { "You" } else { "AI" };
                format!("{who}: {}", m.content)
            })
            .collect();
        out.extend(
            self.attachments
                .iter()
                .map(|a| format!("Attached: {}", a.chip())),
        );
        if let Some(e) = &self.error {
            out.push(format!("Error: {e}"));
        }
        out
    }

    /// The shared tabs' snapshots that changed since they last went (`SPEC-0313` behaviour 5).
    fn shared_snapshots(&mut self, cx: &mut Context<Self>) {
        let ai = crate::appearance::ide_settings(cx).ai;
        let lines = ferris_idea_ai::context::terminal_lines(ai.terminal_lines);
        let budget = ferris_idea_ai::context::attachment_chars(ai.attachment_chars);
        self.shared.retain(|s| s.view.upgrade().is_some());
        let mut new = Vec::new();
        for s in &mut self.shared {
            let Some(view) = s.view.upgrade() else {
                continue;
            };
            let Some(mut a) = view.read(cx).ai_attachment(
                ferris_idea_ai::context::Source::TerminalCommand,
                lines,
                budget,
            ) else {
                continue;
            };
            if s.last.take(&a.code) {
                a.label.push_str(" (shared)");
                new.push(a);
            }
        }
        for a in new {
            if ferris_idea_ai::context::may_add(&self.attachments, &a, budget).is_ok() {
                self.attachments.push(a);
            }
        }
    }

    /// The offered chip's attachment when it is on and not attached already.
    fn offered_attachment(&mut self, cx: &mut Context<Self>) {
        if !self.context_on {
            return;
        }
        self.context_on = false;
        let Some(source) = self.source.clone() else {
            return;
        };
        let Some(a) = (source.attachment)(cx) else {
            return;
        };
        if self.attachments.iter().any(|b| b.label == a.label) {
            return;
        }
        if ferris_idea_ai::context::may_add(&self.attachments, &a, Self::budget(cx)).is_ok() {
            self.attachments.insert(0, a);
        }
    }

    /// Sends the question field's text, or `question` when given, with the waiting chips, the
    /// offered file when its chip is on, and the shared tabs' new output (behaviour 4, v4
    /// behaviour 9, `SPEC-0313` behaviours 1, 5). The answer arrives in the background.
    pub fn ask(&mut self, question: Option<&str>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = match question {
            Some(q) => q.to_owned(),
            None => self.input.read(cx).state.buffer.text(),
        };
        if text.trim().is_empty() {
            return;
        }
        let settings = crate::appearance::ide_settings(cx).ai;
        // Claude's plan set to the Terminal without the Anthropic API template sends nothing, not
        // even to the endpoint beside it; the question stays in the field (SPEC-0160 v2
        // behaviour 6). A missing key is found by the request itself, which sends nothing either.
        if settings.enabled
            && ferris_idea_ai::claude_in_terminal(&settings)
            && (ferris_idea_ai::provider_of(&settings.endpoint) != Some("Anthropic")
                || settings.model.trim().is_empty())
        {
            self.error = Some(ferris_idea_ai::AiError::ClaudePlan.to_string());
            self.info = None;
            cx.notify();
            return;
        }
        // The offered file is read only for a question the person sends from the field; Explain
        // Code attaches its own.
        if question.is_none() && settings.enabled {
            self.offered_attachment(cx);
        }
        if settings.enabled {
            self.shared_snapshots(cx);
        }
        let message = ferris_idea_ai::user_message_with(&text, &self.attachments);
        self.asked.insert(
            self.messages.len(),
            Asked {
                chips: self.chips(),
                question: text.trim().to_owned(),
            },
        );
        let mut sent = self.messages.clone();
        sent.push(message.clone());
        self.messages.push(message);
        self.attachments.clear();
        self.publish(cx);
        self.error = None;
        self.info = None;
        self.adding = false;
        self.preview = None;
        self.busy = true;
        self.started = Some(Instant::now());
        self.input.update(cx, |f, cx| f.set_text("", cx));
        // The answer is streamed into the last message as it arrives (SPEC-0157 behaviour 1).
        self.messages.push(Message::new("assistant", ""));
        self.scroll.scroll_to_bottom();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop = stop.clone();
        let pieces = shared.clone();
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = result.clone();
        cx.background_executor()
            .spawn(async move {
                let key = ferris_idea_ai::stored_key(
                    &ferris_idea_database::SystemStore,
                    &settings.endpoint,
                );
                let answer =
                    ferris_idea_ai::chat_stream(&settings, key.as_deref(), &sent, &mut |piece| {
                        if let Ok(mut s) = pieces.lock() {
                            s.push_str(piece);
                        }
                        !stop.load(std::sync::atomic::Ordering::Relaxed)
                    });
                if let Ok(mut r) = slot.lock() {
                    *r = Some(answer);
                }
            })
            .detach();
        cx.spawn(async move |this, cx| {
            let mut seconds = 0;
            let answer = loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(40))
                    .await;
                if let Some(answer) = result.lock().ok().and_then(|mut r| r.take()) {
                    break answer;
                }
                let so_far = shared.lock().map(|s| s.clone()).unwrap_or_default();
                let alive = this.update(cx, |this, cx| {
                    if let Some(last) = this.messages.last_mut()
                        && last.content != so_far
                    {
                        last.content = so_far;
                        this.scroll.scroll_to_bottom();
                        cx.notify();
                    }
                    // The seconds of "Answering…" count on.
                    let now = this.started.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                    if now != seconds {
                        seconds = now;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.started = None;
                this.scroll.scroll_to_bottom();
                match answer {
                    Ok(text) => {
                        if let Some(last) = this.messages.last_mut() {
                            last.content = text;
                        }
                    }
                    Err(e) => {
                        // An empty answer is not kept.
                        if this.messages.last().is_some_and(|m| m.content.is_empty()) {
                            this.messages.pop();
                        }
                        this.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Stops the answer that is arriving; what came so far stays (`SPEC-0157` behaviour 2).
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            cx.notify();
        }
    }

    /// Starts a new conversation; sharing ends (`SPEC-0313` behaviour 5).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            self.messages.clear();
            self.asked.clear();
            self.attachments.clear();
            self.error = None;
            self.info = None;
            self.preview = None;
            self.pending_link = None;
            self.copied = None;
            self.code_scroll.clear();
            self.context_on = false;
            self.unshare_all(cx);
        }
    }

    /// Asks to open `url` (a link of an answer): it opens only after confirmation, and only
    /// `http` and `https` (`SPEC-0138` v4 behaviour 7).
    pub fn click_link(&mut self, url: String, cx: &mut Context<Self>) {
        self.pending_link = Some(url);
        cx.notify();
    }

    fn open_link(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.pending_link.take()
            && md::openable(&url)
        {
            cx.emit(AiChatEvent::OpenLink(url));
        }
        cx.notify();
    }

    fn copy(&mut self, block: usize, code: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(code.to_owned()));
        self.copied = Some(block);
        cx.notify();
    }

    /// Sends the question, or stops the answer arriving (the field's button).
    fn send_or_stop(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            self.stop(cx);
        } else {
            self.ask(None, cx);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let empty = self.input.read(cx).state.buffer.text().is_empty();
        match k.key.as_str() {
            "enter" if !k.modifiers.shift && !k.modifiers.alt => self.ask(None, cx),
            "escape" if self.pending_link.is_some() || self.adding || self.preview.is_some() => {
                self.pending_link = None;
                self.adding = false;
                self.preview = None;
                cx.notify();
            }
            // Escape in the empty field removes the last chip (`SPEC-0313` behaviour 1).
            "escape" if empty && !self.attachments.is_empty() && !self.busy => {
                self.attachments.pop();
                self.publish(cx);
                cx.notify();
            }
            "escape" if self.busy => self.stop(cx),
            "escape" => cx.emit(AiChatEvent::Hide),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The editors' colours of `code` in `language`, kept while the answer stays.
    fn colours_of(&mut self, language: &str, code: &str) -> Rc<Colours> {
        let key = (language.to_owned(), code.to_owned());
        if let Some(c) = self.colours.get(&key) {
            return c.clone();
        }
        let colours = Rc::new(match md::file_for_language(language) {
            Some(file) => crate::diff_model::syntax_by_line(std::path::Path::new(file), code),
            None => Vec::new(),
        });
        // An answer arriving makes a new entry per piece of its open block.
        if self.colours.len() > 64 {
            self.colours.clear();
        }
        self.colours.insert(key, colours.clone());
        colours
    }
}

/// A small text button of the chat (code block actions, the field's buttons).
fn small_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
) -> Stateful<Div> {
    let p = Palette::current();
    div()
        .id(id)
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(4.))
        // A button, not text: a frame and a fill that darkens under the pointer (window
        // check C2).
        .border_1()
        .border_color(c(p.separator))
        .bg(c(p.island))
        .text_xs()
        .text_color(c(p.ui_text))
        .cursor_pointer()
        .hover(|s| s.bg(c(p.tree_hover)))
        .active(|s| s.bg(c(p.tree_selection_inactive)))
        .child(label.into())
}

fn button_semantics(name: &str, test_id: String, effect: Effect) -> Semantics {
    Semantics::new(Role::Button)
        .name(name)
        .test_id(test_id)
        .action(AxAction::Click)
        .effects(&[effect])
}

/// Counters of the elements drawn in one frame, for their ids.
#[derive(Default)]
struct Counts {
    code: usize,
    command: usize,
    link: usize,
    table: usize,
    piece: usize,
}

impl AiChatView {
    /// Inline runs as words that wrap only between words (`SPEC-0138` v4 behaviour 7): inline
    /// code is a chip in the editors' font, links open after confirmation.
    fn flow(&self, runs: &[Run], counts: &mut Counts, cx: &mut Context<Self>) -> Div {
        let p = Palette::current();
        let words = md::words(runs);
        let mut link_ids: HashMap<usize, usize> = HashMap::new();
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_x_1()
            .gap_y_0p5()
            .children(words.into_iter().map(|word| {
                let shown: String = word.iter().map(|(_, t)| t.as_str()).collect();
                // A word longer than the line is cut at its end, never broken into characters.
                div()
                    .flex_none()
                    .max_w_full()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .relative()
                    .semantics(Semantics::new(Role::TextRun).name(shown))
                    .children(word.into_iter().map(|(i, text)| {
                        let run = &runs[i];
                        // A piece never wraps and never shrinks: a chip that could shrink was
                        // laid out a few pixels wide with its text one character per line over
                        // the words around it (window check C1, 2026-11-15).
                        let piece = div()
                            .flex_none()
                            .whitespace_nowrap()
                            .when(run.bold, |d| d.font_weight(FontWeight::BOLD))
                            .when(run.italic, |d| d.italic());
                        if run.code {
                            return piece
                                .font_family(crate::fonts::editor_family())
                                .text_size(px(12.))
                                .px(px(4.))
                                .rounded(px(3.))
                                .bg(c(p.tree_selection_inactive))
                                .relative()
                                .semantics(Semantics::new(Role::Code).name(text.clone()))
                                .child(text)
                                .into_any_element();
                        }
                        if let Some(url) = run.link.clone() {
                            let n = *link_ids.entry(i).or_insert_with(|| {
                                counts.link += 1;
                                counts.link - 1
                            });
                            let label = md::plain(std::slice::from_ref(run));
                            counts.piece += 1;
                            return piece
                                .id(("ai-link-piece", counts.piece))
                                .text_color(c(p.link))
                                .underline()
                                .cursor_pointer()
                                .relative()
                                .semantics(
                                    Semantics::new(Role::Link)
                                        .name(label)
                                        .description(url.clone())
                                        .test_id(format!("ai-link-{n}"))
                                        .action(AxAction::Click)
                                        .effects(&[Effect::None]),
                                )
                                .child(text)
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.click_link(url.clone(), cx)
                                }))
                                .into_any_element();
                        }
                        piece.child(text).into_any_element()
                    }))
            }))
    }

    /// The sideways scroll of code block `n` and, when its lines are wider than the block, what
    /// shows that they go on (window check C3): a fade at the edge that has more and a thin bar
    /// under the lines with the shown part as its thumb. The sizes are those of the last frame;
    /// when they changed the chat draws once more.
    fn overflow(
        &mut self,
        n: usize,
        cx: &mut Context<Self>,
    ) -> (gpui::ScrollHandle, Option<Vec<AnyElement>>) {
        let p = Palette::current();
        let (scroll, drawn) = self
            .code_scroll
            .entry(n)
            .or_insert_with(|| (gpui::ScrollHandle::new(), 0.))
            .clone();
        let max = f32::from(scroll.max_offset().width);
        if (max - drawn).abs() > 0.5 {
            if let Some(entry) = self.code_scroll.get_mut(&n) {
                entry.1 = max;
            }
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        if max <= 0.5 {
            return (scroll, None);
        }
        let shown = f32::from(scroll.bounds().size.width).max(1.);
        let left = (-f32::from(scroll.offset().x)).clamp(0., max);
        let total = shown + max;
        let bg: gpui::Hsla = gpui::rgb(p.background).into();
        let mut clear = bg;
        clear.a = 0.;
        let fade = |to_right: bool| {
            div()
                .absolute()
                .top_0()
                .bottom(px(4.))
                .w(px(32.))
                .when(to_right, |d| d.right_0())
                .when(!to_right, |d| d.left_0())
                .bg(gpui::linear_gradient(
                    if to_right { 90. } else { 270. },
                    gpui::linear_color_stop(clear, 0.),
                    gpui::linear_color_stop(bg, 1.),
                ))
                .into_any_element()
        };
        let mut parts = Vec::new();
        if left < max - 0.5 {
            parts.push(fade(true));
        }
        if left > 0.5 {
            parts.push(fade(false));
        }
        parts.push(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(4.))
                .semantics(
                    Semantics::new(Role::ScrollBar)
                        .name("The lines go on; scroll sideways")
                        .value(format!("{:.0} of {:.0} px shown", shown, total))
                        .test_id(format!("ai-code-more-{n}")),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(1.))
                        .h(px(3.))
                        .left(px(shown * left / total))
                        .w(px((shown * shown / total).max(16.)))
                        .rounded(px(2.))
                        .bg(c(p.ui_dim)),
                )
                .into_any_element(),
        );
        (scroll, Some(parts))
    }

    /// One code block: a header with its language and actions, the lines in the editors' colours
    /// (`SPEC-0138` v4 behaviour 8).
    fn code_block(
        &mut self,
        language: &str,
        code: &str,
        command: Option<(usize, String)>,
        counts: &mut Counts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::current();
        let n = counts.code;
        counts.code += 1;
        let colours = self.colours_of(language, code);
        let lh = crate::fonts::editor_line_height();
        let shown = if language.is_empty() {
            "text".to_owned()
        } else {
            language.to_owned()
        };
        let copied = self.copied == Some(n);
        let (scroll, more) = self.overflow(n, cx);
        let action = |id: &str, label: &str, effect: Effect, event: fn(String) -> AiChatEvent| {
            let code = code.to_owned();
            small_button(
                gpui::ElementId::Name(format!("ai-code-{id}-{n}").into()),
                label.to_owned(),
            )
            .relative()
            .semantics(button_semantics(label, format!("ai-code-{id}-{n}"), effect))
            .on_click(
                cx.listener(move |_, _: &gpui::ClickEvent, _, cx| cx.emit(event(code.clone()))),
            )
        };
        let insert = action(
            "insert",
            "Insert at Caret",
            Effect::Write,
            AiChatEvent::InsertAtCaret,
        );
        let replace = action(
            "replace",
            "Replace Selection",
            Effect::Write,
            AiChatEvent::ReplaceSelection,
        );
        let copy_code = code.to_owned();
        let apply_code = code.to_owned();
        let preview = self
            .preview
            .as_ref()
            .filter(|(b, _)| *b == n)
            .map(|(_, p)| p.clone());
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.separator))
            .bg(gpui::rgb(p.background))
            .overflow_hidden()
            .relative()
            .semantics(
                Semantics::new(Role::Code)
                    .name(shown.clone())
                    .value(code.to_owned())
                    .test_id(format!("ai-code-{n}")),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py(px(2.))
                    .border_b_1()
                    .border_color(c(p.separator))
                    .bg(c(p.island))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(40.))
                            .text_xs()
                            .text_color(c(p.ui_muted))
                            .child(shown),
                    )
                    .child(
                        small_button(
                            gpui::ElementId::Name(format!("ai-code-copy-{n}").into()),
                            if copied { "Copied" } else { "Copy" },
                        )
                        .relative()
                        .semantics(button_semantics("Copy", format!("ai-code-copy-{n}"), Effect::None))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.copy(n, &copy_code, cx)
                        })),
                    )
                    .when_some(command, |d, (k, command)| {
                        // Typed into a terminal, never run (`SPEC-0313` behaviour 8).
                        d.child(
                            small_button(("ai-insert", k), "Insert into Terminal")
                                .relative()
                                .semantics(button_semantics(
                                    "Insert into Terminal",
                                    format!("ai-insert-{k}"),
                                    Effect::None,
                                ))
                                .tip_text(
                                    format!("ai-insert-tip-{k}"),
                                    ferris_idea_ui::tooltip::Tip::plain(
                                        "Types the command into the terminal; it runs only when you press Enter",
                                    ),
                                )
                                .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                    cx.emit(AiChatEvent::InsertCommand(command.clone()))
                                })),
                        )
                    })
                    .child(insert)
                    .child(replace)
                    .child(
                        small_button(gpui::ElementId::Name(format!("ai-code-apply-{n}").into()), "Apply…")
                            .relative()
                            .semantics(button_semantics("Apply", format!("ai-code-apply-{n}"), Effect::None))
                            .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(AiChatEvent::PreviewApply(n, apply_code.clone()))
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .relative()
                    .child(
                        div()
                            .id(("ai-code-lines", n))
                            .track_scroll(&scroll)
                            .w_full()
                            .flex()
                            .flex_row()
                            .overflow_x_scroll()
                            .px_2()
                            .py_1()
                            .font_family(crate::fonts::editor_family())
                            .text_size(px(crate::fonts::editor_font_size()))
                            .text_color(gpui::rgb(p.text))
                            // As wide as the longest line, so the block scrolls sideways.
                            .child(div().flex_none().flex().flex_col().children(
                                code.split('\n').enumerate().map(|(i, line)| {
                                    let kinds = colours.get(i).cloned().unwrap_or_default();
                                    div()
                                        .h(px(lh))
                                        .whitespace_nowrap()
                                        .child(crate::diff_view::side_text(line, &kinds, &[], 0))
                                }),
                            )),
                    )
                    .when_some(more, |d, more| d.children(more)),
            )
            .when_some(preview, |d, preview| d.child(self.preview_rows(&preview, cx)))
            .into_any_element()
    }

    /// The diff of the Apply preview with its buttons (`SPEC-0138` v4 behaviour 8): nothing
    /// changes before "Apply Changes".
    fn preview_rows(&self, preview: &ApplyPreview, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::current();
        let after = preview.after();
        let rows = ferris_idea_vcs::side_by_side(&preview.before, &after);
        let unified = ferris_idea_vcs::unified_rows(&rows);
        let changed: Vec<usize> = unified
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind.is_some())
            .map(|(i, _)| i)
            .collect();
        // Changed lines with two lines around them.
        let keep: std::collections::BTreeSet<usize> = changed
            .iter()
            .flat_map(|&i| i.saturating_sub(2)..(i + 3).min(unified.len()))
            .collect();
        let added = unified
            .iter()
            .filter(|r| r.kind == Some(ferris_idea_vcs::LineChangeKind::Added))
            .count();
        let deleted = unified
            .iter()
            .filter(|r| r.kind == Some(ferris_idea_vcs::LineChangeKind::Deleted))
            .count();
        let lh = crate::fonts::editor_line_height();
        let mut last = None;
        let mut lines: Vec<AnyElement> = Vec::new();
        for &i in keep.iter().take(400) {
            if last.is_some_and(|l: usize| i > l + 1) {
                lines.push(
                    div()
                        .h(px(lh))
                        .text_color(c(p.ui_dim))
                        .child("⋯")
                        .into_any_element(),
                );
            }
            last = Some(i);
            let r = &unified[i];
            let (sign, bg) = match r.kind {
                Some(ferris_idea_vcs::LineChangeKind::Added) => ("+", Some(p.diff_inserted)),
                Some(ferris_idea_vcs::LineChangeKind::Deleted) => ("−", Some(p.diff_deleted)),
                Some(_) => ("~", Some(p.diff_modified)),
                None => (" ", None),
            };
            lines.push(
                div()
                    .h(px(lh))
                    .whitespace_nowrap()
                    .flex()
                    .when_some(bg, |d, bg| d.bg(gpui::rgba((bg << 8) | 0x55)))
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_color(c(p.ui_muted))
                            .child(sign),
                    )
                    .child(r.text.clone())
                    .into_any_element(),
            );
        }
        let none = changed.is_empty();
        div()
            .w_full()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(c(p.separator))
            .relative()
            .semantics(
                Semantics::new(Role::Group)
                    .name(format!("Apply to {}", preview.label))
                    .value(format!("+{added} −{deleted}"))
                    .test_id("ai-apply-preview"),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(c(p.ui_muted))
                    .child(if none {
                        format!("{} already has this code", preview.label)
                    } else {
                        format!(
                            "Changes to {}: {added} line{} added, {deleted} removed — nothing changes until you apply",
                            preview.label,
                            if added == 1 { "" } else { "s" }
                        )
                    }),
            )
            .child(
                div()
                    .id("ai-apply-diff")
                    .w_full()
                    .max_h(px(lh * 16.))
                    .overflow_y_scroll()
                    .px_2()
                    .font_family(crate::fonts::editor_family())
                    .text_size(px(crate::fonts::editor_font_size()))
                    .children(lines),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .p_2()
                    .child(
                        crate::metrics::button("ai-apply-cancel", "Cancel", false)
                            .relative()
                            .semantics(button_semantics("Cancel", "ai-apply-cancel".into(), Effect::None))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.preview = None;
                                cx.notify();
                            })),
                    )
                    .when(!none, |d| {
                        d.child(
                            crate::metrics::button("ai-apply-confirm", "Apply Changes", true)
                                .relative()
                                .semantics(button_semantics(
                                    "Apply Changes",
                                    "ai-apply-confirm".into(),
                                    Effect::Write,
                                ))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    if let Some((_, preview)) = this.preview.take() {
                                        cx.emit(AiChatEvent::Apply(preview));
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The blocks of an answer (`SPEC-0138` v4 behaviour 7).
    fn blocks(
        &mut self,
        content: &str,
        counts: &mut Counts,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = Palette::current();
        let commands = ferris_idea_ai::context::command_blocks(content);
        let mut shell = 0usize;
        let mut out = Vec::new();
        for block in md::parse(content) {
            let element = match block {
                Block::Heading(level, runs) => {
                    let size = match level {
                        1 => 17.,
                        2 => 15.5,
                        3 => 14.,
                        _ => 13.,
                    };
                    div()
                        .w_full()
                        .mt(px(if level <= 2 { 6. } else { 2. }))
                        .text_size(px(size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .relative()
                        .semantics(Semantics::new(Role::Heading).name(md::plain(&runs)))
                        .child(self.flow(&runs, counts, cx))
                        .into_any_element()
                }
                Block::Paragraph(runs) => div()
                    .w_full()
                    .relative()
                    .semantics(Semantics::new(Role::Paragraph).name(md::plain(&runs)))
                    .child(self.flow(&runs, counts, cx))
                    .into_any_element(),
                Block::Item(depth, marker, runs) => div()
                    .w_full()
                    .flex()
                    .ml(px(depth as f32 * 16.))
                    .relative()
                    .semantics(
                        Semantics::new(Role::ListItem)
                            .name(format!("{marker} {}", md::plain(&runs))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(18.))
                            .pr_1()
                            .text_color(c(p.ui_muted))
                            .child(marker),
                    )
                    .child(div().flex_1().min_w_0().child(self.flow(&runs, counts, cx)))
                    .into_any_element(),
                Block::Quote(runs) => div()
                    .w_full()
                    .pl_2()
                    .border_l_2()
                    .border_color(c(p.separator))
                    .text_color(c(p.ui_muted))
                    .relative()
                    .semantics(Semantics::new(Role::Blockquote).name(md::plain(&runs)))
                    .child(self.flow(&runs, counts, cx))
                    .into_any_element(),
                Block::Rule => div()
                    .w_full()
                    .my_1()
                    .h(px(1.))
                    .bg(c(p.separator))
                    .relative()
                    .semantics(Semantics::new(Role::Splitter).name("Rule"))
                    .into_any_element(),
                Block::Code(language, code) => {
                    let is_shell = ferris_idea_ai::context::SHELL_LANGUAGES
                        .contains(&language.as_str())
                        && !code.trim().is_empty();
                    let command = if is_shell {
                        let k = shell;
                        shell += 1;
                        commands.get(k).cloned().map(|cmd| {
                            let id = counts.command;
                            counts.command += 1;
                            (id, cmd)
                        })
                    } else {
                        None
                    };
                    self.code_block(&language, &code, command, counts, cx)
                }
                Block::Table(header, rows) => {
                    let n = counts.table;
                    counts.table += 1;
                    let columns = header
                        .len()
                        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
                    let row = |cells: &[Vec<Run>],
                               head: bool,
                               this: &Self,
                               counts: &mut Counts,
                               cx: &mut Context<Self>| {
                        div()
                            .w_full()
                            .flex()
                            .when(head, |d| {
                                d.bg(c(p.island)).font_weight(FontWeight::SEMIBOLD)
                            })
                            .children((0..columns).map(|k| {
                                let cell = cells.get(k).cloned().unwrap_or_default();
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_2()
                                    .py(px(3.))
                                    .when(k > 0, |d| d.border_l_1().border_color(c(p.separator)))
                                    .relative()
                                    .semantics(Semantics::new(Role::Cell).name(md::plain(&cell)))
                                    .child(this.flow(&cell, counts, cx))
                            }))
                    };
                    let mut table = div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.separator))
                        .overflow_hidden()
                        .relative()
                        .semantics(
                            Semantics::new(Role::Table)
                                .name(
                                    header
                                        .iter()
                                        .map(|h| md::plain(h))
                                        .collect::<Vec<_>>()
                                        .join(" | "),
                                )
                                .value(format!("{columns} columns, {} rows", rows.len()))
                                .test_id(format!("ai-table-{n}")),
                        )
                        .child(row(&header, true, self, counts, cx));
                    for r in &rows {
                        table = table.child(
                            div()
                                .border_t_1()
                                .border_color(c(p.separator))
                                .child(row(r, false, self, counts, cx)),
                        );
                    }
                    table.into_any_element()
                }
            };
            out.push(element);
        }
        out
    }

    /// The header: the title, the model (the endpoint in its tooltip) and New Chat (`SPEC-0138`
    /// v4 behaviour 10).
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let p = Palette::current();
        let ai = crate::appearance::ide_settings(cx).ai;
        let (model, details): (String, Vec<String>) = if !ai.enabled {
            (
                "Off".into(),
                vec!["Switch it on in Settings › Tools › AI Assistant".into()],
            )
        } else if ferris_idea_ai::claude_in_terminal(&ai)
            && ferris_idea_ai::provider_of(&ai.endpoint) != Some("Anthropic")
        {
            (
                ferris_idea_ai::CLAUDE_TERMINAL_NAME.into(),
                vec![ferris_idea_ai::subscription::CLAUDE_PLAN_MESSAGE.into()],
            )
        } else if let Some(cli) = ferris_idea_ai::subscription::Cli::from_id(&ai.subscription)
            .filter(|_| !ferris_idea_ai::claude_in_terminal(&ai))
        {
            let m = ai.model.trim();
            (
                if m.is_empty() {
                    cli.name().to_owned()
                } else {
                    m.to_owned()
                },
                vec![format!("Through {}", cli.name())],
            )
        } else if ai.endpoint.trim().is_empty() || ai.model.trim().is_empty() {
            (
                "Not set up".into(),
                vec!["Name an endpoint and a model in Settings › Tools › AI Assistant".into()],
            )
        } else {
            (
                ai.model.trim().to_owned(),
                vec![ai.endpoint.trim().to_owned()],
            )
        };
        div()
            // The shared tool window header (SPEC-0061 v6 behaviour 33).
            .map(crate::metrics::panel_header)
            .min_w_0()
            .child(crate::metrics::panel_title("AI Chat"))
            .child(self.tabs(cx))
            .when(self.tab == AiTab::Chat, |d| {
                d.child(
                    div()
                        .id("ai-model")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(c(p.ui_muted))
                        .relative()
                        .semantics(
                            Semantics::new(Role::Label)
                                .name(model.clone())
                                .description(details.join("\n"))
                                .test_id("ai-model"),
                        )
                        .child(model.clone())
                        .tip_text(
                            "ai-model-tip",
                            ferris_idea_ui::tooltip::Tip {
                                title: model,
                                shortcut: String::new(),
                                details,
                            },
                        ),
                )
            })
            .when(self.tab == AiTab::Chat, |d| {
                d.child(
                    small_button("ai-new", "New Chat")
                        .relative()
                        .semantics(button_semantics("New Chat", "ai-new".into(), Effect::None))
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.clear(cx))),
                )
            })
            .when(self.tab == AiTab::Agents, |d| d.child(div().flex_1()))
    }

    /// The tabs Chat and Agents; the Agents tab counts the agents that need the person
    /// (`SPEC-0357` behaviours 1, 10).
    fn tabs(&self, cx: &mut Context<Self>) -> Div {
        let p = Palette::current();
        let needing = self.agents.as_ref().map_or(0, |a| a.read(cx).needing());
        let agents = if needing > 0 {
            format!("Agents ({needing})")
        } else {
            "Agents".to_owned()
        };
        let tab = |id: &'static str, label: String, which: AiTab, on: bool| {
            div()
                .id(id)
                .flex_none()
                .px(px(6.))
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_xs()
                .cursor_pointer()
                .when(on, |d| {
                    d.bg(c(p.tree_selection_inactive)).text_color(c(p.ui_text))
                })
                .when(!on, |d| {
                    d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
                })
                .relative()
                .semantics(
                    Semantics::new(Role::Button)
                        .name(label.clone())
                        .test_id(id)
                        .selected(on)
                        .action(AxAction::Click)
                        .effects(&[Effect::None]),
                )
                .child(label)
                .on_click(
                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.show_tab(which, cx)),
                )
        };
        div()
            .flex()
            .gap_1()
            .child(tab(
                "ai-tab-chat",
                "Chat".into(),
                AiTab::Chat,
                self.tab == AiTab::Chat,
            ))
            .child(tab(
                "ai-tab-agents",
                agents,
                AiTab::Agents,
                self.tab == AiTab::Agents,
            ))
    }

    /// The chips row of the field: "+", the offered file and the waiting chips (`SPEC-0138` v4
    /// behaviour 9, `SPEC-0313` behaviour 1).
    fn chips_row(&self, cx: &mut Context<Self>) -> Div {
        let p = Palette::current();
        let chips = self.chips();
        let offered = self
            .suggestion
            .clone()
            .filter(|s| !self.attachments.iter().any(|a| a.label == *s));
        let on = self.context_on;
        div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(
                small_button("ai-add", "+")
                    .w(px(20.))
                    .justify_center()
                    .border_1()
                    .border_color(c(p.separator))
                    .when(self.adding, |d| d.bg(c(p.tree_hover)))
                    .relative()
                    .semantics(
                        Semantics::new(Role::Button)
                            .name("Attach")
                            .test_id("ai-add")
                            .expanded(self.adding)
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    )
                    .tip_text(
                        "ai-add-tip",
                        ferris_idea_ui::tooltip::Tip::plain("Attach context"),
                    )
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.adding = !this.adding;
                        cx.notify();
                    })),
            )
            .when_some(offered, |d, label| {
                d.child(
                    div()
                        .id("ai-context")
                        .flex()
                        .items_center()
                        .gap_1()
                        .h(px(20.))
                        .px_2()
                        .rounded(px(4.))
                        .border_1()
                        .text_xs()
                        .max_w_full()
                        .cursor_pointer()
                        .when(on, |d| d.border_color(c(p.focus)).text_color(c(p.ui_text)))
                        .when(!on, |d| {
                            d.border_dashed()
                                .border_color(c(p.separator))
                                .text_color(c(p.ui_muted))
                        })
                        .relative()
                        .semantics(
                            Semantics::new(Role::CheckBox)
                                .name(label.clone())
                                .checked(on)
                                .test_id("ai-context")
                                .action(AxAction::Click)
                                .effects(&[Effect::None]),
                        )
                        .tip_text(
                            "ai-context-tip",
                            ferris_idea_ui::tooltip::Tip::plain(if on {
                                "Goes with the next question; click to leave it out"
                            } else {
                                "Not sent; click to send it with the next question"
                            }),
                        )
                        .child(if on { "✓" } else { "+" })
                        .child(div().min_w_0().truncate().child(label))
                        .on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_context(cx)
                            }),
                        ),
                )
            })
            .children(chips.into_iter().enumerate().map(|(i, chip)| {
                div()
                    .id(("ai-chip", i))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(20.))
                    .px_2()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(p.focus))
                    .text_xs()
                    // A long chip stays inside the chat's width; its × stays visible
                    // (hand test 213: it was cut off with the rest of the line).
                    .max_w_full()
                    .child(div().flex_1().min_w_0().truncate().child(chip))
                    .child(
                        div()
                            .id(("ai-chip-remove", i))
                            .flex_none()
                            .relative()
                            .semantics(button_semantics(
                                "Remove attachment",
                                format!("ai-chip-remove-{i}"),
                                Effect::None,
                            ))
                            .cursor_pointer()
                            .text_color(c(p.ui_muted))
                            .child("×")
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.remove_attachment(i, cx)
                            })),
                    )
            }))
    }

    /// The "+" list (`SPEC-0138` v4 behaviour 9).
    fn add_list(&self, cx: &mut Context<Self>) -> Div {
        let p = Palette::current();
        div()
            .w_full()
            .flex()
            .flex_col()
            .py_1()
            .rounded(px(6.))
            .border_1()
            .border_color(c(p.popup_border))
            .bg(c(p.popup))
            .children(AttachKind::ALL.into_iter().map(|kind| {
                div()
                    .id(gpui::ElementId::Name(
                        format!("ai-add-{}", kind.id()).into(),
                    ))
                    .px_2()
                    .py(px(3.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .relative()
                    .semantics(
                        Semantics::new(Role::MenuItem)
                            .name(kind.label())
                            .test_id(format!("ai-add-{}", kind.id()))
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    )
                    .child(kind.label())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.adding = false;
                        cx.emit(AiChatEvent::Attach(kind));
                        cx.notify();
                    }))
            }))
    }
}

impl Render for AiChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The Agents tab (`SPEC-0357` behaviour 1).
        if self.tab == AiTab::Agents {
            let agents = self.ensure_agents(cx);
            return div()
                .id("ai-chat")
                .track_focus(&self.focus)
                .size_full()
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.header(cx))
                .child(div().flex_1().min_h_0().child(agents))
                .into_any_element();
        }
        let p = Palette::current();
        let ai = crate::appearance::ide_settings(cx).ai;
        // Sharing ends when the assistant is switched off (`SPEC-0313` behaviour 5).
        if !ai.enabled && !self.shared.is_empty() {
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |this, cx| this.unshare_all(cx));
            })
            .detach();
        }
        // The offered file follows the active editor; another one starts as suggested again.
        let offered = self.source.as_ref().and_then(|s| (s.label)(cx));
        if offered != self.suggestion {
            self.suggestion = offered;
            self.context_on = false;
        }
        let messages = self.messages.clone();
        let sharing = self.shared_names(cx);
        let mut counts = Counts::default();
        let lines = self.question(cx).split('\n').count().clamp(1, FIELD_LINES);
        let field_height = lines as f32 * crate::fonts::editor_line_height() + 6.;
        let empty = self.question(cx).trim().is_empty();
        let last = messages.len().saturating_sub(1);
        let mut conversation: Vec<AnyElement> = Vec::new();
        for (i, m) in messages.iter().enumerate() {
            if m.role == "user" {
                let asked = self.asked.get(&i).cloned().unwrap_or(Asked {
                    chips: Vec::new(),
                    question: m.content.clone(),
                });
                conversation.push(
                    div()
                        .w_full()
                        .flex()
                        .justify_end()
                        .child(
                            div()
                                .max_w(gpui::relative(0.88))
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .px_3()
                                .py_2()
                                .rounded(px(10.))
                                .bg(c(p.tree_selection_inactive))
                                .relative()
                                .semantics(
                                    Semantics::new(Role::Article)
                                        .name("You")
                                        .value(asked.question.clone())
                                        .test_id(format!("ai-message-{i}")),
                                )
                                .when(!asked.chips.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap_1()
                                            .text_xs()
                                            .text_color(c(p.ui_muted))
                                            .children(asked.chips.iter().map(|chip| {
                                                div()
                                                    .px_1()
                                                    .rounded(px(3.))
                                                    .border_1()
                                                    .border_color(c(p.separator))
                                                    .child(format!("📎 {chip}"))
                                            })),
                                    )
                                })
                                .children(self.blocks(&asked.question, &mut counts, cx)),
                        )
                        .into_any_element(),
                );
            } else {
                let arriving = self.busy && i == last;
                let raw = m.content.clone();
                let blocks = self.blocks(&m.content, &mut counts, cx);
                conversation.push(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .relative()
                        .semantics(
                            Semantics::new(Role::Article)
                                .name("AI")
                                .value(raw.clone())
                                .busy(arriving)
                                .test_id(format!("ai-message-{i}")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .text_color(c(p.ui_dim))
                                .child(div().flex_1().child("Assistant"))
                                .when(!arriving && !raw.is_empty(), |d| {
                                    d.child(
                                        small_button(("ai-copy-answer", i), "Copy")
                                            .text_color(c(p.ui_dim))
                                            .relative()
                                            .semantics(button_semantics(
                                                "Copy answer",
                                                format!("ai-copy-answer-{i}"),
                                                Effect::None,
                                            ))
                                            .on_click(cx.listener(
                                                move |this, _: &gpui::ClickEvent, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(raw.clone()),
                                                    );
                                                    this.info = Some("Copied the answer".into());
                                                    cx.notify();
                                                },
                                            )),
                                    )
                                }),
                        )
                        .children(blocks)
                        .into_any_element(),
                );
            }
        }
        let busy = self.busy;
        let seconds = self.started.map(|s| s.elapsed().as_secs()).unwrap_or(0);
        div()
            .id("ai-chat")
            .key_context("AiChat")
            .capture_key_down(cx.listener(Self::on_key))
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(
                div()
                    .id("ai-messages")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .when(messages.is_empty(), |d| {
                        d.child(
                            div()
                                .pt_4()
                                .text_color(c(p.ui_muted))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child("Ask about your code.")
                                .child(
                                    div().text_xs().child(
                                        "The open file is offered below the conversation; it is sent only when you click it. \"+\" attaches a selection, problems, terminal output or the Git diff.",
                                    ),
                                ),
                        )
                    })
                    .children(conversation)
                    .when_some(self.error.clone(), |d, e| {
                        d.child(
                            div()
                                .text_color(c(p.status_text(crate::palette::Status::Error)))
                                .relative()
                                .semantics(Semantics::new(Role::Status).name(e.clone()).test_id("ai-error"))
                                .child(e),
                        )
                    })
                    .when(self.error.as_deref() == Some(ferris_idea_ai::AiError::ClaudePlan.to_string().as_str()), |d| {
                        // The two ways on with Claude's plan in the Terminal (SPEC-0160 v2
                        // behaviour 6).
                        d.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .child(
                                    small_button("ai-plan-api", "Use the Anthropic API…")
                                        .relative()
                                        .semantics(button_semantics(
                                            "Use the Anthropic API…",
                                            "ai-plan-api".into(),
                                            Effect::None,
                                        ))
                                        .tip_text(
                                            "ai-plan-api-tip",
                                            ferris_idea_ui::tooltip::Tip::plain(
                                                "Opens Settings › AI Assistant on the Anthropic API template; your API key goes to the keychain",
                                            ),
                                        )
                                        .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                            cx.emit(AiChatEvent::UseAnthropicApi)
                                        })),
                                )
                                .child(
                                    small_button("ai-plan-terminal", "Open Claude Code in the Terminal")
                                        .relative()
                                        .semantics(button_semantics(
                                            "Open Claude Code in the Terminal",
                                            "ai-plan-terminal".into(),
                                            Effect::None,
                                        ))
                                        .tip_text(
                                            "ai-plan-terminal-tip",
                                            ferris_idea_ui::tooltip::Tip::plain(
                                                "Runs your own Claude Code (claude) in a new terminal tab of the project; it uses your plan and signs in by itself",
                                            ),
                                        )
                                        .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                            cx.emit(AiChatEvent::OpenClaudeCode)
                                        })),
                                ),
                        )
                    }),
            )
            .when_some(self.pending_link.clone(), |d, url| {
                let ok = md::openable(&url);
                d.child(
                    div()
                        .mx_2()
                        .mb_1()
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(c(p.separator))
                        .bg(c(p.island))
                        .relative()
                        .semantics(
                            Semantics::new(Role::Group)
                                .name("Open link")
                                .value(url.clone())
                                .test_id("ai-link-confirm"),
                        )
                        .child(div().text_xs().child(if ok {
                            "Open this address in the browser?"
                        } else {
                            "Only http and https addresses open from the chat."
                        }))
                        .child(div().text_xs().text_color(c(p.ui_muted)).child(url))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    crate::metrics::button("ai-link-cancel", "Cancel", false)
                                        .relative()
                                        .semantics(button_semantics(
                                            "Cancel",
                                            "ai-link-cancel".into(),
                                            Effect::None,
                                        ))
                                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                            this.pending_link = None;
                                            cx.notify();
                                        })),
                                )
                                .when(ok, |d| {
                                    d.child(
                                        crate::metrics::button("ai-link-open", "Open", true)
                                            .relative()
                                            .semantics(button_semantics(
                                                "Open",
                                                "ai-link-open".into(),
                                                Effect::None,
                                            ))
                                            .on_click(cx.listener(
                                                |this, _: &gpui::ClickEvent, _, cx| this.open_link(cx),
                                            )),
                                    )
                                }),
                        ),
                )
            })
            .when(!sharing.is_empty(), |d| {
                d.child(
                    div()
                        .px_3()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(format!(
                            "Sharing: {} — its last command goes with each question when it changed",
                            sharing
                                .iter()
                                .map(|n| format!("\"{n}\""))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                )
            })
            .when(self.adding, |d| d.child(div().px_2().pb_1().child(self.add_list(cx))))
            // The field: chips, the question over several lines, Send or Stop (`SPEC-0138` v4
            // behaviour 11).
            .child(
                div().p_2().flex_none().child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_2()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(c(p.separator))
                        .bg(gpui::rgb(p.background))
                        .child(self.chips_row(cx))
                        .child(
                            div()
                                .w_full()
                                .h(px(field_height))
                                .min_w_0()
                                .overflow_hidden()
                                .relative()
                                .semantics(
                                    Semantics::new(Role::MultilineTextInput)
                                        .name("Question")
                                        .test_id("ai-question")
                                        .action(AxAction::Click)
                                        .effects(&[Effect::None]),
                                )
                                .child(self.input.clone())
                                .when(empty, |d| {
                                    d.child(
                                        div()
                                            .absolute()
                                            .top(px(3.))
                                            .left_0()
                                            .text_color(c(p.ui_dim))
                                            .child("Ask a question…"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(c(p.ui_dim))
                                        .when(busy, |d| {
                                            d.relative()
                                                .semantics(
                                                    Semantics::new(Role::ProgressIndicator)
                                                        .name(format!("Answering… {seconds} s"))
                                                        .busy(true)
                                                        .test_id("ai-progress"),
                                                )
                                                .text_color(c(p.ui_muted))
                                                .child(format!("● Answering… {seconds} s — Esc stops"))
                                        })
                                        .when(!busy, |d| {
                                            d.child(match &self.info {
                                                Some(info) => info.clone(),
                                                None => "↩ sends · ⇧↩ new line".to_owned(),
                                            })
                                        }),
                                )
                                .child(
                                    crate::metrics::button(
                                        if busy { "ai-stop" } else { "ai-send" },
                                        if busy { "Stop" } else { "Send" },
                                        !busy,
                                    )
                                    .when(!busy && empty, |d| d.opacity(0.5))
                                    .relative()
                                    .semantics(
                                        Semantics::new(Role::Button)
                                            .name(if busy { "Stop" } else { "Send" })
                                            .test_id(if busy { "ai-stop" } else { "ai-send" })
                                            .enabled(busy || !empty)
                                            .action(AxAction::Click)
                                            .effects(&[if busy { Effect::None } else { Effect::Http }]),
                                    )
                                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.send_or_stop(cx)
                                    })),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }
}
