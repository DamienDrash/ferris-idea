# SPEC-0351: Project cockpit (headless)

- **Version:** 1 · **Date:** 2026-11-19
- **Sources:** ADR-0034 decision 8 (the cockpit: tiles with value, definition, sources, "not
  enough data"; the DORA keys; code and build KPIs) and decisions 5 and 6 of its open questions
  (no hosted services and no advisory download without the user); ADR-0007 (no telemetry: every KPI
  is computed on the machine, nothing is sent); ADR-0008 (every feature has an MCP tool);
  SPEC-0088 v3 behaviour 9 (the test history); SPEC-0098 (Ferris IDEA's inspections); `GEN` (DORA
  keys as the DevOps research programme defines them; the OSV format for advisories; LCOV and
  Cobertura for coverage). No observation of the reference: the reference has no project cockpit.
- **Feature matrix rows:** none (ROADMAP M20.4, after 1.0; owner's order of 2026-11-18).

## Purpose
One answer to "how is this project doing", computed on the machine from data that is already there:
the last test runs, Ferris IDEA's inspections, coverage reports, the advisories of a local copy against
the lockfiles, and Git (commits, open branches, deployments as tags). Every number carries its
definition, its sources and the time they were read. Where the data does not support a number, the
tile says so instead of guessing. v1 has no window view; the tool answers the tiles.

## Behaviour

### Answer
1. The cockpit is computed `as_of` a time (seconds since the Unix epoch, default now) over a period
   of `period_days` (1 to 365, default 30) ending then. Records later than `as_of` are left out, so
   the same data and the same `as_of` give the same answer. [GEN]
2. The answer lists the tiles asked for (`tiles`, default all of behaviour 3), each with:
   - `id` and `title`;
   - `status`: `ok`, or `not_enough_data` with a `reason`;
   - `values`: named numbers, times and lists;
   - `definition`: what each value means;
   - `sources`: what was read, each with `kind`, a `path` or name, and the `time` of the data (the
     newest record used, a file's modification time, or `as_of` for what was computed at the call).
   The order is fixed, and lists are sorted. [ADR-0034]
3. The tiles of v1 are `tests`, `build`, `coverage`, `inspections`, `dependencies`, `git` and
   `delivery`. `not_available` names the KPIs of ADR-0034 that v1 does not compute, each with the
   reason:
   - flow KPIs and incidents wait for the tickets of ROADMAP M20.1/M20.2;
   - open pull requests need a connected hosting service, which is off by default (ADR-0034
     question 5);
   - agent KPIs wait for the session records of M19.10;
   - database KPIs are not in v1.
   [ADR-0034]
4. Nothing is sent over the network and nothing is recorded about the call (ADR-0007). The tool
   reads; it writes no file. [ADR-0007]

### Code and build
5. `tests`:
   - Source: the test history of this project in the user's settings folder (SPEC-0088 v3
     behaviour 9). A file counts when its name is the hash of this project and its configuration.
   - Values: per configuration the last run in the period (time and the counts passed, failed,
     errors, skipped); the runs in the period; the green share of the last 20 runs across all
     configurations (green: no failure and no error); the time since the last green run.
   - Without a run in the period: `not_enough_data`. Flaky tests need the commit of each run, which
     the history does not keep; they are not counted.
   [SPEC-0088, ADR-0034]
6. `build`: Ferris IDEA keeps no record of builds yet, so the tile is `not_enough_data`. It says so
   and points to the test runs and to a configured CI as later sources. [ADR-0034]
7. `coverage`:
   - Source: the newest coverage report of these files in the project: `lcov.info`,
     `coverage/lcov.info`, `coverage.lcov`, `target/coverage/lcov.info`, `coverage.xml`,
     `cobertura.xml`, `coverage/cobertura-coverage.xml`.
   - Line coverage in per cent with two decimals. LCOV sums `LF` and `LH`. Cobertura reads
     `line-rate` of the root `coverage` element.
   - The report's modification time is the source's time. Without a readable report:
     `not_enough_data`.
   [GEN, ADR-0034]
8. `inspections`:
   - Ferris IDEA's own inspections over the project's sources, as `inspect_code` runs them
     (SPEC-0098), at the call.
   - Values: problems by level and the ten inspection ids with the most problems (ties by id),
     with `checked_at` = `as_of`.
   - Inspections read the current files; they cannot look back to `as_of`.
   [SPEC-0098]
9. `dependencies`:
   - **Versions:** from the project's `Cargo.lock` (ecosystem `crates.io`) and `package-lock.json`
     (ecosystem `npm`, lockfile versions 2 and 3) at the root.
   - **Advisories:** from a local copy of OSV records in `<user cache>/osv/`: JSON files, one record
     each, in any subfolders; at most 100 000 files, each at most 1 MB. Ferris IDEA does not download
     this copy in v1; the user puts it there (ADR-0034 question 6).
   - **Match:** a package and version are affected when a record names its ecosystem and name, and
     the version is in `versions` or inside a `SEMVER` or `ECOSYSTEM` range of `introduced`,
     `fixed` and `last_affected` events.
   - **Values:** the affected packages with version, advisory ids and the first fixed version, and
     the counts by severity. Severity is `database_specific.severity`, else `unknown`.
   - **Untrusted text:** advisory texts (summary, details) are not answered; they are text from
     outside.
   - The newest record's `modified` time is the copy's date.
   - **No data:** without a copy or without a lockfile, `not_enough_data`.
   [ADR-0034, GEN, ADR-0023 rule 7]

### Git and delivery
10. `git`. Read from the repository through Ferris IDEA's Git library, never by running hooks:
    - the main branch: `main`, else `master`, else HEAD;
    - its commits in the period: count, authors, newest commit time;
    - the local branches whose tip is not reachable from the main branch: name and the time of
      its tip, oldest first, at most 50 named, all counted;
    - the remote-tracking branches not merged: count only.
    - The commit walk stops after 20 000 commits and says so.
    - Without a repository: `not_enough_data`.
    [GEN]
11. `delivery` (the DORA keys from Git):
    - **Deployment:** a tag matching `deploy_tags` (a pattern with `*`, default `v*`). Its time is
      the tag's creation time (tagger date, or the commit time for a lightweight tag).
    - **Deployment frequency:** deployments in the period per week, and the share of the period's
      weeks with at least one.
    - **Lead time for changes:** the median, over the commits first contained in a deployment in
      the period, of the deployment time minus the commit time. A commit is first contained in a
      deployment when no earlier deployment contains it; at most 20 000 commits per deployment.
    - **Failed deployment:** a deployment is a rollback when its commit is an ancestor of the
      commit of the deployment before it. That earlier deployment counts as failed.
    - **Change failure rate:** failed deployments in the period divided by the period's
      deployments.
    - **Failed deployment recovery time:** the median from a failed deployment to the next
      deployment.
    - **Limits:** without incidents (tickets, M20.2) failures are rollbacks only, and the tile
      says so. Fewer than two deployments up to `as_of` give `not_enough_data`.
    [ADR-0034, GEN]

### MCP
12. MCP `project_cockpit` with `as_of`, `period_days`, `tiles` and `deploy_tags` answers
    behaviours 2 and 3. It reads only and needs no right. Unknown tiles and values out of
    bounds are refused. [ADR-0008]

## Acceptance criteria
- **AK-1** `crates/mcp/tests/it/cockpit.rs`: `tests_coverage_and_inspections`:
  - a test history of two configurations, with a run after `as_of` left out;
  - green share and the time since the last green run;
  - an LCOV and a Cobertura report;
  - inspections counted by level;
  - an empty project gives `not_enough_data` with reasons;
  - the `build` tile is always `not_enough_data`.
- **AK-2** `cockpit.rs`: `dependencies_from_a_local_advisory_copy`: `Cargo.lock` and
  `package-lock.json` matched against OSV records with `versions`, `SEMVER` and `ECOSYSTEM` ranges
  and `last_affected`. An unaffected version is not listed, and a fixed version is named. No
  advisory text is answered.
- **AK-3** `cockpit.rs`: `git_and_delivery_from_a_repository`: a repository built in the test:
  - commits in the period;
  - an unmerged branch;
  - four `v*` tags with one rollback, giving frequency, lead time, change failure rate and
    recovery time with their exact values;
  - fewer than two tags give `not_enough_data`.
- **AK-4** `cockpit.rs`: `the_tool_is_deterministic_and_reads_only`:
  - two calls with the same `as_of` answer the same;
  - every tile has a definition and sources with times;
  - unknown tiles and `period_days` out of bounds are refused;
  - the workspace and the settings folder are unchanged.

## Out of scope
- **Window:** the window view, tile choice kept per project, the time series, and thresholds with
  hints.
- **Tickets:** flow KPIs and incidents (M20.1/M20.2).
- **Connected services:** hosted CI, pull requests and deployments (M20.5, ADR-0034 question 5).
- **Advisory copy:** downloading the OSV copy (ADR-0034 question 6).
- **Agents and databases:** agent KPIs (M19.10, M19.7) and database KPIs.
- **Exports:** CSV, Markdown and PDF (M20.6).

## Known deviations from the reference
None: the reference has no project cockpit.
