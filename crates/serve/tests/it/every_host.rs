//! `idecraft-serve` as a program (`SPEC-0321` AK-1, AK-10, AK-15, AK-17).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use idecraft_remote::{ExecRoute, HostRoute, HostSpec, RemoteProject, ServeOptions};
use idecraft_session::Op;

const BIN: &str = env!("CARGO_BIN_EXE_idecraft-serve");

fn scratch() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let p = t.path().canonicalize().unwrap();
    (t, p)
}

#[test]
fn ak1_version_starts_with_the_product_and_version() {
    let out = Command::new(BIN).arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        text.starts_with(&format!("idecraft {}", env!("CARGO_PKG_VERSION"))),
        "{text}"
    );
}

fn pair_refusal(state: &Path, extra: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(BIN)
        .args(["serve", "--pair", "--listen", "127.0.0.1:0", "--state-dir"])
        .arg(state)
        .args(extra)
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn ak10_a_network_address_needs_the_explicit_switch() {
    let (_t, root) = scratch();
    let out = Command::new(BIN)
        .args([
            "serve",
            "--pair",
            "--listen",
            "192.0.2.1:47100",
            "--state-dir",
        ])
        .arg(&root)
        .args(["--key-file"])
        .arg(&root)
        .env("IDECRAFT_CONFIG_DIR", &root)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(err.contains("--allow-network"), "{err}");
}

/// The file store exists only where no keychain is required (`ADR-0025` §3a).
#[cfg(not(target_os = "macos"))]
#[test]
fn ak15_an_open_key_file_exits_78_before_listening() {
    let (_t, root) = scratch();
    let dir = root.join("remote");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let key = dir.join("serve-key");
    std::fs::write(&key, "kind = \"remote-serve-key\"\n").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (code, err) = pair_refusal(&root, &[]);
    assert_eq!(code, Some(78), "{err}");
    assert!(err.contains("group or others"), "{err}");
}

#[cfg(target_os = "macos")]
#[test]
fn ak15_the_file_store_is_not_offered_on_macos() {
    let _ = pair_refusal;
    assert!(idecraft_pairing::keys::keychain_required());
}

/// A stand-in for `docker` that really runs the line, with its own home folder.
fn fake_engine(root: &Path) -> PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let script = root.join("docker");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n# exec -i <container> sh -c <line>\n[ \"$1\" = exec ] || exit 64\nshift 3\nHOME='{}' exec \"$@\"\n",
            home.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn ak17_a_project_opens_and_saves_through_an_exec_route() {
    let (_t, root) = scratch();
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("a.txt"), "one\n").unwrap();
    let route = HostRoute::Exec(
        ExecRoute::new(HostSpec::parse("docker://web").unwrap())
            .unwrap()
            .with_program(fake_engine(&root)),
    );
    let opts = ServeOptions {
        state_dir: Some(root.join("state").display().to_string()),
        run_dir: Some(root.join("run").display().to_string()),
        ..ServeOptions::default()
    };
    let mut p =
        RemoteProject::open_route(route, project.to_str().unwrap(), Path::new(BIN), opts, None)
            .unwrap();
    assert_eq!(p.host, "docker://web");
    p.session.open_doc("a.txt").unwrap();
    p.session
        .edit(
            "a.txt",
            &[Op {
                at: 0,
                delete: 0,
                insert: "zero ".into(),
            }],
        )
        .unwrap();
    assert!(p.session.settle(Duration::from_secs(5)));
    p.session.save("a.txt", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("a.txt")).unwrap(),
        "zero one\n"
    );
    // Copied once, under the home folder of the "container".
    let installed: Vec<_> = std::fs::read_dir(root.join("home/.cache/idecraft/bin"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(installed.len(), 1);
    p.close(idecraft_remote::Unsaved::Discard).unwrap();
}

/// Against a real engine or cluster (`SPEC-0321` AK-17), when the environment names one:
/// `IDECRAFT_TEST_REAL_HOST` (host string), `IDECRAFT_TEST_REAL_FOLDER` (holding `a.txt`),
/// `IDECRAFT_BACKENDS_DIR`, and optionally `IDECRAFT_TEST_REAL_PROGRAM` (the engine program).
#[test]
fn ak17_a_real_container_or_pod() {
    let (Ok(host), Ok(folder)) = (
        std::env::var("IDECRAFT_TEST_REAL_HOST"),
        std::env::var("IDECRAFT_TEST_REAL_FOLDER"),
    ) else {
        return;
    };
    let mut exec = ExecRoute::new(HostSpec::parse(&host).unwrap()).unwrap();
    if let Ok(program) = std::env::var("IDECRAFT_TEST_REAL_PROGRAM") {
        exec = exec.with_program(program.into());
    }
    let mut p = RemoteProject::open_route(
        HostRoute::Exec(exec),
        &folder,
        Path::new(BIN),
        ServeOptions::default(),
        None,
    )
    .unwrap();
    assert!(
        p.session.info.platform.starts_with("linux"),
        "{}",
        p.session.info.platform
    );
    p.session.open_doc("a.txt").unwrap();
    p.session
        .edit(
            "a.txt",
            &[Op {
                at: 0,
                delete: 0,
                insert: "real ".into(),
            }],
        )
        .unwrap();
    assert!(p.session.settle(Duration::from_secs(10)));
    p.session.save("a.txt", false).unwrap();
    p.break_connection();
    p.reconnect().unwrap();
    assert!(p.session.doc("a.txt").unwrap().text().starts_with("real "));
    p.close(idecraft_remote::Unsaved::Discard).unwrap();
}

#[test]
fn ak2_a_missing_container_names_the_cause() {
    let (_t, root) = scratch();
    let script = root.join("docker");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'Error response from daemon: No such container: web' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let route = HostRoute::Exec(
        ExecRoute::new(HostSpec::parse("docker://web").unwrap())
            .unwrap()
            .with_program(script),
    );
    let e = RemoteProject::open_route(route, "/x", Path::new(BIN), ServeOptions::default(), None)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("does not exist or is not running"), "{e}");
}

/// SPEC-0324/AK-2: a device is bound to one project, so `--pair` needs a folder.
#[test]
fn ak2_pair_without_a_folder_is_refused() {
    let (_t, root) = scratch();
    let out = Command::new(BIN)
        .args([
            "serve",
            "--pair",
            "--listen",
            "127.0.0.1:0",
            "--key-file",
            "--state-dir",
        ])
        .arg(&root)
        .env("IDECRAFT_CONFIG_DIR", &root)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs a folder"));
}
