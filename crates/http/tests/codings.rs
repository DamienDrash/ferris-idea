//! SPEC-0046 AK-11 against a local test server: content codings are decoded within limits.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use ferris_idea_http::{
    MAX_DECODE_RATIO, MAX_DECODED_BODY, RATIO_FLOOR, ResolvedRequest, Response, SendOptions, send,
    send_with,
};
use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};

const WAIT: Duration = Duration::from_secs(20);

/// What the server saw: request line and headers.
type Seen = (String, Vec<(String, String)>);

/// Serves one connection for each response (head without `Content-Length`, body); reports what it
/// saw. The thread ends with the last response.
fn server(responses: Vec<(String, Vec<u8>)>) -> (u16, Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for (head, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request_line = line.trim_end().to_owned();
            let mut headers = Vec::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                let Some((k, v)) = line.trim_end().split_once(':') else {
                    break;
                };
                headers.push((k.trim().to_owned(), v.trim().to_owned()));
            }
            let _ = tx.send((request_line, headers));
            let head = format!("{head}Content-Length: {}\r\n\r\n", body.len());
            // A client that has read enough may close before everything is written.
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (port, rx)
}

fn request(method: &str, port: u16, headers: &[(&str, &str)]) -> ResolvedRequest {
    ResolvedRequest {
        name: None,
        line: 1,
        method: method.into(),
        url: format!("http://127.0.0.1:{port}/"),
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: String::new(),
        unresolved: Vec::new(),
    }
}

fn from_file() -> SendOptions {
    SendOptions {
        timeout: WAIT,
        ..SendOptions::default()
    }
}

/// The response to a GET of one body with the content coding `coding`.
fn get(coding: &str, body: Vec<u8>) -> Response {
    let head =
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Encoding: {coding}\r\n");
    let (port, _seen) = server(vec![(head, body)]);
    send_with(&request("GET", port, &[]), &from_file()).unwrap()
}

