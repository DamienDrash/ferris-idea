//! `SPEC-0102` v6 AK-10, `SPEC-0190` v6 AK-10 and `SPEC-0096` v7 AK-11 (OBS-0061 H1, H2) in a
//! window of the test harness: the list under "Env: … ▾" lies wholly inside the window, its title
//! and entries one line apart; ⌘↩ on a request with an unresolved variable sends nothing and Output
//! names it in red. No server is needed: nothing is sent.

#![cfg(target_os = "macos")]

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{ServicesView, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

const WIDTH: f32 = 1372.;

/// A request file with two environments; `name` keeps temporary folders of the tests apart.
fn workspace(name: &str, envs: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-http-h1h3-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("api.http"),
        "### Missing\nGET http://127.0.0.1:9/{{nothing}}/x\n\n### List\nGET http://{{host}}/users\n",
    )
    .unwrap();
    std::fs::write(root.join("http-client.env.json"), envs).unwrap();
    root.canonicalize().unwrap()
}

fn open<'a>(
    root: &std::path::Path,
    cx: &'a mut TestAppContext,
) -> (Entity<WorkbenchView>, &'a mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    // Settings of its own: the user's settings and keymap are never read or written.
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
    cx.simulate_resize(gpui::size(px(WIDTH), px(860.)));
    let file = root.join("api.http");
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    (bench, cx)
}

/// The painted and the visible bounds of the node with test id `id`.
fn bounds(
    id: &str,
    cx: &mut VisualTestContext,
) -> Option<(semantics::Rect, Option<semantics::Rect>)> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let n = tree.iter().find(|n| n.test_id() == Some(id))?;
        Some((n.bounds(), n.visible_bounds))
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let (r, _) = bounds(id, cx).unwrap_or_else(|| panic!("no control {id} in view"));
    let at = point(px(r.x + r.w / 2.), px(r.y + r.h / 2.));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn services(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<ServicesView> {
    for _ in 0..200 {
        cx.run_until_parked();
        if let Some(s) = bench.read_with(cx, |b, _| b.services_view())
            && !s.read_with(cx, |s, _| s.running())
        {
            return s;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the Services window did not open");
}

/// SPEC-0102 v6 AK-10, SPEC-0190 v6 AK-10 (OBS-0061 H1).
#[gpui::test]
fn the_environment_list_stays_inside_the_window(cx: &mut TestAppContext) {
    let root = workspace(
        "list",
        "{\"local\": {\"host\": \"127.0.0.1:9\"}, \"staging\": {\"host\": \"127.0.0.1:9\"}}",
    );
    let (bench, cx) = open(&root, cx);
    // On `### List`: with two environments and none chosen nothing goes and the list is offered.
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("api.http is open");
    editor.update(cx, |e, cx| e.go_to_line(4, cx));
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-enter");
    let services = services(&bench, cx);
    click("services-environment", cx);
    assert!(services.read_with(cx, |s, _| s.environment_list_open()));
    let mut rows = Vec::new();
    for id in [
        "services-environment-title",
        "services-environment-local",
        "services-environment-staging",
    ] {
        let (painted, visible) = bounds(id, cx).unwrap_or_else(|| panic!("{id} is drawn"));
        assert!(
            painted.x >= 0. && painted.x + painted.w <= WIDTH,
            "{id} lies inside the window: {painted:?}"
        );
        assert!(
            visible.is_some_and(|v| v.same(&painted)),
            "{id} is not cut off: {painted:?} {visible:?}"
        );
        rows.push(painted);
    }
    for pair in rows.windows(2) {
        let gap = pair[1].y - pair[0].y;
        assert!(
            gap > 0. && gap <= pair[0].h + 2.,
            "one line apart, no empty line: {rows:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0096 v7 AK-11 (OBS-0061 H2).
#[gpui::test]
fn a_request_that_cannot_go_is_named_in_output(cx: &mut TestAppContext) {
    let root = workspace("unresolved", "{\"local\": {\"host\": \"127.0.0.1:9\"}}");
    let (bench, cx) = open(&root, cx);
    // The caret is on `### Missing`: `{{nothing}}` is no variable of `local`.
    cx.simulate_keystrokes("cmd-enter");
    let services = services(&bench, cx);
    let output = services.read_with(cx, |s, _| s.output_entries().to_vec());
    assert!(
        output.iter().any(|(l, red)| *red
            && l == "GET http://127.0.0.1:9/{{nothing}}/x → Unresolved variables: nothing"),
        "{output:?}"
    );
    assert_eq!(services.read_with(cx, |s, _| s.selected_tab()), "Output");
    let status = bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned());
    assert!(
        status.starts_with("Unresolved variables: nothing"),
        "{status}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
