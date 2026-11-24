# SPEC-0155: Copyright, Properties and grammar checks

- **Version:** 8 · **Date:** 2026-11-23 (v8: `is is` and every other repeated word but `that that`
  and `had had`; words with a capital first letter checked for typos; English translation values
  of a resource bundle checked, other languages' not; "Apply all writing suggestions in file" in
  ⌥↩ and over MCP; the scope of the checks per file type and context in Settings and over MCP —
  behaviours 7, 9, 13, 14, AK-16 to AK-20; OBS-0117 G1, G2, G5, G6, IMP-0535);
  2026-11-19 (v7: spellings of the other English variant and mixed
  spelling as marks of their own, calmer than a typo, with the kind over MCP — behaviours 7, 9,
  AK-14, AK-15; OBS-0115 S2, S3; v6 is the typo latency of #1095); v6: the replacements of a typo within a bound — ⌥↩
  shows them within 200 ms, one search takes at most 50 ms on the built-in en_US, the word list
  is read in the background at start; behaviour 7, AK-13; window check 2026-11-18 of F-081: the
  menu with the suggestions for `mesage` came after 3–5 s); 2026-11-18 (v5: F-081 side by side with the reference — properties
  severities, behaviour 5; line-comment notices byte for byte as the reference's, behaviour 2;
  Update Copyright over a scope, behaviours 9 and 11; up to five typo suggestions, behaviour 7;
  colours of properties files, behaviour 12 — OBS-0091 G1, P1, P2, OBS-0092 C1, C2); 2026-11-16 (v2: both places of a duplicate key are marked, each naming
  the other, and ⌥↩ offers "Remove property"; typos with the built-in word list of SPEC-0159 v2 —
  OBS-0066 P1, G1; v3: "Ignore '<text>'" and "Disable rule" for grammar findings — OBS-0066 G2; v4:
  the hand test's steps in a window test, AK-8; v1: 2026-11-03)
- **Sources:** GEN (the reference bundles open plugins for copyright notices — a profile with a
  notice template, "Update Copyright" writing it as a comment at the top of a file —, for
  `.properties` files — duplicate keys, trailing spaces — and Grazie, which checks grammar and
  spelling of comments, Markdown and text; F-081), STD (`.properties` format of
  `java.util.Properties.load`: `#`/`!` comments, `=`/`:`/white space separators, backslash
  continuation and escapes, `\uXXXX`; Hunspell `.dic` word lists), DOC (action id
  `UpdateCopyright`; the project files `.idea/copyright/profiles_settings.xml` and
  `.idea/copyright/<profile>.xml` with the options `notice`, `keyword`, `myName`), SPEC-0098,
  SPEC-0125, ADR-0008; v2: OBS-0066 P1 (the reference marks every place of a duplicate key and
  offers to remove the property at the caret), SPEC-0159 v2; v3: OBS-0066 G2 (for `an user` the
  reference offers, besides the correction, to ignore that text and to configure the rule),
  SPEC-0315 v2 (the project's inspection profile), SPEC-0125 v2 behaviour 8; v5: OBS-0091 (the
  reference reports a duplicate key and a key missing in another file of the bundle as errors and
  trailing spaces as a warning; it colours keys, separators and values of a properties file and
  names its type Properties; for `mesage` it offers the common word `message` first, Ferris IDEA the
  rare `menage` alone), OBS-0092 (the reference writes a Python notice as `#  ` lines — two spaces
  after `#` — and a shell notice between two lines holding only `#`; it asks each time whether to
  update the whole project, the file or a scope; it leaves a Java file outside a source folder
  alone), SPEC-0159 v3 (word frequency classes), SPEC-0098 v3 behaviour 7 (the scope dialog),
  SPEC-0004 v11 (the Properties language); v6: the window check of F-081 on 2026-11-18 (the
  reference shows its menu on a typo at once), GEN (Damerau–Levenshtein distance, filtering by
  length and letters before comparing), SPEC-0076 v12 behaviour 18; v7: OBS-0115 (the reference marks a spelling of the
  other English variant as a style hint in a colour of its own, apart from typos, offers the
  spelling of the chosen variant, and marks `organize` beside `organise` in one text separately),
  SPEC-0159 v5 behaviour 8 (American and British words and their counterparts), ADR-0009 (marks:
  a hint is drawn calmer than a problem), SPEC-0163 (project dictionary); v8: OBS-0117 (the
  reference reports `is is` as a repeated word and checks capitalised words such as `Quixlet`; it
  offers to apply all writing suggestions of a file at once as one undo step; it checks German
  and French translation values with its English word list, IMP-0535; addendum N7: its Grammar
  and Style page sets the scope per file type and context, string literals off by default),
  SPEC-0105 (the Settings dialog), SPEC-0315 (the inspection profile), SPEC-0159 v6 behaviour 9,
  SPEC-0163 v3
- **Feature matrix rows:** F-081

## Purpose
Notices, message bundles and the prose around the code are kept in order without leaving the IDE,
as with the reference's bundled plugins. The functions are rebuilt from the formats and the
behaviour; no plugin code was read or ported.

## Behaviour
1. **Copyright profile:** the project's notice template comes from `.ferris-idea/copyright.txt` (an
   optional first line `keyword: …` sets the word that marks an existing notice, default
   `Copyright`), else from the reference's `.idea/copyright/` folder (the profile
   `profiles_settings.xml` names as default, or the only one). `$today.year`, `$file.fileName`,
   `$project.name` and `$username` are replaced. [GEN, DOC]
