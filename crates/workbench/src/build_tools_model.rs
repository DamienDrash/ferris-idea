//! Rows of the build tools window (`SPEC-0100` v3), without toolkit: problems, the dependency
//! analyzer, the Maven profiles, the modules nested as their builds nest them, each with its task
//! groups (Maven lifecycle and plugins, Gradle task groups, tasks with their triggers and
//! shortcuts) and its dependency tree.

use std::collections::{HashMap, HashSet};

use idecraft_core::buildtools::{Task, TaskGroup};
use idecraft_core::{DepNode, Module, ModuleKind};

/// One module with what the window shows of it.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The module.
    pub module: Module,
    /// Task groups (behaviour 2).
    pub groups: Vec<TaskGroup>,
    /// Dependency tree, or why it could not be read.
    pub deps: Result<Vec<DepNode>, String>,
}

/// A problem shown above the modules (behaviour 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// What is wrong.
    pub text: String,
    /// The build file and 1-based line it is about, when known.
    pub place: Option<(String, usize)>,
}

/// What a row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// A problem; Enter or a double click opens its place.
    Problem(Option<(String, usize)>),
    /// The "Profiles" node.
    Profiles,
    /// A Maven profile and whether it is active.
    Profile(String, bool),
    /// A module.
    Module(ModuleKind),
    /// A group: "Lifecycle", "Plugins", a plugin, a Gradle task group, "Dependencies".
    Group,
    /// A task.
    Task(Box<Task>),
    /// A dependency: version, scope, repeated, missing.
    Dependency(Option<String>, String, bool, bool),
    /// A message (a lock file that could not be read).
    Note,
    /// A place of the dependency analyzer (v3 behaviour 14): the direct dependency its path starts
    /// with, and whether the version asked for there is used.
    Usage(String, bool),
}

/// One visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Stable key for expansion and selection.
    pub key: String,
    /// Nesting depth.
    pub depth: usize,
    /// Text.
    pub label: String,
    /// Dimmed text after the label (the build tool and folder of a module, a plugin's
    /// coordinates).
    pub detail: String,
    /// Whether it has children, and whether they are shown.
    pub expandable: bool,
    /// Children shown.
    pub expanded: bool,
    /// What it is.
    pub kind: RowKind,
}

/// The build tool of a module as the window names it.
pub fn tool_name(kind: ModuleKind) -> &'static str {
    kind.tool_name()
}

/// Module name as the window shows it: the Maven artifactId or the project's name (behaviour 1).
pub fn module_label(m: &Module) -> String {
    m.short_name().to_owned()
}

/// The dimmed detail of a module row: build tool and folder (`Maven · core`).
pub fn module_detail(m: &Module) -> String {
    if m.dir == "." {
        tool_name(m.kind).to_owned()
    } else {
        format!("{} · {}", tool_name(m.kind), m.dir)
    }
}

/// The module that lists module `i` as a member (a Maven aggregator, a Gradle build's root).
fn parent_of(entries: &[Entry], i: usize) -> Option<usize> {
    let m = &entries[i].module;
    entries.iter().position(|p| {
        p.module.kind == m.kind && p.module.dir != m.dir && p.module.members.contains(&m.dir)
    })
}

/// What the window shows besides the modules.
#[derive(Debug, Clone, Default)]
pub struct Extras {
    /// Problems of the build files and the last resolution.
    pub problems: Vec<Problem>,
    /// Maven profiles of the project: id, active.
    pub profiles: Vec<(String, bool)>,
    /// The dependency analyzer's result for a module (v3 behaviour 14): the module's label and
    /// every artifact of its tree.
    pub analysis: Option<(String, Vec<idecraft_core::depanalysis::Artifact>)>,
    /// What a task row shows dimmed after its label — its triggers and shortcut (v3 behaviours 18,
    /// 19) — by `<module folder>|<task>`.
    pub task_notes: HashMap<String, String>,
    /// Typed text that narrows the analyzer's list of all artifacts (v4), lower case.
    pub analysis_filter: String,
}

