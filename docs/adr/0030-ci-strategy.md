# ADR-0030: CI strategy: check the merge result once, on the server

- **Status:** accepted · **Version:** 2 · **Date:** 2026-11-16 (v2: the server checks batches of
  waiting branches itself, decision 10) · 2026-11-12 (v1) · decided by the coordinating agent
  under the Project Owner's order of 2026-11-12 to research a better CI strategy and implement it
- **Related:** the server CI script, `scripts/check-pr.sh`, `CLAUDE.md` rule 6,
  Project Owner's decision of 2026-11-11: no paid GitHub features, CI on own machines

## Context

Measured on 2026-11-12, a change took 45 to 90 minutes from finished code to main, almost all of it
waiting:

- Every change was checked three times in full: by the agent that wrote it (`cargo test
  --workspace`, 15 to 20 minutes on the Mac, after a cold build in a fresh worktree), by the
  coordinator after merging main into it (about 10 minutes), and by the server CI (10 to 19 minutes).
- The server CI checked the branch tip, not what main would be after the merge, so its result did
  not replace the coordinator's check; after the squash merge it checked main's identical tree again.
- A full server run on 8 cores: clippy 2 to 3 minutes, building and linking the tests 6 to 12
  minutes, running 2,630 tests 2.5 to 4 minutes. The build is dominated by about 180 test binaries,
  155 of them one integration test file each (languages 29, core 25, database 24), each linked
  against the whole crate graph.

Sources: the "not rocket science rule" and merge queues (test the merge result before it lands;
[Graphite on bors and merge queues](https://graphite.com/blog/bors-google-tap-merge-queue)),
[Delete Cargo Integration Tests](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests)
(one integration test binary per crate), [Fast Rust Builds](https://matklad.github.io/2021/09/04/fast-rust-builds.html),
[nextest: archiving and reusing builds](https://nexte.st/book/reusing-builds),
[Tips for faster Rust compile times](https://corrode.dev/blog/tips-for-faster-rust-compile-times/).

## Decision

1. **One full check per change, on the merge result.** The server CI checks every pushed branch as
   the merge of the branch into main's tip. A branch that does not merge cleanly fails at once.
   This is the check that decides a merge; nobody repeats it by hand.
2. **Passed trees are remembered.** Results are kept by commit and by tree. A tree that passed is
   not checked again, so main after a squash merge of a checked branch passes without a second
   run. A tree that differs from a passed tree only in documents (the commit that adds the pull
   request number to the changelog) gets the documents check alone.
3. **The author's check is the targeted one.** Before a commit, agents and the coordinator run
   `scripts/check-pr.sh`: format and lints for the whole workspace, tests of the changed crates and
   of the crates that depend on them. It finds most faults in minutes; the rest is the server's job.
4. **Main is still checked after every merge.** When main moved between a branch's check and its
   merge, the merged tree is new and gets a full run. That run is the safety net, not the gate; a
   red main is fixed first.
5. **Several ready branches may be checked as one.** The coordinator may push a branch `queue/<name>`
   that merges several ready branches; the server checks `queue/*` branches before the others.
   Merging those branches in the same order then gives a tree that already passed.
6. **Every step is timed.** The result line names the seconds per step (fmt, clippy, build, test,
   doctest, docs), so the next slow part is found from numbers.
7. **One integration test program per crate.** The files of `crates/X/tests/` are modules of one
   program `tests/it`; a file whose tests change process-wide state stays a program of its own
   (done on 2026-11-12, #578: 189 test binaries became 79, all 2,643 tests kept file and name).
8. **No compiler cache for the agents' worktrees on the Mac.** Measured on 2026-11-12 with sccache
   0.19.0 and two fresh worktrees: building all tests took 327 s while the cache filled and 295 s
   with it (654 hits, 717 misses, 389 calls that cannot be cached). The workspace's own crates and
   the linking are most of a cold build, and a cache helps neither enough; the targeted check
   (decision 3) and fewer test binaries (decision 7) shorten it instead. The server keeps its cache.

9. **Nothing waits for the check (2026-11-12, second step).** The author's check tests only the
   crates the change touches (`scripts/check-pr.sh --changed-only`; with a change in `crates/core`
   the dependents were 24 of 29 crates and the check took 24 minutes on the Mac). The author pushes
   and goes on with the next task. The merge script waits for the server's result of a
   pull request and merges it the minute it passed, so the time between "passed" and "merged" no
   longer depends on someone looking. On the server the check that decides a merge may be given
   priority over other builds (`env` beside the results).

10. **The server checks batches itself (v2, 2026-11-16).** With 13 pull requests waiting and 25 to
    30 minutes per full run the queue took hours. When the next commit is a branch, the server takes
    up to four waiting branch tips of its class (`IDECRAFT_CI_BATCH`, default 4; 1 switches batches
    off): branches that change documents only and the others are never mixed, the order is the one
    of decision 5 and the script's head (documents first, `queue/*`, newest tip first). The tips are
    merged one after the other onto main's tip, and the merge of all gets one check, as strict as a
    single one (the scope follows from everything the batch changes). A tip that does not merge with
    the ones before it is left out and waits for a later round; a tip that does not merge into main
    alone is checked alone and fails as before.
    - **Green:** each member gets its own result `success … batch=<id> members=<sha,…>`, so the merge
      script merges each pull request by itself as before. The batch's tree counts as passed
      (decision 2).
    - **Red:** the batch is halved and each half checked again, down to the tip that fails alone;
      only that tip gets `failure`. A half that passes gives its members `success` for that half.
      When both halves pass but the whole failed (two branches that fail only together), the second
      half gets no result and is checked again on main's next tip, so after the first half is merged
      it is checked on top of it.
    - **Newer push:** a newer push of any member stops the batch (as it stops a single check,
      `superseded` for that member); the others keep no result and wait for the next round.
    - **Cost and limit:** a green batch of four needs one run instead of four; one red tip among
      four needs at most five runs (the whole, two halves, two quarters), so batches pay while most
      branches pass, which is the usual case after the author's check (decision 3). A member is not
      checked on main alone. A pull request merged after the others of its batch is checked again
      as part of main (decision 4), which remains the safety net.
    A self-test of the server CI script checks a green batch, a red tip in the middle and a
    conflict on a throw-away remote. The running service is switched to a new copy of the script by hand after
    the merge, as for every change of it.

## Measurements

- First run of the new script, main at `456787b8`, with other builds running on the server
  (load 9 to 16 on 8 cores): 928 s; fmt 49, clippy 220, build 444, test 172, doctest 31, docs 6.
- Runs on the merge result before the test programs were joined: 829, 940 and 1079 s (build 450
  to 621 s).
- After (#578): the branch's run took 515 s; fmt 17, clippy 72, build 256, test 136, doctest 30,
  docs 2. Building the tests after a change in `crates/core`, warm build folder: 234.5 and 240.1 s
  before, 166.9 and 143.7 s after, under the same load.
- In use on 2026-11-12: main after a squash merge of a checked branch passed as `cached` in 0 to 1 s
  (five times), a documents-only change in 2 to 10 s.

## Consequences

- A change is checked in full once instead of three times; main's run after a squash merge takes
  seconds when main did not move in between.
- A fault in a crate the change does not touch is found by the server, some minutes later than
  before, and before the merge as before.
- The server is a single point: when it is not reachable, the full check runs on the Mac
  (`cargo test --workspace`) as before 2026-11-12.
- Security: nothing new is exposed. The check still runs as the unprivileged build user from the
  server's own clone, with the script installed from main by hand, never from the branch under test.

## Not decided here

A second runner. Batches of more than four tips, and choosing a batch's size from the load.
