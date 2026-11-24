//! The usages of the symbol at the caret as an editor marks them (`SPEC-0295`), free of the UI
//! toolkit: when a question to the language server is due, which answer is still wanted, which
//! marks are drawn and when they go, the marks kept by `HighlightUsagesInFile`, the step to the
//! next and previous usage (v2 behaviour 15), and the words of the status bar.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ferris_idea_languages::lsp::LspError;
use ferris_idea_languages::occurrences::{Highlights, Occurrence, OccurrenceKind, neighbour};

/// How long the caret rests on a name before the server is asked (behaviour 1).
pub const REST: Duration = Duration::from_millis(300);
/// How often the rest is checked (behaviour 1).
pub const TICK: Duration = Duration::from_millis(100);
/// How long a question waits for its answer (behaviour 2).
pub const WAIT: Duration = Duration::from_secs(10);
/// The most occurrences an editor draws (behaviour 11).
pub const MAX_MARKS: usize = 1000;

/// One marked occurrence: a char range of the text and its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// Char range in the text.
    pub range: Range<usize>,
    /// Read, write or text.
    pub kind: OccurrenceKind,
}

/// What the model sees of an editor: its text's revision and its caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// The editor's count of text changes.
    pub revision: u64,
    /// Char offset of the primary caret.
    pub caret: usize,
    /// One caret without a selection (behaviour 1).
    pub single: bool,
}

/// A question to send to the server.
#[derive(Debug, Clone)]
pub struct Request {
    /// Names the question when its answer comes back.
    pub id: u64,
    /// The text it is asked about.
    pub revision: u64,
    /// The caret it is asked for.
    pub caret: usize,
    /// Asked by `HighlightUsagesInFile`: the marks are kept (behaviour 8).
    pub kept: bool,
    /// Asked by a step to the next (`true`) or previous usage (v2 behaviour 15).
    pub step: Option<bool>,
    /// Set when the question's reason is gone, so it is withdrawn (behaviour 10).
    pub stop: Arc<AtomicBool>,
}

/// A step that waits for its answer: the direction, and whether the answer is also marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepAsk {
    forward: bool,
    mark: bool,
}

/// Where a step to the next or previous usage leads (v2 behaviour 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepTo {
    /// To the start of another usage: its char offset.
    Usage(usize),
    /// The caret is on the only usage.
    Alone,
}

/// What a use of the step actions does at once (v2 behaviour 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepNow {
    /// The caret was on a mark: the step is taken.
    Done(StepTo),
    /// The caret is on no mark: the server has to be asked.
    Ask,
    /// Several carets or a selection: nothing happens.
    Nothing,
}

/// The step from `caret` among the sorted `marks`: the next or previous one, around the ends;
/// `None` without marks.
pub fn step_among(marks: &[Mark], caret: usize, forward: bool) -> Option<StepTo> {
    let spans: Vec<(usize, usize)> = marks.iter().map(|m| (m.range.start, m.range.end)).collect();
    let to = neighbour(&spans, caret, forward)?;
    let (start, end) = spans[to];
    Some(if start <= caret && caret <= end {
        StepTo::Alone
    } else {
        StepTo::Usage(start)
    })
}

/// The status bar's text when the caret is on the symbol's only usage in the file.
pub fn no_other(symbol: &str) -> String {
    format!("No other usage of {symbol} in this file")
}

/// The question in flight.
#[derive(Debug)]
struct Flight {
    id: u64,
    revision: u64,
    kept: bool,
    step: Option<StepAsk>,
    stop: Arc<AtomicBool>,
    /// Its answer is no longer wanted.
    gone: bool,
}

impl Flight {
    fn withdraw(&mut self) {
        self.gone = true;
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// What became of an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// Dropped: asked for another text or caret, or withdrawn (behaviour 10).
    Stale,
    /// Nothing to mark.
    Nothing,
    /// Marked: all occurrences of the answer and how many of them are drawn (behaviour 11).
    Shown {
        /// Occurrences in the answer.
        total: usize,
        /// Occurrences drawn.
        drawn: usize,
    },
}

/// The occurrence marks of one editor view and the question behind them.
#[derive(Debug, Default)]
pub struct Occurrences {
    marks: Vec<Mark>,
    /// The marks are those of `HighlightUsagesInFile`.
    kept: bool,
    seen: Option<Place>,
    /// When `seen` was first seen: the start of the caret's rest.
    since: Option<Instant>,
    /// The text and caret last asked about by itself, so one rest asks once.
    asked: Option<(u64, usize)>,
    flight: Option<Flight>,
    /// A use of `HighlightUsagesInFile` not sent yet: text and caret.
    wanted: Option<(u64, usize)>,
    /// A step whose question is not sent yet: text, caret and the step.
    wanted_step: Option<(u64, usize, StepAsk)>,
    next_id: u64,
}

impl Occurrences {
    /// The marks to draw, sorted.
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// Whether the marks are kept until Escape (behaviour 8).
    pub fn is_kept(&self) -> bool {
        self.kept
    }

    /// Whether a question is in flight.
    pub fn in_flight(&self) -> bool {
        self.flight.is_some()
    }

    fn on_mark(&self, place: Place) -> bool {
        place.single
            && self
                .marks
                .iter()
                .any(|m| m.range.start <= place.caret && place.caret <= m.range.end)
    }

    fn clear(&mut self) -> bool {
        let had = !self.marks.is_empty();
        self.marks.clear();
        self.kept = false;
        had
    }

    /// Takes note of the editor's text and caret at `now`. A caret that left the marked symbol
    /// takes the automatic marks with it, and a question asked by itself for another place is
    /// withdrawn (behaviours 7 and 10). Returns whether the marks changed.
    pub fn caret_at(&mut self, place: Place, now: Instant) -> bool {
        if self.seen == Some(place) {
            return false;
        }
        let same_text = self.seen.is_some_and(|s| s.revision == place.revision);
        self.seen = Some(place);
        self.since = Some(now);
        if !same_text {
            return self.text_changed();
        }
        // A step is given up when the caret moves before its answer (v2 behaviour 15).
        self.wanted_step = None;
        if let Some(f) = self.flight.as_mut().filter(|f| !f.kept) {
            f.withdraw();
        }
        if !self.kept && !self.marks.is_empty() && !self.on_mark(place) {
            return self.clear();
        }
        false
    }

