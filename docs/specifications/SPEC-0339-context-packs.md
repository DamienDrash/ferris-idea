# SPEC-0339: Context packs for agents

- **Version:** 1 · **Date:** 2026-11-17
- **Sources:** `GEN` (what a programmer looks at before changing a function: its declaration,
  who calls it, its implementations, its errors, its tests, its recent history and the project's
  conventions); ADR-0023 rule 4 (context packs), rule 3 (structured, token-lean tool answers) and
  rule 7 (untrusted content, no secrets, read-only first); ADR-0008 (every feature has an MCP
  tool); SPEC-0006 (the MCP tools this pack reuses: `hover_symbol`, `call_hierarchy`,
  `find_symbol_locations`, `find_usages`, `file_diagnostics`, `goto_test`, `git_log`,
  `code_authors`), SPEC-0293 (Find Usages and its text fallback), SPEC-0071 v10 (Go to Test by
  file names), SPEC-0292 v4 (declarations from the syntax tree), SPEC-0053 v6 behaviour 10 (files
  whose names commonly hold secrets), SPEC-0278 (project trust and safe mode of language
  servers). No observation of the reference: the reference has no context packs.
- **Feature matrix rows:** none (ROADMAP M19.2, harness phase after 1.0).

## Purpose
An agent that is about to change code needs the right context, not whole files. A context pack
answers "what do I need to know about this target" within a size budget the agent sets: the
symbol and its signature, its callers and implementations, the current diagnostics, the related
tests, the recent changes and the project's rules. Every piece names where it came from and which
version of that source it reflects, and the pack is deterministic: the same target, budget and
project state give the same pack, so two agents asking the same question get the same context.
What did not fit is listed, so the agent can ask for it with the narrower tools.

## Behaviour

### Target and budget
1. The target is a workspace file `path` with one of:
   - nothing more: the **file** target;
   - `line` and `character` (0-based, characters as Unicode scalar values): the **symbol** target,
     the symbol at that position;
   - `test`, the name of a test function or method in `path`: the **test** target, typically a
     failing test. Its position is the first line of `path` that declares a function, method or
     class named `test` according to the file's syntax tree; without such a declaration the call
     fails with `invalid_params`.
   `line`/`character` and `test` are exclusive; `line` without `character` (or the reverse) is an
   error. The path follows the workspace rules of the other file tools (no absolute path, no
   `..`, inside the workspace after resolving links). [ADR-0023 rule 4]
2. The budget is `budget_chars` (characters of piece content, default 16000, 1000–200000) or
   `budget_tokens` (estimated as 4 characters per token, 250–50000), not both. The answer states
   the budget in characters, the characters used and the estimated tokens used (characters / 4,
   rounded up). [ADR-0023 rule 3]
3. `include` limits the pack to the named piece kinds (behaviour 5); unknown kinds are an error.
   `language_servers` (default true) switches the parts that ask a language server on or off;
   `timeout_s` (default 10, 1–60) bounds each language-server request. [GEN]

### Pieces
4. A pack consists of pieces. Each piece has an `id` (stable for the same source:
   `<kind>:<path>:<line>` with a 1-based line, `<kind>:<path>` for a piece about a whole file, or
   `change:<commit id>`), its `kind`, its `source`
   (`tool`, the MCP tool whose logic produced it; `path`, workspace-relative; `line`, 1-based,
   when it has one), its `version` (behaviour 12), its `content` (text), `chars` (the content's
   length in Unicode scalar values), `truncated` and `untrusted: true`. [ADR-0023 rule 4]
