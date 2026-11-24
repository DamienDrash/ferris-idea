//! Sharing a project room from the window (`SPEC-0323` v6 behaviours 37–43): the Share dialog's
//! people, links in force, the choices for a new link and the link shown once, and the room
//! widget of the status bar. Pure: the window feeds it the room's answers and the time and asks
//! it what to show and which parameters to send; nothing here touches a window, the clipboard or
//! the network.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::room_model::{RoomView, shown_name};

/// Milliseconds of an hour.
const HOUR_MS: u64 = 3_600_000;
/// Milliseconds of a day.
const DAY_MS: u64 = 24 * HOUR_MS;
/// Names the widget's tooltip lists before "and N more" (behaviour 43).
pub const WIDGET_NAMES: usize = 10;

/// The role a link from the dialog hands out (behaviour 39).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShareRole {
    /// Read only; the default.
    #[default]
    Viewer,
    /// Change files.
    Editor,
}

impl ShareRole {
    /// Both, in the order the dialog shows them.
    pub const ALL: [Self; 2] = [Self::Viewer, Self::Editor];

    /// The role's name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
        }
    }

    /// The label in the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Self::Viewer => "Viewer",
            Self::Editor => "Editor",
        }
    }
}

/// How long a link from the dialog works (behaviour 39). There is no choice without an end
/// (IMP-0507).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifetime {
    /// One hour.
    Hour,
    /// One day; the default.
    #[default]
    Day,
    /// Seven days.
    Week,
    /// Thirty days.
    Month,
}

impl Lifetime {
    /// Every choice, in the order the dialog shows them.
    pub const ALL: [Self; 4] = [Self::Hour, Self::Day, Self::Week, Self::Month];

    /// The lifetime in seconds, as `room.share` takes it.
    pub fn seconds(self) -> u64 {
        match self {
            Self::Hour => 3600,
            Self::Day => 86_400,
            Self::Week => 7 * 86_400,
            Self::Month => 30 * 86_400,
        }
    }

    /// The label in the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hour => "1 hour",
            Self::Day => "1 day",
            Self::Week => "7 days",
            Self::Month => "30 days",
        }
    }
}

/// One person in the dialog's list (behaviour 41).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonRow {
    /// The person's id, absent for what a guest sees.
    pub id: Option<String>,
    /// The session's client number, absent for a member who is not there.
    pub client: Option<String>,
    /// The name as shown (cut, control characters dropped).
    pub name: String,
    /// The role.
    pub role: String,
    /// The window's own person.
    pub you: bool,
    /// Away for now.
    pub away: bool,
    /// In the room now; `false` for a member who is not there.
    pub here: bool,
    /// An admin may remove this person.
    pub removable: bool,
}

impl PersonRow {
    /// What stands after the name: role, "you", "away", "not here".
    pub fn detail(&self) -> String {
        let mut parts = vec![self.role.clone()];
        if self.you {
            parts.push("you".into());
        }
        if self.away {
            parts.push("away".into());
        }
        if !self.here {
            parts.push("not here".into());
        }
        parts.join(" · ")
    }
}

/// A link in force (behaviour 40); never its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    /// Public id, to revoke it.
    pub id: String,
    /// The role it hands out.
    pub role: String,
    /// Unix milliseconds it was made.
    pub created: u64,
    /// Unix milliseconds it ends, if ever.
    pub expires: Option<u64>,
}

/// The link just made, shown once (behaviour 39).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshLink {
    /// The whole link with its secret.
    pub link: String,
    /// The role it hands out.
    pub role: String,
    /// Unix milliseconds it ends.
    pub expires: Option<u64>,
}

/// What the Share dialog shows.
#[derive(Debug, Clone, Default)]
pub struct ShareModel {
    /// The window's own client number.
    own: String,
    /// Whether `room.status` answered at all yet.
    known: bool,
    /// `(id, name)` of the room; `None` while there is none.
    room: Option<(String, String)>,
    /// The user's own role in the room.
    role: Option<String>,
    people: Vec<PersonRow>,
    links: Vec<LinkRow>,
    /// The role chosen for the next link.
    pub choice_role: ShareRole,
    /// The lifetime chosen for the next link.
    pub choice_life: Lifetime,
    fresh: Option<FreshLink>,
    /// The person whose removal waits for "Remove" in the row.
    confirm: Option<String>,
    /// The backend's reason for the last failed call.
    pub error: Option<String>,
    /// What the last action did ("Copied the link").
    pub note: Option<String>,
}

