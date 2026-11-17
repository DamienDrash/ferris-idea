//! SPEC-0048 AK-1 – AK-4.

use std::path::{Path, PathBuf};

use idecraft_core::{
    ModuleKind, Workspace, parse_run_configs, read_project, split_pep508, suggest_run_configs,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[test]
fn this_repository_as_cargo_workspace() {
    let root = repo_root();
    let ws = Workspace::open(&root).unwrap();
    let model = read_project(&root, &ws.files().unwrap());
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let top = model.modules.iter().find(|m| m.dir == ".").unwrap();
    assert_eq!(top.kind, ModuleKind::Cargo);
    assert!(
        top.members.contains(&"crates/core".to_owned()),
        "{:?}",
        top.members
    );
    assert!(top.members.contains(&"crates/mcp".to_owned()));
    let core = model
        .modules
        .iter()
        .find(|m| m.dir == "crates/core")
        .unwrap();
    assert_eq!(core.name, "idecraft-core");
    assert_eq!(
        core.version.as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "inherited from [workspace.package]"
    );
    assert_eq!(core.toolchain.as_deref(), Some("rust 1.89"));
    let serde = core
        .dependencies
        .iter()
        .find(|d| d.name == "serde")
        .unwrap();
    assert_eq!(
        (serde.requirement.as_str(), serde.scope.as_str()),
        ("1", "normal"),
        "workspace dependency resolved"
    );
    // The locked version comes from the workspace's Cargo.lock.
    let locked = serde.resolved.as_deref().expect("serde is in Cargo.lock");
    assert!(locked.starts_with("1."), "{locked}");
    assert!(
        core.notes
            .iter()
            .any(|n| n == "locked versions from Cargo.lock"),
        "{:?}",
        core.notes
    );
    let mcp = model
        .modules
        .iter()
        .find(|m| m.dir == "crates/mcp")
        .unwrap();
    let core_dep = mcp
        .dependencies
        .iter()
        .find(|d| d.name == "idecraft-core")
        .unwrap();
    assert_eq!(core_dep.requirement, "path:crates/core");
    assert!(mcp.dependencies.iter().any(|d| d.scope == "dev"));

    // The reference projects in the repository are real npm and Maven projects.
    let react = model
        .modules
        .iter()
        .find(|m| m.dir == "tests/reference-projects/react-basic")
        .unwrap();
    assert_eq!(
        (react.kind, react.name.as_str()),
        (ModuleKind::Npm, "inventory-ui")
    );
    assert!(
        react
            .dependencies
            .iter()
            .any(|d| d.name == "vite" && d.scope == "dev")
    );
    let spring = model
        .modules
        .iter()
        .find(|m| m.dir == "tests/reference-projects/spring-basic")
        .unwrap();
    assert_eq!(spring.name, "lab:inventory-service");
    assert_eq!(spring.toolchain.as_deref(), Some("java 21"));
    assert!(
        spring
            .dependencies
            .iter()
            .any(|d| d.name == "org.postgresql:postgresql")
    );
    // Its parent comes from a repository, so the managed versions are unknown and that is said.
    assert!(
        spring
            .notes
            .iter()
            .any(|n| n.contains("spring-boot-starter-parent 3.5.0 are not available locally")),
        "{:?}",
        spring.notes
    );

    // Suggestions: one build and one test for the Cargo workspace root, not per member.
    let suggestions = suggest_run_configs(&model);
    let names: Vec<&str> = suggestions.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(&names[..2], ["cargo build", "cargo test"]);
    // (The sample projects below `crates/languages/tests/fixtures/` have build files of their own
    // and get their own suggestions; no Cargo suggestion names a member.)
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with("cargo") && n.contains("crates/")),
        "{names:?}"
    );
    assert!(names.contains(&"npm run build (tests/reference-projects/react-basic)"));
}

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("idecraft-project-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, path: &str, text: &str) -> PathBuf {
        let p = self.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn npm_python_maven_go() {
    let d = Dir::new("multi");
    let files = vec![
        d.write(
            "web/package.json",
            r#"{"name": "web", "version": "1.2.0", "engines": {"node": ">=20"},
               "scripts": {"build": "vite build", "test": "vitest"},
               "dependencies": {"react": "^18.2.0"}, "devDependencies": {"vite": "^5.0.0"},
               "peerDependencies": {"react-dom": "^18"}}"#,
        ),
        d.write(
            "api/pyproject.toml",
            "[project]\nname = \"api\"\nversion = \"0.3.0\"\nrequires-python = \">=3.11\"\ndependencies = [\"fastapi>=0.110\", \"uvicorn[standard] ; python_version >= '3.11'\"]\n\n[project.optional-dependencies]\ntest = [\"pytest>=8\"]\n",
        ),
        d.write(
            "legacy/pyproject.toml",
            "[tool.poetry]\nname = \"legacy\"\nversion = \"2.0\"\n\n[tool.poetry.dependencies]\npython = \"^3.9\"\nrequests = \"^2.31\"\n\n[tool.poetry.group.dev.dependencies]\nblack = { version = \"^24\" }\n",
        ),
        d.write("scripts/requirements.txt", "# tools\nclick==8.1.7\n-r other.txt\nrich>=13 # pretty\n"),
        d.write(
            "java/pom.xml",
            r#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1.0</version></parent>
  <artifactId>service</artifactId>
  <packaging>jar</packaging>
  <properties><maven.compiler.release>21</maven.compiler.release></properties>
  <modules><module>core</module></modules>
  <dependencies>
    <dependency><groupId>junit</groupId><artifactId>junit</artifactId><version>4.13.2</version><scope>test</scope></dependency>
    <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId></dependency>
  </dependencies>
</project>"#,
        ),
        d.write(
            "go/go.mod",
            "module example.com/tool\n\ngo 1.22\n\nrequire github.com/spf13/cobra v1.8.0\n\nrequire (\n\tgolang.org/x/sys v0.20.0 // indirect\n\tgithub.com/stretchr/testify v1.9.0\n)\n",
        ),
    ];
    let model = read_project(&d.0, &files);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let by = |dir: &str| model.modules.iter().find(|m| m.dir == dir).unwrap();

    let web = by("web");
    assert_eq!(
        (web.kind, web.toolchain.as_deref()),
        (ModuleKind::Npm, Some("node >=20"))
    );
    assert_eq!(web.tasks["build"], "vite build");
    let scopes: Vec<(&str, &str)> = web
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.scope.as_str()))
        .collect();
    assert_eq!(
        scopes,
        [("react", "normal"), ("vite", "dev"), ("react-dom", "peer")]
    );

    let api = by("api");
    assert_eq!(api.toolchain.as_deref(), Some("python >=3.11"));
    assert_eq!(api.dependencies[0].name, "fastapi");
    assert_eq!(
        api.dependencies[1].requirement,
        "[standard] ; python_version >= '3.11'"
    );
    assert_eq!(
        (
            api.dependencies[2].name.as_str(),
            api.dependencies[2].scope.as_str()
        ),
        ("pytest", "test")
    );

    let legacy = by("legacy");
    assert_eq!(legacy.toolchain.as_deref(), Some("python ^3.9"));
    let names: Vec<(&str, &str)> = legacy
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.scope.as_str()))
        .collect();
    assert_eq!(names, [("requests", "normal"), ("black", "dev")]);

    let scripts = by("scripts");
    assert_eq!(scripts.dependencies.len(), 2);
    assert_eq!(scripts.dependencies[0].requirement, "==8.1.7");

    let java = by("java");
    assert_eq!(java.name, "org.example:service");
    assert_eq!(
        (java.version.as_deref(), java.toolchain.as_deref()),
        (Some("1.0"), Some("java 21"))
    );
    assert_eq!(java.members, ["java/core"]);
    assert_eq!(java.dependencies[0].scope, "test");
    assert_eq!(java.dependencies[1].requirement, "managed");

    let go = by("go");
    assert_eq!(
        (go.name.as_str(), go.toolchain.as_deref()),
        ("example.com/tool", Some("go 1.22"))
    );
    let deps: Vec<(&str, &str)> = go
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.scope.as_str()))
        .collect();
    assert_eq!(
        deps,
        [
            ("github.com/spf13/cobra", "normal"),
            ("golang.org/x/sys", "indirect"),
            ("github.com/stretchr/testify", "normal")
        ]
    );

    // Suggested configurations are valid run configurations.
    let suggestions = suggest_run_configs(&model);
    let names: Vec<&str> = suggestions.iter().map(|c| c.name.as_str()).collect();
    for expected in [
        "npm run build (web)",
        "npm run test (web)",
        "pytest (api)",
        "mvn test (java)",
        "go test (go)",
    ] {
        assert!(names.contains(&expected), "{names:?}");
    }
    #[derive(serde::Serialize)]
    struct RunFile {
        run: Vec<idecraft_core::RunConfig>,
    }
    let toml_text = toml::to_string(&RunFile { run: suggestions }).unwrap();
    assert!(
        parse_run_configs(&toml_text).problems.is_empty(),
        "{toml_text}"
    );
}

