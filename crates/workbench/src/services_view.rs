//! The Services tool window with the results of the SQL console (`SPEC-0093`): an output log, one
//! grid per result set — kept while pinned — and the table data editors opened from the Database
//! tool window (`SPEC-0190`), with copying and exporting (`SPEC-0021` v2).

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    ClipboardItem, Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Subscription, Window,
    div, font, prelude::*, px, rgb,
};

use idecraft_database::{
    Applied, CellState, ConnectionProfile, Dialect, EditTarget, ExportFormat, GridEdits,
    TableQuery, order_text, page_count,
};

use crate::editor_view::EditorView;
use crate::grid_model::{
    ALL_ROWS, GridState, PAGE_SIZE, PAGE_SIZES, numeric_columns, page_label_of, page_range_of,
    page_size_label, pages_of, transpose, visible,
};
use crate::palette::Palette;
use crate::result_model::{
    CopySettings, DEFAULT_TEXT, Selection, SideView, copy_text, default_export_path, format_rows,
    record_lines, summary_lines, value_text,
};
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::sql_console_model::{cell_text, column_widths};
use crate::table_session::{self, TableSession};
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;

/// Width of one character of the grid font, for the column widths.
const CHAR_WIDTH: f32 = 7.8;
/// Height of a grid row.
/// Row height of the tree (`Tree.rowHeight` of the bundled themes, SPEC-0154).
const ROW_HEIGHT: f32 = 24.;
/// Rows the export preview shows (SPEC-0021 v2 behaviour 11).
const PREVIEW_ROWS: usize = 20;
/// Width of the export dialog's column of choices beside the preview (`SPEC-0021` v4 behaviour
/// 11).
const EXPORT_CHOICES_WIDTH: f32 = 250.;
/// Room left and right, above and below the plan's diagram in its area (`SPEC-0110` v14
/// behaviour 21).
const DIAGRAM_MARGIN: (f32, f32) = (12., 4.);

/// What the tool window asks the workbench to do.
pub enum ServicesEvent {
    /// Hide the tool window.
    Hide,
    /// Forward this port of the remote project's host (`SPEC-0318` v5 behaviour 42).
    ForwardPort(u16),
    /// Stop this forward.
    StopForward(String),
    /// Open this forwarded local port in the browser.
    OpenForward(u16),
    /// The HTTP client environment to use from now on (`SPEC-0102` behaviour 3).
    Environment(Option<String>),
    /// Run this workbench action, e.g. Swap or Open Script of the Schema Diff tab (`SPEC-0110` v5
    /// behaviour 13).
    Action(&'static str),
    /// Apply the pending edits of result `index`, then run its query again (`SPEC-0107`
    /// behaviour 5).
    Submit {
        /// The changes, applied in one transaction.
        set: idecraft_database::EditSet,
        /// The query to run again afterwards.
        query: String,
    },
}

/// A row of the Ports tab of a remote project (`SPEC-0318` v5 behaviour 42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortRow {
    /// The port on the host.
    pub port: u16,
    /// `service`, `database`, `debug` or `forwarded`.
    pub kind: String,
    /// The program that listens, if the host named it.
    pub command: String,
    /// Forward id and local port, when it is forwarded.
    pub forward: Option<(String, u16)>,
}

/// The tab index of the Ports tab; results are counted from 1, so this is never one of them.
const PORTS_TAB: usize = usize::MAX - 1;

/// The rows of one result set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultSet {
    /// Column names.
    pub columns: Vec<String>,
    /// Rows; `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// How long the statement took.
    pub time: Duration,
    /// The statement that produced it.
    pub sql: String,
}

/// The outcome of a console run, for the window.
#[derive(Debug, Clone, Default)]
pub struct ConsoleRun {
    /// Output lines, with whether each is an error.
    pub output: Vec<(String, bool)>,
    /// Result sets in statement order.
    pub results: Vec<ResultSet>,
    /// Whether a statement failed.
    pub failed: bool,
    /// How many of the statements ran, the failed one included (`SPEC-0190` v7 behaviour 1).
    pub ran: usize,
    /// An HTTP response as text, shown in a "Response" tab (`SPEC-0096`).
    pub response: Option<String>,
    /// Per result, whether and how its rows can be edited (`SPEC-0107`).
    pub edits: Vec<Option<GridEdits>>,
    /// A query plan, shown in a "Plan" tab (`SPEC-0110`).
    pub plan: Option<idecraft_database::Plan>,
    /// A schema comparison, shown in a "Schema Diff" tab (`SPEC-0110`).
    pub schema_diff: Option<SchemaDiffTab>,
}

/// What the Schema Diff tab shows (`SPEC-0110` behaviour 4).
#[derive(Debug, Clone, Default)]
pub struct SchemaDiffTab {
    /// The differences, one line each.
    pub lines: Vec<crate::db_compare_model::DiffLine>,
    /// The migration script.
    pub script: String,
    /// The statements and which are in the script (`SPEC-0110` v2 behaviour 7).
    pub choice: Option<crate::db_compare_model::MigrationChoice>,
    /// The script file, rewritten when the choice changes.
    pub file: Option<std::path::PathBuf>,
}

/// A table data editor tab (`SPEC-0190` behaviours 2–4 and 9).
struct TableTab {
    /// Identifies the tab across background work, when tabs move.
    id: u64,
    profile: ConnectionProfile,
    query: TableQuery,
    page: usize,
    total: u64,
    session: TableSession,
    loading: bool,
    /// Manual transaction mode.
    manual: bool,
    /// A transaction is open, with this many applied changes.
    tx_open: bool,
    tx_changes: usize,
    /// Savepoints set so far in the open transaction.
    savepoints: usize,
    /// Why the rows cannot be edited, if so.
    read_only: Option<String>,
    /// Isolation of the transactions this tab opens (`SPEC-0190` v3 behaviour 13).
    isolation: idecraft_database::Isolation,
}

/// Which rows an export takes (SPEC-0021 v2 behaviour 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportRows {
    /// The selected cells.
    Selection,
    /// The rows of the result as loaded (filtered and sorted as shown).
    Loaded,
    /// Every row of the table with the current `WHERE`/`ORDER BY` (data editors).
    All,
}

/// The export dialog (SPEC-0021 v2 behaviour 11).
struct ExportDialog {
    rows: ExportRows,
    to_file: bool,
    path: Entity<EditorView>,
    note: Option<(String, bool)>,
    running: bool,
}

/// Per-result state, kept in parallel to `results`.
#[derive(Default)]
struct TabInfo {
    title: String,
    pinned: bool,
    table: Option<TableTab>,
    /// The file and statement a console result came from (`SPEC-0190` v7 behaviour 1).
    origin: Option<crate::sql_console_model::TabOrigin>,
    /// Column widths set by dragging a header edge; `None` fits the content (`SPEC-0099` v2
    /// behaviour 8).
    widths: Vec<Option<f32>>,
    /// Rows shown as columns (`SPEC-0099` v2 behaviour 7).
    transposed: bool,
    /// Table, tree or text view (`SPEC-0099` v3 behaviour 10).
    view: idecraft_database::grid::ResultView,
    /// Rows expanded in the tree view, by position on the page.
    tree_open: std::collections::BTreeSet<usize>,
}

/// How a result tab looked: view, transposition and column widths, carried over when the same
/// query is run again (`SPEC-0099` v4 behaviour 12).
struct ResultViewState {
    view: idecraft_database::grid::ResultView,
    transposed: bool,
    widths: Vec<Option<f32>>,
}

impl ResultViewState {
    fn of(info: &TabInfo) -> Self {
        Self {
            view: info.view,
            transposed: info.transposed,
            widths: info.widths.clone(),
        }
    }

    fn restore(&self, info: &mut TabInfo) {
        info.view = self.view;
        info.transposed = self.transposed;
        info.widths = self.widths.clone();
    }
}

/// A header edge being dragged: column, pointer x and column width when the drag began.
#[derive(Debug, Clone, Copy)]
struct ColumnDrag {
    column: usize,
    start_x: f32,
    start_width: f32,
}

/// The groups of the result toolbar that stay in its one row or go into its ⋮ menu (`SPEC-0190`
/// v5 behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarGroup {
    /// First, previous, the rows shown, next, last.
    Pager,
    /// `WHERE` and `ORDER BY`, or the filter.
    Fields,
    /// Page size menu.
    PageSize,
    /// Transpose switch.
    Transpose,
    /// Table, Tree, Text.
    View,
    /// Add and delete row, Submit, Revert, Preview.
    Edits,
    /// Transaction menu, Commit, Rollback.
    Tx,
    /// Copy format menu.
    CopyAs,
    /// The record side panel.
    Record,
    /// Export.
    Export,
}

impl ToolbarGroup {
    /// When the group gives way to a narrow window: 0 never, the higher the earlier.
    fn gives_way(self) -> u8 {
        match self {
            Self::Pager | Self::Fields => 0,
            Self::Edits => 2,
            Self::Tx => 3,
            Self::Export => 4,
            Self::PageSize => 5,
            Self::CopyAs => 6,
            Self::View => 7,
            Self::Transpose => 8,
            Self::Record => 9,
        }
    }
}

/// Narrowest width a dragged column keeps (`SPEC-0099` v2 behaviour 8).
const MIN_COLUMN: f32 = 24.;

/// The server limit of a data editor page showing all rows (`SPEC-0099` v2 behaviour 5).
const TABLE_ALL: usize = 1_000_000_000;

/// The tool window.
pub struct ServicesView {
    /// Scrolls the Schema Diff tab to the data-loss question when it appears (`SPEC-0110` v8
    /// behaviour 7, hand test 121 retest).
    schema_diff_scroll: gpui::ScrollHandle,
    /// The Ports tab of a remote project: the host and its rows (`SPEC-0318` v5 behaviour 42).
    ports: Option<(String, Vec<PortRow>)>,
    connection: String,
    running: bool,
    output: Vec<(String, bool)>,
    results: Vec<ResultSet>,
    response: Option<String>,
    plan: Option<idecraft_database::Plan>,
    schema_diff: Option<SchemaDiffTab>,
    /// The view the Plan tab shows: Tree, Diagram, Flame or Raw (`SPEC-0110` v13 behaviour 19).
    plan_mode: crate::plan_view_model::PlanMode,
    /// What the bars of the Flame view measure (v13 behaviour 19).
    flame_metric: crate::plan_view_model::FlameMetric,
    /// The selected step of the Plan tab (v5 behaviour 11).
    plan_step: Option<usize>,
    /// Several hints of the Plan tab are all shown rather than the first (v14 behaviour 21).
    plan_hints_open: bool,
    /// How the Diagram view is scaled: as it opens fitted but readable, fitted wholly, or 100 %
    /// (v14 behaviour 21, v15 behaviour 22).
    diagram_zoom: crate::plan_view_model::DiagramZoom,
    /// The Diagram view's horizontal scroll, which starts at the top step (v15 behaviour 22).
    plan_diagram_scroll: gpui::ScrollHandle,
    /// The Diagram view still has to be scrolled to its top step after its next layout.
    plan_diagram_align: std::rc::Rc<std::cell::Cell<bool>>,
    /// Size of the Plan tab's area below its head as last painted (0 before the first paint).
    plan_area: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// The marked statement of the Schema Diff tab (v5 behaviour 14).
    statement_mark: Option<usize>,
    /// Selected tab: 0 is Output, `i + 1` is result `i`, then the response, plan or schema diff.
    tab: usize,
    /// HTTP client environments of the last request and the one used (`SPEC-0102`).
    environments: Vec<String>,
    environment: Option<String>,
    /// The list of the environments under "Env: … ▾" is open (`SPEC-0102` v5 behaviour 3).
    environment_list: bool,
    focus: FocusHandle,
    /// Sorting, filter and page of each result, and the rows each shows (`SPEC-0099`).
    grids: Vec<GridState>,
    visible: Vec<Vec<usize>>,
    /// Title, pin and data-editor state of each result (`SPEC-0190`).
    infos: Vec<TabInfo>,
    next_table_id: u64,
    /// The filter field above the grid; its text belongs to the shown result.
    filter: Entity<EditorView>,
    _filter_watch: Subscription,
    /// The `WHERE` and `ORDER BY` fields of a data editor (behaviour 3).
    where_field: Entity<EditorView>,
    order_field: Entity<EditorView>,
    /// Pending edits of each result whose rows come from one table (`SPEC-0107`).
    edits: Vec<Option<GridEdits>>,
    /// The selected cells of the shown result: row ids are loaded rows (or `rows.len() + i` for
    /// added row `i`).
    selection: Option<Selection>,
    /// The field over the selected cell while it is edited.
    cell: Entity<EditorView>,
    editing: bool,
    /// Why an edit was refused or how a submit went.
    note: Option<String>,
    /// The side panel and its view (behaviour 6).
    side: Option<SideView>,
    /// The DML preview is shown (behaviour 7).
    preview: bool,
    /// Copy format and header (SPEC-0021 v2 behaviour 10).
    copy: CopySettings,
    copy_menu: bool,
    last_copy: Option<String>,
    export: Option<ExportDialog>,
    /// Bottom edges of the export dialog's button row and of the panel, as last painted (D42).
    export_edges: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// The export dialog's format list, choices and preview, which scroll with a scroll bar
    /// (`SPEC-0021` v4 behaviour 11).
    export_scrolls: [gpui::ScrollHandle; 3],
    /// Where exports go by default.
    root: PathBuf,
    /// A header edge being dragged (`SPEC-0099` v2 behaviour 8).
    column_drag: Option<ColumnDrag>,
    /// The page size menu is open (`SPEC-0099` v2 behaviour 5).
    size_menu: bool,
    /// The transaction menu of a data editor is open (`SPEC-0190` v3 behaviour 13).
    tx_menu: bool,
    /// The toolbar's ⋮ menu with the groups that do not fit is open (`SPEC-0190` v5 behaviour 3).
    more_menu: bool,
    /// The toolbar's width as last painted; 0 before the first frame.
    toolbar_width: std::rc::Rc<std::cell::Cell<f32>>,
    /// At most this width for the toolbar (window self-test).
    toolbar_limit: Option<f32>,
    /// The column whose value filter list is open (`SPEC-0099` v3 behaviour 9).
    value_menu: Option<usize>,
    /// The editable value of the side panel (`SPEC-0190` v4 behaviour 14).
    value_field: Entity<EditorView>,
    /// The side panel's value is being edited.
    value_editing: bool,
}

impl EventEmitter<ServicesEvent> for ServicesView {}

