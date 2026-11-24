# SPEC-0113: Change Signature

- **Version:** 2 · **Date:** 2026-11-05 (v2: a reference that covers the whole qualified call, jdtls; hand test Nr. 40, D26)
- **Sources:** GEN (Change Signature reorders, renames, removes and adds parameters of a function and
  updates every call; new parameters get a value for existing calls; F-073), OSS (action id
  `ChangeSignature`, Ctrl+F6 in the default keymap, ⌘F6 on macOS through the inherited Ctrl/⌘ swap of
  SPEC-0028 v2), STD (LSP `textDocument/definition` and `textDocument/references`), SPEC-0040
  (edits per file), SPEC-0074 (applying edits in editors and on disk), ADR-0008
- **Feature matrix rows:** F-073

## Purpose
Change a function's parameters once and have the declaration and every call follow — with Ferris IDEA's
own logic, so it works with every language server that finds references, not only with servers that
offer such a refactoring.

## Behaviour
1. **Opening:** `ChangeSignature` (⌘F6, Ctrl+F6) on a function's name — its declaration or a call —
   asks the language server for the declaration and opens a field with the current parameter list
   (without a receiver: Rust `self`, Python `self`/`cls`). [GEN, OSS, STD]
2. **Editing:** the user edits the list as text. A parameter keeps its arguments by name, so moving a
   parameter reorders the arguments, deleting it drops them, and changing its type or modifiers only
   changes the declaration. A parameter with a new name needs `= value`, the value every existing
   call gets; `= value` does not go into the declaration. A missing value, an empty name or a duplicate
   is refused with the reason. [GEN]
3. **Rewriting:** the declaration's list and every other declaration among the references (a C
   prototype, a header) get the new list; each call gets its arguments in the new order with the new
   values. Calls nested in arguments are rewritten too. Named arguments (`name = value`, `name: value`)
   stay with their parameter. A receiver passed as the first argument (`Type::f(x, …)`) stays first.
   Parameters are told apart by language: `name: Type` (Rust, Python, TypeScript, Kotlin …), `Type
   name` (C, C++, Java, PHP), `name Type` (Go). [GEN]
   **Wide references (v2, D26):** a reference is a call when `(` follows it, or, when the server's
   reference covers more than the name — jdtls gives the whole invocation `r.addiere(1, "abc")` —
   when `(` follows the function's name as a word inside it. A method reference (`this::addiere`)
   or another function whose name starts the same (`addiereAlle`) stays no call. [STD, GEN]
4. **Left alone:** references that are not calls (a function passed as a value, an import) and calls
   whose arguments do not fit the old list (too many, a missing one) are not changed; they are listed
   with the reason. [GEN]
5. **Applying:** open files change through their editors as one undo step each, other files on disk,
   as Rename does (SPEC-0074). The status bar says how many files changed and how many calls were left
   alone, with the first reason. [GEN]
6. **MCP:** `change_signature` (path, line, character, optional `parameters`, `dry_run` default true)
   returns the current list, or the edits per file and the calls left alone, and writes them with
   `dry_run=false`. [ADR-0008]

## Acceptance criteria
- **AK-1** Lists split at top-level commas with strings, brackets, generics and lifetimes; parameter
  names in Rust, C, C++, Java, PHP, Go, Python, Kotlin and JavaScript; receivers; mapping by name with
  values, duplicates and missing values refused; named arguments; declarations told from calls (unit
  tests `signature::tests`).
- **AK-2** With clangd: a prototype, the definition and nested calls are rewritten and the file
  compiles (`crates/languages/tests/it/clangd.rs`: `clangd_change_signature`).
- **AK-3** MCP: reading the list, swapping parameters, and a new parameter without a value refused
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-4** Window self-test with clangd: ⌘F6 on a call shows the list; a new list with a value
  rewrites the declaration and both calls in the editor.
- **AK-6 (v2)** With jdtls in a Maven project, both qualified calls of one overload of `addiere`
  follow its new list and the other overload's call stays (live test
  `tests/java_signature.rs::qualified_calls_of_an_overload_follow`); the call is found after a wide
  reference, not for a method reference or a longer name (unit test
  `signature::tests::calls_are_found_after_a_wide_reference`).
- **AK-5** Hand test (round 4): a Java method with overloads and a Rust method with `self`, next to the
  reference.

## Out of scope
Overriding methods and implementations of an interface (only the function the server names as the
declaration and its references change), default values in the declaration, changing the return type
through the dialog, propagating new parameters to callers, a preview tree.

## Known deviations from the reference
The parameters are edited as one line of text instead of a table with buttons. Calls that do not fit
are listed and left unchanged instead of being changed to something that does not compile (IMP-0041).
Overrides are not followed yet. Not compared side by side yet.
