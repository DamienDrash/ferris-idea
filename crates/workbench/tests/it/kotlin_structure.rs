//! Window check of OBS-0035 L4–L6: in a Kotlin file Type Hierarchy (⌃H) fills the Hierarchy tool
//! window, Implement Methods (⌃I) and Override Methods (⌃O) open their choosers and add the
//! chosen members, Method Hierarchy and Go to Super Method work, and Generate lists the two
//! choosers (`SPEC-0082` v6 AK-13, `SPEC-0122` v6 AK-20, `SPEC-0132` v3 AK-6). No language server
//! takes part.

#![cfg(target_os = "macos")]

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{ChoiceView, WorkbenchView};

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

class Person(val name: String, val age: Int)
";

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-kt-win-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/zoo")).unwrap();
    std::fs::write(root.join("src/zoo/Animals.kt"), ANIMALS).unwrap();
    std::fs::write(root.join("notes.txt"), "plain\n").unwrap();
    root.canonicalize().unwrap()
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join("src/zoo/Animals.kt");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

/// Puts the caret at a 0-based line and column of the active editor.
fn caret(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, line: usize, column: usize) {
    let offset: usize = ANIMALS
        .split_inclusive('\n')
        .take(line)
        .map(|l| l.chars().count())
        .sum::<usize>()
        + column;
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    editor.update(cx, |e, cx| e.select_range(offset..offset, cx));
    cx.run_until_parked();
}

