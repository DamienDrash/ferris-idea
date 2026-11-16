# SPEC-0131: Islands Light, Sync with OS, file icons and control metrics

- **Version:** 4 · **Date:** 2026-11-13 (v2: Svelte, Astro and web template file icons; v3: Java and
  Kotlin files show the kind of their primary declaration, OBS-0027 L21; v4: switching Sync with OS
  off shows the chosen theme again, the defaults of behaviour 2 as SPEC-0060 v6 set them)
- **Sources:** GEN (the reference offers Islands Dark and Islands Light, follows the system's
  light or dark appearance with "Sync with OS" and a preferred theme for each, shows a file icon
  per language, and draws buttons, fields and popups in fixed New UI sizes; F-087), OSS
  (`platform/platform-resources/src/themes/islands/ManyIslandsLight.theme.json`,
  `themes/expUI/expUI_light_with_light_header.theme.json`, file-type icons under
  `platform/icons/src/expui/fileTypes/`, `python/python-parser/…/expui/`,
  `plugins/kotlin/base/…/expui/`, Jewel Int UI metrics in
  `platform/jewel/int-ui/int-ui-standalone/…/styling/`, all Apache-2.0 at `750ce16`), ADR-0009
- **Feature matrix rows:** F-087

## Purpose
IDECraft looks right in light surroundings too, switches with the system, tells file types apart
at a glance and has controls of the reference's size.

## Behaviour
1. **Islands Light** is a bundled theme (unchanged upstream file over its parents `Light with
   Light Header` and New UI `Light`, editor scheme `Light`), listed after Islands Dark in the theme
   popup and on the Appearance page. [OSS]
2. **Sync with OS** (Appearance page, off by default): the theme follows the system's appearance —
   the preferred dark theme (default IDECraft Dark since SPEC-0060 v6, before that Islands Dark)
   while the system is dark, the preferred light theme (default IDECraft Light, before that Islands
   Light) otherwise — at start-up, when switched on and whenever the system changes, in every
   window. The page shows both preferred themes. A preferred name that is no bundled theme gives
   the default of its kind. [GEN]
3. Choosing a theme while syncing makes it the preferred theme of its kind; which one is shown
   stays the system's choice. The other kind's theme and the theme chosen without syncing stay as
   they are. Without syncing, choosing a theme shows it (SPEC-0060). [GEN]
3a. **v4 – Switching off:** when Sync with OS is switched off, the theme chosen without syncing
   (the saved `theme`) is shown again at once in every window — the theme the next start shows.
   Until v4 the theme of the system's appearance stayed on screen until the restart, which then
   showed another one. The two preferred themes are kept for the next time. Changing another
   setting never changes the theme. [GEN]
4. **File icons:** Python, Kotlin, Swift, Dockerfile, `.gitignore`/`.dockerignore`, Vue, GraphQL,
   HTTP requests, Jupyter, Terraform and Perl files get the reference's open icons; Rust, Go, Ruby,
   PHP and the web component languages Svelte and Astro, whose reference icons are not open, get
   IDECraft's own icons in the same 16 px style (no product logos); HTML template files
   (Handlebars, Mustache, EJS, Nunjucks, Twig, Liquid, Jinja, Pug, ERB) get the HTML icon and MDX
   the Markdown icon, so no known web file shows the unknown icon; plain text keeps the text
   icon. [OSS, GEN, ADR-0009]
5. **Metrics:** dialog buttons are 28 px high, at least 72 px wide, with 12 px side padding and
   4 px corners; one-line text fields 28 px high with 9 px side padding; popups and dialogs keep
   8 px corners (Jewel Int UI values). Used by the connection, breakpoint, rename and settings
   dialogs. [OSS]
6. **MCP:** `themes` lists Islands Light; `ide_settings`/`set_ide_settings` (SPEC-0129) carry the
   `appearance` values. [ADR-0008]
