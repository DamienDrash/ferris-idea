//! Sending requests over HTTP/1.1 (`SPEC-0046` behaviours 4, 5, 7, 8 and 12).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::coding::{self, Decoded};
use crate::file::ResolvedRequest;

/// Default time allowed for connecting and for each read.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Most redirects followed.
pub const MAX_REDIRECTS: usize = 5;

/// What went wrong sending a request.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// The request still contains unresolved variables.
    #[error("unresolved variables: {0}")]
    Unresolved(String),
    /// The URL cannot be used.
    #[error("invalid URL {0}")]
    Url(String),
    /// A part of the request cannot be sent as it is (behaviour 8).
    #[error("invalid request: {0}")]
    Request(String),
    /// Connecting, TLS, reading or writing failed.
    #[error("{0}")]
    Io(String),
    /// The server's answer is not HTTP.
    #[error("invalid response: {0}")]
    Protocol(String),
}

fn io<E: std::fmt::Display>(e: E) -> HttpError {
    HttpError::Io(e.to_string())
}

/// An error while writing the request or reading the response; a timeout says that it is one.
fn exchange_error(e: std::io::Error) -> HttpError {
    use std::io::ErrorKind;
    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
        HttpError::Io("timed out: the server did not answer within the time allowed".into())
    } else {
        io(e)
    }
}

/// A response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Reason phrase.
    pub reason: String,
    /// Headers in order.
    pub headers: Vec<(String, String)>,
    /// Body bytes as they arrived, content codings still on them.
    #[serde(skip)]
    pub body: Vec<u8>,
    /// Body as text when it is UTF-8, after undoing its content codings (behaviour 12).
    pub text: Option<String>,
    /// Body length in bytes as it arrived.
    pub size: usize,
    /// The body after undoing its content codings; `None` when nothing was undone (behaviour 12).
    pub decoded: Option<Decoded>,
    /// URL that produced this response (after redirects).
    pub url: String,
    /// URLs redirected through, in order.
    pub redirects: Vec<String>,
    /// Time from sending to the end of the body, in milliseconds.
    pub millis: u64,
    /// What the client did that the request does not show: credentials kept back on a redirect, a
    /// redirect that was not followed and why, a body that was decoded (behaviours 5, 7, 12).
    pub notes: Vec<String>,
}

/// How a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendOptions {
    /// Time allowed for connecting and for each read.
    pub timeout: Duration,
    /// Whether redirects are followed (behaviour 7).
    pub follow_redirects: bool,
    /// Whether `Accept-Encoding: gzip, deflate` is added to a request that does not set the header
    /// and has no `Range` (behaviour 4): for requests from request files. A coded body is decoded
    /// either way.
    pub accept_codings: bool,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            follow_redirects: true,
            accept_codings: true,
        }
    }
}

/// The codings asked for (behaviour 4): the ones behaviour 12 decodes.
const ACCEPT_ENCODING: &str = "gzip, deflate";

impl Response {
    /// First header named `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Parts of an `http`/`https` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Whether TLS is used.
    pub tls: bool,
    /// Host name or address (IPv6 without brackets).
    pub host: String,
    /// Port.
    pub port: u16,
    /// Path and query, starting with `/`.
    pub path: String,
}

/// Splits an absolute URL. A fragment (`#…`) is not part of what is sent (RFC 9112 §3.2).
pub fn parse_url(url: &str) -> Result<Target, HttpError> {
    let bad = || HttpError::Url(url.to_owned());
    let without_fragment = url.split('#').next().unwrap_or(url);
    let (tls, rest) = if let Some(r) = without_fragment.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = without_fragment.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(bad());
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest[i..].starts_with('?') => (&rest[..i], format!("/{}", &rest[i..])),
        Some(i) => (&rest[..i], rest[i..].to_owned()),
        None => (rest, "/".to_owned()),
    };
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let default = if tls { 443 } else { 80 };
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or_else(bad)?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| bad())?,
            None => default,
        };
        (h.to_owned(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), p.parse().map_err(|_| bad())?),
            None => (authority.to_owned(), default),
        }
    };
    if host.is_empty() {
        return Err(bad());
    }
    Ok(Target {
        tls,
        host,
        port,
        path,
    })
}

