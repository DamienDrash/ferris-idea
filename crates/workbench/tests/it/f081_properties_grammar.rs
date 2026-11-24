//! F-081 in a window of the test harness, with keys as a user presses them (OBS-0066): a duplicate
//! key of a properties file is marked at both places, and ⌥↩ on the second offers "Remove
//! property", which Enter applies with the value's continuation line and one ⌘Z takes back
//! (`SPEC-0155` v2 behaviour 5); a grammar finding offers "Ignore '<text>'" and "Disable rule"
//! (`SPEC-0155` v3 behaviour 10). v5 (OBS-0091, OBS-0092): properties files in colour with `#`
//! comments, a resource bundle as one node of the Project tree that opens the bundle editor, and
//! Update Copyright over the whole project in the reference's comment form.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{WorkbenchView, hit_targets, semantics};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-f081-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let dir = root.join(".test-settings");
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    cx.executor().allow_parking();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    draw(cx);
    (bench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        draw(cx);
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    draw(cx);
}

/// The messages of the active editor's findings of inspection `id`, in text order.
fn findings(bench: &Bench, cx: &mut VisualTestContext, id: &str) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .map(|e| {
                e.read(cx)
                    .problems()
                    .iter()
                    .filter(|p| p.inspection == Some(id))
                    .map(|p| p.message.clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// ⌥↩ at the caret; the titles of the list it opens.
fn context_actions(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    keys(cx, "alt-enter");
    until(cx, 15, "⌥↩ opens the list of context actions", |cx| {
        bench.read_with(cx, |b, _| b.intentions_popup().is_some())
    });
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    popup.read_with(cx, |v, _| v.titles())
}

/// SPEC-0155/AK-6 (v2), OBS-0066 P1: both places of `greeting` are marked (v5: as errors) naming
/// each other;
/// ⌥↩ on the second offers "Remove property" first, Enter removes it with its continuation line,
/// and one ⌘Z brings it back.
#[gpui::test]
fn duplicate_key_remove_property_with_alt_enter(cx: &mut TestAppContext) {
    const TEXT: &str = "greeting=Hello\ngreeting=Hi \\\n  there\nother=1\n";
    let root = workspace("props", &[("messages.properties", TEXT)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("messages.properties"));
    let duplicate = "properties.duplicate-key";
    until(cx, 15, "both places of the key are marked", |cx| {
        findings(&bench, cx, duplicate).len() == 2
    });
    let found = findings(&bench, cx, duplicate);
    assert!(
        found[0].contains("Duplicate property key `greeting` (also on line 2)"),
        "{found:?}"
    );
    assert!(found[1].contains("(also on line 1)"), "{found:?}");
    // The caret to the second place, then ⌥↩.
    keys(cx, "down");
    let titles = context_actions(&bench, cx);
    assert_eq!(
        titles.first().map(String::as_str),
        Some("Remove property"),
        "{titles:?}"
    );
    keys(cx, "enter");
    until(cx, 10, "Enter removes the second property", |cx| {
        text(&bench, cx) == "greeting=Hello\nother=1\n"
    });
    until(cx, 15, "no duplicate is left", |cx| {
        findings(&bench, cx, duplicate).is_empty()
    });
    keys(cx, "cmd-z");
    assert_eq!(text(&bench, cx), TEXT, "one ⌘Z brings the property back");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0155/AK-7 (v3), OBS-0066 G2: ⌥↩ on `an user` lists the correction, "Ignore 'an user'" and
/// "Disable rule"; Ignore, typed and chosen with Enter, drops the mark without changing the file
/// and keeps the text in `.ferris-idea/grammar-ignore.toml`; "Disable rule" on the repeated word
/// turns that rule off in the project's inspection profile.
#[gpui::test]
fn grammar_finding_ignore_and_disable_rule(cx: &mut TestAppContext) {
    const TEXT: &str = "Send an user the the file.\n";
    let root = workspace("grammar", &[("notes.txt", TEXT)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.txt"));
    let (article, repeated) = ("grazie.article", "grazie.repeated-word");
    until(
        cx,
        30,
        "the article and the repeated word are marked",
        |cx| findings(&bench, cx, article).len() == 1 && findings(&bench, cx, repeated).len() == 1,
    );
    // The caret onto `an`, then ⌥↩.
    keys(cx, "right right right right right");
    let titles = context_actions(&bench, cx);
    assert_eq!(
        titles.get(..3),
        Some(
            &[
                "Replace with 'a'".to_owned(),
                "Ignore 'an user'".to_owned(),
                "Disable rule 'Article before a vowel or consonant'".to_owned()
            ][..]
        ),
        "{titles:?}"
    );
    cx.simulate_input("Ignore");
    draw(cx);
    keys(cx, "enter");
    until(cx, 15, "the article's mark goes", |cx| {
        findings(&bench, cx, article).is_empty()
    });
    assert_eq!(text(&bench, cx), TEXT, "Ignore leaves the file as it is");
    let kept = std::fs::read_to_string(root.join(".ferris-idea/grammar-ignore.toml")).unwrap();
    assert!(
        kept.contains("rule = \"grazie.article\"") && kept.contains("text = \"an user\""),
        "{kept}"
    );
    assert_eq!(
        findings(&bench, cx, repeated).len(),
        1,
        "other findings stay"
    );
    // The caret onto the second `the`, then "Disable rule".
    keys(
        cx,
        "right right right right right right right right right right right right",
    );
    let titles = context_actions(&bench, cx);
    assert!(
        titles.iter().any(|t| t == "Disable rule 'Repeated word'"),
        "{titles:?}"
    );
    cx.simulate_input("Disable rule");
    draw(cx);
    keys(cx, "enter");
    until(cx, 15, "the rule is off and its mark goes", |cx| {
        findings(&bench, cx, repeated).is_empty()
    });
    let profile =
        std::fs::read_to_string(root.join(".idea/inspectionProfiles/Project_Default.xml")).unwrap();
    assert!(profile.contains("grazie.repeated-word"), "{profile}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0155/AK-13 (v6), F-081 window check 2026-11-18: ⌥↩ on `mesage` opens the list with the
/// five replacements at once — the key press does not wait for a search or a language server —,
/// the common word first, then "Save to project dictionary".
#[gpui::test]
fn typo_suggestions_open_at_once(cx: &mut TestAppContext) {
    const TEXT: &str = "This tool prints a mesage to the console.\n";
    let root = workspace("typo", &[("notes.txt", TEXT)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.txt"));
    until(cx, 30, "the typo is marked", |cx| {
        findings(&bench, cx, "grazie.typo").len() == 1
    });
    // The caret into `mesage`, then ⌥↩ once: the list is there after this one key press.
    keys(cx, &["right"; 21].join(" "));
    let started = Instant::now();
    cx.simulate_keystrokes("alt-enter");
    let took = started.elapsed();
    let popup = bench
        .read_with(cx, |b, _| b.intentions_popup())
        .expect("⌥↩ opened the list without waiting");
    let titles = popup.read_with(cx, |v, _| v.titles());
    assert_eq!(
        titles.get(..6),
        Some(
            &[
                "Replace with 'message'".to_owned(),
                "Replace with 'menage'".to_owned(),
                "Replace with 'manage'".to_owned(),
                "Replace with 'messages'".to_owned(),
                "Replace with 'usage'".to_owned(),
                "Save 'mesage' to project dictionary".to_owned(),
            ][..]
        ),
        "{titles:?}"
    );
    assert!(took < Duration::from_millis(200), "⌥↩ took {took:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The rows of the Project tree as `kind name`.
fn tree_rows(bench: &Bench, cx: &mut VisualTestContext) -> Vec<(String, String, bool)> {
    let state = bench.read_with(cx, |b, _| b.project_tree_state());
    state["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["kind"].as_str().unwrap_or_default().to_owned(),
                r["name"].as_str().unwrap_or_default().to_owned(),
                r["selected"] == true,
            )
        })
        .collect()
}

/// SPEC-0155/AK-12 (v5), SPEC-0004/AK-18 (v11), OBS-0091 P2: a properties file is coloured —
/// comment, key, separator, value, escape —, the status bar names Properties, and ⌘/ comments the
/// line with `#`.
#[gpui::test]
fn properties_files_are_coloured_and_commented(cx: &mut TestAppContext) {
    const TEXT: &str = "# Greetings\ngreeting = Hello\\tyou\n";
    let root = workspace("colours", &[("messages.properties", TEXT)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("messages.properties"));
    let kinds = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.active_editor_entity()
                .map(|e| e.read(cx).plugin_span_kinds())
                .unwrap_or_default()
        })
    };
    until(cx, 10, "the file is coloured", |cx| !kinds(cx).is_empty());
    assert_eq!(
        kinds(cx),
        [
            "comment",
            "keyword",
            "operator",
            "string",
            "string.escape",
            "string"
        ]
    );
    assert_eq!(
        bench
            .read_with(cx, |b, cx| b.status_file_type(cx))
            .as_deref(),
        Some("Properties")
    );
    keys(cx, "down");
    keys(cx, "cmd-/");
    let commented = text(&bench, cx);
    assert!(
        commented.starts_with("# Greetings\n#") && commented.contains("greeting = Hello"),
        "{commented}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0212/AK-6 (v3), OBS-0091 P3: the bundle's two files are one node "Resource Bundle
/// 'messages'" of the Project tree; a double click expands it to its files, F4 opens the
/// Resource Bundle Editor.
#[gpui::test]
fn bundle_node_in_the_project_tree(cx: &mut TestAppContext) {
    hit_targets::enable();
    let root = workspace(
        "bundle",
        &[
            ("messages.properties", "greeting=Hello\nfarewell=Bye\n"),
            ("messages_de.properties", "greeting=Hallo\n"),
            ("other.properties", "a=1\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let names: Vec<String> = tree_rows(&bench, cx).into_iter().map(|r| r.1).collect();
    assert!(
        names.contains(&"Resource Bundle 'messages'".to_owned())
            && names.contains(&"other.properties".to_owned())
            && !names.contains(&"messages_de.properties".to_owned()),
        "{names:?}"
    );
    // A double click on the node expands it.
    let r = hit_targets::get("tree-Resource Bundle 'messages'").expect("the node is drawn");
    let at = gpui::point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    for click_count in [1, 2] {
        cx.simulate_event(gpui::MouseDownEvent {
            position: at,
            modifiers: gpui::Modifiers::none(),
            button: gpui::MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            position: at,
            modifiers: gpui::Modifiers::none(),
            button: gpui::MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
    let rows = tree_rows(&bench, cx);
    let at = rows
        .iter()
        .position(|r| r.0 == "bundle")
        .expect("the bundle node");
    assert!(rows[at].2, "the node is selected: {rows:?}");
    assert_eq!(
        rows[at + 1..at + 3]
            .iter()
            .map(|r| r.1.as_str())
            .collect::<Vec<_>>(),
        ["messages.properties", "messages_de.properties"],
        "{rows:?}"
    );
    // F4 (Jump to Source) opens the bundle editor.
    keys(cx, "f4");
    let editor = bench
        .read_with(cx, |b, _| b.bundle_editor())
        .expect("the Resource Bundle Editor is open");
    assert_eq!(
        editor.read_with(cx, |v, _| v.title().to_owned()),
        "Resource Bundle 'messages'"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0155/AK-9 (v5), OBS-0092 C1/C2: Update Copyright asks for the scope; Whole project writes
/// the notice into every own file — the open Java editor, `tool.py` and `notice.sh` on disk in the
/// reference's bytes, the script keeping its mode —, and a second run changes nothing.
#[gpui::test]
fn update_copyright_over_the_whole_project(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let root = workspace(
        "copyright",
        &[
            (
                ".ferris-idea/copyright.txt",
                "Copyright (c) $today.year Demo\nAll rights reserved.\n",
            ),
            ("src/Main.java", "class Main {}\n"),
            ("tool.py", "print(1)\n"),
            ("notice.sh", "#!/bin/sh\necho hi\n"),
        ],
    );
    std::fs::set_permissions(
        root.join("notice.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let (bench, cx) = window(&root, cx);
    let year = ferris_idea_languages::copyright::current_year();
    open(&bench, cx, &root.join("src/Main.java"));
    let run = |cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| {
            b.menu_action("UpdateCopyright", window, cx)
        });
        draw(cx);
        let (choices, at) = bench
            .read_with(cx, |b, _| b.inspect_dialog())
            .expect("the scope dialog is open");
        assert!(bench.read_with(cx, |b, _| b.copyright_dialog()));
        assert_eq!(choices[0], "Whole project");
        assert!(choices[at].starts_with("Current file"), "{choices:?}");
        keys(cx, "up");
        keys(cx, "enter");
        assert!(bench.read_with(cx, |b, _| b.inspect_dialog()).is_none());
    };
    run(cx);
    assert_eq!(
        std::fs::read_to_string(root.join("tool.py")).unwrap(),
        format!("#  Copyright (c) {year} Demo\n#  All rights reserved.\n\nprint(1)\n")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notice.sh")).unwrap(),
        format!(
            "#!/bin/sh\n#\n# Copyright (c) {year} Demo\n# All rights reserved.\n#\n\necho hi\n"
        )
    );
    assert_eq!(
        std::fs::metadata(root.join("notice.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    let java = text(&bench, cx);
    assert!(
        java.starts_with(&format!("/*\n * Copyright (c) {year} Demo\n")),
        "{java}"
    );
    let status = bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default();
    assert!(
        status.starts_with("Copyright notice updated in "),
        "{status}"
    );
    run(cx);
    let status = bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default();
    assert!(
        status.starts_with("Copyright notice updated in 0 files"),
        "{status}"
    );
    assert_eq!(text(&bench, cx), java);
    let _ = std::fs::remove_dir_all(&root);
}
