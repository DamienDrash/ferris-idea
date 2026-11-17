# SPEC-0095: Refactoring shortcuts

- **Version:** 13 · **Date:** 2026-11-16 (v13: jdtls gets the file's indentation too, behaviour 3b, AK-23; the name popup and `new_name` also after Extract Field and Introduce Parameter, behaviour 8d, AK-24; a refactoring that nobody offers for the language says so, behaviour 7a, AK-25; headless check of F-073 on 2026-11-16, OBS-0072); 2026-11-14 (v12: Safe Delete of a declaration with a usage check, and of files in the Project tree, behaviours 15 and 16, AK-20 to AK-22; ninth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-14 (v11: Introduce Parameter, behaviour 14, AK-19; eighth package of docs/research/KEYMAP_COVERAGE.md; v10: the status bar says from the first moment what is
  asked, a language server that is still loading the project is not taken at its word and is
  asked again once when it is ready, and every request ends in a list, an edit or one status line,
  behaviours 11a, 12 and 13, AK-17 and AK-18; OBS-0027 W1b, where ⌥⌘N on a Java method showed
  neither list nor message while jdtls was loading the project); 2026-11-13 (v9: the question to the language server has a limit in
  all and the status bar says when it is over, behaviour 11, AK-16; window of 2026-11-13, where ⌥⌘N
  on a Java method left "Inline…" in the status bar for more than a minute); 2026-11-13 (v8: Java names an extracted constant after its place and Inline
  Method lists its variants, see SPEC-0122 v8, OBS-0027 J1 and J2); 2026-11-13 (v7: the server gets the file's indentation for its refactorings, behaviour 3a, AK-15, OBS-0028 T7); 2026-11-13 (v6: TypeScript's "Extract to constant in enclosing scope" is Extract Variable, behaviour 2, AK-14, OBS-0028 T4); 2026-11-10 (v2: hand test round 4 Nr. 14 — only `refactor` kinds are
  asked for, a bare caret is widened to the expressions around it, a refusal says why; v3: hand test
  round 5 Nr. 14 — the new name is asked for right after extracting; v4: hand tests Nr. 14 and 210,
  D55 — renaming the new name covers every use even when the server misses some; v5: window run of
  hand tests Nr. 14 and 210 on 2026-11-10 — Refactor This lists Rename and the extractions, the
  name popup sits at the new name and survives switching applications, Escape says the name was
  kept)
