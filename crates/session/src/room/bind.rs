//! The `room.*` methods of `ferris-idea serve` (`SPEC-0323` behaviour 28), the session gate and the
//! pump that carries everything a room sends out, outside the locks.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::access::{self, Admission, Audience, Kind, Person, Role, person_of};
use super::{Body, Files, Handle, Meta, Out, Room, invalid, not_granted};
use crate::backend::{Backend, Call, Right, RoomLink, SessionHandle};
use crate::docs::{SetChange, StoreRecord, StoreSink};
use crate::hello::hex;
use crate::ot::TextOp;
use crate::reattach::{append_private, private_dir, write_private};
use crate::rpc::{ErrorCode, RpcError};

#[path = "comments.rs"]
mod comments;

/// The clock of a room: Unix milliseconds.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The most text in one comment.
const MAX_COMMENT: usize = 16 << 10;
/// The most comments a room keeps.
const MAX_COMMENTS: usize = 10_000;
/// The most entries one history answer returns.
const MAX_LIST: usize = 1000;
/// The most text both sides of one `room.diff` answer carry together, so the answer stays below
/// the session's 1 MiB (`SPEC-0323` v9 behaviour 63).
const MAX_DIFF_TEXT: usize = 448 << 10;
/// The most files one `room.restore` brings back (`SPEC-0323` v9 behaviour 64).
const MAX_RESTORE_DOCS: usize = 200;
/// How often modified documents are saved (`SPEC-0323` behaviour 6).
const SAVE_MS: u64 = 1000;
/// Presence updates per second and session (`SPEC-0323` behaviour 21).
const PRESENCE_PER_SECOND: u32 = 30;

/// The limits of a room.
#[derive(Clone)]
pub struct RoomConfig {
    /// The clock.
    pub now: Clock,
    /// How long a lost participant is shown as away.
    pub away_limit_ms: u64,
    /// How long the journal keeps entries.
    pub retention_ms: u64,
    /// The most journal entries.
    pub max_entries: usize,
    /// How often the pump looks after links, away states and saving.
    pub tick: Duration,
    /// Saves modified documents every tick (`SPEC-0323` behaviour 6); off only for tests that
    /// need unsaved edits to stay unsaved.
    pub autosave: bool,
}

impl Default for RoomConfig {
    fn default() -> Self {
        Self {
            now: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64)
            }),
            away_limit_ms: 60_000,
            retention_ms: 30 * 24 * access::HOUR_MS,
            max_entries: 200_000,
            tick: Duration::from_millis(200),
            autosave: true,
        }
    }
}

#[derive(Default)]
struct Inner {
    room: Option<Room>,
    files: Option<Files>,
}

/// The rooms of a backend (one per project folder; a backend serves one project).
pub struct Rooms {
    cfg: RoomConfig,
    state_dir: Option<std::path::PathBuf>,
    inner: Mutex<Inner>,
    wake: Condvar,
    /// One flush at a time, so nothing written can overtake something queued earlier.
    flushing: Mutex<()>,
    /// When modified documents were last saved.
    saved: AtomicU64,
}

fn random<const N: usize>() -> Result<[u8; N], RpcError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b)
        .map_err(|e| RpcError::new(ErrorCode::Internal, format!("no random numbers: {e}")))?;
    Ok(b)
}

fn no_room() -> RpcError {
    RpcError::new(ErrorCode::NotFound, "this project has no room")
        .hint("create it with room.create")
}

fn str_of<'a>(params: &'a Value, name: &str) -> Result<&'a str, RpcError> {
    params[name]
        .as_str()
        .ok_or_else(|| RpcError::new(ErrorCode::Invalid, format!("`{name}` is missing")))
}

fn backend_of(call: &Call) -> Result<Backend, RpcError> {
    call.session
        .backend_now()
        .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "the backend has stopped"))
}

/// The secret of a link given as `ferris-idea://room/<id>#<secret>` or bare.
fn secret_of(link: &str) -> &str {
    link.rsplit_once('#').map_or(link, |(_, s)| s)
}

