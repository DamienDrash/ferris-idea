//! SPEC-0108 v4 AK-11 and the security tests of ADR-0025 for database SSH tunnels: a real `sshd`
//! of the test (`ferris_idea_remote::testing::TestHost`, loopback only, its own keys) forwards to a
//! local echo server and, when `FERRIS_IDEA_TEST_PG` / `FERRIS_IDEA_TEST_MYSQL` are set, to the reference
//! databases. Skipped where `sshd` or `ssh-keygen` are missing.

use crate::common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

use ferris_idea_database::{
    ConnectionProfile, OnError, Outcome, Session, SessionError, SshTunnel, Tunnel, TunnelError,
};
use ferris_idea_remote::testing::TestHost;

fn host(tag: &str) -> Option<TestHost> {
    let dir =
        std::env::temp_dir().join(format!("ferris-idea-dbtunnel-{tag}-{}", std::process::id()));
    TestHost::start(&dir)
}

fn spec(alias: &str) -> SshTunnel {
    SshTunnel {
        host: alias.into(),
        port: None,
        user: None,
    }
}

/// A server on 127.0.0.1 that answers every line with the same line.
fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 64];
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 || s.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

/// The address of this machine on its network, if it has one besides loopback (no packet is
/// sent: connecting a UDP socket only picks the route).
fn lan_address() -> Option<std::net::IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

#[test]
fn tunnel_forwards_on_loopback_only_and_refuses_unknown_and_changed_keys() {
    let Some(h) = host("echo") else {
        eprintln!("sshd or ssh-keygen missing: skipped");
        return;
    };
    let echo = echo_server();
    let limit = Duration::from_secs(20);
    let tunnel = Tunnel::open_with(
        &spec(&h.target.host),
        "127.0.0.1",
        echo,
        limit,
        &h.target.extra,
        "ssh",
    )
    .expect("tunnel through the test server");
    let local = tunnel.local();
    assert!(local.ip().is_loopback(), "{local}");
    let mut s = TcpStream::connect_timeout(&local, Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"ping").unwrap();
    let mut back = [0u8; 4];
    s.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"ping");

    // ADR-0025: the forwarded port is not reachable from the network.
    if let Some(ip) = lan_address() {
        let outside = SocketAddr::new(ip, local.port());
        assert!(
            TcpStream::connect_timeout(&outside, Duration::from_millis(500)).is_err(),
            "the tunnel port answered on {outside}"
        );
    }
    drop(s);
    drop(tunnel);
    // The port is closed once the tunnel is dropped.
    assert!(TcpStream::connect_timeout(&local, Duration::from_millis(500)).is_err());

    // An unknown host key is refused, nothing is forwarded, nothing is accepted on its own.
    let unknown = h.unknown_key_target();
    let err = Tunnel::open_with(
        &spec(&unknown.host),
        "127.0.0.1",
        echo,
        limit,
        &unknown.extra,
        "ssh",
    )
    .unwrap_err();
    assert!(matches!(err, TunnelError::HostKeyUnknown(_)), "{err}");
    assert!(err.to_string().contains("check the fingerprint"));
    let changed = h.changed_key_target();
    let err = Tunnel::open_with(
        &spec(&changed.host),
        "127.0.0.1",
        echo,
        limit,
        &changed.extra,
        "ssh",
    )
    .unwrap_err();
    assert!(matches!(err, TunnelError::HostKeyChanged(_)), "{err}");
}

/// The reference servers through the tunnel: the session connects to 127.0.0.1:<tunnel>, runs a
/// query and reports the tunnel; an unknown host key stops the connection with the tunnel error.
#[tokio::test]
async fn sessions_go_through_the_tunnel() {
    let Some(h) = host("db") else {
        return;
    };
    for var in ["FERRIS_IDEA_TEST_PG", "FERRIS_IDEA_TEST_MYSQL"] {
        let Ok(url) = std::env::var(var) else {
            continue;
        };
        let mut p = ConnectionProfile::from_url("tunnelled", &url).unwrap();
        p.ssh = Some(spec(&h.target.host));
        let credentials = common::credentials(&p);
        let session = Session::connect_with(&p, &credentials, &h.target.extra)
            .await
            .unwrap_or_else(|e| panic!("{var}: {e}"));
        let via = session.tunnel_address().expect("a tunnel");
        assert!(via.ip().is_loopback());
        assert_ne!(
            Some(via.port()),
            p.port,
            "{var}: the tunnel has its own port"
        );
        let run = session.run_script("select 1", OnError::Stop).await;
        assert!(
            matches!(
                run.outcomes.first().map(|o| &o.outcome),
                Some(Outcome::ResultSet { .. })
            ),
            "{var}"
        );
        drop(session);
        let unknown = h.unknown_key_target();
        p.ssh = Some(spec(&unknown.host));
        let err = Session::connect_with(&p, &credentials, &unknown.extra)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::Tunnel { .. }), "{err}");
        assert!(err.to_string().contains("host key is not known"), "{err}");
    }
}
