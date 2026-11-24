//! SPEC-0052 AK-1 – AK-4.

use std::path::{Path, PathBuf};
use std::process::Command;

use ferris_idea_core::{JavaSearch, find_jdks, java_environment, java_requirements};

/// Skips a test whose tool is missing; with `FERRIS_IDEA_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("ferris-idea-java-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("proj")).unwrap();
        Self(p.canonicalize().unwrap())
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A fake JDK home with `bin/java`, optionally `bin/javac`, and a `release` file.
    fn jdk(&self, home: &str, version: &str, vendor: &str, compiler: bool) {
        self.write(&format!("{home}/bin/java"), "");
        if compiler {
            self.write(&format!("{home}/bin/javac"), "");
        }
        let arch = match std::env::consts::ARCH {
            "aarch64" => "aarch64",
            _ => "x86_64",
        };
        self.write(
            &format!("{home}/release"),
            &format!("IMPLEMENTOR=\"{vendor}\"\nJAVA_VERSION=\"{version}\"\nOS_ARCH=\"{arch}\"\n"),
        );
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn jdks_from_all_locations() {
    let s = Scratch::new("find");
    s.jdk(
        "jvms/temurin-21.jdk/Contents/Home",
        "21.0.4",
        "Eclipse Adoptium",
        true,
    );
    s.jdk(
        "jvms/zulu-8.jdk/Contents/Home",
        "1.8.0_412",
        "Azul Systems, Inc.",
        true,
    );
    s.jdk("sdkman/17.0.11-tem", "17.0.11", "Eclipse Adoptium", true);
    std::os::unix::fs::symlink(s.0.join("sdkman/17.0.11-tem"), s.0.join("sdkman/current")).unwrap();
    s.jdk("jre-11", "11.0.23", "Oracle Corporation", false);
    s.jdk("opt/jdk-22", "22.0.1", "Oracle Corporation", true);
    std::fs::create_dir_all(s.0.join("usr/bin")).unwrap();
    std::os::unix::fs::symlink(s.0.join("opt/jdk-22/bin/java"), s.0.join("usr/bin/java")).unwrap();
    let search = JavaSearch {
        root: s.0.join("proj"),
        path_var: Some(s.0.join("usr/bin").into_os_string()),
        java_home: Some(s.0.join("jre-11")),
        jdk_dirs: vec![
            (s.0.join("jvms"), "system".into()),
            (s.0.join("sdkman"), "sdkman".into()),
        ],
        configured: None,
    };
    let found = find_jdks(&search);
    let summary: Vec<(&str, Option<u32>, bool)> = found
        .iter()
        .map(|j| (j.found_by.as_str(), j.feature, j.has_compiler))
        .collect();
    assert_eq!(
        summary,
        [
            ("JAVA_HOME", Some(11), false),
            ("system", Some(21), true),
            ("system", Some(8), true),
            ("sdkman", Some(17), true),
            ("PATH", Some(22), true),
        ]
    );
    assert_eq!(found[2].version.as_deref(), Some("1.8.0_412"));
    assert_eq!(found[2].vendor.as_deref(), Some("Azul Systems, Inc."));
    assert!(found[1].home.ends_with("temurin-21.jdk/Contents/Home"));

    // No project requirement: JAVA_HOME wins, and it is a runtime only.
    let env = java_environment(&search);
    assert_eq!(env.reason.as_deref(), Some("JAVA_HOME"));
    assert!(
        env.notes[0].ends_with("is a runtime without javac"),
        "{:?}",
        env.notes
    );

    // Maven release 17: the exact feature release is chosen.
    s.write(
        "proj/pom.xml",
        "<project><properties><maven.compiler.release>17</maven.compiler.release></properties></project>",
    );
    let env = java_environment(&search);
    let chosen = &env.jdks[env.selected.unwrap()];
    assert_eq!(chosen.feature, Some(17));
    assert_eq!(env.reason.as_deref(), Some("matches the project's Java 17"));
    assert_eq!(env.compatible, Some(true));

    // Gradle toolchain 20: no exact JDK, the closest newer one (21) is chosen.
    std::fs::remove_file(s.0.join("proj/pom.xml")).unwrap();
    s.write(
        "proj/build.gradle.kts",
        "java {\n    toolchain {\n        languageVersion = JavaLanguageVersion.of(20)\n    }\n}\n",
    );
    let env = java_environment(&search);
    assert_eq!(env.jdks[env.selected.unwrap()].feature, Some(21));
    assert_eq!(
        env.reason.as_deref(),
        Some("is the closest newer JDK for the project's Java 20")
    );

    // Java 25: nothing can build it.
    s.write(
        "proj/build.gradle.kts",
        "java { toolchain { languageVersion = JavaLanguageVersion.of(25) } }",
    );
    let env = java_environment(&search);
    assert_eq!(env.problems, ["no installed JDK can build Java 25"]);
    assert_eq!(env.compatible, Some(false));
}

#[test]
fn requirements_from_project_files() {
    let s = Scratch::new("req");
    let root = s.0.join("proj");
    s.write(
        "proj/pom.xml",
        r#"<project>
  <properties><java.version>21</java.version></properties>
  <build><plugins><plugin>
    <artifactId>maven-compiler-plugin</artifactId>
    <configuration><source>1.8</source></configuration>
  </plugin></plugins></build>
</project>"#,
    );
    s.write(
        "proj/build.gradle",
        "sourceCompatibility = JavaVersion.VERSION_17\ntargetCompatibility = '17'\n",
    );
    s.write("proj/.java-version", "11.0\n");
    s.write("proj/.sdkmanrc", "# sdk\njava=21.0.2-tem\n");
    s.write(
        "proj/.idea/misc.xml",
        r#"<project><component name="ProjectRootManager" languageLevel="JDK_1_8" project-jdk-name="1.8" /></project>"#,
    );
    let req = java_requirements(&root);
    let summary: Vec<(u32, &str, &str)> = req
        .iter()
        .map(|r| (r.feature, r.source.as_str(), r.detail.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (21, "pom.xml", "java.version 21"),
            (21, ".sdkmanrc", "java 21.0.2-tem"),
            (17, "build.gradle", "sourceCompatibility 17"),
            (17, "build.gradle", "targetCompatibility 17"),
            (11, ".java-version", "version 11.0"),
            (8, "pom.xml", "maven-compiler-plugin source 1.8"),
            (8, ".idea/misc.xml", "languageLevel 1.8"),
        ]
    );
    let env = java_environment(&JavaSearch {
        root,
        ..JavaSearch::default()
    });
    assert!(
        env.problems[0]
            .starts_with("the project states different Java versions: java.version 21 (pom.xml)"),
        "{:?}",
        env.problems
    );
}

/// The installed JDK: the version read from its `release` file equals what the running JVM
/// reports, and its compiler builds and runs a program.
#[test]
fn real_jdk() {
    let s = Scratch::new("real");
    let env = java_environment(&JavaSearch::from_env(&s.0.join("proj")));
    let Some(chosen) = env
        .selected
        .map(|i| env.jdks[i].clone())
        .filter(|j| j.has_compiler)
    else {
        skip("no JDK installed");
        return;
    };
    s.write(
        "proj/Version.java",
        "public class Version { public static void main(String[] a) { System.out.println(System.getProperty(\"java.version\")); } }",
    );
    let home = Path::new(&chosen.home);
    let ok = Command::new(home.join("bin/javac"))
        .arg("Version.java")
        .current_dir(s.0.join("proj"))
        .status()
        .unwrap();
    assert!(ok.success());
    let out = Command::new(home.join("bin/java"))
        .args(["-cp", ".", "Version"])
        .current_dir(s.0.join("proj"))
        .output()
        .unwrap();
    let real = String::from_utf8(out.stdout).unwrap();
    assert_eq!(chosen.version.as_deref(), Some(real.trim()), "{chosen:?}");
}
