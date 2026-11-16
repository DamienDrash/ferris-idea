# SPEC-0121: Remote host over SSH

- **Version:** 4 · **Date:** 2026-11-07 (v2 2026-11-03: backend and file operations moved to
  SPEC-0158 and SPEC-0253; v3: post-quantum key exchange required, ADR-0025 v3, PO-STATUS question
  29; v4: one start point of `ssh`, every jump host with the post-quantum key exchange, a
  `ProxyCommand` of the user's own refused by default, ADR-0025 v5)
- **Sources:** GEN (IDEs browse a remote host, open its files for editing with automatic upload on
  save, and open an SSH terminal on it; F-083 "Remote"), STD (OpenSSH client: `BatchMode`,
  `StrictHostKeyChecking`, `KexAlgorithms`, `ssh -Q kex`, `ControlMaster`/`ControlPersist`/
  `ControlPath %C`, `ssh -G`, `-t`,
  `Host`/`Include` in `ssh_config`; POSIX `sh`, `cat`, `cksum`, `cp -p`, `mv`, `pwd -P`),
  SPEC-0053 (agent rights and log), SPEC-0058 (saving with a change check), SPEC-0062 (terminal),
  ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Work on files and programs that live on another machine — a server, a build box, a VM —
from the IDE: browse it, edit its files with the usual editor, open a shell there.

## Behaviour
1. **Hosts and connection:** the window lists the `Host` aliases of `~/.ssh/config` and the files
   it includes (patterns left out), each with the user, host name and port `ssh -G` resolves —
   nothing is connected for that. The field also takes `[user@]host[:port]`. Everything goes through
   the system `ssh` with the user's keys, agent, config and known hosts: batch mode (IDECraft never
   asks for or stores a password or passphrase), strict host key checking, 10 s connect timeout,
   a hybrid post-quantum key exchange only (`KexAlgorithms=mlkem768x25519-sha256,
   sntrup761x25519-sha512,sntrup761x25519-sha512@openssh.com`, narrowed to the ones the local
   client knows; ADR-0025 section 1a), one shared connection per host for 60 s in a folder only the
   user can reach. These shared options come before the target's own arguments, so the config
   cannot weaken them; every `ssh` of IDECraft is built by the one start point
   (`idecraft_remote::SshCommand`, v4). Connecting shows the home folder. [STD, ADR-0025]
1a. **v4 – Jump hosts:** a `ProxyJump` of the config (or `-J`) works, and every hop gets the same
   options: the start point resolves the host with `ssh -G` and replaces the jump with IDECraft's own
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
2. **Refusals:** an unknown host key, a changed host key, a refused login, a host without a
   post-quantum key exchange ("<host>: the host offers no post-quantum key exchange (OpenSSH 9.0 or
   later needed there, ML-KEM from 9.9). IDECraft connects only with post-quantum protection
   (ADR-0025); nothing was sent"), a local `ssh` without one (checked once with `ssh -Q kex`
   before the first connection: "update OpenSSH …", never a classical fallback) and an
   unreachable host each have their own message that says what to do; an unknown or changed key is never accepted
   from the IDE (the user checks it in a terminal). Every call has a time limit (30 s; reading and
   writing files 120 s). [STD]
3. **Files on the host:** only POSIX tools run there, so any Unix host works without installing
   anything. A folder lists folders first, then files, each by name, hidden entries included,
   links marked. A file is read up to 10 MiB with its POSIX `cksum`. Writing goes to a temporary
   file in the same folder that keeps the old file's mode and is renamed into place; with the
   stamp from reading, a file that changed on the host meanwhile is left alone. Arguments are
   quoted for the host's shell, so names with spaces, quotes or `$` stay names. [STD]
4. **Editing:** Enter on a file copies it into the user's cache folder
   (`…/idecraft/remote/<host>/<path>`) and opens the copy, so highlighting, search, undo and local
   history work as for local files. Saving sends it back (status bar "Uploaded to host:path").
   If the host's file changed since it was opened, nothing is sent and the status bar says so;
   saving again overwrites it. Opening the file again reads the host's current copy. [GEN]
5. **Window:** `Remote.ToolWindow` opens "Remote Host" at the bottom: a field ("Host" or "Path")
   with a placeholder and focus border, the list, the connected `host:path`. Enter on a host
   connects, on a folder or `..` goes there, on a file opens it; a path typed into the field
   (absolute, `~…` or relative to the folder shown) and Enter goes there; ⌥↑ goes up; F5 reads
   again; "Hosts" leaves the host; Escape and ⇧Esc hide the window. Errors and an empty folder are
   said in the list. [GEN]
6. **SSH terminal:** "SSH Terminal" (`Remote.OpenTerminal`) opens a terminal tab with an
   interactive login (`ssh -t`) that starts in the folder shown, using the same options as above.
   [GEN, SPEC-0062]
7. **MCP:** `remote_hosts` (no connection); `remote_list`, `remote_read` (content and stamp),
   `remote_write` (`expected_stamp`; `dry_run` default true; size logged, not the content) need
   the user-granted right `ssh`; `remote_exec` (no shell, `cwd`, time limit up to 600 s, `dry_run`
   default true) needs `ssh` and `run`. Writes and runs are logged in `.idecraft/agent-log.jsonl`.
   [ADR-0008, SPEC-0053 v3]

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
  save it back, a host-side change refused once then overwritten, SSH terminal in the folder,
  back to the hosts, Escape.
- **AK-5** Hand test (round 4): a real host from `~/.ssh/config`, next to the reference's remote
  host browser and SSH session.
- **AK-6** SEC-17 (ADR-0025 v3): a test `sshd` limited to classical key exchanges is refused with
  the post-quantum message and nothing runs there, although OpenSSH's defaults reach it; a current
  test `sshd` is accepted and `ssh -v` with the shared options reports an allowed method; every
  product file that starts `ssh` uses the shared options (workspace scan); unit tests for the
  client list, the stricter `IDECRAFT_SSH_KEX=mlkem` variant and the refusal texts
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

## Out of scope
A remote IDE backend (now SPEC-0158: search, symbols, problems and run configurations on the
host), automatic
sync of a whole project, SFTP/FTP/WebDAV, password and keyboard-interactive logins, jump host UI
(works through `ProxyJump` in the config, behaviour 1a), creating, renaming and deleting remote files (now
SPEC-0253), Windows hosts.

## Known deviations from the reference
SSH goes through the system client instead of a built-in one, so config, agent, hardware keys
and `ProxyJump` work exactly as in the terminal, and IDECraft keeps no credentials; host keys are
only accepted outside the IDE, and saving checks that the host's file did not change (IMP-0044).
IDECraft refuses hosts without a post-quantum key exchange, where a terminal `ssh` before OpenSSH
10 may silently use a classical one (ADR-0025 v3), and holds every jump host to the same rule, where
OpenSSH opens them with its own defaults; a `ProxyCommand` of the user's own needs the user's consent
(ADR-0025 v5).
Files are edited as local copies with upload on save rather than through a remote backend. Not
compared side by side yet.
