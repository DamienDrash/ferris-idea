//! Navigation history for Back and Forward (`SPEC-0071` behaviour 4) and the column a
//! declaration is asked for (v5 behaviour 7), without toolkit.

use std::path::{Path, PathBuf};

/// Most places remembered.
pub const NAV_LIMIT: usize = 100;

/// A place in a file: path and 0-based line and column.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    /// Absolute path.
    pub path: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column.
    pub column: usize,
}

/// Places left by jumps; Back walks to older ones, Forward returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavHistory {
    back: Vec<Place>,
    forward: Vec<Place>,
}

impl NavHistory {
    /// Records `from` before a jump; a new jump clears the Forward list. A place on the same line
    /// as the last one is not recorded twice.
    pub fn jumped(&mut self, from: Place) {
        let same = self
            .back
            .last()
            .is_some_and(|l| l.path == from.path && l.line == from.line);
        if !same {
            self.back.push(from);
            if self.back.len() > NAV_LIMIT {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    /// The place to go back to from `here`, which becomes the first Forward place.
    pub fn back(&mut self, here: Place) -> Option<Place> {
        let to = self.back.pop()?;
        self.forward.push(here);
        Some(to)
    }

    /// The place to go forward to from `here`, which goes back on the Back list.
    pub fn forward(&mut self, here: Place) -> Option<Place> {
        let to = self.forward.pop()?;
        self.back.push(here);
        Some(to)
    }

    /// Drops places in a file that no longer exists or was removed from the workspace.
    pub fn forget(&mut self, path: &Path) {
        self.back.retain(|p| p.path != path);
        self.forward.retain(|p| p.path != path);
    }
}

/// Whether opening `target` from the Project tree or Recent Files is a jump that records the
/// caret's place (`SPEC-0071` v13 behaviour 4a): only when an editor is active and shows another
/// file.
pub fn opening_is_a_jump(active: Option<&Path>, target: &Path) -> bool {
    active.is_some_and(|a| a != target)
}

/// Whether `c` belongs to a name in the languages the servers serve.
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The column of `line` to ask the language server about for a caret at `column` (characters):
/// one to the left when the caret stands right after a name and not on one (`person|.greet()`,
/// `Person|` at the end of the line), else `column` (`SPEC-0071` v5 behaviour 7).
pub fn symbol_column(line: &str, column: usize) -> usize {
    let mut chars = line.chars().skip(column.saturating_sub(1));
    let before = (column > 0).then(|| chars.next()).flatten();
    let at = chars.next();
    match (before, at) {
        (Some(b), at) if name_char(b) && !at.is_some_and(name_char) => column - 1,
        _ => column,
    }
}

/// `places` with repeated places left out, in their order (`SPEC-0071` v6 behaviour 8).
pub fn distinct(places: Vec<Place>) -> Vec<Place> {
    let mut seen = std::collections::HashSet::new();
    places
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// The places a ⌘B-like jump goes to from `here` (`SPEC-0071` v6 behaviour 8): for a definition
/// or the usages (`whole_line`) a place on the caret's own line is not a jump, for types and
/// implementations only the caret's own place is skipped; a place named twice counts once.
pub fn jump_targets(found: Vec<Place>, here: &Place, whole_line: bool) -> Vec<Place> {
    distinct(
        found
            .into_iter()
            .filter(|t| {
                if whole_line {
                    !(t.path == here.path && t.line == here.line)
                } else {
                    t != here
                }
            })
            .collect(),
    )
}

/// Whether the caret at `here` stands on the declaration itself: the server found definitions and
/// all of them lie on the caret's line (`SPEC-0071` v7 behaviour 9).
pub fn on_declaration(definitions: &[Place], here: &Place) -> bool {
    !definitions.is_empty()
        && definitions
            .iter()
            .all(|d| d.path == here.path && d.line == here.line)
}

/// Most places Recent Locations keeps (`SPEC-0069` v2 behaviour 7).
pub const RECENT_PLACES_LIMIT: usize = 50;

/// A place the user was at, when, and whether text was changed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentPlace {
    /// The place.
    pub place: Place,
    /// When it was last visited or changed.
    pub at: std::time::SystemTime,
    /// Whether text was changed there.
    pub edited: bool,
}

/// The places of Recent Locations, newest first (`SPEC-0069` v2 behaviour 7): a visit within a
/// few lines of a known place in the same file replaces it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentPlaces {
    places: Vec<RecentPlace>,
}

impl RecentPlaces {
    /// Records `place` at `at`; `edited` marks a change of text there (kept once set).
    pub fn visit(&mut self, place: Place, at: std::time::SystemTime, edited: bool) {
        let near =
            |p: &RecentPlace| p.place.path == place.path && p.place.line.abs_diff(place.line) <= 3;
        let was_edited = self.places.iter().any(|p| near(p) && p.edited);
        self.places.retain(|p| !near(p));
        self.places.insert(
            0,
            RecentPlace {
                place,
                at,
                edited: edited || was_edited,
            },
        );
        self.places.truncate(RECENT_PLACES_LIMIT);
    }

    /// The places, newest first, without the one at `here` and, with `edited_only`, only the
    /// changed ones.
    pub fn list(&self, here: Option<&Place>, edited_only: bool) -> Vec<RecentPlace> {
        self.places
            .iter()
            .filter(|p| {
                here.is_none_or(|h| !(h.path == p.place.path && h.line.abs_diff(p.place.line) <= 3))
            })
            .filter(|p| !edited_only || p.edited)
            .cloned()
            .collect()
    }

    /// Drops the places of a file that is gone.
    pub fn forget(&mut self, path: &Path) {
        self.places.retain(|p| p.place.path != path);
    }
}

/// How long ago `then` was, in words like the reference's list: "just now", "2 minutes ago",
/// "3 hours ago", "yesterday" (`SPEC-0069` v2 behaviour 8).
pub fn ago(then: std::time::SystemTime, now: std::time::SystemTime) -> String {
    let s = now.duration_since(then).map_or(0, |d| d.as_secs());
    match s {
        0..=59 => "just now".into(),
        60..=119 => "a minute ago".into(),
        120..=3599 => format!("{} minutes ago", s / 60),
        3600..=7199 => "an hour ago".into(),
        7200..=86_399 => format!("{} hours ago", s / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    /// Hand test 209 (D59): ⌘B on `Person` in `new Person(…)` on line 2 jumps to the constructor
    /// on line 10 of the same file, also when the server names it twice; a place on the caret's
    /// own line is no jump.
    #[test]
    fn a_constructor_in_the_same_file_is_a_jump() {
        use super::{Place, jump_targets};
        let file = std::path::PathBuf::from("/p/Main.java");
        let at = |line, column| Place {
            path: file.clone(),
            line,
            column,
        };
        let here = at(2, 28);
        let found = vec![at(10, 4), at(10, 4), at(2, 20)];
        assert_eq!(jump_targets(found.clone(), &here, true), [at(10, 4)]);
        // For types and implementations only the caret's own place goes.
        assert_eq!(
            jump_targets(vec![at(2, 28), at(2, 20)], &here, false),
            [at(2, 20)]
        );
        assert!(jump_targets(vec![at(2, 5)], &here, true).is_empty());
    }

    /// SPEC-0069 v2 AK-5: recent places merge nearby visits, keep the edit mark, leave out here.
    #[test]
    fn recent_places_merge_and_filter() {
        use super::*;
        let t0 = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let place = |f: &str, line: usize| Place {
            path: PathBuf::from(f),
            line,
            column: 0,
        };
        let mut r = RecentPlaces::default();
        r.visit(place("a", 10), t0, true);
        r.visit(place("b", 1), t0, false);
        r.visit(place("a", 12), t0, false);
        let all = r.list(None, false);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].place, place("a", 12));
        assert!(all[0].edited, "the edit mark survives a nearby visit");
        assert_eq!(r.list(Some(&place("a", 11)), false).len(), 1);
        assert_eq!(r.list(None, true).len(), 1);
        r.forget(Path::new("a"));
        assert_eq!(r.list(None, false).len(), 1);
        let s = |n| t0 + std::time::Duration::from_secs(n);
        assert_eq!(ago(t0, s(5)), "just now");
        assert_eq!(ago(t0, s(90)), "a minute ago");
        assert_eq!(ago(t0, s(600)), "10 minutes ago");
        assert_eq!(ago(t0, s(10_000)), "2 hours ago");
        assert_eq!(ago(t0, s(100_000)), "yesterday");
    }

    use super::*;

    /// SPEC-0071 v7 AK-6.
    #[test]
    fn only_the_caret_line_means_a_declaration() {
        let p = |path: &str, line| Place {
            path: path.into(),
            line,
            column: 4,
        };
        let here = p("/w/A.java", 6);
        assert!(on_declaration(&[p("/w/A.java", 6)], &here));
        assert!(!on_declaration(&[], &here));
        assert!(!on_declaration(&[p("/w/A.java", 7)], &here));
        assert!(!on_declaration(
            &[p("/w/A.java", 6), p("/w/B.java", 6)],
            &here
        ));
    }

    #[test]
    fn a_place_named_twice_is_one_target() {
        let p = |line| Place {
            path: "/w/greeter.rb".into(),
            line,
            column: 6,
        };
        assert_eq!(distinct(vec![p(1), p(1), p(4), p(1)]), [p(1), p(4)]);
    }

    #[test]
    fn the_name_before_the_caret_counts() {
        let line = "        System.out.println(person.greet());";
        let person = line.find("person").unwrap();
        // Inside and at the start of a name: unchanged.
        assert_eq!(symbol_column(line, person + 2), person + 2);
        assert_eq!(symbol_column(line, person), person);
        // Right after `person`, before the dot: the name.
        assert_eq!(symbol_column(line, person + 6), person + 5);
        // After `;` at the end of the line, and at the start of the line: unchanged.
        assert_eq!(symbol_column(line, line.len()), line.len());
        assert_eq!(symbol_column("Person", 6), 5);
        assert_eq!(symbol_column("x;", 2), 2);
        assert_eq!(symbol_column("", 0), 0);
        assert_eq!(symbol_column("  a", 0), 0);
        // Characters, not bytes.
        assert_eq!(symbol_column("größe ", 5), 4);
    }

    fn at(file: &str, line: usize) -> Place {
        Place {
            path: PathBuf::from(file),
            line,
            column: 0,
        }
    }

    #[test]
    fn back_forward_and_new_jumps() {
        let mut h = NavHistory::default();
        h.jumped(at("a.rs", 1));
        h.jumped(at("a.rs", 1));
        h.jumped(at("b.rs", 5));
        assert_eq!(h.back(at("c.rs", 9)), Some(at("b.rs", 5)));
        assert_eq!(
            h.back(at("b.rs", 5)),
            Some(at("a.rs", 1)),
            "duplicate line kept once"
        );
        assert_eq!(h.back(at("a.rs", 1)), None);
        assert_eq!(h.forward(at("a.rs", 1)), Some(at("b.rs", 5)));
        h.jumped(at("b.rs", 5));
        assert_eq!(h.forward(at("x.rs", 0)), None, "a new jump clears Forward");
        h.forget(Path::new("b.rs"));
        assert_eq!(h.back(at("x.rs", 0)), Some(at("a.rs", 1)));
    }

    /// SPEC-0071 AK-15 (v13, behaviour 4a): opening another file records the place left; Back
    /// returns there and Forward to the opened file at its caret.
    #[test]
    fn opening_another_file_is_a_jump() {
        let main = Path::new("/p/Main.java");
        let inventory = Path::new("/p/Inventory.java");
        assert!(opening_is_a_jump(Some(main), inventory));
        assert!(
            !opening_is_a_jump(Some(main), main),
            "the active file again"
        );
        assert!(!opening_is_a_jump(None, inventory), "no active editor");

        let mut h = NavHistory::default();
        let left = Place {
            path: main.to_path_buf(),
            line: 0,
            column: 0,
        };
        h.jumped(left.clone());
        let opened = Place {
            path: inventory.to_path_buf(),
            line: 3,
            column: 4,
        };
        assert_eq!(h.back(opened.clone()), Some(left.clone()));
        assert_eq!(h.forward(left), Some(opened));
    }
}
