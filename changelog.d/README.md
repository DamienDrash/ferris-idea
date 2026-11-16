# Changelog fragments

A pull request with a user-visible change adds its release-note lines here as
`changelog.d/<PR number>.md` instead of editing [`CHANGELOG.md`](../CHANGELOG.md), so two pull
requests never change the same file. The lines keep the format of
[CONTENT_STYLE_GUIDE.md §7](../docs/design/CONTENT_STYLE_GUIDE.md#7-release-notes), under the
groups they belong to:

```markdown
### Added
- Reformat File… lists what can be done with the file (SPEC-0075) (#123)
### Fixed
- Query plan hints name a step without a table once (SPEC-0110) (#123)
```

- Groups: `Added`, `Changed`, `Fixed`, `Removed`, `Security`, each at most once.
- Every line is `- <change> (#N)` on one line and names the number of its file.
- A branch pushed before its pull request exists names the file after the branch
  (`changelog.d/<branch>.md`) and writes `(#PR)`. Once the number is known,
  `scripts/changelog.sh number <branch> <N>` renames the file and writes the number.

`scripts/changelog.sh check` checks the fragments; `scripts/check-docs.sh` runs it. At the daily
version cut, `scripts/changelog.sh assemble` first moves every fragment under `## Unreleased` in
`CHANGELOG.md` (newest pull request first in each group) and deletes it.
