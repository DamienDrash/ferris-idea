//! The Agents tab of the AI Chat tool window in the window (`SPEC-0357` AK-1 to AK-4, OBS-0121
//! gaps A7–A11): a fake ACP agent (a shell script, no model, no network, no credentials) started,
//! prompted, approved and refused through cards with their diff, a turn's changes undone, ACP
//! programs found and added — with real clicks, keys and typing.

#![cfg(target_os = "macos")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::AiTab;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::semantics::{self, Effect};
use ferris_idea_workbench::{AgentsView, WorkbenchView};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

type Bench = Entity<WorkbenchView>;

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../core/tests/fixtures/fake-acp-agent.sh"
);

/// A workspace, a settings folder beside it (outside it, as `agent-host.toml` must be) and a
/// folder with fake `gemini` and `agy` programs.
fn folders() -> (PathBuf, PathBuf, PathBuf) {
    let base =
        std::env::temp_dir().join(format!("ferris-idea-agents-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (root, settings, bin) = (base.join("ws"), base.join("settings"), base.join("bin"));
    for d in [&root, &settings, &bin] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(root.join("notes.txt"), "hello\n").unwrap();
    std::fs::write(
        settings.join("agent-host.toml"),
        format!("[[agent]]\nid = \"fake\"\nname = \"Fake agent\"\ncommand = \"/bin/sh\"\nargs = [\"{FAKE}\"]\n"),
    )
    .unwrap();
    for (name, mode) in [("gemini", 0o755), ("agy", 0o755), ("codex-acp", 0o644)] {
        std::fs::write(bin.join(name), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(mode)).unwrap();
    }
    (
        root.canonicalize().unwrap(),
        settings.canonicalize().unwrap(),
        bin.canonicalize().unwrap(),
    )
}

fn window(
    root: &Path,
    settings: &Path,
    cx: &mut TestAppContext,
) -> (Bench, &'static mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(900.)));
    cx.run_until_parked();
    (bench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
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
        cx.executor().advance_clock(Duration::from_millis(300));
    }
    panic!("timed out: {what}");
}

/// The node with test id `name` in the semantic tree of the frame just drawn.
fn node(cx: &mut VisualTestContext, name: &str) -> Option<ferris_idea_uidriver::tree::UiNode> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        tree.iter()
            .filter(|n| n.test_id() == Some(name))
            .find(|n| n.visible_bounds.is_some_and(|r| r.w > 0. && r.h > 0.))
            .cloned()
    })
}

fn click(cx: &mut VisualTestContext, name: &str) {
    let n = node(cx, name).unwrap_or_else(|| panic!("{name} is drawn and in view"));
    let r = n.visible_bounds.unwrap();
    cx.simulate_click(
        point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)),
        Modifiers::none(),
    );
    draw(cx);
}

fn agents(bench: &Bench, cx: &mut VisualTestContext) -> Entity<AgentsView> {
    bench.read_with(cx, |b, cx| {
        b.ai_chat_view()
            .and_then(|v| v.read(cx).agents_view())
            .expect("the Agents tab is made")
    })
}

