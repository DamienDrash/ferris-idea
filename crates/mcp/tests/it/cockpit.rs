//! The project cockpit (SPEC-0351): tiles from the test history, coverage reports, inspections, a
//! local advisory copy and Git, computed deterministically as of a given time.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_core::{ReportFormat, TestReport, Totals};
use ferris_idea_mcp::cockpit::{self, Inputs};
use serde_json::{Value, json};

use super::protocol::{connect_exact, is_error, json_text};

const T: u64 = 1_700_000_000;
const D: u64 = 86_400;

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("ferris-idea-cockpit-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("project")).unwrap();
    std::fs::create_dir_all(p.join("history")).unwrap();
    std::fs::create_dir_all(p.join("osv")).unwrap();
    p
}

fn inputs<'a>(root: &'a Path, history: &'a Path, osv: &'a Path, as_of: u64) -> Inputs<'a> {
    Inputs {
        root,
        test_history: Some(history),
        advisories: Some(osv),
        as_of,
        period_days: 30,
        deploy_tags: "v*",
    }
}

fn report(passed: usize, failed: usize) -> TestReport {
    TestReport {
        format: ReportFormat::Junit,
        suites: Vec::new(),
        totals: Totals {
            tests: passed + failed,
            passed,
            failed,
            errors: 0,
            skipped: 1,
        },
    }
}

