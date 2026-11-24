//! SPEC-0122 v4 AK-11 and SPEC-0291 v4 AK-2c (OBS-0027 L17, L18, L22) with a real jdtls: Delegate
//! Methods, the import hint and the hints before the only argument of a constructor, on a small
//! project with a class that holds another class in a field. Skipped without `jdtls`.

use std::path::Path;
use std::time::{Duration, Instant};

use ferris_idea_languages::java_generate::{
    add_delegate_methods_in, delegate_fields_in, import_candidates_at, unresolved_name,
};

const DEADLINE: Duration = Duration::from_secs(300);

fn write(root: &Path, name: &str, text: &str) {
    std::fs::write(root.join(format!("src/main/java/shop/{name}.java")), text).unwrap();
}

#[tokio::test]
async fn delegate_methods_import_hint_and_constructor_hints() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("ferris-idea-java-generate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    let root = root.canonicalize().unwrap();
    write(
        &root,
        "Engine",
        "package shop;\n\npublic class Engine {\n    public void start(int rpm) {}\n\n    public String label() {\n        return \"\";\n    }\n}\n",
    );
    write(
        &root,
        "Dog",
        "package shop;\n\npublic class Dog {\n    private final String name;\n\n    public Dog(String name) {\n        this.name = name;\n    }\n}\n",
    );
    write(
        &root,
        "Circle",
        "package shop;\n\npublic record Circle(double radius) {}\n",
    );
    let car_text = "package shop;\n\npublic class Car {\n    private Engine engine;\n}\n";
    write(&root, "Car", car_text);
    let main_text = "package shop;\n\npublic class Main {\n    void m() {\n        new Dog(\"Rex\");\n        new Circle(2.0);\n        Object o = new LinkedList<String>();\n    }\n}\n";
    write(&root, "Main", main_text);
    let car = root.join("src/main/java/shop/Car.java");
    let main = root.join("src/main/java/shop/Main.java");
    let wait = Duration::from_secs(120);

    // Delegate Methods: the field, without what every object has.
    let start = Instant::now();
    let fields = loop {
        let fields = delegate_fields_in(&root, &car, None, 3, 10, wait)
            .await
            .unwrap();
        if !fields.is_empty() || start.elapsed() > DEADLINE {
            break fields;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    assert_eq!(fields.len(), 1, "{fields:?}");
    assert_eq!(fields[0].name, "engine");
    let mut labels: Vec<String> = fields[0].methods.iter().map(|m| m.label()).collect();
    labels.sort();
    assert_eq!(labels, ["label()", "start(int)"]);
    let chosen: Vec<_> = fields[0]
        .methods
        .iter()
        .filter(|m| m.name == "start")
        .map(|m| (fields[0].clone(), m.clone()))
        .collect();
    let files = add_delegate_methods_in(&root, &car, None, 3, 10, &chosen, wait)
        .await
        .unwrap();
    let edits: Vec<_> = files.iter().flat_map(|f| f.edits.clone()).collect();
    let new = ferris_idea_languages::rename::apply_edits(car_text, &edits).unwrap();
    assert!(
        new.contains("public void start(int rpm) {") && new.contains("engine.start(rpm);"),
        "{new}"
    );
    assert!(!new.contains("label()"), "{new}");

    // Parameter name hints before the only argument of a constructor, record or class.
    let hints = ferris_idea_languages::inlay::parameter_hints_in(&root, &main, None, wait)
        .await
        .unwrap();
    let got: Vec<(u64, &str)> = hints.iter().map(|h| (h.line, h.label.as_str())).collect();
    assert_eq!(got, [(4, "name:"), (5, "radius:")], "{hints:?}");

    // The import hint: jdtls's message names the class, its actions name the package.
    let diagnostics = ferris_idea_languages::lsp::file_diagnostics(&root, &main, wait)
        .await
        .unwrap();
    let d = diagnostics
        .iter()
        .find(|d| unresolved_name(&d.message).is_some())
        .expect("an unresolved name");
    let name = unresolved_name(&d.message).unwrap();
    assert_eq!(name, "LinkedList");
    // Right after a full workspace run jdtls may still be indexing the JDK and offer nothing yet.
    let until = Instant::now() + Duration::from_secs(60);
    let candidates = loop {
        let found = import_candidates_at(
            &root,
            &main,
            None,
            d.start_line,
            d.start_character,
            name,
            wait,
        )
        .await
        .unwrap();
        if !found.is_empty() || Instant::now() > until {
            break found;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    assert_eq!(candidates, ["java.util.LinkedList"]);
    let _ = std::fs::remove_dir_all(&root);
}
