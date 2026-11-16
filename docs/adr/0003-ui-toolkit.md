# ADR-0003: UI toolkit GPUI, fallback egui

- **Status:** accepted · 2026-10-30 · confirmed by the M3 spike on 2026-11-03 (all five criteria met: `docs/research/SPIKE_GPUI.md`)
- **Context:** The UI must be Rust, render large source files smoothly, handle
  IME and complex scripts, and run on macOS, Linux and Windows. Evaluation:
  `docs/research/TECH_EVALUATION.md`.
- **Decision:** GPUI (Apache-2.0) is the target toolkit. All toolkit code stays
  inside `crates/ui` and `crates/app`; feature crates never import it.
- **Spike acceptance criteria (M3):**
  1. Open a 100 000-line file; scrolling stays at display refresh rate on an M1-class Mac.
  2. Typing latency below 16 ms (key event to frame) in that file.
  3. German umlauts, dead keys and an IME (Japanese) input work.
  4. Two windows, split panes, a tree view and a table with 100 000 rows.
  5. Builds on macOS and Linux in CI.
- **Fallback:** If criteria 1–4 fail or GPUI's published crate falls too far
  behind, switch to egui (MIT OR Apache-2.0), which also scores better on
  accessibility (AccessKit). The crate boundary keeps the switch local.
- **Risks:** GPUI is pre-1.0 and driven by Zed's needs; accessibility support
  is limited; documentation is sparse.
