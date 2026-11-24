//! Rules behind the Debug window's further actions (`SPEC-0091` v12 behaviours 25–38) and the
//! further breakpoint options (`SPEC-0127` v3): the code block around a line, thread states, value
//! views and renderers, variable filters, code fragments, stack lines in the console and the
//! filters of a breakpoint. Pure functions, shared by the window and the MCP tools.

use serde::{Deserialize, Serialize};

/// The language a session debugs, as far as the rules here differ (`SPEC-0091` v12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// Java or Kotlin on the JVM (java-debug, kotlin-debug-adapter).
    Java,
    /// Python (debugpy).
    Python,
    /// Anything else (lldb-dap, js-debug).
    Other,
}

impl Lang {
    /// The language of adapter id `adapter_id` (`java`, `kotlin`, `debugpy`, …).
    pub fn of_adapter(adapter_id: &str) -> Self {
        match adapter_id {
            "java" | "java-debug" | "kotlin" | "kotlin-debug-adapter" => Lang::Java,
            "debugpy" | "python" => Lang::Python,
            _ => Lang::Other,
        }
    }
}

/// The innermost code block around 0-based line `exec` of `lines` as 0-based first and last line
/// (Step Out of Code Block, `SPEC-0091` v12 behaviour 25): for braces the lines from the opening
/// brace's line (or the header line above a brace that stands alone) to the closing brace; for
/// Python the header line ending with `:` above the line, indented less, through the last line
/// indented deeper. `None` when the line is in no block.
pub fn block_lines(lines: &[&str], exec: usize, python: bool) -> Option<(usize, usize)> {
    if exec >= lines.len() {
        return None;
    }
    if python {
        return python_block(lines, exec);
    }
    // Opening braces still open at the start of `exec`, innermost last.
    let mut open: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate().take(exec) {
        for c in code_chars(line) {
            match c {
                '{' => open.push(i),
                '}' => {
                    open.pop();
                }
                _ => {}
            }
        }
    }
    let start = *open.last()?;
    // The block ends where the brace open at the start of `exec` closes.
    let outer = open.len();
    let mut depth = outer;
    let mut end = None;
    'lines: for (i, line) in lines.iter().enumerate().skip(exec) {
        for c in code_chars(line) {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth < outer {
                        end = Some(i);
                        break 'lines;
                    }
                }
                _ => {}
            }
        }
    }
    let end = end.unwrap_or(lines.len() - 1);
    // A brace on a line of its own belongs to the header above it (`for (…)` then `{`).
    let first = if lines[start].trim() == "{" {
        (0..start)
            .rev()
            .find(|&l| !lines[l].trim().is_empty())
            .unwrap_or(start)
    } else {
        start
    };
    Some((first, end))
}

