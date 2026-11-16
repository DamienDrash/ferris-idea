//! F-078 · Test runner with result tree (SPEC-0045 AK-1…AK-5, SPEC-0088).

use crate::harness::*;
use serde_json::json;

#[test]
fn junit_tap_and_libtest_reports_become_trees() {
    let p = Project::empty();
    let mut ide = Ide::stdio(&p);
    let junit = r#"<testsuites><testsuite name="S" tests="2"><testcase name="a" classname="C" time="0.1"/><testcase name="b" classname="C"><failure message="boom">trace</failure></testcase></testsuite></testsuites>"#;
    let r = ide.ok("parse_test_report", json!({"text": junit}));
    assert_eq!(r["format"], "junit");
    assert_eq!(r["totals"]["passed"], 1);
    assert_eq!(r["totals"]["failed"], 1);
    let tap = ide.ok(
        "parse_test_report",
        json!({"text": "TAP version 13\n1..3\nok 1 - a\nnot ok 2 - b\nok 3 - c # SKIP later\n"}),
    );
    assert_eq!(tap["format"], "tap");
    assert_eq!(tap["totals"]["skipped"], 1, "{tap}");
    let libtest = "running 2 tests\ntest m::ok_one ... ok\ntest m::bad ... FAILED\n\nfailures:\n\n---- m::bad stdout ----\nboom\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\n";
    let r = ide.ok("parse_test_report", json!({"text": libtest}));
    assert_eq!(r["totals"]["failed"], 1, "{r}");
    assert!(text(&r).contains("boom"), "failure output kept: {r}");
}

#[test]
fn a_test_configuration_attaches_its_report() {
    let p = Project::empty();
    p.write(
        ".idecraft/run.toml",
        "[[run]]\nname = \"t\"\nkind = \"test\"\nreport = \"tap\"\ncommand = [\"sh\", \"-c\", \"printf '1..2\\\\nok 1 - a\\\\nnot ok 2 - b\\\\n'\"]\n",
    )
    .grant(&["run"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("run_configuration", json!({"name": "t", "dry_run": false}));
    assert!(text(&r).contains("\"failed\":1"), "{r}");
}
