# SPEC-0163: Project dictionary and resource bundles

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** OSS (intellij-community spell checker: project dictionary in
  `.idea/dictionaries/<name>.xml`, component `ProjectDictionaryState`, `<w>` elements; Properties
  plugin: resource bundles named `base_language_COUNTRY_variant.properties`), STD (file naming of
  `java.util.ResourceBundle`), SPEC-0155, SPEC-0159, ADR-0008
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
3. **Resource bundles:** `.properties` files of one folder with the same base name form a bundle;
   the locale is read from the end of the name (language, optional country, optional variant). A
   key of one file that another file of the bundle lacks is reported at the key, naming the locales
   ("default" for the base file). [OSS, STD]
4. **MCP:** `project_dictionary` lists the words and adds words (dry run by default);
   `resource_bundle` lists a bundle's files with locale, every key with its locales, and the keys
   missing per file. [ADR-0008]

## Acceptance criteria
- **AK-1** Reading, writing (escapes), the project folder, filtering
  (`project_dictionary::tests::reads_writes_and_filters`).
- **AK-2** Bundle names and missing keys (`properties::tests::bundles_and_their_missing_keys`).
- **AK-3** MCP (`project_dictionary_and_resource_bundles`).
- **AK-4** Window self-test: saving a word writes the dictionary file.
- **AK-5** Hand test (round 4, no. 89).

## Out of scope
A bundle editor showing all locales side by side, user-level and other named dictionaries,
removing words in the UI (edit the file).