/// The characters of `line` that are code: without string and character literals and without a
/// `//` comment.
fn code_chars(line: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == '\\' {
                    chars.next();
                } else if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c == '/' && chars.peek() == Some(&'/') {
                    break;
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn indent(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

fn python_block(lines: &[&str], exec: usize) -> Option<(usize, usize)> {
    let blank = |l: &str| {
        let t = l.trim();
        t.is_empty() || t.starts_with('#')
    };
    let own = indent(lines[exec]);
    let header = (0..exec).rev().find(|&l| {
        !blank(lines[l]) && indent(lines[l]) < own && lines[l].trim_end().ends_with(':')
    })?;
    let depth = indent(lines[header]);
    let mut end = header;
    for (i, line) in lines.iter().enumerate().skip(header + 1) {
        if blank(line) {
            continue;
        }
        if indent(line) <= depth {
            break;
        }
        end = i;
    }
    Some((header, end))
}

/// The Java code fragment that lists every thread of the JVM as `name|state|group;`
/// (`SPEC-0091` v12 behaviour 28), evaluated in a stopped frame. A timed wait in `Thread.sleep`
/// is reported as `SLEEPING` (v13).
pub const JAVA_THREAD_STATES: &str = "StringBuilder b = new StringBuilder(); for (java.util.Map.Entry<Thread, StackTraceElement[]> e : Thread.getAllStackTraces().entrySet()) { Thread t = e.getKey(); StackTraceElement[] s = e.getValue(); String state = t.getState().toString(); if (state.equals(\"TIMED_WAITING\") && s.length > 0 && s[0].getClassName().equals(\"java.lang.Thread\") && s[0].getMethodName().startsWith(\"sleep\")) { state = \"SLEEPING\"; } b.append(t.getName()).append('|').append(state).append('|').append(t.getThreadGroup() == null ? \"\" : t.getThreadGroup().getName()).append(';'); } return b.toString();";

/// A thread as the threads list and the thread dump show it (`SPEC-0091` v12 behaviours 26, 28).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ThreadInfo {
    /// The adapter's thread id.
    pub id: i64,
    /// The thread's name (java-debug's `Thread [main]` shown as `main`).
    pub name: String,
    /// Ferris IDEA's state text: `RUNNING`, `WAIT`, `SLEEPING`, `MONITOR`, `NEW`, `ZOMBIE`, or empty
    /// when the adapter cannot tell.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub state: String,
    /// The thread group, empty when unknown.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Whether the thread is suspended (its frames can be read).
    pub suspended: bool,
}

/// java-debug names threads `Thread [main]`; the window shows `main`.
pub fn thread_name(adapter_name: &str) -> &str {
    adapter_name
        .strip_prefix("Thread [")
        .and_then(|n| n.strip_suffix(']'))
        .unwrap_or(adapter_name)
}

/// The state text for a Java `Thread.State` (`SPEC-0091` v12 behaviour 28): RUNNABLE is RUNNING,
/// WAITING and TIMED_WAITING are WAIT, BLOCKED is MONITOR, TERMINATED is ZOMBIE; a timed wait in
/// `Thread.sleep` is SLEEPING (v13, as the reference shows it).
pub fn java_state(state: &str) -> &'static str {
    match state {
        "RUNNABLE" => "RUNNING",
        "SLEEPING" => "SLEEPING",
        "WAITING" | "TIMED_WAITING" => "WAIT",
        "BLOCKED" => "MONITOR",
        "NEW" => "NEW",
        "TERMINATED" => "ZOMBIE",
        _ => "",
    }
}

/// Name, state text and group of each thread in the answer of [`JAVA_THREAD_STATES`] (quoted by
/// the adapter).
pub fn parse_thread_states(result: &str) -> Vec<(String, String, String)> {
    let text = result.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    text.split(';')
        .filter_map(|entry| {
            let mut parts = entry.rsplitn(3, '|');
            let group = parts.next()?;
            let state = parts.next()?;
            let name = parts.next()?;
            Some((
                name.to_owned(),
                java_state(state).to_owned(),
                group.to_owned(),
            ))
        })
        .collect()
}

/// How a value is shown (`SPEC-0091` v12 behaviour 32, "View as").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewAs {
    /// As the adapter shows it.
    #[default]
    Auto,
    /// An integer in decimal.
    Decimal,
    /// An integer in hexadecimal, `0x1f`.
    Hex,
    /// An integer in binary, `0b11111`.
    Binary,
    /// An integer as the character of that code, `'A'`.
    Char,
    /// The object's text: Java `String.valueOf(…)`, Python `str(…)`.
    Text,
    /// A collection's elements as an array or list.
    Elements,
    /// A map's entries as an array or list.
    Entries,
}

impl ViewAs {
    /// Every view, in the menu's order.
    pub const ALL: [ViewAs; 8] = [
        ViewAs::Auto,
        ViewAs::Decimal,
        ViewAs::Hex,
        ViewAs::Binary,
        ViewAs::Char,
        ViewAs::Text,
        ViewAs::Elements,
        ViewAs::Entries,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            ViewAs::Auto => "Default",
            ViewAs::Decimal => "Decimal",
            ViewAs::Hex => "Hex",
            ViewAs::Binary => "Binary",
            ViewAs::Char => "Character",
            ViewAs::Text => "String (toString)",
            ViewAs::Elements => "Collection Elements",
            ViewAs::Entries => "Map Entries",
        }
    }

    /// Whether the view is computed from the shown number, without asking the adapter.
    pub fn numeric(self) -> bool {
        matches!(
            self,
            ViewAs::Decimal | ViewAs::Hex | ViewAs::Binary | ViewAs::Char
        )
    }

    /// The name used by the MCP tools and the menu's action ids.
    pub fn id(self) -> &'static str {
        match self {
            ViewAs::Auto => "auto",
            ViewAs::Decimal => "decimal",
            ViewAs::Hex => "hex",
            ViewAs::Binary => "binary",
            ViewAs::Char => "char",
            ViewAs::Text => "text",
            ViewAs::Elements => "elements",
            ViewAs::Entries => "entries",
        }
    }

    /// The view called `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == id)
    }
}

