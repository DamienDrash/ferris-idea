//! The mobile core against a real listener over loopback (`SPEC-0326` AK-1 to AK-9): real TCP, real
//! hybrid Noise, a real session backend. Only the keychain is replaced by a map.
#![cfg(unix)]

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use idecraft_mobile::{
    EntryKind, MAX_FILE_BYTES, MobileCore, MobileError, SecretVault, VaultError,
};
use idecraft_pairing::{Console, Limits, ListenOptions, Running, Side, Store, Timing, listen};
use idecraft_session::{Backend, BackendConfig, Right};

#[derive(Default)]
struct MapVault {
    values: Mutex<HashMap<String, Vec<u8>>>,
    refuse: Mutex<bool>,
}

impl MapVault {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.values.lock().unwrap().get(name).cloned()
    }
}

impl SecretVault for MapVault {
    fn load(&self, name: String) -> Result<Option<Vec<u8>>, VaultError> {
        if *self.refuse.lock().unwrap() {
            return Err(VaultError::Failed {
                reason: "OSStatus -25308".to_owned(),
            });
        }
        Ok(self.get(&name))
    }

    fn save(&self, name: String, value: Vec<u8>) -> Result<(), VaultError> {
        self.values.lock().unwrap().insert(name, value);
        Ok(())
    }

    fn delete(&self, name: String) -> Result<(), VaultError> {
        self.values.lock().unwrap().remove(&name);
        Ok(())
    }
}

struct TestConsole {
    confirm: bool,
    grant: Vec<Right>,
    lines: Mutex<Vec<String>>,
}

impl Console for TestConsole {
    fn show(&self, line: &str) {
        self.lines.lock().unwrap().push(line.to_owned());
    }
    fn confirm(&self, _: &str, _: &str, digits: &str) -> bool {
        self.show(&format!("digits {digits}"));
        self.confirm
    }
    fn rights(&self, _: &str, _: &[Right]) -> Vec<Right> {
        self.grant.clone()
    }
}

struct Host {
    _dir: tempfile::TempDir,
    project: PathBuf,
    console: Arc<TestConsole>,
    running: Running,
}

impl Host {
    fn address(&self) -> String {
        self.running.addr.to_string()
    }

    fn folder(&self) -> String {
        self.project.display().to_string()
    }

    fn line_after(&self, marker: &str) -> Option<String> {
        let lines = self.console.lines.lock().unwrap();
        let line = lines.iter().find(|l| l.contains(marker))?;
        let tail = line.rsplit(marker).next()?;
        Some(tail.split_whitespace().next()?.to_owned())
    }

    fn code(&self) -> String {
        self.line_after(" code ").unwrap()
    }

    fn link(&self) -> String {
        self.line_after("Pairing link: ").unwrap()
    }
}

fn start_host(confirm: bool, qr: bool) -> Host {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = dir.path().canonicalize().unwrap();
    let project = root.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("README.md"), "# hello from the host\n").unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(project.join("blob.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let big = vec![b'x'; 700 << 10];
    std::fs::write(project.join("big.txt"), &big).unwrap();
    let state = root.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    serve(dir, state, project, confirm, qr)
}

fn serve(
    dir: tempfile::TempDir,
    state: PathBuf,
    project: PathBuf,
    confirm: bool,
    qr: bool,
) -> Host {
    let store = Store::open_file(Side::Serve, &state.join("remote")).unwrap();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(state.clone()),
        folder: Some(project.clone()),
        peer: "listener".into(),
        ..BackendConfig::default()
    });
    let console = Arc::new(TestConsole {
        confirm,
        grant: vec![Right::View],
        lines: Mutex::new(Vec::new()),
    });
    let running = listen(
        ListenOptions {
            addr: "127.0.0.1:0".parse().unwrap(),
            allow_network: false,
            qr,
            new_device: false,
            timing: Timing::default(),
            limits: Limits::default(),
        },
        store,
        backend,
        console.clone(),
    )
    .unwrap();
    Host {
        _dir: dir,
        project,
        console,
        running,
    }
}

fn core() -> (Arc<MapVault>, Arc<MobileCore>) {
    let vault = Arc::new(MapVault::default());
    let core = MobileCore::new(vault.clone(), "test phone".to_owned());
    (vault, core)
}

