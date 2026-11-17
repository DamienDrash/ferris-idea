//! SPEC-0045 AK-1 – AK-4.

use std::process::Command;

use idecraft_core::{ReportFormat, TestStatus, TestSuite, parse_report};

fn statuses(s: &TestSuite) -> Vec<(String, TestStatus)> {
    s.tests.iter().map(|t| (t.name.clone(), t.status)).collect()
}

const JUNIT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="pkg.MathTest" tests="4" time="0.42">
    <testcase classname="pkg.MathTest" name="adds" time="0.01"/>
    <testcase classname="pkg.MathTest" name="divides" time="0.02">
      <failure message="expected 2 but was 3" type="AssertionError">at MathTest.java:12 &lt;here&gt;</failure>
      <system-out>dividing</system-out>
    </testcase>
    <testcase classname="pkg.MathTest" name="io" time="0.3"><error message="boom"><![CDATA[stack <trace>]]></error></testcase>
    <testcase classname="pkg.MathTest" name="later"><skipped message="not ready"/></testcase>
    <testsuite name="nested">
      <testcase name="inner"/>
    </testsuite>
    <system-out>suite output</system-out>
  </testsuite>
</testsuites>"#;

#[test]
fn junit_report() {
    let r = parse_report(JUNIT, None).unwrap();
    assert_eq!(r.format, ReportFormat::Junit);
    let s = &r.suites[0];
    assert_eq!((s.name.as_str(), s.duration), ("pkg.MathTest", Some(0.42)));
    assert_eq!(
        statuses(s),
        [
            ("adds".into(), TestStatus::Passed),
            ("divides".into(), TestStatus::Failed),
            ("io".into(), TestStatus::Error),
            ("later".into(), TestStatus::Skipped),
        ]
    );
    let div = &s.tests[1];
    assert_eq!(div.message.as_deref(), Some("expected 2 but was 3"));
    assert_eq!(div.details.as_deref(), Some("at MathTest.java:12 <here>"));
    assert_eq!(div.output.as_deref(), Some("dividing"));
    assert_eq!(div.class.as_deref(), Some("pkg.MathTest"));
    assert_eq!(s.tests[2].details.as_deref(), Some("stack <trace>"));
    assert_eq!(s.tests[3].message.as_deref(), Some("not ready"));
    assert_eq!(s.suites[0].tests[0].name, "inner");
    assert_eq!(s.output.as_deref(), Some("suite output"));
    let t = r.totals;
    assert_eq!(
        (t.tests, t.passed, t.failed, t.errors, t.skipped),
        (5, 2, 1, 1, 1)
    );
    assert_eq!(s.totals.tests, 5);

    let bad = parse_report("<testsuite><testcase></testsuite>", None);
    assert!(bad.unwrap_err().to_string().contains("malformed JUnit XML"));
}

#[test]
fn tap_report() {
    let tap = "TAP version 14\n1..6\nok 1 - adds\nnot ok 2 - divides\n  ---\n  message: expected 2\n  ...\nok 3 - network # SKIP offline\nnot ok 4 - unicode # TODO later\n    1..2\n    ok 1 - inner a\n    not ok 2 - inner b\nnot ok 5 - parent\n# a comment for 5\n";
    let r = parse_report(tap, None).unwrap();
    assert_eq!(r.format, ReportFormat::Tap);
    let s = &r.suites[0];
    assert_eq!(
        statuses(s),
        [
            ("adds".into(), TestStatus::Passed),
            ("divides".into(), TestStatus::Failed),
            ("network".into(), TestStatus::Skipped),
            ("unicode".into(), TestStatus::Skipped),
            ("test 6".into(), TestStatus::Error),
        ]
    );
    assert!(
        s.tests[1]
            .details
            .as_deref()
            .unwrap()
            .contains("message: expected 2")
    );
    assert_eq!(s.tests[2].message.as_deref(), Some("offline"));
    assert_eq!(s.tests[3].message.as_deref(), Some("TODO later"));
    // The line after a subtest names it and is no test of its own.
    assert_eq!(s.suites[0].name, "parent");
    assert_eq!(s.suites[0].totals.failed, 1);
    assert_eq!(s.suites[0].tests.len(), 2);
    assert_eq!(s.tests[4].message.as_deref(), Some("planned but not run"));

    // TAP 14 with `# Subtest:` lines; a failing subtest line without failing tests stays visible.
    let tap14 = "TAP version 14\n# Subtest: math\n    1..2\n    ok 1 - adds\n    not ok 2 - divides\n    # expected 2, got 3\nnot ok 1 - math\n# Subtest: text\n    1..1\n    ok 1 - trims\nnot ok 2 - text\n1..2\n";
    let r = parse_report(tap14, None).unwrap();
    let s = &r.suites[0];
    assert!(s.tests.is_empty());
    assert_eq!(s.suites.len(), 2);
    assert_eq!(s.suites[0].name, "math");
    assert_eq!(
        s.suites[0].tests[1].details.as_deref(),
        Some("expected 2, got 3\n")
    );
    assert_eq!(
        statuses(&s.suites[1]),
        [
            ("trims".into(), TestStatus::Passed),
            ("text".into(), TestStatus::Failed)
        ]
    );
    assert_eq!((r.totals.passed, r.totals.failed), (2, 2));

    let bail = parse_report("1..3\nok 1\nBail out! database down\nok 2\n", None).unwrap();
    let names = statuses(&bail.suites[0]);
    assert_eq!(names[1], ("Bail out!".into(), TestStatus::Error));
    assert_eq!(names.len(), 2);
}

