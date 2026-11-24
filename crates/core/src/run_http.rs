//! Runs of HTTP request configurations (`SPEC-0047` v6 behaviour 17, `SPEC-0087` v11 behaviour
//! 19): the requests of a `.http` file are sent with Ferris IDEA's HTTP client (`SPEC-0046`) in the
//! environment of `SPEC-0102`, and the responses become the run's output. Nothing is written into
//! the project (IMP-0506); private environment values appear in the output only as `{{name}}`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use ferris_idea_http::{EnvironmentChoice, SendOptions};

use crate::run::{HttpRun, RunConfig, RunEvent, RunOutcome};

/// The most body text a run writes per response.
const MAX_BODY_TEXT: usize = 1024 * 1024;

/// The run's output: lines go to the outcome and, when streaming, out as events.
struct Out<'a> {
    events: Option<&'a Sender<RunEvent>>,
    private: BTreeMap<String, String>,
}

impl Out<'_> {
    fn line(&self, outcome: &mut RunOutcome, stderr: bool, text: &str) {
        // No private value and no user information of a URL reaches the output (behaviour 17).
        let text = ferris_idea_http::mask_for_log(text, &self.private);
        for line in text.split('\n') {
            let line = line.trim_end_matches('\r');
            let all = if stderr {
                &mut outcome.stderr
            } else {
                &mut outcome.stdout
            };
            all.push_str(line);
            all.push('\n');
            if let Some(tx) = self.events {
                let _ = tx.send(RunEvent::Output {
                    stderr,
                    text: line.to_owned(),
                });
            }
        }
    }
}

/// The request file of `h` below `root`, no symbolic link leading outside.
fn request_file(root: &Path, h: &HttpRun) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("the project folder: {e}"))?;
    let file = root
        .join(&h.file)
        .canonicalize()
        .map_err(|e| format!("the request file {}: {e}", h.file))?;
    if !file.starts_with(&root) {
        return Err(format!(
            "the request file {} lies outside the project",
            h.file
        ));
    }
    if !file.is_file() {
        return Err(format!("the request file {} is no file", h.file));
    }
    Ok(file)
}

