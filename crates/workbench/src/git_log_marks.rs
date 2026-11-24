//! Labels and commit facts of the Git log tool window (`SPEC-0086` v7), free of the UI toolkit:
//! the branch and tag labels of a row as the reference places them — one label for a local
//! branch and the remote branch of the same name on the same commit, tags with their symbol —
//! and the facts that decide the commit context menu, read from the loaded history.

use std::collections::{BTreeMap, HashMap, HashSet};

use ferris_idea_vcs::{CommitFacts, CommitInfo};

use crate::git_log_model::reachable;

/// What a label stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    /// The checked-out branch (alone or with its remote branch).
    Current,
    /// A local branch (alone or with its remote branch).
    Local,
    /// A remote-tracking branch without a local branch of the same name here.
    Remote,
    /// A tag, drawn with the tag symbol.
    Tag,
    /// A detached HEAD.
    Head,
}

/// One label of a row (`SPEC-0086` v7 behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Text shown: `main`, `origin & main`, `origin/x`, a tag's name.
    pub text: String,
    /// Kind, for its look.
    pub kind: LabelKind,
    /// The full names it stands for (`main`, `origin/main`), for the tooltip.
    pub names: Vec<String>,
}

/// The labels of one commit in the order the row shows them: HEAD while detached, the current
/// branch, the other local branches, remote branches, tags. A local branch and a remote branch
/// `<remote>/<same name>` on the same commit become one label `<remote> & <name>`. `remotes` are
/// the names of the remote-tracking branches; names neither local nor remote are tags.
pub fn row_labels(
    names: &[String],
    current: Option<&str>,
    locals: &HashSet<String>,
    remotes: &HashSet<String>,
) -> Vec<Label> {
    let mut out = Vec::new();
    let mut used: HashSet<&str> = HashSet::new();
    if names.iter().any(|n| n == "HEAD") {
        out.push(Label {
            text: "HEAD".into(),
            kind: LabelKind::Head,
            names: vec!["HEAD".into()],
        });
        used.insert("HEAD");
    }
    let mut local: Vec<&String> = names.iter().filter(|n| locals.contains(*n)).collect();
    local.sort_by_key(|n| (Some(n.as_str()) != current, n.as_str()));
    for name in local {
        used.insert(name);
        let partners: Vec<&String> = names
            .iter()
            .filter(|r| {
                remotes.contains(*r)
                    && r.split_once('/')
                        .is_some_and(|(_, rest)| rest == name.as_str())
            })
            .collect();
        let mut all = vec![name.clone()];
        let text = if partners.is_empty() {
            name.clone()
        } else {
            let mut remote_names: Vec<&str> = Vec::new();
            for r in &partners {
                used.insert(r);
                all.push((*r).clone());
                remote_names.extend(r.split_once('/').map(|(remote, _)| remote));
            }
            format!("{} & {name}", remote_names.join(" & "))
        };
        out.push(Label {
            text,
            kind: if Some(name.as_str()) == current {
                LabelKind::Current
            } else {
                LabelKind::Local
            },
            names: all,
        });
    }
    let mut rest: Vec<&String> = names
        .iter()
        .filter(|n| !used.contains(n.as_str()))
        .collect();
    rest.sort_by_key(|n| (!remotes.contains(*n), n.as_str()));
    for name in rest {
        out.push(Label {
            text: name.clone(),
            kind: if remotes.contains(name) {
                LabelKind::Remote
            } else {
                LabelKind::Tag
            },
            names: vec![name.clone()],
        });
    }
    out
}

