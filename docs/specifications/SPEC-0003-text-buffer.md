# SPEC-0003: Text buffer with undo/redo

- **Version:** 3 · **Date:** 2026-11-14 (v3: the size of the history, behaviour 10, and the carets a
  step may carry, behaviour 11; from the check of feature row F-011 without a screen. v2: what undo
  and redo may show, stated as behaviour 9 and checked across the editor commands; no change of
  behaviour)
- **Sources:** GEN (editor fundamentals), STD (Unicode scalar values), OSS (the open platform's
  undo limits in `platform/util/resources/misc/registry.properties`, read as a fact to deviate from)
- **Feature matrix rows:** F-010, F-011

## Purpose
Hold a document in memory, edit it, and undo or redo edits.

## Behaviour
1. Positions are counted in Unicode scalar values, never bytes; a caret cannot land inside a character. [STD]
2. Lines and columns are zero-based internally. A trailing line break produces an empty last line. [GEN]
3. Line text is reported without its terminator; both LF and CRLF are recognised. [GEN]
4. The dominant line ending (LF or CRLF) is detectable so that saving can preserve it. [GEN]
5. Insert, delete and replace reject ranges outside the document and leave it unchanged. [GEN]
6. Every edit can be undone and redone. A new edit after undo discards the redo history. [GEN]
7. Edits can be grouped so that one undo reverts the whole group (e.g. a refactoring). Nested groups flatten. [GEN]
8. The buffer knows whether it differs from the last saved state; undoing or redoing back to that state counts as unmodified, but a *different* edit never does. [GEN]
9. **v2 – Only what was on screen:** undo and redo move between the texts the document had after a
   command, a save or a reload from disk; they never show a text in between (half a grouped edit, half a
   reload, one caret's part of an edit at several carets). A buffer that reports itself unmodified holds
   exactly the text last saved or loaded. This holds across typing runs, edits at several carets,
   formatting, saving and reloading (SPEC-0025, SPEC-0026, SPEC-0057). [GEN]
10. **v3 – Size of the history:** the history of a buffer is not cut after a number of steps: every
    step since the buffer was made can be undone, down to the text it started with, and redone
    again. A step holds only the text it removed and the text it put in, so the history grows with
    what was edited, not with the size of the file. (The open platform keeps the newest 100 steps
    of a document and drops older ones without a word; IMP-0406.) [GEN, OSS]
11. **v3 – Carets of a step:** a step may carry the carets before and after its command; undo and
    redo hand them out for the editor to put back (SPEC-0057 v7 behaviour 9). Steps joined into
    one keep the carets before the first and after the last. Carets that do not fit the text are
    not handed out. [GEN]

## Acceptance criteria
- **AK-1** Insert/delete/replace produce the expected text.
- **AK-2** Offsets and positions count characters (umlauts, euro sign).
- **AK-3** Position ↔ offset round trip; invalid positions fail.
- **AK-4** Out-of-range edits fail without change and without history entry.
- **AK-5** Undo/redo of single steps.
- **AK-6** Grouped edits undo as one step.
- **AK-7** New edit clears redo.
- **AK-8** Modified flag follows the saved state through history.
- **AK-9** Line-ending detection LF/CRLF.
- **AK-10 (v2)** Through the editor commands: undo and redo of edits step by step, of an edit at several
  carets, of a formatting run and of a reload from disk as one step each; undo stops at the saved state
  and goes past it; a new edit drops the redo steps; and over 40 fixed random runs of 300 commands
  (typing, deleting, pasting, two carets, formatting, saving, reloading, undo, redo) no undo or redo shows
  a text that was not on screen before, and an unmodified buffer always equals the saved text
  (`crates/editor/tests/it/undo.rs`).
- **AK-11 (v3)** 1 500 steps are undone down to the first text and redone to the last
  (`crates/editor/tests/it/undo.rs::the_history_is_not_cut_after_a_number_of_steps`); carets noted
  on a step come back with undo and redo, joined steps keep the first and the last, carets
  outside the text are left out (`buffer::tests::carets_noted_on_a_step`).

## Out of scope
Multiple cursors, selections, soft wrap, large-file streaming (later specs).