/// An integer as the adapter shows it: decimal, `0x…` hexadecimal or a Java `'c' 99` character.
fn integer(value: &str) -> Option<i128> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        return i128::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = v.strip_prefix("-0x") {
        return i128::from_str_radix(hex, 16).ok().map(|n| -n);
    }
    v.parse().ok()
}

/// `value` in a numeric `view` (`SPEC-0091` v12 behaviour 32); `None` when the value is no integer
/// or the view is not numeric.
pub fn view_number(value: &str, view: ViewAs) -> Option<String> {
    let n = integer(value)?;
    Some(match view {
        ViewAs::Decimal => n.to_string(),
        ViewAs::Hex if n < 0 => format!("-0x{:X}", -n),
        // Upper-case digits, as the reference shows them (v13).
        ViewAs::Hex => format!("0x{n:X}"),
        ViewAs::Binary if n < 0 => format!("-0b{:b}", -n),
        ViewAs::Binary => format!("0b{n:b}"),
        ViewAs::Char => {
            let c = u32::try_from(n).ok().and_then(char::from_u32)?;
            format!("{c:?} {n}")
        }
        _ => return None,
    })
}

/// The expression that shows `expression` in an evaluated `view` (`SPEC-0091` v12 behaviour 32);
/// `None` for numeric views and languages without one.
pub fn view_expression(lang: Lang, expression: &str, view: ViewAs) -> Option<String> {
    let e = expression;
    match (lang, view) {
        (Lang::Java, ViewAs::Text) => Some(format!("String.valueOf({e})")),
        (Lang::Java, ViewAs::Elements) => Some(format!("({e}).toArray()")),
        (Lang::Java, ViewAs::Entries) => Some(format!("({e}).entrySet().toArray()")),
        (Lang::Python, ViewAs::Text) => Some(format!("str({e})")),
        (Lang::Python, ViewAs::Elements) => Some(format!("list({e})")),
        (Lang::Python, ViewAs::Entries) => Some(format!("list(({e}).items())")),
        _ => None,
    }
}

/// A renderer: how values of one type are shown (`SPEC-0091` v12 behaviour 33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renderer {
    /// The type it applies to: the adapter's type text or its simple name (`Item` matches
    /// `Main$Item` and `dbg.Main.Item`).
    #[serde(rename = "type")]
    pub type_name: String,
    /// What is shown: an expression in which `this` (Java) or `self` (Python) is the value.
    pub expression: String,
    /// Switched off without being removed.
    #[serde(default = "on", skip_serializing_if = "Clone::clone")]
    pub enabled: bool,
}

fn on() -> bool {
    true
}

/// Whether type text `kind` is `type_name`, fully or by its simple name.
pub fn type_matches(type_name: &str, kind: &str) -> bool {
    let simple = |t: &str| t.rsplit(['.', '$']).next().unwrap_or(t).to_owned();
    let (want, kind) = (type_name.trim(), kind.trim());
    !want.is_empty()
        && (want == kind || (!want.contains(['.', '$']) && simple(kind) == want) || {
            // `pkg.Outer.Inner` written for `pkg.Outer$Inner`.
            kind.replace('$', ".") == want.replace('$', ".")
        })
}

/// The first switched-on renderer for type `kind`.
pub fn renderer_for<'a>(renderers: &'a [Renderer], kind: &str) -> Option<&'a Renderer> {
    renderers
        .iter()
        .find(|r| r.enabled && type_matches(&r.type_name, kind))
}

/// `renderer`'s expression with the value's `object` expression in place of `this` (Java) or
/// `self` (Python) as a whole word.
pub fn renderer_expression(lang: Lang, renderer: &str, object: &str) -> String {
    let word = if lang == Lang::Python { "self" } else { "this" };
    let mut out = String::new();
    let mut rest = renderer;
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    while let Some(at) = rest.find(word) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + word.len()..].chars().next();
        out.push_str(&rest[..at]);
        if before.is_some_and(is_word) || before == Some('.') || after.is_some_and(is_word) {
            out.push_str(word);
        } else {
            out.push_str(&format!("({object})"));
        }
        rest = &rest[at + word.len()..];
    }
    out.push_str(rest);
    out
}

/// Which rows the Variables list hides (`SPEC-0091` v12 behaviour 34).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarFilters {
    /// Show static fields (Java, through java-debug's setting) and Python's class variables.
    pub statics: bool,
    /// Show synthetic fields (Java names with `$` such as `this$0`, `val$x`) and Python's special
    /// and function variables.
    pub synthetic: bool,
    /// Show rows whose value is `null` or `None`.
    pub nulls: bool,
}

