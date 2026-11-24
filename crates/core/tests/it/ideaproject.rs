//! SPEC-0049 AK-1 – AK-5, v3 AK-8 – AK-12, v4 AK-13, v7 AK-15.

use std::path::{Path, PathBuf};

use ferris_idea_core::{ModuleSdk, RunKind, SourceKind, idea_run_configs, read_idea_project};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("ferris-idea-idea-{tag}-{}", std::process::id()));
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
    // Deliberately malformed: proves the database tool's personal file is never opened (v7: the
    // shared `dataSources.xml` is read, behaviour 18). Of workspace.xml
    // only the RunManager counts (v3 behaviour 7), so another component adds nothing.
    s.write(
        ".idea/workspace.xml",
        "<project version=\"4\"><component name=\"PropertiesComponent\"><property name=\"x\" value=\"<\" /></component></project>",
    );
    s.write(".idea/dataSources.local.xml", "<project");
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
    assert_eq!(p.not_read, ["dataSources.local.xml", "workspace.xml"]);
    assert!(p.data_sources.is_empty());

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
    use ferris_idea_core::buildtools::FolderRole;
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
    let roles = ferris_idea_core::idea_folder_roles(&root, &p);
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
    let oroles = ferris_idea_core::idea_folder_roles(&oroot, &op);
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
    // Python keeps Ferris IDEA's interpreter choice; the others are used as written.
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
    let mut all = ferris_idea_core::parse_run_configs(
        "[[run]]\nname = \"Run plain\"\ncommand = [\"echo\", \"mine\"]\n",
    );
    let added = ferris_idea_core::add_detected(&mut all, configs);
    assert_eq!(
        all.configs[0].command,
        ["echo", "mine"],
        "run.toml wins a name"
    );
    assert!(!added.contains(&"Run plain".to_owned()));
    assert_eq!(added.len(), 9);
    assert!(!root.join(".ferris-idea").exists(), "nothing is written");
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

/// v4 AK-13 (behaviour 17): the project's code style is read as written; v6: whether it applies.
#[test]
fn code_style_of_the_project() {
    let s = sample("codestyle");
    s.write(
        ".idea/codeStyles/codeStyleConfig.xml",
        r#"<component name="ProjectCodeStyleConfiguration">
  <state>
    <option name="USE_PER_PROJECT_SETTINGS" value="true" />
  </state>
</component>"#,
    );
    s.write(
        ".idea/codeStyles/Project.xml",
        r#"<component name="ProjectCodeStyleConfiguration">
  <code_scheme name="Project" version="173">
    <option name="RIGHT_MARGIN" value="100" />
    <codeStyleSettings language="JAVA">
      <option name="RIGHT_MARGIN" value="140" />
      <indentOptions>
        <option name="INDENT_SIZE" value="2" />
        <option name="CONTINUATION_INDENT_SIZE" value="4" />
        <option name="TAB_SIZE" value="2" />
      </indentOptions>
    </codeStyleSettings>
    <codeStyleSettings language="Makefile">
      <indentOptions>
        <option name="USE_TAB_CHARACTER" value="true" />
        <option name="INDENT_SIZE" value="many" />
      </indentOptions>
    </codeStyleSettings>
  </code_scheme>
</component>"#,
    );
    let p = read_idea_project(&s.0).unwrap();
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let cs = p.code_style.expect("code style");
    assert!(cs.per_project);
    // v6: switched on and read, so it applies (SPEC-0150 v6 behaviour 8).
    assert!(cs.applied && cs.applies());
    assert_eq!(cs.preferred_scheme, None);
    assert_eq!(cs.file.as_deref(), Some(".idea/codeStyles/Project.xml"));
    assert_eq!(cs.right_margin, Some(100));
    assert_eq!(cs.languages.len(), 2);
    let java = &cs.languages[0];
    assert_eq!(java.language, "JAVA");
    assert_eq!(java.right_margin, Some(140));
    assert_eq!(
        (
            java.indent,
            java.continuation_indent,
            java.tab_size,
            java.use_tabs
        ),
        (Some(2), Some(4), Some(2), None)
    );
    let make = &cs.languages[1];
    assert_eq!((make.indent, make.use_tabs), (None, Some(true)));

    // A shared IDE scheme instead: only its name, nothing outside the project is read.
    s.write(
        ".idea/codeStyles/codeStyleConfig.xml",
        r#"<component name="ProjectCodeStyleConfiguration">
  <state>
    <option name="PREFERRED_PROJECT_CODE_STYLE" value="Team" />
  </state>
</component>"#,
    );
    std::fs::remove_file(s.0.join(".idea/codeStyles/Project.xml")).unwrap();
    let cs = read_idea_project(&s.0).unwrap().code_style.unwrap();
    assert!(!cs.per_project);
    assert!(!cs.applied);
    assert_eq!(cs.preferred_scheme.as_deref(), Some("Team"));
    assert_eq!((cs.file, cs.languages.len()), (None, 0));

    // A malformed scheme is a problem naming the file; without the folder there is no code style.
    s.write(".idea/codeStyles/Project.xml", "<component");
    let p = read_idea_project(&s.0).unwrap();
    assert!(
        p.problems
            .iter()
            .any(|e| e.starts_with(".idea/codeStyles/Project.xml")),
        "{:?}",
        p.problems
    );
    std::fs::remove_dir_all(s.0.join(".idea/codeStyles")).unwrap();
    assert_eq!(read_idea_project(&s.0).unwrap().code_style, None);
}

