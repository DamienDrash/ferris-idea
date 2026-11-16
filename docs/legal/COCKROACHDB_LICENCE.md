# CockroachDB Software License — accepted for development and tests

Record of PO-STATUS question 24. It is not legal advice.

| | |
|---|---|
| **Licence** | CockroachDB Software License (Cockroach Labs). The text carries no version number or date; the copy read is the one published at `https://www.cockroachlabs.com/cockroachdb-software-license/` on 2026-11-05. It covers CockroachDB 24.3.0 and later. |
| **Software** | CockroachDB v26.3.5, official image `cockroachdb/cockroach:v26.3.5` |
| **Decision** | The project owner, 2026-11-05: "Wenn nichts dagegenspricht können wir zum entwickeln und testen die Lizenz annehmen" |
| **Accepted on** | 2026-11-05, by the agent running the first live test on the project owner's behalf |
| **Scope** | Development and automated tests of IDECraft on the project owner's Mac (Lima VM `idecraft-docker`) only |
| **Not in scope** | Production use, any deployment outside the test machine, shipping CockroachDB with IDECraft, benchmarks for publication |

## What accepting means here

The licence lets everyone operate the software without a licence key and without sharing telemetry
only for **Limited Non-Production Use**. Its definition, quoted in part: design, prototyping,
testing or development "in non-production, internal development environments", either during the
introductory period or "in deploying a database that runs solely on a single node". Our use meets
both alternatives: it is an internal test environment, and the server runs as one node.

So the acceptance comes with these conditions, which `connectors.sh` of the reference environment enforces:

1. **No account, no licence key.** Nobody registers with Cockroach Labs and no key is entered. Keys
   and accounts are the project owner's job; if a key ever becomes necessary, the tests stop and the question
   goes back to him.
2. **One node.** The server starts with `cockroach start-single-node`. Its own log confirms the state:
   `license updated: type is none … telemetry required: false` and `disable all license enforcement`.
3. **Short-lived.** Every `connectors.sh start cockroachdb` and `connectors.sh test cockroachdb`
   removes the old container and starts a fresh one; `test` removes it again when the test ends. A
   cluster without a key also has a seven-day grace period from initialisation; no test container
   lives that long.
4. **No outbound network.** The server sits only on the internal Docker network
   `idc-crdb-internal`, which has no gateway. Before each start the script runs a probe container on
   that network and refuses to start CockroachDB if the probe can resolve a public name or reach a
   public address. A socat container publishes only the SQL port on `127.0.0.1:26257`. The server
   cannot send telemetry or diagnostics anywhere (ADR-0007). The diagnostics setting is also off
   (`COCKROACH_SKIP_ENABLING_DIAGNOSTIC_REPORTING=true`).
5. **Insecure mode on loopback.** No certificates and no passwords. The server holds only the
   tables the live test creates and drops.

## Why blocking egress is consistent with the licence

The licence forbids circumventing its "Protective Measures", which include the telemetry
requirement. It also says that the software must share telemetry *except for Limited Non-Production
Use*. In that use there is no telemetry requirement, so cutting the network circumvents nothing.
The server confirms this itself with `telemetry required: false`. If a future version of the
licence or the server requires telemetry for single-node test clusters, the precondition fails.
CockroachDB then goes back to Beta and the question goes back to the project owner.

## Effect on the product

The CockroachDB connector is `status = "supported"`, tested against v26.3.5. IDECraft talks to it
through the PostgreSQL wire protocol (`tokio-postgres`); no CockroachDB code is linked into, shipped
with or downloaded by IDECraft. Users who connect IDECraft to their own CockroachDB are bound by
their own licence with Cockroach Labs, not by this record.