/// SPEC-0351 AK-1.
#[test]
fn tests_coverage_and_inspections() {
    let dir = scratch("tests");
    let (root, history, osv) = (dir.join("project"), dir.join("history"), dir.join("osv"));
    let as_of = T + 10 * D;

    // Nothing yet: every tile says why.
    let empty = inputs(&root, &history, &osv, as_of);
    for tile in [cockpit::tests_tile(&empty), cockpit::coverage_tile(&empty)] {
        assert_eq!(tile.status, "not_enough_data", "{tile:?}");
        assert!(tile.reason.is_some());
    }
    let build = cockpit::build_tile();
    assert_eq!(build.status, "not_enough_data");
    assert!(!build.definition.is_empty());

    // Two configurations; a run after as_of is left out; another project's history too.
    ferris_idea_core::record_history(&history, &root, "unit", &report(10, 0), T + D).unwrap();
    ferris_idea_core::record_history(&history, &root, "unit", &report(9, 1), T + 2 * D).unwrap();
    ferris_idea_core::record_history(&history, &root, "it", &report(5, 0), T + 3 * D).unwrap();
    ferris_idea_core::record_history(&history, &root, "it", &report(0, 5), T + 20 * D).unwrap();
    ferris_idea_core::record_history(
        &history,
        &dir.join("other"),
        "unit",
        &report(0, 9),
        T + 4 * D,
    )
    .unwrap();
    let tile = cockpit::tests_tile(&inputs(&root, &history, &osv, as_of));
    assert_eq!(tile.status, "ok", "{tile:?}");
    let v = &tile.values;
    assert_eq!(v["runs_in_period"], json!(3));
    assert_eq!(v["last_runs"]["unit"]["failed"], json!(1));
    assert_eq!(v["last_runs"]["unit"]["time"], json!(T + 2 * D));
    assert_eq!(v["last_runs"]["it"]["passed"], json!(5));
    assert_eq!(v["green_share"], json!(0.67));
    assert_eq!(v["since_last_green_seconds"], json!(7 * D));
    assert_eq!(tile.sources.len(), 2, "{:?}", tile.sources);
    assert!(tile.sources.iter().all(|s| s.time.is_some()));

    // Coverage: LCOV, then a newer Cobertura report wins.
    std::fs::write(
        root.join("lcov.info"),
        "SF:a.rs\nLF:10\nLH:7\nend_of_record\nSF:b.rs\nLF:10\nLH:8\nend_of_record\n",
    )
    .unwrap();
    let lcov = cockpit::coverage_tile(&inputs(&root, &history, &osv, u64::MAX / 2));
    assert_eq!(lcov.values["line_coverage_percent"], json!(75.0));
    assert_eq!(lcov.values["report"], "lcov.info");
    assert_eq!(lcov.sources[0].kind, "lcov report");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        root.join("coverage.xml"),
        "<?xml version=\"1.0\"?>\n<coverage line-rate=\"0.8125\" branch-rate=\"0\">\n</coverage>\n",
    )
    .unwrap();
    let cob = cockpit::coverage_tile(&inputs(&root, &history, &osv, u64::MAX / 2));
    assert_eq!(cob.values["line_coverage_percent"], json!(81.25));
    assert_eq!(cob.values["report"], "coverage.xml");
    // A report newer than as_of is not read.
    let before = cockpit::coverage_tile(&inputs(&root, &history, &osv, T));
    assert_eq!(before.status, "not_enough_data");

    // Inspections: a bare except in Python.
    std::fs::write(root.join("app.py"), "try:\n    x = 1\nexcept:\n    pass\n").unwrap();
    let insp = cockpit::inspections_tile(&inputs(&root, &history, &osv, as_of));
    assert_eq!(insp.status, "ok");
    assert!(
        insp.values["total"].as_u64().unwrap() >= 1,
        "{}",
        insp.values
    );
    assert!(
        insp.values["top"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "python.bare-except"),
        "{}",
        insp.values
    );
    assert_eq!(insp.values["checked_at"], json!(as_of));
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC-0351 AK-2.
#[test]
fn dependencies_from_a_local_advisory_copy() {
    let dir = scratch("deps");
    let (root, history, osv) = (dir.join("project"), dir.join("history"), dir.join("osv"));
    let i = inputs(&root, &history, &osv, T);
    assert_eq!(cockpit::dependencies_tile(&i).status, "not_enough_data");
    std::fs::write(
        root.join("Cargo.lock"),
        r#"version = 4

[[package]]
name = "foo"
version = "1.2.3"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "bar"
version = "0.9.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "baz"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "mine"
version = "0.1.0"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("package-lock.json"),
        r#"{"lockfileVersion": 3, "packages": {
            "": {"name": "app", "version": "1.0.0"},
            "node_modules/left-pad": {"version": "1.0.0"},
            "node_modules/a/node_modules/qs": {"version": "6.5.0"},
            "node_modules/linked": {"version": "1.0.0", "link": true}
        }}"#,
    )
    .unwrap();
    // No copy yet.
    let no_copy = cockpit::dependencies_tile(&inputs(&root, &history, &dir.join("missing"), T));
    assert_eq!(no_copy.status, "not_enough_data");
    assert!(no_copy.reason.unwrap().contains("does not download"));

    let record = |id: &str, eco: &str, name: &str, extra: Value, severity: Option<&str>| {
        let mut r = json!({
            "id": id,
            "modified": "2024-03-01T12:00:00Z",
            "summary": "SUMMARY-TEXT-must-not-show",
            "details": "DETAILS-TEXT-must-not-show",
            "affected": [{"package": {"ecosystem": eco, "name": name}}],
        });
        for (k, v) in extra.as_object().unwrap() {
            r["affected"][0][k] = v.clone();
        }
        if let Some(s) = severity {
            r["database_specific"] = json!({"severity": s});
        }
        r.to_string()
    };
    std::fs::create_dir_all(osv.join("crates.io")).unwrap();
    std::fs::create_dir_all(osv.join("npm")).unwrap();
    let files = [
        (
            "crates.io/RUSTSEC-1.json",
            record(
                "RUSTSEC-1",
                "crates.io",
                "foo",
                json!({"ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "1.2.5"}]}]}),
                Some("HIGH"),
            ),
        ),
        (
            "crates.io/RUSTSEC-2.json",
            record(
                "RUSTSEC-2",
                "crates.io",
                "bar",
                json!({"ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0.5.0"}, {"last_affected": "0.9.0"}]}]}),
                None,
            ),
        ),
        (
            "crates.io/RUSTSEC-3.json",
            record(
                "RUSTSEC-3",
                "crates.io",
                "baz",
                json!({"ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "1.5.0"}]}]}),
                Some("CRITICAL"),
            ),
        ),
        (
            "npm/GHSA-1.json",
            record(
                "GHSA-1",
                "npm",
                "left-pad",
                json!({"versions": ["0.9.0", "1.0.0"]}),
                Some("LOW"),
            ),
        ),
        (
            "npm/GHSA-2.json",
            record(
                "GHSA-2",
                "npm",
                "qs",
                json!({"ranges": [{"type": "SEMVER", "events": [{"introduced": "6.0.0"}, {"fixed": "6.5.3"}]}]}),
                Some("MODERATE"),
            ),
        ),
        (
            "npm/GHSA-3.json",
            record(
                "GHSA-3",
                "npm",
                "linked",
                json!({"versions": ["1.0.0"]}),
                None,
            ),
        ),
    ];
    for (path, text) in &files {
        std::fs::write(osv.join(path), text).unwrap();
    }
    std::fs::write(osv.join("broken.json"), "{").unwrap();
    let tile = cockpit::dependencies_tile(&inputs(&root, &history, &osv, T));
    assert_eq!(tile.status, "ok", "{tile:?}");
    let shown = serde_json::to_string(&tile).unwrap();
    assert!(
        !shown.contains("SUMMARY-TEXT") && !shown.contains("DETAILS-TEXT"),
        "{shown}"
    );
    let affected: Vec<(String, String, Value, String)> = tile.values["affected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap().to_owned(),
                a["version"].as_str().unwrap().to_owned(),
                a["fixed"].clone(),
                a["severity"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        affected,
        [
            ("bar".into(), "0.9.0".into(), Value::Null, "unknown".into()),
            ("foo".into(), "1.2.3".into(), json!("1.2.5"), "high".into()),
            ("left-pad".into(), "1.0.0".into(), Value::Null, "low".into()),
            (
                "qs".into(),
                "6.5.0".into(),
                json!("6.5.3"),
                "moderate".into()
            ),
        ]
    );
    assert_eq!(tile.values["packages_checked"], json!(5));
    assert_eq!(tile.values["advisories_read"], json!(6));
    assert_eq!(tile.values["copy_date"], json!(1_709_294_400));
    let sev: BTreeMap<String, u64> =
        serde_json::from_value(tile.values["by_severity"].clone()).unwrap();
    assert_eq!(sev.values().sum::<u64>(), 4);
    assert_eq!(
        cockpit::compare_versions("1.0.0-rc.1", "1.0.0"),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        cockpit::compare_versions("1.10", "1.9.9"),
        std::cmp::Ordering::Greater
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn git(dir: &Path, args: &[&str], at: u64) {
    let date = format!("@{at} +0000");
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit(dir: &Path, name: &str, at: u64) {
    std::fs::write(dir.join(name), name).unwrap();
    git(dir, &["add", name], at);
    git(dir, &["commit", "-q", "-m", name], at);
}

/// SPEC-0351 AK-3.
#[test]
fn git_and_delivery_from_a_repository() {
    let dir = scratch("git");
    let (root, history, osv) = (dir.join("project"), dir.join("history"), dir.join("osv"));
    let as_of = T + 8 * D;
    let i = inputs(&root, &history, &osv, as_of);
    assert_eq!(cockpit::git_tile(&i).status, "not_enough_data");
    git(&root, &["init", "-q", "-b", "main"], T);
    commit(&root, "c1", T);
    git(&root, &["tag", "-a", "v1", "-m", "v1"], T + D);
    git(&root, &["branch", "feature"], T + D);
    commit(&root, "c2", T + 2 * D);
    commit(&root, "c3", T + 3 * D);
    git(&root, &["tag", "-a", "v2", "-m", "v2"], T + 4 * D);
    // A rollback: v3 deploys the commit of v1 again.
    git(
        &root,
        &["tag", "-a", "v3", "-m", "v3", "v1^{commit}"],
        T + 5 * D,
    );
    commit(&root, "c4", T + 6 * D);
    git(&root, &["tag", "-a", "v4", "-m", "v4"], T + 7 * D);
    // A lightweight tag that is no deployment, and an unmerged branch.
    git(&root, &["tag", "nightly"], T + 7 * D);
    git(&root, &["checkout", "-q", "feature"], T + 7 * D);
    commit(&root, "c5", T + 2 * D + D / 2);
    git(&root, &["checkout", "-q", "main"], T + 7 * D);

    let g = cockpit::git_tile(&i);
    assert_eq!(g.status, "ok", "{g:?}");
    assert_eq!(g.values["main_branch"], "main");
    assert_eq!(g.values["commits_in_period"], json!(4));
    assert_eq!(g.values["authors_in_period"], json!(1));
    assert_eq!(g.values["last_commit"], json!(T + 6 * D));
    assert_eq!(g.values["unmerged_branch_count"], json!(1));
    assert_eq!(g.values["unmerged_branches"][0]["name"], "feature");
    assert_eq!(
        g.values["unmerged_branches"][0]["tip_time"],
        json!(T + 2 * D + D / 2)
    );

    let d = cockpit::delivery_tile(&i);
    assert_eq!(d.status, "ok", "{d:?}");
    let v = &d.values;
    let tags: Vec<&str> = v["deployments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["tag"].as_str().unwrap())
        .collect();
    assert_eq!(tags, ["v1", "v2", "v3", "v4"]);
    assert_eq!(v["deployment_frequency_per_week"], json!(0.93));
    assert_eq!(v["weeks_with_deployment_share"], json!(0.4));
    assert_eq!(v["lead_time_median_seconds"], json!(D));
    assert_eq!(v["lead_time_commits"], json!(4));
    assert_eq!(v["change_failure_rate"], json!(0.25));
    assert_eq!(v["failed_deployments"], json!(["v2"]));
    assert_eq!(v["recovery_time_median_seconds"], json!(D));
    assert_eq!(d.sources[0].time, Some(T + 7 * D));

    // As of an earlier time, later tags and commits are left out.
    let early = cockpit::delivery_tile(&inputs(&root, &history, &osv, T + 4 * D));
    assert_eq!(early.values["deployments"].as_array().unwrap().len(), 2);
    assert_eq!(early.values["change_failure_rate"], json!(0.0));
    // Fewer than two deployments.
    let mut none = inputs(&root, &history, &osv, as_of);
    none.deploy_tags = "release-*";
    assert_eq!(cockpit::delivery_tile(&none).status, "not_enough_data");
    assert!(cockpit::glob("v*", "v1.2") && !cockpit::glob("v*", "nightly"));
    assert!(cockpit::glob("rel-*-prod", "rel-3-prod") && !cockpit::glob("rel-*-prod", "rel-3-dev"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.insert(p.clone(), std::fs::read(&p).unwrap_or_default());
            }
        }
    }
    out
}

/// SPEC-0351 AK-4.
#[tokio::test]
async fn the_tool_is_deterministic_and_reads_only() {
    let (mut client, root) = connect_exact(&[], |s, _| s).await;
    std::fs::write(root.join("lcov.info"), "LF:4\nLH:3\n").unwrap();
    let before = snapshot(&root);
    let args = json!({"as_of": 4_000_000_000u64, "period_days": 30});
    let first = json_text(&client.call("project_cockpit", args.clone()).await);
    let second = json_text(&client.call("project_cockpit", args).await);
    assert_eq!(first, second);
    let ids: Vec<&str> = first["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, cockpit::TILES);
    for t in first["tiles"].as_array().unwrap() {
        assert!(!t["definition"].as_object().unwrap().is_empty(), "{t}");
        assert!(t["status"] == "ok" || t["reason"].is_string(), "{t}");
        for s in t["sources"].as_array().unwrap() {
            assert!(s.get("time").is_some() && s["kind"].is_string(), "{s}");
        }
    }
    assert_eq!(
        first["tiles"][2]["values"]["line_coverage_percent"],
        json!(75.0)
    );
    assert!(first["not_available"].as_array().unwrap().len() >= 4);
    let only = json_text(
        &client
            .call("project_cockpit", json!({"tiles": ["build"], "as_of": T}))
            .await,
    );
    assert_eq!(only["tiles"].as_array().unwrap().len(), 1);
    assert!(is_error(
        &client
            .call("project_cockpit", json!({"tiles": ["weather"]}))
            .await
    ));
    assert!(is_error(
        &client
            .call("project_cockpit", json!({"period_days": 0}))
            .await
    ));
    assert!(is_error(
        &client
            .call("project_cockpit", json!({"period_days": 366}))
            .await
    ));
    assert!(is_error(
        &client
            .call("project_cockpit", json!({"deploy_tags": "--all"}))
            .await
    ));
    assert_eq!(snapshot(&root), before, "the cockpit changed the workspace");
}
