//! TLS configuration of the endpoint (`SPEC-0325` behaviours 4–6, 8, 9): TLS 1.3 only, with
//! `X25519MLKEM768` as the only group; certificate and key from PEM files.

use std::path::Path;
use std::sync::Arc;

use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};

use crate::pq::X25519_MLKEM768;

/// Why a TLS configuration could not be built.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct TlsError(pub String);

/// The ring provider, narrowed to TLS 1.3 suites and the hybrid group.
pub fn provider() -> CryptoProvider {
    let mut provider = rustls::crypto::ring::default_provider();
    provider
        .cipher_suites
        .retain(|s| matches!(s, rustls::SupportedCipherSuite::Tls13(_)));
    provider.kx_groups = vec![X25519_MLKEM768];
    provider
}

/// Reads a PEM certificate chain and key and builds the server's TLS configuration. A key file
/// that others may read refuses the start (behaviour 5).
pub fn load_server_config(cert: &Path, key: &Path) -> Result<Arc<ServerConfig>, TlsError> {
    check_key_permissions(key)?;
    let chain = CertificateDer::pem_file_iter(cert)
        .map_err(|_| TlsError(format!("cannot read the certificate {}", cert.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TlsError(format!("{} is not a PEM certificate chain", cert.display())))?;
    if chain.is_empty() {
        return Err(TlsError(format!("{} holds no certificate", cert.display())));
    }
    let private = PrivateKeyDer::from_pem_file(key)
        .map_err(|_| TlsError(format!("{} is not a PEM private key", key.display())))?;
    server_config(chain, private)
}

/// The server's TLS configuration from a chain and key already in memory.
pub fn server_config(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<ServerConfig>, TlsError> {
    let mut config = ServerConfig::builder_with_provider(Arc::new(provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| TlsError(format!("TLS setup failed: {e}")))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| TlsError(format!("certificate and key are not usable together: {e} (use a PKCS#8 key from OpenSSL 3, Go or mkcert)")))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// A client configuration that trusts only the PEM certificates in `ca_file`.
pub fn client_config(ca_file: &Path) -> Result<Arc<ClientConfig>, TlsError> {
    client_config_with(ca_file, provider())
}

/// The same with another provider (the probe's classical client).
pub fn client_config_with(
    ca_file: &Path,
    provider: CryptoProvider,
) -> Result<Arc<ClientConfig>, TlsError> {
    let mut roots = RootCertStore::empty();
    let certs = CertificateDer::pem_file_iter(ca_file)
        .map_err(|_| TlsError(format!("cannot read {}", ca_file.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TlsError(format!("{} is not PEM", ca_file.display())))?;
    let (added, _) = roots.add_parsable_certificates(certs);
    if added == 0 {
        return Err(TlsError(format!(
            "{} holds no usable certificate",
            ca_file.display()
        )));
    }
    let config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| TlsError(format!("TLS setup failed: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

#[cfg(unix)]
fn check_key_permissions(key: &Path) -> Result<(), TlsError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(key)
        .map_err(|_| TlsError(format!("cannot read the key {}", key.display())))?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(TlsError(format!(
            "the key {} is readable by others; allow only its owner (chmod 600)",
            key.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_key_permissions(_key: &Path) -> Result<(), TlsError> {
    Ok(())
}

/// `https://name[:port]` as a normalized authority, `name:port` in lower case, or an explanation.
/// Rejects everything else: other schemes, paths, wildcards, user information.
pub fn normalize_origin(origin: &str) -> Result<String, String> {
    let refused = || format!("{origin} is not an https origin (https://name[:port])");
    let rest = origin.strip_prefix("https://").ok_or_else(refused)?;
    normalize_authority(rest).ok_or_else(refused)
}

/// `name[:port]` as `name:port` in lower case, 443 by default; `None` if it is not an authority.
pub fn normalize_authority(text: &str) -> Option<String> {
    if text.is_empty() || text.contains(['/', '?', '#', '@', '*', ' ', '\\']) || !text.is_ascii() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix('[') {
        let (addr, tail) = rest.split_once(']')?;
        addr.parse::<std::net::Ipv6Addr>().ok()?;
        let port = match tail {
            "" => "443".to_owned(),
            t => valid_port(t.strip_prefix(':')?)?,
        };
        return Some(format!("[{addr}]:{port}"));
    }
    match lower.split_once(':') {
        None => Some(format!("{lower}:443")),
        Some((host, port)) if !host.is_empty() => Some(format!("{host}:{}", valid_port(port)?)),
        Some(_) => None,
    }
}

fn valid_port(text: &str) -> Option<String> {
    text.parse::<u16>().ok().map(|p| p.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_normalized_and_only_https_passes() {
        assert_eq!(
            normalize_origin("https://IDE.Example.com").unwrap(),
            "ide.example.com:443"
        );
        assert_eq!(
            normalize_origin("https://x.test:8443").unwrap(),
            "x.test:8443"
        );
        assert_eq!(normalize_origin("https://[::1]:7").unwrap(), "[::1]:7");
        for bad in [
            "http://x.test",
            "ws://x.test",
            "https://x.test/path",
            "https://*.x.test",
            "https://a@x.test",
            "https://x.test:999999",
            "null",
            "",
        ] {
            assert!(normalize_origin(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_provider_has_only_the_hybrid_and_tls13() {
        let p = provider();
        assert_eq!(p.kx_groups.len(), 1);
        assert_eq!(p.kx_groups[0].name(), rustls::NamedGroup::X25519MLKEM768);
        assert!(
            p.cipher_suites
                .iter()
                .all(|s| { matches!(s, rustls::SupportedCipherSuite::Tls13(_)) })
        );
        assert!(!p.cipher_suites.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ak4_a_key_others_can_read_refuses_the_start() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("k.pem");
        std::fs::write(&key, "x").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        let cert = dir.path().join("c.pem");
        std::fs::write(&cert, "x").unwrap();
        let e = load_server_config(&cert, &key).unwrap_err().to_string();
        assert!(e.contains("readable by others"), "{e}");
    }
}
