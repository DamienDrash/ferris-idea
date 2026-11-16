//! The Spring tool window (`SPEC-0115`): the project's beans grouped by kind, with the problems of
//! the bean model on top; Enter or a double click opens a declaration. Since v5 (behaviour 15) the
//! list is narrowed by typed text and by kind, the beans of the test sources are left out unless
//! asked for, and a panel beside the list shows what the selected bean needs and what needs it.

use std::path::PathBuf;

use gpui::{Context, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px, rgb};
use idecraft_languages::spring::{SpringModel, SpringProblem};

use crate::palette::Palette;
use crate::workbench_view::c;

/// What the window asks the workbench to do.
pub enum SpringEvent {
    /// Hide the window.
    Hide,
    /// Open this workspace-relative file at this 0-based line.
    Open(PathBuf, usize),
}

/// One row: a problem, a kind heading or a bean.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Problem(SpringProblem),
    Kind(String, usize),
    Bean {
        /// Index into the model's beans.
        index: usize,
        name: String,
        type_name: String,
        file: PathBuf,
        line: usize,
        test: bool,
    },
}

/// What narrows the list of beans (`SPEC-0115` v5 behaviour 15).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BeanFilter {
    /// Text a bean's name, type or kind must contain, whatever the case; empty: every bean.
    pub text: String,
    /// The one kind shown (`Service`, `Bean`, …); none: every kind.
    pub kind: Option<String>,
    /// Whether the beans of the test sources are listed too.
    pub tests: bool,
}

impl BeanFilter {
    /// Whether text or kind narrow the list.
    fn narrows(&self) -> bool {
        !self.text.is_empty() || self.kind.is_some()
    }
}

/// One line of the panel beside the list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Detail {
    /// The bean: name and type, then kind and place.
    Title(String),
    /// Grey text: the place, or that a list is empty.
    Note(String),
    /// A heading with a count.
    Heading(String),
    /// A bean the selected one needs (`→`) or is needed by (`←`), or a type without a bean
    /// (`✕`), and where a click leads.
    Link(&'static str, String, PathBuf, usize),
}

/// The window.
pub struct SpringView {
    model: SpringModel,
    filter: BeanFilter,
    rows: Vec<Row>,
    selected: usize,
    loading: bool,
    /// Frameworks of the project, shown as the title (`SPEC-0134`).
    title: String,
    /// How often the window was painted and the size it had the last time, so a self-test can
    /// tell a window that is drawn from one that is only marked as shown (`SPEC-0115` AK-19).
    painted: std::rc::Rc<std::cell::Cell<(usize, f32, f32)>>,
    focus: FocusHandle,
}

impl EventEmitter<SpringEvent> for SpringView {}

/// Whether `p` is a recommendation (`SPEC-0115` v4 behaviour 11), shown muted.
fn is_weak(p: &SpringProblem) -> bool {
    p.severity == idecraft_languages::inspections::Level::Weak
}

/// The kinds of the beans `filter.tests` lets through, sorted.
fn kinds_of(model: &SpringModel, tests: bool) -> Vec<String> {
    let mut kinds: Vec<String> = model
        .beans
        .iter()
        .filter(|b| tests || !b.test)
        .map(|b| b.kind.clone())
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

/// Rows for `model`: problems (warnings before weak warnings) while nothing narrows the list,
/// then each kind with its beans sorted by name. The beans of the test sources are listed only
/// when the filter asks for them, and then marked (v5 behaviour 15).
fn rows_of(model: &SpringModel, filter: &BeanFilter) -> Vec<Row> {
    let mut rows: Vec<Row> = if filter.narrows() {
        Vec::new()
    } else {
        model.problems().into_iter().map(Row::Problem).collect()
    };
    let needle = filter.text.to_lowercase();
    let fits = |b: &idecraft_languages::spring::Bean| {
        (filter.tests || !b.test)
            && filter.kind.as_ref().is_none_or(|k| *k == b.kind)
            && (needle.is_empty()
                || [&b.name, &b.type_name, &b.kind]
                    .iter()
                    .any(|t| t.to_lowercase().contains(&needle)))
    };
    for kind in kinds_of(model, filter.tests) {
        let mut beans: Vec<_> = model
            .beans
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == kind && fits(b))
            .collect();
        if beans.is_empty() {
            continue;
        }
        beans.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        rows.push(Row::Kind(kind, beans.len()));
        rows.extend(beans.into_iter().map(|(index, b)| Row::Bean {
            index,
            name: b.name.clone(),
            type_name: b.type_name.clone(),
            file: b.file.clone(),
            line: b.line,
            test: b.test,
        }));
    }
    rows
}