impl Rooms {
    fn now(&self) -> u64 {
        (self.cfg.now)()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sink(self: &Handle) -> StoreSink {
        let me = Arc::clone(self);
        Arc::new(move |record: StoreRecord| {
            let now = me.now();
            let mut inner = me.lock();
            if let Some(room) = inner.room.as_mut() {
                room.absorb(record, now);
            }
            drop(inner);
            me.wake.notify_all();
        })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Room) -> Result<T, RpcError>) -> Result<T, RpcError> {
        let mut inner = self.lock();
        let room = inner.room.as_mut().ok_or_else(no_room)?;
        let out = f(room);
        drop(inner);
        self.wake.notify_all();
        out
    }

    /// Carries everything queued out of the room now (the pump does this all the time; tests and
    /// a shutdown can force it).
    pub fn flush(&self, backend: &Backend) {
        let _one = self.flushing.lock().unwrap_or_else(|e| e.into_inner());
        let (out, dir) = {
            let mut inner = self.lock();
            let out = inner
                .room
                .as_mut()
                .map(|r| std::mem::take(&mut r.out))
                .unwrap_or_default();
            (out, inner.files.as_ref().map(|f| f.dir.clone()))
        };
        let ends: Vec<(String, &'static str)> = out
            .iter()
            .filter_map(|o| match o {
                Out::End(sid, why) => Some((sid.clone(), *why)),
                _ => None,
            })
            .collect();
        for o in out {
            match o {
                Out::Notify(s, method, params) => s.notify(method, params),
                Out::End(..) => {}
                Out::Journal(line) => {
                    if let Some(d) = &dir {
                        let _ = append_private(&d.join("journal.jsonl"), &line);
                    }
                }
                Out::Meta(text) => {
                    if let Some(d) = &dir {
                        let _ = write_private(&d.join("room.json"), text.as_bytes());
                    }
                }
                Out::Rewrite(text) => {
                    if let Some(d) = &dir {
                        let _ = write_private(&d.join("journal.jsonl"), text.as_bytes());
                    }
                }
            }
        }
        for (sid, why) in ends {
            backend.end_session_now(&sid, why);
        }
    }

    /// Looks after expired links, lost participants and retention once.
    pub fn tick(&self, backend: &Backend) {
        let now = self.now();
        let handles: Vec<(String, SessionHandle)> = {
            let inner = self.lock();
            let Some(room) = &inner.room else {
                return;
            };
            room.participants
                .iter()
                .map(|(sid, p)| (sid.clone(), p.session.clone()))
                .collect()
        };
        let states: Vec<(String, bool, bool)> = handles
            .into_iter()
            .map(|(sid, h)| {
                let (alive, connected) = (h.alive(), h.connected());
                (sid, alive, connected)
            })
            .collect();
        {
            let mut inner = self.lock();
            let Some(room) = inner.room.as_mut() else {
                return;
            };
            for (sid, alive, connected) in states {
                if !alive {
                    room.leave(&sid);
                    continue;
                }
                let Some(p) = room.participants.get_mut(&sid) else {
                    continue;
                };
                let (was_gone, was_away) = (p.gone, p.away_since.is_some());
                if connected {
                    p.away_since = None;
                    p.gone = false;
                } else {
                    let since = *p.away_since.get_or_insert(now);
                    if now.saturating_sub(since) >= self.cfg.away_limit_ms {
                        p.gone = true;
                    }
                }
                let (gone, away) = (p.gone, p.away_since.is_some());
                // The others learn every change of state (`SPEC-0323` v4 behaviours 10, 21).
                if gone && !was_gone {
                    let p = p.clone();
                    room.announce_left(&p, Some(&sid));
                } else if (was_gone && !gone) || away != was_away {
                    let method = if was_gone {
                        "room.joined"
                    } else {
                        "room.presence"
                    };
                    if let Some((view, public)) = room.views(&sid) {
                        room.announce(method, &view, &public, Some(&sid));
                    }
                }
            }
            let ended: Vec<String> = room
                .participants
                .iter()
                .filter(|(_, p)| {
                    p.link
                        .as_ref()
                        .is_some_and(|l| !room.meta.access.link_live(l, now))
                        || p.expires.is_some_and(|e| now >= e)
                })
                .map(|(sid, _)| sid.clone())
                .collect();
            for sid in ended {
                room.leave(&sid);
                room.out.push(Out::End(sid, "link ended"));
            }
            room.prune(now, self.cfg.retention_ms);
        }
        if self.cfg.autosave && now.saturating_sub(self.saved.load(Ordering::Relaxed)) >= SAVE_MS {
            self.saved.store(now, Ordering::Relaxed);
            backend.autosave_room();
        }
        self.flush(backend);
    }

    fn make(self: &Handle, root: &Path) -> Result<Files, RpcError> {
        let state = self
            .state_dir
            .as_ref()
            .ok_or_else(|| RpcError::new(ErrorCode::Unavailable, "no state folder for a room"))?;
        let files = Files::of(state, root);
        let dir = |p: &Path| {
            private_dir(p)
                .map_err(|e| RpcError::new(ErrorCode::Internal, format!("room folder: {e:?}")))
        };
        dir(state)?;
        dir(&state.join("rooms"))?;
        dir(&files.dir)?;
        dir(&files.blobs())?;
        Ok(files)
    }

    fn admit(
        self: &Handle,
        backend: &Backend,
        session: &SessionHandle,
        token: Option<&str>,
        rights: &[Right],
        peer: &str,
    ) -> Result<Admission, RpcError> {
        let person = person_of(peer);
        let now = self.now();
        let bytes = random::<8>()?;
        let admission = self.with(|room| {
            let admission =
                room.meta
                    .access
                    .join(&person, token, rights, now, Room::guest_person(bytes))?;
            room.enter(session.clone(), &admission);
            room.save_meta();
            Ok(admission)
        })?;
        backend.narrow_session(session.id(), &admission.role.rights());
        Ok(admission)
    }
}

impl RoomLink for Handle {
    fn workspace(&self, root: &Path) -> Option<(StoreSink, std::path::PathBuf)> {
        let state = self.state_dir.as_ref()?;
        let files = Files::of(state, root);
        let room = files.load(
            self.cfg.retention_ms,
            self.cfg.max_entries,
            (self.cfg.now)(),
        )?;
        let blobs = files.blobs();
        let mut inner = self.lock();
        inner.room = Some(room);
        inner.files = Some(files);
        drop(inner);
        Some((self.sink(), blobs))
    }

