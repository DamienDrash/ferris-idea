//! `ferris-idea drive [--project <folder>] [--pid <pid>] [--json] <verb> …` (`SPEC-0319` behaviour
//! 30): a client of the running window's MCP server, found through its discovery file — no
//! listener of its own. It connects to the route of one project, so it reaches only that
//! project's window. Exit status: 0 success, 1 a failed assertion or action, 2 a usage error,
//! 3 no window or the driver is unavailable.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

const USAGE: &str = "usage: ferris-idea drive [--project <folder>] [--pid <pid>] [--json] <verb> …
verbs:
  status | stop
  snapshot [<locator>] [--text] [--depth <n>]
  find <locator> [--wait] [--timeout <ms>]
  click <locator> [--count <n>] [--right] [--modifiers cmd,alt]
  context-click <locator> | hover <locator>
  press <keys> [--on <locator>]
  type <locator> <text> [--clear]
  scroll <locator> [--dx <n>] [--dy <n>] [--pixels]
  drag <locator> (--to <locator> | --offset <x>,<y>)
  expect <locator> <matcher> [<text>] [--not] [--contains | --regex] [--timeout <ms>]
  screenshot <file.png> [--of <locator>] [--region <x>,<y>,<w>,<h>]
  open <path> [--line <n>] [--column <n>]
  tool-window <id> [open | focus | close]
  settings <page> | settings close
  invoke <action id>
  type-text <text>
  popup | choose <index> | choose --label <label> | dismiss
  list
  trace start | trace stop [<folder in the workspace>] | trace status
  run <script.toml>
locators: role=button[name=\"Commit\"], id=project-tree >> role=treeitem[name~=\"main\"], text=\"No problems\"";

/// How a run ended.
#[derive(Debug)]
enum Failure {
    /// A usage error (2).
    Usage(String),
    /// A failed assertion or action (1).
    Failed(String),
    /// No window, or the driver is off or not there (3).
    Unavailable(String),
}

impl Failure {
    fn exit(self, json: bool) -> ExitCode {
        let (code, text) = match self {
            Failure::Usage(t) => (2, t),
            Failure::Failed(t) => (1, t),
            Failure::Unavailable(t) => (3, t),
        };
        if json {
            println!("{}", json!({"ok": false, "error": text}));
        } else {
            eprintln!("error: {text}");
        }
        ExitCode::from(code)
    }

    /// The failure of a tool's error text, by its code prefix.
    fn of_tool(text: String) -> Self {
        let code = text.split(':').next().unwrap_or_default();
        match code {
            "unavailable" | "denied" | "not_granted" => Failure::Unavailable(text),
            "invalid" => Failure::Usage(text),
            _ => Failure::Failed(text),
        }
    }
}

/// Options of the command line.
struct Options {
    project: Option<PathBuf>,
    pid: Option<u32>,
    json: bool,
}

/// Runs `ferris-idea drive …`.
pub fn drive_cli(args: &[String]) -> ExitCode {
    let mut options = Options {
        project: None,
        pid: None,
        json: false,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => match it.next() {
                Some(p) => options.project = Some(PathBuf::from(p)),
                None => return Failure::Usage(USAGE.into()).exit(false),
            },
            "--pid" => match it.next().and_then(|p| p.parse().ok()) {
                Some(p) => options.pid = Some(p),
                None => return Failure::Usage(USAGE.into()).exit(false),
            },
            "--json" => options.json = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => rest.push(a.clone()),
        }
    }
    let json_out = options.json;
    match run(&options, &rest) {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => f.exit(json_out),
    }
}

fn run(options: &Options, args: &[String]) -> Result<(), Failure> {
    let Some((verb, rest)) = args.split_first() else {
        return Err(Failure::Usage(USAGE.into()));
    };
    if verb == "run" {
        let [script] = rest else {
            return Err(Failure::Usage(USAGE.into()));
        };
        return run_script(options, Path::new(script));
    }
    let (tool, arguments) = call_of(verb, rest)?;
    let mut client = connect(options)?;
    let (result, images) = client
        .call_with_images(tool, arguments)
        .map_err(Failure::of_tool)?;
    if verb == "screenshot"
        && let Some(file) = rest.iter().find(|a| !a.starts_with("--") && is_png_name(a))
    {
        save_png(Path::new(file), &images)?;
    }
    print(options.json, verb, &result);
    Ok(())
}

