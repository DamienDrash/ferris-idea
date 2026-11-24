//! Running requests from `.http` files in the window (`SPEC-0096`): which request runs and how its
//! response is shown.

use ferris_idea_http::{RequestFile, ResolvedRequest, Response};

/// Whether `path` is a request file.
pub fn is_request_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("http") || e.eq_ignore_ascii_case("rest"))
}

/// Index of the request the caret on 1-based `line` belongs to: the last one starting on or before
/// it, else the first (behaviour 1).
pub fn request_at(file: &RequestFile, line: usize) -> Option<usize> {
    if file.requests.is_empty() {
        return None;
    }
    Some(
        file.requests
            .iter()
            .rposition(|r| r.line <= line)
            .unwrap_or(0),
    )
}

/// The output line for a response: "GET https://x/ → 200 OK · 12 ms · 345 bytes". The bytes are
/// counted as they arrived; a decoded body adds ", 2048 decoded from gzip" (`SPEC-0046` behaviour 12).
pub fn status_line(request: &ResolvedRequest, response: &Response) -> String {
    let decoded = response.decoded.as_ref().map_or_else(String::new, |d| {
        format!(", {} decoded from {}", d.size, d.codings.join(", then "))
    });
    let class = status_class(response.status).map_or_else(String::new, |c| format!(" · {c}"));
    format!(
        "{} {} → {} {}{class} · {} ms · {} bytes{decoded}",
        request.method,
        request.url,
        response.status,
        response.reason,
        response.millis,
        response.size
    )
}

/// What an error status means, in words (`SPEC-0096` v4 behaviour 3): an answer, not a failure,
/// so it is named instead of shown in red.
pub fn status_class(status: u16) -> Option<&'static str> {
    match status {
        400..=499 => Some("client error"),
        500..=599 => Some("server error"),
        _ => None,
    }
}

/// The 0-based lines of the requests of `text`, where their run marks stand (`SPEC-0096` v4
/// behaviour 6).
pub fn run_lines(text: &str) -> Vec<usize> {
    ferris_idea_http::parse(text)
        .requests
        .iter()
        .map(|r| r.line.saturating_sub(1))
        .collect()
}

/// The colours of a request file (`SPEC-0096` v4 behaviour 5): the classes of
/// [`ferris_idea_http::highlight`], a JSON body with JSON's own, the `{{…}}` in it kept whole.
pub fn spans(text: &str) -> Vec<ferris_idea_languages::syntax::Span> {
    let h = ferris_idea_http::highlight(text);
    // Byte offset of each character, and of the end.
    let bytes: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    let inside = |r: &std::ops::Range<usize>, body: &std::ops::Range<usize>| {
        r.start >= body.start && r.end <= body.end
    };
    let mut out: Vec<(std::ops::Range<usize>, String)> = h
        .spans
        .iter()
        .filter(|(r, _)| !h.bodies.iter().any(|(b, json)| *json && inside(r, b)))
        .map(|(r, k)| (r.clone(), (*k).to_owned()))
        .collect();
    for (body, _) in h.bodies.iter().filter(|(_, json)| *json) {
        let own: Vec<_> = h
            .spans
            .iter()
            .filter(|(r, _)| inside(r, body))
            .cloned()
            .collect();
        let json = ferris_idea_languages::syntax::SyntaxDocument::new(
            ferris_idea_languages::LanguageId::Json,
            false,
            &text[bytes[body.start]..bytes[body.end]],
        )
        .map(|d| d.highlight())
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.start + body.start..s.end + body.start, s.kind))
        .collect();
        out.extend(ferris_idea_http::overlay(json, &own));
    }
    out.sort_by_key(|(r, _)| (r.start, r.end));
    out.into_iter()
        .map(|(r, kind)| ferris_idea_languages::syntax::Span {
            start: r.start,
            end: r.end,
            kind,
        })
        .collect()
}

