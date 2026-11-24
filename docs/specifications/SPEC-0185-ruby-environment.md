# SPEC-0185: Ruby runtimes, Bundler and the Ruby language server

- **Version:** 2 · **Date:** 2026-11-15 (v2: AK-8)
- **Sources:** GEN (IDEs pick a Ruby interpreter per project, respect the project's version file and
  Bundler, and start the language server with that Ruby; F-038), STD (version-manager layouts:
  rbenv `$RBENV_ROOT/versions/<v>`, rvm `$rvm_path/rubies/ruby-<v>`, chruby `~/.rubies/<name>` and
  `/opt/rubies/<name>`, asdf `$ASDF_DATA_DIR/installs/ruby/<v>`, mise
  `$MISE_DATA_DIR/installs/ruby/<v>`; version files `.ruby-version`, `.tool-versions`, `mise.toml` /
  `.mise.toml` `[tools]`, the Gemfile's `ruby` directive; Bundler's `Gemfile.lock` sections `GEM`,
  `PATH`, `GIT` with `specs:`, `RUBY VERSION`, `BUNDLED WITH`; RubyGems requirement operators `=`,
  `!=`, `>`, `<`, `>=`, `<=`, `~>`; `bundle exec`; user gem folder `~/.gem/ruby/<v>/bin`, `GEM_HOME`;
  Ruby LSP (`ruby-lsp`, Shopify) and Solargraph (`solargraph stdio`) command lines), SPEC-0042
- **Feature matrix rows:** F-038

## Purpose
Know which Ruby a project wants and which installed Ruby serves it, read Bundler's lockfile, and start
the Ruby language server with that Ruby and, where the project pins the server in its bundle, through
Bundler — without starting Ruby to find out.

## Behaviour
1. **Runtimes** are found from a configured Ruby (executable or installation folder), rbenv, rvm,
   chruby, asdf and mise installations (each newest first) and `ruby` on `PATH`; duplicates (same
   resolved file) are dropped. Each runtime names how it was found. [STD]
2. **Version** without starting Ruby: from the version manager's folder name (a leading `ruby-` is
   dropped), else from a version-like folder in the resolved path of the executable (e.g. a package
   manager's `ruby/3.3.6/bin/ruby`), else unknown. [STD]
3. **Requested version:** `.ruby-version` (a leading `ruby-` dropped), else `.tool-versions`
   (`ruby <v>`), else `mise.toml`/`.mise.toml` (`ruby = "<v>"` under `[tools]`), else the Gemfile's
   `ruby "<requirement>"`. Every source found is listed; sources that contradict each other (an exact
   version from one outside the requirement of another) are problems. [STD, GEN]
4. **Matching:** a plain version matches as prefix by segments (`3.3` matches `3.3.6`, not `3.30.0`);
   a requirement uses RubyGems operators, `~> 3.2` meaning `>= 3.2, < 4` and `~> 3.2.1` meaning
   `>= 3.2.1, < 3.3`; several comma-separated requirements must all hold. [STD]
5. **Selection:** configured; else the newest managed runtime matching the requested version; else the
   first `ruby` on `PATH` (it may be a version manager shim that honours the version file itself);
   else the newest managed runtime. The reason is reported. A requested version no installed runtime
   matches is a problem. [GEN]
6. **Bundler:** whether `Gemfile` and `Gemfile.lock` exist; the locked gems with versions (the
   four-space-indented `name (version)` lines of the `specs:` blocks); the lockfile's Ruby and Bundler
   versions. A `Gemfile` without lockfile is noted ("run bundle install"). [STD]
7. **Server search path:** the selected runtime's `bin` folder, then `$GEM_HOME/bin`, then
   `~/.gem/ruby/<major.minor.0>/bin` of the selected version, then `PATH`. The Ruby language server and
   `bundle` are looked up there, and the server process gets this search path as its `PATH`, so scripts
   starting with `#!/usr/bin/env ruby` run with the selected Ruby. [STD]
8. **Server through Bundler:** a candidate of SPEC-0042 whose gem (`ruby-lsp`, `solargraph`) is in
   `Gemfile.lock` starts as `bundle exec <program> <args>` when `bundle` is found, so the project's
   pinned version is used; locked candidates come before unlocked ones, since the project chose
   them. The others (`ruby-lsp`, then `solargraph`) start as found on the search path. A gem in the
   lockfile with no `bundle` found falls back to the program on the search path, and the error says
   so when that is missing too. [STD, GEN]
9. MCP tool `ruby_environment` (optional `ruby`: executable or installation folder to use instead of
   the automatic choice): runtimes, selection with reason, requested versions, Bundler state, locked
   gems, the server command line that would start (or why none can) and problems. [ADR-0008]

## Acceptance criteria
- **AK-1** With fake rbenv, rvm, chruby, asdf and mise folders and a fake `PATH`: all runtimes are
  found with version and origin, newest first per manager, duplicates dropped.
- **AK-2** `.ruby-version` `ruby-3.2` selects the newest 3.2.x managed runtime with a reason naming the
  file; without a version file the `PATH` Ruby wins; a configured Ruby wins over everything; a request
  nothing matches is a problem and the fallback is reported.
- **AK-3** Requirements: `~> 3.2`, `~> 3.2.1`, `>= 3.1, < 3.4`, `!= 3.3.0` and plain prefixes match as in
  behaviour 4; `.ruby-version` 3.3.0 against Gemfile `ruby "~> 3.2.0"` is a problem.
- **AK-4** `Gemfile.lock` with `GEM`, `PATH` and `GIT` blocks: locked gems with versions (dependency
  lines ignored), Ruby and Bundler versions; a `Gemfile` without lockfile gives the note.
- **AK-5** Server resolution with crafted folders: `solargraph` in the lockfile and `bundle` in the
  runtime's `bin` gives `bundle exec solargraph stdio` with the runtime's `bin` first on the server's
  `PATH`, even when an unlocked `ruby-lsp` is usable; without the lockfile the program itself, and
  `ruby-lsp` before `solargraph` when both are usable; nothing usable gives an error with both
  install hints.
- **AK-6** The MCP tool returns runtimes, selection, Bundler state and the server command line for a
  workspace.
- **AK-7** With a real Ruby language server installed (optional, skipped cleanly otherwise): outline,
  hover and completion answer, once with ruby-lsp started directly (on a saved file, SPEC-0042 v6
  behaviour 13) and once with solargraph through `bundle exec` in a project that locks it (on
  unsaved text).
- **AK-8 (v2)** Window test with keys and real servers (`crates/workbench/tests/it/f038_window.rs`:
  `hand_test_110_bundle_lock_in_the_terminal_and_f1_through_bundle_exec`, skipped without Ruby,
  Bundler and solargraph): `bundle lock --local` typed into the terminal ends with exit 0, the server
  resolves to `bundle exec solargraph stdio`, and F1 on `Greeter` in the file opened again answers
  through it without a raw link.

## Out of scope
Installing Rubies or gems, running `bundle install`, Ruby debugging and test runners, RubyGems
platforms, Rails-specific features, reading versions by starting Ruby.

## Known deviations from the reference
The reference keeps a list of Ruby SDKs configured by the user; Ferris IDEA derives the choice from the
version managers and version files each time and reports why.