#[test]
fn gradle_detection_and_problems() {
    let d = Dir::new("gradle");
    let files = vec![
        d.write(
            "settings.gradle.kts",
            "rootProject.name = \"shop\"\ninclude(\":app\", \":lib:util\")\n",
        ),
        d.write("build.gradle.kts", "plugins { java }\n"),
        d.write("broken/package.json", "{ not json"),
    ];
    let model = read_project(&d.0, &files);
    let shop = model
        .modules
        .iter()
        .find(|m| m.kind == ModuleKind::Gradle)
        .unwrap();
    assert_eq!(shop.name, "shop");
    assert_eq!(shop.members, ["app", "lib/util"]);
    assert!(shop.notes[0].contains("need Gradle"));
    assert_eq!(model.modules.len(), 1, "one Gradle module for both files");
    assert_eq!(model.problems.len(), 1);
    assert!(
        model.problems[0].starts_with("broken/package.json"),
        "{:?}",
        model.problems
    );
}

#[test]
fn pep508_names() {
    assert_eq!(
        split_pep508("Django>=4.2,<5"),
        ("Django".into(), ">=4.2,<5".into())
    );
    assert_eq!(split_pep508("attrs"), ("attrs".into(), "*".into()));
    assert_eq!(
        split_pep508("zope.interface [docs] ==6"),
        ("zope.interface".into(), "[docs] ==6".into())
    );
}

