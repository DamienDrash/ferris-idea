//! SPEC-0122 v8 AK-22 and AK-23 (OBS-0027 J1, J2) with a real jdtls: Extract Constant and Extract
//! Variable name what they introduce after the parameter the value is passed to, and Inline
//! Method offers its variants at a call and at the declaration. Skipped without `jdtls`.

use std::time::{Duration, Instant};

use ferris_idea_languages::actions::{
    Refactoring, introduced_name, plan_code_action_in, refactoring_targets,
};
use ferris_idea_languages::insight::Range;
use ferris_idea_languages::java_refactor::{
    INLINE_ALL_KEEP, INLINE_ALL_REMOVE, INLINE_METHOD, INLINE_THIS,
};
use ferris_idea_languages::rename::apply_edits;

const DEADLINE: Duration = Duration::from_secs(300);

/// The probe file of OBS-0027 with a second method that uses `totalValue` twice.
const INVENTORY: &str = "package lab.inventory;\n\nimport java.util.List;\n\npublic final class Inventory {\n    public record Item(String sku, String name, int qty, int unitPriceCents) {\n        int totalCents() {\n            return qty * unitPriceCents;\n        }\n    }\n\n    public static int totalValue(List<Item> items) {\n        return items.stream().mapToInt(Item::totalCents).sum();\n    }\n\n    static int twice(List<Item> items) {\n        return totalValue(items) + totalValue(items);\n    }\n\n    public static void main(String[] args) {\n        System.out.println(totalValue(List.of(new Item(\"A-1\", \"Bolt\", 3, 25))));\n    }\n}\n";

fn range(line: u64, start: u64, end: u64) -> Range {
    Range {
        start_line: line,
        start_character: start,
        end_line: line,
        end_character: end,
    }
}

#[tokio::test]
async fn constants_are_named_after_the_parameter_and_inline_method_has_variants() {
    crate::pooled();
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root =
        std::env::temp_dir().join(format!("ferris-idea-java-refactor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/lab/inventory")).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("src/main/java/lab/inventory/Inventory.java");
    std::fs::write(&file, INVENTORY).unwrap();
    let wait = Duration::from_secs(120);
    let column = |line: u64, needle: &str| {
        let text = INVENTORY.lines().nth(line as usize).unwrap();
        text.find(needle).unwrap() as u64
    };
    let plan = async |r: &Range, title: &str| {
        let files = plan_code_action_in(&root, &file, Some(INVENTORY), r, title, wait)
            .await
            .unwrap_or_else(|e| panic!("{title}: {e}"));
        assert_eq!(files.len(), 1, "{files:?}");
        apply_edits(INVENTORY, &files[0].edits).unwrap()
    };

    // Extract Constant on the literal passed as `unitPriceCents` (J1).
    let at = column(20, "25)");
    let literal = range(20, at, at + 2);
    let start = Instant::now();
    let found = loop {
        let found = refactoring_targets(
            &root,
            &file,
            Some(INVENTORY),
            &literal,
            Refactoring::Constant,
            wait,
        )
        .await
        .unwrap();
        if !found.is_empty() || start.elapsed() > DEADLINE {
            break found;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, "25");
    let title = found[0].actions[0].title.clone();
    let after = plan(&literal, &title).await;
    assert!(
        after.contains("private static final int UNIT_PRICE_CENTS = 25;"),
        "{after}"
    );
    assert!(after.contains("\"Bolt\", 3, UNIT_PRICE_CENTS)"), "{after}");
    assert!(!after.contains("_25"), "{after}");
    // The name popup and `new_name` start from it.
    let name = introduced_name(INVENTORY, &after).unwrap();
    assert_eq!(name.name, "UNIT_PRICE_CENTS");

    // Extract Variable there: jdtls's one-letter name gives way to the parameter's.
    let found = refactoring_targets(
        &root,
        &file,
        Some(INVENTORY),
        &literal,
        Refactoring::Variable,
        wait,
    )
    .await
    .unwrap();
    let title = &found[0].actions.last().unwrap().title;
    let after = plan(&literal, title).await;
    assert!(after.contains("int unitPriceCents = 25;"), "{after}");
    assert!(after.contains("\"Bolt\", 3, unitPriceCents)"), "{after}");
    // A returned value of a method without `get` keeps the server's name.
    let product = range(7, column(7, "qty"), column(7, ";"));
    let after = plan(&product, title).await;
    assert!(after.contains("int i = qty * unitPriceCents;"), "{after}");

    // Inline Method on the declaration (J2): all usages, with or without the method.
    let titles = async |r: &Range| {
        let found =
            refactoring_targets(&root, &file, Some(INVENTORY), r, Refactoring::Inline, wait)
                .await
                .unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        found[0]
            .actions
            .iter()
            .map(|a| a.title.clone())
            .collect::<Vec<_>>()
    };
    let declared = column(11, "totalValue") + 2;
    let declared = range(11, declared, declared);
    assert_eq!(
        titles(&declared).await,
        [INLINE_ALL_REMOVE, INLINE_ALL_KEEP]
    );
    let inlined = "items.stream().mapToInt(Item::totalCents).sum()";
    let removed = plan(&declared, INLINE_ALL_REMOVE).await;
    assert!(!removed.contains("totalValue"), "{removed}");
    assert!(
        removed.contains(&format!("return {inlined} + {inlined};")),
        "{removed}"
    );
    let kept = plan(&declared, INLINE_ALL_KEEP).await;
    assert_eq!(kept.matches("totalValue").count(), 1, "{kept}");
    assert!(
        kept.contains("public static int totalValue(List<Item> items) {"),
        "{kept}"
    );
    assert!(
        kept.contains(&format!("return {inlined} + {inlined};")),
        "{kept}"
    );
    assert!(
        kept.contains("System.out.println(List.of(new Item("),
        "{kept}"
    );
    let err = plan_code_action_in(&root, &file, Some(INVENTORY), &declared, INLINE_THIS, wait)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("caret on a call"), "{err}");

    // On a call: also this usage alone.
    let called = column(20, "totalValue") + 2;
    let called = range(20, called, called);
    assert_eq!(
        titles(&called).await,
        [INLINE_ALL_REMOVE, INLINE_ALL_KEEP, INLINE_THIS]
    );
    let this = plan(&called, INLINE_THIS).await;
    assert_eq!(this.matches("totalValue").count(), 3, "{this}");
    assert!(
        this.contains(
            "System.out.println(List.of(new Item(\"A-1\", \"Bolt\", 3, 25)).stream().mapToInt(Item::totalCents).sum());"
        ),
        "{this}"
    );
    assert_eq!(plan(&called, INLINE_ALL_REMOVE).await, removed);
    assert_eq!(plan(&called, INLINE_ALL_KEEP).await, kept);
    // The server's own action is still there by its title (⌥↩, `code_actions` without a
    // refactoring).
    assert_eq!(plan(&called, INLINE_METHOD).await, this);
    assert_eq!(plan(&declared, INLINE_METHOD).await, removed);
    crate::done(&root);
    let _ = std::fs::remove_dir_all(&root);
}
