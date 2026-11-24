//! The side pane of the Containers window (`SPEC-0119` v5 behaviours 10 and 11, gaps D2 and D5 of
//! OBS-0116): a container's logs that follow its output, stop following when scrolled up and keep
//! earlier runs folded apart, or its details with secret-looking values masked; both searched with
//! case, whole word, regular expression, a match count and a filter for matching lines.

use std::ops::Range;

use ferris_idea_containers::details::ContainerDetails;
use ferris_idea_containers::logs::{LogBook, LogRow, LogSearch, hits};
use gpui::UniformListScrollHandle;

/// Height of a pane row in pixels.
pub(crate) const ROW_H: f32 = 18.;

/// Rows a page key moves.
const PAGE: usize = 20;

/// What a pane row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneRowKind {
    /// Text.
    Line,
    /// A section heading of the details.
    Heading,
    /// The heading of a run of a log; a click folds or unfolds earlier runs.
    Run,
    /// A row with an address to open (a published port).
    Link(String),
}

/// One row of the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    /// Text.
    pub text: String,
    /// Byte ranges of the search's matches.
    pub hits: Vec<Range<usize>>,
    /// Kind.
    pub kind: PaneRowKind,
}

/// What the pane shows.
pub enum Body {
    /// A log: of a Docker container (`target`, which can follow) or a static text.
    Logs {
        /// The lines in runs.
        book: LogBook,
        /// Container whose output this is, for following.
        target: Option<String>,
        /// Whether the output is being followed now.
        live: bool,
        /// Whether earlier runs are unfolded.
        show_earlier: bool,
    },
    /// A container's details.
    Details {
        /// The details, masked.
        details: Box<ContainerDetails>,
        /// Whether the whole inspect JSON is shown instead of the sections.
        json: bool,
    },
    /// An image's details (`SPEC-0119` v6 behaviour 13).
    Image(Box<ferris_idea_containers::images::ImageDetails>),
    /// Why the details could not be read.
    Failed(String),
}

/// What a key did in the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneKey {
    /// Handled.
    Handled,
    /// Not a key of the pane.
    Unhandled,
    /// The keyboard goes back to the list.
    Leave,
    /// Open this address.
    Open(String),
}

/// The pane.
pub struct Pane {
    /// Title.
    pub title: String,
    /// Content.
    pub body: Body,
    /// Whether new output scrolls to the end.
    pub follow: bool,
    /// The search.
    pub search: LogSearch,
    /// Whether typing goes into the search.
    pub searching: bool,
    /// Only rows with a match.
    pub only_matching: bool,
    /// The match the search is at (index among the rows with matches).
    pub current: usize,
    /// The first row shown, as the keys moved it.
    top: usize,
    /// Scroll position of the rows.
    pub scroll: UniformListScrollHandle,
}

