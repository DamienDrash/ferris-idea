# SPEC-0163: Project dictionary and resource bundles

- **Version:** 3 · **Date:** 2026-11-23 (v3: project words in any case and in `check_grammar`; a
  missing bundle key is an error in Inspect Code and over MCP too, never naming the base file with
  an empty name, with a fix that adds the key to the other files or says why it cannot —
  behaviours 2–4, AK-7 to AK-9; OBS-0117 G3, G4, IMP-0534, IMP-0536); 2026-11-15 (v2: the hand
  test's steps in a window test, AK-6; v1: 2026-11-03)
- **Sources:** OSS (intellij-community spell checker: project dictionary in
  `.idea/dictionaries/<name>.xml`, component `ProjectDictionaryState`, `<w>` elements; Properties
  plugin: resource bundles named `base_language_COUNTRY_variant.properties`), STD (file naming of
  `java.util.ResourceBundle`), SPEC-0155, SPEC-0159, ADR-0008; v3: OBS-0117 (the reference marks a
  missing key at the key in the file that has it with the locales that lack it, but names the base
  file with an empty name, IMP-0534; its "add missing translation" changed no file and said
  nothing, IMP-0536), SPEC-0193 (one undo group across files), SPEC-0220 v5 (MCP writes through
  open editors)
- **Feature matrix rows:** F-081

## Purpose
Project words such as product names stop being reported as typos, shared with the reference; and
translations that lack a key are found before a user sees the key instead of a text.

## Behaviour
1. **Save to dictionary:** on a typo, ⌥↩ offers "Save '<word>' to project dictionary" besides the
   suggestion. It adds the word, lower-cased, to `.idea/dictionaries/project.xml` of the project
   (the nearest folder above the file with `.idea`, else `.git`, else the window's folder), creating
   it in the reference's format, and checks the open files again. [OSS]
2. **Known words:** words of that file are no typos in the editor, Inspect Code and MCP; the file is
   read again when it changes. [OSS]
   **v3:** a word of the file counts in any case: `quixlet` in the file also covers `Quixlet` at
   the start of a sentence (SPEC-0155 v8 behaviour 7). "MCP" includes `check_grammar`, which reads
   the dictionary of the workspace's folder (`.idea/dictionaries/project.xml`) on every call;
   before v3 it ignored it (OBS-0117 G4). [OBS]
3. **Resource bundles:** `.properties` files of one folder with the same base name form a bundle;
   the locale is read from the end of the name (language, optional country, optional variant). A
   key of one file that another file of the bundle lacks is reported at the key, naming the locales
   ("default" for the base file). [OSS, STD]
   **v3 — an error everywhere, with a fix (OBS-0117 G3, IMP-0534, IMP-0536):**
   - The finding (`properties.missing-in-bundle`, "Property missing in other files of its
     resource bundle") is an error (SPEC-0155 v5 behaviour 5) on the key, in the file that has
     it: message `` `extra.only.fr` is missing in the bundle's default, de ``. The locales are
     those of the other files that lack the key in their order, the base file always named
     `default`, never with an empty name, the same names `resource_bundle` uses. The other files
     are read from disk.
   - It shows in the editor, in the Problems tool window, in Code › Inspect Code and in MCP
     `inspect_code` (which before v3 missed it, because a project run read the bundle's folder
     relative to the program's working folder) and `properties_file` (which before v3 listed only
     duplicate keys and trailing spaces; each such problem also carries `key` and `missing`, the
     locales).
   - ⌥↩ on it offers "Add missing translation to <locales>" (`Add missing translation to default, de`). It adds
     the line `<key>=` — an empty value — at the end of each file of the bundle that lacks the
     key, after a line break if the file did not end with one; the file that has the key is not
     touched. An empty value rather than a copy of the text or a `TODO` marker, so no placeholder
     text ever reaches a user of the program, the bundle editor (SPEC-0212) shows the gap as an
     empty cell, and nothing has to be removed again. Open files change in their editors (they
     stay unsaved, as with every change across files), the others on disk. The whole change is
     one undo group named "Add Missing Key" (SPEC-0193): ⌘Z in an editor it changed takes it
     back in every file; files written on disk also get local history revisions labelled so.
     The status bar names the files (`Added 'extra.only.fr' to messages.properties,
     messages_de.properties`). The mark goes at the next check once the other files hold the key
     on disk.
   - The fix never ends without a visible result (IMP-0536): when nothing can be added it says
     why in the status bar and changes nothing — `'<key>' not added: ` followed by the reason:
     the file has no other file in its bundle, the key is in every file already, a file cannot
     be read, or writing failed.
   - The finding has no "Fix all in file": keys are added one at a time. [OBS, GEN]
   - **Never an empty submenu (OBS-0117 G7):** before v3 the ⌥↩ list showed only the
     inspection's name for this finding, and choosing it opened an empty submenu at the left edge
     of the list. Now the row is the fix itself ("Add missing translation to default, de"); its
     submenu (→) holds the finding's options (Run inspection on…, Disable highlighting, Disable
     inspection, Edit inspection settings). No row ever opens a submenu without options, and a
     submenu that does not fit to the right of the list lies to its left in the same layout as on
     the right, the list staying where it was. [OBS]
4. **MCP:** `project_dictionary` lists the words and adds words (dry run by default);
   `resource_bundle` lists a bundle's files with locale, every key with its locales, and the keys
   missing per file. [ADR-0008]
   **v3:** `fix_inspection` with `properties.missing-in-bundle` at a key (0-based line and column
   as `inspect_code` gives them) applies the fix of behaviour 3: a dry run by default, which
   answers with `fix` (the title), `fixes` (the number of files) and `files` (each with its
   workspace-relative path and new text) and writes nothing; `dry_run=false` (the `write` right)
   writes them as one change, open files through their editors as one undo group. When nothing
   can be added the call fails with the reason. `all=true` is refused: one key at a time.
   [ADR-0008]

## Acceptance criteria
- **AK-1** Reading, writing (escapes), the project folder, filtering
  (`project_dictionary::tests::reads_writes_and_filters`).
- **AK-2** Bundle names and missing keys (`properties::tests::bundles_and_their_missing_keys`).
- **AK-3** MCP (`project_dictionary_and_resource_bundles`).
- **AK-4** Window self-test: saving a word writes the dictionary file.
- **AK-5** Hand test (round 4, no. 89).

- **AK-6 (v2)** Hand test 89, Ferris IDEA's side, in a window of the test harness
  (`crates/workbench/tests/it/open_plugin_handtests.rs`: `hand_test_89_project_dictionary_and_bundle_keys`):
  a word saved to the project dictionary lands in `.idea/dictionaries/project.xml` in the
  reference's format, and `b`, missing in `Messages_de.properties`, is marked in the default file.
  AK-5, next to the reference, follows `docs/research/PLAN-F081.md`.

- **AK-7 (v3)** A key only the French file has names `default, de` and carries the key and the
  locales; the fix's new texts add `key=` to the base and German files, a file without a final
  line break included; refused with the reason when the key is everywhere, a file cannot be read
  or the bundle has one file (`properties::tests::missing_key_names_default_and_is_added_or_refused`,
  `rename::tests::edit_between_two_texts`); Inspect Code over the project reports the key as an
  error with the fix "Add missing translation to default", and ⌥↩ carries the key
  (`inspections::tests::missing_bundle_keys_in_inspect_code_with_their_fix`).
- **AK-8 (v3)** MCP: `inspect_code` and `properties_file` report the key with `default, de`;
  `fix_inspection` as a dry run lists two files and their texts and writes nothing, written adds
  the key, then the finding is gone and the call fails; `all` refused; `check_grammar` no longer
  reports `Quixlet` once `quixlet` is in the project dictionary
  (`crates/mcp/tests/it/protocol.rs`: `grammar_gaps_over_mcp`).
- **AK-9 (v3)** Window tests of the harness: the key is marked as an error with `default, de`;
  ⌥↩ lists "Add missing translation to default, de" first, → opens its submenu with its options,
  ←, Enter adds it to the two open files in
  their editors and the status bar names them, one ⌘Z in the German editor takes it out of both
  (`f081_grammar_gaps::missing_bundle_key_fix_in_open_files_is_one_undo_step`); with the files
  closed it writes them, the mark goes, and a second fix says why it adds nothing
  (`f081_grammar_gaps::missing_bundle_key_fix_writes_closed_files`).

## Out of scope
A bundle editor showing all locales side by side, user-level and other named dictionaries,
removing words in the UI (edit the file). v3: "ignore these locales" for a key (the reference
offers it, OBS-0117 G7 names it too: left for a later version); checking the other files' unsaved editor texts for the mark (they are read from
disk).

## Known deviations from the reference
v3: the base file is named `default` in the list of locales that lack a key, where the reference
leaves its name empty (IMP-0534). The fix that adds a missing key always changes the files or says
why it cannot; the reference's offer changed nothing and said nothing in OBS-0117 (IMP-0536). The
key is added with an empty value.