2. **Update Copyright** (Code menu, `UpdateCopyright`): the notice is written as the file's
   comment — `/* … */` with ` * ` lines in C-like languages, `<!-- … -->` in HTML, XML and
   Markdown, `#`, `--` or `//` line comments otherwise — after a shebang, an XML declaration,
   `<?php` or a Python encoding line, with a blank line after it. A leading comment holding the
   keyword is replaced; an up-to-date notice and languages without comments (JSON) are left alone
   and the status bar says so. One undo step; the caret stays. [GEN]
   **v5 — line comments as the reference writes them:** a Python notice puts two spaces between
   `#` and the text (`#  Copyright 2026 Demo`); a shell script's notice gets a line holding only
   `#` before and after it (`#`, `# Copyright 2026 Demo`, `#`). An empty line of the template is a
   bare `#` in both. Other languages keep the v1 form (Ruby, YAML, TOML, Dockerfile and R with `# `
   lines and no frame: the reference's form for them is not observed). The comment is followed by
   one blank line, as before. So a notice Ferris IDEA writes is the one the reference writes, byte for
   byte, and a run of either program over a notice of the other changes nothing: a notice that is
   already in this form is up to date. A notice in the v1 form of Ferris IDEA (`# ` lines in Python,
   no frame in a shell script) is rewritten once into this form. [OBS]
3. Without a profile the status bar says where to add one. [GEN]
4. **Properties files** are read as `java.util.Properties` does (comments, separators,
   continuation lines, escapes). [STD]
5. **Properties checks** while editing and in Inspect Code: a key that occurs again (warning,
   naming the first line) and spaces at the end of a value (weak, ⌥↩ "Remove trailing spaces"; an
   escaped last space is meant and stays). [GEN]
   **v2:** every place of a key that occurs more than once is a warning, each naming the other
   lines of the key ("also on line 3"). ⌥↩ on one of them offers "Remove property", which removes
   that property — its line and the lines its value continues on with a backslash — as one undo
   step; the other places stay. Which place to keep is the user's choice, so the inspection has no
   "Fix all in file". [OBS, GEN]
   **v5 — severities as the reference's:** a duplicate key (every place of it) and a key that
   another file of its resource bundle lacks (SPEC-0163 behaviour 3) are errors; spaces at the end
   of a value are a warning. The project's inspection profile can still change each level
   (SPEC-0315). [OBS]
6. **MCP:** `properties_file` returns the properties with key, value and line and the problems.
   v2: every place of a duplicate key is a problem, with the title of its fix; `fix_inspection`
   with `properties.duplicate-key` at one of them removes that property, and refuses `all`.
   [ADR-0008]
7. **Grammar** in comments of code, Markdown prose (not code, links or tags), plain text files
   and the comments of properties files: a repeated word (`the the`; not `that that`, `had had`),
   `a`/`an` before a word (`an user` → `a user`, `a hour` → `an hour`; words without vowels such
   as `npm` and all-caps abbreviations are left alone). Typos (lowercase words of four letters or
   more that are not code-like) are reported with an English Hunspell word list — v2: the
   built-in en_US or the dictionary chosen or downloaded in Settings (SPEC-0159 v2); v1 read a list
   of the system (`/usr/share/hunspell`, Homebrew's or `~/Library/Spelling`) —, with
   inflections, programming words, the user's learned words and the words of the file's code
   accepted; a suggestion one edit away is offered. Prose that reads as German is not checked
   for articles and typos. All are weak findings with ⌥↩ fixes. [GEN]
   **v5 — several suggestions, the common word first:** ⌥↩ on a typo lists up to five
   replacements ("Replace with '<word>'"), then "Save '<word>' to project dictionary". They are the
   listed words one edit away (a letter swapped with its neighbour, left out, replaced or added),
   then, while fewer than five are found, those two edits away; within the same distance the more
   common word comes first by the frequency classes of SPEC-0159 v3 behaviour 7, words of no class
   last, ties in alphabetical order. The finding's message names the first one ("did you mean
   \"message\"?" for `mesage`, not the rarer `menage`). [OBS, GEN]
   **v6 — at once:** ⌥↩ on a typo shows the list with its replacements within 200 ms of the key
   press on a normal project, as the reference does, also when the editor's own inspection has not
   looked for them yet. One search for a word's replacements takes at most 50 ms on the built-in
   en_US, even in a build without optimisation: the words of the list are kept by their length
   with the letters they contain, and only the words whose length differs by at most two and whose
   letters differ in at most four are compared, by the number of edits between them (a swap of
   neighbours counting one). The words found and their order are those of v5, which tried every
   string one and two edits away and took up to 0.4 s per word without optimisation. The word list
   and this index are read on a background thread when the window starts and when the dictionary
   changes in Settings, so neither the first inspection nor the first ⌥↩ waits for them. [GEN;
   window check 2026-11-18]
   **v7 — the other English variant and mixed spelling:** two further spelling findings, decided
   by SPEC-0159 v5 behaviour 8 for the same words the typo check looks at (prose that reads as
   English, not code-like, four letters or more), here also a word with a capital first letter and
   the rest lowercase (`Colour` at the start of a sentence; the replacement keeps the capital):
   - **Other variant** (`grazie.other-variant`, "Spelling of the other English variant"): with
     one variant chosen, a word of the other variant that has a counterpart, e.g. `color` with
     English (United Kingdom): message `American spelling "color" (British English: "colour")`,
     fix "Replace with 'colour'", then "Save 'color' to project dictionary". It is not also a typo.
   - **Mixed spelling** (`grazie.mixed-spelling`, "Mixed English spelling"): where both spellings
     of one word family occur in the checked text (a file, or the text given to `check_grammar`),
     each place of the less used one gets a hint: message `Mixed spelling: "organise" here,
     "organize" elsewhere in this text`, fix "Replace with 'organize'". With one variant chosen
     only a spelling of the other variant is so marked — when the chosen spelling occurs at least
     as often, instead of the other-variant mark — and never the chosen spelling, so no fix ever
     offers the other variant's spelling; with several dictionaries in use the less used spelling
     is marked, on a tie the one that first occurs later. Each word gets at most one spelling mark.
   - Both are weak findings, but drawn calmer than a typo (ADR-0009): a dotted underline in the
     colour of `INFO_ATTRIBUTES` instead of the wavy one of `WEAK_WARNING_ATTRIBUTES`. The
     Problems tool window, Inspect Code and the inspection profile list them under their names;
     the profile can turn each off. Words of the project dictionary (SPEC-0163) and names of the
     file's code are not reported. [OBS-0115 S2, S3, GEN]
   **v8 — repeated words, capitalised words, translation values (OBS-0117 G1, G2, IMP-0535):**
   - A repeated word is every word that follows itself with only spaces between, `is is`,
     `do do` and `no no` included. Only two pairs that a correct sentence can hold stay
     unreported, and the list stays this short: `that that` ("he said that that was fine") and
     `had had` ("she had had enough"). v1 to v7 also left `is is`, `do do`, `bye bye` and
     `no no` alone.
   - Typos are also looked for in words with a capital first letter and the rest lowercase (four
     letters or more, not code-like): a name such as `Quixlet`, the first word of a sentence. The
     word's lowercase form is looked up in the word list (with its inflections), the words of the
     file's code and the project dictionary, so a dictionary word at the start of a sentence and a
     project word in any case are no typos. The suggestions keep the capital (`Recieve` →
     `Receive`). Words in capitals (`QUIXLET`, abbreviations) and words with a capital inside
     (`QuixLet`, CamelCase, already code-like) are not checked, so code names in prose do not
     flood the file with marks.
   - The values of a properties file are prose too when the file is a translation in English:
     a file of a resource bundle (SPEC-0163 behaviour 3) with at least one other file, being
     the base file or an English locale (`en`, `en_GB`). The values of another language's file
     are checked with that language's dictionary when one is installed; the catalog
     (SPEC-0159) holds English dictionaries only, so they are not spell-checked at all
     (SPEC-0159 v6 behaviour 9). A properties file outside a bundle holds settings, not prose:
     its values stay unchecked. Keys are never checked. [OBS-0117, GEN]
8. **Inspect Code** and `inspect_code`/`fix_inspection` include these findings. [SPEC-0098,
   SPEC-0125]
9. **MCP:** `check_grammar` checks a text (e.g. a commit message, optionally as Markdown) and
   says whether typos were checked; `update_copyright` previews or writes the notice (refusing a
   file changed meanwhile). [ADR-0008]
   **v5:** each typo of `check_grammar` carries `suggestions`, the list of behaviour 7 v5.
   `update_copyright` takes `scope`: `file` (default, the file named by `path`), `folder` (the
   files below the folder named by `path`) or `project` (the project's own files, `path` not
   needed), as in behaviour 11. With a scope other than `file` it returns, per file that would
   change or changed, its path, and the count of files left alone because they are up to date or
   have no comments; the dry run is still the default. [ADR-0008]
   **v7:** each spelling problem of `check_grammar` carries `kind` — `typo`, `other_variant` or
   `mixed_spelling` — and its `replacement` (for a variant: the chosen or the more used
   spelling); `check_grammar` with `dictionary` decides the variant from those dictionaries.
   Each spelling finding of `inspect_code` carries the same `kind` and `replacement`. [ADR-0008]
   **v8:** `check_grammar` follows the project dictionary as the editor does (SPEC-0163 v3
   behaviour 2) and the rules of behaviour 7 v8. `fix_inspection` with the title "Apply all
   writing suggestions in file" at any grammar or spelling finding does what behaviour 13 does:
   a dry run by default, which returns the edits, their count as `fixes` and the new text;
   `dry_run=false` writes the file, refusing if it changed meanwhile. No new tool: the entry is
   one of the finding's fixes, as in ⌥↩ (ADR-0008). `check_grammar` takes `context`:
   `commit_message` checks the text only while commit messages are on in the scope of
   behaviour 14 (otherwise `checked` is false, no problems, and `reason` says where to turn them
   on); without it the text is checked as before. Every answer carries `scope`, the scope in
   effect. `set_ide_settings` takes `grammar` — `commit_messages`, and for one `file_type` the
   switches `enabled`, `comments`, `documentation`, `string_literals` (an unknown file type, or
   switches without one, are refused and nothing changes; dry run by default); `inspect_code` and
   `fix_inspection` follow the scope of the settings file. [ADR-0008]
10. **Ignore and disable (v3):** ⌥↩ on a repeated word or a wrong article lists, after the
    correction, "Ignore '<text>'" and "Disable rule '<name>'". The text is the article with the
    word after it (`an user`) or both words (`the the`).
    - "Ignore" keeps the rule and the text in the project's `.ferris-idea/grammar-ignore.toml` (the
      nearest such file above the edited file, else the one in the window's project folder;
      written through a temporary file). From then on that rule's finding on the same text —
      compared without case and with runs of white space as one space — is not reported in the
      project's files; open editors drop it at once. The file can be edited or removed by hand.
    - "Disable rule" turns the rule's inspection off in the project's inspection profile
      (SPEC-0315 v2), as "Disable inspection" in the options of a finding does (SPEC-0125 v2
      behaviour 8); the settings page turns it on again.
    - Typos keep "Save to project dictionary" (SPEC-0163) instead.
    - MCP: `fix_inspection` with the title "Ignore '<text>'" or "Disable rule '<name>'" does the
      same (a dry run by default, which only names it); `inspect_code` and `check_grammar`
      follow the ignored texts of the project. [OBS, GEN, ADR-0008]
