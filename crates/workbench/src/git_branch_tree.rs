//! The branches pane of the Git log tool window (`SPEC-0086` v4 behaviour 9), free of the UI
//! toolkit: the current branch on top, folders "Local" and "Remote", name prefixes before a `/` as
//! folders, and a filter; favourites first with a star and a switch to show only them (v10
//! behaviour 19).

use std::collections::{BTreeMap, BTreeSet};

/// What a row of the tree stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeItem {
    /// A folder; the key opens and closes it (`local`, `remote`, `local/feature`, `remote/origin`).
    Folder(String),
    /// A branch by its full short name (`main`, `feature/discount`, `origin/main`).
    Branch(String),
}

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// Indentation level.
    pub depth: usize,
    /// Text shown: the last part of the name, a folder's name, or the current branch in full.
    pub text: String,
    /// What choosing the row does.
    pub item: TreeItem,
    /// Whether this is the current branch.
    pub current: bool,
    /// For a folder: whether it is open.
    pub open: bool,
    /// For a branch: whether it is a favourite (`SPEC-0086` v10 behaviour 19).
    pub favorite: bool,
}

/// Text of the first row while a branch is checked out (`SPEC-0086` v7 behaviour 16).
pub const CURRENT_ROW: &str = "HEAD (Current Branch)";

/// A branch of the repository as the pane needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchName {
    /// Short name (`feature/discount`, `origin/main`).
    pub name: String,
    /// Remote-tracking branch.
    pub remote: bool,
    /// HEAD points at it.
    pub current: bool,
    /// A favourite (`SPEC-0086` v10 behaviour 19).
    pub favorite: bool,
}

#[derive(Default)]
struct Node {
    folders: BTreeMap<String, Node>,
    /// Not a favourite (so favourites sort first), last part, full name.
    branches: BTreeSet<(bool, String, String)>,
}

impl Node {
    fn insert(&mut self, parts: &[&str], full: &str, favorite: bool) {
        match parts {
            [] => {}
            [leaf] => {
                self.branches
                    .insert((!favorite, (*leaf).to_owned(), full.to_owned()));
            }
            [folder, rest @ ..] => self
                .folders
                .entry((*folder).to_owned())
                .or_default()
                .insert(rest, full, favorite),
        }
    }

    fn branch_row(
        depth: usize,
        leaf: &str,
        full: &str,
        favorite: bool,
        current: Option<&str>,
    ) -> TreeRow {
        TreeRow {
            depth,
            text: leaf.to_owned(),
            item: TreeItem::Branch(full.to_owned()),
            current: current == Some(full),
            open: false,
            favorite,
        }
    }

    fn rows(
        &self,
        key: &str,
        depth: usize,
        closed: &BTreeSet<String>,
        filtering: bool,
        current: Option<&str>,
        out: &mut Vec<TreeRow>,
    ) {
        // Favourites first, then the folders, then the other branches (v10 behaviour 19).
        for (_, leaf, full) in self.branches.iter().filter(|b| !b.0) {
            out.push(Self::branch_row(depth, leaf, full, true, current));
        }
        for (name, child) in &self.folders {
            let key = format!("{key}/{name}");
            let open = filtering || !closed.contains(&key);
            out.push(TreeRow {
                depth,
                text: name.clone(),
                item: TreeItem::Folder(key.clone()),
                current: false,
                open,
                favorite: false,
            });
            if open {
                child.rows(&key, depth + 1, closed, filtering, current, out);
            }
        }
        for (_, leaf, full) in self.branches.iter().filter(|b| b.0) {
            out.push(Self::branch_row(depth, leaf, full, false, current));
        }
    }
}

/// The visible rows of the tree (`SPEC-0086` v4 behaviour 9): the current branch (or `HEAD` while
/// detached), then "Local" and "Remote" with their folders. `closed` holds the keys of closed
/// folders; a non-empty `filter` keeps only the branches whose full name contains it (case ignored)
/// and opens every folder on their way.
pub fn tree_rows(
    branches: &[BranchName],
    detached: bool,
    filter: &str,
    closed: &BTreeSet<String>,
) -> Vec<TreeRow> {
    tree_rows_with(branches, detached, filter, closed, false)
}

