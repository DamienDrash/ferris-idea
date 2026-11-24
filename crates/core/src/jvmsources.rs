//! Sources and documentation of Maven and Gradle libraries (`SPEC-0149` v3 behaviours 10 and 11):
//! the build tools download them, the local caches tell which libraries have them, and the sources
//! archive of a library jar is found for the editor.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::buildtools::{BuildOptions, gradle_build};
use crate::deptree::DepNode;
use crate::project::{ModuleKind, ProjectModel};

/// What is downloaded: sources (`-sources.jar`) or documentation (`-javadoc.jar`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The `sources` classifier.
    Sources,
    /// The `javadoc` classifier.
    Javadoc,
}

impl Kind {
    /// The Maven classifier.
    pub fn classifier(self) -> &'static str {
        match self {
            Kind::Sources => "sources",
            Kind::Javadoc => "javadoc",
        }
    }
}

/// The init script that adds `ferrisIdeaDownloadSources` to every project of a Gradle build: it asks
/// Gradle for the sources and Javadoc artifacts of the components on the resolvable classpaths
/// (`-PferrisIdeaKinds=sources,javadoc`) and prints what it found.
pub const GRADLE_SOURCES_SCRIPT: &str = r#"// Ferris IDEA: downloads the sources and documentation of the libraries (SPEC-0149 v3 behaviour 10).
import org.gradle.api.artifacts.component.ModuleComponentIdentifier
import org.gradle.api.artifacts.result.ResolvedArtifactResult
import org.gradle.jvm.JvmLibrary
import org.gradle.language.base.artifact.SourcesArtifact
import org.gradle.language.java.artifact.JavadocArtifact

allprojects { p ->
    p.tasks.register('ferrisIdeaDownloadSources') { t ->
        if (t.metaClass.respondsTo(t, 'notCompatibleWithConfigurationCache', String)) {
            t.notCompatibleWithConfigurationCache('reads the configurations when it runs')
        }
        t.doLast {
            def kinds = (p.findProperty('ferrisIdeaKinds') ?: 'sources').toString().split(',') as List
            def types = []
            if (kinds.contains('sources')) types << SourcesArtifact
            if (kinds.contains('javadoc')) types << JavadocArtifact
            def ids = [] as Set
            p.configurations.findAll { it.canBeResolved && it.name.endsWith('Classpath') }.each { c ->
                try {
                    c.incoming.resolutionResult.allComponents.each { comp ->
                        if (comp.id instanceof ModuleComponentIdentifier) ids << comp.id
                    }
                } catch (Exception e) {
                    println "FERRIS_IDEA-ERROR ${c.name}: ${e.message}"
                }
            }
            if (ids.isEmpty() || types.isEmpty()) return
            def result = p.dependencies.createArtifactResolutionQuery()
                .forComponents(ids)
                .withArtifacts(JvmLibrary, *types)
                .execute()
            result.resolvedComponents.each { comp ->
                types.each { ty ->
                    def kind = ty == SourcesArtifact ? 'sources' : 'javadoc'
                    def found = comp.getArtifacts(ty).findAll { it instanceof ResolvedArtifactResult }
                    if (found.isEmpty()) println "FERRIS_IDEA-MISSING ${kind} ${comp.id.displayName}"
                    found.each { a -> println "FERRIS_IDEA-FOUND ${kind} ${comp.id.displayName} ${a.file}" }
                }
            }
            result.components.findAll { !result.resolvedComponents.contains(it) }.each { comp ->
                println "FERRIS_IDEA-ERROR ${comp.id.displayName}: ${comp.failure?.message}"
            }
        }
    }
}
"#;

/// One command of a download (behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Download {
    /// Folder it runs in, workspace-relative.
    pub dir: String,
    /// The command with the window's options.
    pub command: Vec<String>,
}

