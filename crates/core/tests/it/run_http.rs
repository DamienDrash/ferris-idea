//! SPEC-0047 v6 AK-16 and SPEC-0087 v11 AK-13: HTTP request configurations against a server on
//! 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, channel};

use ferris_idea_core::{
    RunConfig, RunEvent, parse_run_configs, run_one, run_one_until, run_plan, sends_http, start_run,
};

/// What the server saw of one request: request line and headers.
type Seen = (String, Vec<(String, String)>);

fn read_request(stream: &mut TcpStream) -> Seen {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let request_line = line.trim_end().to_owned();
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':').unwrap();
        headers.push((k.trim().to_owned(), v.trim().to_owned()));
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    (request_line, headers)
}

/// Answers each connection with the next response, on 127.0.0.1 only.
fn server(responses: Vec<&'static str>) -> (u16, Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let seen = read_request(&mut stream);
            let _ = tx.send(seen);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, rx)
}

const USERS: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 32\r\nConnection: close\r\n\r\n{\"token\":\"secret-123\",\"n\":[1,2]}";
const ME: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nme";
const MISSING: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// A throwaway project with a request file and environment files for `port`.
struct Project(PathBuf);

impl Project {
    fn new(tag: &str, port: u16, environments: &[&str]) -> Self {
        let p =
            std::env::temp_dir().join(format!("ferris-idea-run-http-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("http")).unwrap();
        std::fs::write(
            p.join("http/api.http"),
            "### Liste\nGET {{host}}/v1/users\nAuthorization: Bearer {{token}}\n\n### Ich\nGET {{host}}/v1/me\n\n### Fehlt\nGET {{host}}/v1/missing\n\n### Offen\nGET {{host}}/v1/{{missing}}\n",
        )
        .unwrap();
        let shared: Vec<String> = environments
            .iter()
            .map(|e| format!("\"{e}\": {{ \"host\": \"127.0.0.1:{port}\" }}"))
            .collect();
        std::fs::write(
            p.join("http/http-client.env.json"),
            format!("{{ {} }}", shared.join(", ")),
        )
        .unwrap();
        let private: Vec<String> = environments
            .iter()
            .map(|e| format!("\"{e}\": {{ \"token\": \"secret-123\" }}"))
            .collect();
        std::fs::write(
            p.join("http/http-client.private.env.json"),
            format!("{{ {} }}", private.join(", ")),
        )
        .unwrap();
        Self(p.canonicalize().unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Every file of the project, relative.
    fn files(&self) -> Vec<String> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.push(p.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.0, &self.0, &mut out);
        out.sort();
        out
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One configuration with `http` from the text of its table.
fn config(http: &str) -> RunConfig {
    let parsed = parse_run_configs(&format!("[[run]]\nname = \"api\"\nhttp = {{ {http} }}\n"));
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    parsed.configs.into_iter().next().unwrap()
}

#[test]
fn http_configurations_load_and_their_problems_name_them() {
    let ok = parse_run_configs(
        "[[run]]\nname = \"api\"\nhttp = { file = \"http/api.http\", request = \"Liste\", environment = \"local\" }\n",
    );
    assert!(ok.problems.is_empty(), "{:?}", ok.problems);
    assert!(ok.configs[0].command.is_empty());
    for (table, extra, problem) in [
        ("file = \"../api.http\"", "", "leaves the workspace"),
        ("file = \"/tmp/api.http\"", "", "leaves the workspace"),
        ("file = \"notes.txt\"", "", "is no .http or .rest file"),
        (
            "file = \"a.http\", request = \" \"",
            "",
            "the HTTP request is empty",
        ),
        (
            "file = \"a.http\", environment = \"\"",
            "",
            "the HTTP environment is empty",
        ),
        (
            "file = \"a.http\"",
            "command = [\"echo\"]\n",
            "has no command",
        ),
        (
            "file = \"a.http\"",
            "url = \"http://127.0.0.1/\"\n",
            "opens no page",
        ),
        ("file = \"a.http\"", "kind = \"test\"\n", "is no test"),
    ] {
        let bad = parse_run_configs(&format!(
            "[[run]]\nname = \"bad\"\n{extra}http = {{ {table} }}\n"
        ));
        assert!(
            bad.problems
                .iter()
                .any(|p| p.starts_with("bad: ") && p.contains(problem)),
            "{table} {extra}: {:?}",
            bad.problems
        );
    }
    let unknown = parse_run_configs(
        "[[run]]\nname = \"bad\"\nhttp = { file = \"a.http\", method = \"GET\" }\n",
    );
    assert!(!unknown.problems.is_empty(), "unknown keys are refused");
}

#[test]
fn a_request_by_name_by_number_and_all_requests() {
    let (port, seen) = server(vec![USERS, ME, USERS, ME, MISSING]);
    let project = Project::new("send", port, &["local"]);
    let before = project.files();

    let by_name = run_one(
        project.path(),
        &config("file = \"http/api.http\", request = \"Liste\", environment = \"local\""),
    );
    assert_eq!(by_name.start_error, None);
    assert_eq!(by_name.exit_code, Some(0), "{}", by_name.stderr);
    let out = &by_name.stdout;
    assert!(out.contains("HTTP environment: local"), "{out}");
    assert!(out.contains("### Liste"), "{out}");
    assert!(
        out.contains(&format!("GET http://127.0.0.1:{port}/v1/users")),
        "{out}"
    );
    assert!(out.contains("HTTP 200 OK · "), "{out}");
    assert!(
        out.to_ascii_lowercase()
            .contains("content-type: application/json"),
        "{out}"
    );
    assert!(
        out.contains("\n  \"n\": [\n"),
        "the JSON body is indented: {out}"
    );
    assert!(
        !out.contains("secret-123"),
        "no private value in the output: {out}"
    );
    assert!(out.contains("{{token}}"), "{out}");
    let (line, headers) = seen.recv().unwrap();
    assert_eq!(line, "GET /v1/users HTTP/1.1");
    assert!(
        headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == "Bearer secret-123"),
        "the real value is sent: {headers:?}"
    );

    // The only environment is used without a name; request 2 is `Ich`.
    let by_number = run_one(
        project.path(),
        &config("file = \"http/api.http\", request = \"2\""),
    );
    assert_eq!(by_number.exit_code, Some(0), "{}", by_number.stderr);
    assert!(by_number.stdout.contains("### Ich"));
    assert_eq!(seen.recv().unwrap().0, "GET /v1/me HTTP/1.1");

    // All requests in file order; the unresolved one is not sent, the 404 counts as answered.
    let all = run_one(project.path(), &config("file = \"http/api.http\""));
    let lines: Vec<String> = (0..3).map(|_| seen.recv().unwrap().0).collect();
    assert_eq!(
        lines,
        [
            "GET /v1/users HTTP/1.1",
            "GET /v1/me HTTP/1.1",
            "GET /v1/missing HTTP/1.1"
        ]
    );
    assert!(all.stdout.contains("HTTP 404 Not Found · ") && all.stdout.contains("client error"));
    assert!(
        all.stderr
            .contains("Not sent: unresolved variables: missing"),
        "{}",
        all.stderr
    );
    assert_eq!(all.exit_code, Some(1), "a request was not sent");
    assert_eq!(
        project.files(),
        before,
        "nothing is written into the project"
    );
}

#[test]
fn a_404_alone_ends_with_0_and_a_refused_connection_with_1() {
    let (port, seen) = server(vec![MISSING]);
    let project = Project::new("status", port, &["local"]);
    let missing = run_one(
        project.path(),
        &config("file = \"http/api.http\", request = \"Fehlt\""),
    );
    assert_eq!(missing.exit_code, Some(0), "{}", missing.stderr);
    assert!(missing.stdout.contains("client error"));
    assert_eq!(seen.recv().unwrap().0, "GET /v1/missing HTTP/1.1");

    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = closed.local_addr().unwrap().port();
    drop(closed);
    let project = Project::new("refused", dead, &["local"]);
    let refused = run_one(
        project.path(),
        &config("file = \"http/api.http\", request = \"Ich\""),
    );
    assert_eq!(refused.start_error, None);
    assert_eq!(refused.exit_code, Some(1));
    assert!(
        !refused.stderr.trim().is_empty(),
        "the error is in the output"
    );
}

#[test]
fn several_environments_and_none_named_send_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let project = Project::new("choose", port, &["local", "staging"]);
    let out = run_one(
        project.path(),
        &config("file = \"http/api.http\", request = \"Ich\""),
    );
    let why = out.start_error.unwrap_or_default();
    assert!(
        why.contains("choose an HTTP environment: local, staging"),
        "{why}"
    );
    assert!(out.stdout.is_empty());
    assert!(listener.accept().is_err(), "nothing reached the server");

    // Named, it is sent.
    listener.set_nonblocking(false).unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        read_request(&mut s);
        s.write_all(ME.as_bytes()).unwrap();
    });
    let named = run_one(
        project.path(),
        &config("file = \"http/api.http\", request = \"Ich\", environment = \"staging\""),
    );
    assert_eq!(
        named.exit_code,
        Some(0),
        "{:?} {}",
        named.start_error,
        named.stderr
    );
}

#[test]
fn refusals_before_anything_is_sent() {
    let project = Project::new("refuse", 9, &["local"]);
    for (table, why) in [
        (
            "file = \"http/api.http\", request = \"Nope\"",
            "has no request Nope",
        ),
        (
            "file = \"http/api.http\", environment = \"prod\"",
            "no HTTP environment prod; the environment files define: local",
        ),
        (
            "file = \"http/none.http\"",
            "the request file http/none.http",
        ),
    ] {
        let out = run_one(project.path(), &config(table));
        assert!(
            out.start_error.as_deref().is_some_and(|e| e.contains(why)),
            "{table}: {:?}",
            out.start_error
        );
        assert_eq!(out.exit_code, None);
    }
    #[cfg(unix)]
    {
        let outside = Project::new("refuse-outside", 9, &["local"]);
        std::os::unix::fs::symlink(
            outside.path().join("http/api.http"),
            project.path().join("http/link.http"),
        )
        .unwrap();
        let out = run_one(project.path(), &config("file = \"http/link.http\""));
        assert!(
            out.start_error
                .as_deref()
                .is_some_and(|e| e.contains("lies outside the project")),
            "{:?}",
            out.start_error
        );
    }
}

#[test]
fn stop_before_the_start_sends_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let project = Project::new("stop", port, &["local"]);
    let out = run_one_until(
        project.path(),
        &config("file = \"http/api.http\""),
        &AtomicBool::new(true),
    );
    assert!(out.stopped);
    assert_eq!(out.exit_code, None);
    assert!(listener.accept().is_err(), "nothing reached the server");
}

#[test]
fn the_streamed_run_reports_the_requests() {
    let (port, _seen) = server(vec![ME]);
    let project = Project::new("stream", port, &["local"]);
    let configs = parse_run_configs(
        "[[run]]\nname = \"api\"\nhttp = { file = \"http/api.http\", request = \"Ich\", environment = \"local\" }\n",
    );
    assert!(sends_http(project.path(), &configs, "api"));
    let plan = run_plan(project.path(), &configs, "api").unwrap();
    assert_eq!(
        plan[0].command,
        [
            "HTTP request",
            "http/api.http",
            "request Ich",
            "environment local"
        ]
    );
    assert_eq!(plan[0].http, configs.configs[0].http);

    let events: Vec<RunEvent> = start_run(project.path(), &configs, "api")
        .unwrap()
        .events
        .iter()
        .collect();
    assert!(
        matches!(&events[0], RunEvent::Started { command, .. } if command[0] == "HTTP request"),
        "{events:?}"
    );
    let output: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Output {
                stderr: false,
                text,
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(output.contains(&"### Ich"), "{output:?}");
    assert!(
        output.iter().any(|l| l.starts_with("HTTP 200 OK")),
        "{output:?}"
    );
    assert!(output.contains(&"me"), "{output:?}");
    match events.last() {
        Some(RunEvent::Finished(o)) => assert_eq!(o.exit_code, Some(0)),
        other => panic!("{other:?}"),
    }

    // A refusal goes to the error stream of a run that does start.
    let open = parse_run_configs(
        "[[run]]\nname = \"open\"\nhttp = { file = \"http/api.http\", request = \"Offen\" }\n",
    );
    let events: Vec<RunEvent> = start_run(project.path(), &open, "open")
        .unwrap()
        .events
        .iter()
        .collect();
    assert!(
        events.iter().any(|e| matches!(e, RunEvent::Output { stderr: true, text } if text.contains("unresolved variables: missing"))),
        "{events:?}"
    );
}
