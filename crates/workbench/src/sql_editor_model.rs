//! The SQL editor bound to a data source (`SPEC-0192`), free of the UI toolkit: what ⌘⏎ runs, the
//! notes after statements, and when cached metadata is checked again (`SPEC-0014` v2).

use std::ops::Range;
use std::time::{Duration, Instant};

use idecraft_database::{Dialect, Outcome, Problem, Statement, split_statements};

use crate::sql_console_model::{statement_at, statements_to_run};

/// Least time between two checks of a connection's catalog fingerprint (`SPEC-0014` v2
/// behaviour 13).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// What ⌘⏎ does (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPlan {
    /// Run these statements at once (a selection, or the only statement).
    Run(Vec<Statement>),
    /// Ask: the statement at the caret, or all of the script.
    Ask {
        /// The statement at the caret.
        statement: Statement,
        /// Every statement of the script.
        all: Vec<Statement>,
    },
    /// Nothing to run at the caret.
    Nothing,
}

/// What ⌘⏎ does for `script` with `selection` and `caret` (byte offsets).
pub fn run_plan(script: &str, selection: Range<usize>, caret: usize, dialect: Dialect) -> RunPlan {
    if !selection.is_empty() {
        let statements = statements_to_run(script, selection, caret, dialect);
        return if statements.is_empty() {
            RunPlan::Nothing
        } else {
            RunPlan::Run(statements)
        };
    }
    let Some(statement) = statement_at(script, caret, dialect) else {
        return RunPlan::Nothing;
    };
    let all = split_statements(script, dialect);
    if all.len() <= 1 {
        RunPlan::Run(vec![statement])
    } else {
        RunPlan::Ask { statement, all }
    }
}

/// The two entries of the choice popup (behaviour 4).
pub fn choice_labels(statement: &Statement, count: usize) -> [String; 2] {
    let first = statement.text.lines().next().unwrap_or_default().trim();
    let shown: String = if first.chars().count() > 48 {
        first.chars().take(47).chain(['…']).collect()
    } else {
        first.to_owned()
    };
    [
        format!("Statement at caret: {shown}"),
        format!("Whole script ({count} statements)"),
    ]
}

/// How a note after a line looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// Grey: a timing.
    Info,
    /// Orange: an inspection warning (IMP-0062).
    Warning,
    /// Red: a failed statement.
    Error,
}

/// A note after the text of `line`, belonging to the statement on lines `first..=last` (0-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineNote {
    /// First line of the statement.
    pub first: usize,
    /// Last line of the statement.
    pub last: usize,
    /// The line the note stands on.
    pub line: usize,
    /// Text.
    pub text: String,
    /// Look.
    pub kind: NoteKind,
}

/// Moves notes after an edit at `line` that replaced `removed` line breaks by `added` ones: a note
/// whose statement the edit touches goes away, notes below move (behaviour 5).
pub fn shift_notes(notes: &[LineNote], line: usize, removed: usize, added: usize) -> Vec<LineNote> {
    let touched = line..=line + removed;
    notes
        .iter()
        .filter(|n| n.last < *touched.start() || n.first > *touched.end())
        .map(|n| {
            if n.first > line {
                let shift = |l: usize| (l + added).saturating_sub(removed);
                LineNote {
                    first: shift(n.first),
                    last: shift(n.last),
                    line: shift(n.line),
                    ..n.clone()
                }
            } else {
                n.clone()
            }
        })
        .collect()
}

/// "12 ms" under a second, else "1.4 s".
pub fn duration_text(time: Duration) -> String {
    if time < Duration::from_secs(1) {
        format!("{} ms", time.as_millis())
    } else {
        format!("{:.1} s", time.as_secs_f64())
    }
}

/// The 0-based line of byte offset `at` in `script`.
pub fn line_of(script: &str, at: usize) -> usize {
    script[..at.min(script.len())].matches('\n').count()
}

