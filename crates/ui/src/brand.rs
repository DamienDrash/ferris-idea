//! The Ferris IDEA app icon, embedded in the binary (`SPEC-0060` v6 behaviour 12, ADR-0018).
//!
//! Ferris IDEA is often started as a bare binary (`ferris-idea ui <folder>`), not from an app bundle, so
//! the system has no icon file to show. The packaged brand icon (brand package v1.1.5, rendered from
//! `docs/design/assets/idecraft-app-icon.svg`) is compiled in and set at start-up.

/// The macOS icon (`packaging/icons/macos/FerrisIDEA.icns`, 16–1024 px).
pub const APP_ICON_ICNS: &[u8] = include_bytes!("../../../packaging/icons/macos/FerrisIDEA.icns");

/// The scalable icon used by Linux desktops (`packaging/icons/linux/hicolor/scalable/apps/ferris-idea.svg`).
pub const APP_ICON_SVG: &str =
    include_str!("../../../packaging/icons/linux/hicolor/scalable/apps/ferris-idea.svg");

/// The application id Linux desktops match against `ferris-idea.desktop` (`StartupWMClass`) to find
/// the icon.
pub const APP_ID: &str = "ferris-idea";

/// The horizontal logo (symbol and wordmark, 12 units apart), the vector master the startup
/// animation draws from (`SPEC-0355` v2 behaviour 23); embedded at build time, never fetched.
pub const HORIZONTAL_LOGO_SVG: &str =
    include_str!("../../../docs/design/assets/idecraft-horizontal.svg");

/// The wordmark on its own; its paths are those of [`HORIZONTAL_LOGO_SVG`] (checked by the
/// startup animation's tests).
pub const WORDMARK_SVG: &str = include_str!("../../../docs/design/assets/idecraft-wordmark.svg");

/// The symbol on its own background, which gives the startup animation its background colour.
pub const SYMBOL_DARK_SVG: &str =
    include_str!("../../../docs/design/assets/idecraft-symbol-dark.svg");

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
        let desktop = include_str!("../../../packaging/icons/linux/ferris-idea.desktop");
        assert!(desktop.contains(&format!("StartupWMClass={APP_ID}")));
        assert!(desktop.contains(&format!("Icon={APP_ID}")));
    }
}
