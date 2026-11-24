; Ferris IDEA's colour roles for Lua (SPEC-0027 v13). Placed before the grammar's own query: for the
; same node the earlier pattern wins, and the grammar's query names every identifier a variable
; first, so its function and parameter patterns never apply.

(function_declaration
  name: [
    (identifier) @function
    (dot_index_expression
      field: (identifier) @function)
    (method_index_expression
      method: (identifier) @function)
  ])

(function_call
  name: [
    (identifier) @function.call
    (dot_index_expression
      field: (identifier) @function.call)
    (method_index_expression
      method: (identifier) @function.call)
  ])

(parameters
  (identifier) @variable.parameter)
