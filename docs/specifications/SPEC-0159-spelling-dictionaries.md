# SPEC-0159: Dictionaries for the typo check

- **Version:** 6 · **Date:** 2026-11-23 (v6: text is checked with the dictionary of its own
  language, so translation values of another language are not checked with an English list,
  behaviour 9 — OBS-0117, IMP-0535); 2026-11-19 (v5: a change of the dictionaries re-checks every open
  editor at once, also when it comes through MCP, behaviour 3; American and British spellings of
  the same word told apart with the two built-in lists, behaviour 8 — OBS-0115 S1, S2, S3);
  2026-11-19 (v4: English (United Kingdom) built in from SCOWL, several
  dictionaries at once, the choice also over MCP — PLAN-F081 point 4); 2026-11-18 (v3: word frequency classes rank the suggestions for a
  typo, behaviour 7 — OBS-0091 G1); 2026-11-15 (v2: English (United States) built in, so typos are checked
  from the first start without a download — OBS-0066 G1)
- **Sources:** GEN (IDEs check spelling with a word list per language that the user can choose and
  download; F-081 Grazie), STD (Hunspell `.dic`/`.aff` format: word count, `word/FLAGS`, `PFX`/`SFX`
  rules with strip, affix and condition), OSS (Hunspell dictionaries of the LibreOffice project,
  `github.com/LibreOffice/dictionaries`, folder `en`, commit `4f1df64`; SCOWL (BSD-style) for en_US,
  en_CA, en_AU, LGPL for en_GB, en_ZA), SPEC-0155, ADR-0008; v2: OBS-0066 G1 (the reference checks
  typos from its first start with an English dictionary it brings along; Ferris IDEA found none in a
  fresh settings folder on macOS, which has no system Hunspell list), OSS (`README_en_US.txt` of the
  same commit: the licence terms of SCOWL, VarCon, Ispell, WordNet and UKACD and the public-domain
  parts, all permissive with notices kept); v3: OBS-0091 G1 (the reference offers the common word
  first: `message` for `mesage`, where Ferris IDEA offered only the rare `menage`), OSS (SCOWL
  release 2020.12.07 by Kevin Atkinson, `scowl-2020.12.07.tar.gz` from the project's SourceForge
  downloads, SHA-256 `5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc`: the files
  `final/english-words.*`, `american-words.*` and `british-words.*` of the levels 10, 20 and 35 and
  the licence file `Copyright`; SCOWL sorts its words
  into size levels by how common they are — level 10 holds the most common English words after the
  public-domain Moby Words II frequency lists —, and its terms are the same permissive ones as for
  the built-in en_US, with the notice kept); v4: OSS (SCOWL's Hunspell dictionary English (United
  Kingdom) with "ise" spelling, release 2020.12.07 by Kevin Atkinson, `hunspell-en_GB-ise-2020.12.07.zip`
  from the folder `speller/2020.12.07` of the SCOWL project's SourceForge downloads (`wordlist`),
  MD5 `1c0d38766cd35c41e80b50f42ceecfba` as SourceForge publishes it, SHA-256
  `2ced4b2ec9d188c3b540dbb75b5143f65b990daf17437758726ff1fab5f01ad3`; it holds `en_GB-ise.dic`,
  `en_GB-ise.aff` and `README_en_GB-ise.txt`, whose terms are word for word those of the built-in
  en_US's `README_en_US.txt` — the en_US zip of the same release holds a `en_US.dic` byte for byte
  equal to the built-in one, so both lists come from one release and one licence; the Hunspell
  en_GB of the LibreOffice project is a different list under the LGPL), GEN (British spelling
  writes `colour`, `behaviour`, `centre`, `organise`; British usage also allows `-ize`; a writer
  of international English may want both spellings accepted); v5: OBS-0115 (side by side with
  the reference, F-081 hand test 85: the reference re-checks an open editor at once when the
  English variant changes, Ferris IDEA only after a restart; with British English the reference marks
  `color`, `center`, `gray` and `behavior` as the other variant, Ferris IDEA accepted `center`, `gray`
  and `organize`; with American English it marks `colour`, `centre`, `grey`, `organise` and
  `behaviour` that way, apart from typos), GEN (American and British spellings of one word differ
  in a few regular ways: `-or`/`-our`, `-er`/`-re`, `-ize`/`-ise`, `-yze`/`-yse`, a doubled `l`, an
  added `e` or `u`, and the letters `z`/`s`, `s`/`c`, `c`/`k`, `a`/`e`, `i`/`y`, `a`/`y`; many
  words are written alike in both, and British English keeps `program` for software beside
  `programme`), SPEC-0155 v7
