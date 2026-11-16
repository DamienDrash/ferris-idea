//! JSON-RPC 2.0 messages with `domain.verb` names and stable error codes (`ADR-0026` §3).

use serde_json::{Value, json};

/// The stable error codes in `error.data.code` (`ADR-0012` rule 2, readiness A4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// A right the session does not have.
    NotGranted,
    /// Refused (a wrong token, a user's answer).
    Denied,
    /// No such document, file, session or method.
    NotFound,
    /// The file changed on disk since it was loaded.
    Conflict,
    /// The revision the request was computed on is gone.
    StaleRevision,
    /// Cancelled with `$/cancel`.
    Cancelled,
    /// The request's `deadline_ms` passed.
    DeadlineExceeded,
    /// Larger than a frame or a limit.
    TooLarge,
    /// No common protocol major.
    UnsupportedVersion,
    /// The backend or the connection is not there.
    Unavailable,
    /// Arguments that make no sense (paths leaving the project, overlapping ops).
    Invalid,
    /// A bug or an unexpected failure.
    Internal,
}

impl ErrorCode {
    /// The string in `error.data.code`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotGranted => "not_granted",
            Self::Denied => "denied",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::StaleRevision => "stale_revision",
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::TooLarge => "too_large",
            Self::UnsupportedVersion => "unsupported_version",
            Self::Unavailable => "unavailable",
            Self::Invalid => "invalid",
            Self::Internal => "internal",
        }
    }

    /// The code for a `data.code` string.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == s)
    }

    /// Every code.
    pub const ALL: [ErrorCode; 12] = [
        Self::NotGranted,
        Self::Denied,
        Self::NotFound,
        Self::Conflict,
        Self::StaleRevision,
        Self::Cancelled,
        Self::DeadlineExceeded,
        Self::TooLarge,
        Self::UnsupportedVersion,
        Self::Unavailable,
        Self::Invalid,
        Self::Internal,
    ];
}

/// JSON-RPC "method not found".
pub const METHOD_NOT_FOUND: i64 = -32601;

/// A JSON-RPC error with its stable code and a hint what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {message}", code.as_str())]
pub struct RpcError {
    /// The stable code.
    pub code: ErrorCode,
    /// What happened, in words.
    pub message: String,
    /// What to do.
    pub hint: Option<String>,
    /// The JSON-RPC number (`-32601` for an unknown method).
    pub number: i64,
}

impl RpcError {
    /// An error with `code` and `message`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let number = match code {
            ErrorCode::Invalid => -32602,
            ErrorCode::Cancelled => -32800,
            ErrorCode::Internal => -32603,
            _ => -32000,
        };
        Self {
            code,
            message: message.into(),
            hint: None,
            number,
        }
    }

    /// The same error with a hint.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The answer to an unknown method.
    pub fn method_not_found(method: &str) -> Self {
        Self {
            number: METHOD_NOT_FOUND,
            ..Self::new(ErrorCode::NotFound, format!("method not found: {method}"))
        }
    }

    /// The error object of a response.
    pub fn to_json(&self) -> Value {
        let mut data = json!({ "code": self.code.as_str() });
        if let Some(h) = &self.hint {
            data["hint"] = json!(h);
        }
        json!({ "code": self.number, "message": self.message, "data": data })
    }

    /// The error of a response's error object; an unknown code reads as `internal`.
    pub fn from_json(v: &Value) -> Self {
        let code = v["data"]["code"]
            .as_str()
            .and_then(ErrorCode::parse)
            .unwrap_or(ErrorCode::Internal);
        Self {
            code,
            message: v["message"].as_str().unwrap_or("error").to_owned(),
            hint: v["data"]["hint"].as_str().map(str::to_owned),
            number: v["code"].as_i64().unwrap_or(-32000),
        }
    }
}

