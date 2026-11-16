//! Who may be in a room and what they may do (`SPEC-0323` behaviours 12–20): roles, the mapping of
//! a role to session rights, links as hashes, and joining. Pure logic: no I/O, no clock of its own.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::Right;
use crate::hello::hex;
use crate::rpc::{ErrorCode, RpcError};

/// One hour in milliseconds.
pub const HOUR_MS: u64 = 3_600_000;
/// Default lifetime of an anonymous link.
pub const ANONYMOUS_DEFAULT_MS: u64 = 24 * HOUR_MS;
/// The longest lifetime of an anonymous link.
pub const ANONYMOUS_MAX_MS: u64 = 7 * 24 * HOUR_MS;
/// The most links a room keeps.
pub const MAX_LINKS: usize = 256;

/// A role in a room, in rising order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Reads.
    Viewer,
    /// Reads and comments.
    Commenter,
    /// Also changes files and commits.
    Editor,
    /// Also terminals, runs, debugging, agents.
    Runner,
    /// Everything, including sharing and roles.
    Admin,
}

impl Role {
    /// Every role, lowest first.
    pub const ALL: [Role; 5] = [
        Self::Viewer,
        Self::Commenter,
        Self::Editor,
        Self::Runner,
        Self::Admin,
    ];

    /// Its name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
            Self::Runner => "runner",
            Self::Admin => "admin",
        }
    }

    /// The role called `name`.
    pub fn parse(name: &str) -> Option<Role> {
        Self::ALL.iter().copied().find(|r| r.name() == name)
    }

    /// The session rights of the role.
    pub fn rights(self) -> Vec<Right> {
        match self {
            Self::Viewer | Self::Commenter => vec![Right::View],
            Self::Editor => vec![Right::View, Right::Edit, Right::VcsWrite],
            Self::Runner => vec![
                Right::View,
                Right::Edit,
                Right::VcsWrite,
                Right::Run,
                Right::Debug,
                Right::Agents,
            ],
            Self::Admin => Right::ALL.to_vec(),
        }
    }

    /// The highest role whose rights all lie within `held` (`SPEC-0323` behaviour 13).
    pub fn cap(held: &[Right]) -> Option<Role> {
        Self::ALL
            .iter()
            .rev()
            .copied()
            .find(|r| r.rights().iter().all(|x| held.contains(x)))
    }
}

/// Who a link admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    /// Everyone the server authenticates (`SPEC-0323` behaviour 18).
    Organisation,
    /// Anyone holding the link, read-only and only for a while (behaviour 19).
    Anonymous,
}

impl Audience {
    /// Its name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Self::Organisation => "organisation",
            Self::Anonymous => "anonymous",
        }
    }
}

/// A link as the room keeps it: never the secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// Public id, to list and revoke it.
    pub id: String,
    /// SHA-256 of the secret.
    pub hash: String,
    /// The role it hands out.
    pub role: Role,
    /// Who it admits.
    pub audience: Audience,
    /// Unix milliseconds.
    pub created: u64,
    /// Who made it.
    pub by: String,
    /// Unix milliseconds after which it no longer works.
    pub expires: Option<u64>,
    /// Withdrawn.
    pub revoked: bool,
}

impl Link {
    fn usable(&self, now: u64) -> bool {
        !self.revoked && self.expires.is_none_or(|e| now < e)
    }
}

/// The kind of a participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A person with a login on the server.
    User,
    /// A paired device.
    Device,
    /// A guest from an anonymous link.
    Guest,
    /// An agent acting for someone.
    Agent,
}

/// A participant's identity, derived from the authenticated channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    /// Stable id.
    pub id: String,
    /// Name to show.
    pub name: String,
    /// Kind.
    pub kind: Kind,
}

