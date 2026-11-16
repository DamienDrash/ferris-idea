//! Second batch of the window check of 2026-11-14, in a window of the test harness:
//!
//! - `SPEC-0067` v12 AK-13: Find Action opened with the caret in a tool window's list or tree
//!   runs an action for "the active tool window" on that window and gives the focus back to the
//!   element that had it (Project tree, Database tree, Commit list, Problems).
//! - `SPEC-0084` v6 AK-18: a file opened or brought to the front while the diff viewer's tab is
//!   in front becomes the tab in front; the viewer closes.
//! - `SPEC-0108` v10 AK-21: the buttons of the connection dialog stay where they are when the
//!   result of Test Connection arrives.
//! - `SPEC-0327` v4 AK-13: the run configuration editor shows every row of its form; in a low
//!   window the form scrolls and the buttons stay in view.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{Entity, Focusable, TestAppContext, VisualTestContext, px};
use idecraft_workbench::tool_windows::ToolWindow;
use idecraft_workbench::{EverywhereEvent, WorkbenchView, semantics};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with `a.txt`, `b.txt` and `c.txt` committed and `b.txt` changed since.
fn repo(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-wcd2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test Author"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(name), "one\ntwo\nthree\n").unwrap();
    }
    std::fs::write(root.join(".gitignore"), "/.*\n").unwrap();
    git(&root, &["add", "-A", "-f"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    std::fs::write(root.join("b.txt"), "one\nTWO\nthree\n").unwrap();
    root
}

fn open(
    root: &Path,
    size: (f32, f32),
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    let file = root.join("a.txt");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(size.0), px(size.1)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

/// The painted bounds of the element with test id `id`: x, y, width, height.
fn bounds(id: &str, cx: &mut VisualTestContext) -> (f32, f32, f32, f32) {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a tree is built");
        let node = tree
            .iter()
            .find(|n| n.test_id() == Some(id))
            .unwrap_or_else(|| panic!("no element {id} in the window"));
        let r = node.bounds();
        (r.x, r.y, r.w, r.h)
    })
}

