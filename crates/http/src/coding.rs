//! Content codings of a response body (`SPEC-0046` behaviour 12): `gzip`, `x-gzip` and `deflate` are
//! undone within limits, so a small response cannot fill the memory; any other coding stays as it is.
//! Formats: RFC 9110 §8.4, RFC 1952 (gzip), RFC 1950 (zlib), RFC 1951 (deflate).

use std::io::Read;

use serde::Serialize;

/// Most decoded bytes of a body.
pub const MAX_DECODED_BODY: usize = 16 * 1024 * 1024;
/// Most decoded bytes for each byte that arrived.
pub const MAX_DECODE_RATIO: usize = 100;
/// Decoded bytes that are always allowed, whatever the ratio.
pub const RATIO_FLOOR: usize = 1024 * 1024;

/// A body after undoing its content codings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decoded {
    /// The codings undone, in the order they were undone: the last one listed by the server first.
    pub codings: Vec<String>,
    /// The codings that are still on [`Decoded::body`] because Ferris IDEA does not decode them, in
    /// the order the server listed them.
    pub remaining: Vec<String>,
    /// The decoded bytes.
    #[serde(skip)]
    pub body: Vec<u8>,
    /// Number of decoded bytes.
    pub size: usize,
    /// Whether decoding stopped at a limit; the body ends there.
    pub cut: bool,
    /// What is wrong with the coded stream; the body is what was decoded up to that place.
    pub damaged: Option<String>,
}

impl Decoded {
    /// Whether the body is all of what the server coded: nothing left, cut or damaged.
    pub fn complete(&self) -> bool {
        self.remaining.is_empty() && !self.cut && self.damaged.is_none()
    }

    /// The sentence for the notes of a response; `received` is the number of bytes that arrived.
    pub fn note(&self, received: usize) -> String {
        let mut note = format!(
            "The body was decoded from {}: {received} bytes received, {} bytes decoded.",
            self.codings.join(", then "),
            self.size
        );
        if self.cut {
            note.push_str(&format!(" It is cut there: {}.", limit_text(received)));
        }
        if let Some(why) = &self.damaged {
            note.push_str(&format!(
                " The coded body is damaged ({why}); this is what was decoded before that place."
            ));
        }
        if !self.remaining.is_empty() {
            note.push_str(&format!(
                " Not decoded: the content coding {}.",
                self.remaining.join(", ")
            ));
        }
        note
    }
}

/// The limit a body of `received` bytes runs into, in words.
pub fn limit_text(received: usize) -> String {
    if allowed(received, &LIMITS) == LIMITS.size {
        format!(
            "the limit is {} MiB of decoded bytes",
            MAX_DECODED_BODY / (1024 * 1024)
        )
    } else {
        format!(
            "the limit is {MAX_DECODE_RATIO} times the bytes received, and at least {} MiB",
            RATIO_FLOOR / (1024 * 1024)
        )
    }
}

/// The limits of decoding.
#[derive(Debug, Clone, Copy)]
struct Limits {
    size: usize,
    ratio: usize,
    floor: usize,
}

const LIMITS: Limits = Limits {
    size: MAX_DECODED_BODY,
    ratio: MAX_DECODE_RATIO,
    floor: RATIO_FLOOR,
};

/// Most decoded bytes for a body of `received` bytes.
fn allowed(received: usize, limits: &Limits) -> usize {
    received
        .saturating_mul(limits.ratio)
        .max(limits.floor)
        .min(limits.size)
}

/// The content codings of a response in the order the server applied them, lower case, without
/// `identity`: every `Content-Encoding` header, each a comma-separated list (RFC 9110 §8.4).
pub fn codings(headers: &[(String, String)]) -> Vec<String> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
        .flat_map(|(_, value)| value.split(','))
        .map(|coding| coding.trim().to_ascii_lowercase())
        .filter(|coding| !coding.is_empty() && coding != "identity")
        .collect()
}

