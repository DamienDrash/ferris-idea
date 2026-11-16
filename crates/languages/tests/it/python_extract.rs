//! Extract Variable and Extract Method for Python (`SPEC-0330`): the refactored programs behave
//! as the originals (AK-8, run with the system's `python3`), and the own actions are reached
//! through the entry points the window and the MCP server use, without a language server (AK-9).
//! Inline Variable and Extract Constant (v4) likewise: AK-15 and AK-16.

use std::process::Command;

use idecraft_languages::actions::{Refactoring, plan_code_action_in, refactoring_targets};
use idecraft_languages::insight::Range;
use idecraft_languages::python_constant::extract_constant;
use idecraft_languages::python_extract::{
    EXTRACT_CONSTANT, EXTRACT_METHOD, EXTRACT_VARIABLE, INLINE_VARIABLE, extract_method,
    extract_variable,
};
use idecraft_languages::python_inline::inline_variable;
use idecraft_languages::rename::apply_edits;

/// The source without the selection marks `«` and `»`, and the selection.
fn marked(source: &str) -> (String, Range) {
    let text = source.replace(['«', '»'], "");
    let a = source.find('«').expect("a start mark");
    let b = source.find('»').expect("an end mark") - '«'.len_utf8();
    let pos = |at: usize| {
        let before = &text[..at];
        let line = before.matches('\n').count() as u64;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        (line, text[start..at].chars().count() as u64)
    };
    let (s, e) = (pos(a), pos(b));
    (
        text,
        Range {
            start_line: s.0,
            start_character: s.1,
            end_line: e.0,
            end_character: e.1,
        },
    )
}

