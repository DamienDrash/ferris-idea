# SPEC-0121: Remote host over SSH

- **Version:** 7 · **Date:** 2026-11-23 (v7: a host file that changed since it was read is
  never overwritten by a plain save: Merge, Compare, Overwrite or Cancel, behaviour 4a;
  `remote_write` answers a stale stamp with a conflict that names both stamps, and a missing
  stamp as missing, behaviour 7; the host list follows its files, behaviour 5a; IMP-0543,
  OBS-0118 gaps R1, R4 and R5; v6 2026-11-19: the unknown-host-key message names the terminal
  command that reaches a Ferris IDEA host, `ssh -F '<file>' <alias>`; Git over SSH and the Compose
  CLI's `ssh` resolve Ferris IDEA's own aliases with the same rule as the start point, behaviour 1b;
  v5: Ferris IDEA's own host list next to `~/.ssh/config`,
  behaviour 1b, Add Host… and Remove Host in the window, `remote_host_add` and
  `remote_host_remove`, IMP-0531; v4 2026-11-07; v2 2026-11-03: backend and file operations moved to
  SPEC-0158 and SPEC-0253; v3: post-quantum key exchange required, ADR-0025 v3, PO-STATUS question
  29; v4: one start point of `ssh`, every jump host with the post-quantum key exchange, a
  `ProxyCommand` of the user's own refused by default, ADR-0025 v5)
- **Sources:** GEN (IDEs browse a remote host, open its files for editing with automatic upload on
  save, and open an SSH terminal on it; F-083 "Remote"), STD (OpenSSH client: `BatchMode`,
  `StrictHostKeyChecking`, `KexAlgorithms`, `ssh -Q kex`, `ControlMaster`/`ControlPersist`/
  `ControlPath %C`, `ssh -G`, `-t`,
  `Host`/`Include`/`Match`/`HostName`/`Port`/`User`/`IdentityFile`/`IdentitiesOnly` in
  `ssh_config`, `ssh -F`; POSIX `sh`, `cat`, `cksum`, `cp -p`, `mv`, `pwd -P`),
  SPEC-0053 (agent rights and log), SPEC-0058 (saving with a change check), SPEC-0062 (terminal),
  SPEC-0037 and SPEC-0101 (three-way merge and the merge view), SPEC-0084 (diff viewer), ADR-0008,
  OBS-0118 (gap R1)
- **Feature matrix rows:** F-083

## Purpose
Work on files and programs that live on another machine — a server, a build box, a VM —
from the IDE: browse it, edit its files with the usual editor, open a shell there.

## Behaviour
1. **Hosts and connection:** the window lists the `Host` aliases of `~/.ssh/config` and the files
   it includes (patterns left out), each with the user, host name and port `ssh -G` resolves —
   nothing is connected for that. The field also takes `[user@]host[:port]`. Everything goes through
   the system `ssh` with the user's keys, agent, config and known hosts: batch mode (Ferris IDEA never
   asks for or stores a password or passphrase), strict host key checking, 10 s connect timeout,
   a hybrid post-quantum key exchange only (`KexAlgorithms=mlkem768x25519-sha256,
   sntrup761x25519-sha512,sntrup761x25519-sha512@openssh.com`, narrowed to the ones the local
   client knows; ADR-0025 section 1a), one shared connection per host for 60 s in a folder only the
   user can reach. These shared options come before the target's own arguments, so the config
   cannot weaken them; every `ssh` of Ferris IDEA is built by the one start point
   (`ferris_idea_remote::SshCommand`, v4). Connecting shows the home folder. [STD, ADR-0025]
