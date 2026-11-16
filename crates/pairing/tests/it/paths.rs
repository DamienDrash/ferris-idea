//! Remote control end to end (`SPEC-0324` AK-1, AK-3–AK-8): real TCP, real Noise, a real backend.
//! AK-1 binds a non-loopback address of this machine when it has one.
#![cfg(unix)]

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use idecraft_pairing::{
    ChildPipe, Console, Identity, Limits, ListenOptions, PairError, Side, Store, Timing,
    connect_known, connect_known_on, fingerprint, listen, parse_link, start_pairing,
    start_pairing_on, start_pairing_with_link,
};
use idecraft_session::{Backend, BackendConfig, ClientSession, Connection, ErrorCode, Right};
use serde_json::{Value, json};

struct TestConsole {
    grant: Vec<Right>,
    lines: Mutex<Vec<String>>,
}

impl Console for TestConsole {
    fn show(&self, line: &str) {
        self.lines.lock().unwrap().push(line.to_owned());
    }
    fn confirm(&self, _: &str, _: &str, digits: &str) -> bool {
        self.show(&format!("digits {digits}"));
        true
    }
    fn rights(&self, _: &str, _: &[Right]) -> Vec<Right> {
        self.grant.clone()
    }
}

struct Host {
    _dir: tempfile::TempDir,
    project: std::path::PathBuf,
    state: std::path::PathBuf,
    console: Arc<TestConsole>,
    running: idecraft_pairing::Running,
    backend: Backend,
    addr: SocketAddr,
}

fn scratch() -> (tempfile::TempDir, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let p = t.path().canonicalize().unwrap();
    (t, p)
}

struct Setup {
    addr: SocketAddr,
    allow_network: bool,
    qr: bool,
    fixed_folder: bool,
    grant: Vec<Right>,
}

fn timing() -> Timing {
    Timing {
        poll: Duration::from_millis(50),
        ..Timing::default()
    }
}

fn host(setup: Setup) -> Result<Host, idecraft_pairing::ListenError> {
    let (dir, root) = scratch();
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("a.txt"), "secret content\n").unwrap();
    let state = root.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_file(Side::Serve, &state.join("remote")).unwrap();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(state.clone()),
        folder: setup.fixed_folder.then(|| project.clone()),
        peer: "listener".into(),
        ..BackendConfig::default()
    });
    let console = Arc::new(TestConsole {
        grant: setup.grant,
        lines: Mutex::new(Vec::new()),
    });
    let running = listen(
        ListenOptions {
            addr: setup.addr,
            allow_network: setup.allow_network,
            new_device: false,
            qr: setup.qr,
            timing: timing(),
            limits: Limits::default(),
        },
        store,
        backend.clone(),
        console.clone(),
    )?;
    let addr = running.addr;
    Ok(Host {
        _dir: dir,
        project,
        state,
        console,
        running,
        backend,
        addr,
    })
}

fn loopback(grant: Vec<Right>) -> Host {
    host(Setup {
        addr: "127.0.0.1:0".parse().unwrap(),
        allow_network: false,
        qr: false,
        fixed_folder: true,
        grant,
    })
    .unwrap()
}

fn code_of(h: &Host) -> String {
    let lines = h.console.lines.lock().unwrap();
    let line = lines.iter().find(|l| l.contains(" code ")).unwrap();
    line.rsplit(" code ")
        .next()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

fn window_store() -> (tempfile::TempDir, Store, Identity) {
    let (t, root) = scratch();
    let store = Store::open_file(Side::Window, &root.join("remote")).unwrap();
    let id = store.identity().unwrap();
    (t, store, id)
}

fn wait(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    f()
}

fn serve_store(h: &Host) -> Store {
    Store::open_file(Side::Serve, &h.state.join("remote")).unwrap()
}

fn pair(h: &Host, store: &Store, id: &Identity) {
    let p = start_pairing(&h.addr.to_string(), &code_of(h), id, "laptop").unwrap();
    p.confirm(store).unwrap();
}

fn open(h: &Host, store: &Store, id: &Identity) -> ClientSession {
    let t = connect_known(&h.addr.to_string(), store, id, "laptop").unwrap();
    ClientSession::open(t, h.project.to_str().unwrap(), "test", "paired", None).unwrap()
}

/// A local address that is not loopback, without sending anything.
fn lan_address() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

#[test]
fn ak1_a_non_loopback_address_pairs_edits_and_resumes() {
    let Some(ip) = lan_address() else {
        eprintln!("no non-loopback address; skipped");
        return;
    };
    let addr = SocketAddr::new(ip, 0);
    // Refused without the switch.
    assert!(matches!(
        host(Setup {
            addr,
            allow_network: false,
            qr: false,
            fixed_folder: true,
            grant: vec![Right::View],
        }),
        Err(idecraft_pairing::ListenError::NotLoopback(_))
    ));
    let h = host(Setup {
        addr,
        allow_network: true,
        qr: false,
        fixed_folder: true,
        grant: vec![Right::View, Right::Edit],
    })
    .unwrap();
    assert_eq!(h.addr.ip(), ip);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let mut s = open(&h, &store, &id);
    assert_eq!(s.info.rights, ["view", "edit"]);
    s.open_doc("a.txt").unwrap();
    s.edit(
        "a.txt",
        &[idecraft_session::Op {
            at: 0,
            delete: 0,
            insert: "lan ".into(),
        }],
    )
    .unwrap();
    assert!(s.settle(Duration::from_secs(5)));
    s.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(h.project.join("a.txt")).unwrap(),
        "lan secret content\n"
    );
    s.drop_connection();
    let t = connect_known(&h.addr.to_string(), &store, &id, "laptop").unwrap();
    s.resume(t).unwrap();
    assert!(s.connected());
}