    fn guest_sessions(&self) -> Vec<String> {
        self.lock().room.as_ref().map_or_else(Vec::new, |room| {
            room.participants
                .keys()
                .filter(|sid| room.is_guest(sid))
                .cloned()
                .collect()
        })
    }

    fn anonymous_link_live(&self, root: &Path, link: &str) -> bool {
        let token = secret_of(link);
        let now = self.now();
        if let Some(room) = self.lock().room.as_ref() {
            return room.meta.access.anonymous_link_live(token, now);
        }
        // The room is loaded by the first session; the credential is checked before that.
        let Some(state) = self.state_dir.as_ref() else {
            return false;
        };
        Files::of(state, root)
            .load(self.cfg.retention_ms, self.cfg.max_entries, now)
            .is_some_and(|room| room.meta.access.anonymous_link_live(token, now))
    }

    fn gate(
        &self,
        backend: &Backend,
        session: &SessionHandle,
        peer: &str,
        method: &str,
    ) -> Result<(), RpcError> {
        let (exists, joined, guest) = {
            let inner = self.lock();
            match &inner.room {
                Some(r) => (
                    true,
                    r.participants.contains_key(session.id()),
                    r.is_guest(session.id()),
                ),
                None => (false, false, false),
            }
        };
        // The host's notices name devices and runs (`SPEC-0323` behaviour 21a).
        if guest && method == "notice.list" {
            return Err(not_granted(
                "a guest of the room gets no notices of the host",
            ));
        }
        if !exists || joined || method.starts_with("session.") || method == "room.join" {
            return Ok(());
        }
        let rights = backend.session_rights(session.id());
        self.admit(backend, session, None, &rights, peer)
            .map(|_| ())
            .map_err(|e| {
                not_granted(format!("this project has a room: {}", e.message))
                    .hint("join it with a link from an admin (room.join)")
            })
    }

