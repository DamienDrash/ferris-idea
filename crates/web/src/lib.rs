//! The secure WebSocket endpoint of `idecraft serve` (`SPEC-0325`, first slice of M18): TLS 1.3
//! with the post-quantum hybrid `X25519MLKEM768` only, origin and host checks, authentication
//! before anything reaches the backend, and the session protocol unchanged on top. There is no
//! plaintext path: the listener can only be built from a TLS configuration.

pub mod client;
pub mod http;
pub mod pq;
pub mod probe;
pub mod server;
pub mod tls;

pub use probe::{EndpointReport, check_endpoint};
pub use server::{Limits, Running, WebConfig, WebError, listen};
pub use tls::{TlsError, load_server_config, normalize_origin};

/// The port `--wss` binds without `--listen`.
pub const DEFAULT_PORT: u16 = 47110;