/// Sends the requests of `h` for configuration `c` and fills `outcome` (behaviour 17).
pub(crate) fn run(
    root: &Path,
    c: &RunConfig,
    h: &HttpRun,
    stop: &AtomicBool,
    events: Option<&Sender<RunEvent>>,
    outcome: &mut RunOutcome,
) {
    let refuse = |outcome: &mut RunOutcome, why: String| {
        outcome.start_error = Some(format!("{}: {why}", c.name));
    };
    let file = match request_file(root, h) {
        Ok(f) => f,
        Err(why) => return refuse(outcome, why),
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => return refuse(outcome, format!("the request file {}: {e}", h.file)),
    };
    let parsed = ferris_idea_http::parse(&text);
    // Which requests: by name, by number counted from 1, or all.
    let chosen: Vec<usize> = match h.request.as_deref().map(str::trim) {
        None => (0..parsed.requests.len()).collect(),
        Some(r) => {
            let by_name = parsed
                .requests
                .iter()
                .position(|q| q.name.as_deref().map(str::trim) == Some(r));
            let by_number = r
                .parse::<usize>()
                .ok()
                .filter(|n| *n >= 1 && *n <= parsed.requests.len())
                .map(|n| n - 1);
            match by_name.or(by_number) {
                Some(i) => vec![i],
                None => {
                    return refuse(outcome, format!("{} has no request {r}", h.file));
                }
            }
        }
    };
    if chosen.is_empty() {
        return refuse(outcome, format!("{} holds no request", h.file));
    }
    // The environment (SPEC-0102 v5 behaviour 4): the named one, else the only one, else none.
    let dir = file.parent().unwrap_or(root);
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let envs = ferris_idea_http::load_environments(dir, &canonical_root);
    if let Some(name) = h.environment.as_deref()
        && !envs.names.iter().any(|n| n == name)
    {
        let defined = if envs.names.is_empty() {
            "none".to_owned()
        } else {
            envs.names.join(", ")
        };
        return refuse(
            outcome,
            format!("no HTTP environment {name}; the environment files define: {defined}"),
        );
    }
    let bare = ferris_idea_http::resolve(&parsed, &BTreeMap::new());
    let needed: Vec<String> = chosen
        .iter()
        .flat_map(|i| bare[*i].unresolved.iter().cloned())
        .collect();
    let environment = match envs.choose(h.environment.as_deref(), &needed) {
        EnvironmentChoice::Use(name) => Some(name),
        EnvironmentChoice::Without => None,
        choice @ EnvironmentChoice::Choose(_) => {
            return refuse(outcome, choice.refusal().unwrap_or_default());
        }
    };
    let (vars, private) = match environment.as_deref() {
        Some(name) => (envs.get(name).unwrap_or_default(), envs.private(name)),
        None => (BTreeMap::new(), BTreeMap::new()),
    };
    let out = Out { events, private };
    if let Some(tx) = events {
        let _ = tx.send(RunEvent::Started {
            name: c.name.clone(),
            command: h.command_line(),
        });
    }
    if let Some(name) = environment.as_deref() {
        out.line(outcome, false, &format!("HTTP environment: {name}"));
    }
    let resolved = ferris_idea_http::resolve(&parsed, &vars);
    let mut all_answered = true;
    for (n, i) in chosen.iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            outcome.stopped = true;
            break;
        }
        if n > 0 {
            out.line(outcome, false, "");
        }
        let raw = &parsed.requests[*i];
        let title = raw
            .name
            .clone()
            .unwrap_or_else(|| format!("Request {}", i + 1));
        out.line(outcome, false, &format!("### {title}"));
        let mut request = resolved[*i].clone();
        if !request.unresolved.is_empty() {
            out.line(
                outcome,
                true,
                &format!(
                    "Not sent: unresolved variables: {}",
                    request.unresolved.join(", ")
                ),
            );
            all_answered = false;
            continue;
        }
        if let Err(why) = ferris_idea_http::finish_body(&mut request, dir, &canonical_root) {
            out.line(outcome, true, &format!("Not sent: {why}"));
            all_answered = false;
            continue;
        }
        out.line(
            outcome,
            false,
            &format!("{} {}", request.method, request.url),
        );
        for note in &raw.notes {
            out.line(outcome, false, &format!("Note: {note}"));
        }
        let options = SendOptions {
            timeout: raw.settings.timeout_secs.map_or(
                ferris_idea_http::DEFAULT_TIMEOUT,
                std::time::Duration::from_secs,
            ),
            follow_redirects: !raw.settings.no_redirect,
            accept_codings: true,
        };
        let response = match ferris_idea_http::send_with(&request, &options) {
            Ok(r) => r,
            Err(e) => {
                out.line(outcome, true, &e.to_string());
                all_answered = false;
                continue;
            }
        };
        let class = match response.status {
            400..=499 => " · client error",
            500..=599 => " · server error",
            _ => "",
        };
        out.line(
            outcome,
            false,
            &format!(
                "HTTP {} {} · {} ms · {} bytes{class}",
                response.status, response.reason, response.millis, response.size
            ),
        );
        for redirect in &response.redirects {
            out.line(outcome, false, &format!("Redirected through {redirect}"));
        }
        for (k, v) in &response.headers {
            out.line(outcome, false, &format!("{k}: {v}"));
        }
        for note in &response.notes {
            out.line(outcome, false, &format!("Note: {note}"));
        }
        match &response.text {
            Some(text) if !text.is_empty() => {
                out.line(outcome, false, "");
                let json = response
                    .header("content-type")
                    .is_some_and(|t| t.to_ascii_lowercase().contains("json"));
                let shown = json
                    .then(|| serde_json::from_str::<serde_json::Value>(text).ok())
                    .flatten()
                    .and_then(|v| serde_json::to_string_pretty(&v).ok())
                    .unwrap_or_else(|| text.clone());
                if shown.len() > MAX_BODY_TEXT {
                    let mut end = MAX_BODY_TEXT;
                    while !shown.is_char_boundary(end) {
                        end -= 1;
                    }
                    out.line(outcome, false, &shown[..end]);
                    out.line(
                        outcome,
                        false,
                        &format!(
                            "Note: the body is cut at 1 MiB here; it has {} bytes",
                            shown.len()
                        ),
                    );
                } else {
                    out.line(outcome, false, &shown);
                }
            }
            Some(_) => {}
            None => out.line(
                outcome,
                false,
                &format!("(a body that is no text, {} bytes)", response.size),
            ),
        }
    }
    outcome.exit_code = (!outcome.stopped).then_some(if all_answered { 0 } else { 1 });
}