/// The branch labels of a commit the commit menu offers a page for (`SPEC-0086` v8 behaviour
/// 17): every local and remote branch at the commit but the current branch, local ones first,
/// each with whether it is remote.
pub fn menu_branches(
    names: &[String],
    current: Option<&str>,
    locals: &HashSet<String>,
    remotes: &HashSet<String>,
) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = names
        .iter()
        .filter(|n| Some(n.as_str()) != current)
        .filter_map(|n| {
            if locals.contains(n) {
                Some((n.clone(), false))
            } else if remotes.contains(n) {
                Some((n.clone(), true))
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
    out
}

/// The commit facts of the history as the window has read it (`SPEC-0086` v7 behaviour 13).
pub struct HistoryFacts {
    on_head: HashSet<String>,
    pushed: HashSet<String>,
    head: Option<String>,
    children: HashMap<String, Vec<String>>,
    detached: bool,
}

impl HistoryFacts {
    /// Facts of `commits`: `on_head` the commits reachable from HEAD, `head` HEAD's commit, the
    /// tips of `remote_tips` the remote-tracking branches.
    pub fn new(
        commits: &[CommitInfo],
        on_head: &HashSet<String>,
        head: Option<&str>,
        remote_tips: &[&str],
        detached: bool,
    ) -> Self {
        let mut pushed = HashSet::new();
        for tip in remote_tips {
            if !pushed.contains(*tip) {
                pushed.extend(reachable(commits, tip));
            }
        }
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for c in commits {
            for p in &c.parents {
                children.entry(p.clone()).or_default().push(c.id.clone());
            }
        }
        Self {
            on_head: on_head.clone(),
            pushed,
            head: head.map(str::to_owned),
            children,
            detached,
        }
    }

    /// The listed commits whose parent `id` is, newest first.
    pub fn children(&self, id: &str) -> &[String] {
        self.children.get(id).map_or(&[], Vec::as_slice)
    }

    /// The facts of commit `commit` of `commits`.
    pub fn facts(&self, commits: &[CommitInfo], commit: &CommitInfo) -> CommitFacts {
        let on_current_branch = self.on_head.contains(&commit.id);
        let merges_after = on_current_branch && {
            let below = reachable(commits, &commit.id);
            commits.iter().any(|c| {
                c.parents.len() > 1 && self.on_head.contains(&c.id) && !below.contains(&c.id)
            })
        };
        CommitFacts {
            is_head: self.head.as_deref() == Some(commit.id.as_str()),
            on_current_branch,
            pushed: self.pushed.contains(&commit.id),
            merge: commit.parents.len() > 1,
            root: commit.parents.is_empty(),
            merges_after,
            detached: self.detached,
            has_child: !self.children(&commit.id).is_empty(),
        }
    }
}

/// The tips of the remote-tracking branches among `labels` (`remotes` their names).
pub fn remote_tips<'a>(
    labels: &'a BTreeMap<String, Vec<String>>,
    remotes: &HashSet<String>,
) -> Vec<&'a str> {
    labels
        .iter()
        .filter(|(_, names)| names.iter().any(|n| remotes.contains(n)))
        .map(|(id, _)| id.as_str())
        .collect()
}

/// The author column's text: the author, with `*` when someone else committed (`SPEC-0086` v7
/// behaviour 15).
pub fn author_text(commit: &CommitInfo) -> String {
    match commit.committer {
        Some(_) => format!("{}*", commit.author),
        None => commit.author.clone(),
    }
}