/// SPEC-0049 v9 AK-18 (CS2 in OBS-0109): an `.ipr` project keeps its code style in one
/// `ProjectCodeStyleConfiguration` component of the `.ipr`, switch and scheme together; it reads as
/// the folder format does, with `file` naming the `.ipr`. Without the component there is none.
#[test]
fn code_style_of_an_ipr_project() {
    let s = Scratch::new("ipr-codestyle");
    s.write(
        "shop.ipr",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectCodeStyleConfiguration">
    <code_scheme name="Project" version="173">
      <option name="RIGHT_MARGIN" value="100" />
      <codeStyleSettings language="JAVA">
        <indentOptions>
          <option name="INDENT_SIZE" value="2" />
        </indentOptions>
      </codeStyleSettings>
    </code_scheme>
    <state>
      <option name="USE_PER_PROJECT_SETTINGS" value="true" />
    </state>
  </component>
  <component name="ProjectModuleManager">
    <modules />
  </component>
</project>"#,
    );
    let p = read_idea_project(&s.0).expect("an .ipr is a project");
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let cs = p.code_style.expect("code style of the .ipr");
    assert!(cs.per_project);
    assert_eq!(cs.preferred_scheme, None);
    assert_eq!(cs.file.as_deref(), Some("shop.ipr"));
    assert_eq!(cs.right_margin, Some(100));
    assert!(
        !cs.applied,
        "an .ipr code style is reported, not applied (gap CS3)"
    );
    assert_eq!(cs.languages.len(), 1);
    assert_eq!(cs.languages[0].language, "JAVA");
    assert_eq!(cs.languages[0].indent, Some(2));
    assert_eq!(cs.languages[0].right_margin, None);

    // The fixture without the component: no code style.
    let plain = read_idea_project(&fixture("ipr-basic")).unwrap();
    assert_eq!(plain.code_style, None);
}

