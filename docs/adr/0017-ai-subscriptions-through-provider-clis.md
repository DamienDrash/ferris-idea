# ADR-0017: Chat subscriptions through the providers' own command line programs

- **Status:** accepted · 2026-11-03
- **Context:** The project owner wants the AI Assistant to use a chat subscription (Claude Pro/Max, ChatGPT,
  Google account) like OpenClaw and Hermes Agent do, with API keys as the alternative. Those tools
  either run their own OAuth flow with the provider's client (OpenAI Codex OAuth) or reuse a
  signed-in CLI. Anthropic's legal and compliance page states that OAuth tokens of Claude Free, Pro
  and Max accounts must not be used in other products, tools or services (enforced since early 2026;
  The Register, 2026-02-20); OpenAI states that Codex sign-in may be used outside the Codex CLI;
  OpenClaw no longer offers Gemini CLI OAuth.
- **Decision:** Ferris IDEA never takes, stores or sends a subscription's tokens. With a subscription
  chosen, it runs the provider's own program that the user installed and signed in to — `claude`
  (Claude Code), `codex` (Codex), `gemini` (Gemini CLI) — once per answer, as a plain chat: in an
  empty temporary folder, Claude Code without tools, settings sources, MCP servers, slash commands or
  session persistence; Codex with a read-only sandbox, no approvals and an ephemeral session; Gemini
  CLI in plan (read-only) mode. Signing in happens in the program, started by Ferris IDEA in a terminal
  tab. The API endpoint with a keychain key stays the alternative.
- **Consequences:** Each provider decides about its program's use; usage counts against the plan's
  limits, and a provider can change what a plan covers (Anthropic, for example, may bill third-party
  driven use of Claude Code as extra usage). Answers come slower than through an API (the program
  starts each time) and Codex sends a whole message at once. No new crate or dependency. Driving the
  agents as agents (their tools, editing the project) stays with M11 (ACP) behind QG-1.
