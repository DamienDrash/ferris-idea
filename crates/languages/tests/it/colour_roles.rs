//! SPEC-0027 v6 AK-8: the colour roles of the three OBS-0033 samples (Python, TypeScript, SQL)
//! resolve in the `Dark` scheme to the specified keys and colours.

use ferris_idea_languages::LanguageId;
use ferris_idea_languages::syntax::SyntaxDocument;
use ferris_idea_ui::theme::{ColorScheme, TextStyle, ThemeId, capture_key};

/// The Python sample of the side-by-side comparison.
const PYTHON: &str = r#"import os
from dataclasses import dataclass

GREETING = "hello"  # a comment


@dataclass
class Point:
    x: int
    y: int = 0

    def norm(self, scale: float = 1.0) -> float:
        """Return the length."""
        return (self.x ** 2 + self.y ** 2) ** 0.5 * scale


def main(args: list[str]) -> None:
    p = Point(3, 4)
    print(f"{GREETING} {p.norm():.2f}", len(args), os.sep, None, True)
"#;

/// The TypeScript sample.
const TYPESCRIPT: &str = r#"import { readFileSync } from "fs";

// a comment
interface Shape { area(): number; readonly name: string }

enum Color { Red, Green = 2 }

export class Circle implements Shape {
  static count = 0;
  readonly name = "circle";
  constructor(private r: number) { Circle.count++; }
  area(): number { return Math.PI * this.r ** 2; }
}

const re = /a+b/g;
const tpl = `r=${new Circle(2).area()} ${Color.Red}`;
function load<T>(path: string): T | null { return JSON.parse(readFileSync(path, "utf8")) as T; }
console.log(tpl, re, load<number>("x.json"), undefined, true);
"#;

/// The SQL sample.
const SQL: &str = r#"-- a comment
CREATE TABLE person (
    id      INTEGER PRIMARY KEY,
    name    VARCHAR(100) NOT NULL,
    born    DATE DEFAULT CURRENT_DATE
);

SELECT p.name, COUNT(*) AS n
FROM person p
WHERE p.born > '2000-01-01' AND p.id IN (1, 2, 3)
GROUP BY p.name
ORDER BY n DESC;
"#;

