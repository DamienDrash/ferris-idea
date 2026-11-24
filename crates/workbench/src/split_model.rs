//! Split editors (`SPEC-0112`): how a position in one view of a file moves when the other view
//! changes the text.

use std::path::PathBuf;

use ferris_idea_editor::Edit;

/// Where char offset `at` lands after `edit`: unchanged before the edit, moved by the length
/// difference after it, and at the end of the inserted text when the edit removed it.
pub fn shift_offset(at: usize, edit: &Edit) -> usize {
    let removed = edit.deleted.chars().count();
    let inserted = edit.inserted.chars().count();
    if at <= edit.offset {
        at
    } else if at >= edit.offset + removed {
        at + inserted - removed
    } else {
        edit.offset + inserted
    }
}

/// The views Save All saves (`SPEC-0261` v3 behaviour 3): every file of the main group, then the
/// files only the second group shows (`true`). A file in both groups is saved once, from the main
/// group; saving its twin as well would find the file "changed on disk" by the first save.
pub fn views_to_save<'a>(
    main: impl IntoIterator<Item = &'a PathBuf>,
    second: impl IntoIterator<Item = &'a PathBuf>,
) -> Vec<(PathBuf, bool)> {
    let mut views: Vec<(PathBuf, bool)> = main.into_iter().map(|p| (p.clone(), false)).collect();
    for p in second {
        if !views.iter().any(|(v, _)| v == p) {
            views.push((p.clone(), true));
        }
    }
    views
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0261 v3 behaviour 3: a file open in both groups is saved once, from the main group.
    #[test]
    fn a_file_in_both_groups_is_saved_once() {
        let (a, b, c) = (
            PathBuf::from("/p/a"),
            PathBuf::from("/p/b"),
            PathBuf::from("/p/c"),
        );
        assert_eq!(
            views_to_save([&a, &b], [&b, &c]),
            [(a.clone(), false), (b.clone(), false), (c.clone(), true)]
        );
        assert_eq!(views_to_save([], [&c]), [(c, true)]);
    }

    fn edit(offset: usize, deleted: &str, inserted: &str) -> Edit {
        Edit {
            offset,
            deleted: deleted.into(),
            inserted: inserted.into(),
        }
    }

    #[test]
    fn positions_move_with_edits_before_them() {
        let typed = edit(4, "", "xyz");
        assert_eq!(shift_offset(2, &typed), 2, "before the edit");
        assert_eq!(
            shift_offset(4, &typed),
            4,
            "at the insertion point it stays"
        );
        assert_eq!(
            shift_offset(9, &typed),
            12,
            "after it moves by the inserted length"
        );
        let removed = edit(4, "äbc", "");
        assert_eq!(shift_offset(9, &removed), 6);
        assert_eq!(
            shift_offset(5, &removed),
            4,
            "inside removed text it lands at the edit"
        );
        let replaced = edit(4, "abc", "z");
        assert_eq!(
            shift_offset(6, &replaced),
            5,
            "inside replaced text: after the new text"
        );
        assert_eq!(shift_offset(7, &replaced), 5);
    }
}
