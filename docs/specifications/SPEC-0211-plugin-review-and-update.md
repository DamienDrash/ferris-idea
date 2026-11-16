# SPEC-0211: Reviewing, updating and inspecting plugins

- **Version:** 2 · **Date:** 2026-11-15 (v2: the review in the semantic tree, behaviour 6; AK-5)
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

## Out of scope
Downloading updates from a repository or marketplace, signatures, automatic updates, dependencies
between plugins.

## Known deviations from the reference
The reference installs from disk without a separate review of permissions (its plugins have none);
IDECraft shows contributions and permissions before anything is copied (IMP-0090).