#[test]
fn maven_parent_chain_properties_and_management() {
    let d = Dir::new("maven2");
    let parent = d.write(
        "pom.xml",
        r#"<project>
  <groupId>org.shop</groupId><artifactId>shop-parent</artifactId><version>2.1.0</version>
  <packaging>pom</packaging>
  <modules><module>api</module></modules>
  <properties>
    <maven.compiler.release>17</maven.compiler.release>
    <jackson.version>2.17.1</jackson.version>
    <junit.version>5.10.2</junit.version>
  </properties>
  <dependencyManagement><dependencies>
    <dependency><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId><version>${jackson.version}</version></dependency>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>${junit.version}</version><scope>test</scope></dependency>
  </dependencies></dependencyManagement>
</project>"#,
    );
    let child = d.write(
        "api/pom.xml",
        r#"<project>
  <parent><groupId>org.shop</groupId><artifactId>shop-parent</artifactId><version>2.1.0</version></parent>
  <artifactId>shop-api</artifactId>
  <dependencies>
    <dependency><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId></dependency>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId></dependency>
    <dependency><groupId>${project.groupId}</groupId><artifactId>shop-model</artifactId><version>${project.version}</version></dependency>
    <dependency><groupId>org.unknown</groupId><artifactId>thing</artifactId></dependency>
  </dependencies>
</project>"#,
    );
    let model = read_project(&d.0, &[parent, child]);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let api = model.modules.iter().find(|m| m.dir == "api").unwrap();
    assert_eq!(api.name, "org.shop:shop-api");
    assert_eq!(api.version.as_deref(), Some("2.1.0"));
    assert_eq!(
        api.toolchain.as_deref(),
        Some("java 17"),
        "inherited property"
    );
    let deps: Vec<(&str, &str, &str)> = api
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.requirement.as_str(), d.scope.as_str()))
        .collect();
    assert_eq!(
        deps,
        [
            (
                "com.fasterxml.jackson.core:jackson-databind",
                "2.17.1",
                "normal"
            ),
            ("org.junit.jupiter:junit-jupiter", "5.10.2", "test"),
            ("org.shop:shop-model", "2.1.0", "normal"),
            ("org.unknown:thing", "managed", "normal"),
        ]
    );
    // The parent is local, so no "not available" note.
    assert!(
        !api.notes
            .iter()
            .any(|n| n.contains("not available locally"))
    );
}

