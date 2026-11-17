//! Re-checks without a screen of the hand tests that held F-083 (Profiler, Docker, Kubernetes,
//! Remote) at stage 2 in PARITY.md: the steps of hand tests 46, 62, 170–173, 180, 181, 182 (its
//! Gradle part) and 183 in a window of the test harness with real keys against the real programs —
//! a Docker Engine and a k3s cluster in the reference environment's Lima VMs
//! (started by its container script), the Compose CLI, JDK Flight Recorder, Gradle,
//! tracemalloc, macOS `leaks` and `sample`, and the system `sshd` started as the current user.
//! Nothing is a stand-in for the program it tests; the sources the window talks to are set with
//! the window's test calls instead of the user's settings, and choosing a run configuration or an
//! SSH host from the user's lists is a call too. Hand tests 84 and 184 need the `idecraft` program
//! as the host's backend and live with its tests (`crates/app/tests/it/f083_handtests.rs`).
//!
//! What needs eyes stays in the hand tests: how the rows and the flame graph are drawn, a real
//! keyboard, the browser a port opens, and the comparison with the reference. Each test is skipped
//! when its programs are missing; the Docker and Kubernetes test needs the variables
//! `containers.sh env` prints. Steps on the test host wait for the window's own answer instead of
//! a fixed time (`until_host`).
//!
//! `SPEC-0119` v4 AK-7, `SPEC-0240`–`SPEC-0243` (hand tests 46, 170–173), `SPEC-0136` v2 AK-6 (62),
//! `SPEC-0250` v3 AK-6 (180), `SPEC-0251` v3 AK-6 (181), `SPEC-0252` v2 AK-6 (182), `SPEC-0253` v2
//! AK-6 (183).

#![cfg(target_os = "macos")]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use idecraft_workbench::{ProfilerTab, WorkbenchView};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-f083-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

/// Waits up to `secs` seconds of real time until `done` holds.
fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

/// [`until`], and on a timeout the state `show` describes.
fn until_shown(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
    show: impl Fn(&mut VisualTestContext) -> String,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    let state = show(cx);
    panic!("timed out: {what}\n{state}");
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(program))
            .find(|f| f.is_file())
    })
}

