# ADR-0005: Language and debug services via LSP and DAP

- **Status:** accepted · 2026-10-30
- **Context:** 14 target languages; writing analysers for each is infeasible.
  Mature, openly licensed language servers and debug adapters exist for all
  priority languages.
- **Decision:** Language intelligence (completion, diagnostics, navigation,
  most refactorings) comes from external language servers over LSP; debugging
  from debug adapters over DAP. Servers run as separate processes the user
  installs or the IDE downloads from their official sources; they are not
  bundled until their licenses are checked. SQL is the exception: SQL
  intelligence is built in (`database` crate), because it needs live schema
  information from the IDE's own connections.
- **Consequences:** Feature depth per language depends on the server; gaps are
  recorded in the feature matrix as "known deviations".