impl ServicesView {
    /// An empty window.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let filter = cx.new(EditorView::field);
        let watch = cx.observe(&filter, |this, field, cx| {
            let text = field.read(cx).state.buffer.text();
            if let Some(i) = this.tab.checked_sub(1).filter(|i| *i < this.grids.len())
                && this.grids[i].filter != text
                && this.infos[i].table.is_none()
            {
                this.grids[i].filter = text;
                this.grids[i].page = 0;
                this.refresh(i);
                cx.notify();
            }
        });
        Self {
            schema_diff_scroll: gpui::ScrollHandle::new(),
            ports: None,
            connection: String::new(),
            running: false,
            output: Vec::new(),
            results: Vec::new(),
            response: None,
            plan: None,
            schema_diff: None,
            plan_mode: crate::plan_view_model::PlanMode::Tree,
            flame_metric: crate::plan_view_model::FlameMetric::Total,
            plan_step: None,
            plan_hints_open: false,
            diagram_zoom: crate::plan_view_model::DiagramZoom::Readable,
            plan_diagram_scroll: gpui::ScrollHandle::new(),
            plan_diagram_align: std::rc::Rc::new(std::cell::Cell::new(true)),
            plan_area: std::rc::Rc::new(std::cell::Cell::new((0., 0.))),
            statement_mark: None,
            tab: 0,
            environments: Vec::new(),
            environment: None,
            environment_list: false,
            focus: cx.focus_handle(),
            grids: Vec::new(),
            visible: Vec::new(),
            infos: Vec::new(),
            next_table_id: 1,
            filter,
            _filter_watch: watch,
            where_field: cx.new(EditorView::field),
            order_field: cx.new(EditorView::field),
            edits: Vec::new(),
            selection: None,
            cell: cx.new(EditorView::field),
            editing: false,
            note: None,
            side: None,
            preview: false,
            copy: CopySettings::default(),
            copy_menu: false,
            last_copy: None,
            export: None,
            export_edges: Default::default(),
            export_scrolls: Default::default(),
            root: std::env::current_dir().unwrap_or_default(),
            column_drag: None,
            size_menu: false,
            tx_menu: false,
            more_menu: false,
            toolbar_width: Default::default(),
            toolbar_limit: None,
            value_menu: None,
            value_field: cx.new(EditorView::field),
            value_editing: false,
        }
    }

    /// The values of `column` of the shown result over all loaded rows, with their counts — the
    /// list of the column's value filter (`SPEC-0099` v3 behaviour 9).
    pub fn column_values(&self, column: usize) -> Vec<(Option<String>, usize)> {
        self.shown_index()
            .map(|i| idecraft_database::grid::column_values(&self.results[i].rows, column))
            .unwrap_or_default()
    }

    /// The column whose value list is open.
    pub fn value_menu(&self) -> Option<usize> {
        self.value_menu
    }

    /// Opens the value list of `column` (`None` closes it); data editors filter on the server
    /// and say so (`SPEC-0099` v3 behaviour 9).
    pub fn open_value_menu(&mut self, column: Option<usize>, cx: &mut Context<Self>) {
        let editor = self
            .shown_index()
            .is_some_and(|i| self.infos[i].table.is_some());
        if column.is_some() && editor {
            self.note = Some("A data editor filters on the server: type the WHERE clause".into());
            self.value_menu = None;
        } else {
            self.value_menu = if self.value_menu == column {
                None
            } else {
                column
            };
        }
        cx.notify();
    }

    /// Checks or unchecks `value` in the value filter of `column` (v3 behaviour 9).
    pub fn toggle_value(&mut self, column: usize, value: Option<String>, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let all: Vec<Option<String>> = self
            .column_values(column)
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        self.grids[i].toggle_value(column, value, &all);
        self.refresh(i);
        cx.notify();
    }

    /// Keeps only `value` in the value filter of `column` (v3 behaviour 9).
    pub fn only_value(&mut self, column: usize, value: Option<String>, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            self.grids[i].only_value(column, value);
            self.refresh(i);
            cx.notify();
        }
    }

    /// Removes the value filter of `column`, or of every column (v3 behaviour 9).
    pub fn clear_values(&mut self, column: Option<usize>, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            self.grids[i].clear_values(column);
            self.refresh(i);
            cx.notify();
        }
    }

    /// Whether `column` of the shown result has a value filter (its header shows the mark).
    pub fn filters_column(&self, column: usize) -> bool {
        self.shown_index()
            .is_some_and(|i| self.grids[i].filters(column))
    }

    /// Whether `value` of `column` is checked in the open value list.
    pub fn value_checked(&self, column: usize, value: &Option<String>) -> bool {
        self.shown_index()
            .is_none_or(|i| self.grids[i].checked(column, value))
    }

    /// How the shown result is shown (`SPEC-0099` v3 behaviour 10).
    pub fn result_view(&self) -> idecraft_database::grid::ResultView {
        self.shown_index()
            .map(|i| self.infos[i].view)
            .unwrap_or_default()
    }

    /// Shows the shown result as a table, a tree or text (v3 behaviour 10).
    pub fn set_result_view(
        &mut self,
        view: idecraft_database::grid::ResultView,
        cx: &mut Context<Self>,
    ) {
        if let Some(i) = self.shown_index() {
            self.infos[i].view = view;
            self.infos[i].tree_open.clear();
            self.editing = false;
            cx.notify();
        }
    }

    /// The rows of the shown page, as loaded.
    fn page_values(&self, i: usize) -> Vec<&[Option<String>]> {
        let rows = &self.results[i].rows;
        self.visible[i][self.page_rows(i)]
            .iter()
            .map(|&r| rows[r].as_slice())
            .collect()
    }

    /// The shown page as the text view (v3 behaviour 10).
    pub fn text_view(&self) -> String {
        let Some(i) = self.shown_index() else {
            return String::new();
        };
        let page = self.page_values(i);
        let owned: Vec<Vec<Option<String>>> = page.iter().map(|r| r.to_vec()).collect();
        let numeric =
            idecraft_database::grid::numeric_columns(self.results[i].columns.len(), &owned);
        idecraft_database::grid::text_view(&self.results[i].columns, &page, &numeric)
    }

    /// The shown page as the tree view (v3 behaviour 10).
    pub fn tree_lines(&self) -> Vec<idecraft_database::grid::TreeLine> {
        let Some(i) = self.shown_index() else {
            return Vec::new();
        };
        let first = self.page_rows(i).start;
        idecraft_database::grid::tree_view(
            &self.results[i].columns,
            &self.page_values(i),
            first,
            &self.infos[i].tree_open,
        )
    }

    /// Expands or collapses row `row` of the page in the tree view.
    pub fn toggle_tree_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            let open = &mut self.infos[i].tree_open;
            if !open.remove(&row) {
                open.insert(row);
            }
            cx.notify();
        }
    }

    /// Whether the side panel's value is being edited (`SPEC-0190` v4 behaviour 14).
    pub fn value_editing(&self) -> bool {
        self.value_editing
    }

    /// Starts editing the selected cell in the side panel: the field takes its full text; read-only
    /// results say why (`SPEC-0190` v4 behaviour 14).
    pub fn start_value_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(i), Some((row, col))) = (self.shown_index(), self.cursor()) else {
            return;
        };
        match &self.edits[i] {
            Some(e) if e.editable(col) => {
                let text = self.value_at(i, row, col).flatten().unwrap_or_default();
                self.value_field.update(cx, |f, cx| f.set_text(&text, cx));
                self.value_editing = true;
                self.side = Some(SideView::Value);
                window.focus(&self.value_field.read(cx).focus());
            }
            _ => self.note = Some(self.read_only_note(i)),
        }
        cx.notify();
    }

    /// Takes the side panel's text as the cell's new value (pending until Submit, like an edit in
    /// the cell) or, with `cancel`, drops it.
    pub fn finish_value_edit(&mut self, cancel: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.value_editing {
            return;
        }
        self.value_editing = false;
        if !cancel {
            let text = self.value_field.read(cx).state.buffer.text();
            self.set_value(Some(text), cx);
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// Sets the side panel's text (tests).
    pub fn set_value_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.value_field.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// The folder exports go to by default.
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = root;
    }

    /// Recomputes the rows result `i` shows.
    fn refresh(&mut self, i: usize) {
        if let (Some(set), Some(state)) = (self.results.get(i), self.grids.get(i)) {
            self.visible[i] = if self.infos[i].table.is_some() {
                // A data editor's rows are filtered and sorted by the server (behaviour 3).
                (0..set.rows.len()).collect()
            } else {
                visible(&set.rows, state)
            };
        }
    }

    /// Makes the Ports tab appear, for a remote project on `host`, with `rows`.
    pub fn set_ports(&mut self, host: String, rows: Vec<PortRow>, cx: &mut Context<Self>) {
        self.ports = Some((host, rows));
        cx.notify();
    }

    /// Selects the Ports tab, if this window has one.
    pub fn show_ports(&mut self, cx: &mut Context<Self>) {
        if self.ports.is_some() {
            self.select_tab(PORTS_TAB, cx);
            cx.notify();
        }
    }

    /// The rows of the Ports tab (tests, the window self-test).
    pub fn port_rows(&self) -> &[PortRow] {
        self.ports.as_ref().map_or(&[], |(_, r)| r.as_slice())
    }

    /// Selects tab `i` and puts the result's filter (or `WHERE`/`ORDER BY`) into the fields.
    fn select_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.tab != i {
            self.selection = None;
            self.editing = false;
            self.preview = false;
        }
        self.tab = i;
        let r = i.checked_sub(1).filter(|r| *r < self.results.len());
        let text = r
            .and_then(|r| self.grids.get(r))
            .map(|g| g.filter.clone())
            .unwrap_or_default();
        let set = |field: &Entity<EditorView>, text: &str, cx: &mut Context<Self>| {
            field.update(cx, |f, cx| {
                if f.state.buffer.text() != text {
                    f.set_text(text, cx);
                }
            });
        };
        set(&self.filter, &text, cx);
        if let Some(t) = r.and_then(|r| self.infos[r].table.as_ref()) {
            let (w, o) = (t.query.filter.clone(), t.query.order.clone());
            set(&self.where_field, &w, cx);
            set(&self.order_field, &o, cx);
        }
        cx.notify();
    }

    /// Index of the shown result, if a result tab is selected.
    fn shown_index(&self) -> Option<usize> {
        self.tab.checked_sub(1).filter(|i| *i < self.results.len())
    }

    /// Sorts the shown result by `column` like a click (⇧-click with `add`) on its header
    /// (behaviour 1); a data editor sorts on the server (`SPEC-0190` behaviour 3).
    pub fn sort_by(&mut self, column: usize, add: bool, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            self.grids[i].click_header(column, add);
            if let Some(t) = self.infos[i].table.as_mut() {
                let d = t.profile.dialect;
                t.query.order = order_text(&self.results[i].columns, &self.grids[i].sort, d);
                t.page = 0;
                let order = t.query.order.clone();
                self.order_field.update(cx, |f, cx| f.set_text(&order, cx));
                self.load_table(i, cx);
            } else {
                self.refresh(i);
            }
            cx.notify();
        }
    }

    /// Puts `text` into the filter field of the shown result (behaviour 2).
    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_text(text, cx));
        if let Some(i) = self.shown_index() {
            self.grids[i].filter = text.to_owned();
            self.grids[i].page = 0;
            self.refresh(i);
            cx.notify();
        }
    }

    /// Moves the shown result by `delta` pages, or to the first (`i64::MIN`) or last (`i64::MAX`)
    /// page (behaviour 3); a data editor loads that page from the server (`SPEC-0190` behaviour 4).
    pub fn turn_page(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let size = self.table_size(i);
        if let Some(t) = self.infos[i].table.as_mut() {
            let last = page_count(t.total, size) as i64 - 1;
            let next = match delta {
                i64::MIN => 0,
                i64::MAX => last,
                d => (t.page as i64 + d).clamp(0, last),
            };
            if next != t.page as i64 && !t.loading {
                t.page = next as usize;
                self.load_table(i, cx);
            }
            return;
        }
        let last = pages_of(self.visible[i].len(), self.grids[i].size()) - 1;
        let page = self.grids[i].page as i64;
        let next = match delta {
            i64::MIN => 0,
            i64::MAX => last as i64,
            d => (page + d).clamp(0, last as i64),
        };
        if next != page {
            self.grids[i].page = next as usize;
            cx.notify();
        }
    }

    /// The row positions of the shown page (data editors show their whole page).
    fn page_rows(&self, i: usize) -> std::ops::Range<usize> {
        if self.infos[i].table.is_some() {
            0..self.visible[i].len()
        } else {
            page_range_of(
                self.visible[i].len(),
                self.grids[i].page,
                self.grids[i].size(),
            )
        }
    }

    /// Rows per server page of data editor `i`: its chosen page size, with "All" as a very large
    /// limit (`SPEC-0099` v2 behaviour 5).
    fn table_size(&self, i: usize) -> usize {
        match self.grids.get(i).map_or(PAGE_SIZE, GridState::size) {
            ALL_ROWS => TABLE_ALL,
            n => n,
        }
    }

    /// Rows per page of the shown result (`SPEC-0099` v2 behaviour 5).
    pub fn page_size(&self) -> Option<usize> {
        self.shown_index().map(|i| self.grids[i].size())
    }

    /// Shows `size` rows per page in the shown result, from the first page; a data editor loads
    /// that page again (`SPEC-0099` v2 behaviour 5).
    pub fn set_page_size(&mut self, size: usize, cx: &mut Context<Self>) {
        self.size_menu = false;
        let Some(i) = self.shown_index() else {
            return;
        };
        self.grids[i].set_page_size(size);
        if let Some(t) = self.infos[i].table.as_mut() {
            t.page = 0;
            self.load_table(i, cx);
        }
        cx.notify();
    }

    /// Whether the shown result shows its rows as columns (`SPEC-0099` v2 behaviour 7).
    pub fn transposed(&self) -> bool {
        self.shown_index().is_some_and(|i| self.infos[i].transposed)
    }

    /// Switches the shown result between rows and transposed rows (`SPEC-0099` v2 behaviour 7).
    pub fn toggle_transpose(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            self.infos[i].transposed = !self.infos[i].transposed;
            self.editing = false;
            cx.notify();
        }
    }

    /// The width set for `column` of the shown result by dragging, if any (`SPEC-0099` v2
    /// behaviour 8).
    pub fn column_width(&self, column: usize) -> Option<f32> {
        let i = self.shown_index()?;
        self.infos[i].widths.get(column).copied().flatten()
    }

    /// Sets `column` of the shown result to `width` (at least [`MIN_COLUMN`]); `None` fits it to
    /// its content again (`SPEC-0099` v2 behaviour 8).
    pub fn set_column_width(&mut self, column: usize, width: Option<f32>, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let n = self.results[i].columns.len();
        if column >= n {
            return;
        }
        let widths = &mut self.infos[i].widths;
        widths.resize(n, None);
        widths[column] = width.map(|w| w.max(MIN_COLUMN));
        cx.notify();
    }

    /// The isolation level data editor transactions open with (`SPEC-0190` v3 behaviour 13).
    pub fn isolation(&self) -> Option<idecraft_database::Isolation> {
        let i = self.shown_index()?;
        self.infos[i].table.as_ref().map(|t| t.isolation)
    }

    /// Chooses the isolation level of the shown data editor's next transaction; refused with the
    /// reason in the note when the database has no such level, or while a transaction is open
    /// (`SPEC-0190` v3 behaviour 13).
    pub fn set_isolation(&mut self, level: idecraft_database::Isolation, cx: &mut Context<Self>) {
        self.tx_menu = false;
        let Some(t) = self
            .shown_index()
            .and_then(|i| self.infos[i].table.as_mut())
        else {
            return;
        };
        if t.tx_open {
            self.note = Some("Commit or roll back before changing the isolation level".into());
        } else if let Err(e) = idecraft_database::begin_statements(t.profile.dialect, level) {
            self.note = Some(e);
        } else {
            t.isolation = level;
            self.note = None;
        }
        cx.notify();
    }

    /// The cells of the shown page of the shown result, as text (for tests).
    pub fn shown_rows(&self) -> Vec<Vec<String>> {
        let Some(i) = self.shown_index() else {
            return Vec::new();
        };
        let rows = &self.results[i].rows;
        let order = &self.visible[i];
        order[self.page_rows(i)]
            .iter()
            .map(|&r| rows[r].iter().map(|v| cell_text(v).to_owned()).collect())
            .collect()
    }

    /// The paging label of the shown result (for tests).
    pub fn page_text(&self) -> Option<String> {
        let i = self.shown_index()?;
        Some(self.label(i))
    }

    fn label(&self, i: usize) -> String {
        match &self.infos[i].table {
            Some(t) => table_label(
                t.total,
                t.page,
                self.results[i].rows.len(),
                self.table_size(i),
            ),
            None => page_label_of(
                self.visible[i].len(),
                self.results[i].rows.len(),
                self.grids[i].page,
                self.grids[i].size(),
            ),
        }
    }

    /// Focus handle of the window.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// A run against `connection` has started (behaviour 3).
    pub fn start(&mut self, connection: &str, cx: &mut Context<Self>) {
        self.connection = connection.to_owned();
        self.running = true;
        // An HTTP run names its environments after this; any other run has none.
        self.environments.clear();
        self.environment_list = false;
        cx.notify();
    }

    /// The environments of the request file and the one used (`SPEC-0102` behaviour 3).
    pub fn set_environments(
        &mut self,
        names: Vec<String>,
        used: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.environments = names;
        self.environment = used;
        cx.notify();
    }

    /// Nothing was sent because an environment must be chosen first (`SPEC-0102` v5 behaviour
    /// 2): Output says so in red and is selected, and "Env: none ▾" offers the list.
    pub fn ask_environment(&mut self, names: Vec<String>, message: String, cx: &mut Context<Self>) {
        self.environments = names;
        self.environment = None;
        self.environment_list = false;
        self.output.push((message, true));
        self.tab = usize::MAX;
        self.select_tab(0, cx);
        cx.notify();
    }

    /// A single request was not sent (`SPEC-0096` v7 behaviour 9): Output names it with its reason
    /// in red and is selected.
    pub fn not_sent(&mut self, line: String, cx: &mut Context<Self>) {
        self.output.push((line, true));
        self.tab = usize::MAX;
        self.select_tab(0, cx);
        cx.notify();
    }

    /// The environment used for the last request, if any.
    pub fn environment(&self) -> Option<&str> {
        self.environment.as_deref()
    }

    /// Whether the list of environments is open.
    pub fn environment_list_open(&self) -> bool {
        self.environment_list
    }

    /// Chooses environment `name` for the following requests of the request file (a click on its
    /// entry in the list, `SPEC-0102` v5 behaviour 3); nothing is sent.
    pub fn choose_environment(&mut self, name: &str, cx: &mut Context<Self>) {
        self.environment_list = false;
        if self.environments.iter().any(|n| n == name) {
            self.environment = Some(name.to_owned());
            cx.emit(ServicesEvent::Environment(self.environment.clone()));
        }
        cx.notify();
    }

    /// The list of environments under "Env: … ▾", the one in use marked (`SPEC-0102` v5
    /// behaviour 3).
    fn environment_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let current = self.environment.clone();
        div()
            .min_w(px(160.))
            .p_1()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(6.))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .px_2()
                    .whitespace_nowrap()
                    .text_color(c(p.ui_dim))
                    .child("HTTP environment")
                    .semantics(
                        crate::semantics::Semantics::new(crate::semantics::Role::Label)
                            .name("HTTP environment")
                            .test_id("services-environment-title"),
                    ),
            )
            .children(self.environments.iter().enumerate().map(|(n, name)| {
                let on = current.as_deref() == Some(name.as_str());
                let chosen = name.clone();
                div()
                    .id(("services-environment-entry", n))
                    .px_2()
                    .whitespace_nowrap()
                    .rounded(px(4.))
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(name.clone())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.choose_environment(&chosen, cx)
                    }))
                    .semantics(
                        crate::semantics::Semantics::new(crate::semantics::Role::MenuItem)
                            .name(name.clone())
                            .test_id(format!("services-environment-{name}"))
                            .selected(on)
                            .action(crate::semantics::AxAction::Click)
                            .effects(&[crate::semantics::Effect::None]),
                    )
            }))
    }

    /// Whether a run is going.
    pub fn running(&self) -> bool {
        self.running
    }

    /// Shows the outcome of a run: the last result set, or the output after a failure (behaviours 4
    /// and 6). Output lines of earlier runs stay above; pinned results and data editors stay, the
    /// other results of the last run are replaced (`SPEC-0190` behaviour 1).
    pub fn finish(&mut self, run: ConsoleRun, cx: &mut Context<Self>) {
        self.finish_from(run, None, cx);
    }

    /// [`Self::finish`] for a run of statements of a file (`origin`): a run of some of its
    /// statements replaces only their tabs, in place, and keeps the file's other tabs (`SPEC-0190`
    /// v7 behaviour 1).
    pub fn finish_from(
        &mut self,
        run: ConsoleRun,
        origin: Option<crate::sql_console_model::RunOrigin>,
        cx: &mut Context<Self>,
    ) {
        use crate::sql_console_model::{OldTab, Slot, arrange_tabs};
        self.running = false;
        self.output.extend(run.output);
        let old: Vec<OldTab> = self
            .infos
            .iter()
            .map(|t| OldTab {
                kept: t.pinned || t.table.is_some(),
                origin: t.origin.clone(),
            })
            .collect();
        let added = run.results.len();
        let origins = match &origin {
            Some(o) => o.result_origins(run.results.iter().map(|r| r.sql.as_str())),
            None => vec![None; added],
        };
        let slots = arrange_tabs(&old, &origins, origin.as_ref(), run.ran);
        // SPEC-0099 v4 behaviour 12: a result replaced by the same query keeps its view.
        let replaced: Vec<(String, Vec<String>, GridState, ResultViewState)> = (0..old.len())
            .filter(|i| !slots.contains(&Slot::Old(*i)))
            .map(|i| {
                (
                    self.results[i].sql.clone(),
                    self.results[i].columns.clone(),
                    self.grids[i].clone(),
                    ResultViewState::of(&self.infos[i]),
                )
            })
            .collect();
        let mut results: Vec<Option<ResultSet>> = std::mem::take(&mut self.results)
            .into_iter()
            .map(Some)
            .collect();
        let mut grids: Vec<Option<GridState>> = std::mem::take(&mut self.grids)
            .into_iter()
            .map(Some)
            .collect();
        let mut visible: Vec<Option<Vec<usize>>> = std::mem::take(&mut self.visible)
            .into_iter()
            .map(Some)
            .collect();
        let mut edits_before: Vec<Option<Option<GridEdits>>> = std::mem::take(&mut self.edits)
            .into_iter()
            .map(Some)
            .collect();
        let mut infos: Vec<Option<TabInfo>> = std::mem::take(&mut self.infos)
            .into_iter()
            .map(Some)
            .collect();
        let mut edits = run.edits;
        edits.resize_with(added, || None);
        let mut new: Vec<Option<(ResultSet, Option<GridEdits>)>> =
            run.results.into_iter().zip(edits).map(Some).collect();
        let mut fresh = Vec::new();
        for slot in slots {
            match slot {
                Slot::Old(i) => {
                    let (Some(set), Some(grid), Some(rows), Some(e), Some(info)) = (
                        results[i].take(),
                        grids[i].take(),
                        visible[i].take(),
                        edits_before[i].take(),
                        infos[i].take(),
                    ) else {
                        continue;
                    };
                    self.results.push(set);
                    self.grids.push(grid);
                    self.visible.push(rows);
                    self.edits.push(e);
                    self.infos.push(info);
                }
                Slot::New(j) => {
                    let Some((set, e)) = new[j].take() else {
                        continue;
                    };
                    let before = replaced.iter().find(|(sql, _, _, _)| *sql == set.sql);
                    let grid = before.map_or_else(GridState::default, |(_, columns, grid, _)| {
                        idecraft_database::grid::kept_view(
                            grid,
                            *columns == set.columns,
                            set.rows.len(),
                        )
                    });
                    let mut info = TabInfo {
                        origin: origins[j].clone(),
                        ..TabInfo::default()
                    };
                    if let Some((_, columns, _, view)) = before
                        && *columns == set.columns
                    {
                        view.restore(&mut info);
                    }
                    self.visible.push((0..set.rows.len()).collect());
                    self.results.push(set);
                    self.grids.push(grid);
                    self.edits.push(e);
                    self.infos.push(info);
                    fresh.push(self.results.len() - 1);
                }
            }
        }
        // Unpinned console results are numbered in their order (v7 behaviour 1).
        let mut n = 0;
        for info in self
            .infos
            .iter_mut()
            .filter(|t| !t.pinned && t.table.is_none())
        {
            n += 1;
            info.title = format!("Result {n}");
        }
        for &i in &fresh {
            self.refresh(i);
        }
        self.response = run.response;
        self.plan = run.plan;
        self.schema_diff = run.schema_diff;
        self.plan_mode = crate::plan_view_model::PlanMode::Tree;
        self.flame_metric = crate::plan_view_model::FlameMetric::Total;
        self.plan_step = None;
        self.plan_hints_open = false;
        self.diagram_zoom = crate::plan_view_model::DiagramZoom::Readable;
        self.plan_diagram_align.set(true);
        self.statement_mark = None;
        self.selection = None;
        self.editing = false;
        self.preview = false;
        self.note = None;
        let extra = self.extra_tab().is_some();
        let tab = if run.failed || (added == 0 && !extra) {
            0
        } else if extra {
            self.results.len() + 1
        } else {
            // The last new result, which is the last tab unless it took an earlier tab's place.
            fresh.last().map_or(self.results.len(), |i| i + 1)
        };
        self.tab = usize::MAX;
        self.select_tab(tab, cx);
    }

    /// The statement texts of the result tabs in their order (tests).
    pub fn result_statements(&self) -> Vec<String> {
        self.results.iter().map(|r| r.sql.clone()).collect()
    }

    /// Adds a line to Output (`SPEC-0191` behaviour 6).
    pub fn log(&mut self, line: String, failed: bool, cx: &mut Context<Self>) {
        self.output.push((line, failed));
        cx.notify();
    }

    /// Lines of work done on the connection named `connection` outside a console, such as Copy
    /// Table (`SPEC-0191` v2 behaviour 6): the header names that connection from now on, the lines
    /// are added to Output and Output is shown.
    pub fn report(
        &mut self,
        connection: &str,
        lines: &[String],
        failed: bool,
        cx: &mut Context<Self>,
    ) {
        self.connection = connection.to_owned();
        self.output
            .extend(lines.iter().map(|line| (line.clone(), failed)));
        self.show_output(cx);
        cx.notify();
    }

    /// The connection the header names: the one of the last run or report (tests).
    pub fn connection(&self) -> &str {
        &self.connection
    }

    /// Output lines so far.
    pub fn output_lines(&self) -> Vec<String> {
        self.output.iter().map(|(l, _)| l.clone()).collect()
    }

    /// The Output lines with whether each is an error (shown in red).
    pub fn output_entries(&self) -> &[(String, bool)] {
        &self.output
    }

    /// The selected tab's title.
    pub fn selected_tab(&self) -> String {
        if self.tab == PORTS_TAB {
            "Ports".into()
        } else if self.tab == 0 {
            "Output".into()
        } else if self.tab > self.results.len() {
            self.extra_tab().unwrap_or("Response").into()
        } else {
            self.infos[self.tab - 1].title.clone()
        }
    }

    /// Titles of the result tabs, pinned ones marked with 📌 (tests).
    pub fn tab_titles(&self) -> Vec<String> {
        self.infos
            .iter()
            .map(|t| {
                if t.pinned {
                    format!("📌 {}", t.title)
                } else {
                    t.title.clone()
                }
            })
            .collect()
    }

    /// Title of the tab after the results: the response, the plan or the schema diff of the last run.
    fn extra_tab(&self) -> Option<&'static str> {
        if self.response.is_some() {
            Some("Response")
        } else if self.plan.is_some() {
            Some("Plan")
        } else if self.schema_diff.is_some() {
            Some("Schema Diff")
        } else {
            None
        }
    }

    /// The plan of the last run, if it was an explain (`SPEC-0110`).
    pub fn plan(&self) -> Option<&idecraft_database::Plan> {
        self.plan.as_ref()
    }

    /// Whether the Plan tab shows the raw text (`SPEC-0110` v5 behaviour 10).
    pub fn plan_raw(&self) -> bool {
        self.plan_mode == crate::plan_view_model::PlanMode::Raw
    }

    /// Switches the Plan tab between Tree and Raw.
    pub fn set_plan_raw(&mut self, raw: bool, cx: &mut Context<Self>) {
        use crate::plan_view_model::PlanMode;
        self.set_plan_mode(if raw { PlanMode::Raw } else { PlanMode::Tree }, cx);
    }

    /// The view the Plan tab shows (`SPEC-0110` v13 behaviour 19).
    pub fn plan_mode(&self) -> crate::plan_view_model::PlanMode {
        self.plan_mode
    }

    /// Shows view `mode` in the Plan tab.
    pub fn set_plan_mode(
        &mut self,
        mode: crate::plan_view_model::PlanMode,
        cx: &mut Context<Self>,
    ) {
        self.plan_mode = mode;
        cx.notify();
    }

    /// What the Flame view's bars measure (v13 behaviour 19).
    pub fn flame_metric(&self) -> crate::plan_view_model::FlameMetric {
        self.flame_metric
    }

    /// Measures the Flame view's bars by `metric`; Time only for a measured plan.
    pub fn set_flame_metric(
        &mut self,
        metric: crate::plan_view_model::FlameMetric,
        cx: &mut Context<Self>,
    ) {
        if self.plan.as_ref().is_some_and(|p| metric.offered(p)) {
            self.flame_metric = metric;
            cx.notify();
        }
    }

    /// Whether several hints of the Plan tab are all shown (v14 behaviour 21).
    pub fn plan_hints_open(&self) -> bool {
        self.plan_hints_open
    }

    /// Shows all hints of the Plan tab, or only the first again; nothing with fewer than two.
    pub fn toggle_plan_hints(&mut self, cx: &mut Context<Self>) {
        if self.plan.as_ref().is_some_and(|p| p.hints.len() > 1) {
            self.plan_hints_open = !self.plan_hints_open;
            cx.notify();
        }
    }

    /// Whether the Diagram view is fitted into its area, as it opens or by Fit (v14 behaviour 21).
    pub fn plan_diagram_fit(&self) -> bool {
        self.diagram_zoom != crate::plan_view_model::DiagramZoom::Full
    }

    /// How the Diagram view is scaled (v15 behaviour 22).
    pub fn plan_diagram_zoom_mode(&self) -> crate::plan_view_model::DiagramZoom {
        self.diagram_zoom
    }

    /// Fit (`true`): the whole diagram inside its area, down to 25 %; or 100 % (v15 behaviour 22).
    pub fn set_plan_diagram_fit(&mut self, fit: bool, cx: &mut Context<Self>) {
        self.diagram_zoom = if fit {
            crate::plan_view_model::DiagramZoom::Fit
        } else {
            crate::plan_view_model::DiagramZoom::Full
        };
        cx.notify();
    }

    /// The Diagram view's horizontal scroll offset in pixels (v15 behaviour 22; tests).
    pub fn plan_diagram_scroll_x(&self) -> f32 {
        -f32::from(self.plan_diagram_scroll.offset().x)
    }

    /// Size of the Plan tab's area below its head, as last painted (v14 behaviour 21).
    pub fn plan_area(&self) -> (f32, f32) {
        self.plan_area.get()
    }

    /// The Diagram view's geometry at 100 % and the zoom it is drawn at (v14 behaviour 21).
    pub fn plan_diagram_zoom(&self) -> Option<(crate::plan_view_model::DiagramMetrics, f32)> {
        let plan = self.plan.as_ref()?;
        let boxes = crate::plan_view_model::diagram(plan);
        let metrics = crate::plan_view_model::diagram_metrics(&boxes, CHAR_WIDTH, ROW_HEIGHT);
        let (w, h) = self.plan_area.get();
        let area = (w - DIAGRAM_MARGIN.0 * 2., h - DIAGRAM_MARGIN.1 * 2.);
        let zoom = crate::plan_view_model::diagram_zoom(&metrics, area, self.diagram_zoom);
        Some((metrics, zoom))
    }

    /// The properties of the selected step as listed below the views (v13 behaviour 20).
    pub fn plan_step_properties(&self) -> Vec<(String, String)> {
        match (self.plan.as_ref(), self.plan_step) {
            (Some(plan), Some(step)) => crate::plan_view_model::properties(plan, step),
            _ => Vec::new(),
        }
    }

    /// The selected step of the Plan tab (v5 behaviour 11).
    pub fn plan_step(&self) -> Option<usize> {
        self.plan_step
    }

    /// Selects step `i` of the Plan tab.
    pub fn select_plan_step(&mut self, i: usize, cx: &mut Context<Self>) {
        self.plan_step = Some(i);
        cx.notify();
    }

    /// The plan as ⌘C copies it: the raw text in Raw, the indented tree in Tree (v5 behaviour 11).
    pub fn plan_copy_text(&self) -> Option<String> {
        let plan = self.plan.as_ref()?;
        Some(if self.plan_raw() {
            plan.raw.clone()
        } else {
            crate::db_compare_model::plan_text(plan)
        })
    }

    /// The marked statement of the Schema Diff tab (v5 behaviour 14).
    pub fn statement_mark(&self) -> Option<usize> {
        self.statement_mark
    }

    /// Whether the Plan tab is the one shown.
    fn plan_shown(&self) -> bool {
        self.tab == self.results.len() + 1 && self.extra_tab() == Some("Plan")
    }

    /// The keys of the Plan and Schema Diff tabs (`SPEC-0110` v5 behaviours 10, 11, 14); true
    /// when the key was used.
    fn on_db_tab_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let cmd = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        if self.plan_shown() {
            let steps = self
                .plan
                .as_ref()
                .map_or(0, |p| crate::db_compare_model::plan_rows(p).len());
            let hints = self.plan.as_ref().map_or(0, |p| p.hints.len());
            match k.key.as_str() {
                "r" if m.alt && !cmd => self.plan_mode = self.plan_mode.next(),
                // Several hints fold to one line and open again (v14 behaviour 21).
                "h" if m.alt && !cmd && hints > 1 && self.plan_mode.has_steps() => {
                    self.plan_hints_open = !self.plan_hints_open
                }
                "c" if cmd => {
                    let Some(text) = self.plan_copy_text() else {
                        return false;
                    };
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                    self.last_copy = Some(text);
                }
                "down" | "up" if self.plan_mode.has_steps() && !cmd => {
                    self.plan_step = crate::db_compare_model::move_marker(
                        self.plan_step,
                        steps,
                        k.key == "down",
                    );
                }
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if self.schema_diff_shown() && !self.confirming_migration() && !cmd {
            let len = self
                .schema_diff
                .as_ref()
                .and_then(|t| t.choice.as_ref())
                .map_or(0, |c| c.statements.len());
            match k.key.as_str() {
                "down" | "up" => {
                    self.statement_mark = crate::db_compare_model::move_marker(
                        self.statement_mark,
                        len,
                        k.key == "down",
                    );
                }
                "space" => {
                    let Some(i) = self.statement_mark else {
                        return false;
                    };
                    self.change_migration(
                        |c| {
                            c.click(i);
                        },
                        cx,
                    );
                }
                _ => return false,
            }
            cx.notify();
            return true;
        }
        false
    }

    /// The schema comparison of the last run, if it was one (`SPEC-0110`).
    pub fn schema_diff(&self) -> Option<&SchemaDiffTab> {
        self.schema_diff.as_ref()
    }

    /// The response text, if the last run was a request.
    pub fn response(&self) -> Option<&str> {
        self.response.as_deref()
    }

    /// The shown result set, if a result tab is selected.
    pub fn shown_result(&self) -> Option<&ResultSet> {
        self.tab.checked_sub(1).and_then(|i| self.results.get(i))
    }

    /// The pending edits of the shown result, if its rows can be edited.
    pub fn shown_edits(&self) -> Option<&GridEdits> {
        self.shown_index().and_then(|i| self.edits[i].as_ref())
    }

    /// The selected cell.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.selection.map(|s| s.cursor)
    }

    /// The selected cells.
    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    /// Whether the selected cell is being edited.
    pub fn editing(&self) -> bool {
        self.editing
    }

    /// The note under the grid.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The value of cell (`row`, `col`) of result `i` as shown, pending changes included; added
    /// rows give `None` for columns left to their defaults.
    fn value_at(&self, i: usize, row: usize, col: usize) -> Option<Option<String>> {
        let rows = &self.results[i].rows;
        match (&self.edits[i], row.checked_sub(rows.len())) {
            (Some(e), Some(added)) => e.added().get(added)?.get(col).cloned(),
            (Some(e), None) => e.value(rows, row, col).cloned(),
            (None, _) => rows.get(row)?.get(col).cloned(),
        }
    }

    /// The text of cell (`row`, `col`) of the shown result as shown, pending changes included.
    pub fn cell_text_at(&self, row: usize, col: usize) -> Option<String> {
        let i = self.shown_index()?;
        let value = self.value_at(i, row, col)?;
        Some(cell_text(&value).to_owned())
    }

    /// Row ids of result `i` in shown order: the page, then the added rows.
    fn shown_ids(&self, i: usize) -> Vec<usize> {
        let loaded = self.results[i].rows.len();
        let mut ids: Vec<usize> = self.visible[i][self.page_rows(i)].to_vec();
        if let Some(e) = &self.edits[i] {
            ids.extend((0..e.added().len()).map(|k| loaded + k));
        }
        ids
    }

    /// Selects cell (`row`, `col`) of the shown result.
    pub fn select_cell(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        self.selection = Some(Selection::cell(row, col));
        self.editing = false;
        cx.notify();
    }

    /// Extends the selection from its anchor to (`row`, `col`) (⇧-click, behaviour 11).
    pub fn extend_selection(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        let anchor = self.selection.map_or((row, col), |s| s.anchor);
        self.selection = Some(Selection {
            anchor,
            cursor: (row, col),
        });
        self.editing = false;
        cx.notify();
    }

    /// Selects every shown row and column (⌘A).
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let ids = self.shown_ids(i);
        let cols = self.results[i].columns.len();
        if let (Some(&first), Some(&last), true) = (ids.first(), ids.last(), cols > 0) {
            self.selection = Some(Selection {
                anchor: (first, 0),
                cursor: (last, cols - 1),
            });
        }
        cx.notify();
    }

    /// Why result `i` cannot be edited.
    fn read_only_note(&self, i: usize) -> String {
        self.infos[i]
            .table
            .as_ref()
            .and_then(|t| t.read_only.clone())
            .unwrap_or_else(|| {
                "Read-only: the query does not select from one table with its key".into()
            })
    }

    /// Starts editing the selected cell: the field takes its current text (behaviour 2).
    pub fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(i), Some((row, col))) = (self.shown_index(), self.cursor()) else {
            return;
        };
        match &self.edits[i] {
            None => self.note = Some(self.read_only_note(i)),
            Some(e) if !e.editable(col) => {
                self.note = Some(format!(
                    "`{}` is not a column of `{}`",
                    self.results[i].columns[col],
                    e.table()
                ));
            }
            Some(_) => {
                let text = self.value_at(i, row, col).flatten().unwrap_or_default();
                self.cell.update(cx, |f, cx| {
                    f.set_text(&text, cx);
                    let end = f.state.buffer.len_chars();
                    f.select_range(end..end, cx);
                });
                self.editing = true;
                window.focus(&self.cell.read(cx).focus());
            }
        }
        cx.notify();
    }

    fn set_value(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        let (Some(i), Some((row, col))) = (self.shown_index(), self.cursor()) else {
            return;
        };
        let rows = &self.results[i].rows;
        if let Some(e) = self.edits[i].as_mut()
            && let Err(err) = e.set(rows, row, col, value)
        {
            self.note = Some(err.to_string());
        }
        cx.notify();
    }

    /// Takes the field's text as the cell's new value (Enter while editing).
    pub fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        self.editing = false;
        let text = self.cell.read(cx).state.buffer.text();
        self.set_value(Some(text), cx);
        window.focus(&self.focus);
    }

    /// Sets the selected cell to NULL (behaviour 2).
    pub fn set_null(&mut self, cx: &mut Context<Self>) {
        self.set_value(None, cx);
    }

    /// Adds a row to the shown result and selects its first cell (behaviour 3).
    pub fn add_row(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let loaded = self.results[i].rows.len();
        if let Some(e) = self.edits[i].as_mut() {
            let row = e.add_row(loaded);
            self.selection = Some(Selection::cell(row, 0));
        } else {
            self.note = Some(self.read_only_note(i));
        }
        cx.notify();
    }

    /// Marks the selected row for deletion, or unmarks it (behaviour 4).
    pub fn toggle_delete_row(&mut self, cx: &mut Context<Self>) {
        let (Some(i), Some((row, _))) = (self.shown_index(), self.cursor()) else {
            return;
        };
        let loaded = self.results[i].rows.len();
        if let Some(e) = self.edits[i].as_mut() {
            e.toggle_delete(loaded, row);
        }
        cx.notify();
    }

    /// Drops the pending edits of the shown result (behaviour 5).
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = self.shown_index().and_then(|i| self.edits[i].as_mut()) {
            e.revert();
            self.note = None;
        }
        cx.notify();
    }

    /// Applies the pending edits of the shown result (behaviour 5): a console result asks the
    /// workbench, a data editor uses its own session in its transaction mode (`SPEC-0190`
    /// behaviour 9).
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let Some(e) = self.edits[i].as_ref().filter(|e| !e.is_empty()) else {
            return;
        };
        let set = match e.edit_set(&self.results[i].rows) {
            Ok(set) => set,
            Err(err) => {
                self.note = Some(err.to_string());
                cx.notify();
                return;
            }
        };
        self.note = Some(format!("Submitting {} change(s)…", e.count()));
        self.preview = false;
        if self.infos[i].table.is_some() {
            self.submit_table(i, set, cx);
        } else {
            cx.emit(ServicesEvent::Submit {
                set,
                query: self.results[i].sql.clone(),
            });
        }
        cx.notify();
    }

    /// Notes how a submit went; the workbench runs the query again after a commit.
    pub fn submitted(&mut self, note: String, cx: &mut Context<Self>) {
        self.note = Some(note);
        cx.notify();
    }

    /// The statements submitting would run, in order (`SPEC-0190` behaviour 7).
    pub fn preview_sql(&self) -> Result<Vec<String>, String> {
        let i = self.shown_index().ok_or("no result")?;
        let e = self.edits[i]
            .as_ref()
            .ok_or_else(|| self.read_only_note(i))?;
        e.edit_set(&self.results[i].rows)
            .map(|s| s.sql())
            .map_err(|e| e.to_string())
    }

    /// Whether the DML preview is shown.
    pub fn previewing(&self) -> bool {
        self.preview
    }

    /// Shows or hides the DML preview (behaviour 7).
    pub fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        self.preview = !self.preview && self.shown_edits().is_some();
        cx.notify();
    }

    /// The side panel's view, if shown (behaviour 6).
    pub fn side_view(&self) -> Option<SideView> {
        self.side
    }

    /// Shows the side panel with `view`, or hides it when it shows that view already (or with
    /// `None`).
    pub fn show_side(&mut self, view: Option<SideView>, cx: &mut Context<Self>) {
        self.side = if view == self.side { None } else { view };
        cx.notify();
    }

    /// The record view of the selected row (behaviour 6).
    pub fn record(&self) -> Vec<(String, String)> {
        let (Some(i), Some((row, _))) = (self.shown_index(), self.cursor()) else {
            return Vec::new();
        };
        let loaded = self.results[i].rows.len();
        let values: Vec<Option<String>> = (0..self.results[i].columns.len())
            .map(|k| match self.value_at(i, row, k) {
                Some(None) if row >= loaded => Some(DEFAULT_TEXT.to_owned()),
                v => v.flatten(),
            })
            .collect();
        record_lines(&self.results[i].columns, &values)
    }

    /// The value view of the selected cell (behaviour 6).
    pub fn value(&self) -> String {
        let (Some(i), Some((row, col))) = (self.shown_index(), self.cursor()) else {
            return String::new();
        };
        value_text(&self.value_at(i, row, col).flatten())
    }

    /// The summary view of the selected column (behaviour 6).
    pub fn summary(&self) -> (String, Vec<String>) {
        let (Some(i), Some((_, col))) = (self.shown_index(), self.cursor()) else {
            return (String::new(), Vec::new());
        };
        let values: Vec<Option<String>> = self.results[i]
            .rows
            .iter()
            .map(|r| r.get(col).cloned().flatten())
            .collect();
        let total = self.infos[i].table.as_ref().map(|t| t.total);
        summary_lines(&values, total)
    }

    /// Pins or unpins the shown result (behaviour 1).
    pub fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            self.infos[i].pinned = !self.infos[i].pinned;
            cx.notify();
        }
    }

    /// Closes result tab `i` (behaviour 1); a data editor's open transaction is rolled back and
    /// Output says so (behaviour 9).
    pub fn close_result(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.results.len() {
            return;
        }
        if let Some(t) = &self.infos[i].table
            && t.tx_open
        {
            let _ = t
                .session
                .run(|rt, s| table_session::execute(rt, s, "ROLLBACK"));
            self.output.push((
                format!(
                    "{}: closed with an open transaction; {} change(s) rolled back",
                    self.infos[i].title, t.tx_changes
                ),
                false,
            ));
        }
        self.results.remove(i);
        self.grids.remove(i);
        self.visible.remove(i);
        self.edits.remove(i);
        self.infos.remove(i);
        let tab = if self.tab == i + 1 {
            i.min(self.results.len())
        } else if self.tab > i + 1 && self.tab != PORTS_TAB {
            self.tab - 1
        } else {
            self.tab
        };
        self.tab = usize::MAX;
        self.select_tab(tab, cx);
    }

    /// The copy format and header setting.
    pub fn copy_settings(&self) -> &CopySettings {
        &self.copy
    }

    /// Chooses the copy format (SPEC-0021 v2 behaviour 10).
    pub fn set_copy_format(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        self.copy.format = format;
        self.copy_menu = false;
        cx.notify();
    }

    /// Switches the header row of every text format (IMP-0061).
    pub fn set_header(&mut self, on: bool, cx: &mut Context<Self>) {
        self.copy.header = on;
        cx.notify();
    }

    /// Table, key and dialect for the SQL formats of result `i`.
    fn sql_target(&self, i: usize) -> (Option<String>, Vec<String>, Dialect) {
        match (&self.edits[i], &self.infos[i].table) {
            (Some(e), _) => (
                Some(e.table().to_owned()),
                e.target().key.clone(),
                e.target().dialect,
            ),
            (None, Some(t)) => (
                Some(t.query.table.clone()),
                t.query.key.clone(),
                t.profile.dialect,
            ),
            (None, None) => (None, Vec::new(), Dialect::PostgreSql),
        }
    }

    /// The text ⌘C copies from the shown result (SPEC-0021 v2 behaviour 10).
    pub fn selection_text(&self) -> Option<String> {
        let i = self.shown_index()?;
        let selection = self.selection?;
        let ids = self.shown_ids(i);
        let (table, key, dialect) = self.sql_target(i);
        let values = |r: usize, c: usize| self.value_at(i, r, c).flatten();
        Some(copy_text(
            &self.results[i].columns,
            &values,
            &selection,
            &ids,
            &self.copy,
            table.as_deref(),
            &key,
            dialect,
        ))
    }

    /// Copies the selected cells to the clipboard (⌘C).
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            self.last_copy = Some(text);
            cx.notify();
        }
    }

    /// The text of the last copy (tests).
    pub fn last_copy(&self) -> Option<&str> {
        self.last_copy.as_deref()
    }

    /// Opens a data editor for `table` of `profile`, or selects its tab (`SPEC-0190` behaviour 2).
    pub fn open_table(&mut self, profile: ConnectionProfile, table: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.infos.iter().position(|t| {
            t.table
                .as_ref()
                .is_some_and(|t| t.profile.id == profile.id && t.query.table == table)
        }) {
            self.select_tab(i + 1, cx);
            return;
        }
        let id = self.next_table_id;
        self.next_table_id += 1;
        self.connection = profile.name.clone();
        self.results.push(ResultSet {
            columns: Vec::new(),
            rows: Vec::new(),
            time: Duration::ZERO,
            sql: String::new(),
        });
        self.grids.push(GridState::default());
        self.visible.push(Vec::new());
        self.edits.push(None);
        self.infos.push(TabInfo {
            title: table.to_owned(),
            pinned: false,
            table: Some(TableTab {
                id,
                session: TableSession::open(profile.clone()),
                // SPEC-0108 v4 behaviour 10: the connection may start editors in Manual.
                manual: !profile.options.auto_commit,
                profile,
                query: TableQuery::new(table, Vec::new()),
                page: 0,
                total: 0,
                loading: true,
                tx_open: false,
                tx_changes: 0,
                savepoints: 0,
                read_only: None,
                isolation: idecraft_database::Isolation::default(),
            }),
            ..TabInfo::default()
        });
        let i = self.results.len() - 1;
        self.tab = usize::MAX;
        self.select_tab(i + 1, cx);
        let Some(t) = &self.infos[i].table else {
            return;
        };
        let name = table.to_owned();
        let d = t.profile.dialect;
        let rx = t.session.run(move |rt, s| {
            let found = table_session::read_table(rt, s, &name)?;
            let target = (found.kind == "table")
                .then(|| EditTarget::new(&found, d))
                .transpose();
            let (key, read_only) = match target {
                Ok(Some(t)) => (t.key.clone(), None),
                Ok(None) => (
                    found.primary_key.clone(),
                    Some(format!("Read-only: `{name}` is a view")),
                ),
                Err(e) => (found.primary_key.clone(), Some(format!("Read-only: {e}"))),
            };
            let mut query = TableQuery::new(&name, key);
            query.schema = None;
            let page = table_session::load_page(rt, s, &query, 0, idecraft_database::TABLE_PAGE)?;
            Ok((found, query, page, read_only))
        });
        let task = cx.background_executor().spawn(async move {
            rx.recv()
                .unwrap_or_else(|_| Err("the connection closed".into()))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(i) = this.table_index(id) else {
                    return;
                };
                match result {
                    Ok((found, query, page, read_only)) => {
                        let edits = match &read_only {
                            None => EditTarget::new(&found, d)
                                .ok()
                                .and_then(|t| GridEdits::new(t, page.columns.clone()).ok()),
                            Some(_) => None,
                        };
                        if let Some(t) = this.infos[i].table.as_mut() {
                            t.query = query;
                            t.read_only = read_only;
                        }
                        this.edits[i] = edits;
                        this.show_page(i, page, cx);
                    }
                    Err(e) => this.table_failed(i, e, cx),
                }
            });
        })
        .detach();
    }

    fn table_index(&self, id: u64) -> Option<usize> {
        self.infos
            .iter()
            .position(|t| t.table.as_ref().is_some_and(|t| t.id == id))
    }

    /// Shows a loaded page in data editor `i`.
    fn show_page(&mut self, i: usize, page: table_session::Page, cx: &mut Context<Self>) {
        if let Some(t) = self.infos[i].table.as_mut() {
            t.total = page.total;
            t.loading = false;
            if !t.query.stable() {
                self.note = Some("The table has no key: pages may overlap".into());
            }
        }
        let columns_changed = self.results[i].columns != page.columns;
        self.results[i] = ResultSet {
            columns: page.columns,
            rows: page.rows,
            time: page.time,
            sql: page.sql,
        };
        if let Some(e) = self.edits[i].as_mut() {
            e.revert();
        }
        if columns_changed && let Some(e) = &self.edits[i] {
            self.edits[i] =
                GridEdits::new(e.target().clone(), self.results[i].columns.clone()).ok();
        }
        self.visible[i] = (0..self.results[i].rows.len()).collect();
        if self.shown_index() == Some(i) {
            self.selection = None;
        }
        cx.notify();
    }

    fn table_failed(&mut self, i: usize, error: String, cx: &mut Context<Self>) {
        if let Some(t) = self.infos[i].table.as_mut() {
            t.loading = false;
        }
        self.output
            .push((format!("{}: {error}", self.infos[i].title), true));
        self.note = Some(error);
        cx.notify();
    }

    /// Loads the current page of data editor `i` again (behaviours 3–4).
    fn load_table(&mut self, i: usize, cx: &mut Context<Self>) {
        let size = self.table_size(i);
        let Some(t) = self.infos[i].table.as_mut() else {
            return;
        };
        let d = t.profile.dialect;
        if let Err(e) = t.query.page_sql(d, t.page, size) {
            self.note = Some(e.to_string());
            cx.notify();
            return;
        }
        t.loading = true;
        self.note = None;
        let (id, query, page) = (t.id, t.query.clone(), t.page);
        let rx = t
            .session
            .run(move |rt, s| table_session::load_page(rt, s, &query, page, size));
        self.after(id, rx, |this, i, page, cx| this.show_page(i, page, cx), cx);
        cx.notify();
    }

    /// Runs `done` with the result of `rx` on data editor `id`, if it is still open; errors go to
    /// the note.
    fn after<T: Send + 'static>(
        &mut self,
        id: u64,
        rx: std::sync::mpsc::Receiver<Result<T, String>>,
        done: impl FnOnce(&mut Self, usize, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let task = cx.background_executor().spawn(async move {
            rx.recv()
                .unwrap_or_else(|_| Err("the connection closed".into()))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(i) = this.table_index(id) {
                    match result {
                        Ok(v) => done(this, i, v, cx),
                        Err(e) => this.table_failed(i, e, cx),
                    }
                }
            });
        })
        .detach();
    }

    /// Takes the `WHERE` and `ORDER BY` fields and loads the first page (Enter, behaviour 3).
    pub fn apply_clauses(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let w = self.where_field.read(cx).state.buffer.text();
        let o = self.order_field.read(cx).state.buffer.text();
        if let Some(t) = self.infos[i].table.as_mut() {
            t.query.filter = w.trim().to_owned();
            if t.query.order != o.trim() {
                self.grids[i].sort.clear();
            }
            t.query.order = o.trim().to_owned();
            t.page = 0;
            self.load_table(i, cx);
        }
    }

    /// Sets the `WHERE` and `ORDER BY` fields and applies them (tests and agents).
    pub fn set_clauses(&mut self, filter: &str, order: &str, cx: &mut Context<Self>) {
        self.where_field.update(cx, |f, cx| f.set_text(filter, cx));
        self.order_field.update(cx, |f, cx| f.set_text(order, cx));
        self.apply_clauses(cx);
    }

    /// The data editor's table, `WHERE`, `ORDER BY` and whether it is loading (tests).
    pub fn table_state(&self) -> Option<(String, String, String, bool)> {
        let i = self.shown_index()?;
        let t = self.infos[i].table.as_ref()?;
        Some((
            t.query.table.clone(),
            t.query.filter.clone(),
            t.query.order.clone(),
            t.loading,
        ))
    }

    /// Manual transaction mode and the number of applied, uncommitted changes, of the shown data
    /// editor (behaviour 9).
    pub fn transaction(&self) -> Option<(bool, Option<usize>)> {
        let t = self.infos[self.shown_index()?].table.as_ref()?;
        Some((t.manual, t.tx_open.then_some(t.tx_changes)))
    }

    /// Switches the shown data editor between Auto and Manual; leaving Manual with an open
    /// transaction rolls it back and says so in Output (behaviour 9).
    pub fn set_manual(&mut self, manual: bool, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let Some(t) = self.infos[i].table.as_mut() else {
            return;
        };
        t.manual = manual;
        if !manual && t.tx_open {
            let n = t.tx_changes;
            self.output.push((
                format!(
                    "{}: switched to Auto; {n} uncommitted change(s) rolled back",
                    self.infos[i].title
                ),
                false,
            ));
            self.end_transaction(i, false, cx);
        }
        cx.notify();
    }

    /// Commits (`commit`) or rolls back the open transaction of the shown data editor, then
    /// loads the page again (behaviour 9).
    pub fn finish_transaction(&mut self, commit: bool, cx: &mut Context<Self>) {
        if let Some(i) = self.shown_index() {
            self.end_transaction(i, commit, cx);
        }
    }

    fn end_transaction(&mut self, i: usize, commit: bool, cx: &mut Context<Self>) {
        let size = self.table_size(i);
        let Some(t) = self.infos[i].table.as_mut() else {
            return;
        };
        if !t.tx_open {
            return;
        }
        let n = t.tx_changes;
        t.tx_open = false;
        t.tx_changes = 0;
        t.savepoints = 0;
        t.loading = true;
        let (id, query, page) = (t.id, t.query.clone(), t.page);
        let sql = if commit { "COMMIT" } else { "ROLLBACK" };
        let rx = t.session.run(move |rt, s| {
            table_session::execute(rt, s, sql)?;
            table_session::load_page(rt, s, &query, page, size)
        });
        self.note = Some(if commit {
            format!("{n} change(s) committed")
        } else {
            format!("{n} change(s) rolled back")
        });
        let note = self.note.clone();
        self.after(
            id,
            rx,
            move |this, i, page, cx| {
                this.show_page(i, page, cx);
                this.note = note;
            },
            cx,
        );
        cx.notify();
    }

    fn submit_table(&mut self, i: usize, set: idecraft_database::EditSet, cx: &mut Context<Self>) {
        let size = self.table_size(i);
        let Some(t) = self.infos[i].table.as_mut() else {
            return;
        };
        let savepoint = t.manual.then(|| {
            t.savepoints += 1;
            (format!("idecraft_{}", t.savepoints), !t.tx_open)
        });
        if t.manual {
            t.tx_open = true;
        }
        let (id, query, page, manual) = (t.id, t.query.clone(), t.page, t.manual);
        let isolation = t.isolation;
        let rx = t.session.run(move |rt, s| {
            let applied = table_session::apply(
                rt,
                s,
                &set,
                savepoint.as_ref().map(|(n, b)| (n.as_str(), *b)),
                isolation,
            )?;
            let page = match applied {
                Applied::Committed(_) => Some(table_session::load_page(rt, s, &query, page, size)?),
                Applied::RolledBack { .. } => None,
            };
            Ok((applied, page))
        });
        self.after(
            id,
            rx,
            move |this, i, (applied, page), cx| {
                let note = match applied {
                    Applied::Committed(n) if manual => {
                        if let Some(t) = this.infos[i].table.as_mut() {
                            t.tx_changes += n;
                        }
                        format!("{n} change(s) applied in the transaction; Commit or Rollback")
                    }
                    Applied::Committed(n) => format!("{n} change(s) committed"),
                    Applied::RolledBack { index, failure } => {
                        let which = index.map_or(String::new(), |k| format!("change {} ", k + 1));
                        if manual {
                            format!("{which}failed; this batch was rolled back, earlier ones stay: {failure}")
                        } else {
                            format!("{which}failed, nothing was changed: {failure}")
                        }
                    }
                };
                if let Some(page) = page {
                    this.show_page(i, page, cx);
                }
                this.note = Some(note);
                cx.notify();
            },
            cx,
        );
    }

    /// Opens the export dialog (SPEC-0021 v2 behaviour 11).
    pub fn open_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let rows = if self.selection.is_some_and(|s| !s.single()) {
            ExportRows::Selection
        } else {
            ExportRows::Loaded
        };
        let path = default_export_path(&self.root, &self.infos[i].title, self.copy.format);
        let field = cx.new(|cx| {
            let mut f = EditorView::field(cx);
            f.set_text(&path.display().to_string(), cx);
            f
        });
        self.export = Some(ExportDialog {
            rows,
            to_file: true,
            path: field,
            note: None,
            running: false,
        });
        self.copy_menu = false;
        window.focus(&self.focus);
        cx.notify();
    }

    /// Whether a result is shown, so there is something to export.
    pub fn shows_result(&self) -> bool {
        self.shown_index().is_some()
    }

    /// Whether the export dialog is open.
    pub fn exporting(&self) -> bool {
        self.export.is_some()
    }

    /// Bottom edges of the export dialog's button row and of the panel, in window pixels, as last
    /// painted; `(0, 0)` before the first paint (self-test of D42).
    pub fn export_edges(&self) -> (f32, f32) {
        self.export_edges.get()
    }

    /// Closes the export dialog.
    pub fn close_export(&mut self, cx: &mut Context<Self>) {
        self.export = None;
        cx.notify();
    }

    /// Chooses the export format; the default file name follows it.
    pub fn set_export_format(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        let old = self.copy.format;
        self.copy.format = format;
        if let Some(e) = &self.export {
            let path = e.path.read(cx).state.buffer.text();
            let suffix = format!(".{}", old.extension());
            if let Some(stem) = path.strip_suffix(&suffix) {
                let new = format!("{stem}.{}", format.extension());
                e.path.update(cx, |f, cx| f.set_text(&new, cx));
            }
        }
        cx.notify();
    }

    /// Chooses which rows the export takes.
    pub fn set_export_rows(&mut self, rows: ExportRows, cx: &mut Context<Self>) {
        if let Some(e) = self.export.as_mut() {
            e.rows = rows;
        }
        cx.notify();
    }

    /// Exports to a file (`Some(path)`) or the clipboard (`None`).
    pub fn set_export_target(&mut self, path: Option<&str>, cx: &mut Context<Self>) {
        if let Some(e) = self.export.as_mut() {
            e.to_file = path.is_some();
            if let Some(p) = path {
                e.path.update(cx, |f, cx| f.set_text(p, cx));
            }
        }
        cx.notify();
    }

    /// Columns and rows the export takes from result `i`; `None` for all rows of a data editor,
    /// which come from the server.
    fn export_data(&self, i: usize) -> Option<table_session::Rows> {
        let rows = self.export.as_ref().map_or(ExportRows::Loaded, |e| e.rows);
        let columns = &self.results[i].columns;
        match (rows, self.selection) {
            (ExportRows::All, _) if self.infos[i].table.is_some() => None,
            (ExportRows::Selection, Some(s)) => {
                let (ids, cols) = s.rect(&self.shown_ids(i));
                Some((
                    columns[cols.clone()].to_vec(),
                    ids.iter()
                        .map(|&r| {
                            cols.clone()
                                .map(|c| self.value_at(i, r, c).flatten())
                                .collect()
                        })
                        .collect(),
                ))
            }
            _ => Some((
                columns.clone(),
                self.visible[i]
                    .iter()
                    .map(|&r| self.results[i].rows[r].clone())
                    .collect(),
            )),
        }
    }

    /// The live preview of the export dialog: the first rows in the chosen format.
    pub fn export_preview(&self) -> String {
        let Some(i) = self.shown_index() else {
            return String::new();
        };
        let (table, key, dialect) = self.sql_target(i);
        let (columns, rows) = self.export_data(i).unwrap_or_else(|| {
            (
                self.results[i].columns.clone(),
                self.results[i].rows.clone(),
            )
        });
        let shown = &rows[..rows.len().min(PREVIEW_ROWS)];
        format_rows(&columns, shown, &self.copy, table.as_deref(), &key, dialect)
    }

    /// Exports with the dialog's choices; the note says how many rows went where.
    pub fn run_export(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.shown_index() else {
            return;
        };
        let Some(dialog) = &self.export else {
            return;
        };
        let target = dialog
            .to_file
            .then(|| PathBuf::from(dialog.path.read(cx).state.buffer.text().trim()));
        let (table, key, dialect) = self.sql_target(i);
        let settings = self.copy.clone();
        match self.export_data(i) {
            Some((columns, rows)) => {
                let text = format_rows(&columns, &rows, &settings, table.as_deref(), &key, dialect);
                self.finish_export(text, rows.len(), target, cx);
            }
            None => {
                let Some(t) = &self.infos[i].table else {
                    return;
                };
                let (id, query) = (t.id, t.query.clone());
                let rx = t
                    .session
                    .run(move |rt, s| table_session::load_all(rt, s, &query));
                if let Some(e) = self.export.as_mut() {
                    e.running = true;
                    e.note = Some(("Reading all rows…".into(), false));
                }
                self.after(
                    id,
                    rx,
                    move |this, _, (columns, rows), cx| {
                        let text = format_rows(
                            &columns,
                            &rows,
                            &settings,
                            table.as_deref(),
                            &key,
                            dialect,
                        );
                        this.finish_export(text, rows.len(), target, cx);
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    fn finish_export(
        &mut self,
        text: String,
        rows: usize,
        target: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let written = match &target {
            Some(path) => path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(path, &text))
                .map(|()| format!("{rows} row(s) exported to {}", path.display()))
                .map_err(|e| format!("{}: {e}", path.display())),
            None => {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                self.last_copy = Some(text);
                Ok(format!("{rows} row(s) copied to the clipboard"))
            }
        };
        match written {
            Ok(note) => {
                self.export = None;
                self.note = Some(note);
            }
            Err(e) => {
                if let Some(d) = self.export.as_mut() {
                    d.running = false;
                    d.note = Some((e, true));
                }
            }
        }
        cx.notify();
    }

    /// Enter keeps and Escape cancels the edited cell; Enter in the `WHERE`/`ORDER BY` fields
    /// applies them. Runs before the fields, which would otherwise take Enter themselves.
    fn on_cell_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let in_clause = self.where_field.read(cx).focus().is_focused(window)
            || self.order_field.read(cx).focus().is_focused(window);
        if in_clause {
            match key {
                "enter" => self.apply_clauses(cx),
                "escape" => window.focus(&self.focus),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if self.export.is_some() && key == "escape" {
            self.close_export(cx);
            cx.stop_propagation();
            return;
        }
        // SPEC-0190 v4 behaviour 14: ⌘↩ (Ctrl+Enter) applies the side panel's value, Escape
        // drops it; a plain Enter is a new line in the value.
        if self.value_editing {
            let m = &event.keystroke.modifiers;
            let cmd = if cfg!(target_os = "macos") {
                m.platform
            } else {
                m.control
            };
            match key {
                "enter" if cmd => self.finish_value_edit(false, window, cx),
                "escape" => self.finish_value_edit(true, window, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if self.value_menu.is_some() && key == "escape" {
            self.open_value_menu(None, cx);
            cx.stop_propagation();
            return;
        }
        if !self.editing {
            return;
        }
        match key {
            "enter" => self.commit_edit(window, cx),
            "escape" => {
                self.editing = false;
                window.focus(&self.focus);
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let cmd = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        // D44: the data-loss question answers to Enter (Include) and Escape (Cancel), only while it
        // is on screen (v4: never from another tab, where it cannot be read).
        if self.confirming_migration() && self.schema_diff_shown() && !cmd {
            match k.key.as_str() {
                "enter" => self.change_migration(
                    |c| {
                        c.confirm();
                    },
                    cx,
                ),
                "escape" => self.change_migration(|c| c.cancel(), cx),
                _ => {}
            }
            if matches!(k.key.as_str(), "enter" | "escape") {
                cx.stop_propagation();
                return;
            }
        }
        if self.on_db_tab_key(event, cx) {
            cx.stop_propagation();
            return;
        }
        // D42: Enter and ⌘Enter run the export while its dialog is open.
        if self.export.is_some() && k.key == "enter" {
            self.run_export(cx);
            cx.stop_propagation();
            return;
        }
        let shown = self.shown_index();
        let ids = shown.map(|i| self.shown_ids(i)).unwrap_or_default();
        let cols = shown.map_or(0, |i| self.results[i].columns.len());
        let (row, col) = self
            .cursor()
            .unwrap_or((ids.first().copied().unwrap_or(0), 0));
        let at = ids.iter().position(|&r| r == row).unwrap_or(0);
        let mut go = |this: &mut Self, r: usize, c: usize| {
            if m.shift {
                this.extend_selection(r, c, cx);
            } else {
                this.select_cell(r, c, cx);
            }
        };
        match k.key.as_str() {
            "escape" if self.export.is_some() => self.close_export(cx),
            "escape" if self.more_menu || self.size_menu || self.tx_menu || self.copy_menu => {
                self.close_toolbar_menus();
            }
            "escape" if self.preview => self.preview = false,
            "escape" => cx.emit(ServicesEvent::Hide),
            "enter" if cmd && m.alt && m.shift => self.toggle_preview(cx),
            "e" if cmd && m.alt && m.shift => self.open_export(window, cx),
            "enter" if cmd => self.submit(cx),
            "enter" if m.shift => self.show_side(Some(self.side.unwrap_or_default()), cx),
            "enter" | "f2" if shown.is_some() => self.start_edit(window, cx),
            "p" if m.alt && m.shift => self.toggle_pin(cx),
            "w" if cmd && shown.is_some() => {
                if let Some(i) = shown {
                    self.close_result(i, cx)
                }
            }
            "f4" if m.control && !cfg!(target_os = "macos") && shown.is_some() => {
                if let Some(i) = shown {
                    self.close_result(i, cx)
                }
            }
            "c" if cmd => self.copy_selection(cx),
            "a" if cmd => self.select_all(cx),
            "z" if cmd && m.alt => self.revert(cx),
            "n" if cmd => self.add_row(cx),
            "insert" if m.alt => self.add_row(cx),
            "backspace" | "delete" if cmd => self.toggle_delete_row(cx),
            "backspace" | "delete" if shown.is_some() => self.set_null(cx),
            "down" if !ids.is_empty() => go(self, ids[(at + 1).min(ids.len() - 1)], col),
            "up" if !ids.is_empty() => go(self, ids[at.saturating_sub(1)], col),
            "right" if cols > 0 => go(self, row, (col + 1).min(cols - 1)),
            "left" if cols > 0 => go(self, row, col.saturating_sub(1)),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn grid(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let mono = mono();
        let set = &self.results[index];
        let state = &self.grids[index];
        let order = &self.visible[index];
        let table = self.infos[index].table.as_ref();
        let range = self.page_rows(index);
        let first =
            range.start + table.map_or(0, |t| t.page.saturating_mul(self.table_size(index)));
        // Hand test 22: a sorted column keeps room for its name next to the mark (`▼1`); a width
        // set by dragging a header edge wins (SPEC-0099 v2 behaviour 8).
        let set_widths = &self.infos[index].widths;
        // An added row shows `<default>` in empty cells: its columns keep room for that text.
        let added = self.edits[index]
            .as_ref()
            .is_some_and(|e| !e.added().is_empty());
        let default_width = DEFAULT_TEXT.chars().count() as f32 * CHAR_WIDTH + 16.;
        let widths: Vec<f32> = column_widths(&set.columns, &set.rows, CHAR_WIDTH)
            .into_iter()
            .enumerate()
            .map(|(k, w)| {
                if let Some(Some(fixed)) = set_widths.get(k) {
                    return *fixed;
                }
                let mark = state.mark(k).map_or(0, |m| m.chars().count() + 1);
                let header = (set.columns[k].chars().count() + mark) as f32 * CHAR_WIDTH + 16.;
                let content = if added { w.max(default_width) } else { w };
                content.max(header) + 2. * CHAR_WIDTH
            })
            .collect();
        // Numbers are aligned right, like the reference (SPEC-0099 v2 behaviour 6).
        let numeric = numeric_columns(set.columns.len(), &set.rows);
        let number_width =
            (format!("{}", first + range.len()).len() as f32 * CHAR_WIDTH + 16.).max(32.);
        let total: f32 = number_width + widths.iter().sum::<f32>();
        let cell = |w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .px_2()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .border_r_1()
                .border_color(c(p.separator))
        };
        let header = div()
            .flex()
            .h(px(ROW_HEIGHT))
            .items_center()
            .w(px(total))
            .border_b_1()
            .border_color(c(p.separator))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(cell(number_width))
            .children(
                set.columns
                    .iter()
                    .zip(&widths)
                    .enumerate()
                    .map(|(k, (name, w))| {
                        let mark = state.mark(k);
                        let width = *w;
                        cell(width)
                            .id(("services-column", k))
                            .relative()
                            .semantics(
                                Semantics::new(Role::ColumnHeader)
                                    .name(name.clone())
                                    .test_id(format!("services-column-{k}")),
                            )
                            .flex()
                            .gap_1()
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .when(numeric[k], |d| d.justify_end())
                            .child(div().min_w_0().text_ellipsis().child(name.clone()))
                            .when_some(mark, |d, m| {
                                d.child(div().flex_none().text_color(c(p.ui_muted)).child(m))
                            })
                            // SPEC-0099 v3 behaviour 9: the column's value filter; the mark is
                            // coloured while the column is filtered.
                            .when(table.is_none(), |d| {
                                let on = state.filters(k);
                                d.child(
                                    div()
                                        .id(("services-column-filter", k))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(c(if on { p.link } else { p.ui_dim }))
                                        .child("⏷")
                                        .on_click(cx.listener(
                                            move |this, _: &gpui::ClickEvent, _, cx| {
                                                cx.stop_propagation();
                                                this.open_value_menu(Some(k), cx);
                                            },
                                        )),
                                )
                            })
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                this.sort_by(k, e.modifiers().shift, cx)
                            }))
                            // SPEC-0099 v2 behaviour 8: drag the right edge to resize, double-click
                            // it to fit the content again.
                            .child(
                                div()
                                    .id(("services-column-edge", k))
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .right(px(-3.))
                                    .w(px(6.))
                                    .cursor_col_resize()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, e: &gpui::MouseDownEvent, _, cx| {
                                                if e.click_count >= 2 {
                                                    this.column_drag = None;
                                                    this.set_column_width(k, None, cx);
                                                } else {
                                                    this.column_drag = Some(ColumnDrag {
                                                        column: k,
                                                        start_x: f32::from(e.position.x),
                                                        start_width: width,
                                                    });
                                                }
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                    .on_click(|_, _, cx| cx.stop_propagation()),
                            )
                    }),
            );
        let edits = self.edits[index].as_ref();
        let loaded = set.rows.len();
        let selection = self.selection;
        let ids = self.shown_ids(index);
        let cursor = self.cursor();
        let editing = self.editing;
        let field = self.cell.clone();
        let changed = p.diff_modified;
        let added = p.diff_inserted;
        let deleted = p.diff_deleted;
        let mut shown: Vec<(usize, String, Vec<Option<String>>)> = order[range.clone()]
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let values = (0..set.columns.len())
                    .map(|k| match edits {
                        Some(e) => e.value(&set.rows, r, k).cloned().flatten(),
                        None => set.rows[r].get(k).cloned().flatten(),
                    })
                    .collect();
                (r, format!("{}", first + i + 1), values)
            })
            .collect();
        if let Some(e) = edits {
            shown.extend(
                e.added()
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (loaded + i, "+".to_owned(), v.clone())),
            );
        }
        // SPEC-0099 v2 behaviour 7: the transposed view of the shown page.
        let tview = self.infos[index]
            .transposed
            .then(|| self.transposed_view(&set.columns, &shown, &numeric, cx));
        let rows = shown
            .into_iter()
            .map(|(r, number, values)| {
                div()
                    .flex()
                    .h(px(ROW_HEIGHT))
                    .items_center()
                    .w(px(total))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(cell(number_width).text_color(c(p.ui_dim)).child(number))
                    .children(widths.iter().enumerate().map(|(k, w)| {
                        let value = values.get(k).cloned().unwrap_or(None);
                        let state = edits.map_or(CellState::Loaded, |e| e.state(loaded, r, k));
                        let here = cursor == Some((r, k));
                        let inside =
                            selection.is_some_and(|s| !s.single() && s.contains(&ids, r, k));
                        let text = match (&value, state) {
                            (None, CellState::Added) => DEFAULT_TEXT.to_owned(),
                            _ => cell_text(&value).to_owned(),
                        };
                        cell(*w)
                            .id(("services-cell", r * 10_000 + k))
                            .relative()
                            .semantics(
                                Semantics::new(Role::Cell)
                                    .test_id(format!("services-cell-{r}-{k}")),
                            )
                            .font(mono.clone())
                            .when(numeric[k], |d| d.text_right())
                            .when(value.is_none(), |d| d.text_color(c(p.ui_dim)))
                            .when(inside, |d| d.bg(c(p.tree_selection)))
                            .when(state == CellState::Changed, |d| d.bg(rgb(changed)))
                            .when(state == CellState::Added, |d| d.bg(rgb(added)))
                            .when(state == CellState::Deleted, |d| {
                                d.bg(rgb(deleted)).line_through()
                            })
                            .when(here, |d| {
                                d.border_1().border_color(c(p.tab_selected_border))
                            })
                            .when(here && editing, |d| d.child(field.clone()))
                            .when(!(here && editing), |d| d.child(text))
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                                if e.modifiers().shift {
                                    this.extend_selection(r, k, cx);
                                    return;
                                }
                                this.select_cell(r, k, cx);
                                if e.click_count() >= 2 {
                                    this.start_edit(window, cx);
                                }
                            }))
                    }))
            })
            .collect::<Vec<_>>();
        let (page, last) = match table {
            Some(t) => (t.page, page_count(t.total, self.table_size(index)) - 1),
            None => (state.page, pages_of(order.len(), state.size()) - 1),
        };
        let pager = |control: &'static idecraft_ui::tooltip::Control,
                     glyph: &'static str,
                     delta: i64,
                     on: bool,
                     cx: &mut Context<Self>| {
            div()
                .id(control.name)
                .tip(control)
                .px_1()
                .rounded(px(4.))
                .when(!on, |d| d.opacity(0.4))
                .when(on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(glyph)
                .on_click(
                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.turn_page(delta, cx)),
                )
        };
        // D41: a field never shrinks below a usable width; the toolbar wraps instead.
        let labelled_field = |label: &'static str, w: f32, field: Entity<EditorView>| {
            div()
                .relative()
                .semantics(
                    Semantics::new(Role::TextInput)
                        .name(label)
                        .test_id(format!("services-field-{label}")),
                )
                .ml_2()
                .w(px(w))
                .flex_none()
                .h(px(22.))
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .child(div().flex_none().text_color(c(p.ui_dim)).child(label))
                .child(div().flex_1().min_w_0().h(px(18.)).child(field))
        };
        let button = |id: &'static str, label: String, cx: &mut Context<Self>| {
            div()
                .id(id)
                .relative()
                .semantics(Semantics::new(Role::Button).name(label.clone()).test_id(id))
                .px_2()
                .rounded(px(4.))
                .hover(|s| s.bg(c(p.tree_hover)))
                .flex_none()
                .whitespace_nowrap()
                .child(label)
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    this.toolbar_action(id, window, cx)
                }))
        };
        // SPEC-0190 v5 behaviour 3: one row; the groups that do not fit go into the ⋮ menu.
        let (shown_groups, more_groups) = self.toolbar_groups(index);
        let has = |g: ToolbarGroup| shown_groups.contains(&g);
        let width_cell = self.toolbar_width.clone();
        let this_view = cx.entity().downgrade();
        let mut toolbar = div()
            .relative()
            .h(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(c(p.separator))
            .child(pager(
                declared!(SERVICES_TOOLBAR, "services-first"),
                "⏮",
                i64::MIN,
                page > 0,
                cx,
            ))
            .child(pager(
                declared!(SERVICES_TOOLBAR, "services-previous"),
                "‹",
                -1,
                page > 0,
                cx,
            ))
            .child(
                div()
                    .px_1()
                    .text_color(c(p.ui_muted))
                    .child(self.label(index)),
            )
            .child(pager(
                declared!(SERVICES_TOOLBAR, "services-next"),
                "›",
                1,
                page < last,
                cx,
            ))
            .child(pager(
                declared!(SERVICES_TOOLBAR, "services-last"),
                "⏭",
                i64::MAX,
                page < last,
                cx,
            ));
        toolbar = match table {
            Some(_) => toolbar
                .child(labelled_field("WHERE", 240., self.where_field.clone()))
                .child(labelled_field("ORDER BY", 180., self.order_field.clone())),
            None => toolbar.child(labelled_field("Filter", 260., self.filter.clone())),
        };
        // SPEC-0099 v2 behaviours 5 and 7: page size and transposed view.
        if has(ToolbarGroup::PageSize) {
            let size_button = button(
                "services-page-size",
                format!("Page: {} ▾", page_size_label(state.size())),
                cx,
            );
            toolbar = toolbar.child(
                div()
                    .relative()
                    .flex_none()
                    .child(size_button)
                    .when(self.size_menu, |d| {
                        d.child(drop_down(px(22.), false, self.size_menu_list(index, cx)))
                    }),
            );
        }
        if has(ToolbarGroup::Transpose) {
            toolbar = toolbar.child(button(
                "services-transpose",
                self.transpose_label(index).into(),
                cx,
            ));
        }
        // SPEC-0099 v3 behaviour 10: Table, Tree or Text.
        if has(ToolbarGroup::View) {
            use idecraft_database::grid::ResultView;
            let current = self.infos[index].view;
            toolbar = toolbar.child(
                div()
                    .ml_1()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(
                        [ResultView::Table, ResultView::Tree, ResultView::Text].map(|v| {
                            div()
                                .id(match v {
                                    ResultView::Table => "services-view-table",
                                    ResultView::Tree => "services-view-tree",
                                    ResultView::Text => "services-view-text",
                                })
                                .tip(match v {
                                    ResultView::Table => {
                                        declared!(SERVICES_TOOLBAR, "services-view-table")
                                    }
                                    ResultView::Tree => {
                                        declared!(SERVICES_TOOLBAR, "services-view-tree")
                                    }
                                    ResultView::Text => {
                                        declared!(SERVICES_TOOLBAR, "services-view-text")
                                    }
                                })
                                .px_1()
                                .rounded(px(4.))
                                .when(v == current, |d| d.bg(c(p.tree_selection)))
                                .hover(|s| s.bg(c(p.tree_hover)))
                                .child(v.label())
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.set_result_view(v, cx)
                                }))
                        }),
                    ),
            );
        }
        if has(ToolbarGroup::Edits) {
            toolbar = toolbar.child(
                div()
                    .ml_2()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(
                        self.toolbar_entries(index, ToolbarGroup::Edits)
                            .into_iter()
                            .map(|(id, label)| button(id, label, cx)),
                    ),
            );
        }
        if has(ToolbarGroup::Tx) {
            let mut entries = self
                .toolbar_entries(index, ToolbarGroup::Tx)
                .into_iter()
                .map(|(id, label)| button(id, label, cx))
                .collect::<Vec<_>>()
                .into_iter();
            if let Some(tx_button) = entries.next() {
                toolbar = toolbar.child(
                    div()
                        .relative()
                        .flex_none()
                        .child(tx_button)
                        .when(self.tx_menu, |d| {
                            d.child(drop_down(px(22.), false, self.tx_menu_list(index, cx)))
                        }),
                );
            }
            toolbar = toolbar.children(entries);
        }
        toolbar = toolbar.child(div().flex_1());
        for g in [
            ToolbarGroup::CopyAs,
            ToolbarGroup::Record,
            ToolbarGroup::Export,
        ] {
            if has(g) {
                toolbar = toolbar.children(
                    self.toolbar_entries(index, g)
                        .into_iter()
                        .map(|(id, label)| button(id, label, cx)),
                );
            }
        }
        // The ⋮ menu: the groups that did not fit, in row order; a page size or transaction menu
        // opened from it hangs from the ⋮ button.
        if !more_groups.is_empty() {
            let entries: Vec<(&'static str, String)> = more_groups
                .iter()
                .flat_map(|g| self.toolbar_entries(index, *g))
                .collect();
            let size_here = self.size_menu && more_groups.contains(&ToolbarGroup::PageSize);
            let tx_here = self.tx_menu && more_groups.contains(&ToolbarGroup::Tx);
            toolbar = toolbar.child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        div()
                            .id("services-more")
                            .tip(declared!(SERVICES_TOOLBAR, "services-more"))
                            .px_1()
                            .rounded(px(4.))
                            .when(self.more_menu, |d| d.bg(c(p.tree_hover)))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(crate::icon_images::icon("general/moreVertical"))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                let open = !this.more_menu;
                                this.close_toolbar_menus();
                                this.more_menu = open;
                                cx.notify();
                            })),
                    )
                    .when(self.more_menu, |d| {
                        d.child(drop_down(
                            px(22.),
                            true,
                            div()
                                .id("services-more-menu")
                                .min_w(px(180.))
                                .p_1()
                                .flex()
                                .flex_col()
                                .bg(c(p.island))
                                .border_1()
                                .border_color(c(p.separator))
                                .rounded(px(6.))
                                .shadow_lg()
                                .occlude()
                                .children(entries.into_iter().map(|(id, label)| {
                                    div()
                                        .id(gpui::SharedString::from(format!("more-{id}")))
                                        .relative()
                                        // Marked, without a tooltip, for clicks.
                                        .tip_text(
                                            format!("more-{id}"),
                                            idecraft_ui::tooltip::Tip::default(),
                                        )
                                        .px_2()
                                        .rounded(px(4.))
                                        .whitespace_nowrap()
                                        .hover(|s| s.bg(c(p.tree_hover)))
                                        .child(label)
                                        .on_click(cx.listener(
                                            move |this, _: &gpui::ClickEvent, window, cx| {
                                                this.more_menu = false;
                                                this.toolbar_action(id, window, cx)
                                            },
                                        ))
                                })),
                        ))
                    })
                    .when(size_here, |d| {
                        d.child(drop_down(px(22.), true, self.size_menu_list(index, cx)))
                    })
                    .when(tx_here, |d| {
                        d.child(drop_down(px(22.), true, self.tx_menu_list(index, cx)))
                    }),
            );
        }
        // The toolbar's width as painted; a change draws the row again with the groups that fit.
        toolbar = toolbar.child(
            gpui::canvas(
                move |bounds, _, cx| {
                    let w = f32::from(bounds.size.width);
                    if (w - width_cell.get()).abs() > 0.5 {
                        width_cell.set(w);
                        let view = this_view.clone();
                        cx.defer(move |cx| {
                            view.update(cx, |_, cx| cx.notify()).ok();
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        let grid = div()
            .id("services-grid")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_scroll()
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                let Some(drag) = this.column_drag else {
                    return;
                };
                if e.pressed_button != Some(gpui::MouseButton::Left) {
                    this.column_drag = None;
                    return;
                }
                let w = drag.start_width + f32::from(e.position.x) - drag.start_x;
                this.set_column_width(drag.column, Some(w), cx);
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, _| this.column_drag = None),
            )
            .child(match (self.infos[index].view, tview) {
                (idecraft_database::grid::ResultView::Text, _) => div()
                    .id("services-text-view")
                    .p_2()
                    .font(mono.clone())
                    .whitespace_nowrap()
                    .flex()
                    .flex_col()
                    .children(self.text_view().lines().map(|l| div().child(l.to_owned())))
                    .into_any_element(),
                (idecraft_database::grid::ResultView::Tree, _) => div()
                    .id("services-tree-view")
                    .p_1()
                    .flex()
                    .flex_col()
                    .children(self.tree_lines().into_iter().map(|line| {
                        let row = line.row;
                        div()
                            .id((
                                "services-tree-line",
                                row * 10_000 + line.column.map_or(0, |c| c + 1),
                            ))
                            .h(px(ROW_HEIGHT))
                            .flex()
                            .items_center()
                            .gap_1()
                            .pl(px(8. + 16. * line.depth as f32))
                            .font(mono.clone())
                            .whitespace_nowrap()
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .when(line.depth == 0, |d| {
                                d.child(if line.expanded { "▾" } else { "▸" })
                            })
                            .child(line.text)
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                match line.column {
                                    None => this.toggle_tree_row(row, cx),
                                    Some(k) => {
                                        if let Some(i) = this.shown_index() {
                                            let range = this.page_rows(i);
                                            if let Some(&r) = this.visible[i][range].get(row) {
                                                this.select_cell(r, k, cx);
                                            }
                                        }
                                    }
                                }
                            }))
                    }))
                    .into_any_element(),
                (_, Some(t)) => t,
                (_, None) => div()
                    .flex()
                    .flex_col()
                    .child(header)
                    .children(rows)
                    .into_any_element(),
            });
        let middle = div()
            .flex_1()
            .min_h_0()
            .flex()
            .relative()
            .child(grid)
            .when_some(self.side, |d, view| d.child(self.side_panel(view, cx)))
            .when(self.copy_menu, |d| d.child(self.copy_menu_list(cx)))
            .when_some(self.value_menu, |d, column| {
                d.child(self.value_menu_list(column, cx))
            });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .relative()
            .child(toolbar)
            .child(middle)
            .when(self.preview, |d| d.child(self.preview_panel(cx)))
            .child(
                div()
                    .h(px(24.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .text_color(c(p.ui_muted))
                    .child(format!(
                        "{} · {} ms{}",
                        self.label(index),
                        set.time.as_millis(),
                        match (&self.note, edits, table) {
                            (Some(n), _, _) => format!(" · {n}"),
                            (None, _, Some(t)) if t.loading => " · loading…".into(),
                            (None, Some(e), _) => format!(" · editing `{}`", e.table()),
                            (None, None, _) => String::new(),
                        }
                    )),
            )
            .when(self.export.is_some(), |d| d.child(self.export_dialog(cx)))
    }

    /// The side panel: record, value or summary of the selection (`SPEC-0190` behaviour 6).
    fn side_panel(&self, view: SideView, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let tabs = SideView::ALL.map(|v| {
            let on = v == view;
            div()
                .id(("services-side", v as usize))
                .px_2()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .when(!on, |d| {
                    d.text_color(c(p.ui_muted)).hover(|s| s.bg(c(p.tree_hover)))
                })
                .child(v.title())
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.side = Some(v);
                    cx.notify();
                }))
        });
        let body = match view {
            SideView::Record => div()
                .flex()
                .flex_col()
                .children(self.record().into_iter().map(|(name, value)| {
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .w(px(110.))
                                .flex_none()
                                .text_ellipsis()
                                .text_color(c(p.ui_muted))
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .font(mono())
                                .child(value),
                        )
                })),
            // SPEC-0190 v4 behaviour 14: the value can be edited here; Apply keeps it pending
            // like an edit in the cell.
            SideView::Value if self.value_editing => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("services-value-field")
                        .min_h(px(120.))
                        .p_1()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c(p.focus))
                        .font(mono())
                        .child(self.value_field.clone()),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("services-value-apply")
                                .text_color(c(p.link))
                                .child(idecraft_ui::keymap::local_keys(
                                    "Apply (⌘↩)",
                                    cfg!(target_os = "macos"),
                                ))
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                    this.finish_value_edit(false, window, cx)
                                })),
                        )
                        .child(
                            div()
                                .id("services-value-cancel")
                                .text_color(c(p.ui_muted))
                                .child("Cancel (Esc)")
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                    this.finish_value_edit(true, window, cx)
                                })),
                        ),
                ),
            SideView::Value => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("services-value-edit")
                        .text_color(c(p.link))
                        .child("Edit")
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.start_value_edit(window, cx)
                        })),
                )
                .child(
                    div().flex().flex_col().font(mono()).children(
                        self.value()
                            .lines()
                            .map(|l| div().child(l.to_owned()))
                            .collect::<Vec<_>>(),
                    ),
                ),
            SideView::Summary => {
                let (scope, lines) = self.summary();
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_color(c(p.ui_muted)).child(scope))
                    .children(lines.into_iter().map(|l| div().font(mono()).child(l)))
            }
        };
        div()
            .id("services-side-panel")
            .w(px(300.))
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .children(tabs),
            )
            .child(
                div()
                    .id("services-side-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(body),
            )
    }

    /// The list of copy formats under "Copy as" (SPEC-0021 v2 behaviour 10).
    /// The rows of the shown page as columns: one line per column with its name, then the value
    /// of each row (`SPEC-0099` v2 behaviour 7). Clicking a value selects that cell.
    fn transposed_view(
        &self,
        columns: &[String],
        shown: &[(usize, String, Vec<Option<String>>)],
        numeric: &[bool],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = Palette::current();
        let refs: Vec<&[Option<String>]> = shown.iter().map(|(_, _, v)| v.as_slice()).collect();
        let lines = transpose(columns, &refs);
        let name_width =
            columns.iter().map(|n| n.chars().count()).max().unwrap_or(4) as f32 * CHAR_WIDTH + 24.;
        let widths: Vec<f32> = shown
            .iter()
            .map(|(_, number, values)| {
                let longest = values
                    .iter()
                    .map(|v| cell_text(v).chars().count())
                    .chain([number.chars().count()])
                    .max()
                    .unwrap_or(1);
                (longest as f32 * CHAR_WIDTH + 16.).min(crate::sql_console_model::MAX_COLUMN)
                    + CHAR_WIDTH
            })
            .collect();
        let total = name_width + widths.iter().sum::<f32>();
        let cursor = self.cursor();
        let cell = |w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .px_2()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .border_r_1()
                .border_color(c(p.separator))
        };
        let header = div()
            .flex()
            .h(px(ROW_HEIGHT))
            .items_center()
            .w(px(total))
            .border_b_1()
            .border_color(c(p.separator))
            .text_color(c(p.ui_dim))
            .child(cell(name_width))
            .children(
                shown
                    .iter()
                    .zip(&widths)
                    .map(|((_, number, _), w)| cell(*w).child(number.clone())),
            );
        let ids: Vec<usize> = shown.iter().map(|(r, _, _)| *r).collect();
        let rows: Vec<_> =
            lines
                .into_iter()
                .enumerate()
                .map(|(k, (name, values))| {
                    let right = numeric.get(k).copied().unwrap_or(false);
                    div()
                        .flex()
                        .h(px(ROW_HEIGHT))
                        .items_center()
                        .w(px(total))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(
                            cell(name_width)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(name),
                        )
                        .children(values.into_iter().zip(&widths).zip(&ids).map(
                            |((value, w), r)| {
                                let r = *r;
                                let here = cursor == Some((r, k));
                                cell(*w)
                                    .id(("services-tcell", r * 10_000 + k))
                                    .font(mono())
                                    .when(right, |d| d.text_right())
                                    .when(value.is_none(), |d| d.text_color(c(p.ui_dim)))
                                    .when(here, |d| {
                                        d.border_1().border_color(c(p.tab_selected_border))
                                    })
                                    .child(cell_text(&value).to_owned())
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.select_cell(r, k, cx)
                                        },
                                    ))
                            },
                        ))
                })
                .collect();
        div()
            .flex()
            .flex_col()
            .child(header)
            .children(rows)
            .into_any_element()
    }

    /// The page size menu (`SPEC-0099` v2 behaviour 5).
    fn size_menu_list(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let current = self.grids[index].size();
        div()
            .w(px(150.))
            .p_1()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(6.))
            .shadow_lg()
            .occlude()
            .child(div().px_2().text_color(c(p.ui_dim)).child("Rows per page"))
            .children(PAGE_SIZES.into_iter().enumerate().map(|(n, size)| {
                div()
                    .id(("services-size", n))
                    .px_2()
                    .rounded(px(4.))
                    .when(size == current, |d| d.bg(c(p.tree_selection)))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(page_size_label(size))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.set_page_size(size, cx)
                    }))
            }))
    }

    /// The transaction menu of a data editor: mode and isolation level (`SPEC-0190` behaviour 9,
    /// v3 behaviour 13).
    fn tx_menu_list(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let (manual, level) = self.infos[index]
            .table
            .as_ref()
            .map_or((false, idecraft_database::Isolation::Default), |t| {
                (t.manual, t.isolation)
            });
        let item = |id: (&'static str, usize), label: String, on: bool| {
            div()
                .id(id)
                .relative()
                .semantics(
                    Semantics::new(Role::MenuItem)
                        .name(label.clone())
                        .test_id(format!("{}-{}", id.0, id.1)),
                )
                .px_2()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(label)
        };
        div()
            .w(px(190.))
            .p_1()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(6.))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .px_2()
                    .text_color(c(p.ui_dim))
                    .child("Transaction mode"),
            )
            .children([false, true].into_iter().map(|m| {
                item(
                    ("services-tx-mode", m as usize),
                    if m { "Manual" } else { "Auto" }.into(),
                    m == manual,
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.tx_menu = false;
                    this.set_manual(m, cx)
                }))
            }))
            .child(
                div()
                    .mt_1()
                    .px_2()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .text_color(c(p.ui_dim))
                    .child("Isolation"),
            )
            .children(
                idecraft_database::Isolation::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(n, l)| {
                        item(("services-tx-isolation", n), l.label().into(), l == level).on_click(
                            cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.set_isolation(l, cx)
                            }),
                        )
                    }),
            )
    }

    fn copy_menu_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let current = self.copy.format;
        let header = self.copy.header;
        div()
            .absolute()
            .top(px(2.))
            .right(px(80.))
            .w(px(200.))
            .p_1()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(6.))
            .shadow_lg()
            .occlude()
            .children(ExportFormat::ALL.into_iter().enumerate().map(|(n, f)| {
                div()
                    .id(("services-format", n))
                    .relative()
                    .semantics(
                        Semantics::new(Role::MenuItem)
                            .name(f.label())
                            .test_id(format!("services-format-{}", f.label())),
                    )
                    .px_2()
                    .rounded(px(4.))
                    .when(f == current, |d| d.bg(c(p.tree_selection)))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child(f.label())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.set_copy_format(f, cx)
                    }))
            }))
            .child(
                div()
                    .id("services-header")
                    .relative()
                    .semantics(
                        Semantics::new(Role::CheckBox)
                            .name("Header row")
                            .checked(header)
                            .test_id("services-header"),
                    )
                    .mt_1()
                    .px_2()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(if header {
                        "☑ Header row"
                    } else {
                        "☐ Header row"
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.set_header(!header, cx)
                    })),
            )
    }

    /// The value filter list of `column` (`SPEC-0099` v3 behaviour 9): each value with its count
    /// over all loaded rows and a check box, "Only" next to each, "Select all" at the top.
    fn value_menu_list(&self, column: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let name = self
            .shown_index()
            .and_then(|i| self.results[i].columns.get(column).cloned())
            .unwrap_or_default();
        let values = self.column_values(column);
        let rows = values.into_iter().enumerate().map(|(n, (value, count))| {
            let checked = self.value_checked(column, &value);
            let shown = value.clone().unwrap_or_else(|| "<null>".into());
            let (toggle, only) = (value.clone(), value);
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id(("services-value", n))
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .rounded(px(4.))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .text_ellipsis()
                        .child(format!("{} {shown}", if checked { "☑" } else { "☐" }))
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.toggle_value(column, toggle.clone(), cx)
                        })),
                )
                .child(div().text_color(c(p.ui_dim)).child(count.to_string()))
                .child(
                    div()
                        .id(("services-value-only", n))
                        .px_1()
                        .text_xs()
                        .text_color(c(p.link))
                        .child("Only")
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.only_value(column, only.clone(), cx)
                        })),
                )
        });
        div()
            .id("services-value-menu")
            .absolute()
            .top(px(2.))
            .left(px(40.))
            .w(px(280.))
            .max_h(px(320.))
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(6.))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_2()
                    .pb_1()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("Values of {name}")),
                    )
                    .child(
                        div()
                            .id("services-value-all")
                            .text_xs()
                            .text_color(c(p.link))
                            .child("Select all")
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.clear_values(Some(column), cx)
                            })),
                    )
                    .child(div().id("services-value-close").ml_2().child("×").on_click(
                        cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            this.open_value_menu(None, cx)
                        }),
                    )),
            )
            .children(rows)
    }

    /// The statements submitting would run (`SPEC-0190` behaviour 7).
    fn preview_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let lines = match self.preview_sql() {
            Ok(sql) if sql.is_empty() => vec![("No pending changes".to_owned(), true)],
            Ok(sql) => sql.into_iter().map(|s| (format!("{s};"), false)).collect(),
            Err(e) => vec![(e, true)],
        };
        div()
            .id("services-preview-panel")
            .h(px(150.))
            .flex_none()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Pending statements"),
                    )
                    .child(div().flex_1())
                    .child(
                        crate::metrics::button("services-preview-submit", "Submit", true).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.submit(cx)),
                        ),
                    )
                    .child(
                        crate::metrics::button("services-preview-close", "Close", false).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.preview = false;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .id("services-preview-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3()
                    .bg(rgb(p.console_background))
                    .font(mono())
                    .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                    .children(
                        lines.into_iter().map(|(l, dim)| {
                            div().when(dim, |d| d.text_color(c(p.ui_muted))).child(l)
                        }),
                    ),
            )
    }

    /// The export dialog with its live preview (SPEC-0021 v2 behaviour 11).
    fn export_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let Some(dialog) = &self.export else {
            return div().into_any_element();
        };
        let current = self.copy.format;
        let header = self.copy.header;
        let data_editor = self
            .shown_index()
            .is_some_and(|i| self.infos[i].table.is_some());
        let choice = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .relative()
                // The header row is a switch; the others pick one of their group.
                .semantics(if id == "export-header" {
                    Semantics::new(Role::CheckBox)
                        .name(label)
                        .checked(on)
                        .test_id(id)
                } else {
                    Semantics::new(Role::RadioButton)
                        .name(label)
                        .selected(on)
                        .test_id(id)
                })
                .px_2()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(label)
                .on_click(
                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| match id {
                        "export-selection" => this.set_export_rows(ExportRows::Selection, cx),
                        "export-loaded" => this.set_export_rows(ExportRows::Loaded, cx),
                        "export-all" => this.set_export_rows(ExportRows::All, cx),
                        "export-file" => {
                            if let Some(e) = this.export.as_mut() {
                                e.to_file = true;
                            }
                            cx.notify();
                        }
                        "export-clipboard" => this.set_export_target(None, cx),
                        "export-header" => this.set_header(!header, cx),
                        _ => {}
                    }),
                )
        };
        let error = c(p.status_text(crate::palette::Status::Error));
        let mut rows = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .child(div().w(px(70.)).text_color(c(p.ui_muted)).child("Rows"))
            .child(choice(
                "export-selection",
                "Selection",
                dialog.rows == ExportRows::Selection,
                cx,
            ))
            .child(choice(
                "export-loaded",
                "Loaded rows",
                dialog.rows == ExportRows::Loaded,
                cx,
            ));
        if data_editor {
            rows = rows.child(choice(
                "export-all",
                "All rows of the table",
                dialog.rows == ExportRows::All,
                cx,
            ));
        }
        // D42: the dialog never grows past the panel. It is held between the panel's top and
        // bottom edges, the format list, the choices and the preview shrink first (and scroll), and
        // the buttons stay at the bottom (SPEC-0021 v3 and v4 behaviour 11).
        let edges = self.export_edges.clone();
        let edge = move |footer: bool| {
            let edges = edges.clone();
            gpui::canvas(
                |_, _, _| {},
                move |bounds, _, _, _| {
                    let bottom = f32::from(bounds.origin.y + bounds.size.height);
                    let (f, p) = edges.get();
                    edges.set(if footer { (bottom, p) } else { (f, bottom) });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        // SPEC-0021 v4 behaviour 11 (B1): the choices and the preview stand side by side, so the
        // preview has the full height of the dialog's middle; whatever does not fit scrolls with a
        // scroll bar beside it.
        let thumb = c(p.ui_dim).opacity(0.6);
        let dialog_box = div()
            .max_w(px(720.))
            .w_full()
            .max_h_full()
            .min_h_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Export Data"),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .id("export-formats")
                            .w(px(150.))
                            .flex_none()
                            .overflow_y_scroll()
                            .track_scroll(&self.export_scrolls[0])
                            .flex()
                            .flex_col()
                            .children(ExportFormat::ALL.into_iter().enumerate().map(|(n, f)| {
                                div()
                                    .id(("export-format", n))
                                    .relative()
                                    .semantics(
                                        Semantics::new(Role::RadioButton)
                                            .name(f.label())
                                            .selected(f == current)
                                            .test_id(format!("export-format-{}", f.label())),
                                    )
                                    .px_2()
                                    .rounded(px(4.))
                                    .when(f == current, |d| d.bg(c(p.tree_selection)))
                                    .hover(|s| s.bg(c(p.tree_hover)))
                                    .child(f.label())
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            this.set_export_format(f, cx)
                                        },
                                    ))
                            })),
                    )
                    .child(crate::scroll_bar::vertical(
                        "export-formats-scrollbar",
                        "Formats",
                        &self.export_scrolls[0],
                        thumb,
                    ))
                    .child(
                        div()
                            .id("export-choices")
                            .w(px(EXPORT_CHOICES_WIDTH))
                            .flex_none()
                            .overflow_y_scroll()
                            .track_scroll(&self.export_scrolls[1])
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(rows)
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(
                                        div().w(px(70.)).text_color(c(p.ui_muted)).child("Options"),
                                    )
                                    .child(choice(
                                        "export-header",
                                        if header {
                                            "☑ Header row"
                                        } else {
                                            "☐ Header row"
                                        },
                                        false,
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(div().w(px(70.)).text_color(c(p.ui_muted)).child("To"))
                                    .child(choice("export-file", "File", dialog.to_file, cx))
                                    .child(choice(
                                        "export-clipboard",
                                        "Clipboard",
                                        !dialog.to_file,
                                        cx,
                                    )),
                            )
                            .when(dialog.to_file, |d| {
                                d.child(
                                    crate::metrics::field_frame()
                                        .child(div().flex_1().min_w_0().child(dialog.path.clone())),
                                )
                            }),
                    )
                    .child(crate::scroll_bar::vertical(
                        "export-choices-scrollbar",
                        "Export options",
                        &self.export_scrolls[1],
                        thumb,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().flex_none().text_color(c(p.ui_muted)).child("Preview"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .child(
                                        div()
                                            .id("export-preview")
                                            .relative()
                                            .semantics(
                                                Semantics::new(Role::ScrollView)
                                                    .name("Preview")
                                                    .test_id("export-preview"),
                                            )
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_scroll()
                                            .track_scroll(&self.export_scrolls[2])
                                            .px_2()
                                            .bg(rgb(p.console_background))
                                            .font(mono())
                                            .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                                            .children(
                                                self.export_preview()
                                                    .lines()
                                                    .enumerate()
                                                    .map(|(n, l)| {
                                                        div()
                                                            .relative()
                                                            .semantics(
                                                                Semantics::new(Role::Label)
                                                                    .name(l.to_owned())
                                                                    .test_id(format!(
                                                                        "export-preview-line-{n}"
                                                                    )),
                                                            )
                                                            .whitespace_nowrap()
                                                            .child(l.to_owned())
                                                    })
                                                    .collect::<Vec<_>>(),
                                            ),
                                    )
                                    .child(crate::scroll_bar::vertical(
                                        "export-preview-scrollbar",
                                        "Preview",
                                        &self.export_scrolls[2],
                                        thumb,
                                    )),
                            ),
                    ),
            )
            .when_some(dialog.note.clone(), |d, (text, failed)| {
                d.child(
                    div()
                        .when(failed, |d| d.text_color(error))
                        .when(!failed, |d| d.text_color(c(p.ui_muted)))
                        .child(text),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_none()
                    .flex()
                    .gap_2()
                    .child(edge(true))
                    .child(div().flex_1())
                    .child(
                        crate::metrics::button("export-cancel", "Cancel", false)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Cancel")
                                    .test_id("export-cancel"),
                            )
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.close_export(cx)
                            })),
                    )
                    .child(
                        crate::metrics::button("export-run", "Export", !dialog.running)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Export")
                                    .enabled(!dialog.running)
                                    .test_id("export-run"),
                            )
                            .on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.run_export(cx)
                                }),
                            ),
                    ),
            );
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .right_0()
            .p(px(8.))
            .pl(px(40.))
            .flex()
            .flex_col()
            .items_start()
            .child(edge(false))
            .child(dialog_box)
            .into_any_element()
    }
}

