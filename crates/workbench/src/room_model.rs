//! The project room as a window shows it (`SPEC-0323` v5 behaviours 30–36): the other
//! participants with their initials and colours, their carets and selections with a name label
//! that fades, what the window sends of its own place, and following another person. Pure: the
//! window feeds it the room's answers and notifications and the time, and asks it what to draw
//! and where to go; nothing here touches a window or the network.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use serde_json::{Value, json};

/// How long a name label stands fully after a caret moved (behaviour 32), in milliseconds.
pub const LABEL_FULL_MS: u64 = 2500;
/// How long it then fades out.
pub const LABEL_FADE_MS: u64 = 500;
/// The window sends its own place at most this often (behaviour 33): ten a second, below the
/// room's limit of thirty.
pub const PRESENCE_EVERY_MS: u64 = 100;
/// Avatars shown before the rest go into one "+N" (behaviour 31).
pub const AVATARS_SHOWN: usize = 5;
/// The longest name a label or tooltip shows, in characters; longer names are cut with "…".
pub const NAME_SHOWN: usize = 40;
/// How long following waits for the followed document to open before a different active
/// document counts as the user's own move (behaviour 35), in milliseconds.
pub const ARRIVAL_MS: u64 = 5000;

/// The participant colours (behaviour 31): twelve hues that read on light and dark editors,
/// as `0xRRGGBB`.
pub const PALETTE: [u32; 12] = [
    0x3d8bf2, // blue
    0xe5663c, // orange
    0x3aa76d, // green
    0xb663d9, // violet
    0xd9a21b, // amber
    0x1fa6b8, // teal
    0xe0457b, // pink
    0x6b7fe0, // indigo
    0x8fae2b, // olive
    0xc9573a, // brick
    0x2f9e9a, // sea
    0x9a6bd1, // lavender
];

/// One place a person stands in a document: the part of a presence that is drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presence {
    /// The document, relative to the project folder.
    pub doc: Option<String>,
    /// The caret, in characters.
    pub caret: Option<usize>,
    /// The selection, in characters, when there is one.
    pub selection: Option<(usize, usize)>,
    /// The client number of the session this person follows.
    pub follows: Option<String>,
}

impl Presence {
    /// From the `presence` part of a participant; anything malformed counts as absent.
    pub fn from_json(v: &Value) -> Self {
        let pos = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
        let selection = v["selection"].as_array().and_then(|a| match a.as_slice() {
            [a, b] => Some((pos(a)?, pos(b)?)),
            _ => None,
        });
        Self {
            doc: v["doc"].as_str().map(str::to_owned),
            caret: pos(&v["caret"]),
            selection,
            follows: v["follows"].as_str().map(str::to_owned),
        }
    }

    /// The parameters of `room.presence` for this place.
    pub fn to_params(&self) -> Value {
        json!({
            "doc": self.doc,
            "caret": self.caret,
            "selection": self.selection.map(|(a, b)| [a, b]),
            "follows": self.follows,
        })
    }
}

/// Another participant of the room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    /// The session's client number (`cN`): what the room names a participant by in follows.
    pub client: String,
    /// The person's id, absent for what a guest sees.
    pub id: Option<String>,
    /// The name the room gives (for a guest's view a neutral one).
    pub name: String,
    /// `member`, `agent`, `guest` or `device`.
    pub kind: String,
    /// The role in the room.
    pub role: String,
    /// Away: the connection is lost for less than the room's limit.
    pub away: bool,
    /// Index into [`PALETTE`].
    pub color: usize,
    /// Where the person stands.
    pub presence: Presence,
}

impl Participant {
    /// From a participant of `room.status`, `room.joined` or `room.presence`.
    pub fn from_json(v: &Value) -> Option<Self> {
        let client = v["client"].as_str()?.to_owned();
        let id = v["id"].as_str().map(str::to_owned);
        let color = match v["color"].as_u64() {
            Some(n) => (n % PALETTE.len() as u64) as usize,
            None => color_of(id.as_deref().unwrap_or(&client)),
        };
        Some(Self {
            name: v["name"].as_str().unwrap_or(&client).to_owned(),
            kind: v["kind"].as_str().unwrap_or("member").to_owned(),
            role: v["role"].as_str().unwrap_or("viewer").to_owned(),
            away: v["away"].as_bool().unwrap_or(false),
            presence: Presence::from_json(&v["presence"]),
            client,
            id,
            color,
        })
    }

