//! The editor view on GPUI (`SPEC-0058`): draws a buffer with gutter, syntax colours, selections and
//! carets, feeds key events through the keymap into the editor commands (`SPEC-0057`), and connects
//! the platform input method for composed text.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, FocusHandle,
    Focusable, Font, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    ScrollWheelEvent, ShapedLine, TextRun, UTF16Selection, Window, canvas, div, fill, font, point,
    prelude::*, px, rgb, size,
};
use idecraft_editor::{Caret, Carets, Clipboard, EditorState};
use idecraft_languages::LanguageId;
use idecraft_languages::folding::Outline;
use idecraft_languages::syntax::SyntaxDocument;
use idecraft_ui::keymap::Keystroke;
use idecraft_vcs::{LineChange, LineChangeKind, line_changes};

use crate::diagnostics_model::{self, Problem, Severity};
use crate::layout::{self, DisplayLine};
use crate::palette::Palette;

/// The editor's line height in pixels, from Settings › Font (`SPEC-0129`).
fn line_height() -> f32 {
    crate::fonts::editor_line_height()
}

const GUTTER_PADDING: f32 = 12.0;
const TEXT_PADDING: f32 = 8.0;
/// Width of a change marker in the gutter.
const MARKER_WIDTH: f32 = 3.0;
/// Width of the folding column at the right of the gutter (`SPEC-0083`).
const FOLD_WIDTH: f32 = 14.0;
/// Width of the run marker column, shown only when the file has markers (`SPEC-0122`).
const RUN_WIDTH: f32 = 14.0;
/// Space left and right of the annotation labels (`SPEC-0277`).
const ANNOTATION_PADDING: f32 = 6.0;

fn color(c: u32) -> Hsla {
    rgb(c).into()
}

/// The chosen editor font (the bundled JetBrains Mono by default, `SPEC-0150`), with JetBrains Mono
/// and a system monospace font as fallbacks; without ligatures unless the user turned them on
/// (`SPEC-0027` v6).
fn editor_font() -> Font {
    let mut f = font(crate::fonts::editor_family());
    if !crate::fonts::editor_ligatures() {
        f.features = gpui::FontFeatures::disable_ligatures();
    }
    f.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        crate::fonts::EDITOR_FONT.into(),
        if cfg!(target_os = "macos") {
            "Menlo".into()
        } else {
            "DejaVu Sans Mono".into()
        },
    ]));
    f
}

/// The syntax styles of the line `line` (char range of the buffer) shown as `display`, by display
/// byte, from the highlight `spans` of the painted range (`SPEC-0027`).
fn syntax_runs(
    spans: &[idecraft_languages::syntax::Span],
    line: Range<usize>,
    display: &DisplayLine,
    p: &Palette,
) -> Vec<(Range<usize>, idecraft_ui::theme::TextStyle)> {
    spans
        .iter()
        .filter(|s| s.end > line.start && s.start < line.end)
        .map(|s| {
            let from = display.byte_of(s.start.max(line.start) - line.start);
            let to = display.byte_of(s.end.min(line.end) - line.start);
            (from..to, p.capture(&s.kind))
        })
        .collect()
}

/// One drawn line, kept for hit testing and the input method's candidate window.
/// IDECraft's own inspections of a parsed text as problems (`SPEC-0098` behaviour 3); words of the
/// project dictionary are no typos (`SPEC-0163`).
fn tree_problems(doc: &SyntaxDocument, path: Option<&std::path::Path>) -> Vec<Problem> {
    let found = idecraft_languages::inspections::inspect(doc);
    let found = match path {
        Some(p) => {
            let found = idecraft_languages::project_dictionary::filter(p, doc.text(), found);
            // The project's inspection profile (SPEC-0098 v3 behaviour 9, SPEC-0315).
            idecraft_languages::inspection_profile::filter(p, found)
        }
        None => found,
    };
    own_problems(doc.text(), found)
}

/// Findings with byte offsets in `text` as problems with char ranges.
fn own_problems(
    text: &str,
    mut findings: Vec<idecraft_languages::inspections::Finding>,
) -> Vec<Problem> {
    findings.sort_by_key(|f| f.start);
    // Byte offsets to char offsets in one pass.
    let mut chars = 0;
    let mut last = 0;
    let mut at = |byte: usize| {
        chars += text[last..byte].chars().count();
        last = byte;
        chars
    };
    let mut own = Vec::new();
    for f in findings {
        // "No highlighting, only fix" (SPEC-0315 v2 behaviour 7): the fix stays in ⌥↩.
        if f.level == idecraft_languages::inspections::Level::Info {
            continue;
        }
        let start = at(f.start);
        let end_chars = start + text[f.start..f.end].chars().count();
        own.push(Problem {
            range: start..end_chars.max(start + 1),
            severity: match f.level {
                idecraft_languages::inspections::Level::Error => Severity::Error,
                idecraft_languages::inspections::Level::Warning => Severity::Warning,
                idecraft_languages::inspections::Level::Weak
                | idecraft_languages::inspections::Level::Info => Severity::Weak,
            },
            message: format!("IDECraft: {}", f.message),
            inspection: Some(f.id),
            unused: idecraft_languages::inspections::is_unused(f.id),
            code: None,
        });
    }
    own
}

/// An inspection finding of a remote document from the host: id, level, message, start and end
/// as 0-based line and character (`SPEC-0318` v2 detail 9).
pub type HostFinding = (String, String, String, (usize, usize), (usize, usize));

/// A tree built in the background with its problems and fold regions.
type Parsed = (SyntaxDocument, Vec<Problem>, Outline);

/// How long the text must rest before the project's unused declarations are checked again
/// (`SPEC-0098` v12 behaviour 26).
const PROJECT_CHECK_DELAY: Duration = Duration::from_millis(300);

/// The editor's check of its file's unused declarations against the project's reference index
/// (`SPEC-0098` v12 behaviour 26, `SPEC-0316` v3 behaviour 8).
#[derive(Default)]
struct ProjectCheck {
    /// The project folder; `None` for files the index does not cover (not Java, remote).
    root: Option<PathBuf>,
    /// The file relative to it.
    rel: PathBuf,
    /// Shown problems, moved by edits until the next result.
    problems: Vec<Problem>,
    /// The findings with their fixes, as byte ranges of the text of `revision`.
    findings: Vec<idecraft_languages::inspections::Finding>,
    /// The text revision `findings` belong to.
    revision: u64,
    /// Cancels the running step.
    cancel: Arc<std::sync::atomic::AtomicBool>,
    /// Counts the checks asked for; a waiting check that is no longer the last one stops.
    asked: u64,
}

impl ProjectCheck {
    /// Stops the running step; its result is dropped.
    fn cancel(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.asked += 1;
    }

    fn clear(&mut self) {
        self.cancel();
        self.problems.clear();
        self.findings.clear();
    }
}

/// `ExpandAllToLevel3` → `(true, 3)`, `ExpandToLevel2` → `(false, 2)`: whether the whole file is
/// meant, and the level, 1 to 5 (`SPEC-0083` v6 behaviour 12).
fn fold_level(id: &str) -> Option<(bool, usize)> {
    // Every id as the keymap names it, so a search for an action id finds where it is carried out.
    Some(match id {
        "ExpandAllToLevel1" => (true, 1),
        "ExpandAllToLevel2" => (true, 2),
        "ExpandAllToLevel3" => (true, 3),
        "ExpandAllToLevel4" => (true, 4),
        "ExpandAllToLevel5" => (true, 5),
        "ExpandToLevel1" => (false, 1),
        "ExpandToLevel2" => (false, 2),
        "ExpandToLevel3" => (false, 3),
        "ExpandToLevel4" => (false, 4),
        "ExpandToLevel5" => (false, 5),
        _ => return None,
    })
}

/// Whether `id` is a folding action of the editor (`SPEC-0083` behaviours 3, 11 and 12).
fn is_fold_action(id: &str) -> bool {
    matches!(
        id,
        "CollapseRegion"
            | "ExpandRegion"
            | "CollapseAllRegions"
            | "ExpandAllRegions"
            | "CollapseRegionRecursively"
            | "ExpandRegionRecursively"
            | "CollapseBlock"
            | "CollapseSelection"
            | "CollapseDocComments"
            | "ExpandDocComments"
    ) || fold_level(id).is_some()
}

/// What the status bar says when Fold Code Block has no blocks to go by (`SPEC-0083` v7
/// behaviour 15).
pub const NO_CODE_BLOCKS: &str = "Fold Code Block: knows the code blocks of Java files only";
/// What it says when Fold Selection cannot fold (`SPEC-0083` v7 behaviour 16).
pub const FOLD_ONE_LINE: &str = "Fold Selection: the selection is within one line";
/// As [`FOLD_ONE_LINE`], for lines that partly overlap a fold.
pub const FOLD_OVERLAPS: &str = "Fold Selection: the selection partly overlaps a folded region";
/// What it says when Context Info finds no declaration (`SPEC-0057` v6 behaviour 27).
pub const NO_CONTEXT: &str = "Context Info: no declaration around the caret";

/// Where the next item at the end of a line starts and where it ends, `gap` after what is already
/// there (SPEC-0091 v15 behaviour 41).
fn place_at_line_end(end: Pixels, gap: Pixels, width: Pixels) -> (Pixels, Pixels) {
    let x = end + gap;
    (x, x + width)
}

/// `runs` with `extra` inserted at byte `at`, splitting the run there.
fn insert_run(runs: &[gpui::TextRun], at: usize, extra: gpui::TextRun) -> Vec<gpui::TextRun> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut pos = 0;
    let mut extra = Some(extra);
    for r in runs {
        if let Some(e) = extra.take_if(|_| pos + r.len > at || pos == at) {
            let before = at - pos;
            if before > 0 {
                out.push(gpui::TextRun {
                    len: before,
                    ..r.clone()
                });
            }
            out.push(e);
            if r.len > before {
                out.push(gpui::TextRun {
                    len: r.len - before,
                    ..r.clone()
                });
            }
        } else {
            out.push(r.clone());
        }
        pos += r.len;
    }
    if let Some(e) = extra {
        out.push(e);
    }
    out
}

#[derive(Clone)]
struct PaintedLine {
    line: usize,
    display: DisplayLine,
    shaped: ShapedLine,
    origin: gpui::Point<Pixels>,
    /// The usage count's row above `line` (`SPEC-0292` behaviour 4), not the line itself.
    above: bool,
}

/// Where the last frame put things.
#[derive(Clone, Default)]
struct LastLayout {
    lines: Vec<PaintedLine>,
    /// x of the line between gutter and text; the folding column ends there.
    gutter_x: Pixels,
    /// Top left corner of the editor.
    origin: gpui::Point<Pixels>,
    /// The editor's area in the window (`SPEC-0122` v5 behaviour 13).
    bounds: Bounds<Pixels>,
    /// x where the annotation column ends; the editor's left edge without annotations
    /// (`SPEC-0277` behaviour 2).
    annotation_x: Pixels,
    /// Folded one-line methods drawn: line, text colours in the placeholder, brace backgrounds
    /// (`SPEC-0083` v3 behaviour 7).
    one_line_folds: Vec<(usize, usize, usize)>,
    /// Folded lines drawn: line, the text of the line as shown and the placeholder behind it
    /// (`SPEC-0083` v8 behaviour 21).
    placeholders: Vec<(usize, String, String)>,
    /// Texts drawn at the end of lines: line and text (usage counts, `SPEC-0292` v2 behaviour 4).
    line_end_texts: Vec<(usize, String)>,
    /// Gutter icons drawn, with where, for their tooltips (`SPEC-0294` behaviour 4).
    gutter_tips: Vec<(Bounds<Pixels>, GutterIcon)>,
    /// Author hints drawn: where, line and text (`SPEC-0292` v4 behaviours 7c, 7e).
    author_hints: Vec<(Bounds<Pixels>, usize, String)>,
    /// Widths of the carets drawn, in pixels (`SPEC-0057` v6 behaviour 25).
    caret_widths: Vec<f32>,
}

/// A gutter icon with a tooltip (`SPEC-0294` behaviour 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GutterIcon {
    /// A ▶ run marker on a 0-based line.
    Run(usize),
    /// A breakpoint on a 0-based line.
    Breakpoint(usize),
    /// A folding mark on a 0-based line, folded or not.
    Fold(usize, bool),
    /// A declaration's mark on a 0-based line (`SPEC-0320` behaviour 9).
    Declaration(usize),
    /// The mark of a bean or an injection point on a 0-based line (`SPEC-0115` v5 behaviour 14).
    Framework(usize),
    /// A bookmark on a 0-based line (`SPEC-0331` behaviour 14).
    Bookmark(usize),
}

impl GutterIcon {
    /// The 0-based line the icon stands on.
    fn line(self) -> usize {
        match self {
            Self::Run(l)
            | Self::Breakpoint(l)
            | Self::Fold(l, _)
            | Self::Declaration(l)
            | Self::Framework(l)
            | Self::Bookmark(l) => l,
        }
    }
}

/// What a view shows of a document besides its text, handed to its other view
/// (`SPEC-0112` v4 behaviour 9).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decorations {
    base: Option<Arc<str>>,
    changes: Vec<LineChange>,
    semantic: crate::semantic_model::SemanticSpans,
    hints: crate::inlay_model::Hints,
    above: Vec<(usize, String)>,
    at_line_end: bool,
    authors: crate::authors_model::AuthorHints,
    /// Problems from the language server, frameworks and plugins (SPEC-0070 v8: the second view
    /// counts the same problems as the first).
    problems: [Vec<Problem>; 3],
    /// Marks of beans and injection points and resolved configuration values (`SPEC-0115` v5
    /// behaviours 13 and 14).
    framework_marks: crate::decl_marks_model::DeclMarks,
    value_folds: crate::value_fold_model::ValueFolds,
}

/// What the status bar shows about an editor.
pub struct EditorStatus {
    /// 1-based caret line.
    pub line: usize,
    /// 1-based caret column.
    pub column: usize,
    /// `LF`, `CRLF` or `CR` (`SPEC-0336` behaviour 6).
    pub line_ending: String,
    /// The encoding the file is read and written in, e.g. `UTF-8` (`SPEC-0336` behaviour 11).
    pub encoding: String,
    /// Indentation unit.
    pub indent: String,
    /// Message about saving or a change on disk (SPEC-0058 behaviours 9–10).
    pub note: Option<String>,
    /// Errors, warnings and weak warnings (SPEC-0070 behaviour 3, v5 behaviour 9).
    pub problems: (usize, usize, usize),
    /// Message of the problem under the caret (SPEC-0070 behaviour 4).
    pub problem_at_caret: Option<String>,
    /// That problem is a finding of a tool run after a change, such as ESLint, not of the
    /// language server (SPEC-0070 v11 behaviour 4c).
    pub problem_from_tool: bool,
    /// Column selection mode is on (SPEC-0151 behaviour 1).
    pub column_mode: bool,
    /// Overwrite mode is on (`SPEC-0057` v6 behaviour 25).
    pub overwrite: bool,
}

/// What an editor asks its workbench to do.
pub enum EditorEvent {
    /// Go to the declaration of the symbol at the caret (⌘-click).
    GotoDeclaration,
    /// The line breakpoints changed (`SPEC-0091` behaviour 1).
    BreakpointsChanged,
    /// Edit the breakpoint on this line (0-based): a right click on it (`SPEC-0127` behaviour 2).
    EditBreakpoint(usize),
    /// Edits that concern the bookmarked lines of this file: the workbench moves the project's
    /// bookmarks with them (`SPEC-0331` behaviours 5–7).
    BookmarkEdits(Vec<idecraft_core::bookmarks::LineEdit>),
    /// The text changed by these edits; another view of the same file adopts the buffer
    /// (`SPEC-0112` behaviour 3).
    Edited(Vec<idecraft_editor::Edit>),
    /// The file was saved; another view of it adopts the saved state.
    Saved,
    /// Save All was chosen in this editor (`SPEC-0058` v4 behaviour 12): the window saves every
    /// editor, each through `SaveDocument`.
    SaveAll,
    /// The gutter mark of a declaration on this 0-based line was clicked (`SPEC-0320` behaviour 9).
    DeclarationMark(usize),
    /// The gutter mark of a bean or an injection point on this 0-based line was clicked
    /// (`SPEC-0115` v5 behaviour 14).
    FrameworkMark(usize),
    /// The ▶ run marker on this 0-based line was clicked (`SPEC-0122` behaviour 1).
    RunMarker(usize),
    /// ⌥] (true) or ⌥[ (false) while an inline completion is shown: the next or previous
    /// suggestion (`SPEC-0162` behaviour 2).
    InlineAlternative(bool),
    /// Undo (false) or redo (true) of this undo step, which belongs to a refactoring across files:
    /// the workbench takes back the whole refactoring (`SPEC-0193` behaviours 3, 4).
    GroupUndo(u64, bool),
    /// Undo (false) or redo (true) of this undo step of the editor alone: the workbench names a
    /// refactoring taken back in the status line (`SPEC-0193` v4 behaviour 9).
    Undid(u64, bool),
    /// The annotation of a line was clicked: show this commit (full id) in the Git log
    /// (`SPEC-0277` behaviour 5).
    ShowCommit(String),
    /// The pointer rested on the problem starting at this char offset (`Some`), or left the
    /// problems (`None`): show or hide the problem popup (`SPEC-0078` v2 behaviour 5).
    ProblemHover(Option<usize>),
    /// The pointer rested on the word starting at this char offset (`Some`), or left it (`None`):
    /// show or hide its documentation (`SPEC-0077` v6 behaviour 5).
    DocHover(Option<usize>),
    /// A click on the change marker of change `.0` (an index into [`EditorView::changes`]): open its
    /// change bar at window position `.1` (`SPEC-0064` v3 behaviour 6).
    ChangeMarker(usize, gpui::Point<Pixels>),
    /// A click on a declaration's author hint: show or hide the annotations of this view
    /// (`SPEC-0292` v4 behaviour 7e).
    ToggleAnnotations,
}

impl gpui::EventEmitter<EditorEvent> for EditorView {}

/// An editor on one buffer.
pub struct EditorView {
    /// Buffer, carets, clipboard and viewport.
    pub state: EditorState,
    path: Option<PathBuf>,
    /// Syntax tree; `None` while a background re-parse holds it.
    syntax: Option<SyntaxDocument>,
    /// Buffer edits not yet applied to the syntax tree.
    syntax_edits: Vec<idecraft_editor::Edit>,
    parsing: bool,
    /// Colour runs (display byte range, colour) by line text, shown while the tree is re-parsed.
    colors: HashMap<String, Vec<(Range<usize>, idecraft_ui::theme::TextStyle)>>,
    focus: FocusHandle,
    scroll_y: f32,
    view_height: f32,
    /// The first frame has given the view its real height.
    laid_out: bool,
    /// A line revealed before that, with a guessed height: revealed again in the first frame, so
    /// a jump into a file that was not open lands in view in a short editor (SPEC-0149 v4).
    reveal_before_layout: Option<usize>,
    /// Width when last painted (0 before the first paint).
    view_width: f32,
    /// Char range composed by the input method.
    marked: Option<Range<usize>>,
    dragging: bool,
    /// Line and column where an ⌥-drag column selection started (`SPEC-0130`).
    column_drag: Option<idecraft_editor::Position>,
    /// Column selection mode: plain clicks, drags and ⇧-arrows select rectangles (SPEC-0151).
    column_mode: bool,
    /// Corner, moving corner (columns may lie past the line end) and the caret offset the last
    /// rectangle left, so ⇧-arrows go on from it while the carets stay as it made them.
    column_block: Option<(idecraft_editor::Position, idecraft_editor::Position, usize)>,
    last: LastLayout,
    /// Input that arrived and was not drawn yet (for latency measurements).
    pending_input: Option<Instant>,
    /// Time from input to the frame that showed it.
    pub latencies: Vec<Duration>,
    /// Text at HEAD the gutter markers compare against (`SPEC-0064`); `None` outside Git.
    base: Option<Arc<str>>,
    /// Gutter markers of the buffer against `base`.
    changes: Vec<LineChange>,
    diffing: bool,
    /// The buffer changed while a diff ran.
    diff_stale: bool,
    /// Char ranges of find results, sorted (`SPEC-0065` behaviour 5).
    highlights: Vec<Range<usize>>,
    /// The usages of the symbol at the caret and the question behind them (`SPEC-0295`).
    occurrences: crate::occurrence_model::Occurrences,
    /// Counts buffer changes, so observers can tell an edit from a redraw.
    revision: u64,
    /// The symbol path at the caret for the breadcrumbs, with the revision, caret and parsed text
    /// length it was computed for (`SPEC-0061` v3 behaviour 5a).
    symbol_path: std::cell::RefCell<((u64, usize, usize), Vec<String>)>,
    /// An input field: no gutter, no current-line background, no folding (find bar, commit message).
    field: bool,
    /// The text cannot be changed or saved: a library's source copied from its sources archive
    /// (`SPEC-0149` v4 behaviour 11).
    read_only: bool,
    /// A document of a remote project (`SPEC-0317` v3 behaviour 8): saving and reloading go
    /// through the session, nothing is written or read on this machine.
    remote_document: bool,
    /// Save All belongs to the project window (`SPEC-0058` v4 behaviour 12): the editor asks it
    /// with [`EditorEvent::SaveAll`] instead of saving only itself. Off for an editor of its own
    /// (`idecraft edit`), which saves its file.
    save_all_in_window: bool,
    /// IDECraft's inspection findings for a remote document, from the host (`SPEC-0318` v2
    /// detail 9): they stand instead of the window's own.
    host_problems: Vec<Problem>,
    /// The read-only state was set for a library source and does not follow the file's
    /// attribute (`SPEC-0336` behaviour 18).
    locked: bool,
    /// The encoding the file was read in and is written in (`SPEC-0336` behaviour 11).
    encoding: idecraft_core::file_properties::FileEncoding,
    /// The state of the encoding choices when the disk was last looked at.
    choice_generation: u64,
    /// The text of a read-only file, put back after any edit.
    frozen: Option<idecraft_editor::TextBuffer>,
    /// A password field: shows a dot per character and never copies its text (`SPEC-0108`).
    masked: bool,
    /// Lines (0-based) with a breakpoint, sorted; they move with edits (`SPEC-0091` behaviour 1).
    breakpoints: Vec<usize>,
    /// How breakpoints with properties look; others are plain (`SPEC-0127` behaviour 1).
    breakpoint_looks: std::collections::BTreeMap<usize, crate::debug_model::BreakpointLook>,
    /// Breakpoints are muted: drawn grey (`SPEC-0194` v3 behaviour 8).
    breakpoints_muted: bool,
    /// Tooltips of breakpoints with properties, by 0-based line (`SPEC-0294` behaviour 4).
    breakpoint_tips: std::collections::BTreeMap<usize, idecraft_ui::tooltip::Tip>,
    /// 0-based lines of breakpoints the running debug session did not bind: drawn hollow
    /// (`SPEC-0127` v5 behaviour 14).
    breakpoints_unbound: std::collections::BTreeSet<usize>,
    /// Method breakpoints on their header lines (0-based) and whether each is on: drawn as a
    /// red square (`SPEC-0146` v6 behaviour 12).
    method_breakpoints: std::collections::BTreeMap<usize, bool>,
    /// The gutter icons moved since the last render: draw again so their tooltips follow.
    gutter_moved: bool,
    /// Line (0-based) where the debugged program is stopped (`SPEC-0091` behaviour 4).
    execution_line: Option<usize>,
    /// Values the debugger shows after lines, 0-based line and text (`SPEC-0091` v5 behaviour 13).
    inline_values: Vec<(usize, String)>,
    /// Notes after lines by their source (`SPEC-0192` behaviours 5 and 7: run times, unsafe-change
    /// warnings).
    line_notes: Vec<(&'static str, crate::sql_editor_model::LineNote)>,
    /// An inline completion shown as grey text at the caret: the caret offset it belongs to, the
    /// buffer revision it was asked for and the text (`SPEC-0157` behaviour 4).
    ghost: Option<(usize, u64, String)>,
    /// Breakpoints moved with an edit since the last event about them.
    breakpoints_moved: bool,
    /// The bookmarks of this file as the project keeps them, and the file's name there
    /// (`SPEC-0331` behaviour 14). They move with edits; the workbench hears of every edit.
    bookmarks: (String, idecraft_core::bookmarks::Bookmarks),
    /// Edits since the last event about them, while the file has bookmarks.
    bookmark_edits: Vec<idecraft_core::bookmarks::LineEdit>,
    /// The buffer is being taken over from another view of the file: that view reports the edits.
    adopting: bool,
    /// Lines (0-based) with a ▶ run marker (`SPEC-0122`).
    run_markers: Vec<usize>,
    /// Marks of declarations that override or are overridden (`SPEC-0320` behaviour 8).
    declaration_marks: crate::decl_marks_model::DeclMarks,
    /// Marks of beans and injection points, in a lane of their own (`SPEC-0115` v5 behaviour 14).
    framework_marks: crate::decl_marks_model::DeclMarks,
    /// Resolved configuration values shown in place of their placeholders (`SPEC-0115` v5
    /// behaviour 13).
    value_folds: crate::value_fold_model::ValueFolds,
    /// The file's entry in the registry of open documents, which mirrors the text for MCP tools
    /// (`SPEC-0220` behaviour 2); `None` without a path. Dropping it unregisters the view.
    document: Option<idecraft_editor::documents::OpenDocument>,
    /// File text as last read from or written to disk, to notice changes by others (behaviour 9).
    disk: Option<String>,
    /// Workspace whose local history records saves (behaviour 9).
    history_root: Option<PathBuf>,
    /// Undo steps that belong to a refactoring across files (`SPEC-0193`).
    group_steps: HashSet<u64>,
    /// Message for the status bar.
    note: Option<String>,
    /// The declaration Context Info shows: its 0-based line, and the text revision, primary
    /// caret and scroll position it was shown for; it is shown only while these still hold
    /// (`SPEC-0057` v6 behaviour 27).
    context: Option<(usize, u64, usize, f32)>,
    /// The file changed on disk; the next save overwrites it anyway.
    overwrite_armed: bool,
    /// Modification time and size seen by the last disk check, to skip unchanged files cheaply.
    disk_stamp: Option<(std::time::SystemTime, u64)>,
    /// Problems from the language server, shifted along with edits (SPEC-0070).
    /// Problems shown: those of the language server and IDECraft's own inspections, by position.
    problems: Vec<Problem>,
    /// The highlighting level of the file (`SPEC-0070` v6 behaviour 11).
    highlight: diagnostics_model::HighlightLevel,
    /// Problems the level hides.
    hidden_by_level: usize,
    /// The language server's problems.
    server_problems: Vec<Problem>,
    /// Findings of IDECraft's own inspections for the parsed text (`SPEC-0098`).
    own_problems: Vec<Problem>,
    /// Findings of framework models, e.g. Spring beans (`SPEC-0115`).
    framework_problems: Vec<Problem>,
    /// The unused declarations of the project index (`SPEC-0098` v12 behaviour 26).
    project: ProjectCheck,
    /// Findings of plugins' inspections (`SPEC-0166`).
    plugin_problems: Vec<Problem>,
    /// A plugin's file type for a file without a language of IDECraft's own (`SPEC-0210`), and
    /// its spans for a revision of the text.
    plugin_file_type: Option<idecraft_plugins::FileType>,
    plugin_spans: (u64, Vec<idecraft_languages::syntax::Span>),
    /// Folded regions (`SPEC-0083`).
    folds: crate::fold_model::Folds,
    /// The language server's semantic tokens, moved by edits until the next answer (`SPEC-0290`).
    /// The SQL dialect of a `.sql` file and the char ranges of its block comments by the
    /// splitter's rules (`SPEC-0005` v5 behaviour 19).
    sql_dialect: Option<idecraft_database::Dialect>,
    sql_comments: Vec<std::ops::Range<usize>>,
    semantic: crate::semantic_model::SemanticSpans,
    /// The language server's parameter name hints, moved by edits (`SPEC-0291`).
    hints: crate::inlay_model::Hints,
    /// Authors of the declarations (`SPEC-0292` v4 behaviour 7).
    authors: crate::authors_model::AuthorHints,
    /// Open the change bar of this change once its line is drawn (`SPEC-0064` v3 behaviour 7).
    pending_bar: Option<usize>,
    /// The one-line methods were folded (or the file had none) when its first tree came
    /// (`SPEC-0083` v2 behaviour 7).
    folded_on_open: bool,
    /// The line a jump landed on before the one-line methods were folded: its method stays open
    /// (`SPEC-0071` v7 behaviour 10).
    jumped_to: Option<usize>,
    /// Foldable regions from the syntax tree and the revision they belong to.
    fold_regions: (u64, Outline),
    /// The folding arrows of open regions are drawn only while the pointer is over the editor
    /// (`SPEC-0083` v8 behaviour 22).
    fold_arrows_on_hover: bool,
    /// The pointer is over the editor.
    pointer_inside: bool,
    /// The first keystroke of a shortcut of two, waiting for the second (`SPEC-0083` v8
    /// behaviour 23).
    first_stroke: Option<Keystroke>,
    /// The file's EditorConfig properties (`SPEC-0137`).
    editorconfig: idecraft_core::editorconfig::EditorConfig,
    /// What the `.editorconfig` files above the file held when they were read last
    /// (`SPEC-0137` v2 behaviour 2b).
    editorconfig_stamp: Vec<Option<u64>>,
    /// Columns a tab advances to.
    tab_width: usize,
    /// Successful saves so far, so the workbench can ask for fresh diagnostics.
    saves: u64,
    /// Annotations with Git blame, mapped onto the buffer (`SPEC-0277`).
    annotations: Option<crate::blame_model::Annotations>,
    /// A remap of the annotations runs; the buffer changed while it ran.
    remapping: bool,
    remap_stale: bool,
    /// The annotation under the pointer: its line and the pointer in window coordinates.
    annotation_hover: Option<(usize, gpui::Point<Pixels>)>,
    /// The problem under the pointer (its start) and whether its popup was asked for
    /// (`SPEC-0078` v2 behaviour 5).
    problem_hover: Option<(usize, bool)>,
    /// The word under the pointer (its start) and whether its documentation was asked for
    /// (`SPEC-0077` v6 behaviour 5).
    doc_hover: Option<(usize, bool)>,
}

impl EditorView {
    /// Whether the text differs from the last save.
    pub fn is_modified(&self) -> bool {
        self.state.buffer.is_modified()
    }