/// The note after a statement that ran (behaviour 5), placed on its last line.
pub fn timing_note(
    script: &str,
    range: Range<usize>,
    outcome: &Outcome,
    time: Duration,
) -> LineNote {
    let first = line_of(script, range.start);
    let last = line_of(script, range.end);
    let plural = |n: usize| if n == 1 { "row" } else { "rows" };
    let (text, kind) = match outcome {
        Outcome::ResultSet { rows, .. } => (
            format!(
                "{} · {} {}",
                duration_text(time),
                rows.len(),
                plural(rows.len())
            ),
            NoteKind::Info,
        ),
        Outcome::UpdateCount(n) => {
            let n = *n as usize;
            (
                format!("{} · {n} {} affected", duration_text(time), plural(n)),
                NoteKind::Info,
            )
        }
        Outcome::Error(_) => (format!("{} · failed", duration_text(time)), NoteKind::Error),
    };
    LineNote {
        first,
        last,
        line: last,
        text,
        kind,
    }
}

/// Inline notes for the unsafe-change warnings among `problems` (behaviour 7, IMP-0062): the text
/// up to its first `;`, after the first line of the statement.
pub fn warning_notes(script: &str, problems: &[Problem], dialect: Dialect) -> Vec<LineNote> {
    let statements = split_statements(script, dialect);
    problems
        .iter()
        .filter(|p| p.table.is_some())
        .filter_map(|p| {
            let s = statements
                .iter()
                .find(|s| s.range.start <= p.start && p.start <= s.range.end)?;
            let first = line_of(script, s.range.start);
            Some(LineNote {
                first,
                last: line_of(script, s.range.end),
                line: first,
                text: p.message.split(';').next().unwrap_or_default().to_owned(),
                kind: NoteKind::Warning,
            })
        })
        .collect()
}

/// The data source in the toolbar (behaviour 3): "shop · public", or the name alone while the
/// schema is not known.
pub fn toolbar_label(connection: &str, schema: Option<&str>) -> String {
    match schema {
        Some(s) if !s.is_empty() => format!("{connection} · {s}"),
        _ => connection.to_owned(),
    }
}

/// Whether the cached schema of a connection must be checked against the database before
/// completing (`SPEC-0014` v2 behaviour 13): never checked, or longer than [`CHECK_INTERVAL`] ago.
pub fn needs_check(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|l| now.saturating_duration_since(l) >= CHECK_INTERVAL)
}

/// What the editor knows about a connection's catalog beside its tables (`SPEC-0014` v2,
/// `SPEC-0192` behaviour 3).
#[derive(Debug, Clone, Default)]
pub struct CatalogCache {
    /// Fingerprint taken with the cached tables.
    pub fingerprint: Option<String>,
    /// When the fingerprint was last compared with the database.
    pub checked: Option<Instant>,
    /// The default schema.
    pub schema: Option<String>,
    /// The other schemas, once listed.
    pub others: Option<Vec<idecraft_database::SchemaEntry>>,
}

/// Text of the unsafe-change popup's choices (behaviour 8).
pub fn unsafe_choices(connection: &str) -> [String; 3] {
    [
        "Run".into(),
        format!("Run, don't ask again for {connection} in this session"),
        "Cancel".into(),
    ]
}

/// A replacement of a byte range.
pub type ByteEdit = (Range<usize>, String);

/// The `⌥↩` entry of Inline on the `WITH` query at byte `offset` (`SPEC-0152` v2 behaviour 6): its
/// title and byte edits; `None` where Inline is refused or the caret is on no `WITH` query.
pub fn inline_fix(text: &str, dialect: Dialect, offset: usize) -> Option<(String, Vec<ByteEdit>)> {
    let (name, edits) = idecraft_database::inline_cte(text, dialect, offset).ok()??;
    Some((
        format!("Inline common table expression `{name}`"),
        edits
            .into_iter()
            .map(|e| (e.start..e.end, e.replacement))
            .collect(),
    ))
}

/// How often the schema of each connection, and of all, was refreshed (`SPEC-0192` v3 behaviour
/// 11): a schema read before a refresh of its connection is stale.
#[derive(Debug, Default, Clone)]
pub struct SchemaGenerations {
    all: u64,
    each: std::collections::HashMap<String, u64>,
}

