# ADR-0007: No trial, licence checks, feature gates or telemetry

- **Status:** accepted · 2026-10-30
- **Context:** The project must not reproduce the reference product's
  commercial restrictions.
- **Decision:** IDECraft has no trial period, no activation, no account
  requirement, no feature tiers and no telemetry. Network access happens only
  for actions the user starts (e.g. fetching a language server) and is visible.
- **Consequences:** No licensing code anywhere; reviews reject it. The
  restrictions of the *reference IDE* still bind us while we observe it
  (RESEARCH_BOUNDARIES, section 4).
