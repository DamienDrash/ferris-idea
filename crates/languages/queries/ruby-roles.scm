; IDECraft's colour roles for Ruby (SPEC-0027 v13). Placed before the grammar's own query: for the
; same node the earlier pattern wins, and the grammar's query names every identifier a variable
; first, so its method patterns never apply.

; Names of defined methods.
(method
  name: [
    (identifier)
    (constant)
  ] @function)
(singleton_method
  name: [
    (identifier)
    (constant)
  ] @function)

; Called methods with a receiver or arguments (`g.greet`, `puts "a"`).
(call
  method: (identifier) @function.call)

; Visibility.
((identifier) @keyword
  (#match? @keyword "^(private|protected|public)$"))
