//! SPEC-0046 AK-3, AK-5 and AK-6 against a local test server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use idecraft_http::{ResolvedRequest, SendOptions, send, send_with};

/// What the server saw: request line, headers, body.
type Seen = (String, Vec<(String, String)>, String);

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
    (request_line, headers, String::from_utf8(body).unwrap())
}

/// Serves `n` connections, answering each with the next raw response; reports what it saw.
fn server(responses: Vec<String>) -> (u16, std::sync::mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx): (Sender<Seen>, _) = channel();
    std::thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let seen = read_request(&mut stream);
            tx.send(seen).unwrap();
            let response = response.replace("{port}", &port.to_string());
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (port, rx)
}

fn request(method: &str, url: String, headers: &[(&str, &str)], body: &str) -> ResolvedRequest {
    ResolvedRequest {
        name: None,
        line: 1,
        method: method.into(),
        url,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.into(),
        unresolved: Vec::new(),
    }
}

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn get_with_content_length() {
    let (port, seen) = server(vec![
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello".into(),
    ]);
    let r = send(
        &request("GET", format!("http://127.0.0.1:{port}/a?b=1"), &[], ""),
        WAIT,
    )
    .unwrap();
    assert_eq!(
        (r.status, r.reason.as_str(), r.text.as_deref()),
        (200, "OK", Some("hello"))
    );
    assert_eq!(r.header("content-type"), Some("text/plain"));
    let (line, headers, _) = seen.recv().unwrap();
    assert_eq!(line, "GET /a?b=1 HTTP/1.1");
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    assert_eq!(get("Host"), Some(format!("127.0.0.1:{port}")));
    assert_eq!(get("User-Agent").as_deref(), Some("IDECraft"));
    assert_eq!(get("Connection").as_deref(), Some("close"));
}

#[test]
fn post_with_body_and_chunked_answer() {
    let (port, seen) = server(vec![
        "HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n6;ext=1\r\npedia \r\n0\r\nX-Trailer: 1\r\n\r\n".into(),
    ]);
    let body = "{\"name\": \"Ada\"}";
    let req = request(
        "POST",
        format!("http://127.0.0.1:{port}/users"),
        &[
            ("Content-Type", "application/json"),
            ("User-Agent", "custom"),
        ],
        body,
    );
    let r = send(&req, WAIT).unwrap();
    assert_eq!((r.status, r.text.as_deref()), (201, Some("Wikipedia ")));
    let (line, headers, got) = seen.recv().unwrap();
    assert_eq!(line, "POST /users HTTP/1.1");
    assert_eq!(got, body);
    let agents: Vec<_> = headers.iter().filter(|(k, _)| k == "User-Agent").collect();
    assert_eq!(agents.len(), 1, "a given header is not added again");
    assert!(headers.contains(&("Content-Length".into(), body.len().to_string())));
}

