#!/bin/sh
# Runs the checks of .github/workflows/ci.yml on this machine. Fallback when GitHub Actions is not
# available (e.g. the included minutes are used up); the output is the evidence for a merge.
#
#   ci-local.sh            all checks; database integration tests use the local reference databases
#   ci-local.sh --no-db    skip the database integration tests
#
# Reference databases: db.sh start of the reference environment (PostgreSQL :54329, MySQL :33069,
# MariaDB :33079 once built with db.sh install-mariadb), user idecraft with the passwords in
# ~/.config/idecraft/refdb-{postgres,mysql}.pass.
set -eu
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$*"; }

if [ "${1:-}" != "--no-db" ]; then
  export IDECRAFT_TEST_PG="${IDECRAFT_TEST_PG:-postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable}"
  export IDECRAFT_TEST_MYSQL="${IDECRAFT_TEST_MYSQL:-mysql://idecraft@127.0.0.1:33069/inventory?ssl-mode=DISABLED}"
  # Passwords of the reference databases (db.sh secure; vault entries idecraft-refdb-postgres and
  # idecraft-refdb-mysql): the tests read the files, the values never appear here.
  # secret_file VAR FILE: export VAR naming FILE when FILE exists and VAR is not set yet.
  secret_file() { if [ -s "$2" ] && [ -z "$(printenv "$1")" ]; then export "$1=$2"; fi; }
  secrets_dir="${XDG_CONFIG_HOME:-$HOME/.config}/idecraft"
  secret_file IDECRAFT_TEST_PG_PASSWORD_FILE "$secrets_dir/refdb-postgres.pass"
  secret_file IDECRAFT_TEST_MYSQL_PASSWORD_FILE "$secrets_dir/refdb-mysql.pass"
  secret_file IDECRAFT_TEST_MARIADB_PASSWORD_FILE "$secrets_dir/refdb-mysql.pass"
  # The MySQL server's own CA certificate (public), for the CA-file tests of SPEC-0016 v2.
  mysql_ca="${REFERENCE_ENV_DIR:-$HOME/ReferenceEnv}/db/mysqldata/ca.pem"
  if [ -r "$mysql_ca" ] && [ -z "${IDECRAFT_TEST_MYSQL_CA:-}" ]; then export IDECRAFT_TEST_MYSQL_CA="$mysql_ca"; fi
  # Oracle Free in the Lima VM (oracle.sh of the reference environment), when it runs.
  if [ -s "${XDG_CONFIG_HOME:-$HOME/.config}/idecraft/oracle-test.pw" ] && nc -z 127.0.0.1 1521 2>/dev/null; then
    export IDECRAFT_TEST_ORACLE="${IDECRAFT_TEST_ORACLE:-oracle://idecraft@127.0.0.1:1521/FREEPDB1}"
    export IDECRAFT_TEST_ORACLE_PASSWORD_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/idecraft/oracle-test.pw"
  fi
  # SQL Server in the Lima VM (mssql.sh of the reference environment), when it runs.
  if [ -s "${XDG_CONFIG_HOME:-$HOME/.config}/idecraft/mssql-test.pw" ] && nc -z 127.0.0.1 1433 2>/dev/null; then
    export IDECRAFT_TEST_SQLSERVER="${IDECRAFT_TEST_SQLSERVER:-sqlserver://idecraft@127.0.0.1:1433/inventory}"
    export IDECRAFT_TEST_SQLSERVER_PASSWORD_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/idecraft/mssql-test.pw"
  fi
  if [ -x "${REFERENCE_ENV_DIR:-$HOME/ReferenceEnv}/db/mariadb-12.3.3/bin/mariadbd" ]; then
    export IDECRAFT_TEST_MARIADB="${IDECRAFT_TEST_MARIADB:-mariadb://idecraft@127.0.0.1:33079/inventory?ssl-mode=DISABLED}"
  fi
fi

step "commit $(git rev-parse --short HEAD) on $(git rev-parse --abbrev-ref HEAD)"
step "cargo fmt --check";  cargo fmt --all --check
step "cargo clippy";       cargo clippy --workspace --all-targets -- -D warnings
step "cargo test";         cargo test --workspace
# SPEC-0008 behaviour 14: a release build ignores the test password folder (~1 min).
step "release build ignores IDECRAFT_TEST_SECRETS_DIR"
IDECRAFT_TEST_SECRETS_DIR=/nonexistent cargo test -q --release -p idecraft-database --lib -- \
  --ignored release_build_ignores_test_secrets_dir
# SPEC-0260: headless performance budgets on a release build (~1 min; fixtures and the result
# history live in ~/.cache/idecraft-bench). The window part runs nightly in scripts/ci-window.sh.
step "performance budgets"; cargo build -q --release -p idecraft-bench
"${CARGO_TARGET_DIR:-target}/release/idecraft-bench" run
# The GPUI binary links and starts without opening a window; window tests: scripts/ci-window.sh.
step "GPUI binary";        cargo build -q -p idecraft && "${CARGO_TARGET_DIR:-target}/debug/idecraft" --version
# SPEC-0282: the macOS app bundle from that debug binary, ad-hoc signed and checked (~5 s, no
# window); the release bundle: scripts/bundle-macos.sh.
if [ "$(uname -s)" = Darwin ]; then
  step "macOS app bundle";  scripts/bundle-macos.sh --debug --no-build
fi
# M8.2: every fuzz target briefly and a short soak of `idecraft mcp` (docs/testing/HARDENING.md).
step "fuzz smoke";         scripts/fuzz.sh smoke 2
step "soak (2 min)";       scripts/soak.sh 2
step "docs check";         scripts/check-docs.sh
if command -v cargo-deny >/dev/null 2>&1; then
  step "cargo deny";       cargo deny check advisories licenses bans sources
else
  step "cargo deny skipped (not installed: cargo install cargo-deny)"
fi
step "all checks passed"
