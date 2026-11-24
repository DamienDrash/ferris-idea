# SPEC-0055: Git remotes — tracking, fetch, pull, push

- **Version:** 5 · **Date:** 2026-11-19 (v5: Git reaches Ferris IDEA's own SSH hosts, SPEC-0121 v6
  behaviour 1b; v4 2026-11-07; v1 2026-10-31; v2: Git over SSH requires a post-quantum
  key exchange, ADR-0025 v3, PO-STATUS question 29; v3: the Azure DevOps exception, ADR-0025 v4,
  PO-STATUS question 30; v4: jump hosts with the post-quantum key exchange, ADR-0025 v5)
- **Sources:** GEN (IDEs show ahead/behind counts, fetch, pull and push with a preview of the commits to
  send, and report rejected pushes; F-074), STD (the `git` command line: `remote get-url`, `for-each-ref`
  with `%(upstream:track)`, `fetch --prune --porcelain`, `push --porcelain`, `--force-with-lease`,
  `--set-upstream`, `GIT_TERMINAL_PROMPT`, `credential.interactive`, SSH `BatchMode`), SPEC-0036 (merge),
  SPEC-0053 (rights and log)
- **Feature matrix rows:** F-074

## Purpose
Work with the shared repository: see how a branch relates to its upstream, get others' commits and publish
one's own, safely and without ever hanging on a password prompt.

## Behaviour
1. Remotes are listed with name, fetch URL and push URL. [STD]
2. Every local branch is listed with whether it is current, its upstream, the commits ahead and behind as
   of the last fetch (the numbers `git status` shows) and whether the upstream is gone. [STD]
3. Network operations run the installed `git`, so the user's credential helpers, SSH configuration and
   transports apply; Ferris IDEA never sees or stores credentials. Git never prompts: no terminal prompt, no
   askpass program, credential helpers non-interactive, SSH in batch mode. A missing or wrong credential
   fails at once and is reported as `auth_required`; a time limit (120 s) ends the whole process group.
   [STD, improvement IMP-0029]