    /// An editor on `text`; `path` selects the language and is where saving writes.
    pub fn new(text: &str, path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut state = EditorState::new(text);
        state.buffer.track_changes();
        let path_text = path.as_ref().map(|_| text.to_owned());
        let language = path.as_deref().and_then(LanguageId::detect);
        state.comment = language
            .and_then(LanguageId::line_comment)
            .map(str::to_owned);
        // Block comments, statements and paste indentation know the language (SPEC-0056 v3,
        // SPEC-0057 v3).
        state.block_comment = language
            .and_then(LanguageId::block_comment)
            .map(|(o, c)| (o.to_owned(), c.to_owned()));
        state.language = language.map(|l| l.lsp_id().to_owned());
        let tsx = path
            .as_deref()
            .and_then(|p| p.extension())
            .is_some_and(|e| e == "tsx");
        // A large file gets its tree on a background thread and shows plain text until then; a
        // very large one stays plain text.
        let background = text.len() > idecraft_languages::syntax::BACKGROUND_PARSE_BYTES;
        let tree_language =
            language.filter(|_| text.len() <= idecraft_languages::syntax::MAX_SYNTAX_BYTES);
        let syntax = tree_language
            .filter(|_| !background)
            .and_then(|l| SyntaxDocument::new(l, tsx, text).ok());
        let mut view = Self {
            state,
            path,
            syntax,
            syntax_edits: Vec::new(),
            parsing: false,
            colors: HashMap::new(),
            focus: cx.focus_handle(),
            scroll_y: 0.0,
            view_height: 600.0,
            laid_out: false,
            reveal_before_layout: None,
            view_width: 0.0,
            marked: None,
            dragging: false,
            column_drag: None,
            column_mode: false,
            column_block: None,
            last: LastLayout::default(),
            pending_input: None,
            latencies: Vec::new(),
            base: None,
            changes: Vec::new(),
            diffing: false,
            diff_stale: false,
            annotations: None,
            remapping: false,
            remap_stale: false,
            annotation_hover: None,
            problem_hover: None,
            doc_hover: None,
            highlights: Vec::new(),
            occurrences: crate::occurrence_model::Occurrences::default(),
            revision: 0,
            symbol_path: Default::default(),
            field: false,
            read_only: false,
            remote_document: false,
            save_all_in_window: false,
            host_problems: Vec::new(),
            locked: false,
            encoding: Default::default(),
            choice_generation: idecraft_core::file_properties::choice_generation(),
            frozen: None,
            masked: false,
            breakpoints: Vec::new(),
            breakpoint_looks: Default::default(),
            breakpoints_muted: false,
            breakpoint_tips: std::collections::BTreeMap::new(),
            breakpoints_unbound: std::collections::BTreeSet::new(),
            method_breakpoints: Default::default(),
            gutter_moved: false,
            execution_line: None,
            inline_values: Vec::new(),
            line_notes: Vec::new(),
            ghost: None,
            breakpoints_moved: false,
            bookmarks: Default::default(),
            bookmark_edits: Vec::new(),
            adopting: false,
            run_markers: Vec::new(),
            declaration_marks: Default::default(),
            framework_marks: Default::default(),
            value_folds: Default::default(),
            document: None,
            disk: path_text,
            history_root: None,
            group_steps: HashSet::new(),
            note: None,
            context: None,
            overwrite_armed: false,
            disk_stamp: None,
            problems: Vec::new(),
            highlight: Default::default(),
            hidden_by_level: 0,
            server_problems: Vec::new(),
            own_problems: Vec::new(),
            framework_problems: Vec::new(),
            project: ProjectCheck::default(),
            plugin_problems: Vec::new(),
            plugin_file_type: None,
            plugin_spans: (u64::MAX, Vec::new()),
            folds: crate::fold_model::Folds::default(),
            semantic: crate::semantic_model::SemanticSpans::default(),
            sql_dialect: None,
            sql_comments: Vec::new(),
            hints: crate::inlay_model::Hints::default(),
            authors: crate::authors_model::AuthorHints::default(),
            pending_bar: None,
            folded_on_open: false,
            jumped_to: None,
            fold_regions: (u64::MAX, Outline::default()),
            fold_arrows_on_hover: true,
            pointer_inside: false,
            first_stroke: None,
            saves: 0,
            editorconfig: Default::default(),
            editorconfig_stamp: Vec::new(),
            tab_width: crate::layout::TAB_WIDTH,
        };
        if let Some(language) = tree_language.filter(|_| background) {
            view.parse_in_background(language, tsx, text.to_owned(), cx);
        }
        view.reload_editorconfig();
        view.refresh_inspections();
        view.fold_on_open(cx);
        view.document = view.path.as_ref().map(|path| {
            idecraft_editor::documents::OpenDocument::open(
                idecraft_editor::documents::DocumentRegistry::global(),
                path,
                view.state.buffer.snapshot(),
            )
        });
        view
    }

    /// Mirrors the text and its saved state into the registry of open documents (`SPEC-0220`).
    fn publish_document(&self) {
        if let Some(doc) = &self.document {
            doc.publish(
                self.state.buffer.snapshot(),
                self.state.buffer.is_modified(),
            );
        }
    }

    /// Runs IDECraft's own inspections again (after the project dictionary changed, `SPEC-0163`).
    pub fn reinspect(&mut self, cx: &mut Context<Self>) {
        self.refresh_inspections();
        self.check_project(cx);
        cx.notify();
    }

    /// From now on checks this file's unused declarations against the reference index of the
    /// project at `root` (`SPEC-0098` v12 behaviour 26, v13 behaviour 27); only a Java or Kotlin
    /// file below `root` that is not a remote document.
    pub fn set_project_root(&mut self, root: &std::path::Path, cx: &mut Context<Self>) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let rel = path
            .strip_prefix(root)
            .map(std::path::Path::to_path_buf)
            .or_else(|_| {
                let (Ok(p), Ok(r)) = (path.canonicalize(), root.canonicalize()) else {
                    return Err(());
                };
                p.strip_prefix(&r)
                    .map(std::path::Path::to_path_buf)
                    .map_err(|_| ())
            });
        let Ok(rel) = rel else {
            return;
        };
        if self.remote_document
            || self.field
            || rel.extension().is_none_or(|e| e != "java" && e != "kt")
        {
            return;
        }
        self.project.rel = rel;
        self.project.root = Some(root.to_path_buf());
        self.check_project(cx);
    }

