//! SPEC-0049 AK-1 – AK-5, v3 AK-8 – AK-12.

use std::path::{Path, PathBuf};

use idecraft_core::{ModuleSdk, RunKind, SourceKind, idea_run_configs, read_idea_project};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("idecraft-idea-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const MODULES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectModuleManager">
    <modules>
      <module fileurl="file://$PROJECT_DIR$/shop.iml" filepath="$PROJECT_DIR$/shop.iml" />
      <module fileurl="file://$PROJECT_DIR$/core/core.iml" filepath="$PROJECT_DIR$/core/core.iml" group="libs" />
    </modules>
  </component>
</project>"#;

const SHOP_IML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<module type="JAVA_MODULE" version="4">
  <component name="NewModuleRootManager" inherit-compiler-output="true">
    <exclude-output />
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/src" isTestSource="false" packagePrefix="com.shop" />
      <sourceFolder url="file://$MODULE_DIR$/test" isTestSource="true" />
      <sourceFolder url="file://$MODULE_DIR$/res" type="java-resource" />
      <excludeFolder url="file://$MODULE_DIR$/build" />
    </content>
    <orderEntry type="inheritedJdk" />
    <orderEntry type="sourceFolder" forTests="false" />
    <orderEntry type="module" module-name="core" exported="" />
    <orderEntry type="library" scope="TEST" name="junit" level="project" />
    <orderEntry type="module-library">
      <library>
        <CLASSES>
          <root url="jar://$MODULE_DIR$/lib/gson.jar!/" />
        </CLASSES>
        <JAVADOC />
        <SOURCES />
      </library>
    </orderEntry>
  </component>
</module>"#;

const CORE_IML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<module type="JAVA_MODULE" version="4">
  <component name="NewModuleRootManager" LANGUAGE_LEVEL="JDK_11">
    <output url="file://$MODULE_DIR$/../out/core" />
    <output-test url="file://$MODULE_DIR$/../out/core-test" />
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/src/main/java" isTestSource="false" />
      <sourceFolder url="file://$MODULE_DIR$/src/test/resources" type="java-test-resource" />
    </content>
    <orderEntry type="jdk" jdkName="11" jdkType="JavaSDK" />
    <orderEntry type="sourceFolder" forTests="false" />
  </component>
</module>"#;

const MISC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectRootManager" version="2" languageLevel="JDK_17" default="true" project-jdk-name="17" project-jdk-type="JavaSDK">
    <output url="file://$PROJECT_DIR$/out" />
  </component>
</project>"#;

const JUNIT: &str = r#"<component name="libraryTable">
  <library name="junit">
    <CLASSES>
      <root url="jar://$PROJECT_DIR$/lib/junit-4.13.jar!/" />
      <root url="jar://$MAVEN_REPOSITORY$/org/hamcrest/hamcrest-core-1.3.jar!/" />
    </CLASSES>
    <JAVADOC />
    <SOURCES>
      <root url="jar://$PROJECT_DIR$/lib/junit-4.13-sources.jar!/" />
    </SOURCES>
  </library>
</component>"#;

const VCS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="VcsDirectoryMappings">
    <mapping directory="" vcs="Git" />
    <mapping directory="$PROJECT_DIR$/vendor/lib" vcs="Git" />
  </component>
</project>"#;

fn sample(tag: &str) -> Scratch {
    let s = Scratch::new(tag);
    s.write(".idea/modules.xml", MODULES);
    s.write(".idea/misc.xml", MISC);
    s.write(".idea/vcs.xml", VCS);
    s.write(".idea/libraries/junit.xml", JUNIT);
    s.write(".idea/.name", "Shop\n");
    s.write("shop.iml", SHOP_IML);
    s.write("core/core.iml", CORE_IML);
    // Deliberately malformed: proves the database tool's file is never opened. Of workspace.xml
    // only the RunManager counts (v3 behaviour 7), so another component adds nothing.
    s.write(
        ".idea/workspace.xml",
        "<project version=\"4\"><component name=\"PropertiesComponent\"><property name=\"x\" value=\"<\" /></component></project>",
    );
    s.write(".idea/dataSources.xml", "<project");
    s
}

#[test]
fn no_idea_folder_is_not_a_reference_project() {
    let s = Scratch::new("none");
    assert!(read_idea_project(&s.0).is_none());
}

#[test]
fn project_modules_libraries_and_vcs() {
    let s = sample("full");
    let p = read_idea_project(&s.0).unwrap();
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(p.name, "Shop");
    assert_eq!(p.sdk.as_deref(), Some("17"));
    assert_eq!(p.sdk_type.as_deref(), Some("JavaSDK"));
    assert_eq!(p.language_level.as_deref(), Some("JDK_17"));
    assert_eq!(p.output.as_deref(), Some("out"));
    assert_eq!(p.not_read, ["dataSources.xml", "workspace.xml"]);

    assert_eq!(p.modules.len(), 2);
    let shop = &p.modules[0];
    assert_eq!(shop.name, "shop");
    assert_eq!(shop.file, "shop.iml");
    assert_eq!(shop.module_type.as_deref(), Some("JAVA_MODULE"));
    assert_eq!(shop.sdk, ModuleSdk::Inherited);
    assert!(shop.inherit_output);
    let root = &shop.content_roots[0];
    assert_eq!(root.dir, ".");
    let kinds: Vec<(&str, SourceKind)> = root
        .sources
        .iter()
        .map(|f| (f.dir.as_str(), f.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("src", SourceKind::Source),
            ("test", SourceKind::Test),
            ("res", SourceKind::Resource)
        ]
    );
    assert_eq!(root.sources[0].package_prefix.as_deref(), Some("com.shop"));
    assert_eq!(root.excluded, ["build"]);

    let deps: Vec<(&str, &str, &str, bool)> = shop
        .dependencies
        .iter()
        .map(|d| {
            (
                d.kind.as_str(),
                d.name.as_str(),
                d.scope.as_str(),
                d.exported,
            )
        })
        .collect();
    assert_eq!(
        deps,
        [
            ("module", "core", "compile", true),
            ("library", "junit", "test", false),
            ("module-library", "lib/gson.jar", "compile", false),
        ]
    );
    assert_eq!(shop.dependencies[1].level.as_deref(), Some("project"));
    assert_eq!(shop.dependencies[2].roots, ["lib/gson.jar"]);

    let core = &p.modules[1];
    assert_eq!(core.group.as_deref(), Some("libs"));
    assert_eq!(core.sdk, ModuleSdk::Named("11".to_owned()));
    assert_eq!(core.language_level.as_deref(), Some("JDK_11"));
    assert!(!core.inherit_output);
    assert_eq!(core.output.as_deref(), Some("out/core"));
    assert_eq!(core.test_output.as_deref(), Some("out/core-test"));
    assert_eq!(core.content_roots[0].dir, "core");
    assert_eq!(core.content_roots[0].sources[0].dir, "core/src/main/java");
    assert_eq!(
        core.content_roots[0].sources[1].kind,
        SourceKind::TestResource
    );

    assert_eq!(p.libraries.len(), 1);
    assert_eq!(
        p.libraries[0].classes,
        [
            "lib/junit-4.13.jar",
            "$MAVEN_REPOSITORY$/org/hamcrest/hamcrest-core-1.3.jar"
        ]
    );
    assert_eq!(p.libraries[0].sources, ["lib/junit-4.13-sources.jar"]);

    let vcs: Vec<(&str, &str)> = p
        .vcs
        .iter()
        .map(|m| (m.dir.as_str(), m.vcs.as_str()))
        .collect();
    assert_eq!(vcs, [(".", "Git"), ("vendor/lib", "Git")]);
}

#[test]
fn broken_files_and_references_are_problems() {
    let s = sample("broken");
    s.write(
        ".idea/modules.xml",
        &MODULES.replace(
            "    </modules>",
            "      <module filepath=\"$PROJECT_DIR$/gone/gone.iml\" />\n    </modules>",
        ),
    );
    s.write(
        "shop.iml",
        &SHOP_IML
            .replace("module-name=\"core\"", "module-name=\"kore\"")
            .replace("name=\"junit\"", "name=\"testng\""),
    );
    s.write("core/core.iml", "<module><component");
    let p = read_idea_project(&s.0).unwrap();
    let joined = p.problems.join("\n");
    assert!(
        joined.contains("gone/gone.iml: module file listed in modules.xml is missing"),
        "{joined}"
    );
    assert!(
        joined.contains("shop.iml: depends on unknown module kore"),
        "{joined}"
    );
    assert!(
        joined.contains("shop.iml: depends on unknown project library testng"),
        "{joined}"
    );
    assert!(
        joined.starts_with("core/core.iml:") || joined.contains("\ncore/core.iml:"),
        "{joined}"
    );
    // The rest is still read.
    assert_eq!(p.modules.len(), 3);
    assert_eq!(p.modules[0].dependencies.len(), 3);
    assert_eq!(p.sdk.as_deref(), Some("17"));
}

/// The local reference project, created by the reference IDE (its `.idea` is not committed).
#[test]
fn reference_project_sql_basic_if_present() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/sql-basic");
    let Some(p) = read_idea_project(&root) else {
        eprintln!("skipped: no .idea in sql-basic");
        return;
    };
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(p.modules.len(), 1);
    assert_eq!(p.modules[0].file, ".idea/sql-basic.iml");
    // `$MODULE_DIR$` of a module file in `.idea` is the project folder.
    assert_eq!(p.modules[0].content_roots[0].dir, ".");
    assert_eq!(p.modules[0].sdk, ModuleSdk::Inherited);
    assert_eq!(p.vcs[0].dir, "../../..");
    assert!(p.not_read.iter().any(|n| n.starts_with("dataSources")));
}

