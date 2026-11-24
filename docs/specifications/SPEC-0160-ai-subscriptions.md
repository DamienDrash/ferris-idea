# SPEC-0160: AI Assistant with a chat subscription

- **Version:** 2 · **Date:** 2026-11-23 (v2: a setting switches how Claude with the Claude plan is
  used — "Chat" behind AI Chat as in v1, the default, or "Terminal" as the user's own Claude Code in a
  terminal tab, where the chat uses Claude only with the user's Anthropic API key or sends nothing
  and says why; behaviours 5–7, AK-2, AK-7–AK-10 — Project Owner's answer to open question 53)
  · 2026-11-03 (v1)
- **Sources:** GEN (agent tools such as OpenClaw and Hermes Agent let users connect a chat
  subscription instead of an API key), OSS (Claude Code `-p --output-format stream-json`, Codex
  `exec --json`, Gemini CLI `-p --output-format stream-json`; their documented output events), DOC
  (v2: Anthropic's Agent SDK overview, https://code.claude.com/docs/en/agent-sdk/overview — without
  prior approval, third-party products may not offer claude.ai login or its rate limits and are
  pointed to API key authentication; Anthropic's consumer terms,
  https://www.anthropic.com/legal/consumer-terms, section 3 — no access through automated or
  non-human means except with an API key or where explicitly permitted), OBS-0121 R6, ADR-0017,
  SPEC-0138, SPEC-0143, SPEC-0157, ADR-0008
- **Feature matrix rows:** F-082

## Purpose
Use the AI plan the user already pays for, without API keys, and without Ferris IDEA ever holding the
plan's credentials. **v2:** for Claude the user chooses between the plan behind the chat (waiting for
Anthropic's approval of this use) and the plan in the terminal as Claude Code (the use within
Anthropic's current terms), with Claude in the chat through the user's own Anthropic API key.

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
   Programs in `~/.local/bin`, Homebrew and npm folders are found even when `PATH` is short.
   **v2:** for Claude only in the Chat mode of behaviour 6. [OSS]
3. **Answer:** Claude Code's and Gemini CLI's pieces are streamed into the chat; Codex's messages
   appear whole. Escape stops the answer and ends the program. No answer within 300 s (30 s for
   inline completion) ends it with a message. [OSS]
4. **Errors:** a missing program names where to get it; a missing sign-in says how to sign in;
   other failures show the program's last error lines. [GEN]
5. **MCP:** `ai_assistant` reports the subscription chosen, the connection's name and each program's
   sign-in state and sign-in command; agents still cannot change the settings or ask through it.
   **v2:** it also reports `claude_plan_mode` (`chat` or `terminal`); in Terminal mode with Claude
   chosen, `ready` is true only with the Anthropic API template, a model and a stored key. [ADR-0008]
6. **v2: Claude plan mode** (`ai.claude_plan_mode`, row "Claude plan" on the AI Assistant page, Enter
   or a click switches): [DOC]
   - **Chat** (the default; also for settings saved before v2): behaviours 1–4 as in v1. The page
     says under the row, in short, that this use waits for Anthropic's approval and that Terminal is
     the use within Anthropic's current terms. The Project Owner asked Anthropic for that approval on
     2026-11-23; the default is to be revisited if Anthropic declines or does not answer.
   - **Terminal:** with Claude chosen as the provider, Claude Code is never run for an answer. AI
     Chat, Explain Code and inline completion use Claude through the endpoint when it is the
     Anthropic API template (SPEC-0143) with a model and a key stored for it (system keychain).
     Otherwise they send nothing — no program runs and the endpoint beside it is not asked either
     (no silent fallback) — and the chat says, in plain words, that Claude with the plan is set to
     run in the Terminal as Claude Code, that Claude in AI Chat then needs the user's own Anthropic
     API key, and that nothing was sent. Two actions follow the message: "Use the Anthropic API…"
     opens Settings › Tools › AI Assistant with the Anthropic template filled in (model and key wait
     for the user; the mode and the provider stay), and "Open Claude Code in the Terminal" does what
     behaviour 7 does. The question stays in the field. Sign in opens Claude Code (behaviour 7); the
     hint under the fields and the chat header say the same.
7. **v2: Claude Code in the Terminal:** opens a new terminal tab of the project and types `claude`
   with Enter — the user's own installed program, signed in by the user there (its own prompt or
   `claude auth login`). When `claude` is found outside `PATH` (behaviour 2's folders), its full path
   is typed in quotes. When it is not installed, the status line names where to get it and nothing
   opens. Ferris IDEA never reads, stores or passes Claude's credentials. Every way to open it goes
   through one function of the workbench (`WorkbenchView::open_claude_code_terminal`), where the
   IDE integration Claude Code talks to (a separate specification) attaches. [DOC]

## Acceptance criteria
- **AK-1** Stand-in programs: Claude Code streams pieces with tools, MCP and persistence switched off
  and the prompt on standard input; Codex and Gemini CLI answers; a failing program; a missing
  sign-in; stopping; the time limit (`subscription::tests`).
- **AK-2** MCP lists the three programs, no subscription by default and `claude_plan_mode` `chat`
  (`ide_settings` protocol test).
- **AK-3** Window self-test: the provider list starts with the subscriptions, the sign-in is checked,
  the API templates follow.
- **AK-4** Real programs on this Mac (2026-11-03): Claude Code and Codex answered through Ferris IDEA's
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
- **AK-7** v2: Chat is the default; in Terminal mode with Claude and no Anthropic API key, `chat`,
  `chat_stream` and `inline_completion` refuse with the message, no request reaches the endpoint, no
  piece arrives; with the Anthropic template, a model and a key the endpoint is used; with the
  assistant off the refusal is "off"
  (`tests::claude_plan_in_the_terminal_sends_nothing_without_an_api_key`).
- **AK-8** v2: window test: the Claude plan row switches Chat to Terminal; Sign in then types Claude
  Code into a new terminal tab; a question sends nothing (no program, not the endpoint) and shows
  the message; "Open Claude Code in the Terminal" types Claude Code into a new terminal tab; "Use the
  Anthropic API…" opens the settings on the Anthropic template and keeps the mode
  (`ai_assistant_handtests::hand_test_86_claude_plan_in_the_terminal_sends_nothing_from_the_chat`).
- **AK-9** v2: Claude Code outside `PATH` is typed as its quoted full path; the message names the
  Terminal, Claude Code and the Anthropic API key
  (`subscription::tests::claude_code_is_found_for_the_terminal`).
- **AK-10** v2: `ai.claude_plan_mode` survives a restart (`ide_settings` tests,
  `every_setting_survives_a_restart`).

## Out of scope
The programs' tools and editing the project through them (M11, ACP), choosing among a program's
models from a list, a provider's own OAuth flow inside Ferris IDEA (ADR-0017). v2: the IDE
integration Claude Code uses while it runs in Ferris IDEA's terminal (selection, open files,
diagnostics, diffs in the editor) is specified separately; whether Codex's and Gemini CLI's
providers allow their use behind the chat is not part of this decision.

## Known deviations from the reference
The reference's AI Assistant uses its vendor's own subscription service; Ferris IDEA uses the plans of
the model providers through their programs, or any OpenAI-compatible API. v2: the reference offers
Claude with a Claude plan only in the terminal (OBS-0121 R6); Ferris IDEA offers that as the
Terminal mode, keeps Claude in AI Chat with the user's own Anthropic API key, and by default keeps
the plan behind the chat while Anthropic's approval is pending.