/// The identity behind a channel's peer name: `device:<fingerprint>:<name>`, `ssh:<user>@<host>`,
/// `local:<user>` (`SPEC-0323` behaviour 18). The device's chosen name is not part of the id.
pub fn person_of(peer: &str) -> Person {
    if let Some(rest) = peer.strip_prefix("device:") {
        let (fingerprint, name) = rest.split_once(':').unwrap_or((rest, rest));
        return Person {
            id: format!("device:{fingerprint}"),
            name: name.to_owned(),
            kind: Kind::Device,
        };
    }
    // A guest of the web endpoint (`SPEC-0325` behaviour 13): never a member, never a user.
    if let Some(rest) = peer.strip_prefix("web-guest:") {
        return Person {
            id: format!("web-guest:{rest}"),
            name: format!("guest {rest}"),
            kind: Kind::Guest,
        };
    }
    if let Some(rest) = peer.strip_prefix("ssh:") {
        let user = rest.split_once('@').map_or(rest, |(u, _)| u);
        return Person {
            id: format!("ssh:{user}"),
            name: user.to_owned(),
            kind: Kind::User,
        };
    }
    let name = peer.split_once(':').map_or(peer, |(_, n)| n);
    Person {
        id: peer.to_owned(),
        name: name.to_owned(),
        kind: Kind::User,
    }
}

/// A member of the room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// Name to show.
    pub name: String,
    /// Role.
    pub role: Role,
    /// Unix milliseconds.
    pub since: u64,
}

/// A place in the room, decided from the identity, a link and the channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    /// Who joined (a guest gets a random one).
    pub person: Person,
    /// The role capped by the channel.
    pub role: Role,
    /// A guest leaves when its link does.
    pub link: Option<String>,
    /// A guest's end.
    pub expires: Option<u64>,
}

fn not_granted(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::NotGranted, message)
}

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Invalid, message)
}

/// SHA-256 of a link's secret, as the room stores it.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Members, removed people and links of a room.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Access {
    /// By person id.
    pub members: BTreeMap<String, Member>,
    /// People removed by an admin: no link brings them back.
    pub removed: BTreeSet<String>,
    /// Every link not yet dropped.
    pub links: Vec<Link>,
}

impl Access {
    /// The person's role, if a member.
    pub fn role(&self, id: &str) -> Option<Role> {
        self.members.get(id).map(|m| m.role)
    }

    /// Makes a link; returns it with its secret (shown once). `role` defaults to viewer,
    /// `audience` to organisation.
    pub fn share(
        &mut self,
        role: Option<Role>,
        audience: Option<Audience>,
        ttl_ms: Option<u64>,
        by: &str,
        now: u64,
        random: [u8; 40],
    ) -> Result<(Link, String), RpcError> {
        if self.links.len() >= MAX_LINKS {
            return Err(invalid(format!(
                "a room keeps at most {MAX_LINKS} links; revoke one first"
            )));
        }
        let role = role.unwrap_or(Role::Viewer);
        let audience = audience.unwrap_or(Audience::Organisation);
        let expires = match audience {
            Audience::Anonymous => {
                if role != Role::Viewer {
                    return Err(invalid(format!(
                        "an anonymous link is read-only; {} is not possible",
                        role.name()
                    )));
                }
                let ttl = ttl_ms.unwrap_or(ANONYMOUS_DEFAULT_MS);
                if ttl == 0 || ttl > ANONYMOUS_MAX_MS {
                    return Err(invalid(format!(
                        "an anonymous link lasts from 1 millisecond to {} days",
                        ANONYMOUS_MAX_MS / (24 * HOUR_MS)
                    )));
                }
                Some(now + ttl)
            }
            Audience::Organisation => {
                if ttl_ms == Some(0) {
                    return Err(invalid("a link cannot expire at once"));
                }
                ttl_ms.map(|t| now + t)
            }
        };
        let (secret, id) = random.split_at(32);
        let token = hex(secret);
        let link = Link {
            id: hex(id),
            hash: token_hash(&token),
            role,
            audience,
            created: now,
            by: by.to_owned(),
            expires,
            revoked: false,
        };
        self.links.push(link.clone());
        Ok((link, token))
    }