/// Whether `role` is the admin role.
fn admin(role: Option<&str>) -> bool {
    role == Some("admin")
}

/// `c2` before `c10`.
fn client_key(client: Option<&str>) -> (u64, String) {
    let c = client.unwrap_or("");
    (
        c.strip_prefix('c')
            .and_then(|n| n.parse().ok())
            .unwrap_or(u64::MAX),
        c.to_owned(),
    )
}

impl ShareModel {
    /// A dialog of the window whose session is `own`; nothing known yet.
    pub fn new(own: &str) -> Self {
        Self {
            own: own.to_owned(),
            ..Self::default()
        }
    }

    /// Whether the room's state arrived at least once.
    pub fn known(&self) -> bool {
        self.known
    }

    /// The room's name, while there is a room.
    pub fn room_name(&self) -> Option<&str> {
        self.room.as_ref().map(|(_, n)| n.as_str())
    }

    /// The user's own role.
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }

    /// Whether the user is an admin of the room: links and removal (behaviours 39–41).
    pub fn is_admin(&self) -> bool {
        admin(self.role.as_deref())
    }

    /// The people in the order the dialog lists them.
    pub fn people(&self) -> &[PersonRow] {
        &self.people
    }

    /// The links in force, newest first.
    pub fn links(&self) -> &[LinkRow] {
        &self.links
    }

    /// The link shown once, if one was just made.
    pub fn fresh(&self) -> Option<&FreshLink> {
        self.fresh.as_ref()
    }

    /// The person whose removal waits for a second click.
    pub fn confirming(&self) -> Option<&str> {
        self.confirm.as_deref()
    }

    /// The window's session changed (after a reconnection).
    pub fn set_own(&mut self, own: &str) {
        own.clone_into(&mut self.own);
    }

    /// The project has no room (behaviour 38).
    pub fn set_no_room(&mut self) {
        self.known = true;
        self.room = None;
        self.role = None;
        self.people.clear();
        self.links.clear();
        self.fresh = None;
        self.confirm = None;
    }

    /// The answer of `room.status` (or `room.join`, `room.create`); a malformed one changes
    /// nothing and returns `false`.
    pub fn set_status(&mut self, status: &Value) -> bool {
        let (Some(id), Some(list)) = (status["id"].as_str(), status["participants"].as_array())
        else {
            return false;
        };
        self.known = true;
        let name = status["name"].as_str().unwrap_or(id).to_owned();
        self.room = Some((id.to_owned(), name));
        let mine = list.iter().find(|p| p["client"] == self.own.as_str());
        self.role = mine
            .and_then(|p| p["role"].as_str())
            .or(status["role"].as_str())
            .map(str::to_owned);
        let own_id = mine.and_then(|p| p["id"].as_str()).map(str::to_owned);
        let is_admin = self.is_admin();
        let members: BTreeSet<String> = status["members"]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        let removable = |id: Option<&str>| {
            is_admin && id.is_some_and(|i| members.contains(i) && Some(i) != own_id.as_deref())
        };
        let mut people: Vec<PersonRow> = list
            .iter()
            .filter_map(|p| {
                let client = p["client"].as_str()?;
                let id = p["id"].as_str();
                Some(PersonRow {
                    id: id.map(str::to_owned),
                    client: Some(client.to_owned()),
                    name: shown_name(p["name"].as_str().unwrap_or(client)),
                    role: p["role"].as_str().unwrap_or("viewer").to_owned(),
                    you: client == self.own,
                    away: p["away"].as_bool().unwrap_or(false),
                    here: true,
                    removable: removable(id),
                })
            })
            .collect();
        let present: BTreeSet<String> = people.iter().filter_map(|p| p.id.clone()).collect();
        if is_admin && let Some(m) = status["members"].as_object() {
            for (id, member) in m {
                if present.contains(id) {
                    continue;
                }
                people.push(PersonRow {
                    id: Some(id.clone()),
                    client: None,
                    name: shown_name(member["name"].as_str().unwrap_or(id)),
                    role: member["role"].as_str().unwrap_or("viewer").to_owned(),
                    you: Some(id.as_str()) == own_id.as_deref(),
                    away: false,
                    here: false,
                    removable: removable(Some(id)),
                });
            }
        }
        people.sort_by(|a, b| {
            b.you
                .cmp(&a.you)
                .then_with(|| b.here.cmp(&a.here))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| client_key(a.client.as_deref()).cmp(&client_key(b.client.as_deref())))
        });
        self.people = people;
        if !is_admin {
            self.links.clear();
            self.fresh = None;
        }
        if self.confirm.as_ref().is_some_and(|c| {
            !self
                .people
                .iter()
                .any(|p| p.removable && p.id.as_ref() == Some(c))
        }) {
            self.confirm = None;
        }
        true
    }

    /// The answer of `room.links` at `now` (unix ms): only links in force, newest first.
    pub fn set_links(&mut self, answer: &Value, now: u64) {
        let Some(list) = answer["links"].as_array() else {
            return;
        };
        let mut links: Vec<LinkRow> = list
            .iter()
            .filter(|l| l["revoked"] != true && l["live"] != false)
            .filter_map(|l| {
                let expires = l["expires"].as_u64();
                if expires.is_some_and(|e| e <= now) {
                    return None;
                }
                Some(LinkRow {
                    id: l["id"].as_str()?.to_owned(),
                    role: l["role"].as_str().unwrap_or("viewer").to_owned(),
                    created: l["created"].as_u64().unwrap_or(0),
                    expires,
                })
            })
            .collect();
        links.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
        self.links = links;
    }

    /// The parameters of `room.share` for the choices (behaviour 39): always with an end.
    pub fn share_params(&self) -> Value {
        json!({
            "role": self.choice_role.name(),
            "audience": "organisation",
            "ttl_seconds": self.choice_life.seconds(),
        })
    }

    /// The answer of `room.share`: the link to show once; `false` when it holds none.
    pub fn set_fresh(&mut self, answer: &Value) -> bool {
        let Some(link) = answer["link"].as_str() else {
            return false;
        };
        self.fresh = Some(FreshLink {
            link: link.to_owned(),
            role: answer["role"]
                .as_str()
                .unwrap_or(self.choice_role.name())
                .to_owned(),
            expires: answer["expires"].as_u64(),
        });
        self.note = None;
        true
    }

    /// Makes the link shown lead to its host (`SPEC-0323` v7 behaviour 44): host, port, project
    /// folder and the host key's fingerprint join the room, role and end. `false` when there is
    /// no short link to extend or a part is unusable; the link then stays as the backend made it.
    pub fn lead_to(&mut self, host: &str, port: u16, folder: &str, fingerprint: &str) -> bool {
        let Some(f) = self.fresh.as_mut() else {
            return false;
        };
        match ferris_idea_remote::RoomLink::compose(
            &f.link,
            host,
            port,
            folder,
            fingerprint,
            &f.role,
            f.expires.map(|ms| ms / 1000),
        ) {
            Some(full) => {
                f.link = full.text();
                true
            }
            None => false,
        }
    }

    /// The first click on Remove (behaviour 41): asks in the row; `false` for someone who
    /// cannot be removed.
    pub fn ask_remove(&mut self, id: &str) -> bool {
        if self
            .people
            .iter()
            .any(|p| p.removable && p.id.as_deref() == Some(id))
        {
            self.confirm = Some(id.to_owned());
            true
        } else {
            false
        }
    }

    /// Keep: the question in the row goes away.
    pub fn keep(&mut self) {
        self.confirm = None;
    }

    /// The second click: the person to remove, when the row asked for exactly this person.
    pub fn confirm_remove(&mut self, id: &str) -> Option<String> {
        (self.confirm.as_deref() == Some(id)).then(|| {
            self.confirm = None;
            id.to_owned()
        })
    }

    /// The headline text under the title.
    pub fn summary(&self) -> String {
        match (&self.room, self.known) {
            (_, false) => "Asking the host about the room…".into(),
            (None, true) => {
                "No room yet. A room lets others work on the same files with you at the same time."
                    .into()
            }
            (Some((_, name)), true) => match self.role.as_deref() {
                Some(role) if admin(Some(role)) => {
                    format!("Room {name} · you are admin")
                }
                Some(role) => format!(
                    "Room {name} · you are {role}. Only an admin of the room makes links and removes people."
                ),
                None => format!("Room {name}"),
            },
        }
    }
}

