//! SPEC-0116 v9 AK-13: the SSL modes of a SQL Server connection against a TDS 7 handshake stood up in
//! the test process, on every platform the tests run on, macOS included. The stand-in server answers
//! the pre-login with encryption on, runs a TLS 1.2 handshake inside pre-login packets as SQL Server
//! does (one message per flight), then reads the login from inside TLS and ends it with `DONE`.
//! It needs no SQL Server, no network beyond the loopback and no file outside a temporary folder:
//! the CA and the server certificate for `localhost` are made here.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use ferris_idea_database::{
    ConnectionProfile, Credentials, Secret, SessionError, SqlServerSession, SslMode,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// TDS packet types (MS-TDS 2.2.3.1.1).
const PRELOGIN: u8 = 0x12;
const REPLY: u8 = 0x04;
const LOGIN7: u8 = 0x10;
const HEADER: usize = 8;
const PACKET: usize = 4096;

/// A certificate authority of the test with a server certificate for `localhost`.
struct Authority {
    pem: String,
    server: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
}

impl Authority {
    fn new(name: &str) -> Self {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca.distinguished_name.push(rcgen::DnType::CommonName, name);
        ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];
        let pem = ca.self_signed(&ca_key).unwrap().pem();
        // As macOS accepts a TLS server's certificate: the name in subjectAltName, serverAuth,
        // a validity well below 825 days.
        let mut server = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        server
            .distinguished_name
            .push(rcgen::DnType::CommonName, "localhost");
        server.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        server.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        let year = this_year();
        server.not_before = rcgen::date_time_ymd(year - 1, 12, 1);
        server.not_after = rcgen::date_time_ymd(year + 1, 6, 1);
        let issuer = rcgen::Issuer::from_params(&ca, &ca_key);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = server.signed_by(&key, &issuer).unwrap();
        Self {
            pem,
            server: cert.der().clone(),
            key: PrivatePkcs8KeyDer::from(key.serialize_der()),
        }
    }

    fn write(&self, dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, &self.pem).unwrap();
        path
    }
}

/// The current year, near enough for a validity of about eighteen months around today.
fn this_year() -> i32 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    1970 + i32::try_from(seconds / 31_556_952).unwrap()
}

/// Reads one TDS packet: its type, status and body.
fn packet(r: &mut impl Read) -> io::Result<(u8, u8, Vec<u8>)> {
    let mut header = [0u8; HEADER];
    r.read_exact(&mut header)?;
    let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
    if length < HEADER {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; length - HEADER];
    r.read_exact(&mut body)?;
    Ok((header[0], header[1], body))
}

/// Reads one TDS message: the packets up to the one marked as the end.
fn message(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut payload = Vec::new();
    loop {
        let (kind, status, body) = packet(r)?;
        payload.extend_from_slice(&body);
        if status & 0x01 != 0 {
            return Ok((kind, payload));
        }
    }
}

/// Writes `payload` as one TDS message of `kind`.
fn send(w: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let chunks: Vec<&[u8]> = payload.chunks(PACKET - HEADER).collect();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let last = i + 1 == chunks.len();
        let length = u16::try_from(chunk.len() + HEADER).unwrap();
        out.extend_from_slice(&[kind, u8::from(last)]);
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&[0, 0, u8::try_from(i + 1).unwrap(), 0]);
        out.extend_from_slice(chunk);
    }
    w.write_all(&out)?;
    w.flush()
}

/// The server's pre-login answer: version, encryption on, end of the options.
fn prelogin_answer() -> Vec<u8> {
    let mut answer = vec![0x00, 0, 11, 0, 6, 0x01, 0, 17, 0, 1, 0xff];
    answer.extend_from_slice(&[16, 0, 0x10, 0x00, 0, 0]);
    answer.push(0x01);
    answer
}

/// The stand-in server's TLS: TLS 1.2 only, as SQL Server speaks it inside TDS 7 pre-login.
fn tls_config(authority: &Authority) -> Arc<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![authority.server.clone()],
            PrivateKeyDer::Pkcs8(authority.key.clone_key()),
        )
        .unwrap();
    Arc::new(config)
}

