//! SPEC-0042/AK-17 (v11, OBS-0027 L35): a Java class written to disk by another program while
//! jdtls runs is told to it with `workspace/didChangeWatchedFiles`, so the type hierarchy lists
//! it without the file being opened; a deleted one leaves it. Skipped without `jdtls`. A test
//! binary of its own, because the pool is switched on for the whole process.

use std::path::Path;
use std::time::{Duration, Instant};

use idecraft_languages::insight::CallNode;
use idecraft_languages::java_structure::{TypeDirection, type_hierarchy_in};
use idecraft_languages::pool;
use idecraft_languages::watched_files::DiskWatch;

const DEADLINE: Duration = Duration::from_secs(300);

fn write(root: &Path, name: &str, text: &str) {
    std::fs::write(root.join(format!("src/main/java/shop/{name}.java")), text).unwrap();
}

/// The subclasses the class hierarchy of `Animal` lists, sorted.
fn subclasses(tree: &CallNode) -> Vec<String> {
    let mut names: Vec<String> = tree
        .children
        .first()
        .map(|animal| animal.children.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Asks for the class hierarchy of `Animal` until its subclasses are `want`, looking at the disk
/// before every question as the window's two-second round does.
async fn hierarchy_becomes(root: &Path, watch: &mut DiskWatch, want: &[&str]) -> Vec<String> {
    let animal = root.join("src/main/java/shop/Animal.java");
    let start = Instant::now();
    loop {
        watch.round(root);
        let tree = type_hierarchy_in(
            root,
            &animal,
            None,
            2,
            24,
            TypeDirection::Class,
            Duration::from_secs(120),
        )
        .await
        .unwrap();
        let seen = tree.as_ref().map(subclasses).unwrap_or_default();
        if seen == want || start.elapsed() > DEADLINE {
            return seen;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test]
async fn a_subclass_written_outside_shows_in_the_type_hierarchy() {
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err() {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-java-watched-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    let root = root.canonicalize().unwrap();
    write(
        &root,
        "Animal",
        "package shop;\n\npublic abstract class Animal {\n    public abstract String sound();\n}\n",
    );
    write(
        &root,
        "Dog",
        "package shop;\n\npublic class Dog extends Animal {\n    public String sound() {\n        return \"woof\";\n    }\n}\n",
    );
    pool::enable();
    let mut watch = DiskWatch::default();
    assert!(!pool::watches(&root), "no server yet: nothing to watch for");

    // jdtls starts and knows the project as it is on disk.
    assert_eq!(
        hierarchy_becomes(&root, &mut watch, &["Dog"]).await,
        ["Dog"]
    );
    assert!(pool::watches(&root));
    // The window's round has looked at the disk since the server appeared: its baseline.
    watch.round(&root);
    assert_eq!(pool::forwarded_events(), 0);

    // Another program writes a subclass (and build output, which is never told); IDECraft does
    // not open it.
    write(
        &root,
        "Cat",
        "package shop;\n\npublic class Cat extends Animal {\n    public String sound() {\n        return \"meow\";\n    }\n}\n",
    );
    std::fs::create_dir_all(root.join("build/gen")).unwrap();
    std::fs::write(root.join("build/gen/Gen.java"), "class Gen {}\n").unwrap();
    assert_eq!(
        hierarchy_becomes(&root, &mut watch, &["Cat", "Dog"]).await,
        ["Cat", "Dog"]
    );
    assert_eq!(pool::forwarded_events(), 1, "Cat.java created, once");

    // The file goes away again: so does the subclass.
    std::fs::remove_file(root.join("src/main/java/shop/Cat.java")).unwrap();
    assert_eq!(
        hierarchy_becomes(&root, &mut watch, &["Dog"]).await,
        ["Dog"]
    );
    assert_eq!(pool::forwarded_events(), 2, "Cat.java deleted");

    pool::shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
