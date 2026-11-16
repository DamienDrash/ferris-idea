# SPEC-0011: SQLite session — open and run scripts

- **Version:** 2 · **Date:** 2026-11-13 (v2: the file is read when it is opened, errors say what to do,
  read-only at the level of SQLite, a WAL database where nothing can be written; v1 2026-10-31)
- **Sources:** STD (SQLite C interface: open flags, column count, `changes()`, storage classes, extended
  result codes, `sqlite3_error_offset`), GEN, ADR-0011, SPEC-0009 (same session contract)
- **Feature matrix rows:** F-050, F-051

## Purpose
Open an SQLite database file from a connection profile and run SQL scripts with the same contract as the
PostgreSQL and MySQL sessions.

## Behaviour
1. The session opens the profile's file. A file that does not exist is refused with an error naming the
   path; nothing is created. Creating a new database is a separate, explicit action. [GEN]
2. A read-only profile opens the file read-only; changes fail. [STD]
3. Scripts are split with SPEC-0005 (SQLite rules). A statement with result columns yields a result set,
   otherwise the number of changed rows. [STD]
4. Values by storage class: NULL → none, INTEGER → decimal, REAL → shortest text that reads back as the
   same number, TEXT → as is, BLOB → `0x` followed by hex digits. [STD]
5. Errors carry the message prefixed with the SQLite result code name (e.g. `SQLITE_ERROR`); SQLite has
   no SQLSTATE. When SQLite reports the byte offset of the error in the statement, it becomes the 1-based
   character position of SPEC-0007. [STD, IMP-0008]
6. Statements run off the async runtime's worker threads, so a long statement does not block other work. [GEN]

7. **v2 – Opening:** the path is given to SQLite as a file name, never as a URI, so a name cannot
   carry open options. A read-only profile opens with SQLite's read-only flag: every change fails in
   SQLite itself (`SQLITE_READONLY`), whatever the statement is (`PRAGMA`, `VACUUM`, `ATTACH`ed
   files included), and no journal is written. After opening, the schema table is read once; a
   failure there is the connect error, in words the user can act on with SQLite's result code after
   it (SPEC-0108 v8 behaviour 16). A statement that fails later because the database is locked says
   so and that it can be tried again. [STD: `sqlite3_open_v2` flags, result codes]
8. **v2 – WAL where nothing can be written:** a database in WAL mode needs a `-shm` file next to it
   even for reading. When that cannot be created (a read-only file in a read-only folder, a read-only
   volume) and no `-wal` file with content lies next to the database, the file is opened as unchanging
   (`immutable=1`): it is read without locks and without any file being created, and every change
   fails. With content in a `-wal` file the connect fails and says that the folder must be writable,
   because the database file alone would be out of date. In a folder that can be written, reading a
   WAL database lets SQLite create its `-wal` and `-shm` files as every SQLite client does. [STD: SQLite
   WAL documentation, read-only databases; URI parameter `immutable`]

## Acceptance criteria
Tests build a temporary database from the reference seed files (`tests/reference-projects/sql-basic`).
- **AK-1** `select count(*), sum(delta) from movement` gives `10000` and `-30`.
- **AK-2** Two SELECTs give two result sets in order.
- **AK-3** `select no_such_column from movement` fails with `SQLITE_ERROR` in the message, and the script
  offset points at `no_such_column`.
- **AK-4** Inserting three rows into a new table gives update count 3.
- **AK-5** With a read-only profile, an update fails and the file is unchanged.
- **AK-6** `select x'ff00', 0.1, 1e300, null` gives `0xff00`, `0.1`, `1e300`, none.
- **AK-7** A profile pointing to a missing file is refused, naming the path, and no file appears.

- **AK-8 (v2)** A WAL database, read-only in a read-only folder, without `-wal` and `-shm`: tables and
  rows are read with a read-only and with a writing profile, an insert fails, the folder still holds
  only the database file with the same bytes
  (`crates/database/tests/it/sqlite_catalogue.rs`
  `a_wal_database_in_a_read_only_folder_is_read_without_side_files`, Unix).
- **AK-9 (v2)** On a read-only connection `CREATE TABLE`, `INSERT`, `PRAGMA user_version = 7` and
  `VACUUM` fail with `SQLITE_READONLY` and the folder is unchanged
  (`reading_the_schema_changes_nothing_on_disk`); the connect errors of SPEC-0108 AK-19.

## Out of scope
Creating databases, attaching further files (a startup script may `ATTACH`), WAL configuration,
extensions, encryption, a choice of the time SQLite waits for a lock (five seconds).

## Known deviations from the reference
- A missing file is not silently created (common behaviour of SQLite clients: a typo creates an empty
  database). Recorded as a general improvement; not yet observed in the reference.
