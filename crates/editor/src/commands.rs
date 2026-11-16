//! Editor commands by keymap action id (`SPEC-0057`).
//!
//! The editor view, agents and tests all press commands through [`perform`]; the view only turns key
//! events into keystrokes and looks them up in the keymap.

use std::ops::Range;

use crate::actions::{
    self, CommentStyle, NewlineOptions, apply, line_of, line_start, line_text, map_offset,
};
use crate::buffer::TextBuffer;
use crate::selection::{Caret, Carets, Motion, is_word};
use crate::small::{self, LineOrder};
use crate::smart;

/// Text held by the editor clipboard (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    /// Copied text.
    pub text: String,
    /// Copied as whole lines (copy or cut without a selection).
    pub whole_lines: bool,
}

/// Visible part of the document, for page commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// First visible line (0-based).
    pub first_line: usize,
    /// Number of visible lines.
    pub lines: usize,
}

/// Everything a command works on (behaviour 1).
#[derive(Debug, Clone)]
pub struct EditorState {
    /// The document.
    pub buffer: TextBuffer,
    /// Its carets.
    pub carets: Carets,
    /// Editor clipboard.
    pub clipboard: Option<Clipboard>,
    /// Indentation unit, e.g. four spaces or a tab.
    pub indent: String,
    /// Columns a tab advances to, for Convert Indents (`SPEC-0332` behaviour 11).
    pub tab_width: usize,
    /// What the last command has to say in the status bar, if anything (`SPEC-0332` behaviours 3
    /// and 12); every command starts without one.
    pub message: Option<String>,
    /// Line comment token of the language, if it has one.
    pub comment: Option<String>,
    /// Block comment tokens of the language, if it has them (`SPEC-0056` v3 behaviour 16).
    pub block_comment: Option<(String, String)>,
    /// Language id of the file (`java`, `python`, `markdown`, …) for the language-aware commands of
    /// v3; `None` for plain text.
    pub language: Option<String>,
    /// Syntax ranges around the primary caret for `EditorSelectWord` (v3 behaviour 13), set by the
    /// caller before the command; empty without a syntax tree.
    pub structure: Vec<Range<usize>>,
    /// Char offsets of the names of the file's method and function declarations, sorted, for
    /// `MethodUp` and `MethodDown` (`SPEC-0071` v9 behaviour 12); set by the caller before the
    /// command, empty without a syntax tree.
    pub methods: Vec<usize>,
    /// For every caret, in the order of `carets`, the elements of the comma-separated list around
    /// its head, for `MoveElementLeft` and `MoveElementRight` (`SPEC-0056` v4 behaviour 19); set
    /// by the caller before the command.
    pub lists: Vec<Vec<Range<usize>>>,
    /// Visible lines.
    pub viewport: Viewport,
    /// The words of the other open files for `HippieCompletion` and `HippieBackwardCompletion`, in
    /// the order they are offered after the text's own (v5 behaviour 23); set by the caller before
    /// the command, needed only when [`expand_word_continues`] is false.
    pub other_words: Vec<String>,
    /// The running cycle of Cyclic Expand Word (v5 behaviour 23).
    expand: Option<ExpandCycle>,
    /// The undo step the walk through the edit locations started at and the place it is on
    /// (`SPEC-0071` v9 behaviour 13).
    change_walk: Option<(Option<u64>, usize)>,
    /// Selections before each `EditorSelectWord`, for `EditorUnSelectWord`.
    select_history: Vec<Carets>,
    /// The undo step the last typing produced and the carets after it; typing that continues right
    /// there joins that step (behaviour 12).
    typed: Option<(u64, Carets)>,
    /// Like `typed` for Backspace and Delete pressed one after another (v3 behaviour 18).
    deleting: Option<(u64, Carets)>,
    /// Overwrite mode: typed text replaces the characters after the caret up to the line end
    /// (v6 behaviour 25). A state of this editor, never saved.
    pub overwrite: bool,
    /// The row of cuts `EditorCutLineEnd` joins on the clipboard (v6 behaviour 24).
    cut_row: Option<CutRow>,
}

/// Where a row of `EditorCutLineEnd` stands (v6 behaviour 24): the undo step its last cut made,
/// the caret's place and what the clipboard holds of the row.
#[derive(Debug, Clone)]
struct CutRow {
    step: Option<u64>,
    at: usize,
    text: String,
}

impl EditorState {
    /// State for `text` with one caret at the start, four-space indentation and a 40-line viewport.
    pub fn new(text: &str) -> Self {
        Self {
            buffer: TextBuffer::from_text(text),
            carets: Carets::new(0),
            clipboard: None,
            indent: "    ".into(),
            tab_width: 4,
            message: None,
            comment: None,
            block_comment: None,
            language: None,
            structure: Vec::new(),
            methods: Vec::new(),
            lists: Vec::new(),
            viewport: Viewport {
                first_line: 0,
                lines: 40,
            },
            other_words: Vec::new(),
            expand: None,
            change_walk: None,
            select_history: Vec::new(),
            typed: None,
            deleting: None,
            overwrite: false,
            cut_row: None,
        }
    }

    /// State for `text` with one caret at the start that keeps this state's settings of the file:
    /// indentation unit, tab width, comment tokens and language. Replacing the whole text of an
    /// open file goes through this, so Tab, the status bar's indent label and Convert Indents go
    /// on following its `.editorconfig` and Code Style (`SPEC-0137` behaviour 2, `SPEC-0150`
    /// behaviour 2) instead of falling back to four spaces.
    pub fn with_text(&self, text: &str) -> Self {
        Self {
            indent: self.indent.clone(),
            tab_width: self.tab_width,
            comment: self.comment.clone(),
            block_comment: self.block_comment.clone(),
            language: self.language.clone(),
            ..Self::new(text)
        }
    }

    /// Ends the row of cuts `EditorCutLineEnd` joins on the clipboard (v6 behaviour 24): the
    /// editor lost the focus, so the clipboard may hold another text by the next cut.
    pub fn end_cut_row(&mut self) {
        self.cut_row = None;
    }
}

/// Every action id [`perform`] handles (behaviour 10).
pub const COMMANDS: &[&str] = &[
    "EditorLeft",
    "EditorRight",
    "EditorUp",
    "EditorDown",
    "EditorPreviousWord",
    "EditorNextWord",
    "EditorLineStart",
    "EditorLineEnd",
    "EditorTextStart",
    "EditorTextEnd",
    "EditorPageUp",
    "EditorPageDown",
    "EditorMoveToPageTop",
    "EditorMoveToPageBottom",
    "EditorCodeBlockStart",
    "EditorCodeBlockEnd",
    "EditorLeftWithSelection",
    "EditorRightWithSelection",
    "EditorUpWithSelection",
    "EditorDownWithSelection",
    "EditorPreviousWordWithSelection",
    "EditorNextWordWithSelection",
    "EditorLineStartWithSelection",
    "EditorLineEndWithSelection",
    "EditorTextStartWithSelection",
    "EditorTextEndWithSelection",
    "EditorPageUpWithSelection",
    "EditorPageDownWithSelection",
    "EditorMoveToPageTopWithSelection",
    "EditorMoveToPageBottomWithSelection",
    "EditorCodeBlockStartWithSelection",
    "EditorCodeBlockEndWithSelection",
    "EditorBackSpace",
    "$Delete",
    "EditorDeleteToWordStart",
    "EditorDeleteToWordEnd",
    "EditorDeleteLine",
    "EditorEnter",
    "EditorStartNewLine",
    "EditorStartNewLineBefore",
    "EditorSplitLine",
    // SPEC-0056 v5 behaviour 20.
    "EditorCompleteStatement",
    "EditorDuplicate",
    "EditorJoinLines",
    "MoveLineUp",
    "MoveLineDown",
    "MoveStatementUp",
    "MoveStatementDown",
    "CommentByLineComment",
    "CommentByBlockComment",
    "EditorTab",
    "EditorIndentSelection",
    "EditorUnindentSelection",
    "$SelectAll",
    "EditorSelectWord",
    "EditorUnSelectWord",
    "EditorEscape",
    "SelectNextOccurrence",
    "UnselectPreviousOccurrence",
    "SelectAllOccurrences",
    "EditorCloneCaretAbove",
    "EditorCloneCaretBelow",
    "$Copy",
    "$Cut",
    "$Paste",
    "EditorPasteSimple",
    "EditorToggleCase",
    "$Undo",
    "$Redo",
    "EditorMatchBrace",
    "EditorScrollToCenter",
    "MethodUp",
    "MethodDown",
    "JumpToLastChange",
    "JumpToNextChange",
    "MoveElementLeft",
    "MoveElementRight",
    "HippieCompletion",
    "HippieBackwardCompletion",
    // SPEC-0332.
    "EditorSortLines",
    "EditorReverseLines",
    "EditorTranspose",
    "EditorAddCaretPerSelectedLine",
    "ConvertIndentsToSpaces",
    "ConvertIndentsToTabs",
    // SPEC-0057 v6.
    "EditorCutLineEnd",
    "EditorToggleInsertState",
    "AutoIndentLines",
];

/// Carries out `action` on `state`. Returns `false` (and changes nothing) for an action id that is
/// not an editor command (behaviour 10).
pub fn perform(state: &mut EditorState, action: &str) -> bool {
    if matches!(action, "$Undo" | "$Redo") {
        return perform_command(state, action);
    }
    let before = (state.buffer.top_step(), state.carets.clone());
    let handled = perform_command(state, action);
    note_carets(state, before);
    handled
}

