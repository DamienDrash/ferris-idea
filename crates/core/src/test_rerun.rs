//! Rerun Failed Tests (`SPEC-0088` v3 behaviour 7, OBS-0034 part 15 R2): the configuration of the
//! last test run narrowed to the tests that failed in its report, in each runner's own way to
//! select tests; or why the runner cannot.

use crate::run::RunConfig;
use crate::testreport::{TestCase, TestReport, TestStatus, TestSuite};

/// The tests that failed or had an error, each with the names of the suites above it.
fn failed(report: &TestReport) -> Vec<(Vec<String>, TestCase)> {
    fn walk(s: &TestSuite, above: &mut Vec<String>, out: &mut Vec<(Vec<String>, TestCase)>) {
        above.push(s.name.clone());
        for c in &s.tests {
            if matches!(c.status, TestStatus::Failed | TestStatus::Error) {
                out.push((above.clone(), c.clone()));
            }
        }
        for sub in &s.suites {
            walk(sub, above, out);
        }
        above.pop();
    }
    let mut out = Vec::new();
    for s in &report.suites {
        walk(s, &mut Vec::new(), &mut out);
    }
    out
}

fn has(command: &[String], word: &str) -> bool {
    command
        .iter()
        .any(|w| w == word || w.ends_with(&format!("/{word}")))
}

