//! SPEC-0149 AK-2: Maven and Gradle resolve the transitive dependencies of a small project with
//! the real build tools; the tree is kept until the build file changes. Skipped without `mvn` or
//! `gradle` (the artifacts come from the local caches or the network).

use std::path::{Path, PathBuf};

use idecraft_core::{DepNode, Workspace, dependency_tree, jvmdeps, read_project};

fn on_path(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn skip(reason: &str) {
    assert!(
        std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

fn project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("idecraft-jvm-resolve-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    dir.canonicalize().unwrap()
}

fn module(root: &Path) -> idecraft_core::Module {
    let files = Workspace::open(root).unwrap().files().unwrap();
    read_project(root, &files).modules.remove(0)
}

fn find<'a>(nodes: &'a [DepNode], name: &str) -> Option<&'a DepNode> {
    nodes.iter().find(|n| n.name == name)
}

#[test]
fn maven_resolves_transitive_dependencies() {
    if !on_path("mvn") {
        return skip("mvn is not installed");
    }
    let root = project(
        "maven",
        &[(
            "pom.xml",
            r#"<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>
<groupId>com.acme</groupId><artifactId>app</artifactId><version>1.0</version>
<dependencies>
<dependency><groupId>junit</groupId><artifactId>junit</artifactId><version>4.13.2</version><scope>test</scope></dependency>
</dependencies></project>"#,
        )],
    );
    let m = module(&root);
    assert!(!jvmdeps::is_resolved(&root, &m));
    let direct = dependency_tree(&root, &m).unwrap();
    assert!(direct.iter().all(|n| n.children.is_empty()));
    jvmdeps::resolve(&root, &m).unwrap();
    let tree = dependency_tree(&root, &m).unwrap();
    let junit = find(&tree, "junit:junit").expect("junit");
    assert_eq!(junit.scope, "test");
    assert_eq!(junit.children[0].name, "org.hamcrest:hamcrest-core");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn gradle_resolves_classpaths() {
    if !on_path("gradle") {
        return skip("gradle is not installed");
    }
    let root = project(
        "gradle",
        &[
            ("settings.gradle.kts", "rootProject.name = \"app\"\n"),
            (
                "build.gradle.kts",
                "plugins { java }\nrepositories { mavenCentral() }\ndependencies {\n    testImplementation(\"junit:junit:4.13.2\")\n}\n",
            ),
        ],
    );
    let m = module(&root);
    jvmdeps::resolve(&root, &m).unwrap();
    let tree = dependency_tree(&root, &m).unwrap();
    let test = find(&tree, "testRuntimeClasspath").expect("test runtime classpath");
    let junit = find(&test.children, "junit:junit").expect("junit");
    assert_eq!(junit.children[0].name, "org.hamcrest:hamcrest-core");
    assert!(
        find(&tree, "compileClasspath").is_none(),
        "an empty configuration is left out"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_broken_build_reports_the_tool_error() {
    if !on_path("mvn") {
        return skip("mvn is not installed");
    }
    let root = project(
        "broken",
        &[(
            "pom.xml",
            "<project><modelVersion>4.0.0</modelVersion><groupId>a</groupId><artifactId>b</artifactId><version>1</version><dependencies><dependency><groupId>no.such</groupId><artifactId>thing</artifactId></dependency></dependencies></project>",
        )],
    );
    let m = module(&root);
    let err = jvmdeps::resolve(&root, &m).unwrap_err();
    assert!(err.starts_with("mvn failed:"), "{err}");
    assert!(!jvmdeps::is_resolved(&root, &m));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0149 v3 AK-7, SPEC-0100 v3 AK-12: the verbose resolution keeps the version Maven left out
/// for a conflict, and the analyzer names the conflict with its path.
#[test]
fn maven_keeps_entries_left_out_for_a_conflict() {
    if !on_path("mvn") {
        return skip("mvn is not installed");
    }
    let root = project(
        "maven-conflict",
        &[(
            "pom.xml",
            r#"<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>
<groupId>lab.l7</groupId><artifactId>l7-core</artifactId><version>1.0</version>
<dependencies>
<dependency><groupId>org.apache.commons</groupId><artifactId>commons-text</artifactId><version>1.12.0</version></dependency>
<dependency><groupId>org.apache.commons</groupId><artifactId>commons-lang3</artifactId><version>3.12.0</version></dependency>
</dependencies></project>"#,
        )],
    );
    let m = module(&root);
    jvmdeps::resolve(&root, &m).unwrap();
    let tree = dependency_tree(&root, &m).unwrap();
    let text = find(&tree, "org.apache.commons:commons-text").unwrap();
    let lang = find(&text.children, "org.apache.commons:commons-lang3").unwrap();
    assert_eq!(
        lang.omitted.as_deref(),
        Some("omitted for conflict with 3.12.0")
    );
    assert!(!lang.missing);
    let analysis = idecraft_core::depanalysis::analyze("l7-core", &tree);
    assert_eq!(analysis[0].name, "org.apache.commons:commons-lang3");
    assert!(analysis[0].conflict);
    assert_eq!(analysis[0].chosen, "3.12.0");
    let _ = std::fs::remove_dir_all(&root);
}