- **Sources:** GEN (Extract Variable, Extract Constant, Extract Method and Inline have their own
  shortcuts and act on the selection or the element at the caret; Refactor This lists the refactorings
  available there; with only a caret, Extract Variable offers the expressions around it; F-073), OSS
  (action ids `IntroduceVariable`, `IntroduceConstant`, `ExtractMethod`, `Inline`,
  `Refactorings.QuickListPopupAction` with their shortcuts in the ported keymaps), STD (LSP code actions
  of kind `refactor.*`, `CodeActionContext.only`, `textDocument/selectionRange`), SPEC-0041 (code
  actions), SPEC-0076 (context actions popup); v3: GEN (after Extract Variable, Constant or Method
  the IDE lets the user type the new element's name at once, starting from a suggestion), SPEC-0074
  (rename from the editor)
- **Feature matrix rows:** F-073
- **Script files (2026-11-09):** see SPEC-0320, which extends this spec for JavaScript and TypeScript.
- **Python (2026-11-12):** see SPEC-0330: where the Python server offers no extraction, Extract
  Variable and Extract Method are IDECraft's own actions, found and applied through this spec's
  path (behaviours 1, 3–10); their refusals replace the message of behaviour 7.

- **Java (2026-11-13, v8):** see SPEC-0122 v8 behaviours 16 and 17. The edits of jdtls's Extract
  Constant and Extract Variable carry a name taken from the place of the expression before
  behaviour 8 looks for the introduced name, so the name popup and `new_name` (behaviour 9) start
  from `UNIT_PRICE_CENTS` instead of `_25`; behaviour 10 applies unchanged, since that name is new
  in the file. For Inline, the one `Inline Method` jdtls offers is replaced in behaviour 1's list
  by two or three variants, so behaviour 3 opens the list instead of applying at once (OBS-0027
  J1, J2).

## Purpose
Reach the common refactorings with the reference's shortcuts instead of looking for them in ⌥↩.

## Behaviour
1. **Shortcuts:** in an editor, `IntroduceVariable` (⌥⌘V / Ctrl+Alt+V), `IntroduceConstant` (⌥⌘C /
   Ctrl+Alt+C), `ExtractMethod` (⌥⌘M / Ctrl+Alt+M) and `Inline` (⌥⌘N / Ctrl+Alt+N) ask the language
   server for the code actions of the selection, or of the caret, on the editor's unsaved text. Only
   `refactor` kinds are asked for (`context.only: ["refactor"]`; clangd's tweaks are all of kind
   `refactor`, so a finer kind such as `refactor.extract` would hide them). While the server answers,
   the status bar names the refactoring ("Extract Variable…"). [GEN, OSS, STD]
2. **Matching:** only enabled actions of a `refactor` kind (or without a kind) count. Since servers name
   their kinds differently (rust-analyzer `refactor.extract`, clangd `refactor`), an action matches by its
   title too: Extract Variable "extract" with "variable" or "local", Extract Constant "extract" or
   "introduce" with "constant", Extract Method "extract" with "function" or "method", Inline a
   `refactor.inline` kind or a title starting with "Inline". [STD]
   (v6) TypeScript names no variable: typescript-language-server offers "Extract to constant in
   enclosing scope" (`refactor.extract.constant`), which writes `const newLocal = …;` before the
   statement, and "Extract to constant in module scope". The first also matches Extract Variable,
   so ⌥⌘V on `x>3` in `if(x>3) return x` applies it at once and opens the name popup at `newLocal`;
   Extract Constant offers both. Before v6 Extract Variable found nothing there and said
   "typescript-language-server cannot extract `x>3` into a variable" (OBS-0028 item 4, T4). Where the
   server offers no extraction at all (TypeScript 7's `tsc --lsp`), SPEC-0320 v7 behaviour 23
   applies. [OBS, STD]
3. **Apply:** exactly one match at one place is applied at once, through the same path as choosing it
   in the ⌥↩ popup (edits on the unsaved text, `codeAction/resolve` or `workspace/executeCommand` with
   the server's `workspace/applyEdit`, one undo step). Several matches at one place open the ⌥↩ popup
   with only those. [GEN, STD]
   3a. **The file's indentation (v7, OBS-0028 T7):** a server that asks the client for a file's
   `formattingOptions` (`workspace/configuration` with the file as `scopeUri`; typescript-language-
   server asks before code actions, refactorings and Optimize Imports and passes them to TypeScript)
   gets `tabSize` and `insertSpaces` from the file's `.editorconfig` (`indent_size`, `tab_width`,
   `indent_style`), else from the indentation the file uses (the greatest common divisor of its
   lines' indents, a lone odd one aside; tabs when more lines start with a tab), else nothing (the
   server's default). Before v7 the answer was empty and TypeScript indented by four: Extract
   Variable wrote `    const newLocal = x > 3;` in a file indented by two, and Extract Function
   indented the new function's body by four. IDECraft's own extractions take the statement's own
   indentation (SPEC-0320 v7 behaviour 26). [OBS-0028, STD]
   3b. **jdtls's indentation (v13):** jdtls asks the same before each code action and quick fix,
   as the two settings `java.format.insertSpaces` and `java.format.tabSize` of the file
   (`workspace/configuration` with the file as `scopeUri`). They get the values of behaviour 3a;
   when the file says nothing (no `.editorconfig`, no indented line), four spaces, the usual Java
   indentation. Before v13 they had no answer and jdtls indented with tabs: in a file indented by
   four spaces, Extract Variable, Extract Field, Extract Method and Extract Constant wrote their
   new lines and the rewritten statement with `\t` (headless check of 2026-11-16, OBS-0072). [STD,
   OSS (eclipse.jdt.ls, EPL-2.0: the two setting names the server asks for)]
4. **Refactor This:** `Refactorings.QuickListPopupAction` (⌃T on macOS, Ctrl+Alt+Shift+T elsewhere)
   opens the popup with every refactoring available at the caret or selection. [GEN, OSS]
   4a. **Own entries (v5):** the server's `refactor` actions at the caret or selection come first;
   IDECraft adds "Rename…" when the caret is on a name (or the selection is exactly one name), since
   a rename is no code action and no server lists it, and "Extract Variable…" and "Extract Method…"
   unless the server's list already has a matching action (behaviour 2). Choosing an own entry runs
   its action (`RenameElement`, `IntroduceVariable`, `ExtractMethod`), so a bare caret is widened
   (behaviour 6) and a refusal is explained (behaviour 7); the popup's preview says what the entry
   will ask. The status bar shows the server's refusal only when the list stays empty, and a
   server's failure (for example a timeout under load) next to the list of own entries. Cause in
   the window run: clangd lists no refactoring on a bare name, and `s.area()` selected in
   `auto placeholder = s.area();` is a variable's whole value, which clangd does not extract. [GEN,
   STD]
5. Agents use `code_actions` with `refactoring` (`extract_variable`, `extract_constant`,
   `extract_method`, `inline`, `any`) to see the places and actions behaviours 1–7 find, and the
   message of behaviour 7, then `apply_code_action` with a place's range and an action's title
   (SPEC-0041). [ADR-0008]
6. **Bare caret:** Extract Variable, Extract Constant and Extract Method need a selection with most
   servers (clangd offers nothing at a bare caret). When nothing matches at a bare caret, IDECraft asks
   the server for the ranges around the caret (`textDocument/selectionRange`) and tries them from inside
   out: for a variable or constant the expressions up to the enclosing statement (not across lines),
   for a function also that first whole statement; pieces that are no expression of their own (a range
   ending in a comma, an argument list in parentheses, the callee `s.area` of the call `s.area()`) and
   repeats are skipped, at most 8 ranges. One place with one match is selected in the editor and
   applied; several places are listed in the popup by their text (one line, at most 60 characters,
   equal texts with their line number), and choosing one selects and applies it. [GEN, STD]
7. **Nothing to do:** when no place matches, the status bar says in the user's terms which server
   refused what, and what to select instead, e.g. "clangd cannot extract `add(s.area(), 1)` into a
   variable. Select part of an expression; a whole statement, or a call that already is a variable's
   whole value, is not extracted." (selection shown trimmed, first line, at most 40 characters; a bare
   caret is "anything at the caret"). Inline says that not every server can inline (clangd cannot). A
   file without a language says that the refactoring needs a language server. [GEN]
   7a. **Not available for the language (v13):** where neither the language server nor IDECraft
   offers the refactoring for the file's language at all — Extract Field and Introduce Parameter
   in Python, Introduce Parameter in JavaScript and TypeScript — the status bar (and `message` of
   `code_actions`) says so: "Extract Field is not available for Python files yet: the language
   server offers none and IDECraft has no own one. Extract Variable and Extract Method are."
   Before v13 it asked for "an expression inside a class" or "inside a method or function" also
   when the selection was one. [GEN]
8. **Name right after extracting (v3):** after an Extract Variable, Extract Constant or Extract
   Method (by its shortcut, ⌃T or ⌥↩; recognised by the title as in behaviour 2) has been applied,
   IDECraft finds the name it introduced in the file — the name the server chose, such as clangd's
   `placeholder` and `extracted` or rust-analyzer's `var_name` and `fun_name` — puts the caret on
   its declaration and opens the rename popup (SPEC-0074 behaviour 1) with that name selected, so
   the user types the name and presses Enter; every use follows through the language server's
   rename. Escape keeps the server's name. The introduced name is the name whose occurrences grew
   most, preferring one that occurs at least twice more (declaration and use) and was not in the
   file before, taken at its first occurrence inside the changed part of the text. If no name is
   found, nothing more happens. The status bar says "Extract Variable: type the name, Enter". [GEN]
   8a. **The prompt follows (v5):** when the popup closes without a new name (Escape, a click
   elsewhere), the status bar says "Kept the name placeholder." instead of the prompt. [GEN]
   8b. **At the name (v5):** the popup opens just below the new name in the editor, where the user
   looks, not at the top of the window; without a painted place (the editor scrolled away) it falls
   back to the top. In the window run the popup was not seen at `extracted` and the keys reached
   the editor; the exact trigger was not reproduced without a window (likely the popup at the top
   was not noticed and closed by a click or by the window losing activation, which left the prompt
   in the status bar). [GEN]
   8c. **Kept across applications (v5):** unlike other popups (SPEC-0275 v4 behaviour 28), a rename
   popup stays open when the window stops being active, with its text; when the window is active
   again the field has the keyboard focus. A name half typed is not lost by switching to another
   application or a screenshot tool, and keys typed after returning never reach the editor
   behind. [GEN]
   8d. **Field and parameter (v13):** behaviours 8–10 also follow an Extract Field and an
   Introduce Parameter (recognised by the title as in behaviour 2: "extract" or "introduce" with
   "field", "introduce", "extract" or "promote" with "parameter"): jdtls names the new field or
   parameter `i` for `price * qty`, and the popup and `new_name` start there. [GEN]
9. **MCP (v3):** `apply_code_action` reports the introduced name and its position as `new_name`
   (`name`, `line`, `character`), so an agent can pass it to `rename_symbol`. [ADR-0008]
10. **Every use follows (v4, D55):** when the introduced name occurred nowhere in the file before the
   extraction, the rename that follows (the popup's Enter, or the next applying `rename_symbol` at
   that name after an applied `apply_code_action`) renames every occurrence of the name in that
   file, adding those the server's rename left out. A server can miss uses when the extracted
   expression does not compile: clangd gives a variable whose type it cannot deduce no references,
   so it renamed only the declaration in the hand test's `shapes.cpp` (whose `= 1` is no pure
   specifier) and left `add(placeholder, 1)` broken. A name that existed before (jdtls derives
   `area` from `area()`) is left to the server alone. [GEN]
