//! The file type actions in a window of the test harness (`SPEC-0004` v5 AK-6), with real keys:
//! a script's hashbang line, Override File Type from Find Action, Revert File Type Override,
//! Associate with File Type, the Project tree's selection, and the items of the tab and tree
//! menus.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    Entity, EntityInputHandler, Modifiers, MouseButton, TestAppContext, VisualTestContext, px,
};
use idecraft_languages::LanguageId;
use idecraft_languages::file_types::{Associations, Choice, FILE};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{ChoiceView, EditorView, MenuPlace, WorkbenchView, semantics};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "idecraft-file-types-window-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    root.canonicalize().unwrap()
}

/// A window on `root` with settings of its own: the user's settings are never read or written.
fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    idecraft_workbench::hit_targets::enable();
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

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench.read_with(cx, |b, _| {
        b.active_editor_entity().expect("the file is open")
    })
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn language(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |e, _| e.language_id().map(str::to_owned))
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

fn saved(root: &Path) -> Associations {
    Associations::read(root).unwrap()
}

/// ⇧⌘A, the action's title typed, Enter once it is the first row.
fn find_action(bench: &Bench, cx: &mut VisualTestContext, title: &str, id: &str) {
    keys(cx, "cmd-shift-a");
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("Find Action is open");
    cx.simulate_input(title);
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let first = popup.read_with(cx, |v, _| {
            let r = v.results();
            r.selected
                .and_then(|i| r.rows.get(i))
                .map(|r| format!("{r:?}"))
                .unwrap_or_default()
        });
        if first.contains(id) {
            break;
        }
        assert!(Instant::now() < end, "{title} is the first row: {first}");
        std::thread::sleep(Duration::from_millis(10));
    }
    keys(cx, "enter");
}

/// The Find Action rows for `query` (their debug text, which names the action id), once the
/// always offered Associate with File Type… is among them; Escape closes the popup again.
fn offered_actions(bench: &Bench, cx: &mut VisualTestContext, query: &str) -> Vec<String> {
    keys(cx, "cmd-shift-a");
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("Find Action is open");
    cx.simulate_input(query);
    let end = Instant::now() + Duration::from_secs(10);
    let rows = loop {
        cx.run_until_parked();
        let rows: Vec<String> = popup.read_with(cx, |v, _| {
            v.results().rows.iter().map(|r| format!("{r:?}")).collect()
        });
        if rows.iter().any(|r| r.contains("AssociateWithFileType")) {
            break rows;
        }
        assert!(Instant::now() < end, "Find Action lists {query}: {rows:?}");
        std::thread::sleep(Duration::from_millis(10));
    };
    keys(cx, "escape");
    rows
}

/// The open file type list.
fn list(bench: &Bench, cx: &mut VisualTestContext) -> Entity<ChoiceView> {
    bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the list of file types is open")
}

/// The typed text of the open list.
fn typed(bench: &Bench, cx: &mut VisualTestContext) -> String {
    list(bench, cx).read_with(cx, |p, _| p.filter().unwrap_or_default().to_owned())
}

/// The label of the selected row of the open list.
fn selected_row(bench: &Bench, cx: &mut VisualTestContext) -> String {
    list(bench, cx).read_with(cx, |p, _| p.items()[p.selected()].clone())
}

/// A key event whose key is `key` (and whose layout character is `key_char`), followed by the
/// text the platform's text input commits for it, `committed`: what synthetic input with a
/// Unicode string, a dead key or an input method sends on macOS (SPEC-0004 v6 behaviour 17).
fn key_then_commit(
    bench: &Bench,
    cx: &mut VisualTestContext,
    key: &str,
    key_char: &str,
    committed: &str,
) {
    let event = gpui::KeyDownEvent {
        keystroke: gpui::Keystroke {
            modifiers: Modifiers::none(),
            key: key.into(),
            key_char: Some(key_char.into()),
        },
        is_held: false,
    };
    // The key event as the platform delivers it, without the text that follows it.
    cx.simulate_event(event);
    let popup = list(bench, cx);
    popup.update_in(cx, |p, window, cx| {
        p.replace_text_in_range(None, committed, window, cx)
    });
    cx.run_until_parked();
}

