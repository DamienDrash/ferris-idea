//! SPEC-0072 v8 AK-19 (OBS-0017 step 9, defect F031-3): TypeScript completion through
//! typescript-language-server: member types from resolving, functions with parentheses, and
//! Ferris IDEA's own postfix templates. A test binary of its own, because the pool is switched on for
//! the whole process. Skipped without `typescript-language-server`.

use std::time::Duration;

use ferris_idea_languages::insight::{CompletionItem, complete_in, resolve_details_in};
use ferris_idea_languages::pool;
use ferris_idea_languages::script_completion::detail_parts;

const INVENTORY: &str = "export interface Item {\n  sku: string;\n  name: string;\n  qty: number;\n}\n\nexport function bySku<T extends { sku: string }>(items: T[]): Map<string, T> {\n  return new Map(items.map((item) => [item.sku, item]));\n}\n";

#[tokio::test]
async fn types_parentheses_and_postfix_templates() {
    // Only typescript-language-server: TypeScript 7's own server is checked in servers.rs.
    if !ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::TypeScript)
        .is_ok_and(|r| r.candidate.program == "typescript-language-server")
    {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: typescript-language-server"
        );
        eprintln!("skipped: typescript-language-server not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-ts-cmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        "{\"compilerOptions\": {\"strict\": true, \"target\": \"es2020\"}, \"include\": [\"src\"]}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/inventory.ts"), INVENTORY).unwrap();
    let file = root.join("src/report.ts");
    pool::enable();
    let wait = Duration::from_secs(120);
    let head = "import { Item } from \"./inventory\";\n\nexport function report(item: Item) {\n  ";
    let tail = "\n}\n";
    let ask = |typed: &'static str| {
        let (root, file) = (root.clone(), file.clone());
        async move {
            let text = format!("{head}{typed}{tail}");
            std::fs::write(&file, &text).unwrap();
            pool::sync_text(&root, &file, &text).await.unwrap();
            let column = 2 + typed.chars().count() as u64;
            let items = complete_in(&root, &file, Some(&text), 3, column, 5000, wait)
                .await
                .unwrap();
            (text, items)
        }
    };
    let find = |items: &[CompletionItem], label: &str| -> CompletionItem {
        items
            .iter()
            .find(|i| i.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("no {label} in {items:#?}"))
    };

    // Member types come from resolving (behaviour 28).
    // The server answers with an empty list while it still loads the project.
    let mut tries = 0;
    let (text, members) = loop {
        let (text, members) = ask("item.").await;
        tries += 1;
        if members.iter().any(|i| i.label == "qty") || tries == 60 {
            break (text, members);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let qty = find(&members, "qty");
    let resolved = resolve_details_in(&root, &file, Some(&text), std::slice::from_ref(&qty), wait)
        .await
        .unwrap();
    let detail = resolved[0]
        .as_ref()
        .and_then(|r| r.detail.clone())
        .expect("a resolved detail");
    assert_eq!(
        detail_parts(&qty.label, &detail).1.as_deref(),
        Some("number")
    );
    // The own postfix templates follow the members (behaviour 30).
    assert!(members.iter().any(|i| i.label == "const" && i.snippet));

    // Functions come with parentheses (behaviour 29), also those to import.
    let (_, functions) = ask("const m = bys").await;
    let by_sku = find(&functions, "bySku");
    assert!(by_sku.snippet, "{by_sku:?}");
    assert!(by_sku.insert.ends_with("($0)"), "{by_sku:?}");

    // A postfix template replaces its expression (behaviour 30).
    let (_, postfix) = ask("item.name.con").await;
    let constant = find(&postfix, "const");
    assert_eq!(constant.insert, "const ${1:name} = item.name;$0");
    let range = constant.edit_range.expect("postfix range");
    assert_eq!((range.start_character, range.end_character), (2, 15));

    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