11. **v5 — Update Copyright over a scope:** Code › Update Copyright… (and Find Action) first opens
    a small dialog as Inspect Code does (SPEC-0098 v3 behaviour 7), titled "Update Copyright:
    scope", with **Whole project**, **Current file** (when an editor is active; chosen at the
    start, so Enter keeps the v1 behaviour) and **Folder of the current file**; ↑/↓ choose,
    **Run** (Enter) updates, **Cancel** (Escape) closes. The whole project means the project's own
    files as Inspect Code takes them (not ignored, excluded or library files); the current file
    or its folder whatever they hold, because they were named. Each file gets the notice as in
    behaviour 2: an open editor as one undo step, any other file on disk through a temporary file.
    The status bar then says how many files were updated and how many were already up to date.
    [OBS, GEN]
12. **v5 — Properties files in colour:** `.properties` files have the language Properties
    (SPEC-0004 v11 behaviour 22): the status bar names it, ⌘/ comments lines with `#`, and the
    editor colours them with Ferris IDEA's own small lexer of the format of behaviour 4, without a
    grammar library: comment lines (`#`, `!`) as comments, the key as a key, the separator
    (`=`, `:` or the white space that ends the key) as an operator, the value as a string, and
    escapes in key and value (`\uXXXX`, `\t`, `\n`, `\\`, `\=`, `\:`, a backslash at a line end
    that continues the value) as escapes; a continued value keeps the string colour on its next
    lines. [OBS, STD]

