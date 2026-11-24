//! The SSH path of `SPEC-0324` (AK-4) over a real OpenSSH server of its own: a private `sshd` on a
//! high port with temporary keys and a private client configuration. Nothing outside the temp folder
//! is read or changed. Ignored by default; run with `cargo test -p ferris-idea-remote --test
//! paired_sshd -- --ignored`.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferris_idea_pairing::{Console, Limits, ListenOptions, Side, Store, Timing, listen};
use ferris_idea_remote::paired::{self, PairedProject};
use ferris_idea_session::{Backend, BackendConfig, Right};

struct Yes(std::sync::Mutex<Vec<String>>);

impl Console for Yes {
    fn show(&self, line: &str) {
        self.0.lock().unwrap().push(line.to_owned());
    }
    fn confirm(&self, _: &str, _: &str, _: &str) -> bool {
        true
    }
    fn rights(&self, _: &str, _: &[Right]) -> Vec<Right> {
        vec![Right::View, Right::Edit]
    }
}

struct Sshd(Child);

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn keygen(path: &Path) {
    assert!(
        Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

#[test]
#[ignore = "starts a private sshd; run with --ignored"]
fn ak4_real_sshd() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (host_key, client_key) = (root.join("host_key"), root.join("client_key"));
    keygen(&host_key);
    keygen(&client_key);
    let user = std::env::var("USER").unwrap();
    let port = free_port();
    std::fs::copy(root.join("client_key.pub"), root.join("authorized_keys")).unwrap();
    std::fs::write(
        root.join("sshd_config"),
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {host}\nAuthorizedKeysFile {auth}\n\
             PasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\n\
             UsePAM no\nStrictModes no\nPidFile {pid}\nAllowTcpForwarding yes\nAllowUsers {user}\n",
            host = host_key.display(),
            auth = root.join("authorized_keys").display(),
            pid = root.join("sshd.pid").display(),
        ),
    )
    .unwrap();
    let mut sshd = Command::new("/usr/sbin/sshd")
        .arg("-D")
        .arg("-e")
        .arg("-f")
        .arg(root.join("sshd_config"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(sshd.try_wait().unwrap().is_none(), "sshd exited");
        assert!(t0.elapsed() < Duration::from_secs(10), "sshd did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _sshd = Sshd(sshd);
    let host_pub = std::fs::read_to_string(root.join("host_key.pub")).unwrap();
    std::fs::write(
        root.join("known_hosts"),
        format!("[127.0.0.1]:{port} {host_pub}"),
    )
    .unwrap();
    std::fs::write(
        root.join("ssh_config"),
        format!(
            "Host m16test\n  HostName 127.0.0.1\n  Port {port}\n  User {user}\n  IdentityFile {key}\n  \
             IdentitiesOnly yes\n  UserKnownHostsFile {known}\n",
            key = client_key.display(),
            known = root.join("known_hosts").display(),
        ),
    )
    .unwrap();
    paired::set_ssh_extra(vec![
        "-F".into(),
        root.join("ssh_config").display().to_string(),
    ]);

    // The accepting side: on loopback only, as the SSH path expects.
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("a.txt"), "over ssh\n").unwrap();
    let state = root.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let serve_store = Store::open_file(Side::Serve, &state.join("remote")).unwrap();
    let backend = Backend::new(BackendConfig {
        state_dir: Some(state.clone()),
        folder: Some(project.clone()),
        ..BackendConfig::default()
    });
    let console = Arc::new(Yes(std::sync::Mutex::new(Vec::new())));
    let running = listen(
        ListenOptions {
            addr: "127.0.0.1:0".parse().unwrap(),
            allow_network: false,
            new_device: false,
            qr: false,
            timing: Timing::default(),
            limits: Limits::default(),
        },
        serve_store,
        backend,
        console.clone(),
    )
    .unwrap();
    let code = {
        let lines = console.0.lock().unwrap();
        let line = lines.iter().find(|l| l.contains(" code ")).unwrap();
        line.rsplit(" code ")
            .next()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    };

    // The device side, entirely through `ssh -W` and the ordinary paths.
    let address = format!("m16test:{}@ssh", running.addr.port());
    let (dev_dir, dev_root) = (tempfile::tempdir().unwrap(), root.join("device"));
    drop(dev_dir);
    std::fs::create_dir(&dev_root).unwrap();
    std::fs::set_permissions(&dev_root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_file(Side::Window, &dev_root.join("remote")).unwrap();
    let identity = store.identity().unwrap();
    let pipe = paired::ssh_pipe("m16test", running.addr.port())
        .unwrap()
        .pipe;
    let pending =
        ferris_idea_pairing::start_pairing_on(pipe, &address, &code, None, &identity, "over-ssh")
            .unwrap();
    assert_eq!(pending.digits().len(), 6);
    pending.confirm(&store).unwrap();
    assert!(store.host_key(&address).unwrap().is_some());

    let mut project_open =
        PairedProject::open_with(&address, project.to_str().unwrap(), None, store, identity)
            .unwrap();
    assert_eq!(project_open.session.info.rights, ["view", "edit"]);
    project_open.session.open_doc("a.txt").unwrap();
    project_open
        .session
        .edit(
            "a.txt",
            &[ferris_idea_session::Op {
                at: 0,
                delete: 0,
                insert: "real ".into(),
            }],
        )
        .unwrap();
    assert!(project_open.session.settle(Duration::from_secs(5)));
    project_open.session.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("a.txt")).unwrap(),
        "real over ssh\n"
    );

    // A broken pipe resumes through a new `ssh -W` and a new handshake.
    project_open.break_connection();
    project_open.reconnect().unwrap();
    assert!(project_open.session.connected());
    let _ = project_open.close(ferris_idea_remote::Unsaved::Discard);
    running.stop();
}