    fn next_group(&self) -> u64 {
        self.with(|room| Ok(room.next_group())).unwrap_or(0)
    }
}

fn parse_role(v: &Value) -> Result<Option<Role>, RpcError> {
    match v.as_str() {
        None => Ok(None),
        Some(name) => Role::parse(name)
            .map(Some)
            .ok_or_else(|| RpcError::new(ErrorCode::Invalid, format!("there is no role {name}"))),
    }
}

/// Sets `role` on every session of `id` and closes them, so they rejoin with it.
fn close_sessions_of(room: &mut Room, id: &str, why: &'static str) {
    let sids: Vec<String> = room
        .participants
        .iter()
        .filter(|(_, p)| p.person.id == id || p.person.id.ends_with(&format!(">{id}")))
        .map(|(sid, _)| sid.clone())
        .collect();
    for sid in sids {
        room.leave(&sid);
        room.out.push(Out::End(sid, why));
    }
}

/// Registers the family, attaches the room to `backend`, starts the pump and loads an existing
/// room once the backend's project is known.
pub fn bind(backend: &Backend, cfg: RoomConfig) -> Handle {
    let rooms: Handle = Arc::new(Rooms {
        cfg,
        state_dir: backend.config().state_dir.clone(),
        inner: Mutex::new(Inner::default()),
        wake: Condvar::new(),
        flushing: Mutex::new(()),
        saved: AtomicU64::new(0),
    });
    backend.attach_room(Arc::new(Arc::clone(&rooms)));
    register(backend, &rooms);
    let (weak, pump) = (backend.downgrade(), Arc::clone(&rooms));
    std::thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            {
                let inner = pump.lock();
                let _ = pump.wake.wait_timeout(inner, pump.cfg.tick);
            }
            let Some(backend) = weak.upgrade() else {
                return;
            };
            pump.flush(&backend);
            if last.elapsed() >= pump.cfg.tick {
                last = Instant::now();
                pump.tick(&backend);
            }
        }
    });
    rooms
}

fn register(backend: &Backend, rooms: &Handle) {
    type F = fn(&Handle, &Backend, &Call) -> Result<Value, RpcError>;
    let reg = |name: &str, audited: bool, f: F| {
        let r = Arc::clone(rooms);
        let h = Arc::new(move |call: &Call| {
            let b = backend_of(call)?;
            f(&r, &b, call)
        });
        if audited {
            backend.register_audited("room", name, Right::View, h);
        } else {
            backend.register("room", name, Right::View, h);
        }
    };
    reg("room.create", true, create);
    reg("room.status", false, status);
    reg("room.join", true, join);
    reg("room.leave", false, leave);
    reg("room.share", true, share);
    reg("room.links", false, links);
    reg("room.revoke_link", true, revoke_link);
    reg("room.set_role", true, set_role);
    reg("room.remove", true, remove);
    reg("room.apply", true, apply);
    reg("room.undo", true, |r, b, c| undo(r, b, c, false));
    reg("room.redo", true, |r, b, c| undo(r, b, c, true));
    reg("room.since", false, since);
    reg("room.history", false, history);
    reg("room.version", true, version);
    reg("room.restore", true, restore);
    reg("room.diff", false, diff);
    reg("room.presence", false, presence);
    reg("room.comment", true, comments::comment);
    reg("room.comments", false, comments::comments);
    reg("room.resolve_comment", true, comments::resolve_comment);
    reg("room.delete_comment", true, comments::delete_comment);
}

fn peer_of(backend: &Backend, call: &Call) -> String {
    backend.session_peer(call.session.id())
}

