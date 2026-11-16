//! Rows, dates and the filter of the Git log tool window (`SPEC-0086`), free of the UI toolkit.

use idecraft_vcs::CommitInfo;

/// Seconds east of UTC of the local time zone, as `date +%z` reports it (0 when unknown).
pub fn local_offset() -> i64 {
    std::process::Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| parse_offset(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or(0)
}

/// The local time zone of dates in the Git log and the annotations (`SPEC-0086` v6 behaviour 11):
/// the offset follows the zone's daylight-saving rules for each point in time, so a commit made
/// in summer shows its summer time also in winter (OBS-0022 L1).
#[derive(Debug, Clone)]
pub enum LocalZone {
    /// A fixed offset in seconds east of UTC (tests, or no zone rules found).
    Fixed(i64),
    /// The system's time zone with its rules.
    System(jiff::tz::TimeZone),
}

impl LocalZone {
    /// The system's zone; the current offset when its rules cannot be read.
    pub fn system() -> Self {
        jiff::tz::TimeZone::try_system()
            .map(Self::System)
            .unwrap_or_else(|_| Self::Fixed(local_offset()))
    }

    /// Seconds east of UTC at `time` (seconds since the epoch).
    pub fn offset_at(&self, time: i64) -> i64 {
        match self {
            Self::Fixed(offset) => *offset,
            Self::System(zone) => jiff::Timestamp::from_second(time)
                .map(|t| i64::from(zone.to_offset(t).seconds()))
                .unwrap_or(0),
        }
    }
}

impl Default for LocalZone {
    fn default() -> Self {
        Self::Fixed(0)
    }
}

/// `+0130` → 5400 seconds.
fn parse_offset(s: &str) -> Option<i64> {
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Year, month, day of a day count since 1970-01-01 (proleptic Gregorian calendar).
pub(crate) fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Order of day, month and year in a short date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    /// `05.11.26`
    Dmy,
    /// `11/5/26`
    Mdy,
    /// `2026-11-05`
    Ymd,
}

/// The short date and time format of a region (`SPEC-0086` v3 behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateStyle {
    /// Order of the date's parts.
    pub order: DateOrder,
    /// Between the date's parts.
    pub separator: char,
    /// Day and month with a leading zero.
    pub pad: bool,
    /// Year with four digits (else two).
    pub full_year: bool,
    /// 12-hour clock with AM/PM.
    pub twelve_hour: bool,
    /// A comma between date and time.
    pub comma: bool,
}

impl DateStyle {
    /// `2026-11-05 14:05`, for regions not in the table.
    pub const ISO: Self = Self::new(DateOrder::Ymd, '-', true, true, false, false);

    const fn new(
        order: DateOrder,
        separator: char,
        pad: bool,
        full_year: bool,
        twelve_hour: bool,
        comma: bool,
    ) -> Self {
        Self {
            order,
            separator,
            pad,
            full_year,
            twelve_hour,
            comma,
        }
    }

    /// The style of `locale` (`de_DE.UTF-8`, `en-GB`, `en_US@rg=dezzzz`, `sv`); `force_24h`
    /// overrides the region's clock when the system forces one.
    pub fn for_locale(locale: &str, force_24h: Option<bool>) -> Self {
        let (language, region) = split_locale(locale);
        let region = region.or_else(|| default_region(&language));
        let mut style = region
            .as_deref()
            .and_then(region_style)
            .unwrap_or(Self::ISO);
        if let Some(on) = force_24h {
            style.twelve_hour = !on;
        }
        style
    }

    /// `14:05` or `2:05 PM` for the minute `minute` of the day.
    pub fn clock(&self, minute: i64) -> String {
        let (h, m) = (minute / 60, minute % 60);
        if self.twelve_hour {
            let half = if h < 12 { "AM" } else { "PM" };
            let h12 = if h % 12 == 0 { 12 } else { h % 12 };
            format!("{h12}:{m:02} {half}")
        } else {
            format!("{h:02}:{m:02}")
        }
    }

