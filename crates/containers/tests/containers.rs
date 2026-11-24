//! SPEC-0119 AK-2 (and SPEC-0240 … SPEC-0243 AK-2) against the stand-ins of
//! `ferris_idea_containers::fake`: a Docker Engine API on a real Unix socket, `kubectl`, `ssh` and
//! `docker-compose` scripts.
#![cfg(unix)]

use std::path::PathBuf;

use std::time::Duration;

use ferris_idea_containers::compose::{self, ComposeAction, ComposeCli};
use ferris_idea_containers::fake::{FakeEngine, compose_script, kubectl_script, ssh_script};
use ferris_idea_containers::ports::PortForwards;
use ferris_idea_containers::{
    ContainerAction, DockerError, DockerHost, ExecOptions, KubeError, Kubectl, exec,
};

fn scratch(tag: &str) -> PathBuf {
    // Unix socket paths are short: keep them under /tmp.
    let p = PathBuf::from(format!("/tmp/idc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn docker_lists_acts_and_reads_logs() {
    let dir = scratch("dock");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let info = host.info().unwrap();
    assert_eq!(
        (info.version.as_str(), info.platform.as_str()),
        ("27.3.1", "linux/arm64")
    );
    let list = host.containers().unwrap();
    let names: Vec<(&str, &str)> = list
        .iter()
        .map(|c| (c.name.as_str(), c.state.as_str()))
        .collect();
    assert_eq!(names, [("web", "running"), ("db", "exited")]);
    assert_eq!(list[0].ports, ["0.0.0.0:8080->80/tcp"]);
    assert_eq!(list[0].compose_project.as_deref(), Some("shop"));
    let images = host.images().unwrap();
    assert_eq!(images[0].tags, ["nginx:1.27"]);
    assert!(images[1].tags.is_empty());
    assert_eq!(host.logs("web", 100).unwrap(), "GET / 200\nwarn: slow\n");
    host.act("web", ContainerAction::Start).unwrap();
    host.act("web", ContainerAction::Stop).unwrap();
    host.act("db", ContainerAction::Restart).unwrap();
    let states: Vec<String> = host
        .containers()
        .unwrap()
        .into_iter()
        .map(|c| format!("{} {}", c.name, c.state))
        .collect();
    assert_eq!(states, ["db running", "web exited"]);
    host.act("web", ContainerAction::Remove).unwrap();
    assert_eq!(host.containers().unwrap().len(), 1);
    assert_eq!(
        host.act("nope", ContainerAction::Start),
        Err(DockerError::Http(404, "No such container: nope".into()))
    );
    let seen = engine.seen();
    assert!(
        seen.contains(&"POST /containers/web/stop?t=10".to_owned()),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"DELETE /containers/web?force=1".to_owned()),
        "{seen:?}"
    );
    let gone = DockerHost::Unix(dir.join("missing.sock"));
    assert!(matches!(gone.info(), Err(DockerError::Connect(..))));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn kubectl_contexts_resources_logs_and_restarts() {
    let dir = scratch("kube");
    let kubectl = Kubectl {
        program: kubectl_script(&dir).unwrap(),
    };
    let contexts = kubectl.contexts().unwrap();
    assert_eq!(contexts.len(), 2);
    assert!(contexts[0].current && contexts[0].namespace == "shop");
    assert_eq!(
        (contexts[1].name.as_str(), contexts[1].namespace.as_str()),
        ("prod", "default")
    );
    assert_eq!(kubectl.namespaces("dev").unwrap(), ["default", "shop"]);
    let resources = kubectl.resources("dev", "shop").unwrap();
    let kinds: Vec<(&str, &str, &str)> = resources
        .iter()
        .map(|r| (r.kind.as_str(), r.name.as_str(), r.status.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Deployment", "web", "Progressing"),
            ("Pod", "web-7d9", "Running"),
            ("Service", "web", "ClusterIP")
        ]
    );
    assert_eq!(resources[1].restarts, Some(2));
    assert_eq!(
        kubectl.resources("prod", "default"),
        Err(KubeError::Failed(
            "error: You must be logged in to the server (Unauthorized)".into()
        ))
    );
    assert_eq!(
        kubectl.logs("dev", "shop", "web-7d9", 200).unwrap(),
        "started on :80\nGET /healthz 200\n"
    );
    assert_eq!(
        kubectl
            .restart("dev", "shop", "Deployment", "web")
            .unwrap()
            .trim(),
        "deployment.apps/web restarted"
    );
    assert_eq!(
        kubectl
            .restart("dev", "shop", "Pod", "web-7d9")
            .unwrap()
            .trim(),
        "pod \"web-7d9\" deleted"
    );
    assert!(kubectl.restart("dev", "shop", "Service", "web").is_err());
    assert!(
        kubectl.logs("dev", "shop", "--all", 1).is_err(),
        "names never become options"
    );
    let missing = Kubectl {
        program: dir.join("nope"),
    };
    assert_eq!(missing.contexts(), Err(KubeError::Missing));
    let log = std::fs::read_to_string(dir.join("kubectl.log")).unwrap();
    assert!(log.lines().all(|l| !l.contains("--all ")), "{log}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn docker_exec_with_and_without_tty() {
    let dir = scratch("exec");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let out = host
        .exec(
            "web",
            &["uname".into(), "-s".into()],
            &ExecOptions::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(out.output, "ran: uname -s\nnote\n");
    assert_eq!(out.exit_code, Some(0));
    assert!(!out.truncated);
    let out = host
        .exec(
            "web",
            &["sh".into(), "-c".into(), "exit 3".into()],
            &ExecOptions::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(out.exit_code, Some(3));
    assert_eq!(
        host.exec(
            "db",
            &["ls".into()],
            &ExecOptions::default(),
            Duration::from_secs(5)
        ),
        Err(DockerError::Http(409, "container db is not running".into()))
    );
    // Interactive: the bridge sends typed lines and the size, and ends with the shell's code.
    let sizes = std::sync::Arc::new(std::sync::Mutex::new((30u16, 100u16)));
    let later = sizes.clone();
    let mut shown = Vec::new();
    let code = exec::bridge(
        &host,
        "web",
        SlowInput(
            b"hello\r".to_vec(),
            Some(b"exit 4\r".to_vec()),
            sizes.clone(),
        ),
        &mut shown,
        move || later.lock().ok().map(|s| *s),
    )
    .unwrap();
    let shown = String::from_utf8_lossy(&shown);
    assert!(shown.starts_with("fake shell\r\n$ "), "{shown}");
    assert!(shown.contains("you said hello"), "{shown}");
    assert_eq!(code, 4);
    let seen = engine.seen();
    assert!(
        seen.iter().any(|r| r == "POST /exec/e3/resize?h=30&w=100"),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|r| r == "POST /exec/e3/resize?h=40&w=120"),
        "{seen:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Input that sends its first part, then changes the terminal size and waits before the second.
struct SlowInput(
    Vec<u8>,
    Option<Vec<u8>>,
    std::sync::Arc<std::sync::Mutex<(u16, u16)>>,
);

impl std::io::Read for SlowInput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.0.is_empty() {
            let n = self.0.len().min(buf.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0.drain(..n);
            return Ok(n);
        }
        let Some(next) = self.1.take() else {
            std::thread::sleep(Duration::from_secs(60));
            return Ok(0);
        };
        if let Ok(mut s) = self.2.lock() {
            *s = (40, 120);
        }
        std::thread::sleep(Duration::from_millis(700));
        self.0 = next;
        self.read(buf)
    }
}

#[test]
fn docker_over_ssh_stand_in() {
    let dir = scratch("ssh");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Ssh {
        destination: "dev@box".into(),
        port: Some(2222),
        extra: vec![],
        program: Some(ssh_script(&dir, &engine.socket).unwrap()),
    };
    assert_eq!(host.info().unwrap().version, "27.3.1");
    assert_eq!(host.containers().unwrap().len(), 2);
    assert_eq!(host.logs("web", 10).unwrap(), "GET / 200\nwarn: slow\n");
    let log = std::fs::read_to_string(dir.join("ssh.log")).unwrap();
    assert!(
        log.lines()
            .all(|l| l.contains("-p 2222 -- dev@box docker system dial-stdio")),
        "{log}"
    );
    assert!(
        log.lines().all(|l| l.contains("-o KexAlgorithms=")),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// SEC test (ADR-0025, SPEC-0243 v2): Docker over SSH to a host whose `sshd` offers only classical
/// key exchanges is refused with the post-quantum message.
#[test]
fn docker_over_ssh_refuses_a_classical_host() {
    let dir = scratch("pqdock");
    let Some(h) = ferris_idea_remote::testing::TestHost::start_classical(&dir.join("sshd")) else {
        eprintln!("skipped: no sshd/ssh-keygen");
        return;
    };
    let host = DockerHost::Ssh {
        destination: h.target.host.clone(),
        port: None,
        extra: h.target.extra.clone(),
        program: None,
    };
    let e = host.info().unwrap_err().to_string();
    assert!(e.contains("no post-quantum key exchange"), "{e}");
    assert!(e.contains("ADR-0025"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The Compose CLI starts `ssh` from PATH itself; for an `ssh://` engine it finds Ferris IDEA's
/// stand-in with the shared options first (ADR-0025, SPEC-0242 v2).
#[test]
fn compose_over_ssh_gets_the_shared_ssh_options() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("pqcompose");
    let project_dir = dir.join("shop");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("compose.yaml"),
        "services:\n  web:\n    image: nginx\n",
    )
    .unwrap();
    let which = dir.join("which");
    let script = dir.join("docker-compose");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncommand -v ssh > '{}'\necho done\n",
            which.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = compose::detect(&project_dir).unwrap();
    let cli = ComposeCli {
        program: script,
        prefix: vec![],
    };
    let host = DockerHost::parse("ssh://dev@box", vec![]).unwrap();
    cli.run(&project, &host, ComposeAction::Down, None).unwrap();
    let ssh = std::fs::read_to_string(&which).unwrap();
    let ssh = ssh.trim();
    assert!(ssh.ends_with("/bin/ssh"), "{ssh}");
    let stand_in = std::fs::read_to_string(ssh).unwrap();
    assert!(stand_in.contains("KexAlgorithms="), "{stand_in}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn kubectl_exec_and_port_forwards() {
    let dir = scratch("kfwd");
    let kubectl = Kubectl {
        program: kubectl_script(&dir).unwrap(),
    };
    let resources = kubectl.resources("dev", "shop").unwrap();
    assert_eq!(resources[1].ports, [80]);
    let args = kubectl
        .exec_args("dev", "shop", "web-7d9", None, &["hostname".into()], false)
        .unwrap();
    let out = kubectl.exec(&args, Duration::from_secs(5)).unwrap();
    assert_eq!(out.exit_code, Some(2));
    assert!(
        out.output.contains("in pod: exec web-7d9"),
        "{}",
        out.output
    );
    assert!(out.output.ends_with("pod note\n"), "{}", out.output);
    let forwards = PortForwards::default();
    let f = forwards
        .start(&kubectl, "dev", "shop", "Pod", "web-7d9", 80, None)
        .unwrap();
    assert_eq!((f.target.as_str(), f.remote_port), ("pod/web-7d9", 80));
    assert!(f.local_port >= 1024 && f.running);
    assert_eq!(forwards.list().len(), 1);
    assert!(forwards.stop(f.id));
    assert!(forwards.list().is_empty());
    assert!(!forwards.stop(f.id));
    assert_eq!(
        forwards.start(&kubectl, "dev", "shop", "Pod", "gone", 80, Some(18_080)),
        Err(KubeError::Failed("error: pods \"gone\" not found".into()))
    );
    assert!(
        forwards
            .start(&kubectl, "dev", "shop", "Job", "j", 80, None)
            .is_err()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn compose_services_states_and_commands() {
    let dir = scratch("compose");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let project_dir = dir.join("shop");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("compose.yaml"),
        "services:\n  web:\n    image: nginx\n  worker:\n    image: busybox\n",
    )
    .unwrap();
    let project = compose::detect(&project_dir).unwrap();
    assert_eq!(project.name, "shop");
    let states = compose::service_states(&project, &host.containers().unwrap());
    let s: Vec<(&str, &str)> = states
        .iter()
        .map(|s| (s.service.as_str(), s.state.as_str()))
        .collect();
    assert_eq!(s, [("web", "running"), ("worker", "not created")]);
    assert_eq!(
        compose::service_logs(&host, &states[0], 50).unwrap(),
        "GET / 200\nwarn: slow\n"
    );
    let cli = ComposeCli {
        program: compose_script(&dir).unwrap(),
        prefix: vec![],
    };
    let out = cli
        .run(&project, &host, ComposeAction::Up, Some("web"))
        .unwrap();
    assert_eq!(compose::last_line(&out), "Container shop-web-1  Started");
    // Each line once, stderr and stdout in the order they came (SPEC-0242 v4 behaviour 9, D7).
    assert_eq!(
        out,
        " Network shop_default Creating\n Network shop_default Created\nContainer shop-web-1  Started\n"
    );
    cli.run(&project, &host, ComposeAction::Down, None).unwrap();
    let log = std::fs::read_to_string(dir.join("compose.log")).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    let base = format!(
        "DOCKER_HOST={host} --project-directory {} -f {} -p shop",
        project_dir.display(),
        project_dir.join("compose.yaml").display()
    );
    assert_eq!(
        lines,
        [format!("{base} up --detach web"), format!("{base} down")]
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Waits up to 5 s for `done`.
fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < end {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out: {what}");
}

/// SPEC-0119 v5 AK-8: changes made outside reach a watcher within a second; when the socket goes
/// away the watcher says so and connects again, with growing pauses, once it is back.
#[test]
fn events_reach_the_watcher_and_it_reconnects() {
    use ferris_idea_containers::events::{EventWatch, WatchEvent};
    let dir = scratch("evt");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let seen: std::sync::Arc<std::sync::Mutex<Vec<(std::time::Instant, WatchEvent)>>> =
        std::sync::Arc::default();
    let sink = std::sync::Arc::clone(&seen);
    let watch = EventWatch::start(host.clone(), move |e| {
        sink.lock().unwrap().push((std::time::Instant::now(), e));
    });
    let has = |f: &dyn Fn(&WatchEvent) -> bool| seen.lock().unwrap().iter().any(|(_, e)| f(e));
    wait_for("connected", || has(&|e| *e == WatchEvent::Connected));
    wait_for("the engine counts the follower", || {
        engine.event_followers() == 1
    });
    let changed = std::time::Instant::now();
    engine.outside_create("cache", "redis:7");
    wait_for("create and start of cache", || {
        has(
            &|e| matches!(e, WatchEvent::Event(ev) if ev.action == "start" && ev.name.as_deref() == Some("cache")),
        )
    });
    let arrived = seen
        .lock()
        .unwrap()
        .iter()
        .find(|(_, e)| matches!(e, WatchEvent::Event(ev) if ev.action == "create"))
        .map(|(t, _)| *t)
        .unwrap();
    assert!(arrived.duration_since(changed) < Duration::from_secs(1));
    assert!(host.containers().unwrap().iter().any(|c| c.name == "cache"));
    engine.outside_remove("cache");
    wait_for("destroy", || {
        has(&|e| matches!(e, WatchEvent::Event(ev) if ev.action == "destroy"))
    });
    engine.go_away();
    wait_for("lost", || has(&|e| matches!(e, WatchEvent::Lost { .. })));
    // While away, every try fails and the pauses grow (no busy loop).
    std::thread::sleep(Duration::from_millis(1700));
    let pauses: Vec<Duration> = seen
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, e)| match e {
            WatchEvent::Lost { retry_in, .. } => Some(*retry_in),
            _ => None,
        })
        .collect();
    assert!(pauses.len() >= 2 && pauses.len() <= 4, "{pauses:?}");
    assert!(pauses.windows(2).all(|w| w[1] > w[0]), "{pauses:?}");
    let reason = seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            WatchEvent::Lost { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .unwrap();
    assert!(reason.contains("cannot reach Docker"), "{reason}");
    let before = seen.lock().unwrap().len();
    engine.come_back().unwrap();
    wait_for("connected again", || {
        seen.lock().unwrap()[before..]
            .iter()
            .any(|(_, e)| *e == WatchEvent::Connected)
    });
    engine.outside_set("db", true);
    wait_for("events after the reconnect", || {
        has(
            &|e| matches!(e, WatchEvent::Event(ev) if ev.action == "start" && ev.name.as_deref() == Some("db")),
        )
    });
    drop(watch);
    let mut on = false;
    wait_for("the watcher ends its stream", || {
        // The engine notices a closed stream when it writes the next event.
        engine.outside_set("db", on);
        on = !on;
        engine.event_followers() == 0
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// SPEC-0119 v5 AK-9: logs follow, a restart starts a new run, `since`/`tail` and time stamps.
#[test]
fn logs_follow_runs_and_queries() {
    use ferris_idea_containers::StopFlag;
    use ferris_idea_containers::logs::{LogBook, LogQuery, LogRow};
    let dir = scratch("logf");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let starts = host.start_times("web").unwrap();
    assert_eq!(starts.len(), 1, "inspect names the one start");
    let book = std::sync::Arc::new(std::sync::Mutex::new(LogBook::new(starts)));
    let stop = StopFlag::new();
    let (h, b, s) = (host.clone(), std::sync::Arc::clone(&book), stop.clone());
    let follower = std::thread::spawn(move || {
        let q = LogQuery {
            tail: Some(1000),
            since: None,
            timestamps: true,
        };
        h.follow_logs("web", &q, &s, |t| b.lock().unwrap().push(t))
    });
    wait_for("the first lines", || book.lock().unwrap().len() == 2);
    engine.log_line("web", 1, "GET /new 200");
    wait_for("a new line while following", || {
        book.lock().unwrap().text().ends_with("GET /new 200\n")
    });
    // A restart ends the followed stream; the next start is a new run.
    engine.outside_restart("web");
    assert!(
        follower.join().unwrap().is_ok(),
        "the stream ends with the run"
    );
    let starts = host.start_times("web").unwrap();
    assert_eq!(starts.len(), 2, "the event history names the second start");
    {
        let mut b = book.lock().unwrap();
        b.new_run(*starts.last().unwrap());
        let since = (b.last_time().unwrap() / 1_000_000_000) as i64;
        let q = LogQuery {
            tail: None,
            since: Some(since),
            timestamps: true,
        };
        let text = host.logs_with("web", &q).unwrap();
        b.push(&text);
        let rows: Vec<String> = b
            .rows(false, None, false)
            .iter()
            .map(LogRow::text)
            .collect();
        assert!(
            rows[0].starts_with("▸ Run 1 (earlier)") && rows[0].ends_with("3 lines ──"),
            "{rows:?}"
        );
        assert!(rows[1].starts_with("▾ Run 2 (current)"), "{rows:?}");
        assert_eq!(rows[2], "restarted");
        assert_eq!(rows.len(), 3);
    }
    // Stopping a follower that waits on a quiet stream takes no longer than a read.
    let stop = StopFlag::new();
    let (h, s) = (host.clone(), stop.clone());
    let quiet = std::thread::spawn(move || {
        h.follow_logs(
            "web",
            &LogQuery {
                tail: Some(0),
                ..LogQuery::default()
            },
            &s,
            |_| {},
        )
    });
    std::thread::sleep(Duration::from_millis(200));
    let asked = std::time::Instant::now();
    stop.stop();
    assert!(quiet.join().unwrap().is_ok());
    assert!(asked.elapsed() < Duration::from_secs(2));
    let tail = host
        .logs_with(
            "web",
            &LogQuery {
                tail: Some(1),
                ..LogQuery::default()
            },
        )
        .unwrap();
    assert_eq!(tail, "restarted\n");
    assert!(
        engine
            .seen()
            .iter()
            .any(|r| r.contains("/logs?") && r.contains("follow=1") && r.contains("timestamps=1"))
    );
    assert!(matches!(
        host.follow_logs("nope", &LogQuery::default(), &StopFlag::new(), |_| {}),
        Err(DockerError::Http(404, _))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// SPEC-0119 v5 AK-10: details with environment, ports, mounts and labels; secrets masked.
#[test]
fn container_details_are_masked() {
    let dir = scratch("insp");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let d = host.container_details("web").unwrap();
    assert_eq!(d.name, "web");
    assert_eq!(d.state, "running");
    let env: Vec<(String, String, bool)> = d
        .env
        .iter()
        .map(|e| (e.name.clone(), e.value.clone(), e.masked))
        .collect();
    assert_eq!(env[0], ("GREETING".into(), "hello".into(), false));
    assert_eq!(
        env[1],
        (
            "DB_PASSWORD".into(),
            ferris_idea_containers::secrets::MASK.into(),
            true
        )
    );
    assert_eq!(d.ports[0].url.as_deref(), Some("http://localhost:8080"));
    assert_eq!(d.mounts[0].destination, "/data");
    assert!(
        d.labels
            .iter()
            .any(|(k, v)| k == "com.docker.compose.project" && v == "shop")
    );
    assert!(!d.inspect.to_string().contains("fake-placeholder-pw"));
    assert!(matches!(
        host.container_details("nope"),
        Err(DockerError::Http(404, _))
    ));
    assert!(host.container_details("../x").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// SPEC-0119 v6 AK-13: pull with progress, digest and the `latest` warning; a registry's refusal;
/// details with the containers that use an image; removing refused while used, then removed.
#[test]
fn images_pull_details_and_remove() {
    use ferris_idea_containers::StopFlag;
    let dir = scratch("img");
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = DockerHost::Unix(engine.socket.clone());
    let mut progress = Vec::new();
    let done = host
        .pull("busybox:1.36.1", &StopFlag::new(), |l| {
            progress.push(l.to_owned())
        })
        .unwrap();
    assert_eq!(done.reference, "busybox:1.36.1");
    assert_eq!(
        done.digest.as_deref(),
        Some(format!("sha256:{}", "1".repeat(64)).as_str())
    );
    assert_eq!(done.status, "Downloaded newer image for busybox:1.36.1");
    assert_eq!(done.warning, None);
    assert!(
        progress.iter().any(|l| l == "a1b2c3d4e5f6: Pull complete"),
        "{progress:?}"
    );
    assert!(
        engine
            .seen()
            .contains(&"POST /images/create?fromImage=busybox&tag=1.36.1".to_owned())
    );
    assert!(
        host.images()
            .unwrap()
            .iter()
            .any(|i| i.tags == ["busybox:1.36.1"])
    );
    let latest = host.pull("alpine", &StopFlag::new(), |_| {}).unwrap();
    assert_eq!(latest.reference, "alpine:latest");
    assert!(latest.warning.unwrap().contains("has no tag"));
    assert_eq!(
        host.pull("nope:1", &StopFlag::new(), |_| {}),
        Err(DockerError::Http(
            500,
            "pull access denied for nope, repository does not exist".into()
        ))
    );
    assert!(host.pull("a b", &StopFlag::new(), |_| {}).is_err());
    let d = host.image_details("nginx:1.27").unwrap();
    assert_eq!(d.platform, "linux/arm64/v8");
    assert_eq!(d.size, 192_000_000);
    assert_eq!(d.containers, [("web".to_owned(), "running".to_owned())]);
    assert_eq!(
        host.remove_image("nginx:1.27"),
        Err(DockerError::Http(
            409,
            "nginx:1.27 is used by web (running): remove it first".into()
        ))
    );
    assert!(
        !engine
            .seen()
            .iter()
            .any(|r| r.starts_with("DELETE /images/")),
        "a used image is not even asked for"
    );
    let gone = host.remove_image("busybox:1.36.1").unwrap();
    assert_eq!(gone[0], "Untagged: busybox:1.36.1");
    assert!(gone[1].starts_with("Deleted: sha256:"));
    assert!(matches!(
        host.image_details("busybox:1.36.1"),
        Err(DockerError::Http(404, _))
    ));
    assert!(host.remove_image("x?force=1").is_err());
    // Compose projects of other folders carry their labels and network.
    engine.outside_compose(
        "billing",
        "api",
        "billing-api-1",
        "nginx:1.27",
        "/srv/billing",
    );
    let c = host
        .containers()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "billing-api-1")
        .unwrap();
    assert_eq!(c.compose_project.as_deref(), Some("billing"));
    assert_eq!(c.compose_dir.as_deref(), Some("/srv/billing"));
    assert_eq!(c.networks, ["billing_default"]);
    assert_eq!(c.image_id, "cccccccccccc");
    let _ = std::fs::remove_dir_all(dir);
}
