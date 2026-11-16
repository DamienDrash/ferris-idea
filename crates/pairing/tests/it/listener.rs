//! The listener of `idecraft serve --pair` end to end over loopback (`SPEC-0321` AK-5–AK-14,
//! AK-16): real TCP, real Noise, a real session backend.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use idecraft_pairing::{
    Console, Identity, Limits, ListenError, ListenOptions, PairError, Side, Store, Timing,
    connect_known, fingerprint, listen, start_pairing,
};
use idecraft_session::{Backend, BackendConfig, ClientSession, Connection, ErrorCode, Right};
use serde_json::json;

/// A console that answers as told and records everything shown, so a test can find the code.
struct TestConsole {
    confirm: bool,
    grant: Vec<Right>,
    /// How long the console user takes to pick the rights.
    slow_rights: Duration,
    /// How long the console user takes to compare the digits.
    slow_confirm: Mutex<Duration>,
    lines: Mutex<Vec<String>>,
}

impl Console for TestConsole {
    fn show(&self, line: &str) {
        self.lines.lock().unwrap().push(line.to_owned());
    }
    fn confirm(&self, _: &str, _: &str, digits: &str) -> bool {
        self.show(&format!("digits {digits}"));
        std::thread::sleep(*self.slow_confirm.lock().unwrap());
        self.confirm
    }
    fn rights(&self, _: &str, _: &[Right]) -> Vec<Right> {
        std::thread::sleep(self.slow_rights);
        self.grant.clone()
    }
}

struct Host {
    _dir: tempfile::TempDir,
    project: std::path::PathBuf,
    state: std::path::PathBuf,
    console: Arc<TestConsole>,
    running: idecraft_pairing::Running,
    addr: SocketAddr,
}

fn scratch() -> (tempfile::TempDir, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let p = t.path().canonicalize().unwrap();
    (t, p)
}

fn start(confirm: bool, grant: Vec<Right>, timing: Timing, new_device: bool) -> Host {
    start_slow(confirm, grant, timing, new_device, Duration::ZERO)
}

fn start_slow(
    confirm: bool,
    grant: Vec<Right>,
    timing: Timing,
    new_device: bool,
    slow_rights: Duration,
) -> Host {
    let (dir, root) = scratch();
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("a.txt"), "hello\n").unwrap();
    let state = root.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_file(Side::Serve, &state.join("remote")).unwrap();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(state.clone()),
        folder: Some(project.clone()),
        peer: "listener".into(),
        ..BackendConfig::default()
    });
    let console = Arc::new(TestConsole {
        confirm,
        grant,
        slow_rights,
        slow_confirm: Mutex::new(Duration::ZERO),
        lines: Mutex::new(Vec::new()),
    });
    let running = listen(
        ListenOptions {
            addr: "127.0.0.1:0".parse().unwrap(),
            allow_network: false,
            qr: false,
            new_device,
            timing,
            limits: Limits::default(),
        },
        store,
        backend,
        console.clone(),
    )
    .unwrap();
    let addr = running.addr;
    Host {
        _dir: dir,
        project,
        state,
        console,
        running,
        addr,
    }
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
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

fn pair(h: &Host, store: &Store, id: &Identity) -> String {
    let p = start_pairing(&h.addr.to_string(), &code_of(h), id, "laptop").unwrap();
    assert_eq!(p.digits().len(), 6);
    p.confirm(store).unwrap()
}

fn open(h: &Host, store: &Store, id: &Identity) -> ClientSession {
    let t = connect_known(&h.addr.to_string(), store, id, "laptop").unwrap();
    ClientSession::open(t, h.project.to_str().unwrap(), "test", "paired", None).unwrap()
}