fn create(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let peer = peer_of(backend, call);
    let person = person_of(&peer);
    if Role::cap(&call.rights) != Some(Role::Admin) {
        return Err(not_granted(
            "creating a room needs a session with every right (the owner's)",
        ));
    }
    let now = rooms.now();
    let id = hex(&random::<16>()?);
    let name = call.params["name"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            call.root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "project".into());
    let files = rooms.make(&call.root)?;
    let blobs = files.blobs();
    {
        let mut inner = rooms.lock();
        if inner.room.is_some() {
            return Err(RpcError::new(
                ErrorCode::Conflict,
                "this project already has a room",
            ));
        }
        let mut access = access::Access::default();
        access.members.insert(
            person.id.clone(),
            access::Member {
                name: person.name.clone(),
                role: Role::Admin,
                since: now,
            },
        );
        let meta = Meta {
            id,
            name,
            created: now,
            next_seq: 0,
            next_group: 0,
            next_comment: 0,
            access,
            comments: Vec::new(),
        };
        let mut room = Room::new(meta, Default::default(), rooms.cfg.max_entries);
        room.save_meta();
        inner.room = Some(room);
        inner.files = Some(files);
    }
    backend.enter_room(rooms.sink(), blobs);
    rooms.admit(backend, &call.session, None, &call.rights, &peer)?;
    rooms.flush(backend);
    rooms.with(|room| Ok(visible_status(room, call.session.id())))
}

/// `room.status` for a participant: only an admin sees the list of members.
fn status(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    rooms.with(|room| Ok(visible_status(room, call.session.id())))
}

fn visible_status(room: &Room, sid: &str) -> Value {
    room.status_for(sid)
}

fn join(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let peer = peer_of(backend, call);
    let token = call.params["link"].as_str().map(secret_of);
    let joined = rooms.with(|room| Ok(room.participants.get(call.session.id()).cloned()))?;
    let admission = match (&joined, token) {
        (Some(p), None) => Admission {
            person: p.person.clone(),
            role: p.role,
            link: p.link.clone(),
            expires: p.expires,
        },
        _ => rooms.admit(backend, &call.session, token, &call.rights, &peer)?,
    };
    let result_role = if let Some(agent) = call.params["agent"].as_str() {
        if agent.is_empty() || agent.len() > 64 || agent.contains(['\n', '>']) {
            return Err(RpcError::new(ErrorCode::Invalid, "`agent` is not a name"));
        }
        if admission.person.kind == Kind::Guest {
            return Err(not_granted("a guest cannot bring an agent"));
        }
        let requested = parse_role(&call.params["role"])?;
        let role = access::agent_role(admission.role, requested);
        let person = Person {
            id: format!("agent:{agent}>{}", admission.person.id),
            name: format!("{agent} ({})", admission.person.name),
            kind: Kind::Agent,
        };
        let now = rooms.now();
        rooms.with(|room| {
            let group = room.next_group();
            let actor = (person.id.clone(), person.name.clone());
            room.append(
                actor,
                group,
                Body::Version {
                    name: format!("before agent {agent}"),
                },
                now,
            );
            room.leave(call.session.id());
            room.enter(
                call.session.clone(),
                &Admission {
                    person: person.clone(),
                    role,
                    link: admission.link.clone(),
                    expires: admission.expires,
                },
            );
            Ok(())
        })?;
        backend.narrow_session(call.session.id(), &role.rights());
        role
    } else {
        admission.role
    };
    rooms.flush(backend);
    let mut out = rooms.with(|room| Ok(visible_status(room, call.session.id())))?;
    out["role"] = json!(result_role.name());
    Ok(out)
}

fn leave(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    rooms.with(|room| {
        room.leave(call.session.id());
        Ok(())
    })?;
    backend.end_session_now(call.session.id(), "left the room");
    Ok(json!({ "left": true }))
}