/// The key of [`Extras::task_notes`] for task `label` of the module in folder `dir`.
pub fn note_key(dir: &str, label: &str) -> String {
    format!("{dir}|{label}")
}

/// The analyzer's rows (v3 behaviour 14): the group, its conflicts and their places.
fn push_analysis(
    module: &str,
    artifacts: &[idecraft_core::depanalysis::Artifact],
    filter: &str,
    expanded: &HashSet<String>,
    out: &mut Vec<Row>,
) {
    let conflicts: Vec<&idecraft_core::depanalysis::Artifact> =
        artifacts.iter().filter(|a| a.conflict).collect();
    let open = expanded.contains("analysis");
    out.push(Row {
        key: "analysis".into(),
        depth: 0,
        label: "Dependency Analyzer".into(),
        detail: format!(
            "{module} · {} conflict{}",
            conflicts.len(),
            if conflicts.len() == 1 { "" } else { "s" }
        ),
        expandable: true,
        expanded: open,
        kind: RowKind::Group,
    });
    if !open {
        return;
    }
    if conflicts.is_empty() {
        out.push(Row {
            key: "analysis/none".into(),
            depth: 1,
            label: "No version conflicts".into(),
            detail: String::new(),
            expandable: false,
            expanded: false,
            kind: RowKind::Note,
        });
    }
    for (i, a) in conflicts.iter().enumerate() {
        let key = format!("analysis/{i}");
        let open = expanded.contains(&key);
        out.push(Row {
            key: key.clone(),
            depth: 1,
            label: a.name.clone(),
            detail: format!("{} · {} versions", a.chosen, a.versions.len()),
            expandable: !a.places.is_empty(),
            expanded: open,
            kind: RowKind::Group,
        });
        if open {
            push_places(a, &key, 2, out);
        }
    }
    // v4: every artifact of the tree, each with the paths that bring it in (the reference's
    // analyzer lists all resolved dependencies and their usages, OBS-0024 re-check).
    let all_open = expanded.contains("analysis/all");
    // Typing in the window narrows the list (the reference's analyzer has a search field).
    let shown: Vec<(usize, &idecraft_core::depanalysis::Artifact)> = artifacts
        .iter()
        .enumerate()
        .filter(|(_, a)| filter.is_empty() || a.name.to_lowercase().contains(filter))
        .collect();
    out.push(Row {
        key: "analysis/all".into(),
        depth: 1,
        label: "All Dependencies".into(),
        detail: if filter.is_empty() {
            format!(
                "{} artifact{}",
                artifacts.len(),
                if artifacts.len() == 1 { "" } else { "s" }
            )
        } else {
            format!("{} of {} · “{filter}”", shown.len(), artifacts.len())
        },
        expandable: !artifacts.is_empty(),
        expanded: all_open,
        kind: RowKind::Group,
    });
    if !all_open {
        return;
    }
    for (i, a) in shown {
        let key = format!("analysis/all/{i}");
        let open = expanded.contains(&key);
        out.push(Row {
            key: key.clone(),
            depth: 2,
            label: a.name.clone(),
            detail: format!(
                "{} · {} path{}{}",
                a.chosen,
                a.places.len(),
                if a.places.len() == 1 { "" } else { "s" },
                if a.conflict { " · conflict" } else { "" }
            ),
            expandable: !a.places.is_empty(),
            expanded: open,
            kind: RowKind::Group,
        });
        if open {
            push_places(a, &key, 3, out);
        }
    }
}

/// The places of artifact `a` under the row `key` at `depth`: the version asked for, whether it
/// is used, and the path from the module.
fn push_places(
    a: &idecraft_core::depanalysis::Artifact,
    key: &str,
    depth: usize,
    out: &mut Vec<Row>,
) {
    for (k, place) in a.places.iter().enumerate() {
        out.push(Row {
            key: format!("{key}/{k}"),
            depth,
            label: place.version.clone(),
            detail: format!("{} · {}", place.status, place.path.join(" → ")),
            expandable: false,
            expanded: false,
            kind: RowKind::Usage(place.direct.clone(), place.used),
        });
    }
}

