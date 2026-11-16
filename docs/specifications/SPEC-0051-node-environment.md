# SPEC-0051: Node.js runtimes, package manager and packages

- **Version:** 2 · **Date:** 2026-11-01 (v2: Vitest and Jest with JSON reports, SPEC-0045 v3)
- **Sources:** GEN (IDEs pick a Node.js interpreter and package manager per project, warn about missing or
  outdated dependencies and run tests; F-036), STD (`package.json` fields `engines`, `packageManager`
  (Corepack), the four dependency maps; npm's semantic-version range grammar (node-semver): caret, tilde,
  x-ranges, hyphen ranges, comparator sets, `||`, pre-release rules; lockfile names of npm, Yarn, pnpm and
  Bun; nvm's `versions/node/vX.Y.Z` and fnm's `node-versions/vX.Y.Z/installation`; `.nvmrc` and
  `.node-version`; `include/node/node_version.h` of a Node.js installation; the `node:test` runner with
  its TAP reporter, Vitest's JUnit reporter, Mocha's TAP reporter), SPEC-0047, SPEC-0045
- **Feature matrix rows:** F-036

## Purpose
Know which Node.js and package manager a project uses, whether the installed packages match
`package.json`, and how to run its tests — without starting anything to find out.

## Behaviour
1. Runtimes are found from a configured runtime (executable or installation folder), nvm and fnm versions
   (newest first) and `node` on `PATH`; duplicates (same resolved file) are dropped. [STD]
2. The version is read without starting node: from the version manager's folder name, else from
   `include/node/node_version.h` of the installation the executable resolves to, else from a `vX.Y.Z`
   folder in its path. Unknown versions are reported as such. [STD; same principle as IMP-0025]
3. Selection: configured; the newest managed version satisfying `.nvmrc`/`.node-version` (read as an npm
   range, a leading `v` ignored; aliases such as `lts/*` are not resolved); first on `PATH`; newest
   managed. A version file no installed runtime satisfies is reported. The selected version is checked
   against `engines.node`. [GEN, STD]
4. Package manager: `packageManager` (name and version, hash dropped), else the lockfile's manager, else
   npm. Lockfiles of more than one manager, and a `packageManager` that differs from the lockfile, are
   problems. [STD, improvement IMP-0026]
5. Installed packages are read from `node_modules/<name>/package.json` (scoped names included). Whether
   `node_modules` exists is reported. [STD]
6. Every declared dependency whose value is a version range is checked against the installed version
   (npm range grammar incl. pre-release rule: a pre-release only matches a comparator set naming a
   pre-release of the same version). Missing or out-of-range dependencies are listed per kind, sorted by
   name; optional and peer dependencies may be absent. Values that are not ranges (tags, `workspace:`,
   `file:`, git, URLs, `npm:` aliases) are listed as not checked. [STD]
7. Test framework from the declared dependencies: Vitest (test configuration with JSON report file
   `.idecraft/vitest.json`), Jest (test configuration `jest --ci --json`, report `.idecraft/jest.json`), Mocha (TAP report), otherwise the built-in
   `node:test` runner of the selected runtime with its TAP reporter. `npx` next to the selected node is used
   when present. Nothing is written automatically. [GEN]
8. MCP tool `node_environment` (optional `runtime`). [ADR-0008]

## Acceptance criteria
- **AK-1** Runtimes from nvm, fnm and PATH (version from folder names and from the header file); selection
  by `.nvmrc`, `.node-version` without a match (reported), configured installation folder.
- **AK-2** npm ranges: caret incl. `0.x` forms, tilde, x-ranges, partial comparators (`>1.2`, `<=1.2`),
  hyphen ranges, `||`, spaced operators, pre-releases; non-range values recognised.
- **AK-3** Project: `engines.node` mismatch, `packageManager` against lockfile, lockfiles of two managers,
  missing and out-of-range packages incl. scoped, optional/peer allowed absent, not-checked values, missing
  `node_modules`, Vitest and Jest configurations, malformed `package.json`.
- **AK-4** The installed node: the statically read version equals `process.versions.node`; the suggested
  `node --test` configuration runs real tests and its TAP report is read (1 passed, 1 skipped).
- **AK-5** The MCP tool reports package manager, problems, unsatisfied packages and test configuration.

## Out of scope
Installing packages, resolving lockfile contents, workspaces' nested `node_modules`, Volta and asdf,
resolving `lts/*` aliases, framework-specific support (React, Angular, Vue), the TypeScript language server
(M4.4), debugging Node.

## Known deviations from the reference
Nothing is started for discovery; conflicting lockfiles are problems (IMP-0026); not compared side by side.
