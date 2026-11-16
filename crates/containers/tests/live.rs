//! Live tests against a real Docker Engine and a real Kubernetes cluster (SPEC-0119 AK-6,
//! SPEC-0240 … SPEC-0243 AK-3). They skip unless the variables printed by
//! `containers.sh env` of the reference environment are set and the engine or cluster answers:
//! `IDECRAFT_TEST_DOCKER` (engine address), `IDECRAFT_TEST_DOCKER_SSH` (ssh arguments ending with
//! the destination, optional), `IDECRAFT_TEST_KUBECONFIG` (kubeconfig file).
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use idecraft_containers::compose::{self, ComposeAction, ComposeCli};
use idecraft_containers::ports::{PortForwards, browser_url};
use idecraft_containers::{ContainerAction, DockerHost, ExecOptions, Kubectl, exec};

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("idecraft-live-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn engine() -> Option<DockerHost> {
    let address = std::env::var("IDECRAFT_TEST_DOCKER").ok()?;
    let host = DockerHost::parse(&address, Vec::new())?;
    match host.info() {
        Ok(_) => Some(host),
        Err(e) => {
            eprintln!("skipped: {address} does not answer: {e}");
            None
        }
    }
}

/// The body of `GET /` on 127.0.0.1:`port`, retried for up to `wait`.
fn http_get(host: &str, port: u16, wait: Duration) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Ok(mut s) = std::net::TcpStream::connect((host, port)) {
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

fn state(host: &DockerHost, name: &str) -> Option<String> {
    host.containers()
        .ok()?
        .into_iter()
        .find(|c| c.name == name)
        .map(|c| c.state)
}

#[test]
fn docker_live() {
    let Some(host) = engine() else {
        eprintln!("skipped: IDECRAFT_TEST_DOCKER not set or not reachable");
        return;
    };
    let Some(cli) = ComposeCli::find(std::env::var_os("PATH").as_deref()) else {
        eprintln!("skipped: no Compose CLI (containers.sh tools of the reference environment)");
        return;
    };
    let info = host.info().unwrap();
    assert!(info.platform.starts_with("linux/"), "{info:?}");
    let dir = scratch("compose").join("idecraft-live");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("compose.yaml"),
        "services:\n  web:\n    image: nginx:1.29-alpine\n    ports:\n      - \"127.0.0.1:18080:80\"\n  worker:\n    image: busybox:1.37\n    command: [\"sh\", \"-c\", \"echo worker up; echo worker warn >&2; sleep 3600\"]\n",
    )
    .unwrap();
    let project = compose::detect(&dir).unwrap();
    assert_eq!(project.services, ["web", "worker"]);
    let _ = cli.run(&project, &host, ComposeAction::Down, None);

    // SPEC-0242: up, states, logs of one service.
    cli.run(&project, &host, ComposeAction::Up, None).unwrap();
    let containers = host.containers().unwrap();
    let states = compose::service_states(&project, &containers);
    assert!(states.iter().all(|s| s.state == "running"), "{states:?}");
    let worker = states.iter().find(|s| s.service == "worker").unwrap();
    let started = Instant::now();
    let logs = loop {
        let logs = compose::service_logs(&host, worker, 50).unwrap();
        if logs.contains("worker warn") || started.elapsed() > Duration::from_secs(10) {
            break logs;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(
        logs.contains("worker up") && logs.contains("worker warn"),
        "{logs}"
    );
    let web_name = states
        .iter()
        .find(|s| s.service == "web")
        .unwrap()
        .containers[0]
        .clone();
    let worker_name = worker.containers[0].clone();

    // SPEC-0241: the published port shown answers on the Mac.
    let web = containers.iter().find(|c| c.name == web_name).unwrap();
    assert_eq!(web.ports, ["127.0.0.1:18080->80/tcp"]);
    assert_eq!(
        browser_url(&web.ports[0]).as_deref(),
        Some("http://127.0.0.1:18080")
    );
    let page = http_get("127.0.0.1", 18080, Duration::from_secs(20)).expect("nginx answers");
    assert!(page.contains("200 OK"), "{page}");

    // SPEC-0240: commands and an interactive shell.
    let run = |cmd: &[&str]| {
        host.exec(
            &worker_name,
            &cmd.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            &ExecOptions::default(),
            Duration::from_secs(30),
        )
        .unwrap()
    };
    let out = run(&["uname", "-s"]);
    assert_eq!((out.output.as_str(), out.exit_code), ("Linux\n", Some(0)));
    assert_eq!(run(&["sh", "-c", "exit 3"]).exit_code, Some(3));
    let out = run(&["sh", "-c", "echo out; sleep 0.2; echo err >&2"]);
    assert_eq!(out.output, "out\nerr\n");
    let out = host
        .exec(
            &worker_name,
            &[
                "sh".into(),
                "-c".into(),
                "pwd; echo $GREETING; id -u".into(),
            ],
            &ExecOptions {
                workdir: Some("/tmp".into()),
                env: vec!["GREETING=hi".into()],
                user: Some("65534".into()),
            },
            Duration::from_secs(30),
        )
        .unwrap();
    assert_eq!(out.output, "/tmp\nhi\n65534\n");
    let mut shown = Vec::new();
    let input = std::io::Cursor::new(b"echo hi-$((40+2))\rexit 4\r".to_vec());
    let code = exec::bridge(&host, &web_name, input, &mut shown, || Some((30, 100))).unwrap();
    let shown = String::from_utf8_lossy(&shown);
    assert!(shown.contains("hi-42"), "{shown}");
    assert_eq!(code, 4);

    // SPEC-0119: stop, start, restart.
    host.act(&worker_name, ContainerAction::Stop).unwrap();
    assert_eq!(state(&host, &worker_name).as_deref(), Some("exited"));
    host.act(&worker_name, ContainerAction::Stop).unwrap();
    host.act(&worker_name, ContainerAction::Start).unwrap();
    host.act(&worker_name, ContainerAction::Restart).unwrap();
    assert_eq!(state(&host, &worker_name).as_deref(), Some("running"));

    // SPEC-0243: the same engine over SSH.
    if let Ok(words) = std::env::var("IDECRAFT_TEST_DOCKER_SSH") {
        let mut words: Vec<String> = words.split_whitespace().map(str::to_owned).collect();
        let destination = words.pop().unwrap();
        let ssh = DockerHost::Ssh {
            destination,
            port: None,
            extra: words,
            program: None,
        };
        assert_eq!(ssh.info().unwrap().version, info.version);
        let mut a: Vec<String> = host
            .containers()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        let mut b: Vec<String> = ssh
            .containers()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        assert!(ssh.logs(&worker_name, 5).unwrap().contains("worker up"));
        let out = ssh
            .exec(
                &worker_name,
                &["echo".into(), "over ssh".into()],
                &ExecOptions::default(),
                Duration::from_secs(30),
            )
            .unwrap();
        assert_eq!(out.output, "over ssh\n");
    } else {
        eprintln!("ssh part skipped: IDECRAFT_TEST_DOCKER_SSH not set");
    }

    // SPEC-0242: remove one service, then down; volumes are never removed.
    cli.run(&project, &host, ComposeAction::Down, Some("worker"))
        .unwrap();
    assert_eq!(state(&host, &worker_name), None);
    cli.run(&project, &host, ComposeAction::Down, None).unwrap();
    assert_eq!(state(&host, &web_name), None);
    host.act(&web_name, ContainerAction::Remove).unwrap_err();
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// A `kubectl` that uses the test kubeconfig.
fn kubectl() -> Option<Kubectl> {
    let config = std::env::var("IDECRAFT_TEST_KUBECONFIG").ok()?;
    let real = Kubectl::find(std::env::var_os("PATH").as_deref())?;
    let dir = scratch("kubectl");
    let path = dir.join("kubectl");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nKUBECONFIG='{config}' exec '{}' \"$@\"\n",
            real.program.display()
        ),
    )
    .ok()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).ok()?;
    let k = Kubectl { program: path };
    match k.contexts() {
        Ok(c) if !c.is_empty() => Some(k),
        other => {
            eprintln!("skipped: no cluster: {other:?}");
            None
        }
    }
}

