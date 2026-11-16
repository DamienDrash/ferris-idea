//! The groups of the window self-test (`idecraft ui --selftest`): which of them a run takes
//! (`IDECRAFT_SELFTEST_ONLY`, `IDECRAFT_SELFTEST_SKIP`), the meta groups that name several at
//! once, and the report of how long each took. The steps themselves are in `workbench.rs`.

use std::time::Duration;

/// Runs only the named groups (comma-separated; group and meta group names).
pub const ONLY: &str = "IDECRAFT_SELFTEST_ONLY";
/// Leaves the named groups out (comma-separated; group and meta group names).
pub const SKIP: &str = "IDECRAFT_SELFTEST_SKIP";

/// Names for several groups at once.
///
/// `debuggers` is every group that starts a debug adapter, attaches to a process or samples one.
/// On macOS a debugger that takes over another process (lldb's `debugserver`, `dlv`, debugpy's
/// attach by process id, `sample`) needs the "Developer Tools Access" right; in the session of a
/// standard user the system asks for an administrator in a dialog, and while that dialog is open
/// debugging hangs in every session of the machine. A run for such a session leaves these groups
/// out. Adapters that only speak to a port of a program they started themselves (java-debug,
/// js-debug, kotlin-debug-adapter, debugpy on a launched script) are in the list as well: one
/// switch turns off everything that debugs.
///
/// The other meta groups keep the names `IDECRAFT_SELFTEST_ONLY` took before every block of steps
/// had a name of its own.
pub const META: &[(&str, &[&str])] = &[
    (
        "debuggers",
        &[
            "python-debug",
            "attach",
            "detected-run-debug",
            "debug",
            "debug-values",
            "java-debug-session",
            "java-smart",
            "java-debug-more",
            "python-debug-more",
            "node-debug",
            "package-script-debug",
            "kotlin-debug",
            "browser-debug",
            "breakpoint-condition-debug",
            "profiler-attach",
        ],
    ),
    (
        "java-debug",
        &["java-debug-session", "java-smart", "java-debug-more"],
    ),
    (
        "debug-more",
        &["java-debug-more", "python-debug-more", "breakpoint-options"],
    ),
    (
        "breakpoints",
        &[
            "breakpoint-dialog",
            "breakpoint-condition-debug",
            "breakpoint-moves",
            "breakpoint-options",
        ],
    ),
    (
        "remote-project",
        &[
            "remote-project-windows",
            "remote-host",
            "remote-tool-windows",
        ],
    ),
];

