# Tests

- Unit tests live next to the code (`cargo test --workspace`).
- `functional/` holds one black-box test module per feature-matrix row (M7.4): it drives the
  `ferris-idea` binary and MCP (stdio and HTTP) on copies of the reference projects; run
  `scripts/functional.sh` for the coverage table `docs/research/FUNCTIONAL_COVERAGE.md`.
- `reference-projects/` holds small, self-written projects with synthetic data.
  They serve two purposes: as inputs for black-box observations of the
  reference IDE (phase A, after GO-1) and as fixtures for Ferris IDEA's own
  integration tests. All content is original; no third-party code or data.