#[test]
fn ak16_pair_open_edit_save_and_resume() {
    let h = start(
        true,
        vec![Right::View, Right::Edit],
        Timing::default(),
        false,
    );
    let (_t, store, id) = window_store();
    let fp = pair(&h, &store, &id);
    assert_eq!(fp.len(), 16);
    assert_eq!(store.hosts().unwrap().len(), 1);
    // Host and window agree on the digits.
    assert!(!h.running.pairing_open());

    let mut s = open(&h, &store, &id);
    assert_eq!(s.info.rights, ["view", "edit"]);
    s.open_doc("a.txt").unwrap();
    s.edit(
        "a.txt",
        &[idecraft_session::Op {
            at: 0,
            delete: 0,
            insert: "new ".into(),
        }],
    )
    .unwrap();
    assert!(s.settle(Duration::from_secs(5)));
    s.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(h.project.join("a.txt")).unwrap(),
        "new hello\n"
    );

    // A broken channel resumes through a new handshake on the same backend.
    s.drop_connection();
    let t = connect_known(&h.addr.to_string(), &store, &id, "laptop").unwrap();
    s.resume(t).unwrap();
    assert!(s.connected());
    // The audit log names the device, not the listener.
    let audit = std::fs::read_to_string(h.state.join("remote/audit.jsonl")).unwrap_or_default();
    assert!(audit.contains(&format!("device:{}", fingerprint(&id.public))));
}

#[test]
fn ak8_sec4_a_view_only_device_gets_not_granted() {
    let h = start(true, vec![Right::View], Timing::default(), false);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let t = connect_known(&h.addr.to_string(), &store, &id, "laptop").unwrap();
    let (c, _rx) = Connection::start(t);
    let hello = c
        .call(
            "session.hello",
            json!({"protocol": "idecraft-session", "majors": [1], "build": "t", "client": "window",
                   "project": h.project.to_str().unwrap()}),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(hello["rights"], json!(["view"]));
    // Reading works; writing does not and leaves the file alone.
    assert!(
        c.call("fs.list", json!({"path": "."}), Duration::from_secs(5))
            .is_ok()
    );
    let e = c
        .call(
            "fs.write",
            json!({"path": "a.txt", "text": "stolen"}),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted);
    assert_eq!(
        std::fs::read_to_string(h.project.join("a.txt")).unwrap(),
        "hello\n"
    );
}

/// The window pins the host only after the host has stored the device with its rights, so a
/// connection made right after pairing, with no retry, is accepted even when the host's user
/// takes a while to pick the rights (`SPEC-0321` behaviour 17, AK-21).
#[test]
fn ak21_a_connection_right_after_pairing_is_accepted() {
    let slow = Duration::from_millis(1500);
    let h = start_slow(
        true,
        vec![Right::View, Right::Edit],
        Timing::default(),
        false,
        slow,
    );
    let (_t, store, id) = window_store();
    let t0 = Instant::now();
    pair(&h, &store, &id);
    // Confirming waited for the rights prompt instead of returning early.
    assert!(t0.elapsed() >= slow, "{:?}", t0.elapsed());
    let s = open(&h, &store, &id);
    assert_eq!(s.info.rights, ["view", "edit"]);
}

/// A host that ends the pairing after both confirmed but before saying it stored the device (an
/// older host, or one that failed) leaves nothing pinned on the window.
#[test]
fn ak21_no_pin_without_the_hosts_stored_message() {
    use idecraft_pairing::noise::{Accept, pairing_key, prologue, respond};
    let (_d, root) = scratch();
    let host_store = Store::open_file(Side::Serve, &root.join("host")).unwrap();
    let host_id = host_store.identity().unwrap();
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let code = "ABCDEFGHJK";
    let host = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let key = pairing_key(code);
        let pinned = |_: &[u8; 32]| false;
        let accept = Accept {
            pairing: Some(&*key),
            pinned: &pinned,
        };
        let (done, _) =
            respond(&mut stream, &host_id, &accept, &prologue("window", "serve")).unwrap();
        let mut channel =
            idecraft_pairing::Channel::new(stream, done.transport, Limits::default()).unwrap();
        channel.write_line("ok").unwrap();
        let _ = channel.read_line(32);
        // An older host stops here.
        channel.close();
    });
    let (_t, store, id) = window_store();
    let p = start_pairing(&addr.to_string(), code, &id, "laptop").unwrap();
    let e = p.confirm(&store).expect_err("not pinned");
    assert!(e.to_string().contains("did not confirm"), "{e}");
    assert!(store.hosts().unwrap().is_empty());
    host.join().unwrap();
}

