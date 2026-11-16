# ADR-0014: Principles for multi-agent orchestration

- **Status:** proposed · 2026-10-31 · planned for milestones M12–M15; no implementation before QG-1
- **Context:** The long-term goal is several agents of different providers, plus people, working in
  parallel on one or more projects under a human or AI Project Owner ([vision](../architecture/AI_NATIVE_VISION.md) §3–§6).
  Such systems fail in predictable ways: models change state they should only propose, rights grow with
  roles, agents spawn agents without limit, parallel changes break each other after merging, and knowledge
  produced by a model is later treated as fact.
- **Decision:**
  1. **Identity, role, capability profile and grant are separate concepts.** Roles are data; any identity
     can hold any role; roles imply default grants that need confirmation, never system rights. The
     Project Owner role grants no administration, secret access or budget change by itself.
  2. **The task graph and scheduler are deterministic Rust code** with a fixed state transition table and
     guards. Models submit proposals through the capability layer; they cannot set states, skip guards or
     grant rights.
  3. **Isolation by default:** one worktree, branch and runtime environment per agent session; isolated
     test databases; integration only through a merge queue that tests the merged state.
  4. **Communication is persisted structured records** (assignment, status, question/answer, blocker,
     review/change request, handoff, decision, completion), independent of any model's context window.
  5. **Bounded delegation:** maximum hierarchy depth, maximum agents per participant, cost budgets split on
     delegation; agents create agents only with an explicit grant.
  6. **Knowledge has provenance:** origin, version, validity and confidence on every stored relation;
     model statements are hypotheses until verified.
  7. **One security model** for people and agents: least privilege, declared effects, approvals, audit,
     budgets, rate limits, emergency stop.
- **Consequences:** More upfront modelling (state tables, schemas) in exchange for reproducible and
  auditable runs. Open choices (storage engine, container runtime, budget accounting per provider) are
  decided at the start of the respective milestone.
