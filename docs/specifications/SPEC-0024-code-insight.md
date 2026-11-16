# SPEC-0024: Code insight over LSP — hover, navigation, completion, symbols

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** STD (LSP 3.17: `general.positionEncodings` / `positionEncoding`, `textDocument/hover`,
  `definition`, `references`, `completion`, `documentSymbol`; `Location`, `LocationLink`,
  `MarkupContent`, `MarkedString`, `CompletionList`, `DocumentSymbol`, `SymbolInformation`),
  SPEC-0023 (client)
- **Feature matrix rows:** F-031, F-034

## Purpose
Answer "what is this", "where is it defined/used", "what can I type here" and "what is in this file" for a
position in a file, with positions that mean the same to every caller whatever encoding the server uses.

## Behaviour
1. Positions in this API are 0-based lines and 0-based characters counted in Unicode scalar values.
   The client offers the position encodings `utf-8`, `utf-16` and `utf-32` and converts to and from the
   one the server chooses (`utf-16` when it names none). Positions past the end of a line are clamped to
   the line end. [STD, improvement: callers never deal with UTF-16 offsets]
2. Hover: the text of the hover (markup value, or marked strings joined by blank lines) and its range, or
   nothing. [STD]
3. Definition and references: a list of locations (file path, range); `Location`, `Location[]` and
   `LocationLink[]` answers are all accepted; references include the declaration. Ranges in other files
   are converted with that file's text. [STD]
4. Completion: items with label, kind name (`function`, `variable`, … per LSP numbering), detail and the
   text to insert (text edit, insert text, or label); a list or an array answer are both accepted; at most
   `limit` items in server order. [STD]
5. Document symbols: name, kind name, detail, range start and nesting depth, in document order
   (hierarchical answers flattened depth-first; flat answers with depth 0). [STD]
6. Each query starts the language's server (SPEC-0023), opens the file, asks, and shuts the server down.
   [GEN]
7. MCP tools `hover_symbol`, `find_symbol_locations` (kind `definition` | `references`), `complete_code`,
   `document_symbols`; paths are workspace-relative. [ADR-0008]

## Acceptance criteria
- **AK-1** Position conversion between characters and utf-8/utf-16/utf-32 offsets for ASCII, `ü`, `€` and
  an emoji, including clamping.
- **AK-2** Parsing of hover (markup, marked string list), locations (single, list, links), completion
  (list, array, text edit) and symbols (hierarchical, flat).
- **AK-3** With `clangd`: in a C file with a function `add` defined once and called twice, hover on a call
  mentions `add`; definition from a call points at the definition line; references give three locations;
  completion after `ad` contains `add`; symbols list `add` and `main`.
- **AK-4** The MCP tools return these results for a workspace file and refuse paths outside it.

## Out of scope
Keeping servers running between queries, signature help, code actions, rename (later M4/M6 steps),
workspace-wide symbol search, semantic tokens.

## Known deviations from the reference
The reference provides these features with its own language engines; results are compared per language
in the guided session.
