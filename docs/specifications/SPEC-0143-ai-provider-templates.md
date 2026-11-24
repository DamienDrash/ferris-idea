# SPEC-0143: AI provider templates and model list

- **Version:** 1 · **Date:** 2026-11-02
- **Sources:** SPEC-0138, STD (OpenAI-compatible API: `GET {base}/models` answers
  `{"data":[{"id":…}]}`; the public base URLs of OpenAI `https://api.openai.com/v1`, Anthropic's
  OpenAI compatibility layer `https://api.anthropic.com/v1` and Google's
  `https://generativelanguage.googleapis.com/v1beta/openai`; Ollama `http://localhost:11434/v1`),
  the project owner's decision of 2026-11-02 (subscriptions with OpenAI, Anthropic and Google may be used),
  ADR-0007
- **Feature matrix rows:** F-082

## Purpose
Setting up the assistant with one of the providers the project owner uses should not need their documentation:
pick the provider, paste the key, see which models the key reaches. Ferris IDEA still ships no
account and chooses no model; what a request costs stays the user's decision.

## Behaviour
1. **Provider row** on Settings › Tools › AI Assistant, below "Enable AI Assistant": shows the
   provider whose base URL the Endpoint field holds, else "Custom". Enter or a click fills the
   Endpoint with the next template (OpenAI → Anthropic → Google → Local (Ollama) → OpenAI; from
   Custom the first) and says so in the note. Model and key are left alone; nothing is applied
   or sent until Enter in a field. [STD]
2. **Model list:** after Enter applies a non-empty endpoint, Ferris IDEA asks `GET {endpoint}/models`
   in the background with the stored key (Bearer; for `api.anthropic.com` also `x-api-key` and
   `anthropic-version`) and appends the ids, sorted, to the note: the first 12 and "and n more",
   "the endpoint lists no models", or "no model list: …" with the endpoint's status and message.
   The Model field is not changed. A later apply replaces an outstanding answer. [STD]
3. **Billing note:** the page says that API use is billed by the provider, separately from a
   chat subscription (a ChatGPT, Claude or Gemini subscription does not include API calls).

## Acceptance criteria
- **AK-1** `provider_of` recognises a template with or without a trailing slash and nothing else;
  `list_models` against the fake endpoint returns the sorted ids with the key sent
  (`ferris_idea_ai::tests`); the note shows at most 12 ids, an empty list and an error
  (`settings_view::tests::model_list_note`).
- **AK-2** Window self-test: applying the fake endpoint lists `model-a, model-b` without changing
  the model; the Provider row fills in the OpenAI template.
- **AK-3** Hand test (round 4, no. 69): with the project owner's own keys for OpenAI, Anthropic and Google:
  template, key, model list, one question each.

## Out of scope
Storing several providers at once, provider-native APIs (Anthropic Messages, Gemini), a model
picker, prices per model, signing in with a subscription account.

## Known deviations from the reference
The reference's assistant uses its vendor's service and account; Ferris IDEA offers templates for
endpoints the user pays for directly. Not compared side by side (the reference's AI is not
examined).
