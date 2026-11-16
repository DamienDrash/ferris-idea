//! Frames of the session protocol (`ADR-0026` §2, `SPEC-0317` AK-1): `length` (u32, big-endian,
//! the bytes after it), `type` (u8), `stream` (u32, big-endian) and the payload. A frame is at
//! most 1 MiB; anything malformed closes the connection.

use std::io::{self, Read, Write};

/// The largest value of the length field: type, stream id and payload together.
pub const MAX_FRAME: usize = 1 << 20;

/// Bytes of type and stream id inside the length.
const HEADER: usize = 5;

/// The largest payload of one frame.
pub const MAX_PAYLOAD: usize = MAX_FRAME - HEADER;

/// What a frame carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// A JSON-RPC message (UTF-8 JSON), stream 0.
    Message = 1,
    /// Raw bytes of a stream.
    Data = 2,
    /// More credit for a stream (u32, big-endian).
    Credit = 3,
    /// The end of a stream with a JSON reason.
    Close = 4,
}

impl FrameKind {
    fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Message),
            2 => Some(Self::Data),
            3 => Some(Self::Credit),
            4 => Some(Self::Close),
            _ => None,
        }
    }
}

/// One frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// What it carries.
    pub kind: FrameKind,
    /// `0` for messages, the stream id otherwise.
    pub stream: u32,
    /// JSON text, raw bytes, a credit or a close reason.
    pub payload: Vec<u8>,
}

/// Why a frame could not be read or written. Every variant except [`FrameError::Io`] is a
/// protocol violation that closes the connection without an answer (`ADR-0025` SEC-14).
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The length field exceeds 1 MiB (or a frame to send would).
    #[error("frame of {0} bytes is larger than 1 MiB")]
    TooLarge(usize),
    /// Unknown frame type.
    #[error("unknown frame type {0}")]
    UnknownType(u8),
    /// A length below the header, a message on a stream, a wrong credit or close payload.
    #[error("malformed frame: {0}")]
    Malformed(&'static str),
    /// A message that is not UTF-8.
    #[error("message is not UTF-8")]
    BadUtf8,
    /// The connection ended inside a frame.
    #[error("connection ended inside a frame")]
    Truncated,
    /// Reading or writing failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl Frame {
    /// A message frame with JSON `text`.
    pub fn message(text: impl Into<String>) -> Self {
        Self {
            kind: FrameKind::Message,
            stream: 0,
            payload: text.into().into_bytes(),
        }
    }

    /// A data frame of `stream`.
    pub fn data(stream: u32, bytes: Vec<u8>) -> Self {
        Self {
            kind: FrameKind::Data,
            stream,
            payload: bytes,
        }
    }

    /// A credit frame granting `bytes` more to `stream`.
    pub fn credit(stream: u32, bytes: u32) -> Self {
        Self {
            kind: FrameKind::Credit,
            stream,
            payload: bytes.to_be_bytes().to_vec(),
        }
    }

    /// A close frame of `stream` with a reason.
    pub fn close(stream: u32, reason: &str) -> Self {
        Self {
            kind: FrameKind::Close,
            stream,
            payload: serde_json::json!({ "reason": reason })
                .to_string()
                .into_bytes(),
        }
    }

    /// The message text of a message frame.
    pub fn text(&self) -> Option<&str> {
        (self.kind == FrameKind::Message)
            .then(|| std::str::from_utf8(&self.payload).ok())
            .flatten()
    }

    /// The credit of a credit frame.
    pub fn credit_bytes(&self) -> Option<u32> {
        (self.kind == FrameKind::Credit)
            .then(|| {
                self.payload
                    .as_slice()
                    .try_into()
                    .ok()
                    .map(u32::from_be_bytes)
            })
            .flatten()
    }

    /// The bytes on the wire; refuses a frame above 1 MiB.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        let length = HEADER + self.payload.len();
        if length > MAX_FRAME {
            return Err(FrameError::TooLarge(length));
        }
        let mut out = Vec::with_capacity(4 + length);
        out.extend_from_slice(&(length as u32).to_be_bytes());
        out.push(self.kind as u8);
        out.extend_from_slice(&self.stream.to_be_bytes());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// Writes the frame and flushes.
    pub fn write_to(&self, w: &mut impl Write) -> Result<(), FrameError> {
        w.write_all(&self.encode()?)?;
        w.flush()?;
        Ok(())
    }
}

