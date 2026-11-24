//! The gaps of OBS-0119 (F-018, 40 frequent shortcuts beside the reference) in a window of the
//! test harness, by keys sent through the window (no input method of the system in between):
//! K1 the Go to Line popup and navigation (`SPEC-0069` v5), K2 the Generate list (`SPEC-0122`
//! v10), K4 Rollback of several files (`SPEC-0109` v4).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_workbench::WorkbenchView;
use ferris_idea_workbench::appearance::{IdeSettingsFile, ThemeFile};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-obs0119-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        let file = root.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    // Settings of the window's own, never the user's.
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-obs0119-settings-{}",
        root.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    let file = file.to_path_buf();
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn caret_line(bench: &Bench, cx: &mut VisualTestContext) -> usize {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .expect("a file is open")
            .read(cx)
            .status()
            .line
    })
}

fn popup_open(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, _| b.goto_popup().is_some())
}

const LINES: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n";

/// `SPEC-0069` v5 AK-7 (behaviour 10, OBS-0119 K1): with the Go to Line popup open, a navigation
/// that moves the caret elsewhere (Back, ⌘[) closes it; Escape closes it also when the focus is
/// no longer in its field.
#[gpui::test]
fn go_to_line_closes_on_navigation_and_on_escape(cx: &mut TestAppContext) {
    let root = workspace("k1", &[("a.txt", LINES), ("b.txt", LINES)]);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("a.txt"));
    // A jump with ⌘L leaves a place for Back.
    keys(cx, "cmd-l");
    assert!(popup_open(&bench, cx), "⌘L opens Go to Line");
    keys(cx, "6 enter");
    assert!(!popup_open(&bench, cx));
    assert_eq!(caret_line(&bench, cx), 6);

    // Back with the popup open and its field focused: the caret goes back, the popup closes.
    keys(cx, "cmd-l");
    assert!(popup_open(&bench, cx));
    keys(cx, "cmd-[");
    assert_eq!(
        caret_line(&bench, cx),
        1,
        "Back went to the place before the jump"
    );
    assert!(
        !popup_open(&bench, cx),
        "a navigation that moves the caret closes the popup"
    );

    // Escape with the focus taken out of the popup (as a click into the editor would).
    keys(cx, "cmd-l");
    assert!(popup_open(&bench, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    keys(cx, "escape");
    assert!(
        !popup_open(&bench, cx),
        "Escape closes the popup wherever the focus is"
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn text_of_active(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn list(bench: &Bench, cx: &mut VisualTestContext) -> Option<(String, Vec<String>, Vec<usize>)> {
    bench.read_with(cx, |b, cx| {
        b.jvm_popup().map(|p| {
            let p = p.read(cx);
            (p.title().to_owned(), p.items().to_vec(), p.checked())
        })
    })
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out: {what}");
}

const PERSON: &str = "package shop;\n\npublic class Person implements Comparable<Person> {\n    private String name;\n    private int age;\n}\n";

/// `SPEC-0122` v10 AK-26 (behaviours 3d and 3e, OBS-0119 K2): ⌘N in a Java class lists the
/// generated kinds, then Override Methods…, Implement Methods…, Delegate Methods…, Test… and
/// Copyright; Test… creates the test class in the test root and opens it with the caret in its
/// body, a second time it opens the same file; Copyright writes the notice.
#[gpui::test]
fn generate_lists_the_choosers_test_and_copyright(cx: &mut TestAppContext) {
    let root = workspace(
        "k2",
        &[
            ("src/main/java/shop/Person.java", PERSON),
            (".ferris-idea/copyright.txt", "Copyright (c) Example"),
        ],
    );
    let person = root.join("src/main/java/shop/Person.java");
    let (bench, cx) = window(&root, cx);
    let at_field = |bench: &Bench, cx: &mut VisualTestContext| {
        open(bench, cx, &person);
        bench.update_in(cx, |b, _, cx| {
            let editor = b.active_editor_entity().expect("a file is open");
            editor.update(cx, |e, cx| e.go_to(3, 4, cx));
        });
        cx.run_until_parked();
    };
    at_field(&bench, cx);
    keys(cx, "cmd-n");
    let (title, items, _) = list(&bench, cx).expect("⌘N opens Generate");
    assert_eq!(title, "Generate in Person");
    assert_eq!(
        items,
        [
            "Constructor",
            "Getter",
            "Setter",
            "Getter and Setter",
            "equals() and hashCode()",
            "toString()",
            "Override Methods…",
            "Implement Methods…",
            "Delegate Methods…",
            "Test…",
            "Copyright",
        ]
    );
    // Test…: typed text narrows to it, Enter creates the class.
    keys(cx, "t e s t enter");
    let test = root.join("src/test/java/shop/PersonTest.java");
    until(cx, "the test class", |_| test.is_file());
    until(cx, "the test class opens", |cx| {
        bench.read_with(cx, |b, _| b.active_file()) == Some(test.clone())
    });
    assert_eq!(
        text_of_active(&bench, cx),
        "package shop;\n\nclass PersonTest {\n\n}\n"
    );
    assert_eq!(caret_line(&bench, cx), 4, "the caret is in the class body");
    assert!(
        status(&bench, cx).contains("Created the test class"),
        "{}",
        status(&bench, cx)
    );

    // A second time the same file opens; nothing is written over it.
    std::fs::write(
        &test,
        "package shop;\n\nclass PersonTest {\n    // mine\n}\n",
    )
    .unwrap();
    at_field(&bench, cx);
    keys(cx, "cmd-n t e s t enter");
    until(cx, "the test class opens again", |cx| {
        bench.read_with(cx, |b, _| b.active_file()) == Some(test.clone())
    });
    assert!(std::fs::read_to_string(&test).unwrap().contains("// mine"));
    assert!(
        status(&bench, cx).contains("Opened the test class"),
        "{}",
        status(&bench, cx)
    );

    // Copyright writes the profile's notice into the class.
    at_field(&bench, cx);
    keys(cx, "cmd-n c o p y r enter");
    let text = text_of_active(&bench, cx);
    assert!(text.contains("Copyright (c) Example"), "{text}");
    assert!(text.contains("public class Person"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

fn git(root: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs")
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// `SPEC-0109` v4 AK-10 (behaviour 12, OBS-0119 K4): ⌥⌘Z with the caret outside a change lists
/// the changed files with the current one checked and no untracked file; a second file checked,
/// Enter asks with the number of files, Rollback rolls both back, the open editor shows HEAD's
/// text and the Local History has the step's label. Cancel changes nothing.
#[gpui::test]
fn rollback_lists_the_changed_files_and_rolls_back_the_checked_ones(cx: &mut TestAppContext) {
    let root = workspace(
        "k4",
        &[("a.txt", LINES), ("b.txt", LINES), ("c.txt", LINES)],
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "start"]);
    let changed = LINES.replace("eight", "EIGHT");
    std::fs::write(root.join("a.txt"), &changed).unwrap();
    std::fs::write(root.join("b.txt"), &changed).unwrap();
    std::fs::write(root.join("new.txt"), "untracked\n").unwrap();
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("a.txt"));

    // Cancel first: nothing changes.
    keys(cx, "alt-cmd-z");
    let (_, rows, checked) = list(&bench, cx).expect("⌥⌘Z lists the changed files");
    assert_eq!(rows, ["a.txt", "b.txt"]);
    assert_eq!(checked, [0], "the current file is checked");
    keys(cx, "enter");
    let (question, buttons, _) = list(&bench, cx).expect("the confirmation");
    assert_eq!(question, "Roll back the changes of a.txt?");
    assert_eq!(buttons, ["Rollback 1 file", "Cancel"]);
    keys(cx, "down enter");
    assert!(list(&bench, cx).is_none());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        changed
    );

    // b.txt checked as well (↓ and Space), Enter, Rollback.
    keys(cx, "alt-cmd-z");
    keys(cx, "down space enter");
    let (question, buttons, _) = list(&bench, cx).expect("the confirmation");
    assert_eq!(question, "Roll back the changes of 2 files?");
    assert_eq!(buttons[0], "Rollback 2 files");
    keys(cx, "enter");
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), LINES);
    assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), LINES);
    assert!(root.join("new.txt").exists(), "the untracked file stays");
    assert_eq!(status(&bench, cx), "Rolled back 2 files");
    assert_eq!(
        text_of_active(&bench, cx),
        LINES,
        "the editor shows HEAD's text"
    );
    let labels: Vec<String> = ferris_idea_core::LocalHistory::open(&root)
        .unwrap()
        .revisions("a.txt")
        .into_iter()
        .filter_map(|r| r.label)
        .collect();
    assert!(labels.iter().any(|l| l == "Rollback"), "{labels:?}");

    // Nothing left to roll back.
    keys(cx, "alt-cmd-z");
    assert!(list(&bench, cx).is_none());
    assert_eq!(status(&bench, cx), "No changes to roll back");
    let _ = std::fs::remove_dir_all(&root);
}