/// The scheme of a URI reference, if it has one (RFC 3986 §3.1).
fn scheme_of(reference: &str) -> Option<&str> {
    let end = reference.find([':', '/', '?', '#'])?;
    let scheme = &reference[..end];
    (reference[end..].starts_with(':')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

/// Removes `.` and `..` segments from an absolute path (RFC 3986 §5.2.4).
fn remove_dot_segments(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let last = segments.len() - 1;
    let mut out: Vec<&str> = Vec::new();
    for (i, segment) in segments.iter().enumerate().skip(1) {
        match *segment {
            "." => {
                if i == last {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if i == last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

/// The URL a `Location` leads to from `base` (RFC 3986 §5; behaviour 7): the fragment is dropped,
/// dot segments are removed. `None` for a location with a scheme other than `http` or `https`.
fn redirect_url(base: &Target, location: &str) -> Option<String> {
    let location = location.split('#').next().unwrap_or_default().trim();
    if let Some(scheme) = scheme_of(location) {
        let lower = scheme.to_ascii_lowercase();
        return matches!(lower.as_str(), "http" | "https")
            .then(|| format!("{lower}{}", &location[scheme.len()..]));
    }
    let scheme = if base.tls { "https" } else { "http" };
    if location.starts_with("//") {
        return Some(format!("{scheme}:{location}"));
    }
    let host = if base.host.contains(':') {
        format!("[{}]", base.host)
    } else {
        base.host.clone()
    };
    let origin = format!("{scheme}://{host}:{}", base.port);
    let base_path = base.path.split('?').next().unwrap_or("/");
    let normal = |reference: &str| match reference.split_once('?') {
        Some((path, query)) => format!("{}?{query}", remove_dot_segments(path)),
        None => remove_dot_segments(reference),
    };
    Some(if location.is_empty() {
        format!("{origin}{}", base.path)
    } else if location.starts_with('?') {
        format!("{origin}{base_path}{location}")
    } else if location.starts_with('/') {
        format!("{origin}{}", normal(location))
    } else {
        let dir = &base_path[..base_path.rfind('/').map_or(0, |i| i + 1)];
        format!("{origin}{}", normal(&format!("{dir}{location}")))
    })
}

/// Whether two targets are the same origin: scheme, host and port.
fn same_origin(a: &Target, b: &Target) -> bool {
    a.tls == b.tls && a.port == b.port && a.host.eq_ignore_ascii_case(&b.host)
}

/// Headers that carry credentials or name the first host; they never follow a redirect to another
/// origin (RFC 9110 §15.4; behaviour 7).
const ORIGIN_BOUND: &[&str] = &["authorization", "proxy-authorization", "cookie", "host"];

/// A token of RFC 9110 §5.6.2: a method or a header name.
fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Refuses a request whose parts could add a header line or a second request (behaviour 8).
fn check_request(method: &str, url: &str, headers: &[(String, String)]) -> Result<(), HttpError> {
    if !is_token(method) {
        return Err(HttpError::Request(format!(
            "the method {method:?} is not a token"
        )));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(HttpError::Request(
            "the URL contains white space or a control character".into(),
        ));
    }
    for (name, value) in headers {
        if !is_token(name) {
            return Err(HttpError::Request(format!(
                "the header name {name:?} is not a token"
            )));
        }
        if value.contains(['\r', '\n', '\0']) {
            return Err(HttpError::Request(format!(
                "the value of the header {name} contains a line break"
            )));
        }
    }
    Ok(())
}

/// The body as text (behaviours 5 and 12): the decoded body when a content coding was undone, else
/// the body as it arrived; UTF-8, or ISO-8859-1 when the content type names that charset. A body
/// that still has a content coding has no text; one that was cut or is damaged ends with its last
/// whole character.
fn body_text(
    headers: &[(String, String)],
    body: &[u8],
    decoded: Option<&Decoded>,
) -> Option<String> {
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.to_ascii_lowercase())
    };
    let body = match decoded {
        Some(d) if !d.remaining.is_empty() => return None,
        Some(d) if d.complete() => &d.body[..],
        Some(d) => coding::whole_characters(&d.body),
        None if body.is_empty() || coding::codings(headers).is_empty() => body,
        None => return None,
    };
    if let Ok(text) = std::str::from_utf8(body) {
        return Some(text.to_owned());
    }
    let content_type = header("content-type")?;
    let charset = content_type
        .split(';')
        .skip(1)
        .find_map(|p| p.trim().strip_prefix("charset="))?
        .trim_matches('"');
    matches!(charset, "iso-8859-1" | "iso_8859-1" | "latin1" | "l1")
        .then(|| body.iter().map(|&b| char::from(b)).collect())
}

/// Status, reason, headers and body of one exchange.
type RawResponse = (u16, String, Vec<(String, String)>, Vec<u8>);

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn connect(target: &Target, timeout: Duration) -> Result<Box<dyn Stream>, HttpError> {
    let addrs = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|e| HttpError::Io(format!("{}: {e}", target.host)))?;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                stream.set_read_timeout(Some(timeout)).map_err(io)?;
                stream.set_write_timeout(Some(timeout)).map_err(io)?;
                if !target.tls {
                    return Ok(Box::new(stream));
                }
                let connector = native_tls::TlsConnector::new().map_err(io)?;
                let tls = connector
                    .connect(&target.host, stream)
                    .map_err(|e| HttpError::Io(format!("TLS with {}: {e}", target.host)))?;
                return Ok(Box::new(tls));
            }
            Err(e) => last = Some(e),
        }
    }
    Err(HttpError::Io(format!(
        "cannot connect to {}:{}: {}",
        target.host,
        target.port,
        last.map_or_else(|| "no address".to_owned(), |e| e.to_string())
    )))
}

fn has(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

fn once(
    method: &str,
    target: &Target,
    headers: &[(String, String)],
    body: &[u8],
    timeout: Duration,
) -> Result<RawResponse, HttpError> {
    let mut collected = Vec::new();
    let (status, reason, response_headers) = once_into(
        method,
        target,
        headers,
        body,
        timeout,
        &mut |bytes: &[u8]| {
            collected.extend_from_slice(bytes);
            true
        },
    )?;
    Ok((status, reason, response_headers, collected))
}

/// Status, reason phrase and headers of a response.
type ResponseHead = (u16, String, Vec<(String, String)>);

/// Sends a request and hands the response body to `sink` piece by piece as it arrives; `sink`
/// returning false stops reading. Status, reason and headers.
fn once_into(
    method: &str,
    target: &Target,
    headers: &[(String, String)],
    body: &[u8],
    timeout: Duration,
    sink: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<ResponseHead, HttpError> {
    check_request(method, &target.path, headers)?;
    let mut stream = connect(target, timeout)?;
    let mut head = format!("{method} {} HTTP/1.1\r\n", target.path);
    if !has(headers, "host") {
        let default = if target.tls { 443 } else { 80 };
        let host = if target.host.contains(':') {
            format!("[{}]", target.host)
        } else {
            target.host.clone()
        };
        if target.port == default {
            head.push_str(&format!("Host: {host}\r\n"));
        } else {
            head.push_str(&format!("Host: {host}:{}\r\n", target.port));
        }
    }
    if !has(headers, "user-agent") {
        head.push_str("User-Agent: FerrisIDEA\r\n");
    }
    if !has(headers, "connection") {
        head.push_str("Connection: close\r\n");
    }
    // A method that defines content says how much there is, also when there is none (RFC 9110 §8.6).
    let expects_content = matches!(method, "POST" | "PUT" | "PATCH");
    if (!body.is_empty() || expects_content)
        && !has(headers, "content-length")
        && !has(headers, "transfer-encoding")
    {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(exchange_error)?;
    stream.write_all(body).map_err(exchange_error)?;
    stream.flush().map_err(exchange_error)?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    // Skip interim 1xx responses.
    let (status, reason, response_headers) = loop {
        line.clear();
        reader.read_line(&mut line).map_err(exchange_error)?;
        let status_line = line.trim_end().to_owned();
        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().unwrap_or_default();
        if !version.starts_with("HTTP/") {
            return Err(HttpError::Protocol(format!("status line {status_line:?}")));
        }
        let status: u16 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| HttpError::Protocol(format!("status line {status_line:?}")))?;
        let reason = parts.next().unwrap_or_default().to_owned();
        let mut hs = Vec::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(exchange_error)? == 0 {
                break;
            }
            let l = line.trim_end();
            if l.is_empty() {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                hs.push((k.trim().to_owned(), v.trim().to_owned()));
            }
        }
        if !(100..200).contains(&status) {
            break (status, reason, hs);
        }
    };
    let no_body = method == "HEAD" || status == 204 || status == 304;
    let chunked = response_headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    let length = response_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok());
    let mut piece = vec![0u8; 8192];
    if no_body {
    } else if chunked {
        loop {
            line.clear();
            reader.read_line(&mut line).map_err(exchange_error)?;
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16)
                .map_err(|_| HttpError::Protocol(format!("chunk size {:?}", line.trim())))?;
            if size == 0 {
                // Trailer section until the empty line.
                loop {
                    line.clear();
                    if reader.read_line(&mut line).map_err(exchange_error)? == 0
                        || line.trim().is_empty()
                    {
                        break;
                    }
                }
                break;
            }
            let mut chunk = vec![0; size];
            reader.read_exact(&mut chunk).map_err(exchange_error)?;
            if !sink(&chunk) {
                return Ok((status, reason, response_headers));
            }
            line.clear();
            reader.read_line(&mut line).map_err(exchange_error)?;
        }
    } else if let Some(n) = length {
        let mut left = n;
        while left > 0 {
            let want = left.min(piece.len());
            reader
                .read_exact(&mut piece[..want])
                .map_err(exchange_error)?;
            left -= want;
            if !sink(&piece[..want]) {
                break;
            }
        }
    } else {
        loop {
            let got = reader.read(&mut piece).map_err(exchange_error)?;
            if got == 0 || !sink(&piece[..got]) {
                break;
            }
        }
    }
    Ok((status, reason, response_headers))
}

