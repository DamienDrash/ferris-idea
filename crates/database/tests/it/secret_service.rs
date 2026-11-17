//! Database passwords in the Secret Service on Linux (`SPEC-0008` v8 behaviour 18, AK-19, AK-20)
//! against a real Secret Service on a private session bus: a `dbus-daemon` and a
//! `gnome-keyring-daemon` started by this test in a temporary folder, never the bus or keyring of
//! the person running it. Skipped (and says so) where those programs are missing, except in CI
//! (`IDECRAFT_REQUIRE_TOOLS`). The refusal without a Secret Service needs neither program.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use idecraft_database::{
    KeychainStore, MemoryStore, Secret, SecretRef, SecretServiceStore, SecretStore,
    keychain_reachable,
};

/// Set by the parent to the private bus it started; the child refuses to run without it.
const PRIVATE_BUS: &str = "IDECRAFT_TEST_PRIVATE_BUS";

struct Daemons(Vec<Child>);

impl Drop for Daemons {
    fn drop(&mut self) {
        // Only the processes this test started, by their own handles.
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn found(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn private_env(cmd: &mut Command, root: &Path, bus: &str) {
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("TMPDIR", std::env::temp_dir())
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("DBUS_SESSION_BUS_ADDRESS", bus);
}

fn private_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    for sub in ["data", "config", "cache", "run"] {
        std::fs::create_dir(root.join(sub)).unwrap();
        std::fs::set_permissions(root.join(sub), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    (tmp, root)
}

/// Runs the ignored child test `name` with `bus` as its only session bus.
fn run_child(name: &str, root: &Path, bus: &str) -> (bool, String) {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.args([
        "--exact",
        name,
        "--include-ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    private_env(&mut child, root, bus);
    child.env(PRIVATE_BUS, bus);
    let output = child.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

/// The child's guard: only the private socket the parent made, never the user's bus.
fn on_private_bus() -> bool {
    let (Some(private), Some(bus)) = (
        std::env::var_os(PRIVATE_BUS),
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS"),
    ) else {
        eprintln!("not started on a private bus; nothing done");
        return false;
    };
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    let private = private.to_string_lossy().into_owned();
    assert_eq!(bus.to_string_lossy(), private);
    assert!(
        private.starts_with(&format!("unix:path={}", tmp.display())),
        "{private} is not under {}",
        tmp.display()
    );
    true
}

fn reference(account: &str) -> SecretRef {
    SecretRef {
        service: "idecraft-selftest".into(),
        account: format!("{account}-{}", std::process::id()),
    }
}

/// AK-19: with a Secret Service, passwords are stored, replaced, read and removed there, and a
/// password of an older version is moved over from the legacy store when first read.
#[test]
fn passwords_live_in_the_secret_service() {
    if !found("dbus-daemon") || !found("gnome-keyring-daemon") {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "dbus-daemon and gnome-keyring-daemon are required"
        );
        eprintln!("skipped: dbus-daemon or gnome-keyring-daemon is not installed");
        return;
    }
    let (_tmp, root) = private_root();
    let bus = format!("unix:path={}", root.join("run/bus").display());
    let mut daemons = Daemons(Vec::new());

    let mut dbus = Command::new("dbus-daemon");
    dbus.args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address={bus}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    private_env(&mut dbus, &root, &bus);
    let mut dbus = dbus.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(dbus.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    daemons.0.push(dbus);
    assert!(line.starts_with(&bus), "{line}");

    // A new login keyring, created and unlocked with a throwaway password from standard input.
    let mut keyring = Command::new("gnome-keyring-daemon");
    keyring
        .args(["--foreground", "--components=secrets", "--unlock"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    private_env(&mut keyring, &root, &bus);
    let mut keyring = keyring.spawn().unwrap();
    {
        let mut stdin = keyring.stdin.take().unwrap();
        stdin.write_all(b"idecraft-test-only").unwrap();
    }
    daemons.0.push(keyring);

    let t0 = Instant::now();
    let output = loop {
        let output = run_child("secret_service::secret_service_child", &root, &bus);
        // The keyring daemon may still be taking its name on the bus.
        if output.0 || t0.elapsed() > Duration::from_secs(10) {
            break output;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(output.0, "{}", output.1);
    assert!(
        output.1.contains("secret service child: done"),
        "{}",
        output.1
    );
}

/// Runs only inside [`passwords_live_in_the_secret_service`], on the private bus it started.
#[test]
#[ignore = "started by passwords_live_in_the_secret_service on its private bus"]
fn secret_service_child() {
    if !on_private_bus() {
        return;
    }
    assert!(keychain_reachable());

    // Stored, replaced (one entry), read and removed through the store IDECraft uses.
    let store = KeychainStore;
    let r = reference("roundtrip");
    store.set(&r, &Secret::new("first")).unwrap();
    store.set(&r, &Secret::new("second")).unwrap();
    assert_eq!(store.get(&r).unwrap().unwrap().expose(), "second");
    store.delete(&r).unwrap();
    assert!(store.get(&r).unwrap().is_none());
    store.delete(&r).unwrap();

    // A password of an older version moves out of the legacy store once.
    let legacy = MemoryStore::default();
    let moved = reference("moved");
    legacy.set(&moved, &Secret::new("from-kernel")).unwrap();
    let migrating = SecretServiceStore::with_legacy(legacy);
    assert_eq!(
        migrating.get(&moved).unwrap().unwrap().expose(),
        "from-kernel"
    );
    // Now only the Secret Service has it: a store with an empty legacy store still reads it.
    let fresh = SecretServiceStore::with_legacy(MemoryStore::default());
    assert_eq!(fresh.get(&moved).unwrap().unwrap().expose(), "from-kernel");
    assert!(
        migrating.get(&moved).unwrap().is_some(),
        "the Secret Service copy stays"
    );
    fresh.delete(&moved).unwrap();
    assert!(fresh.get(&moved).unwrap().is_none());
    println!("secret service child: done");
}

/// AK-20: without a Secret Service (a session bus nobody listens on), saving fails with the
/// reason and stores nothing elsewhere; needs neither daemon.
#[test]
fn without_a_secret_service_saving_is_refused() {
    let (_tmp, root) = private_root();
    // No daemon is started: the socket does not exist.
    let bus = format!("unix:path={}", root.join("run/bus").display());
    let (ok, text) = run_child("secret_service::no_secret_service_child", &root, &bus);
    assert!(ok, "{text}");
    assert!(text.contains("no secret service child: done"), "{text}");
}

/// Runs only inside [`without_a_secret_service_saving_is_refused`].
#[test]
#[ignore = "started by without_a_secret_service_saving_is_refused on a bus nobody serves"]
fn no_secret_service_child() {
    if !on_private_bus() {
        return;
    }
    assert!(!keychain_reachable());
    let legacy = MemoryStore::default();
    let store = SecretServiceStore::with_legacy(&legacy);
    let r = reference("headless");
    let err = store
        .set(&r, &Secret::new("hunter2"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("Secret Service"), "{err}");
    assert!(err.contains("cannot be reached in this session"), "{err}");
    assert!(!err.contains("hunter2"), "{err}");
    assert!(legacy.get(&r).unwrap().is_none(), "nothing weaker took it");
    let err = KeychainStore
        .set(&r, &Secret::new("hunter2"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot be reached in this session"), "{err}");
    println!("no secret service child: done");
}