/// The paging label of a data editor: "1–500 of 1200", "3 rows", "0 rows".
fn table_label(total: u64, page: usize, rows: usize, size: usize) -> String {
    let start = page.saturating_mul(size);
    if page == 0 && rows as u64 >= total {
        return if total == 1 {
            "1 row".into()
        } else {
            format!("{total} rows")
        };
    }
    if rows == 0 {
        return format!("0 of {total}");
    }
    format!("{}–{} of {total}", start + 1, start + rows)
}

impl ServicesView {
    /// The result toolbar's groups of result `index` in row order, split into those that stay in
    /// the row and those that go into the ⋮ menu for the width last painted (`SPEC-0190` v5
    /// behaviour 3).
    pub fn toolbar_groups(&self, index: usize) -> (Vec<ToolbarGroup>, Vec<ToolbarGroup>) {
        use crate::toolbar_fit::{Group, fit, text_width};
        let Some(info) = self.infos.get(index) else {
            return (Vec::new(), Vec::new());
        };
        let table = info.table.is_some();
        let editable = self.edits.get(index).is_some_and(Option::is_some);
        let mut groups = vec![ToolbarGroup::Pager, ToolbarGroup::Fields];
        groups.extend([
            ToolbarGroup::PageSize,
            ToolbarGroup::Transpose,
            ToolbarGroup::View,
        ]);
        if editable {
            groups.push(ToolbarGroup::Edits);
            if table {
                groups.push(ToolbarGroup::Tx);
            }
        }
        groups.extend([
            ToolbarGroup::CopyAs,
            ToolbarGroup::Record,
            ToolbarGroup::Export,
        ]);
        let entries_width = |g: ToolbarGroup| -> f32 {
            self.toolbar_entries(index, g)
                .iter()
                .map(|(_, l)| text_width(l))
                .sum()
        };
        let sized: Vec<Group<ToolbarGroup>> = groups
            .into_iter()
            .map(|g| {
                let width = match g {
                    // Four page buttons and the rows shown.
                    ToolbarGroup::Pager => 4.0 * 22.0 + text_width(&self.label(index)),
                    ToolbarGroup::Fields if table => (8.0 + 240.0 + 4.0) + (8.0 + 180.0 + 4.0),
                    ToolbarGroup::Fields => 8.0 + 260.0 + 4.0,
                    ToolbarGroup::Edits => 8.0 + entries_width(g),
                    ToolbarGroup::View => 4.0 + entries_width(g),
                    _ => entries_width(g),
                };
                Group {
                    key: g,
                    width,
                    gives_way: g.gives_way(),
                }
            })
            .collect();
        // The painted width includes the row's padding on both sides.
        let painted = self.toolbar_width.get();
        let available = if painted > 0.0 { painted - 16.0 } else { 0.0 };
        let available = match self.toolbar_limit {
            Some(l) if available > 0.0 => available.min(l),
            Some(l) => l,
            None => available,
        };
        fit(&sized, available)
    }

