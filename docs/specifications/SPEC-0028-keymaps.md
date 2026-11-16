# SPEC-0028: Keymaps

- **Version:** 6.3 · **Date:** 2026-11-14 (v6.3: the Run tool window is a part of the window with an action of its own, Send EOF on ⌘D and Ctrl+D, SPEC-0087 v10 behaviour 18; the chords of the four list actions are the focused list's own keys, SPEC-0275 v19 behaviours 47 and 48 — both in behaviour 11. v6.2: the Find tool window's ⌃-letter chords of behaviour 11 also hold ⌃R, ⌃W, ⌃I, ⌃D and ⌃O, SPEC-0293 v9 behaviours 22–26. v6.1: ⌃K in a text input cuts to the end of the
  line, now that `EditorCutLineEnd` is carried out, SPEC-0057 v6 behaviour 24. v6: a chord pressed in a text input, behaviour 11 — an
  action the keymap binds for the input itself goes before the input's editing action on the same
  chord; window check on macOS, where ⌃M in the commit message field did nothing. v5: which keystroke a key event stands for, behaviour 10 —
  a chord with Shift and a digit reaches the digit's chord on every keyboard layout the toolkit
  describes; window check of SPEC-0331 on a German layout, where ⌃⇧1 and ⌃⇧2 did nothing. v4: the bundled macOS keymap differs from the upstream files
  only by a listed set of deviations, a test holds that, and a generated document measures the coverage;
  the find bar's search in selection gets the key of the reference's action. v3: an action listed
  several times in one file takes its last entry, OBS-0011 item 5)