1a. **v4 – Jump hosts:** a `ProxyJump` of the config (or `-J`) works, and every hop gets the same
   options: the start point resolves the host with `ssh -G` and replaces the jump with Ferris IDEA's own
   `ProxyCommand`, a private stand-in that opens each hop with the shared options and `-W` to the
   next one (several hops, `user@host:port`, a hop's own `ProxyJump`; at most 8). A hop without a
   post-quantum key exchange ends the connection before anything reaches the host: "<host>: the jump
   host <hop> offers no post-quantum key exchange …; nothing was sent to <host>". Without a jump the
   direct route is pinned (`ProxyCommand=none`). A `ProxyCommand` of the user's own is refused ("<host>:
   the SSH configuration runs its own ProxyCommand …; nothing was started. Use ProxyJump instead, or
   allow it …") unless Settings › Version Control "Allow a ProxyCommand from the SSH configuration"
   (`ssh.allowOwnProxyCommand`, off by default) is on; the setting then shows a warning and each
   such connection writes a line (time, host) to `ssh-proxy-command.log`. MCP can only switch it
   off. [STD, ADR-0025 v5, deviation: OpenSSH opens jump hosts with its own defaults]
1b. **v5 – Ferris IDEA's own hosts:** besides the hosts of `~/.ssh/config`, Ferris IDEA keeps a host list
   of its own, so a host (a throwaway VM, a test box) can be added without editing the user's
   configuration. [GEN, STD, DOC, IMP-0531]
   - **File:** `<settings folder>/ssh/config` (the settings folder of SPEC-0053: `$FERRIS_IDEA_CONFIG_DIR`,
     else `$XDG_CONFIG_HOME/ferris-idea`, else `~/.config/ferris-idea`); the folder `ssh` is made with
     mode 0700, the file is written with mode 0600 through a temporary file in the same folder that
     is renamed into place. It is OpenSSH configuration, so `ssh -F <file> <alias>` in a terminal
     reaches the same host. It starts with a comment that says Ferris IDEA writes it; one `Host`
     block per host with `HostName`, `Port` and `User` when given, and with an identity file
     `IdentityFile` (by path; the key is never copied or read) and `IdentitiesOnly yes`, so only
     that key is offered. After the blocks `Match all` and `Include <home>/.ssh/config` and
     `Include /etc/ssh/ssh_config` follow (the home folder written as an absolute path when the
     file is written), so the user's own defaults (agent, `Host *` options, known hosts) still
     apply to Ferris IDEA's hosts, after the host's own lines as OpenSSH keeps the first value.
   - **Use:** a connection to an alias of this list (also `user@alias`) gets `-F <file>` from the
     one start point (behaviour 1), in front of the caller's own arguments; every other host is
     reached exactly as before, without `-F`. Arguments that already name a `-F` configuration
     (tests) replace both lists. Everything of behaviour 1 and 1a holds unchanged: batch mode,
     strict host key checking (an unknown host key is refused with what to do, as before; the file
     never sets `StrictHostKeyChecking` or `UserKnownHostsFile`), the post-quantum key exchange, the
     route over jump hosts. No password or passphrase is asked or kept: keys and agent only.
   - **List:** the window and `remote_hosts` list Ferris IDEA's hosts first, then the hosts of
     `~/.ssh/config`; each host says where it comes from (`Ferris IDEA` or `~/.ssh/config`). An
     alias in both is listed once, as Ferris IDEA's (its block comes first for `ssh`).
   - **Adding** checks every value before anything is written, so nothing typed can add an option
     or a line of its own: the alias is 1–64 letters, digits, `.`, `_`, `-`, starting with a letter
     or digit (no patterns), the host name 1–253 letters, digits, `.`, `-`, `:` starting with a
     letter, digit or `:`, the user 1–64 letters, digits, `.`, `_`, `-` not starting with `-`, the
     port 1–65535; the identity file is an absolute path or `~/…` (written absolute), an existing
     file, not a `.pub` file, without control characters, quotes, backslashes or `%`. An alias that
     Ferris IDEA's list or `~/.ssh/config` already has is refused ("<alias> is already a host in …").
     Each refusal names the field and what is allowed.
   - **Removing** takes Ferris IDEA's hosts only; the user's `~/.ssh/config` and the folder `~/.ssh`
     are never written, and its hosts are shown read-only.
   - **v6 – programs that start `ssh` themselves:** Git (`GIT_SSH_COMMAND`, SPEC-0055 v5) and the
     Compose CLI (`ssh` from `PATH`) start `ssh` through Ferris IDEA's stand-ins, which only see the
     arguments at run time. Each stand-in carries the path of Ferris IDEA's file and applies the same
     rule as the start point when it runs: when the arguments name no `-F`, the file exists and the
     host that `ssh -G` reports for the arguments (the destination as given, without `user@`) is
     the alias of one of its `Host` blocks (exact comparison, as the start point's), it adds `-F
     <file>` in front of the arguments; the route over jump hosts, the hops and the Azure DevOps
     check of SPEC-0055 then use that file too. Every other destination is reached exactly as
     before, without `-F`. A host added or removed later is seen by the next connection, without
     writing the stand-in again. A process without host files (tests) writes the stand-ins
     without this step. When no stand-in can be written, Git falls back to the strict command line
     as before, without Ferris IDEA's aliases. Host key checking stays strict and nothing is written
     to `~/.ssh`. [STD, ADR-0025 v5]
2. **Refusals:** an unknown host key, a changed host key, a refused login, a host without a
   post-quantum key exchange ("<host>: the host offers no post-quantum key exchange (OpenSSH 9.0 or
   later needed there, ML-KEM from 9.9). Ferris IDEA connects only with post-quantum protection
   (ADR-0025); nothing was sent"), a local `ssh` without one (checked once with `ssh -Q kex`
   before the first connection: "update OpenSSH …", never a classical fallback) and an
   unreachable host each have their own message that says what to do; an unknown or changed key is never accepted
   from the IDE (the user checks it in a terminal). **v6:** the unknown-key message names the
   terminal command that reaches the host as Ferris IDEA does: "<host>: the host key is not known yet. Connect once in a terminal
   (`<command>`), check the fingerprint and accept it there", where `<command>` is `ssh -F
   '<Ferris IDEA's file>' <host>` for one of Ferris IDEA's own hosts (also `user@alias`; the rule of
   behaviour 1b) and `ssh <host>` for every other host. The file is always in single quotes (a
   quote inside written `'\''`), the host only when it holds other characters than letters, digits
   and `-_./=:@%+,`, so the command can be copied into a POSIX shell as it is. The window, MCP
   errors and the database tunnel show the same text. Every call has a time limit (30 s; reading
   and writing files 120 s). [STD]
3. **Files on the host:** only POSIX tools run there, so any Unix host works without installing
   anything. A folder lists folders first, then files, each by name, hidden entries included,
   links marked. A file is read up to 10 MiB with its POSIX `cksum`. Writing goes to a temporary
   file in the same folder that keeps the old file's mode and is renamed into place; with the
   stamp from reading, a file that changed on the host meanwhile is left alone. Arguments are
   quoted for the host's shell, so names with spaces, quotes or `$` stay names. [STD]
4. **Editing:** Enter on a file copies it into the user's cache folder
   (`…/ferris-idea/remote/<host>/<path>`) and opens the copy, so highlighting, search, undo and local
   history work as for local files. Saving sends it back (status bar "Uploaded to host:path").
   Opening the file again reads the host's current copy. Ferris IDEA keeps the version it read
   last (the content and its stamp, the POSIX `cksum` CRC and size) as the base of the copy; an
   upload that succeeds makes the uploaded content the new base. [GEN]
4a. **v7 – Changed on the host:** if the host's file changed since the base was read (its stamp
   differs, or it is gone), nothing is sent, ever, by a save alone; saving again does not
   overwrite it either (before v7 it did). Ferris IDEA reads the host's version and asks in the
   window "<name> changed on the host since you opened it", the detail naming `host:path`, how
   many lines each side changed against the base and what Overwrite would discard ("Overwrite
   discards the host's change: N lines …", or "the host deleted the file"). The answers:
   - **Merge…** (default, Return): the merge view (SPEC-0101) with the base in the middle, "Your
     copy" on the left and "On the host" on the right; changes that do not conflict are taken from
     both sides at once, conflicts are resolved as in a Git merge. Apply puts the result into the
     editor and uploads it with the host version as the expected stamp, so a third change made on
     the host meanwhile asks again. Closing the merge view sends nothing.
   - **Compare**: the diff viewer, "Your copy" against "On the host"; nothing is sent, the next
     save asks again.
   - **Overwrite**: only on its own button, never on Return or Escape; uploads the copy with the
     host version just read as the expected stamp (again refused if the host changed once more).
   - **Cancel** (Escape): nothing is sent; the copy keeps its edits, the host keeps its version,
     and the status bar says that the next save asks again.
   A file that is not text (a NUL byte or invalid UTF-8 on either side) or that the host deleted
   offers Compare, Overwrite (which writes it again) and Cancel only; Compare is the default then. [GEN, STD, deviation: OBS-0118 R1, IMP-0543]
5. **Window:** `Remote.ToolWindow` opens "Remote Host" at the bottom: a field ("Host" or "Path")
   with a placeholder and focus border, the list, the connected `host:path`. Enter on a host
   connects, on a folder or `..` goes there, on a file opens it; a path typed into the field
   (absolute, `~…` or relative to the folder shown) and Enter goes there; ⌥↑ goes up; F5 reads
   again; "Hosts" leaves the host; Escape and ⇧Esc hide the window. Errors and an empty folder are
   said in the list. **v5:** in the host list each host row ends with its source ("Ferris IDEA" or
   "~/.ssh/config"). "Add Host…" shows a form in the window with the fields Alias, Host name, Port,
   User and Identity file (the last three optional), Tab and ⇧Tab move between them, Enter or
   "Add" adds the host, Escape or "Cancel" leaves the form; a refusal stays in the form with the
   values typed. "Remove Host" (and ⌘⌫, Ctrl+Backspace outside macOS) on one of Ferris IDEA's hosts asks
   "Remove <alias> from Ferris IDEA's hosts? Enter removes, Escape keeps it"; on a host of
   `~/.ssh/config` it says that Ferris IDEA does not change that file. [GEN]
5a. **v7 – Host list follows its files:** while the window is open, the host list is read again
   within about a second whenever the SSH configuration (`~/.ssh/config`, or a `-F` file of the
   caller's own) or Ferris IDEA's own host list (`<settings folder>/ssh/config`, behaviour 1b) is
   changed, created or removed (size and modification time are compared every second; no
   restart). The selected host stays selected if it is still listed. Files the configuration
   includes are not watched. [GEN, deviation: OBS-0118 R4]
6. **SSH terminal:** "SSH Terminal" (`Remote.OpenTerminal`) opens a terminal tab with an
   interactive login (`ssh -t`) that starts in the folder shown, using the same options as above.
   [GEN, SPEC-0062]
7. **MCP:** `remote_hosts` (no connection); `remote_list`, `remote_read` (content and stamp),
   `remote_write` (`expected_stamp`; `dry_run` default true; size logged, not the content) need
   the user-granted right `ssh`; v7: a stale `expected_stamp` writes nothing and fails with
   `conflict`: the message and the error data name `path`, `expected_stamp` and the host's
   `host_stamp` (`absent` when the file is gone), and only a call that passes the host's stamp
   as `expected_stamp` overwrites — there is no unconditional write for agents. A call without
   `expected_stamp` only creates a new file; when the file exists it fails with "expected_stamp
   is missing: … already exists …; call remote_read for its content and stamp" (error data
   `missing_expected_stamp`, `host_stamp`), never with the conflict message (OBS-0118 R5); `remote_exec` (no shell, `cwd`, time limit up to 600 s, `dry_run`
   default true) needs `ssh` and `run`. Writes and runs are logged in `.ferris-idea/agent-log.jsonl`.
   [ADR-0008, SPEC-0053 v3] **v5:** `remote_hosts` gives each host its `source` (`ferris-idea` or
   `ssh_config`) and the `config` file it comes from. `remote_host_add` (`alias`, `hostname`,
   optional `port`, `user`, `identity_file`; `dry_run` default true answers the checked entry and
   the file it would go to) and `remote_host_remove` (`alias`; `dry_run` default true) change
   Ferris IDEA's host list, a file of the user's settings folder: the change needs the user-granted
   right `settings` (SPEC-0053 v8 behaviour 12), and adding also `ssh`, because the host is a
   destination Ferris IDEA connects to later (as a setting that names a program needs the right to
   run it). Without them nothing is written and the refusal says which right is missing. A host
   of `~/.ssh/config` is never removed ("comes from ~/.ssh/config, which Ferris IDEA never changes").
   Both are logged (tool, alias, applied). [ADR-0008, SPEC-0053 v8]

## Acceptance criteria
- **AK-1** Quoting, refusal classification, ssh options and the terminal command, typed hosts;
  POSIX `cksum` equal to the system tool; config aliases with includes and patterns; cache paths
  (no `..`); window rows (unit tests in `crates/remote`, `remote_view::tests`).
- **AK-2** Against a real `sshd` started as the user with keys made for the test: home, folder
  resolution, listing with hidden entries and links, reading, writing that keeps the mode, a
  change on the host refused, overwrite, new files, no temporary files left; commands with
  quoting, input, exit code, time limit; unknown and changed host keys, refused login, closed port
  (`crates/remote/tests/it/ssh.rs`).
- **AK-3** MCP: refusal without `ssh`, dry run without any right, list/read/write with a stale
  stamp refused, `remote_exec` refused without `run` and run with both rights, log entries without
  content (`crates/mcp/tests/it/protocol.rs`: `remote_host_tools`).
- **AK-4** Window self-test against the test `sshd`: connect, go to a typed path, open a file,
  save it back, a host-side change refused (v7: with the question of behaviour 4a), SSH
  terminal in the folder, back to the hosts, Escape.
- **AK-5** Hand test (round 4): a real host from `~/.ssh/config`, next to the reference's remote
  host browser and SSH session.
- **AK-6** SEC-17 (ADR-0025 v3): a test `sshd` limited to classical key exchanges is refused with
  the post-quantum message and nothing runs there, although OpenSSH's defaults reach it; a current
  test `sshd` is accepted and `ssh -v` with the shared options reports an allowed method; every
  product file that starts `ssh` uses the shared options (workspace scan); unit tests for the
  client list, the stricter `FERRIS_IDEA_SSH_KEX=mlkem` variant and the refusal texts
  (`crates/remote/tests/it/pq_kex.rs`, `pq::tests`).
- **AK-7** (v4) SEC-17 (ADR-0025 v5): against a post-quantum and a classical test `sshd` as jump hosts
  and two targets, a classical hop is refused alone, first or second in a chain, through a hop's own
  `ProxyJump` and as `-J`, by the start point and by the Git and Compose stand-ins, although plain
  OpenSSH reaches the target through it; one or two post-quantum hops are accepted; a `ProxyCommand`
  of the user's own (target or hop) is refused unless the setting is on, then logged without the
  command (`crates/remote/tests/jump.rs`, `jump::tests`, `launch::tests`). The workspace scan finds
  `ssh` started directly, through a `let`, a parameter or a field and allows only `SshCommand`; the
  scanner's own test covers these variants (`pq_kex::the_scan_finds_ssh_through_variables_too`,
  `pq_kex::ssh_starts_only_at_the_one_start_point`).
- **AK-8** (v5) Ferris IDEA's own hosts: the file is written with 0600 in a 0700 folder, through a
  rename; its text parses back to the same hosts; alias, host name and user with spaces, line
  breaks, a leading `-`, `=` or quotes, patterns, a port 0, a relative, missing, `.pub` or `%`
  identity file are each refused and nothing is written; an alias already in either list is
  refused; removing a host of `~/.ssh/config` is refused and that file stays byte for byte the
  same; the start point adds `-F <file>` for an own alias (also `user@alias`) and for nothing else,
  and not when a `-F` is given; the host list names each source (`own_hosts::tests`,
  `config::tests`, `launch::tests`). Against the test `sshd` with a test home folder and settings
  folder: a host added with the test key is listed with its source, `ssh -G` resolves it, a
  command runs there through the start point, an unknown host key is refused as before, and after
  removing it the alias is gone; the user's `~/.ssh` and settings folder are never touched
  (`crates/remote/tests/it/own_hosts.rs`). MCP: `remote_hosts` shows the source; `remote_host_add`
  dry run writes nothing, applying is refused without `settings` and without `ssh`, then adds;
  `remote_host_remove` removes, refuses a `~/.ssh/config` host; log entries
  (`crates/mcp/tests/it/own_ssh_hosts.rs`). macOS window test: Add Host… with typed fields, a
  refusal kept in the form, the host listed as Ferris IDEA's, connected to the test `sshd` and its
  home folder shown, Remove Host asked and done, a `~/.ssh/config` host not removable
  (`crates/workbench/tests/own_ssh_hosts.rs`).
- **AK-9** (v6) The unknown-key message names `ssh -F '<file>' <alias>` for an own alias (also
  `user@alias`, a file path with a space and a quote quoted for the shell) and `ssh <host>` for any
  other host or when own hosts are off (`ssh::tests`). Git's and the Compose CLI's stand-ins carry
  Ferris IDEA's file only when host files are set, add `-F <file>` for an own alias and for nothing
  else, not when the arguments name a `-F`, and see a host added after they were written
  (`pq_git::tests`, run with a stand-in `ssh` that records its arguments). Against the test `sshd`
  with a test home and settings folder: a clone from `ssh://<alias>/<path>`, a fetch and a push to
  `user@<alias>:<path>` through Ferris IDEA's Git operations reach a repository on the host; an own
  alias whose host key is unknown fails in Git with `auth_required` and Git's "Host key
  verification failed", and the start point's refusal names `ssh -F '<file>' <alias>`
  (`crates/vcs/tests/it/own_ssh_hosts.rs`).

- **AK-10** (v7) Against the test `sshd`: a stale stamp fails with both stamps and leaves the host
  file alone, the host's stamp then overwrites (`crates/remote/tests/it/ssh.rs`); MCP
  `remote_write` with a stale stamp answers `conflict` with `expected_stamp` and `host_stamp`
  and writes nothing, without a stamp on an existing file it says the stamp is missing
  (`protocol.rs`: `remote_host_tools`); in a window of the test harness with
  real keys (`crates/workbench/tests/it/f083_remote_r1r2.rs`): a save after a host-side change
  asks with the four answers and sends nothing, a second save asks again, Cancel keeps both
  versions, Compare opens the diff of the two, Merge with Apply uploads a result that holds the
  host's line and the editor's line, Overwrite uploads the copy; unit tests of the question text
  and the line counts (`remote_conflict::tests`).

- **AK-11** (v7) In a window of the test harness against the test `sshd`
  (`f083_remote_r1r2.rs::the_host_list_follows_its_file_and_keeps_the_selection`): a host added to
  the SSH configuration while the window is open is listed without a restart, before the selected
  host, and the selection stays on the selected host.

## Out of scope
A remote IDE backend (now SPEC-0158: search, symbols, problems and run configurations on the
host), automatic
sync of a whole project, SFTP/FTP/WebDAV, password and keyboard-interactive logins, jump host UI
(works through `ProxyJump` in the config, behaviour 1a), creating, renaming and deleting remote files (now
SPEC-0253), Windows hosts.

## Known deviations from the reference
SSH goes through the system client instead of a built-in one, so config, agent, hardware keys
and `ProxyJump` work exactly as in the terminal, and Ferris IDEA keeps no credentials; host keys are
only accepted outside the IDE, and saving checks that the host's file did not change (IMP-0044); since v7 a change on the host is
merged or compared instead of being overwritten by a second save, and no answer to the question
loses either side without naming it (IMP-0543).
Ferris IDEA refuses hosts without a post-quantum key exchange, where a terminal `ssh` before OpenSSH
10 may silently use a classical one (ADR-0025 v3), and holds every jump host to the same rule, where
OpenSSH opens them with its own defaults; a `ProxyCommand` of the user's own needs the user's consent
(ADR-0025 v5).
Ferris IDEA's own host list is an OpenSSH configuration file that `ssh -F` uses as it is, holds no
password or passphrase and only refers to key files by path, where the reference keeps its own SSH
configurations in its own settings format and can save a password or passphrase there (IMP-0531,
v5); Git and the Compose CLI reach these hosts too, and the unknown-key message names the
`ssh -F` command that works in a terminal (v6).
Files are edited as local copies with upload on save rather than through a remote backend.
Compared side by side in OBS-0118.