    /// The actions of toolbar group `g` of result `index` as (id, label); the ids are those of
    /// [`Self::toolbar_action`].
    fn toolbar_entries(&self, index: usize, g: ToolbarGroup) -> Vec<(&'static str, String)> {
        use idecraft_database::grid::ResultView;
        let current = self.infos.get(index).map(|i| i.view);
        match g {
            ToolbarGroup::Pager | ToolbarGroup::Fields => Vec::new(),
            ToolbarGroup::PageSize => vec![(
                "services-page-size",
                format!(
                    "Page: {} ▾",
                    page_size_label(self.grids.get(index).map_or(PAGE_SIZE, |s| s.size()))
                ),
            )],
            ToolbarGroup::Transpose => {
                vec![("services-transpose", self.transpose_label(index).into())]
            }
            ToolbarGroup::View => [
                ("services-view-table", ResultView::Table),
                ("services-view-tree", ResultView::Tree),
                ("services-view-text", ResultView::Text),
            ]
            .into_iter()
            .map(|(id, v)| {
                let l = v.label();
                (
                    id,
                    if current == Some(v) {
                        format!("{l} ✓")
                    } else {
                        l.to_string()
                    },
                )
            })
            .collect(),
            ToolbarGroup::Edits => {
                let pending = self
                    .edits
                    .get(index)
                    .and_then(Option::as_ref)
                    .map_or(0, |e| e.count());
                vec![
                    ("services-add-row", "+ Row".into()),
                    ("services-delete-row", "− Row".into()),
                    (
                        "services-submit",
                        if pending > 0 {
                            format!("Submit ({pending})")
                        } else {
                            "Submit".into()
                        },
                    ),
                    ("services-revert", "Revert".into()),
                    ("services-preview", "Preview".into()),
                ]
            }
            ToolbarGroup::Tx => {
                let Some(t) = self.infos.get(index).and_then(|i| i.table.as_ref()) else {
                    return Vec::new();
                };
                let mut v = vec![(
                    "services-tx",
                    format!("Tx: {} ▾", if t.manual { "Manual" } else { "Auto" }),
                )];
                if t.tx_open {
                    v.push(("services-commit", format!("Commit ({})", t.tx_changes)));
                    v.push(("services-rollback", "Rollback".into()));
                }
                v
            }
            ToolbarGroup::CopyAs => vec![(
                "services-copy-as",
                format!("Copy as: {} ▾", self.copy.format.label()),
            )],
            ToolbarGroup::Record => vec![("services-record", "Record".into())],
            ToolbarGroup::Export => vec![("services-export", "Export…".into())],
        }
    }

