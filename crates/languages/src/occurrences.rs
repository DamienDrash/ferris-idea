//! The occurrences of the symbol at a position within its file, from the language server's
//! `textDocument/documentHighlight` (`SPEC-0295`): the capability, the request and the decoding of
//! the answer with its kinds. There is no fallback to text matches (behaviour 2).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::insight::{Encoding, Opened, Range, from_server, line_of};
use crate::lsp::LspError;

/// The request.
const METHOD: &str = "textDocument/documentHighlight";

/// The text size above which nothing is asked (behaviour 11): the limit above which the editor has
/// no syntax tree (`SPEC-0260` behaviour 4).
pub const MAX_TEXT_BYTES: usize = crate::syntax::MAX_SYNTAX_BYTES;

/// What an occurrence does with the symbol. The order is the strength used when a server lists a
/// range twice: write before read before text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OccurrenceKind {
    /// The server did not say (LSP kind 1, or none).
    Text,
    /// The symbol is read (LSP kind 2).
    Read,
    /// The symbol is written (LSP kind 3).
    Write,
}

/// One occurrence, in characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occurrence {
    /// Where it is.
    #[serde(flatten)]
    pub range: Range,
    /// Read, write or text.
    pub kind: OccurrenceKind,
}

/// How a question ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Highlights {
    /// The server's occurrences, in the order of the text; empty when it knows none.
    Found(Vec<Occurrence>),
    /// The server (its program name) does not offer usage highlights (behaviour 2).
    NotOffered(String),
    /// The server (its program name) is starting or still loading the project; it was not asked
    /// (behaviour 3).
    NotReady(String),
    /// The text is larger than [`MAX_TEXT_BYTES`]; nothing was asked (behaviour 11).
    TooLarge,
}

/// The occurrence `GotoNextElementUnderCaretUsage` (`forward`) or `GotoPrevElementUnderCaretUsage`
/// goes to from `at` (`SPEC-0295` v2 behaviour 15): its index in `spans`, the occurrences as sorted
/// start and end places. From inside or at an occurrence it is the one after or before it, around
/// the ends; from elsewhere the nearest one after or before `at`, around the ends as well. `None`
/// without occurrences.
pub fn neighbour<P: Ord + Copy>(spans: &[(P, P)], at: P, forward: bool) -> Option<usize> {
    let n = spans.len();
    if n == 0 {
        return None;
    }
    if let Some(on) = spans
        .iter()
        .position(|(start, end)| *start <= at && at <= *end)
    {
        return Some(if forward {
            (on + 1) % n
        } else {
            (on + n - 1) % n
        });
    }
    Some(if forward {
        spans.iter().position(|(start, _)| *start > at).unwrap_or(0)
    } else {
        spans
            .iter()
            .rposition(|(_, end)| *end < at)
            .unwrap_or(n - 1)
    })
}

/// Whether a server with these `capabilities` offers the request: `documentHighlightProvider` is
/// `true` or an options object.
pub fn offered(capabilities: &Value) -> bool {
    let provider = &capabilities["documentHighlightProvider"];
    provider.as_bool() == Some(true) || provider.is_object()
}

/// Decodes the answer `v` for `text` whose positions are in `encoding`: sorted, without empty or
/// reversed ranges and ranges outside the text, equal ranges once with the strongest kind
/// (behaviour 11).
pub fn parse(v: &Value, text: &str, encoding: Encoding) -> Vec<Occurrence> {
    let lines = text.split('\n').count() as u64;
    let mut out: Vec<Occurrence> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let point = |p: &Value| {
                let line = p["line"].as_u64()?;
                if line >= lines {
                    return None;
                }
                let character =
                    from_server(line_of(text, line), p["character"].as_u64()?, encoding);
                Some((line, character))
            };
            let start = point(&h["range"]["start"])?;
            let end = point(&h["range"]["end"])?;
            if end <= start {
                return None;
            }
            let kind = match h["kind"].as_u64() {
                Some(2) => OccurrenceKind::Read,
                Some(3) => OccurrenceKind::Write,
                _ => OccurrenceKind::Text,
            };
            Some(Occurrence {
                range: Range {
                    start_line: start.0,
                    start_character: start.1,
                    end_line: end.0,
                    end_character: end.1,
                },
                kind,
            })
        })
        .collect();
    let place = |o: &Occurrence| {
        (
            o.range.start_line,
            o.range.start_character,
            o.range.end_line,
            o.range.end_character,
        )
    };
    // The strongest kind of an equal range first, so it is the one kept.
    out.sort_by_key(|o| (place(o), std::cmp::Reverse(o.kind)));
    out.dedup_by_key(|o| place(o));
    out
}

