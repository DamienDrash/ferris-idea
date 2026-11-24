# SPEC-0029: Find and replace in a file and in the workspace

- **Version:** 6 · **Date:** 2026-11-16 (v6: each MCP match has its end, behaviour 6a, AK-12, OBS-0071 G3; deviations IMP-0442 and IMP-0443); 2026-11-12 (v5: the host of a remote project searches by the same rule,
  decision of the Project Owner of 2026-11-12 for the remaining places; v4: a file Git tracks is searched although an ignore rule
  matches it, SPEC-0148 v4; v3: a replacement has the scope of its search; v2: Find in Files searches
  only the project's own sources; decision of the Project Owner of 2026-11-12)
- **Sources:** GEN (find/replace with match case, whole words and regular expressions; find in files with a
  file mask; replacement preview; F-015), STD (regular expressions as in the Rust `regex` crate: Perl-like
  syntax without look-around and back-references, linear time; `$1`/`${name}` in replacements),
  SPEC-0001 (workspace files), SPEC-0148 v3 behaviour 7 (the project's own sources)
- **Feature matrix rows:** F-015

## Purpose
Find text quickly and replace it safely, with a preview of every change before anything is written.

## Behaviour
1. A query has a pattern and the options regex (default off), match case (default off) and whole words
   (default off). Without regex the pattern is literal. An invalid regex is an error with the reason. An
   empty pattern finds nothing. [GEN, STD]
2. Whole words: a match counts only if the character before its start and the character after its end are
   not letters, digits or `_` (works for patterns that start or end with punctuation, unlike `\b`). [GEN]
3. A match reports char offsets, 0-based line and column (in chars) and the text of its first line.
   Matches do not overlap; with a regex they may span lines. [GEN]
4. Replacement: literal text, or with regex the capture references `$1`, `${name}` (`$$` for a dollar).
   The preview lists, per match, line, old text and new text, and the resulting file text; nothing is
   written. [GEN, STD]
5. Find in files searches workspace files (v1: SPEC-0001 exclusions; since v2 the project's own sources,
   behaviour 7) matching the file mask — comma-separated
   globs with `*` and `?` on the file name, e.g. `*.rs, *.toml`; files that look binary (a NUL byte in the
   first 8 KiB) or are larger than 10 MiB are skipped and counted. Results are sorted by path and
   limited to `max_matches` (default 1 000) with a "truncated" flag. [GEN]
6. MCP tools `search_in_files` and `replace_in_files_preview` (same options, plus replacement). [ADR-0008]
6a. **The end of a match (v6):** each match of `search_in_files` has, besides its path, 0-based `line`
   and `column` (characters) and the `line_text` of its first line, the 0-based `end_line` and
   `end_column` just after its last character, so a regular expression's match has a known extent; a
   match over a line break ends on a later line. An invalid pattern is an error that names the
   parser's reason, never an empty list. [GEN; OBS-0071 G3]

**v2 (decision of the Project Owner, 2026-11-12: "Find in Files shall leave out virtual environments such
as `.venv` and the other folders that are not the project's own sources, the same rule Rename got").**

7. **Only the project's own sources by default:** Find in Files and Replace in Files search the files
   that are the project's own by the rule of SPEC-0148 v4 behaviour 7 — not virtual environments,
   installed dependencies, build output and caches, version control and IDE data, folders the project
   model excludes, anything behind a symbolic link, anything the ignore rules of Git match unless Git
   tracks it (v3: a file added with `git add -f` is searched). One rule
   in one place (`OwnSources`); the list of seven folder names of v1 is part of it. [GEN; decision]
8. **Searching the rest on purpose:**
   - **A named place wins.** A search in a directory (or file) that is itself not the project's own
     searches every file below it: "in directory `.venv/lib`" finds what lies there. A directory that
     is the project's own is searched by the rule of behaviour 7 (an ignored folder below it stays
     out). A search in a given list of files (the files open in editors) searches those files
     whatever they are. A directory outside the project folder or behind a symbolic link finds
     nothing. [GEN; decision]
   - **The option "libraries and ignored files"** (MCP `include_non_project: true`, default off)
     searches every file below the project folder or the named directory, except version control
     data (`.git`, `.hg`, `.svn`, `.jj`, `CVS`) and Ferris IDEA's own data (`.ferris-idea`, which holds the
     local history's copies); symbolic links are still not followed. [GEN; decision]
9. **Replacing never leaves the own sources unasked:** a replacement changes a file that is not the
   project's own only with the option of behaviour 8; a named directory alone shows the matches
   there but does not allow changing them. Version control data, `.ferris-idea`, files behind a
   symbolic link and files outside the project folder are never changed, with or without the
   option. The preview (`replace_in_files_preview`) lists only files a replacement would change and
   names the others with the reason (`not_changed`). [GEN; decision]
10. **What was left out is said:** when the default search left folders or files out because they
   are not the project's own, the result carries them — the topmost folder or the single file each,
   with the reason, sorted; version control data and `.ferris-idea` are not counted (the option would
   not search them either), files the file mask does not match neither. One line names the count,
   e.g. `2 folders and 1 file not searched (libraries and ignored files)`. The MCP answer has
   `not_searched` (`folders`, `files`, up to 20 `paths` with `reason`, and `note` with that line and
   how to search them) — so nobody wonders about missing hits. [GEN; decision]
11. `search_in_files` and `replace_in_files_preview` take `directory` and `include_non_project`.
   [ADR-0008]

**v3 (fault found with v2: the window's Replace All replaced in the whole project whatever scope the
search had).**

12. **A replacement has the scope of its search:** whatever replaces in files takes its files and
   matches from the search itself (`search_scope` with the same query, file mask, place and option,
   narrowed by the same filters the caller's result list uses), run again at the moment of
   replacing — never from a second search by query and mask alone. A match outside the place, or one
   the search would not return any more, is not changed. Behaviour 9 applies on top. The window's
   Replace and Replace All follow this (SPEC-0066 v5 behaviour 17). `replace_in_files_preview`
   already did: its `directory` and `file_mask` go through the same scope as `search_in_files`, so a
   preview for a directory lists only files below it. The preview has no `context` argument; it
   writes nothing. [GEN; decision "security first"]

**v5 (decision of the Project Owner, 2026-11-12: the places v2 left as they were follow the same rule;
here the search of a remote project).**

13. **Remote projects search by the same rule:** the host's `search.text` (SPEC-0318 v8 behaviour 13)
   takes its files from the same listing as the local search (`search_candidates`), so behaviours 7,
   8 and 10 hold on the host: the project's own sources by default; a named folder that is not the
   project's own is searched completely; `include_non_project: true` searches every file except
   version control data and `.ferris-idea`; a list of files (the files open in editors) is searched
   whatever they are; a folder or file outside the project folder or behind a symbolic link finds
   nothing. The answer carries the line of behaviour 10 as `not_searched` when something was left
   out. Replace in Files stays unavailable for a remote project. [GEN; decision]
14. **A link anywhere on the way:** "behind a symbolic link" in behaviour 8 means a link at any part
   of the path to the named folder or file, also inside a folder that is not the project's own
   (`node_modules/pkg` being a link, or a link inside `.venv`): the place is not read and finds
   nothing. This holds for the local search, for the host and for the files the host is given by
   name. [decision "security first"]

## Acceptance criteria
- **AK-1** Literal and regex search, match case on/off, positions with non-ASCII text, multi-line regex.
- **AK-2** Whole words incl. patterns with punctuation at the edges; invalid regex error; empty pattern.
- **AK-3** Replacement with capture groups, named groups, `$$`, and literal replacement of `$1` without regex.
- **AK-4** Find in files: mask, exclusions, binary and oversize skipping, truncation, sorted output.
- **AK-5** The MCP tools return matches and previews and write nothing.
- **AK-6 (v2)** In a project with `.venv/lib/python3.12/site-packages/x.py`, `node_modules/y.js`, a file
  ignored by `.gitignore` and an own file, all holding the text searched: the default search finds only
  the own file and reports two folders and one file as not searched; with the option it finds all four
  and reports nothing left out; in directory `.venv/lib` it finds `x.py` without the option; `.git`
  and `.ferris-idea` are searched in neither case; a directory outside the project finds nothing.
- **AK-7 (v2)** A replacement is refused for a file that is not the project's own unless the option is
  set, and for version control data, `.ferris-idea` and files behind a symbolic link even then.
- **AK-10 (v4)** A file that Git tracks although an ignore rule matches it is in the listing that Find
  in Files and Replace in Files search; its untracked neighbours are not (SPEC-0148 AK-9,
  `own_sources::tests::files_git_tracks_are_own_although_ignored`).
- **AK-8 (v2)** `search_in_files` and `replace_in_files_preview`: default, `include_non_project` and
  `directory`; `not_searched` with counts, paths and note; the preview for a directory in `.venv` lists
  no file to change and names it under `not_changed`, with the option it lists it; nothing is written.
- **AK-9 (v3)** With the text searched in two own folders, `replace_in_files_preview` with `directory`
  on one lists only that folder's file; the window's replacement set is covered by SPEC-0066 AK-7.
- **AK-11 (v5)** On a host, in the project of AK-6: `search.text` finds only the own file and returns
  `not_searched` naming two folders and one file; with `include_non_project` all four and no
  `not_searched`; with `folder` `.venv/lib` it finds `x.py`; with `files` naming `node_modules/y.js`
  it finds that file alone; a `folder` or a file with `..` is refused; a folder or file reached
  through a symbolic link, also one inside `node_modules`, finds nothing (protocol test
  `find_in_files_on_the_host_searches_the_projects_own_files`; unit test
  `own_sources::tests::a_chosen_place_wins_and_links_are_not_read`).
- **AK-12 (v6)** MCP `search_in_files`: a literal match has `end_line` equal to `line` and
  `end_column` = `column` + its length in characters (also after a non-ASCII character); the regex
  `one\nb\nc` matching over two line breaks ends on the third line; the pattern `(` is an error
  naming the unclosed group (`protocol.rs::search_in_files_gives_the_end_of_each_match`).

## Out of scope
Writing replacements (editor/M6.8), preserve-case replacement, search in libraries outside the workspace,
indexes for very large projects. Since v2 `.gitignore` rules count (behaviour 7). v2 left the other
users of the workspace file list as they were; the Project Owner decided on 2026-11-12 that they follow
the rule: Search Everywhere and Go to File (SPEC-0031 v5), the remote host's `search.text` (v5 here,
SPEC-0318 v8), Inspect Code (SPEC-0098 v8) and Endpoints (SPEC-0103 v2). File listings, the project
model and Find Usages' text fallback keep the plain workspace list.

## Known deviations from the reference
The regex dialect is the `regex` crate's (no look-around or back-references) — linear-time matching
cannot hang on a pathological pattern; to be compared in the guided session.

v6 (OBS-0071): the reference's text and regex search over MCP return only coordinates, without the
line's text, and a pattern that does not parse returns an empty list as if nothing matched. Ferris IDEA
returns the line text with start and end of each match (IMP-0442) and refuses an invalid pattern with
its reason (IMP-0443).
