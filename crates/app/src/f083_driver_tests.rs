//! The F-083 window steps that OBS-0116 and OBS-0118 left open because the comparison agents
//! could not operate the windows: the Containers tool window (start, stop, restart, shell, remove
//! twice, pull and remove an image) and the Remote Host tool window (Add Host… with its form, the
//! host list, connect, a path, New File, SSH Terminal, Hosts, Remove Host and its question). A
//! scripted MCP client over the window's own HTTP server drives them as an agent does
//! (`ui_command`, `ui_act` with real key and pointer events through the window's input handler),
//! so every step passes the driver's checks of declared effects and rights (`SPEC-0319`
//! behaviour 35, v7 behaviour 56). The Docker Engine is the fake engine on a Unix socket of the
//! test's own (`ferris_idea_containers::fake`), the SSH host the system `sshd` started as the current
//! user (`ferris_idea_remote::testing::TestHost`); the settings, the grant file and Ferris IDEA's host
//! file are the test's own, the user's are never read or written.
//!
//! What needs eyes stays in the hand tests: how the rows are drawn, the shell session inside the
//! terminal against a real engine, drag and drop (the driver answers `unsupported` for `drag`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_mcp::client::Client;
use gpui::TestAppContext;
use serde_json::{Value, json};

use crate::ui_driver_window_tests::{WINDOWS, Window, agent, call, command, project, pump, window};

/// One `ui_act` call.
fn act(w: &mut Window, client: Client, args: Value) -> (Client, Result<Value, String>) {
    call(w, client, "ui_act", args)
}

fn press(w: &mut Window, client: Client, keys: &str) -> (Client, Result<Value, String>) {
    act(w, client, json!({"action": "press", "keys": keys}))
}

fn click(w: &mut Window, client: Client, id: &str) -> (Client, Result<Value, String>) {
    act(w, client, json!({"action": "click", "locator": {"id": id}}))
}

