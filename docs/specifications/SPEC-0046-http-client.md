# SPEC-0046: HTTP client — request files and sending

- **Version:** 6 · **Date:** 2026-11-16 (v2: sending through MCP with SPEC-0053; v3 after the headless
  check against a local test server, `crates/http/tests/precheck/`: redirects by RFC 3986
  that keep credentials on their origin, request checks, parts that are never sent, request settings,
  dynamic variables, bodies from files; behaviours 2–5 changed, 7–11 new; v4: content codings are
  decoded within limits, behaviours 4–6 changed, 12 new, AK-11 new; v5: a URL without a scheme is sent
  over http, behaviour 4, AK-12; v6 after OBS-0061 W1: a placeholder with spaces stays one piece of
  the request line, a `{{` that is not closed is unresolved, dynamic values with arguments,
  behaviours 2 and 3, AK-13 and AK-14)
- **Sources:** GEN (IDEs send HTTP requests written in plain text files with variables and show status,
  headers, body and timing; F-079), STD (HTTP/1.1 message syntax RFC 9112, semantics RFC 9110: methods,
  `Host`, `Content-Length`, chunked transfer coding, redirects 301/302/303/307/308 and the advice to
  drop credentials on a redirect to another origin (§15.4), tokens (§5.6.2), no fragment in the request
  target (RFC 9112 §3.2), content codings and `Accept-Encoding` (RFC 9110 §8.4 and §12.5.3), the gzip
  format RFC 1952, the zlib format RFC 1950 and the deflate format RFC 1951; reference resolution RFC 3986 §5; `multipart/form-data` RFC 7578 with CRLF
  line ends (RFC 2046 §5.1.1); UUID version 4 RFC 9562; TLS through the system library via
  `native-tls`), OSS (the open `.http` request file convention as documented by the MIT-licensed REST
  Client project: `###` separators, `@name = value` variables, `{{name}}` references, `# @no-redirect`,
  `< path` bodies, generated values such as `{{$timestamp}}`), DOC (request files of other IDEs also
  carry response handler scripts after `>` and response targets after `>>`; facts only); v5: OSS
  (request files in `JetBrains/intellij-community` at `d4018fb`, Apache-2.0, read as data only:
  `platform/built-in-server/resources/org/jetbrains/ide/ToolboxUpdates.http` writes
  `POST 127.0.0.1:63344/api/…` without a scheme; `plugins/tasks/tests/base/requests/youtrack.http`
  uses `### title` separators and `{{name}}` from `http-client.env.json`); v6: DOC (the public
  documentation of the reference's HTTP client lists the dynamic values `$random.integer(from, to)`
  and `$random.float(from, to)` with `from` included and `to` not, `$random.alphabetic(n)`,
  `$random.alphanumeric(n)` with letters, digits and underscores, `$random.hexadecimal(n)` and
  `$random.email`; facts only), STD (RFC 2606: `example.com` is reserved for examples), OBS-0061 W1
- **Feature matrix rows:** F-079

## Purpose
Write HTTP requests in a text file next to the code and send them from the IDE.

## Behaviour
1. A request file holds requests separated by lines starting with `###` (text after it names the request;
   `# @name x` also names the following request). `#` and `//` lines are comments. `@name = value` lines
   define variables (later definitions win). [OSS]
2. A request is a request line `METHOD URL [HTTP/1.1]` (a bare URL means GET; lines starting with `?` or `&`
   right after it continue the URL), header lines `Name: value` until an empty line, then the body until
   the next separator (trailing empty and comment lines removed). A header-section line that starts with
   white space continues the value of the header before it, joined with one space; a line there that is
   neither a header, a comment nor a continuation is named in the request's notes instead of being
   dropped silently. In a body of type `application/x-www-form-urlencoded` the line breaks between the
   written lines are removed, so a form can be written one field per line. [OSS, STD, GEN]
   **v6:** placeholders are found before the request line is split: the line breaks into method,
   URL and version only at white space outside `{{…}}`, so `{{$random.integer(1, 5)}}` stays whole
   in the URL. The URL runs from the piece after the method to the version at the end of the line
   (or to the end of the line), so white space left in it outside placeholders is not dropped but
   makes the request refused (behaviour 8). A `{{` without `}}` keeps the rest of its line in the URL.
   Until v5 the line was split at every space, and the part of the URL after a space inside a
   placeholder was left out without a word. [GEN, OBS-0061 W1]
