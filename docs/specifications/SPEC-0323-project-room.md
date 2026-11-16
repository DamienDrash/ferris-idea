# SPEC-0323: Project room on the server (first slice of M6.15)

- **Version:** 3 · **Date:** 2026-11-12 (v3: the merged text is written back only over the file
  just read, behaviour 5, AK-4); 2026-11-10 (v2, Project Owner's decision of 2026-11-09 evening:
  guests see no participant ids and nothing else identifying, behaviour 21a, AK-15; "organisation"
  stays the server's SSH users and paired devices, behaviour 18)
- **Sources:** [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M6.15 and the
  Project Owner's decisions of 2026-11-07 and 2026-11-09 (`GEN`), [`ADR-0025`](../adr/0025-network-security-for-remote-connections.md),
  [`ADR-0026`](../adr/0026-session-protocol-window-and-serve.md), SPEC-0317, SPEC-0318, SPEC-0321 (`GEN`);
  operational transformation and selective undo as in the published Jupiter and GOT work (`GEN`).
  The improvements over the reference come from public documentation and reports already recorded in
  `REMOTE_AND_MOBILE.md` (`DOC`, `GEN`).
- **Feature matrix rows:** F-089
- **Status of the comparison:** the roadmap asks for a side-by-side with the reference's Code With
  Me first. **It is still outstanding**: this slice was written without screen control and without
  touching the reference; nothing here comes from observing it. The main session does the
  comparison; what it finds becomes a new version of this spec.

## Purpose

A project on `idecraft serve` can have one shared room. Everyone who has a right to the room works
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
    back within the reattach period and its edits still merge. [GEN]
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
    updates per second and session. Joining and leaving are announced. [GEN]
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
    `room.comment`, `room.comments`, `room.resolve_comment`. Notifications: `room.changed`,
    `room.presence`, `room.joined`, `room.left`. Bound by `idecraft serve` in every mode. Each call
    that changes something is written to the audit log with the person's name. [GEN]
29. MCP tools (`ADR-0008`): `room_create`, `room_status`, `room_join`, `room_share`, `room_links`,
    `room_revoke_link`, `room_set_role`, `room_apply`, `room_undo`, `room_history`, `room_version`,
    `room_restore`, each working on a remote project opened with `remote_project_open`. Changes need
    the user-granted `write` right, sharing and roles the `ssh` right; links are shown once and are
    not written to the tool log. [GEN]

## Acceptance criteria

- **AK-1** Given two clients and an agent editing concurrently, with disk changes and a change set in between, then all replicas and the server have the same text and hash, and the journal lists every change in one order.
- **AK-2** Given a change set with one invalid member, nothing changes; given a valid one over several files, it is one journal group and one undo step.
- **AK-3** Given an edit and a concurrent change set on the same file, neither is lost.
- **AK-4** Given unsaved edits and a change on disk in a room, both end up in the text, no `doc.disk_changed` is sent, and the file holds the merged text. (v3) Given a file truncated by a writer that has not written yet, the merge writes nothing over it, and the writer's text arrives once it is written.
- **AK-5** Given two people's edits in one file, undo by one takes back only that person's text; redo restores it; a rename, create and delete are undone too.
- **AK-6** Given a dropped connection with pending edits while others keep editing, after resuming every text matches and no dialog event arrived; `room.since` returns the missed entries; the participant reads as `away`.
- **AK-7** Given a viewer, `doc.edit`, `fs.write` and a method needing `run` are `not_granted`; a commenter can comment but not edit; a device with only `view` holding an editor link remains a viewer.
- **AK-8** Given a room, a session that is not a member gets `not_granted` for everything but `room.join`; a removed person cannot rejoin through a link; removal and a role change close the session within one second.
- **AK-9** Given links: the default is organisation and viewer; the secret is only stored as a hash and appears in no log; an anonymous link with another role or without a valid expiry is refused; a guest never gets `run` and is removed within a second of expiry or revocation; a revoked or expired link does not admit.
- **AK-10** Given an agent, its edits carry its own name, its role is capped as in behaviour 23, and a named version exists before it joined.
- **AK-11** Given a history, listing, named versions, restoring a file to an earlier point and restoring a deleted file work; a restart keeps members, links, comments and history; retention drops old entries.
- **AK-12** Given a room on the real `idecraft-serve --stdio` process, create, join, apply and history work over the real frames with the state folder only.
- **AK-13** The MCP tools of behaviour 29 work against a real session; without a remote project they refuse.
- **AK-14** Without a room, SPEC-0317 behaviour is unchanged: a change on disk for a modified document still sends `doc.disk_changed`.
- **AK-15** (v2) Given a guest, an owner and a member who joins, moves, follows and leaves: nothing the guest receives (status, `room.joined`, `room.presence`, `room.changed`, `room.left`, `room.history`, `room.since`, `room.comments`) contains a participant id, login or device name, including the guest's own channel identity; `follows` naming an id arrives empty, naming a session arrives; no `notice.posted` reaches the guest and `notice.list` is `not_granted`; the owner still sees ids, members and notices.

## Out of scope (left for later slices)

The window (avatars, cursors, follow mode, share dialog, comment gutters, status of a room): needs a
screen. The web client and the anonymous path over TLS 1.3 (M18). OIDC and passkeys (M18.6), so
"organisation" means the server's own users and paired devices for now. Encryption at rest (M18.6).
A long-running or on-demand-woken room service: a room survives on disk and is live in any running
`idecraft serve` (`--pair` runs until stopped; `--stdio` ends with its session). Runtime state per
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