/// The lines Output gets for a response (behaviour 3): the status line, then one line for each
/// note of the request file (a part that is not supported) and of the response (a redirect that was
/// not followed, credentials kept back, a body that was decoded, cut or damaged; `SPEC-0046`
/// behaviours 5, 7, 9 and 12). None of them is an error.
pub fn output_lines(
    request: &ResolvedRequest,
    response: &Response,
    request_notes: &[String],
) -> Vec<(String, bool)> {
    std::iter::once(status_line(request, response))
        .chain(request_notes.iter().cloned())
        .chain(response.notes.iter().cloned())
        .map(|line| (line, false))
        .collect()
}

/// The text of the Response tab: status line, headers, a blank line and the body, JSON indented
/// (behaviour 3). A body with a content coding is shown decoded (`SPEC-0046` behaviour 12); one that
/// was cut at a limit or is damaged ends with a line saying so. A body without text is described by
/// its size and the content codings that were not decoded; it is never run or handed to another
/// program.
pub fn response_text(response: &Response) -> String {
    let mut out = format!("HTTP {} {}\n", response.status, response.reason);
    for (k, v) in &response.headers {
        out.push_str(&format!("{k}: {v}\n"));
    }
    out.push('\n');
    let json = response
        .header("content-type")
        .is_some_and(|t| t.to_ascii_lowercase().contains("json"));
    match &response.text {
        Some(text) if json => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v) => {
                out.push_str(&serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.clone()))
            }
            Err(_) => out.push_str(text),
        },
        Some(text) => out.push_str(text),
        None => {
            // What is left on the body: after decoding, or all the response names.
            let (size, left) = match &response.decoded {
                Some(d) => (d.size, d.remaining.join(", ")),
                None => (
                    response.size,
                    response
                        .headers
                        .iter()
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
                        .flat_map(|(_, value)| value.split(','))
                        .map(str::trim)
                        .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("identity"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            };
            if left.is_empty() {
                out.push_str(&format!("({size} bytes of binary data)"));
            } else {
                out.push_str(&format!(
                    "({size} bytes, content coding {left}, not decoded)"
                ));
            }
        }
    }
    if let Some(d) = &response.decoded {
        if d.cut {
            out.push_str(&format!(
                "\n\n(cut after {} decoded bytes: {})",
                d.size,
                ferris_idea_http::decode_limit(response.size)
            ));
        } else if let Some(why) = &d.damaged {
            out.push_str(&format!(
                "\n\n(the coded body is damaged, {why}: {} bytes were decoded before that place)",
                d.size
            ));
        }
    }
    out
}

