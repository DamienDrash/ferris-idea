# SPEC-0162: Inline completion mid-line and alternative suggestions

- **Version:** 2 · **Date:** 2026-11-03
- **Sources:** GEN (AI code completion in IDEs suggests before closing brackets and lets the user
  cycle through suggestions with ⌥] and ⌥[), SPEC-0157, SPEC-0160
- **Feature matrix rows:** F-082

## Purpose
Suggestions also where most typing happens, inside a call or literal, and a way to get another one
when the first does not fit.

## Behaviour
1. **Mid-line:** after the pause a suggestion is also asked for when only closing brackets, quotes,
   `;`, `,` and spaces follow the caret on its line. It is drawn into the line at the caret and moves
   the rest of the line to the right; clicks map to the text without it. Tab inserts it. [GEN]
2. **Alternatives:** with a suggestion shown, ⌥] shows the next one; at the last one IDECraft asks for
   a completion different from every one shown so far (the same request plus the list) and shows it,
   or says "No other suggestion" and keeps the current one when the answer repeats one. ⌥[ shows the
   previous one. Typing or moving the caret forgets them. This works with an endpoint and with a
   subscription. The keys are the keymap actions `NextInlineCompletionSuggestionAction` and
   `PrevInlineCompletionSuggestionAction` (ids and ⌥]/⌥[ from intellij-community), rebindable in
   Settings › Keymap, plus ⌃⌥. / ⌃⌥, on macOS and ⌥. / ⌥, elsewhere, which need no bracket key
   (IMP-0050). [GEN, OSS]

## Acceptance criteria
- **AK-1** Where a suggestion fits (`inline_tests::suggestions_fit_before_closers_only`); the
  suggestion's run splits the line's colour runs at the caret
  (`ghost_tests::the_suggestion_run_splits_the_line_at_the_caret`).
- **AK-2** An alternative is asked for with the shown ones and a repeat counts as none
  (`alternatives_ask_for_something_else_and_drop_repeats`).
- **AK-3** Window self-test: ⌥] on a shown suggestion asks for another and keeps the shown one when
  the endpoint repeats itself.
- **AK-4** Hand test (round 4, no. 88).
- **AK-5** Window test without a screen: typed `vec![` with the `]` the IDE closed, the suggestion
  stands before the `]`; ⌥], ⌥[, ⌃⌥. and ⌃⌥, step through the suggestions, a repeated answer is
  "No other suggestion", Tab inserts the one shown
  (`ai_assistant_handtests::hand_tests_83_and_88_streaming_escape_and_inline_completion`).

## Out of scope
Several suggestions requested at once, accepting a suggestion word by word.
