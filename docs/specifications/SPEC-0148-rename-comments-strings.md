# SPEC-0148: Rename in comments, strings and text files

- **Version:** 5 · **Date:** 2026-11-13 (v5: a path is judged by the names it has on disk and the
  fixed folder names count in any spelling of upper and lower case, behaviour 8, AK-10, AK-11; v4: a
  file Git tracks is the project's own although an ignore rule matches it, behaviour 7, AK-9; v3: only the project's own sources are searched and
  changed, OBS-0034 L14; v2: a renamed Java type takes its file along; hand test Nr. 74, D29)
- **Sources:** GEN (IDE renames offer to also change the old name in comments and string
  literals and in non-code files, show those places for review because they are only text
  matches, and let single ones be left out before anything changes; F-072), OSS (the options
  "Search in comments and strings" and "Search for text occurrences" of the rename refactoring,
  `JetBrains/intellij-community`), SPEC-0027 (syntax highlighting marks comments and strings),
  SPEC-0040, SPEC-0074, IMP-0017, ADR-0008
- **Feature matrix rows:** F-072

## Purpose
Keep comments, messages, documentation and configuration in step with a renamed symbol, without
blindly replacing a word that may mean something else there.

## Behaviour
1. **Options:** the rename popup (⇧F6) shows "Search in comments and strings" (⌥C) and "Search
   for text occurrences" (⌥T) under the name field. They start as the last rename left them;
   at first comments and strings are on, text occurrences off. [GEN, OSS]
2. **Occurrences:** after the language server has planned the code usages, the old name is
   searched as a whole word (no letter, digit, `_` or `$` directly before or after it) in the
   project's own sources (behaviour 7; without binary files and files over 2 MiB): with the first option inside comments and string literals of code files, as the
   syntax highlighting marks them; with the second anywhere in text files (Markdown, XML, YAML,
   JSON, TOML and files without a code language). Places the language server already changes
   are not listed again. Open files are searched in their editor text, unsaved changes included.
   [GEN]
