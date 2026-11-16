//! Dependency trees and tasks of a module for the build tools window (`SPEC-0100`): transitive
//! dependencies read from `Cargo.lock` and `package-lock.json`, direct ones for other build tools.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::project::{Module, ModuleKind};

/// Deepest level of a tree; deeper dependencies are cut.
pub const MAX_DEPTH: usize = 12;

/// One dependency with what it depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DepNode {
    /// Package name.
    pub name: String,
    /// Locked or required version, if known.
    pub version: Option<String>,
    /// Scope of a direct dependency (normal, dev, build, test, …); empty for indirect ones.
    pub scope: String,
    /// What it depends on.
    pub children: Vec<DepNode>,
    /// Already shown with its dependencies further up; listed without them here.
    pub repeated: bool,
    /// The build tool could not find the artifact (`SPEC-0149` v2 behaviour 6).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    /// Why the build tool left this entry out of the tree, e.g. `omitted for conflict with 3.12.0`
    /// (Maven `-Dverbose`, `SPEC-0149` v3 behaviour 9); such an entry has no children.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<String>,
}

impl DepNode {
    fn leaf(name: &str, version: Option<String>, scope: &str) -> Self {
        Self {
            name: name.to_owned(),
            version,
            scope: scope.to_owned(),
            children: Vec::new(),
            repeated: false,
            missing: false,
            omitted: None,
        }
    }
}

/// `nodes` without the entries the build tool left out (`SPEC-0149` v3 behaviour 9), as the build
/// tools window and External Libraries show a tree.
pub fn without_omitted(nodes: &[DepNode]) -> Vec<DepNode> {
    nodes
        .iter()
        .filter(|n| n.omitted.is_none())
        .map(|n| DepNode {
            children: without_omitted(&n.children),
            ..n.clone()
        })
        .collect()
}

/// The dependency tree of module `m` (behaviour 3): transitive from the lock file for Cargo and npm,
/// from the build tool's last resolution for Maven and Gradle (`SPEC-0149`), else the direct
/// dependencies with their locked versions. Each package is expanded once; later
/// occurrences are marked `repeated`.
pub fn dependency_tree(root: &Path, m: &Module) -> Result<Vec<DepNode>, String> {
    let lock = match m.kind {
        ModuleKind::Cargo => find_up(root, &m.dir, "Cargo.lock").map(|f| cargo_tree(&f, m)),
        ModuleKind::Npm => {
            find_up(root, &m.dir, "package-lock.json").map(|f| npm_tree(root, &f, m))
        }
        // Resolved by the build tool and kept until the build file changes (SPEC-0149).
        ModuleKind::Maven | ModuleKind::Gradle => crate::jvmdeps::resolved_tree(root, m).map(Ok),
        _ => None,
    };
    match lock {
        Some(tree) => tree,
        None => Ok(m
            .dependencies
            .iter()
            .map(|d| {
                DepNode::leaf(
                    &d.name,
                    d.resolved
                        .clone()
                        .or_else(|| (!d.requirement.is_empty()).then(|| d.requirement.clone())),
                    &d.scope,
                )
            })
            .collect()),
    }
}

fn find_up(root: &Path, dir: &str, name: &str) -> Option<std::path::PathBuf> {
    let mut folder = if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    loop {
        let f = folder.join(name);
        if f.is_file() {
            return Some(f);
        }
        if folder == root || !folder.pop() {
            return None;
        }
    }
}

fn scope_of(m: &Module, name: &str) -> String {
    m.dependencies
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.scope.clone())
        .unwrap_or_else(|| "normal".to_owned())
}

/// `Cargo.lock`: packages with `dependencies` entries `name`, `name version` or
/// `name version (source)`.
fn cargo_tree(file: &Path, m: &Module) -> Result<Vec<DepNode>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let doc: toml::Table = toml::from_str(&text).map_err(|e| e.to_string())?;
    let mut packages: BTreeMap<String, Vec<(String, Vec<String>)>> = BTreeMap::new();
    for p in doc
        .get("package")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(version)) = (
            p.get("name").and_then(|x| x.as_str()),
            p.get("version").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        let deps = p
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|d| d.as_str().map(str::to_owned))
            .collect();
        packages
            .entry(name.to_owned())
            .or_default()
            .push((version.to_owned(), deps));
    }
    let find = |spec: &str| -> Option<(String, String)> {
        let mut parts = spec.split_whitespace();
        let name = parts.next()?;
        let version = parts.next();
        let list = packages.get(name)?;
        let (v, _) = match version {
            Some(v) => list.iter().find(|(pv, _)| pv == v)?,
            None => list.first()?,
        };
        Some((name.to_owned(), v.clone()))
    };
    let deps_of = |name: &str, version: &str| -> Vec<String> {
        packages
            .get(name)
            .and_then(|l| l.iter().find(|(v, _)| v == version))
            .map(|(_, d)| d.clone())
            .unwrap_or_default()
    };
    // The module's own package; its dependency list covers normal, dev and build dependencies.
    let own = m
        .version
        .as_deref()
        .and_then(|v| find(&format!("{} {v}", m.name)))
        .or_else(|| find(&m.name));
    let Some((own_name, own_version)) = own else {
        return Err(format!("{} is not in {}", m.name, file.display()));
    };
    let mut seen = HashSet::new();
    fn expand(
        spec: &str,
        depth: usize,
        find: &dyn Fn(&str) -> Option<(String, String)>,
        deps_of: &dyn Fn(&str, &str) -> Vec<String>,
        seen: &mut HashSet<(String, String)>,
    ) -> Option<DepNode> {
        let (name, version) = find(spec)?;
        let mut node = DepNode::leaf(&name, Some(version.clone()), "");
        if !seen.insert((name.clone(), version.clone())) {
            node.repeated = !deps_of(&name, &version).is_empty();
            return Some(node);
        }
        if depth < MAX_DEPTH {
            node.children = deps_of(&name, &version)
                .iter()
                .filter_map(|d| expand(d, depth + 1, find, deps_of, seen))
                .collect();
        }
        Some(node)
    }
    seen.insert((own_name.clone(), own_version.clone()));
    Ok(deps_of(&own_name, &own_version)
        .iter()
        .filter_map(|d| {
            let mut node = expand(d, 1, &find, &deps_of, &mut seen)?;
            node.scope = scope_of(m, &node.name);
            Some(node)
        })
        .collect())
}