    fn transpose_label(&self, index: usize) -> &'static str {
        if self.infos.get(index).is_some_and(|i| i.transposed) {
            "☑ Transpose"
        } else {
            "☐ Transpose"
        }
    }

    /// Closes the toolbar's menus (⋮, page size, transaction, copy format).
    fn close_toolbar_menus(&mut self) {
        self.more_menu = false;
        self.size_menu = false;
        self.tx_menu = false;
        self.copy_menu = false;
    }

    /// Carries out the toolbar action `id`, from its button or from the ⋮ menu.
    pub fn toolbar_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        use idecraft_database::grid::ResultView;
        match id {
            "services-add-row" => self.add_row(cx),
            "services-delete-row" => self.toggle_delete_row(cx),
            "services-submit" => self.submit(cx),
            "services-revert" => self.revert(cx),
            "services-preview" => self.toggle_preview(cx),
            "services-record" => {
                let view = self.side.unwrap_or_default();
                self.show_side(Some(view), cx)
            }
            "services-export" => self.open_export(window, cx),
            "services-copy-as" => {
                self.copy_menu = !self.copy_menu;
                cx.notify();
            }
            "services-tx" => {
                self.tx_menu = !self.tx_menu;
                self.size_menu = false;
                cx.notify();
            }
            "services-page-size" => {
                self.size_menu = !self.size_menu;
                self.tx_menu = false;
                cx.notify();
            }
            "services-transpose" => self.toggle_transpose(cx),
            "services-commit" => self.finish_transaction(true, cx),
            "services-rollback" => self.finish_transaction(false, cx),
            "services-view-table" => self.set_result_view(ResultView::Table, cx),
            "services-view-tree" => self.set_result_view(ResultView::Tree, cx),
            "services-view-text" => self.set_result_view(ResultView::Text, cx),
            _ => cx.notify(),
        }
    }

    /// Lets the toolbar use at most `limit` pixels, as in a narrower window (window self-test of
    /// the ⋮ menu); `None` gives it its painted width again.
    pub fn set_toolbar_limit(&mut self, limit: Option<f32>, cx: &mut Context<Self>) {
        self.toolbar_limit = limit;
        self.more_menu = false;
        cx.notify();
    }

    /// [`Self::toolbar_groups`] of the shown result.
    pub fn shown_toolbar_groups(&self) -> (Vec<ToolbarGroup>, Vec<ToolbarGroup>) {
        self.shown_index()
            .map(|i| self.toolbar_groups(i))
            .unwrap_or_default()
    }

    /// Whether the toolbar's ⋮ menu is open.
    pub fn more_menu_open(&self) -> bool {
        self.more_menu
    }

    /// The Plan tab: a head that stays at the top — the switches of the four views and the hints —
    /// and below it the view that scrolls: the rows of Tree, the boxes of Diagram, the bars of
    /// Flame with the selected step's properties below, or the raw text (`SPEC-0110` behaviour 2,
    /// v5 behaviours 10–11, v13 behaviours 19–20, v14 behaviour 21).
    fn plan_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::plan_view_model::{FlameMetric, PlanMode, hint_toggle, shown_hints};
        let p = Palette::current();
        let mode = self.plan_mode;
        let warn = p.console_ansi_text[3];
        let switch = |id: String, label: &'static str, on: bool| {
            div()
                .id(gpui::SharedString::from(id.clone()))
                .px_2()
                .rounded(px(4.))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(label)
                .semantics(
                    Semantics::new(Role::Tab)
                        .name(label)
                        .test_id(id)
                        .selected(on)
                        .action(AxAction::Click)
                        .effects(&[Effect::None]),
                )
        };
        let mut switches = div().flex().flex_wrap().gap_1().px_3().pb_1();
        for m in PlanMode::ALL {
            switches = switches.child(
                switch(
                    format!("plan-{}", m.label().to_lowercase()),
                    m.label(),
                    mode == m,
                )
                .on_click(
                    cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.set_plan_mode(m, cx)),
                ),
            );
        }
        if mode == PlanMode::Flame {
            switches = switches.child(div().w(px(12.)));
            for metric in FlameMetric::ALL {
                if !self.plan.as_ref().is_some_and(|plan| metric.offered(plan)) {
                    continue;
                }
                let id = format!(
                    "plan-metric-{}",
                    metric
                        .label()
                        .split(' ')
                        .next()
                        .unwrap_or_default()
                        .to_lowercase()
                );
                switches = switches.child(
                    switch(id, metric.label(), self.flame_metric == metric).on_click(cx.listener(
                        move |this, _: &gpui::ClickEvent, _, cx| this.set_flame_metric(metric, cx),
                    )),
                );
            }
        }
        // Fitted into the area below the head, or at 100 % (v14 behaviour 21).
        if mode == PlanMode::Diagram {
            switches = switches.child(div().w(px(12.)));
            for (fit, label, id) in [
                (true, "Fit", "plan-zoom-fit"),
                (false, "100 %", "plan-zoom-100"),
            ] {
                switches = switches.child(
                    switch(id.into(), label, self.plan_diagram_fit() == fit).on_click(cx.listener(
                        move |this, _: &gpui::ClickEvent, _, cx| this.set_plan_diagram_fit(fit, cx),
                    )),
                );
            }
        }
        let plan = self.plan.as_ref();
        let hints = plan.map(|p| p.hints.clone()).unwrap_or_default();
        let open = self.plan_hints_open;
        let toggle = hint_toggle(hints.len(), open).filter(|_| mode.has_steps());
        let keys = if toggle.is_some() {
            "⌥R switches · ↑↓ step · ⌘C copies · ⌥H hints"
        } else {
            "⌥R switches · ↑↓ step · ⌘C copies"
        };
        let note = plan.filter(|p| p.analyzed).map(|p| {
            if p.rolled_back {
                "Measured; the statement's changes were rolled back"
            } else {
                "Measured"
            }
        });
        let switches = switches
            .child(div().ml_2().text_color(c(p.ui_dim)).child(keys))
            .when_some(note.filter(|_| mode.has_steps()), |d, n| {
                d.child(div().ml_2().text_color(c(p.ui_muted)).child(n))
            });
        // Several hints take one line: the first, cut at the edge, and how many more there are; a
        // click or ⌥H shows all of them, each in full on its own line, and folds them again.
        let hint_lines = mode.has_steps().then(|| {
            let shown = shown_hints(&hints, open).to_vec();
            let folded = toggle.is_some() && !open;
            div()
                .px_3()
                .text_color(rgb(warn))
                .children(shown.into_iter().enumerate().map(|(i, h)| {
                    let line = div().id(("plan-hint", i)).flex().gap_2().child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(folded, |d| {
                                d.overflow_hidden().whitespace_nowrap().text_ellipsis()
                            })
                            .child(format!("⚠ {h}")),
                    );
                    match toggle.clone().filter(|_| i == 0) {
                        Some(label) => line
                            .rounded(px(4.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_plan_hints(cx)
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(c(p.ui_muted))
                                    .child(label.clone()),
                            )
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name(format!("⚠ {h} · {label}"))
                                    .test_id("plan-hints")
                                    .expanded(open)
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            ),
                        None => line.semantics(
                            Semantics::new(Role::Label)
                                .name(format!("⚠ {h}"))
                                .test_id(format!("plan-hint-{i}")),
                        ),
                    }
                }))
        });
        let head = div()
            .flex_none()
            .pt_1()
            .pb_1()
            .border_b_1()
            .border_color(c(p.separator))
            .child(switches)
            .children(hint_lines);
        let content = if mode == PlanMode::Raw {
            let text = plan.map(|p| p.raw.clone()).unwrap_or_default();
            div()
                .py_1()
                .children(
                    text.lines()
                        .map(|l| div().px_3().whitespace_normal().child(l.to_owned())),
                )
                .into_any_element()
        } else {
            let selected_step = self.plan_step;
            let view = match (mode, plan) {
                (PlanMode::Diagram, Some(plan)) => self.plan_diagram(plan, cx).into_any_element(),
                (PlanMode::Flame, Some(plan)) => self.plan_flame(plan, cx).into_any_element(),
                _ => self.plan_tree(cx).into_any_element(),
            };
            let properties = self.plan_step_properties();
            div()
                .pb_1()
                .child(view)
                // The selected step's properties, every field by its own name (v13 behaviour 20).
                .when(selected_step.is_some() && !properties.is_empty(), |d| {
                    d.child(
                        div()
                            .id("plan-step-properties")
                            .mt_2()
                            .mx_3()
                            .pt_1()
                            .border_t_1()
                            .border_color(c(p.separator))
                            .whitespace_normal()
                            .children(properties.into_iter().map(|(name, value)| {
                                let hint = name.starts_with('⚠');
                                div()
                                    .flex()
                                    .gap_2()
                                    .when(hint, |d| d.text_color(rgb(warn)))
                                    .child(
                                        div()
                                            .flex_none()
                                            .when(!hint, |d| d.text_color(c(p.ui_muted)))
                                            .child(format!("{name}:")),
                                    )
                                    .child(div().flex_1().min_w_0().child(value))
                            })),
                    )
                })
                .into_any_element()
        };
        // The area below the head, measured as it is painted; when it changes while the diagram
        // is fitted, the next frame fits it again (as the ER diagram, SPEC-0111 v7 behaviour 8).
        let area = self.plan_area.clone();
        let refit = mode == PlanMode::Diagram && self.plan_diagram_fit();
        let measure = gpui::canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if area.get() != size {
                    area.set(size);
                    if refit {
                        window.defer(cx, |window, _| window.refresh());
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(p.console_background))
            .font(mono())
            .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
            .child(head)
            .child(
                div().relative().flex_1().min_h_0().child(measure).child(
                    div()
                        .id("services-plan")
                        .size_full()
                        .overflow_y_scroll()
                        .own_axis()
                        .child(content),
                ),
            )
            .into_any_element()
    }

    /// The Tree view: one row per step with indentation (behaviour 2).
    fn plan_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let warn = p.console_ansi_text[3];
        let selected_step = self.plan_step;
        let plan = self.plan.as_ref();
        let rows = plan
            .map(crate::db_compare_model::plan_rows)
            .unwrap_or_default();
        let hinted: Vec<bool> = plan
            .map(|p| {
                crate::plan_view_model::steps(p)
                    .iter()
                    .map(|(_, _, n)| !n.hints.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let measured = plan.is_some_and(|p| p.analyzed);
        // Hand test 43: the object column is as wide as its longest text (an index name such as
        // `PK__warehous__3213E83F…` stays whole), between 180 and 480 px.
        let object_width = crate::db_compare_model::plan_object_width(&rows, CHAR_WIDTH);
        let cell = |w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .px_2()
                .overflow_hidden()
                .whitespace_nowrap()
        };
        div()
            .flex()
            .flex_col()
            .children(rows.into_iter().enumerate().map(|(i, r)| {
                let on = selected_step == Some(i);
                let hint = hinted.get(i).copied().unwrap_or(false);
                div()
                    .id(("plan-step", i))
                    .flex()
                    .h(px(ROW_HEIGHT))
                    .items_center()
                    .px_1()
                    .when(on, |d| d.bg(c(p.tree_selection)))
                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.select_plan_step(i, cx)
                    }))
                    .child(
                        cell(260.)
                            .pl(px(crate::metrics::tree_indent(r.depth)))
                            .when(r.full_scan || hint, |d| d.text_color(rgb(warn)))
                            .child(if hint {
                                format!("⚠ {}", r.operation)
                            } else {
                                r.operation
                            }),
                    )
                    .child(cell(object_width).text_ellipsis().child(r.object))
                    .child(cell(220.).text_color(c(p.ui_muted)).child(r.estimate))
                    .when(measured, |d| d.child(cell(240.).child(r.actual)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(c(p.ui_muted))
                            .child(r.details),
                    )
            }))
    }

    /// The Diagram view: a box per step, children in the row below their parent, a line from
    /// each child up to its parent (v13 behaviour 19).
    fn plan_diagram(
        &self,
        plan: &idecraft_database::Plan,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let warn = p.console_ansi_text[3];
        let boxes = crate::plan_view_model::diagram(plan);
        // Every box as wide as its longest line, within 120–320 px; one slot per leaf. Boxes,
        // text, gaps and lines scale together with the zoom that fits the area (v14 behaviour 21).
        let (m, z) = self.plan_diagram_zoom().unwrap_or((
            crate::plan_view_model::diagram_metrics(&boxes, CHAR_WIDTH, ROW_HEIGHT),
            1.,
        ));
        let (box_w, box_h, gap, slot_gap) = (m.box_w * z, m.box_h * z, m.gap * z, m.slot_gap * z);
        let slot = box_w + slot_gap;
        let left = |x: f32| x * slot + slot_gap / 2.;
        let top = |depth: usize| depth as f32 * (box_h + gap);
        let line = |x: f32, y: f32, w: f32, h: f32| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w.max(1.)))
                .h(px(h.max(1.)))
                .bg(c(p.ui_dim))
        };
        let mut lines = Vec::new();
        for b in &boxes {
            let Some(parent) = b.parent.and_then(|i| boxes.get(i)) else {
                continue;
            };
            let child_x = left(b.x) + box_w / 2.;
            let parent_x = left(parent.x) + box_w / 2.;
            let child_top = top(b.depth);
            let parent_bottom = top(parent.depth) + box_h;
            let mid = parent_bottom + gap / 2.;
            lines.push(line(child_x, mid, 1., child_top - mid));
            lines.push(line(
                child_x.min(parent_x),
                mid,
                (child_x - parent_x).abs() + 1.,
                1.,
            ));
            lines.push(line(parent_x, parent_bottom, 1., mid - parent_bottom));
        }
        let selected = self.plan_step;
        // Opened and wider than its area at the readable zoom: it starts with the top step in the
        // middle (v15 behaviour 22); once, after the layout that knows the area.
        let (w, _) = self.plan_area.get();
        if self.diagram_zoom == crate::plan_view_model::DiagramZoom::Readable
            && w > 0.
            && self.plan_diagram_align.get()
            && let Some(top) = boxes.iter().find(|b| b.parent.is_none())
        {
            let x =
                crate::plan_view_model::top_step_offset(&m, z, top.x, w - DIAGRAM_MARGIN.0 * 2.);
            self.plan_diagram_align.set(false);
            self.plan_diagram_scroll
                .set_offset(gpui::point(px(-x), px(0.)));
        }
        div()
            .id("plan-diagram")
            .mx(px(DIAGRAM_MARGIN.0))
            .my(px(DIAGRAM_MARGIN.1))
            .overflow_x_scroll()
            .track_scroll(&self.plan_diagram_scroll)
            .own_axis()
            .child(
                div()
                    .relative()
                    .w(px(m.width * z))
                    .h(px(m.height * z))
                    .text_size(px(crate::fonts::EDITOR_FONT_SIZE * z))
                    .line_height(px(ROW_HEIGHT * z))
                    .children(lines)
                    .children(boxes.into_iter().map(|b| {
                        let on = selected == Some(b.step);
                        let step = b.step;
                        div()
                            .id(("plan-box", b.step))
                            .absolute()
                            .left(px(left(b.x)))
                            .top(px(top(b.depth)))
                            .w(px(box_w))
                            .h(px(box_h))
                            .px(px(8. * z))
                            .py(px(4. * z))
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .rounded(px(4. * z))
                            .border_1()
                            .border_color(if b.hint {
                                rgb(warn).into()
                            } else {
                                c(p.separator)
                            })
                            .bg(if on { c(p.tree_selection) } else { c(p.island) })
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.select_plan_step(step, cx)
                            }))
                            .child(
                                div()
                                    .text_ellipsis()
                                    .when(b.hint, |d| d.text_color(rgb(warn)))
                                    .child(if b.hint {
                                        format!("⚠ {}", b.operation)
                                    } else {
                                        b.operation.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .text_ellipsis()
                                    .text_color(c(p.ui_muted))
                                    .child(b.object.clone()),
                            )
                            .child(div().text_color(c(p.ui_muted)).child(b.share.clone()))
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name(format!("{} {}", b.operation, b.object).trim().to_owned())
                                    .test_id(format!("plan-box-{}", b.step))
                                    .selected(on)
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                    })),
            )
    }

    /// The Flame view: a bar per step across its share of the parent, by the chosen metric
    /// (v13 behaviour 19).
    fn plan_flame(
        &self,
        plan: &idecraft_database::Plan,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::current();
        let warn = p.console_ansi_text[3];
        let bars = crate::plan_view_model::flame(plan, self.flame_metric);
        let row = ROW_HEIGHT + 4.;
        let depth = bars.iter().map(|b| b.depth + 1).max().unwrap_or(0);
        let selected = self.plan_step;
        div()
            .id("plan-flame")
            .relative()
            .mx_3()
            .my_1()
            .h(px(depth as f32 * row))
            .children(bars.into_iter().map(|b| {
                let on = selected == Some(b.step);
                let step = b.step;
                div()
                    .id(("plan-bar", b.step))
                    .absolute()
                    .left(gpui::relative(b.start as f32))
                    .w(gpui::relative(b.width as f32))
                    .top(px(b.depth as f32 * row))
                    .h(px(row - 2.))
                    .px_1()
                    // Long labels are cut at the bar's edge, as in the profiler's flame graph. A bar
                    // is as wide as a part of the view, and text that ends in "…" is measured
                    // before that width is known: every label showed only "…" (window check of
                    // the plan views, 2026-11-16). The full name is in the properties below.
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(if b.hint {
                        rgb(warn).into()
                    } else {
                        c(p.separator)
                    })
                    .bg(if on { c(p.tree_selection) } else { c(p.island) })
                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                    .when(b.hint, |d| d.text_color(rgb(warn)))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.select_plan_step(step, cx)
                    }))
                    .child(if b.hint {
                        format!("⚠ {}", b.label)
                    } else {
                        b.label.clone()
                    })
                    .semantics(
                        Semantics::new(Role::Button)
                            .name(b.label.clone())
                            .test_id(format!("plan-bar-{}", b.step))
                            .selected(on)
                            .action(AxAction::Click)
                            .effects(&[Effect::None]),
                    )
            }))
    }

    /// Changes the statement choice of the Schema Diff tab with `f`, then rewrites the script and
    /// its file (`SPEC-0110` v2 behaviour 7).
    pub fn change_migration(
        &mut self,
        f: impl FnOnce(&mut crate::db_compare_model::MigrationChoice),
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.schema_diff.as_mut() else {
            return;
        };
        let Some(choice) = tab.choice.as_mut() else {
            return;
        };
        let asked = choice.confirmation().is_some();
        f(choice);
        // A new question is scrolled to the top of the tab, so it is read before it is answered.
        if !asked && choice.confirmation().is_some() {
            let header = 1;
            let none = usize::from(tab.lines.is_empty());
            let spacer = 1;
            self.schema_diff_scroll.scroll_to_top_of_item(
                header + none + tab.lines.len() + spacer + choice.statements.len(),
            );
        }
        tab.script = choice.script();
        if let Some(file) = &tab.file {
            let _ = std::fs::write(file, &tab.script);
        }
        cx.notify();
    }

    /// Whether the Schema Diff tab asks to confirm destructive statements (SPEC-0110 v3).
    pub fn confirming_migration(&self) -> bool {
        self.schema_diff
            .as_ref()
            .and_then(|t| t.choice.as_ref())
            .is_some_and(|c| c.confirmation().is_some())
    }

    /// Whether the Schema Diff tab is the one shown.
    fn schema_diff_shown(&self) -> bool {
        self.tab == self.results.len() + 1 && self.extra_tab() == Some("Schema Diff")
    }

    /// Brings the Output tab to the front, as a click on it does (the window self-test).
    pub fn show_output(&mut self, cx: &mut Context<Self>) {
        self.select_tab(0, cx);
        cx.notify();
    }

    /// Brings the Schema Diff tab to the front, if there is one (`SPEC-0110` v4 behaviour 7).
    pub fn show_schema_diff(&mut self, cx: &mut Context<Self>) {
        if self.extra_tab() == Some("Schema Diff") && !self.schema_diff_shown() {
            self.select_tab(self.results.len() + 1, cx);
        }
    }

    /// The Schema Diff tab: the differences, the statements to choose from, then the migration
    /// script (`SPEC-0110` behaviours 4 and 7).
    fn schema_diff_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::db_compare_model::Mark;
        let p = Palette::current();
        let tab = self.schema_diff.clone().unwrap_or_default();
        let colour = |m: Mark| match m {
            Mark::Added => p.console_ansi_text[2],
            Mark::Removed => p.console_ansi_text[1],
            Mark::Changed => p.console_ansi_text[4],
        };
        div()
            .id("services-schema-diff")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.schema_diff_scroll)
            .px_3()
            .py_1()
            .bg(rgb(p.console_background))
            .font(mono())
            .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
            // The direction with Swap and Open Script (v5 behaviour 13).
            .when_some(
                tab.choice
                    .as_ref()
                    .map(|c| (c.source.clone(), c.target.clone())),
                |d, (source, target)| {
                    let action = |id: &'static str,
                                  label: &'static str,
                                  action: &'static str,
                                  effect: Effect| {
                        div()
                            .id(id)
                            .px_2()
                            .rounded(px(4.))
                            .flex_none()
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(label)
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name(label)
                                    .test_id(id)
                                    .action(AxAction::Click)
                                    .effects(&[effect]),
                            )
                            .on_click(cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(ServicesEvent::Action(action))
                            }))
                    };
                    d.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .pb_1()
                            .child(
                                div()
                                    .text_color(c(p.ui_muted))
                                    .child(format!("`{target}` becomes like `{source}`")),
                            )
                            .child(action(
                                "schema-diff-swap",
                                "Swap",
                                "Sql.SwapComparison",
                                Effect::Write,
                            ))
                            .child(action(
                                "schema-diff-open-script",
                                "Open Script",
                                "Sql.OpenMigrationScript",
                                Effect::None,
                            )),
                    )
                },
            )
            .when(tab.lines.is_empty(), |d| {
                d.child(div().text_color(c(p.ui_muted)).child("No differences"))
            })
            .children(tab.lines.into_iter().map(|l| {
                div()
                    .pl(px(16. * l.depth as f32))
                    .text_color(rgb(colour(l.mark)))
                    .child(format!("{} {}", l.mark.sign(), l.text))
            }))
            .child(div().h(px(ROW_HEIGHT)))
            .when_some(tab.choice.clone(), |d, choice| {
                let confirmation = choice.confirmation();
                let mark = self.statement_mark;
                d.children(choice.statements.iter().enumerate().map(|(i, st)| {
                    let on = choice.selected[i];
                    let first = st.sql.lines().next().unwrap_or_default().to_owned();
                    let marked = mark == Some(i);
                    div()
                        .id(("migration-statement", i))
                        .flex()
                        .gap_2()
                        .whitespace_nowrap()
                        .when(marked, |d| d.bg(c(p.tree_selection)))
                        .when(!marked, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .semantics(
                            Semantics::new(Role::ListItem)
                                .name(first.clone())
                                .test_id(format!("migration-statement-{i}"))
                                .checked(on)
                                .selected(marked)
                                .action(AxAction::Click)
                                .effects(&[Effect::Write]),
                        )
                        .child(if on { "[x]" } else { "[ ]" })
                        .child(div().overflow_hidden().text_ellipsis().child(first))
                        .when(st.destructive.is_some(), |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .text_color(rgb(p.console_ansi_text[1]))
                                    .child("data loss"),
                            )
                        })
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.change_migration(
                                |c| {
                                    c.click(i);
                                },
                                cx,
                            );
                        }))
                }))
                .when_some(confirmation.clone(), |d, text| {
                    // D44: the question wraps and the buttons sit on their own line below it, so
                    // they stay reachable however long the list of losses is (SPEC-0110 v3).
                    d.child(
                        div()
                            .id("migration-confirmation")
                            .relative()
                            .semantics(
                                crate::semantics::Semantics::new(crate::semantics::Role::Label)
                                    .name("Data loss question")
                                    .test_id("migration-confirmation"),
                            )
                            .w_full()
                            .min_w_0()
                            .whitespace_normal()
                            .text_color(rgb(p.console_ansi_text[1]))
                            .child(text),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .id("migration-include")
                                    .relative()
                                    .semantics(
                                        crate::semantics::Semantics::new(
                                            crate::semantics::Role::Button,
                                        )
                                        .name("Include")
                                        .test_id("migration-include")
                                        .action(crate::semantics::AxAction::Click)
                                        .effects(&[crate::semantics::Effect::None]),
                                    )
                                    .px_2()
                                    .rounded(px(4.))
                                    .flex_none()
                                    .bg(c(p.tree_selection))
                                    .child("Include (Enter)")
                                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.change_migration(
                                            |c| {
                                                c.confirm();
                                            },
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                div()
                                    .id("migration-cancel")
                                    .relative()
                                    .semantics(
                                        crate::semantics::Semantics::new(
                                            crate::semantics::Role::Button,
                                        )
                                        .name("Cancel")
                                        .test_id("migration-cancel")
                                        .action(crate::semantics::AxAction::Click)
                                        .effects(&[crate::semantics::Effect::None]),
                                    )
                                    .px_2()
                                    .rounded(px(4.))
                                    .flex_none()
                                    .hover(|s| s.bg(c(p.tree_hover)))
                                    .child("Cancel (Esc)")
                                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.change_migration(|c| c.cancel(), cx);
                                    })),
                            ),
                    )
                })
                .child(div().h(px(ROW_HEIGHT)))
            })
            .children(tab.script.lines().map(|l| {
                div()
                    .when(l.starts_with("--"), |d| d.text_color(c(p.ui_muted)))
                    .child(l.to_owned())
            }))
    }
}

