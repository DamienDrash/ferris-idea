//! Context packs for agents (`SPEC-0339`, ADR-0023 rule 4): the pieces of context about a target,
//! ranked in a fixed total order and cut to a character budget, deterministically. Gathering the
//! pieces (language servers, syntax trees, Git) is the MCP tool's part; this module holds the
//! model, the ranking, the budget, the versions and where a project's rules files are.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Longest content of one piece in characters before it is cut (`SPEC-0339` behaviour 8).
pub const MAX_PIECE_CHARS: usize = 4000;

/// Characters per estimated token (`SPEC-0339` behaviour 2).
pub const CHARS_PER_TOKEN: usize = 4;

/// Most rules files in a pack (`SPEC-0339` behaviour 7).
pub const MAX_RULES_FILES: usize = 10;

/// The kind of a piece (`SPEC-0339` behaviour 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceKind {
    /// The declaration around the position.
    Symbol,
    /// The declarations of a file target.
    Outline,
    /// The language server's hover text.
    Signature,
    /// One finding of the target file.
    Diagnostic,
    /// A direct caller.
    Caller,
    /// A function a test calls.
    Callee,
    /// An implementation or override.
    Implementation,
    /// A related test file.
    Test,
    /// A project rules file.
    Rule,
    /// A further reference.
    Usage,
    /// A recent commit.
    Change,
}

impl PieceKind {
    /// Every kind, in priority order.
    pub const ALL: [PieceKind; 11] = [
        PieceKind::Symbol,
        PieceKind::Outline,
        PieceKind::Signature,
        PieceKind::Diagnostic,
        PieceKind::Caller,
        PieceKind::Callee,
        PieceKind::Implementation,
        PieceKind::Test,
        PieceKind::Rule,
        PieceKind::Usage,
        PieceKind::Change,
    ];

    /// The name in answers and in `include`.
    pub fn name(self) -> &'static str {
        match self {
            PieceKind::Symbol => "symbol",
            PieceKind::Outline => "outline",
            PieceKind::Signature => "signature",
            PieceKind::Diagnostic => "diagnostic",
            PieceKind::Caller => "caller",
            PieceKind::Callee => "callee",
            PieceKind::Implementation => "implementation",
            PieceKind::Test => "test",
            PieceKind::Rule => "rule",
            PieceKind::Usage => "usage",
            PieceKind::Change => "change",
        }
    }

    /// The kind called `name`.
    pub fn from_name(name: &str) -> Option<PieceKind> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The priority of behaviour 5: lower comes first.
    pub fn priority(self) -> u8 {
        match self {
            PieceKind::Symbol | PieceKind::Outline | PieceKind::Signature => 0,
            PieceKind::Diagnostic => 1,
            PieceKind::Caller | PieceKind::Callee => 2,
            PieceKind::Implementation => 3,
            PieceKind::Test => 4,
            PieceKind::Rule => 5,
            PieceKind::Usage => 6,
            PieceKind::Change => 7,
        }
    }
}

/// Where a piece came from (`SPEC-0339` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// The MCP tool whose logic produced it.
    pub tool: &'static str,
    /// Workspace-relative, `/`-separated path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// `text` for usages found as text occurrences (`SPEC-0339` behaviour 14).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
}

/// One part of a sort key within a kind (`SPEC-0339` behaviour 10).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// A number.
    N(u64),
    /// A text, compared bytewise.
    S(String),
}

/// A piece before ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Stable id: `<kind>:<path>:<line>`, `<kind>:<path>` or `change:<commit id>`.
    pub id: String,
    /// Kind.
    pub kind: PieceKind,
    /// Source.
    pub source: Source,
    /// Version of the source (`SPEC-0339` behaviour 12).
    pub version: String,
    /// Text.
    pub content: String,
    /// Sort key within the kind.
    pub order: Vec<Key>,
    /// The source's text is withheld (`SPEC-0339` behaviour 15): never included.
    pub withheld: bool,
}

