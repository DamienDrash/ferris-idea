//! SPEC-0317 AK-3, AK-4 and the SSH parts of AK-5 (SEC-2, -7, -9, -10, -11, -14): a folder as a
//! remote project against a real `sshd` started as the current user (skipped without one). This
//! binary is uploaded to a test folder below the home folder, checked by SHA-256 and started as
//! `ferris-idea serve --stdio`; the window half runs in this test. One test function on purpose: the
//! listening-socket checks look at this process, which must not run other tests meanwhile.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ferris_idea_remote::{
    Installed, Remote, RemoteError, RemoteProject, ServeOptions, Unsaved, testing::TestHost,
};
use ferris_idea_session::{Op, Reconnected};

fn op(at: usize, delete: usize, insert: &str) -> Op {
    Op {
        at,
        delete,
        insert: insert.into(),
    }
}

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

fn output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The newest backend process started with this state folder.
fn backend_pid(state: &Path) -> Option<String> {
    let pat = format!("serve --stdio --state-dir {}", state.display());
    let out = output("pgrep", &["-n", "-f", &pat]);
    let pid = out.trim();
    (!pid.is_empty()).then(|| pid.to_owned())
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).map(|m| m.mode() & 0o777).unwrap_or(0)
}

/// Reads stream `id` until its text contains `needle`.
fn read_until(conn: &ferris_idea_session::Connection, id: u32, needle: &str) -> String {
    let t0 = Instant::now();
    let mut all = Vec::new();
    while t0.elapsed() < Duration::from_secs(20) {
        let (data, done) = conn.read_stream(id, Duration::from_millis(100));
        all.extend(data);
        if String::from_utf8_lossy(&all).contains(needle) || done.is_some() {
            break;
        }
    }
    String::from_utf8_lossy(&all).into_owned()
}