/// Scrolling only along an element's own axis (`SPEC-0110` v14 behaviour 21).
trait OwnAxis: Styled + Sized {
    /// Lets the element scroll only along the axis it scrolls on: in the Plan tab a wheel turn
    /// down scrolls the tab and never the diagram sideways, a sideways swipe scrolls the diagram
    /// and never the tab.
    fn own_axis(mut self) -> Self {
        self.style().restrict_scroll_to_axis = Some(true);
        self
    }
}

impl<E: Styled> OwnAxis for E {}

fn mono() -> gpui::Font {
    let mut mono = font(crate::fonts::editor_family());
    mono.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
        if cfg!(target_os = "macos") {
            "Menlo".into()
        } else {
            "DejaVu Sans Mono".into()
        },
    ]));
    mono
}

impl Render for ServicesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No tooltip next to an open toolbar menu (SPEC-0294 v4 behaviour 11).
        if self.more_menu || self.size_menu || self.tx_menu || self.copy_menu {
            idecraft_ui::tooltip::hold_this_frame();
        }
        let p = Palette::current();
        let extra = self.tab == self.results.len() + 1;
        let response = self.response.clone().filter(|_| extra);
        let body = match (self.shown_index(), response) {
            (None, _) if self.tab == PORTS_TAB => self.ports_body(cx).into_any_element(),
            (Some(i), _) => self.grid(i, cx).into_any_element(),
            (None, None) if extra && self.plan.is_some() => self.plan_body(cx).into_any_element(),
            (None, None) if extra && self.schema_diff.is_some() => {
                self.schema_diff_body(cx).into_any_element()
            }
            (None, Some(text)) => div()
                .id("services-response")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_3()
                .py_1()
                .bg(rgb(p.console_background))
                .font(mono())
                .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                .children(text.lines().map(|l| div().child(l.to_owned())))
                .into_any_element(),
            (None, None) => div()
                .id("services-output")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_3()
                .py_1()
                .bg(rgb(p.console_background))
                .font(mono())
                .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                .when(self.output.is_empty(), |d| {
                    d.child(
                        div()
                            .text_color(c(p.ui_muted))
                            .child("Run SQL or an HTTP request from its file with ⌘↩ (Ctrl+Enter)"),
                    )
                })
                .children(self.output.iter().map(|(line, failed)| {
                    div()
                        .when(*failed, |d| d.text_color(rgb(p.console_ansi_text[1])))
                        .child(line.clone())
                }))
                .into_any_element(),
        };
        div()
            .id("services-panel")
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_cell_key))
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            // Title, tabs and labels are in the shared header (SPEC-0061 v5 behaviour 18).
            .child(body)
    }
}