impl Candidate {
    /// A piece about `path`, at the 1-based `line` when given, with the id of behaviour 4.
    pub fn at(
        kind: PieceKind,
        tool: &'static str,
        path: &str,
        line: Option<u64>,
        version: String,
        content: String,
    ) -> Self {
        let id = match line {
            Some(l) => format!("{}:{path}:{l}", kind.name()),
            None => format!("{}:{path}", kind.name()),
        };
        Candidate {
            id,
            kind,
            source: Source {
                tool,
                path: Some(path.to_owned()),
                line,
                mode: None,
            },
            version,
            content,
            order: Vec::new(),
            withheld: false,
        }
    }

    /// With the sort key `order`.
    pub fn ordered(mut self, order: Vec<Key>) -> Self {
        self.order = order;
        self
    }
}

/// An included piece (`SPEC-0339` behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Piece {
    /// Stable id.
    pub id: String,
    /// Kind.
    pub kind: PieceKind,
    /// 1-based place in the ranking of all candidates.
    pub rank: usize,
    /// Source.
    pub source: Source,
    /// Version of the source.
    pub version: String,
    /// Length of `content` in Unicode scalar values.
    pub chars: usize,
    /// The content was cut.
    pub truncated: bool,
    /// Always true: project content, never instructions (ADR-0023 rule 7).
    pub untrusted: bool,
    /// Text.
    pub content: String,
}

/// A piece left out (`SPEC-0339` behaviours 11 and 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Omitted {
    /// Stable id.
    pub id: String,
    /// Kind.
    pub kind: PieceKind,
    /// Source.
    pub source: Source,
    /// Its length after the cut of behaviour 8 (0 when withheld).
    pub chars: usize,
    /// `budget` or `withheld`.
    pub reason: &'static str,
}

/// The ranked and budgeted pieces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assembled {
    /// Included pieces in rank order.
    pub pieces: Vec<Piece>,
    /// Pieces left out, in rank order.
    pub omitted: Vec<Omitted>,
    /// Characters of the included content.
    pub used_chars: usize,
    /// Version of the pack (`SPEC-0339` behaviour 12).
    pub version: String,
}

/// `sha256:` and the first 16 hex digits of the SHA-256 of `bytes` (`SPEC-0339` behaviour 12).
pub fn content_version(bytes: &[u8]) -> String {
    hex16(&Sha256::digest(bytes))
}