fn share(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let now = rooms.now();
    let role = parse_role(&call.params["role"])?;
    let audience = match call.params["audience"].as_str() {
        None | Some("organisation") => None,
        Some("anonymous") => Some(Audience::Anonymous),
        Some(other) => {
            return Err(RpcError::new(
                ErrorCode::Invalid,
                format!("there is no audience {other}"),
            ));
        }
    };
    let ttl = call.params["ttl_seconds"]
        .as_u64()
        .map(|s| s.saturating_mul(1000));
    let bytes = random::<40>()?;
    rooms.with(|room| {
        let id = room.need(call.session.id(), Role::Admin)?.person.id.clone();
        let (link, token) = room
            .meta
            .access
            .share(role, audience, ttl, &id, now, bytes)?;
        room.save_meta();
        Ok(json!({
            "id": link.id,
            "role": link.role.name(),
            "audience": link.audience.name(),
            "expires": link.expires,
            "link": format!("ferris-idea://room/{}#{token}", room.meta.id),
        }))
    })
}

fn links(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let now = rooms.now();
    rooms.with(|room| {
        room.need(call.session.id(), Role::Admin)?;
        Ok(json!({ "links": room.links_json(now) }))
    })
}

fn revoke_link(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let id = str_of(&call.params, "id")?;
    let done = rooms.with(|room| {
        room.need(call.session.id(), Role::Admin)?;
        let found = room.meta.access.revoke(id);
        room.save_meta();
        Ok(found)
    })?;
    rooms.tick(backend);
    Ok(json!({ "revoked": done }))
}

fn set_role(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let target = str_of(&call.params, "person")?;
    let role = parse_role(&call.params["role"])?
        .ok_or_else(|| RpcError::new(ErrorCode::Invalid, "`role` is missing"))?;
    rooms.with(|room| {
        room.need(call.session.id(), Role::Admin)?;
        room.meta.access.set_role(target, role)?;
        room.save_meta();
        close_sessions_of(room, target, "role changed");
        Ok(())
    })?;
    rooms.flush(backend);
    Ok(json!({ "person": target, "role": role.name() }))
}

fn remove(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let target = str_of(&call.params, "person")?;
    rooms.with(|room| {
        room.need(call.session.id(), Role::Admin)?;
        room.meta.access.remove(target)?;
        room.save_meta();
        close_sessions_of(room, target, "removed");
        Ok(())
    })?;
    rooms.flush(backend);
    Ok(json!({ "removed": target }))
}

fn apply(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let changes: Vec<SetChange> = serde_json::from_value(call.params["changes"].clone())
        .map_err(|e| RpcError::new(ErrorCode::Invalid, format!("`changes`: {e}")))?;
    if changes.is_empty() {
        return Err(RpcError::new(ErrorCode::Invalid, "`changes` is empty"));
    }
    let group = rooms.with(|room| {
        room.need(call.session.id(), Role::Editor)?;
        Ok(room.next_group())
    })?;
    let revisions = backend.apply_room_set(&call.client, group, &changes)?;
    Ok(json!({ "group": group, "documents": revisions }))
}

fn undo(rooms: &Handle, backend: &Backend, call: &Call, redo: bool) -> Result<Value, RpcError> {
    let id = rooms.with(|room| {
        Ok(room
            .need(call.session.id(), Role::Editor)?
            .person
            .id
            .clone())
    })?;
    let Some((group, changes, step)) = rooms.with(|room| room.plan_undo(&id, redo))? else {
        return Ok(json!({ "group": null, "changes": 0 }));
    };
    let count = changes.len();
    if count > 0
        && let Err(e) = backend.apply_room_set(&call.client, group, &changes)
    {
        rooms.with(|room| {
            room.restore_step(&id, redo, step, group);
            Ok(())
        })?;
        return Err(e);
    }
    Ok(json!({ "group": group, "undid": step, "changes": count }))
}

fn since(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let after = call.params["seq"].as_u64().unwrap_or(0);
    rooms.with(|room| {
        room.participant(call.session.id())?;
        let oldest = room.journal.front().map_or(0, |e| e.seq.saturating_sub(1));
        if after < oldest {
            return Err(RpcError::new(
                ErrorCode::StaleRevision,
                "those changes are no longer in the history",
            ));
        }
        Ok(room.entries_for(call.session.id(), after, None, usize::MAX, true))
    })
}

