//! Status-bar text for stash, cherry-pick and rebase (`SPEC-0106` behaviour 9) and smart checkout
//! (`SPEC-0231`), without drawing.

use ferris_idea_vcs::{GitError, OpOutcome, Operation, SmartOutcome, SwitchOutcome};

/// Lines of an outcome's output about local changes put aside or brought back (`SPEC-0231`).
fn stash_notes(o: &OpOutcome) -> String {
    o.output
        .lines()
        .filter(|l| l.starts_with("local changes"))
        .map(|l| format!(" — {l}"))
        .collect()
}

/// At most three of the comma-separated `files`, then "+n more" (`SPEC-0231` behaviour 1).
pub fn short_file_list(files: &str) -> String {
    let all: Vec<&str> = files.split(", ").filter(|f| !f.is_empty()).collect();
    let mut out = all.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if all.len() > 3 {
        out.push_str(&format!(" +{} more", all.len() - 3));
    }
    out
}

/// A smart merge as one outcome for the status bar: the stash is named while it waits.
pub fn smart_outcome(done: SmartOutcome<OpOutcome>) -> OpOutcome {
    let mut out = done.result;
    if let Some(stash) = done.stash {
        out.output.push('\n');
        out.output.push_str(&if stash.restored {
            "local changes restored from the smart checkout stash".to_owned()
        } else if out.in_progress.is_some() {
            format!(
                "local changes put aside in the stash \"{}\" until the merge is finished",
                stash.message
            )
        } else {
            out.conflicts = stash.conflicts;
            format!(
                "local changes kept in the stash \"{}\": they conflict",
                stash.message
            )
        });
    }
    out
}

/// A smart checkout as one outcome for the status bar.
pub fn smart_checkout_outcome(done: SmartOutcome<SwitchOutcome>) -> OpOutcome {
    smart_outcome(SmartOutcome {
        result: OpOutcome {
            ok: done.result.switched || done.result.conflicts.is_empty(),
            conflicts: Vec::new(),
            in_progress: None,
            output: format!("checked out {}", done.result.branch),
        },
        stash: done.stash,
    })
}

/// What the status bar says after the operation `label`: done, stopped with conflicts and how to go
/// on, or why it failed.
pub fn report(label: &str, done: &Result<OpOutcome, GitError>) -> String {
    match done {
        Err(e) => format!("{label} failed: {e}"),
        Ok(o) if !o.conflicts.is_empty() => {
            let what = match o.in_progress {
                Some(Operation::Rebase) => "Rebase",
                Some(Operation::CherryPick) => "Cherry-pick",
                Some(Operation::Merge) => "Merge",
                None => label,
            };
            let mut text = format!("{what} stopped: conflicts in {}", o.conflicts.join(", "));
            if o.in_progress.is_some() {
                text.push_str(" — resolve and stage them, then Git: Continue (or Git: Abort)");
            }
            text + &stash_notes(o)
        }
        Ok(o) if o.ok => format!("{label} done{}", stash_notes(o)),
        Ok(o) => {
            let first = o.output.lines().next().unwrap_or("git reported an error");
            format!("{label} failed: {first}")
        }
    }
}

/// What the interactive rebase dialog names as its base (hand test 71, D34): a branch name as
/// given, an expression or id (`<id>^`, `HEAD~2`) as the id of the commit it resolves to, so
/// "from here" on a commit names that commit's parent.
pub fn rebase_base_label(onto: &str, resolved_short: Option<&str>) -> String {
    let expression = onto.contains(['^', '~', '@'])
        || (onto.len() >= 7 && onto.chars().all(|c| c.is_ascii_hexdigit()));
    match resolved_short {
        Some(id) if expression => id.to_owned(),
        _ => onto.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_base_is_named_by_the_resolved_commit() {
        // ⌥I on a commit rebases onto its parent: the title names the parent (D34).
        let id = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            rebase_base_label(&format!("{id}^"), Some("fedcba9")),
            "fedcba9"
        );
        assert_eq!(rebase_base_label("HEAD~2", Some("abc1234")), "abc1234");
        assert_eq!(rebase_base_label(id, Some("0123456")), "0123456");
        assert_eq!(
            rebase_base_label("origin/main", Some("abc1234")),
            "origin/main"
        );
        assert_eq!(rebase_base_label("HEAD~2", None), "HEAD~2");
    }

    #[test]
    fn smart_checkout_texts() {
        assert_eq!(short_file_list("a, b"), "a, b");
        assert_eq!(short_file_list("a, b, c, d, e"), "a, b, c +2 more");
        let stash = |restored: bool, conflicts: &[&str]| ferris_idea_vcs::SmartStash {
            id: "abc".into(),
            message: "On main: Ferris IDEA smart checkout: main → topic".into(),
            restored,
            conflicts: conflicts.iter().map(|c| (*c).to_owned()).collect(),
        };
        let switched = |stash| SmartOutcome {
            result: SwitchOutcome {
                branch: "topic".into(),
                switched: true,
                changes: Vec::new(),
                conflicts: Vec::new(),
            },
            stash,
        };
        let ok = smart_checkout_outcome(switched(Some(stash(true, &[]))));
        assert_eq!(
            report("Smart checkout topic", &Ok(ok)),
            "Smart checkout topic done — local changes restored from the smart checkout stash"
        );
        let kept = smart_checkout_outcome(switched(Some(stash(false, &["b.txt"]))));
        assert_eq!(
            report("Smart checkout topic", &Ok(kept)),
            "Smart checkout topic stopped: conflicts in b.txt — local changes kept in the stash \"On main: Ferris IDEA smart checkout: main → topic\": they conflict"
        );
        let waiting = smart_outcome(SmartOutcome {
            result: outcome(false, &["a.txt"], Some(Operation::Merge)),
            stash: Some(stash(false, &[])),
        });
        assert!(report("Smart merge topic", &Ok(waiting)).ends_with("until the merge is finished"));
    }

    fn outcome(ok: bool, conflicts: &[&str], in_progress: Option<Operation>) -> OpOutcome {
        OpOutcome {
            ok,
            conflicts: conflicts.iter().map(|s| (*s).to_owned()).collect(),
            in_progress,
            output: "error: could not apply 1a2b3c4\nhint: …".into(),
        }
    }

    #[test]
    fn reports_done_conflicts_and_failures() {
        assert_eq!(report("Stash", &Ok(outcome(true, &[], None))), "Stash done");
        let stopped = report(
            "Rebase onto main",
            &Ok(outcome(false, &["a.txt", "b.txt"], Some(Operation::Rebase))),
        );
        assert!(stopped.starts_with("Rebase stopped: conflicts in a.txt, b.txt"));
        assert!(stopped.contains("Git: Continue"));
        let pop = report("Pop stash", &Ok(outcome(false, &["a.txt"], None)));
        assert_eq!(pop, "Pop stash stopped: conflicts in a.txt");
        assert_eq!(
            report("Cherry-pick 1a2b3c4", &Ok(outcome(false, &[], None))),
            "Cherry-pick 1a2b3c4 failed: error: could not apply 1a2b3c4"
        );
        assert_eq!(
            report(
                "Abort",
                &Err(GitError::Rejected("no operation is in progress".into()))
            ),
            "Abort failed: no operation is in progress"
        );
    }
}