    /// The short date of a day count since 1970-01-01.
    pub fn date(&self, day: i64) -> String {
        let (y, m, d) = civil(day);
        let year = if self.full_year {
            format!("{y:04}")
        } else {
            format!("{:02}", y.rem_euclid(100))
        };
        let part = |v: u32| {
            if self.pad {
                format!("{v:02}")
            } else {
                v.to_string()
            }
        };
        let s = self.separator;
        match self.order {
            DateOrder::Dmy => format!("{}{s}{}{s}{year}", part(d), part(m)),
            DateOrder::Mdy => format!("{}{s}{}{s}{year}", part(m), part(d)),
            DateOrder::Ymd => format!("{year}{s}{}{s}{}", part(m), part(d)),
        }
    }
}

/// Language and region of a locale name; the region of a `@rg=xxzzzz` override wins.
fn split_locale(locale: &str) -> (String, Option<String>) {
    let (base, modifiers) = locale.split_once('@').unwrap_or((locale, ""));
    let base = base.split('.').next().unwrap_or_default();
    let mut parts = base.split(['_', '-']);
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let mut region = parts
        .rfind(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase);
    for m in modifiers.split([';', ',']) {
        if let Some(rg) = m.strip_prefix("rg=")
            && rg.len() >= 2
        {
            region = Some(rg[..2].to_ascii_uppercase());
        }
    }
    (language, region)
}

/// The region a language means when the locale names none.
fn default_region(language: &str) -> Option<String> {
    let region = match language {
        "en" => "US",
        "de" => "DE",
        "fr" => "FR",
        "es" => "ES",
        "it" => "IT",
        "nl" => "NL",
        "sv" => "SE",
        "ja" => "JP",
        "zh" => "CN",
        "pt" => "PT",
        "pl" => "PL",
        "ru" => "RU",
        "uk" => "UA",
        _ => return None,
    };
    Some(region.to_owned())
}

/// The short date and time of the common regions (CLDR short formats).
fn region_style(region: &str) -> Option<DateStyle> {
    use DateOrder::*;
    Some(match region {
        "US" | "PH" => DateStyle::new(Mdy, '/', false, false, true, true),
        "DE" | "AT" | "CH" | "LI" => DateStyle::new(Dmy, '.', true, false, false, true),
        "RU" | "UA" | "BY" | "KZ" => DateStyle::new(Dmy, '.', true, true, false, true),
        "PL" => DateStyle::new(Dmy, '.', false, true, false, true),
        "GB" | "IE" | "BR" | "PT" => DateStyle::new(Dmy, '/', true, true, false, true),
        "FR" => DateStyle::new(Dmy, '/', true, true, false, false),
        "ES" => DateStyle::new(Dmy, '/', false, false, false, true),
        "IT" => DateStyle::new(Dmy, '/', true, false, false, true),
        "NL" => DateStyle::new(Dmy, '-', true, true, false, false),
        "SE" => DateStyle::ISO,
        "JP" => DateStyle::new(Ymd, '/', true, true, false, false),
        "CN" => DateStyle::new(Ymd, '/', false, true, false, false),
        _ => return None,
    })
}