/// The body of `GET /` on 127.0.0.1:`port`, retried for up to `wait`.
fn http_get(port: u16, wait: Duration) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n");
            let mut out = String::new();
            if s.read_to_string(&mut out).is_ok() && out.starts_with("HTTP/1.") {
                return Some(out);
            }
        }
        if started.elapsed() > wait {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

// ------------------------------------------------------------ hand tests 46, 170, 171, 172, 173

const COMPOSE: &str = "services:\n  web:\n    image: nginx:1.29-alpine\n    ports:\n      - \"127.0.0.1:18083:80\"\n  db:\n    image: busybox:1.37\n    command: [\"sh\", \"-c\", \"echo db up; sleep 3600\"]\n";

const MANIFEST: &str = "apiVersion: apps/v1\nkind: Deployment\nmetadata: {name: ht-web, namespace: default}\nspec:\n  replicas: 1\n  selector: {matchLabels: {app: ht-web}}\n  template:\n    metadata: {labels: {app: ht-web}}\n    spec:\n      containers:\n        - name: nginx\n          image: nginx:1.29-alpine\n          ports: [{containerPort: 80}]\n---\napiVersion: v1\nkind: Service\nmetadata: {name: ht-web, namespace: default}\nspec:\n  selector: {app: ht-web}\n  ports: [{port: 80, targetPort: 80}]\n";

fn container_texts(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.containers_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

fn has_row(bench: &Bench, cx: &mut VisualTestContext, f: impl Fn(&str) -> bool) -> bool {
    container_texts(bench, cx).iter().any(|l| f(l))
}

/// Moves the Containers window's selection with ↑/↓ to the first row containing `part`, with
/// the focus in that window.
fn go(bench: &Bench, cx: &mut VisualTestContext, part: &str) {
    let view = bench
        .read_with(cx, |b, _| b.containers_view())
        .expect("the Containers window is shown");
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    cx.run_until_parked();
    for _ in 0..200 {
        let (texts, selected) = view.read_with(cx, |v, _| (v.texts(), v.selected_text()));
        if selected.as_deref().is_some_and(|s| s.contains(part)) {
            return;
        }
        let want = texts
            .iter()
            .position(|t| t.contains(part))
            .unwrap_or_else(|| panic!("no row with {part:?}: {texts:#?}"));
        let at = selected
            .and_then(|s| texts.iter().position(|t| *t == s))
            .unwrap_or(0);
        keys(cx, if want > at { "down" } else { "up" });
    }
    panic!("the selection does not reach {part:?}");
}

fn kubectl_with(config: &str, dir: &Path) -> Option<idecraft_containers::Kubectl> {
    let real = on_path("kubectl")?;
    let path = dir.join("kubectl");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nKUBECONFIG='{config}' exec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .ok()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(idecraft_containers::Kubectl { program: path })
}

fn kube(k: &idecraft_containers::Kubectl, args: &[&str], input: Option<&str>) -> String {
    let mut child = Command::new(&k.program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(i) = input {
        child.stdin.take().unwrap().write_all(i.as_bytes()).unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

/// Hand tests 46, 170–173 (`SPEC-0119` v4 AK-7, `SPEC-0240`–`SPEC-0243`) against the real engine
/// and cluster: Compose up with `u`, the published port with `o` answering, `x`/`s`/Enter on a
/// container, a shell with `e` that runs what is typed, a service's logs, `r` on a deployment,
/// Enter on its pod, `f` forwarding the service's port that then answers, `o` and `x` on the
/// forward, `n`/`c`, the same engine over SSH with `e` on the Docker heading, Delete twice and
/// Compose down with `d`.
#[gpui::test]
fn hand_tests_46_and_170_to_173_containers_with_a_real_engine_and_cluster(cx: &mut TestAppContext) {
    let (Ok(address), Ok(config)) = (
        std::env::var("IDECRAFT_TEST_DOCKER"),
        std::env::var("IDECRAFT_TEST_KUBECONFIG"),
    ) else {
        eprintln!("skipped: IDECRAFT_TEST_DOCKER and IDECRAFT_TEST_KUBECONFIG are not set");
        return;
    };
    let Some(host) =
        idecraft_containers::DockerHost::parse(&address, Vec::new()).filter(|h| h.info().is_ok())
    else {
        eprintln!("skipped: {address} does not answer");
        return;
    };
    let Some(cli) =
        idecraft_containers::compose::ComposeCli::find(std::env::var_os("PATH").as_deref())
    else {
        eprintln!("skipped: no Compose CLI");
        return;
    };
    cx.executor().allow_parking();
    let root = workspace("containers");
    let project = root.join("htcompose");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("compose.yaml"), COMPOSE).unwrap();
    let Some(k) = kubectl_with(&config, &root) else {
        eprintln!("skipped: kubectl is missing");
        return;
    };
    let _ = kube(
        &k,
        &["delete", "-f", "-", "--ignore-not-found"],
        Some(MANIFEST),
    );
    let applied = kube(&k, &["apply", "-f", "-"], Some(MANIFEST));
    assert!(applied.contains("ht-web"), "{applied}");

    let (bench, cx) = window(&root, cx);
    let mut engines = vec![idecraft_containers::Engine::from_host("lima", host.clone())];
    if let Ok(words) = std::env::var("IDECRAFT_TEST_DOCKER_SSH") {
        let mut words: Vec<String> = words.split_whitespace().map(str::to_owned).collect();
        let destination = words.pop().unwrap();
        engines.push(idecraft_containers::Engine::from_host(
            "lima-ssh",
            idecraft_containers::DockerHost::Ssh {
                destination,
                port: None,
                extra: words,
                program: None,
            },
        ));
    }
    let over_ssh = engines.len() > 1;
    bench.update_in(cx, |b, window, cx| {
        b.set_container_sources(Some(host.clone()), Some(k.clone()));
        b.set_container_engines(engines);
        b.set_compose_sources(Some(project.clone()), Some(cli.clone()));
        b.toggle_containers(window, cx);
    });
    // The engine's socket, forwarded by Lima, may refuse a connection while it is busy: F5 reads
    // again, as a person would.
    let view = bench.read_with(cx, |b, _| b.containers_view()).unwrap();
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    until(
        cx,
        90,
        "the window lists the engine, Compose and the cluster",
        |cx| {
            if !has_row(&bench, cx, |l| l.starts_with("Compose: htcompose")) {
                keys(cx, "f5");
                std::thread::sleep(Duration::from_millis(300));
            }
            let t = container_texts(&bench, cx);
            t.iter().any(|l| l == "Compose: htcompose (compose.yaml)")
                && t.iter().any(|l| l.starts_with("Kubernetes"))
                && t.iter().any(|l| l.starts_with("  Deployment ht-web"))
        },
    );

    // 172: Compose up with `u` on the project row.
    go(&bench, cx, "Compose: htcompose");
    keys(cx, "u");
    until_shown(
        cx,
        180,
        "Compose up starts both services",
        |cx| {
            let t = container_texts(&bench, cx);
            t.iter().any(|l| l == "  ◆ web running") && t.iter().any(|l| l == "  ◆ db running")
        },
        |cx| {
            format!(
                "{:#?}\nstatus {}",
                container_texts(&bench, cx),
                status(&bench, cx)
            )
        },
    );
    until_shown(
        cx,
        60,
        "the containers are listed as running",
        |cx| {
            let done = has_row(&bench, cx, |l| l.starts_with("  ● htcompose-web-1"))
                && has_row(&bench, cx, |l| l.starts_with("  ● htcompose-db-1"));
            if !done {
                keys(cx, "f5");
                std::thread::sleep(Duration::from_millis(300));
            }
            done
        },
        |cx| {
            format!(
                "{:#?}\nstatus {}",
                container_texts(&bench, cx),
                status(&bench, cx)
            )
        },
    );

    // 171: `o` on the container opens its published port, which answers.
    go(&bench, cx, "● htcompose-web-1");
    keys(cx, "o");
    assert!(
        bench.read_with(cx, |b, _| b
            .opened_urls()
            .iter()
            .any(|u| u == "http://127.0.0.1:18083")),
        "{}",
        status(&bench, cx)
    );
    let page = http_get(18083, Duration::from_secs(30)).expect("nginx answers");
    assert!(page.contains("200 OK"), "{page}");

    // 46: x stops, s starts, Enter shows the logs.
    keys(cx, "x");
    until(cx, 60, "x stops the container", |cx| {
        has_row(&bench, cx, |l| l.starts_with("  ○ htcompose-web-1"))
    });
    go(&bench, cx, "htcompose-web-1");
    keys(cx, "s");
    until(cx, 60, "s starts it again", |cx| {
        has_row(&bench, cx, |l| l.starts_with("  ● htcompose-web-1"))
    });
    go(&bench, cx, "htcompose-web-1");
    keys(cx, "enter");
    until(cx, 30, "Enter shows the container's logs", |cx| {
        bench.read_with(cx, |b, cx| {
            b.containers_view().is_some_and(|v| {
                v.read(cx)
                    .logs()
                    .is_some_and(|(title, _)| title == "Logs: htcompose-web-1")
            })
        })
    });

    // 172: Enter on a service shows its logs.
    go(&bench, cx, "◆ db");
    keys(cx, "enter");
    until(
        cx,
        30,
        "Enter on the service shows its containers' logs",
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx).logs().is_some_and(|(title, text)| {
                        title == "Logs: service db" && text.contains("db up")
                    })
                })
            })
        },
    );

    // 46: r restarts the deployment, Enter on its pod shows the pod's logs.
    go(&bench, cx, "Deployment ht-web");
    keys(cx, "r");
    until(cx, 60, "r restarts the deployment", |cx| {
        status(&bench, cx) == "deployment.apps/ht-web restarted"
    });
    until(cx, 120, "a running pod of the deployment is listed", |cx| {
        keys(cx, "f5");
        has_row(&bench, cx, |l| {
            l.starts_with("  Pod ht-web-") && l.contains("Running 1/1")
        })
    });
    let pod = container_texts(&bench, cx)
        .into_iter()
        .find(|l| l.starts_with("  Pod ht-web-") && l.contains("Running 1/1"))
        .unwrap();
    let pod_name = pod.split_whitespace().nth(1).unwrap().to_owned();
    go(&bench, cx, &format!("Pod {pod_name}"));
    keys(cx, "enter");
    until(
        cx,
        60,
        "Enter on the pod shows its logs through kubectl",
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx)
                        .logs()
                        .is_some_and(|(title, _)| *title == format!("Logs: Pod {pod_name}"))
                })
            })
        },
    );

    // 170: `e` on the pod opens a shell through kubectl; what is typed runs in the pod. (A
    // container's shell is bridged by the `idecraft` program itself, which this test binary is
    // not; that bridge is tested live in `crates/containers/tests/live.rs`.)
    go(&bench, cx, &format!("Pod {pod_name}"));
    keys(cx, "e");
    until(cx, 60, "a terminal tab with the pod's shell opens", |cx| {
        bench.read_with(cx, |b, _| {
            b.terminal_tabs()
                .last()
                .is_some_and(|t| *t == format!("Exec: Pod {pod_name}"))
        })
    });
    let history = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.terminal()
                .map(|t| t.read(cx).history_text())
                .unwrap_or_default()
        })
    };
    until_shown(
        cx,
        60,
        "the shell is ready",
        |cx| history(cx).contains('#') || history(cx).contains('$'),
        |cx| history(cx),
    );
    // Typed into the focused terminal, Enter sends it. `kubectl exec` shows the prompt a moment
    // before the pod's shell reads its terminal; a person starts typing after a breath, too.
    std::thread::sleep(Duration::from_secs(2));
    cx.run_until_parked();
    cx.simulate_input("echo hi-$((40+2)); hostname");
    keys(cx, "enter");
    until_shown(
        cx,
        30,
        "the command runs in the pod",
        |cx| history(cx).contains("hi-42") && history(cx).contains(&pod_name),
        |cx| history(cx),
    );
    cx.simulate_input("exit");
    keys(cx, "enter");
    bench.update_in(cx, |b, window, cx| {
        if b.containers_view().is_none() {
            b.toggle_containers(window, cx);
        }
    });
    cx.run_until_parked();

    // 171: f forwards the service's port on 127.0.0.1; it answers; o and x on the forward.
    go(&bench, cx, "Service ht-web");
    keys(cx, "f");
    until(cx, 60, "the forward is listed", |cx| {
        has_row(&bench, cx, |l| {
            l.starts_with("  ⇄ service/ht-web 127.0.0.1:")
        })
    });
    let row = container_texts(&bench, cx)
        .into_iter()
        .find(|l| l.starts_with("  ⇄ service/ht-web 127.0.0.1:"))
        .unwrap();
    let port: u16 = row
        .split("127.0.0.1:")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("a port in {row:?}"));
    let page = http_get(port, Duration::from_secs(30)).expect("the forward answers");
    assert!(page.contains("200 OK"), "{page}");
    go(&bench, cx, "⇄ service/ht-web");
    keys(cx, "o");
    assert!(bench.read_with(cx, |b, _| {
        b.opened_urls()
            .iter()
            .any(|u| *u == format!("http://127.0.0.1:{port}"))
    }));
    keys(cx, "x");
    until(cx, 30, "x stops the forward", |cx| {
        status(&bench, cx) == "Port forward stopped"
            && !has_row(&bench, cx, |l| l.contains("⇄ service/ht-web"))
    });
    assert!(http_get(port, Duration::from_millis(500)).is_none());

    // 46: n moves to the next namespace, c to the next context (the VM has one).
    let heading = |cx: &mut VisualTestContext| {
        container_texts(&bench, cx)
            .into_iter()
            .find(|l| l.starts_with("Kubernetes"))
            .unwrap_or_default()
    };
    let before = heading(cx);
    go(&bench, cx, "Kubernetes");
    keys(cx, "n");
    until(cx, 30, "n shows another namespace", |cx| {
        heading(cx) != before
    });
    keys(cx, "c");
    until(cx, 30, "c keeps the only context readable", |cx| {
        heading(cx).starts_with("Kubernetes —")
    });

    // 173: the same engine over SSH, with `e` on the Docker heading.
    if over_ssh {
        go(&bench, cx, "Docker ");
        keys(cx, "e");
        until(cx, 60, "e switches to the engine over SSH", |cx| {
            status(&bench, cx).starts_with("Docker engine: lima-ssh (ssh://")
        });
        until(
            cx,
            60,
            "the engine over SSH lists the same containers",
            |cx| has_row(&bench, cx, |l| l.starts_with("  ● htcompose-web-1")),
        );
        go(&bench, cx, "Docker ");
        keys(cx, "e");
        until(cx, 60, "e switches back", |cx| {
            status(&bench, cx).starts_with("Docker engine: lima (")
        });
    } else {
        eprintln!("173 skipped: IDECRAFT_TEST_DOCKER_SSH is not set");
    }

    // 46: Delete asks, the second Delete removes; 172: d takes the project down.
    go(&bench, cx, "htcompose-db-1");
    keys(cx, "x");
    until(cx, 60, "the db container stops", |cx| {
        has_row(&bench, cx, |l| l.starts_with("  ○ htcompose-db-1"))
    });
    go(&bench, cx, "htcompose-db-1");
    keys(cx, "e");
    assert_eq!(
        status(&bench, cx),
        "htcompose-db-1 is not running: start it first (s)"
    );
    keys(cx, "delete");
    assert_eq!(
        bench.read_with(cx, |b, cx| b
            .containers_view()
            .and_then(|v| v.read(cx).removing().map(str::to_owned))),
        Some("htcompose-db-1".to_owned())
    );
    keys(cx, "delete");
    until(cx, 60, "the second Delete removes the container", |cx| {
        !has_row(&bench, cx, |l| l.contains("htcompose-db-1"))
    });
    go(&bench, cx, "Compose: htcompose");
    keys(cx, "d");
    until(
        cx,
        120,
        "Compose down removes the project's containers",
        |cx| {
            keys(cx, "f5");
            !has_row(&bench, cx, |l| l.contains("htcompose-web-1"))
        },
    );

    let _ = kube(
        &k,
        &["delete", "-f", "-", "--ignore-not-found"],
        Some(MANIFEST),
    );
    idecraft_containers::ports::PortForwards::global().stop_all();
    let _ = std::fs::remove_dir_all(&root);
}