/// One JSON-RPC message.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A request expecting an answer.
    Request {
        /// Its id (number or string).
        id: Value,
        /// `domain.verb`.
        method: String,
        /// Parameters (an object, `null` when absent).
        params: Value,
    },
    /// A notification.
    Notification {
        /// `domain.verb`.
        method: String,
        /// Parameters.
        params: Value,
    },
    /// An answer.
    Response {
        /// The request's id.
        id: Value,
        /// Result or error.
        result: Result<Value, RpcError>,
    },
}

impl Message {
    /// A request.
    pub fn request(id: u64, method: &str, params: Value) -> Self {
        Self::Request {
            id: json!(id),
            method: method.to_owned(),
            params,
        }
    }

    /// A notification.
    pub fn notification(method: &str, params: Value) -> Self {
        Self::Notification {
            method: method.to_owned(),
            params,
        }
    }

    /// Parses one message; anything that is not a JSON-RPC 2.0 object is `invalid`.
    pub fn parse(text: &str) -> Result<Self, RpcError> {
        let v: Value = serde_json::from_str(text)
            .map_err(|e| RpcError::new(ErrorCode::Invalid, format!("not JSON: {e}")))?;
        if v["jsonrpc"] != "2.0" {
            return Err(RpcError::new(
                ErrorCode::Invalid,
                "not a JSON-RPC 2.0 message",
            ));
        }
        let id = v
            .get("id")
            .cloned()
            .filter(|i| i.is_number() || i.is_string());
        if let Some(method) = v["method"].as_str() {
            let params = v.get("params").cloned().unwrap_or(Value::Null);
            return Ok(match id {
                Some(id) => Self::Request {
                    id,
                    method: method.to_owned(),
                    params,
                },
                None => Self::Notification {
                    method: method.to_owned(),
                    params,
                },
            });
        }
        let id = id.ok_or_else(|| RpcError::new(ErrorCode::Invalid, "answer without an id"))?;
        if let Some(e) = v.get("error") {
            return Ok(Self::Response {
                id,
                result: Err(RpcError::from_json(e)),
            });
        }
        match v.get("result") {
            Some(r) => Ok(Self::Response {
                id,
                result: Ok(r.clone()),
            }),
            None => Err(RpcError::new(
                ErrorCode::Invalid,
                "answer without result or error",
            )),
        }
    }

    /// The JSON text.
    pub fn to_text(&self) -> String {
        match self {
            Self::Request { id, method, params } => {
                json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
            }
            Self::Notification { method, params } => {
                json!({"jsonrpc": "2.0", "method": method, "params": params})
            }
            Self::Response { id, result: Ok(r) } => {
                json!({"jsonrpc": "2.0", "id": id, "result": r})
            }
            Self::Response { id, result: Err(e) } => {
                json!({"jsonrpc": "2.0", "id": id, "error": e.to_json()})
            }
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_with_stable_codes() {
        for m in [
            Message::request(3, "doc.open", json!({"path": "a.rs"})),
            Message::notification("doc.changed", json!({"seq": 1})),
            Message::Response {
                id: json!(3),
                result: Ok(json!({"revision": 2})),
            },
            Message::Response {
                id: json!("x"),
                result: Err(RpcError::new(ErrorCode::Conflict, "changed").hint("compare")),
            },
        ] {
            assert_eq!(Message::parse(&m.to_text()).unwrap(), m);
        }
        for c in ErrorCode::ALL {
            assert_eq!(ErrorCode::parse(c.as_str()), Some(c));
            let e = RpcError::new(c, "m");
            assert_eq!(RpcError::from_json(&e.to_json()).code, c);
        }
        let nf = RpcError::method_not_found("lsp.start");
        assert_eq!(nf.to_json()["code"], METHOD_NOT_FOUND);
        assert_eq!(nf.to_json()["data"]["code"], "not_found");
        assert!(Message::parse("[1]").is_err());
        assert!(Message::parse(r#"{"jsonrpc":"1.0","method":"x"}"#).is_err());
        assert!(Message::parse(r#"{"jsonrpc":"2.0","id":1}"#).is_err());
    }
}
