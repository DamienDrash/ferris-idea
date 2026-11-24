//! Container logs that follow, are searched and keep earlier runs apart (`SPEC-0119` v5
//! behaviour 10, gap D2 of OBS-0116): the followed log stream of the Engine API, its frames taken
//! apart as they arrive, the lines sorted into runs by the container's start times, and a search
//! with case, whole word, regular expression and a filter for matching lines.

use std::collections::VecDeque;
use std::ops::Range;

use serde_json::Value;

use crate::stream::{StopFlag, read_chunks};
use crate::{DockerError, DockerHost};

/// Most lines a log keeps; the oldest go first.
pub const KEEP_LINES: usize = 20_000;

/// Which part of a container's output to read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    /// Only the last lines (`None`: all).
    pub tail: Option<usize>,
    /// Only output since this Unix time in seconds.
    pub since: Option<i64>,
    /// Each line starts with its RFC 3339 time stamp and a space.
    pub timestamps: bool,
}

impl LogQuery {
    fn path(&self, id: &str, follow: bool) -> String {
        let mut p = format!("/containers/{id}/logs?stdout=1&stderr=1");
        if follow {
            p.push_str("&follow=1");
        }
        match self.tail {
            Some(n) => p.push_str(&format!("&tail={n}")),
            None => p.push_str("&tail=all"),
        }
        if let Some(s) = self.since {
            p.push_str(&format!("&since={s}"));
        }
        if self.timestamps {
            p.push_str("&timestamps=1");
        }
        p
    }
}

/// Takes the daemon's log frames (8-byte header with stream and length) apart while they arrive;
/// output without frames (a container with a TTY) passes through.
#[derive(Debug, Default)]
pub struct FrameDemux {
    pending: Vec<u8>,
    framed: Option<bool>,
}

