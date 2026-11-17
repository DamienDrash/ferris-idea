# SPEC-0060: Themes and editor colour schemes

- **Version:** 12 · **Date:** 2026-11-17 (v12: `themes` follows Sync with OS — names it with the
  preferred themes, switches it, a theme chosen while it is on becomes the preferred theme of its
  kind; behaviour 18, AK-18 — headless check of F-085, OBS-0077); 2026-11-14 (v2: editor font; v3: icons and hover; v4: Islands Dark default;
  v5: switching themes at run time; v6: IDECraft brand themes as default, app icon at run time;
  v7: ⌃` on layouts without a plain backquote, icon of a selected tool window button; hand tests 28, 35;
  v8: coloured icons keep their colours, OBS-0019 item 5; v9: the light root scheme counts last,
  OBS-0023 L24; v10: a colour a theme lacks or spells wrongly falls back for its key alone with a
  note, status and terminal colours read as text in every theme, contrast table over all six themes;
  v11: `Islands Dark` next to the reference, OBS-0047 T1–T3 and T6: frame, islands and selection as
  the open theme defines them, pinned by a test, the toolbar's project colour in SPEC-0061 v14; doc comments and fields in SPEC-0027 v10; static
  fields in the brand schemes)
- **Sources:** OSS (`JetBrains/intellij-community` at `750ce16`: New UI themes `expUI_dark.theme.json` and
  `expUI_light.theme.json`, editor schemes `expUI_darkScheme.xml` and `expUI_lightScheme.xml`, base schemes in
  `DefaultColorSchemesManager.xml`, bundled unchanged as assets; fallback keys of
  `DefaultLanguageHighlighterColors` and the lookup order of `AbstractColorsScheme`/`EditorColorsSchemeImpl`
  as logic templates), ADR-0009; v6: GEN (IDECraft brand identity v1.1.5, `docs/design/DESIGN_SYSTEM.md`
  tokens on the Nord palette, MIT; WCAG 2.2 contrast formula), STD (Apple ICNS, freedesktop
  `StartupWMClass`), ADR-0018; v11: OBS-0047, OSS (`JetBrains/intellij-community` master of
  2026-11-14: `ManyIslandsDark.theme.json`, unchanged since the bundled `750ce16` in the keys below and
  equal in the release tag `idea/262.9437.185`; as logic templates `IslandsUICustomization.kt` and
  `IslandsGradientPainter.kt` (which keys paint the frame and the gaps between islands),
  `RenderingUtil.java` and `JBUI.java` (`CurrentTheme.Tree.Selection`: when a selection counts as
  focused); no code taken)
- **Feature matrix rows:** F-085, F-013

## Purpose
The views look like the reference's New UI: the same editor colours, syntax colours and UI colours, read
from the reference's own open theme files instead of hand-picked values.

## Behaviour
1. **Bundled files:** the dark (`Dark`) and light (`Light`) New UI theme and editor scheme, and the base
   schemes `Default` and `Darcula`, unchanged. [OSS]
2. **Schemes:** a scheme has editor colours (`CARET_ROW_COLOR`, `LINE_NUMBERS_COLOR`, …) and text
   attributes (foreground, background, font type bold/italic). Parents are resolved by `parent_scheme`
   (`Dark` → `Darcula` → `Default`, `Light` → `Default`); a colour comes from the first scheme in the chain
   that defines it. [OSS]
3. **Attribute lookup** as in the reference: a key defined anywhere in the chain wins; a key marked with
   `baseAttributes` uses that key; a key nobody defines uses its fallback key (e.g. `DEFAULT_CLASS_NAME` →
   `DEFAULT_IDENTIFIER` → `TEXT`). [OSS]
   (v9) The root scheme of the chain (`Default`, the light base of every bundled scheme) counts last: a
   key only it defines gives way to the key's fallback when a more specific scheme defines that, so
   Java fields in a dark scheme take its `DEFAULT_INSTANCE_FIELD`, not the light `660e7a`; with the
   root alone (no fallback elsewhere) its own value stays. [OSS; OBS-0023 L24: matches on field names
   were unreadable]
4. **Syntax captures:** tree-sitter capture names map to the reference's default highlighter keys (e.g.
   `keyword` → `DEFAULT_KEYWORD`, `string.escape` → `DEFAULT_VALID_STRING_ESCAPE`, `comment.documentation`
   → `DEFAULT_DOC_COMMENT`, `property` → `DEFAULT_INSTANCE_FIELD`, `type` → `DEFAULT_CLASS_NAME`); unknown
   names use `TEXT`. Bold and italic font types are drawn. [OSS, GEN]
5. **UI theme:** nested `ui` entries are flattened to dotted keys; a value is a `#RRGGBB`/`#RRGGBBAA`
   colour or a palette name from `colors`; a key not set uses the `*` default for its last segment (e.g.
   `Tree.selectionBackground` → `*.selectionBackground`). [OSS]
6. The editor view and the workbench take every colour from the chosen theme; until v5 the default
   was `Islands Dark` (`ManyIslandsDark.theme.json`
   with parent `ExperimentalDark`, the New UI dark theme, and the editor scheme `Islands Dark` with parent
   `Darcula`), the reference's default (OBS-0007). A theme's `parentTheme` is resolved: the child's values
   and palette win, the rest is inherited. The MCP counterpart is `highlight_file` (SPEC-0027), whose capture names map as in
   behaviour 4. [ADR-0008]
7. **Editor font:** JetBrains Mono 2.304 (SIL OFL 1.1, unchanged, licence shipped) is bundled and
   registered at start-up; the editor draws it at 13 px with line spacing 1.2 (16 px lines), the
   reference's defaults. If registration fails, a system monospace font is used. [OSS, GEN]
8. **Icons:** New UI icons of open areas are bundled unchanged in light and dark variants (folder,
   chevrons, close, file types). The tree shows chevrons, folder and file-type icons in rows of
   `Tree.rowHeight` (24 px); tabs show the file-type icon and a small close icon. File types whose icons
   are not open (e.g. Rust, Python, Go) get the plain text icon; unknown types the unknown-file icon.
   SVGs are rasterised at twice their size so they stay sharp on 2× displays. [OSS]
   (v8) Coloured icons keep the colours of their files: the toolkit's SVG images come out with red and
   blue swapped (its raster images are converted to the screen's byte order, SVG images are not), so
   IDECraft swaps red and blue in the icon's colours before rasterising; the amber warning, red
   error and blue information icons look as in the reference (OBS-0019 item 5). [OBS, GEN]
9. **Hover:** tree rows under the pointer get `Tree.hoverBackground` (else `*.hoverBackground`); tabs
   that are not selected are drawn with `EditorTabs.unselectedAlpha` and fully opaque under the pointer;
   the close icon switches to its hovered variant. [OSS, GEN]

10. **Switching themes (v5):** Quick Switch Scheme (`QuickChangeScheme`, ⌃` in both keymaps) and the
    action `ChangeLaf` (Search Everywhere) open a popup listing the bundled themes (behaviour 12), the
    chosen one marked; Up/Down/Enter, a click or the row's digit chooses, Escape closes. Choosing
    redraws every window with the theme and its editor scheme, icons in the theme's variant, and
    saves the name to `theme` in the user's settings folder; the next start uses it, an unknown name
    gives the default (v5: `Islands Dark`; v6: `IDECraft Dark`, behaviour 12). The MCP tool `themes` lists the themes and the choice and with `set` saves a
    choice (ADR-0008); a running window picks it up when it is chosen there or at its next start.
    [GEN, ADR-0008]
    (v7) On keyboard layouts that cannot type `` ` `` without a dead key or ⌥ (German, Austrian,
    Swiss), ⌃` also works on the key macOS moves it to (German: ⌃<) and on the key in its place left
    of 1 (the dead key `^`), through the layout key equivalents of SPEC-0071 behaviour 6. Find
    Action lists the actions as "Quick Switch Scheme…" and "Theme…" (SPEC-0031 v2). [GEN, IMP-0283]
    (v7) The icon of a selected tool window button uses the icon variant that contrasts with the
    button's selected background (light strokes on the blue of the light themes). [GEN]

11. **Brand themes (v6, ADR-0018):** two own themes are bundled, `IDECraft Dark` (parent theme
    `Islands Dark`, editor scheme `IDECraft Dark` with parent scheme `Islands Dark`) and
    `IDECraft Light` (parent theme `Islands Light`, editor scheme `IDECraft Light` with parent
    scheme `Light`). They override colours only — the semantic palette names of their parents
    (`layer-*`, `text-*`, `accent-*`, `selection-*`, `tab-*`, `toolbar-*`, …) and a few editor keys
    (`TEXT`, caret row, selection, line numbers, syntax keys, error and warning effects) — so every
    size, inset, arc and metric is the parent's. Colours come from the brand tokens:

    | Use | `IDECraft Dark` | `IDECraft Light` |
    |---|---|---|
    | Islands, editor, tool windows, dialogs | `background-primary` `#2E3440` | `background-light` `#ECEFF4` |
    | Window behind the islands | `background-secondary` `#3B4252` | `border-default-light` `#D8DEE9` |
    | Popups | `#3B4252` | `#ECEFF4` |
    | UI and editor text | `foreground-secondary` `#D8DEE9` | `foreground-primary-light` `#2E3440` |
    | Muted text (status bar, hints) | `#ABB5C6` (derived) | `foreground-secondary-light` `#4C566A` |
    | Accent: primary buttons, checked boxes, focus, selected tab border, project avatar | `accent-primary` `#88C0D0`, text on it `#2E3440` | `accent-primary-light` `#4C6A91`, text on it `#ECEFF4` |
    | Links | `#88C0D0` | `#4C6A91` |
    | Tree/list selection | `#3E5871` (Frost, darkened) | `#C9D6E6` (Frost tint) |
    | Selected tab | `#3A4A63` | `#DCE4EE` |
    | Error, warning, success | `#DE8C93`, `#EBCB8B`, `#A3BE8C` | `#A3434C`, `#7A5C14`, `#4F6E3A` |
    | Selected tool window button | Frost 3 `#5E81AC` | `#C9D6E6` |

    Syntax colours follow Nord's roles (keywords Frost `#81A1C1`/`#4C6A91`, strings Aurora green,
    functions Frost `#88C0D0`/`#3B6E7E`, types `#8FBCBB`/`#2F6B69`, numbers and constants purple,
    comments grey), each lightened or darkened until it reaches 4.5 : 1 on the editor background.
    [GEN, ADR-0018]
12. **Default and kept choices (v6):** the popup and Settings › Appearance list `IDECraft Dark`,
    `IDECraft Light`, `Islands Dark`, `Islands Light`, `Dark`, `Light` in this order. Without a
    saved `theme` file, or with a name that is not bundled, the views draw with `IDECraft Dark`; a
    saved name is always used, so a user who chose `Islands Dark` keeps it. With Sync with OS
    (SPEC-0131) the defaults are `IDECraft Dark` and `IDECraft Light`; saved names are kept. The
    MCP tool `themes` lists every theme with `dark` and `brand`, the `chosen` one and the
    `default`, and `set` accepts any of the six names in any case. [GEN, ADR-0008]
13. **Accent in the views (v6):** the project avatar (initials square in the toolbar and in the
    recent-projects popup), primary dialog buttons, the commit button and checked boxes of the
    commit window, and Revert in Local History draw with the theme's `Button.default.startBackground`
    and `Button.default.foreground`; a focused or switched-on field border uses `*.focusColor`;
    error and success text use `Label.errorForeground` and `Label.successForeground`. No view
    draws a fixed IntelliJ blue, red or green any more. [GEN]
14. **App icon at run time (v6):** the binary embeds `packaging/icons/macos/IDECraft.icns` (brand
    package v1.1.5) and, on macOS, sets it as the application icon right after start-up, so a
    bare `idecraft ui` shows the brand icon in the Dock and the app switcher. The embedded
    `Info.plist` names `IDECraft` as `CFBundleIconFile` for an app bundle. On Linux every window
    has the application id `idecraft`, the `StartupWMClass` of `idecraft.desktop`, so a desktop
    with the installed icon shows it; GPUI 0.2 offers no window icon call. Windows has no desktop
    UI yet. [STD, ADR-0018]

15. **A colour that is missing or malformed (v10):** a theme never leaves a view without a colour
    and never fails to draw.
    - A value written as a colour that is none (`#12345`, `#zzzzzz`) in a theme file, in its
      `colors` palette or in an editor scheme is left out for its key alone: the parent theme's or
      parent scheme's value counts, else the `*` default, and everything else in the file is used.
      An empty colour value in a scheme keeps its meaning "no colour here".
    - A colour the views need and no file gives — the key is not set, or it names a palette colour
      nobody defines — is derived from the theme's own colours: backgrounds from the island
      colour moved towards the text (hover, inactive selection, borders) or towards the accent
      (selection), popups and tooltips from the island, links from the accent, diff rows from the
      editor background moved towards green, grey, blue and red, error, warning and success from a
      red, amber and green of the theme's kind. A light theme therefore never gets a dark default.
      Until v10 every such default was one fixed colour of the dark theme.
    - Each such case is a note naming the theme or scheme, the key, the bad value and what is
      used instead. The first time a theme is shown in a run of the application its notes go to
      the error output and, one line each with the time in front, to `theme.log` in the log folder
      (`~/Library/Logs/IDECraft` on macOS). Malformed JSON or XML and an unknown parent scheme stay
      errors (AK-4); they can only occur in a bundled file and are caught by the unit tests.
    - The bundled files have no malformed value. The reference's classic `Dark` and `Light` do not
      set `Label.successForeground`; that is their one note each. [GEN]
16. **Colours that are read (v10):** some colours of a theme are made for marks and are also drawn
    as text, on backgrounds their theme did not have in mind. Where such a colour does not reach
    the contrast below on its background, it is moved towards the plain text colour of that
    background in steps of 1/16 until it does; a colour that reaches it is drawn unchanged. The
    theme files are not touched.
    - **Status text** (error, warning, info and success messages in dialogs and tool windows):
      4.5 : 1 on islands and on popups. Warning and info take the scheme's effect colour of
      `WARNING_ATTRIBUTES` and `INFO_ATTRIBUTES`, else its stripe colour. Labels in the error and
      success colours (`Label.errorForeground`, `Label.successForeground`): 4.5 : 1 on islands.
    - **Problem marks** (waves, the ● and ▲ of the inspection widget, underlines of file names):
      3 : 1 on the editor and on islands, from the effect colour, else the stripe colour, else the
      default of the theme's kind. The light schemes of the reference set no effect colour for
      warnings; until v10 the dark amber `f2c55c` was drawn on white there (1.6 : 1).
    - **Terminal colours:** text in one of the 16 terminal colours on the terminal's own
      background reaches 4.5 : 1; the same colours as a cell's background, text on a coloured
      background, the 256-colour cube and direct colours are drawn as the program sent them. Views
      that write in a terminal colour (failed rows, added and removed lines) use the readable one.
      [GEN, IMP-0402]
    - **Diff rows** of the diff, merge, rebase, schema comparison and local changes views take the
      scheme's `DIFF_INSERTED`, `DIFF_DELETED`, `DIFF_MODIFIED` and `DIFF_CONFLICT` from one place.
    [GEN]

17. **`Islands Dark` as its open file defines it (v11, OBS-0047):** the colours below are read from
    `ManyIslandsDark.theme.json` and `IslandSchemeDark.xml` and pinned by a test, so a change in a
    bundled file or in the lookup shows at once. [OSS]

    | Surface | Key | Palette name | Colour |
    |---|---|---|---|
    | Window behind the islands, the gaps between them, main toolbar (also inactive), the two stripes, status bar | `MainWindow.background`, `MainToolbar.background`, `MainToolbar.inactiveBackground`, `ToolWindow.Stripe.background`, `StatusBar.background` | `main-window-bg` = `layer-1-bg` = `gray-30` | `#26282C` |
    | Tool window islands | `ToolWindow.background` | `tool-window-bg` = `layer-0-bg` = `gray-10` | `#191A1C` |
    | Editor island | scheme `TEXT` background | `editor-bg` = `layer-0-bg` | `#191A1C` |
    | Selected row of a tree or list that has the keyboard focus | `Tree.selectionBackground`, `List.selectionBackground` (via `*.selectionBackground`) | `selection-bg-active` = `blue-50` | `#2A4371` |
    | The same while the focus is elsewhere | `Tree.selectionInactiveBackground`, `List.selectionInactiveBackground` | `selection-bg-inactive` = `gray-40` | `#33353B` |

    - **Frame and islands (T1):** the open theme makes the frame one step lighter than the islands:
      `gray-30` around `gray-10` islands. The reference's own frame code paints the frame, the
      toolbar, the stripes, the status bar and the gaps between islands with `MainWindow.background`;
      the variant with lighter tool windows (`alt.*` keys, `alt.ToolWindow.background` = `gray-20` on
      a `gray-40` frame) applies only with the reference's option for a different tool window
      background, which is off in OBS-0047 and which IDECraft does not offer. IDECraft drew exactly
      these values before v11 and keeps them; the coordinator's re-check of OBS-0047 at full
      resolution (2026-11-14, 14:22) found frame, stripes and islands dark in both windows. The one
      difference was the main toolbar: IDECraft tinted it with the bright accent, the reference with
      the theme's muted project colour; that is SPEC-0061 v14 behaviour 28a.
    - **Selected rows (T6):** as in the reference, a selection counts as focused only while its own
      tree or list has the keyboard focus. The Project tree draws `#2A4371` then and `#33353B`
      otherwise (OBS-0007 item 6). The grey row OBS-0047 saw in IDECraft is what this rule gives
      while the tree has no focus; where the focus was in either window is not recorded, so the
      comparison is repeated with the focus in the tree in both (PO-STATUS question 39). Most other
      trees and lists draw the focused colour whatever has the focus (unchanged in v11; the
      reference's behaviour there not compared).
    - **Fields and doc comments (T2, T3):** the open scheme's `DEFAULT_DOC_COMMENT` (`#5F826B`,
      italic), `DEFAULT_INSTANCE_FIELD` (`#C77DBB`) and `DEFAULT_STATIC_FIELD` (`#C77DBB`, italic)
      reach the editor through the syntax roles of SPEC-0027 v10 and the server's tokens of SPEC-0290.
      The brand schemes define `DEFAULT_STATIC_FIELD` themselves (their field colour, italic) instead
      of inheriting the purple of `Islands Dark`, so `IDECraft Dark` and `IDECraft Light` show static
      fields in their own field colour. [GEN]
    - The other themes are not changed: `IDECraft Dark` (`#3B4252` around `#2E3440`, selection
      `#3E5871`), `IDECraft Light`, `Islands Light`, `Dark` and `Light` keep their frame, island and
      selection colours; the same test pins them.

18. **`themes` and Sync with OS (v12):** the MCP tool answers like the window does with Sync
    with OS (SPEC-0131 behaviours 2, 3 and 3a). It returns `sync_with_os`, the preferred
    `dark_theme` and `light_theme` (a name that is no bundled theme gives the default of its kind,
    `IDECraft Dark` or `IDECraft Light`) and `shown`, the theme the windows draw, which it names only
    while Sync with OS is off (the system's appearance decides otherwise). `sync_with_os: true|false`
    switches it, before a `set` of the same call. While it is on, `set` makes the theme the
    preferred theme of its kind, as the theme popup does, and leaves `chosen` (the theme chosen
    without syncing) as it is; `saved_as` says where the choice went (`theme`, `dark_theme`,
    `light_theme`). Both changes need the `settings` right. Until v12 `themes` named only
    `chosen`, which with Sync with OS on is not what is shown, and `set` wrote `chosen` there, so an
    agent's choice changed nothing on screen until Sync with OS was switched off; an agent could
    not switch Sync with OS at all. [GEN, ADR-0008]

**Contrast, all bundled themes (v10; WCAG 2.2, AA = 4.5 : 1 for text, 3 : 1 for marks; checked
2026-11-13 by `palette::tests::contrast_table`, 70 pairs per theme):** UI text and muted text on
islands, the window and popups; links and the four status texts on islands and popups; error and
success labels; text on the accent; UI text on the tree selection, the inactive selection, hovered
rows, the selected tab, hovered tabs and toolbar buttons; tooltip text and shortcut; editor text on
the background, the caret row, a selection and the four diff rows; eleven syntax colours; console
text; the 16 terminal colours; the four problem marks on the editor and on islands.

| Theme | Pairs below the limit |
|---|---|
| `IDECraft Dark` | none (v10: the tooltip's shortcut text was 4.17 and is `#B8C1D0` now) |
| `IDECraft Light` | none |
| `Islands Dark` | editor text on a changed diff row 4.18; comments 4.27; doc comments 4.06 |
| `Islands Light` | comments and doc comments 3.36; annotations 3.51 |
| `Dark` | text on the accent 4.28; tooltip shortcut 4.28; editor text on a changed diff row 4.18; comments 4.04; doc comments 3.84 |
| `Light` | text on the accent 4.28; comments and doc comments 3.36; annotations 3.51 |

The limit binds the brand themes, which are IDECraft's own. The four themes of the reference are
bundled unchanged (behaviour 1), so the pairs their files leave below the limit stay and are
listed; the test fails when one gets worse or a new one appears. Before v10 the terminal colours
were below the limit in every theme (black on the dark backgrounds 1.2 to 1.7, bright black 1.8 to
2.5, bright yellow and bright white on the light ones 1.4 to 1.7; in `IDECraft Dark` ten of
sixteen).

**Contrast of the brand themes' own colours (WCAG 2.2, checked 2026-11-04 by `theme::tests::brand_text_meets_wcag_aa`, AA = 4.5 : 1):**

| Pair | `IDECraft Dark` | `IDECraft Light` |
|---|---|---|
| UI text on islands / window / popups | 9.25 / 7.45 / 7.45 | 10.84 / 9.25 / 10.84 |
| Muted text on islands / window / popups | 6.04 / 4.87 / 4.87 | 6.40 / 5.46 / 6.40 |
| Links on islands | 6.24 | 4.82 |
| Text on the accent (primary button, avatar) | 6.24 | 4.82 |
| Text on tree selection / selected tab / inactive selection | 5.48 / 6.64 / 6.39 | 8.47 / 9.74 / 9.25 |
| Editor text / on caret row / in a selection | 9.25 / 7.45 / 5.46 | 10.84 / 10.26 / 8.47 |
| Keywords, strings, numbers, comments | 4.64, 6.13, 5.49, 4.69 | 4.82, 5.03, 4.89, 4.86 |
| Functions, types, annotations, escapes | 6.24, 5.99, 5.30, 8.00 | 4.90, 5.32, 5.63, 5.41 |
| Error text on islands | 4.92 | 5.26 |

Below AA, by design and not text content: line numbers `#7B88A1` (3.50 dark, 3.10 light; the
reference's are about 2.2), icons on the selected tool window button (Frost 3, about 3 : 1),
error text on the dark window background (3.97, used on islands only).

## Acceptance criteria
- **AK-1** `Dark` resolves through `Darcula` and `Default`: caret row, line numbers, text background,
  keyword colour; `baseAttributes` and code fallbacks; a key defined in a parent wins over the fallback.
  (v9) A key only the root defines yields to a fallback of a specific scheme (`INSTANCE_FIELD_ATTRIBUTES`).
- **AK-2** `Light` resolves through `Default`, including the selection colour; `Dark` inherits one.
- **AK-3** The dark theme resolves palette names, alpha values and `*` defaults; unknown keys give none.
- **AK-4** Malformed JSON and an unknown parent scheme are errors.
- **AK-6** The bundled font files are TrueType and the line height is 16 px (unit test); the window tests
  and the latency measurement pass with the font.
- **AK-7** Every bundled icon has both variants and every file name maps to a bundled icon (unit test);
  the window self-test passes with icons.
- **AK-7b (v8)** Swapping the colours of an icon swaps red and blue of every `#rrggbb` colour and
  leaves other text unchanged (unit test `icon_images::tests::svg_colours_are_swapped_for_the_toolkit`);
  hand check of the Problems tool window next to the reference.
- **AK-8** `Islands Dark` inherits from the New UI dark theme, resolves its multi-level palette names
  and its scheme chain `Islands Dark` → `Darcula` → `Default` (unit test).
- **AK-9** Theme names round-trip and the light palette is light (unit tests); a saved choice is read
  back and an empty file means none (unit test); `themes` lists, sets and rejects unknown names
  (protocol test); the window self-test opens the popup with ⌃`, switches to Light (palette and saved
  file), closes with Escape without switching and switches back. A side-by-side look at the light
  theme is a hand test (round 4).
- **AK-10 (v6)** Every bundled theme defines the editor colours and text attributes the views read,
  the brand themes every UI colour too; the brand themes take the brand tokens and inherit the
  Islands sizes and insets; their scheme chains are `IDECraft Dark` → `Islands Dark` → `Darcula`
  → `Default` and `IDECraft Light` → `Light` → `Default` (unit tests). The text pairs of behaviour
  11 reach 4.5 : 1 (unit test).
- **AK-11 (v6)** Without a saved choice the default is `IDECraft Dark`, a saved name of any bundled
  theme is kept, an unknown one gives the default (unit tests in `idecraft-ui`, `idecraft-core`
  and `idecraft-workbench`); `themes` reports `IDECraft Dark` as chosen and default for a new
  install, lists six themes and keeps `Islands Dark` once set (protocol and functional test); the
  window self-test opens the popup with `IDECraft Dark` marked, switches to `Light`, to `Islands
  Dark` and back.
- **AK-12 (v6)** The palette's accent, accent text, link, focus, error and success colours come from
  the theme (unit test); the views listed in behaviour 13 use them.
- **AK-13 (v6)** The ICNS bytes are embedded and well-formed, the scalable icon is embedded, and the
  application id matches `idecraft.desktop` (unit test); the Dock icon of a bare binary and the
  theme switch are checked by hand (hand test 192).
- **AK-5** The views' palette comes from the bundled files (unit test), and typing latency stays below
  16 ms p95 with styled runs (`idecraft spike-editor`).
- **AK-14 (v10)** A theme and a scheme with malformed colours keep the parent's value for each such
  key, use everything else and note each case; an empty scheme value still means none; the bundled
  files give no note (`theme::tests::a_malformed_colour_falls_back_per_key`). A light theme that
  lacks most colours gives a palette whose every surface is light and whose every text reads, with
  notes for the malformed, the unknown and the missing keys
  (`palette::tests::a_broken_theme_still_gives_a_whole_palette_of_its_kind`); the bundled themes
  note only the missing success label of `Dark` and `Light`
  (`palette::tests::bundled_themes_note_only_what_their_files_leave_out`). The notes of a shown
  theme are in the theme log once however often it is shown
  (`crates/workbench/tests/theme_switch.rs`).
- **AK-15 (v10)** A colour that does not read is moved towards the text until it does, one that
  reads stays (`theme::tests::colours_are_made_readable`); the table above over all six themes,
  with the brand themes free of pairs below the limit and the reference's themes at their listed
  pairs (`palette::tests::contrast_table`).
- **AK-16 (v10)** Every bundled theme colours the editor, the caret row, the selection, tool windows,
  the window, popups and dialogs, the selections, the terminal and the four diff rows in its own
  kind, takes the diff rows and terminal colours from its own scheme and draws a terminal colour
  unchanged exactly when it reads (`palette::tests::every_theme_colours_every_surface_in_its_kind`).
  Chosen in an application context, every bundled theme is the palette the views draw with at
  once, is saved, and is the theme after a restart; a saved name that is no theme gives the default
  (`crates/workbench/tests/theme_switch.rs`). On screen: hand test 28.

- **AK-17 (v11)** For `Islands Dark` the five frame keys give `#26282C`, the islands and the editor
  `#191A1C`, the focused and unfocused tree and list selections `#2A4371` and `#33353B`, and the
  other five themes keep their frame, island and selection colours
  (`palette::tests::frame_islands_and_selection_come_from_the_open_theme`); doc comments, fields and
  static fields take the scheme's colours in every bundled scheme, `5F826B` italic, `C77DBB` and
  `C77DBB` italic in `Islands Dark`, the brand schemes' static fields their field colour
  (`theme::tests::doc_comments_and_fields_take_the_scheme_colours`). A side-by-side look at the
  tree selection with the focus in the tree is open (PO-STATUS question 39).

- **AK-18 (v12, behaviour 18)** MCP protocol test `themes_follow_sync_with_os`: a new install lists
  Sync with OS off, the brand themes as preferred and `shown` `IDECraft Dark`; `set` without
  syncing is `saved_as` `theme`; switched on with `set` `islands dark` in one call it is
  `dark_theme`, `chosen` stays `Light` and `shown` is null; `islands light` goes to `light_theme`;
  both are in the settings file; a preferred name that is no theme reads as the default of its
  kind; switched off, `shown` is `Light` again and the preferred dark theme is kept. In a window:
  not checked (the theme popup already follows behaviour 3 of SPEC-0131).

## Out of scope
Brand typography (Inter) in the application, a brand splash or about screen, a window icon on
Linux through GPUI, an app bundle or installer (packaging), choosing an editor scheme apart from the theme, the other levels of Quick Switch Scheme (code style,
keymap, view mode), following the system appearance, Islands themes, high
contrast, colour-blind variants (`deuteranopia`/`protanopia` values), language-specific attribute keys
of plugins, own icons for languages whose icons are not open, the UI font (system font, as the
reference on macOS), Jewel metrics.

## Known deviations from the reference
`baseAttributes` names are followed directly; the reference follows the key's code fallback, which for
the bundled keys names the same attribute. Not compared side by side in screenshots yet.
The default theme is IDECraft's own (`IDECraft Dark`), not the reference's `Islands Dark`
(ADR-0018): colours differ by design, layout and sizes do not; side-by-side colour comparisons use
`Islands Dark`. The reference's primary buttons are filled with its accent; IDECraft's were drawn
with the selection colour until v6 and now use the accent too.
In the reference ⌃` does nothing on a German keyboard (project owner, 2026-11-05); IDECraft reaches it
there (IMP-0283).
(v10, IMP-0402) The open schemes' terminal colours are drawn unchanged only where they read on the
terminal's background; the others are moved towards the text colour (behaviour 16). Status text
and problem marks are adjusted the same way. How the reference draws these colours was not
compared; the contrast figures are computed from the open scheme files.
Quick Switch Scheme opens the theme list directly instead of the reference's first-level menu of
scheme kinds, since themes are the only scheme IDECraft switches; the change applies without the
reference's restart prompts for some theme changes.