#[test]
fn gradle_dependencies_catalog_and_locks() {
    let d = Dir::new("gradle2");
    let settings = d.write(
        "settings.gradle.kts",
        "rootProject.name = \"shop\"\ninclude(\":app\")\n",
    );
    d.write(
        "gradle/libs.versions.toml",
        "[versions]\nguava = \"33.2.0-jre\"\n\n[libraries]\nguava = { module = \"com.google.guava:guava\", version.ref = \"guava\" }\njunit-jupiter = \"org.junit.jupiter:junit-jupiter:5.10.2\"\nslf4j-api = { group = \"org.slf4j\", name = \"slf4j-api\", version = { strictly = \"2.0.13\" } }\n",
    );
    let app = d.write(
        "app/build.gradle.kts",
        r#"plugins { java }
java { toolchain { languageVersion = JavaLanguageVersion.of(21) } }
dependencies {
    implementation(libs.guava)
    implementation(libs.slf4j.api)
    implementation(platform("org.springframework.boot:spring-boot-dependencies:3.3.0"))
    implementation("org.apache.commons:commons-lang3:3.14.0")
    compileOnly("org.projectlombok:lombok:1.18.32")
    runtimeOnly("org.postgresql:postgresql:$pgVersion")
    testImplementation(libs.junit.jupiter)
    implementation(project(":core"))
    implementation(files("libs/local.jar"))
}
"#,
    );
    d.write(
        "app/gradle.lockfile",
        "# lock\ncom.google.guava:guava:33.2.0-jre=compileClasspath,runtimeClasspath\norg.postgresql:postgresql:42.7.3=runtimeClasspath\nempty=\n",
    );
    let groovy = d.write(
        "lib/build.gradle",
        "dependencies {\n    implementation 'com.squareup.okhttp3:okhttp:4.12.0'\n    testImplementation \"junit:junit:4.13.2\"\n}\n",
    );
    let model = read_project(&d.0, &[settings, app, groovy]);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let app = model.modules.iter().find(|m| m.dir == "app").unwrap();
    assert_eq!(app.toolchain.as_deref(), Some("java 21"));
    let deps: Vec<(&str, &str, &str, Option<&str>)> = app
        .dependencies
        .iter()
        .map(|d| {
            (
                d.name.as_str(),
                d.requirement.as_str(),
                d.scope.as_str(),
                d.resolved.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        deps,
        [
            (
                "com.google.guava:guava",
                "33.2.0-jre",
                "normal",
                Some("33.2.0-jre")
            ),
            ("org.slf4j:slf4j-api", "2.0.13", "normal", None),
            (
                "org.springframework.boot:spring-boot-dependencies",
                "3.3.0",
                "normal",
                None
            ),
            ("org.apache.commons:commons-lang3", "3.14.0", "normal", None),
            ("org.projectlombok:lombok", "1.18.32", "provided", None),
            (
                "org.postgresql:postgresql",
                "$pgVersion",
                "runtime",
                Some("42.7.3")
            ),
            ("org.junit.jupiter:junit-jupiter", "5.10.2", "test", None),
            (":core", "project", "normal", None),
        ]
    );
    assert!(
        app.notes
            .iter()
            .any(|n| n.contains("2 declaration(s) not read statically")),
        "{:?}",
        app.notes
    );
    let lib = model.modules.iter().find(|m| m.dir == "lib").unwrap();
    let names: Vec<(&str, &str)> = lib
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.scope.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("com.squareup.okhttp3:okhttp", "normal"),
            ("junit:junit", "test")
        ]
    );
}