/// Runs an action by its id and waits until background work has settled.
fn act(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    for _ in 0..200 {
        cx.run_until_parked();
        let busy = bench.read_with(cx, |b, cx| {
            b.status_message(cx).is_some_and(|s| s.ends_with('…'))
        });
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

fn chooser(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<ChoiceView> {
    bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the chooser is open")
}

/// The actions keep the keys of the reference's macOS keymap.
#[test]
fn the_actions_have_their_keys() {
    let keys = idecraft_workbench::keymaps::current();
    assert_eq!(keys.shortcut_label("TypeHierarchy", true), "⌃H");
    assert_eq!(keys.shortcut_label("ImplementMethods", true), "⌃I");
    assert_eq!(keys.shortcut_label("OverrideMethods", true), "⌃O");
}

#[gpui::test]
fn type_hierarchy_in_a_kotlin_file(cx: &mut TestAppContext) {
    let root = workspace("hierarchy");
    let (bench, cx) = open(&root, cx);
    caret(&bench, cx, 2, 16);
    act(&bench, cx, "TypeHierarchy");
    let view = bench
        .read_with(cx, |b, _| b.hierarchy())
        .expect("the Hierarchy tool window is open");
    let rows = view.read_with(cx, |v, _| v.marked());
    assert_eq!(rows, ["Any", "* Animal", "Dog", "Cat"]);
    let status = bench.read_with(cx, |b, cx| b.status_message(cx));
    assert!(
        !status.as_deref().unwrap_or_default().contains("Java"),
        "{status:?}"
    );

    // Method Hierarchy on Pet.owner: Dog must define it, Cat does.
    caret(&bench, cx, 9, 9);
    act(&bench, cx, "MethodHierarchy");
    let view = bench.read_with(cx, |b, _| b.hierarchy()).unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.marked()),
        ["+ Pet", "! Dog", "+ Cat"]
    );

    // Go to Super Method from Cat.sound() lands on Animal.sound().
    caret(&bench, cx, 18, 20);
    act(&bench, cx, "GotoSuperMethod");
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    let at = editor.read_with(cx, |e, _| {
        e.state.buffer.offset_to_position(e.caret()).unwrap()
    });
    assert_eq!((at.line, at.column), (3, 17));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn implement_and_override_members_in_a_kotlin_file(cx: &mut TestAppContext) {
    let root = workspace("implement");
    let (bench, cx) = open(&root, cx);

    // ⌃I on Dog: the one abstract member, checked.
    caret(&bench, cx, 13, 7);
    act(&bench, cx, "ImplementMethods");
    let view = chooser(&bench, cx);
    let (title, items, checked) = view.read_with(cx, |v, _| {
        (v.title().to_owned(), v.items().to_vec(), v.checked())
    });
    assert_eq!(title, "Implement Members");
    assert_eq!(items, ["zoo.Pet", "owner: String"]);
    assert_eq!(checked, [1]);
    view.update(cx, |v, cx| cx.emit(v.accept()));
    for _ in 0..200 {
        cx.run_until_parked();
        if text(&bench, cx) != ANIMALS {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let after = text(&bench, cx);
    assert!(
        after.contains(
            "class Dog(name: String) : Animal(name), Pet {\n    override fun sound(): String = \"Woof\"\n\n    override val owner: String\n        get() = TODO(\"Not yet implemented\")\n}\n"
        ),
        "{after}"
    );
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_none());

    // Nothing is left to implement. The caret stands in Dog's body, away from the class name, where
    // a language server's problem would take the status bar.
    caret(&bench, cx, 14, 20);
    act(&bench, cx, "ImplementMethods");
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_none());
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)).as_deref(),
        Some("No members to implement")
    );

    // ⌃O on Person, which has no body: the members of kotlin.Any, the first one checked.
    let line = after
        .lines()
        .position(|l| l.starts_with("class Person"))
        .unwrap();
    let offset: usize = after
        .split_inclusive('\n')
        .take(line)
        .map(|l| l.chars().count())
        .sum::<usize>()
        + 7;
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    editor.update(cx, |e, cx| e.select_range(offset..offset, cx));
    cx.run_until_parked();
    act(&bench, cx, "OverrideMethods");
    let view = chooser(&bench, cx);
    let (title, items, checked) = view.read_with(cx, |v, _| {
        (v.title().to_owned(), v.items().to_vec(), v.checked())
    });
    assert_eq!(title, "Override Members");
    assert_eq!(
        items,
        [
            "kotlin.Any",
            "equals(Any?): Boolean",
            "hashCode(): Int",
            "toString(): String"
        ]
    );
    assert_eq!(checked, [1]);
    view.update(cx, |v, cx| {
        v.toggle(1);
        v.toggle(3);
        cx.emit(v.accept());
    });
    for _ in 0..200 {
        cx.run_until_parked();
        if text(&bench, cx) != after {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let done = text(&bench, cx);
    assert!(
        done.ends_with(
            "class Person(val name: String, val age: Int) {\n    override fun toString(): String {\n        return super.toString()\n    }\n}\n"
        ),
        "{done}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn generate_lists_the_choosers_and_other_files_are_told(cx: &mut TestAppContext) {
    let root = workspace("generate");
    let (bench, cx) = open(&root, cx);
    caret(&bench, cx, 21, 7);
    act(&bench, cx, "Generate");
    let view = chooser(&bench, cx);
    let items = view.read_with(cx, |v, _| v.items().to_vec());
    assert_eq!(
        items,
        [
            "Secondary Constructor",
            "equals() and hashCode()",
            "toString()",
            "Override Methods…",
            "Implement Methods…"
        ]
    );
    // Choosing "Override Methods…" opens the chooser of members.
    view.update(cx, |v, cx| {
        v.select(3);
        cx.emit(v.accept());
    });
    for _ in 0..200 {
        cx.run_until_parked();
        let open = bench.read_with(cx, |b, cx| {
            b.jvm_popup()
                .is_some_and(|v| v.read(cx).title() == "Override Members")
        });
        if open {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        chooser(&bench, cx).read_with(cx, |v, _| v.title().to_owned()),
        "Override Members"
    );

    // A file that is neither Java nor Kotlin says what the action needs.
    let notes = root.join("notes.txt");
    bench.update_in(cx, |b, window, cx| {
        b.open(&notes, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    act(&bench, cx, "TypeHierarchy");
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)).as_deref(),
        Some("Type Hierarchy works in Java and Kotlin files")
    );
    let _ = std::fs::remove_dir_all(&root);
}
