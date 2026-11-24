//! Transitive dependencies of Maven and Gradle modules (`SPEC-0149`): the build tool resolves
//! them (`mvn dependency:tree`, `gradle dependencies`), the output is kept in
//! `.ferris-idea/deps/` until the build file changes, and read into a [`DepNode`] tree.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::deptree::DepNode;
use crate::project::{Module, ModuleKind};

/// The Gradle configurations shown, as the build tool's own window lists the classpaths.
pub const GRADLE_CONFIGURATIONS: [&str; 4] = [
    "compileClasspath",
    "runtimeClasspath",
    "testCompileClasspath",
    "testRuntimeClasspath",
];

/// Longest a resolution may take before it is stopped.
const RESOLVE_LIMIT: Duration = Duration::from_secs(300);

fn node(name: &str, version: Option<String>, scope: &str, repeated: bool) -> DepNode {
    DepNode {
        name: name.to_owned(),
        version,
        scope: scope.to_owned(),
        children: Vec::new(),
        repeated,
        missing: false,
        omitted: None,
    }
}

/// Puts `n` at `depth` (0 = top) under the last node of each level; a level without a node
/// above it takes `n` itself.
fn place(list: &mut Vec<DepNode>, depth: usize, n: DepNode) {
    match list.last_mut() {
        Some(last) if depth > 0 => place(&mut last.children, depth - 1, n),
        _ => list.push(n),
    }
}

/// Reads the text output of `mvn dependency:tree` (behaviour 2): the first line is the module,
/// each level is indented by three characters (`+- `, `\- `, `|  `, `   `); an entry is
/// `group:artifact:type[:classifier]:version:scope`. Top-level entries keep their scope.
pub fn parse_maven_tree(text: &str) -> Vec<DepNode> {
    parse_maven_tree_with(text, &|_| true)
}

/// The local Maven repository: `<localRepository>` of `~/.m2/settings.xml`, else
/// `~/.m2/repository` (STD: Maven settings).
pub fn maven_local_repository() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let settings = std::fs::read_to_string(home.join(".m2/settings.xml")).unwrap_or_default();
    let configured = settings
        .split_once("<localRepository>")
        .and_then(|(_, r)| r.split_once("</localRepository>"))
        .map(|(v, _)| v.trim().replace("${user.home}", &home.to_string_lossy()))
        .filter(|v| !v.is_empty());
    Some(configured.map_or_else(|| home.join(".m2/repository"), PathBuf::from))
}

/// The file of artifact `group:artifact:type[:classifier]:version` in repository `repo` (STD:
/// Maven repository layout); `pom` artifacts are their POM, others their jar.
pub fn maven_artifact_file(repo: &Path, coords: &[&str]) -> Option<PathBuf> {
    let (group, artifact, kind, classifier, version) = match coords {
        [g, a, t, v] => (*g, *a, *t, None, *v),
        [g, a, t, c, v] => (*g, *a, *t, Some(*c), *v),
        _ => return None,
    };
    let ext = match kind {
        "pom" => "pom",
        "war" => "war",
        "test-jar" => "jar",
        _ => "jar",
    };
    let classifier = match (kind, classifier) {
        ("test-jar", None) => Some("tests"),
        (_, c) => c,
    };
    let file = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    Some(
        repo.join(group.replace('.', "/"))
            .join(artifact)
            .join(version)
            .join(file),
    )
}