3. `{{name}}` is replaced from the file's variables, then from the environment given by the caller (e.g. a
   profile like `dev`); variables may refer to other variables. A request with unresolved names is not sent;
   the names are listed. Names starting with `$` that neither the file nor the environment defines are
   generated for each use: `$uuid` and `$random.uuid` (a random UUID, version 4), `$timestamp` (seconds
   since 1970-01-01 UTC), `$isoTimestamp` (UTC date and time such as `2026-11-13T09:30:00Z`) and
   `$randomInt` (0 to 1000). Any other `$` name is unresolved. [OSS, STD, improvement: never send a
   literal `{{name}}`]
   **v6 – values with arguments.** Arguments stand in parentheses after the name, separated by
   commas; spaces around them do not count, and the whole text between `{{` and `}}` is the name
   (`{{ $random.integer(1, 5) }}`). Generated for each use:
   - `$random.integer(from, to)`: a whole number from `from` up to but not including `to`;
     `$random.integer` and `$random.integer()` give 0 to 999;
   - `$random.float(from, to)`: a decimal number from `from` up to but not including `to`; without
     arguments from 0 up to 1000;
   - `$random.alphabetic(n)`: `n` letters `A`–`Z` and `a`–`z`; `$random.alphanumeric(n)`: `n`
     letters, digits and underscores; `$random.hexadecimal(n)`: `n` digits `0`–`9` and `a`–`f`;
     `n` is 1 to 10,000, so a slip of the finger cannot fill the memory;
   - `$random.email`: an address of 10 lowercase letters and digits at `example.com`, a domain
     reserved for examples (RFC 2606), so a generated address never belongs to a real person;
   - `$random.uuid`, `$timestamp` and `$isoTimestamp` as above.
   Arguments that do not fit — not numbers, too many or too few, `from` not below `to`, `n` outside
   1 to 10,000 — leave the name unresolved, as does any other `$` name. A `{{` that is not closed by
   `}}` is unresolved too, named by itself and the rest of its line (`{{$random.integer(1,`).
   Every unresolved name keeps its request from being sent, wherever it stands (URL, header, body)
   and however it is run (⌘↩, a run mark, Run All, `send_http_request`); the names are given.
   [DOC, STD, GEN, OBS-0061 W1, security first]
4. Sending supports `http` and `https`. **v5:** a URL that names no scheme once its variables are
   resolved (`127.0.0.1:63344/api`, `{{host}}/users` with `host = localhost:8080`) is sent over
   `http://`; a URL with any scheme keeps it, so `https` is never dropped and `ftp://…` is still
   refused, and one with unresolved names stays as written. Until v4 such a URL was refused as bad.
   [OSS] `Host`, `Content-Length` (when there is a body, and `0` for a POST,
   PUT or PATCH without one), `Connection: close` and `User-Agent: IDECraft` are added unless given. A request sent from a
   request file also gets `Accept-Encoding: gzip, deflate`, unless the request sets `Accept-Encoding`
   itself (then exactly what it says is sent) or carries a `Range` header (a part of a coded body
   cannot be decoded). The body of a request is never compressed by IDECraft. A
   fragment (`#…`) of the URL is not sent. Responses with `Content-Length`, chunked transfer coding or
   close-delimited bodies are read; HEAD, 204 and 304 have no body; 1xx responses are skipped. A timeout
   (default 30 s) applies to connecting and reading; a request that runs into it fails with a message
   that starts with "timed out". [STD]
5. The result has status code and reason, headers in order, body (the bytes as they arrived and their
   number; text when the body, after decoding its content codings by behaviour 12, is UTF-8 or when the
   content type names the charset `iso-8859-1`), the decoded body with its size when a content coding
   was undone, the final URL, the redirects followed, the time taken in milliseconds and notes: one
   sentence for each thing the client did that the request file does not show (credentials kept back
   on a redirect, a redirect not followed and why, a body that was decoded, cut or damaged). A body
   whose coding IDECraft does not decode has no text. [GEN, STD]
6. MCP tool `http_requests` (path, optional environment) listing the file's requests with resolved method,
   URL, headers and body, their settings and notes and any unresolved names; MCP tool `send_http_request`
   sending one (dry run by default, with bodies from files read in, a refused file reported as
   `not_sent`) under the `http` right of SPEC-0053; its answer carries the notes of the request file and
   of the response, `size` (bytes as they arrived) and `decoded`: `null`, or the codings undone, the
   codings left, the decoded size, whether the body was cut and what is damaged (behaviour 12); `body`
   is the decoded text. [ADR-0008]
