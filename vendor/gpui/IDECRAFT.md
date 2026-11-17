# gpui 0.2.2, vendored with three changes

This folder is the published crate [gpui 0.2.2](https://crates.io/crates/gpui/0.2.2) (Apache-2.0,
Zed Industries; [`LICENSE-APACHE`](LICENSE-APACHE)), unpacked from the `.crate` file whose SHA-256
`979b45cfa6ec723b6f42330915a1b3769b930d02b2d505f9697f8ca602bee707` is the checksum crates.io
publishes and the workspace's `Cargo.lock` had recorded for it. The workspace uses it through
`[patch.crates-io]` in the root `Cargo.toml`, on every platform.

What differs from the published crate, and nothing else:

1. **`src/window.rs`: `Window::dispatch_in_process_pointer_input`** (marked with a comment "IDECraft
   change"). It hands a pointer event (move, button down or up, exit, scroll) to the window's
   `dispatch_event`, the handler the platform layer calls for real input; keyboard events and file
   drops are refused. gpui 0.2.2's `dispatch_event` is public but returns a type private to the
   crate, so code outside it cannot call it (SPEC-0319 v2 behaviour 16a). The addition is the
   smallest hook that lets the window self-test click and hover on Linux in process, without the
   X server or the compositor (SPEC-0319 v5 behaviour 20c, UISEC-9); IDECraft calls it in
   `crates/app/src/native_click.rs` only, which a scan test checks.
2. **The answer of a prompt that holds the keyboard focus** (each place marked "IDECraft change";
   SPEC-0281 v3 behaviour 8, rule 12). `src/platform.rs` adds the variant `PromptButton::Safe`
   (with `safe` and `is_safe`): the answer a prompt names as safe, one that grants nothing and
   removes or closes nothing; otherwise it behaves like `Other`. It also adds
   `prompt_focused_answer`: the named safe answer, else the first cancel answer (Escape), else the
   first answer, the declared default (Return). In `src/platform/mac/window.rs` the macOS alert
   adds the answers in the order given (gpui 0.2.2 added the last non-Cancel answer last, and
   NSAlert focuses the last button it draws, so with keyboard navigation on Space pressed it — in
   IDECraft's question for an agent's right that was "Always for This Workspace"), lays the alert
   out and makes the focused answer's button the window's initial first responder, and its first
   responder again once the sheet is shown. `src/platform/test/platform.rs` keeps the focused
   answer of each prompt and `src/app/test_context.rs` adds
   `TestAppContext::pending_prompt_focused_answer`, so window tests can check it.
   `src/platform/windows/window.rs` maps `Safe` like `Other` (the match must stay exhaustive);
   Windows is otherwise unchanged; on Linux gpui draws the prompt itself (change 3).
3. **The prompt gpui draws where the platform has no alert — Linux, X11 and Wayland** (each place
   marked "IDECraft change"; SPEC-0281 v4 behaviour 9, SPEC-0319 v6 behaviour 35, rule 12). gpui
   0.2.2's fallback prompt took no key at all. In `src/window/prompts.rs` the
   `FallbackPromptRenderer` now answers the keyboard like the macOS alert with keyboard navigation
   on: Return gives the first answer (the default), Escape the first cancel answer and nothing
   without one, Space the focused answer, which starts on `prompt_focused_answer`; Tab and
   Shift+Tab move the focus, wrapping around, and a 2-pixel blue ring around the button shows it.
   Keys reported as held (auto-repeat; the X11 backend reports each repeat as a new press) and
   keys with Control, Alt, the platform key or Fn do nothing; a key
   that types a character moves the focus back to the safe answer, so typing on into a prompt
   that appeared unexpectedly gives at most the safe answer; no key goes past the prompt. The
   answers stay top to bottom in the order given. Its cover over the window occludes the pointer
   (gpui 0.2.2 let clicks through to the window behind). It records where it painted the question
   and each answer, and tells its message, detail, answers, focused, default and cancel answer
   (`message`, `detail`, `answers`, `focused_answer`, `default_answer`, `cancel_answer`,
   `question_bounds`, `answer_bounds`), from which IDECraft builds the question's accessibility
   nodes (`crates/workbench/src/semantics.rs`). `RenderablePromptHandle` keeps the prompt's focus
   handle and a focus request made while it is shown. In `src/window.rs`, `Window::focus` and
   `Window::blur` leave the focus in a drawn prompt while it is shown and remember the request,
   which is carried out once the prompt is answered (gpui 0.2.2 restored the focus the window had
   before, so a view that asked for it meanwhile lost it); `focus_ignoring_prompt` is the path the
   prompt takes itself; `has_drawn_prompt` and `fallback_prompt` tell whether a prompt is drawn and
   give the fallback prompt, so IDECraft's UI driver delivers no input of an external client into
   a window while one is open. A scan test (`crates/uidriver/tests/it/scans.rs`) lists every
   change in `src/window.rs` and checks that none but the pointer hook of change 1 dispatches
   input.
4. **`Cargo.toml`:** the `[[example]]` entries are removed, and with them the folder `examples/`
   (4.7 MB of demos that are never built here). `Cargo.lock` of the package is removed (a path
   dependency uses the workspace's lock file).
   Its `taffy` dependency is raised from `=0.9.0` to `=0.9.2` (2026-11-16): taffy 0.9.0 requires
   `grid` 0.18, which has GHSA-38c5-483c-4qqp (integer overflow in `Grid::expand_rows`, undefined
   behaviour through a safe API, `grid` >= 0.17, <= 1.0.0); taffy 0.9.2 requires `grid` ^1.0.0,
   which resolves to the fixed 1.0.1. The patch release changes no API that gpui uses: between
   0.9.0 and 0.9.2 the hand-written `Default` impls of the style enums became `derive(Default)`
   with the same defaults, and block, flexbox and grid layout got fixes. `Cargo.toml.orig` stays as
   published. gpui sizes a grid only for an element styled `display: grid` (`Styled::grid`);
   no IDECraft crate calls it.
5. **`src/text_system.rs`: font runs of a line** (marked "IDECraft change"; SPEC-0290 behaviour 4,
   OBS-0084 L2). gpui 0.2.2's `WindowTextSystem::layout_line` joined a text run with the run before
   it whenever both had the same colour, underline and strikethrough, without comparing their
   fonts, so an italic or bold run that followed a run of the same colour was shaped in the font
   before it (a static method call in the text colour after `println(` lost its italics, while a
   static field in its own colour kept them). The new public function `font_runs_of` joins a run
   with the one before it only when both resolve to the same font and have the same decorations,
   the rule `shape_text` already followed; `layout_line` builds its font runs with it.
6. **Notices of change (Apache-2.0 §4(b)):** every changed file (`Cargo.toml`, `src/window.rs`,
   `src/text_system.rs`, `src/window/prompts.rs`, `src/platform.rs`, `src/platform/mac/window.rs`,
   `src/platform/windows/window.rs`, `src/platform/test/platform.rs`, `src/app/test_context.rs`)
   starts with a comment "Modified by IDECraft" that names the change and points here.

The files are otherwise byte for byte as published; `cargo fmt` and the workspace lints do not run
on this folder (it is excluded from the workspace). To update gpui, unpack the new version over
this folder, apply the changes again and update this file and the checksum.