/// The keys that show the analyzer with every conflict opened and the list of all artifacts.
pub fn analysis_expanded(artifacts: &[idecraft_core::depanalysis::Artifact]) -> Vec<String> {
    ["analysis".to_owned(), "analysis/all".to_owned()]
        .into_iter()
        .chain(
            (0..artifacts.iter().filter(|a| a.conflict).count()).map(|i| format!("analysis/{i}")),
        )
        .collect()
}

/// The visible rows: problems, the analyzer, profiles, then the modules (behaviour 1).
pub fn rows(entries: &[Entry], extras: &Extras, expanded: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    for (i, p) in extras.problems.iter().enumerate() {
        out.push(Row {
            key: format!("problem{i}"),
            depth: 0,
            label: p.text.clone(),
            detail: p
                .place
                .as_ref()
                .map(|(f, l)| format!("{f}:{l}"))
                .unwrap_or_default(),
            expandable: false,
            expanded: false,
            kind: RowKind::Problem(p.place.clone()),
        });
    }
    if let Some((module, artifacts)) = &extras.analysis {
        push_analysis(
            module,
            artifacts,
            &extras.analysis_filter,
            expanded,
            &mut out,
        );
    }
    if !extras.profiles.is_empty() {
        let open = expanded.contains("profiles");
        out.push(Row {
            key: "profiles".into(),
            depth: 0,
            label: "Profiles".into(),
            detail: String::new(),
            expandable: true,
            expanded: open,
            kind: RowKind::Profiles,
        });
        if open {
            for (id, active) in &extras.profiles {
                out.push(Row {
                    key: format!("profiles/{id}"),
                    depth: 1,
                    label: id.clone(),
                    detail: String::new(),
                    expandable: false,
                    expanded: false,
                    kind: RowKind::Profile(id.clone(), *active),
                });
            }
        }
    }
    let parents: Vec<Option<usize>> = (0..entries.len()).map(|i| parent_of(entries, i)).collect();
    for i in 0..entries.len() {
        if parents[i].is_none() {
            push_module(
                entries,
                &parents,
                i,
                0,
                expanded,
                &extras.task_notes,
                &mut out,
            );
        }
    }
    out
}

fn push_module(
    entries: &[Entry],
    parents: &[Option<usize>],
    i: usize,
    depth: usize,
    expanded: &HashSet<String>,
    notes: &HashMap<String, String>,
    out: &mut Vec<Row>,
) {
    let e = &entries[i];
    let key = format!("m{i}");
    let open = expanded.contains(&key);
    out.push(Row {
        key: key.clone(),
        depth,
        label: module_label(&e.module),
        detail: module_detail(&e.module),
        expandable: true,
        expanded: open,
        kind: RowKind::Module(e.module.kind),
    });
    if !open {
        return;
    }
    for (g, group) in e.groups.iter().enumerate() {
        push_group(
            group,
            &format!("{key}/g{g}"),
            depth + 1,
            (&e.module.dir, notes),
            expanded,
            out,
        );
    }
    // A module without dependencies (a Maven aggregator) has no Dependencies node.
    let empty = matches!(&e.deps, Ok(d) if d.is_empty())
        && matches!(e.module.kind, ModuleKind::Maven | ModuleKind::Gradle);
    if !empty {
        let deps_key = format!("{key}/deps");
        let deps_open = expanded.contains(&deps_key);
        out.push(Row {
            key: deps_key.clone(),
            depth: depth + 1,
            label: "Dependencies".into(),
            detail: String::new(),
            expandable: true,
            expanded: deps_open,
            kind: RowKind::Group,
        });
        if deps_open {
            match &e.deps {
                Ok(nodes) => push_deps(nodes, &deps_key, depth + 2, expanded, out),
                Err(why) => out.push(Row {
                    key: format!("{deps_key}/error"),
                    depth: depth + 2,
                    label: why.clone(),
                    detail: String::new(),
                    expandable: false,
                    expanded: false,
                    kind: RowKind::Note,
                }),
            }
        }
    }
    // Member modules after the module's own nodes, sorted by name (behaviour 1).
    let mut children: Vec<usize> = (0..entries.len())
        .filter(|&c| parents[c] == Some(i))
        .collect();
    children.sort_by_key(|&c| module_label(&entries[c].module));
    for c in children {
        push_module(entries, parents, c, depth + 1, expanded, notes, out);
    }
}

