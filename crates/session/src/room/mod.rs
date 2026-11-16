//! The project room (`SPEC-0323`, first slice of M6.15): one ordered journal of every change in a
//! project, per-person undo, roles, links, presence, comments and a shared history next to Git.
//!
//! The document store stays the only authority for text; the room hears every change it applies
//! ([`crate::docs::StoreSink`]) and adds order across files, authorship, undo and history. The
//! store calls in under its own lock, so a room never calls the backend while holding its lock:
//! everything that must leave a room goes through its outbox and the pump thread in [`bind`].

pub mod access;
pub mod bind;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ropey::Rope;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::backend::SessionHandle;
use crate::docs::{SetChange, StoreEvent, StoreRecord};
use crate::hello::hex;
use crate::ot::{Op, TextOp, text_hash};
use crate::rpc::{ErrorCode, RpcError};
use access::{Access, Admission, Kind, Person, Role};

pub use bind::{RoomConfig, Rooms, bind};

/// Typing by one person in one document closer together than this is one undo step.
const GROUP_GAP_MS: u64 = 1000;

fn not_granted(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::NotGranted, message)
}

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Invalid, message)
}

/// What a journal entry records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Body {
    /// A document changed.
    Text {
        /// Document id at that time.
        doc: String,
        /// The revision after the change.
        revision: u64,
        /// Length of the text before.
        base_len: usize,
        /// The change.
        ops: Vec<Op>,
        /// What takes it back.
        inverse: Vec<Op>,
        /// Hash of the text after.
        hash: String,
    },
    /// The file tree changed.
    File {
        /// `created`, `created-dir`, `renamed`, `deleted`, `changed`.
        change: String,
        /// The path.
        path: String,
        /// A rename's former path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        /// Stored contents of a deleted file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob: Option<String>,
    },
    /// A named version.
    Version {
        /// Its name.
        name: String,
    },
}

/// One entry of the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Room-wide position.
    pub seq: u64,
    /// Unix milliseconds.
    pub time: u64,
    /// Person id, or `disk`.
    pub actor: String,
    /// Name to show.
    pub author: String,
    /// Undo group.
    pub group: u64,
    /// The change.
    #[serde(flatten)]
    pub body: Body,
}

impl Entry {
    /// The form in `room.since` and `room.history`; `full` includes the edit ranges.
    fn to_json(&self, full: bool) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if !full && let Some(o) = v.as_object_mut() {
            o.remove("ops");
            o.remove("inverse");
            o.remove("blob");
        }
        v
    }

    fn text_doc(&self) -> Option<&str> {
        match &self.body {
            Body::Text { doc, .. } => Some(doc),
            _ => None,
        }
    }
}

/// A comment on a position of a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    /// Id.
    pub id: u64,
    /// Document id.
    pub doc: String,
    /// Offset in scalars, moved with the text.
    pub at: usize,
    /// Person id.
    pub author: String,
    /// Name to show.
    pub name: String,
    /// The text.
    pub text: String,
    /// Unix milliseconds.
    pub time: u64,
    /// Resolved.
    pub resolved: bool,
}

/// What survives a restart besides the journal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Meta {
    id: String,
    name: String,
    created: u64,
    next_seq: u64,
    next_group: u64,
    next_comment: u64,
    access: Access,
    comments: Vec<Comment>,
}

/// One session in the room.
#[derive(Clone)]
struct Participant {
    session: SessionHandle,
    person: Person,
    role: Role,
    link: Option<String>,
    expires: Option<u64>,
    color: u8,
    presence: Value,
    away_since: Option<u64>,
    gone: bool,
    window: (u64, u32),
}

impl Participant {
    fn to_json(&self) -> Value {
        json!({
            "client": self.session.client(),
            "id": self.person.id,
            "name": self.person.name,
            "kind": self.person.kind,
            "role": self.role.name(),
            "color": self.color,
            "away": self.away_since.is_some(),
            "presence": self.presence,
        })
    }

