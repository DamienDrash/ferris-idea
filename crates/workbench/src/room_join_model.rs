//! Joining a project room by link from the window (`SPEC-0323` v7 behaviours 46–51): what the
//! Join dialog shows of a pasted link, the host key check, when Join is offered, and what each
//! failure says. Pure: the window feeds it the pasted text, the time, the host key check and the
//! outcome; nothing here touches a window, the clipboard, `known_hosts` or the network.

use ferris_idea_remote::{HostKeyCheck, LinkError, RemoteError, RoomLink};
use ferris_idea_session::{ErrorCode, RpcError};

use crate::room_share_model::remaining;

/// What the dialog says about joining needing an account (behaviour 48).
pub const ACCOUNT_NOTE: &str = "Joining needs an account on the host: you log in with your own SSH keys, as for Remote Development…";

/// The advice about where links come from (behaviour 49).
pub const TRUST_NOTE: &str = "The fingerprint protects against a wrong host, not against a forged link: get links over a channel you trust.";

/// Why joining failed (behaviour 51); never holds the link's secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinFailure {
    /// The link's end had passed (Unix seconds), seen before connecting.
    Expired(u64),
    /// The room refused the link: revoked, ended on the host or unknown.
    NotAdmitted,
    /// The person was removed from the room.
    Removed,
    /// The host key differs from the one trusted for the host.
    WrongHostKey,
    /// The host key is not known yet.
    UnknownHostKey,
    /// No post-quantum key exchange on the way: the reason of `SPEC-0121`.
    NoPostQuantum(String),
    /// The host cannot be reached: the reason.
    Unreachable(String),
    /// The SSH login was refused.
    LoginRefused,
    /// The project has no room.
    NoRoom,
    /// The room differs from the link's.
    OtherRoom,
    /// Anything else, as said.
    Other(String),
}

impl JoinFailure {
    /// From a failure to connect.
    pub fn from_remote(e: &RemoteError) -> Self {
        match e {
            RemoteError::HostKeyChanged(_) => Self::WrongHostKey,
            RemoteError::HostKeyUnknown(_) => Self::UnknownHostKey,
            RemoteError::NoPqKex(_)
            | RemoteError::ClientWithoutPq(_)
            | RemoteError::HopNoPqKex(..) => Self::NoPostQuantum(e.to_string()),
            RemoteError::Unreachable(_, why) => Self::Unreachable(why.clone()),
            RemoteError::TimedOut(_, s) => Self::Unreachable(format!("no answer within {s} s")),
            RemoteError::AuthRequired(_) => Self::LoginRefused,
            other => Self::Other(other.to_string()),
        }
    }

    /// From the room's answer to `room.join`.
    pub fn from_join(e: &RpcError) -> Self {
        match e.code {
            ErrorCode::NotGranted if e.message.contains("removed") => Self::Removed,
            ErrorCode::NotGranted => Self::NotAdmitted,
            ErrorCode::NotFound => Self::NoRoom,
            ErrorCode::Unavailable => Self::Unreachable(e.message.clone()),
            _ => Self::Other(e.message.clone()),
        }
    }

    /// The sentence in the dialog, with what to do (behaviour 51).
    pub fn text(&self, host: &str, port: u16, now: u64) -> String {
        match self {
            Self::Expired(end) => format!(
                "This link ended {}. Ask an admin of the room for a new one.",
                ago(*end, now)
            ),
            Self::NotAdmitted => "The host did not admit this link: it was withdrawn, has ended or is not from this room. Ask an admin of the room for a new link.".into(),
            Self::Removed => "You were removed from this room. Ask an admin to add you.".into(),
            Self::WrongHostKey => WRONG_KEY.into(),
            Self::UnknownHostKey => format!(
                "The host key of {host} is not in your known_hosts yet; Ferris IDEA does not add host keys itself. Follow the steps below, then Check Again."
            ),
            Self::NoPostQuantum(why) => format!(
                "{why}. Ferris IDEA joins only with post-quantum protection; ask the host's owner to update OpenSSH (9.0 or later, ML-KEM from 9.9), or update your own."
            ),
            Self::Unreachable(why) => format!(
                "Cannot reach {host}:{port}: {why}. Check the address, your network or VPN, and that the host is running."
            ),
            Self::LoginRefused => format!(
                "Your SSH login to {host} was refused. Joining needs an account on the host with your SSH key."
            ),
            Self::NoRoom => "The project on the host has no room (any more).".into(),
            Self::OtherRoom => {
                "The project's room is not the one the link names; nothing was joined.".into()
            }
            Self::Other(why) => why.clone(),
        }
    }
}

