//! Java refactorings around the server's own (`SPEC-0122` v8): the name Extract Constant and
//! Extract Variable propose (behaviour 16) and the choice Inline Method gives (behaviour 17).
//! The place is read from the syntax tree, the parameter name and the edits come from the
//! language server.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::Instant;
use tree_sitter::Node;

use crate::actions::{CodeAction, Refactoring, has_word, introduced_name};
use crate::insight::{Opened, Range, parse_locations, parse_signature_help};
use crate::jvm::{Jvm, parse};
use crate::lsp::LspError;
use crate::rename::{FileEdits, apply_edits};

/// How long the questions behind the Inline Method variants may take in all (`SPEC-0122` v9
/// behaviour 17): after that the server's own action stays.
pub(crate) const VARIANTS_WAIT: Duration = Duration::from_secs(15);
/// How long the server may take to name the parameter an extracted value is passed to
/// (`SPEC-0122` v9 behaviour 16): after that the server's name stays.
pub(crate) const NAME_WAIT: Duration = Duration::from_secs(5);

/// The title of the server's own action (jdtls).
pub const INLINE_METHOD: &str = "Inline Method";
/// Inline every usage and remove the method.
pub const INLINE_ALL_REMOVE: &str = "Inline Method: all usages, remove the method";
/// Inline every usage, the method stays.
pub const INLINE_ALL_KEEP: &str = "Inline Method: all usages, keep the method";
/// Inline the call at the caret only, the method stays.
pub const INLINE_THIS: &str = "Inline Method: this usage only, keep the method";

fn is_java(file: &Path) -> bool {
    file.extension().is_some_and(|e| e == "java")
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || a.canonicalize()
            .ok()
            .is_some_and(|c| b.canonicalize().ok() == Some(c))
}

/// The byte offset of `line` and `character` (characters) in `text`.
fn offset(text: &str, line: u64, character: u64) -> usize {
    let mut at = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i as u64 == line {
            return at
                + l.char_indices()
                    .nth(character as usize)
                    .map_or(l.len(), |(b, _)| b);
        }
        at += l.len() + 1;
    }
    text.len()
}

// ------------------------------------------------------------------ names (behaviour 16)

/// What the selected expression is used for, as far as it names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Site {
    /// An argument of a call or constructor: the parameter's name, asked from the server.
    Argument,
    /// The whole initial value of the variable or field of this name.
    Declared(String),
    /// The value returned by the method of this name.
    Returned(String),
}

/// The use of the expression selected by `range` in the Java `text`, with the names used in the
/// method around it (the whole file's outside a method). `None` when the selection is no whole
/// expression or its place names nothing.
pub(crate) fn site(text: &str, range: &Range) -> Option<(Site, String)> {
    let tree = parse(Jvm::Java, text)?;
    let (mut a, mut b) = (
        offset(text, range.start_line, range.start_character),
        offset(text, range.end_line, range.end_character),
    );
    let selected = text.get(a..b)?;
    a += selected.len() - selected.trim_start().len();
    b -= selected.len() - selected.trim_end().len();
    if a >= b {
        return None;
    }
    let mut node = tree.root_node().descendant_for_byte_range(a, b)?;
    if node.byte_range() != (a..b) {
        return None;
    }
    // The outermost node of that text, then out of its parentheses.
    while let Some(p) = node.parent() {
        if p.byte_range() == node.byte_range() || p.kind() == "parenthesized_expression" {
            node = p;
        } else {
            break;
        }
    }
    let parent = node.parent()?;
    let name_of = |n: Node| {
        n.child_by_field_name("name")
            .map(|n| text[n.byte_range()].to_owned())
    };
    let mut scope = text;
    let mut method = None;
    let mut up = Some(parent);
    while let Some(n) = up {
        match n.kind() {
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration" => {
                scope = &text[n.byte_range()];
                method = Some(n);
                break;
            }
            "lambda_expression" | "class_body" | "static_initializer" => {
                scope = &text[n.byte_range()];
                break;
            }
            _ => up = n.parent(),
        }
    }
    let found = match parent.kind() {
        "argument_list" => matches!(
            parent.parent()?.kind(),
            "method_invocation" | "object_creation_expression" | "explicit_constructor_invocation"
        )
        .then_some(Site::Argument)?,
        "variable_declarator"
            if parent
                .child_by_field_name("value")
                .is_some_and(|v| v.id() == node.id()) =>
        {
            Site::Declared(name_of(parent)?)
        }
        "return_statement" => Site::Returned(name_of(
            method.filter(|m| m.kind() == "method_declaration")?,
        )?),
        _ => return None,
    };
    Some((found, scope.to_owned()))
}