    /// Checks the unused declarations of the current tree against the project's index once the
    /// text has rested (`SPEC-0098` v12 behaviour 26): the step runs on a background thread and
    /// its result is installed only for the text it was made for; an edit cancels it. Called
    /// again when the project's index was refreshed.
    pub fn check_project(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.root.clone() else {
            return;
        };
        self.project.cancel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.project.cancel = cancel.clone();
        let asked = self.project.asked;
        let rel = self.project.rel.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PROJECT_CHECK_DELAY).await;
            // The tree that matches the text; a parse that is still running asks again when it
            // ends.
            let Ok(Some((snapshot, revision))) = this.update(cx, |this, _| {
                if this.project.asked != asked {
                    return None;
                }
                let doc = this
                    .syntax
                    .as_ref()
                    .filter(|_| this.syntax_edits.is_empty())?;
                Some((doc.snapshot(), this.revision))
            }) else {
                return;
            };
            let found = cx
                .background_executor()
                .spawn(async move {
                    idecraft_languages::reference_index::editor_unused_of(
                        &root, &rel, &snapshot, &cancel,
                    )
                    .map(|found| (found, snapshot))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some((found, snapshot)) = found else {
                    return;
                };
                if this.project.asked != asked || this.revision != revision {
                    return;
                }
                this.project.problems = own_problems(snapshot.text(), found.clone());
                this.project.findings = found;
                this.project.revision = revision;
                this.merge_problems();
                cx.notify();
            });
        })
        .detach();
    }

    /// The findings of the project's unused declarations at byte offset `at` of the current text,
    /// with their fixes (`SPEC-0098` v12 behaviour 26); none while they belong to an older text.
    pub fn project_findings_at(&self, at: usize) -> Vec<idecraft_languages::inspections::Finding> {
        if self.project.revision != self.revision {
            return Vec::new();
        }
        self.project
            .findings
            .iter()
            .filter(|f| f.start <= at && at <= f.end)
            .cloned()
            .collect()
    }

    /// Reads the `.editorconfig` files above the file again and applies indentation and tab width
    /// (`SPEC-0137` behaviour 2); saving follows the rest.
    pub fn reload_editorconfig(&mut self) {
        self.editorconfig_stamp = match &self.path {
            Some(path) if !self.field => idecraft_core::editorconfig::change_stamp(path),
            _ => Vec::new(),
        };
        let config = match &self.path {
            Some(path) if !self.field => idecraft_core::editorconfig::editorconfig_for(path),
            _ => Default::default(),
        };
        // Code Style gives what `.editorconfig` leaves open (SPEC-0150 behaviour 2).
        let style = match &self.path {
            Some(path) if !self.field => {
                crate::fonts::code_style().indent_for(crate::fonts::code_style_language(path))
            }
            _ => idecraft_core::CodeStyleSettings::default_indent("Default"),
        };
        self.state.indent = config.indent_unit().unwrap_or_else(|| style.unit());
        self.tab_width = config.tab_width().unwrap_or(style.tab as usize);
        // Convert Indents measures with the file's tab width (SPEC-0332 behaviour 11).
        self.state.tab_width = self.tab_width;
        self.editorconfig = config;
    }

    /// The file's EditorConfig properties.
    pub fn editorconfig(&self) -> &idecraft_core::editorconfig::EditorConfig {
        &self.editorconfig
    }

    /// Reads the `.editorconfig` files again when one above the file was created, changed or
    /// deleted since they were read last, by IDECraft or another program (`SPEC-0137` v2
    /// behaviour 2b); true when it did. The text is not touched.
    pub fn check_editorconfig(&mut self) -> bool {
        let Some(path) = self
            .path
            .as_ref()
            .filter(|_| !self.field && !self.remote_document)
        else {
            return false;
        };
        if idecraft_core::editorconfig::change_stamp(path) == self.editorconfig_stamp {
            return false;
        }
        self.reload_editorconfig();
        true
    }

    /// The column after which the right margin is drawn, if any: `max_line_length` of the
    /// `.editorconfig` first, else Code Style (`SPEC-0137` v2 behaviour 2a, `SPEC-0150`
    /// behaviour 3).
    pub fn right_margin_column(&self) -> Option<u32> {
        if self.field {
            return None;
        }
        margin_column(&self.editorconfig, &crate::fonts::code_style())
    }

    /// The indent unit one Tab inserts here.
    pub fn indent_unit(&self) -> &str {
        &self.state.indent
    }

    /// Columns a tab advances to.
    pub fn tab_width(&self) -> usize {
        self.tab_width
    }

    /// An input field without gutter (find bar query `SPEC-0065`, commit message `SPEC-0063`).
    pub fn field(cx: &mut Context<Self>) -> Self {
        Self {
            field: true,
            read_only: false,
            frozen: None,
            ..Self::new("", None, cx)
        }
    }

    /// A field for a password: a dot per character, and copy and cut do nothing (`SPEC-0108`).
    pub fn password_field(cx: &mut Context<Self>) -> Self {
        Self {
            masked: true,
            ..Self::field(cx)
        }
    }

    /// Size (width, height) when last painted; a field drawn 0 px wide or high cannot be clicked
    /// into (hand tests 5 and 6, 2026-11-01). Before the first paint the width is 0.
    pub fn painted_size(&self) -> (f32, f32) {
        (
            self.view_width,
            if self.view_width > 0.0 {
                self.view_height
            } else {
                0.0
            },
        )
    }

    /// Marks when an input event started, for the latency measurement; the time runs until the
    /// frame showing it is painted.
    pub fn mark_input(&mut self, at: Instant) {
        self.pending_input = Some(at);
    }

    fn painted(&mut self, cx: &mut Context<Self>) {
        if let Some(at) = self.pending_input.take() {
            self.latencies.push(at.elapsed());
        }
        // The bar of a change gone to opens once its line is drawn (SPEC-0064 v3 behaviour 7).
        if let Some(index) = self.pending_bar
            && let Some(at) = self.change_bar_anchor(index)
        {
            self.pending_bar = None;
            cx.emit(EditorEvent::ChangeMarker(index, at));
        }
    }

    /// Replaces the whole text (e.g. clearing a commit message after the commit, or a remote
    /// session's text taking over). The file keeps its indentation and tab width from
    /// `.editorconfig` and Code Style (`SPEC-0137` behaviour 2); they fell back to four spaces
    /// before.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let mut state = self.state.with_text(text);
        state.buffer.track_changes();
        self.state = state;
        self.syntax = None;
        self.syntax_edits.clear();
        self.own_problems.clear();
        self.project.clear();
        self.merge_problems();
        self.line_notes.clear();
        self.semantic.clear();
        self.hints.clear();
        self.folds.set_above(Vec::new());
        self.marked = None;
        self.scroll_y = 0.0;
        self.revision += 1;
        self.refresh_sql_comments();
        self.publish_document();
        self.remap_annotations(cx);
        cx.notify();
    }

    /// Replaces the whole text with a proposed or current value and selects it, the caret at its
    /// end: typing replaces the value, End or an arrow key keeps it. Every field that opens with
    /// a name to accept or overwrite is filled through this (`SPEC-0335` v2).
    pub fn set_text_selected(&mut self, text: &str, cx: &mut Context<Self>) {
        self.set_text(text, cx);
        let len = self.state.buffer.len_chars();
        self.select_range(0..len, cx);
    }

    /// Sets the text the gutter markers compare against (HEAD), or `None` for no markers
    /// (`SPEC-0064` behaviour 1).
    pub fn set_base(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        let base: Option<Arc<str>> = base.map(Into::into);
        if base == self.base {
            return;
        }
        self.base = base;
        if self.base.is_none() {
            self.changes.clear();
            cx.notify();
        }
        self.rediff(cx);
    }

    /// Marks find results in the text (`SPEC-0065` behaviour 5); `ranges` are sorted char ranges.
    pub fn set_highlights(&mut self, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        if ranges != self.highlights {
            self.highlights = ranges;
            cx.notify();
        }
    }

    /// What the occurrence marks see of this editor (`SPEC-0295` behaviour 1).
    fn occurrence_place(&self) -> crate::occurrence_model::Place {
        let primary = self.state.carets.primary();
        crate::occurrence_model::Place {
            revision: self.revision,
            caret: primary.head,
            single: self.state.carets.carets().len() == 1 && primary.is_empty(),
        }
    }

    /// The name the primary caret is in or next to, if any (`SPEC-0295` behaviour 1).
    pub fn name_at_caret(&self) -> Option<String> {
        let at = self
            .state
            .buffer
            .offset_to_position(self.state.carets.primary().head)
            .ok()?;
        let line = self.state.buffer.line(at.line)?;
        let name = crate::usages_model::word_at(line.trim_end_matches(['\n', '\r']), at.column);
        (!name.is_empty()).then_some(name)
    }

    /// The question for the usages at the caret that is due at `now`, if any: the caret is
    /// observed, the automatic marks follow the setting, and a text over the size limit is not
    /// asked about by itself (`SPEC-0295` behaviours 1, 3, 11 and 12). `ready` says whether the
    /// file's language server can be asked.
    pub fn due_occurrences(
        &mut self,
        now: Instant,
        automatic: bool,
        ready: impl FnOnce() -> bool,
        cx: &mut Context<Self>,
    ) -> Option<crate::occurrence_model::Request> {
        let mut changed = self.occurrences.caret_at(self.occurrence_place(), now);
        if !automatic {
            changed |= self.occurrences.automatic_off();
        }
        if changed {
            cx.notify();
        }
        let small =
            self.state.buffer.len_chars() <= idecraft_languages::occurrences::MAX_TEXT_BYTES;
        let on_name = automatic && small && self.name_at_caret().is_some();
        self.occurrences
            .due(now, automatic && small, on_name, ready)
    }

    /// `HighlightUsagesInFile`: the usages of the symbol at the caret are asked for next and kept
    /// (`SPEC-0295` behaviour 8).
    pub fn keep_occurrences(&mut self, now: Instant) {
        self.occurrences.keep(self.occurrence_place(), now);
    }

    /// `GotoNextElementUnderCaretUsage` and `GotoPrevElementUnderCaretUsage`: with the only caret
    /// on a marked usage the step is taken at once; otherwise the caller has to ask the server
    /// (`SPEC-0295` v2 behaviour 15).
    pub fn step_occurrence(
        &mut self,
        forward: bool,
        cx: &mut Context<Self>,
    ) -> crate::occurrence_model::StepNow {
        use crate::occurrence_model::{StepNow, StepTo};
        let place = self.occurrence_place();
        if !place.single {
            return StepNow::Nothing;
        }
        if self.occurrences.caret_at(place, Instant::now()) {
            cx.notify();
        }
        match self.occurrences.step(place, forward) {
            Some(to) => {
                if let StepTo::Usage(at) = to {
                    self.jump_to_offset(at, cx);
                }
                StepNow::Done(to)
            }
            None => StepNow::Ask,
        }
    }

    /// A step from a caret that is on no mark: its question is asked next; with `mark` the
    /// answer is also marked (`SPEC-0295` v2 behaviour 15).
    pub fn want_occurrence_step(&mut self, now: Instant, forward: bool, mark: bool) {
        self.occurrences
            .step_wanted(self.occurrence_place(), now, forward, mark);
    }

    /// Puts the caret at a char offset, opens a fold around it and scrolls it into view.
    fn jump_to_offset(&mut self, at: usize, cx: &mut Context<Self>) {
        let at = at.min(self.state.buffer.len_chars());
        self.folds.open_at(self.line_of(at));
        self.note = None;
        self.select_range(at..at, cx);
    }

    /// The answer to the question `request`: the server's occurrences, or `None` when there is
    /// no answer. An answer for another text is dropped (`SPEC-0295` behaviour 10). The answer
    /// to a step's question also moves the caret; where to is returned (v2 behaviour 15).
    pub fn take_occurrences(
        &mut self,
        request: &crate::occurrence_model::Request,
        found: Option<&[idecraft_languages::occurrences::Occurrence]>,
        cx: &mut Context<Self>,
    ) -> (
        crate::occurrence_model::Taken,
        Option<crate::occurrence_model::StepTo>,
    ) {
        let (id, revision) = (request.id, request.revision);
        let buffer = &self.state.buffer;
        let marks = found.filter(|_| revision == self.revision).map(|found| {
            crate::occurrence_model::marks_of(found, |line, character| {
                buffer
                    .position_to_offset(idecraft_editor::Position::new(
                        line as usize,
                        character as usize,
                    ))
                    .ok()
            })
        });
        let stepping = request.step.zip(marks.clone());
        // The model drops the answer when its question was about another text than this one.
        let taken = self.occurrences.answer(id, self.revision, marks);
        let mut stepped = None;
        if matches!(taken, crate::occurrence_model::Taken::Shown { .. }) {
            if let Some((forward, marks)) = stepping {
                stepped = crate::occurrence_model::step_among(&marks, request.caret, forward);
                if let Some(crate::occurrence_model::StepTo::Usage(at)) = stepped {
                    self.jump_to_offset(at, cx);
                }
            }
            cx.notify();
        }
        (taken, stepped)
    }

    /// The marked usages of the symbol at the caret, sorted (`SPEC-0295`).
    pub fn occurrence_marks(&self) -> &[crate::occurrence_model::Mark] {
        self.occurrences.marks()
    }

    /// Whether the marked usages are kept until Escape (`SPEC-0295` behaviour 8).
    pub fn occurrences_kept(&self) -> bool {
        self.occurrences.is_kept()
    }

    /// The automatic highlight was switched off (`SPEC-0295` behaviour 12).
    pub fn occurrences_off(&mut self, cx: &mut Context<Self>) {
        if self.occurrences.automatic_off() {
            cx.notify();
        }
    }

    /// Selects `range` with a single caret and scrolls it into view (the current find result).
    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let len = self.state.buffer.len_chars();
        self.state.carets = Carets::from_carets(vec![Caret::select(
            range.start.min(len),
            range.end.min(len),
        )]);
        let line = self.line_of(range.start);
        self.reveal_line(line);
        cx.notify();
    }

    /// Selects each of `ranges` with a caret of its own (the places of one snippet tab stop,
    /// `SPEC-0072` v7 behaviour 18) and scrolls the first into view.
    pub fn select_ranges(&mut self, ranges: &[Range<usize>], cx: &mut Context<Self>) {
        let Some(first) = ranges.first() else {
            return;
        };
        let len = self.state.buffer.len_chars();
        self.state.carets = Carets::from_carets(
            ranges
                .iter()
                .map(|r| Caret::select(r.start.min(len), r.end.min(len)))
                .collect(),
        );
        let line = self.line_of(first.start);
        self.reveal_line(line);
        cx.notify();
    }

    /// Shows ▶ run markers on these 0-based lines (`SPEC-0122` behaviour 1).
    pub fn set_run_markers(&mut self, mut lines: Vec<usize>, cx: &mut Context<Self>) {
        lines.sort_unstable();
        lines.dedup();
        if lines != self.run_markers {
            self.run_markers = lines;
            cx.notify();
        }
    }

    /// Shows the gutter marks of the text of `revision`; an answer for another text is ignored.
    pub fn set_declaration_marks(
        &mut self,
        revision: u64,
        marks: &[idecraft_languages::ts_structure::DeclarationMark],
        cx: &mut Context<Self>,
    ) {
        if revision != self.revision {
            return;
        }
        let marks = crate::decl_marks_model::DeclMarks::from_marks(marks);
        if marks != self.declaration_marks {
            self.declaration_marks = marks;
            cx.notify();
        }
    }

    /// The gutter mark on a 0-based line.
    pub fn declaration_mark(&self, line: usize) -> Option<&crate::decl_marks_model::LineMark> {
        self.declaration_marks.at(line)
    }

    /// The width of the lane of the declarations' marks.
    fn declaration_width(&self) -> Pixels {
        if self.field || self.declaration_marks.is_empty() {
            px(0.0)
        } else {
            px(RUN_WIDTH)
        }
    }

    /// The width of both lanes of marks: the declarations' and, left of it, the frameworks'.
    fn marks_width(&self) -> Pixels {
        let framework = if self.field || self.framework_marks.is_empty() {
            px(0.0)
        } else {
            px(RUN_WIDTH)
        };
        self.declaration_width() + framework
    }

    /// Shows the marks of beans and injection points and the resolved configuration values of
    /// the text `checked` (`SPEC-0115` v5 behaviours 13 and 14); they are dropped when the text
    /// no longer matches. `folds` are byte ranges of `checked` with the text shown in their place.
    pub fn set_framework_decorations(
        &mut self,
        checked: &str,
        marks: Vec<crate::decl_marks_model::LineMark>,
        folds: &[(Range<usize>, String)],
        cx: &mut Context<Self>,
    ) {
        let text = self.state.buffer.text();
        let (marks, folds) = if text == checked {
            let chars = |b: usize| text.get(..b).map_or(0, |s| s.chars().count());
            (
                crate::decl_marks_model::DeclMarks::from_lines(marks),
                crate::value_fold_model::ValueFolds::new(
                    folds
                        .iter()
                        .map(|(r, shown)| crate::value_fold_model::ValueFold {
                            range: chars(r.start)..chars(r.end),
                            text: shown.clone(),
                        })
                        .collect(),
                ),
            )
        } else {
            Default::default()
        };
        if marks != self.framework_marks || folds != self.value_folds {
            self.framework_marks = marks;
            self.value_folds = folds;
            cx.notify();
        }
    }

    /// The mark of a bean or an injection point on a 0-based line.
    pub fn framework_mark(&self, line: usize) -> Option<&crate::decl_marks_model::LineMark> {
        self.framework_marks.at(line)
    }

    /// The resolved configuration values shown now: 0-based line, the placeholder as written and
    /// the text in its place. A placeholder a caret touches is shown as written and left out.
    pub fn shown_values(&self) -> Vec<(usize, String, String)> {
        let carets: Vec<Range<usize>> = self
            .state
            .carets
            .carets()
            .iter()
            .map(|c| c.range())
            .collect();
        let all = 0..self.state.buffer.len_chars();
        self.value_folds
            .shown_within(all, &carets)
            .into_iter()
            .map(|f| {
                (
                    self.line_of(f.range.start),
                    self.state.buffer.slice(f.range.clone()).unwrap_or_default(),
                    f.text.clone(),
                )
            })
            .collect()
    }

    /// Lines (0-based) with a ▶ run marker.
    pub fn run_markers(&self) -> &[usize] {
        &self.run_markers
    }

    fn run_width(&self) -> Pixels {
        if self.field || self.run_markers.is_empty() {
            px(0.0)
        } else {
            px(RUN_WIDTH)
        }
    }

    /// Lines (0-based) with a breakpoint.
    pub fn breakpoints(&self) -> &[usize] {
        &self.breakpoints
    }

    /// Replaces the breakpoints with saved ones (0-based lines; those past the end are dropped).
    pub fn set_breakpoints(&mut self, mut lines: Vec<usize>, cx: &mut Context<Self>) {
        let count = self.state.buffer.len_lines();
        lines.retain(|l| *l < count);
        lines.sort_unstable();
        lines.dedup();
        self.breakpoints = lines;
        cx.notify();
    }

    /// How the breakpoints with properties look, by 0-based line.
    pub fn set_breakpoint_looks(
        &mut self,
        looks: std::collections::BTreeMap<usize, crate::debug_model::BreakpointLook>,
        cx: &mut Context<Self>,
    ) {
        self.breakpoint_looks = looks;
        cx.notify();
    }

    /// The 0-based lines of breakpoints the running session did not bind; empty without a session
    /// (`SPEC-0127` v5 behaviour 14).
    pub fn set_breakpoints_unbound(
        &mut self,
        lines: std::collections::BTreeSet<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.breakpoints_unbound != lines {
            self.breakpoints_unbound = lines;
            cx.notify();
        }
    }

    /// The method breakpoints of this file: 0-based header line and whether it is on
    /// (`SPEC-0146` v6 behaviour 12).
    pub fn set_method_breakpoints(
        &mut self,
        lines: std::collections::BTreeMap<usize, bool>,
        cx: &mut Context<Self>,
    ) {
        if self.method_breakpoints != lines {
            self.method_breakpoints = lines;
            cx.notify();
        }
    }

    /// The editor's area as the last frame painted it, in window coordinates.
    pub fn painted_bounds(&self) -> gpui::Bounds<Pixels> {
        self.last.bounds
    }

    /// Whether a method breakpoint is on the 0-based `line`, and whether it is on.
    pub fn method_breakpoint(&self, line: usize) -> Option<bool> {
        self.method_breakpoints.get(&line).copied()
    }

    /// Whether the breakpoint on the 0-based `line` is drawn hollow, not bound by the session.
    pub fn breakpoint_unbound(&self, line: usize) -> bool {
        self.breakpoints_unbound.contains(&line)
    }

    /// Draws the breakpoints grey while they are muted (`SPEC-0194` v3 behaviour 8).
    pub fn set_breakpoints_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        if self.breakpoints_muted != muted {
            self.breakpoints_muted = muted;
            cx.notify();
        }
    }

    /// Whether the breakpoints are drawn as muted.
    pub fn breakpoints_muted(&self) -> bool {
        self.breakpoints_muted
    }

    /// How the breakpoint on `line` looks.
    pub fn breakpoint_look(&self, line: usize) -> crate::debug_model::BreakpointLook {
        self.breakpoint_looks
            .get(&line)
            .copied()
            .unwrap_or_default()
    }

    /// The tooltips of breakpoints with properties, by 0-based line (`SPEC-0294` behaviour 4);
    /// others say they are plain line breakpoints.
    pub fn set_breakpoint_tips(
        &mut self,
        tips: std::collections::BTreeMap<usize, idecraft_ui::tooltip::Tip>,
    ) {
        self.breakpoint_tips = tips;
    }

    /// Invisible boxes over the gutter icons of the last frame, each with its tooltip (`SPEC-0294`
    /// behaviour 4). They take no clicks; the editor below handles them.
    fn gutter_tip_overlays(&mut self) -> Vec<gpui::AnyElement> {
        use crate::tips::Tipped;
        self.gutter_moved = false;
        let origin = self.last.origin;
        self.last
            .gutter_tips
            .iter()
            .enumerate()
            .map(|(i, (b, icon))| {
                let (name, tip) = self.gutter_icon_tip(*icon);
                div()
                    .id(("gutter-tip", i))
                    .absolute()
                    .left(b.origin.x - origin.x)
                    .top(b.origin.y - origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .tip_text(name, tip)
                    .into_any_element()
            })
            .collect()
    }

    /// The tooltips of the gutter icons drawn on 0-based `line` in the last frame, left to right
    /// (`EditorShowGutterIconTooltip`, `SPEC-0294` v5 behaviour 12).
    pub fn gutter_tips_on_line(&self, line: usize) -> Vec<idecraft_ui::tooltip::Tip> {
        let mut on: Vec<_> = self
            .last
            .gutter_tips
            .iter()
            .filter(|(_, icon)| icon.line() == line)
            .collect();
        on.sort_by(|a, b| {
            a.0.origin
                .x
                .partial_cmp(&b.0.origin.x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        on.into_iter()
            .map(|(_, icon)| self.gutter_icon_tip(*icon).1)
            .collect()
    }

    /// The name and the tooltip of a gutter icon (`SPEC-0294` behaviour 4).
    fn gutter_icon_tip(&self, icon: GutterIcon) -> (String, idecraft_ui::tooltip::Tip) {
        match icon {
            GutterIcon::Run(line) => (
                format!("gutter-run-{}", line + 1),
                match self.request_tip(line) {
                    Some(tip) => tip,
                    None => idecraft_ui::tooltip::run_marker(
                        &crate::keymaps::current(),
                        crate::tips::macos(),
                    ),
                },
            ),
            GutterIcon::Breakpoint(line) => (
                format!("gutter-breakpoint-{}", line + 1),
                self.breakpoint_tips.get(&line).cloned().unwrap_or_else(|| {
                    idecraft_ui::tooltip::breakpoint(
                        idecraft_ui::tooltip::BreakpointFacts::default(),
                    )
                }),
            ),
            GutterIcon::Fold(line, folded) => (
                format!("gutter-fold-{}", line + 1),
                idecraft_ui::tooltip::fold_mark(folded),
            ),
            GutterIcon::Bookmark(line) => (
                format!("gutter-bookmark-{}", line + 1),
                idecraft_ui::tooltip::Tip {
                    title: "Bookmark".into(),
                    details: self
                        .bookmark_at(line)
                        .map(|b| {
                            b.mnemonic
                                .map(|m| format!("Mnemonic {m}"))
                                .into_iter()
                                .chain(b.description.clone())
                                .collect()
                        })
                        .unwrap_or_default(),
                    ..Default::default()
                },
            ),
            GutterIcon::Declaration(line) => (
                format!("gutter-declaration-{}", line + 1),
                idecraft_ui::tooltip::Tip::plain(
                    self.declaration_marks
                        .at(line)
                        .map_or(String::new(), |m| m.tip.clone()),
                ),
            ),
            GutterIcon::Framework(line) => (
                format!("gutter-framework-{}", line + 1),
                idecraft_ui::tooltip::Tip::plain(
                    self.framework_marks
                        .at(line)
                        .map_or(String::new(), |m| m.tip.clone()),
                ),
            ),
        }
    }

    /// Shows `bookmarks`, the project's bookmarks of this file (named `file` there), in the
    /// gutter (`SPEC-0331` behaviour 14).
    pub fn set_bookmarks(
        &mut self,
        file: String,
        bookmarks: idecraft_core::bookmarks::Bookmarks,
        cx: &mut Context<Self>,
    ) {
        if self.bookmarks.0 != file || self.bookmarks.1 != bookmarks {
            self.bookmarks = (file, bookmarks);
            cx.notify();
        }
    }

    /// The bookmarks shown in the gutter, newest first.
    pub fn bookmarks(&self) -> &[idecraft_core::bookmarks::Bookmark] {
        self.bookmarks.1.list()
    }

    /// The bookmark on the 0-based `line`.
    fn bookmark_at(&self, line: usize) -> Option<&idecraft_core::bookmarks::Bookmark> {
        self.bookmarks.1.at(&self.bookmarks.0, line)
    }

    /// `edit`, already applied to the buffer, as the bookmarked lines see it.
    fn line_edit(&self, edit: &idecraft_editor::Edit) -> idecraft_core::bookmarks::LineEdit {
        let buffer = &self.state.buffer;
        let at = buffer
            .offset_to_position(edit.offset.min(buffer.len_chars()))
            .ok();
        let line = at.as_ref().map_or(0, |p| p.line);
        let head_blank = buffer.line(line).is_none_or(|text| {
            let column = at.as_ref().map_or(0, |p| p.column);
            text.chars().take(column).all(char::is_whitespace)
        });
        let after = edit.offset + edit.inserted.chars().count();
        idecraft_core::bookmarks::LineEdit {
            line,
            head_blank,
            tail_empty: buffer.char_at(after).is_none_or(|c| c == '\n' || c == '\r'),
            deleted: edit.deleted.clone(),
            inserted: edit.inserted.clone(),
        }
    }

    /// Sets or removes the breakpoint on `line` (0-based).
    pub fn toggle_breakpoint(&mut self, line: usize, cx: &mut Context<Self>) {
        match self.breakpoints.binary_search(&line) {
            Ok(i) => {
                self.breakpoints.remove(i);
            }
            Err(i) => self.breakpoints.insert(i, line),
        }
        cx.emit(EditorEvent::BreakpointsChanged);
        cx.notify();
    }

    /// Marks the line where the debugged program stopped, or none.
    pub fn set_execution_line(&mut self, line: Option<usize>, cx: &mut Context<Self>) {
        if line.is_none() && !self.inline_values.is_empty() {
            self.inline_values.clear();
            cx.notify();
        }
        if self.execution_line != line {
            self.execution_line = line;
            if let Some(l) = line {
                self.reveal_line(l);
            }
            cx.notify();
        }
    }

    /// The line where the debugged program stopped, if shown here.
    pub fn execution_line(&self) -> Option<usize> {
        self.execution_line
    }

    /// Shows debugger values after lines (0-based); an empty list removes them.
    pub fn set_inline_values(&mut self, values: Vec<(usize, String)>, cx: &mut Context<Self>) {
        if self.inline_values != values {
            self.inline_values = values;
            cx.notify();
        }
    }

    /// Replaces the notes after lines that come from `source` (`SPEC-0192` behaviours 5 and 7).
    pub fn set_line_notes(
        &mut self,
        source: &'static str,
        notes: Vec<crate::sql_editor_model::LineNote>,
        cx: &mut Context<Self>,
    ) {
        let before = self.line_notes.clone();
        self.line_notes.retain(|(s, _)| *s != source);
        self.line_notes
            .extend(notes.into_iter().map(|n| (source, n)));
        if self.line_notes != before {
            cx.notify();
        }
    }

    /// The notes after lines, by source.
    pub fn line_notes(&self) -> &[(&'static str, crate::sql_editor_model::LineNote)] {
        &self.line_notes
    }

    /// Shows `text` as an inline completion at char offset `at`, if the caret is still there and
    /// the text did not change since `revision` (`SPEC-0157` behaviour 4).
    pub fn set_ghost(&mut self, at: usize, revision: u64, text: String, cx: &mut Context<Self>) {
        let primary = self.state.carets.primary();
        if text.is_empty()
            || self.field
            || revision != self.revision
            || self.state.carets.carets().len() != 1
            || !primary.is_empty()
            || primary.head != at
        {
            return;
        }
        self.ghost = Some((at, revision, text));
        cx.notify();
    }

    /// The inline completion shown, if any.
    pub fn ghost(&self) -> Option<&str> {
        self.ghost
            .as_ref()
            .filter(|(at, rev, _)| *rev == self.revision && *at == self.state.carets.primary().head)
            .map(|(_, _, t)| t.as_str())
    }

    /// Inserts the shown inline completion (Tab); returns whether there was one.
    pub fn accept_ghost(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.ghost().map(str::to_owned) else {
            return false;
        };
        let at = self.state.carets.primary().head;
        self.ghost = None;
        self.replace_ranges(&[(at..at, text.clone())], cx);
        let end = at + text.chars().count();
        self.select_range(end..end, cx);
        true
    }

    /// Hides the inline completion.
    pub fn dismiss_ghost(&mut self, cx: &mut Context<Self>) -> bool {
        let shown = self.ghost().is_some();
        self.ghost = None;
        if shown {
            cx.notify();
        }
        shown
    }

    /// The debugger values shown after lines.
    pub fn inline_values(&self) -> &[(usize, String)] {
        &self.inline_values
    }

    /// Moves breakpoints after an edit at line `line` that removed `removed` and added `added` line
    /// breaks: later lines shift, lines that disappeared give their breakpoint to `line`.
    fn shift_breakpoints(&mut self, line: usize, removed: usize, added: usize) {
        if removed == 0 && added == 0 {
            return;
        }
        let before = self.breakpoints.clone();
        self.breakpoints = crate::debug_model::shift_lines(&self.breakpoints, line, removed, added);
        if self.breakpoints != before {
            self.breakpoints_moved = true;
        }
    }

    /// Whether the char at `offset` is inside a comment or a string, by the syntax highlighting
    /// (false for plain text or while the syntax tree is not ready).
    pub fn in_comment_or_string(&self, offset: usize) -> bool {
        self.syntax.as_ref().is_some_and(|d| {
            d.highlight_range(offset..offset + 1)
                .iter()
                .any(|s| s.kind.starts_with("comment") || s.kind.starts_with("string"))
        })
    }

    /// Unfolds what hides `line` and scrolls its row into view (`SPEC-0083` behaviour 4).
    fn reveal_line(&mut self, line: usize) {
        if !self.laid_out {
            self.reveal_before_layout = Some(line);
        }
        self.folds.reveal(line);
        self.open_fold_under_caret();
        let row = self.folds.row_of_line(line);
        self.scroll_y = layout::reveal(self.scroll_y, self.view_height, line_height(), row);
    }

    /// Opens the fold on the primary caret's line when the caret stands behind the part of that
    /// line that is still shown, in text the placeholder stands for (`SPEC-0083` v8 behaviour
    /// 21). Returns whether a fold was opened.
    fn open_fold_under_caret(&mut self) -> bool {
        let head = self.state.carets.primary().head;
        let line = self.line_of(head);
        match self.folds.kept_chars(line) {
            Some(keep) if head - self.line_start(line) > keep => {
                self.folds.unfold(line);
                true
            }
            _ => false,
        }
    }

    /// After folding: a primary caret on a hidden line goes to the end of the first line of its
    /// fold, and one behind the shown part of a folded line to the end of that part
    /// (`SPEC-0083` behaviour 3, v8 behaviour 21).
    fn settle_caret_in_folds(&mut self) {
        let mut line = self.line_of(self.state.carets.primary().head);
        if self.folds.hidden(line) {
            let start = self.folds.line_of_row(self.folds.row_of_line(line));
            let end = self.line_start(start + 1).saturating_sub(1);
            self.state.carets = Carets::new(end.max(self.line_start(start)));
            line = start;
        }
        if let Some(keep) = self.folds.kept_chars(line) {
            let start = self.line_start(line);
            if self.state.carets.primary().head - start > keep {
                self.state.carets = Carets::new(start + keep);
            }
        }
    }

    /// The foldable regions of the current text with their looks, from the syntax tree when it
    /// is up to date (`SPEC-0030`, `SPEC-0083` v8); empty for plain text.
    fn outline(&mut self) -> &Outline {
        if self.fold_regions.0 != self.revision
            && self.syntax_edits.is_empty()
            && !self.parsing
            && let Some(doc) = &self.syntax
        {
            self.fold_regions = (self.revision, Outline::of(doc));
            self.folds.set_looks(self.fold_regions.1.looks.clone());
        }
        &self.fold_regions.1
    }

    /// Foldable regions of the current text as first and last line.
    fn regions(&mut self) -> Vec<(usize, usize)> {
        self.outline().lines.clone()
    }

    /// Draws the folding arrows of open regions all the time (`true`) or only while the
    /// pointer is over the editor (`SPEC-0083` v8 behaviour 22).
    pub fn set_fold_arrows_always(&mut self, always: bool, cx: &mut Context<Self>) {
        if self.fold_arrows_on_hover == always {
            self.fold_arrows_on_hover = !always;
            cx.notify();
        }
    }

    /// Tells the editor whether the pointer is over it (the hover of the view; tests).
    pub fn set_pointer_inside(&mut self, inside: bool, cx: &mut Context<Self>) {
        if self.pointer_inside != inside {
            self.pointer_inside = inside;
            cx.notify();
        }
    }

    /// The folding arrows of the last frame: line and whether its region is folded
    /// (`SPEC-0083` v8 behaviour 22).
    pub fn painted_fold_arrows(&self) -> Vec<(usize, bool)> {
        self.last
            .gutter_tips
            .iter()
            .filter_map(|(_, icon)| match *icon {
                GutterIcon::Fold(line, folded) => Some((line, folded)),
                _ => None,
            })
            .collect()
    }

    /// The folded lines of the last frame: line, the text of the line as shown and the
    /// placeholder behind it (`SPEC-0083` v8 behaviour 21).
    pub fn painted_placeholders(&self) -> &[(usize, String, String)] {
        &self.last.placeholders
    }

    /// What every folded region shows: first line, last line and placeholder (`SPEC-0083` v8
    /// behaviour 21).
    pub fn fold_placeholders(&self) -> Vec<(usize, usize, String)> {
        self.folds
            .regions()
            .iter()
            .map(|&(s, e)| {
                let text = self.folds.shown_placeholder(s).unwrap_or_default();
                (s, e, text.to_owned())
            })
            .collect()
    }

    /// Runs a folding action at the caret (`SPEC-0083` behaviour 3); returns whether `id` is one.
    fn fold_action(&mut self, id: &str) -> bool {
        let line = self.line_of(self.state.carets.primary().head);
        let outline = self.outline().clone();
        let regions = outline.lines.clone();
        match id {
            "CollapseRegion" => {
                // The innermost open region around the caret.
                if let Some(&(s, e)) = regions
                    .iter()
                    .filter(|&&(s, e)| s <= line && line <= e && !self.folds.is_folded(s))
                    .min_by_key(|&&(s, e)| e - s)
                {
                    self.folds.fold(s, e);
                    if line > s {
                        let end = self.line_start(s + 1).saturating_sub(1);
                        self.state.carets = Carets::new(end.max(self.line_start(s)));
                    }
                }
            }
            "ExpandRegion" => {
                if self.folds.folded_at(line).is_some() {
                    self.folds.unfold(line);
                } else {
                    self.folds.reveal(line);
                }
            }
            // Everything but the outermost container (SPEC-0083 v8 behaviour 18).
            "CollapseAllRegions" => {
                for (s, e) in outline.collapse_all() {
                    self.folds.fold(s, e);
                }
                let line = self.line_of(self.state.carets.primary().head);
                if self.folds.hidden(line) {
                    let row = self.folds.row_of_line(line);
                    let start = self.folds.line_of_row(row);
                    self.state.carets = Carets::new(self.line_start(start));
                }
            }
            "ExpandAllRegions" => self.folds.clear(),
            // With everything inside (SPEC-0083 v6 behaviour 11).
            "CollapseRegionRecursively" => {
                if let Some(&(s, e)) = regions
                    .iter()
                    .filter(|&&(s, e)| s <= line && line <= e && !self.folds.is_folded(s))
                    .min_by_key(|&&(s, e)| e - s)
                {
                    self.folds.fold_keeping(s, e, &regions);
                }
            }
            "ExpandRegionRecursively" => {
                if let Some((s, e)) = self
                    .folds
                    .folded_at(line)
                    .or_else(|| crate::fold_model::innermost_around(&regions, line))
                {
                    self.folds.unfold_within(s, e);
                }
            }
            // The innermost code block that is still open (SPEC-0083 v7 behaviour 15).
            "CollapseBlock" => {
                let head = self.state.carets.primary().head;
                let java =
                    self.path.as_deref().and_then(LanguageId::detect) == Some(LanguageId::Java);
                let blocks = java
                    .then(|| self.with_syntax(|d| d.code_blocks_around(head)))
                    .flatten();
                match blocks {
                    Some(blocks) => {
                        if let Some((s, e)) = crate::fold_model::block_to_fold(&blocks, &self.folds)
                        {
                            self.folds.fold(s, e);
                        }
                    }
                    None => self.note = Some(NO_CODE_BLOCKS.into()),
                }
            }
            // A region from the selection, or the region at the caret (SPEC-0083 v7 behaviour
            // 16).
            "CollapseSelection" => {
                use crate::fold_model::SelectionFold;
                let primary = self.state.carets.primary();
                let selection = (!primary.is_empty()).then(|| {
                    let range = primary.range();
                    let buffer = &self.state.buffer;
                    // A line break at the selection's end is not counted.
                    let mut end = range.end;
                    if buffer.char_at(end - 1) == Some('\n') {
                        end -= 1;
                    }
                    if end > range.start && buffer.char_at(end - 1) == Some('\r') {
                        end -= 1;
                    }
                    (self.line_of(range.start), self.line_of(end))
                });
                match crate::fold_model::selection_fold(&self.folds, &regions, selection, line) {
                    SelectionFold::Fold(s, e) => {
                        self.folds.fold(s, e);
                        if selection.is_some() {
                            let length = self.state.buffer.line(s).map_or(0, |t| t.chars().count());
                            self.state.carets = Carets::new(self.line_start(s) + length);
                        }
                    }
                    SelectionFold::Unfold(start) => self.folds.unfold(start),
                    SelectionFold::OneLine => self.note = Some(FOLD_ONE_LINE.into()),
                    SelectionFold::Overlaps => self.note = Some(FOLD_OVERLAPS.into()),
                    SelectionFold::Nothing => {}
                }
            }
            // The documentation comments, wherever they are (SPEC-0083 v8 behaviour 20).
            "CollapseDocComments" => {
                for &(s, e) in &outline.docs {
                    self.folds.fold(s, e);
                }
            }
            "ExpandDocComments" => {
                self.folds.forget_kept(&outline.docs);
                for &(s, e) in &outline.docs {
                    if self.folds.folded_at(s) == Some((s, e)) {
                        self.folds.unfold(s);
                    }
                }
            }
            // Down to a level (SPEC-0083 v6 behaviour 12; v8 behaviour 18: the outermost
            // container is no level).
            _ => match fold_level(id) {
                Some((true, level)) => {
                    self.folds.clear();
                    for (s, e) in outline.all_to_level(level) {
                        self.folds.fold_keeping(s, e, &regions);
                    }
                }
                Some((false, level)) => {
                    if let Some((within, folds)) = outline.to_level(level, line) {
                        self.folds.unfold_within(within.0, within.1);
                        for (s, e) in folds {
                            self.folds.fold_keeping(s, e, &regions);
                        }
                    }
                }
                None => return false,
            },
        }
        // A caret that is hidden now goes to the end of the first line of its fold.
        self.settle_caret_in_folds();
        let line = self.line_of(self.state.carets.primary().head);
        let row = self.folds.row_of_line(line);
        self.scroll_y = layout::reveal(self.scroll_y, self.view_height, line_height(), row);
        true
    }

    /// Folded regions, sorted (for tests and the self-test).
    pub fn folded_regions(&self) -> Vec<(usize, usize)> {
        self.folds.regions().to_vec()
    }

    /// The custom regions of the file between comment markers, in the order of their lines
    /// (`SPEC-0083` v9 behaviour 24): the ones the syntax tree gives for folding; none for
    /// plain text and until the file is parsed.
    pub fn custom_regions(&self) -> Vec<idecraft_languages::folding::CustomRegion> {
        self.syntax
            .as_ref()
            .map(|doc| idecraft_languages::folding::custom_regions(&doc.folding()))
            .unwrap_or_default()
    }

    /// Replaces char ranges (sorted, not overlapping) as one undo step; the caret goes to the end of
    /// the last replacement (`SPEC-0065` behaviour 8).
    pub fn replace_ranges(&mut self, edits: &[(Range<usize>, String)], cx: &mut Context<Self>) {
        // A read-only file keeps its text (SPEC-0149 v4 behaviour 11).
        if self.read_only {
            return;
        }
        if edits.is_empty() {
            return;
        }
        let before = (self.state.buffer.top_step(), self.state.carets.clone());
        let buffer = &mut self.state.buffer;
        buffer.begin_group();
        for (range, text) in edits.iter().rev() {
            let _ = buffer.replace(range.clone(), text);
        }
        buffer.end_group();
        let shift: isize = edits
            .iter()
            .map(|(r, t)| t.chars().count() as isize - r.len() as isize)
            .sum();
        let (last, text) = &edits[edits.len() - 1];
        let end = (last.start as isize + shift
            - (text.chars().count() as isize - last.len() as isize)) as usize
            + text.chars().count();
        self.state.carets = Carets::new(end.min(self.state.buffer.len_chars()));
        // Undo puts the carets back where they were before the step, redo where they are after
        // it (`SPEC-0057` v7 behaviour 9); a caller that places them itself notes them again.
        if self.state.buffer.top_step() != before.0 {
            self.state.buffer.note_carets(&before.1, &self.state.carets);
        }
        self.after_change(true, cx);
        cx.notify();
    }

    /// Notes the carets as those after the undo step made since `since` was the newest, for redo
    /// (`SPEC-0057` v7 behaviour 9): for an edit whose caller placed the carets after making the
    /// step.
    fn note_carets_after(&mut self, since: Option<u64>) {
        if self.state.buffer.top_step() != since {
            self.state.buffer.note_carets_after(&self.state.carets);
        }
    }

    /// Replaces the text with a formatter's result `new` as one undo step, by small edits where
    /// lines differ, and keeps every caret and selection on its text (`SPEC-0329` v3 behaviour 7,
    /// `SPEC-0123` v3 behaviours 3 and 4). Returns whether anything changed.
    pub fn apply_reformat(&mut self, new: &str, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        let old = self.state.buffer.text();
        let edits = idecraft_editor::reformat::reformat_edits(&old, new);
        if edits.is_empty() {
            return false;
        }
        let carets: Vec<Caret> = self
            .state
            .carets
            .carets()
            .iter()
            .map(|c| {
                let map = |o| idecraft_editor::reformat::map_offset(&old, &edits, o);
                Caret::select(map(c.anchor), map(c.head))
            })
            .collect();
        let primary =
            idecraft_editor::reformat::map_offset(&old, &edits, self.state.carets.primary().head);
        let top = self.state.buffer.top_step();
        self.replace_ranges(&edits, cx);
        let len = self.state.buffer.len_chars();
        self.state.carets = Carets::from_carets(
            carets
                .into_iter()
                .map(|c| Caret::select(c.anchor.min(len), c.head.min(len)))
                .collect(),
        );
        self.note_carets_after(top);
        let line = self.line_of(primary.min(len));
        self.reveal_line(line);
        cx.notify();
        true
    }

    /// Why the editor does not take an agent's edit (`SPEC-0220` v2 behaviour 8), if it does not.
    pub fn agent_edit_refusal(&self) -> Option<&'static str> {
        if self.read_only {
            Some("the file is read-only in the editor")
        } else if self.remote_document || self.path.is_none() {
            Some("the editor does not show this file")
        } else {
            None
        }
    }

    /// An agent's change of this file (`SPEC-0220` v2 behaviour 7): the text becomes `new` as one
    /// undo step that replaces only what differs, carets staying on their text, and is saved when
    /// `save` says so (the buffer had no unsaved changes). A read-only or remote editor refuses.
    pub fn apply_agent_edit(
        &mut self,
        new: &str,
        save: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(why) = self.agent_edit_refusal() {
            return Err(why.into());
        }
        self.apply_reformat(new, cx);
        if save {
            // No tidying on save: the file gets exactly the agent's text.
            self.save();
            cx.emit(EditorEvent::Saved);
        }
        cx.notify();
        Ok(())
    }

    /// Shows findings of a tool (a framework model, `SPEC-0115` behaviours 4 and 11, labelled
    /// with the framework, `SPEC-0134`; ESLint, `SPEC-0123` behaviour 2) with their severity, given as byte ranges of the text they were made for; they are dropped when the
    /// text no longer matches. Messages read `<label>: <message>`.
    pub fn set_tool_problems(
        &mut self,
        label: &str,
        checked: &str,
        found: &[(
            std::ops::Range<usize>,
            crate::diagnostics_model::Severity,
            String,
        )],
        cx: &mut Context<Self>,
    ) {
        let text = self.state.buffer.text();
        self.framework_problems = if text == checked {
            let chars = |b: usize| text.get(..b).map_or(0, |s| s.chars().count());
            found
                .iter()
                .filter(|(r, _, _)| r.start < r.end)
                .map(|(r, severity, m)| Problem {
                    range: chars(r.start)..chars(r.end),
                    severity: *severity,
                    message: format!("{label}: {m}"),
                    inspection: None,
                    unused: false,
                    code: None,
                })
                .collect()
        } else {
            Vec::new()
        };
        self.merge_problems();
        cx.notify();
    }

    /// Highlights and comments the file as a plugin's file type (`SPEC-0210` behaviour 3); only for
    /// files without a language of IDECraft's own, `None` to stop.
    pub fn set_plugin_file_type(
        &mut self,
        file_type: Option<idecraft_plugins::FileType>,
        cx: &mut Context<Self>,
    ) {
        let own = self.path.as_deref().and_then(LanguageId::detect);
        let file_type = file_type.filter(|_| own.is_none());
        if file_type == self.plugin_file_type {
            return;
        }
        self.state.comment = match &file_type {
            Some(t) => t.line_comment.clone(),
            None => own.and_then(LanguageId::line_comment).map(str::to_owned),
        };
        self.plugin_file_type = file_type;
        self.plugin_spans = (u64::MAX, Vec::new());
        self.colors.clear();
        cx.notify();
    }

    /// Kinds of the plugin file type's spans last painted (self-test).
    pub fn plugin_span_kinds(&self) -> Vec<String> {
        self.plugin_spans.1.iter().map(|s| s.kind.clone()).collect()
    }

    /// The plugin file type's name, if one highlights this file.
    pub fn plugin_file_type(&self) -> Option<&str> {
        self.plugin_file_type.as_ref().map(|t| t.name.as_str())
    }

    /// The tooltip of the run mark of the request at 0-based `line` of a request file: "Send GET
    /// url" (`SPEC-0096` v4 behaviour 6).
    fn request_tip(&self, line: usize) -> Option<idecraft_ui::tooltip::Tip> {
        if !self
            .path
            .as_deref()
            .is_some_and(crate::http_model::is_request_file)
        {
            return None;
        }
        let file = idecraft_http::parse(&self.state.buffer.text());
        let r = file.requests.iter().find(|r| r.line == line + 1)?;
        Some(
            idecraft_ui::tooltip::Tip::plain(format!("Send {} {}", r.method, r.url))
                .with_detail("Click to send the request"),
        )
    }

    /// Spans of the plugin file type, or of a request file, intersecting the char range `range`,
    /// highlighted once per revision.
    fn plugin_spans_in(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> Option<Vec<idecraft_languages::syntax::Span>> {
        // A request file is coloured by IDECraft itself (`SPEC-0096` v4 behaviour 5).
        let request = self.plugin_file_type.is_none()
            && self
                .path
                .as_deref()
                .is_some_and(crate::http_model::is_request_file);
        if self.plugin_file_type.is_none() && !request {
            return None;
        }
        if self.plugin_spans.0 != self.revision {
            let text = self.state.buffer.text();
            let spans = match &self.plugin_file_type {
                Some(file_type) => idecraft_plugins::highlight(file_type, &text)
                    .into_iter()
                    .map(|(r, kind)| idecraft_languages::syntax::Span {
                        start: r.start,
                        end: r.end,
                        kind: kind.capture().to_owned(),
                    })
                    .collect(),
                None => crate::http_model::spans(&text),
            };
            self.plugin_spans = (self.revision, spans);
        }
        Some(
            self.plugin_spans
                .1
                .iter()
                .filter(|s| s.end > range.start && s.start < range.end)
                .cloned()
                .collect(),
        )
    }

    /// Shows plugins' findings (`SPEC-0166` behaviour 2) as character ranges of `checked`, the text
    /// they were made for; they are dropped when the text no longer matches. Messages read
    /// `<plugin>: <message>`.
    pub fn set_plugin_problems(
        &mut self,
        checked: &str,
        found: Vec<(
            std::ops::Range<usize>,
            crate::diagnostics_model::Severity,
            String,
        )>,
        cx: &mut Context<Self>,
    ) {
        self.plugin_problems = if self.state.buffer.text() == checked {
            found
                .into_iter()
                .map(|(range, severity, message)| Problem {
                    range,
                    severity,
                    message,
                    inspection: None,
                    unused: false,
                    code: None,
                })
                .collect()
        } else {
            Vec::new()
        };
        self.merge_problems();
        cx.notify();
    }

    /// The SQL dialect of the file, for comments that nest (`SPEC-0005` v5 behaviour 19); `None`
    /// for other files.
    pub fn set_sql_dialect(
        &mut self,
        dialect: Option<idecraft_database::Dialect>,
        cx: &mut Context<Self>,
    ) {
        if self.sql_dialect != dialect {
            self.sql_dialect = dialect;
            self.refresh_sql_comments();
            cx.notify();
        }
    }

    /// Reads the block comments of a SQL file again after its text changed.
    fn refresh_sql_comments(&mut self) {
        self.sql_comments = match self.sql_dialect {
            Some(d) => crate::sql_editor_model::nested_comment_ranges(&self.state.buffer.text(), d),
            None => Vec::new(),
        };
    }

    /// `spans` of the char range `range` with the SQL file's block comments coloured as such.
    fn comment_spans(
        &self,
        spans: Vec<idecraft_languages::syntax::Span>,
        range: std::ops::Range<usize>,
    ) -> Vec<idecraft_languages::syntax::Span> {
        crate::sql_editor_model::with_comments(spans, &self.sql_comments, range)
    }

    /// Shows the language server's semantic tokens for the text of `revision` (`SPEC-0290`
    /// behaviours 2–5); an answer for an older text is ignored. Returns whether it was taken.
    pub fn set_semantic_tokens(
        &mut self,
        revision: u64,
        tokens: &[idecraft_languages::semantic::SemanticToken],
        cx: &mut Context<Self>,
    ) -> bool {
        if revision != self.revision {
            return false;
        }
        let buffer = &self.state.buffer;
        let spans = crate::semantic_model::SemanticSpans::from_tokens(tokens, |l| {
            buffer
                .position_to_offset(idecraft_editor::Position::new(l as usize, 0))
                .ok()
        });
        if spans != self.semantic {
            self.semantic = spans;
            cx.notify();
        }
        true
    }

    /// Shows the language server's parameter name hints for the text of `revision` (`SPEC-0291`
    /// behaviours 4–5); an answer for an older text is ignored. Returns whether it was taken.
    pub fn set_parameter_hints(
        &mut self,
        revision: u64,
        hints: &[idecraft_languages::inlay::ParameterHint],
        cx: &mut Context<Self>,
    ) -> bool {
        if revision != self.revision {
            return false;
        }
        let buffer = &self.state.buffer;
        let hints = crate::inlay_model::Hints::from_hints(hints, |l| {
            buffer
                .position_to_offset(idecraft_editor::Position::new(l as usize, 0))
                .ok()
        });
        if hints != self.hints {
            self.hints = hints;
            cx.notify();
        }
        true
    }

    /// Shows the usage counts of the text of `revision` on rows above their declarations
    /// (`SPEC-0292` behaviours 3–4); an answer for another text is ignored. Returns whether it was
    /// taken.
    pub fn set_usage_counts(
        &mut self,
        revision: u64,
        counts: &[idecraft_languages::usages::UsageCount],
        cx: &mut Context<Self>,
    ) -> bool {
        if revision != self.revision {
            return false;
        }
        let rows: Vec<(usize, String)> = counts
            .iter()
            .map(|c| (c.line as usize, c.label()))
            .collect();
        if rows.as_slice() != self.folds.above() {
            self.folds.set_above(rows);
            cx.notify();
        }
        true
    }

    /// Shows the authors of the declarations of the text of `revision` (`SPEC-0292` v4 behaviour
    /// 7d); an answer for another text is ignored. Returns whether it was taken.
    pub fn set_code_authors(
        &mut self,
        revision: u64,
        authors: &[idecraft_vcs::CodeAuthor],
        cx: &mut Context<Self>,
    ) -> bool {
        if revision != self.revision {
            return false;
        }
        let hints = crate::authors_model::AuthorHints::from_authors(authors);
        if hints != self.authors {
            self.authors = hints;
            cx.notify();
        }
        true
    }

    /// Forgets the author hints (the setting was switched off, `SPEC-0292` v4 behaviour 7f).
    pub fn clear_code_authors(&mut self, cx: &mut Context<Self>) {
        if !self.authors.is_empty() {
            self.authors.clear();
            cx.notify();
        }
    }

    /// The author hints as drawn now: 0-based line and text, `*` from the current markers.
    pub fn author_labels(&self) -> Vec<(usize, String)> {
        self.authors
            .hints()
            .iter()
            .filter_map(|h| Some((h.line, self.authors.label_at(h.line, &self.changes)?)))
            .collect()
    }

    /// Author hints the last frame drew: line and text.
    pub fn painted_author_hints(&self) -> Vec<(usize, String)> {
        self.last
            .author_hints
            .iter()
            .map(|(_, l, t)| (*l, t.clone()))
            .collect()
    }

    /// What this view shows besides the text: the base and markers, semantic colours, parameter
    /// hints, usage counts and authors (`SPEC-0112` v4 behaviour 9).
    pub fn decorations(&self) -> Decorations {
        Decorations {
            base: self.base.clone(),
            changes: self.changes.clone(),
            semantic: self.semantic.clone(),
            hints: self.hints.clone(),
            framework_marks: self.framework_marks.clone(),
            value_folds: self.value_folds.clone(),
            above: self.folds.above().to_vec(),
            at_line_end: self.folds.at_line_end(),
            authors: self.authors.clone(),
            problems: [
                self.server_problems.clone(),
                self.framework_problems.clone(),
                self.plugin_problems.clone(),
            ],
        }
    }

    /// Takes over the decorations of another view of the same document (`SPEC-0112` v4
    /// behaviour 9).
    pub fn set_decorations(&mut self, d: Decorations, cx: &mut Context<Self>) {
        self.base = d.base;
        self.changes = d.changes;
        self.semantic = d.semantic;
        self.hints = d.hints;
        self.framework_marks = d.framework_marks;
        self.value_folds = d.value_folds;
        self.folds.set_above(d.above);
        self.folds.set_at_line_end(d.at_line_end);
        self.authors = d.authors;
        let [server, framework, plugin] = d.problems;
        self.server_problems = server;
        self.framework_problems = framework;
        self.plugin_problems = plugin;
        self.merge_problems();
        cx.notify();
    }

    /// Width of the editor when last painted.
    pub fn view_width(&self) -> f32 {
        self.view_width
    }

    /// The text the markers compare against (HEAD), if any.
    pub fn base_text(&self) -> Option<&str> {
        self.base.as_deref()
    }

    /// The change whose marker is at window position `position`, if the marker column is there
    /// (`SPEC-0064` v3 behaviour 6).
    pub fn marker_change_at(&self, position: gpui::Point<Pixels>) -> Option<usize> {
        if self.field || self.changes.is_empty() {
            return None;
        }
        let x = self.last.gutter_x - px(FOLD_WIDTH + MARKER_WIDTH + 2.0);
        if position.x < x - px(3.0) || position.x > x + px(MARKER_WIDTH + 3.0) {
            return None;
        }
        let painted = self.last.lines.iter().find(|p| {
            !p.above && position.y >= p.origin.y && position.y < p.origin.y + px(line_height())
        })?;
        // The run icon sits just left of the marker: on its line only the bar itself and the
        // slack to its right open the change bar (SPEC-0064 v4; before, such a line, e.g. a
        // folded `main` with a change inside, could not open its bar at all).
        if self.run_markers.binary_search(&painted.line).is_ok() && position.x < x {
            return None;
        }
        let upper = position.y < painted.origin.y + px(line_height() / 2.0);
        crate::change_bar_model::change_at(&self.changes, painted.line, upper).or_else(|| {
            // A folded line stands for its hidden lines: the first change inside the fold opens
            // (SPEC-0064 v4; the marker of a folded `main` with a change inside did nothing).
            let (start, end) = self.folds.folded_at(painted.line)?;
            (start + 1..=end)
                .find_map(|l| crate::change_bar_model::change_at(&self.changes, l, true))
        })
    }

    /// Window position where the change bar of change `index` opens: below the change, at the
    /// text's left edge; `None` while that line is not drawn.
    pub fn change_bar_anchor(&self, index: usize) -> Option<gpui::Point<Pixels>> {
        let change = self.changes.get(index)?;
        let line = crate::change_bar_model::anchor_line(change);
        let painted = self
            .last
            .lines
            .iter()
            .find(|p| !p.above && p.line == line)?;
        Some(point(
            self.last.gutter_x - px(FOLD_WIDTH),
            painted.origin.y + px(line_height()),
        ))
    }

    /// Puts the caret on the first line of change `index`, scrolls it into view and opens its bar
    /// once drawn (`SPEC-0064` v3 behaviour 7).
    pub fn goto_change(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(change) = self.changes.get(index) else {
            return;
        };
        let line = change
            .new_start
            .saturating_sub(1)
            .min(self.state.buffer.len_lines().saturating_sub(1));
        let at = self.line_start(line);
        self.state.carets = Carets::new(at);
        self.reveal_line(line);
        self.pending_bar = Some(index);
        cx.notify();
    }

    /// The text of change `index` at HEAD (`SPEC-0064` v3 behaviours 6–7).
    pub fn change_old_text(&self, index: usize) -> Option<String> {
        let base = self.base.as_deref()?;
        Some(idecraft_vcs::old_text(base, self.changes.get(index)?))
    }

    /// Takes back change `index`: its lines get their HEAD text, as one undo step and without
    /// saving (`SPEC-0064` v3 behaviour 7). Returns the change taken back.
    pub fn revert_change(&mut self, index: usize, cx: &mut Context<Self>) -> Option<LineChange> {
        let change = self.changes.get(index)?.clone();
        self.revert_line_change(&change, cx).then_some(change)
    }

    /// Replaces `change`'s lines by those of the base (also `»` of the diff viewer, `SPEC-0084`
    /// v2 behaviour 9). Returns whether the text changed.
    pub fn revert_line_change(&mut self, change: &LineChange, cx: &mut Context<Self>) -> bool {
        let Some(base) = self.base.clone() else {
            return false;
        };
        let text = self.state.buffer.text();
        let reverted = idecraft_vcs::revert_change(&base, &text, change);
        self.replace_text(&reverted, cx)
    }

    /// Makes the text `new` as one undo step and without saving, replacing only the part that
    /// differs (a change taken over in the diff viewer, `SPEC-0084` v5 behaviour 18). Returns
    /// whether the text changed; a read-only text stays as it is.
    pub fn replace_text(&mut self, new: &str, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        let text = self.state.buffer.text();
        // Only the differing middle is replaced, so carets and undo stay local.
        let (a, b): (Vec<char>, Vec<char>) = (text.chars().collect(), new.chars().collect());
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        if prefix == a.len() && a.len() == b.len() {
            return false;
        }
        let replacement: String = b[prefix..b.len() - suffix].iter().collect();
        let caret = self.caret();
        let top = self.state.buffer.top_step();
        self.replace_ranges(&[(prefix..a.len() - suffix, replacement)], cx);
        let len = self.state.buffer.len_chars();
        self.state.carets = Carets::new(caret.min(len));
        self.note_carets_after(top);
        cx.notify();
        true
    }

    /// Draws the usage counts at the end of their lines (`true`) or on rows above them
    /// (`SPEC-0292` v2 behaviours 4, 4a and 5).
    pub fn set_usage_counts_at_line_end(&mut self, at_line_end: bool, cx: &mut Context<Self>) {
        if self.folds.at_line_end() != at_line_end {
            self.folds.set_at_line_end(at_line_end);
            cx.notify();
        }
    }

    /// Folded one-line methods of the last frame: line, text colours of the placeholder and brace
    /// backgrounds (`SPEC-0083` v3 behaviour 7).
    pub fn painted_one_line_folds(&self) -> &[(usize, usize, usize)] {
        &self.last.one_line_folds
    }

    /// Texts the last frame drew at the end of lines (`SPEC-0292` v2 behaviour 4).
    pub fn painted_line_end_texts(&self) -> &[(usize, String)] {
        &self.last.line_end_texts
    }

    /// Whether the usage counts stand at the end of their lines.
    pub fn usage_counts_at_line_end(&self) -> bool {
        self.folds.at_line_end()
    }

    /// How far the view is scrolled and how high it is, both in rows (for tests).
    pub fn scrolled_rows(&self) -> (f32, f32) {
        (
            self.scroll_y / line_height(),
            self.view_height / line_height(),
        )
    }

    /// Visible rows: lines not hidden by folds, plus rows above lines.
    pub fn visible_rows(&self) -> usize {
        self.folds.rows(self.state.buffer.len_lines())
    }

    /// Forgets the usage counts (the setting was switched off, `SPEC-0292` behaviour 5).
    pub fn clear_usage_counts(&mut self, cx: &mut Context<Self>) {
        if !self.folds.above().is_empty() {
            self.folds.set_above(Vec::new());
            cx.notify();
        }
    }

    /// Usage count rows shown: declaration line and text.
    pub fn usage_rows(&self) -> &[(usize, String)] {
        self.folds.above()
    }

    /// Forgets the parameter name hints (the setting was switched off, `SPEC-0291` behaviour 6).
    pub fn clear_parameter_hints(&mut self, cx: &mut Context<Self>) {
        if !self.hints.hints().is_empty() {
            self.hints.clear();
            cx.notify();
        }
    }

    /// Parameter name hints shown: char offset and label.
    pub fn parameter_hints(&self) -> Vec<(usize, String)> {
        self.hints
            .hints()
            .iter()
            .map(|h| (h.offset, h.text[h.label.clone()].to_owned()))
            .collect()
    }

    /// Forgets the semantic tokens (the setting was switched off, `SPEC-0290` behaviour 6).
    pub fn clear_semantic_tokens(&mut self, cx: &mut Context<Self>) {
        if !self.semantic.spans().is_empty() {
            self.semantic.clear();
            cx.notify();
        }
    }

    /// The syntax capture painted at char `offset` and its style in the current scheme, from the
    /// tree as the painting reads it (`SPEC-0027`); `None` while the tree is not current or the
    /// text there has no capture. The last span over the offset wins, as in the colour runs.
    pub fn syntax_style_at(
        &self,
        offset: usize,
    ) -> Option<(String, idecraft_ui::theme::TextStyle)> {
        let doc = self
            .syntax
            .as_ref()
            .filter(|_| self.syntax_edits.is_empty())?;
        let range = offset..offset + 1;
        let spans = self.comment_spans(doc.highlight_range(range.clone()), range);
        let span = spans
            .iter()
            .rev()
            .find(|s| s.start <= offset && s.end > offset)?;
        Some((
            span.kind.to_string(),
            Palette::current().capture(&span.kind),
        ))
    }

    /// Semantic tokens shown: char ranges with their colour keys.
    pub fn semantic_spans(&self) -> &[(Range<usize>, &'static str)] {
        self.semantic.spans()
    }

    /// The language server's problems of the current text, with their codes (`SPEC-0117` v4).
    pub fn server_problems(&self) -> &[Problem] {
        &self.server_problems
    }

    /// The server's problems that touch char offsets `range`: where each starts and its message.
    pub fn server_problems_in(&self, range: std::ops::Range<usize>) -> Vec<(usize, String)> {
        self.server_problems
            .iter()
            .filter(|p| p.range.start < range.end && range.start <= p.range.end)
            .map(|p| (p.range.start, p.message.clone()))
            .collect()
    }

    /// Shows the language server's diagnostics for the current text (SPEC-0070 behaviour 2).
    pub fn set_diagnostics(
        &mut self,
        diagnostics: &[idecraft_languages::lsp::Diagnostic],
        cx: &mut Context<Self>,
    ) {
        let buffer = &self.state.buffer;
        self.server_problems = diagnostics_model::to_problems(
            diagnostics,
            |l| {
                buffer
                    .line(l)
                    .map(|t| t.trim_end_matches(['\n', '\r']).to_owned())
            },
            |l| {
                buffer
                    .position_to_offset(idecraft_editor::Position::new(l, 0))
                    .unwrap_or(buffer.len_chars())
            },
        );
        self.merge_problems();
        cx.notify();
    }

    fn merge_problems(&mut self) {
        // An own finding on exactly the text of a server diagnostic is shown once (SPEC-0070 v5
        // behaviour 7).
        let own = if self.remote_document {
            diagnostics_model::without_duplicates(&self.server_problems, self.host_problems.clone())
        } else {
            diagnostics_model::without_duplicates(&self.server_problems, self.own_problems.clone())
        };
        let mut all: Vec<Problem> = self
            .server_problems
            .iter()
            .chain(&own)
            .chain(&self.project.problems)
            .chain(&self.framework_problems)
            .chain(&self.plugin_problems)
            .cloned()
            .collect();
        all.sort_by_key(|p| (p.range.start, p.range.end));
        // The file's highlighting level (SPEC-0070 v6 behaviour 11).
        let (shown, hidden) = diagnostics_model::by_level(all, self.highlight);
        self.problems = shown;
        self.hidden_by_level = hidden;
    }

    /// The file's highlighting level (`SPEC-0070` v6 behaviour 11).
    pub fn highlight_level(&self) -> diagnostics_model::HighlightLevel {
        self.highlight
    }

    /// Sets the file's highlighting level and shows its problems accordingly.
    pub fn set_highlight_level(&mut self, level: diagnostics_model::HighlightLevel) {
        if self.highlight != level {
            self.highlight = level;
            self.merge_problems();
        }
    }

    /// How many problems the highlighting level hides.
    pub fn hidden_by_level(&self) -> usize {
        self.hidden_by_level
    }

    /// Runs IDECraft's own inspections on the parsed text (`SPEC-0098` behaviour 3); only when the
    /// syntax tree matches the buffer.
    fn refresh_inspections(&mut self) {
        self.own_problems = match &self.syntax {
            Some(doc) if self.syntax_edits.is_empty() => tree_problems(doc, self.path.as_deref()),
            Some(_) => return,
            // The tree of a large file is still being built.
            None if self.parsing => return,
            // Properties and plain text files have no syntax tree (SPEC-0155 behaviours 5, 7).
            None => {
                let Some(path) = self.path.clone().filter(|_| !self.field) else {
                    return;
                };
                let plain = self.state.buffer.text();
                let findings = idecraft_languages::inspections::inspect_text(&path, &plain);
                own_problems(&plain, findings)
            }
        };
        self.merge_problems();
    }

    /// Problems shown, as char ranges.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// Follows the problem under the pointer: resting half a second on one asks for its popup,
    /// leaving the problems hides it (`SPEC-0078` v2 behaviour 5).
    fn track_problem_hover(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let on_text = self.line_at_y(position.y).is_some() && !self.in_gutter(position);
        let start = on_text
            .then(|| {
                let at = self.offset_at(position);
                diagnostics_model::under(&self.problems, at)
                    .first()
                    .map(|p| p.range.start)
            })
            .flatten();
        match (start, self.problem_hover) {
            (Some(s), Some((h, _))) if s == h => {}
            (Some(s), previous) => {
                if previous.is_some_and(|(_, asked)| asked) {
                    cx.emit(EditorEvent::ProblemHover(None));
                }
                self.problem_hover = Some((s, false));
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.problem_hover == Some((s, false)) {
                            this.problem_hover = Some((s, true));
                            cx.emit(EditorEvent::ProblemHover(Some(s)));
                        }
                    });
                })
                .detach();
            }
            (None, Some((_, asked))) => {
                self.problem_hover = None;
                if asked {
                    cx.emit(EditorEvent::ProblemHover(None));
                }
            }
            (None, None) => {}
        }
    }

    /// The char exactly under a window position in the last frame: `None` beside the text, past a
    /// line's end, in the gutter or on a usage-count row.
    fn char_under(&self, position: gpui::Point<Pixels>) -> Option<usize> {
        if self.field || self.in_gutter(position) {
            return None;
        }
        let painted = self.last.lines.iter().find(|p| {
            !p.above && position.y >= p.origin.y && position.y < p.origin.y + px(line_height())
        })?;
        if position.x < painted.origin.x {
            return None;
        }
        let byte = painted.shaped.index_for_x(position.x - painted.origin.x)?;
        Some(self.line_start(painted.line) + painted.display.column_of(byte))
    }

    /// Follows the word under the pointer: resting half a second on one asks for its documentation,
    /// leaving it hides that again; a problem under the pointer has the problem popup instead
    /// (`SPEC-0077` v6 behaviour 5).
    fn track_doc_hover(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let start = self
            .char_under(position)
            .filter(|&at| diagnostics_model::under(&self.problems, at).is_empty())
            .and_then(|at| {
                crate::doc_model::word_at(|i| self.state.buffer.char_at(i), at).map(|w| w.start)
            });
        match (start, self.doc_hover) {
            (Some(s), Some((h, _))) if s == h => {}
            (Some(s), previous) => {
                if previous.is_some_and(|(_, asked)| asked) {
                    cx.emit(EditorEvent::DocHover(None));
                }
                self.doc_hover = Some((s, false));
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.doc_hover == Some((s, false)) {
                            this.doc_hover = Some((s, true));
                            cx.emit(EditorEvent::DocHover(Some(s)));
                        }
                    });
                })
                .detach();
            }
            (None, Some(_)) => self.leave_doc_hover(cx),
            (None, None) => {}
        }
    }

    /// The pointer left the word it rested on (or the view).
    fn leave_doc_hover(&mut self, cx: &mut Context<Self>) {
        if let Some((_, asked)) = self.doc_hover.take()
            && asked
        {
            cx.emit(EditorEvent::DocHover(None));
        }
    }

    /// The wave colour for the placeholder of the region folded at `line`, when it hides a problem
    /// that is not unused code (SPEC-0070 v5 behaviour 10).
    fn hidden_problem_colour(&self, line: usize, p: &Palette) -> Option<u32> {
        let (_, end) = self.folds.folded_at(line)?;
        let hidden = self.line_start(line + 1)..self.line_start(end + 1);
        let q = diagnostics_model::hidden_in(&self.problems, hidden).filter(|q| !q.unused)?;
        let key = match q.severity {
            Severity::Error => "ERRORS_ATTRIBUTES",
            Severity::Warning => "WARNING_ATTRIBUTES",
            Severity::Weak => "WEAK_WARNING_ATTRIBUTES",
        };
        Some(p.problem_mark(key))
    }

    /// The error stripe's marks: one per line with problems (SPEC-0070 v5 behaviour 8).
    pub fn stripe_marks(&self) -> Vec<diagnostics_model::StripeMark> {
        diagnostics_model::stripe_marks(&self.problems, |o| self.line_of(o))
    }

    /// Number of lines of the document.
    pub fn line_count(&self) -> usize {
        self.state.buffer.len_lines()
    }

    /// The problems with their line, column and offset, for the Problems tool window (`SPEC-0092`).
    pub fn located_problems(&self, path: &std::path::Path) -> Vec<crate::problems_model::Located> {
        self.problems
            .iter()
            .map(|p| {
                let line = self.line_of(p.range.start);
                crate::problems_model::Located {
                    path: path.to_path_buf(),
                    line,
                    column: p.range.start - self.line_start(line),
                    offset: p.range.start,
                    severity: p.severity,
                    message: p.message.clone(),
                    inspection: p.inspection,
                }
            })
            .collect()
    }

    /// Successful saves so far.
    pub fn saves(&self) -> u64 {
        self.saves
    }

    /// Moves problem ranges along with an edit so they stay on their text until fresh diagnostics
    /// arrive; problems inside replaced text are dropped (behaviour 2).
    fn shift_problems(&mut self, edit: &idecraft_editor::Edit) {
        let removed = edit.deleted.chars().count();
        let inserted = edit.inserted.chars().count();
        let end = edit.offset + removed;
        for list in [
            &mut self.problems,
            &mut self.server_problems,
            &mut self.own_problems,
            &mut self.framework_problems,
            &mut self.project.problems,
        ] {
            list.retain_mut(|p| {
                if p.range.end <= edit.offset {
                    true
                } else if p.range.start >= end {
                    p.range.start = p.range.start - removed + inserted;
                    p.range.end = p.range.end - removed + inserted;
                    true
                } else {
                    false
                }
            });
        }
    }

    /// Window position under the start of char `offset` (bottom of its line) in the last frame, for
    /// popups such as completion (SPEC-0072); `None` when the line is not visible.
    pub fn point_below(&self, offset: usize) -> Option<gpui::Point<Pixels>> {
        let line = self.line_of(offset);
        let painted = self
            .last
            .lines
            .iter()
            .find(|p| p.line == line && !p.above)?;
        let column = offset.saturating_sub(self.line_start(line));
        let x = painted.shaped.x_for_index(painted.display.byte_of(column));
        Some(point(
            painted.origin.x + x,
            painted.origin.y + px(line_height()),
        ))
    }

    /// Window position in the middle of char `offset` in the last frame, for the self-test's pointer
    /// (`SPEC-0077` v6 AK-11); `None` when the line is not visible.
    pub fn point_on_char(&self, offset: usize) -> Option<gpui::Point<Pixels>> {
        let below = self.point_below(offset)?;
        let line = self.line_of(offset);
        let painted = self
            .last
            .lines
            .iter()
            .find(|p| p.line == line && !p.above)?;
        let column = offset.saturating_sub(self.line_start(line)) + 1;
        let next = painted.shaped.x_for_index(painted.display.byte_of(column));
        let x = (below.x + painted.origin.x + next) / 2.;
        Some(point(x, below.y - px(line_height()) / 2.))
    }

    /// Like [`Self::point_below`], but for a line outside the painted ones the nearest painted
    /// line's start, so a popup for a caret scrolled out of view still shows (and takes keys).
    pub fn point_near(&self, offset: usize) -> Option<gpui::Point<Pixels>> {
        self.point_below(offset).or_else(|| {
            let line = self.line_of(offset);
            let painted = self
                .last
                .lines
                .iter()
                .min_by_key(|p| p.line.abs_diff(line))?;
            Some(point(
                painted.origin.x,
                painted.origin.y + px(line_height()),
            ))
        })
    }

    /// Where a list opened at char `offset` goes (`SPEC-0122` v5 behaviour 13): the line of the
    /// offset as the last frame painted it (the nearest painted line when it is scrolled out of
    /// view) and the editor's area; `None` before the editor was painted.
    pub fn caret_spot(
        &self,
        offset: usize,
    ) -> Option<(crate::popup_place::CaretSpot, crate::popup_place::Area)> {
        let below = self.point_near(offset)?;
        let b = self.last.bounds;
        if b.size.width <= px(0.) || b.size.height <= px(0.) {
            return None;
        }
        let bottom = f32::from(below.y);
        Some((
            crate::popup_place::CaretSpot {
                x: f32::from(below.x),
                top: bottom - line_height(),
                bottom,
            },
            crate::popup_place::Area {
                left: f32::from(b.origin.x),
                top: f32::from(b.origin.y),
                right: f32::from(b.origin.x + b.size.width),
                bottom: f32::from(b.origin.y + b.size.height),
            },
        ))
    }

    /// Window position over the start of char `offset` (top of its line) in the last frame, for
    /// popups shown above the caret such as parameter info (SPEC-0080).
    pub fn point_above(&self, offset: usize) -> Option<gpui::Point<Pixels>> {
        self.point_below(offset)
            .map(|p| point(p.x, p.y - px(line_height())))
    }

    /// Applies edits given as line/character ranges (characters in Unicode scalar values, as the
    /// language features report them) as one undo step (SPEC-0074 behaviour 3).
    pub fn apply_text_edits(
        &mut self,
        edits: &[idecraft_languages::rename::Edit],
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let mut ranges: Vec<(Range<usize>, String)> = edits
            .iter()
            .map(|e| {
                let at =
                    |line: u64, character: u64| self.line_start(line as usize) + character as usize;
                (
                    at(e.range.start_line, e.range.start_character)
                        ..at(e.range.end_line, e.range.end_character),
                    e.new_text.clone(),
                )
            })
            .collect();
        ranges.sort_by_key(|(r, _)| r.start);
        let caret = self.caret();
        let top = self.state.buffer.top_step();
        self.replace_ranges(&ranges, cx);
        // Keep the caret where it was, shifted by the edits before it.
        let shift: isize = ranges
            .iter()
            .filter(|(r, _)| r.end <= caret)
            .map(|(r, t)| t.chars().count() as isize - r.len() as isize)
            .sum();
        let at = (caret as isize + shift).max(0) as usize;
        self.state.carets = Carets::new(at.min(self.state.buffer.len_chars()));
        self.note_carets_after(top);
        cx.notify();
    }

    /// The document's text and undo history, to keep when this editor of the file closes
    /// (`SPEC-0057` v8 behaviour 31): only of a local file without unsaved changes whose history
    /// holds a step and no refactoring across files.
    pub fn history_to_keep(&self) -> Option<idecraft_editor::TextBuffer> {
        let buffer = &self.state.buffer;
        if self.path.is_none()
            || self.remote_document
            || buffer.is_modified()
            || !self.group_steps.is_empty()
            || (buffer.top_step().is_none() && buffer.redo_step().is_none())
        {
            return None;
        }
        Some(buffer.clone())
    }

    /// Continues the history `kept` when an editor of this file closed, if this editor holds
    /// exactly its text and has no history of its own yet; returns whether it does (`SPEC-0057`
    /// v8 behaviour 31).
    pub fn adopt_history(&mut self, kept: idecraft_editor::TextBuffer) -> bool {
        let buffer = &self.state.buffer;
        if buffer.top_step().is_some()
            || buffer.redo_step().is_some()
            || buffer.is_modified()
            || kept.text() != buffer.text()
        {
            return false;
        }
        self.state.buffer = kept;
        true
    }

    /// Marks undo step `step` as part of a refactoring across files, or unmarks it (`SPEC-0193`).
    pub fn mark_group_step(&mut self, step: u64, on: bool) {
        if on {
            self.group_steps.insert(step);
        } else {
            self.group_steps.remove(&step);
        }
    }

    /// Undoes (or redoes) one step without asking the workbench, for a refactoring's undo across
    /// files (`SPEC-0193` behaviours 3, 4).
    pub fn undo_plain(&mut self, redo: bool, cx: &mut Context<Self>) {
        let step = if redo {
            self.state.buffer.redo_carets()
        } else {
            self.state.buffer.undo_carets()
        };
        if let Some((at, noted)) = step {
            self.state.carets =
                noted.unwrap_or_else(|| Carets::new(at.min(self.state.buffer.len_chars())));
        }
        self.after_change(true, cx);
        cx.notify();
    }

    /// Number of buffer changes so far.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Text of the primary selection when it lies within one line (the find bar starts with it).
    pub fn selected_line_text(&self) -> Option<String> {
        let c = self.state.carets.primary();
        let text = self.state.buffer.slice(c.range()).ok()?;
        (!text.is_empty() && !text.contains('\n')).then_some(text)
    }

    /// The expression to evaluate: the selection within one line, else the identifier at the caret.
    pub fn expression_at_caret(&self) -> Option<String> {
        if let Some(text) = self.selected_line_text() {
            return Some(text);
        }
        let buf = &self.state.buffer;
        let word = |ch: Option<char>| ch.is_some_and(|c| c.is_alphanumeric() || c == '_');
        let at = self.caret();
        let mut start = at;
        while start > 0 && word(buf.char_at(start - 1)) {
            start -= 1;
        }
        let mut end = at;
        while word(buf.char_at(end)) {
            end += 1;
        }
        buf.slice(start..end).ok().filter(|s| !s.is_empty())
    }

    /// The primary selection as a char range, if any text is selected.
    pub fn selection_range(&self) -> Option<Range<usize>> {
        let c = self.state.carets.primary();
        (c.anchor != c.head).then(|| c.anchor.min(c.head)..c.anchor.max(c.head))
    }

    /// Primary caret offset.
    pub fn caret(&self) -> usize {
        self.state.carets.primary().head
    }

    /// Current gutter markers.
    pub fn changes(&self) -> &[LineChange] {
        &self.changes
    }

    /// The markers against the base for the text as it is now, computed at once; `None` for a
    /// file without a base, one not under Git (`SPEC-0075` v6 behaviour 8b).
    pub fn changes_now(&self) -> Option<Vec<LineChange>> {
        let base = self.base.as_ref()?;
        Some(line_changes(base, &self.state.buffer.text()))
    }

    /// Whether the change markers have a base (the file is under Git).
    pub fn has_base(&self) -> bool {
        self.base.is_some()
    }

    /// Recomputes the markers on a background thread; typing never waits for it (behaviour 3).
    fn rediff(&mut self, cx: &mut Context<Self>) {
        let Some(base) = self.base.clone() else {
            return;
        };
        if self.diffing {
            self.diff_stale = true;
            return;
        }
        self.diffing = true;
        self.diff_stale = false;
        let text = self.state.buffer.text();
        cx.spawn(async move |this, cx| {
            let changes = cx
                .background_executor()
                .spawn(async move { line_changes(&base, &text) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.diffing = false;
                this.changes = changes;
                if this.diff_stale {
                    this.rediff(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Hides the annotation's hover card, e.g. when a context menu opens over it (hand test 195:
    /// the card stayed under the menu).
    pub fn hide_annotation_hover(&mut self, cx: &mut Context<Self>) {
        if self.annotation_hover.take().is_some() {
            cx.notify();
        }
    }

    /// Whether an annotation's hover card is shown (checks).
    pub fn annotation_hover_shown(&self) -> bool {
        self.annotation_hover.is_some()
    }

    /// Shows the annotations of `blame` (the file's blame at HEAD), or none (`SPEC-0277`
    /// behaviours 2 and 6); the lines are mapped onto the buffer in the background.
    pub fn set_annotations(
        &mut self,
        blame: Option<Arc<idecraft_vcs::HeadBlame>>,
        cx: &mut Context<Self>,
    ) {
        self.annotation_hover = None;
        self.annotations = blame.map(|b| {
            crate::blame_model::Annotations::new(
                b,
                Vec::new(),
                crate::git_log_model::LocalZone::system(),
                crate::git_log_model::local_date_style(),
            )
        });
        self.remap_annotations(cx);
        cx.notify();
    }

    /// The annotations shown, if any.
    pub fn annotations(&self) -> Option<&crate::blame_model::Annotations> {
        self.annotations.as_ref()
    }

    /// Whether annotations are shown.
    pub fn annotated(&self) -> bool {
        self.annotations.is_some()
    }

    /// Whether a remap of the annotations is still running (tests).
    pub fn remapping_annotations(&self) -> bool {
        self.remapping
    }

    /// Maps the annotations onto the buffer on a background thread; typing never waits for it
    /// (`SPEC-0277` behaviours 7 and 10).
    fn remap_annotations(&mut self, cx: &mut Context<Self>) {
        let Some(blame) = self.annotations.as_ref().map(|a| a.blame().clone()) else {
            return;
        };
        if self.remapping {
            self.remap_stale = true;
            return;
        }
        self.remapping = true;
        self.remap_stale = false;
        let text = self.state.buffer.text();
        cx.spawn(async move |this, cx| {
            let lines = cx
                .background_executor()
                .spawn({
                    let blame = blame.clone();
                    async move { blame.map(&text) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.remapping = false;
                if let Some(a) = this
                    .annotations
                    .as_mut()
                    .filter(|a| Arc::ptr_eq(a.blame(), &blame))
                {
                    a.set_lines(lines);
                }
                if this.remap_stale {
                    this.remap_annotations(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The 0-based line whose annotation is at window position `position`, if annotations are
    /// shown and it is on the column.
    pub fn annotation_line_at(&self, position: gpui::Point<Pixels>) -> Option<usize> {
        self.annotations.as_ref()?;
        if position.x < self.last.origin.x || position.x >= self.last.annotation_x {
            return None;
        }
        self.line_at_y(position.y)
    }

    /// Whether window position `position` is on the gutter (line numbers, markers, folding).
    pub fn in_gutter(&self, position: gpui::Point<Pixels>) -> bool {
        !self.field && position.x >= self.last.origin.x && position.x < self.last.gutter_x
    }

    /// The painted line at window height `y`.
    fn line_at_y(&self, y: Pixels) -> Option<usize> {
        self.last
            .lines
            .iter()
            .find(|p| y >= p.origin.y && y < p.origin.y + px(line_height()))
            .map(|p| p.line)
    }

    /// The hover card of the annotation under the pointer (`SPEC-0277` behaviour 4).
    fn annotation_card(&self) -> Option<gpui::AnyElement> {
        let (line, at) = self.annotation_hover?;
        let text = self.annotations.as_ref()?.hover(line)?;
        let p = Palette::current();
        let c = crate::workbench_view::c;
        let local = at - self.last.origin;
        Some(
            div()
                .absolute()
                .left(local.x + px(12.0))
                .top(local.y + px(16.0))
                .max_w(px(520.0))
                .p_2()
                .flex()
                .flex_col()
                .bg(c(p.popup))
                .border_1()
                .border_color(c(p.popup_border))
                .rounded(px(6.0))
                .shadow_lg()
                .text_color(c(p.ui_text))
                .children(text.into_iter().enumerate().map(|(i, t)| {
                    div()
                        .min_h(px(6.0))
                        .when(i == 0, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                        .child(t)
                }))
                .into_any_element(),
        )
    }

    /// Caret position and file settings for the status bar.
    pub fn status(&self) -> EditorStatus {
        let head = self.state.carets.primary().head;
        let pos = self
            .state
            .buffer
            .offset_to_position(head.min(self.state.buffer.len_chars()))
            .unwrap_or_default();
        EditorStatus {
            line: pos.line + 1,
            column: pos.column + 1,
            line_ending: match self.state.buffer.line_ending() {
                idecraft_editor::LineEnding::Lf => "LF".into(),
                idecraft_editor::LineEnding::CrLf => "CRLF".into(),
                idecraft_editor::LineEnding::Cr => "CR".into(),
            },
            encoding: self.encoding.label(),
            indent: self.state.indent.clone(),
            note: self.note.clone(),
            problems: diagnostics_model::counts3(&self.problems),
            problem_at_caret: diagnostics_model::at(&self.problems, head)
                .map(|p| p.message.clone()),
            problem_from_tool: diagnostics_model::at(&self.problems, head).is_some_and(|p| {
                self.framework_problems
                    .iter()
                    .any(|t| t.range == p.range && t.message == p.message)
            }),
            column_mode: self.column_mode,
            overwrite: self.overwrite(),
        }
    }

    /// The definitions around the caret, outermost first, as the breadcrumbs show them: methods
    /// and functions end in `()` (`SPEC-0061` v3 behaviour 5a).
    pub fn symbol_path(&self) -> Vec<String> {
        let Some(doc) = self.syntax.as_ref() else {
            return Vec::new();
        };
        let caret = self.state.carets.primary().head;
        let key = (self.revision, caret, doc.text().len());
        if let Ok(cache) = self.symbol_path.try_borrow()
            && cache.0 == key
        {
            return cache.1.clone();
        }
        let text = doc.text();
        let byte = text
            .char_indices()
            .nth(caret)
            .map_or(text.len(), |(b, _)| b);
        let path: Vec<String> = doc
            .definitions_around(byte)
            .into_iter()
            .map(|d| match d.kind.as_str() {
                "method" | "function" => format!("{}()", d.name),
                _ => d.name,
            })
            .collect();
        if let Ok(mut cache) = self.symbol_path.try_borrow_mut() {
            *cache = (key, path.clone());
        }
        path
    }

    /// Focus handle, to focus the editor when the window opens.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Queues buffer edits for the syntax tree, keeps the primary caret visible.
    fn after_change(&mut self, reveal: bool, cx: &mut Context<Self>) {
        // Every edit of a read-only file is put back before anyone hears of it (SPEC-0149 v4).
        if let Some(frozen) = &self.frozen
            && (self.state.buffer.is_modified() || self.state.buffer.text() != frozen.text())
        {
            let at = self.state.carets.primary().head.min(frozen.len_chars());
            self.state.buffer = frozen.clone();
            self.state.carets = Carets::new(at);
            self.note = Some(self.read_only_note());
            cx.notify();
            return;
        }
        let edits = self.state.buffer.take_changes();
        if !edits.is_empty() {
            cx.emit(EditorEvent::Edited(edits.clone()));
        }
        self.edited(edits, reveal, cx);
    }

    /// Takes over `buffer` — the text, undo history and saved state of another view of the same
    /// file — after `edits` were applied there; this view's carets move with the edits
    /// (`SPEC-0112` behaviour 3). Emits nothing, so two views do not echo each other.
    pub fn adopt(
        &mut self,
        buffer: &idecraft_editor::TextBuffer,
        edits: &[idecraft_editor::Edit],
        cx: &mut Context<Self>,
    ) {
        let mut buffer = buffer.clone();
        buffer.track_changes();
        let _ = buffer.take_changes();
        let len = buffer.len_chars();
        let moved: Vec<Caret> = self
            .state
            .carets
            .carets()
            .iter()
            .map(|c| {
                let shift = |at: usize| {
                    edits
                        .iter()
                        .fold(at, crate::split_model::shift_offset)
                        .min(len)
                };
                Caret::select(shift(c.anchor), shift(c.head))
            })
            .collect();
        self.state.buffer = buffer;
        self.state.carets.moved_to(moved);
        self.adopting = true;
        self.edited(edits.to_vec(), false, cx);
        self.adopting = false;
        cx.notify();
    }

    /// Bookkeeping after the buffer changed by `edits`: problems, folds and breakpoints move,
    /// the syntax tree and the change markers follow.
    fn edited(&mut self, edits: Vec<idecraft_editor::Edit>, reveal: bool, cx: &mut Context<Self>) {
        if !edits.is_empty() {
            self.revision += 1;
            self.refresh_sql_comments();
            // A running check of the project's unused declarations is for the old text
            // (SPEC-0098 v12 behaviour 26).
            self.project.cancel();
            self.publish_document();
            // Marks of the usages belong to the text they were asked for (SPEC-0295 behaviour 7).
            self.occurrences.text_changed();
        }
        for e in &edits {
            self.shift_problems(e);
            let (deleted, inserted) = (e.deleted.chars().count(), e.inserted.chars().count());
            self.semantic.edited(e.offset, deleted, inserted);
            self.hints.edited(e.offset, deleted, inserted);
            self.value_folds.edited(e.offset, deleted, inserted);
            let line = self.line_of(e.offset);
            let (removed, added) = (
                e.deleted.matches('\n').count(),
                e.inserted.matches('\n').count(),
            );
            // Whole lines put in between two lines change neither of them: the fold and the gutter
            // mark beside them stay (SPEC-0083 v4 behaviour 5a, SPEC-0320 v4 behaviour 20). A
            // replacement counts when what it deletes comes back around what it adds (v5: jdtls
            // replaces the whitespace before a closing brace when it adds a method).
            let whole_lines = crate::decl_marks_model::net_insertion(&e.deleted, &e.inserted)
                .and_then(|(shared, added)| {
                    let at = e.offset + shared;
                    let buffer = &self.state.buffer;
                    crate::decl_marks_model::whole_lines_inserted(
                        added,
                        self.line_of(at),
                        at.checked_sub(1).and_then(|i| buffer.char_at(i)),
                        buffer.char_at(at + added.chars().count()),
                    )
                });
            match whole_lines {
                // The deleted line breaks came back: the lines added are the difference.
                Some(first) => {
                    self.folds.lines_inserted(first, added - removed);
                    self.declaration_marks
                        .lines_inserted(first, added - removed);
                    self.framework_marks.lines_inserted(first, added - removed);
                }
                None => {
                    self.folds.edited(line, removed, added);
                    self.declaration_marks.edited(line, removed, added);
                    self.framework_marks.edited(line, removed, added);
                }
            }
            self.authors.edited(line, removed, added);
            self.shift_breakpoints(line, removed, added);
            // Bookmarks follow their line, or go with it (SPEC-0331 behaviours 5–7).
            if !self.bookmarks.1.is_empty() {
                let edit = self.line_edit(e);
                let file = self.bookmarks.0.clone();
                self.bookmarks.1.edited(&file, &edit);
                self.bookmark_edits.push(edit);
            }
            if !self.line_notes.is_empty() {
                for source in ["run", "inspection"] {
                    let notes: Vec<_> = self
                        .line_notes
                        .iter()
                        .filter(|(s, _)| *s == source)
                        .map(|(_, n)| n.clone())
                        .collect();
                    let moved = crate::sql_editor_model::shift_notes(&notes, line, removed, added);
                    self.line_notes.retain(|(s, _)| *s != source);
                    self.line_notes
                        .extend(moved.into_iter().map(|n| (source, n)));
                }
            }
        }
        if std::mem::take(&mut self.breakpoints_moved) {
            cx.emit(EditorEvent::BreakpointsChanged);
        }
        let bookmark_edits = std::mem::take(&mut self.bookmark_edits);
        if !bookmark_edits.is_empty() && !self.adopting {
            cx.emit(EditorEvent::BookmarkEdits(bookmark_edits));
        }
        if !edits.is_empty() {
            // The stopped position belongs to the text the program was built from.
            self.execution_line = None;
            self.inline_values.clear();
        }
        self.syntax_edits.extend(edits);
        self.reparse(cx);
        self.rediff(cx);
        self.remap_annotations(cx);
        if reveal {
            let head = self.state.carets.primary().head;
            let line = self.line_of(head);
            self.reveal_line(line);
        }
        self.pending_input.get_or_insert_with(Instant::now);
    }

    /// Applies queued edits to the syntax tree on a background thread, so typing never waits for
    /// a re-parse (behaviour 3).
    fn reparse(&mut self, cx: &mut Context<Self>) {
        if self.parsing || self.syntax_edits.is_empty() {
            return;
        }
        let Some(mut doc) = self.syntax.take() else {
            self.syntax_edits.clear();
            // Files without a syntax tree are checked on their text (SPEC-0155 behaviour 7).
            self.refresh_inspections();
            return;
        };
        let edits = std::mem::take(&mut self.syntax_edits);
        self.parsing = true;
        let (path, revision) = (self.path.clone(), self.revision);
        cx.spawn(async move |this, cx| {
            // Inspections and fold regions walk the whole tree, which takes longer than a frame
            // in a large file; they are computed here with the parse, not on the main thread.
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    for e in edits {
                        let end = e.offset + e.deleted.chars().count();
                        doc.edit(e.offset..end, &e.inserted).ok()?;
                    }
                    let problems = tree_problems(&doc, path.as_deref());
                    let folds = Outline::of(&doc);
                    Some((doc, problems, folds))
                })
                .await;
            let _ = this.update(cx, |this, cx| this.parsed(parsed, revision, cx));
        })
        .detach();
    }

    /// Builds the tree of a large file on a background thread (`SPEC-0260` behaviour 4).
    fn parse_in_background(
        &mut self,
        language: LanguageId,
        tsx: bool,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.parsing = true;
        let (path, revision) = (self.path.clone(), self.revision);
        cx.spawn(async move |this, cx| {
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let doc = SyntaxDocument::new(language, tsx, &text).ok()?;
                    let problems = tree_problems(&doc, path.as_deref());
                    let folds = Outline::of(&doc);
                    Some((doc, problems, folds))
                })
                .await;
            let _ = this.update(cx, |this, cx| this.parsed(parsed, revision, cx));
        })
        .detach();
    }

    /// Folds the one-line Java methods and the import block of a Java or Kotlin file once, when
    /// the file's first current tree is there (`SPEC-0083` v2 behaviour 7, v4 behaviour 9); later
    /// trees never fold again.
    fn fold_on_open(&mut self, cx: &App) {
        if self.folded_on_open || self.field {
            return;
        }
        let Some(doc) = self
            .syntax
            .as_ref()
            .filter(|_| self.syntax_edits.is_empty())
        else {
            return;
        };
        self.folded_on_open = true;
        let settings = crate::appearance::ide_settings(cx).editor;
        let jumped_to = self.jumped_to.take();
        // The imports stay open when the file was opened at one of them or the caret is there.
        let caret = self.line_of(self.state.carets.primary().head);
        if settings.fold_imports
            && let Some((start, end)) = doc.import_block()
            && !jumped_to.is_some_and(|l| start <= l && l <= end)
            && !(start < caret && caret <= end)
        {
            self.folds.fold(start, end);
        }
        if !settings.fold_one_line_methods {
            return;
        }
        for b in doc.one_line_bodies() {
            if jumped_to.is_some_and(|l| b.start_line <= l && l <= b.end_line) {
                continue;
            }
            self.folds
                .fold_with(b.start_line, b.end_line, b.placeholder);
        }
    }

    /// Syntax and semantic colours of `line` (its text `text`) by char column, from the current
    /// tree only; empty while it is rebuilt (`SPEC-0083` v3 behaviour 7).
    fn char_colours(
        &self,
        line: usize,
        text: &str,
        p: &Palette,
    ) -> Vec<(std::ops::Range<usize>, idecraft_ui::theme::TextStyle)> {
        let Some(doc) = self
            .syntax
            .as_ref()
            .filter(|_| self.syntax_edits.is_empty())
        else {
            return Vec::new();
        };
        let start = self.line_start(line);
        let end = start + text.chars().count();
        let column = |at: usize| at.clamp(start, end) - start;
        let syntax: Vec<_> = self
            .comment_spans(doc.highlight_range(start..end), start..end)
            .iter()
            .map(|s| (column(s.start)..column(s.end), p.capture(&s.kind)))
            .collect();
        let semantic: Vec<_> = self
            .semantic
            .within(start..end.max(start + 1))
            .iter()
            .map(|(r, key)| (column(r.start)..column(r.end), p.key_style(key)))
            .collect();
        crate::semantic_model::overlay(&syntax, &semantic)
    }

    /// Folded regions with their own placeholder text: first line, last line, text.
    pub fn placeholder_folds(&self) -> Vec<(usize, usize, String)> {
        self.folds
            .regions()
            .iter()
            .filter_map(|&(s, e)| Some((s, e, self.folds.placeholder(s)?.to_owned())))
            .collect()
    }

    /// Installs a tree built in the background with its problems and fold regions; edits made in
    /// the meantime are applied by the next parse, whose results replace these.
    fn parsed(&mut self, parsed: Option<Parsed>, revision: u64, cx: &mut Context<Self>) {
        self.parsing = false;
        match parsed {
            Some((doc, problems, folds)) => {
                self.syntax = Some(doc);
                if self.syntax_edits.is_empty() {
                    self.folds.set_looks(folds.looks.clone());
                    self.fold_regions = (revision, folds);
                    self.own_problems = problems;
                    self.merge_problems();
                    self.fold_on_open(cx);
                    self.check_project(cx);
                } else {
                    self.reparse(cx);
                }
            }
            None => {
                self.syntax = None;
                self.reparse(cx);
                self.refresh_inspections();
            }
        }
        cx.notify();
    }

    /// 0-based line of the char `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        self.state
            .buffer
            .offset_to_position(offset.min(self.state.buffer.len_chars()))
            .map(|p| p.line)
            .unwrap_or(0)
    }

    /// Window position under the code of the execution line, for the choice of Smart Step Into
    /// (`SPEC-0091` v11 behaviour 18); `None` without an execution point.
    pub fn execution_anchor(&self) -> Option<gpui::Point<Pixels>> {
        let line = self.execution_line()?;
        let start = self.line_start(line);
        let indent = self
            .state
            .buffer
            .text()
            .lines()
            .nth(line)
            .map_or(0, |l| l.chars().take_while(|c| c.is_whitespace()).count());
        self.point_near(start + indent)
    }

    fn line_start(&self, line: usize) -> usize {
        self.state
            .buffer
            .position_to_offset(idecraft_editor::Position::new(line, 0))
            .unwrap_or(self.state.buffer.len_chars())
    }

    /// Carries out the editor command bound to `stroke`; returns whether one was.
    fn press(&mut self, stroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let keymap = crate::keymaps::current();
        // A shortcut of two keystrokes (SPEC-0083 v8 behaviour 23, the folding levels): the first
        // one waits, the second one picks the action. Another key acts as itself.
        if let Some(first) = self.first_stroke.take() {
            let ids: Vec<String> = keymap
                .actions_for(&first, Some(stroke))
                .into_iter()
                .map(str::to_owned)
                .collect();
            for id in ids {
                if self.run_action(&id, cx) {
                    return true;
                }
            }
        }
        if !self.field && keymap.starts_two_strokes_only(stroke) {
            self.first_stroke = Some(stroke.clone());
            return true;
        }
        let bound = keymap.actions_for(stroke, None);
        let ids: Vec<String> = idecraft_editor::editor_first(&bound)
            .into_iter()
            .map(str::to_owned)
            .collect();
        // Tab takes a shown inline completion before any other Tab action (SPEC-0157).
        if ids.iter().any(|i| i == "EditorTab") && self.accept_ghost(cx) {
            crate::key_diagnostics::note_ran("accept inline completion", "editor");
            return true;
        }
        // Next and previous suggestion while one is shown (SPEC-0162; ⌥] ⌥[, also ⌃⌥. ⌃⌥, on
        // macOS and ⌥. ⌥, elsewhere — rebindable in Settings › Keymap).
        if self.ghost().is_some() {
            for (id, next) in [
                ("NextInlineCompletionSuggestionAction", true),
                ("PrevInlineCompletionSuggestionAction", false),
            ] {
                if bound.contains(&id) {
                    cx.emit(EditorEvent::InlineAlternative(next));
                    return true;
                }
            }
        }
        match ids.iter().find(|id| self.run_action(id, cx)) {
            Some(id) => {
                crate::key_diagnostics::note_ran(id, "editor");
                true
            }
            None => false,
        }
    }

    /// Carries out an editor action by keymap id; returns whether it is one (also used by Search
    /// Everywhere, SPEC-0067).
    pub fn run_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        // Save All saves every editor of the window, also when this one is read-only; the window
        // asks each for `SaveDocument` (SPEC-0058 v4 behaviour 12).
        if id == "SaveAll" && self.save_all_in_window {
            cx.emit(EditorEvent::SaveAll);
            return true;
        }
        // A read-only file is not saved; edits are undone in `after_change` (SPEC-0149 v4).
        if self.read_only && matches!(id, "SaveAll" | "SaveDocument") {
            return true;
        }
        // A password never leaves its field through the clipboard.
        if self.masked
            && matches!(
                id,
                "$Copy" | "$Cut" | "EditorCopy" | "EditorCut" | "EditorCutLineEnd"
            )
        {
            return true;
        }
        // A read-only text takes neither undo nor redo: text and carets stay (SPEC-0057 v7
        // behaviour 9).
        if self.read_only && matches!(id, "$Undo" | "$Redo") {
            self.note = Some(self.read_only_note());
            cx.notify();
            return true;
        }
        // A refactoring across files is undone and redone as a whole (SPEC-0193 behaviours 3, 4).
        let step = match id {
            "$Undo" => self.state.buffer.top_step(),
            "$Redo" => self.state.buffer.redo_step(),
            _ => None,
        };
        if let Some(step) = step.filter(|s| self.group_steps.contains(s)) {
            cx.emit(EditorEvent::GroupUndo(step, id == "$Redo"));
            return true;
        }
        if let Some(step) = step
            && !self.read_only
        {
            cx.emit(EditorEvent::Undid(step, id == "$Redo"));
        }
        // Tab takes the inline completion, Escape hides it (SPEC-0157 behaviour 4).
        if id == "EditorTab" && self.accept_ghost(cx) {
            return true;
        }
        if id == "EditorEscape" && self.dismiss_ghost(cx) {
            return true;
        }
        // Escape takes the declaration Context Info shows away (SPEC-0057 v6 behaviour 27).
        if id == "EditorEscape" && self.hide_context(cx) {
            return true;
        }
        // Escape removes the usages kept by `HighlightUsagesInFile` and does nothing else
        // (SPEC-0295 behaviour 9).
        if id == "EditorEscape" && self.occurrences.escape() {
            cx.notify();
            return true;
        }
        match id {
            "$Paste" | "EditorPasteSimple" => {
                if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    let same = self
                        .state
                        .clipboard
                        .as_ref()
                        .is_some_and(|c| c.text == text);
                    // Text copied elsewhere joins the paste history (SPEC-0057 v3 behaviour 20).
                    if !self.masked {
                        idecraft_editor::paste_history::PasteHistory::global().record(&text);
                    }
                    if !same {
                        self.state.clipboard = Some(Clipboard {
                            text,
                            whole_lines: false,
                        });
                    }
                }
            }
            "SaveAll" | "SaveDocument" => {
                self.tidy_before_save(cx);
                self.save();
                cx.emit(EditorEvent::Saved);
                return true;
            }
            "EditorToggleColumnMode" if !self.field => {
                self.set_column_mode(!self.column_mode, cx);
                return true;
            }
            // A read-only text is not cut, and the clipboard stays (SPEC-0057 v6 behaviour 24).
            "EditorCutLineEnd" if self.read_only => {
                self.note = Some(self.read_only_note());
                cx.notify();
                return true;
            }
            // Input fields have no overwrite mode, no indentation to set right and no
            // declarations (SPEC-0057 v6 behaviours 25–27).
            "EditorToggleInsertState" | "AutoIndentLines" | "EditorContextInfo" if self.field => {
                return false;
            }
            "EditorContextInfo" => {
                self.context_info(cx);
                return true;
            }
            "EditorLeftWithSelection"
            | "EditorRightWithSelection"
            | "EditorUpWithSelection"
            | "EditorDownWithSelection"
                if self.column_mode =>
            {
                self.extend_column_block(id, cx);
                return true;
            }
            // The caret's row goes to the middle; folded lines take no rows (SPEC-0057 v4
            // behaviour 22).
            "EditorScrollToCenter" if !self.field => {
                let line = self.line_of(self.state.carets.primary().head);
                self.scroll_y = layout::centred(
                    self.view_height,
                    line_height(),
                    self.folds.row_of_line(line),
                    self.folds.rows(self.state.buffer.len_lines()),
                );
                cx.notify();
                return true;
            }
            // What these commands need from the syntax tree (SPEC-0071 v9 behaviour 12,
            // SPEC-0056 v4 behaviour 19).
            "MethodUp" | "MethodDown" => {
                self.state.methods = self
                    .with_syntax(SyntaxDocument::method_offsets)
                    .unwrap_or_default();
            }
            // A hidden text is not completed; the words of the other open files follow the
            // text's own (SPEC-0057 v5 behaviour 23).
            "HippieCompletion" | "HippieBackwardCompletion" => {
                if self.masked {
                    return true;
                }
                if !idecraft_editor::expand_word_continues(&self.state) {
                    self.state.other_words = self.other_open_words();
                }
            }
            "MoveElementLeft" | "MoveElementRight" => {
                let heads: Vec<usize> = self.state.carets.carets().iter().map(|c| c.head).collect();
                self.state.lists = self
                    .with_syntax(|d| heads.iter().map(|h| d.list_elements(*h)).collect())
                    .unwrap_or_default();
            }
            _ if !self.field && is_fold_action(id) => {
                let done = self.fold_action(id);
                cx.notify();
                return done;
            }
            _ => {}
        }
        let first_line = self.state.viewport.first_line;
        // Extend Selection grows along the syntax tree (SPEC-0057 v3 behaviour 13).
        if id == "EditorSelectWord" {
            self.state.structure = self.selection_stages();
        }
        let step = self.state.buffer.top_step();
        let done = idecraft_editor::perform(&mut self.state, id);
        // Cut up to Line End fills the clipboard only when it cut something (SPEC-0057 v6
        // behaviour 24).
        let cut = id == "EditorCutLineEnd" && self.state.buffer.top_step() != step;
        // What the command has to say goes to the status bar (SPEC-0332 behaviours 3 and 12); a
        // read-only text puts its own note there in `after_change`.
        if let Some(message) = self.state.message.take() {
            self.note = Some(message);
        }
        self.state.structure.clear();
        self.state.methods.clear();
        self.state.lists.clear();
        self.state.other_words.clear();
        if done {
            if (matches!(id, "$Copy" | "$Cut") || cut)
                && let Some(c) = &self.state.clipboard
            {
                cx.write_to_clipboard(ClipboardItem::new_string(c.text.clone()));
                idecraft_editor::paste_history::PasteHistory::global().record(&c.text);
            }
            if self.state.viewport.first_line != first_line {
                self.scroll_y =
                    self.folds.row_of_line(self.state.viewport.first_line) as f32 * line_height();
            }
            self.after_change(true, cx);
            return true;
        }
        false
    }

    /// The words of the other files open in editors, file by file in the order of their paths,
    /// for Cyclic Expand Word (`SPEC-0057` v5 behaviour 23). A field has no such source, and a
    /// text over the size limit offers none.
    fn other_open_words(&self) -> Vec<String> {
        use idecraft_editor::documents::DocumentRegistry;
        if self.field {
            return Vec::new();
        }
        let own = self.path.as_deref().map(DocumentRegistry::key);
        DocumentRegistry::global()
            .list()
            .into_iter()
            .filter(|d| Some(&d.path) != own.as_ref())
            .filter(|d| d.text.len_bytes() <= idecraft_editor::EXPAND_WORD_MAX)
            .flat_map(|d| idecraft_editor::words_of(&d.text.to_string()))
            .collect()
    }

    /// The syntax ranges around the primary caret for Extend Selection (`SPEC-0057` v3 behaviour
    /// 13): from the current tree, or from a fresh parse while the tree is being updated; empty for
    /// files without a grammar or too large for one.
    fn selection_stages(&self) -> Vec<std::ops::Range<usize>> {
        let at = self.state.carets.primary().range();
        self.with_syntax(|d| d.selection_ranges(at))
            .unwrap_or_default()
    }

    /// Runs `f` on the syntax tree of the current text: the editor's tree, or a fresh parse while
    /// that one is being updated. `None` for files without a grammar or too large for one.
    fn with_syntax<T>(&self, f: impl FnOnce(&SyntaxDocument) -> T) -> Option<T> {
        if let Some(doc) = self
            .syntax
            .as_ref()
            .filter(|_| self.syntax_edits.is_empty() && !self.parsing)
        {
            return Some(f(doc));
        }
        let language = self.path.as_deref().and_then(LanguageId::detect)?;
        let text = self.state.buffer.text();
        if text.len() > idecraft_languages::syntax::MAX_SYNTAX_BYTES {
            return None;
        }
        let tsx = self
            .path
            .as_deref()
            .and_then(|p| p.extension())
            .is_some_and(|e| e == "tsx");
        SyntaxDocument::new(language, tsx, &text)
            .ok()
            .map(|d| f(&d))
    }

    /// Makes the text read-only (a library source, `SPEC-0149` v4 behaviour 11).
    pub fn set_read_only(&mut self, on: bool) {
        self.locked = on;
        self.read_only = on;
        self.frozen = on.then(|| self.state.buffer.clone());
    }

    /// Takes the read-only state of the file (`SPEC-0336` behaviour 18): an editor of a
    /// read-only file takes no edits and does not save. A library source stays read-only, and
    /// fields and remote documents have no file to follow.
    pub fn follow_file_attribute(&mut self) {
        if self.locked || self.field || self.remote_document {
            return;
        }
        let Some(path) = &self.path else {
            return;
        };
        let Ok(read_only) = idecraft_core::file_properties::is_read_only(path) else {
            return;
        };
        if read_only != self.read_only {
            self.read_only = read_only;
            self.frozen = read_only.then(|| self.state.buffer.clone());
        }
    }

    /// What the status bar says when a read-only text is edited.
    fn read_only_note(&self) -> String {
        if self.locked {
            "Read-only: a library source".into()
        } else {
            "Read-only: the file is read-only (Toggle Read-Only Attribute)".into()
        }
    }

    /// The encoding the file is read and written in (`SPEC-0336` behaviour 11).
    pub fn encoding(&self) -> idecraft_core::file_properties::FileEncoding {
        self.encoding
    }

    /// Sets the encoding the text was read in and saving writes (`SPEC-0336` behaviours 11, 12).
    pub fn set_encoding(&mut self, encoding: idecraft_core::file_properties::FileEncoding) {
        self.encoding = encoding;
        self.choice_generation = idecraft_core::file_properties::choice_generation();
    }

    /// The file's text read from disk in its encoding, which becomes the editor's (`SPEC-0336`
    /// behaviour 11); `None` when it cannot be read as text.
    fn read_disk(&mut self) -> Option<String> {
        let file = idecraft_core::file_properties::read_text(self.path.as_deref()?, None).ok()?;
        self.set_encoding(file.encoding);
        Some(file.text)
    }

    /// Replaces every line break by `target` as one undo step, without saving (`SPEC-0336`
    /// behaviour 8). Returns how many line breaks changed; `None` for a read-only text.
    pub fn convert_line_separators(
        &mut self,
        target: idecraft_core::file_properties::LineSeparator,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.read_only {
            return None;
        }
        let (text, changed) =
            idecraft_core::file_properties::convert_separators(&self.state.buffer.text(), target);
        if changed > 0 {
            self.replace_text(&text, cx);
        }
        Some(changed)
    }

    /// Writes the text to the file in `encoding`, which becomes the file's (`SPEC-0336`
    /// behaviours 14, 15): unsaved changes are written too, the file is replaced in one step and
    /// the editor is saved afterwards. A text `encoding` cannot hold is refused unless `lossy`;
    /// then each such character becomes `?` in the text too, as one undo step. Returns how many
    /// characters were replaced.
    pub fn save_in_encoding(
        &mut self,
        encoding: idecraft_core::file_properties::FileEncoding,
        lossy: bool,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        use idecraft_core::file_properties as fp;
        let path = self
            .path
            .clone()
            .filter(|_| !self.remote_document)
            .ok_or("it is not a file of this machine")?;
        if self.read_only {
            return Err(fp::Refusal::ReadOnly.to_string());
        }
        let mut replaced = 0;
        if let Some(u) = fp::unrepresentable(&self.state.buffer.text(), encoding.charset) {
            if !lossy {
                return Err(u.message(encoding.charset));
            }
            let (_, written, n) = fp::encode_lossy(&self.state.buffer.text(), encoding);
            replaced = n;
            self.replace_text(&written, cx);
        }
        let text = self.state.buffer.text();
        let bytes = fp::encode(&text, encoding).map_err(|u| u.message(encoding.charset))?;
        // The file itself, not a link to it, is replaced.
        let target = path.canonicalize().map_err(|e| e.to_string())?;
        self.record_history(&path);
        fp::write_atomic_keeping_mode(&target, &bytes).map_err(|e| e.to_string())?;
        fp::choose(&path, encoding.charset);
        self.set_encoding(encoding);
        self.state.buffer.mark_saved();
        self.publish_document();
        self.disk = Some(text);
        self.overwrite_armed = false;
        self.disk_stamp = std::fs::metadata(&path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        self.saves += 1;
        self.note = None;
        self.record_history(&path);
        cx.emit(EditorEvent::Saved);
        cx.notify();
        Ok(replaced)
    }

    /// Records the file's content on disk in the local history (`SPEC-0058` behaviour 9).
    fn record_history(&self, path: &std::path::Path) {
        let Some(root) = self.history_root.as_deref() else {
            return;
        };
        if let (Ok(mut history), Ok(relative)) = (
            idecraft_core::LocalHistory::open(root),
            path.strip_prefix(root),
        ) {
            let _ = history.record(&relative.to_string_lossy().replace('\\', "/"));
        }
    }

    /// Whether the text is read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Makes this the editor of a remote document (`SPEC-0317` v3 behaviour 8): ⌘S asks the
    /// workbench to save on the host ([`EditorEvent::Saved`] without writing here), and the disk
    /// of this machine is never read or written for it.
    pub fn set_remote_document(&mut self, on: bool) {
        self.remote_document = on;
        // The backend on the host keeps the registry of its documents for agents (SPEC-0220);
        // this machine's registry would name a path that is not a file here.
        if on {
            self.document = None;
        }
    }

    /// Makes Save All in this editor the project window's (`SPEC-0058` v4 behaviour 12): it emits
    /// [`EditorEvent::SaveAll`], and the window saves every editor.
    pub fn set_save_all_in_window(&mut self, on: bool) {
        self.save_all_in_window = on;
    }

    /// Whether this is the editor of a remote document.
    pub fn is_remote_document(&self) -> bool {
        self.remote_document
    }

    /// Shows the host's inspection findings of a remote document (`lsp.inspect`, `SPEC-0318` v2
    /// detail 9), computed on the text of `revision`; an answer for an older text is dropped.
    /// Each item: inspection id, level, message, start and end as 0-based line and character.
    pub fn set_host_findings(
        &mut self,
        revision: u64,
        items: &[HostFinding],
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.remote_document || revision != self.revision {
            return false;
        }
        let at = |(line, column): (usize, usize)| {
            self.state
                .buffer
                .position_to_offset(idecraft_editor::Position::new(line, column))
                .ok()
        };
        let mut found = Vec::new();
        for (id, level, message, from, to) in items {
            let (Some(start), Some(end)) = (at(*from), at(*to)) else {
                continue;
            };
            let severity = match level.as_str() {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                // "No highlighting, only fix" stays in ⌥⏎ (SPEC-0315 v2 behaviour 7).
                "info" => continue,
                _ => Severity::Weak,
            };
            found.push(Problem {
                range: start..end.max(start + 1),
                severity,
                message: format!("IDECraft: {message}"),
                inspection: idecraft_languages::inspections::known_id(id),
                unused: idecraft_languages::inspections::is_unused(id),
                code: None,
            });
        }
        self.host_problems = found;
        self.merge_problems();
        cx.notify();
        true
    }

    /// The text is saved now (a remote document saved on the host, `SPEC-0317` behaviour 10).
    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.state.buffer.mark_saved();
        self.note = None;
        cx.notify();
    }

    /// Takes changes made elsewhere (char ranges of the current text, sorted, not overlapping)
    /// without moving the caret off the text it is on (`SPEC-0317` behaviour 9).
    pub fn merge_ranges(&mut self, edits: &[(Range<usize>, String)], cx: &mut Context<Self>) {
        if edits.is_empty() {
            return;
        }
        let was_read_only = self.read_only;
        self.read_only = false;
        let caret = self.state.carets.primary().head;
        let top = self.state.buffer.top_step();
        self.replace_ranges(edits, cx);
        let shift: isize = edits
            .iter()
            .filter(|(r, _)| r.end <= caret)
            .map(|(r, t)| t.chars().count() as isize - r.len() as isize)
            .sum();
        let at = ((caret as isize + shift).max(0) as usize).min(self.state.buffer.len_chars());
        self.select_range(at..at, cx);
        self.note_carets_after(top);
        self.read_only = was_read_only;
    }

    /// Puts the caret at a 0-based line and column (clamped to the text) and scrolls it into view.
    pub fn go_to(&mut self, line: usize, column: usize, cx: &mut Context<Self>) {
        let line = line.min(self.state.buffer.len_lines().saturating_sub(1));
        let len = self
            .state
            .buffer
            .line(line)
            .map(|t| t.trim_end_matches(['\n', '\r']).chars().count())
            .unwrap_or(0);
        let at = self.line_start(line) + column.min(len);
        // A jump opens the fold it lands in, also a method folded to one line (SPEC-0071 v7
        // behaviour 10).
        self.folds.open_at(line);
        if !self.folded_on_open {
            self.jumped_to = Some(line);
        }
        // A jump starts afresh: an older note ("Reloaded: …") no longer describes what is shown
        // (hand test 209).
        self.note = None;
        self.select_range(at..at, cx);
    }

    /// Puts the caret at the start of a 0-based line and scrolls it into view.
    pub fn go_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let at = self.line_start(line.min(self.state.buffer.len_lines().saturating_sub(1)));
        self.select_range(at..at, cx);
    }

    /// The file this editor shows was renamed to `path` (a Java class taking its file along,
    /// SPEC-0148 v2 behaviour 6): text, undo steps and unsaved changes stay.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// The language of the file changed (an override or pattern, `SPEC-0004` v5 behaviour 12):
    /// comment tokens, highlighting and the tree follow it; text, carets, undo steps and unsaved
    /// changes stay. False when the language is the one the editor has.
    pub fn redetect_language(&mut self, cx: &mut Context<Self>) -> bool {
        let language = self.path.as_deref().and_then(LanguageId::detect);
        let id = language.map(|l| l.lsp_id().to_owned());
        if id == self.state.language {
            return false;
        }
        self.state.comment = language
            .and_then(LanguageId::line_comment)
            .map(str::to_owned);
        self.state.block_comment = language
            .and_then(LanguageId::block_comment)
            .map(|(o, c)| (o.to_owned(), c.to_owned()));
        self.state.language = id;
        let text = self.state.buffer.text();
        let tsx = self
            .path
            .as_deref()
            .and_then(|p| p.extension())
            .is_some_and(|e| e == "tsx");
        let tree_language =
            language.filter(|_| text.len() <= idecraft_languages::syntax::MAX_SYNTAX_BYTES);
        self.syntax_edits.clear();
        self.colors.clear();
        self.syntax = None;
        match tree_language {
            Some(l) if text.len() > idecraft_languages::syntax::BACKGROUND_PARSE_BYTES => {
                self.parse_in_background(l, tsx, text, cx);
            }
            Some(l) => {
                self.syntax = SyntaxDocument::new(l, tsx, &text).ok();
                self.refresh_inspections();
            }
            None => self.refresh_inspections(),
        }
        cx.notify();
        true
    }

    /// The language the editor treats its text as: the LSP id, `None` for plain text.
    pub fn language_id(&self) -> Option<&str> {
        self.state.language.as_deref()
    }

    /// The file this editor shows now (after a rename, its new name).
    pub fn file_path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    /// Records saves in the local history of the workspace at `root` (behaviour 9).
    pub fn set_history_root(&mut self, root: PathBuf) {
        // Canonical like the file paths, so a root behind a symbolic link (`/var` on macOS) still
        // contains them.
        self.history_root = Some(root.canonicalize().unwrap_or(root));
    }

    /// What Settings › Editor asks of saving (`SPEC-0129` behaviour 4): trailing spaces removed
    /// except on the caret line, a final line break; EditorConfig overrides both and converts line
    /// breaks (`SPEC-0137` behaviour 3). One undo step, the caret stays.
    fn tidy_before_save(&mut self, cx: &mut Context<Self>) {
        if self.field || self.path.is_none() {
            return;
        }
        let settings = crate::appearance::ide_settings(cx).editor;
        let text = self.state.buffer.text();
        let caret = self.state.carets.primary().head;
        let keep = self
            .state
            .buffer
            .offset_to_position(caret)
            .ok()
            .map(|p| p.line);
        let edits = idecraft_core::editorconfig::save_edits(
            &text,
            &self.editorconfig,
            settings.strip_trailing_spaces,
            settings.ensure_line_break_at_end,
            keep,
        );
        if edits.is_empty() {
            return;
        }
        let chars = |b: usize| text[..b].chars().count();
        let ranges: Vec<(Range<usize>, String)> = edits
            .into_iter()
            .map(|(r, t)| (chars(r.start)..chars(r.end), t))
            .collect();
        let shift: isize = ranges
            .iter()
            .filter(|(r, _)| r.end <= caret)
            .map(|(r, t)| t.chars().count() as isize - r.len() as isize)
            .sum();
        self.replace_ranges(&ranges, cx);
        let at = ((caret as isize + shift).max(0) as usize).min(self.state.buffer.len_chars());
        self.select_range(at..at, cx);
    }

    /// Writes the buffer (behaviour 9): refuses once when the file changed on disk since it was read
    /// or saved, so another program's change is not lost silently; records the previous and the new
    /// content in the local history.
    fn save(&mut self) {
        // A remote document is saved on the host by the workbench (`SPEC-0317` behaviour 10).
        if self.remote_document {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let mut history = self
            .history_root
            .as_deref()
            .and_then(|root| Some((idecraft_core::LocalHistory::open(root).ok()?, root)));
        let relative = history.as_ref().and_then(|(_, root)| {
            path.strip_prefix(root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        });
        let mut record = || {
            if let (Some((h, _)), Some(rel)) = (history.as_mut(), relative.as_deref()) {
                let _ = h.record(rel);
            }
        };
        match write_buffer(
            &path,
            &mut self.state.buffer,
            &mut self.disk,
            &mut self.overwrite_armed,
            self.document.as_ref(),
            self.encoding,
            &mut record,
        ) {
            SaveOutcome::Saved => {
                self.saves += 1;
                self.note = None;
            }
            SaveOutcome::ChangedOnDisk => {
                self.note = Some(
                    "The file changed on disk since it was opened: save again to overwrite it."
                        .into(),
                );
            }
            SaveOutcome::Failed(e) => self.note = Some(format!("Could not save: {e}")),
        }
    }

    /// Takes the file's content from disk, discarding unsaved edits (a rollback in the Commit tool
    /// window, `SPEC-0063` v5 behaviour 10). A file that is gone keeps the buffer.
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.remote_document {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(now) = self.read_disk() else {
            return;
        };
        self.follow_file_attribute();
        self.disk_stamp = std::fs::metadata(&path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        // The text of a file that is read-only is replaced like any other.
        if self.frozen.is_some() {
            self.frozen = Some(idecraft_editor::TextBuffer::from_text(&now));
        }
        if self.state.buffer.text() != now || self.state.buffer.is_modified() {
            self.reload_buffer(&now);
            self.after_change(false, cx);
        }
        self.disk = Some(now);
        self.overwrite_armed = false;
        cx.notify();
    }

    /// Takes the text `now` from disk as one undo step; undo puts the carets back where they were
    /// (`SPEC-0026` behaviour 5, `SPEC-0057` v7 behaviour 9).
    fn reload_buffer(&mut self, now: &str) {
        let before = (self.state.buffer.top_step(), self.state.carets.clone());
        self.state.buffer.reload(now);
        let len = self.state.buffer.len_chars();
        let head = self.state.carets.primary().head.min(len);
        self.state.carets = Carets::new(head);
        if self.state.buffer.top_step() != before.0 {
            self.state.buffer.note_carets(&before.1, &self.state.carets);
        }
    }

    /// [`Self::check_disk`] without trusting the file's time stamp and size: the content is read
    /// and compared in any case (`ForceRefresh`, `SPEC-0059` v9.8 behaviour 30a).
    pub fn check_disk_now(&mut self, cx: &mut Context<Self>) {
        self.disk_stamp = None;
        self.check_disk(cx);
    }

    /// Compares the file on disk with what the editor last read or wrote (behaviour 10): an unmodified
    /// buffer reloads another program's change; a modified one keeps its text and says so.
    pub fn check_disk(&mut self, cx: &mut Context<Self>) {
        // The host's changes reach a remote document through the session.
        if self.remote_document {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        // A changed `.editorconfig` reaches the open file (`SPEC-0137` v2 behaviour 2b).
        if self.check_editorconfig() {
            cx.notify();
        }
        // The file's read-only state is followed (`SPEC-0336` behaviour 18).
        let was_read_only = self.read_only;
        self.follow_file_attribute();
        if was_read_only != self.read_only {
            cx.notify();
        }
        let stamp = std::fs::metadata(&path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        // Unchanged bytes are read again when another encoding was chosen for a file
        // (`SPEC-0336` behaviour 13).
        if stamp.is_some()
            && stamp == self.disk_stamp
            && self.choice_generation == idecraft_core::file_properties::choice_generation()
        {
            return;
        }
        self.disk_stamp = stamp;
        let Some(now) = self.read_disk() else {
            return;
        };
        if self.disk.as_deref() == Some(now.as_str()) {
            return;
        }
        match self.state.buffer.reconcile(&now) {
            idecraft_editor::Reconcile::Unchanged => {}
            idecraft_editor::Reconcile::Reload => {
                // A read-only file's text is replaced, not put back.
                if self.frozen.is_some() {
                    self.frozen = Some(idecraft_editor::TextBuffer::from_text(&now));
                }
                self.reload_buffer(&now);
                self.note = Some("Reloaded: the file changed on disk.".into());
                self.after_change(false, cx);
            }
            idecraft_editor::Reconcile::Conflict => {
                self.note = Some(
                    "The file changed on disk and here: saving overwrites the disk version.".into(),
                );
                // Saving now is a deliberate overwrite; the note says so.
                self.overwrite_armed = true;
            }
        }
        self.disk = Some(now);
        cx.notify();
    }

    /// Whether the input method composes here (marked text): it owns the keys then (`SPEC-0028`
    /// v6 behaviour 11a).
    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            return; // The input method owns keys while it composes.
        }
        let m = &event.keystroke.modifiers;
        let Some(stroke) =
            Keystroke::from_platform(&event.keystroke.key, m.control, m.alt, m.shift, m.platform)
        else {
            return;
        };
        if self.press(&stroke, cx) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// Char offset under a window position, from the last frame.
    fn offset_at(&self, position: gpui::Point<Pixels>) -> usize {
        let Some(first) = self.last.lines.first() else {
            return 0;
        };
        let row = ((position.y - first.origin.y) / px(line_height())).floor();
        let painted = if row < 0.0 {
            first
        } else {
            self.last
                .lines
                .get(row as usize)
                .unwrap_or_else(|| self.last.lines.last().expect("not empty"))
        };
        let byte = painted
            .shaped
            .closest_index_for_x(position.x - painted.origin.x);
        self.line_start(painted.line) + painted.display.column_of(byte)
    }

    /// A right click on a breakpoint in the gutter edits it (`SPEC-0127` behaviour 2).
    /// Before the context menu opens (`SPEC-0275` behaviour 14): a secondary click on the text
    /// outside every selection moves the caret there, so Go To and Refactor act on the clicked
    /// symbol; inside a selection the selection stays for Cut and Copy.
    pub fn caret_for_menu(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        if self.field || position.x < self.last.gutter_x {
            return;
        }
        let at = self.offset_at(position);
        if !menu_keeps_selection(self.state.carets.carets(), at) {
            self.state.carets = Carets::new(at);
            cx.notify();
        }
    }

    /// Whether any text is selected.
    pub fn has_selection(&self) -> bool {
        self.state.carets.carets().iter().any(|c| !c.is_empty())
    }

    fn on_right_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.field
            || event.position.x >= self.last.gutter_x
            || self.annotation_line_at(event.position).is_some()
        {
            return;
        }
        if let Some(line) = self
            .last
            .lines
            .iter()
            .find(|p| {
                event.position.y >= p.origin.y && event.position.y < p.origin.y + px(line_height())
            })
            .map(|p| p.line)
            .filter(|l| {
                self.breakpoints.binary_search(l).is_ok() || self.method_breakpoints.contains_key(l)
            })
        {
            cx.emit(EditorEvent::EditBreakpoint(line));
            cx.stop_propagation();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        // A Control-click that arrives as a primary click on macOS opens the context menu; the
        // caret stays for it (SPEC-0275 v20 behaviour 14b).
        if crate::mouse::is_secondary(event) {
            return;
        }
        // A click on an annotation shows its commit (SPEC-0277 behaviour 5).
        if let Some(line) = self
            .annotation_line_at(event.position)
            .filter(|_| !event.modifiers.control)
        {
            if let Some(id) = self
                .annotations
                .as_ref()
                .and_then(|a| a.commit_at(line))
                .map(|c| c.info.id.clone())
            {
                self.annotation_hover = None;
                cx.emit(EditorEvent::ShowCommit(id));
                cx.notify();
            }
            return;
        }
        // A click on a change marker opens its bar (SPEC-0064 v3 behaviour 6).
        if event.click_count == 1
            && let Some(index) = self.marker_change_at(event.position)
        {
            // A change hidden in a fold is shown first; the bar opens below it once drawn.
            let Some(at) = self.change_bar_anchor(index) else {
                self.goto_change(index, cx);
                cx.stop_propagation();
                return;
            };
            cx.emit(EditorEvent::ChangeMarker(index, at));
            cx.stop_propagation();
            return;
        }
        // A click on a declaration's author shows or hides the annotations (SPEC-0292 v4
        // behaviour 7e).
        if self
            .last
            .author_hints
            .iter()
            .any(|(b, _, _)| b.contains(&event.position))
        {
            cx.emit(EditorEvent::ToggleAnnotations);
            cx.stop_propagation();
            return;
        }
        if self.fold_click(event.position) {
            cx.notify();
            return;
        }
        // A click on a declaration's mark goes where it points (SPEC-0320 behaviour 9).
        let run_w = self.run_width();
        let marks_w = self.marks_width();
        let declaration_w = self.declaration_width();
        // A click on the mark of a bean or an injection point goes to the other side of the
        // wiring (SPEC-0115 v5 behaviour 14); its lane is left of the declarations'.
        if marks_w > declaration_w
            && event.position.x >= self.last.gutter_x - px(FOLD_WIDTH) - run_w - marks_w
            && event.position.x < self.last.gutter_x - px(FOLD_WIDTH) - run_w - declaration_w
            && let Some(line) = self
                .last
                .lines
                .iter()
                .find(|p| {
                    event.position.y >= p.origin.y
                        && event.position.y < p.origin.y + px(line_height())
                })
                .map(|p| p.line)
                .filter(|l| self.framework_marks.at(*l).is_some())
        {
            cx.emit(EditorEvent::FrameworkMark(line));
            return;
        }
        if declaration_w > px(0.0)
            && event.position.x >= self.last.gutter_x - px(FOLD_WIDTH) - run_w - declaration_w
            && event.position.x < self.last.gutter_x - px(FOLD_WIDTH) - run_w
            && let Some(line) = self
                .last
                .lines
                .iter()
                .find(|p| {
                    event.position.y >= p.origin.y
                        && event.position.y < p.origin.y + px(line_height())
                })
                .map(|p| p.line)
                .filter(|l| self.declaration_marks.at(*l).is_some())
        {
            cx.emit(EditorEvent::DeclarationMark(line));
            return;
        }
        // A click on a ▶ asks to run what it marks (SPEC-0122 behaviour 1).
        if run_w > px(0.0)
            && event.position.x >= self.last.gutter_x - px(FOLD_WIDTH) - run_w
            && event.position.x < self.last.gutter_x - px(FOLD_WIDTH)
            && let Some(line) = self
                .last
                .lines
                .iter()
                .find(|p| {
                    event.position.y >= p.origin.y
                        && event.position.y < p.origin.y + px(line_height())
                })
                .map(|p| p.line)
                .filter(|l| self.run_markers.binary_search(l).is_ok())
        {
            cx.emit(EditorEvent::RunMarker(line));
            return;
        }
        // A click on a line number sets or removes its breakpoint (SPEC-0091 behaviour 1).
        if !self.field
            && event.click_count == 1
            && event.position.x < self.last.gutter_x - px(FOLD_WIDTH) - run_w - marks_w
            && let Some(line) = self
                .last
                .lines
                .iter()
                .find(|p| {
                    event.position.y >= p.origin.y
                        && event.position.y < p.origin.y + px(line_height())
                })
                .map(|p| p.line)
        {
            self.toggle_breakpoint(line, cx);
            return;
        }
        let at = self.offset_at(event.position);
        let m = &event.modifiers;
        let line = self.line_of(at);
        let (line_start, next_line) = (self.line_start(line), self.line_start(line + 1));
        // ⌘-click (Ctrl-click elsewhere) is `GotoDeclaration` on the clicked symbol (SPEC-0071).
        let jump = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        if event.click_count == 1 && jump && !m.shift && !m.alt {
            self.state.carets = Carets::new(at);
            cx.emit(EditorEvent::GotoDeclaration);
            cx.notify();
            return;
        }
        // The two clicks of SPEC-0130 are mouse shortcuts of the keymap in use (v2 behaviour 5):
        // ⌥ with the primary button starts a column selection, ⌥⇧ adds or removes a caret.
        let (column_click, caret_click) = if event.click_count == 1 {
            let bound = crate::keymaps::primary_click_actions(m);
            (
                bound
                    .iter()
                    .any(|a| a == "EditorCreateRectangularSelectionOnMouseDrag"),
                bound.iter().any(|a| a == "EditorAddOrRemoveCaret"),
            )
        } else {
            (false, false)
        };
        // The click starts a column selection that the drag stretches (SPEC-0130 behaviour 1); in
        // column selection mode a plain click does (SPEC-0151 behaviour 2).
        let plain = !m.alt && !m.shift && !m.control && !m.platform;
        if event.click_count == 1
            && (column_click || (self.column_mode && plain))
            && !caret_click
            && !self.field
        {
            let corner = self.column_at(event.position);
            self.column_select(corner, corner, cx);
            self.column_drag = Some(corner);
            self.dragging = true;
            return;
        }
        let carets = &mut self.state.carets;
        match event.click_count {
            2 => {
                *carets = Carets::new(at);
                idecraft_editor::perform(&mut self.state, "EditorSelectWord");
            }
            3 => *carets = Carets::from_carets(vec![Caret::select(line_start, next_line)]),
            _ if caret_click => {
                carets.toggle_caret(at);
            }
            _ if m.shift => {
                let anchor = carets.primary().anchor;
                *carets = Carets::from_carets(vec![Caret::select(anchor, at)]);
            }
            _ => *carets = Carets::new(at),
        }
        self.dragging = event.click_count == 1 && !caret_click;
        self.pending_input.get_or_insert_with(Instant::now);
        cx.notify();
    }

    /// A click on a folding mark toggles its region; a click on a placeholder unfolds it
    /// (`SPEC-0083` behaviour 2). Returns whether the click was one of these.
    fn fold_click(&mut self, position: gpui::Point<Pixels>) -> bool {
        if self.field {
            return false;
        }
        let Some(painted) = self
            .last
            .lines
            .iter()
            .find(|p| position.y >= p.origin.y && position.y < p.origin.y + px(line_height()))
        else {
            return false;
        };
        let line = painted.line;
        let gutter_x = self.last.gutter_x;
        let in_marks = position.x >= gutter_x - px(FOLD_WIDTH) && position.x < gutter_x;
        let after_text = position.x > painted.origin.x + painted.shaped.width;
        if self.folds.folded_at(line).is_some() && (in_marks || after_text) {
            self.folds.unfold(line);
            return true;
        }
        if in_marks && let Some(&(s, e)) = self.regions().iter().find(|&&(s, _)| s == line) {
            self.folds.fold(s, e);
            let head = self.line_of(self.state.carets.primary().head);
            if self.folds.hidden(head) {
                self.state.carets = Carets::new(self.line_start(s));
            }
            self.settle_caret_in_folds();
            return true;
        }
        false
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The hover card of the annotation under the pointer (SPEC-0277 behaviour 4).
        if self.annotations.is_some() && !self.dragging {
            let hover = self
                .annotation_line_at(event.position)
                .map(|line| (line, event.position));
            let same_line = hover.map(|h| h.0) == self.annotation_hover.map(|h| h.0);
            if !same_line || (hover.is_some() && self.annotation_hover != hover) {
                let changed = !same_line;
                self.annotation_hover = hover;
                if changed {
                    cx.notify();
                }
            }
        }
        if event.pressed_button.is_none() && !self.field {
            self.track_problem_hover(event.position, cx);
            self.track_doc_hover(event.position, cx);
        }
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }
        if let Some(corner) = self.column_drag {
            let to = self.column_at(event.position);
            self.column_select(corner, to, cx);
            return;
        }
        let at = self.offset_at(event.position);
        let anchor = self.state.carets.primary().anchor;
        self.state.carets = Carets::from_carets(vec![Caret::select(anchor, at)]);
        cx.notify();
    }

    /// Line and column under a window position, from the last frame; columns go on past the end
    /// of a line (one per character width of the monospace font), lines stop at the first and last
    /// visible ones.
    fn column_at(&self, position: gpui::Point<Pixels>) -> idecraft_editor::Position {
        let lines = &self.last.lines;
        let Some(first) = lines.first() else {
            return idecraft_editor::Position::new(0, 0);
        };
        let painted = lines
            .iter()
            .find(|p| position.y >= p.origin.y && position.y < p.origin.y + px(line_height()))
            .unwrap_or(if position.y < first.origin.y {
                first
            } else {
                lines.last().expect("not empty")
            });
        // The width the last frame measured for the editor font.
        let width = crate::fonts::editor_advance();
        let column = (f32::from(position.x - painted.origin.x) / width)
            .round()
            .max(0.0);
        idecraft_editor::Position::new(painted.line, column as usize)
    }

    /// A selection on every line from `from` to `to` (0-based line and column), between the two
    /// columns; lines shorter than that get a caret at their end (`SPEC-0130` behaviour 2).
    pub fn column_select(
        &mut self,
        from: idecraft_editor::Position,
        to: idecraft_editor::Position,
        cx: &mut Context<Self>,
    ) {
        self.state.carets.block(&self.state.buffer, from, to);
        self.column_block = Some((from, to, self.state.carets.primary().head));
        self.pending_input.get_or_insert_with(Instant::now);
        cx.notify();
    }

    /// Whether column selection mode is on (SPEC-0151).
    pub fn column_mode(&self) -> bool {
        self.column_mode
    }

    /// Whether overwrite mode is on (`EditorToggleInsertState`, `SPEC-0057` v6 behaviour 25); an
    /// input field has none.
    pub fn overwrite(&self) -> bool {
        self.state.overwrite && !self.field
    }

    /// Switches overwrite mode (the status bar's widget switches it off).
    pub fn set_overwrite(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.field {
            return;
        }
        self.state.overwrite = on;
        cx.notify();
    }

    /// Widths in pixels of the carets the last frame drew (`SPEC-0057` v6 behaviour 25).
    pub fn painted_caret_widths(&self) -> &[f32] {
        &self.last.caret_widths
    }

    /// The declaration Context Info shows at the top of the editor: its 0-based line and that
    /// line's text (`SPEC-0057` v6 behaviour 27). `None` once the caret, the text or the scroll
    /// position are no longer those it was shown for.
    pub fn shown_context(&self) -> Option<(usize, String)> {
        let (line, revision, caret, scroll) = self.context?;
        (revision == self.revision
            && caret == self.state.carets.primary().head
            && scroll == self.scroll_y)
            .then(|| (line, self.state.buffer.line(line).unwrap_or_default()))
    }

    /// Takes the shown declaration away; returns whether one was shown.
    fn hide_context(&mut self, cx: &mut Context<Self>) -> bool {
        let shown = self.shown_context().is_some();
        self.context = None;
        if shown {
            cx.notify();
        }
        shown
    }

    /// Context Info (`EditorContextInfo`, `SPEC-0057` v6 behaviour 27): shows the innermost
    /// declaration around the caret whose line is above the first visible row; used again, the
    /// next one outwards.
    fn context_info(&mut self, cx: &mut Context<Self>) {
        let shown = self.shown_context().map(|(line, _)| line);
        let caret = self.state.carets.primary().head;
        // The first row that is in view from its top on.
        let first_row = (self.scroll_y / line_height()).ceil().max(0.0) as usize;
        let first_visible = self.folds.line_of_row(first_row);
        let found = self.with_syntax(|doc| {
            let text = doc.text();
            let byte = text
                .char_indices()
                .nth(caret)
                .map_or(text.len(), |(b, _)| b);
            let around = doc.definitions_around(byte);
            let show =
                idecraft_languages::syntax::context_declaration(&around, first_visible, shown)
                    .map(|d| d.line);
            (around.is_empty(), show)
        });
        self.context = None;
        match found {
            None | Some((true, _)) => self.note = Some(NO_CONTEXT.into()),
            Some((false, Some(line))) => {
                self.context = Some((line, self.revision, caret, self.scroll_y));
            }
            Some((false, None)) => {}
        }
        cx.notify();
    }

    /// The strip Context Info puts over the top of the editor: the declaration's line in the
    /// editor font, where the text of the editor starts.
    fn context_strip(&self) -> Option<gpui::AnyElement> {
        let (_, text) = self.shown_context()?;
        let p = Palette::current();
        let c = crate::workbench_view::c;
        let left = self
            .last
            .lines
            .first()
            .map_or(self.last.gutter_x, |line| line.origin.x)
            - self.last.origin.x;
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(line_height()))
                .overflow_hidden()
                .bg(c(p.popup))
                .border_b_1()
                .border_color(c(p.popup_border))
                .child(
                    div()
                        .absolute()
                        .left(left)
                        .whitespace_nowrap()
                        .font_family(crate::fonts::editor_family())
                        .text_size(px(crate::fonts::editor_font_size()))
                        .text_color(color(p.text))
                        .child(DisplayLine::new(&text).text),
                )
                .into_any_element(),
        )
    }

    /// Switches column selection mode (`EditorToggleColumnMode`, ⇧⌘8 on macOS); the carets stay
    /// (SPEC-0151 behaviour 1).
    pub fn set_column_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.field {
            return;
        }
        self.column_mode = on;
        self.column_block = None;
        cx.notify();
    }

    /// Moves the free corner of the rectangle one column or line with ⇧-arrows in column mode;
    /// columns go on past the line end, so a rectangle can reach over short lines (SPEC-0151
    /// behaviour 3). Starts at the primary caret when the carets are no longer the rectangle's.
    fn extend_column_block(&mut self, id: &str, cx: &mut Context<Self>) {
        let head = self.state.carets.primary().head;
        let (from, to) = match self.column_block {
            Some((from, to, at)) if at == head => (from, to),
            _ => {
                let p = self
                    .state
                    .buffer
                    .offset_to_position(head.min(self.state.buffer.len_chars()))
                    .unwrap_or_default();
                (p, p)
            }
        };
        let last = self.state.buffer.len_lines().saturating_sub(1);
        let to = match id {
            "EditorLeftWithSelection" => {
                idecraft_editor::Position::new(to.line, to.column.saturating_sub(1))
            }
            "EditorRightWithSelection" => idecraft_editor::Position::new(to.line, to.column + 1),
            "EditorUpWithSelection" => {
                idecraft_editor::Position::new(to.line.saturating_sub(1), to.column)
            }
            _ => idecraft_editor::Position::new((to.line + 1).min(last), to.column),
        };
        self.column_select(from, to, cx);
        self.reveal_line(to.line);
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(line_height()));
        let rows = self.folds.rows(self.state.buffer.len_lines());
        let max = layout::max_scroll(rows, line_height());
        self.scroll_y = (self.scroll_y - f32::from(delta.y)).clamp(0.0, max);
        cx.notify();
    }

    /// Shapes the visible lines and records where everything goes (called before painting).
    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> Frame {
        self.view_height = f32::from(bounds.size.height);
        // The marks of the usages go in the frame in which the caret leaves the symbol
        // (SPEC-0295 behaviour 7).
        if !self.field {
            self.occurrences
                .caret_at(self.occurrence_place(), Instant::now());
        }
        if !self.laid_out {
            self.laid_out = true;
            if let Some(line) = self.reveal_before_layout.take() {
                self.reveal_line(line);
            }
        }
        // A caret that got behind the shown part of a folded line opens the fold (SPEC-0083 v8
        // behaviour 21).
        if !self.field {
            self.open_fold_under_caret();
        }
        let width = f32::from(bounds.size.width);
        // The problem indicator around the view picks its form from this width (SPEC-0070 v7
        // behaviour 12): when the view crosses the narrow limit (a split, a resize), draw the
        // window once more, or the old form stayed until the next input (OBS-0022 re-check).
        let limit = crate::diagnostics_model::COMPACT_BELOW;
        if (self.view_width < limit) != (width < limit) {
            // A refresh asked for while painting is dropped; ask for it in the next frame.
            window.on_next_frame(|window, _| window.refresh());
        }
        self.view_width = width;
        let total = self.state.buffer.len_lines();
        // Folded lines take no rows (SPEC-0083 behaviour 1).
        let total_rows = self.folds.rows(total);
        let (first_row, count) =
            layout::visible_lines(self.scroll_y, self.view_height, line_height(), total_rows);
        let first = self.folds.line_of_row(first_row);
        let last_line = if count == 0 {
            first
        } else {
            self.folds.line_of_row(first_row + count - 1)
        };
        let regions = if self.field {
            Vec::new()
        } else {
            self.regions()
        };
        self.state.viewport.first_line = first;
        self.state.viewport.lines = (self.view_height / line_height()).floor().max(1.0) as usize;
        let text_system = window.text_system().clone();
        let font_size = px(crate::fonts::editor_font_size());
        let base = editor_font();
        let p = Palette::current();
        let run = |len: usize, c: u32| TextRun {
            len,
            font: base.clone(),
            color: color(c),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let styled = |len: usize, style: &idecraft_ui::theme::TextStyle| TextRun {
            len,
            font: Font {
                weight: if style.bold {
                    gpui::FontWeight::BOLD
                } else {
                    base.weight
                },
                style: if style.italic {
                    gpui::FontStyle::Italic
                } else {
                    base.style
                },
                ..base.clone()
            },
            color: color(style.foreground.unwrap_or(p.text)),
            background_color: style.background.map(color),
            // A line under the text in the effect colour, e.g. a reassigned variable
            // (`SPEC-0027` v11).
            underline: style
                .effect
                .filter(|_| style.underline)
                .map(|c| gpui::UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color(c)),
                    wavy: false,
                }),
            strikethrough: None,
        };
        let digits = layout::gutter_digits(total);
        let digit_width = text_system
            .shape_line("0".into(), font_size, &[run(1, p.text)], None)
            .width;
        // The annotation column at the left of the gutter (SPEC-0277 behaviour 2).
        let annotation_w = match &self.annotations {
            Some(a) if !self.field => {
                digit_width * a.width_chars() as f32 + px(2.0 * ANNOTATION_PADDING)
            }
            _ => px(0.0),
        };
        let gutter = if self.field {
            px(0.0)
        } else {
            digit_width * digits as f32
                + px(2.0 * GUTTER_PADDING + FOLD_WIDTH)
                + self.run_width()
                + self.marks_width()
                + annotation_w
        };
        let run_w = self.run_width();
        let marks_w = self.marks_width();
        let value_carets: Vec<Range<usize>> = self
            .state
            .carets
            .carets()
            .iter()
            .map(|c| c.range())
            .collect();
        let text_x = bounds.origin.x + gutter + px(TEXT_PADDING);
        let top = bounds.origin.y - px(self.scroll_y - first_row as f32 * line_height());

        let start_char = self.line_start(first);
        let end_char = self.line_start(last_line + 1);
        // A plugin's file type is highlighted from the text itself (SPEC-0210 behaviour 3).
        let plugin_spans = if self.syntax.is_none() {
            self.plugin_spans_in(start_char..end_char)
        } else {
            None
        };
        // The tree matches the buffer only when no edit is queued or being parsed.
        let fresh = self
            .syntax
            .as_ref()
            .filter(|_| self.syntax_edits.is_empty());
        let (spans, fresh) = match (fresh, plugin_spans) {
            (Some(d), _) => (
                self.comment_spans(
                    d.highlight_range(start_char..end_char),
                    start_char..end_char,
                ),
                true,
            ),
            (None, Some(spans)) => (spans, true),
            (None, None) => (Vec::new(), false),
        };
        if self.colors.len() > 50_000 {
            self.colors.clear();
        }
        let primary = self.state.carets.primary();
        let caret_line = self.line_of(primary.head);

        let first_top = top;
        let folds = &self.folds;
        let markers = layout::marker_spans(&self.changes, first, last_line + 1 - first)
            .into_iter()
            .map(|mut m| {
                // Lines to rows: a span inside a fold shrinks to the fold's row.
                let line = first + m.row;
                let row = folds.row_of_line(line);
                if m.rows > 0 {
                    m.rows = folds.row_of_line(line + m.rows - 1) - row + 1;
                }
                m.row = row - first_row;
                let key = match m.kind {
                    LineChangeKind::Added => "ADDED_LINES_COLOR",
                    LineChangeKind::Modified => "MODIFIED_LINES_COLOR",
                    LineChangeKind::Deleted => "DELETED_LINES_COLOR",
                };
                let y = first_top + px(m.row as f32 * line_height());
                let x = bounds.origin.x + gutter - px(FOLD_WIDTH + MARKER_WIDTH + 2.0);
                let bounds = if m.rows == 0 {
                    // A deletion: a short mark across the boundary between two lines.
                    Bounds::new(
                        point(x, y - px(MARKER_WIDTH)),
                        size(px(MARKER_WIDTH), px(2.0 * MARKER_WIDTH)),
                    )
                } else {
                    Bounds::new(
                        point(x, y),
                        size(px(MARKER_WIDTH), px(m.rows as f32 * line_height())),
                    )
                };
                (bounds, p.scheme_color(key).unwrap_or(p.text))
            })
            .collect();
        // The right margin of Code Style (SPEC-0150 behaviour 3); the measured width also maps
        // mouse positions to columns.
        crate::fonts::set_editor_advance(f32::from(digit_width));
        let right_margin = self
            .right_margin_column()
            .map(|column| text_x + digit_width * column as f32);
        let mut frame = Frame {
            bounds,
            gutter_x: bounds.origin.x + gutter,
            field: self.field,
            markers,
            right_margin,
            breakpoints_muted: self.breakpoints_muted,
            block_carets: self.overwrite(),
            ..Frame::default()
        };
        let mut painted = Vec::with_capacity(count);
        let mut one_line_folds = Vec::new();
        let mut placeholders = Vec::new();
        let arrows_shown = !self.fold_arrows_on_hover || self.pointer_inside;
        let mut caret_widths = Vec::new();
        let mut line_end_texts = Vec::new();
        let mut author_hints = Vec::new();
        let mut gutter_tips = Vec::new();
        for i in 0..count {
            // A usage count's own row above its declaration, indented like it (SPEC-0292
            // behaviour 4); a click there lands at the start of the declaration's line.
            if let Some((line, label)) = self.folds.above_row(first_row + i) {
                let origin = point(text_x, top + px(i as f32 * line_height()));
                let source = self.state.buffer.line(line).unwrap_or_default();
                let lead = source
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .count();
                let indent = DisplayLine::with_tab_width(&source, self.tab_width).byte_of(lead);
                let colour = p
                    .scheme_foreground("INLAY_TEXT_WITHOUT_BACKGROUND")
                    .unwrap_or(p.line_number);
                let shaped_label = text_system.shape_line(
                    label.to_owned().into(),
                    font_size,
                    &[run(label.len(), colour)],
                    None,
                );
                frame.inline_values.push((
                    point(origin.x + digit_width * indent as f32, origin.y),
                    shaped_label,
                ));
                painted.push(PaintedLine {
                    line,
                    display: DisplayLine::new(""),
                    shaped: text_system.shape_line("".into(), font_size, &[], None),
                    origin,
                    above: true,
                });
                continue;
            }
            let line = self.folds.line_of_row(first_row + i);
            let text = self.state.buffer.line(line).unwrap_or_default();
            // A folded line whose placeholder stands for the rest of it shows its first chars
            // only: `import ...` (SPEC-0083 v8 behaviour 21).
            let text = match self.folds.kept_chars(line).filter(|_| !self.field) {
                Some(keep) => text.chars().take(keep).collect(),
                None => text,
            };
            let text = if self.masked {
                "•".repeat(text.chars().count())
            } else {
                text
            };
            let display = DisplayLine::with_tab_width(&text, self.tab_width);
            let line_start = self.line_start(line);
            let line_end = line_start + text.chars().count();
            // Colour runs over display bytes: from the tree when it is current, else the colours
            // last seen for a line with the same text.
            let coloured: Vec<(Range<usize>, idecraft_ui::theme::TextStyle)> = if fresh {
                let c = syntax_runs(&spans, line_start..line_end, &display, p);
                self.colors.insert(text.clone(), c.clone());
                c
            } else {
                self.colors.get(&text).cloned().unwrap_or_default()
            };
            // Parameter name hints take room in the line (SPEC-0291 behaviour 4): the display line
            // gets their labels, and the colour runs move with the columns.
            let here: Vec<(usize, &crate::inlay_model::Hint)> = if self.masked {
                Vec::new()
            } else {
                self.hints
                    .within(line_start..line_end)
                    .iter()
                    .map(|h| (h.offset - line_start, h))
                    .collect()
            };
            // Resolved configuration values stand in place of their placeholders, except where a
            // caret touches one (SPEC-0115 v5 behaviour 13).
            let values: Vec<(Range<usize>, &str)> = if self.masked || self.value_folds.is_empty() {
                Vec::new()
            } else {
                self.value_folds
                    .shown_within(line_start..line_end, &value_carets)
                    .into_iter()
                    .map(|f| {
                        (
                            f.range.start - line_start..f.range.end - line_start,
                            f.text.as_str(),
                        )
                    })
                    .collect()
            };
            let (display, coloured, hint_labels) = if here.is_empty() && values.is_empty() {
                (display, coloured, Vec::new())
            } else {
                let (folded, drawn) = crate::value_fold_model::with_folds(&display, &values);
                let (shown, labels) = crate::inlay_model::with_hints(&folded, &here);
                let value_style = p.key_style("FOLDED_TEXT_ATTRIBUTES");
                let value_labels: Vec<_> = drawn
                    .into_iter()
                    .map(|(column, len)| {
                        let at = shown.byte_of(column);
                        (at..at + len, value_style.clone())
                    })
                    .collect();
                let moved = coloured
                    .into_iter()
                    .map(|(r, c)| {
                        (
                            shown.byte_of(display.column_of(r.start))
                                ..shown.byte_of(display.column_of(r.end)),
                            c,
                        )
                    })
                    // A run inside a placeholder that shows its value has no text left.
                    .filter(|(r, _): &(Range<usize>, _)| r.start < r.end)
                    .collect();
                let hint_style = p.key_style("INLINE_PARAMETER_HINT");
                let labels = labels
                    .into_iter()
                    .map(|r| (r, hint_style.clone()))
                    .chain(value_labels)
                    .collect();
                (shown, moved, labels)
            };
            // Semantic tokens over the syntax colours (SPEC-0290 behaviour 4).
            let semantic: Vec<_> = self
                .semantic
                .within(line_start..line_end.max(line_start + 1))
                .iter()
                .map(|(r, key)| {
                    let from = display.byte_of(r.start.max(line_start) - line_start);
                    let to = display.byte_of(r.end.min(line_end) - line_start);
                    (from..to, p.key_style(key))
                })
                .collect();
            let coloured = crate::semantic_model::overlay(&coloured, &semantic);
            // Reassigned variables keep their underline over the server's tokens, which do not
            // say it (`SPEC-0027` v11).
            let reassigned: Vec<_> = if fresh && !semantic.is_empty() {
                syntax_runs(&spans, line_start..line_end, &display, p)
                    .into_iter()
                    .filter(|(_, c)| c.underline)
                    .collect()
            } else {
                Vec::new()
            };
            let coloured = crate::semantic_model::overlay(&coloured, &reassigned);
            let coloured = crate::semantic_model::overlay(&coloured, &hint_labels);
            // Unused code in grey instead of a wavy underline (SPEC-0070 v5 behaviour 6).
            let unused_style = p.key_style("NOT_USED_ELEMENT_ATTRIBUTES");
            let unused: Vec<_> = self
                .problems
                .iter()
                .filter(|q| {
                    q.unused
                        && q.range.start < line_end.max(line_start + 1)
                        && q.range.end > line_start
                })
                .map(|q| {
                    let from = display.byte_of(q.range.start.max(line_start) - line_start);
                    let to = display.byte_of(q.range.end.min(line_end) - line_start);
                    (from..to, unused_style.clone())
                })
                .collect();
            let coloured = crate::semantic_model::overlay(&coloured, &unused);
            let mut runs = Vec::new();
            let mut at = 0usize;
            for (r, c) in coloured {
                if r.start < at || r.end <= r.start || r.end > display.text.len() {
                    continue;
                }
                if r.start > at {
                    runs.push(run(r.start - at, p.text));
                }
                runs.push(styled(r.end - r.start, &c));
                at = r.end;
            }
            if at < display.text.len() || runs.is_empty() {
                runs.push(run(display.text.len() - at, p.text));
            }
            if let Some(m) = &self.marked
                && m.start < line_end.max(line_start + 1)
                && m.end > line_start
            {
                frame.composition.push((
                    i,
                    display.byte_of(m.start.max(line_start) - line_start)
                        ..display.byte_of(m.end.min(line_end) - line_start),
                ));
            }
            // The inline completion is drawn into the caret line at the caret, moving the rest of
            // the line to the right (SPEC-0157 behaviour 4, SPEC-0162 behaviour 1); clicks are
            // mapped on the line without it.
            let ghost_here = self
                .ghost
                .as_ref()
                .filter(|(at, rev, _)| {
                    *rev == self.revision
                        && line == caret_line
                        && *at == primary.head
                        && (line_start..=line_end).contains(at)
                })
                .map(|(at, _, t)| (*at, t.clone()));
            let plain = ghost_here.as_ref().map(|_| {
                text_system.shape_line(display.text.clone().into(), font_size, &runs, None)
            });
            let shaped = match &ghost_here {
                Some((at, text)) => {
                    let mut lines = text.lines();
                    let first = lines.next().unwrap_or_default().to_owned();
                    let more = lines.count();
                    let shown = if more > 0 {
                        format!("{first}  (+{more} lines, Tab)")
                    } else {
                        first
                    };
                    let colour = p
                        .scheme_foreground("INLINE_COMPLETION_TEXT")
                        .or_else(|| p.scheme_foreground("LINE_COMMENT"))
                        .unwrap_or(p.line_number);
                    let byte = display.byte_of(at - line_start);
                    let mut with = display.text.clone();
                    with.insert_str(byte, &shown);
                    let runs = insert_run(&runs, byte, run(shown.len(), colour));
                    text_system.shape_line(with.into(), font_size, &runs, None)
                }
                None => text_system.shape_line(display.text.clone().into(), font_size, &runs, None),
            };
            let origin = point(text_x, top + px(i as f32 * line_height()));
            // Where the line's text ends, a placeholder included.
            let mut line_end_x = origin.x + shaped.width;
            let number_color = if line == caret_line {
                p.line_number_current
            } else {
                p.line_number
            };
            let number = format!("{}", line + 1);
            let number_shaped = text_system.shape_line(
                number.clone().into(),
                font_size,
                &[run(number.len(), number_color)],
                None,
            );
            let number_x = bounds.origin.x + gutter
                - px(GUTTER_PADDING + FOLD_WIDTH)
                - run_w
                - marks_w
                - number_shaped.width;
            // The line's annotation: age colour and label (SPEC-0277 behaviours 2 and 3).
            if let Some(a) = self.annotations.as_ref().filter(|_| !self.field) {
                use crate::blame_model::Cell;
                let cell = Bounds::new(
                    point(bounds.origin.x, origin.y),
                    size(annotation_w, px(line_height())),
                );
                let dim = p.scheme_color("ANNOTATIONS_COLOR").unwrap_or(p.line_number);
                let (label, colour) = match a.cell(line) {
                    Cell::Empty => (String::new(), dim),
                    Cell::Uncommitted => {
                        (crate::blame_model::NOT_COMMITTED.to_owned(), p.line_number)
                    }
                    Cell::Commit {
                        label, age, newest, ..
                    } => {
                        if let Some(bg) = p.scheme_color(crate::blame_model::AGE_KEYS[age as usize])
                        {
                            frame.annotation_cells.push((cell, bg));
                        }
                        let colour = if newest {
                            p.scheme_color("ANNOTATIONS_LAST_COMMIT_COLOR")
                                .filter(|&c| c != 0 || !p.dark)
                                .unwrap_or(dim)
                        } else {
                            dim
                        };
                        (label.to_owned(), colour)
                    }
                };
                if !label.is_empty() {
                    let shaped = text_system.shape_line(
                        label.clone().into(),
                        font_size,
                        &[run(label.len(), colour)],
                        None,
                    );
                    frame.annotation_texts.push((
                        point(bounds.origin.x + px(ANNOTATION_PADDING), origin.y),
                        shaped,
                    ));
                }
            }
            // Breakpoint and execution lines get their scheme background; a breakpoint shows as a
            // red dot over the line number (SPEC-0091 behaviours 1 and 4).
            let has_breakpoint = self.breakpoints.binary_search(&line).is_ok();
            // A line breakpoint's look, else a method breakpoint's (SPEC-0146 v6 behaviour 12).
            let look = match self.method_breakpoints.get(&line) {
                Some(on) if !has_breakpoint => Some(if *on {
                    crate::debug_model::BreakpointLook::Method
                } else {
                    crate::debug_model::BreakpointLook::MethodDisabled
                }),
                _ => has_breakpoint.then(|| self.breakpoint_look(line)),
            };
            let row = Bounds::new(
                point(bounds.origin.x + gutter, origin.y),
                size(bounds.size.width - gutter, px(line_height())),
            );
            if self.execution_line == Some(line) {
                frame.line_backgrounds.push((
                    row,
                    p.scheme_background("EXECUTIONPOINT_ATTRIBUTES")
                        .unwrap_or(0x2d4e8a),
                ));
            } else if look.is_some_and(|l| l.tints_line()) {
                frame.line_backgrounds.push((
                    row,
                    p.scheme_background("BREAKPOINT_ATTRIBUTES")
                        .unwrap_or(0x3a2323),
                ));
            }
            if !self.field
                && let Some(look) = look
            {
                let d = px(12.0);
                let x = bounds.origin.x + gutter
                    - px(GUTTER_PADDING + FOLD_WIDTH)
                    - run_w
                    - marks_w
                    - px(6.0);
                frame.breakpoints.push((
                    Bounds::new(
                        point(x - d / 2.0, origin.y + (px(line_height()) - d) / 2.0),
                        size(d, d),
                    ),
                    look,
                    has_breakpoint && self.breakpoint_unbound(line),
                ));
                if let Some((b, _, _)) = frame.breakpoints.last() {
                    gutter_tips.push((*b, GutterIcon::Breakpoint(line)));
                }
            } else if let Some(mark) = self.bookmark_at(line).filter(|_| !self.field) {
                // A bookmark shows in place of the line number: a square in the accent colour
                // with its mnemonic (SPEC-0331 behaviour 14).
                let d = px(14.0);
                let x = bounds.origin.x + gutter
                    - px(GUTTER_PADDING + FOLD_WIDTH)
                    - run_w
                    - marks_w
                    - px(7.0);
                let square = Bounds::new(
                    point(x - d / 2.0, origin.y + (px(line_height()) - d) / 2.0),
                    size(d, d),
                );
                let letter = mark.mnemonic.map(|m| {
                    let text = m.to_string();
                    text_system.shape_line(
                        text.clone().into(),
                        font_size * 0.8,
                        &[run(text.len(), p.accent_text.rgb)],
                        None,
                    )
                });
                frame.bookmarks.push((square, letter));
                gutter_tips.push((square, GutterIcon::Bookmark(line)));
            } else if !self.field {
                frame
                    .numbers
                    .push((point(number_x, origin.y), number_shaped));
            }
            let mut note_x = origin.x + shaped.width + px(16.0);
            for (_, note) in self.line_notes.iter().filter(|(_, n)| n.line == line) {
                use crate::sql_editor_model::NoteKind;
                let colour = match note.kind {
                    NoteKind::Info => p.line_number,
                    NoteKind::Warning => p.problem_mark("WARNING_ATTRIBUTES"),
                    NoteKind::Error => p.problem_mark("ERRORS_ATTRIBUTES"),
                };
                let text = note.text.clone();
                let shaped_note = text_system.shape_line(
                    text.clone().into(),
                    font_size,
                    &[run(text.len(), colour)],
                    None,
                );
                let width = shaped_note.width;
                frame
                    .inline_values
                    .push((point(note_x, origin.y), shaped_note));
                note_x += width + px(16.0);
            }
            if let Some(m) = self.declaration_marks.at(line).filter(|_| !self.field) {
                let mark = text_system.shape_line(
                    m.glyph.into(),
                    font_size * 0.9,
                    &[run(m.glyph.len(), p.line_number)],
                    None,
                );
                let lane = self.declaration_width();
                let left = bounds.origin.x + gutter - px(FOLD_WIDTH) - run_w - lane;
                frame
                    .fold_marks
                    .push((point(left + (lane - mark.width) / 2.0, origin.y), mark));
                gutter_tips.push((
                    Bounds::new(point(left, origin.y), size(lane, px(line_height()))),
                    GutterIcon::Declaration(line),
                ));
            }
            if let Some(m) = self.framework_marks.at(line).filter(|_| !self.field) {
                // The green of the platform's run icon: a bean is something that runs.
                let mark = text_system.shape_line(
                    m.glyph.into(),
                    font_size * 0.8,
                    &[run(m.glyph.len(), 0x5fb865)],
                    None,
                );
                let lane = marks_w - self.declaration_width();
                let left = bounds.origin.x + gutter - px(FOLD_WIDTH) - run_w - marks_w;
                frame.fold_marks.push((
                    point(left + (lane - mark.width) / 2.0, origin.y + px(1.0)),
                    mark,
                ));
                gutter_tips.push((
                    Bounds::new(point(left, origin.y), size(lane, px(line_height()))),
                    GutterIcon::Framework(line),
                ));
            }
            if !self.field && self.run_markers.binary_search(&line).is_ok() {
                // The green of the platform's run icon.
                let mark = text_system.shape_line(
                    "▶".into(),
                    font_size * 0.8,
                    &[run("▶".len(), 0x5fb865)],
                    None,
                );
                let x =
                    bounds.origin.x + gutter - px(FOLD_WIDTH) - run_w + (run_w - mark.width) / 2.0;
                frame.fold_marks.push((point(x, origin.y + px(1.0)), mark));
                gutter_tips.push((
                    Bounds::new(
                        point(bounds.origin.x + gutter - px(FOLD_WIDTH) - run_w, origin.y),
                        size(run_w, px(line_height())),
                    ),
                    GutterIcon::Run(line),
                ));
            }
            if !self.field {
                // Folding marks for regions starting here, and the placeholder of a folded one
                // (SPEC-0083 behaviour 2).
                // The arrow of an open region is drawn all the time or only while the pointer is
                // over the editor (v8 behaviour 22).
                let folded = self.folds.folded_at(line).is_some();
                if folded || (arrows_shown && regions.iter().any(|&(s, _)| s == line)) {
                    let glyph = if folded { "▸" } else { "▾" };
                    let mark = text_system.shape_line(
                        glyph.into(),
                        font_size,
                        &[run(glyph.len(), p.line_number)],
                        None,
                    );
                    let x = bounds.origin.x + gutter - px(FOLD_WIDTH)
                        + (px(FOLD_WIDTH) - mark.width) / 2.0;
                    frame.fold_marks.push((point(x, origin.y), mark));
                    gutter_tips.push((
                        Bounds::new(
                            point(bounds.origin.x + gutter - px(FOLD_WIDTH), origin.y),
                            size(px(FOLD_WIDTH), px(line_height())),
                        ),
                        GutterIcon::Fold(line, folded),
                    ));
                }
                // One-line methods show their statement in its own colours right after the brace,
                // only the braces on the folded-text background (SPEC-0083 v3 behaviour 7).
                let one_line = self
                    .folds
                    .shown_placeholder(line)
                    .filter(|_| folded)
                    .and_then(|own| {
                        let hidden = self.state.buffer.line(line + 1)?;
                        let colours = self.char_colours(line + 1, &hidden, p);
                        Some((
                            own.to_owned(),
                            crate::fold_model::placeholder_look(own, &hidden, &colours)?,
                        ))
                    });
                if let Some((label, look)) = one_line {
                    let brace_style = p.key_style("FOLDED_TEXT_ATTRIBUTES");
                    let mut runs = Vec::new();
                    let mut at = 0usize;
                    let pieces = look
                        .coloured
                        .iter()
                        .cloned()
                        .chain(std::iter::once((look.brace.clone(), brace_style)));
                    for (r, style) in pieces {
                        if r.start < at {
                            continue;
                        }
                        if r.start > at {
                            runs.push(run(r.start - at, p.text));
                        }
                        let mut s = styled(r.end - r.start, &style);
                        s.background_color = None;
                        runs.push(s);
                        at = r.end;
                    }
                    if at < label.len() {
                        runs.push(run(label.len() - at, p.text));
                    }
                    let mut colours: Vec<gpui::Hsla> = Vec::new();
                    for r in &runs {
                        if !colours.contains(&r.color) {
                            colours.push(r.color);
                        }
                    }
                    placeholders.push((line, display.text.clone(), label.clone()));
                    let shaped_label = text_system.shape_line(label.into(), font_size, &runs, None);
                    let braces_before = frame.fold_braces.len();
                    let x = origin.x + shaped.width;
                    let brace_box = |from: Pixels, to: Pixels| {
                        Bounds::new(
                            point(from, origin.y + px(2.0)),
                            size(to - from, px(line_height() - 4.0)),
                        )
                    };
                    frame.fold_braces.push(brace_box(
                        x + shaped_label.x_for_index(look.brace.start),
                        x + shaped_label.x_for_index(look.brace.end),
                    ));
                    // The visible opening brace, the line's last character.
                    let source = self.state.buffer.line(line).unwrap_or_default();
                    let trimmed = source.trim_end();
                    if trimmed.ends_with('{') {
                        let col = trimmed.chars().count() - 1;
                        frame.fold_braces.push(brace_box(
                            origin.x + shaped.x_for_index(display.byte_of(col)),
                            origin.x + shaped.x_for_index(display.byte_of(col + 1)),
                        ));
                    }
                    line_end_x = x + shaped_label.width;
                    if let Some(color) = self.hidden_problem_colour(line, p) {
                        frame.underlines.push((
                            point(x, origin.y + px(line_height() - 3.0)),
                            shaped_label.width,
                            color,
                        ));
                    }
                    one_line_folds.push((
                        line,
                        colours.len(),
                        frame.fold_braces.len() - braces_before,
                    ));
                    frame.inline_values.push((point(x, origin.y), shaped_label));
                } else if folded {
                    // Other folds show the placeholder of their region (SPEC-0083 behaviour 2, v8
                    // behaviour 21): right behind an opening bracket, which gets the folded
                    // background too, so the block reads `{...}`; right behind the part of the
                    // line that stays; else a little apart.
                    let own = self.folds.placeholder(line).is_some();
                    let cut = self.folds.kept_chars(line).is_some();
                    let label = self
                        .folds
                        .shown_placeholder(line)
                        .unwrap_or(idecraft_languages::folding::ELLIPSIS)
                        .to_owned();
                    let source = self.state.buffer.line(line).unwrap_or_default();
                    let trimmed = source.trim_end();
                    let bracket = !cut && !own && trimmed.ends_with(['{', '[', '(']);
                    if bracket {
                        let col = trimmed.chars().count() - 1;
                        let from = origin.x + shaped.x_for_index(display.byte_of(col));
                        let to = origin.x + shaped.x_for_index(display.byte_of(col + 1));
                        frame.fold_braces.push(Bounds::new(
                            point(from, origin.y + px(2.0)),
                            size(to - from, px(line_height() - 4.0)),
                        ));
                    }
                    let gap = if own || cut || bracket { 0.0 } else { 4.0 };
                    placeholders.push((line, display.text.clone(), label.clone()));
                    let shaped_label = text_system.shape_line(
                        label.clone().into(),
                        font_size,
                        &[run(
                            label.len(),
                            p.scheme_color("FOLDED_TEXT_ATTRIBUTES")
                                .unwrap_or(p.line_number),
                        )],
                        None,
                    );
                    let x = origin.x + shaped.width + px(gap);
                    let width = shaped_label.width;
                    line_end_x = x + width;
                    if let Some(color) = self.hidden_problem_colour(line, p) {
                        frame.underlines.push((
                            point(x, origin.y + px(line_height() - 3.0)),
                            width,
                            color,
                        ));
                    }
                    frame.placeholders.push((
                        Bounds::new(
                            point(x, origin.y + px(2.0)),
                            size(width, px(line_height() - 4.0)),
                        ),
                        shaped_label,
                    ));
                }
            }
            // A debugger value after the text and any placeholder, before the count (SPEC-0091
            // v15 behaviour 41): one after the other, never over each other.
            if let Some((_, value)) = self.inline_values.iter().find(|(l, _)| *l == line) {
                let colour = p
                    .scheme_foreground("DEBUGGER_INLINED_VALUES")
                    .or_else(|| p.scheme_color("DEBUGGER_INLINED_VALUES"))
                    .unwrap_or(p.line_number);
                let text = value.clone();
                let shaped_value = text_system.shape_line(
                    text.clone().into(),
                    font_size,
                    &[run(text.len(), colour)],
                    None,
                );
                let (x, end) = place_at_line_end(line_end_x, px(16.0), shaped_value.width);
                line_end_x = end;
                frame.inline_values.push((point(x, origin.y), shaped_value));
            }
            // The usage count at the end of the line, after a placeholder (SPEC-0292 v2
            // behaviour 4).
            if let Some(label) = self.folds.line_end_text(line).filter(|_| !self.masked) {
                let colour = p
                    .scheme_foreground("INLAY_TEXT_WITHOUT_BACKGROUND")
                    .unwrap_or(p.line_number);
                let shaped_label = text_system.shape_line(
                    label.to_owned().into(),
                    font_size,
                    &[run(label.len(), colour)],
                    None,
                );
                let (x, end) = place_at_line_end(line_end_x, digit_width * 2.0, shaped_label.width);
                line_end_x = end;
                frame.inline_values.push((point(x, origin.y), shaped_label));
                line_end_texts.push((line, label.to_owned()));
            }
            // The declaration's author after the count, with a person symbol (SPEC-0292 v4
            // behaviour 7c); `*` follows the change markers.
            if let Some(label) = (!self.masked && !self.field)
                .then(|| self.authors.label_at(line, &self.changes))
                .flatten()
            {
                let colour = p
                    .scheme_foreground("INLAY_TEXT_WITHOUT_BACKGROUND")
                    .unwrap_or(p.line_number);
                let (x, _) = place_at_line_end(line_end_x, digit_width * 2.0, px(0.0));
                let lh = px(line_height());
                let unit = (lh * 0.18).max(px(2.0));
                let head = Bounds::new(
                    point(x + unit * 0.75, origin.y + lh * 0.18),
                    size(unit * 1.5, unit * 1.5),
                );
                let body = Bounds::new(
                    point(x, origin.y + lh * 0.18 + unit * 1.75),
                    size(unit * 3.0, unit * 1.6),
                );
                frame.author_icons.push((head, body, colour));
                let text_x = x + unit * 3.0 + digit_width * 0.5;
                let shaped_label = text_system.shape_line(
                    label.clone().into(),
                    font_size,
                    &[run(label.len(), colour)],
                    None,
                );
                let width = text_x + shaped_label.width - x;
                frame
                    .inline_values
                    .push((point(text_x, origin.y), shaped_label));
                author_hints.push((
                    Bounds::new(point(x, origin.y), size(width, lh)),
                    line,
                    label,
                ));
            }

            // Problems on this line as wavy underlines (behaviour 2).
            let first_problem = self.problems.partition_point(|q| q.range.end <= line_start);
            for q in self.problems[first_problem..]
                .iter()
                .take_while(|q| q.range.start <= line_end)
                .filter(|q| !q.unused)
            {
                let from = q.range.start.max(line_start) - line_start;
                let to = q.range.end.min(line_end.max(line_start + 1)) - line_start;
                let x0 = shaped.x_for_index(display.byte_of(from.min(line_end - line_start)));
                let mut x1 = shaped.x_for_index(display.byte_of(to.min(line_end - line_start)));
                if x1 <= x0 {
                    x1 = x0 + digit_width;
                }
                let key = match q.severity {
                    Severity::Error => "ERRORS_ATTRIBUTES",
                    Severity::Warning => "WARNING_ATTRIBUTES",
                    Severity::Weak => "WEAK_WARNING_ATTRIBUTES",
                };
                let color = p.problem_mark(key);
                frame.underlines.push((
                    point(origin.x + x0, origin.y + px(line_height() - 3.0)),
                    x1 - x0,
                    color,
                ));
            }

            // Usages of the symbol at the caret on this line, under the find results
            // (SPEC-0295 behaviour 5).
            let marks = self.occurrences.marks();
            let first_mark = marks.partition_point(|m| m.range.end <= line_start);
            for m in marks[first_mark..]
                .iter()
                .take_while(|m| m.range.start <= line_end)
            {
                let from =
                    shaped.x_for_index(display.byte_of(m.range.start.max(line_start) - line_start));
                let to =
                    shaped.x_for_index(display.byte_of(m.range.end.min(line_end) - line_start));
                frame.occurrences.push((
                    Bounds::new(
                        point(origin.x + from, origin.y),
                        size(to - from, px(line_height())),
                    ),
                    m.kind,
                ));
            }

            // Find results on this line, under the selection.
            let first_hit = self.highlights.partition_point(|h| h.end <= line_start);
            for h in self.highlights[first_hit..]
                .iter()
                .take_while(|h| h.start <= line_end)
            {
                let from =
                    shaped.x_for_index(display.byte_of(h.start.max(line_start) - line_start));
                let to = shaped.x_for_index(display.byte_of(h.end.min(line_end) - line_start));
                frame.highlights.push(Bounds::new(
                    point(origin.x + from, origin.y),
                    size(to - from, px(line_height())),
                ));
            }

            // Selections and carets on this line: every caret, not only the primary one
            // (SPEC-0057).
            let marks = line_marks(self.state.carets.carets(), line_start, line_end);
            for s in &marks.selections {
                let from = shaped.x_for_index(display.byte_of(s.from));
                let mut to = shaped.x_for_index(display.byte_of(s.to));
                if s.line_break {
                    to += digit_width; // the selected line break
                }
                frame.selections.push(Bounds::new(
                    point(origin.x + from, origin.y),
                    size(to - from, px(line_height())),
                ));
            }
            for column in &marks.carets {
                let x = shaped.x_for_index(display.byte_of(*column));
                // In overwrite mode the caret covers the character it stands on (SPEC-0057 v6
                // behaviour 25).
                let next = shaped.x_for_index(display.byte_of(*column + 1));
                let width = layout::caret_width(
                    frame.block_carets,
                    f32::from(next - x),
                    f32::from(digit_width),
                );
                caret_widths.push(width);
                frame.carets.push(Bounds::new(
                    point(origin.x + x, origin.y),
                    size(px(width), px(line_height())),
                ));
            }
            if line == caret_line && primary.is_empty() && !self.field {
                frame.caret_row = Some(origin.y);
            }
            frame.lines.push((origin, shaped.clone()));
            painted.push(PaintedLine {
                line,
                display,
                shaped: plain.unwrap_or(shaped),
                origin,
                above: false,
            });
        }
        if gutter_tips != self.last.gutter_tips {
            self.gutter_moved = true;
        }
        self.last = LastLayout {
            lines: painted,
            gutter_x: bounds.origin.x + gutter,
            origin: bounds.origin,
            bounds,
            annotation_x: bounds.origin.x + annotation_w,
            one_line_folds,
            placeholders,
            line_end_texts,
            gutter_tips,
            author_hints,
            caret_widths,
        };
        frame
    }
}

/// What one frame paints.
#[derive(Default)]
struct Frame {
    bounds: Bounds<Pixels>,
    caret_row: Option<Pixels>,
    /// x of the line between gutter and text.
    gutter_x: Pixels,
    /// x of the right margin line, when it is shown.
    right_margin: Option<Pixels>,
    /// No gutter line in a one-line field.
    field: bool,
    /// Change markers and their colours (`SPEC-0064` behaviour 2).
    markers: Vec<(Bounds<Pixels>, u32)>,
    /// Age colours and labels of the annotation column (`SPEC-0277`).
    annotation_cells: Vec<(Bounds<Pixels>, u32)>,
    annotation_texts: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    numbers: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    lines: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    /// Usages of the symbol at the caret with their kinds (`SPEC-0295` behaviour 4).
    occurrences: Vec<(
        Bounds<Pixels>,
        idecraft_languages::occurrences::OccurrenceKind,
    )>,
    /// Find results (`SPEC-0065` behaviour 5).
    highlights: Vec<Bounds<Pixels>>,
    /// Whole-line backgrounds: execution point and breakpoint lines (`SPEC-0091`).
    line_backgrounds: Vec<(Bounds<Pixels>, u32)>,
    /// Breakpoint dots in the gutter, hollow when the session did not bind them.
    breakpoints: Vec<(Bounds<Pixels>, crate::debug_model::BreakpointLook, bool)>,
    /// Bookmark squares in the gutter with their mnemonic (`SPEC-0331` behaviour 14).
    bookmarks: Vec<(Bounds<Pixels>, Option<ShapedLine>)>,
    /// The breakpoints are muted: grey.
    breakpoints_muted: bool,
    /// Wavy underlines of problems: start, width, colour (`SPEC-0070`).
    underlines: Vec<(gpui::Point<Pixels>, Pixels, u32)>,
    selections: Vec<Bounds<Pixels>>,
    carets: Vec<Bounds<Pixels>>,
    /// The carets are blocks: overwrite mode (`SPEC-0057` v6 behaviour 25).
    block_carets: bool,
    /// Painted line index and display byte range being composed.
    composition: Vec<(usize, Range<usize>)>,
    /// Folding marks in the gutter (`SPEC-0083`).
    fold_marks: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    /// Placeholders after folded lines: box and text.
    placeholders: Vec<(Bounds<Pixels>, ShapedLine)>,
    /// Backgrounds of the braces of folded one-line methods (`SPEC-0083` v3 behaviour 7).
    fold_braces: Vec<Bounds<Pixels>>,
    /// Debugger values after lines (`SPEC-0091` v5 behaviour 13).
    inline_values: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    /// Person symbols before author hints: head and body, colour (`SPEC-0292` v4 behaviour 7c).
    author_icons: Vec<(Bounds<Pixels>, Bounds<Pixels>, u32)>,
}

fn paint(frame: Frame, focused: bool, window: &mut Window, cx: &mut App) {
    let b = frame.bounds;
    let p = Palette::current();
    window.paint_quad(fill(b, color(p.background)));
    // Line between gutter and text (OBS-0007, item 7).
    let sep = p.separator;
    if !frame.field {
        window.paint_quad(fill(
            Bounds::new(
                point(frame.gutter_x, b.origin.y),
                size(px(1.0), b.size.height),
            ),
            gpui::rgba((sep.rgb << 8) | u32::from(sep.alpha)),
        ));
    }
    if let Some(y) = frame.caret_row {
        window.paint_quad(fill(
            Bounds::new(point(b.origin.x, y), size(b.size.width, px(line_height()))),
            color(p.caret_row),
        ));
    }
    for (bounds, rgb) in &frame.line_backgrounds {
        window.paint_quad(fill(*bounds, color(*rgb)));
    }
    if let Some(x) = frame.right_margin
        && x < b.origin.x + b.size.width
    {
        window.paint_quad(fill(
            Bounds::new(point(x, b.origin.y), size(px(1.0), b.size.height)),
            gpui::rgba((sep.rgb << 8) | u32::from(sep.alpha)),
        ));
    }
    // Usages of the symbol at the caret, in the scheme's read and write backgrounds; a scheme
    // without them draws none (SPEC-0295 behaviour 4).
    for (bounds, kind) in &frame.occurrences {
        if let Some(rgb) = p.scheme_background(crate::occurrence_model::scheme_key(*kind)) {
            window.paint_quad(fill(*bounds, color(rgb)));
        }
    }
    let hit = p
        .scheme_background("TEXT_SEARCH_RESULT_ATTRIBUTES")
        .unwrap_or(p.selection);
    for h in &frame.highlights {
        window.paint_quad(fill(*h, color(hit)));
    }
    for s in &frame.selections {
        window.paint_quad(fill(*s, color(p.selection)));
    }
    for (bounds, rgb) in &frame.markers {
        window.paint_quad(fill(*bounds, color(*rgb)));
    }
    for (bounds, rgb) in &frame.annotation_cells {
        window.paint_quad(fill(*bounds, color(*rgb)));
    }
    for (origin, shaped) in &frame.annotation_texts {
        let _ = shaped.paint(*origin, px(line_height()), window, cx);
    }
    for (origin, shaped) in &frame.numbers {
        let _ = shaped.paint(*origin, px(line_height()), window, cx);
    }
    for (dot, look, unbound) in &frame.breakpoints {
        use crate::debug_model::BreakpointLook;
        // The red of the platform's breakpoint icon (dark variant); amber for one that only logs;
        // grey while muted (SPEC-0194 v3 behaviour 8).
        let round = dot.size.width / 2.0;
        let (red, amber) = if frame.breakpoints_muted {
            (0x8c8c8c, 0x8c8c8c)
        } else {
            (0xdb5c5c, 0xe0a23b)
        };
        // Not bound by the running session: a ring two pixels wide in the dot's colour; a
        // switched-off breakpoint stays the thin ring (SPEC-0127 v5 behaviour 14).
        if *unbound && *look != BreakpointLook::Disabled {
            let rgb = if *look == BreakpointLook::Log {
                amber
            } else {
                red
            };
            window.paint_quad(
                gpui::outline(*dot, color(rgb), gpui::BorderStyle::Solid)
                    .border_widths(px(2.0))
                    .corner_radii(round),
            );
            continue;
        }
        match look {
            // A method breakpoint: a diamond, a switched-off one its outline (SPEC-0146 v7,
            // OBS-0056 P1; until v6 a square).
            BreakpointLook::Method | BreakpointLook::MethodDisabled => {
                let corners = method_diamond(*dot);
                let mut path = if *look == BreakpointLook::Method {
                    gpui::PathBuilder::fill()
                } else {
                    gpui::PathBuilder::stroke(px(1.0))
                };
                path.move_to(corners[0]);
                for c in &corners[1..] {
                    path.line_to(*c);
                }
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, color(red));
                }
            }
            BreakpointLook::Disabled => window.paint_quad(
                gpui::outline(*dot, color(red), gpui::BorderStyle::Solid).corner_radii(round),
            ),
            BreakpointLook::Log => window.paint_quad(fill(*dot, color(amber)).corner_radii(round)),
            BreakpointLook::Plain | BreakpointLook::Conditional => {
                window.paint_quad(fill(*dot, color(red)).corner_radii(round))
            }
        }
        if *look == BreakpointLook::Conditional {
            let inner = Bounds::new(
                point(
                    dot.origin.x + dot.size.width * 0.3,
                    dot.origin.y + dot.size.height * 0.3,
                ),
                size(dot.size.width * 0.4, dot.size.height * 0.4),
            );
            window.paint_quad(fill(inner, color(0xf2f2f2)).corner_radii(inner.size.width / 2.0));
        }
    }
    for (square, letter) in &frame.bookmarks {
        window.paint_quad(fill(*square, color(p.accent.rgb)).corner_radii(px(3.0)));
        if let Some(letter) = letter {
            let origin = point(
                square.origin.x + (square.size.width - letter.width) / 2.0,
                square.origin.y + (square.size.height - px(line_height())) / 2.0,
            );
            let _ = letter.paint(origin, px(line_height()), window, cx);
        }
    }
    for (origin, shaped) in &frame.fold_marks {
        let _ = shaped.paint(*origin, px(line_height()), window, cx);
    }
    let folded_bg = p
        .scheme_background("FOLDED_TEXT_ATTRIBUTES")
        .unwrap_or(p.selection);
    for bounds in &frame.fold_braces {
        window.paint_quad(fill(*bounds, color(folded_bg)).corner_radii(px(3.0)));
    }
    for (head, body, rgb) in &frame.author_icons {
        window.paint_quad(fill(*head, color(*rgb)).corner_radii(head.size.width / 2.0));
        window.paint_quad(fill(*body, color(*rgb)).corner_radii(gpui::Corners {
            top_left: body.size.height * 0.8,
            top_right: body.size.height * 0.8,
            bottom_left: px(1.0),
            bottom_right: px(1.0),
        }));
    }
    for (origin, shaped) in &frame.inline_values {
        let _ = shaped.paint(*origin, px(line_height()), window, cx);
    }
    for (bounds, shaped) in &frame.placeholders {
        window.paint_quad(fill(*bounds, color(folded_bg)).corner_radii(px(3.0)));
        let _ = shaped.paint(
            point(bounds.origin.x, bounds.origin.y - px(2.0)),
            px(line_height()),
            window,
            cx,
        );
    }
    for (origin, shaped) in &frame.lines {
        let _ = shaped.paint(*origin, px(line_height()), window, cx);
    }
    for (origin, width, rgb) in &frame.underlines {
        window.paint_underline(
            *origin,
            *width,
            &gpui::UnderlineStyle {
                thickness: px(1.0),
                color: Some(color(*rgb)),
                wavy: true,
            },
        );
    }
    for (i, range) in &frame.composition {
        if let Some((origin, shaped)) = frame.lines.get(*i) {
            let from = shaped.x_for_index(range.start);
            let to = shaped.x_for_index(range.end);
            window.paint_quad(fill(
                Bounds::new(
                    point(origin.x + from, origin.y + px(line_height() - 2.0)),
                    size(to - from, px(1.0)),
                ),
                color(p.text),
            ));
        }
    }
    if focused {
        // A block caret lets the character under it show through.
        let caret = if frame.block_carets {
            color(p.caret).opacity(0.5)
        } else {
            color(p.caret)
        };
        for c in &frame.carets {
            window.paint_quad(fill(*c, caret));
        }
    }
}

/// Whether typed text is one control character other than a line break or a tab: what the
/// platform's text system hands over for a chord with ⌃ it has no command for (⌃Q as U+0011).
fn is_control_input(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let b = &self.state.buffer;
        let range = b.utf16_to_char(range_utf16.start)..b.utf16_to_char(range_utf16.end);
        actual_range.replace(b.char_to_utf16(range.start)..b.char_to_utf16(range.end));
        b.slice(range).ok()
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let c = self.state.carets.primary();
        let b = &self.state.buffer;
        let r = c.range();
        Some(UTF16Selection {
            range: b.char_to_utf16(r.start)..b.char_to_utf16(r.end),
            reversed: c.head < c.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let b = &self.state.buffer;
        self.marked
            .as_ref()
            .map(|m| b.char_to_utf16(m.start)..b.char_to_utf16(m.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            self.marked = None;
            self.note = Some(self.read_only_note());
            cx.notify();
            return;
        }
        // A chord with ⌃ that no action took may come back from the platform's text system as a
        // control character to type; it is not text (`SPEC-0028` v6 behaviour 11e).
        if range_utf16.is_none() && self.marked.is_none() && is_control_input(text) {
            return;
        }
        let b = &self.state.buffer;
        let range = range_utf16
            .map(|r| b.utf16_to_char(r.start)..b.utf16_to_char(r.end))
            .or_else(|| self.marked.clone());
        match range {
            // Committed composition or a replacement the input method asked for.
            Some(r) => {
                let end = r.start + text.chars().count();
                if self.state.buffer.replace(r, text).is_ok() {
                    self.state.carets = Carets::new(end);
                }
            }
            // Plain typing goes through the editor commands (pairs, all carets).
            None => idecraft_editor::type_text(&mut self.state, text),
        }
        self.marked = None;
        self.after_change(true, cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let b = &self.state.buffer;
        let range = range_utf16
            .map(|r| b.utf16_to_char(r.start)..b.utf16_to_char(r.end))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.state.carets.primary().range());
        let len = text.chars().count();
        if self.state.buffer.replace(range.clone(), text).is_ok() {
            self.state.carets = Carets::new(range.start + len);
            self.marked = (len > 0).then(|| range.start..range.start + len);
        }
        self.after_change(true, cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let at = self.state.buffer.utf16_to_char(range_utf16.start);
        let line = self.line_of(at);
        let column = at - self.line_start(line);
        let Some(p) = self.last.lines.iter().find(|p| p.line == line && !p.above) else {
            return Some(element_bounds);
        };
        let x = p.shaped.x_for_index(p.display.byte_of(column));
        Some(Bounds::new(
            point(p.origin.x + x, p.origin.y),
            size(px(2.0), px(line_height())),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let at = self.offset_at(point);
        Some(self.state.buffer.char_to_utf16(at))
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Without the focus the clipboard may get another text: the next Cut up to Line End
        // starts afresh (SPEC-0057 v6 behaviour 24).
        if !self.focus.is_focused(window) || !window.is_window_active() {
            self.state.end_cut_row();
        }
        let context = self.context_strip();
        self.fold_arrows_on_hover = !crate::appearance::fold_arrows_always(cx);
        let entity = cx.entity();
        let input_entity = entity.clone();
        let focus = self.focus.clone();
        let card = self.annotation_card();
        let tips = self.gutter_tip_overlays();
        div()
            .id("editor-view")
            .size_full()
            .relative()
            .track_focus(&self.focus)
            // The hover card goes when the pointer leaves the view, also when a menu now lies
            // between them (SPEC-0061 v9 behaviour 48).
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                // The folding arrows of open regions follow the pointer (SPEC-0083 v8
                // behaviour 22).
                this.set_pointer_inside(*hovered, cx);
                if !*hovered {
                    this.hide_annotation_hover(cx);
                    // Documentation on hover goes with the pointer too (SPEC-0077 v6 behaviour 5).
                    this.leave_doc_hover(cx);
                }
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.dragging = false;
                    this.column_drag = None;
                }),
            )
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        entity.update(cx, |this, cx| {
                            let frame = this.prepare(bounds, window);
                            // The gutter icons' tooltips follow them in the next frame.
                            if this.gutter_moved {
                                cx.notify();
                            }
                            frame
                        })
                    },
                    move |bounds, frame, window, cx| {
                        let focused = focus.is_focused(window);
                        paint(frame, focused, window, cx);
                        input_entity.update(cx, |this, cx| this.painted(cx));
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, input_entity.clone()),
                            cx,
                        );
                    },
                )
                .size_full(),
            )
            .children(card)
            .children(tips)
            .children(context)
    }
}