/// The groups a comma-separated list names, meta groups replaced by their members. Blanks around
/// a name and empty entries are ignored; a name that is neither a group nor a meta group is an
/// error that lists what exists.
pub fn expand(list: &str, names: &[&'static str]) -> Result<Vec<&'static str>, String> {
    let mut out: Vec<&'static str> = Vec::new();
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let members: Vec<&'static str> = match META.iter().find(|(meta, _)| *meta == part) {
            Some((_, members)) => members.to_vec(),
            None => match names.iter().find(|n| **n == part) {
                Some(name) => vec![*name],
                None => {
                    return Err(format!(
                        "unknown self-test group {part:?}; groups: {}; meta groups: {}",
                        names.join(", "),
                        META.iter()
                            .map(|(meta, _)| *meta)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            },
        };
        for m in members {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    Ok(out)
}

/// What a run takes of the groups `names` (in run order).
#[derive(Debug, PartialEq, Eq)]
pub struct Selection {
    /// Per group of `names`: whether its steps run.
    pub run: Vec<bool>,
    /// The groups `skip` took out of the run, in run order.
    pub skipped: Vec<&'static str>,
}

/// `only` (unset or empty: every group) without `skip`. The order of a run is always the order of
/// `names`, whatever the order of the lists.
pub fn select(
    names: &[&'static str],
    only: Option<&str>,
    skip: Option<&str>,
) -> Result<Selection, String> {
    let only = match only.map(str::trim).filter(|o| !o.is_empty()) {
        Some(list) => Some(expand(list, names).map_err(|e| format!("{ONLY}: {e}"))?),
        None => None,
    };
    let skip = match skip {
        Some(list) => expand(list, names).map_err(|e| format!("{SKIP}: {e}"))?,
        None => Vec::new(),
    };
    let wanted = |n: &&'static str| only.as_ref().is_none_or(|o| o.contains(n));
    Ok(Selection {
        run: names
            .iter()
            .map(|n| wanted(n) && !skip.contains(n))
            .collect(),
        skipped: names
            .iter()
            .filter(|n| wanted(n) && skip.contains(n))
            .copied()
            .collect(),
    })
}

/// The selection the environment asks for.
pub fn from_env(names: &[&'static str]) -> Result<Selection, String> {
    select(
        names,
        std::env::var(ONLY).ok().as_deref(),
        std::env::var(SKIP).ok().as_deref(),
    )
}

/// The line on stderr for a group left out by [`SKIP`], like the lines of groups whose tools are
/// missing.
pub fn skipped_line(name: &str) -> String {
    format!("{name} steps skipped: {SKIP}")
}

/// How long one group took: its name, its duration and the number of its steps.
pub type Timing = (&'static str, Duration, usize);

/// One line per group that had steps, slowest first (equal ones in run order), and the sum.
pub fn timing_lines(timings: &[Timing]) -> Vec<String> {
    let mut sorted: Vec<&Timing> = timings.iter().filter(|t| t.2 > 0).collect();
    sorted.sort_by_key(|t| std::cmp::Reverse(t.1));
    let total: Duration = sorted.iter().map(|t| t.1).sum();
    let mut lines: Vec<String> = sorted
        .iter()
        .map(|(name, took, steps)| {
            format!(
                "self-test group {name}: {:.1} s ({steps} steps)",
                took.as_secs_f64()
            )
        })
        .collect();
    lines.push(format!(
        "self-test groups: {:.1} s in {} groups",
        total.as_secs_f64(),
        sorted.len()
    ));
    lines
}

/// The result line of a run: the number of checks, the failed checks and the groups left out by
/// [`SKIP`].
pub fn result_line(checks: usize, failed: &[&str], skipped: &[&str]) -> String {
    format!("{{\"checks\":{checks},\"failed\":{failed:?},\"skipped\":{skipped:?}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &[
        "editor",
        "python-debug",
        "debug",
        "java-debug-session",
        "java-smart",
        "java-debug-more",
        "python-debug-more",
        "breakpoint-dialog",
        "breakpoint-condition-debug",
        "breakpoint-moves",
        "breakpoint-options",
        "mouse",
    ];

    fn ran(s: &Selection) -> Vec<&'static str> {
        NAMES
            .iter()
            .zip(&s.run)
            .filter(|(_, run)| **run)
            .map(|(n, _)| *n)
            .collect()
    }

    #[test]
    fn without_lists_every_group_runs() {
        let s = select(NAMES, None, None).unwrap();
        assert_eq!(ran(&s), NAMES);
        assert!(s.skipped.is_empty());
        // An empty value is no list.
        assert_eq!(select(NAMES, Some(""), Some(" ")).unwrap(), s);
    }

    #[test]
    fn only_takes_a_comma_separated_list_in_run_order() {
        let s = select(NAMES, Some("mouse, editor,,mouse"), None).unwrap();
        assert_eq!(ran(&s), ["editor", "mouse"]);
        assert!(s.skipped.is_empty());
    }

    #[test]
    fn the_names_used_before_still_select_their_steps() {
        let only = |list| ran(&select(NAMES, Some(list), None).unwrap());
        assert_eq!(
            only("java-debug"),
            ["java-debug-session", "java-smart", "java-debug-more"]
        );
        assert_eq!(
            only("debug-more"),
            ["java-debug-more", "python-debug-more", "breakpoint-options"]
        );
        assert_eq!(
            only("breakpoints"),
            [
                "breakpoint-dialog",
                "breakpoint-condition-debug",
                "breakpoint-moves",
                "breakpoint-options"
            ]
        );
        assert_eq!(only("java-smart"), ["java-smart"]);
        assert_eq!(only("mouse"), ["mouse"]);
    }

    #[test]
    fn skip_leaves_groups_out_and_names_them() {
        let s = select(NAMES, None, Some("mouse,debuggers")).unwrap();
        assert_eq!(
            ran(&s),
            [
                "editor",
                "breakpoint-dialog",
                "breakpoint-moves",
                "breakpoint-options"
            ]
        );
        assert_eq!(
            s.skipped,
            [
                "python-debug",
                "debug",
                "java-debug-session",
                "java-smart",
                "java-debug-more",
                "python-debug-more",
                "breakpoint-condition-debug",
                "mouse"
            ]
        );
    }

    #[test]
    fn skip_applies_after_only_and_names_only_what_would_have_run() {
        let s = select(NAMES, Some("breakpoints,editor"), Some("debuggers")).unwrap();
        assert_eq!(
            ran(&s),
            [
                "editor",
                "breakpoint-dialog",
                "breakpoint-moves",
                "breakpoint-options"
            ]
        );
        assert_eq!(s.skipped, ["breakpoint-condition-debug"]);
    }

    #[test]
    fn an_unknown_name_is_an_error_that_names_the_variable() {
        let e = select(NAMES, Some("mouse,nope"), None).unwrap_err();
        assert!(
            e.starts_with("IDECRAFT_SELFTEST_ONLY: unknown self-test group \"nope\""),
            "{e}"
        );
        assert!(
            e.contains("editor, python-debug") && e.contains("debuggers"),
            "{e}"
        );
        let e = select(NAMES, None, Some("debugger")).unwrap_err();
        assert!(e.starts_with("IDECRAFT_SELFTEST_SKIP: "), "{e}");
    }

    #[test]
    fn the_debuggers_group_holds_every_debug_adapter_and_the_attaching_profiler() {
        let (_, members) = META.iter().find(|(m, _)| *m == "debuggers").unwrap();
        for name in [
            "debug",
            "debug-values",
            "python-debug",
            "python-debug-more",
            "attach",
            "java-debug-session",
            "java-smart",
            "java-debug-more",
            "node-debug",
            "package-script-debug",
            "kotlin-debug",
            "browser-debug",
            "detected-run-debug",
            "breakpoint-condition-debug",
            "profiler-attach",
        ] {
            assert!(members.contains(&name), "{name}");
        }
        // The profiler runs that start their own program need no debugging rights.
        assert!(!members.contains(&"profiler"));
    }

    #[test]
    fn meta_groups_have_no_duplicates_and_are_not_nested() {
        for (meta, members) in META {
            for (i, m) in members.iter().enumerate() {
                assert!(!members[..i].contains(m), "{meta}: {m} twice");
                assert!(
                    META.iter().all(|(other, _)| other != m),
                    "{meta}: {m} is a meta group"
                );
            }
        }
    }

    #[test]
    fn the_skipped_line_reads_like_the_lines_of_missing_tools() {
        assert_eq!(
            skipped_line("kotlin-debug"),
            "kotlin-debug steps skipped: IDECRAFT_SELFTEST_SKIP"
        );
    }

    #[test]
    fn timings_are_listed_slowest_first_without_empty_groups() {
        let lines = timing_lines(&[
            ("editor", Duration::from_millis(1500), 10),
            ("go-test", Duration::ZERO, 0),
            ("debug", Duration::from_secs(42), 7),
            ("mouse", Duration::from_millis(300), 3),
        ]);
        assert_eq!(
            lines,
            [
                "self-test group debug: 42.0 s (7 steps)",
                "self-test group editor: 1.5 s (10 steps)",
                "self-test group mouse: 0.3 s (3 steps)",
                "self-test groups: 43.8 s in 3 groups",
            ]
        );
    }

    #[test]
    fn the_result_line_keeps_checks_and_failed_and_adds_skipped() {
        assert_eq!(
            result_line(3, &[], &[]),
            r#"{"checks":3,"failed":[],"skipped":[]}"#
        );
        let line = result_line(2, &["a \"quoted\" check"], &["debug", "attach"]);
        assert_eq!(
            line,
            r#"{"checks":2,"failed":["a \"quoted\" check"],"skipped":["debug", "attach"]}"#
        );
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["failed"][0], "a \"quoted\" check");
        assert_eq!(v["skipped"][1], "attach");
    }
}