fn pair(core: &Arc<MobileCore>, host: &Host) -> String {
    let p = core
        .clone()
        .begin_pairing(host.address(), host.code())
        .unwrap();
    assert_eq!(p.digits().len(), 6);
    let confirmed = p.confirm().unwrap();
    let shown = host.line_after("digits ").unwrap();
    assert_eq!(shown, p.digits(), "both sides show the same digits");
    assert_eq!(confirmed, p.host_fingerprint());
    confirmed
}

#[test]
fn ak1_pair_list_open_and_read_with_view_only() {
    let host = start_host(true, false);
    let (_vault, core) = core();
    let fp = pair(&core, &host);
    assert_eq!(core.hosts().unwrap()[0].fingerprint, fp);

    let project = core.open_project(host.address(), host.folder()).unwrap();
    assert_eq!(project.root(), host.folder());
    assert_eq!(project.rights(), ["view"]);
    let entries = project.list(".".to_owned()).unwrap();
    assert_eq!(entries[0].name, "src");
    assert_eq!(entries[0].kind, EntryKind::Directory);
    let readme = entries.iter().find(|e| e.name == "README.md").unwrap();
    assert_eq!(readme.kind, EntryKind::File);

    let file = project.read_file(readme.path.clone()).unwrap();
    assert_eq!(file.text.as_deref(), Some("# hello from the host\n"));

    let nested = project.list("src".to_owned()).unwrap();
    assert_eq!(nested[0].path, "src/main.rs");
    project.disconnect();
}

#[test]
fn ak2_keys_and_pins_are_only_in_the_vault() {
    let host = start_host(true, false);
    let (vault, core) = core();
    pair(&core, &host);
    assert_eq!(vault.get("identity").unwrap().len(), 64);
    let pins = String::from_utf8(vault.get("pins").unwrap()).unwrap();
    assert!(pins.contains(&host.address()), "{pins}");
    // Nothing that crosses to the interface carries a key.
    let shown = format!("{:?}", core.hosts().unwrap());
    let identity: String = vault.get("identity").unwrap()[32..]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!shown.contains(&identity));
    assert!(!pins.contains(&identity));
}

#[test]
fn ak3_a_key_is_reused_and_a_damaged_one_is_not_replaced() {
    let (vault, core) = core();
    let host = start_host(true, false);
    pair(&core, &host);
    let key = vault.get("identity").unwrap();
    let second = start_host(true, false);
    pair(&core, &second);
    assert_eq!(vault.get("identity").unwrap(), key);

    let third = start_host(true, false);
    vault
        .values
        .lock()
        .unwrap()
        .insert("identity".into(), vec![1, 2, 3]);
    let e = core
        .clone()
        .begin_pairing(third.address(), third.code())
        .err()
        .unwrap();
    assert!(matches!(e, MobileError::Vault(_)), "{e}");
    assert_eq!(vault.get("identity").unwrap(), [1, 2, 3]);
}

#[test]
fn ak4_declining_stores_nothing() {
    let host = start_host(true, false);
    let (vault, core) = core();
    let p = core
        .clone()
        .begin_pairing(host.address(), host.code())
        .unwrap();
    p.decline();
    assert!(core.hosts().unwrap().is_empty());
    assert!(vault.get("pins").is_none());
    assert!(matches!(p.confirm(), Err(MobileError::Finished)));

    let refusing = start_host(false, false);
    let p = core
        .clone()
        .begin_pairing(refusing.address(), refusing.code())
        .unwrap();
    // The host may close before the phone's answer is written: then it reads as a broken pairing.
    assert!(matches!(
        p.confirm(),
        Err(MobileError::Declined | MobileError::Pairing(_))
    ));
    assert!(core.hosts().unwrap().is_empty());
}

