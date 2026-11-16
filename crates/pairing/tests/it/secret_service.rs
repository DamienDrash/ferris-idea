//! The Secret Service key store (`SPEC-0321` behaviour 20, AK-22) against a real Secret Service
//! on a private session bus: a `dbus-daemon` and a `gnome-keyring-daemon` started by this test in
//! a temporary folder, never the bus or keyring of the person running it. Skipped (and says so)
//! where those programs are missing.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use idecraft_pairing::{Side, Store, fingerprint};

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

#[test]
fn secret_service_on_a_private_bus() {
    if !found("dbus-daemon") || !found("gnome-keyring-daemon") {
        // CI installs both and sets IDECRAFT_REQUIRE_TOOLS, so there a missing one fails.
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "dbus-daemon and gnome-keyring-daemon are required"
        );
        eprintln!("skipped: dbus-daemon or gnome-keyring-daemon is not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    for sub in ["data", "config", "cache", "run"] {
        std::fs::create_dir(root.join(sub)).unwrap();
        std::fs::set_permissions(root.join(sub), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
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

    // The child test does the work with the private bus as its only session bus.
    let t0 = Instant::now();
    let output = loop {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args([
            "--exact",
            "secret_service::secret_service_child",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
        private_env(&mut child, &root, &bus);
        child.env(PRIVATE_BUS, &bus);
        let output = child.output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        // The keyring daemon may still be taking its name on the bus.
        if output.status.success() || t0.elapsed() > Duration::from_secs(10) {
            break (output.status.success(), text);
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

/// Runs only inside [`secret_service_on_a_private_bus`], on the private bus it started.
#[test]
#[ignore = "started by secret_service_on_a_private_bus on its private bus"]
fn secret_service_child() {
    let (Some(private), Some(bus)) = (
        std::env::var_os(PRIVATE_BUS),
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS"),
    ) else {
        eprintln!("not started on a private bus; nothing done");
        return;
    };
    // Never the bus of the person running the tests: only the socket the parent made.
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    let private = private.to_string_lossy().into_owned();
    assert_eq!(bus.to_string_lossy(), private);
    assert!(
        private.starts_with(&format!("unix:path={}", tmp.display())),
        "{private} is not under {}",
        tmp.display()
    );

    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let dir = home.join("config/idecraft/remote");
    let store = Store::open_secret_service(Side::Serve, &dir).unwrap();
    assert!(
        store.describe().contains("Secret Service"),
        "{}",
        store.describe()
    );
    let id = store.identity().unwrap();
    // The same identity on the next start, and no key in any file.
    let again = Store::open_secret_service(Side::Serve, &dir).unwrap();
    assert_eq!(again.identity().unwrap().public, id.public);
    assert!(!dir.join("serve-key").exists());

    let phone = [4u8; 32];
    store.add_device(&phone, "phone", &["view".into()]).unwrap();
    assert!(again.device_for(&phone).unwrap().is_some());
    let list = std::fs::read_to_string(dir.join("paired-keys.toml")).unwrap();
    let hex: String = phone.iter().map(|b| format!("{b:02x}")).collect();
    assert!(!list.contains(&hex), "{list}");
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);

    assert!(store.revoke(&fingerprint(&phone)).unwrap());
    assert!(again.device_for(&phone).unwrap().is_none());

    // The window side pins hosts in the same service.
    let window = Store::open_secret_service(Side::Window, &home.join("config/w")).unwrap();
    window.identity().unwrap();
    window.add_host("host:47100", &[3; 32]).unwrap();
    assert_eq!(window.host_key("host:47100").unwrap(), Some([3; 32]));
    assert!(window.forget_host("host:47100").unwrap());
    assert_eq!(window.host_key("host:47100").unwrap(), None);
    println!("secret service child: done");
}
