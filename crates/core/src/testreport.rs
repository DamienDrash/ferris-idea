//! Test reports (JUnit XML, TAP, libtest, Jest/Vitest JSON, `go test -json`) as one result tree
//! (`SPEC-0045`).

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

/// Most "planned but not run" entries listed one by one for a TAP plan; beyond, one entry says how
/// many are missing.
const MAX_MISSING_PLANNED: usize = 1000;

/// Report format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    /// JUnit XML.
    Junit,
    /// Test Anything Protocol.
    Tap,
    /// Plain output of Rust's libtest (`cargo test`).
    Libtest,
    /// The JSON report of Jest (`--json`) and Vitest (`--reporter=json`).
    Jest,
    /// The event stream of `go test -json`.
    Gotest,
}

impl ReportFormat {
    /// Parses a format name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "junit" | "xml" => Some(Self::Junit),
            "tap" => Some(Self::Tap),
            "libtest" | "cargo" | "rust" => Some(Self::Libtest),
            "jest" | "vitest" | "jest-json" => Some(Self::Jest),
            "gotest" | "go" | "go-json" => Some(Self::Gotest),
            _ => None,
        }
    }

    /// Detects the format of `text` (behaviour 1).
    pub fn detect(text: &str) -> Option<Self> {
        let t = text.trim_start();
        if t.starts_with("<?xml") || t.starts_with("<testsuite") {
            return Some(Self::Junit);
        }
        if t.starts_with('{') && t.contains("\"testResults\"") {
            return Some(Self::Jest);
        }
        if text
            .lines()
            .any(|l| l.starts_with('{') && l.contains("\"Action\":") && l.contains("\"Package\":"))
        {
            return Some(Self::Gotest);
        }
        let lines = || text.lines().map(str::trim);
        if lines()
            .any(|l| l.starts_with("running ") && (l.ends_with(" tests") || l.ends_with(" test")))
            || lines().any(|l| l.starts_with("test ") && l.contains(" ... "))
        {
            return Some(Self::Libtest);
        }
        if lines().any(|l| {
            l.starts_with("TAP version")
                || (l.starts_with("1..") && l[3..].trim().chars().all(|c| c.is_ascii_digit()))
                || l.starts_with("ok ")
                || l.starts_with("not ok")
        }) {
            return Some(Self::Tap);
        }
        None
    }
}

/// Outcome of a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    /// Passed.
    Passed,
    /// An assertion failed.
    Failed,
    /// The test could not run properly.
    Error,
    /// Skipped, ignored or a failing TODO.
    Skipped,
}

/// One test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestCase {
    /// Name.
    pub name: String,
    /// Class or module, if the format gives one.
    pub class: Option<String>,
    /// Outcome.
    pub status: TestStatus,
    /// Seconds, if known.
    pub duration: Option<f64>,
    /// Short failure or skip message.
    pub message: Option<String>,
    /// Details (stack trace, diagnostics).
    pub details: Option<String>,
    /// Captured output.
    pub output: Option<String>,
}

/// Counts of a subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Totals {
    /// All tests.
    pub tests: usize,
    /// Passed.
    pub passed: usize,
    /// Failed.
    pub failed: usize,
    /// Errors.
    pub errors: usize,
    /// Skipped.
    pub skipped: usize,
}

impl Totals {
    fn add(&mut self, other: Totals) {
        self.tests += other.tests;
        self.passed += other.passed;
        self.failed += other.failed;
        self.errors += other.errors;
        self.skipped += other.skipped;
    }
}

/// A suite of tests and nested suites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TestSuite {
    /// Name.
    pub name: String,
    /// Seconds, if known.
    pub duration: Option<f64>,
    /// Tests directly in this suite.
    pub tests: Vec<TestCase>,
    /// Nested suites.
    pub suites: Vec<TestSuite>,
    /// Output of the suite itself.
    pub output: Option<String>,
    /// Counts of the whole subtree.
    pub totals: Totals,
}

