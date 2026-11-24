//! A failed install is visible (`SPEC-0117` v5 behaviour 9, `SPEC-0070` v9 behaviour 4a): the
//! Python Packages window opens with the command and its output, and the failure stays in the
//! status bar in front of the problem at the caret until the caret moves. The environment is a
//! fixture made the way `uv venv` makes one (no pip), `uv` a stub found through an injected
//! `PATH` (`SPEC-0171` v2 behaviour 4).

#![cfg(target_os = "macos")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use ferris_idea_workbench::WorkbenchView;
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const PYRIGHT: &str = "Import \"requests\" could not be resolved";

fn workspace(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferris-idea-pyfail-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let env = root.join(".venv");
    std::fs::create_dir_all(env.join("bin")).unwrap();
    std::fs::create_dir_all(env.join("lib/python3.12/site-packages")).unwrap();
    std::fs::write(
        env.join("pyvenv.cfg"),
        "home = /usr/bin\nuv = 0.5.1\nversion_info = 3.12.4\n",
    )
    .unwrap();
    std::fs::write(env.join("bin/python"), "").unwrap();
    std::fs::write(root.join("requirements.txt"), "requests\n").unwrap();
    std::fs::write(root.join("main.py"), "import requests\n").unwrap();
    // The stub `uv`: fails like an index that cannot be reached.
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(
        tools.join("uv"),
        "#!/bin/sh\necho \"Resolving $*\"\necho 'error: stub uv cannot reach the index' >&2\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(tools.join("uv"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(root.join("no-tools")).unwrap();
    root.canonicalize().unwrap()
}

/// The workbench with `main.py` open, pyright's mark on `requests` and the caret inside it.
fn open(
    root: &Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join("main.py");
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
        b.refresh_python(cx);
    });
    cx.run_until_parked();
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().expect("main.py is open");
        editor.update(cx, |e, cx| {
            e.set_diagnostics(
                &[ferris_idea_languages::lsp::Diagnostic {
                    severity: "warning".into(),
                    start_line: 0,
                    start_character: 7,
                    end_line: 0,
                    end_character: 15,
                    message: PYRIGHT.into(),
                    source: Some("Pyright".into()),
                    code: Some("reportMissingImports".into()),
                    unnecessary: false,
                }],
                cx,
            );
            e.go_to(0, 9, cx);
        });
    });
    cx.run_until_parked();
    (bench, cx)
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn go_to(bench: &Entity<WorkbenchView>, column: usize, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, _, cx| {
        let editor = b.active_editor_entity().unwrap();
        editor.update(cx, |e, cx| e.go_to(0, column, cx));
    });
    cx.run_until_parked();
}

fn failure(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.python_view()
            .and_then(|v| v.read(cx).failure().map(|f| f.texts()))
            .unwrap_or_default()
    })
}

#[gpui::test]
fn a_failed_install_opens_the_window_and_holds_the_status(cx: &mut TestAppContext) {
    let root = workspace("failed");
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, _, cx| {
        b.set_python_tool_path(Some(root.join("tools").into()), cx)
    });
    assert!(
        status(&bench, cx).contains(PYRIGHT),
        "the caret's problem is shown first: {}",
        status(&bench, cx)
    );
    assert!(
        bench.read_with(cx, |b, _| b.python_view().is_none()),
        "the Python Packages window is closed before the install"
    );

    // The install of the quick fix: the environment has no pip, so the stub uv runs and fails.
    bench.update_in(cx, |b, window, cx| {
        b.install_python(vec!["requests".into()], window, cx)
    });
    cx.run_until_parked();

    let python = root.join(".venv/bin/python");
    let texts = failure(&bench, cx);
    assert_eq!(texts[0], "Command failed (exit code 2)", "{texts:?}");
    assert_eq!(
        texts[1],
        format!(
            "$ {} pip install --python {} requests",
            root.join("tools/uv").display(),
            python.display()
        )
    );
    assert_eq!(
        texts.last().map(String::as_str),
        Some("error: stub uv cannot reach the index"),
        "{texts:?}"
    );
    assert!(
        bench.read_with(cx, |b, _| b.python_view().is_some()),
        "the window opened"
    );
    // The failure is in front of the caret's problem …
    assert_eq!(
        status(&bench, cx),
        "Failed: error: stub uv cannot reach the index"
    );
    assert_eq!(
        status(&bench, cx),
        "Failed: error: stub uv cannot reach the index",
        "and stays while nothing moves"
    );
    // … until the caret moves: then the problem under it is back,
    go_to(&bench, 10, cx);
    assert!(
        status(&bench, cx).contains(PYRIGHT),
        "{}",
        status(&bench, cx)
    );
    // also back where the failure was reported (the hold is over),
    go_to(&bench, 9, cx);
    assert!(
        status(&bench, cx).contains(PYRIGHT),
        "{}",
        status(&bench, cx)
    );
    // and off the problem the failure is the ordinary last note.
    go_to(&bench, 0, cx);
    assert_eq!(
        status(&bench, cx),
        "Failed: error: stub uv cannot reach the index"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn without_pip_and_uv_nothing_starts_and_the_remedies_are_named(cx: &mut TestAppContext) {
    let root = workspace("refused");
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, _, cx| {
        b.set_python_tool_path(Some(root.join("no-tools").into()), cx)
    });
    bench.update_in(cx, |b, window, cx| {
        b.install_python(vec!["requests".into()], window, cx)
    });
    cx.run_until_parked();

    let python = root.join(".venv/bin/python");
    let texts = failure(&bench, cx);
    assert_eq!(texts[0], "Not installed: requests", "{texts:?}");
    assert_eq!(texts.len(), 2, "no command ran: {texts:?}");
    assert!(
        texts[1].contains("has no pip and uv was not found"),
        "{texts:?}"
    );
    assert!(
        texts[1].contains(&format!("`{} -m ensurepip`", python.display())),
        "{texts:?}"
    );
    assert!(texts[1].contains("install uv"), "{texts:?}");
    let shown = status(&bench, cx);
    assert!(shown.starts_with("Not installed: "), "{shown}");
    assert!(
        bench.read_with(cx, |b, cx| b
            .python_view()
            .is_some_and(|v| v.read(cx).busy().is_none())),
        "the window is open and nothing runs"
    );
    let _ = std::fs::remove_dir_all(&root);
}
