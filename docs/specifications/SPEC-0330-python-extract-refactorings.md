# SPEC-0330: Extract Variable, Extract Method, Extract Constant and Inline Variable for Python

- **Version:** 4 · **Date:** 2026-11-13 (v4: Inline Variable and Extract Constant, behaviours 17
  to 25, AK-13 to AK-19; v3 of 2026-11-12: Extract Method writes the types it knows for certain
  on the new function, behaviour 16, AK-12; v2: Extract Variable refuses when it would change the
  order of evaluation inside the statement, behaviour 8, AK-11; the limit of v1 is gone)
- **Sources:** OBS-0034 item 47 (L13: in a Python file the reference extracts a selected expression
  into a variable above the statement and offers a name to type over; Ferris IDEA did nothing, since
  pyright offers no extract refactoring) and item 52 (L15: for a selected statement in `def
  main(argv: list[str]) -> int:` that uses `argv` and a local list of `Item` objects, the reference
  writes both parameters of the new function with their types, `list[str]` and `list[Item]`, and
  no return type; it offers a box for parameter and return types in its dialog, seen ticked;
  Ferris IDEA wrote the parameters without types), an own headless check of Ferris IDEA on 2026-11-13
  (no observation of the reference: in a Python file `Inline` and `IntroduceConstant` did nothing,
  since pyright offers neither), GEN (what Extract Variable, Extract Method, Extract Constant and
  Inline Variable do; which selections they accept; that inlining and hoisting must not change
  what a program does), STD (The Python Language Reference: "Operator precedence", "Formatted
  string literals", "Assignment statements"; PEP 8 "Constants"; PEP 701; "Naming and binding", "Evaluation
  order", "Boolean operations", "Conditional expressions", "Comparisons", "Displays for lists, sets
  and dictionaries", "Function definitions", "Annotated assignment statements", "Coroutines"; PEP
  484, PEP 563, PEP 585, PEP 604; LSP code actions and `textDocument/hover`), SPEC-0095 (refactoring
  shortcuts, the name asked for right after extracting), SPEC-0041 (code actions), SPEC-0320 (own
  refactorings where the server has none), SPEC-0098 v6 (a method that does not use `self`)
- **Feature matrix rows:** F-073, F-035

## Purpose
Extract a selected expression into a variable or into a module-level constant, extract selected
statements or an expression into a method or function, and inline a local variable, in Python
files, although the Python language servers offer none of these.

## Behaviour
**Where it applies.** Python files (`.py`, `.pyi`, `.pyw`). The syntax tree of the editor's text
decides what is extracted or inlined and how; no language server is needed. Only the types of
behaviour 16 may come from the server. A refusal changes nothing and names its reason.

1. **Own actions:** `IntroduceVariable`, `ExtractMethod` and `Refactorings.QuickListPopupAction`
   (SPEC-0095 behaviours 1 and 4) first ask the language server as for every language. When the
   server offers no matching action at the place, or cannot be asked (not installed, failed),
   Ferris IDEA offers its own actions titled `Extract variable` and `Extract method` (kind
   `refactor.extract`). They are found, applied, undone and followed by the name popup exactly as
   a server's action: SPEC-0095 behaviours 3, 5, 8, 9 and 10 hold unchanged. An action with one of
   these titles that the server does offer is the server's. [OBS-0034 item 47, GEN]
2. **Selection:** leading and trailing white space of the selection does not count. Extract
   Variable needs exactly one complete expression. Extract Method needs one or more complete
   statements of one block, or exactly one complete expression; a selection that is a whole
   statement counts as a statement. Comment lines the selection starts or ends with, and comments
   between the statements, move with them. What stands between the parentheses of a generator
   expression means the generator expression, also where the parentheses are a call's
   (`sum(x for x in xs)`). [GEN]
3. **Bare caret:** without a selection the expressions around the caret are offered from inside
   out, up to the enclosing statement, at most 8: those the refactoring does not refuse, without
   plain names and without the callee of a call (`s.area` of `s.area()`). Extract Method also
   offers the enclosing statement. One place is applied at once, several are listed (SPEC-0095
   behaviour 6). Refactor This with a bare caret keeps its entries of SPEC-0095 behaviour 4a. [GEN]
4. **Not an expression of its own:** refused for both refactorings: part of an expression (`b + c`
   of `a + b + c`, part of a string), a name after a dot, the name of a keyword argument, a
   parameter, an import, a starred or `**` argument, a slice, a key-value pair; an expression that
   contains `yield` or an assignment expression (`:=`); and an expression in one of these places:
   the target of an assignment, of `for`, of `with … as`, of `del` or of `except … as`, also inside
   such a target (`a[f(i)] = 1`); a default value of a parameter; an annotation; a decorator; the
   header of a `def` or `class` (base classes, return type); a `case` pattern or guard. Code with
   syntax errors in the top-level statement around the selection is refused. [GEN, STD]

**Extract Variable.**

