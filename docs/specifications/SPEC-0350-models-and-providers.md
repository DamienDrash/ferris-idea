# SPEC-0350: Models and providers for agents

- **Version:** 1 · **Date:** 2026-11-19
- **Sources:** `GEN` (agent tools keep a list of model providers with an endpoint and the models
  they offer, choose a model per task, find local model servers on the usual ports, and add up
  costs from the token counts a provider reports against a price list the user keeps); ADR-0017
  (credentials stay with the provider's program or in the system keychain); ADR-0023 (Ferris IDEA as
  agent harness); ADR-0008 (every feature has an MCP tool); SPEC-0138 (AI Assistant: endpoint,
  keychain key, no redirects); SPEC-0143 (provider templates); SPEC-0313 behaviour 6 (secret
  patterns); SPEC-0342 (scheduler: tasks, budgets); SPEC-0053 (agent rights). No observation of the
  reference: the reference has no provider register for agents.
- **Feature matrix rows:** none (ROADMAP M19.7, harness phase after 1.0; owner's order of
  2026-11-18).

## Purpose
Agents run on models from different providers: a cloud API with a key, or a local server.
Ferris IDEA keeps one register of the providers the user set up, knows which model each task uses,
finds local servers without starting or downloading anything, and adds up what the runs cost from
the token counts the providers report. Keys never leave the keychain except toward their own
endpoint, and Ferris IDEA never invents a price.

## Behaviour

### Register
1. The register is `providers.toml` beside the user's grant file (the user's settings folder),
   never in a workspace; a register path inside the workspace is refused. A missing file is an
   empty register. The file is written atomically with mode 0600. [GEN, ADR-0023]
2. A provider has `id` (1 to 64 characters of `a-z`, `0-9`, `-`, `_`), `kind`, `endpoint` (the
   base URL of its API), `key`, `models` (the model names known for it, at most 500, each 1 to
   200 characters without control characters; empty means not listed yet) and `enabled` (default
   true). At most 32 providers. Unknown keys and values out of bounds are refused with the file
   named; read tools then show the problem and change tools refuse until it is fixed. [GEN]
3. `kind` is one of:
   - `openai`: an OpenAI-compatible API;
   - `anthropic`: Anthropic's API (its own headers next to `Authorization`, SPEC-0138);
   - `ollama`: a local Ollama server;
   - `llamacpp`: a local llama.cpp server or another local OpenAI-compatible server.
   [GEN, SPEC-0143]
4. `endpoint` is an `https` URL, or an `http` URL whose host is a loopback address (`127.0.0.0/8`,
   `::1`, `localhost`). It has no user name, password, query or fragment. `ollama` and `llamacpp`
   are local only: their endpoint must be on a loopback address. A server on another machine is
   kind `openai` with `https`. Ferris IDEA never follows a redirect to another address (SPEC-0138).
   [GEN, rule 12]
5. `key` is `none` or `keychain`. With `keychain`, the API key is the system keychain's entry of
   the endpoint: the same entry the AI Assistant's settings page writes (service `ferris-idea-ai`,
   account host and port). A key is never written to the register, the usage ledger, an answer, a
   log or an error. Error messages of an endpoint have the key and anything that looks like a
   secret masked. `vault:` references are refused until the vault of ROADMAP M19.15 exists. No
   tool takes a key as an argument. [ADR-0017, SPEC-0138, rule 5]
6. No field of the register may hold text that looks like a secret (SPEC-0313 behaviour 6): such
   a value is refused, and the refusal does not repeat it. [SPEC-0313, rule 5]

### Local servers
7. Detection asks the loopback address only, on the default ports 11434 (Ollama) and 8080
   (llama.cpp server), or on the ports given (at most 16). It sends `GET /api/tags`: an answer
   with `models` is an Ollama server. Otherwise it sends `GET /v1/models`: an answer with `data`
   is a `llamacpp` server. Each request has 2 seconds. It answers per server found the port, kind,
   endpoint (`http://127.0.0.1:<port>/v1`), the model names and whether a registered provider
   already uses that endpoint. Detection never starts a server, pulls or downloads a model, or
   changes the register. [GEN]

### Models
8. Listing a provider's models asks `GET <endpoint>/models` (SPEC-0138), with the key only when
   `key` is `keychain`, and only toward that endpoint. Listing changes nothing; the names can then
   be written to the provider's `models`. [GEN, SPEC-0138]
