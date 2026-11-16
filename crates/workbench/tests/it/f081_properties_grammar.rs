//! F-081 in a window of the test harness, with keys as a user presses them (OBS-0066): a duplicate
//! key of a properties file is marked at both places, and ⌥↩ on the second offers "Remove
//! property", which Enter applies with the value's continuation line and one ⌘Z takes back
//! (`SPEC-0155` v2 behaviour 5); a grammar finding offers "Ignore '<text>'" and "Disable rule"
//! (`SPEC-0155` v3 behaviour 10).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{WorkbenchView, semantics};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-f081-{tag}-{}", std::process::id()));
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
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
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

/// SPEC-0155/AK-6 (v2), OBS-0066 P1: both places of `greeting` are warnings naming each other;
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
/// and keeps the text in `.idecraft/grammar-ignore.toml`; "Disable rule" on the repeated word
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
    let kept = std::fs::read_to_string(root.join(".idecraft/grammar-ignore.toml")).unwrap();
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