fn effects(r: &Value) -> Vec<String> {
    r["effects"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// One call whose right the window asks the user for: the question is answered with `answer`,
/// as the user does; returns the question too.
fn act_asked(
    w: &mut Window,
    client: Client,
    args: Value,
    answer: &str,
) -> (Client, Result<Value, String>, Option<String>) {
    let mut client = client;
    let job = std::thread::spawn(move || {
        let r = client.call("ui_act", args);
        (client, r)
    });
    let mut asked = None;
    let end = Instant::now() + Duration::from_secs(60);
    while !job.is_finished() {
        assert!(Instant::now() < end, "ui_act answers");
        pump(w.cx);
        if let Some((q, _)) = w.cx.pending_prompt() {
            asked = Some(q);
            w.cx.simulate_prompt_answer(answer);
        }
    }
    let (client, r) = job.join().unwrap();
    (client, r, asked)
}

/// Waits up to `secs` of real time for `done`, with the window running.
fn wait(w: &mut Window, secs: u64, what: &str, mut done: impl FnMut(&mut Window) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        pump(w.cx);
        if done(w) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let bench = w.bench.clone();
    let state = w.cx.update(|_, cx| {
        let b = bench.read(cx);
        format!(
            "containers {:#?}\nstatus {:?}\nterminals {:?}",
            b.containers_view().map(|v| v.read(cx).texts()),
            b.status_message(cx),
            b.terminal_tabs()
        )
    });
    panic!("timed out: {what}\n{state}");
}

/// Writes the grant file of `root` with `rights` (the test's own settings folder).
fn write_rights(root: &Path, settings: &Path, rights: &[&str]) {
    let allow: Vec<String> = rights.iter().map(|r| format!("\"{r}\"")).collect();
    std::fs::write(
        settings.join("agent-permissions.toml"),
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [{}]\n",
            root.display(),
            allow.join(", ")
        ),
    )
    .unwrap();
}

fn container_texts(w: &mut Window) -> Vec<String> {
    let bench = w.bench.clone();
    w.cx.update(|_, cx| {
        bench
            .read(cx)
            .containers_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

fn container_selected(w: &mut Window) -> String {
    let bench = w.bench.clone();
    w.cx.update(|_, cx| {
        bench
            .read(cx)
            .containers_view()
            .and_then(|v| v.read(cx).selected_text())
            .unwrap_or_default()
    })
}

fn seen(engine: &ferris_idea_containers::fake::FakeEngine, line: &str) -> bool {
    engine.seen().iter().any(|l| l.starts_with(line))
}

/// Moves the selection to the last row containing `name` with ↓/↑ through the driver.
fn select_row(w: &mut Window, client: Client, name: &str) -> Client {
    let mut client = client;
    for _ in 0..60 {
        let texts = container_texts(w);
        let selected = container_selected(w);
        let want = texts
            .iter()
            .rposition(|t| t.contains(name))
            .unwrap_or_else(|| panic!("no row {name:?}: {texts:#?}"));
        let at = texts.iter().position(|t| *t == selected).unwrap_or(0);
        if at == want {
            return client;
        }
        let (c, r) = press(w, client, if want > at { "down" } else { "up" });
        r.unwrap();
        client = c;
    }
    panic!("the selection does not reach {name:?}");
}

/// OBS-0116 N7 and D3 in the window (steps 3a, 3b, 5): s, x, r, e, Delete twice and the image
/// pull and removal, each a real key through the UI driver on the focused Containers list; the
/// list declares `run` and `ports`, so without `run` nothing reaches the engine.
#[gpui::test]
fn containers_start_stop_shell_remove_and_pull_through_the_ui_driver(cx: &mut TestAppContext) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    // A short socket path: Unix socket paths take about 100 bytes.
    let dir = PathBuf::from(format!("/tmp/idc-f083drv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let engine = ferris_idea_containers::fake::FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = ferris_idea_containers::DockerHost::Unix(engine.socket.clone());
    let (root, settings) = project("f083c", true, &["ui"]);
    let mut w = window(&root, &settings, true, cx);
    w.bench.update(w.cx, |b, _| {
        b.set_container_sources(Some(host.clone()), None);
        b.set_container_engines(vec![ferris_idea_containers::Engine::from_host(
            "fake", host,
        )]);
        b.set_compose_sources(None, None);
    });
    let client = agent(&settings);
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "containers", "op": "focus"}),
    );
    let r = r.unwrap();
    assert_eq!(r["shown"], json!(true), "{r}");
    assert_eq!(r["focused"], json!(true), "{r}");
    wait(&mut w, 10, "the containers are listed", |w| {
        let t = container_texts(w);
        t.iter()
            .any(|l| l.trim_start().starts_with("○ db postgres:18"))
            && t.iter()
                .any(|l| l.trim_start().starts_with("● web nginx:1.27"))
    });

    // A click on the row selects it (effect none).
    let (client, r) = click(&mut w, client, "containers-row-container:db");
    let r = r.unwrap();
    assert_eq!(effects(&r), ["none"], "{r}");
    assert!(container_selected(&mut w).contains("db postgres:18"));

    // Without `run` the window asks the user; denied, `s` is refused and nothing reaches the
    // engine.
    let (client, r, asked) = act_asked(
        &mut w,
        client,
        json!({"action": "press", "keys": "s"}),
        "Deny",
    );
    assert!(
        asked
            .as_deref()
            .is_some_and(|q| q.contains("the right \"run\"")),
        "{asked:?}"
    );
    let e = r.unwrap_err();
    assert!(e.starts_with("not_granted") && e.contains("`run`"), "{e}");
    pump(w.cx);
    assert!(
        !seen(&engine, "POST /containers/db/start"),
        "{:?}",
        engine.seen()
    );
    drop(client);

    // With `run` and `ports` granted, as the list declares.
    write_rights(&root, &settings, &["ui", "run", "ports"]);
    let client = agent(&settings);

    // 3a: s starts db.
    let (client, r) = press(&mut w, client, "s");
    let r = r.unwrap();
    assert!(effects(&r).contains(&"run".to_owned()), "{r}");
    wait(&mut w, 10, "s starts db", |w| {
        seen(&engine, "POST /containers/db/start")
            && container_texts(w)
                .iter()
                .any(|l| l.trim_start().starts_with("● db postgres:18"))
    });

    // 3a: x stops web, r restarts it.
    let (client, r) = click(&mut w, client, "containers-row-container:web");
    r.unwrap();
    let (client, r) = press(&mut w, client, "x");
    r.unwrap();
    wait(&mut w, 10, "x stops web", |w| {
        seen(&engine, "POST /containers/web/stop")
            && container_texts(w)
                .iter()
                .any(|l| l.trim_start().starts_with("○ web nginx:1.27"))
    });
    let (client, r) = press(&mut w, client, "r");
    r.unwrap();
    wait(&mut w, 10, "r restarts web", |w| {
        seen(&engine, "POST /containers/web/restart")
            && container_texts(w)
                .iter()
                .any(|l| l.trim_start().starts_with("● web nginx:1.27"))
    });

    // 5: e opens a shell in the running container as a terminal tab.
    let (client, r) = press(&mut w, client, "e");
    r.unwrap();
    let bench = w.bench.clone();
    wait(&mut w, 10, "e opens Exec: web", |w| {
        w.cx.update(|_, cx| bench.read(cx).terminal_tabs())
            .iter()
            .any(|t| t == "Exec: web")
    });
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "containers", "op": "focus"}),
    );
    assert_eq!(r.unwrap()["focused"], json!(true));

    // 3b: the first Delete only asks; the second removes.
    let (client, r) = click(&mut w, client, "containers-row-container:db");
    r.unwrap();
    let (client, r) = press(&mut w, client, "x");
    r.unwrap();
    wait(&mut w, 10, "x stops db", |w| {
        container_texts(w)
            .iter()
            .any(|l| l.trim_start().starts_with("○ db postgres:18"))
    });
    let (client, r) = press(&mut w, client, "delete");
    r.unwrap();
    pump(w.cx);
    let removing = w.cx.update(|_, cx| {
        bench
            .read(cx)
            .containers_view()
            .and_then(|v| v.read(cx).removing().map(str::to_owned))
    });
    assert_eq!(removing.as_deref(), Some("db"));
    assert!(
        !seen(&engine, "DELETE /containers/db"),
        "{:?}",
        engine.seen()
    );
    let (client, r) = press(&mut w, client, "delete");
    r.unwrap();
    wait(&mut w, 10, "the second Delete removes db", |w| {
        seen(&engine, "DELETE /containers/db")
            && !container_texts(w).iter().any(|l| l.contains("db postgres"))
    });

    // D3 in the window: p on an image row opens the pull field; the reference is typed into it,
    // Enter pulls; Delete twice on the pulled image removes it.
    let client = select_row(&mut w, client, "nginx:1.27");
    let (client, r) = press(&mut w, client, "p");
    r.unwrap();
    let (client, r) = act(
        &mut w,
        client,
        json!({"action": "type", "locator": {"id": "containers-pull"}, "text": "busybox:1.36.1"}),
    );
    r.unwrap();
    let typed = w.cx.update(|_, cx| {
        bench
            .read(cx)
            .containers_view()
            .and_then(|v| v.read(cx).pull_input().map(str::to_owned))
    });
    assert_eq!(typed.as_deref(), Some("busybox:1.36.1"));
    let (client, r) = press(&mut w, client, "enter");
    r.unwrap();
    wait(&mut w, 10, "Enter pulls the image", |w| {
        seen(&engine, "POST /images/create?fromImage=busybox")
            && container_texts(w)
                .iter()
                .any(|l| l.contains("busybox:1.36.1"))
    });
    let client = select_row(&mut w, client, "busybox:1.36.1");
    let (client, r) = press(&mut w, client, "delete");
    r.unwrap();
    let (_client, r) = press(&mut w, client, "delete");
    r.unwrap();
    wait(&mut w, 10, "the second Delete removes the image", |w| {
        seen(&engine, "DELETE /images/busybox")
            && !container_texts(w)
                .iter()
                .any(|l| l.contains("busybox:1.36.1"))
    });

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