5. **Edit:** a new line `name = <expression>` is put directly before the statement that contains
   the expression, with that statement's indentation and the file's line ending, and the selection
   is replaced by `name`. An expression over several lines that is not itself in parentheses is
   put in parentheses in the new line. A generator expression that is the only argument of a call
   keeps the call's parentheses (`sum(name)`). Where the name would touch a word (`not(x)`), a
   space is added. When the expression is a whole expression statement, that statement becomes
   `name = <expression>` and no second line is written. Only the selected occurrence is replaced
   (deviation). [OBS-0034 item 47, GEN]
6. **Name:** from the expression: of a call the called name (a leading `get_` dropped), of an
   attribute the attribute name, of a subscript what its value suggests, of an operator expression
   what its right side suggests, else its left side; parentheses, `await`, `not` and signs are
   looked through. A plain variable name suggests nothing (`n * 2` would only give `n1`).
   `CamelCase` becomes `camel_case`, leading underscores are dropped. Without a suggestion:
   `value`. The name is made unique among the names used in the enclosing function (at
   module level: in the file), names after a dot not counted: a Python keyword or built-in name
   gets a trailing `_` (`type_`), any other taken name a number (`items1`). `self.price *
   self.qty` gives `qty`. [OBS-0034 item 47, GEN]
7. **Refused, because the new line would be evaluated at another time, another number of times or
   in another scope** (each with its own reason): [STD]
   - inside the body of a `lambda`;
   - inside a comprehension or generator expression, except in the iterable of its first `for`
     (which is evaluated once, where the comprehension stands);
   - on the right side of `and` or `or`, in the value or the `else` branch of a conditional
     expression (the condition itself is allowed), in the third or a later operand of a chained
     comparison (`a < b < c`): evaluated only sometimes;
   - in the condition of `while` (evaluated before every round) and of `elif` (only when the
     conditions before are false);
   - in an `except` clause's expression, in an `assert` (condition and message), in the second or
     a later item of a `with` statement;
   - the statement stands directly in a class body (the name would become a class attribute);
   - the statement does not start its line (`if x: return f(y)`, the second statement of
     `a = 1; b = f(a)`);
   - the expression reads a name that an assignment expression binds earlier in the same
     statement.
8. **Refused, because the order of evaluation would change** (v2): the new line runs before
   everything else of its statement. Extract Variable therefore refuses when a part of the
   statement that Python evaluates before the expression can have side effects and the expression
   itself contains a call, `await` or `yield`; the reason names that part, as in `it would then run
   before `first()`, which comes first in the statement and can have side effects` for `total =
   first() + second()` with `second()` selected. [STD]
   - Parts evaluated before: Python's order, left to right through operands, the called
     expression before its arguments, arguments and display items in order, a key before its
     value, the value before the subscript; except that the right side of an assignment comes
     before its targets and annotation (`d[k()] = make()` runs `make()` first), the iterable of
     `for` and of a comprehension's first `for` before the target and the element, and the
     condition of a conditional expression before its branches.
   - Can have side effects: a call, `await`, `yield` or an assignment expression (`:=`) anywhere in
     that part, except inside the body of a `lambda`. Reading an attribute or a subscript does not
     count.
   - Allowed: an expression without a call, `await` or `yield` after any part (`total = first() +
     self.price * self.qty`), and a call after parts without side effects (`obj.method(g())`,
     `a[0] + g()`).
   - An expression that reads a name bound by `:=` in the statement is already refused by
     behaviour 7, and an expression that holds `:=` by behaviour 4.
   - Remaining limit: an expression without a call is taken to be unaffected by the calls before
     it. If such a call changes what the expression reads (`self.first() + self.price * self.qty`
     where `first()` changes `self.qty`), the result differs. Python cannot tell this from the
     syntax; the rule follows the coordinator's decision of 2026-11-12 to keep such pure
     expressions extractable. [GEN]

**Extract Method.**

9. **Parameters and results:** names are followed by Python's scoping rules, nested functions,
   lambdas and comprehensions included. A *local* name is one bound in an enclosing function,
   lambda or comprehension (parameter, assignment, `for`, `with … as`, `except … as`, `import`,
   `def`, `class`, `:=`) and not declared `global` there. [STD]
   - **Parameter:** a local name the selection reads at a place where the selection itself has
     not assigned it on every path before, in the order of the first such read.
   - **Result:** a name the selection assigns that may be read afterwards before it is assigned
     again: in the statements that follow, up to the end of the function, and, inside a loop, in
     the whole loop. Results are returned in the order of their first assignment and assigned at
     the call: `a, b = extracted(x)`.
   - A result the selection assigns only on some paths is also passed in as a parameter when it
     has a value before the selection; a name of an enclosing function further out counts as
     having a value.
   - At module level the names the selection assigns would become locals of the new function: one
     assigned on every path is returned and assigned again at the call, so it stays a module name;
     one read before its assignment is a parameter; one assigned only on some paths (a loop
     variable) is dropped when nothing after the selection reads it.
