//! The Run tool window and the run configuration chooser (`SPEC-0087`).

use crate::semantics::{Annotated, Role, Semantics};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    Context, EventEmitter, FocusHandle, KeyDownEvent, UniformListScrollHandle, Window, div, font,
    prelude::*, px, rgb, uniform_list,
};
use idecraft_core::{
    RunConfig, RunConfigs, RunEvent, RunHandle, TestReport, TestStatus, start_run,
};

use crate::icon_images::icon;
use crate::palette::Palette;
use crate::run_list_model::{
    Choice, FilterKey, ListInput, ListRow, NOTHING_MATCHES, RowAction, RowKind, choice,
    escape_clears, filtered, menu, row_for_digit, rows as list_rows, typed,
};
use crate::run_model::{Console, LineKind, command_line};
use crate::test_tree_model::{
    Row as TestRow, RowPath, all_suites, rows as test_rows, status_line, test_at, test_details,
    test_source,
};
use crate::tips::{Tipped, declared};
use crate::workbench_view::c;
use idecraft_ui::tooltip::Tip;

/// How often running consoles take the new output.
const POLL: Duration = Duration::from_millis(50);

/// What the tool window asks the workbench to do.
pub enum RunViewEvent {
    /// Hide the tool window.
    Hide,
    /// A secondary click on the console or the test tree: open the context menu at this window
    /// position (`SPEC-0313` behaviour 4).
    ContextMenu(gpui::Point<gpui::Pixels>),
    /// Rerun this configuration (`SPEC-0087` v6 behaviour 12; a running one is stopped first).
    Rerun(String),
    /// Open this configuration in the configuration editor (behaviour 12, `SPEC-0327`).
    OpenConfiguration(String),
    /// Run only the tests of this configuration that failed in this report (`SPEC-0088` v3
    /// behaviour 7).
    RerunFailed(String, TestReport),
    /// Open this file with the caret at the start of this 0-based line: the source of a test
    /// (`SPEC-0088` v5 behaviour 12).
    OpenSource(PathBuf, usize),
    /// Say this in the status bar.
    Status(String),
}

/// What the test results tree of a tab shows, for checks.
pub struct TestResults {
    /// The summary line.
    pub summary: String,
    /// Visible rows: depth and name.
    pub rows: Vec<(usize, String)>,
    /// Details of the selected test, if a test is selected.
    pub details: Option<String>,
}

/// One configuration's tab; a configuration that allows several instances has one tab per
/// instance (`SPEC-0047` v3 behaviour 10).
struct Tab {
    /// The tab's title: the configuration's name, for a further instance with a number.
    name: String,
    /// The configuration it runs.
    config: String,
    console: Console,
    handle: Option<RunHandle>,
    /// The command line of the last step that started, for Copy Command Line (v6 behaviour 12).
    command: Option<Vec<String>>,
    /// The test results of the last run, for a test configuration (SPEC-0088).
    tests: Option<TestTree>,
}

/// The test results tree of a tab (SPEC-0088).
struct TestTree {
    /// What the tree shows: the run's or a remembered report, in the chosen order.
    report: TestReport,
    /// The last run's report, its top node named after what ran (v3 behaviour 10).
    current: TestReport,
    expanded: HashSet<RowPath>,
    show_passed: bool,
    selected: usize,
    /// The order of the tree (v3 behaviour 8).
    sort: crate::test_tree_model::Sort,
    /// The configuration's last runs, newest first, this one included (v3 behaviour 9).
    history: Vec<idecraft_core::HistoryEntry>,
    /// The remembered run shown instead of the last one.
    shown: Option<usize>,
    /// The history list is open.
    history_open: bool,
}

impl TestTree {
    fn new(report: TestReport, config: &str) -> Self {
        let current = crate::test_tree_model::named_after(&report, config);
        let expanded = all_suites(&current);
        let mut tree = Self {
            report: current.clone(),
            current,
            expanded,
            show_passed: true,
            selected: 0,
            sort: crate::test_tree_model::Sort::Report,
            history: Vec::new(),
            shown: None,
            history_open: false,
        };
        // The first failed test is selected, as after a failing run in the reference.
        tree.selected = tree
            .rows()
            .iter()
            .position(|r| r.is_test && matches!(r.status, TestStatus::Failed | TestStatus::Error))
            .unwrap_or(0);
        tree
    }

    fn rows(&self) -> Vec<TestRow> {
        test_rows(&self.report, &self.expanded, self.show_passed)
    }

    /// Shows the last run (`None`) or remembered run `shown` in the current order, every suite
    /// open.
    fn show(&mut self, shown: Option<usize>) {
        self.shown = shown.filter(|i| *i < self.history.len());
        let base = match self.shown {
            Some(i) => &self.history[i].report,
            None => &self.current,
        };
        self.report = crate::test_tree_model::sorted(base, self.sort);
        self.expanded = all_suites(&self.report);
        self.selected = 0;
    }

    /// The toolbar's sort button: the next order (v3 behaviour 8).
    fn next_sort(&mut self) {
        self.sort = self.sort.next();
        self.show(self.shown);
    }

    /// Expand All and Collapse All (v3 behaviour 8): every suite, or only the root, open.
    fn expand_all(&mut self, open: bool) {
        self.expanded = if open {
            all_suites(&self.report)
        } else {
            std::iter::once(Vec::new()).collect()
        };
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }

    /// The details of the selected test, if a test is selected.
    fn details(&self) -> Option<String> {
        let row = self.rows().into_iter().nth(self.selected)?;
        row.is_test
            .then(|| test_at(&self.report, &row.path).map(test_details))
            .flatten()
    }

    fn nav_rows(&self) -> Vec<crate::tree_nav::NavRow> {
        self.rows()
            .iter()
            .map(|r| crate::tree_nav::NavRow::new(r.depth, r.has_children, r.expanded))
            .collect()
    }

    /// Carries out a command of the shared tree behaviour (`SPEC-0275` behaviours 8–11); opening
    /// a test returns its path, for the jump to its source (`SPEC-0088` v5 behaviour 12).
    fn apply(&mut self, command: crate::tree_nav::TreeCommand) -> Option<RowPath> {
        use crate::tree_nav::TreeCommand as C;
        let rows = self.rows();
        match command {
            C::Select(i) => self.selected = i.min(rows.len().saturating_sub(1)),
            C::Open(i) => {
                self.selected = i.min(rows.len().saturating_sub(1));
                return rows
                    .get(self.selected)
                    .filter(|r| r.is_test)
                    .map(|r| r.path.clone());
            }
            C::Expand(i) => {
                if let Some(r) = rows.get(i) {
                    self.expanded.insert(r.path.clone());
                }
            }
            // Everything below the row (SPEC-0275 v13 behaviour 32).
            C::ExpandFully(i) => {
                crate::tree_nav::fully_expand(self, i, Self::nav_rows, |t, row| {
                    t.apply(C::Expand(row));
                });
            }
            C::Collapse(i) => {
                if let Some(r) = rows.get(i) {
                    self.expanded.remove(&r.path);
                }
            }
            C::None => {}
        }
        None
    }

    /// A key of the tree: `None` when the tree has no use for it, else the test to open, if any.
    fn key(&mut self, key: &str) -> Option<Option<RowPath>> {
        let key = crate::tree_nav::tree_key(key, false, false)?;
        Some(self.nav_key(key))
    }

    fn nav_key(&mut self, key: crate::tree_nav::TreeKey) -> Option<RowPath> {
        let command = crate::tree_nav::on_key(&self.nav_rows(), self.selected, key);
        self.apply(command)
    }

    /// A click on row `i`; returns the test a double click opens.
    fn click(&mut self, i: usize, on_arrow: bool, clicks: usize) -> Option<RowPath> {
        let mut open = None;
        for c in crate::tree_nav::on_click(&self.nav_rows(), i, on_arrow, clicks) {
            if let Some(path) = self.apply(c) {
                open = Some(path);
            }
        }
        open
    }

    /// The lines the console shows for the selected test, if a test is selected.
    fn detail_lines(&self) -> Option<usize> {
        self.details().map(|d| d.lines().count())
    }
}

