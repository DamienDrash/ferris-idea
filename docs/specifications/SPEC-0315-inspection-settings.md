# SPEC-0315: Inspection settings and the project's inspection profile

- **Version:** 2 · **Date:** 2026-11-06 (v2: the level "No highlighting, only fix" — defect F032-2)
- **Sources:** OBS-0019 item 7 (the reference has a settings page with every inspection, its on/off
  state and severity, and stores the project's deviations in its project folder), OSS
  (`JetBrains/intellij-community`: the directory-based project format's
  `.idea/inspectionProfiles/Project_Default.xml` — component `InspectionProjectProfileManager`, a
  `profile` with option `myName` and one `inspection_tool` per deviation with the attributes `class`,
  `enabled`, `level`, `enabled_by_default`; levels `ERROR`, `WARNING`, `WEAK WARNING`), SPEC-0098 v3,
  SPEC-0125 v2, SPEC-0129 (settings pages), ADR-0008; v2: OSS (`intellij-community`: the display
  level "do not show" is stored as `level="INFORMATION"`), SPEC-0125 v3
- **Feature matrix rows:** F-032

## Purpose
Turn single inspections off or change how serious they are, per project, in the same file the
reference uses, so a project shared between both IDEs behaves alike.

## Behaviour
1. **Profile file:** the project's profile is `<project>/.idea/inspectionProfiles/Project_Default.xml`.
   IDECraft reads it when the project opens and whenever it changes on disk. Each `inspection_tool`
   whose `class` is the short name of one of IDECraft's inspections (SPEC-0125 v2 behaviour 7) or its
   IDECraft id sets that inspection's on/off state (`enabled`) and severity (`level`: `ERROR` → error,
   `WARNING` → warning, `WEAK WARNING`, `INFO`, `INFORMATION`, `TYPO` and others → weak warning).
   A missing file or a missing entry means the default. A file that cannot be read as XML is ignored
   and the status bar says so once. [OSS]
2. **Writing:** a change writes the same file. Only deviations from the default are written; entries
   whose `class` IDECraft does not know are kept unchanged and in their order; the `myName` option is
   `Project Default`. The `.idea` folder is created when missing. With no deviation left the file
   keeps its unknown entries, or is removed when it has none. Writing is atomic (temporary file and
   rename). [OSS, GEN]
3. **Effect:** the profile applies to the editor's own problems, Inspect Code and the MCP tools
   (SPEC-0098 v3 behaviour 9); a change applies at once to the open editors. [OBS-0019]
4. **Settings page:** Settings › Editor › **Inspections** lists IDECraft's inspections sorted by
   language (Java, Kotlin, JavaScript and TypeScript, Python, Properties, Natural language, General)
   and name, each row with a checkbox, the name, the language and the severity; changed rows are
   drawn in the accent colour. The header names the profile ("Project Default · stored in project").
   The dialog's search text (SPEC-0129) narrows the rows by name, id, short name or language (every
   word must match) and finds the page by an inspection's name as well as by "inspection",
   "profile", "severity" and "suppress". The right part shows the selected inspection's description
   (SPEC-0098 v3 behaviour 6), its severity (Error, Warning, Weak Warning) and a **Reset** link back
   to the default. Space or a click on the checkbox turns it on or off; Enter or a click on the
   severity cycles it; Delete resets it. [OBS-0019]
5. **From the editor:** "Disable inspection" in a finding's submenu (SPEC-0125 v2 behaviour 8) turns it
   off in the profile; "Edit inspection settings" opens this page with the inspection selected.
6. **MCP:** `inspection_profile` lists every inspection with id, short name, name, language, default
   level, current state and level, and description; with `id` and `enabled` and/or `severity`
   (`error`, `warning`, `weak`) it changes the profile and writes the file; `reset=true` with an `id`
   returns it to the default. [ADR-0008]
7. **No highlighting, only fix (v2):** a fourth level besides error, warning and weak warning. The
   profile reads and writes it as `level="INFORMATION"` (replacing the mapping of `INFORMATION` to
   weak warning in behaviour 1; `INFO`, `TYPO` and other levels stay weak warnings). Findings of an
   inspection at this level are not drawn, counted or listed in the editor and the Problems tool
   window's File tab, but their quick fix and submenu stay in ⌥↩ (SPEC-0125 v3); Inspect Code and
   `inspect_code` still list them, with the level `info`. The settings page cycles Error → Warning →
   Weak Warning → No highlighting, only fix; `inspection_profile` takes and returns `info`. [OSS]

## Acceptance criteria
- **AK-1** Reading the reference's file format (known and unknown classes, all levels), writing only
  deviations, keeping unknown entries, removing an empty file, atomic write (unit tests
  `inspection_profile::tests`).
- **AK-2** A disabled inspection is not reported and a raised severity is reported as an error (unit
  test in `inspections::tests`).
- **AK-3** MCP: `inspection_profile` lists, disables, sets a severity and resets
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-4** Window self-test: the settings page lists the Java inspections; turning off
  `java.string-equality` removes the finding from the open editor and writes the profile file.
- **AK-5** Side by side in OBS-0019 (re-check): a profile written by IDECraft is read by the
  reference and the other way round.
- **AK-6 (v2)** `INFORMATION` read as and written for the fix-only level (unit test
  `inspection_profile::tests::reads_the_reference_format`); `inspection_profile` with severity `info`
  (`crates/mcp/tests/it/protocol.rs`: `inspection_profile_tool`).

## Out of scope
Several named profiles and an IDE-level profile, scopes per inspection, the options of single
inspections, "Highlighting in editor" separate from the severity, custom severities, the settings
of the reference's own inspections.

## Known deviations from the reference
The reference offers an IDE-wide profile and named profiles besides the project's; IDECraft has the
project's profile only. Improvement: agents read and change the profile through MCP, and the file is
the reference's, so both IDEs share one profile (IMP-0308).