/// `package-lock.json` v2/v3: `packages` keyed by install path; a dependency of the package at `P`
/// resolves to `P/node_modules/<name>`, else the same in the folders above, as Node does.
fn npm_tree(root: &Path, file: &Path, m: &Module) -> Result<Vec<DepNode>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let Some(packages) = v["packages"].as_object() else {
        return Err(format!(
            "{}: lock file version 1 has no package tree",
            file.display()
        ));
    };
    // The module's entry: its folder relative to the lock file's folder ("" for the same folder).
    let lock_dir = file.parent().unwrap_or(root);
    let module_dir = if m.dir == "." {
        root.to_path_buf()
    } else {
        root.join(&m.dir)
    };
    let own = module_dir
        .strip_prefix(lock_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let lookup = |from: &str, name: &str| -> Option<String> {
        let mut base = from.to_owned();
        loop {
            let candidate = if base.is_empty() {
                format!("node_modules/{name}")
            } else {
                format!("{base}/node_modules/{name}")
            };
            if packages.contains_key(&candidate) {
                return Some(candidate);
            }
            if base.is_empty() {
                return None;
            }
            base = match base.rfind("/node_modules/") {
                Some(i) => base[..i].to_owned(),
                None => String::new(),
            };
        }
    };
    let names = |entry: &serde_json::Value, dev: bool| -> Vec<String> {
        let mut keys = vec!["dependencies", "optionalDependencies"];
        if dev {
            keys.push("devDependencies");
        }
        let mut out: Vec<String> = keys
            .iter()
            .filter_map(|k| entry[*k].as_object())
            .flat_map(|o| o.keys().cloned())
            .collect();
        out.sort();
        out.dedup();
        out
    };
    fn expand(
        path: &str,
        name: &str,
        depth: usize,
        packages: &serde_json::Map<String, serde_json::Value>,
        lookup: &dyn Fn(&str, &str) -> Option<String>,
        names: &dyn Fn(&serde_json::Value, bool) -> Vec<String>,
        seen: &mut HashSet<String>,
    ) -> DepNode {
        let entry = &packages[path];
        let mut node = DepNode::leaf(name, entry["version"].as_str().map(str::to_owned), "");
        let deps = names(entry, false);
        if !seen.insert(path.to_owned()) {
            node.repeated = !deps.is_empty();
            return node;
        }
        if depth < MAX_DEPTH {
            node.children = deps
                .iter()
                .filter_map(|d| {
                    let at = lookup(path, d)?;
                    Some(expand(&at, d, depth + 1, packages, lookup, names, seen))
                })
                .collect();
        }
        node
    }
    let entry = packages
        .get(&own)
        .ok_or_else(|| format!("{} is not in {}", m.dir, file.display()))?;
    let mut seen = HashSet::new();
    Ok(names(entry, true)
        .iter()
        .map(|d| match lookup(&own, d) {
            Some(at) => {
                let mut node = expand(&at, d, 1, packages, &lookup, &names, &mut seen);
                node.scope = scope_of(m, d);
                node
            }
            None => DepNode::leaf(d, Some("not installed".into()), &scope_of(m, d)),
        })
        .collect())
}

/// The tasks the build tools window offers for module `m` (behaviour 2): label and command, run in
/// the module's folder.
pub fn module_tasks(m: &Module) -> Vec<(String, Vec<String>)> {
    let task = |label: &str, command: &[&str]| {
        (
            label.to_owned(),
            command.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    };
    match m.kind {
        ModuleKind::Cargo => ["build", "check", "test", "clippy", "doc", "run"]
            .iter()
            .map(|t| task(t, &["cargo", t]))
            .collect(),
        ModuleKind::Npm => std::iter::once(task("install", &["npm", "install"]))
            .chain(m.tasks.keys().map(|s| task(s, &["npm", "run", s.as_str()])))
            .collect(),
        ModuleKind::Maven => [
            "clean", "validate", "compile", "test", "package", "verify", "install",
        ]
        .iter()
        .map(|t| task(t, &["mvn", t]))
        .collect(),
        ModuleKind::Gradle => {
            let runner = m
                .tasks
                .get("build")
                .and_then(|c| c.split_whitespace().next())
                .unwrap_or("gradle")
                .to_owned();
            [
                "build",
                "assemble",
                "check",
                "test",
                "clean",
                "dependencies",
            ]
            .iter()
            .map(|t| task(t, &[&runner, t]))
            .collect()
        }
        ModuleKind::Go => vec![
            task("build", &["go", "build", "./..."]),
            task("test", &["go", "test", "./..."]),
            task("vet", &["go", "vet", "./..."]),
            task("mod tidy", &["go", "mod", "tidy"]),
        ],
        ModuleKind::Python => {
            let mut out = Vec::new();
            if m.dependencies
                .iter()
                .any(|d| d.name.eq_ignore_ascii_case("pytest"))
            {
                out.push(task("pytest", &["python3", "-m", "pytest"]));
            }
            out
        }
    }
}