/// Notes on a step made since `before` where the carets were then and where they are now, so undo
/// and redo put them back (v7 behaviour 9).
fn note_carets(state: &mut EditorState, before: (Option<u64>, Carets)) {
    // Step ids only grow: an older step on top means the command took one back and made none.
    let made = match (state.buffer.top_step(), before.0) {
        (Some(now), Some(was)) => now > was,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if made {
        state.buffer.note_carets(&before.1, &state.carets);
    }
}

fn perform_command(state: &mut EditorState, action: &str) -> bool {
    state.typed = None;
    state.message = None;
    // Any other command ends a row of cuts to the line end (v6 behaviour 24).
    let cut_row = state.cut_row.take();
    // Any other command ends a cycle of Cyclic Expand Word (v5 behaviour 23).
    let expanding = state.expand.take();
    if matches!(action, "HippieCompletion" | "HippieBackwardCompletion") {
        state.deleting = None;
        state.select_history.clear();
        expand_word(state, expanding, action == "HippieBackwardCompletion");
        return true;
    }
    let deleting = state.deleting.take();
    // Only occurrence commands keep "add next occurrence" to whole words (`SPEC-0025` v4
    // behaviour 4b).
    if !matches!(
        action,
        "SelectNextOccurrence" | "UnselectPreviousOccurrence"
    ) {
        state.carets.end_occurrence_row();
    }
    // Copying between the steps keeps the chain of `EditorUnSelectWord` (v3 behaviour 13).
    if !matches!(action, "EditorSelectWord" | "EditorUnSelectWord" | "$Copy") {
        state.select_history.clear();
    }
    let braces = smart::has_braces(state.language.as_deref());
    let (base, extend) = match action.strip_suffix("WithSelection") {
        Some(base) => (base, true),
        None => (action, false),
    };
    let motion = match base {
        "EditorLeft" => Some(Motion::Left),
        "EditorRight" => Some(Motion::Right),
        "EditorUp" => Some(Motion::Up),
        "EditorDown" => Some(Motion::Down),
        "EditorPreviousWord" => Some(Motion::WordLeft),
        "EditorNextWord" => Some(Motion::WordRight),
        "EditorLineEnd" => Some(Motion::LineEnd),
        "EditorTextStart" => Some(Motion::DocumentStart),
        "EditorTextEnd" => Some(Motion::DocumentEnd),
        _ => None,
    };
    if let Some(m) = motion {
        state.carets.move_all(&state.buffer, m, extend);
        return true;
    }
    let (buf, carets) = (&mut state.buffer, &mut state.carets);
    match base {
        "EditorLineStart" => smart_home(buf, carets, extend),
        "EditorPageUp" | "EditorPageDown" => {
            let down = base == "EditorPageDown";
            let n = state.viewport.lines.max(1);
            for _ in 0..n {
                carets.move_all(buf, if down { Motion::Down } else { Motion::Up }, extend);
            }
            let max_first = buf.len_lines().saturating_sub(1);
            state.viewport.first_line = if down {
                (state.viewport.first_line + n).min(max_first)
            } else {
                state.viewport.first_line.saturating_sub(n)
            };
        }
        "EditorMoveToPageTop" | "EditorMoveToPageBottom" => {
            let v = state.viewport;
            let last = buf.len_lines().saturating_sub(1);
            let line = if base == "EditorMoveToPageTop" {
                v.first_line.min(last)
            } else {
                (v.first_line + v.lines.max(1) - 1).min(last)
            };
            let b = &*buf;
            set_heads(carets, extend, |c| {
                let column = c.head - line_start(b, line_of(b, c.head));
                line_start(b, line) + column.min(line_text(b, line).chars().count())
            });
        }
        "EditorCodeBlockStart" | "EditorCodeBlockEnd" => {
            let start = base == "EditorCodeBlockStart";
            let b = &*buf;
            set_heads(carets, extend, |c| {
                enclosing_bracket(b, c.head, start).unwrap_or(c.head)
            });
        }
        _ if extend => return false,
        "EditorBackSpace" | "$Delete" => {
            // Deleting one after another is one undo step (v3 behaviour 18).
            let before = buf.top_step();
            let continues =
                matches!(&deleting, Some((id, c)) if Some(*id) == before && c == &*carets);
            if base == "EditorBackSpace" {
                backspace(buf, carets, &state.indent, braces);
            } else {
                carets.delete(buf);
            }
            let after = buf.top_step();
            if after != before && continues {
                buf.join_last_steps();
            }
            state.deleting = buf
                .top_step()
                .filter(|_| after != before || continues)
                .map(|id| (id, carets.clone()));
        }
        "EditorDeleteToWordStart" | "EditorDeleteToWordEnd" => {
            if carets.carets().iter().all(Caret::is_empty) {
                let m = if base == "EditorDeleteToWordStart" {
                    Motion::WordLeft
                } else {
                    Motion::WordRight
                };
                carets.move_all(buf, m, true);
            }
            carets.type_text(buf, "");
        }
        "EditorDeleteLine" => actions::delete_lines(buf, carets),
        "EditorEnter" => {
            let close = if braces {
                closes_on_enter(buf, carets)
            } else {
                None
            };
            actions::smart_newline(buf, carets, &newline(&state.indent));
            if let Some(indent) = close {
                // `{` gets its `}` on Enter (SPEC-0056 v3 behaviour 18), in the same undo step.
                let at = carets.primary().head;
                let nl = buf.line_ending().as_str();
                apply(buf, &[(at..at, format!("{nl}{indent}}}"))]);
                buf.join_last_steps();
                *carets = Carets::new(at);
            }
        }
        "EditorStartNewLine" => {
            carets.move_all(buf, Motion::LineEnd, false);
            actions::smart_newline(buf, carets, &newline(&state.indent));
        }
        "EditorStartNewLineBefore" => new_line_before(buf, carets),
        // SPEC-0056 v5 behaviour 20: the primary caret's line, in one undo step.
        "EditorCompleteStatement" => {
            let line = line_of(buf, carets.primary().head);
            let mut lines: Vec<String> = buf.text().lines().map(str::to_owned).collect();
            if lines.len() <= line {
                lines.resize(line + 1, String::new());
            }
            if let Some(done) = crate::statement::complete_statement(
                &lines,
                line,
                state.language.as_deref(),
                &state.indent,
            ) {
                let start = line_start(buf, line);
                let end = start + lines[line].chars().count();
                let nl = buf.line_ending().as_str();
                let caret = start
                    + done.lines[..done.caret.0]
                        .iter()
                        .map(|l| l.chars().count() + nl.chars().count())
                        .sum::<usize>()
                    + done.caret.1;
                apply(buf, &[(start..end, done.lines.join(nl))]);
                *carets = Carets::new(caret);
            }
        }
        "EditorSplitLine" => {
            actions::smart_newline(buf, carets, &newline(&state.indent));
            carets.move_all(buf, Motion::Up, false);
            carets.move_all(buf, Motion::LineEnd, false);
        }
        "EditorDuplicate" => {
            let one = carets.carets().len() == 1 && carets.primary().is_empty();
            let line = line_of(buf, carets.primary().head);
            actions::duplicate(buf, carets);
            // A duplicated Java declaration puts the caret on the name (SPEC-0056 v3 behaviour 12).
            if one
                && state.language.as_deref() == Some("java")
                && let Some(column) = smart::declared_name_column(&line_text(buf, line + 1))
            {
                *carets = Carets::new(line_start(buf, line + 1) + column);
            }
        }
        "EditorJoinLines" => {
            let one = carets.carets().len() == 1 && carets.primary().is_empty();
            let line = line_of(buf, carets.primary().head);
            match smart::unwrap_one_line_block(buf, line).filter(|_| one && braces) {
                Some((range, text)) => {
                    let at = range.start + 1;
                    apply(buf, &[(range, text)]);
                    *carets = Carets::new(at);
                }
                None => actions::join_lines(buf, carets),
            }
        }
        "MoveLineUp" => actions::move_lines(buf, carets, true),
        "MoveLineDown" => actions::move_lines(buf, carets, false),
        "MoveStatementUp" | "MoveStatementDown" => {
            let up = base == "MoveStatementUp";
            if braces && carets.carets().len() == 1 {
                smart::move_statement(buf, carets, up, &state.indent);
            } else {
                actions::move_lines(buf, carets, up);
            }
        }
        "CommentByLineComment" => {
            let single = carets.carets().len() == 1 && carets.primary().is_empty();
            let line = line_of(buf, carets.primary().head);
            let blank = line_text(buf, line).trim().is_empty();
            match (&state.comment, &state.block_comment) {
                (Some(token), _) => {
                    actions::toggle_line_comment(buf, carets, &CommentStyle::new(token));
                    // On a blank line the caret stays after the token, elsewhere it goes one line
                    // down (SPEC-0056 v3 behaviour 15).
                    if single && blank {
                        let text = line_text(buf, line);
                        let at = text.find(token.as_str()).map(|i| text[..i].chars().count());
                        if let Some(at) = at {
                            *carets =
                                Carets::new(line_start(buf, line) + at + token.chars().count());
                        }
                    } else if single {
                        carets.move_all(buf, Motion::Down, false);
                    }
                }
                (None, Some((open, close))) if open == "<!--" => {
                    smart::toggle_wrapped_lines(buf, carets, open, close);
                    if single && !blank {
                        carets.move_all(buf, Motion::Down, false);
                    }
                }
                _ => return false,
            }
        }
        "CommentByBlockComment" => match &state.block_comment {
            Some((open, close)) => smart::toggle_block_comment(buf, carets, open, close),
            None => return false,
        },
        "EditorTab" => tab(buf, carets, &state.indent),
        // Without a selection Tab inserts at the caret (v3 behaviour 17).
        "EditorIndentSelection" if carets.carets().iter().all(Caret::is_empty) => return false,
        "EditorIndentSelection" => actions::indent_lines(buf, carets, &state.indent, false),
        "EditorUnindentSelection" => actions::indent_lines(buf, carets, &state.indent, true),
        "$SelectAll" => *carets = Carets::from_carets(vec![Caret::select(0, buf.len_chars())]),
        "EditorSelectWord" => {
            let before = carets.clone();
            let structure = &state.structure;
            let grown = Carets::from_carets(
                carets
                    .carets()
                    .iter()
                    .map(|c| {
                        let r = grow(buf, c.range(), structure);
                        Caret::select(r.start, r.end)
                    })
                    .collect(),
            );
            if grown != before {
                state.select_history.push(before);
                *carets = grown;
            }
        }
        "EditorUnSelectWord" => {
            if let Some(previous) = state.select_history.pop() {
                *carets = previous;
            }
        }
        "EditorEscape" => *carets = Carets::new(carets.primary().head),
        "SelectNextOccurrence" => {
            carets.add_next_occurrence(buf);
        }
        "UnselectPreviousOccurrence" => {
            carets.remove_last_added();
        }
        "SelectAllOccurrences" => carets.select_all_occurrences(buf),
        "EditorCloneCaretAbove" => carets.add_vertical(buf, false),
        "EditorCloneCaretBelow" => carets.add_vertical(buf, true),
        "$Copy" => state.clipboard = Some(copy(buf, carets)),
        "$Cut" => {
            let clip = copy(buf, carets);
            if clip.whole_lines {
                actions::delete_lines(buf, carets);
            } else {
                carets.type_text(buf, "");
            }
            state.clipboard = Some(clip);
        }
        "$Paste" | "EditorPasteSimple" => {
            if let Some(clip) = state.clipboard.clone() {
                // Source files re-indent pasted lines (v3 behaviour 19); plain text, Markdown and
                // `EditorPasteSimple` do not.
                let indent = (base == "$Paste")
                    .then_some(state.language.as_deref())
                    .flatten()
                    .filter(|l| !matches!(*l, "markdown" | "plaintext"))
                    .map(|_| (state.indent.as_str(), braces));
                // Plain text has no formatter: the pasted block's further lines are indented to
                // the caret's column (`SPEC-0025` v4 behaviour 6a).
                let block = (base == "$Paste"
                    && matches!(state.language.as_deref(), None | Some("plaintext")))
                .then_some(state.tab_width);
                paste(buf, carets, &clip, indent, block);
            }
        }
        "EditorToggleCase" => toggle_case(buf, carets),
        // SPEC-0057 v6 behaviour 24.
        "EditorCutLineEnd" => {
            state.cut_row = cut_line_end(buf, carets, &mut state.clipboard, cut_row);
        }
        // SPEC-0057 v6 behaviour 25.
        "EditorToggleInsertState" => state.overwrite = !state.overwrite,
        // SPEC-0057 v6 behaviour 26.
        "AutoIndentLines" => {
            if braces {
                auto_indent_lines(buf, carets, &state.indent);
            } else {
                state.message = Some(NO_INDENT_RULE.to_owned());
            }
        }
        // SPEC-0332 behaviours 3–5.
        "EditorSortLines" | "EditorReverseLines" => {
            let (order, title) = if base == "EditorSortLines" {
                (LineOrder::Sorted, "Sort Lines")
            } else {
                (LineOrder::Reversed, "Reverse Lines")
            };
            if !small::permute_lines(buf, carets, order) {
                state.message = Some(format!("{title}: fewer than two lines"));
            }
        }
        // SPEC-0332 behaviours 6–8.
        "EditorTranspose" => small::transpose(buf, carets),
        // SPEC-0332 behaviour 9.
        "EditorAddCaretPerSelectedLine" => small::carets_at_line_ends(buf, carets),
        // SPEC-0332 behaviours 10–12.
        "ConvertIndentsToSpaces" | "ConvertIndentsToTabs" => {
            let tabs = base == "ConvertIndentsToTabs";
            let lines = small::convert_indents(buf, carets, state.tab_width, tabs);
            state.message = Some(small::indents_message(lines, tabs));
        }
        // SPEC-0057 v4 behaviour 21.
        "EditorMatchBrace" => {
            let b = &*buf;
            set_heads(carets, false, |c| match_brace(b, c.head).unwrap_or(c.head));
        }
        // SPEC-0057 v4 behaviour 22.
        "EditorScrollToCenter" => {
            let line = line_of(buf, carets.primary().head);
            state.viewport.first_line =
                line.saturating_sub(state.viewport.lines.saturating_sub(1) / 2);
        }
        // SPEC-0071 v9 behaviour 12.
        "MethodUp" | "MethodDown" => {
            let at = carets.primary().head;
            let target = if base == "MethodUp" {
                state.methods.iter().rev().find(|m| **m < at)
            } else {
                state.methods.iter().find(|m| **m > at)
            };
            if let Some(&to) = target {
                *carets = Carets::new(to.min(buf.len_chars()));
            }
        }
        // SPEC-0071 v9 behaviour 13.
        "JumpToLastChange" | "JumpToNextChange" => {
            let top = buf.top_step();
            let walk = state
                .change_walk
                .filter(|(step, _)| *step == top)
                .map(|(_, i)| i);
            let next = match (base, walk) {
                ("JumpToLastChange", None) => Some(0),
                ("JumpToLastChange", Some(i)) => Some(i + 1),
                (_, Some(i)) => i.checked_sub(1),
                (_, None) => None,
            };
            let places = buf.change_places();
            if let Some(i) = next
                && let Some(&at) = places.get(i)
            {
                *carets = Carets::new(at.min(buf.len_chars()));
                state.change_walk = Some((top, i));
            }
        }
        // SPEC-0056 v4 behaviour 19.
        "MoveElementLeft" | "MoveElementRight" => {
            move_element(buf, carets, &state.lists, base == "MoveElementLeft");
        }
        "$Undo" => {
            if let Some((at, noted)) = buf.undo_carets() {
                *carets = noted.unwrap_or_else(|| Carets::new(at.min(buf.len_chars())));
            }
        }
        "$Redo" => {
            if let Some((at, noted)) = buf.redo_carets() {
                *carets = noted.unwrap_or_else(|| Carets::new(at.min(buf.len_chars())));
            }
        }
        _ => return false,
    }
    true
}

/// A text longer than this offers and takes no words for Cyclic Expand Word (v5 behaviour 23; the
/// limit above which the editor has no syntax tree either, `SPEC-0260` behaviour 4).
pub const EXPAND_WORD_MAX: usize = 8 * 1024 * 1024;

/// A cycle of Cyclic Expand Word (v5 behaviour 23).
#[derive(Debug, Clone)]
struct ExpandCycle {
    /// The cycle of `HippieBackwardCompletion`.
    backward: bool,
    /// How many words have been shown; 0 while the prefixes stand alone.
    shown: usize,
    /// The newest undo step and the carets after the last use: the cycle goes on only from there.
    step: Option<u64>,
    carets: Carets,
    /// The carets before the first use, at the end of their prefixes.
    origin: Carets,
    /// For every caret of `origin`, what each of its words adds to the prefix, in their order.
    rests: Vec<Vec<String>>,
}

/// Whether the next `HippieCompletion` or `HippieBackwardCompletion` goes on with the running
/// cycle, so that [`EditorState::other_words`] is not needed (v5 behaviour 23).
pub fn expand_word_continues(state: &EditorState) -> bool {
    state
        .expand
        .as_ref()
        .is_some_and(|c| c.carets == state.carets && c.step == state.buffer.top_step())
}

/// The words of `text`, runs of letters, digits and `_`, each once in the order of the text: what
/// another open file offers to Cyclic Expand Word (v5 behaviour 23).
pub fn words_of(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.split(|c: char| !is_word(c))
        .filter(|w| !w.is_empty() && seen.insert(*w))
        .map(str::to_owned)
        .collect()
}

/// The word runs of `text` as char ranges, in order.
fn word_spans(text: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if !is_word(text[i]) {
            i += 1;
            continue;
        }
        let from = i;
        while i < text.len() && is_word(text[i]) {
            i += 1;
        }
        out.push(from..i);
    }
    out
}

/// What the words for the prefix before `head` add to it, in the order Cyclic Expand Word offers
/// them (v5 behaviour 23); empty without a prefix.
fn expand_rests(
    text: &[char],
    spans: &[Range<usize>],
    head: usize,
    backward: bool,
    other: &[String],
) -> Vec<String> {
    let mut start = head;
    while start > 0 && is_word(text[start - 1]) {
        start -= 1;
    }
    if start == head {
        return Vec::new();
    }
    let prefix = &text[start..head];
    let fits =
        |s: &&Range<usize>| s.len() > prefix.len() && text[s.start..s.end].starts_with(prefix);
    // The word the caret is in or at starts where the prefix does; it is not offered.
    let own = spans.partition_point(|s| s.start < start);
    let before = spans[..own].iter();
    let after = spans[(own + 1).min(spans.len())..].iter();
    let order: Vec<&Range<usize>> = if backward {
        after.chain(before).filter(fits).collect()
    } else {
        before.rev().chain(after.rev()).filter(fits).collect()
    };
    let typed: String = prefix.iter().collect();
    let mut seen = std::collections::HashSet::new();
    let mut rests = Vec::new();
    let own_words = order.into_iter().map(|s| {
        text[s.start + prefix.len()..s.end]
            .iter()
            .collect::<String>()
    });
    let other_words = other
        .iter()
        .filter(|w| w.len() > typed.len())
        .filter_map(|w| w.strip_prefix(typed.as_str()).map(str::to_owned));
    for rest in own_words.chain(other_words) {
        if seen.insert(rest.clone()) {
            rests.push(rest);
        }
    }
    rests
}

