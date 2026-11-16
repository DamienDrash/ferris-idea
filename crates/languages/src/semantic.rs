//! Semantic tokens from the language server (`SPEC-0290`): the request, the decoding of the relative
//! five-number encoding against the server's legend, and the colour key of each token.

use std::path::Path;
use std::time::Duration;

use idecraft_ui::theme::KeyFamily;
use serde::Serialize;
use serde_json::{Value, json};

use crate::insight::{Encoding, Opened, from_server};
use crate::lsp::LspError;

/// One token, in characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticToken {
    /// 0-based line.
    pub line: u64,
    /// First character.
    pub start: u64,
    /// Character after the token.
    pub end: u64,
    /// Type name from the legend, e.g. `method`.
    #[serde(rename = "type")]
    pub token_type: String,
    /// Modifier names from the legend, e.g. `static`.
    pub modifiers: Vec<String>,
    /// Colour-scheme key, if the token changes the syntax colour (behaviour 3).
    pub key: Option<&'static str>,
}

/// The legend of `capabilities.semanticTokensProvider` when the server offers full tokens.
pub fn legend(capabilities: &Value) -> Option<(Vec<String>, Vec<String>)> {
    let provider = capabilities.get("semanticTokensProvider")?;
    let full = provider.get("full")?;
    if !(full.as_bool() == Some(true) || full.is_object()) {
        return None;
    }
    let names = |k: &str| -> Vec<String> {
        provider["legend"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    Some((names("tokenTypes"), names("tokenModifiers")))
}

/// Decodes `data` (behaviour 2) for `text` with positions in `encoding`; `family` picks the colour
/// keys (Java, JavaScript/TypeScript or neutral).
pub fn decode(
    data: &[u64],
    types: &[String],
    modifiers: &[String],
    text: &str,
    encoding: Encoding,
    family: KeyFamily,
) -> Vec<SemanticToken> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut out = Vec::with_capacity(data.len() / 5);
    let (mut line, mut start) = (0u64, 0u64);
    for group in data.as_chunks::<5>().0 {
        let [dl, ds, len, ty, bits] = [group[0], group[1], group[2], group[3], group[4]];
        if dl > 0 {
            line += dl;
            start = ds;
        } else {
            start += ds;
        }
        let Some(token_type) = types.get(ty as usize) else {
            continue;
        };
        let Some(source) = lines.get(line as usize) else {
            continue;
        };
        let from = from_server(source, start, encoding);
        let to = from_server(source, start + len, encoding);
        if to <= from {
            continue;
        }
        let names: Vec<String> = modifiers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < 64 && bits & (1 << i) != 0)
            .map(|(_, m)| m.clone())
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let key = idecraft_ui::theme::semantic_key_in(token_type, &refs, family);
        out.push(SemanticToken {
            line,
            start: from,
            end: to,
            token_type: token_type.clone(),
            modifiers: names,
            key,
        });
    }
    if family == KeyFamily::Kotlin {
        kotlin_roles(&mut out, text);
    }
    out
}

/// Corrects kotlin-language-server's tokens with the syntax tree (behaviour 3a, v3): the server
/// reports a local `val`/`var` as a property being declared and a `val`/`var` of a primary
/// constructor as a parameter, and sends a token for a whole `$name` in a string template before
/// the token of the name itself.
fn kotlin_roles(tokens: &mut Vec<SemanticToken>, text: &str) {
    use crate::syntax::{KotlinDeclared, SyntaxDocument};
    let starts = crate::inlay::line_starts(text);
    let chars: Vec<char> = text.chars().collect();
    let offset = |t: &SemanticToken| starts.get(t.line as usize).map(|s| s + t.start as usize);
    tokens.retain(|t| offset(t).and_then(|o| chars.get(o)) != Some(&'$'));
    let declares = |t: &SemanticToken, ty: &str| {
        t.token_type == ty && t.modifiers.iter().any(|m| m == "declaration")
    };
    if !tokens
        .iter()
        .any(|t| declares(t, "property") || declares(t, "parameter"))
    {
        return;
    }
    let Ok(doc) = SyntaxDocument::new(crate::LanguageId::Kotlin, false, text) else {
        return;
    };
    for t in tokens.iter_mut() {
        let Some(at) = offset(t) else {
            continue;
        };
        let role = if declares(t, "property") {
            (doc.kotlin_declared(at) == Some(KotlinDeclared::Local)).then_some("variable")
        } else if declares(t, "parameter") {
            (doc.kotlin_declared(at) == Some(KotlinDeclared::ConstructorProperty))
                .then_some("property")
        } else {
            None
        };
        if let Some(role) = role {
            t.token_type = role.to_owned();
            let refs: Vec<&str> = t.modifiers.iter().map(String::as_str).collect();
            t.key = idecraft_ui::theme::semantic_key_in(role, &refs, KeyFamily::Kotlin);
        }
    }
}

