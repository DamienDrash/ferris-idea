; Ferris IDEA's colour roles for Python (SPEC-0027 v6), with the Python plugin's keys (`PY.*`, see
; `ferris_idea_ui::theme::capture_key`). Placed before the grammar's own query: for the same node the
; earlier pattern wins. `self`/`cls` as first parameter get their role from `SyntaxDocument`.

; Decorators: the `@` and the decorator's name.
(decorator) @attribute.python
(decorator
  (identifier) @attribute.python)
(decorator
  (attribute
    object: (identifier) @attribute.python
    attribute: (identifier) @attribute.python))
(decorator
  (call
    function: (identifier) @attribute.python))
(decorator
  (call
    function: (attribute
      object: (identifier) @attribute.python
      attribute: (identifier) @attribute.python)))

; Names in `def`.
(function_definition
  name: (identifier) @function.python)

; Docstrings: the first string statement of a module, class or function.
(module
  .
  (expression_statement
    (string) @comment.doc.python))
(class_definition
  body: (block
    .
    (expression_statement
      (string) @comment.doc.python)))
(function_definition
  body: (block
    .
    (expression_statement
      (string) @comment.doc.python)))
