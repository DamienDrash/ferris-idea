//! Undo and redo in a window of the test harness, case by case (feature row F-011): what one ⌘Z
//! takes back, where the carets are afterwards, and which text it may touch.
//!
//! `SPEC-0003` v3, `SPEC-0057` v7 behaviours 9, 10, 12 and 18, `SPEC-0025` behaviour 5,
//! `SPEC-0026` behaviour 5, `SPEC-0075`, `SPEC-0098`, `SPEC-0112` behaviour 3, `SPEC-0122`,
//! `SPEC-0063` behaviour 22, `SPEC-0072` behaviour 4, `SPEC-0193` behaviours 3, 4 and 9. Every
//! chord goes through the window's key dispatch (`simulate_keystrokes`,
//! `Window::dispatch_keystroke`), typed text through the input handler (`simulate_input`); a
//! chord nothing in the window takes goes on to the menu bar's action, as the macOS application
//! does it. The two tests with a language server (completion, rename across files) use clangd
//! and are skipped without it. What needs eyes on both IDEs is not here: how the reference
//! places its carets, what its Edit menu calls a step, and what ⌘Z does there while a tool
//! window has the focus.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{ClipboardItem, Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;
type Editor = Entity<EditorView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-undo-redo-{tag}-{}",
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

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Editor {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

fn text(editor: &Editor, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.state.buffer.text())
}

/// The text with the carets marked: `|` for a caret, `[`…`]` for a selection.
fn marked(editor: &Editor, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| {
        let text: Vec<char> = e.state.buffer.text().chars().collect();
        let carets = e.state.carets.carets();
        let mut out = String::new();
        for i in 0..=text.len() {
            for c in carets {
                if c.anchor == c.head && c.head == i {
                    out.push('|');
                } else if c.anchor != c.head && c.range().end == i {
                    out.push(']');
                }
            }
            for c in carets {
                if c.anchor != c.head && c.range().start == i {
                    out.push('[');
                }
            }
            if let Some(ch) = text.get(i) {
                out.push(*ch);
            }
        }
        out
    })
}

