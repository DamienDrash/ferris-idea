//! OBS-0016 defects through jdtls: the class, package and module of implementations
//! (SPEC-0081 v2 AK-4), the kind and context of usages (SPEC-0293 v3 AK-8) and the inherited
//! members of the File Structure popup (SPEC-0079 v4 AK-6). A test binary of its own, because the
//! pool is switched on for the whole process. Skipped without `jdtls`.

use std::time::Duration;

use ferris_idea_languages::context::{UsageKind, contexts_in};
use ferris_idea_languages::find_usages::find_usages_in;
use ferris_idea_languages::insight::{LocationKind, locations_in, symbols_in};
use ferris_idea_languages::pool;
use ferris_idea_languages::structure::{ordered, with_inherited_in};

const DISCOUNT: &str = "package lab.inventory;\n\n/** A rule that lowers a price. */\npublic interface Discount {\n    /** Returns the price in cents after this rule. */\n    int apply(int cents);\n}\n";

const DISCOUNTS: &str = "package lab.inventory;\n\npublic final class Discounts {\n    static final class None implements Discount {\n        public int apply(int cents) {\n            return cents;\n        }\n    }\n\n    static final class Half implements Discount {\n        public int apply(int cents) {\n            return cents / 2;\n        }\n    }\n\n    static int priceOf(int cents, Discount rule) {\n        return rule.apply(cents);\n    }\n\n    static int bothPrices(int cents) {\n        Discount half = new Half();\n        return priceOf(cents, new None()) + priceOf(cents, half);\n    }\n}\n";

const CHILD: &str = "package lab.inventory;\n\npublic class Child extends Discounts.Half {\n    int extra;\n\n    void own() {\n    }\n}\n";

#[tokio::test]
async fn implementations_usages_and_inherited_members() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-java-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("shop/src/main/java/lab/inventory");
    std::fs::create_dir_all(&dir).unwrap();
    let root = root.canonicalize().unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        root.join("shop/pom.xml"),
        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion><groupId>lab</groupId><artifactId>shop</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>\n",
    )
    .unwrap();
    let project = root.join("shop");
    std::fs::write(dir.join("Discount.java"), DISCOUNT).unwrap();
    std::fs::write(dir.join("Discounts.java"), DISCOUNTS).unwrap();
    std::fs::write(dir.join("Child.java"), CHILD).unwrap();
    pool::enable();
    let wait = Duration::from_secs(120);
    let discounts = dir.join("Discounts.java");
    pool::sync_text(&project, &discounts, DISCOUNTS)
        .await
        .unwrap();

    // SPEC-0081 v2 AK-4: the implementations of `rule.apply` name their class and package.
    // jdtls answers them from its search index, which its first search builds; on a loaded
    // machine that can take longer than one question's wait. The pooled server keeps building it
    // after a question that ran out of time, so the condition is a server that answers: asked
    // again after a timeout, until it does (SPEC-0042 v21 behaviour 37).
    let until = std::time::Instant::now() + Duration::from_secs(600);
    let found = loop {
        match locations_in(
            &project,
            &discounts,
            None,
            16,
            20,
            LocationKind::Implementation,
            wait,
        )
        .await
        {
            Err(ferris_idea_languages::lsp::LspError::Timeout(..))
                if std::time::Instant::now() < until => {}
            other => break other.unwrap(),
        }
    };
    let contexts = contexts_in(&project, &discounts, None, &found, wait)
        .await
        .unwrap();
    let mut containers: Vec<String> = contexts
        .iter()
        .filter_map(|c| c.container.clone())
        .collect();
    containers.sort();
    assert_eq!(
        containers,
        ["Discounts.Half", "Discounts.None"],
        "{contexts:?}"
    );
    assert!(
        contexts
            .iter()
            .all(|c| c.package.as_deref() == Some("lab.inventory")
                && c.module == "shop"
                && c.symbol.as_deref() == Some("apply(int)"))
    );

    // SPEC-0293 v3 AK-8: the call of `apply` in `priceOf`.
    let search = find_usages_in(
        &project,
        &dir.join("Discount.java"),
        None,
        5,
        8,
        "apply",
        Duration::from_secs(60),
        None,
    )
    .await
    .unwrap();
    assert_eq!(search.target.kind.as_deref(), Some("method"), "{search:?}");
    let call = search
        .usages
        .iter()
        .position(|u| u.range.start_line == 16)
        .expect("the call in priceOf");
    let d = &search.details[call];
    assert_eq!(d.kind, UsageKind::Call);
    assert_eq!(d.context.module, "shop");
    assert_eq!(d.context.package.as_deref(), Some("lab.inventory"));
    assert_eq!(d.context.container.as_deref(), Some("Discounts"));
    assert!(
        d.context
            .method
            .as_deref()
            .is_some_and(|m| m.starts_with("priceOf(")),
        "{d:?}"
    );

    // SPEC-0079 v4 AK-6: `Child` inherits `apply(int)` from `Discounts.Half`.
    let child = dir.join("Child.java");
    let symbols = symbols_in(&project, &child, None, wait).await.unwrap();
    let all = with_inherited_in(&project, &child, None, symbols, wait)
        .await
        .unwrap();
    let inherited: Vec<(&str, &str)> = all
        .iter()
        .filter_map(|s| Some((s.name.as_str(), s.inherited_from.as_deref()?)))
        .collect();
    assert!(
        inherited
            .iter()
            .any(|(n, from)| *n == "apply(int)" && from.contains("Half")),
        "{all:?}"
    );
    // Ordered: methods (own and inherited) before fields.
    let shown: Vec<String> = ordered(all)
        .into_iter()
        .filter(|s| s.depth == 1)
        .map(|s| s.name)
        .collect();
    assert_eq!(shown.last().map(String::as_str), Some("extra"), "{shown:?}");
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