fn kube_run(k: &Kubectl, args: &[&str], input: Option<&str>) -> String {
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
    assert!(
        out.status.success(),
        "kubectl {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const MANIFEST: &str = r#"
apiVersion: v1
kind: Namespace
metadata: {name: idecraft-live}
---
apiVersion: apps/v1
kind: Deployment
metadata: {name: web, namespace: idecraft-live}
spec:
  replicas: 1
  selector: {matchLabels: {app: web}}
  template:
    metadata: {labels: {app: web}}
    spec:
      containers:
        - name: nginx
          image: nginx:1.29-alpine
          ports: [{containerPort: 80}]
---
apiVersion: v1
kind: Service
metadata: {name: web, namespace: idecraft-live}
spec:
  selector: {app: web}
  ports: [{port: 80, targetPort: 80}]
"#;

#[test]
fn kubernetes_live() {
    let Some(k) = kubectl() else {
        eprintln!("skipped: IDECRAFT_TEST_KUBECONFIG not set or cluster not reachable");
        return;
    };
    let context = k
        .contexts()
        .unwrap()
        .into_iter()
        .find(|c| c.current)
        .unwrap()
        .name;
    kube_run(&k, &["apply", "-f", "-"], Some(MANIFEST));
    kube_run(
        &k,
        &[
            "rollout",
            "status",
            "deployment/web",
            "-n",
            "idecraft-live",
            "--timeout=180s",
        ],
        None,
    );
    let ns = "idecraft-live";
    assert!(k.namespaces(&context).unwrap().contains(&ns.to_owned()));
    let resources = k.resources(&context, ns).unwrap();
    let dep = resources.iter().find(|r| r.kind == "Deployment").unwrap();
    assert_eq!(
        (dep.status.as_str(), dep.ports.as_slice()),
        ("Available", &[80u16][..])
    );
    let pod = resources.iter().find(|r| r.kind == "Pod").unwrap();
    assert_eq!(pod.status, "Running");
    let svc = resources.iter().find(|r| r.kind == "Service").unwrap();
    assert_eq!(svc.ports, [80]);
    k.logs(&context, ns, "deployment/web", 20).unwrap();

    // SPEC-0240: a command in the deployment's pod.
    let object = idecraft_containers::exec_object("Deployment", "web").unwrap();
    let args = k
        .exec_args(&context, ns, &object, None, &["hostname".into()], false)
        .unwrap();
    let out = k.exec(&args, Duration::from_secs(30)).unwrap();
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.output.trim(), pod.name);
    let args = k
        .exec_args(
            &context,
            ns,
            &pod.name,
            Some("nginx"),
            &["sh".into(), "-c".into(), "exit 5".into()],
            false,
        )
        .unwrap();
    assert_eq!(
        k.exec(&args, Duration::from_secs(30)).unwrap().exit_code,
        Some(5)
    );

    // SPEC-0241: forward the service, read the page, stop.
    let forwards = PortForwards::default();
    let f = forwards
        .start(&k, &context, ns, "Service", "web", 80, None)
        .unwrap();
    let page =
        http_get("127.0.0.1", f.local_port, Duration::from_secs(10)).expect("forward answers");
    assert!(page.contains("200 OK"), "{page}");
    assert_eq!(forwards.list()[0].target, "service/web");
    assert!(forwards.stop(f.id));
    std::thread::sleep(Duration::from_millis(300));
    assert!(std::net::TcpStream::connect(("127.0.0.1", f.local_port)).is_err());

    // SPEC-0119: rollout restart.
    let out = k.restart(&context, ns, "Deployment", "web").unwrap();
    assert!(out.contains("restarted"), "{out}");
    kube_run(&k, &["delete", "namespace", ns, "--wait=false"], None);
}