/// A selected stretch of one painted line, in chars from the line start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineSelection {
    /// First selected column.
    pub from: usize,
    /// Column after the last selected one on this line.
    pub to: usize,
    /// The selection goes on past the end of the line (its line break is selected).
    pub line_break: bool,
}

/// What the editor paints of `carets` on the line `line_start..line_end` (chars, without the line
/// break): the selected stretches and the caret columns. Every caret counts, so the occurrences
/// that ⌃G adds show as selections with a caret each (SPEC-0057; hand test 9 of round 4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LineMarks {
    /// Selected stretches.
    pub selections: Vec<LineSelection>,
    /// Caret columns.
    pub carets: Vec<usize>,
}

pub(crate) fn line_marks(carets: &[Caret], line_start: usize, line_end: usize) -> LineMarks {
    let mut marks = LineMarks::default();
    for c in carets {
        let r = c.range();
        if !c.is_empty() && r.start <= line_end && r.end > line_start {
            marks.selections.push(LineSelection {
                from: r.start.saturating_sub(line_start),
                to: r.end.min(line_end) - line_start,
                line_break: r.end > line_end,
            });
        }
        if c.head >= line_start && c.head <= line_end {
            marks.carets.push(c.head - line_start);
        }
    }
    marks
}

/// What [`write_buffer`] did.
#[derive(Debug, PartialEq)]
pub(crate) enum SaveOutcome {
    /// Written; the buffer and its registry entry are unmodified now.
    Saved,
    /// Refused once: the file changed on disk since it was read or saved; the next save overwrites.
    ChangedOnDisk,
    /// The write failed, with the reason.
    Failed(String),
}