fn remote_texts(w: &mut Window) -> Vec<String> {
    let bench = w.bench.clone();
    w.cx.update(|_, cx| {
        bench
            .read(cx)
            .remote_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

fn remote_place(w: &mut Window) -> Option<String> {
    let bench = w.bench.clone();
    w.cx.update(|_, cx| {
        bench
            .read(cx)
            .remote_view()
            .and_then(|v| v.read(cx).place().map(|(_, p)| p.clone()))
    })
}

/// Waits for the Remote Host window; an error it shows ends the wait at once.
fn wait_remote(w: &mut Window, what: &str, mut done: impl FnMut(&mut Window) -> bool) {
    let end = Instant::now() + Duration::from_secs(300);
    while Instant::now() < end {
        pump(w.cx);
        if done(w) {
            return;
        }
        let bench = w.bench.clone();
        let failed = w.cx.update(|_, cx| {
            bench
                .read(cx)
                .remote_view()
                .and_then(|v| v.read(cx).error().map(str::to_owned))
        });
        if let Some(e) = failed {
            panic!(
                "{what}: the window reports {e:?}; rows {:?}",
                remote_texts(w)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out: {what}; rows {:?}", remote_texts(w));
}

/// The test id of the row containing `part`.
fn remote_row(w: &mut Window, part: &str) -> String {
    let texts = remote_texts(w);
    let i = texts
        .iter()
        .position(|t| t.contains(part))
        .unwrap_or_else(|| panic!("no row {part:?}: {texts:#?}"));
    format!("remote-row-{i}")
}

/// OBS-0118 N1–N8 in the window, through the UI driver: Add Host… opens the form, the five
/// fields are typed into, Add lists the host as Ferris IDEA's; a double click connects and shows the
/// session's home folder; a path typed into the field and Enter goes there; New File with a name
/// and Enter makes and opens the file on the host; SSH Terminal opens a terminal tab; Hosts goes
/// back; Remove Host asks in the window and Enter removes the host from Ferris IDEA's file. Every
/// control declares its effect: typing into the form needs `ssh`, and nothing is written without.
#[gpui::test]
fn remote_host_add_connect_files_terminal_and_remove_through_the_ui_driver(
    cx: &mut TestAppContext,
) {
    let _one = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let _turn = ferris_idea_remote::testing::one_host_at_a_time();
    ferris_idea_remote::testing::isolate_ssh();
    ferris_idea_remote::testing::stretch_limits(4);
    let base = std::env::temp_dir().join(format!("ferris-idea-f083drv-ssh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("session-home")).unwrap();
    let base = base.canonicalize().unwrap();
    let session_home = base.join("session-home");
    let Some(host) = ferris_idea_remote::testing::TestHost::start_with_env(
        &base.join("sshd"),
        &[("HOME", &session_home.display().to_string())],
    ) else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    host.wait_ready(Duration::from_secs(300))
        .expect("the test host lets a session in");
    let port: u16 = std::fs::read_to_string(host.dir.join("ssh_config"))
        .unwrap()
        .lines()
        .find_map(|l| l.trim().strip_prefix("Port "))
        .and_then(|p| p.trim().parse().ok())
        .unwrap();
    // The user's configuration of this test: the test's known hosts, no agent.
    let user_file = base.join("user_ssh_config");
    std::fs::write(
        &user_file,
        format!(
            "Host *\n  UserKnownHostsFile {}\n  IdentityAgent none\n",
            host.dir.join("known_hosts").display()
        ),
    )
    .unwrap();
    let files = ferris_idea_remote::HostFiles {
        own: base.join("own/ssh/config"),
        user: user_file,
    };
    ferris_idea_remote::set_host_files(Some(files.clone()));
    let folder = base.join("work");
    std::fs::create_dir_all(&folder).unwrap();

    let (root, settings) = project("f083r", true, &["ui"]);
    let mut w = window(&root, &settings, true, cx);
    w.bench.update(w.cx, |b, _| b.set_remote_ssh(Vec::new()));
    let client = agent(&settings);
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "remote", "op": "focus"}),
    );
    assert_eq!(r.unwrap()["shown"], json!(true));

    // Add Host… changes nothing outside the window; typing into the form needs `ssh`.
    let (client, r) = click(&mut w, client, "remote-add-host");
    assert_eq!(effects(&r.unwrap()), ["none"]);
    let (client, r, asked) = act_asked(
        &mut w,
        client,
        json!({"action": "type", "locator": {"id": "remote-add-host-field-0"}, "text": "own-drv"}),
        "Deny",
    );
    assert!(
        asked
            .as_deref()
            .is_some_and(|q| q.contains("the right \"ssh\"")),
        "{asked:?}"
    );
    let e = r.unwrap_err();
    assert!(e.starts_with("not_granted") && e.contains("`ssh`"), "{e}");
    drop(client);
    write_rights(&root, &settings, &["ui", "ssh", "run"]);
    let mut client = agent(&settings);

    let key = host.dir.join("client_key").display().to_string();
    let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
    let port = port.to_string();
    for (i, text) in [
        "own-drv",
        "127.0.0.1",
        port.as_str(),
        user.as_str(),
        key.as_str(),
    ]
    .into_iter()
    .enumerate()
    {
        let (c, r) = act(
            &mut w,
            client,
            json!({"action": "type", "locator": {"id": format!("remote-add-host-field-{i}")}, "text": text}),
        );
        client = c;
        r.unwrap();
    }
    let bench = w.bench.clone();
    let values = w.cx.update(|_, cx| {
        bench
            .read(cx)
            .remote_view()
            .map(|v| v.read(cx).add_host_values(cx))
            .unwrap_or_default()
    });
    assert_eq!(
        values,
        [
            "own-drv",
            "127.0.0.1",
            port.as_str(),
            user.as_str(),
            key.as_str()
        ]
    );
    let (client, r) = click(&mut w, client, "remote-add-host-ok");
    assert_eq!(effects(&r.unwrap()), ["ssh"]);
    wait_remote(&mut w, "the host is listed as Ferris IDEA's", |w| {
        remote_texts(w)
            .iter()
            .any(|t| t.starts_with("own-drv") && t.ends_with("· Ferris IDEA"))
    });
    assert!(
        std::fs::read_to_string(&files.own)
            .unwrap()
            .contains("Host own-drv\n")
    );

    // A double click on the row connects and shows the session's home folder.
    let row = remote_row(&mut w, "own-drv");
    let (client, r) = act(
        &mut w,
        client,
        json!({"action": "click", "count": 2, "locator": {"id": row}}),
    );
    assert_eq!(effects(&r.unwrap()), ["ssh"]);
    wait_remote(&mut w, "the home folder of the host is shown", |w| {
        remote_place(w).is_some_and(|p| Path::new(&p) == session_home)
    });

    // A path typed into the field, Enter goes there.
    let (client, r) = act(
        &mut w,
        client,
        json!({"action": "type", "locator": {"id": "remote-field"}, "text": folder.display().to_string(), "clear": true}),
    );
    r.unwrap();
    let (client, r) = act(
        &mut w,
        client,
        json!({"action": "press", "locator": {"id": "remote-field"}, "keys": "enter"}),
    );
    assert!(effects(&r.unwrap()).contains(&"ssh".to_owned()));
    wait_remote(&mut w, "the work folder is shown", |w| {
        remote_place(w).is_some_and(|p| Path::new(&p) == folder)
    });

    // New File, a name, Enter: made on the host and opened.
    let (client, r) = click(&mut w, client, "remote-new-file");
    assert_eq!(effects(&r.unwrap()), ["ssh"]);
    let (client, r) = act(
        &mut w,
        client,
        json!({"action": "type", "locator": {"id": "remote-field"}, "text": "neu.txt", "clear": true}),
    );
    r.unwrap();
    let (client, r) = act(
        &mut w,
        client,
        json!({"action": "press", "locator": {"id": "remote-field"}, "keys": "enter"}),
    );
    r.unwrap();
    wait_remote(&mut w, "neu.txt is made on the host and opens", |w| {
        let bench = w.bench.clone();
        folder.join("neu.txt").is_file()
            && w.cx.update(|_, cx| {
                bench
                    .read(cx)
                    .active_file()
                    .is_some_and(|p| p.ends_with("neu.txt"))
            })
    });

    // SSH Terminal opens a terminal tab (`ssh` and `run`).
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "remote", "op": "focus"}),
    );
    r.unwrap();
    let before = w.cx.update(|_, cx| bench.read(cx).terminal_tabs().len());
    let (client, r) = click(&mut w, client, "remote-terminal");
    assert_eq!(effects(&r.unwrap()), ["run", "ssh"]);
    wait(&mut w, 30, "a terminal tab opens", |w| {
        w.cx.update(|_, cx| bench.read(cx).terminal_tabs().len()) > before
    });

    // Hosts goes back to the list; Remove Host asks in the window, Enter removes.
    let (client, r) = command(
        &mut w,
        client,
        json!({"command": "tool_window", "id": "remote", "op": "focus"}),
    );
    r.unwrap();
    let (client, r) = click(&mut w, client, "remote-hosts");
    assert_eq!(effects(&r.unwrap()), ["none"]);
    wait_remote(&mut w, "back to the hosts", |w| remote_place(w).is_none());
    let row = remote_row(&mut w, "own-drv");
    let (client, r) = click(&mut w, client, &row);
    r.unwrap();
    let (client, r) = click(&mut w, client, "remote-remove-host");
    assert_eq!(effects(&r.unwrap()), ["ssh"]);
    let asked = w.cx.update(|_, cx| {
        bench.read(cx).remote_view().is_some_and(|v| {
            matches!(
                v.read(cx).pending(),
                Some(ferris_idea_workbench::RemotePending::RemoveHost(a)) if a == "own-drv"
            )
        })
    });
    assert!(asked, "Remove Host asks first");
    assert!(
        std::fs::read_to_string(&files.own)
            .unwrap()
            .contains("own-drv")
    );
    // Enter goes to the focused element of the window, which takes the window's effects.
    let (_client, r) = press(&mut w, client, "enter");
    assert!(effects(&r.unwrap()).contains(&"ssh".to_owned()));
    wait_remote(&mut w, "the host is gone from the list", |w| {
        !remote_texts(w).iter().any(|t| t.contains("own-drv"))
    });
    assert!(
        !std::fs::read_to_string(&files.own)
            .unwrap()
            .contains("own-drv")
    );

    ferris_idea_remote::set_host_files(None);
    drop(host);
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
