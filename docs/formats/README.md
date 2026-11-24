# Ferris IDEA file formats

Ferris IDEA writes TOML files that say what they are. Each starts with an `[ferris-idea]` table naming
its `kind` and `format` version ([ADR-0021](../adr/0021-ferris-idea-file-formats.md),
[SPEC-0303](../specifications/SPEC-0303-ferris-idea-file-formats.md)):

```toml
[ferris-idea]
kind = "theme"
format = 1
name = "Light"          # optional: id, name, author, licence, homepage, min_version

[theme]
name = "Ferris IDEA Light"
```

Ferris IDEA reads every format up to its own and migrates older ones when it reads them. It refuses a
file from a newer version and says which format it found. Import reports every problem at once,
each with its key path.

## Exchange files

Exchange files are made to be shared. Opening one shows what it contains and what it would change.
Nothing changes until you confirm. Exports leave out passwords, stored-password references, agent
rights, environment variables named like secrets, paths outside the project, and the AI, MCP and
terminal settings.

| Kind | Extension | Schema | Import |
|---|---|---|---|
| `run` | `.idcrun` | [run.schema.json](run.schema.json) | Works |
| `keymap` | `.idckeymap` | [keymap.schema.json](keymap.schema.json) | Works |
| `theme` | `.idctheme` | [theme.schema.json](theme.schema.json) | Works |
| `sources` | `.idcsources` | [sources.schema.json](sources.schema.json) | Works |
| `settings` | `.idcsettings` | [settings.schema.json](settings.schema.json) | Works |
| `connector` | `.idcconnector` | — | Planned (SPEC-0302) |
| `code-style` | `.idcstyle` | — | Planned |
| `live-templates` | `.idctemplates` | — | Planned |
| `plugin` | `.idcplugin` | — | Planned |

Import also accepts these files renamed to `.toml`, for example `my.idckeymap.toml`.

## Project files

| Kind | File | Schema |
|---|---|---|
| `breakpoints` | `.ferris-idea/breakpoints.toml` | [breakpoints.schema.json](breakpoints.schema.json) |
| `bookmarks` | `.ferris-idea/bookmarks.toml` | [bookmarks.schema.json](bookmarks.schema.json) |
| `run-configurations` | `.ferris-idea/run.toml` | — |
| `connections` | `.ferris-idea/connections.toml` | — |
| `sql-files` | `.ferris-idea/sql-files.toml` | — |
| `python` | `.ferris-idea/python.toml` | — |
| `ide-settings` | `settings.toml` in the settings folder | — |
| `keymap` | `keymaps/FerrisIDEA.toml` in the settings folder | [keymap.schema.json](keymap.schema.json) |

Files from older versions are still read. The first time Ferris IDEA writes the new form, it keeps the
old file once as `<name>.bak`.

## State files

State files hold what Ferris IDEA remembers for one user. They are not made to be shared and are
JSON with a `version` ([ADR-0021](../adr/0021-ferris-idea-file-formats.md)).

| File | Schema |
|---|---|
| `window-layouts.json` in the settings folder: the saved tool window layouts ([SPEC-0333](../specifications/SPEC-0333-named-tool-window-layouts.md)) | [window-layouts.schema.json](window-layouts.schema.json) |

Ferris IDEA does not read a state file from a newer version and does not overwrite it.

## Using a schema in your editor

The schemas are JSON Schema 2020-12. A TOML language server such as Taplo can check a file
against one. Put a `#:schema` comment on the first line of the file, for example:

```toml
#:schema ./docs/formats/run.schema.json
```

The schemas are generated from the code. A test fails when they differ. To regenerate them, run
`FERRIS_IDEA_WRITE_SCHEMAS=1 cargo test -p ferris-idea-core published_schemas`, and for
`window-layouts.schema.json` `FERRIS_IDEA_WRITE_SCHEMAS=1 cargo test -p ferris-idea-ui published_schema`.
