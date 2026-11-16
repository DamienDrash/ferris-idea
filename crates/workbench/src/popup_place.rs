//! Where a list that opens at the caret goes (`SPEC-0122` v5 behaviour 13): under the caret's
//! line, over it when only that fits, else on the roomier side with a limited height; always inside
//! the editor area. Free of the UI toolkit, so the rule is tested without a window.

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
    let below = area.bottom - MARGIN - (caret.bottom + GAP);
    let over = caret.top - GAP - (area.top + MARGIN);
    // Under the line whenever it fits there; over it when only that fits; else the roomier side.
    let above = height > below && (height <= over || over > below);
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

    #[test]
    fn rows_that_fit_the_room() {
        assert_eq!(rows_that_fit(570., 40., 24.), 22);
        assert_eq!(rows_that_fit(64., 40., 24.), 1);
        assert_eq!(rows_that_fit(10., 40., 24.), 1);
    }
}