/// The commands that download the sources and documentation of the Maven and Gradle modules of
/// `model` (behaviour 10): `dependency:sources` and `dependency:resolve -Dclassifier=javadoc` per
/// Maven module with dependencies, the init script's task once per Gradle build; with the
/// window's options. Writes the init script when there is a Gradle build.
pub fn download_commands(
    root: &Path,
    model: &ProjectModel,
    options: &BuildOptions,
) -> Result<Vec<Download>, String> {
    let profiles = crate::buildtools::project_profiles(model);
    let mut out = Vec::new();
    for m in model.modules.iter().filter(|m| m.kind == ModuleKind::Maven) {
        if m.dependencies.is_empty() && m.packaging() == Some("pom") {
            continue;
        }
        let dir = if m.dir == "." {
            root.to_path_buf()
        } else {
            root.join(&m.dir)
        };
        let mvn = crate::jvmdeps::wrapper(root, &dir, "mvnw")
            .map(|w| w.display().to_string())
            .unwrap_or_else(|| "mvn".to_owned());
        for goal in [
            vec!["dependency:sources"],
            vec!["dependency:resolve", "-Dclassifier=javadoc"],
        ] {
            let mut command = vec![mvn.clone(), "-B".to_owned(), "-N".to_owned()];
            command.extend(goal.iter().map(|g| (*g).to_owned()));
            out.push(Download {
                dir: m.dir.clone(),
                command: options.apply(&command, &profiles),
            });
        }
    }
    let mut builds: Vec<String> = Vec::new();
    for m in model
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Gradle)
    {
        let build = gradle_build(root, m);
        if builds.contains(&build.root_dir) {
            continue;
        }
        builds.push(build.root_dir.clone());
        let script = crate::buildtasks::write_init_script(
            root,
            "ferris-idea-sources.gradle",
            GRADLE_SOURCES_SCRIPT,
        )?;
        let dir = if build.root_dir == "." {
            root.to_path_buf()
        } else {
            root.join(&build.root_dir)
        };
        let runner = if build.runner == "./gradlew" {
            dir.join("gradlew").display().to_string()
        } else {
            "gradle".to_owned()
        };
        let command = vec![
            runner,
            "-q".to_owned(),
            "--console=plain".to_owned(),
            "--init-script".to_owned(),
            script.display().to_string(),
            "-PferrisIdeaKinds=sources,javadoc".to_owned(),
            "ferrisIdeaDownloadSources".to_owned(),
        ];
        out.push(Download {
            dir: build.root_dir,
            command: options.apply(&command, &profiles),
        });
    }
    Ok(out)
}

/// Longest one download command may take.
const DOWNLOAD_LIMIT: Duration = Duration::from_secs(600);

/// Runs the download commands (behaviour 10) one after the other; returns one line per command
/// that failed, with the tool's last lines.
pub fn download(root: &Path, model: &ProjectModel) -> Result<Vec<String>, String> {
    // The build tool runs the project's build logic: never in safe mode (SPEC-0278).
    crate::trust::require_trusted(root)?;
    let options = BuildOptions::load(root);
    let mut failures = Vec::new();
    for d in download_commands(root, model, &options)? {
        let cwd = if d.dir == "." {
            root.to_path_buf()
        } else {
            root.join(&d.dir)
        };
        let mut child = Command::new(&d.command[0])
            .args(&d.command[1..])
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", d.command[0]))?;
        let read = |r: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_string(&mut s);
                }
                s
            })
        };
        let out = read(
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        );
        let err = read(
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        );
        let started = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break Some(s);
            }
            if started.elapsed() > DOWNLOAD_LIMIT {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let text = out.join().unwrap_or_default() + &err.join().unwrap_or_default();
        let shown = format!("{} ({})", d.command.join(" "), d.dir);
        match status {
            Some(s) if s.success() => {}
            Some(_) => {
                let tail: Vec<&str> = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .rev()
                    .take(2)
                    .collect();
                let tail: Vec<&str> = tail.into_iter().rev().collect();
                failures.push(format!("{shown} failed: {}", tail.join(" / ")));
            }
            None => failures.push(format!(
                "{shown} took longer than {} s",
                DOWNLOAD_LIMIT.as_secs()
            )),
        }
    }
    Ok(failures)
}

/// Gradle's user folder: `GRADLE_USER_HOME`, else `~/.gradle`.
pub fn gradle_user_home() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("GRADLE_USER_HOME") {
        return Some(PathBuf::from(d));
    }
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".gradle"))
}

/// `<artifact>-<version>-<classifier>.jar` of `group:artifact` in any folder of the version in the
/// Gradle cache `files` (`…/modules-2/files-2.1`).
pub fn gradle_cache_archive(
    files: &Path,
    name: &str,
    version: &str,
    kind: Kind,
) -> Option<PathBuf> {
    let (group, artifact) = name.split_once(':')?;
    let wanted = format!("{artifact}-{version}-{}.jar", kind.classifier());
    let dir = files.join(group).join(artifact).join(version);
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path().join(&wanted))
        .find(|p| p.is_file())
}