    /// The text changed: every mark goes, kept ones included, and no answer for the old text is
    /// wanted (behaviours 7, 8 and 10). Returns whether marks were removed.
    pub fn text_changed(&mut self) -> bool {
        if let Some(f) = self.flight.as_mut() {
            f.withdraw();
        }
        self.wanted = None;
        self.wanted_step = None;
        self.clear()
    }

    /// The automatic highlight was switched off: its marks go and its question is withdrawn
    /// (behaviour 12). Returns whether marks were removed.
    pub fn automatic_off(&mut self) -> bool {
        // A step's question was asked for, not sent by the automatic highlight.
        if let Some(f) = self.flight.as_mut().filter(|f| !f.kept && f.step.is_none()) {
            f.withdraw();
        }
        self.asked = None;
        !self.kept && self.clear()
    }

    /// `HighlightUsagesInFile` at `place`: its question is asked next; one in flight is withdrawn
    /// first (behaviours 8 and 10).
    pub fn keep(&mut self, place: Place, now: Instant) {
        self.caret_at(place, now);
        if let Some(f) = self.flight.as_mut() {
            f.withdraw();
        }
        self.wanted = Some((place.revision, place.caret));
    }

    /// The step from `place` among the marks, when its only caret is inside or at one of them: no
    /// question is needed (v2 behaviour 15). `None` otherwise.
    pub fn step(&self, place: Place, forward: bool) -> Option<StepTo> {
        self.on_mark(place)
            .then(|| step_among(&self.marks, place.caret, forward))
            .flatten()
    }

    /// A step from `place`, which is on no mark: its question is asked next, one in flight is
    /// withdrawn first. With `mark` the answer is also marked as the automatic highlight would,
    /// unless marks are kept (v2 behaviour 15).
    pub fn step_wanted(&mut self, place: Place, now: Instant, forward: bool, mark: bool) {
        self.caret_at(place, now);
        if let Some(f) = self.flight.as_mut() {
            f.withdraw();
        }
        self.wanted = None;
        self.wanted_step = Some((place.revision, place.caret, StepAsk { forward, mark }));
    }

    /// The question to send at `now`, if one is due: a use of `HighlightUsagesInFile` first, else
    /// the automatic one when `automatic` is on, the only caret has rested for [`REST`] on a name
    /// (`on_name`) that is not marked yet, and the server is `ready` — asked last, and only when
    /// everything else holds (behaviours 1, 3, 9 and 10). Never while a question is in flight.
    pub fn due(
        &mut self,
        now: Instant,
        automatic: bool,
        on_name: bool,
        ready: impl FnOnce() -> bool,
    ) -> Option<Request> {
        if self.flight.is_some() {
            return None;
        }
        let place = self.seen?;
        if let Some((revision, caret)) = self.wanted.take() {
            return (revision == place.revision).then(|| self.fly(revision, caret, true, None));
        }
        if let Some((revision, caret, step)) = self.wanted_step.take() {
            return (revision == place.revision && caret == place.caret)
                .then(|| self.fly(revision, caret, false, Some(step)));
        }
        if !automatic || self.kept || !place.single || !on_name || self.on_mark(place) {
            return None;
        }
        if self.asked == Some((place.revision, place.caret)) {
            return None;
        }
        if now.saturating_duration_since(self.since?) < REST || !ready() {
            return None;
        }
        self.asked = Some((place.revision, place.caret));
        Some(self.fly(place.revision, place.caret, false, None))
    }

    fn fly(&mut self, revision: u64, caret: usize, kept: bool, step: Option<StepAsk>) -> Request {
        self.next_id += 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.flight = Some(Flight {
            id: self.next_id,
            revision,
            kept,
            step,
            stop: Arc::clone(&stop),
            gone: false,
        });
        Request {
            id: self.next_id,
            revision,
            caret,
            kept,
            step: step.map(|s| s.forward),
            stop,
        }
    }

    /// The question `id` ended, with the `found` occurrences as sorted marks of the text of
    /// `revision` (the editor's current one), or without an answer. An answer for another text, a
    /// withdrawn one, and an automatic one whose occurrences the caret is no longer on are dropped;
    /// at most [`MAX_MARKS`] are drawn (behaviours 10 and 11).
    pub fn answer(&mut self, id: u64, revision: u64, found: Option<Vec<Mark>>) -> Taken {
        if self.flight.as_ref().is_none_or(|f| f.id != id) {
            return Taken::Stale;
        }
        let Some(flight) = self.flight.take() else {
            return Taken::Stale;
        };
        if flight.gone || flight.revision != revision {
            return Taken::Stale;
        }
        let Some(mut found) = found else {
            return Taken::Nothing;
        };
        if found.is_empty() {
            return Taken::Nothing;
        }
        let total = found.len();
        found.truncate(MAX_MARKS);
        // A step's answer is marked like an automatic one, or leaves the marks alone (v2
        // behaviour 15); the caret is still where the step was asked, or the flight were gone.
        if let Some(step) = flight.step {
            let drawn = found.len();
            if step.mark && !self.kept {
                self.marks = found;
            }
            return Taken::Shown { total, drawn };
        }
        if !flight.kept {
            let on_answer = self.seen.is_some_and(|place| {
                place.single
                    && found
                        .iter()
                        .any(|m| m.range.start <= place.caret && place.caret <= m.range.end)
            });
            if self.kept || !on_answer {
                return Taken::Stale;
            }
        }
        let drawn = found.len();
        self.marks = found;
        self.kept = flight.kept;
        Taken::Shown { total, drawn }
    }

