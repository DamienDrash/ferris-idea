; Highlight query for tree-sitter-kotlin-ng, which ships none (own query, SPEC-0027 v4).

[
  "abstract" "actual" "annotation" "as" "by" "catch" "class" "companion" "const" "constructor"
  "crossinline" "data" "do" "else" "enum" "expect" "external" "final" "finally" "for" "fun" "if"
  "import" "in" "infix" "init" "inline" "inner" "interface" "internal" "is" "lateinit" "noinline"
  "object" "open" "operator" "out" "override" "package" "private" "protected" "public" "return"
  "sealed" "suspend" "tailrec" "this" "throw" "try" "typealias" "val" "value" "var" "vararg" "when"
  "where" "while" "super"
] @keyword

[(line_comment) (block_comment)] @comment

[(string_literal) (multiline_string_literal) (character_literal)] @string

[(number_literal) (float_literal)] @number


(user_type (identifier) @type)

(annotation) @attribute

; Names by their role (SPEC-0027 v8), the roles the language server's tokens give them too
; (SPEC-0290 v3): a function where it is declared and where it is called, a property where it is
; declared and where it is read through a receiver. For the same node the earlier pattern wins.
(function_declaration name: (identifier) @function)
(call_expression . (identifier) @function.call)
(call_expression . (navigation_expression (identifier) @function.call .))
(class_declaration name: (identifier) @type)

; A `val`/`var` of a primary constructor is a property; other parameters keep the text colour.
(class_parameter ["val" "var"] (identifier) @property)

; A `val`/`var` of a file or a class is a property; one in a body is a local variable and keeps
; the text colour, like its uses.
(source_file (property_declaration (variable_declaration (identifier) @property)))
(class_body (property_declaration (variable_declaration (identifier) @property)))
(enum_class_body (property_declaration (variable_declaration (identifier) @property)))

; Member access: the name after the dot, unless it starts like a type or a constant.
((navigation_expression (_) (identifier) @property .)
 (#match? @property "^[a-z_]"))
