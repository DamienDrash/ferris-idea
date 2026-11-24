//! SPEC-0046 AK-10: the request files of `tests/precheck/requests/` against `tests/precheck/server.py`
//! (Python standard library, 127.0.0.1 only, a port the system chooses). Every request of every file
//! has its expected outcome here; a request without one fails the test. Skipped without `python3`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ferris_idea_http::{
    Response, SendOptions, finish_body, load_environments, parse, resolve, send_with,
};
use serde_json::Value;

/// The test server; ended with the test.
struct Server {
    child: Child,
    port: String,
}

impl Server {
    fn start(script: &Path, arguments: &[&std::ffi::OsStr]) -> Option<Self> {
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(script)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut line)
            .ok()?;
        let address = line
            .trim()
            .strip_prefix("listening on http://127.0.0.1:")
            .or_else(|| line.trim().strip_prefix("listening on https://127.0.0.1:"));
        match address {
            Some(port) => Some(Self {
                port: port.to_owned(),
                child,
            }),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What became of one request.
enum Outcome {
    /// Not sent: unresolved names or a refused body file.
    NotSent(String),
    /// Sent, no response.
    Failed(String),
    /// A response, with the notes of the request file.
    Answer(Box<Response>, Vec<String>),
}

fn requests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/precheck/requests")
}

/// Runs every request of `file` in the environment `local` on `port`; outcomes by request name.
fn run(file: &str, port: &str) -> Vec<(String, Outcome)> {
    let dir = requests_dir();
    let text = std::fs::read_to_string(dir.join(file)).unwrap();
    let parsed = parse(&text);
    let mut variables = load_environments(&dir, &dir).get("local").unwrap();
    variables.insert("port".into(), port.to_owned());
    let resolved = resolve(&parsed, &variables);
    parsed
        .requests
        .iter()
        .zip(resolved)
        .map(|(raw, mut request)| {
            let name = raw.name.clone().unwrap_or_default();
            if !request.unresolved.is_empty() {
                let names = request.unresolved.join(", ");
                return (name, Outcome::NotSent(format!("unresolved: {names}")));
            }
            if let Err(e) = finish_body(&mut request, &dir, &dir) {
                return (name, Outcome::NotSent(e));
            }
            let options = SendOptions {
                timeout: Duration::from_secs(raw.settings.timeout_secs.unwrap_or(10)),
                follow_redirects: !raw.settings.no_redirect,
                accept_codings: true,
            };
            let outcome = match send_with(&request, &options) {
                Ok(r) => Outcome::Answer(Box::new(r), raw.notes.clone()),
                Err(e) => Outcome::Failed(e.to_string()),
            };
            (name, outcome)
        })
        .collect()
}

/// What the server's `/echo` saw.
struct Echo(Value);

impl Echo {
    fn of(response: &Response) -> Self {
        Self(serde_json::from_str(response.text.as_deref().unwrap_or("null")).unwrap())
    }
    fn method(&self) -> &str {
        self.0["method"].as_str().unwrap_or_default()
    }
    fn target(&self) -> &str {
        self.0["target"].as_str().unwrap_or_default()
    }
    fn body(&self) -> &str {
        self.0["body"].as_str().unwrap_or_default()
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.0["headers"].as_array()?.iter().find_map(|pair| {
            pair[0]
                .as_str()
                .is_some_and(|k| k.eq_ignore_ascii_case(name))
                .then(|| pair[1].as_str())
                .flatten()
        })
    }
}

/// Checks all outcomes of `file` with `check`, which returns false for a request it does not know.
fn check_file(file: &str, port: &str, check: impl Fn(&str, &Outcome) -> bool) {
    let outcomes = run(file, port);
    assert!(!outcomes.is_empty(), "{file} has no requests");
    for (name, outcome) in &outcomes {
        assert!(
            check(name, outcome),
            "{file}: no expectation for the request {name:?}"
        );
    }
}

fn answer<'a>(file: &str, name: &str, outcome: &'a Outcome) -> (&'a Response, &'a [String]) {
    match outcome {
        Outcome::Answer(r, notes) => (r, notes),
        Outcome::NotSent(e) => panic!("{file} {name:?} was not sent: {e}"),
        Outcome::Failed(e) => panic!("{file} {name:?} failed: {e}"),
    }
}

