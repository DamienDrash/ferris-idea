# ADR-0021: Ferris IDEA file formats — one envelope, own extensions for files that travel

- **Status:** accepted · 2026-11-05 · decided on the project owner's request of 2026-11-05 ("a standard file
  format of our own for database definitions; check whether project files and other file types that
  belong to Ferris IDEA should have their own formats"). Extension names may still change before the
  first public release (GO-4).
- **Context:** Ferris IDEA writes about 25 kinds of files of its own. They grew one spec at a time. The
  result is uneven:
  - **Syntax:** most are TOML (`run.toml`, `connections.toml`, `sql-files.toml`,
    `ide-settings.toml`, `trust.toml`, `agent-permissions.toml`). Others are JSON
    (`breakpoint-properties.json`, `permissions.json`, `state.json`) or JSON Lines
    (`revisions.jsonl`). One is XML (`keymap.xml`). Several are plain text without an extension:
    `breakpoints`, `profiles/`, `theme`, `recent-projects`, `recent-files`.
  - **Versioning:** no file names its kind or format version. A file copied to another machine or
    a newer Ferris IDEA cannot say what it is, and a reader can only guess.
  - **Sharing:** files meant to be shared have no way to be recognised and opened by double-click
    or drag and drop, from mail or from a GitHub issue. This applies to connector definitions
    (SPEC-0300), run configurations, keymaps, themes, code styles and settings exports.

  The reference uses XML everywhere, with names that do not say the kind. That makes merge
  conflicts hard to read, and shared settings must be imported through a dialog.

## Decision

**1. Three classes of files.**

| Class | Where | Syntax | Own extension |
|---|---|---|---|
| **Exchange** — made to be shared, imported, attached to issues | anywhere | TOML | yes, see 3 |
| **Project** — under version control, read by people and tools | `.ferris-idea/` in the project | TOML (JSON only where a foreign tool defines the format) | no, `*.toml` |
| **State** — machine-written, per user, not shared | user config and cache folders | JSON or JSON Lines | no |

TOML for everything a person reads, writes or reviews in a diff. JSON and JSON Lines only for
machine state, appended logs, and formats owned by other tools (`http-client.env.json`, test
reports, `package.json`).

**2. One envelope.** Every exchange and project file starts with the same table:

```toml
[ferris-idea]
kind = "connector"        # connector, run, keymap, theme, code-style, settings, live-template, …
format = 1                # integer, raised only on incompatible change
# optional: id, name, author, licence (SPDX), homepage, min_version
```

Rules:
- **Reading:** a reader refuses a file whose `kind` is wrong. It reads every `format` up to its own
  and migrates older ones in memory. For a newer `format` it gives a clear message ("written by a
  newer Ferris IDEA, format 2; this version reads up to 1") instead of guessing.
- **Writing:** writers always write the current format.
- **Validation:** every kind has a JSON Schema published under `docs/formats/`. Editors with a
  TOML language server validate and complete these files. Ferris IDEA validates on import and reports
  all problems at once with key paths (as SPEC-0300 AK-2).
- **Content:** exchange files are data, never code. No paths outside the file, no commands that run
  on import, and no secrets: passwords stay in the keychain, and an export leaves them out.

**3. Own extensions for exchange files.** These extensions are registered with macOS as document
types of the app:

| Kind | Extension | Replaces / origin |
|---|---|---|
| Database connector | `.idcconnector` | SPEC-0300 `connectors/*.toml` |
| Data source export (without passwords) | `.idcsources` | export of `connections.toml` |
| Run configuration | `.idcrun` | single entry of `run.toml` |
| Keymap | `.idckeymap` | `keymap.xml` |
| Theme | `.idctheme` | `theme` |
| Code style | `.idcstyle` | — |
| Live templates | `.idctemplates` | — |
| Settings bundle | `.idcsettings` | export of `ide-settings.toml` and the above |
| Plugin | `.idcplugin` | ZIP with `plugin.toml` manifest and WASM (ADR-0016) |

The extensions are:
- **Unambiguous:** all start with `idc`, so they do not clash with other software.
- **Plain TOML:** any editor opens them. GitHub shows them as text in issues when renamed to
  `.toml`, and import accepts both names.

Opening one in Ferris IDEA (double-click, drag and drop, `ferris-idea open`, the MCP tool `import_file`)
validates it, shows what it contains and what it would change, and imports only after confirmation.

**4. Project files keep their names**, gain the envelope, and become uniformly TOML:
- `keymap.xml` becomes `keymap.toml`.
- `breakpoints` becomes `breakpoints.toml`. It absorbs `breakpoint-properties.json` (breakpoints
  are reviewed in diffs).
- `theme` becomes a key in `ide-settings.toml`.

Old files are read and rewritten in the new form the first time Ferris IDEA saves. The old file is kept
once as `*.bak`.

**5. State files stay JSON.** These are `recent-projects`, `recent-files`, `revisions.jsonl`,
`state.json` and the profiler output. Each gets the extension that says its syntax, and a
`"format": n` field where it is a single document. They never travel and need no own extension.

**6. Sharing goes through GitHub Issues** (project owner, 2026-11-05):
- "Share" on an exchange file opens a prefilled issue from the matching issue template, with the
  file attached.
- "Import from URL" accepts the raw URL of such an attachment.
- Nothing is sent without the user pressing the button (ADR-0007).

## Consequences
- One shared crate module (`ferris_idea_core::format`) reads the envelope, checks kind and version,
  runs migrations and collects problems. Every kind plugs in a schema and its migrations.
- A follow-up spec (SPEC-0303, file formats) defines each kind's keys, the migrations of
  section 4, the macOS document types and the import preview. SPEC-0300 switches its connector files
  to `.idcconnector` with the envelope. `connectors/*.toml` stays readable.
- Diffs of project settings become readable. Files carry their kind and version, so shared files
  can be checked before anything changes.
- Cost: one migration per renamed project file, and a schema per kind kept in step with the code.

## Alternatives considered
- **JSON everywhere:** has no comments and is noisy in diffs for hand-edited settings. Rejected for
  project and exchange files.
- **XML as in the reference:** verbose, hard to merge, and no gain. Rejected.
- **A binary or zipped format for everything:** cannot be reviewed or diffed. Only the plugin uses
  ZIP, because it carries a WASM module.
- **Plain `.toml` without own extensions:** the OS cannot open such files in Ferris IDEA, and an issue
  attachment does not say what it is. Rejected for exchange files, kept for project files.
