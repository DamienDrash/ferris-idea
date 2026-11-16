//! Debug Adapter Protocol base protocol (`SPEC-0039` behaviour 1).

use std::io::{BufRead, Read, Write};

use serde_json::Value;

use crate::DapError;

/// Encodes one message with its `Content-Length` header.
pub fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Writes one framed message.
pub fn write_message(writer: &mut impl Write, message: &Value) -> Result<(), DapError> {
    writer.write_all(&frame(message)).map_err(DapError::io)?;
    writer.flush().map_err(DapError::io)
}

/// Largest message body read (M8.2): a larger `Content-Length` is a protocol error instead of an
/// allocation of that size.
pub const MAX_MESSAGE: usize = 256 << 20;

/// Reads one framed message; `None` at a clean end of input.
pub fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, DapError> {
    let mut length = None;
    let mut first = true;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(DapError::io)? == 0 {
            return if first {
                Ok(None)
            } else {
                Err(DapError::Protocol("input ended inside a header".into()))
            };
        }
        first = false;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>().map_err(|_| {
                DapError::Protocol(format!("invalid Content-Length: {}", value.trim()))
            })?);
        }
    }
    let length = length.ok_or_else(|| DapError::Protocol("missing Content-Length".into()))?;
    if length > MAX_MESSAGE {
        return Err(DapError::Protocol(format!(
            "Content-Length {length} is larger than {MAX_MESSAGE}"
        )));
    }
    // Grows with what arrives, so a large length on a short stream allocates little.
    let mut body = Vec::new();
    reader
        .take(length as u64)
        .read_to_end(&mut body)
        .map_err(DapError::io)?;
    if body.len() < length {
        return Err(DapError::Protocol("input ended inside a message".into()));
    }
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| DapError::Protocol(format!("invalid JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_and_errors() {
        let a = json!({"seq": 1, "type": "request", "command": "initialize"});
        let b = json!({"seq": 2, "type": "event", "event": "output", "body": {"output": "ä\n"}});
        let mut bytes = frame(&a);
        bytes.extend(frame(&b));
        let mut reader = std::io::BufReader::with_capacity(3, bytes.as_slice());
        assert_eq!(read_message(&mut reader).unwrap(), Some(a));
        assert_eq!(read_message(&mut reader).unwrap(), Some(b));
        assert_eq!(read_message(&mut reader).unwrap(), None);

        let mut bad = b"Content-Length: x\r\n\r\n{}".as_slice();
        assert!(read_message(&mut bad).is_err());
        let mut missing = b"Other: 1\r\n\r\n{}".as_slice();
        assert!(read_message(&mut missing).is_err());
        let mut cut = b"Content-Length: 2\r\n".as_slice();
        assert!(read_message(&mut cut).is_err());
    }

    /// M8.2 fuzz finding: a huge `Content-Length` allocated its size up front and aborted.
    #[test]
    fn huge_or_cut_length_is_an_error() {
        let mut huge = b"Content-Length: 5222222222222222\r\n\r\n{}".as_slice();
        assert!(matches!(
            read_message(&mut huge),
            Err(DapError::Protocol(_))
        ));
        let mut cut = b"Content-Length: 100000\r\n\r\n{}".as_slice();
        assert!(matches!(read_message(&mut cut), Err(DapError::Protocol(_))));
    }
}
