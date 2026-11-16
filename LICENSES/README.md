# Licences of third-party parts

IDECraft's own code and documentation are under the PolyForm Shield License 1.0.0, see
[`../LICENSE.md`](../LICENSE.md). The texts in this folder are **not** licences of the project as a
whole. They belong to third-party material that is kept in this repository and keeps its own
licence.

| File | Licence | Covers |
|---|---|---|
| [`Apache-2.0.txt`](Apache-2.0.txt) | Apache License 2.0 | Material from `JetBrains/intellij-community`, listed entry by entry in [`docs/legal/PROVENANCE.md`](../docs/legal/PROVENANCE.md): the bundled keymaps in `crates/ui/keymaps/`, the upstream themes and colour schemes in `crates/ui/themes/`, and the New-UI icons in `crates/ui/icons/` except the folder `idecraft/`. The folder READMEs name the upstream paths and commits. |
| [`OFL-1.1.txt`](OFL-1.1.txt) | SIL Open Font License 1.1 | JetBrains Mono in `crates/workbench/fonts/` (the same text lies beside the font files as `OFL.txt`). |
| [`../crates/languages/dictionaries/README_en_US.txt`](../crates/languages/dictionaries/README_en_US.txt) | SCOWL and the notices of its sources: permission notices of Kevin Atkinson and Benjamin Titze, Ispell's BSD-style licence, the WordNet and UKACD terms; other sources in the public domain | The built-in English (United States) Hunspell dictionary in `crates/languages/dictionaries/` (the text lies beside the files, unchanged, as the licence asks). |

Own files beside that material (`crates/ui/icons/idecraft/`, the `IDECraft*` themes,
`crates/ui/keymaps/macos-deviations.json`) are under the project's licence.

The Rust dependencies and the native libraries compiled into the binaries are listed with their
licence texts in [`../THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md); upstream notices are in
[`../NOTICE`](../NOTICE). Questions about the licences: [`../LICENSING.md`](../LICENSING.md).