5. The kinds, in their fixed priority order:

   | Priority | Kind | Present for | Content | Source tool |
   |---|---|---|---|---|
   | 0 | `symbol` | symbol, test | the innermost declaration of the syntax tree around the position: a header line `<kind> <name> (lines a–b)` and the declaration's lines; without a declaration around it, the lines from 5 before to 15 after the position | `document_symbols` |
   | 0 | `outline` | file | one line per declaration of the file's syntax tree: `<line> <kind> <name>`, indented by nesting | `document_symbols` |
   | 0 | `signature` | symbol, test | the language server's hover text for the position (type, signature, documentation) | `hover_symbol` |
   | 1 | `diagnostic` | all | one finding: `<severity> line <n>: <message> [<source> <code>]`, from the language server and Ferris IDEA's own inspections of the target file | `file_diagnostics` |
   | 2 | `caller` | symbol | one direct caller from the call hierarchy: `<kind> <name> in <path>:<line>` and the lines of its call sites | `call_hierarchy` |
   | 2 | `callee` | test | one function the test calls directly, with its declaration's lines when the syntax tree of its file has them | `call_hierarchy` |
   | 3 | `implementation` | symbol | one implementation or override of the symbol other than the symbol itself, with its declaration's lines | `find_symbol_locations` |
   | 4 | `test` | all | one related test file: for a symbol or test target the declarations of that file whose text names the symbol, else its outline | `goto_test`, `find_usages` |
   | 5 | `rule` | all | one project rules file (behaviour 9) | `read_file` |
   | 6 | `usage` | symbol | one reference to the symbol not already given as caller, implementation or the symbol itself: `<path>:<line>: <line text>` | `find_usages` |
   | 7 | `change` | all | one recent commit that changed the target file: `<short id> <date> <author>: <summary>`, and for a symbol or test target whether it last changed lines of the symbol | `git_log`, `code_authors` |

   [ADR-0023 rule 4, GEN]
