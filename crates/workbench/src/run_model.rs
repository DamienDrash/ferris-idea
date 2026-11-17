//! Console lines of the Run tool window (`SPEC-0087`), free of the UI toolkit.

use std::collections::VecDeque;

use idecraft_core::{RunEvent, RunOutcome};

/// The status text when documents could not be saved before a start (behaviour 3a), naming the
/// first; `None` when none is left.
pub fn unsaved_refusal(left: &[String]) -> Option<String> {
    let first = left.first()?;
    Some(match left.len() {
        1 => format!("Not started: could not save {first}"),
        n => format!("Not started: could not save {first} and {} more", n - 1),
    })
}

/// Most lines a console keeps (behaviour 4).
pub const MAX_LINES: usize = 10_000;

/// Colour of a console line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Command lines and closing texts (`CONSOLE_SYSTEM_OUTPUT`).
    System,
    /// Standard output (`CONSOLE_NORMAL_OUTPUT`).
    Normal,
    /// Standard error (`CONSOLE_ERROR_OUTPUT`).
    Error,
}

/// The output of one configuration's runs.
#[derive(Debug, Default)]
pub struct Console {
    lines: VecDeque<(LineKind, String)>,
    /// Whether the run is still going.
    pub running: bool,
    /// Exit code of the last finished step (`SPEC-0313` behaviour 9).
    pub exit_code: Option<i32>,
}

/// A command line as a shell reads it back: arguments with spaces, quotes or other characters a
/// shell gives a meaning are quoted, so copying it into a terminal runs the same program with the
/// same arguments (`SPEC-0087` v6 behaviour 13, IMP-0395).
pub fn command_line(command: &[String]) -> String {
    idecraft_core::join_command_line(command)
}

/// The line the Run window shows for a start that cannot happen (`SPEC-0087` v6 behaviour 14,
/// IMP-0394), also the status text.
pub fn cannot_start(name: &str, reason: &str) -> String {
    format!("Cannot start {name}: {reason}")
}

/// The closing lines of a finished step (behaviour 4).
pub fn finished_lines(o: &RunOutcome) -> Vec<(LineKind, String)> {
    let mut out = Vec::new();
    if let Some(e) = &o.start_error {
        out.push((LineKind::Error, format!("Cannot run program: {e}")));
        return out;
    }
    if let Some(e) = &o.report_error {
        out.push((LineKind::Error, format!("Test report: {e}")));
    }
    if let Some(r) = &o.report {
        let t = &r.totals;
        out.push((
            LineKind::System,
            format!(
                "Tests passed: {}, failed: {}, skipped: {}",
                t.passed,
                t.failed + t.errors,
                t.skipped
            ),
        ));
    }
    // Without a code the process group was killed with SIGKILL: 128 + 9, as shells report it.
    let code = o.exit_code.unwrap_or(137);
    let why = if o.stopped {
        " (stopped)"
    } else if o.timed_out {
        " (timed out)"
    } else {
        ""
    };
    out.push((
        LineKind::System,
        format!("Process finished with exit code {code}{why}"),
    ));
    out
}

impl Console {
    /// A fresh console for a new run.
    pub fn starting() -> Self {
        Self {
            lines: VecDeque::new(),
            running: true,
            exit_code: None,
        }
    }

    /// Empties the console; a run that goes on keeps adding to it (`SPEC-0087` v6 behaviour 12).
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Adds a line, dropping the oldest beyond [`MAX_LINES`].
    pub fn push(&mut self, kind: LineKind, text: String) {
        if self.lines.len() == MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back((kind, text));
    }

    /// Adds what an event of the run shows.
    pub fn apply(&mut self, event: &RunEvent) {
        match event {
            RunEvent::Started { command, .. } => {
                if !self.lines.is_empty() {
                    self.push(LineKind::System, String::new());
                }
                self.push(LineKind::System, command_line(command));
            }
            RunEvent::Output { stderr, text } => {
                let kind = if *stderr {
                    LineKind::Error
                } else {
                    LineKind::Normal
                };
                self.push(kind, text.clone());
            }
            RunEvent::Finished(o) => {
                self.exit_code = o.exit_code;
                for (kind, text) in finished_lines(o) {
                    self.push(kind, text);
                }
            }
            RunEvent::ChainStopped { failed } => {
                self.push(LineKind::Error, format!("Run stopped: {failed} failed"));
            }
        }
    }

    /// The kept lines, oldest first.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &(LineKind, String)> {
        self.lines.iter()
    }

    /// The kept text, one line per row.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The status after Run of a page configuration without a command: it only opened the page, so the
