//! The dependency analyzer and the dependency diagram of the build tools window (`SPEC-0100` v3
//! behaviours 14 and 15): which artifacts a resolved tree reaches in more than one version, every
//! place it reaches them with the path from the module, and the tree as a Mermaid flowchart.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::deptree::DepNode;

/// One place where the tree reaches an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    /// The version asked for there.
    pub version: String,
    /// `used`, `replaced by 2.0.9` (Gradle chose another version) or the build tool's reason for
    /// leaving it out (`omitted for conflict with 3.12.0`, `omitted for duplicate`).
    pub status: String,
    /// Whether the version asked for there is the one on the classpath.
    pub used: bool,
    /// From the module (and for Gradle the classpath) through the libraries that bring it in,
    /// each as `artifact version`, ending with the artifact itself.
    pub path: Vec<String>,
    /// The direct dependency the path starts with (`group:artifact`), whose line of the build file
    /// the window opens.
    pub direct: String,
}

/// An artifact of the tree with every place it is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Artifact {
    /// `group:artifact`.
    pub name: String,
    /// The version on the classpath.
    pub chosen: String,
    /// The versions asked for, sorted.
    pub versions: Vec<String>,
    /// Asked for in more than one version, or a version was replaced or left out for a conflict.
    pub conflict: bool,
    /// Every place, in the order of the tree.
    pub places: Vec<Place>,
}