fn python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// What a program prints, and whether it ended well.
fn run(program: &str) -> (String, String, bool) {
    let out = Command::new("python3")
        .args(["-I", "-c", program])
        .output()
        .expect("python3 runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

const VARIABLE: &[(&str, &str)] = &[
    (
        "the observed expression",
        "class Item:\n    def __init__(self, price, qty):\n        self.price = price\n        self.qty = qty\n\n    def total(self):\n        return «self.price * self.qty»\n\n\nprint(Item(3, 4).total())\n",
    ),
    (
        "several lines",
        "def f(a, b):\n    return max(«a +\n               b», 3)\n\n\nprint(f(1, 5))\n",
    ),
    (
        "a generator as the only argument",
        "xs = [1, 2, 3]\nprint(sum(«x * 2 for x in xs»))\n",
    ),
    (
        "touching a word",
        "flag = False\nif not«(flag)»:\n    print('off')\n",
    ),
    (
        "a whole expression statement",
        "def f():\n    «print('hi')»\n\n\nf()\n",
    ),
    (
        "the first iterable of a comprehension",
        "n = 2\nprint([x for x in «range(n * 2)» if x])\n",
    ),
    (
        "the condition of a conditional expression",
        "def f(n):\n    return 'big' if «n > 2» else 'small'\n\n\nprint(f(1), f(5))\n",
    ),
    (
        "the condition of if and the iterable of for",
        "def f(items):\n    out = []\n    for i in «sorted(items)»:\n        if i > 1:\n            out.append(i)\n    return out\n\n\nprint(f([3, 1, 2]))\n",
    ),
    (
        "inside an f-string",
        "def f(n):\n    return f'{«n * 2»} items'\n\n\nprint(f(4))\n",
    ),
    (
        "the left side of and",
        "def f(a, b):\n    return «a.strip()» and b\n\n\nprint(f(' x ', 1), f('  ', 2))\n",
    ),
    (
        "the first item of with",
        "import io\n\n\ndef f(s):\n    with «io.StringIO(s)» as fh:\n        return fh.read()\n\n\nprint(f('abc'))\n",
    ),
    (
        "a name that is taken",
        "def f(items):\n    count = 1\n    return «items.count(1)» + count\n\n\nprint(f([1, 1, 2]))\n",
    ),
    (
        "tabs",
        "def f(a):\n\tif a:\n\t\treturn «a * 2» + 1\n\treturn 0\n\n\nprint(f(4))\n",
    ),
    // SPEC-0330 v2 behaviour 8: what runs before has no side effects, or the selection has none.
    (
        "a pure expression after a call",
        "class Item:\n    def __init__(self):\n        self.price = 3\n        self.qty = 4\n\n    def first(self):\n        print('first')\n        return 1\n\n    def total(self):\n        return self.first() + «self.price * self.qty»\n\n\nprint(Item().total())\n",
    ),
    (
        "a call after an attribute read",
        "class C:\n    def scale(self, x):\n        return x * 2\n\n\ndef g():\n    print('g')\n    return 5\n\n\nc = C()\nprint(c.scale(«g()»))\n",
    ),
    (
        "the value before a subscript target",
        "def k():\n    print('k')\n    return 'a'\n\n\ndef make():\n    print('make')\n    return 1\n\n\nd = {}\nd[k()] = «make()»\nprint(d)\n",
    ),
    (
        "the call that runs first",
        "def first():\n    print('first')\n    return 1\n\n\ndef second():\n    print('second')\n    return 2\n\n\nprint(«first()» + second())\n",
    ),
];

const METHOD: &[(&str, &str)] = &[
    (
        "a parameter and a result",
        "def f(a, b):\n    «total = a + b\n    total *= 2»\n    return total\n\n\nprint(f(1, 2))\n",
    ),
    (
        "two results",
        "def f(xs):\n    «lo = min(xs)\n    hi = max(xs)»\n    return hi - lo\n\n\nprint(f([4, 9, 1]))\n",
    ),
    (
        "a running total in a loop",
        "def f(xs):\n    total = 0\n    for x in xs:\n        «total = total + x»\n    return total\n\n\nprint(f([1, 2, 3]))\n",
    ),
    (
        "a result set on some paths that had a value",
        "def f(n):\n    label = 'small'\n    «if n > 2:\n        label = 'big'»\n    return label\n\n\nprint(f(1), f(5))\n",
    ),
    (
        "statements at module level",
        "xs = [3, 1, 2]\n«ys = sorted(xs)\nfirst = ys[0]»\nprint(ys, first)\n",
    ),
    (
        "a loop at module level",
        "xs = [3, 1, 2]\n«for x in xs:\n    print(x)»\nprint('done')\n",
    ),
    (
        "return on every path",
        "def f(n):\n    n += 1\n    «if n > 2:\n        return 'big'\n    else:\n        return 'small'»\n\n\nprint(f(0), f(5))\n",
    ),
    (
        "a method that uses self",
        "class Item:\n    def __init__(self, price, qty):\n        self.price = price\n        self.qty = qty\n\n    def total(self, tax):\n        «net = self.price * self.qty\n        gross = net * (1 + tax)»\n        return gross\n\n    def name(self):\n        return 'item'\n\n\nprint(Item(3, 4).total(0.5))\n",
    ),
    (
        "a method that does not use self",
        "class Box:\n    def area(self, w, h):\n        «a = w * h»\n        return a\n\n\nprint(Box().area(2, 3))\n",
    ),
    (
        "a class method",
        "class Box:\n    sides = 4\n\n    @classmethod\n    def describe(cls, n):\n        «text = str(cls.sides * n)»\n        return text\n\n\nprint(Box.describe(2))\n",
    ),
    (
        "a static method",
        "class Box:\n    @staticmethod\n    def double(n):\n        «m = n * 2»\n        return m\n\n\nprint(Box.double(4))\n",
    ),
    (
        "a nested function with a name of the outer one",
        "def outer(k):\n    def inner(x):\n        «y = x * k»\n        return y\n    return inner(3)\n\n\nprint(outer(5))\n",
    ),
    (
        "an expression in a comprehension",
        "def f(xs, k):\n    return [«x * k + 1» for x in xs]\n\n\nprint(f([1, 2], 10))\n",
    ),
    (
        "an expression in a lambda",
        "def f(k):\n    g = lambda x: «x + k»\n    return g(1)\n\n\nprint(f(4))\n",
    ),
    (
        "a generator as the only argument",
        "def f(xs):\n    return sum(«x * x for x in xs»)\n\n\nprint(f([1, 2, 3]))\n",
    ),
    (
        "await in statements",
        "import asyncio\n\n\nasync def twice(n):\n    return n * 2\n\n\nasync def main(n):\n    «a = await twice(n)\n    b = a + 1»\n    return b\n\n\nprint(asyncio.run(main(3)))\n",
    ),
    (
        "await in an expression",
        "import asyncio\n\n\nasync def twice(n):\n    return n * 2\n\n\nasync def main(n):\n    return «await twice(n)» + 1\n\n\nprint(asyncio.run(main(3)))\n",
    ),
    (
        "a string over several lines",
        "def f(name):\n    if name:\n        «text = \"\"\"Dear\n    {}\n  bye\"\"\".format(name)\n        text = text.upper()»\n        return text\n    return ''\n\n\nprint(f('x'))\n",
    ),
    (
        "raise on every path",
        "def f(n):\n    if n < 0:\n        «message = 'negative: %d' % n\n        raise ValueError(message)»\n    return n\n\n\ntry:\n    f(-1)\nexcept ValueError as e:\n    print(e, f(2))\n",
    ),
    (
        "a with block",
        "import io\n\n\ndef f(s):\n    «with io.StringIO(s) as fh:\n        data = fh.read()»\n    return data.upper()\n\n\nprint(f('abc'))\n",
    ),
    (
        "a try whose handler raises",
        "def f(s):\n    «try:\n        n = int(s)\n    except ValueError:\n        raise KeyError(s)»\n    return n + 1\n\n\nprint(f('4'))\n",
    ),
    (
        "a nested function that is used after",
        "def f(k):\n    «def add(x):\n        return x + k»\n    return add(1)\n\n\nprint(f(2))\n",
    ),
    (
        "comments around",
        "def f(a):\n    # first\n    «# doubles\n    b = a * 2  # twice\n    c = b + 1»  # one more\n    return c\n\n\nprint(f(3))\n",
    ),
    (
        "tabs",
        "def f(a):\n\tif a:\n\t\t«b = a * 2\n\t\tc = b + 1»\n\t\treturn c\n\treturn 0\n\n\nprint(f(3))\n",
    ),
    (
        "a comment attached to the function",
        "import math\n\n# The area.\n# Of a circle.\ndef area(r):\n    «sq = r * r\n    a = math.pi * sq»\n    return round(a)\n\n\nprint(area(2))\n",
    ),
    (
        "a name set before and again after",
        "def f(xs):\n    for i in xs:\n        print(i)\n    «for i in xs:\n        print(i * 2)»\n    for i in xs:\n        print(i * 3)\n\n\nf([1, 2])\n",
    ),
    (
        "an inner loop with break",
        "def f(xs):\n    found = None\n    «for x in xs:\n        if x > 1:\n            found = x\n            break»\n    return found\n\n\nprint(f([1, 2, 3]))\n",
    ),
    (
        "inside a branch inside a loop",
        "def f(xs):\n    best = None\n    for x in xs:\n        if x > 1:\n            «best = x\n            print('new', best)»\n    return best\n\n\nprint(f([1, 2, 3]))\n",
    ),
    (
        "the body of a try with else",
        "def f(s):\n    try:\n        «n = int(s)»\n    except ValueError:\n        return -1\n    else:\n        return n + 1\n\n\nprint(f('4'), f('x'))\n",
    ),
    (
        "a counter in nested loops",
        "def f(rows):\n    count = 0\n    for row in rows:\n        for cell in row:\n            «if cell:\n                count += 1»\n    return count\n\n\nprint(f([[1, 0], [1, 1]]))\n",
    ),
    (
        "a class in a function",
        "def f(k):\n    «class P:\n        scale = k\n\n        def get(self):\n            return self.scale * k»\n    return P().get()\n\n\nprint(f(3))\n",
    ),
    (
        "a name declared global",
        "n = 5\n\n\ndef f():\n    global n\n    «x = n + 1»\n    return x\n\n\nprint(f())\n",
    ),
    (
        "a lambda with a default value in a loop",
        "def f(xs):\n    fs = []\n    for i in xs:\n        «fs.append(lambda i=i: i * 2)»\n    return [g() for g in fs]\n\n\nprint(f([1, 2]))\n",
    ),
    (
        "a while loop with a carried name",
        "def f(n):\n    steps = 0\n    while n > 1:\n        «if n % 2:\n            n = 3 * n + 1\n        else:\n            n = n // 2\n        steps += 1»\n    return steps\n\n\nprint(f(6))\n",
    ),
    (
        "a loop that keeps its own previous value",
        "def f(xs):\n    «for x in xs:\n        if x > 1:\n            print(prev)\n        prev = x»\n    return 0\n\n\nprint(f([1, 2, 3]))\n",
    ),
    (
        "a module name passed in and kept",
        "count = 0\nfor step in (1, 2):\n    «count = count + step»\nprint(count)\n",
    ),
    (
        "an early raise and a result",
        "class Acc:\n    def __init__(self):\n        self.limit = 10\n\n    def add(self, a, b):\n        «if a < 0:\n            raise ValueError(a)\n        total = a + b\n        if total > self.limit:\n            total = self.limit»\n        return total\n\n\nprint(Acc().add(3, 4), Acc().add(8, 9))\n",
    ),
    (
        "an expression in a nested function",
        "def outer(k):\n    def inner(x):\n        return «x * k + len(str(k))»\n    return inner(2)\n\n\nprint(outer(12))\n",
    ),
    (
        "__new__ gives a function",
        "class P:\n    def __new__(cls, n):\n        «self = object.__new__(cls)\n        self.n = n * 2»\n        return self\n\n\nprint(P(4).n)\n",
    ),
    (
        "walrus as a local",
        "def f(xs):\n    «if (n := len(xs)) > 1:\n        print(n)»\n    return 0\n\n\nprint(f([1, 2]))\n",
    ),
];

#[test]
fn extracted_programs_behave_the_same() {
    if !python3() {
        eprintln!("skipped: no python3");
        return;
    }
    // The assignment expression and positional parameters need Python 3.8.
    let cases = VARIABLE
        .iter()
        .map(|c| (c, true))
        .chain(METHOD.iter().map(|c| (c, false)));
    for ((name, source), variable) in cases {
        let (text, range) = marked(source);
        let edits = if variable {
            extract_variable(&text, &range)
        } else {
            extract_method(&text, &range)
        }
        .unwrap_or_else(|e| panic!("{name}: refused: {e}"));
        let after = apply_edits(&text, &edits).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_ne!(after, text, "{name}");
        let (expected, _, ok) = run(&text);
        assert!(ok, "{name}: the original fails");
        let (got, stderr, ok) = run(&after);
        if std::env::var_os("IDECRAFT_SHOW_EXTRACT").is_some() {
            println!("=== {name}\n{after}");
        }
        assert!(ok, "{name}: the result fails:\n{after}\n{stderr}");
        assert_eq!(got, expected, "{name}:\n{after}");
    }
}

/// SPEC-0330 v2 behaviour 8, AK-11: the refused extractions would change what runs first. Each
/// case is refused, and the text the extraction would give, written here by hand, prints
/// something else than the original under `python3`.
#[test]
fn refused_reorderings_would_change_the_output() {
    let cases: &[(&str, &str)] = &[
        (
            "def first():\n    print('first')\n    return 1\n\n\ndef second():\n    print('second')\n    return 2\n\n\ntotal = first() + «second()»\nprint(total)\n",
            "def first():\n    print('first')\n    return 1\n\n\ndef second():\n    print('second')\n    return 2\n\n\nvalue = second()\ntotal = first() + value\nprint(total)\n",
        ),
        (
            "items = []\n\n\ndef add(x):\n    items.append(x)\n    return x\n\n\nprint(add(1), «len(items)»)\n",
            "items = []\n\n\ndef add(x):\n    items.append(x)\n    return x\n\n\nlength = len(items)\nprint(add(1), length)\n",
        ),
    ];
    for (source, reordered) in cases {
        let (text, range) = marked(source);
        let why = extract_variable(&text, &range).expect_err(source);
        assert!(why.contains("would then run before"), "{why}");
        if !python3() {
            eprintln!("skipped the python3 part: no python3");
            continue;
        }
        let (before, _, ok) = run(&text);
        assert!(ok, "{text}");
        let (after, stderr, ok) = run(reordered);
        assert!(ok, "{reordered}\n{stderr}");
        assert_ne!(before, after, "{reordered}");
    }
}

const PRICES: &str = "class Item:\n    def __init__(self, price, qty):\n        self.price = price\n        self.qty = qty\n\n    def total(self):\n        return self.price * self.qty\n\n    def report(self):\n        total = self.total()\n        line = f\"{self.qty} x {self.price} = {total}\"\n        print(line)\n";

/// The entry points of the window and the MCP server, in a folder without project files: with
/// pyright installed the server offers no extract action, without it the server cannot be asked;
/// both ways the own actions are found and planned on the unsaved text.
#[tokio::test(flavor = "multi_thread")]
async fn through_the_refactoring_entry_points() {
    let wait = std::time::Duration::from_secs(60);
    let root = std::env::temp_dir().join(format!("idecraft-python-extract-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("prices.py");
    // The file on disk is older than the editor's text.
    std::fs::write(&file, "class Item:\n    pass\n").unwrap();

    let selected = Range {
        start_line: 6,
        start_character: 15,
        end_line: 6,
        end_character: 36,
    };
    let found = refactoring_targets(
        &root,
        &file,
        Some(PRICES),
        &selected,
        Refactoring::Variable,
        wait,
    )
    .await
    .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, "self.price * self.qty");
    assert_eq!(found[0].actions[0].title, EXTRACT_VARIABLE);
    assert!(Refactoring::Variable.matches(&found[0].actions[0]));
    let plan = plan_code_action_in(
        &root,
        &file,
        Some(PRICES),
        &found[0].range,
        EXTRACT_VARIABLE,
        wait,
    )
    .await
    .unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].path, file);
    let after = apply_edits(PRICES, &plan[0].edits).unwrap();
    assert!(
        after.contains("        qty = self.price * self.qty\n        return qty\n"),
        "{after}"
    );
    let name = idecraft_languages::actions::introduced_name(PRICES, &after).unwrap();
    assert_eq!(
        (name.name.as_str(), name.line, name.character),
        ("qty", 6, 8)
    );

    // A bare caret in `self.price` offers the expressions around it.
    let caret = Range {
        start_line: 6,
        start_character: 22,
        end_line: 6,
        end_character: 22,
    };
    let found = refactoring_targets(
        &root,
        &file,
        Some(PRICES),
        &caret,
        Refactoring::Variable,
        wait,
    )
    .await
    .unwrap();
    let texts: Vec<&str> = found.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, ["self.price", "self.price * self.qty"]);

    // Two statements become a method; Refactor This lists it for the selection.
    let lines = Range {
        start_line: 10,
        start_character: 0,
        end_line: 11,
        end_character: 19,
    };
    for refactoring in [Refactoring::Method, Refactoring::Any] {
        let found = refactoring_targets(&root, &file, Some(PRICES), &lines, refactoring, wait)
            .await
            .unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].actions.iter().any(|a| a.title == EXTRACT_METHOD));
    }
    let plan = plan_code_action_in(&root, &file, Some(PRICES), &lines, EXTRACT_METHOD, wait)
        .await
        .unwrap();
    let after = apply_edits(PRICES, &plan[0].edits).unwrap();
    assert!(
        after.ends_with(
            "        total = self.total()\n        self.extracted(total)\n\n    def extracted(self, total):\n        line = f\"{self.qty} x {self.price} = {total}\"\n        print(line)\n"
        ),
        "{after}"
    );

    // A refusal says why, in place of the server's message.
    let part = Range {
        start_line: 6,
        start_character: 15,
        end_line: 6,
        end_character: 27,
    };
    let found = refactoring_targets(
        &root,
        &file,
        Some(PRICES),
        &part,
        Refactoring::Variable,
        wait,
    )
    .await
    .unwrap();
    assert!(found.is_empty());
    let message =
        idecraft_languages::actions::own_refusal(&file, Some(PRICES), &part, Refactoring::Variable)
            .unwrap();
    assert_eq!(
        message,
        "Cannot extract `self.price *` into a variable: the selection is not one complete expression."
    );
    let err = plan_code_action_in(&root, &file, Some(PRICES), &part, EXTRACT_VARIABLE, wait)
        .await
        .unwrap_err();
    assert!(err.to_string().contains(&message), "{err}");
    // Other languages and other refactorings have no own message.
    assert!(
        idecraft_languages::actions::own_refusal(
            &root.join("a.rs"),
            Some("fn main() {}"),
            &part,
            Refactoring::Variable
        )
        .is_none()
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0330 v3 behaviour 16, AK-12: sources with annotations, and the `def` line the extraction
/// writes for each without a language server.
const TYPED: &[(&str, &str, &str)] = &[
    (
        "annotated parameters next to a plain one",
        "def f(a: int, b: str = 'x', c=1):\n    «t = b * a\n    print(t, c)»\n\n\nf(2)\n",
        "def extracted(b: str, a: int, c):",
    ),
    (
        "an annotated variable that is also the result",
        "def f(n: int):\n    total: float = 0.5\n    «total = total + n»\n    return total\n\n\nprint(f(2))\n",
        "def extracted(total: float, n: int) -> float:",
    ),
    (
        "annotations with imported names",
        "from typing import List, Optional\n\n\ndef f(xs: List[int], d: Optional[dict] = None):\n    «out = [x * 2 for x in xs]\n    print(out, d)»\n\n\nf([1, 2])\n",
        "def extracted(xs: List[int], d: Optional[dict]):",
    ),
    (
        "a string annotation of a class below",
        "def f(it: \"Item\", n: int):\n    «print(it.v * n)»\n\n\nclass Item:\n    v = 3\n\n\nf(Item(), 2)\n",
        "def extracted(it: \"Item\", n: int):",
    ),
    (
        "a variable annotated with a class below",
        "def f():\n    it: Item = Item()\n    «print(it.v)»\n\n\nclass Item:\n    v = 3\n\n\nf()\n",
        "def extracted(it):",
    ),
    (
        "a method",
        "class Box:\n    def area(self, w: int, h: \"int\") -> int:\n        «a = w * h»\n        return a\n\n\nprint(Box().area(2, 3))\n",
        "def extracted(self, w: int, h: \"int\"):",
    ),
    (
        "a class of the enclosing function",
        "def outer():\n    class P:\n        v = 2\n\n    def inner(x: P, y: int):\n        «print(x.v * y)»\n\n    inner(P(), 3)\n\n\nouter()\n",
        "def extracted(x, y: int):",
    ),
    (
        "a returned annotated parameter",
        "def f(a: int, b):\n    «if b:\n        return a\n    raise ValueError(b)»\n\n\nprint(f(4, 1))\n",
        "def extracted(b, a: int) -> int:",
    ),
    (
        "annotations that are not evaluated",
        "from __future__ import annotations\n\n\ndef f(n: int | None, it: Later):\n    «print(n, it.v)»\n\n\nclass Later:\n    v = 1\n\n\nf(None, Later())\n",
        "def extracted(n: int | None, it: Later):",
    ),
    (
        "a tuple of annotated names",
        "def f(a: int, b: str):\n    «return b, a»\n\n\nprint(f(1, 'x'))\n",
        "def extracted(b: str, a: int) -> tuple[str, int]:",
    ),
];

/// The annotated functions still parse and run, and print what the originals print.
#[test]
fn typed_programs_behave_the_same() {
    for (name, source, def) in TYPED {
        let (text, range) = marked(source);
        let edits =
            extract_method(&text, &range).unwrap_or_else(|e| panic!("{name}: refused: {e}"));
        let after = apply_edits(&text, &edits).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            after.lines().any(|l| l.trim() == *def),
            "{name}: no `{def}` in\n{after}"
        );
        if !python3() {
            eprintln!("skipped the python3 part: no python3");
            continue;
        }
        let (expected, _, ok) = run(&text);
        assert!(ok, "{name}: the original fails");
        let (got, stderr, ok) = run(&after);
        assert!(ok, "{name}: the result fails:\n{after}\n{stderr}");
        assert_eq!(got, expected, "{name}:\n{after}");
    }
}