/// `HippieCompletion` and `HippieBackwardCompletion` (v5 behaviour 23): the next word of the cycle
/// that is running from exactly this text and these carets, else the first word of a new one.
fn expand_word(state: &mut EditorState, cycle: Option<ExpandCycle>, backward: bool) {
    let cycle = cycle.filter(|c| c.carets == state.carets && c.step == state.buffer.top_step());
    if let Some(c) = &cycle {
        // What the last use inserted is taken back first, so a whole cycle is one undo step.
        if c.shown > 0 {
            state.buffer.undo();
        }
        state.carets = c.origin.clone();
    }
    let mut cycle = match cycle {
        Some(c) if c.backward == backward => c,
        _ => {
            if !state.carets.carets().iter().all(Caret::is_empty)
                || state.buffer.len_chars() > EXPAND_WORD_MAX
            {
                return;
            }
            let text: Vec<char> = state.buffer.text().chars().collect();
            let spans = word_spans(&text);
            let rests = state
                .carets
                .carets()
                .iter()
                .map(|c| expand_rests(&text, &spans, c.head, backward, &state.other_words))
                .collect();
            ExpandCycle {
                backward,
                shown: 0,
                step: None,
                carets: state.carets.clone(),
                origin: state.carets.clone(),
                rests,
            }
        }
    };
    let most = cycle.rests.iter().map(Vec::len).max().unwrap_or(0);
    if most == 0 {
        return;
    }
    // After the last word the prefixes stand alone once, then the cycle starts over.
    cycle.shown = if cycle.shown >= most {
        0
    } else {
        cycle.shown + 1
    };
    let ranges: Vec<Range<usize>> = cycle
        .origin
        .carets()
        .iter()
        .map(|c| c.head..c.head)
        .collect();
    let texts: Vec<String> = cycle
        .rests
        .iter()
        .map(|rests| {
            cycle
                .shown
                .checked_sub(1)
                .and_then(|i| rests.get(i))
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let top = state.buffer.top_step();
    state
        .carets
        .replace_each(&mut state.buffer, &ranges, &texts);
    // Undo of the cycle puts the carets back at the prefix (v7 behaviour 9).
    if state.buffer.top_step() != top {
        state.buffer.note_carets(&cycle.origin, &state.carets);
    }
    cycle.step = state.buffer.top_step();
    cycle.carets = state.carets.clone();
    state.expand = Some(cycle);
}

/// Orders the actions bound to one keystroke for the focused editor: `Editor…` actions first, the
/// rest in their given order (behaviour 12). On macOS `⌘⌫` is bound to both `$Delete` and
/// `EditorDeleteLine`; in the editor the line is deleted.
pub fn editor_first<'a>(bound: &[&'a str]) -> Vec<&'a str> {
    let (mut first, rest): (Vec<&str>, Vec<&str>) =
        bound.iter().partition(|id| id.starts_with("Editor"));
    first.extend(rest);
    first
}

/// Types `text` at every caret: a single character with bracket and quote handling, a line break as
/// `EditorEnter`, anything else as is.
/// Consecutive typing at the caret where the last typing ended is one undo step, as in the
/// reference; any other command, a caret move or a save starts a new one (behaviour 12).
pub fn type_text(state: &mut EditorState, text: &str) {
    let before = (state.buffer.top_step(), state.carets.clone());
    type_text_command(state, text);
    note_carets(state, before);
}

fn type_text_command(state: &mut EditorState, text: &str) {
    state.select_history.clear();
    state.expand = None;
    state.deleting = None;
    state.cut_row = None;
    let mut chars = text.chars();
    if let (Some('\n'), None) = (chars.next(), chars.next()) {
        perform(state, "EditorEnter");
        return;
    }
    let before = state.buffer.top_step();
    let continues = matches!(&state.typed, Some((id, carets)) if Some(*id) == before && *carets == state.carets);
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        // Overwrite mode closes no pairs and steps over nothing (v6 behaviour 25).
        _ if state.overwrite => overwrite_text(&mut state.buffer, &mut state.carets, text),
        (Some(c), None) => actions::type_char(&mut state.buffer, &mut state.carets, c),
        _ => state.carets.type_text(&mut state.buffer, text),
    }
    let after = state.buffer.top_step();
    if after != before && continues {
        state.buffer.join_last_steps();
    }
    state.typed = match state.buffer.top_step() {
        Some(id) if after != before || continues => Some((id, state.carets.clone())),
        _ => None,
    };
}

/// What the status bar says when `AutoIndentLines` has no rule for the language (v6 behaviour
/// 26).
pub const NO_INDENT_RULE: &str = "Auto-Indent Lines: no indentation rule for this language";

/// Offset after the last character of `line`, before its line break.
fn line_end(buf: &TextBuffer, line: usize) -> usize {
    line_start(buf, line) + line_text(buf, line).chars().count()
}

/// What a caret at `at` cuts with `EditorCutLineEnd` (v6 behaviour 24): up to the end of its
/// line, and the line break too when only spaces and tabs are left before it and a line follows.
pub fn cut_range(buf: &TextBuffer, at: usize) -> Range<usize> {
    let line = line_of(buf, at);
    let end = line_end(buf, line).max(at);
    let blank = (at..end).all(|i| matches!(buf.char_at(i), Some(' ' | '\t')));
    if blank && line + 1 < buf.len_lines() {
        at..line_start(buf, line + 1)
    } else {
        at..end
    }
}

/// `EditorCutLineEnd` (v6 behaviour 24): cuts from every caret to the end of its line and puts
/// the cut text into `clipboard`, joined to the cuts of `row` when this one goes on from there.
/// Returns the row for the next cut; `None` with several carets or when nothing was cut.
fn cut_line_end(
    buf: &mut TextBuffer,
    carets: &mut Carets,
    clipboard: &mut Option<Clipboard>,
    row: Option<CutRow>,
) -> Option<CutRow> {
    let heads: Vec<usize> = carets.carets().iter().map(|c| c.head).collect();
    // The clipboard holds line breaks as `\n` whatever the file uses.
    let taken = |buf: &TextBuffer, range: Range<usize>| {
        buf.slice(range)
            .unwrap_or_default()
            .replace("\r\n", "\n")
            .replace('\r', "\n")
    };
    if let &[at] = heads.as_slice() {
        *carets = Carets::new(at);
        let range = cut_range(buf, at);
        // Nothing to cut leaves the clipboard alone (IMP-0405).
        if range.is_empty() {
            return None;
        }
        let mut text = taken(buf, range.clone());
        let goes_on = row.filter(|r| {
            r.step == buf.top_step()
                && r.at == at
                && clipboard
                    .as_ref()
                    .is_some_and(|c| !c.whole_lines && c.text == r.text)
        });
        if let Some(row) = goes_on {
            text.insert_str(0, &row.text);
        }
        buf.delete(range).ok()?;
        *clipboard = Some(Clipboard {
            text: text.clone(),
            whole_lines: false,
        });
        return Some(CutRow {
            step: buf.top_step(),
            at,
            text,
        });
    }
    // Several carets: each range becomes a selection, overlapping ones one; their texts go to the
    // clipboard a line each and nothing is joined to an earlier cut.
    let mut cut = Carets::from_carets(
        heads
            .iter()
            .map(|at| {
                let r = cut_range(buf, *at);
                Caret::select(r.end, r.start)
            })
            .collect(),
    );
    if cut.carets().iter().all(Caret::is_empty) {
        *carets = cut;
        return None;
    }
    let text = cut
        .carets()
        .iter()
        .map(|c| taken(buf, c.range()))
        .collect::<Vec<_>>()
        .join("\n");
    cut.type_text(buf, "");
    *carets = cut;
    *clipboard = Some(Clipboard {
        text,
        whole_lines: false,
    });
    None
}

/// Typing in overwrite mode (v6 behaviour 25): at every caret the selection goes, and `text`
/// takes the place of as many characters after it as it has itself, up to the end of the line.
fn overwrite_text(buf: &mut TextBuffer, carets: &mut Carets, text: &str) {
    let typed = text.chars().count();
    let list = carets.carets();
    let ranges: Vec<Range<usize>> = list
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let r = c.range();
            let mut end = (r.end + typed).min(line_end(buf, line_of(buf, r.end)).max(r.end));
            // Never into the next caret's text.
            if let Some(next) = list.get(i + 1) {
                end = end.min(next.range().start).max(r.end);
            }
            r.start..end
        })
        .collect();
    let texts = vec![text.to_owned(); ranges.len()];
    carets.replace_each(buf, &ranges, &texts);
}

/// `AutoIndentLines` (v6 behaviour 26) for a language with braces: re-indents the lines of the
/// primary caret's selection, or its line; without a selection the caret then goes one line down.
fn auto_indent_lines(buf: &mut TextBuffer, carets: &mut Carets, unit: &str) {
    let primary = carets.primary();
    let range = primary.range();
    let first = line_of(buf, range.start);
    // The line of the last selected character: a selection ending at a line start leaves that
    // line out.
    let last = if primary.is_empty() {
        first
    } else {
        line_of(buf, range.end - 1)
    };
    let column = primary.head - line_start(buf, line_of(buf, primary.head));
    let edits = smart::auto_indent(buf, first..=last, unit);
    if !edits.is_empty() {
        apply(buf, &edits);
    }
    let moved = carets
        .carets()
        .iter()
        .map(|c| {
            if *c == primary && primary.is_empty() {
                let line = line_of(buf, small::through(c.head, &edits));
                if line + 1 < buf.len_lines() {
                    let next = line_text(buf, line + 1).chars().count();
                    return Caret::at(line_start(buf, line + 1) + column.min(next));
                }
            }
            Caret::select(
                small::through(c.anchor, &edits),
                small::through(c.head, &edits),
            )
        })
        .collect();
    carets.moved_to(moved);
}

fn newline(indent: &str) -> NewlineOptions {
    NewlineOptions {
        unit: indent.to_owned(),
        indent_after: vec!['{', '(', '[', ':'],
    }
}

/// Replaces every caret by one whose head comes from `head`; with `extend` the anchor stays.
fn set_heads(carets: &mut Carets, extend: bool, head: impl Fn(&Caret) -> usize) {
    let list = carets
        .carets()
        .iter()
        .map(|c| {
            let h = head(c);
            Caret::select(if extend { c.anchor } else { h }, h)
        })
        .collect();
    *carets = Carets::from_carets(list);
}

