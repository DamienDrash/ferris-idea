//! Attachments from the tool windows for the AI chat (`SPEC-0313` behaviour 3), free of the UI
//! toolkit: failed tests of a report, the problems of a file, the working tree's Git diff.

use std::path::Path;

use idecraft_ai::live::FailedTest;
use idecraft_core::{TestReport, TestStatus, TestSuite};

use crate::problems_model::Located;

/// The failed and erroneous tests of `report`, suites joined with ` › `.
pub fn failed_tests(report: &TestReport) -> Vec<FailedTest> {
    fn walk(suite: &TestSuite, prefix: &str, out: &mut Vec<FailedTest>) {
        let here = if prefix.is_empty() {
            suite.name.clone()
        } else if suite.name.is_empty() {
            prefix.to_owned()
        } else {
            format!("{prefix} › {}", suite.name)
        };
        for t in &suite.tests {
            if matches!(t.status, TestStatus::Failed | TestStatus::Error) {
                let details = [&t.details, &t.output]
                    .into_iter()
                    .flatten()
                    .map(|s| s.trim_end())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                out.push(FailedTest {
                    name: if here.is_empty() {
                        t.name.clone()
                    } else {
                        format!("{here} › {}", t.name)
                    },
                    message: t.message.clone().unwrap_or_default(),
                    details,
                });
            }
        }
        for s in &suite.suites {
            walk(s, &here, out);
        }
    }
    let mut out = Vec::new();
    for s in &report.suites {
        walk(s, "", &mut out);
    }
    out
}

/// The text of problems: `file:line:column: severity: message` per problem, 1-based, with the
/// path relative to `root`.
pub fn problems_text(problems: &[Located], root: &Path) -> String {
    problems
        .iter()
        .map(|p| {
            let path = p.path.strip_prefix(root).unwrap_or(&p.path);
            let severity = match p.severity {
                crate::diagnostics_model::Severity::Error => "error",
                crate::diagnostics_model::Severity::Warning => "warning",
                crate::diagnostics_model::Severity::Weak => "info",
            };
            format!(
                "{}:{}:{}: {severity}: {}",
                path.display(),
                p.line + 1,
                p.column + 1,
                p.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The working tree's diff against `HEAD` (staged and unstaged changes of tracked files) and the
/// untracked files, with the number of files; `None` outside a repository.
pub fn git_diff(root: &Path) -> Option<(String, usize)> {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    git(&["rev-parse", "--is-inside-work-tree"])?;
    // A repository without commits has no HEAD: everything is new.
    let diff = git(&["diff", "HEAD", "--no-color", "--no-ext-diff"])
        .or_else(|| git(&["diff", "--cached", "--no-color", "--no-ext-diff"]))
        .unwrap_or_default();
    let untracked = git(&["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    let mut files = diff
        .lines()
        .filter(|l| l.starts_with("diff --git "))
        .count();
    let mut text = diff.trim_end().to_owned();
    let new: Vec<&str> = untracked.lines().filter(|l| !l.is_empty()).collect();
    if !new.is_empty() {
        files += new.len();
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("Untracked files:\n");
        text.push_str(&new.join("\n"));
    }
    Some((text, files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use idecraft_core::{ReportFormat, parse_report};

    #[test]
    fn failed_tests_with_suites_message_and_details() {
        let report = parse_report(
            r#"<testsuites><testsuite name="calc"><testcase name="adds"/><testcase name="divides"><failure message="expected 2, got 3">at calc.rs:3</failure></testcase></testsuite></testsuites>"#,
            Some(ReportFormat::Junit),
        )
        .unwrap();
        let failed = failed_tests(&report);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].name, "calc › divides");
        assert_eq!(failed[0].message, "expected 2, got 3");
        assert!(failed[0].details.contains("at calc.rs:3"));
    }

    #[test]
    fn problems_as_lines() {
        let p = Located {
            path: "/ws/src/a.rs".into(),
            line: 2,
            column: 4,
            offset: 0,
            severity: crate::diagnostics_model::Severity::Error,
            message: "mismatched types".into(),
            inspection: None,
        };
        assert_eq!(
            problems_text(&[p], Path::new("/ws")),
            "src/a.rs:3:5: error: mismatched types"
        );
    }

    #[test]
    fn git_diff_of_a_repository_with_a_change_and_a_new_file() {
        let dir = std::env::temp_dir().join(format!("idecraft-ai-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(git_diff(&dir), None, "not a repository");
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .unwrap()
        };
        run(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-qm",
            "a",
        ]);
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let (text, files) = git_diff(&dir).unwrap();
        assert_eq!(files, 2);
        assert!(text.contains("-one\n+two"), "{text}");
        assert!(text.ends_with("Untracked files:\nb.txt"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