/// Sends `request` once (no redirects) and hands the response body to `sink` as it arrives, e.g.
/// server-sent events; `sink` returning false stops. Status and headers (`SPEC-0157`).
pub fn send_streaming(
    request: &ResolvedRequest,
    timeout: Duration,
    sink: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<(u16, Vec<(String, String)>), HttpError> {
    if !request.unresolved.is_empty() {
        return Err(HttpError::Unresolved(request.unresolved.join(", ")));
    }
    check_request(&request.method, &request.url, &request.headers)?;
    let target = parse_url(&request.url)?;
    let (status, _, headers) = once_into(
        &request.method,
        &target,
        &request.headers,
        request.body.as_bytes(),
        timeout,
        sink,
    )?;
    Ok((status, headers))
}

/// Sends `request` once without following redirects, for callers whose headers must not reach
/// another server (an API key, `SPEC-0138`). No `Accept-Encoding` is added.
pub fn send_once(request: &ResolvedRequest, timeout: Duration) -> Result<Response, HttpError> {
    send_with(
        request,
        &SendOptions {
            timeout,
            follow_redirects: false,
            accept_codings: false,
        },
    )
}

/// Sends `request`, following redirects (behaviours 4, 5, 7). No `Accept-Encoding` is added: the
/// callers are parts of Ferris IDEA that read the body as it arrives.
pub fn send(request: &ResolvedRequest, timeout: Duration) -> Result<Response, HttpError> {
    send_with(
        request,
        &SendOptions {
            timeout,
            follow_redirects: true,
            accept_codings: false,
        },
    )
}

/// Where the redirect `location` leads from `target`, or the note saying why it is not followed
/// (behaviour 7): only `http` and `https`, and never from `https` to `http`.
fn next_hop(target: &Target, location: &str) -> Result<(String, Target), String> {
    let Some(next) = redirect_url(target, location) else {
        return Err(format!(
            "The redirect to {location} was not followed: only http and https locations are followed."
        ));
    };
    let Ok(to) = parse_url(&next) else {
        return Err(format!(
            "The redirect to {next} was not followed: the location is not a usable URL."
        ));
    };
    if target.tls && !to.tls {
        return Err(format!(
            "The redirect to {next} was not followed: it leaves https for http."
        ));
    }
    Ok((next, to))
}

/// Takes the headers bound to the first origin out of `headers`; their names as written.
fn keep_back(headers: &mut Vec<(String, String)>) -> Vec<String> {
    let mut kept_back: Vec<String> = Vec::new();
    headers.retain(|(name, _)| {
        let bound = ORIGIN_BOUND.iter().any(|b| name.eq_ignore_ascii_case(b));
        if bound && !kept_back.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            kept_back.push(name.clone());
        }
        !bound
    });
    kept_back
}

