//! A client of the endpoint for tools and tests (`SPEC-0325`): connects with TLS 1.3 and the
//! hybrid group only, authenticates, and hands back a session [`Transport`].

use std::net::SocketAddr;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use idecraft_session::Transport;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::Request;

use crate::server::bridge;
use crate::tls::client_config;

fn runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    static RT: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("web-client")
            .enable_all()
            .build()
            .ok()
    })
    .as_ref()
    .ok_or_else(|| "no runtime for the web client".to_owned())
}

/// Connects and authenticates; `host` is the name the certificate is checked against and the one
/// used for `Host` and `Origin`.
pub fn connect(
    addr: SocketAddr,
    host: &str,
    ca_file: &Path,
    origin: &str,
    credential: &str,
) -> Result<(Transport, Value), String> {
    let rt = runtime()?;
    let config = client_config(ca_file).map_err(|e| e.to_string())?;
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| format!("{host} is not a host name"))?;
    let authority = format!("{host}:{}", addr.port());
    let origin = origin.to_owned();
    let credential = credential.to_owned();
    rt.block_on(async move {
        let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
        let tls = TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS: {e}"))?;
        let request = Request::builder()
            .uri(format!("wss://{authority}/"))
            .header("Host", authority.as_str())
            .header("Origin", origin)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| e.to_string())?;
        let (mut ws, _) = client_async(request, tls)
            .await
            .map_err(|e| format!("upgrade: {e}"))?;
        ws.send(Message::text(
            json!({ "credential": credential }).to_string(),
        ))
        .await
        .map_err(|e| e.to_string())?;
        let answer = match tokio::time::timeout(Duration::from_secs(15), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => t,
            Ok(Some(Ok(Message::Close(f)))) => {
                return Err(format!(
                    "closed: {}",
                    f.map_or_else(String::new, |f| format!(
                        "{} {}",
                        u16::from(f.code),
                        f.reason
                    ))
                ));
            }
            other => return Err(format!("no answer: {other:?}")),
        };
        let value: Value = serde_json::from_str(answer.as_str()).map_err(|e| e.to_string())?;
        let transport = bridge(
            ws,
            Duration::from_secs(30),
            Duration::from_secs(90),
            tokio::runtime::Handle::current(),
        );
        Ok((transport, value))
    })
}