#[test]
fn tap_from_perl_test_more() {
    let script = r#"use Test::More tests => 4; ok(1, "first"); ok(0, "second"); SKIP: { skip "no db", 1; ok(1) } TODO: { local $TODO = "later"; ok(0, "fourth") }"#;
    let Ok(out) = Command::new("perl").args(["-e", script]).output() else {
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let r = parse_report(&text, None).unwrap();
    assert_eq!(r.format, ReportFormat::Tap);
    let t = r.totals;
    assert_eq!(
        (t.tests, t.passed, t.failed, t.skipped),
        (4, 1, 1, 2),
        "{text}"
    );
    assert_eq!(r.suites[0].tests[0].name, "first");
}

const LIBTEST: &str = "     Running unittests src/lib.rs (target/debug/deps/demo-1)

running 3 tests
test math::adds ... ok
test math::divides ... FAILED
test slow ... ignored, takes long

failures:

---- math::divides stdout ----
thread 'math::divides' panicked at src/lib.rs:9:5:
assertion failed

failures:
    math::divides

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out

     Running tests/it.rs (target/debug/deps/it-2)

running 1 test
test works ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
";

#[test]
fn libtest_report() {
    let r = parse_report(LIBTEST, None).unwrap();
    assert_eq!(r.format, ReportFormat::Libtest);
    assert_eq!(r.suites.len(), 2);
    assert_eq!(r.suites[0].name, "src/lib.rs");
    assert_eq!(r.suites[1].name, "tests/it.rs");
    let s = &r.suites[0];
    assert_eq!(
        statuses(s),
        [
            ("adds".into(), TestStatus::Passed),
            ("divides".into(), TestStatus::Failed),
            ("slow".into(), TestStatus::Skipped),
        ]
    );
    assert_eq!(s.tests[0].class.as_deref(), Some("math"));
    assert_eq!(s.tests[2].message.as_deref(), Some("takes long"));
    assert!(
        s.tests[1]
            .output
            .as_deref()
            .unwrap()
            .contains("assertion failed")
    );
    assert_eq!(r.totals.tests, 4);
}

#[test]
fn libtest_from_real_cargo() {
    let dir = std::env::temp_dir().join(format!("idecraft-libtest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "#[cfg(test)]\nmod math {\n    #[test]\n    fn adds() { assert_eq!(1 + 1, 2); }\n    #[test]\n    fn divides() { println!(\"dividing\"); assert_eq!(6 / 2, 2); }\n    #[test]\n    #[ignore = \"takes long\"]\n    fn slow() {}\n}\n",
    )
    .unwrap();
    let Ok(out) = Command::new(env!("CARGO"))
        .args([
            "test",
            "--quiet",
            "--offline",
            "--",
            "--test-threads=1",
            "--format=pretty",
        ])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .output()
    else {
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // Without a report the inner build failed; its own messages say why.
    let r = parse_report(&text, None).unwrap_or_else(|e| {
        panic!(
            "{e:?}: no test report from the inner cargo test ({}); its error output:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let t = r.totals;
    assert_eq!(
        (t.tests, t.passed, t.failed, t.skipped),
        (3, 1, 1, 1),
        "{text}"
    );
    let divides = r.suites[0]
        .tests
        .iter()
        .find(|c| c.name == "divides")
        .unwrap();
    assert!(
        divides
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("dividing"),
        "{text}"
    );
    let slow = r.suites[0].tests.iter().find(|c| c.name == "slow").unwrap();
    assert_eq!(slow.message.as_deref(), Some("takes long"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn format_detection() {
    assert_eq!(ReportFormat::detect(JUNIT), Some(ReportFormat::Junit));
    assert_eq!(
        ReportFormat::detect("1..2\nok 1\n"),
        Some(ReportFormat::Tap)
    );
    assert_eq!(ReportFormat::detect(LIBTEST), Some(ReportFormat::Libtest));
    assert_eq!(ReportFormat::detect("hello world"), None);
    assert!(parse_report("hello", None).is_err());
    assert_eq!(
        ReportFormat::from_name("cargo"),
        Some(ReportFormat::Libtest)
    );
}

/// Names and statuses of a suite's tests and of its nested suites, depth first.
fn tree(s: &TestSuite, depth: usize, out: &mut Vec<String>) {
    for t in &s.tests {
        out.push(format!("{}{} {:?}", "  ".repeat(depth), t.name, t.status));
    }
    for n in &s.suites {
        out.push(format!("{}{}/", "  ".repeat(depth), n.name));
        tree(n, depth + 1, out);
    }
}

fn lines(r: &idecraft_core::TestReport) -> Vec<String> {
    let mut out = Vec::new();
    for s in &r.suites {
        out.push(format!("{}/", s.name));
        tree(s, 1, &mut out);
    }
    out
}

/// SPEC-0045 v3 AK-6: a real Vitest JSON report (`vitest run --reporter=json`).
#[test]
fn vitest_report() {
    let text = include_str!("../reports/vitest.json");
    assert_eq!(ReportFormat::detect(text), Some(ReportFormat::Jest));
    let r = parse_report(text, None).unwrap();
    assert_eq!(
        lines(&r),
        [
            "math.test.js/",
            "  top level Passed",
            "  math/",
            "    adds Passed",
            "    fails Failed",
            "    later Skipped",
        ]
    );
    assert_eq!(
        (r.totals.tests, r.totals.failed, r.totals.skipped),
        (4, 1, 1)
    );
    let fails = &r.suites[0].suites[0].tests[1];
    assert!(
        fails
            .message
            .as_deref()
            .unwrap()
            .contains("expected 2 to be 3")
    );
}

/// SPEC-0045 v3 AK-6: a real Jest JSON report (`jest --json`), with a file that does not parse;
/// v6 AK-16: the suites keep their files.
#[test]
fn jest_report() {
    let text = include_str!("../reports/jest.json");
    let r = parse_report(text, Some(ReportFormat::from_name("jest").unwrap())).unwrap();
    assert_eq!(
        lines(&r),
        [
            "sum.test.js/",
            "  sum/",
            "    adds Passed",
            "    fails Failed",
            "broken.test.js/",
            "  Test suite failed to run Error",
        ]
    );
    let broken = &r.suites[1].tests[0];
    assert!(
        broken
            .message
            .as_deref()
            .unwrap()
            .starts_with("Jest encountered")
    );
    // v6 behaviour 8: each file's suite keeps the file the runner named, for the jump to a test
    // (SPEC-0088 v5 behaviour 12); the inner suites name none.
    for suite in &r.suites {
        let file = suite.file.as_deref().expect("the suite names its file");
        assert!(file.ends_with(&format!("/{}", suite.name)), "{file}");
        assert!(suite.suites.iter().all(|s| s.file.is_none()));
    }
}

/// SPEC-0045 v3 AK-7: a real `go test -json` stream with a subtest table.
#[test]
fn gotest_report() {
    let text = include_str!("../reports/gotest.json");
    assert_eq!(ReportFormat::detect(text), Some(ReportFormat::Gotest));
    let r = parse_report(text, None).unwrap();
    assert_eq!(
        lines(&r),
        [
            "demo/",
            "  TestAdd Passed",
            "  TestFail Failed",
            "  TestTable/",
            "    one Passed",
            "    two Skipped",
        ]
    );
    let fail = &r.suites[0].tests[1];
    assert!(fail.message.as_deref().unwrap().contains("want 3, got 2"));
    let build = parse_report(
        "{\"Action\":\"output\",\"Package\":\"bad\",\"Output\":\"x.go:3: undefined: y\\n\"}\n{\"Action\":\"fail\",\"Package\":\"bad\"}\n",
        None,
    )
    .unwrap();
    assert_eq!(build.suites[0].tests[0].status, TestStatus::Error);
}

/// M8.2 fuzz finding: a TAP plan of `1..64666666…` listed every missing test and ran out of
/// memory.
#[test]
fn huge_tap_plan_gives_one_missing_entry() {
    let r = parse_report("1..6466666666666\nok 1 - a\n", Some(ReportFormat::Tap)).unwrap();
    let tests: Vec<_> = r.suites.iter().flat_map(|s| s.tests.iter()).collect();
    assert_eq!(tests.len(), 2);
    assert_eq!(tests[1].status, TestStatus::Error);
    assert_eq!(
        tests[1].message.as_deref(),
        Some("6466666666665 planned tests not run")
    );
    let small = parse_report("1..3\nok 1 - a\n", Some(ReportFormat::Tap)).unwrap();
    assert_eq!(small.suites.iter().map(|s| s.tests.len()).sum::<usize>(), 3);
}

/// `node --test --test-reporter=tap` of Node 22, paths shortened.
const NODE_TAP: &str = "TAP version 13
# Subtest: adds
ok 1 - adds
  ---
  duration_ms: 1.820988
  type: 'test'
  ...
# Subtest: fails
not ok 2 - fails
  ---
  duration_ms: 1.684234
  type: 'test'
  location: '/p/math.test.js:4:1'
  failureType: 'testCodeFailure'
  error: |-
    wrong sum

    2 !== 3

  code: 'ERR_ASSERTION'
  name: 'AssertionError'
  stack: |-
    TestContext.<anonymous> (/p/math.test.js:4:30)
  ...
# Subtest: quoted
not ok 3 - quoted
  ---
  duration_ms: 0.5
  error: 'one line'
  ...
1..3
# tests 3
";

/// SPEC-0045 v7 AK-17: the YAML block of a TAP test gives its duration and message; the block
/// with Node's `location` and `stack` stays in the details.
#[test]
fn tap_yaml_gives_duration_and_message() {
    let r = parse_report(NODE_TAP, None).unwrap();
    assert_eq!(r.format, ReportFormat::Tap);
    let t = &r.suites[0].tests;
    assert_eq!(t.len(), 3);
    assert_eq!(t[0].status, TestStatus::Passed);
    assert!((t[0].duration.unwrap() - 0.001820988).abs() < 1e-12);
    assert_eq!(t[0].message, None, "a passed test gets no message");
    assert_eq!(t[1].status, TestStatus::Failed);
    assert_eq!(t[1].message.as_deref(), Some("wrong sum"));
    let details = t[1].details.as_deref().unwrap();
    assert!(
        details.contains("location: '/p/math.test.js:4:1'"),
        "{details}"
    );
    assert!(details.contains("math.test.js:4:30"), "{details}");
    assert_eq!(t[2].message.as_deref(), Some("one line"));
    assert_eq!(t[2].duration, Some(0.0005));
}

/// SPEC-0045 v7 AK-17: a failed libtest test gets the panic's first line as its message and
/// `panicked at file:line:column` as its details, in the format of Rust 1.73 and later and in the
/// older one; the captured output is kept.
#[test]
fn libtest_panic_gives_message_and_place() {
    let new = "running 2 tests
test tests::adds ... ok
test tests::fails ... FAILED

failures:

---- tests::fails stdout ----

thread 'tests::fails' (1016936) panicked at src/main.rs:10:18:
assertion `left == right` failed: wrong sum
  left: 2
 right: 3
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::fails

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";
    let r = parse_report(new, Some(ReportFormat::Libtest)).unwrap();
    let t = &r.suites[0].tests;
    assert_eq!(t[0].message, None);
    assert_eq!(
        t[1].message.as_deref(),
        Some("assertion `left == right` failed: wrong sum")
    );
    assert_eq!(
        t[1].details.as_deref(),
        Some("panicked at src/main.rs:10:18")
    );
    assert!(t[1].output.as_deref().unwrap().contains("left: 2"));

    let old = new.replace(
        "thread 'tests::fails' (1016936) panicked at src/main.rs:10:18:\nassertion `left == right` failed: wrong sum\n",
        "thread 'tests::fails' panicked at 'wrong sum', src/main.rs:10:18\n",
    );
    let r = parse_report(&old, Some(ReportFormat::Libtest)).unwrap();
    let t = &r.suites[0].tests;
    assert_eq!(t[1].message.as_deref(), Some("wrong sum"));
    assert_eq!(
        t[1].details.as_deref(),
        Some("panicked at src/main.rs:10:18")
    );
}
