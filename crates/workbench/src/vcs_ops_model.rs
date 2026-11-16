//! The VCS operations popup and the status of fetch, pull and push (`SPEC-0109`), without drawing.

use idecraft_vcs::{GitError, MergeKind, Outgoing, PullOutcome, RemoteOutcome, Stash, UpdateInfo};

/// The entries of the VCS operations popup: title and action, in the reference's order; the first
/// ten carry the digits 1–9 and 0 (`SPEC-0109` v2 behaviour 6, OBS-0022 L13).
pub const OPERATIONS: [(&str, &str); 16] = [
    ("Commit…", "CheckinProject"),
    ("Commit File…", "CheckinFiles"),
    ("Rollback…", "ChangesView.Revert"),
    ("Show History", "Vcs.ShowTabbedFileHistory"),
    ("Annotate with Git Blame", "Annotate"),
    ("Show Diff", "Compare.SameVersion"),
    ("Branches…", "Git.Branches"),
    ("Push…", "Vcs.Push"),
    ("Stash Changes…", "Git.Stash"),
    ("Unstash Changes…", "Git.Unstash"),
    ("Update Project…", "Vcs.UpdateProject"),
    ("Fetch", "Git.Fetch"),
    ("Resolve Conflicts…", "Git.ResolveConflicts"),
    ("Add to VCS", "ChangesView.AddUnversioned"),
    ("Copy Branch Name", "Git.CopyBranchName"),
    ("Local History", "LocalHistory.ShowHistory"),
];

/// The digit of entry `i`: 1–9, then 0 for the tenth, none after it.
pub fn operation_digit(i: usize) -> Option<char> {
    match i {
        0..=8 => char::from_digit(i as u32 + 1, 10),
        9 => Some('0'),
        _ => None,
    }
}

/// The entry a digit key runs.
pub fn operation_for_digit(key: &str) -> Option<usize> {
    match key.parse::<usize>().ok()? {
        0 => Some(9),
        d @ 1..=9 => Some(d - 1),
        _ => None,
    }
}

/// Whether pushing to `target` creates a new branch on the remote: not among the remote's
/// branches as of the last fetch (`SPEC-0109` v2 behaviour 9, OBS-0022 L24).
pub fn push_creates_branch(outgoing: &Outgoing, target: &str) -> bool {
    !outgoing.remote_branches.iter().any(|b| b == target)
}