fn is_png_name(a: &str) -> bool {
    a.to_ascii_lowercase().ends_with(".png")
}

/// Writes the PNG of a screenshot (`SPEC-0319` v7 behaviour 53), readable by the user only.
fn save_png(file: &Path, images: &[(String, String)]) -> Result<(), Failure> {
    use base64::Engine as _;
    let Some((_, data)) = images.iter().find(|(m, _)| m == "image/png") else {
        return Err(Failure::Failed("the window sent no image".into()));
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| Failure::Failed(format!("the image is not base64: {e}")))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options
        .open(file)
        .map_err(|e| Failure::Usage(format!("{}: {e}", file.display())))?;
    std::io::Write::write_all(&mut out, &bytes)
        .map_err(|e| Failure::Failed(format!("{}: {e}", file.display())))?;
    Ok(())
}

/// The tool and arguments of a verb.
fn call_of(verb: &str, rest: &[String]) -> Result<(&'static str, Value), Failure> {
    let usage = || Failure::Usage(USAGE.into());
    let mut positional: Vec<&str> = Vec::new();
    let mut flags: std::collections::BTreeMap<&str, Option<&str>> =
        std::collections::BTreeMap::new();
    let with_value = [
        "--depth",
        "--timeout",
        "--count",
        "--modifiers",
        "--on",
        "--dx",
        "--dy",
        "--to",
        "--offset",
        "--of",
        "--region",
        "--line",
        "--column",
        "--label",
    ];
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--").map(|_| a.as_str()) {
            if with_value.contains(&name) {
                flags.insert(name, Some(it.next().ok_or_else(usage)?.as_str()));
            } else {
                flags.insert(name, None);
            }
        } else {
            positional.push(a);
        }
    }
    let num = |name: &str| -> Result<Option<Value>, Failure> {
        match flags.get(name).copied().flatten() {
            None => Ok(None),
            Some(v) => v
                .parse::<f64>()
                .map(|n| Some(json!(n)))
                .map_err(|_| Failure::Usage(format!("{name} takes a number"))),
        }
    };
    let int = |name: &str| -> Result<Option<Value>, Failure> {
        match flags.get(name).copied().flatten() {
            None => Ok(None),
            Some(v) => v
                .parse::<u64>()
                .map(|n| Some(json!(n)))
                .map_err(|_| Failure::Usage(format!("{name} takes a whole number"))),
        }
    };
    let mut args = serde_json::Map::new();
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            args.insert(k.into(), v);
        }
    };
    let tool = match (verb, positional.as_slice()) {
        ("status", []) => "ui_driver_status",
        ("stop", []) => "ui_driver_stop",
        ("snapshot", p) if p.len() <= 1 => {
            put("locator", p.first().map(|l| json!(l)));
            put(
                "format",
                flags.contains_key("--text").then(|| json!("text")),
            );
            put("depth", int("--depth")?);
            "ui_snapshot"
        }
        ("find", [l]) => {
            put("locator", Some(json!(l)));
            put("wait", flags.contains_key("--wait").then_some(json!(true)));
            put("timeout_ms", int("--timeout")?);
            "ui_find"
        }
        ("click", [l]) => {
            put("locator", Some(json!(l)));
            put("action", Some(json!("click")));
            put("count", int("--count")?);
            put(
                "button",
                flags.contains_key("--right").then(|| json!("right")),
            );
            put(
                "modifiers",
                flags
                    .get("--modifiers")
                    .copied()
                    .flatten()
                    .map(|m| json!(m.split(',').collect::<Vec<_>>())),
            );
            put("timeout_ms", int("--timeout")?);
            "ui_act"
        }
        ("context-click" | "hover", [l]) => {
            put("locator", Some(json!(l)));
            put(
                "action",
                Some(json!(if verb == "hover" {
                    "hover"
                } else {
                    "context_click"
                })),
            );
            put("timeout_ms", int("--timeout")?);
            "ui_act"
        }
        ("press", [keys]) => {
            put("action", Some(json!("press")));
            put("keys", Some(json!(keys)));
            put(
                "locator",
                flags.get("--on").copied().flatten().map(|l| json!(l)),
            );
            put("timeout_ms", int("--timeout")?);
            "ui_act"
        }
        ("type", [l, text]) => {
            put("locator", Some(json!(l)));
            put("action", Some(json!("type")));
            put("text", Some(json!(text)));
            put(
                "clear",
                flags.contains_key("--clear").then_some(json!(true)),
            );
            put("timeout_ms", int("--timeout")?);
            "ui_act"
        }
        ("scroll", [l]) => {
            put("locator", Some(json!(l)));
            put("action", Some(json!("scroll")));
            put("dx", num("--dx")?);
            put("dy", num("--dy")?);
            put(
                "unit",
                flags.contains_key("--pixels").then(|| json!("pixels")),
            );
            "ui_act"
        }
        ("drag", [l]) => {
            put("locator", Some(json!(l)));
            put("action", Some(json!("drag")));
            put("to", flags.get("--to").copied().flatten().map(|t| json!(t)));
            if let Some(o) = flags.get("--offset").copied().flatten() {
                let (x, y) = o
                    .split_once(',')
                    .and_then(|(x, y)| {
                        Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
                    })
                    .ok_or_else(|| Failure::Usage("--offset takes <x>,<y>".into()))?;
                put("offset", Some(json!({"x": x, "y": y})));
            }
            "ui_act"
        }
        ("expect", [l, matcher, text @ ..]) if text.len() <= 1 => {
            put("locator", Some(json!(l)));
            put("matcher", Some(json!(matcher)));
            put("text", text.first().map(|t| json!(t)));
            put("not", flags.contains_key("--not").then_some(json!(true)));
            put(
                "compare",
                if flags.contains_key("--contains") {
                    Some(json!("contains"))
                } else if flags.contains_key("--regex") {
                    Some(json!("regex"))
                } else {
                    None
                },
            );
            put("timeout_ms", int("--timeout")?);
            if *matcher == "count" {
                let n = text
                    .first()
                    .and_then(|t| t.parse::<usize>().ok())
                    .ok_or_else(|| Failure::Usage("expect <locator> count <n>".into()))?;
                args.remove("text");
                args.insert("exactly".into(), json!(n));
            }
            "ui_expect"
        }
        ("screenshot", [file]) if is_png_name(file) => {
            put(
                "locator",
                flags.get("--of").copied().flatten().map(|l| json!(l)),
            );
            if let Some(r) = flags.get("--region").copied().flatten() {
                let n: Vec<f32> = r
                    .split(',')
                    .map(|v| v.trim().parse::<f32>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| Failure::Usage("--region takes <x>,<y>,<w>,<h>".into()))?;
                let [x, y, w, h] = n.as_slice() else {
                    return Err(Failure::Usage("--region takes <x>,<y>,<w>,<h>".into()));
                };
                put("region", Some(json!({"x": x, "y": y, "w": w, "h": h})));
            }
            "ui_screenshot"
        }
        // The window commands of SPEC-0319 v7 behaviours 46-52.
        ("open", [path]) => {
            put("command", Some(json!("open_file")));
            put("path", Some(json!(path)));
            put("line", int("--line")?);
            put("column", int("--column")?);
            "ui_command"
        }
        ("tool-window", [id, op @ ..]) if op.len() <= 1 => {
            put("command", Some(json!("tool_window")));
            put("id", Some(json!(id)));
            put("op", op.first().map(|o| json!(o)));
            "ui_command"
        }
        ("settings", ["close"]) => {
            put("command", Some(json!("close_settings")));
            "ui_command"
        }
        ("settings", [page]) => {
            put("command", Some(json!("settings")));
            put("page", Some(json!(page)));
            "ui_command"
        }
        ("invoke", [action]) => {
            put("command", Some(json!("invoke")));
            put("action", Some(json!(action)));
            "ui_command"
        }
        ("type-text", [text]) => {
            put("command", Some(json!("type_text")));
            put("text", Some(json!(text)));
            "ui_command"
        }
        ("popup" | "list" | "dismiss", []) => {
            put("command", Some(json!(verb)));
            "ui_command"
        }
        ("choose", []) if flags.get("--label").copied().flatten().is_some() => {
            put("command", Some(json!("choose")));
            put(
                "label",
                flags.get("--label").copied().flatten().map(|l| json!(l)),
            );
            "ui_command"
        }
        ("choose", [index]) => {
            let index = index
                .parse::<u64>()
                .map_err(|_| Failure::Usage("choose <index> | choose --label <label>".into()))?;
            put("command", Some(json!("choose")));
            put("index", Some(json!(index)));
            "ui_command"
        }
        ("trace", ["start" | "status"]) => {
            put("command", positional.first().map(|c| json!(c)));
            "ui_trace"
        }
        ("trace", ["stop", path @ ..]) if path.len() <= 1 => {
            put("command", Some(json!("stop")));
            put("path", path.first().map(|p| json!(p)));
            "ui_trace"
        }
        _ => return Err(usage()),
    };
    Ok((tool, Value::Object(args)))
}