11. **A limit for the server (v9):** while a shortcut of behaviour 1 looks for its places the
   status bar shows the refactoring's name with an ellipsis ("Inline…"), and while a chosen action
   is planned the action's title with an ellipsis. Neither stays for ever: the language server has
   45 seconds in all to say where the refactoring can act, and again 45 seconds to plan the chosen
   action. After that IDECraft stops waiting, changes nothing, and the status bar names the
   refactoring or action, the server and the limit: "Inline failed: jdtls: no answer within 45 s".
   An answer that arrives later is dropped. IDECraft's own refactorings (SPEC-0330, SPEC-0320) do
   not depend on the server: when it does not answer in time they are found and planned without
   it. The limit is one for the whole question, because a question is made of several waits (the
   server's start, the document, one request per place, further requests such as those of
   SPEC-0122 behaviours 16 and 17), each with a limit of its own, and because a server that the
   editor already runs keeps the limit per request it was started with, whatever the single
   question asks for. `code_actions` and `apply_code_action` keep their per-request waits. The
   window of 2026-11-13 did not get its list although the same question was answered outside the
   window in about ten seconds; what held the server's answer back there was not reproduced
   without that window. [GEN, STD]
   11a. **Said from the first moment (v10, OBS-0027 W1b):** the line of behaviour 11 ("Inline…",
   the action's title with an ellipsis, and "Context actions…" for ⌥↩ of SPEC-0076) is set when
   the key is pressed and stands in front of the problem at the caret (SPEC-0070 behaviour 4a),
   as do the lines a request ends in (behaviours 7, 11 and 12). Before v10 these were ordinary
   notes, which a problem under the caret hides for as long as the caret stays there. Whether
   that is what hid "Inline…" in the window of OBS-0027 W1b was not reproduced without that
   window; the other reason found is that the question waited behind the server's start for up
   to the 45 seconds of behaviour 11, longer than the 20 and 11 seconds that were waited. When the language server is still starting or loading the project
   the line says so at once: "Inline… (jdtls is still loading the project)". The line of ⌥↩ goes
   when its list opens. [GEN]
12. **A server that is still loading (v10, OBS-0027 W1b):** a language server is *loading* while
   IDECraft is starting it (until it is initialised and, if it reports it, ready), while one that
   reports readiness has not reported it (rust-analyzer's `experimental/serverStatus` with
   `quiescent`, jdtls's `language/status` with `ServiceReady`, the end of ruby-lsp's
   `indexing-progress`), and, for the other servers, while a `$/progress` it began with a title
   that names loading (load, index, import, initialise, set up, start, scan) has not ended; a
   check or a build on a loaded project does not count. This is known only for the servers the
   editor keeps running; a server started for one question (MCP) is waited for as before. From a
   loading server "nothing to offer" and a failure, the limit of behaviour 11 included, are not
   final:
   - **A refactoring's shortcut** (behaviour 1, except ⌃T) says "jdtls is still loading the
     project; Inline is asked again when it is ready." and repeats the question once, by itself,
     when the server has loaded within 45 seconds of the keystroke. The repeated answer is final
     and shown as any other (behaviours 3, 6, 7, 11). When the server has not loaded in time the
     status bar says "jdtls is still loading the project; try again when it is ready." The
     repeated answer is used only if the caret and the text of the file are still those of the
     keystroke, since it may be an edit applied at once; otherwise nothing changes and the status
     bar says "The project is loaded, but the text or the caret changed meanwhile: Inline was not
     repeated." A newer refactoring request replaces a waiting one. Decided 2026-11-13 (Project
     Owner not reachable): repeat once rather than only tell, because the user asked for exactly
     this and would otherwise have to watch for the server and press again; once, so that nothing
     loops.
   - **Refactor This (⌃T)** lists IDECraft's own entries at once (behaviour 4a) and says beside
     the list "jdtls is still loading the project; try again when it is ready."; it is not
     repeated, since the list is already open.
   - **⌥↩** (SPEC-0076) with no action to list says the same line instead of "No context actions
     available here."; it is not repeated, since it would open a list under a caret the user may
     have moved on from. With own fixes its list opens as before.
   - **A chosen action** whose plan fails says the failure followed by the same line.
   - IDECraft's own refactorings (SPEC-0330, SPEC-0320) and their refusals do not depend on the
     server and are final at once.
   An empty answer from a server that is not loading is said in the words of behaviour 7. [GEN, STD]