/// SPEC-0049 v5 AK-14: HTTP request configurations of the reference become configurations with
/// `http`; a position alone, a missing name in a file of several requests and a file outside the
/// project are listed with their reason.
#[test]
fn http_request_configurations_are_translated() {
    use ferris_idea_core::HttpRun;
    let root = fixture("http-configs");
    let p = read_idea_project(&root).unwrap();
    let names: Vec<&str> = p
        .run_configurations
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "api | all",
            "one",
            "api | Liste",
            "api | by position",
            "api | no name",
            "outside"
        ]
    );
    let http = |name: &str| {
        p.run_configurations
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .http
            .clone()
    };
    assert_eq!(
        http("api | Liste"),
        Some(HttpRun {
            file: "http/api.http".into(),
            request: Some("Liste".into()),
            environment: Some("local".into()),
        })
    );
    assert_eq!(
        http("api | all"),
        Some(HttpRun {
            file: "http/api.http".into(),
            request: None,
            environment: Some("staging".into()),
        })
    );
    assert_eq!(
        http("one"),
        Some(HttpRun {
            file: "http/one.http".into(),
            request: None,
            environment: None,
        }),
        "keys from options, the only request of the file"
    );
    let all = p
        .run_configurations
        .iter()
        .find(|r| r.name == "api | all")
        .unwrap();
    assert_eq!(all.folder.as_deref(), Some("HTTP"));
    for (name, reason) in [
        ("api | by position", "by position only"),
        ("api | no name", "by position only"),
        ("outside", "lies outside the project"),
    ] {
        let r = p
            .run_configurations
            .iter()
            .find(|r| r.name == name)
            .unwrap();
        assert!(r.http.is_none() && r.config.is_none(), "{name}");
        assert!(
            r.reason.as_deref().is_some_and(|x| x.contains(reason)),
            "{name}: {:?}",
            r.reason
        );
        assert!(
            p.problems
                .iter()
                .any(|x| x.contains(&format!("run configuration {name}: "))),
            "{name}: {:?}",
            p.problems
        );
    }
    let configs = idea_run_configs(&root);
    assert_eq!(configs.len(), 3);
    for c in &configs {
        assert!(c.command.is_empty() && c.http.is_some(), "{c:?}");
    }
    // The translated configurations are valid configurations of SPEC-0047 v6.
    let text = toml::to_string(&std::collections::BTreeMap::from([("run", &configs)])).unwrap();
    let parsed = ferris_idea_core::parse_run_configs(&text);
    assert!(parsed.problems.is_empty(), "{:?}\n{text}", parsed.problems);
    assert!(!root.join(".ferris-idea").exists(), "nothing is written");
}

/// Copies the fixture `name` into a scratch folder, so a test can add a `run.toml` beside it.
fn fixture_copy(name: &str, tag: &str) -> Scratch {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let target = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &target);
            } else {
                std::fs::copy(e.path(), target).unwrap();
            }
        }
    }
    let s = Scratch::new(tag);
    copy(&fixture(name), &s.0);
    s
}

/// SPEC-0047 v7 AK-17: what `run_configuration` can start is `run.toml` followed by the
/// translated configurations of the reference that it does not shadow; the reference's
/// configuration a name stands for is found, translated or not.
#[test]
fn startable_configurations_add_the_idea_ones() {
    let s = fixture_copy("run-configs", "startable");
    s.write(
        ".ferris-idea/run.toml",
        "[[run]]\nname = \"Run plain\"\ncommand = [\"echo\", \"mine\"]\n\n[[run]]\nname = \"own\"\ncommand = [\"echo\", \"own\"]\n",
    );
    let (configs, idea) = ferris_idea_core::startable_run_configs(&s.0, "Run plain");
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    assert_eq!(configs.configs[0].name, "Run plain");
    assert_eq!(
        configs.configs[0].command,
        ["echo", "mine"],
        "run.toml wins"
    );
    assert_eq!(configs.configs[1].name, "own");
    assert_eq!(
        configs.configs.len(),
        2 + 9,
        "the nine translated ones not shadowed follow"
    );
    assert!(
        idea.is_none(),
        "run.toml holds the name: no reference origin"
    );

    let (_, idea) = ferris_idea_core::startable_run_configs(&s.0, "personal py");
    let idea = idea.expect("a configuration of workspace.xml");
    assert_eq!(idea.source, ".idea/workspace.xml");
    assert!(idea.config.is_some());

    let (configs, idea) = ferris_idea_core::startable_run_configs(&s.0, "npm start");
    let idea = idea.expect("the untranslated one is found");
    assert!(idea.config.is_none() && idea.reason.is_some());
    assert_eq!(idea.source, ".idea/runConfigurations/Npm.xml");
    assert!(
        configs.configs.iter().all(|c| c.name != "npm start"),
        "an untranslated one is never startable"
    );

    let (_, idea) = ferris_idea_core::startable_run_configs(&s.0, "nothing of that name");
    assert!(idea.is_none());
}

