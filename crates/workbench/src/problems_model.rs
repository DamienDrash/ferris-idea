//! Rows of the Problems tool window (`SPEC-0092`): the problems of the current file, the errors of
//! the project grouped by file, and the Inspect Code results as a tree by inspection
//! (`SPEC-0098` v3 behaviour 8).

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::diagnostics_model::Severity;

/// One problem with its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// Absolute file path.
    pub path: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column (chars).
    pub column: usize,
    /// Char offset of the start, for the caret.
    pub offset: usize,
    /// Severity.
    pub severity: Severity,
    /// Message.
    pub message: String,
    /// The Ferris IDEA inspection that found it; `None` for language servers and tools.
    pub inspection: Option<&'static str>,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A file heading with its counts.
    File {
        /// The file.
        path: PathBuf,
        /// Errors in it.
        errors: usize,
        /// Warnings and weaker problems in it.
        warnings: usize,
    },
    /// A heading for an inspection when grouping by inspection (SPEC-0092 v3 behaviour 8).
    Group {
        /// The inspection's name.
        name: String,
        /// Its problems listed.
        count: usize,
    },
    /// A problem.
    Problem(Located),
}

/// Which problems are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// The active file.
    #[default]
    File,
    /// The errors of the project: open files and what the servers published for others (v3
    /// behaviour 7).
    Project,
    /// The results of the last Inspect Code run (`SPEC-0098` v3 behaviour 8).
    Inspection,
}

/// The File tab's toolbar choices (SPEC-0092 v3 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Problems of inspections under a heading per inspection.
    pub group_by_inspection: bool,
    /// By message instead of by severity.
    pub sort_by_name: bool,
    /// Warnings are listed.
    pub show_warnings: bool,
    /// Weak warnings are listed.
    pub show_weak: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            group_by_inspection: false,
            sort_by_name: false,
            show_warnings: true,
            show_weak: true,
        }
    }
}

fn rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Weak => 2,
    }
}

/// Rows for `problems` with the default choices.
pub fn rows(problems: &[Located], scope: Scope) -> Vec<Row> {
    rows_with(problems, scope, &ViewOptions::default())
}

/// Rows for `problems` (behaviours 2, 6–8): the file tab has a heading for the file, then its
/// problems by severity and line (or by message), optionally grouped by inspection and filtered;
/// the other tabs put a heading before each file, files with errors first, then by path.
pub fn rows_with(problems: &[Located], scope: Scope, options: &ViewOptions) -> Vec<Row> {
    let mut sorted: Vec<Located> = problems
        .iter()
        .filter(|p| match p.severity {
            Severity::Error => true,
            Severity::Warning => options.show_warnings || scope != Scope::File,
            Severity::Weak => options.show_weak || scope != Scope::File,
        })
        .cloned()
        .collect();
    if scope == Scope::File {
        if options.sort_by_name {
            sorted.sort_by(|a, b| (&a.message, a.line).cmp(&(&b.message, b.line)));
        } else {
            sorted.sort_by_key(|p| (rank(p.severity), p.line, p.column));
        }
        let mut out = Vec::new();
        if let Some(first) = problems.first() {
            let e = problems
                .iter()
                .filter(|p| p.severity == Severity::Error)
                .count();
            out.push(Row::File {
                path: first.path.clone(),
                errors: e,
                warnings: problems.len() - e,
            });
        }
        if !options.group_by_inspection {
            out.extend(sorted.into_iter().map(Row::Problem));
            return out;
        }
        // Server problems first, then one heading per inspection by name (behaviour 8).
        let (plain, grouped): (Vec<Located>, Vec<Located>) =
            sorted.into_iter().partition(|p| p.inspection.is_none());
        out.extend(plain.into_iter().map(Row::Problem));
        let mut names: Vec<(&'static str, &'static str)> = grouped
            .iter()
            .filter_map(|p| p.inspection)
            .map(|id| (ferris_idea_languages::inspections::name_of(id), id))
            .collect();
        names.sort();
        names.dedup();
        for (name, id) in names {
            let members: Vec<Located> = grouped
                .iter()
                .filter(|p| p.inspection == Some(id))
                .cloned()
                .collect();
            out.push(Row::Group {
                name: name.to_owned(),
                count: members.len(),
            });
            out.extend(members.into_iter().map(Row::Problem));
        }
        return out;
    }
    sorted.sort_by(|a, b| {
        (&a.path, a.line, rank(a.severity), a.column).cmp(&(
            &b.path,
            b.line,
            rank(b.severity),
            b.column,
        ))
    });
    let mut files: Vec<(PathBuf, Vec<Located>)> = Vec::new();
    for p in sorted {
        match files.last_mut() {
            Some((path, list)) if *path == p.path => list.push(p),
            _ => files.push((p.path.clone(), vec![p])),
        }
    }
    let errors = |list: &[Located]| {
        list.iter()
            .filter(|p| p.severity == Severity::Error)
            .count()
    };
    files.sort_by(|(a, la), (b, lb)| (errors(la) == 0, a).cmp(&(errors(lb) == 0, b)));
    let mut out = Vec::new();
    for (path, list) in files {
        let e = errors(&list);
        out.push(Row::File {
            path,
            errors: e,
            warnings: list.len() - e,
        });
        out.extend(list.into_iter().map(Row::Problem));
    }
    out
}