/// The disk half of Save (behaviour 9), without a window: refuses once when the file changed on
/// disk since `disk` (unless `overwrite_armed`), else writes the buffer, marks it saved and
/// publishes the saved state to the registry of open documents (`SPEC-0220`), so quitting
/// (`SPEC-0261` behaviour 15) no longer counts the file. `record` runs before and after the write
/// (local history).
pub(crate) fn write_buffer(
    path: &std::path::Path,
    buffer: &mut idecraft_editor::TextBuffer,
    disk: &mut Option<String>,
    overwrite_armed: &mut bool,
    document: Option<&idecraft_editor::documents::OpenDocument>,
    encoding: idecraft_core::file_properties::FileEncoding,
    record: &mut dyn FnMut(),
) -> SaveOutcome {
    use idecraft_core::file_properties as fp;
    let on_disk = fp::read_text(path, None).ok().map(|f| f.text);
    if let (Some(known), Some(now)) = (disk.as_ref(), &on_disk)
        && known != now
        && !*overwrite_armed
    {
        *overwrite_armed = true;
        return SaveOutcome::ChangedOnDisk;
    }
    let text = buffer.text();
    // Nothing is written that the file's encoding cannot hold (`SPEC-0336` behaviour 14).
    let bytes = match fp::encode(&text, encoding) {
        Ok(bytes) => bytes,
        Err(u) => {
            return SaveOutcome::Failed(format!(
                "{}; convert the file with File Encoding or remove them",
                u.message(encoding.charset)
            ));
        }
    };
    record();
    match std::fs::write(path, &bytes) {
        Ok(()) => {
            // The file keeps this encoding however its bytes read now.
            fp::choose(path, encoding.charset);
            buffer.mark_saved();
            if let Some(doc) = document {
                doc.publish(buffer.snapshot(), buffer.is_modified());
            }
            *disk = Some(text);
            *overwrite_armed = false;
            record();
            SaveOutcome::Saved
        }
        Err(e) => SaveOutcome::Failed(e.to_string()),
    }
}

