# Security policy

Ferris IDEA is at the stable stage (since v1.0.0) and has had no independent security review. It runs programs, opens network
connections to hosts you name and offers its operations to AI agents, so reports about its
security are welcome and taken seriously.

## Reporting a vulnerability

Report privately. Do not open a public issue, pull request or discussion for a security problem.

- Use GitHub's private vulnerability reporting: "Report a vulnerability" on the Security tab of
  this repository. Only the project owner sees the report.
- If that is not possible for you, write to the project owner at the contact named in the
  [README](README.md#who-decides) and say only that you have a security report; you will get a
  way to send it privately.

Please include:

- the version (`ferris-idea --version`) or the commit, and your operating system;
- what an attacker needs (a file in a project, a host on the network, a granted right, a paired
  device) and what they gain;
- steps to reproduce, with a minimal project or request where possible;
- whether the problem is already public.

Do not include real secrets, tokens or personal data in a report. Describe where they were
exposed instead.

## What to expect

This is a research project run by one person with AI agents, without a security team and without
a guaranteed response time. The owner reads every report, answers as soon as he can, and tells
you whether the report is accepted and what is planned. Please give a reasonable time for a fix
before you publish details. There is no bug bounty and no payment for reports. Reporters are
credited in the release notes if they wish.

## Supported versions

Only the latest version on `main` is supported. Earlier tags receive no fixes. There are no
published releases or downloads yet.

## Scope

In scope, with the documents that state the intended behaviour:

- **Agent access:** a tool that changes something without checking a right, a project file that
  gives an agent a right or a database permission, reaching a database or a database file the
  user did not grant, a changing call that leaves no line in the agent log, project trust
  (safe mode), the withholding of secret-named files, or the checks of the MCP server over HTTP
  (loopback only, token, `Host` and `Origin`):
  [SPEC-0053](docs/specifications/SPEC-0053-agent-permissions.md),
  [SPEC-0221](docs/specifications/SPEC-0221-mcp-http-transport.md),
  [SPEC-0278](docs/specifications/SPEC-0278-project-trust-and-safe-mode.md).
- **Remote paths:** SSH options, the paired channel, the WebSocket endpoint, `ferris-idea serve`:
  [ADR-0025](docs/adr/0025-network-security-for-remote-connections.md).
- **Plugins:** escaping the WebAssembly sandbox or its limits:
  [ADR-0016](docs/adr/0016-plugins-as-wasm.md).
- **UI driver:** driving the window without the `ui` right, or answering a dialog reserved for the
  user: [SPEC-0319](docs/specifications/SPEC-0319-ui-driver.md).
- **Stored secrets:** database passwords, API keys or pairing keys leaving the OS keychain, or
  appearing in a log, an export or a tool result.
- **Parsers and protocol decoders** that crash or misbehave on crafted input.

Known limits, documented and not vulnerabilities in themselves (improvements are still welcome):

- With the `run` right in a trusted project, an agent starts programs as the user. There is no
  sandbox and no per-command allow list.
- The agent log is a plain local file, and it records that a call changed something, not what was
  written.
- Some tools that only read start a program or reach a service of the user's without a right:
  language servers and the project's own formatter and linter start under project trust alone;
  commit hooks run under `vcs-write` (SPEC-0053, behaviour 14).
- SQL is classified by its text; a function or view of the database's own that changes data when
  a query reads it is not seen.
- Secrets are recognised by file name only; program output and HTTP responses are not filtered.
- The MCP server over HTTP is off by default. Once you switch it on, its token is in a file readable
  by every process of the same user.
- Prompt injection through project or web content that an agent reads is possible; two tool
  descriptions tell the agent not to follow instructions in the result.
- The way the cryptographic primitives of the remote paths are combined is this project's own code
  and has had no independent audit.

Out of scope: problems in language servers, debug adapters, database servers and other programs
Ferris IDEA starts but does not ship (report those to their projects); a machine or user account that
is already compromised; social engineering.

The README lists these limits for users under
[What the Model Does Not Cover](README.md#what-the-model-does-not-cover).
