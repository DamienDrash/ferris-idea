//! Bundled icons as toolkit images, cached (`SPEC-0060` behaviour 8).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{Image, ImageFormat, Img, img, prelude::*, px};

/// Icon edge in logical pixels.
pub const SIZE: f32 = 16.0;

/// The toolkit rasterises SVGs at their own size; doubling `width`/`height` keeps them sharp on 2×
/// displays (the files themselves stay unchanged).
fn doubled(svg: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(svg);
    // The 16 px icons and the 20 px stripe and toolbar icons (SPEC-0061 v4 behaviour 12).
    let edge = if text.contains(r#"width="20""#) {
        20
    } else {
        16
    };
    text.replacen(
        &format!(r#"width="{edge}""#),
        &format!(r#"width="{}""#, edge * 2),
        1,
    )
    .replacen(
        &format!(r#"height="{edge}""#),
        &format!(r#"height="{}""#, edge * 2),
        1,
    )
    .into_bytes()
}

/// The icon with red and blue swapped in every `#rrggbb` colour (`SPEC-0060` v8 behaviour 8): the
/// toolkit hands SVG images to the screen without converting them to its blue-first byte order
/// (its raster images are converted), so the swap here gives the file's own colours on screen.
fn swap_red_blue(svg: &[u8]) -> Vec<u8> {
    let mut out = svg.to_vec();
    let hex = |b: u8| b.is_ascii_hexdigit();
    let mut i = 0;
    while i + 7 <= out.len() {
        let colour = out[i] == b'#'
            && out[i + 1..i + 7].iter().all(|&b| hex(b))
            && out.get(i + 7).is_none_or(|&b| !hex(b));
        if colour {
            out.swap(i + 1, i + 5);
            out.swap(i + 2, i + 6);
            i += 7;
        } else {
            i += 1;
        }
    }
    out
}

/// Rasterised icons by key and variant (dark or light).
type IconCache = Mutex<HashMap<(String, bool), Arc<Image>>>;

fn image(key: &str, dark: bool) -> Option<Arc<Image>> {
    static CACHE: OnceLock<IconCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = cache.get(&(key.to_owned(), dark)) {
        return Some(i.clone());
    }
    let svg = ferris_idea_ui::icons::svg(key, dark)?;
    let i = Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        swap_red_blue(&doubled(svg)),
    ));
    cache.insert((key.to_owned(), dark), i.clone());
    Some(i)
}

/// Ferris IDEA's app icon (brand package, ADR-0018) as an image, for the Welcome window's column
/// (`SPEC-0355` behaviour 5).
pub fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            swap_red_blue(ferris_idea_ui::brand::APP_ICON_SVG.as_bytes()),
        ))
    })
    .clone()
}

/// A 16 px icon element for `key` in the variant of the chosen theme, or an empty spacer if the
/// icon is missing.
pub fn icon(key: &str) -> gpui::AnyElement {
    match image(key, crate::palette::Palette::current().dark) {
        Some(i) => icon_img(img(i)).into_any_element(),
        None => gpui::div().size(px(SIZE)).flex_none().into_any_element(),
    }
}

/// Whether icons drawn on `background` need the dark theme's variant (light strokes): the
/// background is dark, such as the blue of a selected tool window button in the light themes (D4,
/// hand test 28).
pub fn dark_variant_on(background: ferris_idea_ui::theme::Rgba) -> bool {
    let [r, g, b] =
        [16, 8, 0].map(|shift| f32::from(((background.rgb >> shift) & 0xff) as u8) / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5
}

/// An icon element of `size` logical pixels (20 on the stripes and in the main toolbar,
/// `SPEC-0061` v4); 16 px icons are scaled as the reference does.
pub fn icon_sized(key: &str, size: f32) -> gpui::AnyElement {
    match image(key, crate::palette::Palette::current().dark) {
        Some(i) => img(i).size(px(size)).flex_none().into_any_element(),
        None => gpui::div().size(px(size)).flex_none().into_any_element(),
    }
}

/// An icon element of `size` logical pixels drawn grey and faint, for a button that cannot act
/// now (the main toolbar's Run and Debug without anything to run, `SPEC-0061` v17 behaviour 66).
pub fn icon_sized_disabled(key: &str, size: f32) -> gpui::AnyElement {
    match image(key, crate::palette::Palette::current().dark) {
        Some(i) => img(i)
            .size(px(size))
            .flex_none()
            .grayscale(true)
            .opacity(0.5)
            .into_any_element(),
        None => gpui::div().size(px(size)).flex_none().into_any_element(),
    }
}

/// An icon of `size` logical pixels for `key` drawn on `background`, in the variant that
/// contrasts with it.
pub fn icon_on_sized(
    key: &str,
    background: ferris_idea_ui::theme::Rgba,
    size: f32,
) -> gpui::AnyElement {
    match image(key, dark_variant_on(background)) {
        Some(i) => img(i).size(px(size)).flex_none().into_any_element(),
        None => gpui::div().size(px(size)).flex_none().into_any_element(),
    }
}

fn icon_img(i: Img) -> Img {
    i.size(px(SIZE)).flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D4: on the blue of a selected stripe button the light icons (grey strokes) vanish; the
    /// dark variant is used there, on light backgrounds the light one.
    // SPEC-0060 v8 AK-7b.
    #[test]
    fn svg_colours_are_swapped_for_the_toolkit() {
        let svg =
            br##"<path fill="#F2C55C"/><circle fill="#548af7" stroke="white"/><g id="#abc"/>"##;
        let swapped = String::from_utf8(swap_red_blue(svg)).unwrap();
        assert_eq!(
            swapped,
            r##"<path fill="#5CC5F2"/><circle fill="#f78a54" stroke="white"/><g id="#abc"/>"##
        );
        assert_eq!(swap_red_blue(swapped.as_bytes()), svg.to_vec());
    }

    /// SPEC-0061 v4 behaviour 12: 20 px stripe icons are rasterised at twice their size too.
    #[test]
    fn twenty_pixel_icons_are_doubled() {
        let svg = ferris_idea_ui::icons::svg("toolwindows/project@20x20", true).unwrap();
        let out = String::from_utf8(doubled(svg)).unwrap();
        assert!(out.contains(r#"width="40""#) && out.contains(r#"height="40""#));
        let small = ferris_idea_ui::icons::svg("toolwindows/project", true).unwrap();
        assert!(
            String::from_utf8(doubled(small))
                .unwrap()
                .contains(r#"width="32""#)
        );
    }

    #[test]
    fn icon_variant_follows_the_background() {
        let rgba = |rgb| ferris_idea_ui::theme::Rgba { rgb, alpha: 255 };
        assert!(dark_variant_on(rgba(0x3574f0)));
        assert!(dark_variant_on(rgba(0x2b2d30)));
        assert!(!dark_variant_on(rgba(0xffffff)));
        assert!(!dark_variant_on(rgba(0xdfe1e5)));
    }
}
