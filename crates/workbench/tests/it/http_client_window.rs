//! `SPEC-0096` v6 AK-8 and `SPEC-0102` v5 AK-7 in a window of the test harness, against a local
//! server, by keys and by clicks on controls found through the semantic tree (`SPEC-0319`): with
//! two environments and none chosen ⌘↩ sends nothing and names them; the list under "Env: … ▾"
//! chooses one per request file; ⌘↩ sends the request at the caret with it and the Services window
//! shows the response; the run mark sends its request; Run All Requests in File from the editor's
//! menu sends every request that can go in file order, a placeholder with spaces whole, names the
//! ones with unresolved names and shows the last response; a URL without a scheme goes over http.

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{MenuPlace, ServicesView, WorkbenchView, semantics};

/// A local server answering every request with JSON naming its method and path; it records the
/// request lines it saw.
fn server() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut length = 0;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = header.split_once(':')
                    && k.eq_ignore_ascii_case("content-length")
                {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let request = line.trim().to_owned();
            let mut parts = request.split_whitespace();
            let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            log.lock().unwrap().push(format!("{method} {path}"));
            let answer = format!("{{\"method\":\"{method}\",\"path\":\"{path}\"}}");
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (port, seen)
}

fn workspace(port: u16) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-http-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("api.http"),
        "### List\nGET {{host}}{{base}}/users\nAccept: application/json\n\n### Create\nPOST http://{{host}}{{base}}/users\nContent-Type: application/json\n\n{\"name\": \"a\"}\n\n### Missing\nGET http://{{nothing}}/x\n\n### Random\nGET http://{{host}}{{base}}/random?n={{$random.integer(1, 5)}}\n\n### Unknown\nGET http://{{host}}{{base}}/unknown?x={{$foo bar}}\n",
    )
    .unwrap();
    // A second request file beside it, with the same environments and a choice of its own.
    std::fs::write(
        root.join("other.http"),
        "### Other\nGET http://{{host}}{{base}}/other\n",
    )
    .unwrap();
    std::fs::write(
        root.join("http-client.env.json"),
        format!(
            "{{\"local\": {{\"host\": \"127.0.0.1:{port}\", \"base\": \"/v1\"}}, \"staging\": {{\"host\": \"127.0.0.1:{port}\", \"base\": \"/v2\"}}}}"
        ),
    )
    .unwrap();
    root.canonicalize().unwrap()
}

fn node_centre(id: &str, cx: &mut VisualTestContext) -> Option<gpui::Point<gpui::Pixels>> {
    cx.run_until_parked();
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let node = tree.iter().find(|n| n.test_id() == Some(id))?;
        let r = node.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
        Some(point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)))
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let at = node_centre(id, cx).unwrap_or_else(|| panic!("no control {id} in view"));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