/// As [`tree_rows`]; with `only_favorites` the tree keeps the HEAD row and the favourite branches,
/// with the folders on their way open (`SPEC-0086` v10 behaviour 19).
pub fn tree_rows_with(
    branches: &[BranchName],
    detached: bool,
    filter: &str,
    closed: &BTreeSet<String>,
    only_favorites: bool,
) -> Vec<TreeRow> {
    let needle = filter.trim().to_lowercase();
    let keep = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);
    let filtering = !needle.is_empty() || only_favorites;
    let current_branch = branches.iter().find(|b| b.current);
    let current = current_branch.map(|b| b.name.as_str());
    let current_favorite = current_branch.is_some_and(|b| b.favorite);
    let mut out = Vec::new();
    match current {
        // The reference names the row "HEAD (Current Branch)" (`SPEC-0086` v7 behaviour 16).
        Some(name) if keep(name) => out.push(TreeRow {
            depth: 0,
            text: CURRENT_ROW.into(),
            item: TreeItem::Branch(name.to_owned()),
            current: true,
            open: false,
            favorite: current_favorite,
        }),
        None if detached && keep("HEAD") => out.push(TreeRow {
            depth: 0,
            text: "HEAD".into(),
            item: TreeItem::Branch("HEAD".into()),
            current: true,
            open: false,
            favorite: false,
        }),
        _ => {}
    }
    for (remote, key, title) in [(false, "local", "Local"), (true, "remote", "Remote")] {
        let mut root = Node::default();
        for b in branches
            .iter()
            .filter(|b| b.remote == remote && keep(&b.name) && (b.favorite || !only_favorites))
        {
            let parts: Vec<&str> = b.name.split('/').filter(|p| !p.is_empty()).collect();
            root.insert(&parts, &b.name, b.favorite);
        }
        if root.folders.is_empty() && root.branches.is_empty() {
            continue;
        }
        let open = filtering || !closed.contains(key);
        out.push(TreeRow {
            depth: 0,
            text: title.into(),
            item: TreeItem::Folder(key.into()),
            current: false,
            open,
            favorite: false,
        });
        if open {
            root.rows(key, 1, closed, filtering, current, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, remote: bool, current: bool) -> BranchName {
        BranchName {
            name: name.into(),
            remote,
            current,
            favorite: false,
        }
    }

    fn texts(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.text))
            .collect()
    }

    fn sample() -> Vec<BranchName> {
        vec![
            b("main", false, true),
            b("feature/discount", false, false),
            b("feature/api/v2", false, false),
            b("bugfix", false, false),
            b("origin/main", true, false),
            b("origin/feature/discount", true, false),
            b("upstream/main", true, false),
        ]
    }

    /// SPEC-0086 v4 AK-10.
    #[test]
    fn current_on_top_local_and_remote_with_prefix_folders() {
        let rows = tree_rows(&sample(), false, "", &BTreeSet::new());
        assert_eq!(
            texts(&rows),
            [
                CURRENT_ROW,
                "Local",
                "  feature",
                "    api",
                "      v2",
                "    discount",
                "  bugfix",
                "  main",
                "Remote",
                "  origin",
                "    feature",
                "      discount",
                "    main",
                "  upstream",
                "    main",
            ]
        );
        assert!(
            rows[0].current && rows[7].current,
            "the current branch is marked in Local too"
        );
        assert_eq!(rows[5].item, TreeItem::Branch("feature/discount".into()));
        assert_eq!(rows[9].item, TreeItem::Folder("remote/origin".into()));
    }

    #[test]
    fn closed_folders_hide_their_branches() {
        let closed: BTreeSet<String> = ["local/feature".to_owned(), "remote".to_owned()].into();
        let rows = tree_rows(&sample(), false, "", &closed);
        assert_eq!(
            texts(&rows),
            [
                CURRENT_ROW,
                "Local",
                "  feature",
                "  bugfix",
                "  main",
                "Remote"
            ]
        );
        assert!(!rows[2].open && !rows[5].open && rows[1].open);
    }

    #[test]
    fn the_filter_keeps_matching_branches_with_their_folders_open() {
        let closed: BTreeSet<String> = ["local/feature".to_owned()].into();
        let rows = tree_rows(&sample(), false, "DISC", &closed);
        assert_eq!(
            texts(&rows),
            [
                "Local",
                "  feature",
                "    discount",
                "Remote",
                "  origin",
                "    feature",
                "      discount",
            ]
        );
        assert!(tree_rows(&sample(), false, "nothing", &BTreeSet::new()).is_empty());
    }

    #[test]
    fn detached_head_on_top() {
        let mut branches = sample();
        branches[0].current = false;
        let rows = tree_rows(&branches, true, "", &BTreeSet::new());
        assert_eq!(rows[0].text, "HEAD");
        assert!(rows[0].current);
        assert!(rows.iter().skip(1).all(|r| !r.current));
    }

    fn starred(names: &[&str]) -> Vec<BranchName> {
        let mut all = sample();
        for b in &mut all {
            b.favorite = names.contains(&b.name.as_str());
        }
        all
    }

    /// SPEC-0086 v10 AK-30: favourites first in every folder, before the sub-folders.
    #[test]
    fn favourites_first_in_every_folder() {
        let rows = tree_rows(
            &starred(&["main", "feature/discount", "origin/main"]),
            false,
            "",
            &BTreeSet::new(),
        );
        assert_eq!(
            texts(&rows),
            [
                CURRENT_ROW,
                "Local",
                "  main",
                "  feature",
                "    discount",
                "    api",
                "      v2",
                "  bugfix",
                "Remote",
                "  origin",
                "    main",
                "    feature",
                "      discount",
                "  upstream",
                "    main",
            ]
        );
        assert!(
            rows[0].favorite,
            "the HEAD row shows the current branch's star"
        );
        assert!(rows[2].favorite && rows[4].favorite && rows[10].favorite);
        assert!(!rows[3].favorite && !rows[7].favorite && !rows[14].favorite);
    }

    /// SPEC-0086 v10 AK-30: Show Only Favorites keeps HEAD and the favourites, folders open, and
    /// the text filter applies on top.
    #[test]
    fn only_favourites_keeps_head_and_their_folders() {
        let closed: BTreeSet<String> = ["local/feature".to_owned(), "remote".to_owned()].into();
        let all = starred(&["feature/discount", "origin/main"]);
        let rows = tree_rows_with(&all, false, "", &closed, true);
        assert_eq!(
            texts(&rows),
            [
                CURRENT_ROW,
                "Local",
                "  feature",
                "    discount",
                "Remote",
                "  origin",
                "    main",
            ]
        );
        let rows = tree_rows_with(&all, false, "disc", &closed, true);
        assert_eq!(texts(&rows), ["Local", "  feature", "    discount"]);
        assert!(tree_rows_with(&sample(), false, "", &BTreeSet::new(), true).len() == 1);
    }
}