13. **v8 — Apply all writing suggestions in file (OBS-0117 G5):** ⌥↩ on a grammar or spelling
    finding (repeated word, article, typo, other variant, mixed spelling) lists, after that
    finding's own entries, "Apply all writing suggestions in file" when the file has at least two
    such findings with a fix (with one, its own entry does the same). It replaces every such
    finding of the file by its first suggestion — the one its own first entry writes — in file
    order; a finding whose range overlaps one already taken is skipped. The findings are those the
    editor shows: the project dictionary, ignored texts (behaviour 10) and the inspection profile
    apply, typos without a suggestion are left. The change is one undo step: one ⌘Z brings the
    whole text back. [OBS, GEN]
14. **v8 — Scope of the checks (OBS-0117 G6):** Settings › Editor › Natural Languages › Grammar
    and Style (a page of its own under Editor, found by "grammar", "string literals", "commit
    messages" …) sets where grammar and spelling are checked, kept in the IDE settings
    (`[editor.grammar]`, the same for every project):
    - **Commit messages** (on by default): applies where a commit message is checked, today
      `check_grammar` with the context `commit_message` (behaviour 9).
    - **One row per file type**, every language of SPEC-0004 by its status-bar name and
      `Plain text`, with four switches: the file type itself (off: nothing in such files is
      checked), **Comments**, **Documentation** (documentation comments `/** … */`, Rust's `///`
      and `//!`, and Python docstrings — a string that stands as a statement of its own) and
      **String literals** (off by default: they mostly hold keys, formats and messages for
      machines). Markdown and plain text are prose as a whole and follow the file-type switch
      alone; a properties file's comments follow Comments, its English values (behaviour 7 v8)
      the file-type switch. Only file types that differ from the defaults are written.
    - **The writing rules** (repeated word, article, typo, other variant, mixed spelling) with
      their switch; it is the switch of the project's inspection profile (SPEC-0315), the same
      the Inspections page shows, so the page needs an open project for them.
    - Keys: ↑/↓ choose the row, ←/→ the column of a file type's row, Space or Enter turn the
      switch over; a click on a switch does the same. Open editors follow at once, also when the
      settings file changes through `set_ide_settings` (within a second, as SPEC-0159 v5
      behaviour 3). [OBS, GEN]

## Acceptance criteria
- **AK-1** Notices per language, shebang/XML/encoding preambles, updating and leaving alone, the
  reference's profile files with XML entities, the year (`copyright::tests`); properties parsing
  and checks (`properties::tests`); repeated words, articles, German prose, typos with inflections
  and suggestions, Hunspell lists, Markdown prose (`grazie::tests`).
- **AK-2** MCP: notice preview, write and up-to-date; properties with continuation and duplicate;
  `inspect_code` on a properties file; repeated word and article in a commit message
  (`copyright_properties_and_grammar_tools`).
- **AK-3** Window self-test: Update Copyright under a shebang; duplicate key and trailing spaces
  in a properties file, ⌥↩ removes the spaces; a repeated word in a text file, ⌥↩ removes it.
- **AK-4** Hand test (round 4, no. 81): a project with `.idea/copyright`, a messages file and a
  README next to the reference.
- **AK-5** No false findings on Ferris IDEA's own sources and specifications (269 files, checked
  2026-11-03).