/// The panel for bean `index`: what it is, what it needs and what needs it.
fn details_of(model: &SpringModel, index: usize) -> Vec<Detail> {
    let Some(b) = model.beans.get(index) else {
        return Vec::new();
    };
    let file = b
        .file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut what = format!("{} · {file}:{}", b.kind, b.line + 1);
    for (on, label) in [
        (b.primary, "primary"),
        (b.conditional, "conditional"),
        (b.fallback, "fallback"),
        (b.test, "test"),
    ] {
        if on {
            what.push_str(&format!(" · {label}"));
        }
    }
    let mut out = vec![
        Detail::Title(format!("{}: {}", b.name, b.type_name)),
        Detail::Note(what),
    ];
    let needs = model.dependencies(index);
    out.push(Detail::Heading(format!("Needs ({})", needs.len())));
    if needs.is_empty() {
        out.push(Detail::Note("No bean of the project".into()));
    }
    out.extend(needs.into_iter().map(|l| {
        if l.missing {
            Detail::Link("✕", format!("{}: no bean", l.type_name), l.file, l.line)
        } else {
            Detail::Link("→", format!("{}: {}", l.name, l.type_name), l.file, l.line)
        }
    }));
    let needed = model.dependents(index);
    out.push(Detail::Heading(format!("Needed by ({})", needed.len())));
    if needed.is_empty() {
        out.push(Detail::Note("No bean of the project".into()));
    }
    out.extend(
        needed
            .into_iter()
            .map(|l| Detail::Link("←", format!("{}: {}", l.name, l.type_name), l.file, l.line)),
    );
    out
}

