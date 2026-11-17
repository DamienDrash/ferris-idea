; IDECraft's colour roles for C, also used for C++ (SPEC-0027 v13). Placed before the grammar's own
; query: for the same node the earlier pattern wins, and the grammar's query names every
; identifier a variable first, so its own function and constant patterns never apply.

; Names of declared and defined functions.
(function_declarator
  declarator: (identifier) @function)
(function_declarator
  declarator: (parenthesized_declarator
    (identifier) @function))

; Called functions, also through a field (`ops.run()`, `p->run()`).
(call_expression
  function: (identifier) @function.call)
(call_expression
  function: (field_expression
    field: (field_identifier) @function.call))

; Macros and functions defined with `#define NAME(…)`.
(preproc_function_def
  name: (identifier) @function.macro)

; Names written in capitals are constants (`MAX_SIZE`, `EOF`).
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))
