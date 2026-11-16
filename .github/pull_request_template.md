## Summary

<!-- What changes for users or contributors, in one to three bullets. -->

## Role

<!-- One role per PR where possible: obs / spec / impl / docs / ci / chore -->

## Checks

- [ ] `scripts/check-pr.sh --changed-only` passes (format, lints, tests of the changed crates); from a fork: `scripts/check-pr.sh --full`
- [ ] User-visible change: lines added as `changelog.d/<PR number>.md` (not in `CHANGELOG.md`)
- [ ] Specs and `docs/specifications/TRACEABILITY.md` updated for new behaviour
- [ ] No code, text, icons or screenshots from proprietary products; ported Apache-2.0 material is in `docs/legal/PROVENANCE.md`
- [ ] Public wording follows `docs/design/CONTENT_STYLE_GUIDE.md` (status words, no unsupported claims)
- [ ] `scripts/check-docs.sh` passes when docs changed
- [ ] Contribution from outside the project: the description contains the acceptance sentence of the [Contributor License Agreement](../docs/legal/CLA.md) (first pull request only)
- [ ] No security problem is described here; those go through `SECURITY.md`