/// Whether a failed request for tokens or hints is worth asking again a little later
/// (`SPEC-0290` v3 behaviour 1a): the server did not answer in time, answered with an error or
/// stopped — a server that is still importing the build does all three. A missing server, a server
/// that cannot be started and a caller that gave up are final.
pub fn worth_asking_again(error: &LspError) -> bool {
    matches!(
        error,
        LspError::Timeout(..)
            | LspError::Response { .. }
            | LspError::Unsupported { .. }
            | LspError::Closed(_)
    )
}

/// How long to wait before asking again after the `attempt`-th failure in a row (1-based), and
/// `None` when it was the last try: 3, 10, 30, 60 and 120 seconds.
pub fn retry_delay(attempt: u32) -> Option<Duration> {
    const SECONDS: [u64; 5] = [3, 10, 30, 60, 120];
    let i = usize::try_from(attempt.checked_sub(1)?).ok()?;
    SECONDS.get(i).map(|s| Duration::from_secs(*s))
}

/// Semantic tokens of `file` whose content is `text` (the editor's text) or the disk content.
/// A server without full semantic tokens gives none (behaviour 1).
pub async fn semantic_tokens_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<Vec<SemanticToken>, LspError> {
    let o = Opened::open_text(root, file, text, wait).await?;
    let Some((types, modifiers)) = legend(&o.capabilities) else {
        o.close().await;
        return Ok(Vec::new());
    };
    let uri = o.uri.clone();
    let result = o
        .ask(
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": uri}}),
        )
        .await
        .map(|v| {
            let data: Vec<u64> = v["data"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            let family = KeyFamily::of_path(&file.to_string_lossy());
            decode(&data, &types, &modifiers, &o.text, o.files.encoding, family)
        });
    o.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legend jdtls 1.64 announces.
    fn jdtls_legend() -> Value {
        json!({"semanticTokensProvider": {"legend": {
            "tokenTypes": ["namespace", "class", "interface", "enum", "enumMember", "type",
                "typeParameter", "method", "property", "variable", "parameter", "modifier",
                "keyword", "decorator", "annotationMember", "record", "recordComponent"],
            "tokenModifiers": ["abstract", "static", "readonly", "deprecated", "declaration",
                "documentation", "public", "private", "protected", "native", "generic",
                "typeArgument", "importDeclaration", "constructor"]},
            "range": false, "full": {"delta": false}}})
    }

    const SOURCE: &str = "class A {\n    record Item(String sku, int qty) {}\n    static int add(int left) { return left; }\n    void run() { int n = add(qty); }\n}\n";

    /// SPEC-0290 AK-1: a jdtls-style answer decodes to the declared method, the record component,
    /// the static call and the parameter at their characters.
    #[test]
    fn decodes_a_jdtls_answer() {
        let (types, modifiers) = legend(&jdtls_legend()).expect("full tokens offered");
        // static = 2, declaration = 16, readonly = 4.
        let data = [
            1, 11, 4, 15, 16, // record Item (declaration)
            0, 12, 3, 16, 16, // sku (recordComponent, declaration)
            1, 15, 3, 7, 18, // add (method, static declaration)
            0, 8, 4, 10, 16, // left (parameter, declaration)
            1, 25, 3, 7, 2, // add( (static call)
            0, 4, 3, 99, 0, // unknown type index: skipped
        ];
        let tokens = decode(
            &data,
            &types,
            &modifiers,
            SOURCE,
            Encoding::Utf16,
            KeyFamily::Java,
        );
        let found: Vec<(u64, u64, u64, &str, Option<&str>)> = tokens
            .iter()
            .map(|t| (t.line, t.start, t.end, t.token_type.as_str(), t.key))
            .collect();
        assert_eq!(
            found,
            [
                (1, 11, 15, "record", Some("CLASS_NAME_ATTRIBUTES")),
                (
                    1,
                    23,
                    26,
                    "recordComponent",
                    Some("INSTANCE_FIELD_ATTRIBUTES")
                ),
                (2, 15, 18, "method", Some("METHOD_DECLARATION_ATTRIBUTES")),
                (2, 23, 27, "parameter", Some("PARAMETER_ATTRIBUTES")),
                (3, 25, 28, "method", Some("STATIC_METHOD_ATTRIBUTES")),
            ]
        );
        assert_eq!(tokens[2].modifiers, ["static", "declaration"]);
        assert_eq!(&SOURCE.lines().nth(2).unwrap()[15..18], "add");
        assert_eq!(&SOURCE.lines().nth(3).unwrap()[25..28], "add");
    }

    /// Positions in UTF-16 units after non-ASCII text become characters.
    #[test]
    fn utf16_positions_become_characters() {
        let (types, modifiers) = legend(&jdtls_legend()).unwrap();
        // "𝔘" is two UTF-16 units and one character.
        let text = "/*𝔘*/ int value;";
        let data = [0, 11, 5, 8, 0];
        let t = decode(
            &data,
            &types,
            &modifiers,
            text,
            Encoding::Utf16,
            KeyFamily::Neutral,
        );
        assert_eq!((t[0].start, t[0].end), (10, 15));
        assert_eq!(t[0].key, Some("DEFAULT_INSTANCE_FIELD"));
    }

    /// SPEC-0290 AK-7 (v3): tokens as kotlin-language-server 1.3 sends them (legend and shapes
    /// recorded from a real answer) take the Java keys; the syntax tree turns the local `val` into
    /// a variable and the constructor's `val` into a property, and the `$name` token goes.
    #[test]
    fn kotlin_tokens_take_java_keys_and_the_trees_roles() {
        let caps = json!({"semanticTokensProvider": {"legend": {
            "tokenTypes": ["keyword", "variable", "function", "property", "parameter",
                "enumMember", "class", "interface", "enum", "type", "string", "number", "variable"],
            "tokenModifiers": ["declaration", "definition", "abstract", "readonly"]},
            "range": true, "full": true}});
        let (types, modifiers) = legend(&caps).expect("full tokens offered");
        let source = "abstract class Animal(val name: String) {\n    fun describe(): String = \"$name says\"\n}\nfun total(a: Int): Int {\n    val twice = a + a\n    return twice\n}\n";
        // Line, start, length, type index, modifier bits (declaration 1, abstract 4, readonly 8).
        let absolute: [[u64; 5]; 15] = [
            [0, 15, 6, 6, 5],
            [0, 26, 4, 4, 9],
            [0, 32, 6, 6, 0],
            [1, 8, 8, 2, 1],
            [1, 20, 6, 6, 0],
            [1, 30, 5, 12, 0],
            [1, 31, 4, 3, 8],
            [3, 4, 5, 2, 1],
            [3, 10, 1, 4, 9],
            [3, 13, 3, 6, 0],
            [3, 19, 3, 6, 0],
            [4, 8, 5, 3, 9],
            [4, 16, 1, 1, 8],
            [4, 20, 1, 1, 8],
            [5, 11, 5, 1, 8],
        ];
        let mut data = Vec::new();
        let (mut line, mut start) = (0, 0);
        for [l, s, len, ty, bits] in absolute {
            data.extend([
                l - line,
                if l == line { s - start } else { s },
                len,
                ty,
                bits,
            ]);
            (line, start) = (l, s);
        }
        let tokens = decode(
            &data,
            &types,
            &modifiers,
            source,
            Encoding::Utf16,
            KeyFamily::Kotlin,
        );
        let lines: Vec<&str> = source.lines().collect();
        let found: Vec<(&str, &str, Option<&str>)> = tokens
            .iter()
            .map(|t| {
                (
                    &lines[t.line as usize][t.start as usize..t.end as usize],
                    t.token_type.as_str(),
                    t.key,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("Animal", "class", Some("CLASS_NAME_ATTRIBUTES")),
                ("name", "property", Some("INSTANCE_FIELD_ATTRIBUTES")),
                ("String", "class", Some("CLASS_NAME_ATTRIBUTES")),
                (
                    "describe",
                    "function",
                    Some("METHOD_DECLARATION_ATTRIBUTES")
                ),
                ("String", "class", Some("CLASS_NAME_ATTRIBUTES")),
                ("name", "property", Some("INSTANCE_FIELD_ATTRIBUTES")),
                ("total", "function", Some("METHOD_DECLARATION_ATTRIBUTES")),
                ("a", "parameter", Some("PARAMETER_ATTRIBUTES")),
                ("Int", "class", Some("CLASS_NAME_ATTRIBUTES")),
                ("Int", "class", Some("CLASS_NAME_ATTRIBUTES")),
                ("twice", "variable", Some("LOCAL_VARIABLE_ATTRIBUTES")),
                ("a", "variable", Some("LOCAL_VARIABLE_ATTRIBUTES")),
                ("a", "variable", Some("LOCAL_VARIABLE_ATTRIBUTES")),
                ("twice", "variable", Some("LOCAL_VARIABLE_ATTRIBUTES")),
            ]
        );
        // Other languages keep the server's tokens as they are.
        let neutral = decode(
            &data,
            &types,
            &modifiers,
            source,
            Encoding::Utf16,
            KeyFamily::Neutral,
        );
        assert_eq!(neutral.len(), absolute.len());
        assert_eq!(neutral[11].token_type, "property");
    }

    /// SPEC-0290 AK-8 (v3): a slow or failing server is asked again, a missing one is not; the
    /// pauses grow and end.
    #[test]
    fn failed_requests_are_asked_again_a_few_times() {
        assert!(worth_asking_again(&LspError::Timeout(
            "textDocument/semanticTokens/full".into(),
            30
        )));
        assert!(worth_asking_again(&LspError::Response {
            method: "initialize".into(),
            code: -32603,
            message: "Internal error".into(),
        }));
        assert!(worth_asking_again(&LspError::Closed(String::new())));
        assert!(!worth_asking_again(&LspError::NoServer("kotlin".into())));
        assert!(!worth_asking_again(&LspError::Start {
            command: "kotlin-language-server".into(),
            message: "not found".into(),
        }));
        assert!(!worth_asking_again(&LspError::Stopped("x".into())));
        let pauses: Vec<u64> = (1..=6)
            .filter_map(retry_delay)
            .map(|d| d.as_secs())
            .collect();
        assert_eq!(pauses, [3, 10, 30, 60, 120]);
        assert_eq!(retry_delay(0), None);
        assert_eq!(retry_delay(6), None);
    }

    #[test]
    fn servers_without_full_tokens_have_no_legend() {
        assert!(legend(&json!({})).is_none());
        assert!(
            legend(&json!({"semanticTokensProvider": {"legend": {}, "range": true}})).is_none()
        );
        let mut caps = jdtls_legend();
        caps["semanticTokensProvider"]["full"] = json!(true);
        assert!(legend(&caps).is_some());
    }
}
