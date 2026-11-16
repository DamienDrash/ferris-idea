//! Delegate Methods and the import hint for Java (`SPEC-0122` v4, OBS-0027 L17, L22), through jdtls.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::insight::Opened;
use crate::lsp::LspError;

/// A method a field can forward to.
#[derive(Debug, Clone, Serialize)]
pub struct DelegateMethod {
    /// Name.
    pub name: String,
    /// Parameter types as the server names them.
    pub parameters: Vec<String>,
    /// The server's description, handed back unchanged.
    #[serde(skip)]
    pub raw: Value,
}

impl DelegateMethod {
    /// `name(Type, …)`.
    pub fn label(&self) -> String {
        format!("{}({})", self.name, self.parameters.join(", "))
    }
}

/// A field whose type's methods can be delegated to.
#[derive(Debug, Clone, Serialize)]
pub struct DelegateField {
    /// Field name.
    pub name: String,
    /// Field type.
    pub ty: String,
    /// The methods not in the class yet.
    pub methods: Vec<DelegateMethod>,
    /// The server's description, handed back unchanged.
    #[serde(skip)]
    pub raw: Value,
}

/// Reads jdtls's answer to `java/checkDelegateMethodsStatus`.
pub fn parse_delegate_fields(v: &Value) -> Vec<DelegateField> {
    v["delegateFields"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| DelegateField {
            name: f["field"]["name"].as_str().unwrap_or_default().to_owned(),
            ty: f["field"]["type"].as_str().unwrap_or_default().to_owned(),
            methods: f["delegateMethods"]
                .as_array()
                .into_iter()
                .flatten()
                // Forwarding what every object has would change its identity (behaviour 10).
                .filter(|m| {
                    !m["bindingKey"]
                        .as_str()
                        .is_some_and(|k| k.starts_with("Ljava/lang/Object;"))
                })
                .map(|m| DelegateMethod {
                    name: m["name"].as_str().unwrap_or_default().to_owned(),
                    parameters: m["parameters"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| p.as_str().map(str::to_owned))
                        .collect(),
                    raw: m.clone(),
                })
                .collect(),
            raw: f["field"].clone(),
        })
        .collect()
}

fn params(o: &Opened, line: u64, character: u64) -> Value {
    let p = o.position(line, character);
    json!({
        "textDocument": p["textDocument"],
        "range": {"start": p["position"], "end": p["position"]},
        "context": {"diagnostics": []},
    })
}

/// The fields of the class at a place and the methods each can forward to.
pub async fn delegate_fields_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    wait: Duration,
) -> Result<Vec<DelegateField>, LspError> {
    let o = Opened::open_text(root, file, text, wait).await?;
    let result = o
        .ask(
            "java/checkDelegateMethodsStatus",
            params(&o, line, character),
        )
        .await
        .map(|v| parse_delegate_fields(&v));
    o.close().await;
    result
}

/// The edits that add forwarding methods for `chosen` (field, method).
pub async fn add_delegate_methods_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    chosen: &[(DelegateField, DelegateMethod)],
    wait: Duration,
) -> Result<Vec<crate::rename::FileEdits>, LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let entries: Vec<Value> = chosen
            .iter()
            .map(|(f, m)| json!({"field": f.raw, "delegateMethod": m.raw}))
            .collect();
        let edit = o
            .ask(
                "java/generateDelegateMethods",
                json!({"context": params(&o, line, character), "delegateEntries": entries}),
            )
            .await?;
        if edit.is_null() {
            return Err(LspError::Protocol("the server added nothing".into()));
        }
        crate::rename::file_edits(&mut o, &[edit])
    }
    .await;
    o.close().await;
    result
}

/// The simple name an unresolved-name message of jdtls is about, or `None` for other messages
/// (`SPEC-0122` v4 behaviour 11). A source prefix in front of the message (`Java: `) is ignored.
pub fn unresolved_name(message: &str) -> Option<&str> {
    let message = message.rsplit(": ").next().unwrap_or(message).trim();
    let name = message
        .strip_suffix(" cannot be resolved to a type")
        .or_else(|| message.strip_suffix(" cannot be resolved"))?;
    let simple = name.chars().next().is_some_and(char::is_uppercase)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$');
    simple.then_some(name)
}