fn gzip_at(data: &[u8], level: Compression) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), level);
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn gzip(data: &[u8]) -> Vec<u8> {
    gzip_at(data, Compression::default())
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

const TEXT: &str =
    "{\"greeting\": \"Grüße\", \"filler\": \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}";

#[test]
fn gzip_and_deflate_are_decoded() {
    for (coding, body) in [
        ("gzip", gzip(TEXT.as_bytes())),
        ("x-gzip", gzip(TEXT.as_bytes())),
        ("GZip", gzip(TEXT.as_bytes())),
        ("deflate", zlib(TEXT.as_bytes())),
        ("deflate", bare(TEXT.as_bytes())),
        ("identity, gzip", gzip(TEXT.as_bytes())),
    ] {
        let raw = body.clone();
        let r = get(coding, body);
        assert_eq!(r.text.as_deref(), Some(TEXT), "{coding}");
        // The bytes as they arrived stay, and `size` counts them.
        assert_eq!((r.size, &r.body), (raw.len(), &raw), "{coding}");
        let d = r.decoded.as_ref().unwrap();
        assert!(d.complete(), "{coding}");
        assert_eq!((d.size, &d.body[..]), (TEXT.len(), TEXT.as_bytes()));
        let name = coding.rsplit(", ").next().unwrap().to_ascii_lowercase();
        assert_eq!(
            r.notes,
            [format!(
                "The body was decoded from {name}: {} bytes received, {} bytes decoded.",
                raw.len(),
                TEXT.len()
            )],
            "{coding}"
        );
    }
    // `identity` alone is no coding.
    let r = get("identity", TEXT.as_bytes().to_vec());
    assert_eq!(r.text.as_deref(), Some(TEXT));
    assert!(r.decoded.is_none() && r.notes.is_empty());
}

#[test]
fn stacked_and_unknown_codings() {
    // Applied in the listed order, undone from the last.
    let r = get("deflate, gzip", gzip(&zlib(TEXT.as_bytes())));
    assert_eq!(r.text.as_deref(), Some(TEXT));
    assert_eq!(r.decoded.as_ref().unwrap().codings, ["gzip", "deflate"]);
    assert!(r.notes[0].starts_with("The body was decoded from gzip, then deflate: "));
    let r = get("gzip, deflate", bare(&gzip(TEXT.as_bytes())));
    assert_eq!(r.text.as_deref(), Some(TEXT));

    // In two header lines.
    let head = "HTTP/1.1 200 OK\r\nContent-Encoding: deflate\r\nContent-Encoding: gzip\r\n";
    let (port, _seen) = server(vec![(head.into(), gzip(&zlib(TEXT.as_bytes())))]);
    let r = send_with(&request("GET", port, &[]), &from_file()).unwrap();
    assert_eq!(r.text.as_deref(), Some(TEXT));

    // A coding Ferris IDEA does not decode: the body stays as it arrived, without text or note.
    for unknown in ["br", "zstd", "gzip, br"] {
        let raw = gzip(TEXT.as_bytes());
        let r = get(unknown, raw.clone());
        assert_eq!((r.text.as_deref(), r.size), (None, raw.len()), "{unknown}");
        assert!(r.decoded.is_none() && r.notes.is_empty() && r.body == raw);
    }
    // A known coding around it is undone; what is under it stays coded.
    let r = get("br, gzip", gzip(b"\x0b\x02\x80not decoded here\x03"));
    let d = r.decoded.as_ref().unwrap();
    assert_eq!((r.text.as_deref(), d.size), (None, 20));
    assert_eq!(
        (&d.codings[..], &d.remaining[..]),
        (&["gzip".to_owned()][..], &["br".to_owned()][..])
    );
    assert!(
        r.notes[0].ends_with(" Not decoded: the content coding br."),
        "{:?}",
        r.notes
    );
}

#[test]
fn a_damaged_stream_shows_what_was_decoded() {
    let text: String = (0..20_000u32)
        .map(|i| format!("{} ", i.wrapping_mul(2_654_435_761)))
        .collect();
    let whole = gzip(text.as_bytes());
    let r = get("gzip", whole[..whole.len() / 2].to_vec());
    assert_eq!(r.status, 200, "not an error");
    let d = r.decoded.as_ref().unwrap();
    assert_eq!(d.damaged.as_deref(), Some("it ends too early"));
    assert!(!d.cut && d.size > 1000 && text.as_bytes().starts_with(&d.body));
    assert_eq!(r.text.as_deref().map(str::len), Some(d.size));
    assert!(
        r.notes[0].contains("The coded body is damaged (it ends too early)"),
        "{:?}",
        r.notes
    );

    // Labelled gzip, but plain text.
    let r = get("gzip", b"plain text".to_vec());
    let d = r.decoded.as_ref().unwrap();
    assert!(d.damaged.is_some() && d.size == 0);
    assert_eq!(
        (r.text.as_deref(), &r.body[..]),
        (Some(""), &b"plain text"[..])
    );
}

#[test]
fn a_bomb_is_cut_at_the_limits() {
    // 256 MiB of zeros in about 260 kB: stops at the size limit, with that much in memory.
    let mut e = GzEncoder::new(Vec::new(), Compression::fast());
    let block = vec![0u8; 1024 * 1024];
    for _ in 0..256 {
        e.write_all(&block).unwrap();
    }
    let bomb = e.finish().unwrap();
    assert!(bomb.len() * MAX_DECODE_RATIO > MAX_DECODED_BODY && bomb.len() < 2 * 1024 * 1024);
    let r = get("gzip", bomb.clone());
    let d = r.decoded.as_ref().unwrap();
    assert!(d.cut && d.damaged.is_none());
    assert_eq!(
        (d.size, d.body.len(), r.size),
        (MAX_DECODED_BODY, MAX_DECODED_BODY, bomb.len())
    );
    assert_eq!(r.text.as_deref().map(str::len), Some(MAX_DECODED_BODY));
    assert!(
        r.notes[0].ends_with(" It is cut there: the limit is 16 MiB of decoded bytes."),
        "{:?}",
        r.notes
    );

    // 8 MiB of zeros in about 8 kB: more than 100 times what arrived.
    let small = gzip(&vec![0u8; 8 * 1024 * 1024]);
    assert!(small.len() * MAX_DECODE_RATIO < 4 * 1024 * 1024);
    let r = get("gzip", small.clone());
    let d = r.decoded.as_ref().unwrap();
    assert!(d.cut);
    assert_eq!(d.size, (small.len() * MAX_DECODE_RATIO).max(RATIO_FLOOR));
    assert!(
        r.notes[0].ends_with(
            " It is cut there: the limit is 100 times the bytes received, and at least 1 MiB."
        ),
        "{:?}",
        r.notes
    );

    // A bomb in a bomb is bounded layer by layer.
    let nested = gzip(&gzip_at(&vec![0u8; 64 * 1024 * 1024], Compression::fast()));
    let r = get("gzip, gzip", nested.clone());
    let d = r.decoded.as_ref().unwrap();
    assert!(d.cut && d.size <= (nested.len() * MAX_DECODE_RATIO).max(RATIO_FLOOR));

    // A body that compresses well but stays within the limits is whole.
    let fine = vec![b'a'; 900 * 1024];
    let r = get("gzip", gzip(&fine));
    assert!(r.decoded.as_ref().unwrap().complete());
    assert_eq!(r.text.as_deref().map(str::len), Some(fine.len()));
}

#[test]
fn empty_bodies_and_head() {
    let coded = "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n".to_owned();
    let (port, _seen) = server(vec![(coded.clone(), Vec::new())]);
    let r = send_with(&request("GET", port, &[]), &from_file()).unwrap();
    assert_eq!((r.text.as_deref(), r.size), (Some(""), 0));
    assert!(r.decoded.is_none() && r.notes.is_empty());

    // HEAD: the length of the body a GET would bring, and no body.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() > 2 {
            line.clear();
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 51\r\n\r\n")
            .unwrap();
    });
    let r = send_with(&request("HEAD", port, &[]), &from_file()).unwrap();
    served.join().unwrap();
    assert_eq!((r.status, r.text.as_deref(), r.size), (200, Some(""), 0));
    assert!(r.decoded.is_none() && r.notes.is_empty());

    let no_content = "HTTP/1.1 204 No Content\r\nContent-Encoding: gzip\r\n".to_owned();
    let (port, _seen) = server(vec![(no_content, Vec::new())]);
    let r = send_with(&request("GET", port, &[]), &from_file()).unwrap();
    assert!(r.status == 204 && r.decoded.is_none() && r.notes.is_empty());
}

