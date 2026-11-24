# SPEC-0037: Three-way text merge (merge editor model)

- **Version:** 3 · **Date:** 2026-11-13 (v2: simple conflicts resolved word by word; v3: markers in files
  with CRLF line ends, recognising left-over markers; found by driving real conflicts against the `git`
  program)
- **Sources:** GEN (IDEs resolve conflicts in a three-pane editor: their changes, the result and our changes;
  non-conflicting changes are applied automatically, each conflict is accepted from one side, both sides
  or edited by hand; F-075), STD (diff3 merge semantics; conflict markers `<<<<<<<`, `|||||||`, `=======`,
  `>>>>>>>` as written by `git merge-file`, compared against the `git` program), SPEC-0032 (line diff)
- **Feature matrix rows:** F-075

## Purpose
Turn a base text and two changed versions into a list of chunks a merge editor can show and resolve, and
into the merged text.

## Behaviour
1. Texts are compared line by line (line endings kept). Lines unchanged on both sides relative to the base
   form *stable* chunks; the regions between them are *changed* chunks with the base, ours and theirs
   lines. [STD diff3]
2. A changed chunk is classified: changed only by us, only by them, by both identically (all resolved
   automatically) or by both differently (*conflict*). [STD]
3. For a conflict, lines equal at the start and at the end of ours and theirs are moved out of the conflict
   into stable text (as Git's "zealous" merge does), so only the differing lines conflict. For the diff3
   marker style this trimming is not applied, as in Git, because the base shown belongs to the whole
   region. [STD]
4. Each conflict can be resolved as ours, theirs, ours then theirs, theirs then ours, base, or custom text;
   a resolution can be undone. The number of open conflicts is reported. [GEN]
5. The merged text joins all chunks; open conflicts are written with markers, labelled with the side names,
   in merge style (ours / theirs) or diff3 style (ours / base / theirs). [STD]
6. A file whose last line has no line break keeps that state in the result. [STD]
7. MCP tool `merge_three_way` (base, ours, theirs texts, optional labels and style) returning chunks,
   conflict count and merged text. [ADR-0008]
8. **v2 – Simple conflicts:** inside a conflict, ours and theirs are compared with the base word by word
   (tokens: runs of letters, digits and `_`; runs of spaces and tabs; a line break; any other single
   character). The conflict is *simple* when every change of ours and every change of theirs touches
   different base tokens with at least one unchanged token between them, or when overlapping changes are
   identical; its resolution is the base with both sides' changes applied (custom text). "Resolve simple
   conflicts" resolves all simple open conflicts and reports how many; other conflicts stay open.
   `merge_three_way` reports `simple_conflicts` and takes `resolve_simple`. [GEN, STD (diff3 applied to
   words)]
9. **v3 – Line ends of the markers:** the marker lines of an open conflict end as the file's lines do, as
   `git merge-file` writes them: with CR LF when the line before the conflict (the first line when the
   conflict starts the file) ends with CR LF in ours and in theirs; where a side has no such line to tell
   by, the first line of the base decides instead; in all other cases with LF. A side's last line without
   a line break is closed with the same line end before the next marker. Before v3 the markers always
   ended with LF, so a file with CRLF line ends got mixed line ends. [STD]
10. **v3 – Left-over markers:** a text *has conflict markers* when it holds, in this order, a line that
    starts with `<<<<<<<` followed by a space or the line end, a line that is exactly `=======`, and a line
    that starts with `>>>>>>>` followed by a space or the line end (a CR before the line end is ignored). A
    lone `=======` line (a heading underline) is none. Callers use this to refuse writing an unresolved
    text as resolved (SPEC-0101 v6 behaviour 13). [STD, GEN]

## Acceptance criteria
- **AK-1** Chunk classification: only ours, only theirs, both same, conflict, insertions at start and end,
  deletions, empty base.
- **AK-2** Zealous trimming of common lines inside a conflict.
- **AK-3** Merged text with markers equals `git merge-file -p` (merge and `--diff3` style, same labels) for
  a set of cases including clean merges and conflicts.
- **AK-4** Resolving each conflict variant, undo, open-conflict count, custom text.
- **AK-5** The MCP tool returns chunks, count and text.
- **AK-6 (v2)** Simple conflicts: changes to different words of one line merge; adjacent or overlapping
  different changes stay open; identical overlapping changes merge; insertions on both sides at the same
  place stay open (unit tests).
- **AK-7 (v3)** Files with CRLF line ends, with LF, with mixed line ends and without a final line break:
  the merged text with markers equals `git merge-file -p` byte for byte (integration test against the
  `git` program).
- **AK-8 (v3)** Left-over markers are recognised in merge and diff3 style and with CRLF; a lone `=======`
  line, markers in the wrong order and longer runs of `<` are not (unit test).

## Out of scope
Character-level merging inside words, ignoring whitespace, the editor UI (SPEC-0101).

## Known deviations from the reference
Git's `zealous_alnum` level (joining conflicts separated only by lines without letters or digits) is not
applied; such conflicts stay separate, each resolvable on its own. Git also joins two conflicts with at
most three unchanged lines between them into one, lines in between included; Ferris IDEA keeps them apart
(v3: measured with three conflicts two lines apart, which `git merge-file` writes as one block and
Ferris IDEA lists as three), so the open-conflict count of such a file is higher than the number of marker
blocks Git wrote into the working tree, and each of them can be resolved on its own.