fn history(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let after = call.params["since"].as_u64().unwrap_or(0);
    let doc = call.params["doc"].as_str();
    let limit = call.params["limit"]
        .as_u64()
        .map_or(MAX_LIST, |n| (n as usize).min(MAX_LIST));
    rooms.with(|room| {
        room.participant(call.session.id())?;
        Ok(room.entries_for(call.session.id(), after, doc, limit, false))
    })
}

fn version(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let name = str_of(&call.params, "name")?.to_owned();
    let now = rooms.now();
    rooms.with(|room| {
        let p = room.need(call.session.id(), Role::Editor)?;
        let actor = (p.person.id.clone(), p.person.name.clone());
        let group = room.next_group();
        let seq = room.append(actor, group, Body::Version { name: name.clone() }, now);
        Ok(json!({ "seq": seq, "name": name }))
    })
}

/// The documents of a `room.restore`: `docs` (1 to [`MAX_RESTORE_DOCS`] paths, each once) or
/// `doc` (`SPEC-0323` v9 behaviour 64).
fn restore_docs(params: &Value) -> Result<Vec<String>, RpcError> {
    let Some(list) = params.get("docs").filter(|v| !v.is_null()) else {
        return Ok(vec![str_of(params, "doc")?.to_owned()]);
    };
    let list = list
        .as_array()
        .filter(|l| !l.is_empty() && l.len() <= MAX_RESTORE_DOCS)
        .ok_or_else(|| invalid(format!("`docs` is a list of 1 to {MAX_RESTORE_DOCS} paths")))?;
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for d in list {
        let d = d
            .as_str()
            .ok_or_else(|| invalid("`docs` holds paths"))?
            .to_owned();
        if !out.contains(&d) {
            out.push(d);
        }
    }
    Ok(out)
}

/// What brings `doc` back to journal position `seq`; `None` when it already reads so.
fn restore_change(
    rooms: &Handle,
    backend: &Backend,
    doc: &str,
    seq: u64,
) -> Result<Option<SetChange>, RpcError> {
    match backend.open_for_room(doc) {
        Ok(info) => {
            let target = rooms.with(|room| room.text_at(doc, &info.text, seq))?;
            if target == info.text {
                return Ok(None);
            }
            Ok(Some(SetChange::Edit {
                path: info.doc,
                base: Some(info.revision),
                ops: TextOp::diff(&info.text, &target).to_ops(),
            }))
        }
        Err(e) if e.code == ErrorCode::NotFound => {
            let blob = rooms.with(|room| {
                Ok(room.journal.iter().rev().find_map(|e| match &e.body {
                    Body::File {
                        change, path, blob, ..
                    } if change == "deleted" && *path == doc => blob.clone(),
                    _ => None,
                }))
            })?;
            let blob = blob.ok_or_else(|| {
                RpcError::new(ErrorCode::NotFound, format!("{doc} is not in the history"))
            })?;
            Ok(Some(SetChange::Create {
                path: doc.to_owned(),
                dir: false,
                text: None,
                blob: Some(blob),
            }))
        }
        Err(e) => Err(e),
    }
}

/// `room.restore`: one file (`doc`) or several (`docs`, v9) back to a journal position, as one
/// change set: one group, one undo step, nothing changes when one of them cannot be restored.
fn restore(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let docs = restore_docs(&call.params)?;
    let seq = call.params["seq"]
        .as_u64()
        .ok_or_else(|| RpcError::new(ErrorCode::Invalid, "`seq` is missing"))?;
    let group = rooms.with(|room| {
        room.need(call.session.id(), Role::Editor)?;
        if seq > room.meta.next_seq {
            return Err(RpcError::new(
                ErrorCode::Invalid,
                "that point has not happened yet",
            ));
        }
        Ok(room.next_group())
    })?;
    let mut changes = Vec::with_capacity(docs.len());
    for doc in &docs {
        if let Some(change) = restore_change(rooms, backend, doc, seq)? {
            changes.push(change);
        }
    }
    if changes.is_empty() {
        return Ok(json!({ "group": null, "changes": 0 }));
    }
    backend.apply_room_set(&call.client, group, &changes)?;
    Ok(json!({ "group": group, "changes": changes.len() }))
}

