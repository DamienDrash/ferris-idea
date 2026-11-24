# SPEC-0303: Ferris IDEA file formats — envelope, exchange files, migrations, import preview

- **Version:** 3 · **Date:** 2026-11-06 (v2: details settled while implementing — `python.toml` body, one save for breakpoint lines and properties, `--print`; v3: a breakpoint's `rules` table, SPEC-0127 v3)
- **Sources:** GEN (versioned file headers, schema validation with key paths, "preview before
  import" in desktop tools, document types of desktop applications), STD (TOML 1.0, JSON Schema
  2020-12, Apple Uniform Type Identifiers and `CFBundleDocumentTypes`, RFC 3986 percent-encoding,
  GitHub issue form query parameters), ADR-0007 (no network use without the user), ADR-0008 (MCP for
  every feature), ADR-0021 (this decision), SPEC-0028/SPEC-0105 (keymaps), SPEC-0047 (run
  configurations), SPEC-0060/SPEC-0131 (themes), SPEC-0008 (connection profiles), SPEC-0091/SPEC-0127
  (saved breakpoints), SPEC-0129 (IDE settings), SPEC-0282 (app bundle), SPEC-0300 (connector files)
- **Feature matrix rows:** F-002, F-077

## Purpose
Every file Ferris IDEA writes for people says what it is and in which format version. Files made to
travel (run configurations, keymaps, themes, data sources without passwords, settings bundles) have
their own `.idc…` extensions: a double-click, `ferris-idea open`, a drop on the window or an agent's
MCP call shows what the file contains and what it would change, and imports only after the user
confirms. Project files under `.ferris-idea/` become uniformly TOML with the same envelope, and older
files are read and migrated without loss.

## Behaviour

### Envelope and reading
1. **Envelope.** Exchange and project files start with the table
   ```toml
   [ferris-idea]
   kind = "run"       # the kind, see behaviour 9
   format = 1         # integer ≥ 1, raised only on an incompatible change
   # optional: id, name, author, licence (SPDX expression), homepage (https://), min_version
   ```
   Other keys in `[ferris-idea]` are problems. Writers put the envelope first. [ADR-0021]
2. **Reading** collects every problem at once; each has a key path (`run.command[1]`,
   `keymap.actions.SaveAll[0]`, `ferris-idea.format`) or `(file)` and a message. In this order:
   - an exchange file over 64 KiB is refused unread ("larger than 64 KiB");
   - TOML syntax errors name line and column;
   - an exchange file without `[ferris-idea]` is "not a Ferris IDEA file";
   - `kind` must be known and the expected one ("is a `keymap` file, not `run`");
   - `format` must be an integer ≥ 1;
   - a newer format is refused with exactly: `written by a newer Ferris IDEA (format N); this version
     reads <kind> files up to format M`;
   - older formats are migrated in memory, one step at a time, through the kind's migrations; the
     file on disk is not changed by reading;
   - the body is checked against the kind's shape: unknown keys, wrong types and missing required
     keys, then the kind's own checks (behaviour 10).
   A file with problems is not imported in part. [ADR-0021, GEN]
3. **Writing.** Writers always write the current format of their kind. A writer refuses to replace a
   file whose envelope names a newer format (same message as behaviour 2), so a downgrade never
   destroys data. [ADR-0021]
4. **Schemas.** Every kind with a body defined here has a JSON Schema (draft 2020-12) at
   `docs/formats/<kind>.schema.json`, generated from the same shape the reader checks; a test fails
   when the published schema differs from the code. `docs/formats/README.md` lists the kinds,
   extensions and how to point a TOML language server at a schema. [STD]
5. **Content rules.** Exchange files are data, never code: importing never runs a command, never
   reads another file and never contacts a host. Path-valued keys (`run.cwd`, `run.report_file`, a
   SQLite `path`) must be relative without `..`. Exports leave out secrets and machine-bound values
   and list them under "left out": stored-password references, agent rights of connections,
   environment variables whose name contains `PASSWORD`, `PASSWD`, `SECRET`, `TOKEN`, `API_KEY`,
   `APIKEY`, `PRIVATE_KEY` or `CREDENTIAL` (case-insensitive), absolute paths, and the `ai`, `mcp`
   and `terminal` settings (an endpoint, a server switch or a shell program are not settings to
   take from someone else). Importing a file that contains these is refused with a problem at the
   key. [ADR-0007, ADR-0021]

### Kinds and extensions
6. **Exchange kinds** (TOML unless noted; import also accepts the same file renamed to
   `<name>.<ext>.toml` or to a plain `.toml` with the envelope):

   | Kind | Extension | Body | Import in this spec |
   |---|---|---|---|
   | `connector` | `.idcconnector` | SPEC-0300 definition | reserved: registered, recognised, import by SPEC-0302 |
   | `sources` | `.idcsources` | `[[connection]]` as in `connections.toml` without secrets | yes |
   | `run` | `.idcrun` | one `[run]` table as a `[[run]]` entry of `run.toml` | yes |
   | `keymap` | `.idckeymap` | `[keymap]` name, parent, `actions` | yes |
   | `theme` | `.idctheme` | `[theme]` name of a bundled theme, optional `compact_mode` | yes |
   | `code-style` | `.idcstyle` | — | reserved |
   | `live-templates` | `.idctemplates` | — | reserved |
   | `settings` | `.idcsettings` | `[settings]` editor, appearance, vcs, code_style; optional `[keymap]` | yes |
   | `plugin` | `.idcplugin` | ZIP with `plugin.toml` and WASM (ADR-0016) | reserved |

   Opening a reserved kind shows the preview with the problem "importing `<kind>` files is not
   available in this version". [ADR-0021]
7. **Project and user kinds** (no own extension):

   | Kind | File | Before this spec |
   |---|---|---|
   | `run-configurations` | `.ferris-idea/run.toml` | same file without envelope |
   | `connections` | `.ferris-idea/connections.toml` | same file without envelope |
   | `sql-files` | `.ferris-idea/sql-files.toml` | same file without envelope |
   | `python` | `.ferris-idea/python.toml` | same file without envelope |
   | `breakpoints` | `.ferris-idea/breakpoints.toml` | `.ferris-idea/breakpoints` (text) and `.ferris-idea/breakpoint-properties.json` |
   | `ide-settings` | `settings.toml` in the settings folder | same file without envelope; the theme was in `theme` |
   | `keymap` | `keymaps/FerrisIDEA.toml` in the settings folder | `keymaps/FerrisIDEA.xml` (reference format) |

   The user keymap and an `.idckeymap` file share kind and body. [ADR-0021]
8. **Format numbers.** Every kind starts at format 1. A project file without envelope counts as
   format 0 of its kind; the migration 0 → 1 adds the envelope and leaves the body as it is, except
   `python.toml`, whose top-level `interpreter` key moves into a `[python]` table (a body must be
   tables so that nothing falls into `[ferris-idea]`). Empty lists are left out when written.

### Bodies
9. **Bodies of the implemented exchange kinds** (unknown keys are problems):
   - `run`: `[run]` with `name` (string, required), `command` (array of strings, required unless
     `url`), `cwd`, `env` (table of strings), `kind` (`run`/`test`), `report`, `report_file`,
     `before` (array of strings), `timeout` (integer seconds, ≥ 1), `url` (`http://` or
     `https://`) — the keys of SPEC-0047.
   - `keymap`: `[keymap]` with `name` (required), `parent` (optional) and `[keymap.actions]`, a
     table from action id to an array of shortcuts. A shortcut is a string: a keyboard shortcut in
     keymap notation (`"control alt S"`, two strokes `"control K, control C"`), a mouse shortcut
     `"mouse: alt button1 doubleClick"` or a gesture `"gesture: dblClick shift SHIFT"`. An empty
     array removes the action's shortcuts.
   - `theme`: `[theme]` with `name` (required, one of the bundled themes of SPEC-0060) and
     `compact_mode` (bool, optional).
   - `sources`: one or more `[[connection]]` tables with the keys of `connections.toml` except
     `secret` and `ai`.
   - `settings`: `[settings]` with any of the tables `editor`, `appearance` (including `theme`),
     `vcs`, `code_style` of SPEC-0129, and optionally a `[keymap]` table as in `keymap`.
10. **Own checks** after the shape: run `name` not empty, `cwd`/`report_file` relative inside,
    `report` one of `libtest`, `tap`, `junit`; keymap shortcut strings parse; theme name is
    bundled; sources pass the profile validation of SPEC-0008 (id pattern, unique ids, dialect,
    port) and use no URL with a password; settings values are clamped on import as when read
    (SPEC-0129) and unknown theme names are problems.

### Import
11. **Preview.** Opening an exchange file shows a preview before anything changes: kind and file
    name, the envelope's name, author, licence and homepage when given, "migrated from format N"
    when it was older, the contents as lines, the changes it would make (each with its target file
    and `add`, `replace` or `unchanged`, plus the old and new value where one is replaced), and the
    problems. With problems the import button is disabled. The window draws the preview as a
    dialog: Enter or "Import" confirms, Escape or "Cancel" closes without change, "Share…" opens
    the issue (behaviour 18). [GEN]
12. **What import changes:**
    - `run` adds the configuration to `.ferris-idea/run.toml` (appended as text, comments kept); when
      the name exists it is imported as `<name> (2)` (`(3)`, … while taken) and the preview says so.
    - `keymap` merges its actions into the user keymap: listed actions get exactly the listed
      shortcuts, others stay. The preview lists every action whose shortcuts change, old → new.
    - `theme` sets `appearance.theme` (and `compact_mode` when given) in the IDE settings; the
      window switches at once.
    - `sources` adds each connection to `.ferris-idea/connections.toml`; an identical one with the
      same id is `unchanged`; a different one with the same id is replaced and keeps the existing
      stored-password reference and agent rights. New connections get read-only agent rights as
      in SPEC-0008.
    - `settings` replaces the given settings tables key by key (the preview lists every key that
      changes) and merges the keymap as `keymap` does.
    Files are written through their writers (behaviour 3, envelope, `.bak` of behaviour 15).
13. **Export** writes the current format and returns the text with a suggested file name
    (`<name>.<ext>`, name made safe for file systems):
    - `run`: one configuration of `run.toml` by name;
    - `keymap`: the user keymap (its changes over the bundled keymap);
    - `theme`: the chosen theme and compact mode;
    - `sources`: all connections or the given ids, without secrets (behaviour 5);
    - `settings`: editor, appearance (with theme), vcs and code_style, plus the user keymap.
    Exports set the envelope's `name`; nothing else of the envelope is filled in automatically.
    An export never overwrites an existing file unless asked to.

### Migrating project files
14. **Renamed files:**
    - Keymap: the user keymap is read from `keymaps/FerrisIDEA.toml`; while it does not exist,
      `keymaps/FerrisIDEA.xml` is read. The next save writes the TOML file.
    - Breakpoints: `.ferris-idea/breakpoints.toml` holds every line breakpoint as a
      `[[breakpoint]]` table with `file` (workspace-relative), `line` (1-based) and, when set,
      `condition`, `hit_condition`, `log_message`, `enabled = false` and (v3) the table `rules`
      of SPEC-0127 v3 (`suspend`, `log_hit`, `log_stack`, `remove_once_hit`, `depends_on` as
      workspace-relative `path:line`, `depends_keep`, `instance_filter`, `class_filter`,
      `caller_filter`). While it does not exist,
      `.ferris-idea/breakpoints` and `.ferris-idea/breakpoint-properties.json` are read. The next save of
      breakpoints or of their properties writes the TOML file with both, in one write, so a moved
      breakpoint keeps its condition; properties of lines without a breakpoint are dropped.
    - Theme: the chosen theme is `appearance.theme` in the IDE settings file. While it is not set
      there, the `theme` file of the settings folder is read. Choosing a theme writes the key.
15. **Backups.** When Ferris IDEA first writes the new form of a renamed file, the old file is renamed
    to `<old name>.bak` (`FerrisIDEA.xml.bak`, `breakpoints.bak`, `breakpoint-properties.json.bak`,
    `theme.bak`). When it first rewrites a file without envelope, it copies it to `<name>.bak`
    first. An existing `.bak` is never replaced: the backup is kept once. Old files that are left
    beside a new file are ignored; the new file wins.
16. **Envelope on save.** `connections.toml`, `sql-files.toml`, `python.toml`, the IDE settings
    and (on import) `run.toml` are written with the envelope; they are read with or without it.
    A file whose envelope names another kind or a newer format is reported as a problem by its
    reader (for `run.toml` in the run configurations' problem list, for `connections.toml` as a
    load error) and is not overwritten.

### Opening, CLI, MCP, sharing
17. **Opening a file.**
    - macOS: the app bundle declares every exchange extension of behaviour 6 as an exported type
      (`UTExportedTypeDeclarations`, identifiers `dev.ferrisidea.<kind>`, conforming to
      `public.utf8-plain-text`, the plugin to `public.zip-archive`) and as a document type
      (`CFBundleDocumentTypes`, role Editor, rank Owner). Double-clicking such a file starts
      Ferris IDEA or uses the running one; the preview opens in the front project window (after the
      window of the start rules of SPEC-0282 when Ferris IDEA was not running).
    - Dropping an exchange file onto the window opens the preview instead of a tab.
    - `ferris-idea open <file>` with an exchange file: when a Ferris IDEA window serves MCP over HTTP,
      it asks that window to show the preview and prints which window; otherwise it starts the
      window as `ferris-idea launch` does and shows the preview there. `ferris-idea open --print <file>`
      prints the preview as text and exits: 0 without problems, 1 with problems.
      `ferris-idea open <directory>` is unchanged.
    [STD, SPEC-0282]
18. **Share.** "Share…" in the preview (and MCP `export_file` with `share: true`) builds a GitHub
    issue URL for the Ferris IDEA repository with the `shared-file.yml` issue form, the title
    `<kind>: <name>`, the kind field and the file text in the content field, percent-encoded. When
    the URL would be longer than 8 000 characters the content field is left empty and the body asks
    to attach the file. Ferris IDEA opens the URL in the default browser and sends nothing itself; the
    user submits the issue in the browser. [ADR-0007, ADR-0021]
19. **MCP** (ADR-0008):
    - `import_file {path, dry_run = true, show = false}` returns the preview of behaviour 11 as
      JSON (`kind`, `format`, `migrated_from`, `envelope`, `contents`, `changes`, `problems`).
      `dry_run: false` applies it (needs the `write` right; refused with problems). `show: true`
      asks the running window to show its preview (over HTTP only; the user confirms there).
    - `export_file {kind, name?, ids?, path?, overwrite = false, share = false}` returns `text`,
      `file_name` and `left_out`; with `path` it writes the file (needs `write`, refuses an
      existing file unless `overwrite`); `share: true` adds the issue URL of behaviour 18.
20. **Window self-test** steps open a theme file's preview, check its lines, cancel with Escape
    without change, import with Enter, and check that a file of a newer format shows its message
    and cannot be imported.

## Acceptance criteria
- **AK-1** Envelope reader: kind mismatch, unknown kind, missing envelope, bad and newer format
  (exact message), 70 KiB file, TOML syntax error with line, several body problems with key paths
  reported at once; an older format is migrated in memory (unit tests `format::tests`).
- **AK-2** Every kind with a body has `docs/formats/<kind>.schema.json` equal to the schema
  generated from its shape; the shapes accept every value the writers produce (unit tests).
- **AK-3** Round trips for `run`, `keymap`, `theme`, `sources`, `settings`: export → read → same
  data; exports leave out secrets and list them; importing a file with a secret, an absolute path
  or an `ai` table is refused with the key path (unit tests in `crates/formats`).
- **AK-4** Import changes: run name clash → `<name> (2)` with comments kept; keymap merge with
  old → new; sources replace keeps the stored-password reference; settings list changed keys; a
  preview writes nothing (unit tests).
- **AK-5** Migrations old → new with `.bak` kept once: `keymaps/FerrisIDEA.xml` → `.toml`;
  `breakpoints` + `breakpoint-properties.json` → `breakpoints.toml`; `theme` → `appearance.theme`;
  envelope added to `connections.toml`, `sql-files.toml`, `python.toml`, IDE settings; old files
  still read; newer-format files are not overwritten (unit tests in their crates).
- **AK-6** MCP `import_file` (preview, apply with `write`, problems) and `export_file` (text,
  file, `left_out`, overwrite refusal, share URL) (protocol tests).
- **AK-7** `ferris-idea open --print` prints the preview and exits 0/1 (integration test of the
  binary); `open <directory>` unchanged.
- **AK-8** The app bundle's `Info.plist` declares every exchange extension as document and
  exported type (`plutil -lint`, test that the script lists every extension of the registry).
- **AK-9** The Share URL: template, title, fields, encoding, 8 000-character limit (unit test).
- **AK-10** Window self-test steps of behaviour 20 (written; run with the window self-test).

## Out of scope
Import from a URL (ADR-0021 section 6) — it needs a network call and gets its own spec; the bodies
of `connector` (SPEC-0300/SPEC-0302), `code-style`, `live-templates` and `plugin`; colour themes
of one's own (a theme file names a bundled theme; format 2 can add colours); state files
(ADR-0021 section 5); reading new-format files with an older Ferris IDEA.

## Known deviations from the reference
Shared settings are typed, versioned TOML files with their kind in the file and a JSON Schema,
validated with every problem and its key path, instead of untyped XML in archives that an import
dialog applies without saying what changes (IMP-0294). Every import shows its changes, old → new,
and needs a confirmation; files from a newer version are refused with the version instead of
being half read (IMP-0295). Exports never carry passwords, tokens, agent rights, programs to run
or AI endpoints (IMP-0296). Sharing is a prefilled issue opened in the browser, never an upload
by the IDE (IMP-0297).
