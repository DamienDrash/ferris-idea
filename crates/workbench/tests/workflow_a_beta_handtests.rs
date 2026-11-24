//! The open hand tests of priority workflow (a) "a project in a priority language: open, edit,
//! navigate, run or debug, and commit with Git" walked in a window of the test harness with keys
//! and the workbench's actions, without the screen, before the beta (the internal hand-test log,
//! Runde 4; the list is `docs/research/BETA_WORKFLOW_A.md`):
//!
//! - the core path in Python and in Java: open a project, edit and save, Go to Declaration, Find
//!   Usages, run, debug to a breakpoint, commit from the Commit tool window.
//! - 12 (SPEC-0042 v3): Python, TypeScript and Java files: the list while typing, ⌘B, ⇧F6, a
//!   wavy line for an error without saving.
//! - 209 (SPEC-0042 v7, SPEC-0071 v5): Java in a single file without a build file: ⌘B on the
//!   constructor call, on a variable and on a method, ⌘[ back; nothing to go to after `;`; a
//!   library class opens read-only.
//! - 207 (SPEC-0059 v4, SPEC-0117 v2): a file with an error marks its tab and its folders, a
//!   warning does not, the marks go without saving; the Python interpreter follows the active tab.
//! - 18 (SPEC-0098): Ferris IDEA's own inspections in Java, JavaScript and Python while typing.
//! - 20 (SPEC-0039 v5): a Node.js program stops at a breakpoint with its variables; Step Over and
//!   Step Into.
//! - 19 (SPEC-0045 v3, SPEC-0051 v2): ⌃⇧R on a pytest and on a Jest test file shows the tests as
//!   a tree with the failure and its message; a double click or Enter on a test opens its file at
//!   the test (SPEC-0088 v5), the message starts at its top; the ▶ of a Jest test debugs it
//!   (SPEC-0045 v6).
//! - 20 also steps into a call in the arguments of `console.log` (beta finding B4).
//! - 209 also: the library source shows no problems (SPEC-0149 v5).
//! - 6 (SPEC-0066 v2): Replace in Files over an open and a closed file, one ⌘Z, the Local History.
//! - 7 (SPEC-0092): the Problems window with ⌘6, ↓ and Enter, the Project tab, the list following a
//!   fix.
//! - 208 (SPEC-0275 v4 behaviour 28), step 4: popups close at a click into the editor, which places
//!   the caret.
//! - 197 (SPEC-0278, IMP-0006), steps 3–4: an untrusted project opens in safe mode, can be
//!   edited, and runs nothing until it is trusted.
//!
//! The tests run as the application does, with the language server pool (diagnostics of the
//! unsaved text come only through it, `SPEC-0090` behaviour 4) and with safe mode switched on, so
//! a project runs only when trusted. Both are switched on for the whole process, so these tests
//! are a program of their own, as `java_delegate_import.rs` is; every other test here trusts its
//! project.
//!
//! What needs eyes on a real window stays in the hand tests: how lists, wavy lines and trees are
//! drawn, real keys of the Mac keyboard, the comparison with the reference. Each test is skipped
//! when its language server, debugger or test runner is missing.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_languages::LanguageId;
use ferris_idea_workbench::DebugState;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{EditorView, WorkbenchView, semantics};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

/// How long a cold language server may take to answer the first time (jdtls imports the folder).
const COLD: u64 = 300;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-workflow-a-{tag}-{}",
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

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Makes `root` a repository with one commit of everything in it, with an identity of its own.
/// `false` without `git`.
fn repository(root: &Path) -> bool {
    if !git(root, &["init", "-q", "-b", "main"]) {
        return false;
    }
    for args in [
        &["config", "user.name", "Beta Test"][..],
        &["config", "user.email", "beta@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        &["add", "."],
        &["commit", "-q", "-m", "base"],
    ] {
        assert!(git(root, args), "git {args:?}");
    }
    true
}

/// A window on `root` with a settings file and theme file of its own: the user's settings are
/// never read or written.
fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    ferris_idea_languages::pool::enable();
    ferris_idea_core::trust::enforce();
    ferris_idea_workbench::hit_targets::enable();
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

/// Waits up to `secs` seconds of real time until `done` holds, letting background work and timers
/// run.
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
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

/// Lets pending work run and draws a frame, as the window does after each event: focus and the
/// timers that open lists by themselves follow the drawn frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

/// Types `text` key by key, as the keyboard does: typed letters open the completion list, text
/// put in at once does not (`SPEC-0072` v3).
fn type_keys(cx: &mut VisualTestContext, text: &str) {
    for c in text.chars() {
        keys(cx, &c.to_string());
    }
}

/// Types `prefix` key by key and waits up to `secs` for the language server's answer to the
/// completion request that typing starts; types the last letter again (a new request) while the
/// answer lacks `wanted`, as a cold server may answer the first one before it has read the file.
/// Returns the last list shown.
fn typed_completion(
    bench: &Bench,
    cx: &mut VisualTestContext,
    prefix: &str,
    wanted: &str,
    secs: u64,
) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut shown = Vec::new();
    let mut first = true;
    while Instant::now() < end {
        let before = bench.read_with(cx, |b, _| b.completions_answered());
        if first {
            type_keys(cx, prefix);
            first = false;
        } else {
            keys(cx, "escape");
            keys(cx, "backspace");
            type_keys(cx, &prefix[prefix.len() - 1..]);
        }
        let left = end
            .saturating_duration_since(Instant::now())
            .as_secs()
            .max(1);
        until(
            cx,
            left,
            "the language server answers the completion request",
            |cx| bench.read_with(cx, |b, _| b.completions_answered()) > before,
        );
        // The list opens after the answer; give it a moment.
        let shown_by = Instant::now() + Duration::from_secs(2);
        while Instant::now() < shown_by {
            draw(cx);
            if let Some(items) = bench.read_with(cx, |b, _| b.completion_items()) {
                shown = items;
                if shown.iter().any(|s| s.starts_with(wanted)) {
                    return shown;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
            cx.executor().advance_clock(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    shown
}

fn action(bench: &Bench, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    bench.read_with(cx, |b, _| {
        b.active_editor_entity().expect("the file is open")
    })
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn active_name(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| {
        b.active_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()
    })
}

/// Puts the caret on line `line`, column `column` (both from 0) of the active editor.
fn caret_to(bench: &Bench, cx: &mut VisualTestContext, line: usize, column: usize) {
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().expect("a file is open");
        editor.update(cx, |e, cx| e.go_to(line, column, cx));
    });
    cx.run_until_parked();
}

/// Line and column (from 0) of the caret in the active editor.
fn caret(bench: &Bench, cx: &mut VisualTestContext) -> (usize, usize) {
    let (line, column) = bench
        .read_with(cx, |b, cx| b.caret_position(cx))
        .expect("a file is open");
    (line - 1, column - 1)
}

/// Line and column (from 0) of the first `needle` in `text`, plus `skip` chars.
fn spot(text: &str, needle: &str, skip: usize) -> (usize, usize) {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in the text"))
        + skip;
    let line = text[..at].matches('\n').count();
    let column = at - text[..at].rfind('\n').map_or(0, |n| n + 1);
    (line, column)
}

/// The messages of the language server's and Ferris IDEA's problems in the active editor.
fn problems(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .map(|e| {
                e.read(cx)
                    .problems()
                    .iter()
                    .map(|p| p.message.clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn has_server(language: LanguageId, root: &Path) -> bool {
    ferris_idea_languages::servers::resolve_for(language, root).is_ok()
}

fn paused(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Paused(_)))
    })
}

fn ended(bench: &Bench, cx: &mut VisualTestContext) -> bool {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Ended(_)))
    })
}

fn variables(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .map(|v| v.read(cx).variable_texts())
            .unwrap_or_default()
    })
}

fn stack(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.debug_view()
            .map(|v| v.read(cx).stack_text())
            .unwrap_or_default()
    })
}

