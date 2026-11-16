# ADR-0008: Built-in MCP server

- **Status:** accepted · 2026-10-30 · the HTTP transport for the running window followed on
  2026-11-03 (`SPEC-0221`)
- **Context:** LLM agents should be able to use the IDE as a tool. The Model
  Context Protocol (MCP) is the common interface; the reference platform ships
  an open-source MCP server plugin (`plugins/mcp-server`, Apache-2.0) whose
  tool grouping serves as a logic template.
- **Decision:** New crate `idecraft-mcp` built on the official Rust SDK `rmcp`
  (Apache-2.0). Transport first: stdio (`idecraft mcp <workspace>`); later
  streamable HTTP on localhost for a running UI instance. Tools mirror IDE
  capabilities and grow with them; every tool works on one workspace root and
  refuses paths outside it. Write tools edit through the editor model so undo
  works when the UI exists.
- **First tools:** `list_files`, `read_file`, `detect_language`,
  `split_sql_statements`, `workspace_info`.
- **Consequences:** Each new IDE feature should get an MCP tool and a test.
  MCP access is local only by default; remote access needs explicit opt-in.
