# ADR-0031: Project files never grant agents anything

- **Status:** accepted · **Version:** 1 · **Date:** 2026-11-14 · applies the Project Owner's rule
  "security first" (CLAUDE.md rule 12) to two findings of the security review of 2026-11-14
- **Related:** [ADR-0008](0008-mcp-server.md) (MCP server), SPEC-0053 v8 (agent rights),
  SPEC-0012 v3 (database permissions), SPEC-0278 v2 (trust and safe mode), SPEC-0310 v3 (stores),
  IMP-0007, IMP-0028

## Context

Agent rights have been kept outside the project since SPEC-0053 v1, so that a cloned repository
cannot give an agent the right to run its code. Two things did not follow that rule:

- The four permissions of a database connection were stored in the project file
  `.idecraft/connections.toml`, and reading was on by default. A repository could ship a profile
  with all four, and an agent ran `CREATE TABLE` and `INSERT` through it in a folder that was not
  trusted, with an empty settings folder, without a preview and without a log line. A profile
  with an absolute `path` read a database file outside the workspace that `read_file` refuses.
- Rights were checked where a tool's author remembered them. `set_shortcut` wrote the user's
  keymap and `git_branch_favorite` a project file without any right; the pass over all 308 tools
  for this decision found more of the kind (SPEC-0053 v8 behaviour 13).

## Decision

1. **A file of the project can take away from an agent, never give.** Everything that lets an
   agent do more than read the workspace is decided by the user and stored in the user's settings
   folder. What a project file says about agents is an upper limit or a request.
2. **Database permissions are the user's.** They are kept in the grant file beside the rights,
   per workspace, connection and target. The target is a digest of everything the project file
   says about where the connection leads and what runs on connecting; when the project changes
   that, the grant no longer applies. Nothing is granted by default, reading included.
3. **Safe mode comes first.** In a project that is not trusted an agent uses no connection the
   project describes.
4. **A change is previewed and logged.** A statement that can change data or schema runs for an
   agent only with the permission and with `apply: true`; every use of a connection by an agent
   leaves a line in the agent log without the statement's text.
5. **File databases follow the file rules.** What an agent reaches through SQLite or DuckDB lies
   inside the workspace, or the user granted exactly that file.
6. **Every changing effect has a right, and a test keeps it so.** Each tool has a row in one table
   that names what it changes, how a dry run is asked for and the rights its code checks. The same
   row decides the line of the agent log (SPEC-0053 v7 behaviour 11) and the right (v8 behaviour
   13), so a new tool cannot satisfy one rule and miss the other; the tests fail for a tool
   without a row and for a change without a right. The emergency stop of the UI driver (ADR-0027)
   and `local_history`, which writes only IDECraft's own record, are the two marked exceptions.
7. **The person at the window is not an agent.** None of this changes what the window does for
   its user.

## Consequences

- Existing workspaces: agents lose database access until the user grants it again, once per
  connection, through the question in the window or an entry in the grant file. `ai` lines in
  project files stay valid as upper limits.
- The grant file gets a second kind of table, `[[connection]]`. A build before this decision
  rejects a file with such a table as malformed and grants nothing from it — the safe direction.
- A new right `settings` covers the user's own settings. Granted in one workspace, it lets agents
  there change settings that hold for every project; the question in the window says so.
- Tools that only stop, close or revoke now need a right as well. An agent can no longer take
  back trust or a plugin grant unasked.
- The table is checked against the source text of the handlers, not against their behaviour at
  run time: it proves that a check is written in the tool's code, not that every path reaches it.
  The protocol tests of the single tools cover the paths.

## Alternatives considered

- **Keep the permissions in the project and require trust.** Rejected: trust says the project's
  code may run, not that an agent may write to a database the project names; and it leaves
  reading on by default.
- **A check in the central dispatch from the table.** Rejected for now: a right is checked in the
  step that really changes something, after the dry run was decided, and the dispatch cannot know
  that step. The table is written so that the dispatch can use it later (logging of changing
  calls).
