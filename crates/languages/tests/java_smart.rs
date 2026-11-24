//! SPEC-0072 v8 AK-17 (OBS-0017 step 4, defect F031-1): smart type completion with jdtls: the
//! expected type from a declaration, an argument and a return, and only fitting suggestions,
//! with the static factories of the expected type. A test binary of its own, because the pool is
//! switched on for the whole process. Skipped without `jdtls`.

use std::time::Duration;

use ferris_idea_languages::pool;
use ferris_idea_languages::smart::{Origin, smart_complete_in};

const HEAD: &str = "package demo;\n\nimport java.util.List;\n\nclass A {\n    String m(List<String> names, String s) {\n        ";
const TAIL: &str = "\n        return s;\n    }\n}\n";

#[tokio::test]
async fn smart_type_completion() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-java-smart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main/java/demo/A.java");
    pool::enable();
    let wait = Duration::from_secs(120);
    let ask = |typed: &'static str| {
        let (root, file) = (root.clone(), file.clone());
        async move {
            let text = format!("{HEAD}{typed}{TAIL}");
            std::fs::write(&file, &text).unwrap();
            pool::sync_text(&root, &file, &text).await.unwrap();
            let column = 8 + typed.chars().count() as u64;
            smart_complete_in(&root, &file, &text, 6, column, 100, wait)
                .await
                .unwrap()
        }
    };
    let labels = |items: &[ferris_idea_languages::insight::CompletionItem]| -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    };

    // A declaration: the variable of that type first, then `List.of` with the class name.
    let list = ask("List<String> l = ").await;
    assert_eq!(list.expected, ["List<String>"]);
    assert_eq!(list.origin, Some(Origin::Declaration));
    let found = labels(&list.items);
    assert_eq!(
        found.first().map(String::as_str),
        Some("names"),
        "{found:?}"
    );
    assert!(found.iter().any(|l| l == "List.of"), "{found:?}");
    assert!(!found.iter().any(|l| l == "s"), "{found:?}");
    let of = list
        .items
        .iter()
        .find(|i| i.label == "List.of")
        .expect("List.of");
    assert!(of.insert.starts_with("List.of("), "{of:?}");

    // An argument: the parameter's type from the signature help.
    let add = ask("names.add(").await;
    assert_eq!(add.origin, Some(Origin::Argument));
    assert!(add.expected.contains(&"String".to_owned()), "{add:?}");
    let found = labels(&add.items);
    assert!(found.iter().any(|l| l == "s"), "{found:?}");
    assert!(!found.iter().any(|l| l == "names"), "{found:?}");

    // A return: the method's return type from the document symbols.
    let ret = ask("return ").await;
    assert_eq!(ret.expected, ["String"]);
    assert_eq!(ret.origin, Some(Origin::Return));
    let found = labels(&ret.items);
    assert!(found.iter().any(|l| l == "s"), "{found:?}");
    assert!(!found.iter().any(|l| l == "names"), "{found:?}");

    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