fn modified(editor: &Editor, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |e, _| e.is_modified())
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn typed(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn menu(bench: &Bench, cx: &mut VisualTestContext, action: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(action, window, cx));
    cx.run_until_parked();
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// Presses a chord as the macOS application does: the window's key dispatch first; a chord
/// nothing there took goes to the menu bar, whose key equivalent carries out `action`. Returns
/// whether the window took it.
fn chord(bench: &Bench, cx: &mut VisualTestContext, keystroke: &str, action: &str) -> bool {
    let key = gpui::Keystroke::parse(keystroke).unwrap();
    let taken = cx.update(|window, cx| window.dispatch_keystroke(key, cx));
    cx.run_until_parked();
    if !taken {
        menu(bench, cx, action);
    }
    taken
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

const LINES: &str = "one two\nthree\nfour\n";

/// The editing commands, each through its chord: one ⌘Z takes the command back and puts the
/// carets and selections where they were before it; ⇧⌘Z makes it again with the carets after it
/// (`SPEC-0057` v7 behaviours 9, 10, 12, 18; AK-27).
#[gpui::test]
fn editing_commands_are_undone_with_their_carets(cx: &mut TestAppContext) {
    let root = workspace("commands", &[("a.txt", LINES), ("b.rs", "fn f() {\n}\n")]);
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("a.txt"));
    let at = |cx: &mut VisualTestContext, line: usize, column: usize| {
        e.update(cx, |e, cx| e.go_to(line, column, cx));
    };

    // A run of typing with spaces in it is one step.
    at(cx, 0, 7);
    typed(cx, " ab cd");
    assert_eq!(marked(&e, cx), "one two ab cd|\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one two|\nthree\nfour\n");
    assert!(!modified(&e, cx), "back at the text on disk");
    keys(cx, "cmd-z");
    assert_eq!(text(&e, cx), LINES, "nothing is left to undo");
    keys(cx, "cmd-shift-z");
    assert_eq!(marked(&e, cx), "one two ab cd|\nthree\nfour\n");
    keys(cx, "cmd-shift-z");
    assert_eq!(
        text(&e, cx),
        "one two ab cd\nthree\nfour\n",
        "nothing to redo"
    );
    // Enter ends the run; a new edit after an undo drops the redo steps.
    keys(cx, "enter");
    typed(cx, "x");
    assert_eq!(marked(&e, cx), "one two ab cd\nx|\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one two ab cd\n|\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one two ab cd|\nthree\nfour\n");
    typed(cx, "!");
    keys(cx, "cmd-shift-z");
    assert_eq!(
        marked(&e, cx),
        "one two ab cd!|\nthree\nfour\n",
        "the undone line break does not come back"
    );
    keys(cx, "cmd-z cmd-z");
    assert_eq!(marked(&e, cx), "one two|\nthree\nfour\n");

    // Paste, and typing over a selection: the selection comes back.
    cx.write_to_clipboard(ClipboardItem::new_string("PASTE".into()));
    keys(cx, "cmd-v");
    assert_eq!(marked(&e, cx), "one twoPASTE|\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one two|\nthree\nfour\n");
    e.update(cx, |e, cx| e.select_range(0..3, cx));
    typed(cx, "x");
    assert_eq!(marked(&e, cx), "x| two\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "[one] two\nthree\nfour\n");
    keys(cx, "cmd-shift-z");
    assert_eq!(marked(&e, cx), "x| two\nthree\nfour\n");
    keys(cx, "cmd-z");

    // Cut of a selection.
    keys(cx, "cmd-x");
    assert_eq!(marked(&e, cx), "| two\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "[one] two\nthree\nfour\n");

    // Delete Line, Duplicate, Move Line Up: the caret is back on its place in its line.
    for (chord, after) in [
        ("cmd-backspace", "one two\nfo|ur\n"),
        ("cmd-d", "one two\nthree\nth|ree\nfour\n"),
        ("alt-shift-up", "th|ree\none two\nfour\n"),
    ] {
        at(cx, 1, 2);
        keys(cx, chord);
        assert_eq!(marked(&e, cx), after, "{chord}");
        keys(cx, "cmd-z");
        assert_eq!(marked(&e, cx), "one two\nth|ree\nfour\n", "{chord}");
        keys(cx, "cmd-shift-z");
        assert_eq!(marked(&e, cx), after, "redo of {chord}");
        keys(cx, "cmd-z");
        assert!(!modified(&e, cx), "{chord}");
    }

    // Backspace pressed twice is one step.
    at(cx, 0, 3);
    keys(cx, "backspace backspace");
    assert_eq!(marked(&e, cx), "o| two\nthree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one| two\nthree\nfour\n");

    // Two carets (Clone Caret Below): one step, and both carets are there after undo and redo.
    at(cx, 0, 0);
    e.update(cx, |e, cx| {
        assert!(e.run_action("EditorCloneCaretBelow", cx));
    });
    typed(cx, "mc");
    assert_eq!(marked(&e, cx), "mc|one two\nmc|three\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "|one two\n|three\nfour\n");
    keys(cx, "cmd-shift-z");
    assert_eq!(marked(&e, cx), "mc|one two\nmc|three\nfour\n");
    keys(cx, "cmd-z escape");

    // Column selection: ⇧↓ makes a caret per line; typing there is one step.
    at(cx, 0, 1);
    e.update(cx, |e, cx| e.set_column_mode(true, cx));
    keys(cx, "shift-down");
    typed(cx, "C");
    assert_eq!(marked(&e, cx), "oC|ne two\ntC|hree\nfour\n");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "o|ne two\nt|hree\nfour\n");
    e.update(cx, |e, cx| e.set_column_mode(false, cx));
    assert_eq!(text(&e, cx), LINES);
    assert!(!modified(&e, cx));

    // An auto-closed bracket: typing `(` and removing it with Backspace are two steps, and the
    // caret is between the brackets again.
    let rs = open(&bench, cx, &root.join("b.rs"));
    rs.update(cx, |e, cx| e.go_to(0, 8, cx));
    keys(cx, "enter");
    typed(cx, "g(");
    let with_call = marked(&rs, cx);
    assert!(with_call.contains("g(|)"), "{with_call:?}");
    keys(cx, "backspace");
    assert!(marked(&rs, cx).contains("g|\n"), "{:?}", marked(&rs, cx));
    keys(cx, "cmd-z");
    assert_eq!(marked(&rs, cx), with_call);
    // Typing after an undo is a new run; the closing bracket typed over the inserted one
    // belongs to it.
    typed(cx, "1)");
    assert!(marked(&rs, cx).contains("g(1)|"), "{:?}", marked(&rs, cx));
    keys(cx, "cmd-z");
    assert_eq!(marked(&rs, cx), with_call);
    keys(cx, "cmd-z");
    assert_eq!(
        marked(&rs, cx),
        "fn f() {\n    |\n}\n",
        "the run with its bracket"
    );
    keys(cx, "cmd-z");
    assert_eq!(marked(&rs, cx), "fn f() {|\n}\n");
    assert!(!modified(&rs, cx));
    // The other file's history was not touched.
    assert_eq!(text(&e, cx), LINES);
}

const QUERY: &str = "select id,name from users where id=1;\n";
/// An unused import for Optimize Imports and a comparison with `true` for a quick fix.
const CART: &str = "package shop;\n\nimport java.util.List;\nimport java.util.Map;\n\npublic class \
                    Cart {\n    List<String> items;\n\n    int size(boolean on) {\n        if (on \
                    == true) {\n            return 1;\n        }\n        return 0;\n    }\n}\n";

/// Steps the window makes: a formatter's result, Optimize Imports, a quick fix, a reload from
/// disk — one ⌘Z each, with the caret where it was before the step; and undo around a save
/// (`SPEC-0075` behaviour 2, `SPEC-0122`, `SPEC-0098` behaviour 8, `SPEC-0026` behaviour 5,
/// `SPEC-0003` behaviour 8, `SPEC-0057` v7 behaviour 9).
#[gpui::test]
fn steps_of_the_window_are_undone_with_their_carets(cx: &mut TestAppContext) {
    let root = workspace(
        "window-steps",
        &[
            ("q.sql", QUERY),
            ("src/shop/Cart.java", CART),
            ("a.txt", LINES),
        ],
    );
    let (bench, cx) = window(&root, cx);

    // Reformat Code with Ferris IDEA's own SQL formatter: several places change, one step.
    let sql = open(&bench, cx, &root.join("q.sql"));
    sql.update(cx, |e, cx| e.go_to(0, 12, cx));
    let before = marked(&sql, cx);
    assert_eq!(before, "select id,na|me from users where id=1;\n");
    menu(&bench, cx, "ReformatCode");
    let formatted = marked(&sql, cx);
    assert_ne!(text(&sql, cx), QUERY, "{}", status(&bench, cx));
    assert!(
        formatted.contains("na|me"),
        "the caret stays on its text: {formatted:?}"
    );
    keys(cx, "cmd-z");
    assert_eq!(marked(&sql, cx), before, "one step, the caret where it was");
    assert!(!modified(&sql, cx));
    keys(cx, "cmd-shift-z");
    assert_eq!(marked(&sql, cx), formatted);
    keys(cx, "cmd-z");

    // Optimize Imports in a Java file.
    let path = root.join("src/shop/Cart.java");
    let java = open(&bench, cx, &path);
    java.update(cx, |e, cx| e.go_to(6, 8, cx));
    let before = marked(&java, cx);
    menu(&bench, cx, "OptimizeImports");
    assert_eq!(status(&bench, cx), "Removed 1 import");
    assert!(!text(&java, cx).contains("java.util.Map"));
    assert!(marked(&java, cx).contains("    List|<String> items;"));
    keys(cx, "cmd-z");
    assert_eq!(marked(&java, cx), before);
    keys(cx, "cmd-shift-z");
    assert!(!text(&java, cx).contains("java.util.Map"));
    keys(cx, "cmd-z");
    assert!(!modified(&java, cx));

    // The quick fix of a Ferris IDEA inspection.
    let findings = |cx: &mut VisualTestContext| {
        java.read_with(cx, |e, _| {
            e.located_problems(&path)
                .into_iter()
                .filter(|p| p.inspection.is_some() && p.line == 9)
                .collect::<Vec<_>>()
        })
    };
    until(cx, "the inspection finds the comparison with true", |cx| {
        !findings(cx).is_empty()
    });
    let found = findings(cx).remove(0);
    java.update(cx, |e, cx| e.go_to(12, 8, cx));
    let before = marked(&java, cx);
    bench.update(cx, |b, cx| b.fix_inspection_finding(&found, false, cx));
    cx.run_until_parked();
    assert_eq!(status(&bench, cx), "Fixed");
    assert!(text(&java, cx).contains("if (on) {"), "{}", text(&java, cx));
    assert!(marked(&java, cx).contains("        |return 0;"));
    keys(cx, "cmd-z");
    assert_eq!(marked(&java, cx), before, "one step, the caret on its line");
    assert!(!modified(&java, cx));
    keys(cx, "cmd-shift-z");
    assert!(text(&java, cx).contains("if (on) {"));
    keys(cx, "cmd-z");

    // Undo around a save: it stops at the saved text and goes past it.
    let e = open(&bench, cx, &root.join("a.txt"));
    e.update(cx, |e, cx| e.go_to(2, 4, cx));
    typed(cx, "S");
    keys(cx, "cmd-s");
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one two\nthree\nfourS\n"
    );
    typed(cx, "T");
    assert!(modified(&e, cx));
    keys(cx, "cmd-z");
    assert_eq!(
        marked(&e, cx),
        "one two\nthree\nfourS|\n",
        "cut at the save"
    );
    assert!(!modified(&e, cx), "the text on disk");
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one two\nthree\nfour|\n");
    assert!(modified(&e, cx), "the disk still has the saved letter");
    keys(cx, "cmd-shift-z");
    assert!(!modified(&e, cx));

    // Another program changes the file: the reload is one step, and undoing it leaves the
    // editor with unsaved text.
    std::fs::write(root.join("a.txt"), "disk\ntext\n").unwrap();
    menu(&bench, cx, "Synchronize");
    assert_eq!(text(&e, cx), "disk\ntext\n");
    assert!(!modified(&e, cx));
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), "one two\nthree\nfourS|\n");
    assert!(modified(&e, cx));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "disk\ntext\n",
        "undo does not write"
    );
    keys(cx, "cmd-shift-z");
    assert_eq!(text(&e, cx), "disk\ntext\n");
    assert!(!modified(&e, cx));
}