#[test]
fn ak2_a_device_needs_a_fixed_folder() {
    let h = host(Setup {
        addr: "127.0.0.1:0".parse().unwrap(),
        allow_network: false,
        qr: false,
        fixed_folder: false,
        grant: vec![Right::View],
    })
    .unwrap();
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let t = connect_known(&h.addr.to_string(), &store, &id, "laptop").unwrap();
    let (c, _rx) = Connection::start(t);
    let e = c
        .call(
            "session.hello",
            json!({"protocol": "idecraft-session", "majors": [1], "build": "t", "client": "window",
                   "project": h.project.to_str().unwrap()}),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
}

#[test]
fn ak3_a_qr_link_pairs_and_a_wrong_key_is_refused() {
    let setup = |qr| Setup {
        addr: "127.0.0.1:0".parse().unwrap(),
        allow_network: false,
        qr,
        fixed_folder: true,
        grant: vec![Right::View],
    };
    let h = host(setup(true)).unwrap();
    let link = {
        let lines = h.console.lines.lock().unwrap();
        let line = lines
            .iter()
            .find(|l| l.contains("idecraft-pair://"))
            .unwrap();
        line.rsplit("Pairing link: ").next().unwrap().to_owned()
    };
    assert!(
        !h.console
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains(" code ")),
        "a QR window shows no short code"
    );
    let parsed = parse_link(&link).unwrap();

    // A link naming another key is refused before digits, and nothing is stored.
    let (_t, store, id) = window_store();
    let mut wrong = parsed.clone();
    wrong.key = [9u8; 32];
    let err = match start_pairing_with_link(&wrong, &id, "laptop") {
        Err(e) => e,
        Ok(_) => panic!("a wrong key must be refused"),
    };
    assert!(matches!(err, PairError::WrongKey(_)), "{err}");
    assert!(store.hosts().unwrap().is_empty());

    let h2 = host(setup(true)).unwrap();
    let link2 = {
        let lines = h2.console.lines.lock().unwrap();
        lines
            .iter()
            .find(|l| l.contains("idecraft-pair://"))
            .unwrap()
            .rsplit("Pairing link: ")
            .next()
            .unwrap()
            .to_owned()
    };
    let parsed2 = parse_link(&link2).unwrap();
    let pending = start_pairing_with_link(&parsed2, &id, "laptop").unwrap();
    assert_eq!(pending.digits().len(), 6);
    pending.confirm(&store).unwrap();
    assert_eq!(store.hosts().unwrap().len(), 1);
}