/// SPEC-0004 v6 AK-11 (OBS-0052 G3, G4): the list of file types takes the committed text, not the
/// key behind it — key events of the key `a` committing `H`, `T`, `M`, `L` give `HTML` with the
/// exact match selected; upper case and German letters typed with real keys; ↑/↓, ⌫ and Escape
/// keep their meaning; every row has its icon.
#[gpui::test]
fn typed_text_reaches_the_file_type_list(cx: &mut TestAppContext) {
    let root = workspace("committed", &[("page.tpl", "<p>hi</p>\n")]);
    let (bench, cx) = window(&root, cx);
    let page = open(&bench, cx, &root.join("page.tpl"));
    find_action(&bench, cx, "override file type", "OverrideFileTypeAction");
    // Every row has an icon: the text icon for Plain Text, the language's for the others.
    let (items, icons) =
        list(&bench, cx).read_with(cx, |p, _| (p.items().to_vec(), p.icons().to_vec()));
    assert_eq!(items.len(), icons.len());
    assert_eq!(icons[0], "fileTypes/text");
    let html = items.iter().position(|i| i == "HTML").unwrap();
    assert_eq!(icons[html], "fileTypes/html");
    assert!(icons.iter().all(|i| *i != "fileTypes/unknown"), "{icons:?}");

    // The key is `a` each time (key code 0 on macOS); the committed text decides.
    for ch in ["H", "T", "M", "L"] {
        key_then_commit(&bench, cx, "a", "a", ch);
    }
    assert_eq!(typed(&bench, cx), "HTML");
    assert_eq!(list_rows(&bench, cx), ["HTML"]);
    assert_eq!(selected_row(&bench, cx), "HTML");
    // ⌫ takes the last character away; Escape clears the text and the list stays open.
    keys(cx, "backspace");
    assert_eq!(typed(&bench, cx), "HTM");
    keys(cx, "escape");
    assert_eq!(typed(&bench, cx), "");
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_some());

    // A German layout's key (`ö` where US keyboards have `;`) committing `ä`: the commit counts;
    // no file type has it, so no row is shown and Enter chooses nothing.
    key_then_commit(&bench, cx, ";", "ö", "ä");
    assert_eq!(typed(&bench, cx), "ä");
    assert!(list_rows(&bench, cx).is_empty());
    keys(cx, "enter");
    assert!(
        bench.read_with(cx, |b, _| b.jvm_popup()).is_some(),
        "Enter with no row does nothing"
    );
    keys(cx, "escape");

    // Real keys: upper case with Shift, `ü` as a German keyboard types it.
    cx.simulate_input("Rü");
    assert_eq!(typed(&bench, cx), "Rü");
    keys(cx, "backspace");
    assert_eq!(typed(&bench, cx), "R");
    // `R` is the exact match among the many rows that contain it.
    assert_eq!(selected_row(&bench, cx), "R");
    keys(cx, "escape");
    cx.simulate_input("C");
    assert_eq!(selected_row(&bench, cx), "C");
    keys(cx, "down");
    assert_ne!(selected_row(&bench, cx), "C", "↓ moves on");
    keys(cx, "up");
    assert_eq!(selected_row(&bench, cx), "C");
    keys(cx, "escape");
    cx.simulate_input("HTML");
    assert_eq!(typed(&bench, cx), "HTML");
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "page.tpl is treated as HTML");
    assert_eq!(language(&page, cx).as_deref(), Some("html"));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0004 v6 AK-9 (OBS-0052 G1): the icons of the Project tree and the tabs follow the