/// `SPEC-0057` v7 behaviour 9: a read-only text takes neither undo nor redo; text and caret
/// stay and the status bar names the reason.
#[gpui::test]
fn a_read_only_text_takes_no_undo(cx: &mut TestAppContext) {
    let root = workspace("read-only", &[("a.txt", LINES)]);
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("a.txt"));
    e.update(cx, |e, cx| e.go_to(0, 0, cx));
    typed(cx, "ab");
    keys(cx, "cmd-z");
    typed(cx, "cd");
    // The file becomes read-only with one step to undo and one to redo... the redo step is gone
    // with the new typing; undo it once more to have one.
    keys(cx, "cmd-s");
    typed(cx, "ef");
    keys(cx, "cmd-z");
    e.update(cx, |e, _| e.set_read_only(true));
    e.update(cx, |e, cx| e.go_to(2, 1, cx));
    let frozen = marked(&e, cx);
    assert_eq!(frozen, "cdone two\nthree\nf|our\n");
    for chord in ["cmd-z", "cmd-shift-z"] {
        keys(cx, chord);
        assert_eq!(marked(&e, cx), frozen, "{chord}");
        assert_eq!(
            e.read_with(cx, |e, _| e.status().note).as_deref(),
            Some("Read-only: a library source"),
            "{chord}"
        );
    }
    typed(cx, "x");
    assert_eq!(marked(&e, cx), frozen, "typing changes nothing either");
    // Writable again: both steps are still there.
    e.update(cx, |e, _| e.set_read_only(false));
    keys(cx, "cmd-shift-z");
    assert_eq!(text(&e, cx), "cdefone two\nthree\nfour\n");
    keys(cx, "cmd-z cmd-z");
    assert_eq!(text(&e, cx), LINES);
}