// --------------------------------------------------------------------- hand tests 62, 180, 181

fn profiler_texts(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.profiler_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

fn profiled(bench: &Bench, cx: &mut VisualTestContext, starts: &str) -> bool {
    bench.read_with(cx, |b, cx| {
        b.profiler_view().is_some_and(|v| {
            let v = v.read(cx);
            !v.busy() && v.summary().starts_with(starts)
        })
    })
}

fn focus_profiler(bench: &Bench, cx: &mut VisualTestContext) {
    let view = bench
        .read_with(cx, |b, _| b.profiler_view())
        .expect("the Profiler window is shown");
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    cx.run_until_parked();
}

fn run_toml(root: &Path, name: &str, command: &[&str]) {
    let words: Vec<String> = command.iter().map(|w| format!("{w:?}")).collect();
    std::fs::write(
        root.join(".idecraft/run.toml"),
        format!(
            "[[run]]\nname = \"{name}\"\ncommand = [{}]\n",
            words.join(", ")
        ),
    )
    .unwrap();
}

const RECHEN: &str = "package demo;\n\npublic class Rechen {\n    static long teuer(long n) {\n        long s = 0;\n        for (long i = 0; i < n; i++) s += i % 7;\n        return s;\n    }\n    public static void main(String[] a) {\n        long t = 0;\n        for (int k = 0; k < 1500; k++) t += teuer(1_000_000);\n        System.out.println(t);\n    }\n}\n";

const MEM_PY: &str = "keep = []\n\n\ndef fuelle(n):\n    return [str(i) * 20 for i in range(n)]\n\n\nfor _ in range(4):\n    keep.append(fuelle(20000))\n";

const LEAK_C: &str = "#include <stdlib.h>\n#include <string.h>\n\nstatic void *kept[200];\n\nvoid lege_an(int i) {\n    kept[i] = malloc(64 * 1024);\n    memset(kept[i], 1, 64 * 1024);\n}\n\nint main(void) {\n    for (int i = 0; i < 200; i++) lege_an(i);\n    return 0;\n}\n";

/// Hand tests 62 and 180 (`SPEC-0136` v2, `SPEC-0250` v2; D37, hand test 180's missing note):
/// Find Action › Profile Selected records a single Java file with JFR, `2` shows Hot Spots with
/// `Rechen.teuer` first and its line, Enter opens it there, a share and a time that fit together;
/// Profile Memory records a Python script with tracemalloc (the busy function first, sizes in
/// bytes) and a C program built with `-g` with `leaks`, whose notes say that `leaks` does not
/// report the exit code. Skipped without `java`, or `cc` and `leaks`, for that part.
#[gpui::test]
fn hand_tests_62_and_180_profiles_with_keys(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = workspace("profiles");
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    std::fs::write(root.join("src/main/java/demo/Rechen.java"), RECHEN).unwrap();
    std::fs::write(root.join("mem.py"), MEM_PY).unwrap();
    std::fs::write(root.join("leak.c"), LEAK_C).unwrap();
    idecraft_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);

    if on_path("java").is_some() {
        run_toml(&root, "rechen", &["java", "src/main/java/demo/Rechen.java"]);
        bench.update(cx, |b, cx| b.select_run_configuration("rechen", cx));
        bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
        cx.run_until_parked();
        // Find Action, typed, Enter.
        keys(cx, "cmd-shift-a");
        cx.simulate_input("ProfileSelected");
        until_shown(
            cx,
            10,
            "Find Action offers Profile Selected",
            |cx| {
                bench.read_with(cx, |b, cx| {
                    b.everywhere().is_some_and(|v| {
                        matches!(
                            v.read(cx).results().selected_row(),
                            Some(idecraft_workbench::everywhere_model::Row::Action(a))
                                if a.id == "Profiler.ProfileSelected"
                        )
                    })
                })
            },
            |cx| {
                bench.read_with(cx, |b, cx| {
                    format!(
                        "{:?}",
                        b.everywhere().map(|v| v.read(cx).results().rows.clone())
                    )
                })
            },
        );
        keys(cx, "enter");
        until(cx, 240, "the Java file is recorded with JFR", |cx| {
            profiled(&bench, cx, "rechen · JFR · ")
        });
        let run_output = bench.read_with(cx, |b, cx| {
            b.run_view()
                .and_then(|v| v.read(cx).selected_output().map(|(_, t)| t))
                .unwrap_or_default()
        });
        assert!(
            !run_output.contains("jdk.jfr") && !run_output.contains("Started recording"),
            "no JFR messages in the output: {run_output}"
        );
        focus_profiler(&bench, cx);
        keys(cx, "2");
        let first = profiler_texts(&bench, cx)
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(
            first.starts_with("Rechen.teuer") && first.contains("Rechen.java:6"),
            "{first}"
        );
        // D37: the time of the share fits the run — it is that share of the run's wall time named
        // in the summary (before, JFR's samples read as "95 ms" for 71 % of 7.7 s). Measured
        // against the run itself, so a slow run under load passes as long as both agree.
        let summary = bench.read_with(cx, |b, cx| {
            b.profiler_view()
                .map(|v| v.read(cx).summary())
                .unwrap_or_default()
        });
        let run_ms = summary
            .split(" · ")
            .find_map(|part| part.strip_suffix(" ms")?.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("the run's time in {summary}"));
        let share = first
            .split("self ")
            .nth(1)
            .and_then(|s| s.split(" %").next())
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or_else(|| panic!("the share in {first}"));
        let shown_ms = first
            .split('(')
            .nth(1)
            .and_then(|t| t.split(')').next())
            .and_then(|t| match t.strip_suffix(" s") {
                Some(s) => s.parse::<f64>().ok().map(|s| s * 1000.0),
                None => t.strip_suffix(" ms")?.parse::<f64>().ok(),
            })
            .unwrap_or_else(|| panic!("the time in {first}"));
        let expected = share / 100.0 * run_ms;
        // The share is shown to 0.1 %, the time to 1 ms or 10 ms.
        let slack = run_ms * 0.0006 + 10.0;
        assert!(
            shown_ms > 0.0 && (shown_ms - expected).abs() <= slack,
            "{first}: {shown_ms} ms is not {share} % of the run's {run_ms} ms ({summary})"
        );
        keys(cx, "enter");
        until(cx, 20, "Enter opens Rechen.java at the line", |cx| {
            bench.read_with(cx, |b, cx| {
                b.active_file().is_some_and(|p| p.ends_with("Rechen.java"))
                    && b.caret_position(cx).map(|(l, _)| l) == Some(6)
            })
        });
    } else {
        eprintln!("62 skipped: java is missing");
    }

    run_toml(&root, "mem", &["python3", "mem.py"]);
    bench.update(cx, |b, cx| b.select_run_configuration("mem", cx));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("Profiler.ProfileMemory", window, cx)
    });
    until(
        cx,
        120,
        "Profile Memory records the script with tracemalloc",
        |cx| profiled(&bench, cx, "mem · tracemalloc · "),
    );
    let summary = bench.read_with(cx, |b, cx| {
        b.profiler_view()
            .map(|v| v.read(cx).summary())
            .unwrap_or_default()
    });
    assert!(summary.contains(" held"), "{summary}");
    focus_profiler(&bench, cx);
    keys(cx, "2");
    let first = profiler_texts(&bench, cx)
        .first()
        .cloned()
        .unwrap_or_default();
    assert!(
        first.starts_with("fuelle") && first.contains("mem.py:5") && first.contains("MB)"),
        "{first}"
    );
    keys(cx, "enter");
    until(cx, 20, "Enter opens mem.py at the line", |cx| {
        bench.read_with(cx, |b, cx| {
            b.active_file().is_some_and(|p| p.ends_with("mem.py"))
                && b.caret_position(cx).map(|(l, _)| l) == Some(5)
        })
    });

    if on_path("cc").is_some() && Path::new("/usr/bin/leaks").exists() {
        let built = Command::new("cc")
            .args(["-g", "-O0", "-o", "leak", "leak.c"])
            .current_dir(&root)
            .status()
            .is_ok_and(|s| s.success());
        assert!(built, "leak.c builds");
        run_toml(&root, "leak", &["./leak"]);
        bench.update(cx, |b, cx| b.select_run_configuration("leak", cx));
        bench.update_in(cx, |b, window, cx| {
            b.menu_action("Profiler.ProfileMemory", window, cx)
        });
        until(
            cx,
            180,
            "Profile Memory records the C program with leaks",
            |cx| profiled(&bench, cx, "leak · leaks · "),
        );
        let notes = bench.read_with(cx, |b, cx| {
            b.profiler_view()
                .and_then(|v| v.read(cx).notes_line())
                .unwrap_or_default()
        });
        assert!(
            notes.contains("leaks does not report the program's exit code"),
            "hand test 180: the note is shown: {notes:?}"
        );
        focus_profiler(&bench, cx);
        keys(cx, "2");
        let first = profiler_texts(&bench, cx)
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(first.starts_with("lege_an"), "{first}");
    } else {
        eprintln!("180, C part skipped: cc or leaks is missing");
    }
    let _ = std::fs::remove_dir_all(&root);
}

