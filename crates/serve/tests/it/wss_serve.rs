//! SPEC-0325 AK-4 and AK-8: `ferris-idea-serve --wss` as a real process, with a certificate generated
//! here and real frames over TLS.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ferris_idea_session::{ClientSession, ErrorCode};
use ferris_idea_web::client::connect;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use serde_json::json;

struct Certs {
    ca: PathBuf,
    cert: PathBuf,
    key: PathBuf,
}

fn certs(dir: &Path) -> Certs {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "test ca");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&ca_params, &ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();
    let c = Certs {
        ca: dir.join("ca.pem"),
        cert: dir.join("cert.pem"),
        key: dir.join("key.pem"),
    };
    std::fs::write(&c.ca, ca_cert.pem()).unwrap();
    std::fs::write(&c.cert, leaf.pem()).unwrap();
    std::fs::write(&c.key, leaf_key.serialize_pem()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&c.key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    c
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Serve {
    child: Child,
    addr: SocketAddr,
    secret: String,
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(project: &Path, state: &Path, c: &Certs, extra: &[&str]) -> Serve {
    let port = free_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferris-idea-serve"))
        .args(["serve", "--wss", "--state-dir"])
        .arg(state)
        .arg("--listen")
        .arg(addr.to_string())
        .arg("--wss-cert")
        .arg(&c.cert)
        .arg("--wss-key")
        .arg(&c.key)
        .args(extra)
        .arg(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut secret = None;
    let t0 = Instant::now();
    let mut line = String::new();
    while secret.is_none() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no start line");
        line.clear();
        if err.read_line(&mut line).unwrap() == 0 {
            panic!("the process ended");
        }
        if let Some(rest) = line.trim().strip_prefix("Web access secret (shown once): ") {
            secret = Some(rest.to_owned());
        }
    }
    // Keep reading standard error so the process never blocks on a full pipe.
    std::thread::spawn(move || {
        let mut sink = String::new();
        while matches!(err.read_line(&mut sink), Ok(n) if n > 0) {
            sink.clear();
        }
    });
    Serve {
        child,
        addr,
        secret: secret.unwrap(),
    }
}

#[test]
fn ak8_a_session_runs_on_the_real_process_with_view_only() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(project.join("a.txt"), "hello\n").unwrap();
    let project = project.canonicalize().unwrap();
    let c = certs(dir.path());
    let serve = start(&project, &state, &c, &[]);
    let origin = format!("https://localhost:{}", serve.addr.port());
    let (transport, answer) =
        connect(serve.addr, "localhost", &c.ca, &origin, &serve.secret).unwrap();
    assert_eq!(answer["as"], "owner");
    let mut session =
        ClientSession::open(transport, &project.display().to_string(), "t", "h", None).unwrap();
    assert_eq!(session.open_doc("a.txt").unwrap().text(), "hello\n");
    let denied = session
        .call("fs.write", json!({"path": "a.txt", "text": "x"}))
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::NotGranted, "{denied}");
    let log = std::fs::read_to_string(state.join("remote/audit.jsonl")).unwrap();
    assert!(!log.contains(&serve.secret), "the secret is not logged");
}

fn exit_of(args: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ferris-idea-serve"))
        .arg("serve")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn ak4_the_start_is_refused_without_what_it_needs() {
    let dir = tempfile::tempdir().unwrap();
    let c = certs(dir.path());
    let (cert, key) = (c.cert.display().to_string(), c.key.display().to_string());
    let project = dir.path().display().to_string();
    let state = dir.path().join("state").display().to_string();
    // No folder.
    let (code, err) = exit_of(&[
        "--wss",
        "--state-dir",
        &state,
        "--wss-cert",
        &cert,
        "--wss-key",
        &key,
    ]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("needs a folder"), "{err}");
    // No certificate.
    let (code, err) = exit_of(&["--wss", "--state-dir", &state, &project]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("--wss-cert"), "{err}");
    // Network address without the switch.
    let (code, err) = exit_of(&[
        "--wss",
        "--state-dir",
        &state,
        "--listen",
        "192.0.2.1:1",
        "--wss-cert",
        &cert,
        "--wss-key",
        &key,
        &project,
    ]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("--allow-network"), "{err}");
    // Network address without an origin.
    let (code, err) = exit_of(&[
        "--wss",
        "--allow-network",
        "--state-dir",
        &state,
        "--listen",
        "192.0.2.1:1",
        "--wss-cert",
        &cert,
        "--wss-key",
        &key,
        &project,
    ]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("--wss-origin"), "{err}");
    // Not an https origin.
    let (code, err) = exit_of(&[
        "--wss",
        "--state-dir",
        &state,
        "--wss-origin",
        "http://a.test",
        "--wss-cert",
        &cert,
        "--wss-key",
        &key,
        &project,
    ]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("https"), "{err}");
}
