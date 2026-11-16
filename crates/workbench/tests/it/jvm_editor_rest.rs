//! Window checks of the small Java and Kotlin editor gaps of OBS-0027 (L12, L19, L21) and
//! OBS-0035 (the gutter mark lost after Implement Members): the import block is folded when a
//! file opens (`SPEC-0083` v4 AK-10), tabs and tree show the kind of the primary declaration
//! (`SPEC-0131` v3 AK-7), the list of context actions closes the problem's popup (`SPEC-0076` v5
//! AK-6), and Kotlin's gutter marks follow an edit (`SPEC-0320` v4 AK-13). No language server
//! takes part.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::{EditorView, WorkbenchView};

const ANIMALS: &str = "package zoo

abstract class Animal(val name: String) {
    abstract fun sound(): String
}

interface Pet {
    val owner: String
}

class Dog(name: String) : Animal(name), Pet {
    override fun sound() = \"woof\"
}

class Cat(name: String) : Animal(name) {
    override fun sound() = \"meow\"
}
";

const MAIN_JAVA: &str = "package shop;

import java.util.ArrayList;
import java.util.List;

import static java.util.Map.entry;

public class Main {
    List<String> names = new ArrayList<>();
}
";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-jvm-rest-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/zoo")).unwrap();
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    for (name, text) in [
        ("src/zoo/Animals.kt", ANIMALS),
        (
            "src/zoo/Main.kt",
            "package zoo\n\nimport kotlin.math.max\nimport kotlin.math.min\n\nfun main() { println(max(1, min(2, 3))) }\n",
        ),
        (
            "src/zoo/Registry.kt",
            "package zoo\n\nobject Registry {\n    val all = mutableListOf<String>()\n}\n",
        ),
        ("src/shop/Main.java", MAIN_JAVA),
        (
            "src/shop/One.java",
            "package shop;\n\nimport java.util.List;\n\nclass One {\n    List<String> a;\n}\n",
        ),
        (
            "src/shop/Pet.java",
            "package shop;\n\npublic interface Pet {\n    String owner();\n}\n",
        ),
        (
            "src/shop/Circle.java",
            "package shop;\n\npublic record Circle(double radius) {}\n",
        ),
        (
            "src/shop/Marker.java",
            "package shop;\n\npublic @interface Marker {}\n",
        ),
        ("app.properties", "greeting=hello   \nname=x\n"),
    ] {
        std::fs::write(root.join(name), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    (bench, cx)
}

fn open(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    file: &Path,
) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

/// Waits until `done` holds, letting background work and timers run.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

#[gpui::test]
fn the_import_block_is_folded_when_a_file_opens(cx: &mut TestAppContext) {
    let root = workspace("imports");
    let (bench, cx) = window(&root, cx);

    // Java: three imports in two groups fold to the first import's line.
    let java = open(&bench, cx, &root.join("src/shop/Main.java"));
    until(cx, "the Java imports fold", |cx| {
        !java.read_with(cx, |e, _| e.folded_regions()).is_empty()
    });
    assert_eq!(java.read_with(cx, |e, _| e.folded_regions()), [(2, 5)]);
    assert_eq!(
        java.read_with(cx, |e, _| e.state.buffer.text()),
        MAIN_JAVA,
        "folding changes no text"
    );
    // Expand All opens them like any fold.
    java.update(cx, |e, cx| {
        e.run_action("ExpandAllRegions", cx);
    });
    assert!(java.read_with(cx, |e, _| e.folded_regions()).is_empty());

    // Kotlin too.
    let kotlin = open(&bench, cx, &root.join("src/zoo/Main.kt"));
    until(cx, "the Kotlin imports fold", |cx| {
        !kotlin.read_with(cx, |e, _| e.folded_regions()).is_empty()
    });
    assert_eq!(kotlin.read_with(cx, |e, _| e.folded_regions()), [(2, 3)]);

    // One import is not worth a fold; a file without imports has none.
    let one = open(&bench, cx, &root.join("src/shop/One.java"));
    let pet = open(&bench, cx, &root.join("src/shop/Pet.java"));
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(one.read_with(cx, |e, _| e.folded_regions()).is_empty());
    assert!(pet.read_with(cx, |e, _| e.folded_regions()).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn the_setting_keeps_the_imports_open(cx: &mut TestAppContext) {
    let root = workspace("imports-off");
    let (bench, cx) = window(&root, cx);
    cx.update(|_, cx| {
        let mut settings = idecraft_workbench::appearance::ide_settings(cx);
        assert!(settings.editor.fold_imports, "on by default");
        settings.editor.fold_imports = false;
        cx.set_global(idecraft_workbench::appearance::CurrentIdeSettings(settings));
    });
    let java = open(&bench, cx, &root.join("src/shop/Main.java"));
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(java.read_with(cx, |e, _| e.folded_regions()).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn tabs_and_tree_show_the_kind_of_the_declaration(cx: &mut TestAppContext) {
    let root = workspace("icons");
    let (bench, cx) = window(&root, cx);
    let icon = |cx: &mut VisualTestContext, name: &str| {
        let path = root.join(name);
        bench.read_with(cx, |b, cx| b.icon_of(&path, cx))
    };
    // Files that are not open: read from disk, as the project tree shows them.
    assert_eq!(icon(cx, "src/shop/Main.java"), "nodes/class");
    assert_eq!(icon(cx, "src/shop/Pet.java"), "nodes/interface");
    assert_eq!(icon(cx, "src/shop/Circle.java"), "idecraft/record");
    assert_eq!(icon(cx, "src/shop/Marker.java"), "idecraft/annotation");
    assert_eq!(icon(cx, "src/zoo/Registry.kt"), "idecraft/object");
    // A Kotlin file with several declarations, or with functions, is a file.
    assert_eq!(icon(cx, "src/zoo/Animals.kt"), "fileTypes/kotlin");
    assert_eq!(icon(cx, "src/zoo/Main.kt"), "fileTypes/kotlin");
    assert_eq!(icon(cx, "app.properties"), "fileTypes/properties");

    // An open file: the tab follows the editor's text, saved or not.
    let pet = open(&bench, cx, &root.join("src/shop/Pet.java"));
    assert_eq!(icon(cx, "src/shop/Pet.java"), "nodes/interface");
    pet.update(cx, |e, cx| {
        e.set_text("package shop;\n\npublic enum Pet { CAT, DOG }\n", cx)
    });
    cx.run_until_parked();
    assert_eq!(icon(cx, "src/shop/Pet.java"), "nodes/enum");
    pet.update(cx, |e, cx| e.set_text("package shop;\n", cx));
    cx.run_until_parked();
    assert_eq!(icon(cx, "src/shop/Pet.java"), "fileTypes/java");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn the_list_of_context_actions_closes_the_problems_popup(cx: &mut TestAppContext) {
    let root = workspace("tooltip");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("app.properties"));
    until(cx, "the trailing spaces are reported", |cx| {
        !editor.read_with(cx, |e, _| e.problems().is_empty())
    });
    let start = editor.read_with(cx, |e, _| e.problems()[0].range.start);
    editor.update(cx, |e, cx| e.select_range(start..start, cx));
    cx.run_until_parked();

    // The pointer rests on the problem: its text shows.
    bench.update(cx, |b, cx| b.rest_on_problem(Some(start), cx));
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.doc_blocks()).is_some(),
        "the problem's popup is open"
    );

    // ⌥↩: the list opens and the popup is gone.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ShowIntentionActions", window, cx)
    });
    until(cx, "the list of context actions opens", |cx| {
        bench.read_with(cx, |b, _| b.intentions_popup().is_some())
    });
    assert!(
        bench.read_with(cx, |b, _| b.doc_blocks()).is_none(),
        "the popup closed when the list opened"
    );

    // While the list is open the resting pointer opens nothing over it.
    bench.update(cx, |b, cx| b.rest_on_problem(Some(start), cx));
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.doc_blocks()).is_none());
    assert!(bench.read_with(cx, |b, _| b.intentions_popup().is_some()));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn kotlin_gutter_marks_follow_an_inserted_member(cx: &mut TestAppContext) {
    let root = workspace("marks");
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("src/zoo/Animals.kt"));
    let line_of =
        |text: &str, needle: &str| text[..text.find(needle).unwrap()].matches('\n').count();
    let dog_sound = line_of(ANIMALS, "override fun sound() = \"woof\"");
    let cat_sound = line_of(ANIMALS, "override fun sound() = \"meow\"");
    let glyph = |cx: &mut VisualTestContext, line: usize| {
        editor.read_with(cx, |e, _| e.declaration_mark(line).map(|m| m.glyph))
    };
    until(cx, "the marks of the opened file", |cx| {
        glyph(cx, dog_sound).is_some()
    });
    assert_eq!(glyph(cx, dog_sound), Some("↑"));
    assert_eq!(glyph(cx, cat_sound), Some("↑"));

    // Implement Members on Dog adds `owner` after `sound()`, on the lines below it.
    let dog = ANIMALS.find("Dog(").unwrap();
    let dog = ANIMALS[..dog].chars().count();
    editor.update(cx, |e, cx| e.select_range(dog..dog, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ImplementMethods", window, cx)
    });
    until(cx, "the Implement Members list", |cx| {
        bench.read_with(cx, |b, _| b.jvm_popup().is_some())
    });
    let chooser = bench.read_with(cx, |b, _| b.jvm_popup()).unwrap();
    chooser.update(cx, |v, cx| cx.emit(v.accept()));
    until(cx, "the member is added", |cx| {
        editor.read_with(cx, |e, _| e.state.buffer.text()) != ANIMALS
    });
    let text = editor.read_with(cx, |e, _| e.state.buffer.text());
    let owner = line_of(&text, "override val owner");
    assert!(owner > dog_sound, "{text}");
    // The mark of `Dog.sound()` is still there right away: the edit only added lines below it.
    assert_eq!(glyph(cx, dog_sound), Some("↑"), "{text}");

    // The marks are computed again for the new text: `owner` implements `Pet.owner`, and
    // `Cat.sound()` has moved down with its mark.
    let cat_now = line_of(&text, "override fun sound() = \"meow\"");
    until(cx, "the marks of the new text", |cx| {
        glyph(cx, owner).is_some()
    });
    assert_eq!(glyph(cx, owner), Some("↑"));
    assert_eq!(glyph(cx, dog_sound), Some("↑"));
    assert_eq!(glyph(cx, cat_now), Some("↑"));
    assert!(cat_now > cat_sound);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0083 v5 AK-12 (OBS-0027 L31): methods added the way jdtls adds them — the whitespace before
