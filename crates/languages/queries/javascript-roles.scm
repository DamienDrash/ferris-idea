; IDECraft's colour roles for JavaScript and TypeScript (SPEC-0027 v6). Placed before the
; grammar's own query: for the same node the earlier pattern wins.

; `constructor` is a keyword, other method and function names are declarations.
(method_definition
  name: (property_identifier) @keyword
  (#eq? @keyword "constructor"))
(method_definition
  name: (property_identifier) @function)
(function_declaration
  name: (identifier) @function)
(generator_function_declaration
  name: (identifier) @function)
(function_expression
  name: (identifier) @function)

; Calls are plain text, as in the reference's default.
(call_expression
  function: (identifier) @function.call)
(call_expression
  function: (member_expression
    property: (property_identifier) @function.call))

; Global objects.
((identifier) @variable.global.js
  (#match? @variable.global.js "^(Math|JSON|console|window|document|globalThis)$"))

; Regular expressions, slashes and flags included.
(regex) @string.regex.js
(regex
  "/" @string.regex.js)
(regex
  (regex_pattern) @string.regex.js)
(regex
  (regex_flags) @string.regex.js)
