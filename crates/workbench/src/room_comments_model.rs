//! The comments of the project room as a window shows them (`SPEC-0323` v8 behaviours 53–60):
//! threads per document and line, the gutter marks with their counts, anchors that move with the
//! text before the room answers again, who may comment, the text a person types made plain, and
//! the Add Comment entry of the editor's menu. Pure: the window feeds it the room's answers, the
//! texts of its documents and the time; nothing here touches a window or the network.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::context_menu::{MenuEntry, MenuItem};
use crate::room_model::{safe_doc, shown_name};

/// Add Comment on the caret's line: the editor's menu and Find Action (behaviour 54).
pub const ADD_COMMENT: &str = "Room.AddComment";
/// The most text in one comment, as the room keeps it (behaviour 57).
pub const MAX_TEXT: usize = 16 * 1024;
/// How long a document's text stands still before the window asks the room for its comments
/// again (behaviour 59), in milliseconds.
pub const REFRESH_QUIET_MS: u64 = 800;
/// The status line when the window cannot comment (behaviour 54).
pub const CANNOT_COMMENT: &str =
    "Comments need a project room and the role commenter or higher in it";

/// One comment as the room lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Id in the room.
    pub id: u64,
    /// Document, relative to the project folder.
    pub doc: String,
    /// Position in characters, moved with the text.
    pub at: usize,
    /// Author's name as shown (at most 40 characters, no control characters).
    pub name: String,
    /// The text, plain.
    pub text: String,
    /// Unix milliseconds.
    pub time: u64,
    /// Resolved (the thread's first comment speaks for the thread).
    pub resolved: bool,
    /// The first comment of the thread this one answers.
    pub reply_to: Option<u64>,
    /// Written by this window's person: only then it can be deleted here.
    pub own: bool,
}

impl Comment {
    /// A comment of `room.comments`; `None` when malformed or when its document leaves the
    /// project.
    pub fn from_json(v: &Value) -> Option<Self> {
        let doc = v["doc"].as_str()?;
        if !safe_doc(doc) {
            return None;
        }
        let reply_to = match &v["reply_to"] {
            Value::Null => None,
            r => Some(r.as_u64()?),
        };
        let mut text = v["text"].as_str()?.to_owned();
        if text.len() > MAX_TEXT {
            let mut cut = MAX_TEXT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        Some(Self {
            id: v["id"].as_u64()?,
            doc: doc.to_owned(),
            at: usize::try_from(v["at"].as_u64()?).ok()?,
            name: shown_name(v["name"].as_str().unwrap_or("")),
            text,
            time: v["time"].as_u64().unwrap_or(0),
            resolved: v["resolved"].as_bool().unwrap_or(false),
            reply_to,
            own: v["own"].as_bool().unwrap_or(false),
        })
    }
}

/// A thread: its first comment and the replies, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// The first comment; its id names the thread.
    pub first: Comment,
    /// The replies, oldest first.
    pub replies: Vec<Comment>,
}

impl Thread {
    /// The thread's id.
    pub fn id(&self) -> u64 {
        self.first.id
    }

    /// Whether the thread is resolved.
    pub fn resolved(&self) -> bool {
        self.first.resolved
    }

    /// Its comments, first and replies.
    pub fn comments(&self) -> impl Iterator<Item = &Comment> {
        std::iter::once(&self.first).chain(self.replies.iter())
    }

    /// The number of comments.
    pub fn len(&self) -> usize {
        1 + self.replies.len()
    }

    /// Never empty: a thread has its first comment.
    pub fn is_empty(&self) -> bool {
        false
    }
}

/// The gutter mark of one line (behaviour 53).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentMark {
    /// 0-based line.
    pub line: usize,
    /// Comments on the line, replies counted.
    pub comments: usize,
    /// Threads on the line.
    pub threads: usize,
    /// Some thread on the line is not resolved: the mark is drawn in the accent colour, else grey.
    pub open: bool,
}