13. **Every request ends in something visible (v10):** a refactoring's shortcut, ⌃T, ⌥↩ and a
   chosen action each end in a list, an edit, or exactly one line in the status bar; no path
   returns silently. That covers the cases that said nothing before: no file in the editor
   ("Inline needs a file open in the editor."), ⌥↩ in a file without a language and without own
   fixes ("No context actions available here."), an action chosen for a file that was closed
   meanwhile, a place without an action. The request of a shortcut is one function from the
   server's answers and state to an outcome that is a list with rows, places with actions, or a
   line with text, so the rule can be tested without a window. A request replaced by a newer one
   of the same kind shows nothing itself; the newer one does. [GEN]
14. **Introduce Parameter (v11):** `IntroduceParameter` (⌥⌘P, Ctrl+Alt+P; Refactor ›
    Extract/Introduce › Introduce Parameter…) is a refactoring shortcut like Extract Variable:
    behaviours 1–13 hold for it. It takes the server's refactorings whose kind is a
    refactoring and whose title names a parameter together with introduce, extract or promote
    ("Introduce Parameter…" of jdtls); a bare caret is widened to the expressions around it
    (behaviour 6). When the server offers none, the status bar says it cannot introduce the
    selection or anything at the caret as a parameter and to select an expression inside a
    method or function. Servers without such a refactoring (rust-analyzer, clangd,
    typescript-language-server, the Python servers) get that message. MCP: `code_actions` with
    `refactoring: "introduce_parameter"`. [OSS: id, key and title of the open platform; GEN]