/// Remembers a finished test run of `config` in the settings folder and returns the history, or
/// only this run when it cannot be written (`SPEC-0088` v3 behaviour 9).
fn remember(root: &Path, config: &str, report: &TestReport) -> Vec<idecraft_core::HistoryEntry> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let this = || {
        vec![idecraft_core::HistoryEntry {
            time,
            config: config.to_owned(),
            report: idecraft_core::tree_only(report),
        }]
    };
    let Some(settings) = idecraft_core::user_config_dir() else {
        return this();
    };
    let dir = idecraft_core::history_dir(&settings);
    idecraft_core::record_history(&dir, root, config, report, time).unwrap_or_else(|_| this())
}

/// The Run tool window: one tab per configuration that was run (behaviour 6).
pub struct RunView {
    root: PathBuf,
    tabs: Vec<Tab>,
    selected: usize,
    polling: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// When the tabs were last published for `run_output` (`SPEC-0313` behaviour 9).
    published: Option<std::time::Instant>,
    /// Scroll to End: new lines scroll the console down (`SPEC-0087` v6 behaviour 12).
    follow: bool,
    /// Long lines wrap (v6 behaviour 12).
    soft_wrap: bool,
    /// The ⋮ menu is open.
    menu_open: Option<gpui::Point<gpui::Pixels>>,
}

impl EventEmitter<RunViewEvent> for RunView {}

impl RunView {
    /// An empty tool window for the workspace at `root`.
    pub fn new(root: &Path, cx: &mut Context<Self>) -> Self {
        Self {
            root: root.to_path_buf(),
            tabs: Vec::new(),
            selected: 0,
            polling: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            published: None,
            follow: true,
            soft_wrap: false,
            menu_open: None,
        }
    }

    fn tab_index(&mut self, name: &str) -> usize {
        match self.tabs.iter().position(|t| t.name == name) {
            Some(i) => i,
            None => {
                self.tabs.push(Tab {
                    name: name.to_owned(),
                    config: name.to_owned(),
                    console: Console::default(),
                    handle: None,
                    command: None,
                    tests: None,
                });
                self.tabs.len() - 1
            }
        }
    }

    /// Shows in the tab of configuration `name` why it could not start (`SPEC-0087` v6
    /// behaviour 14, IMP-0394); a run of it that still goes is left alone.
    pub fn refuse(&mut self, name: &str, reason: &str, cx: &mut Context<Self>) {
        let i = self.tab_index(name);
        let tab = &mut self.tabs[i];
        if tab.handle.is_none() {
            tab.console = Console::default();
        }
        tab.console.push(
            LineKind::Error,
            crate::run_model::cannot_start(name, reason),
        );
        self.selected = i;
        self.publish(true);
        cx.notify();
    }

    /// Name of the selected tab's configuration.
    pub fn selected_name(&self) -> Option<String> {
        self.tabs.get(self.selected).map(|t| t.config.clone())
    }

    /// Title of the selected tab: its configuration's name, for a further instance with a number.
    pub fn selected_title(&self) -> Option<String> {
        self.tabs.get(self.selected).map(|t| t.name.clone())
    }

    /// Title and configuration of every tab, in tab order (Rerun Tests, `SPEC-0088` v4
    /// behaviour 11).
    pub fn tabs(&self) -> Vec<(String, String)> {
        self.tabs
            .iter()
            .map(|t| (t.name.clone(), t.config.clone()))
            .collect()
    }

    /// Selects the tab titled `title`, if there is one.
    pub fn select_title(&mut self, title: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.tabs.iter().position(|t| t.name == title) {
            self.selected = i;
            self.publish(true);
            cx.notify();
        }
    }

    /// How many instances of configuration `config` run in tabs (`SPEC-0047` v3 behaviour 10).
    pub fn running_instances(&self, config: &str) -> usize {
        self.tabs
            .iter()
            .filter(|t| t.config == config && t.console.running)
            .count()
    }