impl TestSuite {
    fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Recomputes the counts of the subtree.
    fn finish(&mut self) -> Totals {
        let mut t = Totals::default();
        for case in &self.tests {
            t.tests += 1;
            match case.status {
                TestStatus::Passed => t.passed += 1,
                TestStatus::Failed => t.failed += 1,
                TestStatus::Error => t.errors += 1,
                TestStatus::Skipped => t.skipped += 1,
            }
        }
        for s in &mut self.suites {
            t.add(s.finish());
        }
        self.totals = t;
        t
    }
}

/// A parsed report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestReport {
    /// Format read.
    pub format: ReportFormat,
    /// Top-level suites.
    pub suites: Vec<TestSuite>,
    /// Counts of everything.
    pub totals: Totals,
}

/// A report that cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// The format is unknown.
    #[error("cannot tell the report format; give junit, tap, libtest, jest or gotest")]
    UnknownFormat,
    /// The JSON is malformed.
    #[error("malformed JSON report: {0}")]
    Json(String),
    /// The XML is malformed.
    #[error("malformed JUnit XML at byte {position}: {message}")]
    Xml {
        /// Byte offset.
        position: u64,
        /// Parser message.
        message: String,
    },
}

/// Reads `text` in `format` (or the detected one).
pub fn parse_report(text: &str, format: Option<ReportFormat>) -> Result<TestReport, ReportError> {
    let format = format
        .or_else(|| ReportFormat::detect(text))
        .ok_or(ReportError::UnknownFormat)?;
    let mut suites = match format {
        ReportFormat::Junit => junit(text)?,
        ReportFormat::Tap => vec![tap(text)],
        ReportFormat::Libtest => libtest(text),
        ReportFormat::Jest => jest(text)?,
        ReportFormat::Gotest => gotest(text),
    };
    let mut totals = Totals::default();
    for s in &mut suites {
        totals.add(s.finish());
    }
    Ok(TestReport {
        format,
        suites,
        totals,
    })
}

fn append(slot: &mut Option<String>, text: &str) {
    if text.is_empty() {
        return;
    }
    slot.get_or_insert_with(String::new).push_str(text);
}

/// Whether `path` exists and was modified at or after `since` (two seconds of clock slack).
pub fn written_since(path: &std::path::Path, since: std::time::SystemTime) -> bool {
    let since = since - std::time::Duration::from_secs(2);
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t >= since)
}

/// Reads the JUnit files (`*.xml`) of `dir` modified at or after `since` into one report, files in
/// name order; a folder without such files is an error (`SPEC-0097` behaviour 2).
pub fn read_junit_dir(
    dir: &std::path::Path,
    since: std::time::SystemTime,
) -> Result<TestReport, String> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .filter(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                // File times can be coarser than the clock; allow two seconds.
                .is_ok_and(|t| t + std::time::Duration::from_secs(2) >= since)
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err("no test report written by this run".into());
    }
    let mut merged = TestReport {
        format: ReportFormat::Junit,
        suites: Vec::new(),
        totals: Totals::default(),
    };
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
        let report = parse_report(&text, Some(ReportFormat::Junit))
            .map_err(|e| format!("{}: {e}", f.display()))?;
        merged.totals.add(report.totals);
        merged.suites.extend(report.suites);
    }
    Ok(merged)
}

/// Where text inside JUnit elements goes.
#[derive(Clone, Copy, PartialEq)]
enum Sink {
    None,
    Details,
    Out,
    Err,
}