impl SchemaGenerations {
    /// What a read of connection `id` started now is compared with later.
    pub fn of(&self, id: &str) -> (u64, u64) {
        (self.all, self.each.get(id).copied().unwrap_or(0))
    }

    /// A refresh of connection `id`, or of all with `None`.
    pub fn refreshed(&mut self, id: Option<&str>) {
        match id {
            Some(id) => *self.each.entry(id.to_owned()).or_default() += 1,
            None => self.all += 1,
        }
    }

    /// Whether a read of `id` that started at `then` is still of the current schema.
    pub fn is_current(&self, id: &str, then: (u64, u64)) -> bool {
        self.of(id) == then
    }
}

/// The open `.sql` files to inspect again after a refresh of connection `refreshed` (`None`: all),
/// from each open file and the connection it is inspected with (`SPEC-0192` v3 behaviour 11).
pub fn files_to_inspect(
    open: &[(std::path::PathBuf, Option<String>)],
    refreshed: Option<&str>,
) -> Vec<std::path::PathBuf> {
    open.iter()
        .filter(|(_, id)| match refreshed {
            None => true,
            Some(r) => id.as_deref() == Some(r),
        })
        .map(|(p, _)| p.clone())
        .collect()
}

/// The char ranges of the block comments of `text` in `dialect`, as the splitter finds them
/// (`SPEC-0005` v5 behaviour 19); empty for a dialect whose comments do not nest, where the syntax
/// tree already ends a comment at its first `*/`.
pub fn nested_comment_ranges(text: &str, dialect: Dialect) -> Vec<Range<usize>> {
    if !(dialect.postgres_like() || dialect == Dialect::SqlServer) || !text.contains("/*") {
        return Vec::new();
    }
    let char_at = |byte: usize| text[..byte].chars().count();
    idecraft_database::block_comments(text, dialect)
        .into_iter()
        .map(|r| char_at(r.start)..char_at(r.end))
        .collect()
}

/// `spans` of the char range `range` with `comments` coloured as comment over whatever the syntax
/// tree made of them (`SPEC-0005` v5 behaviour 19); sorted by start.
pub fn with_comments(
    spans: Vec<idecraft_languages::syntax::Span>,
    comments: &[Range<usize>],
    range: Range<usize>,
) -> Vec<idecraft_languages::syntax::Span> {
    use idecraft_languages::syntax::Span;
    let comments: Vec<Range<usize>> = comments
        .iter()
        .filter(|c| c.start < range.end && range.start < c.end)
        .map(|c| c.start.max(range.start)..c.end.min(range.end))
        .collect();
    if comments.is_empty() {
        return spans;
    }
    let mut out = Vec::new();
    for span in spans {
        // The parts of the span outside every comment.
        let mut at = span.start;
        for c in comments
            .iter()
            .filter(|c| c.start < span.end && span.start < c.end)
        {
            if at < c.start {
                out.push(Span {
                    start: at,
                    end: c.start,
                    kind: span.kind.clone(),
                });
            }
            at = at.max(c.end);
        }
        if at < span.end {
            out.push(Span {
                start: at,
                end: span.end,
                kind: span.kind,
            });
        }
    }
    out.extend(comments.into_iter().map(|c| Span {
        start: c.start,
        end: c.end,
        kind: "comment".into(),
    }));
    out.sort_by_key(|s| (s.start, s.end));
    out
}

/// The status line for a refactoring SQL files do not offer, as one whole sentence and a list
/// (`SPEC-0152` v4 behaviour 6a), with `keys` giving the key of an action in the keymap in use
/// (`SPEC-0028` v9 behaviour 14).
pub fn not_offered(refactoring: &str, keys: impl Fn(&str) -> String) -> String {
    let p = |action: &str| crate::keymaps::in_parens(&keys(action));
    let intentions = keys("ShowIntentionActions");
    let intentions = if intentions.is_empty() {
        "the context actions".to_string()
    } else {
        format!("the actions of {intentions}")
    };
    format!(
        "{refactoring} is not offered in SQL files. Offered: Rename{}, Inline{} and {intentions}.",
        p("RenameElement"),
        p("Inline")
    )
}