const BUSY: &str = "package demo;\n\npublic class Busy {\n    static long inner(long n) {\n        long s = 0;\n        for (long i = 0; i < n; i++) s += i % 7;\n        return s;\n    }\n    public static void main(String[] args) {\n        long t = 0;\n        for (int k = 0; k < 1500; k++) t += inner(1_000_000);\n        System.out.println(\"done \" + t);\n    }\n}\n";

/// Whether the Gradle distribution the wrapper in `root` names is unpacked in the Gradle cache,
/// so `./gradlew` runs without downloading anything.
fn wrapper_distribution_cached(root: &Path) -> bool {
    let properties = std::fs::read_to_string(root.join("gradle/wrapper/gradle-wrapper.properties"))
        .unwrap_or_default();
    let Some(zip) = properties
        .lines()
        .find_map(|l| l.strip_prefix("distributionUrl="))
        .and_then(|u| u.rsplit('/').next())
    else {
        return false;
    };
    let name = zip.trim_end_matches(".zip");
    let unpacked = name.trim_end_matches("-bin").trim_end_matches("-all");
    let user_home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gradle")));
    let Some(dists) = user_home.map(|h| h.join("wrapper/dists").join(name)) else {
        return false;
    };
    std::fs::read_dir(dists).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().join(unpacked).join("bin/gradle").is_file())
    })
}