/// Where `kind` of library `name` (`group:artifact`) in `version` lies in the local caches of
/// `tool` (behaviour 10): beside the jar in the local Maven repository, or in the Gradle cache.
pub fn local_archive(tool: ModuleKind, name: &str, version: &str, kind: Kind) -> Option<PathBuf> {
    match tool {
        ModuleKind::Gradle => {
            let files = gradle_user_home()?.join("caches/modules-2/files-2.1");
            gradle_cache_archive(&files, name, version, kind)
        }
        _ => {
            let repo = crate::jvmdeps::maven_local_repository()?;
            let (group, artifact) = name.split_once(':')?;
            crate::jvmdeps::maven_artifact_file(
                &repo,
                &[group, artifact, "jar", kind.classifier(), version],
            )
            .filter(|f| f.is_file())
        }
    }
}

/// The sources archive of library jar `jar` (behaviour 11): `<stem>-sources.jar` beside it, or —
/// in the Gradle cache, where every file has a folder of its own — in another folder of the same
/// version.
pub fn sources_archive_for(jar: &Path) -> Option<PathBuf> {
    let stem = jar.file_stem()?.to_str()?;
    let name = format!("{stem}-sources.jar");
    let beside = jar.with_file_name(&name);
    if beside.is_file() {
        return Some(beside);
    }
    // files-2.1/<group>/<artifact>/<version>/<hash>/<file>
    let version_dir = jar.parent()?.parent()?;
    if !version_dir
        .ancestors()
        .any(|a| a.file_name().is_some_and(|n| n == "files-2.1"))
    {
        return None;
    }
    std::fs::read_dir(version_dir)
        .ok()?
        .flatten()
        .map(|e| e.path().join(&name))
        .find(|p| p.is_file())
}

/// Whether a library has its sources and documentation in the local caches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryState {
    /// `group:artifact`.
    pub name: String,
    /// The version on the classpath.
    pub version: String,
    /// The build tool (`maven`, `gradle`).
    pub tool: ModuleKind,
    /// The sources archive, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<PathBuf>,
    /// The Javadoc archive, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub javadoc: Option<PathBuf>,
}