/// The pytest node id of a failed test: its file from the class's module part (`tests.test_prices`
/// → `tests/test_prices.py`, the classes that follow — names starting with a capital — kept),
/// matched against the file given in the command when there is one.
fn pytest_node(case: &TestCase, files: &[String]) -> Option<String> {
    let class = case.class.as_deref()?;
    let parts: Vec<&str> = class.split('.').collect();
    let first_class = parts
        .iter()
        .position(|p| p.starts_with(|c: char| c.is_uppercase()))
        .unwrap_or(parts.len());
    let module = parts[..first_class].join("/");
    let classes = &parts[first_class..];
    // The file as the command named it: its module ends like the report's.
    let file = files
        .iter()
        .find(|f| {
            let m = f.trim_end_matches(".py");
            m == module || m.ends_with(&format!("/{module}")) || module.ends_with(m)
        })
        .cloned()
        .unwrap_or_else(|| format!("{module}.py"));
    let mut id = vec![file];
    id.extend(classes.iter().map(|c| c.to_string()));
    id.push(case.name.clone());
    Some(id.join("::"))
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `config` narrowed to the tests that failed in `report`, named `Failed tests of <name>`; or the
/// reason why that is not possible: nothing failed, or the runner cannot select single tests.
pub fn rerun_failed(config: &RunConfig, report: &TestReport) -> Result<RunConfig, String> {
    let failed = failed(report);
    if failed.is_empty() {
        return Err("No test failed in the last run".into());
    }
    let mut c = config.clone();
    c.name = format!("Failed tests of {}", config.name);
    let command = &config.command;
    let mut out: Vec<String> = Vec::new();
    if has(command, "pytest") {
        // The files of the command go; the failed tests' node ids come instead.
        let is_path = |w: &str| w.ends_with(".py") || w.contains(".py::");
        let files: Vec<String> = command
            .iter()
            .filter(|w| is_path(w))
            .map(|w| w.split("::").next().unwrap_or(w).to_owned())
            .collect();
        out.extend(command.iter().filter(|w| !is_path(w)).cloned());
        for (_, case) in &failed {
            let node = pytest_node(case, &files)
                .ok_or("pytest's report names no module for a failed test")?;
            if !out.contains(&node) {
                out.push(node);
            }
        }
    } else if has(command, "mvn") || has(command, "mvnw") {
        let tests: Vec<String> = failed
            .iter()
            .filter_map(|(_, case)| Some(format!("{}#{}", case.class.as_deref()?, case.name)))
            .collect();
        if tests.len() != failed.len() {
            return Err("the report names no class for a failed test".into());
        }
        out.extend(
            command
                .iter()
                .filter(|w| !w.starts_with("-Dtest="))
                .cloned(),
        );
        out.push(format!("-Dtest={}", tests.join(",")));
    } else if has(command, "gradle") || has(command, "gradlew") {
        let mut skip = false;
        for w in command {
            if skip {
                skip = false;
                continue;
            }
            if w == "--tests" {
                skip = true;
                continue;
            }
            out.push(w.clone());
        }
        for (_, case) in &failed {
            let class = case
                .class
                .as_deref()
                .ok_or("the report names no class for a failed test")?;
            out.push("--tests".into());
            out.push(format!("{class}.{}", case.name));
        }
    } else if has(command, "vitest") || has(command, "jest") {
        // `-t` is matched against `suite … test`; a failed test's title ends it.
        let mut skip = false;
        for w in command {
            if skip {
                skip = false;
                continue;
            }
            if w == "-t" || w == "--testNamePattern" {
                skip = true;
                continue;
            }
            out.push(w.clone());
        }
        let mut titles: Vec<String> = failed.iter().map(|(_, c)| escape(&c.name)).collect();
        titles.dedup();
        out.push("-t".into());
        out.push(format!("(?:{})$", titles.join("|")));
    } else if command.first().is_some_and(|w| w == "go") && has(command, "test") {
        let mut skip = false;
        for w in command {
            if skip {
                skip = false;
                continue;
            }
            if w == "-run" {
                skip = true;
                continue;
            }
            out.push(w.clone());
        }
        let mut names: Vec<String> = failed
            .iter()
            .map(|(_, c)| escape(c.name.split('/').next().unwrap_or(&c.name)))
            .collect();
        names.dedup();
        let at = out.iter().position(|w| w == "-json").map_or(2, |i| i + 1);
        out.insert(at, "-run".into());
        out.insert(at + 1, format!("^(?:{})$", names.join("|")));
    } else if command.first().is_some_and(|w| w == "cargo") && has(command, "test") {
        // Cargo's own options stay; the filters and libtest's options after `--` are replaced.
        const VALUED: &[&str] = &[
            "--test",
            "-p",
            "--package",
            "--bin",
            "--example",
            "--bench",
            "--features",
            "--manifest-path",
            "-j",
            "--target",
            "--profile",
        ];
        let mut it = command.iter();
        while let Some(w) = it.next() {
            if w == "--" {
                break;
            }
            if w.starts_with('-') || out.len() < 2 {
                out.push(w.clone());
                if VALUED.contains(&w.as_str())
                    && let Some(v) = it.next()
                {
                    out.push(v.clone());
                }
            }
        }
        out.extend(["--".into(), "--exact".into()]);
        for (_, case) in &failed {
            let full = match &case.class {
                Some(class) => format!("{class}::{}", case.name),
                None => case.name.clone(),
            };
            if !out.contains(&full) {
                out.push(full);
            }
        }
    } else {
        let runner = command.first().map_or("This runner", String::as_str);
        return Err(format!("{runner} cannot run single tests again"));
    }
    c.command = out;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::RunKind;
    use crate::testreport::Totals;

    fn config(command: &[&str]) -> RunConfig {
        RunConfig {
            name: "tests".into(),
            command: command.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            env: Default::default(),
            kind: RunKind::Test,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: None,
            verbatim: false,
            http: None,
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
        }
    }

    fn case(class: Option<&str>, name: &str, status: TestStatus) -> TestCase {
        TestCase {
            name: name.into(),
            class: class.map(str::to_owned),
            status,
            duration: Some(0.01),
            message: None,
            details: None,
            output: None,
        }
    }

    fn report(cases: Vec<TestCase>) -> TestReport {
        let suite = TestSuite {
            name: "pytest".into(),
            duration: None,
            tests: cases,
            suites: Vec::new(),
            output: None,
            totals: Totals::default(),
            file: None,
        };
        TestReport {
            format: crate::testreport::ReportFormat::Junit,
            suites: vec![suite],
            totals: Totals::default(),
        }
    }

    fn command(c: &RunConfig) -> Vec<&str> {
        c.command.iter().map(String::as_str).collect()
    }

    /// OBS-0034 item 64 (R2): only `test_fails_on_purpose` runs again.
    #[test]
    fn pytest_reruns_the_failed_node_ids() {
        let r = report(vec![
            case(Some("tests.test_prices"), "test_total", TestStatus::Passed),
            case(
                Some("tests.test_prices"),
                "test_fails_on_purpose",
                TestStatus::Failed,
            ),
            case(
                Some("tests.test_prices.TestCart"),
                "test_empty",
                TestStatus::Error,
            ),
        ]);
        let c = config(&[
            "python3",
            "-m",
            "pytest",
            "tests/test_prices.py",
            "--junitxml=.ferris-idea/pytest.xml",
        ]);
        let again = rerun_failed(&c, &r).unwrap();
        assert_eq!(
            command(&again),
            [
                "python3",
                "-m",
                "pytest",
                "--junitxml=.ferris-idea/pytest.xml",
                "tests/test_prices.py::test_fails_on_purpose",
                "tests/test_prices.py::TestCart::test_empty",
            ]
        );
        assert_eq!(again.name, "Failed tests of tests");
        // A whole project's run: the file from the module.
        let whole = config(&["pytest"]);
        assert_eq!(
            command(&rerun_failed(&whole, &r).unwrap())[1..],
            [
                "tests/test_prices.py::test_fails_on_purpose",
                "tests/test_prices.py::TestCart::test_empty"
            ]
        );
    }

    #[test]
    fn every_runner_selects_its_failed_tests() {
        let r = report(vec![
            case(Some("shop.CartTest"), "adds", TestStatus::Failed),
            case(Some("shop.CartTest"), "keeps", TestStatus::Passed),
            case(Some("shop.PriceTest"), "rounds", TestStatus::Failed),
        ]);
        let maven = rerun_failed(&config(&["mvn", "test", "-Dtest=CartTest"]), &r).unwrap();
        assert_eq!(
            command(&maven),
            [
                "mvn",
                "test",
                "-Dtest=shop.CartTest#adds,shop.PriceTest#rounds"
            ]
        );
        let gradle = rerun_failed(
            &config(&["./gradlew", "test", "--tests", "shop.CartTest"]),
            &r,
        )
        .unwrap();
        assert_eq!(
            command(&gradle),
            [
                "./gradlew",
                "test",
                "--tests",
                "shop.CartTest.adds",
                "--tests",
                "shop.PriceTest.rounds"
            ]
        );
        let js = report(vec![
            case(None, "adds (1+1)", TestStatus::Failed),
            case(None, "keeps", TestStatus::Passed),
        ]);
        let vitest = rerun_failed(
            &config(&["npx", "vitest", "run", "src/a.test.ts", "-t", "old"]),
            &js,
        )
        .unwrap();
        assert_eq!(
            command(&vitest),
            [
                "npx",
                "vitest",
                "run",
                "src/a.test.ts",
                "-t",
                "(?:adds \\(1\\+1\\))$"
            ]
        );
        let go = report(vec![
            case(Some("shop"), "TestTotal", TestStatus::Failed),
            case(Some("shop"), "TestTotal/zero", TestStatus::Failed),
        ]);
        let gotest = rerun_failed(
            &config(&["go", "test", "-json", "-run", "^X$", "./shop"]),
            &go,
        )
        .unwrap();
        assert_eq!(
            command(&gotest),
            ["go", "test", "-json", "-run", "^(?:TestTotal)$", "./shop"]
        );
        let rs = report(vec![
            case(Some("tests"), "adds", TestStatus::Failed),
            case(None, "top", TestStatus::Failed),
        ]);
        let cargo = rerun_failed(
            &config(&["cargo", "test", "--test", "it", "cart", "--", "--nocapture"]),
            &rs,
        )
        .unwrap();
        assert_eq!(
            command(&cargo),
            [
                "cargo",
                "test",
                "--test",
                "it",
                "--",
                "--exact",
                "tests::adds",
                "top"
            ]
        );
    }

    #[test]
    fn refusals_say_why() {
        let r = report(vec![case(Some("a"), "t", TestStatus::Passed)]);
        assert_eq!(
            rerun_failed(&config(&["pytest"]), &r).unwrap_err(),
            "No test failed in the last run"
        );
        let r = report(vec![case(Some("a"), "t", TestStatus::Failed)]);
        assert_eq!(
            rerun_failed(&config(&["make", "check"]), &r).unwrap_err(),
            "make cannot run single tests again"
        );
        let r = report(vec![case(None, "t", TestStatus::Failed)]);
        assert!(rerun_failed(&config(&["mvn", "test"]), &r).is_err());
    }
}