- **AK-6 (v2)** Both places of a duplicate key, each naming the other line; "Remove property" at
  each, removing a continued value with it; no "Fix all" (`properties::tests::
  reports_duplicates_and_trailing_spaces`, `inspections::tests::duplicate_keys_offer_remove_property`).
  MCP: both places, `fix_inspection` removes one, `all` refused
  (`copyright_properties_and_grammar_tools`). Window test of the harness: ⌥↩ on the second place,
  Enter removes it, one ⌘Z brings it back
  (`f081_properties_grammar::duplicate_key_remove_property_with_alt_enter`).
- **AK-7 (v3)** The text of an article and of a repeated word; reading, writing and matching
  ignored texts without case and with spaces folded; findings of an ignored text dropped, other
  texts kept (`grammar_ignore::tests`, `grazie::tests::phrase_of_a_finding`); ⌥↩ lists the
  correction, "Ignore 'an user'" and "Disable rule 'Article before a vowel or consonant'", and
  typos have no "Ignore" (`inspections::tests::grammar_findings_offer_ignore_and_disable_rule`).
  MCP: `fix_inspection` with both titles as a dry run and written; `inspect_code` and
  `check_grammar` follow the file (`grammar_ignore_and_disable_rule_tools`). Window test of the
  harness: ⌥↩ on `an user`, "Ignore" chosen, the mark goes and the file holds the text; "Disable
  rule" on a repeated word turns the rule off in the profile
  (`f081_properties_grammar::grammar_finding_ignore_and_disable_rule`).

