# ADR-0011: Native driver per database behind one session interface

- **Status:** accepted · 2026-10-31 · refines the driver row of `docs/research/TECH_EVALUATION.md` ·
  extended by the same rule to MariaDB (`mysql_async`), SQL Server (`tiberius`, `SPEC-0116`) and
  Oracle (`oracle` over the Instant Client, `SPEC-0142`)
- **Context:** The evaluation picked `sqlx` and named `tokio-postgres`, `mysql_async` and `rusqlite`
  as alternatives behind the same driver trait. A database tool must show values of *any* column type
  as text (including types the tool does not know), report errors with SQLSTATE and the database's
  error position (IMP-0008), and keep one outcome per statement (IMP-0009). With `sqlx`, decoding
  unknown types to text needs per-type code, and the PostgreSQL error position is not exposed in a
  portable way.
- **Decision:** One native, permissively licensed driver per database, each behind the same session
  interface in `ferris-idea-database` (connect from a SPEC-0008 profile, run a script into SPEC-0007
  outcomes). PostgreSQL first with `tokio-postgres` 0.7 (MIT OR Apache-2.0): its simple query protocol
  returns every value as text and its errors carry SQLSTATE and position. MySQL (`mysql_async`) and
  SQLite (`rusqlite`) follow with the same interface.
- **Consequences:** Three dependencies instead of one; each driver is small and tested against the
  reference databases. TLS is added per driver; until then profiles that require TLS are refused
  instead of silently connecting without it.
