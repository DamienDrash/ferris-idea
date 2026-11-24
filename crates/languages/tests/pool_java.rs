//! A single Java file outside any build, through the language server pool as the window uses it
//! (`SPEC-0042` v7 behaviour 14, hand tests 12 and 16 of round 5). A test binary of its own,
//! because the pool is switched on for the whole process. Skipped without `jdtls`.

use std::time::Duration;

use ferris_idea_languages::insight::{self, LocationKind};
use ferris_idea_languages::pool;

fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

const TEXT: &str = "class Person {
    private final String name;

    Person(String name) {
        this.name = name;
    }

    String greet() {
        return \"Hello, \" + name;
    }
}

public class Main {
    public static void main(String[] args) {
        Person person = new Person(\"Ada\");
        System.out.println(person.greet());
    }
}
";

#[tokio::test]
async fn declarations_in_a_java_file_without_build() {
    if ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err() {
        skip("jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("ferris-idea-pool-java-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sprachen")).unwrap();
    std::fs::write(root.join("shapes.cpp"), "int main() { return 0; }\n").unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("sprachen/test.java");
    std::fs::write(&file, TEXT).unwrap();
    pool::enable();
    let wait = Duration::from_secs(60);

    // The editor hands its text to the pool when the file opens, then asks.
    pool::sync_text(&root, &file, TEXT).await.unwrap();
    for (line, character, want) in [(14u64, 30u64, 3u64), (15, 31, 14), (15, 35, 7), (14, 8, 0)] {
        let found = insight::locations_in(
            &root,
            &file,
            Some(TEXT),
            line,
            character,
            LocationKind::Definition,
            wait,
        )
        .await
        .unwrap();
        let lines: Vec<u64> = found.iter().map(|l| l.range.start_line).collect();
        assert_eq!(lines, [want], "definition at {line}:{character}: {found:?}");
    }
    // Hand test 209 (D59): the layout of the hand test, `Main` first, written while open (the
    // editor reloads it); ⌘B in `Person` of `new Person("Ada")` goes to the constructor.
    let main_first = "public class Main {\n    public static void main(String[] args) {\n        Person person = new Person(\"Ada\");\n        System.out.println(person.greet());\n    }\n}\n\nclass Person {\n    private final String name;\n\n    Person(String name) {\n        this.name = name;\n    }\n\n    String greet() {\n        return \"Hello, \" + name;\n    }\n}\n";
    std::fs::write(&file, main_first).unwrap();
    pool::sync_text(&root, &file, main_first).await.unwrap();
    for (line, character, want) in [
        (2u64, 28u64, 10u64),
        (2, 33, 10),
        (2, 34, 10),
        (3, 36, 14),
        (3, 30, 2),
    ] {
        let found = insight::locations_in(
            &root,
            &file,
            Some(main_first),
            line,
            character,
            LocationKind::Definition,
            wait,
        )
        .await
        .unwrap();
        let lines: Vec<u64> = found.iter().map(|l| l.range.start_line).collect();
        assert_eq!(lines, [want], "definition at {line}:{character}: {found:?}");
    }
    std::fs::write(&file, TEXT).unwrap();
    pool::sync_text(&root, &file, TEXT).await.unwrap();
    // `String` is a class of the JDK: no file, but named (SPEC-0042 v7 behaviour 16).
    let found = insight::found_in(
        &root,
        &file,
        Some(TEXT),
        1,
        20,
        LocationKind::Definition,
        wait,
    )
    .await
    .unwrap();
    assert!(found.locations.is_empty(), "{found:?}");
    assert_eq!(found.outside, ["java.lang.String"], "{found:?}");
    pool::close_text(&file).await;
    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