    fn is_guest(&self) -> bool {
        self.person.kind == Kind::Guest
    }

    /// What a guest sees of this participant (`SPEC-0323` behaviour 21a): the session's
    /// client number, a neutral name, role and presence; no id, no login or device name, no
    /// kind beyond member, agent or guest, and a colour that does not follow the person from
    /// room to room. `clients` are the room's current sessions, the only targets `follows` may
    /// name.
    fn guest_json(&self, room_id: &str, clients: &HashMap<String, String>) -> Value {
        let client = self.session.client();
        let mut presence = self.presence.clone();
        if let Some(o) = presence.as_object_mut()
            && !o["follows"]
                .as_str()
                .is_some_and(|f| clients.contains_key(f))
        {
            o.insert("follows".into(), Value::Null);
        }
        let color = Sha256::digest(format!("{room_id}\n{client}").as_bytes())[0] % 12;
        json!({
            "client": client,
            "name": public_name(&self.person.id, &self.person.name),
            "kind": public_kind(self.person.kind),
            "role": self.role.name(),
            "color": color,
            "away": self.away_since.is_some(),
            "presence": presence,
        })
    }
}

/// The name a guest is shown for the person `id` (`SPEC-0323` behaviour 21a): a guest's random
/// name, never a login, device or agent name.
fn public_name(id: &str, name: &str) -> String {
    if id.starts_with("guest:") || id.starts_with("web-guest:") || id == "disk" {
        return name.to_owned();
    }
    if id.starts_with("agent:") {
        return "Agent".to_owned();
    }
    "Member".to_owned()
}

fn public_kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Guest => "guest",
        Kind::Agent => "agent",
        Kind::User | Kind::Device => "member",
    }
}

/// A journal entry as a guest sees it: no actor id, a neutral author.
fn guest_entry(entry: &Entry, full: bool) -> Value {
    let mut v = entry.to_json(full);
    if let Some(o) = v.as_object_mut() {
        o.remove("actor");
        o.insert(
            "author".into(),
            json!(public_name(&entry.actor, &entry.author)),
        );
    }
    v
}

/// A comment as a guest sees it: no author id, a neutral name.
fn guest_comment(c: &Comment) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("author");
        o.insert("name".into(), json!(public_name(&c.author, &c.name)));
    }
    v
}

/// How an undo or redo group was made.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Undo,
    Redo,
}

