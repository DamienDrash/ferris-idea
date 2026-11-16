//! Which groups of a one-row toolbar fit its width; the others go into the toolbar's ⋮ menu
//! (`SPEC-0190` v5 behaviour 3). The row never wraps into a second, ragged row.

/// Width of the ⋮ button with the gap before it.
pub const MORE_WIDTH: f32 = 30.0;

/// One group of controls that stays in the row or goes into the menu as a whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group<K> {
    /// What the group is.
    pub key: K,
    /// Its width in the row, gaps included.
    pub width: f32,
    /// When it gives way: 0 never, otherwise the higher the earlier.
    pub gives_way: u8,
}

/// Estimated width of a text button with `label` (8 px padding on both sides, 4 px gap).
pub fn text_width(label: &str) -> f32 {
    label.chars().count() as f32 * 7.5 + 20.0
}

/// The keys of `groups` that stay in the row and those that go into the ⋮ menu, both in row
/// order, for a row `available` pixels wide. Groups give way in their order until the rest and
/// the ⋮ button fit; groups that never give way stay even when they do not fit. A width of zero
/// or less (not measured yet) keeps every group.
pub fn fit<K: Copy>(groups: &[Group<K>], available: f32) -> (Vec<K>, Vec<K>) {
    let total: f32 = groups.iter().map(|g| g.width).sum();
    if available <= 0.0 || total <= available {
        return (groups.iter().map(|g| g.key).collect(), Vec::new());
    }
    let mut order: Vec<usize> = (0..groups.len())
        .filter(|&i| groups[i].gives_way > 0)
        .collect();
    // Highest first; equal ones from the right end of the row.
    order.sort_by(|&a, &b| {
        groups[b]
            .gives_way
            .cmp(&groups[a].gives_way)
            .then(b.cmp(&a))
    });
    let mut hidden = vec![false; groups.len()];
    let mut width = total + MORE_WIDTH;
    for i in order {
        if width <= available {
            break;
        }
        hidden[i] = true;
        width -= groups[i].width;
    }
    let mut shown = Vec::new();
    let mut more = Vec::new();
    for (g, h) in groups.iter().zip(hidden) {
        if h {
            more.push(g.key);
        } else {
            shown.push(g.key);
        }
    }
    (shown, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(key: char, width: f32, gives_way: u8) -> Group<char> {
        Group {
            key,
            width,
            gives_way,
        }
    }

    // SPEC-0190 v5 AK-9: one row; what does not fit goes into the ⋮ menu in its order.
    #[test]
    fn groups_give_way_in_their_order_and_never_wrap() {
        let row = [
            g('p', 120., 0),
            g('f', 300., 0),
            g('t', 90., 8),
            g('e', 200., 2),
            g('x', 80., 4),
        ];
        // Wide enough: everything in the row, no menu.
        assert_eq!(fit(&row, 1000.), (vec!['p', 'f', 't', 'e', 'x'], vec![]));
        // Not measured yet: everything stays.
        assert_eq!(fit(&row, 0.).1, Vec::<char>::new());
        // 790 needed and 30 for ⋮: the transpose group (8) goes first, then export (4), then
        // the edit group (2).
        assert_eq!(fit(&row, 760.), (vec!['p', 'f', 'e', 'x'], vec!['t']));
        assert_eq!(fit(&row, 700.), (vec!['p', 'f', 'e'], vec!['t', 'x']));
        assert_eq!(fit(&row, 600.), (vec!['p', 'f'], vec!['t', 'e', 'x']));
        // Very narrow: only what never gives way stays.
        assert_eq!(fit(&row, 300.), (vec!['p', 'f'], vec!['t', 'e', 'x']));
        // The ⋮ button counts: 790 − 90 + 30 does not fit 725.
        assert_eq!(fit(&row, 725.).1, vec!['t', 'x']);
        let (shown, more) = fit(&row, 785.);
        let used: f32 = row
            .iter()
            .filter(|x| shown.contains(&x.key))
            .map(|x| x.width)
            .sum::<f32>()
            + MORE_WIDTH;
        assert!(!more.is_empty() && used <= 785.);
        assert!(text_width("Export…") > text_width("Row"));
    }
}