/// A person id as part of a control's name: letters and digits kept, the rest `-`.
pub fn key_of(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// How long a link still works, as the dialog says it (behaviour 40).
pub fn remaining(expires: Option<u64>, now: u64) -> String {
    let Some(end) = expires else {
        return "never ends".into();
    };
    let left = end.saturating_sub(now);
    if left < 60_000 {
        "ends in under a minute".into()
    } else if left < HOUR_MS {
        format!("ends in {} min", left / 60_000)
    } else if left < 2 * DAY_MS {
        let h = (left + HOUR_MS / 2) / HOUR_MS;
        format!("ends in {h} h")
    } else {
        let d = (left + DAY_MS / 2) / DAY_MS;
        format!("ends in {d} days")
    }
}

/// The connection as the room widget names it (behaviour 43).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// Connected to the host.
    Connected,
    /// Trying again.
    Reconnecting,
    /// Not connected.
    Offline,
}

impl Connection {
    /// Its name in `room_presence`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Offline => "offline",
        }
    }

    fn line(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Reconnecting => {
                "Reconnecting — your edits wait and are sent when the connection is back"
            }
            Self::Offline => "Offline — your edits wait until you reconnect",
        }
    }
}

/// The room widget of the status bar (behaviour 43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widget {
    /// "Room · N".
    pub text: String,
    /// The tooltip's title: the room's name.
    pub title: String,
    /// The tooltip's lines.
    pub lines: Vec<String>,
    /// Participants not shown as gone, oneself included.
    pub participants: usize,
    /// The connection.
    pub connection: Connection,
}