    /// The colour as `0xRRGGBB`.
    pub fn rgb(&self) -> u32 {
        PALETTE[self.color % PALETTE.len()]
    }

    /// The name as labels and tooltips show it.
    pub fn shown_name(&self) -> String {
        shown_name(&self.name)
    }
}

/// A stable colour for a key (a participant id, or a client number when no colour came):
/// FNV-1a over its bytes, so it is the same in every window and run.
pub fn color_of(key: &str) -> usize {
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    (h % PALETTE.len() as u32) as usize
}

/// The name cut to [`NAME_SHOWN`] characters, control characters dropped.
pub fn shown_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() <= NAME_SHOWN {
        return clean.to_owned();
    }
    let mut cut: String = clean.chars().take(NAME_SHOWN - 1).collect();
    cut.push('…');
    cut
}

/// The initials on an avatar (behaviour 31): the first letters of the first two words, or the
/// first two letters of a single word, in capitals; "?" without letters or digits.
pub fn initials(name: &str) -> String {
    let words: Vec<Vec<char>> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect();
    let picked: Vec<char> = match words.as_slice() {
        [] => return "?".to_owned(),
        [one] => one.iter().take(2).copied().collect(),
        [first, second, ..] => vec![first[0], second[0]],
    };
    picked.iter().flat_map(|c| c.to_uppercase()).collect()
}

/// One avatar of the toolbar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avatar {
    /// The participant's client number.
    pub client: String,
    /// Up to two capitals.
    pub initials: String,
    /// The colour as `0xRRGGBB`.
    pub rgb: u32,
    /// Drawn at half strength with a grey dot.
    pub away: bool,
    /// The window follows this person.
    pub followed: bool,
    /// The tooltip: title and lines below it.
    pub title: String,
    /// The tooltip's lines below the title.
    pub details: Vec<String>,
}

/// What the toolbar shows of the room.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Avatars {
    /// The avatars drawn, at most [`AVATARS_SHOWN`].
    pub shown: Vec<Avatar>,
    /// Names of the participants beyond [`AVATARS_SHOWN`], for the "+N" tooltip.
    pub more: Vec<String>,
}

/// A caret of another person in one editor (behaviour 32).
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteCaret {
    /// The participant's client number.
    pub client: String,
    /// The name on the label.
    pub name: String,
    /// The colour as `0xRRGGBB`.
    pub rgb: u32,
    /// In characters.
    pub caret: usize,
    /// The selection, in characters, ordered and not empty.
    pub selection: Option<Range<usize>>,
    /// Opacity of the name label, 0 when it is not shown.
    pub label: f32,
    /// Drawn at half strength.
    pub away: bool,
}

/// The following state (behaviour 34).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    /// Nobody is followed.
    Off,
    /// The window follows `client`.
    On {
        /// The client number of the person followed.
        client: String,
        /// The document the window was last sent to and when, while it is on its way.
        arriving: Option<(String, u64)>,
    },
}

/// Where following takes the window now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Go {
    /// The document, relative to the project folder; checked to stay inside it.
    pub doc: String,
    /// The caret, in characters.
    pub caret: usize,
}

/// What a change of the room did that the window tells the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// Following this person started.
    Following(String),
    /// Following this person stopped by the user's own move or choice.
    Stopped(String),
    /// The person followed left.
    Left(String),
}

impl Note {
    /// The status bar text (behaviours 34, 35).
    pub fn text(&self) -> String {
        match self {
            Note::Following(n) => format!("Following {n}. Type or move the caret to stop."),
            Note::Stopped(n) => format!("Stopped following {n}"),
            Note::Left(n) => format!("{n} left the room; stopped following"),
        }
    }
}

/// Why following cannot start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowError {
    /// No participant with this client number.
    Unknown(String),
    /// The window's own session.
    Yourself,
}

impl FollowError {
    /// The reason as the window and the tool give it.
    pub fn text(&self) -> String {
        match self {
            FollowError::Unknown(c) => format!("no participant {c} in the room"),
            FollowError::Yourself => "a window cannot follow itself".to_owned(),
        }
    }
}

