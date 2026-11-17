# Architecture Decision Records

| ADR | Title | Status |
|---|---|---|
| [0001](0001-project-name.md) | Project name `rust-ide-lab` instead of `intellijcraft` | superseded by 0010 |
| [0002](0002-license.md) | License MIT OR Apache-2.0 (ported crates Apache-2.0) | superseded by 0032 |
| [0003](0003-ui-toolkit.md) | UI toolkit GPUI, fallback egui | accepted, confirmed by the M3 spike |
| [0004](0004-clean-room-process.md) | Clean-room process and traceability | accepted |
| [0005](0005-language-services-via-protocols.md) | Language and debug services via LSP/DAP | accepted |
| [0006](0006-apache-porting.md) | Use of IntelliJ Platform sources under Apache-2.0 | accepted, crate licence amended by 0031 |
| [0007](0007-no-restrictions.md) | No trial, licence checks, feature gates or telemetry | accepted |
| [0008](0008-mcp-server.md) | Built-in MCP server | accepted |
| [0009](0009-look-and-feel.md) | Look and feel close to the reference | accepted, colours and app icon amended by 0018 |
| [0010](0010-name-idecraft.md) | Project name IDECraft and separate brand domain | accepted, palette and app icon amended by 0018 |
| [0011](0011-database-drivers.md) | Native driver per database behind one session interface | accepted |
| [0012](0012-headless-core-capability-api.md) | Headless core and one capability layer for all callers | accepted 2026-11-14 (capability layer and core service: M10) |
| [0013](0013-protocol-adapters.md) | MCP, ACP, CLI and SDK as adapters over the capability layer | proposed |
| [0014](0014-orchestration-principles.md) | Principles for multi-agent orchestration | proposed |
| [0015](0015-quality-gate-ai-native.md) | Quality gate QG-1 before AI-native work starts | accepted |
| [0016](0016-plugins-as-wasm.md) | Plugins as sandboxed WebAssembly modules | accepted |
| [0017](0017-ai-subscriptions-through-provider-clis.md) | Chat subscriptions through the providers' own command line programs | accepted |
| [0018](0018-brand-palette-and-icon-in-the-app.md) | Brand palette and app icon in the desktop application | accepted (amends 0009, 0010 in part) |
| [0019](0019-accessibility-path-for-gpui.md) | Accessibility path for the GPUI windows | proposed (interim rules apply now; upgrade is the project owner's decision) |
| [0020](0020-database-connectors.md) | Database connectors: protocol families, declarative definitions, ODBC as the generic route | accepted |
| [0021](0021-idecraft-file-formats.md) | IDECraft file formats: one envelope, own extensions for files that travel | accepted |
| [0022](0022-non-relational-data-sources.md) | Non-relational data sources: document and key-value store families beside the SQL dialects | accepted |
| [0023](0023-idecraft-as-agent-harness.md) | IDECraft as an agent harness: tools, verification and orchestration, no agent loop of our own | accepted 2026-11-17 (rule 7 applies now; the rest is planned in M10–M12 and M19) |
| [0024](0024-plugin-network-to-profile-host.md) | Network for WebAssembly connector plugins: only to the picked profile's host and port | accepted (amends 0016) |
| [0025](0025-network-security-for-remote-connections.md) | Network security for remote connections: system SSH with strict host keys, Noise with pinned keys for paired channels, pairing with SAS, mandatory security tests | accepted (three questions with defaults for the Project Owner) |
| [0026](0026-session-protocol-window-and-serve.md) | Session protocol between the window and `idecraft serve`: framed JSON-RPC with streams, backend-owned documents with server-sequenced operations, resumable sessions | accepted |
| [0027](0027-ui-driver-and-semantic-tree.md) | UI driver and semantic interface tree: one AccessKit tree for screen readers and automation, locators by role and name, in-process input, off by default with right `ui` and emergency stop | accepted (phase 3 waits for ADR-0019) |
| [0028](0028-web-client-rendering.md) | Rendering of the web client: GPUI on `wasm32` as a hybrid with a DOM layer, conditional on a browser run (proposed, not decided) |
| [0029](0029-mobile-rust-core-native-ui.md) | Mobile apps: one Rust core with UniFFI, native interfaces, keys in the platform store; App Store rules on running code checked, partly open |
| [0030](0030-ci-strategy.md) | CI strategy: one full check per change on the merge result, on the server; passed trees remembered; authors run the targeted check |
| [0031](0031-project-files-never-grant-agents.md) | Project files never grant agents anything: database permissions are the user's, kept outside the project and bound to the connection's target; every changing effect of a tool has a right, held by a test | accepted |
| [0032](0032-source-available-licence.md) | Source-available licence: PolyForm Shield 1.0.0; third-party parts keep their licences; contributions need an inbound grant | accepted (supersedes 0002, amends 0006; open questions for a lawyer before publication) |
