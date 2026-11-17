# SPEC-0211: Reviewing, updating and inspecting plugins

- **Version:** 3 · **Date:** 2026-11-17 (v3: installing writes the bytes that were reviewed,
  behaviour 7; grants carry over only to the same plugin, a different plugin under the same file
  name is a Replace, behaviour 8; AK-6 — headless check of F-080, OBS-0077); 2026-11-15 (v2: the
  review in the semantic tree, behaviour 6; AK-5)
- **Sources:** GEN (IDEs show a plugin's name, version, description and what it changes before it
  is installed, and update an installed plugin to a newer version; F-080), SPEC-0126, SPEC-0156,
  SPEC-0210, ADR-0008, ADR-0016
- **Feature matrix rows:** F-080, F-002

## Purpose
Before a plugin is installed or updated, the user sees what it brings and what it asks for, and
which version replaces which; the installed list says what each plugin contributes.

## Behaviour
1. **List:** each row of Settings › Plugins names, besides name, version and description, what the
   plugin contributes — `N actions`, `completion`, `inspections`, `status bar`, `tool window`,
   `file types: Name (.ext)` — and its permissions with their state (SPEC-0156 behaviour 3). [GEN]
2. **Review before installing:** Install Plugin from Disk… (⌘O) no longer installs at once. The
   chosen file is loaded and checked; the page then shows a review: "Install" (new file name),
   "Update `old` → `new`" (same file name, newer version), "Reinstall" (same version) or
   "Downgrade `old` → `new`" (older version, shown as a warning), with the plugin's contributions
   and permissions. Enter or the Install button installs; Escape or Cancel drops it. A file that
   does not load is refused with the reason and nothing is shown for review. [GEN, IMP-0090]
3. **Versions** are compared by their dot-separated parts, numerically where both parts are
   numbers, else as text; a missing part counts as 0. [GEN]
4. **Grants on update:** what the user granted stays granted only for permissions the new version
   still asks for; a permission the new version asks for and the old one did not is shown as "not
   granted" in the review and is never granted by updating. [IMP-0090]
5. **MCP:** `plugins` lists each plugin's contributions; `inspect_plugin_file` loads a `.wasm` file
   of the workspace without installing it and returns its manifest, contributions, permissions,
   and, against the installed plugins, whether it would install, update, reinstall or downgrade.
   Installing stays with the user (SPEC-0126 behaviour 5). [ADR-0008]
6. **(v2) Semantic tree (SPEC-0319):** the review's headline is a heading (`plugin-review-headline`),
   each permission a check box (`plugin-review-grant-<i>`), its buttons `plugin-review-cancel` and
   `plugin-review-install`; the Install button and the check boxes are reserved for the user
   (`decision`, SPEC-0319 UISEC-4), so an external driver can neither install a plugin nor grant its
   rights. "Install Plugin from Disk…" is a button (`settings-plugins-install-from-disk`). [ADR-0027]

7. **(v3) What was reviewed is what is installed:** the file is read once (at most 16 MiB, also
   when it grows while it is read) and the review keeps the bytes it checked; Enter installs
   exactly these bytes, never the file again. Until v3 the file was read anew on Enter, so a file
   that changed between the review and Enter (a download still being written, a file replaced by
   another program) was installed unreviewed. What installing does (behaviour 2) is decided again
   at that moment against the installed plugins. The module is written through a new file beside
   the target that is created fresh (an old leftover or a link there is removed first, never
   written through) and renamed into place; a failure removes it. [GEN, rule 12]
8. **(v3) Grants belong to one plugin:** a file is the same plugin as the one installed under its
   file name only when that one loads and its manifest has the same `name`. Only then is it an
   Update, Reinstall or Downgrade, and only then do grants carry over (behaviour 4). A loadable
   plugin with another `name` under the same file name is shown as "Replace `old name` `old
   version` with `name` `version`", as a warning like a downgrade; a file name without a loadable
   plugin is an Install. For Install and Replace the review shows every permission as not granted,
   and installing first drops what was granted under that file name — permissions
   (`permissions.json`) and network grants (SPEC-0314 behaviour 5) — also grants left behind by a
   plugin removed by hand. Until v3 any plugin installed under a file name inherited what the user
   had granted to the plugin of that name before, without being asked. The MCP `inspect_plugin_file`
   reports `replace` with `from_name` and `from`. [GEN, rule 12]

## Acceptance criteria
- **AK-1** Version order; review of a new, a newer, the same and an older version; grants kept and
  dropped on update; contributions listed (`crates/plugins/tests/samples.rs`:
  `review_and_update`).
- **AK-2** MCP `inspect_plugin_file` (`crates/mcp/tests/it/protocol.rs`: `plugin_extension_points`).
- **AK-3** Window self-test (`plugin_steps`): ⌘O's file is shown for review and only Enter
  installs it; reinstalling the same file says "Reinstall"; Escape drops a review.
- **AK-4** Hand test (round 4, no. 151): update a sample plugin to a newer version and read the
  review.
- **AK-5 (v2)** Window test (`crates/workbench/tests/it/f080_window.rs`:
  `hand_test_151_install_reinstall_update_downgrade`): the review of a plugin asking for
  `project-read` reads "Install Marker 1.0", Space ticks the grant and a click on Install installs
  and grants; the same file is "Reinstall Marker 1.0" and Escape drops only the review; version 1.1
  is "Update Marker 1.0 → 1.1" and keeps the grant; 1.0 again is "Downgrade Marker 1.1 → 1.0" and
  Cancel keeps 1.1. The native file dialog itself stays for the run next to the reference.
- **AK-6 (v3, behaviours 7 and 8)** `crates/plugins/tests/samples.rs`:
  `reviewed_bytes_and_grants_of_the_same_plugin_only`: a permission and a network grant left behind
  under a file name are shown as not granted and dropped by an Install; a file replaced after its
  review installs the reviewed module, not the new file, and leaves no `.part`; an Update keeps both
  grants; a plugin with another name under the same file name is "Replace Sample Panel 1.1 with
  Other Panel 2.0" (`kind` `replace` in JSON), shows nothing granted and drops both grants. In a
  window: not checked (the warning colour of a Replace).

## Out of scope
Downloading updates from a repository or marketplace, signatures, automatic updates, dependencies
between plugins.

## Known deviations from the reference
The reference installs from disk without a separate review of permissions (its plugins have none);
IDECraft shows contributions and permissions before anything is copied (IMP-0090).