/// The label of the File tab: "File" and the number of problems (v3 behaviour 6).
pub fn file_tab_label(count: usize) -> String {
    if count == 0 {
        "File".into()
    } else {
        format!("File {count}")
    }
}

/// "6 problems", "1 problem" (the file heading, v3 behaviour 6).
pub fn problem_count(n: usize) -> String {
    format!("{n} {}", if n == 1 { "problem" } else { "problems" })
}

/// The next selectable row from `from` in direction `down`, skipping headings, wrapping around.
pub fn step(rows: &[Row], from: Option<usize>, down: bool) -> Option<usize> {
    let problems: Vec<usize> = (0..rows.len())
        .filter(|&i| matches!(rows[i], Row::Problem(_)))
        .collect();
    let first = *problems.first()?;
    let Some(from) = from else {
        return Some(if down {
            first
        } else {
            *problems.last().expect("not empty")
        });
    };
    let at = problems.iter().position(|&i| i == from);
    Some(match (at, down) {
        (Some(k), true) => problems[(k + 1) % problems.len()],
        (Some(k), false) => problems[(k + problems.len() - 1) % problems.len()],
        (None, _) => first,
    })
}

/// Header text, e.g. "2 errors, 1 warning, 1 weak warning" or "No problems" (v3: the editor
/// indicator's words, `SPEC-0070` v5 behaviour 9).
pub fn summary(problems: &[Located]) -> String {
    let n = |s: Severity| problems.iter().filter(|p| p.severity == s).count();
    crate::diagnostics_model::summary_words((
        n(Severity::Error),
        n(Severity::Warning),
        n(Severity::Weak),
    ))
}

/// "2 errors, 1 warning", "1 error", "3 warnings" or "No problems".
pub fn counts(errors: usize, warnings: usize) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match (errors, warnings) {
        (0, 0) => "No problems".into(),
        (e, 0) => plural(e, "error", "errors"),
        (0, w) => plural(w, "warning", "warnings"),
        (e, w) => format!(
            "{}, {}",
            plural(e, "error", "errors"),
            plural(w, "warning", "warnings")
        ),
    }
}

/// What Inspect Code runs over (`SPEC-0098` v3 behaviour 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectScope {
    /// Every project file.
    Project,
    /// One file (absolute path).
    File(PathBuf),
    /// The files below a folder (absolute path).
    Folder(PathBuf),
}

impl InspectScope {
    /// The choices of the scope dialog for the active file `active`, in order.
    pub fn choices(active: Option<&std::path::Path>) -> Vec<InspectScope> {
        let mut out = vec![InspectScope::Project];
        if let Some(path) = active {
            out.push(InspectScope::File(path.to_path_buf()));
            if let Some(dir) = path.parent() {
                out.push(InspectScope::Folder(dir.to_path_buf()));
            }
        }
        out
    }

    /// The choice as the dialog names it, relative to `root`.
    pub fn label(&self, root: &std::path::Path) -> String {
        let rel = |p: &std::path::Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
        match self {
            InspectScope::Project => "Whole project".into(),
            InspectScope::File(p) => format!("Current file ({})", rel(p)),
            InspectScope::Folder(p) => format!("Folder of the current file ({}/)", rel(p)),
        }
    }