/// Whether a secondary click at char offset `at` keeps the selections (`SPEC-0275` v20 behaviour
/// 14b): it lies on a selected range, its first and its last character included.
fn menu_keeps_selection(carets: &[idecraft_editor::Caret], at: usize) -> bool {
    carets.iter().any(|c| {
        let r = c.range();
        !c.is_empty() && r.start <= at && at <= r.end
    })
}

/// The corners of a method breakpoint's diamond in the mark's bounds `dot` (`SPEC-0146` v7): the
/// middles of its top, right, bottom and left edges.
pub fn method_diamond(dot: Bounds<Pixels>) -> [gpui::Point<Pixels>; 4] {
    let (x, y) = (dot.origin.x, dot.origin.y);
    let (w, h) = (dot.size.width, dot.size.height);
    [
        point(x + w / 2.0, y),
        point(x + w, y + h / 2.0),
        point(x + w / 2.0, y + h),
        point(x, y + h / 2.0),
    ]
}

/// The right margin's column for a file with `config` under Code Style `style`
/// (`SPEC-0137` v2 behaviour 2a).
fn margin_column(
    config: &idecraft_core::editorconfig::EditorConfig,
    style: &idecraft_core::CodeStyleSettings,
) -> Option<u32> {
    match config.right_margin() {
        Some(column) => column.map(|c| c as u32),
        None => style.show_right_margin.then_some(style.right_margin),
    }
}

