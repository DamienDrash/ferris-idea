//! SPEC-0115 AK-2: the Spring model of the reference project `tests/reference-projects/spring-basic`
//! finds its beans, resolves the repository injected into the controller, leaves the auto-configured
//! `JdbcClient` alone, reads `application.properties` and reports nothing.

use std::path::{Path, PathBuf};

#[test]
fn reference_project_has_beans_and_no_problems() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference-projects/spring-basic");
    let files: Vec<PathBuf> = idecraft_core::Workspace::open(&root)
        .unwrap()
        .files()
        .unwrap();
    let m = idecraft_languages::spring::model(&root, &files);
    let mut beans: Vec<(&str, &str)> = m
        .beans
        .iter()
        .map(|b| (b.name.as_str(), b.kind.as_str()))
        .collect();
    beans.sort();
    assert_eq!(
        beans,
        [
            ("inventoryApplication", "SpringBootApplication"),
            ("stockController", "RestController"),
            ("stockRepository", "Repository")
        ]
    );
    let repo = m
        .injections
        .iter()
        .find(|i| i.type_name == "StockRepository")
        .expect("controller injects the repository");
    assert_eq!(repo.candidates.len(), 1);
    assert!(
        m.injections
            .iter()
            .any(|i| i.type_name == "JdbcClient" && !i.own_type)
    );
    assert!(m.properties.iter().any(|p| p.key == "server.port"));
    assert!(m.problems().is_empty(), "{:?}", m.problems());
}

/// SPEC-0134/AK-6 (v2): a Quarkus project as code.quarkus.io writes it — no `io.quarkus` import,
/// the BOM through properties in `pom.xml` — is Quarkus, read through the workspace's file list.
#[test]
fn quarkus_project_without_quarkus_imports() {
    let root = std::env::temp_dir().join(format!("idecraft-quarkus-pom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let java = root.join("src/main/java/dev/ht");
    std::fs::create_dir_all(&java).unwrap();
    std::fs::create_dir_all(root.join("src/main/resources")).unwrap();
    std::fs::write(
        root.join("pom.xml"),
        "<project>\n  <properties>\n    <quarkus.platform.group-id>io.quarkus.platform</quarkus.platform.group-id>\n  </properties>\n  <dependencies>\n    <dependency>\n      <groupId>io.quarkus</groupId>\n      <artifactId>quarkus-rest</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n",
    )
    .unwrap();
    std::fs::write(
        java.join("Greeter.java"),
        "package dev.ht;\npublic interface Greeter {}\n",
    )
    .unwrap();
    for name in ["PlainGreeter", "LoudGreeter"] {
        std::fs::write(
            java.join(format!("{name}.java")),
            format!(
                "package dev.ht;\nimport jakarta.enterprise.context.ApplicationScoped;\n@ApplicationScoped\npublic class {name} implements Greeter {{}}\n"
            ),
        )
        .unwrap();
    }
    std::fs::write(
        java.join("GreetingService.java"),
        "package dev.ht;\nimport jakarta.enterprise.context.ApplicationScoped;\nimport jakarta.inject.Inject;\nimport org.eclipse.microprofile.config.inject.ConfigProperty;\n@ApplicationScoped\npublic class GreetingService {\n  @Inject Greeter greeter;\n  @ConfigProperty(name = \"greeting.suffix\") String suffix;\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main/resources/application.properties"), "\n").unwrap();
    let files: Vec<PathBuf> = idecraft_core::Workspace::open(&root)
        .unwrap()
        .files()
        .unwrap();
    let m = idecraft_languages::spring::model(&root, &files);
    assert_eq!(m.frameworks, ["Quarkus", "Jakarta EE"]);
    assert_eq!(m.label(), "Quarkus");
    assert_eq!(m.problems().len(), 2, "{:?}", m.problems());
    let _ = std::fs::remove_dir_all(&root);
}
