; Ferris IDEA's colour roles for C++ (SPEC-0027 v13), after the C roles and before the grammar's own
; queries: methods declared in a class and qualified names.

; Methods declared or defined in a class body, and `Class::method` defined outside it.
(function_declarator
  declarator: (field_identifier) @function)
(function_declarator
  declarator: (qualified_identifier
    name: (identifier) @function))
(function_declarator
  declarator: (destructor_name
    (identifier) @function))

; Calls of qualified names (`std::move(…)`) and templates (`make_unique<T>(…)`).
(call_expression
  function: (qualified_identifier
    name: (identifier) @function.call))
(call_expression
  function: (template_function
    name: (identifier) @function.call))
