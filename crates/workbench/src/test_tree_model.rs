//! Rows, status line and details of the test results tree in the Run tool window (`SPEC-0088`),
//! free of the UI toolkit.

use std::collections::HashSet;

use idecraft_core::{TestCase, TestReport, TestStatus, TestSuite, Totals};

/// Path of a row: suite indexes from the top, then optionally a test index.
pub type RowPath = Vec<usize>;

/// What a row shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Where it is in the report (empty for the root).
    pub path: RowPath,
    /// Whether it is a test (else the root or a suite).
    pub is_test: bool,
    /// Indentation level.
    pub depth: usize,
    /// Name.
    pub label: String,
    /// Outcome (of the whole subtree for suites).
    pub status: TestStatus,
    /// Duration text, e.g. `12 ms`.
    pub duration: Option<String>,
    /// Whether it has children and whether they are shown.
    pub has_children: bool,
    /// Whether its children are shown.
    pub expanded: bool,
}

/// Outcome of a subtree: failed or error wins, then passed, skipped only when nothing ran.
pub fn status_of(t: &Totals) -> TestStatus {
    if t.errors > 0 && t.failed == 0 {
        TestStatus::Error
    } else if t.failed + t.errors > 0 {
        TestStatus::Failed
    } else if t.passed == 0 && t.skipped > 0 {
        TestStatus::Skipped
    } else {
        TestStatus::Passed
    }
}

/// `0.012` → `12 ms`, `1.5` → `1 s 500 ms`, `75` → `1 m 15 s` (the reference's style).
pub fn duration_text(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round() as u64;
    match ms {
        0..1000 => format!("{ms} ms"),
        1000..60_000 => match ms % 1000 {
            0 => format!("{} s", ms / 1000),
            r => format!("{} s {r} ms", ms / 1000),
        },
        _ => match (ms / 1000) % 60 {
            0 => format!("{} m", ms / 60_000),
            s => format!("{} m {s} s", ms / 60_000),
        },
    }
}

/// The status line above the console (behaviour 2), e.g. "Tests failed: 1, passed: 2 of 3 tests – 12 ms".
pub fn status_line(report: &TestReport) -> String {
    let t = &report.totals;
    let failed = t.failed + t.errors;
    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(format!("Tests failed: {failed}"));
        parts.push(format!("passed: {}", t.passed));
    } else {
        parts.push(format!("Tests passed: {}", t.passed));
    }
    if t.skipped > 0 {
        parts.push(format!("ignored: {}", t.skipped));
    }
    let noun = if t.tests == 1 { "test" } else { "tests" };
    let mut line = format!("{} of {} {noun}", parts.join(", "), t.tests);
    let total: f64 = report.suites.iter().filter_map(|s| s.duration).sum();
    if total > 0.0 {
        line.push_str(&format!(" – {}", duration_text(total)));
    }
    line
}

/// Every suite path, for showing the tree fully expanded at first (behaviour 1).
pub fn all_suites(report: &TestReport) -> HashSet<RowPath> {
    fn walk(suites: &[TestSuite], prefix: &RowPath, out: &mut HashSet<RowPath>) {
        for (i, s) in suites.iter().enumerate() {
            let mut p = prefix.clone();
            p.push(i);
            walk(&s.suites, &p, out);
            out.insert(p);
        }
    }
    let mut out = HashSet::new();
    out.insert(Vec::new());
    walk(&report.suites, &Vec::new(), &mut out);
    out
}

/// The visible rows: the root "Test Results", then suites and tests of expanded suites; with
/// `show_passed` false, passed tests and suites without other outcomes are left out (behaviour 3).
pub fn rows(report: &TestReport, expanded: &HashSet<RowPath>, show_passed: bool) -> Vec<Row> {
    fn walk(
        suites: &[TestSuite],
        prefix: &RowPath,
        depth: usize,
        expanded: &HashSet<RowPath>,
        show_passed: bool,
        out: &mut Vec<Row>,
    ) {
        for (i, s) in suites.iter().enumerate() {
            let status = status_of(&s.totals);
            if !show_passed && status == TestStatus::Passed {
                continue;
            }
            let mut path = prefix.clone();
            path.push(i);
            let open = expanded.contains(&path);
            out.push(Row {
                path: path.clone(),
                is_test: false,
                depth,
                label: s.name.clone(),
                status,
                duration: s.duration.map(duration_text),
                has_children: !s.suites.is_empty() || !s.tests.is_empty(),
                expanded: open,
            });
            if open {
                walk(&s.suites, &path, depth + 1, expanded, show_passed, out);
                for (j, case) in s.tests.iter().enumerate() {
                    if !show_passed && case.status == TestStatus::Passed {
                        continue;
                    }
                    let mut p = path.clone();
                    p.push(s.suites.len() + j);
                    out.push(Row {
                        path: p,
                        is_test: true,
                        depth: depth + 1,
                        label: case.name.clone(),
                        status: case.status,
                        duration: case.duration.map(duration_text),
                        has_children: false,
                        expanded: false,
                    });
                }
            }
        }
    }
    let root_open = expanded.contains(&Vec::new());
    let mut out = vec![Row {
        path: Vec::new(),
        is_test: false,
        depth: 0,
        label: "Test Results".into(),
        status: status_of(&report.totals),
        duration: None,
        has_children: !report.suites.is_empty(),
        expanded: root_open,
    }];
    if root_open {
        walk(
            &report.suites,
            &Vec::new(),
            1,
            expanded,
            show_passed,
            &mut out,
        );
    }
    out
}

