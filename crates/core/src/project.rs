//! Project model from build files (`SPEC-0048`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Serialize;

use crate::run::{RunConfig, RunKind};

/// Build tool of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleKind {
    /// Rust / Cargo.
    Cargo,
    /// JavaScript / npm.
    Npm,
    /// Python (`pyproject.toml` or `requirements.txt`).
    Python,
    /// Java / Maven.
    Maven,
    /// Go modules.
    Go,
    /// Gradle (detected only).
    Gradle,
}

/// One dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dependency {
    /// Package name.
    pub name: String,
    /// Version requirement or origin (`path:…`, `git:…`, `workspace`).
    pub requirement: String,
    /// Scope: normal, dev, build, test, provided, peer, optional, indirect or an optional group.
    pub scope: String,
    /// Version locked in the lock file, if there is one (behaviour 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
}

/// One module (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Module {
    /// Build tool.
    pub kind: ModuleKind,
    /// Name.
    pub name: String,
    /// Version, if given.
    pub version: Option<String>,
    /// Folder of the build file, workspace-relative (`.` for the root).
    pub dir: String,
    /// Toolchain requirement (Rust, Node, Python, Java release, Go version).
    pub toolchain: Option<String>,
    /// Dependencies per section (npm: sorted by name; others in file order).
    pub dependencies: Vec<Dependency>,
    /// Named tasks (npm scripts).
    pub tasks: BTreeMap<String, String>,
    /// Member or sub-module folders, workspace-relative.
    pub members: Vec<String>,
    /// Remarks such as limitations of static reading.
    pub notes: Vec<String>,
    /// Maven profiles of the build file: id and whether it is active by default (`SPEC-0048` v3
    /// behaviour 9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<(String, bool)>,
    /// Build plugins named in the build file, `group:artifact[:version]` (`SPEC-0048` v3
    /// behaviour 9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<String>,
}

impl Module {
    pub(crate) fn new(kind: ModuleKind, name: String, dir: &str) -> Self {
        Self {
            kind,
            name,
            version: None,
            dir: dir.to_owned(),
            toolchain: None,
            dependencies: Vec::new(),
            tasks: BTreeMap::new(),
            members: Vec::new(),
            notes: Vec::new(),
            profiles: Vec::new(),
            plugins: Vec::new(),
        }
    }

    /// The Maven packaging (`jar` when the POM names none); `None` for other build tools.
    pub fn packaging(&self) -> Option<&str> {
        (self.kind == ModuleKind::Maven).then(|| {
            self.notes
                .iter()
                .find_map(|n| n.strip_prefix("packaging "))
                .unwrap_or("jar")
        })
    }

    /// The module's own name without its group: the Maven artifactId, else the name.
    pub fn short_name(&self) -> &str {
        match self.kind {
            ModuleKind::Maven => self.name.rsplit(':').next().unwrap_or(&self.name),
            _ => &self.name,
        }
    }
}

/// The model of a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ProjectModel {
    /// Modules sorted by folder and kind.
    pub modules: Vec<Module>,
    /// Problems with individual build files.
    pub problems: Vec<String>,
}

impl ModuleKind {
    /// Name of the build tool as the library entries show it (`SPEC-0061` v4 behaviour 16).
    pub fn tool_name(self) -> &'static str {
        match self {
            ModuleKind::Cargo => "Cargo",
            ModuleKind::Npm => "npm",
            ModuleKind::Python => "Python",
            ModuleKind::Maven => "Maven",
            ModuleKind::Go => "Go",
            ModuleKind::Gradle => "Gradle",
        }
    }
}

/// The entries of the Project tool window's "External Libraries" node (`SPEC-0061` v4 behaviour
/// 16): each module's toolchain as `< toolchain >` first, then the dependencies as
/// `Tool: name:version` (the locked version, else the requirement), each sorted and once.
pub fn library_entries(model: &ProjectModel) -> Vec<String> {
    let mut toolchains = std::collections::BTreeSet::new();
    let mut libraries = std::collections::BTreeSet::new();
    for m in &model.modules {
        if let Some(t) = m
            .toolchain
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            toolchains.insert(format!("< {t} >"));
        }
        for d in &m.dependencies {
            let version = d.resolved.as_deref().unwrap_or(&d.requirement).trim();
            let entry = if version.is_empty() {
                format!("{}: {}", m.kind.tool_name(), d.name)
            } else {
                format!("{}: {}:{version}", m.kind.tool_name(), d.name)
            };
            libraries.insert(entry);
        }
    }
    toolchains.into_iter().chain(libraries).collect()
}

pub(crate) const BUILD_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "pom.xml",
    "go.mod",
    "settings.gradle",
    "settings.gradle.kts",
    "build.gradle",
    "build.gradle.kts",
];

fn rel_dir(root: &Path, file: &Path) -> String {
    let dir = file.parent().unwrap_or(root);
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() { ".".into() } else { s }
}

fn dir_name(root: &Path, dir: &str) -> String {
    let p = if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_owned())
}