/// The words of a name: `unitPriceCents`, `unit_price` and `URLParser` split at their humps and
/// underscores.
fn name_words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '$' {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1);
        let hump = c.is_uppercase()
            && previous.is_some_and(|p| {
                p.is_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_uppercase() && next.is_some_and(|n| n.is_lowercase()))
            });
        if hump && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// `unitPriceCents` as a constant's name: `UNIT_PRICE_CENTS`.
pub(crate) fn constant_case(name: &str) -> String {
    name_words(name)
        .iter()
        .map(|w| w.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// The name of the parameter labelled `label` in a signature (`int unitPriceCents`,
/// `String... args`); `None` for the names of a class without parameter names (`arg0`).
pub(crate) fn parameter_name(label: &str) -> Option<String> {
    let name: String = label
        .trim_end_matches(|c: char| c == ']' || c == '[' || c.is_whitespace())
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let generated = name
        .strip_prefix("arg")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let starts = name.chars().next().is_some_and(|c| !c.is_ascii_digit());
    // A label of one word is a type without a name.
    (starts && !generated && name.len() < label.trim().len()).then_some(name)
}

/// A method's name without its `get`: `getLimit` is `limit`; `None` without that prefix.
fn without_get(method: &str) -> Option<String> {
    let rest = method.strip_prefix("get")?;
    let first = rest.chars().next().filter(|c| c.is_uppercase())?;
    Some(first.to_lowercase().chain(rest.chars().skip(1)).collect())
}

/// The better name for what `refactoring` (Extract Constant or Extract Variable) introduced as
/// `server_name`, from the `source` name of the place (a parameter, a variable or field, a
/// method): for a constant in upper case with underscores and numbered until no name of `file`
/// equals it; for a variable only in place of a one-letter or `_` name, from a parameter or a
/// getter, and only when the method (`scope`) has no such name yet. `None` keeps the server's.
pub(crate) fn better_name(
    refactoring: Refactoring,
    server_name: &str,
    site: &Site,
    parameter: Option<&str>,
    file: &str,
    scope: &str,
) -> Option<String> {
    let name = match refactoring {
        Refactoring::Constant => {
            let source = match site {
                Site::Argument => parameter?.to_owned(),
                Site::Declared(name) => name.clone(),
                Site::Returned(method) => without_get(method).unwrap_or_else(|| method.clone()),
            };
            let base = constant_case(&source);
            if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            let mut name = base.clone();
            let mut n = 2;
            while has_word(file, &name) {
                name = format!("{base}_{n}");
                n += 1;
            }
            name
        }
        Refactoring::Variable => {
            let poor = server_name.chars().count() == 1 || server_name.starts_with('_');
            let name = match site {
                Site::Argument => parameter?.to_owned(),
                Site::Returned(method) => without_get(method)?,
                Site::Declared(_) => return None,
            };
            if !poor || has_word(scope, &name) {
                return None;
            }
            name
        }
        _ => return None,
    };
    (name != server_name).then_some(name)
}

/// `text` with the word `old` replaced by `new`.
fn replace_word(text: &str, old: &str, new: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut before: Option<char> = None;
    while let Some(at) = rest.find(old) {
        let end = at + old.len();
        let left = rest[..at].chars().next_back().or(before);
        let right = rest[end..].chars().next();
        out.push_str(&rest[..at]);
        if left.is_some_and(word) || right.is_some_and(word) {
            out.push_str(old);
        } else {
            out.push_str(new);
        }
        before = old.chars().next_back();
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Which extraction the action titled `title` is, if it is one whose name is improved.
fn naming(title: &str) -> Option<Refactoring> {
    let action = CodeAction {
        title: title.to_owned(),
        kind: None,
        preferred: false,
        disabled: None,
        fixes: Vec::new(),
    };
    [Refactoring::Constant, Refactoring::Variable]
        .into_iter()
        .find(|r| r.matches(&action))
}

/// Gives the constant or variable that the server's extraction titled `title` introduced into the
/// Java `file` a name from the place of the selected `range` (`SPEC-0122` v8 behaviour 16): the
/// planned `files` then carry that name instead of the server's. Without a better name, or when
/// the server's name was in the file before, nothing changes; neither when the server does not
/// name the parameter within `wait` (v9).
pub(crate) async fn improve_name(
    o: &mut Opened,
    file: &Path,
    range: &Range,
    title: &str,
    files: &mut [FileEdits],
    wait: Duration,
) {
    if !is_java(file) {
        return;
    }
    let Some(refactoring) = naming(title) else {
        return;
    };
    let before = o.text.clone();
    let Some((site, scope)) = site(&before, range) else {
        return;
    };
    let Some(planned) = files.iter_mut().find(|f| same_file(&f.path, file)) else {
        return;
    };
    let Some(server_name) = apply_edits(&before, &planned.edits)
        .ok()
        .and_then(|after| introduced_name(&before, &after))
        .map(|n| n.name)
        .filter(|n| !has_word(&before, n))
    else {
        return;
    };
    let parameter = if site == Site::Argument {
        let at = o.position(range.start_line, range.start_character);
        o.ask_bounded("textDocument/signatureHelp", at, wait, None)
            .await
            .ok()
            .and_then(|v| parse_signature_help(&v))
            .and_then(|help| {
                let s = help.signatures.get(help.active_signature)?;
                let label: String = s
                    .label
                    .chars()
                    .skip(s.parameters.get(s.active_parameter?)?.start)
                    .take(s.parameters[s.active_parameter?].len())
                    .collect();
                parameter_name(&label)
            })
    } else {
        None
    };
    let Some(name) = better_name(
        refactoring,
        &server_name,
        &site,
        parameter.as_deref(),
        &before,
        &scope,
    ) else {
        return;
    };
    for e in &mut planned.edits {
        e.new_text = replace_word(&e.new_text, &server_name, &name);
    }
}

// ------------------------------------------------------------------ inline (behaviour 17)

/// Whether `title` is one of Ferris IDEA's Inline Method variants for the Java `file`.
pub fn is_inline_choice(file: &Path, title: &str) -> bool {
    is_java(file) && [INLINE_ALL_REMOVE, INLINE_ALL_KEEP, INLINE_THIS].contains(&title)
}

/// Whether the caret at `line` and `character` is on the name of a method's declaration.
pub(crate) fn on_method_name(text: &str, line: u64, character: u64) -> bool {
    let Some(tree) = parse(Jvm::Java, text) else {
        return false;
    };
    let at = offset(text, line, character);
    let on = |at: usize| {
        tree.root_node()
            .descendant_for_byte_range(at, at)
            .filter(|n| n.kind() == "identifier")
            .and_then(|n| Some((n, n.parent()?)))
            .is_some_and(|(n, p)| {
                p.kind() == "method_declaration"
                    && p.child_by_field_name("name")
                        .is_some_and(|name| name.id() == n.id())
            })
    };
    // A caret right behind the name is on it too.
    on(at) || (at > 0 && on(at - 1))
}

fn caret(line: u64, character: u64) -> Range {
    Range {
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character,
    }
}

/// The server's own Inline Method at `range`, if it offers it there; asked until `deadline`.
async fn server_inline(
    o: &mut Opened,
    range: &Range,
    deadline: Instant,
) -> Result<Option<Value>, LspError> {
    let asked = crate::actions::request_actions(o, range, &[], &["refactor"]);
    let list = tokio::time::timeout_at(deadline, asked)
        .await
        .map_err(|_| LspError::Timeout("textDocument/codeAction".into(), 0))??;
    Ok(list
        .into_iter()
        .find(|a| a["title"] == INLINE_METHOD && a["disabled"].is_null()))
}

/// What Inline Method is asked about: where the method is declared and what uses it.
struct Method {
    /// The caret is on the declaration, not on a call.
    on_declaration: bool,
    /// The name in the declaration, in this file.
    declaration: Range,
    /// The usages, when all of them are in this file.
    usages: Option<Vec<Range>>,
}

/// The method at the start of `range` in the Java `file`; `None` when it is declared in another
/// file or the server does not say where until `deadline`. The questions are withdrawn when the
/// time is over, so the server stops looking.
async fn method_at(
    o: &mut Opened,
    file: &Path,
    range: &Range,
    deadline: Instant,
) -> Option<Method> {
    let left = || deadline.saturating_duration_since(Instant::now());
    let (line, character) = (range.start_line, range.start_character);
    let on_declaration = on_method_name(&o.text, line, character);
    let declaration = if on_declaration {
        caret(line, character)
    } else {
        let at = o.position(line, character);
        let answer = o
            .ask_bounded("textDocument/definition", at, left(), None)
            .await
            .ok()?;
        let found = parse_locations(&answer, &mut o.files);
        let first = found.first()?;
        if !same_file(&first.path, file) {
            return None;
        }
        caret(first.range.start_line, first.range.start_character)
    };
    let mut params = o.position(declaration.start_line, declaration.start_character);
    params["context"] = json!({"includeDeclaration": false});
    let asked = o
        .ask_bounded("textDocument/references", params, left(), None)
        .await;
    let usages = match asked {
        Ok(answer) => {
            let found = parse_locations(&answer, &mut o.files);
            found
                .iter()
                .all(|l| same_file(&l.path, file))
                .then(|| found.into_iter().map(|l| l.range).collect())
        }
        // Without the usages in time nothing is known about the method.
        Err(LspError::Timeout(..)) => return None,
        Err(_) => None,
    };
    Some(Method {
        on_declaration,
        declaration,
        usages,
    })
}

/// The Inline Method variants for `range` of the Java `file`, in place of the server's single
/// `Inline Method` among `actions` (`SPEC-0122` v8 behaviour 17): all usages with and without the
/// method, and for a caret on a call that usage alone. The server's action is kept as it is when
/// fewer than two variants can be done (another refactoring, a method declared in another file),
/// and when the server does not answer what the variants need within `wait` (v9).
pub(crate) async fn inline_choices(
    o: &mut Opened,
    file: &Path,
    range: &Range,
    actions: Vec<CodeAction>,
    wait: Duration,
) -> Vec<CodeAction> {
    let Some(at) = actions.iter().position(|a| a.title == INLINE_METHOD) else {
        return actions;
    };
    if !is_java(file) {
        return actions;
    }
    let deadline = Instant::now() + wait;
    let Some(method) = method_at(o, file, range, deadline).await else {
        return actions;
    };
    let mut titles = Vec::new();
    if method.on_declaration {
        titles.push(INLINE_ALL_REMOVE);
    } else {
        match server_inline(o, &method.declaration, deadline).await {
            Ok(Some(_)) => titles.push(INLINE_ALL_REMOVE),
            Err(LspError::Timeout(..)) => return actions,
            _ => {}
        }
    }
    if method.usages.as_ref().is_some_and(|u| !u.is_empty()) {
        titles.push(INLINE_ALL_KEEP);
    }
    if !method.on_declaration {
        titles.push(INLINE_THIS);
    }
    if titles.len() < 2 {
        return actions;
    }
    let mut out = actions;
    let server = out.remove(at);
    for (i, title) in titles.into_iter().enumerate() {
        out.insert(
            at + i,
            CodeAction {
                title: title.to_owned(),
                preferred: i == 0,
                ..server.clone()
            },
        );
    }
    out
}

/// The workspace edits of the Inline Method variant `title` for `range` of the Java `file`: the
/// server's action at the declaration (all usages, the method removed), at the caret (this usage)
/// or at every usage (all usages, the method kept). The server has `wait` to say where the method
/// is declared and used, and `wait` for each place's action.
pub(crate) async fn plan_inline(
    o: &mut Opened,
    file: &Path,
    range: &Range,
    title: &str,
    wait: Duration,
) -> Result<Vec<Value>, LspError> {
    let refused = |why: &str| LspError::Protocol(format!("{title}: {why}"));
    let method = method_at(o, file, range, Instant::now() + wait)
        .await
        .ok_or_else(|| {
            refused(
                "the server did not say in time where the method is declared and used, or it \
                 is declared in another file; use the server's Inline Method",
            )
        })?;
    let places: Vec<Range> = match title {
        INLINE_THIS if method.on_declaration => {
            return Err(refused("put the caret on a call of the method"));
        }
        INLINE_THIS => vec![range.clone()],
        INLINE_ALL_REMOVE => vec![method.declaration.clone()],
        _ => {
            let usages = method
                .usages
                .ok_or_else(|| refused("the method is used in other files"))?;
            if usages.is_empty() {
                return Err(refused("the method is not used"));
            }
            usages
                .iter()
                .map(|u| caret(u.start_line, u.start_character))
                .collect()
        }
    };
    let mut edits = Vec::new();
    for place in &places {
        let asked = server_inline(o, place, Instant::now() + wait).await;
        let action = asked?.ok_or_else(|| {
            refused(&format!(
                "the server cannot inline the method at line {}",
                place.start_line + 1
            ))
        })?;
        edits.extend(crate::actions::action_edits(o, &action, INLINE_METHOD).await?);
    }
    if edits.is_empty() {
        return Err(refused("changes nothing"));
    }
    Ok(edits)
}

/// Refuses `files` whose edits overlap: the usages of [`INLINE_ALL_KEEP`] are inlined one by one,
/// and two that touch the same text (a call inside a call's argument) cannot be put together.
pub(crate) fn check_apart(title: &str, files: &[FileEdits]) -> Result<(), LspError> {
    for f in files {
        for pair in f.edits.windows(2) {
            let end = (pair[0].range.end_line, pair[0].range.end_character);
            let start = (pair[1].range.start_line, pair[1].range.start_character);
            if start < end {
                return Err(LspError::Protocol(format!(
                    "{title}: two usages touch the same text at line {}; inline them one by one",
                    start.0 + 1
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVENTORY: &str = "package lab.inventory;\n\nimport java.util.List;\n\npublic final class Inventory {\n    public record Item(String sku, String name, int qty, int unitPriceCents) {\n        int totalCents() {\n            return qty * unitPriceCents;\n        }\n    }\n\n    public static int totalValue(List<Item> items) {\n        return items.stream().mapToInt(Item::totalCents).sum();\n    }\n\n    int getLimit() {\n        int timeout = 30;\n        return (40);\n    }\n\n    public static void main(String[] args) {\n        System.out.println(totalValue(List.of(new Item(\"A-1\", \"Bolt\", 3, 25))));\n    }\n}\n";

    fn range_of(text: &str, needle: &str) -> Range {
        let at = text.find(needle).unwrap();
        let line = text[..at].matches('\n').count() as u64;
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let character = text[start..at].chars().count() as u64;
        Range {
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character + needle.chars().count() as u64,
        }
    }

    // SPEC-0122/AK-22 (v8, OBS-0027 J1): the place of the selected expression.
    #[test]
    fn the_place_of_an_expression_names_it() {
        let at = |needle: &str, part: &str| {
            let outer = range_of(INVENTORY, needle);
            let inner = needle.find(part).unwrap() as u64;
            let r = Range {
                start_character: outer.start_character + inner,
                end_character: outer.start_character + inner + part.len() as u64,
                ..outer
            };
            site(INVENTORY, &r).map(|(s, _)| s)
        };
        assert_eq!(at("3, 25)", "25"), Some(Site::Argument));
        assert_eq!(at("\"Bolt\", 3", "\"Bolt\""), Some(Site::Argument));
        assert_eq!(
            at("timeout = 30", "30"),
            Some(Site::Declared("timeout".into()))
        );
        assert_eq!(
            at("return (40)", "40"),
            Some(Site::Returned("getLimit".into()))
        );
        assert_eq!(
            at("return qty * unitPriceCents", "qty * unitPriceCents"),
            Some(Site::Returned("totalCents".into()))
        );
        // Part of an expression, a piece that is no expression, a name's half.
        assert_eq!(at("return qty * unitPriceCents", "qty"), None);
        assert_eq!(at("3, 25)", "3, 25"), None);
        assert_eq!(at("timeout = 30", "3"), None);
        // The names of the method around it.
        let (_, scope) = site(INVENTORY, &range_of(INVENTORY, "25")).unwrap();
        assert!(scope.starts_with("public static void main") && !scope.contains("getLimit"));
    }

    // SPEC-0122/AK-22 (v8): the names.
    #[test]
    fn names_follow_the_place() {
        assert_eq!(constant_case("unitPriceCents"), "UNIT_PRICE_CENTS");
        assert_eq!(constant_case("URLParser2x"), "URL_PARSER2X");
        assert_eq!(constant_case("max_size"), "MAX_SIZE");
        assert_eq!(
            parameter_name("int unitPriceCents").as_deref(),
            Some("unitPriceCents")
        );
        assert_eq!(parameter_name("String... args").as_deref(), Some("args"));
        assert_eq!(parameter_name("final int[] xs").as_deref(), Some("xs"));
        assert_eq!(parameter_name("int arg3"), None);
        assert_eq!(parameter_name("int"), None);
        let constant = |server: &str, site: &Site, parameter: Option<&str>, file: &str| {
            better_name(Refactoring::Constant, server, site, parameter, file, file)
        };
        assert_eq!(
            constant("_25", &Site::Argument, Some("unitPriceCents"), INVENTORY).as_deref(),
            Some("UNIT_PRICE_CENTS")
        );
        // Unknown parameter: the server's name stays.
        assert_eq!(constant("_25", &Site::Argument, None, INVENTORY), None);
        assert_eq!(
            constant("_30", &Site::Declared("timeout".into()), None, INVENTORY).as_deref(),
            Some("TIMEOUT")
        );
        assert_eq!(
            constant("_40", &Site::Returned("getLimit".into()), None, INVENTORY).as_deref(),
            Some("LIMIT")
        );
        assert_eq!(
            constant("_40", &Site::Returned("limit".into()), None, INVENTORY).as_deref(),
            Some("LIMIT")
        );
        // Taken names are numbered.
        assert_eq!(
            constant(
                "_30",
                &Site::Declared("timeout".into()),
                None,
                "int TIMEOUT, TIMEOUT_2;"
            )
            .as_deref(),
            Some("TIMEOUT_3")
        );
        let variable = |server: &str, site: &Site, parameter: Option<&str>, scope: &str| {
            better_name(
                Refactoring::Variable,
                server,
                site,
                parameter,
                INVENTORY,
                scope,
            )
        };
        assert_eq!(
            variable("i", &Site::Argument, Some("unitPriceCents"), "void m() {}").as_deref(),
            Some("unitPriceCents")
        );
        assert_eq!(
            variable(
                "i",
                &Site::Returned("getLimit".into()),
                None,
                "int getLimit() {}"
            )
            .as_deref(),
            Some("limit")
        );
        // A name that says something stays, so does a clash, a plain method and an initial value.
        assert_eq!(
            variable("item", &Site::Argument, Some("unitPriceCents"), ""),
            None
        );
        assert_eq!(
            variable("i", &Site::Argument, Some("qty"), "void m(int qty) {}"),
            None
        );
        assert_eq!(
            variable("i", &Site::Returned("totalCents".into()), None, ""),
            None
        );
        assert_eq!(
            variable("i", &Site::Declared("timeout".into()), None, ""),
            None
        );
        assert_eq!(
            replace_word("int _25 = 25; f(_25, x_25, _25);", "_25", "PRICE"),
            "int PRICE = 25; f(PRICE, x_25, PRICE);"
        );
    }

    // SPEC-0122/AK-23 (v8, OBS-0027 J2): declaration or call, the variants' titles, overlaps.
    #[test]
    fn inline_knows_a_declaration_from_a_call() {
        let declared = range_of(INVENTORY, "totalValue(List<Item>");
        let called = range_of(INVENTORY, "totalValue(List.of");
        assert!(on_method_name(
            INVENTORY,
            declared.start_line,
            declared.start_character + 2
        ));
        assert!(on_method_name(
            INVENTORY,
            declared.start_line,
            declared.start_character + 10
        ));
        assert!(!on_method_name(
            INVENTORY,
            declared.start_line,
            declared.start_character + 11
        ));
        assert!(!on_method_name(
            INVENTORY,
            called.start_line,
            called.start_character + 2
        ));
        let java = Path::new("A.java");
        assert!(is_inline_choice(java, INLINE_THIS) && is_inline_choice(java, INLINE_ALL_KEEP));
        assert!(!is_inline_choice(java, INLINE_METHOD));
        assert!(!is_inline_choice(Path::new("a.py"), INLINE_ALL_REMOVE));
        for title in [INLINE_ALL_REMOVE, INLINE_ALL_KEEP, INLINE_THIS] {
            let action = CodeAction {
                title: title.to_owned(),
                kind: Some("refactor.inline".into()),
                preferred: false,
                disabled: None,
                fixes: Vec::new(),
            };
            assert!(Refactoring::Inline.matches(&action));
            assert!(!crate::actions::introduces_name(title));
        }
        let edit = |a: u64, b: u64| crate::rename::Edit {
            range: Range {
                start_line: 3,
                start_character: a,
                end_line: 3,
                end_character: b,
            },
            old_text: String::new(),
            new_text: String::new(),
        };
        let files = |edits| {
            vec![FileEdits {
                path: "A.java".into(),
                edits,
            }]
        };
        assert!(check_apart(INLINE_ALL_KEEP, &files(vec![edit(2, 8), edit(8, 12)])).is_ok());
        let err = check_apart(INLINE_ALL_KEEP, &files(vec![edit(2, 8), edit(6, 12)])).unwrap_err();
        assert!(err.to_string().contains("line 4"), "{err}");
    }

    /// SPEC-0122 v9 AK-24: the variants never wait longer than their time for a server that
    /// does not say what uses the method (a pooled server keeps the long limit it was started
    /// with); the server's own action stays, and the questions are withdrawn.
    #[tokio::test]
    async fn the_variants_give_way_when_the_server_is_silent() {
        use crate::lsp::{LspClient, read_message};
        use tokio::io::{BufReader, DuplexStream, duplex};

        let own = CodeAction {
            title: INLINE_METHOD.into(),
            kind: Some("refactor.inline".into()),
            preferred: false,
            disabled: None,
            fixes: Vec::new(),
        };
        let file = Path::new("/p/src/Inventory.java");
        let declaration = range_of(INVENTORY, "totalValue(List<Item> items)");
        let call = range_of(INVENTORY, "totalValue(List.of");
        // On the declaration the usages are asked for, on a call the declaration first.
        for (place, question) in [
            (&declaration, "textDocument/references"),
            (&call, "textDocument/definition"),
        ] {
            let (client_out, server_in) = duplex(1 << 16);
            let (_server_out, client_in) = duplex(1 << 16);
            let mut client = LspClient::new(client_in, client_out, None::<DuplexStream>);
            client.timeout = Duration::from_secs(600);
            let mut o = Opened::over(client, file, INVENTORY);
            let at = caret(place.start_line, place.start_character);
            let server = async {
                let mut input = BufReader::new(server_in);
                let asked = read_message(&mut input).await.unwrap().unwrap();
                assert_eq!(asked["method"], question);
                let withdrawn = read_message(&mut input).await.unwrap().unwrap();
                assert_eq!(withdrawn["method"], "$/cancelRequest");
                assert_eq!(withdrawn["params"]["id"], asked["id"]);
            };
            let asking = inline_choices(
                &mut o,
                file,
                &at,
                vec![own.clone()],
                Duration::from_millis(200),
            );
            let both = async { tokio::join!(asking, server) };
            let (listed, ()) = tokio::time::timeout(Duration::from_secs(10), both)
                .await
                .expect("the variants wait for the server without a limit");
            assert_eq!(listed, std::slice::from_ref(&own), "{question}");
        }
    }
}
