//! OBS-0071 O4 through jdtls: hover on a method reference describes the referenced method, not the
//! functional interface (`SPEC-0024` v2 AK-5), and the callees of a method include `new` of a
//! record, whose canonical constructor jdtls leaves out (`SPEC-0082` v8 AK-15). A test binary of
//! its own, because the pool is switched on for the whole process. Skipped without `jdtls`.

use std::time::Duration;

use ferris_idea_languages::insight::{CallDirection, call_hierarchy_in, hover_in};
use ferris_idea_languages::pool;

const SHOP: &str = "package demo;

import java.util.List;
import java.util.function.Function;

public class Shop {
    record Rec(String name, int cents) {}

    static int totalValue(List<Item> items) {
        return items.stream().mapToInt(Item::totalCents).sum();
    }

    public static void main(String[] args) {
        Item a = new Item(\"pen\", 2, 150);
        List<Item> all = List.of(a, new Item(\"ink\", 1, 400));
        System.out.println(totalValue(all));
        Rec r = new Rec(\"x\", 1);
        Function<String, Integer> f = String::length;
        Runnable run = new Runnable() { public void run() { new Rec(\"inner\", 0); } };
        Shop s = new Shop();
        String t = new Rec(\"y\", 2).toString() + List.of(new Rec(\"z\", 3));
    }
}
";

const ITEM: &str = "package demo;

/** A line of an order. */
public class Item {
    final int count;
    final int cents;

    /** Makes a line. */
    Item(String name, int count, int cents) {
        this.count = count;
        this.cents = cents;
    }

    /** The price of the whole line in cents. */
    int totalCents() {
        return count * cents;
    }
}
";

/// Line and character of the `nth` occurrence of `word` in `text`.
fn at(text: &str, word: &str, nth: usize) -> (u64, u64) {
    let (line, l) = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(word))
        .nth(nth)
        .unwrap();
    (line as u64, l.find(word).unwrap() as u64)
}

#[tokio::test]
async fn method_references_and_record_constructors() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        assert!(
            ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-java-mref-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(
        root.join("pom.xml"),
        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>mref</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>\n",
    )
    .unwrap();
    let shop = root.join("src/main/java/demo/Shop.java");
    std::fs::write(&shop, SHOP).unwrap();
    std::fs::write(root.join("src/main/java/demo/Item.java"), ITEM).unwrap();
    pool::enable();
    let wait = Duration::from_secs(120);
    pool::sync_text(&root, &shop, SHOP).await.unwrap();

    // SPEC-0024 v2 AK-5: on `::`, on the name's first character and inside it, the referenced
    // method; jdtls alone gave `ToIntFunction.applyAsInt` on the first two.
    let (line, colons) = at(SHOP, "::totalCents", 0);
    for character in [colons, colons + 1, colons + 2, colons + 6] {
        let hover = hover_in(&root, &shop, None, line, character, wait)
            .await
            .unwrap()
            .unwrap();
        assert!(
            hover.text.starts_with("int demo.Item.totalCents()"),
            "{character}: {}",
            hover.text
        );
        assert!(!hover.text.contains("applyAsInt"), "{}", hover.text);
    }
    let (line, colons) = at(SHOP, "::length", 0);
    let hover = hover_in(&root, &shop, None, line, colons + 2, wait)
        .await
        .unwrap()
        .unwrap();
    assert!(
        hover.text.starts_with("int java.lang.String.length()"),
        "{}",
        hover.text
    );
    // The type before `::` stays the type.
    let hover = hover_in(&root, &shop, None, line, colons - 2, wait)
        .await
        .unwrap()
        .unwrap();
    assert!(hover.text.starts_with("java.lang.String"), "{}", hover.text);

    // SPEC-0082 v8 AK-15: the callees of `main` have `Rec` with its three call sites in `main`
    // (not the one in the anonymous class), beside what jdtls reports, each once, in source order.
    let (line, character) = at(SHOP, "main(", 0);
    let tree = call_hierarchy_in(
        &root,
        &shop,
        None,
        line,
        character + 1,
        CallDirection::Callees,
        2,
        wait,
    )
    .await
    .unwrap()
    .unwrap();
    let names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
    let short: Vec<&str> = names
        .iter()
        .map(|n| n.split(['(', ' ']).next().unwrap())
        .collect();
    let rec = short.iter().position(|n| *n == "Rec").expect("Rec");
    assert_eq!(
        short.iter().filter(|n| **n == "Rec").count(),
        1,
        "{names:?}"
    );
    for once in ["Item", "Runnable", "Shop", "totalValue", "println"] {
        assert_eq!(
            short.iter().filter(|n| **n == once).count(),
            1,
            "{once}: {names:?}"
        );
    }
    let println = short.iter().position(|n| *n == "println").unwrap();
    let runnable = short.iter().position(|n| *n == "Runnable").unwrap();
    assert!(println < rec && rec < runnable, "{names:?}");
    let node = &tree.children[rec];
    let (rec_line, rec_character) = at(SHOP, "Rec(String", 0);
    assert_eq!(
        (
            node.location.range.start_line,
            node.location.range.start_character
        ),
        (rec_line, rec_character),
        "{node:?}"
    );
    let sites: Vec<(u64, u64)> = node
        .call_sites
        .iter()
        .map(|r| (r.start_line, r.start_character))
        .collect();
    let first = at(SHOP, "new Rec(\"x\"", 0);
    let (line, _) = at(SHOP, "new Rec(\"y\"", 0);
    let text = SHOP.lines().nth(line as usize).unwrap();
    let second = (line, text.find("new Rec(\"y\"").unwrap() as u64);
    let third = (line, text.find("new Rec(\"z\"").unwrap() as u64);
    assert_eq!(sites, vec![first, second, third], "{node:?}");
    assert_eq!(node.call_sites[0].end_character, first.1 + 15, "{node:?}");
    assert!(!node.recursive);
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
