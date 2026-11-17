//! The parts of hand test 110 (F-038; `SPEC-0185`, `SPEC-0027` v5, `SPEC-0042` v6) that were left
//! for a real window, in a window of the test harness with keys and real servers, without the
//! screen: Ruby and PHP code is coloured with the scheme's colours (no language server needed);
//! `bundle lock --local` typed into IDECraft's terminal makes the Ruby server run through
//! `bundle exec solargraph`, and F1 answers through it after the file is opened again (skipped
//! without Ruby, Bundler and solargraph on `PATH`, e.g. `~/.local/ruby/bin`).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{EditorView, WorkbenchView};

type Bench = Entity<WorkbenchView>;

const GREETER_RB: &str = "class Greeter\n  # Greets someone.\n  def greet(name)\n    \"Hello #{name}\"\n  end\nend\n\ngreeter = Greeter.new\ngreeter.greet(\"you\")\n";

const GREETER_PHP: &str = "<?php\n// Greets someone.\nfunction greet(string $name): string {\n    return \"Hello \" . $name;\n}\necho greet(\"you\");\n";

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-f038-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(
    root: &Path,
    settings: idecraft_core::IdeSettings,
    cx: &mut TestAppContext,
) -> (Bench, &'static mut VisualTestContext) {
    let dir = root.join(".test-settings");
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(CurrentIdeSettings(settings));
    });
    cx.executor().allow_parking();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open")
}

/// The capture at the first char of `needle` (after `skip` chars) and its colour.
fn colour_of(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    text: &str,
    needle: &str,
    skip: usize,
) -> (String, Option<u32>) {
    let byte = text.find(needle).unwrap_or_else(|| panic!("{needle}"));
    let at = text[..byte].chars().count() + skip;
    let mut found = None;
    until(cx, 10, &format!("the tree colours `{needle}`"), |cx| {
        found = editor.read_with(cx, |e, _| e.syntax_style_at(at));
        found.is_some()
    });
    let (kind, style) = found.unwrap();
    (kind, style.foreground)
}