7. **Redirects.** 301, 302, 303, 307 and 308 with a `Location` are followed up to 5 times (303, and
   301/302 for POST, switch to GET without body and without `Content-Type` and `Content-Length`). The
   location is resolved against the URL that was asked (RFC 3986 §5: absolute, `//host/path`, `/path`,
   `path`, `../path`, `?query`; dot segments removed, fragment dropped). Security rules, which no request
   file and no setting can switch off:
   - A redirect to another origin (scheme, host or port differ) is followed without the request's
     `Authorization`, `Proxy-Authorization`, `Cookie` and hand-written `Host` headers; they stay away
     for the rest of the chain, also when it returns to the first origin. A note names the headers and
     the origin.
   - A redirect from `https` to `http` is not followed: the 3xx response is the result, with a note.
   - A location with any other scheme (`file:`, `ftp:`, `javascript:` …) is not followed; note.
   - After the fifth redirect the next 3xx response is the result, with a note that the limit was
     reached. A 3xx response without `Location` is the result as it is.
   [STD, security first]
8. **Request checks.** Before anything is sent the request is refused, naming the part, when the method
   is not a token, the URL contains white space or control characters, a header name is not a token, or
   a header value contains CR, LF or NUL. Values come from variables and environment files, so a value
   must never be able to add a header line or a second request. [STD, security first]
9. **Parts that are never sent, run or written.** At the end of a request, after its body: a response
   handler (`> {% … %}` over one or several lines, or `> path` naming a script file), a response target
   (`>> path`, `>>! path`) and a stored-response reference (`<> path`). They are taken out of the body
   and listed in the request's notes as not supported. IDECraft never runs code from a request file or
   from a response, never writes a response to a file and never hands a response to another program.
   [DOC, GEN, security first]
10. **Request settings.** Comment lines between the separator and the request line: `# @no-redirect`
    (the first response is the result, also a 3xx) and `# @timeout N` (N seconds for connecting and for
    each read, 1 to 600, instead of 30). A value that is not such a number is named in the notes and the
    default stays. Other `# @word` lines are comments. [OSS, GEN]
11. **Bodies from files.** A body line `< path` is replaced by the content of that file when the request
    is sent (and in the MCP dry run): a body that is only this line is the file exactly; inside a larger
    body (a multipart part) the content replaces the line, without the file's last line break. Rules:
    the path is relative to the folder of the request file (variables are resolved first); absolute
    paths and `~` are refused; after resolving links the file must lie inside the project folder; it
    must be a regular file of at most 16 MiB of UTF-8 text; the private environment file
    (`http-client.private.env.json`) and files under `.git/` and `.idecraft/` are refused. Variables
    inside the file are not replaced. A refused or missing file means the request is not sent; the
    message names the path and the rule. A body of type `multipart/…` is sent with CRLF line ends
    (RFC 2046) for the lines written in the request file and ends with CRLF; file content is not
    changed. [OSS, STD, security first]