    /// The name of the results tab (behaviour 8).
    pub fn tab(&self) -> String {
        match self {
            InspectScope::Project => "Inspection: Project".into(),
            InspectScope::File(p) | InspectScope::Folder(p) => format!(
                "Inspection: {}",
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
        }
    }

    /// The results tab's name for a run limited to the inspection `only`, else [`Self::tab`]
    /// (`SPEC-0098` v4 behaviour 10).
    pub fn tab_for(&self, only: Option<&str>) -> String {
        match only {
            Some(id) => match ferris_idea_languages::inspections::known_id(id) {
                Some(id) => format!(
                    "Inspection: {}",
                    ferris_idea_languages::inspections::name_of(id)
                ),
                None => format!("Inspection: {id}"),
            },
            None => self.tab(),
        }
    }

    /// The file or folder the scope names; `None` for the whole project (`SPEC-0098` v8
    /// behaviour 16: a named place is inspected whatever it is).
    pub fn place(&self) -> Option<&std::path::Path> {
        match self {
            InspectScope::Project => None,
            InspectScope::File(p) | InspectScope::Folder(p) => Some(p),
        }
    }

    /// Whether the project-relative file `rel` (under `root`) belongs to the scope.
    pub fn covers(&self, root: &std::path::Path, rel: &std::path::Path) -> bool {
        match self {
            InspectScope::Project => true,
            InspectScope::File(p) => root.join(rel) == *p,
            InspectScope::Folder(d) => root.join(rel).starts_with(d),
        }
    }
}

/// One row of the Inspect Code results tree (`SPEC-0098` v3 behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    /// The root: the profile and the number of findings.
    Root {
        /// Findings listed.
        count: usize,
    },
    /// An inspection with its findings.
    Inspection {
        /// Inspection id.
        id: &'static str,
        /// Display name.
        name: &'static str,
        /// Findings.
        count: usize,
        /// Whether its files are shown.
        open: bool,
    },
    /// A file under an inspection.
    File {
        /// Inspection id.
        id: &'static str,
        /// The file.
        path: PathBuf,
        /// Findings of the inspection in it.
        count: usize,
        /// Whether its findings are shown.
        open: bool,
    },
    /// A finding.
    Finding(Located),
}

impl TreeRow {
    /// The key a folded node is remembered by.
    pub fn fold_key(&self) -> Option<String> {
        match self {
            TreeRow::Inspection { id, .. } => Some((*id).to_owned()),
            TreeRow::File { id, path, .. } => Some(format!("{id}\u{0}{}", path.display())),
            _ => None,
        }
    }

    /// Indentation level.
    pub fn depth(&self) -> usize {
        match self {
            TreeRow::Root { .. } => 0,
            TreeRow::Inspection { .. } => 1,
            TreeRow::File { .. } => 2,
            TreeRow::Finding(_) => 3,
        }
    }
}

/// The results tree of `found`: a root, the inspections by name, under each its files by path and
/// their findings by line; nodes whose key is in `folded` hide their children.
pub fn inspection_tree(found: &[Located], folded: &BTreeSet<String>) -> Vec<TreeRow> {
    let mut out = vec![TreeRow::Root { count: found.len() }];
    let mut ids: Vec<(&'static str, &'static str)> = found
        .iter()
        .filter_map(|p| p.inspection)
        .map(|id| (ferris_idea_languages::inspections::name_of(id), id))
        .collect();
    ids.sort();
    ids.dedup();
    for (name, id) in ids {
        let mut members: Vec<&Located> =
            found.iter().filter(|p| p.inspection == Some(id)).collect();
        members.sort_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
        let open = !folded.contains(id);
        out.push(TreeRow::Inspection {
            id,
            name,
            count: members.len(),
            open,
        });
        if !open {
            continue;
        }
        let mut paths: Vec<&PathBuf> = members.iter().map(|p| &p.path).collect();
        paths.dedup();
        for path in paths {
            let in_file: Vec<&&Located> = members.iter().filter(|p| &p.path == path).collect();
            let row = TreeRow::File {
                id,
                path: path.clone(),
                count: in_file.len(),
                open: true,
            };
            let key = row.fold_key().unwrap_or_default();
            let open = !folded.contains(&key);
            out.push(TreeRow::File {
                id,
                path: path.clone(),
                count: in_file.len(),
                open,
            });
            if open {
                out.extend(in_file.into_iter().map(|p| TreeRow::Finding((*p).clone())));
            }
        }
    }
    out
}