/// `SPEC-0112` behaviour 3: two views of one file share the undo history — an edit made in one
/// is taken back from the other, in both; another file keeps its own history.
#[gpui::test]
fn undo_reaches_both_views_of_a_file_and_no_other_file(cx: &mut TestAppContext) {
    let root = workspace("split", &[("a.txt", LINES), ("b.txt", "other\n")]);
    let (bench, cx) = window(&root, cx);
    let other = open(&bench, cx, &root.join("b.txt"));
    typed(cx, "B");
    let main = open(&bench, cx, &root.join("a.txt"));
    main.update(cx, |e, cx| e.go_to(1, 0, cx));
    menu(&bench, cx, "SplitVertically");
    assert!(bench.read_with(cx, |b, _| b.split_active()));
    let split = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the second view");
    assert_ne!(split.entity_id(), main.entity_id());
    // Typing in the second view shows in both.
    split.update(cx, |e, cx| e.go_to(2, 4, cx));
    typed(cx, "!!");
    let both = |cx: &mut VisualTestContext| bench.read_with(cx, |b, cx| b.split_texts(cx));
    let edited = "one two\nthree\nfour!!\n".to_owned();
    assert_eq!(both(cx), Some((edited.clone(), edited.clone())));
    // ⌘Z in the first view takes it back in both; its caret goes to the place of the edit.
    main.update_in(cx, |e, window, _| window.focus(&e.focus()));
    cx.run_until_parked();
    keys(cx, "cmd-z");
    assert_eq!(both(cx), Some((LINES.to_owned(), LINES.to_owned())));
    assert_eq!(marked(&main, cx), "one two\nthree\nfour|\n");
    assert!(!modified(&main, cx) && !modified(&split, cx));
    // ⇧⌘Z in the second view makes it again in both.
    split.update_in(cx, |e, window, _| window.focus(&e.focus()));
    cx.run_until_parked();
    keys(cx, "cmd-shift-z");
    assert_eq!(both(cx), Some((edited.clone(), edited)));
    keys(cx, "cmd-z");
    keys(cx, "cmd-z");
    assert_eq!(both(cx), Some((LINES.to_owned(), LINES.to_owned())));
    // Undo is per file: the other file still has its letter and its step.
    assert_eq!(text(&other, cx), "Bother\n");
    assert!(modified(&other, cx));
}

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A text input with the focus keeps ⌘Z to itself: the editor behind a search field, a popup or
/// the commit message is not changed by it, and the commit message has its own history
/// (`SPEC-0063` behaviour 22, `SPEC-0057` behaviour 12 in a field).
#[gpui::test]
fn undo_in_a_text_input_leaves_the_editor_alone(cx: &mut TestAppContext) {
    let root = workspace("inputs", &[("a.txt", LINES)]);
    if !git(&root, &["init", "-q", "-b", "main"]) {
        eprintln!("skipped: no git");
        return;
    }
    for args in [
        &["config", "user.name", "T"][..],
        &["config", "user.email", "t@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        &["add", "."],
        &["commit", "-q", "-m", "base"],
    ] {
        assert!(git(&root, args), "git {args:?}");
    }
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("a.txt"));
    e.update(cx, |e, cx| e.go_to(0, 0, cx));
    typed(cx, "zz");
    let edited = "zzone two\nthree\nfour\n";

    for action in [
        "Find",
        "Replace",
        "SearchEverywhere",
        "GotoFile",
        "GotoLine",
        "FindInPath",
    ] {
        menu(&bench, cx, action);
        typed(cx, "q");
        for (keystroke, id) in [("cmd-z", "$Undo"), ("cmd-shift-z", "$Redo")] {
            let taken = chord(&bench, cx, keystroke, id);
            assert!(taken, "{action}: {keystroke} stays in the input");
            assert_eq!(text(&e, cx), edited, "{action}: {keystroke}");
        }
        keys(cx, "escape");
        keys(cx, "escape");
        bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
        cx.run_until_parked();
        assert_eq!(text(&e, cx), edited, "{action}: after closing");
    }

    // The commit message: its own typing is one step of its own history.
    menu(&bench, cx, "ActivateCommitToolWindow");
    let commit = bench
        .read_with(cx, |b, _| b.commit_view())
        .expect("the Commit tool window is open");
    let field = commit.read_with(cx, |v, _| v.message_field());
    field.update_in(cx, |f, window, _| window.focus(&f.focus()));
    cx.run_until_parked();
    typed(cx, "Fix the cart");
    assert_eq!(text(&field, cx), "Fix the cart");
    assert!(chord(&bench, cx, "cmd-z", "$Undo"));
    assert_eq!(text(&field, cx), "");
    assert_eq!(text(&e, cx), edited, "the editor keeps its text");
    assert!(chord(&bench, cx, "cmd-z", "$Undo"));
    assert_eq!(text(&e, cx), edited, "also with nothing left in the field");
    assert!(chord(&bench, cx, "cmd-shift-z", "$Redo"));
    assert_eq!(text(&field, cx), "Fix the cart");
    // Back in the editor ⌘Z is the editor's.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    assert!(chord(&bench, cx, "cmd-z", "$Undo"));
    assert_eq!(text(&e, cx), LINES);
    assert_eq!(text(&field, cx), "Fix the cart");
}