15. **Safe Delete of a declaration (v12):** `SafeDelete` (⌘⌦ in the bundled macOS keymap,
    Alt+Delete in the default one; Refactor › Safe Delete…) in an editor deletes the declaration
    of the symbol at the caret, but first looks for its usages, so that nothing still in use
    goes without the user knowing it.
    a. **What is searched.** The symbol at the caret is found as Find Usages finds it (SPEC-0293
       v8 behaviour 20: a caret on no name says so and asks no server); its declaration and its
       usages come from the same bounded search (SPEC-0293 behaviours 2, 3 and 10: the
       language server's references on the editor's unsaved text, in project files only,
       without the declaration itself; when the server is too slow, the whole-word text
       occurrences instead). While it runs the status bar says "Safe Delete: looking for usages
       of `name`…". [GEN, SPEC-0293]
    b. **What is deleted.** The declaration is the symbol of its file's structure (the language
       server's document symbols, SPEC-0079) that has the declared name at the declaration's
       place, the innermost one when several do. Deleted are its whole lines, from the line of
       its name to the end of its body, together with the comment, documentation, attribute,
       annotation and decorator lines directly above it (lines starting with `//`, `/*`, `*`,
       `#[`, `@` or `template`), and one blank line after it where a blank line or the start
       of the file is also before it, so that no double blank line is left. A declaration that
       shares its first or last line with other code (`int a; int b;`) is not deleted: the
       status bar says that Safe Delete deletes only declarations on lines of their own. A
       symbol the file's structure does not list (a local variable for most servers, a
       parameter) is not deleted either, and the status bar says that Safe Delete works on the
       declarations File Structure shows. Without a declaration in the project (a library
       symbol) the status bar says so. [GEN]
    c. **No usages.** The declaration is deleted at once. In a file open in an editor this is
       one undo step there (⌘Z brings it back with the caret where it was); a file no editor
       has open is written on disk with a Local History revision before and after (SPEC-0164 v3
       behaviour 13), so Show History has the old text. The status bar names the deleted kind
       and name and how to take it back ("Deleted function `total`; ⌘Z brings it back",
       "Deleted function `total` in src/b.rs; Local History has the old text"). [GEN]
    d. **Usages found: the preview.** Nothing is deleted. A list opens titled "Safe Delete
       `name`: n usages" (or "n text occurrences" when they come from the text search). Its
       first row is Cancel and is selected, its second is "Delete anyway", and then one row per
       usage with its path, line and line text. Enter on Cancel or Escape changes nothing;
       choosing a usage opens it in the editor (the place left is recorded for Back) and
       deletes nothing; only "Delete anyway" deletes as in c, and the status bar then adds how
       many usages are left behind. [GEN]
    e. **Changed meanwhile.** When the declaration's file changed between the search and the
       deletion (typing, another tool), nothing is deleted and the status bar asks to run Safe
       Delete again. A document of a remote project is not offered: the status bar says Safe
       Delete is not available for it. [GEN]
    f. **MCP.** `safe_delete` (path, 0-based line and character) does the same search and
       returns the declaration (`name`, `kind`, `path`, its line range), `usages` (path, line,
       character, line text), `source` (`language_server` or `text`) and the text after the
       deletion. `dry_run` defaults to true; `dry_run=false` deletes only when there are no
       usages, or with `force=true`; it writes through the open editor's buffer as one undo
       step, else to disk (SPEC-0220 v2 behaviours 7–10). [ADR-0008]
    [OSS: id, keys and the title "Safe Delete…" of the open platform; GEN]