fn push_group(
    group: &TaskGroup,
    key: &str,
    depth: usize,
    (module_dir, notes): (&str, &HashMap<String, String>),
    expanded: &HashSet<String>,
    out: &mut Vec<Row>,
) {
    let open = expanded.contains(key);
    out.push(Row {
        key: key.to_owned(),
        depth,
        label: group.name.clone(),
        detail: group.detail.clone(),
        expandable: !group.tasks.is_empty() || !group.groups.is_empty(),
        expanded: open,
        kind: RowKind::Group,
    });
    if !open {
        return;
    }
    for (g, sub) in group.groups.iter().enumerate() {
        push_group(
            sub,
            &format!("{key}/{g}"),
            depth + 1,
            (module_dir, notes),
            expanded,
            out,
        );
    }
    for (t, task) in group.tasks.iter().enumerate() {
        out.push(Row {
            key: format!("{key}/t{t}"),
            depth: depth + 1,
            label: task.label.clone(),
            detail: notes
                .get(&note_key(module_dir, &task.label))
                .cloned()
                .unwrap_or_default(),
            expandable: false,
            expanded: false,
            kind: RowKind::Task(Box::new(task.clone())),
        });
    }
}

fn push_deps(
    nodes: &[DepNode],
    parent: &str,
    depth: usize,
    expanded: &HashSet<String>,
    out: &mut Vec<Row>,
) {
    for (i, n) in nodes.iter().enumerate() {
        let key = format!("{parent}/{i}");
        let open = expanded.contains(&key);
        out.push(Row {
            key: key.clone(),
            depth,
            label: n.name.clone(),
            detail: String::new(),
            expandable: !n.children.is_empty(),
            expanded: open && !n.children.is_empty(),
            kind: RowKind::Dependency(n.version.clone(), n.scope.clone(), n.repeated, n.missing),
        });
        if open {
            push_deps(&n.children, &key, depth + 1, expanded, out);
        }
    }
}

/// Keys expanded when the window opens: the profiles and every module.
pub fn initially_expanded(entries: &[Entry]) -> HashSet<String> {
    (0..entries.len())
        .map(|i| format!("m{i}"))
        .chain(std::iter::once("profiles".to_owned()))
        .collect()
}