#[cfg(test)]
mod tests {
    use super::menu_keeps_selection;
    use idecraft_editor::Caret;

    /// SPEC-0137 v2 AK-5: `max_line_length` wins over Code Style, `off` draws none, without it
    /// Code Style decides.
    #[test]
    fn the_right_margin_follows_max_line_length() {
        use idecraft_core::editorconfig::{EditorConfig, Setting};
        let config = |value: Option<&str>| {
            let mut c = EditorConfig::default();
            if let Some(value) = value {
                c.properties.insert(
                    "max_line_length".into(),
                    Setting {
                        value: value.into(),
                        file: ".editorconfig".into(),
                        line: 2,
                    },
                );
            }
            c
        };
        let shown = idecraft_core::CodeStyleSettings {
            right_margin: 120,
            show_right_margin: true,
            ..Default::default()
        };
        let hidden = idecraft_core::CodeStyleSettings {
            show_right_margin: false,
            ..shown.clone()
        };
        assert_eq!(super::margin_column(&config(Some("20")), &shown), Some(20));
        assert_eq!(super::margin_column(&config(Some("20")), &hidden), Some(20));
        assert_eq!(super::margin_column(&config(Some("off")), &shown), None);
        assert_eq!(super::margin_column(&config(None), &shown), Some(120));
        assert_eq!(super::margin_column(&config(None), &hidden), None);
        assert_eq!(
            super::margin_column(&config(Some("wide")), &shown),
            Some(120)
        );
    }

