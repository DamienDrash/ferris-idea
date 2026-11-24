//! What `tools/list` costs a client (`SPEC-0006` v60 behaviour 26, ADR-0023 rule 3).
//!
//! The list is built once per server from the routers: each schema loses what tells an agent
//! nothing (the `$schema` line MCP already implies, the number formats of the Rust types next to
//! their `minimum`), and a description longer than [`MAX_LISTED_DESCRIPTION`] characters is cut at
//! a sentence and points to `find_tools` with `describe`, which answers the full text. The full
//! descriptions stay what `find_tools` searches. Budgets in this file's tests keep the default
//! offer, every tool and the whole list in bounds.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde_json::{Map, Value};

/// The most characters of a tool's own description `tools/list` shows; a longer one is cut at the
/// last sentence that fits.
pub const MAX_LISTED_DESCRIPTION: usize = 1200;

/// What a cut description ends with.
pub const MORE: &str = "(More: find_tools with `describe`.)";

/// Number formats of the Rust types (schemars) that add nothing to the schema for an agent: the
/// type is `integer` or `number`, and an unsigned one has `minimum: 0`.
const RUST_NUMBER_FORMATS: &[&str] = &[
    "uint", "uint8", "uint16", "uint32", "uint64", "uint128", "int", "int8", "int16", "int32",
    "int64", "int128", "float", "double",
];

/// `schema` without the `$schema` line and without the Rust number formats, at every depth.
pub fn compact_schema(schema: &mut Map<String, Value>) {
    schema.remove("$schema");
    strip_formats(schema);
}

fn strip_formats(map: &mut Map<String, Value>) {
    if map
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|f| RUST_NUMBER_FORMATS.contains(&f))
    {
        map.remove("format");
    }
    for value in map.values_mut() {
        strip_value(value);
    }
}

fn strip_value(value: &mut Value) {
    match value {
        Value::Object(map) => strip_formats(map),
        Value::Array(items) => items.iter_mut().for_each(strip_value),
        _ => {}
    }
}

/// The description `tools/list` shows: the tool's own text, cut at the last sentence within
/// [`MAX_LISTED_DESCRIPTION`] characters with [`MORE`] when it is longer and that makes it
/// shorter, then the untrusted note
/// of behaviour 25.
pub fn listed_description(tool: &str, own: &str) -> String {
    let own = own.trim_end();
    if own.chars().count() <= MAX_LISTED_DESCRIPTION {
        return crate::untrusted::describe(tool, own);
    }
    let head: String = own.chars().take(MAX_LISTED_DESCRIPTION).collect();
    let cut = head
        .rfind(". ")
        .map(|i| &head[..=i])
        .or_else(|| head.rfind(' ').map(|i| &head[..i]))
        .unwrap_or(&head);
    let shown = format!("{} {MORE}", cut.trim_end());
    // A cut that would not make the entry shorter is no cut.
    if shown.len() >= own.len() {
        return crate::untrusted::describe(tool, own);
    }
    crate::untrusted::describe(tool, &shown)
}

/// Every tool's full description with the untrusted note, by name.
static FULL: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    crate::IdeServer::all_tools()
        .list_all()
        .into_iter()
        .map(|t| {
            let own = t.description.as_deref().unwrap_or_default();
            let full = crate::untrusted::describe(&t.name, own.trim_end());
            (t.name.to_string(), full)
        })
        .collect()
});

