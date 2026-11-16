# Bundled keymaps

These files are unchanged copies from `JetBrains/intellij-community`, licensed under the Apache License 2.0
(see the repository's `NOTICE` and `LICENSES/Apache-2.0.txt`):

| File | Source path | Commit |
|---|---|---|
| `$default.xml` | `platform/platform-resources/src/keymaps/$default.xml` | `d4018fb57c367e7b5d895bd8a0c84c382efd0e17` |
| `Mac OS X 10.5+.xml` | `platform/platform-resources/src/keymaps/Mac OS X 10.5+.xml` | `d4018fb57c367e7b5d895bd8a0c84c382efd0e17` |

They are read by `crates/ui/src/keymap.rs` (own code, SPEC-0028). Do not edit them; update by copying a newer
upstream revision and recording the commit here and in `docs/legal/PROVENANCE.md`. A test holds their length
and checksum (`keymap::tests::bundled_macos_keymap_differs_from_upstream_only_as_listed`).

`macos-deviations.json` is IDECraft's own file, not an upstream copy: every shortcut by which the bundled
macOS keymap differs from these two files, with the specification that decides it and the reason
(SPEC-0028 v4 behaviour 7). The same test fails on any difference that is not listed there;
`scripts/keymap-coverage.py` reads it for `docs/research/KEYMAP_COVERAGE.md`.