/// Reads the model from the given workspace files (absolute or root-relative paths), with the
/// build files linked by hand and without the folders unlinked by hand (behaviour 12).
pub fn read_project(root: &Path, files: &[PathBuf]) -> ProjectModel {
    let mut modules = Vec::new();
    let mut problems = Vec::new();
    let links = crate::buildtools::BuildOptions::load(root);
    let mut abs: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            if f.is_absolute() {
                f.clone()
            } else {
                root.join(f)
            }
        })
        .chain(
            links
                .linked
                .iter()
                .map(|f| root.join(f))
                .filter(|f| f.is_file()),
        )
        .filter(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| BUILD_FILES.contains(&n))
        })
        .filter(|f| !links.is_unlinked(&rel_dir(root, f)))
        .collect();
    // A linked file the workspace lists too is read once.
    let mut seen = std::collections::HashSet::new();
    abs.retain(|f| seen.insert(f.clone()));
    let has = |dir: &Path, name: &str| {
        abs.iter()
            .any(|f| f.parent() == Some(dir) && f.ends_with(name))
    };
    let mut gradle_dirs: Vec<PathBuf> = Vec::new();
    for file in &abs {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let dir = rel_dir(root, file);
        let parent = file.parent().unwrap_or(root);
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("{}: {e}", file.display()));
                continue;
            }
        };
        let shown = if dir == "." {
            name.to_owned()
        } else {
            format!("{dir}/{name}")
        };
        let result = match name {
            "Cargo.toml" => cargo(root, &dir, &text).map(Some),
            "package.json" => npm(root, &dir, &text).map(Some),
            "pyproject.toml" => pyproject(root, &dir, &text).map(Some),
            "requirements.txt" if !has(parent, "pyproject.toml") => {
                Ok(Some(requirements(root, &dir, &text)))
            }
            "pom.xml" => maven(root, &dir, &text).map(Some),
            "go.mod" => Ok(Some(gomod(&dir, &text))),
            n if n.contains("gradle") => {
                if gradle_dirs.iter().any(|d| d == parent) {
                    Ok(None)
                } else {
                    gradle_dirs.push(parent.to_path_buf());
                    Ok(Some(gradle(root, &dir, parent)))
                }
            }
            _ => Ok(None),
        };
        match result {
            Ok(Some(m)) => modules.push(m),
            Ok(None) => {}
            Err(e) => problems.push(format!("{shown}: {e}")),
        }
    }
    modules.sort_by(|a, b| (&a.dir, a.kind).cmp(&(&b.dir, b.kind)));
    for m in &mut modules {
        attach_lock(root, m, &mut problems);
    }
    ProjectModel { modules, problems }
}

// ---------- Lock files ----------

/// Nearest file `name` in the module folder or above it, up to the workspace root.
fn find_up(root: &Path, dir: &str, name: &str) -> Option<PathBuf> {
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

/// Lock file (workspace-relative) and package name → locked versions.
type Locked = (String, BTreeMap<String, Vec<String>>);

/// Package name → locked versions from the module's lock file (behaviour 8).
fn lock_versions(root: &Path, m: &Module) -> Result<Option<Locked>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut add = |name: &str, version: &str| {
        let list = out.entry(name.to_owned()).or_default();
        if !list.iter().any(|v| v == version) {
            list.push(version.to_owned());
        }
    };
    let toml_packages =
        |file: &Path, add: &mut dyn FnMut(&str, &str), normalize: bool| -> Result<(), String> {
            let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let doc: toml::Table = toml::from_str(&text).map_err(|e| e.to_string())?;
            for p in doc
                .get("package")
                .and_then(|p| p.as_array())
                .into_iter()
                .flatten()
            {
                if let (Some(n), Some(v)) = (
                    p.get("name").and_then(|x| x.as_str()),
                    p.get("version").and_then(|x| x.as_str()),
                ) {
                    let n = if normalize {
                        crate::python::normalize_name(n)
                    } else {
                        n.to_owned()
                    };
                    add(&n, v);
                }
            }
            Ok(())
        };
    let file = match m.kind {
        ModuleKind::Cargo => {
            let Some(f) = find_up(root, &m.dir, "Cargo.lock") else {
                return Ok(None);
            };
            toml_packages(&f, &mut add, false)?;
            f
        }
        ModuleKind::Python => {
            let Some(f) =
                find_up(root, &m.dir, "poetry.lock").or_else(|| find_up(root, &m.dir, "uv.lock"))
            else {
                return Ok(None);
            };
            toml_packages(&f, &mut add, true)?;
            f
        }
        ModuleKind::Npm => {
            let Some(f) = find_up(root, &m.dir, "package-lock.json") else {
                return Ok(None);
            };
            let text = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if let Some(packages) = v["packages"].as_object() {
                for (path, p) in packages {
                    // Top-level installs only: `node_modules/<name>` (scoped names have one slash more).
                    let Some(name) = path.strip_prefix("node_modules/") else {
                        continue;
                    };
                    if name.contains("/node_modules/") {
                        continue;
                    }
                    if let Some(ver) = p["version"].as_str() {
                        add(name, ver);
                    }
                }
            } else if let Some(deps) = v["dependencies"].as_object() {
                for (name, p) in deps {
                    if let Some(ver) = p["version"].as_str() {
                        add(name, ver);
                    }
                }
            }
            f
        }
        ModuleKind::Gradle => {
            let Some(f) = find_up(root, &m.dir, "gradle.lockfile") else {
                return Ok(None);
            };
            let text = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
            for line in text
                .lines()
                .filter(|l| !l.starts_with('#') && !l.starts_with("empty="))
            {
                let coord = line.split('=').next().unwrap_or_default();
                let parts: Vec<&str> = coord.split(':').collect();
                if let [g, a, v] = parts.as_slice() {
                    add(&format!("{g}:{a}"), v);
                }
            }
            f
        }
        ModuleKind::Maven | ModuleKind::Go => return Ok(None),
    };
    let shown = file
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| file.display().to_string());
    Ok(Some((shown, out)))
}

fn attach_lock(root: &Path, m: &mut Module, problems: &mut Vec<String>) {
    match lock_versions(root, m) {
        Ok(Some((file, versions))) => {
            let normalize = m.kind == ModuleKind::Python;
            for d in &mut m.dependencies {
                let key = if normalize {
                    crate::python::normalize_name(&d.name)
                } else {
                    d.name.clone()
                };
                if let Some(list) = versions.get(&key) {
                    d.resolved = Some(list.join(", "));
                }
            }
            m.notes.push(format!("locked versions from {file}"));
        }
        Ok(None) => {}
        Err(e) => problems.push(format!("lock file of {}: {e}", m.dir)),
    }
}

// ---------- Cargo ----------