    /// Escape: the kept marks go, and a use of `HighlightUsagesInFile` not answered yet with them.
    /// The automatic highlight does not come back for the place the caret rests on (behaviour 9).
    /// Returns whether there was something to remove — then Escape did nothing else.
    pub fn escape(&mut self) -> bool {
        let mut had = self.wanted.take().is_some();
        if let Some(f) = self.flight.as_mut().filter(|f| f.kept && !f.gone) {
            f.withdraw();
            had = true;
        }
        if self.kept {
            self.clear();
            had = true;
        }
        if had {
            self.asked = self.seen.map(|s| (s.revision, s.caret));
        }
        had
    }
}

/// The marks of the server's `found` occurrences; `offset_of` gives the char offset of a line and
/// character, `None` outside the text. Sorted as the occurrences are.
pub fn marks_of(found: &[Occurrence], offset_of: impl Fn(u64, u64) -> Option<usize>) -> Vec<Mark> {
    found
        .iter()
        .filter_map(|o| {
            let start = offset_of(o.range.start_line, o.range.start_character)?;
            let end = offset_of(o.range.end_line, o.range.end_character)?;
            (start < end).then_some(Mark {
                range: start..end,
                kind: o.kind,
            })
        })
        .collect()
}

/// The colour-scheme key of an occurrence's background and stripe mark (behaviour 4).
pub fn scheme_key(kind: OccurrenceKind) -> &'static str {
    match kind {
        OccurrenceKind::Write => "WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES",
        OccurrenceKind::Read | OccurrenceKind::Text => "IDENTIFIER_UNDER_CARET_ATTRIBUTES",
    }
}

/// A line's mark in the error stripe (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeLine {
    /// 0-based line.
    pub line: usize,
    /// Char offset of the line's first occurrence, where a click goes.
    pub offset: usize,
    /// Write when the line has a write occurrence, else the first occurrence's kind.
    pub kind: OccurrenceKind,
}

/// One stripe mark per line with an occurrence (the line an occurrence starts on).
pub fn stripe_lines(marks: &[Mark], line_of: impl Fn(usize) -> usize) -> Vec<StripeLine> {
    let mut out: Vec<StripeLine> = Vec::new();
    for m in marks {
        let line = line_of(m.range.start);
        match out.last_mut().filter(|l| l.line == line) {
            Some(last) => {
                if m.kind == OccurrenceKind::Write {
                    last.kind = OccurrenceKind::Write;
                }
            }
            None => out.push(StripeLine {
                line,
                offset: m.range.start,
                kind: m.kind,
            }),
        }
    }
    out
}

/// How a use of `HighlightUsagesInFile` ended (behaviour 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// Marked.
    Shown {
        /// Occurrences the server listed.
        total: usize,
        /// Occurrences drawn.
        drawn: usize,
    },
    /// The server knows no occurrence.
    NoUsages,
    /// The server (its name) has no usage highlights.
    NotOffered(String),
    /// The server (its name) is still loading; it was not asked.
    NotReady(String),
    /// The text is over the size limit.
    TooLarge,
    /// The text changed before the answer came.
    Changed,
    /// The caret is not on a name.
    NoName,
    /// No usable server, or the question failed: the reason.
    Failed(String),
}

impl End {
    /// From the question's `result` and what the editor did with its answer.
    pub fn of(result: &Result<Highlights, LspError>, taken: Taken) -> End {
        match (result, taken) {
            (Ok(Highlights::NotOffered(server)), _) => End::NotOffered(server.clone()),
            (Ok(Highlights::NotReady(server)), _) => End::NotReady(server.clone()),
            (Ok(Highlights::TooLarge), _) => End::TooLarge,
            (Err(LspError::Stopped(_)), _) | (_, Taken::Stale) => End::Changed,
            (Err(e), _) => End::Failed(e.to_string()),
            (Ok(Highlights::Found(_)), Taken::Shown { total, drawn }) => {
                End::Shown { total, drawn }
            }
            (Ok(Highlights::Found(_)), Taken::Nothing) => End::NoUsages,
        }
    }
}

