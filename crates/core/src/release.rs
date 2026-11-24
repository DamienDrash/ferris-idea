//! Ferris IDEA's version and maturity stage, shown by `ferris-idea --version`, the About dialog and
//! problem reports.
//!
//! The version has one source: `[workspace.package] version` in the workspace `Cargo.toml`. The
//! stage follows the criteria in `docs/design/CONTENT_STYLE_GUIDE.md` §5 and must match the
//! `Stage:` line of the newest version in `CHANGELOG.md` (checked by a test below).

/// Ferris IDEA's version (SemVer), from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The project's maturity stage (CONTENT_STYLE_GUIDE §5): `pre-alpha`, `alpha`, `beta` or
/// `stable` (the 1.0 stage).
pub const STAGE: &str = "stable";

/// Version and stage as one label, e.g. `0.8.0 (alpha)`.
pub fn version_label() -> String {
    format!("{VERSION} ({STAGE})")
}

/// What `ferris-idea --version` prints, e.g. `ferris-idea 0.8.0 (alpha)`. The first two words stay
/// `ferris-idea <version>`, which the SSH backend probe compares.
pub fn version_line() -> String {
    format!("ferris-idea {}", version_label())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
    /// The heading of versions released before the rename (`SPEC-0356`).
    const FORMER_HEADING: &str = "## IDECraft v"; // rebrand:keep

    /// The newest `## Ferris IDEA vX.Y.Z — date` section and the stage written under it.
    fn newest_release() -> (String, String) {
        let mut lines = CHANGELOG.lines();
        let heading = lines
            .by_ref()
            .find_map(|l| {
                l.strip_prefix("## Ferris IDEA v")
                    .or_else(|| l.strip_prefix(FORMER_HEADING))
            })
            .expect("CHANGELOG.md has a release section");
        let version = heading.split_whitespace().next().unwrap().to_owned();
        let stage = lines
            .find_map(|l| l.strip_prefix("Stage: "))
            .expect("the newest release names its stage")
            .trim()
            .to_owned();
        (version, stage)
    }

    #[test]
    fn version_and_stage_match_the_newest_changelog_release() {
        let (version, stage) = newest_release();
        assert_eq!(version, VERSION, "Cargo.toml and CHANGELOG.md disagree");
        assert_eq!(stage, STAGE, "STAGE and CHANGELOG.md disagree");
        assert!(["pre-alpha", "alpha", "beta", "stable"].contains(&STAGE));
    }

    #[test]
    fn changelog_keeps_an_unreleased_section_above_the_releases() {
        let unreleased = CHANGELOG
            .find("\n## Unreleased")
            .expect("## Unreleased exists");
        let first = CHANGELOG
            .find("\n## Ferris IDEA v")
            .into_iter()
            .chain(CHANGELOG.find(&format!("\n{FORMER_HEADING}")))
            .min()
            .unwrap();
        assert!(unreleased < first);
    }

    #[test]
    fn version_line_names_version_and_stage() {
        assert_eq!(version_line(), format!("ferris-idea {VERSION} ({STAGE})"));
        assert!(version_line().starts_with(&format!("ferris-idea {VERSION}")));
    }
}