/// The order of the tree (`SPEC-0088` v3 behaviour 8): as the report gives it, by name, or by
/// duration with the longest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// The runner's order.
    #[default]
    Report,
    /// Alphabetical, case ignored.
    Name,
    /// Longest first; without a duration last.
    Duration,
}

impl Sort {
    /// The next order of the toolbar's sort button: report → name → duration → report.
    pub fn next(self) -> Self {
        match self {
            Sort::Report => Sort::Name,
            Sort::Name => Sort::Duration,
            Sort::Duration => Sort::Report,
        }
    }

    /// The button's tooltip after the name: what a click does now.
    pub fn label(self) -> &'static str {
        match self {
            Sort::Report => "Sort Alphabetically",
            Sort::Name => "Sort by Duration",
            Sort::Duration => "Sort as Run",
        }
    }
}

/// `report` with its suites and tests in the order `by`, at every level.
pub fn sorted(report: &TestReport, by: Sort) -> TestReport {
    fn key_duration(d: Option<f64>) -> std::cmp::Reverse<u64> {
        std::cmp::Reverse(d.map_or(0, |s| (s * 1_000_000.0) as u64 + 1))
    }
    fn sort_suite(s: &mut TestSuite, by: Sort) {
        for sub in &mut s.suites {
            sort_suite(sub, by);
        }
        match by {
            Sort::Report => {}
            Sort::Name => {
                s.suites.sort_by_key(|x| x.name.to_lowercase());
                s.tests.sort_by_key(|t| t.name.to_lowercase());
            }
            Sort::Duration => {
                s.suites.sort_by_key(|x| key_duration(x.duration));
                s.tests.sort_by_key(|t| key_duration(t.duration));
            }
        }
    }
    let mut out = report.clone();
    let mut root = TestSuite {
        name: String::new(),
        duration: None,
        tests: Vec::new(),
        suites: std::mem::take(&mut out.suites),
        output: None,
        totals: out.totals,
    };
    sort_suite(&mut root, by);
    out.suites = root.suites;
    out
}

/// `report` with a top suite that bears only the runner's name (`pytest`) named after what ran
/// (`SPEC-0088` v3 behaviour 10, OBS-0034 item 63): the module its tests share (`test_prices`),
/// else the configuration's file (`test_prices.py::test_total` → `test_prices`).
pub fn named_after(report: &TestReport, config: &str) -> TestReport {
    let mut out = report.clone();
    for s in &mut out.suites {
        if s.name != "pytest" {
            continue;
        }
        let module = |class: &str| -> String {
            class
                .split('.')
                .take_while(|p| !p.starts_with(|c: char| c.is_uppercase()))
                .last()
                .unwrap_or(class)
                .to_owned()
        };
        let modules: HashSet<String> = s
            .tests
            .iter()
            .filter_map(|t| t.class.as_deref().map(module))
            .collect();
        let file = config.split("::").next().unwrap_or(config);
        let stem = std::path::Path::new(file)
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_else(|| config.to_owned());
        s.name = match modules.len() {
            1 => modules.into_iter().next().unwrap_or(stem),
            _ => stem,
        };
    }
    out
}

/// The time of a remembered run as the history lists it: `2026-11-13 03:44` in UTC.
pub fn run_time(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60
    )
}

/// The test a path points at, if it is a test.
pub fn test_at<'a>(report: &'a TestReport, path: &[usize]) -> Option<&'a TestCase> {
    let (&last, suites) = path.split_last()?;
    let mut list = &report.suites;
    let mut suite: Option<&TestSuite> = None;
    for &i in suites {
        let s = list.get(i)?;
        list = &s.suites;
        suite = Some(s);
    }
    let s = suite?;
    last.checked_sub(s.suites.len())
        .and_then(|j| s.tests.get(j))
}