#[test]
fn the_precheck_files_run_against_the_python_server() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/precheck/server.py");
    let Some(server) = Server::start(&script, &[]) else {
        eprintln!(
            "skipped: python3 is not available to run {}",
            script.display()
        );
        return;
    };
    let port = server.port.as_str();
    let here = format!("127.0.0.1:{port}");
    let other = format!("localhost:{port}");

    let file = "01-basics.http";
    check_file(file, port, |name, outcome| {
        let (r, notes) = answer(file, name, outcome);
        assert!(notes.is_empty() && r.notes.is_empty(), "{name}: {notes:?}");
        match name {
            "get with headers" => {
                let e = Echo::of(r);
                assert_eq!((e.method(), e.target()), ("GET", "/echo/basics"));
                assert_eq!(e.header("X-Request-Name"), Some("get with headers"));
                assert_eq!(e.header("X-After-Comment"), Some("yes"));
                assert_eq!(e.header("Host"), Some(here.as_str()));
                assert_eq!(e.header("User-Agent"), Some("FerrisIDEA"));
            }
            "query over several lines" => assert_eq!(
                Echo::of(r).target(),
                "/echo/query?page=2&size=10&q=two%20words"
            ),
            "named-by-comment" => {
                let e = Echo::of(r);
                assert_eq!(e.method(), "POST");
                let sent: Value = serde_json::from_str(e.body()).unwrap();
                assert_eq!(sent["nested"]["n"], 1);
                assert_eq!(e.header("Content-Length"), Some("63"));
            }
            "form body" => assert_eq!(Echo::of(r).body(), "name=Ada+Lovelace&lang=en&note=a%26b"),
            "bare url means get" => assert_eq!(Echo::of(r).method(), "GET"),
            "put" => assert_eq!(
                (Echo::of(r).method(), Echo::of(r).body()),
                ("PUT", "replaced")
            ),
            "patch" => assert_eq!(Echo::of(r).method(), "PATCH"),
            "delete" => {
                assert_eq!(Echo::of(r).method(), "DELETE");
                assert_eq!(Echo::of(r).header("Content-Length"), None);
            }
            "options" => assert_eq!(Echo::of(r).method(), "OPTIONS"),
            "head has no body" => assert_eq!((r.status, r.size), (200, 0)),
            "no content" => assert_eq!((r.status, r.size), (204, 0)),
            "not found" => assert_eq!(r.status, 404),
            "post without a body" => {
                assert_eq!(Echo::of(r).header("Content-Length"), Some("0"))
            }
            "fragment is not sent" => assert_eq!(Echo::of(r).target(), "/echo/fragment?x=1"),
            "body with comment-like lines" => assert_eq!(
                Echo::of(r).body(),
                "first line\n# not a comment inside a body\n// neither is this\nlast line"
            ),
            _ => return false,
        }
        true
    });

    let file = "02-variables.http";
    check_file(file, port, |name, outcome| {
        if let Outcome::NotSent(why) = outcome {
            match name {
                "missing variable is not sent" => assert_eq!(why, "unresolved: does_not_exist"),
                "nested object is not a variable" => assert_eq!(why, "unresolved: nested"),
                _ => panic!("{name}: {why}"),
            }
            return true;
        }
        let (r, _) = answer(file, name, outcome);
        let e = Echo::of(r);
        match name {
            "environment values" => {
                assert_eq!(e.target(), "/echo/env");
                assert_eq!(e.header("X-Where"), Some("public-local"));
                assert_eq!(e.header("X-Flag"), Some("true"));
                assert_eq!(e.header("X-Port"), Some(port));
            }
            "private file wins over public file" => {
                assert_eq!(e.header("X-Both"), Some("from-private-file"))
            }
            "file variable wins over environment" => {
                assert_eq!(e.header("X-Greeting"), Some("file-greeting"))
            }
            "variable refers to variables" => {
                assert_eq!(e.header("X-Composed"), Some("echo-public-local"))
            }
            "private token in a header" => assert_eq!(
                e.header("Authorization"),
                Some("Bearer precheck-private-token")
            ),
            "private token in the url" => assert_eq!(
                e.target(),
                "/echo/token-in-url?access_token=precheck-private-token"
            ),
            "spaces inside the braces" => assert_eq!(e.header("X-Where"), Some("public-local")),
            _ => return false,
        }
        true
    });

    let file = "03-bodies.http";
    check_file(file, port, |name, outcome| {
        if let Outcome::NotSent(why) = outcome {
            let rule = match name {
                "body file outside the project is refused" => "outside the project",
                "body file with an absolute path is refused" => "relative to the request file",
                "body file that does not exist" => "no-such-file.txt",
                "private environment file is never a body" => "never sent",
                _ => panic!("{name}: {why}"),
            };
            assert!(why.contains(rule), "{name}: {why}");
            return true;
        }
        let (r, _) = answer(file, name, outcome);
        let e = Echo::of(r);
        match name {
            "json" => assert_eq!(e.body(), "{\"id\": 7, \"text\": \"zwölf ✓\"}"),
            "form over several lines" => assert_eq!(e.body(), "name=Ada&lang=en"),
            "multipart written by hand" => {
                assert!(e.body().starts_with(
                    "--precheck-boundary\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nA title\r\n--precheck-boundary\r\n"
                ));
                assert!(e.body().ends_with("\r\nfile text\r\n--precheck-boundary--\r\n"));
                assert!(!e.body().replace("\r\n", "").contains('\n'));
            }
            "body from a file beside the request file" => {
                assert_eq!(e.body(), "{\"from\": \"payload.json\", \"n\": 1}\n")
            }
            "body from a file in a sub-folder" => {
                assert_eq!(e.body(), "{\"from\": \"nested/payload.json\"}\n")
            }
            "multipart with a part from a file" => assert!(
                e.body().ends_with(
                    "Content-Type: application/json\r\n\r\n{\"from\": \"payload.json\", \"n\": 1}\r\n--precheck-boundary--\r\n"
                ),
                "{:?}",
                e.body()
            ),
            _ => return false,
        }
        true
    });

    let file = "04-responses.http";
    check_file(file, port, |name, outcome| {
        if let Outcome::Failed(why) = outcome {
            match name {
                "nothing listens" => assert!(why.contains("cannot connect"), "{why}"),
                "not a url" => assert!(why.contains("invalid URL"), "{why}"),
                _ => panic!("{name}: {why}"),
            }
            return true;
        }
        let (r, _) = answer(file, name, outcome);
        let text = r.text.as_deref();
        match name {
            "chunked" => assert_eq!(text, Some("{\"chunks\": 3, \"ok\": true}")),
            // Asked for by the request, by Ferris IDEA (no header in the file), or sent unasked:
            // decoded each time (behaviour 12).
            "gzip when accepted" | "gzip asked for by the client" | "gzip forced" => {
                assert_eq!(r.header("content-encoding"), Some("gzip"));
                assert!(text.unwrap().starts_with("{\"gzipped\": true"));
                let d = r.decoded.as_ref().unwrap();
                assert!(d.complete() && d.codings == ["gzip"]);
                assert!(r.size < d.size && r.body.starts_with(&[0x1f, 0x8b]));
                assert_eq!(r.notes, [d.note(r.size)]);
            }
            "gzip refused" => {
                assert_eq!(r.header("content-encoding"), None);
                assert!(text.unwrap().starts_with("{\"gzipped\": true"));
                assert!(r.decoded.is_none() && r.notes.is_empty());
            }
            "slow two seconds" => assert!(r.millis >= 2000 && r.status == 200),
            "large one megabyte" => assert_eq!((r.size, r.body.len()), (1_048_576, 1_048_576)),
            "binary" => {
                assert_eq!((r.size, text), (256, None));
                assert_eq!(r.body, (0..=255).collect::<Vec<u8>>());
            }
            "json labelled text" => {
                assert_eq!(r.header("content-type"), Some("text/plain"));
                assert_eq!(text, Some("{\"labelled\": \"text/plain\"}"));
            }
            "no content type" => {
                assert_eq!(r.header("content-type"), None);
                assert_eq!(text, Some("{\"labelled\": null}"));
            }
            "broken json" => assert_eq!(text, Some("{\"broken\": ")),
            "latin1 text" => assert_eq!(text, Some("Grüße")),
            "close delimited" => assert_eq!(text, Some("ends when the connection closes")),
            "interim response" => assert_eq!((r.status, text), (200, Some("{\"after\": \"100\"}"))),
            "server error" => assert_eq!(r.status, 500),
            "unauthorised" => {
                assert_eq!(r.status, 401);
                assert!(r.header("www-authenticate").is_some());
            }
            _ => return false,
        }
        true
    });

    let file = "05-redirects.http";
    check_file(file, port, |name, outcome| {
        let (r, _) = answer(file, name, outcome);
        let final_url = format!("http://{here}/echo");
        match name {
            "301 get" | "302 get" => {
                assert_eq!((r.status, r.redirects.len()), (200, 1));
                assert_eq!(r.url, final_url);
                assert_eq!(Echo::of(r).method(), "GET");
            }
            "303 post becomes get" | "301 post becomes get" => {
                let e = Echo::of(r);
                assert_eq!((e.method(), e.body()), ("GET", ""));
                assert_eq!(e.header("Content-Type"), None);
            }
            "307 post stays post" => {
                let e = Echo::of(r);
                assert_eq!((e.method(), e.body()), ("POST", "{\"kept\": true}"));
            }
            "308 put stays put" => {
                let e = Echo::of(r);
                assert_eq!((e.method(), e.body()), ("PUT", "{\"kept\": true}"));
            }
            "chain" => assert_eq!((r.status, r.redirects.len()), (200, 2)),
            "loop ends" => {
                assert_eq!((r.status, r.redirects.len()), (302, 5));
                assert!(
                    r.notes[0].contains("Stopped after 5 redirects"),
                    "{:?}",
                    r.notes
                );
            }
            "relative location" => {
                assert_eq!(r.status, 200);
                assert_eq!(Echo::of(r).target(), "/echo?from=relative");
            }
            "other host keeps no credentials" => {
                let e = Echo::of(r);
                assert_eq!(e.header("Host"), Some(other.as_str()));
                for kept_back in ["Authorization", "Cookie", "Proxy-Authorization"] {
                    assert_eq!(
                        e.header(kept_back),
                        None,
                        "{kept_back} reached another origin"
                    );
                }
                assert_eq!(e.header("X-Kept"), Some("yes"));
                assert_eq!(r.notes.len(), 1);
                let note = &r.notes[0];
                assert!(
                    note.contains("Authorization, Cookie, Proxy-Authorization")
                        && note.contains(&other),
                    "{note}"
                );
                assert!(
                    !note.contains("precheck-private-token"),
                    "no value in the note"
                );
            }
            "other host 307 keeps the body" => {
                let e = Echo::of(r);
                assert_eq!((e.method(), e.body()), ("POST", "body for the other host"));
                assert_eq!(e.header("Authorization"), None);
                assert_eq!(e.header("Content-Type"), Some("text/plain"));
            }
            "same host keeps credentials" => {
                assert_eq!(
                    Echo::of(r).header("Authorization"),
                    Some("Bearer precheck-private-token")
                );
                assert!(r.notes.is_empty());
            }
            "scheme relative location" => {
                let e = Echo::of(r);
                assert_eq!(r.url, format!("http://{other}/echo"));
                assert_eq!(e.header("Authorization"), None);
            }
            "no location" => {
                assert_eq!((r.status, r.redirects.len()), (302, 0));
                assert!(r.notes.is_empty());
            }
            "location with another scheme is not followed" => {
                assert_eq!((r.status, r.redirects.len()), (302, 0));
                assert!(r.notes[0].contains("file:///etc/hosts"), "{:?}", r.notes);
                assert!(r.notes[0].contains("not followed"));
            }
            _ => return false,
        }
        true
    });

    let file = "06-cookies.http";
    check_file(file, port, |name, outcome| {
        let (r, _) = answer(file, name, outcome);
        match name {
            "set cookies" => {
                let cookies: Vec<&str> = r
                    .headers
                    .iter()
                    .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
                    .map(|(_, v)| v.as_str())
                    .collect();
                assert_eq!(r.status, 204);
                assert_eq!(cookies.len(), 2, "both Set-Cookie headers are kept apart");
            }
            "cookies are not kept between requests" => {
                assert_eq!(r.text.as_deref(), Some("{\"cookie\": null}"))
            }
            "cookie header written by hand" => assert_eq!(
                r.text.as_deref(),
                Some("{\"cookie\": \"session=by-hand; theme=light\"}")
            ),
            _ => return false,
        }
        true
    });

    let file = "07-dialect.http";
    let saved = requests_dir().join("saved-response.json");
    check_file(file, port, |name, outcome| {
        match (name, outcome) {
            ("unknown dynamic variable is not sent", Outcome::NotSent(why)) => {
                assert_eq!(why, "unresolved: $doesNotExist");
                return true;
            }
            ("timeout setting", Outcome::Failed(why)) => {
                assert!(why.starts_with("timed out"), "{why}");
                return true;
            }
            _ => {}
        }
        let (r, notes) = answer(file, name, outcome);
        match name {
            "dynamic variables" => {
                let e = Echo::of(r);
                let uuid = e.header("X-Uuid").unwrap();
                assert_eq!(uuid.len(), 36);
                assert_eq!(&uuid[14..15], "4");
                assert_ne!(e.header("X-Other-Uuid"), Some(uuid));
                assert!(e.header("X-Timestamp").unwrap().parse::<u64>().unwrap() > 1_700_000_000);
                assert!(e.header("X-Iso").unwrap().ends_with('Z'));
                assert!(e.header("X-Random").unwrap().parse::<u32>().unwrap() <= 1000);
            }
            "header continued on the next line" => {
                let e = Echo::of(r);
                assert_eq!(e.header("X-Long"), Some("first part, second part"));
                assert_eq!(e.header("X-Next"), Some("yes"));
            }
            "no-redirect setting" => {
                assert_eq!((r.status, r.redirects.len()), (302, 0));
                assert_eq!(r.header("location"), Some("/echo"));
            }
            "response handler is not run" => {
                let e = Echo::of(r);
                assert_eq!((e.method(), e.body()), ("GET", ""));
                assert_eq!(
                    e.header("Content-Length"),
                    None,
                    "nothing of the script is sent"
                );
                assert_eq!(notes.len(), 1);
                assert!(notes[0].contains("not run"));
            }
            "handler over several lines after a body" => {
                assert_eq!(Echo::of(r).body(), "the body stays");
                assert!(notes[0].contains("not run"));
            }
            "response is not saved to a file" => {
                assert_eq!(Echo::of(r).body(), "");
                assert!(notes[0].contains("./saved-response.json"), "{notes:?}");
                assert!(!saved.exists(), "a response is never written to a file");
            }
            _ => return false,
        }
        true
    });

    // The environment files: what is private is known by name, and masked for a log.
    let env = load_environments(&requests_dir(), &requests_dir());
    let private: BTreeMap<String, String> = env.private("local");
    assert_eq!(
        private.keys().map(String::as_str).collect::<Vec<_>>(),
        ["both", "token"]
    );
    assert_eq!(
        ferris_idea_http::mask_for_log(
            &format!("http://{here}/echo/token-in-url?access_token=precheck-private-token"),
            &private
        ),
        format!("http://{here}/echo/token-in-url?access_token={{{{token}}}}")
    );
}

