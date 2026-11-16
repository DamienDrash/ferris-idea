# Bundled icons

New UI icons, unchanged copies from `JetBrains/intellij-community`, `platform/icons/src/expui/` at commit
`750ce16ff6661b1210ea1bbda65c847f753eeb4b`, licensed under the Apache License 2.0 (see the repository's `NOTICE` and `LICENSES/Apache-2.0.txt`). Each icon comes as
`name.svg` (light) and `name_dark.svg` (dark), under the same sub-folder as upstream (`nodes/`,
`general/`, `fileTypes/`, `toolwindows/`, `codeInsight/`, `run/`, `breakpoints/`, `debugger/`, `status/`). `toolwindows/terminal*.svg` comes from the open terminal plugin of the same repository
(`plugins/terminal/resources/icons/expui/toolwindow/`). Only icons of open areas are taken (ADR-0009); no product logos.

Files named `name@20x20.svg` / `name@20x20_dark.svg` are the upstream 20 px variants used on the
tool window stripes and in the main toolbar (SPEC-0061 v4); `toolwindows/terminal@20x20*` comes from the
terminal plugin like the 16 px one. `toolwindows/todo*.svg` (the TODO tool window, SPEC-0334; 16 px and
20 px, light and dark) are unchanged copies from `platform/icons/src/expui/toolwindows/` at commit
`d4018fb57c367e7b5d895bd8a0c84c382efd0e17`. So are `toolwindows/structure*.svg`, `toolwindows/notifications*.svg` (the Structure and
Notifications tool windows, SPEC-0061 v17; 16 px and 20 px, light and dark) and `general/dropdown*.svg` (the menu
marker of header buttons), from `platform/icons/src/expui/` at the same commit.

They are read by `crates/ui/src/icons.rs` (own code, SPEC-0060). Do not edit them; update by copying a newer
upstream revision and recording the commit here and in `docs/legal/PROVENANCE.md`.

`idecraft/` holds IDECraft's own icons (under the project's licence, `LICENSE.md`) for areas whose reference icons are not open, such
as the Database tool window (keys and indexes) and the file types Rust, Go, Ruby, PHP, Svelte and Astro. `fileTypes/python*` and
`fileTypes/kotlin*` come from the open Python and Kotlin plugins of the same repository (see `PROVENANCE.md`). They are drawn from scratch in the same 16 px stroke
style, not traced from the reference. `idecraft/debug*.svg` (pause, run to cursor, rerun, mute and view breakpoints in the Debug
window's toolbar, SPEC-0194 v3) are own drawings too, until the open platform icons for them are copied. `idecraft/gitTag.svg` (the tag
labels of the Git log, SPEC-0086 v7) is an own drawing as well, and so are the Git icons
`idecraft/{vcsUpdate,vcsCommit,vcsPush,tag,rollback,diff,groupBy}*.svg` (branch popup, SPEC-0068 v5; Commit window
toolbar, SPEC-0063 v7), and so are the favourite stars `idecraft/{star,starOutline}*.svg` (branch popup and the
branches pane of the Git log, SPEC-0068 v8, SPEC-0086 v10). The search icons `idecraft/{filter,preview,openInFind,newLine,pin,history,preserveCase}*.svg`
(Search Everywhere, find bar and Find in Files, SPEC-0067 v5, SPEC-0065 v4, SPEC-0066 v3) are own drawings as well. So are `idecraft/{execute,offline,skipTests}*.svg` (the build tools window's toolbar, SPEC-0100 v2), the folder roles
`idecraft/{sourceRoot,testRoot,resourcesRoot,testResourcesRoot,excludedRoot}*.svg` and the POM icon `idecraft/maven*.svg`
(project tree, SPEC-0048 v3), and `idecraft/{download,dependencyAnalyzer,diagram,remove}*.svg` (sources, dependency
analyzer, diagram and unlink in the build tools window, SPEC-0100 v3). So are `idecraft/{record,annotation,object}*.svg`
(the kinds of a Java or Kotlin file's primary declaration that have no bundled node icon, on tabs and in the project
tree, SPEC-0131 v3): a circle badge with a stroked letter, in colours the bundled node icons already use. The override mark `idecraft/overrideMark*.svg` (a file whose type is overridden, project tree and tabs,
SPEC-0004 v6) is an own drawing as well.