impl Pane {
    fn with(title: String, body: Body, follow: bool) -> Self {
        Self {
            title,
            body,
            follow,
            search: LogSearch::default(),
            searching: false,
            only_matching: false,
            current: 0,
            top: 0,
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// A log; `target` is the container a live log follows.
    pub fn logs(title: String, target: Option<String>, book: LogBook, live: bool) -> Self {
        Self::with(
            title,
            Body::Logs {
                book,
                target,
                live,
                show_earlier: false,
            },
            true,
        )
    }

    /// A container's details, or why they could not be read.
    pub fn details(title: String, details: Result<ContainerDetails, String>) -> Self {
        let body = match details {
            Ok(d) => Body::Details {
                details: Box::new(d),
                json: false,
            },
            Err(e) => Body::Failed(e),
        };
        Self::with(title, body, false)
    }

    /// An image's details, or why they could not be read.
    pub fn image(
        title: String,
        details: Result<ferris_idea_containers::images::ImageDetails, String>,
    ) -> Self {
        let body = match details {
            Ok(d) => Body::Image(Box::new(d)),
            Err(e) => Body::Failed(e),
        };
        Self::with(title, body, false)
    }

    /// The image the details are of (its first tag, else its short id).
    pub fn image_of(&self) -> Option<&str> {
        match &self.body {
            Body::Image(_) => self.title.strip_prefix("Image: "),
            _ => None,
        }
    }

    /// The container a live log follows.
    pub fn log_target(&self) -> Option<&str> {
        match &self.body {
            Body::Logs { target, .. } => target.as_deref(),
            _ => None,
        }
    }

    /// The container the details are of.
    pub fn details_of(&self) -> Option<&str> {
        match &self.body {
            Body::Details { details, .. } => Some(&details.name),
            _ => None,
        }
    }

    /// The rows to show, and why the search expression is not valid.
    pub fn rows(&self) -> (Vec<PaneRow>, Option<String>) {
        let (re, error) = match self.search.compile() {
            Ok(re) => (re, None),
            Err(e) => (None, Some(e)),
        };
        let mark = |text: String, kind: PaneRowKind| -> Option<PaneRow> {
            let found = match (&re, &kind) {
                (Some(re), PaneRowKind::Line | PaneRowKind::Link(_)) => hits(re, &text),
                _ => Vec::new(),
            };
            let filtered = self.only_matching && re.is_some();
            if filtered && (found.is_empty() || kind == PaneRowKind::Heading) {
                return None;
            }
            Some(PaneRow {
                text,
                hits: found,
                kind,
            })
        };
        let rows = match &self.body {
            Body::Failed(e) => vec![PaneRow {
                text: e.clone(),
                hits: Vec::new(),
                kind: PaneRowKind::Line,
            }],
            Body::Logs {
                book, show_earlier, ..
            } => book
                .rows(*show_earlier, re.as_ref(), self.only_matching)
                .into_iter()
                .map(|r| match &r {
                    LogRow::Run { .. } => PaneRow {
                        text: r.text(),
                        hits: Vec::new(),
                        kind: PaneRowKind::Run,
                    },
                    LogRow::Line { text, hits, .. } => PaneRow {
                        text: text.clone(),
                        hits: hits.clone(),
                        kind: PaneRowKind::Line,
                    },
                })
                .collect(),
            Body::Details { details, json } => detail_rows(details, *json)
                .into_iter()
                .filter_map(|(t, k)| mark(t, k))
                .collect(),
            Body::Image(d) => image_rows(d)
                .into_iter()
                .filter_map(|(t, k)| mark(t, k))
                .collect(),
        };
        (rows, error)
    }

    /// The text of the rows (for tests).
    pub fn texts(&self) -> Vec<String> {
        self.rows().0.into_iter().map(|r| r.text).collect()
    }

    /// The line under the title: following, the search and its count, the toggles.
    pub fn status(&self) -> String {
        let (rows, error) = self.rows();
        let mut parts: Vec<String> = Vec::new();
        if let Body::Logs {
            live,
            target: Some(_),
            ..
        } = &self.body
        {
            parts.push(
                match (live, self.follow) {
                    (true, true) => "Following",
                    (true, false) => "Paused (End follows)",
                    (false, _) => "Not running",
                }
                .into(),
            );
        }
        if self.searching || !self.search.pattern.is_empty() {
            let count: usize = rows.iter().map(|r| r.hits.len()).sum();
            parts.push(match error {
                Some(e) => e,
                None if self.search.pattern.is_empty() => "Search: ".into(),
                None => format!(
                    "Search: {} — {count} match{}",
                    self.search.pattern,
                    if count == 1 { "" } else { "es" }
                ),
            });
            let flags: Vec<&str> = [
                (self.search.case_sensitive, "Aa"),
                (self.search.whole_word, "W"),
                (self.search.regex, ".*"),
                (self.only_matching, "matching lines only"),
            ]
            .into_iter()
            .filter_map(|(on, name)| on.then_some(name))
            .collect();
            if !flags.is_empty() {
                parts.push(flags.join(" "));
            }
        }
        parts.join(" · ")
    }

    /// Indices of the rows with a match.
    fn match_rows(rows: &[PaneRow]) -> Vec<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, r)| !r.hits.is_empty())
            .map(|(i, _)| i)
            .collect()
    }

    /// The row the current match is in.
    pub fn current_row(&self) -> Option<usize> {
        let (rows, _) = self.rows();
        Self::match_rows(&rows).get(self.current).copied()
    }

    /// The first row shown.
    pub fn top_line(&self) -> usize {
        let state = self.scroll.0.borrow();
        if let Some(asked) = &state.deferred_scroll_to_item {
            return asked.item_index;
        }
        let down = -f32::from(state.base_handle.offset().y);
        (down / ROW_H).round().max(0.) as usize
    }

    fn scroll_to(&mut self, row: usize, rows: usize) {
        self.top = row.min(rows.saturating_sub(1));
        self.scroll
            .scroll_to_item(self.top, gpui::ScrollStrategy::Top);
    }

    /// After new rows: to the end when following.
    pub fn after_change(&mut self) {
        if self.follow {
            let n = self.rows().0.len();
            if n > 0 {
                self.scroll_to(n - 1, n);
            }
        }
    }

    /// The user scrolled up with the mouse or the trackpad: stop following.
    pub fn scrolled_up(&mut self) {
        self.follow = false;
    }

    fn step_match(&mut self, back: bool) {
        let (rows, _) = self.rows();
        let found = Self::match_rows(&rows);
        if found.is_empty() {
            return;
        }
        self.current = if back {
            (self.current + found.len() - 1) % found.len()
        } else if self.current + 1 >= found.len() {
            0
        } else {
            self.current + 1
        };
        self.follow = false;
        let row = found[self.current];
        self.top = row;
        self.scroll
            .scroll_to_item(row, gpui::ScrollStrategy::Center);
    }

    fn toggle(&mut self, key: &str) -> bool {
        match key {
            "c" => self.search.case_sensitive = !self.search.case_sensitive,
            "w" => self.search.whole_word = !self.search.whole_word,
            "x" => self.search.regex = !self.search.regex,
            "l" => self.only_matching = !self.only_matching,
            _ => return false,
        }
        self.current = 0;
        true
    }

    /// Text typed into the search (committed by the text input).
    pub fn type_text(&mut self, text: &str) {
        if self.searching {
            self.search.pattern.push_str(text);
            self.current = 0;
        }
    }

    /// Handles a key while the pane has the keyboard; typed text comes through
    /// [`Self::type_text`].
    pub fn key(&mut self, key: &str, shift: bool, alt: bool, platform: bool) -> PaneKey {
        if alt && self.toggle(key) {
            return PaneKey::Handled;
        }
        let rows = self.rows().0.len();
        if self.searching {
            match key {
                "escape" => {
                    self.searching = false;
                    self.search.pattern.clear();
                    self.only_matching = false;
                    self.current = 0;
                    self.after_change();
                }
                "tab" => self.searching = false,
                "enter" => self.step_match(shift),
                "backspace" => {
                    self.search.pattern.pop();
                    self.current = 0;
                }
                _ => return PaneKey::Unhandled,
            }
            return PaneKey::Handled;
        }
        if platform {
            return if key == "f" {
                self.searching = true;
                PaneKey::Handled
            } else {
                PaneKey::Unhandled
            };
        }
        let top = self.top_line();
        match key {
            "escape" | "tab" => return PaneKey::Leave,
            "up" | "pageup" => {
                self.follow = false;
                let by = if key == "up" { 1 } else { PAGE };
                self.scroll_to(top.saturating_sub(by), rows);
            }
            "down" | "pagedown" => {
                let by = if key == "down" { 1 } else { PAGE };
                self.scroll_to(top + by, rows);
            }
            "home" => {
                self.follow = false;
                self.scroll_to(0, rows);
            }
            "end" => {
                self.follow = matches!(self.body, Body::Logs { .. });
                self.scroll_to(rows.saturating_sub(1), rows);
            }
            "/" => self.searching = true,
            "enter" | "n" => self.step_match(shift),
            "p" => match &mut self.body {
                Body::Logs { show_earlier, .. } => {
                    *show_earlier = !*show_earlier;
                    self.after_change();
                }
                _ => return PaneKey::Unhandled,
            },
            "j" => match &mut self.body {
                Body::Details { json, .. } => {
                    *json = !*json;
                    self.current = 0;
                    self.scroll_to(0, rows);
                }
                _ => return PaneKey::Unhandled,
            },
            "o" => {
                let (rows, _) = self.rows();
                return match rows.into_iter().find_map(|r| match r.kind {
                    PaneRowKind::Link(u) => Some(u),
                    _ => None,
                }) {
                    Some(u) => PaneKey::Open(u),
                    None => PaneKey::Unhandled,
                };
            }
            _ => return PaneKey::Unhandled,
        }
        PaneKey::Handled
    }

    /// Adds followed output.
    pub fn append(&mut self, text: &str) {
        if let Body::Logs { book, .. } = &mut self.body {
            book.push(text);
        }
        self.after_change();
    }

    /// The followed container started again at `time`.
    pub fn new_run(&mut self, time: i128) {
        if let Body::Logs { book, live, .. } = &mut self.body {
            book.new_run(time);
            *live = true;
        }
    }

    /// Whether the followed output is live.
    pub fn set_live(&mut self, on: bool) {
        if let Body::Logs { live, .. } = &mut self.body {
            *live = on;
        }
    }

    /// Time stamp of the newest line (to follow again after a restart).
    pub fn last_time(&self) -> Option<i128> {
        match &self.body {
            Body::Logs { book, .. } => book.last_time(),
            _ => None,
        }
    }

    /// Folds or unfolds the earlier runs (a click on a run heading).
    pub fn toggle_earlier(&mut self) {
        if let Body::Logs { show_earlier, .. } = &mut self.body {
            *show_earlier = !*show_earlier;
        }
    }

    /// The whole text of a log.
    pub fn log_text(&self) -> Option<String> {
        match &self.body {
            Body::Logs { book, .. } => Some(book.text()),
            _ => None,
        }
    }
}