/// The lines of a preview around a problem (`SPEC-0092` v4 behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// 0-based number of the first line.
    pub first: usize,
    /// The lines.
    pub lines: Vec<String>,
    /// 0-based number of the problem's line.
    pub line: usize,
}

/// Lines above and below the problem in the preview.
pub const PREVIEW_RADIUS: usize = 5;

/// The preview of the problem on 0-based `line` of `text`.
pub fn preview(text: &str, line: usize) -> Preview {
    let (first, lines) =
        ferris_idea_languages::inspections::preview_lines(text, line, PREVIEW_RADIUS);
    Preview {
        first,
        line: line.min(first + lines.len().saturating_sub(1)),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str, line: usize, severity: Severity) -> Located {
        Located {
            path: PathBuf::from(path),
            line,
            column: 0,
            offset: 0,
            severity,
            message: format!("{path}:{line}"),
            inspection: None,
        }
    }

    #[test]
    fn file_and_project_rows_counts_and_stepping() {
        let all = [
            p("/b.c", 3, Severity::Warning),
            p("/a.c", 5, Severity::Warning),
            p("/b.c", 1, Severity::Error),
            p("/a.c", 5, Severity::Error),
        ];
        let file = rows(&all[1..2], Scope::File);
        assert_eq!(file.len(), 2, "a heading and the problem");
        let proj = rows(&all, Scope::Project);
        assert!(
            matches!(&proj[0], Row::File { path, errors: 1, warnings: 1 } if path.ends_with("a.c"))
        );
        assert!(
            matches!(&proj[1], Row::Problem(q) if q.severity == Severity::Error && q.line == 5)
        );
        assert!(matches!(&proj[3], Row::File { path, .. } if path.ends_with("b.c")));
        assert_eq!(step(&proj, None, true), Some(1));
        assert_eq!(step(&proj, Some(2), true), Some(4), "skips the heading");
        assert_eq!(step(&proj, Some(1), false), Some(5), "wraps around");
        assert_eq!(summary(&all), "2 errors, 2 warnings");
        assert_eq!(summary(&all[..1]), "1 warning");
        assert_eq!(summary(&[]), "No problems");
        let mixed = rows(
            &[
                p("/a.c", 1, Severity::Warning),
                p("/z.c", 1, Severity::Error),
            ],
            Scope::Project,
        );
        assert!(matches!(&mixed[0], Row::File { path, .. } if path.ends_with("z.c")));
    }

    // SPEC-0092 v3 AK-4.
    #[test]
    fn file_tab_heading_severity_order_grouping_sorting_and_filters() {
        let mut own = p("/A.java", 2, Severity::Weak);
        own.inspection = Some("java.unused-import");
        own.message = "b unused".into();
        let mut eq = p("/A.java", 11, Severity::Warning);
        eq.inspection = Some("java.string-equality");
        eq.message = "a equality".into();
        let mut error = p("/A.java", 30, Severity::Error);
        error.message = "z broken".into();
        let list = [own.clone(), eq.clone(), error.clone()];
        let r = rows(&list, Scope::File);
        assert!(matches!(
            &r[0],
            Row::File {
                errors: 1,
                warnings: 2,
                ..
            }
        ));
        assert_eq!(r[1], Row::Problem(error.clone()), "errors first");
        assert_eq!(r[2], Row::Problem(eq.clone()));
        assert_eq!(r[3], Row::Problem(own.clone()));
        let grouped = rows_with(
            &list,
            Scope::File,
            &ViewOptions {
                group_by_inspection: true,
                ..Default::default()
            },
        );
        assert_eq!(
            grouped[1],
            Row::Problem(error.clone()),
            "server problems on top"
        );
        assert!(matches!(&grouped[2], Row::Group { count: 1, .. }));
        let by_name = rows_with(
            &list,
            Scope::File,
            &ViewOptions {
                sort_by_name: true,
                ..Default::default()
            },
        );
        assert_eq!(by_name[1], Row::Problem(eq.clone()));
        let errors_only = rows_with(
            &list,
            Scope::File,
            &ViewOptions {
                show_warnings: false,
                show_weak: false,
                ..Default::default()
            },
        );
        assert_eq!(errors_only.len(), 2);
        assert_eq!(file_tab_label(3), "File 3");
        assert_eq!(file_tab_label(0), "File");
        assert_eq!(problem_count(1), "1 problem");
        assert_eq!(summary(&list), "1 error, 1 warning, 1 weak warning");
    }

    // SPEC-0098 v3 AK-5.
    #[test]
    fn inspection_tree_scopes() {
        let root = std::path::Path::new("/p");
        let choices = InspectScope::choices(Some(std::path::Path::new("/p/src/A.java")));
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0].label(root), "Whole project");
        assert_eq!(choices[1].label(root), "Current file (src/A.java)");
        assert_eq!(choices[2].label(root), "Folder of the current file (src/)");
        assert_eq!(choices[1].tab(), "Inspection: A.java");
        assert!(choices[2].covers(root, std::path::Path::new("src/B.java")));
        assert!(!choices[1].covers(root, std::path::Path::new("src/B.java")));
        assert_eq!(InspectScope::choices(None), [InspectScope::Project]);
    }

    // SPEC-0098 v4 AK-6.
    #[test]
    fn inspection_tab_names() {
        let scope = InspectScope::Project;
        assert_eq!(scope.tab_for(None), "Inspection: Project");
        assert_eq!(
            scope.tab_for(Some("java.string-equality")),
            "Inspection: String comparison with '=='"
        );
        assert_eq!(
            scope.tab_for(Some("java.unused-declaration")),
            "Inspection: Unused declaration"
        );
    }

    // SPEC-0092 v4 AK-5.
    #[test]
    fn preview_lines_around_a_problem() {
        let text: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let start = preview(&text, 0);
        assert_eq!((start.first, start.line, start.lines.len()), (0, 0, 6));
        assert_eq!(start.lines[0], "line 1");
        let middle = preview(&text, 10);
        assert_eq!((middle.first, middle.line, middle.lines.len()), (5, 10, 11));
        assert_eq!(middle.lines[5], "line 11");
        // The text ends with a line break: the last line is empty.
        let end = preview(&text, 19);
        assert_eq!((end.first, end.line), (14, 19));
        assert_eq!(end.lines.last().map(String::as_str), Some(""));
        let past = preview("one\ntwo", 9);
        assert_eq!((past.first, past.line, past.lines.len()), (0, 1, 2));
    }

    #[test]
    fn inspection_tree_groups_by_inspection_and_folds() {
        let mut a = p("/src/B.java", 4, Severity::Warning);
        a.inspection = Some("java.string-equality");
        let mut b = p("/src/A.java", 9, Severity::Warning);
        b.inspection = Some("java.string-equality");
        let mut c = p("/src/A.java", 2, Severity::Warning);
        c.inspection = Some("java.empty-catch");
        let found = [a.clone(), b.clone(), c.clone()];
        let tree = inspection_tree(&found, &BTreeSet::new());
        assert_eq!(tree[0], TreeRow::Root { count: 3 });
        assert!(matches!(
            &tree[1],
            TreeRow::Inspection {
                id: "java.empty-catch",
                count: 1,
                ..
            }
        ));
        assert!(matches!(
            &tree[4],
            TreeRow::Inspection {
                id: "java.string-equality",
                count: 2,
                ..
            }
        ));
        assert!(
            matches!(&tree[5], TreeRow::File { path, count: 1, .. } if path.ends_with("A.java"))
        );
        assert_eq!(tree[6], TreeRow::Finding(b.clone()));
        assert_eq!(tree[6].depth(), 3);
        let folded: BTreeSet<String> = ["java.string-equality".to_owned()].into();
        let tree = inspection_tree(&found, &folded);
        assert_eq!(tree.len(), 5);
        assert!(matches!(&tree[4], TreeRow::Inspection { open: false, .. }));
    }
}