16. **Safe Delete in the Project tree (v12):** while the Project tree has the focus `SafeDelete`
    deletes the selected files the way Delete does (SPEC-0275 behaviour 6: one confirmation,
    the files kept in Local History, one undo step), after a check of their names: for each
    selected file IDECraft looks in the project's own files (SPEC-0148 v3 behaviour 7) for
    whole-word occurrences of the file's name without its extension (`pricing` of
    `pricing.py`), at most 100, other than in the file itself. Without such a mention the
    tree's Delete confirmation follows; with mentions the list of behaviour 15d opens instead
    ("Safe Delete pricing.py: n mentions"), and "Delete anyway" deletes without a further
    question. Folders, and files whose names say nothing about their content (`mod`, `lib`,
    `main`, `index`, `__init__`, `__main__`), go straight to the confirmation. The check is by
    text, not by a language server: it finds imports, includes and module declarations that
    name the file, and also unrelated words that happen to be equal. The project folder and
    the extra nodes stay (SPEC-0061 v4 behaviour 16). [GEN; SPEC-0275]

## Acceptance criteria
- **AK-1** Matching by kind and title for rust-analyzer and clangd titles, disabled and quick-fix
  actions left out, action ids (unit test `intentions_model::tests::refactorings_by_kind_and_title`).
- **AK-2** In a real window (`idecraft ui --selftest`, with clangd): with `x * 2` selected in a C++
  file, ⌥⌘V applies clangd's "Extract subexpression to variable" to the editor text.
- **AK-3** By hand: the four shortcuts and ⌃T with rust-analyzer and clangd, next to the reference.
- **AK-4** Against real servers (live tests, skipped when the server is missing): with clangd on the
  hand test's `shapes.cpp`, a selected `s.area()` is extracted through executeCommand/applyEdit, a bare
  caret in `side * side` finds that expression, the selected initial value `add(s.area(), 1)` finds
  nothing, a whole line finds Extract Method, Inline finds nothing and ⌃T finds clangd's refactorings
  (`clangd_refactoring_shortcuts`); with rust-analyzer a selection, a bare caret (the call and the
  enclosing call, no statement), a line for Extract Method and Inline on a variable
  (`rust_analyzer_refactoring_shortcuts`).
- **AK-5** Widening, labels and messages without a server (unit tests
  `actions::tests::widening_keeps_expressions_and_stops_at_the_statement`,
  `actions::tests::refactorings_say_why_nothing_happened`,
  `intentions_model::tests::target_rows_are_short_single_lines`).
- **AK-6** MCP `code_actions` with `refactoring`: places for a bare caret, an empty list with the
  message for a refused selection, an unknown name is an error (protocol test `code_insight_tools`).
- **AK-7 (v3)** The introduced name is found after clangd's Extract Variable (`auto placeholder`) and
  Extract Function (`extracted`), rust-analyzer's `var_name` and `fun_name`, a server-chosen name
  that existed before (jdtls), and not found when nothing new was introduced; titles of Extract
  Variable, Constant and Method count, Inline does not (unit tests
  `actions::tests::the_introduced_name_is_found`, `actions::tests::extractions_introduce_a_name`).
- **AK-8 (v3)** By hand (round 5, Nr. 14 and 210): ⌥⌘V on `s.area()` in `shapes.cpp` opens the
  rename popup with `placeholder` selected; typing `flaeche` and Enter renames declaration and use;
  ⌘Z twice restores the original.
- **AK-9 (v4)** With the pooled clangd on the hand test's `shapes.cpp`, extracting `s.area()` and
  renaming `placeholder` to `flaeche` changes declaration and use (live test
  `tests/pool_extract.rs::the_extracted_variable_is_renamed_everywhere`); the missing uses are added
  once each (unit test `actions::tests::a_new_name_is_renamed_everywhere`); over MCP,
  `apply_code_action` then `rename_symbol` on a file that does not compile renames both (protocol
  test `code_insight_tools`).