/// What leaves the room through the pump.
pub(crate) enum Out {
    Notify(SessionHandle, &'static str, Value),
    End(String, &'static str),
    Journal(String),
    Meta(String),
    Rewrite(String),
}

/// A room in memory.
pub(crate) struct Room {
    meta: Meta,
    journal: VecDeque<Entry>,
    participants: HashMap<String, Participant>,
    by_client: HashMap<String, String>,
    undo: BTreeMap<String, Vec<u64>>,
    redo: BTreeMap<String, Vec<u64>>,
    typing: HashMap<(String, String), (u64, u64)>,
    origins: HashMap<u64, Origin>,
    recent: HashMap<String, u64>,
    max_entries: usize,
    pub(crate) out: Vec<Out>,
}

fn is_noise(path: &str) -> bool {
    path.contains(".idecraft-save-")
        || path.contains(".idecraft-write-")
        || [".git", "target", "node_modules", ".idecraft"]
            .iter()
            .any(|d| path == *d || path.starts_with(&format!("{d}/")))
}

fn move_prefix(name: &str, from: &str, to: &str) -> Option<String> {
    if name == from {
        return Some(to.to_owned());
    }
    name.strip_prefix(&format!("{from}/"))
        .map(|rest| format!("{to}/{rest}"))
}

/// Moves `at` over `ops`, ranges of the text `at` refers to.
fn shift_anchor(at: usize, ops: &[Op]) -> usize {
    let mut delta = 0isize;
    for op in ops {
        if op.at + op.delete <= at {
            delta += op.insert.chars().count() as isize - op.delete as isize;
        } else if op.at <= at {
            let moved = op.at as isize + delta;
            return moved.max(0) as usize;
        } else {
            break;
        }
    }
    (at as isize + delta).max(0) as usize
}

impl Room {
    fn new(meta: Meta, journal: VecDeque<Entry>, max_entries: usize) -> Self {
        Self {
            meta,
            journal,
            participants: HashMap::new(),
            by_client: HashMap::new(),
            undo: BTreeMap::new(),
            redo: BTreeMap::new(),
            typing: HashMap::new(),
            origins: HashMap::new(),
            recent: HashMap::new(),
            max_entries,
            out: Vec::new(),
        }
    }

    fn save_meta(&mut self) {
        if let Ok(text) = serde_json::to_string(&self.meta) {
            self.out.push(Out::Meta(text));
        }
    }

    fn next_group(&mut self) -> u64 {
        self.meta.next_group += 1;
        self.meta.next_group
    }

    /// Who a store author is.
    fn actor(&self, author: &str) -> (String, String) {
        if author == "disk" {
            return ("disk".into(), "disk".into());
        }
        if let Some(p) = self
            .by_client
            .get(author)
            .and_then(|sid| self.participants.get(sid))
        {
            return (p.person.id.clone(), p.person.name.clone());
        }
        let name = self
            .meta
            .access
            .members
            .get(author)
            .map_or_else(|| author.to_owned(), |m| m.name.clone());
        (author.to_owned(), name)
    }

    /// Sends `full` to members and `guest` to guests (`SPEC-0323` behaviour 21a).
    fn announce(
        &mut self,
        method: &'static str,
        full: &Value,
        guest: &Value,
        except: Option<&str>,
    ) {
        for (sid, p) in &self.participants {
            if except != Some(sid.as_str()) && !p.gone {
                let params = if p.is_guest() { guest } else { full };
                self.out
                    .push(Out::Notify(p.session.clone(), method, params.clone()));
            }
        }
    }

    /// Whether the session `sid` is a guest of the room.
    pub(crate) fn is_guest(&self, sid: &str) -> bool {
        self.participants
            .get(sid)
            .is_some_and(Participant::is_guest)
    }

    /// The participant `sid` as members and as guests see it.
    fn views(&self, sid: &str) -> Option<(Value, Value)> {
        let p = self.participants.get(sid)?;
        Some((p.to_json(), p.guest_json(&self.meta.id, &self.by_client)))
    }

    fn push_entry(&mut self, entry: Entry) {
        if let Ok(line) = serde_json::to_string(&entry) {
            self.out.push(Out::Journal(line));
        }
        let summary = entry.to_json(false);
        let public = guest_entry(&entry, false);
        self.journal.push_back(entry);
        while self.journal.len() > self.max_entries {
            self.journal.pop_front();
        }
        self.announce("room.changed", &summary, &public, None);
    }

    fn append(&mut self, actor: (String, String), group: u64, body: Body, now: u64) -> u64 {
        self.meta.next_seq += 1;
        let seq = self.meta.next_seq;
        self.push_entry(Entry {
            seq,
            time: now,
            actor: actor.0,
            author: actor.1,
            group,
            body,
        });
        seq
    }

    /// Adds a change the store has just applied (called under the store's lock).
    pub(crate) fn absorb(&mut self, record: StoreRecord, now: u64) {
        let actor = self.actor(&record.author);
        match record.event {
            StoreEvent::Text(t) => {
                let group = match record.group {
                    Some(g) => g,
                    None => {
                        let key = (actor.0.clone(), t.doc.clone());
                        let g = match self.typing.get(&key) {
                            Some((last, g)) if now.saturating_sub(*last) < GROUP_GAP_MS => *g,
                            _ => self.next_group(),
                        };
                        self.typing.insert(key, (now, g));
                        g
                    }
                };
                self.track(&actor.0, group);
                let ops = t.op.to_ops();
                for c in self.meta.comments.iter_mut().filter(|c| c.doc == t.doc) {
                    c.at = shift_anchor(c.at, &ops);
                }
                self.append(
                    actor,
                    group,
                    Body::Text {
                        doc: t.doc,
                        revision: t.revision,
                        base_len: t.op.base_len(),
                        ops,
                        inverse: t.inverse.to_ops(),
                        hash: t.hash,
                    },
                    now,
                );
            }
            StoreEvent::File(f) => {
                if actor.0 == "disk" {
                    if is_noise(&f.path) {
                        return;
                    }
                    // The echo of a change made through the room.
                    if self.recent.get(&f.path).is_some_and(|until| now < *until) {
                        return;
                    }
                } else {
                    if self.recent.len() > 4096 {
                        self.recent.retain(|_, until| now < *until);
                    }
                    self.recent.insert(f.path.clone(), now + 5000);
                    if let Some(from) = &f.from {
                        self.recent.insert(from.clone(), now + 5000);
                    }
                }
                let group = match record.group {
                    Some(g) => g,
                    None => self.next_group(),
                };
                self.track(&actor.0, group);
                if let (Some(from), "renamed") = (&f.from, f.kind) {
                    for c in &mut self.meta.comments {
                        if let Some(n) = move_prefix(&c.doc, from, &f.path) {
                            c.doc = n;
                        }
                    }
                }
                self.append(
                    actor,
                    group,
                    Body::File {
                        change: f.kind.to_owned(),
                        path: f.path,
                        from: f.from,
                        blob: f.blob,
                    },
                    now,
                );
            }
        }
    }

    /// Puts `group` on the right stack of `person`.
    fn track(&mut self, person: &str, group: u64) {
        if person == "disk" {
            return;
        }
        match self.origins.get(&group) {
            Some(Origin::Undo) => push_once(self.redo.entry(person.into()).or_default(), group),
            Some(Origin::Redo) => push_once(self.undo.entry(person.into()).or_default(), group),
            None => {
                push_once(self.undo.entry(person.into()).or_default(), group);
                self.redo.entry(person.into()).or_default().clear();
            }
        }
    }

    /// The newest name of `doc` after the journal position `pos`.
    fn name_after(&self, mut doc: String, pos: usize) -> String {
        for e in self.journal.iter().skip(pos + 1) {
            if let Body::File {
                change,
                path,
                from: Some(from),
                ..
            } = &e.body
                && change == "renamed"
                && let Some(n) = move_prefix(&doc, from, path)
            {
                doc = n;
            }
        }
        doc
    }

    /// The change set that takes back `group`, against everything since (`SPEC-0323` behaviour 8).
    fn plan_reversal(&self, group: u64) -> Result<Vec<SetChange>, RpcError> {
        let positions: Vec<usize> = self
            .journal
            .iter()
            .enumerate()
            .filter(|(_, e)| e.group == group)
            .map(|(i, _)| i)
            .collect();
        if positions.is_empty() {
            return Err(invalid("that change is no longer in the history"));
        }
        let mut totals: BTreeMap<String, (TextOp, u64)> = BTreeMap::new();
        let mut files = Vec::new();
        for &pos in positions.iter().rev() {
            let e = &self.journal[pos];
            match &e.body {
                Body::Text {
                    doc,
                    inverse,
                    ops,
                    base_len,
                    ..
                } => {
                    let forward = TextOp::from_ops(*base_len, ops)?;
                    let mut op = TextOp::from_ops(forward.target_len(), inverse)?;
                    let mut name = doc.clone();
                    let mut newest = None;
                    for later in self.journal.iter().skip(pos + 1) {
                        match &later.body {
                            Body::File {
                                change,
                                path,
                                from: Some(from),
                                ..
                            } if change == "renamed" => {
                                if let Some(n) = move_prefix(&name, from, path) {
                                    name = n;
                                }
                            }
                            Body::Text {
                                doc: d,
                                ops,
                                base_len,
                                revision,
                                ..
                            } if *d == name => {
                                let l = TextOp::from_ops(*base_len, ops)?;
                                op = TextOp::transform(&l, &op)?.1;
                                newest = Some(*revision);
                            }
                            _ => {}
                        }
                    }
                    let base = newest.unwrap_or(match &e.body {
                        Body::Text { revision, .. } => *revision,
                        _ => 0,
                    });
                    match totals.remove(&name) {
                        Some((total, b)) => {
                            let next = TextOp::transform(&total, &op)?.1;
                            totals.insert(name, (total.compose(&next)?, b));
                        }
                        None => {
                            totals.insert(name, (op, base));
                        }
                    }
                }
                Body::File {
                    change,
                    path,
                    from,
                    blob,
                } => {
                    let now = self.name_after(path.clone(), pos);
                    match change.as_str() {
                        "created" | "created-dir" => files.push(SetChange::Delete { path: now }),
                        "renamed" => {
                            if let Some(from) = from {
                                files.push(SetChange::Rename {
                                    from: now,
                                    to: from.clone(),
                                });
                            }
                        }
                        "deleted" => files.push(SetChange::Create {
                            path: path.clone(),
                            dir: false,
                            text: None,
                            blob: blob.clone(),
                        }),
                        _ => {}
                    }
                }
                Body::Version { .. } => {}
            }
        }
        let mut out = files;
        for (name, (op, base)) in totals {
            if !op.is_noop() {
                out.push(SetChange::Edit {
                    path: name,
                    base: Some(base),
                    ops: op.to_ops(),
                });
            }
        }
        Ok(out)
    }

    /// Pops the next step of `person`; returns the new group and its changes.
    fn plan_undo(
        &mut self,
        person: &str,
        redo: bool,
    ) -> Result<Option<(u64, Vec<SetChange>, u64)>, RpcError> {
        let stack = if redo { &mut self.redo } else { &mut self.undo };
        let Some(group) = stack.get_mut(person).and_then(Vec::pop) else {
            return Ok(None);
        };
        let plan = self.plan_reversal(group);
        match plan {
            Ok(changes) => {
                let new = self.next_group();
                self.origins
                    .insert(new, if redo { Origin::Redo } else { Origin::Undo });
                Ok(Some((new, changes, group)))
            }
            Err(e) => {
                let stack = if redo { &mut self.redo } else { &mut self.undo };
                stack.entry(person.into()).or_default().push(group);
                Err(e)
            }
        }
    }

    /// Puts a step back after a failed undo.
    fn restore_step(&mut self, person: &str, redo: bool, step: u64, new: u64) {
        self.origins.remove(&new);
        let stack = if redo { &mut self.redo } else { &mut self.undo };
        stack.entry(person.into()).or_default().push(step);
    }

    /// The text of `doc` at journal position `seq`, walking back from `current`
    /// (`SPEC-0323` behaviour 25).
    fn text_at(&self, doc: &str, current: &str, seq: u64) -> Result<String, RpcError> {
        let gap = || {
            RpcError::new(
                ErrorCode::Conflict,
                format!(
                    "{doc} changed while no backend was watching; the history does not reach \
                     back across that gap"
                ),
            )
        };
        let mut rope = Rope::from_str(current);
        let mut name = doc.to_owned();
        let mut checked = false;
        let mut target_hash = None;
        for e in self.journal.iter().rev() {
            match &e.body {
                Body::Text {
                    doc: d,
                    base_len,
                    ops,
                    inverse,
                    hash,
                    ..
                } if *d == name => {
                    if e.seq <= seq {
                        target_hash = Some(hash.clone());
                        break;
                    }
                    if !checked {
                        if text_hash(&rope) != *hash {
                            return Err(gap());
                        }
                        checked = true;
                    }
                    let forward = TextOp::from_ops(*base_len, ops)?;
                    let back = TextOp::from_ops(forward.target_len(), inverse)?;
                    back.apply(&mut rope).map_err(|_| gap())?;
                }
                Body::File {
                    change,
                    path,
                    from: Some(from),
                    ..
                } if change == "renamed" && *path == name && e.seq > seq => {
                    name.clone_from(from);
                }
                _ => {}
            }
        }
        if let Some(h) = target_hash
            && text_hash(&rope) != h
        {
            return Err(gap());
        }
        Ok(rope.to_string())
    }

    /// The room for the participant `viewer`: a guest sees participants as
    /// [`Participant::guest_json`] shows them; only an admin sees the list of members.
    fn status_for(&self, viewer: &str) -> Value {
        let mut out = self.status();
        let role = self.participants.get(viewer).map(|p| p.role);
        if self.is_guest(viewer) {
            out["participants"] = self
                .participants
                .values()
                .filter(|p| !p.gone)
                .map(|p| p.guest_json(&self.meta.id, &self.by_client))
                .collect();
        }
        if role != Some(Role::Admin)
            && let Some(o) = out.as_object_mut()
        {
            o.remove("members");
        }
        out
    }

    /// The journal entries after `after` (of `doc` only, when given) as `viewer` sees them.
    fn entries_for(
        &self,
        viewer: &str,
        after: u64,
        doc: Option<&str>,
        limit: usize,
        full: bool,
    ) -> Value {
        let guest = self.is_guest(viewer);
        let list: Vec<Value> = self
            .journal
            .iter()
            .filter(|e| e.seq > after)
            .filter(|e| {
                doc.is_none_or(|d| {
                    e.text_doc() == Some(d)
                        || matches!(&e.body, Body::File { path, from, .. }
                            if path == d || from.as_deref() == Some(d))
                })
            })
            .take(limit)
            .map(|e| {
                if guest {
                    guest_entry(e, full)
                } else {
                    e.to_json(full)
                }
            })
            .collect();
        json!({ "entries": list, "seq": self.meta.next_seq })
    }

    /// The comments (of `doc` only, when given) as `viewer` sees them.
    fn comments_for(&self, viewer: &str, doc: Option<&str>) -> Value {
        let guest = self.is_guest(viewer);
        let list: Vec<Value> = self
            .meta
            .comments
            .iter()
            .filter(|c| doc.is_none_or(|d| c.doc == d))
            .map(|c| {
                if guest {
                    guest_comment(c)
                } else {
                    serde_json::to_value(c).unwrap_or(Value::Null)
                }
            })
            .collect();
        json!({ "comments": list })
    }

    fn status(&self) -> Value {
        let people: Vec<Value> = self
            .participants
            .values()
            .filter(|p| !p.gone)
            .map(Participant::to_json)
            .collect();
        json!({
            "id": self.meta.id,
            "name": self.meta.name,
            "link": format!("idecraft://room/{}", self.meta.id),
            "seq": self.meta.next_seq,
            "participants": people,
            "members": self.meta.access.members,
        })
    }

    fn links_json(&self, now: u64) -> Value {
        Value::Array(
            self.meta
                .access
                .links
                .iter()
                .map(|l| {
                    json!({
                        "id": l.id, "role": l.role.name(), "audience": l.audience.name(),
                        "by": l.by, "created": l.created, "expires": l.expires,
                        "revoked": l.revoked, "live": self.meta.access.link_live(&l.id, now),
                    })
                })
                .collect(),
        )
    }

    fn enter(&mut self, session: SessionHandle, admission: &Admission) {
        let sid = session.id().to_owned();
        let color = Sha256::digest(admission.person.id.as_bytes())[0] % 12;
        let client = session.client().to_owned();
        let p = Participant {
            session,
            person: admission.person.clone(),
            role: admission.role,
            link: admission.link.clone(),
            expires: admission.expires,
            color,
            presence: Value::Null,
            away_since: None,
            gone: false,
            window: (0, 0),
        };
        self.by_client.insert(client, sid.clone());
        self.participants.insert(sid.clone(), p);
        if let Some((view, public)) = self.views(&sid) {
            self.announce("room.joined", &view, &public, Some(&sid));
        }
    }

    fn leave(&mut self, sid: &str) {
        if let Some(p) = self.participants.remove(sid) {
            self.by_client.remove(p.session.client());
            self.announce_left(&p, None);
        }
    }

    /// `room.left` for `p`: members learn its id, guests only its client number.
    fn announce_left(&mut self, p: &Participant, except: Option<&str>) {
        let client = p.session.client();
        let params = json!({"client": client, "id": p.person.id});
        self.announce("room.left", &params, &json!({ "client": client }), except);
    }

    /// Removes old entries (`SPEC-0323` behaviour 26).
    fn prune(&mut self, now: u64, retention_ms: u64) {
        let before = self.journal.len();
        let cut = now.saturating_sub(retention_ms);
        while self.journal.front().is_some_and(|e| e.time < cut) {
            self.journal.pop_front();
        }
        if self.journal.len() != before {
            let lines: Vec<String> = self
                .journal
                .iter()
                .filter_map(|e| serde_json::to_string(e).ok())
                .collect();
            self.out.push(Out::Rewrite(lines.join("\n")));
        }
    }

    fn participant(&self, sid: &str) -> Result<&Participant, RpcError> {
        self.participants
            .get(sid)
            .ok_or_else(|| not_granted("not in this room"))
    }

    fn need(&self, sid: &str, role: Role) -> Result<&Participant, RpcError> {
        let p = self.participant(sid)?;
        if p.role < role {
            return Err(not_granted(format!(
                "not granted: this needs the role {}",
                role.name()
            )));
        }
        Ok(p)
    }

    /// A guest's identity.
    fn guest_person(random: [u8; 8]) -> Person {
        let id = hex(&random);
        Person {
            name: format!("Guest {}", &id[..6]),
            id: format!("guest:{id}"),
            kind: Kind::Guest,
        }
    }
}

fn push_once(stack: &mut Vec<u64>, group: u64) {
    if stack.last() != Some(&group) {
        stack.push(group);
    }
}

/// A folder name for a project.
fn folder_for(root: &Path) -> String {
    hex(&Sha256::digest(root.display().to_string().as_bytes())[..16])
}

/// The files of a room.
pub(crate) struct Files {
    pub(crate) dir: PathBuf,
}

impl Files {
    pub(crate) fn of(state_dir: &Path, root: &Path) -> Self {
        Self {
            dir: state_dir.join("rooms").join(folder_for(root)),
        }
    }

    pub(crate) fn blobs(&self) -> PathBuf {
        self.dir.join("blobs")
    }

    pub(crate) fn meta(&self) -> PathBuf {
        self.dir.join("room.json")
    }

    pub(crate) fn journal(&self) -> PathBuf {
        self.dir.join("journal.jsonl")
    }

    fn load(&self, retention_ms: u64, max_entries: usize, now: u64) -> Option<Room> {
        let text = std::fs::read_to_string(self.meta()).ok()?;
        let mut meta: Meta = serde_json::from_str(&text).ok()?;
        let mut journal = VecDeque::new();
        if let Ok(lines) = std::fs::read_to_string(self.journal()) {
            for line in lines.lines() {
                if let Ok(e) = serde_json::from_str::<Entry>(line) {
                    meta.next_seq = meta.next_seq.max(e.seq);
                    meta.next_group = meta.next_group.max(e.group);
                    journal.push_back(e);
                }
            }
        }
        while journal.len() > max_entries {
            journal.pop_front();
        }
        let mut room = Room::new(meta, journal, max_entries);
        room.prune(now, retention_ms);
        Some(room)
    }
}

/// A handle kept by the store's sink.
pub(crate) type Handle = Arc<Rooms>;
