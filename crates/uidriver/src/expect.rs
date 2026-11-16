//! Assertions that retry until their deadline (`SPEC-0319` behaviour 22, web-first assertions).

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, UiError};
use crate::locator::{MatchMode, TextMatch};
use crate::tree::{ElementId, ElementInfo, Tree, View};

/// How a text matcher compares, in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compare {
    /// The whole text.
    #[default]
    Exact,
    /// A part.
    Contains,
    /// A regular expression.
    Regex,
}

/// What an assertion expects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "matcher", rename_all = "snake_case")]
pub enum Matcher {
    /// One element, visible.
    Visible,
    /// No visible match (none at all counts).
    Hidden,
    /// One element, enabled.
    Enabled,
    /// One element, disabled.
    Disabled,
    /// One element with the focus.
    Focused,
    /// One element, checked.
    Checked,
    /// One element, selected.
    Selected,
    /// One element, expanded.
    Expanded,
    /// One element whose accessible name matches.
    Name {
        /// Expected text.
        text: String,
        /// How it compares.
        #[serde(default)]
        compare: Compare,
    },
    /// One element whose value matches (never for secret nodes).
    Value {
        /// Expected text.
        text: String,
        /// How it compares.
        #[serde(default)]
        compare: Compare,
    },
    /// One element whose text (with its descendants) matches.
    Text {
        /// Expected text.
        text: String,
        /// How it compares.
        #[serde(default)]
        compare: Compare,
    },
    /// The number of matches.
    Count {
        /// Exactly.
        #[serde(default)]
        exactly: Option<usize>,
        /// At least.
        #[serde(default)]
        at_least: Option<usize>,
        /// At most.
        #[serde(default)]
        at_most: Option<usize>,
    },
    /// No match at all.
    Absent,
}

fn text_match(text: &str, compare: Compare) -> TextMatch {
    TextMatch {
        mode: match compare {
            Compare::Exact => MatchMode::Exact,
            Compare::Contains => MatchMode::Contains,
            Compare::Regex => MatchMode::Regex,
        },
        text: text.to_owned(),
        ignore_case: false,
    }
}

impl Matcher {
    /// Its name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Hidden => "hidden",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Focused => "focused",
            Self::Checked => "checked",
            Self::Selected => "selected",
            Self::Expanded => "expanded",
            Self::Name { .. } => "name",
            Self::Value { .. } => "value",
            Self::Text { .. } => "text",
            Self::Count { .. } => "count",
            Self::Absent => "absent",
        }
    }

    /// Checks the arguments (a regular expression that does not compile).
    pub fn validate(&self) -> Result<(), UiError> {
        match self {
            Self::Name { text, compare }
            | Self::Value { text, compare }
            | Self::Text { text, compare } => text_match(text, *compare).validate(),
            Self::Count {
                exactly: None,
                at_least: None,
                at_most: None,
            } => Err(UiError::invalid(
                "invalid: `count` needs `exactly`, `at_least` or `at_most`",
            )),
            _ => Ok(()),
        }
    }

    /// Whether it is about one element (strict: more than one match fails with `ambiguous`).
    fn single(&self) -> bool {
        !matches!(self, Self::Hidden | Self::Count { .. } | Self::Absent)
    }
}

/// The outcome of evaluating an assertion once.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It holds.
    Pass,
    /// It does not hold yet; what the element shows.
    Fail(String),
    /// It can never hold as asked (an ambiguous locator).
    Error(UiError),
}

/// Evaluates `matcher` (inverted with `not`) over the matches of a locator.
pub fn evaluate(
    matcher: &Matcher,
    not: bool,
    tree: &Tree,
    matches: &[ElementId],
    view: &View,
) -> Outcome {
    if matcher.single() && matches.len() > 1 {
        let list = matches
            .iter()
            .filter_map(|id| tree.get(*id))
            .map(|n| ElementInfo::of(tree, n, view))
            .collect();
        return Outcome::Error(
            UiError::new(
                ErrorCode::Ambiguous,
                format!(
                    "ambiguous: the locator matches {} elements; `{}` needs one",
                    matches.len(),
                    matcher.name()
                ),
                "make the locator more precise (exact name, test id, within) or add nth",
            )
            .with_elements(list),
        );
    }
    let (holds, shows) = check(matcher, tree, matches, view);
    if holds != not {
        Outcome::Pass
    } else {
        Outcome::Fail(shows)
    }
}

/// Whether the matcher holds, and what was there.
fn check(matcher: &Matcher, tree: &Tree, matches: &[ElementId], view: &View) -> (bool, String) {
    let node = matches.first().and_then(|id| tree.get(*id));
    let shown = node
        .map(|n| ElementInfo::of(tree, n, view).outline())
        .unwrap_or_else(|| "no element".to_owned());
    let count = matches.len();
    let holds = match matcher {
        Matcher::Hidden => matches
            .iter()
            .filter_map(|id| tree.get(*id))
            .all(|n| !n.visible()),
        Matcher::Absent => count == 0,
        Matcher::Count {
            exactly,
            at_least,
            at_most,
        } => {
            return (
                exactly.is_none_or(|e| count == e)
                    && at_least.is_none_or(|l| count >= l)
                    && at_most.is_none_or(|m| count <= m),
                format!("{count} matches"),
            );
        }
        _ => {
            let Some(n) = node else {
                return (false, shown);
            };
            match matcher {
                Matcher::Visible => n.visible(),
                Matcher::Enabled => n.enabled(),
                Matcher::Disabled => !n.enabled(),
                Matcher::Focused => tree.is_focused(n.id),
                Matcher::Checked => n.checked() == Some(true),
                Matcher::Selected => n.selected() == Some(true),
                Matcher::Expanded => n.expanded() == Some(true),
                Matcher::Name { text, compare } => n
                    .name()
                    .is_some_and(|x| text_match(text, *compare).matches(x)),
                Matcher::Value { text, compare } => view
                    .value_of(n)
                    .is_some_and(|x| text_match(text, *compare).matches(x)),
                Matcher::Text { text, compare } => {
                    text_match(text, *compare).matches(&tree.text_of(n.id, view))
                }
                _ => false,
            }
        }
    };
    (holds, shown)
}