- **Sources:** OBS-0011 (v3: the reference's editor menu on macOS shows ⌃⇧R and ⌃⇧D for running and
  debugging, the bindings of the macOS file's last entries), OSS (`JetBrains/intellij-community` at `d4018fb`,
  `platform/platform-resources/src/keymaps/$default.xml` and `Mac OS X 10.5+.xml`, Apache-2.0, bundled as
  assets; ADR-0009; the upstream keymap files name keys, not the characters a layout types on them),
  STD (Java `KeyStroke` text form: modifiers then key name), GEN; v5: OSS (`gpui` 0.2.2, the
  toolkit: `platform/mac/events.rs` and `platform/linux/platform.rs` report a key pressed with Shift
  that is not a letter as the character the layout types there and take Shift out of the event; a key
  event carries no key code; `platform/mac/keyboard.rs` gives, per layout, the keys macOS moves US
  shortcut keys to), SPEC-0071 v4 behaviour 6 (a layout's key stands in for a US key); v6: OSS
  (`gpui` 0.2.2, `platform/mac/window.rs` `handle_key_event` and `do_command_by_selector`,
  `platform/mac/events.rs` `parse_keystroke`: the order in which a key reaches the application and
  the macOS text system), DOC (Apple, Cocoa text system: the default key bindings of text fields),
  SPEC-0063 v10 behaviour 22, GEN
- **Feature matrix rows:** F-017, F-018

## Purpose
Offer the reference IDE's default key bindings on Windows/Linux and macOS, so users keep their muscle
memory, and answer "which keys run this action" and "what does this key do".

## Behaviour
1. The bundled keymaps are the two upstream XML files, unchanged. `Mac OS X 10.5+` has `$default` as
   parent. [OSS]
2. A keystroke is written as modifiers (`shift`, `control`/`ctrl`, `alt`, `meta`, `altGraph`, case-insensitive;
   `pressed`/`released`/`typed` are ignored) followed by a key name; key names are compared in upper case.
   A keyboard shortcut has a first and an optional second keystroke. Mouse and double-modifier gesture
   shortcuts are kept as given. [STD, OSS]
3. Resolving a keymap with a parent: an action listed in the child replaces all of the parent's shortcuts
   for that action; an action listed without shortcuts has none. Actions not listed are inherited. An action
   listed several times in one file takes the shortcuts of its last entry only (v3; the macOS file lists
   `RunClass` empty, then with `control shift F10`, then with `control shift R`, and only ⌃⇧R is
   bound). [OSS, OBS-0011]
3a. For the macOS keymap, shortcuts inherited from `$default` exchange Control and Meta (keyboard and
   mouse shortcuts, gestures unchanged), so `control C` from `$default` becomes `meta C`; shortcuts the
   macOS file lists itself are taken as written. Logic template: `MacOSDefaultKeymap.kt`
   (`platform/platform-impl/src/com/intellij/openapi/keymap/impl/`). [OSS]
4. Lookups: shortcuts of an action; actions bound to a keystroke (as first keystroke, or as first plus
   second for two-stroke shortcuts), sorted by action id. [GEN]
5. Display: macOS uses the symbols ⌃ ⌥ ⇧ ⌘ in that order followed by the key; other systems use
   `Ctrl+Alt+Shift+Meta+Key`. Two-stroke shortcuts are joined with `, `. [GEN]
6. MCP tool `keymap_lookup` (keymap `default` | `macos`, action or keystroke): bindings in both
   directions with display text. [ADR-0008]
7. **Coverage rule (v4).** The bundled macOS keymap is the upstream macOS keymap (behaviours 3 and 3a
   applied to the two files, nothing else) plus the deviations listed below, and nothing more: every
   shortcut the upstream keymap gives an action stays bound to that action, and every shortcut the
   bundled keymap adds or removes is listed in `crates/ui/keymaps/macos-deviations.json` with the
   specification or improvement entry that decides it and a reason. A deviation is allowed when
   (a) the reference binds the key in the declaration of an action or plugin instead of in the keymap
   files, (b) the action is IDECraft's own or has no key upstream, (c) macOS takes the reference's chord
   itself, or (d) a specification names the difference. The two upstream files stay byte for byte the
   copies of the pinned commit; a newer revision is taken only together with its entry in
   `crates/ui/keymaps/README.md` and `docs/legal/PROVENANCE.md`. [OSS, GEN]
8. **Search in selection (v4).** `ToggleInSelection`, IDECraft's id of the find bar's search in selection
   (SPEC-0065 behaviour 12), is bound to the key the upstream files give `ToggleFindInSelection`, the
   reference's id of the same command (⌃⌥E in both bundled keymaps), beside that action. Before v4 it
   was bound only "where free", which the key never was, so it had no shortcut. [OSS]
9. **Coverage document (v4).** `scripts/keymap-coverage.py` writes `docs/research/KEYMAP_COVERAGE.md`
   from the repository alone: for every action the upstream macOS keymap binds, its shortcuts there,
   whether IDECraft carries the action out (its id in the sources that dispatch actions, or a mapped
   action), IDECraft's shortcuts, and one of same / different shortcut / action exists without shortcut
   / action missing; the chords on which IDECraft binds an action the upstream keymap does not bind
   there; counts and shares, also by area; and the missing actions by area with the specification that
   would own each. The same input gives the same document; `--check` fails when the document is out of
   date. Missing actions are listed, not invented. [GEN]

10. **The keystroke of a key event (v5).** A chord in a keymap names a key (`control shift 1`); the
   toolkit reports a key event as the character the keyboard layout types, and for Shift with a key
   that is not a letter it reports the shifted character without Shift: ⌃⇧1 arrives as ⌃ with `!`,
   ⌃⇧2 as ⌃ with `@` on a US layout and with `"` on a German one. Until v5 such a character had no
   key name, so no chord with Shift and a digit ran from the keyboard, on any layout. For an event
   with ⌃, ⌥ or ⌘ held (without one of them a key types text and is no chord) the workbench asks the
   keymap in this order:
   a. **Digit with Shift.** The character is taken as a US keyboard types it: the US key the
      layout's key stands in for (SPEC-0071 v4 behaviour 6; German `"` stands for `@`, `§` for `#`,
      `&` for `^`, `/` for `&`, `(` for `*`, `)` for `(`, `=` for `)`), else the character itself where
      the layout does not move that US character to another key. If that is one of `!@#$%^&*()`, the
      event is Shift with the digit 1 … 9, 0 of that key and the event's other modifiers; it is used
      when the keymap binds that chord. This holds on every layout the toolkit describes, also on
      one with the digits on Shift (French: ⌃⇧1 arrives as `1`, ⌃1 as `&`).
   b. **The key under its own name** (as before v5), when the keymap binds it or the layout offers
      no stand-in.
   c. **The layout's stand-in** for a US key (as before v5, SPEC-0071 v4 behaviour 6).
   d. **Another key with Shift.** For the other characters Shift types on a US keyboard
      (`_ { } | : " < > ? ~` for the keys `- [ ] \ ; ' , . /` and `` ` ``), taken as in (a): Shift
      with that key — only when (b) and (c) give no keystroke, so no event that had a keystroke
      before v5 gets another one. `+` keeps its own key name `ADD`.

   An event that still carries Shift names its key itself and is read as before. The keymap
   settings record a new shortcut by the same rule: a digit with Shift first, then (b), (c), (d).
   Chords without Shift, and chords with Shift and a letter or a named key, are untouched: every
   keystroke of the two bundled keymaps resolves from the event that names its key exactly as
   before v5. The one exception follows from (a): on a layout where an event can only be Shift
   with a digit — German `/` is ⇧7 and `=` is ⇧0 — the digit's chord wins where the keymap binds
   both. The bundled macOS keymap has no such pair; the default keymap has three (`control SLASH`
   and `control shift 7`, `alt SLASH` and `alt shift 7`, `control EQUALS` and `control shift 0`),
   which matter only where the toolkit describes the layout, today on macOS. [OSS, GEN]

11. **A chord in a text input (v6).** A text input is every place text is typed into: the editor,
   the commit message field, search and filter fields, name fields, the fields of dialogs. While
   one has the focus, a key event with ⌃, ⌥ or ⌘ held, and every key that types no text, is offered
   in this order; the first that carries it out ends it:
   a. **The input method**, while it composes (marked text): it gets every key first and the
      keymap is not asked for a key it takes. A key it leaves over while it composes is no chord
      of the input and none of the part around it: (b) and the part's actions of (d) do not run
      for it, as the input's editing actions of (c) do not.
   b. **The input's own actions**: the actions the keymap binds to the chord for exactly this
      input. Today that is one input: the commit message field with Commit Message History
      (`Vcs.ShowMessageHistory`, ⌃M; SPEC-0063 v10 behaviour 22). Until v6 the field's editing
      action on the same chord, Move Caret to Matching Brace, took ⌃M first, so the history never
      opened from the keyboard.
   c. **The input's editing actions** the keymap binds to the chord (the `Editor…` actions and
      the other actions the editor carries out).
   d. **The actions of the part around the input**, with the focus rule of that part (the Commit
      tool window's ⌃O opens its options also from the message field), then the other actions of
      the keymap.
   e. **Nothing.** A chord the keymap binds to no action that runs does nothing in a text input.

   This order is IDECraft's choice. Its consequences on macOS, for ⌃ with a letter:
   - The text-field chords macOS users know stay, because the bundled macOS keymap binds them to
     the same editing: ⌃A and ⌃E (start and end of the line), ⌃B and ⌃F (one character back and
     forward), ⌃P and ⌃N (one line up and down). In an input they go before an action of the part
     around it on the same key: in the commit message field ⌃P moves the caret up and is not
     Group By Directory, in the rest of the Commit tool window it groups. ⌃M in every other input
     stays Move Caret to Matching Brace; ⌃L centres the caret's line in the editor. ⌃K is bound
     to `EditorCutLineEnd` and, since v6.1, cuts from the caret to the end of the line in the
     editor and in every input made of it, the commit message field among them (SPEC-0057 v6
     behaviour 24); an input that hides its text does not cut.
   - The keymap decides the other chords also where the macOS text system has an editing command
     of its own for them: ⌃D is Debug (not delete forward), ⌃H Type Hierarchy (not delete
     backward), ⌃O Override Methods or the focused part's action (not a line break after the
     caret), ⌃T Refactor This (not transpose), ⌃V the VCS operations popup (not page down). ⌃Y
     (yank) is bound to nothing and does nothing. No editing command of the macOS text system
     runs in an input behind the keymap's back: the toolkit hands every chord with ⌃ to the
     application first and drops the command the text system proposes for a chord nothing took.
     Where the text system hands such a chord over as text to type — one control character, ⌃Q
     as U+0011 — the editor and the fields made of it do not insert it; a line break and a tab
     are text.
   - Return and ⌃M are different keystrokes: the toolkit names the key from the event's
     characters without modifiers, so ⌃M is `control M` and Return is `ENTER`; Return in the
     commit message field stays a line break. An event that carries a carriage return as its
     character without modifiers is Return with ⌃, whatever key a program meant to send; a
     synthetic ⌃M has to carry the key M.

   The ⌃-letter chords the bundled macOS keymap binds to an action limited to a part of the
   window, and whether that part holds a text input: the commit message field ⌃M (Commit Message
   History; a text input); the Commit tool window ⌃M, ⌃P, ⌃O (Group By Module, Group By
   Directory, commit options; holds the message and author fields); the Find tool window ⌃T, ⌃M,
   ⌃P, ⌃S (grouping) and, since v6.2, ⌃R, ⌃W, ⌃I (its filters), ⌃D and ⌃O (Directory Structure,
   Flatten Modules and Export to Text File; SPEC-0293 v9 behaviours 22–26), and the TODO tool
   window ⌃M, ⌃P, ⌃F (grouping); neither holds a text input. The diff viewer, the
   branch popup, the Git log and the Project tree have none.

   v6.3: the Run tool window is such a part too, with one action: Send EOF (SPEC-0087 v10
   behaviour 18) on ⌘D in the bundled macOS keymap and Ctrl+D in the default one. It has no
   ⌃-letter chord on macOS and holds no text input; everywhere else the chord stays Duplicate
   Line (and Compare Two Files in the Project tree). The chords of `List-selectFirstRow`,
   `List-selectLastRow` and their two extending variants (⌥↑, ⌥↓, ⌥⇧↑, ⌥⇧↓ in the bundled macOS
   keymap) belong to no part: the list that has the keyboard takes them itself, before the
   other actions of the chord (SPEC-0275 v19 behaviours 47 and 48), and where no list has it —
   in the editor or the find bar — they stay what they are there (⌥↑ and ⌥↓: Extend Selection
   and Shrink Selection, and the recent searches of the find bar; ⌥⇧↑ and ⌥⇧↓: the editor's
   actions on those chords).

   The Commit tool window has the focus, for its chords, while the focus is anywhere in it: in
   the file list (a click on a row) as well as in its fields. Until v6 only the message field
   counted, so ⌃M and ⌃P did nothing after a click into the list. [OSS, DOC, GEN]

## Acceptance criteria
- **AK-1** Keystroke parsing incl. `ctrl`/`control`, lower-case key names, `pressed`, invalid input.
- **AK-2** The bundled default keymap parses: 422 actions; `SelectNextOccurrence` is `alt J`;
  `SearchEverywhere` is a double-shift gesture; two-stroke shortcuts are read.
- **AK-3** The resolved macOS keymap: overridden actions use the macOS bindings (e.g. `SelectNextOccurrence`
  `control G`, `GotoDeclaration` contains `meta B`), actions listed empty have no shortcut, others are
  inherited with Control and Meta exchanged (`$Copy` is `meta C`, `$Undo` is `meta Z`); a mouse
  shortcut is converted the same way.
- **AK-3a (v3)** In the resolved macOS keymap `RunClass` is only ⌃⇧R, `DebugClass` only ⌃⇧D and
  `FindNext` only ⌘G. (`keymap::tests`)
- **AK-4** Reverse lookup and display on both systems.
- **AK-5** The MCP tool answers both directions and rejects unknown keymaps.
- **AK-6 (v4)** A test compares the bundled macOS keymap with the upstream one and fails when a shortcut
  was added or removed that the deviation list does not hold, when a listed deviation is not in the
  keymap, when an entry has no deciding document or reason, or when one of the two upstream files is
  not the pinned copy (length and checksum). (`keymap::tests`)
- **AK-7 (v4)** In both bundled keymaps ⌃⌥E reaches `ToggleInSelection` beside `ToggleFindInSelection`;
  the upstream macOS keymap gives `ToggleInSelection` no shortcut. (`keymap::tests`)
- **AK-8 (v4)** `python3 -I scripts/keymap-coverage.py --check` passes on the committed
  `docs/research/KEYMAP_COVERAGE.md`; two runs give the same bytes.

- **AK-9 (v5)** With ⌃ held, each of `!@#$%^&*()` (US) and each of `!"§$%&/()=` with the German
  layout's stand-ins resolves to `control shift 1` … `control shift 0` in both bundled keymaps and
  reaches `ToggleBookmark1` … `ToggleBookmark0`; the digits without Shift keep their chords; ⇧⌘8
  reaches `EditorToggleColumnMode` from `*` and from German `(`; with the digits on Shift, `1` is
  `control shift 1` and `&` is `control 1`; without ⌃, ⌥ or ⌘ a character stands for no key; `{`
  with ⌘ is `shift meta OPEN_BRACKET`; the keymap settings would record the digit's chord.
  (`keymap::tests::shift_and_a_digit_is_the_digits_chord_on_us_and_german_layouts`.) In a window
  without a screen, ⌃ with `!` moves the mnemonic 1 to the caret's line
  (`crates/workbench/tests/it/bookmarks.rs`).
- **AK-10 (v5)** Every keystroke of the keyboard shortcuts of the two bundled keymaps whose key the
  toolkit names resolves, from the event naming that key, to the keystroke it resolved to before v5,
  without a layout and with the German one — but for the three pairs of behaviour 10 in the default
  keymap, none in the macOS keymap; every one with Shift and a digit or punctuation key also
  resolves from the event carrying the US character.
  (`keymap::tests::bundled_keymaps_resolve_every_chord_as_before`.)
- **AK-11 (v5)** With a keyboard: ⌃⇧1 … ⌃⇧0 and ⇧⌘8 on a German and on a US layout. (Hand test;
  open — nothing of v5 was tried with a real keyboard.)
- **AK-12 (v6)** In a window without a screen, through the window's key dispatch (the entry the
  platform calls for a chord with ⌃): with the focus in the commit message field ⌃M opens Commit
  Message History and a second ⌃M closes it, the text unchanged; Return there is a line break and
  opens nothing; while the input method composes in the field ⌃M opens nothing and groups nothing;
  ⌃A and ⌃E move the caret to the start and end of the line, ⌃B and ⌃F a character, ⌃P and ⌃N a
  line and the grouping stays, ⌃K cuts from the caret to the end of the line and fills the
  clipboard (v6.1), ⌃O opens the commit options. With the focus in the file list of the Commit tool window ⌃M and ⌃P group by
  module and directory and no history opens. In the author field and in the editor ⌃M opens no
  history; in the editor it moves the caret to the matching brace. A control character handed to
  the field as typed text is not inserted. The decision is a function
  without a window: of the actions of a chord, those of the focused input.
  (`crates/workbench/tests/it/chords_in_text_inputs.rs`; `keymap_actions::tests`.)