/// The dialect the text of a `.sql` file reads like, from marks only one family uses: MySQL's
/// `DELIMITER` and backticks, PostgreSQL's dollar quotes and `::` casts, SQL Server's `GO` lines
/// and `NVARCHAR`, Oracle's `/` lines and `VARCHAR2`, SQLite's `AUTOINCREMENT` and `PRAGMA`.
/// `None` when no family has a mark or two have as many (`SPEC-0192` v5 behaviour 3a).
pub fn content_dialect(text: &str) -> Option<idecraft_database::Dialect> {
    use idecraft_database::Dialect;
    let code: Vec<String> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .map(|l| l.to_ascii_lowercase())
        .collect();
    let all = code.join("\n");
    let lines = |word: &str| code.iter().filter(|l| l.trim() == word).count();
    let has = |needle: &str| usize::from(all.contains(needle));
    let starts = |word: &str| {
        code.iter()
            .filter(|l| l.trim_start().starts_with(word))
            .count()
    };
    let dollar = all.contains("$$")
        || all.match_indices('$').any(|(i, _)| {
            all[i + 1..].split('$').next().is_some_and(|tag| {
                !tag.is_empty()
                    && tag.len() < 20
                    && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && all[i + 1 + tag.len()..].starts_with('$')
            })
        });
    let scores = [
        (
            Dialect::MySql,
            starts("delimiter ") + has("`") + has("auto_increment") + has("engine="),
        ),
        (
            Dialect::PostgreSql,
            usize::from(dollar) + has("::") + has("raise notice") + has(" ilike ") + has("serial"),
        ),
        (
            Dialect::SqlServer,
            lines("go") + has("nvarchar") + has("identity(") + has("[dbo]"),
        ),
        (
            Dialect::Oracle,
            lines("/") + has("varchar2") + has("sysdate") + has(" nvl("),
        ),
        (Dialect::Sqlite, has("autoincrement") + starts("pragma ")),
    ];
    let best = scores.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let mut leaders = scores.iter().filter(|(_, n)| *n == best && best > 0);
    match (leaders.next(), leaders.next()) {
        (Some((dialect, _)), None) => Some(*dialect),
        _ => None,
    }
}

/// Whether a file written for `content` runs on a connection of `dialect`: the same dialect, or
/// MySQL and MariaDB, which share their lexis.
pub fn dialect_fits(
    content: idecraft_database::Dialect,
    dialect: idecraft_database::Dialect,
) -> bool {
    use idecraft_database::Dialect::{MariaDb, MySql};
    content == dialect || matches!((content, dialect), (MySql, MariaDb) | (MariaDb, MySql))
}

/// The hint for an unbound file whose text reads like another dialect than the connection offered
/// for it (`SPEC-0192` v5 behaviour 3a, OBS-0068 L12); `None` when it fits or nothing is clear.
pub fn dialect_hint(text: &str, offered: idecraft_database::Dialect) -> Option<String> {
    let content = content_dialect(text)?;
    (!dialect_fits(content, offered)).then(|| {
        format!(
            "the file reads like {}, the data source is {}",
            content.label(),
            offered.label()
        )
    })
}

/// What ⌘⏎ or ▶ does in a `.sql` file that has no connection by its binding, its console folder or
/// the Database window's selection (`SPEC-0192` v6 behaviour 3b, OBS-0070 V1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoDataSource {
    /// The workspace has no connection: the status bar says how to add one.
    AddOne,
    /// It has one: the file runs there (behaviour 1).
    RunOnTheOnly,
    /// It has several: the data-source popup opens, and the run goes on after the choice.
    Choose,
}