    /// Withdraws a link; `false` when there is none with that id.
    pub fn revoke(&mut self, id: &str) -> bool {
        match self.links.iter_mut().find(|l| l.id == id) {
            Some(l) => {
                l.revoked = true;
                true
            }
            None => false,
        }
    }

    /// Whether `token` is an anonymous link that works now (`SPEC-0325` behaviour 13).
    pub fn anonymous_link_live(&self, token: &str, now: u64) -> bool {
        let hash = token_hash(token);
        self.links
            .iter()
            .any(|l| l.hash == hash && l.audience == Audience::Anonymous && l.usable(now))
    }

    /// Whether the link `id` works now.
    pub fn link_live(&self, id: &str, now: u64) -> bool {
        self.links.iter().any(|l| l.id == id && l.usable(now))
    }

    /// Decides whether `person`, on a channel with `rights`, gets a place, with `token` if given;
    /// `guest` is the identity of a guest admitted anonymously.
    pub fn join(
        &mut self,
        person: &Person,
        token: Option<&str>,
        rights: &[Right],
        now: u64,
        guest: Person,
    ) -> Result<Admission, RpcError> {
        // The same refusal for an unknown, used-up or wrong link: it tells a prober nothing.
        let refuse = || {
            not_granted("not admitted: the link is unknown, expired or withdrawn")
                .hint("ask an admin of the room for a new link")
        };
        if self.removed.contains(&person.id) {
            return Err(not_granted("removed from this room").hint("ask an admin to add you"));
        }
        let cap = Role::cap(rights)
            .ok_or_else(|| not_granted("this session has no right to read the project"))?;
        let held = self.role(&person.id);
        let link = match token {
            Some(t) => {
                let hash = token_hash(t);
                let found = self
                    .links
                    .iter()
                    .find(|l| l.hash == hash && l.usable(now))
                    .cloned();
                Some(found.ok_or_else(refuse)?)
            }
            None => None,
        };
        match (link, held) {
            (None, None) => {
                Err(not_granted("not a member of this room").hint("join with a link from an admin"))
            }
            (None, Some(role)) => Ok(Admission {
                person: person.clone(),
                role: role.min(cap),
                link: None,
                expires: None,
            }),
            (Some(l), held) if l.audience == Audience::Anonymous => {
                // A member keeps their membership; otherwise a guest, never stored.
                if let Some(role) = held {
                    return Ok(Admission {
                        person: person.clone(),
                        role: role.min(cap),
                        link: None,
                        expires: None,
                    });
                }
                Ok(Admission {
                    person: guest,
                    role: Role::Viewer,
                    link: Some(l.id),
                    expires: l.expires,
                })
            }
            (Some(l), held) => {
                if !matches!(person.kind, Kind::User | Kind::Device) {
                    return Err(refuse());
                }
                let role = held.map_or(l.role, |r| r.max(l.role));
                self.members
                    .entry(person.id.clone())
                    .and_modify(|m| m.role = role)
                    .or_insert_with(|| Member {
                        name: person.name.clone(),
                        role,
                        since: now,
                    });
                Ok(Admission {
                    person: person.clone(),
                    role: role.min(cap),
                    link: None,
                    expires: None,
                })
            }
        }
    }

    /// An admin changes a role.
    pub fn set_role(&mut self, id: &str, role: Role) -> Result<(), RpcError> {
        let lowers_admin = self.role(id) == Some(Role::Admin) && role != Role::Admin;
        if lowers_admin && self.admins() <= 1 {
            return Err(invalid("the last admin keeps the role"));
        }
        let m = self
            .members
            .get_mut(id)
            .ok_or_else(|| invalid(format!("{id} is not a member")))?;
        m.role = role;
        Ok(())
    }

    /// An admin removes a person.
    pub fn remove(&mut self, id: &str) -> Result<(), RpcError> {
        if self.role(id) == Some(Role::Admin) && self.admins() <= 1 {
            return Err(invalid("the last admin stays"));
        }
        if self.members.remove(id).is_none() {
            return Err(invalid(format!("{id} is not a member")));
        }
        self.removed.insert(id.to_owned());
        Ok(())
    }

