//! What the build tools window offers per module (`SPEC-0100` v2): task groups (the Maven
//! lifecycle and plugins, Gradle's tasks by group, run from the build's root), the options every
//! build-tool run gets (offline, skip tests, Maven profiles), the roles of a module's folders in the
//! project tree, and the libraries of the project with transitive ones once the build tool
//! resolved them (`SPEC-0048` v3, `SPEC-0149` v2).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::deptree::DepNode;
use crate::project::{Module, ModuleKind, ProjectModel};

/// One runnable entry of the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    /// Label in the tree (`test`, `compiler:compile`).
    pub label: String,
    /// The command, without the options of [`BuildOptions`].
    pub command: Vec<String>,
    /// Folder it runs in, workspace-relative (`.` for the root).
    pub dir: String,
    /// Name of its run configuration: `shop-core [test]`, `gradle-kts [:lib:test]`.
    pub run_name: String,
    /// Whether it runs the module's tests (the run shows the test tree).
    pub test: bool,
}

/// A group of tasks (`Lifecycle`, `Plugins`, a plugin, a Gradle task group).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskGroup {
    /// Name.
    pub name: String,
    /// Shown after the name, dimmed (a plugin's coordinates).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// Tasks directly in it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,
    /// Groups in it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<TaskGroup>,
}

impl TaskGroup {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            detail: String::new(),
            tasks: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Every task in it and in its groups, depth first.
    pub fn all_tasks(&self) -> Vec<&Task> {
        let mut out: Vec<&Task> = self.tasks.iter().collect();
        for g in &self.groups {
            out.extend(g.all_tasks());
        }
        out
    }
}

/// The phases of Maven's clean, default and site lifecycles as the window lists them (behaviour 2).
pub const MAVEN_PHASES: [&str; 9] = [
    "clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy",
];

/// The goals of well-known Maven plugins, by prefix (STD: the plugins' documentation).
fn known_goals(prefix: &str) -> &'static [&'static str] {
    match prefix {
        "clean" => &["clean", "help"],
        "resources" => &["copy-resources", "help", "resources", "testResources"],
        "compiler" => &["compile", "help", "testCompile"],
        "surefire" => &["help", "test"],
        "failsafe" => &["help", "integration-test", "verify"],
        "jar" => &["help", "jar", "test-jar"],
        "war" => &["exploded", "help", "inplace", "war"],
        "install" => &["help", "install", "install-file"],
        "deploy" => &["deploy", "deploy-file", "help"],
        "site" => &[
            "attach-descriptor",
            "deploy",
            "effective-site",
            "help",
            "jar",
            "run",
            "site",
            "stage",
            "stage-deploy",
        ],
        "dependency" => &[
            "analyze",
            "copy-dependencies",
            "help",
            "list",
            "resolve",
            "sources",
            "tree",
        ],
        "source" => &["help", "jar", "jar-no-fork", "test-jar"],
        "javadoc" => &["help", "jar", "javadoc", "test-javadoc"],
        "exec" => &["exec", "help", "java"],
        "spring-boot" => &["build-image", "help", "repackage", "run", "start", "stop"],
        "shade" => &["help", "shade"],
        "assembly" => &["help", "single"],
        "antrun" => &["help", "run"],
        "release" => &["clean", "help", "perform", "prepare", "rollback"],
        _ => &["help"],
    }
}

/// The prefix of a Maven plugin: `maven-compiler-plugin` → `compiler`, `exec-maven-plugin` →
/// `exec`, `spring-boot-maven-plugin` → `spring-boot` (STD: Maven's prefix convention).
pub fn plugin_prefix(artifact: &str) -> String {
    if let Some(p) = artifact
        .strip_prefix("maven-")
        .and_then(|a| a.strip_suffix("-plugin"))
    {
        return p.to_owned();
    }
    if let Some(p) = artifact.strip_suffix("-maven-plugin") {
        return p.to_owned();
    }
    artifact
        .strip_suffix("-plugin")
        .unwrap_or(artifact)
        .to_owned()
}

/// The plugins bound to the lifecycle of a packaging when the POM names none (STD: Maven's
/// default lifecycle bindings), as `artifact` of `org.apache.maven.plugins`.
fn default_plugins(packaging: &str) -> &'static [&'static str] {
    match packaging {
        "pom" => &[
            "maven-clean-plugin",
            "maven-install-plugin",
            "maven-deploy-plugin",
            "maven-site-plugin",
        ],
        "war" => &[
            "maven-clean-plugin",
            "maven-resources-plugin",
            "maven-compiler-plugin",
            "maven-surefire-plugin",
            "maven-war-plugin",
            "maven-install-plugin",
            "maven-deploy-plugin",
            "maven-site-plugin",
        ],
        _ => &[
            "maven-clean-plugin",
            "maven-resources-plugin",
            "maven-compiler-plugin",
            "maven-surefire-plugin",
            "maven-jar-plugin",
            "maven-install-plugin",
            "maven-deploy-plugin",
            "maven-site-plugin",
        ],
    }
}

/// The Gradle build a module belongs to (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleBuild {
    /// Folder of the build's settings file (or the module's own), workspace-relative.
    pub root_dir: String,
    /// The project's path in the build: `:lib`, or empty for the root project.
    pub path: String,
    /// `./gradlew` when the build has a wrapper, else `gradle`.
    pub runner: String,
    /// Name of the build's folder (`gradle-kts`), for run configuration names.
    pub folder_name: String,
}