/// Serves one connection; returns whether the login arrived through TLS.
fn serve_one(mut tcp: TcpStream, config: Arc<rustls::ServerConfig>) -> io::Result<bool> {
    let (kind, _) = message(&mut tcp)?;
    assert_eq!(kind, PRELOGIN, "the client starts with its pre-login");
    send(&mut tcp, REPLY, &prelogin_answer())?;
    let mut tls = rustls::ServerConnection::new(config).map_err(io::Error::other)?;
    while tls.is_handshaking() {
        if tls.wants_write() {
            let mut flight = Vec::new();
            while tls.wants_write() {
                tls.write_tls(&mut flight)?;
            }
            send(&mut tcp, PRELOGIN, &flight)?;
            continue;
        }
        // One pre-login message holds the client's whole flight. Like SQL Server, the stand-in
        // closes the connection when a message ends before the flight does (SPEC-0116 v6
        // behaviour 14: the macOS TLS library hands over each record on its own).
        let (kind, records) = message(&mut tcp)?;
        if kind != PRELOGIN {
            return Ok(false);
        }
        let mut rest = records.as_slice();
        while !rest.is_empty() {
            tls.read_tls(&mut rest)?;
            tls.process_new_packets().map_err(io::Error::other)?;
        }
        if tls.is_handshaking() && !tls.wants_write() {
            return Ok(false);
        }
    }
    let mut flight = Vec::new();
    while tls.wants_write() {
        tls.write_tls(&mut flight)?;
    }
    if !flight.is_empty() {
        send(&mut tcp, PRELOGIN, &flight)?;
    }
    // After the handshake the TDS packets travel inside TLS.
    let mut stream = rustls::StreamOwned::new(tls, tcp);
    let (kind, login) = message(&mut stream)?;
    if kind != LOGIN7 || login.len() < 36 {
        return Ok(false);
    }
    // DONE: status, current command, row count.
    let mut done = vec![0xfd, 0, 0, 0, 0];
    done.extend_from_slice(&0u64.to_le_bytes());
    send(&mut stream, REPLY, &done)?;
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
    Ok(true)
}

/// A stand-in SQL Server on the loopback for one connection.
fn server(authority: &Authority) -> (u16, JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = tls_config(authority);
    let handle = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(20)))
            .unwrap();
        serve_one(tcp, config).unwrap_or(false)
    });
    (port, handle)
}

fn profile(host: &str, port: u16, mode: SslMode, ca: Option<&Path>) -> ConnectionProfile {
    let url = format!("sqlserver://tester@{host}:{port}/inventory");
    let mut p = ConnectionProfile::from_url("stand-in", &url).unwrap();
    p.ssl = mode;
    p.tls.ca_file = ca.map(Path::to_path_buf);
    p
}

/// Connects with `mode`; returns the session's encryption, or the error, and whether the server
/// received the login inside TLS.
async fn connect(
    authority: &Authority,
    host: &str,
    mode: SslMode,
    ca: Option<&Path>,
) -> (Result<bool, SessionError>, bool) {
    let (port, server) = server(authority);
    let p = profile(host, port, mode, ca);
    // Not a credential: the stand-in server accepts any login.
    let credentials = Credentials::Available(Secret::new(String::from("stand-in")));
    let result = SqlServerSession::connect(&p, &credentials)
        .await
        .map(|s| s.encrypted());
    let login = tokio::task::spawn_blocking(move || server.join().unwrap_or(false))
        .await
        .unwrap();
    (result, login)
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ferris-idea-tds-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn tls_error(result: &Result<bool, SessionError>, mode: &str, text: &str) {
    match result {
        Err(e @ SessionError::Tls { mode: m, .. }) => {
            assert_eq!(*m, mode, "{e}");
            assert!(e.to_string().contains(text), "{e}");
        }
        Err(e) => panic!("expected a TLS error of {mode}, got {e}"),
        Ok(_) => panic!("expected a TLS error of {mode}, got a connection"),
    }
}

/// SPEC-0116 v9 AK-13: `require`, `verify-ca` and `verify-full` complete the TDS 7 TLS handshake on
/// this platform and send the login only inside TLS; with a CA file the file is the only anchor.
#[tokio::test]
async fn sql_server_ssl_modes_against_a_local_handshake() {
    let ours = Authority::new("Ferris IDEA stand-in CA");
    let dir = temp("modes");
    let ca = ours.write(&dir, "ca.pem");

    let (result, login) = connect(&ours, "127.0.0.1", SslMode::Require, None).await;
    assert!(result.unwrap(), "require connects encrypted");
    assert!(login, "require: the login arrived inside TLS");

    for mode in [SslMode::VerifyFull, SslMode::VerifyCa] {
        let (result, login) = connect(&ours, "localhost", mode, Some(&ca)).await;
        assert!(result.unwrap(), "{mode:?} with the CA file");
        assert!(login, "{mode:?}: the login arrived inside TLS");
    }

    // Without the CA file the system's roots do not know the stand-in CA.
    let (result, login) = connect(&ours, "localhost", SslMode::VerifyFull, None).await;
    tls_error(&result, "verify-full", "");
    assert!(!login, "no login after a refused certificate");

    // The certificate names `localhost`, not 127.0.0.1.
    let (result, login) = connect(&ours, "127.0.0.1", SslMode::VerifyFull, Some(&ca)).await;
    tls_error(&result, "verify-full", "");
    assert!(!login, "no login after a refused host name");

    // A CA file that did not sign the server's certificate: the chain check names the file.
    let foreign = Authority::new("Another CA").write(&dir, "foreign.pem");
    let named = format!(
        "not issued by a certificate in the CA file {}",
        foreign.display()
    );
    for (mode, name) in [
        (SslMode::VerifyFull, "verify-full"),
        (SslMode::VerifyCa, "verify-ca"),
    ] {
        let (result, login) = connect(&ours, "localhost", mode, Some(&foreign)).await;
        tls_error(&result, name, &named);
        assert!(!login, "{name}: no login with a foreign CA file");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
