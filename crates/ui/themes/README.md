# Bundled themes and editor colour schemes

These files are unchanged copies from `JetBrains/intellij-community`, licensed under the Apache License 2.0
(see the repository's `NOTICE` and `LICENSES/Apache-2.0.txt`):

| File | Source path | Commit |
|---|---|---|
| `expUI_dark.theme.json` | `platform/platform-resources/src/themes/expUI/expUI_dark.theme.json` | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |
| `expUI_light.theme.json` | `platform/platform-resources/src/themes/expUI/expUI_light.theme.json` | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |
| `expUI_darkScheme.xml` | `platform/platform-resources/src/themes/expUI/expUI_darkScheme.xml` | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |
| `expUI_lightScheme.xml` | `platform/platform-resources/src/themes/expUI/expUI_lightScheme.xml` | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |
| `ManyIslandsDark.theme.json` | `platform/platform-resources/src/themes/islands/ManyIslandsDark.theme.json` | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |
| `IslandSchemeDark.xml` | `platform/platform-resources/src/themes/islands/IslandSchemeDark.xml` | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |
| `PythonDarcula.xml` | `python/python-code-insight/resources/colorSchemes/PythonDarcula.xml` (added to `Darcula`) | `d4018fb57c367e7b5d895bd8a0c84c382efd0e17` |
| `PythonDefault.xml` | `python/python-code-insight/resources/colorSchemes/PythonDefault.xml` (added to `Default`) | `d4018fb57c367e7b5d895bd8a0c84c382efd0e17` |
| `DefaultColorSchemesManager.xml` | `platform/platform-resources/src/DefaultColorSchemesManager.xml` (parent schemes `Default` and `Darcula`) | `750ce16ff6661b1210ea1bbda65c847f753eeb4b` |

They are read by `crates/ui/src/theme.rs` (own code, SPEC-0060). Do not edit them; update by copying a newer
upstream revision and recording the commit here and in `docs/legal/PROVENANCE.md`.

## Own files (IDECraft brand themes)

| File | Parent | Licence |
|---|---|---|
| `IDECraftDark.theme.json` | `Islands Dark` (`ManyIslandsDark.theme.json`) | the project's licence, `LICENSE.md` (own work) |
| `IDECraftDarkScheme.xml` | editor scheme `Islands Dark` | the project's licence, `LICENSE.md` (own work) |
| `IDECraftLight.theme.json` | `Islands Light` (`ManyIslandsLight.theme.json`) | the project's licence, `LICENSE.md` (own work) |
| `IDECraftLightScheme.xml` | editor scheme `Light` | the project's licence, `LICENSE.md` (own work) |
| `IslandsLightScheme.xml` | editor scheme `Light` | the project's licence, `LICENSE.md` (own work) |

They override only colours: the semantic palette names of the Islands themes and a few editor
scheme keys take the IDECraft brand tokens (`docs/design/DESIGN_SYSTEM.md`, Nord palette by Arctic
Ice Studio, MIT). Layout, sizes and every other value are inherited (ADR-0018, SPEC-0060 v6). These
files may be edited; contrast is checked by `theme::tests::brand_text_meets_wcag_aa`.

`IslandsLightScheme.xml` is the editor scheme of the reference's `Islands Light` theme as IDECraft
draws it: `Light` with function and method declarations in the text colour, as the reference
shows them (SPEC-0060 v13 behaviour 19, OBS-0079 L1).
