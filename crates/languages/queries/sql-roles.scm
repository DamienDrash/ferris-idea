; Ferris IDEA's colour roles for SQL (SPEC-0027 v6). Placed before the grammar's own query: for the
; same node the earlier pattern wins. Data types, `NULL`, `DEFAULT`, `ASC` and `DESC` become
; keywords in `SyntaxDocument`; aliases where they are used get their role there too.

; Column names in a column definition.
(column_definition
  name: (identifier) @property.column.sql)

; Built-in functions (standard SQL and the common ones of the big databases).
(invocation
  (object_reference
    name: (identifier) @function.builtin.sql
    (#match? @function.builtin.sql "^(?i)(count|sum|min|max|avg|coalesce|nullif|greatest|least|now|lower|upper|length|char_length|character_length|octet_length|substring|substr|trim|ltrim|rtrim|replace|concat|position|abs|round|floor|ceil|ceiling|mod|power|sqrt|exp|ln|log|sign|random|extract|date_trunc|to_char|to_date|row_number|rank|dense_rank|ntile|lag|lead|first_value|last_value|string_agg|array_agg|group_concat|ifnull|isnull|nvl)$")))
((identifier) @function.builtin.sql
  (#match? @function.builtin.sql "^(?i)(current_date|current_time|current_timestamp|localtime|localtimestamp|current_user|session_user)$"))
(keyword_current_timestamp) @function.builtin.sql

; Numbers (the grammar's own number pattern does not match).
((literal) @number
  (#match? @number "^[-+]?([0-9]+(\\.[0-9]*)?|\\.[0-9]+)([eE][-+]?[0-9]+)?$"))