/// SPEC-0049 AK-5 (v2): the roles the modules give the folders.
#[test]
fn folder_roles_of_the_modules() {
    use idecraft_core::buildtools::FolderRole;
    let s = sample("roles");
    for dir in [
        "src",
        "test",
        "res",
        "build",
        "core/src/main/java",
        "core/src/test/resources",
        "out/core",
        "out/core-test",
    ] {
        std::fs::create_dir_all(s.0.join(dir)).unwrap();
    }
    let root = s.0.canonicalize().unwrap();
    let p = read_idea_project(&root).unwrap();
    let roles = idecraft_core::idea_folder_roles(&root, &p);
    let role = |rel: &str| {
        roles
            .get(&root.join(rel))
            .map(|i| (i.role, i.module_name.clone()))
    };
    assert_eq!(role("src"), Some((FolderRole::Sources, None)));
    assert_eq!(role("test"), Some((FolderRole::Tests, None)));
    assert_eq!(role("res"), Some((FolderRole::Resources, None)));
    assert_eq!(role("build"), Some((FolderRole::Excluded, None)));
    assert_eq!(role("core"), Some((FolderRole::Module, None)));
    assert_eq!(
        role("core/src/main/java"),
        Some((FolderRole::Sources, None))
    );
    assert_eq!(
        role("core/src/test/resources"),
        Some((FolderRole::TestResources, None))
    );
    // The module's own output folders and the project's output are excluded.
    assert_eq!(role("out/core"), Some((FolderRole::Excluded, None)));
    assert_eq!(role("out/core-test"), Some((FolderRole::Excluded, None)));
    assert_eq!(role("out"), Some((FolderRole::Excluded, None)));
    // The project folder is the content root of `shop`, whose name differs from the folder's.
    assert_eq!(
        roles.get(&root).map(|i| (i.role, i.module_name.clone())),
        Some((FolderRole::Module, Some("shop".into())))
    );
    // Nothing above the project, no role for the folder of a module library.
    assert!(roles.keys().all(|k| k.starts_with(&root)), "{roles:?}");
    assert!(!roles.contains_key(&root.join("lib")));
    // The same list for agents, project-relative.
    let folders: Vec<(String, FolderRole)> =
        p.folders.iter().map(|f| (f.dir.clone(), f.role)).collect();
    assert!(
        folders.contains(&(".".into(), FolderRole::Module)),
        "{folders:?}"
    );
    assert!(folders.contains(&("core/src/main/java".into(), FolderRole::Sources)));

    // A content root above the project, a missing folder and an unexpanded macro mark nothing.
    let other = Scratch::new("roles-outside");
    other.write(
        ".idea/modules.xml",
        "<project version=\"4\"><component name=\"ProjectModuleManager\"><modules><module filepath=\"$PROJECT_DIR$/m.iml\" /></modules></component></project>",
    );
    other.write(
        "m.iml",
        "<module type=\"JAVA_MODULE\" version=\"4\"><component name=\"NewModuleRootManager\" inherit-compiler-output=\"true\"><content url=\"file://$MODULE_DIR$/../elsewhere\"><sourceFolder url=\"file://$MODULE_DIR$/../elsewhere/src\" isTestSource=\"false\" /></content><content url=\"file://$MODULE_DIR$\"><sourceFolder url=\"file://$MODULE_DIR$/missing\" isTestSource=\"false\" /><excludeFolder url=\"file://$USER_HOME$/cache\" /></content></component></module>",
    );
    let oroot = other.0.canonicalize().unwrap();
    let op = read_idea_project(&oroot).unwrap();
    let oroles = idecraft_core::idea_folder_roles(&oroot, &op);
    assert_eq!(oroles.len(), 1, "only the module folder: {oroles:?}");
    assert_eq!(oroles.get(&oroot).map(|i| i.role), Some(FolderRole::Module));
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/idea-projects")
        .join(name)
}