/// SPEC-0318 AK-10 and its SEC-7/SEC-10 parts over the SSH channel.
fn host_functions_over_ssh(p: &mut RemoteProject, project: &Path, backend: &str) {
    use serde_json::json;
    let long = Duration::from_secs(90);
    for family in ["lsp", "search", "git", "term", "run", "port", "dap", "mcp"] {
        assert!(
            p.session.info.capabilities.iter().any(|c| c == family),
            "{family}: {:?}",
            p.session.info.capabilities
        );
    }
    // Search on the host.
    let found = p
        .session
        .call_within(
            "search.text",
            json!({ "query": "changed on the host" }),
            long,
        )
        .unwrap();
    assert!(found.to_string().contains("notes.txt"), "{found}");
    // An untrusted host project runs nothing (safe mode in `serve`); the test leaves the host's
    // trust list alone.
    let refused = p
        .session
        .call_within("run.start", json!({ "name": "anything" }), long)
        .unwrap_err();
    assert_eq!(
        refused.code,
        ferris_idea_session::ErrorCode::Denied,
        "{refused:?}"
    );
    // A terminal on the host: a pseudo terminal of the backend, no second ssh process.
    let t = p
        .session
        .call_within("term.open", json!({ "cols": 80, "rows": 24 }), long)
        .unwrap();
    let stream = t["stream"].as_u64().unwrap() as u32;
    let conn = p.session.connection().unwrap();
    conn.open_sending(stream);
    conn.send_data(stream, b"echo host-$((6*7))\n", Duration::from_secs(5))
        .unwrap();
    let out = read_until(&conn, stream, "host-42");
    assert!(out.contains("host-42"), "{out}");
    // A forward to a program the terminal started, through the session; the window side listens
    // on loopback only, the backend on nothing (SEC-7).
    if Command::new("python3").arg("--version").output().is_ok() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(project.join("page.html"), "page on the host\n").unwrap();
        conn.send_data(
            stream,
            format!("python3 -m http.server {port} --bind 127.0.0.1\n").as_bytes(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(wait_until(Duration::from_secs(30), || {
            p.session.pump(Duration::from_millis(50));
            p.session
                .host_events
                .iter()
                .any(|(m, e)| m == "port.detected" && e["port"] == port)
        }));
        let f = p
            .session
            .call_within("port.forward", json!({ "port": port }), long)
            .unwrap();
        let local = ferris_idea_session::forward::LocalForward::start(
            conn.clone(),
            f["forward"].as_str().unwrap(),
            0,
        )
        .unwrap();
        let mut c = std::net::TcpStream::connect(("127.0.0.1", local.port())).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        std::io::Write::write_all(&mut c, b"GET /page.html HTTP/1.0\r\n\r\n").unwrap();
        let mut answer = String::new();
        let _ = c.read_to_string(&mut answer);
        assert!(answer.contains("page on the host"), "{answer}");
        let inet = output(
            "lsof",
            &["-nP", "-a", "-p", backend, "-iTCP", "-sTCP:LISTEN"],
        );
        assert!(inet.trim().is_empty(), "the backend listens: {inet}");
        let me = std::process::id().to_string();
        let mine = output(
            "lsof",
            &["-nP", "-a", "-p", &me, "-iTCP", "-sTCP:LISTEN", "-Fn"],
        );
        for line in mine.lines().filter_map(|l| l.strip_prefix('n')) {
            assert!(
                line.starts_with("127.0.0.1:") || line.starts_with("[::1]:"),
                "the window listens on {line}"
            );
        }
        let e = p
            .session
            .call_within("port.forward", json!({ "port": 5005 }), long)
            .unwrap_err();
        assert_eq!(e.code, ferris_idea_session::ErrorCode::Denied);
        local.close();
        p.session
            .call_within("port.close", json!({ "forward": f["forward"] }), long)
            .unwrap();
    }
    // Diagnostics of a C file from clangd on the host, following an edit of the session. Whether the
    // host has clangd is asked there: its PATH is the SSH session's, not this test's.
    let status = p
        .session
        .call_within("lsp.status", json!({}), long)
        .unwrap();
    let host_has_clangd = status["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|l| l["program"] == "clangd");
    if host_has_clangd {
        std::fs::write(project.join("calc.c"), "int main(void) { return 0; }\n").unwrap();
        p.session.open_doc("calc.c").unwrap();
        p.session
            .edit("calc.c", &[op(17, 0, "return nothing_here; ")])
            .unwrap();
        assert!(p.session.settle(Duration::from_secs(10)));
        assert!(
            wait_until(long, || {
                p.session.pump(Duration::from_millis(50));
                p.session
                    .host_events
                    .iter()
                    .any(|(m, e)| m == "lsp.diagnostics" && e.to_string().contains("nothing_here"))
            }),
            "{:?}",
            p.session.host_events
        );
        // Saved, so the reconciliation later in the test sees only its own documents.
        p.session.save("calc.c", false).unwrap();
        p.session.close_doc("calc.c").unwrap();
    } else {
        eprintln!("skipped the clangd part: no clangd on the host's PATH");
    }
    // SEC-10: no resume token in any process's arguments or environment (shell, language
    // server and the rest started by the backend included).
    let token = p.session.resume_token().expose();
    let processes = output("ps", &["-axeww", "-o", "command="]);
    assert!(!processes.contains(&token), "token in a process list");
    let _ = p
        .session
        .call_within("term.close", json!({ "term": t["term"] }), long);
}

/// Removes its folders when dropped, also after a failed assertion.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for dir in &self.0 {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[test]
fn a_folder_on_an_ssh_host_opens_survives_breaks_and_stays_private() {
    // One test at a time uploads and starts a backend on a test host (see the helper).
    let _turn = ferris_idea_remote::testing::one_host_at_a_time();
    let base = std::env::temp_dir().join(format!("ferris-idea-rproj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home")).unwrap();
    let base = std::fs::canonicalize(&base).unwrap();
    // The sessions' home folder is the test's own: the uploaded backend never lands in the
    // user's home folder, and a run beside this one never shares it.
    let home = base.join("home");
    let Some(host) =
        TestHost::start_with_env(&base.join("ssh"), &[("HOME", &home.display().to_string())])
    else {
        eprintln!("skipped: no sshd");
        return;
    };
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("notes.txt"), "remote notes\n").unwrap();
    std::fs::write(project.join("run.sh"), "echo a\n").unwrap();
    std::fs::write(project.join("other.txt"), "other\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            project.join("run.sh"),
            std::fs::Permissions::from_mode(0o750),
        )
        .unwrap();
    }
    let state = base.join("state");
    // Unix socket paths are short; the run folder of the host lives below /tmp.
    let run = PathBuf::from(format!("/tmp/idc-rp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&run);
    let recovery = base.join("window-state/recovery");
    let opts = ServeOptions {
        backend_dir: format!(".cache/ferris-idea-test-rp-{}", std::process::id()),
        state_dir: Some(state.display().to_string()),
        run_dir: Some(run.display().to_string()),
        reattach_minutes: None,
    };
    let binary = Path::new(env!("CARGO_BIN_EXE_ferris-idea"));
    let folder = project.display().to_string();
    // The uploaded backend (a debug build is gigabytes), the run folder and the test's folder go
    // also when the test fails.
    let _cleanup = Cleanup(vec![
        home.join(&opts.backend_dir),
        run.clone(),
        base.clone(),
    ]);

    // SEC-2: unknown and changed host keys are refused without a prompt.
    let unknown = RemoteProject::open(
        Remote::new(host.unknown_key_target()),
        &folder,
        binary,
        opts.clone(),
        None,
    );
    assert!(
        matches!(unknown, Err(RemoteError::HostKeyUnknown(_))),
        "{:?}",
        unknown.err()
    );
    let changed = RemoteProject::open(
        Remote::new(host.changed_key_target()),
        &folder,
        binary,
        opts.clone(),
        None,
    );
    let e = changed.err().unwrap();
    assert!(matches!(e, RemoteError::HostKeyChanged(_)), "{e:?}");
    assert!(e.to_string().contains("Nothing was sent"));

    // AK-3: upload with checksum, tree, documents.
    let remote = Remote::new(host.target.clone());
    let mut p = RemoteProject::open(
        remote.clone(),
        &folder,
        binary,
        opts.clone(),
        Some(recovery.clone()),
    )
    .unwrap();
    assert!(
        matches!(p.installed, Installed::Uploaded(_)),
        "{:?}",
        p.installed
    );
    assert!(p.session.info.build.starts_with("ferris-idea "));
    // SEC-9: a backend that gets no session.hello closes after 10 s. It is started now and checked
    // at the end, so the test does not wait those 10 s on their own.
    let stalled = {
        // Its own state folder, not one starting with `state`: `backend_pid` looks for the newest
        // backend whose command line contains `--state-dir <state>`.
        let own = ServeOptions {
            state_dir: Some(base.join("stalled").display().to_string()),
            ..opts.clone()
        };
        let line = ferris_idea_remote::serve_line(p.installed.path(), &own);
        let mut child = remote
            .line(ferris_idea_remote::SshPurpose::Serve)
            .unwrap()
            .command()
            .arg(line)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let t0 = Instant::now();
        let waited = std::thread::spawn(move || {
            let mut out = Vec::new();
            stdout.read_to_end(&mut out).unwrap();
            (out, t0.elapsed())
        });
        (child, waited)
    };
    let tree = p.session.list(".").unwrap();
    let names: Vec<&str> = tree.iter().filter_map(|e| e["path"].as_str()).collect();
    assert_eq!(names, ["notes.txt", "other.txt", "run.sh"]);
    p.session.watch(&[]).unwrap();
    p.session.open_doc("run.sh").unwrap();
    p.session.open_doc("notes.txt").unwrap();
    p.session.edit("run.sh", &[op(5, 1, "b")]).unwrap();
    p.session.save("run.sh", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("run.sh")).unwrap(),
        "echo b\n"
    );
    #[cfg(unix)]
    assert_eq!(mode(&project.join("run.sh")), 0o750);
    let history = ferris_idea_core::LocalHistory::open(&project).unwrap();
    assert!(!history.revisions("run.sh").is_empty());
    // A host-side change while unmodified reloads; while modified it is refused for compare.
    std::fs::write(project.join("notes.txt"), "changed on the host\n").unwrap();
    assert!(wait_until(Duration::from_secs(5), || {
        p.session.pump(Duration::from_millis(20));
        p.session.doc("notes.txt").unwrap().text() == "changed on the host\n"
    }));
    p.session.edit("run.sh", &[op(0, 0, "# ")]).unwrap();
    assert!(p.session.settle(Duration::from_secs(10)));
    std::fs::write(project.join("run.sh"), "echo theirs\n").unwrap();
    assert!(wait_until(Duration::from_secs(5), || {
        p.session.pump(Duration::from_millis(20));
        p.session.doc("run.sh").unwrap().disk_changed
    }));
    let conflict = p.session.save("run.sh", false).unwrap_err();
    assert_eq!(conflict.code, ferris_idea_session::ErrorCode::Conflict);
    p.session.save("run.sh", true).unwrap();
    // A host-side create appears through fs.watch.
    std::fs::write(project.join("created.rs"), "fn c() {}\n").unwrap();
    assert!(wait_until(Duration::from_secs(5), || {
        p.session.pump(Duration::from_millis(20));
        p.session
            .tree_events
            .iter()
            .any(|e| e["path"] == "created.rs" && e["kind"] == "created")
    }));
    // SEC-14: the backend refuses paths that leave the folder.
    for bad in ["../outside.txt", "/etc/hosts", "notes.txt/../../x"] {
        assert!(p.session.open_doc(bad).is_err(), "{bad}");
    }

    // SEC-7: the backend listens on no network interface, only on its reattach socket; this
    // window process listens on nothing either.
    let pid = backend_pid(&state).expect("backend process");
    let inet = output("lsof", &["-nP", "-a", "-p", &pid, "-i"]);
    assert!(inet.trim().is_empty(), "{inet}");
    // Under load `lsof` sometimes answers with nothing; it is asked again for a while before the
    // missing socket counts as a failure.
    let mut unix = String::new();
    let listed = wait_until(Duration::from_secs(20), || {
        unix = output("lsof", &["-nP", "-a", "-p", &pid, "-U"]);
        unix.contains(&run.display().to_string()) || unix.contains(".sock")
    });
    assert!(listed, "lsof -U for {pid}: {unix:?}");
    let me = std::process::id().to_string();
    let mine = output("lsof", &["-nP", "-a", "-p", &me, "-iTCP", "-sTCP:LISTEN"]);
    // The SSH test host may run inside this test process; its port is the test's, not the window's.
    let test_host = host
        .port_in_this_process()
        .map(|port| format!("127.0.0.1:{port} (LISTEN)"));
    let window: Vec<&str> = mine
        .lines()
        .filter(|l| !l.starts_with("COMMAND"))
        .filter(|l| test_host.as_ref().is_none_or(|h| !l.contains(h.as_str())))
        .collect();
    assert!(window.is_empty(), "{mine}");

    // SEC-10 and SEC-11: the token is nowhere outside the channel; folders 0700, files 0600.
    let token = p.session.resume_token().expose();
    let processes = output("ps", &["-axeww", "-o", "command="]);
    assert!(!processes.contains(&token), "token in a process list");
    assert!(
        processes.contains("serve --stdio"),
        "the backend should be visible: {}",
        processes
            .lines()
            .filter(|l| l.contains("ferris-idea"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let described = p.describe().to_string();
    assert!(!described.contains(&token));
    assert!(described.contains("\"state\":\"connected\""), "{described}");
    let audit = std::fs::read_to_string(state.join("remote/audit.jsonl")).unwrap();
    assert!(audit.contains("ssh:"), "{audit}");
    assert!(audit.contains("\"method\":\"doc.save\""));
    assert!(!audit.contains(&token));
    assert!(!p.connection_log().join("\n").contains(&token));
    #[cfg(unix)]
    {
        assert_eq!(mode(&state), 0o700);
        assert_eq!(mode(&state.join("remote")), 0o700);
        assert_eq!(mode(&state.join("remote/audit.jsonl")), 0o600);
        assert_eq!(mode(&run), 0o700);
    }

    // SPEC-0318 AK-10: the uploaded backend binds the host functions; a terminal, a forward and
    // (where clangd is installed) diagnostics run on the host through the SSH channel.
    host_functions_over_ssh(&mut p, &project, &pid);

    // AK-4: the ssh process dies during typing; edits made meanwhile are kept and applied once.
    p.session.open_doc("other.txt").unwrap();
    p.session.edit("other.txt", &[op(0, 0, "1 ")]).unwrap();
    assert!(p.session.settle(Duration::from_secs(10)));
    p.break_connection();
    assert!(!p.session.connected());
    p.session.edit("other.txt", &[op(2, 0, "2 ")]).unwrap();
    p.session.edit("other.txt", &[op(4, 0, "3 ")]).unwrap();
    assert!(p.session.has_recovery("other.txt"));
    for f in std::fs::read_dir(&recovery).unwrap() {
        let f = f.unwrap().path();
        #[cfg(unix)]
        assert_eq!(mode(&f), 0o600);
        assert!(!std::fs::read_to_string(&f).unwrap().contains(&token));
    }
    let not_saved = p.session.save("other.txt", false).unwrap_err();
    assert!(
        not_saved.message.starts_with("Not saved — reconnecting"),
        "{not_saved}"
    );
    assert!(
        p.describe()
            .to_string()
            .contains("\"state\":\"disconnected\"")
    );
    let r = p.reconnect().unwrap();
    assert!(matches!(r, Reconnected::Resumed { .. }), "{r:?}");
    assert!(p.session.settle(Duration::from_secs(10)));
    let text = p.session.doc("other.txt").unwrap().text();
    assert_eq!(text, "1 2 3 other\n");
    assert_eq!(
        std::fs::read_to_string(project.join("other.txt")).unwrap(),
        text
    );
    assert!(!p.session.has_recovery("other.txt"));
    // The token is still the same session's; `--attach` carried no secret either.
    let processes = output("ps", &["-axeww", "-o", "command="]);
    assert!(!processes.contains(&token));

    // The backend goes away (as after a host restart): a new one reconciles one unchanged and
    // one changed document.
    p.session.edit("notes.txt", &[op(0, 0, "mine ")]).unwrap();
    p.session.edit("other.txt", &[op(0, 0, "mine ")]).unwrap();
    assert!(p.session.settle(Duration::from_secs(10)));
    let pid = backend_pid(&state).unwrap();
    p.break_connection();
    let _ = Command::new("kill").arg(&pid).status();
    assert!(wait_until(Duration::from_secs(5), || {
        !output("ps", &["-p", &pid, "-o", "pid="]).contains(&pid)
    }));
    std::fs::write(project.join("other.txt"), "rewritten on the host\n").unwrap();
    let r = p.reconnect().unwrap();
    assert_eq!(
        r,
        Reconnected::NewBackend {
            followed: vec!["notes.txt".into()],
            kept: vec!["other.txt".into()],
        }
    );
    assert_eq!(
        p.session.doc("notes.txt").unwrap().text(),
        "mine changed on the host\n"
    );
    let kept = p.session.doc("other.txt").unwrap();
    assert_eq!(kept.local_version.as_deref(), Some("mine 1 2 3 other\n"));
    assert!(
        p.connection_log()
            .iter()
            .any(|l| l.contains("kept as local versions"))
    );
    let documents = p.documents().to_string();
    assert!(documents.contains("\"pending_edits\":0"), "{documents}");

    // Closing refuses while files are unsaved; discarding ends the backend.
    assert!(p.close(Unsaved::Refuse).is_err());
    let pid = backend_pid(&state).unwrap();
    let unsaved = p.close(Unsaved::Discard).unwrap();
    assert!(unsaved.contains(&"notes.txt".to_owned()), "{unsaved:?}");
    assert!(wait_until(Duration::from_secs(10), || {
        !output("ps", &["-p", &pid, "-o", "pid="]).contains(&pid)
    }));

    // SPEC-0317 v3: the window's handle over SSH — the ssh process is killed, the supervisor
    // reconnects by itself through `serve --attach`, and what was typed during the break is kept
    // and saved on the host.
    {
        use ferris_idea_remote::{ConnState, LiveProject, Timing};
        let open = || {
            RemoteProject::open(
                remote.clone(),
                &folder,
                binary,
                opts.clone(),
                Some(recovery.clone()),
            )
        };
        // On the Mac runner under a load average above 400 (2026-11-15) the backend's first answer
        // took longer than the client's 30 s once; one more attempt, everything else unchanged.
        let p2 = match open() {
            Err(RemoteError::Failed(m)) if m.contains("no answer to session.hello") => {
                open().unwrap()
            }
            r => r.unwrap(),
        };
        let timing = Timing {
            delays: vec![Duration::from_millis(300)],
            then: Duration::from_millis(500),
            heartbeat: Duration::from_secs(1),
            ..Timing::default()
        };
        let live = LiveProject::start(Box::new(p2), timing);
        let (_, text, _, mut sync) = live.open_doc("run.sh").unwrap();
        let mut editor = format!("# live\n{text}");
        live.sync_doc(&mut sync, &editor).unwrap();
        live.break_connection();
        editor.push_str("# typed during the break\n");
        let _ = live.sync_doc(&mut sync, &editor);
        let mut seen = false;
        assert!(
            wait_until(Duration::from_secs(30), || {
                seen |= matches!(live.state(), ConnState::Reconnecting { .. });
                let _ = live.sync_doc(&mut sync, &editor);
                seen && matches!(live.state(), ConnState::Connected { .. })
                    && live.pending_edits() == Some(0)
                    && sync.synced == editor
            }),
            "{:?} {:?}",
            live.state(),
            live.log()
        );
        live.save("run.sh", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join("run.sh")).unwrap(),
            editor
        );
        assert!(live.log().iter().any(|l| l.contains("Resumed")));
        live.close(Unsaved::Discard).unwrap();
    }

    // SEC-9 (started after the first open): the backend without session.hello has closed after
    // 10 s, not sooner, and said nothing.
    let installed = p.installed.path().to_owned();
    let (mut stalled, waited) = stalled;
    let (out, waited) = waited.join().unwrap();
    assert!(out.is_empty());
    assert!(
        waited >= Duration::from_secs(9) && waited < Duration::from_secs(20),
        "{waited:?}"
    );
    let _ = stalled.kill();
    let _ = stalled.wait();

    // Behaviour 4: a copy whose SHA-256 differs from this build is deleted and refused.
    let wrong = "0".repeat(64);
    let size = std::fs::metadata(binary).unwrap().len();
    let e = ferris_idea_remote::verify_backend(&remote, &installed, &wrong, size).unwrap_err();
    assert!(e.to_string().contains("does not match this build"), "{e}");
    let copy = installed.replace("$HOME", &home.display().to_string());
    assert!(!Path::new(&copy).exists(), "{copy}");

    drop(p);
    let _ = std::fs::remove_dir_all(home.join(&opts.backend_dir));
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&base);
}