impl CommentMark {
    /// The count drawn in the mark: none for a single comment, at most "9+".
    pub fn label(&self) -> Option<String> {
        match self.comments {
            0 | 1 => None,
            n if n > 9 => Some("9+".to_owned()),
            n => Some(n.to_string()),
        }
    }

    /// The mark's tooltip: "1 comment", "3 comments · 1 resolved thread".
    pub fn tooltip(&self) -> String {
        let count = if self.comments == 1 {
            "1 comment".to_owned()
        } else {
            format!("{} comments", self.comments)
        };
        if self.open {
            count
        } else if self.threads == 1 {
            format!("{count} · resolved")
        } else {
            format!("{count} · all resolved")
        }
    }
}

/// The comments a window knows, per document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comments {
    docs: BTreeMap<String, Vec<Comment>>,
}

impl Comments {
    /// The room's answer for `doc` (`room.comments` with `doc`) replaces what the window knew of
    /// it; entries of other documents and malformed ones are left out. Returns whether anything
    /// changed.
    pub fn set_doc(&mut self, doc: &str, answer: &Value) -> bool {
        let list: Vec<Comment> = answer["comments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Comment::from_json)
            .filter(|c| c.doc == doc)
            .collect();
        let old = self.docs.get(doc).cloned().unwrap_or_default();
        if old == list {
            return false;
        }
        if list.is_empty() {
            self.docs.remove(doc);
        } else {
            self.docs.insert(doc.to_owned(), list);
        }
        true
    }

    /// Forgets everything (the window left the room).
    pub fn clear(&mut self) {
        self.docs.clear();
    }

    /// Whether `doc` has comments.
    pub fn has(&self, doc: &str) -> bool {
        self.docs.contains_key(doc)
    }

    /// The comment `id` of `doc`.
    pub fn get(&self, doc: &str, id: u64) -> Option<&Comment> {
        self.docs.get(doc)?.iter().find(|c| c.id == id)
    }

    /// The threads of `doc` in the order of their places, then ids; replies whose first comment
    /// is missing are not shown.
    pub fn threads(&self, doc: &str) -> Vec<Thread> {
        let Some(all) = self.docs.get(doc) else {
            return Vec::new();
        };
        let mut threads: Vec<Thread> = all
            .iter()
            .filter(|c| c.reply_to.is_none())
            .map(|c| Thread {
                first: c.clone(),
                replies: Vec::new(),
            })
            .collect();
        threads.sort_by_key(|t| (t.first.at, t.first.id));
        let mut replies: Vec<&Comment> = all.iter().filter(|c| c.reply_to.is_some()).collect();
        replies.sort_by_key(|c| (c.time, c.id));
        for r in replies {
            if let Some(t) = threads.iter_mut().find(|t| Some(t.first.id) == r.reply_to) {
                t.replies.push(r.clone());
            }
        }
        threads
    }

    /// An edit of `doc`: `deleted` characters at `at` replaced by `inserted` ones. Every comment
    /// moves as the room moves it (behaviour 22), so the marks follow their lines at once.
    pub fn edit(&mut self, doc: &str, at: usize, deleted: usize, inserted: usize) {
        if let Some(all) = self.docs.get_mut(doc) {
            for c in all {
                c.at = shift(c.at, at, deleted, inserted);
            }
        }
    }

    /// The marks of `doc`, one per line with comments, in line order; `line_of` gives the 0-based
    /// line of a position (clamped to the text).
    pub fn marks(&self, doc: &str, line_of: impl Fn(usize) -> usize) -> Vec<CommentMark> {
        let mut by_line: BTreeMap<usize, CommentMark> = BTreeMap::new();
        for t in self.threads(doc) {
            let line = line_of(t.first.at);
            let m = by_line.entry(line).or_insert(CommentMark {
                line,
                comments: 0,
                threads: 0,
                open: false,
            });
            m.comments += t.len();
            m.threads += 1;
            m.open |= !t.resolved();
        }
        by_line.into_values().collect()
    }

    /// The threads of `doc` on the 0-based `line`.
    pub fn threads_on(
        &self,
        doc: &str,
        line: usize,
        line_of: impl Fn(usize) -> usize,
    ) -> Vec<Thread> {
        self.threads(doc)
            .into_iter()
            .filter(|t| line_of(t.first.at) == line)
            .collect()
    }
}

/// Where a position at `pos` goes when `deleted` characters at `at` are replaced by `inserted`
/// ones: after the change it moves by the difference, inside a deletion to its start, and text
/// inserted right at it pushes it along, as the room moves its anchors.
pub fn shift(pos: usize, at: usize, deleted: usize, inserted: usize) -> usize {
    if at + deleted <= pos {
        pos - deleted + inserted
    } else if at <= pos {
        at
    } else {
        pos
    }
}

/// The one change that turns `old` into `new`, in characters: where it starts, how many
/// characters went and how many came. `None` when the texts are equal.
pub fn text_change(old: &str, new: &str) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let rest = (a.len() - prefix).min(b.len() - prefix);
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take(rest)
        .take_while(|(x, y)| x == y)
        .count();
    Some((prefix, a.len() - prefix - suffix, b.len() - prefix - suffix))
}

