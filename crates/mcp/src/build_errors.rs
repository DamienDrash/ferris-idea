//! The errors and warnings of a build's output (`SPEC-0006` v56 behaviour 12, `build_project`):
//! the common forms of the compilers IDECraft builds with — rustc through Cargo, javac and
//! kotlinc through Maven and Gradle, the Go compiler, GCC and Clang, and tsc. One problem per
//! message, in output order, at most [`MAX_PROBLEMS`].

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

/// The most problems one answer lists; `more` counts the rest.
pub const MAX_PROBLEMS: usize = 200;

/// One problem of a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildProblem {
    /// `error` or `warning`.
    pub severity: String,
    /// The file as the compiler names it (made workspace-relative by the caller where it can).
    pub file: Option<String>,
    /// 1-based line.
    pub line: Option<u32>,
    /// 1-based column.
    pub column: Option<u32>,
    /// The message.
    pub message: String,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

/// The problems of `output` (stdout and stderr together) and how many more there were.
pub fn parse(output: &str) -> (Vec<BuildProblem>, usize) {
    static RUST_HEAD: OnceLock<Regex> = OnceLock::new();
    static RUST_AT: OnceLock<Regex> = OnceLock::new();
    static MAVEN: OnceLock<Regex> = OnceLock::new();
    static KOTLIN: OnceLock<Regex> = OnceLock::new();
    static TSC: OnceLock<Regex> = OnceLock::new();
    static GCC: OnceLock<Regex> = OnceLock::new();
    static PLAIN: OnceLock<Regex> = OnceLock::new();
    let rust_head = re(&RUST_HEAD, r"^(error|warning)(?:\[[A-Za-z0-9]+\])?: (.+)$");
    let rust_at = re(&RUST_AT, r"^\s*--> (.+?):(\d+):(\d+)\s*$");
    let maven = re(
        &MAVEN,
        r"^\[(ERROR|WARNING)\] (?:(.+?):\[(\d+),(\d+)\] )?(.+)$",
    );
    let kotlin = re(&KOTLIN, r"^(e|w): (?:file://)?(.+?):(\d+):(\d+):? (.+)$");
    let tsc = re(
        &TSC,
        r"^(.+?)\((\d+),(\d+)\): (error|warning) (?:TS\d+: )?(.+)$",
    );
    let gcc = re(
        &GCC,
        r"^(.+?):(\d+):(\d+): (?:fatal )?(error|warning): (.+)$",
    );
    // Go and javac without a severity word: `file:line:col: message`, `file:line: error: …`.
    let plain = re(
        &PLAIN,
        r"^(\S+?\.(?:go|java)):(\d+):(?:(\d+):)? (?:(error|warning): )?(.+)$",
    );

    let mut problems: Vec<BuildProblem> = Vec::new();
    let mut more = 0;
    let mut push = |p: BuildProblem, problems: &mut Vec<BuildProblem>| {
        if problems.len() < MAX_PROBLEMS {
            problems.push(p);
        } else {
            more += 1;
        }
    };
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_end_matches('\r');
        i += 1;
        if let Some(c) = rust_head.captures(line) {
            let message = c[2].to_owned();
            // A summary line, not a problem of its own.
            if message.starts_with("could not compile")
                || message.starts_with("aborting due to")
                || (c[1] == *"warning"
                    && message.contains("generated")
                    && message.contains("warning"))
            {
                continue;
            }
            let mut p = BuildProblem {
                severity: c[1].to_owned(),
                file: None,
                line: None,
                column: None,
                message,
            };
            if let Some(at) = lines.get(i).and_then(|l| rust_at.captures(l)) {
                p.file = Some(at[1].to_owned());
                p.line = at[2].parse().ok();
                p.column = at[3].parse().ok();
                i += 1;
            }
            push(p, &mut problems);
            continue;
        }
        if let Some(c) = maven.captures(line) {
            let message = c[5].trim().to_owned();
            // Maven's own framing lines carry no problem.
            if c.get(2).is_none()
                && (message.is_empty()
                    || message.starts_with("Failed to execute goal")
                    || message.starts_with("->")
                    || message.starts_with("To see the full")
                    || message.starts_with("Re-run Maven")
                    || message.starts_with("For more information")
                    || message.starts_with("[Help")
                    || message.starts_with("COMPILATION ERROR")
                    || message.contains("BUILD FAILURE"))
            {
                continue;
            }
            push(
                BuildProblem {
                    severity: if &c[1] == "ERROR" { "error" } else { "warning" }.to_owned(),
                    file: c.get(2).map(|m| m.as_str().to_owned()),
                    line: c.get(3).and_then(|m| m.as_str().parse().ok()),
                    column: c.get(4).and_then(|m| m.as_str().parse().ok()),
                    message,
                },
                &mut problems,
            );
            continue;
        }
        let found = if let Some(c) = kotlin.captures(line) {
            Some((
                if &c[1] == "e" { "error" } else { "warning" }.to_owned(),
                c[2].to_owned(),
                c[3].parse().ok(),
                c[4].parse().ok(),
                c[5].to_owned(),
            ))
        } else if let Some(c) = tsc.captures(line) {
            Some((
                c[4].to_owned(),
                c[1].to_owned(),
                c[2].parse().ok(),
                c[3].parse().ok(),
                c[5].to_owned(),
            ))
        } else if let Some(c) = gcc.captures(line) {
            Some((
                c[4].to_owned(),
                c[1].to_owned(),
                c[2].parse().ok(),
                c[3].parse().ok(),
                c[5].to_owned(),
            ))
        } else {
            plain.captures(line).map(|c| {
                (
                    c.get(4).map_or("error", |m| m.as_str()).to_owned(),
                    c[1].to_owned(),
                    c[2].parse().ok(),
                    c.get(3).and_then(|m| m.as_str().parse().ok()),
                    c[5].to_owned(),
                )
            })
        };
        if let Some((severity, file, line, column, message)) = found {
            push(
                BuildProblem {
                    severity,
                    file: Some(file),
                    line,
                    column,
                    message,
                },
                &mut problems,
            );
        }
    }
    (problems, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(p: &BuildProblem) -> String {
        format!(
            "{} {}:{}:{} {}",
            p.severity,
            p.file.as_deref().unwrap_or("-"),
            p.line.unwrap_or(0),
            p.column.unwrap_or(0),
            p.message
        )
    }

    /// SPEC-0006 v56 AK-10: the compilers' forms, summary lines left out.
    #[test]
    fn the_compilers_forms_are_read() {
        let output = "\
   Compiling shop v0.1.0 (/tmp/shop)
error[E0425]: cannot find value `y` in this scope
 --> src/main.rs:3:13
  |
warning: unused variable: `x`
  --> src/lib.rs:10:9
warning: `shop` (bin \"shop\") generated 1 warning
error: could not compile `shop` (bin \"shop\") due to 1 previous error
[ERROR] /p/src/main/java/a/A.java:[12,5] cannot find symbol
[WARNING] /p/src/main/java/a/B.java:[3,1] [deprecation] old() in a.C has been deprecated
[ERROR] Failed to execute goal org.apache.maven.plugins:maven-compiler-plugin:3.13.0:compile
[ERROR] -> [Help 1]
e: file:///p/src/main/kotlin/K.kt:7:9 Unresolved reference: foo
./main.go:12:5: undefined: x
src/a.ts(4,7): error TS2322: Type 'string' is not assignable to type 'number'.
calc.cpp:9:3: error: use of undeclared identifier 'q'
src/main/java/a/D.java:5: error: ';' expected
";
        let (problems, more) = parse(output);
        let got: Vec<String> = problems.iter().map(short).collect();
        assert_eq!(
            got,
            [
                "error src/main.rs:3:13 cannot find value `y` in this scope",
                "warning src/lib.rs:10:9 unused variable: `x`",
                "error /p/src/main/java/a/A.java:12:5 cannot find symbol",
                "warning /p/src/main/java/a/B.java:3:1 [deprecation] old() in a.C has been deprecated",
                "error /p/src/main/kotlin/K.kt:7:9 Unresolved reference: foo",
                "error ./main.go:12:5 undefined: x",
                "error src/a.ts:4:7 Type 'string' is not assignable to type 'number'.",
                "error calc.cpp:9:3 use of undeclared identifier 'q'",
                "error src/main/java/a/D.java:5:0 ';' expected",
            ]
        );
        assert_eq!(more, 0);
    }

    #[test]
    fn the_list_is_bounded() {
        let output = "./a.go:1:1: x\n".repeat(MAX_PROBLEMS + 5);
        let (problems, more) = parse(&output);
        assert_eq!((problems.len(), more), (MAX_PROBLEMS, 5));
    }
}