/// Connects to the window that serves the project (`--project`, else the current folder).
fn connect(options: &Options) -> Result<ferris_idea_mcp::client::Client, Failure> {
    let Some(config) = ferris_idea_core::user_config_dir() else {
        return Err(Failure::Unavailable(
            "no settings folder (HOME unset)".into(),
        ));
    };
    let folder = match &options.project {
        Some(p) => p.clone(),
        None => std::env::current_dir().map_err(|e| Failure::Unavailable(e.to_string()))?,
    };
    let folder = std::fs::canonicalize(&folder).unwrap_or(folder);
    let windows = ferris_idea_mcp::client::running_windows(&config);
    let candidates: Vec<_> = windows
        .iter()
        .filter(|w| options.pid.is_none_or(|p| w.pid == p))
        .filter_map(|w| {
            // The project the folder lies in.
            w.projects
                .iter()
                .filter(|r| {
                    let r = std::fs::canonicalize(r).unwrap_or_else(|_| PathBuf::from(r));
                    folder.starts_with(&r)
                })
                .max_by_key(|r| r.len())
                .map(|r| (w, r.clone()))
        })
        .collect();
    let (window, root) = match candidates.as_slice() {
        [one] => one.clone(),
        [] => {
            return Err(Failure::Unavailable(format!(
                "no running Ferris IDEA window serves a project containing {} over MCP (Settings › AI Assistant › \"Serve MCP to local agents over HTTP\")",
                folder.display()
            )));
        }
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|(w, r)| format!("{} ({r})", w.pid))
                .collect();
            return Err(Failure::Usage(format!(
                "several Ferris IDEA windows serve this project; pass --pid with one of: {}",
                list.join("; ")
            )));
        }
    };
    ferris_idea_mcp::client::Client::connect_project(window, &root, "ferris-idea-drive")
        .map_err(Failure::Unavailable)
}

