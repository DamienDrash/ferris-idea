//! Performance budgets of `SPEC-0260` and their check.
//!
//! The budgets live in `budgets.toml` next to this crate; the headless harness
//! (`idecraft-bench run`) and the nightly window job (`scripts/perf-window.sh`) both write flat
//! metric maps (`name → number`) and check them here, so one file states every bar.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

/// The budget file, compiled in so the check needs no path.
pub const BUDGETS: &str = include_str!("../budgets.toml");

/// Where a metric is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Without a window, on every CI run (`scripts/ci-local.sh`).
    Headless,
    /// With a window, in the nightly window job (`scripts/ci-window.sh`).
    Window,
}

/// One budget: the metric may not exceed `max`; CI fails above `max × (1 + tolerance_pct/100)`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Budget {
    /// Metric name as written by the harness.
    pub metric: String,
    /// The budget itself.
    pub max: f64,
    /// Allowance for measurement noise, in percent of `max`.
    pub tolerance_pct: f64,
    /// Where it is measured.
    pub mode: Mode,
    /// The reference's value or the improvement target the budget derives from.
    pub bar: String,
}

#[derive(Deserialize)]
struct File {
    budget: Vec<Budget>,
}

/// Parses a budget file.
pub fn parse_budgets(text: &str) -> Result<Vec<Budget>, toml::de::Error> {
    Ok(toml::from_str::<File>(text)?.budget)
}

/// The compiled-in budgets.
pub fn budgets() -> Vec<Budget> {
    parse_budgets(BUDGETS).expect("budgets.toml is valid")
}

/// Outcome of one budget.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// At or below the budget.
    Within(f64),
    /// Above the budget but inside the tolerance: reported, not failed.
    Tolerated(f64),
    /// Above budget and tolerance: CI fails.
    Exceeded(f64),
    /// The run did not produce the metric.
    Missing,
}

impl Verdict {
    /// Whether CI fails on this verdict (a missing metric fails only when it was expected).
    pub fn fails(&self) -> bool {
        matches!(self, Verdict::Exceeded(_))
    }
}

/// Checks `metrics` against every budget of `mode`.
pub fn check<'a>(
    budgets: &'a [Budget],
    mode: Mode,
    metrics: &BTreeMap<String, f64>,
) -> Vec<(&'a Budget, Verdict)> {
    budgets
        .iter()
        .filter(|b| b.mode == mode)
        .map(|b| {
            let verdict = match metrics.get(&b.metric) {
                None => Verdict::Missing,
                Some(&v) if v <= b.max => Verdict::Within(v),
                Some(&v) if v <= b.max * (1.0 + b.tolerance_pct / 100.0) => Verdict::Tolerated(v),
                Some(&v) => Verdict::Exceeded(v),
            };
            (b, verdict)
        })
        .collect()
}

/// One line per budget for the log, e.g. `ok    typing_sync_ms_p99  1.20 ≤ 4 (bar: …)`.
pub fn report(results: &[(&Budget, Verdict)]) -> String {
    let mut out = String::new();
    for (b, v) in results {
        let (tag, value) = match v {
            Verdict::Within(x) => ("ok", format!("{x:.2} ≤ {}", b.max)),
            Verdict::Tolerated(x) => (
                "warn",
                format!("{x:.2} > {} (within +{}%)", b.max, b.tolerance_pct),
            ),
            Verdict::Exceeded(x) => ("FAIL", format!("{x:.2} > {} + {}%", b.max, b.tolerance_pct)),
            Verdict::Missing => ("miss", "not measured".to_owned()),
        };
        out.push_str(&format!(
            "{tag:<5} {:<26} {value}  (bar: {})\n",
            b.metric, b.bar
        ));
    }
    out
}

/// The `p`-quantile (0.0–1.0) of `values` in milliseconds, nearest rank.
pub fn percentile_ms(values: &[Duration], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort();
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i].as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_file_parses_and_names_are_unique() {
        let b = budgets();
        assert!(b.iter().any(|b| b.mode == Mode::Headless));
        assert!(b.iter().any(|b| b.mode == Mode::Window));
        let mut names: Vec<_> = b.iter().map(|b| b.metric.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), b.len());
        assert!(b.iter().all(|b| b.max > 0.0 && b.tolerance_pct >= 0.0));
    }

    #[test]
    fn verdicts_respect_the_tolerance() {
        let b = parse_budgets(
            "[[budget]]\nmetric = \"a\"\nmax = 10.0\ntolerance_pct = 20\nmode = \"headless\"\nbar = \"x\"\n\
             [[budget]]\nmetric = \"b\"\nmax = 1.0\ntolerance_pct = 0\nmode = \"window\"\nbar = \"y\"\n",
        )
        .unwrap();
        let at = |v: f64| {
            let m = BTreeMap::from([("a".to_owned(), v)]);
            check(&b, Mode::Headless, &m)[0].1.clone()
        };
        assert_eq!(at(10.0), Verdict::Within(10.0));
        assert_eq!(at(11.9), Verdict::Tolerated(11.9));
        assert!(at(12.1).fails());
        let none = check(&b, Mode::Window, &BTreeMap::new());
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].1, Verdict::Missing);
        assert!(!none[0].1.fails());
        assert!(report(&none).contains("miss"));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let v: Vec<_> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile_ms(&v, 0.5), 51.0);
        assert_eq!(percentile_ms(&v, 0.99), 99.0);
        assert_eq!(percentile_ms(&v, 1.0), 100.0);
        assert_eq!(percentile_ms(&[], 0.5), 0.0);
    }
}