    /// Starts a further instance of configuration `name` beside those that run, in a tab of its
    /// own (`SPEC-0047` v3 behaviour 10): the first of `name`, `name (2)`, … whose run does not
    /// go any more. Returns the tab's title.
    pub fn run_instance(
        &mut self,
        configs: &RunConfigs,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let busy: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.console.running)
            .map(|t| t.name.clone())
            .collect();
        let title = idecraft_core::instance_title(name, &busy);
        self.run_in_tab(configs, name, &title, cx)?;
        Ok(title)
    }

    /// Runs configuration `name` in the tab titled `title`, stopping the run that still goes
    /// there (Rerun of one instance's tab).
    pub fn run_in_tab(
        &mut self,
        configs: &RunConfigs,
        name: &str,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let handle = start_run(&self.root, configs, name)?;
        self.run_handle(title, handle, cx);
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.name == title) {
            tab.config = name.to_owned();
        }
        Ok(())
    }

    /// Rerun of the selected tab (⌘R, the toolbar's Rerun): asks the workbench to run its
    /// configuration again, which stops a run that still goes first (v6 behaviour 12).
    pub fn rerun_selected(&mut self, cx: &mut Context<Self>) -> bool {
        match self.selected_name() {
            Some(name) => {
                cx.emit(RunViewEvent::Rerun(name));
                true
            }
            None => false,
        }
    }

    /// Clear: empties the selected tab's console, the run goes on (v6 behaviour 12).
    pub fn clear_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.tabs.get_mut(self.selected) {
            t.console.clear();
            self.publish(true);
            cx.notify();
        }
    }

    /// Soft-Wrap on or off (v6 behaviour 12).
    pub fn toggle_soft_wrap(&mut self, cx: &mut Context<Self>) {
        self.soft_wrap = !self.soft_wrap;
        cx.notify();
    }

    /// Whether long lines wrap.
    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    /// Scroll to End on or off; switching it on scrolls down at once (v6 behaviour 12).
    pub fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        self.follow = !self.follow;
        if self.follow {
            self.scroll_to_end();
        }
        cx.notify();
    }

    /// Whether new lines scroll the console down.
    pub fn follows(&self) -> bool {
        self.follow
    }

    /// The number of lines the console shows: the selected test's details, else the output.
    fn shown_lines(&self) -> usize {
        let Some(tab) = self.tabs.get(self.selected) else {
            return 0;
        };
        tab.tests
            .as_ref()
            .and_then(TestTree::detail_lines)
            .unwrap_or_else(|| tab.console.lines().len())
    }

    fn scroll_to_end(&self) {
        let rows = self.shown_lines();
        if rows > 0 {
            self.scroll
                .scroll_to_item(rows - 1, gpui::ScrollStrategy::Top);
        }
    }

    /// After the console's content changed: a selected test's details start at their first line,
    /// where the message and the expected and received values are (`SPEC-0088` v5 behaviour 13);
    /// the output follows Scroll to End.
    fn scroll_console(&self) {
        let details = self
            .tabs
            .get(self.selected)
            .and_then(|t| t.tests.as_ref())
            .and_then(TestTree::detail_lines)
            .is_some();
        if details {
            self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
        } else if self.follow {
            self.scroll_to_end();
        }
    }

    /// Whether the console has more lines than it shows, for checks.
    pub fn console_scrollable(&self) -> bool {
        self.scroll.is_scrollable()
    }

    /// The first line the console shows, for checks.
    pub fn console_top_line(&self) -> usize {
        let state = self.scroll.0.borrow();
        if let Some(asked) = &state.deferred_scroll_to_item {
            return asked.item_index;
        }
        // The console's lines are 18 px high.
        let down = -f32::from(state.base_handle.offset().y);
        (down / 18.).round().max(0.) as usize
    }

    /// Opens the source of the test at `path` of the selected tab's tree (`SPEC-0088` v5
    /// behaviour 12), or says that it cannot be found.
    fn open_test(&mut self, path: &[usize], cx: &mut Context<Self>) {
        let Some(tree) = self.tabs.get(self.selected).and_then(|t| t.tests.as_ref()) else {
            return;
        };
        match test_source(&tree.report, path, &self.root) {
            Some((file, line)) => cx.emit(RunViewEvent::OpenSource(file, line)),
            None => {
                let name = test_at(&tree.report, path).map_or("", |t| t.name.as_str());
                cx.emit(RunViewEvent::Status(format!(
                    "The source of test {name} was not found"
                )));
            }
        }
    }

    /// The selected tab's command line, shell-quoted (Copy Command Line, v6 behaviours 12–13).
    pub fn selected_command_line(&self) -> Option<String> {
        self.tabs
            .get(self.selected)
            .and_then(|t| t.command.as_ref())
            .map(|c| command_line(c))
    }

    /// Copies the selected tab's command line to the clipboard.
    pub fn copy_command_line(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let line = self.selected_command_line()?;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(line.clone()));
        Some(line)
    }

    /// The ⋮ menu's entry `i`: Copy Command Line (0) or Open Run Configuration (1).
    pub fn more_action(&mut self, i: usize, cx: &mut Context<Self>) {
        self.menu_open = None;
        if i == 0 {
            self.copy_command_line(cx);
        } else if let Some(name) = self.selected_name() {
            cx.emit(RunViewEvent::OpenConfiguration(name));
        }
        cx.notify();
    }

    /// Publishes the tabs for the MCP tool `run_output`, at most twice a second while output
    /// arrives (`force` publishes now) (`SPEC-0313` behaviour 9).
    fn publish(&mut self, force: bool) {
        if !force
            && self
                .published
                .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.published = Some(std::time::Instant::now());
        let tabs = self
            .tabs
            .iter()
            .map(|t| idecraft_ai::live::RunTab {
                name: t.name.clone(),
                running: t.console.running,
                exit_code: t.console.exit_code,
                text: t.console.text(),
                failed: t
                    .tests
                    .as_ref()
                    .map(|tree| crate::ai_context_model::failed_tests(&tree.report))
                    .unwrap_or_default(),
            })
            .collect();
        idecraft_ai::live::Live::global().set_runs(&self.root, tabs);
    }

    /// The selected test when it failed (`SPEC-0313` behaviour 3).
    pub fn selected_failed_test(&self) -> Option<idecraft_ai::live::FailedTest> {
        let tree = self.tabs.get(self.selected)?.tests.as_ref()?;
        let row = tree.rows().into_iter().nth(tree.selected)?;
        if !row.is_test || !matches!(row.status, TestStatus::Failed | TestStatus::Error) {
            return None;
        }
        let case = test_at(&tree.report, &row.path)?;
        let all = crate::ai_context_model::failed_tests(&tree.report);
        Some(
            all.into_iter()
                .find(|f| f.name.ends_with(&case.name))
                .unwrap_or_else(|| idecraft_ai::live::FailedTest {
                    name: row.label.clone(),
                    message: case.message.clone().unwrap_or_default(),
                    details: test_details(case),
                }),
        )
    }

    /// Focus handle of the console.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Runs configuration `name` in its tab, stopping a run of it that is still going (behaviour 3).
    pub fn run(
        &mut self,
        configs: &RunConfigs,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let handle = start_run(&self.root, configs, name)?;
        self.run_handle(name, handle, cx);
        Ok(())
    }

    /// Shows the run behind `handle` in the tab of configuration `name` (a run on a remote
    /// project's host, `SPEC-0318` v2 detail 3, or a local one), stopping a run of it that is
    /// still going.
    pub fn run_handle(&mut self, name: &str, handle: RunHandle, cx: &mut Context<Self>) {
        let i = self.tab_index(name);
        let tab = &mut self.tabs[i];
        if let Some(old) = tab.handle.take() {
            old.stop();
        }
        tab.console = Console::starting();
        tab.handle = Some(handle);
        tab.tests = None;
        self.selected = i;
        self.poll(cx);
        cx.notify();
    }

    /// Per tab: configuration name, running, exit code and failed tests (agents, `SPEC-0318` v2
    /// detail 13).
    pub fn tab_states(&self) -> Vec<(String, bool, Option<i32>, usize)> {
        self.tabs
            .iter()
            .map(|t| {
                let failed = t
                    .tests
                    .as_ref()
                    .map(|tree| crate::ai_context_model::failed_tests(&tree.report).len())
                    .unwrap_or_default();
                (
                    t.name.clone(),
                    t.console.running,
                    t.console.exit_code,
                    failed,
                )
            })
            .collect()
    }

    /// Stops the run of the selected tab (behaviour 5); whether one was running.
    pub fn stop_selected(&mut self) -> bool {
        match self.tabs.get(self.selected).and_then(|t| t.handle.as_ref()) {
            Some(h) => {
                h.stop();
                true
            }
            None => false,
        }
    }

    /// Sends the end of input to the program of the selected tab (`SPEC-0087` v10
    /// behaviour 18). `None` when the tab runs nothing; else whether an open input was closed
    /// (false once it was sent, and for a run on the host of a remote project).
    pub fn send_eof(&mut self) -> Option<bool> {
        let tab = self.tabs.get(self.selected).filter(|t| t.console.running)?;
        Some(tab.handle.as_ref().is_some_and(RunHandle::send_eof))
    }

    /// Stops every run of the window (closing it, `SPEC-0094` v3 behaviour 7); how many ran.
    pub fn stop_all(&mut self) -> usize {
        let mut n = 0;
        for t in self.tabs.iter().filter(|t| t.console.running) {
            if let Some(h) = &t.handle {
                h.stop();
                n += 1;
            }
        }
        n
    }

    /// Names of the configurations still running, in tab order (`SPEC-0261` v2 behaviour 6).
    pub fn running_names(&self) -> Vec<String> {
        self.tabs
            .iter()
            .filter(|t| t.console.running)
            .map(|t| t.name.clone())
            .collect()
    }

    /// Whether the selected tab's configuration is running.
    pub fn running(&self) -> bool {
        self.tabs
            .get(self.selected)
            .is_some_and(|t| t.console.running)
    }

    /// The console text of the tab of configuration `name`, whether it is selected or not.
    pub fn output_of(&self, name: &str) -> Option<String> {
        self.tabs
            .iter()
            .rev()
            .find(|t| t.name == name)
            .map(|t| t.console.text())
    }

    /// Name and console text of the selected tab.
    pub fn selected_output(&self) -> Option<(String, String)> {
        self.tabs
            .get(self.selected)
            .map(|t| (t.name.clone(), t.console.text()))
    }

    /// Takes the new output of running tabs every [`POLL`] until none runs.
    fn poll(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let more = this.update(cx, |this, cx| {
                    let mut any = false;
                    let mut grew = false;
                    for tab in &mut this.tabs {
                        let Some(h) = &tab.handle else { continue };
                        loop {
                            match h.events.try_recv() {
                                Ok(event) => {
                                    tab.console.apply(&event);
                                    if let RunEvent::Started { command, .. } = &event {
                                        tab.command = Some(command.clone());
                                    }
                                    if let RunEvent::Finished(o) = &event
                                        && let Some(report) = &o.report
                                    {
                                        let mut tree = TestTree::new(report.clone(), &tab.name);
                                        tree.history =
                                            remember(&this.root, &tab.name, &tree.current);
                                        tab.tests = Some(tree);
                                    }
                                    grew = true;
                                }
                                Err(std::sync::mpsc::TryRecvError::Empty) => {
                                    any = true;
                                    break;
                                }
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                    tab.console.running = false;
                                    tab.handle = None;
                                    grew = true;
                                    break;
                                }
                            }
                        }
                    }
                    if grew {
                        this.scroll_console();
                        cx.notify();
                    }
                    if grew {
                        this.publish(!any);
                    }
                    if !any {
                        this.polling = false;
                    }
                    any
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// What the selected tab's test results tree shows.
    pub fn test_results(&self) -> Option<TestResults> {
        let tree = self.tabs.get(self.selected)?.tests.as_ref()?;
        Some(TestResults {
            summary: status_line(&tree.report),
            rows: tree
                .rows()
                .into_iter()
                .map(|r| (r.depth, r.label))
                .collect(),
            details: tree.details(),
        })
    }

    /// Shows or hides passed tests in the selected tab's tree (behaviour 3).
    pub fn toggle_show_passed(&mut self, cx: &mut Context<Self>) {
        if let Some(tree) = self
            .tabs
            .get_mut(self.selected)
            .and_then(|t| t.tests.as_mut())
        {
            tree.show_passed = !tree.show_passed;
            tree.selected = tree.selected.min(tree.rows().len().saturating_sub(1));
            cx.notify();
        }
    }

    /// Closes the selected tab as its close button does (`CloseActiveTab`, SPEC-0167 v5
    /// behaviour 18).
    pub fn close_selected_tab(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected;
        self.close_tab(selected, cx);
    }

    fn close_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.tabs.len() {
            return;
        }
        if let Some(h) = self.tabs.remove(i).handle {
            h.stop();
        }
        self.selected = self.selected.min(self.tabs.len().saturating_sub(1));
        self.publish(true);
        if self.tabs.is_empty() {
            cx.emit(RunViewEvent::Hide);
        }
        cx.notify();
    }

    /// After a click or key in the test tree: the console starts at the new selection's first
    /// line, and a test that was opened (double click, Enter) opens in the editor.
    fn tree_changed(&mut self, open: Option<RowPath>, cx: &mut Context<Self>) {
        self.scroll_console();
        if let Some(path) = open {
            self.open_test(&path, cx);
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.key == "escape" && !k.modifiers.shift {
            cx.emit(RunViewEvent::Hide);
            cx.stop_propagation();
            return;
        }
        // Expanding and collapsing are the keymap's actions (SPEC-0275 v13 behaviour 32).
        if let Some(key) = crate::keymaps::tree_action_key(k)
            && let Some(tree) = self
                .tabs
                .get_mut(self.selected)
                .and_then(|t| t.tests.as_mut())
        {
            let open = tree.nav_key(key);
            cx.stop_propagation();
            self.tree_changed(open, cx);
            return;
        }
        let plain =
            !(k.modifiers.control || k.modifiers.alt || k.modifiers.platform || k.modifiers.shift);
        if plain
            && let Some(tree) = self
                .tabs
                .get_mut(self.selected)
                .and_then(|t| t.tests.as_mut())
            && let Some(open) = tree.key(&k.key)
        {
            cx.stop_propagation();
            self.tree_changed(open, cx);
        }
    }
}

impl Render for RunView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The "More" tooltip stayed next to the open ⋮ menu (SPEC-0294 v4 behaviour 11).
        if self.menu_open.is_some() {
            idecraft_ui::tooltip::hold_this_frame();
        }
        let p = Palette::current();
        let color = |kind: LineKind| {
            let key = match kind {
                LineKind::System => "CONSOLE_SYSTEM_OUTPUT",
                LineKind::Normal => "CONSOLE_NORMAL_OUTPUT",
                LineKind::Error => "CONSOLE_ERROR_OUTPUT",
            };
            p.scheme_foreground(key).unwrap_or(p.console_text)
        };
        let tab = self.tabs.get(self.selected);
        let details = tab.and_then(|t| t.tests.as_ref()).and_then(|t| t.details());
        let lines: Vec<(u32, String)> = match (&details, tab) {
            // A selected test shows its message, details and output (SPEC-0088 behaviour 4).
            (Some(text), _) => text
                .lines()
                .map(|l| (color(LineKind::Normal), l.to_owned()))
                .collect(),
            (None, Some(t)) => t
                .console
                .lines()
                .map(|(k, s)| (color(*k), s.clone()))
                .collect(),
            (None, None) => Vec::new(),
        };
        let tree_pane = tab
            .and_then(|t| t.tests.as_ref())
            .map(|tree| self.tree_pane(tree, cx));
        // The overall result above the console, as in the reference (SPEC-0088 v2).
        let status = tab.and_then(|t| t.tests.as_ref()).map(|tree| {
            let overall = tree.rows().first().map_or(TestStatus::Passed, |r| r.status);
            div()
                .h(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(icon(status_icon(overall)))
                .child(status_line(&tree.report))
        });
        let mut mono = font(crate::fonts::editor_family());
        mono.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec![
            if cfg!(target_os = "macos") {
                "Menlo".into()
            } else {
                "DejaVu Sans Mono".into()
            },
        ]));
        // Soft-Wrap gives lines of different heights, so the wrapped console is a plain scrolled
        // column (v6 behaviour 12); without it the fixed-height list draws only what is visible.
        let console = if self.soft_wrap {
            // Keyed by the selected row: another test's details start at the top (v5
            // behaviour 13).
            let row = tab.and_then(|t| t.tests.as_ref()).map_or(0, |t| t.selected);
            div()
                .id(("run-console-wrapped", row))
                .overflow_y_scroll()
                .font(mono)
                .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
                .flex_1()
                .children(lines.into_iter().map(|(color, text)| {
                    div()
                        .min_h(px(18.))
                        .px_3()
                        .whitespace_normal()
                        .text_color(rgb(color))
                        .child(text)
                }))
                .into_any_element()
        } else {
            uniform_list(
                "run-console",
                lines.len(),
                cx.processor(move |_this, range: Range<usize>, _window, _cx| {
                    range
                        .map(|i| {
                            div()
                                .h(px(18.))
                                .px_3()
                                .whitespace_nowrap()
                                .text_color(rgb(lines[i].0))
                                .child(lines[i].1.clone())
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .font(mono)
            .text_size(px(crate::fonts::EDITOR_FONT_SIZE))
            .flex_1()
            .into_any_element()
        };
        // Below the ⋮ button that opened it, its right edge under the button (window check of
        // #485, finding b: it opened at the right edge of the Run window).
        let menu = self.menu_open.map(|at| {
            let frame = div()
                .id("run-more-menu")
                .w(px(220.))
                .p_1()
                .rounded(px(6.))
                .border_1()
                .border_color(c(p.popup_border))
                .bg(c(p.popup))
                .shadow_lg()
                .occlude()
                .children(
                    ["Copy Command Line", "Open Run Configuration"]
                        .into_iter()
                        .enumerate()
                        .map(|(i, label)| {
                            div()
                                .id(("run-more-item", i))
                                .px_2()
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .hover(|s| s.bg(c(p.tree_hover)))
                                .child(label)
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    this.more_action(i, cx)
                                }))
                        }),
                )
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.menu_open = None;
                    cx.notify();
                }));
            gpui::deferred(
                gpui::anchored()
                    .position(at)
                    .anchor(gpui::Corner::TopRight)
                    .snap_to_window_with_margin(px(4.))
                    .child(frame),
            )
            .with_priority(2)
        });
        div()
            .id("run-panel")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .semantics(
                crate::semantics::Semantics::new(crate::semantics::Role::Pane)
                    .name("Run")
                    .test_id("run-panel")
                    .action(crate::semantics::AxAction::Click)
                    .effects(&[crate::semantics::Effect::None]),
            )
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            // The console's and the test tree's context menu (`SPEC-0313` behaviour 4).
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|_, e: &gpui::MouseDownEvent, _, cx| {
                    cx.emit(RunViewEvent::ContextMenu(e.position))
                }),
            )
            // Title, session tabs and Stop are in the shared header (SPEC-0061 v5 behaviour 18).
            .child(
                div().flex_1().min_h_0().flex().children(tree_pane).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .bg(rgb(p.console_background))
                        .children(status)
                        .child(console),
                ),
            )
            .children(menu)
    }
}