#[test]
fn ak5_sec1_unpaired_and_changed_keys_are_refused() {
    // The refused attempts below start a backoff for this address; it is not what this test
    // checks, and under load a late-recorded failure let the backoff refuse the next attempt
    // before its handshake (a refusal without a byte instead of the changed key).
    let timing = Timing {
        backoff: Duration::from_millis(1),
        backoff_cap: Duration::from_millis(1),
        ..Timing::default()
    };
    let h = start(true, vec![Right::View], timing, false);
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);

    // A key that was never paired.
    let (_t2, other_store, other_id) = window_store();
    other_store
        .add_host(
            &h.addr.to_string(),
            &store
                .host_key(&store.hosts().unwrap()[0].address)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    let t = connect_known(&h.addr.to_string(), &other_store, &other_id, "other");
    // The handshake may pass the Noise checks on the window's side; the listener closes it.
    if let Ok(t) = t {
        let (c, _rx) = Connection::start(t);
        assert!(
            c.call(
                "session.hello",
                json!({"protocol": "idecraft-session", "majors": [1], "project": "x"}),
                Duration::from_secs(5)
            )
            .is_err()
        );
    }
    assert!(wait(Duration::from_secs(3), || {
        h.console
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("not paired"))
    }));

    // A window pinned to another key: named as a changed key.
    let (_t3, wrong_store, wrong_id) = window_store();
    wrong_store
        .add_host(&h.addr.to_string(), &Identity::generate().unwrap().public)
        .unwrap();
    let e = connect_known(&h.addr.to_string(), &wrong_store, &wrong_id, "x")
        .err()
        .expect("refused");
    assert!(matches!(e, PairError::ChangedKey(_)), "{e}");
    assert!(e.to_string().contains("does not match"));
}