#[test]
fn npm_and_python_lock_files() {
    let d = Dir::new("locks");
    let pkg = d.write(
        "web/package.json",
        r#"{"name": "web", "dependencies": {"react": "^19.0.0", "@scope/ui": "~2.1.0"}, "devDependencies": {"vite": "^7.0.0"}}"#,
    );
    d.write(
        "web/package-lock.json",
        r#"{"lockfileVersion": 3, "packages": {"": {"name": "web"}, "node_modules/react": {"version": "19.1.0"}, "node_modules/@scope/ui": {"version": "2.1.4"}, "node_modules/x/node_modules/react": {"version": "18.0.0"}}}"#,
    );
    let py = d.write(
        "api/pyproject.toml",
        "[project]\nname = \"api\"\ndependencies = [\"Flask>=3\", \"zope.interface\"]\n",
    );
    d.write(
        "api/poetry.lock",
        "[[package]]\nname = \"flask\"\nversion = \"3.0.3\"\n\n[[package]]\nname = \"zope-interface\"\nversion = \"6.4\"\n",
    );
    let broken = d.write("bad/package.json", r#"{"name": "bad"}"#);
    d.write("bad/package-lock.json", "{ nope");
    let model = read_project(&d.0, &[pkg, py, broken]);
    let web = model.modules.iter().find(|m| m.dir == "web").unwrap();
    let r: Vec<(&str, Option<&str>)> = web
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.resolved.as_deref()))
        .collect();
    assert!(r.contains(&("react", Some("19.1.0"))), "{r:?}");
    assert!(r.contains(&("@scope/ui", Some("2.1.4"))), "{r:?}");
    assert!(r.contains(&("vite", None)), "not installed: {r:?}");
    let api = model.modules.iter().find(|m| m.dir == "api").unwrap();
    let r: Vec<(&str, Option<&str>)> = api
        .dependencies
        .iter()
        .map(|d| (d.name.as_str(), d.resolved.as_deref()))
        .collect();
    assert_eq!(
        r,
        [("Flask", Some("3.0.3")), ("zope.interface", Some("6.4"))]
    );
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(model.problems[0].starts_with("lock file of bad:"));
}

/// SPEC-0048 v8 behaviour 6a (AK-16): a module of a Spring Boot, Quarkus or Micronaut
/// application gets a configuration that starts it, from its dependencies or Maven plugins, with
/// the timeout of a server; a plain Java module gets none.
#[test]
fn framework_applications_get_a_start_configuration() {
    let fixtures = repo_root().join("crates/languages/tests/fixtures/frameworks");
    let started = |root: &Path, build: &str| -> Vec<(String, Vec<String>, Option<u64>)> {
        suggest_run_configs(&read_project(root, &[root.join(build)]))
            .into_iter()
            .filter(|c| c.timeout.is_some())
            .map(|c| (c.name, c.command, c.timeout))
            .collect()
    };
    let day = Some(idecraft_core::SERVER_TIMEOUT_SECS);
    let cmd = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        started(&fixtures.join("spring-shop"), "pom.xml"),
        [("Spring Boot".into(), cmd(&["mvn", "spring-boot:run"]), day)]
    );
    assert_eq!(
        started(&fixtures.join("quarkus-greeting"), "pom.xml"),
        [("Quarkus dev".into(), cmd(&["mvn", "quarkus:dev"]), day)]
    );
    assert_eq!(
        started(&fixtures.join("micronaut-notes"), "build.gradle"),
        [("Micronaut run".into(), cmd(&["gradle", "run"]), day)]
    );
    assert!(started(&fixtures.join("jakarta-library"), "pom.xml").is_empty());

    let d = Dir::new("frameworks");
    d.write(
        "boot/build.gradle.kts",
        "plugins {\n    id(\"org.springframework.boot\") version \"3.3.4\"\n}\n\
         dependencies {\n    implementation(\"org.springframework.boot:spring-boot-starter-web\")\n}\n",
    );
    d.write(
        "mn/pom.xml",
        "<project><groupId>a</groupId><artifactId>mn</artifactId><version>1</version>\
         <build><plugins><plugin><groupId>io.micronaut.maven</groupId>\
         <artifactId>micronaut-maven-plugin</artifactId></plugin></plugins></build></project>",
    );
    d.write(
        "plain/pom.xml",
        "<project><groupId>a</groupId><artifactId>plain</artifactId><version>1</version></project>",
    );
    assert_eq!(
        started(&d.0.join("boot"), "build.gradle.kts"),
        [("Spring Boot".into(), cmd(&["gradle", "bootRun"]), day)]
    );
    assert_eq!(
        started(&d.0.join("mn"), "pom.xml"),
        [("Micronaut run".into(), cmd(&["mvn", "mn:run"]), day)]
    );
    assert!(started(&d.0.join("plain"), "pom.xml").is_empty());
    // The others keep the default timeout.
    let all = suggest_run_configs(&read_project(
        &fixtures.join("spring-shop"),
        &[fixtures.join("spring-shop/pom.xml")],
    ));
    let names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["mvn compile", "mvn test", "Spring Boot"]);
}
