# SPEC-0212: Resource bundle editor

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (IDEs with the open Properties plugin edit a resource bundle in one view: the keys
  of all its files in a list, and for the selected key one value per locale; keys missing in a
  locale are marked; F-081), STD (`.properties` format of `java.util.Properties`, file naming of
  `java.util.ResourceBundle`), SPEC-0155, SPEC-0163, ADR-0008
- **Feature matrix rows:** F-081

## Purpose
Translate and maintain a resource bundle in one place instead of opening each locale's file.

## Behaviour
1. **Opening:** in a `.properties` file of a bundle (SPEC-0163 behaviour 3), Search Everywhere
   "Open Resource Bundle Editor" (also in the Code menu) opens the dialog "Resource Bundle
   '<base>'". [GEN]
2. **Keys:** the left list holds every key of the bundle's files, in the order of the default file
   and then the keys only other files have; a key missing in some locale is marked with ⚠ and the
   missing locales. ↑/↓ choose a key; typing in the filter field above the list keeps the keys that
   contain the text. [GEN]
3. **Values:** for the selected key, one field per locale (the default file first as "default"),
   holding the value with escapes resolved; an empty field of a missing key says "missing". [GEN]
4. **Saving:** Enter, choosing another key or closing the dialog writes each changed value into its
   file: an existing property is changed on its own line(s) only, keeping its key, separator and
   the rest of the file; a new one is appended at the end. The value is escaped so that `java.util.Properties.load`
   reads it back unchanged (backslashes, line breaks, tabs, a leading space or `#`/`!`), and
   non-ASCII characters are written as `\uXXXX` only when the file has no non-ASCII text yet. An
   open editor of that file is updated as one undo step instead of the file on disk.
   [STD, GEN, IMP-0091]
5. **Adding and removing:** ⌘N asks for a new key (refused if it exists) and adds it with an empty
   value to the default file; Delete on the key list asks, a second Delete removes the key from
   every file of the bundle. Escape closes the dialog. [GEN]
6. **MCP:** `resource_bundle` (SPEC-0163) gets `set` (`key`, `locale`, `value`) and `remove` (`key`)
   that write the same way and return the bundle afterwards. [ADR-0008]

## Acceptance criteria
- **AK-1** Model: keys in order with missing locales, a value set in place keeping the rest of the
  file, continuation lines replaced, a new key appended, escaping with and without non-ASCII text,
  a key removed from every file (`properties::tests::bundle_editor_model`).
- **AK-2** MCP: `resource_bundle` with `set` and `remove` (`project_dictionary_and_resource_bundles`).
- **AK-3** Window self-test (`bundle_editor_steps`): the dialog lists the keys with the missing one
  marked, Enter in the German field writes `messages_de.properties`, ⌘N adds a key, Delete twice
  removes it.
- **AK-4** Hand test (round 4, no. 152): edit a bundle of three locales in the dialog and compare
  with the reference's bundle editor.

## Out of scope
Renaming a key across the bundle and its usages, adding a locale, sorting, the reference's
"unused property" inspection, XML property files.

## Known deviations from the reference
Only the edited property's lines change and the file's convention for non-ASCII text is kept, so a
translation does not rewrite or re-escape the whole file (IMP-0091).