/// Undoes the content codings of `body`. `None` when there is nothing to undo: no coding, an empty
/// body, or a coding Ferris IDEA does not decode as the last one applied.
pub fn decode(headers: &[(String, String)], body: &[u8]) -> Option<Decoded> {
    decode_within(headers, body, &LIMITS)
}

fn decode_within(headers: &[(String, String)], body: &[u8], limits: &Limits) -> Option<Decoded> {
    let mut remaining = codings(headers);
    if body.is_empty() {
        return None;
    }
    let most = allowed(body.len(), limits);
    let mut undone = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    let (mut cut, mut damaged) = (false, None);
    while let Some(kind) = remaining.last().and_then(|name| Kind::of(name)) {
        let input = current.as_deref().unwrap_or(body);
        // A stream that follows a cut or damaged one ends too early by itself: that is no news.
        let partial = cut || damaged.is_some();
        let (output, end) = inflate(kind, input, most);
        match end {
            End::Complete => {}
            End::Cut => cut = true,
            End::Damaged(_) if partial => {}
            End::Damaged(why) => damaged = Some(why),
        }
        current = Some(output);
        undone.extend(remaining.pop());
    }
    let body = current?;
    Some(Decoded {
        codings: undone,
        remaining,
        size: body.len(),
        body,
        cut,
        damaged,
    })
}

/// A coding Ferris IDEA decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Gzip,
    Deflate,
}

impl Kind {
    fn of(name: &str) -> Option<Self> {
        match name {
            "gzip" | "x-gzip" => Some(Self::Gzip),
            "deflate" => Some(Self::Deflate),
            _ => None,
        }
    }
}

/// How decoding one coding ended.
#[derive(Debug, PartialEq, Eq)]
enum End {
    Complete,
    Cut,
    Damaged(String),
}

/// Whether `input` starts with a zlib header (RFC 1950 §2.2): method 8 and a valid check value.
fn has_zlib_header(input: &[u8]) -> bool {
    matches!(input, [cmf, flg, ..]
        if cmf & 0x0f == 8 && cmf >> 4 <= 7 && (u16::from(*cmf) << 8 | u16::from(*flg)) % 31 == 0)
}

/// Decodes `input` piece by piece and stops after `most` bytes.
fn inflate(kind: Kind, input: &[u8], most: usize) -> (Vec<u8>, End) {
    match kind {
        Kind::Gzip => drain(flate2::read::MultiGzDecoder::new(input), most),
        Kind::Deflate if has_zlib_header(input) => {
            let (output, end) = drain(flate2::read::ZlibDecoder::new(input), most);
            if output.is_empty() && matches!(end, End::Damaged(_)) {
                // The two bytes looked like a zlib header by chance.
                let bare = drain(flate2::read::DeflateDecoder::new(input), most);
                if !bare.0.is_empty() || bare.1 == End::Complete {
                    return bare;
                }
            }
            (output, end)
        }
        Kind::Deflate => drain(flate2::read::DeflateDecoder::new(input), most),
    }
}

