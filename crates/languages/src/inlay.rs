//! Parameter name hints from the language server (`SPEC-0291`): the request over the whole file and
//! the parsing of `textDocument/inlayHint` answers.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::insight::{Encoding, Opened, from_server, to_server};
use crate::lsp::LspError;

/// One parameter name hint, in characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParameterHint {
    /// 0-based line.
    pub line: u64,
    /// Character the hint stands before.
    pub character: u64,
    /// The label, e.g. `left:`.
    pub label: String,
    /// A space before the label.
    pub padding_left: bool,
    /// A space after the label.
    pub padding_right: bool,
    /// A type hint (`: string` after a name or parameter list; behaviour 2a), not a parameter name.
    pub is_type: bool,
    /// The value of an enum member without an initializer (`= 0` after its name; behaviour 2b),
    /// not a parameter name.
    pub is_value: bool,
}

impl ParameterHint {
    /// The parameter's name: the label without its colon.
    pub fn name(&self) -> &str {
        self.label.trim().trim_end_matches(':').trim()
    }
}

/// The range of the whole of `text` in server units: from the start to the end of its last line,
/// never past it (behaviour 1).
pub fn whole_range(text: &str, encoding: Encoding) -> Value {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.len().saturating_sub(1);
    let line = lines[last].strip_suffix('\r').unwrap_or(lines[last]);
    let end = to_server(line, line.chars().count() as u64, encoding);
    json!({"start": {"line": 0, "character": 0}, "end": {"line": last, "character": end}})
}

/// The parameter name hints of an answer (behaviour 2): kind `Parameter`, or no kind and a label
/// ending with a colon; label parts are joined.
pub fn parse(answer: &Value, text: &str, encoding: Encoding) -> Vec<ParameterHint> {
    parse_kinds(answer, text, encoding, false)
}

/// Like [`parse`], and with `types` (JavaScript and TypeScript) also the type hints (kind `Type`;
/// behaviour 2a) and the values of enum members, which typescript-language-server sends without a
/// kind and with a label starting with `=` (behaviour 2b).
pub fn parse_kinds(
    answer: &Value,
    text: &str,
    encoding: Encoding,
    types: bool,
) -> Vec<ParameterHint> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut out: Vec<ParameterHint> = answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let label = match &h["label"] {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts.iter().filter_map(|p| p["value"].as_str()).collect(),
                _ => return None,
            };
            let parameter = match h["kind"].as_u64() {
                Some(kind) => kind == 2,
                None => label.trim_end().ends_with(':'),
            };
            let is_type = types && h["kind"].as_u64() == Some(1);
            let is_value = types && h["kind"].is_null() && label.trim_start().starts_with('=');
            if !(parameter || is_type || is_value) || label.trim().is_empty() {
                return None;
            }
            let line = h["position"]["line"].as_u64()?;
            let units = h["position"]["character"].as_u64()?;
            let source = lines.get(line as usize)?;
            let character = from_server(source, units, encoding);
            // A parameter name right after the opening bracket or after a space needs no space
            // before it, whatever the server asks for: `Cat(name: "Tom")` (behaviour 4a, v6).
            let before = (character as usize)
                .checked_sub(1)
                .and_then(|i| source.chars().nth(i));
            let opens = before.is_none_or(|c| c.is_whitespace() || matches!(c, '(' | '[' | '{'));
            Some(ParameterHint {
                line,
                character,
                label: label.trim().to_owned(),
                padding_left: h["paddingLeft"].as_bool().unwrap_or(false)
                    && (is_type || is_value || !opens),
                padding_right: h["paddingRight"].as_bool().unwrap_or(false),
                is_type,
                is_value,
            })
        })
        .collect();
    out.sort_by_key(|h| (h.line, h.character));
    out
}

/// Whether the argument starting at char `at` of `chars` already says what it is or is the call's
/// only argument (behaviour 3), so the hint `name` is left out.
pub fn self_describing(chars: &[char], at: usize, name: &str) -> bool {
    argument_says_it(chars, at, name, true)
}

