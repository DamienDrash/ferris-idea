//! `session.hello`: protocol name, version choice without downgrade, resume tokens (`ADR-0026` §4,
//! `ADR-0025` §3).

use crate::rpc::{ErrorCode, RpcError};

/// The protocol name in `session.hello`.
pub const PROTOCOL: &str = "idecraft-session";

/// The major this build speaks first.
pub const MAJOR: u32 = 1;

/// What this build's window needs of a backend within [`MAJOR`] (`ADR-0026` §4). Raised whenever a
/// method is added, or a window starts relying on a new field. The backend answers its own
/// `minor` in `session.hello`; a backend without one predates this check. A newer backend serves a
/// superset (removing anything raises the major), so only an older one is refused. Over SSH the
/// backend is meant to be the window's own build, so a lower `minor` means a stale file.
pub const MINOR: u32 = 1;

/// The majors this backend accepts: the current one and the previous one, once there is one.
pub fn supported_majors() -> Vec<u32> {
    (MAJOR.saturating_sub(1).max(1)..=MAJOR).rev().collect()
}

/// The capabilities a backend of this build offers (families bound so far).
pub const CAPABILITIES: [&str; 6] = ["doc.ops", "fs", "fs.watch", "event", "notice", "device"];

/// The highest major both sides speak. A peer that offers the current major always gets it, so an
/// attacker who removes it from nothing can only make the hello fail, never pick an older one; a
/// peer without a common major is refused with both sides' versions.
pub fn choose_major(offered: &[u32], supported: &[u32]) -> Result<u32, RpcError> {
    offered
        .iter()
        .copied()
        .filter(|m| supported.contains(m))
        .max()
        .ok_or_else(|| {
            RpcError::new(
                ErrorCode::UnsupportedVersion,
                format!(
                    "the window speaks protocol versions {offered:?}, this backend {supported:?}"
                ),
            )
            .hint("update IDECraft on the older side so both speak the same major version")
        })
}

/// Whether a backend answering `minor` (absent in builds that predate it) serves this window:
/// refused with both revisions and what to do, before the window relies on anything. It only ever
/// refuses; no answer of the backend's lowers what the window requires.
pub fn check_backend_minor(minor: Option<u64>, build: &str) -> Result<(), RpcError> {
    let theirs = minor.unwrap_or(0);
    if theirs >= u64::from(MINOR) {
        return Ok(());
    }
    let revision = minor.map_or_else(|| "none".to_owned(), |m| m.to_string());
    Err(RpcError::new(
        ErrorCode::UnsupportedVersion,
        format!(
            "the backend on the host ({build}) is older than this window, even if the version \
             number is the same: it serves protocol revision {revision}, this window needs \
             {MINOR}. Build the backends again (scripts/build-backends.sh) or update IDECraft on \
             the host, then open the project again",
        ),
    )
    .hint("the backend's code does not match this window; nothing was opened"))
}

/// 256 random bits that let a window resume its session. Kept only in memory; printed redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Token([u8; 32]);

impl Token {
    /// A new random token.
    pub fn random() -> Self {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).expect("the system random source");
        Self(b)
    }

    /// The token as 64 hex digits (only for the session channel).
    pub fn expose(&self) -> String {
        hex(&self.0)
    }

    /// A token from its hex form.
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() != 64 {
            return None;
        }
        let mut b = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let text = std::str::from_utf8(chunk).ok()?;
            b[i] = u8::from_str_radix(text, 16).ok()?;
        }
        Some(Self(b))
    }

    /// Compares in constant time.
    pub fn matches(&self, other: &Token) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

/// Lower-case hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A random id of 128 bits as hex (session ids are not secret).
pub fn random_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system random source");
    hex(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_choice_never_downgrades() {
        assert_eq!(supported_majors(), [1]);
        assert_eq!(choose_major(&[1], &[1]).unwrap(), 1);
        // The current major wins over an older one the peer also offers.
        assert_eq!(choose_major(&[1, 2], &[2, 1]).unwrap(), 2);
        assert_eq!(choose_major(&[2, 1], &[3, 2]).unwrap(), 2);
        let e = choose_major(&[2], &[1]).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnsupportedVersion);
        assert!(
            e.message.contains("[2]") && e.message.contains("[1]"),
            "{e}"
        );
        assert!(e.hint.unwrap().contains("update"));
        assert!(choose_major(&[], &[1]).is_err());
    }

    #[test]
    fn an_older_backend_is_refused_a_newer_one_accepted() {
        let current = u64::from(MINOR);
        assert!(check_backend_minor(Some(current), "idecraft x").is_ok());
        assert!(check_backend_minor(Some(current + 1), "idecraft x").is_ok());
        for stale in [None, Some(0), Some(current - 1)] {
            let e = check_backend_minor(stale, "idecraft 0.11.0").unwrap_err();
            assert_eq!(e.code, ErrorCode::UnsupportedVersion);
            assert!(e.message.contains("idecraft 0.11.0"), "{e}");
            assert!(e.message.contains("build-backends.sh"), "{e}");
            assert!(e.message.contains(&format!("needs {MINOR}")), "{e}");
        }
    }

    #[test]
    fn tokens_are_random_redacted_and_compared_fully() {
        let a = Token::random();
        let b = Token::random();
        assert!(!a.matches(&b));
        assert!(a.matches(&Token::parse(&a.expose()).unwrap()));
        assert_eq!(a.expose().len(), 64);
        assert_eq!(format!("{a:?}"), "Token(…)");
        assert!(Token::parse("xy").is_none());
        assert!(Token::parse(&"g".repeat(64)).is_none());
        assert_eq!(random_id().len(), 32);
    }
}
