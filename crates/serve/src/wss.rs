//! `ferris-idea serve --wss` (`SPEC-0325`): the secure WebSocket endpoint for browsers.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use ferris_idea_session::{Backend, Right};
use ferris_idea_web::{Limits, WebConfig, listen, load_server_config, normalize_origin};

/// What `--wss` was given.
pub struct WssOptions {
    /// `--listen`.
    pub addr: Option<SocketAddr>,
    /// `--allow-network`.
    pub allow_network: bool,
    /// `--wss-cert`.
    pub cert: Option<PathBuf>,
    /// `--wss-key`.
    pub key: Option<PathBuf>,
    /// `--wss-origin`.
    pub origins: Vec<String>,
    /// `--wss-rights`.
    pub rights: Option<Vec<Right>>,
    /// A `<folder>` was given.
    pub has_folder: bool,
}

/// The address `--wss` binds without `--listen`.
pub fn default_address() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], ferris_idea_web::DEFAULT_PORT))
}

fn refuse(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(2)
}

/// Runs the endpoint until the process ends.
pub fn run(backend: Backend, o: WssOptions) -> ExitCode {
    if !o.has_folder {
        return refuse(
            "--wss needs a folder: a connection is bound to one project \
             (ferris-idea serve --wss <folder> ...)",
        );
    }
    let (Some(cert), Some(key)) = (o.cert, o.key) else {
        return refuse("--wss needs --wss-cert and --wss-key (PEM files)");
    };
    let tls = match load_server_config(&cert, &key) {
        Ok(t) => t,
        Err(e) => return refuse(&e.to_string()),
    };
    let mut origins = Vec::with_capacity(o.origins.len());
    for origin in &o.origins {
        match normalize_origin(origin) {
            Ok(a) => origins.push(a),
            Err(e) => return refuse(&e),
        }
    }
    let config = WebConfig {
        addr: o.addr.unwrap_or_else(default_address),
        allow_network: o.allow_network,
        origins,
        tls,
        owner_rights: o.rights.unwrap_or_else(|| vec![Right::View]),
        limits: Limits::default(),
    };
    let running = match listen(config, backend) {
        Ok(r) => r,
        Err(e) => return refuse(&e.to_string()),
    };
    let addr = running.addr();
    if addr.ip().is_loopback() {
        eprintln!("ferris-idea serve --wss: listening on wss://{addr}");
    } else {
        eprintln!("ferris-idea serve --wss: listening on wss://{addr}, reachable from the network");
    }
    // Shown once, like the pairing code (behaviour 12); it ends with this process.
    if let Some(secret) = running.take_owner_secret() {
        eprintln!("Web access secret (shown once): {secret}");
    }
    while !running.stopped() {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_address_is_loopback() {
        assert!(super::default_address().ip().is_loopback());
    }
}