/// Whether a role of the room may comment: `commenter` and above (behaviour 12).
pub fn can_comment(role: &str) -> bool {
    matches!(role, "commenter" | "editor" | "runner" | "admin")
}

/// Why typed text is not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    /// Nothing but white space.
    Empty,
    /// Over 16 384 bytes.
    TooLong,
}

impl InputError {
    /// The line the popup shows.
    pub fn text(self) -> &'static str {
        match self {
            Self::Empty => "Write a comment first",
            Self::TooLong => "A comment has at most 16384 bytes",
        }
    }
}

/// What the person typed, as the room takes it (behaviour 57): line ends as `\n`, no control
/// characters but line breaks and tabs, white space at both ends removed.
pub fn clean_input(text: &str) -> Result<String, InputError> {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let kept: String = unified
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let trimmed = kept.trim();
    if trimmed.is_empty() {
        return Err(InputError::Empty);
    }
    if trimmed.len() > MAX_TEXT {
        return Err(InputError::TooLong);
    }
    Ok(trimmed.to_owned())
}

/// A comment's text as the popup draws it: plain text, any control character but a line break
/// or a tab shown as U+FFFD. Never markup and never a link (behaviour 57).
pub fn shown_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

/// When a comment was written, as the popup says it: "just now", "5 min ago", "3 h ago", else
/// the day (UTC) as `YYYY-MM-DD`.
pub fn time_text(time: u64, now: u64) -> String {
    let age = now.saturating_sub(time) / 1000;
    match age {
        a if a < 60 => "just now".to_owned(),
        a if a < 3600 => format!("{} min ago", a / 60),
        a if a < 86_400 => format!("{} h ago", a / 3600),
        _ => {
            let (y, m, d) = civil_day((time / 86_400_000) as i64);
            format!("{y:04}-{m:02}-{d:02}")
        }
    }
}