10. **Kind and place of the new function:** [GEN, SPEC-0098 v6]
    - The selection is directly in a method of a class (not `@staticmethod`): a new method of that
      class, directly after the method, called through the method's first parameter:
      `self.extracted(x)`. It has `self` also when the selection does not use it: code taken out
      of a method stays with its class, and the weak warning of SPEC-0098 v6 then offers Make
      method static. In a `@classmethod` the new method is a `@classmethod` called through `cls`.
    - Otherwise (module level, a function, a nested function, a `@staticmethod`): a function at
      module level, directly before the top-level statement that contains the selection (above
      comment lines directly attached to it), with two empty lines after it and, unless an empty
      line or the start of the file is above, before it. This also holds for the methods
      `__new__`, `__init_subclass__` and `__class_getitem__`, whose first parameter is no
      instance.
    - Name: `extracted`, with a number when the file already has that word.
    - The body is the selected text with its indentation moved; lines inside a string literal over
      several lines are not touched. An expression becomes `return <expression>`. The indentation
      step is the one the file uses, else four spaces.
11. **`return`:** a selection in which every path ends in `return` or `raise` is called as `return
    extracted(…)` when it contains a `return`. Every path ends so when the last statement is
    `return` or `raise`, or an `if` with `else` whose branches all end so; a `try` with handlers
    ends so when its body and every handler do. A `with` block never counts as ending so, since
    its context manager may swallow an error. A selection that always ends in `raise` and has no
    `return` is called as a plain statement. [GEN]
12. **`await`:** a selection with `await`, `async for` or `async with` of its own (not of a nested
    function) gives an `async def`, called with `await`; inside an expression as `(await
    extracted(…))`. [STD]
13. **Refused, each with its own reason; wrong code is never preferred to a refusal:** [GEN, STD]
    - incomplete statements, statements of different blocks, a `case` clause, a selection that
      shares its first or last line with other code (a comment behind it is allowed and stays);
    - a selection directly in a class body;
    - `return` on only some paths (behaviour 11), and `return` at module level;
    - `yield` or `yield from`;
    - `break` or `continue` whose loop is outside the selection;
    - a `global` or `nonlocal` statement; an assignment to a name the function declares `global`
      or `nonlocal`; `del` of a name; `from __future__ import`; `from … import *`;
    - a call of `locals`, `vars`, `eval`, `exec` or `dir` (they see the local names);
    - a `match` statement anywhere in the enclosing function (its captures are not followed);
    - a parameter that may or may not have a value when the selection starts (assigned before
      only on some paths, or only in an earlier round of an enclosing loop);
    - the selection reads a local name of its function that is assigned only after the selection
      (also from a nested function: the new function would look it up in the module);
    - a result that the selection assigns only on some paths and that has no value before;
    - a name the selection assigns is read in an `except` or `finally` block of a `try` around the
      selection (after an error inside the selection, or after its `return`, it would not have
      been assigned);
    - the selection is in the body of a `try` with `except` clauses and assigns a name that is
      needed after it, unless it assigns that name once, in its last statement, and that is a
      plain assignment to names (after an error in a later line the handler would go on without
      the value);
    - a name the selection assigns is used by a nested function, lambda or generator expression
      outside the selection; at module level: by any function, lambda or generator expression of
      the file outside the selection (they read the module's name directly);
    - a nested function, lambda or generator expression inside the selection uses a name that is a
      result, or a local name that is assigned again after the selection (or elsewhere in an
      enclosing loop);
    - the selection assigns the method's first parameter (`self`);
    - where the new function is a module-level function: code that uses `super` or `__class__`,
      and code from inside a class that uses a private name `__x` (mangled inside the class only).
14. **Messages:** a refusal reads "Cannot extract `<selection>` into a variable: <reason>." (or
    "into a method"), the selection shown as in SPEC-0095 behaviour 7; a bare caret without any
    expression: "There is no expression at the caret to extract." The status bar and MCP
    `code_actions` (`message`) show it instead of the server's refusal of SPEC-0095 behaviour 7.
    `apply_code_action` with one of the own titles on a refused selection fails with the same
    text. [GEN]
15. **MCP:** `code_actions` with `refactoring` `extract_variable`, `extract_method` or `any` lists
    the own actions as targets; `apply_code_action` with the title applies them and reports
    `new_name` (SPEC-0095 behaviour 9). Both work without a Python language server. [ADR-0008]

16. **Types of the new function** (v3): Extract Method writes a type on each parameter of the new
    function, and a return type, where the type is known for certain and can be written with
    names the file already has. A parameter whose type is not known that way stays without one;
    annotated and plain parameters may stand side by side. Nothing is guessed, no import is added,
    and the order of the parameters (behaviour 9) does not change. There is no setting: known
    types are always written (deviation). [OBS-0034 item 52, STD]
    - **Sources of a parameter's type, the first that applies:**
      1. the name is a parameter of the function the selection is in and has an annotation there:
         the annotation's text as written (`*args` and `**kwargs` excepted, whose annotation is the
         type of one element);
      2. the name is a variable of that function (at module level: of the module) with an annotated
         assignment or declaration (`x: int = …`, `x: int`), is not one of its parameters, and all
         its annotations read the same: that text;
      3. the Python language server (pyright or basedpyright), asked with `textDocument/hover` on
         the name's first read in the selection, on the editor's text, saved or not. Only an
         answer of the form `(variable|parameter|constant) name: type` counts. The read must be
         the first use of the name the selection evaluates: when it stands in the value of a
         conditional expression or in the element of a comprehension that uses the name again
         (in the condition, a `for` or an `if`), the server may already have narrowed the type
         there, and the parameter stays plain.
    - **A copied annotation (1, 2) is written only when it means the same at the new place.**
      For a new method (behaviour 10) a parameter's annotation does, unless it uses a type
      parameter of the method itself. Otherwise every name in it must be a built-in type or
      bound at the top level of the file, and must not be a local name, a parameter or a type
      parameter of an enclosing function, nor a name bound in the body of an enclosing class
      (a variable's annotation does not see the class body, the new method's signature would;
      a function outside the class sees none of it). Because Python
      evaluates the annotations of a `def` when it runs, a name outside a string must be bound
      above the top-level statement the selection is in, and not under `if TYPE_CHECKING:`; with
      `from __future__ import annotations` any place in the file will do, as for names inside a
      string annotation (the strings of `Literal[…]` are no names).
    - **A type from the server (3) is written only when all of this holds:** it reads as a type
      made of plain names, subscripts (`list[Item]`, `dict[str, int]`, `tuple[int, ...]`), `None`
      and unions with `|`; no name in it is `Unknown` or `Any` (an unresolved type is never
      written, and `Any` cannot be told from one); no name has a dot (the server's module path
      need not be importable as written); `list`, `dict`, `set`, `frozenset`, `tuple` and `type`
      have their arguments; every other name is a built-in type the file does not bind itself, or
      is bound exactly once at the top level of the file, above the selection's top-level
      statement and not under `if TYPE_CHECKING:` (anywhere with the future import), and is not
      hidden by a local name, a type parameter or a name of an enclosing class body. A literal type is widened:
      `Literal[5]` to `int`, `Literal['a']` to `str`, likewise `bool` and `bytes`; other literals
      (enum members, mixed kinds) are not written. A union with `|` is written only where the file
      shows it can be evaluated: the future import, or a `|` union already written in an
      annotation of a function's signature. Subscripted built-in types assume Python 3.9.
    - **Return type:** only when the new function's body has exactly one `return`, with a value
      (the one Ferris IDEA writes for an expression or for the results, or the selection's own single
      `return`; those of nested functions do not count). Each returned part is typed by itself:
      a name by sources 1 and 2, a result of the selection otherwise by the server's hover on its
      assignment when the selection assigns it exactly once and on every path; a name that is no
      result, an attribute (`a.b`) and a call (`f(x)`, `a.m(x)`) by the server's hover on the name
      or on the called name, from which the declared result type is taken (`… -> type`; a
      function with several signatures, a class, a type variable are not written); `await f(x)`
      takes the result of the coroutine. Operators, literals and every other expression are not
      typed. Several parts give `tuple[A, B]`; when one part is unknown there is no return type. A
      new function that returns nothing gets none, as in the reference.
    - **Without a language server**, with another Python server, or when the server does not
      answer within the wait: sources 1 and 2 only. The extraction itself never fails for a
      missing type.
    - **Entry points:** the window's shortcut and MCP `apply_code_action` ask the server, in the
      session that is open anyway to see whether the server has an extraction of its own
      (behaviour 1); the pure functions on the text (`extract_method`) use sources 1 and 2.

