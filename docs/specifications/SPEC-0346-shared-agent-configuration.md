# SPEC-0346: Shared agent configuration

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (instruction files that coding agents read from a repository: plain Markdown
  with headings, optional front matter for scope; skills as a folder with a `SKILL.md`; lifecycle
  hooks as commands in a JSON settings file); `DOC` (the public documentation of the agents named
  in behaviour 2 about where each looks for its project rules, skills and hooks; facts only, in
  own words); `STD` (the AGENTS.md convention as an open, tool-neutral file name; JSON; UTF-8);
  ADR-0023 rule 7 (untrusted content, no secrets, read-only first); ADR-0031 (project files never
  grant agents anything); ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights, the
  agent log); SPEC-0278 (trust and safe mode); SPEC-0313 behaviour 6 (the secret masker). No
  observation of the reference: the reference has no shared configuration for third-party agents.
- **Feature matrix rows:** none (ROADMAP M19.8, harness phase after 1.0; owner's order of
  2026-11-17).

## Purpose
A project often carries the same rules several times: `AGENTS.md` for one agent, `CLAUDE.md` for
another, a Cursor rule, Copilot instructions. They drift apart, and every new agent needs its own
copy. Ferris IDEA reads all the rule, skill and hook files it knows in a project, shows one merged
view in which every item names the files and lines it came from, and renders that view for any
agent's format, so the rules are maintained once and every hosted agent gets the same ones. An
existing agent's files can be imported into another agent's format. Everything read is data:
Ferris IDEA never follows a rule, never runs a hook and never copies a secret.

This version is the headless core and its MCP tools. Handing the view to agents that Ferris IDEA
starts itself waits for the agent host (ROADMAP M19.5); until then agents read it with
`agent_config`, and `agent_config_export` writes a target agent's file.

## Behaviour

### Discovery
1. Ferris IDEA looks only at a fixed list of paths relative to the workspace root (behaviour 2); it
   never searches the tree, never reads a path a file names, and never reads outside the
   workspace. Folders in the list are read one level deep, their entries sorted by name. [GEN]
2. The known paths, per agent (`agent` names in brackets):
   - shared (`agents_md`): `AGENTS.md`; skills in `.agents/skills/<name>/SKILL.md`.
   - Claude Code (`claude`): `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md`, rules in
     `.claude/rules/*.md`, skills in `.claude/skills/<name>/SKILL.md`, hooks in
     `.claude/settings.json` and `.claude/settings.local.json`.
   - Gemini CLI (`gemini`): `GEMINI.md`.
   - GitHub Copilot (`copilot`): `.github/copilot-instructions.md`,
     `.github/instructions/*.instructions.md`.
   - Cursor (`cursor`): `.cursorrules`, `.cursor/rules/*.mdc`, hooks in `.cursor/hooks.json`.
   - Windsurf (`windsurf`): `.windsurfrules`, `.windsurf/rules/*.md`.
   - Cline (`cline`): `.clinerules` as a file, or `.clinerules/*.md` when it is a folder.
   - Junie (`junie`): `.junie/guidelines.md`.
   The list is part of Ferris IDEA; a project cannot extend it. [DOC]
3. Links: a path or folder entry that is a symbolic link is read only when its target, fully
   resolved, lies inside the resolved workspace root; otherwise it is listed with the problem
   "link leads outside the project" and not opened. A path that resolves to something other than
   a regular file (or, for a folder in the list, a folder) is skipped with a problem. [GEN]
4. Limits: a file larger than 256 KiB is not read (problem with its size); at most 2 MiB are read
   in all and at most 200 files, further files are listed with the problem that the budget is
   used up; at most 500 entries of one folder are looked at. A file that is not valid UTF-8 is not
   read. A file is read with a bound on the bytes taken, so a file that grows while it is read
   cannot pass the limit. [GEN]
5. Every source file is answered with its path, agent, kind (`rule`, `skill`, `hook`), size, a
   digest of its bytes (the first 16 hex digits of SHA-256, its version) and its problem, if any.
   [GEN]