impl FrameDemux {
    /// The text that `bytes` complete.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        if self.framed.is_none() {
            if self.pending.len() < 8 {
                return String::new();
            }
            let p = &self.pending;
            self.framed = Some(p[0] <= 2 && p[1..4] == [0, 0, 0]);
        }
        if self.framed == Some(false) {
            // Keep an incomplete UTF-8 sequence at the end for the next piece.
            let cut = utf8_complete(&self.pending);
            let out: Vec<u8> = self.pending.drain(..cut).collect();
            return String::from_utf8_lossy(&out).into_owned();
        }
        let mut out = Vec::new();
        while self.pending.len() >= 8 {
            let p = &self.pending;
            let len = u32::from_be_bytes([p[4], p[5], p[6], p[7]]) as usize;
            if p.len() < 8 + len {
                break;
            }
            out.extend_from_slice(&p[8..8 + len]);
            self.pending.drain(..8 + len);
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

/// Length of the prefix of `b` that ends on a complete UTF-8 sequence.
fn utf8_complete(b: &[u8]) -> usize {
    let n = b.len();
    for back in 1..=3.min(n) {
        let c = b[n - back];
        if c & 0xC0 == 0x80 {
            continue;
        }
        let need = match c {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        };
        return if need > back { n - back } else { n };
    }
    n
}

impl DockerHost {
    /// Output of container `id` as `query` asks, stdout and stderr in order.
    pub fn logs_with(&self, id: &str, query: &LogQuery) -> Result<String, DockerError> {
        let id = crate::docker::checked_id(id)?;
        let body = self.call("GET", &query.path(id, false))?;
        Ok(crate::demux_logs(&body))
    }

    /// Follows the output of container `id`: `on` gets the text as it arrives, until the
    /// container stops (the daemon ends the stream), the connection is lost or `stop` is set.
    pub fn follow_logs(
        &self,
        id: &str,
        query: &LogQuery,
        stop: &StopFlag,
        mut on: impl FnMut(&str),
    ) -> Result<(), DockerError> {
        let id = crate::docker::checked_id(id)?;
        let stream = self.open_stream("GET", &query.path(id, true))?;
        let mut demux = FrameDemux::default();
        read_chunks(stream, stop, |bytes| {
            let text = demux.push(bytes);
            if !text.is_empty() {
                on(&text);
            }
        })
        .map_err(|e| DockerError::Connect(self.to_string(), e.to_string()))
    }

    /// The times container `id` was started, oldest first, in nanoseconds since 1970: from the
    /// engine's event history where it still has them, and the last start from inspect.
    pub fn start_times(&self, id: &str) -> Result<Vec<i128>, DockerError> {
        let id = crate::docker::checked_id(id)?;
        let inspect = self.json(&format!("/containers/{id}/json"))?;
        let full = inspect
            .get("Id")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_owned();
        let mut out: Vec<i128> = Vec::new();
        let created = inspect
            .get("Created")
            .and_then(Value::as_str)
            .and_then(parse_time)
            .map_or(0, |n| (n / 1_000_000_000) as i64);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
            + 1;
        let filters = format!(
            "%7B%22container%22%3A%5B%22{full}%22%5D%2C%22type%22%3A%5B%22container%22%5D%2C%22event%22%3A%5B%22start%22%5D%7D"
        );
        if let Ok(body) = self.call(
            "GET",
            &format!("/events?since={created}&until={now}&filters={filters}"),
        ) {
            for line in String::from_utf8_lossy(&body).lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line)
                    && let Some(t) = v.get("timeNano").and_then(Value::as_i64)
                {
                    out.push(i128::from(t));
                }
            }
        }
        if let Some(t) = inspect
            .pointer("/State/StartedAt")
            .and_then(Value::as_str)
            .and_then(parse_time)
            .filter(|t| *t > 0)
        {
            // The event of the last start and inspect name the same moment within a second.
            if !out.iter().any(|e| (e - t).abs() < 1_000_000_000) {
                out.push(t);
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date of days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

/// An RFC 3339 time (`2026-11-23T10:00:01.123456789Z`, also with an offset) in nanoseconds since
/// 1970; `None` when it is not one. Docker's "never" (`0001-01-01T00:00:00Z`) is negative.
pub fn parse_time(s: &str) -> Option<i128> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |r: Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut at = 19;
    let mut nanos: i128 = 0;
    if b.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while at < b.len() && b[at].is_ascii_digit() {
            at += 1;
        }
        let digits = &s[start..at];
        let mut padded = digits.chars().take(9).collect::<String>();
        while padded.len() < 9 {
            padded.push('0');
        }
        nanos = padded.parse().ok()?;
    }
    let offset = match b.get(at)? {
        b'Z' | b'z' => 0,
        b'+' | b'-' => {
            let sign = if b[at] == b'-' { -1 } else { 1 };
            let oh = num(at + 1..at + 3)?;
            let om = num(at + 4..at + 6)?;
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset;
    Some(i128::from(secs) * 1_000_000_000 + nanos)
}

/// A time in nanoseconds since 1970 as `2026-11-23 10:00:01` (UTC).
pub fn show_time(nanos: i128) -> String {
    let secs = nanos.div_euclid(1_000_000_000) as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// A time in nanoseconds since 1970 as RFC 3339 with nanoseconds (`2026-11-23T10:00:01.000000000Z`),
/// as the engine writes it.
pub fn rfc3339(nanos: i128) -> String {
    let frac = nanos.rem_euclid(1_000_000_000);
    format!("{}.{frac:09}Z", show_time(nanos).replace(' ', "T"))
}

/// One line of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// When the container wrote it (nanoseconds since 1970), when known.
    pub time: Option<i128>,
    /// The run it belongs to (0 is the first known run).
    pub run: usize,
    /// The text without its time stamp.
    pub text: String,
}

/// The lines of one container's output sorted into runs.
#[derive(Debug, Clone, Default)]
pub struct LogBook {
    lines: VecDeque<LogLine>,
    partial: String,
    starts: Vec<i128>,
    last: Option<i128>,
    /// Lines dropped at the front to stay within [`KEEP_LINES`].
    pub dropped: usize,
}

impl LogBook {
    /// An empty log whose runs started at `starts` (nanoseconds, any order).
    pub fn new(mut starts: Vec<i128>) -> Self {
        starts.sort_unstable();
        starts.dedup();
        Self {
            starts,
            ..Self::default()
        }
    }

    /// The run a line written at `time` belongs to.
    fn run_at(&self, time: Option<i128>) -> usize {
        match time {
            Some(t) => self
                .starts
                .iter()
                .filter(|s| **s <= t)
                .count()
                .saturating_sub(1),
            None => self.current_run(),
        }
    }

    /// The run now going on.
    pub fn current_run(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    /// When run `run` started.
    pub fn run_start(&self, run: usize) -> Option<i128> {
        self.starts.get(run).copied()
    }

    /// The container was started again at `time`: later lines belong to a new run.
    pub fn new_run(&mut self, time: i128) {
        if self.starts.last().is_none_or(|l| time > *l) {
            self.starts.push(time);
        }
    }

    /// Adds text as it arrives; lines of a time stamp at or before the last one seen are left
    /// out (a stream started again after a restart repeats them).
    pub fn push(&mut self, text: &str) {
        self.partial.push_str(text);
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            let (time, body) = match line.split_once(' ') {
                Some((ts, rest)) if parse_time(ts).is_some() => (parse_time(ts), rest),
                _ => match parse_time(line) {
                    Some(t) => (Some(t), ""),
                    None => (None, line),
                },
            };
            if let (Some(t), Some(last)) = (time, self.last)
                && t <= last
            {
                continue;
            }
            if time.is_some() {
                self.last = time;
            }
            let run = self.run_at(time);
            self.lines.push_back(LogLine {
                time,
                run,
                text: body.to_owned(),
            });
            if self.lines.len() > KEEP_LINES {
                self.lines.pop_front();
                self.dropped += 1;
            }
        }
    }

    /// Time stamp of the newest line, for following again after a restart.
    pub fn last_time(&self) -> Option<i128> {
        self.last
    }

    /// All lines, oldest first.
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// Number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether there are no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The text of all lines, one per line.
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| format!("{}\n", l.text)).collect()
    }

    /// The rows to show: a heading per run when there is more than one, earlier runs folded
    /// unless `show_earlier`; with a search, each line's matches, and with `only_matching` only
    /// the lines that have one.
    pub fn rows(
        &self,
        show_earlier: bool,
        search: Option<&regex::Regex>,
        only_matching: bool,
    ) -> Vec<LogRow> {
        let current = self.current_run();
        let runs: std::collections::BTreeSet<usize> = self.lines.iter().map(|l| l.run).collect();
        let several = runs.len() > 1 || runs.iter().any(|r| *r != current);
        let mut out = Vec::new();
        let mut at_run: Option<usize> = None;
        for (i, l) in self.lines.iter().enumerate() {
            if several && at_run != Some(l.run) {
                at_run = Some(l.run);
                let count = self.lines.iter().filter(|x| x.run == l.run).count();
                out.push(LogRow::Run {
                    run: l.run,
                    started: self.run_start(l.run),
                    lines: count,
                    current: l.run == current,
                    open: l.run == current || show_earlier,
                });
            }
            if several && l.run != current && !show_earlier {
                continue;
            }
            let hits = search.map(|re| hits(re, &l.text)).unwrap_or_default();
            if only_matching && search.is_some() && hits.is_empty() {
                continue;
            }
            out.push(LogRow::Line {
                index: i,
                text: l.text.clone(),
                hits,
            });
        }
        out
    }
}

/// A row of the log view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogRow {
    /// The heading of a run.
    Run {
        /// Run number from 0.
        run: usize,
        /// When it started.
        started: Option<i128>,
        /// Its number of lines.
        lines: usize,
        /// Whether it is the run going on.
        current: bool,
        /// Whether its lines are shown.
        open: bool,
    },
    /// A line with the byte ranges of its matches.
    Line {
        /// Index into [`LogBook::lines`].
        index: usize,
        /// Text.
        text: String,
        /// Matches.
        hits: Vec<Range<usize>>,
    },
}

