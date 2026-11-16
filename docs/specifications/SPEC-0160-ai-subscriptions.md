# SPEC-0160: AI Assistant with a chat subscription

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (agent tools such as OpenClaw and Hermes Agent let users connect a chat
  subscription instead of an API key), OSS (Claude Code `-p --output-format stream-json`, Codex
  `exec --json`, Gemini CLI `-p --output-format stream-json`; their documented output events),
  ADR-0017, SPEC-0138, SPEC-0143, SPEC-0157, ADR-0008
- **Feature matrix rows:** F-082

## Purpose
Use the AI plan the user already pays for, without API keys, and without IDECraft ever holding the
plan's credentials.

## Behaviour
1. **Choice:** Settings › Tools › AI Assistant › Provider cycles through Claude subscription (Claude
   Code), ChatGPT subscription (Codex), Google account (Gemini CLI), then the API templates
   (SPEC-0143). Choosing a subscription saves `ai.subscription` and checks in the background whether
   the program is installed and signed in (yes or no only). The Sign in row shows that state; Enter
   on it opens a terminal tab running the program's sign-in (`claude auth login`, `codex login`,
   `gemini`). [GEN]
2. **Asking:** with a subscription, every chat answer, Explain Code and inline completion runs the
   program once in an empty temporary folder that is removed afterwards, as a plain chat (ADR-0017),
   with the model field as its model when not empty. The conversation goes in as one prompt: the
   system prompt (Claude Code: its option), earlier turns marked User/Assistant, then the question.
   Programs in `~/.local/bin`, Homebrew and npm folders are found even when `PATH` is short. [OSS]
3. **Answer:** Claude Code's and Gemini CLI's pieces are streamed into the chat; Codex's messages
   appear whole. Escape stops the answer and ends the program. No answer within 300 s (30 s for
   inline completion) ends it with a message. [OSS]
4. **Errors:** a missing program names where to get it; a missing sign-in says how to sign in;
   other failures show the program's last error lines. [GEN]
5. **MCP:** `ai_assistant` reports the subscription chosen, the connection's name and each program's
   sign-in state and sign-in command; agents still cannot change the settings or ask through it.
   [ADR-0008]

## Acceptance criteria
- **AK-1** Stand-in programs: Claude Code streams pieces with tools, MCP and persistence switched off
  and the prompt on standard input; Codex and Gemini CLI answers; a failing program; a missing
  sign-in; stopping; the time limit (`subscription::tests`).
- **AK-2** MCP lists the three programs and no subscription by default (`ide_settings` protocol
  test).
- **AK-3** Window self-test: the provider list starts with the subscriptions, the sign-in is checked,
  the API templates follow.
- **AK-4** Real programs on this Mac (2026-11-03): Claude Code and Codex answered through IDECraft's
  code, inline completion through Claude Code gave `a + b`; Gemini CLI, not signed in, was reported
  as such.
- **AK-5** Hand test (round 4, no. 86).
- **AK-6** Window test without a screen with stand-ins of the three programs, found first through
  a folder the test program names (debug builds only, `subscription::use_test_programs`, so the
  test changes no `PATH`): the sign-in is checked, Sign in types `claude auth login` into a new
  terminal tab (whose program has not started yet when it is typed: the input waits for it), a
  question runs Claude Code without tools in an empty folder outside the project, streamed, Escape
  keeps the start, Codex answers whole, Gemini CLI gets the prompt as an argument, inline
  completion through Claude Code; the endpoint is never asked
  (`ai_assistant_handtests::hand_test_86_subscriptions_run_the_programs_without_tools`).

## Out of scope
The programs' tools and editing the project through them (M11, ACP), choosing among a program's
models from a list, a provider's own OAuth flow inside IDECraft (ADR-0017).

## Known deviations from the reference
The reference's AI Assistant uses its vendor's own subscription service; IDECraft uses the plans of
the model providers through their programs, or any OpenAI-compatible API.
