# ADR-0015: Quality gate QG-1 before AI-native work starts

- **Status:** accepted · 2026-11-02 · the Project Owner confirmed the thresholds below ("decide by the
  gold standard", PO-STATUS 2026-11-02)
- **Context:** The AI-native milestones (M10–M15, [future milestones](../architecture/FUTURE_MILESTONES.md))
  must not delay or narrow the classical IDE. They need a measurable entry condition based on what the
  project already measures: the parity measure ([PARITY.md](../research/PARITY.md), `scripts/parity.sh`),
  the test suite, CI and the performance observations.
- **Decision:** Active work on the AI-native milestones M10–M15 starts only when **all** of the following
  hold, each with evidence in ROADMAP.md (M8 Stabilisation & Hardening belongs to the classical phase and works towards the gate; M9 is the gate itself):
  1. **Parity:** `scripts/parity.sh` reports at least **85 %** overall, and every matrix row with weight 5
     or 4 is at level 3 or higher.
  2. **Verified parity:** at least **50 %** of the total weight is at level 4 (compared against the
     reference with observation and test).
  3. **Tests:** `cargo test --workspace` and the window self-test (`idecraft ui --selftest`) pass on macOS
     and Linux; **20 consecutive CI runs on `main` without a flaky failure**.
  4. **Performance:** typing latency p95 below 16 ms in a 100 000-line file; cold start and idle memory at
     or below the reference values recorded in the observations (OBS-0001 ff.).
  5. **Stability:** no open blocker or escalation in PO-STATUS; no known data-loss bug (unsaved changes,
     file writes, database edits).
  6. **Architecture readiness:** readiness items A1–A3 are decided and A2 (document owner) is implemented
     as part of the classical M6.8.
  7. **Release:** the Project Owner gives an explicit go (**GO-5**).
- **Consequences:** The future milestones stay visible but are marked as blocked by QG-1; agents working
  autonomously on the roadmap do not pick them up. Thresholds can be changed only by the Project Owner and
  are recorded here.
- **Amendment 2026-11-07:** the Project Owner moved the headless service `idecraft serve` (M10.3)
  before the gate: it is built as the backend of the remote project (ROADMAP M6.12), without AI
  functions, and remote control of the local app (M16) follows it. The rest of M10–M15 and the mobile
  apps (M17) stay behind QG-1 ([plan](../architecture/REMOTE_AND_MOBILE.md); PO-STATUS question 27).