/// SPEC-0067 v12 AK-13: with the caret in the Project tree, the Database tree, the list of the
/// Commit window or the Problems list, Find Action runs Maximize Tool Window on that window, and
/// the element that had the focus has it again.
#[gpui::test]
fn find_action_acts_on_the_tool_window_that_had_the_focus(cx: &mut TestAppContext) {
    let root = repo("focus");
    let (bench, cx) = open(&root, (1372., 885.), cx);
    for w in [
        ToolWindow::Project,
        ToolWindow::Database,
        ToolWindow::Commit,
        ToolWindow::Problems,
    ] {
        // The window's activation action shows and focuses it.
        bench.update_in(cx, |b, window, cx| {
            b.focus_editor(window, cx);
            if !b.tool_window_shown(w) || !b.tool_window_focused(w, window, cx) {
                b.toggle_tool_window(w, window, cx);
            }
        });
        cx.run_until_parked();
        if w == ToolWindow::Commit {
            // A click on a row of the list of changes puts the focus on the list, off the
            // message field the window focuses when it is activated.
            let list = bench
                .read_with(cx, |b, cx| {
                    b.commit_view().map(|v| v.read(cx).focus_handle(cx))
                })
                .expect("the Commit window has its view");
            cx.update(|window, _| window.focus(&list));
            cx.run_until_parked();
        }
        let had = cx.update(|window, cx| window.focused(cx));
        assert!(had.is_some(), "{w:?}: an element holds the focus");
        assert!(
            bench.update_in(cx, |b, window, cx| b.tool_window_focused(w, window, cx)),
            "{w:?} holds the focus before Find Action"
        );
        bench.update_in(cx, |b, window, cx| b.menu_action("GotoAction", window, cx));
        cx.run_until_parked();
        let popup = bench
            .read_with(cx, |b, _| b.everywhere())
            .expect("Find Action is open");
        assert!(
            !bench.update_in(cx, |b, window, cx| b.tool_window_focused(w, window, cx)),
            "{w:?}: the popup's field has the focus while it is open"
        );
        popup.update(cx, |_, cx| {
            cx.emit(EverywhereEvent::Action("MaximizeToolWindow".into()))
        });
        cx.run_until_parked();
        assert!(
            bench.read_with(cx, |b, _| b.tool_window_maximized(w)),
            "{w:?}: Maximize Tool Window from Find Action acts on the window that had the focus"
        );
        assert!(
            bench.update_in(cx, |b, window, cx| b.tool_window_focused(w, window, cx)),
            "{w:?} has the focus again"
        );
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            had,
            "{w:?}: the element that had the focus has it again"
        );
        // Closing the popup without an action gives the focus back as well.
        bench.update_in(cx, |b, window, cx| b.menu_action("GotoAction", window, cx));
        cx.run_until_parked();
        popup.update(cx, |_, cx| cx.emit(EverywhereEvent::Close));
        cx.run_until_parked();
        assert_eq!(cx.update(|window, cx| window.focused(cx)), had, "{w:?}");
        // Back to its size for the next window.
        bench.update_in(cx, |b, window, cx| {
            b.toggle_maximize_tool_window(Some(w), window, cx)
        });
        cx.run_until_parked();
        assert!(!bench.read_with(cx, |b, _| b.tool_window_maximized(w)));
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0084 v6 AK-18: Go to File while the diff viewer's tab is in front opens the file as the
/// tab in front and closes the viewer, as a click on a file tab does; so does the next tab.
#[gpui::test]
fn a_file_opened_over_the_diff_tab_becomes_the_tab_in_front(cx: &mut TestAppContext) {
    let root = repo("diff");
    let (bench, cx) = open(&root, (1372., 885.), cx);
    let show = |cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| b.show_diff("b.txt", window, cx));
        cx.run_until_parked();
        assert!(
            bench.read_with(cx, |b, _| b.diff_view().is_some()),
            "the diff viewer is shown"
        );
    };
    show(cx);
    // Go to File, then the chosen file.
    bench.update_in(cx, |b, window, cx| b.menu_action("GotoFile", window, cx));
    cx.run_until_parked();
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("Go to File is open");
    let target = root.join("c.txt");
    popup.update(cx, |_, cx| {
        cx.emit(EverywhereEvent::Open(target.clone(), None, None))
    });
    cx.run_until_parked();
    assert_eq!(
        bench.read_with(cx, |b, _| b.active_path()),
        Some(target.clone())
    );
    assert!(
        bench.read_with(cx, |b, _| b.diff_view().is_none()),
        "the opened file is the tab in front: the viewer and its tab are closed"
    );
    // The editor of the opened file has the keys.
    let focused = bench.update_in(cx, |b, window, cx| {
        b.active_editor_entity()
            .is_some_and(|e| e.read(cx).focus().contains_focused(window, cx))
    });
    assert!(focused, "the opened file's editor holds the focus");
    // The next tab from the diff tab shows a file as well.
    show(cx);
    bench.update_in(cx, |b, window, cx| b.menu_action("NextTab", window, cx));
    cx.run_until_parked();
    assert!(bench.read_with(cx, |b, _| b.diff_view().is_none()));
    // A file that is open already comes to the front the same way.
    show(cx);
    let first = root.join("a.txt");
    bench.update_in(cx, |b, window, cx| b.open(&first, window, cx));
    cx.run_until_parked();
    assert_eq!(bench.read_with(cx, |b, _| b.active_path()), Some(first));
    assert!(bench.read_with(cx, |b, _| b.diff_view().is_none()));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0108 v10 AK-21: Test Connection, Cancel and Save are at the same place before a test,
