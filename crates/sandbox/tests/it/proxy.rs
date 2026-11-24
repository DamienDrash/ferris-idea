//! SPEC-0347 AK-3: the allowlist proxy.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use ferris_idea_sandbox::{AllowlistProxy, Policy};

fn ask(proxy: &AllowlistProxy, request: &str) -> (String, TcpStream) {
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") && s.read(&mut byte).unwrap() == 1 {
        buf.push(byte[0]);
    }
    let line = String::from_utf8_lossy(&buf)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    (line, s)
}

#[test]
fn proxy_tunnels_only_allowed_destinations() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = target.local_addr().unwrap().port();
    let echo = std::thread::spawn(move || {
        let (mut c, _) = target.accept().unwrap();
        let mut b = [0u8; 4];
        c.read_exact(&mut b).unwrap();
        c.write_all(&b).unwrap();
    });
    let rules = Policy {
        hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        ..Policy::default()
    }
    .validate(None)
    .unwrap()
    .hosts()
    .to_vec();
    let proxy = AllowlistProxy::start(rules).unwrap();

    let (line, mut tunnel) = ask(
        &proxy,
        &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n"),
    );
    assert!(line.contains(" 200 "), "{line}");
    tunnel.write_all(b"ping").unwrap();
    let mut back = [0u8; 4];
    tunnel.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"ping");
    echo.join().unwrap();

    let other = port.wrapping_add(1).max(1);
    let (line, _) = ask(
        &proxy,
        &format!("CONNECT 127.0.0.1:{other} HTTP/1.1\r\n\r\n"),
    );
    assert!(line.contains(" 403 "), "{line}");
    let (line, _) = ask(&proxy, "CONNECT example.com:443 HTTP/1.1\r\n\r\n");
    assert!(line.contains(" 403 "), "{line}");
    // Listed, but the name leads to the machine itself.
    let (line, _) = ask(
        &proxy,
        &format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n"),
    );
    assert!(line.contains(" 403 "), "{line}");
    let (line, _) = ask(
        &proxy,
        &format!("GET http://127.0.0.1:{port}/ HTTP/1.1\r\n\r\n"),
    );
    assert!(line.contains(" 405 "), "{line}");

    let d = proxy.decisions();
    let allowed: Vec<bool> = d.iter().map(|d| d.allowed).collect();
    assert_eq!(allowed, [true, false, false, false, false], "{d:?}");
    assert_eq!(d[1].reason, "not in the allowlist");
    assert!(d[3].reason.contains("local or private"), "{d:?}");
    assert_eq!(d[4].reason, "only CONNECT is served");
    let env = proxy.env();
    assert!(env.contains(&(
        "HTTPS_PROXY".into(),
        format!("http://127.0.0.1:{}", proxy.port())
    )));
    assert!(env.iter().any(|(k, _)| k == "all_proxy"));
}
