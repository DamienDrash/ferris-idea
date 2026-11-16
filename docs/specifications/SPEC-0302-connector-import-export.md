# SPEC-0302: Importing, exporting and sharing connector definitions

- **Version:** 1 · **Date:** 2026-11-05
- **Sources:** GEN (sharing configuration as files; reviewing downloaded content before use),
  the project owner 2026-11-05 (sharing through GitHub Issues), ADR-0007, ADR-0008, ADR-0020, ADR-0021 (file
  envelope, `.idcconnector`, `.idcsources`), SPEC-0008, SPEC-0300 (format and validation), SPEC-0301
- **Feature matrix rows:** F-060

## Purpose
A user who added a database system can hand it to others as one file, and anyone can bring such a
file in from disk or a URL after seeing exactly what it contains.

## Behaviour
0. **Exchange format (ADR-0021).** A shared definition is a `.idcconnector` file that starts with
   the envelope `[idecraft]` (`kind = "connector"`, `format = 1`, `id`, `name`, optional `author`,
   `licence`, `homepage`, `min_version`), followed by `[connector]` with the remaining top-level keys
   of SPEC-0300 (`family`, `category`, `note`, `aliases`, `report_url`, …) and the tables
   `[connection]`, `[sql]`, `[introspection]`. The SPEC-0300 layout (top-level `format = 1`) stays
   readable; a file with both layouts, a `kind` other than `connector` or a newer `format` is refused
   with that reason ("written by a newer IDECraft, format 2; this version reads up to 1"). Problem
   paths name the table (`connector.family`, `idecraft.kind`). [ADR-0021]
1. **Import from a file or a URL.** The New Data Source list ends with "Import database system…";
   it asks for a file path (`.idcconnector` or `.toml`) or an `https://` URL (e.g. the raw URL of an
   issue attachment). A URL is fetched only on the user's click, with a
   10-second limit and at most 64 KiB, without cookies or credentials, following at most 3
   redirects, all `https`. [ADR-0007]
2. **Review before saving.** The definition is validated (SPEC-0300 behaviour 4) and shown: its
   text, name, family, category, schemes, port, and every SQL text it contains under a heading
   "Queries this system will run on your connections". Problems are listed with their key paths and
   disable Save. An id that already exists asks "Replace <name>?" for a user or imported file and is
   refused for a built-in. Nothing is stored before Save. [GEN]
3. **Storing.** Save writes the file as received (text unchanged) to `connectors/<id>.idcconnector`
   in the IDECraft settings folder (SPEC-0300 behaviour 2) atomically, removing an older
   `<id>.toml` of the same system, records its origin (file path or URL, and
   the time) in `connectors/imported.json` (machine-written state, ADR-0021), and reloads the catalogue; the system appears with the
   badge `Imported`. Files placed into the folder by hand show as `User`. [SPEC-0300]
4. **Export and share a definition.** Under the chosen system the connection dialog offers
   "Export definition…" (a save dialog proposes `<id>.idcconnector`) and "Share…" for every system,
   built-in too, and "Remove" for user and imported ones. Exports are written in the exchange format
   of behaviour 0; a file already in it is written unchanged; a built-in loses `status` and `tested`,
   so a shared copy never claims Supported. "Share…" copies the definition to the clipboard and
   opens a prefilled GitHub issue from the template `connector-definition.md` (title
   `[connector] <name>`, body with the summary and the definition in a code block, cut like
   SPEC-0301 behaviour 4); nothing is sent by IDECraft (project owner 2026-11-05, ADR-0007).
5. **Export a connection profile.** Tools › Database › "Export Connection…" and Find Action
   (`Database.ExportConnection`) write the connection selected in the Database tool window as a
   `.idcsources` file: the envelope (`kind = "sources"`, `format = 1`) and one `[[connection]]` table
   as in `.idecraft/connections.toml`, without `secret`, without agent permissions and without the
   user name (MCP: `include_user`). If the connection's system is a user or imported one, its
   definition is written next to it as `<id>.idcconnector`. [SPEC-0008, ADR-0021]
6. **Remove.** A user or imported system's context menu offers "Remove"; profiles using it stay
   and show "unknown database system" until it is imported again. [GEN]
7. **MCP.** `list_connectors` (read, SPEC-0300); `validate_connector` (read: `text` or `path` in,
   problems with key paths, the parsed summary and every query out; does not fetch URLs);
   `import_connector` (`text`, `path` or `url`; `dry_run` true by default returns the review of
   behaviour 2; `dry_run: false` stores and needs the `write` right; `replace` for a user or
   imported system; never replaces a built-in); `export_connector` (read: id in, the
   `.idcconnector` text of behaviour 4 and the prefilled share address out — nothing is opened or
   sent); `export_connection` (read: connection id or name, `include_user`; the `.idcsources` text,
   what was left out, and a user or imported system's definition). [ADR-0008]

## Acceptance criteria
- **AK-0** Envelope layout read and written; legacy layout read; wrong kind, newer format and
  mixed layouts refused with key paths (unit tests).
- **AK-1** Import from a file: valid file stored with origin, invalid file refused with problems,
  existing user id replaced only when confirmed, built-in id refused (unit tests with a temporary
  folder).
- **AK-2** URL import: `http://` refused before any network access; `validate_connector` never
  fetches (protocol test); size and redirect limits in `idecraft_http::fetch_small_text`.
- **AK-3** Export: built-in exported without `status` and `tested`; round trip export → import
  gives the same system; profile export has no `secret`, no `ai`, user only when asked; share issue
  URL from the template (unit tests).
- **AK-4** MCP: `validate_connector`, `import_connector` dry run and apply (needs `write`),
  `export_connector`, `export_connection` (protocol test).
- **AK-5** Window self-test steps: "Import database system…" with a file shows the review and its
  queries; Save adds the system with the `Imported` badge and selects it; Remove takes it out again.

## Out of scope
A hosted registry or search of community definitions; signing definitions; automatic updates of
imported definitions.

## Known deviations from the reference
Shared database definitions are readable text with every query shown before import (IMP-0285).
