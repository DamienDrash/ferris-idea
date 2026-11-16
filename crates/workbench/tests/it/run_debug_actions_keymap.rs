//! The run, debug and window actions taken from the keymap, carried out in a window of the test
//! harness: Toggle Temporary Line Breakpoint (`SPEC-0127` v8 behaviour 18), the chooser opened
//! for debugging (`SPEC-0087` v8 behaviour 16) and the next and previous project window
//! (`SPEC-0094` v2 behaviour 5).

#![cfg(target_os = "macos")]

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::project_windows::{RecentProjectsFile, register_window};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-rda-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    std::fs::write(
        root.join(".idecraft/run.toml"),
        "[[run]]\nname = \"serve\"\ncommand = [\"echo\", \"1\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("main.js"), "let a = 1;\nlet b = 2;\nlet c = 3;\n").unwrap();
    // The directory as the workbench names it, so paths compare equal.
    root.canonicalize().unwrap()
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join("main.js");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

/// SPEC-0127 AK-13 (v8): the action sets a breakpoint with "Remove once hit" on the caret line,
/// saved with the workspace, and takes it away again; on a plain breakpoint it removes that one.
#[gpui::test]
fn the_temporary_breakpoint_is_set_saved_and_removed(cx: &mut TestAppContext) {
    let root = workspace("temporary");
    let (bench, cx) = open(&root, cx);
    let lines = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.active_editor_entity()
                .expect("an editor is open")
                .read(cx)
                .breakpoints()
                .to_vec()
        })
    };
    let saved = || std::fs::read_to_string(root.join(".idecraft/breakpoints.toml")).ok();
    let act = |id: &str, cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
        cx.run_until_parked();
    };
    assert!(lines(cx).is_empty());

    act("ToggleTemporaryLineBreakpoint", cx);
    assert_eq!(lines(cx), [0], "a breakpoint on the caret line");
    let text = saved().expect("the breakpoint is saved");
    assert!(text.contains("remove_once_hit = true"), "{text}");

    act("ToggleTemporaryLineBreakpoint", cx);
    assert!(lines(cx).is_empty(), "the second toggle removes it");
    assert!(
        !saved().unwrap_or_default().contains("remove_once_hit"),
        "its property goes with it"
    );

    // A plain breakpoint is removed too, and no property is left behind.
    act("ToggleLineBreakpoint", cx);
    assert_eq!(lines(cx), [0]);
    assert!(!saved().unwrap_or_default().contains("remove_once_hit"));
    act("ToggleTemporaryLineBreakpoint", cx);
    assert!(lines(cx).is_empty());
    assert!(!saved().unwrap_or_default().contains("remove_once_hit"));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0087 AK-10 (v8): Debug… opens the chooser for debugging, Run… the chooser as it was;
/// both list the same rows and Escape closes them.
#[gpui::test]
fn debug_opens_the_chooser_for_debugging(cx: &mut TestAppContext) {
    let root = workspace("chooser");
    let (bench, cx) = open(&root, cx);
    let state = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.run_chooser().map(|c| {
                let c = c.read(cx);
                let labels: Vec<String> = c.rows().iter().map(|r| r.label().to_owned()).collect();
                (c.debugs_first(), labels)
            })
        })
    };
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ChooseDebugConfiguration", window, cx)
    });
    cx.run_until_parked();
    let (debugging, rows) = state(cx).expect("the chooser is open");
    assert!(debugging, "opened for debugging");
    assert!(rows.contains(&"serve".to_owned()), "{rows:?}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(state(cx).is_none(), "Escape closed the chooser");

    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ChooseRunConfiguration", window, cx)
    });
    cx.run_until_parked();
    assert_eq!(state(cx), Some((false, rows)));
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0004 v6 AK-11: the configuration chooser takes the text the platform's text input
/// commits, not the key behind it — key events of the key `a` committing `s` and `e` type `se`;
/// real keys go on with it, ⌫ and Escape keep their meaning.
#[gpui::test]
fn committed_text_filters_the_chooser(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let root = workspace("committed");
    let (bench, cx) = open(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ChooseRunConfiguration", window, cx)
    });
    cx.run_until_parked();
    let chooser = bench
        .read_with(cx, |b, _| b.run_chooser())
        .expect("the chooser is open");
    let filter = |cx: &mut VisualTestContext| chooser.read_with(cx, |c, _| c.filter().to_owned());
    for committed in ["s", "e"] {
        let event = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke {
                modifiers: gpui::Modifiers::none(),
                key: "a".into(),
                key_char: Some("a".into()),
            },
            is_held: false,
        };
        cx.simulate_event(event);
        chooser.update_in(cx, |c, window, cx| {
            c.replace_text_in_range(None, committed, window, cx)
        });
        cx.run_until_parked();
    }
    assert_eq!(filter(cx), "se");
    cx.simulate_input("rV");
    assert_eq!(filter(cx), "serV");
    let rows: Vec<String> = chooser.read_with(cx, |c, _| {
        c.rows().iter().map(|r| r.label().to_owned()).collect()
    });
    assert!(rows.contains(&"serve".to_owned()), "{rows:?}");
    cx.simulate_keystrokes("backspace");
    assert_eq!(filter(cx), "ser");
    cx.simulate_keystrokes("escape");
    assert_eq!(filter(cx), "");
    assert!(bench.read_with(cx, |b, _| b.run_chooser()).is_some());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0094 AK-4 (v2): with two project windows each action brings the other one to the
/// front; a third one makes Next and Previous differ and both wrap.
#[gpui::test]
fn next_and_previous_project_window_step_through_the_windows(cx: &mut TestAppContext) {
    let recent = std::env::temp_dir().join(format!("idecraft-rda-recent-{}", std::process::id()));
    cx.update(|cx| cx.set_global(RecentProjectsFile(recent.clone())));
    let roots = [workspace("w1"), workspace("w2"), workspace("w3")];
    let mut benches = Vec::new();
    let mut handles = Vec::new();
    for root in &roots {
        let shown = root.clone();
        let window = cx.add_window(move |_, cx| WorkbenchView::new(&shown, cx));
        cx.update(|cx| register_window(root, window.into(), cx));
        benches.push(window.root(cx).unwrap());
        handles.push(gpui::AnyWindowHandle::from(window));
    }
    cx.run_until_parked();
    let step = |from: usize, id: &str, cx: &mut TestAppContext| {
        let bench = benches[from].clone();
        handles[from]
            .update(cx, |_, window, cx| {
                bench.update(cx, |b, cx| b.menu_action(id, window, cx))
            })
            .unwrap();
        cx.run_until_parked();
        let active = cx
            .update(|cx| cx.active_window())
            .expect("a window is in front");
        handles.iter().position(|h| *h == active).unwrap()
    };
    assert_eq!(step(0, "NextProjectWindow", cx), 1);
    assert_eq!(step(1, "NextProjectWindow", cx), 2);
    assert_eq!(
        step(2, "NextProjectWindow", cx),
        0,
        "after the last, the first"
    );
    assert_eq!(
        step(0, "PreviousProjectWindow", cx),
        2,
        "before the first, the last"
    );
    assert_eq!(step(2, "PreviousProjectWindow", cx), 1);
    for root in &roots {
        let _ = std::fs::remove_dir_all(root);
    }
    let _ = std::fs::remove_file(&recent);
}
