//! `SPEC-0325`: the endpoint on real sockets with real TLS. Test certificates are generated here,
//! never committed.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferris_idea_session::room::RoomConfig;
use ferris_idea_session::{Backend, BackendConfig, ClientSession, ErrorCode, Right, Transport};
use ferris_idea_web::client::connect;
use ferris_idea_web::http::SECURITY_HEADERS;
use ferris_idea_web::tls::{client_config, client_config_with, load_server_config, provider};
use ferris_idea_web::{Limits, Running, WebConfig, WebError, check_endpoint, listen};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use rustls::ClientConfig;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

struct Fixture {
    _dir: tempfile::TempDir,
    project: PathBuf,
    state: PathBuf,
    ca: PathBuf,
    cert: PathBuf,
    key: PathBuf,
}

/// A throw-away certificate authority and a leaf for `localhost`.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(project.join("a.txt"), "hello\n").unwrap();
    let project = project.canonicalize().unwrap();

    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "test ca");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&ca_params, &ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf_params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
    let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

    let ca = dir.path().join("ca.pem");
    let cert = dir.path().join("cert.pem");
    let key = dir.path().join("key.pem");
    std::fs::write(&ca, ca_cert.pem()).unwrap();
    std::fs::write(&cert, leaf.pem()).unwrap();
    std::fs::write(&key, leaf_key.serialize_pem()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    Fixture {
        _dir: dir,
        project,
        state,
        ca,
        cert,
        key,
    }
}

fn backend_for(f: &Fixture) -> Backend {
    let backend = Backend::new(BackendConfig {
        build: "test".into(),
        folder: Some(f.project.clone()),
        state_dir: Some(f.state.clone()),
        peer: "local:test".into(),
        ..BackendConfig::default()
    });
    ferris_idea_session::room::bind(&backend, RoomConfig::default());
    backend
}

fn limits() -> Limits {
    Limits {
        credential: Duration::from_millis(400),
        head: Duration::from_secs(2),
        ..Limits::default()
    }
}

fn start_with(f: &Fixture, rights: Vec<Right>, limits: Limits) -> (Running, Backend) {
    let backend = backend_for(f);
    let running = listen(
        WebConfig {
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            allow_network: false,
            origins: Vec::new(),
            tls: load_server_config(&f.cert, &f.key).unwrap(),
            owner_rights: rights,
            limits,
        },
        backend.clone(),
    )
    .unwrap();
    (running, backend)
}

fn origin_of(running: &Running) -> String {
    format!("https://localhost:{}", running.addr().port())
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn raw(addr: SocketAddr, config: Arc<ClientConfig>, request: &str) -> Result<String, String> {
    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let mut tls = TlsConnector::from(config)
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            tcp,
        )
        .await
        .map_err(|e| e.to_string())?;
    tls.write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut out)).await;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn upgrade(port: u16, host: &str, origin: Option<&str>) -> String {
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    format!(
        "GET / HTTP/1.1\r\nHost: {host}:{port}\r\n{origin}Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    )
}

fn open(transport: Transport, f: &Fixture) -> Result<ClientSession, ferris_idea_session::RpcError> {
    ClientSession::open(transport, &f.project.display().to_string(), "t", "h", None)
}

#[test]
fn ak1_only_the_hybrid_over_tls13_gets_through() {
    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let addr = running.addr();
    let rt = rt();
    let get = format!("GET / HTTP/1.1\r\nHost: localhost:{}\r\n\r\n", addr.port());

    // The hybrid works and is what was negotiated.
    let answer = rt
        .block_on(raw(addr, client_config(&f.ca).unwrap(), &get))
        .unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");

    // Only classical groups: no handshake.
    let mut classical = provider();
    classical.kx_groups = vec![rustls::crypto::ring::kx_group::X25519];
    let config = client_config_with(&f.ca, classical).unwrap();
    assert!(rt.block_on(raw(addr, config, &get)).is_err());

    // TLS 1.2 only: no handshake.
    let mut roots = rustls::RootCertStore::empty();
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    roots.add_parsable_certificates(
        CertificateDer::pem_file_iter(&f.ca)
            .unwrap()
            .map(Result::unwrap),
    );
    let tls12 =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
    assert!(rt.block_on(raw(addr, Arc::new(tls12), &get)).is_err());
}

#[test]
fn ak3_a_plain_request_gets_no_http_answer() {
    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let addr = running.addr();
    rt().block_on(async {
        let mut tcp = TcpStream::connect(addr).await.unwrap();
        tcp.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0u8; 32];
        let n = tokio::time::timeout(Duration::from_secs(3), tcp.read(&mut buf))
            .await
            .map_or(0, |r| r.unwrap_or(0));
        assert!(
            !buf[..n].starts_with(b"HTTP/"),
            "plain HTTP answered: {:?}",
            &buf[..n]
        );
    });
}

