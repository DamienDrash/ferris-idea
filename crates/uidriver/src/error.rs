//! Stable error codes of the driver (`SPEC-0319` behaviour 40). Every error carries a hint and,
//! where it helps, the elements it is about (the matches of an ambiguous locator, the closest
//! candidates of one that matched nothing, the element that covers a target).

use serde::{Deserialize, Serialize};

use crate::tree::ElementInfo;

/// The code of an error, as `data.code` of the session protocol and the prefix of MCP errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The locator matched nothing before the deadline.
    NotFound,
    /// The locator matched more than one element where one was needed.
    Ambiguous,
    /// The element handle names a node that is gone.
    Stale,
    /// The element has no visible part.
    NotVisible,
    /// The element is disabled.
    NotEnabled,
    /// The element does not take text.
    NotEditable,
    /// The hit test at the action point lands on another element.
    Obscured,
    /// A wait passed its deadline.
    DeadlineExceeded,
    /// Cancelled (cancellation handle, emergency stop).
    Cancelled,
    /// The user's own input paused the client.
    Interrupted,
    /// A right the client does not have.
    NotGranted,
    /// The element or action is reserved for the user.
    Reserved,
    /// The element declares no effect for its action.
    Undeclared,
    /// External driving is off, or there is no window.
    Unavailable,
    /// Not possible here (screenshots on GPUI 0.2.2, the native level for external clients).
    Unsupported,
    /// Arguments that make no sense (an unparsable locator).
    Invalid,
    /// Refused for its origin (a remote project's backend).
    Denied,
    /// A bug or an unexpected failure.
    Internal,
}

impl ErrorCode {
    /// The code as written in `data.code`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Ambiguous => "ambiguous",
            Self::Stale => "stale",
            Self::NotVisible => "not_visible",
            Self::NotEnabled => "not_enabled",
            Self::NotEditable => "not_editable",
            Self::Obscured => "obscured",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::NotGranted => "not_granted",
            Self::Reserved => "reserved",
            Self::Undeclared => "undeclared",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::Invalid => "invalid",
            Self::Denied => "denied",
            Self::Internal => "internal",
        }
    }

    /// The code of a `data.code` string.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == s)
    }

    /// Every code.
    pub const ALL: [ErrorCode; 18] = [
        Self::NotFound,
        Self::Ambiguous,
        Self::Stale,
        Self::NotVisible,
        Self::NotEnabled,
        Self::NotEditable,
        Self::Obscured,
        Self::DeadlineExceeded,
        Self::Cancelled,
        Self::Interrupted,
        Self::NotGranted,
        Self::Reserved,
        Self::Undeclared,
        Self::Unavailable,
        Self::Unsupported,
        Self::Invalid,
        Self::Denied,
        Self::Internal,
    ];
}

/// An error of the driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiError {
    /// The stable code.
    pub code: ErrorCode,
    /// What went wrong, without any value of a secret node or typed text.
    pub message: String,
    /// What to do about it.
    pub hint: String,
    /// The right a `not_granted` error names (`ui`, `vcs-write`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    /// The elements the error is about (at most 10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<ElementInfo>,
}

impl UiError {
    /// An error with `code`, `message` and `hint`.
    pub fn new(code: ErrorCode, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
            right: None,
            elements: Vec::new(),
        }
    }

    /// `not_granted` naming `right`.
    pub fn not_granted(right: &str, meaning: &str) -> Self {
        Self {
            right: Some(right.to_owned()),
            ..Self::new(
                ErrorCode::NotGranted,
                format!("not granted: this needs the `{right}` right ({meaning})"),
                "only the user grants rights: answer the question in the Ferris IDEA window, or add the right to agent-permissions.toml in the user's settings folder",
            )
        }
    }

    /// `invalid` with `message`.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::Invalid,
            message,
            "see SPEC-0319 behaviours 8, 9 and 16-22 for the forms",
        )
    }

    /// `unavailable` because external driving is off.
    pub fn driving_off() -> Self {
        Self::new(
            ErrorCode::Unavailable,
            "unavailable: agents and scripts may not operate the Ferris IDEA window",
            "only the user can switch it on: Settings › AI Assistant › \"Allow agents and scripts to operate the window\"",
        )
    }

    /// The same error with `elements` attached (at most 10).
    pub fn with_elements(mut self, elements: Vec<ElementInfo>) -> Self {
        self.elements = elements.into_iter().take(10).collect();
        self
    }

    /// One line: `code: message (hint)`.
    pub fn line(&self) -> String {
        // Messages usually start with their code already ("unavailable: …"): said once.
        let code = self.code.as_str();
        let mut text = if self
            .message
            .strip_prefix(code)
            .is_some_and(|rest| rest.starts_with(':'))
        {
            self.message.clone()
        } else {
            format!("{code}: {}", self.message)
        };
        if !self.hint.is_empty() {
            text.push_str(&format!(" ({})", self.hint));
        }
        text
    }
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.line())
    }
}

impl std::error::Error for UiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for c in ErrorCode::ALL {
            assert_eq!(ErrorCode::parse(c.as_str()), Some(c));
            assert!(seen.insert(c.as_str()));
            let json = serde_json::to_value(c).unwrap();
            assert_eq!(json, serde_json::json!(c.as_str()));
        }
        assert_eq!(ErrorCode::parse("nope"), None);
    }

    #[test]
    fn lines_start_with_the_code() {
        let e = UiError::not_granted("vcs-write", "change the repository");
        assert!(
            e.line()
                .starts_with("not_granted: not granted: this needs the `vcs-write`")
        );
        assert_eq!(e.right.as_deref(), Some("vcs-write"));
        assert!(UiError::driving_off().line().starts_with("unavailable:"));
        // A message that names its code already says it once (window check 2026-11-07).
        assert!(
            !UiError::driving_off()
                .line()
                .starts_with("unavailable: unavailable:")
        );
        let e = UiError::new(ErrorCode::Invalid, "invalid: no trace runs", "");
        assert_eq!(e.line(), "invalid: no trace runs");
    }
}
