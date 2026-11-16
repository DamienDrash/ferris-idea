# SPEC-0157: Streamed answers and inline completion

- **Version:** 2 · **Date:** 2026-11-15 (v1 2026-11-03; v2: secrets around the caret are masked,
  Tab and Escape while the completion list is open too — found by the re-check of hand test 83
  without a screen)
- **Sources:** GEN (AI assistants in IDEs show answers word by word as the model writes them and
  can be stopped; inline completion suggests code at the caret as grey text after a pause, Tab
  accepts, Escape dismisses; F-082), STD (OpenAI-compatible chat completions with `stream: true`
  as server-sent events: `data:` lines with `choices[0].delta.content`, ended by `data: [DONE]`),
  SPEC-0138, SPEC-0143, ADR-0008
- **Feature matrix rows:** F-082

## Purpose
Answers appear while they are written, and code can be completed by the user's own endpoint
without leaving the editor.

## Behaviour
1. **Streaming:** the chat asks with `stream: true` and shows the answer as it arrives (redrawn
   every 40 ms). An endpoint that answers with plain JSON instead is shown when complete. Errors
   (status, transport) are shown as before; an answer that never started leaves no empty message.
   [GEN, STD]
2. **Stop:** Escape in the chat while an answer arrives stops it; what came so far stays in the
   conversation. [GEN]
3. **Inline completion** (Settings › Tools › AI Assistant, "Inline completion", off by default, only
   while the assistant is on): 500 ms after typing stops, when there is one caret, no selection, and
   only white space after the caret on its line, the code around the caret (at most 4 000
   characters each side, the caret marked `<CURSOR>`) is sent with a prompt asking for only the
   text to insert. The answer is cleaned (code fences, a repeated start of the line). Only the
   newest request counts. **v2:** before it is sent, the code around the caret is masked like an
   attachment (SPEC-0313 behaviour 6, IMP-0299), so a token in an open `.env` or settings file
   does not leave with it; a secret on the caret's own line masks that whole line, which then ends
   at the caret, so no part of the secret goes on either side. [GEN, SPEC-0313]
4. **Showing:** the suggestion's first line appears in grey at the caret, with "(+n lines, Tab)" if
   it is longer, as long as the caret stays there and the text does not change. Tab inserts all of
   it (before any other Tab action) and puts the caret after it; Escape hides it; any edit or caret
   move drops it. **v2:** this holds while the completion list (SPEC-0072) is open as well, as it
   usually is after typing a name: Tab inserts the suggestion and closes the list, Escape hides both
   at once; Enter still chooses the list's item. [GEN]
5. **MCP:** `ai_assistant` reports `inline_completion` and `streaming`; agents still cannot switch
   the assistant on or send through it. [ADR-0008]

## Acceptance criteria
- **AK-1** Streaming piece by piece, stopping, event parsing across chunk borders; inline
  completion request with `<CURSOR>` and the prompt, cleaning (`idecraft-ai` tests against the
  fake endpoint, which now streams).
- **AK-2** MCP: `inline_completion` false and `streaming` true by default (`ide_settings_tools`).
- **AK-3** Window self-test: chat requests are streamed; with inline completion on, a space after
  `let x =` shows the endpoint's suggestion at the caret; Tab inserts it.
- **AK-4** Hand test (round 4, no. 83): a local Ollama model and a provider — answers appear word
  by word, Escape stops, inline completion in Rust and Python.
- **AK-5** (v2) Masking around the caret (`inline_completion_masks_secrets_around_the_caret`);
  window test without a screen: a streamed answer of 200 pieces, Escape while it waits keeps its
  start, grey text in Rust and Python with Tab, typing on and Escape, Tab and Escape with the
  completion list open, a token in an open Python file masked in the request
  (`crates/workbench/tests/it/ai_assistant_handtests.rs`:
  `hand_tests_83_and_88_streaming_escape_and_inline_completion`).

## Out of scope
Completion in the middle of a line, several suggestions to cycle, accepting word by word,
fill-in-the-middle APIs of specific providers, caching.

## Known deviations from the reference
Inline completion is off until the user switches it on, because it sends the code around the
caret at every pause (IMP-0049).
