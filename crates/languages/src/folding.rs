//! What a file folds to (`SPEC-0083` v8), shared by the editor and MCP: custom regions from
//! comment markers, the text shown in place of a folded region, the outermost container that
//! Collapse All leaves open, nesting levels and the documentation comments. The regions
//! themselves come from the syntax tree (`SPEC-0030`, [`SyntaxDocument::folding`]).

use crate::syntax::{FoldRegion, SyntaxDocument};

/// The three dots that stand for hidden text (`SPEC-0083` v8 behaviour 21).
pub const ELLIPSIS: &str = "...";

/// A comment that starts or ends a custom region (`SPEC-0083` v8 behaviour 19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionMarker {
    /// The start, with the region's name (may be empty).
    Start(String),
    /// The end.
    End,
}

/// The comment signs a marker may stand behind, longest first.
const COMMENT_OPENERS: [&str; 7] = ["<!--", "/*", "//", "--", "#", ";", "%"];

/// Whether `comment`, the text of a comment on one line, is a region marker: `region Name` /
/// `endregion` behind the comment sign (with or without a space, also as `#region`), or
/// `<editor-fold desc="Name">` / `</editor-fold>` (`SPEC-0083` v8 behaviour 19).
pub fn region_marker(comment: &str) -> Option<RegionMarker> {
    let mut text = comment.trim();
    if text.contains('\n') {
        return None;
    }
    let opener = COMMENT_OPENERS.iter().find(|o| text.starts_with(**o))?;
    text = &text[opener.len()..];
    for closer in ["-->", "*/"] {
        if let Some(rest) = text.strip_suffix(closer) {
            text = rest;
        }
    }
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("<editor-fold") {
        if !rest.starts_with(|c: char| c.is_whitespace() || c == '>') {
            return None;
        }
        let name = rest
            .split_once("desc=\"")
            .and_then(|(_, after)| after.split_once('"'))
            .map_or("", |(name, _)| name);
        return Some(RegionMarker::Start(name.trim().to_owned()));
    }
    if text.starts_with("</editor-fold>") {
        return Some(RegionMarker::End);
    }
    // `// #region` and, in a language whose comments start with `#`, `#region` and `# region`.
    let text = text.strip_prefix('#').unwrap_or(text);
    let word = |keyword: &str| {
        text.strip_prefix(keyword)
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    };
    if word("endregion").is_some() {
        return Some(RegionMarker::End);
    }
    word("region").map(|name| RegionMarker::Start(name.trim().to_owned()))
}

/// Pairs region markers given in the order of their lines: an end closes the nearest open start
/// before it. A marker without a partner gives nothing (`SPEC-0083` v8 behaviour 19). Returns
/// first line, last line and name of every pair that spans more than one line, sorted.
pub fn pair_markers(markers: &[(usize, RegionMarker)]) -> Vec<(usize, usize, String)> {
    let mut open: Vec<(usize, &str)> = Vec::new();
    let mut pairs = Vec::new();
    for (line, marker) in markers {
        match marker {
            RegionMarker::Start(name) => open.push((*line, name)),
            RegionMarker::End => {
                if let Some((start, name)) = open.pop()
                    && *line > start
                {
                    pairs.push((start, *line, name.to_owned()));
                }
            }
        }
    }
    pairs.sort();
    pairs
}

/// A custom region of a file, for the chooser of `GotoCustomRegion` (`SPEC-0083` v9
/// behaviour 24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomRegion {
    /// 0-based line of the marker that starts it.
    pub start_line: usize,
    /// 0-based line of the marker that ends it.
    pub end_line: usize,
    /// The name behind the start marker; empty for a region without one.
    pub name: String,
}

impl CustomRegion {
    /// The row of the chooser: the name (`Unnamed region` without one) and the 1-based line.
    pub fn label(&self) -> String {
        let name = if self.name.is_empty() {
            "Unnamed region"
        } else {
            &self.name
        };
        format!("{name}  (line {})", self.start_line + 1)
    }
}