fn rel(root: &Path, dir: &Path) -> String {
    match dir.strip_prefix(root) {
        Ok(r) if r.as_os_str().is_empty() => ".".to_owned(),
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => ".".to_owned(),
    }
}

fn folder(root: &Path, dir: &str) -> PathBuf {
    if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    }
}

/// The build of Gradle module `m`: the nearest folder at or above it (inside the workspace) with a
/// settings file; its project path from the folders between (STD: Gradle's default project
/// paths). Tasks of a sub-project run there as `:lib:test`, so the build's wrapper is found.
pub fn gradle_build(root: &Path, m: &Module) -> GradleBuild {
    let dir = folder(root, &m.dir);
    let build_root = dir
        .ancestors()
        .take_while(|d| d.starts_with(root))
        .find(|d| d.join("settings.gradle.kts").is_file() || d.join("settings.gradle").is_file())
        .unwrap_or(&dir)
        .to_path_buf();
    let path = dir
        .strip_prefix(&build_root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map(|r| {
            r.components()
                .map(|c| format!(":{}", c.as_os_str().to_string_lossy()))
                .collect::<String>()
        })
        .unwrap_or_default();
    let runner = if build_root.join("gradlew").is_file() {
        "./gradlew"
    } else {
        "gradle"
    };
    GradleBuild {
        root_dir: rel(root, &build_root),
        path,
        runner: runner.to_owned(),
        folder_name: build_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// Where the task list of Gradle module `m` is kept (beside its resolved dependencies).
pub fn gradle_tasks_file(root: &Path, m: &Module) -> Option<PathBuf> {
    let deps = crate::jvmdeps::cache_file(root, m)?;
    (m.kind == ModuleKind::Gradle).then(|| deps.with_extension("tasks.txt"))
}

/// Reads the output of `gradle tasks --all` (STD): each group is a title line ending in
/// `tasks` underlined with dashes, then `name - description` lines until an empty line. Tasks of
/// other projects (`app:run`) and rules are left out. Groups are named in lower case without
/// `tasks` (`Build Setup tasks` → `build setup`), groups and tasks sorted.
pub fn parse_gradle_tasks(text: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        let title = lines[i].trim();
        let underline = lines[i + 1].trim();
        let is_title = title.ends_with(" tasks")
            && !underline.is_empty()
            && underline.chars().all(|c| c == '-');
        if !is_title {
            i += 1;
            continue;
        }
        let group = title.strip_suffix(" tasks").unwrap_or(title).to_lowercase();
        i += 2;
        while i < lines.len() && !lines[i].trim().is_empty() {
            let name = lines[i].split(" - ").next().unwrap_or("").trim();
            if !name.is_empty() && !name.contains(':') && !name.contains(' ') {
                out.entry(group.clone())
                    .or_default()
                    .insert(name.to_owned());
            }
            i += 1;
        }
    }
    out.into_iter()
        .map(|(g, t)| (g, t.into_iter().collect()))
        .collect()
}

/// The task groups of module `m` (behaviour 2): Maven `Lifecycle` with [`MAVEN_PHASES`] and
/// `Plugins` (the packaging's default plugins and those of the POM, each with its goals);
/// Gradle `Tasks` grouped as Gradle lists them once resolved (else the common tasks), run from
/// the build's root; the other build tools one `Tasks` group as before.
pub fn task_groups(root: &Path, m: &Module) -> Vec<TaskGroup> {
    match m.kind {
        ModuleKind::Maven => maven_groups(m),
        ModuleKind::Gradle => vec![gradle_group(root, m)],
        _ => {
            let mut g = TaskGroup::new("Tasks");
            g.tasks = crate::deptree::module_tasks(m)
                .into_iter()
                .map(|(label, command)| Task {
                    run_name: format!("{} [{label}]", m.name),
                    test: label == "test" || label == "pytest",
                    label,
                    command,
                    dir: m.dir.clone(),
                })
                .collect();
            if g.tasks.is_empty() {
                Vec::new()
            } else {
                vec![g]
            }
        }
    }
}

fn maven_task(m: &Module, label: &str, goal: &str) -> Task {
    Task {
        label: label.to_owned(),
        command: vec!["mvn".into(), goal.into()],
        dir: m.dir.clone(),
        run_name: format!("{} [{goal}]", m.short_name()),
        test: goal == "test" || goal == "surefire:test",
    }
}

fn maven_groups(m: &Module) -> Vec<TaskGroup> {
    let mut lifecycle = TaskGroup::new("Lifecycle");
    lifecycle.tasks = MAVEN_PHASES.iter().map(|p| maven_task(m, p, p)).collect();
    // Declared plugins replace a default one of the same artifact (and give its version).
    let mut plugins: BTreeMap<String, (String, String)> = BTreeMap::new();
    for artifact in default_plugins(m.packaging().unwrap_or("jar")) {
        plugins.insert(
            plugin_prefix(artifact),
            (
                format!("org.apache.maven.plugins:{artifact}"),
                (*artifact).to_owned(),
            ),
        );
    }
    for coords in &m.plugins {
        let artifact = coords.split(':').nth(1).unwrap_or(coords).to_owned();
        plugins.insert(plugin_prefix(&artifact), (coords.clone(), artifact));
    }
    let mut group = TaskGroup::new("Plugins");
    for (prefix, (coords, _)) in plugins {
        let mut p = TaskGroup::new(&prefix);
        p.detail = coords;
        p.tasks = known_goals(&prefix)
            .iter()
            .map(|goal| {
                let label = format!("{prefix}:{goal}");
                maven_task(m, &label, &label)
            })
            .collect();
        group.groups.push(p);
    }
    let mut out = vec![lifecycle, group];
    // The repositories of a build's top POM: Maven Central and those it names (behaviour 2).
    if !m.members.is_empty() || m.dir == "." {
        let mut repos = TaskGroup::new("Repositories");
        let mut listed = vec![(
            "central".to_owned(),
            "https://repo.maven.apache.org/maven2".to_owned(),
        )];
        for n in &m.notes {
            if let Some((id, url)) = n
                .strip_prefix("repository ")
                .and_then(|r| r.split_once(' '))
            {
                listed.retain(|(i, _)| i != id);
                listed.push((id.to_owned(), url.to_owned()));
            }
        }
        for (id, url) in listed {
            let mut r = TaskGroup::new(&id);
            r.detail = url;
            repos.groups.push(r);
        }
        out.push(repos);
    }
    out
}

/// The tasks Gradle is asked for before the build listed its own (behaviour 2).
const GRADLE_COMMON: [&str; 6] = [
    "build",
    "assemble",
    "check",
    "test",
    "clean",
    "dependencies",
];

fn gradle_group(root: &Path, m: &Module) -> TaskGroup {
    let build = gradle_build(root, m);
    let task = |name: &str| {
        let target = if build.path.is_empty() {
            name.to_owned()
        } else {
            format!("{}:{name}", build.path)
        };
        Task {
            label: name.to_owned(),
            command: vec![build.runner.clone(), target.clone()],
            dir: build.root_dir.clone(),
            run_name: format!("{} [{target}]", build.folder_name),
            test: name == "test",
        }
    };
    let mut g = TaskGroup::new("Tasks");
    let listed = gradle_tasks_file(root, m)
        .filter(|_| crate::jvmdeps::is_resolved(root, m))
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| parse_gradle_tasks(&t))
        .unwrap_or_default();
    if listed.is_empty() {
        g.tasks = GRADLE_COMMON.iter().map(|t| task(t)).collect();
    } else {
        for (name, tasks) in listed {
            let mut sub = TaskGroup::new(&name);
            sub.tasks = tasks.iter().map(|t| task(t)).collect();
            g.groups.push(sub);
        }
    }
    g
}

/// Options applied to every run of a build tool from the window and to dependency resolution
/// (behaviours 6, 7), kept in `.ferris-idea/build-tools.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOptions {
    /// Maven `-o`, Gradle `--offline`: use only what is in the local caches.
    #[serde(default)]
    pub offline: bool,
    /// Maven `-DskipTests`: the test phase compiles but does not run the tests.
    #[serde(default)]
    pub skip_tests: bool,
    /// Build files linked by hand, workspace-relative (`SPEC-0048` v4 behaviour 12).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked: Vec<String>,
    /// Module folders unlinked by hand, workspace-relative (`.` for the root).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlinked: Vec<String>,
    /// Maven profiles switched on or off by the user; the others keep their default.
    #[serde(default)]
    pub profiles: BTreeMap<String, bool>,
    /// Tasks bound to Build Project or to every run (`SPEC-0100` v3 behaviour 18).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<crate::buildtasks::Trigger>,
}