/// `SPEC-0057` v8 AK-29 (behaviour 30): an input field takes back its own typing, a run at a
/// time, with what it drives; the editor behind it keeps its text; a field the window filled
/// keeps that text.
#[gpui::test]
fn an_input_field_undoes_its_own_typing(cx: &mut TestAppContext) {
    let root = workspace("field", &[("a.txt", "abc abd abc\n")]);
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("a.txt"));
    e.update(cx, |e, cx| e.go_to(0, 0, cx));
    typed(cx, "zz");
    let edited = "zzabc abd abc\n";
    let query = |bench: &Bench, cx: &mut VisualTestContext| {
        bench
            .read_with(cx, |b, cx| b.find_query_text(cx))
            .expect("the find bar is open")
    };
    let label = |bench: &Bench, cx: &mut VisualTestContext| {
        bench
            .read_with(cx, |b, cx| b.find_label(cx))
            .unwrap_or_default()
    };

    menu(&bench, cx, "Find");
    typed(cx, "ab");
    let of_ab = label(&bench, cx);
    keys(cx, "left");
    keys(cx, "right");
    typed(cx, "c");
    assert_eq!(query(&bench, cx), "abc");
    let of_abc = label(&bench, cx);
    assert_ne!(of_ab, of_abc, "three matches of ab, two of abc");

    // The last run goes, and the matches are those of the text that is back.
    assert!(chord(&bench, cx, "cmd-z", "$Undo"));
    assert_eq!(query(&bench, cx), "ab");
    assert_eq!(label(&bench, cx), of_ab);
    assert_eq!(text(&e, cx), edited, "the editor keeps its text");
    assert!(chord(&bench, cx, "cmd-z", "$Undo"));
    assert_eq!(query(&bench, cx), "");
    // Nothing left in the field: the editor is still not touched.
    assert!(chord(&bench, cx, "cmd-z", "$Undo"));
    assert_eq!(query(&bench, cx), "");
    assert_eq!(text(&e, cx), edited);
    assert!(chord(&bench, cx, "cmd-shift-z", "$Redo"));
    assert_eq!(query(&bench, cx), "ab");
    assert!(chord(&bench, cx, "cmd-shift-z", "$Redo"));
    assert_eq!(query(&bench, cx), "abc");
    assert_eq!(label(&bench, cx), of_abc);
    assert_eq!(text(&e, cx), edited);
    keys(cx, "escape");

    // A field the window filled: the name to rename is its starting point, not a step.
    let file = root.join("a.txt");
    bench.update_in(cx, |b, window, cx| b.start_rename(file, window, cx));
    cx.run_until_parked();
    let name = |bench: &Bench, cx: &mut VisualTestContext| {
        bench
            .read_with(cx, |b, cx| b.name_field_text(cx))
            .expect("the name field is open")
    };
    assert_eq!(name(&bench, cx), "a.txt");
    chord(&bench, cx, "cmd-z", "$Undo");
    assert_eq!(
        name(&bench, cx),
        "a.txt",
        "undo never empties a prefilled field"
    );
    typed(cx, "b");
    assert_eq!(
        name(&bench, cx),
        "b.txt",
        "typing replaces the selected stem"
    );
    chord(&bench, cx, "cmd-z", "$Undo");
    assert_eq!(name(&bench, cx), "a.txt");
    assert_eq!(text(&e, cx), edited);
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// `SPEC-0057` v8 AK-30 (behaviour 31): the history of a file outlives its tab — undo after
/// closing and opening the file again takes the last edit back, redo a step that was undone
/// before; a file that another program wrote meanwhile starts without a history.
#[gpui::test]
fn the_history_of_a_file_outlives_its_tab(cx: &mut TestAppContext) {
    let root = workspace("closed-tab", &[("a.txt", LINES)]);
    let (bench, cx) = window(&root, cx);
    let file = root.join("a.txt");
    let edited = "zzone two\nthree\nfour\n";
    let close = |bench: &Bench, cx: &mut VisualTestContext| {
        menu(bench, cx, "CloseContent");
        assert!(
            bench
                .read_with(cx, |b, _| b.active_editor_entity())
                .is_none(),
            "the tab is closed"
        );
    };

    // Edited, saved, closed, opened again: ⌘Z takes the edit back.
    {
        let e = open(&bench, cx, &file);
        e.update(cx, |e, cx| e.go_to(0, 0, cx));
        typed(cx, "zz");
        menu(&bench, cx, "SaveAll");
        assert!(!modified(&e, cx));
    }
    close(&bench, cx);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);
    {
        let e = open(&bench, cx, &file);
        assert_eq!(text(&e, cx), edited);
        assert!(!modified(&e, cx));
        keys(cx, "cmd-z");
        assert_eq!(text(&e, cx), LINES, "the edit made before the tab closed");
        assert!(modified(&e, cx), "the disk still has the saved text");
        keys(cx, "cmd-shift-z");
        assert_eq!(text(&e, cx), edited);
        assert!(!modified(&e, cx));

        // Closed with a step undone and saved: ⇧⌘Z makes it again after reopening.
        keys(cx, "cmd-z");
        menu(&bench, cx, "SaveAll");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), LINES);
    }
    close(&bench, cx);
    {
        let e = open(&bench, cx, &file);
        assert_eq!(text(&e, cx), LINES);
        keys(cx, "cmd-shift-z");
        assert_eq!(
            text(&e, cx),
            edited,
            "the step undone before the tab closed"
        );
        menu(&bench, cx, "SaveAll");
    }
    close(&bench, cx);

    // Another program writes the file while it is closed: no history for the other text.
    std::fs::write(&file, "disk\ntext\n").unwrap();
    {
        let e = open(&bench, cx, &file);
        assert_eq!(text(&e, cx), "disk\ntext\n");
        keys(cx, "cmd-z");
        keys(cx, "cmd-shift-z");
        assert_eq!(text(&e, cx), "disk\ntext\n", "nothing to take back");
        assert!(!modified(&e, cx));
    }
    close(&bench, cx);
    // And the dropped history does not come back with the old text.
    std::fs::write(&file, edited).unwrap();
    {
        let e = open(&bench, cx, &file);
        keys(cx, "cmd-z");
        assert_eq!(text(&e, cx), edited);
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn clangd() -> bool {
    let found = std::process::Command::new("clangd")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !found {
        eprintln!("skipped: no clangd");
    }
    found
}

/// `SPEC-0072` behaviour 4 in the window, with clangd: an accepted suggestion is one step of its
/// own — ⌘Z puts back the typed prefix with the caret after it, the next ⌘Z takes the typing.
/// Skipped without `clangd`.
#[gpui::test]
fn an_accepted_completion_is_one_undo_step(cx: &mut TestAppContext) {
    if !clangd() {
        return;
    }
    let code = "int breadth(int s) { return s; }\nint main() {\n    ;\n}\n";
    let root = workspace("completion", &[("area.cpp", code)]);
    let (bench, cx) = window(&root, cx);
    let e = open(&bench, cx, &root.join("area.cpp"));
    e.update(cx, |e, cx| e.go_to(2, 4, cx));
    typed(cx, "brea");
    let prefix = marked(&e, cx);
    assert!(prefix.contains("    brea|;"), "{prefix:?}");
    let offered = |cx: &mut VisualTestContext| {
        bench
            .read_with(cx, |b, _| b.completion_items())
            .is_some_and(|items| items.iter().any(|i| i.contains("breadth")))
    };
    if !offered(cx) {
        menu(&bench, cx, "CodeCompletion");
    }
    until(cx, "the function is suggested", offered);
    keys(cx, "enter");
    until(cx, "the suggestion is inserted", |cx| {
        text(&e, cx).contains("    breadth")
    });
    cx.run_until_parked();
    keys(cx, "cmd-z");
    assert_eq!(marked(&e, cx), prefix, "back to what was typed");
    keys(cx, "cmd-z");
    assert_eq!(text(&e, cx), code);
    assert!(!modified(&e, cx));
    keys(cx, "cmd-shift-z cmd-shift-z");
    assert!(text(&e, cx).contains("    breadth"), "{}", text(&e, cx));
}

/// `SPEC-0193` behaviours 3, 4 and 9 in the window, with clangd: a rename that changed the open
/// file and a closed one is one ⌘Z in the editor — both files come back, without a question,
/// and the status bar says so; ⇧⌘Z renames again. While the closed file holds another text the
/// undo is refused each time and names the file; nothing is taken back in one file alone.
/// Skipped without `clangd`.
#[gpui::test]
fn a_rename_across_files_is_one_undo_step(cx: &mut TestAppContext) {
    if !clangd() {
        return;
    }
    let code = "int width(int s) { return s; }\nint main() { return width(2); }\n";
    let renamed = "int w(int s) { return s; }\nint main() { return w(2); }\n";
    let notes = "Call width first.\n";
    let root = workspace("rename", &[("width.cpp", code), ("WIDTH.md", notes)]);
    let (bench, cx) = window(&root, cx);
    bench.update(cx, |b, _| {
        b.set_rename_options(ferris_idea_languages::rename_text::TextOptions {
            comments_and_strings: true,
            text_files: true,
        })
    });
    let e = open(&bench, cx, &root.join("width.cpp"));
    let at = code.find("width").unwrap() + 1;
    e.update(cx, |e, cx| e.select_range(at..at, cx));
    let on_disk = || std::fs::read_to_string(root.join("WIDTH.md")).unwrap();

    keys(cx, "shift-f6");
    keys(cx, "w");
    keys(cx, "enter");
    until(
        cx,
        "the preview lists the occurrence in the text file",
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.rename_preview()
                    .is_some_and(|p| !p.read(cx).rows().is_empty())
            })
        },
    );
    keys(cx, "enter");
    until(
        cx,
        "the rename changes the open and the closed file",
        |cx| text(&e, cx) == renamed && on_disk() == "Call w first.\n",
    );
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    let caret = marked(&e, cx);

    // One ⌘Z: both files, at once.
    keys(cx, "cmd-z");
    assert_eq!(text(&e, cx), code);
    assert_eq!(on_disk(), notes);
    assert_eq!(status(&bench, cx), "Undone: Rename width to w (2 files)");
    // ⇧⌘Z: both again, the caret where it was after the rename.
    keys(cx, "cmd-shift-z");
    assert_eq!(marked(&e, cx), caret);
    assert_eq!(on_disk(), "Call w first.\n");
    assert_eq!(status(&bench, cx), "Redone: Rename width to w (2 files)");

    // The closed file changes: the undo is refused, the second time too, and names the file.
    std::fs::write(root.join("WIDTH.md"), "Call w first. Then stop.\n").unwrap();
    for _ in 0..2 {
        keys(cx, "cmd-z");
        assert_eq!(text(&e, cx), renamed, "not undone in one file alone");
        assert_eq!(on_disk(), "Call w first. Then stop.\n");
        assert_eq!(
            status(&bench, cx),
            "Cannot undo Rename width to w: WIDTH.md changed since. Undo that change first, or \
             restore from Local History."
        );
    }
    // With the refactoring's text back in the file the whole group is undone.
    std::fs::write(root.join("WIDTH.md"), "Call w first.\n").unwrap();
    keys(cx, "cmd-z");
    assert_eq!(text(&e, cx), code);
    assert_eq!(on_disk(), notes);
    assert!(!modified(&e, cx));
}