/// The custom regions among the folding regions of a file, in the order of their lines: the
/// same regions that fold (behaviour 19), so a marker without a partner and a region that
/// crosses a block are not listed either.
pub fn custom_regions(regions: &[FoldRegion]) -> Vec<CustomRegion> {
    let mut found: Vec<CustomRegion> = regions
        .iter()
        .filter(|r| r.kind == "region")
        .map(|r| CustomRegion {
            start_line: r.start_line,
            end_line: r.end_line,
            name: if r.collapsed_text == ELLIPSIS {
                String::new()
            } else {
                r.collapsed_text.clone()
            },
        })
        .collect();
    found.sort_by_key(|r| (r.start_line, r.end_line));
    found
}

/// The region the chooser selects first: the innermost one around `line`, else the first that
/// starts behind it, else the last one; `None` without regions.
pub fn region_at(regions: &[CustomRegion], line: usize) -> Option<usize> {
    let around = regions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.start_line <= line && line <= r.end_line)
        .min_by_key(|(_, r)| r.end_line - r.start_line)
        .map(|(i, _)| i);
    around
        .or_else(|| regions.iter().position(|r| r.start_line > line))
        .or(regions.len().checked_sub(1))
}

/// The first sentence of a documentation comment whose lines, without the comment signs, are
/// `lines` (`SPEC-0083` v8 behaviour 21): the text up to the first full stop that ends a word,
/// or else up to the first empty line or tag line (`@param`); empty when there is no text.
pub fn first_sentence<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for line in lines {
        let line = line.trim().trim_start_matches('*').trim();
        if line.is_empty() || line.starts_with('@') {
            if text.is_empty() && !line.starts_with('@') {
                continue;
            }
            break;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(line);
    }
    let mut end = text.len();
    for (at, _) in text.match_indices('.') {
        let after = &text[at + 1..];
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            end = at + 1;
            break;
        }
    }
    text.truncate(end);
    text
}

/// What stands in place of a comment over several lines whose text is `comment` and whose
/// first line starts at char column `column`: whether it documents, how many chars of its first
/// line stay, and the placeholder (`SPEC-0083` v8 behaviour 21).
pub fn comment_look(comment: &str, column: usize) -> (bool, Option<usize>, String) {
    if let Some(body) = comment
        .strip_prefix("/**")
        .and_then(|rest| rest.strip_suffix("*/"))
        && !body.is_empty()
    {
        let sentence = first_sentence(body.lines());
        let text = if sentence.is_empty() {
            format!("{ELLIPSIS}*/")
        } else {
            format!(" {sentence} {ELLIPSIS}*/")
        };
        return (true, Some(column + 3), text);
    }
    for (open, close) in [("/*", "*/"), ("<!--", "-->")] {
        if comment.starts_with(open) && comment.ends_with(close) {
            return (
                false,
                Some(column + open.chars().count()),
                format!("{ELLIPSIS}{close}"),
            );
        }
    }
    (false, None, ELLIPSIS.to_owned())
}

/// As [`comment_look`], for a run of comments of one line each, `comments`: lines that all
/// start with `///` or `//!` document and show their first sentence; other runs keep their
/// comment sign.
pub fn comment_run_look(comments: &[&str], column: usize) -> (bool, Option<usize>, String) {
    let doc_sign = |c: &str| {
        ["///", "//!"]
            .into_iter()
            .find(|sign| c.starts_with(sign) && !c.starts_with("////"))
    };
    if !comments.is_empty() && comments.iter().all(|c| doc_sign(c).is_some()) {
        let sentence = first_sentence(comments.iter().map(|c| &c[3..]));
        let text = if sentence.is_empty() {
            ELLIPSIS.to_owned()
        } else {
            format!(" {sentence} {ELLIPSIS}")
        };
        return (true, Some(column + 3), text);
    }
    let first = comments.first().copied().unwrap_or_default();
    match ["//", "--", "#", ";", "%"]
        .into_iter()
        .find(|sign| first.starts_with(sign))
    {
        Some(sign) => (false, Some(column + sign.len()), ELLIPSIS.to_owned()),
        None => (false, None, ELLIPSIS.to_owned()),
    }
}