/// The status bar's text after `HighlightUsagesInFile` on `symbol` (behaviour 8), in the words of
/// Find Usages (`SPEC-0293`).
pub fn status(symbol: &str, end: &End) -> String {
    const ESCAPE: &str = "(Escape removes the highlighting)";
    match end {
        End::Shown { total: 1, .. } => format!("1 usage of {symbol} highlighted {ESCAPE}"),
        End::Shown { total, drawn } if drawn < total => {
            format!("{total} usages of {symbol}, the first {drawn} highlighted {ESCAPE}")
        }
        End::Shown { total, .. } => format!("{total} usages of {symbol} highlighted {ESCAPE}"),
        End::NoUsages => format!("No usages of {symbol} found in this file"),
        End::NotOffered(server) => {
            format!("No usages found: {server} does not offer usage highlights")
        }
        End::NotReady(server) => {
            format!("No usages found yet: {server} is still loading the project")
        }
        End::TooLarge => "No usages found: the file is larger than 8 MiB".into(),
        End::Changed => "No usages found: the text changed meanwhile".into(),
        End::NoName => crate::usages_model::NO_NAME_AT_CARET.into(),
        End::Failed(reason) => format!("No usages found: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferris_idea_languages::insight::Encoding;
    use ferris_idea_languages::occurrences::{offered, parse};
    use serde_json::{Value, json};

    const TEXT: &str = "int total = 0;\nint other = 1;\ntotal = total + other;\n";

    fn highlight(line: u64, from: u64, to: u64, kind: u64) -> Value {
        json!({"kind": kind, "range": {
            "start": {"line": line, "character": from},
            "end": {"line": line, "character": to},
        }})
    }

    /// A scripted language server: whether it is ready, what it announces and what it answers.
    struct Server {
        ready: bool,
        capabilities: Value,
        /// The answer for a caret on `total`; anywhere else it knows no occurrence.
        total: Value,
        /// The positions it was asked at.
        asked: Vec<(u64, u64)>,
    }

    impl Server {
        fn new() -> Self {
            Self {
                ready: true,
                capabilities: json!({"documentHighlightProvider": true}),
                total: json!([
                    highlight(0, 4, 9, 3),
                    highlight(2, 0, 5, 3),
                    highlight(2, 8, 13, 2),
                ]),
                asked: Vec::new(),
            }
        }

        /// What `document_highlights_in` gives for this server.
        fn ask(&mut self, text: &str, line: u64, character: u64) -> Highlights {
            if !self.ready {
                return Highlights::NotReady("scripted-server".into());
            }
            if !offered(&self.capabilities) {
                return Highlights::NotOffered("scripted-server".into());
            }
            self.asked.push((line, character));
            let word = crate::usages_model::word_at(
                text.split('\n').nth(line as usize).unwrap_or(""),
                character as usize,
            );
            let answer = if word == "total" {
                self.total.clone()
            } else {
                json!([])
            };
            Highlights::Found(parse(&answer, text, Encoding::Utf16))
        }
    }

    /// An editor as the workbench drives it: text, caret, the setting and the clock.
    struct Editor {
        text: String,
        revision: u64,
        caret: usize,
        single: bool,
        automatic: bool,
        model: Occurrences,
        start: Instant,
        ms: u64,
    }

    impl Editor {
        fn new() -> Self {
            Self {
                text: TEXT.to_owned(),
                revision: 1,
                caret: 0,
                single: true,
                automatic: true,
                model: Occurrences::default(),
                start: Instant::now(),
                ms: 0,
            }
        }

        fn now(&self) -> Instant {
            self.start + Duration::from_millis(self.ms)
        }

        fn place(&self) -> Place {
            Place {
                revision: self.revision,
                caret: self.caret,
                single: self.single,
            }
        }

        fn position(&self, offset: usize) -> (u64, u64) {
            let before: String = self.text.chars().take(offset).collect();
            let line = before.matches('\n').count() as u64;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u64;
            (line, column)
        }

        fn offset(&self, line: u64, character: u64) -> Option<usize> {
            let mut at = 0;
            for (i, l) in self.text.split('\n').enumerate() {
                if i as u64 == line {
                    return (character as usize <= l.chars().count())
                        .then_some(at + character as usize);
                }
                at += l.chars().count() + 1;
            }
            None
        }

        /// Puts the caret at the `nth` occurrence (0-based) of `word`, `inside` chars in.
        fn caret_on(&mut self, word: &str, nth: usize, inside: usize) {
            let byte = self.text.match_indices(word).nth(nth).unwrap().0;
            self.caret = self.text[..byte].chars().count() + inside;
            self.model.caret_at(self.place(), self.now());
        }

        fn type_text(&mut self, text: &str) {
            let byte = self
                .text
                .char_indices()
                .nth(self.caret)
                .map_or(self.text.len(), |(i, _)| i);
            self.text.insert_str(byte, text);
            self.caret += text.chars().count();
            self.revision += 1;
            self.model.text_changed();
            // The editor's next frame sees the new text and caret.
            self.model.caret_at(self.place(), self.now());
        }

        /// One check of the workbench after `ms` more milliseconds: the question due, if any.
        fn tick(&mut self, ms: u64, server: &Server) -> Option<Request> {
            self.ms += ms;
            let now = self.now();
            self.model.caret_at(self.place(), now);
            if !self.automatic {
                self.model.automatic_off();
            }
            let (line, column) = self.position(self.caret);
            let on_name = !crate::usages_model::word_at(
                self.text.split('\n').nth(line as usize).unwrap_or(""),
                column as usize,
            )
            .is_empty();
            self.model
                .due(now, self.automatic, on_name, || server.ready)
        }

        /// The server answers `request` for the text it was asked about, and the editor takes it.
        fn deliver(&mut self, request: &Request, asked_text: &str, server: &mut Server) -> End {
            let stopped = request.stop.load(Ordering::SeqCst);
            let (line, column) = {
                let before: String = asked_text.chars().take(request.caret).collect();
                (
                    before.matches('\n').count() as u64,
                    before.rsplit('\n').next().unwrap_or("").chars().count() as u64,
                )
            };
            let result = if stopped {
                Err(LspError::Stopped("textDocument/documentHighlight".into()))
            } else {
                Ok(server.ask(asked_text, line, column))
            };
            let found = match &result {
                Ok(Highlights::Found(found)) => Some(marks_of(found, |l, c| self.offset(l, c))),
                _ => None,
            };
            let taken = self.model.answer(request.id, self.revision, found);
            End::of(&result, taken)
        }

        /// The action: asked at once, as the workbench does.
        fn highlight_usages(&mut self, server: &mut Server) -> End {
            self.model.keep(self.place(), self.now());
            let request = self.tick(0, server).expect("the action's question is due");
            assert!(request.kept);
            let text = self.text.clone();
            self.deliver(&request, &text, server)
        }

        fn marks(&self) -> Vec<(usize, usize, OccurrenceKind)> {
            self.model
                .marks()
                .iter()
                .map(|m| (m.range.start, m.range.end, m.kind))
                .collect()
        }
    }

    const TOTAL_MARKS: [(usize, usize, OccurrenceKind); 3] = [
        (4, 9, OccurrenceKind::Write),
        (30, 35, OccurrenceKind::Write),
        (38, 43, OccurrenceKind::Read),
    ];

    /// Rests on the first `total` until the automatic marks are there.
    fn marked() -> (Editor, Server) {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        assert!(e.tick(200, &s).is_none());
        let request = e.tick(100, &s).expect("due after the rest");
        let text = e.text.clone();
        e.deliver(&request, &text, &mut s);
        assert_eq!(e.marks(), TOTAL_MARKS);
        (e, s)
    }

    /// SPEC-0295/AK-9: with marks the step is taken among them and nothing is asked.
    #[test]
    fn a_step_among_the_marks_asks_nothing() {
        let (mut e, s) = marked();
        let mut walk = Vec::new();
        for _ in 0..3 {
            let Some(StepTo::Usage(to)) = e.model.step(e.place(), true) else {
                panic!("a step among three marks");
            };
            e.caret = to;
            e.model.caret_at(e.place(), e.now());
            assert_eq!(e.marks(), TOTAL_MARKS, "the marks stay");
            walk.push(to);
        }
        assert_eq!(walk, [30, 38, 4], "next, next, around the end");
        assert_eq!(e.model.step(e.place(), false), Some(StepTo::Usage(38)));
        assert!(e.tick(1000, &s).is_none());
        assert_eq!(s.asked.len(), 1, "only the automatic question was asked");
        // Off the marks, or with a selection, there is no step among them.
        e.single = false;
        assert_eq!(e.model.step(e.place(), true), None);
        e.single = true;
        e.caret = 11;
        assert_eq!(e.model.step(e.place(), true), None);

        // The only usage: the caret stays, and the status bar says so.
        let (mut e, mut s) = (Editor::new(), Server::new());
        s.total = json!([highlight(0, 4, 9, 3)]);
        e.caret_on("total", 0, 2);
        let request = e.tick(300, &s).expect("due");
        let text = e.text.clone();
        e.deliver(&request, &text, &mut s);
        assert_eq!(e.model.step(e.place(), true), Some(StepTo::Alone));
        assert_eq!(e.model.step(e.place(), false), Some(StepTo::Alone));
        assert_eq!(no_other("total"), "No other usage of total in this file");
        assert_eq!(step_among(&[], 3, true), None);
    }

    /// SPEC-0295/AK-9: without marks the server is asked at once; the answer gives the target
    /// and the marks. A question in flight is withdrawn and ends first.
    #[test]
    fn a_step_without_marks_asks_the_server() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 1, 2);
        assert_eq!(e.model.step(e.place(), true), None);
        e.model.step_wanted(e.place(), e.now(), true, true);
        let request = e.tick(0, &s).expect("asked without a rest");
        assert_eq!((request.step, request.kept), (Some(true), false));
        assert_eq!((request.revision, request.caret), (1, 32));
        let text = e.text.clone();
        assert_eq!(
            e.deliver(&request, &text, &mut s),
            End::Shown { total: 3, drawn: 3 }
        );
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert!(!e.model.is_kept());
        assert_eq!(
            step_among(e.model.marks(), request.caret, true),
            Some(StepTo::Usage(38))
        );
        assert_eq!(
            step_among(e.model.marks(), request.caret, false),
            Some(StepTo::Usage(4))
        );
        // On the target the caret is on a mark: the next step asks nothing.
        e.caret = 38;
        e.model.caret_at(e.place(), e.now());
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert_eq!(e.model.step(e.place(), true), Some(StepTo::Usage(4)));
        assert!(e.tick(1000, &s).is_none());

        // The automatic question is in flight: the step withdraws it and follows when it ended.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let automatic = e.tick(300, &s).expect("due");
        e.model.step_wanted(e.place(), e.now(), false, true);
        assert!(automatic.stop.load(Ordering::SeqCst));
        assert!(e.tick(0, &s).is_none(), "one question at a time");
        let text = e.text.clone();
        assert_eq!(e.deliver(&automatic, &text, &mut s), End::Changed);
        let request = e.tick(0, &s).expect("the step follows");
        assert_eq!(request.step, Some(false));
        e.deliver(&request, &text, &mut s);
        assert_eq!(
            step_among(e.model.marks(), request.caret, false),
            Some(StepTo::Usage(38)),
            "before the first usage comes the last"
        );
    }

    /// SPEC-0295/AK-9: with the automatic highlight off, or with kept marks of another symbol,
    /// the step's answer changes no mark.
    #[test]
    fn a_step_changes_no_mark_with_the_highlight_off_or_kept_marks() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.automatic = false;
        e.caret_on("total", 0, 2);
        assert!(e.tick(1000, &s).is_none());
        e.model.step_wanted(e.place(), e.now(), true, false);
        let request = e.tick(0, &s).expect("asked although the highlight is off");
        let text = e.text.clone();
        assert_eq!(
            e.deliver(&request, &text, &mut s),
            End::Shown { total: 3, drawn: 3 }
        );
        assert!(e.marks().is_empty());

        // Kept marks of `total`; the step is asked for `other`, which the server does not know.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        assert_eq!(
            e.highlight_usages(&mut s),
            End::Shown { total: 3, drawn: 3 }
        );
        e.caret_on("other", 0, 1);
        assert_eq!(e.model.step(e.place(), true), None);
        e.model.step_wanted(e.place(), e.now(), true, true);
        let request = e.tick(0, &s).expect("asked for the other symbol");
        let text = e.text.clone();
        assert_eq!(e.deliver(&request, &text, &mut s), End::NoUsages);
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert!(e.model.is_kept());
        // An answer with occurrences leaves kept marks alone as well.
        s.total = json!([highlight(1, 4, 9, 2)]);
        e.text = e.text.replace("other = 1", "total = 1");
        e.model.step_wanted(e.place(), e.now(), true, true);
        let request = e.tick(0, &s).expect("asked again");
        let text = e.text.clone();
        assert_eq!(
            e.deliver(&request, &text, &mut s),
            End::Shown { total: 1, drawn: 1 }
        );
        assert_eq!(e.marks(), TOTAL_MARKS);
    }

    /// SPEC-0295/AK-9: a caret that moved, or a text that changed, before the answer gets no step.
    #[test]
    fn a_step_is_given_up_when_the_caret_moved() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 1, 2);
        e.model.step_wanted(e.place(), e.now(), true, true);
        let request = e.tick(0, &s).expect("asked");
        e.caret_on("other", 0, 1);
        assert!(request.stop.load(Ordering::SeqCst), "withdrawn");
        let text = e.text.clone();
        assert_eq!(e.deliver(&request, &text, &mut s), End::Changed);
        assert!(e.marks().is_empty());

        // Moved before the question was sent: it is not sent.
        let (mut e, s) = (Editor::new(), Server::new());
        e.automatic = false;
        e.caret_on("total", 1, 2);
        e.model.step_wanted(e.place(), e.now(), true, false);
        e.caret_on("total", 0, 2);
        assert!(e.tick(0, &s).is_none());

        // The text changed before the question was sent.
        let (mut e, s) = (Editor::new(), Server::new());
        e.automatic = false;
        e.caret_on("total", 1, 2);
        e.model.step_wanted(e.place(), e.now(), true, false);
        e.type_text("x");
        assert!(e.tick(0, &s).is_none());
    }

    /// SPEC-0295/AK-3: 300 ms of rest on a name; not on white space, not with a selection.
    #[test]
    fn asked_after_the_caret_rested_on_a_name() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        assert!(e.tick(100, &s).is_none());
        assert!(e.tick(100, &s).is_none());
        // Moving starts the rest anew.
        e.caret_on("total", 0, 3);
        assert!(e.tick(200, &s).is_none());
        let request = e.tick(100, &s).expect("due after 300 ms of rest");
        assert!(!request.kept);
        assert_eq!((request.revision, request.caret), (1, 7));
        // One rest asks once, also after the answer.
        assert!(e.tick(100, &s).is_none());
        let text = e.text.clone();
        assert_eq!(
            e.deliver(&request, &text, &mut s),
            End::Shown { total: 3, drawn: 3 }
        );
        assert_eq!(s.asked, [(0, 7)]);
        assert!(e.tick(1000, &s).is_none());

        // Between `=` and `0` there is no name; at the end of a name there is.
        let (mut e, s) = (Editor::new(), Server::new());
        e.caret = 11;
        assert!(e.tick(0, &s).is_none());
        assert!(e.tick(1000, &s).is_none());
        e.caret_on("total", 0, 5);
        assert!(e.tick(300, &s).is_some());

        // A selection or a second caret: nothing by itself.
        let (mut e, s) = (Editor::new(), Server::new());
        e.single = false;
        e.caret_on("total", 0, 2);
        assert!(e.tick(1000, &s).is_none());
    }

    /// SPEC-0295/AK-3: no question while the server is starting or loading; the rest is not used
    /// up, so the question follows when the server is ready.
    #[test]
    fn a_server_that_is_not_ready_is_asked_when_it_is() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        s.ready = false;
        e.caret_on("total", 0, 2);
        assert!(e.tick(300, &s).is_none());
        assert!(e.tick(5000, &s).is_none());
        assert!(!e.model.in_flight());
        assert!(s.asked.is_empty());
        s.ready = true;
        let request = e.tick(100, &s).expect("asked once the server is ready");
        let text = e.text.clone();
        e.deliver(&request, &text, &mut s);
        assert_eq!(e.marks(), TOTAL_MARKS);
    }

    /// SPEC-0295/AK-3: without the capability the server is not asked and nothing is marked — no
    /// text matches instead.
    #[test]
    fn a_server_without_highlights_leaves_the_editor_unmarked() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        s.capabilities = json!({"referencesProvider": true});
        e.caret_on("total", 0, 2);
        let request = e.tick(300, &s).unwrap();
        let text = e.text.clone();
        assert_eq!(
            e.deliver(&request, &text, &mut s),
            End::NotOffered("scripted-server".into())
        );
        assert!(e.marks().is_empty());
        assert!(s.asked.is_empty());
        // Not asked again for the same rest.
        assert!(e.tick(1000, &s).is_none());
    }

    /// SPEC-0295/AK-3: write and read kinds, and a kind the server leaves out as text.
    #[test]
    fn read_and_write_kinds_reach_the_marks() {
        let (e, _) = marked();
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert!(!e.model.is_kept());
        assert_eq!(
            scheme_key(OccurrenceKind::Write),
            "WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES"
        );
        assert_eq!(
            scheme_key(OccurrenceKind::Read),
            "IDENTIFIER_UNDER_CARET_ATTRIBUTES"
        );
        assert_eq!(
            scheme_key(OccurrenceKind::Text),
            scheme_key(OccurrenceKind::Read)
        );

        let (mut e, mut s) = (Editor::new(), Server::new());
        s.total = json!([{"range": {
            "start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 9},
        }}]);
        e.caret_on("total", 0, 0);
        let request = e.tick(300, &s).unwrap();
        let text = e.text.clone();
        e.deliver(&request, &text, &mut s);
        assert_eq!(e.marks(), [(4, 9, OccurrenceKind::Text)]);
    }

    /// SPEC-0295/AK-3: an answer for an older text is dropped; so is an automatic answer when the
    /// caret has left the answered occurrences.
    #[test]
    fn stale_answers_are_dropped() {
        // The text changes while the question is in flight.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let request = e.tick(300, &s).unwrap();
        let asked_text = e.text.clone();
        e.type_text("x");
        assert!(request.stop.load(Ordering::SeqCst), "withdrawn");
        // Even an answer that was already on its way is not taken.
        request.stop.store(false, Ordering::SeqCst);
        assert_eq!(e.deliver(&request, &asked_text, &mut s), End::Changed);
        assert!(e.marks().is_empty());

        // The caret moves to another name while the question is in flight.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let request = e.tick(300, &s).unwrap();
        e.caret_on("other", 0, 1);
        assert!(request.stop.load(Ordering::SeqCst), "withdrawn");
        request.stop.store(false, Ordering::SeqCst);
        let text = e.text.clone();
        assert_eq!(e.deliver(&request, &text, &mut s), End::Changed);
        assert!(e.marks().is_empty());

        // An answer nobody asked for.
        assert_eq!(
            e.model.answer(99, e.revision, Some(Vec::new())),
            Taken::Stale
        );

        // The model itself checks the revision the answer is for.
        let (mut e, s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let request = e.tick(300, &s).unwrap();
        let mark = Mark {
            range: 4..9,
            kind: OccurrenceKind::Read,
        };
        assert_eq!(
            e.model.answer(request.id, e.revision + 1, Some(vec![mark])),
            Taken::Stale
        );
        assert!(e.marks().is_empty());
    }

    /// SPEC-0295/AK-3: at most one question in flight; the next is sent when the withdrawn one
    /// has ended.
    #[test]
    fn one_question_at_a_time() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let first = e.tick(300, &s).unwrap();
        assert!(e.model.in_flight());
        // The caret rests on another name; the first question has not ended.
        e.caret_on("total", 1, 1);
        assert!(first.stop.load(Ordering::SeqCst));
        assert!(e.tick(300, &s).is_none());
        assert!(e.tick(1000, &s).is_none());
        // It ends (withdrawn), and the next check asks for the new place.
        let text = e.text.clone();
        assert_eq!(e.deliver(&first, &text, &mut s), End::Changed);
        assert!(!e.model.in_flight());
        let second = e.tick(100, &s).expect("the next question");
        assert_ne!(second.id, first.id);
        assert_eq!(second.caret, 31);
        e.deliver(&second, &text, &mut s);
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert_eq!(s.asked, [(2, 1)]);
    }

    /// SPEC-0295/AK-3: the marks stay while the caret moves between the occurrences, go when it
    /// leaves the symbol, with a selection and with a change of the text.
    #[test]
    fn marks_go_when_the_caret_leaves_or_the_text_changes() {
        let (mut e, s) = marked();
        // To another occurrence, and to its end: the marks stay, nothing is asked.
        e.caret_on("total", 2, 0);
        assert_eq!(e.marks(), TOTAL_MARKS);
        e.caret_on("total", 2, 5);
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert!(e.tick(1000, &s).is_none());
        assert_eq!(s.asked.len(), 1);
        // Away from the symbol.
        e.caret_on("other", 0, 1);
        assert!(e.marks().is_empty());

        let (mut e, _) = marked();
        e.single = false;
        e.model.caret_at(e.place(), e.now());
        assert!(e.marks().is_empty());

        let (mut e, s) = marked();
        e.type_text("s");
        assert!(e.marks().is_empty());
        // The new text is asked about after its own rest.
        assert!(e.tick(200, &s).is_none());
        assert!(e.tick(100, &s).is_some());
    }

    /// SPEC-0295/AK-3: switched off, nothing is asked, the automatic marks go and a question in
    /// flight is withdrawn; kept marks stay and the action still works.
    #[test]
    fn the_setting_switches_the_automatic_highlight_off() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.automatic = false;
        e.caret_on("total", 0, 2);
        assert!(e.tick(1000, &s).is_none());
        assert!(s.asked.is_empty());
        assert_eq!(
            e.highlight_usages(&mut s),
            End::Shown { total: 3, drawn: 3 }
        );
        assert!(e.tick(100, &s).is_none());
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert!(e.model.is_kept());

        let (mut e, s) = marked();
        e.automatic = false;
        assert!(e.tick(100, &s).is_none());
        assert!(e.marks().is_empty());

        let (mut e, s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let request = e.tick(300, &s).unwrap();
        e.automatic = false;
        assert!(e.tick(100, &s).is_none());
        assert!(request.stop.load(Ordering::SeqCst));
        // Switched on again, the resting caret is asked about again.
        let text = e.text.clone();
        let mut s = s;
        e.deliver(&request, &text, &mut s);
        e.automatic = true;
        assert!(e.tick(100, &s).is_some());
    }

    /// SPEC-0295/AK-3: more occurrences than the bound are cut, the count stays.
    #[test]
    fn at_most_a_thousand_occurrences_are_drawn() {
        let mut model = Occurrences::default();
        let now = Instant::now();
        let place = Place {
            revision: 1,
            caret: 1,
            single: true,
        };
        model.caret_at(place, now);
        let request = model.due(now + REST, true, true, || true).unwrap();
        let found: Vec<Mark> = (0..MAX_MARKS + 5)
            .map(|i| Mark {
                range: i * 4..i * 4 + 3,
                kind: OccurrenceKind::Read,
            })
            .collect();
        assert_eq!(
            model.answer(request.id, 1, Some(found)),
            Taken::Shown {
                total: MAX_MARKS + 5,
                drawn: MAX_MARKS
            }
        );
        assert_eq!(model.marks().len(), MAX_MARKS);
        assert_eq!(
            model.marks()[MAX_MARKS - 1].range.start,
            (MAX_MARKS - 1) * 4
        );
        // Occurrences outside the text are left out.
        let outside = Occurrence {
            range: ferris_idea_languages::insight::Range {
                start_line: 7,
                start_character: 0,
                end_line: 7,
                end_character: 3,
            },
            kind: OccurrenceKind::Read,
        };
        assert!(marks_of(&[outside], |l, c| (l == 0).then_some(c as usize)).is_empty());
    }

    /// SPEC-0295/AK-4: the action's marks stay when the caret moves, the automatic highlight is
    /// off meanwhile, a second use replaces them, and Escape removes them.
    #[test]
    fn kept_marks_stay_until_escape() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        // Used before the caret rested: asked at once.
        assert_eq!(
            e.highlight_usages(&mut s),
            End::Shown { total: 3, drawn: 3 }
        );
        assert!(e.model.is_kept());
        // The caret moves to another name and rests there.
        e.caret_on("other", 0, 1);
        assert_eq!(e.marks(), TOTAL_MARKS);
        assert!(
            e.tick(1000, &s).is_none(),
            "no automatic question meanwhile"
        );
        // A symbol without usages leaves the kept marks as they are.
        e.caret = 11;
        e.model.caret_at(e.place(), e.now());
        assert_eq!(e.highlight_usages(&mut s), End::NoUsages);
        assert_eq!(e.marks(), TOTAL_MARKS);
        // A second use on a symbol with usages replaces them.
        s.total = json!([highlight(0, 4, 9, 3)]);
        e.caret_on("total", 0, 1);
        assert_eq!(
            e.highlight_usages(&mut s),
            End::Shown { total: 1, drawn: 1 }
        );
        assert_eq!(e.marks(), [(4, 9, OccurrenceKind::Write)]);
        // Escape removes them once; the next Escape is the editor's own.
        assert!(e.model.escape());
        assert!(e.marks().is_empty());
        assert!(!e.model.is_kept());
        assert!(!e.model.escape());

        // The action while an automatic question is in flight: that one is withdrawn, the
        // action's is asked when it has ended.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        let automatic = e.tick(300, &s).unwrap();
        e.model.keep(e.place(), e.now());
        assert!(automatic.stop.load(Ordering::SeqCst));
        assert!(e.tick(0, &s).is_none(), "one question at a time");
        let text = e.text.clone();
        e.deliver(&automatic, &text, &mut s);
        let kept = e.tick(0, &s).expect("the action's question");
        assert!(kept.kept);
        e.deliver(&kept, &text, &mut s);
        assert!(e.model.is_kept());
        assert_eq!(e.marks(), TOTAL_MARKS);

        // A change of the text removes kept marks too.
        e.type_text("x");
        assert!(e.marks().is_empty());
        assert!(!e.model.is_kept());
    }

    /// SPEC-0295/AK-4: after Escape the caret that still rests on the symbol is not highlighted
    /// again; the next movement starts the automatic highlight anew.
    #[test]
    fn escape_does_not_bring_the_automatic_highlight_back() {
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        e.highlight_usages(&mut s);
        assert!(e.tick(1000, &s).is_none());
        assert!(e.model.escape());
        assert!(e.tick(1000, &s).is_none());
        assert!(e.marks().is_empty());
        e.caret_on("total", 2, 1);
        let request = e.tick(300, &s).expect("asked again after a movement");
        assert!(!request.kept);

        // Escape before the action's answer: the answer is not wanted any more.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        e.model.keep(e.place(), e.now());
        let request = e.tick(0, &s).unwrap();
        assert!(e.model.escape());
        assert!(request.stop.load(Ordering::SeqCst));
        request.stop.store(false, Ordering::SeqCst);
        let text = e.text.clone();
        assert_eq!(e.deliver(&request, &text, &mut s), End::Changed);
        assert!(e.marks().is_empty());
        // Without anything kept Escape is not taken.
        let (mut e, _) = marked();
        assert!(!e.model.escape());
        assert_eq!(e.marks(), TOTAL_MARKS);
    }

    /// SPEC-0295/AK-4: the status bar's words.
    #[test]
    fn the_status_says_how_the_action_ended() {
        let shown = |total, drawn| status("total", &End::Shown { total, drawn });
        assert_eq!(
            shown(1, 1),
            "1 usage of total highlighted (Escape removes the highlighting)"
        );
        assert_eq!(
            shown(3, 3),
            "3 usages of total highlighted (Escape removes the highlighting)"
        );
        assert_eq!(
            shown(1234, 1000),
            "1234 usages of total, the first 1000 highlighted (Escape removes the highlighting)"
        );
        assert_eq!(
            status("total", &End::NoUsages),
            "No usages of total found in this file"
        );
        assert_eq!(
            status("total", &End::NotOffered("some-server".into())),
            "No usages found: some-server does not offer usage highlights"
        );
        assert_eq!(
            status("total", &End::NotReady("jdtls".into())),
            "No usages found yet: jdtls is still loading the project"
        );
        assert_eq!(
            status("", &End::NoName),
            "No usages found: the caret is not on a name"
        );
        assert_eq!(
            status("total", &End::TooLarge),
            "No usages found: the file is larger than 8 MiB"
        );
        assert_eq!(
            status("total", &End::Changed),
            "No usages found: the text changed meanwhile"
        );
        // How the question's result and the editor's use of it become an end.
        let found = Ok(Highlights::Found(Vec::new()));
        assert_eq!(End::of(&found, Taken::Nothing), End::NoUsages);
        assert_eq!(End::of(&found, Taken::Stale), End::Changed);
        assert_eq!(
            End::of(&found, Taken::Shown { total: 2, drawn: 2 }),
            End::Shown { total: 2, drawn: 2 }
        );
        assert_eq!(
            End::of(&Ok(Highlights::NotReady("gopls".into())), Taken::Nothing),
            End::NotReady("gopls".into())
        );
        assert_eq!(
            End::of(&Ok(Highlights::TooLarge), Taken::Nothing),
            End::TooLarge
        );
        let missing = Err(LspError::NoServer("notes.txt".into()));
        let End::Failed(reason) = End::of(&missing, Taken::Nothing) else {
            panic!("a missing server is a failure");
        };
        assert_eq!(
            status("x", &End::Failed(reason.clone())),
            format!("No usages found: {reason}")
        );

        // With the scripted server: not ready, and without the capability.
        let (mut e, mut s) = (Editor::new(), Server::new());
        e.caret_on("total", 0, 2);
        s.ready = false;
        assert_eq!(
            status("total", &e.highlight_usages(&mut s)),
            "No usages found yet: scripted-server is still loading the project"
        );
        s.ready = true;
        s.capabilities = json!({});
        assert_eq!(
            status("total", &e.highlight_usages(&mut s)),
            "No usages found: scripted-server does not offer usage highlights"
        );
        assert!(e.marks().is_empty());
    }

    /// SPEC-0295/AK-5: one stripe mark per line, in the write colour when the line has a write.
    #[test]
    fn a_line_with_a_write_takes_the_write_colour() {
        let (e, _) = marked();
        let line_of = |offset: usize| e.position(offset).0 as usize;
        assert_eq!(
            stripe_lines(e.model.marks(), line_of),
            [
                StripeLine {
                    line: 0,
                    offset: 4,
                    kind: OccurrenceKind::Write
                },
                StripeLine {
                    line: 2,
                    offset: 30,
                    kind: OccurrenceKind::Write
                },
            ]
        );
        let reads = [
            Mark {
                range: 38..43,
                kind: OccurrenceKind::Read,
            },
            Mark {
                range: 46..51,
                kind: OccurrenceKind::Write,
            },
        ];
        let lines = stripe_lines(&reads, line_of);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            (lines[0].offset, lines[0].kind),
            (38, OccurrenceKind::Write)
        );
        assert!(stripe_lines(&[], line_of).is_empty());
    }
}