/// Keys of every row with children, for "Expand All" (dependency trees stay as they are: they can
/// be large).
pub fn all_group_keys(entries: &[Entry], extras: &Extras) -> HashSet<String> {
    let mut all: HashSet<String> = initially_expanded(entries);
    loop {
        let before = all.len();
        for r in rows(entries, extras, &all) {
            let dependency = matches!(r.kind, RowKind::Dependency(..)) || r.key.ends_with("/deps");
            if r.expandable && !dependency {
                all.insert(r.key);
            }
        }
        if all.len() == before {
            return all;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(kind: ModuleKind, name: &str, dir: &str, members: &[&str]) -> Module {
        Module {
            kind,
            name: name.into(),
            version: None,
            dir: dir.into(),
            toolchain: None,
            dependencies: Vec::new(),
            tasks: Default::default(),
            members: members.iter().map(|m| m.to_string()).collect(),
            notes: Vec::new(),
            profiles: Vec::new(),
            plugins: Vec::new(),
        }
    }

    fn entry(m: Module, groups: Vec<TaskGroup>, deps: Vec<DepNode>) -> Entry {
        Entry {
            module: m,
            groups,
            deps: Ok(deps),
        }
    }

    fn dep(name: &str, children: Vec<DepNode>, missing: bool) -> DepNode {
        DepNode {
            name: name.into(),
            version: Some("1.0.0".into()),
            scope: "normal".into(),
            children,
            repeated: false,
            missing,
            omitted: None,
        }
    }

    fn labels(entries: &[Entry], extras: &Extras, open: &HashSet<String>) -> Vec<String> {
        rows(entries, extras, open)
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.label))
            .collect()
    }

    /// SPEC-0100 v1 AK-1 / v2 AK-1: groups and trees expand.
    #[test]
    fn groups_and_trees_expand() {
        let mut tasks = TaskGroup {
            name: "Tasks".into(),
            detail: String::new(),
            tasks: Vec::new(),
            groups: Vec::new(),
        };
        tasks.tasks.push(Task {
            label: "build".into(),
            command: vec!["cargo".into(), "build".into()],
            dir: ".".into(),
            run_name: "app [build]".into(),
            test: false,
        });
        let entries = [entry(
            module(ModuleKind::Cargo, "app", ".", &[]),
            vec![tasks],
            vec![dep(
                "serde",
                vec![dep("serde_derive", vec![], false)],
                false,
            )],
        )];
        let extras = Extras::default();
        let mut open = initially_expanded(&entries);
        assert_eq!(
            labels(&entries, &extras, &open),
            ["app", "  Tasks", "  Dependencies"]
        );
        open.insert("m0/g0".into());
        open.insert("m0/deps".into());
        open.insert("m0/deps/0".into());
        assert_eq!(
            labels(&entries, &extras, &open),
            [
                "app",
                "  Tasks",
                "    build",
                "  Dependencies",
                "    serde",
                "      serde_derive"
            ]
        );
        let RowKind::Task(task) = rows(&entries, &extras, &open)[2].kind.clone() else {
            panic!("a task row");
        };
        assert_eq!(task.command, ["cargo", "build"]);
        assert_eq!(rows(&entries, &extras, &open)[0].detail, "Cargo");
    }

    /// SPEC-0100 v2 AK-1: problems and profiles first, members nested under their aggregator and
    /// sorted, aggregators without dependencies have no Dependencies node, missing artifacts
    /// marked; Expand All opens the groups but not the dependency trees.
    #[test]
    fn modules_nest_with_profiles_and_problems() {
        let entries = [
            entry(
                module(ModuleKind::Maven, "lab:shop-parent", ".", &["core", "app"]),
                Vec::new(),
                Vec::new(),
            ),
            entry(
                module(ModuleKind::Maven, "lab:shop-app", "app", &[]),
                Vec::new(),
                Vec::new(),
            ),
            entry(
                module(ModuleKind::Maven, "lab:shop-core", "core", &[]),
                Vec::new(),
                vec![dep("lab.missing:nowhere", vec![], true)],
            ),
        ];
        let extras = Extras {
            problems: vec![Problem {
                text: "Could not find lab.missing:nowhere:1.0.0".into(),
                place: Some(("core/pom.xml".into(), 1)),
            }],
            profiles: vec![("ci".into(), false), ("dev".into(), true)],
            ..Default::default()
        };
        let mut open = initially_expanded(&entries);
        assert_eq!(
            labels(&entries, &extras, &open),
            [
                "Could not find lab.missing:nowhere:1.0.0",
                "Profiles",
                "  ci",
                "  dev",
                "shop-parent",
                "  shop-app",
                "  shop-core",
                "    Dependencies"
            ]
        );
        let all = rows(&entries, &extras, &open);
        assert_eq!(all[0].detail, "core/pom.xml:1");
        assert_eq!(all[3].kind, RowKind::Profile("dev".into(), true));
        assert_eq!(all[6].detail, "Maven · core");
        open.insert("m2/deps".into());
        let r = rows(&entries, &extras, &open);
        assert_eq!(
            r.last().unwrap().kind,
            RowKind::Dependency(Some("1.0.0".into()), "normal".into(), false, true)
        );
        let expanded = all_group_keys(&entries, &extras);
        assert!(expanded.contains("m2") && !expanded.contains("m2/deps"));
    }

    /// SPEC-0100 v3 AK-12: the analyzer's group with its conflicts and places, and task rows with
    /// their triggers and shortcuts.
    #[test]
    fn analyzer_rows() {
        let mut omitted = dep("org.apache.commons:commons-lang3", vec![], false);
        omitted.version = Some("3.14.0".into());
        omitted.omitted = Some("omitted for conflict with 3.12.0".into());
        let mut lang = dep("org.apache.commons:commons-lang3", vec![], false);
        lang.version = Some("3.12.0".into());
        let tree = vec![
            dep("org.apache.commons:commons-text", vec![omitted], false),
            lang,
        ];
        let artifacts = idecraft_core::depanalysis::analyze("l7-core", &tree);
        let mut test = TaskGroup {
            name: "Lifecycle".into(),
            detail: String::new(),
            tasks: Vec::new(),
            groups: Vec::new(),
        };
        test.tasks.push(Task {
            label: "clean".into(),
            command: vec!["mvn".into(), "clean".into()],
            dir: "core".into(),
            run_name: "l7-core [clean]".into(),
            test: false,
        });
        let entries = vec![entry(
            module(ModuleKind::Maven, "lab:l7-core", "core", &[]),
            vec![test],
            tree.clone(),
        )];
        let mut extras = Extras {
            analysis: Some(("l7-core".into(), artifacts.clone())),
            ..Default::default()
        };
        extras
            .task_notes
            .insert(note_key("core", "clean"), "Before Build · ⌥⌘C".into());
        let mut open: HashSet<String> = analysis_expanded(&artifacts).into_iter().collect();
        open.insert("m0".into());
        open.insert("m0/g0".into());
        let all = rows(&entries, &extras, &open);
        assert_eq!(all[0].label, "Dependency Analyzer");
        assert_eq!(all[0].detail, "l7-core · 1 conflict");
        assert_eq!(all[1].label, "org.apache.commons:commons-lang3");
        assert_eq!(all[1].detail, "3.12.0 · 2 versions");
        assert_eq!(all[2].label, "3.14.0");
        assert_eq!(
            all[2].detail,
            "omitted for conflict with 3.12.0 · l7-core → commons-text 1.0.0 → commons-lang3 3.14.0"
        );
        assert_eq!(
            all[2].kind,
            RowKind::Usage("org.apache.commons:commons-text".into(), false)
        );
        assert_eq!(
            all[3].kind,
            RowKind::Usage("org.apache.commons:commons-lang3".into(), true)
        );
        // v4: all artifacts below the conflicts, each with its paths.
        assert_eq!(all[4].label, "All Dependencies");
        assert_eq!(all[4].detail, "2 artifacts");
        assert_eq!(all[5].label, "org.apache.commons:commons-lang3");
        assert_eq!(all[5].detail, "3.12.0 · 2 paths · conflict");
        assert_eq!(all[6].label, "org.apache.commons:commons-text");
        assert_eq!(all[6].detail, "1.0.0 · 1 path");
        assert!(all[6].expandable && !all[6].expanded);
        let mut opened = open.clone();
        opened.insert("analysis/all/1".into());
        let with_paths = rows(&entries, &extras, &opened);
        assert_eq!(with_paths[7].depth, 3);
        assert_eq!(with_paths[7].detail, "used · l7-core → commons-text 1.0.0");
        // Typed text narrows the list; the keys stay those of the whole list.
        let narrowed = Extras {
            analysis_filter: "text".into(),
            ..extras.clone()
        };
        let rows_now = rows(&entries, &narrowed, &opened);
        assert_eq!(rows_now[4].detail, "1 of 2 · “text”");
        assert_eq!(rows_now[5].label, "org.apache.commons:commons-text");
        assert_eq!(rows_now[5].key, "analysis/all/1");
        let clean = all.iter().find(|r| r.label == "clean").unwrap();
        assert_eq!(clean.detail, "Before Build · ⌥⌘C");
        let none = Extras {
            analysis: Some(("x".into(), Vec::new())),
            ..Default::default()
        };
        let open: HashSet<String> = ["analysis".to_owned()].into_iter().collect();
        assert_eq!(rows(&[], &none, &open)[1].label, "No version conflicts");
    }
}