#[test]
fn ak3_no_setting_drops_tls() {
    let server = include_str!("../src/server.rs");
    assert!(!server.contains("ws://"));
    for needle in ["insecure", "plaintext", "allow_plain", "no_tls"] {
        assert!(!server.contains(needle), "{needle}");
    }
    // The request head is only read after the TLS accept.
    let accept = server.find("inner.acceptor.accept").unwrap();
    let head = server.find("http::read_head").unwrap();
    assert!(accept < head);
}

#[test]
fn ak4_a_network_listener_needs_the_switch_and_an_origin() {
    let f = fixture();
    let tls = load_server_config(&f.cert, &f.key).unwrap();
    let base = |allow: bool, origins: Vec<String>| WebConfig {
        addr: SocketAddr::from(([192, 0, 2, 1], 0)),
        allow_network: allow,
        origins,
        tls: Arc::clone(&tls),
        owner_rights: vec![Right::View],
        limits: Limits::default(),
    };
    assert!(matches!(
        listen(base(false, vec![]), backend_for(&f)),
        Err(WebError::NetworkNotAllowed(_))
    ));
    assert!(matches!(
        listen(base(true, vec![]), backend_for(&f)),
        Err(WebError::NoOrigin)
    ));
    let no_folder = Backend::new(BackendConfig {
        build: "t".into(),
        ..BackendConfig::default()
    });
    let mut cfg = base(false, vec![]);
    cfg.addr = SocketAddr::from(([127, 0, 0, 1], 0));
    assert!(matches!(listen(cfg, no_folder), Err(WebError::NoFolder)));
}

#[test]
fn ak5_origin_and_host_are_checked_before_any_credential() {
    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let addr = running.addr();
    let port = addr.port();
    let rt = rt();
    let config = client_config(&f.ca).unwrap();
    for origin in [
        Some("https://evil.example:9".to_owned()),
        Some("null".to_owned()),
        Some(format!("http://localhost:{port}")),
        None,
    ] {
        let answer = rt
            .block_on(raw(
                addr,
                Arc::clone(&config),
                &upgrade(port, "localhost", origin.as_deref()),
            ))
            .unwrap();
        assert!(answer.starts_with("HTTP/1.1 403"), "{origin:?}: {answer}");
    }
    let answer = rt
        .block_on(raw(
            addr,
            Arc::clone(&config),
            &upgrade(
                port,
                "evil.example",
                Some(&format!("https://localhost:{port}")),
            ),
        ))
        .unwrap();
    assert!(answer.starts_with("HTTP/1.1 421"), "{answer}");
    // A POST or an upgrade with a wrong version never gets further either.
    let post = format!("POST / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n");
    let answer = rt.block_on(raw(addr, Arc::clone(&config), &post)).unwrap();
    assert!(answer.starts_with("HTTP/1.1 405"), "{answer}");
}

#[test]
fn ak6_every_answer_has_the_security_headers() {
    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let addr = running.addr();
    let port = addr.port();
    let rt = rt();
    let config = client_config(&f.ca).unwrap();
    for (path, status) in [("/", "200"), ("/check.js", "200"), ("/nope", "404")] {
        let answer = rt
            .block_on(raw(
                addr,
                Arc::clone(&config),
                &format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"),
            ))
            .unwrap();
        assert!(
            answer.starts_with(&format!("HTTP/1.1 {status}")),
            "{answer}"
        );
        for (name, value) in SECURITY_HEADERS {
            assert!(answer.contains(&format!("{name}: {value}")), "{name}");
        }
    }
}