#[test]
fn ak4_the_channel_runs_over_a_child_pipe() {
    if Command::new("nc").arg("-h").output().is_err() {
        eprintln!("no nc; skipped");
        return;
    }
    let pipe = |port: u16| {
        let child = Command::new("nc")
            .args(["127.0.0.1", &port.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        ChildPipe::new(child).unwrap()
    };
    let h = loopback(vec![Right::View]);
    let (_t, store, id) = window_store();
    let address = h.addr.to_string();
    let p = start_pairing_on(
        pipe(h.addr.port()),
        &address,
        &code_of(&h),
        None,
        &id,
        "piped",
    )
    .unwrap();
    p.confirm(&store).unwrap();
    let pinned = store.host_key(&address).unwrap().unwrap();
    let t = connect_known_on(pipe(h.addr.port()), &address, &pinned, &id, "piped").unwrap();
    let mut s = ClientSession::open(t, h.project.to_str().unwrap(), "t", "piped", None).unwrap();
    let doc = s.open_doc("a.txt").unwrap();
    assert!(doc.text().contains("secret content"));
    let _ = s.bye();
}

fn device_call(s: &ClientSession, method: &str, params: Value) -> Result<Value, ErrorCode> {
    s.call(method, params).map_err(|e| e.code)
}

#[test]
fn ak5_widening_applies_live_and_narrowing_ends_the_session() {
    let h = loopback(vec![Right::View, Right::Edit]);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let fp = fingerprint(&id.public);
    let s = open(&h, &store, &id);

    // A widening reaches the live session without a reconnect.
    assert!(
        serve_store(&h)
            .set_rights(&fp, &["view".into(), "edit".into(), "run".into()])
            .unwrap()
    );
    assert!(wait(Duration::from_secs(3), || {
        device_call(&s, "device.whoami", json!({})).is_ok_and(|v| {
            v["rights"]
                .as_array()
                .is_some_and(|a| a.iter().any(|r| r == "run"))
        })
    }));
    assert!(s.connected());

    // A narrowing ends it within a second.
    serve_store(&h).set_rights(&fp, &["view".into()]).unwrap();
    let t0 = Instant::now();
    assert!(wait(Duration::from_secs(3), || !s.connected()));
    assert!(
        t0.elapsed() < Duration::from_millis(1500),
        "{:?}",
        t0.elapsed()
    );
    let audit = std::fs::read_to_string(h.state.join("remote/audit.jsonl")).unwrap();
    assert!(audit.contains("rights-reduced"), "{audit}");

    let s2 = open(&h, &store, &id);
    assert_eq!(s2.info.rights, ["view"]);
}

#[test]
fn ak6_every_device_request_is_logged() {
    let h = loopback(vec![Right::View]);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let fp = fingerprint(&id.public);
    let t = connect_known(&h.addr.to_string(), &store, &id, "laptop").unwrap();
    let (c, _rx) = Connection::start(t);
    c.call(
        "session.hello",
        json!({"protocol": "idecraft-session", "majors": [1], "build": "t", "client": "window",
               "project": h.project.to_str().unwrap()}),
        Duration::from_secs(5),
    )
    .unwrap();
    c.call("fs.read", json!({"path": "a.txt"}), Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        c.call(
            "fs.write",
            json!({"path": "a.txt", "text": "x"}),
            Duration::from_secs(5)
        )
        .unwrap_err()
        .code,
        ErrorCode::NotGranted
    );
    c.call("session.ping", json!({}), Duration::from_secs(5))
        .unwrap();
    c.call("doc.open", json!({"path": "a.txt"}), Duration::from_secs(5))
        .unwrap();

    let lines = idecraft_pairing::audit::read_log(&h.state, Some(&fp), 100);
    let methods: Vec<&str> = lines.iter().filter_map(|l| l["method"].as_str()).collect();
    assert!(methods.contains(&"fs.read"), "{methods:?}");
    assert!(methods.contains(&"fs.write"), "{methods:?}");
    assert!(methods.contains(&"doc.open"), "{methods:?}");
    assert!(!methods.contains(&"session.ping"), "{methods:?}");
    assert!(lines.iter().any(|l| l["outcome"] == "not_granted"));
    assert!(
        lines
            .iter()
            .all(|l| l["peer"].as_str().unwrap().contains(&fp))
    );
    let log = std::fs::read_to_string(h.state.join("remote/audit.jsonl")).unwrap();
    assert!(!log.contains("secret content"), "no content in the log");
}

#[test]
fn ak6_doc_edit_is_coalesced() {
    let h = loopback(vec![Right::View, Right::Edit]);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let mut s = open(&h, &store, &id);
    s.open_doc("a.txt").unwrap();
    for _ in 0..5 {
        s.edit(
            "a.txt",
            &[idecraft_session::Op {
                at: 0,
                delete: 0,
                insert: "x".into(),
            }],
        )
        .unwrap();
    }
    assert!(s.settle(Duration::from_secs(5)));
    let log = std::fs::read_to_string(h.state.join("remote/audit.jsonl")).unwrap();
    assert_eq!(log.matches("\"doc.edit\"").count(), 1, "{log}");
}

#[test]
fn ak7_access_off_ends_sessions_and_the_listener() {
    let h = loopback(vec![Right::View]);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let s = open(&h, &store, &id);
    serve_store(&h).set_access_off(true).unwrap();
    let t0 = Instant::now();
    assert!(wait(Duration::from_secs(3), || !s.connected()));
    assert!(wait(Duration::from_secs(3), || h.running.stopped()));
    assert!(
        t0.elapsed() < Duration::from_millis(1500),
        "{:?}",
        t0.elapsed()
    );
    let err = connect_known(&h.addr.to_string(), &store, &id, "laptop");
    assert!(err.is_err(), "a later connection is refused");
}

#[test]
fn ak8_whoami_and_notices() {
    let h = loopback(vec![Right::View]);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let s = open(&h, &store, &id);
    let who = device_call(&s, "device.whoami", json!({})).unwrap();
    assert_eq!(who["name"], "laptop");
    assert_eq!(who["fingerprint"], fingerprint(&id.public));
    assert_eq!(who["rights"], json!(["view"]));

    let paired = device_call(&s, "notice.list", json!({})).unwrap();
    assert!(
        paired["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "device"),
        "{paired}"
    );
    let last = paired["notices"].as_array().unwrap().last().unwrap()["seq"]
        .as_u64()
        .unwrap();

    h.backend.post_notice("run", "tests finished", None);
    let newer = device_call(&s, "notice.list", json!({"since": last})).unwrap();
    let list = newer["notices"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["title"], "tests finished");
    assert_eq!(list[0]["seq"].as_u64().unwrap(), last + 1);
}