/// The date style of this system (`SPEC-0086` v3 behaviour 2): on macOS from the system settings
/// (`AppleLocale` and the forced 12/24-hour clock), elsewhere from `LC_ALL`, `LC_TIME` or `LANG`.
/// Read once per process.
pub fn local_date_style() -> DateStyle {
    static STYLE: std::sync::OnceLock<DateStyle> = std::sync::OnceLock::new();
    *STYLE.get_or_init(|| {
        if cfg!(target_os = "macos") {
            let read = |key: &str| {
                std::process::Command::new("defaults")
                    .args(["read", "-g", key])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            };
            if let Some(locale) = read("AppleLocale") {
                let force = if read("AppleICUForce24HourTime").as_deref() == Some("1") {
                    Some(true)
                } else if read("AppleICUForce12HourTime").as_deref() == Some("1") {
                    Some(false)
                } else {
                    None
                };
                return DateStyle::for_locale(&locale, force);
            }
        }
        ["LC_ALL", "LC_TIME", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .filter(|v| v != "C" && v != "POSIX" && !v.starts_with("C."))
            .map_or(DateStyle::ISO, |v| DateStyle::for_locale(&v, None))
    })
}

/// The date column (`SPEC-0086` v3 behaviour 2): "Today 14:05", "Yesterday 09:12", otherwise date and
/// time in `style`; `time` and `now` in seconds since the epoch, each in the local zone's offset at
/// that time.
pub fn date_text(time: i64, now: i64, zone: &LocalZone, style: &DateStyle) -> String {
    let local = time + zone.offset_at(time);
    let day = local.div_euclid(86_400);
    let today = (now + zone.offset_at(now)).div_euclid(86_400);
    let clock = style.clock(local.rem_euclid(86_400) / 60);
    match today - day {
        0 => format!("Today {clock}"),
        1 => format!("Yesterday {clock}"),
        _ => {
            let comma = if style.comma { "," } else { "" };
            format!("{}{comma} {clock}", style.date(day))
        }
    }
}

/// Whether `commit` matches the filter text (behaviour 3): subject, author or e-mail contain it, or
/// the hash starts with it, case ignored. An empty filter matches everything.
pub fn matches(commit: &CommitInfo, filter: &str) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty()
        || commit.id.starts_with(&f)
        || [&commit.summary, &commit.author, &commit.email]
            .iter()
            .any(|t| t.to_lowercase().contains(&f))
}

/// Full id of the commit the branch (or `HEAD`, or tag) `name` points at, from the labels per
/// commit (`SPEC-0086` v4 behaviour 9).
pub fn tip_of<'a>(
    labels: &'a std::collections::BTreeMap<String, Vec<String>>,
    name: &str,
) -> Option<&'a str> {
    labels
        .iter()
        .find(|(_, names)| names.iter().any(|n| n == name))
        .map(|(id, _)| id.as_str())
}

/// Ids of the listed commits reachable from `tip` through their parents (`SPEC-0086` v4
/// behaviour 9: choosing a branch shows only its commits).
pub fn reachable(commits: &[CommitInfo], tip: &str) -> std::collections::HashSet<String> {
    let by_id: std::collections::HashMap<&str, &CommitInfo> =
        commits.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![tip];
    while let Some(id) = todo.pop() {
        let Some(c) = by_id.get(id) else {
            continue;
        };
        if seen.insert(c.id.clone()) {
            todo.extend(c.parents.iter().map(String::as_str));
        }
    }
    seen
}

/// Indexes of the commits that match `filter`, in order.
pub fn filtered(commits: &[CommitInfo], filter: &str) -> Vec<usize> {
    (0..commits.len())
        .filter(|&i| matches(&commits[i], filter))
        .collect()
}

/// The row to select after filtering or reading again: the previously selected commit if it is
/// still shown, else none (`SPEC-0086` v3 behaviour 2: nothing is preselected).
pub fn reselect(commits: &[CommitInfo], shown: &[usize], previous: Option<&str>) -> Option<usize> {
    previous.and_then(|id| shown.iter().position(|&i| commits[i].id == id))
}

/// How far back the date filter reaches (`SPEC-0086` v5 behaviour 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateRange {
    /// Every commit.
    #[default]
    All,
    /// The last 24 hours.
    Day,
    /// The last 7 days.
    Week,
    /// The last 30 days.
    Month,
    /// The last 365 days.
    Year,
}

impl DateRange {
    /// Every choice, in menu order.
    pub const ALL: [Self; 5] = [Self::All, Self::Day, Self::Week, Self::Month, Self::Year];

    /// Name in the menu and on the filter button.
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Day => "Last 24 hours",
            Self::Week => "Last 7 days",
            Self::Month => "Last 30 days",
            Self::Year => "Last 365 days",
        }
    }

    /// Earliest commit time kept, seconds since the epoch.
    pub fn since(self, now: i64) -> Option<i64> {
        let days = match self {
            Self::All => return None,
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
        };
        Some(now - days * 86_400)
    }
}

