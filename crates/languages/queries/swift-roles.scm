; Ferris IDEA's colour roles for Swift (SPEC-0027 v13). Placed before the grammar's own query: for the
; same node the earlier pattern wins, and the grammar's query names every identifier a variable
; first, so its function patterns never apply.

(function_declaration
  name: (simple_identifier) @function)
(protocol_function_declaration
  name: (simple_identifier) @function)

(call_expression
  (simple_identifier) @function.call)
(call_expression
  (navigation_expression
    (navigation_suffix
      (simple_identifier) @function.call)))