/// Sends `request` as `options` say (behaviours 4, 5, 7, 8, 12).
pub fn send_with(request: &ResolvedRequest, options: &SendOptions) -> Result<Response, HttpError> {
    if !request.unresolved.is_empty() {
        return Err(HttpError::Unresolved(request.unresolved.join(", ")));
    }
    check_request(&request.method, &request.url, &request.headers)?;
    let started = Instant::now();
    let mut method = request.method.clone();
    let mut body = request.body.clone().into_bytes();
    let mut headers = request.headers.clone();
    if options.accept_codings && !has(&headers, "accept-encoding") && !has(&headers, "range") {
        headers.push(("Accept-Encoding".into(), ACCEPT_ENCODING.into()));
    }
    let mut url = request.url.clone();
    let mut redirects = Vec::new();
    let mut notes = Vec::new();
    loop {
        let target = parse_url(&url)?;
        let (status, reason, response_headers, response_body) =
            once(&method, &target, &headers, &body, options.timeout)?;
        let location = response_headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("location"))
            .map(|(_, v)| v.clone());
        if let (true, Some(location)) = (
            options.follow_redirects && matches!(status, 301 | 302 | 303 | 307 | 308),
            location,
        ) {
            if redirects.len() >= MAX_REDIRECTS {
                notes.push(format!(
                    "Stopped after {MAX_REDIRECTS} redirects; the next one leads to {location}."
                ));
            } else {
                match next_hop(&target, &location) {
                    Err(why) => notes.push(why),
                    Ok((next, to)) => {
                        if !same_origin(&target, &to) {
                            let kept_back = keep_back(&mut headers);
                            if !kept_back.is_empty() {
                                notes.push(format!(
                                    "Not sent to {}:{}, another origin than {}:{}: {}.",
                                    to.host,
                                    to.port,
                                    target.host,
                                    target.port,
                                    kept_back.join(", ")
                                ));
                            }
                        }
                        if status == 303 || (matches!(status, 301 | 302) && method == "POST") {
                            method = "GET".into();
                            body.clear();
                            headers.retain(|(k, _)| {
                                !k.eq_ignore_ascii_case("content-length")
                                    && !k.eq_ignore_ascii_case("content-type")
                            });
                        }
                        redirects.push(url.clone());
                        url = next;
                        continue;
                    }
                }
            }
        }
        let decoded = coding::decode(&response_headers, &response_body);
        if let Some(d) = &decoded {
            notes.push(d.note(response_body.len()));
        }
        let text = body_text(&response_headers, &response_body, decoded.as_ref());
        return Ok(Response {
            status,
            reason,
            headers: response_headers,
            size: response_body.len(),
            body: response_body,
            text,
            decoded,
            url,
            redirects,
            millis: started.elapsed().as_millis() as u64,
            notes,
        });
    }
}

