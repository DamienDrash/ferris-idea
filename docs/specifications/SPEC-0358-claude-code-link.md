# SPEC-0358: Claude Code link

- **Version:** 1 · **Date:** 2026-11-23
- **Sources:**
  - `OSS` `coder/claudecode.nvim` (MIT, Copyright 2025 Coder Technologies Inc.), commit
    `8bcc7bb816750405b624b4a4e05f9ef367715d1e`: `PROTOCOL.md` and the plugin's sources
    `lua/claudecode/lockfile.lua`, `server/init.lua`, `server/handshake.lua`, `tools/*.lua`,
    `diff.lua`. Read for the protocol only (file names, field names, message names, result
    texts); no code was taken over. Recorded in `docs/legal/PROVENANCE.md`.
  - `DOC` Anthropic's public Claude Code documentation, page "Environment variables"
    (code.claude.com/docs/en/env-vars, read 2026-11-23): the variables that turn the IDE
    connection on and off, restated in own words.
  - `STD` JSON-RPC 2.0; the Model Context Protocol (MCP) message shapes `initialize`, `tools/list`,
    `tools/call` with text content; WebSocket (RFC 6455).
  - `GEN` Loopback servers with a bearer secret in a private file (as `SPEC-0221` behaviour 2);
    a hand test with Claude Code 2.1.330 on 2026-11-23 (its own debug log and screen output, run as
    a user; behaviours 8a and 18).
  - No Anthropic or JetBrains program, extension or package was unpacked, decompiled or read
    inside (`docs/legal/RESEARCH_BOUNDARIES.md`). The reference IDE is not involved.
