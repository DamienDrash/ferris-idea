#!/bin/sh
# Runs the checks of .github/workflows/ci.yml on this machine. Fallback when GitHub Actions is not
# available (e.g. the included minutes are used up); the output is the evidence for a merge.
#
#   ci-local.sh            all checks; database integration tests use the local reference databases
#   ci-local.sh --no-db    skip the database integration tests
#
# Reference databases: db.sh start of the reference environment (PostgreSQL :54329, MySQL :33069,
# MariaDB :33079 once built with db.sh install-mariadb), user idecraft with the passwords in
# ~/.config/ferris-idea/refdb-{postgres,mysql}.pass.
set -eu
# Variables under the former name (IDECRAFT_*) still count while today's is not set. rebrand:keep
for former in $(env | sed -n 's/^IDECRAFT_\([A-Za-z0-9_]*\)=.*/\1/p'); do # rebrand:keep
  eval "[ -n \"\${FERRIS_IDEA_$former+x}\" ] || { FERRIS_IDEA_$former=\$IDECRAFT_$former; export FERRIS_IDEA_$former; }" # rebrand:keep
done
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$*"; }

if [ "${1:-}" != "--no-db" ]; then
  export FERRIS_IDEA_TEST_PG="${FERRIS_IDEA_TEST_PG:-postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable}"
  export FERRIS_IDEA_TEST_MYSQL="${FERRIS_IDEA_TEST_MYSQL:-mysql://idecraft@127.0.0.1:33069/inventory?ssl-mode=DISABLED}"
  # Passwords of the reference databases (db.sh secure; vault entries idecraft-refdb-postgres and
  # idecraft-refdb-mysql): the tests read the files, the values never appear here.
  # secret_file VAR FILE: export VAR naming FILE when FILE exists and VAR is not set yet.
  secret_file() { if [ -s "$2" ] && [ -z "$(printenv "$1")" ]; then export "$1=$2"; fi; }
  secrets_dir="${XDG_CONFIG_HOME:-$HOME/.config}/ferris-idea"
  [ -d "$secrets_dir" ] || secrets_dir="${XDG_CONFIG_HOME:-$HOME/.config}/idecraft" # rebrand:keep (the settings folder before its takeover, SPEC-0356)
  secret_file FERRIS_IDEA_TEST_PG_PASSWORD_FILE "$secrets_dir/refdb-postgres.pass"
  secret_file FERRIS_IDEA_TEST_MYSQL_PASSWORD_FILE "$secrets_dir/refdb-mysql.pass"
  secret_file FERRIS_IDEA_TEST_MARIADB_PASSWORD_FILE "$secrets_dir/refdb-mysql.pass"
  # The MySQL server's own CA certificate (public), for the CA-file tests of SPEC-0016 v2.
  mysql_ca="${REFERENCE_ENV_DIR:-$HOME/ReferenceEnv}/db/mysqldata/ca.pem"
  if [ -r "$mysql_ca" ] && [ -z "${FERRIS_IDEA_TEST_MYSQL_CA:-}" ]; then export FERRIS_IDEA_TEST_MYSQL_CA="$mysql_ca"; fi
  # Oracle Free in the Lima VM (oracle.sh of the reference environment), when it runs.
  if [ -s "$secrets_dir/oracle-test.pw" ] && nc -z 127.0.0.1 1521 2>/dev/null; then
    export FERRIS_IDEA_TEST_ORACLE="${FERRIS_IDEA_TEST_ORACLE:-oracle://idecraft@127.0.0.1:1521/FREEPDB1}"
    export FERRIS_IDEA_TEST_ORACLE_PASSWORD_FILE="$secrets_dir/oracle-test.pw"
  fi
  # SQL Server in the Lima VM (mssql.sh of the reference environment), when it runs.
  if [ -s "$secrets_dir/mssql-test.pw" ] && nc -z 127.0.0.1 1433 2>/dev/null; then
    export FERRIS_IDEA_TEST_SQLSERVER="${FERRIS_IDEA_TEST_SQLSERVER:-sqlserver://idecraft@127.0.0.1:1433/inventory}"
    export FERRIS_IDEA_TEST_SQLSERVER_PASSWORD_FILE="$secrets_dir/mssql-test.pw"
    # The test CA that `mssql.sh start` gives the server (public certificate, SPEC-0016 v4).
    mssql_ca="$secrets_dir/mssql-tls/ca.pem"
    if [ -r "$mssql_ca" ] && [ -z "${FERRIS_IDEA_TEST_SQLSERVER_CA:-}" ]; then export FERRIS_IDEA_TEST_SQLSERVER_CA="$mssql_ca"; fi
  fi
  if [ -x "${REFERENCE_ENV_DIR:-$HOME/ReferenceEnv}/db/mariadb-12.3.3/bin/mariadbd" ]; then
    export FERRIS_IDEA_TEST_MARIADB="${FERRIS_IDEA_TEST_MARIADB:-mariadb://idecraft@127.0.0.1:33079/inventory?ssl-mode=DISABLED}"
  fi
fi

step "commit $(git rev-parse --short HEAD) on $(git rev-parse --abbrev-ref HEAD)"
step "cargo fmt --check";  cargo fmt --all --check
step "cargo clippy";       cargo clippy --workspace --all-targets -- -D warnings
step "cargo test";         cargo test --workspace
# SPEC-0008 behaviour 14: a release build ignores the test password folder (~1 min).
step "release build ignores FERRIS_IDEA_TEST_SECRETS_DIR"
FERRIS_IDEA_TEST_SECRETS_DIR=/nonexistent cargo test -q --release -p ferris-idea-database --lib -- \
  --ignored release_build_ignores_test_secrets_dir
# SPEC-0260: headless performance budgets on a release build (~1 min; fixtures and the result
# history live in ~/.cache/ferris-idea-bench). The window part runs nightly in scripts/ci-window.sh.
step "performance budgets"; cargo build -q --release -p ferris-idea-bench
"${CARGO_TARGET_DIR:-target}/release/ferris-idea-bench" run
# The GPUI binary links and starts without opening a window; window tests: scripts/ci-window.sh.
step "GPUI binary";        cargo build -q -p ferris-idea && "${CARGO_TARGET_DIR:-target}/debug/ferris-idea" --version
# SPEC-0282: the macOS app bundle from that debug binary, ad-hoc signed and checked (~5 s, no
# window); the release bundle: scripts/bundle-macos.sh.
if [ "$(uname -s)" = Darwin ]; then
  step "macOS app bundle";  scripts/bundle-macos.sh --debug --no-build
fi
# M8.2: every fuzz target briefly and a short soak of `ferris-idea mcp` (docs/testing/HARDENING.md).
step "fuzz smoke";         scripts/fuzz.sh smoke 2
step "soak (2 min)";       scripts/soak.sh 2
step "docs check";         scripts/check-docs.sh
if command -v cargo-deny >/dev/null 2>&1; then
  step "cargo deny";       cargo deny check advisories licenses bans sources
else
  step "cargo deny skipped (not installed: cargo install cargo-deny)"
fi
step "all checks passed"
