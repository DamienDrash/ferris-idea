//! The main author of each declaration of a file from its blame at HEAD (`SPEC-0292` v4
//! behaviours 7–7b): most committed lines win, `+N` other authors, `*` when a line of it changed.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::diff::{LineChange, line_changes};
use crate::git::HeadBlame;

/// Longest author name drawn before it is cut with `…` (behaviour 7c).
pub const AUTHOR_CHARS: usize = 24;

/// A declaration and the lines it covers, as the editor's grammar finds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationLines {
    /// The declared name.
    pub name: String,
    /// `class`, `interface`, `method` or `function`.
    pub kind: String,
    /// 0-based line of the name, where the hint stands.
    pub line: usize,
    /// 0-based first line of the whole declaration.
    pub start: usize,
    /// 0-based last line of the whole declaration.
    pub end: usize,
}

/// The author hint of one declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodeAuthor {
    /// The declared name.
    pub name: String,
    /// `class`, `interface`, `method` or `function`.
    pub kind: String,
    /// 0-based line of the name.
    pub line: usize,
    /// 0-based first line of the declaration.
    pub start: usize,
    /// 0-based last line of the declaration.
    pub end: usize,
    /// The main author's name.
    pub author: String,
    /// The main author's e-mail address.
    pub email: String,
    /// Other authors with at least one committed line of it.
    pub others: usize,
    /// A line of it differs from HEAD.
    pub changed: bool,
    /// The hint as drawn, e.g. `Anna Muster +1 *`.
    pub label: String,
}

/// `Name`, `Name +N`, `Name *` or `Name +N *`, the name cut after [`AUTHOR_CHARS`] chars
/// (behaviour 7c).
pub fn author_label(author: &str, others: usize, changed: bool) -> String {
    let mut out: String = if author.chars().count() > AUTHOR_CHARS {
        let mut cut: String = author.chars().take(AUTHOR_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        author.to_owned()
    };
    if others > 0 {
        out.push_str(&format!(" +{others}"));
    }
    if changed {
        out.push_str(" *");
    }
    out
}

/// Whether one of `changes` lies within the 0-based lines `start..=end`; a deletion counts when its
/// boundary lies between two of these lines (behaviour 7b).
pub fn changed_within(changes: &[LineChange], start: usize, end: usize) -> bool {
    changes.iter().any(|c| {
        if c.new_end >= c.new_start {
            c.new_start - 1 <= end && c.new_end > start
        } else {
            let below = c.new_start - 1;
            below > start && below <= end
        }
    })
}

/// The author hints of `declarations` in `text`, whose blame at HEAD is `blame` (behaviour 7b).
/// Declarations without a committed line are left out.
pub fn declaration_authors(
    blame: &HeadBlame,
    text: &str,
    declarations: &[DeclarationLines],
) -> Vec<CodeAuthor> {
    let per_line = blame.map(text);
    let changes = line_changes(&blame.head_text, text);
    let mut out = Vec::new();
    for d in declarations {
        // (name, email) → (lines, newest commit time)
        let mut by: BTreeMap<(&str, &str), (usize, i64)> = BTreeMap::new();
        for c in per_line
            .iter()
            .take(d.end + 1)
            .skip(d.start)
            .flatten()
            .filter_map(|&c| blame.commits.get(c as usize))
        {
            let e = by
                .entry((c.info.author.as_str(), c.info.email.as_str()))
                .or_insert((0, i64::MIN));
            e.0 += 1;
            e.1 = e.1.max(c.info.time);
        }
        let Some((&(author, email), _)) =
            by.iter().max_by_key(|&(_, &(lines, time))| (lines, time))
        else {
            continue;
        };
        let others = by.len() - 1;
        let changed = changed_within(&changes, d.start, d.end);
        out.push(CodeAuthor {
            name: d.name.clone(),
            kind: d.kind.clone(),
            line: d.line,
            start: d.start,
            end: d.end,
            author: author.to_owned(),
            email: email.to_owned(),
            others,
            changed,
            label: author_label(author, others, changed),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{BlameCommit, CommitInfo};

    fn commit(author: &str, time: i64) -> BlameCommit {
        BlameCommit {
            info: CommitInfo {
                id: format!("{author}{time}"),
                short_id: "abc1234".into(),
                summary: "s".into(),
                author: author.into(),
                email: format!(
                    "{}@example.invalid",
                    author.to_lowercase().replace(' ', ".")
                ),
                time,
                parents: Vec::new(),
                committer: None,
            },
            message: "s".into(),
        }
    }

    fn decl(name: &str, line: usize, start: usize, end: usize) -> DeclarationLines {
        DeclarationLines {
            name: name.into(),
            kind: "method".into(),
            line,
            start,
            end,
        }
    }

    // SPEC-0292 v4 AK-8.
    #[test]
    fn main_author_others_and_changed_lines() {
        let head =
            "class A {\n  void a() {\n    x();\n    y();\n  }\n  void b() {\n    z();\n  }\n}\n";
        let blame = HeadBlame {
            path: "A.java".into(),
            head_text: head.into(),
            // Anna wrote the class and a(); Bert wrote y() and b().
            lines: vec![
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(0),
                Some(1),
                Some(1),
                Some(1),
                Some(0),
            ],
            commits: vec![commit("Anna Muster", 10), commit("Bert Beispiel", 20)],
        };
        let decls = [decl("A", 0, 0, 8), decl("a", 1, 1, 4), decl("b", 5, 5, 7)];
        let same = declaration_authors(&blame, head, &decls);
        let labels: Vec<&str> = same.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Anna Muster +1", "Anna Muster +1", "Bert Beispiel"]
        );
        assert_eq!(same[0].email, "anna.muster@example.invalid");
        assert!(same.iter().all(|a| !a.changed));
        // An edited line of b() marks b only; a deletion inside a() marks a.
        let edited = head.replace("z();", "z(1);").replace("    y();\n", "");
        let after = declaration_authors(
            &blame,
            &edited,
            &[decl("A", 0, 0, 7), decl("a", 1, 1, 3), decl("b", 4, 4, 6)],
        );
        let labels: Vec<&str> = after.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Anna Muster +1 *", "Anna Muster *", "Bert Beispiel *"]
        );
        // A declaration with no committed line has no author; a tie goes to the newer commit.
        let added = format!("{head}void c() {{}}\n");
        assert!(declaration_authors(&blame, &added, &[decl("c", 9, 9, 9)]).is_empty());
        let tie = declaration_authors(&blame, head, &[decl("t", 3, 3, 4)]);
        assert_eq!(tie[0].author, "Bert Beispiel");
    }

    // SPEC-0292 v4 AK-8.
    #[test]
    fn labels_and_cutting() {
        assert_eq!(author_label("Anna", 0, false), "Anna");
        assert_eq!(author_label("Anna", 2, true), "Anna +2 *");
        let long = "Maximiliane Mustermann-Beispiel";
        let cut = author_label(long, 0, false);
        assert_eq!(cut.chars().count(), AUTHOR_CHARS);
        assert!(cut.ends_with('…'));
        // A deletion at the declaration's first line lies above it, at its end inside it.
        let deletion = |at: usize| LineChange {
            kind: crate::LineChangeKind::Deleted,
            new_start: at + 1,
            new_end: at,
            old_start: at + 1,
            old_end: at + 1,
        };
        assert!(!changed_within(&[deletion(2)], 2, 4));
        assert!(changed_within(&[deletion(4)], 2, 4));
        assert!(!changed_within(&[deletion(5)], 2, 4));
    }
}
