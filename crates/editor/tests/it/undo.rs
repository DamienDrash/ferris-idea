//! Undo and redo through the editor commands, case by case (`SPEC-0003` behaviours 6–8,
//! `SPEC-0057` behaviours 9, 10, 12 and 18, `SPEC-0025` behaviour 5, `SPEC-0026` behaviour 5): what
//! one step takes back, where it stops, and that no step shows a text that was never on screen.

use std::collections::HashSet;

use ferris_idea_editor::reformat::reformat_edits;
use ferris_idea_editor::{Carets, EditorState, Reconcile, perform, type_text};

fn typed(s: &mut EditorState, text: &str) {
    text.chars().for_each(|c| type_text(s, &c.to_string()));
}

fn at_end(text: &str) -> EditorState {
    let mut s = EditorState::new(text);
    s.carets = Carets::new(text.chars().count());
    s
}

fn undo(s: &mut EditorState) -> String {
    assert!(perform(s, "$Undo"));
    s.buffer.text()
}

fn redo(s: &mut EditorState) -> String {
    assert!(perform(s, "$Redo"));
    s.buffer.text()
}

/// Applies a formatter's result the way the editor does: small edits, last first, in one group.
fn reformat(s: &mut EditorState, formatted: &str) {
    let old = s.buffer.text();
    s.buffer.begin_group();
    for (range, text) in reformat_edits(&old, formatted).into_iter().rev() {
        s.buffer
            .replace(range, &text)
            .expect("edit inside the text");
    }
    s.buffer.end_group();
}

#[test]
fn edits_are_undone_and_redone_step_by_step() {
    let mut s = at_end("one");
    typed(&mut s, " two");
    assert!(perform(&mut s, "EditorEnter"));
    typed(&mut s, "three");
    assert!(perform(&mut s, "EditorDeleteLine"));
    assert_eq!(
        s.buffer.text(),
        "one two",
        "the last line goes with its break"
    );
    assert_eq!(undo(&mut s), "one two\nthree");
    assert_eq!(undo(&mut s), "one two\n");
    assert_eq!(undo(&mut s), "one two");
    assert_eq!(undo(&mut s), "one");
    assert_eq!(undo(&mut s), "one", "nothing is left to undo");
    assert!(!s.buffer.is_modified());
    assert_eq!(redo(&mut s), "one two");
    assert_eq!(redo(&mut s), "one two\n");
    assert_eq!(redo(&mut s), "one two\nthree");
    assert_eq!(redo(&mut s), "one two");
    assert_eq!(redo(&mut s), "one two", "nothing is left to redo");
}

#[test]
fn a_new_edit_after_undo_drops_the_redo_steps() {
    let mut s = at_end("a");
    typed(&mut s, "b");
    assert!(perform(&mut s, "EditorEnter"));
    assert_eq!(undo(&mut s), "ab");
    typed(&mut s, "c");
    assert_eq!(
        redo(&mut s),
        "abc",
        "the undone line break does not come back"
    );
    assert_eq!(undo(&mut s), "ab");
    assert_eq!(undo(&mut s), "a");
    assert_eq!(redo(&mut s), "ab");
    assert_eq!(redo(&mut s), "abc");
}

#[test]
fn undo_and_redo_end_a_run_of_typing() {
    let mut s = at_end("");
    typed(&mut s, "ab");
    assert_eq!(undo(&mut s), "");
    assert_eq!(redo(&mut s), "ab");
    // The caret is where the typing ended, but the redone run is closed.
    typed(&mut s, "cd");
    assert_eq!(undo(&mut s), "ab");
    assert_eq!(undo(&mut s), "");
}

#[test]
fn an_edit_at_several_carets_is_one_step() {
    let mut s = EditorState::new("x\nx\nx\n");
    s.carets = Carets::new(1);
    s.carets.add_caret(3);
    s.carets.add_caret(5);
    typed(&mut s, "yz");
    assert_eq!(s.buffer.text(), "xyz\nxyz\nxyz\n");
    assert!(perform(&mut s, "EditorBackSpace"));
    assert_eq!(s.buffer.text(), "xy\nxy\nxy\n");
    assert_eq!(undo(&mut s), "xyz\nxyz\nxyz\n");
    assert_eq!(
        undo(&mut s),
        "x\nx\nx\n",
        "typing at three carets is one run"
    );
    assert_eq!(redo(&mut s), "xyz\nxyz\nxyz\n");
    assert_eq!(redo(&mut s), "xy\nxy\nxy\n");
}

