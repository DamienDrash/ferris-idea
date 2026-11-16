# SPEC-0155: Copyright, Properties and grammar checks

- **Version:** 3 · **Date:** 2026-11-16 (v2: both places of a duplicate key are marked, each naming
  the other, and ⌥↩ offers "Remove property"; typos with the built-in word list of SPEC-0159 v2 —
  OBS-0066 P1, G1; v3: "Ignore '<text>'" and "Disable rule" for grammar findings — OBS-0066 G2)
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
  SPEC-0315 v2 (the project's inspection profile), SPEC-0125 v2 behaviour 8
- **Feature matrix rows:** F-081

## Purpose
Notices, message bundles and the prose around the code are kept in order without leaving the IDE,
as with the reference's bundled plugins. The functions are rebuilt from the formats and the
behaviour; no plugin code was read or ported.

## Behaviour
1. **Copyright profile:** the project's notice template comes from `.idecraft/copyright.txt` (an
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
8. **Inspect Code** and `inspect_code`/`fix_inspection` include these findings. [SPEC-0098,
   SPEC-0125]
9. **MCP:** `check_grammar` checks a text (e.g. a commit message, optionally as Markdown) and
   says whether typos were checked; `update_copyright` previews or writes the notice (refusing a
   file changed meanwhile). [ADR-0008]
10. **Ignore and disable (v3):** ⌥↩ on a repeated word or a wrong article lists, after the
    correction, "Ignore '<text>'" and "Disable rule '<name>'". The text is the article with the
    word after it (`an user`) or both words (`the the`).
    - "Ignore" keeps the rule and the text in the project's `.idecraft/grammar-ignore.toml` (the
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
- **AK-5** No false findings on IDECraft's own sources and specifications (269 files, checked
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

## Out of scope
Copyright scopes, per-language formatting options, updating on save or commit; resource bundles
(several locales of one bundle), references from code to keys; Grazie's style rules, other
languages, spelling without a word list, a project dictionary. v3: ignoring one place only, a
list of ignored texts in Settings, and rule options beyond on and off.

## Known deviations from the reference
v2: typos are checked with the built-in en_US (SPEC-0159 v2), as the reference checks with its
bundled list; the v1 deviation is gone. German prose is skipped instead of being checked with
German rules.
