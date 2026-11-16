# ADR-0019: Accessibility path for the GPUI windows

- **Status:** proposed · 2026-11-04 · the toolkit upgrade is the project owner's decision; the interim rules
  below apply now. Same day, the Project Owner chose the spike ("Spike auf Branch", PO-STATUS
  question 22): a time-boxed spike with upstream GPUI pinned as a Git dependency after zed#64143,
  `gpui-ce` as fallback, on its own branch as a draft PR that is not merged. The ADR stays proposed
  until the spike's measurements decide adoption.
- **Context:** IMP-0005 asks that every window and dialog be exposed through the platform
  accessibility API (role, title, actionable buttons). IDECraft uses GPUI 0.2.2 from crates.io
  (ADR-0003). That release has no accessibility tree: a screen reader or an accessibility client
  sees an IDECraft window as one opaque view. Only native alerts drawn by macOS (quit question,
  SPEC-0261; trust question, SPEC-0278) are readable and pressable. State of the toolkit on
  2026-11-04, checked against public sources:
  - **crates.io `gpui`:** newest release 0.2.2 (2025-10-22); no AccessKit dependency.
  - **Upstream Zed repository:** AccessKit support in GPUI was merged on 2026-05-27
    (zed-industries/zed PR #56065, "gpui: Accesskit support", +2781 lines, GPUI only, Zed's own
    components not annotated yet); stable accessibility identifiers followed on 2026-08-07
    (PR #61926, `accessibility_id(...)`, mapped to `AXIdentifier` on macOS); a retain cycle that
    kept every closed macOS window alive through the accessibility adapter was fixed on
    2026-09-20 (PR #64143). The Windows screen-reader issue (#41138) is still open. Upstream GPUI
    is not published to crates.io; it is only available as a Git dependency, and the platform code
    has moved into separate crates (`gpui_macos`, `gpui_windows`, `gpui_linux`, `gpui_platform`).
  - **GPUI Community Edition (`gpui-ce`, Apache-2.0):** version 0.3.0 published 2026-10-20,
    split into about 30 crates (`gpui_ce_macos`, `gpui_ce_linux`, …); it depends on
    `accesskit 0.24` and, on macOS, `accesskit_macos 0.26`. It is a fork with its own crate names
    and API changes since 0.2.
- **Options:**
  1. **Stay on GPUI 0.2.2** (no tree); make every dialog decision reachable through MCP and the
     command line; draw simple confirmations as native alerts. Low risk, no screen-reader access
     to drawn dialogs, tool windows or the editor.
  2. **Upstream GPUI as a Git dependency** at a pinned commit after PR #64143. Gives AccessKit with
     the code Zed itself ships, but crosses the 0.2 → main API break (platform crates,
     renamed APIs), pins IDECraft to an unreleased commit, and `cargo deny` must allow a Git
     source. Every view then needs roles, labels and actions added by hand.
  3. **Switch to `gpui-ce` 0.3.** A published crate with AccessKit, but a fork maintained by a
     community team, with a breaking rename of every GPUI import and the same per-view annotation
     work as option 2.
  4. **Bridge only IDECraft's own dialogs** by attaching an AccessKit subtree to the GPUI 0.2.2
     `NSView` (via `accesskit_macos` directly). Rejected: GPUI 0.2.2 owns the view and its
     `accessibility*` methods; a second adapter on the same view would fight GPUI's focus and hit
     testing, and would not exist on Linux or Windows.
- **Decision (interim, now):** option 1. Simple yes/no confirmations become native alerts
  (SPEC-0281); every decision of a drawn dialog has an MCP tool and, through `idecraft call`, a
  command line (SPEC-0281); the inventory of dialogs is machine-readable (MCP
  `dialog_decisions`). No toolkit upgrade without the project owner.
- **Proposed next step (for the project owner):** a time-boxed spike of option 2 on a branch, pinned to an
  upstream commit after PR #64143: build, run the window self-test, measure the M3 spike criteria
  (ADR-0003), annotate the trust and delete dialogs and one tool window, and check them with the
  macOS Accessibility Inspector. Option 3 is the fallback if upstream stays unpublished. The
  crate boundary of ADR-0003 (toolkit only in `crates/ui`, `crates/workbench`, `crates/app`)
  keeps either switch local.
- **Follow-up (2026-11-07):** [ADR-0027](0027-ui-driver-and-semantic-tree.md) makes the
  accessibility tree the source of truth for IDECraft's UI driver as well. Its phases 1 and 2 run on
  GPUI 0.2.2 with a bridge producer of AccessKit nodes; its phase 3 (one tree for screen readers
  and the driver) needs the adoption decided here, after the spike's pending window measurements.
- **Consequences:** IMP-0005 stays "partly implemented": decisions are reachable for automation and
  native alerts are accessible; drawn dialogs, tool windows and the editor stay invisible to
  screen readers until a toolkit with an accessibility tree is adopted.