/// Hand test 110: Ruby and PHP code is coloured (`SPEC-0027` v5).
#[gpui::test]
fn hand_test_110_ruby_and_php_are_coloured(cx: &mut TestAppContext) {
    let root = workspace(
        "colours",
        &[("greeter.rb", GREETER_RB), ("greeter.php", GREETER_PHP)],
    );
    let (bench, cx) = window(&root, idecraft_core::IdeSettings::default(), cx);
    for (file, text, parts) in [
        (
            "greeter.rb",
            GREETER_RB,
            vec![
                ("class Greeter", 0, "keyword"),
                ("# Greets", 0, "comment"),
                ("\"Hello", 1, "string"),
                ("def greet", 0, "keyword"),
            ],
        ),
        (
            "greeter.php",
            GREETER_PHP,
            vec![
                ("function greet", 0, "keyword"),
                ("// Greets", 0, "comment"),
                ("\"Hello", 1, "string"),
                ("$name)", 1, "variable"),
            ],
        ),
    ] {
        let editor = open(&bench, cx, &root.join(file));
        let mut colours = Vec::new();
        for (needle, skip, kind) in parts {
            let (found, colour) = colour_of(&editor, cx, text, needle, skip);
            assert!(
                found.starts_with(kind),
                "{file}: `{needle}` is {found}, not {kind}"
            );
            let colour =
                colour.unwrap_or_else(|| panic!("{file}: `{needle}` ({found}) has a colour"));
            colours.push((needle, colour));
        }
        // Keywords, comments and strings are told apart by colour.
        let distinct: std::collections::BTreeSet<u32> =
            colours.iter().take(3).map(|(_, c)| *c).collect();
        assert_eq!(distinct.len(), 3, "{file}: {colours:x?}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 110: `bundle lock --local` in IDECraft's terminal puts solargraph behind
/// `bundle exec`; the file opened again answers F1 through it (`SPEC-0185`).
#[gpui::test]
fn hand_test_110_bundle_lock_in_the_terminal_and_f1_through_bundle_exec(cx: &mut TestAppContext) {
    let gemfile = "source \"https://rubygems.org\"\ngem \"solargraph\"\n";
    let root = workspace(
        "bundle",
        &[("greeter.rb", GREETER_RB), ("Gemfile", gemfile)],
    );
    let has = |program: &str| {
        std::process::Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !(has("ruby") && has("bundle") && has("solargraph")) {
        eprintln!("skipped: ruby, bundle or solargraph is not on PATH");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    idecraft_core::trust::trust_for_process(&root);
    let mut settings = idecraft_core::IdeSettings::default();
    settings.terminal.shell = Some("/bin/zsh".into());
    let (bench, cx) = window(&root, settings, cx);
    let file = root.join("greeter.rb");
    open(&bench, cx, &file);

    // The terminal: `bundle lock --local` typed and run.
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Terminal.NewTab", window, cx)
    });
    until(cx, 30, "the shell marks its prompt", |cx| {
        bench.read_with(cx, |b, cx| {
            b.terminal()
                .is_some_and(|t| !t.read(cx).prompts().is_empty())
        })
    });
    let terminal = bench.read_with(cx, |b, _| b.terminal()).unwrap();
    cx.update(|window, cx| window.focus(&terminal.read(cx).focus()));
    let command = "bundle lock --local";
    cx.simulate_input(command);
    cx.simulate_keystrokes("enter");
    until(cx, 120, "bundle lock ran", |cx| {
        terminal.read_with(cx, |t, _| {
            t.prompts()
                .iter()
                .any(|p| p.command.as_deref() == Some(command) && p.exit.is_some())
        })
    });
    let exit = terminal.read_with(cx, |t, _| {
        t.prompts()
            .iter()
            .find(|p| p.command.as_deref() == Some(command))
            .and_then(|p| p.exit)
    });
    assert_eq!(exit, Some(0), "{}", terminal.read_with(cx, |t, _| t.text()));
    assert!(root.join("Gemfile.lock").exists());
    let resolved =
        idecraft_languages::servers::resolve_for(idecraft_languages::LanguageId::Ruby, &root)
            .expect("a Ruby server");
    assert_eq!(
        resolved.args(),
        ["exec", "solargraph", "stdio"],
        "{}",
        resolved.command_line()
    );

    // The file opened again: F1 answers through bundle exec.
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.menu_action("CloseContent", window, cx);
    });
    cx.run_until_parked();
    let editor = open(&bench, cx, &file);
    let at = GREETER_RB.find("Greeter.new").unwrap() + 2;
    let mut shown = String::new();
    until(cx, 180, "F1 shows the class", |cx| {
        editor.update(cx, |e, cx| e.select_range(at..at, cx));
        bench.update_in(cx, |b, window, cx| {
            b.menu_action("QuickJavaDoc", window, cx)
        });
        cx.run_until_parked();
        shown = format!("{:?}", bench.read_with(cx, |b, _| b.doc_blocks()));
        let found = shown.contains("Greeter");
        if !found {
            cx.simulate_keystrokes("escape");
            std::thread::sleep(Duration::from_millis(500));
        }
        found
    });
    assert!(!shown.contains("file://"), "{shown}");
    cx.simulate_keystrokes("escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// The page and stylesheet of OBS-0069: `index.html` links `style.css`; `class="card"` and
/// `id="hello"` on line 7, an empty `class` value on line 8; `.card` on line 1, `#hello` on line
/// 5, `.side` on line 10 and `.side-bar` on line 13 of the stylesheet.
const OBS_0069_INDEX: &str = "<!DOCTYPE html>\n<html>\n<head>\n  <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n  <main class=\"card\" id=\"hello\">\n    <div class=\"\">x</div>\n  </main>\n</body>\n</html>\n";
const OBS_0069_STYLE: &str = ".card {\n  padding: 1em;\n}\n\n#hello {\n  color: navy;\n}\n\n\n.side {\n  margin: 0;\n}\n.side-bar {\n}\n";

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn text_of(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// ⇧F6 at `line` and `column` (0-based) of `editor`, the name typed key by key, Enter.
fn rename_at(
    bench: &Bench,
    cx: &mut VisualTestContext,
    editor: &Entity<EditorView>,
    (line, column): (usize, usize),
    name: &str,
) {
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    editor.update(cx, |e, cx| e.go_to(line, column, cx));
    cx.run_until_parked();
    keys(cx, "shift-f6");
    assert!(
        bench.read_with(cx, |b, _| b.goto_popup().is_some()),
        "the rename popup is open"
    );
    let typed: Vec<String> = name.chars().map(|c| c.to_string()).collect();
    keys(cx, &typed.join(" "));
    keys(cx, "enter");
}

/// `SPEC-0042` v22 AK-44 (behaviour 38, OBS-0069 G2): ⇧F6 on `.card` of the stylesheet keeps the
/// dot and renames `class="card"` of the page in the same step, one ⌘Z takes both back; the same
/// from the page and for an id; a name that is no CSS identifier is refused and nothing changes.
#[gpui::test]
fn obs_0069_rename_keeps_the_mark_and_changes_the_page(cx: &mut TestAppContext) {
    let root = workspace(
        "obs0069-rename",
        &[
            ("index.html", OBS_0069_INDEX),
            ("style.css", OBS_0069_STYLE),
        ],
    );
    let (bench, cx) = window(&root, idecraft_core::IdeSettings::default(), cx);
    let page_on_disk = || std::fs::read_to_string(root.join("index.html")).unwrap();
    let style = open(&bench, cx, &root.join("style.css"));

    // From the stylesheet, the page closed: `.box {` and `class="box"` on disk.
    rename_at(&bench, cx, &style, (0, 2), "box");
    until(cx, 30, "the rename is written", |cx| {
        text_of(&style, cx).starts_with(".box {\n")
    });
    assert_eq!(
        text_of(&style, cx),
        OBS_0069_STYLE.replacen(".card", ".box", 1)
    );
    assert_eq!(
        page_on_disk(),
        OBS_0069_INDEX.replace("class=\"card\"", "class=\"box\"")
    );
    assert_eq!(status(&bench, cx), "Renamed card to box in 2 file(s).");
    // One ⌘Z: both files.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-z");
    assert_eq!(text_of(&style, cx), OBS_0069_STYLE);
    assert_eq!(page_on_disk(), OBS_0069_INDEX);
    assert_eq!(status(&bench, cx), "Undone: Rename card to box (2 files)");

    // From the open page, on `card` of `class="card"`: both editors change.
    let page = open(&bench, cx, &root.join("index.html"));
    rename_at(&bench, cx, &page, (6, 16), "box");
    until(cx, 30, "the rename reaches both editors", |cx| {
        text_of(&page, cx).contains("class=\"box\"") && text_of(&style, cx).starts_with(".box {")
    });
    assert_eq!(status(&bench, cx), "Renamed card to box in 2 file(s).");
    keys(cx, "cmd-z");
    assert_eq!(text_of(&page, cx), OBS_0069_INDEX);
    assert_eq!(text_of(&style, cx), OBS_0069_STYLE);

    // An id, from the stylesheet: `#main` and `id="main"`.
    let style = open(&bench, cx, &root.join("style.css"));
    rename_at(&bench, cx, &style, (4, 1), "main");
    until(cx, 30, "the id is renamed", |cx| {
        text_of(&style, cx).contains("\n#main {\n") && text_of(&page, cx).contains("id=\"main\"")
    });
    keys(cx, "cmd-z");
    assert_eq!(text_of(&style, cx), OBS_0069_STYLE);
    assert_eq!(text_of(&page, cx), OBS_0069_INDEX);

    // No CSS identifier: refused, with the reason, nothing written.
    rename_at(&bench, cx, &style, (0, 0), "1box");
    until(cx, 30, "the name is refused", |cx| {
        status(&bench, cx).starts_with("Rename failed: \"1box\" is not a valid CSS class name")
    });
    assert_eq!(text_of(&style, cx), OBS_0069_STYLE);
    assert_eq!(text_of(&page, cx), OBS_0069_INDEX);
    assert_eq!(page_on_disk(), OBS_0069_INDEX);
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0042` v22 AK-44 (behaviour 39, OBS-0069 G1 and G3) and `SPEC-0072` v15 AK-36: Code
/// Completion in an empty `class` value lists the stylesheet's classes with file and line;
/// typing opens the list by itself and narrows over `-`; Enter writes the whole name. ⌘B on
/// `card` puts the caret on the name in the rule.
#[gpui::test]
fn obs_0069_class_names_are_offered_in_html(cx: &mut TestAppContext) {
    let root = workspace(
        "obs0069-complete",
        &[
            ("index.html", OBS_0069_INDEX),
            ("style.css", OBS_0069_STYLE),
        ],
    );
    let (bench, cx) = window(&root, idecraft_core::IdeSettings::default(), cx);
    let page = open(&bench, cx, &root.join("index.html"));
    let rows = |cx: &mut VisualTestContext| bench.read_with(cx, |b, _| b.completion_rows());
    let items = |cx: &mut VisualTestContext| bench.read_with(cx, |b, _| b.completion_items());

    // Code › Code Completion between the quotes of line 8.
    page.update(cx, |e, cx| e.go_to(7, 16, cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("CodeCompletion", window, cx)
    });
    until(cx, 10, "the list opens", |cx| rows(cx).is_some());
    let row = |l: &str, t: &str| (l.to_owned(), t.to_owned());
    assert_eq!(
        rows(cx).unwrap(),
        [
            row("card", "style.css:1"),
            row("side", "style.css:10"),
            row("side-bar", "style.css:13")
        ]
    );
    keys(cx, "escape");
    assert_eq!(items(cx), None);

    // Typing opens it by itself; Enter writes the name.
    cx.simulate_input("s");
    until(cx, 10, "the list opens while typing", |cx| {
        items(cx).is_some_and(|i| i == ["side", "side-bar"])
    });
    keys(cx, "enter");
    assert!(
        text_of(&page, cx).contains("<div class=\"side\">x</div>"),
        "{}",
        text_of(&page, cx)
    );
    // A second word: the list narrows over `-` and Enter writes the whole name.
    cx.simulate_input(" ");
    cx.simulate_input("side");
    until(cx, 10, "the list opens for the second word", |cx| {
        items(cx).is_some_and(|i| i.contains(&"side-bar".to_owned()))
    });
    cx.simulate_input("-");
    cx.run_until_parked();
    assert_eq!(items(cx), Some(vec!["side-bar".to_owned()]));
    keys(cx, "enter");
    assert!(
        text_of(&page, cx).contains("<div class=\"side side-bar\">x</div>"),
        "{}",
        text_of(&page, cx)
    );

    // ⌘B on `card` of line 7: the caret on the name in the rule, line 1 column 2.
    page.update(cx, |e, cx| e.go_to(6, 16, cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("GotoDeclaration", window, cx)
    });
    until(cx, 10, "the stylesheet opens at the rule", |cx| {
        bench.read_with(cx, |b, _| b.active_file()) == Some(root.join("style.css"))
    });
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    let at = editor.read_with(cx, |e, _| e.status());
    assert_eq!((at.line, at.column), (1, 2));
    let _ = std::fs::remove_dir_all(&root);
}