- **Feature matrix rows:** F-081

## Purpose
Let each user pick the English they write — American, British, Canadian, Australian, South
African, or more than one of them — and get a working typo check without installing anything by
hand.

## Behaviour
1. **Catalog:** Settings › Editor › Natural Languages lists English (United States), (United
   Kingdom), (Canada), (Australia), (South Africa) with size, licence and state (size to download,
   downloaded, used). v2: English (United States) is marked as built in. v4: English (United Kingdom) is built in too,
   with SCOWL's licence; the LibreOffice project's en_GB (LGPL) is no longer offered. Each
   dictionary in use is marked. [GEN, OSS]
2. **Download:** Enter (or a click) downloads the selected dictionary's `.dic` and `.aff` from the
   LibreOffice project at the fixed commit, checks both against the size and SHA-256 Ferris IDEA knows,
   and keeps them in `dictionaries` of the user's settings folder (written through temporary files;
   a failed check keeps nothing). It is then used. Delete removes it. Downloading is the user's
   action only; no file is downloaded without it. v2: Enter on the built-in dictionary chooses it
   without a download; Delete on it removes nothing and says it is built in. v4: a built-in dictionary
   is never downloaded (the catalog's link names where its files come from); Delete on it removes
   a file of the same code left in `dictionaries` by an earlier download, else nothing. [GEN]
3. **Choice (v2):** the chosen dictionary is saved in the settings (`editor.spelling_dictionary`) and
   used for typos at once in every window. The dictionary used is the chosen one when it is
   downloaded or built in, else the first downloaded one in catalog order, else the built-in one.
   No English Hunspell list of the system is read any more: the check is the same on every
   machine. [GEN]
   v4: **several at once.** `editor.spelling_dictionary` holds one code or several separated by
   commas (`en_GB,en_US`); unknown codes and repeats are left out. The dictionaries used are the
   chosen ones that are built in or downloaded, in the order given; when none is, the first
   downloaded one in catalog order, else English (United States). A word is known when any
   dictionary used knows it, and suggestions for a typo come from the words of all of them, ranked
   as in behaviour 7, so with English (United Kingdom) alone the suggestions are British
   (`colour` for `color`). A built-in dictionary is always read from the program: a file of the
   same code in `dictionaries` (an earlier download) is not read. In Settings › Editor › Natural
   Languages, Enter uses only the selected dictionary (as before) and Space adds it to the
   dictionaries used or takes it away (the last one stays); every dictionary used is marked. With
   nothing chosen (a fresh settings folder) English (United States) stays the one used. [GEN]
   v5: **at once in open editors.** A change of the dictionaries re-checks every open editor of
   every window at once: the typo and variant marks of behaviour 8 follow the new choice without a
   restart and without an edit. This holds for a change in Settings › Editor › Natural Languages
   (Enter, Space, Delete) and for a change of `editor.spelling_dictionary` in the saved settings
   file by anything else, such as `set_ide_settings` over MCP or another Ferris IDEA process: each
   window looks at the file about twice a second and takes over the new choice within a second
   (only this setting; the others are read at the next start as before). The word lists of the new
   choice and the variant table of behaviour 8 are read off the window's thread before the editors
   are checked again. [OBS-0115 S1, GEN]
4. **Reading:** prefixes and suffixes of the `.aff` file are applied to the word list (one-character
   flags, conditions with character sets), so forms such as `input` (`in` + `put`) or `cities` are
   known. [STD]
5. **MCP:** `spelling_dictionaries` lists the catalog with URL, SHA-256 and state, the folder, the
   dictionary used and the word list file in effect; read only. `check_grammar` and `inspect_code`
   use the same dictionary. v2: each entry says whether it is built in, and the answer says whether
   the built-in word list is in effect (`word_list` is then empty). v4: the answer also gives the
   setting (`chosen`), every dictionary used in order (`in_use`), and per dictionary used its code,
   whether it is built in and its file (`word_lists`); `used` stays the first of them, `word_list`
   the file of the first downloaded one in use, and `built_in` says whether all of them are built
   in. The setting is changed with `set_ide_settings` `spelling_dictionary` (the codes as in
   behaviour 3; dry run by default, the `settings` right to write): a code that is not in the
   catalog, or neither built in nor downloaded, is refused and nothing changes; empty goes back to
   the default. `check_grammar` takes `dictionary` (the same codes) to check one text with those
   dictionaries instead of the setting, under the same rule. An agent never downloads. [ADR-0008]
6. **Built in (v2):** Ferris IDEA carries English (United States) inside the program: `en_US.dic` and
   `en_US.aff` of the same commit as the catalog, unchanged, kept in the repository beside their
   licence text (`README_en_US.txt`, unchanged). The program checks both against the catalog's size
   and SHA-256 before it reads them; a mismatch means no built-in word list, never a changed one.
   Nothing is fetched from the network at start or for the built-in dictionary. The licence text
   goes with the program and is named in the repository's notices. [OSS, GEN]
   v4: English (United Kingdom) is built in the same way: `en_GB-ise.dic`, `en_GB-ise.aff` and
   `README_en_GB-ise.txt` from SCOWL's zip (Sources), unchanged, checked against the catalog's
   size and SHA-256 before they are read. The program reads a built-in list only when it is used.
   The British list is about as large as the American one (552,106 against 551,762 bytes of
   `.dic`), so the program grows by about 0.57 MB and reading it takes about as long; an "ize"
   form such as `organize` is still accepted where the general inflection rules of SPEC-0155 find
   its stem, as British usage allows both. [OSS, GEN] v5: no longer — with English (United
   Kingdom) alone `organize` is the American spelling of behaviour 8.
7. **Frequency classes (v3):** Ferris IDEA carries a small list of common English words with their
   class: the words of SCOWL's levels 10 (class 1, the most common), 20 (class 2) and 35 (class 3)
   of its `english-words`, `american-words` and `british-words` lists, about 40,000 words, as one
   plain text file beside the built-in dictionary (`en_frequency.txt`: three comment lines naming
   the source, then a line `# class N` before the words of each class, one word per line; a word
   stands in the lowest class it is listed in, the words of a class are sorted, possessive forms
   with `'s` are left out, the text is re-encoded from Latin-1 to UTF-8) with SCOWL's licence file
   unchanged (`SCOWL-Copyright.txt`). It is compiled into the program
   and serves every dictionary of the catalog alike (the classes are English, not one country's).
   A word's class is that of its lower-cased form; a word of no list has none and counts as rarer
   than class 3. The classes only order suggestions (SPEC-0155 v5 behaviour 7); they never make a
   word known. No file of the reference was read for this. [OSS, GEN]

8. **American and British spellings (v5).** Ferris IDEA tells the spellings of one word apart with
   the two built-in lists alone, read as behaviour 4 reads them (affixes applied, lowercase) but
   without the entries that start with a capital letter: those are names (`Gray`, `Kokomo`), which
   say nothing about how a common word is spelt. No list of words is written by hand.
   - **Words of one variant:** an *American word* is one the en_US list holds and the en_GB list
     does not (`color`, `center`, `gray`, `organize`, `behavior`, `analyze`, `judgment`); a
     *British word* the reverse (`colour`, `centre`, `grey`, `organise`, `behaviour`, `analyse`,
     `judgement`, `programme`). With the 2020.12.07 lists these are about 3,000 American and
     3,060 British words. A word both lists hold is neither — `program`, `license`, `practice`,
     `meter`, `dialog`, `check`, `tire` — and is never reported as a variant.
   - **Counterpart:** the counterpart of an American word is the British word nearest to it, at
     most two edits away and starting with the same letter: an edit is a letter left out or added,
     two neighbours swapped, or a letter replaced by one of the pairs `z`/`s`, `s`/`c`, `c`/`k`,
     `a`/`e`, `i`/`y`, `a`/`y` (any other replacement counts as two edits); among equally near ones
     the more common by the frequency classes of behaviour 7, then the first in alphabetical
     order. The same holds the other way round. So `color` ↔ `colour`, `center` ↔ `centre`,
     `gray` ↔ `grey`, `organize` ↔ `organise`, `colorizes` ↔ `colourises`, `pediatrician` ↔
     `paediatrician`. A word of one variant without a counterpart (about 120 of the American
     words) is no variant either: `plow` (`plough` is three edits away), and `programme`,
     whose American form `program` both lists hold; such a word stays with the typo check of
     SPEC-0155 as before (with en_US `programme` is a typo, with en_GB `plow` is one). The American
     word and its British counterpart are one *word family*.
   - **When:** with exactly one dictionary in use and that being English (United States) or
     English (United Kingdom), a word of the other variant with a counterpart is reported as the
     other variant (SPEC-0155 v7 behaviour 7) — also when the general inflection rules of
     SPEC-0155 would accept it (`center` as `cent` + `er`, `organize` as `organ` + `ize`) — and the
     mixed spelling of SPEC-0155 v7 applies. With several dictionaries in use at once, or one
     downloaded English (Canada, Australia, South Africa), no word is reported as the other variant:
     both spellings are the user's choice; only mixed spelling applies, to words the dictionaries
     in use know.
   - The table of American and British words is read once per process, when the first text is
     checked or before (the window reads it off its thread at start and when the dictionaries
     change; about 0.1 s in a release build, 1.2 s without optimisation). [OBS-0115 S2, OSS, GEN]

9. **The language of the text (v6).** Text is checked with a dictionary of its own language.
   The values of a resource bundle's file have the language of its locale (SPEC-0163 behaviour
   3); the base file counts as English, as the default text of a program usually is. A file
   whose locale is English (`en`, `en_US`, `en_GB`) is checked with the dictionaries in use; a
   file of another language would be checked with a dictionary of that language when one is
   installed. The catalog holds English dictionaries only, so today the values of `_de`, `_fr`
   and other non-English files are not spell-checked at all, never with an English list: a
   correct `Wiedersehen` or `Seulement` is no typo. Comments of those files stay prose as
   before (SPEC-0155 behaviour 7), with the check of German prose of SPEC-0155. [OBS-0117,
   GEN]

## Acceptance criteria
- **AK-1** A tampered download is refused and nothing kept; the chosen or first installed dictionary
  is used; removing (`dictionaries::tests`); affix expansion (`grazie::tests::applies_hunspell_affixes`).
- **AK-2** MCP: five dictionaries with URL and SHA-256 (`spelling_dictionaries_are_listed`).
- **AK-3** Window self-test: the Natural Languages page opens; the self-test runs without the
  machine's dictionaries.
- **AK-4** With en_US on Ferris IDEA's own sources and specifications (316 files, 2026-11-03): no
  false alarms for common words; British spellings are reported as en_US expects; with en_GB only
  technical terms remain.
- **AK-5** Hand test (round 4, no. 85): download en_GB, switch to en_US, remove one.
- **AK-6 (v2)** The built-in files match the catalog's size and SHA-256; a fresh settings folder
  without any download uses the built-in en_US, and `recieve` in prose is a typo with the suggestion
  `receive`; a downloaded dictionary and the choice still come first; no fetch happens
  (`dictionaries::tests::built_in_en_us_without_a_download`,
  `grazie::tests::built_in_word_list_finds_typos`). MCP: the built-in entry and word list
  (`spelling_dictionaries_are_listed`).

- **AK-7 (v3)** The list has the three classes in order, `message` in class 1 and `menage` in none;
  its licence file lies beside it (`grazie::tests::frequency_classes_of_scowl`).

- **AK-8 (v4)** The built-in en_GB files match the catalog's size and SHA-256 and are never
  downloaded; with en_GB `colour`, `organise`, `behaviour` and `centre` are known and `color`,
  `behavior` and `favor` are typos whose first suggestion is `colour`, `behaviour` and `favour`
  (`center` is not tested: the inflection rules of SPEC-0155 read it as `cent` + `er`);
  with nothing chosen en_US is used and the American forms are known; `recieve` is a typo with the
  suggestion `receive` with both (`dictionaries::tests::built_in_en_gb`,
  `grazie::tests::british_word_list`).
- **AK-9 (v4)** `en_GB,en_US` uses both in that order and both spellings are known; unknown and
  repeated codes are left out; a file of a built-in code in the folder is not read; Space adds and
  takes away a dictionary, the last one stays (`dictionaries::tests::several_dictionaries_at_once`,
  `grazie::tests::two_dictionaries_know_both_spellings`).
- **AK-10 (v4)** MCP: `set_ide_settings` `spelling_dictionary` writes `en_GB,en_US` and refuses an
  unknown or not downloaded code without a change; `spelling_dictionaries` shows `chosen`, `in_use`
  and `word_lists`; `check_grammar` with `dictionary` `en_GB` reports `color` and not `colour`
  (`crates/mcp/tests/it/protocol.rs`: `spelling_language_over_mcp`).
- **AK-11 (v4)** Size and time: the British list gives within 5 % as many words as the American
  one, and reading it and the first suggestions take no longer than for en_US by more than the
  measuring noise (measured in the pull request; `grazie::tests::british_list_is_as_large_as_the_american`).

- **AK-12 (v5)** A change of the dictionaries in Settings re-checks the open editors at once, and a
  change of `editor.spelling_dictionary` in the settings file (as `set_ide_settings` writes it)
  within a second: in a window of the test harness a Markdown file with `colour` and `color` shows
  `color` as the other variant with en_GB and `colour` with en_US, without an edit and without a
  restart (`f081_spelling_variants::switching_the_dictionary_rechecks_open_editors`).
- **AK-13 (v5)** American and British words from the two lists: `color`, `center`, `gray`,
  `organize`, `behavior` are American with the counterparts `colour`, `centre`, `grey`,
  `organise`, `behaviour` and the other way round; `program`, `license`, `practice`, `meter` are
  neither; `programme` and `plow` have no counterpart; the modes for `en_US`, `en_GB`,
  `en_GB,en_US` and `en_AU` (`spelling_variants::tests::american_and_british_words_from_the_lists`,
  `spelling_variants::tests::modes_of_the_dictionaries_in_use`).

- **AK-14 (v6)** Only translations in English have their values checked: the base file and
  `_en_GB` of a bundle yes, `_de` and `_fr` and a lone properties file no
  (`properties::tests::missing_key_names_default_and_is_added_or_refused`); in Inspect Code a
  typo in the base file's value is reported and none in the German values
  (`inspections::tests::missing_bundle_keys_in_inspect_code_with_their_fix`); over MCP no typo in
  French values (`crates/mcp/tests/it/protocol.rs`: `grammar_gaps_over_mcp`).

## Out of scope
v6: dictionaries of other languages for translation values (none in the catalog yet).
v5: variants of the downloadable English dictionaries (Canada, Australia, South Africa) — they are
checked as their lists say, and mixed spelling applies; British "ize" spelling (Oxford) as a
choice of its own; a variant word found only through the general inflection rules (`centers`
where the list lacks it) is not reported as the other variant.
Dictionaries of other languages (the grammar rules are English), a project or user word list and
"Save to dictionary", compound rules, multi-character flags. v2: built-in dictionaries other than
en_US, and the system's Hunspell lists. v4: built-in dictionaries other than en_US and en_GB; the
"ize" British list of SCOWL as a dictionary of its own; the LibreOffice project's en_GB.

## Known deviations from the reference
v2: like the reference, an English dictionary comes with the program, so this deviation of v1 is
gone. The other English variants are downloaded on request, from a fixed source and checked. v4: English
(United Kingdom) needs no download either; several dictionaries can be used at once. v5: with
several dictionaries in use (which the reference does not allow) no word is reported as the other
variant; mixed spelling still is. v6: the values of a non-English translation are not checked
with the English list, as the reference does (OBS-0117, IMP-0535); they wait for a dictionary of
their language.