/// [`parse_maven_tree`], marking entries `missing` whose coordinates
/// (`group:artifact:type[:classifier]:version`) `exists` rejects (`SPEC-0149` v2 behaviour 6).
pub fn parse_maven_tree_with(text: &str, exists: &dyn Fn(&[&str]) -> bool) -> Vec<DepNode> {
    let mut top = Vec::new();
    for line in text.lines().skip(1) {
        let Some(at) = line.find("+- ").or_else(|| line.find("\\- ")) else {
            continue;
        };
        let depth = at / 3;
        let entry = line[at + 3..].trim();
        // `-Dverbose`: an entry Maven left out is in parentheses with its reason after ` - `
        // (`SPEC-0149` v3 behaviour 9).
        let (entry, omitted) = match entry
            .strip_prefix('(')
            .and_then(|e| e.strip_suffix(')'))
            .and_then(|e| e.split_once(" - "))
        {
            Some((coords, reason)) => (coords, Some(omission(reason))),
            None => (entry.split(" (").next().unwrap_or(entry), None),
        };
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() < 5 {
            continue;
        }
        let name = format!("{}:{}", parts[0], parts[1]);
        let version = parts[parts.len() - 2].to_owned();
        let scope = if depth == 0 {
            parts[parts.len() - 1]
        } else {
            ""
        };
        let optional = line.contains("(optional)");
        let mut n = node(&name, Some(version), scope, false);
        // A left-out entry was never meant to be downloaded.
        n.missing = omitted.is_none() && !exists(&parts[..parts.len() - 1]);
        n.omitted = omitted;
        if optional && depth == 0 {
            n.scope = format!("{scope}, optional");
        }
        place(&mut top, depth, n);
    }
    top
}

/// The reason of a left-out Maven entry without a managed version before it: `version managed
/// from 1.1; omitted for duplicate` → `omitted for duplicate`.
fn omission(reason: &str) -> String {
    reason
        .rsplit("; ")
        .next()
        .unwrap_or(reason)
        .trim()
        .to_owned()
}

/// Reads the output of `gradle dependencies` (behaviour 3) for the first project in it: one node
/// per configuration of [`GRADLE_CONFIGURATIONS`] that has dependencies, each level indented by
/// five characters (`+--- `, `\--- `, `|    `). `a -> b` shows the version Gradle chose as
/// `a → b`, `(*)` marks a repeated subtree, constraints `(c)` and unresolvable entries `(n)` are
/// left out, `project :x` names a module of the build.
pub fn parse_gradle_tree(text: &str) -> Vec<DepNode> {
    let mut out = Vec::new();
    let mut current: Option<DepNode> = None;
    let mut projects = 0;
    for line in text.lines() {
        if line.starts_with("Root project ") || line.starts_with("Project ") {
            projects += 1;
            if projects > 1 {
                break;
            }
            continue;
        }
        let header = line.split(" - ").next().unwrap_or("");
        if !line.starts_with([' ', '+', '\\', '|']) && !header.is_empty() && !header.contains(' ') {
            if let Some(c) = current.take()
                && !c.children.is_empty()
            {
                out.push(c);
            }
            current = GRADLE_CONFIGURATIONS
                .contains(&header)
                .then(|| node(header, None, "configuration", false));
            continue;
        }
        let Some(config) = current.as_mut() else {
            continue;
        };
        let Some(at) = line.find("+--- ").or_else(|| line.find("\\--- ")) else {
            continue;
        };
        let depth = at / 5;
        let mut entry = line[at + 5..].trim();
        if entry.ends_with(" (c)") || entry.ends_with(" (n)") {
            continue;
        }
        let repeated = entry.ends_with(" (*)");
        entry = entry.trim_end_matches(" (*)");
        // An artifact Gradle could not find (SPEC-0149 v2 behaviour 6).
        let failed = entry.ends_with(" FAILED");
        entry = entry.trim_end_matches(" FAILED");
        let (name, version) = if let Some(project) = entry.strip_prefix("project ") {
            (project.to_owned(), None)
        } else {
            let (coords, chosen) = match entry.split_once(" -> ") {
                Some((c, v)) => (c, Some(v)),
                None => (entry, None),
            };
            let parts: Vec<&str> = coords.split(':').collect();
            let name = parts.iter().take(2).copied().collect::<Vec<_>>().join(":");
            let requested = parts.get(2).map(|v| v.to_string());
            let version = match (requested, chosen) {
                (Some(r), Some(c)) => Some(format!("{r} → {c}")),
                (None, Some(c)) => Some(c.to_owned()),
                (r, None) => r,
            };
            (name, version)
        };
        let mut n = node(&name, version, "", repeated);
        n.missing = failed;
        place(&mut config.children, depth, n);
    }
    if let Some(c) = current
        && !c.children.is_empty()
    {
        out.push(c);
    }
    out
}