/// `room.diff` (`SPEC-0323` v9 behaviour 63): the text of `doc` after journal position `from`
/// and after `to`, walked back from its text now. Every participant may read it.
fn diff(rooms: &Handle, backend: &Backend, call: &Call) -> Result<Value, RpcError> {
    let doc = str_of(&call.params, "doc")?.to_owned();
    let position = |name: &str| {
        call.params[name]
            .as_u64()
            .ok_or_else(|| invalid(format!("`{name}` is a journal position")))
    };
    let (from, to) = (position("from")?, position("to")?);
    rooms.with(|room| {
        room.participant(call.session.id())?;
        if from > to {
            return Err(invalid("`from` comes after `to`"));
        }
        if to > room.meta.next_seq {
            return Err(invalid("that point has not happened yet"));
        }
        Ok(())
    })?;
    let info = backend.open_for_room(&doc)?;
    let (before, after) = rooms.with(|room| {
        Ok((
            room.text_at(&info.doc, &info.text, from)?,
            room.text_at(&info.doc, &info.text, to)?,
        ))
    })?;
    if before.len() + after.len() > MAX_DIFF_TEXT {
        return Err(RpcError::new(
            ErrorCode::TooLarge,
            format!(
                "{} is too large to compare here ({} KiB before and after)",
                info.doc,
                (before.len() + after.len()) >> 10
            ),
        ));
    }
    Ok(json!({ "doc": info.doc, "from": from, "to": to, "before": before, "after": after }))
}

/// The longest document path in a presence update.
const MAX_PRESENCE_DOC: usize = 4096;
/// The longest session or person a presence update may follow.
const MAX_PRESENCE_FOLLOWS: usize = 256;

/// The presence of `params` in its fixed shape (`SPEC-0323` v4 behaviour 21): a document path,
/// a caret, a selection of two positions and whom the person follows, each optional. Anything
/// else is refused, so nobody can pass large or nested data on to every participant.
fn presence_of(params: &Value) -> Result<Value, RpcError> {
    let text = |name: &str, max: usize| match &params[name] {
        Value::Null => Ok(Value::Null),
        Value::String(s) if s.len() <= max => Ok(Value::String(s.clone())),
        _ => Err(invalid(format!(
            "`{name}` is a text of at most {max} bytes"
        ))),
    };
    let position = |v: &Value, name: &str| match v {
        Value::Null => Ok(Value::Null),
        Value::Number(n) if n.as_u64().is_some() => Ok(v.clone()),
        _ => Err(invalid(format!("`{name}` is a position"))),
    };
    let selection = match &params["selection"] {
        Value::Null => Value::Null,
        Value::Array(a) if a.len() == 2 && a.iter().all(|v| v.as_u64().is_some()) => {
            params["selection"].clone()
        }
        _ => return Err(invalid("`selection` is two positions")),
    };
    Ok(json!({
        "doc": text("doc", MAX_PRESENCE_DOC)?,
        "caret": position(&params["caret"], "caret")?,
        "selection": selection,
        "follows": text("follows", MAX_PRESENCE_FOLLOWS)?,
    }))
}

fn presence(rooms: &Handle, _b: &Backend, call: &Call) -> Result<Value, RpcError> {
    let update = presence_of(&call.params)?;
    let now = rooms.now();
    rooms.with(|room| {
        let sid = call.session.id();
        {
            let p = room
                .participants
                .get_mut(sid)
                .ok_or_else(|| not_granted("not in this room"))?;
            let sec = now / 1000;
            if p.window.0 == sec {
                p.window.1 += 1;
            } else {
                p.window = (sec, 1);
            }
            if p.window.1 > PRESENCE_PER_SECOND {
                return Ok(json!({ "dropped": true }));
            }
            p.presence = update;
        }
        if let Some((view, public)) = room.views(sid) {
            room.announce("room.presence", &view, &public, Some(sid));
        }
        Ok(json!({ "dropped": false }))
    })
}