/// Hand test 182, Gradle part (`SPEC-0252` behaviour 3): a Gradle application (`application`
/// plugin) with the run configuration `./gradlew run`, Find Action › Profile Selected with keys:
/// JDK Flight Recorder records the application's JVM, not Gradle's — `Busy.inner` is the hottest
/// method with its file and line, no Gradle frame is among the hot spots — and Enter opens the
/// line. The wrapper is made by the installed Gradle (`gradle wrapper`, offline) and runs the
/// distribution already in the Gradle cache; the project's `gradle.properties` turns the daemon
/// off, so no Gradle process outlives the test. Skipped without `gradle` or `java`, or when the
/// wrapper's distribution is not cached (it would be downloaded).
#[gpui::test]
fn hand_test_182_gradle_run_profiles_the_application(cx: &mut TestAppContext) {
    let root = workspace("gradle");
    if on_path("gradle").is_none() || on_path("java").is_none() {
        eprintln!("182 skipped: gradle or java is missing");
        return;
    }
    std::fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    std::fs::write(root.join("settings.gradle"), "rootProject.name = 'busy'\n").unwrap();
    std::fs::write(
        root.join("build.gradle"),
        "plugins { id 'application' }\napplication { mainClass = 'demo.Busy' }\n",
    )
    .unwrap();
    std::fs::write(root.join("gradle.properties"), "org.gradle.daemon=false\n").unwrap();
    std::fs::write(root.join("src/main/java/demo/Busy.java"), BUSY).unwrap();
    let wrapped = Command::new("gradle")
        .args(["-q", "--offline", "wrapper"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        wrapped.status.success(),
        "gradle wrapper: {}",
        String::from_utf8_lossy(&wrapped.stderr)
    );
    if !wrapper_distribution_cached(&root) {
        eprintln!("182 skipped: the wrapper's Gradle distribution is not in the Gradle cache");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    cx.executor().allow_parking();
    idecraft_core::trust::trust_for_process(&root);
    let (bench, cx) = window(&root, cx);
    run_toml(&root, "app", &["./gradlew", "run"]);
    bench.update(cx, |b, cx| b.select_run_configuration("app", cx));
    bench.update_in(cx, |b, window, cx| b.focus_tree(window, cx));
    cx.run_until_parked();
    keys(cx, "cmd-shift-a");
    cx.simulate_input("ProfileSelected");
    until(cx, 10, "Find Action offers Profile Selected", |cx| {
        bench.read_with(cx, |b, cx| {
            b.everywhere().is_some_and(|v| {
                matches!(
                    v.read(cx).results().selected_row(),
                    Some(idecraft_workbench::everywhere_model::Row::Action(a))
                        if a.id == "Profiler.ProfileSelected"
                )
            })
        })
    });
    keys(cx, "enter");
    until_shown(
        cx,
        20,
        "the Profiler window records the configuration",
        |cx| bench.read_with(cx, |b, _| b.profiler_view().is_some()),
        |cx| status(&bench, cx),
    );
    // Gradle starts its own JVM and then the application's; under load that takes minutes. The
    // window says when it is done (no longer busy), with the profile or the reason it failed.
    let end = Instant::now() + Duration::from_secs(900);
    let summary = loop {
        cx.run_until_parked();
        let (busy, summary) = bench.read_with(cx, |b, cx| {
            b.profiler_view()
                .map(|v| (v.read(cx).busy(), v.read(cx).summary()))
                .unwrap_or_default()
        });
        if !busy {
            break summary;
        }
        assert!(Instant::now() < end, "Profile Selected hangs: {summary}");
        std::thread::sleep(Duration::from_millis(100));
        cx.executor().advance_clock(Duration::from_millis(100));
    };
    assert!(
        summary.starts_with("app · JFR · "),
        "{summary}: {}",
        status(&bench, cx)
    );
    // The run is kept with its output (SPEC-0120 behaviour 6): the application's line, none of
    // the recorder's.
    let kept: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(".idecraft/profiles/last.json")).unwrap(),
    )
    .unwrap();
    let output = kept["output"].as_str().unwrap_or_default();
    assert!(
        output.contains("done ")
            && !output.contains("jdk.jfr")
            && !output.contains("Started recording"),
        "the application ran under Gradle: {output}"
    );
    assert_eq!(kept["exit_code"], serde_json::json!(0), "{output}");
    focus_profiler(&bench, cx);
    keys(cx, "2");
    let rows = profiler_texts(&bench, cx);
    let first = rows.first().cloned().unwrap_or_default();
    assert!(
        first.starts_with("Busy.inner") && first.contains("Busy.java:6"),
        "the application's method is the hottest: {rows:#?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("org.gradle")),
        "the recording is the application's, not Gradle's: {rows:#?}"
    );
    keys(cx, "enter");
    until(cx, 20, "Enter opens Busy.java at the line", |cx| {
        bench.read_with(cx, |b, cx| {
            b.active_file().is_some_and(|p| p.ends_with("Busy.java"))
                && b.caret_position(cx).map(|(l, _)| l) == Some(6)
        })
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 181 (`SPEC-0251` v2): Attach Profiler to Process lists the running processes;
/// typed text narrows the list, Enter attaches; the program goes on running and a sampled call
/// tree of it is shown; a look-alike of the reference is refused with the reason. macOS samples
/// with `/usr/bin/sample`.
#[gpui::test]
fn hand_test_181_attach_to_a_running_process_with_keys(cx: &mut TestAppContext) {
    if !Path::new("/usr/bin/sample").exists() {
        eprintln!("skipped: /usr/bin/sample is missing");
        return;
    }
    cx.executor().allow_parking();
    let root = workspace("attach");
    let spin_name = format!("ht181-spin-{}", std::process::id());
    let mut spin = Command::new("python3")
        .args([
            "-c",
            "import time\nwhile True: sum(range(1000))",
            &format!("ht181-spin-{}", std::process::id()),
        ])
        .spawn()
        .unwrap();
    let pid = spin.id();
    let mut alike = Command::new("/bin/sh")
        .args(["-c", "sleep 120; true", "com.intellij.ht181"])
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.set_attach_seconds(2);
        b.menu_action("Profiler.AttachToProcess", window, cx);
    });
    let processes = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.profiler_view()
                .map(|v| v.read(cx).process_texts())
                .unwrap_or_default()
        })
    };
    until(
        cx,
        30,
        "the process list shows the started processes",
        |cx| {
            let t = processes(cx);
            t.iter().any(|l| l.contains(&spin_name))
                && t.iter()
                    .any(|l| l.contains("com.intellij.ht181") && l.ends_with("— refused"))
        },
    );
    focus_profiler(&bench, cx);
    cx.simulate_input("com.intellij.ht181");
    keys(cx, "enter");
    let note = bench.read_with(cx, |b, cx| {
        b.profiler_view()
            .and_then(|v| v.read(cx).picker_note().map(str::to_owned))
            .unwrap_or_default()
    });
    assert!(note.contains("RESEARCH_BOUNDARIES"), "{note:?}");
    for _ in 0.."com.intellij.ht181".len() {
        keys(cx, "backspace");
    }
    cx.simulate_input(&spin_name);
    cx.run_until_parked();
    let shown = processes(cx);
    assert!(
        shown.iter().all(|l| l.contains(&spin_name)),
        "typed text narrows the list: {shown:?}"
    );
    keys(cx, "enter");
    until_shown(
        cx,
        60,
        "a call tree of the attached process is shown",
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.profiler_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.busy()
                        && !v.picking()
                        && v.summary()
                            .starts_with(&format!("process {pid} · sample · "))
                        && v.notes_line()
                            .is_some_and(|n| n.contains(&format!("attached to process {pid}")))
                })
            })
        },
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.profiler_view()
                    .map(|v| {
                        let v = v.read(cx);
                        format!(
                            "busy {} picking {} summary {:?} notes {:?} note {:?} processes {:?} status {:?}",
                            v.busy(),
                            v.picking(),
                            v.summary(),
                            v.notes_line(),
                            v.picker_note(),
                            v.process_texts(),
                            b.status_message(cx)
                        )
                    })
                    .unwrap_or_default()
            })
        },
    );
    assert_eq!(
        bench.read_with(cx, |b, cx| b.profiler_view().map(|v| v.read(cx).tab())),
        Some(ProfilerTab::CallTree)
    );
    assert!(
        spin.try_wait().unwrap().is_none(),
        "the program goes on running"
    );
    let _ = spin.kill();
    let _ = alike.kill();
    let _ = spin.wait();
    let _ = alike.wait();
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------- hand test 183

