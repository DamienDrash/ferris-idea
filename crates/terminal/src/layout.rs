//! Layout of the shells of a terminal tab (`SPEC-0205` behaviour 3): the tab's first split
//! decides the outer direction; a split across it stacks shells inside one cell, so ⌘D then
//! ⇧⌘D gives two columns of which the second holds two shells one above the other.

use std::ops::Range;

/// Index and cell of the shell a split adds next to `active`: `groups` holds the cell of each
/// shell in order, `outer_side_by_side` the tab's outer direction and `side_by_side` the split's
/// own (⌘D true, ⇧⌘D false). With one shell the split decides the outer direction itself.
pub fn split_position(
    groups: &[usize],
    active: usize,
    outer_side_by_side: bool,
    side_by_side: bool,
) -> (usize, usize) {
    let active = active.min(groups.len().saturating_sub(1));
    let fresh = groups.iter().max().map_or(0, |g| g + 1);
    if groups.len() <= 1 || side_by_side == outer_side_by_side {
        // A new cell after the one with focus.
        let end = cell_runs(groups)
            .into_iter()
            .find(|r| r.contains(&active))
            .map_or(groups.len(), |r| r.end);
        (end, fresh)
    } else {
        (active + 1, groups[active])
    }
}

/// The shells of each cell as ranges of consecutive equal `groups`, in order.
pub fn cell_runs(groups: &[usize]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        match runs.last_mut() {
            Some(r) if groups[r.start] == *g => r.end = i + 1,
            _ => runs.push(i..i + 1),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_split_across_the_first_one_stacks_inside_the_cell() {
        // ⌘D on one shell: a second column.
        assert_eq!(split_position(&[0], 0, true, true), (1, 1));
        // ⇧⌘D on the second column: below it, same cell (D47).
        assert_eq!(split_position(&[0, 1], 1, true, false), (2, 1));
        // ⇧⌘D on the first column: below the first shell, in its cell.
        assert_eq!(split_position(&[0, 1, 1], 0, true, false), (1, 0));
        // ⌘D on the first of a stacked cell: a new column after the whole cell.
        assert_eq!(split_position(&[0, 0, 1], 0, true, true), (2, 2));
        assert_eq!(cell_runs(&[0, 0, 2, 1, 1]), vec![0..2, 2..3, 3..5]);
        assert!(cell_runs(&[]).is_empty());
    }
}