/// SPEC-0049 v3 AK-8: the `.ipr` format reads like the folder format.
#[test]
fn an_ipr_project_reads_like_the_folder_format() {
    let p = read_idea_project(&fixture("ipr-basic")).expect("an .ipr is a project");
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(p.name, "ipr-basic");
    assert_eq!(p.ipr.as_deref(), Some("ipr-basic.ipr"));
    assert_eq!(p.sdk.as_deref(), Some("17"));
    assert_eq!(p.output.as_deref(), Some("out"));
    let names: Vec<_> = p.modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["core", "app"]);
    assert_eq!(p.modules[1].group.as_deref(), Some("apps"));
    assert_eq!(p.modules[1].content_roots[0].dir, "app");
    assert_eq!(p.libraries.len(), 1);
    assert_eq!(p.libraries[0].classes, ["lib/util.jar"]);
    assert_eq!(p.vcs[0].dir, ".");
    let runs: Vec<_> = p
        .run_configurations
        .iter()
        .map(|r| (r.name.as_str(), r.source.as_str()))
        .collect();
    assert_eq!(
        runs,
        [("Run app", "ipr-basic.ipr"), ("hello", "ipr-basic.iws")]
    );
    // The `.iws`'s other components are not used.
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains("secret-looking"), "{json}");
    // The application runs with the dependency's output and the library on the class path.
    let app = p.run_configurations[0].config.as_ref().expect("translated");
    assert_eq!(app.cwd, None);
    let script = &app.command[2];
    assert!(
        script.contains("-cp 'out/production/app:out/production/core:lib/util.jar'"),
        "{script}"
    );
    assert!(script.ends_with("'Main' '--greet' 'two words'"), "{script}");
    assert_eq!(
        p.folders.iter().filter(|f| f.dir.ends_with("src")).count(),
        2
    );

    // A second .ipr is named; with .idea present the .ipr is ignored.
    let s = Scratch::new("ipr2");
    s.write("a.ipr", "<project version=\"4\"/>");
    s.write("b.ipr", "<project version=\"4\"/>");
    let two = read_idea_project(&s.0).unwrap();
    assert_eq!(two.name, "a");
    assert_eq!(two.problems, ["b.ipr: another project file; a.ipr is read"]);
    s.write(".idea/.name", "Folder");
    let folder = read_idea_project(&s.0).unwrap();
    assert_eq!(
        (folder.name.as_str(), folder.ipr.as_deref()),
        ("Folder", None)
    );
}