fn remote_texts(bench: &Bench, cx: &mut VisualTestContext) -> Vec<String> {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    })
}

/// How long a step on the test host may take before it counts as hanging. Every call of the
/// window has its own limit (10 s to connect, 30 s a command, SPEC-0121 behaviour 2) and shows
/// an error when it is exceeded, so a slow machine only waits longer for the same result.
const HOST_HANG: Duration = Duration::from_secs(300);

/// A local `sshd` as the test host, ready: it has completed a whole SSH session through
/// IDECraft's own `ssh`, so the steps that follow do not race the server's start under load.
fn ready_host(dir: &Path) -> Option<idecraft_remote::testing::TestHost> {
    let host = idecraft_remote::testing::TestHost::start(dir)?;
    if let Err(e) = host.wait_ready(HOST_HANG) {
        panic!("the test host started but never let a session in: {e}");
    }
    Some(host)
}

/// The Remote Host window's state, for a failure message.
fn remote_state(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .map(|v| {
                let v = v.read(cx);
                format!(
                    "place {:?} field {:?} busy {} error {:?} rows {:?} status {:?}",
                    v.place(),
                    v.field_text(cx),
                    v.busy(),
                    v.error(),
                    v.texts(),
                    b.status_message(cx)
                )
            })
            .unwrap_or_default()
    })
}

