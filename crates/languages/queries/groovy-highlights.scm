; Highlight query for tree-sitter-groovy, which ships none (own query, SPEC-0027 v3).

[
  "abstract" "as" "assert" "break" "case" "catch" "class" "continue" "def" "default" "do" "else"
  "enum" "extends" "final" "finally" "for" "if" "implements" "import" "in" "instanceof"
  "interface" "new" "package" "private" "protected" "public" "return" "static" "switch"
  "synchronized" "throw" "throws" "transient" "try" "volatile" "while"
] @keyword

[(line_comment) (block_comment)] @comment

[(string_literal) (character_literal)] @string
(string_interpolation) @embedded

[
  (decimal_integer_literal) (hex_integer_literal) (octal_integer_literal)
  (binary_integer_literal) (decimal_floating_point_literal) (hex_floating_point_literal)
] @number

[(true) (false) (null_literal)] @constant.builtin

[(integral_type) (floating_point_type) (boolean_type) (void_type)] @type.builtin
(type_identifier) @type

(annotation name: (identifier) @attribute)
(marker_annotation name: (identifier) @attribute)

(method_declaration name: (identifier) @function.method)
(method_invocation name: (identifier) @function.method)
(class_declaration name: (identifier) @type)