/// What the right side shows for a selected test (behaviour 4): its message, details and output.
pub fn test_details(case: &TestCase) -> String {
    let mut parts = Vec::new();
    for text in [&case.message, &case.details, &case.output]
        .into_iter()
        .flatten()
    {
        let t = text.trim_end();
        if !t.is_empty() && !parts.contains(&t) {
            parts.push(t);
        }
    }
    if parts.is_empty() {
        return match case.status {
            TestStatus::Passed => "Test passed".into(),
            TestStatus::Skipped => "Test ignored".into(),
            _ => "Test failed".into(),
        };
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_core::{ReportFormat, parse_report};

    const TAP: &str = "TAP version 13\n1..3\nok 1 - adds\nnot ok 2 - divides\n  ---\n  message: expected 2, got 3\n  ...\nok 3 - skips # SKIP no db\n";

    fn junit() -> TestReport {
        parse_report(
            r#"<testsuites><testsuite name="calc" time="0.012">
<testcase name="adds" classname="calc" time="0.002"/>
<testcase name="divides" classname="calc" time="0.010"><failure message="expected 2">trace</failure></testcase>
</testsuite><testsuite name="io" time="0.5"><testcase name="reads" time="0.5"/></testsuite></testsuites>"#,
            Some(ReportFormat::Junit),
        )
        .unwrap()
    }

    #[test]
    fn rows_status_and_filter() {
        let r = junit();
        let all = all_suites(&r);
        let shown: Vec<(usize, String, TestStatus)> = rows(&r, &all, true)
            .into_iter()
            .map(|row| (row.depth, row.label, row.status))
            .collect();
        assert_eq!(
            shown,
            [
                (0, "Test Results".into(), TestStatus::Failed),
                (1, "calc".into(), TestStatus::Failed),
                (2, "adds".into(), TestStatus::Passed),
                (2, "divides".into(), TestStatus::Failed),
                (1, "io".into(), TestStatus::Passed),
                (2, "reads".into(), TestStatus::Passed),
            ]
        );
        let failing: Vec<String> = rows(&r, &all, false)
            .into_iter()
            .map(|row| row.label)
            .collect();
        assert_eq!(failing, ["Test Results", "calc", "divides"]);
        let mut closed = all.clone();
        closed.remove(&vec![0]);
        assert_eq!(rows(&r, &closed, true).len(), 4);
        assert_eq!(
            status_line(&r),
            "Tests failed: 1, passed: 2 of 3 tests – 512 ms"
        );
        let divides = rows(&r, &all, true)[3].path.clone();
        let case = test_at(&r, &divides).unwrap();
        assert_eq!(case.name, "divides");
        assert!(test_details(case).starts_with("expected 2"));
        assert!(test_at(&r, &[0]).is_none());
    }

    #[test]
    fn tap_report_and_durations() {
        let r = parse_report(TAP, Some(ReportFormat::Tap)).unwrap();
        assert_eq!(
            status_line(&r),
            "Tests failed: 1, passed: 1, ignored: 1 of 3 tests"
        );
        assert_eq!(duration_text(0.012), "12 ms");
        assert_eq!(duration_text(1.5), "1 s 500 ms");
        assert_eq!(duration_text(2.0), "2 s");
        assert_eq!(duration_text(75.0), "1 m 15 s");
    }

    fn case(name: &str, class: &str, d: f64) -> TestCase {
        TestCase {
            name: name.into(),
            class: Some(class.into()),
            status: TestStatus::Passed,
            duration: Some(d),
            message: None,
            details: None,
            output: None,
        }
    }

    fn pytest_report() -> TestReport {
        TestReport {
            format: idecraft_core::ReportFormat::Junit,
            suites: vec![TestSuite {
                name: "pytest".into(),
                duration: Some(0.3),
                tests: vec![
                    case("test_total", "tests.test_prices", 0.01),
                    case("Test_b", "tests.test_prices", 0.2),
                    case("test_a", "tests.test_prices.TestCart", 0.05),
                ],
                suites: Vec::new(),
                output: None,
                totals: Totals::default(),
            }],
            totals: Totals::default(),
        }
    }

    /// SPEC-0088 v3 behaviours 8 and 10 (OBS-0034 items 63, 64): the top node is named after the
    /// tests' module, and the tree sorts by name and by duration.
    #[test]
    fn the_tree_is_named_after_its_file_and_sorts() {
        let r = named_after(&pytest_report(), "test_prices.py");
        assert_eq!(r.suites[0].name, "test_prices");
        let names = |r: &TestReport| -> Vec<String> {
            r.suites[0].tests.iter().map(|t| t.name.clone()).collect()
        };
        assert_eq!(
            names(&sorted(&r, Sort::Report)),
            ["test_total", "Test_b", "test_a"]
        );
        assert_eq!(
            names(&sorted(&r, Sort::Name)),
            ["test_a", "Test_b", "test_total"]
        );
        assert_eq!(
            names(&sorted(&r, Sort::Duration)),
            ["Test_b", "test_a", "test_total"]
        );
        assert_eq!(Sort::Report.next().next().next(), Sort::Report);
        assert_eq!(run_time(1_794_541_440), "2026-11-13 03:44");
        assert_eq!(run_time(0), "1970-01-01 00:00");
        // Tests of several modules: the configuration's file.
        let mut mixed = pytest_report();
        mixed.suites[0].tests[0].class = Some("tests.test_cart".into());
        assert_eq!(
            named_after(&mixed, "tests/test_prices.py::test_total").suites[0].name,
            "test_prices"
        );
        // Other runners keep their suites' names.
        let mut junit = pytest_report();
        junit.suites[0].name = "shop.CartTest".into();
        assert_eq!(
            named_after(&junit, "CartTest").suites[0].name,
            "shop.CartTest"
        );
    }
}
