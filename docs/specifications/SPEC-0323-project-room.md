# SPEC-0323: Project room on the server (first slice of M6.15)

- **Version:** 9 · **Date:** 2026-11-19 (v9, numbered after v7 of #1072 and v8 on main: the
  room's history in the window: a Room History dialog from File, Find Action and the Share dialog
  listing changes and editing phases newest first with author, time, files and a summary, the
  diff of a row, and Restore this version for editors and above with an inline confirmation, as
  one new change of all its files; the room gains `room.diff` and `room.restore` with `docs`;
  the MCP tool `room_diff` and `docs` for `room_restore`; behaviours 61–66, AK-36 to AK-40);
  2026-11-19 (v8, numbered after v6 of #1065 and v7 of #1072: comments
  in the editor window: a mark in the gutter on lines with comments, with the count when there
  are several, a thread popover with author, time, text and resolved state, Add Comment on the
  caret's line from the editor's menu and Find Action, replies, resolving and reopening, deleting
  one's own, marks that follow their lines, plain text only, the roles of the room; the room gains
  replies, reopening, `room.delete_comment` and the notification `room.comments_changed`; the MCP
  tool `room_comment`; behaviours 53–60, AK-31 to AK-35); 2026-11-19 (v7: joining by link from the window: a link that carries
  the host, its port, the project folder, the SHA-256 fingerprint of the host key, the room, the
  role and the end besides its secret; File › Join Project by Link… shows all of it before anything
  connects, checks the fingerprint against the user's own known hosts, refuses a different key as a
  possible attack, explains how to add an unknown key instead of accepting it, and opens the remote
  project window in the room; the MCP tool `room_join_link`; behaviours 44–52, AK-26 to AK-30);
  2026-11-19 (v6: the second part in the window: a Share dialog that
  starts the room, makes expiring links with a role and copies them only on a click, lists and
  revokes links, lists the people with their roles and lets an admin remove one, and a room widget
  in the status bar; `room_presence` also says what the widget shows; behaviours 37–43, AK-22 to
  AK-25); 2026-11-19 (v5: the first part in the window: the other
  participants as avatars in the main toolbar, their carets and selections in the editor with a
  fading name label, following a person, the window's own place sent to the room, and the MCP tools
  `room_presence` and `room_follow`, behaviours 30–36, AK-17 to AK-21); 2026-11-19 (v4: going away, coming back and returning after being
  shown as gone reach the others as notifications, behaviour 10, AK-6; presence has a fixed shape
  and size, behaviour 21, AK-16); 2026-11-12 (v3: the merged text is written back only over the file
  just read, behaviour 5, AK-4); 2026-11-10 (v2, Project Owner's decision of 2026-11-09 evening:
  guests see no participant ids and nothing else identifying, behaviour 21a, AK-15; "organisation"
  stays the server's SSH users and paired devices, behaviour 18)
- **Sources:** [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M6.15 and the
  Project Owner's decisions of 2026-11-07 and 2026-11-09 (`GEN`), [`ADR-0025`](../adr/0025-network-security-for-remote-connections.md),
  [`ADR-0026`](../adr/0026-session-protocol-window-and-serve.md), SPEC-0317, SPEC-0318, SPEC-0321 (`GEN`);
  v7 also SPEC-0121 and IMP-0044 (host keys stay with the system OpenSSH), SPEC-0053, SPEC-0282,
  [`ADR-0021`](../adr/0021-ferris-idea-file-formats.md); the `SHA256:` fingerprint form and the
  `known_hosts` format of OpenSSH (`ssh-keygen(1)`, `sshd(8)`), RFC 3986 for the link (`STD`);
  operational transformation and selective undo as in the published Jupiter and GOT work (`GEN`).
  The improvements over the reference come from public documentation and reports already recorded in
  `REMOTE_AND_MOBILE.md` (`DOC`, `GEN`).
- **Feature matrix rows:** F-089
- **Status of the comparison:** the roadmap asks for a side-by-side with the reference's Code With
  Me first. **It is still outstanding**: this slice was written without screen control and without
  touching the reference; nothing here comes from observing it. The main session does the
  comparison; what it finds becomes a new version of this spec. The window part of v5 follows the
  general arrangement of shared editing in IDEs (people as round avatars at the right of the main
  toolbar, carets in each person's colour, a follow mode; `GEN`, ADR-0009) in our own words and
  look; its sizes, colours and timings are our own choice until the comparison.

## Purpose

A project on `ferris-idea serve` can have one shared room. Everyone who has a right to the room works
on the same state: typing, refactorings, file operations, changes from outside and agents all pass
through one ordering authority, no conflict question ever appears, undo takes back only one's own
work, a short network loss goes unnoticed, and a server-side history records who did what. Access
comes from a role, and a link hands out a role.

## Behaviour

### One ordering authority

1. A project has at most one room, kept per project folder below the backend's state folder. It
   has a random stable id and survives the end of every session and a restart of the backend:
   members, links (as hashes only), comments, history. [GEN]
2. The backend's document store is the only authority for text (`ADR-0026` §5). A room adds no
   second copy: it records every change the store applies, in the store's order, as one journal
   with a room-wide sequence number. Typing keeps using `doc.edit`, so the existing window half
   works unchanged. [GEN]
3. These changes get a journal entry with author and time: `doc.edit`, edits the backend makes on
   someone's behalf, `room.apply` change sets, file operations (`fs.write`, `fs.create`, `fs.rename`,
   `fs.delete` and those in a change set) and changes from outside noticed by the file watcher
   (terminal, `git pull`, build tools). [GEN]
4. `room.apply` is one atomic change set: text edits of several files and file operations
   (create, rename, delete). All are validated first; if one is invalid nothing changes. A failure
   of the file system midway undoes what was already done. The whole set is one journal group and
   one undo step. A text edit carries the revision it was computed on and is transformed against
   later edits, so a concurrent keystroke is never overwritten. [GEN]
5. With a room, a change from outside to an open document never asks anyone. The backend merges it
   three-way: the difference between the text it last synchronised with the disk and the new file
   is transformed over the edits since then and applied as an edit with the author `disk`. The
   merged result is written back at once when unsaved edits were involved, but only while the file
   still holds what was read (v3): if a program is still writing it, nothing is written and the next
   change merges again, so the merge never replaces what that program writes. The watcher reports a
   file only once it stands still (SPEC-0026 v3). A file that is no longer
   UTF-8 text keeps the document's text and is not overwritten by it until it is text again. [GEN]
6. A room saves automatically about once a second; there is no dirty state to decide about. A file
   deleted from outside leaves its document open in memory and is not recreated by the automatic
   save. Renaming a file or folder moves its open documents; a late edit with the old document id
   still arrives. [GEN]
7. Without a room nothing changes for a project: the behaviour of SPEC-0317, including its window
   decision for changes on disk, stays. [GEN]

### Undo per person

8. Undo and redo (`room.undo`, `room.redo`) apply to a person's own groups, newest first. A person's
   typing in one document within a second is one group. Text is undone by transforming the inverse
   of the original edit over everything that followed, so other people's later work stays. [GEN]
9. Undoing a file operation applies its opposite: a rename back, a created file removed, a deleted
   file created again with its contents from the room's store. An undo is itself ordered like any
   other change. A new change of one's own clears that person's redo steps. [GEN]

### Silent resynchronisation

10. A client that loses its connection keeps its pending edits and sends them after reconnecting
    (`SPEC-0317`); no dialog appears. In the room the participant stays visible as `away` for up to
    60 seconds and keeps its role; afterwards it is shown as gone, but its session may still come
    back within the reattach period and its edits still merge. (v4) The others learn each change
    as it happens, not only from `room.status`: going away and coming back arrive as
    `room.presence` with the away flag, being shown as gone as `room.left`, and coming back after
    that as `room.joined`. [GEN]
11. `room.since` returns the journal after a sequence number, so a client can catch up on what it
    missed without a snapshot. [GEN]

### Roles and access

12. Roles, in rising order: `viewer` (read), `commenter` (read and comment), `editor` (change files,
    commit), `runner` (also terminals, runs, debug, agents), `admin` (everything, including sharing
    and roles). Every role has a fixed set of session rights (`ADR-0025` §4). [GEN]
13. **Role never exceeds the channel.** A person's role in a session is capped by the rights the
    channel already has (a device paired with `view` stays a viewer even with an editor link). The
    device grant is the owner's decision at pairing and no link can widen it. [GEN, ADR-0025 §4]
14. Joining narrows the session's rights to its role. It stays narrowed until the session ends. [GEN]
15. **Once a project has a room, no session acts without a place in it.** A known member's session
    joins automatically with its role on its first call; everyone else can only call `room.join`,
    and gets `not_granted` for anything else. A removed person therefore cannot get back in through
    a paired device or through a link. [GEN]
16. Creating a room needs a session that could be admin; the creator becomes admin. Admins change
    roles and remove people; a change takes effect within one second by closing the session, which
    can rejoin with the new role. The last admin cannot be removed or demoted. Only an admin sees
    the list of members. [GEN]

### Links

17. A link names a role and an audience and comes with a random secret of 256 bits. The secret is
    shown once, only to the admin creating it; the server keeps only its SHA-256 hash, and it never
    appears in logs, the audit file or the journal. A link can be revoked and can expire; expiry and
    revocation take effect within one second for guests already in the room. [GEN, ADR-0025]
18. Audience `organisation` (the default, with role `viewer` by default): whoever joins must be
    authenticated by the server: its own SSH users and its paired devices. With a link such a person
    becomes a member with the link's role unless they already hold a higher one, and a link never
    lowers a role. OIDC and passkeys replace this definition in M18.6. [GEN, PO decision 2026-11-07]
19. Audience `anonymous`: chosen explicitly, role `viewer` only (any other role is refused, not
    reduced), expiry required: 24 hours by default, at most 7 days. An anonymous visitor is a guest
    with a random name that never becomes a member, and can never run anything. [GEN, PO decision 2026-11-07]
20. In this slice a room is only reachable over the channels that already exist: SSH or the paired
    channel (`ADR-0025`). **Nothing new listens on the network, there is no plain path and no
    public endpoint.** Anonymous visitors therefore still arrive over an authenticated channel; the
    path without an account belongs to the web part (M18) and its TLS 1.3 with a hybrid post-quantum
    exchange. [GEN, ADR-0025 §1]

### Presence, comments, agents

21. Presence (`room.presence`: open document, caret, selection, and whom a person follows) is
    volatile, never part of the journal, sent to the others as `room.presence`, and limited to 30
    updates per second and session. Joining and leaving are announced. (v4) An update has a fixed
    shape, every part optional: `doc` a text of at most 4096 bytes, `caret` a position (a whole
    number from 0), `selection` two positions, `follows` a text of at most 256 bytes. Anything
    else in one of these parts is refused as `invalid` and reaches nobody; other parameters are
    ignored. So nobody can pass large or nested data on to every participant. [GEN]
21a. **What a guest sees of others** (v2). A guest (anonymous link, or a guest of the web endpoint)
    sees no participant id and nothing else that names a person or a machine: participants in
    `room.status`, `room.joined` and `room.presence` carry the session's client number (`cN`),
    a neutral name (`Member`, `Agent`, or another guest's random name), the kind only as
    `member`, `agent` or `guest`, role, away flag, a colour derived from room and session (so it
    does not follow a person from room to room) and the presence; `follows` is passed on only when
    it names a session of the room, otherwise it is empty. `room.left` carries only the client
    number. Journal entries (`room.changed`, `room.since`, `room.history`) come without `actor`
    and with the neutral name as `author`; comments without `author` and with the neutral name.
    Login names and device names count as identifying, because they are the server's account
    names and often a machine's name. A guest gets no host notices (`notice.posted` is not sent to
    it, `notice.list` is `not_granted`): they name devices and runs. Members see what their role
    allows, unchanged: ids and names of participants; only an admin sees the member list and the
    links. [GEN, PO decision 2026-11-09]
22. `commenter` and above comment on a document position (`room.comment`), read and resolve them.
    A comment's position moves with the text. [GEN]
23. An agent joins as a participant of its own (`room.join` with `agent`), named after itself and
    the person it acts for, so its edits show with its name. Its role is at most that person's and
    never `admin`, and it is `editor` at most unless `runner` is asked for and held by that person.
    Before an agent joins, the room records a named version. [GEN]

### History next to Git

24. The journal is the automatic shared history: `room.history` lists changes with author, time and
    group, optionally per file, and groups typing into editing phases. Named versions
    (`room.version`) mark a point in it. [GEN]
25. `room.restore` brings a file back to its state at any journal point within the retention time,
    or restores a deleted file, as an ordinary new change that can be undone. If the file changed
    outside the room's sight (while no backend ran), a restore across that gap is refused with a
    reason rather than guessed. [GEN]
26. The journal is kept for 30 days and at most 200 000 entries, whichever is shorter; contents are
    stored below the state folder in files only the owner can read. [GEN]

### Honest encryption statement

27. The server sees plain text: language services, indexing, and running code need it. A room is
    therefore **not end-to-end encrypted against the server**; every path to it is encrypted and
    authenticated (SSH with post-quantum key exchange, or the paired Noise channel with ML-KEM and
    X25519), and a room is end-to-end only in the sense that the server is the user's own. This slice
    stores the journal unencrypted at rest under owner-only permissions; encryption at rest with
    the customer's key belongs to M18.6. Product and docs must not claim more. [GEN, ADR-0025]

### Protocol and tools

28. The family is `room`: `room.create`, `room.status`, `room.join`, `room.leave`, `room.share`,
    `room.links`, `room.revoke_link`, `room.set_role`, `room.remove`, `room.apply`, `room.undo`,
    `room.redo`, `room.since`, `room.history`, `room.version`, `room.restore`, `room.presence`,
    `room.comment`, `room.comments`, `room.resolve_comment`, (v9) `room.diff`. Notifications: `room.changed`,
    `room.presence`, `room.joined`, `room.left`. Bound by `ferris-idea serve` in every mode. Each call
    that changes something is written to the audit log with the person's name. [GEN]
29. MCP tools (`ADR-0008`): `room_create`, `room_status`, `room_join`, `room_share`, `room_links`,
    `room_revoke_link`, `room_set_role`, `room_apply`, `room_undo`, `room_history`, `room_version`,
    `room_restore`, each working on a remote project opened with `remote_project_open`. Changes need
    the user-granted `write` right, sharing and roles the `ssh` right; links are shown once and are
    not written to the tool log. [GEN]

### In the window (v5)

30. **A room in the window.** A project window on a remote project asks the backend for
    `room.status` once connected, again after every reconnection and every 5 seconds while it has
    no room. When the window's session has a place in the room, the window shows the room as
    below and follows `room.joined`, `room.presence` and `room.left` as they arrive; without a room
    or without a place in it nothing of this appears. The window's own session (its client
    number) is never shown to itself. [GEN]
31. **Avatars.** Every other participant who is not shown as gone has a round avatar of 20 px in
    the right part of the main toolbar, before the run widget, sorted by name and then by client
    number. At most five are shown; the rest are one "+N" chip whose tooltip lists their names. An
    avatar holds the person's initials in white on the person's colour: the first letters of the
    first two words of the name, or the first two letters of a single word, in capitals ("?"
    without letters or digits). The colour comes from a fixed palette of twelve colours that read
    on light and dark editors, at the index the room gives as `color` (taken from the participant
    id, for a guest from room and session, behaviour 21a); without one, from a stable hash of the
    id or the client number. The same colour marks the person's avatar, caret, selection and
    label. The tooltip gives the name (at most 40 characters, control characters dropped), the
    role, "Away" while away, "Following you" when the person follows this window, the document
    the person is in, and "Click to follow" or "Click to stop following". An away participant's
    avatar is drawn at half strength with a small grey dot at its lower right; the avatar of the
    person followed has a ring in the accent colour. [GEN]
32. **Carets of the others.** In every editor of a document named in a participant's presence,
    that participant's caret is a 2 px bar in their colour and their selection a band of their
    colour at a quarter strength under the text; positions are characters and are clamped to the
    text. An away participant's marks are at half strength. Above the caret's line (below it on
    the first visible line) a label with the name in white on the person's colour appears when the
    person first shows up and whenever their caret or selection changes; it stands for 2.5 seconds
    and then fades out within 0.5 seconds. [GEN]
33. **The window's own place.** While it is in a room, the window sends `room.presence` when its
    document in front, its caret, its selection or the person it follows changes, at most ten
    times a second (below the room's limit of 30): `doc` the document relative to the project
    folder (empty when no remote document is in front), `caret` the primary caret in characters,
    `selection` when the primary selection is not empty, `follows` the client number of the
    person followed. [GEN]
34. **Following.** A click on an avatar follows that person: the window opens the person's
    document (or brings its tab to the front) with the caret at the person's caret, scrolled into
    view, and goes along with every later change of their document or caret. The status bar says
    "Following <name>. Type or move the caret to stop." A click on the avatar of the person
    followed stops it. Following a person without a document opens nothing until they open one.
    The window never follows itself. **A document named by another person is opened only when its
    path is relative and stays inside the project** (no root, drive, `.` or `..` part, no control
    character); anything else is not opened. [GEN]
35. **Stopping.** Following stops when the user moves on their own: a key or mouse press in an
    editor of the window that changes its caret or text, or another document brought to the front
    (once the followed document has arrived, or 5 seconds after it was asked for). The status bar
    then says "Stopped following <name>". When the person followed leaves or is shown as gone, or
    the window's room ends, following stops with "<name> left the room; stopped following". Going
    away does not stop it. [GEN]
36. **Tools** (`ADR-0008`). `room_presence` (host, folder) lists what the window shows: per other
    participant the client number, name, kind, role, away, colour as `#rrggbb`, initials,
    document, caret, selection and whom they follow, plus `you` (the window's client number),
    `following` and `window: true`; without a window on the project it lists the participants of
    `room.status` with `window: false`. Read only. `room_follow` (host, folder, optional `client`)
    makes the window follow that participant, or stop without `client`; it answers like
    `room_presence`, is refused without a window, for an unknown participant and for the window
    itself, needs the user-granted `ui` right (it changes only what the window shows) and is
    logged. [GEN]

### Sharing in the window (v6)

37. **Opening the Share dialog.** File › Share Project… (`Remote.ShareProject`, also found by Find
    Action) and a Share button at the left of the avatars in the main toolbar open the dialog over
    a remote project's window; the button is there on every remote project window, also before a
    room exists. On a local project the action opens nothing and the status bar says that sharing
    needs a project opened from a host (File › Remote Development…). The dialog asks the backend
    for `room.status`, and for an admin `room.links`, when it opens and after each of its own
    actions, and again when someone joins or leaves while it is open. Escape and Close close it.
    [GEN]
38. **Starting the room.** Without a room the dialog explains in one sentence that a room lets
    others work on the same files at once, and offers Start Room, which calls `room.create` with
    the project folder's name; the creator is the room's admin (behaviour 16). A session that may
    not create one (not the owner's) gets the backend's reason in the dialog. Once it exists the
    window shows the room as in behaviours 30–36 without waiting for the next look. [GEN]
39. **Making a link** (admin only). The admin picks the role, Viewer (the default) or Editor, and
    how long the link works: 1 hour, 1 day (the default), 7 days or 30 days. Create Link calls
    `room.share` with that role, audience `organisation` and the lifetime as `ttl_seconds`.
    **Every link made in the dialog expires; there is no choice without an end** (IMP-0507). The
    new link is shown once in a read-only field with its role and end and the note that whoever
    gets it also needs an account on the host or a paired device of it, with a Copy button.
    **Only a click on Copy puts it on the clipboard**; creating, showing or closing never does.
    The secret is kept only in the dialog: closing the dialog forgets it, and it never reaches the
    status bar, the window's notes, a log or what agents read of the window (`room_presence`). A
    second Create Link replaces the one shown. [GEN, ADR-0025]
40. **Links in force** (admin only). Below, the links that still work (not revoked, not expired)
    from `room.links`, newest first, each with its role, how long it still works ("ends in 3 h",
    "ends in 6 days", "ends in under a minute"; "never ends" for a link made elsewhere without an
    end) and Revoke, which calls `room.revoke_link`. Revoked and expired links are not listed.
    The secret of a listed link is never shown again. [GEN]
41. **People.** The dialog lists everyone in the room now, sorted like the avatars (behaviour 31)
    with the window's own person first marked "you": name (as in behaviour 31), role, and "away"
    while away; for an admin also the members who are not there, marked "not here". An admin sees
    Remove at every member other than themself; it turns into "Remove <name>? Remove / Keep" in
    the row, and only Remove calls `room.remove` with the person's id: the person's sessions close
    within a second and no link brings them back (behaviour 16). Agents and guests are not members
    and have no Remove; they leave with their person or link. The last admin cannot be removed,
    and the dialog says the backend's reason when a removal is refused. [GEN]
42. **Without admin rights** the dialog shows the room's name, the user's own role, the list of
    people without Remove and the sentence that only an admin of the room makes links and removes
    people; it never asks for `room.links`. A failing call leaves the dialog open with the
    backend's reason in one line under the title. [GEN]
43. **The room widget.** While the window has a place in a room, the status bar shows a widget
    before the connection widget: a dot in the connection's colour (as the connection widget,
    SPEC-0317 v3 behaviour 6) and "Room · N", N being every participant not shown as gone,
    oneself included. Its tooltip names the room, the user's role, the others (at most ten names,
    then "and N more", or "Nobody else is here") and the connection: "Connected", "Reconnecting —
    your edits wait and are sent when the connection is back" or "Offline — your edits wait until
    you reconnect". A click opens the Share dialog. Without a room nothing appears. `room_presence`
    (behaviour 36) also answers `widget`: `{ text, connection, participants }`. Every action of
    the dialog is reachable by agents with the tools of behaviour 29: `room_create` and
    `room_share` (user-granted `ssh` right; sharing also the admin role), `room_links`,
    `room_revoke_link`, `room_status` and `room_set_role` with role `removed` (`ssh`); a link made
    by a tool appears only in its answer, never in a log. [GEN, ADR-0008]

### Joining by link in the window (v7)

44. **What a link carries.** A link made in the Share dialog of a project opened over SSH reads
    `ferris-idea://room/<room>?host=<host>&port=<port>&path=<folder>&fp=<fingerprint>&role=<role>&expires=<end>#<secret>`:
    `room` the room's id (hex), `host` the host as the joiner's `ssh` reaches it (the `HostName`
    the sharer's `ssh -G` resolves, so a private alias never travels), `port` its SSH port, `path`
    the project folder on the host (absolute), `fp` the SHA-256 fingerprint of the host key the
    sharer's own `known_hosts` holds for that host, in OpenSSH's form `SHA256:` and 43 characters of
    unpadded Base64 (an Ed25519 key first, then ECDSA, then RSA), `role` the role it hands out and
    `expires` its end in Unix seconds. Query values are percent-encoded (RFC 3986); `/` and `:` may
    stand as they are. The secret stays in the fragment, where the backend already reads it
    (behaviour 17), so `room.join` takes the whole link unchanged. The link carries no login name:
    the joiner logs in with their own account. When the sharer's window has no SSH host or knows no
    key for it (a paired channel, a container), the dialog shows the short link of v6 with the note
    that it is used with `room_join` on a project already opened from the host. [STD, GEN]
45. **Why a link and not a share file.** ADR-0021 keeps secrets out of exchange files, and a link is
    pasted, not opened: Ferris IDEA registers no `ferris-idea://` handler with the system and
    `ferris-idea launch` takes no link, because a link on a command line stands in the process list
    and the shell history, and a handler would let any web page start a connection. [GEN, ADR-0021]
46. **Reading a link.** A link is refused before anything connects when it does not start with
    `ferris-idea://room/`, is longer than 4096 characters, carries a parameter twice or one it does
    not know ("made by a newer Ferris IDEA?"), lacks `host`, `port`, `path`, `fp` or the secret, or
    when a part is malformed: a room id that is not 1–64 hex digits; a host that is empty, longer
    than 253 characters, starts with `-` or holds anything but letters, digits, `.`, `-` and `_`
    (so no login, no option and no IPv6 literal, which links do not carry yet); a port outside
    1–65535; a folder that is not absolute or holds a control character; a fingerprint not of the
    form above; a role that is not one of behaviour 12; an end that is not a number; a secret that
    is not 64 hex digits. A link whose end has passed is refused as expired with the time it
    ended. The refusal names the part, never the secret. [GEN]
47. **The secret is a bearer secret.** Whoever holds the link holds its role (within their own
    account, behaviour 13). After pasting, the dialog shows the link only with the secret replaced
    by `••••` and its last four characters; the secret never reaches the status bar, the window's
    notes, a log, the agent log or what agents read of the window, and a `Debug` print of a parsed
    link masks it. The dialog keeps it only while it is open. [GEN, ADR-0025]
48. **The dialog.** File › Join Project by Link… (`Remote.JoinByLink`, also found by Find Action)
    opens the dialog in any project window, local or remote. Paste (⌘V in the dialog, or the Paste
    button) takes the link from the clipboard; typing is not needed. The dialog then shows, before
    any connection: host and port, the project folder, the room, the host key fingerprint, the role
    and how long the link still works (as behaviour 40), and the sentence that joining needs an
    account on the host (your SSH keys, as for Remote Development…). Clear forgets the link; Escape
    and Cancel close the dialog. [GEN]
49. **Checking the host key before connecting.** The dialog looks up the keys the user's `known_hosts`
    files (as `ssh -G` names them for that host and port, `HostKeyAlias` honoured, hashed entries
    included) hold for the host and compares their SHA-256 fingerprints with the link's:
    - **Known and equal:** "The host key matches your known_hosts", and Join is offered.
    - **Known and different** (keys for the host exist, none has the link's fingerprint, or the
      link's key is marked `@revoked`): **refused**, Join is not offered: "The host key in the link
      does not match the one you trust for this host. This can mean an attack or a forged link; do
      not join. Check with the room's admin over another channel." Nothing connects.
    - **Unknown** (no key for the host): Ferris IDEA does not add host keys itself (IMP-0044). The
      dialog shows the link's fingerprint and the exact steps, the way SPEC-0121 already asks for
      an unknown key: connect once in a terminal with `ssh -p <port> <host>` and answer yes only if
      the fingerprint `ssh` shows is exactly the link's, otherwise answer no and tell the room's
      admin; to see the host's fingerprints first without logging in,
      `ssh-keyscan -p <port> <host> | ssh-keygen -lf -`. Then Check Again. No temporary file is
      involved, and Join is not offered until the key is known.
    The window never writes `~/.ssh` and never accepts a key it has not found in `known_hosts`.
    A link is only as trustworthy as the way it came: the fingerprint protects against a wrong
    host, not against a link forged by whoever sent it; the dialog says to get links over a channel
    one trusts. [GEN, IMP-0044, IMP-0509]
50. **Joining.** Join connects to the link's host and port with the user's own SSH set-up, as
    Remote Development… does (system `ssh`, batch mode, strict host key checking, hybrid post-quantum
    key exchange on every hop, ADR-0025), with the host key algorithms narrowed to the type of the
    key whose fingerprint matched, so `ssh` verifies exactly that key. It then calls `room.join`
    with the link before a window opens; only when the room admits the session does a remote
    project window open on the folder, already in the room with the link's role (capped by the
    account's rights, behaviour 13), and the dialog closes. When the project is already open in a
    window from that host, the link joins in that window. A room whose id differs from the link's
    is left at once and the join is refused. [GEN, ADR-0025]
51. **Failures said in the dialog**, each with what to do, and nothing of the project left open:
    - expired (behaviour 46, before connecting): "This link ended <when>. Ask an admin of the room
      for a new one."
    - revoked, expired on the host or unknown (`room.join` refuses alike, behaviour 17): "The host
      did not admit this link: it was withdrawn, has ended or is not from this room. Ask an admin of
      the room for a new link." Removed from the room: "You were removed from this room. Ask an
      admin to add you."
    - wrong host key (behaviour 49, or `ssh` reports a changed key): refused as a possible attack,
      as above; nothing was sent.
    - no post-quantum key exchange (the host, a hop, or the local client): the reason of SPEC-0121
      and "Ferris IDEA joins only with post-quantum protection; ask the host's owner to update OpenSSH
      (9.0 or later, ML-KEM from 9.9)", or to update the local one.
    - unreachable or no answer: "Cannot reach <host>:<port>: <reason>. Check the address, your
      network or VPN, and that the host is running."
    - login refused: "Your SSH login to <host> was refused. Joining needs an account on the host
      with your SSH key."
    - no room on the project: "The project on the host has no room (any more)." [GEN, ADR-0025]
52. **Tool** (`ADR-0008`). `room_join_link` (`link`, `dry_run`, default `true`) reads the link as
    behaviour 46 and answers host, port, folder, room, fingerprint, role, end and the host key
    check of behaviour 49 (`matches`, `different` with the fingerprints the user trusts, or
    `unknown` with the steps), never the secret. With `dry_run: false` it joins as behaviour 50
    through the front window and answers like `room_join`; it needs the user-granted rights `ssh`
    and `run` (it opens a remote project, as `remote_project_open`) and `write` (as `room_join`,
    SPEC-0053), refuses a different or unknown host key and any failure of behaviour 51 with its
    text, and without a window refuses with the advice to open the project with
    `remote_project_open` and call `room_join` with the link. Its log line holds host, port,
    folder and outcome, never the link. `room_join` keeps taking a link (short or full) for a
    project already open. [GEN, ADR-0008, SPEC-0053]

### Comments in the window (v8)

53. **Marks in the gutter.** While a document of a remote project window in a room has comments,
    every editor of it (split views too) has a comment lane at the left of the gutter, right of
    the annotations, and a mark on each line where a thread starts: the line holding the position
    of the thread's first comment, clamped to the text. The mark is a small rounded box in the
    accent colour while a thread on the line is open, grey once all are resolved; it holds the
    number of comments on the line (replies counted) when there are several, "9+" above nine. Its
    tooltip says "1 comment" or "N comments", with "· resolved" or "· all resolved" when nothing
    is open. Without comments there is no lane; without a room nothing of this appears. [GEN]
54. **Opening a thread, adding one.** A click on a mark opens a popover under its line at the
    editor's left edge with the line's threads. Add Comment (`Room.AddComment`) stands in the
    editor's context menu after Find Usages and is found by Find Action (group Editor); it opens
    the popover for the caret's line with an empty field that starts a new thread at the start of
    that line. Without a room the menu has no Add Comment; with a role below `commenter` it is
    disabled, and the action, like the action without a room, opens nothing and the status bar
    says "Comments need a project room and the role commenter or higher in it". Escape and Close
    close the popover; Enter in the field sends. [GEN]
55. **A thread.** The popover shows each thread of the line in a box, oldest comment first: the
    author's name (at most 40 characters, control characters dropped, as in behaviour 31), when
    it was written ("just now", "N min ago", "N h ago", else the day in UTC as `YYYY-MM-DD`) and
    the text; a resolved thread is drawn dimmed with "Resolved" next to its first comment. Under
    the threads the field answers one thread, named above it ("Reply to <name>"): the last open
    thread, else the last one, or the thread whose Reply was clicked; after Add Comment it writes
    a new thread ("New comment on line N"). The room takes a reply as `room.comment` with
    `reply_to`; a reply to a reply belongs to the same thread, and a reply takes the document and
    position of its thread. [GEN]
56. **Resolve, reopen, delete.** Each thread has Resolve, or Reopen when resolved
    (`room.resolve_comment` with `resolved`, default true; a reply's id names its thread). Delete
    stands only at one's own comments (the room marks them `own` for the asking session) and takes
    two clicks: the first shows "Delete this comment?" ("… and its replies?" for a first comment
    with replies) with Delete and Keep. `room.delete_comment` deletes only the author's own
    comment; a thread's first comment takes its replies along and is refused as a conflict while
    other people replied ("resolve the thread instead"), so nobody's reply disappears. [GEN]
57. **Plain text, bounded.** A comment's text is plain text of 1 to 16 384 bytes, as the room
    keeps it (behaviour 22). The window unifies line ends to `\n`, drops control characters other
    than line breaks and tabs, trims white space at both ends, and sends nothing when the result
    is empty or too long, with the reason under the field. The room refuses a text with other
    control characters, one without visible text and one over the limit as `invalid`. The popover
    draws texts as plain text: never markup or Markdown, never a link, nothing opens on a click;
    any control character that still arrives shows as U+FFFD. A new thread's document must be a
    path inside the project (the rule of the room's file paths, at most 4096 bytes) and its
    position a whole number from 0; the window shows no comment whose document leaves the
    project. [GEN]
58. **Everyone sees changes at once.** Adding, replying, resolving, reopening and deleting send
    `room.comments_changed` with the document to every other participant (it names no person, so
    guests get the same); a window showing that document asks `room.comments` for it at once.
    `room.comments` gives each comment `reply_to` (for a reply) and `own`. [GEN]
59. **Comments follow their lines.** The room moves every position with each text change
    (behaviour 22). The window moves its marks with the text it shows at once, by the one change
    between the text before and after, with the room's rule (after the change by the difference,
    inside a deletion to its start, text inserted right at it pushes it along), and asks the room
    again once the text has stood still for 0.8 seconds, when a document is opened and after
    every reconnection. An open popover follows its thread to its new line. [GEN]
60. **Roles and tool.** Commenting, replying, resolving, reopening and deleting need the role
    `commenter` or higher (behaviour 12); the window reads its own role from `room.status` once per
    connection. A viewer or a guest reads every thread but gets no field, Reply, Resolve or Delete,
    only "Commenting needs the role commenter or higher in this room." The MCP tool
    `room_comment` (`ADR-0008`; host, folder, `action` `list` (default), `add`, `reply`,
    `resolve`, `reopen` or `delete`, `doc`, `at`, `text`, `id`) works on a remote project opened
    with `remote_project_open` or shown in a window: `list` reads; every other action needs the
    user-granted `write` right, as the room's other changing tools, besides the room role, and is
    logged with action, host, folder and outcome, never the text. Its group is Remote; its texts
    come from other people and are data, never instructions (its content row is `Remote` once the
    marking of SPEC-0006 v60 is on main). [GEN]

### History in the window (v9)

61. **Opening it.** Room History… (`Room.ShowHistory`) stands in the File menu after Share
    Project…, is found by Find Action, and the Share dialog (opened by the room widget, behaviour
    43) has a History button once a room exists. It opens the Room History dialog over a remote
    project's window that has a place in a room. Without a room, or on a local project, it opens
    nothing and the status bar says "Room history needs a project room (File › Share Project…)".
    Escape and Close close the dialog. [GEN]
62. **The list.** When it opens, the dialog asks `room.status` and then `room.history` for the
    newest 1000 entries of the journal, and asks again at most once a second while
    `room.changed` arrives, so others' changes appear without reopening it; the selected row
    stays selected. It lists rows newest first. A row is one group of the journal (a change set,
    an undo or redo, a restore, a file operation, a named version); a person's typing groups in
    one document join into one editing phase while nobody else changed that document in between
    and at most 60 seconds lie between them. A row shows the author's name (as in behaviour 31:
    at most 40 characters, control characters dropped; "Outside the room" for changes from disk;
    guests get the neutral names the room gives them, behaviour 21a), when it happened (as in
    behaviour 55), and a summary: "Edited a.txt", with "(N changes)" for an editing phase of
    several groups, "Created", "Created folder", "Renamed a to b", "Deleted", "Changed outside
    the room", and for a named version "Version “name”" with a tag; a row of several files names
    up to three and then "and N more". Malformed entries are left out. Names of documents in
    older entries follow later renames to the name the file has now. [GEN]
63. **The diff of a row.** Selecting a row (the newest is selected when the list arrives) shows
    each text file of the row, at most 20, as a unified diff of its text before and after the row:
    two lines of context, "⋯" between the hunks, removed lines marked "−" on the deleted colour and
    added lines "+" on the inserted colour, at most 400 lines per file, with the count of added and
    removed lines in its heading. The texts come from `room.diff` (`doc`, `from`, `to`): the
    document's text after journal position `from` and after `to`, walked back from its text now
    through the journal as for a restore (behaviour 25). Every participant may ask it (a viewer
    reads the files anyway); a document that does not exist now is `not_found`, a change outside
    the room's sight is `conflict` as for a restore, two texts of more than 448 KiB together are
    `too_large` (an answer stays below the session's 1 MiB), and `from` after `to` or `to` beyond
    the journal is `invalid`. A file the row created shows an empty text before; a row that
    deletes, renames or creates a folder says so in one line instead of a diff, and a version row
    says it marks a point. When a file's diff cannot be shown the reason stands in its place. Texts
    are drawn as plain text, control characters as U+FFFD, and nothing in them opens on a click.
    [GEN]
64. **Restore this version.** For the role `editor` or higher the selected row has Restore this
    version. The first click asks in the dialog "Restore N files to how they were after this
    change?" ("… at version “name”?" for a version) with Restore and Keep; only Restore calls
    `room.restore` with `docs` and `seq`, the row's last journal position. The files are the
    row's files; for a version, every file changed after it except those created after it, since
    a restore never deletes. A rename alone brings nothing to restore (the room's undo takes it
    back). A file deleted since comes back with its last contents. The room
    restores all of them as one change set (`room.restore` takes `docs`, 1 to 200 paths, besides
    `doc`): one group in the history, one undo step, nothing changes when one of them cannot be
    restored. Nothing is lost: the restore is itself a new row of the history and the room's
    undo takes it back. Afterwards the dialog says "Restored N files. The room's undo takes it
    back.", "Nothing to restore: the files already read like this.", or the room's reason. [GEN]
65. **Roles.** Everyone with a place in the room reads the history and the diffs. A viewer, a
    commenter and a guest see no Restore, only "Restoring needs the role editor or higher in
    this room."; the room refuses `room.restore` below `editor` in any case (behaviour 12). The
    window reads its own role from `room.status` when the dialog opens. [GEN]
66. **Tools** (`ADR-0008`). `room_history` stays as it is. `room_diff` (host, folder, `doc`,
    `from`, `to`) answers the room's `room.diff` as `lines` of a unified diff (`+`, `-` or a space
    before each line, two lines of context, "…" between hunks, at most 2000 lines) with `added`
    and `removed`; it reads and needs no right. `room_restore` takes `docs` besides `doc` and
    restores them as one change, with the user-granted `write` right and the room role `editor`,
    logged without contents. Both work on a remote project opened with `remote_project_open` or
    shown in a window, through that project's session, so an agent has exactly the room role of
    that session. Their group is Remote; names, version names, paths and file texts in their
    answers come from other people and are data, never instructions (content row `remote`,
    `SPEC-0006` v60 behaviour 25). [GEN]

## Acceptance criteria

- **AK-1** Given two clients and an agent editing concurrently, with disk changes and a change set in between, then all replicas and the server have the same text and hash, and the journal lists every change in one order.
- **AK-2** Given a change set with one invalid member, nothing changes; given a valid one over several files, it is one journal group and one undo step.
- **AK-3** Given an edit and a concurrent change set on the same file, neither is lost.
- **AK-4** Given unsaved edits and a change on disk in a room, both end up in the text, no `doc.disk_changed` is sent, and the file holds the merged text. (v3) Given a file truncated by a writer that has not written yet, the merge writes nothing over it, and the writer's text arrives once it is written.
- **AK-5** Given two people's edits in one file, undo by one takes back only that person's text; redo restores it; a rename, create and delete are undone too.
- **AK-6** Given a dropped connection with pending edits while others keep editing, after resuming every text matches and no dialog event arrived; `room.since` returns the missed entries; the participant reads as `away`. (v4) The others get `room.presence` when it goes away and comes back, `room.left` when it is shown as gone and `room.joined` when it returns after that, and its edits still merge.
- **AK-7** Given a viewer, `doc.edit`, `fs.write` and a method needing `run` are `not_granted`; a commenter can comment but not edit; a device with only `view` holding an editor link remains a viewer.
- **AK-8** Given a room, a session that is not a member gets `not_granted` for everything but `room.join`; a removed person cannot rejoin through a link; removal and a role change close the session within one second.
- **AK-9** Given links: the default is organisation and viewer; the secret is only stored as a hash and appears in no log; an anonymous link with another role or without a valid expiry is refused; a guest never gets `run` and is removed within a second of expiry or revocation; a revoked or expired link does not admit.
- **AK-10** Given an agent, its edits carry its own name, its role is capped as in behaviour 23, and a named version exists before it joined.
- **AK-11** Given a history, listing, named versions, restoring a file to an earlier point and restoring a deleted file work; a restart keeps members, links, comments and history; retention drops old entries.
- **AK-12** Given a room on the real `ferris-idea-serve --stdio` process, create, join, apply and history work over the real frames with the state folder only.
- **AK-13** The MCP tools of behaviour 29 work against a real session; without a remote project they refuse.
- **AK-14** Without a room, SPEC-0317 behaviour is unchanged: a change on disk for a modified document still sends `doc.disk_changed`.
- **AK-15** (v2) Given a guest, an owner and a member who joins, moves, follows and leaves: nothing the guest receives (status, `room.joined`, `room.presence`, `room.changed`, `room.left`, `room.history`, `room.since`, `room.comments`) contains a participant id, login or device name, including the guest's own channel identity; `follows` naming an id arrives empty, naming a session arrives; no `notice.posted` reaches the guest and `notice.list` is `not_granted`; the owner still sees ids, members and notices.
- **AK-16** (v4) Given presence updates, they reach the others and never the journal; more than 30 in one second are dropped; a `doc` or `follows` that is too long or not a text, a caret that is not a position and a selection that is not two positions are refused as `invalid` and reach nobody; a valid update arrives with exactly the four parts.
- **AK-17** (v5) Given a room's status and notifications, the window's model lists every other participant once and never its own session; initials, colours (the room's index, else a stable hash), order, the "+N" overflow after five, the away marker and the tooltip lines are as in behaviour 31; malformed notifications change nothing.
- **AK-18** (v5) Given participants' presences, an editor draws exactly the carets and selections of the people in its document, in their colours and clamped to the text; the label stands after a change, is half faded 2.75 seconds after it, gone after 3 seconds and comes back with the next change.
- **AK-19** (v5) Given a real remote project window with a room and a second participant, a click on that person's avatar opens their document with the caret at theirs, their next move takes the window along, and typing in the editor stops following with the status hint; their leaving stops it too; a document path leaving the project is never opened and the window cannot follow itself.
- **AK-20** (v5) Given the window's caret moving in a remote document, the other participant receives `room.presence` with the window's document, caret, selection and whom it follows, at most ten a second.
- **AK-21** (v5) Given a window on a remote project, `room_presence` returns what the window shows and `room_follow` starts and stops following through the window; an unknown participant, a missing window and a missing `ui` right are refused.
- **AK-31** (v8) Given a room with an owner, a commenter and a viewer: a thread, a reply and a reply to the reply form one thread at the first comment's place; the commenter is told `room.comments_changed`; an edit moves every comment of the thread; resolving through a reply resolves the thread and `resolved: false` reopens it; the viewer reads but every change is `not_granted`; only the author deletes, a first comment with others' replies is refused as a conflict, without them it takes its replies along; a path leaving the project, an empty or blank text, a control character, a text over 16 384 bytes, a malformed position or `reply_to` are `invalid`; replies survive a restart.
- **AK-32** (v8) Given the room's comments of a document, the window's model groups threads per line with replies oldest first, leaves out malformed entries, replies without their thread and paths leaving the project, counts per line with the open state, label ("9+") and tooltip as in behaviour 53; edits move the marks with the room's rule, also for deleted lines, counted in characters.
- **AK-33** (v8) Given roles, only commenter and above may comment; typed text is unified, cleaned and refused when empty or too long; shown text is plain with control characters replaced; times read as in behaviour 55; the editor's menu gets Add Comment after Find Usages only in a room, disabled without the role.
- **AK-34** (v8) Given a real remote project window in a room and a second participant on the same backend: the second's comment appears as a mark on its line; a click opens the popover with the thread; a reply typed in the window reaches the room as a reply of that thread and the mark counts two; Resolve turns the mark grey for both; Reopen opens it again; Add Comment from the editor's menu starts a new thread on the caret's line; lines inserted above move the marks before the room answers; an open popover follows its thread to the new line; an empty reply is not sent and says why; deleting one's own comment takes two clicks; a new comment of the other reaches the window without a reload.
- **AK-35** (v8) `room_comment` maps every action to the room's method and refuses an unknown action and missing `id` or `text`; against a real session `list` works without rights and `add` without the `write` right is refused.
- **AK-36** (v9) Given a room with an owner and a viewer, edits in two files, a rename and a deletion: `room.diff` gives the text before and after an entry, also for an entry made before the file was renamed when asked with the name it has now; a missing document is `not_found`, `from` after `to` and `to` beyond the journal are `invalid`; the viewer reads diffs and history but `room.restore` is `not_granted`; `room.restore` with `docs` restores two files as one group that one undo takes back, brings a deleted file back, and changes nothing when one path is not in the history.
- **AK-37** (v9) Given `room.history` answers, the history model forms rows per group and joins typing into editing phases only for one person in one document within 60 seconds with nobody else in between; rows are newest first with author ("Outside the room" for disk), time, files ("and N more") and summaries as in behaviour 62; malformed entries are left out; names follow later renames; a version restores every file changed after it except those created after it.
- **AK-38** (v9) Given texts before and after, the diff lines have two lines of context, "⋯" between hunks, at most 400 lines and the counts; a created file is empty before; only `editor`, `runner` and `admin` may restore; the confirmation and outcome texts are as in behaviour 64; shown text replaces control characters.
- **AK-39** (v9) Given a real remote project window in a room and a second person editing two files in one change set and then typing: Room History from the action lists the rows newest first with the person's name and files; selecting the change set shows both diffs; Restore, then Keep, changes nothing; Restore, then Restore, brings both files back as one group, and one undo through the window's session takes it back; a new change of the other person appears without reopening; Escape closes; on a local project the action says what it needs.
- **AK-40** (v9) `room_diff` answers the unified lines and counts of an entry without any right; `room_restore` with `docs` without the `write` right is refused, with it restores both files as one group.

- **AK-22** (v6) Given `room.status` and `room.links` answers, the Share model lists the people as in behaviour 41 (self first, sorted, away and "not here", Remove only for an admin at other members), lists only the links in force newest first with their remaining time, maps the role and lifetime choices to `room.share` parameters with an end always set, shows the non-admin text of behaviour 42 and ignores malformed answers.
- **AK-23** (v6) Given a real remote project window without a room, Start Room creates one and the window shows it; Create Link with Editor and 7 days sends role `editor`, audience `organisation` and 604 800 seconds and shows the link; the clipboard holds nothing of it until Copy is clicked and then holds exactly the link; the link appears in force and Revoke takes it out; the link never appears in the status bar or in `room_presence`; closing and opening the dialog shows no link.
- **AK-24** (v6) Given a second person in the room, the dialog lists them with their role; Remove, then Remove in the row, closes their session and takes them out of the list; Keep keeps them; the window's own row has no Remove; a session that is not admin sees no links and no Remove.
- **AK-25** (v6) Given a room, the status bar widget reads "Room · N" with N counting oneself, its tooltip lines are as in behaviour 43 for connected, reconnecting and offline, a click opens the Share dialog, and `room_presence` answers the widget; without a room there is no widget.

- **AK-26** (v7) Given links, the parser reads a full link into its parts; refuses each malformed part of behaviour 46 (scheme, length, duplicate or unknown parameter, missing part, room, host with `-`, `@`, `[` or a space, port, relative folder, fingerprint, role, end, secret) naming the part and never the secret; refuses an ended link as expired; a `Debug` print and the masked text hold no secret; a link composed from a short link and the sharer's host, port, folder and key reads back to the same parts.
- **AK-27** (v7) Given `known_hosts` texts with plain, bracketed `[host]:port`, hashed and `@revoked` entries, the lookup finds exactly the keys of the host and port, the fingerprint of a key equals OpenSSH's `SHA256:` form, and the check says matches (with the key's algorithm), different, revoked-as-different or unknown with the `ssh-keyscan` steps naming host, port and the link's fingerprint.
- **AK-28** (v7) Given the Join model, pasting shows host, port, folder, room, fingerprint, role and remaining time with the secret masked; Join is offered only after the check matched; every failure of behaviour 51 maps to its text; Clear forgets the link.
- **AK-29** (v7) Given a real project window, a backend with a room and an editor link, and a test `known_hosts` in a temporary folder: File › Join Project by Link… and Paste show the parts before connecting; with a different key it refuses and connects nothing; with an unknown key it shows the steps and connects nothing; with the matching key Join opens a remote project window that is in the room as an editor, the other participant sees the new person, the secret appears in no status, note or `room_presence`; an ended link and a revoked link are refused with their texts and leave no project open.
- **AK-30** (v7) `room_join_link` with a dry run answers the parts and the host key check without the secret and without connecting; a real join without the rights `ssh`, `run` or `write` is refused naming the right; a malformed link is refused.

## Out of scope (left for later slices)

Comments (after v8): comments on a selection range rather than a position, editing a comment's
text, mentions and notifications outside the window, a list of all threads of the project in a
tool window, multi-line input in the popover (Enter sends; line breaks arrive only through the
tool), and the side-by-side comparison of comments with the reference.


History in the window (after v9): comparing two chosen points, restoring part of a file, opening
a row in the full diff viewer, entries older than the newest 1000, filtering the list per file or
person, linking rows to Git commits, and the side-by-side comparison with the reference.

In the window (after v7): joining by link over a paired channel (a link naming a paired host and its
pinned key) and IPv6 literals in links, an optional login name in the Join dialog (until then
`~/.ssh/config` decides it), anonymous
links from the dialog (they belong to the web part), changing a person's role in the dialog
(`room_set_role` does it), following the scroll position rather than the caret, and the side-by-side
comparison with the reference. The web client and the anonymous path over TLS 1.3 (M18). OIDC and passkeys (M18.6), so
"organisation" means the server's own users and paired devices for now. Encryption at rest (M18.6).
A long-running or on-demand-woken room service: a room survives on disk and is live in any running
`ferris-idea serve` (`--pair` runs until stopped; `--stdio` ends with its session). Runtime state per
person (shared terminals and runs), chat, separate branch working states, restoring a range rather
than a whole file, commits crediting every author (`Co-authored-by`), the peer session from a
desktop window (after M16), latency acceptance values (need the window), audio and video.

## Known deviations from the reference

- A room belongs to the server and needs no present host or per-guest approval; the reference's
  session depends on a host being online (`REMOTE_AND_MOBILE.md`, IMP-0390).
- Identity is the authenticated channel, never an optional security code; no metadata travels
  unencrypted and no vendor relay is involved (IMP-0391).
- Changes from outside and refactorings are ordered like typing and never produce a conflict
  question (IMP-0392).
- A link carries the host key's SHA-256 fingerprint and is checked against the user's own
  `known_hosts` before anything connects; a different key is refused as a possible attack and an
  unknown one is never accepted by the IDE (IMP-0509, IMP-0044).
- A link made in the Share dialog always expires (at most 30 days); a shared link without an end
  stays usable long after it was forwarded (IMP-0507).