/// Fetches a small text file over `https` for an import the user started (`SPEC-0302` behaviour 1):
/// GET without cookies or credentials, at most `max_redirects` redirects (each to `https`), at most
/// `max_bytes` of body. Returns the final URL and the text.
pub fn fetch_small_text(
    url: &str,
    max_bytes: usize,
    max_redirects: usize,
    timeout: Duration,
) -> Result<(String, String), String> {
    let mut url = url.trim().to_owned();
    for _ in 0..=max_redirects {
        if !url.starts_with("https://") {
            return Err(format!("only https:// addresses are fetched, not {url}"));
        }
        let request = ResolvedRequest {
            name: None,
            line: 0,
            method: "GET".into(),
            url: url.clone(),
            headers: vec![
                (
                    "User-Agent".into(),
                    format!("FerrisIDEA/{}", env!("CARGO_PKG_VERSION")),
                ),
                ("Accept".into(), "text/plain, application/toml, */*".into()),
            ],
            body: String::new(),
            unresolved: Vec::new(),
        };
        let mut body = Vec::new();
        let mut too_big = false;
        let (status, headers) = send_streaming(&request, timeout, &mut |piece| {
            body.extend_from_slice(piece);
            too_big = body.len() > max_bytes;
            !too_big
        })
        .map_err(|e| e.to_string())?;
        if (300..400).contains(&status) {
            let location = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("redirect {status} without a location"))?;
            url = if location.starts_with('/') {
                let target = parse_url(&url).map_err(|e| e.to_string())?;
                format!("https://{}{location}", target.host)
            } else {
                location
            };
            continue;
        }
        if too_big {
            return Err(format!("the file is larger than {} KiB", max_bytes / 1024));
        }
        if status != 200 {
            return Err(format!("the server answered {status}"));
        }
        let text = String::from_utf8(body).map_err(|_| "the file is not UTF-8 text".to_owned())?;
        return Ok((url, text));
    }
    Err(format!("more than {max_redirects} redirects"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let t = parse_url("https://example.com/a/b?x=1").unwrap();
        assert_eq!(
            (t.tls, t.host.as_str(), t.port, t.path.as_str()),
            (true, "example.com", 443, "/a/b?x=1")
        );
        let t = parse_url("http://localhost:8080?q").unwrap();
        assert_eq!((t.port, t.path.as_str()), (8080, "/?q"));
        let t = parse_url("http://[::1]:9000/x").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("::1", 9000));
        assert!(parse_url("ftp://x").is_err());
        assert!(parse_url("http://host:port/").is_err());
        // A fragment is not sent.
        let t = parse_url("http://h/a?x=1#part").unwrap();
        assert_eq!(t.path, "/a?x=1");
        assert_eq!(parse_url("http://h#part").unwrap().path, "/");
    }

    #[test]
    fn redirect_locations() {
        let base = parse_url("http://h:81/dir/page?x").unwrap();
        let to = |location: &str| redirect_url(&base, location);
        assert_eq!(to("/root").as_deref(), Some("http://h:81/root"));
        assert_eq!(to("next").as_deref(), Some("http://h:81/dir/next"));
        assert_eq!(to("https://o/").as_deref(), Some("https://o/"));
        assert_eq!(to("HTTP://o/x").as_deref(), Some("http://o/x"));
        assert_eq!(to("../up?q=1").as_deref(), Some("http://h:81/up?q=1"));
        assert_eq!(to("./a/./b/../c/").as_deref(), Some("http://h:81/dir/a/c/"));
        assert_eq!(to("../../../x").as_deref(), Some("http://h:81/x"));
        assert_eq!(to("/a/b/..").as_deref(), Some("http://h:81/a/"));
        assert_eq!(to("?y=2").as_deref(), Some("http://h:81/dir/page?y=2"));
        assert_eq!(to("").as_deref(), Some("http://h:81/dir/page?x"));
        assert_eq!(to("//other:82/p").as_deref(), Some("http://other:82/p"));
        assert_eq!(to("/p#frag").as_deref(), Some("http://h:81/p"));
        // The query is not a path: its dots stay.
        assert_eq!(
            to("/p?next=../x").as_deref(),
            Some("http://h:81/p?next=../x")
        );
        for other in [
            "file:///etc/hosts",
            "ftp://h/x",
            "javascript:alert(1)",
            "data:text/plain,x",
        ] {
            assert_eq!(to(other), None, "{other}");
        }
        let secure = parse_url("https://h/a").unwrap();
        assert_eq!(
            redirect_url(&secure, "//o/p").as_deref(),
            Some("https://o/p")
        );
        let v6 = parse_url("http://[::1]:9000/x").unwrap();
        assert_eq!(
            redirect_url(&v6, "y").as_deref(),
            Some("http://[::1]:9000/y")
        );

        // A redirect never leaves TLS, and only http and https are followed.
        let plain = next_hop(&secure, "http://h/a").unwrap_err();
        assert!(plain.contains("leaves https for http"), "{plain}");
        assert!(
            next_hop(&secure, "//h:8443/a")
                .is_ok_and(|(url, to)| to.tls && url == "https://h:8443/a")
        );
        assert!(next_hop(&base, "https://h/a").is_ok_and(|(_, to)| to.tls));
        assert!(
            next_hop(&base, "file:///etc/hosts")
                .unwrap_err()
                .contains("only http and https")
        );
        assert!(
            next_hop(&base, "http://h:port/")
                .unwrap_err()
                .contains("not a usable URL")
        );
        let mut headers: Vec<(String, String)> = [
            "Authorization",
            "X-Kept",
            "cookie",
            "Cookie",
            "PROXY-AUTHORIZATION",
            "Host",
            "Accept",
        ]
        .iter()
        .map(|k| (k.to_string(), "v".to_owned()))
        .collect();
        assert_eq!(
            keep_back(&mut headers),
            ["Authorization", "cookie", "PROXY-AUTHORIZATION", "Host"]
        );
        assert_eq!(
            headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["X-Kept", "Accept"]
        );

        let origin = |url: &str| parse_url(url).unwrap();
        assert!(same_origin(&origin("http://H:80/a"), &origin("http://h/b")));
        assert!(!same_origin(&origin("http://h/a"), &origin("https://h/a")));
        assert!(!same_origin(
            &origin("http://h/a"),
            &origin("http://h:81/a")
        ));
        assert!(!same_origin(
            &origin("http://127.0.0.1/a"),
            &origin("http://localhost/a")
        ));
    }

    #[test]
    fn requests_are_checked() {
        let check = |method: &str, url: &str, headers: &[(&str, &str)]| {
            let headers: Vec<(String, String)> = headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            check_request(method, url, &headers).map_err(|e| e.to_string())
        };
        assert!(
            check(
                "GET",
                "http://h/a?b=c",
                &[("X-A", "one two"), ("Accept", "*/*")]
            )
            .is_ok()
        );
        assert!(check("M-SEARCH", "http://h/", &[]).is_ok());
        let refused = [
            check("GET /x HTTP/1.1\r\nX: y", "http://h/", &[]),
            check("", "http://h/", &[]),
            check("GET", "http://h/a b", &[]),
            check("GET", "http://h/a\r\nX-Injected: 1", &[]),
            check("GET", "http://h/", &[("X-A", "v\r\nX-Injected: 1")]),
            check("GET", "http://h/", &[("X-A", "v\nX-Injected: 1")]),
            check("GET", "http://h/", &[("X A", "v")]),
            check("GET", "http://h/", &[("X-A\r\nX-B", "v")]),
            check("GET", "http://h/", &[("", "v")]),
        ];
        for r in refused {
            assert!(r.unwrap_err().starts_with("invalid request: "));
        }
        // Nothing is opened for a refused request: port 1 would refuse the connection otherwise.
        let request = ResolvedRequest {
            name: None,
            line: 1,
            method: "GET".into(),
            url: "http://127.0.0.1:1/".into(),
            headers: vec![("X-A".into(), "v\r\nX-Injected: 1".into())],
            body: String::new(),
            unresolved: Vec::new(),
        };
        let e = send(&request, Duration::from_secs(1)).unwrap_err();
        assert!(e.to_string().contains("line break"), "{e}");
    }

    #[test]
    fn body_texts() {
        let headers = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(body_text(&[], b"plain", None).as_deref(), Some("plain"));
        assert_eq!(body_text(&[], &[0xff, 0x00], None), None);
        let latin = headers(&[("Content-Type", "text/plain; charset=ISO-8859-1")]);
        assert_eq!(
            body_text(&latin, &[0x47, 0x72, 0xfc, 0xdf, 0x65], None).as_deref(),
            Some("Grüße")
        );
        // A coding that is not decoded leaves no text; an empty body is empty text.
        let coded = headers(&[("Content-Encoding", "br")]);
        assert_eq!(body_text(&coded, b"looks like text", None), None);
        assert_eq!(body_text(&coded, b"", None).as_deref(), Some(""));
        let identity = headers(&[("Content-Encoding", "identity")]);
        assert_eq!(body_text(&identity, b"text", None).as_deref(), Some("text"));

        // The text of a decoded body is the decoded bytes (behaviour 12).
        let gzip = headers(&[("Content-Encoding", "gzip")]);
        let mut decoded = Decoded {
            codings: vec!["gzip".into()],
            remaining: Vec::new(),
            body: "Grüße".as_bytes().to_vec(),
            size: 7,
            cut: false,
            damaged: None,
        };
        let text = |d: &Decoded| body_text(&gzip, &[0x1f, 0x8b], Some(d));
        assert_eq!(text(&decoded).as_deref(), Some("Grüße"));
        // Cut in the middle of a character: the text ends before it.
        decoded.body.truncate(3);
        assert_eq!(text(&decoded), None, "a whole body that is not UTF-8");
        decoded.cut = true;
        assert_eq!(text(&decoded).as_deref(), Some("Gr"));
        decoded.remaining = vec!["br".into()];
        assert_eq!(text(&decoded), None, "still coded");
    }
}