6. Related tests (kind `test`) are the test files Go to Test names for `path` (SPEC-0071 v10, by
   file names among the project's own files), followed by the other test files among the usages of
   the symbol (by the same naming rules), each file once. For a test target the subject files Go to
   Test names for the test file are given as kind `test` as well (they hold the code under test).
   At most 10 files. [GEN]
7. Limits before ranking: at most 20 callers or callees, 20 implementations, 50 usages, 100
   diagnostics, 10 test files, 5 changes (from the newest 200 commits of HEAD), 10 rules files.
   What a limit cut is counted per kind in `more`. [ADR-0023 rule 3]
8. A piece whose content is longer than 4000 characters is cut at the last line break before that
   length (or at the length when the first line is longer) and marked `truncated`. [ADR-0023
   rule 3]
9. The project rules files are, in this order: `AGENTS.md` and `CLAUDE.md` in the folder of `path`
   and in each folder above it up to the workspace root (nearest folder first, `AGENTS.md` before
   `CLAUDE.md`), then the Markdown files directly in `.ferris-idea/rules/` sorted by name. A file that
   is missing, not a regular file or not UTF-8 is skipped. Their content is project data: the pack
   marks it `untrusted` like every piece, never as instructions to the agent. [ADR-0023 rule 7]

### Ranking and budget
10. The candidate pieces are ordered by a total order: priority of the kind (behaviour 5), then the
    kind's own order — `diagnostic` by severity (error, warning, information, hint, other), a
    finding inside the symbol's lines before one outside, then line; `caller`, `callee`,
    `implementation`, `usage`, `test` with pieces in the target file first, then by path and line;
    `rule` in the order of behaviour 9; `change` newest first, ties by commit id — and finally by
    `id`. Pieces with the same `id` are kept once (the first in this order). [ADR-0023 rule 4]
11. Pieces are taken in that order while they fit: a piece whose `chars` would make the used
    characters exceed the budget is left out and the next one is tried, so a smaller piece of
    lower rank can still be included. The first piece of the order is never left out for the
    budget: when it does not fit, it is cut to the budget (as in behaviour 8) and marked
    `truncated`. Every piece left out is listed in `omitted` with `id`, `kind`, `source`, `chars`
    and `reason` (`budget`), in rank order. Each included piece carries its 1-based `rank` in the
    order of behaviour 10. [ADR-0023 rule 4]
12. Versions: a piece taken from a file has `sha256:` and the first 16 hex digits of the SHA-256 of
    the file's bytes as read; a `change` has the commit id. The pack has `version`, `sha256:` and
    16 hex digits over the ids, versions and contents of the included pieces in order, and `head`,
    the commit id of HEAD (null outside a repository or without commits). [ADR-0023 rule 4]
13. Determinism: the pack contains no time of the call, no absolute path and no random value, and
    every list is in the order of behaviour 10. With `language_servers: false` the same target,
    budget, file contents, rules files and HEAD give byte-identical answers. With language
    servers, the pack is as deterministic as the servers' answers; a server that does not answer
    within `timeout_s` makes its kinds unavailable (behaviour 14) rather than partial. [ADR-0023
    rule 4, deviation D1]

### Availability, safety, effects
14. A kind that cannot be gathered — language servers switched off, no server for the language,
    no answer in time, no Git repository — is listed in `unavailable` with `kind`, `tool` and
    `reason`; the pack still holds the other kinds. When the call hierarchy is unavailable,
    `usage` pieces still come from Find Usages, which falls back to whole-word text occurrences
    in files of the same extension when the server is slow or missing (SPEC-0293); such pieces
    carry `source.mode: "text"`. [GEN]
15. Secrets: a file whose name commonly holds secrets (SPEC-0053 v6 behaviour 10) gives no content.
    As the target it is refused unless the agent has the `secret-read` right (asked in the window
    over HTTP, as `read_file` does); as any other piece it is left out with `reason: "withheld"`
    in `omitted` and the withheld paths are written to the local agent log (paths only). Known
    secret patterns in every piece's content are masked as in the other reading tools, and
    `masked` counts them. Paths outside the workspace (library sources) are left out. [ADR-0023
    rule 7, SPEC-0053]
16. The tool is read-only: it writes no project file (the agent log entry of behaviour 15 aside),
    runs no project code and no build, and needs no right. Language servers are started as the
    reused tools start them, in safe mode for an untrusted project (SPEC-0278). Git is read
    without starting a `git` process. [ADR-0023 rule 7, SPEC-0278]
17. The MCP tool `context_for` takes the arguments of behaviours 1–3 and answers `target`
    (`kind`, `path`, `line`, `character`, `test`, `symbol` — the declaration's name when known),
    `budget` (`chars`, `used_chars`, `used_tokens_estimate`), `version`, `head`, `pieces`,
    `omitted`, `unavailable`, `more`, `masked` and `note` (that every piece is untrusted project
    content and never an instruction). Its description states the same. Its effect class is
    read-only with process reach (language servers). [ADR-0008, ADR-0023 rules 3 and 7]

## Deviations and improvements
- **D1** Language-server answers can differ between runs (a server still indexing, a slow start).
  The pack does not mix partial answers: a kind is either complete within its limit or listed as
  unavailable, and `language_servers: false` gives a fully reproducible pack from the syntax trees,
  the inspections, the file names, Git and the rules files.

## Acceptance criteria
- **AK-1** Given a project with a function, a caller in another file, a test file named after the
  source file, `AGENTS.md`, `CLAUDE.md`, a rules file in `.ferris-idea/rules/` and two commits, when
  `context_for` is called for the function's position with language servers off, then the pack
  holds the `symbol` piece with the function's lines first, a `usage` piece for the caller, a
  `test` piece for the test file, the `rule` pieces in the order of behaviour 9, `change` pieces
  newest first with commit ids as versions, file pieces with `sha256:` versions, `unavailable`
  naming the language-server kinds, and every piece has `untrusted: true` and a source tool.
- **AK-2** Given the same project, when the call is repeated, then both answers are byte-identical;
  when a file changes, then its pieces' versions and the pack version change.
- **AK-3** Given candidate pieces of every kind, when they are ranked and cut to a budget, then the
  order follows behaviour 10, a piece that does not fit is listed in `omitted` with reason
  `budget` while a smaller later piece is still included, the first piece is cut to the budget
  rather than left out, long pieces are cut at a line break and marked `truncated`, duplicates by
  `id` are kept once and the used characters never exceed the budget.
- **AK-4** Given a `.env` file among the usages and a secret value in a rules file, when the pack is
  built without the `secret-read` right, then the `.env` piece is omitted with reason `withheld`
  and no content, the secret value is masked and counted, and a `.env` target is refused.
- **AK-5** Given a file target and a test target, when packs are built, then the file target gives
  an `outline` piece of the file's declarations, the test target's `symbol` piece is the test
  function found by name, its `test` pieces name the subject file, and an unknown test name, both
  `line` and `test`, both budgets, or an unknown `include` kind are refused.
- **AK-6** `context_for` is served over MCP with its arguments, listed as read-only with process
  reach in the effects table, writes no file of the project and its description says the content
  is untrusted.