3a. **Git over SSH** uses the shared `ssh` options of SPEC-0121 through `GIT_SSH_COMMAND` (batch mode,
   strict host keys, hybrid post-quantum key exchange only); it replaces a `GIT_SSH_COMMAND`, `GIT_SSH`,
   `GIT_SSH_VARIANT` or `core.sshCommand` of the user (`~/.ssh/config` still applies). A remote whose SSH
   server offers no post-quantum key exchange, or a local `ssh` without one, fails with `ssh_refusal`
   naming the host and what to update, not with `auth_required`. Checked 2026-11-07: GitHub, GitLab,
   Bitbucket and Codeberg negotiate `mlkem768x25519-sha256`; Azure DevOps offers only classical exchanges
   (behaviour 3b). **v4:** `GIT_SSH_COMMAND` is always Ferris IDEA's stand-in: it resolves the host with
   `ssh -G` and routes a `ProxyJump` through Ferris IDEA's hop stand-in, so every jump host needs a
   post-quantum key exchange too; a `ProxyCommand` of the user's own is refused unless
   `ssh.allowOwnProxyCommand` is on (SPEC-0121 v4 behaviour 1a); it never reuses a shared connection
   the user opened by hand. [STD, ADR-0025] **v5:** a remote URL whose host is one of Ferris IDEA's own
   SSH hosts (`<alias>:<path>`, `ssh://[user@]<alias>[:port]/<path>`; SPEC-0121 v5 behaviour 1b)
   reaches that host: the stand-in adds `-F <Ferris IDEA's file>` for it at run time, by the rule of
   SPEC-0121 v6 behaviour 1b, for fetch, pull, push, clone and every other Git command Ferris IDEA runs.
   Every other remote is reached exactly as before. Host keys stay strict: an own host whose key is
   not known yet fails like any other (`auth_required`, Git's "Host key verification failed"), and
   `ssh -F '<file>' <alias>` in a terminal accepts it. [STD, ADR-0025]
3b. **Azure DevOps exception** (ADR-0025 v4 section 1a, PO-STATUS question 30). While the setting
   `git.ssh.allowClassicKexForAzureDevOps` is on (default) and `FERRIS_IDEA_SSH_KEX` is not `mlkem`,
   `GIT_SSH_COMMAND` is a private stand-in that resolves the host with `ssh -G` (Git's own arguments)
   and, only when the resolved `hostname` is exactly `ssh.dev.azure.com` or `vs-ssh.visualstudio.com`,
   allows `diffie-hellman-group18-sha512` and `diffie-hellman-group16-sha512` after the post-quantum
   methods; every other host gets the strict options. Host keys stay strict. A fetch, pull or push that
   ran without a post-quantum exchange reports `pq: false` and `pq_notice` (host, method, "use HTTPS with
   a token, or SSH once Azure DevOps offers a post-quantum key exchange"), the window's status text adds
   the warning once per session, and a line (time, subcommand, host, method; no URL or credential) goes
   to `git-ssh-kex.log` in the Ferris IDEA log folder. The setting is on Settings → Version Control and in
   `settings.toml` (`[git.ssh] allowClassicKexForAzureDevOps`); MCP `set_ide_settings` can only switch it
   off (`git_ssh_allow_classic_kex_for_azure_devops: false`). A post-quantum method offered by Azure is
   chosen automatically. [ADR-0025]
4. Fetch (one remote or all) prunes deleted remote branches and reports every reference update (new,
   fast-forward, forced, pruned) from git's porcelain output. It changes no local branch or file. [STD]
5. Pull fetches the upstream's remote and merges the upstream into the current branch with SPEC-0036
   (fast-forward or merge commit). A merge that would conflict, or local changes in the way, is reported
   and not attempted. Without an upstream or on a detached HEAD, pull is refused before any network
   access. [GEN, STD]
6. Push sends a local branch (default current) to a remote (default the upstream's remote, else `origin`),
   optionally recording the upstream. The preview lists the commits the remote lacks; git's own dry run
   also shows rejections. A rejected push reports git's reason (`fetch first`, `non-fast-forward`). There
   is no plain force push: overwriting needs `force_with_lease` with the commit the remote branch is
   expected to point at, and a stale expectation is refused. [STD, improvement IMP-0029]
7. MCP tools: `git_remotes` (no network, no right), `git_fetch` (right `remote`), `git_pull` and
   `git_push` (dry run without network by default; real runs need the right `remote`). Real network
   operations are logged per SPEC-0053. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Remotes and tracking against a bare repository: ahead 2 / behind 1 equal to `git status -sb`,
  a branch without upstream, a gone upstream after a pruning fetch.
- **AK-2** Pull: preview changes nothing; fast-forward; merge commit when diverged; conflict reported with
  HEAD unchanged; refused without upstream.
- **AK-3** Push: preview with commits and git's dry run; new branch with upstream; rejection after someone
  else pushed, with reason; force with lease refused when stale and applied when current; unknown branch
  or remote refused.
- **AK-4** A remote answering 401 makes fetch fail quickly with `auth_required`, without a prompt.
- **AK-5** MCP: remotes and dry runs without right; fetch, pull, push refused without `remote`; with it
  push, fetch and pull run and are logged.
- **AK-6** SEC-17 (ADR-0025 v3): git's command carries `GIT_SSH_COMMAND` with the shared options, removes
  `GIT_SSH` and pins `ssh.variant=ssh`; a key-exchange refusal becomes `ssh_refusal` with the peer, not
  `auth_required` (`remote::tests`, `vcs_ops_model::tests`).
- **AK-6a** (v4) Jump hosts (ADR-0025 v5): Git's stand-in refuses a classical jump host and a
  `ProxyCommand` of the user's own and reaches the target through two post-quantum hops
  (`crates/remote/tests/jump.rs`); the stand-in carries the shared options and the route
  (`remote::tests::git_over_ssh_gets_the_shared_options`, `pq_git::tests`).
- **AK-7** Azure DevOps exception (ADR-0025 v4): exact host comparison (case, trailing dot, sub- and
  look-alike domains refused); an alias resolving to Azure gets the classical methods, an Azure-looking
  argument resolving elsewhere does not; non-Git paths to an Azure host name stay post-quantum only; no
  SHA-1, `group1`, `group14` or group-exchange method in the list; setting off or `FERRIS_IDEA_SSH_KEX=mlkem`
  gives the strict command; `pq: false`, notice once per session, log line without arguments
  (`pq_git::tests`, `pq_kex::azure_exception_follows_the_resolved_host_name`,
  `pq_kex::non_git_paths_to_azure_get_no_exception`, `remote::tests`, `vcs_ops_model::tests`); the
  ignored `pq_kex::azure_devops_still_lacks_post_quantum` re-checks Azure nightly.
- **AK-8** (v5) A remote URL with a Ferris IDEA alias: against the test `sshd`, clone, fetch and push
  reach the repository on the host, an unknown host key of an own alias is `auth_required`; the
  stand-in adds `-F` for own aliases only (`crates/vcs/tests/it/own_ssh_hosts.rs`,
  `pq_git::tests`; SPEC-0121 v6 AK-9).

## Out of scope
Rebase on pull, stash, cherry-pick, tags, submodules, the conflict resolution view (M3.7), remote
management (add/rename/remove), credential management.

## Known deviations from the reference
Not compared side by side. The reference offers plain force push; Ferris IDEA only force-with-lease, and
never prompts for credentials (IMP-0029).
