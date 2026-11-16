//! The colours of a request file (`SPEC-0096` v4 behaviour 5): token classes by line, read as
//! [`crate::parse`] reads the file. Offsets are in characters, as the editor counts them.

use std::ops::Range;

/// The classes of a request file and where its bodies are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Highlight {
    /// Coloured parts in file order, with the class names of the other languages (`keyword`,
    /// `string`, `property`, `comment`, `attribute`, `constant`, `function.builtin`). They do not
    /// overlap.
    pub spans: Vec<(Range<usize>, &'static str)>,
    /// Each request body's text and whether it is JSON; its `{{…}}` are among `spans`.
    pub bodies: Vec<(Range<usize>, bool)>,
}

const METHODS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Before a request line: separators, comments, settings, variables.
    Between,
    /// After the request line, until the first blank line.
    Headers,
    /// After the blank line, until the next separator.
    Body,
    /// Inside a response handler script `> {% … %}`.
    Handler,
}

fn is_comment(t: &str) -> bool {
    (t.starts_with('#') && !t.starts_with("###")) || t.starts_with("//")
}

/// A response handler or response target line (`SPEC-0046` behaviour 9).
fn is_handler(t: &str) -> bool {
    t.starts_with("> ") || t.starts_with(">> ") || t.starts_with(">>! ") || t.starts_with("<> ")
}

/// Character offsets of the parts of `line` (starting at character `at`).
struct Line<'a> {
    text: &'a str,
    at: usize,
}

impl Line<'_> {
    /// Characters from byte `start` to byte `end` of the line, as file offsets.
    fn chars(&self, start: usize, end: usize) -> Range<usize> {
        let a = self.at + self.text[..start].chars().count();
        a..a + self.text[start..end].chars().count()
    }

    /// The trimmed line as a range.
    fn trimmed(&self) -> Range<usize> {
        let start = self.text.len() - self.text.trim_start().len();
        self.chars(start, self.text.trim_end().len())
    }
}

