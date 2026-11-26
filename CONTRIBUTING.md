# Contributing

Ferris IDEA has stricter rules than most projects, because it rebuilds the *functional scope* of a
proprietary product from observation and open sources. Read
[`docs/legal/RESEARCH_BOUNDARIES.md`](docs/legal/RESEARCH_BOUNDARIES.md) before anything else.
Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## What you can do today

- **Report a bug or ask a question:** open an issue; forms exist for both.
- **Send a reproduction:** a minimal project or request that shows a problem is the most useful
  thing you can give.
- **Report a hand-test result:** say which feature row or hand test you ran, on which system and
  commit, and what happened.
- **Report a security problem privately:** [SECURITY.md](SECURITY.md). Never in a public issue.

Do not attach code, help texts, icons or screenshots of other products to an issue.

## Before your first pull request

Code from outside the project is merged only after you have accepted the
[Contributor License Agreement](docs/legal/CLA.md) ([Licensing of contributions](#licensing-of-contributions))
and the checks of your pull request have passed. A pull request from a fork runs the public CI only
after a maintainer approved the run.

## The rules, when that changes

1. **Never** add code, decompiled output, icons, images, fonts, colour themes,
   help texts or screenshots from proprietary JetBrains products, or from
   anything else whose license you have not checked. Apache-2.0 material from
   `intellij-community` is allowed under rule 8.
2. Code is written against a specification in `docs/specifications/`. If none
   exists, write the spec first (separate commit, prefix `spec:`).
3. Every spec statement names its source type (`GEN`, `STD`, `OSS`, `DOC`, `OBS`).
4. Commits are prefixed by role: `obs:`, `spec:`, `impl:`, or `docs:`, `ci:`,
   `chore:` for everything else. One role per commit.
5. Each module names the spec it implements; each acceptance criterion is
   mapped to a test in `docs/specifications/TRACEABILITY.md` (new rows as fragments, rule 10).
6. Dependencies must pass `cargo deny check licenses`: the allow-list in `deny.toml` holds
   permissive licenses plus MPL-2.0 and three narrow exceptions.
7. Public-facing text follows [`docs/design/`](docs/design/README.md): English, the
   status words *Works / Experimental / In progress / Planned / Not planned*, no
   unsupported claims, real screenshots only. Run `scripts/check-docs.sh`.
8. Material ported from `JetBrains/intellij-community` (Apache-2.0) follows
   [`docs/legal/APACHE_PORTING.md`](docs/legal/APACHE_PORTING.md).
9. A user-visible change adds its lines as `changelog.d/<PR number>.md`, not in `CHANGELOG.md`
   ([changelog.d/README.md](changelog.d/README.md)).
10. Shared lists are not edited by parallel pull requests
   ([ADR-0035](docs/adr/0035-tool-registry-and-number-reservations.md)): a new MCP tool module
   keeps its router, its rows of the effects table and its open-files entries in its own file;
   traceability rows go to
   [`docs/specifications/traceability.d/`](docs/specifications/traceability.d/README.md); a new
   specification or ADR number is reserved first in
   [`docs/specifications/RESERVED.md`](docs/specifications/RESERVED.md)
   (`scripts/spec-number.sh next` prints the next free one).

## Checks

Before every commit:

```sh
scripts/check-pr.sh --changed-only
```

It runs the format check and the lints for the whole workspace (`cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`) and the tests of the crates the change
touches. A change to documents only runs `scripts/check-docs.sh` alone.

To test the whole workspace yourself:

```sh
scripts/check-pr.sh --full     # the same checks with `cargo test --workspace`
```

A cold run takes 15 to 20 minutes on a Mac, and many tests need external tools: language servers,
debug adapters, databases.

How branches of the project's own repository are checked before a merge (the server check, batch
merges, the window self-test and its groups) is described in
[docs/testing/CHECKS_AND_SELFTEST.md](docs/testing/CHECKS_AND_SELFTEST.md).

## Licensing of contributions

Ferris IDEA is open source under the [Apache License 2.0](LICENSE.md); notes on the terms are in
[LICENSING.md](LICENSING.md).

Contributions are accepted only under the
[Ferris IDEA Contributor License Agreement](docs/legal/CLA.md) (CLA), version 1.2. You keep the
copyright in your work. The CLA gives the licensor a license to ship it under any terms, now and
later, plus a patent license. You confirm that the work is yours to give, or that its source and
license are named.

To accept it, put this sentence in the description of your first pull request:

> I have read the Ferris IDEA Contributor License Agreement, version 1.2, and I accept it for all my
> contributions. — @your-login

The project records the acceptance in [docs/legal/CLA_SIGNATURES.md](docs/legal/CLA_SIGNATURES.md)
before it merges the pull request. If you contribute as part of your work, your employer may need to
accept the CLA as well (section 6 of the CLA). Material ported from Apache-2.0 sources keeps its
license and its header (rule 8).

## AI-assisted contributions

Allowed and expected: this project is developed by AI agents under the rules in
[AGENTS.md](AGENTS.md), with the project owner deciding approvals. Do not paste code or texts of
the reference product into prompts. Implementation prompts contain the specification only. You
answer for what you submit, whoever or whatever wrote it.
