//! Hand tests 81, 89 and 152 (`SPEC-0155` v4 AK-8, `SPEC-0163` v2 AK-6, `SPEC-0212` v2 AK-5) in
//! a window of the test harness, IDECraft's side with real keys where the hand test types:
//! Update Copyright in a Java and a Python file, twice; a properties file's duplicate key and
//! trailing spaces marked and fixed with ⌥↩; a repeated word and a wrong article in a README fixed
//! with ⌥↩; a word saved to the project dictionary in the reference's format and a key missing in
//! a bundle's language marked; the Resource Bundle Editor writing one line per change. The
//! comparison with the reference stays in `docs/research/PLAN-F081.md`. Hand test 85 downloads a
//! dictionary and stays for a run with the network.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{EditorView, WorkbenchView};

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "idecraft-open-plugins-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    for (name, text) in files {
        let file = root.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
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
        .expect("the file is open")
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

/// Waits until the editor's inspections list a problem containing `what`.
fn problem(editor: &Entity<EditorView>, cx: &mut VisualTestContext, what: &str) -> bool {
    for _ in 0..200 {
        cx.run_until_parked();
        if editor.read_with(cx, |e, _| {
            e.problems().iter().any(|p| p.message.contains(what))
        }) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    false
}

/// ⌥↩ at `(line, column)` and Enter on the intention titled `title`, which comes first.
fn fix(
    bench: &Entity<WorkbenchView>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    at: (usize, usize),
    title: &str,
) {
    editor.update(cx, |e, cx| e.go_to(at.0, at.1, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "alt-enter");
    let titles = bench.read_with(cx, |b, cx| {
        b.intentions_popup()
            .map(|v| v.read(cx).titles())
            .unwrap_or_default()
    });
    assert_eq!(
        titles.first().map(String::as_str),
        Some(title),
        "{titles:?}"
    );
    keys(cx, "enter");
}

/// Hand test 81: Update Copyright twice in Java and Python, the properties marks and their
/// fixes, a repeated word and a wrong article.
#[gpui::test]
fn hand_test_81_copyright_properties_and_grammar(cx: &mut TestAppContext) {
    let root = workspace(
        "81",
        &[
            (
                ".idecraft/copyright.txt",
                "Copyright (c) $today.year Example\n",
            ),
            ("src/Main.java", "package app;\n\nclass Main {}\n"),
            ("tool.py", "print(1)\n"),
            ("messages.properties", "greeting=Hi  \ngreeting=Hello\n"),
            ("README.md", "Open the the file as an user.\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let year = idecraft_languages::copyright::current_year();

    // Code › Update Copyright…: the notice at the top; a second run changes nothing.
    let java = open(&bench, cx, &root.join("src/Main.java"));
    let run = |cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| {
            b.menu_action("UpdateCopyright", window, cx)
        });
        cx.run_until_parked();
    };
    run(cx);
    let once = text(&java, cx);
    assert!(
        once.starts_with(&format!("/*\n * Copyright (c) {year} Example\n */\n"))
            && once.ends_with("package app;\n\nclass Main {}\n"),
        "{once}"
    );
    run(cx);
    assert_eq!(text(&java, cx), once, "a second run changes nothing");
    let py = open(&bench, cx, &root.join("tool.py"));
    run(cx);
    let once = text(&py, cx);
    assert!(
        once.starts_with(&format!("# Copyright (c) {year} Example\n"))
            && once.ends_with("print(1)\n"),
        "{once}"
    );
    run(cx);
    assert_eq!(text(&py, cx), once);

    // Properties: the duplicate key and the trailing spaces are marked; ⌥↩ removes the spaces.
    let props = open(&bench, cx, &root.join("messages.properties"));
    assert!(problem(&props, cx, "Duplicate property key `greeting`"));
    assert!(problem(&props, cx, "Trailing spaces"));
    fix(&bench, &props, cx, (0, 12), "Remove trailing spaces");
    assert_eq!(text(&props, cx), "greeting=Hi\ngreeting=Hello\n");

    // Grammar: the repeated word and the article are marked; ⌥↩ corrects each.
    let readme = open(&bench, cx, &root.join("README.md"));
    assert!(problem(&readme, cx, "Repeated word"));
    assert!(problem(&readme, cx, "Use \"a\" before \"user\""));
    fix(&bench, &readme, cx, (0, 9), "Remove the repeated word");
    assert_eq!(text(&readme, cx), "Open the file as an user.\n");
    assert!(problem(&readme, cx, "Use \"a\""));
    fix(&bench, &readme, cx, (0, 17), "Replace with 'a'");
    assert_eq!(text(&readme, cx), "Open the file as a user.\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 89: a word saved to the project dictionary lands in the reference's file format;
/// a key missing in the bundle's German file is marked in the default file.
#[gpui::test]
fn hand_test_89_project_dictionary_and_bundle_keys(cx: &mut TestAppContext) {
    let root = workspace(
        "89",
        &[
            ("Messages.properties", "a=1\nb=2\n"),
            ("Messages_de.properties", "a=eins\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    bench.update(cx, |b, cx| b.learn_word("Frobnicate", cx));
    cx.run_until_parked();
    let dictionary =
        std::fs::read_to_string(root.join(".idea/dictionaries/project.xml")).expect("written");
    assert!(
        dictionary.contains("<component name=\"ProjectDictionaryState\">"),
        "{dictionary}"
    );
    assert!(
        dictionary.contains("<dictionary name=\"project\">"),
        "{dictionary}"
    );
    assert!(dictionary.contains("<w>frobnicate</w>"), "{dictionary}");
    let messages = open(&bench, cx, &root.join("Messages.properties"));
    assert!(problem(&messages, cx, "`b` is missing in the bundle's de"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 152: the Resource Bundle Editor lists every key, marks the missing one, writes one
/// line per change (a value typed with an umlaut escaped in an ASCII file), adds a key to the
/// default file and removes it from every file.
#[gpui::test]
fn hand_test_152_resource_bundle_editor(cx: &mut TestAppContext) {
    let root = workspace(
        "152",
        &[
            ("messages.properties", "hello=Hello\nbye=Bye\n"),
            ("messages_de.properties", "hello=Hallo\n"),
            ("messages_fr.properties", "hello=Bonjour\nbye=Au revoir\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("messages_de.properties"));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("OpenResourceBundleEditor", window, cx)
    });
    cx.run_until_parked();
    let editor = bench
        .read_with(cx, |b, _| b.bundle_editor())
        .expect("the Resource Bundle Editor is open");
    assert_eq!(
        editor.read_with(cx, |v, _| v.title().to_owned()),
        "Resource Bundle 'messages'"
    );
    assert_eq!(
        editor.read_with(cx, |v, cx| v.visible_keys(cx)),
        ["hello", "bye"]
    );
    let missing = editor.read_with(cx, |v, _| v.bundle().keys[1].missing().to_vec());
    assert_eq!(
        missing.len(),
        1,
        "bye is missing in one language: {missing:?}"
    );

    // The missing German value typed with real keys, Enter.
    editor.update(cx, |v, cx| v.select("bye", cx));
    let de = editor
        .read_with(cx, |v, _| {
            let at = v
                .bundle()
                .locales
                .iter()
                .position(|l| l == "de")
                .expect("de");
            v.value_field(at)
        })
        .expect("a field for de");
    bench.update_in(cx, |_, window, cx| window.focus(&de.read(cx).focus()));
    cx.simulate_input("Tschüss");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(
        bench.read_with(cx, |b, cx| b.active_text(cx)),
        Some("hello=Hallo\nbye=Tsch\\u00FCss\n".to_owned()),
        "one new line, the umlaut escaped as the file is ASCII"
    );

    // A new key goes into the default file only; Delete twice removes it everywhere.
    bench.update_in(cx, |b, window, cx| {
        if let Some(v) = b.bundle_editor() {
            v.update(cx, |v, cx| {
                v.start_new_key(window, cx);
                v.add_key("farewell", cx);
            });
        }
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(root.join("messages.properties")).unwrap(),
        "hello=Hello\nbye=Bye\nfarewell=\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("messages_fr.properties")).unwrap(),
        "hello=Bonjour\nbye=Au revoir\n"
    );
    bench.update_in(cx, |_, window, cx| window.focus(&editor.read(cx).focus()));
    keys(cx, "delete");
    assert!(editor.read_with(cx, |v, _| {
        v.note()
            .is_some_and(|n| n.starts_with("Press Delete again"))
    }));
    keys(cx, "delete");
    assert_eq!(
        std::fs::read_to_string(root.join("messages.properties")).unwrap(),
        "hello=Hello\nbye=Bye\n"
    );
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.bundle_editor().is_none()));
    let _ = std::fs::remove_dir_all(&root);
}