fn cargo(root: &Path, dir: &str, text: &str) -> Result<Module, String> {
    let doc: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    // The workspace root for inherited fields: this file or the root Cargo.toml.
    let ws_root: Option<toml::Table> = if doc.contains_key("workspace") {
        Some(doc.clone())
    } else {
        std::fs::read_to_string(root.join("Cargo.toml"))
            .ok()
            .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
            .filter(|t| t.contains_key("workspace"))
    };
    let ws = ws_root
        .as_ref()
        .and_then(|t| t.get("workspace"))
        .and_then(|w| w.as_table());
    let inherited =
        |key: &str| -> Option<String> { ws?.get("package")?.get(key)?.as_str().map(str::to_owned) };
    let field = |pkg: &toml::Table, key: &str| -> Option<String> {
        match pkg.get(key)? {
            toml::Value::String(s) => Some(s.clone()),
            toml::Value::Table(t) if t.get("workspace").and_then(|v| v.as_bool()) == Some(true) => {
                inherited(key)
            }
            _ => None,
        }
    };
    let package = doc.get("package").and_then(|p| p.as_table());
    let name = package
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| dir_name(root, dir));
    let mut m = Module::new(ModuleKind::Cargo, name, dir);
    if let Some(p) = package {
        m.version = field(p, "version");
        m.toolchain = field(p, "rust-version").map(|v| format!("rust {v}"));
    }
    for (section, scope) in [
        ("dependencies", "normal"),
        ("dev-dependencies", "dev"),
        ("build-dependencies", "build"),
    ] {
        let Some(deps) = doc.get(section).and_then(|d| d.as_table()) else {
            continue;
        };
        for (dep, spec) in deps {
            let requirement = match spec {
                toml::Value::String(v) => v.clone(),
                toml::Value::Table(t) => {
                    if t.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                        ws.and_then(|w| w.get("dependencies"))
                            .and_then(|d| d.get(dep))
                            .map(|v| match v {
                                toml::Value::String(s) => s.clone(),
                                toml::Value::Table(t) => t
                                    .get("version")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_owned)
                                    .or_else(|| {
                                        t.get("path")
                                            .and_then(|p| p.as_str())
                                            .map(|p| format!("path:{p}"))
                                    })
                                    .unwrap_or_else(|| "workspace".into()),
                                _ => "workspace".into(),
                            })
                            .unwrap_or_else(|| "workspace".into())
                    } else if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                        v.to_owned()
                    } else if let Some(p) = t.get("path").and_then(|v| v.as_str()) {
                        format!("path:{p}")
                    } else if let Some(g) = t.get("git").and_then(|v| v.as_str()) {
                        format!("git:{g}")
                    } else {
                        "*".into()
                    }
                }
                _ => "*".into(),
            };
            m.dependencies.push(Dependency {
                name: dep.clone(),
                requirement,
                scope: scope.into(),
                resolved: None,
            });
        }
    }
    if let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|v| v.as_array())
    {
        let base = if dir == "." {
            root.to_path_buf()
        } else {
            root.join(dir)
        };
        for pattern in members.iter().filter_map(|v| v.as_str()) {
            m.members.extend(expand_member(&base, dir, pattern));
        }
        m.members.sort();
    }
    Ok(m)
}

/// Expands a member pattern; `*` is supported as a whole path component.
fn expand_member(base: &Path, dir: &str, pattern: &str) -> Vec<String> {
    let prefix = |p: &str| {
        if dir == "." {
            p.to_owned()
        } else {
            format!("{dir}/{p}")
        }
    };
    if let Some(parent) = pattern.strip_suffix("/*") {
        let Ok(entries) = std::fs::read_dir(base.join(parent)) else {
            return Vec::new();
        };
        let mut out: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| prefix(&format!("{parent}/{}", e.file_name().to_string_lossy())))
            .collect();
        out.sort();
        out
    } else {
        vec![prefix(pattern)]
    }
}

// ---------- npm ----------

fn npm(root: &Path, dir: &str, text: &str) -> Result<Module, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let name = v["name"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| dir_name(root, dir));
    let mut m = Module::new(ModuleKind::Npm, name, dir);
    m.version = v["version"].as_str().map(str::to_owned);
    m.toolchain = v["engines"]["node"].as_str().map(|n| format!("node {n}"));
    for (key, scope) in [
        ("dependencies", "normal"),
        ("devDependencies", "dev"),
        ("peerDependencies", "peer"),
        ("optionalDependencies", "optional"),
    ] {
        if let Some(map) = v[key].as_object() {
            // Sorted by name: JSON object order depends on a serde_json feature other crates enable.
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(dep, _)| dep.as_str());
            for (dep, req) in entries {
                m.dependencies.push(Dependency {
                    name: dep.clone(),
                    requirement: req.as_str().unwrap_or("*").to_owned(),
                    scope: scope.into(),
                    resolved: None,
                });
            }
        }
    }
    if let Some(scripts) = v["scripts"].as_object() {
        for (k, cmd) in scripts {
            m.tasks
                .insert(k.clone(), cmd.as_str().unwrap_or_default().to_owned());
        }
    }
    let ws = v["workspaces"]
        .as_array()
        .or_else(|| v["workspaces"]["packages"].as_array());
    if let Some(list) = ws {
        let base = if dir == "." {
            root.to_path_buf()
        } else {
            root.join(dir)
        };
        for p in list.iter().filter_map(|p| p.as_str()) {
            m.members.extend(expand_member(&base, dir, p));
        }
    }
    Ok(m)
}

// ---------- Python ----------

/// Splits a PEP 508 string into name and the rest (extras, version, markers).
pub fn split_pep508(spec: &str) -> (String, String) {
    let spec = spec.trim();
    let end = spec
        .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(spec.len());
    let rest = spec[end..].trim();
    (
        spec[..end].to_owned(),
        if rest.is_empty() {
            "*".into()
        } else {
            rest.to_owned()
        },
    )
}