- **Feature matrix rows:** none (AI integration, the "terminal" mode of the Claude plan).
- **Related:** `SPEC-0221` (MCP over HTTP: same loopback and token rules), `SPEC-0220` (agents'
  edits of open files go through the editor's buffer), `SPEC-0084` (diff viewer), ADR-0008 (every
  feature has an MCP tool), ADR-0017 (credentials stay with the provider's program), CLAUDE.md
  rule 12 (security first).

## Purpose
A person runs Claude Code, Anthropic's `claude` command-line program signed in with their own
account, in a terminal tab of Ferris IDEA. Claude Code then treats Ferris IDEA as its IDE, as it
does with the official editor integrations: it knows which file and text the person has selected,
which files are open, reads the errors and warnings of the editor, and shows each change it wants
to make as a diff in the Ferris IDEA window that the person accepts or rejects there. Claude Code
stays the person's own client; Ferris IDEA only answers it.

## Protocol as established
What Claude Code expects, from the sources above:
- **Discovery.** The IDE writes a lock file `<port>.lock` into `~/.claude/ide/` (or
  `$CLAUDE_CONFIG_DIR/ide/`), a JSON object with `pid`, `workspaceFolders` (absolute paths),
  `ideName`, `transport` (`"ws"`) and `authToken`. A terminal started by the IDE carries
  `CLAUDE_CODE_SSE_PORT` (the port) and `ENABLE_IDE_INTEGRATION=true`; Claude Code started there
  connects to that port. Without the variables, `/ide` inside Claude Code lists the lock files whose
  folder holds the current directory. `[OSS]` `CLAUDE_CODE_AUTO_CONNECT_IDE=false` turns the
  automatic connection off on Claude Code's side. `[DOC]`
- **Transport.** A WebSocket on 127.0.0.1; one JSON-RPC 2.0 message per text frame. The opening
  request carries the token in the header `x-claude-code-ide-authorization`. `[OSS]` It also offers
  a WebSocket subprotocol and refuses a server whose answer names none ("Missing client protocol");
  the server answers with the offered `mcp`. (Own hand test of the client, 2026-11-23, from Claude
  Code's own debug log; no program was looked into.)
- **Handshake.** Claude Code sends `initialize` (with `clientInfo`), then the notifications
  `notifications/initialized` and `ide_connected` (`{pid}`), and asks `tools/list`. `[OSS, STD]`
- **Tools** (called with `tools/call`, answered with MCP text content): `openFile`, `openDiff`
  (answers only when the person decided: `FILE_SAVED` plus the accepted text, or `DIFF_REJECTED`
  plus the diff's name), `getCurrentSelection`, `getLatestSelection`, `getOpenEditors`,
  `getWorkspaceFolders`, `getDiagnostics`, `checkDocumentDirty`, `saveDocument`, `close_tab`
  (answers `TAB_CLOSED`), `closeAllDiffTabs` (answers `CLOSED_<n>_DIFF_TABS`), `executeCode`
  (notebooks only). `[OSS]`
- **Notifications from the IDE.** `selection_changed` with `text`, `filePath`, `fileUrl` and
  `selection` (`start`, `end` as 0-based `line`/`character`, `isEmpty`); `at_mentioned` with
  `filePath`, `lineStart`, `lineEnd`. `[OSS]`
- **Who writes.** After `FILE_SAVED` Claude Code writes the accepted text to the file itself. `[OSS]`

## Behaviour

### Start, discovery and lifetime
1. A project window registers its folder when it opens. The link of a project starts the first
   time a terminal of that project's window starts a shell or program (also a terminal restored with
   the window, at its first frame), or when another part of Ferris IDEA asks for it (`ensure_started`, the hook for the
   Claude Code terminal tab of the AI settings). A window without a terminal opens no port and
   writes no lock file. [GEN]
2. Every terminal of a linked project gets `CLAUDE_CODE_SSE_PORT=<port>` and
   `ENABLE_IDE_INTEGRATION=true`; the variables of the shell integration and the user's
   environment stay as they are. [OSS]
3. The lock folder is `FERRIS_IDEA_CLAUDE_IDE_DIR` when set; the self-test's own temporary folder; a
   folder `claude-ide` inside the settings folder when Ferris IDEA runs with a settings folder of
   its own (`FERRIS_IDEA_CONFIG_DIR`, as every window test does), so tests never write the user's
   `~/.claude`; otherwise `$CLAUDE_CONFIG_DIR/ide`, else `~/.claude/ide`. [OSS, GEN]
4. The lock file is `<port>.lock` with `pid` (Ferris IDEA's process), `workspaceFolders` (the
   project folder, links resolved), `ideName` `Ferris IDEA`, `transport` `ws` and `authToken`. It
   is written through a new temporary file created with mode 0600 and renamed into place; the
   folder is created with, or set to, mode 0700. Off Unix the link does not start (a private file
   cannot be made yet). [OSS, GEN]
5. The link stops when its project window closes and when Ferris IDEA quits: the listener closes,
   every connection ends, every waiting diff is answered as rejected, and the lock file is
   removed. [GEN]
6. At start, lock files in the folder whose `ideName` is `Ferris IDEA` and whose process no longer
   runs are removed. Files of other IDEs, of running processes, of this process, links, files over
   64 KiB and unreadable files stay. [GEN]

### Security
7. The link listens on 127.0.0.1 only, on a port the system picks from its ephemeral range;
   connections from other addresses are dropped. [GEN]
8. Every link has its own token: 32 bytes from the operating system's random source, written as 64
   hex digits, only into the lock file. The opening request is refused (HTTP 401) without the header
   or with any other value; the comparison takes time that depends only on the length. It is also
   refused (HTTP 403) when it carries an `Origin` header (a browser page) or a `Host` other than the
   loopback address with this port (DNS rebinding). A refusal tells nothing about the token. [GEN]
8a. The answer to the opening request names the subprotocol the client offered: `mcp` when
    offered, else the first offered name (letters, digits, `.`, `-`, `_`, at most 64 characters),
    else none. [GEN: hand test of Claude Code 2.1.330, its own debug log]
9. A message is at most 16 MiB; at most 8 clients are connected and each has at most 16 tool calls
   running at once; a file read or written by the link is at most 10 MiB of UTF-8 text. [GEN]
10. Requests other than `initialize` and `ping` are refused (`-32002`) until the client sent
    `initialize`. The client's name and version (at most 100 printable characters) and the pid of
    `ide_connected` are kept for the state (behaviour 23). Unknown notifications are ignored;
    unknown methods answer `-32601`. [STD]
11. Questions to the window wait at most 10 seconds; then the call fails with "the Ferris IDEA
    window did not answer". Neither the link nor the window logs file contents, selections or the
    token. [GEN]

### Tools
12. `tools/list` lists `openFile`, `openDiff`, `getCurrentSelection`, `getLatestSelection`,
    `getOpenEditors`, `getWorkspaceFolders`, `getDiagnostics`, `checkDocumentDirty`,
    `saveDocument` and `closeAllDiffTabs`; `close_tab` is answered but not listed. `executeCode` is
    not offered. Every path a tool takes is an absolute path or `file://` URI. [OSS]
13. `openFile` opens a file of the project in an editor tab and selects from the first occurrence
    of `startText` to the end of the first occurrence of `endText` after it (with
    `selectToEndOfLine`, to the end of that line), or the lines `startLine` to `endLine` (1-based).
    With `makeFrontmost` (default) the window comes to the front and the editor takes the focus,
    and the answer is `Opened file: <path>`; without, the answer is JSON with `success`,
    `filePath`, `languageId` and `lineCount`. A missing file fails. [OSS]
14. `getCurrentSelection` answers the selection of the active editor (`success`, `text`,
    `filePath`, `fileUrl`, `selection`), or `success: false` without an editor. Lines and
    characters are 0-based; a character counts Unicode scalar values. While a client is connected
    the window looks at the selection every 200 ms and sends `selection_changed` to every
    initialized client when it differs from the one sent last. `getLatestSelection` answers the
    last selection seen, or `success: false`. [OSS]
15. `getOpenEditors` answers `{tabs: [{uri, isActive, label, languageId, isDirty}]}` for the editor
    tabs of the project window. `checkDocumentDirty` answers `isDirty` of an open file, or
    `success: false` for a file that is not open. `getWorkspaceFolders` answers the project folder.
    [OSS]
16. `getDiagnostics` answers, per file, the problems the editor shows (language servers and Ferris
    IDEA's inspections) with `message`, `severity` (`Error`, `Warning`, `Information`, `Hint` for
    unused code), and `range` (`start`, `end`): of the file `uri` names, or of every open file.
    [OSS]
17. `openDiff` reads the file as it is on disk (empty for a new file) and shows the proposal in the
    diff viewer of the project window: title "Claude Code proposes a change to <name>", left side
    "Current" (or "New file"), right side "Proposed by Claude Code", syntax colours of the file.
    The window comes to the front. Proposals that arrive while one is shown wait and are shown one
    after the other; a proposal with the name of a waiting one replaces it, and the replaced call
    is answered `DIFF_REJECTED`. [OSS]
18. The viewer of a proposal has Accept and Reject in its header. Accept answers `FILE_SAVED` and
    the proposed text; Claude Code then writes the file itself, and an editor that shows the file
    follows the change on disk. The link writes nothing: a write of its own before Claude Code's
    made Claude Code's edit fail as "changed since it was last read" (hand test 2026-11-23). Reject
    and Escape answer `DIFF_REJECTED` with the diff's name; nothing is written. [OSS, GEN: hand test]
19. A waiting proposal is answered `DIFF_REJECTED` and closed, writing nothing, when Claude Code
    calls `close_tab` with its name or `closeAllDiffTabs` (which answers the count), when the
    client that proposed it disconnects, when another viewer takes its place in the window, when
    the project window closes, and when the link stops. [OSS, GEN]
20. Every path is checked before anything is shown or written: absolute, no `.` or `..`, and the
    deepest existing folder on the way, with links resolved, lies inside the project folder. A path
    outside the project is refused (`-32602`); Claude Code then falls back to its own permission
    prompt in the terminal. [GEN]
21. No tool of the link writes a file: `saveDocument` answers `success: false` and saves nothing;
    `executeCode` does not exist. Claude Code writes a proposed change only after the person's
    Accept (in its default permission mode, where it proposes changes as diffs). [GEN]
22. Unknown tools answer `-32601`; wrong arguments `-32602`; failures `-32000` with the reason.
    [STD]

### State and display
23. The status bar of a project window shows "Claude Code connected" while a client that sent
    `initialize` is connected to the project's link. The MCP tool `claude_code_link` answers the
    state: `running`, `listening`, `port`, `clients` (name, version, pid, seconds connected),
    `pending_diffs` and `lock_file`; never the token. In a server without the window (stdio) it
    lists the running Ferris IDEA links of the project found in the lock folder, with port and
    pid only. [ADR-0008]

## Acceptance criteria
- **AK-1** Given a started link, then its lock file `<port>.lock` names this process, the project,
  `Ferris IDEA` and `ws`, holds a 64-hex-digit token different from another link's, has mode 0600
  in a folder of mode 0700, and no temporary file stays.
- **AK-2** When the link stops, its lock file is gone and the port is closed.
- **AK-3** The lock folder follows `CLAUDE_CONFIG_DIR`, else the home folder.
- **AK-4** Given lock files of Ferris IDEA for an ended process, for a running process and for this
  process, and of another IDE for an ended process, then cleaning up removes only the first.
- **AK-5** A connection without the token, with a wrong token, with a token one character short,
  with an `Origin` header or with another `Host` is refused before any message; the right token
  connects. The offered subprotocol `mcp` is named in the answer (behaviour 8a).
- **AK-6** `initialize` answers the asked protocol version (the oldest known one for an unknown
  version) and the server name `Ferris IDEA`; `tools/list` names the tools of behaviour 12 and not
  `executeCode`; a request before `initialize` is refused with `-32002`.
- **AK-7** `getCurrentSelection` answers the window's selection; a changed selection is sent once as
  `selection_changed`, a repeat is not; `getLatestSelection` answers the newest.
- **AK-8** `getOpenEditors`, `getWorkspaceFolders`, `getDiagnostics` (all files and one file) and
  `checkDocumentDirty` answer the window's state in the shapes of behaviours 15 and 16.
- **AK-9** `openFile` hands the window the selection asked for, by text and by lines; a file
  outside the project, a path with `..` and a missing file are refused.
- **AK-10** An accepted diff answers `FILE_SAVED` with the accepted text and the link writes
  nothing; a rejected diff and a rejected new file answer `DIFF_REJECTED` and leave the disk as it
  was; a path outside the project is refused before a diff is shown.
- **AK-11** `close_tab`, `closeAllDiffTabs`, a replacing proposal of the same name and a
  disconnecting client answer the waiting diff `DIFF_REJECTED`; a late acceptance writes nothing.
- **AK-12** `saveDocument` writes nothing.
- **AK-13** The link's state names the client (name, version, pid), the port and the lock file, and
  never contains the token.
- **AK-14** The MCP tool `claude_code_link` answers `running: false` before the first terminal,
  then the port and lock file without the token; the project's terminal gets
  `CLAUDE_CODE_SSE_PORT` and `ENABLE_IDE_INTEGRATION`; closing the project window stops the link
  and removes the lock file.
- **AK-15** With the real `claude` program started with the variables of a Ferris IDEA terminal tab
  of a debug build, Claude Code connects (`/ide` lists Ferris IDEA as connected, the status bar
  says "Claude Code connected"), a change it proposes opens as a diff in the window, and after
  Accept Claude Code's own edit succeeds and the file holds the change (hand test 2026-11-23,
  recorded in the pull request).

## Out of scope
- `at_mentioned` (sending the selection to Claude Code from the editor's context menu) and the
  Claude Code terminal tab itself (the AI settings' "terminal" mode, another change).
- `executeCode` (Jupyter notebooks).
- Editing the proposal inside the diff viewer before accepting it.
- Windows: the link does not start there until a private lock file can be made.
- Several project folders per window: the link serves the window's one project folder.

## Known deviations
- **Characters** in selections and ranges count Unicode scalar values, as the rest of Ferris IDEA;
  VS Code counts UTF-16 code units and Neovim bytes. Lines, which Claude Code mostly uses, agree.
- **Paths outside the project** are refused for `openFile`, `openDiff` and `getDiagnostics` (the
  open-source plugin and the official integrations open any file). Security first (CLAUDE.md
  rule 12): Claude Code asks in its terminal instead.
- **`saveDocument`** saves nothing (behaviour 21).
- **Origin and Host checks** (behaviour 8) go beyond the token check of the open-source plugin.

## Open questions
- Whether a future Claude Code version expects `executeCode` or `at_mentioned` from every IDE; the
  link answers `-32601` for unknown tools and Claude Code went on without them in the hand test.
