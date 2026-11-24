# Reserved specification and ADR numbers

A specification or ADR number is reserved here before a branch uses it, so two branches never pick
the same "next free" number ([ADR-0035](../adr/0035-tool-registry-and-number-reservations.md)
decision 6).

- **Taking a number.** `scripts/spec-number.sh next` (or `next ADR`) prints the next free number:
  one above the highest used by a file or reserved here, on `origin/main` and in your tree. Add one
  row below on a fresh branch from `origin/main` and merge that pull request at once, before the
  branch that writes the specification; or the coordinator assigns the number and adds the row.
- **The check.** `scripts/spec-number.sh check` (run by `scripts/check-pr.sh`) fails when a branch
  adds a `SPEC-NNNN-*.md` or `docs/adr/NNNN-*.md` whose number another file on `origin/main`
  already uses, or that this file on `origin/main` reserves for another branch. A branch cell may
  name several branches, comma-separated (a branch stacked on another); an empty one reserves the
  number for whichever branch uses it first.
- **After the merge.** A row stays as the record; the number is then used by its file.
- Rows from before this file (open pull requests of 2026-11-18) were taken from those branches.

| Number | Topic | Branch | Date |
|---|---|---|---|
| SPEC-0338 | Check gates | m19-3-check-gates | 2026-11-17 |
| SPEC-0339 | Context packs | m19-2-context-packages | 2026-11-17 |
| SPEC-0340 | Change sets | m19-4-change-sets | 2026-11-17 |
| SPEC-0341 | Agent heartbeat | m19-12-agent-heartbeat | 2026-11-17 |
| SPEC-0343 | Memory layers | m19-14-memory | 2026-11-18 |
| SPEC-0344 | Session history | m19-10-session-history-v2 | 2026-11-18 |
| SPEC-0345 | Agent benchmark | m19-11-benchmark | 2026-11-18 |
| SPEC-0346 | Shared agent configuration | m19-8-agent-config | 2026-11-18 |
| SPEC-0347 | Agent process sandbox | m19-9-sandbox | 2026-11-18 |
| SPEC-0348 | ACP agent host | m19-5-agent-host, m19-6-agent-control | 2026-11-18 |
| SPEC-0349 | Agent control | m19-6-agent-control | 2026-11-18 |
| SPEC-0350 | Models and providers | m19-7-models | 2026-11-18 |
| SPEC-0351 | Project cockpit | m20-4-cockpit | 2026-11-18 |
| SPEC-0352 | Tickets | m20-1-tickets | 2026-11-18 |
| SPEC-0353 | Internal vault for agents | m19-15-vault | 2026-11-18 |
| SPEC-0354 | Linux packages | linux-packages | 2026-11-18 |
| SPEC-0355 | Welcome window | welcome-window | 2026-11-18 |
| ADR-0035 | Tool registry and number reservations | mcp-tool-registry | 2026-11-18 |
| ADR-0036 | Windows support | windows-prep | 2026-11-18 |
| SPEC-0356 | Taking over what the former name left behind | rebrand-step2-takeover | 2026-11-23 |
| SPEC-0358 | Claude Code link | claude-code-ide-link | 2026-11-23 |
| SPEC-0357 | Agents in the window | f082-agents-window | 2026-11-23 |
