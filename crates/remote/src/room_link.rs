//! Room links that lead to their host (`SPEC-0323` v7 behaviours 44–47): what a link carries so
//! that the joiner can check the host before connecting, reading it strictly, and keeping its
//! secret out of every text but the link itself.
//!
//! `ferris-idea://room/<room>?host=<host>&port=<port>&path=<folder>&fp=<fingerprint>&role=<role>&expires=<end>#<secret>`

use std::fmt;

/// The longest link read (behaviour 46).
pub const MAX_LINK: usize = 4096;

/// The roles a link may hand out (behaviour 12).
pub const ROLES: [&str; 5] = ["viewer", "commenter", "editor", "runner", "admin"];

const PREFIX: &str = "ferris-idea://room/";
/// The prefix of links made under the product's former name, still read (`SPEC-0356`).
const FORMER_PREFIX: &str = "idecraft://room/"; // rebrand:keep

/// A full room link, read.
#[derive(Clone, PartialEq, Eq)]
pub struct RoomLink {
    /// The room's id (hex).
    pub room: String,
    /// The host as `ssh` reaches it (no login).
    pub host: String,
    /// Its SSH port.
    pub port: u16,
    /// The project folder on the host (absolute).
    pub folder: String,
    /// The host key's fingerprint (`SHA256:…`).
    pub fingerprint: String,
    /// The role it hands out.
    pub role: String,
    /// Its end in Unix seconds, if any.
    pub expires: Option<u64>,
    /// The bearer secret (64 hex digits); never printed.
    secret: String,
}

impl fmt::Debug for RoomLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomLink")
            .field("room", &self.room)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("folder", &self.folder)
            .field("fingerprint", &self.fingerprint)
            .field("role", &self.role)
            .field("expires", &self.expires)
            .field("secret", &mask(&self.secret))
            .finish()
    }
}

/// Why a link was refused (behaviour 46); never holds the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Not a link at all, or a part is malformed: the text names the part.
    Malformed(String),
    /// The link's end has passed (Unix seconds).
    Expired(u64),
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "This is not a usable room link: {why}"),
            Self::Expired(_) => write!(f, "This link has ended"),
        }
    }
}

fn bad(why: impl Into<String>) -> LinkError {
    LinkError::Malformed(why.into())
}

/// `secret` as the dialog shows it: `••••` and its last four characters.
pub fn mask(secret: &str) -> String {
    let tail: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
}

fn hex_of(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `host` is a host a link may name: a DNS name or IPv4 address, never a login, an
/// option or an IPv6 literal.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Percent-encodes a query value; `/` and `:` stay (RFC 3986 allows them in a query).
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(value: &str, part: &str) -> Result<String, LinkError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value
                .get(i + 1..i + 3)
                .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| bad(format!("{part} has a broken %-escape")))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| bad(part.to_owned()))?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| bad(format!("{part} is not UTF-8")))
}

