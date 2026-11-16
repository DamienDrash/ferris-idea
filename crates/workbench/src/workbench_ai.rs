//! The AI chat and what the person hands it from the terminal, the Run tool window, the Problems
//! tool window and the Git diff (`SPEC-0138`, `SPEC-0313`): chips, Explain with AI, sharing a
//! terminal tab, and typing a suggested command into a terminal without running it.

use gpui::{App, AppContext, Context, Entity, Window};
use idecraft_ai::Attachment;
use idecraft_ai::context::{self as ai_context, Source};

use super::WorkbenchView;
use crate::TerminalView;
use crate::context_menu as model;

impl WorkbenchView {
    /// The chat, created on first use with its events wired (`SPEC-0138` behaviour 4).
    pub(super) fn ensure_ai_chat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<crate::AiChatView> {
        if let Some(v) = &self.ai_chat {
            return v.clone();
        }
        let root = self.root.clone();
        let view = cx.new(|cx| crate::AiChatView::new(root, cx));
        // The current file offered as a context chip (`SPEC-0138` v4 behaviour 9): its label on
        // every frame, its text only when the question goes with the chip on.
        let (a, b) = (cx.entity().downgrade(), cx.entity().downgrade());
        view.update(cx, |v, _| {
            v.set_context_source(crate::ai_chat_view::ContextSource {
                label: std::rc::Rc::new(move |cx: &App| {
                    a.upgrade().and_then(|w| w.read(cx).ai_offered_label(cx))
                }),
                attachment: std::rc::Rc::new(move |cx: &App| {
                    b.upgrade()
                        .and_then(|w| w.read(cx).ai_attachment_of(None, cx))
                }),
            })
        });
        self._ai_chat_events = Some(cx.subscribe_in(
            &view,
            window,
            |this, chat, event: &crate::AiChatEvent, window, cx| match event {
                crate::AiChatEvent::Hide => {
                    this.show_ai_chat = false;
                    this.focus_active(window, cx);
                    cx.notify();
                }
                crate::AiChatEvent::InsertCommand(command) => {
                    this.ai_insert_command(command, window, cx)
                }
                crate::AiChatEvent::InsertAtCaret(code) => {
                    this.ai_insert_code(chat, code, false, window, cx)
                }
                crate::AiChatEvent::ReplaceSelection(code) => {
                    this.ai_insert_code(chat, code, true, window, cx)
                }
                crate::AiChatEvent::PreviewApply(block, code) => {
                    this.ai_preview_apply(chat, *block, code, cx)
                }
                crate::AiChatEvent::Apply(preview) => this.ai_apply(chat, preview, window, cx),
                crate::AiChatEvent::Attach(kind) => this.ai_attach_kind(chat, *kind, window, cx),
                crate::AiChatEvent::OpenLink(url) => {
                    // Only addresses the person confirmed, and only http and https (behaviour 7).
                    if crate::ai_markdown::openable(url) {
                        cx.open_url(url);
                        this.opened_urls.push(url.clone());
                        this.status = Some(format!("Opened {url}"));
                        cx.notify();
                    }
                }
            },
        ));
        self.ai_chat = Some(view.clone());
        view
    }