fn collect(
    model: &ProjectModel,
    tool: ModuleKind,
    nodes: &[DepNode],
    out: &mut Vec<(ModuleKind, String, String)>,
) {
    for n in nodes {
        let library = n.scope != "configuration"
            && n.omitted.is_none()
            && !n.missing
            && !n.name.starts_with(':')
            && !model
                .modules
                .iter()
                .any(|m| m.kind == ModuleKind::Maven && m.name == n.name);
        if library {
            let v = n.version.clone().unwrap_or_default();
            let v = v.rsplit("→ ").next().unwrap_or(&v).trim().to_owned();
            let entry = (tool, n.name.clone(), v);
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
        collect(model, tool, &n.children, out);
    }
}

/// The libraries of the resolved Maven and Gradle modules with their sources and documentation in
/// the local caches, sorted by name (behaviour 10). Modules not resolved yet are left out.
pub fn library_states(root: &Path, model: &ProjectModel) -> Vec<LibraryState> {
    let mut all: Vec<(ModuleKind, String, String)> = Vec::new();
    for m in &model.modules {
        if !matches!(m.kind, ModuleKind::Maven | ModuleKind::Gradle) {
            continue;
        }
        if let Some(tree) = crate::jvmdeps::resolved_tree(root, m) {
            collect(model, m.kind, &tree, &mut all);
        }
    }
    let mut out: Vec<LibraryState> = all
        .into_iter()
        .map(|(tool, name, version)| LibraryState {
            sources: local_archive(tool, &name, &version, Kind::Sources),
            javadoc: local_archive(tool, &name, &version, Kind::Javadoc),
            name,
            version,
            tool,
        })
        .collect();
    out.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    out
}

/// "12 of 14 libraries have sources, 11 documentation; none for a:b:1, c:d:2" (behaviour 10 and
/// SPEC-0100 v3 behaviour 13).
pub fn summary(states: &[LibraryState]) -> String {
    if states.is_empty() {
        return "No resolved Maven or Gradle libraries yet".to_owned();
    }
    let sources = states.iter().filter(|s| s.sources.is_some()).count();
    let docs = states.iter().filter(|s| s.javadoc.is_some()).count();
    let without: Vec<String> = states
        .iter()
        .filter(|s| s.sources.is_none())
        .map(|s| format!("{}:{}", s.name, s.version))
        .collect();
    let mut text = format!(
        "{sources} of {} libraries have sources, {docs} documentation",
        states.len()
    );
    if !without.is_empty() {
        let shown: Vec<&str> = without.iter().take(3).map(String::as_str).collect();
        text.push_str(&format!("; no sources for {}", shown.join(", ")));
        if without.len() > 3 {
            text.push_str(&format!(" and {} more", without.len() - 3));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ferris-idea-jvmsources-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// SPEC-0149 v3 AK-8: the commands per tool with the options and the init script; archives
    /// beside the jar and in another folder of the Gradle cache.
    #[test]
    fn download_commands_and_archives() {
        let root = tmp("commands");
        std::fs::write(
            root.join("pom.xml"),
            r#"<project><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>app</artifactId><version>1</version><dependencies><dependency><groupId>junit</groupId><artifactId>junit</artifactId><version>4.13.2</version></dependency></dependencies></project>"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("g/lib")).unwrap();
        std::fs::write(root.join("g/settings.gradle.kts"), "include(\"lib\")\n").unwrap();
        std::fs::write(root.join("g/lib/build.gradle.kts"), "plugins { java }\n").unwrap();
        std::fs::write(root.join("g/gradlew"), "").unwrap();
        let files = crate::Workspace::open(&root)
            .and_then(|w| w.files())
            .unwrap();
        let model = crate::read_project(&root, &files);
        let options = BuildOptions {
            offline: true,
            ..Default::default()
        };
        let commands = download_commands(&root, &model, &options).unwrap();
        assert_eq!(
            commands[0].command,
            ["mvn", "-o", "-B", "-N", "dependency:sources"]
        );
        assert_eq!(
            commands[1].command,
            [
                "mvn",
                "-o",
                "-B",
                "-N",
                "dependency:resolve",
                "-Dclassifier=javadoc"
            ]
        );
        let gradle: Vec<&Download> = commands.iter().filter(|c| c.dir == "g").collect();
        assert_eq!(gradle.len(), 1, "once per build");
        assert!(gradle[0].command[0].ends_with("g/gradlew"));
        assert_eq!(gradle[0].command[1], "--offline");
        assert_eq!(
            gradle[0].command.last().unwrap(),
            "ferrisIdeaDownloadSources"
        );
        let script = root.join(".ferris-idea/gradle/ferris-idea-sources.gradle");
        assert!(
            std::fs::read_to_string(script)
                .unwrap()
                .contains("SourcesArtifact")
        );
        // Archives: beside a Maven jar, in another folder of the Gradle cache.
        let m2 = root.join("m2/org/x/lib/1.0");
        std::fs::create_dir_all(&m2).unwrap();
        std::fs::write(m2.join("lib-1.0.jar"), "").unwrap();
        std::fs::write(m2.join("lib-1.0-sources.jar"), "").unwrap();
        assert_eq!(
            sources_archive_for(&m2.join("lib-1.0.jar")),
            Some(m2.join("lib-1.0-sources.jar"))
        );
        let cache = root.join("files-2.1");
        let jar_dir = cache.join("org.x/lib/2.0/aaa");
        let src_dir = cache.join("org.x/lib/2.0/bbb");
        std::fs::create_dir_all(&jar_dir).unwrap();
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(jar_dir.join("lib-2.0.jar"), "").unwrap();
        std::fs::write(src_dir.join("lib-2.0-sources.jar"), "").unwrap();
        assert_eq!(
            sources_archive_for(&jar_dir.join("lib-2.0.jar")),
            Some(src_dir.join("lib-2.0-sources.jar"))
        );
        assert_eq!(
            gradle_cache_archive(&cache, "org.x:lib", "2.0", Kind::Sources),
            Some(src_dir.join("lib-2.0-sources.jar"))
        );
        assert!(gradle_cache_archive(&cache, "org.x:lib", "2.0", Kind::Javadoc).is_none());
        let states = vec![
            LibraryState {
                name: "a:b".into(),
                version: "1".into(),
                tool: ModuleKind::Maven,
                sources: Some(PathBuf::from("/x")),
                javadoc: None,
            },
            LibraryState {
                name: "c:d".into(),
                version: "2".into(),
                tool: ModuleKind::Maven,
                sources: None,
                javadoc: None,
            },
        ];
        assert_eq!(
            summary(&states),
            "1 of 2 libraries have sources, 0 documentation; no sources for c:d:2"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