impl BuildOptions {
    /// The file the options are kept in.
    pub fn file(root: &Path) -> PathBuf {
        root.join(".ferris-idea/build-tools.toml")
    }

    /// The options of the project at `root` (defaults when there are none or they are unreadable).
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(Self::file(root))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Keeps the options.
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        let file = Self::file(root);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        crate::write_atomic(&file, text.as_bytes()).map_err(|e| e.to_string())
    }

    /// Whether profile `id` (active by default or not) is active.
    pub fn profile_active(&self, id: &str, default: bool) -> bool {
        self.profiles.get(id).copied().unwrap_or(default)
    }

    /// Whether the module in folder `dir` (workspace-relative) is unlinked: its folder or one
    /// above it was unlinked by hand (`SPEC-0048` v4 behaviour 12).
    pub fn is_unlinked(&self, dir: &str) -> bool {
        self.unlinked.iter().any(|u| {
            u == "."
                || u == dir
                || dir
                    .strip_prefix(u.as_str())
                    .is_some_and(|r| r.starts_with('/'))
        })
    }

    /// Links build file `file` (workspace-relative): it is read from now on, and an unlink of its
    /// folder or a folder above it is undone. Errors name what is wrong with the path.
    pub fn link(&mut self, root: &Path, file: &str) -> Result<(), String> {
        let file = file.trim().trim_start_matches("./").replace('\\', "/");
        let path = Path::new(&file);
        if path.is_absolute() || path.components().any(|c| c.as_os_str() == "..") {
            return Err(format!(
                "{file}: only build files inside the project can be linked"
            ));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !crate::project::BUILD_FILES.contains(&name.as_str()) {
            return Err(format!(
                "{file}: not a build file (pom.xml, build.gradle[.kts], settings.gradle[.kts], …)"
            ));
        }
        if !root.join(&file).is_file() {
            return Err(format!("{file}: no such file"));
        }
        let dir = path
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        self.unlinked.retain(|u| {
            !(u == "."
                || *u == dir
                || dir
                    .strip_prefix(u.as_str())
                    .is_some_and(|r| r.starts_with('/')))
        });
        if !self.linked.contains(&file) {
            self.linked.push(file);
            self.linked.sort();
        }
        Ok(())
    }

    /// Unlinks the module in folder `dir` and the modules inside it; build files linked inside it
    /// are no longer linked.
    pub fn unlink(&mut self, dir: &str) {
        let dir = if dir.is_empty() { "." } else { dir };
        self.linked.retain(|f| {
            let parent = Path::new(f)
                .parent()
                .map(|d| d.to_string_lossy().into_owned())
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| ".".to_owned());
            !(dir == "."
                || parent == dir
                || parent.strip_prefix(dir).is_some_and(|r| r.starts_with('/')))
        });
        if !self.unlinked.iter().any(|u| u == dir) {
            self.unlinked.push(dir.to_owned());
            self.unlinked.sort();
        }
    }

    /// `command` with the options as the build tool's flags after the program: Maven `-o`,
    /// `-DskipTests`, `-P a,!b` for the profiles that differ from their default (`profiles` of the
    /// project); Gradle `--offline`. Other programs are left alone.
    pub fn apply(&self, command: &[String], profiles: &[(String, bool)]) -> Vec<String> {
        let Some(program) = command.first() else {
            return Vec::new();
        };
        let name = Path::new(program)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut flags: Vec<String> = Vec::new();
        match name.as_str() {
            "mvn" | "mvnw" | "mvn.cmd" | "mvnw.cmd" => {
                if self.offline {
                    flags.push("-o".into());
                }
                if self.skip_tests {
                    flags.push("-DskipTests".into());
                }
                let mut changed: Vec<String> = Vec::new();
                let mut seen = BTreeSet::new();
                for (id, default) in profiles {
                    if !seen.insert(id.clone()) {
                        continue;
                    }
                    match self.profiles.get(id) {
                        Some(true) if !default => changed.push(id.clone()),
                        Some(false) if *default => changed.push(format!("!{id}")),
                        _ => {}
                    }
                }
                if !changed.is_empty() {
                    flags.push("-P".into());
                    flags.push(changed.join(","));
                }
            }
            "gradle" | "gradlew" | "gradle.bat" | "gradlew.bat" if self.offline => {
                flags.push("--offline".into());
            }
            _ => {}
        }
        let mut out = vec![program.clone()];
        out.extend(flags);
        out.extend(command[1..].iter().cloned());
        out
    }
}

