//! Where a list that opens at the caret goes (`SPEC-0122` v5 behaviour 13): under the caret's
//! line, over it when only that fits, else on the roomier side with a limited height; always inside
//! the editor area. Parameter info (`SPEC-0080` v5 behaviour 9) prefers the place over the line.
//! Free of the UI toolkit, so the rule is tested without a window.

/// Space between the caret's line and the list.
pub const GAP: f32 = 2.;
/// Space kept free at the edges of the editor area.
pub const MARGIN: f32 = 8.;

/// The caret's line in window coordinates: the caret's x, the line's top and bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretSpot {
    /// x of the caret.
    pub x: f32,
    /// Top of the caret's line.
    pub top: f32,
    /// Bottom of the caret's line.
    pub bottom: f32,
}

/// The editor area in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Right edge.
    pub right: f32,
    /// Bottom edge.
    pub bottom: f32,
}

/// Where the list goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Left edge of the list.
    pub x: f32,
    /// The list's top when it stands under the line, its bottom when it stands over it.
    pub y: f32,
    /// The list stands over the caret's line.
    pub above: bool,
    /// The height the list may take at most.
    pub max_height: f32,
}

/// The place of a list `width` wide and `height` high (as it would be with all its rows) opened at
/// `caret` inside `area`.
pub fn place_at_caret(caret: CaretSpot, area: Area, width: f32, height: f32) -> Place {
    place(caret, area, width, height, false)
}

/// The place of parameter info `width` wide and `height` high opened at `caret` inside `area`
/// (`SPEC-0080` v5 behaviour 9): over the caret's line whenever it fits there, under it when only
/// that fits, else the roomier side. Like a list at the caret it starts at the caret and moves
/// left only as far as it must to stay inside the editor area, never further left than the area:
/// a popup too wide to fit is not turned around to end at the caret.
pub fn place_over_caret(caret: CaretSpot, area: Area, width: f32, height: f32) -> Place {
    place(caret, area, width, height, true)
}

fn place(caret: CaretSpot, area: Area, width: f32, height: f32, prefer_over: bool) -> Place {
    let below = area.bottom - MARGIN - (caret.bottom + GAP);
    let over = caret.top - GAP - (area.top + MARGIN);
    // The preferred side whenever it fits there; the other when only that fits; else the roomier.
    let above = if prefer_over {
        height <= over || (height > below && over > below)
    } else {
        height > below && (height <= over || over > below)
    };
    let room = if above { over } else { below };
    let leftmost = area.left + MARGIN;
    let x = caret.x.min(area.right - MARGIN - width).max(leftmost);
    Place {
        x,
        y: if above {
            caret.top - GAP
        } else {
            caret.bottom + GAP
        },
        above,
        max_height: room.max(0.),
    }
}

/// Whether a submenu `width` wide that opens beside a list goes to the list's left
/// (`SPEC-0076` v13 behaviour 19): the list starts at `list_left` and is `list_width` wide in a
/// window `window_width` wide. The submenu stands right of the list whenever it fits there; when it
/// does not, it goes left of the list if there is more room on that side.
pub fn submenu_left(list_left: f32, list_width: f32, width: f32, window_width: f32) -> bool {
    let right = window_width - MARGIN - (list_left + list_width + GAP);
    let left = list_left - GAP - MARGIN;
    width > right && left > right
}