#[test]
fn ak6_sec5_plain_protocols_get_no_reply() {
    let h = start(true, vec![Right::View], Timing::default(), false);
    for probe in [
        &b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session.hello\"}\n"[..],
        b"GET / HTTP/1.1\r\nHost: x\r\n\r\n",
        b"SSH-2.0-OpenSSH_9.9\r\n",
    ] {
        let mut s = TcpStream::connect(h.addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(probe).unwrap();
        let mut back = Vec::new();
        let _ = s.read_to_end(&mut back);
        assert!(back.is_empty(), "replied to a plain probe");
    }
}

#[test]
fn ak11_sec9_rate_limit_and_stalled_handshake() {
    let timing = Timing {
        handshake: Duration::from_millis(300),
        attempts_per_minute: 3,
        backoff: Duration::from_millis(50),
        ..Timing::default()
    };
    let h = start(true, vec![Right::View], timing, false);
    // A stalled handshake is closed after the limit.
    let mut stalled = TcpStream::connect(h.addr).unwrap();
    let t0 = Instant::now();
    stalled
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut b = [0u8; 8];
    let n = stalled.read(&mut b).unwrap_or(0);
    assert_eq!(n, 0);
    assert!(t0.elapsed() < Duration::from_secs(3));
    // Further attempts within the minute are refused without a byte.
    std::thread::sleep(Duration::from_millis(100));
    let mut refused = 0;
    for _ in 0..6 {
        let mut s = TcpStream::connect(h.addr).unwrap();
        // A refused connection is closed without a byte as soon as it is accepted; under load
        // that took longer than the 400 ms this test used to wait.
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = s.write_all(&[0xff; 4]);
        let mut b = [0u8; 8];
        if matches!(s.read(&mut b), Ok(0)) {
            refused += 1;
        }
    }
    assert!(refused >= 1, "no attempt was refused");
}

#[test]
fn ak11_three_wrong_codes_close_the_pairing_window() {
    let timing = Timing {
        backoff: Duration::from_millis(5),
        backoff_cap: Duration::from_millis(5),
        ..Timing::default()
    };
    let h = start(true, vec![Right::View], timing, false);
    let (_t, _store, id) = window_store();
    // The host finds a wrong code only in the third handshake message, after the window's side
    // is done; an attempt that comes while the host still checks the last one is not a pairing
    // attempt (one device at a time). So each attempt waits until the host has refused it; a
    // fixed 40 ms was too short under load.
    let refused = |h: &Host| {
        h.console
            .lines
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("refused a connection"))
            .count()
    };
    for n in 1..=3 {
        let _ = start_pairing(&h.addr.to_string(), "WRONGCODE0", &id, "x");
        assert!(wait(Duration::from_secs(10), || refused(&h) >= n));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(wait(Duration::from_secs(2), || !h.running.pairing_open()));
    // The right code no longer works.
    let code = code_of(&h);
    std::thread::sleep(Duration::from_millis(40));
    assert!(start_pairing(&h.addr.to_string(), &code, &id, "x").is_err());
}

#[test]
fn ak11_an_expired_pairing_window_refuses() {
    let timing = Timing {
        pairing_window: Duration::from_millis(50),
        backoff: Duration::from_millis(1),
        ..Timing::default()
    };
    let h = start(true, vec![Right::View], timing, false);
    std::thread::sleep(Duration::from_millis(120));
    let (_t, _s, id) = window_store();
    assert!(start_pairing(&h.addr.to_string(), &code_of(&h), &id, "x").is_err());
}

#[test]
fn ak13_sec13_a_rejection_stores_nothing_on_either_side() {
    // The host's user declines.
    let h = start(false, vec![Right::View], Timing::default(), false);
    let (_t, store, id) = window_store();
    let p = start_pairing(&h.addr.to_string(), &code_of(&h), &id, "laptop").unwrap();
    let answer = p.confirm(&store);
    assert!(matches!(answer, Err(PairError::Declined)), "{answer:?}");
    assert!(store.hosts().unwrap().is_empty());
    assert!(
        !std::fs::read_to_string(h.state.join("remote/paired-keys.toml"))
            .unwrap_or_default()
            .contains("laptop")
    );

    // The window's user declines.
    let h = start(true, vec![Right::View], Timing::default(), false);
    let (_t, store, id) = window_store();
    let p = start_pairing(&h.addr.to_string(), &code_of(&h), &id, "laptop").unwrap();
    p.decline();
    assert!(
        wait(Duration::from_secs(3), || !h.running.pairing_open()),
        "{:?}",
        h.console.lines.lock().unwrap()
    );
    assert!(store.hosts().unwrap().is_empty());
    assert!(
        !std::fs::read_to_string(h.state.join("remote/paired-keys.toml"))
            .unwrap_or_default()
            .contains("laptop")
    );
}

/// SPEC-0321 v5 AK-13 (behaviour 17): the host declined and closed before the window's user
/// confirmed; the window still reads the host's `no` and reports the rejection, not a broken
/// connection.
#[test]
fn ak13_a_host_that_declined_first_is_reported_as_declined() {
    let h = start(false, vec![Right::View], Timing::default(), false);
    let (_t, store, id) = window_store();
    let p = start_pairing(&h.addr.to_string(), &code_of(&h), &id, "laptop").unwrap();
    // The host's `no` and its close come first.
    assert!(wait(Duration::from_secs(3), || {
        h.console
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("pairing declined"))
    }));
    std::thread::sleep(Duration::from_millis(200));
    let answer = p.confirm(&store);
    assert!(matches!(answer, Err(PairError::Declined)), "{answer:?}");
    assert!(store.hosts().unwrap().is_empty());
    assert!(!h.running.pairing_open());
}

/// SPEC-0321 v5 AK-13 (behaviour 17): the window declined and closed before the host's user
/// confirmed; the host's `ok` cannot be written, and the pairing window still closes and nothing
/// is stored.
#[test]
fn ak13_a_window_that_declined_first_closes_the_pairing_window() {
    let h = start(true, vec![Right::View], Timing::default(), false);
    *h.console.slow_confirm.lock().unwrap() = Duration::from_millis(500);
    let (_t, store, id) = window_store();
    let p = start_pairing(&h.addr.to_string(), &code_of(&h), &id, "laptop").unwrap();
    p.decline();
    assert!(
        wait(Duration::from_secs(5), || !h.running.pairing_open()),
        "{:?}",
        h.console.lines.lock().unwrap()
    );
    assert!(store.hosts().unwrap().is_empty());
    assert!(
        !std::fs::read_to_string(h.state.join("remote/paired-keys.toml"))
            .unwrap_or_default()
            .contains("laptop")
    );
    // The used code opens nothing any more.
    let (_t2, _store2, id2) = window_store();
    assert!(start_pairing(&h.addr.to_string(), &code_of(&h), &id2, "other").is_err());
}