/// The Maven profiles of the whole project, each once, sorted by id: id and whether it is active
/// by default (behaviour 6).
pub fn project_profiles(model: &ProjectModel) -> Vec<(String, bool)> {
    let mut out: BTreeMap<String, bool> = BTreeMap::new();
    for m in &model.modules {
        for (id, default) in &m.profiles {
            let e = out.entry(id.clone()).or_insert(false);
            *e |= *default;
        }
    }
    out.into_iter().collect()
}

/// The role of a folder in the project tree (`SPEC-0048` v3 behaviour 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderRole {
    /// The folder of a Maven or Gradle module.
    Module,
    /// Production sources.
    Sources,
    /// Test sources.
    Tests,
    /// Production resources.
    Resources,
    /// Test resources.
    TestResources,
    /// Build output, left out of searches and indexes.
    Excluded,
}

/// What the project tree shows for a folder: its role and, for a module whose name differs from
/// the folder's, that name (`maven [shop-parent]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderInfo {
    /// Role.
    pub role: FolderRole,
    /// Module name shown in brackets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_name: Option<String>,
}

/// The roles of the Maven and Gradle modules' folders (`SPEC-0048` v3 behaviour 10): the module
/// folder itself, `src/{main,test}/{java,kotlin,groovy,scala}` and `src/{main,test}/resources`
/// (or the POM's `sourceDirectory`/`testSourceDirectory`), Maven's `target` and Gradle's
/// `build` and `.gradle` as excluded. Only folders that exist are listed; paths are absolute.
pub fn folder_roles(root: &Path, model: &ProjectModel) -> BTreeMap<PathBuf, FolderInfo> {
    let mut out = BTreeMap::new();
    for m in &model.modules {
        if !matches!(m.kind, ModuleKind::Maven | ModuleKind::Gradle) {
            continue;
        }
        let dir = folder(root, &m.dir);
        let folder_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = m.short_name().to_owned();
        out.insert(
            dir.clone(),
            FolderInfo {
                role: FolderRole::Module,
                module_name: (!name.is_empty() && name != folder_name).then_some(name),
            },
        );
        let note = |key: &str| {
            m.notes
                .iter()
                .find_map(|n| n.strip_prefix(&format!("{key} ")))
                .map(|v| v.trim_start_matches("${project.basedir}/").to_owned())
        };
        let mut roles: Vec<(String, FolderRole)> = Vec::new();
        for (part, main, test) in [
            ("java", FolderRole::Sources, FolderRole::Tests),
            ("kotlin", FolderRole::Sources, FolderRole::Tests),
            ("groovy", FolderRole::Sources, FolderRole::Tests),
            ("scala", FolderRole::Sources, FolderRole::Tests),
            (
                "resources",
                FolderRole::Resources,
                FolderRole::TestResources,
            ),
        ] {
            roles.push((format!("src/main/{part}"), main));
            roles.push((format!("src/test/{part}"), test));
        }
        if let Some(s) = note("sourceDirectory") {
            roles.push((s, FolderRole::Sources));
        }
        if let Some(s) = note("testSourceDirectory") {
            roles.push((s, FolderRole::Tests));
        }
        match m.kind {
            ModuleKind::Maven => roles.push(("target".into(), FolderRole::Excluded)),
            _ => {
                roles.push(("build".into(), FolderRole::Excluded));
                roles.push((".gradle".into(), FolderRole::Excluded));
            }
        }
        for (path, role) in roles {
            let p = dir.join(&path);
            if p.is_dir() {
                out.entry(p).or_insert(FolderInfo {
                    role,
                    module_name: None,
                });
            }
        }
    }
    out
}

