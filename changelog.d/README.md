# Changelog fragments

A pull request with a user-visible change adds its release-note lines here as
`changelog.d/<branch>.md` (or `<PR number>.md`) instead of editing [`CHANGELOG.md`](../CHANGELOG.md), so two pull
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
- Every line is `- <change> (#N)` on one line and names the number of its file (or `#PR`).
- Name the file after the branch (`changelog.d/<branch>.md`) and write `(#PR)`. Do not rename it
  or push again only to write the number: every new push starts the server CI's check of the pull
  request again. At the version cut, `assemble` takes the number from the squash commit that added
  the file to main ("… (#N)").
- In a numbered fragment, `(#PR)` stands for the number of its file and `assemble` writes that
  number, so a plain rename to `<N>.md` is enough.

`scripts/changelog.sh check` checks the fragments; `scripts/check-docs.sh` runs it. At the daily
version cut, `scripts/changelog.sh assemble` first moves every fragment under `## Unreleased` in
`CHANGELOG.md` (newest pull request first in each group) and deletes it.