/// The full description of `tool` (behaviour 26): what `find_tools` searches and `describe`
/// answers.
pub fn full_description(tool: &str) -> Option<&'static str> {
    FULL.get(tool).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of a `tools/list` result holding these tools.
    fn bytes(tools: &[rmcp::model::Tool]) -> usize {
        serde_json::to_string(&serde_json::json!({ "tools": tools }))
            .unwrap()
            .len()
    }

    fn listed() -> Vec<rmcp::model::Tool> {
        crate::annotated(crate::IdeServer::all_tools()).list_all()
    }

    /// The default offer (the core set) in `tools/list` (SPEC-0006 v60 AK-25).
    pub(crate) const DEFAULT_BUDGET: usize = 16 * 1024;
    /// One tool in `tools/list` (`set_ide_settings`, the largest, is about 8 KiB).
    pub(crate) const TOOL_BUDGET: usize = 10 * 1024;
    /// Every tool (`--tools all`); a ratchet that a new tool's text must fit under.
    pub(crate) const ALL_BUDGET: usize = 512 * 1024;

    /// SPEC-0006 v60 AK-25: the list a client gets stays in its budgets.
    #[test]
    fn tools_list_stays_in_its_budgets() {
        let tools = listed();
        let core: Vec<rmcp::model::Tool> = tools
            .iter()
            .filter(|t| crate::groups::CORE.contains(&t.name.as_ref()))
            .cloned()
            .collect();
        assert_eq!(core.len(), crate::groups::CORE.len());
        let (core_bytes, all_bytes) = (bytes(&core), bytes(&tools));
        eprintln!(
            "tools/list: default {core_bytes} bytes, all {all_bytes} bytes, {} tools",
            tools.len()
        );
        assert!(
            core_bytes <= DEFAULT_BUDGET,
            "the default tools/list is {core_bytes} bytes, budget {DEFAULT_BUDGET}"
        );
        assert!(
            all_bytes <= ALL_BUDGET,
            "tools/list with every tool is {all_bytes} bytes, budget {ALL_BUDGET}: shorten descriptions"
        );
        let over: Vec<(String, usize)> = tools
            .iter()
            .map(|t| (t.name.to_string(), bytes(std::slice::from_ref(t))))
            .filter(|(_, n)| *n > TOOL_BUDGET)
            .collect();
        assert!(
            over.is_empty(),
            "tools over {TOOL_BUDGET} bytes in tools/list (shorten the schema's field docs): {over:?}"
        );
    }

    /// SPEC-0006 v60 AK-25: no `$schema`, no Rust number formats; long descriptions are cut with a
    /// pointer, and the full text stays reachable.
    #[test]
    fn listed_tools_are_compact_and_lose_nothing() {
        for t in listed() {
            let schema = serde_json::to_string(&t.input_schema).unwrap();
            assert!(!schema.contains("\"$schema\""), "{}: {schema}", t.name);
            for f in RUST_NUMBER_FORMATS {
                assert!(
                    !schema.contains(&format!("\"format\":\"{f}\"")),
                    "{}: {schema}",
                    t.name
                );
            }
            let shown = t.description.as_deref().unwrap_or_default();
            let full = full_description(&t.name).expect("full description");
            if shown != full {
                assert!(shown.contains(MORE), "{}: {shown}", t.name);
                let head = shown.split(MORE).next().unwrap().trim_end();
                assert!(full.starts_with(head), "{}", t.name);
                assert!(full.len() > shown.len(), "{}", t.name);
            }
            // The untrusted note is never cut away.
            if let Some(note) =
                crate::untrusted::sources_of(&t.name).and_then(crate::untrusted::description_note)
            {
                assert!(
                    shown.ends_with(&note) && full.ends_with(&note),
                    "{}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn a_long_description_is_cut_at_a_sentence() {
        let long = format!("First part. {}", "word ".repeat(400));
        let shown = listed_description("themes", &long);
        assert_eq!(shown, format!("First part. {MORE}"));
        assert_eq!(listed_description("themes", "Short. "), "Short.");
        let mut schema = serde_json::json!({
            "$schema": "x", "type": "object",
            "properties": {"n": {"type": ["integer", "null"], "format": "uint", "minimum": 0},
                           "d": {"type": "string", "format": "date-time"},
                           "a": {"anyOf": [{"type": "integer", "format": "int64"}]}}
        });
        compact_schema(schema.as_object_mut().unwrap());
        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {"n": {"type": ["integer", "null"], "minimum": 0},
                               "d": {"type": "string", "format": "date-time"},
                               "a": {"anyOf": [{"type": "integer"}]}}
            })
        );
    }
}