#[test]
fn redirects_and_errors() {
    let (port, seen) = server(vec![
        "HTTP/1.1 303 See Other\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/final\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 404 Not Found\r\nContent-Length: 7\r\n\r\nmissing".into(),
    ]);
    let req = request(
        "POST",
        format!("http://127.0.0.1:{port}/start"),
        &[],
        "data",
    );
    let r = send(&req, WAIT).unwrap();
    assert_eq!((r.status, r.text.as_deref()), (404, Some("missing")));
    assert_eq!(r.redirects.len(), 2);
    assert!(r.url.ends_with("/final"));
    let lines: Vec<String> = (0..3).map(|_| seen.recv().unwrap().0).collect();
    assert_eq!(
        lines,
        [
            "POST /start HTTP/1.1",
            "GET /next HTTP/1.1",
            "GET /final HTTP/1.1"
        ]
    );

    // A server that never answers runs into the timeout.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let _hold = std::thread::spawn(move || {
        let conn = listener.accept();
        std::thread::sleep(Duration::from_secs(3));
        drop(conn);
    });
    let req = request("GET", format!("http://127.0.0.1:{port}/"), &[], "");
    let late = send(&req, Duration::from_millis(300)).unwrap_err();
    assert!(late.to_string().starts_with("timed out"), "{late}");

    // Unresolved variables are never sent.
    let mut req = request("GET", "http://127.0.0.1:1/{{x}}".into(), &[], "");
    req.unresolved = vec!["x".into()];
    assert!(
        send(&req, WAIT)
            .unwrap_err()
            .to_string()
            .contains("unresolved")
    );
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[test]
fn redirects_keep_credentials_on_their_origin() {
    // 127.0.0.1 and localhost are two origins served by the same listener.
    let (port, seen) = server(vec![
        "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/other\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/back\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".into(),
    ]);
    let secrets = [
        ("Authorization", "Bearer test-token"),
        ("Cookie", "session=test"),
        ("Proxy-Authorization", "Basic dGVzdA=="),
        ("X-Kept", "yes"),
    ];
    let req = request(
        "GET",
        format!("http://127.0.0.1:{port}/start"),
        &secrets,
        "",
    );
    let r = send(&req, WAIT).unwrap();
    assert_eq!((r.status, r.redirects.len()), (200, 2));
    let first = seen.recv().unwrap().1;
    assert_eq!(header(&first, "Authorization"), Some("Bearer test-token"));
    for hop in ["the other origin", "back on the first origin"] {
        let headers = seen.recv().unwrap().1;
        for name in ["Authorization", "Cookie", "Proxy-Authorization"] {
            assert_eq!(header(&headers, name), None, "{name} on {hop}");
        }
        assert_eq!(header(&headers, "X-Kept"), Some("yes"));
    }
    assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
    assert!(r.notes[0].contains("Authorization, Cookie, Proxy-Authorization"));
    assert!(r.notes[0].contains(&format!("localhost:{port}")));
    assert!(!r.notes[0].contains("test-token"), "names only, no values");

    // A hand-written Host header does not follow to another origin either.
    let (port, seen) = server(vec![
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: //localhost:{port}/other\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".into(),
    ]);
    let req = request(
        "POST",
        format!("http://127.0.0.1:{port}/start"),
        &[("Host", "internal.invalid")],
        "kept body",
    );
    let r = send(&req, WAIT).unwrap();
    assert_eq!(r.url, format!("http://localhost:{port}/other"));
    assert_eq!(
        header(&seen.recv().unwrap().1, "Host"),
        Some("internal.invalid")
    );
    let (line, headers, body) = seen.recv().unwrap();
    assert_eq!(
        (line.as_str(), body.as_str()),
        ("POST /other HTTP/1.1", "kept body")
    );
    assert_eq!(
        header(&headers, "Host"),
        Some(format!("localhost:{port}").as_str())
    );

    // The same origin keeps them; a relative location is resolved, its dot segments removed.
    let (port, seen) = server(vec![
        "HTTP/1.1 302 Found\r\nLocation: ../up?q=1#part\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".into(),
    ]);
    let req = request(
        "GET",
        format!("http://127.0.0.1:{port}/a/b/c"),
        &secrets,
        "",
    );
    let r = send(&req, WAIT).unwrap();
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    let _ = seen.recv().unwrap();
    let (line, headers, _) = seen.recv().unwrap();
    assert_eq!(line, "GET /a/up?q=1 HTTP/1.1");
    assert_eq!(header(&headers, "Authorization"), Some("Bearer test-token"));
    assert_eq!(header(&headers, "Cookie"), Some("session=test"));

    // A location with another scheme is not followed, and a loop ends after five redirects.
    let (port, _seen) = server(vec![
        "HTTP/1.1 302 Found\r\nLocation: file:///etc/hosts\r\nContent-Length: 0\r\n\r\n".into(),
    ]);
    let r = send(
        &request("GET", format!("http://127.0.0.1:{port}/"), &[], ""),
        WAIT,
    )
    .unwrap();
    assert_eq!((r.status, r.redirects.len()), (302, 0));
    assert!(r.notes[0].contains("only http and https"), "{:?}", r.notes);
    let again = "HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\n\r\n".to_owned();
    let (port, seen) = server(vec![again; 6]);
    let r = send(
        &request("GET", format!("http://127.0.0.1:{port}/loop"), &[], ""),
        WAIT,
    )
    .unwrap();
    assert_eq!((r.status, r.redirects.len()), (302, 5));
    assert!(
        r.notes[0].contains("Stopped after 5 redirects"),
        "{:?}",
        r.notes
    );
    assert_eq!(seen.iter().take(6).count(), 6);

    // Without following, the first response is the result.
    let (port, _seen) = server(vec![
        "HTTP/1.1 301 Moved Permanently\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n".into(),
    ]);
    let options = SendOptions {
        timeout: WAIT,
        follow_redirects: false,
        accept_codings: false,
    };
    let r = send_with(
        &request("GET", format!("http://127.0.0.1:{port}/"), &[], ""),
        &options,
    )
    .unwrap();
    assert_eq!((r.status, r.header("location")), (301, Some("/next")));
    assert!(r.redirects.is_empty() && r.notes.is_empty());
}