7. **Declaration kinds (v3):** the editor tab, the tab list, the Switcher and the project tree show a
   Java or Kotlin file with the icon of its primary declaration's kind instead of the language's file
   icon: class, interface, enum, record, annotation type, Kotlin object. [OBS-0027 item 11 (L21), GEN]
   - **Java:** the top-level type named like the file, else the first top-level type: `class`,
     `interface`, `enum`, `record`, `@interface`. `package-info.java`, `module-info.java` and a file
     without a type keep the Java file icon.
   - **Kotlin:** the file's only top-level declaration when it is a type: `class` (also `data`, `sealed`,
     `value`), `interface` (also `fun interface`), `enum class` → enum, `annotation class` → annotation
     type, `object`. A file with several top-level declarations, or with any top-level function, property
     or type alias, keeps the Kotlin file icon; so do `.kts` scripts.
   - **Icons:** class, interface and enum are the open node icons already bundled; record, annotation type
     and object are IDECraft's own drawings in the same 16 px badge style (a circle with a stroked `R`,
     `@`, `O`), light and dark, never traced from another product.
   - **Source of the kind:** the top level of the text is scanned without a syntax tree (comments, strings,
     annotations with arguments and everything inside brackets skipped; at most the first 64 KiB, a longer
     Kotlin file keeps the file icon since its top level is not all seen). An open file is read from its
     editor's text and follows every edit, saved or not; another file is read from disk, again only when
     its modification time or length changed and looked at once a second at most. A remote project's files
     that are not open keep the file icon.
   - Other lists that show file icons (search results, problems, the commit window) keep the language's
     file icon. No MCP tool: the icon is a view of the file, and `file_structure` (SPEC-0079) already names
     a file's declarations and their kinds. [ADR-0008]

## Acceptance criteria
- **AK-1** Islands Light resolves over its parents (light, own island values, inherited ones);
  theme names round-trip (`theme::tests`); every icon has both variants and file names map to
  them, Svelte, Astro, Vue and the template languages included (`icons::tests`).
- **AK-2** Window self-test (`theme_steps`): the popup lists four themes; Islands Light draws light
  islands; Sync with OS shows the theme for the system's appearance; a dark theme chosen while
  syncing becomes the dark preference; switching sync off and choosing Islands Dark restores it.
- **AK-3** Checked on screen 2026-11-02: Islands Light with the settings dialog and the new file
  icons.
- **AK-4** Hand test (round 4): switching the macOS appearance with Sync with OS on, icons and
  dialogs next to the reference.
- **AK-5 (v3)** Every Java and Kotlin kind of behaviour 7 from a text, with the near misses: the type named
  like the file before an earlier helper, nested types, text blocks and character literals with braces,
  class literals and keywords in annotation arguments, `package-info.java`, a variable named `record`;
  Kotlin files with several declarations, a function, a property, a type alias, a `::class` reference;
  a long file read only at its start; every kind's icon bundled in both variants (unit tests
  `declaration_kind::tests::{java_primary_declarations, kotlin_primary_declarations,
  icons_are_bundled_and_long_files_are_cut}`, `icons::tests`).
- **AK-6 (v3)** The icon of an open file is computed once per revision and follows its text; a file on
  disk is read again after it changed, not within the same second; other files keep their icon
  (`decl_icon_model::tests::icons_follow_the_text_and_the_file`).
- **AK-7 (v3)** In a window (`crates/workbench/tests/it/jvm_editor_rest.rs::tabs_and_tree_show_the_kind_of_the_declaration`):
  closed files give class, interface, record, annotation and object icons and the Kotlin file icon for a
  file with several declarations; an open interface file edited to an enum, unsaved, gives the enum icon,
  and edited to no type the Java file icon. Not checked on screen: how the three new icons look.
- **AK-8 (v4)** In an application context without a window (`crates/workbench/tests/theme_switch.rs`):
  with Sync with OS the theme of the system's appearance is shown, both appearances have their
  theme, a theme chosen while syncing changes its kind alone and leaves the saved `theme`; the next
  start shows the same; switched off, the chosen theme is back at once and after a restart, and the
  two preferred themes are kept; a preferred name that is no theme gives the kind's default. Window
  self-test (`theme_steps`): switching Sync with OS off shows Islands Light again, the theme chosen
  before. A change of the macOS appearance while the window is open stays a hand test (AK-4).

## Out of scope
High Contrast and Darcula themes, custom themes, accent colours, more own icons (folders, run
configurations), Jewel metrics of trees, tabs and menus.

## Known deviations from the reference
Own icons for Rust, Go, Ruby, PHP, Svelte and Astro (the reference's are not open). Template
languages share the HTML icon instead of one icon each. Not compared side by side.

v3 (OBS-0027 L21): the reference has Kotlin variants of the class, interface and object icons and marks
abstract, final and test classes on the icon; IDECraft shows the plain kind. Own drawings for record,
annotation type and object until open ones are copied.
