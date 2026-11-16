//! The IDECraft app icon, embedded in the binary (`SPEC-0060` v6 behaviour 12, ADR-0018).
//!
//! IDECraft is often started as a bare binary (`idecraft ui <folder>`), not from an app bundle, so
//! the system has no icon file to show. The packaged brand icon (brand package v1.1.5, rendered from
//! `docs/design/assets/idecraft-app-icon.svg`) is compiled in and set at start-up.

/// The macOS icon (`packaging/icons/macos/IDECraft.icns`, 16–1024 px).
pub const APP_ICON_ICNS: &[u8] = include_bytes!("../../../packaging/icons/macos/IDECraft.icns");

/// The scalable icon used by Linux desktops (`packaging/icons/linux/hicolor/scalable/apps/idecraft.svg`).
pub const APP_ICON_SVG: &str =
    include_str!("../../../packaging/icons/linux/hicolor/scalable/apps/idecraft.svg");

/// The application id Linux desktops match against `idecraft.desktop` (`StartupWMClass`) to find
/// the icon.
pub const APP_ID: &str = "idecraft";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_bytes_are_embedded() {
        // An ICNS file starts with `icns` and its own length (big endian).
        assert_eq!(&APP_ICON_ICNS[..4], b"icns");
        let len = u32::from_be_bytes(APP_ICON_ICNS[4..8].try_into().unwrap());
        assert_eq!(len as usize, APP_ICON_ICNS.len());
        assert!(APP_ICON_ICNS.len() > 10_000, "all sizes, not a stub");
        // At least one PNG-backed entry (`ic07`…`ic14`, `icp4`…).
        assert!(APP_ICON_ICNS.windows(4).any(|w| w == b"\x89PNG"));
        assert!(APP_ICON_SVG.trim_start().starts_with("<svg") || APP_ICON_SVG.contains("<svg"));
        let desktop = include_str!("../../../packaging/icons/linux/idecraft.desktop");
        assert!(desktop.contains(&format!("StartupWMClass={APP_ID}")));
        assert!(desktop.contains(&format!("Icon={APP_ID}")));
    }
}