fn print(json_out: bool, verb: &str, result: &Value) {
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
        return;
    }
    let element = |e: &Value| -> String {
        let mut line = format!("{} {}", e["id"], e["role"].as_str().unwrap_or("?"));
        if let Some(n) = e["name"].as_str() {
            line.push_str(&format!(" {n:?}"));
        }
        if let Some(t) = e["test_id"].as_str() {
            line.push_str(&format!(" #{t}"));
        }
        if let Some(states) = e["states"].as_array().filter(|s| !s.is_empty()) {
            let s: Vec<&str> = states.iter().filter_map(Value::as_str).collect();
            line.push_str(&format!(" [{}]", s.join(", ")));
        }
        line
    };
    match verb {
        "snapshot" => match result["outline"].as_str() {
            Some(o) => print!("{o}"),
            None => println!(
                "{}",
                serde_json::to_string_pretty(result).unwrap_or_default()
            ),
        },
        "find" => {
            for m in result["matches"].as_array().into_iter().flatten() {
                println!("{}", element(m));
            }
            println!("{} match(es)", result["count"]);
        }
        "status" => println!(
            "external driving: {}; drivers: {}; paused: {}",
            if result["external"] == json!(true) {
                "on"
            } else {
                "off"
            },
            result["drivers"].as_array().map_or(0, Vec::len),
            result["paused"]
        ),
        "stop" => println!("UI automation stopped; external driving is off"),
        "expect" => println!(
            "ok: {} holds",
            result["matcher"].as_str().unwrap_or("assertion")
        ),
        "screenshot" => println!(
            "ok: {}x{} pixels at scale {}, {} area(s) filled",
            result["width"], result["height"], result["scale"], result["masked"]
        ),
        "popup" => {
            if result["open"] == json!(true) {
                println!("{} popup:", result["kind"].as_str().unwrap_or("?"));
                for (i, item) in result["items"].as_array().into_iter().flatten().enumerate() {
                    let mark = if result["selected"] == json!(i) {
                        ">"
                    } else {
                        " "
                    };
                    let mut line = format!(
                        "{mark}{i:>3} [{}] {}",
                        item["kind"].as_str().unwrap_or("?"),
                        item["label"].as_str().unwrap_or_default()
                    );
                    if let Some(d) = item["detail"].as_str() {
                        line.push_str(&format!("  ({d})"));
                    }
                    println!("{line}");
                }
            } else {
                println!("no popup is open");
            }
        }
        "list" => {
            println!("tool windows:");
            for t in result["tool_windows"].as_array().into_iter().flatten() {
                println!(
                    "  {:<14} {}{}",
                    t["id"].as_str().unwrap_or_default(),
                    t["title"].as_str().unwrap_or_default(),
                    if t["shown"] == json!(true) {
                        " (shown)"
                    } else {
                        ""
                    }
                );
            }
            println!("settings pages:");
            for p in result["settings_pages"].as_array().into_iter().flatten() {
                println!(
                    "  {:<22} {}{}",
                    p["id"].as_str().unwrap_or_default(),
                    p["title"].as_str().unwrap_or_default(),
                    if p["reserved"] == json!(true) {
                        " (reserved for the user)"
                    } else {
                        ""
                    }
                );
            }
        }
        "open" | "tool-window" | "settings" | "invoke" | "type-text" | "choose" | "dismiss" => {
            println!("ok: {result}")
        }
        _ => match result.get("element") {
            Some(e) if !e.is_null() => println!("ok: {}", element(e)),
            _ => println!("ok"),
        },
    }
}