#[test]
fn ak7_wrong_credentials_close_alike_and_reach_nothing() {
    let f = fixture();
    let (running, backend) = start_with(&f, vec![Right::View], limits());
    let origin = origin_of(&running);
    let wrong = connect(running.addr(), "localhost", &f.ca, &origin, "wrong")
        .map(|_| ())
        .unwrap_err();
    assert!(
        wrong.contains("1008") && wrong.contains("not authenticated"),
        "{wrong}"
    );
    // No credential within the limit.
    let silent = rt().block_on(async {
        use tokio_tungstenite::client_async;
        use tokio_tungstenite::tungstenite::http::Request;
        let tcp = TcpStream::connect(running.addr()).await.unwrap();
        let tls = TlsConnector::from(client_config(&f.ca).unwrap())
            .connect(
                rustls::pki_types::ServerName::try_from("localhost").unwrap(),
                tcp,
            )
            .await
            .unwrap();
        let request = Request::builder()
            .uri(format!("wss://localhost:{}/", running.addr().port()))
            .header("Host", format!("localhost:{}", running.addr().port()))
            .header("Origin", origin.as_str())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .unwrap();
        let (mut ws, _) = client_async(request, tls).await.unwrap();
        use futures_util::StreamExt;
        ws.next().await
    });
    match silent {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(Some(f)))) => {
            assert_eq!(u16::from(f.code), 1008);
            assert_eq!(f.reason.as_str(), "not authenticated");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(backend.sessions(), 0);
    // The audit log names outcomes, never the secret.
    let log = std::fs::read_to_string(backend.audit_file().unwrap()).unwrap();
    assert!(log.contains("credential_refused") && log.contains("no_credential"));
    assert!(!log.contains("wrong"));
}

#[test]
fn ak8_the_owner_is_capped_to_the_granted_rights() {
    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let origin = origin_of(&running);
    let secret = running.take_owner_secret().unwrap();
    assert!(running.take_owner_secret().is_none(), "shown once");
    let (transport, answer) =
        connect(running.addr(), "localhost", &f.ca, &origin, &secret).unwrap();
    assert_eq!(answer["as"], "owner");
    let mut session = open(transport, &f).unwrap();
    let doc = session.open_doc("a.txt").unwrap();
    assert_eq!(doc.text(), "hello\n");
    let denied = session
        .call("fs.write", json!({"path": "a.txt", "text": "x"}))
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::NotGranted, "{denied}");
}

#[test]
fn ak8_a_session_cannot_name_another_project() {
    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let origin = origin_of(&running);
    let secret = running.take_owner_secret().unwrap();
    let (transport, _) = connect(running.addr(), "localhost", &f.ca, &origin, &secret).unwrap();
    let other = tempfile::tempdir().unwrap();
    let refused = ClientSession::open(
        transport,
        &other.path().canonicalize().unwrap().display().to_string(),
        "t",
        "h",
        None,
    );
    assert!(refused.is_err());
}

fn anonymous_link(f: &Fixture, running: &Running, secret: &str) -> (ClientSession, String, String) {
    let origin = origin_of(running);
    let (transport, _) = connect(running.addr(), "localhost", &f.ca, &origin, secret).unwrap();
    let owner = open(transport, f).unwrap();
    owner.call("room.create", json!({"name": "web"})).unwrap();
    let share = owner
        .call("room.share", json!({"audience": "anonymous"}))
        .unwrap();
    let link = share["link"].as_str().unwrap().to_owned();
    let id = share["id"].as_str().unwrap().to_owned();
    (owner, link, id)
}

#[test]
fn ak7_an_organisation_link_is_refused_at_the_door() {
    let f = fixture();
    let (running, backend) = start_with(&f, Right::ALL.to_vec(), limits());
    let origin = origin_of(&running);
    let secret = running.take_owner_secret().unwrap();
    let (owner, _, _) = anonymous_link(&f, &running, &secret);
    let share = owner.call("room.share", json!({})).unwrap();
    let link = share["link"].as_str().unwrap().to_owned();
    let sessions = backend.sessions();
    let refused = connect(running.addr(), "localhost", &f.ca, &origin, &link)
        .map(|_| ())
        .unwrap_err();
    assert!(refused.contains("1008"), "{refused}");
    assert_eq!(backend.sessions(), sessions);
}

