# SPEC-0301: Report a problem with a database system

- **Version:** 1 · **Date:** 2026-11-05
- **Sources:** The project owner 2026-11-05 (destination: GitHub Issues, prefilled from templates; PO-STATUS), GEN (bug report templates: version, environment, steps, expected and actual result;
  issue trackers accept prefilled titles and bodies in the new-issue URL), ADR-0007 (no telemetry:
  network only for actions the user starts, visibly), ADR-0008, ADR-0020, SPEC-0300
- **Feature matrix rows:** F-060

## Purpose
When a Beta (or any) database system fails, the user can hand the maintainers a precise report in
two clicks — without IDECraft sending anything by itself and without leaking credentials, data or
host names.

## Behaviour
1. **Where.** "Report a problem with this database…" is offered: in the connection dialog as a link
   next to Test Connection after a failed test and whenever the chosen system is Beta, User or
   Imported (a failed test's message becomes the error, step `connect`); and as Tools › Database ›
   "Report a Problem with This Database…" and in Find Action (`Database.ReportProblem`) for the
   connection selected in the Database tool window. [GEN]
2. **What is collected — locally, nothing is sent.** IDECraft version and platform (`macos-arm64`),
   the system (connector id, name, status, origin, definition `format` and, for user or imported
   ones, a SHA-256 of the definition file), the family, the server version if a session was open
   (the connector's `version_query`, else the family's), the failing step (`connect`, `test query`,
   `list schemas`, `introspect`, `run statement`, `explain`, `edit`, `other`) and its error message,
   and a field "What did you expect?" left empty for the user. [ADR-0007]
3. **What is never in it.** Passwords and keychain references, the user name, SQL text and result
   values. Host names, IP addresses, ports, database and schema names, file paths and the profile
   name are replaced by placeholders (`<host>`, `<port>`, `<database>`, `<schema>`, `<path>`,
   `<user>`) wherever they occur, including inside the error message, before the user sees the
   report. A checkbox "Include host and database names" (off) puts the real ones back for this
   report only. A failing statement is included only when the user ticks "Include the statement"
   (off), and then as written in the editor. [ADR-0007]
4. **Preview and edit.** The report opens in a dialog as plain Markdown text the user can change
   freely: a title line (`[<system>] <step> fails: <first line of the error>`, ≤ 120 characters)
   and a body with the sections of behaviour 2. Buttons: **Copy** (title and body to the clipboard),
   **Open issue in browser** (opens the configured destination with title and body as URL query
   parameters, cut to 6 000 characters of body with a note "(shortened — paste the full report
   from the clipboard)", and copies the full report first), **Cancel**. Nothing is sent by IDECraft
   itself; opening the browser is the only network effect and only on the user's click. [GEN,
   ADR-0007]
5. **Destination.** GitHub Issues (project owner, 2026-11-05): by default "Open issue in browser" opens a
   new issue from the template `database-system-problem.md` of IDECraft's repository with title and
   body filled in (usable by others once the repository is public, GO-4). The setting
   `[database] report_url` in the user settings file (`settings.toml`, SPEC-0129; agents set it with
   `set_ide_settings` → `database_report_url`) replaces it: an `https://` template with `{title}`
   and `{body}` (URL-encoded on use), or `off` for Copy only; empty means the default. A definition
   may name its own `report_url` (top-level key of the SPEC-0300 format, https only) that wins for
   that system. Only `https://` templates are accepted. [project owner, GEN]
6. **MCP.** `database_problem_report` (read) takes a connection id or a connector id, the step, the
   error message and optionally `include_hosts`, returns the title, body and the destination URL
   that the user would open; it never opens a browser or sends anything. [ADR-0008]

## Acceptance criteria
- **AK-1** Redaction: host, IP (v4 and v6), port, database, schema, user, profile name, file paths
  and a `password=` fragment in an error message are replaced; with `include_hosts` the host and
  database stay; the statement is absent unless asked for (unit tests `report::tests`).
- **AK-2** Title format and length, body sections, definition hash for user and imported systems,
  server version present only when known (unit tests).
- **AK-3** Destination: the GitHub template by default, `off` gives none, template filled and
  encoded, body cut at 6 000 characters with the note, non-https templates refused, a definition's
  `report_url` wins (unit tests).
- **AK-4** MCP `database_problem_report` for a connector and for a connection (protocol test).
- **AK-5** Window self-test steps: selecting a Beta system shows the link; the report dialog opens
  with the redacted text; Copy fills the clipboard; Escape closes it.

## Out of scope
Sending reports from IDECraft, attachments, logs or crash dumps, any automatic collection.

## Known deviations from the reference
Problem reports are written and edited by the user and never sent by the IDE (ADR-0007, IMP-0298).
