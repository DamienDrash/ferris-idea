# SPEC-0159: Dictionaries for the typo check

- **Version:** 2 · **Date:** 2026-11-15 (v2: English (United States) built in, so typos are checked
  from the first start without a download — OBS-0066 G1)
- **Sources:** GEN (IDEs check spelling with a word list per language that the user can choose and
  download; F-081 Grazie), STD (Hunspell `.dic`/`.aff` format: word count, `word/FLAGS`, `PFX`/`SFX`
  rules with strip, affix and condition), OSS (Hunspell dictionaries of the LibreOffice project,
  `github.com/LibreOffice/dictionaries`, folder `en`, commit `4f1df64`; SCOWL (BSD-style) for en_US,
  en_CA, en_AU, LGPL for en_GB, en_ZA), SPEC-0155, ADR-0008; v2: OBS-0066 G1 (the reference checks
  typos from its first start with an English dictionary it brings along; IDECraft found none in a
  fresh settings folder on macOS, which has no system Hunspell list), OSS (`README_en_US.txt` of the
  same commit: the licence terms of SCOWL, VarCon, Ispell, WordNet and UKACD and the public-domain
  parts, all permissive with notices kept)
- **Feature matrix rows:** F-081

## Purpose
Let each user pick the English they write — American, British, Canadian, Australian, South
African — and get a working typo check without installing anything by hand.

## Behaviour
1. **Catalog:** Settings › Editor › Natural Languages lists English (United States), (United
   Kingdom), (Canada), (Australia), (South Africa) with size, licence and state (size to download,
   downloaded, used). v2: English (United States) is marked as built in. [GEN, OSS]
2. **Download:** Enter (or a click) downloads the selected dictionary's `.dic` and `.aff` from the
   LibreOffice project at the fixed commit, checks both against the size and SHA-256 IDECraft knows,
   and keeps them in `dictionaries` of the user's settings folder (written through temporary files;
   a failed check keeps nothing). It is then used. Delete removes it. Downloading is the user's
   action only; no file is downloaded without it. v2: Enter on the built-in dictionary chooses it
   without a download; Delete on it removes nothing and says it is built in. [GEN]
3. **Choice (v2):** the chosen dictionary is saved in the settings (`editor.spelling_dictionary`) and
   used for typos at once in every window. The dictionary used is the chosen one when it is
   downloaded or built in, else the first downloaded one in catalog order, else the built-in one.
   No English Hunspell list of the system is read any more: the check is the same on every
   machine. [GEN]
4. **Reading:** prefixes and suffixes of the `.aff` file are applied to the word list (one-character
   flags, conditions with character sets), so forms such as `input` (`in` + `put`) or `cities` are
   known. [STD]
5. **MCP:** `spelling_dictionaries` lists the catalog with URL, SHA-256 and state, the folder, the
   dictionary used and the word list file in effect; read only. `check_grammar` and `inspect_code`
   use the same dictionary. v2: each entry says whether it is built in, and the answer says whether
   the built-in word list is in effect (`word_list` is then empty). [ADR-0008]
6. **Built in (v2):** IDECraft carries English (United States) inside the program: `en_US.dic` and
   `en_US.aff` of the same commit as the catalog, unchanged, kept in the repository beside their
   licence text (`README_en_US.txt`, unchanged). The program checks both against the catalog's size
   and SHA-256 before it reads them; a mismatch means no built-in word list, never a changed one.
   Nothing is fetched from the network at start or for the built-in dictionary. The licence text
   goes with the program and is named in the repository's notices. [OSS, GEN]

## Acceptance criteria
- **AK-1** A tampered download is refused and nothing kept; the chosen or first installed dictionary
  is used; removing (`dictionaries::tests`); affix expansion (`grazie::tests::applies_hunspell_affixes`).
- **AK-2** MCP: five dictionaries with URL and SHA-256 (`spelling_dictionaries_are_listed`).
- **AK-3** Window self-test: the Natural Languages page opens; the self-test runs without the
  machine's dictionaries.
- **AK-4** With en_US on IDECraft's own sources and specifications (316 files, 2026-11-03): no
  false alarms for common words; British spellings are reported as en_US expects; with en_GB only
  technical terms remain.
- **AK-5** Hand test (round 4, no. 85): download en_GB, switch to en_US, remove one.
- **AK-6 (v2)** The built-in files match the catalog's size and SHA-256; a fresh settings folder
  without any download uses the built-in en_US, and `recieve` in prose is a typo with the suggestion
  `receive`; a downloaded dictionary and the choice still come first; no fetch happens
  (`dictionaries::tests::built_in_en_us_without_a_download`,
  `grazie::tests::built_in_word_list_finds_typos`). MCP: the built-in entry and word list
  (`spelling_dictionaries_are_listed`).

## Out of scope
Dictionaries of other languages (the grammar rules are English), a project or user word list and
"Save to dictionary", compound rules, multi-character flags. v2: built-in dictionaries other than
en_US, and the system's Hunspell lists.

## Known deviations from the reference
v2: like the reference, an English dictionary comes with the program, so this deviation of v1 is
gone. The other English variants are downloaded on request, from a fixed source and checked.