/// The classes the code actions offer to import for `name`, fully qualified, in their order.
pub fn import_candidates<'a>(name: &str, titles: impl Iterator<Item = &'a str>) -> Vec<String> {
    let prefix = format!("Import '{name}' (");
    titles
        .filter_map(|t| t.strip_prefix(&prefix)?.strip_suffix(')'))
        .map(|package| format!("{package}.{name}"))
        .collect()
}

/// The title of the server's action that imports the fully qualified `class` for the unresolved
/// `name` (behaviour 11a); `None` when `class` is not a class of that name in a package.
pub fn import_action_title(name: &str, class: &str) -> Option<String> {
    let package = class.strip_suffix(name)?.strip_suffix('.')?;
    (!package.is_empty()).then(|| format!("Import '{name}' ({package})"))
}

/// The text of the hint (behaviour 11): the one class, or how many there are.
pub fn import_hint_text(name: &str, candidates: &[String], shortcut: &str) -> Option<String> {
    match candidates {
        [] => None,
        [one] => Some(format!("Import {one}? {shortcut}")),
        many => Some(format!("Import {name}? {} classes, {shortcut}", many.len())),
    }
}

/// The classes that would import the unresolved `name` at a place (MCP `import_hint` and the
/// window's hint).
pub async fn import_candidates_at(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    line: u64,
    character: u64,
    name: &str,
    wait: Duration,
) -> Result<Vec<String>, LspError> {
    let range = crate::insight::Range {
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character,
    };
    let actions = crate::actions::code_actions_in(root, file, text, &range, wait).await?;
    Ok(import_candidates(
        name,
        actions.iter().map(|a| a.title.as_str()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_names_are_read() {
        assert_eq!(
            unresolved_name("LinkedList cannot be resolved to a type"),
            Some("LinkedList")
        );
        assert_eq!(
            unresolved_name("Java: Arrays cannot be resolved"),
            Some("Arrays")
        );
        assert_eq!(unresolved_name("foo cannot be resolved"), None);
        assert_eq!(unresolved_name("Type mismatch"), None);
    }

    #[test]
    fn candidates_come_from_import_actions() {
        let t = [
            "Import 'List' (java.util)",
            "Import 'List' (java.awt)",
            "Create class 'List'",
            "Import 'Set' (java.util)",
        ];
        let c = import_candidates("List", t.iter().copied());
        assert_eq!(c, ["java.util.List", "java.awt.List"]);
        assert_eq!(
            import_hint_text("List", &c[..1], "\u{2325}\u{21a9}").as_deref(),
            Some("Import java.util.List? \u{2325}\u{21a9}")
        );
        assert_eq!(
            import_hint_text("List", &c, "\u{2325}\u{21a9}").as_deref(),
            Some("Import List? 2 classes, \u{2325}\u{21a9}")
        );
        assert_eq!(import_hint_text("List", &[], "x"), None);
        // The way back from a class to the action that imports it (v5 behaviour 11a).
        let titles: Vec<_> = c
            .iter()
            .filter_map(|class| import_action_title("List", class))
            .collect();
        assert_eq!(titles, t[..2]);
        assert_eq!(import_action_title("List", "List"), None);
        assert_eq!(import_action_title("List", "java.util.ArrayList"), None);
    }

    #[test]
    fn delegate_answer_is_read_without_object_methods() {
        let v = json!({"delegateFields": [{
            "field": {"bindingKey": "k", "name": "engine", "type": "Engine"},
            "delegateMethods": [
                {"bindingKey": "Lshop/Engine;.start(I)V", "name": "start", "parameters": ["int"]},
                {"bindingKey": "Ljava/lang/Object;.toString()Ljava/lang/String;", "name": "toString", "parameters": []},
            ],
        }]});
        let f = parse_delegate_fields(&v);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].name.as_str(), f[0].ty.as_str()), ("engine", "Engine"));
        let labels: Vec<String> = f[0].methods.iter().map(DelegateMethod::label).collect();
        assert_eq!(labels, ["start(int)"]);
    }
}