/// detected language — a hashbang script is Python; an override gives the language's icon and
/// the override mark at once; a pattern gives the icon without the mark; Revert brings the
/// name's icon back.
#[gpui::test]
fn icons_follow_the_detected_language(cx: &mut TestAppContext) {
    let root = workspace(
        "icons",
        &[
            ("runme", "#!/usr/bin/env python3\nprint(1)\n"),
            ("page.tpl", "<p>hi</p>\n"),
            ("notes.data", "x\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let icon = |bench: &Bench, cx: &mut VisualTestContext, path: &Path| {
        bench.read_with(cx, |b, cx| {
            (b.icon_of(path, cx), b.file_type_overridden(path))
        })
    };
    let script = root.join("runme");
    let page = root.join("page.tpl");
    let data = root.join("notes.data");
    assert_eq!(icon(&bench, cx, &script), ("fileTypes/python", false));
    assert_eq!(icon(&bench, cx, &page), ("fileTypes/unknown", false));
    open(&bench, cx, &page);
    find_action(&bench, cx, "override file type", "OverrideFileTypeAction");
    cx.simulate_input("html");
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "page.tpl is treated as HTML");
    // At once, not only after the second a decision is kept.
    assert_eq!(icon(&bench, cx, &page), ("fileTypes/html", true));
    // A pattern: the language's icon without the mark.
    open(&bench, cx, &data);
    find_action(
        &bench,
        cx,
        "associate with file type",
        "AssociateWithFileType",
    );
    cx.simulate_input("yaml");
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "*.data files are treated as YAML");
    assert_eq!(icon(&bench, cx, &data), ("fileTypes/yaml", false));
    // Revert: the name's icon again, no mark.
    open(&bench, cx, &page);
    find_action(
        &bench,
        cx,
        "revert file type override",
        "ReverteOverrideFileTypeAction",
    );
    assert_eq!(icon(&bench, cx, &page), ("fileTypes/unknown", false));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0004 v6 AK-10 (OBS-0052 G2): Find Action offers Override and not Revert for a file
/// without an override, Revert and not Override for one with it.
#[gpui::test]
fn find_action_offers_override_or_revert(cx: &mut TestAppContext) {
    let root = workspace("offered", &[("app.cfg", "a: 1\n")]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("app.cfg"));
    let has = |rows: &[String], id: &str| rows.iter().any(|r| r.contains(id));
    let rows = offered_actions(&bench, cx, "file type");
    assert!(has(&rows, "\"OverrideFileTypeAction\""), "{rows:?}");
    assert!(!has(&rows, "ReverteOverrideFileTypeAction"), "{rows:?}");
    find_action(&bench, cx, "override file type", "OverrideFileTypeAction");
    cx.simulate_input("yaml");
    keys(cx, "enter");
    let rows = offered_actions(&bench, cx, "file type");
    assert!(has(&rows, "ReverteOverrideFileTypeAction"), "{rows:?}");
    assert!(!has(&rows, "\"OverrideFileTypeAction\""), "{rows:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The rows of the open file type list.
fn list_rows(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench
        .read_with(cx, |b, _| b.jvm_popup())
        .expect("the list of file types is open")
        .read_with(cx, |p, _| p.shown_items())
}

/// SPEC-0004 AK-6: a script without extension is Python by its hashbang line; Override File Type
/// from Find Action makes `app.cfg` YAML (⌘/ comments with `#`, the unsaved text and the undo
/// step stay, the override is saved); Revert brings plain text back; Associate with File Type on
/// `page.tpl` saves `*.tpl` as HTML.
#[gpui::test]
fn override_revert_and_associate_with_keys(cx: &mut TestAppContext) {
    let root = workspace(
        "keys",
        &[
            ("deploy", "#!/usr/bin/env python3\nprint('hi')\n"),
            ("conf/app.cfg", "name: shop\nport: 80\n"),
            ("page.tpl", "<p>hi</p>\n"),
            ("notes.md", "# Notes\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);

    let script = open(&bench, cx, &root.join("deploy"));
    assert_eq!(language(&script, cx).as_deref(), Some("python"));

    let cfg = root.join("conf/app.cfg");
    let editor = open(&bench, cx, &cfg);
    assert_eq!(language(&editor, cx), None, "plain text by its name");
    // An unsaved change before the override.
    cx.simulate_input("debug: true\n");
    cx.run_until_parked();
    let typed = text(&editor, cx);
    assert_eq!(typed, "debug: true\nname: shop\nport: 80\n");

    find_action(&bench, cx, "override file type", "OverrideFileTypeAction");
    let rows = list_rows(&bench, cx);
    assert_eq!(rows[0], "Plain Text");
    assert!(rows.iter().any(|r| r == "YAML"), "{rows:?}");
    cx.simulate_input("yaml");
    cx.run_until_parked();
    assert_eq!(list_rows(&bench, cx), ["YAML"]);
    keys(cx, "enter");
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_none());
    assert_eq!(status(&bench, cx), "app.cfg is treated as YAML");
    assert_eq!(language(&editor, cx).as_deref(), Some("yaml"));
    assert_eq!(text(&editor, cx), typed, "the unsaved text stays");
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    assert_eq!(
        saved(&root).overrides.get("conf/app.cfg"),
        Some(&Choice::Language(LanguageId::Yaml))
    );
    assert_eq!(LanguageId::detect(&cfg), Some(LanguageId::Yaml));
    // ⌘/ comments the caret's line with YAML's `#`.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-/");
    assert_eq!(text(&editor, cx), "debug: true\n#name: shop\nport: 80\n");
    // ⌘Z takes the comment away, and the typing from before the override can be undone too.
    keys(cx, "cmd-z");
    assert_eq!(text(&editor, cx), typed);
    for _ in 0..5 {
        if text(&editor, cx) == "name: shop\nport: 80\n" {
            break;
        }
        keys(cx, "cmd-z");
    }
    assert_eq!(text(&editor, cx), "name: shop\nport: 80\n");

    // Revert File Type Override: plain text again, the file has no overrides left.
    find_action(
        &bench,
        cx,
        "revert file type override",
        "ReverteOverrideFileTypeAction",
    );
    assert_eq!(
        status(&bench, cx),
        "app.cfg is detected from its name again"
    );
    assert_eq!(language(&editor, cx), None);
    assert!(!root.join(FILE).exists(), "nothing is left to keep");
    // SPEC-0004 v6 behaviour 16: without an override Revert is no longer offered.
    let offered = offered_actions(&bench, cx, "file type");
    assert!(
        !offered
            .iter()
            .any(|r| r.contains("ReverteOverrideFileTypeAction")),
        "{offered:?}"
    );

    // Associate with File Type: `*.tpl` as HTML.
    let page = open(&bench, cx, &root.join("page.tpl"));
    assert_eq!(language(&page, cx), None);
    find_action(
        &bench,
        cx,
        "associate with file type",
        "AssociateWithFileType",
    );
    let rows = list_rows(&bench, cx);
    assert!(!rows.iter().any(|r| r == "Plain Text"), "{rows:?}");
    cx.simulate_input("html");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "*.tpl files are treated as HTML");
    assert_eq!(language(&page, cx).as_deref(), Some("html"));
    assert_eq!(
        saved(&root).patterns.get("*.tpl"),
        Some(&Choice::Language(LanguageId::Html))
    );
    // A file whose language is known: nothing to associate.
    open(&bench, cx, &root.join("notes.md"));
    find_action(
        &bench,
        cx,
        "associate with file type",
        "AssociateWithFileType",
    );
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_none());
    assert_eq!(status(&bench, cx), "notes.md already is Markdown");
    // Escape closes the list without a choice.
    find_action(&bench, cx, "override file type", "OverrideFileTypeAction");
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.jvm_popup()).is_none());
    assert!(saved(&root).overrides.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0004 AK-6: with the Project tree focused the override acts on both selected files; the
/// tree's and the tab's menus hold the three items with their enabled state.
#[gpui::test]
fn the_tree_selection_and_the_menus(cx: &mut TestAppContext) {
    let root = workspace(
        "tree",
        &[
            ("a.cfg", "a = 1\n"),
            ("b.cfg", "b = 2\n"),
            ("main.py", "print(1)\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let a = root.join("a.cfg");
    let editor = open(&bench, cx, &a);
    bench.update_in(cx, |b, window, cx| {
        assert!(b.reveal_in_tree(&a, cx), "a.cfg is in the tree");
        b.focus_tree(window, cx);
    });
    cx.run_until_parked();
    keys(cx, "shift-down");
    find_action(&bench, cx, "override file type", "OverrideFileTypeAction");
    cx.simulate_input("toml");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "2 files are treated as TOML");
    let overrides = saved(&root).overrides;
    assert_eq!(
        overrides.keys().cloned().collect::<Vec<_>>(),
        ["a.cfg", "b.cfg"]
    );
    assert_eq!(language(&editor, cx).as_deref(), Some("toml"));

    let menu = |bench: &Bench, cx: &mut VisualTestContext, place: MenuPlace, path: &Path| {
        bench.update(cx, |b, cx| {
            b.open_context_menu(
                place,
                Some(path.to_path_buf()),
                gpui::point(px(200.), px(200.)),
                cx,
            )
        });
        cx.run_until_parked();
        let (_, items) = bench
            .read_with(cx, |b, _| b.context_menu_items())
            .expect("the menu is open");
        // `None` when the menu does not hold the item (SPEC-0004 v6 behaviour 16).
        let state = |label: &str| items.iter().find(|(l, _)| l == label).map(|(_, on)| *on);
        (
            state("Associate with File Type…"),
            state("Override File Type"),
            state("Revert File Type Override"),
        )
    };
    // a.cfg has an override and a language now; main.py is Python by its name.
    assert_eq!(
        menu(&bench, cx, MenuPlace::ProjectTree, &a),
        (Some(false), None, Some(true))
    );
    let py = root.join("main.py");
    assert_eq!(
        menu(&bench, cx, MenuPlace::ProjectTree, &py),
        (Some(false), Some(true), None)
    );
    // Revert from the tree's menu on a.cfg alone (the selection is that row now).
    bench.update_in(cx, |b, window, cx| {
        assert!(b.reveal_in_tree(&a, cx));
        b.focus_tree(window, cx);
    });
    cx.run_until_parked();
    bench.update(cx, |b, cx| {
        b.open_context_menu(
            MenuPlace::ProjectTree,
            Some(a.clone()),
            gpui::point(px(200.), px(200.)),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(bench.update_in(cx, |b, window, cx| {
        b.choose_context_item("Revert File Type Override", window, cx)
    }));
    cx.run_until_parked();
    assert_eq!(status(&bench, cx), "a.cfg is detected from its name again");
    assert_eq!(
        saved(&root).overrides.keys().cloned().collect::<Vec<_>>(),
        ["b.cfg"]
    );
    assert_eq!(language(&editor, cx), None);
    // A secondary click on the tab of a.cfg: no override now, the type unknown.
    cx.update(|window, cx| window.draw(cx).clear());
    let r = idecraft_workbench::hit_targets::get("tab-0").expect("the tab is drawn");
    let at = gpui::point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    let (place, items) = bench
        .read_with(cx, |b, _| b.context_menu_items())
        .expect("the tab has a menu");
    assert!(
        matches!(place, MenuPlace::Tab { index: 0, .. }),
        "{place:?}"
    );
    let at = items
        .iter()
        .position(|(l, _)| l == "Rename File…")
        .expect("Rename File… in the tab menu");
    assert_eq!(
        items[at + 1..],
        [
            ("Associate with File Type…".to_owned(), true),
            ("Override File Type".to_owned(), true),
        ]
    );
    // Override File Type from the tab menu acts on the tab's file.
    assert!(bench.update_in(cx, |b, window, cx| {
        b.choose_context_item("Override File Type", window, cx)
    }));
    cx.run_until_parked();
    cx.simulate_input("json");
    cx.run_until_parked();
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "a.cfg is treated as JSON");
    assert_eq!(language(&editor, cx).as_deref(), Some("json"));
    let _ = std::fs::remove_dir_all(&root);
}

/// The status bar widget `status-file-type`'s text, as the window drew it last.
fn status_file_type(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| b.status_file_type(cx))
}

/// SPEC-0004 v9 AK-16: a file whose name says nothing is YAML by its Vim modeline; a script's
/// hashbang line typed and saved with ⌘S makes it Python at once, its text and caret kept; the
/// status bar names the type and a real click on it opens Override File Type for the file.
#[gpui::test]
fn modelines_saved_hashbangs_and_the_status_bar(cx: &mut TestAppContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let root = workspace(
        "modeline",
        &[
            ("settings.conf", "a: 1\n# vim: set ft=yaml :\n"),
            ("job", "print(1)\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let conf = open(&bench, cx, &root.join("settings.conf"));
    assert_eq!(language(&conf, cx).as_deref(), Some("yaml"));
    assert_eq!(status_file_type(&bench, cx).as_deref(), Some("YAML"));

    let job = open(&bench, cx, &root.join("job"));
    assert_eq!(language(&job, cx), None);
    assert_eq!(status_file_type(&bench, cx).as_deref(), Some("Plain Text"));
    // The modification time must differ from the one the first detection kept.
    std::thread::sleep(Duration::from_millis(20));
    // The caret stands at the start of the newly opened file.
    cx.simulate_input("#!/usr/bin/env python3\n");
    assert_eq!(language(&job, cx), None, "not before it is saved");
    keys(cx, "cmd-s");
    assert_eq!(language(&job, cx).as_deref(), Some("python"));
    assert_eq!(text(&job, cx), "#!/usr/bin/env python3\nprint(1)\n");
    assert!(!job.read_with(cx, |e, _| e.is_modified()));
    assert_eq!(status_file_type(&bench, cx).as_deref(), Some("Python"));

    // A real click on the widget, found by its test id in the semantic tree (SPEC-0319), opens
    // the list for the active file.
    cx.update(|window, cx| window.draw(cx).clear());
    let at = cx
        .update(|window, _| {
            let tree = semantics::tree(semantics::window_id(window))?;
            let node = tree
                .iter()
                .find(|n| n.test_id() == Some("status-file-type"))?;
            let r = node.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
            Some(gpui::point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)))
        })
        .expect("the status bar shows the file type");
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(
        bench.read_with(cx, |b, _| b.jvm_popup()).is_some(),
        "the list is open"
    );
    cx.simulate_input("plain text");
    keys(cx, "enter");
    assert_eq!(status(&bench, cx), "job is treated as Plain Text");
    assert_eq!(language(&job, cx), None);
    assert_eq!(status_file_type(&bench, cx).as_deref(), Some("Plain Text"));
    assert_eq!(saved(&root).overrides.get("job"), Some(&Choice::PlainText));
    let _ = std::fs::remove_dir_all(&root);
}