/// Waits until the Services window has finished its run.
fn finished(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> Entity<ServicesView> {
    for _ in 0..200 {
        cx.run_until_parked();
        if let Some(s) = bench.read_with(cx, |b, _| b.services_view())
            && !s.read_with(cx, |s, _| s.running())
        {
            return s;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the request did not finish");
}

fn seen(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().unwrap().clone()
}

/// Chooses environment `name` from the list under "Env: … ▾", by clicks on the controls.
fn choose(name: &str, cx: &mut VisualTestContext) {
    click("services-environment", cx);
    click(&format!("services-environment-{name}"), cx);
}

/// AK-8 (SPEC-0096 v5, v6), AK-7 (SPEC-0102 v4, v5) and the window part of SPEC-0102 v5 AK-8.
#[gpui::test]
fn requests_go_from_keys_marks_and_run_all(cx: &mut TestAppContext) {
    let (port, log) = server();
    let root = workspace(port);
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    // Settings of its own: the user's settings and keymap are never read or written.
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    let file = root.join("api.http");
    let open = |path: &std::path::Path, cx: &mut VisualTestContext| {
        bench.update_in(cx, |b, window, cx| {
            b.open(path, window, cx);
            b.focus_editor(window, cx);
        });
        cx.run_until_parked();
    };
    open(&file, cx);

    // ⌘↩ with two environments and none chosen: nothing is sent, Output names both, and
    // "Env: none ▾" offers them (SPEC-0102 v5 behaviour 2).
    cx.simulate_keystrokes("cmd-enter");
    let services = finished(&bench, cx);
    assert!(seen(&log).is_empty(), "{:?}", seen(&log));
    let output = services.read_with(cx, |s, _| s.output_lines());
    assert!(
        output
            .iter()
            .any(|l| l == "Not sent: choose an HTTP environment: local, staging"),
        "{output:?}"
    );
    assert_eq!(services.read_with(cx, |s, _| s.selected_tab()), "Output");
    assert_eq!(
        services.read_with(cx, |s, _| s.environment().map(str::to_owned)),
        None
    );

    // The list chooses `local`; choosing sends nothing.
    click("services-environment", cx);
    assert!(services.read_with(cx, |s, _| s.environment_list_open()));
    click("services-environment-local", cx);
    assert!(!services.read_with(cx, |s, _| s.environment_list_open()));
    assert_eq!(
        services.read_with(cx, |s, _| s.environment().map(str::to_owned)),
        Some("local".to_owned())
    );
    assert!(seen(&log).is_empty());

    // ⌘↩ on the first request: `local`, a URL without a scheme over http, the Response tab with
    // the indented body.
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.simulate_keystrokes("cmd-enter");
    let services = finished(&bench, cx);
    assert_eq!(seen(&log), ["GET /v1/users"]);
    let response = services.read_with(cx, |s, _| s.response().map(str::to_owned));
    let response = response.expect("a response");
    assert!(response.starts_with("HTTP 200 OK\n"), "{response}");
    assert!(
        response.contains("\"path\": \"/v1/users\""),
        "indented: {response}"
    );
    assert_eq!(services.read_with(cx, |s, _| s.selected_tab()), "Response");
    let output = services.read_with(cx, |s, _| s.output_lines());
    assert!(
        output
            .iter()
            .any(|l| l.starts_with(&format!("GET http://127.0.0.1:{port}/v1/users → 200 OK"))),
        "{output:?}"
    );
    assert_eq!(
        services.read_with(cx, |s, _| s.environment().map(str::to_owned)),
        Some("local".to_owned())
    );

    // The choice belongs to `api.http`: another request file asks again and sends nothing.
    open(&root.join("other.http"), cx);
    cx.simulate_keystrokes("cmd-enter");
    let services = finished(&bench, cx);
    assert_eq!(seen(&log), ["GET /v1/users"]);
    assert_eq!(
        services.read_with(cx, |s, _| s.environment().map(str::to_owned)),
        None
    );

    // Back in `api.http` its choice still holds; then the list switches it to `staging`.
    open(&file, cx);
    cx.simulate_keystrokes("cmd-enter");
    let services = finished(&bench, cx);
    assert_eq!(
        seen(&log),
        ["GET /v1/users", "GET /v1/users"],
        "still local"
    );
    choose("staging", cx);
    assert_eq!(
        services.read_with(cx, |s, _| s.environment().map(str::to_owned)),
        Some("staging".to_owned())
    );

    // The run mark of the second request (0-based line 5) sends it, wherever the caret is.
    bench.update_in(cx, |b, window, cx| b.run_marker_menu(&file, 5, window, cx));
    let services = finished(&bench, cx);
    assert_eq!(
        seen(&log),
        ["GET /v1/users", "GET /v1/users", "POST /v2/users"]
    );
    let response = services.read_with(cx, |s, _| s.response().map(str::to_owned));
    assert!(response.unwrap().contains("\"method\": \"POST\""));

    // Run All Requests in File from the editor's menu (SPEC-0096 v6 behaviour 8): those that can
    // go, in file order, the placeholder with spaces whole; the unresolved ones are named in
    // Output and never sent.
    bench.update(cx, |b, cx| {
        b.open_context_menu(
            MenuPlace::Editor,
            b.active_path(),
            point(px(300.), px(200.)),
            cx,
        )
    });
    cx.run_until_parked();
    let chosen = bench.update_in(cx, |b, window, cx| {
        b.choose_context_item("Run All Requests in File", window, cx)
    });
    assert!(chosen, "the editor's menu of a request file has Run All");
    let services = finished(&bench, cx);
    let sent = seen(&log);
    assert_eq!(sent.len(), 6, "{sent:?}");
    assert_eq!(sent[3..5], ["GET /v2/users", "POST /v2/users"]);
    let n: i64 = sent[5]
        .strip_prefix("GET /v2/random?n=")
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{sent:?}"));
    assert!((1..5).contains(&n), "{sent:?}");
    let output = services.read_with(cx, |s, _| s.output_lines());
    assert!(
        output
            .iter()
            .any(|l| l.contains("{{nothing}}") && l.contains("Unresolved variables: nothing")),
        "{output:?}"
    );
    assert!(
        output.iter().any(|l| l.contains("/unknown?x={{$foo bar}}")
            && l.contains("Unresolved variables: $foo bar")),
        "{output:?}"
    );
    let response = services.read_with(cx, |s, _| s.response().map(str::to_owned));
    assert!(
        response.unwrap().contains("\"path\": \"/v2/random?n="),
        "the last response"
    );
    assert_eq!(services.read_with(cx, |s, _| s.selected_tab()), "Response");
    let _ = std::fs::remove_dir_all(&root);
}