/// The refusal of a different host key (behaviour 49).
pub const WRONG_KEY: &str = "The host key in the link does not match the one you trust for this host. This can mean an attack or a forged link; do not join. Check with the room's admin over another channel.";

/// How long ago `end` (Unix seconds) was, as the dialog says it.
pub fn ago(end: u64, now: u64) -> String {
    let past = now.saturating_sub(end);
    if past < 60 {
        "under a minute ago".into()
    } else if past < 3600 {
        format!("{} min ago", past / 60)
    } else if past < 2 * 86_400 {
        format!("{} h ago", past / 3600)
    } else {
        format!("{} days ago", past / 86_400)
    }
}

/// The capitalised role (`Editor`).
fn role_label(role: &str) -> String {
    let mut c = role.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// What the Join dialog shows.
#[derive(Debug, Clone, Default)]
pub struct JoinModel {
    link: Option<RoomLink>,
    check: Option<HostKeyCheck>,
    /// Checking the host key or joining is under way.
    pub busy: bool,
    /// Why the link was refused or joining failed.
    pub error: Option<String>,
    failure: Option<JoinFailure>,
}

impl JoinModel {
    /// Reads a pasted `text` at `now` (Unix seconds); `true` when it is a usable link. The check
    /// of an earlier link is forgotten.
    pub fn paste(&mut self, text: &str, now: u64) -> bool {
        *self = Self::default();
        match RoomLink::parse(text, now) {
            Ok(l) => {
                self.link = Some(l);
                true
            }
            Err(LinkError::Expired(end)) => {
                let f = JoinFailure::Expired(end);
                self.error = Some(f.text("", 0, now));
                self.failure = Some(f);
                false
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    /// Clear: forgets the link.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The link read, if any.
    pub fn link(&self) -> Option<&RoomLink> {
        self.link.as_ref()
    }

    /// The last failure, if any.
    pub fn failure(&self) -> Option<&JoinFailure> {
        self.failure.as_ref()
    }

    /// The host key check, once done.
    pub fn check(&self) -> Option<&HostKeyCheck> {
        self.check.as_ref()
    }

    /// The parts of the link shown before connecting (behaviour 48), `(label, value)`; `now` in
    /// Unix seconds. The link itself only masked.
    pub fn rows(&self, now: u64) -> Vec<(&'static str, String)> {
        let Some(l) = &self.link else {
            return Vec::new();
        };
        vec![
            ("Host", format!("{}:{}", l.host, l.port)),
            ("Project", l.folder.clone()),
            ("Room", l.room.clone()),
            ("Host key", l.fingerprint.clone()),
            ("Role", role_label(&l.role)),
            (
                "Link",
                remaining(l.expires.map(|e| e * 1000), now.saturating_mul(1000)),
            ),
            ("Pasted", l.masked()),
        ]
    }

    /// The result of the host key check (behaviour 49).
    pub fn set_check(&mut self, check: HostKeyCheck) {
        self.failure = match &check {
            HostKeyCheck::Matches { .. } => None,
            HostKeyCheck::Different { .. } => Some(JoinFailure::WrongHostKey),
            HostKeyCheck::Unknown => Some(JoinFailure::UnknownHostKey),
        };
        self.error = self.failure.as_ref().map(|f| self.say(f));
        self.check = Some(check);
    }

    /// The line about the host key and whether it is good.
    pub fn check_line(&self) -> Option<(String, bool)> {
        match self.check.as_ref()? {
            HostKeyCheck::Matches { .. } => {
                Some(("The host key matches your known_hosts".into(), true))
            }
            HostKeyCheck::Different { .. } => Some((WRONG_KEY.into(), false)),
            HostKeyCheck::Unknown => {
                Some(("The host key is not in your known_hosts yet".into(), false))
            }
        }
    }

    /// The steps that add an unknown key after checking it (behaviour 49).
    pub fn steps(&self) -> Vec<String> {
        match (&self.link, &self.check) {
            (Some(l), Some(HostKeyCheck::Unknown)) => {
                ferris_idea_remote::known_hosts::keyscan_steps(&l.host, l.port, &l.fingerprint)
            }
            _ => Vec::new(),
        }
    }

    /// Whether Join is offered: a link whose host key matched, nothing under way.
    pub fn can_join(&self) -> bool {
        self.link.is_some()
            && !self.busy
            && matches!(self.check, Some(HostKeyCheck::Matches { .. }))
    }

    /// Joining failed with `failure`; `now` in Unix seconds.
    pub fn fail(&mut self, failure: JoinFailure, now: u64) {
        let (host, port) = self
            .link
            .as_ref()
            .map_or((String::new(), 0), |l| (l.host.clone(), l.port));
        self.error = Some(failure.text(&host, port, now));
        if matches!(failure, JoinFailure::WrongHostKey) {
            self.check = Some(HostKeyCheck::Different {
                trusted: Vec::new(),
            });
        }
        self.failure = Some(failure);
        self.busy = false;
    }

    fn say(&self, f: &JoinFailure) -> String {
        let (host, port) = self
            .link
            .as_ref()
            .map_or((String::new(), 0), |l| (l.host.clone(), l.port));
        f.text(&host, port, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const FP: &str = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
    const NOW: u64 = 1_000_000;

    fn link(expires: u64) -> String {
        RoomLink::compose(
            &format!("ferris-idea://room/ab12#{SECRET}"),
            "dev.example.com",
            2222,
            "/srv/app",
            FP,
            "editor",
            Some(expires),
        )
        .unwrap()
        .text()
    }

    #[test]
    fn pasting_shows_the_parts_with_the_secret_masked() {
        let mut m = JoinModel::default();
        assert!(m.paste(&link(NOW + 6 * 86_400), NOW));
        let rows = m.rows(NOW);
        let get = |k: &str| rows.iter().find(|r| r.0 == k).unwrap().1.clone();
        assert_eq!(get("Host"), "dev.example.com:2222");
        assert_eq!(get("Project"), "/srv/app");
        assert_eq!(get("Room"), "ab12");
        assert_eq!(get("Host key"), FP);
        assert_eq!(get("Role"), "Editor");
        assert_eq!(get("Link"), "ends in 6 days");
        assert!(get("Pasted").ends_with("#••••cdef"));
        for (_, v) in &rows {
            assert!(!v.contains(SECRET), "{v}");
        }
        assert!(!format!("{m:?}").contains(SECRET));
        assert!(!m.can_join(), "Join waits for the host key check");
    }

    #[test]
    fn join_is_offered_only_after_a_match() {
        let mut m = JoinModel::default();
        m.paste(&link(NOW + 60), NOW);
        m.set_check(HostKeyCheck::Unknown);
        assert!(!m.can_join());
        assert_eq!(m.failure(), Some(&JoinFailure::UnknownHostKey));
        let steps = m.steps();
        assert!(steps[0].contains("ssh -p 2222 dev.example.com"));
        assert!(steps[0].contains(FP));
        m.set_check(HostKeyCheck::Different {
            trusted: vec!["SHA256:x".into()],
        });
        assert!(!m.can_join());
        assert_eq!(m.check_line().unwrap(), (WRONG_KEY.to_owned(), false));
        assert!(m.steps().is_empty());
        m.set_check(HostKeyCheck::Matches {
            key_type: "ssh-ed25519".into(),
        });
        assert!(m.can_join());
        assert!(m.error.is_none());
        m.busy = true;
        assert!(!m.can_join());
        m.clear();
        assert!(m.link().is_none() && !m.can_join() && m.rows(NOW).is_empty());
    }

    #[test]
    fn malformed_and_expired_links_are_refused() {
        let mut m = JoinModel::default();
        assert!(!m.paste("hello", NOW));
        assert!(m.error.as_deref().unwrap().contains("ferris-idea://room/"));
        assert!(!m.paste(&link(NOW - 7200), NOW));
        assert_eq!(m.failure(), Some(&JoinFailure::Expired(NOW - 7200)));
        assert_eq!(
            m.error.as_deref(),
            Some("This link ended 2 h ago. Ask an admin of the room for a new one.")
        );
        assert!(m.link().is_none());
    }

    #[test]
    fn every_failure_says_what_to_do() {
        let host = "dev.example.com".to_owned();
        let not = |m: &str| RpcError::new(ErrorCode::NotGranted, m);
        assert_eq!(
            JoinFailure::from_join(&not(
                "not admitted: the link is unknown, expired or withdrawn"
            )),
            JoinFailure::NotAdmitted
        );
        assert_eq!(
            JoinFailure::from_join(&not("removed from this room")),
            JoinFailure::Removed
        );
        assert_eq!(
            JoinFailure::from_join(&RpcError::new(
                ErrorCode::NotFound,
                "this project has no room"
            )),
            JoinFailure::NoRoom
        );
        assert_eq!(
            JoinFailure::from_remote(&RemoteError::HostKeyChanged(host.clone())),
            JoinFailure::WrongHostKey
        );
        assert_eq!(
            JoinFailure::from_remote(&RemoteError::HostKeyUnknown(host.clone())),
            JoinFailure::UnknownHostKey
        );
        assert!(matches!(
            JoinFailure::from_remote(&RemoteError::NoPqKex(host.clone())),
            JoinFailure::NoPostQuantum(_)
        ));
        assert!(matches!(
            JoinFailure::from_remote(&RemoteError::HopNoPqKex("hop".into(), host.clone())),
            JoinFailure::NoPostQuantum(_)
        ));
        assert_eq!(
            JoinFailure::from_remote(&RemoteError::Unreachable(host.clone(), "refused".into())),
            JoinFailure::Unreachable("refused".into())
        );
        assert_eq!(
            JoinFailure::from_remote(&RemoteError::AuthRequired(host.clone())),
            JoinFailure::LoginRefused
        );
        let t = |f: JoinFailure| f.text(&host, 2222, NOW);
        assert!(t(JoinFailure::NotAdmitted).contains("Ask an admin of the room for a new link"));
        assert!(t(JoinFailure::Removed).contains("removed"));
        assert!(t(JoinFailure::WrongHostKey).contains("attack"));
        assert!(t(JoinFailure::NoPostQuantum("x".into())).contains("post-quantum"));
        assert!(
            t(JoinFailure::Unreachable("refused".into()))
                .starts_with("Cannot reach dev.example.com:2222: refused.")
        );
        assert!(t(JoinFailure::LoginRefused).contains("account on the host"));
        assert!(t(JoinFailure::NoRoom).contains("no room"));
        assert!(t(JoinFailure::OtherRoom).contains("nothing was joined"));
        let mut m = JoinModel::default();
        m.paste(&link(NOW + 60), NOW);
        m.set_check(HostKeyCheck::Matches {
            key_type: "ssh-ed25519".into(),
        });
        m.busy = true;
        m.fail(JoinFailure::WrongHostKey, NOW);
        assert!(!m.busy && !m.can_join(), "a changed key takes Join away");
        assert_eq!(m.error.as_deref(), Some(WRONG_KEY));
    }
}