12. **Content codings.** A response body with `Content-Encoding` is decoded for the text of the result
    (RFC 9110 §8.4):
    - `gzip` and `x-gzip` (RFC 1952, also several members one after another) and `deflate` (the zlib
      format of RFC 1950; a body that does not start with a zlib header is read as the bare deflate
      stream of RFC 1951, which some servers send). `identity` changes nothing. Names are compared
      without regard to case.
    - Several codings, in one header or in several, are undone from the last one listed to the first,
      the reverse of the order in which the server applied them.
    - Any other coding (`br`, `zstd`, `compress` …) is not decoded: decoding stops there, the body
      has no text and is described by its size and the codings left. Codings listed after it are
      undone first.
    - An empty body (HEAD, 204, 304, `Content-Length: 0`) is not decoded and gets no note, whatever
      the header says.
    - **Limits**, checked while decoding, piece by piece, so that a small response cannot fill the
      memory (decompression bomb): at most 16 MiB of decoded bytes, and at most 100 times the number
      of bytes that arrived, where 1 MiB is always allowed. With several codings both limits hold for
      the output of each of them, the ratio always against the bytes that arrived. At a limit the
      decoded body is cut there, marked as cut, and a note names the limit; this is not an error.
    - A stream that is damaged or ends too early gives what was decoded up to that place, marked as
      damaged, and a note; this is not an error either. Text cut or damaged in the middle of a
      character ends before that character.
    - The bytes as they arrived stay in the result unchanged beside the decoded ones, and `size`
      counts them. No request file and no setting changes the limits.
    - A note says from which codings the body was decoded and gives both sizes ("The body was decoded
      from gzip: 51 bytes received, 232 bytes decoded.").
    [STD, security first]

## Acceptance criteria
- **AK-1** Parsing: separators and names, comments, variables, bare URL, continued query lines, headers,
  body, several requests.
- **AK-2** Variables: file and environment, nesting, precedence, unresolved names listed.
- **AK-3** Against a local test server: GET with Content-Length, POST with body and added headers, chunked
  response, redirect chain (303 to GET), 404, timeout on a server that never answers.
- **AK-4** The MCP tool lists resolved requests and unresolved names.
- **AK-5** Redirects: `../path`, `//host/path` and `?query` locations resolve by RFC 3986; a redirect to
  another loopback host name arrives without `Authorization`, `Proxy-Authorization` and `Cookie` and
  with a note, one to the same origin keeps them; a `file:` location is not followed; a loop ends after
  five redirects with a note; `# @no-redirect` returns the 3xx response.
- **AK-6** Request checks: a variable value with a line break in a header or in the URL is refused and
  nothing reaches the server; a fragment is not sent; a POST without body carries `Content-Length: 0`.
- **AK-7** Never sent: a response handler and a response target after a request reach no server and are
  listed in the notes; no file is written.
- **AK-8** Bodies from files: a file beside the request file and one in a sub-folder are sent exactly;
  a path leaving the project, an absolute path, a link pointing outside, the private environment file
  and a missing file are refused and nothing is sent; a multipart body arrives with CRLF line ends and
  the file content unchanged.
- **AK-9** Dynamic variables: `$uuid` has the form of a version 4 UUID and differs between uses,
  `$timestamp` is the current second, `$isoTimestamp` a UTC date and time, `$randomInt` a number from 0
  to 1000; an unknown `$name` is unresolved. Settings: `# @timeout 1` ends a request to a server that
  answers after three seconds; a continued header line and a form body over several lines arrive joined.
- **AK-10** The request files of `crates/http/tests/precheck/requests/` run against
  `server.py` of that folder (loopback only) with the outcome each request states.
- **AK-11** Content codings against a local test server: gzip, x-gzip, deflate in the zlib form and as
  a bare stream, two codings on one body, `identity`, an unknown coding alone and around a known one,
  a gzip stream that ends too early (the decoded part and a note), a body that would decode to more
  than the size limit and one beyond the ratio (cut, marked, memory bounded), an empty body and a HEAD
  response with the header set; `Accept-Encoding: gzip, deflate` arrives when the request does not set
  the header, the request's own value arrives alone, none with `Range`.

- **AK-12 (v5, behaviour 4)** Unit test `file::tests::a_url_without_a_scheme_is_http`: a bare
  `host:port/path` and a host from a variable get `http://`, an `https` URL keeps its scheme, `ftp://`
  stays, an unresolved URL stays as written; the window test of SPEC-0096 v5 AK-8 sends such a URL to
  a local server.
- **AK-13 (v6, behaviour 2)** Unit test `file::tests::placeholders_with_spaces_stay_whole`: the URL
  of `GET http://{{host}}/d?r={{$random.integer(1, 5)}} HTTP/1.1` keeps the whole placeholder and the
  version is not part of it; `{{ $uuid }}` with spaces, a bare URL with a placeholder, a URL with a
  space outside placeholders (kept whole and refused by the request check) and a `{{` that is not
  closed.
- **AK-14 (v6, behaviour 3)** Unit test `file::tests::random_values_with_arguments`: every value of
  behaviour 3 v6 in its range and alphabet, `to` never reached, the defaults without arguments, the
  address at `example.com`, arguments that do not fit, an unknown name with a space and an unclosed
  `{{` unresolved. Against a local test server (`crates/http/tests/server.rs`:
  `unresolved_placeholders_are_never_sent`): a request with `{{$foo bar}}` or an unclosed `{{` is
  refused and nothing reaches the server; `$random.integer(1, 5)` arrives whole as a number from 1
  to 4, also among the other dynamic values of OBS-0061.

## Out of scope
HTTP/2, binary request bodies from files, variables inside body files, multipart helpers, running
response handler scripts, cookies across requests (no cookie jar: the `Set-Cookie` of one response is not
sent with the next request), authentication helpers, saving responses, proxies, the content codings
`br`, `zstd` and `compress`, compressing request bodies, a limit on the size of a response body as it
arrives (the limits of behaviour 12 hold for decoding only), a rule for requests to link-local, private or cloud metadata
addresses (a person's request file may name any host; agents need the `http` right, SPEC-0053).

## Known deviations from the reference
Not compared side by side yet. Response handler scripts and response targets in request files are
recognised and left out instead of being run or written (behaviour 9). Credentials are never forwarded to
another origin and a redirect never leaves TLS (behaviour 7), whatever the reference does.