#[test]
fn ak9_a_guest_joins_by_link_is_a_viewer_and_leaves_with_it() {
    let f = fixture();
    let (running, _b) = start_with(&f, Right::ALL.to_vec(), limits());
    let origin = origin_of(&running);
    let secret = running.take_owner_secret().unwrap();
    let (owner, link, id) = anonymous_link(&f, &running, &secret);

    let (transport, answer) = connect(running.addr(), "localhost", &f.ca, &origin, &link).unwrap();
    assert_eq!(answer["as"], "guest");
    let guest = open(transport, &f).unwrap();
    // Without a place in the room nothing but joining works.
    let e = guest.call("fs.read", json!({"path": "a.txt"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted, "{e}");
    let joined = guest.call("room.join", json!({"link": link})).unwrap();
    assert_eq!(joined["role"], "viewer");
    let e = guest
        .call("fs.write", json!({"path": "a.txt", "text": "x"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotGranted, "{e}");

    owner.call("room.revoke_link", json!({"id": id})).unwrap();
    // A new connection with the link is refused now.
    let refused = connect(running.addr(), "localhost", &f.ca, &origin, &link)
        .map(|_| ())
        .unwrap_err();
    assert!(refused.contains("1008"), "{refused}");
    // The guest's session ends within a second or two (the room's tick).
    let t0 = Instant::now();
    while guest.call("room.status", json!({})).is_ok() {
        assert!(t0.elapsed() < Duration::from_secs(5), "still in");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn ak10_too_many_failures_block_the_address() {
    let f = fixture();
    let limits = Limits {
        failures: 3,
        window: Duration::from_secs(30),
        block: Duration::from_secs(30),
        ..limits()
    };
    let (running, _b) = start_with(&f, vec![Right::View], limits);
    let origin = origin_of(&running);
    let secret = running.take_owner_secret().unwrap();
    for _ in 0..3 {
        connect(running.addr(), "localhost", &f.ca, &origin, "wrong")
            .map(|_| ())
            .unwrap_err();
    }
    std::thread::sleep(Duration::from_millis(50));
    let e = connect(running.addr(), "localhost", &f.ca, &origin, &secret)
        .map(|_| ())
        .unwrap_err();
    assert!(e.contains("TLS"), "{e}");
}

#[test]
fn ak11_text_after_authentication_closes_and_message_boundaries_mean_nothing() {
    use ferris_idea_session::frame::Frame;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, http::Request};

    let f = fixture();
    let (running, _b) = start_with(&f, vec![Right::View], limits());
    let origin = origin_of(&running);
    let secret = running.take_owner_secret().unwrap();
    let addr = running.addr();
    let rt = rt();

    async fn ws(
        addr: SocketAddr,
        origin: &str,
        ca: &Path,
        secret: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>> {
        let tcp = TcpStream::connect(addr).await.unwrap();
        let tls = TlsConnector::from(client_config(ca).unwrap())
            .connect(
                rustls::pki_types::ServerName::try_from("localhost").unwrap(),
                tcp,
            )
            .await
            .unwrap();
        let request = Request::builder()
            .uri(format!("wss://localhost:{}/", addr.port()))
            .header("Host", format!("localhost:{}", addr.port()))
            .header("Origin", origin)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .unwrap();
        let (mut ws, _) = tokio_tungstenite::client_async(request, tls).await.unwrap();
        ws.send(Message::text(json!({"credential": secret}).to_string()))
            .await
            .unwrap();
        assert!(matches!(ws.next().await, Some(Ok(Message::Text(_)))));
        ws
    }

    rt.block_on(async {
        // Text after authentication closes.
        let mut c = ws(addr, &origin, &f.ca, &secret).await;
        c.send(Message::text("oops")).await.unwrap();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), c.next()).await {
                Ok(Some(Ok(Message::Close(_))) | None | Some(Err(_))) => break,
                Ok(Some(Ok(_))) => {}
                Err(_) => panic!("not closed"),
            }
        }

        // A hello split over two messages is answered.
        let mut c = ws(addr, &origin, &f.ca, &secret).await;
        let hello = json!({
            "jsonrpc": "2.0", "id": 1, "method": "session.hello",
            "params": {
                "protocol": "idecraft-session", "majors": [1], "build": "t", "client": "t",
                "project": f.project.display().to_string(), "capabilities": [],
            }
        })
        .to_string();
        let mut bytes = Vec::new();
        Frame::message(hello).write_to(&mut bytes).unwrap();
        let (a, b) = bytes.split_at(7);
        c.send(Message::binary(a.to_vec())).await.unwrap();
        c.send(Message::binary(b.to_vec())).await.unwrap();
        let answer = tokio::time::timeout(Duration::from_secs(5), c.next())
            .await
            .unwrap();
        assert!(matches!(answer, Some(Ok(Message::Binary(_)))), "{answer:?}");
    });
}

#[test]
fn ak12_the_check_tool_reports_the_real_endpoint() {
    let f = fixture();
    let (running, backend) = start_with(&f, vec![Right::View], limits());
    let url = format!("wss://localhost:{}", running.addr().port());
    let report = rt().block_on(check_endpoint(&url, &f.ca)).unwrap();
    assert_eq!(report.tls_version, "TLSv1_3");
    assert_eq!(report.key_exchange, "X25519MLKEM768");
    assert!(report.classical_refused);
    assert!(report.plain_refused);
    assert!(report.foreign_origin_refused);
    assert!(report.headers_missing.is_empty(), "{report:?}");
    assert_eq!(backend.sessions(), 0);
}
