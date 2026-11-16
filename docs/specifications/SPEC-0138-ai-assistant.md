# SPEC-0138: AI Assistant with an endpoint of the user's choice

- **Version:** 4 · **Date:** 2026-11-15 (v1 2026-11-02; v2: the keychain is read and written off the
  UI thread, OBS-0026 L26; v3: an endpoint's error never shows the key, the conversation follows
  the newest message — found by the re-check of hand test 64 without a screen; v4: the chat drawn
  anew after the project owner's look at it with a real model, OBS-0063 A1–A6: Markdown, code blocks with
  actions and Apply as a previewed diff, the open file offered as a context chip, "+", a field of
  several lines with Send and Stop, Explain Code with the focus in the chat)
- **Sources:** GEN (IDEs offer an optional AI assistant: a chat tool window, questions about the
  selected code, "explain code"; F-082 — the reference's own AI service is not examined, its terms
  differ, RESEARCH_BOUNDARIES), STD (OpenAI-compatible Chat Completions API as offered by local
  servers such as Ollama, LM Studio and llama.cpp and by many providers: `POST {base}/chat/completions`
  with `model`, `messages` (`system`/`user`/`assistant`), answer in `choices[0].message.content`,
  errors in `error.message`, key as `Authorization: Bearer`), STD (CommonMark as models write it:
  ATX headings, emphasis, code spans, fenced code blocks with an info string, lists, block quotes,
  thematic breaks, links; GitHub-flavoured tables), GEN (assistant chats in IDEs draw answers as
  rich text with code blocks that can be copied, inserted or applied, and offer the current file as
  removable context — only what is publicly known of such chats; the reference's AI Assistant is
  not examined, F-082), OBS-0063, SPEC-0129 (settings), SPEC-0108 (keychain), ADR-0007, ADR-0008,
  ADR-0009
- **Feature matrix rows:** F-082

## Purpose
An assistant in the IDE for those who want one, with the provider, its cost and where the code
goes decided by the user alone. IDECraft ships no provider, no account and no telemetry
(ADR-0007); off, it sends nothing.

## Behaviour
1. **Settings › Tools › AI Assistant:** "Enable AI Assistant" (off by default), Endpoint (base URL
   of an OpenAI-compatible API), Model, API key. Enter applies; endpoint and model go to
   `settings.toml`, the key into the system keychain (service `idecraft-ai`, account host:port of
   the endpoint) and the field is emptied; the page says whether a key is stored. A key is optional
   (local servers need none). **v2:** the keychain is read (once per endpoint when the page is shown,
   not on every search keystroke) and written in the background, never on the UI thread: a system
   prompt for keychain access cannot stop the window from drawing; meanwhile the page says "checking
   the keychain…" or "storing the API key in the keychain…" with "(if the system asks for keychain
   access, allow it)", and the window draws again when it becomes active (SPEC-0108 v4 behaviour 12).
   The model list is fetched after the key is stored. [STD, SPEC-0108, OBS-0026 L26]
2. **Sending:** only when switched on with endpoint and model; otherwise the chat names what is
   missing. One request per question to `{endpoint}/chat/completions` with a short system prompt,
   the conversation so far and the question; no streaming; timeout 120 s. The key goes only to that
   host: redirects are not followed. Errors show the endpoint's message and status. **v3:** some
   endpoints repeat the key they were sent in their error ("Incorrect API key provided: …"); the
   message is shown with that key, and anything else SPEC-0313 behaviour 6 finds, replaced by
   `‹masked›` — in the chat and in the settings page's model list alike. [STD, SPEC-0313]
3. **Attachments:** code is attached visibly ("Attached: src/a.rs:3-9 (7 lines)") before it is
   sent: opening the chat with a selection attaches it; Escape removes it; the question sends it
   as a fenced block with the file label and language. Nothing else from the project is sent.
4. **AI Chat window** (`IDECraft.AiChat`, Tools › AI Assistant, View › Tool Windows): status line
   with model and endpoint, the conversation (You / AI), "Waiting for the answer…", errors, the
   question field (Enter sends, Shift+Enter is a line break), New Chat; Escape and ⇧Esc hide it.
   **v3:** the conversation scrolls to the newest message when a question is sent, while the answer
   arrives and when it ends, so the answer and its actions (SPEC-0313 behaviour 8) are in view.