    fn admins(&self) -> usize {
        self.members
            .values()
            .filter(|m| m.role == Role::Admin)
            .count()
    }
}

/// The role of an agent acting for `owner` (`SPEC-0323` behaviour 23).
pub fn agent_role(owner: Role, requested: Option<Role>) -> Role {
    let wanted = requested.unwrap_or(Role::Editor).min(Role::Runner);
    wanted.min(owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> Person {
        Person {
            id: format!("ssh:{id}"),
            name: id.into(),
            kind: Kind::User,
        }
    }

    fn guest() -> Person {
        Person {
            id: "guest:x".into(),
            name: "Guest x".into(),
            kind: Kind::Guest,
        }
    }

    fn rand(n: u8) -> [u8; 40] {
        [n; 40]
    }

    fn with_admin() -> Access {
        let mut a = Access::default();
        a.members.insert(
            "ssh:owner".into(),
            Member {
                name: "owner".into(),
                role: Role::Admin,
                since: 0,
            },
        );
        a
    }

    #[test]
    fn roles_map_to_rights_and_are_capped_by_the_channel() {
        assert_eq!(Role::Viewer.rights(), [Right::View]);
        assert!(!Role::Editor.rights().contains(&Right::Run));
        assert!(Role::Runner.rights().contains(&Right::Run));
        assert!(!Role::Runner.rights().contains(&Right::Push));
        assert_eq!(Role::Admin.rights(), Right::ALL);
        assert_eq!(Role::cap(&[Right::View]), Some(Role::Commenter));
        assert_eq!(Role::cap(&[]), None);
        assert_eq!(Role::cap(&Right::ALL), Some(Role::Admin));
        assert_eq!(
            Role::cap(&[Right::View, Right::Edit, Right::VcsWrite, Right::Run]),
            Some(Role::Editor)
        );
        assert_eq!(Role::parse("runner"), Some(Role::Runner));
        assert_eq!(Role::parse("root"), None);
    }

    #[test]
    fn identity_comes_from_the_channel_without_self_chosen_parts() {
        let d = person_of("device:ab12:Alices phone");
        assert_eq!(
            (d.id.as_str(), d.name.as_str(), d.kind),
            ("device:ab12", "Alices phone", Kind::Device)
        );
        let s = person_of("ssh:alice@10.0.0.1");
        assert_eq!((s.id.as_str(), s.kind), ("ssh:alice", Kind::User));
        assert_eq!(person_of("local:alice").name, "alice");
    }

    #[test]
    fn default_link_is_organisation_and_viewer_and_stores_only_a_hash() {
        let mut a = with_admin();
        let (l, token) = a.share(None, None, None, "ssh:owner", 10, rand(7)).unwrap();
        assert_eq!((l.role, l.audience), (Role::Viewer, Audience::Organisation));
        assert_eq!(token.len(), 64);
        let stored = serde_json::to_string(&a).unwrap();
        assert!(!stored.contains(&token), "the secret must not be stored");
        assert!(stored.contains(&token_hash(&token)));
        let got = a
            .join(&user("bob"), Some(&token), &Right::ALL, 20, guest())
            .unwrap();
        assert_eq!(got.role, Role::Viewer);
        assert_eq!(a.role("ssh:bob"), Some(Role::Viewer));
    }

    #[test]
    fn a_link_never_lowers_a_role_and_the_channel_caps_it() {
        let mut a = with_admin();
        let (_, viewer) = a.share(None, None, None, "o", 0, rand(1)).unwrap();
        let (_, editor) = a
            .share(Some(Role::Editor), None, None, "o", 0, rand(2))
            .unwrap();
        a.join(&user("bob"), Some(&editor), &Right::ALL, 1, guest())
            .unwrap();
        a.join(&user("bob"), Some(&viewer), &Right::ALL, 2, guest())
            .unwrap();
        assert_eq!(a.role("ssh:bob"), Some(Role::Editor));
        let weak = a
            .join(&user("carl"), Some(&editor), &[Right::View], 3, guest())
            .unwrap();
        assert_eq!(weak.role, Role::Commenter);
    }

    #[test]
    fn anonymous_links_are_read_only_expiring_and_guests_are_never_members() {
        let mut a = with_admin();
        let e = a
            .share(
                Some(Role::Editor),
                Some(Audience::Anonymous),
                None,
                "o",
                0,
                rand(1),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        let long = ANONYMOUS_MAX_MS + 1;
        assert!(
            a.share(None, Some(Audience::Anonymous), Some(long), "o", 0, rand(1))
                .is_err()
        );
        assert!(
            a.share(None, Some(Audience::Anonymous), Some(0), "o", 0, rand(1))
                .is_err()
        );
        let (l, t) = a
            .share(None, Some(Audience::Anonymous), None, "o", 100, rand(3))
            .unwrap();
        assert_eq!(l.expires, Some(100 + ANONYMOUS_DEFAULT_MS));
        let got = a
            .join(&user("eve"), Some(&t), &Right::ALL, 200, guest())
            .unwrap();
        assert_eq!(got.role, Role::Viewer);
        assert_eq!(got.person.kind, Kind::Guest);
        assert_eq!(a.role("ssh:eve"), None);
        // Expired and revoked links refuse with the same message as unknown ones.
        let late = a
            .join(
                &user("eve"),
                Some(&t),
                &Right::ALL,
                100 + ANONYMOUS_DEFAULT_MS,
                guest(),
            )
            .unwrap_err();
        let unknown = a
            .join(&user("eve"), Some("nope"), &Right::ALL, 200, guest())
            .unwrap_err();
        assert_eq!(late.code, ErrorCode::NotGranted);
        assert_eq!(late.message, unknown.message);
        assert!(a.revoke(&l.id));
        assert!(!a.link_live(&l.id, 200));
        assert!(
            a.join(&user("eve"), Some(&t), &Right::ALL, 200, guest())
                .is_err()
        );
        assert!(!a.revoke("missing"));
    }

    #[test]
    fn guests_and_agents_cannot_be_organisation_members() {
        let mut a = with_admin();
        let (_, t) = a.share(None, None, None, "o", 0, rand(1)).unwrap();
        let g = Person {
            id: "guest:a".into(),
            name: "a".into(),
            kind: Kind::Guest,
        };
        assert!(a.join(&g, Some(&t), &Right::ALL, 1, guest()).is_err());
    }

    #[test]
    fn removal_is_final_for_links_and_the_last_admin_stays() {
        let mut a = with_admin();
        let (_, t) = a
            .share(Some(Role::Editor), None, None, "o", 0, rand(1))
            .unwrap();
        a.join(&user("bob"), Some(&t), &Right::ALL, 1, guest())
            .unwrap();
        a.remove("ssh:bob").unwrap();
        let e = a
            .join(&user("bob"), Some(&t), &Right::ALL, 2, guest())
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::NotGranted);
        assert_eq!(a.remove("ssh:owner").unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(
            a.set_role("ssh:owner", Role::Editor).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(a.remove("ssh:nobody").is_err());
        // Without a link, a stranger is refused.
        assert!(a.join(&user("x"), None, &Right::ALL, 3, guest()).is_err());
    }

    #[test]
    fn agents_stay_below_their_owner_and_below_admin() {
        assert_eq!(agent_role(Role::Admin, None), Role::Editor);
        assert_eq!(agent_role(Role::Admin, Some(Role::Admin)), Role::Runner);
        assert_eq!(agent_role(Role::Admin, Some(Role::Runner)), Role::Runner);
        assert_eq!(agent_role(Role::Editor, Some(Role::Runner)), Role::Editor);
        assert_eq!(agent_role(Role::Viewer, None), Role::Viewer);
    }
}
