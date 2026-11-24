//! SPEC-0113 v2 AK-6 (hand test 40, D26): Change Signature of an overloaded Java method through
//! jdtls, whose references cover the whole qualified call `r.addiere(1, "abc")`. A test binary of
//! its own, because the pool is switched on for the whole process. Skipped without `jdtls`.

use std::time::Duration;

use ferris_idea_languages::pool;
use ferris_idea_languages::rename::apply_edits;
use ferris_idea_languages::signature::plan_change_signature_in;

#[tokio::test]
async fn qualified_calls_of_an_overload_follow() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-java-sig-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(
        root.join("pom.xml"),
        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>sig</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>\n",
    )
    .unwrap();
    let file = root.join("src/main/java/demo/Rechner.java");
    let text = "package demo;\n\npublic class Rechner {\n    int addiere(int a, String text) {\n        return a + text.length();\n    }\n\n    int addiere(int a) {\n        return a;\n    }\n\n    public static void main(String[] args) {\n        Rechner r = new Rechner();\n        System.out.println(r.addiere(1, \"abc\"));\n        System.out.println(r.addiere(2, \"x\") + r.addiere(3));\n    }\n}\n";
    std::fs::write(&file, text).unwrap();
    pool::enable();
    let wait = Duration::from_secs(120);
    pool::sync_text(&root, &file, text).await.unwrap();
    let plan = plan_change_signature_in(
        &root,
        &file,
        Some(text),
        3,
        9,
        Some("String text, int a, int faktor = 1"),
        wait,
    )
    .await
    .unwrap();
    assert!(plan.skipped.is_empty(), "{plan:?}");
    assert_eq!(plan.files.len(), 1, "{plan:?}");
    let changed = apply_edits(text, &plan.files[0].edits).unwrap();
    assert!(
        changed.contains("int addiere(String text, int a, int faktor) {"),
        "{changed}"
    );
    assert!(changed.contains("r.addiere(\"abc\", 1, 1)"), "{changed}");
    assert!(
        changed.contains("r.addiere(\"x\", 2, 1) + r.addiere(3)"),
        "{changed}"
    );
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