/// The placeholder of a block from `first`, the text of its first line, to `last`, the text of
/// its last line: three dots and, when the first line ends with an opening bracket and the last
/// one starts with its partner, that partner — the block reads `{...}` (`SPEC-0083` v8
/// behaviour 21).
pub fn block_placeholder(first: &str, last: &str) -> String {
    let closer = match first.trim_end().chars().next_back() {
        Some('{') => '}',
        Some('[') => ']',
        Some('(') => ')',
        _ => return ELLIPSIS.to_owned(),
    };
    if last.trim_start().starts_with(closer) {
        format!("{ELLIPSIS}{closer}")
    } else {
        ELLIPSIS.to_owned()
    }
}

/// A region as its 0-based first and last line.
pub type Lines = (usize, usize);

/// How a region looks while it is folded (`SPEC-0083` v8 behaviour 21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// 0-based first line of the region.
    pub start: usize,
    /// 0-based last line.
    pub end: usize,
    /// How many chars of the first line stay in front of the placeholder; `None`: the whole line.
    pub keep: Option<usize>,
    /// The placeholder.
    pub text: String,
}

/// The foldable regions of a file and what the folding actions do with them (`SPEC-0083` v8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    /// The regions as first and last line, sorted, one per first line.
    pub lines: Vec<(usize, usize)>,
    /// The outermost container of the file, which Collapse All leaves open and which is no
    /// nesting level (behaviour 18).
    pub outermost: Option<(usize, usize)>,
    /// The documentation comments (behaviour 20).
    pub docs: Vec<(usize, usize)>,
    /// How each region looks while folded, in the order of `lines`.
    pub looks: Vec<Look>,
}

impl Outline {
    /// The outline of a parsed file. One-line Java methods keep the statement as their
    /// placeholder (`SPEC-0083` v2 behaviour 7).
    pub fn of(doc: &SyntaxDocument) -> Self {
        let mut outline = Self::from_regions(&doc.folding());
        for body in doc.one_line_bodies() {
            if let Some(look) = outline
                .looks
                .iter_mut()
                .find(|l| (l.start, l.end) == (body.start_line, body.end_line))
            {
                look.keep = None;
                look.text = body.placeholder;
            }
        }
        outline
    }

    /// The outline of `regions`, sorted with one region per first line.
    pub fn from_regions(regions: &[FoldRegion]) -> Self {
        let span = |r: &FoldRegion| (r.start_line, r.end_line);
        Self {
            lines: regions.iter().map(span).collect(),
            outermost: regions.iter().find(|r| r.outermost).map(span),
            docs: regions.iter().filter(|r| r.doc).map(span).collect(),
            looks: regions
                .iter()
                .map(|r| Look {
                    start: r.start_line,
                    end: r.end_line,
                    keep: r.keep,
                    text: r.collapsed_text.clone(),
                })
                .collect(),
        }
    }

    /// The regions without the outermost container.
    fn inner(&self) -> Vec<(usize, usize)> {
        self.lines
            .iter()
            .copied()
            .filter(|r| Some(*r) != self.outermost)
            .collect()
    }

    /// The regions Collapse All folds, an outer one before those inside it: every region but
    /// the outermost container (behaviour 18).
    pub fn collapse_all(&self) -> Vec<(usize, usize)> {
        self.inner()
    }

    /// The regions "Expand All to Level `level`" folds: those with `level` regions around
    /// them, the outermost container not counted (behaviours 12 and 18).
    pub fn all_to_level(&self, level: usize) -> Vec<(usize, usize)> {
        level_folds(&self.inner(), level, None)
    }

    /// The region "Expand to Level `level`" works in at `line` — the innermost around it — and
    /// the regions it folds there (behaviour 12).
    pub fn to_level(&self, level: usize, line: usize) -> Option<(Lines, Vec<Lines>)> {
        let within = innermost_around(&self.lines, line)?;
        Some((within, level_folds(&self.lines, level, Some(within))))
    }

    /// The look of the region `start..=end`, if it is one of the outline.
    pub fn look(&self, start: usize, end: usize) -> Option<&Look> {
        self.looks.iter().find(|l| (l.start, l.end) == (start, end))
    }
}