/// the class's closing brace replaced by the new member and that whitespace again — leave the
/// one-line method above folded. Applied through the editor as the workbench applies the edits of
/// Implement and Override Methods.
#[gpui::test]
fn a_one_line_method_stays_folded_when_methods_are_added(cx: &mut TestAppContext) {
    let root = workspace("added-methods");
    let dog = "package shop;\n\npublic class Dog implements Pet {\n    public String name() {\n        return \"Rex\";\n    }\n}\n";
    std::fs::write(root.join("src/shop/Dog.java"), dog).unwrap();
    let (bench, cx) = window(&root, cx);
    let java = open(&bench, cx, &root.join("src/shop/Dog.java"));
    until(cx, "the one-line method folds", |cx| {
        !java.read_with(cx, |e, _| e.folded_regions()).is_empty()
    });
    assert_eq!(java.read_with(cx, |e, _| e.folded_regions()), [(3, 5)]);
    let range = |sl, sc, el, ec| idecraft_languages::insight::Range {
        start_line: sl,
        start_character: sc,
        end_line: el,
        end_character: ec,
    };
    // From the end of the method's last line to the start of the closing brace's line.
    let edit = idecraft_languages::rename::Edit {
        range: range(5, 5, 6, 0),
        old_text: "\n".into(),
        new_text: "\n\n    @Override\n    public String owner() {\n        return null;\n    }\n"
            .into(),
    };
    java.update(cx, |e, cx| e.apply_text_edits(&[edit], cx));
    cx.run_until_parked();
    assert!(
        java.read_with(cx, |e, _| e.state.buffer.text())
            .contains("public String owner()")
    );
    assert_eq!(
        java.read_with(cx, |e, _| e.folded_regions()),
        [(3, 5)],
        "the method above the added one stays folded"
    );
    let _ = std::fs::remove_dir_all(&root);
}