#[test]
fn accept_encoding_is_sent_unless_the_request_sets_it() {
    let ok = || ("HTTP/1.1 200 OK\r\n".to_owned(), b"ok".to_vec());
    let accept = |seen: &Receiver<Seen>| -> Vec<String> {
        seen.recv()
            .unwrap()
            .1
            .into_iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"))
            .map(|(_, v)| v)
            .collect()
    };
    let redirect = (
        "HTTP/1.1 302 Found\r\nLocation: /next\r\n".to_owned(),
        Vec::new(),
    );
    let (port, seen) = server(vec![ok(), ok(), ok(), ok(), redirect, ok(), ok(), ok()]);
    // From a request file: asked for, once.
    send_with(&request("GET", port, &[]), &from_file()).unwrap();
    assert_eq!(accept(&seen), ["gzip, deflate"]);
    // The request's own header is sent alone, whatever it says.
    for own in ["identity", "br", ""] {
        let r = request("GET", port, &[("accept-encoding", own)]);
        send_with(&r, &from_file()).unwrap();
        assert_eq!(accept(&seen), [own]);
    }
    // After a redirect too.
    let r = send_with(&request("GET", port, &[]), &from_file()).unwrap();
    assert_eq!(r.redirects.len(), 1);
    assert_eq!(accept(&seen), ["gzip, deflate"]);
    assert_eq!(accept(&seen), ["gzip, deflate"]);
    // Not for a part of a body.
    let range = request("GET", port, &[("Range", "bytes=0-9")]);
    send_with(&range, &from_file()).unwrap();
    assert!(accept(&seen).is_empty());
    // Not for Ferris IDEA's own requests, which read the body as it arrives.
    send(&request("GET", port, &[]), WAIT).unwrap();
    assert!(accept(&seen).is_empty());

    // A request body is sent as written, never compressed.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let (mut line, mut head) = (String::new(), String::new());
        while reader.read_line(&mut line).unwrap() > 2 {
            head.push_str(&line);
            line.clear();
        }
        let mut body = vec![0u8; 400];
        std::io::Read::read_exact(&mut reader, &mut body).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        (head, body)
    });
    let mut post = request("POST", port, &[]);
    post.body = "a".repeat(400);
    send_with(&post, &from_file()).unwrap();
    let (head, body) = served.join().unwrap();
    assert!(
        head.contains("Content-Length: 400\r\n")
            && !head.to_lowercase().contains("content-encoding")
    );
    assert_eq!(body, vec![b'a'; 400]);
}