/// Fills `buf` completely; `Ok(false)` when the stream ended before the first byte.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> Result<bool, FrameError> {
    let mut done = 0;
    while done < buf.len() {
        match r.read(&mut buf[done..]) {
            Ok(0) if done == 0 => return Ok(false),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(true)
}

/// Reads the next frame; `Ok(None)` when the connection ended between frames. The payload of an
/// oversized frame is never read.
pub fn read_frame(r: &mut impl Read) -> Result<Option<Frame>, FrameError> {
    let mut len = [0u8; 4];
    if !read_full(r, &mut len)? {
        return Ok(None);
    }
    let length = u32::from_be_bytes(len) as usize;
    if length > MAX_FRAME {
        return Err(FrameError::TooLarge(length));
    }
    if length < HEADER {
        return Err(FrameError::Malformed("length below the header"));
    }
    let mut head = [0u8; HEADER];
    if !read_full(r, &mut head)? {
        return Err(FrameError::Truncated);
    }
    let kind = FrameKind::from_u8(head[0]).ok_or(FrameError::UnknownType(head[0]))?;
    let stream = u32::from_be_bytes([head[1], head[2], head[3], head[4]]);
    let mut payload = vec![0u8; length - HEADER];
    if !payload.is_empty() && !read_full(r, &mut payload)? {
        return Err(FrameError::Truncated);
    }
    match kind {
        FrameKind::Message if stream != 0 => {
            return Err(FrameError::Malformed("message on a stream"));
        }
        FrameKind::Message if std::str::from_utf8(&payload).is_err() => {
            return Err(FrameError::BadUtf8);
        }
        FrameKind::Data | FrameKind::Credit | FrameKind::Close if stream == 0 => {
            return Err(FrameError::Malformed("stream frame on stream 0"));
        }
        FrameKind::Credit if payload.len() != 4 => {
            return Err(FrameError::Malformed("credit is not four bytes"));
        }
        FrameKind::Close if serde_json::from_slice::<serde_json::Value>(&payload).is_err() => {
            return Err(FrameError::Malformed("close reason is not JSON"));
        }
        _ => {}
    }
    Ok(Some(Frame {
        kind,
        stream,
        payload,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_the_cap_holds() {
        let frames = [
            Frame::message(r#"{"jsonrpc":"2.0","method":"session.ping"}"#),
            Frame::data(7, vec![0, 255, 10]),
            Frame::credit(7, 262_144),
            Frame::close(7, "done"),
        ];
        let mut wire = Vec::new();
        for f in &frames {
            f.write_to(&mut wire).unwrap();
        }
        // Length, type and stream id are big-endian.
        assert_eq!(&wire[..9], &[0, 0, 0, 46, 1, 0, 0, 0, 0]);
        let mut r = wire.as_slice();
        for f in &frames {
            assert_eq!(&read_frame(&mut r).unwrap().unwrap(), f);
        }
        assert!(read_frame(&mut r).unwrap().is_none());
        assert_eq!(frames[2].credit_bytes(), Some(262_144));

        // Exactly 1 MiB passes, one byte more is refused before anything is written.
        let max = Frame::data(1, vec![b'x'; MAX_PAYLOAD]);
        let bytes = max.encode().unwrap();
        assert_eq!(bytes.len(), 4 + MAX_FRAME);
        assert_eq!(read_frame(&mut bytes.as_slice()).unwrap().unwrap(), max);
        let over = Frame::data(1, vec![b'x'; MAX_PAYLOAD + 1]);
        assert!(matches!(over.encode(), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn malformed_frames_are_refused() {
        let frame = |len: u32, kind: u8, stream: u32, payload: &[u8]| {
            let mut v = len.to_be_bytes().to_vec();
            v.push(kind);
            v.extend_from_slice(&stream.to_be_bytes());
            v.extend_from_slice(payload);
            v
        };
        let read = |bytes: Vec<u8>| read_frame(&mut bytes.as_slice());
        // An oversized length is refused without reading the payload.
        assert!(matches!(
            read(((MAX_FRAME + 1) as u32).to_be_bytes().to_vec()),
            Err(FrameError::TooLarge(_))
        ));
        // Plain JSON and HTTP read as huge lengths.
        assert!(matches!(
            read(b"{\"jsonrpc\":\"2.0\"}".to_vec()),
            Err(FrameError::TooLarge(_))
        ));
        assert!(matches!(
            read(b"GET / HTTP/1.1\r\n\r\n".to_vec()),
            Err(FrameError::TooLarge(_))
        ));
        assert!(matches!(
            read(frame(5, 9, 0, b"")),
            Err(FrameError::UnknownType(9))
        ));
        assert!(matches!(
            read(frame(3, 1, 0, b"")),
            Err(FrameError::Malformed(_))
        ));
        assert!(matches!(
            read(frame(7, 1, 0, &[0xff, 0xfe])),
            Err(FrameError::BadUtf8)
        ));
        assert!(matches!(
            read(frame(7, 1, 3, b"{}")),
            Err(FrameError::Malformed(_))
        ));
        assert!(matches!(
            read(frame(7, 3, 1, b"ab")),
            Err(FrameError::Malformed(_))
        ));
        assert!(matches!(
            read(frame(7, 2, 0, b"ab")),
            Err(FrameError::Malformed(_))
        ));
        assert!(matches!(
            read(frame(7, 4, 1, b"no")),
            Err(FrameError::Malformed(_))
        ));
        assert!(matches!(
            read(frame(9, 1, 0, b"ab")),
            Err(FrameError::Truncated)
        ));
    }
}
