; IDECraft's colour roles for R (SPEC-0027 v13). Placed before the grammar's own query: for the
; same node the earlier pattern wins, and the grammar's query names every identifier a variable
; first, so its function patterns never apply.

(binary_operator
  lhs: (identifier) @function
  operator: "<-"
  rhs: (function_definition))
(binary_operator
  lhs: (identifier) @function
  operator: "="
  rhs: (function_definition))

(call
  function: (identifier) @function.call)

(parameters
  (parameter
    name: (identifier) @variable.parameter))
