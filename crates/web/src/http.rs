//! The HTTP layer of the endpoint (`SPEC-0325` behaviours 7–10): a request head read with limits,
//! host and origin checks, and the security headers on every answer.

use tokio::io::{AsyncRead, AsyncReadExt};

/// The longest request head.
pub const MAX_HEAD: usize = 16 << 10;

/// The most header fields.
pub const MAX_HEADERS: usize = 64;

/// The security headers every answer carries.
pub const SECURITY_HEADERS: [(&str, &str); 7] = [
    (
        "Content-Security-Policy",
        "default-src 'none'; script-src 'self'; connect-src 'self'; style-src 'self'; \
         base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Cross-Origin-Embedder-Policy", "require-corp"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("Cache-Control", "no-store"),
];

/// A parsed request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The request method.
    pub method: String,
    /// The target as sent.
    pub path: String,
    /// Header names in lower case with their values.
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// The value of the header `name` (lower case) when it occurs exactly once.
    pub fn header(&self, name: &str) -> Option<&str> {
        let mut found = self.headers.iter().filter(|(n, _)| n == name);
        let first = found.next()?;
        if found.next().is_some() {
            return None;
        }
        Some(first.1.as_str())
    }

    /// Whether this asks for a WebSocket upgrade.
    pub fn is_upgrade(&self) -> bool {
        self.header("upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    }
}

/// Why a head could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum HeadError {
    /// Over the limits.
    TooLarge,
    /// Not HTTP, or bytes after the head.
    Malformed,
    /// The connection ended first.
    Closed,
}

/// Reads one request head; nothing may follow it before the answer.
pub async fn read_head<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Head, HeadError> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .await
            .map_err(|_| HeadError::Closed)?;
        if n == 0 {
            return Err(HeadError::Closed);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            if end + 4 != buf.len() {
                return Err(HeadError::Malformed);
            }
            return parse(&buf);
        }
        if buf.len() > MAX_HEAD {
            return Err(HeadError::TooLarge);
        }
    }
}

fn parse(bytes: &[u8]) -> Result<Head, HeadError> {
    if bytes.len() > MAX_HEAD {
        return Err(HeadError::TooLarge);
    }
    let mut fields = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut fields);
    match request.parse(bytes) {
        Ok(httparse::Status::Complete(_)) => {}
        Err(httparse::Error::TooManyHeaders) => return Err(HeadError::TooLarge),
        _ => return Err(HeadError::Malformed),
    }
    if request.version != Some(1) {
        return Err(HeadError::Malformed);
    }
    let headers = request
        .headers
        .iter()
        .map(|h| {
            std::str::from_utf8(h.value)
                .map(|v| (h.name.to_ascii_lowercase(), v.trim().to_owned()))
                .map_err(|_| HeadError::Malformed)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Head {
        method: request.method.ok_or(HeadError::Malformed)?.to_owned(),
        path: request.path.ok_or(HeadError::Malformed)?.to_owned(),
        headers,
    })
}

/// An answer with the security headers; `strict` adds HSTS (non-loopback listeners).
pub fn response(status: &str, content_type: &str, body: &[u8], strict: bool) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    push_security(&mut out, strict);
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// The `101` answer for an accepted upgrade.
pub fn switching(accept: &str, strict: bool) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n"
    );
    push_security(&mut out, strict);
    out.push_str("\r\n");
    out.into_bytes()
}

fn push_security(out: &mut String, strict: bool) {
    for (name, value) in SECURITY_HEADERS {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    if strict {
        out.push_str("Strict-Transport-Security: max-age=31536000\r\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn head_of(bytes: &[u8]) -> Result<Head, HeadError> {
        let mut r = bytes;
        read_head(&mut r).await
    }

    fn block_on<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn a_head_parses_and_lower_cases_names() {
        let h = block_on(head_of(
            b"GET / HTTP/1.1\r\nHost: a:1\r\nUpgrade: WebSocket\r\n\r\n",
        ))
        .unwrap();
        assert_eq!(h.method, "GET");
        assert_eq!(h.header("host"), Some("a:1"));
        assert!(h.is_upgrade());
    }

    #[test]
    fn ak5_limits_and_malformed_heads_are_refused() {
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD + 10));
        assert_eq!(block_on(head_of(long.as_bytes())), Err(HeadError::TooLarge));
        let mut many = String::from("GET / HTTP/1.1\r\n");
        for i in 0..=MAX_HEADERS {
            many.push_str(&format!("X{i}: 1\r\n"));
        }
        many.push_str("\r\n");
        assert_eq!(block_on(head_of(many.as_bytes())), Err(HeadError::TooLarge));
        assert_eq!(
            block_on(head_of(b"GET / HTTP/1.1\r\n\r\nextra")),
            Err(HeadError::Malformed)
        );
        assert_eq!(
            block_on(head_of(b"nonsense\r\n\r\n")),
            Err(HeadError::Malformed)
        );
    }

    #[test]
    fn a_duplicated_header_counts_as_absent() {
        let h = block_on(head_of(
            b"GET / HTTP/1.1\r\nOrigin: https://a\r\nOrigin: https://b\r\n\r\n",
        ))
        .unwrap();
        assert_eq!(h.header("origin"), None);
    }

    #[test]
    fn ak6_answers_carry_the_security_headers() {
        let text = String::from_utf8(response("200 OK", "text/html", b"x", false)).unwrap();
        for (name, _) in SECURITY_HEADERS {
            assert!(text.contains(name), "{name}");
        }
        assert!(!text.contains("Strict-Transport"));
        let strict = String::from_utf8(response("200 OK", "text/html", b"x", true)).unwrap();
        assert!(strict.contains("Strict-Transport-Security"));
    }
}