/// The console text of the run tab `name` (or of the selected one).
fn run_output(bench: &Bench, cx: &mut VisualTestContext, name: Option<&str>) -> String {
    bench.read_with(cx, |b, cx| {
        b.run_view()
            .and_then(|v| match name {
                Some(n) => v.read(cx).output_of(n),
                None => v.read(cx).selected_output().map(|(_, t)| t),
            })
            .unwrap_or_default()
    })
}

fn debugpy_installed() -> bool {
    ferris_idea_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some())
}

/// Find Usages (⌥F7) at the caret; waits until the Find tool window lists rows and returns them.
fn find_usages(bench: &Bench, cx: &mut VisualTestContext, secs: u64) -> Vec<String> {
    action(bench, cx, "FindUsages");
    let mut rows = Vec::new();
    until(cx, secs, "Find Usages lists the usages", |cx| {
        rows = bench.read_with(cx, |b, cx| {
            b.find_tool_window()
                .map(|v| {
                    v.read(cx)
                        .visible()
                        .into_iter()
                        .map(|(_, l)| l)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });
        !rows.is_empty()
    });
    rows
}

/// Commits `files` with `message` from the Commit tool window, as the hand test does with ⌘K.
fn commit(bench: &Bench, cx: &mut VisualTestContext, files: &[&str], message: &str) {
    action(bench, cx, "ActivateCommitToolWindow");
    let view = bench
        .read_with(cx, |b, _| b.commit_view())
        .expect("the Commit tool window is open");
    view.update(cx, |v, cx| {
        v.refresh();
        v.check_only(files, cx);
        v.set_message(message, cx);
        v.commit_now(cx);
    });
    until(cx, 30, "the commit is made", |cx| {
        view.read_with(cx, |v, _| {
            v.note().is_some_and(|n| n.starts_with("Committed"))
        })
    });
}

// ------------------------------------------------------------------------- core path, Python

const PRICES_PY: &str = "def total(price, qty):\n    return price * qty\n";

const MAIN_PY: &str =
    "from prices import total\n\n\nsumme = total(3, 4)\nprint(\"Summe\", summe)\n";

/// The core path in Python: open the project, edit `main.py` and save with ⌘S; ⌘B on `total`
/// goes to its definition in `prices.py`, ⌘[ comes back; ⌥F7 lists the call in `main.py`; ⌃⇧R
/// runs the file and shows its output with exit code 0; with a breakpoint ⌃⇧D stops on its line
/// with the variables; the change is committed from the Commit tool window. Skipped without
/// pyright, debugpy or git.
#[gpui::test]
fn core_path_python_open_edit_navigate_run_debug_commit(cx: &mut TestAppContext) {
    let root = workspace("core-py", &[("prices.py", PRICES_PY), ("main.py", MAIN_PY)]);
    if !has_server(LanguageId::Python, &root) || !debugpy_installed() || !repository(&root) {
        eprintln!("skipped: pyright, debugpy or git is missing");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let main = root.join("main.py");
    let editor = open(&bench, cx, &main);

    // Edit and save: a second print at the end, ⌘S writes it.
    caret_to(&bench, cx, 5, 0);
    cx.simulate_input("print(\"fertig\")\n");
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    keys(cx, "cmd-s");
    until(cx, 10, "⌘S writes the file", |_| {
        std::fs::read_to_string(&main).is_ok_and(|t| t.ends_with("print(\"fertig\")\n"))
    });
    assert!(!editor.read_with(cx, |e, _| e.is_modified()));

    // ⌘B on `total` in the call: to `def total` in prices.py; ⌘[ back.
    let current = text(&bench, cx);
    let (line, column) = spot(&current, "total(3", 2);
    caret_to(&bench, cx, line, column);
    until(cx, COLD, "⌘B goes to the definition in prices.py", |cx| {
        if active_name(&bench, cx) == "prices.py" {
            return true;
        }
        action(&bench, cx, "GotoDeclaration");
        std::thread::sleep(Duration::from_millis(300));
        cx.run_until_parked();
        active_name(&bench, cx) == "prices.py"
    });
    assert_eq!(caret(&bench, cx).0, 0, "on the line of `def total`");
    action(&bench, cx, "Back");
    until(cx, 10, "⌘[ comes back to main.py", |cx| {
        active_name(&bench, cx) == "main.py"
    });
    assert_eq!(caret(&bench, cx), (line, column));

    // ⌥F7 on `total`: the call in main.py is listed.
    let rows = find_usages(&bench, cx, 120);
    assert!(
        rows.iter()
            .any(|r| r.contains("total(3, 4)") || r.contains("main.py")),
        "{rows:?}"
    );

    // ⌃⇧R: runs main.py; the output and the exit code are shown.
    open(&bench, cx, &main);
    keys(cx, "ctrl-shift-r");
    until(cx, 60, "the run shows the output and exit code 0", |cx| {
        let out = run_output(&bench, cx, None);
        out.contains("Summe 12") && out.contains("fertig") && out.contains("exit code 0")
    });

    // A breakpoint on the print line, ⌃⇧D: stops there with `summe`.
    let (print_line, _) = spot(&text(&bench, cx), "print(\"Summe\"", 0);
    editor.update(cx, |e, cx| e.toggle_breakpoint(print_line, cx));
    cx.run_until_parked();
    keys(cx, "ctrl-shift-d");
    until(cx, 120, "the program stops at the breakpoint", |cx| {
        paused(&bench, cx)
    });
    until(cx, 30, "the variables show summe = 12", |cx| {
        variables(&bench, cx)
            .iter()
            .any(|v| v.contains("summe") && v.contains("12"))
    });
    action(&bench, cx, "Resume");
    until(cx, 60, "the program runs to its end", |cx| {
        ended(&bench, cx)
    });

    // Commit the change: the repository's log has it, the tree is clean.
    commit(&bench, cx, &["main.py"], "Print when done");
    assert_eq!(
        git_out(&root, &["log", "-1", "--format=%s"]).trim(),
        "Print when done"
    );
    assert!(
        git_out(&root, &["status", "--porcelain", "--", "main.py"]).is_empty(),
        "main.py is committed"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// -------------------------------------------------------------------------- core path, Java

const SHOP_JAVA: &str = "public class Shop {\n    static int total(int price, int qty) {\n        return price * qty;\n    }\n\n    public static void main(String[] args) {\n        int summe = total(3, 4);\n        System.out.println(\"Summe \" + summe);\n    }\n}\n";

/// The core path in Java, in a folder without a build file: edit and save; ⌘B on the call of
/// `total` goes to the method, ⌘[ back; ⌥F7 lists the call; ⌃⇧R runs `main` with its output;
/// ⌃⇧D stops at a breakpoint with `summe`; the change is committed. Skipped without jdtls,
/// java-debug or git.
#[gpui::test]
fn core_path_java_open_edit_navigate_run_debug_commit(cx: &mut TestAppContext) {
    let root = workspace("core-java", &[("Shop.java", SHOP_JAVA)]);
    if !has_server(LanguageId::Java, &root)
        || !ferris_idea_languages::java_debug::java_debug_usable()
        || !repository(&root)
    {
        eprintln!("skipped: jdtls, java-debug or git is missing");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let file = root.join("Shop.java");
    let editor = open(&bench, cx, &file);

    // Edit and save: one more line in `main`, ⌘S writes it.
    let (line, _) = spot(SHOP_JAVA, "        System.out.println(\"Summe", 0);
    caret_to(&bench, cx, line + 1, 0);
    cx.simulate_input("        System.out.println(\"fertig\");\n");
    cx.run_until_parked();
    keys(cx, "cmd-s");
    until(cx, 10, "⌘S writes the file", |_| {
        std::fs::read_to_string(&file).is_ok_and(|t| t.contains("println(\"fertig\");"))
    });
    assert!(!editor.read_with(cx, |e, _| e.is_modified()));

    // ⌘B on the call of `total`: to the method's line; ⌘[ back.
    let (line, column) = spot(SHOP_JAVA, "total(3", 2);
    caret_to(&bench, cx, line, column);
    until(cx, COLD, "⌘B goes to the method", |cx| {
        action(&bench, cx, "GotoDeclaration");
        std::thread::sleep(Duration::from_millis(300));
        cx.run_until_parked();
        caret(&bench, cx).0 == 1
    });
    action(&bench, cx, "Back");
    until(cx, 10, "⌘[ comes back to the call", |cx| {
        caret(&bench, cx) == (line, column)
    });

    // ⌥F7 on `total`: the call in `main` is listed.
    let rows = find_usages(&bench, cx, 120);
    assert!(rows.iter().any(|r| r.contains("total(3, 4)")), "{rows:?}");

    // ⌃⇧R in `main`: runs the class with its output.
    open(&bench, cx, &file);
    caret_to(&bench, cx, line, 8);
    keys(cx, "ctrl-shift-r");
    until(cx, 180, "the run shows the output and exit code 0", |cx| {
        let out = run_output(&bench, cx, None);
        out.contains("Summe 12") && out.contains("fertig") && out.contains("exit code 0")
    });

    // A breakpoint on the println line, ⌃⇧D: stops there with `summe`.
    let (print_line, _) = spot(&text(&bench, cx), "System.out.println(\"Summe", 0);
    editor.update(cx, |e, cx| e.toggle_breakpoint(print_line, cx));
    cx.run_until_parked();
    keys(cx, "ctrl-shift-d");
    until(cx, COLD, "the program stops at the breakpoint", |cx| {
        paused(&bench, cx)
    });
    until(cx, 60, "the variables show summe = 12", |cx| {
        variables(&bench, cx)
            .iter()
            .any(|v| v.contains("summe") && v.contains("12"))
    });
    action(&bench, cx, "Resume");
    until(cx, 120, "the program runs to its end", |cx| {
        ended(&bench, cx)
    });

    commit(&bench, cx, &["Shop.java"], "Say when done");
    assert_eq!(
        git_out(&root, &["log", "-1", "--format=%s"]).trim(),
        "Say when done"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 12

/// One language of hand test 12: the file, where a line is typed (start of that line), the
/// prefix typed and the name the list must offer, the call `⌘B` starts on and the line it lands
/// on, the declaration `⇧F6` renames and the expected texts after it, and a line with an error.
struct Round {
    file: &'static str,
    text: &'static str,
    type_at: &'static str,
    prefix: &'static str,
    offered: &'static str,
    call: &'static str,
    declared_on: usize,
    rename_at: &'static str,
    renamed: &'static [&'static str],
    error_at: &'static str,
    error_line: &'static str,
}

const ROUNDS: &[(LanguageId, Round)] = &[
    (
        LanguageId::Python,
        Round {
            file: "sprachen/shapes.py",
            text: "def area(w, h):\n    return w * h\n\n\nprint(area(2, 3))\n",
            type_at: "\n\nprint(",
            prefix: "ar",
            offered: "area",
            call: "area(2",
            declared_on: 0,
            rename_at: "def area",
            renamed: &["def flaeche(w, h):", "print(flaeche(2, 3))"],
            error_at: "print(",
            error_line: "x = undefined_name\n",
        },
    ),
    (
        LanguageId::TypeScript,
        Round {
            file: "sprachen/shapes.ts",
            text: "function area(w: number, h: number): number {\n    return w * h;\n}\n\nconsole.log(area(2, 3));\n",
            type_at: "\n\nconsole.log(",
            prefix: "ar",
            offered: "area",
            call: "area(2",
            declared_on: 0,
            rename_at: "function area",
            renamed: &["function flaeche(w: number", "console.log(flaeche(2, 3));"],
            error_at: "console.log(",
            error_line: "const n: number = \"text\";\n",
        },
    ),
    (
        LanguageId::Java,
        Round {
            file: "sprachen/Shapes.java",
            text: "public class Shapes {\n    static int area(int w, int h) {\n        return w * h;\n    }\n\n    public static void main(String[] args) {\n\n        System.out.println(area(2, 3));\n    }\n}\n",
            type_at: "{\n\n        System",
            prefix: "ar",
            offered: "area",
            call: "area(2",
            declared_on: 1,
            rename_at: "int area",
            renamed: &[
                "static int flaeche(int w",
                "System.out.println(flaeche(2, 3));",
            ],
            error_at: "        System.out",
            error_line: "        int n = \"text\";\n",
        },
    ),
];

/// Hand test 12 (`SPEC-0042` v3, `SPEC-0072` v3, `SPEC-0071`, `SPEC-0074`; D59 and the language
/// label of DEFECTS-2026-11-05): in a `.py`, a `.ts` and a `.java` file of a project without a
/// build file, typing opens the list with the function; ⌘B on its call goes to its declaration;
/// ⇧F6 renames declaration and call in one go; a type error gets a mark without saving. Each
/// language is skipped when its server is missing.
#[gpui::test]
fn hand_test_12_python_typescript_java_in_the_editor(cx: &mut TestAppContext) {
    let files: Vec<(&str, &str)> = ROUNDS.iter().map(|(_, r)| (r.file, r.text)).collect();
    let root = workspace("12", &files);
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let mut ran = 0;
    for (language, round) in ROUNDS {
        if !has_server(*language, &root) {
            eprintln!("{language:?} skipped: no language server");
            continue;
        }
        ran += 1;
        let what = |step: &str| format!("{language:?}: {step}");
        let editor = open(&bench, cx, &root.join(round.file));

        // Typing a prefix on a new line opens the list with the function, without ⌃Space.
        // The empty line that the needle's first line break ends.
        let skip = round.type_at.find('\n').map_or(0, |i| i + 1);
        let (line, column) = spot(round.text, round.type_at, skip);
        caret_to(&bench, cx, line, column);
        let shown = typed_completion(&bench, cx, round.prefix, round.offered, COLD);
        assert!(
            shown.iter().any(|s| s.starts_with(round.offered)),
            "{}: {shown:?}",
            what("typing opens the list with the function")
        );
        keys(cx, "escape");
        for _ in 0..round.prefix.len() {
            keys(cx, "backspace");
        }
        assert_eq!(
            text(&bench, cx),
            round.text,
            "{}",
            what("the typed prefix is gone")
        );

        // ⌘B on the call: to the declaration's line.
        let (line, column) = spot(round.text, round.call, 1);
        caret_to(&bench, cx, line, column);
        until(cx, 120, &what("⌘B goes to the declaration"), |cx| {
            action(&bench, cx, "GotoDeclaration");
            std::thread::sleep(Duration::from_millis(200));
            cx.run_until_parked();
            caret(&bench, cx).0 == round.declared_on
        });
        assert!(
            !status(&bench, cx).contains("Cannot find"),
            "{}",
            status(&bench, cx)
        );

        // ⇧F6 on the declaration: declaration and call are renamed.
        let (line, column) = spot(round.text, round.rename_at, round.rename_at.len() - 2);
        caret_to(&bench, cx, line, column);
        keys(cx, "shift-f6");
        cx.simulate_input("flaeche");
        keys(cx, "enter");
        until(cx, 120, &what("⇧F6 renames declaration and call"), |cx| {
            let now = text(&bench, cx);
            round.renamed.iter().all(|r| now.contains(r))
        });

        // A type error on a new line is marked without saving.
        let now = text(&bench, cx);
        let (line, _) = spot(&now, round.error_at, 0);
        caret_to(&bench, cx, line, 0);
        cx.simulate_input(round.error_line);
        cx.run_until_parked();
        let at = text(&bench, cx).find(round.error_line).unwrap();
        until(cx, 120, &what("the error is marked without saving"), |cx| {
            editor.read_with(cx, |e, _| {
                e.problems().iter().any(|p| {
                    p.severity == ferris_idea_workbench::diagnostics_model::Severity::Error
                        && p.range.start >= at
                        && p.range.start < at + round.error_line.len()
                })
            })
        });
        assert!(
            editor.read_with(cx, |e, _| e.is_modified()),
            "{}",
            what("not saved")
        );
        // The status bar names the file's language, not another one (D-language label).
        assert_eq!(
            bench.read_with(cx, |b, _| b.python_widget_shown()),
            *language == LanguageId::Python,
            "{}",
            what("the Python widget follows the file")
        );
    }
    if ran == 0 {
        eprintln!("skipped: no language server for Python, TypeScript or Java");
    }
    let _ = std::fs::remove_dir_all(&root);
}

// --------------------------------------------------------------------------- hand test 209

const MAIN_JAVA: &str = "public class Main {\n    public static void main(String[] args) {\n        Person person = new Person(\"Ada\");\n        System.out.println(person.greet());\n    }\n}\n\nclass Person {\n    private final String name;\n\n    Person(String name) {\n        this.name = name;\n    }\n\n    String greet() {\n        return \"Hello \" + name;\n    }\n}\n";

/// Hand test 209 (`SPEC-0042` v7, `SPEC-0071` v5; D59): Java in a single file without a build
/// file, with the file named `test.java` as in the hand test. ⌘B in `Person` of `new Person(…)`
/// goes to the constructor, right after `person` in `person.greet()` to the variable, in `greet`
/// to the method; ⌘[ comes back each time; after `;` there is nothing to go to; in `String` the
/// JDK's source opens read-only (newer than the hand test, which expected only its name in the
/// status bar); typing `pers` offers `person`. Step 6 (no jdtls on the
/// `PATH` of the app) stays in the hand test. Skipped without jdtls.
#[gpui::test]
fn hand_test_209_java_in_a_single_file(cx: &mut TestAppContext) {
    let root = workspace("209", &[("sprachen/test.java", MAIN_JAVA)]);
    if !has_server(LanguageId::Java, &root) {
        eprintln!("skipped: jdtls is missing");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("sprachen/test.java"));

    let jump =
        |bench: &Bench, cx: &mut VisualTestContext, from: (usize, usize), to: usize, what: &str| {
            caret_to(bench, cx, from.0, from.1);
            until(cx, COLD, what, |cx| {
                action(bench, cx, "GotoDeclaration");
                std::thread::sleep(Duration::from_millis(300));
                cx.run_until_parked();
                caret(bench, cx).0 == to
            });
            action(bench, cx, "Back");
            until(cx, 10, &format!("⌘[ after: {what}"), |cx| {
                caret(bench, cx) == from
            });
        };
    // In `Person` of `new Person("Ada")`: the constructor on line 10.
    jump(
        &bench,
        cx,
        spot(MAIN_JAVA, "new Person", 6),
        10,
        "⌘B goes to the constructor",
    );
    // Right after `person` in `person.greet()`: the variable on line 2.
    jump(
        &bench,
        cx,
        spot(MAIN_JAVA, "person.greet", 6),
        2,
        "⌘B goes to the variable",
    );
    // In `greet`: the method on line 14.
    jump(
        &bench,
        cx,
        spot(MAIN_JAVA, "greet()", 2),
        14,
        "⌘B goes to the method",
    );

    // After `;` at the end of the line: nothing to go to.
    let (line, column) = spot(MAIN_JAVA, "(\"Ada\");", 8);
    caret_to(&bench, cx, line, column);
    action(&bench, cx, "GotoDeclaration");
    until(cx, 60, "nothing to go to after `;`", |cx| {
        status(&bench, cx) == "Cannot find a declaration to go to."
    });
    assert_eq!(caret(&bench, cx), (line, column));

    // In `String`: the JDK's source opens read-only (SPEC-0149 v4 behaviour 11; when the hand
    // test was written Ferris IDEA could only name the library class); ⌘[ comes back.
    let (line, column) = spot(MAIN_JAVA, "String[]", 2);
    caret_to(&bench, cx, line, column);
    action(&bench, cx, "GotoDeclaration");
    let mut said = String::new();
    until(cx, 60, "the library class opens", |cx| {
        let now = status(&bench, cx);
        if now != said {
            eprintln!("status: {now}");
            said = now;
        }
        active_name(&bench, cx) == "String.java" && said.starts_with("Library source")
    });
    // Read, not checked (SPEC-0149 v5 behaviour 11, beta finding B2): no inspections and no
    // diagnostics in the editor, nothing in the Problems window's File tab, also after the
    // checks had time to run.
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        draw(cx);
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    assert_eq!(
        problems(&bench, cx),
        Vec::<String>::new(),
        "no problems in String.java"
    );
    assert_eq!(
        bench.read_with(cx, |b, cx| b.problems_list(cx).len()),
        0,
        "the File tab lists nothing for String.java"
    );
    action(&bench, cx, "Back");
    until(cx, 10, "⌘[ comes back from String.java", |cx| {
        active_name(&bench, cx) == "test.java"
    });

    // Typing `pers` on a new line in `main` offers `person`.
    let (line, _) = spot(MAIN_JAVA, "        System.out", 0);
    caret_to(&bench, cx, line, 0);
    cx.simulate_input("        ");
    let shown = typed_completion(&bench, cx, "pers", "person", 120);
    assert!(
        shown.iter().any(|s| s.starts_with("person")),
        "typing offers `person`: {shown:?}"
    );
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

// --------------------------------------------------------------------------- hand test 207

const TEST_PY: &str = "def greet(name):\n    return \"Hello \" + name\n";
const TEST_TS: &str =
    "export function greet(name: string): string {\n    return \"Hello \" + name;\n}\n";
const TEST_JAVA: &str = "public class Main {\n    public static void main(String[] args) {\n        System.out.println(\"hi\");\n    }\n}\n";

/// Hand test 207 (`SPEC-0059` v4, `SPEC-0117` v2): `sprachen/test.java` holds `public class Main`,
/// an error; with it active its tab and its folders up to the project are marked and the Python
/// interpreter is not in the status bar. Renaming `Main` to `test` without saving removes the
/// marks, typing `Main` again brings them back. With `test.py` active the interpreter is shown,
/// with `test.ts` not; a warning alone in `test.ts` marks nothing. Skipped without jdtls.
#[gpui::test]
fn hand_test_207_files_with_errors_and_the_python_widget(cx: &mut TestAppContext) {
    let root = workspace(
        "207",
        &[
            ("sprachen/test.py", TEST_PY),
            ("sprachen/test.ts", TEST_TS),
            ("sprachen/test.java", TEST_JAVA),
        ],
    );
    if !has_server(LanguageId::Java, &root) {
        eprintln!("skipped: jdtls is missing");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let dir = root.join("sprachen");
    let java = root.join("sprachen/test.java");
    open(&bench, cx, &root.join("sprachen/test.py"));
    open(&bench, cx, &root.join("sprachen/test.ts"));
    let editor = open(&bench, cx, &java);
    let marked = |bench: &Bench, cx: &mut VisualTestContext, path: &Path, dir: bool| {
        bench.read_with(cx, |b, cx| b.problem_files(cx).marked(path, dir))
    };

    until(
        cx,
        COLD,
        "test.java, its folder and the project are marked",
        |cx| {
            marked(&bench, cx, &java, false)
                && marked(&bench, cx, &dir, true)
                && marked(&bench, cx, &root, true)
        },
    );
    assert!(
        !bench.read_with(cx, |b, _| b.python_widget_shown()),
        "no Python for test.java"
    );

    // `Main` → `test` without saving: the marks go.
    let at = TEST_JAVA.find("Main").unwrap();
    editor.update(cx, |e, cx| e.select_range(at..at + 4, cx));
    cx.simulate_input("test");
    cx.run_until_parked();
    let mut seen = problems(&bench, cx);
    until(cx, 120, "the marks go without saving", |cx| {
        let now = problems(&bench, cx);
        if now != seen {
            eprintln!("problems after the rename: {now:?}");
            seen = now;
        }
        !marked(&bench, cx, &java, false) && !marked(&bench, cx, &dir, true)
    });
    assert!(editor.read_with(cx, |e, _| e.is_modified()));
    // `Main` again: they come back.
    editor.update(cx, |e, cx| e.select_range(at..at + 4, cx));
    cx.simulate_input("Main");
    cx.run_until_parked();
    assert_eq!(text(&bench, cx), TEST_JAVA, "the class is `Main` again");
    let mut seen = Vec::new();
    until(cx, 120, "the marks come back", |cx| {
        let now = problems(&bench, cx);
        if now != seen {
            eprintln!("problems: {now:?}");
            seen = now;
        }
        marked(&bench, cx, &java, false) && marked(&bench, cx, &dir, true)
    });

    // The interpreter follows the active tab.
    open(&bench, cx, &root.join("sprachen/test.py"));
    assert!(
        bench.read_with(cx, |b, _| b.python_widget_shown()),
        "Python for test.py"
    );
    let ts = root.join("sprachen/test.ts");
    open(&bench, cx, &ts);
    assert!(
        !bench.read_with(cx, |b, _| b.python_widget_shown()),
        "no Python for test.ts"
    );

    // A warning alone does not mark the file.
    if has_server(LanguageId::TypeScript, &root) {
        caret_to(&bench, cx, 1, 0);
        cx.simulate_input("    let unused = 1;\n");
        cx.run_until_parked();
        let ts_editor = bench.read_with(cx, |b, _| b.active_editor_entity().unwrap());
        until(
            cx,
            120,
            "the TypeScript server has looked at the file",
            |cx| ts_editor.read_with(cx, |e, _| !e.server_problems().is_empty()),
        );
        assert!(
            ts_editor.read_with(cx, |e, _| e.problems().iter().all(
                |p| p.severity != ferris_idea_workbench::diagnostics_model::Severity::Error
            )),
            "only a warning or hint"
        );
        assert!(!marked(&bench, cx, &ts, false), "a warning marks nothing");
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 18

/// Hand test 18 (`SPEC-0098`): Ferris IDEA's own inspections mark an empty `catch` block in Java,
/// `==` in JavaScript and a bare `except:` in Python with "Ferris IDEA: …", and Inspect Code over the
/// whole project lists all three. Needs no language server.
#[gpui::test]
fn hand_test_18_own_inspections_and_inspect_code(cx: &mut TestAppContext) {
    let root = workspace(
        "18",
        &[
            (
                "Insp.java",
                "class Insp {\n    void f() {\n        try { g(); } catch (Exception e) {}\n    }\n    void g() {}\n}\n",
            ),
            ("insp.js", "function same(a, b) {\n  return a == b;\n}\n"),
            (
                "insp.py",
                "def safe():\n    try:\n        return 1\n    except:\n        return 0\n",
            ),
        ],
    );
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    for (file, start) in [
        ("Insp.java", "Ferris IDEA: Empty 'catch' block"),
        ("insp.js", "Ferris IDEA: '==' and '!='"),
        ("insp.py", "Ferris IDEA: Too broad exception clause"),
    ] {
        open(&bench, cx, &root.join(file));
        until(cx, 30, &format!("{file} is marked"), |cx| {
            problems(&bench, cx).iter().any(|m| m.starts_with(start))
        });
    }
    bench.update_in(cx, |b, window, cx| b.inspect_code(window, cx));
    cx.run_until_parked();
    let (choices, at) = bench
        .read_with(cx, |b, _| b.inspect_dialog())
        .expect("Inspect Code asks for the scope");
    assert_eq!((choices[0].as_str(), at), ("Whole project", 0));
    bench.update_in(cx, |b, window, cx| b.confirm_inspect_dialog(window, cx));
    let mut rows = Vec::new();
    until(cx, 60, "Inspect Code lists the three findings", |cx| {
        rows = bench
            .read_with(cx, |b, cx| (!b.inspecting()).then(|| b.problems_rows(cx)))
            .flatten()
            .unwrap_or_default();
        [
            "Empty 'catch' block (",
            "Equality operator may cause type coercion (",
            "Too broad exception clause (",
        ]
        .iter()
        .all(|name| rows.iter().any(|r| r.starts_with(name)))
    });
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 20

const NODE_JS: &str = "function add(a, b) {\n  const sum = a + b;\n  return sum;\n}\n\nfunction helper(n) {\n  const next = n + 1;\n  return next;\n}\n\nconst result = add(2, 3);\nconsole.log(\"result\", result, helper(1));\n";

/// Hand test 20 (`SPEC-0039` v5): a breakpoint in a `.js` file, Debug: the program stops there
/// with its variables and the stack; Step Into goes into `add`, Step Over to its next line,
/// Step Out back; on the line `console.log("result", result, helper(1))` Step Into goes into
/// `helper` (beta finding B4), Resume runs to the end with the output. Skipped without js-debug
/// or Node.js.
#[gpui::test]
fn hand_test_20_debug_node(cx: &mut TestAppContext) {
    let usable = ferris_idea_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "js-debug" && a.command.is_some())
        && std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
    if !usable {
        eprintln!("skipped: js-debug or node is missing");
        return;
    }
    let root = workspace("20", &[("debug/app.js", NODE_JS)]);
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let editor = open(&bench, cx, &root.join("debug/app.js"));
    let (line, _) = spot(NODE_JS, "const result", 0);
    editor.update(cx, |e, cx| e.toggle_breakpoint(line, cx));
    cx.run_until_parked();

    keys(cx, "ctrl-shift-d");
    until(cx, 120, "the program stops at the breakpoint", |cx| {
        paused(&bench, cx)
    });
    until(cx, 30, "the stack shows the top level", |cx| {
        stack(&bench, cx).contains("app.js")
    });
    let before = stack(&bench, cx);
    action(&bench, cx, "StepInto");
    until(cx, 30, "Step Into goes into add", |cx| {
        paused(&bench, cx) && stack(&bench, cx) != before && stack(&bench, cx).contains("add")
    });
    until(cx, 30, "the variables show a and b", |cx| {
        let vars = variables(&bench, cx);
        vars.iter().any(|v| v.starts_with("a") && v.contains('2'))
            && vars.iter().any(|v| v.starts_with("b") && v.contains('3'))
    });
    let before = stack(&bench, cx);
    action(&bench, cx, "StepOver");
    until(cx, 30, "Step Over goes to the next line", |cx| {
        paused(&bench, cx) && stack(&bench, cx) != before
    });
    // Back at the top level, one line on: the line whose argument calls `helper`.
    let mut at_log = false;
    for _ in 0..4 {
        let before = stack(&bench, cx);
        let step = if before.lines().next().unwrap_or_default().contains("add") {
            "StepOut"
        } else {
            "StepOver"
        };
        action(&bench, cx, step);
        until(cx, 30, &format!("{step} stops again"), |cx| {
            paused(&bench, cx) && stack(&bench, cx) != before
        });
        let (log_line, _) = spot(NODE_JS, "console.log", 0);
        let now = stack(&bench, cx);
        let top = now.lines().next().unwrap_or_default().to_owned();
        if !top.contains("add") && top.ends_with(&format!(":{} (app.js)", log_line + 1)) {
            at_log = true;
            break;
        }
    }
    assert!(
        at_log,
        "stopped on the console.log line: {}",
        stack(&bench, cx)
    );
    // F7, the key of the hand test, with the editor focused as after the stop.
    let before = stack(&bench, cx);
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    draw(cx);
    keys(cx, "f7");
    until(cx, 30, "Step Into goes into helper", |cx| {
        (paused(&bench, cx) && stack(&bench, cx) != before) || ended(&bench, cx)
    });
    assert!(
        !ended(&bench, cx)
            && stack(&bench, cx)
                .lines()
                .next()
                .is_some_and(|top| top.contains("helper")),
        "Step Into on the console.log line stops in helper: {}",
        stack(&bench, cx)
    );
    action(&bench, cx, "Resume");
    until(cx, 60, "the program runs to its end", |cx| {
        ended(&bench, cx)
    });

    // The same line stopped at by a breakpoint of its own, in a new session: F7 enters helper.
    let (log_line, _) = spot(NODE_JS, "console.log", 0);
    editor.update(cx, |e, cx| {
        e.toggle_breakpoint(line, cx);
        e.toggle_breakpoint(log_line, cx);
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "ctrl-shift-d");
    until(cx, 120, "the program stops on the console.log line", |cx| {
        paused(&bench, cx)
            && stack(&bench, cx)
                .lines()
                .next()
                .is_some_and(|top| top.ends_with(&format!(":{} (app.js)", log_line + 1)))
    });
    let before = stack(&bench, cx);
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    draw(cx);
    keys(cx, "f7");
    until(
        cx,
        30,
        "Step Into from the breakpoint goes into helper",
        |cx| (paused(&bench, cx) && stack(&bench, cx) != before) || ended(&bench, cx),
    );
    assert!(
        !ended(&bench, cx)
            && stack(&bench, cx)
                .lines()
                .next()
                .is_some_and(|top| top.contains("helper")),
        "Step Into from the breakpoint stops in helper: {}",
        stack(&bench, cx)
    );
    action(&bench, cx, "Resume");
    until(cx, 60, "the second run ends", |cx| ended(&bench, cx));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 19

/// The row of the test tree whose name is `name`, by its index among the visible rows.
fn test_row(bench: &Bench, cx: &mut VisualTestContext, name: &str) -> usize {
    bench.read_with(cx, |b, cx| {
        b.run_view()
            .and_then(|v| v.read(cx).test_results())
            .and_then(|t| t.rows.iter().position(|(_, n)| n == name))
            .unwrap_or_else(|| panic!("the tree has a row {name}"))
    })
}

/// Clicks the test row `name` with real mouse events where it was painted: once, or twice for a
/// double click.
fn click_test_row(bench: &Bench, cx: &mut VisualTestContext, name: &str, clicks: usize) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    draw(cx);
    let i = test_row(bench, cx, name);
    let id = format!("test-row-{i}");
    let r = cx
        .update(|window, _| {
            let tree = semantics::tree(semantics::window_id(window))?;
            tree.iter()
                .filter(|n| n.test_id() == Some(id.as_str()))
                .filter_map(|n| n.visible_bounds)
                .filter(|r| r.w > 0. && r.h > 0.)
                .last()
        })
        .unwrap_or_else(|| panic!("the row {name} is painted"));
    let at = gpui::point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    for count in 1..=clicks {
        cx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::none(),
            click_count: count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::none(),
            click_count: count,
        });
    }
    draw(cx);
}

/// The first line the Run window's console shows.
fn console_top(bench: &Bench, cx: &mut VisualTestContext) -> usize {
    bench.read_with(cx, |b, cx| {
        b.run_view()
            .map_or(usize::MAX, |v| v.read(cx).console_top_line())
    })
}

/// Waits until the active editor shows `file` with the caret on 0-based `line`.
fn opened_at(bench: &Bench, cx: &mut VisualTestContext, file: &str, line: usize, what: &str) {
    until(cx, 10, what, |cx| {
        active_name(bench, cx) == file
            && bench
                .read_with(cx, |b, cx| b.caret_position(cx))
                .is_some_and(|(l, _)| l == line + 1)
    });
}

/// The interpreter the installed `pytest` program runs with, if any: the test file then runs
/// with a Python that has pytest.
fn python_with_pytest() -> Option<PathBuf> {
    let program = std::process::Command::new("sh")
        .args(["-c", "command -v pytest"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|p| !p.is_empty())?;
    let first = std::fs::read_to_string(&program).ok()?;
    let python = PathBuf::from(first.lines().next()?.strip_prefix("#!")?.trim());
    std::process::Command::new(&python)
        .args(["-c", "import pytest"])
        .output()
        .is_ok_and(|o| o.status.success())
        .then_some(python)
}

/// Hand test 19 for pytest (`SPEC-0045` v3): ⌃⇧R on a test file runs it and shows the tests as a
/// tree, the failing one with its message. Skipped without a Python that has pytest.
const PYTEST_CALC: &str = "\"\"\"Tests of the sums.\"\"\"\n\n\ndef test_good():\n    assert 1 + 2 == 3\n\n\ndef test_broken():\n    assert 1 + 2 == 4, \"want 4\"\n";

#[gpui::test]
fn hand_test_19_pytest_tree(cx: &mut TestAppContext) {
    let Some(python) = python_with_pytest() else {
        eprintln!("skipped: no Python with pytest");
        return;
    };
    let root = workspace("19py", &[("test_calc.py", PYTEST_CALC)]);
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    bench.update(cx, |b, cx| b.set_interpreter(Some(python), cx));
    open(&bench, cx, &root.join("test_calc.py"));
    keys(cx, "ctrl-shift-r");
    let mut last = String::new();
    until(
        cx,
        120,
        "the tests are a tree with the failure and its message",
        |cx| {
            let out = run_output(&bench, cx, None);
            if out != last {
                eprintln!("run: {out}");
                last = out;
            }
            bench.read_with(cx, |b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).test_results().is_some_and(|t| {
                        t.rows.iter().any(|(_, n)| n.contains("test_good"))
                            && t.rows.iter().any(|(_, n)| n.contains("test_broken"))
                            && t.details.as_deref().is_some_and(|d| d.contains("want 4"))
                    })
                })
            })
        },
    );

    // SPEC-0088 v5 behaviour 12 (beta blocker): a double click on a test opens its file at the
    // test's line; Enter on a selected test does the same.
    let (good, _) = spot(PYTEST_CALC, "def test_good", 4);
    let (broken, _) = spot(PYTEST_CALC, "def test_broken", 4);
    caret_to(&bench, cx, 0, 0);
    click_test_row(&bench, cx, "test_good", 2);
    opened_at(
        &bench,
        cx,
        "test_calc.py",
        good,
        "a double click puts the caret on test_good",
    );
    click_test_row(&bench, cx, "test_broken", 1);
    assert_eq!(
        caret(&bench, cx).0,
        good,
        "a single click only selects and shows the message"
    );
    keys(cx, "enter");
    opened_at(
        &bench,
        cx,
        "test_calc.py",
        broken,
        "Enter puts the caret on test_broken",
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The folder of the Jest package of the installed `jest` program, if any.
fn installed_jest() -> Option<PathBuf> {
    let program = std::process::Command::new("sh")
        .args(["-c", "command -v jest"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|p| !p.is_empty())?;
    let real = std::fs::canonicalize(program).ok()?;
    // …/node_modules/jest/bin/jest.js
    let package = real.parent()?.parent()?.to_path_buf();
    package.join("package.json").is_file().then_some(package)
}

const JEST_CALC: &str = "// Tests of the sums.\n\ntest(\"good\", () => {\n  expect(1 + 2).toBe(3);\n});\n\ntest(\"broken\", () => {\n  expect(1 + 2).toBe(4);\n});\n";

/// Hand test 19 for Jest (`SPEC-0045` v3, `SPEC-0051` v2): ⌃⇧R on a test file of a package that
/// depends on Jest runs it and shows the tests as a tree with the failure. The installed Jest is
/// linked into the package's `node_modules`, so nothing is downloaded. Skipped without Jest.
#[gpui::test]
fn hand_test_19_jest_tree(cx: &mut TestAppContext) {
    let Some(jest) = installed_jest() else {
        eprintln!("skipped: jest is not installed");
        return;
    };
    let root = workspace(
        "19js",
        &[
            (
                "package.json",
                "{\n  \"name\": \"calc\",\n  \"private\": true,\n  \"devDependencies\": { \"jest\": \"*\" }\n}\n",
            ),
            ("calc.test.js", JEST_CALC),
        ],
    );
    std::fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
    std::os::unix::fs::symlink(&jest, root.join("node_modules/jest")).unwrap();
    std::os::unix::fs::symlink("../jest/bin/jest.js", root.join("node_modules/.bin/jest")).unwrap();
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("calc.test.js"));
    keys(cx, "ctrl-shift-r");
    let mut last = String::new();
    until(cx, 180, "the tests are a tree with the failure", |cx| {
        let out = run_output(&bench, cx, None);
        if out != last {
            eprintln!("run: {out}");
            last = out;
        }
        bench.read_with(cx, |b, cx| {
            b.run_view().is_some_and(|v| {
                v.read(cx).test_results().is_some_and(|t| {
                    t.rows.iter().any(|(_, n)| n.contains("good"))
                        && t.rows.iter().any(|(_, n)| n.contains("broken"))
                        && t.details.is_some()
                })
            })
        })
    });

    // SPEC-0088 v5 behaviour 13 (B3): the failure's details start at their first line, where
    // Expected and Received are, not at the end of the stack trace. A low window, so that the
    // details are longer than the console.
    cx.simulate_resize(gpui::size(px(1372.), px(420.)));
    draw(cx);
    let details = bench.read_with(cx, |b, cx| {
        b.run_view()
            .and_then(|v| v.read(cx).test_results())
            .and_then(|t| t.details)
            .unwrap_or_default()
    });
    assert!(details.contains("Expected"), "{details}");
    assert!(
        bench.read_with(cx, |b, cx| b
            .run_view()
            .is_some_and(|v| v.read(cx).console_scrollable())),
        "the details are longer than the console"
    );
    assert_eq!(console_top(&bench, cx), 0, "the details start at the top");
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    draw(cx);

    // SPEC-0088 v5 behaviour 12 (beta blocker), with the file closed: a double click opens it at
    // the test; Enter on another selected test moves there.
    action(&bench, cx, "CloseContent");
    assert_ne!(
        active_name(&bench, cx),
        "calc.test.js",
        "the file is closed"
    );
    let (good, _) = spot(JEST_CALC, "test(\"good\"", 0);
    let (broken, _) = spot(JEST_CALC, "test(\"broken\"", 0);
    click_test_row(&bench, cx, "broken", 2);
    opened_at(
        &bench,
        cx,
        "calc.test.js",
        broken,
        "a double click opens the test file at broken",
    );
    click_test_row(&bench, cx, "good", 1);
    assert_eq!(
        console_top(&bench, cx),
        0,
        "a selected test's message starts at the top"
    );
    keys(cx, "enter");
    opened_at(
        &bench,
        cx,
        "calc.test.js",
        good,
        "Enter puts the caret on good",
    );

    // SPEC-0045 v6 behaviour 12 (B5): the ▶ of a Jest test offers Debug too, and Debug stops at a
    // breakpoint in the test. Skipped without js-debug.
    let js_debug = ferris_idea_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "js-debug" && a.command.is_some());
    let file = root.join("calc.test.js");
    let editor = open(&bench, cx, &file);
    until(cx, 20, "the tests have their ▶", |cx| {
        bench.update_in(cx, |b, window, cx| {
            b.run_marker_menu(&file, broken, window, cx)
        });
        cx.run_until_parked();
        bench.read_with(cx, |b, _| b.jvm_popup()).is_some()
    });
    let popup = bench.read_with(cx, |b, _| b.jvm_popup()).unwrap();
    let items = popup.read_with(cx, |p, _| p.items().to_vec());
    assert_eq!(
        items,
        ["Run 'calc.test.js: broken'", "Debug 'calc.test.js: broken'"],
        "the ▶ menu of a Jest test"
    );
    if !js_debug {
        eprintln!("skipped: Debug of a Jest test needs js-debug");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    editor.update(cx, |e, cx| e.toggle_breakpoint(broken + 1, cx));
    cx.run_until_parked();
    popup.update(cx, |p, cx| {
        p.select(1);
        cx.emit(p.accept());
    });
    until(
        cx,
        180,
        "Debug stops at the breakpoint in the Jest test",
        |cx| {
            paused(&bench, cx)
                && stack(&bench, cx)
                    .lines()
                    .next()
                    .is_some_and(|top| top.ends_with(&format!(":{} (calc.test.js)", broken + 2)))
        },
    );
    action(&bench, cx, "Resume");
    until(cx, 120, "the debugged tests end", |cx| ended(&bench, cx));
    let _ = std::fs::remove_dir_all(&root);
}

// --------------------------------------------------------------------------- hand test 197

/// Hand test 197 steps 3–4 (`SPEC-0278`, IMP-0006): a project opened without trust is in safe
/// mode; its file can be edited, but ⌃⇧R starts nothing and the status bar says why; once the
/// project is trusted (for this process here, as Trust Project does in the user's trust file),
/// ⌃⇧R runs it. The native question, the banner as drawn, the Trusted Locations page and the
/// restart (steps 1–2, 5–6) stay in the hand test: they need the system alert and the user's trust
/// file, which a test must not write.
#[gpui::test]
fn hand_test_197_safe_mode_until_trusted(cx: &mut TestAppContext) {
    let root = workspace("197", &[("list.py", "print(\"listed\")\n")]);
    let (bench, cx) = window(&root, cx);
    assert!(
        bench.read_with(cx, |b, _| b.safe_mode()),
        "an untrusted project opens in safe mode"
    );
    let editor = open(&bench, cx, &root.join("list.py"));
    caret_to(&bench, cx, 1, 0);
    cx.simulate_input("print(\"more\")\n");
    draw(cx);
    assert!(
        editor.read_with(cx, |e, _| e.is_modified()),
        "editing works in safe mode"
    );

    // ⌃⇧R: nothing starts; the status bar says that the project is not trusted.
    keys(cx, "ctrl-shift-r");
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        draw(cx);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !run_output(&bench, cx, None).contains("listed"),
        "nothing runs in safe mode"
    );
    let said = status(&bench, cx);
    assert!(
        said.contains("Safe mode: this project is not trusted"),
        "{said}"
    );

    // Trusted: it runs, with the edited text (Run saves first).
    ferris_idea_core::trust::trust_for_process(&root);
    bench.update(cx, |b, cx| b.refresh_trust(cx));
    draw(cx);
    assert!(
        !bench.read_with(cx, |b, _| b.safe_mode()),
        "trusted: no safe mode"
    );
    keys(cx, "ctrl-shift-r");
    until(cx, 60, "the trusted project runs", |cx| {
        let out = run_output(&bench, cx, None);
        out.contains("listed") && out.contains("more")
    });
    let _ = std::fs::remove_dir_all(&root);
}

// ----------------------------------------------------------------------------- hand test 6

const PRICES: &str = "def total(a, b):\n    return a + b\n\nprint(total(1, 2))\n";
const REPORT: &str = "from prices import total\n\n# total of the day\nprint(total(3, 4))\n";

/// Hand test 6 (`SPEC-0066` v2), in Python instead of the hand test's C++ files: ⇧⌘R with `total`
/// and `sum` finds 5 matches in 2 files; Replace All changes the open file in its editor, unsaved,
/// and the closed one on disk; one ⌘Z takes it back in the open file; the closed file's Local
/// History keeps the old text (what Show History lists). How the fields look stays in the hand
/// test.
#[gpui::test]
fn hand_test_6_replace_in_files(cx: &mut TestAppContext) {
    let root = workspace(
        "6",
        &[("shop/prices.py", PRICES), ("shop/report.py", REPORT)],
    );
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    let prices = root.join("shop/prices.py");
    let editor = open(&bench, cx, &prices);

    keys(cx, "cmd-shift-r");
    let view = bench
        .read_with(cx, |b, _| b.find_in_files())
        .expect("Replace in Files opens");
    view.update(cx, |v, cx| {
        v.set_query("total", cx);
        v.set_replacement("sum", cx);
    });
    until(cx, 30, "5 matches in 2 files", |cx| {
        view.read_with(cx, |v, _| {
            v.replacing() && v.summary() == "5 matches in 2 files"
        })
    });
    view.update(cx, |v, cx| v.replace(true, cx));
    until(cx, 30, "both files are changed", |cx| {
        std::fs::read_to_string(root.join("shop/report.py"))
            .is_ok_and(|t| t.matches("sum").count() == 3 && !t.contains("total"))
            && text(&bench, cx).matches("sum").count() == 2
    });
    assert!(
        editor.read_with(cx, |e, _| e.is_modified()),
        "the open file is changed in its editor"
    );
    // The list searched again shows what the editors hold: nothing is left (SPEC-0066 v7
    // behaviour 19, beta finding B1), not the disk's text of the open file.
    until(cx, 30, "the list shows no match after Replace All", |cx| {
        view.read_with(cx, |v, _| v.summary().is_empty())
    });
    assert_eq!(
        std::fs::read_to_string(&prices).unwrap(),
        PRICES,
        "and not on disk"
    );

    // One ⌘Z in the open file takes the whole replacement back.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    keys(cx, "cmd-z");
    assert_eq!(text(&bench, cx), PRICES, "one ⌘Z takes it back");

    // The closed file's history keeps the text before the replacement.
    let history = ferris_idea_core::LocalHistory::open(&root).unwrap();
    let revisions = history.revisions("shop/report.py");
    assert!(
        revisions.iter().any(|r| history
            .content("shop/report.py", r.id)
            .is_ok_and(|c| c.as_deref() == Some(REPORT.as_bytes()))),
        "the old text is in the Local History"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ----------------------------------------------------------------------------- hand test 7

/// Hand test 7 (`SPEC-0092`), with pyright's errors in two Python files: ⌘6 lists the active
/// file's error under its name; ↓ and Enter put the caret on it; the Project tab groups the
/// errors of both files by file; fixing the error without saving takes it off the list. Skipped
/// without pyright.
#[gpui::test]
fn hand_test_7_problems_window(cx: &mut TestAppContext) {
    let root = workspace(
        "7",
        &[
            ("a.py", "value = 1\nprint(missing_a)\n"),
            ("b.py", "print(missing_b)\n"),
        ],
    );
    if !has_server(LanguageId::Python, &root) {
        eprintln!("skipped: pyright is missing");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("b.py"));
    until(cx, COLD, "b.py has its error", |cx| {
        bench
            .read_with(cx, |b, cx| b.active_problems(cx))
            .is_some_and(|(errors, _)| errors >= 1)
    });
    let editor = open(&bench, cx, &root.join("a.py"));
    until(cx, 120, "a.py has its error", |cx| {
        bench
            .read_with(cx, |b, cx| b.active_problems(cx))
            .is_some_and(|(errors, _)| errors >= 1)
    });

    let rows = |cx: &mut VisualTestContext| {
        bench
            .read_with(cx, |b, cx| b.problems_rows(cx))
            .unwrap_or_default()
    };
    keys(cx, "cmd-6");
    until(cx, 10, "⌘6 lists the error of a.py under its name", |cx| {
        let rows = rows(cx);
        rows.first().is_some_and(|r| r.starts_with("a.py"))
            && rows.iter().any(|r| r.contains("missing_a"))
    });
    keys(cx, "down");
    keys(cx, "enter");
    assert_eq!(
        caret(&bench, cx).0,
        1,
        "Enter puts the caret on the error's line"
    );

    bench.update(cx, |b, cx| {
        b.set_problems_scope(ferris_idea_workbench::problems_model::Scope::Project, cx)
    });
    until(cx, 10, "the Project tab groups both files", |cx| {
        let rows = rows(cx);
        rows.iter().any(|r| r.starts_with("a.py"))
            && rows.iter().any(|r| r.starts_with("b.py"))
            && rows.iter().any(|r| r.contains("missing_a"))
            && rows.iter().any(|r| r.contains("missing_b"))
    });

    // Fixing the error without saving takes it off the list.
    let at = "value = 1\nprint(".len();
    editor.update(cx, |e, cx| e.select_range(at..at + "missing_a".len(), cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_input("value");
    draw(cx);
    until(cx, 120, "the list follows the fix", |cx| {
        let rows = rows(cx);
        !rows.iter().any(|r| r.contains("missing_a"))
            && rows.iter().any(|r| r.contains("missing_b"))
    });
    let _ = std::fs::remove_dir_all(&root);
}

// --------------------------------------------------------------------------- hand test 208

/// Whether any of the popups of hand test 208 step 4 is open.
fn popup_open(bench: &Bench, cx: &mut VisualTestContext) -> Vec<&'static str> {
    bench.read_with(cx, |b, _| {
        let mut open = Vec::new();
        if b.run_chooser().is_some() {
            open.push("Run…");
        }
        if b.goto_popup().is_some() {
            open.push("Go to Line / Recent Files");
        }
        if b.everywhere().is_some() {
            open.push("Search Everywhere");
        }
        if b.structure_popup().is_some() {
            open.push("File Structure");
        }
        if b.intentions_popup().is_some() {
            open.push("⌥↩");
        }
        open
    })
}

/// The clicked line lies near the bottom of the editor, below where the popups are drawn: a
/// click on a popup chooses in it instead (behaviour 28b).
const POPUP_PY: &str = concat!(
    "def safe():\n    try:\n        return 1\n    except:\n        return 0\n",
    "\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n",
    "print(safe())\n"
);

/// Hand test 208 step 4 (`SPEC-0275` v4 behaviour 28): Run… (⌃⌥R), Go to Line (⌘L), Recent
/// Files (⌘E), Search Everywhere (⇧⇧), File Structure (⌘F12) and ⌥↩ each close at a click into the
/// editor, and the caret goes where the click was. The gutter's ▶ menu, the click into the tree,
/// ⌘Tab and the run configuration dialog (steps 1–3, 5, 6) stay in the hand test.
#[gpui::test]
fn hand_test_208_popups_close_on_a_click_beside_them(cx: &mut TestAppContext) {
    // `package.json` gives Run… its detected entries, as in the hand test's project.
    let root = workspace(
        "208",
        &[
            ("safe.py", POPUP_PY),
            ("other.py", "x = 1\n"),
            (
                "package.json",
                "{\n  \"name\": \"popups\",\n  \"scripts\": { \"hello\": \"echo hello\" }\n}\n",
            ),
        ],
    );
    ferris_idea_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    open(&bench, cx, &root.join("other.py"));
    let editor = open(&bench, cx, &root.join("safe.py"));
    // ⌥↩ needs a finding at the caret: Ferris IDEA's own one on the bare `except:`.
    until(cx, 30, "the bare except is marked", |cx| {
        problems(&bench, cx)
            .iter()
            .any(|m| m.starts_with("Ferris IDEA: Too broad"))
    });
    let target = POPUP_PY.find("print(safe").unwrap() + 2;
    // Opened through their actions: the keys are checked by the keymap tests; this is about
    // closing.
    for (key, name) in [
        ("ChooseRunConfiguration", "Run…"),
        ("GotoLine", "Go to Line / Recent Files"),
        ("RecentFiles", "Go to Line / Recent Files"),
        ("SearchEverywhere", "Search Everywhere"),
        ("FileStructurePopup", "File Structure"),
        ("ShowIntentionActions", "⌥↩"),
    ] {
        let at = POPUP_PY.find("except").unwrap() + 2;
        editor.update(cx, |e, cx| e.select_range(at..at, cx));
        bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
        draw(cx);
        action(&bench, cx, key);
        until(cx, 10, &format!("{key} opens {name}"), |cx| {
            popup_open(&bench, cx).contains(&name)
        });
        // A click into the editor text, on a line below the popup.
        draw(cx);
        let below = editor
            .read_with(cx, |e, _| e.point_below(target))
            .expect("the line is drawn");
        let spot = gpui::point(below.x, below.y - px(4.));
        cx.simulate_click(spot, gpui::Modifiers::none());
        draw(cx);
        assert!(
            popup_open(&bench, cx).is_empty(),
            "{name} closes at the click: {:?}",
            popup_open(&bench, cx)
        );
        assert_eq!(
            editor.read_with(cx, |e, _| e.caret()),
            target,
            "{key}: the caret goes where the click was"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