/// The Output lines of a run, each request's lines at its place in the file, sent or not
/// (`SPEC-0096` v7 behaviour 9): `runs` holds the request's index in the file and its lines.
pub fn output_in_file_order(mut runs: Vec<(usize, Vec<(String, bool)>)>) -> Vec<(String, bool)> {
    runs.sort_by_key(|(index, _)| *index);
    runs.into_iter().flat_map(|(_, lines)| lines).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0096 v7 AK-11 (OBS-0061 H3): a skipped request at its place in the file.
    #[test]
    fn output_keeps_file_order() {
        let line = |t: &str, red: bool| (t.to_owned(), red);
        let lines = output_in_file_order(vec![
            (0, vec![line("GET /a → 200", false), line("note", false)]),
            (2, vec![line("GET /c → 404", true)]),
            (1, vec![line("GET /u → Unresolved variables: x", true)]),
        ]);
        let texts: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            texts,
            [
                "GET /a → 200",
                "note",
                "GET /u → Unresolved variables: x",
                "GET /c → 404"
            ]
        );
    }

    fn response(content_type: &str, text: &str) -> Response {
        Response {
            status: 200,
            reason: "OK".into(),
            headers: vec![("Content-Type".into(), content_type.into())],
            body: text.as_bytes().to_vec(),
            text: Some(text.into()),
            size: text.len(),
            decoded: None,
            url: "http://x/".into(),
            redirects: Vec::new(),
            millis: 7,
            notes: Vec::new(),
        }
    }

    #[test]
    fn decoded_bodies() {
        let decoded = |text: &str| ferris_idea_http::Decoded {
            codings: vec!["gzip".into()],
            remaining: Vec::new(),
            body: text.as_bytes().to_vec(),
            size: text.len(),
            cut: false,
            damaged: None,
        };
        let mut r = response("application/json", "{\"a\":1}");
        r.headers.push(("Content-Encoding".into(), "gzip".into()));
        r.size = 27;
        r.decoded = Some(decoded("{\"a\":1}"));
        // Shown decoded and indented like any JSON; both sizes in the output line.
        assert!(response_text(&r).ends_with("\n\n{\n  \"a\": 1\n}"));
        let file = ferris_idea_http::parse("GET http://a/\n");
        let req = ferris_idea_http::resolve(&file, &Default::default()).remove(0);
        assert_eq!(
            status_line(&req, &r),
            "GET http://a/ → 200 OK · 7 ms · 27 bytes, 7 decoded from gzip"
        );
        r.notes = vec![r.decoded.as_ref().unwrap().note(27)];
        assert_eq!(
            output_lines(&req, &r, &[])[1],
            (
                "The body was decoded from gzip: 27 bytes received, 7 bytes decoded.".to_owned(),
                false
            )
        );

        // Cut at a limit: what was decoded, then a line that says so.
        let mut cut = response("text/plain", "aaaa");
        cut.size = 20;
        cut.decoded = Some(ferris_idea_http::Decoded {
            cut: true,
            ..decoded("aaaa")
        });
        assert!(
            response_text(&cut).ends_with(
                "\n\naaaa\n\n(cut after 4 decoded bytes: the limit is 100 times the bytes received, and at least 1 MiB)"
            ),
            "{}",
            response_text(&cut)
        );
        let mut damaged = response("text/plain", "aa");
        damaged.decoded = Some(ferris_idea_http::Decoded {
            damaged: Some("it ends too early".into()),
            ..decoded("aa")
        });
        assert!(response_text(&damaged).ends_with(
            "\n\naa\n\n(the coded body is damaged, it ends too early: 2 bytes were decoded before that place)"
        ));

        // A coding under a decoded one that Ferris IDEA does not decode.
        let mut left = response("text/plain", "");
        left.text = None;
        left.size = 30;
        left.decoded = Some(ferris_idea_http::Decoded {
            remaining: vec!["br".into()],
            ..decoded("0123456789")
        });
        assert!(response_text(&left).ends_with("\n\n(10 bytes, content coding br, not decoded)"));
    }

    #[test]
    fn bodies_without_text_and_notes() {
        let mut coded = response("application/json", "");
        coded.headers.push(("Content-Encoding".into(), "br".into()));
        coded.text = None;
        coded.size = 51;
        assert!(response_text(&coded).ends_with("\n\n(51 bytes, content coding br, not decoded)"));
        let mut binary = response("application/octet-stream", "");
        binary.text = None;
        binary.size = 256;
        assert!(response_text(&binary).ends_with("\n\n(256 bytes of binary data)"));
        // A body labelled JSON that is not JSON is shown as it arrived.
        assert!(
            response_text(&response("application/json", "{\"broken\": "))
                .ends_with("\n\n{\"broken\": ")
        );

        let file = ferris_idea_http::parse("GET http://a/\n\n> {% client.log(1); %}\n");
        let req = ferris_idea_http::resolve(&file, &Default::default()).remove(0);
        assert_eq!(
            req.body, "",
            "the handler script is not part of the request"
        );
        let mut r = response("text/plain", "ok");
        r.notes = vec!["Not sent to b:80, another origin than a:80: Authorization.".into()];
        let lines = output_lines(&req, &r, &file.requests[0].notes);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].0, "GET http://a/ → 200 OK · 7 ms · 2 bytes");
        assert!(lines[1].0.contains("not run"));
        assert!(lines[2].0.contains("Authorization"));
        assert!(lines.iter().all(|(_, red)| !red), "notes are not errors");
    }

    #[test]
    fn request_at_the_caret_and_response_texts() {
        let file = ferris_idea_http::parse("GET http://a/\n\n###\nPOST http://b/\n\n{}\n");
        assert_eq!(request_at(&file, 1), Some(0));
        assert_eq!(request_at(&file, 2), Some(0));
        assert_eq!(
            request_at(&file, 6),
            Some(1),
            "inside the body of the second"
        );
        assert_eq!(request_at(&ferris_idea_http::parse(""), 1), None);
        let r = response("application/json", "{\"a\":1}");
        assert_eq!(
            response_text(&r),
            "HTTP 200 OK\nContent-Type: application/json\n\n{\n  \"a\": 1\n}"
        );
        assert!(response_text(&response("text/plain", "{\"a\":1}")).ends_with("\n\n{\"a\":1}"));
        let req = ferris_idea_http::resolve(&file, &Default::default()).remove(0);
        assert_eq!(
            status_line(&req, &r),
            "GET http://a/ → 200 OK · 7 ms · 7 bytes"
        );
    }

    /// SPEC-0096 v4 AK-6: an error status is named in words and is no failure.
    #[test]
    fn error_statuses_are_named_not_failed() {
        let file = ferris_idea_http::parse("GET http://a/\n");
        let req = ferris_idea_http::resolve(&file, &Default::default()).remove(0);
        let mut r = response("text/plain", "gone");
        r.status = 404;
        r.reason = "Not Found".into();
        assert_eq!(
            status_line(&req, &r),
            "GET http://a/ → 404 Not Found · client error · 7 ms · 4 bytes"
        );
        assert!(
            output_lines(&req, &r, &[])
                .iter()
                .all(|(_, failed)| !failed)
        );
        r.status = 503;
        r.reason = "Service Unavailable".into();
        assert!(status_line(&req, &r).contains("503 Service Unavailable · server error ·"));
        assert_eq!(status_class(200), None);
        assert_eq!(status_class(302), None);
        assert_eq!(status_class(499), Some("client error"));
        assert_eq!(status_class(500), Some("server error"));
    }

    /// SPEC-0096 v4 AK-5: one mark per request at its request line; the request a mark sends is
    /// the one at that line.
    #[test]
    fn run_marks_stand_at_the_request_lines() {
        let text = "@host = http://a\n# GET http://not-a-request/\n\n### one\nGET {{host}}/x\nAccept: */*\n\n{\"GET\": 1}\n\n### two\n\nPOST http://b/\n";
        let lines = run_lines(text);
        assert_eq!(lines, vec![4, 11]);
        let file = ferris_idea_http::parse(text);
        assert_eq!(request_at(&file, lines[1] + 1), Some(1));
        assert_eq!(request_at(&file, lines[0] + 1), Some(0));
        assert!(run_lines("# only a comment\n\n").is_empty());
        assert!(run_lines("").is_empty());
    }

    /// SPEC-0096 v4 AK-4: a JSON body gets JSON's classes, the variables in it stay whole, the
    /// parts outside the body keep theirs; no two spans overlap.
    #[test]
    fn a_json_body_is_coloured_as_json() {
        let text = "POST http://a/ HTTP/1.1\nContent-Type: application/json\n\n{\"n\": 12, \"id\": \"{{id}}\"}\n";
        let found = spans(text);
        let chars: Vec<char> = text.chars().collect();
        let of = |kind: &str| -> Vec<String> {
            found
                .iter()
                .filter(|s| s.kind.starts_with(kind))
                .map(|s| chars[s.start..s.end].iter().collect())
                .collect()
        };
        assert!(of("keyword").contains(&"POST".to_owned()), "{found:?}");
        assert!(
            of("property").contains(&"Content-Type".to_owned()),
            "{found:?}"
        );
        assert!(of("number").contains(&"12".to_owned()), "{found:?}");
        assert!(of("constant").contains(&"{{id}}".to_owned()), "{found:?}");
        assert!(
            found.windows(2).all(|w| w[0].end <= w[1].start),
            "{found:?}"
        );
    }
}
