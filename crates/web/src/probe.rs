//! `web_endpoint_check` (`SPEC-0325` behaviour 17): the facts of a running endpoint, found without
//! any credential.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls::{ClientConfig, NamedGroup, ProtocolVersion};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::http::SECURITY_HEADERS;
use crate::tls::{client_config, client_config_with};

const LIMIT: Duration = Duration::from_secs(10);

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointReport {
    /// TLS version, `TLSv1_3`.
    pub tls_version: String,
    /// The negotiated group.
    pub key_exchange: String,
    /// A client with only classical groups failed the handshake.
    pub classical_refused: bool,
    /// A plain request got no HTTP answer.
    pub plain_refused: bool,
    /// An upgrade from a foreign origin answered 403.
    pub foreign_origin_refused: bool,
    /// Security headers found on `GET /`.
    pub headers_present: Vec<String>,
    /// Security headers missing on `GET /`.
    pub headers_missing: Vec<String>,
}

impl EndpointReport {
    /// As JSON for the tool's answer.
    pub fn to_json(&self) -> Value {
        json!({
            "tls_version": self.tls_version,
            "key_exchange": self.key_exchange,
            "classical_refused": self.classical_refused,
            "plain_refused": self.plain_refused,
            "foreign_origin_refused": self.foreign_origin_refused,
            "headers_present": self.headers_present,
            "headers_missing": self.headers_missing,
        })
    }
}

fn parse(url: &str) -> Result<(String, u16), String> {
    let rest = url
        .strip_prefix("wss://")
        .ok_or("the address must start with wss://")?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.contains(['/', '?', '#', '@']) {
        return Err("the address is wss://host[:port]".into());
    }
    if let Some(addr) = rest.strip_prefix('[') {
        let (host, tail) = addr.split_once(']').ok_or("malformed address")?;
        let port = tail.strip_prefix(':').map_or(Ok(443), |p| {
            p.parse::<u16>().map_err(|_| "malformed port".to_owned())
        })?;
        return Ok((host.to_owned(), port));
    }
    match rest.split_once(':') {
        None => Ok((rest.to_owned(), 443)),
        Some((host, port)) => Ok((
            host.to_owned(),
            port.parse::<u16>().map_err(|_| "malformed port")?,
        )),
    }
}

async fn handshake(
    host: &str,
    port: u16,
    config: Arc<ClientConfig>,
) -> Result<TlsStream<TcpStream>, String> {
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| format!("{host} is not a host name"))?;
    let tcp = tokio::time::timeout(LIMIT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| "connect timed out".to_owned())?
        .map_err(|e| format!("cannot connect: {e}"))?;
    tokio::time::timeout(LIMIT, TlsConnector::from(config).connect(name, tcp))
        .await
        .map_err(|_| "TLS handshake timed out".to_owned())?
        .map_err(|e| format!("TLS handshake failed: {e}"))
}

async fn read_answer(stream: &mut TlsStream<TcpStream>) -> Result<String, String> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = tokio::time::timeout(LIMIT, stream.read(&mut chunk)).await;
        match read {
            Ok(Ok(0)) | Ok(Err(_)) => break,
            Ok(Ok(n)) => {
                out.extend_from_slice(&chunk[..n]);
                if out.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return Err("the server did not answer".into()),
        }
        if out.len() > 64 << 10 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Checks the endpoint at `url` (`wss://host[:port]`), trusting only `ca_file`.
pub async fn check_endpoint(url: &str, ca_file: &Path) -> Result<EndpointReport, String> {
    let (host, port) = parse(url)?;
    let authority = format!("{host}:{port}");
    let origin_host = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        authority.clone()
    };

    // The real handshake and `GET /`.
    let config = client_config(ca_file).map_err(|e| e.to_string())?;
    let mut tls = handshake(&host, port, Arc::clone(&config)).await?;
    let (tls_version, key_exchange) = {
        let (_, conn) = tls.get_ref();
        let version = conn
            .protocol_version()
            .map_or("unknown".into(), |v| match v {
                ProtocolVersion::TLSv1_3 => "TLSv1_3".to_owned(),
                other => format!("{other:?}"),
            });
        let group = conn
            .negotiated_key_exchange_group()
            .map_or("unknown".to_owned(), |g| match g.name() {
                NamedGroup::X25519MLKEM768 => "X25519MLKEM768".to_owned(),
                other => format!("{other:?}"),
            });
        (version, group)
    };
    let request = format!("GET / HTTP/1.1\r\nHost: {origin_host}\r\nConnection: close\r\n\r\n");
    tls.write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let answer = read_answer(&mut tls).await?.to_ascii_lowercase();
    let (mut headers_present, mut headers_missing) = (Vec::new(), Vec::new());
    for (name, _) in SECURITY_HEADERS {
        if answer.contains(&format!("{}:", name.to_ascii_lowercase())) {
            headers_present.push(name.to_owned());
        } else {
            headers_missing.push(name.to_owned());
        }
    }

    // A client that offers only classical groups must not get through.
    let mut classical = crate::tls::provider();
    classical.kx_groups = vec![rustls::crypto::ring::kx_group::X25519];
    let classical = client_config_with(ca_file, classical).map_err(|e| e.to_string())?;
    let classical_refused = handshake(&host, port, classical).await.is_err();

    // A plain request must get no HTTP answer.
    let plain_refused = match TcpStream::connect((host.as_str(), port)).await {
        Ok(mut tcp) => {
            let _ = tcp
                .write_all(format!("GET / HTTP/1.1\r\nHost: {origin_host}\r\n\r\n").as_bytes())
                .await;
            let mut buf = [0u8; 16];
            let read = tokio::time::timeout(LIMIT, tcp.read(&mut buf)).await;
            !matches!(read, Ok(Ok(n)) if buf[..n].starts_with(b"HTTP/"))
        }
        Err(_) => true,
    };

    // An upgrade from another origin must be refused.
    let mut tls = handshake(&host, port, config).await?;
    let upgrade = format!(
        "GET / HTTP/1.1\r\nHost: {origin_host}\r\nOrigin: https://foreign.invalid\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    tls.write_all(upgrade.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let answer = read_answer(&mut tls).await.unwrap_or_default();
    let foreign_origin_refused = answer.starts_with("HTTP/1.1 403");

    Ok(EndpointReport {
        tls_version,
        key_exchange,
        classical_refused,
        plain_refused,
        foreign_origin_refused,
        headers_present,
        headers_missing,
    })
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn urls_must_be_wss() {
        assert_eq!(parse("wss://a.test:9").unwrap(), ("a.test".into(), 9));
        assert_eq!(parse("wss://a.test/").unwrap(), ("a.test".into(), 443));
        assert_eq!(parse("wss://[::1]:5").unwrap(), ("::1".into(), 5));
        assert!(parse("ws://a.test").is_err());
        assert!(parse("https://a.test").is_err());
        assert!(parse("wss://a.test/x").is_err());
    }
}