/// Whether `name` names a module of the project itself (a sibling Maven module, a Gradle
/// project), which is no library.
fn internal(model: &ProjectModel, name: &str) -> bool {
    name.starts_with(':')
        || model
            .modules
            .iter()
            .any(|m| m.kind == ModuleKind::Maven && m.name == name)
}

fn flatten(model: &ProjectModel, nodes: &[DepNode], out: &mut BTreeSet<(String, String)>) {
    for n in nodes {
        // Entries the build tool left out are not on the classpath (SPEC-0149 v3 behaviour 9).
        let skip = n.scope == "configuration"
            || internal(model, &n.name)
            || n.missing
            || n.omitted.is_some();
        if !skip {
            let version = n
                .version
                .as_deref()
                .map(|v| v.rsplit("→ ").next().unwrap_or(v).trim().to_owned())
                .unwrap_or_default();
            out.insert((n.name.clone(), version));
        }
        flatten(model, &n.children, out);
    }
}

/// The entries of "External Libraries" (`SPEC-0061` v4 behaviour 16, `SPEC-0048` v3 behaviour
/// 11): each module's toolchain first — a Java release with the folder of a matching JDK as
/// `< 17 > /path` when `jdk_home` finds one — then the libraries as `Tool: name:version`: for
/// Maven and Gradle modules the whole resolved tree once the build tool resolved it, without the
/// project's own modules and artifacts that were not found; else the declared dependencies.
pub fn library_entries(
    root: &Path,
    model: &ProjectModel,
    jdk_home: &dyn Fn(u32) -> Option<String>,
) -> Vec<String> {
    library_entries_with(model, jdk_home, &|m| {
        matches!(m.kind, ModuleKind::Maven | ModuleKind::Gradle)
            .then(|| crate::jvmdeps::resolved_tree(root, m))
            .flatten()
    })
}

/// [`library_entries`] with the resolved tree of a module from `tree_of`.
pub fn library_entries_with(
    model: &ProjectModel,
    jdk_home: &dyn Fn(u32) -> Option<String>,
    tree_of: &dyn Fn(&Module) -> Option<Vec<DepNode>>,
) -> Vec<String> {
    let mut toolchains = BTreeSet::new();
    let mut libraries = BTreeSet::new();
    for m in &model.modules {
        if let Some(t) = m
            .toolchain
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let java = t
                .strip_prefix("java ")
                .and_then(|v| crate::java_feature(v).map(|f| (v, f)));
            toolchains.insert(match java {
                Some((v, feature)) => match jdk_home(feature) {
                    Some(home) => format!("< {v} > {home}"),
                    None => format!("< {v} >"),
                },
                None => format!("< {t} >"),
            });
        }
        let tool = m.kind.tool_name();
        let resolved = tree_of(m);
        let mut entries = BTreeSet::new();
        match resolved {
            Some(tree) => flatten(model, &tree, &mut entries),
            None => {
                for d in &m.dependencies {
                    if internal(model, &d.name) {
                        continue;
                    }
                    let version = d.resolved.as_deref().unwrap_or(&d.requirement).trim();
                    entries.insert((d.name.clone(), version.to_owned()));
                }
            }
        }
        for (name, version) in entries {
            libraries.insert(if version.is_empty() {
                format!("{tool}: {name}")
            } else {
                format!("{tool}: {name}:{version}")
            });
        }
    }
    toolchains.into_iter().chain(libraries).collect()
}

/// The JDK home for Java feature release `feature` among the JDKs found for `root` (a full JDK
/// before a runtime), with `~` for the home folder as the project tree shows paths.
pub fn jdk_home_for(root: &Path, feature: u32) -> Option<String> {
    let jdks = crate::find_jdks(&crate::JavaSearch::from_env(root));
    let jdk = jdks
        .iter()
        .filter(|j| j.feature == Some(feature))
        .max_by_key(|j| j.has_compiler)?;
    Some(jdk.home.clone())
}

/// Takes the `missing` mark off the project's own modules in a resolved tree: a sibling module of a
/// multi-module build is built by the reactor and need not be in the local repository (behaviour 8).
pub fn unmark_modules(model: &ProjectModel, nodes: &mut [DepNode]) {
    for n in nodes {
        if n.missing && internal(model, &n.name) {
            n.missing = false;
        }
        unmark_modules(model, &mut n.children);
    }
}

/// The artifacts of a resolved tree that the build tool could not find, as `name:version`, each
/// once (behaviour 8).
pub fn missing_artifacts(nodes: &[DepNode]) -> Vec<String> {
    fn walk(nodes: &[DepNode], out: &mut BTreeSet<String>) {
        for n in nodes {
            if n.missing && n.omitted.is_none() {
                out.insert(match &n.version {
                    Some(v) => format!("{}:{v}", n.name),
                    None => n.name.clone(),
                });
            }
            walk(&n.children, out);
        }
    }
    let mut out = BTreeSet::new();
    walk(nodes, &mut out);
    out.into_iter().collect()
}