fn junit(xml: &str) -> Result<Vec<TestSuite>, ReportError> {
    let mut reader = Reader::from_str(xml);
    let err = |reader: &Reader<&[u8]>, e: &dyn std::fmt::Display| ReportError::Xml {
        position: reader.error_position(),
        message: e.to_string(),
    };
    let mut stack: Vec<TestSuite> = vec![TestSuite::named("")];
    let mut case: Option<TestCase> = None;
    let mut sink = Sink::None;
    loop {
        let event = reader.read_event().map_err(|e| err(&reader, &e))?;
        let (tag, empty) = match &event {
            Event::Start(t) => (Some(t.clone()), false),
            Event::Empty(t) => (Some(t.clone()), true),
            Event::End(t) => {
                match t.name().as_ref() {
                    b"testsuite" if stack.len() > 1 => {
                        let done = stack.pop().unwrap_or_default();
                        if let Some(parent) = stack.last_mut() {
                            parent.suites.push(done);
                        }
                    }
                    b"testcase" => {
                        if let (Some(c), Some(s)) = (case.take(), stack.last_mut()) {
                            s.tests.push(c);
                        }
                    }
                    b"failure" | b"error" | b"skipped" | b"system-out" | b"system-err" => {
                        sink = Sink::None
                    }
                    _ => {}
                }
                continue;
            }
            Event::Text(t) => {
                let text = t.decode().map_err(|e| err(&reader, &e))?;
                junit_text(&mut stack, &mut case, sink, &text);
                continue;
            }
            Event::CData(t) => {
                let text = String::from_utf8_lossy(t);
                junit_text(&mut stack, &mut case, sink, &text);
                continue;
            }
            Event::GeneralRef(r) => {
                let text = if r.is_char_ref() {
                    r.resolve_char_ref()
                        .map_err(|e| err(&reader, &e))?
                        .map(String::from)
                        .unwrap_or_default()
                } else {
                    match &**r {
                        b"lt" => "<",
                        b"gt" => ">",
                        b"amp" => "&",
                        b"quot" => "\"",
                        b"apos" => "'",
                        _ => "",
                    }
                    .to_owned()
                };
                junit_text(&mut stack, &mut case, sink, &text);
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let Some(tag) = tag else { continue };
        let mut attrs = BTreeMap::new();
        for a in tag.attributes() {
            let a = a.map_err(|e| err(&reader, &e))?;
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| err(&reader, &e))?;
            attrs.insert(
                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                value.into_owned(),
            );
        }
        let get = |k: &str| attrs.get(k).cloned();
        let time = || attrs.get("time").and_then(|t| t.parse::<f64>().ok());
        match tag.name().as_ref() {
            b"testsuite" => {
                let mut s = TestSuite::named(get("name").unwrap_or_default());
                s.duration = time();
                if empty {
                    if let Some(parent) = stack.last_mut() {
                        parent.suites.push(s);
                    }
                } else {
                    stack.push(s);
                }
            }
            b"testcase" => {
                let c = TestCase {
                    name: get("name").unwrap_or_default(),
                    class: get("classname"),
                    status: TestStatus::Passed,
                    duration: time(),
                    message: None,
                    details: None,
                    output: None,
                };
                if empty {
                    if let Some(s) = stack.last_mut() {
                        s.tests.push(c);
                    }
                } else {
                    case = Some(c);
                }
            }
            name @ (b"failure" | b"error" | b"skipped") => {
                if let Some(c) = case.as_mut() {
                    c.status = match name {
                        b"failure" => TestStatus::Failed,
                        b"error" => TestStatus::Error,
                        _ => TestStatus::Skipped,
                    };
                    c.message = get("message").or(c.message.take());
                }
                sink = if empty { Sink::None } else { Sink::Details };
            }
            b"system-out" if !empty => sink = Sink::Out,
            b"system-err" if !empty => sink = Sink::Err,
            _ => {}
        }
    }
    // The implicit root holds top-level suites (or tests outside any suite).
    let mut root = stack.into_iter().next().unwrap_or_default();
    if !root.tests.is_empty() {
        let loose = TestSuite {
            name: "tests".into(),
            tests: std::mem::take(&mut root.tests),
            ..TestSuite::default()
        };
        root.suites.insert(0, loose);
    }
    Ok(root.suites)
}

fn junit_text(stack: &mut [TestSuite], case: &mut Option<TestCase>, sink: Sink, text: &str) {
    match (case.as_mut(), sink) {
        (Some(c), Sink::Details) => append(&mut c.details, text),
        (Some(c), Sink::Out | Sink::Err) => append(&mut c.output, text),
        (None, Sink::Out | Sink::Err) => {
            if let Some(s) = stack.last_mut() {
                append(&mut s.output, text);
            }
        }
        _ => {}
    }
}

/// Jest/Vitest JSON (behaviour 4): one suite per test file, named relative to the folder all files
/// share; `describe` blocks become nested suites. A file that failed without tests (a syntax error)
/// becomes one error.
fn jest(text: &str) -> Result<Vec<TestSuite>, ReportError> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(text).map_err(|e| ReportError::Json(e.to_string()))?;
    let files = v["testResults"].as_array().cloned().unwrap_or_default();
    let names: Vec<&str> = files.iter().filter_map(|f| f["name"].as_str()).collect();
    let common = names.iter().fold(None::<&str>, |acc, n| {
        let dir = n.rfind('/').map_or("", |i| &n[..=i]);
        Some(match acc {
            None => dir,
            Some(a) => {
                let mut end = a.len().min(dir.len());
                while !dir.starts_with(&a[..end]) || !a.is_char_boundary(end) {
                    end -= 1;
                }
                a[..end].rfind('/').map_or("", |i| &a[..=i])
            }
        })
    });
    let mut out = Vec::new();
    for f in &files {
        let full = f["name"].as_str().unwrap_or("tests");
        let name = full.strip_prefix(common.unwrap_or("")).unwrap_or(full);
        let mut suite = TestSuite::named(name);
        if let (Some(a), Some(b)) = (f["startTime"].as_f64(), f["endTime"].as_f64()) {
            suite.duration = Some((b - a).max(0.0) / 1000.0);
        }
        let cases = f["assertionResults"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for c in &cases {
            let status = match c["status"].as_str().unwrap_or("") {
                "passed" => TestStatus::Passed,
                "failed" => TestStatus::Failed,
                _ => TestStatus::Skipped,
            };
            let failures: Vec<&str> = c["failureMessages"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let details = (!failures.is_empty()).then(|| failures.join("\n"));
            let case = TestCase {
                name: c["title"].as_str().unwrap_or("test").to_owned(),
                class: None,
                status,
                duration: c["duration"].as_f64().map(|ms| ms / 1000.0),
                message: details
                    .as_deref()
                    .and_then(|d| d.lines().find(|l| !l.trim().is_empty()))
                    .map(|l| l.trim().to_owned()),
                details,
                output: None,
            };
            let mut at = &mut suite;
            for title in c["ancestorTitles"].as_array().into_iter().flatten() {
                let title = title.as_str().unwrap_or("");
                let i = match at.suites.iter().position(|s| s.name == title) {
                    Some(i) => i,
                    None => {
                        at.suites.push(TestSuite::named(title));
                        at.suites.len() - 1
                    }
                };
                at = &mut at.suites[i];
            }
            at.tests.push(case);
        }
        if cases.is_empty() && f["status"].as_str() == Some("failed") {
            let message = f["message"].as_str().unwrap_or("").to_owned();
            suite.tests.push(TestCase {
                name: "Test suite failed to run".to_owned(),
                class: None,
                status: TestStatus::Error,
                duration: None,
                message: message
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty() && !l.starts_with('●'))
                    .map(str::to_owned),
                details: (!message.is_empty()).then_some(message),
                output: None,
            });
        }
        out.push(suite);
    }
    Ok(out)
}

/// `go test -json` (behaviour 4): one suite per package; subtests (`TestA/case`) nest under their
/// test as suites; a package that failed without tests (a build error) becomes one error with its
/// output. Lines that are not JSON events (build output) are ignored.
fn gotest(text: &str) -> Vec<TestSuite> {
    use serde_json::Value;
    #[derive(Default)]
    struct Run {
        status: Option<TestStatus>,
        elapsed: Option<f64>,
        output: String,
    }
    // Package name, its tests by name, and the package's own events.
    type Package = (String, Vec<(String, Run)>, Run);
    let mut packages: Vec<Package> = Vec::new();
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(package) = e["Package"].as_str() else {
            continue;
        };
        let p = match packages.iter().position(|(n, ..)| n == package) {
            Some(i) => i,
            None => {
                packages.push((package.to_owned(), Vec::new(), Run::default()));
                packages.len() - 1
            }
        };
        let (_, tests, own) = &mut packages[p];
        let run = match e["Test"].as_str() {
            Some(t) => match tests.iter().position(|(n, _)| n == t) {
                Some(i) => &mut tests[i].1,
                None => {
                    tests.push((t.to_owned(), Run::default()));
                    &mut tests.last_mut().expect("just pushed").1
                }
            },
            None => own,
        };
        match e["Action"].as_str().unwrap_or("") {
            "output" => run.output.push_str(e["Output"].as_str().unwrap_or("")),
            action @ ("pass" | "fail" | "skip") => {
                run.status = Some(match action {
                    "pass" => TestStatus::Passed,
                    "fail" => TestStatus::Failed,
                    _ => TestStatus::Skipped,
                });
                run.elapsed = e["Elapsed"].as_f64();
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (name, tests, own) in packages {
        let mut suite = TestSuite::named(name);
        suite.duration = own.elapsed;
        for (test, run) in &tests {
            let status = run.status.unwrap_or(TestStatus::Error);
            let failed = matches!(status, TestStatus::Failed | TestStatus::Error);
            let case = TestCase {
                name: test.rsplit('/').next().unwrap_or(test).to_owned(),
                class: None,
                status,
                duration: run.elapsed,
                message: failed
                    .then(|| {
                        run.output
                            .lines()
                            .map(str::trim)
                            .find(|l| {
                                !l.is_empty() && !l.starts_with("=== ") && !l.starts_with("--- ")
                            })
                            .map(str::to_owned)
                    })
                    .flatten(),
                details: None,
                output: (!run.output.is_empty()).then(|| run.output.clone()),
            };
            // Parents of a subtest are suites; a test that has subtests is listed as a suite only.
            let parts: Vec<&str> = test.split('/').collect();
            let has_children = tests.iter().any(|(t, _)| {
                t.len() > test.len()
                    && t.starts_with(test.as_str())
                    && t.as_bytes()[test.len()] == b'/'
            });
            let mut at = &mut suite;
            for part in &parts[..parts.len() - 1] {
                let i = match at.suites.iter().position(|s| s.name == *part) {
                    Some(i) => i,
                    None => {
                        at.suites.push(TestSuite::named(*part));
                        at.suites.len() - 1
                    }
                };
                at = &mut at.suites[i];
            }
            if has_children {
                let last = parts[parts.len() - 1];
                if !at.suites.iter().any(|s| s.name == last) {
                    let mut s = TestSuite::named(last);
                    s.duration = run.elapsed;
                    at.suites.push(s);
                }
            } else {
                at.tests.push(case);
            }
        }
        if tests.is_empty() && own.status == Some(TestStatus::Failed) {
            suite.tests.push(TestCase {
                name: "build".to_owned(),
                class: None,
                status: TestStatus::Error,
                duration: None,
                message: own
                    .output
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim().to_owned()),
                details: None,
                output: (!own.output.is_empty()).then(|| own.output.clone()),
            });
        }
        if !suite.tests.is_empty() || !suite.suites.is_empty() {
            out.push(suite);
        }
    }
    out
}

/// TAP (behaviour 4); indented subtests become nested suites.
fn tap(text: &str) -> TestSuite {
    let lines: Vec<&str> = text.lines().collect();
    let mut pos = 0;
    tap_level(&lines, &mut pos, 0, "TAP")
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn tap_level(lines: &[&str], pos: &mut usize, level: usize, name: &str) -> TestSuite {
    let mut suite = TestSuite::named(name);
    let mut plan: Option<usize> = None;
    let mut numbered = 0;
    let mut pending_subtest: Option<TestSuite> = None;
    let mut in_yaml = false;
    let mut bailed = false;
    // The last line closed a subtest: comments after it belong to no test.
    let mut after_subtest = false;
    while *pos < lines.len() {
        let raw = lines[*pos];
        if raw.trim().is_empty() {
            *pos += 1;
            continue;
        }
        let ind = indent(raw);
        if ind < level {
            break;
        }
        let line = raw.trim();
        if in_yaml {
            *pos += 1;
            if line == "..." {
                in_yaml = false;
            } else if let Some(last) = suite.tests.last_mut() {
                append(&mut last.details, &format!("{line}\n"));
            }
            continue;
        }
        if ind > level {
            // A subtest block (or a YAML block of the previous test).
            if line == "---" {
                in_yaml = true;
                *pos += 1;
                continue;
            }
            let sub_name = suite
                .tests
                .len()
                .checked_add(1)
                .map_or_else(String::new, |n| format!("subtest {n}"));
            pending_subtest = Some(tap_level(lines, pos, ind, &sub_name));
            continue;
        }
        *pos += 1;
        if let Some(reason) = line.strip_prefix("Bail out!") {
            suite.tests.push(TestCase {
                name: "Bail out!".into(),
                class: None,
                status: TestStatus::Error,
                duration: None,
                message: Some(reason.trim().to_owned()),
                details: None,
                output: None,
            });
            *pos = lines.len();
            bailed = true;
            break;
        }
        if let Some(n) = line.strip_prefix("1..") {
            plan = n.split_whitespace().next().and_then(|n| n.parse().ok());
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            // `# Subtest: name` (TAP 14) announces the next subtest; it is no detail of a test.
            if comment.trim_start().starts_with("Subtest:") || after_subtest {
                continue;
            }
            if let Some(last) = suite.tests.last_mut() {
                append(&mut last.details, &format!("{}\n", comment.trim()));
            }
            continue;
        }
        let (ok, rest) = if let Some(r) = line.strip_prefix("not ok") {
            (false, r)
        } else if let Some(r) = line.strip_prefix("ok") {
            (true, r)
        } else {
            continue;
        };
        let rest = rest.trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            numbered = numbered.max(digits.parse().unwrap_or(0));
        } else {
            numbered += 1;
        }
        let rest = rest[digits.len()..].trim_start();
        let rest = rest.strip_prefix('-').unwrap_or(rest).trim_start();
        let (desc, directive) = match rest
            .split_once(" # ")
            .or_else(|| rest.strip_prefix("# ").map(|d| ("", d)))
        {
            Some((d, dir)) => (d.trim(), Some(dir.trim())),
            None => (rest.trim(), None),
        };
        let upper = directive.map(str::to_ascii_uppercase);
        let (status, message) = match upper.as_deref() {
            Some(d) if d.starts_with("SKIP") => (
                TestStatus::Skipped,
                directive.map(|d| d[4..].trim().to_owned()),
            ),
            Some(d) if d.starts_with("TODO") => (
                if ok {
                    TestStatus::Passed
                } else {
                    TestStatus::Skipped
                },
                directive.map(|d| format!("TODO {}", d[4..].trim()).trim().to_owned()),
            ),
            _ => (
                if ok {
                    TestStatus::Passed
                } else {
                    TestStatus::Failed
                },
                None,
            ),
        };
        let name = if desc.is_empty() {
            format!("test {numbered}")
        } else {
            desc.to_owned()
        };
        if let Some(mut sub) = pending_subtest.take() {
            // The test line after a subtest only closes it and names it; it is not a test of its
            // own. A failing line over a subtest without failures stays visible inside it.
            if status == TestStatus::Failed && !has_failure(&sub) {
                sub.tests.push(TestCase {
                    name: name.clone(),
                    class: None,
                    status,
                    duration: None,
                    message,
                    details: None,
                    output: None,
                });
            }
            sub.name = name;
            suite.suites.push(sub);
            after_subtest = true;
            continue;
        }
        after_subtest = false;
        suite.tests.push(TestCase {
            name,
            class: None,
            status,
            duration: None,
            message: message.filter(|m| !m.is_empty()),
            details: None,
            output: None,
        });
    }
    if let Some(sub) = pending_subtest {
        suite.suites.push(sub);
    }
    if let Some(planned) = plan
        && !bailed
        && numbered < planned
    {
        let missing = |name: String, message: String| TestCase {
            name,
            class: None,
            status: TestStatus::Error,
            duration: None,
            message: Some(message),
            details: None,
            output: None,
        };
        if planned - numbered <= MAX_MISSING_PLANNED {
            for n in numbered + 1..=planned {
                suite
                    .tests
                    .push(missing(format!("test {n}"), "planned but not run".into()));
            }
        } else {
            // A plan far beyond the tests seen (a broken or hostile report, M8.2 fuzz finding)
            // gets one entry instead of millions.
            suite.tests.push(missing(
                format!("tests {}–{planned}", numbered + 1),
                format!("{} planned tests not run", planned - numbered),
            ));
        }
    }
    suite
}

/// Whether `suite` or one nested in it has a failed or erroneous test.
fn has_failure(suite: &TestSuite) -> bool {
    suite
        .tests
        .iter()
        .any(|t| matches!(t.status, TestStatus::Failed | TestStatus::Error))
        || suite.suites.iter().any(has_failure)
}

/// libtest plain output (behaviour 5).
fn libtest(text: &str) -> Vec<TestSuite> {
    let mut suites: Vec<TestSuite> = Vec::new();
    let mut current: Option<TestSuite> = None;
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut binary: Option<String> = None;
    let flush = |current: &mut Option<TestSuite>,
                 suites: &mut Vec<TestSuite>,
                 sections: &mut BTreeMap<String, String>| {
        if let Some(mut s) = current.take() {
            for case in &mut s.tests {
                let key = match &case.class {
                    Some(c) => format!("{c}::{}", case.name),
                    None => case.name.clone(),
                };
                if let Some(out) = sections.remove(&key) {
                    case.output = Some(out);
                }
            }
            suites.push(s);
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if let Some(path) = t.strip_prefix("Running ") {
            let path = path.strip_prefix("unittests ").unwrap_or(path);
            binary = Some(path.split_whitespace().next().unwrap_or(path).to_owned());
            section = None;
            continue;
        }
        if t.starts_with("running ") && (t.ends_with(" tests") || t.ends_with(" test")) {
            flush(&mut current, &mut suites, &mut sections);
            current = Some(TestSuite::named(
                binary.take().unwrap_or_else(|| "tests".into()),
            ));
            section = None;
            continue;
        }
        if let Some(name) = t
            .strip_prefix("---- ")
            .and_then(|r| r.strip_suffix(" ----"))
        {
            let name = name
                .trim_end_matches(" stdout")
                .trim_end_matches(" stderr")
                .to_owned();
            section = Some(name);
            continue;
        }
        if t == "failures:" || t.starts_with("test result:") {
            section = None;
            continue;
        }
        if let Some(name) = &section {
            let entry = sections.entry(name.clone()).or_default();
            entry.push_str(line);
            entry.push('\n');
            continue;
        }
        let Some(rest) = t.strip_prefix("test ") else {
            continue;
        };
        let Some((name, result)) = rest.rsplit_once(" ... ") else {
            continue;
        };
        let (status, message) = match result.trim() {
            "ok" => (TestStatus::Passed, None),
            "FAILED" => (TestStatus::Failed, None),
            r if r.starts_with("ignored") => (
                TestStatus::Skipped,
                r.strip_prefix("ignored")
                    .map(|m| m.trim().trim_start_matches(',').trim().to_owned())
                    .filter(|m| !m.is_empty()),
            ),
            _ => continue,
        };
        let (class, name) = match name.rsplit_once("::") {
            Some((c, n)) => (Some(c.to_owned()), n.to_owned()),
            None => (None, name.to_owned()),
        };
        current
            .get_or_insert_with(|| TestSuite::named("tests"))
            .tests
            .push(TestCase {
                name,
                class,
                status,
                duration: None,
                message,
                details: None,
                output: None,
            });
    }
    flush(&mut current, &mut suites, &mut sections);
    suites
}