const MODEL: &str = "from dataclasses import dataclass\n\n\n@dataclass\nclass Item:\n    name: str\n    price: float\n    qty: int = 1\n\n    def total(self) -> float:\n        return self.price * self.qty\n\n\ndef order_total(items: list[Item]) -> float:\n    return sum(i.total() for i in items)\n\n\ndef newest() -> Item:\n    return Item(\"pen\", 1.5)\n";

/// SPEC-0330 v3 behaviour 16, AK-12, live: pyright tells the types the text does not declare, on
/// the editor's unsaved text; a type the file has no name for is left out; the annotated result
/// runs. Skipped without pyright or basedpyright.
#[tokio::test(flavor = "multi_thread")]
async fn types_from_the_language_server() {
    let server = idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Python);
    if !server
        .is_ok_and(|s| idecraft_languages::python_server::is_python_server(s.candidate.program))
    {
        assert!(
            std::env::var_os("IDECRAFT_REQUIRE_TOOLS").is_none(),
            "required tool missing: pyright-langserver"
        );
        eprintln!("skipped: no pyright or basedpyright installed");
        return;
    }
    let wait = std::time::Duration::from_secs(120);
    let root = std::env::temp_dir().join(format!("idecraft-python-types-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("app/__init__.py"), "").unwrap();
    std::fs::write(root.join("app/prices.py"), MODEL).unwrap();
    let file = root.join("main.py");
    // The file on disk is older than the editor's text.
    std::fs::write(&file, "import sys\n").unwrap();
    let extract = async |source: &str| {
        let (text, range) = marked(source);
        let plan = plan_code_action_in(&root, &file, Some(&text), &range, EXTRACT_METHOD, wait)
            .await
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        apply_edits(&text, &plan[0].edits).unwrap()
    };

    // OBS-0034 item 52: `argv` from its annotation, `items` from the server.
    let observed = "import sys\n\nfrom app.prices import Item, order_total\n\n\ndef main(argv: list[str]) -> int:\n    items = [Item(\"pen\", 1.5, 4), Item(\"book\", 12.0)]\n    «print(f\"total {order_total(items):.2f} for {argv[1:]}\")»\n    return 0\n\n\nif __name__ == \"__main__\":\n    raise SystemExit(main(sys.argv))\n";
    let after = extract(observed).await;
    assert!(
        after.contains("def extracted(items: list[Item], argv: list[str]):\n"),
        "{after}"
    );
    if python3() {
        let run_in = |program: &str| {
            let out = Command::new("python3")
                .args(["-c", program])
                .current_dir(&root)
                .output()
                .expect("python3 runs");
            (
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        };
        let before = run_in(&observed.replace(['«', '»'], ""));
        assert!(before.0.starts_with("total 18.00"), "{before:?}");
        assert_eq!(run_in(&after), before, "{after}");
    } else {
        eprintln!("skipped the python3 part: no python3");
    }

    // The server knows the type, but the file has no name for it: `Item` is not imported.
    let unnamed = "from app.prices import newest\n\n\ndef main(n: int):\n    it = newest()\n    «print(it.total() * n)»\n";
    let after = extract(unnamed).await;
    assert!(after.contains("def extracted(it, n: int):\n"), "{after}");

    // An unresolved import gives no type.
    let unknown =
        "import nowhere\n\n\ndef main(n: int):\n    it = nowhere.thing()\n    «print(it, n)»\n";
    let after = extract(unknown).await;
    assert!(after.contains("def extracted(it, n: int):\n"), "{after}");

    // A return type: the declared result of the called function, and of a result's assignment.
    let called = "from app.prices import Item, order_total\n\n\ndef main(items: list[Item]):\n    return «order_total(items)» + 1\n";
    let after = extract(called).await;
    assert!(
        after.contains("def extracted(items: list[Item]) -> float:\n"),
        "{after}"
    );
    let result = "from app.prices import Item, newest\n\n\ndef main():\n    «it = newest()»\n    return it\n";
    let after = extract(result).await;
    assert!(after.contains("def extracted() -> Item:\n"), "{after}");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0330 v4, AK-15: Inline Variable with the variable's name selected (`«name»`).
const INLINE: &[(&str, &str)] = &[
    (
        "the observed variable, read in an f-string",
        "class Item:\n    def __init__(self, name, price):\n        self.name = name\n        self.price = price\n\n    def total(self):\n        print('total')\n        return self.price\n\n\ndef describe(item, factor=1):\n    \"\"\"Return a label for an item.\"\"\"\n    «value» = item.total() * factor\n    unused = 3\n    return f\"{item.name}: {value}\"\n\n\nprint(describe(Item('x', 2.0), 3))\n",
    ),
    (
        "several reads and the operators around them",
        "def f(a, b):\n    «t» = a + b\n    print(t * 2, 2 - t, t - 2, t ** 2, 2 ** t, -t, not t, t if t else 0, [t, (t)], t < 5 < 9)\n    return t\n\n\nprint(f(1, 2), f(-3, 3))\n",
    ),
    (
        "a sign under a power",
        "def f(a):\n    «n» = -a\n    return n ** 2, 2 ** n, abs(n)\n\n\nprint(f(3))\n",
    ),
    (
        "a comparison",
        "def f(a, b):\n    «c» = a < b\n    return c == True, not c, c and a, c or b, c < 1, 1 if c else 2\n\n\nprint(f(1, 2), f(2, 1))\n",
    ),
    (
        "a conditional expression",
        "def f(a, b):\n    «v» = a if a > b else b\n    return v + 1, v * 2, [v], v if v else 0, 1 if v else 0, {v: v}\n\n\nprint(f(1, 2), f(2, 1), f(0, 0))\n",
    ),
    (
        "boolean operators",
        "def f(a, b):\n    «v» = a or b\n    return v and 1, 1 and v, v or 2, 2 or v, not v\n\n\nprint(f(0, 0), f(0, 5), f(3, 0))\n",
    ),
    (
        "bit operators",
        "def f(a, b):\n    «m» = a | b\n    return m & 3, m ^ 1, m << 1, 1 << m, ~m, m + 1\n\n\nprint(f(1, 4))\n",
    ),
    (
        "a tuple without parentheses",
        "def f(a, b):\n    «p» = a, b\n    return p, len(p), p[0], p + (1,)\n\n\nprint(f(1, 2))\n",
    ),
    (
        "subscript and slice",
        "def f(xs, a):\n    «i» = a - 1\n    return xs[i], xs[i:], xs[:i], xs[i - 1]\n\n\nprint(f([1, 2, 3, 4], 3))\n",
    ),
    (
        "a number before a dot",
        "def f():\n    «five» = 5\n    return five.real, five.bit_length(), five + 1\n\n\nprint(f())\n",
    ),
    (
        "joined strings",
        "def f():\n    «s» = 'a' 'b'\n    return s.upper(), s + 'c', s * 2\n\n\nprint(f())\n",
    ),
    (
        "a call read in the head of an if",
        "def noisy(x):\n    print('noisy', x)\n    return x\n\n\ndef f(a):\n    «v» = noisy(a)\n    if v:\n        return 'yes'\n    return 'no'\n\n\nprint(f(1), f(0))\n",
    ),
    (
        "a call read in the head of a for",
        "def numbers(n):\n    print('numbers')\n    return list(range(n))\n\n\ndef f(n):\n    «xs» = numbers(n)\n    for x in xs:\n        print(x)\n\n\nf(3)\n",
    ),
    (
        "a call after plain assignments, as an argument",
        "def noisy(x):\n    print('noisy', x)\n    return x\n\n\ndef f(a, b):\n    «v» = noisy(a)\n    c = b + 1\n    d, e = b, 2\n    return max(c, v, d, e)\n\n\nprint(f(7, 2))\n",
    ),
    (
        "a call as the first iterable of a comprehension",
        "def numbers(n):\n    print('numbers')\n    return range(n)\n\n\ndef f(n):\n    «xs» = numbers(n)\n    return [x * 2 for x in xs if x]\n\n\nprint(f(4))\n",
    ),
    (
        "an awaited call",
        "import asyncio\n\n\nasync def g():\n    print('g')\n    return 4\n\n\nasync def f():\n    «v» = await g()\n    return v + 1\n\n\nprint(asyncio.run(f()))\n",
    ),
    (
        "a new list read once",
        "def fill(xs):\n    xs.append(1)\n    return xs\n\n\ndef f():\n    «acc» = []\n    print('between')\n    return fill(acc)\n\n\nprint(f(), f())\n",
    ),
    (
        "a lambda as a key",
        "def f(xs):\n    «key» = lambda x: -x\n    return sorted(xs, key=key)\n\n\nprint(f([2, 3, 1]))\n",
    ),
    (
        "reads in a loop and in a comprehension",
        "def f(a, xs):\n    «k» = a * 2\n    for x in xs:\n        print(x * k)\n    return [x + k for x in xs]\n\n\nprint(f(2, [1, 2]))\n",
    ),
    (
        "an attribute read after calls that do not get the object",
        "class Counter:\n    def __init__(self):\n        self.count = 3\n\n    def show(self, other):\n        «n» = self.count\n        print('a')\n        print(len(other))\n        return n, n + 1\n\n\nprint(Counter().show([1]))\n",
    ),
    (
        "a value over several lines",
        "def f(a, b):\n    «t» = (a +\n         b)\n    return max(t, 0) * t\n\n\nprint(f(1, 2))\n",
    ),
    (
        "an assignment in a block",
        "def f(a, b):\n    if a:\n        «t» = b + 1  # one more\n        print(t)\n        while b < 3:\n            b = b + 1\n        return b\n    return b\n\n\nprint(f(1, 0), f(0, 0))\n",
    ),
    (
        "an input that changes after the last read",
        "def f(a):\n    «v» = a + 1\n    print(v)\n    a = 10\n    return a\n\n\nprint(f(1))\n",
    ),
    (
        "a keyword argument of the same name",
        "def g(t=0):\n    return t * 2\n\n\ndef f(a):\n    «t» = a + 1\n    return g(t=t)\n\n\nprint(f(1))\n",
    ),
];

/// SPEC-0330 v4, AK-15: Extract Constant with the expression selected.
const CONSTANT: &[(&str, &str)] = &[
    (
        "arithmetic in a function",
        "def seconds(hours):\n    return hours * («60 * 60»)\n\n\nprint(seconds(2), seconds(3))\n",
    ),
    (
        "in a method",
        "class Clock:\n    def hours(self, s):\n        return s / «3600»\n\n\nprint(Clock().hours(7200))\n",
    ),
    (
        "in a class body",
        "class Limits:\n    size = «4 * 1024»\n\n\nprint(Limits.size)\n",
    ),
    (
        "a default value",
        "def wait(timeout=«30 * 60»):\n    return timeout\n\n\nprint(wait(), wait(1))\n",
    ),
    (
        "names bound above",
        "BASE = 10\nSIZE = BASE * 2\n\n\ndef f(n):\n    return n + («SIZE - BASE»)\n\n\nprint(f(1))\n",
    ),
    (
        "an attribute of a module",
        "import math\n\n\ndef turn(n):\n    return round(«math.pi * 2» * n, 3)\n\n\nprint(turn(1))\n",
    ),
    (
        "built-in functions on constants",
        "NAMES = ('a', 'b', 'c')\n\n\ndef f(n):\n    return n + «max(len(NAMES), 2)»\n\n\nprint(f(1))\n",
    ),
    (
        "a tuple of types",
        "def is_number(x):\n    return isinstance(x, «(int, float)»)\n\n\nprint(is_number(1), is_number('a'))\n",
    ),
    (
        "out of a loop at module level",
        "total = 0\nfor i in range(3):\n    if i:\n        total += i * «1.5»\nprint(total)\n",
    ),
    (
        "an f-string of constants",
        "HOST = 'example.org'\n\n\ndef url(path):\n    return «f'https://{HOST}/'» + path\n\n\nprint(url('a'))\n",
    ),
    (
        "a string",
        "def headers():\n    return {'Content-Type': «'application/json'»}\n\n\nprint(headers())\n",
    ),
    (
        "a decorated function with a comment above",
        "import functools\n\n\n# Cached.\n@functools.lru_cache(maxsize=None)\ndef wait(n):\n    return n * «60»\n\n\nprint(wait(2), wait(2))\n",
    ),
    (
        "a conditional expression over several lines",
        "DEBUG = False\n\n\ndef level():\n    return [«'debug' if DEBUG\n            else 'info'»]\n\n\nprint(level())\n",
    ),
    (
        "in a nested function",
        "def outer():\n    def inner(x):\n        return x + «2 ** 10»\n    return inner(1)\n\n\nprint(outer())\n",
    ),
];

/// SPEC-0330 v4, AK-15: the inlined programs and the programs with a new constant print what
/// the originals print.
#[test]
fn inlined_and_constant_programs_behave_the_same() {
    if !python3() {
        eprintln!("skipped: no python3");
        return;
    }
    let cases = INLINE
        .iter()
        .map(|c| (c, true))
        .chain(CONSTANT.iter().map(|c| (c, false)));
    for ((name, source), inline) in cases {
        let (text, range) = marked(source);
        let edits = if inline {
            inline_variable(&text, &range)
        } else {
            extract_constant(&text, &range)
        }
        .unwrap_or_else(|e| panic!("{name}: refused: {e}"));
        let after = apply_edits(&text, &edits).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_ne!(after, text, "{name}");
        if inline {
            let variable = &text[..].lines().nth(range.start_line as usize).unwrap()
                [range.start_character as usize..range.end_character as usize];
            assert!(
                !after
                    .lines()
                    .any(|l| l.trim_start().starts_with(&format!("{variable} = "))),
                "{name}: the assignment stays:\n{after}"
            );
        }
        let (expected, _, ok) = run(&text);
        assert!(ok, "{name}: the original fails");
        let (got, stderr, ok) = run(&after);
        if std::env::var_os("IDECRAFT_SHOW_EXTRACT").is_some() {
            println!("=== {name}\n{after}");
        }
        assert!(ok, "{name}: the result fails:\n{after}\n{stderr}");
        assert_eq!(got, expected, "{name}:\n{after}");
    }
}

/// SPEC-0330 v4 behaviours 19 and 23, AK-15: what is refused would change what the program does.
/// Each case is refused, and the text the refactoring would give, written here by hand, prints
/// something else than the original under `python3` or fails.
#[test]
fn refused_inlines_and_constants_would_change_the_output() {
    // Whether Inline Variable (else Extract Constant), the source, the reason, and the result by
    // hand.
    let cases: &[(bool, &str, &str, &str)] = &[
        (
            true,
            "def noisy():\n    print('noisy')\n    return 1\n\n\ndef f():\n    «v» = noisy()\n    return v + v\n\n\nprint(f())\n",
            "run 2 times",
            "def noisy():\n    print('noisy')\n    return 1\n\n\ndef f():\n    return noisy() + noisy()\n\n\nprint(f())\n",
        ),
        (
            true,
            "def f(items):\n    «n» = len(items)\n    items.append(1)\n    return n\n\n\nprint(f([]))\n",
            "would then run after `items.append(1)`",
            "def f(items):\n    items.append(1)\n    return len(items)\n\n\nprint(f([]))\n",
        ),
        (
            true,
            "def first():\n    print('first')\n    return 1\n\n\ndef second():\n    print('second')\n    return 2\n\n\ndef f():\n    «v» = second()\n    return first() + v\n\n\nprint(f())\n",
            "would then run after `first()`",
            "def first():\n    print('first')\n    return 1\n\n\ndef second():\n    print('second')\n    return 2\n\n\ndef f():\n    return first() + second()\n\n\nprint(f())\n",
        ),
        (
            true,
            "def noisy():\n    print('noisy')\n    return 1\n\n\ndef f(flag):\n    «v» = noisy()\n    return flag and v\n\n\nprint(f(False))\n",
            "not evaluated exactly once",
            "def noisy():\n    print('noisy')\n    return 1\n\n\ndef f(flag):\n    return flag and noisy()\n\n\nprint(f(False))\n",
        ),
        (
            true,
            "def noisy():\n    print('noisy')\n    return 1\n\n\ndef f(xs):\n    «v» = noisy()\n    for x in xs:\n        print(x + v)\n\n\nf([1, 2])\n",
            "inside a block that does not run exactly once",
            "def noisy():\n    print('noisy')\n    return 1\n\n\ndef f(xs):\n    for x in xs:\n        print(x + noisy())\n\n\nf([1, 2])\n",
        ),
        (
            true,
            "def f(a):\n    «v» = a + 1\n    a = 10\n    return v\n\n\nprint(f(1))\n",
            "assigned again on line 3",
            "def f(a):\n    a = 10\n    return a + 1\n\n\nprint(f(1))\n",
        ),
        (
            true,
            "def f(a, xs):\n    «v» = a * 2\n    for a in xs:\n        pass\n    return v\n\n\nprint(f(1, [5]))\n",
            "assigned again on line 3",
            "def f(a, xs):\n    for a in xs:\n        pass\n    return a * 2\n\n\nprint(f(1, [5]))\n",
        ),
        (
            true,
            "class Counter:\n    def __init__(self):\n        self.count = 0\n\n    def bump(self):\n        self.count += 1\n\n    def f(self):\n        «v» = self.count\n        self.bump()\n        return v\n\n\nprint(Counter().f())\n",
            "`self.bump()` on line 10 may change",
            "class Counter:\n    def __init__(self):\n        self.count = 0\n\n    def bump(self):\n        self.count += 1\n\n    def f(self):\n        self.bump()\n        return self.count\n\n\nprint(Counter().f())\n",
        ),
        (
            true,
            "def f(xs):\n    «v» = xs[0]\n    xs[0] = 9\n    return v\n\n\nprint(f([1]))\n",
            "`xs[0]` on line 3 changes it",
            "def f(xs):\n    xs[0] = 9\n    return xs[0]\n\n\nprint(f([1]))\n",
        ),
        (
            true,
            "def f():\n    «v» = []\n    v.append(1)\n    return v\n\n\nprint(f())\n",
            "would no longer share one",
            "def f():\n    [].append(1)\n    return []\n\n\nprint(f())\n",
        ),
        (
            true,
            "def f(xs):\n    «acc» = []\n    for x in xs:\n        acc.append(x)\n        print(len(acc))\n\n\nf([1, 2])\n",
            "would no longer share one",
            "def f(xs):\n    for x in xs:\n        [].append(x)\n        print(len([]))\n\n\nf([1, 2])\n",
        ),
        (
            true,
            "def f(a):\n    «v» = a + 1\n    g = lambda: v\n    a = 10\n    return g()\n\n\nprint(f(1))\n",
            "it is also used in a `lambda`",
            "def f(a):\n    g = lambda: a + 1\n    a = 10\n    return g()\n\n\nprint(f(1))\n",
        ),
        (
            true,
            "def f(a, xs):\n    «v» = a + 1\n    return [v for a in xs]\n\n\nprint(f(1, [5, 6]))\n",
            "would hide the `a` its value reads",
            "def f(a, xs):\n    return [a + 1 for a in xs]\n\n\nprint(f(1, [5, 6]))\n",
        ),
        (
            true,
            "def f(a, flag):\n    if flag:\n        «v» = a + 1\n    else:\n        pass\n    try:\n        return v\n    except NameError:\n        return 'unset'\n\n\nprint(f(1, False))\n",
            "can be reached without the assignment",
            "def f(a, flag):\n    if flag:\n        pass\n    else:\n        pass\n    try:\n        return a + 1\n    except NameError:\n        return 'unset'\n\n\nprint(f(1, False))\n",
        ),
        (
            false,
            "def f():\n    xs = «[1]»\n    xs.append(2)\n    return xs\n\n\nf()\nprint(f())\n",
            "creates a new list",
            "CONSTANT = [1]\n\n\ndef f():\n    xs = CONSTANT\n    xs.append(2)\n    return xs\n\n\nf()\nprint(f())\n",
        ),
        (
            false,
            "N = 1\n\n\ndef bump():\n    global N\n    N = N + 1\n\n\ndef f():\n    return «N * 10»\n\n\nbump()\nprint(f())\n",
            "a function assigns `N` with `global`",
            "N = 1\n\n\ndef bump():\n    global N\n    N = N + 1\n\n\nCONSTANT = N * 10\n\n\ndef f():\n    return CONSTANT\n\n\nbump()\nprint(f())\n",
        ),
        (
            false,
            "def f():\n    return «N * 10»\n\n\nN = 2\nprint(f())\n",
            "`N` is not bound above",
            "CONSTANT = N * 10\n\n\ndef f():\n    return CONSTANT\n\n\nN = 2\nprint(f())\n",
        ),
        (
            false,
            "ITEMS = []\n\n\ndef count():\n    return «len(ITEMS)»\n\n\nITEMS.append(1)\nprint(count())\n",
            "`ITEMS` is not a constant itself",
            "ITEMS = []\n\n\nLEN = len(ITEMS)\n\n\ndef count():\n    return LEN\n\n\nITEMS.append(1)\nprint(count())\n",
        ),
        (
            false,
            "class Config:\n    debug = False\n\n\ncfg = Config()\n\n\ndef level():\n    return «cfg.debug»\n\n\ncfg.debug = True\nprint(level())\n",
            "`cfg` is not a constant itself",
            "class Config:\n    debug = False\n\n\ncfg = Config()\n\n\nDEBUG = cfg.debug\n\n\ndef level():\n    return DEBUG\n\n\ncfg.debug = True\nprint(level())\n",
        ),
        (
            false,
            "import time\n\n\ndef stamp():\n    return «time.monotonic_ns()»\n\n\na = stamp()\nprint(a < stamp() or a == 0)\n",
            "it calls `time.monotonic_ns`",
            "import time\n\n\nMONOTONIC_NS = time.monotonic_ns()\n\n\ndef stamp():\n    return MONOTONIC_NS\n\n\na = stamp()\nprint(a < stamp() or a == 0)\n",
        ),
        (
            false,
            "def f(n):\n    return «n * 2»\n\n\nprint(f(2))\n",
            "it reads `n`",
            "CONSTANT = n * 2\n\n\ndef f(n):\n    return CONSTANT\n\n\nprint(f(2))\n",
        ),
    ];
    for (inline, source, reason, by_hand) in cases {
        let (text, range) = marked(source);
        let why = if *inline {
            inline_variable(&text, &range)
        } else {
            extract_constant(&text, &range)
        }
        .expect_err(source);
        assert!(why.contains(reason), "{why:?} does not name {reason:?}");
        if !python3() {
            eprintln!("skipped the python3 part: no python3");
            continue;
        }
        let (before, _, ok) = run(&text);
        assert!(ok, "{text}");
        let (after, _, ok) = run(by_hand);
        assert!(!ok || before != after, "{by_hand}");
    }
}

/// SPEC-0330 v4 behaviours 17 and 22, AK-16: Inline Variable and Extract Constant through the
/// entry points of the window and the MCP server, on unsaved text, with or without pyright.
#[tokio::test(flavor = "multi_thread")]
async fn inline_and_constant_through_the_entry_points() {
    const PROBE: &str = "def describe(item, factor=1):\n    \"\"\"Return a label for an item.\"\"\"\n    value = item.total() * factor\n    unused = 3\n    return f\"{item.name}: {value}\" + str(60 * 60)\n";
    let wait = std::time::Duration::from_secs(60);
    let root = std::env::temp_dir().join(format!("idecraft-python-inline-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("probe.py");
    // The file on disk is older than the editor's text.
    std::fs::write(&file, "def describe(item):\n    pass\n").unwrap();
    let at = |line, from, to| Range {
        start_line: line,
        start_character: from,
        end_line: line,
        end_character: to,
    };

    // A caret in `value`, at its assignment and at its read.
    for caret in [at(2, 6, 6), at(4, 30, 30)] {
        let found =
            refactoring_targets(&root, &file, Some(PROBE), &caret, Refactoring::Inline, wait)
                .await
                .unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].text, "value");
        assert_eq!(found[0].actions[0].title, INLINE_VARIABLE);
        assert!(Refactoring::Inline.matches(&found[0].actions[0]));
        let plan = plan_code_action_in(&root, &file, Some(PROBE), &caret, INLINE_VARIABLE, wait)
            .await
            .unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].path, file);
        assert_eq!(
            apply_edits(PROBE, &plan[0].edits).unwrap(),
            "def describe(item, factor=1):\n    \"\"\"Return a label for an item.\"\"\"\n    unused = 3\n    return f\"{item.name}: {item.total() * factor}\" + str(60 * 60)\n"
        );
    }
    // Refactor This lists it at the caret.
    let found = refactoring_targets(
        &root,
        &file,
        Some(PROBE),
        &at(2, 6, 6),
        Refactoring::Any,
        wait,
    )
    .await
    .unwrap();
    assert!(
        found
            .iter()
            .any(|t| t.actions.iter().any(|a| a.title == INLINE_VARIABLE)),
        "{found:?}"
    );

    // A function is refused with what is supported, in place of the server's message.
    let on_function = at(0, 6, 6);
    let found = refactoring_targets(
        &root,
        &file,
        Some(PROBE),
        &on_function,
        Refactoring::Inline,
        wait,
    )
    .await
    .unwrap();
    assert!(found.is_empty(), "{found:?}");
    let message = idecraft_languages::actions::own_refusal(
        &file,
        Some(PROBE),
        &on_function,
        Refactoring::Inline,
    )
    .unwrap();
    assert_eq!(
        message,
        "Cannot inline `describe`: it is a function, and inlining a function is not supported; Inline supports a local variable of a function that is assigned once, by `name = expression`."
    );
    let err = plan_code_action_in(
        &root,
        &file,
        Some(PROBE),
        &on_function,
        INLINE_VARIABLE,
        wait,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains(&message), "{err}");

    // Extract Constant: the selection, and a bare caret widened to the constant expressions.
    let selected = at(4, 41, 48);
    let found = refactoring_targets(
        &root,
        &file,
        Some(PROBE),
        &selected,
        Refactoring::Constant,
        wait,
    )
    .await
    .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, "60 * 60");
    assert_eq!(found[0].actions[0].title, EXTRACT_CONSTANT);
    assert!(Refactoring::Constant.matches(&found[0].actions[0]));
    let plan = plan_code_action_in(&root, &file, Some(PROBE), &selected, EXTRACT_CONSTANT, wait)
        .await
        .unwrap();
    let after = apply_edits(PROBE, &plan[0].edits).unwrap();
    assert!(
        after.starts_with("CONSTANT = 60 * 60\n\n\ndef describe(item, factor=1):\n")
            && after.ends_with(" + str(CONSTANT)\n"),
        "{after}"
    );
    assert!(idecraft_languages::actions::introduces_name(
        EXTRACT_CONSTANT
    ));
    let name = idecraft_languages::actions::introduced_name(PROBE, &after).unwrap();
    assert_eq!(
        (name.name.as_str(), name.line, name.character),
        ("CONSTANT", 0, 0)
    );
    let found = refactoring_targets(
        &root,
        &file,
        Some(PROBE),
        &at(4, 42, 42),
        Refactoring::Constant,
        wait,
    )
    .await
    .unwrap();
    let texts: Vec<&str> = found.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, ["60", "60 * 60", "str(60 * 60)"]);

    // A refusal says why.
    let local = at(2, 12, 33);
    let found = refactoring_targets(
        &root,
        &file,
        Some(PROBE),
        &local,
        Refactoring::Constant,
        wait,
    )
    .await
    .unwrap();
    assert!(found.is_empty(), "{found:?}");
    let message =
        idecraft_languages::actions::own_refusal(&file, Some(PROBE), &local, Refactoring::Constant)
            .unwrap();
    assert!(
        message.starts_with("Cannot extract `item.total() * factor` into a constant: it "),
        "{message}"
    );
    let err = plan_code_action_in(&root, &file, Some(PROBE), &local, EXTRACT_CONSTANT, wait)
        .await
        .unwrap_err();
    assert!(err.to_string().contains(&message), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}
