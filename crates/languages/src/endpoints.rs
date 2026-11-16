//! HTTP endpoints declared in the code (`SPEC-0103`): Spring MVC, JAX-RS (Jakarta EE, Quarkus),
//! Micronaut and NestJS annotations, FastAPI and Flask decorators, Express routes. A line scanner
//! over the declarations, not a compiler: it finds what the frameworks' annotations say literally.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use idecraft_core::hump_match;
use serde::Serialize;

/// Whether a row is a route the application serves or one it calls (`SPEC-0103` v4 behaviour 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    /// A route of the application.
    Server,
    /// A method of a REST client interface: a route of another service.
    Client,
}

/// One row: a mapping under one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Endpoint {
    /// Framework, e.g. `Spring`, `JAX-RS`, `FastAPI`.
    pub framework: &'static str,
    /// Served by the application or called by it.
    pub kind: EndpointKind,
    /// HTTP methods in capitals, in the order the mapping names them; empty when it names none
    /// and so takes every method.
    pub methods: Vec<String>,
    /// Path with the class or router prefix, starting with `/`.
    pub path: String,
    /// Handler: `Class.method`, a function name, or empty for an inline handler.
    pub handler: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line of the declaration.
    pub line: usize,
}

impl Endpoint {
    /// The methods as one row shows them: `GET`, `POST|PUT`, empty for a mapping without a
    /// method.
    pub fn method_label(&self) -> String {
        self.methods.join("|")
    }

    /// The row as text: methods, path and handler, a client marked as such.
    pub fn row_text(&self) -> String {
        let mut text = format!("{} {} {}", self.method_label(), self.path, self.handler)
            .trim()
            .to_owned();
        if self.kind == EndpointKind::Client {
            text.push_str(" (client)");
        }
        text
    }
}

/// The first string literal in `s` ('…', "…" or `…`).
fn first_string(s: &str) -> Option<String> {
    let start = s.find(['"', '\'', '`'])?;
    let quote = s[start..].chars().next()?;
    let rest = &s[start + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

/// The string after `key =` / `key=` inside an annotation's arguments, e.g. `value = "/x"`.
fn named_string(args: &str, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        let at = args
            .find(&format!("{k} ="))
            .or_else(|| args.find(&format!("{k}=")))?;
        first_string(&args[at..])
    })
}

/// Where the parenthesis opening at the start of `s` closes; quotes are skipped. `None` when it
/// stays open.
fn close_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether every parenthesis of `s` is closed (quotes skipped).
fn balanced(s: &str) -> bool {
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        match close_paren(&rest[open..]) {
            Some(close) => rest = &rest[open + close + 1..],
            None => return false,
        }
    }
    true
}

/// Annotation arguments split at the top-level commas: quotes, parentheses, braces and brackets
/// nest (`value = {"/a", "/b"}, method = RequestMethod.GET` gives two parts).
pub(crate) fn split_args(args: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (i, ch) in args.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = args[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// One argument as attribute name and value: `path = "/x"` gives `(Some("path"), "\"/x\"")`, a
/// value alone `(None, value)`.
pub(crate) fn named_arg(part: &str) -> (Option<&str>, &str) {
    let t = part.trim();
    let len: usize = t
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .map(char::len_utf8)
        .sum();
    let rest = t[len..].trim_start();
    if len > 0 && rest.starts_with('=') && !rest.starts_with("==") {
        (Some(&t[..len]), rest[1..].trim())
    } else {
        (None, t)
    }
}

/// Every string literal in `s`.
fn all_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find(['"', '\'', '`']) {
        let quote = rest[start..].chars().next().unwrap_or('"');
        let body = &rest[start + 1..];
        let Some(end) = body.find(quote) else {
            break;
        };
        out.push(body[..end].to_owned());
        rest = &body[end + 1..];
    }
    out
}

/// The strings an attribute value gives: one literal, or every literal of an array
/// (`{"/a", "/b"}`, `["/a", "/b"]`). A constant or an expression gives none.
pub(crate) fn value_strings(value: &str) -> Vec<String> {
    let v = value.trim();
    if v.starts_with(['{', '[']) {
        all_strings(v)
    } else if v.starts_with(['"', '\'', '`']) {
        first_string(v).into_iter().collect()
    } else {
        Vec::new()
    }
}

/// The paths of a mapping annotation (v3 behaviour 6): the attribute `path`, `value`, `uri` or
/// `uris`, else the value written without a name. Other attributes (`produces`, `params`,
/// `consumes`, …) are never a path. `None` when the annotation names no literal path.
fn paths_arg(args: &str) -> Option<Vec<String>> {
    let parts: Vec<(Option<&str>, &str)> = split_args(args).into_iter().map(named_arg).collect();
    let value = parts
        .iter()
        .find(|(n, _)| matches!(n, Some("path" | "value" | "uri" | "uris")))
        .or_else(|| parts.iter().find(|(n, _)| n.is_none()))
        .map(|(_, v)| *v)?;
    let found = value_strings(value);
    (!found.is_empty()).then_some(found)
}

/// The HTTP methods of `@RequestMapping(method = …)`: one `RequestMethod`, an array of them, or
/// statically imported names, each once. Empty without the attribute.
fn request_methods(args: &str) -> Vec<String> {
    const VERBS: &[&str] = &[
        "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE",
    ];
    let value = split_args(args)
        .into_iter()
        .map(named_arg)
        .find(|(n, _)| *n == Some("method"))
        .map_or("", |(_, v)| v);
    let mut found: Vec<String> = Vec::new();
    for word in value
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| VERBS.contains(w))
    {
        if !found.iter().any(|f| f == word) {
            found.push(word.to_owned());
        }
    }
    found
}

/// The annotation `line` starts with: its simple name, its arguments and the rest of the line.
fn leading_annotation(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix('@')?;
    let len: usize = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .map(char::len_utf8)
        .sum();
    let name = rest[..len].rsplit('.').next().unwrap_or("");
    let after = &rest[len..];
    if !after.starts_with('(') {
        return Some((name, "", after));
    }
    match close_paren(after) {
        Some(close) => Some((name, &after[1..close], &after[close + 1..])),
        None => Some((name, &after[1..], "")),
    }
}

/// The lines of `text` as declarations read them: an annotation whose arguments run over several
/// lines is one line, at the line it starts on; comment lines are left out.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with("//") || line.starts_with('*') || line.starts_with("/*") {
            continue;
        }
        if open && let Some((_, last)) = out.last_mut() {
            last.push(' ');
            last.push_str(line);
            open = !balanced(last) && last.len() < 4096;
            continue;
        }
        open = line.starts_with('@') && !balanced(line);
        out.push((n, line.to_owned()));
    }
    out
}