/// Where the Run window's ⋮ menu hangs from: just below the pointer on the ⋮ button, so its top
/// right corner sits under the button (window check of #485, finding b).
pub fn more_menu_anchor(click: gpui::Point<gpui::Pixels>) -> gpui::Point<gpui::Pixels> {
    gpui::point(click.x + px(8.), click.y + px(12.))
}

/// Icon of a test status in the tree and the status line.
fn status_icon(s: TestStatus) -> &'static str {
    match s {
        TestStatus::Passed => "run/testPassed",
        TestStatus::Failed => "run/testFailed",
        TestStatus::Error => "run/testError",
        TestStatus::Skipped => "run/testIgnored",
    }
}

impl RunView {
    /// A content tab per run session for the shared header, with the session's ×; a click selects
    /// it (SPEC-0061 v5 behaviour 18, SPEC-0087).
    pub fn header_tabs(&mut self, focused: bool, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                crate::metrics::header_tab(("run-tab", i), i == self.selected, focused)
                    .child(icon(if t.console.running {
                        "run/run"
                    } else {
                        "toolwindows/run"
                    }))
                    .child(t.name.clone())
                    .child(
                        div()
                            .id(("run-tab-close", i))
                            .tip(declared!(RUN_TOOLBAR, "run-tab-close"))
                            .rounded(px(3.))
                            .hover(|s| s.bg(c(p.tree_hover)))
                            .child(icon("general/closeSmall"))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.close_tab(i, cx)
                            })),
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.selected = i;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
    }

    /// The Run window's toolbar in the shared header (SPEC-0061 v5 behaviour 18; `SPEC-0087` v6
    /// behaviour 12): Rerun, Stop (enabled while the selected session runs), Scroll to End,
    /// Soft-Wrap, Clear and ⋮ with Copy Command Line and Open Run Configuration.
    pub fn header_toolbar(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        let running = self.running();
        let any = !self.tabs.is_empty();
        let (follow, wrap) = (self.follow, self.soft_wrap);
        vec![
            crate::metrics::header_button("run-rerun")
                .tip(declared!(RUN_TOOLBAR, "run-rerun"))
                .when(!any, |d| d.opacity(0.4))
                .child(icon("idecraft/debugRerun"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.rerun_selected(cx);
                }))
                .into_any_element(),
            crate::metrics::header_button("run-stop")
                .tip(declared!(RUN_TOOLBAR, "run-stop"))
                .when(!running, |d| d.opacity(0.4))
                .child(icon("run/stop"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.stop_selected();
                    cx.notify();
                }))
                .into_any_element(),
            crate::metrics::header_button("run-scroll-end")
                .tip(declared!(RUN_TOOLBAR, "run-scroll-end"))
                .when(follow, |d| d.bg(c(p.tree_selection)))
                .child(icon("general/scrollDown"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_follow(cx)))
                .into_any_element(),
            crate::metrics::header_button("run-soft-wrap")
                .tip(declared!(RUN_TOOLBAR, "run-soft-wrap"))
                .when(wrap, |d| d.bg(c(p.tree_selection)))
                .child(icon("general/softWrap"))
                .on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_soft_wrap(cx)),
                )
                .into_any_element(),
            crate::metrics::header_button("run-clear")
                .tip(declared!(RUN_TOOLBAR, "run-clear"))
                .when(!any, |d| d.opacity(0.4))
                .child(icon("general/delete"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.clear_selected(cx)))
                .into_any_element(),
            crate::metrics::header_button("run-window-more")
                .tip(declared!(RUN_TOOLBAR, "run-window-more"))
                .child(icon("general/moreVertical"))
                .on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                    this.menu_open = match this.menu_open {
                        Some(_) => None,
                        None => Some(more_menu_anchor(e.position())),
                    };
                    cx.notify();
                }))
                .into_any_element(),
        ]
    }

    /// Whether the selected tab's failed tests can run again, or why not (`SPEC-0088` v3
    /// behaviour 7): its runner must select single tests, and something must have failed.
    pub fn rerun_failed_state(&self) -> Result<(), String> {
        let tab = self.tabs.get(self.selected).ok_or("No test run")?;
        let tree = tab.tests.as_ref().ok_or("No test run")?;
        if tree.shown.is_some() {
            return Err("Shows a remembered run; choose the last run in the history".into());
        }
        let config = idecraft_core::RunConfig {
            name: tab.name.clone(),
            command: tab.command.clone().unwrap_or_default(),
            cwd: None,
            env: Default::default(),
            kind: idecraft_core::RunKind::Test,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            verbatim: false,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
        };
        idecraft_core::rerun_failed(&config, &tree.current).map(|_| ())
    }

    /// Rerun Failed Tests of the selected tab (v3 behaviour 7); `false` when it cannot.
    pub fn rerun_failed_selected(&mut self, cx: &mut Context<Self>) -> bool {
        if self.rerun_failed_state().is_err() {
            return false;
        }
        let Some(tab) = self.tabs.get(self.selected) else {
            return false;
        };
        let Some(tree) = &tab.tests else {
            return false;
        };
        cx.emit(RunViewEvent::RerunFailed(
            tab.name.clone(),
            tree.current.clone(),
        ));
        true
    }

    /// The selected tab's test tree, to change it.
    fn tree_mut(&mut self) -> Option<&mut TestTree> {
        self.tabs.get_mut(self.selected)?.tests.as_mut()
    }

    /// The sort button (v3 behaviour 8).
    pub fn next_sort(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.tree_mut() {
            t.next_sort();
            cx.notify();
        }
    }

    /// Expand All (`true`) or Collapse All (v3 behaviour 8).
    pub fn expand_all(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Some(t) = self.tree_mut() {
            t.expand_all(open);
            cx.notify();
        }
    }

    /// Shows the last run (`None`) or the remembered run `index` of the history (v3 behaviour 9).
    pub fn show_history(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if let Some(t) = self.tree_mut() {
            t.show(index);
            t.history_open = false;
            cx.notify();
        }
    }

    /// The remembered runs of the selected tab: time and status line, newest first.
    pub fn history(&self) -> Vec<(u64, String)> {
        self.tabs
            .get(self.selected)
            .and_then(|t| t.tests.as_ref())
            .map(|t| {
                t.history
                    .iter()
                    .map(|e| (e.time, status_line(&e.report)))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn tree_buttons(&self, tree: &TestTree, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let p = Palette::current();
        let rerun = self.rerun_failed_state();
        let button = |id: &'static str| {
            div()
                .id(id)
                .px_1()
                .rounded(px(4.))
                .flex()
                .items_center()
                .hover(|s| s.bg(c(p.tree_hover)))
        };
        let rerun_tip = match &rerun {
            Ok(()) => Tip::plain("Rerun Failed Tests"),
            Err(why) => Tip {
                title: "Rerun Failed Tests".into(),
                shortcut: String::new(),
                details: vec![why.clone()],
            },
        };
        vec![
            button("test-rerun-failed")
                .tip_text("test-rerun-failed", rerun_tip)
                .when(rerun.is_err(), |d| d.opacity(0.4))
                .child(icon("idecraft/debugRerun"))
                .child(icon("run/testFailed"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    this.rerun_failed_selected(cx);
                }))
                .into_any_element(),
            button("test-sort")
                .tip_text("test-sort", Tip::plain(tree.sort.label()))
                .when(tree.sort != crate::test_tree_model::Sort::Report, |d| {
                    d.bg(c(p.tree_selection))
                })
                .text_size(px(12.))
                .text_color(c(p.ui_muted))
                .child("⇅")
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.next_sort(cx)))
                .into_any_element(),
            button("test-expand-all")
                .tip(declared!(RUN_TOOLBAR, "test-expand-all"))
                .child(icon("general/expandAll"))
                .on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.expand_all(true, cx)),
                )
                .into_any_element(),
            button("test-collapse-all")
                .tip(declared!(RUN_TOOLBAR, "test-collapse-all"))
                .child(icon("general/collapseAll"))
                .on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.expand_all(false, cx)),
                )
                .into_any_element(),
            button("test-history")
                .tip(declared!(RUN_TOOLBAR, "test-history"))
                .when(tree.history_open || tree.shown.is_some(), |d| {
                    d.bg(c(p.tree_selection))
                })
                .child(icon("idecraft/history"))
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    if let Some(t) = this.tree_mut() {
                        t.history_open = !t.history_open;
                        cx.notify();
                    }
                }))
                .into_any_element(),
        ]
    }

    /// The history list below the tree's toolbar (v3 behaviour 9): the last run first, then the
    /// remembered ones with their time and result; a click shows that tree.
    fn history_list(&self, tree: &TestTree, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let row = |i: usize, label: String, chosen: bool| {
            div()
                .id(("test-history-row", i))
                .px_2()
                .py(px(2.))
                .text_size(px(12.))
                .whitespace_nowrap()
                .overflow_hidden()
                .when(chosen, |d| d.bg(c(p.tree_selection)))
                .when(!chosen, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(label)
        };
        let mut rows = vec![
            row(0, "Last run".into(), tree.shown.is_none())
                .on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.show_history(None, cx)),
                )
                .into_any_element(),
        ];
        for (i, e) in tree.history.iter().enumerate().skip(1) {
            let label = format!(
                "{} — {}",
                crate::test_tree_model::run_time(e.time),
                status_line(&e.report)
            );
            rows.push(
                row(i, label, tree.shown == Some(i))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.show_history(Some(i), cx)
                    }))
                    .into_any_element(),
            );
        }
        div()
            .flex_none()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(c(p.separator))
            .children(rows)
            .into_any_element()
    }

    /// The test results tree at the left of a test configuration's tab (SPEC-0088 behaviours 1–3).
    fn tree_pane(&self, tree: &TestTree, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = Palette::current();
        let rows = tree.rows();
        let selected = tree.selected;
        div()
            .w(px(340.))
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(c(p.separator))
            .child(
                div()
                    .h(px(28.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .border_b_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .id("show-passed")
                            .tip(declared!(RUN_TOOLBAR, "show-passed"))
                            .px_1()
                            .rounded(px(4.))
                            .when(tree.show_passed, |d| d.bg(c(p.tree_selection)))
                            .when(!tree.show_passed, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(icon("run/testPassed"))
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.toggle_show_passed(cx)
                            })),
                    )
                    .children(self.tree_buttons(tree, cx)),
            )
            .children(tree.history_open.then(|| self.history_list(tree, cx)))
            .child(
                div()
                    .id("test-tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_1()
                    .children(rows.into_iter().enumerate().map(|(i, row)| {
                        let chevron = if !row.has_children {
                            None
                        } else if row.expanded {
                            Some("general/chevronDown")
                        } else {
                            Some("general/chevronRight")
                        };
                        div()
                            .id(("test-row", i))
                            .semantics(
                                Semantics::new(Role::TreeItem)
                                    .name(row.label.clone())
                                    .test_id(format!("test-row-{i}"))
                                    .selected(i == selected),
                            )
                            .h(px(crate::palette::Palette::current().tree_row_height))
                            .flex()
                            .items_center()
                            .gap_1()
                            .pl(px(crate::metrics::tree_indent(row.depth)))
                            .pr_2()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when(i == selected, |d| d.bg(c(p.tree_selection)))
                            .when(i != selected, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .child(match chevron {
                                // One click on the arrow toggles (SPEC-0275 behaviour 8).
                                Some(key) => div()
                                    .id(("test-arrow", i))
                                    .flex_none()
                                    .child(icon(key))
                                    .on_click(cx.listener(
                                        move |this, _: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            if let Some(tree) = this
                                                .tabs
                                                .get_mut(this.selected)
                                                .and_then(|t| t.tests.as_mut())
                                            {
                                                tree.click(i, true, 1);
                                            }
                                            this.tree_changed(None, cx);
                                        },
                                    ))
                                    .into_any_element(),
                                None => div().w(px(16.)).flex_none().into_any_element(),
                            })
                            .child(icon(status_icon(row.status)))
                            .child(div().flex_1().overflow_hidden().child(row.label.clone()))
                            .children(
                                row.duration
                                    .clone()
                                    .map(|d| div().flex_none().text_color(c(p.ui_dim)).child(d)),
                            )
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                let open = this
                                    .tabs
                                    .get_mut(this.selected)
                                    .and_then(|t| t.tests.as_mut())
                                    .and_then(|tree| tree.click(i, false, e.click_count().min(2)));
                                this.tree_changed(open, cx);
                            }))
                    })),
            )
            .into_any_element()
    }
}