    /// Puts a code block of an answer at the active editor's caret, or in place of its selection,
    /// as one undo step (`SPEC-0138` v4 behaviour 8).
    fn ai_insert_code(
        &mut self,
        chat: &Entity<crate::AiChatView>,
        code: &str,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_file()) else {
            chat.update(cx, |c, cx| {
                c.note("Open a file to put the code into".into(), cx)
            });
            return;
        };
        let caret = editor.read(cx).state.carets.primary();
        let (from, to) = (caret.anchor.min(caret.head), caret.anchor.max(caret.head));
        if replace && from == to {
            chat.update(cx, |c, cx| {
                c.note("Select the code to replace in the editor first".into(), cx)
            });
            return;
        }
        let at = if replace {
            from..to
        } else {
            caret.head..caret.head
        };
        editor.update(cx, |e, cx| e.replace_ranges(&[(at, code.to_owned())], cx));
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let done = if replace {
            "Replaced the selection in"
        } else {
            "Inserted into"
        };
        chat.update(cx, |c, cx| {
            c.inform(format!("{done} {rel}; Undo takes it back"), cx)
        });
        self.focus_editor(window, cx);
        cx.notify();
    }

    /// The preview of applying code block `block` to the active file: its selection when there
    /// is one, else the whole file (`SPEC-0138` v4 behaviour 8). Nothing changes yet.
    fn ai_preview_apply(
        &mut self,
        chat: &Entity<crate::AiChatView>,
        block: usize,
        code: &str,
        cx: &mut Context<Self>,
    ) {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_file()) else {
            chat.update(cx, |c, cx| {
                c.note("Open the file to apply the code to".into(), cx)
            });
            return;
        };
        let e = editor.read(cx);
        let before = e.state.buffer.text();
        let caret = e.state.carets.primary();
        let (from, to) = (caret.anchor.min(caret.head), caret.anchor.max(caret.head));
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let mut replacement = code.to_owned();
        let (range, label) = if from < to {
            (from..to, format!("{rel} (selection)"))
        } else {
            // A whole file keeps its last line break.
            if before.ends_with('\n') && !replacement.ends_with('\n') {
                replacement.push('\n');
            }
            (0..before.chars().count(), rel)
        };
        let preview = crate::ai_chat_view::ApplyPreview {
            path,
            label,
            before,
            range,
            replacement,
        };
        chat.update(cx, |c, cx| c.show_apply_preview(block, preview, cx));
    }

    /// Makes the change confirmed in the preview as one undo step; refused when the file changed
    /// since the preview was made (`SPEC-0138` v4 behaviour 8).
    fn ai_apply(
        &mut self,
        chat: &Entity<crate::AiChatView>,
        preview: &crate::ai_chat_view::ApplyPreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editors.get(&preview.path).cloned() else {
            chat.update(cx, |c, cx| c.note("The file is no longer open".into(), cx));
            return;
        };
        if editor.read(cx).state.buffer.text() != preview.before {
            chat.update(cx, |c, cx| {
                c.note(
                    "The file changed since the preview; press Apply again to see the new diff"
                        .into(),
                    cx,
                )
            });
            return;
        }
        editor.update(cx, |e, cx| {
            e.replace_ranges(&[(preview.range.clone(), preview.replacement.clone())], cx)
        });
        let label = preview.label.clone();
        chat.update(cx, |c, cx| {
            c.inform(format!("Applied to {label}; Undo takes it back"), cx)
        });
        let path = preview.path.clone();
        self.open(&path, window, cx);
        cx.notify();
    }

    /// What "+" in the chat attaches (`SPEC-0138` v4 behaviour 9); what cannot be attached is
    /// said in the chat, where the person looks.
    fn ai_attach_kind(
        &mut self,
        chat: &Entity<crate::AiChatView>,
        kind: crate::ai_chat_view::AttachKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::ai_chat_view::AttachKind;
        match kind {
            AttachKind::File | AttachKind::Selection => {
                let whole = kind == AttachKind::File;
                match self.ai_attachment_of(Some(whole), cx) {
                    Some(a) => chat.update(cx, |c, cx| c.attach(a, cx)),
                    None => chat.update(cx, |c, cx| {
                        c.note(
                            if whole {
                                "Open a file to attach it".into()
                            } else {
                                "Select code in the editor to attach it".into()
                            },
                            cx,
                        )
                    }),
                }
                window.focus(&chat.read(cx).focus(cx));
            }
            AttachKind::Problems | AttachKind::Terminal | AttachKind::GitDiff => {
                self.status = None;
                match kind {
                    AttachKind::Problems => self.ai_attach_problems(window, cx),
                    AttachKind::Terminal => {
                        self.ai_attach_terminal(Source::TerminalCommand, false, window, cx)
                    }
                    _ => self.ai_attach_git_diff(window, cx),
                }
                if let Some(note) = self.status.take() {
                    chat.update(cx, |c, cx| c.note(note, cx));
                }
            }
        }
        cx.notify();
    }

    /// Shows the chat with `attachment` as a chip (from `terminal` when given) and, when given,
    /// `question` in the field, not sent (`SPEC-0313` behaviours 1, 4).
    fn ai_show_with(
        &mut self,
        attachment: Option<Attachment>,
        terminal: Option<Entity<TerminalView>>,
        question: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.ensure_ai_chat(window, cx);
        view.update(cx, |v, cx| {
            match (attachment, &terminal) {
                (Some(a), Some(t)) => v.attach_from_terminal(a, t, cx),
                (Some(a), None) => v.attach(a, cx),
                _ => {}
            }
            if let Some(q) = question {
                v.prefill(q, cx);
            }
        });
        self.show_ai_chat = true;
        window.focus(&view.read(cx).focus(cx));
        cx.notify();
    }

    fn ai_enabled(&self, cx: &App) -> bool {
        crate::appearance::ide_settings(cx).ai.enabled
    }

    /// Nothing is read while the assistant is off (`SPEC-0313` behaviour 10).
    fn ai_refuse_when_off(&mut self, cx: &mut Context<Self>) -> bool {
        if self.ai_enabled(cx) {
            return false;
        }
        self.status =
            Some("The AI Assistant is off; switch it on in Settings › Tools › AI Assistant".into());
        cx.notify();
        true
    }

    fn ai_limits(cx: &App) -> (usize, usize) {
        let ai = crate::appearance::ide_settings(cx).ai;
        (
            ai_context::terminal_lines(ai.terminal_lines),
            ai_context::attachment_chars(ai.attachment_chars),
        )
    }

    /// Attaches the active terminal's selection, last command or last lines to the chat; with
    /// `explain`, the selection else the last command and the question to explain it
    /// (`SPEC-0313` behaviours 2, 4).
    pub fn ai_attach_terminal(
        &mut self,
        kind: Source,
        explain: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai_refuse_when_off(cx) {
            return;
        }
        let Some(view) = self.terminal() else {
            self.status = Some("No terminal tab is open".into());
            cx.notify();
            return;
        };
        let (lines, budget) = Self::ai_limits(cx);
        let a = {
            let v = view.read(cx);
            if explain {
                v.ai_attachment(Source::TerminalSelection, lines, budget)
                    .or_else(|| v.ai_attachment(Source::TerminalCommand, lines, budget))
            } else {
                v.ai_attachment(kind, lines, budget)
            }
        };
        let Some(a) = a else {
            self.status = Some("The terminal shows nothing to attach".into());
            cx.notify();
            return;
        };
        let question = explain.then_some(ai_context::EXPLAIN_OUTPUT);
        self.ai_show_with(Some(a), Some(view), question, window, cx);
    }

    /// Shares the active terminal tab with the chat, or stops sharing it (`SPEC-0313`
    /// behaviour 5).
    pub fn ai_share_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.terminal() else {
            return;
        };
        let on = !view.read(cx).shared();
        if on && self.ai_refuse_when_off(cx) {
            return;
        }
        let chat = self.ensure_ai_chat(window, cx);
        chat.update(cx, |c, cx| c.share(&view, on, cx));
        if on {
            self.show_ai_chat = true;
        }
        self.status = Some(if on {
            "This tab is shared with the AI chat until you stop sharing it".into()
        } else {
            "This tab is no longer shared with the AI chat".into()
        });
        cx.notify();
    }

    /// Names of the terminal tabs shared with the chat (the tab marker, tests).
    pub fn shared_terminal_tabs(&self, cx: &App) -> Vec<String> {
        self.terminals
            .iter()
            .filter(|t| t.panes.iter().any(|p| p.view.read(cx).shared()))
            .map(|t| t.name.clone())
            .collect()
    }

    /// Attaches the Run console's output; with `explain`, also the question (`SPEC-0313`
    /// behaviours 3, 4).
    pub fn ai_attach_run(&mut self, explain: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_refuse_when_off(cx) {
            return;
        }
        let Some((name, text)) = self
            .run_view
            .as_ref()
            .and_then(|v| v.read(cx).selected_output())
        else {
            self.status = Some("The Run tool window shows no output".into());
            cx.notify();
            return;
        };
        let (_, budget) = Self::ai_limits(cx);
        let a = ai_context::attachment(
            Source::Run,
            format!("Run \"{name}\""),
            "text",
            &text,
            budget,
        );
        let question = explain.then_some(ai_context::EXPLAIN_OUTPUT);
        self.ai_show_with(Some(a), None, question, window, cx);
    }

    /// Attaches the selected failed test; with `explain`, also the question (`SPEC-0313`
    /// behaviours 3, 4).
    pub fn ai_attach_failed_test(
        &mut self,
        explain: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai_refuse_when_off(cx) {
            return;
        }
        let Some(test) = self
            .run_view
            .as_ref()
            .and_then(|v| v.read(cx).selected_failed_test())
        else {
            self.status = Some("Select a failed test first".into());
            cx.notify();
            return;
        };
        let (_, budget) = Self::ai_limits(cx);
        let a = ai_context::attachment(
            Source::Test,
            format!("Test {}", test.name),
            "text",
            &test.text(),
            budget,
        );
        let question = explain.then_some(ai_context::EXPLAIN_TEST);
        self.ai_show_with(Some(a), None, question, window, cx);
    }

    /// Attaches the selected problem, else every problem listed (`SPEC-0313` behaviour 3).
    pub fn ai_attach_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_refuse_when_off(cx) {
            return;
        }
        let list = self.problems_list(cx);
        let rows = crate::problems_model::rows(&list, self.problems_scope);
        let chosen: Vec<crate::problems_model::Located> =
            match self.problems_selected.and_then(|i| rows.get(i)) {
                Some(crate::problems_model::Row::Problem(p)) => vec![p.clone()],
                _ => list,
            };
        if chosen.is_empty() {
            self.status = Some("No problems to attach".into());
            cx.notify();
            return;
        }
        let label = match chosen.as_slice() {
            [one] => format!(
                "Problems {}",
                one.path
                    .strip_prefix(&self.root)
                    .unwrap_or(&one.path)
                    .display()
            ),
            many => {
                let first = &many[0].path;
                if many.iter().all(|p| &p.path == first) {
                    format!(
                        "Problems {} · {}",
                        first.strip_prefix(&self.root).unwrap_or(first).display(),
                        many.len()
                    )
                } else {
                    format!("Problems · {}", many.len())
                }
            }
        };
        let (_, budget) = Self::ai_limits(cx);
        let text = crate::ai_context_model::problems_text(&chosen, &self.root);
        let a = ai_context::attachment(Source::Problems, label, "text", &text, budget);
        self.ai_show_with(Some(a), None, None, window, cx);
    }

    /// Attaches the working tree's diff against `HEAD` and the untracked files (`SPEC-0313`
    /// behaviour 3).
    pub fn ai_attach_git_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_refuse_when_off(cx) {
            return;
        }
        let Some((text, files)) = crate::ai_context_model::git_diff(&self.root) else {
            self.status = Some("The project is not a Git repository".into());
            cx.notify();
            return;
        };
        if text.is_empty() {
            self.status = Some("No changes against HEAD".into());
            cx.notify();
            return;
        }
        let (_, budget) = Self::ai_limits(cx);
        let label = format!(
            "Git diff · {files} file{}",
            if files == 1 { "" } else { "s" }
        );
        let a = ai_context::attachment(Source::GitDiff, label, "diff", &text, budget);
        self.ai_show_with(Some(a), None, None, window, cx);
    }

    /// Types `command` into a terminal without pressing Enter: the shared tab, else the one of
    /// the newest terminal chip, else the active tab, else a new one (`SPEC-0313` behaviour 8).
    pub fn ai_insert_command(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .ai_chat
            .as_ref()
            .and_then(|c| c.read(cx).command_target())
            .or_else(|| self.terminal());
        let view = match target {
            Some(v) => v,
            None => {
                self.restore_terminals(window, cx);
                if self.terminals.is_empty() {
                    self.add_terminal_tab(None, None, window, cx);
                }
                match self.terminal() {
                    Some(v) => v,
                    None => return,
                }
            }
        };
        let bracketed = view.read(cx).bracketed_paste();
        match ai_context::insertion(command, bracketed) {
            Ok(text) => {
                if let Some((tab, pane)) = self.locate_terminal(&view) {
                    self.terminal_tab = tab;
                    self.terminals[tab].active = pane;
                }
                view.update(cx, |v, _| v.paste(&text));
                self.show_terminal = true;
                self.focus_terminal(window, cx);
                self.status = Some("Inserted into the terminal; press Enter to run it".into());
            }
            Err(e) => {
                if let Some(chat) = &self.ai_chat {
                    chat.update(cx, |c, cx| c.note(e.to_string(), cx));
                }
            }
        }
        cx.notify();
    }

    /// Names every shell for the registry readers use (`SPEC-0313` behaviour 9): the tab name, and
    /// `name · n` for the further shells of a split tab.
    pub(super) fn sync_terminal_labels(&self, cx: &App) {
        for tab in &self.terminals {
            for (k, pane) in tab.panes.iter().enumerate() {
                let name = if k == 0 {
                    tab.name.clone()
                } else {
                    format!("{} · {}", tab.name, k + 1)
                };
                pane.view.read(cx).set_label(&self.root, &name);
            }
        }
    }

    /// Carries out an AI item of the terminal's or the Run console's context menu or of the
    /// menus (`SPEC-0313`); false when `action` is none of them.
    pub(super) fn ai_item(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            model::AI_EXPLAIN_TERMINAL => {
                self.ai_attach_terminal(Source::TerminalCommand, true, window, cx)
            }
            model::AI_ATTACH_SELECTION => {
                self.ai_attach_terminal(Source::TerminalSelection, false, window, cx)
            }
            model::AI_ATTACH_COMMAND => {
                self.ai_attach_terminal(Source::TerminalCommand, false, window, cx)
            }
            model::AI_ATTACH_LINES => {
                self.ai_attach_terminal(Source::TerminalLines, false, window, cx)
            }
            model::AI_SHARE_TERMINAL => self.ai_share_terminal(window, cx),
            model::AI_EXPLAIN_RUN => self.ai_attach_run(true, window, cx),
            model::AI_ATTACH_RUN => self.ai_attach_run(false, window, cx),
            model::AI_EXPLAIN_TEST => self.ai_attach_failed_test(true, window, cx),
            model::AI_ATTACH_TEST => self.ai_attach_failed_test(false, window, cx),
            model::AI_ATTACH_PROBLEMS => self.ai_attach_problems(window, cx),
            model::AI_ATTACH_DIFF => self.ai_attach_git_diff(window, cx),
            _ => return false,
        }
        true
    }

    /// Opens the context menu of the terminal tab shown (the self-test; a secondary click does
    /// the same).
    pub fn open_terminal_menu(&mut self, cx: &mut Context<Self>) {
        self.open_context_menu(
            super::MenuPlace::Terminal,
            None,
            gpui::point(gpui::px(200.), gpui::px(200.)),
            cx,
        );
    }

    /// Opens the context menu of the Run tool window (the self-test).
    pub fn open_run_menu(&mut self, cx: &mut Context<Self>) {
        self.open_context_menu(
            super::MenuPlace::Run,
            None,
            gpui::point(gpui::px(200.), gpui::px(200.)),
            cx,
        );
    }
}
