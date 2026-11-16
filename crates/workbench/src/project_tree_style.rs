//! How the project tree draws the folders of Maven and Gradle modules (`SPEC-0048` v3 behaviour
//! 10): an icon per role and a tint behind test and excluded folders.

use idecraft_core::buildtools::FolderRole;

/// The icon of a folder with `role`.
pub fn folder_role_icon(role: FolderRole) -> &'static str {
    match role {
        FolderRole::Module => "nodes/module",
        FolderRole::Sources => "idecraft/sourceRoot",
        FolderRole::Tests => "idecraft/testRoot",
        FolderRole::Resources => "idecraft/resourcesRoot",
        FolderRole::TestResources => "idecraft/testResourcesRoot",
        FolderRole::Excluded => "idecraft/excludedRoot",
    }
}

/// The background of a row with `role`: green for tests, orange for excluded output, none
/// otherwise; faint enough for the text and the selection to stay readable.
pub fn folder_role_tint(role: FolderRole) -> Option<gpui::Rgba> {
    let dark = crate::palette::Palette::current().dark;
    match (role, dark) {
        (FolderRole::Tests | FolderRole::TestResources, true) => Some(gpui::rgba(0x2f4a3366)),
        (FolderRole::Tests | FolderRole::TestResources, false) => Some(gpui::rgba(0xdff2e3ff)),
        (FolderRole::Excluded, true) => Some(gpui::rgba(0x4a372b66)),
        (FolderRole::Excluded, false) => Some(gpui::rgba(0xfbe9dbff)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0048 v3 AK-10: every role has a bundled icon; only tests and excluded output are tinted.
    #[test]
    fn every_role_has_an_icon() {
        for role in [
            FolderRole::Module,
            FolderRole::Sources,
            FolderRole::Tests,
            FolderRole::Resources,
            FolderRole::TestResources,
            FolderRole::Excluded,
        ] {
            assert!(idecraft_ui::icons::svg(folder_role_icon(role), true).is_some());
            assert_eq!(
                folder_role_tint(role).is_some(),
                matches!(
                    role,
                    FolderRole::Tests | FolderRole::TestResources | FolderRole::Excluded
                )
            );
        }
    }
}