fn indentation(text: &str) -> usize {
    text.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// First non-blank character, then column 0, and back (behaviour 3).
fn smart_home(buf: &TextBuffer, carets: &mut Carets, extend: bool) {
    set_heads(carets, extend, |c| {
        let line = line_of(buf, c.head);
        let start = line_start(buf, line);
        let text = line_text(buf, line);
        let first = indentation(&text);
        let column = c.head - start;
        // Inside the indentation (or on the first character) to column 0, from column 0 and from
        // behind the first character to it (v3 behaviour 15).
        if first == text.chars().count() || (column > 0 && column <= first) {
            start
        } else {
            start + first
        }
    });
}

/// Where `EditorMatchBrace` puts a caret at `at`: at the bracket matching the one at or just
/// before it, before an opening bracket and after a closing one (`SPEC-0057` v4 behaviour 21).
fn match_brace(buf: &TextBuffer, at: usize) -> Option<usize> {
    let to = actions::matching_bracket(buf, at)?;
    let opening = matches!(buf.char_at(to)?, '(' | '[' | '{');
    Some(if opening { to } else { to + 1 })
}

/// Exchanges the list element at every caret with its left or right neighbour, as one undo step
/// (`SPEC-0056` v4 behaviour 19). `lists` holds, for every caret in the order of `carets`, the
/// elements of the comma-separated list around its head (char ranges in order; empty without one).
/// Exchanges that would touch the same element leave only the primary caret's.
pub fn move_element(
    buf: &mut TextBuffer,
    carets: &mut Carets,
    lists: &[Vec<Range<usize>>],
    left: bool,
) {
    let len = buf.len_chars();
    let pair_of = |i: usize, c: &Caret| -> Option<(Range<usize>, Range<usize>)> {
        let list = lists.get(i)?;
        let k = list
            .iter()
            .position(|r| r.start <= c.head && c.head <= r.end)?;
        let (a, b) = if left {
            (k.checked_sub(1)?, k)
        } else {
            (k, k + 1)
        };
        let (a, b) = (list[a].clone(), list.get(b)?.clone());
        (a.start < a.end && a.end <= b.start && b.start < b.end && b.end <= len).then_some((a, b))
    };
    let primary = carets.primary();
    let mut pairs: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    let mut of_primary = None;
    for (i, c) in carets.carets().iter().enumerate() {
        if let Some(pair) = pair_of(i, c) {
            if *c == primary {
                of_primary = Some(pair.clone());
            }
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
    }
    pairs.sort_by_key(|(a, _)| a.start);
    if pairs.windows(2).any(|w| w[1].0.start < w[0].1.end) {
        pairs = of_primary.into_iter().collect();
    }
    if pairs.is_empty() {
        return;
    }
    let mut edits = Vec::new();
    for (a, b) in &pairs {
        let text = |r: &Range<usize>| buf.slice(r.clone()).unwrap_or_default();
        edits.push((a.clone(), text(b)));
        edits.push((b.clone(), text(a)));
    }
    // Each caret stays on its text: the left element's text ends up where the right one ended.
    let heads: Vec<Caret> = carets
        .carets()
        .iter()
        .map(|c| {
            let h = c.head;
            let moved = pairs.iter().find_map(|(a, b)| {
                if h < a.start || h > b.end {
                    None
                } else if h <= a.end {
                    Some(b.start + b.len() - a.len() + (h - a.start))
                } else if h < b.start {
                    Some(h + b.len() - a.len())
                } else {
                    Some(a.start + (h - b.start))
                }
            });
            Caret::at(moved.unwrap_or(h))
        })
        .collect();
    apply(buf, &edits);
    carets.moved_to(heads);
}

/// Opener (`start`) or closer of the bracket pair enclosing `offset`.
fn enclosing_bracket(buf: &TextBuffer, offset: usize, start: bool) -> Option<usize> {
    let mut depth = 0usize;
    if start {
        let mut i = offset;
        while i > 0 {
            i -= 1;
            match buf.char_at(i)? {
                ')' | ']' | '}' => depth += 1,
                '(' | '[' | '{' if depth == 0 => return Some(i),
                '(' | '[' | '{' => depth -= 1,
                _ => {}
            }
        }
    } else {
        let mut i = offset;
        while let Some(c) = buf.char_at(i) {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => return Some(i),
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
    }
    None
}

/// Where Enter after a `{` adds its `}`: the opening line's indentation, when one empty caret
/// stands after a `{` at a line end and the document's braces are not balanced (SPEC-0056 v3
/// behaviour 18).
fn closes_on_enter(buf: &TextBuffer, carets: &Carets) -> Option<String> {
    let c = carets.primary();
    if carets.carets().len() != 1 || !c.is_empty() {
        return None;
    }
    let line = line_of(buf, c.head);
    let start = line_start(buf, line);
    let text = line_text(buf, line);
    let before: String = text.chars().take(c.head - start).collect();
    let after: String = text.chars().skip(c.head - start).collect();
    if !before.trim_end().ends_with('{') || !after.trim().is_empty() {
        return None;
    }
    smart::braces_open(&buf.text()).then(|| text.chars().take(indentation(&text)).collect())
}

/// Backspace with indentation units and empty pairs (behaviour 4); in languages with braces, in the
/// leading whitespace up to the proper indentation it joins the line to the previous one (v3
/// behaviour 16).
fn backspace(buf: &mut TextBuffer, carets: &mut Carets, indent: &str, braces: bool) {
    let unit = indent.chars().count();
    let spaces = !indent.is_empty() && indent.chars().all(|c| c == ' ');
    let ranges: Vec<Range<usize>> = carets
        .carets()
        .iter()
        .map(|c| {
            if !c.is_empty() {
                return c.range();
            }
            let line = line_of(buf, c.head);
            let start = line_start(buf, line);
            let column = c.head - start;
            let before: String = line_text(buf, line).chars().take(column).collect();
            let leading = column > 0 && before.chars().all(|ch| ch == ' ' || ch == '\t');
            if braces && leading && line > 0 {
                let proper = smart::proper_indent(buf, line, indent).chars().count();
                if column <= proper {
                    let mut from = start - 1;
                    if from > 0 && buf.char_at(from - 1) == Some('\r') {
                        from -= 1;
                    }
                    return from..start + indentation(&line_text(buf, line));
                }
                return start + proper..c.head;
            }
            if spaces
                && column > 0
                && column.is_multiple_of(unit)
                && before.chars().all(|ch| ch == ' ')
            {
                c.head - unit..c.head
            } else {
                c.head.saturating_sub(1)..c.head
            }
        })
        .collect();
    if ranges.iter().all(|r| r.len() <= 1) {
        actions::backspace_pair(buf, carets);
    } else {
        let texts = vec![String::new(); ranges.len()];
        carets.replace_each(buf, &ranges, &texts);
    }
}

/// Inserts an indented line above every caret line and puts the caret on it (behaviour 5).
fn new_line_before(buf: &mut TextBuffer, carets: &mut Carets) {
    let mut lines: Vec<usize> = carets
        .carets()
        .iter()
        .map(|c| line_of(buf, c.head))
        .collect();
    lines.dedup();
    let brk = buf.line_ending().as_str();
    let edits: Vec<(Range<usize>, String)> = lines
        .iter()
        .map(|&l| {
            let text = line_text(buf, l);
            let indent: String = text.chars().take(indentation(&text)).collect();
            let at = line_start(buf, l);
            (at..at, format!("{indent}{brk}"))
        })
        .collect();
    let heads: Vec<usize> = edits
        .iter()
        .map(|(r, t)| map_offset(r.start, &edits, true) + t.chars().count() - brk.chars().count())
        .collect();
    apply(buf, &edits);
    *carets = Carets::from_carets(heads.into_iter().map(Caret::at).collect());
}

/// Tab: indent multi-line selections, else spaces to the next unit (behaviour 6).
fn tab(buf: &mut TextBuffer, carets: &mut Carets, indent: &str) {
    let multiline = carets
        .carets()
        .iter()
        .any(|c| line_of(buf, c.range().start) != line_of(buf, c.range().end));
    if multiline {
        actions::indent_lines(buf, carets, indent, false);
        return;
    }
    let unit = indent.chars().count().max(1);
    let ranges: Vec<Range<usize>> = carets.carets().iter().map(Caret::range).collect();
    let texts: Vec<String> = ranges
        .iter()
        .map(|r| {
            if indent == "\t" {
                return "\t".into();
            }
            let column = r.start - line_start(buf, line_of(buf, r.start));
            " ".repeat(unit - column % unit)
        })
        .collect();
    carets.replace_each(buf, &ranges, &texts);
}

fn word_at(buf: &TextBuffer, offset: usize) -> Range<usize> {
    let mut start = offset;
    while start > 0 && buf.char_at(start - 1).is_some_and(is_word) {
        start -= 1;
    }
    let mut end = offset;
    while buf.char_at(end).is_some_and(is_word) {
        end += 1;
    }
    start..end
}

/// Next selection stage: with syntax ranges the smallest larger one of word, `structure` and
/// document (v3 behaviour 13); without, word, line content, whole line, document (behaviour 7).
fn grow(buf: &TextBuffer, r: Range<usize>, structure: &[Range<usize>]) -> Range<usize> {
    if !structure.is_empty() {
        let word = if r.is_empty() {
            word_at(buf, r.start)
        } else {
            r.clone()
        };
        let mut candidates: Vec<Range<usize>> = structure.to_vec();
        candidates.push(word);
        candidates.push(0..buf.len_chars());
        return candidates
            .into_iter()
            .filter(|s| s.start <= r.start && s.end >= r.end && *s != r && !s.is_empty())
            .min_by_key(|s| s.len())
            .unwrap_or(r);
    }
    let first = line_of(buf, r.start);
    let mut last = line_of(buf, r.end);
    if last > first && r.end == line_start(buf, last) {
        last -= 1;
    }
    let content =
        line_start(buf, first)..line_start(buf, last) + line_text(buf, last).chars().count();
    let whole = line_start(buf, first)..line_start(buf, last + 1);
    let word = word_at(buf, r.start);
    let stages = [
        if r.is_empty() || word.contains(&r.start) && word.end >= r.end {
            word
        } else {
            r.clone()
        },
        content,
        whole,
        0..buf.len_chars(),
    ];
    stages
        .into_iter()
        .find(|s| s.start <= r.start && s.end >= r.end && *s != r && !s.is_empty())
        .unwrap_or(r)
}

/// Copies selections, or whole caret lines when nothing is selected (behaviour 8).
fn copy(buf: &TextBuffer, carets: &Carets) -> Clipboard {
    if carets.carets().iter().any(|c| !c.is_empty()) {
        let text = carets
            .carets()
            .iter()
            .filter(|c| !c.is_empty())
            .map(|c| buf.slice(c.range()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        return Clipboard {
            text,
            whole_lines: false,
        };
    }
    let mut lines: Vec<usize> = carets
        .carets()
        .iter()
        .map(|c| line_of(buf, c.head))
        .collect();
    lines.dedup();
    let brk = buf.line_ending().as_str();
    let text = lines
        .iter()
        .map(|&l| format!("{}{brk}", line_text(buf, l)))
        .collect();
    Clipboard {
        text,
        whole_lines: true,
    }
}

/// Indents the lines after the first of `text` by `column` spaces, as text pasted at that column
/// of a file without a formatter (`SPEC-0025` v4 behaviour 6a). Nothing changes for text that
/// starts with a line break or holds only blanks, and a line break at the end opens no line.
pub fn indent_block(text: &str, column: usize) -> String {
    let first = text.chars().find(|c| !matches!(c, ' ' | '\t'));
    if column == 0 || matches!(first, None | Some('\n' | '\r')) {
        return text.to_owned();
    }
    let pad = " ".repeat(column);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('\n') {
        out.push_str(&rest[..=at]);
        rest = &rest[at + 1..];
        if !rest.is_empty() {
            out.push_str(&pad);
        }
    }
    out.push_str(rest);
    out
}

/// Pastes `clip`; with `indent` (indentation unit, braces) multi-line text pasted after only
/// whitespace is re-indented for its place (v3 behaviour 19); with `block` (the tab width) the
/// further lines of text pasted at one caret are indented to its column (`SPEC-0025` v4
/// behaviour 6a).
fn paste(
    buf: &mut TextBuffer,
    carets: &mut Carets,
    clip: &Clipboard,
    indent: Option<(&str, bool)>,
    block: Option<usize>,
) {
    let all_empty = carets.carets().iter().all(Caret::is_empty);
    if let Some(tab) = block
        && carets.carets().len() == 1
        && !(clip.whole_lines && all_empty)
        && clip.text.contains('\n')
    {
        let at = carets.primary().range().start;
        let start = line_start(buf, line_of(buf, at));
        // The column as shown, a tab reaching the next tab stop.
        let column = buf
            .slice(start..at)
            .unwrap_or_default()
            .chars()
            .fold(0, |col, c| {
                if c == '\t' {
                    (col / tab.max(1) + 1) * tab.max(1)
                } else {
                    col + 1
                }
            });
        let text = indent_block(&clip.text, column);
        carets.paste(buf, &text);
        return;
    }
    if let Some((unit, braces)) = indent
        && !(clip.whole_lines && all_empty)
        && carets.carets().len() == 1
    {
        let r = carets.primary().range();
        let line = line_of(buf, r.start);
        let start = line_start(buf, line);
        let text = line_text(buf, line);
        let column = r.start - start;
        let before: String = text.chars().take(column).collect();
        // Text with a line break, or any text pasted onto a blank line.
        let lines = clip.text.contains('\n') || text.trim().is_empty();
        if lines && before.chars().all(|c| c == ' ' || c == '\t') {
            let mut target = smart::proper_indent(buf, line, unit);
            if text.trim().is_empty() && column > target.chars().count() {
                target = before.clone();
            }
            let mut pasted = smart::reindent_paste(&clip.text, &target, unit, braces);
            let rest_blank = buf
                .slice(r.end..start + text.chars().count())
                .unwrap_or_default()
                .trim()
                .is_empty();
            if pasted.ends_with('\n') && !rest_blank {
                pasted.push_str(&before);
            }
            let at = start + pasted.chars().count();
            apply(buf, &[(start..r.end, pasted)]);
            *carets = Carets::new(at);
            return;
        }
    }
    let per_caret = carets.carets().len() > 1
        && clip.text.trim_end_matches(['\n', '\r']).lines().count() == carets.carets().len();
    if !(clip.whole_lines && all_empty) || per_caret {
        let text = if clip.whole_lines && per_caret {
            clip.text.trim_end_matches(['\n', '\r'])
        } else {
            &clip.text
        };
        carets.paste(buf, text);
        return;
    }
    let mut lines: Vec<usize> = carets
        .carets()
        .iter()
        .map(|c| line_of(buf, c.head))
        .collect();
    lines.dedup();
    let edits: Vec<(Range<usize>, String)> = lines
        .iter()
        .map(|&l| {
            let at = line_start(buf, l);
            (at..at, clip.text.clone())
        })
        .collect();
    let heads: Vec<usize> = carets
        .carets()
        .iter()
        .map(|c| map_offset(c.head, &edits, false))
        .collect();
    apply(buf, &edits);
    *carets = Carets::from_carets(heads.into_iter().map(Caret::at).collect());
}

/// Lower-cases selections with an upper-case letter, upper-cases the rest (behaviour 9).
fn toggle_case(buf: &mut TextBuffer, carets: &mut Carets) {
    let ranges: Vec<Range<usize>> = carets
        .carets()
        .iter()
        .map(|c| {
            if c.is_empty() {
                word_at(buf, c.head)
            } else {
                c.range()
            }
        })
        .collect();
    let texts: Vec<String> = ranges
        .iter()
        .map(|r| {
            let s = buf.slice(r.clone()).unwrap_or_default();
            if s.chars().any(char::is_uppercase) {
                s.to_lowercase()
            } else {
                s.to_uppercase()
            }
        })
        .collect();
    let had_selection: Vec<bool> = carets.carets().iter().map(|c| !c.is_empty()).collect();
    let heads: Vec<usize> = carets.carets().iter().map(|c| c.head).collect();
    let edits: Vec<(Range<usize>, String)> = ranges.iter().cloned().zip(texts).collect();
    apply(buf, &edits);
    let mut shift = 0isize;
    let list = edits
        .iter()
        .zip(had_selection.iter().zip(heads))
        .map(|((r, t), (selected, head))| {
            let start = (r.start as isize + shift) as usize;
            let end = start + t.chars().count();
            shift += t.chars().count() as isize - r.len() as isize;
            if *selected {
                Caret::select(start, end)
            } else {
                Caret::at((head as isize + shift).max(0) as usize)
            }
        })
        .collect();
    *carets = Carets::from_carets(list);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// State from text with `|` marking carets and `[`…`]` marking selections (anchor at `[`).
    fn state(marked: &str) -> EditorState {
        let mut text = String::new();
        let mut list = Vec::new();
        let mut anchor = None;
        for c in marked.chars() {
            let at = text.chars().count();
            match c {
                '|' => list.push(Caret::at(at)),
                '[' => anchor = Some(at),
                ']' => list.push(Caret::select(anchor.take().expect("open"), at)),
                _ => text.push(c),
            }
        }
        let mut s = EditorState::new(&text);
        s.carets = Carets::from_carets(list);
        s
    }

    /// Text with the carets marked like [`state`].
    fn show(s: &EditorState) -> String {
        let text: Vec<char> = s.buffer.text().chars().collect();
        let mut out = String::new();
        for i in 0..=text.len() {
            for c in s.carets.carets() {
                let r = c.range();
                if c.is_empty() && c.head == i {
                    out.push('|');
                } else if !c.is_empty() && r.end == i {
                    out.push(']');
                }
            }
            for c in s.carets.carets() {
                if !c.is_empty() && c.range().start == i {
                    out.push('[');
                }
            }
            if let Some(ch) = text.get(i) {
                out.push(*ch);
            }
        }
        out
    }

    /// SPEC-0057 v7 behaviour 9 (AK-19): undo puts the carets and selections back where they were
    /// before the step's command, redo where they were after it.
    #[test]
    fn undo_and_redo_put_the_carets_back() {
        let undo = |s: &mut EditorState| {
            assert!(perform(s, "$Undo"));
            show(s)
        };
        let redo = |s: &mut EditorState| {
            assert!(perform(s, "$Redo"));
            show(s)
        };
        // Typing over a selection: the selection comes back, with its direction.
        let mut s = state("[one] two");
        type_text(&mut s, "x");
        assert_eq!(show(&s), "x| two");
        assert_eq!(undo(&mut s), "[one] two");
        assert_eq!(s.carets.primary(), Caret::select(0, 3));
        assert_eq!(redo(&mut s), "x| two");
        let mut s = state("one two");
        s.carets = Carets::from_carets(vec![Caret::select(7, 4)]);
        assert!(perform(&mut s, "EditorBackSpace"));
        assert_eq!(undo(&mut s), "one [two]");
        assert_eq!(
            s.carets.primary(),
            Caret::select(7, 4),
            "the head stays in front"
        );

        // An edit at three carets: all three are there after undo and after redo.
        let mut s = state("a|\nb|\nc|\n");
        type_text(&mut s, "x");
        type_text(&mut s, "y");
        assert_eq!(show(&s), "axy|\nbxy|\ncxy|\n");
        assert_eq!(
            undo(&mut s),
            "a|\nb|\nc|\n",
            "before the first key of the run"
        );
        assert_eq!(redo(&mut s), "axy|\nbxy|\ncxy|\n");

        // Line commands: the caret stays on its place in its line.
        let lines = "one\ntw|o\nthree\n";
        for (action, after) in [
            ("MoveLineUp", "tw|o\none\nthree\n"),
            ("EditorDeleteLine", "one\nth|ree\n"),
            ("EditorDuplicate", "one\ntwo\ntw|o\nthree\n"),
        ] {
            let mut s = state(lines);
            assert!(perform(&mut s, action), "{action}");
            assert_eq!(show(&s), after, "{action}");
            assert_eq!(undo(&mut s), lines, "{action}");
            assert_eq!(redo(&mut s), after, "{action}");
        }

        // An auto-closed bracket and its removal: the caret is between the two again.
        let mut s = state("f|");
        s.language = Some("rust".into());
        type_text(&mut s, "(");
        assert_eq!(show(&s), "f(|)");
        assert!(perform(&mut s, "EditorBackSpace"));
        assert_eq!(show(&s), "f|");
        assert_eq!(undo(&mut s), "f(|)");
        assert_eq!(undo(&mut s), "f|");
        assert_eq!(redo(&mut s), "f(|)");

        // A run of deleting: before its first key, after its last.
        let mut s = state("abcd|");
        for _ in 0..3 {
            assert!(perform(&mut s, "EditorBackSpace"));
        }
        assert_eq!(show(&s), "a|");
        assert_eq!(undo(&mut s), "abcd|");
        assert_eq!(redo(&mut s), "a|");

        // A caret move between two edits is no step of its own (IMP-0305), and each undo puts
        // the caret where that edit began.
        let mut s = state("ab|");
        type_text(&mut s, "c");
        assert!(perform(&mut s, "EditorTextStart"));
        type_text(&mut s, "x");
        assert_eq!(show(&s), "x|abc");
        assert_eq!(undo(&mut s), "|abc");
        assert_eq!(undo(&mut s), "ab|");
        assert_eq!(redo(&mut s), "abc|");
        assert_eq!(redo(&mut s), "x|abc");
    }

    /// SPEC-0025 v2 behaviour 8: ⌃⇧G undoes ⌃G step by step.
    #[test]
    fn unselect_occurrence_undoes_select_next_occurrence() {
        let next = "SelectNextOccurrence";
        let back = "UnselectPreviousOccurrence";
        assert_eq!(
            run("a|b x ab ab", &[next, next, next, back]),
            "[ab] x [ab] ab"
        );
        assert_eq!(run("a|b x ab", &[next, next, back, back]), "ab| x ab");
        assert_eq!(run("a|b", &[back]), "a|b", "one empty caret stays");
    }

    /// D52 (hand test 198): after typing at every occurrence, ⌃⇧G still removes the caret added
    /// last, not the first one.
    #[test]
    fn unselect_occurrence_after_typing_removes_the_latest_caret() {
        let next = "SelectNextOccurrence";
        let back = "UnselectPreviousOccurrence";
        let mut s = state("w|ort\nwort\nwort\nwort");
        for _ in 0..3 {
            assert!(perform(&mut s, next));
        }
        type_text(&mut s, "X");
        assert_eq!(show(&s), "X|\nX|\nX|\nwort");
        assert!(perform(&mut s, back));
        type_text(&mut s, "Y");
        assert_eq!(
            show(&s),
            "XY|\nXY|\nX\nwort",
            "the third (latest) caret went"
        );
    }

    fn run(marked: &str, actions: &[&str]) -> String {
        let mut s = state(marked);
        for a in actions {
            assert!(perform(&mut s, a), "{a} not handled");
        }
        show(&s)
    }

    #[test]
    fn typing_is_one_undo_step_until_something_else_happens() {
        let typed = |s: &mut EditorState, text: &str| {
            text.chars().for_each(|c| type_text(s, &c.to_string()))
        };
        let mut s = state("x|");
        typed(&mut s, "int a = 1;");
        assert_eq!(show(&s), "xint a = 1;|");
        perform(&mut s, "$Undo");
        assert_eq!(show(&s), "x|", "the whole run of typing at once");
        perform(&mut s, "$Redo");
        assert_eq!(show(&s), "xint a = 1;|");
        // Enter, a caret move and a save each end the group.
        type_text(&mut s, "\n");
        typed(&mut s, "b");
        perform(&mut s, "EditorLeft");
        perform(&mut s, "EditorRight");
        typed(&mut s, "c");
        s.buffer.mark_saved();
        typed(&mut s, "d");
        assert_eq!(show(&s), "xint a = 1;\nbcd|");
        for expected in [
            "xint a = 1;\nbc|",
            "xint a = 1;\nb|",
            "xint a = 1;\n|",
            "xint a = 1;|",
            "x|",
        ] {
            perform(&mut s, "$Undo");
            assert_eq!(show(&s), expected);
        }
        // Stepping over a typed closer stays in the same group.
        let mut s = state("|");
        typed(&mut s, "f()");
        assert_eq!(show(&s), "f()|");
        perform(&mut s, "$Undo");
        assert_eq!(show(&s), "|");
    }

    #[test]
    fn motions_with_and_without_selection() {
        assert_eq!(run("ab|c", &["EditorLeft"]), "a|bc");
        assert_eq!(run("ab|c", &["EditorLeftWithSelection"]), "a[b]c");
        assert_eq!(run("a[bc]", &["EditorLeft"]), "a|bc");
        assert_eq!(run("a[bc]", &["EditorRight"]), "abc|");
        assert_eq!(
            run("one |two", &["EditorNextWordWithSelection"]),
            "one [two]"
        );
        assert_eq!(
            run("abcdef|\nx\nabcdefgh", &["EditorDown", "EditorDown"]),
            "abcdef\nx\nabcdef|gh"
        );
        assert_eq!(run("a|b\nc|d", &["EditorLineEnd"]), "ab|\ncd|");
        assert_eq!(run("x\ny|", &["EditorTextStartWithSelection"]), "[x\ny]");
        // Code block start/end.
        assert_eq!(
            run("f(a, (b|), c)", &["EditorCodeBlockStart"]),
            "f(a, |(b), c)"
        );
        assert_eq!(
            run("f(a, |(b), c)", &["EditorCodeBlockEnd"]),
            "f(a, (b), c|)"
        );
    }

    #[test]
    fn page_commands_move_the_viewport() {
        let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let mut s = EditorState::new(&text);
        s.viewport = Viewport {
            first_line: 0,
            lines: 10,
        };
        perform(&mut s, "EditorPageDown");
        assert_eq!(
            s.buffer
                .offset_to_position(s.carets.primary().head)
                .unwrap()
                .line,
            10
        );
        assert_eq!(s.viewport.first_line, 10);
        perform(&mut s, "EditorMoveToPageBottomWithSelection");
        let p = s.carets.primary();
        assert_eq!(s.buffer.offset_to_position(p.head).unwrap().line, 19);
        assert_eq!(s.buffer.offset_to_position(p.anchor).unwrap().line, 10);
        perform(&mut s, "EditorPageUp");
        assert_eq!(s.viewport.first_line, 0);
        perform(&mut s, "EditorMoveToPageTop");
        assert_eq!(s.carets.primary().head, 0);
    }

    #[test]
    fn smart_home_cycles() {
        assert_eq!(run("    let |x", &["EditorLineStart"]), "    |let x");
        assert_eq!(
            run("    let |x", &["EditorLineStart", "EditorLineStart"]),
            "|    let x"
        );
        assert_eq!(
            run(
                "    let |x",
                &["EditorLineStart", "EditorLineStart", "EditorLineStart"]
            ),
            "    |let x"
        );
        assert_eq!(run("   |", &["EditorLineStart"]), "|   ");
        assert_eq!(run("  ab|", &["EditorLineStartWithSelection"]), "  [ab]");
    }

    #[test]
    fn deleting() {
        assert_eq!(
            run("if x:\n        |y", &["EditorBackSpace"]),
            "if x:\n    |y"
        );
        assert_eq!(run("  |y", &["EditorBackSpace"]), " |y");
        assert_eq!(run("f(|)", &["EditorBackSpace"]), "f|");
        assert_eq!(run("ab|c", &["$Delete"]), "ab|");
        assert_eq!(run("foo bar|", &["EditorDeleteToWordStart"]), "foo |");
        assert_eq!(run("|foo bar", &["EditorDeleteToWordEnd"]), "| bar");
        assert_eq!(run("a\nb|\nc", &["EditorDeleteLine"]), "a\nc|");
    }

    #[test]
    fn lines_and_tab() {
        assert_eq!(run("    a|b", &["EditorStartNewLine"]), "    ab\n    |");
        assert_eq!(
            run("x\n    a|b", &["EditorStartNewLineBefore"]),
            "x\n    |\n    ab"
        );
        assert_eq!(run("ab|cd", &["EditorSplitLine"]), "ab|\ncd");
        assert_eq!(run("ab|", &["EditorTab"]), "ab  |");
        assert_eq!(run("|ab", &["EditorTab"]), "    |ab");
        assert_eq!(run("[a\nb]", &["EditorTab"]), "[    a\n    b]");
        assert_eq!(run("    |a", &["EditorUnindentSelection"]), "|a");
        let mut s = state("x|");
        s.indent = "\t".into();
        perform(&mut s, "EditorTab");
        assert_eq!(show(&s), "x\t|");
    }

    /// SPEC-0137/AK-6: a new whole text keeps the file's indentation and tab width (they fell back
    /// to four spaces and 4 before), so Tab still inserts one whole unit on an empty line and goes
    /// to the next stop after text.
    #[test]
    fn a_new_text_keeps_the_settings_of_the_file() {
        let mut s = state("old|");
        s.indent = "  ".into();
        s.tab_width = 2;
        s.comment = Some("#".into());
        s.block_comment = Some(("/*".into(), "*/".into()));
        s.language = Some("python".into());
        let mut t = s.with_text("x\n");
        assert_eq!(show(&t), "|x\n");
        assert_eq!(
            (t.indent.as_str(), t.tab_width, t.comment.as_deref()),
            ("  ", 2, Some("#"))
        );
        assert_eq!(t.block_comment, s.block_comment);
        assert_eq!(t.language.as_deref(), Some("python"));
        t.carets = Carets::new(2);
        perform(&mut t, "EditorTab");
        assert_eq!(show(&t), "x\n  |");
        let mut t = s.with_text("x");
        t.carets = Carets::new(1);
        perform(&mut t, "EditorTab");
        assert_eq!(show(&t), "x |");
    }

    #[test]
    fn clipboard_whole_lines_and_per_caret() {
        let mut s = state("one\ntw|o\nthree");
        perform(&mut s, "$Copy");
        assert_eq!(
            s.clipboard,
            Some(Clipboard {
                text: "two\n".into(),
                whole_lines: true
            })
        );
        perform(&mut s, "EditorDown");
        perform(&mut s, "$Paste");
        assert_eq!(show(&s), "one\ntwo\ntwo\nth|ree");
        let mut s = state("a\nb|\nc");
        perform(&mut s, "$Cut");
        assert_eq!(s.buffer.text(), "a\nc");
        let mut s = state("[x] [y]\n|\n|");
        perform(&mut s, "$Copy");
        assert_eq!(s.clipboard.as_ref().unwrap().text, "x\ny");
        s.carets = Carets::from_carets(vec![Caret::at(4), Caret::at(5)]);
        perform(&mut s, "$Paste");
        assert_eq!(s.buffer.text(), "x y\nx\ny");
        assert_eq!(run("[ab]c", &["$Cut", "EditorRight", "$Paste"]), "cab|");
    }

    #[test]
    fn selection_stages_escape_case_and_history() {
        let stages = [
            "a\nfoo b|ar baz\nc",
            "a\nfoo [bar] baz\nc",
            "a\n[foo bar baz]\nc",
            "a\n[foo bar baz\n]c",
            "[a\nfoo bar baz\nc]",
        ];
        let mut s = state(stages[0]);
        for want in &stages[1..] {
            perform(&mut s, "EditorSelectWord");
            assert_eq!(show(&s), *want);
        }
        for want in stages[1..4].iter().rev() {
            perform(&mut s, "EditorUnSelectWord");
            assert_eq!(show(&s), *want);
        }
        // `state` makes the first caret primary.
        assert_eq!(run("a| b| c|", &["EditorEscape"]), "a| b c");
        assert_eq!(
            run("[Hello] w|orld", &["EditorToggleCase"]),
            "[hello] W|ORLD"
        );
        assert_eq!(run("ab|", &["EditorBackSpace", "$Undo"]), "ab|");
        assert_eq!(run("ab|", &["EditorBackSpace", "$Undo", "$Redo"]), "a|");
    }

    /// SPEC-0057 v4 AK-16.
    #[test]
    fn match_brace_moves_outside_the_pair() {
        let mb = "EditorMatchBrace";
        assert_eq!(run("|(a)", &[mb]), "(a)|");
        assert_eq!(run("|(a)", &[mb, mb]), "|(a)");
        assert_eq!(run("(|a)", &[mb]), "(a)|");
        assert_eq!(run("f(a|)", &[mb]), "f|(a)");
        assert_eq!(run("|(a) {b}|", &[mb]), "(a)| |{b}");
        // A bracket inside a string literal is not the partner.
        assert_eq!(run("|(\"a)\" + b)", &[mb]), "(\"a)\" + b)|");
        assert_eq!(run("(\"a)\" + b)|", &[mb]), "|(\"a)\" + b)");
        // A selection is dropped; without a bracket or a partner the caret stays.
        assert_eq!(run("x[ab](c)", &[mb]), "xab(c)|");
        assert_eq!(run("a|b", &[mb]), "a|b");
        assert_eq!(run("(|", &[mb]), "(|");
    }

    /// SPEC-0057 v4 AK-17.
    #[test]
    fn scroll_to_center() {
        let text = "x\n".repeat(100);
        let mut s = EditorState::new(&text);
        s.viewport.lines = 10;
        s.carets = Carets::from_carets(vec![Caret::select(100, 101)]);
        let before = s.carets.clone();
        assert!(perform(&mut s, "EditorScrollToCenter"));
        assert_eq!(s.viewport.first_line, 46, "line 50 is the fifth of ten");
        assert_eq!(s.carets, before);
        s.carets = Carets::new(4);
        assert!(perform(&mut s, "EditorScrollToCenter"));
        assert_eq!(s.viewport.first_line, 0);
        assert!(!s.buffer.is_modified());
    }

    /// SPEC-0071 v9 AK-8.
    #[test]
    fn method_up_and_down() {
        let mut s =
            state("class A {\n  void f() {\n    x|;\n  }\n  void g() {}\n  void h() {}\n}\n");
        let text = s.buffer.text();
        let at = |name: &str| text.find(name).unwrap();
        s.methods = vec![at("f()"), at("g()"), at("h()")];
        let head = |s: &EditorState| s.carets.primary().head;
        assert!(perform(&mut s, "MethodUp"));
        assert_eq!(
            head(&s),
            at("f()"),
            "the name of the method the caret is in"
        );
        assert!(perform(&mut s, "MethodUp"));
        assert_eq!(head(&s), at("f()"), "no method before the first");
        assert!(perform(&mut s, "MethodDown"));
        assert_eq!(head(&s), at("g()"));
        assert!(perform(&mut s, "MethodDown"));
        assert!(perform(&mut s, "MethodDown"));
        assert_eq!(head(&s), at("h()"), "no method after the last");
        assert!(perform(&mut s, "MethodUp"));
        assert_eq!(head(&s), at("g()"));
        // Selections and further carets are dropped.
        s.carets = Carets::from_carets(vec![Caret::select(0, 3), Caret::at(at("h()") + 2)]);
        assert!(perform(&mut s, "MethodDown"));
        assert_eq!(s.carets, Carets::new(at("f()")), "from the primary caret");
        // Without declarations the caret stays.
        s.methods.clear();
        assert!(perform(&mut s, "MethodDown") && perform(&mut s, "MethodUp"));
        assert_eq!(s.carets, Carets::new(at("f()")));
        assert!(!s.buffer.is_modified());
    }

    /// SPEC-0071 v9 AK-9.
    #[test]
    fn jump_to_last_and_next_change() {
        let (last, next) = ("JumpToLastChange", "JumpToNextChange");
        let mut s = state("one\ntwo\nthree|");
        // No edits: both do nothing.
        assert!(perform(&mut s, last) && perform(&mut s, next));
        assert_eq!(show(&s), "one\ntwo\nthree|");
        type_text(&mut s, "A");
        perform(&mut s, "EditorTextStart");
        type_text(&mut s, "B");
        perform(&mut s, "EditorDown");
        assert_eq!(show(&s), "Bone\nt|wo\nthreeA");
        // Nothing to go forward to before the first step back.
        assert!(perform(&mut s, next));
        assert_eq!(show(&s), "Bone\nt|wo\nthreeA");
        assert!(perform(&mut s, last));
        assert_eq!(show(&s), "B|one\ntwo\nthreeA");
        assert!(perform(&mut s, last));
        assert_eq!(show(&s), "Bone\ntwo\nthreeA|");
        assert!(perform(&mut s, last));
        assert_eq!(show(&s), "Bone\ntwo\nthreeA|", "the oldest place");
        assert!(perform(&mut s, next));
        assert_eq!(show(&s), "B|one\ntwo\nthreeA");
        assert!(perform(&mut s, next));
        assert_eq!(show(&s), "B|one\ntwo\nthreeA", "the newest place");
        // A caret move in between does not end the walk; an edit starts it over.
        perform(&mut s, "EditorTextEnd");
        assert!(perform(&mut s, last));
        assert_eq!(show(&s), "Bone\ntwo\nthreeA|");
        perform(&mut s, "EditorUp");
        type_text(&mut s, "C");
        perform(&mut s, "EditorTextStart");
        assert!(perform(&mut s, last));
        assert_eq!(show(&s), "Bone\ntwoC|\nthreeA");
        // Undo takes the place away and starts over as well.
        perform(&mut s, "$Undo");
        perform(&mut s, "EditorTextEnd");
        assert!(perform(&mut s, last));
        assert_eq!(show(&s), "B|one\ntwo\nthreeA");
    }

    /// SPEC-0056 v4 AK-13.
    #[test]
    fn move_element_left_and_right() {
        let (left, right) = ("MoveElementLeft", "MoveElementRight");
        // f(aa, b, ccc): the elements at 2..4, 6..7 and 9..12.
        let with = |marked: &str| {
            let mut s = state(marked);
            let list = vec![2..4, 6..7, 9..12];
            s.lists = vec![list; s.carets.carets().len()];
            s
        };
        let mut s = with("f(aa, b|, ccc)");
        assert!(perform(&mut s, left));
        assert_eq!(show(&s), "f(b|, aa, ccc)");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "f(aa, b, ccc)", "one undo step");
        assert!(!s.buffer.is_modified());
        let mut s = with("f(a|a, b, ccc)");
        assert!(perform(&mut s, right));
        assert_eq!(show(&s), "f(b, a|a, ccc)");
        // The edges stay, and nothing is written into the history.
        let mut s = with("f(a|a, b, ccc)");
        assert!(perform(&mut s, left));
        assert_eq!(show(&s), "f(a|a, b, ccc)");
        let mut s = with("f(aa, b, ccc|)");
        assert!(perform(&mut s, right));
        assert_eq!(show(&s), "f(aa, b, ccc|)");
        assert_eq!(s.buffer.top_step(), None);
        // A selection becomes a caret at its head.
        let mut s = with("f(aa, b, ccc)");
        s.carets = Carets::from_carets(vec![Caret::select(9, 11)]);
        assert!(perform(&mut s, left));
        assert_eq!(show(&s), "f(aa, cc|c, b)");
        // Two carets in two lists both move.
        let mut s = state("f(|a, b) g(|c, d)");
        s.lists = vec![vec![2..3, 5..6], vec![10..11, 13..14]];
        assert!(perform(&mut s, right));
        assert_eq!(show(&s), "f(b, |a) g(d, |c)");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "f(a, b) g(c, d)");
        // Two carets on neighbours: only the primary caret's element moves, the other caret
        // stays on its text.
        let mut s = state("f(|a, |b, c)");
        s.lists = vec![vec![2..3, 5..6, 8..9]; 2];
        assert!(perform(&mut s, right));
        assert_eq!(show(&s), "f(|b, |a, c)");
        // Without a list nothing happens.
        let mut s = state("f(a|, b)");
        assert!(perform(&mut s, left) && perform(&mut s, right));
        assert_eq!(show(&s), "f(a|, b)");
        s.lists = vec![Vec::new()];
        assert!(perform(&mut s, left));
        assert_eq!(s.buffer.top_step(), None);
    }

    #[test]
    fn listed_commands_are_handled_and_others_are_not() {
        for id in COMMANDS {
            // Without a selection Tab goes on to `EditorTab` (v3 behaviour 17).
            if *id == "EditorIndentSelection" {
                continue;
            }
            let mut s = state("fn main() {\n    let |x = 1;\n}\n");
            s.comment = Some("//".into());
            s.block_comment = Some(("/*".into(), "*/".into()));
            s.clipboard = Some(Clipboard {
                text: "y".into(),
                whole_lines: false,
            });
            assert!(perform(&mut s, id), "{id}");
        }
        let mut s = state("a|b");
        assert!(!perform(&mut s, "GotoFile"));
        assert!(!perform(&mut s, "EditorBackSpaceWithSelection"));
        assert!(!perform(&mut s, "CommentByLineComment"), "no comment token");
        assert_eq!(show(&s), "a|b");
    }

    #[test]
    fn editor_actions_win() {
        assert_eq!(
            editor_first(&["$Delete", "EditorDeleteLine", "Other"]),
            ["EditorDeleteLine", "$Delete", "Other"]
        );
    }

    #[test]
    fn typing() {
        let mut s = state("f|");
        type_text(&mut s, "(");
        assert_eq!(show(&s), "f(|)");
        type_text(&mut s, "\n");
        assert_eq!(show(&s), "f(\n    |\n)");
        type_text(&mut s, "äö");
        assert_eq!(show(&s), "f(\n    äö|\n)");
    }

    // ---- SPEC-0056 v3 / SPEC-0057 v3 (OBS-0018) ----

    const EDIT: &str = "package lab.inventory;

import java.util.List;

public class Edit {
    int sum(List<Integer> xs) {
        int total = 0;
        for (int x : xs) {
            total += x;
        }
        return total;
    }

    String greet(String name) {
        String a = \"Hello, \" + name;
        String b = a.trim();
        return b;
    }

    void calls() {
        System.out.println(sum(List.of(1, 2, 3)));
        System.out.println(greet(\"Bob\"));
    }
}
";

    /// A Java editor on `EDIT` with the caret at 1-based `line`:`column`.
    fn java(line: usize, column: usize) -> EditorState {
        let mut s = EditorState::new(EDIT);
        s.language = Some("java".into());
        s.comment = Some("//".into());
        s.block_comment = Some(("/*".into(), "*/".into()));
        let at = s
            .buffer
            .position_to_offset(crate::Position::new(line - 1, column - 1))
            .expect("inside");
        s.carets = Carets::new(at);
        s
    }

    /// 1-based line and column of the primary caret's head.
    fn at(s: &EditorState) -> (usize, usize) {
        let p = s
            .buffer
            .offset_to_position(s.carets.primary().head)
            .unwrap();
        (p.line + 1, p.column + 1)
    }

    fn line(s: &EditorState, n: usize) -> String {
        s.buffer
            .line(n - 1)
            .unwrap_or_default()
            .trim_end_matches('\n')
            .to_owned()
    }

    /// SPEC-0056 v3 behaviour 12 (AK-6).
    #[test]
    fn duplicate_puts_the_caret_on_the_declared_name() {
        let mut s = java(15, 12);
        perform(&mut s, "EditorDuplicate");
        assert_eq!(line(&s, 16), "        String a = \"Hello, \" + name;");
        assert_eq!(at(&s), (16, 16));
        let mut s = java(7, 5);
        perform(&mut s, "EditorDuplicate");
        assert_eq!(at(&s), (8, 13));
        let mut s = java(17, 12);
        perform(&mut s, "EditorDuplicate");
        assert_eq!(at(&s), (18, 12), "not a declaration: column kept");
    }

    /// SPEC-0056 v3 behaviour 13 (AK-7).
    #[test]
    fn move_statement_follows_blocks() {
        let mut s = java(8, 12);
        perform(&mut s, "MoveStatementUp");
        assert_eq!(line(&s, 7), "        for (int x : xs) {");
        assert_eq!(line(&s, 10), "        int total = 0;");
        assert_eq!(at(&s), (7, 12));

        let mut s = java(14, 8);
        perform(&mut s, "MoveStatementUp");
        assert_eq!(line(&s, 6), "    String greet(String name) {");
        assert_eq!(line(&s, 11), "");
        assert_eq!(line(&s, 12), "    int sum(List<Integer> xs) {");
        assert_eq!(line(&s, 19), "");
        assert_eq!(line(&s, 20), "    void calls() {");

        for (l, c, up) in [
            (15, 10, true),
            (6, 8, true),
            (20, 8, false),
            (17, 10, false),
        ] {
            let mut s = java(l, c);
            perform(
                &mut s,
                if up {
                    "MoveStatementUp"
                } else {
                    "MoveStatementDown"
                },
            );
            assert_eq!(s.buffer.text(), EDIT, "{l}:{c} stays");
        }

        let mut s = java(11, 10);
        perform(&mut s, "MoveStatementUp");
        assert_eq!(line(&s, 10), "            return total;");
        assert_eq!(line(&s, 11), "        }");
        assert_eq!(at(&s), (10, 14));

        let mut s = java(9, 14);
        perform(&mut s, "MoveStatementDown");
        assert_eq!(line(&s, 9), "        }");
        assert_eq!(line(&s, 10), "        total += x;");

        let mut s = java(16, 10);
        perform(&mut s, "MoveStatementDown");
        assert_eq!(line(&s, 16), "        return b;");
        assert_eq!(line(&s, 17), "        String b = a.trim();");

        let mut s = state("a = 1\nb| = 2\n");
        s.language = Some("python".into());
        perform(&mut s, "MoveStatementUp");
        assert_eq!(show(&s), "b| = 2\na = 1\n");
    }

    /// SPEC-0056 v3 behaviour 14 (AK-8).
    #[test]
    fn join_removes_the_braces_of_a_one_line_block() {
        let mut s = java(8, 5);
        perform(&mut s, "EditorJoinLines");
        assert_eq!(line(&s, 8), "        for (int x : xs) total += x;");
        assert_eq!(line(&s, 9), "        return total;");
        assert_eq!(at(&s), (8, 26));
        let mut s = java(7, 5);
        perform(&mut s, "EditorJoinLines");
        assert_eq!(line(&s, 7), "        int total = 0; for (int x : xs) {");
        assert_eq!(at(&s), (7, 23));
    }

    /// SPEC-0056 v3 behaviours 15–17 (AK-9).
    #[test]
    fn comment_moves_the_caret_down() {
        let mut s = java(15, 14);
        perform(&mut s, "CommentByLineComment");
        assert_eq!(line(&s, 15), "//        String a = \"Hello, \" + name;");
        assert_eq!(at(&s), (16, 16));
        perform(&mut s, "CommentByLineComment");
        assert_eq!(line(&s, 16), "//        String b = a.trim();");
        let mut s = java(13, 1);
        perform(&mut s, "CommentByLineComment");
        assert_eq!(line(&s, 13), "//");
        assert_eq!(at(&s), (13, 3));
    }

    #[test]
    fn block_comments() {
        assert_eq!(run_block("b = [a.trim()];"), "b = [/*a.trim()*/];");
        assert_eq!(run_block("b = [/*a.trim()*/];"), "b = [a.trim()];");
        assert_eq!(run_block("b = |a;"), "b = /*|*/a;");
        assert_eq!(run_block("b = /* x| */a;"), "b =  x| a;");
        let mut s = state("a|b");
        assert!(!perform(&mut s, "CommentByBlockComment"), "no tokens");
    }

    fn run_block(marked: &str) -> String {
        let mut s = state(marked);
        s.block_comment = Some(("/*".into(), "*/".into()));
        assert!(perform(&mut s, "CommentByBlockComment"));
        show(&s)
    }

    #[test]
    fn markdown_comments() {
        let mut s = state("# T\n\nSome |text.\n- item\n");
        s.language = Some("markdown".into());
        s.block_comment = Some(("<!--".into(), "-->".into()));
        assert!(perform(&mut s, "CommentByLineComment"));
        assert_eq!(s.buffer.text(), "# T\n\n<!-- Some text. -->\n- item\n");
        let mut s = state("[<!-- a -->\n<!-- b -->]\n");
        s.block_comment = Some(("<!--".into(), "-->".into()));
        assert!(perform(&mut s, "CommentByLineComment"));
        assert_eq!(s.buffer.text(), "a\nb\n");
        let mut s = state("x [y] z");
        s.block_comment = Some(("<!--".into(), "-->".into()));
        assert!(perform(&mut s, "CommentByBlockComment"));
        assert_eq!(show(&s), "x [<!--y-->] z");
    }

    /// SPEC-0056 v3 behaviour 18 (AK-10).
    #[test]
    fn brace_closes_on_enter() {
        let mut s = state("class A {\n    void f() |\n}\n");
        s.language = Some("java".into());
        type_text(&mut s, "{");
        assert_eq!(show(&s), "class A {\n    void f() {|\n}\n");
        perform(&mut s, "EditorEnter");
        assert_eq!(show(&s), "class A {\n    void f() {\n        |\n    }\n}\n");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "class A {\n    void f() {\n}\n");
        let mut s = java(6, 32);
        perform(&mut s, "EditorEnter");
        assert_eq!(line(&s, 7), "        ");
        assert_eq!(line(&s, 8), "        int total = 0;", "balanced: no closer");
        let mut s = state("x = (|)");
        type_text(&mut s, "{");
        assert_eq!(show(&s), "x = ({|)");
    }

    /// SPEC-0057 v3 behaviour 13 (AK-10).
    #[test]
    fn select_word_uses_the_given_stages() {
        let mut s = state("ab Hel|lo cd");
        s.structure = vec![2..9, 0..11];
        perform(&mut s, "EditorSelectWord");
        assert_eq!(show(&s), "ab [Hello] cd");
        perform(&mut s, "EditorSelectWord");
        assert_eq!(show(&s), "ab[ Hello ]cd");
        perform(&mut s, "$Copy");
        perform(&mut s, "EditorUnSelectWord");
        assert_eq!(show(&s), "ab [Hello] cd", "copy keeps the chain");
    }

    /// SPEC-0057 v3 behaviour 14 (AK-11).
    #[test]
    fn word_stops_follow_punctuation_and_line_ends() {
        let mut s = java(16, 16);
        let mut seen = Vec::new();
        for _ in 0..8 {
            perform(&mut s, "EditorNextWord");
            seen.push(at(&s));
        }
        assert_eq!(
            seen,
            [
                (16, 17),
                (16, 19),
                (16, 21),
                (16, 22),
                (16, 26),
                (16, 27),
                (16, 28),
                (16, 29)
            ]
        );
        perform(&mut s, "EditorNextWord");
        assert_eq!(at(&s), (17, 1), "line end to next line start");
        perform(&mut s, "EditorNextWord");
        assert_eq!(at(&s), (17, 15));
        let mut s = java(17, 9);
        perform(&mut s, "EditorPreviousWord");
        assert_eq!(
            at(&s),
            (16, 29),
            "from the first character to the previous line end"
        );
        perform(&mut s, "EditorPreviousWord");
        assert_eq!(at(&s), (16, 28));
        let mut s = java(15, 21);
        for want in [(15, 26), (15, 27), (15, 29)] {
            perform(&mut s, "EditorNextWord");
            assert_eq!(at(&s), want);
        }
    }

    /// SPEC-0057 v3 behaviours 15–17 (AK-12).
    #[test]
    fn backspace_to_the_indentation() {
        let mut s = java(17, 9);
        perform(&mut s, "EditorBackSpace");
        assert_eq!(line(&s, 16), "        String b = a.trim();return b;");
        assert_eq!(at(&s), (16, 29));
        let mut s = java(9, 9);
        perform(&mut s, "EditorBackSpace");
        assert_eq!(line(&s, 8), "        for (int x : xs) {total += x;");
        let mut s = java(16, 29);
        perform(&mut s, "EditorStartNewLine");
        assert_eq!(at(&s), (17, 9));
        perform(&mut s, "EditorBackSpace");
        assert_eq!(s.buffer.text(), EDIT, "blank indented line removed");
        let mut s = state("if x:\n        |y");
        s.language = Some("python".into());
        perform(&mut s, "EditorBackSpace");
        assert_eq!(show(&s), "if x:\n    |y", "no braces: one unit");
        assert_eq!(run("  |  let", &["EditorLineStart"]), "|    let");
    }

    #[test]
    fn tab_inside_a_word() {
        let mut s = java(16, 12);
        let ids = editor_first(&["EditorIndentSelection", "EditorTab"]);
        let done = ids.iter().find(|id| perform(&mut s, id)).copied();
        assert_eq!(done, Some("EditorTab"));
        assert_eq!(line(&s, 16), "        Str ing b = a.trim();");
        let mut s = state("[a\nb]");
        assert!(perform(&mut s, "EditorIndentSelection"));
    }

    /// SPEC-0057 v3 behaviour 18 (AK-13); IMP-0305.
    #[test]
    fn repeated_deleting_is_one_undo_step() {
        let mut s = state("abc def|");
        for _ in 0..3 {
            perform(&mut s, "EditorBackSpace");
        }
        assert_eq!(show(&s), "abc |");
        perform(&mut s, "$Undo");
        assert_eq!(show(&s), "abc def|");
        let mut s = state("abc def|");
        perform(&mut s, "EditorBackSpace");
        perform(&mut s, "EditorLeft");
        perform(&mut s, "EditorBackSpace");
        assert_eq!(show(&s), "abc |e");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "abc de", "the caret move is no undo step");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "abc def");
    }

    /// SPEC-0057 v3 behaviour 19 (AK-14).
    #[test]
    fn pasted_block_is_indented_to_the_caret_in_plain_text() {
        // SPEC-0025 v4 AK-11: three lines copied in column mode, pasted at the end of `x xz x`.
        let mut s = EditorState::new("x xz x\nab\ncd\nef");
        s.carets = Carets::new(6);
        s.clipboard = Some(Clipboard {
            text: "a\nc\ne".into(),
            whole_lines: false,
        });
        perform(&mut s, "$Paste");
        assert_eq!(s.buffer.text(), "x xz xa\n      c\n      e\nab\ncd\nef");
        assert_eq!(s.carets.primary(), Caret::at(23), "after the pasted block");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "x xz x\nab\ncd\nef", "one undo step");
        // Paste Simple and source files keep their own ways.
        s.carets = Carets::new(6);
        perform(&mut s, "EditorPasteSimple");
        assert_eq!(s.buffer.text(), "x xz xa\nc\ne\nab\ncd\nef");
        assert_eq!(
            indent_block("a\nc\n", 2),
            "a\n  c\n",
            "no line after the last break"
        );
        assert_eq!(indent_block("\nc", 2), "\nc", "starts with a break");
        assert_eq!(indent_block("a\nc", 0), "a\nc");
        let mut t = EditorState::new("\tx\n");
        t.carets = Carets::new(2);
        t.clipboard = s.clipboard.clone();
        perform(&mut t, "$Paste");
        assert_eq!(
            t.buffer.text(),
            "\txa\n     c\n     e\n",
            "a tab reaches column 4"
        );
    }

    #[test]
    fn paste_reindents_lines() {
        let paste_into = |mut s: EditorState, text: &str| {
            s.clipboard = Some(Clipboard {
                text: text.into(),
                whole_lines: false,
            });
            perform(&mut s, "$Paste");
            s
        };
        let mut s = java(7, 23);
        perform(&mut s, "EditorStartNewLine");
        let s = paste_into(s, "if (total > 0) {\nreturn total;\n}");
        assert_eq!(line(&s, 8), "        if (total > 0) {");
        assert_eq!(line(&s, 9), "            return total;");
        assert_eq!(line(&s, 10), "        }");
        let s = paste_into(
            java(13, 1),
            "                  int y = 2;\n                  int z = 3;",
        );
        assert_eq!(line(&s, 13), "    int y = 2;");
        assert_eq!(line(&s, 14), "    int z = 3;");
        let s = paste_into(java(13, 1), "/*a.trim()*/");
        assert_eq!(
            line(&s, 13),
            "    /*a.trim()*/",
            "one line onto a blank line"
        );
        let s = paste_into(java(9, 13), "int q = 0;\n");
        assert_eq!(line(&s, 9), "            int q = 0;");
        assert_eq!(line(&s, 10), "            total += x;");
        let mut plain = state("    x\n|");
        plain.clipboard = Some(Clipboard {
            text: "a\n  b".into(),
            whole_lines: false,
        });
        perform(&mut plain, "$Paste");
        assert_eq!(plain.buffer.text(), "    x\na\n  b", "plain text as is");
    }

    const WORDS: &str = "alpha albatross\nal| alto\nalbum alpha al\n";

    fn pressed(s: &mut EditorState, id: &str) -> String {
        assert!(perform(s, id), "{id}");
        show(s)
    }

    /// SPEC-0057 v5 AK-18: the nearest word before the caret, the next, the words after the
    /// caret from the end back, the prefix again, and the first word once more.
    #[test]
    fn expand_word_cycles_through_the_words() {
        let mut s = state(WORDS);
        let line = |s: &mut EditorState| {
            let shown = pressed(s, "HippieCompletion");
            shown.lines().nth(1).unwrap().to_owned()
        };
        let seen: Vec<String> = (0..6).map(|_| line(&mut s)).collect();
        assert_eq!(
            seen,
            [
                "albatross| alto",
                "alpha| alto",
                "album| alto",
                "alto| alto",
                "al| alto",
                "albatross| alto",
            ],
            "each word once, `al` itself never"
        );
        assert!(expand_word_continues(&s));
        // Inside a word: the word at the caret is not offered, the rest goes in at the caret.
        let mut s = state("foobar fo|x");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "foobar foobar|x");
        // No prefix: nothing changes, the command is used up all the same.
        let mut s = state("foo |bar");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "foo |bar");
        assert_eq!(s.buffer.top_step(), None);
        // Upper and lower case as typed.
        let mut s = state("Alpha alpha\nAl|");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "Alpha alpha\nAlpha|");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "Alpha alpha\nAl|");
    }

    /// SPEC-0057 v5 AK-18: the backward command starts after the caret; the other command inside
    /// a cycle puts the prefix back and starts its own order.
    #[test]
    fn expand_word_backward_and_switching() {
        let mut s = state(WORDS);
        let line = |s: &mut EditorState, id: &str| {
            let shown = pressed(s, id);
            shown.lines().nth(1).unwrap().to_owned()
        };
        let back = "HippieBackwardCompletion";
        let seen: Vec<String> = (0..5).map(|_| line(&mut s, back)).collect();
        assert_eq!(
            seen,
            [
                "alto| alto",
                "album| alto",
                "alpha| alto",
                "albatross| alto",
                "al| alto",
            ]
        );
        assert_eq!(line(&mut s, back), "alto| alto");
        assert_eq!(line(&mut s, "HippieCompletion"), "albatross| alto");
        assert_eq!(line(&mut s, "HippieCompletion"), "alpha| alto");
        assert_eq!(line(&mut s, back), "alto| alto");
        assert!(perform(&mut s, "$Undo"));
        assert_eq!(show(&s), WORDS, "one step, however the cycle went");
    }

    /// SPEC-0057 v5 AK-18: the words of the other open files follow the text's own, each word
    /// once.
    #[test]
    fn expand_word_takes_other_files_last() {
        let mut s = state("alpha al|");
        s.other_words = ["alpha", "alkali", "beta", "al", "alkali", "alto"]
            .map(str::to_owned)
            .to_vec();
        assert!(!expand_word_continues(&s));
        assert_eq!(pressed(&mut s, "HippieCompletion"), "alpha alpha|");
        // The cycle keeps its words: the caller need not give them again.
        s.other_words.clear();
        assert!(expand_word_continues(&s));
        assert_eq!(pressed(&mut s, "HippieCompletion"), "alpha alkali|");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "alpha alto|");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "alpha al|");
        // Only another file knows a word.
        let mut s = state("be|");
        s.other_words = vec!["beta".into()];
        assert_eq!(pressed(&mut s, "HippieBackwardCompletion"), "beta|");
        assert_eq!(
            words_of("int total = total + other_1;\n"),
            ["int", "total", "other_1"]
        );
    }

    /// SPEC-0057 v5 AK-18: one undo for a whole cycle, none after a cycle back to the prefix; a
    /// new cycle after typing or a caret move; typing before it is its own step.
    #[test]
    fn expand_word_is_one_undo_step() {
        let text = "total tomato\nto|";
        let mut s = state(text);
        assert_eq!(pressed(&mut s, "HippieCompletion"), "total tomato\ntomato|");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "total tomato\ntotal|");
        assert!(perform(&mut s, "$Undo"));
        assert_eq!(show(&s), text);
        assert_eq!(s.buffer.top_step(), None, "nothing else to undo");

        let mut s = state(text);
        for _ in 0..3 {
            perform(&mut s, "HippieCompletion");
        }
        assert_eq!(show(&s), text, "back on the prefix");
        assert_eq!(s.buffer.top_step(), None, "the cycle left no step");

        // Typing ends the cycle: the longer prefix has no word, and undo takes the typing first.
        let mut s = state(text);
        perform(&mut s, "HippieCompletion");
        type_text(&mut s, "e");
        assert_eq!(
            pressed(&mut s, "HippieCompletion"),
            "total tomato\ntomatoe|"
        );
        perform(&mut s, "$Undo");
        assert_eq!(show(&s), "total tomato\ntomato|");
        perform(&mut s, "$Undo");
        assert_eq!(show(&s), text);

        // A caret move ends it as well: `tomato` is the prefix now and nothing is longer.
        let mut s = state(text);
        perform(&mut s, "HippieCompletion");
        perform(&mut s, "EditorLeft");
        perform(&mut s, "EditorRight");
        assert!(!expand_word_continues(&s));
        assert_eq!(pressed(&mut s, "HippieCompletion"), "total tomato\ntomato|");

        // The typed prefix is not undone with the word.
        let mut s = state("total\n|");
        type_text(&mut s, "t");
        type_text(&mut s, "o");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "total\ntotal|");
        perform(&mut s, "$Undo");
        assert_eq!(show(&s), "total\nto|");
    }

    /// SPEC-0057 v5 AK-18: every caret has its own prefix and words; a selection or no prefix
    /// anywhere does nothing.
    #[test]
    fn expand_word_with_several_carets() {
        let text = "apple banana avocado\na| b| |";
        let mut s = state(text);
        let line = |s: &mut EditorState| {
            let shown = pressed(s, "HippieCompletion");
            shown.lines().nth(1).unwrap().to_owned()
        };
        assert_eq!(line(&mut s), "avocado| banana| |");
        assert_eq!(line(&mut s), "apple| b| |", "the second caret ran out");
        assert_eq!(line(&mut s), "a| b| |");
        assert_eq!(line(&mut s), "avocado| banana| |");
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "apple banana avocado\na b ");
        assert_eq!(s.buffer.top_step(), None);

        let mut s = state("alpha [al]");
        assert_eq!(pressed(&mut s, "HippieCompletion"), "alpha [al]");
        let mut s = state("alpha | |");
        assert_eq!(pressed(&mut s, "HippieBackwardCompletion"), "alpha | |");
    }

    /// State for `text` with one caret at the char offset `at`.
    fn caret_at(text: &str, at: usize) -> EditorState {
        let mut s = EditorState::new(text);
        s.carets = Carets::new(at);
        s
    }

    fn clip(s: &EditorState) -> Option<&str> {
        s.clipboard.as_ref().map(|c| c.text.as_str())
    }

    fn heads(s: &EditorState) -> Vec<usize> {
        s.carets.carets().iter().map(|c| c.head).collect()
    }

    const CUT: &str = "EditorCutLineEnd";

    /// SPEC-0057 v6 AK-19: the rest of the line, the line break when nothing but blanks is left,
    /// the last line, `\r\n`, letters outside the Basic Multilingual Plane, a selection, undo.
    #[test]
    fn cut_to_line_end_takes_the_rest_of_the_line() {
        // From the middle of a line: the line break stays.
        let mut s = caret_at("one two\nthree\n", 4);
        assert!(perform(&mut s, CUT));
        assert_eq!(s.buffer.text(), "one \nthree\n");
        assert_eq!(clip(&s), Some("two"));
        assert_eq!(heads(&s), [4]);
        assert!(!s.clipboard.as_ref().unwrap().whole_lines);
        // One undo step.
        assert!(perform(&mut s, "$Undo"));
        assert_eq!(s.buffer.text(), "one two\nthree\n");
        assert_eq!(s.buffer.top_step(), None);

        // At the line end the next line joins.
        let mut s = caret_at("one\ntwo\n", 3);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "onetwo\n");
        assert_eq!(clip(&s), Some("\n"));
        assert_eq!(heads(&s), [3]);

        // Only spaces and tabs before the line end go with the line break.
        let mut s = caret_at("one \t \ntwo", 3);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "onetwo");
        assert_eq!(clip(&s), Some(" \t \n"));

        // On the last line there is no line break to take: blanks go, an empty rest cuts nothing
        // and leaves the clipboard alone (IMP-0405).
        let mut s = caret_at("one\ntwo  ", 7);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "one\ntwo");
        assert_eq!(clip(&s), Some("  "));
        let mut s = caret_at("one\ntwo", 7);
        s.clipboard = Some(Clipboard {
            text: "kept".into(),
            whole_lines: true,
        });
        assert!(perform(&mut s, CUT));
        assert_eq!(s.buffer.text(), "one\ntwo");
        assert_eq!(clip(&s), Some("kept"));
        assert_eq!(s.buffer.top_step(), None);
        let mut s = caret_at("", 0);
        assert!(perform(&mut s, CUT));
        assert_eq!(clip(&s), None);

        // `\r\n` goes as a whole and reaches the clipboard as `\n`; a rest keeps it.
        let mut s = caret_at("one\r\ntwo\r\n", 3);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "onetwo\r\n");
        assert_eq!(clip(&s), Some("\n"));
        let mut s = caret_at("one\r\ntwo\r\n", 1);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "o\r\ntwo\r\n");
        assert_eq!(clip(&s), Some("ne"));

        // Offsets are characters, also outside the Basic Multilingual Plane.
        let mut s = caret_at("ä𝄞x𝄞é\nz", 2);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "ä𝄞\nz");
        assert_eq!(clip(&s), Some("x𝄞é"));

        // A selection plays no part: the cut starts at the caret and the selection is dropped.
        let mut s = EditorState::new("one two three\n");
        s.carets = Carets::from_carets(vec![Caret::select(0, 8)]);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "one two \n");
        assert_eq!(clip(&s), Some("three"));
        assert!(s.carets.primary().is_empty());
        let mut s = EditorState::new("one two three\n");
        s.carets = Carets::from_carets(vec![Caret::select(8, 4)]);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "one \n");
        assert_eq!(clip(&s), Some("two three"));
    }

    /// SPEC-0057 v6 AK-19: cuts in a row at the same place are one clipboard text; anything else
    /// ends the row.
    #[test]
    fn cut_to_line_end_joins_cuts_in_a_row() {
        let text = "one\ntwo\nthree\nfour\n";
        let mut s = caret_at(text, 0);
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("one"));
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("one\n"));
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("one\ntwo"));
        assert_eq!(s.buffer.text(), "\nthree\nfour\n");
        // Each cut is an undo step of its own.
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "two\nthree\nfour\n");
        // The undo ended the row.
        s.carets = Carets::new(0);
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("two"));

        // A caret move, also one that comes back, ends the row.
        let mut s = caret_at(text, 0);
        perform(&mut s, CUT);
        perform(&mut s, "EditorRight");
        perform(&mut s, "EditorLeft");
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("\n"));

        // Typing ends it.
        let mut s = caret_at("ab\ncd\n", 1);
        perform(&mut s, CUT);
        type_text(&mut s, "x");
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("\n"));
        assert_eq!(s.buffer.text(), "axcd\n");

        // Another text on the editor's clipboard ends it, and so does a caret put elsewhere.
        let mut s = caret_at(text, 0);
        perform(&mut s, CUT);
        s.clipboard = Some(Clipboard {
            text: "foreign".into(),
            whole_lines: false,
        });
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("\n"));
        let mut s = caret_at(text, 0);
        perform(&mut s, CUT);
        s.carets = Carets::new(1);
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("two"));

        // The editor lost the focus in between.
        let mut s = caret_at(text, 0);
        perform(&mut s, CUT);
        s.end_cut_row();
        perform(&mut s, CUT);
        assert_eq!(clip(&s), Some("\n"));
    }

    /// SPEC-0057 v6 AK-19: every caret cuts; the texts are a line each; nothing is joined.
    #[test]
    fn cut_to_line_end_with_several_carets() {
        let mut s = EditorState::new("one two\nthree four\nfive\n");
        s.carets = Carets::from_carets(vec![Caret::at(4), Caret::at(14)]);
        assert!(perform(&mut s, CUT));
        assert_eq!(s.buffer.text(), "one \nthree \nfive\n");
        assert_eq!(clip(&s), Some("two\nfour"));
        assert_eq!(heads(&s), [4, 11]);
        // Again: both carets take their line break, and the clipboard holds this cut only.
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "one three five\n");
        assert_eq!(clip(&s), Some("\n\n\n"));
        // One undo step for all carets.
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "one \nthree \nfive\n");

        // Two carets on one line: the overlapping ranges are cut once.
        let mut s = EditorState::new("one two three\nx");
        s.carets = Carets::from_carets(vec![Caret::at(4), Caret::at(8)]);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "one \nx");
        assert_eq!(clip(&s), Some("two three"));
        assert_eq!(heads(&s), [4]);

        // A caret with nothing to cut gives an empty line.
        let mut s = EditorState::new("ab\ncd");
        s.carets = Carets::from_carets(vec![Caret::at(1), Caret::at(5)]);
        perform(&mut s, CUT);
        assert_eq!(s.buffer.text(), "a\ncd");
        assert_eq!(clip(&s), Some("b\n"));
    }

    const MODE: &str = "EditorToggleInsertState";

    /// SPEC-0057 v6 AK-20: typed text replaces what stands after the caret, up to the line end.
    #[test]
    fn overwrite_mode_replaces_up_to_the_line_end() {
        let mut s = caret_at("abc\ndef", 1);
        assert!(!s.overwrite);
        assert!(perform(&mut s, MODE));
        assert!(s.overwrite);
        type_text(&mut s, "X");
        assert_eq!(s.buffer.text(), "aXc\ndef");
        assert_eq!(heads(&s), [2]);
        type_text(&mut s, "Y");
        // At the line end typing inserts; the line break stays.
        type_text(&mut s, "Z");
        assert_eq!(s.buffer.text(), "aXYZ\ndef");
        assert_eq!(heads(&s), [4]);

        // Before `\r\n` as well.
        let mut s = caret_at("ab\r\ncd", 1);
        perform(&mut s, MODE);
        type_text(&mut s, "x");
        type_text(&mut s, "y");
        assert_eq!(s.buffer.text(), "axy\r\ncd");

        // A longer text replaces up to the line end and inserts the rest.
        let mut s = caret_at("abc\ndef", 1);
        perform(&mut s, MODE);
        type_text(&mut s, "1234");
        assert_eq!(s.buffer.text(), "a1234\ndef");
        assert_eq!(heads(&s), [5]);

        // A letter outside the Basic Multilingual Plane is one character on both sides.
        let mut s = caret_at("a𝄞b", 1);
        perform(&mut s, MODE);
        type_text(&mut s, "x");
        assert_eq!(s.buffer.text(), "axb");
        type_text(&mut s, "𝄞");
        assert_eq!(s.buffer.text(), "ax𝄞");

        // No pair is closed, and a closer is typed over like any character.
        let mut s = caret_at("xyz", 0);
        perform(&mut s, MODE);
        type_text(&mut s, "(");
        assert_eq!(s.buffer.text(), "(yz");
        let mut s = caret_at("()", 1);
        perform(&mut s, MODE);
        type_text(&mut s, ")");
        assert_eq!((s.buffer.text().as_str(), heads(&s)), ("()", vec![2]));
        // In insert mode the pair is closed as before.
        let mut s = caret_at("", 0);
        type_text(&mut s, "(");
        assert_eq!(s.buffer.text(), "()");

        // Enter and Backspace work as in insert mode.
        let mut s = caret_at("abc", 1);
        perform(&mut s, MODE);
        type_text(&mut s, "\n");
        assert_eq!(s.buffer.text(), "a\nbc");
        perform(&mut s, "EditorBackSpace");
        assert_eq!(s.buffer.text(), "abc");
        assert!(s.overwrite, "a command does not leave the mode");
    }

    /// SPEC-0057 v6 AK-20: selections, several carets, undo of a typed run, back to insert mode.
    #[test]
    fn overwrite_mode_with_carets_selections_and_undo() {
        // The selection goes first, then the typed character replaces the next one.
        let mut s = EditorState::new("abcdef");
        s.carets = Carets::from_carets(vec![Caret::select(1, 3)]);
        perform(&mut s, MODE);
        type_text(&mut s, "X");
        assert_eq!(s.buffer.text(), "aXef");
        assert_eq!(heads(&s), [2]);

        // Every caret types; a caret's text is not taken by the one before it.
        let mut s = EditorState::new("abc\nabc");
        s.carets = Carets::from_carets(vec![Caret::at(0), Caret::at(4)]);
        perform(&mut s, MODE);
        type_text(&mut s, "x");
        type_text(&mut s, "y");
        assert_eq!(s.buffer.text(), "xyc\nxyc");
        assert_eq!(heads(&s), [2, 6]);
        // The run is one undo step.
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "abc\nabc");
        assert_eq!(s.buffer.top_step(), None);
        let mut s = EditorState::new("abcd");
        s.carets = Carets::from_carets(vec![Caret::at(0), Caret::at(1)]);
        perform(&mut s, MODE);
        type_text(&mut s, "12");
        assert_eq!(s.buffer.text(), "1212d");

        // Switching back: typing inserts again.
        let mut s = caret_at("abc", 1);
        perform(&mut s, MODE);
        perform(&mut s, MODE);
        assert!(!s.overwrite);
        type_text(&mut s, "X");
        assert_eq!(s.buffer.text(), "aXbc");
    }

    const INDENT: &str = "AutoIndentLines";

    fn java_state(text: &str) -> EditorState {
        let mut s = EditorState::new(text);
        s.language = Some("java".into());
        s
    }

    /// SPEC-0057 v6 AK-21: without a selection the caret's line is re-indented and the caret
    /// goes one line down; with one, its lines; further carets stay on their text.
    #[test]
    fn auto_indent_lines_moves_the_caret_down() {
        let text = "class A {\nint x;\n  int y;\n}\n";
        // Caret in `int x;` at column 2.
        let mut s = java_state(text);
        s.carets = Carets::new(12);
        assert!(perform(&mut s, INDENT));
        assert_eq!(s.buffer.text(), "class A {\n    int x;\n  int y;\n}\n");
        // Line 2, the column from before the change.
        assert_eq!(heads(&s), [21 + 2]);
        assert_eq!(s.message, None);
        // Again: the next line, and the caret reaches the brace's line, which is shorter.
        s.carets = Carets::new(21 + 7);
        perform(&mut s, INDENT);
        assert_eq!(s.buffer.text(), "class A {\n    int x;\n    int y;\n}\n");
        assert_eq!(heads(&s), [33]);
        // One undo step each.
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), "class A {\n    int x;\n  int y;\n}\n");

        // A line that stands right: no undo step, the caret still goes down.
        let mut s = java_state("class A {\n    int x;\n}");
        s.carets = Carets::new(14);
        perform(&mut s, INDENT);
        assert_eq!(s.buffer.top_step(), None);
        assert_eq!(heads(&s), [22]);
        // On the last line the caret stays.
        perform(&mut s, INDENT);
        assert_eq!(heads(&s), [22]);

        // A selection from line 1 to the start of line 3 takes lines 1 and 2 and stays on its
        // text; a further caret behind an indentation that grows moves with it.
        let mut s = java_state(text);
        s.carets = Carets::from_carets(vec![Caret::select(10, 26), Caret::at(27)]);
        perform(&mut s, INDENT);
        assert_eq!(s.buffer.text(), "class A {\n    int x;\n    int y;\n}\n");
        let list = s.carets.carets();
        assert_eq!((list[0].anchor, list[0].head), (10, 32));
        assert_eq!(list[1].head, 33);
        perform(&mut s, "$Undo");
        assert_eq!(s.buffer.text(), text);
        // A selection inside one line takes that line and does not move.
        let mut s = java_state(text);
        s.carets = Carets::from_carets(vec![Caret::select(11, 13)]);
        perform(&mut s, INDENT);
        assert_eq!(s.buffer.text(), "class A {\n    int x;\n  int y;\n}\n");
        let c = s.carets.primary();
        assert_eq!((c.anchor, c.head), (15, 17));
    }

    /// SPEC-0057 v6 AK-21: languages without braces have no rule; nothing changes.
    #[test]
    fn auto_indent_lines_without_rules_says_so() {
        for language in [Some("python"), Some("plaintext"), Some("yaml"), None] {
            let text = "def f():\nreturn 1\n";
            let mut s = EditorState::new(text);
            s.language = language.map(str::to_owned);
            s.carets = Carets::new(10);
            assert!(perform(&mut s, INDENT), "{language:?}");
            assert_eq!(s.buffer.text(), text);
            assert_eq!(heads(&s), [10]);
            assert_eq!(s.message.as_deref(), Some(NO_INDENT_RULE));
            assert_eq!(s.buffer.top_step(), None);
        }
    }

    /// SPEC-0056 AK-15 (v5): Complete Current Statement on the primary caret's line, the caret
    /// where the next statement starts, one undo step that puts text and caret back.
    #[test]
    fn complete_statement_is_one_undo_step() {
        let mut s = state("class A {\n    void m() {\n        call(1|, 2\n    }\n}");
        s.language = Some("java".into());
        assert!(COMMANDS.contains(&"EditorCompleteStatement"));
        assert!(perform(&mut s, "EditorCompleteStatement"));
        assert_eq!(
            show(&s),
            "class A {\n    void m() {\n        call(1, 2);\n        |\n    }\n}"
        );
        perform(&mut s, "$Undo");
        assert_eq!(
            show(&s),
            "class A {\n    void m() {\n        call(1|, 2\n    }\n}"
        );
        let mut s = state("if x > 1|");
        s.language = Some("rust".into());
        perform(&mut s, "EditorCompleteStatement");
        assert_eq!(show(&s), "if x > 1 {\n    |\n}");
        // CRLF documents keep their line breaks.
        let mut s = state("a(|\r\nb");
        s.language = Some("c".into());
        perform(&mut s, "EditorCompleteStatement");
        assert_eq!(show(&s), "a();\r\n|\r\nb");
    }
}