impl RoomLink {
    /// Reads `text` (behaviour 46); a link whose end is before `now` (Unix seconds) is expired.
    pub fn parse(text: &str, now: u64) -> Result<Self, LinkError> {
        let text = text.trim();
        if text.len() > MAX_LINK {
            return Err(bad(format!("it is longer than {MAX_LINK} characters")));
        }
        let rest = text
            .strip_prefix(PREFIX)
            .or_else(|| text.strip_prefix(FORMER_PREFIX))
            .ok_or_else(|| bad("it does not start with ferris-idea://room/"))?;
        let (rest, secret) = rest
            .split_once('#')
            .ok_or_else(|| bad("the secret after # is missing"))?;
        if !hex_of(secret, 64, 64) {
            return Err(bad("the secret is not 64 hex digits"));
        }
        let (room, query) = rest.split_once('?').unwrap_or((rest, ""));
        if !hex_of(room, 1, 64) {
            return Err(bad("the room id is not 1 to 64 hex digits"));
        }
        let mut host = None;
        let mut port = None;
        let mut folder = None;
        let mut fp = None;
        let mut role = None;
        let mut expires = None;
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let slot = match key {
                "host" => &mut host,
                "port" => &mut port,
                "path" => &mut folder,
                "fp" => &mut fp,
                "role" => &mut role,
                "expires" => &mut expires,
                _ => {
                    return Err(bad(format!(
                        "it has a part `{}` this Ferris IDEA does not know (made by a newer Ferris IDEA?)",
                        key.chars().take(32).collect::<String>()
                    )));
                }
            };
            if slot.is_some() {
                return Err(bad(format!("`{key}` is given twice")));
            }
            *slot = Some(decode(value, key)?);
        }
        let host = host.ok_or_else(|| bad("the host is missing"))?;
        if !valid_host(&host) {
            return Err(bad(
                "the host is not a host name or IPv4 address (no login, no option, no IPv6 literal)",
            ));
        }
        let port: u16 = port
            .ok_or_else(|| bad("the port is missing"))?
            .parse()
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| bad("the port is not between 1 and 65535"))?;
        let folder = folder.ok_or_else(|| bad("the project folder is missing"))?;
        if !crate::bootstrap::absolute_folder(&folder)
            || folder.chars().any(char::is_control)
            || folder.len() > MAX_LINK
        {
            return Err(bad(
                "the project folder is not an absolute path without control characters",
            ));
        }
        let fingerprint = fp.ok_or_else(|| bad("the host key fingerprint is missing"))?;
        let fingerprint = crate::known_hosts::parse_fingerprint(&fingerprint)
            .ok_or_else(|| bad("the host key fingerprint is not of the form SHA256:…"))?;
        let role = role.unwrap_or_else(|| "viewer".to_owned());
        if !ROLES.contains(&role.as_str()) {
            return Err(bad(
                "the role is not one of viewer, commenter, editor, runner, admin",
            ));
        }
        let expires = match expires {
            None => None,
            Some(e) => Some(
                e.parse::<u64>()
                    .map_err(|_| bad("the end is not a number of seconds"))?,
            ),
        };
        if let Some(end) = expires
            && end <= now
        {
            return Err(LinkError::Expired(end));
        }
        Ok(Self {
            room: room.to_owned(),
            host,
            port,
            folder,
            fingerprint,
            role,
            expires,
            secret: secret.to_owned(),
        })
    }

    /// The whole link, secret included: only for `room.join` and the sharer's Copy.
    pub fn text(&self) -> String {
        let mut q = format!(
            "host={}&port={}&path={}&fp={}&role={}",
            encode(&self.host),
            self.port,
            encode(&self.folder),
            encode(&self.fingerprint),
            encode(&self.role)
        );
        if let Some(e) = self.expires {
            q.push_str(&format!("&expires={e}"));
        }
        format!("{PREFIX}{}?{q}#{}", self.room, self.secret)
    }

    /// The link as the dialog shows it after pasting: the secret masked (behaviour 47).
    pub fn masked(&self) -> String {
        let full = self.text();
        let cut = full.rfind('#').unwrap_or(full.len());
        format!("{}#{}", &full[..cut], mask(&self.secret))
    }

    /// The full link from the backend's short one (`ferris-idea://room/<id>#<secret>`) and what the
    /// sharer's window knows of its host (behaviour 44); `None` when `short` is not a short link
    /// or a part is unusable.
    #[allow(clippy::too_many_arguments)]
    pub fn compose(
        short: &str,
        host: &str,
        port: u16,
        folder: &str,
        fingerprint: &str,
        role: &str,
        expires: Option<u64>,
    ) -> Option<Self> {
        let (room, secret) = short.strip_prefix(PREFIX)?.split_once('#')?;
        let link = Self {
            room: room.to_owned(),
            host: host.to_owned(),
            port,
            folder: folder.to_owned(),
            fingerprint: fingerprint.to_owned(),
            role: role.to_owned(),
            expires,
            secret: secret.to_owned(),
        };
        // Whatever is composed must read back the same (never a link the joiner refuses).
        (Self::parse(&link.text(), 0).ok()? == link).then_some(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const FP: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";

    fn full() -> String {
        format!(
            "ferris-idea://room/ab12?host=dev.example.com&port=2222&path=/srv/my%20app&fp=SHA256:47DEQpj8HBSa%2B/TImW%2B5JCeuQeRkm5NMpJWZG3hSuFU&role=editor&expires=2000#{SECRET}"
        )
    }

    #[test]
    fn a_full_link_is_read_into_its_parts() {
        let l = RoomLink::parse(&full(), 1000).unwrap();
        assert_eq!(l.room, "ab12");
        assert_eq!(l.host, "dev.example.com");
        assert_eq!(l.port, 2222);
        assert_eq!(l.folder, "/srv/my app");
        assert_eq!(l.fingerprint, FP);
        assert_eq!(l.role, "editor");
        assert_eq!(l.expires, Some(2000));
        assert_eq!(l.text(), full());
        assert_eq!(RoomLink::parse(&l.text(), 1000).unwrap(), l);
        // A link made under the former name is read the same and written with today's prefix.
        let former = full().replacen(PREFIX, FORMER_PREFIX, 1);
        assert!(former.starts_with(FORMER_PREFIX) || PREFIX == FORMER_PREFIX);
        assert_eq!(RoomLink::parse(&former, 1000).unwrap(), l);
    }

    #[test]
    fn the_secret_never_shows_but_in_the_link() {
        let l = RoomLink::parse(&full(), 1000).unwrap();
        let debug = format!("{l:?}");
        assert!(!debug.contains(SECRET), "{debug}");
        assert!(debug.contains("••••cdef"));
        let masked = l.masked();
        assert!(!masked.contains(SECRET));
        assert!(masked.ends_with("#••••cdef"), "{masked}");
        assert!(masked.contains("host=dev.example.com"));
    }

    #[test]
    fn an_ended_link_is_expired() {
        assert_eq!(
            RoomLink::parse(&full(), 2000),
            Err(LinkError::Expired(2000))
        );
        assert_eq!(
            RoomLink::parse(&full(), 5000),
            Err(LinkError::Expired(2000))
        );
    }

    #[test]
    fn every_malformed_part_is_named_never_the_secret() {
        let with = |from: &str, to: &str| full().replacen(from, to, 1);
        let cases: Vec<(String, &str)> = vec![
            ("https://example.com/room".into(), "ferris-idea://room/"),
            (format!("{}{}", full(), "x".repeat(MAX_LINK)), "longer"),
            (full().replace(&format!("#{SECRET}"), ""), "secret"),
            (with(SECRET, "abc"), "secret"),
            (with("room/ab12", "room/xyz"), "room id"),
            (with("&port=2222", "&port=2222&port=22"), "twice"),
            (with("&role=editor", "&role=editor&shell=1"), "newer"),
            (with("host=dev.example.com&", ""), "host is missing"),
            (with("dev.example.com", "-oProxyCommand=x"), "host"),
            (with("dev.example.com", "me@dev"), "host"),
            (with("dev.example.com", "[::1]"), "host"),
            (with("dev.example.com", "dev%20box"), "host"),
            (with("port=2222", "port=0"), "port"),
            (with("port=2222", "port=70000"), "port"),
            (with("&port=2222", ""), "port is missing"),
            (with("path=/srv/my%20app", "path=srv"), "folder"),
            (with("path=/srv/my%20app", "path=/srv/%0Ax"), "folder"),
            (with("&path=/srv/my%20app", ""), "folder is missing"),
            (with("fp=SHA256:", "fp=MD5:"), "fingerprint"),
            (with("&fp=", "&nofp="), "newer"),
            (with("role=editor", "role=owner"), "role"),
            (with("expires=2000", "expires=soon"), "end"),
            (with("path=/srv/my%20app", "path=/srv/%zz"), "escape"),
        ];
        for (link, part) in cases {
            match RoomLink::parse(&link, 1000) {
                Err(LinkError::Malformed(why)) => {
                    assert!(why.contains(part), "{link}: {why} should name {part}");
                    assert!(!why.contains(SECRET));
                }
                other => panic!("{link}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_composed_link_reads_back() {
        let short = format!("ferris-idea://room/ab12#{SECRET}");
        let l =
            RoomLink::compose(&short, "10.0.0.5", 22, "/srv/a b", FP, "viewer", Some(9)).unwrap();
        let back = RoomLink::parse(&l.text(), 0).unwrap();
        assert_eq!(back, l);
        assert_eq!(back.folder, "/srv/a b");
        assert!(RoomLink::compose(&short, "-x", 22, "/srv", FP, "viewer", None).is_none());
        assert!(
            RoomLink::compose("ferris-idea://room/ab12", "h", 22, "/s", FP, "viewer", None)
                .is_none()
        );
        assert!(RoomLink::compose(&short, "h", 22, "/s", "SHA256:x", "viewer", None).is_none());
    }
}