/// The room of one window.
#[derive(Debug, Clone)]
pub struct RoomView {
    /// The window's own client number: never listed, never followed.
    own: String,
    others: BTreeMap<String, Participant>,
    /// When each participant's caret or selection last changed (for the label).
    moved_at: HashMap<String, u64>,
    follow: Follow,
    /// What the window last sent of its own place, and when.
    sent: Option<(Presence, u64)>,
    /// The room's name (`SPEC-0323` v6 behaviour 43).
    name: String,
    /// The window's own role in the room.
    own_role: Option<String>,
}

impl RoomView {
    /// A room seen by the session `own`, from `room.status`, at `now` (ms).
    pub fn from_status(own: &str, status: &Value, now: u64) -> Self {
        let mut v = Self {
            own: own.to_owned(),
            others: BTreeMap::new(),
            moved_at: HashMap::new(),
            follow: Follow::Off,
            sent: None,
            name: String::new(),
            own_role: None,
        };
        v.reset(own, status, now);
        v
    }

    /// The room's state again (after a reconnection): participants replaced, following kept
    /// while that person is still there.
    pub fn reset(&mut self, own: &str, status: &Value, now: u64) -> Option<Note> {
        self.own = own.to_owned();
        self.name = status["name"]
            .as_str()
            .or(status["id"].as_str())
            .unwrap_or("")
            .to_owned();
        self.own_role = status["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["client"] == own)
            .and_then(|p| p["role"].as_str())
            .or(status["role"].as_str())
            .map(str::to_owned);
        let fresh: BTreeMap<String, Participant> = status["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Participant::from_json)
            .filter(|p| p.client != self.own)
            .map(|p| (p.client.clone(), p))
            .collect();
        for (c, p) in &fresh {
            if self
                .others
                .get(c)
                .map(|o| (&o.presence.doc, o.presence.caret, o.presence.selection))
                != Some((&p.presence.doc, p.presence.caret, p.presence.selection))
            {
                self.moved_at.insert(c.clone(), now);
            }
        }
        let gone = match &self.follow {
            Follow::On { client, .. } if !fresh.contains_key(client) => Some(self.name_of(client)),
            _ => None,
        };
        self.others = fresh;
        self.moved_at.retain(|c, _| self.others.contains_key(c));
        gone.map(|n| {
            self.follow = Follow::Off;
            Note::Left(n)
        })
    }

    /// The window's own client number.
    pub fn own(&self) -> &str {
        &self.own
    }

    /// The room's name.
    pub fn room_name(&self) -> &str {
        &self.name
    }

    /// The window's own role in the room, when the room said it.
    pub fn own_role(&self) -> Option<&str> {
        self.own_role.as_deref()
    }

    /// The other participants, by client number.
    pub fn others(&self) -> impl Iterator<Item = &Participant> {
        self.others.values()
    }

    /// The participant with this client number.
    pub fn participant(&self, client: &str) -> Option<&Participant> {
        self.others.get(client)
    }

    /// The following state.
    pub fn follow(&self) -> &Follow {
        &self.follow
    }

    /// The client number of the person followed.
    pub fn following(&self) -> Option<&str> {
        match &self.follow {
            Follow::On { client, .. } => Some(client),
            Follow::Off => None,
        }
    }

    fn name_of(&self, client: &str) -> String {
        self.others
            .get(client)
            .map_or_else(|| client.to_owned(), Participant::shown_name)
    }

    /// A notification of the room (`room.joined`, `room.presence`, `room.left`). Returns a note
    /// for the user and where following goes now.
    pub fn event(&mut self, method: &str, params: &Value, now: u64) -> (Option<Note>, Option<Go>) {
        match method {
            "room.joined" | "room.presence" => {
                let Some(p) = Participant::from_json(params) else {
                    return (None, None);
                };
                if p.client == self.own {
                    if method == "room.joined" {
                        self.own_role = Some(p.role);
                    }
                    return (None, None);
                }
                let moved = self.others.get(&p.client).is_none_or(|o| {
                    (&o.presence.doc, o.presence.caret, o.presence.selection)
                        != (&p.presence.doc, p.presence.caret, p.presence.selection)
                });
                if moved {
                    self.moved_at.insert(p.client.clone(), now);
                }
                let client = p.client.clone();
                self.others.insert(client.clone(), p);
                let go = (moved && self.following() == Some(client.as_str()))
                    .then(|| self.go_to(&client, now))
                    .flatten();
                (None, go)
            }
            "room.left" => {
                let Some(client) = params["client"].as_str() else {
                    return (None, None);
                };
                let name = self.name_of(client);
                self.others.remove(client);
                self.moved_at.remove(client);
                if self.following() == Some(client) {
                    self.follow = Follow::Off;
                    return (Some(Note::Left(name)), None);
                }
                (None, None)
            }
            _ => (None, None),
        }
    }

    /// Where the person `client` stands, as a place to go; remembered as arriving.
    fn go_to(&mut self, client: &str, now: u64) -> Option<Go> {
        let p = self.others.get(client)?;
        let doc = p.presence.doc.clone()?;
        // A path from another person is opened only inside the project (behaviour 34).
        if !safe_doc(&doc) {
            return None;
        }
        let caret = p.presence.caret.unwrap_or(0);
        if let Follow::On { arriving, .. } = &mut self.follow {
            *arriving = Some((doc.clone(), now));
        }
        Some(Go { doc, caret })
    }

    /// A click on the avatar of `client` (behaviour 34): starts following, or stops when the
    /// window already follows that person.
    pub fn toggle_follow(
        &mut self,
        client: &str,
        now: u64,
    ) -> Result<(Note, Option<Go>), FollowError> {
        if self.following() == Some(client) {
            return Ok((
                self.stop().unwrap_or(Note::Stopped(client.to_owned())),
                None,
            ));
        }
        self.start_follow(client, now)
    }

    /// Starts following `client` (a click, or `room_follow`).
    pub fn start_follow(
        &mut self,
        client: &str,
        now: u64,
    ) -> Result<(Note, Option<Go>), FollowError> {
        if client == self.own {
            return Err(FollowError::Yourself);
        }
        if !self.others.contains_key(client) {
            return Err(FollowError::Unknown(client.to_owned()));
        }
        self.follow = Follow::On {
            client: client.to_owned(),
            arriving: None,
        };
        let go = self.go_to(client, now);
        Ok((Note::Following(self.name_of(client)), go))
    }

    /// Stops following; the note names whom.
    pub fn stop(&mut self) -> Option<Note> {
        let client = self.following()?.to_owned();
        self.follow = Follow::Off;
        Some(Note::Stopped(self.name_of(&client)))
    }

    /// The window's own activity at `now` (behaviour 35): `input` when the user typed or pressed
    /// in an editor since the last look, `active` the document in front. Stops following when
    /// the user moved on their own.
    pub fn own_activity(&mut self, input: bool, active: Option<&str>, now: u64) -> Option<Note> {
        let Follow::On { client, arriving } = &mut self.follow else {
            return None;
        };
        let client = client.clone();
        if input {
            return self.stop();
        }
        // The followed document is on its way: another one in front is not the user's move yet.
        if let Some((doc, since)) = arriving.clone() {
            if active == Some(doc.as_str()) {
                *arriving = None;
            } else if now.saturating_sub(since) < ARRIVAL_MS {
                return None;
            } else {
                *arriving = None;
            }
        }
        let theirs = self
            .others
            .get(&client)
            .and_then(|p| p.presence.doc.clone());
        match (theirs, active) {
            (Some(t), Some(a)) if t != a => self.stop(),
            (Some(_), None) => self.stop(),
            _ => None,
        }
    }

    /// The avatars of the toolbar (behaviour 31), by name, then client number.
    pub fn avatars(&self) -> Avatars {
        let mut list: Vec<&Participant> = self.others.values().collect();
        list.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| client_order(&a.client).cmp(&client_order(&b.client)))
        });
        let following = self.following();
        let avatar = |p: &Participant| {
            let followed = following == Some(p.client.as_str());
            let mut details = vec![format!("Role: {}", p.role)];
            if p.away {
                details.push("Away".to_owned());
            }
            if p.presence.follows.as_deref() == Some(self.own.as_str()) {
                details.push("Following you".to_owned());
            }
            if let Some(doc) = &p.presence.doc {
                details.push(format!("In {}", shown_name(doc)));
            }
            details.push(if followed {
                "Click to stop following".to_owned()
            } else {
                "Click to follow".to_owned()
            });
            Avatar {
                client: p.client.clone(),
                initials: initials(&p.name),
                rgb: p.rgb(),
                away: p.away,
                followed,
                title: p.shown_name(),
                details,
            }
        };
        Avatars {
            shown: list.iter().take(AVATARS_SHOWN).map(|p| avatar(p)).collect(),
            more: list
                .iter()
                .skip(AVATARS_SHOWN)
                .map(|p| p.shown_name())
                .collect(),
        }
    }

    /// The other people's carets in `doc` at `now`, each clamped to a text of `len` characters
    /// (behaviour 32).
    pub fn carets_in(&self, doc: &str, len: usize, now: u64) -> Vec<RemoteCaret> {
        self.others
            .values()
            .filter(|p| p.presence.doc.as_deref() == Some(doc))
            .filter_map(|p| {
                let caret = p.presence.caret?.min(len);
                let selection = p.presence.selection.and_then(|(a, b)| {
                    let (a, b) = (a.min(b).min(len), a.max(b).min(len));
                    (a < b).then_some(a..b)
                });
                let label = self
                    .moved_at
                    .get(&p.client)
                    .map_or(0.0, |at| label_alpha(now.saturating_sub(*at)));
                Some(RemoteCaret {
                    client: p.client.clone(),
                    name: p.shown_name(),
                    rgb: p.rgb(),
                    caret,
                    selection,
                    label,
                    away: p.away,
                })
            })
            .collect()
    }

    /// Whether a label still fades at `now`: the window redraws until none does.
    pub fn labels_changing(&self, now: u64) -> bool {
        self.moved_at
            .values()
            .any(|at| now.saturating_sub(*at) <= LABEL_FULL_MS + LABEL_FADE_MS)
    }

    /// The parameters of `room.presence` to send now for the window's own `place`, if it changed
    /// and the last one went out long enough ago (behaviour 33).
    pub fn presence_due(&mut self, mut place: Presence, now: u64) -> Option<Value> {
        place.follows = self.following().map(str::to_owned);
        if let Some((last, at)) = &self.sent
            && (*last == place || now.saturating_sub(*at) < PRESENCE_EVERY_MS)
        {
            return None;
        }
        let params = place.to_params();
        self.sent = Some((place, now));
        Some(params)
    }

    /// What `room_presence` answers for this window (behaviour 36).
    pub fn to_json(&self) -> Value {
        let people: Vec<Value> = self
            .avatars_all()
            .iter()
            .map(|p| {
                json!({
                    "client": p.client,
                    "name": p.name,
                    "kind": p.kind,
                    "role": p.role,
                    "away": p.away,
                    "color": format!("#{:06x}", p.rgb()),
                    "initials": initials(&p.name),
                    "doc": p.presence.doc,
                    "caret": p.presence.caret,
                    "selection": p.presence.selection.map(|(a, b)| [a, b]),
                    "follows": p.presence.follows,
                })
            })
            .collect();
        json!({
            "you": self.own,
            "participants": people,
            "following": self.following(),
        })
    }

    fn avatars_all(&self) -> Vec<&Participant> {
        let mut list: Vec<&Participant> = self.others.values().collect();
        list.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| client_order(&a.client).cmp(&client_order(&b.client)))
        });
        list
    }
}