- **AK-8 (v4)** Hand test 81, Ferris IDEA's side, in a window of the test harness
  (`crates/workbench/tests/it/open_plugin_handtests.rs`: `hand_test_81_copyright_properties_and_grammar`):
  Code › Update Copyright in a Java and a Python file writes the notice at the top and a second run
  changes nothing; a properties file's duplicate key and trailing spaces are marked and ⌥↩ removes
  the spaces; a repeated word and a wrong article in a README are marked and ⌥↩ corrects each. AK-4,
  next to the reference, follows `docs/research/PLAN-F081.md`.

- **AK-9 (v5)** Notices byte for byte as OBS-0092 recorded them: `tool.py` gets
  `#  Copyright (c) 2026 Demo\n#  All rights reserved.\n\n` before its code, `notice.sh`
  `#!/bin/sh\n#\n# Copyright (c) 2026 Demo\n# All rights reserved.\n#\n\n` after its shebang; both
  are then up to date, a file in the reference's form is left alone and a v1 notice is rewritten
  (`copyright::tests::line_comment_notices_as_the_reference_writes_them`). The scope: every own
  file of a project, a folder, one file (`copyright::tests::files_of_a_scope`). MCP: `scope`
  `project` and `folder` as a dry run and written (`copyright_properties_and_grammar_tools`).
  Window test of the harness: Update Copyright opens the scope dialog, Enter on Current file and
  on Whole project (`open_plugin_handtests::hand_test_81_copyright_properties_and_grammar`,
  `f081_properties_grammar::update_copyright_over_the_whole_project`).
