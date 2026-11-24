//! The author hints of an editor (`SPEC-0292` v4 behaviours 7–7d), free of the UI toolkit: one
//! hint per declaration with the lines it covers, moved by edits until the next read, and the `*`
//! of a declaration taken from the current change markers.

use ferris_idea_vcs::{CodeAuthor, LineChange, author_label, changed_within};

/// One declaration's author as the editor keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorHint {
    /// 0-based line of the declaration's name, where the hint is drawn.
    pub line: usize,
    /// 0-based first line of the declaration.
    pub start: usize,
    /// 0-based last line of the declaration.
    pub end: usize,
    /// The main author.
    pub author: String,
    /// Other authors.
    pub others: usize,
}

/// The author hints of an editor, sorted by line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorHints {
    hints: Vec<AuthorHint>,
}

impl AuthorHints {
    /// Hints from a fresh read (behaviour 7d); one per line, the outermost declaration first.
    pub fn from_authors(authors: &[CodeAuthor]) -> Self {
        let mut hints: Vec<AuthorHint> = authors
            .iter()
            .map(|a| AuthorHint {
                line: a.line,
                start: a.start,
                end: a.end,
                author: a.author.clone(),
                others: a.others,
            })
            .collect();
        hints.sort_by_key(|h| h.line);
        hints.dedup_by_key(|h| h.line);
        Self { hints }
    }

    /// The hints.
    pub fn hints(&self) -> &[AuthorHint] {
        &self.hints
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }

    /// The text drawn at the end of `line`, with `*` when one of `changes` lies within its
    /// declaration (behaviours 7b–7d).
    pub fn label_at(&self, line: usize, changes: &[LineChange]) -> Option<String> {
        let i = self.hints.binary_search_by_key(&line, |h| h.line).ok()?;
        let h = &self.hints[i];
        Some(author_label(
            &h.author,
            h.others,
            changed_within(changes, h.start, h.end),
        ))
    }

    /// Moves the hints after an edit that turned `removed` lines starting at `line` into `added`
    /// lines: hints below shift, a hint whose line was replaced goes, a hint on the edited line
    /// stays, and declarations around the edit grow or shrink with it (behaviour 7d).
    pub fn edited(&mut self, line: usize, removed: usize, added: usize) {
        let last = line + removed;
        let shift = |x: usize| {
            if x > last {
                x + added - removed
            } else if x > line {
                x.min(line + added)
            } else {
                x
            }
        };
        self.hints.retain(|h| h.line <= line || h.line > last);
        for h in &mut self.hints {
            h.line = shift(h.line);
            h.start = shift(h.start);
            h.end = shift(h.end).max(h.line);
        }
    }

    /// Forgets every hint.
    pub fn clear(&mut self) {
        self.hints.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_vcs::LineChangeKind;

    fn author(line: usize, start: usize, end: usize, who: &str, others: usize) -> CodeAuthor {
        CodeAuthor {
            name: "x".into(),
            kind: "method".into(),
            line,
            start,
            end,
            author: who.into(),
            email: "a@example.invalid".into(),
            others,
            changed: false,
            label: String::new(),
        }
    }

    fn modified(line: usize) -> LineChange {
        LineChange {
            kind: LineChangeKind::Modified,
            new_start: line + 1,
            new_end: line + 1,
            old_start: line + 1,
            old_end: line + 1,
        }
    }

    // SPEC-0292 v4 AK-8: labels with `*` from the markers, moved by edits.
    #[test]
    fn hints_follow_edits_and_markers() {
        let mut h = AuthorHints::from_authors(&[
            author(2, 2, 20, "Anna Muster", 1),
            author(5, 4, 8, "Bert Beispiel", 0),
            author(12, 12, 14, "Anna Muster", 0),
        ]);
        assert_eq!(h.label_at(2, &[]).as_deref(), Some("Anna Muster +1"));
        assert_eq!(h.label_at(3, &[]), None);
        // A change in line 6 marks the method around it and the class, not the last method.
        let changes = [modified(6)];
        assert_eq!(h.label_at(5, &changes).as_deref(), Some("Bert Beispiel *"));
        assert_eq!(h.label_at(2, &changes).as_deref(), Some("Anna Muster +1 *"));
        assert_eq!(h.label_at(12, &changes).as_deref(), Some("Anna Muster"));
        // Two lines inserted in line 6: the method grows, the hints below move.
        h.edited(6, 0, 2);
        let lines: Vec<(usize, usize, usize)> =
            h.hints().iter().map(|x| (x.line, x.start, x.end)).collect();
        assert_eq!(lines, [(2, 2, 22), (5, 4, 10), (14, 14, 16)]);
        // An edit on a hint's line keeps it; deleting the lines of another drops it.
        h.edited(5, 0, 0);
        assert!(h.label_at(5, &[]).is_some());
        h.edited(13, 2, 0);
        assert_eq!(h.hints().len(), 2);
        h.clear();
        assert!(h.is_empty());
    }
}