#[test]
fn formatting_is_one_step() {
    let before = "fn main(){\nlet a=1;\n\n\nlet b = 2;\n}\n";
    let after = "fn main() {\n    let a = 1;\n\n    let b = 2;\n}\n";
    let mut s = at_end(before);
    typed(&mut s, "// x");
    let unformatted = s.buffer.text();
    reformat(&mut s, &format!("{after}// x"));
    assert_eq!(s.buffer.text(), format!("{after}// x"));
    assert_eq!(undo(&mut s), unformatted, "all regions come back at once");
    assert_eq!(redo(&mut s), format!("{after}// x"));
    assert_eq!(undo(&mut s), unformatted);
    assert_eq!(undo(&mut s), before, "the typing before it is its own step");
}

#[test]
fn undo_stops_at_the_saved_state_and_can_go_past_it() {
    let mut s = at_end("");
    typed(&mut s, "ab");
    s.buffer.mark_saved();
    typed(&mut s, "cd");
    assert!(s.buffer.is_modified());
    assert_eq!(undo(&mut s), "ab", "the run is cut at the save");
    assert!(!s.buffer.is_modified(), "back at what is on disk");
    assert_eq!(undo(&mut s), "", "undo goes past the save");
    assert!(s.buffer.is_modified(), "the disk still has `ab`");
    assert_eq!(redo(&mut s), "ab");
    assert!(!s.buffer.is_modified());
    assert_eq!(redo(&mut s), "abcd");
    assert!(s.buffer.is_modified());
    // The same text typed again is not the saved state.
    assert_eq!(undo(&mut s), "ab");
    assert_eq!(undo(&mut s), "");
    typed(&mut s, "ab");
    assert!(s.buffer.is_modified());
}

#[test]
fn an_external_change_is_one_step_and_can_be_undone() {
    let mut s = at_end("one\ntwo\nthree\n");
    s.carets = Carets::new(3);
    typed(&mut s, "!");
    s.buffer.mark_saved();
    let disk = "one!\nTWO\nthree\nfour\n";
    assert_eq!(s.buffer.reconcile(disk), Reconcile::Reload);
    s.buffer.reload(disk);
    assert!(!s.buffer.is_modified());
    assert_eq!(
        undo(&mut s),
        "one!\ntwo\nthree\n",
        "the text before the reload"
    );
    assert!(s.buffer.is_modified(), "the disk has the other text");
    assert_eq!(undo(&mut s), "one\ntwo\nthree\n");
    assert_eq!(redo(&mut s), "one!\ntwo\nthree\n");
    assert_eq!(redo(&mut s), disk);
    assert!(!s.buffer.is_modified());
    // Typing right after a reload never joins it.
    typed(&mut s, "x");
    assert_eq!(undo(&mut s), disk);
    // With unsaved edits the disk text is not taken.
    typed(&mut s, "y");
    assert_eq!(s.buffer.reconcile("other\n"), Reconcile::Conflict);
}