- **AK-10 (v5)** Severities: a duplicate key and a key missing in the bundle are errors, trailing
  spaces a warning (`inspections::tests::properties_severities_as_the_reference`).
- **AK-11 (v5)** `mesage` gives `message` first among at most five suggestions, ranked by distance,
  then frequency class (`grazie::tests::suggestions_ranked_by_distance_then_frequency`); ⌥↩ lists
  them before "Save to project dictionary" (`inspections::tests::typos_offer_up_to_five_suggestions`);
  `check_grammar` returns them (`copyright_properties_and_grammar_tools`).
- **AK-13 (v6)** The distance counts the steps of v5 for every string one and two steps from a
  typo, an apostrophe and a letter outside `a`–`z` included
  (`grazie::tests::edit_distance_counts_the_steps`); the index finds the same words in the same
  order as v5's search on the built-in en_US (`grazie::tests::index_search_finds_what_every_edit_finds`);
  each of five typos takes under 50 ms in a test build without optimisation
  (`grazie::tests::suggestions_found_within_the_bound`); `prepare` builds the index of the current
  word list (`grazie::tests::prepare_builds_the_index`). Window test of the harness: one ⌥↩ on
  `mesage` opens the list with message, menage, manage, messages, usage and "Save 'mesage' to
  project dictionary" within 200 ms (`f081_properties_grammar::typo_suggestions_open_at_once`).
- **AK-12 (v5)** The lexer: key, separator, value, comment and escapes, a continued value, a key
  ended by white space (`properties::tests::highlights_keys_separators_values_comments_escapes`);
  window test: a `.properties` file is coloured, the status bar names Properties and ⌘/ writes `#`
  (`f081_properties_grammar::properties_files_are_coloured_and_commented`).
- **AK-14 (v7)** With en_GB `color`, `center`, `gray`, `organize`, `behavior` are other-variant
  findings with `colour`, `centre`, `grey`, `organise`, `behaviour` and not typos, `recieve` stays a
  typo; with en_US the reverse; `Colour` gives `Color`; `program` is no finding with either;
  with `en_GB,en_US` no other-variant finding; mixed spelling on the minority form in both
  modes, on a tie the later one, never on the chosen spelling
  (`grazie::tests::other_variant_both_ways`, `grazie::tests::mixed_spelling_on_the_minority`);
  ⌥↩ lists the replacement and "Save to project dictionary", and a saved word is not reported
  (`inspections::tests::variant_findings_and_their_fixes`). MCP: `check_grammar` gives `kind` and
  `replacement` for a typo, an other-variant and a mixed-spelling problem, `inspect_code` the
  same (`crates/mcp/tests/it/protocol.rs`: `spelling_kinds_over_mcp`).
- **AK-15 (v7)** Window test: the other-variant mark is a dotted underline in the info colour,
  apart from the typo's wavy one, and ⌥↩ on it replaces the word by the chosen spelling
  (`f081_spelling_variants::other_variant_mark_and_its_fix`).