impl LogRow {
    /// The row as text: a run heading reads `── Run 2 (current) · started … · 12 lines ──`.
    pub fn text(&self) -> String {
        match self {
            LogRow::Line { text, .. } => text.clone(),
            LogRow::Run {
                run,
                started,
                lines,
                current,
                open,
            } => format!(
                "{} Run {}{}{} · {lines} line{} ──",
                if *open { "▾" } else { "▸" },
                run + 1,
                if *current { " (current)" } else { " (earlier)" },
                started
                    .map(|t| format!(" · started {}", show_time(t)))
                    .unwrap_or_default(),
                if *lines == 1 { "" } else { "s" }
            ),
        }
    }
}

/// How a log is searched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogSearch {
    /// What to look for.
    pub pattern: String,
    /// Upper and lower case differ.
    pub case_sensitive: bool,
    /// Only whole words.
    pub whole_word: bool,
    /// `pattern` is a regular expression.
    pub regex: bool,
}

impl LogSearch {
    /// The compiled search, `None` for an empty pattern, or why the expression is not valid. The
    /// expression engine runs in linear time, and its size is limited.
    pub fn compile(&self) -> Result<Option<regex::Regex>, String> {
        if self.pattern.is_empty() {
            return Ok(None);
        }
        let mut p = if self.regex {
            self.pattern.clone()
        } else {
            regex::escape(&self.pattern)
        };
        if self.whole_word {
            p = format!(r"\b(?:{p})\b");
        }
        regex::RegexBuilder::new(&p)
            .case_insensitive(!self.case_sensitive)
            .size_limit(1 << 20)
            .dfa_size_limit(1 << 20)
            .build()
            .map(Some)
            .map_err(|e| format!("not a valid regular expression: {e}"))
    }
}