fn pyproject(root: &Path, dir: &str, text: &str) -> Result<Module, String> {
    let doc: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let project = doc.get("project").and_then(|p| p.as_table());
    let poetry = doc
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.as_table());
    let get = |t: Option<&toml::Table>, k: &str| {
        t.and_then(|t| t.get(k))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let name = get(project, "name")
        .or_else(|| get(poetry, "name"))
        .unwrap_or_else(|| dir_name(root, dir));
    let mut m = Module::new(ModuleKind::Python, name, dir);
    m.version = get(project, "version").or_else(|| get(poetry, "version"));
    m.toolchain = get(project, "requires-python").map(|v| format!("python {v}"));
    if let Some(p) = project {
        for d in p
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(s) = d.as_str() {
                let (name, requirement) = split_pep508(s);
                m.dependencies.push(Dependency {
                    name,
                    requirement,
                    scope: "normal".into(),
                    resolved: None,
                });
            }
        }
        if let Some(groups) = p.get("optional-dependencies").and_then(|g| g.as_table()) {
            for (group, list) in groups {
                for d in list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| d.as_str())
                {
                    let (name, requirement) = split_pep508(d);
                    m.dependencies.push(Dependency {
                        name,
                        requirement,
                        scope: group.clone(),
                        resolved: None,
                    });
                }
            }
        }
    }
    if let Some(p) = poetry {
        let mut add = |table: Option<&toml::Value>, scope: &str| {
            for (dep, spec) in table.and_then(|t| t.as_table()).into_iter().flatten() {
                if dep == "python" {
                    if m.toolchain.is_none() {
                        m.toolchain = spec.as_str().map(|v| format!("python {v}"));
                    }
                    continue;
                }
                let requirement = match spec {
                    toml::Value::String(s) => s.clone(),
                    toml::Value::Table(t) => t
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("*")
                        .to_owned(),
                    _ => "*".into(),
                };
                m.dependencies.push(Dependency {
                    name: dep.clone(),
                    requirement,
                    scope: scope.into(),
                    resolved: None,
                });
            }
        };
        add(p.get("dependencies"), "normal");
        add(p.get("dev-dependencies"), "dev");
        if let Some(groups) = p.get("group").and_then(|g| g.as_table()) {
            for (group, g) in groups {
                let scope = if group == "dev" {
                    "dev".to_owned()
                } else {
                    group.clone()
                };
                add(g.get("dependencies"), &scope);
            }
        }
    }
    Ok(m)
}

fn requirements(root: &Path, dir: &str, text: &str) -> Module {
    let mut m = Module::new(ModuleKind::Python, dir_name(root, dir), dir);
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() || line.starts_with('-') {
            continue;
        }
        let (name, requirement) = split_pep508(line);
        m.dependencies.push(Dependency {
            name,
            requirement,
            scope: "normal".into(),
            resolved: None,
        });
    }
    m
}

// ---------- Maven ----------

/// The parts of a POM that the model uses.
#[derive(Default)]
struct Pom {
    values: BTreeMap<String, String>,
    modules: Vec<String>,
    deps: Vec<BTreeMap<String, String>>,
    managed: Vec<BTreeMap<String, String>>,
    /// Profiles: id and whether active by default.
    profiles: Vec<(String, bool)>,
    /// `<build><plugins>`: groupId, artifactId, version.
    plugins: Vec<BTreeMap<String, String>>,
    /// `<repositories>`: id and url.
    repositories: Vec<(String, String)>,
}