/// Comments and blank space taken out of a build file's text, so a change only to them does
/// not ask for a sync (behaviour 9): XML comments in POMs, `//` and `/* */` in Gradle scripts, `#`
/// in TOML.
pub fn normalized_build_text(name: &str, text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    if name.ends_with(".xml") {
        let mut rest = text;
        while let Some(start) = rest.find("<!--") {
            s.push_str(&rest[..start]);
            rest = rest[start..]
                .find("-->")
                .map_or("", |end| &rest[start + end + 3..]);
        }
        s.push_str(rest);
    } else if name.ends_with(".toml") {
        for line in text.lines() {
            s.push_str(line.split('#').next().unwrap_or(""));
            s.push('\n');
        }
    } else {
        // `//` and `/* */` outside string literals (`"https://…"` is no comment).
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let mut quote: Option<char> = None;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            match quote {
                Some(q) => {
                    s.push(c);
                    if c == '\\' {
                        if let Some(n) = next {
                            s.push(n);
                        }
                        i += 2;
                        continue;
                    }
                    if c == q {
                        quote = None;
                    }
                }
                None if c == '/' && next == Some('/') => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                None if c == '/' && next == Some('*') => {
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                        i += 1;
                    }
                    i += 2;
                    s.push(' ');
                    continue;
                }
                None => {
                    if c == '"' || c == '\'' {
                        quote = Some(c);
                    }
                    s.push(c);
                }
            }
            i += 1;
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The build files of the project's Maven and Gradle modules whose changes the sync badge
/// watches (behaviour 9), absolute.
pub fn sync_files(root: &Path, model: &ProjectModel) -> Vec<PathBuf> {
    let mut out = BTreeSet::new();
    for m in &model.modules {
        let dir = folder(root, &m.dir);
        let names: &[&str] = match m.kind {
            ModuleKind::Maven => &["pom.xml"],
            ModuleKind::Gradle => &[
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
                "gradle/libs.versions.toml",
            ],
            _ => &[],
        };
        for n in names {
            let f = dir.join(n);
            if f.is_file() {
                out.insert(f);
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ferris-idea-buildtools-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(root: &Path, path: &str, text: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn model(root: &Path) -> ProjectModel {
        let files = crate::Workspace::open(root).unwrap().files().unwrap();
        crate::read_project(root, &files)
    }

    const PARENT: &str = r#"<project><modelVersion>4.0.0</modelVersion>
<groupId>lab</groupId><artifactId>shop-parent</artifactId><version>1</version>
<packaging>pom</packaging><modules><module>core</module></modules>
<profiles><profile><id>dev</id><activation><activeByDefault>true</activeByDefault></activation>
<properties><env>dev</env></properties></profile><profile><id>ci</id></profile></profiles>
<repositories><repository><id>inhouse</id><url>https://repo.example.invalid/maven</url></repository>
</repositories></project>"#;
    const CORE: &str = r#"<project><modelVersion>4.0.0</modelVersion>
<parent><groupId>lab</groupId><artifactId>shop-parent</artifactId><version>1</version></parent>
<artifactId>shop-core</artifactId>
<build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version>
<configuration><release>17</release></configuration></plugin>
<plugin><groupId>org.codehaus.mojo</groupId><artifactId>exec-maven-plugin</artifactId></plugin>
</plugins></build></project>"#;

    /// SPEC-0048 v4 AK-12, SPEC-0100 v3 AK-13: a linked POM in an excluded folder becomes a
    /// module; an unlinked folder drops its module and those inside it; linking undoes it.
    #[test]
    fn linked_and_unlinked_build_files() {
        let root = tmp("links");
        write(&root, "pom.xml", PARENT);
        write(&root, "core/pom.xml", CORE);
        write(
            &root,
            "core/sub/pom.xml",
            "<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>sub</artifactId><version>1</version></project>",
        );
        write(
            &root,
            "node_modules/vendored/pom.xml",
            "<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>vendored</artifactId><version>1</version></project>",
        );
        let dirs = |root: &Path| -> Vec<String> {
            model(root).modules.iter().map(|m| m.dir.clone()).collect()
        };
        assert_eq!(dirs(&root), [".", "core", "core/sub"]);
        let mut options = BuildOptions::default();
        assert!(options.link(&root, "../elsewhere/pom.xml").is_err());
        assert!(options.link(&root, "core/README.md").is_err());
        assert!(options.link(&root, "node_modules/nothing/pom.xml").is_err());
        options
            .link(&root, "node_modules/vendored/pom.xml")
            .unwrap();
        options.unlink("core");
        options.save(&root).unwrap();
        assert_eq!(dirs(&root), [".", "node_modules/vendored"]);
        let mut options = BuildOptions::load(&root);
        assert!(options.is_unlinked("core/sub") && !options.is_unlinked("corex"));
        options.link(&root, "core/sub/pom.xml").unwrap();
        assert!(
            options.unlinked.is_empty(),
            "linking inside undoes the unlink"
        );
        options.save(&root).unwrap();
        assert_eq!(
            dirs(&root),
            [".", "core", "core/sub", "node_modules/vendored"]
        );
        let mut options = BuildOptions::load(&root);
        options.unlink("node_modules");
        assert!(options.linked.iter().all(|f| f == "core/sub/pom.xml"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v2 AK-5: Maven lifecycle with nine phases, default and declared plugins with
    /// goals, run names by artifactId; profiles of the parent.
    #[test]
    fn maven_lifecycle_plugins_and_profiles() {
        let root = tmp("maven");
        write(&root, "pom.xml", PARENT);
        write(&root, "core/pom.xml", CORE);
        let model = model(&root);
        let core = model.modules.iter().find(|m| m.dir == "core").unwrap();
        let groups = task_groups(&root, core);
        assert_eq!(groups[0].name, "Lifecycle");
        let phases: Vec<&str> = groups[0].tasks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(phases, MAVEN_PHASES);
        let test = &groups[0].tasks[3];
        assert_eq!(test.run_name, "shop-core [test]");
        assert!(test.test);
        assert_eq!(test.dir, "core");
        let plugins: Vec<(String, String)> = groups[1]
            .groups
            .iter()
            .map(|g| (g.name.clone(), g.detail.clone()))
            .collect();
        assert!(plugins.contains(&(
            "compiler".into(),
            "org.apache.maven.plugins:maven-compiler-plugin:3.13.0".into()
        )));
        assert!(plugins.contains(&("exec".into(), "org.codehaus.mojo:exec-maven-plugin".into())));
        assert!(plugins.iter().any(|(n, _)| n == "surefire"));
        let compiler = groups[1]
            .groups
            .iter()
            .find(|g| g.name == "compiler")
            .unwrap();
        assert_eq!(
            compiler
                .tasks
                .iter()
                .map(|t| t.label.as_str())
                .collect::<Vec<_>>(),
            ["compiler:compile", "compiler:help", "compiler:testCompile"]
        );
        // The aggregator has only the pom packaging's plugins, and the repositories.
        let parent = model.modules.iter().find(|m| m.dir == ".").unwrap();
        let top = task_groups(&root, parent);
        assert_eq!(top[1].groups.len(), 4);
        assert_eq!(top[2].name, "Repositories");
        assert_eq!(
            top[2]
                .groups
                .iter()
                .map(|g| (g.name.as_str(), g.detail.as_str()))
                .collect::<Vec<_>>(),
            [
                ("central", "https://repo.maven.apache.org/maven2"),
                ("inhouse", "https://repo.example.invalid/maven")
            ]
        );
        assert_eq!(groups.len(), 2, "a member module lists no repositories");
        assert_eq!(
            project_profiles(&model),
            [("ci".to_owned(), false), ("dev".to_owned(), true)]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v2 AK-6: options as flags; profiles only where they differ from the default.
    #[test]
    fn options_become_flags_and_are_kept() {
        let root = tmp("options");
        let mut o = BuildOptions::default();
        let profiles = [("ci".to_owned(), false), ("dev".to_owned(), true)];
        let cmd = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(o.apply(&cmd(&["mvn", "test"]), &profiles), ["mvn", "test"]);
        o.offline = true;
        o.skip_tests = true;
        o.profiles.insert("ci".into(), true);
        o.profiles.insert("dev".into(), false);
        assert_eq!(
            o.apply(&cmd(&["mvn", "package"]), &profiles),
            ["mvn", "-o", "-DskipTests", "-P", "ci,!dev", "package"]
        );
        assert_eq!(
            o.apply(&cmd(&["./gradlew", ":lib:test"]), &profiles),
            ["./gradlew", "--offline", ":lib:test"]
        );
        assert_eq!(o.apply(&cmd(&["npm", "test"]), &profiles), ["npm", "test"]);
        o.save(&root).unwrap();
        assert_eq!(BuildOptions::load(&root), o);
        assert!(o.profile_active("ci", false) && !o.profile_active("dev", true));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v2 AK-7: Gradle sub-project tasks run from the build's root with their path.
    #[test]
    fn gradle_tasks_run_from_the_build_root() {
        let root = tmp("gradle");
        write(
            &root,
            "settings.gradle.kts",
            "rootProject.name = \"shop-kts\"\ninclude(\"lib\")\n",
        );
        write(&root, "gradlew", "#!/bin/sh\n");
        write(
            &root,
            "lib/build.gradle.kts",
            "plugins { `java-library` }\n",
        );
        let model = model(&root);
        let lib = model.modules.iter().find(|m| m.dir == "lib").unwrap();
        let build = gradle_build(&root, lib);
        assert_eq!(build.path, ":lib");
        assert_eq!(build.root_dir, ".");
        let groups = task_groups(&root, lib);
        let test = groups[0].tasks.iter().find(|t| t.label == "test").unwrap();
        assert_eq!(test.command, ["./gradlew", ":lib:test"]);
        assert_eq!(test.dir, ".");
        let folder = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(test.run_name, format!("{folder} [:lib:test]"));
        let top = model.modules.iter().find(|m| m.dir == ".").unwrap();
        let build = task_groups(&root, top)[0].tasks[0].clone();
        assert_eq!(build.command, ["./gradlew", "build"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v2 AK-7: `gradle tasks --all` output grouped, other projects' tasks left out.
    #[test]
    fn gradle_task_list_is_grouped() {
        let text = "\n------------------------------------------------------------\n\
Tasks runnable from project ':lib'\n\
------------------------------------------------------------\n\n\
Build tasks\n-----------\nassemble - Assembles the outputs.\nbuild - Builds.\nclasses - Classes.\n\n\
Build Setup tasks\n-----------------\ninit - Initializes.\n\n\
Verification tasks\n------------------\ncheck - Runs all checks.\ntest - Runs the tests.\n\n\
Other tasks\n-----------\napp:run - Runs app.\ncompileJava - Compiles.\n\n\
Rules\n-----\nPattern: clean<TaskName>: Cleans the output files of a task.\n";
        assert_eq!(
            parse_gradle_tasks(text),
            [
                (
                    "build".to_owned(),
                    vec!["assemble".to_owned(), "build".into(), "classes".into()]
                ),
                ("build setup".to_owned(), vec!["init".to_owned()]),
                ("other".to_owned(), vec!["compileJava".to_owned()]),
                (
                    "verification".to_owned(),
                    vec!["check".to_owned(), "test".into()]
                ),
            ]
        );
    }

    /// SPEC-0048 v3 AK-10: module folders with their names, source, test, resource and excluded
    /// folders.
    #[test]
    fn folder_roles_of_maven_and_gradle_modules() {
        let root = tmp("roles");
        write(&root, "pom.xml", PARENT);
        write(&root, "core/pom.xml", CORE);
        for d in [
            "core/src/main/java",
            "core/src/main/resources",
            "core/src/test/java",
            "core/src/test/resources",
            "core/target",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let roles = folder_roles(&root, &model(&root));
        let role = |p: &str| roles.get(&root.join(p)).map(|i| i.role);
        assert_eq!(role("core"), Some(FolderRole::Module));
        assert_eq!(
            roles[&root.join("core")].module_name.as_deref(),
            Some("shop-core")
        );
        assert_eq!(
            roles[&root].module_name.as_deref(),
            Some("shop-parent"),
            "the root folder's name differs"
        );
        assert_eq!(role("core/src/main/java"), Some(FolderRole::Sources));
        assert_eq!(role("core/src/main/resources"), Some(FolderRole::Resources));
        assert_eq!(role("core/src/test/java"), Some(FolderRole::Tests));
        assert_eq!(
            role("core/src/test/resources"),
            Some(FolderRole::TestResources)
        );
        assert_eq!(role("core/target"), Some(FolderRole::Excluded));
        assert_eq!(role("core/src/main/kotlin"), None, "only existing folders");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0048 v3 AK-11: resolved libraries without the project's own modules and missing
    /// artifacts; Java toolchain with the JDK's folder.
    #[test]
    fn libraries_from_the_resolved_tree() {
        let root = tmp("libs");
        write(&root, "pom.xml", PARENT);
        write(
            &root,
            "core/pom.xml",
            &CORE.replace(
                "<build>",
                "<properties><maven.compiler.release>17</maven.compiler.release></properties>\
<dependencies><dependency><groupId>lab</groupId><artifactId>shop-parent</artifactId>\
<version>1</version></dependency><dependency><groupId>g</groupId><artifactId>a</artifactId>\
<version>1</version></dependency></dependencies><build>",
            ),
        );
        let model = model(&root);
        let home = |f: u32| (f == 17).then(|| "/jdks/17".to_owned());
        assert_eq!(
            library_entries(&root, &model, &home),
            ["< 17 > /jdks/17", "Maven: g:a:1"],
            "declared dependencies before a resolution, without the sibling module"
        );
        let tree = "lab:shop-core:jar:1\n+- g:a:jar:1:compile\n|  \\- g:b:jar:2:compile\n\
+- g:gone:jar:3:compile\n\\- lab:shop-parent:pom:1:compile\n";
        let resolved = |m: &Module| {
            (m.dir == "core").then(|| {
                let mut nodes = crate::jvmdeps::parse_maven_tree_with(tree, &|c| {
                    c[1] != "gone" && c[1] != "shop-parent"
                });
                unmark_modules(&model, &mut nodes);
                assert_eq!(
                    missing_artifacts(&nodes),
                    ["g:gone:3"],
                    "not the sibling module"
                );
                nodes
            })
        };
        assert_eq!(
            library_entries_with(&model, &home, &resolved),
            ["< 17 > /jdks/17", "Maven: g:a:1", "Maven: g:b:2"],
            "the whole tree, without the module and the missing artifact"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0100 v2 AK-8: comment-only changes keep the normalised text.
    #[test]
    fn comments_do_not_count_as_changes() {
        let pom = "<project>\n  <a>1</a>\n</project>\n";
        let commented = "<project>\n  <!-- note -->\n  <a>1</a>\n</project>";
        assert_eq!(
            normalized_build_text("pom.xml", pom),
            normalized_build_text("pom.xml", commented)
        );
        assert_ne!(
            normalized_build_text("pom.xml", pom),
            normalized_build_text("pom.xml", "<project><a>2</a></project>")
        );
        let kts = "repositories { mavenCentral() }\nval u = \"https://x\"\n";
        assert_eq!(
            normalized_build_text("build.gradle.kts", kts),
            normalized_build_text(
                "build.gradle.kts",
                &format!("{kts}// touched\n/* block */\n")
            )
        );
        assert!(normalized_build_text("build.gradle.kts", kts).contains("https://x"));
        assert_ne!(
            normalized_build_text("build.gradle.kts", kts),
            normalized_build_text("build.gradle.kts", &format!("{kts}version = \"1.1\""))
        );
    }
}