impl Default for VarFilters {
    fn default() -> Self {
        Self {
            statics: true,
            synthetic: true,
            nulls: true,
        }
    }
}

/// Whether a row named `name` with `value` is hidden by `filters`.
pub fn hidden_row(filters: &VarFilters, name: &str, value: &str) -> bool {
    if !filters.nulls && (value == "null" || value == "None") {
        return true;
    }
    if !filters.synthetic
        && (name.contains('$')
            || name == "special variables"
            || name == "function variables"
            || (name.starts_with("__") && name.ends_with("__")))
    {
        return true;
    }
    if !filters.statics && (name == "class variables" || name == "static") {
        return true;
    }
    false
}

/// A Python code fragment split for debugpy (`SPEC-0091` v12 behaviour 36): the statements to run
/// first and the last line to evaluate for the shown value. A last line that is indented (inside a
/// block) or empty is run with the rest, and there is no value.
pub fn python_fragment(code: &str) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = code.trim_end().lines().collect();
    let Some((last, before)) = lines.split_last() else {
        return (None, None);
    };
    if last.starts_with([' ', '\t']) || last.trim().is_empty() {
        return (Some(lines.join("\n")), None);
    }
    let before = before.join("\n");
    (
        (!before.trim().is_empty()).then_some(before),
        Some((*last).to_owned()),
    )
}

/// The file and 1-based line a console line names as a stack frame (`SPEC-0091` v12 behaviour
/// 38): Java `at pkg.Main.run(Main.java:12)` and Python `File "/x/app.py", line 12, in run`.
pub fn stack_location(line: &str) -> Option<(String, u64)> {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("File \"") {
        let (file, rest) = rest.split_once('"')?;
        let n = rest.trim_start_matches(',').trim().strip_prefix("line ")?;
        let n: String = n.chars().take_while(char::is_ascii_digit).collect();
        return Some((file.to_owned(), n.parse().ok()?));
    }
    let inner = t.strip_prefix("at ")?;
    let open = inner.rfind('(')?;
    let close = inner[open..].find(')')? + open;
    let (file, n) = inner[open + 1..close].rsplit_once(':')?;
    if file.is_empty() || !file.contains('.') {
        return None;
    }
    Some((file.to_owned(), n.trim().parse().ok()?))
}

/// Whether `name` matches `pattern`, where `*` stands for any text.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(part) else {
                return false;
            };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Whether a class or caller filter allows `name` (`SPEC-0127` v3 behaviour 11): patterns
/// separated by spaces or commas, `*` as a wildcard, a leading `-` excludes. With only exclusions
/// every other name is allowed; an empty filter allows everything.
pub fn filter_allows(filter: &str, name: &str) -> bool {
    let patterns: Vec<&str> = filter
        .split([' ', ','])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.is_empty() {
        return true;
    }
    let name = name.replace('$', ".");
    let hit = |p: &str| {
        let p = p.replace('$', ".");
        wildcard(&p, &name) || name.rsplit('.').next() == Some(p.as_str())
    };
    if patterns
        .iter()
        .filter_map(|p| p.strip_prefix('-'))
        .any(&hit)
    {
        return false;
    }
    let wanted: Vec<&&str> = patterns.iter().filter(|p| !p.starts_with('-')).collect();
    wanted.is_empty() || wanted.iter().any(|p| hit(p))
}