fn hex16(digest: &[u8]) -> String {
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// `text` cut to at most `max` characters at the last line break before that length, or at the
/// length when there is none (`SPEC-0339` behaviour 8); whether it was cut.
pub fn cut(text: &str, max: usize) -> (String, bool) {
    let Some((end, _)) = text.char_indices().nth(max) else {
        return (text.to_owned(), false);
    };
    let head = &text[..end];
    match head.rfind('\n') {
        Some(at) if at > 0 => (head[..at].to_owned(), true),
        _ => (head.to_owned(), true),
    }
}

/// Ranks `candidates` (`SPEC-0339` behaviour 10), keeps each id once, cuts long pieces
/// (behaviour 8) and takes them while they fit into `budget` characters (behaviour 11).
pub fn assemble(mut candidates: Vec<Candidate>, budget: usize) -> Assembled {
    candidates.sort_by(|a, b| {
        (a.kind.priority(), &a.order, &a.id).cmp(&(b.kind.priority(), &b.order, &b.id))
    });
    let mut seen = HashSet::new();
    candidates.retain(|c| seen.insert(c.id.clone()));
    let mut pieces = Vec::new();
    let mut omitted = Vec::new();
    let mut used = 0usize;
    for (i, c) in candidates.into_iter().enumerate() {
        if c.withheld {
            omitted.push(Omitted {
                id: c.id,
                kind: c.kind,
                source: c.source,
                chars: 0,
                reason: "withheld",
            });
            continue;
        }
        let (mut content, mut truncated) = cut(&c.content, MAX_PIECE_CHARS);
        let mut chars = content.chars().count();
        if used + chars > budget {
            if !pieces.is_empty() {
                omitted.push(Omitted {
                    id: c.id,
                    kind: c.kind,
                    source: c.source,
                    chars,
                    reason: "budget",
                });
                continue;
            }
            // The first piece is cut to the budget rather than left out.
            (content, _) = cut(&content, budget);
            truncated = true;
            chars = content.chars().count();
        }
        used += chars;
        pieces.push(Piece {
            id: c.id,
            kind: c.kind,
            rank: i + 1,
            source: c.source,
            version: c.version,
            chars,
            truncated,
            untrusted: true,
            content,
        });
    }
    let mut h = Sha256::new();
    for p in &pieces {
        for part in [&p.id, &p.version, &p.content] {
            h.update(part.as_bytes());
            h.update([0]);
        }
    }
    Assembled {
        pieces,
        omitted,
        used_chars: used,
        version: hex16(&h.finalize()),
    }
}

/// The project rules files for the workspace file `relative` (`SPEC-0339` behaviour 9):
/// `AGENTS.md` and `CLAUDE.md` from its folder up to `root`, nearest first, then the Markdown files
/// directly in `.ferris-idea/rules/` by name. Only regular files (no links); at most
/// [`MAX_RULES_FILES`]; the second value counts the rest. Paths are workspace-relative.
pub fn rules_files(root: &Path, relative: &str) -> (Vec<String>, usize) {
    let regular = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file());
    let mut found = Vec::new();
    let mut folder = Path::new(relative).parent().map(Path::to_path_buf);
    while let Some(dir) = folder {
        for name in ["AGENTS.md", "CLAUDE.md"] {
            let rel = dir.join(name);
            if regular(&root.join(&rel)) {
                found.push(slash(&rel));
            }
        }
        folder = if dir.as_os_str().is_empty() {
            None
        } else {
            Some(dir.parent().map(Path::to_path_buf).unwrap_or_default())
        };
    }
    let rules = Path::new(".ferris-idea").join("rules");
    let mut extra: Vec<PathBuf> = std::fs::read_dir(root.join(&rules))
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "md") && regular(p))
                .filter_map(|p| p.file_name().map(|n| rules.join(n)))
                .collect()
        })
        .unwrap_or_default();
    extra.sort();
    found.extend(extra.iter().map(|p| slash(p)));
    let more = found.len().saturating_sub(MAX_RULES_FILES);
    found.truncate(MAX_RULES_FILES);
    (found, more)
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(kind: PieceKind, path: &str, line: u64, content: &str) -> Candidate {
        Candidate::at(
            kind,
            "test",
            path,
            Some(line),
            "v".into(),
            content.to_owned(),
        )
        .ordered(vec![Key::S(path.into()), Key::N(line)])
    }

    /// SPEC-0339 AK-3: the fixed kind order, then the kind's own key, then the id.
    #[test]
    fn ranked_in_kind_order() {
        let a = assemble(
            vec![
                cand(PieceKind::Change, "a", 1, "c"),
                cand(PieceKind::Usage, "b", 2, "u2"),
                cand(PieceKind::Usage, "a", 9, "u1"),
                cand(PieceKind::Rule, "AGENTS.md", 1, "r"),
                cand(PieceKind::Diagnostic, "a", 3, "d"),
                cand(PieceKind::Symbol, "a", 1, "s"),
                cand(PieceKind::Caller, "x", 1, "k"),
                cand(PieceKind::Implementation, "x", 1, "i"),
                cand(PieceKind::Test, "t", 1, "t"),
            ],
            10_000,
        );
        let order: Vec<&str> = a.pieces.iter().map(|p| p.content.as_str()).collect();
        assert_eq!(order, ["s", "d", "k", "i", "t", "r", "u1", "u2", "c"]);
        assert_eq!(
            a.pieces.iter().map(|p| p.rank).collect::<Vec<_>>(),
            (1..=9).collect::<Vec<_>>()
        );
        assert!(a.pieces.iter().all(|p| p.untrusted));
        assert!(a.version.starts_with("sha256:") && a.version.len() == 23);
    }

    /// SPEC-0339 AK-3: a piece that does not fit is listed, a smaller later one still fits, and the
    /// budget is never exceeded.
    #[test]
    fn budget_skips_and_keeps_smaller() {
        let a = assemble(
            vec![
                cand(PieceKind::Symbol, "a", 1, &"s".repeat(60)),
                cand(PieceKind::Diagnostic, "a", 2, &"d".repeat(50)),
                cand(PieceKind::Usage, "a", 3, &"u".repeat(30)),
            ],
            100,
        );
        assert_eq!(a.pieces.len(), 2);
        assert_eq!(a.pieces[1].kind, PieceKind::Usage);
        assert_eq!(a.pieces[1].rank, 3);
        assert_eq!(a.used_chars, 90);
        assert_eq!(a.omitted.len(), 1);
        assert_eq!(a.omitted[0].reason, "budget");
        assert_eq!(a.omitted[0].chars, 50);
        assert_eq!(a.omitted[0].id, "diagnostic:a:2");
    }

    /// SPEC-0339 AK-3: the first piece is cut to the budget instead of being left out.
    #[test]
    fn first_piece_is_cut_not_dropped() {
        let text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let a = assemble(vec![cand(PieceKind::Symbol, "a", 1, &text)], 50);
        assert_eq!(a.pieces.len(), 1);
        assert!(a.pieces[0].truncated);
        assert!(a.pieces[0].chars <= 50);
        assert!(a.pieces[0].content.ends_with("line 6"));
        assert!(a.used_chars <= 50);
    }

    /// SPEC-0339 AK-3: long pieces are cut at a line break and marked.
    #[test]
    fn long_pieces_cut_at_line_break() {
        let long = format!("{}\n{}", "a".repeat(3990), "b".repeat(100));
        let (text, cut_it) = cut(&long, MAX_PIECE_CHARS);
        assert!(cut_it);
        assert_eq!(text, "a".repeat(3990));
        let (one, cut_it) = cut(&"x".repeat(5000), MAX_PIECE_CHARS);
        assert!(cut_it);
        assert_eq!(one.chars().count(), MAX_PIECE_CHARS);
        assert_eq!(cut("äö\nü", 10), ("äö\nü".to_owned(), false));
        let a = assemble(vec![cand(PieceKind::Usage, "a", 1, &long)], 100_000);
        assert!(a.pieces[0].truncated);
        assert_eq!(a.pieces[0].chars, 3990);
    }

    /// SPEC-0339 AK-3: the same id is kept once, the first in rank order; withheld pieces are
    /// listed without content.
    #[test]
    fn duplicates_kept_once() {
        let mut w = cand(PieceKind::Usage, ".env", 1, "SECRET");
        w.withheld = true;
        let a = assemble(
            vec![
                cand(PieceKind::Usage, "a", 1, "second"),
                cand(PieceKind::Usage, "a", 1, "second again"),
                w,
            ],
            1000,
        );
        assert_eq!(a.pieces.len(), 1);
        assert_eq!(a.omitted.len(), 1);
        assert_eq!(a.omitted[0].reason, "withheld");
        assert_eq!(a.omitted[0].chars, 0);
        assert_eq!(a.pieces[0].content, "second");
        assert_eq!(content_version(b"x"), content_version(b"x"));
        assert_ne!(content_version(b"x"), content_version(b"y"));
    }

    /// SPEC-0339 behaviour 9: nearest folder first, `.ferris-idea/rules/*.md` by name, no links.
    #[test]
    fn rules_files_in_order() {
        let root = std::env::temp_dir().join(format!("ferris-idea-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join(".ferris-idea/rules")).unwrap();
        for f in [
            "AGENTS.md",
            "CLAUDE.md",
            "src/AGENTS.md",
            ".ferris-idea/rules/b.md",
            ".ferris-idea/rules/a.md",
            ".ferris-idea/rules/notes.txt",
        ] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", root.join("src/deep/CLAUDE.md")).unwrap();
        let (files, more) = rules_files(&root, "src/deep/x.rs");
        assert_eq!(
            files,
            [
                "src/AGENTS.md",
                "AGENTS.md",
                "CLAUDE.md",
                ".ferris-idea/rules/a.md",
                ".ferris-idea/rules/b.md"
            ]
        );
        assert_eq!(more, 0);
        let (top, _) = rules_files(&root, "x.rs");
        assert_eq!(top[0], "AGENTS.md");
        let _ = std::fs::remove_dir_all(&root);
    }
}