/// A small generator without a dependency (xorshift), so the walk below is the same on every run.
struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Data safety: whatever is typed, deleted, pasted, formatted, saved, reloaded, undone and redone,
/// undo and redo only ever show a text that was on screen before, and a buffer that says it is
/// unmodified holds exactly what was last saved or loaded.
#[test]
fn undo_and_redo_only_show_texts_that_were_on_screen() {
    for seed in 1..=40u64 {
        let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut s = at_end("alpha beta\ngamma\n");
        let mut seen: HashSet<String> = HashSet::from([s.buffer.text()]);
        let mut disk = s.buffer.text();
        let mut log: Vec<String> = Vec::new();
        for _ in 0..300 {
            let name = match random.below(16) {
                0..=3 => {
                    let c = ['a', ' ', '(', '"', 'z', '\n'][random.below(6)];
                    type_text(&mut s, &c.to_string());
                    format!("type {c:?}")
                }
                4 | 5 => {
                    let a = [
                        "EditorBackSpace",
                        "$Delete",
                        "EditorDeleteLine",
                        "EditorDuplicate",
                    ][random.below(4)];
                    perform(&mut s, a);
                    a.to_owned()
                }
                6 => {
                    let a = ["EditorLeft", "EditorUp", "EditorLineEnd", "EditorTextStart"]
                        [random.below(4)];
                    perform(&mut s, a);
                    a.to_owned()
                }
                7 => {
                    let n = s.buffer.len_chars();
                    s.carets = Carets::new(random.below(n + 1));
                    s.carets.add_caret(random.below(n + 1));
                    "two carets".to_owned()
                }
                8 => {
                    perform(&mut s, "EditorSelectWord");
                    perform(&mut s, "$Copy");
                    perform(&mut s, "EditorLineEnd");
                    perform(&mut s, "$Paste");
                    "copy and paste".to_owned()
                }
                9 => {
                    let formatted = s.buffer.text().replace("  ", " ").replace('a', "A");
                    reformat(&mut s, &formatted);
                    s.carets = Carets::new(s.buffer.len_chars());
                    "format".to_owned()
                }
                10 => {
                    s.buffer.mark_saved();
                    disk = s.buffer.text();
                    "save".to_owned()
                }
                11 => {
                    // Another program changes the file; an unmodified editor takes it.
                    let changed = format!("{disk}external {seed}\n");
                    if s.buffer.reconcile(&changed) == Reconcile::Reload {
                        s.buffer.reload(&changed);
                        disk = changed;
                        s.carets = Carets::new(s.buffer.len_chars());
                    }
                    "external change".to_owned()
                }
                12 | 13 => {
                    let before = s.buffer.text();
                    perform(&mut s, "$Undo");
                    let now = s.buffer.text();
                    assert!(
                        seen.contains(&now),
                        "seed {seed}: undo showed a text that was never on screen\nbefore: \
                         {before:?}\nnow: {now:?}\nlog: {log:?}"
                    );
                    "undo".to_owned()
                }
                _ => {
                    let before = s.buffer.text();
                    perform(&mut s, "$Redo");
                    let now = s.buffer.text();
                    assert!(
                        seen.contains(&now),
                        "seed {seed}: redo showed a text that was never on screen\nbefore: \
                         {before:?}\nnow: {now:?}\nlog: {log:?}"
                    );
                    "redo".to_owned()
                }
            };
            log.push(name);
            // The carets undo and redo put back lie inside the text (SPEC-0057 v7 behaviour 9).
            let len = s.buffer.len_chars();
            assert!(
                s.carets
                    .carets()
                    .iter()
                    .all(|c| c.anchor <= len && c.head <= len),
                "seed {seed}: a caret outside the text\nlog: {log:?}"
            );
            seen.insert(s.buffer.text());
            if !s.buffer.is_modified() {
                assert_eq!(
                    s.buffer.text(),
                    disk,
                    "seed {seed}: unmodified, but not the saved text\nlog: {log:?}"
                );
            }
        }
        // All the way back and forth again ends on the same text.
        let end = s.buffer.text();
        let mut steps = 0;
        while s.buffer.undo() {
            steps += 1;
        }
        for _ in 0..steps {
            assert!(s.buffer.redo());
        }
        assert_eq!(s.buffer.text(), end, "seed {seed}");
    }
}

/// SPEC-0003 v3 behaviour 10 (AK-11, IMP-0406): the history is not cut after a number of steps.
#[test]
fn the_history_is_not_cut_after_a_number_of_steps() {
    const STEPS: usize = 1500;
    let mut s = at_end("start");
    for i in 0..STEPS {
        // Enter is a step of its own, so every line is one step.
        assert!(perform(&mut s, "EditorEnter"));
        if i % 500 == 0 {
            s.buffer.mark_saved();
        }
    }
    let end = s.buffer.text();
    assert_eq!(end.matches('\n').count(), STEPS);
    for left in (0..STEPS).rev() {
        assert_eq!(undo(&mut s).matches('\n').count(), left);
    }
    assert_eq!(s.buffer.text(), "start", "down to the text it started with");
    assert_eq!(undo(&mut s), "start", "nothing is left to undo");
    for _ in 0..STEPS {
        assert!(s.buffer.redo());
    }
    assert_eq!(s.buffer.text(), end);
    assert!(!s.buffer.redo());
}
