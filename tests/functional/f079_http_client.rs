//! F-079 · HTTP client (request files): send requests from text files (SPEC-0046 AK-1, AK-2, AK-4,
//! SPEC-0053 AK-5).

use std::io::{Read, Write};

use crate::harness::*;
use serde_json::json;

#[test]
fn reference_request_file_is_parsed() {
    let p = Project::reference("http-requests");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("http_requests", json!({"path": "stock.http"}));
    let all = text(&r);
    assert!(all.contains("http://127.0.0.1:8089/stock/value"), "{r}");
    assert!(all.contains("/stock/missing"), "{r}");
}

#[test]
fn unresolved_variables_are_never_sent_and_sending_needs_the_http_right() {
    let p = Project::empty();
    p.write("a.http", "### Missing\nGET http://{{host}}/x\n");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("http_requests", json!({"path": "a.http"}));
    assert!(text(&r).contains("host"), "unresolved names listed: {r}");
    ide.refused(
        "send_http_request",
        json!({"path": "a.http", "request": "Missing", "dry_run": false}),
    );
}

#[test]
fn a_post_reaches_a_local_server_and_the_answer_comes_back() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 4096];
        let mut got = String::new();
        while !got.contains("\"qty\"") {
            let n = s.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        let body = "{\"ok\":true}";
        write!(s, "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        got
    });
    let p = Project::empty();
    p.write(
        "a.http",
        &format!("@host = 127.0.0.1:{port}\n\n### Add\nPOST http://{{{{host}}}}/stock\nContent-Type: application/json\n\n{{\"qty\": 5}}\n"),
    )
    .grant(&["http"]);
    let mut ide = Ide::stdio(&p);
    let r = ide.ok(
        "send_http_request",
        json!({"path": "a.http", "request": "Add", "dry_run": false}),
    );
    let all = text(&r);
    assert!(all.contains("201") && all.contains("ok"), "{r}");
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /stock"), "{request}");
}