3. **Preview:** if there are such occurrences, a preview lists them under the count of code
   usages: kind (comment or string, text), file and line, the line's text. All are included;
   Space or a click includes or excludes the selected one, ↑/↓ move. Enter renames ("Do
   Refactor"), Escape cancels and nothing changes. Without such occurrences the rename is
   applied at once as before. [GEN]
4. **Applying:** code usages and included occurrences are applied together: open files through
   their editors (one undo step per file), others on disk. The status line says how many of the
   edits were in comments, strings or text files. [SPEC-0074]
5. **MCP:** `rename_symbol` takes `search_comments_and_strings` and `search_text_occurrences`
   (both off by default, so earlier callers see no change) and returns the extra places as
   numbered `occurrences` (path, kind, range, line text); `exclude` leaves the given numbers
   unchanged when applying. [ADR-0008]

6. **A Java type takes its file along (v2, D29):** renaming a class, interface, enum or record in
   the `.java` file named after it (`Lager` in `Lager.java`) also renames the file (`Speicher.java`),
   because Java wants a public top-level type in the file of its name and jdtls renames only the
   type; the build would break otherwise. Not when the target file exists. In the window an open
   editor of the file follows with its tab, text, undo steps and breakpoints; the status bar adds
   "the file is now Speicher.java". The rename is part of the refactoring's undo group (SPEC-0193):
   one ⌘Z takes back the text of every file and the file name, ⇧⌘Z does both again. MCP
   `rename_symbol` lists it as `renamed_files` (from, to) and `undo_refactoring` reverses it. [GEN, STD]

7. **Only the project's own sources (v3, OBS-0034 L14):** a rename never searches for text
   occurrences outside the project's own sources and never changes a file there without being
   told to. Before v3 the search took every file of the workspace except a short list of folder
   names (the list of Find in Files, SPEC-0029: `.git`, `.hg`, `.svn`, `.idecraft`, `target`,
   `node_modules`, `__pycache__`); in a Python project with a `.venv` all 27 listed occurrences
   lay in installed packages and were preselected. [OBS, GEN]
   - **Not the project's own** is a file that lies outside the project folder, is reached through
     a symbolic link, or has a folder above it (below the project folder) that is one of:
     - version control data: `.git`, `.hg`, `.svn`, `.jj`, `CVS`;
     - IDE data: `.idecraft`, `.idea`;
     - a virtual environment or interpreter: `.venv`, `venv`, `site-packages`, `.tox`, `.nox`,
       `__pypackages__`, and any folder that holds a `pyvenv.cfg` or a `conda-meta` folder,
       whatever its name;
     - installed dependencies: `node_modules`, `bower_components`, `.gradle`, `Pods`, `.bundle`,
       and `vendor` when the folder next to it holds `go.mod`, `composer.json`, `Gemfile` or
       `Cargo.toml`, or `vendor` itself holds `modules.txt` or `autoload.php`;
     - build output and caches: `target`, `__pycache__`, `.mypy_cache`, `.pytest_cache`,
       `.ruff_cache`, and `build`, `out`, `dist` unless a folder named `src` lies above them (a
       package may be called `build`);
     - excluded by the project model: an excluded folder of a content root or the compiler output
       in `.idea` (SPEC-0048);
     - ignored: the folder, or the file itself, matches the ignore rules of Git — the `.gitignore`
       files from the repository's top folder down to the file's folder, `.git/info/exclude` and
       the user's global ignore file, with Git's precedence and negation, **unless Git tracks the
       file** (v4): a file in Git's index (added with `git add -f`, or matched by a rule written
       after it was added, also by `.git/info/exclude`) is the project's own, and a folder an
       ignore rule matches is entered for the tracked files in it, while its other files stay
       ignored. Tracking does not outweigh the other reasons: a tracked file in a virtual
       environment, in installed dependencies, in build output, in a folder the project model
       excludes or behind a symbolic link stays outside. The rules and the index are read from the
       files (the index of the work tree, also of a linked work tree or a submodule, decoded by
       `gix-index`, at most 1 GiB, a split index joined); no `git` process is started. When the
       index cannot be read (no repository, a damaged file, another hash than SHA-1), nothing
       counts as tracked and the rules alone decide, as in v3.
     [GEN, STD]
   - **When unsure, left out (rule 12):** a folder that cannot be read is skipped, and a folder named like
     build output counts as such. The cost is an occurrence that has to be changed by hand; the
     other way round a rename would rewrite files nobody looked at.
   - **Text occurrences** (behaviour 2) are searched only in the project's own sources. Files
     elsewhere are not read and not listed.
   - **Code usages:** edits the language server returns for a file that is not the project's own
     (a method of the same name inside `.venv`, a generated file) are not part of the rename.
     They are listed apart from it, per file with the number of usages and the reason ("in a
     virtual environment", "ignored by .gitignore", …), and are not included; each file can be
     included on purpose. Exception: the file the rename was started in is always part of the
     rename — the user chose it by renaming there.
   - **Preview:** the preview of behaviour 3 also opens when there are only such files. Their
     rows stand after the occurrences, start unticked and are toggled like the others; the
     summary line counts them apart. With none of either kind the rename is applied at once.
   - **MCP:** `rename_symbol` returns them as `outside_project` (index, path, reason, edits,
     included) and leaves them out of `plan`; `include_outside` takes the indices to apply too.
     Without it nothing outside the project's own sources is written. [ADR-0008]
   - **One rule for all:** the decision lives in one place (`idecraft_core::OwnSources`) so that
     other project-wide actions can use it. Find in Files and Replace in Files keep their
     behaviour in this version (SPEC-0029 is not changed here).

8. **The names on disk decide, not the spelling (v5; decision of the coordinator of 2026-11-13
   under the Project Owner's general authority, security first, CLAUDE.md rule 12).** Until v4 the
   rule of behaviour 7 compared folder names letter for letter. On a file system that ignores upper
   and lower case (the default on macOS and Windows) `Node_Modules/x.js`, `.VENV/lib/x.py` and
   `.GIT/config` reach the real files, and the rule called them the project's own. Everything that
   asks the rule with a path it was given was affected: Rename, Find and Replace in Files, Move,
   the SQL rename, Inspect Code, Endpoints, Search Everywhere, the reference index and the plugin
   API. Listings were not: they take the names from the disk. Since v5:
   - **Resolved first:** before the rule looks at a path, the names of its folders and of the file
     are replaced by the names they have on disk, as far as the path exists without a symbolic link
     on the way (the resolved path the system reports). Another case, a short name of Windows
     (`NODE_M~1`) and another Unicode form of a name all become the one name on disk. The part of a
     path that does not exist is judged as it is written. The answer of "which path may be read"
     (`OwnSources::readable`) names the file as it is on disk too. [GEN, STD]
   - **Fixed names in any case, on every platform:** the names of behaviour 7 for version control
     data, IDE data, virtual environments, installed dependencies, build output and caches, and
     `vendor`, match whatever the upper and lower case, also on a file system that tells `Target`
     from `target`. Two ways were open: ask each project's file system and compare by its answer,
     or always ignore case for these names. The second was chosen: it is the stricter one, it
     cannot be wrong where the question to the file system is (a folder with its own setting, a
     volume mounted inside the project), and a repository that is checked out on macOS and on Linux
     gets the same answer on both. The cost falls under "when unsure, left out": on a file system
     that keeps case apart, a folder of the project's own that is called `Build`, `Out`, `Dist`
     (outside `src`), `Target` or `Pods` is now left out, as it already is on macOS and Windows; a
     place named on purpose still wins (SPEC-0029 behaviour 8). Letters that are an upper or lower
     case of another letter only in one direction (`ſ`, `ı`) are folded with it. [GEN]
   - **Ignore rules as Git reads them there:** Git matches ignore rules without regard to case when
     `core.ignorecase` is set, which it sets itself when it creates or clones a repository on a
     file system that ignores case. IDECraft asks the file system the same question once per
     project folder: the folder is looked up under its own name with the case of every ASCII letter
     flipped, and if that is the same entry (same device and inode; on Windows the same resolved
     path), the file system ignores case. A name without such a letter passes the question to the
     folder above; with none at all, the platform's usual answer is taken (macOS and Windows
     ignore case). Where it does, ignore rules match whatever the case (a rule `Logs/` covers a
     folder `logs`), and the files Git tracks, the folders the project model excludes and the `src`
     folder above `build`, `out` and `dist` are compared the same way. Where it does not, these
     four compare exactly, as before: there `Logs` and `logs` are two folders. The value of
     `core.ignorecase` in the repository's settings is not read: what a path reaches is decided by
     the file system, not by a setting a repository can carry. [STD, GEN]
   - **Not covered:** the question is asked for the project folder. A folder inside the project
     that treats case differently from the project folder (a setting per folder on Windows or
     ext4, another volume mounted there) gets the project folder's answer for ignore rules and
     tracked files; resolving and the fixed names hold there too.
   - There is no option.

## Acceptance criteria
- **AK-1** Whole-word matching, comments and strings only in code (Rust, Python), workspace
  search that skips excluded folders and planned edits and uses editor text, merging into the
  plan (`rename_text::tests`).
- **AK-2** The preview counts code usages and includes every occurrence until it is excluded (`RenamePreview::apply` through the self-test, AK-4).
- **AK-3** MCP with clangd: without options no occurrences; with text occurrences two places in
  a Markdown file, one excluded, the other renamed with the code (`code_insight_tools`).
- **AK-4** Window self-test with clangd: ⇧F6 shows the default options, ⌥T switches text
  occurrences on, the preview lists two Markdown places and the comment, Space leaves the Markdown
  places out, Enter renames the code and the comment only.
- **AK-6 (v2)** With jdtls over MCP: `Lager` → `Speicher` renames `Lager.java` to `Speicher.java`
  and the use in `App.java`; `undo_refactoring` restores both texts and the name (protocol test
  `renaming_a_java_class_renames_its_file`); which files go along (unit test
  `rename::tests::a_java_type_takes_its_file_along`); undo and redo of a group with a renamed file
  (`undo_group::tests::renamed_files_follow_undo_and_redo`); a tab keeps its place
  (`tabs::tests::a_renamed_file_keeps_its_tab`).
- **AK-5** Hand test (round 4, no. 74): renaming a Java class and a Python function with
  comments, strings and a README next to the reference.

- **AK-7 (v3, OBS-0034 L14)** In a project with `.venv/lib/python3.12/site-packages/x.py`,
  `node_modules/y.js`, a file ignored by `.gitignore` and an own file, each with the old name in a
  comment, only the own file's occurrence is found; a folder with a `pyvenv.cfg` under another
  name, a `vendor` folder next to `go.mod`, an excluded folder of the project model and a negated
  ignore rule are decided as behaviour 7 says (`own_sources::tests`,
  `rename_text::tests::occurrences_only_in_the_projects_own_sources`). Edits of the plan for files
  that are not the project's own are split off with their reason, the file the rename started in
  stays (`rename_text::tests::usages_outside_the_own_sources_are_split_off`).
- **AK-9 (v4)** With a real Git repository in a temporary folder: a file added with `git add -f`
  although `*.log` ignores it, a file in a folder `logs/` that `.gitignore` ignores, and a file
  `.git/info/exclude` names, all added, are the project's own and listed; their untracked
  neighbours in the same places stay ignored and are named as left out; a tracked file in `.venv`
  stays in the virtual environment; a project folder below the top of the work tree sees its
  tracked files relative to itself; with a damaged or empty index the rules alone decide
  (`own_sources::tests::files_git_tracks_are_own_although_ignored`).
- **AK-10 (v5)** The comparison of names, on every platform: the fixed names in other cases
  (`Node_Modules`, `.GIT`, `.Idea`, `.VENV`, `TARGET`, `Build`, and `ı` or `ſ` in place of `i` or
  `s`), names that only look alike are not matched; by path alone `Node_Modules/x.js`,
  `.VENV/lib/x.py`, `.GIT/config`, `.Idea/workspace.xml`, `app/Target/debug/x` and `BUILD/x.js` are
  not the project's own while `src/build/x.js` is; a folder `Bower_Components` and a folder `.Hg`
  on disk are not entered by a listing or by the reference index's list, and `.Hg` not by a search
  for everything (`own_sources::tests::fixed_names_count_in_any_case`).
- **AK-11 (v5)** Where the temporary folder's file system ignores case (the test says so and ends
  where it does not): with `node_modules`, `.venv`, `.git`, an ignored folder `logs` under a rule
  written `Logs/`, a file ignored by a rule in another case and an own `src/Main.py` on disk,
  the spellings `Node_Modules/x.js`, `NODE_MODULES/X.JS`, `.VENV/lib/x.py`, `.GIT/config`,
  `LOGS/APP.TXT` and `LIB/BUILD/x.py`, relative and absolute, get the reason of the folder on disk;
  `SRC/main.PY` is the project's own, is named `src/Main.py`, and a listing below `SRC` lists it
  (`own_sources::tests::another_spelling_is_judged_by_the_name_on_disk`).