/// `{{name}}` and `{{$dynamic}}` in bytes `start..end` of `line`, and the parts between them
/// (bytes) for the caller's class.
fn templates(
    line: &Line,
    start: usize,
    end: usize,
    out: &mut Vec<(Range<usize>, &'static str)>,
    between: Option<&'static str>,
) {
    let text = &line.text[..end];
    let mut from = start;
    let mut at = start;
    while let Some(open) = text[at..].find("{{") {
        let open = at + open;
        let Some(close) = text[open + 2..].find("}}") else {
            break;
        };
        let close = open + 2 + close + 2;
        if let Some(kind) = between
            && open > from
        {
            out.push((line.chars(from, open), kind));
        }
        let inner = text[open + 2..close - 2].trim();
        let kind = if inner.starts_with('$') {
            "function.builtin"
        } else {
            "constant"
        };
        out.push((line.chars(open, close), kind));
        from = close;
        at = close;
    }
    if let Some(kind) = between
        && end > from
    {
        out.push((line.chars(from, end), kind));
    }
}

/// The value part of `text` from byte `start`: leading spaces skipped.
fn skip_spaces(text: &str, start: usize) -> usize {
    start + (text[start..].len() - text[start..].trim_start().len())
}

/// Colours `text` (behaviour 5).
pub fn highlight(text: &str) -> Highlight {
    let mut out = Highlight::default();
    let mut part = Part::Between;
    let mut json = false;
    // The body being read: its first and last character with text.
    let mut body: Option<Range<usize>> = None;
    let mut at = 0;
    let finish = |body: &mut Option<Range<usize>>, json: bool, out: &mut Highlight| {
        if let Some(b) = body.take() {
            out.bodies.push((b, json));
        }
    };
    for raw in text.split_inclusive('\n') {
        let content = raw.strip_suffix('\n').unwrap_or(raw);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let line = Line { text: content, at };
        at += raw.chars().count();
        let t = content.trim();
        if t.starts_with("###") {
            finish(&mut body, json, &mut out);
            out.spans.push((line.trimmed(), "comment"));
            part = Part::Between;
            continue;
        }
        match part {
            Part::Between => {
                if t.is_empty() {
                    continue;
                }
                if is_comment(t) {
                    out.spans.push((line.trimmed(), "comment"));
                    continue;
                }
                if let Some(rest) = t.strip_prefix('@')
                    && let Some((name, _)) = rest.split_once('=')
                    && !name.trim().is_empty()
                {
                    let start = content.find('@').unwrap_or(0);
                    let name_end = start + 1 + name.trim_end().len();
                    out.spans.push((line.chars(start, name_end), "attribute"));
                    let eq = content[start..]
                        .find('=')
                        .map_or(content.len(), |e| start + e + 1);
                    let value = skip_spaces(content, eq);
                    templates(
                        &line,
                        value,
                        content.trim_end().len(),
                        &mut out.spans,
                        Some("string"),
                    );
                    continue;
                }
                // The request line: method, URL, version, split as `parse` splits it (a
                // placeholder with spaces is one piece, SPEC-0046 v6 behaviour 2).
                let words: Vec<(usize, usize)> = crate::file::request_line_words(content)
                    .into_iter()
                    .map(|r| (r.start, r.end))
                    .collect();
                let mut rest = words.as_slice();
                if let Some(&(s, e)) = rest.first()
                    && METHODS.contains(&content[s..e].to_ascii_uppercase().as_str())
                {
                    out.spans.push((line.chars(s, e), "keyword"));
                    rest = &rest[1..];
                }
                if let [(s, e), tail @ ..] = rest {
                    let (version, url_end) = match tail.last() {
                        Some(&(vs, ve)) if content[vs..ve].starts_with("HTTP/") => {
                            (Some((vs, ve)), tail.len() - 1)
                        }
                        _ => (None, tail.len()),
                    };
                    let end = if url_end == 0 {
                        *e
                    } else {
                        tail[url_end - 1].1
                    };
                    templates(&line, *s, end, &mut out.spans, Some("string"));
                    if let Some((vs, ve)) = version {
                        out.spans.push((line.chars(vs, ve), "keyword"));
                    }
                }
                part = Part::Headers;
                json = false;
            }
            Part::Headers => {
                if t.is_empty() {
                    part = Part::Body;
                    continue;
                }
                if is_handler(t) {
                    part = Part::Body;
                } else {
                    if is_comment(t) {
                        out.spans.push((line.trimmed(), "comment"));
                    } else if t.starts_with('?') || t.starts_with('&') {
                        let s = content.len() - content.trim_start().len();
                        templates(
                            &line,
                            s,
                            content.trim_end().len(),
                            &mut out.spans,
                            Some("string"),
                        );
                    } else if let Some(colon) = content.find(':') {
                        let s = content.len() - content.trim_start().len();
                        let name = content[..colon].trim_end();
                        if name.len() > s {
                            out.spans.push((line.chars(s, name.len()), "property"));
                        }
                        if name.trim().eq_ignore_ascii_case("content-type")
                            && content[colon + 1..].to_ascii_lowercase().contains("json")
                        {
                            json = true;
                        }
                        let v = skip_spaces(content, colon + 1);
                        templates(
                            &line,
                            v,
                            content.trim_end().len(),
                            &mut out.spans,
                            Some("string"),
                        );
                    } else {
                        // A continued header value.
                        let s = content.len() - content.trim_start().len();
                        templates(
                            &line,
                            s,
                            content.trim_end().len(),
                            &mut out.spans,
                            Some("string"),
                        );
                    }
                    continue;
                }
                // A handler right after the headers is read as in the body.
                body_line(&line, t, &mut part, &mut body, &mut json, &mut out);
            }
            Part::Body | Part::Handler => {
                body_line(&line, t, &mut part, &mut body, &mut json, &mut out);
            }
        }
    }
    finish(&mut body, json, &mut out);
    out
}

/// One line of a body: a handler, a file reference, a comment or body text.
fn body_line(
    line: &Line,
    t: &str,
    part: &mut Part,
    body: &mut Option<Range<usize>>,
    json: &mut bool,
    out: &mut Highlight,
) {
    if *part == Part::Handler {
        out.spans.push((line.trimmed(), "comment"));
        if t.contains("%}") {
            *part = Part::Body;
        }
        return;
    }
    if t.is_empty() {
        return;
    }
    if is_handler(t) {
        out.spans.push((line.trimmed(), "comment"));
        if t.starts_with("> {%") && !t.contains("%}") {
            *part = Part::Handler;
        }
        return;
    }
    if is_comment(t) && body.is_none() {
        out.spans.push((line.trimmed(), "comment"));
        return;
    }
    if t.starts_with("< ") {
        out.spans.push((line.trimmed(), "attribute"));
        return;
    }
    let s = line.text.len() - line.text.trim_start().len();
    if body.is_none() && (t.starts_with('{') || t.starts_with('[')) {
        *json = true;
    }
    let range = line.chars(s, line.text.trim_end().len());
    *body = Some(match body.take() {
        Some(b) => b.start..range.end,
        None => range,
    });
    templates(line, s, line.text.trim_end().len(), &mut out.spans, None);
}

/// `spans` without the parts that `over` covers, then `over`, in order: the classes of an embedded
/// part (JSON) with the `{{…}}` inside it kept whole.
pub fn overlay(
    spans: Vec<(Range<usize>, String)>,
    over: &[(Range<usize>, &'static str)],
) -> Vec<(Range<usize>, String)> {
    let mut out: Vec<(Range<usize>, String)> = Vec::new();
    for (range, kind) in spans {
        let mut pieces = vec![range];
        for (o, _) in over {
            pieces = pieces
                .into_iter()
                .flat_map(|p| {
                    if o.end <= p.start || o.start >= p.end {
                        vec![p]
                    } else {
                        let mut v = Vec::new();
                        if o.start > p.start {
                            v.push(p.start..o.start);
                        }
                        if o.end < p.end {
                            v.push(o.end..p.end);
                        }
                        v
                    }
                })
                .collect();
        }
        out.extend(pieces.into_iter().map(|p| (p, kind.clone())));
    }
    out.extend(over.iter().map(|(r, k)| (r.clone(), (*k).to_owned())));
    out.sort_by_key(|(r, _)| (r.start, r.end));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text and class of each span.
    fn classes(text: &str) -> Vec<(String, &'static str)> {
        let chars: Vec<char> = text.chars().collect();
        highlight(text)
            .spans
            .into_iter()
            .map(|(r, k)| (chars[r].iter().collect(), k))
            .collect()
    }

    fn has(found: &[(String, &'static str)], text: &str, class: &str) -> bool {
        found.iter().any(|(t, k)| t == text && *k == class)
    }

    /// SPEC-0096 v4 AK-4: every part of behaviour 5 gets its class at its place.
    #[test]
    fn the_parts_of_a_request_file_get_their_classes() {
        let text = "@host = https://api.example\n\
                    # A comment\n\
                    ### Create a user\n\
                    // @no-redirect\n\
                    POST {{host}}/users?id={{$uuid}} HTTP/1.1\n\
                    Content-Type: application/json\n\
                    Authorization: Bearer {{token}}\n\
                    \n\
                    {\"name\": \"Ada\", \"id\": \"{{$uuid}}\"}\n\
                    \n\
                    > {%\n\
                        client.global.set(\"x\", 1);\n\
                    %}\n\
                    >> out.json\n\
                    ###\n\
                    GET https://ä.example/ü\n\
                    \n\
                    < ./body.txt\n";
        let found = classes(text);
        for (part, class) in [
            ("@host", "attribute"),
            ("https://api.example", "string"),
            ("# A comment", "comment"),
            ("### Create a user", "comment"),
            ("// @no-redirect", "comment"),
            ("POST", "keyword"),
            ("{{host}}", "constant"),
            ("/users?id=", "string"),
            ("{{$uuid}}", "function.builtin"),
            ("HTTP/1.1", "keyword"),
            ("Content-Type", "property"),
            ("application/json", "string"),
            ("Authorization", "property"),
            ("Bearer ", "string"),
            ("{{token}}", "constant"),
            ("> {%", "comment"),
            ("client.global.set(\"x\", 1);", "comment"),
            ("%}", "comment"),
            (">> out.json", "comment"),
            ("###", "comment"),
            ("GET", "keyword"),
            ("https://ä.example/ü", "string"),
            ("< ./body.txt", "attribute"),
        ] {
            assert!(has(&found, part, class), "{part} as {class}: {found:?}");
        }
        // The JSON body is a body of its own, the variable in it is coloured.
        let h = highlight(text);
        let chars: Vec<char> = text.chars().collect();
        let (first, json) = &h.bodies[0];
        assert!(*json);
        assert_eq!(
            chars[first.clone()].iter().collect::<String>(),
            "{\"name\": \"Ada\", \"id\": \"{{$uuid}}\"}"
        );
        assert_eq!(found.iter().filter(|(t, _)| t == "{{$uuid}}").count(), 2);
        // Spans do not overlap and come in order.
        assert!(
            h.spans.windows(2).all(|w| w[0].0.end <= w[1].0.start),
            "{:?}",
            h.spans
        );
    }

    /// SPEC-0096 v4 AK-4: a text body is plain, a body that starts with `{` is JSON without a
    /// header, a URL without a method is a GET.
    #[test]
    fn bodies_are_json_or_text() {
        let h = highlight(
            "POST https://x/\nContent-Type: text/plain\n\nhello {{name}}\nworld\n###\nhttps://y/\n\n[1, 2]\n",
        );
        assert_eq!(h.bodies.len(), 2);
        assert!(!h.bodies[0].1);
        assert!(h.bodies[1].1);
        let found = classes("https://y/ HTTP/2\n");
        assert!(has(&found, "https://y/", "string"));
        assert!(has(&found, "HTTP/2", "keyword"));
    }

    /// The parts of JSON that a `{{…}}` covers give way to it.
    #[test]
    fn templates_are_laid_over_json() {
        let json = vec![
            (0..10, "string".to_owned()),
            (11..12, "punctuation".to_owned()),
        ];
        let laid = overlay(json, &[(3..7, "constant")]);
        assert_eq!(
            laid,
            vec![
                (0..3, "string".to_owned()),
                (3..7, "constant".to_owned()),
                (7..10, "string".to_owned()),
                (11..12, "punctuation".to_owned()),
            ]
        );
    }

    /// SPEC-0096 v4 AK-4: 2,000 requests are coloured quickly (the budget is checked in a release
    /// build; here only that it ends).
    #[test]
    fn a_long_file_is_coloured() {
        let text = "### r\nPOST https://x/{{id}}\nContent-Type: application/json\n\n{\"a\": 1}\n\n"
            .repeat(2000);
        let h = highlight(&text);
        assert_eq!(h.bodies.len(), 2000);
    }
}