/// SPEC-0049 v3 AK-9: every open type translated, the others with their reason.
#[test]
fn run_configurations_are_translated() {
    let p = read_idea_project(&fixture("run-configs")).unwrap();
    let by = |name: &str| {
        p.run_configurations
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("{name} in {:?}", p.run_configurations))
    };
    let command = |name: &str| by(name).command.clone();
    let order: Vec<_> = p
        .run_configurations
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "Run maven",
            "Run plain",
            "All",
            "grd build",
            "Test class",
            "Test method",
            "Test package",
            "Test plain",
            "mvn install",
            "npm start",
            "outside",
            "http server",
            "hello script",
            "personal py",
        ],
        "shared files by name, then workspace.xml; a repeated name and the template left out"
    );

    let plain = by("Run plain");
    assert_eq!(plain.folder.as_deref(), Some("Java"));
    assert_eq!(plain.env, ["GREETING"]);
    assert_eq!(plain.kind, Some(RunKind::Run));
    let script = &plain.command[2];
    assert_eq!(
        script,
        "mkdir -p 'out/production/plain' && javac -d 'out/production/plain' -cp 'out/production/plain' $(find 'plain/src' -name '*.java') && 'java' '-Xmx64m' '-cp' 'out/production/plain' 'Hello' 'a' 'b c'"
    );
    assert_eq!(
        command("Run maven"),
        [
            "mvn",
            "-q",
            "compile",
            "exec:java",
            "-Dexec.mainClass=com.example.App",
            "-Dexec.args='one' 'two'"
        ]
    );
    let maven = by("Run maven").config.as_ref().unwrap();
    assert_eq!(maven.cwd.as_deref(), Some("mvn"));
    assert_eq!(
        maven.env.get("MAVEN_OPTS").map(String::as_str),
        Some("-Dx=1")
    );

    assert_eq!(
        command("Test class"),
        ["mvn", "test", "-Dtest=com.example.AppTest"]
    );
    assert_eq!(
        command("Test method"),
        ["./gradlew", "test", "--tests", "com.example.CalcTest.adds"]
    );
    assert_eq!(
        command("Test package"),
        ["mvn", "test", "-Dtest=com.example.**"]
    );
    let test = by("Test method").config.as_ref().unwrap();
    assert_eq!(test.kind, RunKind::Test);
    assert_eq!(test.report.as_deref(), Some("junit"));
    assert_eq!(
        test.report_file.as_deref(),
        Some("grd/build/test-results/test")
    );

    assert_eq!(
        command("grd build"),
        ["./gradlew", "clean", "build", "--info"]
    );
    assert_eq!(by("grd build").env, ["CI"]);
    assert_eq!(by("grd build").cwd.as_deref(), Some("grd"));
    assert_eq!(
        command("mvn install"),
        ["mvn", "clean", "install", "-P", "dev,!slow", "-DskipITs"]
    );
    assert_eq!(
        command("http server"),
        ["python3", "-u", "-m", "http.server", "8000"]
    );
    assert_eq!(
        command("hello script"),
        ["/bin/bash", "-e", "hello.sh", "world"]
    );
    assert_eq!(by("hello script").cwd.as_deref(), Some("scripts"));
    let personal = by("personal py");
    assert_eq!(personal.command, ["python3", "tool.py", "--fast"]);
    assert_eq!(personal.folder.as_deref(), Some("Personal"));
    assert_eq!(personal.source, ".idea/workspace.xml");
    // Python keeps IDECraft's interpreter choice; the others are used as written.
    assert!(!personal.config.as_ref().unwrap().verbatim);
    assert!(plain.config.as_ref().unwrap().verbatim);

    for (name, reason) in [
        ("All", "a compound configuration"),
        (
            "Test plain",
            "JUnit in a module without a Maven or Gradle build",
        ),
        (
            "npm start",
            "an npm configuration; its format is not part of the open sources",
        ),
        (
            "outside",
            "its working folder $PROJECT_DIR$/../elsewhere lies outside the project",
        ),
    ] {
        let r = by(name);
        assert!(r.config.is_none() && r.command.is_empty(), "{name}");
        assert!(
            r.reason.as_deref().is_some_and(|x| x.starts_with(reason)),
            "{name}: {:?}",
            r.reason
        );
        assert!(
            p.problems
                .iter()
                .any(|x| x.contains(&format!("run configuration {name}: {reason}"))),
            "{name}: {:?}",
            p.problems
        );
    }
    assert_eq!(p.problems.len(), 4, "{:?}", p.problems);
}