/// status says how to debug it (hand test 60: "Debug in Browser" chosen with Enter opened the
/// user's own browser and no debugger, which looked like a broken debug start).
pub fn page_run_status(c: &idecraft_core::RunConfig, debug_key: &str) -> Option<String> {
    let url = c.url.as_ref().filter(|_| c.command.is_empty())?;
    Some(format!(
        "Opened {url} in the default browser · Debug{} debugs it in Chrome with its own profile",
        crate::keymaps::in_parens(debug_key)
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn running_a_page_says_how_to_debug_it() {
        let config = |command: Vec<String>| idecraft_core::RunConfig {
            name: "Debug in Browser".into(),
            command,
            cwd: None,
            env: Default::default(),
            kind: idecraft_core::RunKind::Run,
            report: None,
            report_file: None,
            before: Vec::new(),
            timeout: None,
            url: Some("http://localhost:5173".into()),
            folder: None,
            allow_multiple: false,
            before_launch: Vec::new(),
            verbatim: false,
        };
        let page = config(Vec::new());
        assert_eq!(
            super::page_run_status(&page, "⌃D").as_deref(),
            Some(
                "Opened http://localhost:5173 in the default browser · Debug (⌃D) debugs it in Chrome with its own profile"
            )
        );
        let server = config(vec!["npm".into(), "run".into(), "dev".into()]);
        assert_eq!(super::page_run_status(&server, "⌃D"), None);
    }

    use super::*;

    fn outcome(code: Option<i32>, stopped: bool, timed_out: bool) -> RunOutcome {
        RunOutcome {
            name: "app".into(),
            exit_code: code,
            timed_out,
            stopped,
            start_error: None,
            stdout: String::new(),
            stderr: String::new(),
            millis: 1,
            report: None,
            report_error: None,
            browser: Vec::new(),
        }
    }

    #[test]
    fn events_become_lines() {
        let mut c = Console::starting();
        c.apply(&RunEvent::Started {
            name: "app".into(),
            command: vec!["cargo".into(), "run".into(), "a b".into()],
        });
        c.apply(&RunEvent::Output {
            stderr: false,
            text: "hi".into(),
        });
        c.apply(&RunEvent::Output {
            stderr: true,
            text: "warn".into(),
        });
        c.apply(&RunEvent::Finished(Box::new(outcome(
            Some(0),
            false,
            false,
        ))));
        let lines: Vec<(LineKind, &str)> = c.lines().map(|(k, t)| (*k, t.as_str())).collect();
        assert_eq!(
            lines,
            [
                (LineKind::System, "cargo run 'a b'"),
                (LineKind::Normal, "hi"),
                (LineKind::Error, "warn"),
                (LineKind::System, "Process finished with exit code 0"),
            ]
        );
        c.apply(&RunEvent::Started {
            name: "next".into(),
            command: vec!["make".into()],
        });
        c.apply(&RunEvent::ChainStopped {
            failed: "gen".into(),
        });
        assert!(c.text().ends_with("\n\nmake\nRun stopped: gen failed"));
    }

    #[test]
    fn closing_texts_and_limit() {
        let text = |o: &RunOutcome| finished_lines(o).last().unwrap().1.clone();
        assert_eq!(
            text(&outcome(Some(3), false, false)),
            "Process finished with exit code 3"
        );
        assert_eq!(
            text(&outcome(None, true, false)),
            "Process finished with exit code 137 (stopped)"
        );
        assert_eq!(
            text(&outcome(None, false, true)),
            "Process finished with exit code 137 (timed out)"
        );
        let mut failed = outcome(None, false, false);
        failed.start_error = Some("nope: not found".into());
        assert_eq!(text(&failed), "Cannot run program: nope: not found");
        let mut c = Console::starting();
        for i in 0..MAX_LINES + 5 {
            c.push(LineKind::Normal, i.to_string());
        }
        assert_eq!(c.lines().len(), MAX_LINES);
        assert_eq!(c.lines().next().unwrap().1, "5");
        assert_eq!(command_line(&["it's".into()]), "'it'\\''s'");
    }

    /// SPEC-0087 v6 AK-7: the shown command line is quoted so a shell runs the same arguments.
    #[test]
    fn the_shown_command_line_is_quoted() {
        let shown = command_line(&[
            "node".into(),
            "src/main.js".into(),
            "two words".into(),
            "it's".into(),
        ]);
        assert_eq!(shown, "node src/main.js 'two words' 'it'\\''s'");
        assert_eq!(
            idecraft_core::split_command_line(&shown).unwrap(),
            ["node", "src/main.js", "two words", "it's"]
        );
        assert_eq!(
            cannot_start("app", "program not found: node"),
            "Cannot start app: program not found: node"
        );
        let mut c = Console::starting();
        c.push(LineKind::Normal, "a".into());
        c.clear();
        assert_eq!(c.lines().len(), 0);
        assert!(c.running);
    }

    /// SPEC-0087 v5 AK-6.
    #[test]
    fn a_document_that_cannot_be_saved_stops_the_start() {
        assert_eq!(unsaved_refusal(&[]), None);
        assert_eq!(
            unsaved_refusal(&["a.js".into()]).as_deref(),
            Some("Not started: could not save a.js")
        );
        assert_eq!(
            unsaved_refusal(&["a.js".into(), "b.js".into(), "c.js".into()]).as_deref(),
            Some("Not started: could not save a.js and 2 more")
        );
    }
}