/// [`NoDataSource`] for a workspace with `connections` connections.
pub fn no_data_source(connections: usize) -> NoDataSource {
    match connections {
        0 => NoDataSource::AddOne,
        1 => NoDataSource::RunOnTheOnly,
        _ => NoDataSource::Choose,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0192 v6 AK-11 (OBS-0070 V1): a run without a data source asks for one when there is a
    /// choice.
    #[test]
    fn a_run_without_a_data_source_asks_for_one() {
        assert_eq!(no_data_source(0), NoDataSource::AddOne);
        assert_eq!(no_data_source(1), NoDataSource::RunOnTheOnly);
        assert_eq!(no_data_source(2), NoDataSource::Choose);
        assert_eq!(no_data_source(7), NoDataSource::Choose);
    }

    /// SPEC-0192 v5 AK-10 (OBS-0068 L12): the dialect a file reads like, and the hint.
    #[test]
    fn the_dialect_a_file_reads_like() {
        use idecraft_database::Dialect::*;
        let mysql = "-- for MySQL\ncreate table pd_note (id int primary key);\n\nDELIMITER //\ncreate procedure pd_two_sets()\nbegin\n  select 1;\nend //\nDELIMITER ;\n";
        assert_eq!(content_dialect(mysql), Some(MySql));
        assert_eq!(content_dialect("select `name` from t"), Some(MySql));
        assert_eq!(
            content_dialect("do $pd$ begin raise notice 'x'; end $pd$;"),
            Some(PostgreSql)
        );
        assert_eq!(content_dialect("select a::text from t"), Some(PostgreSql));
        assert_eq!(
            content_dialect("create table t (a nvarchar(4));\nGO\n"),
            Some(SqlServer)
        );
        assert_eq!(content_dialect("begin null; end;\n/\n"), Some(Oracle));
        assert_eq!(
            content_dialect("create table t (id integer primary key autoincrement)"),
            Some(Sqlite)
        );
        assert_eq!(
            content_dialect("select * from t where a = 1"),
            None,
            "plain SQL"
        );
        assert_eq!(
            content_dialect("select `a`::text"),
            None,
            "a tie says nothing"
        );
        assert_eq!(
            content_dialect("-- DELIMITER //\nselect 1"),
            None,
            "comments do not count"
        );

        assert_eq!(
            dialect_hint(mysql, PostgreSql).as_deref(),
            Some("the file reads like MySQL, the data source is PostgreSQL")
        );
        assert_eq!(dialect_hint(mysql, MariaDb), None, "MySQL runs on MariaDB");
        assert_eq!(dialect_hint("select 1", PostgreSql), None);
    }

    /// SPEC-0152 v3 AK-7 (OBS-0068 L4): no language server is named for a SQL file.
    #[test]
    fn sql_files_name_their_own_refactorings() {
        let line = not_offered("Extract Method", |_| String::new());
        assert!(
            line.starts_with("Extract Method is not offered in SQL files. Offered: "),
            "{line}"
        );
        assert!(!line.contains("language server"), "{line}");
        // SPEC-0028 v9 AK-16: the keys of the keymap in use, for macOS and for Linux.
        // SPEC-0152 v4 AK-8 (OBS-0068 N1): a whole sentence, not cut off.
        let mac = idecraft_ui::keymap::Keymap::bundled_macos();
        let line = not_offered("Extract Method", |a| mac.shortcut_label(a, true));
        assert_eq!(
            line,
            "Extract Method is not offered in SQL files. Offered: Rename (⇧F6), Inline (⌥⌘N) and the actions of ⌥↩."
        );
        let linux = idecraft_ui::keymap::Keymap::bundled_default();
        let line = not_offered("Extract Method", |a| linux.shortcut_label(a, false));
        assert_eq!(
            line,
            "Extract Method is not offered in SQL files. Offered: Rename (Shift+F6), Inline (Ctrl+Alt+N) and the actions of Alt+Enter."
        );
    }
    use idecraft_database::{DbError, inspect};

    /// SPEC-0152 v2 AK-5.
    #[test]
    fn inline_is_offered_at_a_with_query() {
        let text = "with c as (select 1 as x) select c.x from c";
        let at = text.rfind("c").unwrap();
        let (title, edits) = inline_fix(text, Dialect::PostgreSql, at).unwrap();
        assert_eq!(title, "Inline common table expression `c`");
        let mut out = text.to_owned();
        for (r, t) in edits.into_iter().rev() {
            out.replace_range(r, &t);
        }
        assert_eq!(out, "select c.x from (select 1 as x) c");
        // Refused or no WITH query: nothing offered.
        assert!(inline_fix("with c as (select 1) select 2", Dialect::PostgreSql, 5).is_none());
        assert!(inline_fix("select 1", Dialect::PostgreSql, 7).is_none());
    }

    /// SPEC-0192 v3 AK-7 (OBS-0051 L7).
    #[test]
    fn a_schema_read_before_a_refresh_is_not_kept() {
        let mut g = SchemaGenerations::default();
        let then = g.of("pg");
        let other = g.of("my");
        g.refreshed(Some("pg"));
        assert!(!g.is_current("pg", then), "pg was refreshed");
        assert!(
            g.is_current("my", other),
            "another connection is unaffected"
        );
        let now = g.of("pg");
        assert!(g.is_current("pg", now));
        g.refreshed(None);
        assert!(
            !g.is_current("pg", now) && !g.is_current("my", other),
            "a refresh of all"
        );
    }

    /// SPEC-0192 v3 AK-7.
    #[test]
    fn a_refresh_names_the_open_files_to_inspect() {
        use std::path::PathBuf;
        let open = [
            (PathBuf::from("/w/a.sql"), Some("pg".to_owned())),
            (PathBuf::from("/w/b.sql"), Some("my".to_owned())),
            (PathBuf::from("/w/c.sql"), None),
        ];
        assert_eq!(
            files_to_inspect(&open, Some("pg")),
            [PathBuf::from("/w/a.sql")]
        );
        assert_eq!(files_to_inspect(&open, None).len(), 3);
    }

    /// SPEC-0005 v5 AK-20 (OBS-0051 L7): after the inner `*/` the rest of a nested comment is
    /// comment in PostgreSQL, code in MySQL.
    #[test]
    fn nested_comments_are_coloured_per_dialect() {
        use idecraft_languages::syntax::Span;
        let text = "/* a /* b */ c */ select 1";
        let tree = vec![
            Span {
                start: 0,
                end: 12,
                kind: "comment".into(),
            },
            Span {
                start: 13,
                end: 14,
                kind: "variable".into(),
            },
            Span {
                start: 18,
                end: 24,
                kind: "keyword".into(),
            },
        ];
        let pg = nested_comment_ranges(text, Dialect::PostgreSql);
        assert_eq!(pg, vec![(0..17)]);
        let spans = with_comments(tree.clone(), &pg, 0..text.len());
        let kind_at = |spans: &[Span], at: usize| {
            spans
                .iter()
                .find(|s| s.start <= at && at < s.end)
                .map(|s| s.kind.clone())
        };
        assert_eq!(kind_at(&spans, 13).as_deref(), Some("comment"), "{spans:?}");
        assert_eq!(kind_at(&spans, 18).as_deref(), Some("keyword"));
        // MySQL: the syntax tree's colours stay.
        let my = nested_comment_ranges(text, Dialect::MySql);
        assert!(my.is_empty());
        let spans = with_comments(tree, &my, 0..text.len());
        assert_eq!(kind_at(&spans, 13).as_deref(), Some("variable"));
        // Multi-byte text: char ranges.
        assert_eq!(
            nested_comment_ranges("/*ä /* */ */x", Dialect::PostgreSql),
            vec![(0..12)]
        );
    }

    const SCRIPT: &str = "select 1;\nselect *\n  from t;\n\n-- note\n";

    fn pg(selection: Range<usize>, caret: usize) -> RunPlan {
        run_plan(SCRIPT, selection, caret, Dialect::PostgreSql)
    }

    #[test]
    fn what_command_enter_runs() {
        let RunPlan::Ask { statement, all } = pg(0..0, 12) else {
            panic!("{:?}", pg(0..0, 12));
        };
        assert_eq!(statement.text, "select *\n  from t");
        assert_eq!(all.len(), 2);
        assert_eq!(
            choice_labels(&statement, all.len()),
            [
                "Statement at caret: select *".to_owned(),
                "Whole script (2 statements)".to_owned()
            ]
        );
        let RunPlan::Run(s) = pg(0..8, 3) else {
            panic!()
        };
        assert_eq!(s[0].text, "select 1");
        let RunPlan::Run(s) = run_plan("select 2", 0..0, 3, Dialect::PostgreSql) else {
            panic!()
        };
        assert_eq!(s[0].text, "select 2");
        assert_eq!(pg(0..0, SCRIPT.len() - 2), RunPlan::Nothing);
        assert_eq!(
            run_plan("  ", 1..2, 1, Dialect::PostgreSql),
            RunPlan::Nothing
        );
        let long = Statement {
            range: 0..80,
            text: format!("select {}", "x, ".repeat(25)),
        };
        assert!(choice_labels(&long, 3)[0].ends_with('…'));
    }

    #[test]
    fn timing_notes_on_the_last_line() {
        let rows = Outcome::ResultSet {
            columns: vec!["a".into()],
            rows: vec![vec![None], vec![None], vec![None]],
        };
        let n = timing_note(SCRIPT, 10..27, &rows, Duration::from_millis(12));
        assert_eq!((n.first, n.last, n.line), (1, 2, 2));
        assert_eq!(n.text, "12 ms · 3 rows");
        assert_eq!(n.kind, NoteKind::Info);
        let n = timing_note(
            SCRIPT,
            0..8,
            &Outcome::UpdateCount(1),
            Duration::from_millis(1400),
        );
        assert_eq!(n.text, "1.4 s · 1 row affected");
        let error = Outcome::Error(DbError {
            message: "boom".into(),
            sqlstate: None,
            position: None,
        });
        let n = timing_note(SCRIPT, 0..8, &error, Duration::from_millis(3));
        assert_eq!(
            (n.text.as_str(), n.kind),
            ("3 ms · failed", NoteKind::Error)
        );
    }

    #[test]
    fn notes_move_with_edits_and_vanish_when_their_statement_changes() {
        let note = |first, last| LineNote {
            first,
            last,
            line: last,
            text: String::new(),
            kind: NoteKind::Info,
        };
        let notes = vec![note(0, 0), note(2, 4), note(6, 6)];
        // A line inserted on line 1 (between statements) moves the notes below.
        let moved = shift_notes(&notes, 1, 0, 1);
        assert_eq!(moved, vec![note(0, 0), note(3, 5), note(7, 7)]);
        // Typing on line 3 removes the note of the statement on lines 2–4 only.
        assert_eq!(shift_notes(&notes, 3, 0, 0), vec![note(0, 0), note(6, 6)]);
        // Joining lines 4 and 5 touches the second statement and moves the third up.
        assert_eq!(shift_notes(&notes, 4, 1, 0), vec![note(0, 0), note(5, 5)]);
    }

    #[test]
    fn unsafe_change_warning_stands_inline() {
        let script = "select 1;\ndelete\n  from stock;";
        let problems = inspect(script, Dialect::PostgreSql, &[]);
        let notes = warning_notes(script, &problems, Dialect::PostgreSql);
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].first, notes[0].last, notes[0].line), (1, 2, 1));
        assert_eq!(
            notes[0].text,
            "DELETE without WHERE removes every row of `stock`"
        );
        assert_eq!(notes[0].kind, NoteKind::Warning);
        assert_eq!(
            unsafe_choices("shop")[1],
            "Run, don't ask again for shop in this session"
        );
    }

    #[test]
    fn toolbar_and_metadata_checks() {
        assert_eq!(toolbar_label("shop", Some("public")), "shop · public");
        assert_eq!(toolbar_label("shop", None), "shop");
        let now = Instant::now();
        assert!(needs_check(None, now));
        assert!(!needs_check(Some(now), now + Duration::from_millis(1500)));
        assert!(needs_check(Some(now), now + CHECK_INTERVAL));
    }
}