- **AK-8 (v3)** MCP with a Python language server: in a project with the old name in comments of
  an own file, of a package in `.venv/…/site-packages`, of a file in `node_modules` and of an
  ignored module that also uses the function, `rename_symbol` with both options lists one
  occurrence (the own file), returns the ignored module's code usages under `outside_project`
  with the reason and not in `plan`, leaves all three files unchanged when applying, changes the
  ignored module with `include_outside` and refuses an index that is not listed
  (`protocol.rs::rename_leaves_files_outside_the_own_sources_alone`).

## Out of scope
Using the same rule in Find in Files, Replace in Files and the file watcher (v3: they keep their
own lists for now). Qualified names in strings (`com.acme.Shop`) beyond the simple name, renaming files that carry
the old name, the reference's further options (rename tests, overloads, variables of the
renamed type), grouping the preview in the Find tool window.

## Known deviations from the reference
- Matches are whole words in comments and strings as the syntax highlighting sees them; the
  reference also matches inside some identifiers of non-code text.
- The preview is a dialog listing only the non-code places; the code usages from the language
  server are counted, not listed.
- v5: fixed folder names match in any case also where the file system keeps case apart; the
  reference was not compared for it.
- v3: the reference was not compared for behaviour 7 (rename could not be driven reliably there,
  OBS-0034 item 48). Since v4 a tracked file that matches an ignore rule is the project's own, as
  Git itself treats it.
