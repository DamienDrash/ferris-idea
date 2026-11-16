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