#[test]
fn checked_requests_and_empty_posts() {
    let ok = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned();
    let (port, seen) = server(vec![ok; 3]);
    let url = |path: &str| format!("http://127.0.0.1:{port}{path}");
    // A POST without a body says so; a GET without one says nothing.
    send(&request("POST", url("/empty"), &[], ""), WAIT).unwrap();
    assert_eq!(header(&seen.recv().unwrap().1, "Content-Length"), Some("0"));
    send(&request("GET", url("/get"), &[], ""), WAIT).unwrap();
    assert_eq!(header(&seen.recv().unwrap().1, "Content-Length"), None);
    // The fragment stays here.
    send(&request("GET", url("/a?x=1#part"), &[], ""), WAIT).unwrap();
    assert_eq!(seen.recv().unwrap().0, "GET /a?x=1 HTTP/1.1");

    // A value that would add a header line or a second request is refused before connecting.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/");
    let smuggled = "v\r\n\r\nGET /second HTTP/1.1\r\nHost: x";
    for bad in [
        request("GET", url.clone(), &[("X-A", smuggled)], ""),
        request("GET", url.clone(), &[("X-A\r\nX-B", "v")], ""),
        request("GET", format!("{url}a\r\nX-Injected: 1"), &[], ""),
        request("GET", format!("{url}a b"), &[], ""),
        request("GET /other HTTP/1.1\r\nX:", url.clone(), &[], ""),
    ] {
        let e = send(&bad, WAIT).unwrap_err().to_string();
        assert!(e.starts_with("invalid request: "), "{e}");
    }
    assert!(listener.accept().is_err(), "nothing connected");
}

/// SPEC-0046 v6 AK-14 (OBS-0061 W1): a placeholder with spaces is one piece of the request line
/// and is generated whole; an unknown one with a space and one that is not closed keep their
/// request back, so nothing of them reaches the server.
#[test]
fn unresolved_placeholders_are_never_sent() {
    const SENDS: usize = 12;
    let ok = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_owned();
    let (port, seen) = server(vec![ok; SENDS]);
    let file = idecraft_http::parse(
        "@host = 127.0.0.1:{{port}}\n\n\
         ### unknown\nGET http://{{host}}/u?x={{$foo bar}}\n\n\
         ### not closed\nGET http://{{host}}/n\nX-Id: {{$random.integer(1,\n\n\
         ### in the body\nPOST http://{{host}}/b\nContent-Type: text/plain\n\nvalue={{ $random.nothing(1, 2) }}\n\n\
         ### OBS-0061\nGET http://{{host}}/d?u={{$uuid}}&t={{$timestamp}}&i={{$randomInt}}&r={{$random.integer(1, 5)}}\n",
    );
    let env = std::collections::BTreeMap::from([("port".to_owned(), port.to_string())]);
    let requests = idecraft_http::resolve(&file, &env);
    assert_eq!(requests[0].unresolved, ["$foo bar"]);
    assert_eq!(requests[1].unresolved, ["{{$random.integer(1,"]);
    assert_eq!(requests[2].unresolved, ["$random.nothing(1, 2)"]);
    for refused in &requests[..3] {
        let e = send(refused, WAIT).unwrap_err().to_string();
        assert!(e.contains("unresolved"), "{e}");
    }
    assert!(requests[3].unresolved.is_empty(), "{:?}", requests[3]);
    // The values are generated afresh for each send.
    for _ in 0..SENDS {
        let again = idecraft_http::resolve(&file, &env).remove(3);
        assert_eq!(send(&again, WAIT).unwrap().status, 200);
        // The server sees only these: the refused requests never connected.
        let (line, _, _) = seen.recv_timeout(WAIT).unwrap();
        let target = line
            .strip_prefix("GET /d?")
            .and_then(|t| t.strip_suffix(" HTTP/1.1"))
            .unwrap_or_else(|| panic!("{line}"));
        assert!(!target.contains('{'), "{line}");
        let r = target
            .split('&')
            .find_map(|kv| kv.strip_prefix("r="))
            .unwrap_or_else(|| panic!("{line}"));
        let r: i64 = r.parse().unwrap_or_else(|_| panic!("whole: {line}"));
        assert!((1..5).contains(&r), "{line}");
    }
    assert!(seen.try_recv().is_err(), "nothing else arrived");
}