/// Whether the argument starting at char `at` is directly in a `new Type(...)` call (behaviour 3c),
/// also with type arguments or a qualified name.
fn constructor_argument(chars: &[char], at: usize) -> bool {
    let mut i = at.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 || chars[i - 1] != '(' {
        return false;
    }
    i -= 1;
    // The type: names, dots and balanced type arguments, back to the word `new`.
    let mut depth = 0i32;
    while i > 0 {
        let c = chars[i - 1];
        match c {
            '>' => depth += 1,
            '<' => depth -= 1,
            _ if depth > 0 => {}
            _ if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' || c.is_whitespace() => {}
            _ => break,
        }
        if depth == 0
            && chars[i - 1..].starts_with(&['n', 'e', 'w'])
            && chars.get(i + 2).is_some_and(|c| c.is_whitespace())
            && (i < 2 || !(chars[i - 2].is_alphanumeric() || chars[i - 2] == '_'))
        {
            return true;
        }
        i -= 1;
    }
    false
}

/// Whether the argument starting at char `at` starts with a literal: a string, a character, a
/// number (also negative), `true`, `false` or `null` (behaviour 3d).
pub fn literal_argument(chars: &[char], at: usize) -> bool {
    let rest = &chars[at.min(chars.len())..];
    let word: String = rest
        .iter()
        .take_while(|c| c.is_alphanumeric() || **c == '_')
        .collect();
    match rest.first() {
        Some('"' | '\'') => true,
        Some('-') => rest.get(1).is_some_and(char::is_ascii_digit),
        Some(c) if c.is_ascii_digit() => true,
        _ => matches!(word.as_str(), "true" | "false" | "null"),
    }
}

/// Whether the argument starting at char `at` is directly in a Kotlin constructor call: the name
/// before the parenthesis starts with an upper-case letter (behaviour 3d).
pub fn kotlin_constructor_argument(chars: &[char], at: usize) -> bool {
    let mut i = at.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 || chars[i - 1] != '(' {
        return false;
    }
    i -= 1;
    let end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }
    i < end && chars[i].is_uppercase()
}

/// Behaviour 3; `only_argument` also counts the call's only argument as saying what it is (not in
/// JavaScript and TypeScript, behaviour 3b).
fn argument_says_it(chars: &[char], at: usize, name: &str, only_argument: bool) -> bool {
    let before = chars[..at.min(chars.len())]
        .iter()
        .rev()
        .find(|c| !c.is_whitespace());
    // The argument's text up to the next comma or closing bracket at its own depth.
    let mut depth = 0i32;
    let mut end = at;
    let mut closes = false;
    let mut quote: Option<char> = None;
    let mut i = at;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else {
            match c {
                '"' | '\'' => quote = Some(c),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => {
                    closes = c == ')';
                    break;
                }
                ')' | ']' | '}' => depth -= 1,
                ',' | ';' if depth == 0 => break,
                _ => {}
            }
        }
        i += 1;
        end = i;
    }
    if only_argument && before == Some(&'(') && closes {
        return true;
    }
    let argument: String = chars[at.min(end)..end].iter().collect();
    let argument = argument.trim();
    let argument = argument.strip_suffix("()").unwrap_or(argument);
    let last = argument
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .next()
        .unwrap_or_default();
    if last.is_empty() || last.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    let named = |s: &str| s.eq_ignore_ascii_case(name);
    named(last)
        || ["get", "is"].iter().any(|p| {
            last.len() > p.len()
                && last[..p.len()].eq_ignore_ascii_case(p)
                && named(&last[p.len()..])
        })
}

/// Generic names of a varargs parameter that say nothing about one argument (behaviour 3a).
const GENERIC_VARARGS: &[&str] = &[
    "elements", "items", "values", "objects", "args", "others", "rest",
];

/// Whether the parameter name `name` says nothing: one letter followed by digits (`e1`, `k2`) or a
/// generic varargs name (behaviour 3a).
pub fn meaningless_name(name: &str) -> bool {
    let name = name.trim_start_matches('.');
    let mut chars = name.chars();
    let numbered = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.len() > 1
        && chars.all(|c| c.is_ascii_digit());
    numbered || GENERIC_VARARGS.iter().any(|g| g.eq_ignore_ascii_case(name))
}

