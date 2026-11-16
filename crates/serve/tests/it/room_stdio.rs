//! SPEC-0323 AK-12: a room on the real `idecraft-serve --stdio` process, over the real frames; no
//! key store is involved on this path.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use idecraft_session::{ClientSession, Transport};
use serde_json::json;

struct Serve {
    child: Child,
    session: ClientSession,
}

fn start(project: &std::path::Path, state: &std::path::Path) -> Serve {
    let mut child = Command::new(env!("CARGO_BIN_EXE_idecraft-serve"))
        .args(["serve", "--stdio", "--state-dir"])
        .arg(state)
        .arg(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("idecraft-serve starts");
    let transport = Transport {
        reader: Box::new(child.stdout.take().unwrap()),
        writer: Box::new(child.stdin.take().unwrap()),
        closer: Arc::new(|| {}),
    };
    let session = ClientSession::open(transport, &project.display().to_string(), "t", "h", None)
        .expect("session opens");
    Serve { child, session }
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_room_lives_on_the_real_process_and_survives_its_end() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("a.txt"), "hello\n").unwrap();
    let project = project.canonicalize().unwrap();

    let serve = start(&project, &state);
    let room = serve
        .session
        .call("room.create", json!({"name": "real"}))
        .unwrap();
    let share = serve
        .session
        .call("room.share", json!({"audience": "anonymous"}))
        .unwrap();
    let secret = share["link"]
        .as_str()
        .unwrap()
        .rsplit_once('#')
        .unwrap()
        .1
        .to_owned();
    serve
        .session
        .call(
            "room.apply",
            json!({"changes": [
                {"kind": "edit", "path": "a.txt", "ops": [{"at": 0, "insert": "real "}]},
                {"kind": "create", "path": "b.txt", "text": "made\n"},
            ]}),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("b.txt")).unwrap(),
        "made\n"
    );
    // The automatic save reaches the file.
    let t0 = std::time::Instant::now();
    while std::fs::read_to_string(project.join("a.txt")).unwrap() != "real hello\n" {
        assert!(t0.elapsed() < Duration::from_secs(10), "not saved");
        std::thread::sleep(Duration::from_millis(50));
    }
    let history = serve.session.call("room.history", json!({})).unwrap();
    assert_eq!(history["entries"].as_array().unwrap().len(), 2);
    // Give the pump a moment to write the journal before the process ends.
    let journal = state.join("rooms");
    let t0 = std::time::Instant::now();
    while t0.elapsed() < Duration::from_secs(5) {
        if count_lines(&journal) >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(serve);

    // Nothing stores the secret, and the room comes back with the next process.
    let mut files = Vec::new();
    walk(&state, &mut files);
    assert!(files.iter().any(|f| f.ends_with("room.json")), "{files:?}");
    for f in &files {
        let text = String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned();
        assert!(!text.contains(&secret), "{f:?} holds the link secret");
    }
    let again = start(&project, &state);
    let status = again.session.call("room.status", json!({})).unwrap();
    assert_eq!(status["id"], room["id"]);
    let history = again.session.call("room.history", json!({})).unwrap();
    assert_eq!(history["entries"].as_array().unwrap().len(), 2);
}

fn count_lines(dir: &std::path::Path) -> usize {
    let mut files = Vec::new();
    walk(dir, &mut files);
    files
        .iter()
        .filter(|f| f.ends_with("journal.jsonl"))
        .map(|f| {
            std::fs::read_to_string(f)
                .unwrap_or_default()
                .lines()
                .count()
        })
        .sum()
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.path().is_dir() {
            walk(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}