/// Byte ranges of the non-empty matches of `re` in `line`.
pub fn hits(re: &regex::Regex, line: &str) -> Vec<Range<usize>> {
    re.find_iter(line)
        .filter(|m| !m.is_empty())
        .map(|m| m.range())
        .collect()
}

/// The lines of `text` that match `search`, and the number of matches.
pub fn matching_lines(text: &str, search: &LogSearch) -> Result<(String, usize), String> {
    let Some(re) = search.compile()? else {
        return Ok((text.to_owned(), 0));
    };
    let mut out = String::new();
    let mut count = 0;
    for line in text.lines() {
        let n = hits(&re, line).len();
        if n > 0 {
            count += n;
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok((out, count))
}

/// A `since` value as Unix seconds: digits are Unix seconds, `30s`, `10m`, `2h`, `1d` count back
/// from `now`, an RFC 3339 time is taken as is.
pub fn parse_since(s: &str, now: i64) -> Option<i64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    if let Some(t) = parse_time(s) {
        return Some((t / 1_000_000_000) as i64);
    }
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86_400,
        _ => return None,
    };
    Some(now - secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse_and_show() {
        let t = parse_time("2026-11-23T10:00:01.5Z").unwrap();
        assert_eq!(show_time(t), "2026-11-23 10:00:01");
        assert_eq!(rfc3339(t), "2026-11-23T10:00:01.500000000Z");
        assert_eq!(parse_time(&rfc3339(t)), Some(t));
        assert_eq!(
            parse_time("2026-11-23T11:00:01.500000000+01:00"),
            Some(t),
            "offsets count"
        );
        assert!(parse_time("2026-11-23T10:00:01Z").unwrap() < t);
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert!(parse_time("0001-01-01T00:00:00Z").unwrap() < 0);
        assert_eq!(parse_time("hello world, not a time"), None);
        assert_eq!(parse_since("1700000000", 0), Some(1_700_000_000));
        assert_eq!(parse_since("10m", 10_000), Some(9_400));
        assert_eq!(parse_since("2h", 10_000), Some(2_800));
        assert_eq!(parse_since("1970-01-01T00:01:00Z", 0), Some(60));
        assert_eq!(parse_since("soon", 0), None);
    }

    #[test]
    fn frames_arrive_in_pieces() {
        let mut d = FrameDemux::default();
        let mut all = vec![1u8, 0, 0, 0, 0, 0, 0, 6];
        all.extend_from_slice(b"hello\n");
        all.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 3]);
        all.extend_from_slice(b"ok\n");
        let mut out = String::new();
        for b in all.chunks(3) {
            out.push_str(&d.push(b));
        }
        assert_eq!(out, "hello\nok\n");
        let mut raw = FrameDemux::default();
        let text = "tty öut\n".as_bytes();
        let mut out = String::new();
        for b in text.chunks(5) {
            out.push_str(&raw.push(b));
        }
        out.push_str(&raw.push(b""));
        assert_eq!(out, "tty öut\n", "a split UTF-8 character stays whole");
    }

    #[test]
    fn runs_are_kept_apart_and_folded() {
        let s1 = parse_time("2026-11-23T10:00:00Z").unwrap();
        let s2 = parse_time("2026-11-23T10:05:00Z").unwrap();
        let mut b = LogBook::new(vec![s2, s1]);
        b.push("2026-11-23T10:00:01.000000001Z first a\n2026-11-23T10:00:02Z first b\n2026-11-23T10:05:01Z sec");
        b.push("ond a\n");
        assert_eq!(b.len(), 3);
        assert_eq!(b.text(), "first a\nfirst b\nsecond a\n");
        let folded: Vec<String> = b
            .rows(false, None, false)
            .iter()
            .map(LogRow::text)
            .collect();
        assert_eq!(
            folded,
            [
                "▸ Run 1 (earlier) · started 2026-11-23 10:00:00 · 2 lines ──",
                "▾ Run 2 (current) · started 2026-11-23 10:05:00 · 1 line ──",
                "second a",
            ]
        );
        let open: Vec<String> = b.rows(true, None, false).iter().map(LogRow::text).collect();
        assert_eq!(open.len(), 5);
        // Following again after a restart repeats lines: those up to the last time stamp are
        // left out; a start seen live begins a new run.
        let s3 = parse_time("2026-11-23T10:09:00Z").unwrap();
        b.new_run(s3);
        b.push("2026-11-23T10:05:01Z second a\n2026-11-23T10:09:01Z third\n");
        assert_eq!(b.text(), "first a\nfirst b\nsecond a\nthird\n");
        assert_eq!(b.lines().last().unwrap().run, 2);
        // One run only: no headings.
        let mut one = LogBook::new(vec![s1]);
        one.push("2026-11-23T10:00:01Z x\nplain\n");
        assert_eq!(
            one.rows(false, None, false)
                .iter()
                .map(LogRow::text)
                .collect::<Vec<_>>(),
            ["x", "plain"]
        );
    }

    #[test]
    fn search_case_words_expressions_and_the_filter() {
        let mut b = LogBook::new(vec![]);
        b.push("GET /a 200\nget /b 500\nwarning: target\nerror at 3\n");
        let s = |p: &str, case: bool, word: bool, re: bool| LogSearch {
            pattern: p.into(),
            case_sensitive: case,
            whole_word: word,
            regex: re,
        };
        let count = |q: LogSearch| {
            let re = q.compile().unwrap().unwrap();
            b.lines().map(|l| hits(&re, &l.text).len()).sum::<usize>()
        };
        assert_eq!(count(s("get", false, false, false)), 3, "GET, get, tarGET");
        assert_eq!(count(s("get", true, false, false)), 2);
        assert_eq!(count(s("get", false, true, false)), 2);
        assert_eq!(count(s(r"[45]00", false, false, true)), 1);
        assert_eq!(
            count(s("/a", false, false, false)),
            1,
            "plain text is not an expression"
        );
        assert!(s("(", false, false, true).compile().is_err());
        assert!(s("", false, false, false).compile().unwrap().is_none());
        let re = s("error|500", false, false, true)
            .compile()
            .unwrap()
            .unwrap();
        let only: Vec<String> = b
            .rows(false, Some(&re), true)
            .iter()
            .map(LogRow::text)
            .collect();
        assert_eq!(only, ["get /b 500", "error at 3"]);
        let LogRow::Line { hits, .. } = &b.rows(false, Some(&re), false)[1] else {
            panic!()
        };
        assert_eq!((hits.len(), hits[0].clone()), (1, 7..10));
        let (text, n) = matching_lines(&b.text(), &s("GET", false, false, false)).unwrap();
        assert_eq!(
            (text.as_str(), n),
            ("GET /a 200\nget /b 500\nwarning: target\n", 3)
        );
    }
}