5. **Explain Code** (`IDECraft.AiExplainCode`): the selection, else the whole file, is attached and
   asked to be explained; the answer appears in the chat.
   **v4:** Explain Code is available wherever the focus is in the window, the chat's field
   included, as long as an editor is open (OBS-0063 A6; the menu bar asks the focused element's
   path, which reaches the window's handler).
6. **MCP:** `ai_assistant` returns `enabled`, `endpoint`, `model`, `key_stored` and `ready`,
   never the key. Agents cannot switch the assistant on, change its endpoint or send through it;
   `set_ide_settings` does not touch it. [ADR-0008]
7. **(v4) Answers as Markdown** (OBS-0063 A1): headings (`#`–`######`, larger and semibold),
   paragraphs, bold and italic, inline code as a chip in the editors' font, numbered and bulleted
   lists with nesting, block quotes, tables with a header row and cell borders, rules and links.
   Lines break between words only: never inside inline code, a word or before its punctuation.
   An inline code chip is one unit in the line, one line high and as wide as its text; a chip or
   word wider than the column is cut at the column's edge, never set character by character
   (window check C1, 2026-11-15). An answer still arriving is drawn the same way;
   an open fence is already a code block. Marks that are no Markdown (`snake_case`, `2 * 3`,
   `\*`) stay text. **Links** open only after confirmation: a click shows the address with Open
   and Cancel; only `http` and `https` addresses open, in the system browser; anything else
   (`file:`, `javascript:`) is named and not opened. [STD, OBS-0063]
8. **(v4) Code blocks** (OBS-0063 A2): a header naming the language of the fence (`text`
   without one) with **Copy** (to the clipboard), **Insert at Caret** (into the active editor at
   its caret), **Replace Selection** (in place of the active editor's selection; without one the
   chat says to select first and nothing changes) and **Apply…**, each drawn as a small framed
   button that darkens under the pointer (C2); the lines in the editors' syntax colours for the
   fence's language, in the editors' font, scrolled sideways rather than wrapped. Lines wider than
   the block show that they go on: a fade at the edge with more to see and a thin bar under the
   lines whose thumb is the part shown (C3). A shell block keeps Insert into Terminal (SPEC-0313 behaviour 8). **Apply…** changes
   nothing: it shows the diff of the active file against the file with the block in place of the
   selection, or of the whole file without one (the file's last line break kept), with the lines
   added and removed, the changed lines and two around them, "Apply Changes" and "Cancel" (Escape
   cancels). Apply Changes makes the change as **one undo step** and is refused when the file
   changed since the preview. Insert and Replace are one undo step each too. Every action says in
   the chat what it did ("Applied to stats.py; Undo takes it back"). No code is changed without a
   click on one of these buttons. [GEN, OBS-0063]
9. **(v4) Context** (OBS-0063 A3, A8): below the conversation the active file is **offered** as a
   chip "stats.py:1-16" (with the selection's lines when there is one): dashed and muted while
   suggested, which is how it starts and how it returns after each question and when another file
   or selection becomes active; one click turns it on (accent border, ✓), another off. Only when it is on does
   the next question read and send the file (or its selection), masked and within the budget like
   every chip (SPEC-0313 behaviours 6, 7); nothing is sent unasked. The chip is not offered when
   the same code is attached already. **"+"** opens a list: Current File, Selection, Problems,
   Terminal (last command), Git Diff; each attaches a chip as the corresponding menu entries do
   (SPEC-0313 behaviour 3); what cannot be attached is said in the chat. [GEN, OBS-0063]
10. **(v4) Layout** (OBS-0063 A4, ADR-0009): the header holds the title, the model's name — its
    endpoint, or the subscription's program, only in its tooltip — and "New Chat", which never
    overlaps the title, the model or the header's buttons (the model's name is cut with "…").
    The person's messages are bubbles on the right with the chips they went with; the assistant's
    answers use the full width under a small "Assistant" line with Copy (the answer's Markdown).
    16 px between messages. Before the first question the chat says what it offers.
11. **(v4) Field** (OBS-0063 A5): several lines — ↩ sends, ⇧↩ makes a line, the field grows to
    8 lines and then scrolls — with "Ask a question…" while empty and the hint "↩ sends · ⇧↩ new
    line". **Send** next to it (dimmed while the field is empty). While an answer arrives the
    button is **Stop** and the line says "● Answering… N s — Esc stops"; Stop keeps what came so
    far (SPEC-0157 behaviour 2).

## Acceptance criteria
- **AK-1** Refusals when off, unconfigured or with a bad URL; message with attachment; request
  body; key reference; a chat with a fake endpoint including Bearer key, no header without key,
  and an error status (unit tests `idecraft_ai::tests`).
- **AK-2** MCP: off and without provider by default, the key not stored, the settings carry `ai`
  (`crates/mcp/tests/it/protocol.rs`: `ide_settings_tools`).
- **AK-3** Window self-test against a fake endpoint on the loopback address: the settings page
  switches on with endpoint and model; Explain Code sends the selection with model and question
  and shows the answer; a follow-up question carries the conversation; Escape hides the chat;
  switched off again.
- **AK-3a** (v2) No keychain access on the UI thread: a scan of the window's code finds every use of
  the system store inside a background task or a function documented to run on one
  (`crates/workbench/tests/it/keychain_thread.rs`).
- **AK-4** Hand test (round 4): with a local model server and, if the project owner chooses one, a provider:
  chat, Explain Code, a wrong key, an unreachable endpoint.
- **AK-5** (v3) An error repeating the key shows it masked (`an_error_never_repeats_the_key`);
  window test without a screen against a fake endpoint that checks the key: nothing sent while off,
  switched on with clicks and typing, the key in the test secrets folder and not in
  `settings.toml`, the model list asked with it, the selection visible as a chip before sending,
  Explain Code and a follow-up, a wrong key and an unreachable endpoint named, the key only in the
  `Authorization` header (`crates/workbench/tests/it/ai_assistant_handtests.rs`:
  `hand_tests_64_and_69_endpoint_key_explain_and_errors`); the newest answer's Insert into
  Terminal is clickable after a long conversation
  (`hand_tests_213_and_214_terminal_run_problems_and_diff_in_the_chat`).
- **AK-6** (v4) Markdown: the blocks of an answer with every kind, an open fence while arriving,
  text that is no Markdown, words that keep code and punctuation together, links only over
  http(s), fence languages to the highlighter (unit tests `ai_markdown::tests`).
- **AK-7** (v4) Window test without a screen against a fake endpoint answering with a heading,
  inline code, bold, a link, a nested list, a table, a rule, a Python and a shell block
  (`crates/workbench/tests/it/ai_assistant_handtests.rs`:
  `chat_draws_markdown_code_actions_and_offers_the_file`), checked in the semantic tree: heading,
  list items, table and cells, paragraphs without Markdown marks, the rule, the code blocks with
  their languages; ⇧↩ makes a line without sending, ↩ sends; the model in the header.
- **AK-8** (v4) Same test: Copy puts the block on the clipboard; Insert at Caret inserts and one
  Undo takes it back; Replace Selection without a selection changes nothing and says so; Apply…
  shows "+2 −0" for the file and changes nothing, Apply Changes changes it and one Undo takes it
  back; a link opens only after Open.
- **AK-9** (v4) Same test: the open file is offered unchecked and not sent with the first
  question; one click turns it on and the next question carries it, then it is offered again; "+"
  › Current File attaches it; with the focus in the chat's field Explain Code is available to the
  menu bar and explains the open file.
- **AK-9a** (v4, window check of 2026-11-15) Same test, from the bounds in the semantic tree:
  every inline code chip with several characters (`mean([])`, `ZeroDivisionError`,
  `[3, 1, 4, 1, 5, 9]`, two words in one chip) is at most 1.3 lines high, at least 5 px per
  character wide and covers no other word or chip (C1); a Python line wider than the chat shows
  the bar that it goes on, a short shell block does not (C3).
- **AK-10** (v4) Hand test: with a real model, how the chat looks (project owner, OBS-0063 repeated).

## Out of scope
Inline completion, commit messages, several providers at once, provider-specific APIs other than
the OpenAI-compatible one, the reference's AI service; selecting text inside an answer (Copy
copies a block or the whole answer); images and HTML in answers.

## Known deviations from the reference
The reference's assistant uses its vendor's service with its own account; IDECraft talks to an
endpoint the user names and ships none. Not compared side by side (the reference's AI is not
examined).