/// What the chooser asks the workbench to do.
pub enum RunChooserEvent {
    /// Select and run this configuration.
    Run(String),
    /// Only select this configuration (the run widget's list, D23).
    Select(String),
    /// Select and debug this configuration (⇧ held, or the row's Debug; `SPEC-0087` v6
    /// behaviours 9–10).
    Debug(String),
    /// "Current File": run (or debug) the file of the focused editor, or in the run widget's list
    /// only select running it (v6 behaviour 9).
    CurrentFile {
        /// Debug instead of run.
        debug: bool,
        /// Only select it.
        select_only: bool,
    },
    /// Open the configuration editor, with this configuration selected (Edit…) or not ("Edit
    /// Configurations…"; `SPEC-0327` behaviour 1).
    Edit(Option<String>),
    /// Delete this configuration, after a confirmation (v6 behaviour 10).
    Delete(String),
    /// Pin or unpin this configuration; the chooser stays open (v6 behaviour 10).
    TogglePin(String),
    /// Save this temporary configuration to `run.toml` (`SPEC-0327` behaviour 9).
    SaveTemporary(String),
    /// Copy this detected configuration to `run.toml` (v6 behaviour 10).
    CopyDetected(String),
    /// Close without choosing.
    Close,
    /// Forget "always allow" of this plugin suggestion (`SPEC-0276` behaviour 8).
    Revoke(String),
}