/// The capture name, colour key and style of the `nth` (0-based) occurrence of `token` in `text`.
fn role(
    language: LanguageId,
    text: &str,
    token: &str,
    nth: usize,
) -> (String, &'static str, TextStyle) {
    let byte = text
        .match_indices(token)
        .filter(|(i, _)| {
            // Whole tokens only.
            let before = text[..*i].chars().next_back();
            let after = text[i + token.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            !(word(before) && token.starts_with(|c: char| c.is_alphanumeric()))
                && !(word(after) && token.ends_with(|c: char| c.is_alphanumeric()))
        })
        .nth(nth)
        .unwrap_or_else(|| panic!("`{token}` #{nth} not in the sample"))
        .0;
    let start = text[..byte].chars().count();
    let end = start + token.chars().count();
    let doc = SyntaxDocument::new(language, false, text).unwrap();
    let spans = doc.highlight();
    let span = spans
        .iter()
        .find(|s| s.start <= start && s.end >= end)
        .unwrap_or_else(|| panic!("`{token}` #{nth} has no span: {spans:?}"));
    let scheme = ColorScheme::for_theme(ThemeId::Dark);
    (
        span.kind.clone(),
        capture_key(&span.kind),
        scheme.capture_style(&span.kind),
    )
}

/// Asserts the key and the foreground (and italic) of a token in the `Dark` scheme.
fn check(
    language: LanguageId,
    text: &str,
    token: &str,
    nth: usize,
    key: &str,
    rgb: u32,
    italic: bool,
) {
    let (kind, k, style) = role(language, text, token, nth);
    assert_eq!(k, key, "`{token}` #{nth} ({kind})");
    assert_eq!(style.foreground, Some(rgb), "`{token}` #{nth} ({kind})");
    assert_eq!(style.italic, italic, "`{token}` #{nth} ({kind})");
}

/// Plain text in `Dark`.
const TEXT: u32 = 0xbcbec4;

/// OBS-0033 L1–L4.
#[test]
fn python_roles_resolve_in_dark() {
    let py = |token, nth, key, rgb, italic| {
        check(LanguageId::Python, PYTHON, token, nth, key, rgb, italic)
    };
    // L1: decorator → metadata (`PY.DECORATOR` → `DEFAULT_METADATA`), the `@` included.
    py("@dataclass", 0, "PY.DECORATOR", 0xb3ae60, false);
    // L2: `self` as first parameter and its uses → the plugin's self-parameter colour.
    py("self", 0, "PY.SELF_PARAMETER", 0x94558d, false);
    py("self", 1, "PY.SELF_PARAMETER", 0x94558d, false);
    py("self", 2, "PY.SELF_PARAMETER", 0x94558d, false);
    // L3: names in `def` → function declaration.
    py("norm", 0, "PY.FUNC_DEFINITION", 0x56a8f5, false);
    py("main", 0, "PY.FUNC_DEFINITION", 0x56a8f5, false);
    // A call of the method is no declaration.
    assert_ne!(
        role(LanguageId::Python, PYTHON, "norm", 1).1,
        "PY.FUNC_DEFINITION"
    );
    // L4: docstring → doc comment, italic.
    py(
        "\"\"\"Return the length.\"\"\"",
        0,
        "PY.DOC_COMMENT",
        0x5f826b,
        true,
    );
    // An ordinary string stays a string.
    py("\"hello\"", 0, "DEFAULT_STRING", 0x6aab73, false);

    // Keys without a value in a scheme fall back to the neutral key (behaviour v6).
    let dark = ColorScheme::for_theme(ThemeId::Dark);
    assert_eq!(
        dark.attributes("PY.DECORATOR"),
        dark.attributes("DEFAULT_METADATA")
    );
    assert_eq!(
        dark.attributes("PY.DOC_COMMENT"),
        dark.attributes("DEFAULT_DOC_COMMENT")
    );
    for id in ThemeId::ALL {
        let s = ColorScheme::for_theme(id);
        assert!(s.capture_style("attribute.python").foreground.is_some());
        assert_eq!(
            s.attributes("PY.FUNC_DEFINITION"),
            s.attributes("DEFAULT_FUNCTION_DECLARATION"),
            "{id:?}"
        );
    }
    // `self` of a static method and outside a class is no self parameter.
    let other = "class A:\n    @staticmethod\n    def f(self):\n        return self\n\ndef g(self):\n    pass\n";
    for nth in 0..3 {
        assert_ne!(
            role(LanguageId::Python, other, "self", nth).1,
            "PY.SELF_PARAMETER",
            "#{nth}"
        );
    }
}

/// OBS-0033 L6–L10.
#[test]
fn typescript_roles_resolve_in_dark() {
    let ts = |token, nth, key, rgb, italic| {
        check(
            LanguageId::TypeScript,
            TYPESCRIPT,
            token,
            nth,
            key,
            rgb,
            italic,
        )
    };
    // L6: method declarations in interface and class, and function declarations.
    ts("area", 0, "DEFAULT_FUNCTION_DECLARATION", 0x56a8f5, false);
    ts("area", 1, "DEFAULT_FUNCTION_DECLARATION", 0x56a8f5, false);
    ts("load", 0, "DEFAULT_FUNCTION_DECLARATION", 0x56a8f5, false);
    // `constructor` → keyword.
    ts("constructor", 0, "DEFAULT_KEYWORD", 0xcf8e6d, false);
    // L7: method and function calls → plain text.
    for (token, nth) in [
        ("area", 2),
        ("log", 0),
        ("parse", 0),
        ("readFileSync", 1),
        ("load", 1),
    ] {
        ts(token, nth, "DEFAULT_FUNCTION_CALL", TEXT, false);
    }
    // L8: global objects → global variable, italic.
    for token in ["Math", "JSON", "console"] {
        ts(token, 0, "JS.GLOBAL_VARIABLE", 0xc77dba, true);
    }
    // L9: the type parameter where declared and where used.
    for nth in 0..3 {
        ts("T", nth, "TYPE_PARAMETER_NAME_ATTRIBUTES", 0x16baac, false);
    }
    // L10: the regular expression, slashes and flags included.
    ts("/a+b/g", 0, "JS.REGEXP", 0x42c3d4, false);
    // Properties keep the field colour.
    ts("name", 0, "DEFAULT_INSTANCE_FIELD", 0xc77dbb, false);
    // JavaScript files get the same roles.
    let js = "function f() { return Math.max(1, 2); }\nf();\n";
    check(
        LanguageId::JavaScript,
        js,
        "f",
        0,
        "DEFAULT_FUNCTION_DECLARATION",
        0x56a8f5,
        false,
    );
    check(
        LanguageId::JavaScript,
        js,
        "max",
        0,
        "DEFAULT_FUNCTION_CALL",
        TEXT,
        false,
    );
    check(
        LanguageId::JavaScript,
        js,
        "Math",
        0,
        "JS.GLOBAL_VARIABLE",
        0xc77dba,
        true,
    );
}

/// OBS-0033 L12–L17.
#[test]
fn sql_roles_resolve_in_dark() {
    let sql =
        |token, nth, key, rgb, italic| check(LanguageId::Sql, SQL, token, nth, key, rgb, italic);
    // L12: column names in a column definition → column (`SQL_COLUMN` → instance field).
    for token in ["id", "name", "born"] {
        sql(token, 0, "SQL_COLUMN", 0xc77dbb, false);
    }
    // L13: data types and `NULL` → keyword.
    for token in ["INTEGER", "VARCHAR", "DATE", "NULL"] {
        sql(token, 0, "DEFAULT_KEYWORD", 0xcf8e6d, false);
    }
    // L14 (v7): built-in functions → own key, function-declaration colour, italic.
    for token in ["COUNT", "CURRENT_DATE"] {
        sql(token, 0, "SQL_BUILTIN_FUNCTION", 0x56a8f5, true);
    }
    // L15: every keyword, `DEFAULT` and `DESC` included.
    for token in ["DEFAULT", "DESC", "CREATE", "SELECT", "ORDER"] {
        sql(token, 0, "DEFAULT_KEYWORD", 0xcf8e6d, false);
    }
    // L16: numbers → number.
    for token in ["100", "1", "2", "3"] {
        sql(token, 0, "DEFAULT_NUMBER", 0x2aacb8, false);
    }
    // L17: an alias where it is used → plain text.
    sql("n", 1, "TEXT", TEXT, false);
    sql("p", 0, "TEXT", TEXT, false);
    sql("p", 2, "TEXT", TEXT, false);
    // The comment is a comment.
    sql("-- a comment", 0, "DEFAULT_LINE_COMMENT", 0x7a7e85, false);
    // A column of the same name as an alias elsewhere keeps its colour.
    let other = "SELECT a AS n FROM t;\nSELECT n FROM u ORDER BY n;\n";
    assert_eq!(
        role(LanguageId::Sql, other, "n", 1).1,
        "DEFAULT_INSTANCE_FIELD"
    );
    // `ASC` and `CURRENT_TIMESTAMP` too.
    let more = "SELECT CURRENT_TIMESTAMP, SUM(x) FROM t ORDER BY x ASC;\n";
    check(
        LanguageId::Sql,
        more,
        "ASC",
        0,
        "DEFAULT_KEYWORD",
        0xcf8e6d,
        false,
    );
    check(
        LanguageId::Sql,
        more,
        "SUM",
        0,
        "SQL_BUILTIN_FUNCTION",
        0x56a8f5,
        true,
    );
}
