//! Text buffer, edit history and cursor model.
//!
//! Implements `SPEC-0003` (text buffer with undo/redo), `SPEC-0025`
//! (selections and multiple carets), the buffer side of `SPEC-0026`
//! (reacting to changes on disk) and `SPEC-0056` (editing actions) and `SPEC-0057`
//! (editor commands by keymap action) and `SPEC-0220` (the registry of open documents) from
//! `docs/specifications/`; v3 of `SPEC-0056`/`SPEC-0057` adds [`smart`] (statements, block and
//! Markdown comments, paste indentation) and [`paste_history`]; [`reformat`] applies a formatter's
//! result with carets kept on their text (`SPEC-0329`, `SPEC-0123`); [`small`] holds the small
//! editing commands of `SPEC-0332` (sorting and reversing lines, Transpose, carets at line ends,
//! converting indentation).

mod actions;
mod buffer;
mod commands;
pub mod documents;
pub mod paste_history;
pub mod reformat;
mod selection;
pub mod small;
pub mod smart;
pub mod statement;

pub use actions::{
    CommentStyle, NewlineOptions, PAIRS, backspace_pair, delete_lines, duplicate, indent_lines,
    join_lines, matching_bracket, move_lines, smart_newline, toggle_line_comment, type_char,
};
pub use buffer::{Edit, EditError, LineEnding, Position, Reconcile, TextBuffer, TextSnapshot};
pub use commands::{
    COMMANDS, Clipboard, EXPAND_WORD_MAX, EditorState, NO_INDENT_RULE, Viewport, cut_range,
    editor_first, expand_word_continues, move_element, perform, type_text, words_of,
};
pub use selection::{Caret, Carets, Motion};