/// Popup listing the run configurations (behaviours 2 and 9–11).
pub struct RunChooser {
    configs: Vec<RunConfig>,
    /// Names of the configurations detected from build files, listed under their own heading.
    detected: Vec<String>,
    /// Names of the temporary configurations, under "Temporary" (v6 behaviour 11).
    temporary: Vec<String>,
    /// Pinned names, under "Pinned" (v6 behaviour 10).
    pins: Vec<String>,
    /// Why a configuration cannot start, by name (v6 behaviour 14).
    blockers: HashMap<String, String>,
    /// Suggestions of plugins, listed under their plugin and type (`SPEC-0276` behaviour 4).
    plugin: Vec<crate::plugin_run_model::PluginRunEntry>,
    /// The rows as listed: those the typed filter leaves.
    rows: Vec<ListRow>,
    /// Every row, as listed without a filter.
    all: Vec<ListRow>,
    /// The text typed into the list (`SPEC-0087` v7 behaviour 15).
    filter: String,
    /// The configuration that was selected when the chooser opened.
    current: Option<String>,
    selected: usize,
    hovered: Option<usize>,
    /// The row whose ⋮ menu is open.
    menu: Option<usize>,
    focus: FocusHandle,
    /// Opened from the run widget: choosing selects without running (D23).
    select_only: bool,
    /// Opened for debugging (`ChooseDebugConfiguration`): choosing debugs and ⇧ runs
    /// (`SPEC-0087` v8 behaviour 16).
    debug_first: bool,
}

impl EventEmitter<RunChooserEvent> for RunChooser {}

impl RunChooserEvent {
    /// A short name of the event, for traces.
    pub fn label(&self) -> String {
        match self {
            Self::Run(n) => format!("run {n}"),
            Self::Select(n) => format!("select {n}"),
            Self::Debug(n) => format!("debug {n}"),
            Self::CurrentFile { debug, select_only } => {
                format!("current file (debug {debug}, select only {select_only})")
            }
            Self::Edit(n) => format!("edit {n:?}"),
            Self::Delete(n) => format!("delete {n}"),
            Self::TogglePin(n) => format!("pin {n}"),
            Self::SaveTemporary(n) => format!("save temporary {n}"),
            Self::CopyDetected(n) => format!("copy detected {n}"),
            Self::Close => "close".into(),
            Self::Revoke(n) => format!("revoke {n}"),
        }
    }
}