/// Reads `reader` to its end, to its first error or to `most` bytes, whichever comes first. The
/// output never grows beyond `most`.
fn drain(mut reader: impl Read, most: usize) -> (Vec<u8>, End) {
    let mut output = Vec::new();
    let mut piece = vec![0u8; 32 * 1024];
    loop {
        match reader.read(&mut piece) {
            Ok(0) => return (output, End::Complete),
            Ok(got) => {
                let room = most - output.len();
                if got > room {
                    output.extend_from_slice(&piece[..room]);
                    return (output, End::Cut);
                }
                output.extend_from_slice(&piece[..got]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return (output, End::Damaged("it ends too early".into()));
            }
            Err(e) => return (output, End::Damaged(e.to_string())),
        }
    }
}

/// The start of `bytes` that is whole UTF-8 characters, when only the last character is
/// incomplete: the text of a body that was cut or is damaged.
pub fn whole_characters(bytes: &[u8]) -> &[u8] {
    match std::str::from_utf8(bytes) {
        Err(e) if e.error_len().is_none() => &bytes[..e.valid_up_to()],
        _ => bytes,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};

    use super::*;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn bare(data: &[u8]) -> Vec<u8> {
        let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn coded(value: &str) -> Vec<(String, String)> {
        vec![("Content-Encoding".into(), value.into())]
    }

    #[test]
    fn known_codings_are_undone_in_reverse_order() {
        let text = b"{\"hello\": \"world\"}".repeat(20);
        for (header, body) in [
            ("gzip", gzip(&text)),
            ("x-gzip", gzip(&text)),
            ("GZIP", gzip(&text)),
            ("deflate", zlib(&text)),
            ("deflate", bare(&text)),
            ("identity, gzip", gzip(&text)),
            // Applied in the listed order: deflate first, gzip around it.
            ("deflate, gzip", gzip(&zlib(&text))),
            ("gzip, deflate", bare(&gzip(&text))),
            ("gzip, gzip", gzip(&gzip(&text))),
        ] {
            let d = decode(&coded(header), &body).unwrap_or_else(|| panic!("{header}"));
            assert_eq!(d.body, text, "{header}");
            assert!(d.complete() && d.size == text.len(), "{header}");
        }
        // Two members one after another are one gzip body (RFC 1952 §2.2).
        let two = [gzip(b"first, "), gzip(b"second")].concat();
        assert_eq!(decode(&coded("gzip"), &two).unwrap().body, b"first, second");
        // Several header lines are one list.
        let lines = vec![
            ("content-encoding".to_owned(), "deflate".to_owned()),
            ("Content-Encoding".to_owned(), "gzip".to_owned()),
        ];
        let d = decode(&lines, &gzip(&zlib(&text))).unwrap();
        assert_eq!(
            (d.codings.clone(), d.body.clone()),
            (vec!["gzip".into(), "deflate".into()], text.clone())
        );
        assert_eq!(
            d.note(77),
            format!(
                "The body was decoded from gzip, then deflate: 77 bytes received, {} bytes decoded.",
                text.len()
            )
        );
    }

    #[test]
    fn nothing_to_undo() {
        assert_eq!(decode(&[], b"plain"), None);
        assert_eq!(decode(&coded("identity"), b"plain"), None);
        assert_eq!(decode(&coded(""), b"plain"), None);
        assert_eq!(decode(&coded("gzip"), b""), None, "an empty body");
        for unknown in ["br", "zstd", "compress", "gzip, br"] {
            assert_eq!(decode(&coded(unknown), &gzip(b"x")), None, "{unknown}");
        }
        // A known coding around an unknown one is undone; the unknown one stays.
        let d = decode(&coded("br, gzip"), &gzip(b"not really brotli")).unwrap();
        assert_eq!(d.body, b"not really brotli");
        assert_eq!(
            (d.codings.clone(), d.remaining.clone()),
            (vec!["gzip".to_owned()], vec!["br".to_owned()])
        );
        assert!(!d.complete());
        assert!(d.note(1).ends_with(" Not decoded: the content coding br."));
    }

    #[test]
    fn damaged_streams_keep_what_was_decoded() {
        // Text that does not compress to a few bytes, so half of the stream holds half of it.
        let text: Vec<u8> = (0..20_000u32)
            .flat_map(|i| format!("{} ", i.wrapping_mul(2_654_435_761)).into_bytes())
            .collect();
        let whole = gzip(&text);
        let d = decode(&coded("gzip"), &whole[..whole.len() / 2]).unwrap();
        assert_eq!(d.damaged.as_deref(), Some("it ends too early"));
        assert!(!d.cut && !d.body.is_empty() && text.starts_with(&d.body));
        assert!(
            d.note(9)
                .contains("The coded body is damaged (it ends too early)")
        );

        let mut broken = zlib(&text);
        let middle = broken.len() / 2;
        broken[middle..middle + 64].fill(0xff);
        let d = decode(&coded("deflate"), &broken).unwrap();
        assert!(d.damaged.is_some(), "{} bytes", d.size);
        // What precedes the damage is right; the decoder may pass on a few wrong bytes before it
        // notices (deflate has no check before the end of the stream).
        let sure = text.len() / 4;
        assert!(d.size > sure && d.body[..sure] == text[..sure]);

        // Not a gzip stream at all: nothing decoded, said so, no panic.
        let d = decode(&coded("gzip"), b"plain text labelled gzip").unwrap();
        assert!(d.body.is_empty() && d.damaged.is_some());
        let d = decode(&coded("deflate"), &[0x78, 0x9c, 0xff, 0xff, 0xff]).unwrap();
        assert!(d.body.is_empty() && d.damaged.is_some());

        // A damaged outer coding does not hide what the inner one still gives.
        let outer = gzip(&zlib(&text));
        let d = decode(&coded("deflate, gzip"), &outer[..outer.len() / 2]).unwrap();
        assert_eq!(d.damaged.as_deref(), Some("it ends too early"));
        assert!(d.remaining.is_empty() && !d.body.is_empty() && text.starts_with(&d.body));

        assert_eq!(whole_characters("Grüße".as_bytes()), "Grüße".as_bytes());
        assert_eq!(whole_characters(&"Grü".as_bytes()[..3]), b"Gr");
        assert_eq!(whole_characters(&[0xff, b'a']), &[0xff, b'a'], "not text");
    }

    #[test]
    fn bombs_are_cut_at_the_limits() {
        let small = Limits {
            size: 10_000,
            ratio: 100,
            floor: 1_000,
        };
        assert_eq!(allowed(1, &small), 1_000);
        assert_eq!(allowed(50, &small), 5_000);
        assert_eq!(allowed(5_000, &small), 10_000);
        assert_eq!(allowed(usize::MAX, &small), 10_000);
        assert_eq!(allowed(10, &LIMITS), RATIO_FLOOR);
        assert_eq!(allowed(20_000, &LIMITS), 2_000_000);
        assert_eq!(allowed(1_000_000, &LIMITS), MAX_DECODED_BODY);

        // 1 MB of zeros in about 1 kB: cut at the ratio, exactly there.
        let zeros = vec![0u8; 1_000_000];
        let bomb = gzip(&zeros);
        assert!(bomb.len() < 2_000);
        let most = allowed(bomb.len(), &small);
        let d = decode_within(&coded("gzip"), &bomb, &small).unwrap();
        assert!(d.cut && d.damaged.is_none());
        assert_eq!((d.size, d.body.len()), (most, most));
        // A body of exactly the limit is whole.
        let fits = gzip(&zeros[..most]);
        let d = decode_within(
            &coded("gzip"),
            &fits,
            &Limits {
                floor: most,
                ..small
            },
        )
        .unwrap();
        assert!(d.complete() && d.size == most);

        // A bomb inside a bomb: the limits hold for each layer, against the bytes that arrived.
        let nested = gzip(&gzip(&zeros));
        let d = decode_within(&coded("gzip, gzip"), &nested, &small).unwrap();
        assert!(d.cut && d.size <= allowed(nested.len(), &small));
        assert!(d.remaining.is_empty() && d.damaged.is_none(), "{d:?}");

        // The real limits: 64 MiB of zeros arrive as about 64 kB and stop at the ratio.
        let mut e = GzEncoder::new(Vec::new(), Compression::fast());
        let block = vec![0u8; 1024 * 1024];
        for _ in 0..64 {
            e.write_all(&block).unwrap();
        }
        let big = e.finish().unwrap();
        let d = decode(&coded("gzip"), &big).unwrap();
        assert!(d.cut);
        assert_eq!(d.size, allowed(big.len(), &LIMITS));
        assert!(d.size <= MAX_DECODED_BODY && d.body.iter().all(|&b| b == 0));
        assert!(d.note(big.len()).contains("It is cut there: the limit is"));
        assert!(limit_text(10).contains("100 times the bytes received"));
        assert!(limit_text(10_000_000).contains("16 MiB of decoded bytes"));
    }
}