/// An agent's change of `files` (path, new text) of open editors as the window's MCP server hands
/// it over (`SPEC-0220` v4 behaviour 15), under `name`, with `on_disk` (path, before, after)
/// written to disk as the server does after the window applied the rest.
fn agent_change(
    cx: &mut VisualTestContext,
    name: &str,
    files: &[(&Path, &str)],
    on_disk: &[(&Path, &str, &str)],
) {
    use ferris_idea_editor::documents::{
        DiskChange, DocumentChange, DocumentEdit, DocumentRegistry,
    };
    let registry = DocumentRegistry::global();
    let edits = files
        .iter()
        .map(|(path, text)| {
            let doc = registry.get(path).expect("open");
            DocumentEdit {
                path: doc.path,
                revision: doc.revision,
                text: (*text).to_owned(),
                save: !doc.modified,
            }
        })
        .collect();
    let change = DocumentChange {
        edits,
        name: Some(name.to_owned()),
        on_disk: on_disk
            .iter()
            .map(|(path, before, after)| DiskChange {
                path: path.to_path_buf(),
                before: (*before).to_owned(),
                after: (*after).to_owned(),
            })
            .collect(),
    };
    let applied = cx.update(|_, cx| WorkbenchView::edit_documents_in(&change, cx));
    assert!(applied.is_ok(), "{applied:?}");
    for (path, _, after) in on_disk {
        std::fs::write(path, after).unwrap();
    }
    cx.run_until_parked();
}