/// Client numbers in their numeric order (`c2` before `c10`).
fn client_order(client: &str) -> (u64, String) {
    let n = client
        .strip_prefix('c')
        .and_then(|n| n.parse().ok())
        .unwrap_or(u64::MAX);
    (n, client.to_owned())
}

/// The opacity of a label `age` milliseconds after the caret moved.
pub fn label_alpha(age: u64) -> f32 {
    if age <= LABEL_FULL_MS {
        1.0
    } else if age >= LABEL_FULL_MS + LABEL_FADE_MS {
        0.0
    } else {
        1.0 - (age - LABEL_FULL_MS) as f32 / LABEL_FADE_MS as f32
    }
}

/// Whether a document path from the room stays inside the project: relative, no `..`, no
/// drive or root, no control characters.
pub fn safe_doc(doc: &str) -> bool {
    !doc.is_empty()
        && !doc.starts_with('/')
        && !doc.starts_with('\\')
        && !doc.contains(':')
        && !doc.chars().any(char::is_control)
        && doc
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(client: &str, id: &str, name: &str, color: u64, presence: Value) -> Value {
        json!({
            "client": client, "id": id, "name": name, "kind": "member", "role": "editor",
            "color": color, "away": false, "presence": presence,
        })
    }

    fn status() -> Value {
        json!({"participants": [
            person("c1", "ssh:owner", "owner", 3, json!({})),
            person("c2", "device:bob", "Bob Stone", 7, json!({"doc": "a.txt", "caret": 4})),
            person("c3", "device:eve", "eve", 1, json!({"doc": "b.txt", "caret": 1, "selection": [1, 3]})),
        ]})
    }

    #[test]
    fn initials_take_two_words_or_two_letters() {
        assert_eq!(initials("Bob Stone"), "BS");
        assert_eq!(initials("eve"), "EV");
        assert_eq!(initials("anna-lena maier"), "AL");
        assert_eq!(initials("Guest 4821"), "G4");
        assert_eq!(initials("x"), "X");
        assert_eq!(initials("  --  "), "?");
        assert_eq!(initials("élodie"), "ÉL");
    }

    #[test]
    fn colours_are_stable_and_from_the_room() {
        let p = Participant::from_json(&person("c2", "device:bob", "Bob", 7, json!({}))).unwrap();
        assert_eq!(p.rgb(), PALETTE[7]);
        // Without a colour: the same for the same id, always inside the palette.
        let mut v = person("c9", "device:zed", "Zed", 0, json!({}));
        v.as_object_mut().unwrap().remove("color");
        let a = Participant::from_json(&v).unwrap();
        let b = Participant::from_json(&v).unwrap();
        assert_eq!(a.color, b.color);
        assert_eq!(a.color, color_of("device:zed"));
        assert!(a.color < PALETTE.len());
        let distinct: std::collections::HashSet<u32> = PALETTE.iter().copied().collect();
        assert_eq!(distinct.len(), PALETTE.len());
    }

    #[test]
    fn the_own_session_is_never_listed() {
        let room = RoomView::from_status("c1", &status(), 0);
        let clients: Vec<&str> = room.others().map(|p| p.client.as_str()).collect();
        assert_eq!(clients, ["c2", "c3"]);
        let mut room = room;
        let own = person("c1", "ssh:owner", "owner", 3, json!({"doc": "a.txt"}));
        room.event("room.presence", &own, 5);
        assert_eq!(room.others().count(), 2);
    }

    #[test]
    fn avatars_are_sorted_marked_and_overflow() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        let a = room.avatars();
        let titles: Vec<&str> = a.shown.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Bob Stone", "eve"]);
        assert_eq!(a.shown[0].initials, "BS");
        assert_eq!(a.shown[0].rgb, PALETTE[7]);
        assert_eq!(
            a.shown[0].details,
            ["Role: editor", "In a.txt", "Click to follow"]
        );
        // Away, and following the window.
        let mut away = person("c3", "device:eve", "eve", 1, json!({"follows": "c1"}));
        away["away"] = json!(true);
        room.event("room.presence", &away, 10);
        let eve = room.avatars().shown[1].clone();
        assert!(eve.away);
        assert_eq!(
            eve.details,
            ["Role: editor", "Away", "Following you", "Click to follow"]
        );
        // More than five: the rest by name in "+N".
        for i in 4..10 {
            room.event(
                "room.joined",
                &person(
                    &format!("c{i}"),
                    &format!("device:p{i}"),
                    &format!("p{i}"),
                    0,
                    json!({}),
                ),
                10,
            );
        }
        let a = room.avatars();
        assert_eq!(a.shown.len(), AVATARS_SHOWN);
        assert_eq!(a.more, ["p7", "p8", "p9"]);
    }

    #[test]
    fn long_and_odd_names_are_cut_for_display() {
        let long = "a".repeat(80);
        assert_eq!(shown_name(&long).chars().count(), NAME_SHOWN);
        assert!(shown_name(&long).ends_with('…'));
        assert_eq!(shown_name("bob\u{7}\n"), "bob");
    }

    #[test]
    fn carets_are_per_document_clamped_and_labelled_while_moving() {
        let mut room = RoomView::from_status("c1", &status(), 1000);
        let a = room.carets_in("a.txt", 100, 1000);
        assert_eq!(a.len(), 1);
        assert_eq!(
            (a[0].client.as_str(), a[0].caret, a[0].label),
            ("c2", 4, 1.0)
        );
        let b = room.carets_in("b.txt", 2, 1000);
        assert_eq!(b[0].caret, 1);
        assert_eq!(b[0].selection, Some(1..2), "clamped to the text");
        // The label fades after it stood.
        assert_eq!(
            room.carets_in("a.txt", 100, 1000 + LABEL_FULL_MS)[0].label,
            1.0
        );
        let half = room.carets_in("a.txt", 100, 1000 + LABEL_FULL_MS + LABEL_FADE_MS / 2)[0].label;
        assert!((half - 0.5).abs() < 0.01, "{half}");
        assert_eq!(room.carets_in("a.txt", 100, 5000)[0].label, 0.0);
        assert!(!room.labels_changing(5000));
        // A move brings it back; a repeat of the same place does not.
        let moved = person(
            "c2",
            "device:bob",
            "Bob Stone",
            7,
            json!({"doc": "a.txt", "caret": 9}),
        );
        room.event("room.presence", &moved, 6000);
        assert_eq!(room.carets_in("a.txt", 100, 6000)[0].label, 1.0);
        assert!(room.labels_changing(6000));
        room.event("room.presence", &moved, 9000);
        assert_eq!(room.carets_in("a.txt", 100, 9000)[0].label, 0.0);
        // Gone: no caret.
        room.event("room.left", &json!({"client": "c2"}), 9100);
        assert!(room.carets_in("a.txt", 100, 9100).is_empty());
    }

    #[test]
    fn following_goes_along_and_stops_on_own_moves() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        let (note, go) = room.toggle_follow("c2", 0).unwrap();
        assert_eq!(note, Note::Following("Bob Stone".into()));
        assert_eq!(
            go,
            Some(Go {
                doc: "a.txt".into(),
                caret: 4
            })
        );
        // On its way: another document in front is not a move of the user.
        assert_eq!(room.own_activity(false, Some("z.txt"), 10), None);
        assert_eq!(room.own_activity(false, Some("a.txt"), 20), None);
        // Their next move takes the window along.
        let moved = person(
            "c2",
            "device:bob",
            "Bob Stone",
            7,
            json!({"doc": "b.txt", "caret": 2}),
        );
        let (_, go) = room.event("room.presence", &moved, 30);
        assert_eq!(
            go,
            Some(Go {
                doc: "b.txt".into(),
                caret: 2
            })
        );
        assert_eq!(room.own_activity(false, Some("b.txt"), 40), None);
        assert_eq!(room.following(), Some("c2"));
        // The window's own presence names whom it follows.
        let sent = room.presence_due(Presence::default(), 50).unwrap();
        assert_eq!(sent["follows"], "c2");
        // Typing stops it.
        assert_eq!(
            room.own_activity(true, Some("b.txt"), 60),
            Some(Note::Stopped("Bob Stone".into()))
        );
        assert_eq!(room.following(), None);
        let (_, go) = room.event("room.presence", &moved, 70);
        assert_eq!(go, None, "not following any more");
    }

    #[test]
    fn another_document_in_front_stops_following_once_arrived() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        room.start_follow("c2", 0).unwrap();
        assert_eq!(room.own_activity(false, Some("a.txt"), 10), None);
        assert_eq!(
            room.own_activity(false, Some("other.txt"), 20)
                .map(|n| n.text()),
            Some("Stopped following Bob Stone".to_owned())
        );
        // A document that never arrives: after the wait another one counts as a move.
        let mut room = RoomView::from_status("c1", &status(), 0);
        room.start_follow("c2", 0).unwrap();
        assert_eq!(
            room.own_activity(false, Some("z.txt"), ARRIVAL_MS - 1),
            None
        );
        assert!(
            room.own_activity(false, Some("z.txt"), ARRIVAL_MS)
                .is_some()
        );
    }

    #[test]
    fn a_click_on_the_followed_avatar_stops_and_leaving_ends_it() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        room.toggle_follow("c3", 0).unwrap();
        assert!(room.avatars().shown[1].followed);
        assert_eq!(
            room.avatars().shown[1].details.last().unwrap(),
            "Click to stop following"
        );
        let (note, go) = room.toggle_follow("c3", 1).unwrap();
        assert_eq!((note, go), (Note::Stopped("eve".into()), None));
        room.toggle_follow("c3", 2).unwrap();
        let (note, _) = room.event("room.left", &json!({"client": "c3"}), 3);
        assert_eq!(note.unwrap().text(), "eve left the room; stopped following");
        assert_eq!(room.following(), None);
        // Away does not end it.
        room.toggle_follow("c2", 4).unwrap();
        let mut away = person(
            "c2",
            "device:bob",
            "Bob Stone",
            7,
            json!({"doc": "a.txt", "caret": 4}),
        );
        away["away"] = json!(true);
        room.event("room.presence", &away, 5);
        assert_eq!(room.following(), Some("c2"));
        // A new status without that person ends it too.
        let note = room.reset("c1", &json!({"participants": []}), 6);
        assert_eq!(note, Some(Note::Left("Bob Stone".into())));
    }

    #[test]
    fn following_refuses_oneself_and_strangers_and_never_leaves_the_project() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        assert_eq!(room.start_follow("c1", 0), Err(FollowError::Yourself));
        assert_eq!(
            room.start_follow("c42", 0),
            Err(FollowError::Unknown("c42".into()))
        );
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../../x",
            "C:\\x",
            "a//b",
            "./a",
            "",
        ] {
            let p = person(
                "c2",
                "device:bob",
                "Bob",
                7,
                json!({"doc": bad, "caret": 1}),
            );
            room.event("room.presence", &p, 1);
            let (_, go) = room.start_follow("c2", 1).unwrap();
            assert_eq!(go, None, "{bad}");
        }
        assert!(safe_doc("src/main.rs"));
        // Following someone without a document: on, nothing to open.
        let p = person("c2", "device:bob", "Bob", 7, json!({}));
        room.event("room.presence", &p, 2);
        let (_, go) = room.start_follow("c2", 2).unwrap();
        assert_eq!(go, None);
        assert_eq!(room.own_activity(false, Some("a.txt"), 3), None);
    }

    #[test]
    fn own_presence_goes_out_on_change_at_most_ten_a_second() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        let at = |caret| Presence {
            doc: Some("a.txt".into()),
            caret: Some(caret),
            selection: None,
            follows: None,
        };
        let first = room.presence_due(at(1), 0).unwrap();
        assert_eq!(
            first,
            json!({"doc": "a.txt", "caret": 1, "selection": null, "follows": null})
        );
        assert_eq!(room.presence_due(at(1), 500), None, "unchanged");
        assert_eq!(
            room.presence_due(at(2), 550),
            Some(json!({"doc": "a.txt", "caret": 2, "selection": null, "follows": null}))
        );
        assert_eq!(room.presence_due(at(3), 600), None, "too soon");
        assert!(room.presence_due(at(3), 650).is_some());
        let mut sel = at(5);
        sel.selection = Some((2, 5));
        assert_eq!(
            room.presence_due(sel, 800).unwrap()["selection"],
            json!([2, 5])
        );
        let mut count = 0;
        for t in 0..1000u64 {
            if room.presence_due(at(t as usize + 10), 1000 + t).is_some() {
                count += 1;
            }
        }
        assert!(count <= 10, "{count}");
    }

    #[test]
    fn the_agents_view_lists_what_the_window_shows() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        room.start_follow("c3", 0).unwrap();
        let v = room.to_json();
        assert_eq!(v["you"], "c1");
        assert_eq!(v["following"], "c3");
        assert_eq!(v["participants"][0]["client"], "c2");
        assert_eq!(v["participants"][0]["initials"], "BS");
        assert_eq!(
            v["participants"][0]["color"],
            format!("#{:06x}", PALETTE[7])
        );
        assert_eq!(v["participants"][1]["selection"], json!([1, 3]));
    }

    #[test]
    fn malformed_notifications_change_nothing() {
        let mut room = RoomView::from_status("c1", &status(), 0);
        room.event("room.presence", &json!({"name": "no client"}), 1);
        room.event("room.left", &json!({}), 1);
        room.event("room.other", &json!({"client": "c2"}), 1);
        let p = person(
            "c2",
            "device:bob",
            "Bob Stone",
            7,
            json!({"doc": 5, "caret": -1, "selection": [1]}),
        );
        room.event("room.presence", &p, 2);
        let bob = room.participant("c2").unwrap();
        assert_eq!(bob.presence, Presence::default());
        assert_eq!(room.others().count(), 2);
    }
}