    /// SPEC-0275 v20 AK-44 (OBS-0051 L8).
    /// SPEC-0146 v7 AK-v7-1: the diamond's corners are the middles of the mark's edges.
    #[test]
    fn method_breakpoint_diamond() {
        use gpui::{Bounds, point, px, size};
        let dot = Bounds::new(point(px(10.), px(20.)), size(px(12.), px(12.)));
        assert_eq!(
            super::method_diamond(dot),
            [
                point(px(16.), px(20.)),
                point(px(22.), px(26.)),
                point(px(16.), px(32.)),
                point(px(10.), px(26.)),
            ]
        );
    }

    #[test]
    fn a_secondary_click_inside_a_selection_keeps_it() {
        let backwards = [Caret::select(10, 4)];
        for at in [4, 5, 9, 10] {
            assert!(menu_keeps_selection(&backwards, at), "{at}");
        }
        for at in [0, 3, 11, 40] {
            assert!(!menu_keeps_selection(&backwards, at), "{at}");
        }
        assert!(
            !menu_keeps_selection(&[Caret::select(7, 7)], 7),
            "no selection"
        );
        let several = [Caret::select(0, 2), Caret::select(20, 25)];
        assert!(menu_keeps_selection(&several, 22));
        assert!(!menu_keeps_selection(&several, 10));
    }
}

#[cfg(test)]
mod caret_tests {
    use super::*;
    use idecraft_editor::{EditorState, perform, type_text};

    const SHAPES: &str = "struct Shape {\n    virtual int area() const = 0;\n};\nstruct Square : Shape {\n    int area() const override { return side * side; }\n};\nint total = add(s.area(), 1);\n";

    /// Marks of every line, as (line, selected text, caret columns).
    fn painted(state: &EditorState) -> Vec<(usize, Vec<String>, Vec<usize>)> {
        let text = state.buffer.text();
        let mut start = 0;
        let mut out = Vec::new();
        for (i, line) in text.split('\n').enumerate() {
            let end = start + line.chars().count();
            let m = line_marks(state.carets.carets(), start, end);
            if !m.selections.is_empty() || !m.carets.is_empty() {
                let chars: Vec<char> = line.chars().collect();
                let selected = m
                    .selections
                    .iter()
                    .map(|s| chars[s.from..s.to].iter().collect())
                    .collect();
                out.push((i, selected, m.carets));
            }
            start = end + 1;
        }
        out
    }

    /// Hand test 9 of round 4: caret on `area`, ⌃G three times, type, Escape, ⌃⌘G — each step
    /// leaves selections and carets that the editor paints.
    #[test]
    fn occurrences_are_painted_with_a_caret_each() {
        let mut state = EditorState::new(SHAPES);
        let at = SHAPES.find("area").unwrap() + 1;
        state.carets = Carets::new(at);
        assert!(perform(&mut state, "SelectNextOccurrence"));
        assert_eq!(painted(&state), [(1, vec!["area".into()], vec![20])]);
        assert!(perform(&mut state, "SelectNextOccurrence"));
        assert!(perform(&mut state, "SelectNextOccurrence"));
        let shown = painted(&state);
        assert_eq!(shown.len(), 3, "three lines with a selection: {shown:?}");
        for (_, selected, carets) in &shown {
            assert_eq!(selected, &["area"]);
            assert_eq!(carets.len(), 1);
        }
        type_text(&mut state, "size");
        assert_eq!(state.buffer.text().matches("size()").count(), 3);
        assert_eq!(state.buffer.text().matches("area").count(), 0);
        assert_eq!(state.carets.carets().len(), 3, "typing keeps every caret");
        assert!(perform(&mut state, "EditorEscape"));
        assert_eq!(state.carets.carets().len(), 1);
        // ⌃⌘G on a word selects every occurrence of it.
        let at = state.buffer.text().find("side").unwrap();
        state.carets = Carets::new(at);
        assert!(perform(&mut state, "SelectAllOccurrences"));
        let shown = painted(&state);
        let selected: usize = shown.iter().map(|(_, s, _)| s.len()).sum();
        assert_eq!(selected, 2, "both `side`: {shown:?}");
        assert!(shown.iter().all(|(_, s, c)| s.len() == c.len()));
    }

    #[test]
    fn a_selection_over_a_line_break_marks_both_lines() {
        let carets = [Caret::select(2, 7)];
        // Line one is 0..4 ("abcd"), line two 5..9.
        let first = line_marks(&carets, 0, 4);
        assert_eq!(
            first.selections,
            [LineSelection {
                from: 2,
                to: 4,
                line_break: true
            }]
        );
        let second = line_marks(&carets, 5, 9);
        assert_eq!(
            second.selections,
            [LineSelection {
                from: 0,
                to: 2,
                line_break: false
            }]
        );
        assert_eq!(second.carets, [2]);
        assert!(first.carets.is_empty());
    }
}

#[cfg(test)]
mod colour_role_tests {
    use super::{DisplayLine, Palette, SyntaxDocument, syntax_runs};
    use idecraft_languages::LanguageId;
    use idecraft_ui::theme::ThemeId;

    /// SPEC-0027 AK-14 (v11) through the editor's own path: in `Islands Dark` the loop counter's
    /// runs carry the underline in the effect colour, the parameter that never changes does not.
    #[test]
    fn reassigned_locals_are_underlined() {
        let text = "class A {\n    void add(String name) {\n        for (int i = 0; i < 2; i++) { use(name + i); }\n    }\n}\n";
        let doc = SyntaxDocument::new(LanguageId::Java, false, text).unwrap();
        let p = Palette::bundled(ThemeId::IslandsDark);
        let line = text.split('\n').nth(2).unwrap();
        let start: usize = text
            .split('\n')
            .take(2)
            .map(|l| l.chars().count() + 1)
            .sum();
        let spans = doc.highlight_range(0..text.chars().count());
        let display = DisplayLine::with_tab_width(line, 4);
        let runs = syntax_runs(&spans, start..start + line.len(), &display, &p);
        let style_at = |at: usize| {
            runs.iter()
                .find(|(r, _)| r.start <= at && r.end > at)
                .map(|(_, s)| s.clone())
                .unwrap_or_default()
        };
        for (at, _) in line.match_indices('i').filter(|(at, _)| {
            !line[at + 1..].starts_with(|c: char| c.is_alphanumeric())
                && !line[..*at].ends_with(|c: char| c.is_alphanumeric())
        }) {
            let s = style_at(at);
            assert!(s.underline, "`i` at {at}: {s:?}");
            assert_eq!(s.effect, Some(0x84868c));
        }
        let name = line.find("name").unwrap();
        assert!(!style_at(name).underline);
    }

    /// SPEC-0027 v7 AK-8 through the editor's own path: the visible lines are highlighted as a
    /// range and each line takes its styles from the palette of `Dark`, as `paint` does; SQL
    /// built-ins come out in the function-declaration colour, italic.
    #[test]
    fn sql_built_ins_are_italic_in_the_function_declaration_colour() {
        let text = "-- a comment\nCREATE TABLE person (\n    born    DATE DEFAULT CURRENT_DATE\n);\n\nSELECT p.name, COUNT(*) AS n\nFROM person p\nORDER BY n DESC;\n";
        let doc = SyntaxDocument::new(LanguageId::Sql, false, text).unwrap();
        let p = Palette::bundled(ThemeId::Dark);
        let declaration = p.key_style("DEFAULT_FUNCTION_DECLARATION").foreground;
        assert_eq!(declaration, Some(0x56a8f5));
        assert!(!p.key_style("DEFAULT_FUNCTION_DECLARATION").italic);
        // Every line as the first visible one, so ranges starting mid-text are covered too.
        let lines: Vec<&str> = text.split('\n').collect();
        for first in 0..lines.len() {
            let start: usize = lines[..first].iter().map(|l| l.chars().count() + 1).sum();
            let spans = doc.highlight_range(start..text.chars().count());
            let mut line_start = start;
            for line in &lines[first..] {
                let len = line.chars().count();
                let display = DisplayLine::with_tab_width(line, 4);
                let runs = syntax_runs(&spans, line_start..line_start + len, &display, &p);
                for token in ["COUNT", "CURRENT_DATE"] {
                    if let Some(at) = line.find(token) {
                        let run = runs
                            .iter()
                            .find(|(r, _)| r.start <= at && r.end >= at + token.len())
                            .unwrap_or_else(|| panic!("{token}: no run in {runs:?}"));
                        assert_eq!(run.1.foreground, declaration, "{token} from line {first}");
                        assert!(run.1.italic, "{token} from line {first}");
                    }
                }
                line_start += len + 1;
            }
        }
    }
}

#[cfg(test)]
mod ghost_tests {
    use super::insert_run;

    fn r(len: usize) -> gpui::TextRun {
        gpui::TextRun {
            len,
            font: gpui::font("Menlo"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    /// SPEC-0091/AK-20 (v15): a value and a count at the end of a line lie one after the other.
    #[test]
    fn items_at_the_line_end_do_not_overlap() {
        use super::place_at_line_end;
        use gpui::px;
        let (value_x, end) = place_at_line_end(px(100.0), px(16.0), px(60.0));
        assert_eq!(value_x, px(116.0));
        let (count_x, end) = place_at_line_end(end, px(16.0), px(50.0));
        assert_eq!(count_x, px(192.0));
        assert!(count_x >= value_x + px(60.0));
        assert_eq!(end, px(242.0));
    }

    #[test]
    fn the_suggestion_run_splits_the_line_at_the_caret() {
        let lens = |v: Vec<gpui::TextRun>| v.iter().map(|r| r.len).collect::<Vec<_>>();
        assert_eq!(lens(insert_run(&[r(4), r(3)], 5, r(9))), [4, 1, 9, 2]);
        assert_eq!(lens(insert_run(&[r(4), r(3)], 4, r(9))), [4, 9, 3]);
        assert_eq!(lens(insert_run(&[r(4), r(3)], 7, r(9))), [4, 3, 9]);
        assert_eq!(lens(insert_run(&[r(4)], 0, r(9))), [9, 4]);
    }
}

#[cfg(test)]
mod save_tests {
    use super::{SaveOutcome, write_buffer};
    use idecraft_editor::TextBuffer;
    use idecraft_editor::documents::{DocumentRegistry, OpenDocument};

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("idecraft-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "saved\n").unwrap();
        path
    }

    /// An editor's buffer on `path` with "unsaved" typed in front, registered as open and modified.
    fn dirty(path: &std::path::Path, registry: &DocumentRegistry) -> (TextBuffer, OpenDocument) {
        let mut buffer = TextBuffer::from_text("saved\n");
        buffer.track_changes();
        let doc = OpenDocument::open(registry, path, buffer.snapshot());
        buffer.insert(0, "unsaved").unwrap();
        doc.publish(buffer.snapshot(), buffer.is_modified());
        assert!(registry.get(path).unwrap().modified);
        (buffer, doc)
    }

    /// SPEC-0261 v3 behaviour 15 (the save step of quitting saves every editor the way Save does):
    /// the dirty buffer is written and its registry entry is no longer modified.
    #[test]
    fn saving_a_dirty_buffer_writes_it_and_clears_the_registry_entry() {
        let path = fixture("dirty");
        let registry = DocumentRegistry::default();
        let (mut buffer, doc) = dirty(&path, &registry);
        let mut disk = Some("saved\n".to_owned());
        let (mut armed, mut records) = (false, 0);
        let outcome = write_buffer(
            &path,
            &mut buffer,
            &mut disk,
            &mut armed,
            Some(&doc),
            Default::default(),
            &mut || records += 1,
        );
        assert_eq!(outcome, SaveOutcome::Saved);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "unsavedsaved\n");
        assert!(!buffer.is_modified());
        let entry = registry.get(&path).unwrap();
        assert!(!entry.modified);
        assert_eq!(entry.text.to_string(), "unsavedsaved\n");
        assert_eq!(disk.as_deref(), Some("unsavedsaved\n"));
        assert_eq!(records, 2, "local history before and after the write");
    }

    /// SPEC-0261 v3 behaviour 3: a file changed on disk meanwhile is not written by the first save
    /// and stays modified in the registry, so quitting does not go on; the next save overwrites it.
    #[test]
    fn a_file_changed_on_disk_stays_modified_until_saved_again() {
        let path = fixture("changed");
        let registry = DocumentRegistry::default();
        let (mut buffer, doc) = dirty(&path, &registry);
        std::fs::write(&path, "elsewhere\n").unwrap();
        let mut disk = Some("saved\n".to_owned());
        let mut armed = false;
        let mut save = |buffer: &mut TextBuffer| {
            write_buffer(
                &path,
                buffer,
                &mut disk,
                &mut armed,
                Some(&doc),
                Default::default(),
                &mut || {},
            )
        };
        assert_eq!(save(&mut buffer), SaveOutcome::ChangedOnDisk);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "elsewhere\n");
        assert!(registry.get(&path).unwrap().modified);
        assert_eq!(save(&mut buffer), SaveOutcome::Saved);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "unsavedsaved\n");
        assert!(!registry.get(&path).unwrap().modified);
    }
}

#[cfg(test)]
mod file_property_tests {
    use super::{SaveOutcome, write_buffer};
    use idecraft_core::file_properties::{Charset, FileEncoding, read_text};
    use idecraft_editor::TextBuffer;

    fn fixture(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "idecraft-editor-file-properties-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Saves `text` typed over the file's text, as the editor does.
    fn save(path: &std::path::Path, text: &str, encoding: FileEncoding) -> SaveOutcome {
        let read = read_text(path, None).unwrap();
        let mut buffer = TextBuffer::from_text(&read.text);
        let len = buffer.len_chars();
        buffer.replace(0..len, text).unwrap();
        let mut disk = Some(read.text);
        write_buffer(
            path,
            &mut buffer,
            &mut disk,
            &mut false,
            None,
            encoding,
            &mut || {},
        )
    }

    /// SPEC-0336 AK-8: a file that is not UTF-8 is read in windows-1252 and saved in it, byte
    /// for byte but for the edit.
    #[test]
    fn a_windows_1252_file_is_read_and_saved_in_its_encoding() {
        let path = fixture(
            "legacy",
            &[b'c', b'a', b'f', 0xE9, b' ', 0x80, b'\r', b'\n'],
        );
        let read = read_text(&path, None).unwrap();
        assert_eq!(read.text, "café €\r\n");
        assert_eq!(read.encoding, FileEncoding::of(Charset::Windows1252));
        assert!(matches!(
            save(&path, "café €5\r\n", read.encoding),
            SaveOutcome::Saved
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            [b'c', b'a', b'f', 0xE9, b' ', 0x80, b'5', b'\r', b'\n']
        );
        // Only ASCII is left: the file still is windows-1252 for this run.
        assert!(matches!(
            save(&path, "plain\n", read.encoding),
            SaveOutcome::Saved
        ));
        assert_eq!(
            read_text(&path, None).unwrap().encoding,
            FileEncoding::of(Charset::Windows1252)
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// SPEC-0336 AK-8, behaviour 14: saving never replaces a character; it fails and says how
    /// many the encoding cannot hold.
    #[test]
    fn a_text_the_encoding_cannot_hold_is_not_saved() {
        let path = fixture("ascii", b"plain\n");
        let SaveOutcome::Failed(why) =
            save(&path, "naïve 日本\n", FileEncoding::of(Charset::UsAscii))
        else {
            panic!("the save must fail");
        };
        assert_eq!(
            why,
            "3 characters cannot be written in US-ASCII (the first is 'ï', U+00EF); convert the file with File Encoding or remove them"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"plain\n");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// SPEC-0336 AK-8, behaviour 11: the mark is not in the text and is written back.
    #[test]
    fn the_byte_order_mark_survives_a_save() {
        let path = fixture("bom", b"\xEF\xBB\xBFone\n");
        let read = read_text(&path, None).unwrap();
        assert_eq!(read.text, "one\n");
        assert!(read.encoding.bom);
        assert!(matches!(
            save(&path, "two\n", read.encoding),
            SaveOutcome::Saved
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"\xEF\xBB\xBFtwo\n");
        let wide = fixture("wide", &[0xFE, 0xFF, 0, b'a']);
        let read = read_text(&wide, None).unwrap();
        assert_eq!(read.encoding, FileEncoding::of(Charset::Utf16Be));
        assert!(matches!(
            save(&wide, "é", read.encoding),
            SaveOutcome::Saved
        ));
        assert_eq!(std::fs::read(&wide).unwrap(), [0xFE, 0xFF, 0, 0xE9]);
        for p in [path, wide] {
            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
    }
}
