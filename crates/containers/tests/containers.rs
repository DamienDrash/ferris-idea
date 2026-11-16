//! SPEC-0119 AK-2 (and SPEC-0240 … SPEC-0243 AK-2) against the stand-ins of
//! `idecraft_containers::fake`: a Docker Engine API on a real Unix socket, `kubectl`, `ssh` and
//! `docker-compose` scripts.
#![cfg(unix)]

use std::path::PathBuf;

use std::time::Duration;

use idecraft_containers::compose::{self, ComposeAction, ComposeCli};
use idecraft_containers::fake::{FakeEngine, compose_script, kubectl_script, ssh_script};
use idecraft_containers::ports::PortForwards;
use idecraft_containers::{
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
    let Some(h) = idecraft_remote::testing::TestHost::start_classical(&dir.join("sshd")) else {
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

/// The Compose CLI starts `ssh` from PATH itself; for an `ssh://` engine it finds IDECraft's
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