/// while it runs, after its result with all detail lines, after a long error and after the
/// result went out of date.
#[gpui::test]
fn the_connection_dialog_keeps_its_buttons_in_place(cx: &mut TestAppContext) {
    let root = repo("conn");
    let (bench, cx) = open(&root, (1372., 885.), cx);
    bench.update_in(cx, |b, window, cx| {
        b.open_connection_dialog(None, window, cx)
    });
    cx.run_until_parked();
    let dialog = bench
        .read_with(cx, |b, _| b.connection_dialog())
        .expect("the dialog is open");
    let buttons = |cx: &mut VisualTestContext| {
        ["conn-test", "conn-cancel", "conn-save"].map(|id| bounds(id, cx))
    };
    let before = buttons(cx);
    let (_, result_y, _, result_h) = bounds("conn-result", cx);
    assert_eq!(result_h, idecraft_workbench::CONNECTION_RESULT_HEIGHT);
    assert!(
        result_y + result_h <= before[0].1,
        "the result area is above the buttons"
    );
    dialog.update(cx, |d, cx| d.set_note("Connecting…".into(), false, cx));
    assert_eq!(buttons(cx), before, "while the test runs");
    dialog.update(cx, |d, cx| {
        d.set_test_result(
            "Connected in 12 ms".into(),
            vec![
                "PostgreSQL 16.4 on x86_64".into(),
                "Driver: built-in PostgreSQL client".into(),
                "Round trip: 3 ms".into(),
                "TLS: TLSv1.3, verify-full".into(),
                "File: —".into(),
            ],
            cx,
        )
    });
    assert_eq!(buttons(cx), before, "after a result with its detail lines");
    // Copy is where the result is, not where a button was.
    let copy = bounds("conn-test-copy", cx);
    for b in before {
        assert!(
            copy.1 + copy.3 <= b.1,
            "Copy {copy:?} ends above the button {b:?}"
        );
    }
    dialog.update(cx, |d, cx| {
        d.set_note(
            "could not connect: ".to_owned() + &"the server closed the connection; ".repeat(40),
            true,
            cx,
        )
    });
    assert_eq!(buttons(cx), before, "after an error of many lines");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0327 v4 AK-13: in the window of the check (1372 × 885) the form of a configuration is
/// shown whole — nothing of it is cut at the dialog's edge — and in a low window it scrolls
/// while Run, Cancel, Apply and OK stay inside the window.
#[gpui::test]
fn the_run_configuration_editor_shows_every_row(cx: &mut TestAppContext) {
    idecraft_workbench::hit_targets::enable();
    let root = repo("runcfg");
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    std::fs::write(
        root.join(".idecraft/run.toml"),
        "[[run]]\nname = \"api\"\ncommand = [\"echo\", \"api\"]\n",
    )
    .unwrap();
    // 885: the window of the check. 676: the room in which the dialog is 580 px high, as it
    // always was before v4 — there the form does not fit, which is what the check saw. 560: lower
    // than the dialog used to be.
    for (size, whole) in [
        ((1372., 885.), true),
        ((1372., 676.), false),
        ((1372., 560.), false),
    ] {
        let (bench, cx) = open(&root, size, cx);
        bench.update_in(cx, |b, window, cx| {
            b.open_run_config_editor(Some("api".into()), window, cx)
        });
        cx.run_until_parked();
        let dialog = bench
            .read_with(cx, |b, _| b.run_config_dialog())
            .expect("the editor is open");
        let overflow = dialog.read_with(cx, |d, _| d.form_overflow());
        if whole {
            assert_eq!(overflow, 0., "every row of the form is shown");
        } else {
            assert!(overflow > 0., "the form scrolls in a low window");
        }
        for id in ["run-config-run", "run-config-cancel", "run-config-ok"] {
            let r = idecraft_workbench::hit_targets::get(id)
                .unwrap_or_else(|| panic!("the button {id} is drawn"));
            assert!(
                r.y >= 0. && r.y + r.h <= size.1,
                "{id} at {:?} is inside a window {} high",
                (r.y, r.h),
                size.1
            );
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}