/// Waits until `done` holds after a step on the test host: on the window's own answer, not a
/// fixed time. An error the window shows ends the wait at once with that error; a step that
/// neither finishes nor fails within [`HOST_HANG`] is a hang.
fn until_host(
    bench: &Bench,
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + HOST_HANG;
    while Instant::now() < end {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        let failed = bench.read_with(cx, |b, cx| {
            b.remote_view()
                .and_then(|v| v.read(cx).error().map(str::to_owned))
        });
        if let Some(e) = failed {
            panic!(
                "{what}: the window reports {e:?}\n{}",
                remote_state(bench, cx)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    let state = remote_state(bench, cx);
    panic!("timed out: {what}\n{state}");
}

/// Moves the Remote Host window's selection with ↑/↓ to the row `name`, focus on the list.
fn go_remote(bench: &Bench, cx: &mut VisualTestContext, name: &str) {
    let view = bench
        .read_with(cx, |b, _| b.remote_view())
        .expect("the Remote Host window is shown");
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    cx.run_until_parked();
    for _ in 0..100 {
        let (texts, selected) = view.read_with(cx, |v, _| (v.texts(), v.selected_text()));
        if selected.as_deref().is_some_and(|s| s.contains(name)) {
            return;
        }
        let want = texts
            .iter()
            .position(|t| t.contains(name))
            .unwrap_or_else(|| panic!("no row {name:?}: {texts:#?}"));
        let at = selected
            .and_then(|s| texts.iter().position(|t| *t == s))
            .unwrap_or(0);
        keys(cx, if want > at { "down" } else { "up" });
    }
    panic!("the selection does not reach {name:?}");
}

/// Hand test 183 (`SPEC-0253` v2) against the system `sshd` started as the current user: in a
/// test folder ⌘N with a typed name and Enter creates and opens a file, ⇧F6 renames it, ⇧⌘N
/// creates a folder, ⌘⌫ on the folder asks first and names what it holds, Enter deletes it, and
/// ⌘⌫ on the file deletes it. Each step waits for the window's own answer (the result, or the
/// error it shows), after the test host has let a first session in (`ready_host`): under load the
/// first connection once took longer than a fixed 60 s.
#[gpui::test]
fn hand_test_183_files_on_the_host_with_keys(cx: &mut TestAppContext) {
    let base = std::env::temp_dir().join(format!("idecraft-f083-ssh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let Some(host) = ready_host(&base) else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    cx.executor().allow_parking();
    let folder = base.canonicalize().unwrap().join("ht183");
    std::fs::create_dir_all(&folder).unwrap();
    let root = workspace("remote");
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.toggle_remote(window, cx);
        b.remote_connect(host.target.clone(), cx);
    });
    until_host(&bench, cx, "the home folder of the host is shown", |cx| {
        bench.read_with(cx, |b, cx| {
            b.remote_view()
                .is_some_and(|v| v.read(cx).place().is_some())
        })
    });
    // The test folder typed into the field, Enter.
    let field = bench.read_with(cx, |b, cx| {
        b.remote_view().map(|v| v.read(cx).field_focus(cx)).unwrap()
    });
    cx.update(|window, _| window.focus(&field));
    keys(cx, "cmd-a");
    cx.simulate_input(&folder.display().to_string());
    keys(cx, "enter");
    until_host(&bench, cx, "the test folder is shown", |cx| {
        bench.read_with(cx, |b, cx| {
            b.remote_view().is_some_and(|v| {
                v.read(cx)
                    .place()
                    .is_some_and(|(_, p)| Path::new(p) == folder)
            })
        })
    });

    // ⌘N, a name, Enter: the file is made and opens.
    go_remote(&bench, cx, "..");
    keys(cx, "cmd-n");
    cx.simulate_input("neu.txt");
    keys(cx, "enter");
    until_host(&bench, cx, "neu.txt is made on the host and opens", |cx| {
        folder.join("neu.txt").is_file()
            && bench.read_with(cx, |b, _| {
                b.active_file().is_some_and(|p| p.ends_with("neu.txt"))
            })
    });
    until_host(&bench, cx, "the list shows it", |cx| {
        remote_texts(&bench, cx)
            .iter()
            .any(|t| t.contains("neu.txt"))
    });

    // ⇧F6: the field holds the name; a new one, Enter.
    go_remote(&bench, cx, "neu.txt");
    keys(cx, "shift-f6");
    let named = bench.read_with(cx, |b, cx| {
        b.remote_view().map(|v| v.read(cx).field_text(cx)).unwrap()
    });
    assert_eq!(named, "neu.txt");
    keys(cx, "cmd-a");
    cx.simulate_input("umbenannt.txt");
    keys(cx, "enter");
    until_host(&bench, cx, "the file is renamed on the host", |_| {
        folder.join("umbenannt.txt").is_file() && !folder.join("neu.txt").exists()
    });

    // ⇧⌘N: a folder; a file put into it on the host.
    go_remote(&bench, cx, "..");
    keys(cx, "cmd-shift-n");
    cx.simulate_input("ordner");
    keys(cx, "enter");
    until_host(&bench, cx, "the folder is made on the host", |_| {
        folder.join("ordner").is_dir()
    });
    std::fs::write(folder.join("ordner/inhalt.txt"), "x\n").unwrap();
    keys(cx, "f5");
    until_host(&bench, cx, "the list shows the folder", |cx| {
        remote_texts(&bench, cx)
            .iter()
            .any(|t| t.contains("ordner"))
    });

    // ⌘⌫ on the folder asks and names what it holds; Enter deletes it.
    go_remote(&bench, cx, "ordner");
    keys(cx, "cmd-backspace");
    until_host(
        &bench,
        cx,
        "the question names the folder and its one entry",
        |cx| {
            bench.read_with(cx, |b, cx| {
                b.remote_view().is_some_and(|v| {
                    matches!(
                        v.read(cx).pending(),
                        Some(idecraft_workbench::RemotePending::Delete(p, 1))
                            if p.ends_with("/ordner")
                    )
                })
            })
        },
    );
    assert!(
        folder.join("ordner").exists(),
        "nothing is deleted before Enter"
    );
    keys(cx, "enter");
    until_host(&bench, cx, "the folder is deleted on the host", |_| {
        !folder.join("ordner").exists()
    });

    // ⌘⌫ on the file.
    keys(cx, "f5");
    until_host(&bench, cx, "the list is read again", |cx| {
        !remote_texts(&bench, cx)
            .iter()
            .any(|t| t.contains("ordner"))
    });
    go_remote(&bench, cx, "umbenannt.txt");
    keys(cx, "cmd-backspace");
    cx.run_until_parked();
    if folder.join("umbenannt.txt").exists() {
        keys(cx, "enter");
    }
    until_host(&bench, cx, "the file is deleted on the host", |_| {
        !folder.join("umbenannt.txt").exists()
    });
    drop(host);
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&root);
}