/// `SPEC-0193` v5 behaviour 10 (AK-9): an agent's refactoring of two open files and one on disk
/// is one ⌘Z in either editor — all three files come back, with the status the user's own
/// refactoring gets; ⇧⌘Z redoes it; after typing in the other editor ⌘Z refuses and changes
/// nothing; an agent's change of one editor names its step.
#[gpui::test]
fn an_agents_refactoring_is_one_undo_step_in_the_window(cx: &mut TestAppContext) {
    let (calc, main, notes) = (
        "int add(int a, int b);\n",
        "int main(void) { return add(1, 2); }\n",
        "Call add first.\n",
    );
    let root = workspace(
        "agent-group",
        &[("calc.h", calc), ("main.c", main), ("NOTES.md", notes)],
    );
    let (bench, cx) = window(&root, cx);
    let (h, c, md) = (
        root.join("calc.h"),
        root.join("main.c"),
        root.join("NOTES.md"),
    );
    let eh = open(&bench, cx, &h);
    let ec = open(&bench, cx, &c);
    // The user typed in main.c without saving; the agent's rename keeps that typing.
    typed(cx, "// typed\n");
    let main_typed = text(&ec, cx);
    let renamed = |t: &str| t.replace("add", "sum");
    let on_disk = || std::fs::read_to_string(&md).unwrap();
    agent_change(
        cx,
        "Rename add to sum",
        &[(&h, &renamed(calc)), (&c, &renamed(&main_typed))],
        &[(&md, notes, &renamed(notes))],
    );
    assert_eq!(text(&eh, cx), renamed(calc));
    assert_eq!(text(&ec, cx), renamed(&main_typed));
    assert!(
        !modified(&eh, cx),
        "the saved header was saved with the change"
    );
    assert!(modified(&ec, cx));

    // One ⌘Z in main.c: every file of the refactoring, at once.
    keys(cx, "cmd-z");
    assert_eq!(text(&ec, cx), main_typed);
    assert_eq!(text(&eh, cx), calc);
    assert_eq!(on_disk(), notes);
    assert_eq!(status(&bench, cx), "Undone: Rename add to sum (3 files)");
    // ⇧⌘Z: all again.
    keys(cx, "cmd-shift-z");
    assert_eq!(text(&eh, cx), renamed(calc));
    assert_eq!(text(&ec, cx), renamed(&main_typed));
    assert_eq!(on_disk(), renamed(notes));
    assert_eq!(status(&bench, cx), "Redone: Rename add to sum (3 files)");

    // The user types in main.c: ⌘Z in the header refuses and names the file.
    typed(cx, "x");
    let typed_after = text(&ec, cx);
    let eh = open(&bench, cx, &h);
    keys(cx, "cmd-z");
    assert_eq!(text(&eh, cx), renamed(calc), "not undone in one file alone");
    assert_eq!(text(&ec, cx), typed_after);
    assert_eq!(on_disk(), renamed(notes));
    assert_eq!(
        status(&bench, cx),
        "Cannot undo Rename add to sum: main.c changed since. Undo that change first, or \
         restore from Local History."
    );
    // Once that typing is undone, ⌘Z in the header undoes the whole refactoring.
    let ec = open(&bench, cx, &c);
    keys(cx, "cmd-z");
    assert_eq!(text(&ec, cx), renamed(&main_typed));
    let eh = open(&bench, cx, &h);
    keys(cx, "cmd-z");
    assert_eq!(text(&eh, cx), calc);
    assert_eq!(text(&ec, cx), main_typed);
    assert_eq!(on_disk(), notes);

    // An agent's change of one editor alone: ⌘Z names it.
    agent_change(
        cx,
        "Rename add to plus",
        &[(&h, "int plus(int a, int b);\n")],
        &[],
    );
    keys(cx, "cmd-z");
    assert_eq!(text(&eh, cx), calc);
    assert_eq!(status(&bench, cx), "Undone: Rename add to plus");
}