9. An assignment gives a task a provider and a model:
   - `task` is 1 to 64 characters of `a-z`, `0-9`, `-`, `_`, `.`; the task `default` applies to
     every task without one of its own;
   - the provider must exist and be enabled;
   - when the provider lists models, the model must be one of them.

   Assigning again changes the choice. The assignment keeps its last 20 earlier choices with the
   time of each change. The model of a task is its own assignment, else `default`, else none.
   [GEN]
10. Removing a provider that an assignment uses is refused, and the refusal names those tasks.
    Removing a provider removes its prices. [GEN]

### Costs
11. Prices are configuration in the register, never built in. A price row has `provider`, `model`
    (or `*` for every model of that provider; an exact model wins), `currency` (three capital
    letters, default `USD`), and `input_per_mtok` and `output_per_mtok`: decimal numbers as text
    with at most 6 decimal places, from 0 to 1 000 000, per million tokens. Without a matching row
    the price is unknown. [GEN]
12. A usage record books one call. It has the time, the provider, the key reference (`none`, or
    `keychain:<host>:<port>`; never the key), the model, the task (default `default`), the input and
    output tokens as the provider reported them (each 0 to 10⁹), and the cost. The cost is computed
    once at booking from the price then in force, in millionths of the currency; with no price it
    stays unknown. Changing a price later does not rewrite earlier records. [GEN]
13. The usage ledger is `model-usage.jsonl` beside the register: one record per line, written
    atomically with mode 0600, at most 20 000 records. Beyond that the oldest records are dropped,
    and summaries say how many were dropped. [GEN]
14. A summary groups the records since a given time by `provider` (default), `key`, `model` or
    `task`. Per group it has the calls, input and output tokens, the cost per currency (as text
    with six decimal places), and the calls without a known price. It never guesses a missing
    price. [GEN]

### MCP
15. `model_providers` answers the providers (with `key` as `none` or `keychain`, never a key),
    the prices, the assignments with their history, the file paths, the problems and the bounds.
    It needs no right. [ADR-0008]
16. `model_provider_set` adds, replaces or removes a provider, `model_price_set` sets or removes a
    price row, and `model_assign` sets or removes a task's assignment. Each is a dry run by
    default that answers what would be written; `dry_run=false` needs the `settings` right.
    [ADR-0008, SPEC-0053]
17. `model_list` lists a provider's models (behaviour 8). It needs the `http` right unless the
    endpoint is on a loopback address. `model_detect_local` detects local servers (behaviour 7)
    and needs no right. [SPEC-0053]
18. `model_usage` answers a summary (behaviour 14) and needs no right. `model_usage_record` books a
    record (behaviour 12) for a registered provider; it is a dry run by default that answers the
    computed cost, and `dry_run=false` needs the `settings` right. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** `crates/ai/tests/it/providers.rs`: `register_and_its_bounds`: a full register read and
  written back (mode 0600); unknown keys, ids, kinds, plain-http remote endpoints, remote
  endpoints for local kinds, endpoints with user info or query, `vault:` keys, secret-looking
  values (not repeated in the refusal), too many models and prices out of bounds are refused; a
  register path inside the workspace is refused.
- **AK-2** `providers.rs`: `local_servers_are_detected_only`: a fake Ollama server and a fake
  llama.cpp server on loopback ports are found with their kinds and models. A closed port is
  skipped. Nothing is written.
- **AK-3** `providers.rs`: `models_and_assignments`: listing models sends the keychain key only
  to its endpoint (no key with `none`). An endpoint error that repeats the key is masked. An
  assignment, its change with history, the `default` fallback and an unknown model are covered.
  Removing a provider in use is refused.
- **AK-4** `providers.rs`: `costs_from_reported_tokens`: a booking with an exact price, with a `*`
  price and without a price; a price change leaves earlier records unchanged; summaries by
  provider, key, model and task; the ledger bound.
- **AK-5** `crates/mcp/tests/it/models.rs`: `model_tools`: the tools against a fake loopback
  server. Changes are refused without `settings` and written with it. `model_list` on loopback
  needs no right. No answer holds a key.

## Out of scope
Starting or stopping Ollama or llama.cpp and downloading models (the user does that; ROADMAP M19.7
names it for later); booking a run's tokens automatically from the agent host (SPEC-0348, M19.5)
or the AI Assistant's chat; budgets per key (M19.6); subscriptions through the providers' programs
(ADR-0017, SPEC-0160, unchanged); the vault (M19.15); a settings page in the window.

## Known deviations from the reference
None: the reference has no provider register for agents.