- **AK-10 (v5)** Refactor This's own entries: Rename on a name, the extractions the server did not
  list, a disabled action not counted (unit test
  `actions::tests::refactor_this_adds_rename_and_missing_extractions`); against clangd a bare name
  gets no server action and the own Rename, a selected `s.area()` gets clangd's Extract Variable and
  the same text as a variable's whole value nothing (`clangd_refactoring_shortcuts`); against
  rust-analyzer a name gets server actions without Rename (`rust_analyzer_refactoring_shortcuts`).
- **AK-11 (v5)** Messages: "Kept the name …" (unit test
  `actions::tests::status_messages_follow_the_last_action`).
- **AK-12 (v5)** Extract Method on the hand test's line 26: clangd's `extracted` is found at its
  declaration `void extracted(` (line of the new function, character 5)
  (`clangd_refactoring_shortcuts`); rust-analyzer's `fun_name` is found
  (`rust_analyzer_refactoring_shortcuts`).
- **AK-13 (v5)** In a real window (`idecraft ui --selftest`, `refactor_name_steps`): ⌘Z after the
  rename and after the extraction names what was taken back; Escape in the name popup says the name
  was kept; ⌥⌘M on the whole line opens the name popup at `extracted` and Enter renames declaration
  and call; ⌃T on a name lists Rename and Extract Variable. Not run yet. Behaviour 8c (switching
  applications) needs a hand test.
- **AK-14 (v6)** The titles of typescript-language-server 5.3.0 with TypeScript 5.6.3 (as it
  answered for `x>3`, `[1,2,3]` and a whole statement): Extract Variable matches the constant in
  the enclosing scope only, Extract Constant both constants, Extract Method both functions,
  disabled actions none (`actions::tests::typescript_extractions_match_their_refactorings`). Live
  with the server (`crates/languages/tests/it/ts_extract.rs::typescript_language_server_extracts_variables_constants_and_functions`,
  skipped unless IDECraft chooses typescript-language-server): one place each for `x>3` and
  `a+b*2` with "Extract to constant in enclosing scope", applied, the introduced name found and used
  in `if(…)`; `[1,2,3]` offers the module-scope constant; a whole statement offers "Extract to
  function in module scope", applied, the new function found. MCP
  (`protocol.rs::typescript_extract_variable_tool`) with either server.

- **AK-15 (v7, behaviour 3a)** The indentation of a file and the answer to `formattingOptions`,
  `.editorconfig` first (`lsp::tests::formatting_options_follow_the_file`); with
  typescript-language-server, in a file indented by two, the new line of Extract Variable and the
  body of Extract Function are indented by two (live test
  `crates/languages/tests/it/ts_extract.rs::typescript_language_server_extracts_variables_constants_and_functions`;
  without the answer they are indented by four).
- **AK-16 (v9, behaviour 11)** A question that is not answered is given up at its limit with an
  error naming the server, one answered in time is passed on, and a file without a language server
  is "the language server" in the message (`actions::tests::a_question_is_given_up_at_its_limit`).
  In a window with the server pool and a real jdtls, ⌥⌘N on a Java method's name ends with the
  list or a message, never with "Inline…" left standing
  (`crates/workbench/tests/java_delegate_import.rs::inline_on_a_method_lists_its_variants`,
  skipped without jdtls). With a server that never answers, in a window: to be checked on the Mac.
- **AK-17 (v10, behaviours 11a and 12; OBS-0027 W1b)** With a scripted server: one that is
  loading and does not load in time gives the waiting line and then "jdtls is still loading the
  project; try again when it is ready." after one question; one that loads gives the repeated
  question's list, or, when that is empty, the words of behaviour 7, after two questions; a
  question given up at its limit while the server loads waits as well and is not repeated with
  no time left; a ready server's empty answer, list and failure are said at once without a wait;
  an own refusal does not wait; Refactor This keeps its own entries with the loading line beside
  them; the progress, ⌥↩ and failure lines name a loading server
  (`intentions_model::tests::a_loading_server_is_asked_again_and_a_ready_one_is_taken_at_its_word`).
  Loading work announced with `$/progress` counts until its end and a check does not
  (`pool::tests::progress_that_loads_the_project_is_followed_to_its_end`). In a window with the
  server pool and a real jdtls, ⌥⌘N pressed once right after the file was opened shows a line
  starting with "Inline…" or naming jdtls as loading at every moment until the list opens, and
  the list opens without a second keystroke when jdtls loads in time
  (`crates/workbench/tests/java_delegate_import.rs::inline_on_a_method_lists_its_variants`,
  skipped without jdtls). Next to the window of OBS-0027 W1b: to be checked on the Mac.