impl RunChooser {
    /// A chooser over `configs` with `current` selected; `detected` names come from build files
    /// (SPEC-0087 v2 behaviour 7).
    pub fn new(
        configs: Vec<RunConfig>,
        detected: Vec<String>,
        current: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut chooser = Self {
            configs,
            detected,
            temporary: Vec::new(),
            pins: Vec::new(),
            blockers: HashMap::new(),
            plugin: Vec::new(),
            rows: Vec::new(),
            all: Vec::new(),
            filter: String::new(),
            current: current.map(str::to_owned),
            selected: 0,
            hovered: None,
            menu: None,
            focus: cx.focus_handle(),
            select_only: false,
            debug_first: false,
        };
        chooser.rebuild();
        chooser
    }

    /// Lays out the rows again, keeping the highlight on its entry.
    fn rebuild(&mut self) {
        let keep = self
            .rows
            .get(self.selected)
            .map(|r| r.label().to_owned())
            .or_else(|| self.current.clone());
        let plugin: Vec<(String, String)> = self
            .plugin
            .iter()
            .map(|e| (e.config_name.clone(), e.heading()))
            .collect();
        self.all = list_rows(&ListInput {
            configs: &self.configs,
            temporary: &self.temporary,
            detected: &self.detected,
            plugin: &plugin,
            pins: &self.pins,
            blockers: Some(&self.blockers),
        });
        self.rows = filtered(&self.all, &self.filter);
        self.selected = keep
            .and_then(|k| {
                self.rows
                    .iter()
                    .position(|r| r.config_name() == Some(k.as_str()))
            })
            .unwrap_or(0);
        self.menu = None;
    }