- **AK-13 (v6)** With a keyboard on macOS, German and US layout: ⌃M in the commit message field
  opens and closes the history, Return breaks the line, ⌃A ⌃E ⌃B ⌃F ⌃N ⌃P edit in the field and
  in the editor, ⌃K cuts to the end of the line (v6.1), ⌃Y and ⌃D insert or delete nothing in a field, after a click on a row of the
  file list ⌃M groups by module, and ⌃M while a Japanese or Chinese input method composes goes to
  the input method. (Hand test; open — the fix was made
  without a screen; the Cocoa part of the path, from the key to the toolkit's key event, was read
  in the toolkit's source and not run.)

## Out of scope
The other upstream keymaps (Emacs, Sublime Text, GNOME/KDE, XWin, macOS system shortcuts) — later, same
format; user keymaps and editing; dispatching keys to actions (UI), but for which keystroke an event
stands for (behaviour 10) and the order in a text input (behaviour 11). The editing commands of the
macOS text system the keymap does not bind (⌃Y yank, and ⌃D, ⌃H, ⌃O, ⌃T, ⌃V with their text-system
meaning) and a kill buffer besides the clipboard (⌃K itself is carried out since v6.1). Shift with a digit on a layout the toolkit does not describe: on Linux and
Windows the toolkit gives no stand-ins, so there a character is read as a US keyboard types it (a
German ⌃⇧2 arrives as `"` and is not the digit's chord); it needs the key code, which the toolkit's
key event does not carry.

## Known deviations from the reference
The bindings are the upstream files. Action ids only take effect where IDECraft implements the action;
`docs/research/KEYMAP_COVERAGE.md` lists which do. The bundled macOS keymap adds these shortcuts
(behaviour 7; all are additions, no upstream shortcut is removed or moved):

| Action | Added shortcut | Kind | Decided by |
|---|---|---|---|
| `CheckinProject` | ⌘K | (a) key of the reference's version control plugin | SPEC-0063 |
| `Vcs.UpdateProject` | ⌘T | (a) | SPEC-0109 |
| `Vcs.Push` | ⇧⌘K | (a) | SPEC-0109 |
| `ExternalSystem.ProjectRefreshAction` | ⇧⌘I | (a) key of the reference's Maven and Gradle plugins | SPEC-0100 |
| `ReformatWithPrettierAction` | ⌥⇧⌘P | (a) key of the reference's Prettier plugin | SPEC-0123 |
| `NextInlineCompletionSuggestionAction`, `PrevInlineCompletionSuggestionAction` | ⌥] and ⌥[ | (a) declared with the actions | SPEC-0162 |
| `NextInlineCompletionSuggestionAction`, `PrevInlineCompletionSuggestionAction` | ⌃⌥. and ⌃⌥, | (d) bracket keys need ⌥ on German and other layouts | IMP-0050 |
| `CodeCompletion` | ⌥Esc beside ⌃Space | (c) macOS switches the input source with ⌃Space | IMP-0032 |
| `StepOutOfBlock` | ⌃⌥⇧F10 | (b) no key upstream | SPEC-0091 |
| `Hotswap` | ⌃⌥⇧F9 | (b) no key upstream | SPEC-0091 |
| `IDECraft.Database.ShowDdl` | ⌥⇧⌘D | (b) IDECraft's own action | SPEC-0089 |
| `ReopenClosedTab` | ⇧⌘T | (b), (d): no key upstream; of the chord's upstream actions `ServiceView.ShowServices` is not carried out and `GotoTest` yields the chord: it runs from the Navigate menu and Find Action (SPEC-0071 v10 behaviour 16) | SPEC-0059 |
| `MaximizeEditorInSplit` | ⇧⌘' | (d) shares the key of `MaximizeToolWindow`, the focused part decides | SPEC-0059 |
| `ToggleInSelection` | ⌃⌥E | (d) the same command as `ToggleFindInSelection` under IDECraft's id | behaviour 8 |

v5: the upstream keymaps name keys; IDECraft's toolkit reports characters, so behaviour 10 works the
key out from the character and the layout's stand-ins. For the digit row that gives the key pressed
on the layouts the toolkit describes on macOS and on a US layout everywhere; it does not on other
layouts under Linux and Windows (out of scope above), and for punctuation keys with Shift the key is
the US one for that character, not always the key pressed.

Chords of the reference that macOS takes with its default settings (⌃Space, ⌃←, ⌃→, ⌘M, ⌘Q and others,
listed in the coverage document) are kept as the reference has them; only completion has a second key.
The keys the reference binds outside its keymap files are known only where a specification above names
them; the coverage document does not measure them.
