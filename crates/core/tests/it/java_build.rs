//! SPEC-0097 AK-2: a Maven and a Gradle project are detected, built and tested with the real tools;
//! the JUnit reports of the run become one result tree. Skipped when the tool is not installed.

use std::path::{Path, PathBuf};

use ferris_idea_core::{
    RunConfigs, RunKind, TestStatus, Workspace, read_project, run_config, run_one,
    suggest_run_configs, test_method_config,
};

const POM: &str = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>dev.ferrisidea</groupId>
  <artifactId>calc</artifactId>
  <version>1.0</version>
  <properties>
    <maven.compiler.release>17</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencies>
    <dependency>
      <groupId>org.junit.jupiter</groupId>
      <artifactId>junit-jupiter</artifactId>
      <version>5.10.2</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-surefire-plugin</artifactId>
        <version>3.2.5</version>
        <configuration><testFailureIgnore>true</testFailureIgnore></configuration>
      </plugin>
    </plugins>
  </build>
</project>
"#;

const CALC: &str =
    "package calc;\npublic class Calc {\n    public static int twice(int x) { return x * 2; }\n}\n";

const TEST: &str = "package calc;\nimport static org.junit.jupiter.api.Assertions.assertEquals;\nimport org.junit.jupiter.api.Test;\nclass CalcTest {\n    @Test void twiceDoubles() { assertEquals(4, Calc.twice(2)); }\n    @Test void twiceIsWrongHere() { assertEquals(5, Calc.twice(2)); }\n}\n";

fn has(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn project(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("ferris-idea-java-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("src/main/java/calc")).unwrap();
    std::fs::create_dir_all(p.join("src/test/java/calc")).unwrap();
    std::fs::write(p.join("src/main/java/calc/Calc.java"), CALC).unwrap();
    std::fs::write(p.join("src/test/java/calc/CalcTest.java"), TEST).unwrap();
    p
}

fn detected(root: &Path) -> RunConfigs {
    let files = Workspace::open(root).unwrap().files().unwrap();
    RunConfigs {
        configs: suggest_run_configs(&read_project(root, &files)),
        problems: Vec::new(),
    }
}

fn check_tests(root: &Path, configs: &RunConfigs, name: &str) {
    let config = configs.configs.iter().find(|c| c.name == name).unwrap();
    assert_eq!(config.kind, RunKind::Test);
    let runs = run_config(root, configs, name).unwrap();
    let outcome = runs.last().unwrap();
    let report = outcome.report.as_ref().unwrap_or_else(|| {
        panic!(
            "no report: {:?}\n{}\n{}",
            outcome.report_error, outcome.stdout, outcome.stderr
        )
    });
    assert_eq!(report.totals.tests, 2, "{name}");
    assert_eq!(report.totals.failed, 1, "{name}");
    let failed: Vec<_> = report
        .suites
        .iter()
        .flat_map(|s| &s.tests)
        .filter(|t| t.status == TestStatus::Failed)
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        failed.iter().any(|n| n.contains("twiceIsWrongHere")),
        "{failed:?}"
    );
}

/// SPEC-0122 AK-2: one test method runs alone, and the tree has only it.
fn check_one_method(root: &Path) {
    let files = Workspace::open(root).unwrap().files().unwrap();
    let model = read_project(root, &files);
    let config = test_method_config(
        &model,
        "src/test/java/calc/CalcTest.java",
        TEST,
        "twiceDoubles",
    )
    .unwrap();
    assert_eq!(config.name, "CalcTest.twiceDoubles");
    let outcome = run_one(root, &config);
    let report = outcome.report.as_ref().unwrap_or_else(|| {
        panic!(
            "no report: {:?}\n{}\n{}",
            outcome.report_error, outcome.stdout, outcome.stderr
        )
    });
    assert_eq!(
        (report.totals.tests, report.totals.failed),
        (1, 0),
        "{:?}",
        config.command
    );
}

#[test]
fn maven_compile_and_junit_tests() {
    if !has("mvn") {
        eprintln!("skipped: mvn is not installed");
        return;
    }
    let root = project("maven");
    std::fs::write(root.join("pom.xml"), POM).unwrap();
    let configs = detected(&root);
    let names: Vec<_> = configs.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["mvn compile", "mvn test"]);
    let compiled = run_config(&root, &configs, "mvn compile").unwrap();
    assert_eq!(
        compiled.last().unwrap().exit_code,
        Some(0),
        "{:?}",
        compiled.last().unwrap().stdout
    );
    check_tests(&root, &configs, "mvn test");
    check_one_method(&root);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn gradle_build_and_junit_tests() {
    if !has("gradle") {
        eprintln!("skipped: gradle is not installed");
        return;
    }
    let root = project("gradle");
    std::fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"calc\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("build.gradle.kts"),
        "plugins { java }\nrepositories { mavenCentral() }\ndependencies {\n    testImplementation(\"org.junit.jupiter:junit-jupiter:5.10.2\")\n    testRuntimeOnly(\"org.junit.platform:junit-platform-launcher\")\n}\ntasks.test { useJUnitPlatform(); ignoreFailures = true }\n",
    )
    .unwrap();
    let configs = detected(&root);
    let names: Vec<_> = configs.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["gradle build", "gradle test"]);
    assert_eq!(configs.configs[0].command[0], "gradle", "no wrapper here");
    check_tests(&root, &configs, "gradle test");
    check_one_method(&root);
    let _ = std::fs::remove_dir_all(&root);
}