/// Year, month and day of a day counted from 1970-01-01.
fn civil_day(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The editor's menu with Add Comment after Find Usages (behaviour 54): absent without a room
/// (`None`), disabled without the right role.
pub fn with_comment_entry(mut menu: Vec<MenuEntry>, room: Option<bool>) -> Vec<MenuEntry> {
    let Some(enabled) = room else {
        return menu;
    };
    let at = menu
        .iter()
        .position(|e| matches!(e, MenuEntry::Item(i) if i.action == "FindUsages"))
        .map_or(menu.len(), |i| i + 1);
    menu.insert(
        at,
        MenuEntry::Item(MenuItem {
            label: "Add Comment".to_owned(),
            action: ADD_COMMENT.to_owned(),
            enabled,
            checked: false,
        }),
    );
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn c(id: u64, at: usize, reply_to: Option<u64>, resolved: bool) -> Value {
        json!({
            "id": id, "doc": "src/a.rs", "at": at, "author": "ssh:ann", "name": "Ann",
            "text": format!("comment {id}"), "time": 1000 + id, "resolved": resolved,
            "reply_to": reply_to, "own": id == 1,
        })
    }

    /// Line starts of a text, for `line_of`.
    fn line_of(text: &str) -> impl Fn(usize) -> usize + '_ {
        move |at| {
            text.chars()
                .take(at.min(text.chars().count()))
                .filter(|c| *c == '\n')
                .count()
        }
    }

    /// SPEC-0323 v8 AK-32: threads per line with replies, counts and the open state.
    #[test]
    fn threads_group_per_line_with_counts() {
        let text = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let mut m = Comments::default();
        let answer = json!({"comments": [
            c(1, 10, None, false),
            c(2, 12, None, true),
            c(3, 0, Some(1), false),
            c(4, 0, Some(1), false),
            c(5, 25, None, true),
            c(6, 0, Some(99), false),
            {"id": 7, "doc": "../x", "at": 0, "name": "x", "text": "x"},
            {"id": 8, "doc": "src/a.rs", "at": "0", "name": "x", "text": "x"},
            {"id": 9, "doc": "src/b.rs", "at": 0, "name": "x", "text": "x"},
        ]});
        assert!(m.set_doc("src/a.rs", &answer));
        assert!(
            !m.set_doc("src/a.rs", &answer),
            "the same answer changes nothing"
        );
        let threads = m.threads("src/a.rs");
        assert_eq!(
            threads.iter().map(Thread::id).collect::<Vec<_>>(),
            vec![1, 2, 5],
            "a reply without its thread, a path leaving the project, a malformed place and \
             another document are left out"
        );
        assert_eq!(
            threads[0].replies.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(threads[0].first.own && !threads[0].replies[0].own);
        let marks = m.marks("src/a.rs", line_of(text));
        assert_eq!(
            marks,
            vec![
                CommentMark {
                    line: 1,
                    comments: 4,
                    threads: 2,
                    open: true
                },
                CommentMark {
                    line: 2,
                    comments: 1,
                    threads: 1,
                    open: false
                },
            ]
        );
        assert_eq!(marks[0].label().as_deref(), Some("4"));
        assert_eq!(marks[1].label(), None);
        assert_eq!(marks[0].tooltip(), "4 comments");
        assert_eq!(marks[1].tooltip(), "1 comment · resolved");
        let many = CommentMark {
            line: 0,
            comments: 12,
            threads: 2,
            open: false,
        };
        assert_eq!(many.label().as_deref(), Some("9+"));
        assert_eq!(many.tooltip(), "12 comments · all resolved");
        assert_eq!(
            m.threads_on("src/a.rs", 1, line_of(text))
                .iter()
                .map(Thread::id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(m.threads_on("src/a.rs", 0, line_of(text)).is_empty());
        // A place past the end stands on the last line.
        m.set_doc("src/a.rs", &json!({"comments": [c(1, 999, None, false)]}));
        assert_eq!(m.marks("src/a.rs", line_of(text))[0].line, 3);
        assert!(m.set_doc("src/a.rs", &json!({"comments": []})));
        assert!(!m.has("src/a.rs"));
    }

    /// SPEC-0323 v8 AK-32: anchors move with edits as the room moves them.
    #[test]
    fn anchors_move_with_edits() {
        // After the change: moved by the difference; inside a deletion: to its start; text put
        // right at it pushes it along; before it: unchanged.
        assert_eq!(shift(10, 0, 0, 5), 15);
        assert_eq!(shift(10, 10, 0, 3), 13);
        assert_eq!(shift(10, 4, 3, 0), 7);
        assert_eq!(shift(10, 8, 5, 1), 8);
        assert_eq!(shift(10, 11, 2, 0), 10);
        let text = "one\ntwo\nthree\n";
        let mut m = Comments::default();
        m.set_doc(
            "src/a.rs",
            &json!({"comments": [c(1, 4, None, false), c(2, 8, None, false)]}),
        );
        // Two lines inserted at the top: both comments go down two lines.
        let new = "zero\nhalf\none\ntwo\nthree\n";
        let (at, del, ins) = text_change(text, new).unwrap();
        assert_eq!((at, del, ins), (0, 0, 10));
        m.edit("src/a.rs", at, del, ins);
        let lines: Vec<usize> = m
            .marks("src/a.rs", line_of(new))
            .iter()
            .map(|k| k.line)
            .collect();
        assert_eq!(lines, vec![3, 4]);
        // The line "two" deleted: its comment moves to the start of the deletion, the line
        // after it.
        let after = "zero\nhalf\none\nthree\n";
        let (at, del, ins) = text_change(new, after).unwrap();
        m.edit("src/a.rs", at, del, ins);
        assert_eq!(
            m.marks("src/a.rs", line_of(after))
                .iter()
                .map(|k| (k.line, k.comments))
                .collect::<Vec<_>>(),
            vec![(3, 2)]
        );
        assert_eq!(text_change("same", "same"), None);
        assert_eq!(text_change("abcd", "abXd"), Some((2, 1, 1)));
        assert_eq!(text_change("aaa", "aaaa"), Some((3, 0, 1)));
        assert_eq!(
            text_change("ä€x", "ä€"),
            Some((2, 1, 0)),
            "characters, not bytes"
        );
    }

    /// SPEC-0323 v8 AK-33: who may comment, plain text within the limit, times, the menu entry.
    #[test]
    fn roles_text_times_and_the_menu() {
        for role in ["commenter", "editor", "runner", "admin"] {
            assert!(can_comment(role), "{role}");
        }
        for role in ["viewer", "guest", "", "Admin"] {
            assert!(!can_comment(role), "{role}");
        }
        assert_eq!(
            clean_input("  a\r\nb\rc\u{1b}[31m\t \n").unwrap(),
            "a\nb\nc[31m"
        );
        assert_eq!(clean_input(" \n\t"), Err(InputError::Empty));
        assert_eq!(
            clean_input(&"x".repeat(MAX_TEXT + 1)),
            Err(InputError::TooLong)
        );
        assert!(clean_input(&"x".repeat(MAX_TEXT)).is_ok());
        assert_eq!(
            shown_text("<b>[x](https://e.com)</b>\u{7}"),
            "<b>[x](https://e.com)</b>\u{fffd}"
        );
        let day = 86_400_000;
        let t = 1_795_000_000_000;
        assert_eq!(time_text(t, t + 30_000), "just now");
        assert_eq!(time_text(t, t + 5 * 60_000), "5 min ago");
        assert_eq!(time_text(t, t + 3 * 3_600_000), "3 h ago");
        assert_eq!(time_text(0, 2 * day), "1970-01-01");
        assert_eq!(
            time_text(1_795_000_000_000, 1_795_000_000_000 + 3 * day),
            "2026-11-18"
        );
        assert_eq!(time_text(951_782_400_000, u64::MAX), "2000-02-29");
        let menu = vec![
            MenuEntry::Item(MenuItem {
                label: "Find Usages".into(),
                action: "FindUsages".into(),
                enabled: true,
                checked: false,
            }),
            MenuEntry::Separator,
        ];
        assert_eq!(with_comment_entry(menu.clone(), None), menu);
        let with = with_comment_entry(menu.clone(), Some(false));
        assert!(
            matches!(&with[1], MenuEntry::Item(i) if i.label == "Add Comment" && i.action == ADD_COMMENT && !i.enabled)
        );
        assert!(
            matches!(&with_comment_entry(menu, Some(true))[1], MenuEntry::Item(i) if i.enabled)
        );
        // A long text from the room is cut on a character boundary.
        let long = format!("{}é", "x".repeat(MAX_TEXT - 1));
        let v = json!({"id": 1, "doc": "a", "at": 0, "name": "\u{1b}Ann", "text": long});
        let parsed = Comment::from_json(&v).unwrap();
        assert_eq!(parsed.text.len(), MAX_TEXT - 1);
        assert_eq!(parsed.name, "Ann");
    }
}
