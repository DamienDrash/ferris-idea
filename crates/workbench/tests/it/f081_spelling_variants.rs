//! F-081 in a window of the test harness (OBS-0115): a change of the spelling dictionaries
//! re-checks the open editor at once, from Settings and from the settings file as
//! `set_ide_settings` writes it (`SPEC-0159` v5 behaviour 3); a spelling of the other English
//! variant has a dotted mark in the info colour, apart from a typo's wavy one, and ⌥↩ writes the
//! chosen spelling (`SPEC-0155` v7 behaviour 7).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_languages::spelling_variants::OTHER_VARIANT;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::diagnostics_model::{Underline, underline};
use ferris_idea_workbench::{WorkbenchView, semantics};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-f081-variants-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let dir = root.join(".test-settings");
    std::fs::create_dir_all(&dir).unwrap();
    // The default English (United States), as a fresh settings folder has it.
    ferris_idea_languages::grazie::choose_dictionary("");
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

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    draw(cx);
}

/// The words of the active editor's findings of inspection `id`, in text order, with their
/// underline.
fn marked(bench: &Bench, cx: &mut VisualTestContext, id: &str) -> Vec<(String, Underline)> {
    bench.read_with(cx, |b, cx| {
        let text = b.active_text(cx).unwrap_or_default();
        let chars: Vec<char> = text.chars().collect();
        b.active_editor_entity()
            .map(|e| {
                e.read(cx)
                    .problems()
                    .iter()
                    .filter(|p| p.inspection == Some(id))
                    .map(|p| (chars[p.range.clone()].iter().collect(), underline(p)))
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn words(bench: &Bench, cx: &mut VisualTestContext, id: &str) -> Vec<String> {
    marked(bench, cx, id).into_iter().map(|(w, _)| w).collect()
}

/// SPEC-0159/AK-12 (v5), OBS-0115 S1: en_US marks `colour`; switching to en_GB in Settings marks
/// `center` instead at once, and the settings file written back to en_US (as `set_ide_settings`
/// over MCP writes it) marks `colour` again — in the same window, without an edit.
#[gpui::test]
fn switching_the_dictionary_rechecks_open_editors(cx: &mut TestAppContext) {
    const TEXT: &str = "Pick the colour of the frame and the center of the text.\n";
    let root = workspace("switch", &[("notes.md", TEXT)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.md"));
    until(cx, 30, "en_US marks colour", |cx| {
        words(&bench, cx, OTHER_VARIANT) == ["colour"]
    });
    // Settings › Editor › Natural Languages: Enter on English (United Kingdom).
    let mut settings = ferris_idea_core::IdeSettings::default();
    settings.editor.spelling_dictionary = "en_GB".into();
    cx.update(|_, cx| ferris_idea_workbench::appearance::set_ide_settings(settings.clone(), cx))
        .unwrap();
    let switched = Instant::now();
    until(cx, 30, "en_GB marks center instead", |cx| {
        words(&bench, cx, OTHER_VARIANT) == ["center"]
    });
    let took = switched.elapsed();
    // The settings file changed by another writer, as `set_ide_settings` over MCP does.
    settings.editor.spelling_dictionary = "en_US".into();
    let file = root.join(".test-settings/settings.toml");
    ferris_idea_core::save_ide_settings(&file, &settings).unwrap();
    until(cx, 30, "the file's en_US marks colour again", |cx| {
        words(&bench, cx, OTHER_VARIANT) == ["colour"]
    });
    assert_eq!(ferris_idea_languages::grazie::chosen_dictionary(), "en_US");
    assert_eq!(
        cx.update(|_, cx| ferris_idea_workbench::appearance::ide_settings(cx))
            .editor
            .spelling_dictionary,
        "en_US",
        "the settings in effect hold the file's choice"
    );
    let shown = bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default();
    assert_eq!(shown, TEXT, "no edit was needed");
    eprintln!("en_GB marks after {took:?}");
    ferris_idea_languages::grazie::choose_dictionary("");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0155/AK-15 (v7), OBS-0115 S3: with en_US `colour` has a dotted mark in the info colour,
/// `recieve` the typo's wavy one; ⌥↩ on `colour` offers `color`, then saving the word, and Enter
/// writes `color`.
#[gpui::test]
fn other_variant_mark_and_its_fix(cx: &mut TestAppContext) {
    const TEXT: &str = "Pick a colour and recieve it.\n";
    let root = workspace("mark", &[("notes.txt", TEXT)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("notes.txt"));
    until(cx, 30, "colour and recieve are marked", |cx| {
        !marked(&bench, cx, OTHER_VARIANT).is_empty()
            && !marked(&bench, cx, "grazie.typo").is_empty()
    });
    assert_eq!(
        marked(&bench, cx, OTHER_VARIANT),
        [("colour".to_owned(), Underline::Dotted("INFO_ATTRIBUTES"))]
    );
    assert_eq!(
        marked(&bench, cx, "grazie.typo"),
        [(
            "recieve".to_owned(),
            Underline::Wavy("WEAK_WARNING_ATTRIBUTES")
        )]
    );
    // The two colours differ in the theme in use.
    let palette = ferris_idea_workbench::palette::Palette::for_theme(
        ferris_idea_workbench::palette::current_theme(),
    );
    assert_ne!(
        palette.problem_mark("INFO_ATTRIBUTES"),
        palette.problem_mark("WEAK_WARNING_ATTRIBUTES")
    );
    // The caret into `colour`, then ⌥↩.
    keys(cx, &["right"; 9].join(" "));
    keys(cx, "alt-enter");
    until(cx, 15, "⌥↩ opens the list", |cx| {
        bench.read_with(cx, |b, _| b.intentions_popup().is_some())
    });
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    let titles = popup.read_with(cx, |v, _| v.titles());
    assert_eq!(
        titles.get(..2),
        Some(
            &[
                "Replace with 'color'".to_owned(),
                "Save 'colour' to project dictionary".to_owned()
            ][..]
        ),
        "{titles:?}"
    );
    keys(cx, "enter");
    until(cx, 15, "Enter writes color", |cx| {
        bench
            .read_with(cx, |b, cx| b.active_text(cx))
            .is_some_and(|t| t == "Pick a color and recieve it.\n")
    });
    until(cx, 15, "the mark goes", |cx| {
        marked(&bench, cx, OTHER_VARIANT).is_empty()
    });
    let _ = std::fs::remove_dir_all(&root);
}