/// The name of the method whose call holds the argument starting at char `at`: the identifier
/// before the unmatched opening bracket left of it, after explicit type arguments (`List.<T>of`).
pub fn called_method(chars: &[char], at: usize) -> Option<String> {
    let mut depth = 0i32;
    let mut i = at.min(chars.len());
    let open = loop {
        if i == 0 {
            return None;
        }
        i -= 1;
        match chars[i] {
            ')' | ']' | '}' => depth += 1,
            '(' if depth == 0 => break i,
            '[' | '{' if depth == 0 => return None,
            '(' | '[' | '{' => depth -= 1,
            ';' if depth == 0 => return None,
            _ => {}
        }
    };
    let mut end = open;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && (chars[start - 1].is_alphanumeric() || matches!(chars[start - 1], '_' | '$'))
    {
        start -= 1;
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

/// Whether `method` is a collection factory: `of`, `copyOf`, `ofEntries`, `asList` or a name ending
/// in `Of` (`listOf`, `mapOf`) (behaviour 3a).
pub fn collection_factory(method: &str) -> bool {
    matches!(method, "of" | "copyOf" | "ofEntries" | "asList")
        || (method.len() > 2 && method.ends_with("Of"))
}

/// Whether the hint `label` before the argument starting at char `at` names a parameter that says
/// nothing in a collection factory or varargs call (behaviour 3a).
pub fn meaningless_factory_hint(chars: &[char], at: usize, label: &str) -> bool {
    let label = label.trim().trim_end_matches(':').trim();
    if !meaningless_name(label) {
        return false;
    }
    label.starts_with("...") || called_method(chars, at).is_some_and(|m| collection_factory(&m))
}

/// Parameter name hints of `file` whose content is `text` (the editor's text) or the disk content,
/// without those behaviours 3 and 3a leave out. A server without inlay hints gives none.
pub async fn parameter_hints_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<ParameterHint>, LspError> {
    let o = Opened::open_text(root, file, text, wait).await?;
    let offered = o
        .capabilities
        .get("inlayHintProvider")
        .is_some_and(|p| p.as_bool() == Some(true) || p.is_object());
    if !offered {
        o.close().await;
        return Ok(Vec::new());
    }
    let uri = o.uri.clone();
    let encoding = o.files.encoding;
    let script = crate::LanguageId::detect(file).is_some_and(|l| {
        matches!(
            l,
            crate::LanguageId::JavaScript | crate::LanguageId::TypeScript
        )
    });
    let kotlin = crate::kotlin_structure::is_kotlin(file);
    let result = o
        .ask(
            "textDocument/inlayHint",
            json!({"textDocument": {"uri": uri}, "range": whole_range(&o.text, encoding)}),
        )
        .await
        .map(|v| {
            let hints = parse_kinds(&v, &o.text, encoding, script);
            let chars: Vec<char> = o.text.chars().collect();
            let starts = line_starts(&o.text);
            hints
                .into_iter()
                .filter(|h| {
                    let at =
                        starts.get(h.line as usize).copied().unwrap_or(0) + h.character as usize;
                    if kotlin {
                        // kotlin-language-server names every argument (behaviour 3d).
                        return literal_argument(&chars, at)
                            && !argument_says_it(
                                &chars,
                                at,
                                h.name(),
                                !kotlin_constructor_argument(&chars, at),
                            )
                            && !meaningless_factory_hint(&chars, at, &h.label);
                    }
                    h.is_type
                        || h.is_value
                        || (!argument_says_it(
                            &chars,
                            at,
                            h.name(),
                            !script && !constructor_argument(&chars, at),
                        ) && !meaningless_factory_hint(&chars, at, &h.label))
                })
                .collect()
        });
    o.close().await;
    result
}

/// Char offset of each line start of `text`.
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0291 AK-2d (v5): Kotlin keeps the hints before literals; a constructor call is told by
    /// its upper-case name, so its only argument keeps its hint.
    #[test]
    fn kotlin_literal_arguments() {
        let at = |text: &str, needle: &str| {
            let chars: Vec<char> = text.chars().collect();
            let i = text.find(needle).unwrap();
            (chars, text[..i].chars().count())
        };
        for (text, needle, literal) in [
            ("Cat(\"Tom\", owner)", "\"Tom\"", true),
            ("Cat(\"Tom\", owner)", "owner", false),
            ("Circle(2.0)", "2.0", true),
            ("move(-1, 'c')", "-1", true),
            ("move(-1, 'c')", "'c'", true),
            ("move(-x, y)", "-x", false),
            ("set(true, null)", "true", true),
            ("set(true, null)", "null", true),
            ("set(trueish, nullable)", "trueish", false),
            ("set(falsey, f())", "f()", false),
        ] {
            let (chars, i) = at(text, needle);
            assert_eq!(literal_argument(&chars, i), literal, "{text} at {needle}");
        }
        let (chars, i) = at("Shape.Circle(2.0)", "2.0");
        assert!(kotlin_constructor_argument(&chars, i));
        let (chars, i) = at("Circle( 2.0)", "2.0");
        assert!(kotlin_constructor_argument(&chars, i));
        let (chars, i) = at("println(2.0)", "2.0");
        assert!(!kotlin_constructor_argument(&chars, i));
        let (chars, i) = at("Cat(\"Tom\", \"Ann\")", "\"Ann\"");
        assert!(!kotlin_constructor_argument(&chars, i));
        // The only argument of a function says nothing new; that of a constructor does.
        let (chars, i) = at("println(\"x\")", "\"x\"");
        assert!(argument_says_it(
            &chars,
            i,
            "message",
            !kotlin_constructor_argument(&chars, i)
        ));
        let (chars, i) = at("Circle(2.0)", "2.0");
        assert!(!argument_says_it(
            &chars,
            i,
            "radius",
            !kotlin_constructor_argument(&chars, i)
        ));
    }

    /// SPEC-0291 AK-1: jdtls answers without kinds; another server with kinds and label parts.
    #[test]
    fn parses_parameter_hints() {
        let text = "class A {\n  /*𝔘*/ int x = add(1, count);\n}\n";
        let jdtls = json!([
            {"position": {"line": 1, "character": 21}, "label": "left:", "paddingRight": true},
            {"position": {"line": 1, "character": 24}, "label": "right:", "paddingRight": true}
        ]);
        let hints = parse(&jdtls, text, Encoding::Utf16);
        assert_eq!(hints.len(), 2);
        assert_eq!((hints[0].line, hints[0].character), (1, 20));
        assert_eq!(hints[0].name(), "left");
        assert!(hints[0].padding_right && !hints[0].padding_left);
        let line: Vec<char> = text.lines().nth(1).unwrap().chars().collect();
        assert_eq!(line[20], '1');
        assert_eq!(line[23], 'c');
        let other = json!([
            {"position": {"line": 0, "character": 6}, "label": ": i32", "kind": 1},
            {"position": {"line": 0, "character": 2}, "label": [{"value": "na"}, {"value": "me:"}], "kind": 2},
            {"position": {"line": 0, "character": 1}, "label": "chained"}
        ]);
        let hints = parse(&other, "abcdefgh", Encoding::Utf16);
        assert_eq!(hints.len(), 1, "{hints:?}");
        assert_eq!(hints[0].label, "name:");
        assert_eq!(hints[0].character, 2);
    }

    /// SPEC-0291 AK-3a (v6): kotlin-language-server asks for a space on both sides of every
    /// parameter name (recorded from a real answer); right after the bracket and after a space the
    /// one before it is left out, after a comma without a space and for type hints it stays.
    #[test]
    fn no_space_before_a_hint_after_the_bracket() {
        let text = "val cat = Cat(\"Tom\", true)\nval n = total(1,2)\nval x = 1\n  call(\n    7)\n";
        let hint = |line: u64, character: u64, label: &str, kind: u64| {
            json!({"position": {"line": line, "character": character}, "label": label,
                "kind": kind, "paddingLeft": true, "paddingRight": true})
        };
        let answer = json!([
            hint(0, 14, "name:", 2),
            hint(0, 21, "friendly:", 2),
            hint(1, 14, "a:", 2),
            hint(1, 16, "b:", 2),
            hint(2, 5, ": Int", 1),
            hint(4, 4, "count:", 2),
        ]);
        let hints = parse_kinds(&answer, text, Encoding::Utf16, true);
        let padding: Vec<(&str, bool, bool)> = hints
            .iter()
            .map(|h| (h.label.as_str(), h.padding_left, h.padding_right))
            .collect();
        assert_eq!(
            padding,
            [
                ("name:", false, true),
                ("friendly:", false, true),
                ("a:", false, true),
                ("b:", true, true),
                (": Int", true, true),
                ("count:", false, true),
            ]
        );
    }

    /// SPEC-0291 AK-2e (v7): the answer of typescript-language-server 6.0.2 for a class field
    /// without an annotation and an enum without initializers (recorded): the field's type has kind
    /// `Type`, the members' values have no kind; both keep the space before them. Other languages
    /// drop them.
    #[test]
    fn field_types_and_enum_values() {
        let text = "class Counter {\n    count = 0;\n}\nenum Color { Red, Green }\n";
        let answer = json!([
            {"position": {"line": 1, "character": 9},
                "label": [{"value": ": "}, {"value": "number"}], "kind": 1, "paddingLeft": true},
            {"position": {"line": 3, "character": 16}, "label": "= 0", "paddingLeft": true},
            {"position": {"line": 3, "character": 23}, "label": "= 1", "paddingLeft": true}
        ]);
        let hints = parse_kinds(&answer, text, Encoding::Utf16, true);
        let seen: Vec<(u64, u64, &str, bool, bool, bool)> = hints
            .iter()
            .map(|h| {
                (
                    h.line,
                    h.character,
                    h.label.as_str(),
                    h.is_type,
                    h.is_value,
                    h.padding_left,
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                (1, 9, ": number", true, false, true),
                (3, 16, "= 0", false, true, true),
                (3, 23, "= 1", false, true, true),
            ]
        );
        assert!(parse(&answer, text, Encoding::Utf16).is_empty());
        // A parameter name is never a value hint, and a hint with a kind is not one either.
        let other = json!([
            {"position": {"line": 1, "character": 4}, "label": "name:", "kind": 2},
            {"position": {"line": 1, "character": 9}, "label": "= 0", "kind": 2}
        ]);
        assert!(
            parse_kinds(&other, text, Encoding::Utf16, true)
                .iter()
                .all(|h| !h.is_value)
        );
    }

    /// SPEC-0291 AK-2b: type hints only when asked for.
    #[test]
    fn type_hints_only_when_asked() {
        let answer = json!([
            {"position": {"line": 0, "character": 7}, "label": ": string", "kind": 1},
            {"position": {"line": 0, "character": 2}, "label": "name:", "kind": 2}
        ]);
        assert_eq!(parse(&answer, "abcdefgh", Encoding::Utf16).len(), 1);
        let both = parse_kinds(&answer, "abcdefgh", Encoding::Utf16, true);
        assert_eq!(both.len(), 2);
        assert!(!both[0].is_type && both[1].is_type);
    }

    #[test]
    fn the_range_ends_at_the_last_line() {
        assert_eq!(
            whole_range("ab\ncd𝔘e", Encoding::Utf16),
            json!({"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 5}})
        );
        assert_eq!(
            whole_range("ab\n", Encoding::Utf16)["end"],
            json!({"line": 1, "character": 0})
        );
    }

    fn hidden(call: &str, argument: &str, name: &str) -> bool {
        let chars: Vec<char> = call.chars().collect();
        let at = call
            .find(argument)
            .map(|b| call[..b].chars().count())
            .unwrap();
        self_describing(&chars, at, name)
    }

    /// SPEC-0291 AK-2: arguments that name their parameter, and only arguments, hide the hint.
    #[test]
    fn single_constructor_arguments_keep_their_hint() {
        let at = |t: &str, needle: &str| (t.chars().collect::<Vec<_>>(), t.find(needle).unwrap());
        for (t, n) in [
            ("new Dog(\"Rex\")", "\"Rex"),
            ("x = new a.b.Dog(1)", "1)"),
            ("new Box<List<String>>(1)", "1)"),
            ("new  Dog( 1 )", "1 "),
        ] {
            let (c, i) = at(t, n);
            assert!(constructor_argument(&c, i), "{t}");
        }
        for (t, n) in [
            ("renew(1)", "1)"),
            ("dog.bark(1)", "1)"),
            ("new Dog(f(1))", "1))"),
            ("newest(1)", "1)"),
        ] {
            let (c, i) = at(t, n);
            assert!(!constructor_argument(&c, i), "{t}");
        }
    }

    #[test]
    fn self_describing_arguments_hide_the_hint() {
        assert!(hidden("add(left, 2)", "left", "left"));
        assert!(!hidden("add(left, 2)", "2)", "right"));
        assert!(hidden("add(p.getLeft(), 2)", "p.getLeft", "left"));
        assert!(hidden("add(isLeft(), 2)", "isLeft", "left"));
        assert!(hidden("add(this.Left , 2)", "this.Left", "left"));
        assert!(!hidden("add(leftover, 2)", "leftover", "left"));
        assert!(!hidden("add(1, 2)", "1", "left"));
        // The only argument, also with nested calls, strings and brackets holding commas.
        assert!(hidden("println(5)", "5", "x"));
        assert!(hidden("println(f(1, 2))", "f(", "x"));
        assert!(hidden("println(\"a, b\")", "\"a", "x"));
        assert!(hidden("log(new int[] {1, 2})", "new", "values"));
        assert!(!hidden("put(\"a, b\", 3)", "\"a", "key"));
        assert!(!hidden("put(\"a\\\", b\", 3)", "\"a", "key"));
        // A call over several lines.
        assert!(!hidden("add(\n    1,\n    2)", "1", "left"));
    }

    fn factory_hidden(call: &str, argument: &str, label: &str) -> bool {
        let chars: Vec<char> = call.chars().collect();
        let at = call
            .find(argument)
            .map(|b| call[..b].chars().count())
            .unwrap();
        meaningless_factory_hint(&chars, at, label)
    }

    /// SPEC-0291 v2 AK-2a: meaningless names in collection factories and varargs hide the hint.
    #[test]
    fn factory_arguments_hide_meaningless_hints() {
        assert!(factory_hidden("var l = List.of(1, 2, 3);", "1", "e1:"));
        assert!(factory_hidden("var l = List.of(1, 2, 3);", "3", "e3:"));
        assert!(factory_hidden("Map.of(\"a\", 1)", "\"a", "k1:"));
        assert!(factory_hidden("Map.of(\"a\", 1)", "1)", "v1:"));
        assert!(factory_hidden("List.<Integer>of(1, 2)", "2", "e2:"));
        assert!(factory_hidden("Arrays.asList(1, 2)", "1", "elements:"));
        assert!(factory_hidden("listOf(1, 2)", "2", "e2:"));
        assert!(factory_hidden("print(f(x), 1, 2)", "1,", "...values:"));
        assert!(factory_hidden("List.of(f(1, 2), 3)", "3", "e2:"));
        // Not a factory, or a name that says something.
        assert!(!factory_hidden("g.drawLine(0, 0, 1, 1)", "1,", "x2:"));
        assert!(!factory_hidden("Path.of(\"a\", \"b\")", "\"a", "first:"));
        assert!(!factory_hidden("Path.of(\"a\", \"b\")", "\"b", "more:"));
        assert!(!factory_hidden("add(1, 2)", "1", "a:"));
        assert!(!factory_hidden("of(1, 2)", "1", "count:"));
        assert!(meaningless_name("e10") && meaningless_name("Elements"));
        assert!(!meaningless_name("e") && !meaningless_name("x") && !meaningless_name("e1x"));
        assert_eq!(
            called_method(&"a.b(List.of(1, 2))".chars().collect::<Vec<_>>(), 15),
            Some("of".into())
        );
        assert_eq!(
            called_method(&"x = 1, 2".chars().collect::<Vec<_>>(), 7),
            None
        );
    }
}
