//! SPEC-0082 v6 AK-12, SPEC-0122 v6 AK-19, SPEC-0320 v3 AK-11, SPEC-0292 v6 AK-12 and SPEC-0291
//! v5 AK-2d (OBS-0035): the Kotlin project of the observation on disk. Hierarchies, members to
//! override and gutter marks come from the syntax trees and need no server; usage counts and
//! parameter hints ask a real kotlin-language-server and are skipped without it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use idecraft_languages::insight::CallNode;
use idecraft_languages::java_marks::declaration_marks_in;
use idecraft_languages::java_structure::{
    TypeDirection, add_overridable_methods_in, method_hierarchy_in, overridable_methods_in,
    super_target_in, type_hierarchy_in,
};
use idecraft_languages::ts_structure::MarkKind;

const ANIMALS: &str = "package zoo

abstract class Animal(val name: String) {
    abstract fun sound(): String

    open fun describe(): String = \"$name says ${sound()}\"
}

interface Pet {
    val owner: String
    fun friendly(): Boolean = true
}

class Dog(name: String) : Animal(name), Pet {
    override fun sound(): String = \"Woof\"
}

class Cat(name: String, override val owner: String) : Animal(name), Pet {
    override fun sound(): String = \"Meow\"
}

sealed class Shape {
    data class Circle(val radius: Double) : Shape()
    data class Square(val side: Double) : Shape()
}

class Person(val name: String, val age: Int)
";

const MAIN: &str = "package zoo

fun area(shape: Shape): Double = when (shape) {
    is Shape.Circle -> 3.14 * shape.radius * shape.radius
    is Shape.Square -> shape.side * shape.side
}

fun total(shapes: List<Shape>): Double = shapes.sumOf { area(it) }