#[test]
fn ak9_sec3_revocation_closes_the_session_and_blocks_new_ones() {
    let h = start(
        true,
        vec![Right::View, Right::Edit],
        Timing::default(),
        false,
    );
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let s = open(&h, &store, &id);
    assert!(s.connected());
    let fp = fingerprint(&id.public);
    let host_store = Store::open_file(Side::Serve, &h.state.join("remote")).unwrap();
    assert!(host_store.revoke(&fp).unwrap());
    let mut s = s;
    assert!(wait(Duration::from_millis(1000), || {
        s.pump(Duration::from_millis(10));
        !s.connected()
    }));
    // No new connection. The handshake itself may finish; the listener then closes it.
    let refused = match connect_known(&h.addr.to_string(), &store, &id, "laptop") {
        Err(_) => true,
        Ok(t) => {
            let (c, _rx) = Connection::start(t);
            c.call("session.ping", json!({}), Duration::from_secs(3))
                .is_err()
        }
    };
    assert!(refused);
    // A restored old list cannot bring it back: the revocation is kept with the list.
    assert!(host_store.is_revoked(&fp).unwrap());
}

#[test]
fn ak10_sec7_loopback_only_unless_allowed() {
    let (_d, root) = scratch();
    let store = Store::open_file(Side::Serve, &root.join("remote")).unwrap();
    let console = Arc::new(TestConsole {
        confirm: true,
        grant: vec![],
        slow_rights: Duration::ZERO,
        slow_confirm: Mutex::new(Duration::ZERO),
        lines: Mutex::new(Vec::new()),
    });
    let e = listen(
        ListenOptions {
            addr: "192.0.2.1:47100".parse().unwrap(),
            allow_network: false,
            new_device: false,
            qr: false,
            timing: Timing::default(),
            limits: Limits::default(),
        },
        store,
        Backend::new(BackendConfig::default()),
        console,
    )
    .err()
    .unwrap();
    assert!(matches!(e, ListenError::NotLoopback(_)), "{e}");
    assert!(e.to_string().contains("--allow-network"));
}

#[test]
fn ak12_sec10_the_code_never_reaches_a_file_and_no_secret_in_the_store() {
    let h = start(true, vec![Right::View], Timing::default(), false);
    let (_t, store, id) = window_store();
    let code = code_of(&h);
    pair(&h, &store, &id);
    let mut seen = String::new();
    for f in walk(&h.state) {
        if let Ok(t) = std::fs::read_to_string(&f) {
            seen.push_str(&t);
        }
    }
    assert!(
        !seen.contains(&code),
        "the pairing code was written to disk"
    );
    let audit = std::fs::read_to_string(h.state.join("remote/audit.jsonl")).unwrap_or_default();
    let window_list = std::fs::read_to_string(store.list_path()).unwrap();
    assert!(!audit.contains(&code) && !window_list.contains(&code));
    // The list on the window's side has public material only.
    assert!(!window_list.contains("key = "), "{window_list}");
    assert!(
        h.console
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains(&code))
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            } else {
                out.push(e.path());
            }
        }
    }
    out
}

#[test]
fn ak14_sec14_paths_outside_and_oversized_frames_are_refused() {
    let h = start(
        true,
        vec![Right::View, Right::Edit],
        Timing::default(),
        false,
    );
    let (_t, store, id) = window_store();
    pair(&h, &store, &id);
    let outside = h.project.parent().unwrap().join("outside.txt");
    let t = connect_known(&h.addr.to_string(), &store, &id, "laptop").unwrap();
    let (c, _rx) = Connection::start(t);
    c.call(
        "session.hello",
        json!({"protocol": "idecraft-session", "majors": [1], "build": "t", "client": "window",
               "project": h.project.to_str().unwrap()}),
        Duration::from_secs(5),
    )
    .unwrap();
    let e = c
        .call(
            "fs.write",
            json!({"path": "../outside.txt", "text": "x"}),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert!(!outside.exists(), "{e}");
    let e = c
        .call(
            "fs.write",
            json!({"path": outside.to_str().unwrap(), "text": "x"}),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert!(!outside.exists(), "{e}");
    // An oversized frame closes the connection.
    let huge = vec![0u8; 8];
    let mut frame = (u32::MAX).to_be_bytes().to_vec();
    frame.extend_from_slice(&huge);
    c.send_raw(frame);
    assert!(wait(Duration::from_secs(3), || !c.alive()));
}
