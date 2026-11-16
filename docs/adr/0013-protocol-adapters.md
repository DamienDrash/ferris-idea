# ADR-0013: MCP, ACP, CLI and SDK as adapters over the capability layer

- **Status:** proposed · 2026-10-31 · planned for milestones M10–M11; extends ADR-0008
- **Context:** Agents reach IDEs through two different protocols: the **Model Context Protocol** (MCP,
  agents call the IDE's tools; latest revision `2026-07-28`, stateless, Tasks as an extension, Roots and
  Sampling deprecated) and the **Agent Client Protocol** (ACP, the IDE hosts an agent as a subprocess;
  stable version 1). Not every agent supports both: Gemini CLI speaks ACP natively, Claude Code and Codex
  CLI only through adapters, and some agents only offer a headless CLI with JSON output. Protocol
  revisions are frequent. State checked on 2026-10-31, sources in the [vision](../architecture/AI_NATIVE_VISION.md) §2.3.
- **Decision:**
  1. Every external interface — MCP server, ACP client, CLI, SDK, later remote access — is an adapter over
     the capability layer of ADR-0012. Adapters contain translation, transport and protocol state only;
     no domain logic.
  2. **MCP:** keep `rmcp` (ADR-0008). Generate the tool list and JSON schemas from the capability registry;
     add Streamable HTTP on localhost for the running GUI (M6.8); use the Tasks extension for long
     operations; take the workspace as an explicit parameter rather than relying on Roots; do not depend on
     Sampling.
  3. **ACP:** IDECraft is an ACP *client* that hosts agents. `fs/*` and `terminal/*` callbacks map to
     `workspace.*` and `execution.*` with the same permission checks as every other caller. IDECraft passes
     its own MCP server to the agent in `session/new`.
  4. **Agent adapter layer:** per agent type, a record of how it is connected (native ACP, ACP adapter,
     headless CLI) and which features are missing; missing permission callbacks force stricter isolation
     (own worktree, no network grant).
  5. **A2A** (Agent2Agent, Linux Foundation) is not used for coordination inside IDECraft; an optional
     bridge for external agent systems may follow.
  6. Protocol libraries are added only when their milestone starts, after the licence check of
     `deny.toml`.
- **Consequences:** One implementation per operation; protocol upgrades touch one adapter. The ADR-0008
  rule "every IDE capability gets an MCP tool" becomes "every capability is registered once and exposed by
  all adapters".
