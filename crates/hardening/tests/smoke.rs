//! M8.2 in `cargo test`: every fuzz target on its seeds plus a short mutation run, and every
//! minimised crash input in `regressions/<target>/` (one per fixed finding) replayed.

use idecraft_hardening::{Budget, TARGETS, run, target};

#[test]
fn every_target_survives_a_short_run() {
    let mut failures = Vec::new();
    for t in TARGETS {
        // The MCP target talks to a live server; fewer inputs keep the test short.
        let iterations = if t.name == "mcp_jsonrpc" { 100 } else { 400 };
        let report = run(t, Budget::iterations(iterations), 7, &[]);
        assert_eq!(report.iterations, iterations, "{}", t.name);
        for c in report.crashes {
            failures.push(format!("{}: {} (input {:?})", t.name, c.message, c.input));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn fixed_findings_stay_fixed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("regressions");
    let mut replayed = 0;
    for entry in std::fs::read_dir(&dir).expect("regressions folder") {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let t = target(&name).unwrap_or_else(|| panic!("regressions/{name}: no such target"));
        for input in std::fs::read_dir(entry.path()).unwrap() {
            let bytes = std::fs::read(input.unwrap().path()).unwrap();
            (t.run)(&bytes);
            replayed += 1;
        }
    }
    assert!(replayed >= 5, "only {replayed} regression inputs");
}

#[test]
fn targets_have_unique_names_and_seeds() {
    for (i, t) in TARGETS.iter().enumerate() {
        assert!(!t.seeds.is_empty(), "{} has no seeds", t.name);
        assert!(
            TARGETS[..i].iter().all(|u| u.name != t.name),
            "{} twice",
            t.name
        );
    }
}
