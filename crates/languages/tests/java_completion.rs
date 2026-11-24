//! SPEC-0072 v7 AK-13 (OBS-0017): Java completion through jdtls with snippets: method calls come
//! with empty parentheses, postfix templates replace the expression, live templates expand, an
//! unimported type brings its import, and JDK-internal types stay out of the list. A test binary of
//! its own, because the pool is switched on for the whole process. Skipped without `jdtls`.
//!
//! jdtls reports `ServiceReady` before it has indexed the JDK: until then type-name completion
//! answers with an empty list. On a loaded machine that takes long, so every question is repeated
//! until its item is there, within [`DEADLINE`]; on an idle machine the first answer counts.

use std::time::{Duration, Instant};

use ferris_idea_languages::insight::{CompletionItem, complete_in, resolve_completion_in};
use ferris_idea_languages::pool;

const HEAD: &str =
    "package demo;\n\nclass A {\n    void m(java.util.List<String> names, String s) {\n        ";
const TAIL: &str = "\n    }\n}\n";

/// How long a question is repeated until jdtls has what it asks for (only spent when it is slow).
const DEADLINE: Duration = Duration::from_secs(300);

fn varargs_of(items: &[CompletionItem]) -> Option<&CompletionItem> {
    items
        .iter()
        .find(|i| i.label_detail.as_deref() == Some("(E... elements)"))
}

fn has(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|i| i.label == label)
}

#[tokio::test]
async fn snippets_postfix_templates_and_imports() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-java-cmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main/java/demo/A.java");
    pool::enable();
    let wait = Duration::from_secs(120);
    let ask_once = |typed: &'static str| {
        let (root, file) = (root.clone(), file.clone());
        async move {
            let text = format!("{HEAD}{typed}{TAIL}");
            std::fs::write(&file, &text).unwrap();
            pool::sync_text(&root, &file, &text).await.unwrap();
            let column = 8 + typed.chars().count() as u64;
            complete_in(&root, &file, Some(&text), 4, column, 2000, wait)
                .await
                .unwrap()
        }
    };
    // Completion after `typed`, asked again until `ready` holds for the list or the deadline
    // passed; the last list is returned either way and checked by the caller.
    let ask = |typed: &'static str, ready: fn(&[CompletionItem]) -> bool| {
        let ask_once = &ask_once;
        async move {
            let start = Instant::now();
            let mut tries = 1;
            let mut items = ask_once(typed).await;
            while !ready(&items) && start.elapsed() < DEADLINE {
                tokio::time::sleep(Duration::from_secs(1)).await;
                tries += 1;
                items = ask_once(typed).await;
            }
            if tries > 1 {
                eprintln!("{typed:?}: {tries} questions in {:?}", start.elapsed());
            }
            items
        }
    };
    let find = |items: &[CompletionItem], label: &str| -> CompletionItem {
        items
            .iter()
            .find(|i| i.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("no {label} in {items:#?}"))
    };

    // Method calls: empty parentheses, caret inside (behaviour 21).
    let of = ask("java.util.List.o", |items| varargs_of(items).is_some()).await;
    let varargs = varargs_of(&of).expect("varargs overload");
    assert!(varargs.snippet, "{varargs:?}");
    assert_eq!(varargs.insert, "of(${0})", "{varargs:?}");

    // Postfix: the range covers the expression (behaviour 19).
    let postfix = ask("s.length().var", |items| has(items, "var")).await;
    let var = find(&postfix, "var");
    assert!(
        var.insert.starts_with("int ${1:length} = s.length();"),
        "{var:?}"
    );
    let range = var.edit_range.clone().expect("postfix range");
    assert_eq!((range.start_character, range.end_character), (8, 22));

    // Live templates (behaviour 18).
    let sout = ask("sout", |items| has(items, "sout")).await;
    assert!(find(&sout, "sout").insert.contains("System.out.println("));
    // JDK-internal types are left out (behaviour 21).
    assert!(
        !sout.iter().any(|i| i
            .detail
            .as_deref()
            .is_some_and(|d| d.starts_with("com.sun."))),
        "{sout:#?}"
    );

    // An unimported type brings its import (behaviour 20). Type names come from jdtls' index of
    // the JDK, which answers nothing until the indexing after the start finished.
    let list = ask("new ArrayLis", |items| has(items, "ArrayList")).await;
    let array_list = find(&list, "ArrayList");
    assert!(
        array_list
            .additional_edits
            .iter()
            .any(|e| e.text.contains("import java.util.ArrayList;")),
        "{array_list:?}"
    );

    // Documentation that jdtls garbles is rendered from the JDK's source (SPEC-0072 v8
    // behaviour 27, OBS-0017 step 11). List and resolve are asked again until jdtls has the
    // documentation.
    let text = format!("{HEAD}s.isB{TAIL}");
    let start = Instant::now();
    let doc = loop {
        let blank = ask("s.isB", |items| has(items, "isBlank")).await;
        let is_blank = find(&blank, "isBlank");
        let doc = resolve_completion_in(&root, &file, Some(&text), &is_blank, wait)
            .await
            .unwrap();
        if doc.is_some() || start.elapsed() >= DEADLINE {
            break doc;
        }
        eprintln!("isBlank: no documentation yet after {:?}", start.elapsed());
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    .expect("documentation of isBlank");
    assert!(
        !ferris_idea_languages::javadoc::looks_garbled(&doc),
        "{doc}"
    );
    assert!(!doc.contains("jdt://"), "{doc}");
    assert!(doc.contains("`true`"), "{doc}");

    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