/// The rows of an image's details.
fn image_rows(d: &ferris_idea_containers::images::ImageDetails) -> Vec<(String, PaneRowKind)> {
    use PaneRowKind::{Heading, Line};
    let mb = d.size as f64 / f64::from(1u32 << 20);
    let mut out = vec![
        (
            d.tags.first().cloned().unwrap_or_else(|| "<none>".into()),
            Heading,
        ),
        (format!("  ID {}", d.id), Line),
        (format!("  Size {mb:.1} MB"), Line),
        (format!("  Platform {}", d.platform), Line),
        (
            format!(
                "  Created {}",
                d.created.get(..19).unwrap_or(&d.created).replace('T', " ")
            ),
            Line,
        ),
        (format!("Tags ({})", d.tags.len()), Heading),
    ];
    out.extend(d.tags.iter().map(|t| (format!("  {t}"), Line)));
    out.push((format!("Digests ({})", d.digests.len()), Heading));
    out.extend(d.digests.iter().map(|t| (format!("  {t}"), Line)));
    out.push((format!("Used by ({})", d.containers.len()), Heading));
    out.extend(
        d.containers
            .iter()
            .map(|(n, s)| (format!("  {n} ({s})"), Line)),
    );
    out
}

/// The rows of a container's details: sections, or the whole inspect JSON.
fn detail_rows(d: &ContainerDetails, json: bool) -> Vec<(String, PaneRowKind)> {
    use PaneRowKind::{Heading, Line, Link};
    if json {
        return serde_json::to_string_pretty(&d.inspect)
            .unwrap_or_default()
            .lines()
            .map(|l| (l.to_owned(), Line))
            .collect();
    }
    let mut out = vec![(
        format!(
            "{} · {} · {}{}",
            d.name,
            d.image,
            d.state,
            if d.started_at.starts_with("0001") || d.started_at.is_empty() {
                String::new()
            } else {
                format!(
                    " · started {}",
                    d.started_at
                        .get(..19)
                        .unwrap_or(&d.started_at)
                        .replace('T', " ")
                )
            }
        ),
        Heading,
    )];
    if !d.command.is_empty() {
        out.push((format!("  {}", d.command.join(" ")), Line));
    }
    out.push((format!("Environment ({})", d.env.len()), Heading));
    out.extend(d.env.iter().map(|e| {
        (
            format!(
                "  {}={}{}",
                e.name,
                e.value,
                if e.masked { "  (masked)" } else { "" }
            ),
            Line,
        )
    }));
    out.push((format!("Ports ({})", d.ports.len()), Heading));
    out.extend(d.ports.iter().map(|p| {
        let text = match (&p.published, &p.url) {
            (Some(at), Some(url)) => format!("  {} → {at}  {url}", p.container),
            (Some(at), None) => format!("  {} → {at}", p.container),
            (None, _) => format!("  {} (not published)", p.container),
        };
        (text, p.url.clone().map_or(Line, Link))
    }));
    out.push((format!("Mounts ({})", d.mounts.len()), Heading));
    out.extend(d.mounts.iter().map(|m| {
        (
            format!(
                "  {} {} → {} ({}{})",
                m.kind,
                m.source,
                m.destination,
                if m.writable { "rw" } else { "ro" },
                if m.mode.is_empty() || m.mode == "ro" || m.mode == "rw" {
                    String::new()
                } else {
                    format!(", {}", m.mode)
                }
            ),
            Line,
        )
    }));
    out.push((format!("Labels ({})", d.labels.len()), Heading));
    out.extend(d.labels.iter().map(|(k, v)| (format!("  {k}={v}"), Line)));
    out.push((format!("Networks ({})", d.networks.len()), Heading));
    out.extend(
        d.networks
            .iter()
            .map(|(n, ip)| (format!("  {n} {ip}").trim_end().to_owned(), Line)),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> LogBook {
        let mut b = LogBook::new(vec![]);
        b.push("GET /a 200\nget /b 500\nerror at 3\n");
        b
    }

    #[test]
    fn keys_scroll_search_and_follow() {
        let mut p = Pane::logs("Logs: web".into(), Some("web".into()), book(), true);
        assert_eq!(p.status(), "Following");
        assert_eq!(p.key("up", false, false, false), PaneKey::Handled);
        assert!(!p.follow, "scrolling up stops following");
        assert_eq!(p.status(), "Paused (End follows)");
        p.key("end", false, false, false);
        assert!(p.follow, "End follows again");
        p.key("/", false, false, false);
        for c in ["g", "e", "t"] {
            p.type_text(c);
        }
        assert_eq!(p.status(), "Following · Search: get — 2 matches");
        p.key("c", false, true, false);
        assert_eq!(p.status(), "Following · Search: get — 1 match · Aa");
        p.key("c", false, true, false);
        p.key("l", false, true, false);
        assert_eq!(p.texts(), ["GET /a 200", "get /b 500"]);
        p.key("enter", false, false, false);
        assert_eq!(p.current_row(), Some(1));
        assert!(!p.follow, "jumping to a match stops following");
        p.key("enter", true, false, false);
        assert_eq!(p.current_row(), Some(0));
        p.key("backspace", false, false, false);
        p.key("x", false, true, false);
        p.type_text("(");
        assert!(
            p.status().contains("not a valid regular expression"),
            "{}",
            p.status()
        );
        p.key("escape", false, false, false);
        assert!(!p.searching && p.search.pattern.is_empty() && !p.only_matching);
        assert_eq!(p.texts().len(), 3);
        assert_eq!(p.key("tab", false, false, false), PaneKey::Leave);
        p.set_live(false);
        assert_eq!(p.status(), "Not running");
    }

    #[test]
    fn details_rows_links_and_json() {
        let raw = serde_json::json!({
            "Id": "abc", "Name": "/web", "State": {"Status": "running", "Running": true, "StartedAt": "2026-11-23T10:00:01.5Z"},
            "Config": {"Image": "nginx", "Env": ["A=1", "API_TOKEN=t"], "Labels": {"tier": "front"}},
            "NetworkSettings": {"Ports": {"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}]}, "Networks": {"bridge": {"IPAddress": "172.17.0.2"}}},
            "Mounts": [{"Type": "volume", "Name": "data", "Destination": "/data", "Mode": "z", "RW": true}]
        });
        let mut p = Pane::details(
            "Details: web".into(),
            Ok(ferris_idea_containers::details::details(&raw)),
        );
        let t = p.texts();
        assert_eq!(t[0], "web · nginx · running · started 2026-11-23 10:00:01");
        assert!(
            t.contains(&"  API_TOKEN=••••••  (masked)".to_owned()),
            "{t:#?}"
        );
        assert!(t.contains(&"  80/tcp → 0.0.0.0:8080  http://localhost:8080".to_owned()));
        assert!(t.contains(&"  volume data → /data (rw, z)".to_owned()));
        assert!(t.contains(&"  bridge 172.17.0.2".to_owned()));
        assert_eq!(
            p.key("o", false, false, false),
            PaneKey::Open("http://localhost:8080".into())
        );
        p.key("j", false, false, false);
        let json = p.texts().join("\n");
        assert!(json.contains("\"Image\": \"nginx\""));
        assert!(!json.contains("\"API_TOKEN=t\""));
        p.key("/", false, false, false);
        for c in "front".chars() {
            p.type_text(&c.to_string());
        }
        assert!(p.status().ends_with("1 match"), "{}", p.status());
        let failed = Pane::details(
            "Details: x".into(),
            Err("Docker answered 404: No such container: x".into()),
        );
        assert_eq!(
            failed.texts(),
            ["Docker answered 404: No such container: x"]
        );
    }
}