/// `group:artifact` → `artifact`.
fn short(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// The version asked for and the one used of a node: Gradle's `2.0.7 → 2.0.9` asks for 2.0.7 and
/// uses 2.0.9.
pub fn versions(n: &DepNode) -> (String, String) {
    let v = n.version.clone().unwrap_or_default();
    match v.split_once(" → ") {
        Some((asked, chosen)) => (asked.trim().to_owned(), chosen.trim().to_owned()),
        None => (v.clone(), v),
    }
}

fn walk(
    nodes: &[DepNode],
    path: &mut Vec<String>,
    direct: Option<&str>,
    out: &mut BTreeMap<String, Vec<Place>>,
) {
    for n in nodes {
        // A Gradle classpath is a step of the path, not an artifact.
        if n.scope == "configuration" {
            path.push(n.name.clone());
            walk(&n.children, path, None, out);
            path.pop();
            continue;
        }
        let (asked, chosen) = versions(n);
        let status = match &n.omitted {
            Some(reason) => reason.clone(),
            None if asked != chosen => format!("replaced by {chosen}"),
            None => "used".to_owned(),
        };
        let used = n.omitted.as_deref().is_none_or(|r| r.contains("duplicate")) && asked == chosen;
        let step = format!("{} {asked}", short(&n.name)).trim().to_owned();
        path.push(step);
        let first = direct.unwrap_or(&n.name).to_owned();
        out.entry(n.name.clone()).or_default().push(Place {
            version: asked,
            status,
            used,
            path: path.clone(),
            direct: first.clone(),
        });
        walk(&n.children, path, Some(&first), out);
        path.pop();
    }
}

/// Every artifact of `tree` (a module's resolved tree with the left-out entries) with its places,
/// conflicts first, then by name (behaviour 14). `module` starts each path.
pub fn analyze(module: &str, tree: &[DepNode]) -> Vec<Artifact> {
    let mut by_name: BTreeMap<String, Vec<Place>> = BTreeMap::new();
    let mut path = vec![module.to_owned()];
    walk(tree, &mut path, None, &mut by_name);
    let mut out: Vec<Artifact> = by_name
        .into_iter()
        // The project's own modules (`:lib`) are no artifacts.
        .filter(|(name, _)| !name.starts_with(':'))
        .map(|(name, places)| {
            let versions: Vec<String> = places
                .iter()
                .map(|p| p.version.clone())
                .filter(|v| !v.is_empty())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let chosen = places
                .iter()
                .find(|p| p.status == "used")
                .map(|p| p.version.clone())
                .or_else(|| {
                    places
                        .iter()
                        .find_map(|p| p.status.strip_prefix("replaced by ").map(str::to_owned))
                })
                .or_else(|| {
                    places.iter().find_map(|p| {
                        p.status
                            .strip_prefix("omitted for conflict with ")
                            .map(str::to_owned)
                    })
                })
                .unwrap_or_default();
            let conflict = versions.len() > 1
                || places
                    .iter()
                    .any(|p| p.status.starts_with("replaced by") || p.status.contains("conflict"));
            Artifact {
                name,
                chosen,
                versions,
                conflict,
                places,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.conflict
            .cmp(&a.conflict)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Only the artifacts in conflict.
pub fn conflicts(artifacts: &[Artifact]) -> Vec<&Artifact> {
    artifacts.iter().filter(|a| a.conflict).collect()
}

fn label(text: &str) -> String {
    text.replace('"', "#quot;")
}

/// The tree as a Mermaid flowchart (behaviour 15): the module as the root, one node per artifact
/// and version (per classpath for Gradle, as subgraphs), an edge per dependency, left-out edges
/// dashed with their reason, a replaced version's edge labelled with the version asked for, and
/// artifacts in conflict in class `conflict`.
pub fn mermaid(module: &str, tree: &[DepNode]) -> String {
    let in_conflict: BTreeSet<String> = analyze(module, tree)
        .into_iter()
        .filter(|a| a.conflict)
        .map(|a| a.name)
        .collect();
    struct Graph {
        ids: BTreeMap<(String, String, String), String>,
        nodes: Vec<String>,
        edges: Vec<String>,
        seen_edges: BTreeSet<(String, String)>,
        conflict_ids: BTreeSet<String>,
    }
    impl Graph {
        fn node(
            &mut self,
            scope: &str,
            name: &str,
            version: &str,
            conflict: bool,
        ) -> (String, bool) {
            let key = (scope.to_owned(), name.to_owned(), version.to_owned());
            if let Some(id) = self.ids.get(&key) {
                return (id.clone(), false);
            }
            let id = format!("n{}", self.ids.len() + 1);
            self.ids.insert(key, id.clone());
            let text = if version.is_empty() {
                short(name).to_owned()
            } else {
                format!("{}<br/>{version}", short(name))
            };
            self.nodes.push(format!("{id}[\"{}\"]", label(&text)));
            if conflict {
                self.conflict_ids.insert(id.clone());
            }
            (id, true)
        }
        fn edge(&mut self, from: &str, to: &str, text: Option<&str>, dashed: bool) {
            if !self.seen_edges.insert((from.to_owned(), to.to_owned())) {
                return;
            }
            let arrow = if dashed { "-.->" } else { "-->" };
            self.edges.push(match text {
                Some(t) => format!("{from} {arrow}|\"{}\"| {to}", label(t)),
                None => format!("{from} {arrow} {to}"),
            });
        }
    }
    fn add(
        g: &mut Graph,
        scope: &str,
        from: &str,
        nodes: &[DepNode],
        in_conflict: &BTreeSet<String>,
        lines: &mut Vec<String>,
    ) {
        for n in nodes {
            let (asked, chosen) = versions(n);
            let shown = if n.omitted.is_some() { &asked } else { &chosen };
            let before = g.nodes.len();
            let (id, _) = g.node(scope, &n.name, shown, in_conflict.contains(&n.name));
            lines.extend(g.nodes[before..].iter().cloned());
            let text = match &n.omitted {
                Some(reason) => Some(reason.clone()),
                None if asked != chosen => Some(format!("asks {asked}")),
                None => None,
            };
            let before = g.edges.len();
            g.edge(from, &id, text.as_deref(), n.omitted.is_some());
            lines.extend(g.edges[before..].iter().cloned());
            add(g, scope, &id, &n.children, in_conflict, lines);
        }
    }
    let mut g = Graph {
        ids: BTreeMap::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        seen_edges: BTreeSet::new(),
        conflict_ids: BTreeSet::new(),
    };
    let mut out = vec!["graph LR".to_owned()];
    out.push(format!("  root[\"{}\"]", label(module)));
    for n in tree {
        if n.scope == "configuration" {
            let mut lines = Vec::new();
            add(
                &mut g,
                &n.name,
                "root",
                &n.children,
                &in_conflict,
                &mut lines,
            );
            out.push(format!("  subgraph {}", n.name));
            out.extend(lines.into_iter().map(|l| format!("    {l}")));
            out.push("  end".to_owned());
        } else {
            let mut lines = Vec::new();
            add(
                &mut g,
                "",
                "root",
                std::slice::from_ref(n),
                &in_conflict,
                &mut lines,
            );
            out.extend(lines.into_iter().map(|l| format!("  {l}")));
        }
    }
    if !g.conflict_ids.is_empty() {
        let ids: Vec<String> = g.conflict_ids.iter().cloned().collect();
        out.push(format!("  class {} conflict", ids.join(",")));
        out.push("  classDef conflict stroke:#d9534f,stroke-width:2px".to_owned());
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(name: &str, version: &str, children: Vec<DepNode>) -> DepNode {
        DepNode {
            name: name.into(),
            version: (!version.is_empty()).then(|| version.to_owned()),
            scope: String::new(),
            children,
            repeated: false,
            missing: false,
            omitted: None,
        }
    }

    fn maven() -> Vec<DepNode> {
        let mut omitted = n("org.apache.commons:commons-lang3", "3.14.0", vec![]);
        omitted.omitted = Some("omitted for conflict with 3.12.0".into());
        vec![
            n("org.apache.commons:commons-text", "1.12.0", vec![omitted]),
            n("org.apache.commons:commons-lang3", "3.12.0", vec![]),
            n(
                "junit:junit",
                "4.13.2",
                vec![n("org.hamcrest:hamcrest-core", "1.3", vec![])],
            ),
        ]
    }

    /// SPEC-0100 v3 AK-12: a Maven conflict with its places, paths and the version used; a Gradle
    /// replaced version per classpath.
    #[test]
    fn conflicts_with_places_and_paths() {
        let all = analyze("l7-core", &maven());
        let lang = &all[0];
        assert_eq!(lang.name, "org.apache.commons:commons-lang3");
        assert!(lang.conflict);
        assert_eq!(lang.chosen, "3.12.0");
        assert_eq!(lang.versions, ["3.12.0", "3.14.0"]);
        assert_eq!(lang.places[0].status, "omitted for conflict with 3.12.0");
        assert!(!lang.places[0].used);
        assert_eq!(
            lang.places[0].path,
            ["l7-core", "commons-text 1.12.0", "commons-lang3 3.14.0"]
        );
        assert_eq!(lang.places[0].direct, "org.apache.commons:commons-text");
        assert_eq!(lang.places[1].status, "used");
        assert!(lang.places[1].used);
        assert_eq!(conflicts(&all).len(), 1);
        assert!(!all[1].conflict);
        let mut classpath = n(
            "compileClasspath",
            "",
            vec![
                n(
                    "org.slf4j:slf4j-simple",
                    "2.0.9",
                    vec![n("org.slf4j:slf4j-api", "2.0.9", vec![])],
                ),
                n("org.slf4j:slf4j-api", "2.0.7 → 2.0.9", vec![]),
                n(":lib", "", vec![]),
            ],
        );
        classpath.scope = "configuration".into();
        let gradle = analyze("lib", &[classpath]);
        let api = gradle
            .iter()
            .find(|a| a.name == "org.slf4j:slf4j-api")
            .unwrap();
        assert!(api.conflict);
        assert_eq!(api.chosen, "2.0.9");
        assert_eq!(api.places[1].status, "replaced by 2.0.9");
        assert_eq!(
            api.places[1].path,
            ["lib", "compileClasspath", "slf4j-api 2.0.7"]
        );
        assert_eq!(api.places[0].direct, "org.slf4j:slf4j-simple");
        assert!(gradle.iter().all(|a| a.name != ":lib"));
    }

    /// SPEC-0100 v3 AK-12, IMP-0339: the Mermaid text with the root, edges, the dashed omitted edge
    /// and the conflict class; Gradle classpaths as subgraphs.
    #[test]
    fn mermaid_diagram() {
        let text = mermaid("l7-core", &maven());
        assert!(text.starts_with("graph LR\n  root[\"l7-core\"]\n"));
        assert!(text.contains("n1[\"commons-text<br/>1.12.0\"]"));
        assert!(text.contains("root --> n1"));
        assert!(text.contains("n1 -.->|\"omitted for conflict with 3.12.0\"| n2"));
        assert!(text.contains("class n2,n3 conflict"));
        assert!(text.contains("classDef conflict"));
        let mut classpath = n(
            "testRuntimeClasspath",
            "",
            vec![n("junit:junit", "4.12 → 4.13.2", vec![])],
        );
        classpath.scope = "configuration".into();
        let g = mermaid("lib", &[classpath]);
        assert!(g.contains("  subgraph testRuntimeClasspath\n    n1[\"junit<br/>4.13.2\"]\n    root -->|\"asks 4.12\"| n1\n  end"));
    }
}
