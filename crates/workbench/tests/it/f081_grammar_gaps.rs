//! F-081 gaps of OBS-0117 in a window of the test harness, with real keys: "Apply all writing
//! suggestions in file" in the ⌥↩ list corrects every grammar and spelling finding of a file as
//! one undo step (`SPEC-0155` v8 behaviour 13); a key that other files of its resource bundle lack
//! is an error at the key naming the locales, `default` for the base file, and its fix adds the
//! key to those files — open ones in their editors, the others on disk — as one undo group
//! (`SPEC-0163` v3 behaviour 3, IMP-0534, IMP-0536).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::diagnostics_model::Severity;
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-f081-gaps-{tag}-{}",
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
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
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

/// Waits until `check` holds on the editor's problems (message and severity).
fn until_problems(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    what: &str,
    check: impl Fn(&[(String, Severity)]) -> bool,
) {
    for _ in 0..300 {
        cx.run_until_parked();
        let problems: Vec<(String, Severity)> = editor.read_with(cx, |e, _| {
            e.problems()
                .iter()
                .map(|p| (p.message.clone(), p.severity))
                .collect()
        });
        if check(&problems) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

/// ⌥↩ at `(line, column)`: the titles of the list.
fn context_actions(
    bench: &Entity<WorkbenchView>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    at: (usize, usize),
) -> Vec<String> {
    editor.update(cx, |e, cx| e.go_to(at.0, at.1, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "alt-enter");
    bench.read_with(cx, |b, cx| {
        b.intentions_popup()
            .map(|v| v.read(cx).titles())
            .unwrap_or_default()
    })
}

/// Down to the row titled `title` of the open list, then Enter, as a user would.
fn choose(cx: &mut VisualTestContext, titles: &[String], title: &str) {
    let at = titles
        .iter()
        .position(|t| t == title)
        .unwrap_or_else(|| panic!("no `{title}` in {titles:?}"));
    for _ in 0..at {
        keys(cx, "down");
    }
    keys(cx, "enter");
}

/// SPEC-0155/AK-18 (v8, OBS-0117 G5): on a grammar finding the ⌥↩ list holds "Apply all writing
/// suggestions in file" once; Enter on it corrects the repeated word, the article and the typo at
/// once, the marks go, and one ⌘Z brings the whole text back.
#[gpui::test]
fn apply_all_writing_suggestions_with_alt_enter(cx: &mut TestAppContext) {
    const TEXT: &str = "Open the the file as an user. We recieve it.\n";
    let root = workspace("writing", &[("README.md", TEXT)]);
    let (bench, cx) = window(&root, cx);
    let readme = open(&bench, cx, &root.join("README.md"));
    until_problems(&readme, cx, "three writing findings", |p| {
        p.iter().any(|(m, _)| m.contains("Repeated word"))
            && p.iter().any(|(m, _)| m.contains("Use \"a\""))
            && p.iter().any(|(m, _)| m.contains("recieve"))
    });
    let titles = context_actions(&bench, &readme, cx, (0, 21));
    assert_eq!(titles.first().map(String::as_str), Some("Replace with 'a'"));
    assert_eq!(
        titles
            .iter()
            .filter(|t| *t == "Apply all writing suggestions in file")
            .count(),
        1,
        "{titles:?}"
    );
    choose(cx, &titles, "Apply all writing suggestions in file");
    assert_eq!(
        text(&readme, cx),
        "Open the file as a user. We receive it.\n"
    );
    until_problems(&readme, cx, "the marks go", |p| p.is_empty());
    keys(cx, "cmd-z");
    assert_eq!(text(&readme, cx), TEXT, "one ⌘Z takes all corrections back");
    let _ = std::fs::remove_dir_all(&root);
}

const BASE: &str = "hello=Hello\n";
const DE: &str = "hello=Hallo\n";
const FR: &str = "hello=Salut\nextra.only.fr=Seulement\n";
const MESSAGE: &str = "`extra.only.fr` is missing in the bundle's default, de";
const FIX: &str = "Add missing translation to default, de";

fn bundle(tag: &str) -> PathBuf {
    workspace(
        tag,
        &[
            ("res/messages.properties", BASE),
            ("res/messages_de.properties", DE),
            ("res/messages_fr.properties", FR),
        ],
    )
}

/// SPEC-0163/AK-9 (v3, OBS-0117 G3): a key only the French file has is an error at the key naming
/// `default, de`; ⌥↩ offers "Add missing translation to default, de" first and Enter adds it with an
/// empty value to the two open files in their editors, the status bar naming them; one ⌘Z in one
/// of them takes the key out of both.
#[gpui::test]
fn missing_bundle_key_fix_in_open_files_is_one_undo_step(cx: &mut TestAppContext) {
    let root = bundle("open");
    let (bench, cx) = window(&root, cx);
    let base = open(&bench, cx, &root.join("res/messages.properties"));
    let de = open(&bench, cx, &root.join("res/messages_de.properties"));
    let fr = open(&bench, cx, &root.join("res/messages_fr.properties"));
    until_problems(&fr, cx, "the missing key is an error", |p| {
        p.iter()
            .any(|(m, s)| m.contains(MESSAGE) && *s == Severity::Error)
    });
    let titles = context_actions(&bench, &fr, cx, (1, 3));
    assert_eq!(titles.first().map(String::as_str), Some(FIX), "{titles:?}");
    // OBS-0117 G7: the row's submenu (→) holds its options, never nothing; ← closes it.
    keys(cx, "right");
    let labels = bench.read_with(cx, |b, cx| {
        b.intentions_popup()
            .and_then(|v| v.read(cx).submenu_labels())
            .unwrap_or_default()
    });
    assert!(
        labels.iter().any(|l| l == "Disable inspection"),
        "{labels:?}"
    );
    keys(cx, "left");
    keys(cx, "enter");
    assert_eq!(text(&base, cx), "hello=Hello\nextra.only.fr=\n");
    assert_eq!(text(&de, cx), "hello=Hallo\nextra.only.fr=\n");
    assert_eq!(text(&fr, cx), FR, "the file with the key is left alone");
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)).as_deref(),
        Some("Added 'extra.only.fr' to messages.properties, messages_de.properties")
    );
    // One ⌘Z in the German editor takes the key out of both files.
    let de_again = open(&bench, cx, &root.join("res/messages_de.properties"));
    assert_eq!(de_again.entity_id(), de.entity_id());
    keys(cx, "cmd-z");
    assert_eq!(text(&de, cx), DE);
    assert_eq!(text(&base, cx), BASE, "the group is undone in every file");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0163/AK-9 (v3, IMP-0536): with the other files closed the fix writes them on disk, and the
/// mark goes; a second fix has nothing to add and the status bar says why.
#[gpui::test]
fn missing_bundle_key_fix_writes_closed_files(cx: &mut TestAppContext) {
    let root = bundle("closed");
    let (bench, cx) = window(&root, cx);
    let fr = open(&bench, cx, &root.join("res/messages_fr.properties"));
    until_problems(&fr, cx, "the missing key is an error", |p| {
        p.iter().any(|(m, _)| m.contains(MESSAGE))
    });
    let titles = context_actions(&bench, &fr, cx, (1, 3));
    assert_eq!(titles.first().map(String::as_str), Some(FIX), "{titles:?}");
    keys(cx, "enter");
    let read = |name: &str| std::fs::read_to_string(root.join("res").join(name)).unwrap();
    assert_eq!(read("messages.properties"), "hello=Hello\nextra.only.fr=\n");
    assert_eq!(
        read("messages_de.properties"),
        "hello=Hallo\nextra.only.fr=\n"
    );
    until_problems(&fr, cx, "the mark goes", |p| {
        p.iter().all(|(m, _)| !m.contains("is missing"))
    });
    // Nothing left to add: the fix says so instead of doing nothing (IMP-0536).
    bench.update(cx, |b, cx| {
        b.add_missing_bundle_key(
            &root.join("res/messages_fr.properties"),
            "extra.only.fr",
            cx,
        )
    });
    assert_eq!(
        bench.read_with(cx, |b, cx| b.status_message(cx)).as_deref(),
        Some("'extra.only.fr' not added: `extra.only.fr` is in every file of the bundle already")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0155/AK-20 (v8, OBS-0117 G6): Settings › Editor › Natural Languages › Grammar and Style
/// lists commit messages, the file types with their contexts and the writing rules; Space on Lua
/// turns its checks off and the open Lua file loses the mark in its comment at once, Space again
/// brings it back; string literals are off by default. (Lua, because the scope holds for the whole
/// process and no other window test checks Lua prose.)
#[gpui::test]
fn grammar_scope_in_settings(cx: &mut TestAppContext) {
    use ferris_idea_workbench::settings_model::{GrammarRow, Page, grammar_rows};
    let root = workspace(
        "scope",
        &[("tool.lua", "-- Open the the file.\nprint(1)\n")],
    );
    let (bench, cx) = window(&root, cx);
    let readme = open(&bench, cx, &root.join("tool.lua"));
    until_problems(&readme, cx, "the repeated word is marked", |p| {
        p.iter().any(|(m, _)| m.contains("Repeated word"))
    });
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Grammar, window, cx)
    });
    cx.run_until_parked();
    let settings = bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("the Settings dialog is open");
    let rows = grammar_rows();
    let lua = rows
        .iter()
        .position(|r| *r == GrammarRow::FileType("Lua"))
        .expect("Lua is listed");
    assert!(rows.contains(&GrammarRow::Rule("grazie.typo")));
    let current = cx.update(|_, cx| ferris_idea_workbench::appearance::ide_settings(cx));
    assert!(!current.editor.grammar.scope("Java").string_literals);
    settings.update_in(cx, |v, window, cx| {
        v.select_row(lua, cx);
        v.set_grammar_column(0);
        window.focus(&v.focus());
    });
    keys(cx, "space");
    let current = cx.update(|_, cx| ferris_idea_workbench::appearance::ide_settings(cx));
    assert!(!current.editor.grammar.scope("Lua").enabled);
    until_problems(&readme, cx, "Lua off: no marks", |p| {
        p.iter().all(|(m, _)| !m.contains("Repeated word"))
    });
    keys(cx, "space");
    until_problems(&readme, cx, "Lua on again", |p| {
        p.iter().any(|(m, _)| m.contains("Repeated word"))
    });
    let _ = std::fs::remove_dir_all(&root);
}
