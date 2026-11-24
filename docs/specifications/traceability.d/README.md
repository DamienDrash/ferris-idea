# Traceability fragments

A pull request adds its rows of [`TRACEABILITY.md`](../TRACEABILITY.md) here as
`traceability.d/<PR number>.md` (or `<branch>.md` before the pull request exists) instead of
appending them to the end of `TRACEABILITY.md`, where every other pull request appends too
([ADR-0035](../../adr/0035-tool-registry-and-number-reservations.md) decision 5). A fragment holds
table rows only, in the format of `TRACEABILITY.md`, one row per acceptance criterion:

```markdown
| SPEC-0338/AK-1 | `crates/core/src/check_gates.rs` (`default_gates`) | `check_gates::tests::default_gates_per_build_system` |
```

- Every row names a spec that exists and has three cells: spec/AK, implementation, tests.
- A key (`SPEC-NNNN/AK-N`, with its qualifier such as `(v2)`) is given once in `TRACEABILITY.md`
  and the fragments together.
- A row that changes an existing row of `TRACEABILITY.md` is still edited there.

`scripts/traceability.sh check` checks the fragments; `scripts/check-docs.sh` runs it. At the daily
version cut, after `scripts/changelog.sh assemble`, `scripts/traceability.sh assemble` appends the
rows of every fragment to `TRACEABILITY.md` and deletes the fragments.
