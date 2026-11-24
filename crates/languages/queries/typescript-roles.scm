; Ferris IDEA's colour roles for TypeScript only (SPEC-0027 v6), before `javascript-roles.scm`.

; Method and function declarations in interfaces, abstract classes and overloads.
(method_signature
  name: (property_identifier) @function)
(abstract_method_signature
  name: (property_identifier) @function)
(function_signature
  name: (identifier) @function)

; Type parameters where declared; their uses get the role from `SyntaxDocument`.
(type_parameter
  name: (type_identifier) @type.parameter)