#[test]
fn ak5_a_changed_host_key_is_refused() {
    let host = start_host(true, false);
    let (_vault, core) = core();
    pair(&core, &host);
    let address = host.address();
    let folder = host.folder();
    let Host {
        _dir,
        project,
        running,
        console,
    } = host;
    running.stop();
    drop(console);
    // A different machine (new key) now answers on the same address.
    let other = tempfile::tempdir().unwrap();
    std::fs::set_permissions(other.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = other.path().canonicalize().unwrap().join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_file(Side::Serve, &state.join("remote")).unwrap();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(state.clone()),
        folder: Some(project),
        peer: "listener".into(),
        ..BackendConfig::default()
    });
    let console = Arc::new(TestConsole {
        confirm: true,
        grant: vec![Right::View],
        lines: Mutex::new(Vec::new()),
    });
    let mut attacker = None;
    for _ in 0..50 {
        match listen(
            ListenOptions {
                addr: address.parse().unwrap(),
                allow_network: false,
                qr: false,
                new_device: false,
                timing: Timing::default(),
                limits: Limits::default(),
            },
            Store::open_file(Side::Serve, &state.join("remote")).unwrap(),
            backend.clone(),
            console.clone(),
        ) {
            Ok(r) => {
                attacker = Some(r);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
    drop(store);
    let _attacker = attacker.expect("the port became free");
    let e = core.open_project(address, folder).err().unwrap();
    assert!(matches!(e, MobileError::ChangedKey(_)), "{e}");
}

#[test]
fn ak6_a_link_with_another_key_is_refused_before_any_digits() {
    let host = start_host(true, true);
    let (vault, core) = core();
    let link = host.link();
    let at = link.find("k=").unwrap() + 2;
    let mut wrong = link.clone();
    let flipped = if &link[at..at + 1] == "0" { "1" } else { "0" };
    wrong.replace_range(at..at + 1, flipped);
    let e = core.clone().begin_pairing_link(wrong).err().unwrap();
    assert!(matches!(e, MobileError::ChangedKey(_)), "{e}");
    assert!(vault.get("pins").is_none());

    // A failed attempt uses up that pairing window; the host shows a fresh link for the next one.
    let host = start_host(true, true);
    let p = core.clone().begin_pairing_link(host.link()).unwrap();
    p.confirm().unwrap();
    assert_eq!(core.hosts().unwrap().len(), 1);
    assert!(matches!(
        core.clone().begin_pairing_link("https://x".to_owned()),
        Err(MobileError::Address(_))
    ));
}

#[test]
fn ak7_paths_and_files_outside_the_rules_are_refused() {
    let host = start_host(true, false);
    let (_vault, core) = core();
    pair(&core, &host);
    let project = core.open_project(host.address(), host.folder()).unwrap();
    assert!(project.list("../..".to_owned()).is_err());
    assert!(project.read_file("../../etc/passwd".to_owned()).is_err());
    assert!(matches!(
        project.read_file("src".to_owned()),
        Err(MobileError::NotAFile(_))
    ));

    // Above the inline limit the text comes through a stream.
    let big = project.read_file("big.txt".to_owned()).unwrap();
    assert_eq!(big.text.unwrap().len(), 700 << 10);
    // Not UTF-8: no text, but the size.
    let blob = project.read_file("blob.bin".to_owned()).unwrap();
    assert_eq!((blob.text, blob.size), (None, 4));

    let huge = std::fs::File::create(host.project.join("huge.bin")).unwrap();
    huge.set_len(MAX_FILE_BYTES + 1).unwrap();
    assert!(matches!(
        project.read_file("huge.bin".to_owned()),
        Err(MobileError::TooLarge { .. })
    ));
}

#[test]
fn ak8_a_failing_vault_stops_everything_and_says_so() {
    let host = start_host(true, false);
    let (vault, core) = core();
    *vault.refuse.lock().unwrap() = true;
    let e = core
        .clone()
        .begin_pairing(host.address(), host.code())
        .err()
        .unwrap();
    assert!(matches!(e, MobileError::Vault(_)), "{e}");
    assert!(e.to_string().contains("-25308"), "{e}");
}

#[test]
fn ak9_pairs_are_refused_twice_forgotten_and_unknown_hosts_named() {
    let host = start_host(true, false);
    let (_vault, core) = core();
    assert!(matches!(
        core.open_project(host.address(), host.folder()),
        Err(MobileError::NotPaired(_))
    ));
    pair(&core, &host);
    assert!(matches!(
        core.clone()
            .begin_pairing(host.address(), "ABCD".to_owned()),
        Err(MobileError::AlreadyPaired(_))
    ));
    assert!(core.forget_host(host.address()).unwrap());
    assert!(!core.forget_host(host.address()).unwrap());
    assert!(core.hosts().unwrap().is_empty());
}