/// SPEC-0047 v7 AK-17: only a configuration the reference's files alone hold is read only, and
/// the refusal names its source; nothing is written.
#[test]
fn idea_origin_and_read_only_refusal() {
    let root = fixture("run-configs");
    let why = ferris_idea_core::idea_read_only_refusal(&root, "Run plain", false)
        .expect("a reference configuration is read only");
    assert!(
        why.contains(".idea/runConfigurations/App_plain.xml") && why.contains("read only"),
        "{why}"
    );
    assert!(
        why.contains(".ferris-idea/run.toml"),
        "names the way out: {why}"
    );
    assert_eq!(
        ferris_idea_core::idea_read_only_refusal(&root, "Run plain", true),
        None,
        "run.toml holds it too: its own configuration is changed"
    );
    assert_eq!(
        ferris_idea_core::idea_read_only_refusal(&root, "not there", false),
        None
    );
    let s = Scratch::new("no-idea");
    assert_eq!(
        ferris_idea_core::idea_read_only_refusal(&s.0, "Run plain", false),
        None,
        "no reference project"
    );
    assert!(!root.join(".ferris-idea").exists(), "nothing is written");
}

/// SPEC-0049 AK-15 (v7, behaviour 18): the shared data sources, cleaned; the personal part of the
/// database tool is never opened.
#[test]
fn data_sources_of_the_project() {
    const MARK: &str = "MARK-never-read-7f3c";
    let s = sample("data-sources");
    let urls = [
        (
            "inventory",
            "sqlite.xerial",
            "jdbc:sqlite:$PROJECT_DIR$/inventory.sqlite",
        ),
        (
            "shop",
            "postgresql",
            "jdbc:postgresql://bob:hunter2@db.example:5433/shop?password=hunter3&amp;sslmode=disable&amp;token=t0k3n",
        ),
        ("orders", "mysql.8", "jdbc:mysql://localhost/orders"),
        ("maria", "mariadb", "jdbc:mariadb://127.0.0.1:3307/m"),
        (
            "mssql",
            "sqlserver.ms",
            "jdbc:sqlserver://srv:1433;databaseName=sales;password=hunter4;encrypt=true",
        ),
        ("oracle", "oracle.19", "jdbc:oracle:thin:@h:1521/x"),
        (
            "elsewhere",
            "sqlite.xerial",
            "jdbc:sqlite:$PROJECT_DIR$/../elsewhere.sqlite",
        ),
        (
            "named",
            "sqlserver.ms",
            "jdbc:sqlserver://srv\\SQLEXPRESS;database=x",
        ),
    ];
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"DataSourceManagerImpl\" format=\"xml\" multifile-model=\"true\">\n",
    );
    for (i, (name, driver, url)) in urls.iter().enumerate() {
        xml.push_str(&format!(
            "    <data-source source=\"LOCAL\" name=\"{name}\" uuid=\"00000000-0000-0000-0000-00000000000{i}\">\n      <driver-ref>{driver}</driver-ref>\n      <synchronize>true</synchronize>\n      <jdbc-driver>some.Driver</jdbc-driver>\n      <jdbc-url>{url}</jdbc-url>\n      <working-dir>$ProjectFileDir$</working-dir>\n    </data-source>\n"
        ));
    }
    xml.push_str("  </component>\n</project>\n");
    s.write(".idea/dataSources.xml", &xml);
    // The personal part holds the marker; neither may be opened.
    s.write(
        ".idea/dataSources.local.xml",
        &format!("<project><component name=\"dataSourceStorageLocal\"><data-source name=\"{MARK}\"><user-name>{MARK}</user-name></data-source></component></project>"),
    );
    s.write(".idea/dataSources/x.xml", MARK);
    let p = read_idea_project(&s.0).unwrap();
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let shown = format!("{p:?}");
    for secret in [MARK, "hunter", "t0k3n", "bob"] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }
    assert_eq!(
        p.not_read,
        ["dataSources", "dataSources.local.xml", "workspace.xml"]
    );
    let ds = &p.data_sources;
    assert_eq!(ds.len(), 8);
    let by = |n: &str| ds.iter().find(|d| d.name == n).unwrap();

    let sqlite = by("inventory");
    assert!(sqlite.offered(), "{sqlite:?}");
    assert_eq!(sqlite.kind.as_deref(), Some("sqlite"));
    assert_eq!(sqlite.driver.as_deref(), Some("sqlite.xerial"));
    assert_eq!(sqlite.file.as_deref(), Some("inventory.sqlite"));
    let url = sqlite.url.as_deref().unwrap();
    assert!(
        url.starts_with("jdbc:sqlite:") && url.ends_with("/inventory.sqlite") && !url.contains('$'),
        "{url}"
    );

    let pg = by("shop");
    assert!(pg.offered(), "{pg:?}");
    assert_eq!(pg.kind.as_deref(), Some("postgresql"));
    assert_eq!(
        pg.url.as_deref(),
        Some("jdbc:postgresql://db.example:5433/shop?sslmode=disable")
    );
    assert_eq!(pg.removed, ["user info", "password", "token"]);
    assert_eq!(
        (pg.host.as_deref(), pg.port, pg.database.as_deref()),
        (Some("db.example"), Some(5433), Some("shop"))
    );

    let my = by("orders");
    assert_eq!(
        (
            my.kind.as_deref(),
            my.host.as_deref(),
            my.port,
            my.database.as_deref()
        ),
        (Some("mysql"), Some("localhost"), None, Some("orders"))
    );
    assert_eq!(by("maria").kind.as_deref(), Some("mariadb"));
    assert_eq!(by("maria").port, Some(3307));

    let ms = by("mssql");
    assert!(ms.offered(), "{ms:?}");
    assert_eq!(ms.kind.as_deref(), Some("sqlserver"));
    assert_eq!(ms.database.as_deref(), Some("sales"));
    assert_eq!(ms.removed, ["password"]);
    assert_eq!(
        ms.url.as_deref(),
        Some("jdbc:sqlserver://srv:1433;databaseName=sales;encrypt=true")
    );

    for (name, reason) in [
        ("oracle", "driver `oracle.19` is not supported"),
        ("elsewhere", "the file lies outside the project"),
        ("named", "named SQL Server instances are not supported"),
    ] {
        let d = by(name);
        assert!(!d.offered());
        assert_eq!(d.reason.as_deref(), Some(reason), "{d:?}");
    }

    // The window's reader gives the same list.
    assert_eq!(&ferris_idea_core::read_idea_data_sources(&s.0), ds);

    // A malformed file is a problem; the rest is still read.
    s.write(".idea/dataSources.xml", "<project><component");
    let bad = read_idea_project(&s.0).unwrap();
    assert!(
        bad.problems
            .iter()
            .any(|x| x.starts_with(".idea/dataSources.xml: ")),
        "{:?}",
        bad.problems
    );
    assert!(bad.data_sources.is_empty());
    assert_eq!(bad.modules.len(), 2);

    // A symbolic link is not followed.
    #[cfg(unix)]
    {
        std::fs::remove_file(s.0.join(".idea/dataSources.xml")).unwrap();
        std::os::unix::fs::symlink(
            s.0.join(".idea/dataSources.local.xml"),
            s.0.join(".idea/dataSources.xml"),
        )
        .unwrap();
        let linked = read_idea_project(&s.0).unwrap();
        assert!(linked.data_sources.is_empty());
        assert!(
            linked
                .problems
                .contains(&".idea/dataSources.xml: a symbolic link, not followed".to_owned()),
            "{:?}",
            linked.problems
        );
        assert!(!format!("{linked:?}").contains(MARK));
    }
}