/// A certificate nobody vouches for is refused and nothing is sent (`SPEC-0046` behaviour 4: TLS
/// through the system library). The key and certificate are made by `openssl` in a temporary
/// folder; no trust store is touched and no check is switched off. Skipped without `python3` or
/// `openssl`.
#[test]
fn a_self_signed_server_is_not_trusted() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/precheck/server.py");
    let dir = std::env::temp_dir().join(format!("ferris-idea-http-tls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (key, cert) = (dir.join("key.pem"), dir.join("cert.pem"));
    let made = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
        ])
        .args(["-subj", "/CN=127.0.0.1", "-keyout"])
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("skipped: openssl is not available to make a test certificate");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let arguments = [
        "--tls-cert".as_ref(),
        cert.as_os_str(),
        "--tls-key".as_ref(),
        key.as_os_str(),
    ];
    let Some(server) = Server::start(&script, &arguments) else {
        eprintln!(
            "skipped: python3 is not available to run {}",
            script.display()
        );
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    let request = ferris_idea_http::ResolvedRequest {
        name: None,
        line: 1,
        method: "GET".into(),
        url: format!("https://127.0.0.1:{}/echo", server.port),
        headers: vec![("Authorization".into(), "Bearer never-sent".into())],
        body: String::new(),
        unresolved: Vec::new(),
    };
    let options = SendOptions {
        timeout: Duration::from_secs(10),
        follow_redirects: true,
        accept_codings: true,
    };
    let refused = send_with(&request, &options).unwrap_err().to_string();
    assert!(refused.starts_with("TLS with 127.0.0.1"), "{refused}");
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}