/// The counters of a local branch in the branches pane: `↙` the commits of the upstream the
/// branch lacks (incoming), `↗` its own the upstream lacks (outgoing); empty when both are 0
/// (`SPEC-0086` v7 behaviour 16).
pub fn tracking_text(ahead: usize, behind: usize) -> String {
    let mut parts = Vec::new();
    if behind > 0 {
        parts.push(format!("↙{behind}"));
    }
    if ahead > 0 {
        parts.push(format!("↗{ahead}"));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_name_incoming_then_outgoing() {
        assert_eq!(tracking_text(0, 0), "");
        assert_eq!(tracking_text(1, 0), "↗1");
        assert_eq!(tracking_text(1, 2), "↙2 ↗1");
    }

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn menu_branches_skip_the_current_one() {
        let locals = set(&["main", "topic", "b-local"]);
        let remotes = set(&["origin/main", "origin/topic"]);
        let at = names(&[
            "origin/topic",
            "topic",
            "main",
            "v1",
            "b-local",
            "origin/main",
        ]);
        assert_eq!(
            menu_branches(&at, Some("main"), &locals, &remotes),
            [
                ("b-local".to_owned(), false),
                ("topic".to_owned(), false),
                ("origin/main".to_owned(), true),
                ("origin/topic".to_owned(), true),
            ]
        );
        assert!(menu_branches(&names(&["v1"]), None, &locals, &remotes).is_empty());
    }

    #[test]
    fn local_and_remote_on_one_commit_are_one_label() {
        let locals = set(&["main", "feature/discount"]);
        let remotes = set(&["origin/main", "origin/feature/discount", "origin/old"]);
        let labels = row_labels(
            &names(&[
                "feature/discount",
                "main",
                "origin/feature/discount",
                "origin/main",
                "v1.0",
                "origin/old",
            ]),
            Some("main"),
            &locals,
            &remotes,
        );
        let texts: Vec<_> = labels.iter().map(|l| (l.text.as_str(), l.kind)).collect();
        assert_eq!(
            texts,
            [
                ("origin & main", LabelKind::Current),
                ("origin & feature/discount", LabelKind::Local),
                ("origin/old", LabelKind::Remote),
                ("v1.0", LabelKind::Tag),
            ]
        );
        assert_eq!(labels[0].names, ["main", "origin/main"]);
    }

    #[test]
    fn a_remote_branch_elsewhere_stays_apart_and_head_comes_first() {
        let labels = row_labels(
            &names(&["main", "HEAD"]),
            None,
            &set(&["main"]),
            &set(&["origin/main"]),
        );
        assert_eq!(labels[0].kind, LabelKind::Head);
        assert_eq!(labels[1].text, "main");
        assert_eq!(labels[1].kind, LabelKind::Local);
    }

    fn commit(id: &str, parents: &[&str]) -> CommitInfo {
        CommitInfo {
            id: id.into(),
            short_id: id.into(),
            summary: id.into(),
            author: "A".into(),
            email: "a@example.invalid".into(),
            time: 0,
            parents: names(parents),
            committer: None,
        }
    }

    #[test]
    fn facts_from_the_loaded_history() {
        // d (HEAD, main) — c (origin/main) — b — a; s (side) on b; m is a merge on top of side.
        let commits = vec![
            commit("d", &["c"]),
            commit("m", &["s", "b"]),
            commit("s", &["b"]),
            commit("c", &["b"]),
            commit("b", &["a"]),
            commit("a", &[]),
        ];
        let on_head = reachable(&commits, "d");
        let h = HistoryFacts::new(&commits, &on_head, Some("d"), &["c"], false);
        let f = |id: &str| h.facts(&commits, commits.iter().find(|c| c.id == id).unwrap());
        let d = f("d");
        assert!(d.is_head && d.on_current_branch && !d.pushed && !d.has_child);
        let c = f("c");
        assert!(c.pushed && c.has_child && !c.is_head);
        assert!(f("a").root && f("a").pushed);
        let s = f("s");
        assert!(!s.on_current_branch && s.has_child);
        assert!(f("m").merge);
        assert_eq!(h.children("b"), ["m", "s", "c"]);
        // A merge on HEAD's line makes rewriting the commits below it non-linear.
        let on_m = reachable(&commits, "m");
        let h = HistoryFacts::new(&commits, &on_m, Some("m"), &[], false);
        let b = commits.iter().find(|c| c.id == "b").unwrap();
        assert!(h.facts(&commits, b).merges_after);
        let s = commits.iter().find(|c| c.id == "s").unwrap();
        assert!(h.facts(&commits, s).merges_after);
    }

    #[test]
    fn a_different_committer_gets_a_star() {
        let mut c = commit("a", &[]);
        assert_eq!(author_text(&c), "A");
        c.committer = Some("B <b@example.invalid>".into());
        assert_eq!(author_text(&c), "A*");
    }
}