/// The text filter compiled once: plain text or a regular expression, with or without case
/// (`SPEC-0086` v5 behaviour 3).
#[derive(Debug, Clone)]
pub enum TextMatcher {
    /// Matches everything.
    Any,
    /// Plain text, lower-cased unless case matters.
    Plain {
        /// The text, lower-cased unless case matters.
        text: String,
        /// Case matters.
        case: bool,
    },
    /// A regular expression.
    Regex(regex::Regex),
}

impl TextMatcher {
    /// Compiles `text`; an invalid regular expression gives its error message.
    pub fn new(text: &str, regex: bool, case: bool) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Self::Any);
        }
        if regex {
            return regex::RegexBuilder::new(text)
                .case_insensitive(!case)
                .size_limit(1 << 20)
                .build()
                .map(Self::Regex)
                .map_err(|e| e.to_string());
        }
        Ok(Self::Plain {
            text: if case {
                text.to_owned()
            } else {
                text.to_lowercase()
            },
            case,
        })
    }

    /// Whether the subject, author or e-mail match, or (plain text only) the hash starts with it.
    pub fn matches(&self, commit: &CommitInfo) -> bool {
        let fields = [&commit.summary, &commit.author, &commit.email];
        match self {
            Self::Any => true,
            Self::Plain { text, case } => {
                commit.id.starts_with(&text.to_lowercase())
                    || fields.iter().any(|f| {
                        if *case {
                            f.contains(text.as_str())
                        } else {
                            f.to_lowercase().contains(text.as_str())
                        }
                    })
            }
            Self::Regex(re) => fields.iter().any(|f| re.is_match(f)),
        }
    }
}

/// Every filter of the log at once (`SPEC-0086` v5 behaviour 10); empty fields filter nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFilter {
    /// Text or hash.
    pub text: String,
    /// The text is a regular expression.
    pub regex: bool,
    /// Case matters.
    pub case: bool,
    /// Only commits by this author (name).
    pub author: Option<String>,
    /// Only commits in this time range.
    pub date: DateRange,
    /// Only commits that change one of these repository-relative paths (files or folders).
    pub paths: Vec<String>,
}

impl LogFilter {
    /// Whether any part filters (the branch filter is separate).
    pub fn active(&self) -> bool {
        !self.text.trim().is_empty()
            || self.author.is_some()
            || self.date != DateRange::All
            || !self.paths.is_empty()
    }
}

/// Indexes of the commits that pass `filter` (`SPEC-0086` v5 behaviour 10). `in_branch` limits to
/// the commits of the chosen branch, `touching` to those that change one of the filter's paths
/// (both `None` when that filter is off); `now` is the current time. An invalid regular expression
/// gives its message.
pub fn apply(
    commits: &[CommitInfo],
    filter: &LogFilter,
    in_branch: Option<&std::collections::HashSet<String>>,
    touching: Option<&std::collections::HashSet<String>>,
    now: i64,
) -> Result<Vec<usize>, String> {
    let text = TextMatcher::new(&filter.text, filter.regex, filter.case)?;
    let since = filter.date.since(now);
    Ok((0..commits.len())
        .filter(|&i| {
            let c = &commits[i];
            in_branch.is_none_or(|s| s.contains(&c.id))
                && touching.is_none_or(|s| s.contains(&c.id))
                && filter.author.as_ref().is_none_or(|a| &c.author == a)
                && since.is_none_or(|t| c.time >= t)
                && text.matches(c)
        })
        .collect())
}

