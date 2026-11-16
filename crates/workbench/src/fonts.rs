//! Bundled editor font (`SPEC-0060` behaviour 7): JetBrains Mono, the reference's default editor font.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Family name of the bundled editor font.
pub const EDITOR_FONT: &str = "JetBrains Mono";
/// Editor font size in pixels and line spacing, the reference's defaults.
pub const EDITOR_FONT_SIZE: f32 = 13.0;
/// Line height as a multiple of the font size.
pub const LINE_SPACING: f32 = 1.2;

/// The four bundled cuts.
pub const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../fonts/JetBrainsMono-BoldItalic.ttf"),
];

/// Line height in pixels for the default editor font, rounded to whole pixels.
pub fn line_height() -> f32 {
    (EDITOR_FONT_SIZE * LINE_SPACING).round()
}

static EDITOR_FAMILY: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);
static EDITOR_ADVANCE: AtomicU32 = AtomicU32::new(0);
static CODE_STYLE: std::sync::RwLock<Option<idecraft_core::CodeStyleSettings>> =
    std::sync::RwLock::new(None);

/// The editor font family chosen in Settings › Font (`SPEC-0150` behaviour 1).
pub fn editor_family() -> String {
    EDITOR_FAMILY
        .read()
        .ok()
        .and_then(|f| f.clone())
        .unwrap_or_else(|| EDITOR_FONT.to_owned())
}

/// Width of one character of the editor font at its size, as the editor measured it last
/// (`SPEC-0150` behaviour 1); before that, JetBrains Mono's 600 of 1000 units.
pub fn editor_advance() -> f32 {
    match f32::from_bits(EDITOR_ADVANCE.load(Ordering::Relaxed)) {
        w if w > 0.0 => w,
        _ => editor_font_size() * 0.6,
    }
}

/// Keeps the measured character width of the editor font.
pub fn set_editor_advance(width: f32) {
    EDITOR_ADVANCE.store(width.to_bits(), Ordering::Relaxed);
}

/// The Code Style settings (`SPEC-0150` behaviour 2).
pub fn code_style() -> idecraft_core::CodeStyleSettings {
    CODE_STYLE
        .read()
        .ok()
        .and_then(|c| c.clone())
        .unwrap_or_default()
}

/// The Code Style language name for a file's language.
pub fn code_style_language(path: &std::path::Path) -> &'static str {
    idecraft_languages::format::code_style_language(path)
}

static EDITOR_LIGATURES: AtomicBool = AtomicBool::new(false);

/// Whether the editor draws font ligatures (`SPEC-0027` v6: off unless the user turned them on).
pub fn editor_ligatures() -> bool {
    EDITOR_LIGATURES.load(Ordering::Relaxed)
}

static EDITOR_SIZE: AtomicU32 = AtomicU32::new(EDITOR_FONT_SIZE.to_bits());
static EDITOR_LINE: AtomicU32 = AtomicU32::new(16.0_f32.to_bits());

/// The editor font size chosen in Settings › Font (`SPEC-0129` behaviour 2).
pub fn editor_font_size() -> f32 {
    f32::from_bits(EDITOR_SIZE.load(Ordering::Relaxed))
}

/// The editor line height in whole pixels for the chosen size and spacing.
pub fn editor_line_height() -> f32 {
    f32::from_bits(EDITOR_LINE.load(Ordering::Relaxed))
}

/// Takes the editor font of `settings`; editors use it from their next frame.
pub fn set_editor_font(settings: &idecraft_core::IdeSettings) {
    EDITOR_SIZE.store(settings.editor.font_size.to_bits(), Ordering::Relaxed);
    EDITOR_LINE.store(settings.line_height().to_bits(), Ordering::Relaxed);
    EDITOR_LIGATURES.store(settings.editor.ligatures, Ordering::Relaxed);
    if let Ok(mut f) = EDITOR_FAMILY.write() {
        *f = Some(settings.editor.font_family.clone());
    }
    if let Ok(mut c) = CODE_STYLE.write() {
        *c = Some(settings.code_style.clone());
    }
}

/// Monospaced families offered on the Font page besides the bundled one, where installed.
pub const KNOWN_MONOSPACE: [&str; 14] = [
    "Menlo",
    "Monaco",
    "SF Mono",
    "Courier New",
    "Fira Code",
    "Fira Mono",
    "Source Code Pro",
    "Hack",
    "Cascadia Code",
    "IBM Plex Mono",
    "Ubuntu Mono",
    "DejaVu Sans Mono",
    "Roboto Mono",
    "Inconsolata",
];

/// The editor font families to choose from (`SPEC-0150` behaviour 1): the bundled JetBrains Mono,
/// then the known monospaced families among `installed`, and `current` if it is none of these.
pub fn font_choices(installed: &[String], current: &str) -> Vec<String> {
    let mut out = vec![EDITOR_FONT.to_owned()];
    out.extend(
        KNOWN_MONOSPACE
            .iter()
            .filter(|f| installed.iter().any(|i| i == *f))
            .map(|f| f.to_string()),
    );
    if !out.iter().any(|f| f == current) {
        out.push(current.to_owned());
    }
    out
}

/// Registers the bundled fonts with the toolkit; returns whether that worked (else the view falls
/// back to a system monospace font).
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn register(cx: &mut gpui::App) -> bool {
    cx.text_system()
        .add_fonts(FONT_FILES.iter().map(|f| Cow::Borrowed(*f)).collect())
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_choices_start_with_the_bundled_font() {
        let installed = ["Menlo".to_owned(), "Arial".to_owned(), "Hack".to_owned()];
        assert_eq!(
            font_choices(&installed, "JetBrains Mono"),
            ["JetBrains Mono", "Menlo", "Hack"]
        );
        assert_eq!(font_choices(&[], "Mine").last().unwrap(), "Mine");
        assert_eq!(code_style_language(std::path::Path::new("a.go")), "Go");
        assert_eq!(code_style_language(std::path::Path::new("a.scss")), "SCSS");
        assert_eq!(code_style_language(std::path::Path::new("a.less")), "Less");
        assert_eq!(code_style_language(std::path::Path::new("a.css")), "CSS");
        assert_eq!(
            code_style_language(std::path::Path::new("a.txt")),
            "Default"
        );
    }

    #[test]
    fn fonts_are_bundled_truetype_files() {
        for f in FONT_FILES {
            // TrueType files start with 0x00010000.
            assert_eq!(&f[..4], &[0, 1, 0, 0]);
            assert!(f.len() > 200_000);
        }
        assert_eq!(line_height(), 16.0);
    }
}
