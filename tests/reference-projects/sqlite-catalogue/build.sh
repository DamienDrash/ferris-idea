#!/usr/bin/env bash
# Builds the SQLite sample databases for the catalogue checks of F-051/F-052 (SPEC-0013, SPEC-0089,
# SPEC-0108) into a new folder, the same in every run, so that two IDEs can open identical files.
#
#   tests/reference-projects/sqlite-catalogue/build.sh <target-folder> [--tables N]
#
# The target folder must not exist yet or be empty. Needs the `sqlite3` command line (3.37 or later
# for STRICT tables, with FTS5 and R-Tree). Nothing here is a credential; all names and rows are
# invented.
#
#   catalogue.db        every catalogue feature (catalogue.sql)
#   attached.db         a second file to ATTACH as "archive" (attached.sql)
#   empty.db            a valid database without any object
#   zero.db             a file of zero bytes (SQLite reads it as an empty database)
#   wal.db              a small database in WAL journal mode
#   many.db             N tables (default 3000), each with an index, every tenth with a foreign key,
#                       every hundredth with a view
#   locked.db, lock.sh  ./lock.sh 60 holds a write lock on locked.db for 60 seconds
#   readonly/ro.db      a read-only file in a read-only folder (rollback journal)
#   readonly/ro-wal.db  the same in WAL mode, without its -wal and -shm files
#   notadb.db           a text file that is not a database
#   broken-view.db      two tables and two views, one view over a table that was dropped
#   (missing.db)        is deliberately not created
#   .idecraft/connections.toml  IDECraft profiles for all of them (open <target-folder> as project)
#
# Before deleting the folder: chmod -R u+w <target-folder>
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
target="${1:?usage: build.sh <target-folder> [--tables N]}"
tables=3000
if [ "${2:-}" = "--tables" ]; then tables="${3:?--tables needs a number}"; fi
case "$tables" in '' | *[!0-9]*) echo "--tables needs a number" >&2; exit 1 ;; esac

command -v sqlite3 >/dev/null || { echo "sqlite3 not found" >&2; exit 1; }
if [ -e "$target" ] && [ -n "$(ls -A "$target" 2>/dev/null)" ]; then
    echo "target folder is not empty: $target" >&2
    exit 1
fi
mkdir -p "$target/readonly" "$target/.idecraft"
target="$(cd "$target" && pwd)"

sqlite3 "$target/catalogue.db" < "$here/catalogue.sql"
sqlite3 "$target/attached.db" < "$here/attached.sql"
sqlite3 "$target/empty.db" "vacuum;"
: > "$target/zero.db"
sqlite3 "$target/wal.db" "pragma journal_mode = wal; create table note (id integer primary key, body text); insert into note (body) values ('one');" > /dev/null
sqlite3 "$target/locked.db" "create table held (id integer primary key, body text); insert into held (body) values ('row');"
printf 'This is a text file, not a SQLite database.\n' > "$target/notadb.db"
sqlite3 "$target/broken-view.db" "create table base (id integer primary key, v text); create table other (id integer primary key, v text); create view on_base as select id, v from base; create view on_other as select v from other; drop table base;"

# The large database, in one transaction.
awk -v n="$tables" 'BEGIN {
    print "begin;"
    for (i = 1; i <= n; i++) {
        name = sprintf("t_%05d", i)
        cols = "id integer primary key, name text not null, amount numeric default 0, created text default current_timestamp"
        if (i % 10 == 0) {
            cols = cols sprintf(", parent_id integer references t_%05d (id) on delete cascade", i - 1)
        }
        printf "create table %s (%s);\n", name, cols
        printf "create index %s_name on %s (name);\n", name, name
        if (i % 100 == 0) {
            printf "create view v_%05d as select id, name from %s where amount > 0;\n", i, name
        }
    }
    print "commit;"
}' | sqlite3 "$target/many.db"

sqlite3 "$target/readonly/ro.db" "create table kept (id integer primary key, body text not null); create index kept_body on kept (body); insert into kept (body) values ('do not change');"
sqlite3 "$target/readonly/ro-wal.db" "pragma journal_mode = wal; create table kept (id integer primary key, body text not null); insert into kept (body) values ('do not change');" > /dev/null
rm -f "$target/readonly/ro-wal.db-wal" "$target/readonly/ro-wal.db-shm"
chmod 444 "$target/readonly/ro.db" "$target/readonly/ro-wal.db"
chmod 555 "$target/readonly"

cat > "$target/lock.sh" << 'LOCK'
#!/usr/bin/env bash
# Holds a write lock (an open exclusive transaction) on locked.db for the given seconds.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
seconds="${1:-60}"
case "$seconds" in '' | *[!0-9]*) echo "usage: lock.sh <seconds>" >&2; exit 1 ;; esac
sqlite3 "$here/locked.db" << SQL
begin exclusive;
insert into held (body) values ('locked');
.shell sleep $seconds
rollback;
SQL
LOCK
chmod +x "$target/lock.sh"

profile() { # id, name, path, extra lines
    printf '\n[[connection]]\nid = "%s"\nname = "%s"\ndialect = "sqlite"\npath = "%s"\n%s' "$1" "$2" "$3" "${4:-}"
}
{
    printf '[idecraft]\nkind = "connections"\nformat = 1\n'
    profile catalogue "catalogue" "catalogue.db"
    profile catalogue-ro "catalogue read-only" "catalogue.db" $'read_only = true\n'
    profile with-archive "with archive" "catalogue.db" \
        "$(printf '\n[connection.options]\nstartup_script = "attach database %s%s/attached.db%s as archive"\n' "'" "$target" "'")"
    profile empty "empty" "empty.db"
    profile zero "zero bytes" "zero.db"
    profile wal "wal" "wal.db"
    profile many "many" "many.db"
    profile locked "locked" "locked.db"
    profile ro-file "read-only file" "readonly/ro.db"
    profile ro-wal "read-only wal file" "readonly/ro-wal.db"
    profile notadb "not a database" "notadb.db"
    profile broken-view "broken view" "broken-view.db"
    profile missing "missing" "missing.db"
} > "$target/.idecraft/connections.toml"

echo "built in $target"