impl ServicesView {
    /// The Ports tab: one row per port with its buttons (`SPEC-0318` v5 behaviour 42).
    fn ports_body(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = Palette::current();
        let (host, rows) = self.ports.clone().unwrap_or_default();
        let button = |id: (&'static str, usize), text: &'static str| {
            div()
                .id(id)
                .ml_2()
                .px_2()
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(text)
        };
        let empty = rows.is_empty();
        div()
            .id("services-ports")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_3()
            .py_1()
            .when(empty, |d| {
                d.child(
                    div()
                        .text_color(c(p.ui_muted))
                        .child(format!("No program started on {host} listens on a port")),
                )
            })
            .children(rows.into_iter().enumerate().map(|(i, row)| {
                let port = row.port;
                let text = match (&row.forward, row.kind.as_str()) {
                    (Some((_, local)), _) => format!("Port {port} → 127.0.0.1:{local}"),
                    (None, "debug") => {
                        format!(
                            "Port {port} — debug port, reached by the debugger through the session"
                        )
                    }
                    (None, kind) => format!("Port {port} — {kind} — {}", row.command),
                };
                let mut line = div().flex().items_center().py(px(2.)).child(text);
                match row.forward {
                    Some((id, local)) => {
                        line = line
                            .child(
                                button(("services-port-open", i), "Open in Browser").on_click(
                                    cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                        cx.emit(ServicesEvent::OpenForward(local));
                                    }),
                                ),
                            )
                            .child(
                                button(("services-port-stop", i), "Stop Forwarding").on_click(
                                    cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                        cx.emit(ServicesEvent::StopForward(id.clone()));
                                    }),
                                ),
                            );
                    }
                    None if row.kind != "debug" => {
                        line =
                            line.child(button(("services-port-forward", i), "Forward").on_click(
                                cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                    cx.emit(ServicesEvent::ForwardPort(port));
                                }),
                            ));
                    }
                    None => {}
                }
                line
            }))
    }

    /// The content tabs for the shared header: "Output", a tab per result with pin and ×, the extra
    /// tab (SPEC-0061 v5 behaviour 18, SPEC-0190 behaviour 1).
    pub fn header_tabs(&mut self, focused: bool, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        let selected = self.tab;
        let tab = |i: usize, cx: &mut Context<Self>| {
            crate::metrics::header_tab(("services-tab", i), selected == i, focused).on_click(
                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.select_tab(i, cx);
                }),
            )
        };
        let mut tabs = vec![tab(0, cx).child("Output").into_any_element()];
        for i in 0..self.results.len() {
            let info = &self.infos[i];
            let title = if info.pinned {
                format!("📌 {}", info.title)
            } else {
                info.title.clone()
            };
            tabs.push(
                tab(i + 1, cx)
                    .child(title)
                    .when(info.table.is_none(), |d| {
                        d.child(
                            div()
                                .id(("services-pin", i))
                                .tip(declared!(SERVICES_TOOLBAR, "services-pin"))
                                .px(px(2.))
                                .text_color(c(p.ui_dim))
                                .hover(|s| s.text_color(c(p.ui_muted)))
                                .child(if info.pinned { "◆" } else { "◇" })
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.infos[i].pinned = !this.infos[i].pinned;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .id(("services-close", i))
                            .tip(declared!(SERVICES_TOOLBAR, "services-close"))
                            .px(px(2.))
                            .text_color(c(p.ui_dim))
                            .hover(|s| s.text_color(c(p.ui_muted)))
                            .child("×")
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.close_result(i, cx)
                            })),
                    )
                    .into_any_element(),
            );
        }
        if let Some(title) = self.extra_tab() {
            tabs.push(
                tab(self.results.len() + 1, cx)
                    .child(title)
                    .into_any_element(),
            );
        }
        if self.ports.is_some() {
            tabs.push(tab(PORTS_TAB, cx).child("Ports").into_any_element());
        }
        tabs
    }

    /// The connection, "Running…" and the environment chooser before Options in the shared header
    /// (SPEC-0061 v5 behaviour 18).
    pub fn header_toolbar(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        let mut out = Vec::new();
        if !self.connection.is_empty() {
            out.push(
                div()
                    .mr_2()
                    .text_color(c(p.ui_muted))
                    .child(self.connection.clone())
                    .into_any_element(),
            );
        }
        if self.running {
            out.push(
                div()
                    .mr_2()
                    .text_color(c(p.ui_muted))
                    .child("Running…")
                    .into_any_element(),
            );
        }
        // SPEC-0102 v5 behaviour 3: once a request file with environments ran or asked for one,
        // "Env: … ▾" opens the list to choose from.
        if !self.environments.is_empty() {
            let env = self
                .environment
                .clone()
                .unwrap_or_else(|| "none".to_owned());
            let open = self.environment_list;
            let button = div()
                .id("services-environment")
                .mr_1()
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(p.separator))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(format!("Env: {env} ▾"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.environment_list = !this.environment_list;
                    cx.notify();
                }))
                .semantics(
                    crate::semantics::Semantics::new(crate::semantics::Role::Button)
                        .name(format!("Environment {env}"))
                        .test_id("services-environment")
                        .expanded(open)
                        .action(crate::semantics::AxAction::Click)
                        .effects(&[crate::semantics::Effect::None]),
                );
            let list = open.then(|| self.environment_menu(cx));
            out.push(
                div()
                    .relative()
                    .flex_none()
                    .child(button)
                    .when_some(list, |d, list| d.child(drop_down(px(24.), true, list)))
                    .into_any_element(),
            );
        }
        out
    }
}
/// A list a toolbar button opens (`SPEC-0190` v6 behaviour 16): drawn above the window, `top`
/// below the button's top, aligned with its right (`right`) or left edge, and moved in when it
/// would reach past the window's edge, so it lies wholly inside the window.
fn drop_down(top: gpui::Pixels, right: bool, list: impl IntoElement) -> impl IntoElement {
    let at = div().absolute().top(top);
    let (at, corner) = if right {
        (at.right_0(), gpui::Corner::TopRight)
    } else {
        (at.left_0(), gpui::Corner::TopLeft)
    };
    gpui::deferred(
        at.child(
            gpui::anchored()
                .anchor(corner)
                .snap_to_window_with_margin(px(4.))
                .child(list),
        ),
    )
}