impl SpringView {
    /// An empty window, loading.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            model: SpringModel::default(),
            filter: BeanFilter::default(),
            rows: Vec::new(),
            selected: 0,
            loading: true,
            title: "Spring".into(),
            painted: Default::default(),
            focus: cx.focus_handle(),
        }
    }

    /// How often the window was painted, and its width and height at the last paint.
    pub fn painted(&self) -> (usize, f32, f32) {
        self.painted.get()
    }

    /// Shows `model` (behaviour 6).
    pub fn show(&mut self, model: &SpringModel, cx: &mut Context<Self>) {
        self.model = model.clone();
        self.title = if model.frameworks.is_empty() {
            "Spring".into()
        } else {
            model.frameworks.join(" · ")
        };
        self.loading = false;
        // A kind the project no longer has narrows nothing.
        if let Some(kind) = &self.filter.kind
            && !kinds_of(&self.model, self.filter.tests).contains(kind)
        {
            self.filter.kind = None;
        }
        self.refill();
        cx.notify();
    }

    fn refill(&mut self) {
        self.rows = rows_of(&self.model, &self.filter);
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    /// Marks the window as reading the project again.
    pub fn loading(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        // Shown anew: not painted yet.
        self.painted.set((0, 0., 0.));
        cx.notify();
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// What narrows the list now.
    pub fn filter(&self) -> &BeanFilter {
        &self.filter
    }

    /// Narrows the list to the beans whose name, type or kind contains `text` (v5 behaviour 15).
    pub fn set_filter_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.text = text.to_owned();
        self.selected = 0;
        self.refill();
        cx.notify();
    }

    /// Shows the beans of one kind, or of every kind.
    pub fn set_kind(&mut self, kind: Option<&str>, cx: &mut Context<Self>) {
        self.filter.kind = kind.map(str::to_owned);
        self.selected = 0;
        self.refill();
        cx.notify();
    }

    /// The next kind after the one shown; after the last, every kind again.
    pub fn next_kind(&mut self, cx: &mut Context<Self>) {
        let kinds = kinds_of(&self.model, self.filter.tests);
        let next = match &self.filter.kind {
            None => kinds.first().cloned(),
            Some(k) => kinds
                .iter()
                .position(|x| x == k)
                .and_then(|i| kinds.get(i + 1).cloned()),
        };
        self.set_kind(next.as_deref(), cx);
    }

    /// Lists the beans of the test sources too, or only those of the application.
    pub fn set_show_tests(&mut self, tests: bool, cx: &mut Context<Self>) {
        self.filter.tests = tests;
        self.refill();
        cx.notify();
    }

    /// The count in the header: the beans listed, of how many, and the test beans left out.
    pub fn count_label(&self) -> String {
        let listed = self
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Bean { .. }))
            .count();
        let tests = self.model.beans.iter().filter(|b| b.test).count();
        let all = if self.filter.tests {
            self.model.beans.len()
        } else {
            self.model.beans.len() - tests
        };
        let mut label = if listed == all {
            format!("{all} beans")
        } else {
            format!("{listed} of {all} beans")
        };
        if tests > 0 && !self.filter.tests {
            label.push_str(&format!(" · {tests} in tests not listed"));
        }
        label
    }

    /// The rows as text (for tests): problems with `!` (weak warnings with `~`), kinds with their
    /// count, beans indented, a bean of the test sources marked `(test)`.
    pub fn texts(&self) -> Vec<String> {
        self.rows.iter().map(row_text).collect()
    }

    /// The panel beside the list as text (for tests): the selected bean, `→` what it needs, `←`
    /// what needs it, `✕` a type without a bean. Empty when no bean is selected.
    pub fn details(&self) -> Vec<String> {
        self.selected_details()
            .into_iter()
            .map(|d| match d {
                Detail::Title(t) | Detail::Note(t) | Detail::Heading(t) => t,
                Detail::Link(arrow, t, ..) => format!("{arrow} {t}"),
            })
            .collect()
    }

    fn selected_details(&self) -> Vec<Detail> {
        match self.rows.get(self.selected) {
            Some(Row::Bean { index, .. }) => details_of(&self.model, *index),
            _ => Vec::new(),
        }
    }

    /// Selects the first row whose text contains `part` (for tests).
    pub fn select_text(&mut self, part: &str) -> bool {
        match self.texts().iter().position(|t| t.contains(part)) {
            Some(i) => {
                self.selected = i;
                true
            }
            None => false,
        }
    }

    /// Opens the selected bean or problem.
    pub fn open_selected(&mut self, cx: &mut Context<Self>) {
        match self.rows.get(self.selected) {
            Some(Row::Bean { file, line, .. }) => cx.emit(SpringEvent::Open(file.clone(), *line)),
            Some(Row::Problem(p)) => cx.emit(SpringEvent::Open(p.file.clone(), p.line)),
            _ => {}
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let n = self.rows.len();
        let k = &event.keystroke;
        // The first and the last row (`SPEC-0275` v19 behaviour 47).
        if let Some(to) = crate::list_nav::edge(k, n) {
            self.selected = to;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match k.key.as_str() {
            "down" if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "enter" => self.open_selected(cx),
            // Escape first clears the typed text.
            "escape" if !self.filter.text.is_empty() => self.set_filter_text("", cx),
            "escape" => cx.emit(SpringEvent::Hide),
            "backspace" if !self.filter.text.is_empty() => {
                let mut text = self.filter.text.clone();
                text.pop();
                self.set_filter_text(&text, cx);
            }
            // Typed text arrives through the platform's text input (`SPEC-0004` v7 behaviour
            // 17a), not as the key behind it.
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Takes text the platform's text input committed: it extends the typed filter
    /// (`SPEC-0004` v7 behaviour 17a).
    pub fn take_committed(&mut self, committed: &str, cx: &mut Context<Self>) {
        if crate::speed_search::types_into(&self.filter.text, committed) {
            let text = format!("{}{committed}", self.filter.text);
            self.set_filter_text(&text, cx);
            cx.notify();
        }
    }
}

crate::committed_text::committed_text_input!(SpringView, take_committed);

fn row_text(row: &Row) -> String {
    match row {
        Row::Problem(p) if is_weak(p) => format!("~ {}", p.message),
        Row::Problem(p) => format!("! {}", p.message),
        Row::Kind(k, n) => format!("{k} ({n})"),
        Row::Bean {
            name,
            type_name,
            test,
            ..
        } => format!(
            "  {name}: {type_name}{}",
            if *test { " (test)" } else { "" }
        ),
    }
}

impl Render for SpringView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let selected = self.selected;
        let rows = self.rows.clone();
        let details = self.selected_details();
        let chip = |id: &'static str, text: String, on: bool| {
            div()
                .id(id)
                .px_2()
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(c(if on { p.ui_text } else { p.ui_muted }))
                .when(on, |d| d.bg(c(p.tree_selection)))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(text)
        };
        let filter_text = if self.filter.text.is_empty() {
            "Type to filter".to_owned()
        } else {
            format!("Filter: {}", self.filter.text)
        };
        let kind_text = format!("Kind: {}", self.filter.kind.as_deref().unwrap_or("All"));
        let painted = self.painted.clone();
        div()
            .id("spring-panel")
            .relative()
            .child(
                gpui::canvas(
                    move |b, _, _| {
                        let (n, _, _) = painted.get();
                        painted.set((n + 1, f32::from(b.size.width), f32::from(b.size.height)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // Typed text from the text input (`SPEC-0004` v7 behaviour 17a).
            .relative()
            .child(crate::committed_text::input_area(&self.focus, cx.entity()))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    // The shared tool window header (SPEC-0061 v6 behaviour 33).
                    .map(crate::metrics::panel_header)
                    .child(crate::metrics::panel_title(self.title.clone()))
                    .child(div().text_color(c(p.ui_muted)).child(if self.loading {
                        "Reading beans…".to_owned()
                    } else {
                        self.count_label()
                    })),
            )
            .child(
                // The filters (v5 behaviour 15): typed text, one kind, the test sources.
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .text_color(c(if self.filter.text.is_empty() {
                                p.ui_dim
                            } else {
                                p.ui_text
                            }))
                            .child(filter_text),
                    )
                    .child(div().flex_1())
                    .child(
                        chip("spring-kind", kind_text, self.filter.kind.is_some()).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.next_kind(cx)),
                        ),
                    )
                    .child(
                        chip("spring-tests", "Tests".to_owned(), self.filter.tests).on_click(
                            cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                let tests = !this.filter.tests;
                                this.set_show_tests(tests, cx);
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .border_t_1()
                    .border_color(c(p.separator))
                    .child(
                        div()
                            .id("spring-list")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .px_1()
                            .when(rows.is_empty() && !self.loading, |d| {
                                d.child(div().px_3().py_2().text_color(c(p.ui_muted)).child(
                                    if self.filter.narrows() {
                                        "No bean matches the filter"
                                    } else {
                                        "No Spring beans found in the project's Java sources"
                                    },
                                ))
                            })
                            .children(rows.into_iter().enumerate().map(|(i, row)| {
                                let on = i == selected;
                                let base = div()
                                    .id(("spring-row", i))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .when(on, |d| d.bg(c(p.tree_selection)))
                                    .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                                    .on_click(cx.listener(
                                        move |this, ev: &gpui::ClickEvent, _, cx| {
                                            this.selected = i;
                                            if ev.click_count() > 1 {
                                                this.open_selected(cx);
                                            }
                                            cx.notify();
                                        },
                                    ));
                                match row {
                                    Row::Problem(pr) if is_weak(&pr) => base
                                        .text_color(c(p.ui_muted))
                                        .child(format!("· {}", pr.message))
                                        .child(div().flex_1())
                                        .child(div().text_color(c(p.ui_dim)).child(format!(
                                            "{}:{}",
                                            pr.file.display(),
                                            pr.line + 1
                                        ))),
                                    Row::Problem(pr) => base
                                        .text_color(rgb(p.console_ansi_text[3]))
                                        .child(format!("⚠ {}", pr.message))
                                        .child(div().flex_1())
                                        .child(div().text_color(c(p.ui_dim)).child(format!(
                                            "{}:{}",
                                            pr.file.display(),
                                            pr.line + 1
                                        ))),
                                    Row::Kind(kind, n) => base
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(format!("{kind} ({n})")),
                                    Row::Bean {
                                        name,
                                        type_name,
                                        file,
                                        line,
                                        test,
                                        ..
                                    } => base
                                        .pl(px(24.))
                                        .child(name)
                                        .child(div().text_color(c(p.ui_muted)).child(type_name))
                                        .when(test, |d| {
                                            d.child(div().text_color(c(p.ui_dim)).child("test"))
                                        })
                                        .child(div().flex_1())
                                        .child(div().text_color(c(p.ui_dim)).child(format!(
                                            "{}:{}",
                                            file.file_name()
                                                .map(|n| n.to_string_lossy().into_owned())
                                                .unwrap_or_default(),
                                            line + 1
                                        ))),
                                }
                            })),
                    )
                    // What the selected bean needs and what needs it (v5 behaviour 15).
                    .when(!details.is_empty(), |d| {
                        d.child(
                            div()
                                .id("spring-details")
                                .w(px(300.))
                                .flex_none()
                                .h_full()
                                .overflow_y_scroll()
                                .border_l_1()
                                .border_color(c(p.separator))
                                .px_2()
                                .py_1()
                                .children(details.into_iter().enumerate().map(|(i, detail)| {
                                    let line = div()
                                        .id(("spring-detail", i))
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .overflow_hidden()
                                        .whitespace_nowrap();
                                    match detail {
                                        Detail::Title(t) => {
                                            line.font_weight(gpui::FontWeight::SEMIBOLD).child(t)
                                        }
                                        Detail::Note(t) => line.text_color(c(p.ui_muted)).child(t),
                                        Detail::Heading(t) => {
                                            line.pt_1().text_color(c(p.ui_muted)).child(t)
                                        }
                                        Detail::Link(arrow, t, file, at) => line
                                            .pl(px(8.))
                                            .rounded(px(4.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(c(p.tree_hover)))
                                            .child(div().text_color(c(p.ui_dim)).child(arrow))
                                            .child(t)
                                            .on_click(cx.listener(
                                                move |_, _: &gpui::ClickEvent, _, cx| {
                                                    cx.emit(SpringEvent::Open(file.clone(), at));
                                                },
                                            )),
                                    }
                                })),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(model: &SpringModel, filter: &BeanFilter) -> Vec<String> {
        rows_of(model, filter).iter().map(row_text).collect()
    }

    fn sample() -> SpringModel {
        let files = vec![
            (
                PathBuf::from("src/main/java/A.java"),
                "@Service\nclass Zed { Zed(Gone g, Repo r) {} }\n".to_owned(),
            ),
            (
                PathBuf::from("src/main/java/B.java"),
                "@Service\nclass Alpha { Alpha(Zed z) {} }\n".to_owned(),
            ),
            (
                PathBuf::from("src/main/java/C.java"),
                "@Repository\nclass Repo {}\n".to_owned(),
            ),
            (
                PathBuf::from("src/main/java/D.java"),
                "interface Gone {}\n".to_owned(),
            ),
            (
                PathBuf::from("src/test/java/T.java"),
                "@TestConfiguration\nclass Fakes {\n    @Bean\n    Repo fakeRepo() { return null; }\n}\n"
                    .to_owned(),
            ),
        ];
        idecraft_languages::spring::model_of(&files)
    }

    #[test]
    fn rows_put_problems_first_and_group_beans() {
        // The beans of the test sources are not listed by default (v5 behaviour 15).
        assert_eq!(
            texts(&sample(), &BeanFilter::default()),
            [
                "! No bean of type Gone found",
                "Repository (1)",
                "  repo: Repo",
                "Service (2)",
                "  alpha: Alpha",
                "  zed: Zed"
            ]
        );
    }

    /// SPEC-0115 AK-15: text narrows by name, type and kind whatever the case; one kind; the
    /// test beans only when asked for, and marked; problems only while nothing narrows.
    #[test]
    fn filters_narrow_the_beans() {
        let model = sample();
        let by = |text: &str, kind: Option<&str>, tests: bool| {
            texts(
                &model,
                &BeanFilter {
                    text: text.into(),
                    kind: kind.map(str::to_owned),
                    tests,
                },
            )
        };
        assert_eq!(by("ZE", None, false), ["Service (1)", "  zed: Zed"]);
        assert_eq!(by("repo", None, false), ["Repository (1)", "  repo: Repo"]);
        assert_eq!(
            by("", Some("Service"), false),
            ["Service (2)", "  alpha: Alpha", "  zed: Zed"]
        );
        assert_eq!(by("alp", Some("Repository"), false), [""; 0]);
        assert_eq!(
            by("", None, true),
            [
                "! No bean of type Gone found",
                "Bean (1)",
                "  fakeRepo: Repo (test)",
                "Repository (1)",
                "  repo: Repo",
                "Service (2)",
                "  alpha: Alpha",
                "  zed: Zed",
                "TestConfiguration (1)",
                "  fakes: Fakes (test)",
            ]
        );
        assert_eq!(kinds_of(&model, false), ["Repository", "Service"]);
        assert_eq!(
            kinds_of(&model, true),
            ["Bean", "Repository", "Service", "TestConfiguration"]
        );
    }

    /// SPEC-0115 AK-15: the panel names what the selected bean needs and what needs it.
    #[test]
    fn details_name_dependencies_and_dependents() {
        let model = sample();
        let lines = |name: &str| -> Vec<String> {
            let index = model.beans.iter().position(|b| b.name == name).unwrap();
            details_of(&model, index)
                .into_iter()
                .map(|d| match d {
                    Detail::Title(t) | Detail::Note(t) | Detail::Heading(t) => t,
                    Detail::Link(arrow, t, file, line) => format!(
                        "{arrow} {t} @{}:{}",
                        file.file_name().unwrap().to_string_lossy(),
                        line + 1
                    ),
                })
                .collect()
        };
        assert_eq!(
            lines("zed"),
            [
                "zed: Zed",
                "Service · A.java:2",
                "Needs (2)",
                "✕ Gone: no bean @A.java:2",
                "→ repo: Repo @C.java:2",
                "Needed by (1)",
                "← alpha: Alpha @B.java:2",
            ]
        );
        assert_eq!(
            lines("fakeRepo"),
            [
                "fakeRepo: Repo",
                "Bean · T.java:4 · test",
                "Needs (0)",
                "No bean of the project",
                "Needed by (0)",
                "No bean of the project",
            ]
        );
    }
}