- **AK-16 (v8)** `is is`, `Is is`, `do do`, `no no` are repeated words, `that that` and `had had`
  not, and the list holds those two only (`grazie::tests::repeats_every_word_but_that_and_had`);
  `Quixlet` is a typo, `Recieve` a typo with `Receive`, a list word at a sentence start, words in
  capitals, CamelCase and a word of the code are not
  (`grazie::tests::checks_capitalised_words`); the values of the English base file of a bundle
  are checked, the German ones not
  (`inspections::tests::missing_bundle_keys_in_inspect_code_with_their_fix`,
  `properties::tests::missing_key_names_default_and_is_added_or_refused`); "Apply all writing
  suggestions in file" takes the first suggestion of three findings, is listed last on any of
  them and not offered for one finding alone (`inspections::tests::apply_all_writing_suggestions`,
  `inspections::tests::grammar_findings_offer_ignore_and_disable_rule`).
- **AK-17 (v8)** MCP: `check_grammar` reports `is is` and `Quixlet`, then, with `quixlet` in the
  project dictionary, no longer `Quixlet`; `fix_inspection` with "Apply all writing suggestions in
  file" as a dry run returns two fixes and the corrected text and leaves the file
  (`crates/mcp/tests/it/protocol.rs`: `grammar_gaps_over_mcp`).
- **AK-18 (v8)** Window test of the harness: ⌥↩ on `an user` in a README with three findings lists
  the entry once, ↓ and Enter correct all three, the marks go and one ⌘Z brings the text back
  (`f081_grammar_gaps::apply_all_writing_suggestions_with_alt_enter`).

- **AK-19 (v8)** The scope picks comments, documentation comments, string literals and a Python
  docstring by their switches, string literals off by default, nothing for a file type turned
  off (`inspections::tests::grammar_scope_picks_the_contexts`); the page's rows, their switches,
  a scope back at the default not kept, the page copied by Revert
  (`settings_model::tests::grammar_rows_and_switches`); every setting survives a restart
  (`ide_settings::tests::every_setting_survives_a_restart`); MCP: `check_grammar` returns the
  scope and with `commit_message` stops checking once commit messages are off;
  `set_ide_settings` `grammar` as a dry run, an unknown file type and switches without one
  refused (`crates/mcp/tests/it/protocol.rs`: `grammar_gaps_over_mcp`).
- **AK-20 (v8)** Window test of the harness: the Grammar and Style page lists the file types and
  the rules; Space on Lua takes the mark of the open Lua file's comment away at once, Space again
  brings it back (`f081_grammar_gaps::grammar_scope_in_settings`).

## Out of scope
v8: grammar rule groups per language and exceptions of the reference's Grammar and Style page
(the rules are Ferris IDEA's five; texts are excepted with "Ignore", behaviour 10).
Per-language formatting options, updating on save or commit; resource bundles (several locales
of one bundle), references from code to keys; Grazie's style rules, other languages, spelling
without a word list, a project dictionary. v3: ignoring one place only, a list of ignored texts
in Settings, and rule options beyond on and off. v5: named custom scopes for Update Copyright
(Ferris IDEA has none; the folder of the current file stands for them); the reference's notice form
for `#` languages other than Python and Shell until it is observed.

## Known deviations from the reference
v8: translation values of a language other than English are not spell-checked with the English
word list; the reference checks them with it and reports correct words such as `Wiedersehen` and
`Seulement` as typos (OBS-0117, IMP-0535).
v7: with several dictionaries in use (SPEC-0159 v4) no spelling is reported as the other variant,
only mixed spelling. The reference shows the other variant as a style hint with a colour of its
own; Ferris IDEA draws it as a dotted underline in the info colour.
v5: a Java file outside a source folder gets the notice; the reference leaves it alone without a
word (OBS-0092). A file the user names gets what was asked for, and a project without a build
model has no source folders at all. Named custom scopes are not offered (see Out of scope).
v2: typos are checked with the built-in en_US (SPEC-0159 v2), as the reference checks with its
bundled list; the v1 deviation is gone. German prose is skipped instead of being checked with
German rules.