- **AK-18 (v10, behaviour 13)** For every refactoring, every state of the server (ready, loading
  for good, loading then ready), every pair of first and repeated answer (empty, failure, one
  place, several places) and with or without an own refusal, the request ends in an outcome the
  user sees and asks the server at most twice; every wait is announced and only a loading server
  is waited for; no place, a place without an action, an empty list and a blank line are no
  outcome (`intentions_model::tests::every_request_ends_in_a_list_or_a_status_line`).
- **AK-19 (v11, behaviour 14)** Which code actions are the refactoring and its message
  (`actions::tests::introduce_parameter_matches_parameter_refactorings`); for every answer of a scripted server
  the request ends in places, a list or a status line
  (`intentions_model::tests::every_request_ends_in_a_list_or_a_status_line`); in a window
  without a language server the status bar says what is missing
  (`eighth_package_keymap.rs::introduce_parameter_is_a_refactoring_shortcut`).
- **AK-20 (v12, behaviour 15b)** The lines of a declaration with its comment and attribute
  lines and one blank line, a declaration sharing a line refused, the innermost symbol of the
  name at the place chosen (unit tests
  `safe_delete::tests::declaration_lines_take_comments_attributes_and_one_blank_line`,
  `safe_delete::tests::a_shared_line_is_refused`,
  `safe_delete::tests::the_innermost_symbol_of_the_name_is_chosen`); the rows and messages
  (`keymap_actions::tests::safe_delete_rows_and_messages`).
- **AK-21 (v12, behaviours 15c–15e)** In a window of the test harness, with a search answered
  without a language server: no usages deletes the function as one undo step; with usages the
  list opens with Cancel selected, Enter deletes nothing, "Delete anyway" deletes; a
  declaration in a file that is not open is written with Local History; a file changed
  meanwhile is left alone (`ninth_package_keymap.rs::safe_delete_*`). MCP `safe_delete` with a
  plan answered without a server is checked by its unit test; with rust-analyzer: not checked.
- **AK-22 (v12, behaviour 16)** Mentions by whole words outside the file itself, generic names
  skipped (`safe_delete::tests::mentions_are_whole_words_in_other_files`); in a window ⌘⌦ in
  the tree on an unmentioned file asks the Delete question, on a mentioned file opens the list
  and "Delete anyway" deletes (`ninth_package_keymap.rs::safe_delete_in_the_project_tree`).
- **AK-23 (v13, behaviour 3b)** `lsp::tests::formatting_options_follow_the_file`: the two jdtls
  settings of a Java file indented by two spaces, of one indented by tabs and of one without
  indented lines (four spaces). Headless with jdtls (OBS-0072): Extract Variable, Field, Method and
  Constant in a file indented by four spaces write no tab.
- **AK-24 (v13, behaviour 8d)** `actions::tests::{fields_and_parameters_introduce_a_name,
  extractions_introduce_a_name}`: jdtls's field and parameter `i` are found at their declaration;
  "Extract to field" and "Introduce Parameter..." introduce a name, "Create parameter 'rate'" does
  not. Headless with jdtls (OBS-0072): `apply_code_action` returns `new_name` `i` for both.
- **AK-25 (v13, behaviour 7a)** `actions::tests::missing_refactorings_say_so`: the message for
  Extract Field and Introduce Parameter in Python and Introduce Parameter in JavaScript and
  TypeScript, none for the refactorings that exist, the same text as the own refusal.

## Out of scope
IntelliJ's own refactoring engine (the dialogs with name suggestions, occurrences "Replace all", a list
of suggested names; v12: Safe Delete's search in comments and strings, deleting overriding methods and
parameters along), Change Signature, Move, Pull Members Up, refactorings the server does not
offer (clangd has no Inline).

## Known deviations from the reference
The reference computes these refactorings itself, asks for the name in place and offers to replace all
occurrences; IDECraft applies what the language server offers, with the server's name for the new
element and then asks for the name in the rename popup (behaviour 8), placed at the name (v5), rather than
editing it in place in the text. The reference extracts any expression; IDECraft can only extract what the server accepts and
then says what the server refused. Compared side by side for Java in OBS-0027 (2026-11-13): the
findings J1 (default name of a constant) and J2 (no choice for Inline Method) are answered in
SPEC-0122 v8, with what still differs named there.