/// Asks the server of the opened file for the occurrences at a position, waiting at most `wait` or
/// until `stop` is set; a given-up question is withdrawn with `$/cancelRequest` (behaviour 10). A
/// server that does not announce the capability is not asked.
pub(crate) async fn highlights_of(
    o: &Opened,
    line: u64,
    character: u64,
    wait: Duration,
    stop: Option<Arc<AtomicBool>>,
) -> Result<Highlights, LspError> {
    if !offered(&o.capabilities) {
        return Ok(Highlights::NotOffered(o.lsp().name.clone()));
    }
    match o
        .ask_bounded(METHOD, o.position(line, character), wait, stop)
        .await
    {
        Ok(v) => Ok(Highlights::Found(parse(&v, &o.text, o.files.encoding))),
        // A server that announces the capability and does not know the request.
        Err(LspError::Unsupported { server, .. }) => Ok(Highlights::NotOffered(server)),
        Err(e) => Err(e),
    }
}

/// The occurrences of the symbol at a position of `file` within the file, on `text` (the editor's
/// text) or the saved content (`SPEC-0295` behaviours 2, 3, 10 and 11).
///
/// With `only_if_ready` (the editor) a pooled server that is starting or still loading the project
/// is not asked; without it (agents) the question waits for the server like any other.
#[allow(clippy::too_many_arguments)]
pub async fn document_highlights_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
    stop: Option<Arc<AtomicBool>>,
    only_if_ready: bool,
) -> Result<Highlights, LspError> {
    let size = match text {
        Some(t) => t.len() as u64,
        None => std::fs::metadata(file)
            .map_err(|e| LspError::Io(e.to_string()))?
            .len(),
    };
    if size > MAX_TEXT_BYTES as u64 {
        return Ok(Highlights::TooLarge);
    }
    if only_if_ready && let Some(program) = crate::pool::loading(root, file) {
        return Ok(Highlights::NotReady(program));
    }
    let o = Opened::open_text(root, file, text, wait).await?;
    let result = highlights_of(&o, line, character, wait, stop).await;
    o.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{LspClient, frame, read_message};
    use serde_json::json;
    use std::sync::atomic::Ordering;
    use tokio::io::{AsyncWriteExt, BufReader, DuplexStream, duplex};

    const TEXT: &str = "int total = 0;\ntotal = total + 1;\n";

    /// SPEC-0295/AK-9: next and previous occurrence, around both ends.
    #[test]
    fn the_neighbour_wraps_at_the_ends() {
        let spans = [(4, 9), (30, 35), (38, 43)];
        // From inside, the start and the end of an occurrence.
        for at in [4, 6, 9] {
            assert_eq!(neighbour(&spans, at, true), Some(1), "forward from {at}");
            assert_eq!(neighbour(&spans, at, false), Some(2), "backward from {at}");
        }
        assert_eq!(neighbour(&spans, 40, true), Some(0));
        assert_eq!(neighbour(&spans, 40, false), Some(1));
        // From between occurrences, before the first and after the last.
        assert_eq!(neighbour(&spans, 20, true), Some(1));
        assert_eq!(neighbour(&spans, 20, false), Some(0));
        assert_eq!(neighbour(&spans, 0, true), Some(0));
        assert_eq!(neighbour(&spans, 0, false), Some(2));
        assert_eq!(neighbour(&spans, 50, true), Some(0));
        assert_eq!(neighbour(&spans, 50, false), Some(2));
        // One occurrence leads to itself; none leads nowhere.
        assert_eq!(neighbour(&spans[..1], 5, true), Some(0));
        assert_eq!(neighbour(&spans[..1], 5, false), Some(0));
        assert_eq!(neighbour::<usize>(&[], 5, true), None);
        // Places of line and character compare the same way.
        let lines = [((0u64, 4u64), (0, 9)), ((2, 0), (2, 5))];
        assert_eq!(neighbour(&lines, (0, 5), true), Some(1));
        assert_eq!(neighbour(&lines, (1, 0), false), Some(0));
    }

    fn highlight(line: u64, from: u64, to: u64, kind: Option<u64>) -> Value {
        let mut h = json!({"range": {
            "start": {"line": line, "character": from},
            "end": {"line": line, "character": to},
        }});
        if let Some(k) = kind {
            h["kind"] = json!(k);
        }
        h
    }

    fn places(found: &[Occurrence]) -> Vec<(u64, u64, u64, OccurrenceKind)> {
        found
            .iter()
            .map(|o| {
                (
                    o.range.start_line,
                    o.range.start_character,
                    o.range.end_character,
                    o.kind,
                )
            })
            .collect()
    }

    /// An opened file over a scripted server with the given capabilities.
    fn scripted(capabilities: Value) -> (Opened, BufReader<DuplexStream>, DuplexStream) {
        let (client_out, server_in) = duplex(1 << 16);
        let (server_out, client_in) = duplex(1 << 16);
        let mut client = LspClient::new(client_in, client_out, None::<DuplexStream>);
        client.name = "scripted-server".to_owned();
        let mut o = Opened::over(client, Path::new("/p/sum.c"), TEXT);
        o.capabilities = capabilities;
        (o, BufReader::new(server_in), server_out)
    }

    /// SPEC-0295/AK-1.
    #[test]
    fn the_capability_is_true_or_an_object() {
        assert!(offered(&json!({"documentHighlightProvider": true})));
        assert!(offered(
            &json!({"documentHighlightProvider": {"workDoneProgress": false}})
        ));
        assert!(!offered(&json!({"documentHighlightProvider": false})));
        assert!(!offered(&json!({"hoverProvider": true})));
        assert!(!offered(&Value::Null));
    }

    /// SPEC-0295/AK-1: kinds, a missing kind as text, sorting, empty and outside ranges, equal
    /// ranges once with the strongest kind, UTF-16 positions as characters.
    #[test]
    fn an_answer_is_decoded_with_its_kinds() {
        let answer = json!([
            highlight(1, 8, 13, Some(2)),
            highlight(1, 0, 5, Some(3)),
            highlight(0, 4, 9, None),
            highlight(0, 4, 9, Some(3)),
            highlight(0, 4, 9, Some(2)),
            highlight(1, 3, 3, Some(2)),
            highlight(9, 0, 5, Some(2)),
            highlight(0, 11, 12, Some(1)),
        ]);
        assert_eq!(
            places(&parse(&answer, TEXT, Encoding::Utf16)),
            [
                (0, 4, 9, OccurrenceKind::Write),
                (0, 11, 12, OccurrenceKind::Text),
                (1, 0, 5, OccurrenceKind::Write),
                (1, 8, 13, OccurrenceKind::Read),
            ]
        );
        assert!(parse(&Value::Null, TEXT, Encoding::Utf16).is_empty());
        // `𝑥` is two UTF-16 units and one character.
        let text = "𝑥𝑥 = n;\n";
        let found = parse(&json!([highlight(0, 7, 8, Some(2))]), text, Encoding::Utf16);
        assert_eq!(places(&found), [(0, 5, 6, OccurrenceKind::Read)]);
        let serialized = serde_json::to_value(&found[0]).unwrap();
        assert_eq!(
            serialized,
            json!({"start_line": 0, "start_character": 5, "end_line": 0, "end_character": 6, "kind": "read"})
        );
    }

    /// SPEC-0295/AK-2: the question carries the caret's position; the answer's kinds come back.
    #[tokio::test]
    async fn the_scripted_server_is_asked_at_the_caret() {
        let (o, mut input, mut output) = scripted(json!({"documentHighlightProvider": true}));
        let server = async {
            let asked = read_message(&mut input).await.unwrap().unwrap();
            assert_eq!(asked["method"], METHOD);
            assert_eq!(asked["params"]["textDocument"]["uri"], "file:///p/sum.c");
            assert_eq!(
                asked["params"]["position"],
                json!({"line": 1, "character": 2})
            );
            let result = json!([
                highlight(0, 4, 9, Some(3)),
                highlight(1, 0, 5, Some(3)),
                highlight(1, 8, 13, Some(2)),
            ]);
            let answer = json!({"jsonrpc": "2.0", "id": asked["id"], "result": result});
            output.write_all(&frame(&answer)).await.unwrap();
        };
        let asking = highlights_of(&o, 1, 2, Duration::from_secs(5), None);
        let (found, ()) = tokio::join!(asking, server);
        let Highlights::Found(found) = found.unwrap() else {
            panic!("the server offers highlights");
        };
        assert_eq!(
            places(&found),
            [
                (0, 4, 9, OccurrenceKind::Write),
                (1, 0, 5, OccurrenceKind::Write),
                (1, 8, 13, OccurrenceKind::Read),
            ]
        );
    }

    /// SPEC-0295/AK-2: without the capability nothing is sent.
    #[tokio::test]
    async fn a_server_without_the_capability_is_not_asked() {
        let (o, mut input, _output) = scripted(json!({"hoverProvider": true}));
        let found = highlights_of(&o, 1, 2, Duration::from_secs(5), None)
            .await
            .unwrap();
        assert_eq!(found, Highlights::NotOffered("scripted-server".into()));
        // Nothing reaches the server.
        let read = tokio::time::timeout(Duration::from_millis(200), read_message(&mut input)).await;
        assert!(read.is_err(), "{read:?}");
    }

    /// SPEC-0295/AK-2: "method not found" from a server that announced the capability.
    #[tokio::test]
    async fn an_unknown_request_counts_as_not_offered() {
        let (o, mut input, mut output) = scripted(json!({"documentHighlightProvider": {}}));
        let server = async {
            let asked = read_message(&mut input).await.unwrap().unwrap();
            let answer = json!({"jsonrpc": "2.0", "id": asked["id"],
                "error": {"code": -32601, "message": "unknown"}});
            output.write_all(&frame(&answer)).await.unwrap();
        };
        let (found, ()) = tokio::join!(
            highlights_of(&o, 0, 5, Duration::from_secs(5), None),
            server
        );
        assert_eq!(
            found.unwrap(),
            Highlights::NotOffered("scripted-server".into())
        );
    }

    /// SPEC-0295/AK-2: a question whose reason is gone is withdrawn.
    #[tokio::test]
    async fn a_stopped_question_is_withdrawn() {
        let (o, mut input, _output) = scripted(json!({"documentHighlightProvider": true}));
        let stop = Arc::new(AtomicBool::new(false));
        let server = async {
            let asked = read_message(&mut input).await.unwrap().unwrap();
            assert_eq!(asked["method"], METHOD);
            stop.store(true, Ordering::SeqCst);
            let withdrawn = read_message(&mut input).await.unwrap().unwrap();
            assert_eq!(withdrawn["method"], "$/cancelRequest");
            assert_eq!(withdrawn["params"]["id"], asked["id"]);
        };
        let asking = highlights_of(&o, 0, 5, Duration::from_secs(600), Some(Arc::clone(&stop)));
        let both = async { tokio::join!(asking, server) };
        let (found, ()) = tokio::time::timeout(Duration::from_secs(10), both)
            .await
            .expect("a stopped question ends");
        assert!(matches!(found, Err(LspError::Stopped(_))), "{found:?}");
    }

    /// SPEC-0295/AK-2: a text over the limit is not asked about, whatever server the file has.
    #[tokio::test]
    async fn a_large_text_is_not_asked_about() {
        let text = "a".repeat(MAX_TEXT_BYTES + 1);
        let found = document_highlights_in(
            Path::new("/nowhere"),
            Path::new("/nowhere/big.c"),
            Some(&text),
            0,
            0,
            Duration::from_secs(1),
            None,
            true,
        )
        .await
        .unwrap();
        assert_eq!(found, Highlights::TooLarge);
    }
}