/// One step of a UI script.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    locator: Option<String>,
    action: Option<String>,
    /// A window command (`ui_command`): `open_file`, `tool_window`, `settings`, …
    command: Option<String>,
    path: Option<String>,
    line: Option<u64>,
    column: Option<u64>,
    id: Option<String>,
    op: Option<String>,
    page: Option<String>,
    /// `invoke`: the action id.
    invoke: Option<String>,
    index: Option<u64>,
    label: Option<String>,
    expect: Option<String>,
    keys: Option<String>,
    text: Option<String>,
    clear: Option<bool>,
    count: Option<u8>,
    button: Option<String>,
    compare: Option<String>,
    not: Option<bool>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    #[serde(default)]
    step: Vec<Step>,
}

/// The steps of a UI script: a project file of kind `ui-script` (ADR-0021).
fn read_script(path: &Path) -> Result<Vec<Step>, Failure> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::Usage(format!("{}: {e}", path.display())))?;
    let mut table: toml::Table = toml::from_str(&text)
        .map_err(|e| Failure::Usage(format!("{}: {}", path.display(), e.message())))?;
    ferris_idea_core::format::adopt_former_envelope(&mut table);
    if !table.contains_key("ferris-idea") {
        return Err(Failure::Usage(format!(
            "{}: a UI script starts with [ferris-idea] kind = \"ui-script\" format = 1",
            path.display()
        )));
    }
    ferris_idea_core::format::take_envelope(&mut table, "ui-script").map_err(|p| {
        Failure::Usage(format!(
            "{}: {}",
            path.display(),
            ferris_idea_core::format::problems_text(&p)
        ))
    })?;
    let script: Script = toml::Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| {
            Failure::Usage(format!("{}: {}", path.display(), e.message()))
        })?;
    Ok(script.step)
}