fun main() {
    val cat = Cat(\"Tom\", \"Ann\")
    println(cat.describe())
    println(total(listOf(Shape.Circle(2.0), Shape.Square(3.0))))
    val any: Any = cat
    if (any is Animal) {
        println(any.sound())
    }
}
";

/// The project of OBS-0035 in a fresh folder: (root, Animals.kt, Main.kt).
fn project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("idecraft-kt-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let src = root.join("src/main/kotlin/zoo");
    std::fs::create_dir_all(&src).unwrap();
    // A copy in the build folder must not count as a second project type.
    std::fs::create_dir_all(root.join("build/generated")).unwrap();
    std::fs::write(
        root.join("build/generated/Stale.kt"),
        "package zoo\nclass Stale : Animal(\"x\")\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    let src = root.join("src/main/kotlin/zoo");
    std::fs::write(src.join("Animals.kt"), ANIMALS).unwrap();
    std::fs::write(src.join("Main.kt"), MAIN).unwrap();
    (root.clone(), src.join("Animals.kt"), src.join("Main.kt"))
}

fn names(n: &CallNode) -> String {
    let mark = n.mark.clone().unwrap_or_default();
    if n.children.is_empty() {
        format!("{mark}{}", n.name)
    } else {
        let below: Vec<String> = n.children.iter().map(names).collect();
        format!("{mark}{}[{}]", n.name, below.join(" "))
    }
}

/// The Java entry points answer for `.kt` files without any language server.
#[tokio::test]
async fn kotlin_hierarchies_members_and_marks_without_a_server() {
    let (root, animals, _) = project("structure");
    let wait = Duration::from_secs(5);
    let none = |_: &Path| None;

    // ⌃H on Animal (OBS-0035 item 9): Any › * Animal › Dog, Cat.
    let tree = type_hierarchy_in(&root, &animals, None, 2, 16, TypeDirection::Class, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(names(&tree), "Any[*Animal[Dog Cat]]");
    assert_eq!(tree.children[0].location.path, animals);
    assert_eq!(tree.children[0].kind, "class");
    let up = type_hierarchy_in(
        &root,
        &animals,
        None,
        13,
        7,
        TypeDirection::Supertypes,
        wait,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(names(&up), "*Dog[Animal Pet]");
    assert_eq!(up.children[1].kind, "interface");
    // The editor's unsaved text wins over the disk.
    let edited = format!(
        "{ANIMALS}\nclass Fox : Animal(\"fox\") {{\n    override fun sound() = \"?\"\n}}\n"
    );
    let down = type_hierarchy_in(
        &root,
        &animals,
        Some(&edited),
        2,
        16,
        TypeDirection::Subtypes,
        wait,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(names(&down), "*Animal[Dog Cat Fox]");

    let (method, tree) = method_hierarchy_in(&root, &animals, None, 9, 9, &none, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(method, "owner");
    assert_eq!(names(&tree), "+Pet[!Dog +Cat]");
    let s = super_target_in(&root, &animals, None, 14, 20, &none, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.name, "Animal.sound()");
    assert_eq!(s.location.unwrap().range.start_line, 3);

    // ⌃I on Dog (OBS-0035 item 8): `owner: String` of zoo.Pet is the one member to implement.
    let (ty, methods) = overridable_methods_in(&root, &animals, None, 13, 7, wait)
        .await
        .unwrap();
    assert_eq!(ty, "zoo.Dog");
    let todo: Vec<_> = methods
        .iter()
        .filter(|m| m.unimplemented)
        .cloned()
        .collect();
    assert_eq!(todo.len(), 1);
    assert_eq!(
        (todo[0].label().as_str(), todo[0].declaring_class.as_str()),
        ("owner: String", "zoo.Pet")
    );
    let files = add_overridable_methods_in(&root, &animals, None, 13, 7, &todo, wait)
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    let new = idecraft_languages::rename::apply_edits(ANIMALS, &files[0].edits).unwrap();
    assert!(
        new.contains(
            "    override fun sound(): String = \"Woof\"\n\n    override val owner: String\n        get() = TODO(\"Not yet implemented\")\n}\n"
        ),
        "{new}"
    );

    // Gutter marks (OBS-0035 item 5).
    let marks = declaration_marks_in(&root, &animals, None, wait)
        .await
        .unwrap();
    let at = |line: u64| -> Vec<MarkKind> {
        marks
            .iter()
            .filter(|m| m.line == line)
            .map(|m| m.kind)
            .collect()
    };
    assert_eq!(at(2), [MarkKind::Inherited]);
    assert_eq!(at(3), [MarkKind::Implemented]);
    assert_eq!(at(8), [MarkKind::Inherited]);
    assert_eq!(at(9), [MarkKind::Implemented]);
    assert_eq!(at(14), [MarkKind::Implements]);
    assert_eq!(at(18), [MarkKind::Implements]);
    assert_eq!(at(21), [MarkKind::Inherited]);

    // Files changed or added on disk are read again; unchanged ones come from the cache.
    std::fs::write(&animals, &edited).unwrap();
    std::fs::write(
        animals.with_file_name("Bird.kt"),
        "package zoo\n\nclass Bird : Animal(\"bird\") {\n    override fun sound() = \"Tweet\"\n}\n",
    )
    .unwrap();
    let down = type_hierarchy_in(&root, &animals, None, 2, 16, TypeDirection::Subtypes, wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(names(&down), "*Animal[Dog Cat Fox Bird]");
    let _ = std::fs::remove_dir_all(&root);
}

fn kotlin_server_installed() -> bool {
    let found = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join("kotlin-language-server").is_file())
    });
    if !found {
        // Optional like the other real-server Kotlin test: CI has no kotlin-language-server.
        eprintln!("skipped: kotlin-language-server not installed (optional)");
    }
    found
}

/// Usage counts of members with what is below them, through the real server's references.
#[tokio::test]
async fn kotlin_usage_counts_with_the_real_server() {
    if !kotlin_server_installed() {
        return;
    }
    let (root, animals, main) = project("counts");
    idecraft_core::trust::trust_for_process(&root);
    let wait = Duration::from_secs(120);
    // Asked once: the question waits for the server's diagnostics of the file, which it
    // publishes once it has compiled it (SPEC-0042 v20 behaviour 35), whatever the load.
    let counts = idecraft_languages::usages::usage_counts_in(&root, &animals, None, wait)
        .await
        .unwrap();
    let row = |name: &str, line: u64| {
        counts
            .iter()
            .find(|c| c.name == name && c.line == line)
            .unwrap_or_else(|| panic!("{name} in {counts:?}"))
    };
    let sound = row("sound", 3);
    assert_eq!(sound.kind, "abstract function");
    assert!(sound.count >= 2, "{sound:?}");
    assert_eq!(sound.inheritors, 2);
    assert!(
        sound.label().ends_with("· 2 implementations"),
        "{}",
        sound.label()
    );
    let animal = row("Animal", 2);
    assert!(animal.count >= 2, "{animal:?}");
    assert_eq!(animal.inheritors, 2);
    assert_eq!(row("Pet", 8).inheritors, 2);
    assert_eq!(row("owner", 9).inheritors, 1);
    assert_eq!(row("Shape", 21).inheritors, 2);
    assert!(row("describe", 5).count >= 1);
    // A function at file level (OBS-0035 item 12).
    let in_main = idecraft_languages::usages::usage_counts_in(&root, &main, None, wait)
        .await
        .unwrap();
    let total = in_main
        .iter()
        .find(|c| c.name == "total")
        .unwrap_or_else(|| panic!("total in {in_main:?}"));
    assert_eq!((total.kind, total.count), ("function", 1));
    // Declarations nobody uses and nothing overrides show nothing.
    assert!(!in_main.iter().any(|c| c.name == "main"), "{in_main:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Parameter names before literal arguments, from the real server's inlay hints.
#[tokio::test]
async fn kotlin_parameter_hints_with_the_real_server() {
    if !kotlin_server_installed() {
        return;
    }
    let (root, _, main) = project("hints");
    idecraft_core::trust::trust_for_process(&root);
    let wait = Duration::from_secs(120);
    // Asked once, as the tokens and counts are (SPEC-0042 v20 behaviour 35).
    let hints = idecraft_languages::inlay::parameter_hints_in(&root, &main, None, wait)
        .await
        .unwrap();
    let found: Vec<(u64, u64, &str)> = hints
        .iter()
        .map(|h| (h.line, h.character, h.label.as_str()))
        .collect();
    // `Cat("Tom", "Ann")`, `Shape.Circle(2.0)` and `Shape.Square(3.0)`; nothing before
    // variables, calls or the only argument of a function.
    assert_eq!(
        found,
        [
            (10, 18, "name:"),
            (10, 25, "owner:"),
            (12, 38, "radius:"),
            (12, 57, "side:"),
        ]
    );
    // The server asks for a space on both sides; after `(` and after `, ` none stands before
    // the name (SPEC-0291 v6 behaviour 4a).
    assert!(
        hints.iter().all(|h| !h.padding_left && h.padding_right),
        "{hints:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The real server's semantic tokens with the Java keys and the syntax tree's corrections
/// (`SPEC-0290` v3 AK-7): a declared function, a called one, a property read through a receiver,
/// the constructor's `val` as a field and a local `val` as a variable.
#[tokio::test]
async fn kotlin_semantic_tokens_with_the_real_server() {
    if !kotlin_server_installed() {
        return;
    }
    let (root, animals, main) = project("tokens");
    idecraft_core::trust::trust_for_process(&root);
    let wait = Duration::from_secs(120);
    // Asked once: before the server has compiled the file it gives the tokens of the file's own
    // declarations only, without the references; the question waits for the diagnostics it
    // publishes after compiling (SPEC-0042 v20 behaviour 35), whatever the load.
    let ask = async |file: &std::path::Path| {
        idecraft_languages::semantic::semantic_tokens_in(&root, file, None, wait)
            .await
            .unwrap()
    };
    let key = |tokens: &[idecraft_languages::semantic::SemanticToken], text: &str, needle: &str| {
        let at = text.find(needle).unwrap_or_else(|| panic!("{needle}"));
        let line = text[..at].matches('\n').count() as u64;
        let start = text[..at].rsplit('\n').next().unwrap().chars().count() as u64;
        tokens
            .iter()
            .find(|t| t.line == line && t.start == start)
            .map(|t| (t.token_type.clone(), t.key))
            .unwrap_or_else(|| panic!("no token at {needle}: {tokens:?}"))
    };
    let in_main = ask(&main).await;
    let function = |key| ("function".to_owned(), Some(key));
    assert_eq!(
        key(&in_main, MAIN, "area(shape"),
        function("METHOD_DECLARATION_ATTRIBUTES")
    );
    assert_eq!(
        key(&in_main, MAIN, "describe())"),
        function("METHOD_CALL_ATTRIBUTES")
    );
    assert_eq!(
        key(&in_main, MAIN, "radius * shape.radius"),
        ("property".to_owned(), Some("INSTANCE_FIELD_ATTRIBUTES"))
    );
    // A local `val`: the server says property, the tree says variable.
    assert_eq!(
        key(&in_main, MAIN, "cat = Cat"),
        ("variable".to_owned(), Some("LOCAL_VARIABLE_ATTRIBUTES"))
    );
    let in_animals = ask(&animals).await;
    // A `val` of the primary constructor: the server says parameter, the tree says property.
    assert_eq!(
        key(&in_animals, ANIMALS, "name: String) {"),
        ("property".to_owned(), Some("INSTANCE_FIELD_ATTRIBUTES"))
    );
    // `$name` in the template is one token, the name's.
    assert_eq!(
        key(&in_animals, ANIMALS, "name says"),
        ("property".to_owned(), Some("INSTANCE_FIELD_ATTRIBUTES"))
    );
    assert!(
        !in_animals.iter().any(|t| {
            let line = ANIMALS.lines().nth(t.line as usize).unwrap_or_default();
            line.chars().nth(t.start as usize) == Some('$')
        }),
        "{in_animals:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
