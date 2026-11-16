# SPEC-0142: Oracle connections through Instant Client

- **Version:** 1 · **Date:** 2026-11-02
- **Sources:** SPEC-0135 (Oracle without a connection), STD (Oracle Call Interface through ODPI-C
  as wrapped by the crate `oracle` 0.6, UPL-1.0 OR Apache-2.0; EZConnect `//host:port/service`
  and `tcps://`; data dictionary views `user_tables`, `user_tab_columns`, `user_constraints`,
  `user_cons_columns`, `user_indexes`, `user_ind_columns`; `EXPLAIN PLAN SET STATEMENT_ID … FOR`
  into `PLAN_TABLE`; `sys_context('USERENV', 'NETWORK_PROTOCOL')`), the project owner's approval of
  2026-11-02 (Instant Client may be used), SPEC-0005, SPEC-0008, SPEC-0016, SPEC-0018, SPEC-0108,
  ADR-0008
- **Feature matrix rows:** F-060

## Purpose
Connect to Oracle databases like to the other systems: run scripts, browse the schema, read plans,
honour read-only profiles. Oracle's client library (Instant Client) is not shipped: the user
installs it, IDECraft loads it at runtime.

## Behaviour
1. **Client library:** loaded once, on the first Oracle connection, from `IDECRAFT_ORACLE_CLIENT`,
   else the newest `~/.local/opt/instantclient_*` holding `libclntsh`, else the system search path.
   If none loads, Test Connection and Connect say so and where to install it. [STD]
2. **Connecting:** `oracle://user@host:port/SERVICE` (SPEC-0135) becomes `//host:port/SERVICE`;
   SSL mode require or verify-full becomes `tcps://`. The password comes from the keychain as for
   every profile. Test Connection reports the server version and whether the session is
   encrypted (`NETWORK_PROTOCOL = tcps`). [STD, SPEC-0108]
3. **Scripts:** statements and PL/SQL blocks split as in SPEC-0135 run one by one; result sets,
   update counts and errors with `ORA-nnnnn` and the error position. Autocommit after each DML
   statement unless a transaction was started with `SET TRANSACTION`; `COMMIT` and `ROLLBACK`
   go through the driver. Blocking calls run off the UI thread. [STD, SPEC-0005]
4. **Read-only profiles** refuse DML, DDL and anonymous PL/SQL blocks (`BEGIN`/`DECLARE`) before
   they reach the server; queries run. `BEGIN` counts as a schema change for Oracle and, unless
   `TRAN`/`TRANSACTION`/`DISTRIBUTED` follows, for SQL Server too. [SPEC-0016]
5. **Catalog:** tables, columns with type, nullability and default (trimmed), primary keys,
   foreign keys and indexes of the user's schema from the `user_*` views. [STD, SPEC-0018]
6. **Plans:** Explain Plan writes into `PLAN_TABLE` under the statement id `IDECRAFT`, reads the
   tree back (operation with options, object, cost, rows, predicates) and deletes its rows.
   Explain Analyze is refused with a reason (Oracle needs a different mechanism). [STD]
7. **MCP:** the existing database tools work for Oracle profiles; no new tool. [ADR-0008]

## Acceptance criteria
- **AK-1** Unit tests: connect strings with and without TLS, refusals for read-only profiles,
  `BEGIN` classification for Oracle and SQL Server, plan parsing (`idecraft_database` unit tests).
- **AK-2** Live tests against Oracle Database Free 23 in a container
  (`oracle.sh start` of the reference environment; `crates/database/tests/it/oracle.rs`): a script with a
  PL/SQL block, commit and rollback, an error with code and position, the catalog with keys,
  defaults and indexes, a plan; a read-only profile runs a query and refuses DML and a block.
  `scripts/ci-local.sh` runs them when the database answers.
- **AK-3** Window self-test: the connection dialog for Oracle reaches the driver and reports the
  unreachable address `127.0.0.1:1`.
- **AK-4** Hand test (round 4, no. 68): connection dialog against the container, schema tree,
  a script, Explain Plan, a read-only profile.

## Out of scope
Shipping Instant Client, Oracle Wallet and LDAP/TNS names, Kerberos, Explain Analyze
(`DBMS_XPLAN.DISPLAY_CURSOR`), other schemas than the user's, packages and their bodies in the
tree, `DBMS_OUTPUT`.

## Known deviations from the reference
The reference ships a JDBC driver on demand; IDECraft loads the user's Instant Client. Not
compared side by side yet (hand test 68).