/// SPEC-0049 v3 AK-10: only `RunManager` of workspace.xml, and a malformed one is a problem.
#[test]
fn only_the_run_manager_of_workspace_xml_is_used() {
    let p = read_idea_project(&fixture("run-configs")).unwrap();
    assert!(
        !p.run_configurations
            .iter()
            .any(|r| r.name == "not a run configuration")
    );
    assert!(
        p.not_read.contains(&"workspace.xml".to_owned()),
        "{:?}",
        p.not_read
    );
    let s = sample("bad-workspace");
    s.write(".idea/workspace.xml", "<project");
    let bad = read_idea_project(&s.0).unwrap();
    assert!(
        bad.problems
            .iter()
            .any(|x| x.starts_with(".idea/workspace.xml: ")),
        "{:?}",
        bad.problems
    );
    assert_eq!(bad.modules.len(), 2, "the rest is read");
}

/// SPEC-0049 v3 AK-11: the translated configurations come after `run.toml`, nothing is written.
#[test]
fn translated_configurations_come_after_run_toml() {
    let root = fixture("run-configs");
    let configs = idea_run_configs(&root);
    assert_eq!(configs.len(), 10);
    assert!(configs.iter().all(|c| !c.command.is_empty()));
    let personal = configs.iter().find(|c| c.name == "personal py").unwrap();
    assert_eq!(
        personal.env.get("SECRET_TOKEN").map(String::as_str),
        Some("abc-not-for-agents"),
        "the window's configuration keeps the value"
    );
    let mut all = idecraft_core::parse_run_configs(
        "[[run]]\nname = \"Run plain\"\ncommand = [\"echo\", \"mine\"]\n",
    );
    let added = idecraft_core::add_detected(&mut all, configs);
    assert_eq!(
        all.configs[0].command,
        ["echo", "mine"],
        "run.toml wins a name"
    );
    assert!(!added.contains(&"Run plain".to_owned()));
    assert_eq!(added.len(), 9);
    assert!(!root.join(".idecraft").exists(), "nothing is written");
}

/// SPEC-0049 v3 AK-12: facets per module, and no environment value in what agents get.
#[test]
fn facets_per_module() {
    let p = read_idea_project(&fixture("run-configs")).unwrap();
    let mvn = p.modules.iter().find(|m| m.name == "mvn").unwrap();
    let facets: Vec<_> = mvn
        .facets
        .iter()
        .map(|f| (f.facet_type.as_str(), f.name.as_str()))
        .collect();
    assert_eq!(facets, [("Spring", "Spring"), ("web", "Web")]);
    assert!(
        p.modules
            .iter()
            .find(|m| m.name == "plain")
            .unwrap()
            .facets
            .is_empty()
    );
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"SECRET_TOKEN\""), "the name is listed");
    assert!(!json.contains("abc-not-for-agents"), "the value is not");
}