/// Of `regions`, those that lie in no other of them: what is seen folded when all of them are
/// folded.
pub fn outermost_of(regions: &[(usize, usize)]) -> Vec<(usize, usize)> {
    regions
        .iter()
        .copied()
        .filter(|&(s, e)| {
            !regions
                .iter()
                .any(|&(os, oe)| (os, oe) != (s, e) && os <= s && e <= oe)
        })
        .collect()
}

/// The level of every region of `regions` (sorted by first line, one per first line): one more than
/// the number of regions that contain it (`SPEC-0083` v6 behaviour 12).
pub fn levels(regions: &[(usize, usize)]) -> Vec<usize> {
    let mut open: Vec<usize> = Vec::new();
    regions
        .iter()
        .map(|&(_, e)| {
            while open.last().is_some_and(|&end| end < e) {
                open.pop();
            }
            open.push(e);
            open.len()
        })
        .collect()
}

/// The innermost region of `regions` around `line` (its first and last line included).
pub fn innermost_around(regions: &[(usize, usize)], line: usize) -> Option<(usize, usize)> {
    regions
        .iter()
        .copied()
        .filter(|&(s, e)| s <= line && line <= e)
        .min_by_key(|&(s, e)| e - s)
}

/// The regions to fold so that the regions down to `level` stay open (`SPEC-0083` v6 behaviour
/// 12): those of level `level + 1`. With `within`, levels count from that region (level 1) and only
/// regions inside it are given.
pub fn level_folds(
    regions: &[(usize, usize)],
    level: usize,
    within: Option<(usize, usize)>,
) -> Vec<(usize, usize)> {
    let all = levels(regions);
    let base = match within {
        Some(w) => match regions.iter().position(|r| *r == w) {
            Some(i) => all[i] - 1,
            None => return Vec::new(),
        },
        None => 0,
    };
    regions
        .iter()
        .zip(&all)
        .filter(|&(&(s, e), &l)| {
            l == base + level + 1 && within.is_none_or(|(ws, we)| s > ws && e <= we)
        })
        .map(|(r, _)| *r)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LanguageId;

    /// SPEC-0083 v8 AK-21: which comments are region markers.
    #[test]
    fn comments_that_mark_regions() {
        let start = |name: &str| Some(RegionMarker::Start(name.to_owned()));
        assert_eq!(region_marker("// region Fields"), start("Fields"));
        assert_eq!(
            region_marker("//region Fields and more "),
            start("Fields and more")
        );
        assert_eq!(region_marker("// region"), start(""));
        assert_eq!(region_marker("// endregion"), Some(RegionMarker::End));
        assert_eq!(region_marker("//endregion Fields"), Some(RegionMarker::End));
        assert_eq!(region_marker("// #region Setup"), start("Setup"));
        assert_eq!(region_marker("#region Setup"), start("Setup"));
        assert_eq!(region_marker("# region Setup"), start("Setup"));
        assert_eq!(region_marker("# endregion"), Some(RegionMarker::End));
        assert_eq!(region_marker("#endregion"), Some(RegionMarker::End));
        assert_eq!(region_marker("-- region Queries"), start("Queries"));
        assert_eq!(region_marker("<!-- region Head -->"), start("Head"));
        assert_eq!(region_marker("<!-- endregion -->"), Some(RegionMarker::End));
        assert_eq!(region_marker("/* region Block */"), start("Block"));
        assert_eq!(
            region_marker("// <editor-fold desc=\"Getters and setters\">"),
            start("Getters and setters")
        );
        assert_eq!(
            region_marker("// <editor-fold defaultstate=\"collapsed\" desc=\"Init\">"),
            start("Init")
        );
        assert_eq!(region_marker("//<editor-fold>"), start(""));
        assert_eq!(region_marker("// </editor-fold>"), Some(RegionMarker::End));
        assert_eq!(
            region_marker("<!-- <editor-fold desc=\"Nav\"> -->"),
            start("Nav")
        );
        // Words that only begin like a marker, and text that is no comment.
        assert_eq!(region_marker("// regional settings"), None);
        assert_eq!(region_marker("// endregions"), None);
        assert_eq!(region_marker("// a region"), None);
        assert_eq!(region_marker("// <editor-folding>"), None);
        assert_eq!(region_marker("region Fields"), None);
        assert_eq!(region_marker("/* region\n A */"), None);
    }

    /// SPEC-0083 v8 AK-21: markers pair innermost first; one without a partner gives nothing.
    #[test]
    fn markers_pair_innermost_first() {
        let s = |n: &str| RegionMarker::Start(n.to_owned());
        let e = RegionMarker::End;
        assert_eq!(
            pair_markers(&[(1, s("A")), (3, s("B")), (5, e.clone()), (9, e.clone())]),
            [(1, 9, "A".to_owned()), (3, 5, "B".to_owned())]
        );
        // An end without a start, a start without an end.
        assert_eq!(
            pair_markers(&[(0, e.clone()), (2, s("A")), (4, e.clone()), (6, s("B"))]),
            [(2, 4, "A".to_owned())]
        );
        assert!(pair_markers(&[(2, s("A"))]).is_empty());
        assert!(pair_markers(&[(2, e)]).is_empty());
    }

    /// SPEC-0083 v8 AK-23: the first sentence and the placeholders of comments and blocks.
    #[test]
    fn placeholders_of_comments_and_blocks() {
        assert_eq!(
            first_sentence(["", " * A class to fold.", " * Second line of the doc.", " "]),
            "A class to fold."
        );
        assert_eq!(
            first_sentence([
                " * Folds v1.2 files",
                " * over two lines. More.",
                " * @since 1"
            ]),
            "Folds v1.2 files over two lines."
        );
        assert_eq!(
            first_sentence([" * No full stop", " *", " * Next"]),
            "No full stop"
        );
        assert_eq!(first_sentence([" * Text", " * @param a x"]), "Text");
        assert_eq!(first_sentence(["", " * @param a x"]), "");
        assert_eq!(
            comment_look("/**\n * A class to fold.\n * Second line.\n */", 0),
            (true, Some(3), " A class to fold. ...*/".to_owned())
        );
        assert_eq!(
            comment_look("/**\n     */", 4),
            (true, Some(7), "...*/".to_owned())
        );
        assert_eq!(
            comment_look("/*\n * Header\n */", 0),
            (false, Some(2), "...*/".to_owned())
        );
        assert_eq!(
            comment_look("<!-- a\n b -->", 2),
            (false, Some(6), "...-->".to_owned())
        );
        assert_eq!(
            comment_look("=begin\nx\n=end", 0),
            (false, None, "...".to_owned())
        );
        assert_eq!(
            comment_run_look(&["// one", "// two"], 4),
            (false, Some(6), "...".to_owned())
        );
        assert_eq!(
            comment_run_look(&["# one", "# two"], 0),
            (false, Some(1), "...".to_owned())
        );
        assert_eq!(
            comment_run_look(&["/// Adds two numbers. Never", "/// overflows."], 0),
            (true, Some(3), " Adds two numbers. ...".to_owned())
        );
        assert_eq!(
            comment_run_look(&["/// doc", "// plain"], 0),
            (false, Some(2), "...".to_owned())
        );
        assert_eq!(block_placeholder("    void run() {", "    }"), "...}");
        assert_eq!(block_placeholder("let v = [", "];"), "...]");
        assert_eq!(block_placeholder("call(", "    b);"), "...");
        assert_eq!(block_placeholder("if (a) {", "    b();"), "...");
        assert_eq!(block_placeholder("class A:", "        return 1"), "...");
    }

    const FOLD_JAVA: &str = "/*\n * Header comment\n * over three lines.\n */\nimport java.util.ArrayList;\nimport java.util.List;\nimport java.util.Map;\n\n/**\n * A class to fold.\n * Second line of the doc.\n */\npublic class Fold {\n    // region Fields\n    private final List<String> names = new ArrayList<>();\n    private int count;\n    // endregion\n\n    // one\n    // two\n    // three\n    public int getCount() {\n        return count;\n    }\n\n    public void add(String name) {\n        if (name != null) {\n            for (int i = 0; i < 2; i++) {\n                names.add(name + i);\n            }\n        } else {\n            count--;\n        }\n        count++;\n    }\n\n    Runnable task = new Runnable() {\n        @Override\n        public void run() {\n            names.forEach(n -> {\n                System.out.println(n);\n            });\n        }\n    };\n\n    static class Inner {\n        Map<String, Integer> sizes = Map.of(\n                \"a\", 1,\n                \"b\", 2);\n    }\n}\n";

    fn outline(language: LanguageId, text: &str) -> Outline {
        Outline::of(&SyntaxDocument::new(language, false, text).unwrap())
    }

    /// SPEC-0083 v8 AK-20, AK-22 and AK-23 on the Java file of the comparison: the outermost
    /// class stays open, the levels, the documentation comment and every placeholder.
    #[test]
    fn outline_of_a_java_file() {
        let o = outline(LanguageId::Java, FOLD_JAVA);
        assert_eq!(o.outermost, Some((12, 50)));
        assert_eq!(
            outermost_of(&o.collapse_all()),
            [
                (0, 3),
                (4, 6),
                (8, 11),
                (13, 16),
                (18, 20),
                (21, 23),
                (25, 34),
                (36, 43),
                (45, 49)
            ]
        );
        // `if` ends before the line of `} else {`, so both blocks fold.
        assert_eq!(o.all_to_level(1), [(26, 29), (30, 32), (38, 42), (46, 48)]);
        assert_eq!(o.all_to_level(2), [(27, 29), (39, 41)]);
        assert!(o.all_to_level(3).is_empty());
        assert_eq!(o.to_level(1, 26), Some(((26, 29), vec![(27, 29)])));
        assert_eq!(
            o.to_level(1, 33),
            Some(((25, 34), vec![(26, 29), (30, 32)]))
        );
        // In the class body itself the class is the region the levels count from.
        assert_eq!(
            o.to_level(2, 24)
                .map(|(within, folds)| (within, folds.len())),
            Some(((12, 50), 4))
        );
        assert_eq!(o.docs, [(8, 11)]);
        let look = |start: usize| {
            let l = o.looks.iter().find(|l| l.start == start).unwrap();
            (l.keep, l.text.as_str())
        };
        assert_eq!(look(0), (Some(2), "...*/"));
        assert_eq!(look(4), (Some(7), "..."));
        assert_eq!(look(8), (Some(3), " A class to fold. ...*/"));
        assert_eq!(look(13), (Some(4), "Fields"));
        assert_eq!(look(18), (Some(6), "..."));
        // A one-line method keeps its statement.
        assert_eq!(look(21), (None, " return count; }"));
        assert_eq!(look(25), (None, "...}"));
        assert_eq!(look(26), (None, "..."));
        assert_eq!(look(30), (None, "...}"));
        assert_eq!(look(36), (None, "...}"));
        assert_eq!(look(45), (None, "...}"));
    }

    /// SPEC-0083 v8 AK-20: which languages have an outermost container.
    #[test]
    fn outermost_containers() {
        let outermost = |language, text: &str| outline(language, text).outermost;
        // One type at the top level of a Java or Kotlin file.
        assert_eq!(
            outermost(LanguageId::Java, "interface A {\n    void f();\n}\n"),
            Some((0, 2))
        );
        assert_eq!(
            outermost(LanguageId::Java, "enum E {\n    A,\n    B\n}\n"),
            Some((0, 3))
        );
        assert_eq!(
            outermost(
                LanguageId::Java,
                "class A {\n    int a;\n}\nclass B {\n    int b;\n}\n"
            ),
            None
        );
        assert_eq!(
            outermost(
                LanguageId::Kotlin,
                "package a\n\nclass A {\n    val x = 1\n}\n"
            ),
            Some((2, 4))
        );
        assert_eq!(
            outermost(
                LanguageId::Kotlin,
                "class A {\n    val x = 1\n}\n\nfun main() {\n    println()\n}\n"
            ),
            None
        );
        // The root value of a JSON file, the root element of an HTML or XML file.
        assert_eq!(
            outermost(LanguageId::Json, "{\n  \"a\": [\n    1\n  ]\n}"),
            Some((0, 4))
        );
        assert_eq!(
            outermost(
                LanguageId::Html,
                "<!DOCTYPE html>\n<html>\n<body>\n<p>x</p>\n</body>\n</html>\n"
            ),
            Some((1, 5))
        );
        assert_eq!(
            outermost(
                LanguageId::Xml,
                "<?xml version=\"1.0\"?>\n<a>\n  <b>\n    x\n  </b>\n</a>\n"
            ),
            Some((1, 5))
        );
        // Languages whose files hold several things side by side have none.
        assert_eq!(outermost(LanguageId::Rust, "fn f() {\n    g();\n}\n"), None);
        assert_eq!(
            outermost(
                LanguageId::Python,
                "class A:\n    def f(self):\n        return 1\n"
            ),
            None
        );
        let json = outline(LanguageId::Json, "{\n  \"a\": [\n    1\n  ]\n}");
        assert_eq!(json.collapse_all(), [(1, 3)]);
    }

    /// SPEC-0083 v8 AK-21: custom regions in the languages' comments; markers without a
    /// partner and regions that cross a block fold nothing.
    #[test]
    fn custom_regions_from_comments() {
        let regions = |language, text: &str| -> Vec<(usize, usize, Option<usize>, String)> {
            SyntaxDocument::new(language, false, text)
                .unwrap()
                .folding()
                .into_iter()
                .filter(|r| r.kind == "region")
                .map(|r| (r.start_line, r.end_line, r.keep, r.collapsed_text))
                .collect()
        };
        assert_eq!(
            regions(
                LanguageId::Java,
                "class A {\n    //region Fields\n    int a;\n    // endregion\n    // <editor-fold desc=\"Rest\">\n    int b;\n    // </editor-fold>\n}\n"
            ),
            [
                (1, 3, Some(4), "Fields".to_owned()),
                (4, 6, Some(4), "Rest".to_owned())
            ]
        );
        assert_eq!(
            regions(
                LanguageId::Python,
                "#region Setup\nimport os\n# endregion\n# region\nx = 1\n#endregion\n"
            ),
            [
                (0, 2, Some(0), "Setup".to_owned()),
                (3, 5, Some(0), "...".to_owned())
            ]
        );
        assert_eq!(
            regions(
                LanguageId::Html,
                "<div>\n<!-- region Head -->\n<p>a</p>\n<!-- endregion -->\n</div>\n"
            ),
            [(1, 3, Some(0), "Head".to_owned())]
        );
        assert_eq!(
            regions(
                LanguageId::Rust,
                "// region Types\nstruct A;\nstruct B;\n// endregion\n"
            ),
            [(0, 3, Some(0), "Types".to_owned())]
        );
        // Unbalanced: nothing folds, nothing fails.
        assert!(
            regions(
                LanguageId::Java,
                "class A {\n    // region Fields\n    int a;\n}\n"
            )
            .is_empty()
        );
        assert!(
            regions(
                LanguageId::Java,
                "class A {\n    int a;\n    // endregion\n}\n"
            )
            .is_empty()
        );
        // A marker behind code on its line is none.
        assert!(
            regions(
                LanguageId::Java,
                "class A {\n    int a; // region X\n    int b;\n    // endregion\n}\n"
            )
            .is_empty()
        );
        // A region that starts in a method and ends behind it crosses the method's block.
        assert!(
            regions(
                LanguageId::Java,
                "class A {\n    void f() {\n        // region X\n        g();\n    }\n    // endregion\n}\n"
            )
            .is_empty()
        );
        // The marker lines are no run of line comments of their own.
        let all = SyntaxDocument::new(
            LanguageId::Java,
            false,
            "class A {\n    // region Fields\n    // the first\n    // the second\n    int a;\n    // endregion\n}\n",
        )
        .unwrap()
        .folding();
        let spans: Vec<(usize, usize, &str)> = all
            .iter()
            .map(|r| (r.start_line, r.end_line, r.kind.as_str()))
            .collect();
        assert_eq!(
            spans,
            [(0, 6, "block"), (1, 5, "region"), (2, 3, "comment")]
        );
    }

    /// SPEC-0083 v6 AK-14.
    #[test]
    fn levels_of_regions() {
        // Imports 0–1; a class 3–20 with methods 5–9 (blocks 6–8, in it 7–8) and 11–13; a comment
        // 22–24.
        let regions = [(0, 1), (3, 20), (5, 9), (6, 8), (7, 8), (11, 13), (22, 24)];
        assert_eq!(levels(&regions), [1, 1, 2, 3, 4, 2, 1]);
        // `} else {`: the second block starts where the first ends and is not inside it.
        assert_eq!(levels(&[(1, 9), (2, 4), (4, 6)]), [1, 2, 2]);
        assert_eq!(level_folds(&regions, 1, None), [(5, 9), (11, 13)]);
        assert_eq!(level_folds(&regions, 2, None), [(6, 8)]);
        assert_eq!(level_folds(&regions, 3, None), [(7, 8)]);
        assert!(level_folds(&regions, 4, None).is_empty());
        // Inside the method 5–9, which is level 1 there.
        assert_eq!(level_folds(&regions, 1, Some((5, 9))), [(6, 8)]);
        assert_eq!(level_folds(&regions, 2, Some((5, 9))), [(7, 8)]);
        assert!(level_folds(&regions, 1, Some((11, 13))).is_empty());
        assert!(level_folds(&regions, 1, Some((40, 41))).is_empty());
        assert_eq!(innermost_around(&regions, 7), Some((7, 8)));
        assert_eq!(innermost_around(&regions, 5), Some((5, 9)));
        assert_eq!(innermost_around(&regions, 10), Some((3, 20)));
        assert_eq!(innermost_around(&regions, 21), None);
        // With an outermost container the levels start inside it.
        let outline = Outline {
            lines: regions.to_vec(),
            outermost: Some((3, 20)),
            ..Outline::default()
        };
        assert_eq!(outline.all_to_level(1), [(6, 8)]);
        assert_eq!(outline.all_to_level(2), [(7, 8)]);
        assert_eq!(
            outline.collapse_all(),
            [(0, 1), (5, 9), (6, 8), (7, 8), (11, 13), (22, 24)]
        );
        assert_eq!(
            outermost_of(&outline.collapse_all()),
            [(0, 1), (5, 9), (11, 13), (22, 24)]
        );
    }

    /// SPEC-0083 AK-30 (v9): the chooser of `GotoCustomRegion` lists the regions that fold, in
    /// the order of their lines, with their names; the one around the caret is selected first.
    #[test]
    fn custom_regions_are_listed_for_the_chooser() {
        let listed = |language, text: &str| {
            custom_regions(
                &SyntaxDocument::new(language, false, text)
                    .unwrap()
                    .folding(),
            )
        };
        let java = listed(
            LanguageId::Java,
            "class A {\n    // region Outer\n    int a;\n    // region Inner\n    int b;\n    // endregion\n    int c;\n    // endregion\n    // region\n    int d;\n    // endregion\n    // region Never closed\n    int e;\n}\n",
        );
        let rows: Vec<(usize, usize, &str)> = java
            .iter()
            .map(|r| (r.start_line, r.end_line, r.name.as_str()))
            .collect();
        assert_eq!(rows, [(1, 7, "Outer"), (3, 5, "Inner"), (8, 10, "")]);
        let labels: Vec<String> = java.iter().map(CustomRegion::label).collect();
        assert_eq!(
            labels,
            [
                "Outer  (line 2)",
                "Inner  (line 4)",
                "Unnamed region  (line 9)"
            ]
        );
        // The innermost region around the caret, else the next one, else the last.
        assert_eq!(region_at(&java, 4), Some(1));
        assert_eq!(region_at(&java, 6), Some(0));
        assert_eq!(region_at(&java, 0), Some(0));
        assert_eq!(region_at(&java, 9), Some(2));
        assert_eq!(region_at(&java, 12), Some(2));
        assert_eq!(region_at(&[], 3), None);
        // A file without markers has none; the blocks of the file are no custom regions.
        assert!(listed(LanguageId::Rust, "fn a() {\n    let x = 1;\n}\n").is_empty());
        assert_eq!(
            listed(
                LanguageId::Python,
                "#region Setup\nimport os\n# endregion\n"
            ),
            [CustomRegion {
                start_line: 0,
                end_line: 2,
                name: "Setup".into()
            }]
        );
    }
}