/// How many rows `row` high fit into `max_height` next to `chrome` (heading line, padding and
/// border); at least one, so a list never disappears.
pub fn rows_that_fit(max_height: f32, chrome: f32, row: f32) -> usize {
    (((max_height - chrome) / row).floor().max(1.)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Area = Area {
        left: 300.,
        top: 100.,
        right: 1300.,
        bottom: 800.,
    };

    fn caret(x: f32, top: f32) -> CaretSpot {
        CaretSpot {
            x,
            top,
            bottom: top + 20.,
        }
    }

    // SPEC-0122 v5 AK-15.
    #[test]
    fn under_the_line_when_it_fits() {
        let p = place_at_caret(caret(500., 200.), AREA, 360., 180.);
        assert_eq!((p.x, p.y, p.above), (500., 222., false));
        assert_eq!(p.max_height, 800. - 8. - 222.);
        // Exactly as high as the room still counts as fitting.
        let p = place_at_caret(caret(500., 200.), AREA, 360., 570.);
        assert!(!p.above);
    }

    #[test]
    fn over_the_line_when_only_that_fits() {
        let p = place_at_caret(caret(500., 700.), AREA, 360., 180.);
        assert!(p.above);
        // The list's bottom, 2 px over the line.
        assert_eq!((p.x, p.y), (500., 698.));
        assert_eq!(p.max_height, 698. - 108.);
    }

    #[test]
    fn the_roomier_side_with_a_limited_height() {
        // 900 px of rows fit nowhere: more room under a caret near the top …
        let p = place_at_caret(caret(500., 200.), AREA, 360., 900.);
        assert!(!p.above);
        assert_eq!(p.max_height, 570.);
        // … and over a caret near the bottom.
        let p = place_at_caret(caret(500., 600.), AREA, 360., 900.);
        assert!(p.above);
        assert_eq!(p.max_height, 490.);
        // A caret outside the area leaves no negative room.
        let p = place_at_caret(caret(500., 50.), AREA, 360., 2000.);
        assert!(!p.above);
        let p = place_at_caret(caret(500., 900.), AREA, 360., 2000.);
        assert!(p.above && p.max_height > 0.);
    }

    #[test]
    fn moved_left_to_stay_inside() {
        let p = place_at_caret(caret(1200., 200.), AREA, 360., 180.);
        assert_eq!(p.x, 1300. - 8. - 360.);
        // Never left of the area, also when the area is narrower than the list.
        let narrow = Area {
            right: 500.,
            ..AREA
        };
        let p = place_at_caret(caret(450., 200.), narrow, 360., 180.);
        assert_eq!(p.x, 308.);
        let p = place_at_caret(caret(100., 200.), AREA, 360., 180.);
        assert_eq!(p.x, 308.);
    }

    // SPEC-0080 v5 AK-7: parameter info over the line, at the caret, inside the editor area.
    #[test]
    fn parameter_info_over_the_line_at_the_caret() {
        let p = place_over_caret(caret(500., 400.), AREA, 300., 120.);
        assert_eq!((p.x, p.y, p.above), (500., 398., true));
        // Under the line when the room over it is too small (a caret near the top).
        let p = place_over_caret(caret(500., 150.), AREA, 300., 120.);
        assert_eq!((p.x, p.y, p.above), (500., 172., false));
        // Neither fits: the roomier side.
        let p = place_over_caret(caret(500., 600.), AREA, 300., 900.);
        assert!(p.above);
    }

    // Window check 2026-11-18 gap P1: the twelve overloads of `List.of` are wider than the room
    // right of the caret. The popup moves left only as far as needed and never leaves the editor
    // area for the project tree on its left; it is not turned around to end at the caret.
    #[test]
    fn wide_parameter_info_stays_over_the_editor() {
        let p = place_over_caret(caret(900., 400.), AREA, 700., 300.);
        assert_eq!(p.x, 1300. - 8. - 700.);
        assert!(p.x >= AREA.left + 8. && p.x + 700. <= AREA.right - 8.);
        // Wider than the whole area: it starts at the area's left edge, not over the tree.
        let p = place_over_caret(caret(900., 400.), AREA, 1200., 300.);
        assert_eq!(p.x, 308.);
        // Narrow enough: it starts at the caret.
        let p = place_over_caret(caret(600., 400.), AREA, 600., 300.);
        assert_eq!(p.x, 600.);
    }

    #[test]
    fn rows_that_fit_the_room() {
        assert_eq!(rows_that_fit(570., 40., 24.), 22);
        assert_eq!(rows_that_fit(64., 40., 24.), 1);
        assert_eq!(rows_that_fit(10., 40., 24.), 1);
    }

    /// SPEC-0076 v13 AK-16 (behaviour 19, OBS-0100 SM1): the ⌥↩ list (420 wide) with its submenu
    /// (380 wide) in a window 1400 wide: right of the list while it fits, left of it near the right
    /// edge, right again when the left side has even less room.
    #[test]
    fn submenu_flips_left_near_the_right_edge() {
        assert!(!submenu_left(200., 420., 380., 1400.));
        // 610 + 420 + 2 + 380 + 8 = 1420 > 1400: cut off on the right, so it goes left.
        assert!(submenu_left(610., 420., 380., 1400.));
        assert!(submenu_left(1000., 420., 380., 1400.));
        // Just fits on the right.
        assert!(!submenu_left(590., 420., 380., 1400.));
        // A narrow window: neither side fits, the right has more room.
        assert!(!submenu_left(100., 420., 380., 900.));
    }
}