fn module_dir(root: &Path, m: &Module) -> PathBuf {
    if m.dir == "." {
        root.to_path_buf()
    } else {
        root.join(&m.dir)
    }
}

/// The build files whose change makes a resolution stale.
fn build_files(root: &Path, m: &Module) -> Vec<PathBuf> {
    let dir = module_dir(root, m);
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
    let mut files: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
    // Settings and the version catalog of the whole build live at the root.
    if m.kind == ModuleKind::Gradle && dir != root {
        files.extend(
            [
                "settings.gradle",
                "settings.gradle.kts",
                "gradle/libs.versions.toml",
            ]
            .iter()
            .map(|n| root.join(n)),
        );
    }
    files.into_iter().filter(|f| f.is_file()).collect()
}

/// Where the resolution of module `m` is kept; `None` for build tools that are not resolved.
pub fn cache_file(root: &Path, m: &Module) -> Option<PathBuf> {
    let tool = match m.kind {
        ModuleKind::Maven => "maven",
        ModuleKind::Gradle => "gradle",
        _ => return None,
    };
    let key = if m.dir == "." {
        "root".to_owned()
    } else {
        m.dir.replace(['/', '\\'], "_")
    };
    Some(
        root.join(".ferris-idea/deps")
            .join(format!("{key}.{tool}.txt")),
    )
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Whether module `m` has a resolution newer than its build files (behaviour 4).
pub fn is_resolved(root: &Path, m: &Module) -> bool {
    let Some(cache) = cache_file(root, m) else {
        return false;
    };
    let Some(at) = modified(&cache) else {
        return false;
    };
    build_files(root, m)
        .iter()
        .all(|f| modified(f).is_none_or(|t| t <= at))
}

/// The resolved tree of module `m`, if it is resolved and current.
pub fn resolved_tree(root: &Path, m: &Module) -> Option<Vec<DepNode>> {
    if !is_resolved(root, m) {
        return None;
    }
    let text = std::fs::read_to_string(cache_file(root, m)?).ok()?;
    Some(match m.kind {
        ModuleKind::Maven => {
            // Maven lists artifacts it could not download as if they were there; the local
            // repository tells (SPEC-0149 v2 behaviour 6).
            let repo = maven_local_repository();
            parse_maven_tree_with(&text, &|coords| {
                let Some(repo) = repo.as_deref() else {
                    return true;
                };
                maven_artifact_file(repo, coords).is_none_or(|f| f.is_file())
            })
        }
        _ => parse_gradle_tree(&text),
    })
}

/// The wrapper script `name` in `dir` or a folder above it up to `root`.
pub(crate) fn wrapper(root: &Path, dir: &Path, name: &str) -> Option<PathBuf> {
    let mut d = dir.to_path_buf();
    loop {
        let f = d.join(name);
        if f.is_file() {
            return Some(f);
        }
        if d == root || !d.pop() {
            return None;
        }
    }
}

/// The command that resolves module `m` and the folder it runs in (behaviour 1): the project's
/// wrapper (`mvnw`, `gradlew`) when there is one, else `mvn` or `gradle`. Maven writes the tree to
/// `out` itself; Gradle prints it.
pub fn resolve_command(root: &Path, m: &Module, out: &Path) -> Option<(PathBuf, Vec<String>)> {
    let dir = module_dir(root, m);
    let tool = |wrap: &str, plain: &str| {
        wrapper(root, &dir, wrap)
            .map(|w| w.display().to_string())
            .unwrap_or_else(|| plain.to_owned())
    };
    let command = match m.kind {
        ModuleKind::Maven => vec![
            tool("mvnw", "mvn"),
            "-B".into(),
            "-q".into(),
            "-N".into(),
            "dependency:tree".into(),
            // Entries left out for a conflict or as duplicates are kept (SPEC-0149 v3 behaviour 9).
            "-Dverbose".into(),
            format!("-DoutputFile={}", out.display()),
        ],
        ModuleKind::Gradle => vec![
            tool("gradlew", "gradle"),
            "-q".into(),
            "--console=plain".into(),
            "dependencies".into(),
        ],
        _ => return None,
    };
    Some((dir, command))
}

/// Resolves module `m` with its build tool and keeps the result (behaviours 1, 4). Errors carry
/// the tool's last lines.
pub fn resolve(root: &Path, m: &Module) -> Result<(), String> {
    // Build-tool sync runs the build logic: never in safe mode (SPEC-0278 behaviour 5).
    crate::trust::require_trusted(root)?;
    let out = cache_file(root, m).ok_or("only Maven and Gradle modules are resolved")?;
    // The tree is written beside the kept one and renamed when complete.
    let partial = out.with_extension("part");
    let (dir, command) = resolve_command(root, m, &partial).ok_or("no build tool")?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&partial);
    // Offline mode and the chosen Maven profiles apply to the resolution too (SPEC-0100 v2
    // behaviour 6).
    let files = crate::Workspace::open(root)
        .and_then(|w| w.files())
        .unwrap_or_default();
    let profiles = crate::buildtools::project_profiles(&crate::read_project(root, &files));
    let command = crate::buildtools::BuildOptions::load(root).apply(&command, &profiles);
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    if m.kind == ModuleKind::Maven {
        cmd.stdout(Stdio::piped());
    } else {
        let file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
        cmd.stdout(file);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", command[0]))?;
    let stderr = child.stderr.take();
    let stdout = child.stdout.take();
    let read = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut r) = r {
                let _ = r.read_to_string(&mut s);
            }
            s
        })
    };
    let err_text = read(stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
    let out_text = read(stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if started.elapsed() > RESOLVE_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&partial);
            return Err(format!(
                "{} took longer than {} s",
                command[0],
                RESOLVE_LIMIT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let messages = err_text.join().unwrap_or_default() + &out_text.join().unwrap_or_default();
    if !status.success() || !partial.is_file() {
        let _ = std::fs::remove_file(&partial);
        let tail: Vec<&str> = messages
            .lines()
            .filter(|l| !l.trim().is_empty())
            .rev()
            .take(3)
            .collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!(
            "{} failed: {}",
            Path::new(&command[0])
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            if tail.is_empty() {
                format!("{status}")
            } else {
                tail.join(" / ")
            }
        ));
    }
    std::fs::rename(&partial, &out).map_err(|e| e.to_string())?;
    if m.kind == ModuleKind::Gradle {
        list_gradle_tasks(root, m, &dir, &command[0]);
    }
    Ok(())
}

/// Asks Gradle for the module's tasks (`gradle tasks --all`) and keeps the list beside the
/// resolution (`SPEC-0100` v2 behaviour 3); a failure keeps the previous list.
fn list_gradle_tasks(root: &Path, m: &Module, dir: &Path, runner: &str) {
    let Some(file) = crate::buildtools::gradle_tasks_file(root, m) else {
        return;
    };
    let mut args = vec!["-q", "--console=plain", "tasks", "--all"];
    if crate::buildtools::BuildOptions::load(root).offline {
        args.insert(0, "--offline");
    }
    let Ok(output) = Command::new(runner)
        .args(&args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    if output.status.success() {
        let _ = crate::write_atomic(&file, &output.stdout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(nodes: &[DepNode], depth: usize, out: &mut Vec<String>) {
        for n in nodes {
            out.push(format!(
                "{}{} {}{}{}",
                "  ".repeat(depth),
                n.name,
                n.version.as_deref().unwrap_or("-"),
                if n.scope.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", n.scope)
                },
                if n.repeated { " *" } else { "" }
            ));
            texts(&n.children, depth + 1, out);
        }
    }

    fn lines(nodes: &[DepNode]) -> Vec<String> {
        let mut out = Vec::new();
        texts(nodes, 0, &mut out);
        out
    }

    /// SPEC-0149 v2 AK-6: artifacts the local repository lacks and Gradle's FAILED entries are
    /// marked missing; the repository layout of jars, POMs and classifiers.
    #[test]
    fn missing_artifacts_are_marked() {
        let text = "com.acme:app:jar:1.0\n\
                    +- junit:junit:jar:4.13.2:test\n\
                    \\- lab.missing:nowhere:jar:9.9:compile\n";
        let nodes = parse_maven_tree_with(text, &|c| c[0] != "lab.missing");
        assert!(!nodes[0].missing && nodes[1].missing);
        assert_eq!(
            crate::buildtools::missing_artifacts(&nodes),
            ["lab.missing:nowhere:9.9"]
        );
        let repo = Path::new("/r");
        assert_eq!(
            maven_artifact_file(repo, &["org.junit", "j", "jar", "5"]).unwrap(),
            Path::new("/r/org/junit/j/5/j-5.jar")
        );
        assert_eq!(
            maven_artifact_file(repo, &["g", "p", "pom", "1"]).unwrap(),
            Path::new("/r/g/p/1/p-1.pom")
        );
        assert_eq!(
            maven_artifact_file(repo, &["g", "n", "jar", "linux", "2"]).unwrap(),
            Path::new("/r/g/n/2/n-2-linux.jar")
        );
        let gradle = "compileClasspath - Compile classpath.\n\
+--- com.google.guava:guava:32.1.3-jre\n\
\\--- lab.missing:nowhere:9.9 FAILED\n";
        let tree = parse_gradle_tree(gradle);
        let entries = &tree[0].children;
        assert_eq!(entries[1].name, "lab.missing:nowhere");
        assert_eq!(entries[1].version.as_deref(), Some("9.9"));
        assert!(entries[1].missing && !entries[0].missing);
    }

    /// SPEC-0149 v3 AK-7: entries Maven left out keep their reason, never count as missing and
    /// stay out of the window's tree and the libraries.
    #[test]
    fn verbose_maven_entries() {
        let text = "lab.l7:l7-core:jar:1.0\n\
                    +- org.apache.commons:commons-text:jar:1.12.0:compile\n\
                    |  \\- (org.apache.commons:commons-lang3:jar:3.14.0:compile - omitted for conflict with 3.12.0)\n\
                    +- org.apache.commons:commons-lang3:jar:3.12.0:compile\n\
                    +- org.slf4j:slf4j-api:jar:2.0.9:compile (version managed from 2.0.7)\n\
                    \\- junit:junit:jar:4.13.2:test\n   \
                    +- org.hamcrest:hamcrest-core:jar:1.3:test\n   \
                    \\- (org.slf4j:slf4j-api:jar:2.0.9:test - version managed from 1.7.0; omitted for duplicate)\n";
        let nodes = parse_maven_tree_with(text, &|c| c[2] != "jar" || c[3] != "3.14.0");
        let lang = &nodes[0].children[0];
        assert_eq!(lang.name, "org.apache.commons:commons-lang3");
        assert_eq!(lang.version.as_deref(), Some("3.14.0"));
        assert_eq!(
            lang.omitted.as_deref(),
            Some("omitted for conflict with 3.12.0")
        );
        assert!(!lang.missing, "a left-out entry is never missing");
        assert_eq!(nodes[2].version.as_deref(), Some("2.0.9"));
        assert_eq!(nodes[2].scope, "compile");
        assert_eq!(
            nodes[3].children[1].omitted.as_deref(),
            Some("omitted for duplicate")
        );
        let shown = crate::deptree::without_omitted(&nodes);
        assert!(shown[0].children.is_empty());
        assert_eq!(shown[3].children.len(), 1);
        assert!(crate::buildtools::missing_artifacts(&nodes).is_empty());
    }

    #[test]
    fn maven_tree_output() {
        let text = "com.acme:app:jar:1.0\n\
                    +- junit:junit:jar:4.13.2:test\n\
                    |  \\- org.hamcrest:hamcrest-core:jar:1.3:test\n\
                    +- com.acme:native:jar:linux-x86_64:2.1:compile (optional)\n\
                    \\- org.slf4j:slf4j-simple:jar:2.0.9:compile\n   \
                    \\- org.slf4j:slf4j-api:jar:2.0.9:compile\n";
        assert_eq!(
            lines(&parse_maven_tree(text)),
            [
                "junit:junit 4.13.2 (test)",
                "  org.hamcrest:hamcrest-core 1.3",
                "com.acme:native 2.1 (compile, optional)",
                "org.slf4j:slf4j-simple 2.0.9 (compile)",
                "  org.slf4j:slf4j-api 2.0.9",
            ]
        );
    }

    #[test]
    fn gradle_dependencies_output() {
        let text = "\n------------------------------------------------------------\n\
Root project 'app'\n\
------------------------------------------------------------\n\n\
annotationProcessor - Annotation processors and their dependencies for source set 'main'.\n\
No dependencies\n\n\
compileClasspath - Compile classpath for source set 'main'.\n\
+--- org.slf4j:slf4j-simple:2.0.9\n\
|    \\--- org.slf4j:slf4j-api:2.0.9\n\
+--- org.slf4j:slf4j-api:2.0.7 -> 2.0.9\n\
+--- project :lib\n\
|    \\--- org.slf4j:slf4j-simple:2.0.9 (*)\n\
\\--- org.slf4j:slf4j-bom:2.0.9 (c)\n\n\
implementation - Implementation dependencies for the 'main' feature. (n)\n\
\\--- org.slf4j:slf4j-simple:2.0.9 (n)\n\n\
testRuntimeClasspath - Runtime classpath of source set 'test'.\n\
\\--- junit:junit:4.13.2\n     \
\\--- org.hamcrest:hamcrest-core:1.3\n\n\
(*) - Indicates repeated occurrences of a transitive dependency subtree.\n\n\
------------------------------------------------------------\n\
Project ':lib'\n\
------------------------------------------------------------\n\
compileClasspath - Compile classpath for source set 'main'.\n\
\\--- other:thing:1\n";
        assert_eq!(
            lines(&parse_gradle_tree(text)),
            [
                "compileClasspath - (configuration)",
                "  org.slf4j:slf4j-simple 2.0.9",
                "    org.slf4j:slf4j-api 2.0.9",
                "  org.slf4j:slf4j-api 2.0.7 → 2.0.9",
                "  :lib -",
                "    org.slf4j:slf4j-simple 2.0.9 *",
                "testRuntimeClasspath - (configuration)",
                "  junit:junit 4.13.2",
                "    org.hamcrest:hamcrest-core 1.3",
            ]
        );
    }

    #[test]
    fn resolutions_go_stale_with_the_build_file() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-jvmdeps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/pom.xml"), "<project/>").unwrap();
        let mut m = Module::new(ModuleKind::Maven, "sub".into(), "sub");
        m.dir = "sub".into();
        let cache = cache_file(&dir, &m).unwrap();
        assert!(cache.ends_with(".ferris-idea/deps/sub.maven.txt"));
        assert!(!is_resolved(&dir, &m));
        std::thread::sleep(Duration::from_millis(20));
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, "com.acme:sub:jar:1\n\\- a:b:jar:1:compile\n").unwrap();
        assert!(is_resolved(&dir, &m));
        assert_eq!(resolved_tree(&dir, &m).unwrap()[0].name, "a:b");
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(
            dir.join("sub/pom.xml"),
            "<project><!-- changed --></project>",
        )
        .unwrap();
        assert!(!is_resolved(&dir, &m));
        let (cwd, command) = resolve_command(&dir, &m, &cache).unwrap();
        assert_eq!(cwd, dir.join("sub"));
        assert_eq!(command[0], "mvn");
        assert!(command.contains(&"dependency:tree".to_owned()));
        std::fs::write(dir.join("mvnw"), "").unwrap();
        let (_, command) = resolve_command(&dir, &m, &cache).unwrap();
        assert!(command[0].ends_with("mvnw"));
        let npm = Module::new(ModuleKind::Npm, "x".into(), ".");
        assert!(cache_file(&dir, &npm).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