6. A file whose first ten lines hold the marker `ferris-idea:agent-config generated` was written by
   behaviour 15; it is listed as generated and contributes no items, so an export never feeds
   itself. [GEN]

### Parsing
7. Rule files are Markdown. Front matter between a first line `---` and the next `---` is read
   for `description`, `globs`, `applyTo` and `alwaysApply`; `globs` or `applyTo` become the
   item's `applies_to`. The body is split into sections at ATX headings (`#` to `######`)
   outside fenced code blocks; text before the first heading is a section titled with the file's
   name. A section with no text besides its heading gives no item. Each item keeps its title, its
   text and its first and last line in the file. [GEN]
8. A line of a rule file that consists of `@` and a path (an import of another file) is reported
   in the source's `imports` and is never followed; the named file contributes only when it is
   itself a known path. [GEN]
9. A skill is a folder with a `SKILL.md`; its front matter gives `name` and `description` (the
   folder's name when `name` is missing), its body is the item's text. Other files of the folder
   are not read. [DOC]
10. Hooks are read from the JSON files of behaviour 2, only their `hooks` member; nothing else of
    the file is answered. Claude Code's form maps an event to a list of matcher groups, each with
    a list of hooks that have a `command`; Cursor's form maps an event to a list of objects with a
    `command`. Each command is an item with its event and matcher. A file that is not valid JSON or
    whose `hooks` member has another shape is listed with a problem. Hooks are never executed,
    tested or started by this feature, in a trusted project or not; every hook item says
    `executed: false`. [DOC, ADR-0031]

### Secrets
11. Every item's title and text are checked with the secret masker of SPEC-0313 behaviour 6. An
    item with at least one hit is rejected: it is answered with its title, sources and the number
    of hits, without its text, and it never enters an export. Only the rejected section is
    withheld, not the whole file. [ADR-0023 rule 7]

### Merged view
12. The merged view lists the items in the order of behaviour 2 and, within a file, in file
    order. Items of the same kind whose text is the same after trimming every line and dropping
    empty lines are one item with several sources; each source names its path, agent and lines.
    Every item has an `id`: the first 12 hex digits of SHA-256 over its kind and normalised text.
    The view is deterministic: the same files give the same view. [GEN, ADR-0023 rule 4]
13. The view is filtered by `from` (one agent: only its files are read, the import of behaviour
    16) and by `kind`; it is paged with `offset` and `limit` (default 100, at most 500). [GEN]
14. All text in the view is untrusted project data: the answer says so, and Ferris IDEA itself never
    acts on it. The answer carries whether the workspace is trusted (SPEC-0278); in a project that
    is not trusted the view is still answered (reading is allowed in safe mode). [ADR-0023 rule 7]

### Export and import
15. `agent_config_export` renders the merged view (or the files of one agent, `from`) for a
    `target` agent and answers the file it would write and its whole content. Targets and files:
    `agents_md` `AGENTS.md`, `claude` `CLAUDE.md`, `gemini` `GEMINI.md`, `copilot`
    `.github/copilot-instructions.md`, `cursor` `.cursor/rules/ferris-idea-shared.mdc`, `windsurf`
    `.windsurf/rules/ferris-idea-shared.md`, `cline` `.clinerules/ferris-idea-shared.md`, `junie`
    `.junie/guidelines.md`. The content starts with the marker of behaviour 6 in an HTML comment
    (after the front matter for Cursor, which gets `description` and `alwaysApply: true`), then a
    `# Project rules` heading, the sources by path, every rule as a `##` section with its scope
    (`applies_to`) and its sources, and a `## Skills` list with each skill's name, description and
    path. Hooks and rejected items are never rendered; the answer lists them with the reason. The
    rendering is deterministic. [GEN, DOC]
16. Import from an existing agent is the export with `from`: for example `from: claude`,
    `target: agents_md` turns Claude Code's files into an `AGENTS.md`. `from` equal to `target`
    is refused. [GEN]
17. `dry_run` defaults to true and writes nothing. `dry_run: false` needs the `write` right and a
    trusted workspace (the written file steers agents; ADR-0031). The target must stay inside the
    workspace after resolving links of the file and of every folder above it that exists; a target
    that exists and is not a regular file is refused. A target that exists and was not generated
    by behaviour 15 is replaced only with `overwrite: true`; the dry run says `needs_overwrite`. An
    existing target is written through the editor's buffer when it is open (SPEC-0220), and refused
    when it changed since it was read; an unchanged content writes nothing. Every write is logged
    in the agent log by the effects table (SPEC-0053). [ADR-0023 rule 7, ADR-0031]

### MCP
18. `agent_config` answers the view of behaviours 5 to 14: `trusted`, `sources`, `generated`,
    `items` (id, kind, title, text, applies_to, description, event, matcher, sources, rejected),
    `total`, `problems`, the known paths per agent, the limits and `hooks_executed: false`. It needs
    no right. `agent_config_export` answers behaviours 15 to 17: `target`, `file`, `content`,
    `items`, `skills`, `rejected`, `hooks_not_exported`, `exists`, `generated_before`,
    `needs_overwrite`, `changed`, `dry_run`, `written`. [ADR-0008]

## Acceptance criteria
- **AK-1** Given a project with files of several agents, when it is discovered, then every known
  path that exists is a source with agent, kind, size and digest, folders are read one level deep
  in name order, nothing outside the list is read, and a generated file contributes no items
  (behaviours 1, 2, 5, 6).
- **AK-2** Given a symbolic link to a file outside the workspace, a file over 256 KiB, a file that
  is not UTF-8 and more files than the budget, when discovered, then none of them is read and each
  is listed with its problem; a link inside the workspace is read (behaviours 3, 4).
- **AK-3** Given rule files with front matter, headings, fenced code with `#` lines and an `@`
  import, when parsed, then sections, titles, lines and scope follow behaviour 7, the import is
  reported and not followed; skills and the hooks of both JSON forms are items, malformed JSON is a
  problem, and no hook runs (behaviours 7 to 10).
- **AK-4** Given a section with a token or a password assignment, when the view is built, then the
  item is rejected with its hit count and no text, other sections of the file stay, and an export
  leaves it out (behaviour 11).
- **AK-5** Given the same section in `AGENTS.md` and `CLAUDE.md`, when merged, then it is one
  item with both sources and a stable id; filters and paging work; the view is the same on a
  second run (behaviours 12, 13).
- **AK-6** Given a view, when rendered for each target, then the file and content follow
  behaviour 15, Cursor gets its front matter, hooks are not rendered, the output is deterministic
  and contains the marker; `from` equal to `target` is refused (behaviours 15, 16).
- **AK-7** Given an MCP client, when it calls `agent_config`, then it gets the view without a
  right; `agent_config_export` answers a dry run by default, refuses `dry_run: false` without the
  `write` right and in a workspace that is not trusted, refuses to replace a hand-written target
  without `overwrite`, refuses a target behind a link to outside, and with the right writes the
  file, which a second `agent_config` lists as generated (behaviours 14, 17, 18).

## Out of scope
- Handing the view to hosted agents at their start (ROADMAP M19.5); the view and the rendering
  are ready for the host.
- Exporting hooks into another agent's settings: writing a hook makes that agent run it, so it
  needs its own decision with a review step; this version lists hooks only.
- Copying skill folders into another agent's skill folder; the export lists skills with their
  path.
- Following imports, nested rule folders, user-level files outside the project (for example in the
  home folder), commands folders (`.claude/commands`) and MCP server definitions of other agents.
- A view in the window; the MCP tools are the interface of this version.

## Known deviations from the reference
The reference has no shared configuration for third-party agents; nothing is copied. Deviations
from common practice, on purpose:
- **D1** Rule files are data for Ferris IDEA: it never follows them, never executes hooks, and
  never reads a path a rule file names (behaviours 1, 8, 10).
- **D2** A section with a secret-looking value is withheld instead of being copied to other
  agents' files; a false positive costs a reworded line, a true one would spread a credential
  (behaviour 11).
- **D3** Writing another agent's file needs the `write` right, a trusted workspace and, for a
  hand-written file, `overwrite: true` (behaviour 17).
