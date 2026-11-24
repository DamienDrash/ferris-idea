# Agent benchmark tasks

Tasks of the agent benchmark (`docs/specifications/SPEC-0345-agent-benchmark.md`, ROADMAP M19.11).
Each folder holds `task.toml` (prompt, grants, limits, checks), `project/` (the starting state, copied
for every run and never changed) and `scripts/` (recorded runs replayed without a model; a name
starting with `fail` is a negative control the checks must catch).

Run them with `ferris-idea agent-bench` or the MCP tool `agent_benchmark`; the test
`crates/mcp/tests/it/agent_bench.rs` runs them in CI.