fn run_script(options: &Options, path: &Path) -> Result<(), Failure> {
    let steps = read_script(path)?;
    let mut client = connect(options)?;
    for (i, step) in steps.iter().enumerate() {
        let mut args = serde_json::Map::new();
        if let Some(l) = &step.locator {
            args.insert("locator".into(), json!(l));
        }
        if let Some(t) = step.timeout_ms {
            args.insert("timeout_ms".into(), json!(t));
        }
        let tool = match (&step.action, &step.expect) {
            (None, None) if step.command.is_some() => {
                args.insert("command".into(), json!(step.command));
                for (k, v) in [
                    ("path", step.path.as_ref().map(|v| json!(v))),
                    ("line", step.line.map(|v| json!(v))),
                    ("column", step.column.map(|v| json!(v))),
                    ("id", step.id.as_ref().map(|v| json!(v))),
                    ("op", step.op.as_ref().map(|v| json!(v))),
                    ("page", step.page.as_ref().map(|v| json!(v))),
                    ("action", step.invoke.as_ref().map(|v| json!(v))),
                    ("text", step.text.as_ref().map(|v| json!(v))),
                    ("index", step.index.map(|v| json!(v))),
                    ("label", step.label.as_ref().map(|v| json!(v))),
                ] {
                    if let Some(v) = v {
                        args.insert(k.into(), v);
                    }
                }
                args.remove("locator");
                "ui_command"
            }
            (Some(action), None) => {
                args.insert("action".into(), json!(action));
                for (k, v) in [
                    ("keys", step.keys.as_ref().map(|v| json!(v))),
                    ("text", step.text.as_ref().map(|v| json!(v))),
                    ("clear", step.clear.map(|v| json!(v))),
                    ("count", step.count.map(|v| json!(v))),
                    ("button", step.button.as_ref().map(|v| json!(v))),
                ] {
                    if let Some(v) = v {
                        args.insert(k.into(), v);
                    }
                }
                "ui_act"
            }
            (None, Some(matcher)) => {
                args.insert("matcher".into(), json!(matcher));
                for (k, v) in [
                    ("text", step.text.as_ref().map(|v| json!(v))),
                    ("compare", step.compare.as_ref().map(|v| json!(v))),
                    ("not", step.not.map(|v| json!(v))),
                ] {
                    if let Some(v) = v {
                        args.insert(k.into(), v);
                    }
                }
                "ui_expect"
            }
            _ => {
                return Err(Failure::Usage(format!(
                    "{}: step {}: give `action`, `expect` or `command`",
                    path.display(),
                    i + 1
                )));
            }
        };
        match client.call(tool, Value::Object(args)) {
            Ok(_) => {
                if !options.json {
                    println!("step {}: ok", i + 1);
                }
            }
            Err(e) => {
                let f = Failure::of_tool(format!("step {}: {e}", i + 1));
                return Err(match f {
                    Failure::Usage(t) | Failure::Failed(t) => Failure::Failed(t),
                    other => other,
                });
            }
        }
    }
    if options.json {
        println!("{}", json!({"ok": true, "steps": steps.len()}));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn verbs_become_tool_calls() {
        let (tool, args) =
            call_of("find", &s(&["role=button[name=\"Commit\"]", "--wait"])).unwrap();
        assert_eq!(tool, "ui_find");
        assert_eq!(
            args,
            json!({"locator": "role=button[name=\"Commit\"]", "wait": true})
        );
        let (tool, args) = call_of("press", &s(&["alt-f12"])).unwrap();
        assert_eq!(tool, "ui_act");
        assert_eq!(args, json!({"action": "press", "keys": "alt-f12"}));
        let (_, args) = call_of("type", &s(&["id=search", "@ö 日本語", "--clear"])).unwrap();
        assert_eq!(args["text"], "@ö 日本語");
        let (_, args) = call_of("expect", &s(&["id=list", "count", "3"])).unwrap();
        assert_eq!(args["exactly"], 3);
        assert!(args.get("text").is_none());
        let (tool, args) = call_of("open", &s(&["src/main.rs", "--line", "3"])).unwrap();
        assert_eq!(tool, "ui_command");
        assert_eq!(
            args,
            json!({"command": "open_file", "path": "src/main.rs", "line": 3})
        );
        let (_, args) = call_of("tool-window", &s(&["problems"])).unwrap();
        assert_eq!(args, json!({"command": "tool_window", "id": "problems"}));
        let (_, args) = call_of("tool-window", &s(&["containers", "close"])).unwrap();
        assert_eq!(args["op"], "close");
        let (_, args) = call_of("settings", &s(&["close"])).unwrap();
        assert_eq!(args, json!({"command": "close_settings"}));
        let (_, args) = call_of("invoke", &s(&["ShowIntentionActions"])).unwrap();
        assert_eq!(
            args,
            json!({"command": "invoke", "action": "ShowIntentionActions"})
        );
        let (_, args) = call_of("choose", &s(&["2"])).unwrap();
        assert_eq!(args, json!({"command": "choose", "index": 2}));
        let (_, args) = call_of("choose", &s(&["--label", "Remove TODO"])).unwrap();
        assert_eq!(args["label"], "Remove TODO");
        let (_, args) = call_of("popup", &[]).unwrap();
        assert_eq!(args, json!({"command": "popup"}));
        let (tool, args) =
            call_of("screenshot", &s(&["shot.png", "--region", "0,0,200,100"])).unwrap();
        assert_eq!(tool, "ui_screenshot");
        assert_eq!(
            args["region"],
            json!({"x": 0.0, "y": 0.0, "w": 200.0, "h": 100.0})
        );
        assert!(matches!(
            call_of("screenshot", &s(&["shot.txt"])),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            call_of("choose", &s(&["first"])),
            Err(Failure::Usage(_))
        ));
        let (tool, _) = call_of("stop", &[]).unwrap();
        assert_eq!(tool, "ui_driver_stop");
        assert!(matches!(call_of("click", &[]), Err(Failure::Usage(_))));
        assert!(matches!(
            call_of("dance", &s(&["x"])),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            call_of("find", &s(&["id=x", "--timeout", "soon"])),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn tool_errors_map_to_exit_codes() {
        let code = |t: &str| match Failure::of_tool(t.into()) {
            Failure::Usage(_) => 2,
            Failure::Failed(_) => 1,
            Failure::Unavailable(_) => 3,
        };
        assert_eq!(code("unavailable: agents and scripts may not operate"), 3);
        assert_eq!(code("not_granted: …"), 3);
        assert_eq!(code("invalid: unknown role"), 2);
        assert_eq!(code("deadline_exceeded: expected visible"), 1);
        assert_eq!(code("ambiguous: 2 matches"), 1);
    }

    #[test]
    fn ui_scripts_need_their_envelope() {
        let dir = std::env::temp_dir().join(format!("ferris-idea-drive-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("smoke.toml");
        std::fs::write(
            &good,
            "[ferris-idea]\nkind = \"ui-script\"\nformat = 1\n\n[[step]]\naction = \"press\"\nkeys = \"cmd-1\"\n\n[[step]]\nlocator = \"id=project-tree\"\nexpect = \"visible\"\n",
        )
        .unwrap();
        let steps = read_script(&good).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].expect.as_deref(), Some("visible"));
        let bare = dir.join("bare.toml");
        std::fs::write(&bare, "[[step]]\naction = \"press\"\nkeys = \"cmd-1\"\n").unwrap();
        assert!(read_script(&bare).is_err());
        let other = dir.join("other.toml");
        std::fs::write(&other, "[ferris-idea]\nkind = \"run\"\nformat = 1\n").unwrap();
        assert!(read_script(&other).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
