//! SPEC-0082 v5 AK-9 and SPEC-0122 v3 AK-8 (OBS-0027): type hierarchy, method hierarchy, super
//! method and the methods to override or implement, through a real jdtls on a small project with
//! an abstract class, an interface and two subclasses. Skipped without `jdtls`.

use std::path::Path;
use std::time::{Duration, Instant};

use idecraft_languages::insight::CallNode;
use idecraft_languages::java_structure::{
    TypeDirection, add_overridable_methods_in, method_hierarchy_in, overridable_methods_in,
    super_target_in, type_hierarchy_in,
};

const DEADLINE: Duration = Duration::from_secs(300);

fn names(n: &CallNode, depth: usize, out: &mut Vec<String>) {
    out.push(format!(
        "{}{}{}",
        "  ".repeat(depth),
        n.mark
            .as_deref()
            .map(|m| format!("{m} "))
            .unwrap_or_default(),
        n.name
    ));
    for c in &n.children {
        names(c, depth + 1, out);
    }
}

fn write(root: &Path, name: &str, text: &str) {
    std::fs::write(root.join(format!("src/main/java/shop/{name}.java")), text).unwrap();
}

#[tokio::test]
async fn hierarchies_super_method_and_overridable_methods() {
    crate::pooled();
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err() {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-java-structure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    let root = root.canonicalize().unwrap();
    write(
        &root,
        "Animal",
        "package shop;\n\npublic abstract class Animal {\n    public abstract String sound();\n\n    public String describe() {\n        return sound();\n    }\n}\n",
    );
    write(
        &root,
        "Pet",
        "package shop;\n\npublic interface Pet {\n    String owner();\n}\n",
    );
    write(
        &root,
        "Dog",
        "package shop;\n\npublic class Dog extends Animal implements Pet {\n    @Override\n    public String sound() {\n        return \"woof\";\n    }\n\n    @Override\n    public String owner() {\n        return \"Ada\";\n    }\n}\n",
    );
    write(
        &root,
        "Cat",
        "package shop;\n\npublic class Cat extends Animal {\n    public String sound() {\n        return \"meow\";\n    }\n}\n",
    );
    let animal = root.join("src/main/java/shop/Animal.java");
    let dog = root.join("src/main/java/shop/Dog.java");
    let cat = root.join("src/main/java/shop/Cat.java");
    let wait = Duration::from_secs(120);
    let none = |_: &Path| None;

    // Class hierarchy from inside Animal's body: Object → * Animal → Cat, Dog (in some order).
    let start = Instant::now();
    let tree = loop {
        let tree = type_hierarchy_in(&root, &animal, None, 5, 10, TypeDirection::Class, wait)
            .await
            .unwrap();
        let ok = tree.as_ref().is_some_and(|t| {
            t.name == "Object" && t.children.first().is_some_and(|a| a.children.len() == 2)
        });
        if ok || start.elapsed() > DEADLINE {
            break tree.expect("a hierarchy");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    let mut rows = Vec::new();
    names(&tree, 0, &mut rows);
    assert_eq!(rows[0], "Object", "{rows:?}");
    assert_eq!(rows[1], "  * Animal", "{rows:?}");
    let mut subs = rows[2..].to_vec();
    subs.sort();
    assert_eq!(subs, ["    Cat", "    Dog"], "{rows:?}");

    // Supertypes of Dog: the class and the interface.
    let up = type_hierarchy_in(&root, &dog, None, 2, 14, TypeDirection::Supertypes, wait)
        .await
        .unwrap()
        .unwrap();
    let ups: Vec<&str> = up.children.iter().map(|c| c.name.as_str()).collect();
    assert!(ups.contains(&"Animal") && ups.contains(&"Pet"), "{ups:?}");

    // Super method of Dog.sound() is Animal.sound(); of Dog.owner() Pet.owner(); Dog → Animal.
    let s = super_target_in(&root, &dog, None, 5, 8, &none, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.name, "Animal.sound()");
    let at = s.location.unwrap();
    assert!(at.path.ends_with("Animal.java"));
    assert_eq!((at.range.start_line, at.range.start_character), (3, 27));
    let s = super_target_in(&root, &dog, None, 9, 20, &none, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.name, "Pet.owner()");
    let s = super_target_in(&root, &dog, None, 2, 14, &none, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.name, "Animal");

    // Method hierarchy of Cat.sound(): from Animal, which declares it, down to both subclasses.
    let (method, tree) = method_hierarchy_in(&root, &cat, None, 3, 20, &none, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(method, "sound");
    let mut rows = Vec::new();
    names(&tree, 0, &mut rows);
    assert_eq!(rows[0], "+ Animal", "{rows:?}");
    let mut subs = rows[1..].to_vec();
    subs.sort();
    assert_eq!(subs, ["  + Cat", "  + Dog"], "{rows:?}");

    // Override in Cat: describe() of Animal and the methods of Object; nothing to implement.
    let (ty, methods) = overridable_methods_in(&root, &cat, None, 4, 0, wait)
        .await
        .unwrap();
    assert_eq!(ty, "Cat");
    assert!(
        methods
            .iter()
            .any(|m| m.name == "describe" && m.declaring_class.ends_with("Animal"))
    );
    assert!(methods.iter().any(|m| m.name == "toString"));
    assert!(!methods.iter().any(|m| m.unimplemented), "{methods:?}");
    let describe: Vec<_> = methods
        .into_iter()
        .filter(|m| m.name == "describe")
        .collect();
    let files = add_overridable_methods_in(&root, &cat, None, 2, 14, &describe, wait)
        .await
        .unwrap();
    let text = std::fs::read_to_string(&cat).unwrap();
    let edits = &files
        .iter()
        .find(|f| f.path.ends_with("Cat.java"))
        .unwrap()
        .edits;
    let after = idecraft_languages::rename::apply_edits(&text, edits).unwrap();
    assert!(after.contains("@Override"), "{after}");
    assert!(after.contains("public String describe()"), "{after}");
    assert!(after.contains("super.describe()"), "{after}");
    crate::done(&root);
    std::fs::remove_dir_all(&root).ok();
}

// SPEC-0320 AK-10 (v2) and SPEC-0292 AK-11 (v5): marks and counts at members through jdtls.
#[tokio::test]
async fn gutter_marks_and_member_counts() {
    crate::pooled();
    use idecraft_languages::java_marks::declaration_marks_in;
    use idecraft_languages::ts_structure::MarkKind;
    use idecraft_languages::usages::usage_counts_in;
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err() {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: jdtls"
        );
        eprintln!("skipped: jdtls not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("idecraft-java-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/main/java/shop")).unwrap();
    let root = root.canonicalize().unwrap();
    write(
        &root,
        "Animal",
        "package shop;\n\npublic abstract class Animal {\n    public abstract String sound();\n\n    public String describe() {\n        return sound();\n    }\n}\n",
    );
    write(
        &root,
        "Dog",
        "package shop;\n\npublic class Dog extends Animal {\n    private int legs = 4;\n\n    @Override\n    public String sound() {\n        return \"woof\";\n    }\n\n    private void helper() {\n    }\n\n    public static void main(String[] a) {\n        System.out.println(new Dog().sound());\n    }\n}\n",
    );
    let animal = root.join("src/main/java/shop/Animal.java");
    let dog = root.join("src/main/java/shop/Dog.java");
    let wait = Duration::from_secs(120);

    // jdtls may still be indexing: ask until Animal's implementation is seen.
    let start = Instant::now();
    let marks = loop {
        let marks = declaration_marks_in(&root, &animal, None, wait)
            .await
            .unwrap();
        let done = marks.iter().any(|m| m.kind == MarkKind::Implemented);
        if done || start.elapsed() > DEADLINE {
            break marks;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    assert!(
        marks
            .iter()
            .any(|m| m.name == "Animal" && m.kind == MarkKind::Inherited),
        "{marks:?}"
    );
    let sound = marks
        .iter()
        .find(|m| m.name == "sound" && m.kind == MarkKind::Implemented)
        .unwrap_or_else(|| panic!("{marks:?}"));
    assert!(sound.targets.iter().any(|t| t.name == "Dog"), "{marks:?}");

    let marks = declaration_marks_in(&root, &dog, None, wait).await.unwrap();
    let sound = marks
        .iter()
        .find(|m| m.name == "sound" && m.kind == MarkKind::Implements)
        .unwrap_or_else(|| panic!("{marks:?}"));
    assert_eq!(sound.targets[0].name, "Animal", "{marks:?}");
    assert_eq!(sound.targets[0].location.range.start_line, 3);
    assert!(marks.iter().all(|m| m.name != "helper"), "{marks:?}");

    // Counts at types and members, no usages included.
    let counts = usage_counts_in(&root, &dog, None, wait).await.unwrap();
    let helper = counts.iter().find(|c| c.name == "helper").unwrap();
    assert_eq!(helper.count, 0, "{counts:?}");
    assert_eq!(helper.label(), "no usages");
    let sound = counts.iter().find(|c| c.name == "sound").unwrap();
    assert!(sound.count >= 1, "{counts:?}");
    let counts = usage_counts_in(&root, &animal, None, wait).await.unwrap();
    let a = counts.iter().find(|c| c.kind == "class").unwrap();
    assert_eq!(a.inheritors, 1, "{counts:?}");
    crate::done(&root);
    let _ = std::fs::remove_dir_all(&root);
}