/// Authors of `commits`, most commits first, then by name (`SPEC-0086` v5 behaviour 10).
pub fn authors(commits: &[CommitInfo]) -> Vec<String> {
    let mut count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for c in commits {
        *count.entry(c.author.as_str()).or_default() += 1;
    }
    let mut out: Vec<(&str, usize)> = count.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    out.into_iter().map(|(a, _)| a.to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(id: &str, summary: &str, author: &str) -> CommitInfo {
        CommitInfo {
            id: id.into(),
            short_id: id[..7].into(),
            summary: summary.into(),
            author: author.into(),
            email: format!("{}@example.invalid", author.to_lowercase()),
            time: 0,
            parents: Vec::new(),
            committer: None,
        }
    }

    #[test]
    fn dates_read_like_the_reference() {
        // 2026-10-31 14:05 UTC.
        let now = 1_793_455_500;
        let iso = &DateStyle::ISO;
        assert_eq!(
            date_text(now, now, &LocalZone::Fixed(0), iso),
            "Today 14:05"
        );
        assert_eq!(
            date_text(now - 86_400, now, &LocalZone::Fixed(0), iso),
            "Yesterday 14:05"
        );
        assert_eq!(
            date_text(now - 3 * 86_400, now, &LocalZone::Fixed(0), iso),
            "2026-10-28 14:05"
        );
        // 23:30 UTC is already the next day at +01:00.
        assert_eq!(
            date_text(now + 9 * 3600 + 25 * 60, now, &LocalZone::Fixed(3600), iso),
            "2026-11-01 00:30"
        );
        assert_eq!(
            date_text(0, now, &LocalZone::Fixed(0), iso),
            "1970-01-01 00:00"
        );
        assert_eq!(
            date_text(951_782_400, now, &LocalZone::Fixed(0), iso),
            "2000-02-29 00:00"
        );
        assert_eq!(parse_offset("+0130"), Some(5400));
        assert_eq!(parse_offset("-0800"), Some(-28_800));
        assert_eq!(parse_offset("UTC"), None);
    }

    #[test]
    fn filter_and_selection() {
        let commits = [
            commit("a1b2c3d4e5", "Fix the parser", "Ada"),
            commit("f6e7d8c9b0", "Add tests", "Bob"),
            commit("0123456789", "Parser docs", "Cy"),
        ];
        assert_eq!(filtered(&commits, ""), [0, 1, 2]);
        assert_eq!(filtered(&commits, "PARSER"), [0, 2]);
        assert_eq!(filtered(&commits, "bob"), [1]);
        assert_eq!(filtered(&commits, "f6e7"), [1]);
        assert_eq!(filtered(&commits, "e7d8"), Vec::<usize>::new());
        assert_eq!(filtered(&commits, "cy@example"), [2]);
        let shown = filtered(&commits, "parser");
        assert_eq!(reselect(&commits, &shown, Some("0123456789")), Some(1));
        assert_eq!(reselect(&commits, &shown, Some("f6e7d8c9b0")), None);
        assert_eq!(
            reselect(&commits, &shown, None),
            None,
            "nothing preselected"
        );
    }

    /// SPEC-0086 v4 AK-10: the commits of a branch, found from its tip through the parents.
    #[test]
    fn commits_reachable_from_a_branch() {
        let c = |id: &str, parents: &[&str]| {
            let mut c = commit(&format!("{id}000000"), id, "Ada");
            c.parents = parents.iter().map(|p| format!("{p}000000")).collect();
            c
        };
        let commits = [
            c("merge", &["main1", "side1"]),
            c("side1", &["base"]),
            c("main1", &["base"]),
            c("other", &["base"]),
            c("base", &["beyond"]),
        ];
        let ids = |set: std::collections::HashSet<String>| {
            let mut v: Vec<String> = set.into_iter().map(|s| s[..s.len() - 6].into()).collect();
            v.sort();
            v
        };
        assert_eq!(
            ids(reachable(&commits, "merge000000")),
            ["base", "main1", "merge", "side1"]
        );
        assert_eq!(ids(reachable(&commits, "other000000")), ["base", "other"]);
        assert!(reachable(&commits, "unknown").is_empty());
        let labels: std::collections::BTreeMap<String, Vec<String>> = [
            ("merge000000".to_owned(), vec!["main".to_owned()]),
            (
                "other000000".to_owned(),
                vec!["origin/x".into(), "v1".into()],
            ),
        ]
        .into();
        assert_eq!(tip_of(&labels, "v1"), Some("other000000"));
        assert_eq!(tip_of(&labels, "main"), Some("merge000000"));
        assert_eq!(tip_of(&labels, "gone"), None);
    }

    /// SPEC-0086 v5 AK-13: text plain or as a regular expression, with or without case; author,
    /// date range, branch and paths together; the list of authors.
    #[test]
    fn filters_combine() {
        let day = 86_400;
        let now = 100 * day;
        let mut commits = vec![
            commit("a1b2c3d4e5", "Fix the Parser", "Ada"),
            commit("f6e7d8c9b0", "Add tests", "Bob"),
            commit("0123456789", "parser docs", "Ada"),
        ];
        commits[0].time = now - 3600;
        commits[1].time = now - 3 * day;
        commits[2].time = now - 40 * day;
        let run = |f: &LogFilter| apply(&commits, f, None, None, now).unwrap();
        let text = |t: &str, regex: bool, case: bool| LogFilter {
            text: t.into(),
            regex,
            case,
            ..LogFilter::default()
        };
        assert_eq!(run(&LogFilter::default()), [0, 1, 2]);
        assert!(!LogFilter::default().active());
        assert_eq!(run(&text("parser", false, false)), [0, 2]);
        assert_eq!(run(&text("Parser", false, true)), [0]);
        assert_eq!(run(&text("f6e7", false, false)), [1], "hash start");
        assert_eq!(run(&text("^(fix|add)", true, false)), [0, 1]);
        assert_eq!(run(&text("^parser", true, true)), [2]);
        assert_eq!(
            run(&text("f6e7", true, false)),
            Vec::<usize>::new(),
            "no hash in regex mode"
        );
        assert!(apply(&commits, &text("(", true, false), None, None, now).is_err());
        let ada = LogFilter {
            author: Some("Ada".into()),
            ..LogFilter::default()
        };
        assert!(ada.active());
        assert_eq!(run(&ada), [0, 2]);
        let week = LogFilter {
            date: DateRange::Week,
            ..LogFilter::default()
        };
        assert_eq!(run(&week), [0, 1]);
        assert_eq!(
            run(&LogFilter {
                date: DateRange::Day,
                ..LogFilter::default()
            }),
            [0]
        );
        assert_eq!(DateRange::All.since(now), None);
        assert_eq!(DateRange::Month.since(now), Some(70 * day));
        let both = LogFilter {
            author: Some("Ada".into()),
            date: DateRange::Year,
            text: "docs".into(),
            ..LogFilter::default()
        };
        assert_eq!(run(&both), [2]);
        let branch: std::collections::HashSet<String> =
            [commits[1].id.clone(), commits[2].id.clone()].into();
        let paths: std::collections::HashSet<String> =
            [commits[0].id.clone(), commits[2].id.clone()].into();
        assert_eq!(
            apply(
                &commits,
                &LogFilter::default(),
                Some(&branch),
                Some(&paths),
                now
            )
            .unwrap(),
            [2]
        );
        assert_eq!(authors(&commits), ["Ada", "Bob"]);
    }

    /// SPEC-0086 v5 behaviour 3 / AK-13: a regular expression on commit subjects: anchors, the
    /// case switch, author and e-mail searched too, an invalid expression.
    #[test]
    fn regex_filter_on_subjects() {
        let mut commits = vec![
            commit("a1b2c3d4e5", "initial", "Self Test"),
            commit("f6e7d8c9b0", "selftest commit", "Self Test"),
            commit("0123456789", "Select the row", "Bob"),
            commit("9876543210", "outside on side", "Bob"),
        ];
        commits[0].email = "selftest@example.invalid".into();
        commits[1].email = "selftest@example.invalid".into();
        commits[3].email = "self@test.invalid".into();
        let run = |t: &str, case: bool| {
            apply(
                &commits,
                &LogFilter {
                    text: t.into(),
                    regex: true,
                    case,
                    ..LogFilter::default()
                },
                None,
                None,
                0,
            )
        };
        // The window self-test's fixture: the e-mails start with "sel" as well, so `^sel` keeps
        // every commit whose e-mail does, plus the capital subject when case is ignored.
        assert_eq!(run("^sel", true).unwrap(), [0, 1, 3]);
        assert_eq!(run("^sel", false).unwrap(), [0, 1, 2, 3]);
        assert_eq!(run("^Sel", true).unwrap(), [0, 1, 2], "author Self Test");
        // Anchored on both ends only the subject matches.
        assert_eq!(run(r"^sel\w* commit$", true).unwrap(), [1]);
        assert_eq!(run("^select", false).unwrap(), [2]);
        assert_eq!(run("^select", true).unwrap(), Vec::<usize>::new());
        assert_eq!(run("side$", true).unwrap(), [3]);
        assert_eq!(run("commit$", true).unwrap(), [1]);
        // Blanks around the expression are not part of it.
        assert_eq!(run("  commit$ ", true).unwrap(), [1]);
        // No hash match in regex mode.
        assert_eq!(run("^f6e7", false).unwrap(), Vec::<usize>::new());
        // An invalid expression gives its message; plain text never fails.
        for bad in ["(", "[a-", "sel(", "*"] {
            assert!(run(bad, false).is_err(), "{bad}");
        }
        assert!(TextMatcher::new("(", false, true).is_ok());
    }

    /// SPEC-0086 v3 AK-8: dates and times in the system's regional format.
    #[test]
    fn summer_times_keep_their_offset_in_winter() {
        // OBS-0022 L1: a commit of 2026-10-05 11:30 CEST read on 2026-11-06 (CET) shows 11:30.
        let Ok(berlin) = jiff::tz::TimeZone::get("Europe/Berlin") else {
            return; // no zone database on this system
        };
        let zone = LocalZone::System(berlin);
        let (summer, winter) = (1_791_192_600, 1_793_948_400);
        assert_eq!(zone.offset_at(summer), 7200);
        assert_eq!(zone.offset_at(winter), 3600);
        let de = DateStyle::for_locale("de_DE", Some(true));
        assert_eq!(date_text(summer, winter, &zone, &de), "05.10.26, 11:30");
        assert_eq!(LocalZone::Fixed(3600).offset_at(summer), 3600);
    }

    #[test]
    fn regional_dates_and_clocks() {
        // 2026-11-05 14:05 UTC, shown two days later.
        let t = 1_793_455_500 + 5 * 86_400;
        let now = t + 2 * 86_400;
        let text = |locale: &str, force: Option<bool>| {
            date_text(
                t,
                now,
                &LocalZone::Fixed(0),
                &DateStyle::for_locale(locale, force),
            )
        };
        assert_eq!(text("de_DE", None), "05.11.26, 14:05");
        assert_eq!(text("de_DE.UTF-8", None), "05.11.26, 14:05");
        assert_eq!(text("en_US", None), "11/5/26, 2:05 PM");
        assert_eq!(text("en-GB", None), "05/11/2026, 14:05");
        assert_eq!(text("fr_FR", None), "05/11/2026 14:05");
        assert_eq!(text("sv_SE", None), "2026-11-05 14:05");
        assert_eq!(text("ja_JP", None), "2026/11/05 14:05");
        assert_eq!(text("xx_QQ", None), "2026-11-05 14:05", "unknown region");
        assert_eq!(text("de", None), "05.11.26, 14:05", "language alone");
        // macOS: English with the region set to Germany.
        assert_eq!(text("en_US@rg=dezzzz", None), "05.11.26, 14:05");
        assert_eq!(text("de_DE", Some(false)), "05.11.26, 2:05 PM");
        assert_eq!(text("en_US", Some(true)), "11/5/26, 14:05");
        let us = DateStyle::for_locale("en_US", None);
        assert_eq!(us.clock(0), "12:00 AM");
        assert_eq!(us.clock(12 * 60 + 1), "12:01 PM");
        assert_eq!(
            date_text(now - 3600, now, &LocalZone::Fixed(0), &us),
            "Today 1:05 PM",
            "today keeps the region's clock"
        );
    }
}
