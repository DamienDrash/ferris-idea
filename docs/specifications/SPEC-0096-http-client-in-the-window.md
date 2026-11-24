# SPEC-0096: HTTP client in the window

- **Version:** 7 · **Date:** 2026-11-16 (v7 after OBS-0061 H2, H3: a single request that
  cannot go is named in red in Output too, Run All names a skipped request at its place in the file,
  behaviour 9, AK-11 new); 2026-11-16 (v6 after OBS-0061 W1–W3: a placeholder with spaces and
  every other unresolved name keep the request back, also in Run All (behaviour 2); the environment
  is asked for (SPEC-0102 v5); Run All Requests in File in the editor's menu of a request file
  (behaviour 8); AK-8 changed, AK-10 new; v5: Run All Requests in File, behaviour 8; the window test
  against a local server, AK-8, closes AK-2 and the click on a mark of AK-7 without a screen; v4:
  request files are coloured, each request has a run mark in
  the gutter, the run action has a title in menus and Find Action, an error status is named in words,
  behaviours 5 to 7 and 3; v2 with SPEC-0046 v3: settings, bodies from files and notes in
  the window; a coded body and a broken JSON body are described as what they are; v3 with SPEC-0046
  v4: a body with a content coding is shown decoded, behaviour 3 and AK-1 changed)