fn parse_pom(text: &str) -> Result<Pom, String> {
    let mut reader = Reader::from_str(text);
    let mut path: Vec<String> = Vec::new();
    let mut pom = Pom::default();
    let mut dep: BTreeMap<String, String> = BTreeMap::new();
    const DEP: &str = "project/dependencies/dependency";
    const MANAGED: &str = "project/dependencyManagement/dependencies/dependency";
    const PROFILE: &str = "project/profiles/profile";
    const PLUGIN: &str = "project/build/plugins/plugin";
    let mut profile: (String, bool) = Default::default();
    let mut plugin: BTreeMap<String, String> = BTreeMap::new();
    const REPOSITORY: &str = "project/repositories/repository";
    let mut repository: (String, String) = Default::default();
    loop {
        let event = reader.read_event().map_err(|e| {
            // Line and column of the error, so the Build window can open the place
            // (SPEC-0100 v2 behaviour 12).
            let at = (reader.error_position() as usize).min(text.len());
            let before = &text[..at];
            let line = before.matches('\n').count() + 1;
            let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
            format!("line {line}, column {column}: {e}")
        })?;
        match event {
            Event::Start(t) => {
                path.push(String::from_utf8_lossy(t.local_name().as_ref()).into_owned());
            }
            Event::End(_) => {
                let closed = path.join("/");
                if closed == DEP {
                    pom.deps.push(std::mem::take(&mut dep));
                } else if closed == MANAGED {
                    pom.managed.push(std::mem::take(&mut dep));
                } else if closed == PROFILE {
                    let p = std::mem::take(&mut profile);
                    if !p.0.is_empty() {
                        pom.profiles.push(p);
                    }
                } else if closed == PLUGIN {
                    pom.plugins.push(std::mem::take(&mut plugin));
                } else if closed == REPOSITORY {
                    pom.repositories.push(std::mem::take(&mut repository));
                }
                path.pop();
            }
            Event::Text(t) => {
                let text = t.decode().map_err(|e| e.to_string())?.trim().to_owned();
                if text.is_empty() {
                    continue;
                }
                let key = path.join("/");
                if key == "project/modules/module" {
                    pom.modules.push(text);
                } else if key == format!("{PROFILE}/id") {
                    profile.0 = text;
                } else if key == format!("{PROFILE}/activation/activeByDefault") {
                    profile.1 = text == "true";
                } else if key == format!("{REPOSITORY}/id") {
                    repository.0 = text;
                } else if key == format!("{REPOSITORY}/url") {
                    repository.1 = text;
                } else if key.starts_with(&format!("{PROFILE}/")) {
                    // A profile's own settings apply only when it is active.
                } else if let Some(field) = key.strip_prefix(&format!("{PLUGIN}/")) {
                    if !field.contains('/') {
                        plugin.insert(field.to_owned(), text);
                    }
                } else if let Some(field) = key
                    .strip_prefix(&format!("{DEP}/"))
                    .or_else(|| key.strip_prefix(&format!("{MANAGED}/")))
                {
                    dep.insert(field.to_owned(), text);
                } else {
                    pom.values.insert(key, text);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(pom)
}

/// Replaces `${name}` from `props` (repeatedly, for nested properties); unknown names stay.
fn interpolate(text: &str, props: &BTreeMap<String, String>) -> String {
    let mut out = text.to_owned();
    for _ in 0..8 {
        let mut changed = false;
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(start) = rest.find("${") {
            result.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                result.push_str(&rest[start..]);
                rest = "";
                break;
            };
            let name = &rest[start + 2..start + end];
            match props.get(name) {
                Some(v) => {
                    result.push_str(v);
                    changed = true;
                }
                None => result.push_str(&rest[start..start + end + 1]),
            }
            rest = &rest[start + end + 1..];
        }
        result.push_str(rest);
        out = result;
        if !changed {
            break;
        }
    }
    out
}

/// Properties of a POM for interpolation: `<properties>` and the `project.*` coordinates.
fn pom_properties(pom: &Pom) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    for (k, v) in &pom.values {
        if let Some(name) = k.strip_prefix("project/properties/") {
            props.insert(name.to_owned(), v.clone());
        }
    }
    let get = |k: &str| pom.values.get(k).cloned();
    if let Some(v) = get("project/version").or_else(|| get("project/parent/version")) {
        props.insert("project.version".into(), v);
    }
    if let Some(g) = get("project/groupId").or_else(|| get("project/parent/groupId")) {
        props.insert("project.groupId".into(), g);
    }
    if let Some(a) = get("project/artifactId") {
        props.insert("project.artifactId".into(), a);
    }
    if let Some(v) = get("project/parent/version") {
        props.insert("project.parent.version".into(), v);
    }
    props
}

/// The parent POM on disk (`relativePath`, default `../pom.xml`) when its coordinates match, with
/// its folder.
fn local_parent(folder: &Path, pom: &Pom) -> Option<(Pom, PathBuf)> {
    let group = pom.values.get("project/parent/groupId")?;
    let artifact = pom.values.get("project/parent/artifactId")?;
    let relative = match pom.values.get("project/parent/relativePath") {
        Some(r) => r.clone(),
        // `<relativePath/>` (empty) means: look only in repositories.
        None if pom.values.contains_key("project/parent/version") => "../pom.xml".into(),
        None => return None,
    };
    let mut file = folder.join(relative);
    if file.is_dir() {
        file = file.join("pom.xml");
    }
    let parent = parse_pom(&std::fs::read_to_string(&file).ok()?).ok()?;
    let pg = parent
        .values
        .get("project/groupId")
        .or_else(|| parent.values.get("project/parent/groupId"));
    let matches = pg == Some(group) && parent.values.get("project/artifactId") == Some(artifact);
    matches.then(|| {
        (
            parent,
            file.parent().map(Path::to_path_buf).unwrap_or_default(),
        )
    })
}

fn maven(root: &Path, dir: &str, text: &str) -> Result<Module, String> {
    let pom = parse_pom(text)?;
    let folder = if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    // Parent chain on disk (at most 5 levels): properties and managed versions are inherited.
    let mut chain: Vec<Pom> = Vec::new();
    let mut next = local_parent(&folder, &pom);
    while let Some((parent, parent_folder)) = next.take() {
        next = local_parent(&parent_folder, &parent);
        chain.push(parent);
        if chain.len() >= 5 {
            break;
        }
    }
    let mut props = BTreeMap::new();
    for p in chain.iter().rev() {
        props.extend(pom_properties(p));
    }
    props.extend(pom_properties(&pom));
    let mut managed: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    for p in chain.iter().rev().chain(std::iter::once(&pom)) {
        for d in &p.managed {
            let key = format!(
                "{}:{}",
                d.get("groupId")
                    .map(|g| interpolate(g, &props))
                    .unwrap_or_default(),
                d.get("artifactId").cloned().unwrap_or_default()
            );
            if let Some(v) = d.get("version") {
                managed.insert(key, (interpolate(v, &props), d.get("scope").cloned()));
            }
        }
    }
    let get = |k: &str| pom.values.get(k).map(|v| interpolate(v, &props));
    let group = get("project/groupId").or_else(|| get("project/parent/groupId"));
    let artifact = get("project/artifactId").unwrap_or_else(|| dir_name(root, dir));
    let name = match &group {
        Some(g) => format!("{g}:{artifact}"),
        None => artifact,
    };
    let mut m = Module::new(ModuleKind::Maven, name, dir);
    m.version = get("project/version").or_else(|| get("project/parent/version"));
    m.toolchain = [
        "maven.compiler.release",
        "maven.compiler.source",
        "java.version",
    ]
    .iter()
    .find_map(|k| props.get(*k))
    .map(|v| format!("java {}", interpolate(v, &props)));
    if let Some(p) = get("project/packaging") {
        m.notes.push(format!("packaging {p}"));
    }
    let remote_parent = pom.values.contains_key("project/parent/artifactId") && chain.is_empty();
    let mut unresolved = false;
    for d in &pom.deps {
        let name = format!(
            "{}:{}",
            d.get("groupId")
                .map(|g| interpolate(g, &props))
                .unwrap_or_default(),
            d.get("artifactId").cloned().unwrap_or_default()
        );
        let from_management = managed.get(&name);
        let requirement = match (d.get("version"), from_management) {
            (Some(v), _) => interpolate(v, &props),
            (None, Some((v, _))) => v.clone(),
            (None, None) => {
                unresolved = true;
                "managed".into()
            }
        };
        let scope = d
            .get("scope")
            .cloned()
            .or_else(|| from_management.and_then(|(_, s)| s.clone()))
            .unwrap_or_else(|| "normal".into());
        m.dependencies.push(Dependency {
            name,
            requirement,
            scope,
            resolved: None,
        });
    }
    if unresolved && remote_parent {
        m.notes.push(format!(
            "versions managed by the parent {}:{} {} are not available locally",
            get("project/parent/groupId").unwrap_or_default(),
            get("project/parent/artifactId").unwrap_or_default(),
            get("project/parent/version").unwrap_or_default()
        ));
    }
    m.members = pom
        .modules
        .iter()
        .map(|p| {
            if dir == "." {
                p.clone()
            } else {
                format!("{dir}/{p}")
            }
        })
        .collect();
    m.profiles = pom.profiles.clone();
    m.plugins = pom
        .plugins
        .iter()
        .filter_map(|p| {
            let artifact = p.get("artifactId")?;
            let group = p
                .get("groupId")
                .map(|g| interpolate(g, &props))
                .unwrap_or_else(|| "org.apache.maven.plugins".into());
            Some(match p.get("version") {
                Some(v) => format!("{group}:{artifact}:{}", interpolate(v, &props)),
                None => format!("{group}:{artifact}"),
            })
        })
        .collect();
    for (id, url) in &pom.repositories {
        m.notes
            .push(format!("repository {id} {}", interpolate(url, &props)));
    }
    for (key, field) in [
        ("project/build/sourceDirectory", "sourceDirectory"),
        ("project/build/testSourceDirectory", "testSourceDirectory"),
    ] {
        if let Some(v) = get(key) {
            m.notes.push(format!("{field} {v}"));
        }
    }
    Ok(m)
}

// ---------- Go ----------

fn gomod(dir: &str, text: &str) -> Module {
    let mut m = Module::new(ModuleKind::Go, String::new(), dir);
    let mut in_block = false;
    for line in text.lines() {
        let (code, comment) = match line.split_once("//") {
            Some((c, k)) => (c.trim(), k.trim()),
            None => (line.trim(), ""),
        };
        if let Some(rest) = code.strip_prefix("module ") {
            m.name = rest.trim().trim_matches('"').to_owned();
        } else if let Some(rest) = code.strip_prefix("go ") {
            m.toolchain = Some(format!("go {}", rest.trim()));
        } else if code == "require (" {
            in_block = true;
        } else if in_block && code == ")" {
            in_block = false;
        } else if in_block || code.starts_with("require ") {
            let spec = code.strip_prefix("require ").unwrap_or(code).trim();
            let mut parts = spec.split_whitespace();
            if let (Some(name), Some(version)) = (parts.next(), parts.next()) {
                m.dependencies.push(Dependency {
                    name: name.to_owned(),
                    requirement: version.to_owned(),
                    scope: if comment == "indirect" {
                        "indirect"
                    } else {
                        "normal"
                    }
                    .into(),
                    resolved: None,
                });
            }
        }
    }
    m
}

// ---------- Gradle ----------

/// The string literal right after `key` (e.g. `rootProject.name = "x"`).
fn literal_after(text: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix(key) else {
            continue;
        };
        let mut s = rest;
        while let Some(start) = s.find(['"', '\'']) {
            let quote = s.as_bytes()[start] as char;
            let Some(len) = s[start + 1..].find(quote) else {
                break;
            };
            out.push(s[start + 1..start + 1 + len].to_owned());
            s = &s[start + 1 + len + 1..];
        }
    }
    out
}

/// Version catalog `gradle/libs.versions.toml`: alias (normalised to dots) → (group:name, version).
fn version_catalog(folder: &Path) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    let mut dir = Some(folder);
    // The catalog lives next to the settings file, possibly above a sub-project.
    let text = loop {
        let Some(d) = dir else { return out };
        if let Ok(t) = std::fs::read_to_string(d.join("gradle/libs.versions.toml")) {
            break t;
        }
        dir = d.parent();
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&text) else {
        return out;
    };
    let versions = doc.get("versions").and_then(|v| v.as_table());
    let version_of = |v: &toml::Value| -> String {
        match v {
            toml::Value::String(s) => s.clone(),
            toml::Value::Table(t) => {
                if let Some(r) = t.get("ref").and_then(|r| r.as_str()) {
                    versions
                        .and_then(|vs| vs.get(r))
                        .map(|x| match x {
                            toml::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_else(|| format!("ref:{r}"))
                } else {
                    t.get("strictly")
                        .or_else(|| t.get("require"))
                        .or_else(|| t.get("prefer"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("*")
                        .to_owned()
                }
            }
            other => other.to_string(),
        }
    };
    if let Some(libs) = doc.get("libraries").and_then(|l| l.as_table()) {
        for (alias, spec) in libs {
            let key = alias.replace(['-', '_'], ".");
            let entry = match spec {
                toml::Value::String(s) => {
                    let parts: Vec<&str> = s.split(':').collect();
                    match parts.as_slice() {
                        [g, a, v] => Some((format!("{g}:{a}"), (*v).to_owned())),
                        [g, a] => Some((format!("{g}:{a}"), "*".to_owned())),
                        _ => None,
                    }
                }
                toml::Value::Table(t) => {
                    let module = t
                        .get("module")
                        .and_then(|m| m.as_str())
                        .map(str::to_owned)
                        .or_else(|| {
                            Some(format!(
                                "{}:{}",
                                t.get("group")?.as_str()?,
                                t.get("name")?.as_str()?
                            ))
                        });
                    let version = t
                        .get("version")
                        .map(version_of)
                        .unwrap_or_else(|| "*".into());
                    module.map(|m| (m, version))
                }
                _ => None,
            };
            if let Some(e) = entry {
                out.insert(key, e);
            }
        }
    }
    out
}

/// Gradle configuration → dependency scope.
fn gradle_scope(configuration: &str) -> Option<&'static str> {
    Some(match configuration {
        "implementation" | "api" | "compile" => "normal",
        "compileOnly" | "compileOnlyApi" => "provided",
        "runtimeOnly" | "runtime" => "runtime",
        "testImplementation" | "testCompileOnly" | "testRuntimeOnly" | "testCompile" => "test",
        "annotationProcessor" | "kapt" | "ksp" => "build",
        "developmentOnly" => "dev",
        _ => return None,
    })
}

/// Literal dependency declarations of a Gradle build script (behaviour 4): `conf("g:a:v")`,
/// `conf 'g:a:v'`, `conf(libs.x.y)`, also wrapped in `platform(...)`.
fn gradle_dependencies(
    script: &str,
    catalog: &BTreeMap<String, (String, String)>,
) -> (Vec<Dependency>, usize) {
    let mut deps = Vec::new();
    let mut dynamic = 0;
    for line in script.lines() {
        let line = line.trim();
        let Some(conf_end) = line.find(|c: char| !(c.is_alphanumeric() || c == '_')) else {
            continue;
        };
        let Some(scope) = gradle_scope(&line[..conf_end]) else {
            continue;
        };
        let mut arg = line[conf_end..].trim().trim_start_matches('(').trim();
        for wrapper in ["platform(", "enforcedPlatform(", "kotlin(", "testFixtures("] {
            if let Some(rest) = arg.strip_prefix(wrapper) {
                arg = rest.trim();
            }
        }
        if let Some(q) = arg.chars().next().filter(|c| *c == '"' || *c == '\'') {
            let Some(end) = arg[1..].find(q) else {
                continue;
            };
            let coord = &arg[1..1 + end];
            if coord.contains('$') {
                dynamic += 1;
            }
            let parts: Vec<&str> = coord.split(':').collect();
            if parts.len() >= 2 {
                deps.push(Dependency {
                    name: format!("{}:{}", parts[0], parts[1]),
                    requirement: parts
                        .get(2)
                        .map(|v| (*v).to_owned())
                        .unwrap_or_else(|| "managed".into()),
                    scope: scope.into(),
                    resolved: None,
                });
            }
        } else if let Some(alias) = arg.strip_prefix("libs.") {
            let alias: String = alias
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
                .collect();
            let key = alias.trim_end_matches(".get").replace(['-', '_'], ".");
            match catalog.get(&key) {
                Some((module, version)) => deps.push(Dependency {
                    name: module.clone(),
                    requirement: version.clone(),
                    scope: scope.into(),
                    resolved: None,
                }),
                None => dynamic += 1,
            }
        } else if arg.starts_with("project(") {
            let path = literal_after(arg, "project(")
                .into_iter()
                .next()
                .unwrap_or_default();
            deps.push(Dependency {
                name: path,
                requirement: "project".into(),
                scope: scope.into(),
                resolved: None,
            });
        } else {
            dynamic += 1;
        }
    }
    (deps, dynamic)
}

fn gradle(root: &Path, dir: &str, folder: &Path) -> Module {
    let settings = ["settings.gradle.kts", "settings.gradle"]
        .iter()
        .find_map(|n| std::fs::read_to_string(folder.join(n)).ok())
        .unwrap_or_default();
    let name = literal_after(&settings, "rootProject.name")
        .into_iter()
        .next()
        .unwrap_or_else(|| dir_name(root, dir));
    let mut m = Module::new(ModuleKind::Gradle, name, dir);
    m.members = literal_after(&settings, "include")
        .into_iter()
        .map(|p| {
            let p = p.trim_start_matches(':').replace(':', "/");
            if dir == "." { p } else { format!("{dir}/{p}") }
        })
        .collect();
    let script = ["build.gradle.kts", "build.gradle"]
        .iter()
        .find_map(|n| std::fs::read_to_string(folder.join(n)).ok())
        .unwrap_or_default();
    let catalog = version_catalog(folder);
    let (deps, dynamic) = gradle_dependencies(&script, &catalog);
    m.dependencies = deps;
    if let Some(i) = script.find("JavaLanguageVersion.of(") {
        let v: String = script[i + "JavaLanguageVersion.of(".len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !v.is_empty() {
            m.toolchain = Some(format!("java {v}"));
        }
    }
    // The wrapper of the build (here or in a parent folder) runs its tasks, as in the reference.
    let wrapper = folder
        .ancestors()
        .take_while(|f| f.starts_with(root))
        .any(|f| f.join("gradlew").is_file());
    let runner = if wrapper { "./gradlew" } else { "gradle" };
    for task in ["build", "test"] {
        m.tasks.insert(task.to_owned(), format!("{runner} {task}"));
    }
    let mut note = "dependencies are read from literal declarations; computed ones and tasks need Gradle itself".to_owned();
    if dynamic > 0 {
        note.push_str(&format!(" ({dynamic} declaration(s) not read statically)"));
    }
    m.notes.push(note);
    m
}

// ---------- Suggested run configurations ----------

/// `path` inside module folder `dir` (workspace-relative).
fn in_dir(dir: &str, path: &str) -> String {
    if dir == "." {
        path.to_owned()
    } else {
        format!("{dir}/{path}")
    }
}

fn config(
    name: String,
    command: &[&str],
    dir: &str,
    kind: RunKind,
    report: Option<&str>,
) -> RunConfig {
    RunConfig {
        name,
        command: command.iter().map(|s| s.to_string()).collect(),
        cwd: (dir != ".").then(|| dir.to_owned()),
        env: BTreeMap::new(),
        kind,
        report: report.map(str::to_owned),
        report_file: None,
        before: Vec::new(),
        timeout: None,
        url: None,
        folder: None,
        allow_multiple: false,
        before_launch: Vec::new(),
        verbatim: false,
        http: None,
    }
}

/// Name of the suggested configuration that starts a Spring Boot application.
const SPRING_BOOT: &str = "Spring Boot";
/// Name of the suggested configuration that starts Quarkus in dev mode.
const QUARKUS_DEV: &str = "Quarkus dev";
/// Name of the suggested configuration that starts a Micronaut application.
const MICRONAUT_RUN: &str = "Micronaut run";
/// How long a suggested application server may run before the timeout ends it: a day
/// (`SPEC-0048` v8 behaviour 6a; the default of `SPEC-0047` is ten minutes).
pub const SERVER_TIMEOUT_SECS: u64 = 86_400;

/// The framework whose application a Maven or Gradle module starts, from its dependencies and
/// Maven plugins (`SPEC-0048` v8 behaviour 6a): the configuration's name and the Maven goal.
/// Quarkus before Micronaut before Spring Boot, as a Quarkus application may use Spring's
/// compatibility libraries.
fn framework_server(m: &Module) -> Option<(&'static str, &'static str)> {
    let group = |g: &str| {
        m.dependencies
            .iter()
            .any(|d| d.name.split(':').next() == Some(g))
            || m.plugins.iter().any(|p| p.split(':').next() == Some(g))
    };
    let plugin = |artifact: &str| {
        m.plugins
            .iter()
            .any(|p| p.split(':').nth(1) == Some(artifact))
    };
    if group("io.quarkus") || group("io.quarkus.platform") || plugin("quarkus-maven-plugin") {
        Some((QUARKUS_DEV, "quarkus:dev"))
    } else if group("io.micronaut") || plugin("micronaut-maven-plugin") {
        Some((MICRONAUT_RUN, "mn:run"))
    } else if group("org.springframework.boot") || plugin("spring-boot-maven-plugin") {
        Some((SPRING_BOOT, "spring-boot:run"))
    } else {
        None
    }
}

/// A suggested configuration that starts an application server: it runs until stopped.
fn server_config(name: String, command: &[&str], dir: &str) -> RunConfig {
    let mut c = config(name, command, dir, RunKind::Run, None);
    c.timeout = Some(SERVER_TIMEOUT_SECS);
    c
}

/// Run configurations that fit the modules (behaviour 6).
pub fn suggest_run_configs(model: &ProjectModel) -> Vec<RunConfig> {
    let mut out = Vec::new();
    let label = |m: &Module, what: &str| {
        if m.dir == "." {
            what.to_owned()
        } else {
            format!("{what} ({})", m.dir)
        }
    };
    for m in &model.modules {
        match m.kind {
            // Cargo members are covered by the workspace root's commands.
            ModuleKind::Cargo
                if m.dir == "." || model.modules.iter().all(|o| !o.members.contains(&m.dir)) =>
            {
                out.push(config(
                    label(m, "cargo build"),
                    &["cargo", "build"],
                    &m.dir,
                    RunKind::Run,
                    None,
                ));
                out.push(config(
                    label(m, "cargo test"),
                    &["cargo", "test"],
                    &m.dir,
                    RunKind::Test,
                    Some("libtest"),
                ));
            }
            ModuleKind::Npm => {
                for script in m.tasks.keys() {
                    out.push(config(
                        label(m, &format!("npm run {script}")),
                        &["npm", "run", script],
                        &m.dir,
                        RunKind::Run,
                        None,
                    ));
                }
                // Test runners with a JSON report for the test tree (SPEC-0045 v3).
                let uses = |name: &str| m.dependencies.iter().any(|d| d.name == name);
                if uses("vitest") {
                    let mut test = config(
                        label(m, "vitest"),
                        &[
                            "npx",
                            "vitest",
                            "run",
                            "--reporter=json",
                            "--outputFile=.ferris-idea/vitest.json",
                        ],
                        &m.dir,
                        RunKind::Test,
                        Some("jest"),
                    );
                    test.report_file = Some(in_dir(&m.dir, ".ferris-idea/vitest.json"));
                    out.push(test);
                }
                if uses("jest") {
                    let mut test = config(
                        label(m, "jest"),
                        &[
                            "npx",
                            "jest",
                            "--json",
                            "--outputFile=.ferris-idea/jest.json",
                        ],
                        &m.dir,
                        RunKind::Test,
                        Some("jest"),
                    );
                    test.report_file = Some(in_dir(&m.dir, ".ferris-idea/jest.json"));
                    out.push(test);
                }
            }
            ModuleKind::Go => out.push(config(
                label(m, "go test"),
                &["go", "test", "-json", "./..."],
                &m.dir,
                RunKind::Test,
                Some("gotest"),
            )),
            // Java builds (SPEC-0097): compile, and test with the JUnit reports of the run.
            ModuleKind::Maven if model.modules.iter().all(|o| !o.members.contains(&m.dir)) => {
                out.push(config(
                    label(m, "mvn compile"),
                    &["mvn", "-q", "compile"],
                    &m.dir,
                    RunKind::Run,
                    None,
                ));
                let mut test = config(
                    label(m, "mvn test"),
                    &["mvn", "test"],
                    &m.dir,
                    RunKind::Test,
                    Some("junit"),
                );
                test.report_file = Some(in_dir(&m.dir, "target/surefire-reports"));
                out.push(test);
                if let Some((what, goal)) = framework_server(m) {
                    out.push(server_config(label(m, what), &["mvn", goal], &m.dir));
                }
            }
            ModuleKind::Gradle if model.modules.iter().all(|o| !o.members.contains(&m.dir)) => {
                let runner = m
                    .tasks
                    .get("build")
                    .and_then(|c| c.split_whitespace().next())
                    .unwrap_or("gradle")
                    .to_owned();
                out.push(config(
                    label(m, "gradle build"),
                    &[&runner, "build"],
                    &m.dir,
                    RunKind::Run,
                    None,
                ));
                let mut test = config(
                    label(m, "gradle test"),
                    &[&runner, "test"],
                    &m.dir,
                    RunKind::Test,
                    Some("junit"),
                );
                test.report_file = Some(in_dir(&m.dir, "build/test-results/test"));
                out.push(test);
                if let Some((what, _)) = framework_server(m) {
                    let task = match what {
                        SPRING_BOOT => "bootRun",
                        QUARKUS_DEV => "quarkusDev",
                        _ => "run",
                    };
                    out.push(server_config(label(m, what), &[&runner, task], &m.dir));
                }
            }
            ModuleKind::Python
                if m.dependencies
                    .iter()
                    .any(|d| d.name.eq_ignore_ascii_case("pytest")) =>
            {
                let mut test = config(
                    label(m, "pytest"),
                    &[
                        "python3",
                        "-m",
                        "pytest",
                        "--junitxml=.ferris-idea/pytest.xml",
                    ],
                    &m.dir,
                    RunKind::Test,
                    Some("junit"),
                );
                test.report_file = Some(in_dir(&m.dir, ".ferris-idea/pytest.xml"));
                out.push(test);
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(
        kind: ModuleKind,
        toolchain: Option<&str>,
        deps: &[(&str, &str, Option<&str>)],
    ) -> Module {
        let mut m = Module::new(kind, "m".into(), ".");
        m.toolchain = toolchain.map(str::to_owned);
        m.dependencies = deps
            .iter()
            .map(|(name, req, resolved)| Dependency {
                name: (*name).into(),
                requirement: (*req).into(),
                scope: "normal".into(),
                resolved: resolved.map(str::to_owned),
            })
            .collect();
        m
    }

    /// SPEC-0061 v4 behaviour 16 (AK-8): toolchains first, then the libraries, sorted and once.
    #[test]
    fn library_entries() {
        let model = ProjectModel {
            modules: vec![
                module(
                    ModuleKind::Cargo,
                    Some("1.85"),
                    &[("serde", "1", Some("1.0.219")), ("anyhow", "1.0", None)],
                ),
                module(
                    ModuleKind::Maven,
                    Some("17"),
                    &[("junit:junit", "4.13.2", None), ("x:y", "", None)],
                ),
                module(
                    ModuleKind::Cargo,
                    Some("1.85"),
                    &[("serde", "1", Some("1.0.219"))],
                ),
            ],
            problems: Vec::new(),
        };
        assert_eq!(
            super::library_entries(&model),
            [
                "< 1.85 >",
                "< 17 >",
                "Cargo: anyhow:1.0",
                "Cargo: serde:1.0.219",
                "Maven: junit:junit:4.13.2",
                "Maven: x:y",
            ]
        );
        assert!(super::library_entries(&ProjectModel::default()).is_empty());
    }
}
