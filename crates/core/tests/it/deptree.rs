//! SPEC-0100 AK-1: dependency trees from lock files and the tasks of each build tool.

use std::path::{Path, PathBuf};

use idecraft_core::{DepNode, Workspace, dependency_tree, module_tasks, read_project};

fn fixture(files: &[(&str, &str)]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("idecraft-deptree-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (p, text) in files {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    root
}

fn lines(nodes: &[DepNode], depth: usize, out: &mut Vec<String>) {
    for n in nodes {
        out.push(format!(
            "{}{} {}{}{}",
            "  ".repeat(depth),
            n.name,
            n.version.as_deref().unwrap_or("?"),
            if n.scope.is_empty() {
                String::new()
            } else {
                format!(" [{}]", n.scope)
            },
            if n.repeated { " (repeated)" } else { "" }
        ));
        lines(&n.children, depth + 1, out);
    }
}

fn tree(root: &Path, kind: &str) -> Vec<String> {
    let files = Workspace::open(root).unwrap().files().unwrap();
    let model = read_project(root, &files);
    let m = model
        .modules
        .iter()
        .find(|m| format!("{:?}", m.kind).to_lowercase() == kind)
        .unwrap();
    let mut out = Vec::new();
    lines(&dependency_tree(root, m).unwrap(), 0, &mut out);
    out
}

#[test]
fn cargo_lock_tree_with_repeats() {
    let root = fixture(&[
        (
            "Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\nlog = \"0.4\"\n\n[dev-dependencies]\nonce = \"1\"\n",
        ),
        (
            "Cargo.lock",
            "version = 3\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"log\", \"once\", \"serde\"]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\ndependencies = [\"serde_derive\", \"log 0.4.21\"]\n\n[[package]]\nname = \"serde_derive\"\nversion = \"1.0.200\"\n\n[[package]]\nname = \"log\"\nversion = \"0.4.21\"\ndependencies = [\"cfg-if\"]\n\n[[package]]\nname = \"cfg-if\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"once\"\nversion = \"1.19.0\"\n",
        ),
    ]);
    assert_eq!(
        tree(&root, "cargo"),
        [
            "log 0.4.21 [normal]",
            "  cfg-if 1.0.0",
            "once 1.19.0 [dev]",
            "serde 1.0.200 [normal]",
            "  serde_derive 1.0.200",
            "  log 0.4.21 (repeated)",
        ]
    );
}

#[test]
fn npm_lock_tree_with_nested_installs() {
    let root = fixture(&[
        (
            "package.json",
            r#"{"name": "web", "dependencies": {"a": "^1"}, "devDependencies": {"b": "^2"}, "scripts": {"test": "vitest"}}"#,
        ),
        (
            "package-lock.json",
            r#"{"name": "web", "lockfileVersion": 3, "packages": {
  "": {"name": "web", "dependencies": {"a": "^1"}, "devDependencies": {"b": "^2"}},
  "node_modules/a": {"version": "1.2.0", "dependencies": {"c": "^1"}},
  "node_modules/b": {"version": "2.0.0", "dev": true, "dependencies": {"c": "^2"}},
  "node_modules/b/node_modules/c": {"version": "2.1.0"},
  "node_modules/c": {"version": "1.5.0"}
}}"#,
        ),
    ]);
    assert_eq!(
        tree(&root, "npm"),
        [
            "a 1.2.0 [normal]",
            "  c 1.5.0",
            "b 2.0.0 [dev]",
            "  c 2.1.0",
        ]
    );
}

#[test]
fn direct_dependencies_and_tasks_elsewhere() {
    let root = fixture(&[(
        "go.mod",
        "module demo\n\ngo 1.22\n\nrequire github.com/x/y v1.2.3\n",
    )]);
    assert_eq!(tree(&root, "go"), ["github.com/x/y v1.2.3 [normal]"]);
    let files = Workspace::open(&root).unwrap().files().unwrap();
    let model = read_project(&root, &files);
    let tasks: Vec<String> = module_tasks(&model.modules[0])
        .into_iter()
        .map(|(l, c)| format!("{l}: {}", c.join(" ")))
        .collect();
    assert_eq!(
        tasks,
        [
            "build: go build ./...",
            "test: go test ./...",
            "vet: go vet ./...",
            "mod tidy: go mod tidy"
        ]
    );
}

#[test]
fn this_workspace_resolves_its_own_lock_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap();
    let files = Workspace::open(&root).unwrap().files().unwrap();
    let model = read_project(&root, &files);
    let core = model
        .modules
        .iter()
        .find(|m| m.name == "idecraft-core")
        .unwrap();
    let deps = dependency_tree(&root, core).unwrap();
    let serde = deps.iter().find(|d| d.name == "serde").unwrap();
    assert!(serde.version.is_some());
    assert!(deps.iter().any(|d| !d.children.is_empty()));
}