**Inline Variable** (v4).

17. **Own action and place:** `Inline` (SPEC-0095 behaviour 1) first asks the language server as
    for every language. When the server offers no inline action at the place, or cannot be asked,
    Ferris IDEA offers its own action titled `Inline variable` (kind `refactor.inline`). It acts on
    the name the caret is in, directly before or directly behind, or on a selection that is
    exactly one name; the name may be the variable's assignment or any of its reads. The whole
    variable is inlined: every read is replaced by the assigned expression and the assignment is
    removed, as one undo step; there is no name popup afterwards. Refactor This lists the action
    when the caret is on, or the selection is, a name that can be inlined. An action of this
    title that the server does offer is the server's. [GEN]
18. **Which names:** only a local variable of a function (`def`, also a method or a nested
    function) that this function binds exactly once, by a plain statement `name = expression`
    that has its lines to itself. Everything else is refused, saying what the name is and what
    Inline supports: [GEN, STD]
    - a function or method (at its `def`, at a call, as an imported or local function): inlining a
      function is out of scope; a class; an attribute (a name after a dot); the name of a keyword
      argument;
    - a parameter of the function or of a `lambda`; the variable of a comprehension; a name bound
      in a class body; a name the function declares `global` or `nonlocal`;
    - **a module-level name, always** (also one that starts with `_`): other modules can import
      it and every function of the file reads it when it runs, neither of which the file's syntax
      shows; a name the file does not bind at all (a built-in);
    - a variable assigned more than once in its function (the lines are named), counting
      augmented assignments (`+=`), loop targets, `with … as`, `except … as`, imports, `def`,
      `class` and `:=`; a single binding that is one of those instead of an assignment;
    - an assignment to several names (`a, b = …`, `[a] = …`) or a chained one (`a = b = …`); an
      assignment with an annotation (`x: int = …`), and a variable that is also declared without a
      value (`x: int`);
    - an assignment that shares its line with other code (`a = 1; b = 2`, `if c: a = 1`);
    - a value that contains `yield` or an assignment expression (`:=`);
    - code with syntax errors in the top-level statement around the name.
