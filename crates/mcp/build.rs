//! The tool registry of the MCP server (ADR-0035): finds every tool router and
//! every per-module classification under `src/` and writes them to `tool_registry.rs`, which
//! `lib.rs` includes as `mod registry`. A new tool module therefore touches only its own file and
//! its one `mod` line; no shared list names it.
//!
//! A module declares, at the start of a line:
//! - `#[tool_router(router = NAME, vis = "pub(crate)")]` on its `impl IdeServer` block (the crate
//!   root's own `#[tool_router]` keeps the default name `tool_router`);
//! - `pub(crate) const TOOL_EFFECTS: &[ToolEffect]`: its rows of the effects table;
//! - `pub(crate) const THROUGH_BUFFER_TOOLS: &[&str]` and `pub(crate) const OWN_FILES_TOOLS:
//!   &[(&str, &str)]`: its entries of the open-files lists, when it has tools that change files;
//! - `pub(crate) const TOOL_GROUPS: &[(&str, crate::groups::Group)]`: the group of each of its
//!   tools (`SPEC-0006` behaviour 17);
//! - `pub(crate) const TOOL_CONTENT: &[(&str, &[crate::untrusted::Source])]`: where the untrusted
//!   text of each of its tools comes from (`SPEC-0006` v60 behaviour 25).
//!
//! The lists are sorted (routers by name, modules by path), so the output does not depend on the
//! order the file system lists the sources in.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The per-module consts collected into one list each: (const name, registry name, element type).
const LISTS: [(&str, &str, &str); 5] = [
    ("TOOL_EFFECTS", "EFFECTS", "&[crate::effects::ToolEffect]"),
    ("THROUGH_BUFFER_TOOLS", "THROUGH_BUFFER", "&[&str]"),
    ("OWN_FILES_TOOLS", "OWN_FILES", "&[(&str, &str)]"),
    ("TOOL_GROUPS", "GROUPS", "&[(&str, crate::groups::Group)]"),
    (
        "TOOL_CONTENT",
        "CONTENT",
        "&[(&str, &[crate::untrusted::Source])]",
    ),
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// The module path of a source file below `src/`: `crate`, `crate::a`, `crate::a::b`.
fn module_of(relative: &Path) -> String {
    let mut parts: Vec<String> = relative
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.last().is_some_and(|p| p == "mod" || p == "lib") {
        parts.pop();
    }
    let mut path = String::from("crate");
    for p in parts {
        path.push_str("::");
        path.push_str(&p);
    }
    path
}

/// The router name of a `#[tool_router…]` attribute: `router = NAME`, else `tool_router`.
fn router_name(attribute: &str) -> String {
    let Some(at) = attribute
        .find("router =")
        .or_else(|| attribute.find("router="))
    else {
        return "tool_router".to_owned();
    };
    let rest = attribute[at + "router".len()..]
        .trim_start()
        .trim_start_matches('=')
        .trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    rest[..end].to_owned()
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = root.join("src");
    println!("cargo:rerun-if-changed=src");

    let mut files = Vec::new();
    sources(&src, &mut files);

    // router name -> (file, module); per list: module paths.
    let mut routers: BTreeMap<String, String> = BTreeMap::new();
    let mut lists: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();
    for file in &files {
        let relative = file.strip_prefix(&src).expect("below src");
        let shown = format!("src/{}", relative.display());
        let module = module_of(relative);
        let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[tool_router") {
                // The attribute up to its closing bracket, also across lines.
                let mut attribute = String::new();
                for part in &lines[i..] {
                    attribute.push_str(part.trim());
                    if part.trim_end().ends_with(']') {
                        break;
                    }
                }
                let name = router_name(&attribute);
                if module != "crate" && !attribute.contains("vis =") {
                    problems.push(format!(
                        "{shown}:{}: give the router `vis = \"pub(crate)\"` so the registry can join it: #[tool_router(router = {name}, vis = \"pub(crate)\")]",
                        i + 1
                    ));
                }
                if let Some(first) = routers.insert(name.clone(), shown.clone()) {
                    problems.push(format!(
                        "{shown}:{}: router `{name}` is also defined in {first}; name each router after its module",
                        i + 1
                    ));
                }
            }
            for (constant, list, _) in LISTS {
                let declared = ["pub(crate) const ", "pub const "]
                    .iter()
                    .any(|p| trimmed.starts_with(&format!("{p}{constant}:")));
                if declared {
                    lists.entry(list).or_default().push(module.clone());
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the tool registry (crates/mcp/build.rs):\n{}",
        problems.join("\n")
    );

    let mut out = String::from(
        "// Generated by crates/mcp/build.rs from the sources under src/; do not edit.\n\n",
    );
    out.push_str(
        "/// A function that builds one tool router: the crate root's or a tool module's.\n",
    );
    out.push_str("pub(crate) type Router = fn() -> rmcp::handler::server::router::tool::ToolRouter<crate::IdeServer>;\n\n");
    out.push_str("/// Every tool router of the server by name, sorted.\n");
    out.push_str("pub(crate) const ROUTERS: &[(&str, Router)] = &[\n");
    for name in routers.keys() {
        out.push_str(&format!("    (\"{name}\", crate::IdeServer::{name}),\n"));
    }
    out.push_str("];\n");
    for (constant, list, element) in LISTS {
        out.push_str(&format!(
            "\n/// Every module's `{constant}`, sorted by module path.\npub(crate) const {list}: &[{element}] = &[\n"
        ));
        for module in lists.get(list).into_iter().flatten() {
            out.push_str(&format!("    {module}::{constant},\n"));
        }
        out.push_str("];\n");
    }

    let target = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("tool_registry.rs");
    if fs::read_to_string(&target).ok().as_deref() != Some(out.as_str()) {
        fs::write(&target, out).unwrap_or_else(|e| panic!("{}: {e}", target.display()));
    }
}