/// Earlier stash messages for the stash dialog's history, newest first and each once: the ones
/// typed in this session, then those of the stash list without Git's `On <branch>: ` prefix;
/// Git's own `WIP on …` messages are left out (`SPEC-0106` v3 behaviour 14).
pub fn stash_message_history(typed: &[String], stashes: &[Stash]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let listed = stashes.iter().filter_map(|s| {
        let m = s.message.as_str();
        if m.starts_with("WIP on ") {
            return None;
        }
        Some(
            m.strip_prefix("On ")
                .and_then(|r| r.split_once(": "))
                .map_or(m, |(_, msg)| msg)
                .to_owned(),
        )
    });
    for m in typed.iter().rev().cloned().chain(listed) {
        if !m.trim().is_empty() && !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// The rows of the Update Info popup and its title (`SPEC-0144` v4 behaviour 8): the commits,
/// newest first, then the files with Git's status letter.
pub fn update_info_rows(info: &UpdateInfo) -> (String, Vec<String>) {
    let title = format!(
        "Update Info: {} commit(s), {} file(s) — Enter opens",
        info.commits.len(),
        info.files.len()
    );
    let rows = info
        .commits
        .iter()
        .map(|c| format!("{}  {} — {}", c.short_id, c.summary, c.author))
        .chain(
            info.files
                .iter()
                .map(|f| format!("{}  {}", f.status, f.path)),
        )
        .collect();
    (title, rows)
}

/// The reset choice of Update Project, naming what it gives up (`SPEC-0144` v4 behaviour 9,
/// IMP-0322).
pub fn reset_choice_label(ahead: usize) -> String {
    match ahead {
        0 => "Reset the current branch to its upstream (no local commits to lose) (⌥E)".into(),
        1 => "Reset the current branch to its upstream, giving up 1 local commit (⌥E)".into(),
        n => format!("Reset the current branch to its upstream, giving up {n} local commits (⌥E)"),
    }
}

/// Status text after a reset to the upstream (`SPEC-0144` v4 behaviour 9).
fn reset_text(done: &Result<PullOutcome, GitError>) -> String {
    match done {
        Err(e) => format!("Reset failed: {e}"),
        Ok(p) if !p.fetch.ok => format!("Reset failed: {}", failure(&p.fetch)),
        Ok(p) => match &p.reset {
            None => "Reset: nothing done".into(),
            Some(r) if !r.ok => format!(
                "Reset failed: {}",
                r.output.lines().next().unwrap_or("git refused")
            ),
            Some(r) => {
                let mut text = format!("Reset to {}", p.upstream);
                if !p.dropped.is_empty() {
                    text.push_str(&format!(
                        "; gave up {} local commit(s): {}",
                        p.dropped.len(),
                        p.dropped
                            .iter()
                            .map(|d| d.split(' ').next().unwrap_or(d))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !r.conflicts.is_empty() {
                    text.push_str(&format!(
                        "; local changes kept in the stash, they conflict in {}",
                        r.conflicts.join(", ")
                    ));
                }
                text
            }
        },
    }
}

/// Whether this session already showed the notice of a Git operation without a post-quantum key
/// exchange (ADR-0025 section 1a: once per session).
static CLASSIC_KEX_NOTICE_SHOWN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `text` with the notice of `o` appended when `o` ran without a post-quantum key exchange and
/// `shown` was not yet set (it is set then).
fn with_kex_notice(
    text: String,
    o: Option<&RemoteOutcome>,
    shown: &std::sync::atomic::AtomicBool,
) -> String {
    match o.and_then(|o| o.pq_notice.as_deref()) {
        Some(notice) if !shown.swap(true, std::sync::atomic::Ordering::SeqCst) => {
            format!("{text}. Warning: {notice}")
        }
        _ => text,
    }
}

/// Status text after a reset to the upstream (`SPEC-0144` v4 behaviour 9).
pub fn reset_report(done: &Result<PullOutcome, GitError>) -> String {
    let o = done.as_ref().ok().map(|p| &p.fetch);
    with_kex_notice(reset_text(done), o, &CLASSIC_KEX_NOTICE_SHOWN)
}

/// Status text after a fetch.
pub fn fetch_report(done: &Result<RemoteOutcome, GitError>) -> String {
    with_kex_notice(
        fetch_text(done),
        done.as_ref().ok(),
        &CLASSIC_KEX_NOTICE_SHOWN,
    )
}

/// Status text after Update Project.
pub fn pull_report(done: &Result<PullOutcome, GitError>) -> String {
    let o = done.as_ref().ok().map(|p| &p.fetch);
    with_kex_notice(pull_text(done), o, &CLASSIC_KEX_NOTICE_SHOWN)
}

/// Status text after a push of `count` commits.
pub fn push_report(done: &Result<RemoteOutcome, GitError>, count: usize) -> String {
    with_kex_notice(
        push_text(done, count),
        done.as_ref().ok(),
        &CLASSIC_KEX_NOTICE_SHOWN,
    )
}

/// Why a remote operation failed, from git's outcome: credentials, time limit, rejection or git's
/// first message.
fn failure(o: &RemoteOutcome) -> String {
    if let Some(why) = &o.ssh_refusal {
        return why.clone();
    }
    if o.auth_required {
        return "the remote needs credentials; set up an SSH key or a credential helper".into();
    }
    if o.timed_out {
        return "the remote did not answer in time".into();
    }
    // `parse_updates` names git's `!` flag "rejected" (hand test 70: the reason was lost).
    if let Some(u) = o
        .updates
        .iter()
        .find(|u| u.kind == "rejected" || u.kind == "!")
    {
        let why = u
            .summary
            .trim()
            .trim_start_matches("[remote rejected]")
            .trim_start_matches("[rejected]")
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .to_owned();
        return match rejection_hint(&why) {
            Some(hint) => format!("{} rejected ({why}): {hint}", short_ref(&u.reference)),
            None => format!("{} rejected ({why})", short_ref(&u.reference)),
        };
    }
    o.messages
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("git failed")
        .trim()
        .to_owned()
}

/// What the user can do about a rejected reference, from git's reason (`SPEC-0144` v2
/// behaviour 4).
fn rejection_hint(why: &str) -> Option<&'static str> {
    if why.contains("stale info") {
        Some(
            "someone pushed to it since the last fetch; fetch, look at their commits, then push again",
        )
    } else if why.contains("fetch first") || why.contains("non-fast-forward") {
        Some("the remote has commits that the branch lacks; Update Project first, then push")
    } else if why.contains("already exists") {
        Some("the remote has a different tag of that name")
    } else {
        None
    }
}

/// `refs/heads/main` → `main`, `refs/tags/v1` → `tag v1`.
fn short_ref(reference: &str) -> String {
    match (
        reference.strip_prefix("refs/heads/"),
        reference.strip_prefix("refs/tags/"),
    ) {
        (Some(b), _) => b.to_owned(),
        (_, Some(t)) => format!("tag {t}"),
        _ => reference.to_owned(),
    }
}

/// Status text after a fetch.
fn fetch_text(done: &Result<RemoteOutcome, GitError>) -> String {
    match done {
        Err(e) => format!("Fetch failed: {e}"),
        Ok(o) if !o.ok => format!("Fetch failed: {}", failure(o)),
        Ok(o) => match o.updates.iter().filter(|u| u.kind != "=").count() {
            0 => "Fetch: everything is up to date".into(),
            n => format!("Fetched {n} reference(s)"),
        },
    }
}

/// Status text after Update Project (fetch and merge of the upstream).
fn pull_text(done: &Result<PullOutcome, GitError>) -> String {
    match done {
        Err(e) => format!("Update failed: {e}"),
        Ok(p) if !p.fetch.ok => format!("Update failed: {}", failure(&p.fetch)),
        Ok(p) if p.rebase.is_some() => rebase_report(p),
        Ok(p) => match &p.merge {
            None => format!("Update: nothing merged from {}", p.upstream),
            Some(m) if !m.local_conflicts.is_empty() => format!(
                "Update stopped: local changes in {} would be overwritten",
                m.local_conflicts.join(", ")
            ),
            Some(m) if m.kind == MergeKind::Conflict => format!(
                "Update stopped: {} conflicts with {} — resolve with Resolve Conflicts…",
                m.merge_conflicts.join(", "),
                p.upstream
            ),
            Some(m) if m.kind == MergeKind::UpToDate => "Update: already up to date".into(),
            Some(m) => format!(
                "Updated from {}: {} file(s) changed",
                p.upstream,
                m.changes.len()
            ),
        },
    }
}

/// The rows of a stash's diff (`SPEC-0145`): per file a header row (line 0) with its path and
/// whether it is new or deleted, then its side-by-side rows.
pub fn stash_diff_rows(files: &[idecraft_vcs::StashFile]) -> Vec<idecraft_vcs::DiffRow> {
    let mut rows = Vec::new();
    for f in files {
        let what = match (f.old.is_empty(), f.new.is_empty()) {
            (true, false) => " (new)",
            (false, true) => " (deleted)",
            _ => "",
        };
        let header = format!("── {}{what}", f.path);
        rows.push(idecraft_vcs::DiffRow {
            kind: None,
            left: Some((0, header.clone())),
            right: Some((0, header)),
        });
        rows.extend(idecraft_vcs::side_by_side(&f.old, &f.new));
    }
    rows
}

/// Status text after Update Project with rebase (`SPEC-0144`).
fn rebase_report(p: &PullOutcome) -> String {
    let Some(r) = &p.rebase else {
        return String::new();
    };
    if r.ok && r.output.contains("is up to date") {
        "Update: already up to date".into()
    } else if r.ok {
        format!(
            "Updated from {} by rebase: {} commit(s) replayed",
            p.upstream,
            p.replayed.len()
        )
    } else if !r.conflicts.is_empty() {
        format!(
            "Update stopped: {} conflicts while rebasing onto {} — resolve, then Git.Continue or Git.Abort",
            r.conflicts.join(", "),
            p.upstream
        )
    } else {
        format!(
            "Update failed: {}",
            r.output.lines().next().unwrap_or("rebase stopped")
        )
    }
}

/// Status text after a push of `count` commits.
fn push_text(done: &Result<RemoteOutcome, GitError>, count: usize) -> String {
    match done {
        Err(e) => format!("Push failed: {e}"),
        Ok(o) if !o.ok => format!("Push failed: {}", failure(o)),
        Ok(_) => format!("Pushed {count} commit(s)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_vcs::RefUpdate;

    fn outcome(ok: bool, updates: &[(&str, &str)], messages: &str) -> RemoteOutcome {
        RemoteOutcome {
            ok,
            command: Vec::new(),
            updates: updates
                .iter()
                .map(|(kind, reference)| RefUpdate {
                    kind: (*kind).into(),
                    reference: (*reference).into(),
                    summary: "non-fast-forward".into(),
                })
                .collect(),
            auth_required: false,
            ssh_refusal: None,
            timed_out: false,
            messages: messages.into(),
            pq: None,
            pq_notice: None,
        }
    }

    #[test]
    fn a_classical_key_exchange_is_named_once_per_session() {
        let shown = std::sync::atomic::AtomicBool::new(false);
        let mut azure = outcome(true, &[("=", "refs/remotes/origin/main")], "");
        azure.pq = Some(false);
        azure.pq_notice = Some("Git over SSH to ssh.dev.azure.com used the classical key exchange diffie-hellman-group18-sha512".into());
        let first = with_kex_notice(fetch_text(&Ok(azure.clone())), Some(&azure), &shown);
        assert_eq!(
            first,
            "Fetch: everything is up to date. Warning: Git over SSH to ssh.dev.azure.com used the classical key exchange diffie-hellman-group18-sha512"
        );
        let again = with_kex_notice(fetch_text(&Ok(azure.clone())), Some(&azure), &shown);
        assert_eq!(again, "Fetch: everything is up to date");
        // Post-quantum connections never carry the notice.
        let fresh = std::sync::atomic::AtomicBool::new(false);
        let github = outcome(true, &[], "");
        assert_eq!(
            with_kex_notice("Pushed 1 commit(s)".into(), Some(&github), &fresh),
            "Pushed 1 commit(s)"
        );
        assert!(!fresh.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn reports_say_what_happened() {
        assert_eq!(
            fetch_report(&Ok(outcome(true, &[("=", "refs/remotes/origin/main")], ""))),
            "Fetch: everything is up to date"
        );
        assert_eq!(
            fetch_report(&Ok(outcome(true, &[(" ", "refs/remotes/origin/x")], ""))),
            "Fetched 1 reference(s)"
        );
        let mut auth = outcome(false, &[], "fatal: could not read Username");
        auth.auth_required = true;
        assert!(push_report(&Ok(auth), 2).contains("needs credentials"));
        // ADR-0025: a host without a post-quantum key exchange is named, not "credentials".
        let mut classic = outcome(false, &[], "no matching key exchange method found");
        classic.ssh_refusal =
            Some("dev.azure.com: the host offers no post-quantum key exchange (…)".into());
        assert!(push_report(&Ok(classic), 1).contains("no post-quantum key exchange"));
        let rejected = outcome(false, &[("rejected", "refs/heads/main")], "");
        assert_eq!(
            push_report(&Ok(rejected), 1),
            "Push failed: main rejected (non-fast-forward): the remote has commits that the branch lacks; Update Project first, then push"
        );
        // Hand test 70: a lease push after someone else pushed says why it was refused.
        let mut stale = outcome(
            false,
            &[("rejected", "refs/heads/ziel")],
            "error: failed to push some refs",
        );
        stale.updates[0].summary = "[rejected] (stale info)".into();
        assert_eq!(
            push_report(&Ok(stale), 1),
            "Push failed: ziel rejected (stale info): someone pushed to it since the last fetch; fetch, look at their commits, then push again"
        );
        assert_eq!(
            push_report(&Ok(outcome(true, &[], "")), 3),
            "Pushed 3 commit(s)"
        );
        assert_eq!(
            pull_report(&Err(GitError::Rejected(
                "branch main has no upstream".into()
            ))),
            "Update failed: branch main has no upstream"
        );
    }

    #[test]
    fn operations_digits_push_target_history_and_update_rows() {
        // SPEC-0109 v2 behaviour 6: the reference's first ten entries with 1–9 and 0.
        assert_eq!(OPERATIONS[0].1, "CheckinProject");
        assert_eq!(OPERATIONS[7].1, "Vcs.Push");
        assert_eq!(OPERATIONS[9].1, "Git.Unstash");
        assert_eq!(operation_digit(0), Some('1'));
        assert_eq!(operation_digit(9), Some('0'));
        assert_eq!(operation_digit(10), None);
        assert_eq!(operation_for_digit("0"), Some(9));
        assert_eq!(operation_for_digit("3"), Some(2));
        assert_eq!(operation_for_digit("x"), None);
        // L24: a branch the remote has is not new.
        let out = Outgoing {
            branch: "main".into(),
            remote: "origin".into(),
            has_upstream: false,
            commits: Vec::new(),
            files: Default::default(),
            remote_branches: vec!["main".into(), "ziel".into()],
            up_to: None,
        };
        assert!(!push_creates_branch(&out, "ziel"));
        assert!(!push_creates_branch(&out, "main"));
        assert!(push_creates_branch(&out, "review/me"));
        // Stash messages: typed newest first, then the list's own, without Git's WIP entries.
        let stashes = [
            Stash {
                index: 0,
                message: "On main: half done".into(),
            },
            Stash {
                index: 1,
                message: "WIP on main: abc1234 subject".into(),
            },
            Stash {
                index: 2,
                message: "On topic: typed".into(),
            },
        ];
        assert_eq!(
            stash_message_history(&["older".into(), "typed".into()], &stashes),
            ["typed", "older", "half done"]
        );
        let info = UpdateInfo {
            from: "a".into(),
            to: "b".into(),
            commits: vec![idecraft_vcs::CommitInfo {
                id: "0123456789".into(),
                short_id: "0123456".into(),
                summary: "b1".into(),
                author: "Anna Muster".into(),
                email: "anna@example.invalid".into(),
                time: 0,
                parents: Vec::new(),
                committer: None,
            }],
            files: vec![idecraft_vcs::ChangedPath {
                status: "A".into(),
                path: "b.txt".into(),
            }],
        };
        let (title, rows) = update_info_rows(&info);
        assert!(title.starts_with("Update Info: 1 commit(s), 1 file(s)"));
        assert_eq!(rows, ["0123456  b1 — Anna Muster", "A  b.txt"]);
    }

    #[test]
    fn reset_reports_name_what_was_given_up() {
        let op = |ok: bool, conflicts: &[&str]| idecraft_vcs::OpOutcome {
            ok,
            conflicts: conflicts.iter().map(|c| (*c).to_owned()).collect(),
            in_progress: None,
            output: "HEAD is now at abc".into(),
        };
        let pull = |reset, dropped: Vec<String>| {
            Ok(PullOutcome {
                fetch: outcome(true, &[], ""),
                merge: None,
                upstream: "origin/main".into(),
                rebase: None,
                replayed: Vec::new(),
                info: None,
                dropped,
                reset: Some(reset),
            })
        };
        assert_eq!(
            reset_report(&pull(op(true, &[]), vec!["abc1234 mine".into()])),
            "Reset to origin/main; gave up 1 local commit(s): abc1234"
        );
        assert!(
            reset_report(&pull(op(true, &["a.txt"]), Vec::new()))
                .ends_with("local changes kept in the stash, they conflict in a.txt")
        );
        assert!(reset_report(&pull(op(false, &[]), Vec::new())).starts_with("Reset failed"));
        assert!(reset_choice_label(2).contains("giving up 2 local commits"));
        assert!(reset_choice_label(1).contains("giving up 1 local commit ("));
        assert!(reset_choice_label(0).contains("no local commits"));
    }

    #[test]
    fn stash_rows_have_a_header_per_file() {
        let files = [
            idecraft_vcs::StashFile {
                path: "a.txt".into(),
                old: "one\n".into(),
                new: "two\n".into(),
            },
            idecraft_vcs::StashFile {
                path: "new.txt".into(),
                old: String::new(),
                new: "x\n".into(),
            },
        ];
        let rows = stash_diff_rows(&files);
        let headers: Vec<&str> = rows
            .iter()
            .filter(|r| r.left.as_ref().is_some_and(|l| l.0 == 0))
            .map(|r| r.left.as_ref().unwrap().1.as_str())
            .collect();
        assert_eq!(headers, ["── a.txt", "── new.txt (new)"]);
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn rebase_reports() {
        let op = |ok: bool, conflicts: &[&str], output: &str| idecraft_vcs::OpOutcome {
            ok,
            conflicts: conflicts.iter().map(|c| (*c).to_owned()).collect(),
            in_progress: None,
            output: output.into(),
        };
        let pull = |rebase| {
            Ok(PullOutcome {
                fetch: outcome(true, &[], ""),
                merge: None,
                upstream: "origin/main".into(),
                rebase: Some(rebase),
                replayed: vec!["abc1234 a1".into()],
                info: None,
                dropped: Vec::new(),
                reset: None,
            })
        };
        assert_eq!(
            pull_report(&pull(op(true, &[], ""))),
            "Updated from origin/main by rebase: 1 commit(s) replayed"
        );
        assert_eq!(
            pull_report(&pull(op(true, &[], "Current branch main is up to date."))),
            "Update: already up to date"
        );
        assert!(
            pull_report(&pull(op(false, &["a.txt"], "")))
                .starts_with("Update stopped: a.txt conflicts while rebasing onto origin/main")
        );
    }
}