- **Sources:** GEN (requests in `.http` files run from the editor; the response with status, headers,
  body and timing appears in the Services tool window, JSON formatted; F-079), OSS (action ids
  `Console.Execute.Multiline` and `ActivateServicesToolWindow`), SPEC-0046 (request files and sending),
  SPEC-0093 (Services tool window); v5: GEN (request files are often run as a whole, one request
  after another, e.g. to set up and then read), SPEC-0046 v5; v6: OBS-0061 (W1–W3), SPEC-0046 v6,
  SPEC-0102 v5, SPEC-0004 (the editor's menu)
- **Feature matrix rows:** F-079

## Purpose
Try an HTTP endpoint from a request file and read the answer without leaving the IDE.

## Behaviour
1. **Run:** in an editor on a `.http` or `.rest` file, `Console.Execute.Multiline` (⌘↩ on macOS,
   Ctrl+Enter elsewhere) sends the request the caret is in: the last request starting on or before the
   caret's line (its headers and body included), else the first. A file without requests leaves "No
   request in this file" in the status bar. [GEN]
2. **Variables:** `@name = value` variables of the file are substituted (SPEC-0046). A request with
   unresolved variables is not sent; the status bar names them. [GEN] **v6:** this holds for every
   name SPEC-0046 v6 leaves unresolved — an unknown `$` name, also one with spaces
   (`{{$foo bar}}`), arguments that do not fit, a `{{` that is not closed — and for ⌘↩, a run mark
   and Run All alike; a placeholder with spaces in the URL is never cut at a space and sent in part.
   The environment a request runs in is chosen by SPEC-0102 v5 behaviour 2: with several
   environments and none chosen, nothing that needs one is sent. [GEN, OBS-0061 W1 and W2]
3. **Response:** the request is sent off the UI thread with the client of SPEC-0046 (30 s and redirects
   followed unless the request's settings say otherwise; bodies from files read in under the rules of
   SPEC-0046 behaviour 11, a refused file leaves its reason in the status bar and nothing is sent). The
   Services window opens with the request in its header. Output gets "GET url → 200 OK · T ms · N
   bytes" (N counts the bytes as they arrived; for a decoded body "N bytes, M decoded from gzip"), then
   one line for each note of the request and of the response (a part of the request file that is not
   supported, a redirect that was not followed, credentials kept back, a body that was decoded, cut or
   damaged; SPEC-0046 behaviours 5, 7, 9 and 12), and a "Response" tab, selected, shows the status line, the headers, a blank line and the
   body; a JSON body (by `Content-Type`) is indented, and shown as it arrived when it is not valid
   JSON; a body with a content coding is shown decoded (SPEC-0046 behaviour 12), and a decoded body that
   was cut at a limit or is damaged ends with a line in brackets that says so; a body without text is
   described by its size, and by the content codings that were not decoded when it has some
   ("51 bytes, content coding br, not decoded"). A failure (no connection, timeout, bad URL, a
   refused request) appears in red in Output, which is selected. **v4:** a response with an error
   status is an answer, not a failure: the Response tab stays selected and nothing turns red, but the
   status line says what the status means — "GET url → 404 Not Found · client error · T ms · N
   bytes", "… → 503 Service Unavailable · server error · …". Red stays the sign that nothing came
   back, so a server's answer and a broken connection are never confused, while the class of the
   status is read at a glance, also by a screen reader and by an agent reading the window's text.
   [GEN, decision of v4] The response is only shown: it is
   never run, opened in another program or written to a file, and no history of requests or responses
   is kept. [GEN, security first]
4. Agents send requests through `send_http_request` (SPEC-0053, with the `http` right). [ADR-0008]
5. **Colours (v4):** a `.http` or `.rest` file is coloured with the token classes and theme colours
   of the other languages, line by line as SPEC-0046 reads it:
   - the method (`GET`, `POST`, …) and the HTTP version as keywords, the URL as a string;
   - a header's name as a property and its value as a string;
   - the `###` separator with its title, and comment lines (`#`, `//`), request settings
     (`# @no-redirect`) included, as comments;
   - a variable line `@name = value`: the name as metadata, the value as a string;
   - `{{name}}` as a constant and `{{$uuid}}` and the other dynamic variables as a built-in function,
     wherever they stand (URL, header, body);
   - the body: as JSON when the request's `Content-Type` is JSON or the body begins with `{` or `[`,
     else plain text; a `< path` line as metadata;
   - response handler and response target lines (`> {% … %}`, `> file.js`, `>> file`, `>>! file`)
     as comments: Ferris IDEA never runs or writes them (SPEC-0046 behaviour 9), and the colour says so.
   The colouring follows each edit like the other languages. [GEN, OSS token classes]
6. **Run mark (v4):** every request has a run mark (▶) in the gutter at its request line, like the
   marks of `main` functions and tests. Its tooltip names the request ("Send GET https://…").
   Clicking it sends that request exactly as `Console.Execute.Multiline` with the caret on that line:
   same variables, environment, checks, Services window and output. A file without requests has no
   marks; the marks follow each edit. [GEN]
7. **Title (v4):** `Console.Execute.Multiline` has an entry under Tools › HTTP Client, "Send Request
   at Caret", which is also its title in Find Action (the menu label is the title, SPEC-0031); before,
   Find Action listed it only by its id's words. The same action still runs the statement in an SQL
   console (SPEC-0015). [GEN, OSS action id]

8. **Run All Requests in File (v5):** Tools › HTTP Client › Run All Requests in File
   (`FerrisIDEA.HttpRunAllRequests`, also in Find Action under that title) sends every request of the
   active request file one after another in file order, with the chosen environment, the checks and
   the settings of behaviours 1–3, one request at a time and off the UI thread. The Services window is
   headed "All requests · N". Output gets each request's status line and notes in file order, a
   failure in red; a request that cannot go (unresolved variables, a refused body file) is named with
   its reason ("GET http://{{nothing}}/x → Unresolved variables: nothing") and the others still go.
   The Response tab shows the last response that came back and is selected unless none came back.
   In another file the status bar says that the action works in a `.http` or `.rest` file. Agents run
   a file's requests one by one through `send_http_request` with the numbers `http_requests` lists
   (behaviour 4); a tool of its own would add nothing an agent cannot do. [GEN, ADR-0008]
   **v6:** the editor's context menu of a `.http` or `.rest` file has Run All Requests in File
   after Reformat Code, behind a separator; other files' menus stay as they are. Before, Find Action
   and the Tools menu were the only ways. Ferris IDEA has no editor toolbar for request files, so the
   entry goes nowhere else. [GEN, OBS-0061 W3]
9. **A request that cannot go (v7, OBS-0061 H2, H3):** a single request (⌘↩ or its run mark) that
   cannot go — unresolved variables, a refused body file — is not sent; its reason stands in the
   status bar and, as for a missing environment (SPEC-0102 v5 behaviour 2), as a red line in Output
   ("GET http://{{nothing}}/x → Unresolved variables: nothing"), with the Services window shown and
   Output selected. In Run All (behaviour 8) a request that cannot go is named at its place in file
   order among the others' status lines, not after them. Before v7 the single request said so only
   in the status bar, and Run All put the skipped requests at the end. [GEN]

## Acceptance criteria
- **AK-1** Request at the caret, response text with indented JSON, status line, a decoded body with
  both sizes, a cut one marked, a body with a coding that is not decoded and a body without text
  described, the notes as output lines (unit tests `http_model::tests`).
- **AK-2** In a real window (`ferris-idea ui --selftest`): ⌘↩ in a `.http` file sends the request to a
  local server; the Response tab shows `HTTP 200 OK` and the indented body, Output the status line.
- **AK-3** By hand: a real API with headers and a JSON body, next to the reference.
- **AK-4 (v4, behaviour 5)** Unit tests `http_model::tests` (the colours): every part of behaviour 5
  gets its class at its place — method, URL, version, header name and value, separator, comments,
  variable line, `{{name}}` and `{{$uuid}}` inside URL, header and body, a JSON body with JSON's
  classes, a text body plain, a `< path` line and handler lines; no two parts overlap; a file of
  2,000 requests is coloured.
- **AK-5 (v4, behaviour 6)** Unit tests `http_model::tests` (the marks): one mark per request at its
  request line (0-based), none in a file without requests or in comments and bodies; the request a
  mark sends is the one at that line, also when the caret is elsewhere.
- **AK-6 (v4, behaviours 3 and 7)** Unit tests: the status line of a 404 and a 503 names the class
  and is no failure; a 200 has no class; the action's title in Find Action and the menu entry.
- **AK-7 (v4)** By hand in a window: colours in a dark and a light theme, a click on a mark sends the
  request and the Services window shows it.

- **AK-8 (v5, v6)** In a window of the test harness against a local server, by keys and by clicks on
  controls found through the semantic tree (`crates/workbench/tests/it/http_client_window.rs`:
  `requests_go_from_keys_marks_and_run_all`): the first ⌘↩ with two environments and none chosen
  sends nothing and names both (SPEC-0102 v5 AK-7); after `local` is chosen from the list, ⌘↩ on the
  first request sends it with a URL without a scheme over http; the Response tab is selected and
  shows `HTTP 200 OK` and the indented body, Output the status line; the list chooses `staging`; the
  run mark of the second request sends it; Run All Requests in File sends those that can go in file
  order — among them `{{$random.integer(1, 5)}}` whole, as a number from 1 to 4 — names the one with
  an unresolved variable and the one with `{{$foo bar}}` with their reasons (neither reaches the
  server) and shows the last response. This closes AK-2 and the click on a mark of AK-7 without a
  screen; the colours of AK-7 stay by hand.
- **AK-9 (v5)** Unit test `menus::tests::the_request_at_the_caret_has_a_menu_entry`: Run All Requests
  in File is under Tools › HTTP Client and its label is its title in Find Action.
- **AK-10 (v6, behaviour 8)** Unit test
  `context_menu::tests::request_files_have_run_all_in_the_editor_menu`: the editor's menu of a
  request file has Run All Requests in File (`FerrisIDEA.HttpRunAllRequests`) after Reformat Code, the
  menu of another file does not.
- **AK-11 (v7, behaviour 9)** Unit test `http_model::tests::output_keeps_file_order`: sent and
  skipped requests in file order. In a window of the test harness
  (`crates/workbench/tests/it/http_client_h1_h3.rs`: `a_request_that_cannot_go_is_named_in_output`):
  ⌘↩ on a request with an unresolved variable sends nothing and Output has the red line.

## Out of scope
Response handler scripts, saving responses to files, request history, multipart helpers, cookies, a
separate result per request, a menu on the run mark (it sends; the reference also offers running
with an environment from the mark). Environment files: SPEC-0102.

## Known deviations from the reference
The reference keeps a history of responses; Ferris IDEA shows the latest response. **(v5)** The
reference runs response handler scripts (`> {% … %}`) that test the response and set variables for
later requests, and keeps cookies between requests; Ferris IDEA runs no script from a request file and
keeps no cookie jar (SPEC-0046 behaviour 9 and Out of scope): a script in a request file would run
with the rights of the IDE on whatever a server answers, and a jar written into the project would
keep session tokens in a file. Both are to be compared next to the reference first
(`docs/research/observations/OBS-0059.md`). Run All Requests in File shows one Response tab for the
last response, not one result per request. Both run a request
from its gutter mark (v4) and from the key at the caret. A person needs no right to send
from the window; agents still do (SPEC-0053). Compared next to the reference in OBS-0061; its gaps
W1–W3 are closed in v6.