/// Joins a prefix and a path into `/a/b`.
fn join(prefix: &str, path: &str) -> String {
    let parts: Vec<&str> = prefix
        .split('/')
        .chain(path.split('/'))
        .filter(|p| !p.is_empty())
        .collect();
    format!("/{}", parts.join("/"))
}

/// Whether `framework` tells a path with a slash at its end from the path without (v4 behaviour
/// 7): Spring MVC since 6.0, FastAPI and Flask by their documentation. The others match both.
fn slash_counts(framework: &str) -> bool {
    matches!(framework, "Spring" | "FastAPI" | "Flask")
}

/// [`join`] as the row shows it: where the framework tells the two paths apart, a slash at the
/// end of the handler's own path stays (`/api/products/` for the path `/` below `/api/products`).
fn shown_path(framework: &str, prefix: &str, path: &str) -> String {
    let mut joined = join(prefix, path);
    if slash_counts(framework) && path.ends_with('/') && !joined.ends_with('/') {
        joined.push('/');
    }
    joined
}

/// The text between the first `(` after `at` and its matching `)` on the line (or to the end).
fn args_of(line: &str, at: usize) -> &str {
    let Some(open) = line[at..].find('(').map(|i| at + i) else {
        return "";
    };
    let mut depth = 0;
    for (i, ch) in line[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &line[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    &line[open + 1..]
}

/// The name declared on a method line: the identifier before the first `(` (Java, Kotlin `fun`, TS).
fn declared_name(line: &str) -> String {
    let head = line.split('(').next().unwrap_or("");
    head.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .rfind(|w| !w.is_empty())
        .unwrap_or("")
        .to_owned()
}

fn is_class_line(line: &str) -> bool {
    let t = line.trim_start();
    [
        "class ",
        "interface ",
        "object ",
        "export class ",
        "public class ",
        "abstract class ",
    ]
    .iter()
    .any(|k| t.starts_with(k) || t.contains(&format!(" {k}")))
}

/// A mapping annotation seen before a declaration.
struct Mapping {
    framework: &'static str,
    /// HTTP methods; `None` for an annotation that only gives a path (`@Path`, `@Controller`),
    /// empty for a mapping that names no method.
    methods: Option<Vec<String>>,
    /// Paths; `None` when the annotation names none.
    paths: Option<Vec<String>>,
    /// 0-based line of the annotation.
    line: usize,
}

/// Annotation-based frameworks: Java, Kotlin and TypeScript (NestJS).
fn annotated(rel: &Path, text: &str, out: &mut Vec<Endpoint>) {
    const MAPPINGS: &[(&str, &str)] = &[
        ("GetMapping", "GET"),
        ("PostMapping", "POST"),
        ("PutMapping", "PUT"),
        ("DeleteMapping", "DELETE"),
        ("PatchMapping", "PATCH"),
    ];
    const JAXRS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    // Micronaut and NestJS method decorators.
    const SHORT: &[(&str, &str)] = &[
        ("Get", "GET"),
        ("Post", "POST"),
        ("Put", "PUT"),
        ("Delete", "DELETE"),
        ("Patch", "PATCH"),
        ("Head", "HEAD"),
        ("Options", "OPTIONS"),
        ("Trace", "TRACE"),
    ];
    // Interfaces with these annotations describe a service the application calls (MicroProfile
    // REST client, Micronaut declarative client, OpenFeign), not one it offers: their methods are
    // rows of kind client (v4 behaviour 7).
    const CLIENTS: &[&str] = &["RegisterRestClient", "Client", "FeignClient"];
    let ts = rel.extension().is_some_and(|e| e == "ts");
    // Spring's stereotype `@Controller` carries a bean name, not a path.
    let spring_controller = !ts && text.contains("org.springframework.");
    let mut prefixes: Vec<String> = vec![String::new()];
    let mut class = String::new();
    // Whether the class being read serves its routes or is a client.
    let mut kind = EndpointKind::Server;
    // Mapping annotations seen since the last declaration.
    let mut pending: Vec<Mapping> = Vec::new();
    // A client annotation seen since the last declaration, with the path prefix it names.
    let mut client: Option<Option<String>> = None;
    for (n, line) in logical_lines(text) {
        let mut rest = line.as_str();
        while let Some((name, args, after)) = leading_annotation(rest) {
            let mut push = |framework, methods, paths| {
                pending.push(Mapping {
                    framework,
                    methods,
                    paths,
                    line: n,
                });
            };
            if name == "RequestMapping" {
                push("Spring", Some(request_methods(args)), paths_arg(args));
            } else if let Some((_, m)) = MAPPINGS.iter().find(|(a, _)| *a == name) {
                push("Spring", Some(vec![(*m).to_owned()]), paths_arg(args));
            } else if name == "Path" {
                push("JAX-RS", None, paths_arg(args));
            } else if JAXRS.contains(&name) {
                push("JAX-RS", Some(vec![name.to_owned()]), None);
            } else if name == "Controller" && !spring_controller {
                let framework = if ts { "NestJS" } else { "Micronaut" };
                push(framework, None, paths_arg(args));
            } else if let Some((_, m)) = SHORT.iter().find(|(a, _)| *a == name) {
                let framework = if ts { "NestJS" } else { "Micronaut" };
                push(framework, Some(vec![(*m).to_owned()]), paths_arg(args));
            } else if CLIENTS.contains(&name) {
                client = Some(client_prefix(name, args));
            }
            rest = after.trim_start();
        }
        if rest.is_empty() {
            // Annotations alone on their line: the declaration follows.
            continue;
        }
        if is_class_line(rest) && (!pending.is_empty() || !rest.contains('(')) {
            // The class-level mapping is the prefix; a class without one resets it.
            class = declared_class(rest);
            prefixes = pending
                .iter()
                .filter(|m| m.methods.as_ref().is_none_or(Vec::is_empty))
                .find_map(|m| m.paths.clone())
                .or_else(|| client.clone().flatten().map(|p| vec![p]))
                .unwrap_or_else(|| vec![String::new()]);
            kind = if client.is_some() {
                EndpointKind::Client
            } else {
                EndpointKind::Server
            };
            client = None;
            pending.clear();
            continue;
        }
        client = None;
        if pending.is_empty() {
            continue;
        }
        // A method: one row for each path of its annotations under each prefix of the class,
        // with every HTTP method the mapping names (v4 behaviour 7).
        if let Some(http) = pending.iter().find(|m| m.methods.is_some()) {
            let methods = http.methods.clone().unwrap_or_default();
            let paths = pending
                .iter()
                .find_map(|m| m.paths.clone())
                .unwrap_or_else(|| vec![String::new()]);
            let handler = declared_name(rest);
            let handler = if class.is_empty() {
                handler
            } else {
                format!("{class}.{handler}")
            };
            let framework = pending[0].framework;
            for prefix in &prefixes {
                for path in &paths {
                    out.push(Endpoint {
                        framework,
                        kind,
                        methods: methods.clone(),
                        path: shown_path(framework, prefix, path),
                        handler: handler.clone(),
                        file: rel.to_path_buf(),
                        line: http.line,
                    });
                }
            }
        }
        pending.clear();
    }
}

/// The path every method of a client interface starts with, as its annotation names it: the
/// `path` of `@FeignClient` and of Micronaut's `@Client`, and a `@Client` value that is a path
/// (it starts with `/`) and not an address or a service name. MicroProfile's `@RegisterRestClient`
/// names none; its interface carries `@Path`.
fn client_prefix(name: &str, args: &str) -> Option<String> {
    let parts: Vec<(Option<&str>, &str)> = split_args(args).into_iter().map(named_arg).collect();
    let named = |key: &str| {
        parts
            .iter()
            .find(|(n, _)| *n == Some(key))
            .and_then(|(_, v)| value_strings(v).into_iter().next())
    };
    match name {
        "FeignClient" => named("path"),
        "Client" => named("path").or_else(|| {
            parts
                .iter()
                .find(|(n, _)| matches!(n, None | Some("value" | "id")))
                .and_then(|(_, v)| value_strings(v).into_iter().next())
                .filter(|v| v.starts_with('/'))
        }),
        _ => None,
    }
}

/// The base path of a JAX-RS application: the value of `@ApplicationPath` (v3 behaviour 6).
fn application_path(text: &str) -> Option<String> {
    logical_lines(text).into_iter().find_map(|(_, line)| {
        let mut rest = line.as_str();
        while let Some((name, args, after)) = leading_annotation(rest) {
            if name == "ApplicationPath" {
                return first_string(args);
            }
            rest = after.trim_start();
        }
        None
    })
}

fn declared_class(line: &str) -> String {
    let words: Vec<&str> = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    words
        .iter()
        .position(|w| matches!(*w, "class" | "interface" | "object"))
        .and_then(|i| words.get(i + 1))
        .map(|w| w.to_string())
        .unwrap_or_default()
}

/// FastAPI and Flask: `@app.get("/x")`, `@router.post(...)`, `@bp.route("/x", methods=[...])`, with
/// `APIRouter(prefix=…)` and `Blueprint(…, url_prefix=…)` prefixes of the same file.
fn python(rel: &Path, text: &str, out: &mut Vec<Endpoint>) {
    let mut prefixes: HashMap<String, (String, &'static str)> = HashMap::new();
    for line in text.lines() {
        let t = line.trim();
        let Some((var, value)) = t.split_once('=') else {
            continue;
        };
        let var = var.trim();
        if value.contains("APIRouter(") {
            let p = named_string(value, &["prefix"]).unwrap_or_default();
            prefixes.insert(var.to_owned(), (p, "FastAPI"));
        } else if value.contains("FastAPI(") {
            prefixes.insert(var.to_owned(), (String::new(), "FastAPI"));
        } else if value.contains("Blueprint(") {
            let p = named_string(value, &["url_prefix"]).unwrap_or_default();
            prefixes.insert(var.to_owned(), (p, "Flask"));
        } else if value.contains("Flask(") {
            prefixes.insert(var.to_owned(), (String::new(), "Flask"));
        }
    }
    let lines: Vec<&str> = text.lines().collect();
    for (n, raw) in lines.iter().enumerate() {
        let t = raw.trim();
        let Some(rest) = t.strip_prefix('@') else {
            continue;
        };
        let Some((var, call)) = rest.split_once('.') else {
            continue;
        };
        let verb: String = call.chars().take_while(|c| c.is_alphanumeric()).collect();
        let args = args_of(call, 0);
        let Some(path) = first_string(args) else {
            continue;
        };
        let (prefix, framework) = prefixes
            .get(var)
            .cloned()
            .unwrap_or((String::new(), "FastAPI"));
        let methods: Vec<String> = match verb.as_str() {
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => {
                vec![verb.to_uppercase()]
            }
            "route" | "api_route" => {
                let mut list = args
                    .find("methods")
                    .map(|i| &args[i..])
                    .and_then(|s| s.find('[').zip(s.find(']')).map(|(a, b)| &s[a + 1..b]))
                    .map(|inner| {
                        inner
                            .split(',')
                            .filter_map(first_string)
                            .map(|m| m.to_uppercase())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let mut seen: Vec<String> = Vec::new();
                list.retain(|m| {
                    let new = !seen.contains(m);
                    seen.push(m.clone());
                    new
                });
                if list.is_empty() {
                    vec!["GET".into()]
                } else {
                    list
                }
            }
            _ => continue,
        };
        let framework = if verb == "route" { "Flask" } else { framework };
        let handler = lines[n + 1..]
            .iter()
            .map(|l| l.trim())
            .find(|l| l.starts_with("def ") || l.starts_with("async def "))
            .map(declared_name)
            .unwrap_or_default();
        // One row with every method of `methods=[…]` (v4 behaviour 7).
        out.push(Endpoint {
            framework,
            kind: EndpointKind::Server,
            methods,
            path: shown_path(framework, &prefix, &path),
            handler,
            file: rel.to_path_buf(),
            line: n,
        });
    }
}

/// Express: `app.get('/x', …)`, `router.post("/y", …)`, `app.all(…)`, `app.route('/x').get(…)`.
fn express(rel: &Path, text: &str, out: &mut Vec<Endpoint>) {
    if !text.contains("express") {
        return;
    }
    for (n, raw) in text.lines().enumerate() {
        let t = raw.trim();
        for verb in ["get", "post", "put", "delete", "patch", "all"] {
            let Some(at) = t.find(&format!(".{verb}(")) else {
                continue;
            };
            let receiver: String = t[..at]
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if !matches!(receiver.as_str(), "app" | "router" | "api" | "server")
                && !receiver.ends_with("Router")
            {
                continue;
            }
            let Some(path) = first_string(args_of(t, at)) else {
                continue;
            };
            if !path.starts_with('/') {
                continue;
            }
            out.push(Endpoint {
                framework: "Express",
                kind: EndpointKind::Server,
                // `app.all` takes every method.
                methods: if verb == "all" {
                    Vec::new()
                } else {
                    vec![verb.to_uppercase()]
                },
                path: join("", &path),
                handler: String::new(),
                file: rel.to_path_buf(),
                line: n,
            });
        }
    }
}

/// The endpoints declared in one file (behaviour 1), by its extension.
pub fn endpoints_in(rel: &Path, text: &str) -> Vec<Endpoint> {
    let mut out = Vec::new();
    match rel.extension().and_then(|e| e.to_str()) {
        Some("java" | "kt") => annotated(rel, text, &mut out),
        Some("ts") if text.contains("@nestjs") => annotated(rel, text, &mut out),
        Some("py") => python(rel, text, &mut out),
        Some("js" | "mjs" | "cjs" | "ts") => express(rel, text, &mut out),
        _ => {}
    }
    out
}

/// The rows of the project files (relative to `root`), sorted by path and methods (behaviour 1).
/// Files over 1 MiB and test sources under `src/test` are skipped.
pub fn endpoints(root: &Path, files: &[PathBuf]) -> Vec<Endpoint> {
    let mut out = Vec::new();
    let mut bases: Vec<String> = Vec::new();
    for rel in files {
        let s = rel.to_string_lossy();
        if s.contains("src/test/") || s.contains("node_modules/") {
            continue;
        }
        let path = root.join(rel);
        if std::fs::metadata(&path).map_or(true, |m| m.len() > 1 << 20) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.extend(endpoints_in(rel, &text));
            if rel.extension().is_some_and(|e| e == "java" || e == "kt")
                && text.contains("ApplicationPath")
                && let Some(base) = application_path(&text)
                && !bases.contains(&base)
            {
                bases.push(base);
            }
        }
    }
    // One JAX-RS application in the project: its base path leads every JAX-RS endpoint it
    // serves; a client's paths are another service's.
    if let [base] = bases.as_slice() {
        for e in out
            .iter_mut()
            .filter(|e| e.framework == "JAX-RS" && e.kind == EndpointKind::Server)
        {
            e.path = join(base, &e.path);
        }
    }
    out.sort_by(|a, b| {
        (&a.path, &a.methods, &a.file, a.line).cmp(&(&b.path, &b.methods, &b.file, b.line))
    });
    out
}

/// The endpoints of the project at `root` (`SPEC-0103` v2 behaviour 5): only the project's own
/// sources are scanned — no virtual environment, no installed dependencies, no build output,
/// nothing Git ignores — and of those the files [`endpoints`] takes.
pub fn project_endpoints(root: &Path) -> Vec<Endpoint> {
    endpoints(root, &idecraft_core::OwnSources::new(root).files())
}

/// The port the application listens on by its framework's default or its configuration
/// (`server.port` in `application.properties`/`.yml`, Micronaut `micronaut.server.port`, Quarkus
/// `quarkus.http.port`), for
/// generated requests (behaviour 3).
pub fn default_port(root: &Path, framework: &str) -> u16 {
    let configured = [
        "src/main/resources/application.properties",
        "src/main/resources/application.yml",
    ]
    .iter()
    .filter_map(|f| std::fs::read_to_string(root.join(f)).ok())
    .find_map(|text| {
        text.lines().find_map(|l| {
            let l = l.trim();
            let value = l
                .strip_prefix("server.port")
                .or_else(|| l.strip_prefix("micronaut.server.port"))
                .or_else(|| l.strip_prefix("quarkus.http.port"))
                .or_else(|| l.strip_prefix("port:"))?;
            value
                .trim_start_matches([' ', '=', ':'])
                .trim()
                .parse()
                .ok()
        })
    });
    configured.unwrap_or(match framework {
        "FastAPI" => 8000,
        "Flask" => 5000,
        "Express" | "NestJS" => 3000,
        _ => 8080,
    })
}

/// The request that tries `e` (behaviour 3): its first method (GET for a mapping without one) and
/// the URL on the framework's port, path parameters as they are. `None` for a client row: its
/// path is served by another service, whose address the scan does not know.
pub fn request(root: &Path, e: &Endpoint) -> Option<(String, String)> {
    if e.kind == EndpointKind::Client {
        return None;
    }
    let method = e.methods.first().map_or("GET", String::as_str);
    let port = default_port(root, e.framework);
    Some((
        method.to_owned(),
        format!("http://localhost:{port}{}", e.path),
    ))
}

/// The request as text for a request file: a `###` title (the handler, else methods and path) and
/// the request line. `None` for a client row.
pub fn request_text(root: &Path, e: &Endpoint) -> Option<String> {
    let (method, url) = request(root, e)?;
    let title = if e.handler.is_empty() {
        format!("{} {}", e.method_label(), e.path).trim().to_owned()
    } else {
        e.handler.clone()
    };
    Some(format!("### {title}\n{method} {url}\n"))
}

/// A row found by [`search_endpoints`], with the characters that matched (`SPEC-0067` v9
/// behaviour 20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointHit {
    /// The row of the model.
    pub endpoint: Endpoint,
    /// Char positions of the path that matched the query.
    pub path_positions: Vec<usize>,
    /// Char positions of the handler that matched the query.
    pub handler_positions: Vec<usize>,
}

/// The HTTP methods a query may start with to narrow the rows.
const QUERY_METHODS: [&str; 8] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE",
];

/// Splits a leading HTTP method off a query: `get users` is `GET` and `users`. The method counts
/// in either case and only with a space after it; `get` alone is text (`SPEC-0067` v9 behaviour
/// 20).
pub fn method_prefix(query: &str) -> (Option<&'static str>, &str) {
    let query = query.trim_start();
    if let Some((word, rest)) = query.split_once(char::is_whitespace)
        && let Some(method) = QUERY_METHODS.iter().find(|m| m.eq_ignore_ascii_case(word))
    {
        return (Some(method), rest.trim());
    }
    (None, query.trim())
}

/// How `text` matches a row: whether the path alone carries the match, the score and the
/// matched characters of path and handler.
fn match_endpoint(text: &str, e: &Endpoint) -> Option<(bool, i64, Vec<usize>, Vec<usize>)> {
    let handler = hump_match(text, &e.handler);
    if let Some(path) = hump_match(text, &e.path) {
        let handler_positions = handler.map(|m| m.positions).unwrap_or_default();
        return Some((true, path.score, path.positions, handler_positions));
    }
    if let Some(handler) = handler {
        return Some((false, handler.score, Vec::new(), handler.positions));
    }
    // Several words: each in the path or in the handler, in any order.
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let (mut in_path, mut score) = (true, 0);
    let (mut path_positions, mut handler_positions) = (Vec::new(), Vec::new());
    for word in &words {
        if let Some(m) = hump_match(word, &e.path) {
            score += m.score;
            path_positions.extend(m.positions);
        } else {
            let m = hump_match(word, &e.handler)?;
            in_path = false;
            score += m.score;
            handler_positions.extend(m.positions);
        }
    }
    for positions in [&mut path_positions, &mut handler_positions] {
        positions.sort_unstable();
        positions.dedup();
    }
    Some((
        in_path,
        score - 10 * words.len() as i64,
        path_positions,
        handler_positions,
    ))
}

/// Searches the rows of the model for `query`, at most `limit` (`SPEC-0067` v9 behaviour 20,
/// `SPEC-0103` v5 behaviour 8): a leading HTTP method keeps the rows of that method and those
/// without one, the rest is matched with Search Everywhere's rule for names against path and
/// handler. Rows whose path matches come first, then those found by the handler; within each
/// the higher score, the shorter path, then path, methods, file and line. Without text the rows
/// keep the order they have.
pub fn search_endpoints(endpoints: &[Endpoint], query: &str, limit: usize) -> Vec<EndpointHit> {
    let (method, text) = method_prefix(query);
    let of_method = |e: &&Endpoint| {
        method.is_none_or(|m| e.methods.is_empty() || e.methods.iter().any(|own| own == m))
    };
    let rows = endpoints.iter().filter(of_method);
    if text.is_empty() {
        return rows
            .take(limit)
            .map(|e| EndpointHit {
                endpoint: e.clone(),
                path_positions: Vec::new(),
                handler_positions: Vec::new(),
            })
            .collect();
    }
    // Path match first, then score, then the shorter path, then the model's order.
    let mut found: Vec<(bool, i64, EndpointHit)> = rows
        .filter_map(|e| {
            let (in_path, score, path_positions, handler_positions) = match_endpoint(text, e)?;
            let hit = EndpointHit {
                endpoint: e.clone(),
                path_positions,
                handler_positions,
            };
            Some((in_path, score, hit))
        })
        .collect();
    found.sort_by(|a, b| {
        let (x, y) = (&a.2.endpoint, &b.2.endpoint);
        (b.0, b.1)
            .cmp(&(a.0, a.1))
            .then(x.path.chars().count().cmp(&y.path.chars().count()))
            .then_with(|| {
                (&x.path, &x.methods, &x.file, x.line).cmp(&(&y.path, &y.methods, &y.file, y.line))
            })
    });
    found
        .into_iter()
        .take(limit)
        .map(|(_, _, hit)| hit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, text: &str) -> Vec<String> {
        endpoints_in(Path::new(name), text)
            .into_iter()
            .map(|e| format!("{} {} @{}", e.framework, e.row_text(), e.line))
            .collect()
    }

    // SPEC-0103/AK-5 (v2, behaviour 5).
    #[test]
    fn only_the_projects_own_sources_are_scanned() {
        let dir =
            std::env::temp_dir().join(format!("idecraft-endpoints-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let route = |path: &str| {
            format!(
                "from flask import Flask\napp = Flask(__name__)\n\n@app.route(\"/{path}\")\ndef handler():\n    pass\n"
            )
        };
        for (file, text) in [
            (".gitignore", "scratch.py\n".to_owned()),
            ("app.py", route("own")),
            ("scratch.py", route("ignored")),
            ("src/test/fixture.py", route("test")),
            (".venv/lib/python3.12/site-packages/x.py", route("venv")),
            (
                "node_modules/y.js",
                "const app = require(\"express\")();\napp.get(\"/module\", (req, res) => {});\n"
                    .to_owned(),
            ),
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        // The project is its own repository, wherever the temporary folder lies.
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let paths =
            |list: Vec<Endpoint>| -> Vec<String> { list.into_iter().map(|e| e.path).collect() };
        assert_eq!(paths(project_endpoints(&dir)), ["/own"]);
        // The plain workspace list finds the routes of the environment and of the ignored file too.
        let every = idecraft_core::Workspace::open(&dir)
            .unwrap()
            .files()
            .unwrap();
        assert_eq!(
            paths(endpoints(&dir, &every)),
            ["/ignored", "/own", "/venv"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn spring_and_jaxrs_and_micronaut() {
        let spring = r#"
@RestController
@RequestMapping("/api/items")
public class ItemController {
    @GetMapping
    public List<Item> list() { return items; }

    @GetMapping("/{id}")
    public Item one(@PathVariable long id) { return null; }

    @PostMapping(value = "/", consumes = "application/json")
    public Item add(@RequestBody Item i) { return i; }

    @RequestMapping(path = "/legacy", method = RequestMethod.DELETE)
    void legacy() {}

    // @GetMapping("/commented")
    private void helper() {}
}
"#;
        assert_eq!(
            found("ItemController.java", spring),
            [
                "Spring GET /api/items ItemController.list @4",
                "Spring GET /api/items/{id} ItemController.one @7",
                // Spring tells `/api/items/` from `/api/items` (v4 behaviour 7).
                "Spring POST /api/items/ ItemController.add @10",
                "Spring DELETE /api/items/legacy ItemController.legacy @13",
            ]
        );
        let jaxrs = "@Path(\"/greet\")\npublic class Greeter {\n  @GET\n  @Path(\"{name}\")\n  public String hi(@PathParam(\"name\") String n) { return n; }\n  @POST public void post() {}\n}\n";
        assert_eq!(
            found("Greeter.java", jaxrs),
            [
                "JAX-RS GET /greet/{name} Greeter.hi @2",
                "JAX-RS POST /greet Greeter.post @5"
            ]
        );
        let micronaut = "@Controller(\"/hello\")\nclass HelloController {\n    @Get(\"/{name}\")\n    fun hello(name: String) = \"Hi\"\n}\n";
        assert_eq!(
            found("Hello.kt", micronaut),
            ["Micronaut GET /hello/{name} HelloController.hello @2"]
        );
    }

    #[test]
    fn python_and_express_and_nest() {
        let fastapi = "from fastapi import APIRouter\nrouter = APIRouter(prefix=\"/users\")\n\n@router.get(\"/{uid}\")\nasync def read_user(uid: int):\n    ...\n\n@router.post(\"\")\ndef create(u):\n    ...\n";
        assert_eq!(
            found("users.py", fastapi),
            [
                "FastAPI GET /users/{uid} read_user @3",
                "FastAPI POST /users create @7"
            ]
        );
        let flask = "from flask import Flask\napp = Flask(__name__)\n\n@app.route(\"/login\", methods=[\"GET\", \"POST\"])\ndef login():\n    pass\n";
        assert_eq!(found("app.py", flask), ["Flask GET|POST /login login @3"]);
        let express = "const express = require('express');\nconst app = express();\napp.get('/health', (req, res) => res.send('ok'));\nrouter.post(\"/orders\", create);\nconsole.get('/nope');\n";
        assert_eq!(
            found("server.js", express),
            ["Express GET /health @2", "Express POST /orders @3"]
        );
        let nest = "import { Controller, Get } from '@nestjs/common';\n@Controller('cats')\nexport class CatsController {\n  @Get(':id')\n  findOne(): string { return ''; }\n}\n";
        assert_eq!(
            found("cats.controller.ts", nest),
            ["NestJS GET /cats/:id CatsController.findOne @3"]
        );
    }

    #[test]
    fn ports_from_configuration_or_defaults() {
        let root = std::env::temp_dir().join(format!("idecraft-endpoints-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/main/resources")).unwrap();
        assert_eq!(default_port(&root, "Spring"), 8080);
        assert_eq!(default_port(&root, "FastAPI"), 8000);
        std::fs::write(
            root.join("src/main/resources/application.properties"),
            "spring.application.name=x\nserver.port = 9090\n",
        )
        .unwrap();
        assert_eq!(default_port(&root, "Spring"), 9090);
        let _ = std::fs::remove_dir_all(&root);
    }

    // SPEC-0103/AK-6 (v3, behaviour 6).
    #[test]
    fn mapping_forms_of_spring() {
        let controller = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/items")
public class ItemController {
    @GetMapping(produces = MediaType.APPLICATION_JSON_VALUE, params = "name")
    public List<Item> byName(@RequestParam("name") String name) { return null; }

    @PutMapping(value = {"/{id}", "/update/{id}"})
    public Item update(
            @PathVariable("id") long id,
            @RequestBody Item item) { return item; }

    @PatchMapping(
            value = "/{id}/price",
            consumes = MediaType.APPLICATION_JSON_VALUE)
    public Item reprice(@PathVariable long id) { return null; }

    @RequestMapping(path = "/bulk", method = {RequestMethod.POST, RequestMethod.PUT})
    public void bulk() {}

    @RequestMapping(value = "/head", method = HEAD)
    public void head() {}

    @ResponseStatus(HttpStatus.OK)
    @GetMapping("/health") @ResponseBody public String health() { return "ok"; }
}
"#;
        assert_eq!(
            found("ItemController.java", controller),
            [
                "Spring GET /api/items ItemController.byName @6",
                "Spring PUT /api/items/{id} ItemController.update @9",
                "Spring PUT /api/items/update/{id} ItemController.update @9",
                "Spring PATCH /api/items/{id}/price ItemController.reprice @14",
                "Spring POST|PUT /api/items/bulk ItemController.bulk @19",
                "Spring HEAD /api/items/head ItemController.head @22",
                "Spring GET /api/items/health ItemController.health @26",
            ]
        );
        // Spring's stereotype is no path; both class paths lead every endpoint.
        let stereotype = "import org.springframework.stereotype.Controller;\n@Controller\n@RequestMapping(path = {\"/orders\", \"/api/orders\"})\npublic class OrderController {\n    @PostMapping\n    public Order place(Order order) { return order; }\n}\n";
        assert_eq!(
            found("OrderController.java", stereotype),
            [
                "Spring POST /orders OrderController.place @4",
                "Spring POST /api/orders OrderController.place @4",
            ]
        );
        // An OpenFeign client calls a service: its methods are client rows, below its `path`.
        let feign = "@FeignClient(name = \"stock\", url = \"http://stock\", path = \"/v2\")\npublic interface StockClient {\n    @GetMapping(\"/stock/{id}\")\n    Stock get(@PathVariable long id);\n}\n";
        assert_eq!(
            found("StockClient.java", feign),
            ["Spring GET /v2/stock/{id} StockClient.get (client) @2"]
        );
    }

    // SPEC-0103/AK-8 (v4, behaviour 7).
    #[test]
    fn rows_methods_slashes_and_clients() {
        let controller = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/products")
public class ProductController {
    @RequestMapping(path = "/bulk", method = {RequestMethod.POST, RequestMethod.PUT, RequestMethod.POST})
    public void bulk() {}

    @PostMapping(path = "/", consumes = "application/json")
    public void create() {}

    @RequestMapping("/ping")
    public String ping() { return "pong"; }

    @GetMapping
    public String all() { return ""; }
}
"#;
        let list = endpoints_in(Path::new("ProductController.java"), controller);
        let rows: Vec<String> = list.iter().map(Endpoint::row_text).collect();
        assert_eq!(
            rows,
            [
                "POST|PUT /api/products/bulk ProductController.bulk",
                "POST /api/products/ ProductController.create",
                "/api/products/ping ProductController.ping",
                "GET /api/products ProductController.all",
            ]
        );
        // One row names every method once; a mapping without a method names none.
        assert_eq!(list[0].methods, ["POST", "PUT"]);
        assert!(list[2].methods.is_empty());
        assert!(list.iter().all(|e| e.kind == EndpointKind::Server));
        // The request of a row with several methods uses the first, without a method GET.
        let nowhere = Path::new("/nonexistent");
        assert_eq!(
            request_text(nowhere, &list[0]).as_deref(),
            Some("### ProductController.bulk\nPOST http://localhost:8080/api/products/bulk\n")
        );
        assert_eq!(
            request(nowhere, &list[2]),
            Some((
                "GET".to_owned(),
                "http://localhost:8080/api/products/ping".to_owned()
            ))
        );
        assert_eq!(
            request(nowhere, &list[1]).map(|r| r.1).as_deref(),
            Some("http://localhost:8080/api/products/")
        );
        // The slash stays where the framework tells the two paths apart, not elsewhere.
        let fastapi = "from fastapi import APIRouter\nrouter = APIRouter(prefix=\"/users\")\n\n@router.get(\"/\")\ndef all():\n    ...\n";
        assert_eq!(found("users.py", fastapi), ["FastAPI GET /users/ all @3"]);
        let micronaut = "@Controller(\"/notes\")\npublic class NoteController {\n    @Get(\"/\")\n    public String list() { return \"\"; }\n}\n";
        assert_eq!(
            found("NoteController.java", micronaut),
            ["Micronaut GET /notes NoteController.list @2"]
        );
        let jaxrs = "@Path(\"/books\")\npublic class Books {\n  @GET\n  @Path(\"/\")\n  public String list() { return \"\"; }\n}\n";
        assert_eq!(
            found("Books.java", jaxrs),
            ["JAX-RS GET /books Books.list @2"]
        );
        // Express: `all` takes every method.
        let express = "const app = require('express')();\napp.all('/any', handler);\n";
        assert_eq!(found("server.js", express), ["Express /any @1"]);
        // A client row has no request, and no application path.
        let client = "@Client(id = \"spell\", path = \"/v1\")\npublic interface SpellClient {\n    @Get(\"/check/{word}\")\n    String check(String word);\n}\n";
        let list = endpoints_in(Path::new("SpellClient.java"), client);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, EndpointKind::Client);
        assert_eq!(list[0].path, "/v1/check/{word}");
        assert_eq!(request(nowhere, &list[0]), None);
        assert_eq!(request_text(nowhere, &list[0]), None);
        // As JSON: the kind in lower case and the list of methods.
        let json = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(json["kind"], "client");
        assert_eq!(json["methods"], serde_json::json!(["GET"]));
    }

    // SPEC-0103/AK-6 (v3, behaviour 6).
    #[test]
    fn mapping_forms_of_jaxrs_and_micronaut() {
        // A REST client interface, then a resource of the application in the same file.
        let jaxrs = "@Path(\"/v3.1\")\n@RegisterRestClient(configKey = \"countries\")\npublic interface CountryClient {\n    @GET\n    @Path(\"/name/{name}\")\n    String byName(@PathParam(\"name\") String name);\n}\n\n@Path(\"/own\")\npublic class Own {\n  @GET\n  public String get() { return \"\"; }\n}\n";
        assert_eq!(
            found("CountryClient.java", jaxrs),
            [
                "JAX-RS GET /v3.1/name/{name} CountryClient.byName (client) @3",
                "JAX-RS GET /own Own.get @10"
            ]
        );
        let micronaut = "@Controller(\"/notes\")\npublic class NoteController {\n    @Get(uri = \"/export\", produces = MediaType.TEXT_PLAIN)\n    public String export() { return \"\"; }\n    @Get(produces = MediaType.TEXT_PLAIN, uri = \"/count\")\n    public String count() { return \"0\"; }\n    @Get(uris = {\"/a\", \"/b\"})\n    public String both() { return \"\"; }\n    @Head(\"/{id}\")\n    public void exists(long id) {}\n    @Inject\n    @Client(\"/\")\n    HttpClient http;\n    @Get\n    public String list() { return \"\"; }\n}\n";
        assert_eq!(
            found("NoteController.java", micronaut),
            [
                "Micronaut GET /notes/export NoteController.export @2",
                "Micronaut GET /notes/count NoteController.count @4",
                "Micronaut GET /notes/a NoteController.both @6",
                "Micronaut GET /notes/b NoteController.both @6",
                "Micronaut HEAD /notes/{id} NoteController.exists @8",
                "Micronaut GET /notes NoteController.list @13",
            ]
        );
        let client = "@Client(\"https://spell.example\")\npublic interface SpellClient {\n    @Get(\"/check/{word}\")\n    String check(String word);\n}\n";
        // An address is no path prefix.
        assert_eq!(
            found("SpellClient.java", client),
            ["Micronaut GET /check/{word} SpellClient.check (client) @2"]
        );
    }

    // SPEC-0103/AK-6 (v3, behaviour 6).
    #[test]
    fn application_path_and_quarkus_port() {
        let root =
            std::env::temp_dir().join(format!("idecraft-endpoints-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |file: &str, text: &str| {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let mut files: Vec<PathBuf> = [
            "src/main/java/a/App.java",
            "src/main/java/a/BookResource.java",
            "src/main/java/a/Ping.java",
            "src/main/resources/application.properties",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        write(
            "src/main/java/a/App.java",
            "@ApplicationPath(\"/api\")\npublic class App extends Application {}\n",
        );
        write(
            "src/main/java/a/BookResource.java",
            "@Path(\"/books\")\npublic class BookResource {\n  @GET\n  public String list() { return \"\"; }\n}\n",
        );
        write(
            "src/main/java/a/Ping.java",
            "@RestController\nclass Ping {\n  @GetMapping(\"/ping\")\n  String ping() { return \"\"; }\n}\n",
        );
        write(
            "src/main/resources/application.properties",
            "quarkus.http.port=8081\n",
        );
        // A REST client's paths are another service's: no application path (v4 behaviour 7).
        write(
            "src/main/java/a/Remote.java",
            "@Path(\"/remote\")\n@RegisterRestClient\npublic interface Remote {\n  @GET\n  String get();\n}\n",
        );
        files.push(PathBuf::from("src/main/java/a/Remote.java"));
        let list = endpoints(&root, &files);
        let paths: Vec<&str> = list.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            ["/api/books", "/ping", "/remote"],
            "only served JAX-RS endpoints get the base path"
        );
        assert_eq!(
            request(&root, &list[0]),
            Some((
                "GET".to_owned(),
                "http://localhost:8081/api/books".to_owned()
            ))
        );
        // Two applications: which one serves a resource is not known, the paths stay as written.
        write(
            "src/main/java/a/Admin.java",
            "@ApplicationPath(\"admin\")\npublic class Admin extends Application {}\n",
        );
        files.push(PathBuf::from("src/main/java/a/Admin.java"));
        let paths: Vec<String> = endpoints(&root, &files)
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(paths, ["/books", "/ping", "/remote"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn row(methods: &[&str], path: &str, handler: &str) -> Endpoint {
        Endpoint {
            framework: "Spring",
            kind: EndpointKind::Server,
            methods: methods.iter().map(|m| (*m).to_owned()).collect(),
            path: path.into(),
            handler: handler.into(),
            file: PathBuf::from("src/Api.java"),
            line: 1,
        }
    }

    fn searched(rows: &[Endpoint], query: &str) -> Vec<String> {
        search_endpoints(rows, query, 50)
            .iter()
            .map(|h| h.endpoint.row_text())
            .collect()
    }

    // SPEC-0067/AK-10 (v9, behaviour 20): path segments in any case, `/` in the query, `*`, the
    // handler, several words over path and handler.
    #[test]
    fn search_matches_path_segments_and_handlers() {
        let rows = [
            row(&["POST"], "/api/orders", "OrderApi.create"),
            row(&["GET"], "/api/orders/{id}", "OrderApi.get"),
            row(&["GET"], "/api/users/{id}", "UserApi.find"),
            row(&["GET"], "/health-check", "Health.ping"),
            row(&["GET"], "/users/:id", ""),
        ];
        let orders = [
            "POST /api/orders OrderApi.create",
            "GET /api/orders/{id} OrderApi.get",
        ];
        assert_eq!(searched(&rows, "ORDERS"), orders);
        assert_eq!(searched(&rows, "api/ord"), orders);
        assert_eq!(
            searched(&rows, "users/id"),
            ["GET /users/:id", "GET /api/users/{id} UserApi.find"]
        );
        // A fragment starts at a segment or after `-`; inside a word only after `*`.
        assert_eq!(searched(&rows, "rders"), [""; 0]);
        assert_eq!(searched(&rows, "*rders"), orders);
        assert_eq!(searched(&rows, "check"), ["GET /health-check Health.ping"]);
        // The handler alone.
        let hits = search_endpoints(&rows, "find", 50);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].endpoint.handler, "UserApi.find");
        assert!(hits[0].path_positions.is_empty());
        assert_eq!(hits[0].handler_positions, [8, 9, 10, 11]);
        // The matched characters of the path.
        let hits = search_endpoints(&rows, "health", 50);
        assert_eq!(hits[0].path_positions, [1, 2, 3, 4, 5, 6]);
        // Words in any order, one in the path and one in the handler.
        for query in ["orders create", "create orders"] {
            assert_eq!(
                searched(&rows, query),
                ["POST /api/orders OrderApi.create"],
                "{query}"
            );
        }
        assert_eq!(searched(&rows, "orders find"), [""; 0]);
        assert_eq!(searched(&rows, "zzz"), [""; 0]);
    }

    // SPEC-0067/AK-10 (v9, behaviour 20): a path match before a handler match, the shorter path
    // first.
    #[test]
    fn search_ranks_paths_before_handlers() {
        let rows = [
            row(&["GET"], "/api/users/{id}", "Accounts.get"),
            row(&["GET"], "/api/v1/users", "Accounts.all"),
            row(&["GET"], "/people", "UsersController.all"),
            row(&["GET"], "/users", "Accounts.list"),
            row(&["POST"], "/users", "Accounts.add"),
        ];
        assert_eq!(
            searched(&rows, "users"),
            [
                "GET /users Accounts.list",
                "POST /users Accounts.add",
                "GET /api/users/{id} Accounts.get",
                "GET /api/v1/users Accounts.all",
                "GET /people UsersController.all",
            ]
        );
        // The whole path typed comes first whatever follows it in other paths.
        assert_eq!(
            searched(&rows, "/api/v1/users")[0],
            "GET /api/v1/users Accounts.all"
        );
    }

    // SPEC-0067/AK-10 (v9, behaviour 20): a leading method narrows, in either case; rows without
    // a method stay; a method name without a space is text.
    #[test]
    fn search_narrows_by_a_leading_method() {
        let rows = [
            row(&["POST", "PUT"], "/a/bulk", "A.bulk"),
            row(&["GET"], "/a/items", "A.list"),
            row(&["POST"], "/a/items", "A.add"),
            row(&[], "/a/ping", "A.ping"),
            row(&["GET"], "/get-started", "Docs.start"),
        ];
        assert_eq!(method_prefix("delete  x "), (Some("DELETE"), "x"));
        assert_eq!(method_prefix(" Get "), (Some("GET"), ""));
        assert_eq!(method_prefix("get"), (None, "get"));
        assert_eq!(method_prefix("getter x"), (None, "getter x"));
        assert_eq!(searched(&rows, "GET items"), ["GET /a/items A.list"]);
        assert_eq!(
            searched(&rows, "post "),
            [
                "POST|PUT /a/bulk A.bulk",
                "POST /a/items A.add",
                "/a/ping A.ping"
            ]
        );
        assert_eq!(
            searched(&rows, "PUT a"),
            ["POST|PUT /a/bulk A.bulk", "/a/ping A.ping"]
        );
        for query in ["get", "GET", "get get"] {
            assert_eq!(
                searched(&rows, query),
                ["GET /get-started Docs.start"],
                "{query}"
            );
        }
        assert_eq!(searched(&rows, "TRACE "), ["/a/ping A.ping"]);
        // Without text the rows keep the model's order.
        for query in ["", "   "] {
            assert_eq!(searched(&rows, query).len(), 5, "{query:?}");
            assert_eq!(searched(&rows, query)[0], "POST|PUT /a/bulk A.bulk");
        }
    }

    // SPEC-0067/AK-10 (v9, behaviour 20): a project without endpoints; many endpoints and the
    // limit.
    #[test]
    fn search_of_an_empty_project_and_of_many_endpoints() {
        let dir =
            std::env::temp_dir().join(format!("idecraft-endpoints-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "no routes\n").unwrap();
        let none = project_endpoints(&dir);
        assert!(none.is_empty());
        for query in ["", "users", "GET ", "GET users"] {
            assert!(search_endpoints(&none, query, 50).is_empty(), "{query:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);

        let many: Vec<Endpoint> = (0..600)
            .map(|i| {
                row(
                    if i % 2 == 0 { &["GET"] } else { &["POST"] },
                    &format!("/api/r{i:03}/items"),
                    &format!("R{i:03}.list"),
                )
            })
            .collect();
        assert_eq!(search_endpoints(&many, "items", 50).len(), 50);
        assert_eq!(search_endpoints(&many, "items", usize::MAX).len(), 600);
        assert_eq!(search_endpoints(&many, "POST items", usize::MAX).len(), 300);
        assert_eq!(search_endpoints(&many, "", 50).len(), 50);
        assert_eq!(
            search_endpoints(&many, "", 50)[49].endpoint.path,
            "/api/r049/items"
        );
        assert_eq!(searched(&many, "r599"), ["POST /api/r599/items R599.list"]);
        assert_eq!(search_endpoints(&many, "items", 0).len(), 0);
    }
}