    /// The text typed into the list.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Filters the rows by `text` (`SPEC-0087` v7 behaviour 15): the highlight goes to the first
    /// row whose label starts with it, else to the first row left; clearing the text keeps the
    /// highlight on its entry.
    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        let keep = self.rows.get(self.selected).map(|r| r.label().to_owned());
        self.filter = text.to_owned();
        self.rows = filtered(&self.all, &self.filter);
        let lower = self.filter.to_lowercase();
        self.selected = if self.filter.is_empty() {
            keep.and_then(|k| self.rows.iter().position(|r| r.label() == k))
        } else {
            self.rows
                .iter()
                .position(|r| r.label().to_lowercase().starts_with(&lower))
        }
        .unwrap_or(0);
        self.menu = None;
        self.hovered = None;
        cx.notify();
    }

    /// The same chooser choosing without running, as the run widget's list does (D23: a click
    /// there started the configuration).
    pub fn selecting(mut self) -> Self {
        self.select_only = true;
        self
    }

    /// The same chooser opened for debugging: choosing debugs and ⇧ runs (`SPEC-0087` v8
    /// behaviour 16).
    pub fn debugging(mut self) -> Self {
        self.debug_first = true;
        self
    }

    /// Whether the chooser was opened for debugging.
    pub fn debugs_first(&self) -> bool {
        self.debug_first
    }

    /// The same chooser with these temporary configurations under "Temporary".
    pub fn with_temporary(mut self, names: Vec<String>) -> Self {
        self.temporary = names;
        self.rebuild();
        self
    }

    /// The same chooser with these pins under "Pinned".
    pub fn with_pins(mut self, pins: Vec<String>) -> Self {
        self.pins = pins;
        self.rebuild();
        self
    }

    /// The same chooser with a warning sign on the configurations that cannot start.
    pub fn with_blockers(mut self, blockers: HashMap<String, String>) -> Self {
        self.blockers = blockers;
        self.rebuild();
        self
    }

    /// Replaces the pins (after Pin or Unpin; the chooser stays open).
    pub fn set_pins(&mut self, pins: Vec<String>, cx: &mut Context<Self>) {
        self.pins = pins;
        self.rebuild();
        cx.notify();
    }

    /// What choosing row `i` asks for: running it, debugging it with ⇧, or in the run widget's
    /// list only selecting it.
    fn choose(&self, i: usize, shift: bool) -> Option<RunChooserEvent> {
        let row = self.rows.get(i)?;
        let debug = crate::run_list_model::debug_asked(self.debug_first, shift);
        Some(match choice(row, self.select_only, debug) {
            Choice::Debug(name) => RunChooserEvent::Debug(name),
            Choice::Select(name) => RunChooserEvent::Select(name),
            Choice::Run(name) => RunChooserEvent::Run(name),
            Choice::CurrentFile { debug, select_only } => {
                RunChooserEvent::CurrentFile { debug, select_only }
            }
            Choice::EditConfigurations => RunChooserEvent::Edit(None),
        })
    }

    /// The same chooser with the suggestions of plugins marked as such (`SPEC-0276`).
    pub fn with_plugin_entries(
        mut self,
        plugin: Vec<crate::plugin_run_model::PluginRunEntry>,
    ) -> Self {
        self.plugin = plugin;
        self.rebuild();
        self
    }

    /// The plugin suggestion behind configuration `name`, if it is one.
    fn plugin_entry(&self, name: &str) -> Option<&crate::plugin_run_model::PluginRunEntry> {
        self.plugin.iter().find(|e| e.config_name == name)
    }

    /// Marks the suggestion `name` as no longer always allowed (after a revoke).
    pub fn forget(&mut self, name: &str, cx: &mut Context<Self>) {
        for e in self.plugin.iter_mut().filter(|e| e.config_name == name) {
            e.always_allowed = false;
        }
        cx.notify();
    }

    /// Focus handle of the list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Name of the highlighted configuration (none on "Current File" and "Edit
    /// Configurations…").
    pub fn highlighted(&self) -> Option<String> {
        self.rows
            .get(self.selected)
            .and_then(|r| r.config_name())
            .map(str::to_owned)
    }

    /// Moves the highlight to configuration `name` (for the self-test and MCP).
    pub fn highlight(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.rows.iter().position(|r| r.config_name() == Some(name)) {
            self.selected = i;
            cx.notify();
        }
    }

    /// Names of the listed configurations, in the order they are listed.
    pub fn names(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| r.config_name().map(str::to_owned))
            .collect()
    }

    /// The rows as listed (checks).
    pub fn rows(&self) -> &[ListRow] {
        &self.rows
    }

    /// Chooses the row a digit picks (v6 behaviour 9), with ⇧ to debug.
    pub fn pick_digit(&mut self, digit: u8, shift: bool, cx: &mut Context<Self>) {
        if let Some(i) = row_for_digit(&self.rows, digit) {
            self.selected = i;
            if let Some(e) = self.choose(i, shift) {
                cx.emit(e);
            }
        }
    }

    /// Carries out entry `action` of the ⋮ menu of row `i` (v6 behaviour 10).
    pub fn row_action(&mut self, i: usize, action: RowAction, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(name) = self.rows.get(i).and_then(|r| r.config_name()) else {
            return;
        };
        let name = name.to_owned();
        cx.emit(match action {
            RowAction::Edit => RunChooserEvent::Edit(Some(name)),
            RowAction::Delete => RunChooserEvent::Delete(name),
            RowAction::Pin | RowAction::Unpin => RunChooserEvent::TogglePin(name),
            RowAction::SaveConfiguration => RunChooserEvent::SaveTemporary(name),
            RowAction::CopyToRunToml => RunChooserEvent::CopyDetected(name),
        });
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let n = self.rows.len().max(1);
        let k = &event.keystroke;
        // The first and the last row of the list (`SPEC-0275` v19 behaviour 47); the open menu
        // of a row keeps its keys.
        if self.menu.is_none()
            && let Some(to) = crate::list_nav::edge(k, self.rows.len())
        {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let plain = !(k.modifiers.control || k.modifiers.alt || k.modifiers.platform);
        match k.key.as_str() {
            "down" => self.selected = (self.selected + 1) % n,
            "up" => self.selected = (self.selected + n - 1) % n,
            "enter" => {
                if let Some(e) = self.choose(self.selected, k.modifiers.shift) {
                    cx.emit(e);
                }
            }
            "escape" if self.menu.is_some() => self.menu = None,
            // Escape first clears the typed text (v7 behaviour 15).
            "escape" if escape_clears(&self.filter) => self.set_filter("", cx),
            "escape" => cx.emit(RunChooserEvent::Close),
            // Digits pick a row while nothing is typed, with ⇧ to debug.
            d if plain
                && self.filter.is_empty()
                && d.len() == 1
                && d.as_bytes()[0].is_ascii_digit() =>
            {
                self.pick_digit(d.as_bytes()[0] - b'0', k.modifiers.shift, cx);
            }
            "backspace" if !self.filter.is_empty() => {
                let mut text = self.filter.clone();
                text.pop();
                self.set_filter(&text, cx);
            }
            "backspace" | "delete" => {
                let Some(name) = self.highlighted() else {
                    return;
                };
                if !self.plugin_entry(&name).is_some_and(|e| e.always_allowed) {
                    return;
                }
                cx.emit(RunChooserEvent::Revoke(name));
            }
            // Typing filters the rows by name; the text arrives through the platform's text
            // input, not as the key behind it (`SPEC-0004` v6 behaviour 17).
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Takes text the platform's text input committed (`SPEC-0004` v6 behaviour 17): it extends
    /// the filter, or a digit picks its row while nothing is typed.
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        match typed(&self.filter, committed) {
            FilterKey::Filter(text) => self.set_filter(&text, cx),
            FilterKey::Digit(digit) => self.pick_digit(digit, false, cx),
            FilterKey::Other => return,
        }
        cx.notify();
    }
}

crate::committed_text::committed_text_input!(RunChooser, take_committed);

impl Render for RunChooser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.menu.is_some() {
            idecraft_ui::tooltip::hold_this_frame();
        }
        let p = Palette::current();
        let rows = self.rows.clone();
        let typed_text = (!self.filter.is_empty()).then(|| self.filter.clone());
        let nothing = typed_text.is_some() && rows.is_empty();
        div()
            .id("run-chooser")
            .track_focus(&self.focus)
            // Clicks inside the list never reach what lies beneath (window check of #485, D2).
            .occlude()
            .on_key_down(cx.listener(Self::on_key))
            // Typed text comes from the text input (`SPEC-0004` v6 behaviour 17).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .w(px(460.))
            .p_1()
            .rounded(px(8.))
            .border_1()
            .border_color(c(p.separator))
            .bg(c(p.island))
            .shadow_lg()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(c(p.ui_muted))
                    .child("Run Configurations")
                    // The typed text beside the title, as in the other filtering lists.
                    .when_some(typed_text, |d, text| {
                        d.child(div().text_color(c(p.ui_text)).child(text))
                    }),
            )
            .when(nothing, |d| {
                d.child(
                    div()
                        .h(px(26.))
                        .px_2()
                        .flex()
                        .items_center()
                        .text_color(c(p.ui_dim))
                        .child(NOTHING_MATCHES),
                )
            })
            .children(rows.into_iter().enumerate().map(|(i, row)| {
                let on = i == self.selected;
                let active = on || self.hovered == Some(i);
                let name = row.config_name().map(str::to_owned);
                let plugin = name.as_deref().and_then(|n| self.plugin_entry(n)).cloned();
                let heading = row.heading.clone().map(|text| {
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_color(c(p.ui_muted))
                        .child(text)
                });
                let tag = |id: (&'static str, usize), text: &'static str| {
                    div()
                        .id(id)
                        .flex_none()
                        .px_1()
                        .rounded(px(3.))
                        .border_1()
                        .border_color(c(p.separator))
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(text)
                };
                let allowed = plugin.as_ref().is_some_and(|e| e.always_allowed);
                let revoke_name = name.clone().unwrap_or_default();
                let tags = plugin.as_ref().map(|_| {
                    div()
                        .flex()
                        .gap_1()
                        .child(tag(("run-plugin-tag", i), "plugin"))
                        .when(allowed, |d| {
                            d.child(tag(("run-plugin-allowed", i), "always allowed ×").on_click(
                                cx.listener(move |_, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    cx.emit(RunChooserEvent::Revoke(revoke_name.clone()))
                                }),
                            ))
                        })
                });
                let row_icon = match &row.kind {
                    RowKind::Config { .. } => "toolwindows/run",
                    RowKind::CurrentFile => "run/run",
                    RowKind::EditConfigurations => "general/settings",
                };
                let menu_items = menu(&row);
                let small = |id: (&'static str, usize), key: &'static str| {
                    div()
                        .id(id)
                        .flex_none()
                        .rounded(px(3.))
                        .hover(|s| s.bg(c(p.tree_hover)))
                        .child(icon(key))
                };
                // Run, Debug and ⋮ on the hovered or highlighted row (v6 behaviour 10).
                let actions = (active && name.is_some()).then(|| {
                    div()
                        .flex_none()
                        .flex()
                        .gap_1()
                        .child(small(("run-row-run", i), "run/run").on_click(cx.listener(
                            move |this, _: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                if let Some(e) = this.choose(i, false).map(|e| match e {
                                    RunChooserEvent::Select(n) => RunChooserEvent::Run(n),
                                    e => e,
                                }) {
                                    cx.emit(e);
                                }
                            },
                        )))
                        .child(
                            small(("run-row-debug", i), "run/debug").on_click(cx.listener(
                                move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    if let Some(e) = this.choose(i, true) {
                                        cx.emit(e);
                                    }
                                },
                            )),
                        )
                        .when(!menu_items.is_empty(), |d| {
                            d.child(small(("run-row-more", i), "general/moreVertical").on_click(
                                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.menu = if this.menu == Some(i) { None } else { Some(i) };
                                    cx.notify();
                                }),
                            ))
                        })
                });
                let row_menu = (self.menu == Some(i)).then(|| {
                    div()
                        .ml(px(28.))
                        .mb_1()
                        .p_1()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(c(p.popup_border))
                        .bg(c(p.popup))
                        .flex()
                        .flex_col()
                        .children(menu_items.iter().enumerate().map(|(k, action)| {
                            let action = *action;
                            div()
                                .id(("run-row-menu", i * 16 + k))
                                .h(px(24.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .hover(|s| s.bg(c(p.tree_hover)))
                                .child(action.label())
                                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.row_action(i, action, cx)
                                }))
                        }))
                });
                let warning = row.warning.clone();
                div()
                    .flex()
                    .flex_col()
                    .children(heading)
                    .when(matches!(row.kind, RowKind::CurrentFile), |d| {
                        d.child(div().mt_1().border_t_1().border_color(c(p.separator)))
                    })
                    .child(
                        div()
                            .id(("run-choice", i))
                            // A click target for checks (no tooltip).
                            .tip_text(
                                format!("run-choice-{i}"),
                                idecraft_ui::tooltip::Tip::default(),
                            )
                            .h(px(26.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered {
                                    this.hovered = Some(i);
                                } else if this.hovered == Some(i) {
                                    this.hovered = None;
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(12.))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(c(p.ui_dim))
                                    .child(row.digit.map(|d| d.to_string()).unwrap_or_default()),
                            )
                            .child(icon(row_icon))
                            .child(div().flex_none().child(row.label().to_owned()))
                            .children(warning.map(|why| {
                                div()
                                    .id(("run-choice-warning", i))
                                    .flex_none()
                                    .child(icon("status/warning"))
                                    .tip_text(
                                        format!("run-choice-warning-{i}"),
                                        idecraft_ui::tooltip::Tip {
                                            title: why,
                                            shortcut: String::new(),
                                            details: Vec::new(),
                                        },
                                    )
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_color(c(p.ui_dim))
                                    .child(row.command.clone()),
                            )
                            .children(tags)
                            .children(actions)
                            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                                this.selected = i;
                                if let Some(ev) = this.choose(i, e.modifiers().shift) {
                                    cx.emit(ev);
                                }
                            })),
                    )
                    .children(row_menu)
            }))
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(crate::run_list_model::footer(self.debug_first)),
            )
    }
}

#[cfg(test)]
mod tests {
    /// Window check of #485 (finding b): the ⋮ menu hangs from the button, not from the Run
    /// window's edge.
    #[test]
    fn the_more_menu_hangs_from_the_button() {
        use gpui::{point, px};
        let at = super::more_menu_anchor(point(px(700.), px(420.)));
        assert_eq!(at, point(px(708.), px(432.)));
    }
}