19. **Refused because the program could behave differently** (each with its own reason, naming
    the line): after inlining, the expression is evaluated where the reads stand, that is later,
    and once per read. [STD, GEN]
    - **Reads that are not reached only through the assignment:** every read must stand later in
      the block the assignment is in, directly or inside statements of that block. Refused: a
      read before the assignment; a read outside that block (after the `if`, loop, `try` or
      `with` whose body assigns; in an `except`, `else` or `finally` part); the assignment
      reading its own name; a variable that is never read.
    - **Reads that run at another time:** the variable is used in a nested function, a `lambda`,
      a class body or a generator expression (they read it when they run).
    - **The function looks at its names or changes the variable another way:** a call of
      `locals`, `vars`, `eval`, `exec` or `dir` anywhere in the function; `del` of the variable;
      an attribute or item of the variable is assigned or deleted (`v.x = 1`, `v[0] += 1`); a
      `match` statement anywhere in the function (its captures are not followed).
    - **A value with a call or `await`** (anywhere outside the body of a `lambda`) must run
      exactly once and at the same moment as before. Allowed only when the variable is read
      exactly once; that read stands in the statement itself that follows, not inside a block
      of it (the condition of an `if`, the iterable of a `for`, the first item of a `with` are
      part of the statement itself); it is evaluated exactly once there (the places of behaviour
      7 are refused: right side of `and`/`or`, branch of a conditional expression, later operand
      of a chained comparison, comprehension other than its first iterable, condition of `while`
      or `elif`, `assert`, later `with` item); between the assignment and that statement stand
      only comment lines, `pass` and assignments of expressions without call, `await`, `yield` or
      `:=` to plain names; and nothing that Python evaluates earlier in that statement has side
      effects (behaviour 8).
    - **A value that creates a new object each time** (a list, dict or set display, a
      comprehension, a generator expression, a `lambda`, also as a part of the value) must be read
      exactly once, and not inside a loop or comprehension that starts after the assignment:
      several reads would no longer share one object.
    - **What the value reads must be the same at every read.** Refused when, between the
      assignment and the end of the evaluation of a read, a name the value reads is bound again
      (assignment, augmented assignment, loop target, `with … as`, `except … as`, import, `def`,
      `class`, `:=`) or deleted, or an attribute or item of such a name is assigned or deleted
      (`self.price = 0` for a value that reads `self.price * 2`). For a read inside a loop or
      comprehension that starts after the assignment, the whole loop counts, the lines after the
      read included. Also refused: a nested function declares a name the value reads `nonlocal`
      or `global`; the read stands in a comprehension whose variable has the name of something
      the value reads.
    - **A value that reads an attribute or an item** (`self.count`, `xs[0]`) is refused when, in
      that same stretch, a call is handed the object (as the object of the call or anywhere in
      its arguments: `self.bump()`, `update(xs)`), or an `await` or `yield` stands there: such
      code may change what the attribute or item gives. Calls that are not handed the object
      are taken not to change it.
    - **In an f-string:** a read inside a replacement field is refused when the value contains a
      quote, a backslash, a comment or a line break (not allowed there before Python 3.12), and
      when the field ends in `=` (which prints the expression's text).
20. **Edit:** the lines of the assignment are removed, with a comment behind it; comment lines
    above and empty lines stay. Each read becomes the value's text. Parentheses that held the
    whole value on one line are dropped first (`v = (a + b)`). The value is then put in
    parentheses only where the read's place requires it: [STD]
    - as an operand of an operator that binds tighter, or as tightly on the non-associating
      side: `(a + b) * c`, `c - (a - b)`, `(a < b) == c`, `not (a or b)`, `(-a) ** 2`, but
      `a * b + c`, `a - b - c`, `2 ** -a`;
    - as the object of an attribute, a call or a subscript unless the value is a name, a literal,
      a call, an attribute, a subscript or a bracketed display: `(a + b).real`, `(5).real`,
      `(lambda: a)()`;
    - a conditional expression or a `lambda` inside an operator or a conditional expression;
      after `await` and `*`;
    - a tuple written without parentheses always (`a, b` becomes `(a, b)`); a value over several
      lines that is not bracketed itself;
    - in an f-string field a `lambda` and a value that starts with `{`;
    - none as an argument, an element of a display, a subscript's index, the right side of an
      assignment, after `return`, or as the condition of `if`/`while` and the iterable of `for`.
    A word stays apart from its neighbours by a space.
21. **Messages and MCP:** a refusal reads "Cannot inline `<name>`: <reason>."; where the name is
    not a local variable assigned once, the reason ends with "Inline supports a local variable of
    a function that is assigned once, by `name = expression`"; without a name at the caret:
    "There is no variable at the caret to inline." Status bar, `code_actions` (`message`) and
    `apply_code_action` show it as in behaviours 14 and 15. `code_actions` with `refactoring`
    `inline` gives the variable's name as the one target; `apply_code_action` with the title
    applies it and reports no `new_name`. Both work without a Python language server. [ADR-0008]

**Extract Constant** (v4).

22. **Own action, selection and place:** `IntroduceConstant` falls back to an own action titled
    `Extract constant` (kind `refactor.extract`) exactly as behaviour 1 says for the other two;
    the selection rules of behaviour 2 for one expression and the places of behaviour 4 hold,
    except that a default value of a parameter is allowed. With a bare caret the constant
    expressions around it are offered from inside out as in behaviour 3. The edit puts a line
    `NAME = <expression>` at module level directly before the top-level statement that contains
    the selection, above comment lines directly attached to that statement and above its
    decorators, and replaces the selection by `NAME`; only the selected occurrence. Before a
    top-level `def` or `class` two empty lines follow the new line. The constant goes to module
    level also when the selection is in a function, a method, a nested function or a class body.
    An expression over several lines is put in parentheses in the new line (behaviour 5). The
    name popup opens at the new name as after Extract Variable (SPEC-0095 behaviour 8).
    Refused: a docstring; a top-level statement that does not start its line. [GEN]
23. **What may become a constant:** the new line is evaluated once, when the module is loaded,
    while the selection may run later and many times. So the expression must give the same value
    whenever it is evaluated, and one object that cannot be changed. Allowed, in any
    combination: [STD, GEN]
    - literals: numbers, strings, bytes, `True`, `False`, `None`, `...`; an f-string whose fields
      are allowed themselves;
    - operators on allowed parts: arithmetic, bit, comparison, `in`, `is`, `not`, `and`, `or`,
      conditional expressions, parentheses, tuples;
    - a name the module binds exactly once, directly at its top level (not inside `if`, `try`,
      a loop or `with`), above the top-level statement the selection is in, by an import, a
      `def`, a `class`, or a plain assignment `NAME = <expression>` (also annotated) whose
      expression is allowed by these same rules; and that no function assigns with `global` and
      nothing deletes with `del`;
    - a built-in type (`int`, `str`, `ValueError`, …) or one of the functions below as a value,
      when the file binds no such name and has no `from … import *`;
    - an attribute chain that starts at an imported name, a class of the module or a built-in
      type: `math.pi`, `os.path.sep`, `Color.RED`;
    - a call of one of these built-in functions with allowed arguments, without `*` or `**`:
      `abs`, `bin`, `bool`, `bytes`, `chr`, `complex`, `divmod`, `float`, `frozenset`, `hex`,
      `int`, `len`, `max`, `min`, `oct`, `ord`, `pow`, `range`, `round`, `str`, `sum`, `tuple`.
      They have no side effects, give the same result for the same constant arguments, and their
      result cannot be changed in place. No other call is allowed: not a method (`"a".upper()`),
      not a function of a module (`re.compile(…)`), not `sorted` or `list` (a new list).
    Refused, each with its reason:
    - a name that is a parameter or a local name of a function, `lambda`, comprehension or class
      body around the selection (`self`, a loop variable, a class attribute read in the class
      body);
    - a module name that is bound more than once, only below, inside a compound statement, by a
      tuple assignment, a chained assignment, a loop or `with`; one whose own value is not a
      constant (`N = load()`, `N = []`); one a function assigns with `global`; one that is
      deleted; a name the file does not bind and that is no built-in type or allowed function;
      any unbound name in a file with `from … import *`;
    - a list, dict or set display and a comprehension or generator expression (a new object at
      each evaluation; a constant would share one that can be changed); a `lambda`;
    - a subscript (`N[0]`: the content may change); an attribute of a name bound by an
      assignment or a `def`, or of a computed value (`cfg.debug`, `(1).real`);
    - `await`, and everything behaviour 4 refuses (`yield`, `:=`, targets, annotations,
      decorators, headers).
24. **Name:** in `UPPER_SNAKE_CASE`: what behaviour 6 suggests for the expression, in capitals
    (`math.pi * 2` gives `PI`, `len(NAMES)` gives `LEN`); for a string literal without fields its
    words, when they start with a letter and give at most 30 characters (`"application/json"`
    gives `APPLICATION_JSON`); else `CONSTANT`. The name is made unique among all names of the
    file, attribute names included, by a number: `CONSTANT1`. [STD PEP 8, GEN]
25. **Messages and MCP:** a refusal reads "Cannot extract `<selection>` into a constant:
    <reason>." (behaviour 14). `code_actions` with `refactoring` `extract_constant` lists the
    own action; `apply_code_action` with the title applies it and reports `new_name`. Refactor
    This lists `Extract constant` for a selection it accepts. Both work without a Python language
    server. [ADR-0008]

## Acceptance criteria
- **AK-1** `return self.price * self.qty` with the expression selected gives `qty = self.price *
  self.qty` above and `return qty`; the introduced name is found at the new line (unit test
  `python_extract::tests::the_observed_expression`).
- **AK-2** Edit shapes of behaviour 5: indentation, several lines, generator argument, touching
  words, whole expression statement, CRLF (`python_extract::tests::variable_edit_shapes`).
- **AK-3** Names of behaviour 6 (`python_extract::tests::variable_names`).
- **AK-4** Every refusal of behaviours 4 and 7, each changing nothing and naming its reason, and
  the allowed neighbours (first iterable, condition of a conditional expression, first two
  operands of a comparison, `if` condition, first `with` item)
  (`python_extract::tests::variable_refusals`, `python_extract::tests::variable_allowed_neighbours`).
- **AK-5** Extract Method: parameters, results, in/out names, loop, module level, `return`,
  `await`, method, class method, static method, nested function, indentation, comments and
  strings over several lines (`python_extract::tests::{method_parameters_and_results,
  method_at_module_level, method_return_and_await, method_in_a_class, method_text_shapes}`).
- **AK-6** Every refusal of behaviour 13 (`python_extract::tests::method_refusals`).
- **AK-7** Bare caret and messages (`python_extract::tests::{caret_places, messages}`).
- **AK-8** The results run: for 17 cases of Extract Variable and 40 of Extract Method the
  original and the refactored program print the same with the system's `python3`
  (`crates/languages/tests/it/python_extract.rs::extracted_programs_behave_the_same`; skipped with a
  note when there is no `python3`).
- **AK-9** Through the entry points of window and MCP without a server:
  `actions::refactoring_targets` and `actions::plan_code_action_in` on unsaved text
  (`crates/languages/tests/it/python_extract.rs::through_the_refactoring_entry_points`); MCP
  `code_actions` with `refactoring` and `apply_code_action` with `new_name`, and the refusal
  message (protocol test `python_extract_tools`).
- **AK-11 (v2, behaviour 8)** Refused, naming the earlier part: a call after a call in an operator,
  in arguments, in keyword arguments, in a list, after a subscript holding a call, after the target
  of `+=`, a dict value after its key, a comparison operand, an `await` after an `await`;
  allowed: a pure expression after a call, the first call, a call after an attribute or a
  subscript read, the value of a subscript assignment, an argument of a method, a call after a
  `lambda` (`python_extract::tests::{variable_refusals, variable_allowed_neighbours}`). With
  `python3`: four more allowed cases print the same as before
  (`extracted_programs_behave_the_same`), and two refused ones, extracted by hand, print something
  else (`crates/languages/tests/it/python_extract.rs::refused_reorderings_would_change_the_output`).
- **AK-12 (v3, behaviour 16)** Unit tests without a server: a parameter's annotation and a
  variable's annotation are copied, a parameter without a known type stays plain next to them, the
  order is unchanged; `*args`, a local class, a name bound below or only for type checkers, a name
  the class body hides and differing declarations are not written; a new method keeps its
  parameter's annotation (`python_extract::tests::method_types_from_the_text`). With recorded hover answers: `list[Item]`
  is written; `Unknown`, `Any`, partly unknown types, dotted names, names the file does not bind or
  binds twice, below or hides (local name, type parameter, class body, rebound built-in), bare
  containers, function and module hovers, a `|` union in a file that shows no support for it, and
  a read the server may have narrowed are not; literals are widened; return types for a name, a
  call, an attribute, an awaited call, a tuple and two results
  (`python_extract::tests::{method_types_from_the_server, method_return_types}`). Live with
  pyright, on unsaved text: the case of OBS-0034 item 52 gives `def extracted(items: list[Item],
  argv: list[str]):` and runs as before; a type the file cannot name and an unresolved one are
  left out; the return types `float` of a called function and `Item` of a result
  (`crates/languages/tests/it/python_extract.rs::types_from_the_language_server`; skipped with a note
  without the server). With `python3`: 10 annotated cases have the expected `def` line and print
  the same as before (`typed_programs_behave_the_same`; the `python3` part skipped with a note).
- **AK-13 (v4, behaviours 17 and 20)** Inline Variable: the caret in, before and behind the name
  and the selected name, at the assignment and at a read; every read replaced, also in a loop, a
  comprehension and a block; the assignment's lines removed with its comment, CRLF kept; the
  parentheses of behaviour 20 for 50 pairs of value and place and the heads of statements
  (`python_inline::tests::{the_observed_variable, caret_and_selection, every_read_is_replaced,
  the_assignment_line_goes, parentheses_where_precedence_needs_them}`).
- **AK-14 (v4, behaviours 18, 19 and 21)** Every refusal of behaviours 18 and 19 with its reason,
  next to the allowed neighbours, and the messages (`python_inline::tests::{not_a_plain_local_variable,
  what_else_the_function_does_with_the_name, reads_reached_only_through_the_assignment,
  a_value_with_side_effects_is_read_once_and_at_once, a_new_object_is_read_once,
  what_the_value_reads_stays_the_same, places_and_messages}`).
- **AK-15 (v4)** The results run: for 24 cases of Inline Variable and 14 of Extract Constant the
  original and the refactored program print the same with the system's `python3`
  (`crates/languages/tests/it/python_extract.rs::inlined_and_constant_programs_behave_the_same`);
  14 refused inlines and 7 refused constants, written out by hand, print something else or fail
  (`refused_inlines_and_constants_would_change_the_output`; the `python3` parts are skipped with
  a note when there is no `python3`).
- **AK-16 (v4, behaviours 17, 21, 22 and 25)** Through the entry points of window and MCP, with
  or without a server: `actions::refactoring_targets`, `actions::plan_code_action_in` and
  `actions::own_refusal` on unsaved text
  (`crates/languages/tests/it/python_extract.rs::inline_and_constant_through_the_entry_points`);
  MCP `code_actions` with `refactoring` `inline` and `extract_constant`, `apply_code_action`
  with preview, `new_name`, undo and the refusal messages (protocol test
  `python_inline_and_constant_tools`).
- **AK-17 (v4, behaviours 22 and 24)** Extract Constant: the place of the new line (function,
  method, nested function, class body, module level, loop, default value, comment lines and
  decorators), one edit where the expression starts the statement, several lines, CRLF; the
  names (`python_constant::tests::{the_line_goes_before_the_top_level_statement, constant_names}`).
- **AK-18 (v4, behaviours 23 and 25)** What counts as a constant, every refusal of behaviour 23
  with its reason, the bare caret, Refactor This and the messages
  (`python_constant::tests::{what_counts_as_constant, constant_refusals, places_and_messages}`).
- **AK-19 (v4)** In the window: ⌥⌘N on a local variable replaces its reads and removes the
  assignment, ⌥⌘C on a selected literal expression writes the constant and opens the name popup;
  ⌘Z takes each back in one step; a refusal shows in the status bar. Open (hand test).
- **AK-10** In the window: ⌥⌘V on `self.price * self.qty` writes the line and opens the name popup
  at `qty`; ⌥⌘M on two statements; ⌘Z takes each back in one step. Open (hand test).

## Out of scope
Replacing all equal occurrences (also by a new constant); choosing the name, the parameters'
order or the place in a dialog; Extract Field and Extract Parameter for Python; inlining a
function, a method, a parameter, a module-level name or a class attribute; inlining one read of
several; a constant inside a class; adding imports
for a type, and types of expressions the server gives no hover for (operators, literals); a
setting for the types; `match` statements in Extract Method; extracting class-level code;
generators (`yield`).

## Known deviations from the reference
- Own implementation because the server has none: SPEC-0095 applies what the language server
  offers; pyright and basedpyright offer no extract refactoring, so for Python Ferris IDEA computes
  the edit itself from the syntax tree (as SPEC-0320 does for the class refactorings of
  TypeScript).
- The reference offers to replace all occurrences and edits the name in place in the text;
  Ferris IDEA replaces the selected occurrence and asks for the name in the rename popup (SPEC-0095
  behaviour 8). After Extract Method inside a class the popup stands at the call, since the new
  method follows the current one.
- Types (behaviour 16): the reference asks in its dialog whether to add parameter and return
  types (a box, seen ticked). Ferris IDEA extracts at once, without a dialog before the extraction
  (SPEC-0095 behaviour 8), so there is no place to ask: it always writes the types it knows for
  certain and leaves the others out. A type that would need a new import is left out; whether the
  reference adds imports was not observed. Only the one case of OBS-0034 item 52 was compared: both
  write `argv: list[str]` and `items: list[Item]` and no return type; the order of the parameters
  differs (behaviour 9, unchanged).
- Only the one extraction of OBS-0034 item 47 was seen in the reference. Whether it refuses the
  places of behaviour 7 was not compared. Extract Method was run in the reference once (OBS-0034 item 52): it asks for the name
  and shows the parameters in a dialog first and puts the new function after the current one;
  Ferris IDEA extracts at once and puts it before. Behaviours 9–13 are Ferris IDEA's own design.
- Inline Variable and Extract Constant (v4, behaviours 17 to 25) are Ferris IDEA's own design from
  what Python guarantees; the reference was not observed for them. Where a reference inlines or
  hoists without these checks, Ferris IDEA refuses and says why (CLAUDE.md rule 9: a refactoring
  that can change what the program does is not copied). Inline acts on the whole variable at
  once and asks nothing; the constant always goes to module level.
- Limits of Inline Variable, where it assumes rather than proves: a call that is not handed an
  object is taken not to change that object's attributes and items (it could through another
  name for the same object); names that are not local (module names, built-ins) are taken not
  to be rebound by calls between the assignment and a read; a value without a call that is read
  several times is taken not to be changed in place between the reads (`v = a + b` for two
  lists, then `v.append(1)` and `return v`, would work on two different lists); operators and
  attribute reads are taken to have no side effects, and an error they raise is raised at the
  read instead of at the assignment; `del` and rebinding are followed by position in the text.
- Limits of Extract Constant: an imported name, a class and a `def` are taken to stay what they
  are, and an attribute of an imported name or of a class to keep its value (`math.pi`,
  `Color.RED`; another module could assign them); another module assigning a name of this module
  is not seen; an operator that raises (`1 / 0`) raises when the module is loaded instead of
  where it stood; the built-in functions of behaviour 23 are taken to be the built-ins when the
  file binds no such name.
- Limits of the analysis, where it assumes rather than proves: an expression without a call is
  taken to be unaffected by calls before it in the statement (behaviour 8); a `with` block is taken to run to its end (a context manager that
  swallows an exception could leave a result unassigned); a name of an enclosing function further
  out is taken to have a value; module names are followed only inside the file, so another module
  that reads a loop variable of this module's top level is not seen; `del` of a name before the
  selection is not followed; a nested function inside a default value is not looked into; an
  unknown decorator of the enclosing method is taken to leave its first parameter the instance;
  a class name the server reports is taken to be the class the file binds under that name (two
  classes of the same name from different modules are not told apart; the type checker then marks
  the call).