impl Widget {
    /// The widget for `view` over `connection`.
    pub fn of(view: &RoomView, connection: Connection) -> Self {
        let mut names: Vec<String> = view.others().map(|p| p.shown_name()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        let count = names.len() + 1;
        let mut lines = vec![format!(
            "You are {}",
            view.own_role().unwrap_or("in the room")
        )];
        if names.is_empty() {
            lines.push("Nobody else is here".into());
        } else {
            let more = names.len().saturating_sub(WIDGET_NAMES);
            let mut shown: Vec<String> = names.into_iter().take(WIDGET_NAMES).collect();
            if more > 0 {
                shown.push(format!("and {more} more"));
            }
            lines.push(format!("With {}", shown.join(", ")));
        }
        lines.push(connection.line().into());
        lines.push("Click to share".into());
        Self {
            text: format!("Room · {count}"),
            title: format!("Project room {}", view.room_name()),
            lines,
            participants: count,
            connection,
        }
    }

    /// What `room_presence` answers as `widget`.
    pub fn to_json(&self) -> Value {
        json!({
            "text": self.text,
            "connection": self.connection.name(),
            "participants": self.participants,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn status_admin() -> Value {
        json!({
            "id": "r1", "name": "demo",
            "participants": [
                {"client": "c10", "id": "ssh:zoe", "name": "zoe", "kind": "user", "role": "editor", "away": true},
                {"client": "c2", "id": "ssh:me", "name": "me", "kind": "user", "role": "admin"},
                {"client": "c3", "id": "agent:Bob>ssh:me", "name": "Bob (me)", "kind": "agent", "role": "editor"},
                {"client": "c4", "id": "ssh:anna", "name": "Anna", "kind": "user", "role": "viewer"},
            ],
            "members": {
                "ssh:me": {"name": "me", "role": "admin", "since": 1},
                "ssh:zoe": {"name": "zoe", "role": "editor", "since": 2},
                "ssh:anna": {"name": "Anna", "role": "viewer", "since": 3},
                "device:ab": {"name": "tablet", "role": "viewer", "since": 4},
            },
        })
    }

    /// SPEC-0323 v6 AK-22: self first, then here before not here, by name; Remove only at
    /// members other than oneself, and only for an admin.
    #[test]
    fn people_are_listed_with_roles_and_removal_for_an_admin() {
        let mut m = ShareModel::new("c2");
        assert!(m.set_status(&status_admin()));
        assert!(m.is_admin());
        let rows: Vec<(String, String, bool)> = m
            .people()
            .iter()
            .map(|p| (p.name.clone(), p.detail(), p.removable))
            .collect();
        assert_eq!(
            rows,
            [
                ("me".to_owned(), "admin · you".to_owned(), false),
                ("Anna".into(), "viewer".into(), true),
                ("Bob (me)".into(), "editor".into(), false),
                ("zoe".into(), "editor · away".into(), true),
                ("tablet".into(), "viewer · not here".into(), true),
            ]
        );
        assert_eq!(m.summary(), "Room demo · you are admin");
    }

    /// SPEC-0323 v6 AK-22, AK-24: a member who is not admin sees no members who are absent, no
    /// Remove, no links, and the sentence of behaviour 42.
    #[test]
    fn a_member_without_admin_sees_no_links_and_no_removal() {
        let mut status = status_admin();
        status["participants"][1]["role"] = json!("editor");
        status.as_object_mut().unwrap().remove("members");
        let mut m = ShareModel::new("c2");
        m.set_links(&json!({"links": [{"id": "l1", "role": "viewer", "created": 1, "expires": NOW + 5, "live": true}]}), NOW);
        assert!(m.set_status(&status));
        assert!(!m.is_admin());
        assert!(m.links().is_empty());
        assert!(m.people().iter().all(|p| !p.removable && p.here));
        assert_eq!(m.people().len(), 4);
        assert_eq!(
            m.summary(),
            "Room demo · you are editor. Only an admin of the room makes links and removes people."
        );
        assert!(!m.ask_remove("ssh:anna"));
    }

    /// SPEC-0323 v6 AK-22: only links in force, newest first, with what remains.
    #[test]
    fn only_links_in_force_are_listed_newest_first() {
        let mut m = ShareModel::new("c2");
        m.set_status(&status_admin());
        m.set_links(
            &json!({"links": [
                {"id": "old", "role": "viewer", "created": 10, "expires": NOW + 3 * HOUR_MS, "live": true, "revoked": false},
                {"id": "new", "role": "editor", "created": 20, "expires": NOW + 7 * DAY_MS - 1000, "live": true, "revoked": false},
                {"id": "revoked", "role": "viewer", "created": 30, "expires": NOW + HOUR_MS, "live": false, "revoked": true},
                {"id": "expired", "role": "viewer", "created": 40, "expires": NOW - 1, "live": true},
                {"id": "forever", "role": "viewer", "created": 5, "expires": null, "live": true},
                {"role": "viewer"},
            ]}),
            NOW,
        );
        let ids: Vec<&str> = m.links().iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["new", "old", "forever"]);
        let left: Vec<String> = m
            .links()
            .iter()
            .map(|l| remaining(l.expires, NOW))
            .collect();
        assert_eq!(left, ["ends in 7 days", "ends in 3 h", "never ends"]);
        assert_eq!(remaining(Some(NOW + 59_000), NOW), "ends in under a minute");
        assert_eq!(remaining(Some(NOW + 20 * 60_000), NOW), "ends in 20 min");
        assert_eq!(remaining(Some(NOW + 30 * HOUR_MS), NOW), "ends in 30 h");
        assert_eq!(remaining(Some(NOW - 5), NOW), "ends in under a minute");
    }

    /// SPEC-0323 v6 AK-22: the choices map to `room.share` with an end always set.
    #[test]
    fn choices_become_share_parameters_with_an_end() {
        let mut m = ShareModel::new("c2");
        assert_eq!(
            m.share_params(),
            json!({"role": "viewer", "audience": "organisation", "ttl_seconds": 86_400})
        );
        m.choice_role = ShareRole::Editor;
        m.choice_life = Lifetime::Week;
        assert_eq!(
            m.share_params(),
            json!({"role": "editor", "audience": "organisation", "ttl_seconds": 604_800})
        );
        for life in Lifetime::ALL {
            m.choice_life = life;
            let ttl = m.share_params()["ttl_seconds"].as_u64().unwrap();
            assert!(ttl > 0 && ttl <= 30 * 86_400, "{life:?}");
        }
        assert_eq!(
            Lifetime::ALL.map(Lifetime::label),
            ["1 hour", "1 day", "7 days", "30 days"]
        );
        assert!(!m.set_fresh(&json!({"id": "x"})));
        assert!(m.fresh().is_none());
        assert!(m.set_fresh(
            &json!({"link": "ferris-idea://room/r1#s", "role": "editor", "expires": NOW})
        ));
        assert_eq!(m.fresh().unwrap().link, "ferris-idea://room/r1#s");
    }

    /// SPEC-0323 v6 AK-24: Remove asks in the row; only the second click for the same person
    /// removes; Keep keeps.
    #[test]
    fn removal_asks_in_the_row_first() {
        let mut m = ShareModel::new("c2");
        m.set_status(&status_admin());
        assert!(!m.ask_remove("ssh:me"), "never oneself");
        assert!(!m.ask_remove("agent:Bob>ssh:me"), "an agent is no member");
        assert!(m.ask_remove("ssh:anna"));
        assert_eq!(m.confirming(), Some("ssh:anna"));
        assert_eq!(m.confirm_remove("ssh:zoe"), None);
        m.keep();
        assert_eq!(m.confirm_remove("ssh:anna"), None);
        assert!(m.ask_remove("ssh:anna"));
        assert_eq!(m.confirm_remove("ssh:anna").as_deref(), Some("ssh:anna"));
        assert_eq!(m.confirming(), None);
    }

    /// SPEC-0323 v6 AK-22: without a room, the explanation; a malformed answer changes nothing.
    #[test]
    fn no_room_and_malformed_answers() {
        let mut m = ShareModel::new("c2");
        assert!(!m.known());
        assert_eq!(m.summary(), "Asking the host about the room…");
        m.set_no_room();
        assert!(m.known() && m.room_name().is_none());
        assert!(m.summary().starts_with("No room yet."));
        assert!(!m.set_status(&json!({"participants": "x"})));
        assert!(!m.set_status(&json!(null)));
        assert!(m.room_name().is_none());
        m.set_status(&status_admin());
        m.set_links(&json!({"links": 5}), NOW);
        m.set_links(&json!(null), NOW);
        assert!(m.links().is_empty());
        assert!(m.set_status(&json!({"id": "r1", "participants": [{"name": "no client"}, 7]})));
        assert!(m.people().is_empty());
    }

    /// SPEC-0323 v6 AK-25: "Room · N" counts oneself; the tooltip names the others, at most ten.
    #[test]
    fn the_widget_counts_oneself_and_names_the_others() {
        let status = status_admin();
        let view = RoomView::from_status("c2", &status, 0);
        let w = Widget::of(&view, Connection::Connected);
        assert_eq!(w.text, "Room · 4");
        assert_eq!(w.title, "Project room demo");
        assert_eq!(
            w.lines,
            [
                "You are admin",
                "With Anna, Bob (me), zoe",
                "Connected",
                "Click to share"
            ]
        );
        assert_eq!(
            w.to_json(),
            json!({"text": "Room · 4", "connection": "connected", "participants": 4})
        );
        let alone = RoomView::from_status(
            "c2",
            &json!({"id": "r", "name": "solo", "participants": [{"client": "c2", "role": "admin"}]}),
            0,
        );
        let w = Widget::of(&alone, Connection::Reconnecting);
        assert_eq!(w.text, "Room · 1");
        assert_eq!(w.lines[1], "Nobody else is here");
        assert!(w.lines[2].starts_with("Reconnecting — your edits wait"));
        assert!(Widget::of(&alone, Connection::Offline).lines[2].starts_with("Offline"));
        let many: Vec<Value> = (0..13)
            .map(|i| json!({"client": format!("c{}", i + 10), "name": format!("p{i:02}")}))
            .collect();
        let crowd = RoomView::from_status("c2", &json!({"id": "r", "participants": many}), 0);
        let w = Widget::of(&crowd, Connection::Connected);
        assert_eq!(w.text, "Room · 14");
        assert!(w.lines[1].ends_with("p09, and 3 more"), "{}", w.lines[1]);
    }

    #[test]
    fn a_link_leads_to_its_host_when_the_window_knows_it() {
        let fp = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
        let secret = "ab".repeat(32);
        let mut m = ShareModel::new("c1");
        assert!(
            !m.lead_to("h.test", 22, "/srv/p", fp),
            "nothing to extend yet"
        );
        m.set_fresh(&json!({
            "link": format!("ferris-idea://room/0f0f#{secret}"),
            "role": "editor",
            "expires": 7_200_000u64,
        }));
        assert!(!m.lead_to("-bad", 22, "/srv/p", fp));
        assert!(m.fresh().unwrap().link.ends_with(&format!("0f0f#{secret}")));
        assert!(m.lead_to("h.test", 2222, "/srv/p", fp));
        let link = ferris_idea_remote::RoomLink::parse(&m.fresh().unwrap().link, 0).unwrap();
        assert_eq!(
            (link.host.as_str(), link.port, link.folder.as_str()),
            ("h.test", 2222, "/srv/p")
        );
        assert_eq!(link.fingerprint, fp);
        assert_eq!((link.role.as_str(), link.expires), ("editor", Some(7200)));
        assert_eq!(link.room, "0f0f");
    }
}