fn prompt(cx: &mut VisualTestContext, text: &str) {
    click(cx, "agents-prompt");
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn texts(bench: &Bench, cx: &mut VisualTestContext) -> String {
    agents(bench, cx).read_with(cx, |v, _| v.texts().join("\n"))
}

fn state(bench: &Bench, cx: &mut VisualTestContext) -> String {
    agents(bench, cx).read_with(cx, |v, _| {
        v.rows().last().map(|r| r.1.clone()).unwrap_or_default()
    })
}

#[gpui::test]
fn agents_are_found_started_prompted_approved_refused_and_undone(cx: &mut TestAppContext) {
    let (root, settings, bin) = folders();
    let (bench, cx) = window(&root, &settings, cx);
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("FerrisIDEA.AiChat", window, cx)
    });
    draw(cx);
    click(cx, "ai-tab-agents");
    let chat = bench.read_with(cx, |b, _| b.ai_chat_view()).unwrap();
    assert_eq!(chat.read_with(cx, |v, _| v.tab()), AiTab::Agents);
    agents(&bench, cx).update(cx, |v, cx| v.with_folders(vec![bin.clone()], cx));
    draw(cx);

    // AK-4: installed ACP programs are offered; Add writes the configuration (reserved for the
    // person: the button carries the effect the UI driver refuses).
    let found = agents(&bench, cx).read_with(cx, |v, _| v.found());
    assert_eq!(
        found,
        vec![
            ("Gemini CLI".to_owned(), false),
            ("Antigravity".to_owned(), false)
        ]
    );
    let add = node(cx, "agents-add-gemini").expect("Add is drawn");
    assert_eq!(add.effects, Some(vec![Effect::Trust]));
    click(cx, "agents-add-gemini");
    let file = std::fs::read_to_string(settings.join("agent-host.toml")).unwrap();
    assert!(
        file.contains("id = \"gemini\"") && file.contains("--acp"),
        "{file}"
    );
    assert!(
        file.contains(&bin.join("gemini").display().to_string()),
        "{file}"
    );
    let mode = std::fs::metadata(settings.join("agent-host.toml"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(
        node(cx, "agents-start-gemini").is_some(),
        "the added agent can be started"
    );
    assert!(node(cx, "agents-add-gemini").is_none());

    // AK-1: Start, prompt, answer, Idle.
    click(cx, "agents-start-fake");
    until(cx, 10, "the agent needs input", |cx| {
        state(&bench, cx) == "Needs input"
    });
    prompt(cx, "hello");
    until(cx, 10, "the answer arrives", |cx| {
        texts(&bench, cx).contains("message: echo: hello")
    });
    until(cx, 10, "the agent is idle", |cx| {
        state(&bench, cx) == "Idle"
    });
    assert!(texts(&bench, cx).contains("prompt: hello"));

    // AK-2: a card with the diff, no Always allow; the tab counts the agent; Reject, then the
    // repeated request carries the mark.
    prompt(cx, "edit-twice");
    until(cx, 10, "a card is shown", |cx| {
        node(cx, "agents-card-1").is_some()
    });
    let card = node(cx, "agents-card-1").unwrap();
    assert!(card.name().unwrap().contains("Edit notes.txt"), "{card:?}");
    let allow = node(cx, "agents-allow-1-allow").expect("Allow once is drawn");
    assert_eq!(
        allow.effects,
        Some(vec![Effect::Trust]),
        "approving is reserved for the person"
    );
    let reject = node(cx, "agents-reject-1-reject").expect("Reject is drawn");
    assert_eq!(reject.effects, Some(vec![Effect::None]));
    let tab = node(cx, "ai-tab-agents").unwrap();
    assert_eq!(tab.name(), Some("Agents (1)"));
    let shown = cx.update(|window, _| {
        semantics::tree(semantics::window_id(window))
            .unwrap()
            .iter()
            .filter_map(|n| n.name().map(str::to_owned))
            .collect::<Vec<_>>()
            .join("\n")
    });
    assert!(
        !shown.to_lowercase().contains("always"),
        "no standing approval is offered"
    );
    click(cx, "agents-reject-1-reject");
    until(cx, 10, "the repeated request is shown", |cx| {
        node(cx, "agents-card-2").is_some()
    });
    let marked = agents(&bench, cx).read_with(cx, |v, _| v.texts().join("\n"));
    assert!(
        marked.contains("you refused a similar request in this turn (request 1)"),
        "{marked}"
    );
    click(cx, "agents-reject-2-reject");
    until(cx, 10, "the refused turn ends", |cx| {
        state(&bench, cx) == "Idle"
    });
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "hello\n"
    );

    // Allow once, pressed by the person in the window.
    prompt(cx, "needs-approval");
    until(cx, 10, "the approval card", |cx| {
        node(cx, "agents-card-3").is_some()
    });
    click(cx, "agents-allow-3-allow");
    until(cx, 10, "the approved turn ends", |cx| {
        texts(&bench, cx).contains("message: approved")
    });

    // AK-3: the turn's changes and Undo.
    std::fs::remove_file(root.join("notes.txt")).unwrap();
    prompt(cx, "write-files");
    until(cx, 10, "the changes block", |cx| {
        node(cx, "agents-changes").is_some()
    });
    let block = node(cx, "agents-changes").unwrap();
    assert_eq!(block.name(), Some("Turn 4 changed 2 file(s)"));
    assert!(root.join("Built.class").exists());
    click(cx, "agents-undo");
    assert!(!root.join("Built.class").exists());
    assert!(!root.join("notes.txt").exists());
    let note = agents(&bench, cx).read_with(cx, |v, _| v.note()).unwrap();
    assert_eq!(note, "Turn 4 undone: 2 file(s) put back");

    // Stop ends it Done.
    let id = agents(&bench, cx).read_with(cx, |v, _| v.rows()[0].0.clone());
    click(cx, &format!("agents-stop-{id}"));
    until(cx, 10, "the agent is done", |cx| {
        state(&bench, cx) == "Done"
    });
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
