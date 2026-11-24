//! Screenshots drawn by the window itself (`SPEC-0319` v7 behaviour 53): what is filled before
//! the image leaves the process — secret nodes, password inputs, terminal text for clients
//! without `terminal-read`, editors of files outside the workspace — and the region cut out of
//! the frame. Pixels are RGBA, 8 bits per channel, row after row, in device pixels.

use accesskit::Role;

use crate::tree::{Rect, TextClass, Tree};

/// The margin around a filled area, in logical pixels (behaviour 33).
pub const MASK_MARGIN: f32 = 2.0;

/// The colour of filled areas (opaque mid grey).
pub const MASK_RGBA: [u8; 4] = [0x80, 0x80, 0x80, 0xff];

/// The areas to fill, in logical window coordinates with their margin (behaviour 33).
pub fn masked_bounds(tree: &Tree, terminal_read: bool) -> Vec<Rect> {
    tree.iter()
        .filter(|n| {
            n.secret
                || n.role() == Role::PasswordInput
                || n.text_class == TextClass::OutsideWorkspace
                || (!terminal_read
                    && (n.role() == Role::Terminal || n.text_class == TextClass::Terminal))
        })
        .filter_map(|n| n.visible_bounds.or_else(|| Some(n.bounds())))
        .filter(|r| !r.is_empty())
        .map(|r| {
            Rect::new(
                r.x - MASK_MARGIN,
                r.y - MASK_MARGIN,
                r.w + 2. * MASK_MARGIN,
                r.h + 2. * MASK_MARGIN,
            )
        })
        .collect()
}

/// Device-pixel bounds of a logical rectangle, clipped to the image (left, top, right, bottom).
fn device(r: &Rect, scale: f32, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let clamp = |v: f32, max: u32| (v.max(0.) as u32).min(max);
    let left = clamp((r.x * scale).floor(), width);
    let top = clamp((r.y * scale).floor(), height);
    let right = clamp(((r.x + r.w) * scale).ceil(), width);
    let bottom = clamp(((r.y + r.h) * scale).ceil(), height);
    (right > left && bottom > top).then_some((left, top, right, bottom))
}

/// Fills `rects` (logical pixels) in an RGBA image of `width` × `height` device pixels drawn at
/// `scale`. Returns how many areas were filled.
pub fn fill(pixels: &mut [u8], width: u32, height: u32, scale: f32, rects: &[Rect]) -> usize {
    let mut filled = 0;
    for r in rects {
        let Some((l, t, rt, b)) = device(r, scale, width, height) else {
            continue;
        };
        for y in t..b {
            for x in l..rt {
                let i = ((y * width + x) * 4) as usize;
                if let Some(px) = pixels.get_mut(i..i + 4) {
                    px.copy_from_slice(&MASK_RGBA);
                }
            }
        }
        filled += 1;
    }
    filled
}

/// The part of an RGBA image inside `region` (logical pixels), with its size in device pixels;
/// `None` when the region lies outside the image.
pub fn crop(
    pixels: &[u8],
    width: u32,
    height: u32,
    scale: f32,
    region: &Rect,
) -> Option<(Vec<u8>, u32, u32)> {
    let (l, t, r, b) = device(region, scale, width, height)?;
    let (w, h) = (r - l, b - t);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in t..b {
        let start = ((y * width + l) * 4) as usize;
        out.extend_from_slice(pixels.get(start..start + (w * 4) as usize)?);
    }
    Some((out, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Semantics, TreeBuilder};

    #[test]
    fn secrets_and_terminal_text_are_filled_with_a_margin() {
        let mut b = TreeBuilder::new();
        let pw = Rect::new(10., 10., 20., 10.);
        b.push(
            Semantics::new(Role::PasswordInput).name("Password"),
            pw,
            Some(pw),
        );
        let term = Rect::new(0., 40., 50., 20.);
        b.push(
            Semantics::new(Role::Terminal).name("Terminal"),
            term,
            Some(term),
        );
        let plain = Rect::new(0., 0., 5., 5.);
        b.push(Semantics::new(Role::Button).name("OK"), plain, Some(plain));
        let tree = b.finish(1);
        let without = masked_bounds(&tree, false);
        assert_eq!(without.len(), 2);
        assert_eq!(without[0], Rect::new(8., 8., 24., 14.));
        assert_eq!(masked_bounds(&tree, true).len(), 1);
    }

    #[test]
    fn fill_and_crop_work_in_device_pixels() {
        let (w, h) = (8u32, 6u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        // Logical (1,1) 1×1 at scale 2 is device (2,2)-(4,4).
        assert_eq!(fill(&mut px, w, h, 2.0, &[Rect::new(1., 1., 1., 1.)]), 1);
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        assert_eq!(at(2, 2), &MASK_RGBA);
        assert_eq!(at(3, 3), &MASK_RGBA);
        assert_eq!(at(4, 4), &[0, 0, 0, 0]);
        let (out, cw, ch) = crop(&px, w, h, 2.0, &Rect::new(1., 1., 2., 1.)).unwrap();
        assert_eq!((cw, ch), (4, 2));
        assert_eq!(&out[0..4], &MASK_RGBA);
        assert!(crop(&px, w, h, 2.0, &Rect::new(10., 10., 1., 1.)).is_none());
        // Off the image: nothing filled, nothing panics.
        assert_eq!(fill(&mut px, w, h, 2.0, &[Rect::new(-5., -5., 1., 1.)]), 0);
    }
}