/// Whether an instance filter (object ids separated by spaces or commas) allows object id `id`
/// (the digits after `@` or a `0x` address); an empty filter allows every object.
pub fn instance_allows(filter: &str, id: Option<&str>) -> bool {
    let ids: Vec<String> = filter
        .split([' ', ','])
        .map(|p| p.trim().trim_start_matches('@').to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    if ids.is_empty() {
        return true;
    }
    let Some(id) = id else {
        return false;
    };
    let id = id.rsplit('@').next().unwrap_or(id).to_lowercase();
    ids.contains(&id)
}

/// The id of the object a value shows (Mark Object, instance filters; `SPEC-0091` v11 behaviour
/// 24): Java's `Class@id` (java-debug, the JDWP object id) or a hexadecimal address `0x…`
/// (Python's and the native debuggers' default texts); `None` for values without one (numbers,
/// strings).
pub fn object_id_of(value: &str) -> Option<String> {
    let value = value.split(" \"").next().unwrap_or(value);
    if value.starts_with('"') || value.starts_with('\'') {
        return None;
    }
    if let Some(at) = value.rfind('@') {
        let id: String = value[at + 1..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        let class = &value[..at];
        if !id.is_empty() && !class.is_empty() && !class.contains(' ') {
            return Some(format!("{class}@{id}"));
        }
    }
    let at = value.find("0x")?;
    let id: String = value[at + 2..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    (!id.is_empty()).then(|| format!("0x{id}"))
}

/// The class part of a frame name: `Main$Item.total()` → `Main$Item`, `dbg.Main.main(String[])`
/// → `dbg.Main`; empty for a plain function name.
pub fn frame_class(frame: &str) -> &str {
    let head = frame.split('(').next().unwrap_or(frame).trim();
    head.rsplit_once('.').map_or("", |(class, _)| class)
}

/// The method part of a frame name with its class: `Main$Item.total()` → `Main$Item.total`.
pub fn frame_method(frame: &str) -> &str {
    frame.split('(').next().unwrap_or(frame).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0091 v12 behaviour 25: the block around a line, by braces and by indentation.
    #[test]
    fn blocks_by_braces_and_indentation() {
        let java = [
            "static void main() {",              // 0
            "    int sum = 0;",                  // 1
            "    for (int i = 0; i < 3; i++) {", // 2
            "        sum += i; // {",            // 3
            "        String s = \"}\";",         // 4
            "    }",                             // 5
            "    if (sum > 1)",                  // 6
            "    {",                             // 7
            "        sum--;",                    // 8
            "    }",                             // 9
            "}",                                 // 10
        ];
        assert_eq!(block_lines(&java, 3, false), Some((2, 5)));
        assert_eq!(block_lines(&java, 4, false), Some((2, 5)));
        assert_eq!(block_lines(&java, 1, false), Some((0, 10)));
        assert_eq!(
            block_lines(&java, 8, false),
            Some((6, 9)),
            "header of a lone brace"
        );
        assert_eq!(block_lines(&java, 0, false), None);
        let py = [
            "def main():",     // 0
            "    n = 0",       // 1
            "    for i in r:", // 2
            "        n += i",  // 3
            "",                // 4
            "        n -= 1",  // 5
            "    print(n)",    // 6
            "main()",          // 7
        ];
        assert_eq!(block_lines(&py, 3, true), Some((2, 5)));
        assert_eq!(block_lines(&py, 6, true), Some((0, 6)));
        assert_eq!(block_lines(&py, 7, true), None);
    }

    /// SPEC-0091 v12 behaviour 28: thread names and states.
    #[test]
    fn thread_names_and_states() {
        assert_eq!(thread_name("Thread [main]"), "main");
        assert_eq!(thread_name("MainThread"), "MainThread");
        let parsed = parse_thread_states(
            "\"Signal Dispatcher|RUNNABLE|system;worker-1|TIMED_WAITING|main;lock|BLOCKED|main;a|b|NEW|main;nap|SLEEPING|main;\"",
        );
        assert_eq!(
            parsed,
            [
                (
                    "Signal Dispatcher".into(),
                    "RUNNING".into(),
                    "system".into()
                ),
                ("worker-1".into(), "WAIT".into(), "main".into()),
                ("lock".into(), "MONITOR".into(), "main".into()),
                ("a|b".into(), "NEW".into(), "main".into()),
                ("nap".into(), "SLEEPING".into(), "main".into()),
            ]
        );
    }

    /// SPEC-0091 v12 behaviour 32: numeric views and evaluated views.
    #[test]
    fn views_of_values() {
        assert_eq!(view_number("255", ViewAs::Hex).as_deref(), Some("0xFF"));
        assert_eq!(
            view_number("0x102", ViewAs::Decimal).as_deref(),
            Some("258")
        );
        assert_eq!(view_number("5", ViewAs::Binary).as_deref(), Some("0b101"));
        assert_eq!(view_number("-5", ViewAs::Hex).as_deref(), Some("-0x5"));
        assert_eq!(view_number("65", ViewAs::Char).as_deref(), Some("'A' 65"));
        assert_eq!(view_number("\"pen\"", ViewAs::Hex), None);
        assert_eq!(view_number("12", ViewAs::Text), None);
        assert_eq!(
            view_expression(Lang::Java, "items.get(0)", ViewAs::Text).as_deref(),
            Some("String.valueOf(items.get(0))")
        );
        assert_eq!(
            view_expression(Lang::Python, "stock", ViewAs::Entries).as_deref(),
            Some("list((stock).items())")
        );
        assert_eq!(view_expression(Lang::Other, "x", ViewAs::Text), None);
        assert_eq!(ViewAs::from_id("hex"), Some(ViewAs::Hex));
    }

    /// SPEC-0091 v12 behaviour 33: renderers by type, `this`/`self` replaced as whole words.
    #[test]
    fn renderers_by_type() {
        let r = vec![
            Renderer {
                type_name: "Item".into(),
                expression: "this.name() + \" x\" + this.qty()".into(),
                enabled: true,
            },
            Renderer {
                type_name: "Point".into(),
                expression: "self.x".into(),
                enabled: false,
            },
        ];
        assert!(renderer_for(&r, "Main$Item").is_some());
        assert!(renderer_for(&r, "dbg.Main.Item").is_some());
        assert!(renderer_for(&r, "Items").is_none());
        assert!(renderer_for(&r, "Point").is_none(), "switched off");
        assert_eq!(
            renderer_expression(Lang::Java, &r[0].expression, "items.get(0)"),
            "(items.get(0)).name() + \" x\" + (items.get(0)).qty()"
        );
        assert_eq!(
            renderer_expression(Lang::Python, "self.x + this_x", "p"),
            "(p).x + this_x"
        );
        assert!(type_matches("dbg.Main.Item", "dbg.Main$Item"));
    }

    /// SPEC-0091 v12 behaviour 34: filters of the Variables list.
    #[test]
    fn variable_filters() {
        let all = VarFilters {
            statics: true,
            synthetic: true,
            nulls: true,
        };
        assert!(!hidden_row(&all, "this$0", "Main@3"));
        let none = VarFilters {
            statics: false,
            synthetic: false,
            nulls: false,
        };
        assert!(hidden_row(&none, "this$0", "Main@3"));
        assert!(hidden_row(&none, "special variables", ""));
        assert!(hidden_row(&none, "__name__", "'x'"));
        assert!(hidden_row(&none, "class variables", ""));
        assert!(hidden_row(&none, "next", "null"));
        assert!(!hidden_row(&none, "qty", "3"));
        assert!(VarFilters::default().synthetic);
    }

    /// SPEC-0091 v12 behaviour 36: Python fragments run their statements, then show the last line.
    #[test]
    fn python_fragments() {
        assert_eq!(
            python_fragment("s = 0\nfor v in items: s += v\ns"),
            (
                Some("s = 0\nfor v in items: s += v".into()),
                Some("s".into())
            )
        );
        assert_eq!(python_fragment("n + 1"), (None, Some("n + 1".into())));
        assert_eq!(
            python_fragment("for v in x:\n    print(v)\n"),
            (Some("for v in x:\n    print(v)".into()), None)
        );
    }

    /// SPEC-0091 v12 behaviour 38: stack lines of Java and Python.
    #[test]
    fn stack_lines_in_the_console() {
        assert_eq!(
            stack_location("\tat dbg.Main.risky(Main.java:31)"),
            Some(("Main.java".into(), 31))
        );
        assert_eq!(
            stack_location("  File \"/w/app.py\", line 12, in run"),
            Some(("/w/app.py".into(), 12))
        );
        assert_eq!(
            stack_location("at java.base/Thread.run(Native Method)"),
            None
        );
        assert_eq!(stack_location("value 1"), None);
    }

    /// SPEC-0127 v3 behaviour 11: class, caller and instance filters.
    #[test]
    fn breakpoint_filters() {
        assert!(filter_allows("", "Main"));
        assert!(filter_allows("dbg.*", "dbg.Main$Item"));
        assert!(!filter_allows("dbg.* -dbg.Main$Item", "dbg.Main$Item"));
        assert!(filter_allows("-Main.Item", "Main$Worker"));
        assert!(!filter_allows("-Main.Item", "Main$Item"));
        assert!(filter_allows("Item", "dbg.Main$Item"));
        assert!(filter_allows("Main.add", "Main.add"));
        assert!(!filter_allows("Main.add", "Main.square"));
        assert!(instance_allows("", None));
        assert!(instance_allows("33, 35", Some("Main$Item@35")));
        assert!(!instance_allows("33", Some("Main$Item@35")));
        assert!(!instance_allows("33", None));
        assert!(instance_allows("0x1F", Some("0x1f")));
        assert_eq!(frame_class("Main$Item.total()"), "Main$Item");
        assert_eq!(frame_class("main"), "");
        assert_eq!(frame_method("dbg.Main.add(int, int)"), "dbg.Main.add");
        assert!(wildcard("a*c*e", "abcde"));
        assert!(!wildcard("a*c*e", "abcd"));
    }
}
