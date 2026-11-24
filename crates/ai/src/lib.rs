//! Optional AI Assistant for Ferris IDEA (`SPEC-0138`): a chat with an OpenAI-compatible endpoint the
//! user names — a local server (Ollama, LM Studio, llama.cpp) or any provider. Off by default; no
//! provider is built in, and nothing is sent until the user switches it on and names an endpoint.
//! The API key stays in the system keychain and is sent only to that endpoint (no redirects).

pub mod agent_config;
pub mod context;
pub mod live;
pub mod providers;
pub mod subscription;

use std::time::Duration;

use ferris_idea_core::AiSettings;
use ferris_idea_database::{SecretRef, SecretStore};
use ferris_idea_http::{ResolvedRequest, parse_url, send_once, send_streaming};
use serde::{Deserialize, Serialize};

/// Providers with an OpenAI-compatible API the settings offer as templates (`SPEC-0143`): name and
/// base URL. Each bills its API separately from its chat subscription; the user's key decides.
pub const PROVIDERS: &[(&str, &str)] = &[
    ("OpenAI", "https://api.openai.com/v1"),
    ("Anthropic", "https://api.anthropic.com/v1"),
    (
        "Google",
        "https://generativelanguage.googleapis.com/v1beta/openai",
    ),
    ("Local (Ollama)", "http://localhost:11434/v1"),
];

/// The template whose base URL `endpoint` is, if any.
pub fn provider_of(endpoint: &str) -> Option<&'static str> {
    let e = endpoint.trim().trim_end_matches('/');
    PROVIDERS
        .iter()
        .find(|(_, url)| *url == e)
        .map(|(name, _)| *name)
}

/// The model ids the endpoint offers (`GET {endpoint}/models`), sorted; sends nothing else.
pub fn list_models(endpoint: &str, key: Option<&str>) -> Result<Vec<String>, AiError> {
    let url = format!("{}/models", endpoint.trim().trim_end_matches('/'));
    parse_url(&url).map_err(|e| AiError::Endpoint(endpoint.to_owned(), e.to_string()))?;
    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        headers.push(("Authorization".into(), format!("Bearer {k}")));
        // Anthropic's native header, which its compatibility layer accepts too.
        if url.starts_with("https://api.anthropic.com/") {
            headers.push(("x-api-key".into(), k.to_owned()));
            headers.push(("anthropic-version".into(), "2023-06-01".into()));
        }
    }
    let request = ResolvedRequest {
        name: None,
        line: 0,
        method: "GET".into(),
        url,
        headers,
        body: String::new(),
        unresolved: Vec::new(),
    };
    let response = send_once(&request, TIMEOUT).map_err(|e| AiError::Transport(e.to_string()))?;
    let text = response.text.unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    if !(200..300).contains(&response.status) {
        return Err(status_error(response.status, &text, key));
    }
    let mut ids: Vec<String> = json["data"]
        .as_array()
        .or_else(|| json["models"].as_array())
        .map(|list| {
            list.iter()
                .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
                .map(|id| id.trim_start_matches("models/").to_owned())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    Ok(ids)
}

/// Keychain service of API keys; the account is the endpoint's host and port.
pub const KEY_SERVICE: &str = "ferris-idea-ai";

/// How long one answer may take.
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// What the assistant is told before every conversation.
pub const SYSTEM_PROMPT: &str = "You are a programming assistant inside the Ferris IDEA IDE. \
Answer concisely; put code in fenced blocks with the language name.";

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// `system`, `user` or `assistant`.
    pub role: String,
    /// Text.
    pub content: String,
}

impl Message {
    /// A message with `role`.
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_owned(),
            content: content.into(),
        }
    }
}

/// Code attached to a question: where it is from and the text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// File label with lines, e.g. `src/main.rs:10-24`.
    pub label: String,
    /// Language name for the fence, e.g. `rust`.
    pub language: String,
    /// The code, or the masked and budgeted text of another source (`SPEC-0313`).
    pub code: String,
    /// Where it comes from (`SPEC-0313` behaviour 1).
    #[serde(default)]
    pub source: context::Source,
    /// Secrets masked in it (`SPEC-0313` behaviour 6).
    #[serde(default)]
    pub masked: usize,
    /// Whether it was cut to the budget (`SPEC-0313` behaviour 7).
    #[serde(default)]
    pub truncated: bool,
}

/// Why no answer came.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AiError {
    /// The assistant is switched off.
    #[error("the AI Assistant is off; switch it on in Settings › Tools › AI Assistant")]
    Disabled,
    /// No endpoint or model.
    #[error("no endpoint and model set in Settings › Tools › AI Assistant")]
    NotConfigured,
    /// The endpoint is not a usable URL.
    #[error("endpoint `{0}`: {1}")]
    Endpoint(String, String),
    /// Sending failed.
    #[error("{0}")]
    Transport(String),
    /// The endpoint answered with an error.
    #[error("the endpoint answered {status}: {message}")]
    Status {
        /// HTTP status.
        status: u16,
        /// Its error message or the start of the body.
        message: String,
    },
    /// The answer was not understood.
    #[error("unexpected answer: {0}")]
    Answer(String),
    /// The subscription's program failed or is missing (`SPEC-0160`).
    #[error("{0}")]
    Subscription(String),
    /// Claude with the plan is set to run in the Terminal and no Anthropic API key is set up;
    /// nothing was sent (`SPEC-0160` v2 behaviour 6).
    #[error("{} Nothing was sent.", subscription::CLAUDE_PLAN_MESSAGE)]
    ClaudePlan,
}

/// The keychain entry of the endpoint's API key.
pub fn key_ref(endpoint: &str) -> Result<SecretRef, AiError> {
    let t = parse_url(endpoint.trim())
        .map_err(|e| AiError::Endpoint(endpoint.to_owned(), e.to_string()))?;
    Ok(SecretRef {
        service: KEY_SERVICE.into(),
        account: format!("{}:{}", t.host, t.port),
    })
}

/// The API key stored for the endpoint, if any.
pub fn stored_key(store: &dyn SecretStore, endpoint: &str) -> Option<String> {
    let r = key_ref(endpoint).ok()?;
    store.get(&r).ok().flatten().map(|s| s.expose().to_owned())
}

/// The user message for `question` with the attached code in a fence.
pub fn user_message(question: &str, attachment: Option<&Attachment>) -> Message {
    user_message_with(
        question,
        attachment.map(std::slice::from_ref).unwrap_or_default(),
    )
}

/// The user message for `question` with every attachment in a fence under its label, in order
/// (`SPEC-0313` behaviour 1).
pub fn user_message_with(question: &str, attachments: &[Attachment]) -> Message {
    let mut text = String::new();
    for a in attachments {
        let fence = if a.code.contains("```") {
            "````"
        } else {
            "```"
        };
        text.push_str(&format!(
            "{}:\n{fence}{}\n{}\n{fence}\n\n",
            a.label,
            a.language,
            a.code.trim_end()
        ));
    }
    text.push_str(question.trim());
    Message::new("user", text)
}

/// The question "Explain Code" asks.
pub const EXPLAIN: &str = "Explain what this code does, step by step, and point out anything \
that looks wrong.";

/// The JSON body of a chat request: the system prompt, then `messages`.
pub fn request_body(model: &str, messages: &[Message]) -> String {
    body_with(model, SYSTEM_PROMPT, messages, false)
}

fn body_with(model: &str, system: &str, messages: &[Message], stream: bool) -> String {
    let mut all = vec![Message::new("system", system)];
    all.extend(messages.iter().cloned());
    serde_json::json!({"model": model, "messages": all, "stream": stream}).to_string()
}

/// How long a subscription's program may take for a chat answer.
pub const SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(300);

/// Whether the settings choose Claude with the plan while the plan is to be used only in the
/// Terminal (`SPEC-0160` v2 behaviour 6).
pub fn claude_in_terminal(settings: &AiSettings) -> bool {
    subscription::Cli::from_id(&settings.subscription) == Some(subscription::Cli::Claude)
        && settings.claude_plan_mode == ferris_idea_core::ClaudePlanMode::Terminal
}

/// Whether Claude in the chat can go through the user's own Anthropic API key: the endpoint is the
/// Anthropic template, a model is named and a key is stored for it (`SPEC-0160` v2 behaviour 6).
pub fn claude_through_api(settings: &AiSettings, key: Option<&str>) -> bool {
    provider_of(&settings.endpoint) == Some("Anthropic")
        && !settings.model.trim().is_empty()
        && key.is_some_and(|k| !k.trim().is_empty())
}

/// The subscription program the settings choose (`SPEC-0160`), or `None` for the endpoint; refuses
/// when the assistant is off. With Claude's plan in the Terminal the chat uses the Anthropic API
/// key when one is set up and otherwise refuses, sending nothing anywhere (`SPEC-0160` v2
/// behaviour 6: no silent fallback to another endpoint).
pub fn subscribed(
    settings: &AiSettings,
    key: Option<&str>,
) -> Result<Option<subscription::Cli>, AiError> {
    let cli = subscription::Cli::from_id(&settings.subscription);
    if cli.is_some() && !settings.enabled {
        return Err(AiError::Disabled);
    }
    if claude_in_terminal(settings) {
        if claude_through_api(settings, key) {
            return Ok(None);
        }
        return Err(AiError::ClaudePlan);
    }
    Ok(cli)
}

/// Name of the connection while Claude's plan runs in the Terminal and the chat has no Anthropic
/// API template (`SPEC-0160` v2 behaviour 6).
pub const CLAUDE_TERMINAL_NAME: &str = "Claude plan in the Terminal";

/// What the assistant uses, for the settings and the chat: the subscription's name or the
/// endpoint.
pub fn connection_name(settings: &AiSettings) -> String {
    if claude_in_terminal(settings) && provider_of(&settings.endpoint) != Some("Anthropic") {
        return CLAUDE_TERMINAL_NAME.to_owned();
    }
    match subscription::Cli::from_id(&settings.subscription) {
        Some(_) if claude_in_terminal(settings) => settings.endpoint.trim().to_owned(),
        Some(cli) => cli.name().to_owned(),
        None if settings.endpoint.trim().is_empty() => "no endpoint".to_owned(),
        None => settings.endpoint.trim().to_owned(),
    }
}

/// `{endpoint}/chat/completions`.
pub fn completions_url(endpoint: &str) -> String {
    format!("{}/chat/completions", endpoint.trim().trim_end_matches('/'))
}

/// The request for `messages` with the assistant's settings and key; refuses when the assistant is
/// off or has no endpoint and model.
fn chat_request(
    settings: &AiSettings,
    key: Option<&str>,
    body: String,
    stream: bool,
) -> Result<ResolvedRequest, AiError> {
    if !settings.enabled {
        return Err(AiError::Disabled);
    }
    if settings.endpoint.trim().is_empty() || settings.model.trim().is_empty() {
        return Err(AiError::NotConfigured);
    }
    let url = completions_url(&settings.endpoint);
    parse_url(&url).map_err(|e| AiError::Endpoint(settings.endpoint.clone(), e.to_string()))?;
    let accept = if stream {
        "text/event-stream"
    } else {
        "application/json"
    };
    let mut headers = vec![
        ("Content-Type".to_owned(), "application/json".to_owned()),
        ("Accept".to_owned(), accept.to_owned()),
    ];
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        headers.push(("Authorization".into(), format!("Bearer {k}")));
    }
    Ok(ResolvedRequest {
        name: None,
        line: 0,
        method: "POST".into(),
        url,
        headers,
        body,
        unresolved: Vec::new(),
    })
}

fn status_error(status: u16, text: &str, key: Option<&str>) -> AiError {
    let json: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let message = json["error"]["message"]
        .as_str()
        .or_else(|| json["error"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| text.chars().take(200).collect());
    AiError::Status {
        status,
        message: without_key(&message, key),
    }
}

/// `message` of an endpoint with the API key that was sent, and anything else that looks secret,
/// masked (`SPEC-0138` v3 behaviour 2): some endpoints repeat a wrong key in their error, which the
/// chat and the settings page would otherwise show.
fn without_key(message: &str, key: Option<&str>) -> String {
    let mut out = message.to_owned();
    if let Some(k) = key.map(str::trim).filter(|k| k.len() >= 4) {
        out = out.replace(k, context::MASK);
    }
    context::mask_secrets(&out).0
}

fn answer_of(text: &str) -> Result<String, AiError> {
    let json: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| AiError::Answer(text.chars().take(200).collect()))
}

/// Asks the endpoint for the next assistant message after `messages`. Refuses when the assistant
/// is off or has no endpoint and model; `key` goes into `Authorization: Bearer` when given.
pub fn chat(
    settings: &AiSettings,
    key: Option<&str>,
    messages: &[Message],
) -> Result<String, AiError> {
    if let Some(cli) = subscribed(settings, key)? {
        return subscription::ask(
            cli,
            &settings.model,
            SYSTEM_PROMPT,
            messages,
            SUBSCRIPTION_TIMEOUT,
            &mut |_| true,
        );
    }
    let body = request_body(settings.model.trim(), messages);
    let request = chat_request(settings, key, body, false)?;
    let response = send_once(&request, TIMEOUT).map_err(|e| AiError::Transport(e.to_string()))?;
    let text = response.text.unwrap_or_default();
    if !(200..300).contains(&response.status) {
        return Err(status_error(response.status, &text, key));
    }
    answer_of(&text)
}

/// Text pieces of server-sent events: complete `data:` lines of `buffer` are taken out and their
/// `choices[0].delta.content` returned; `done` turns true at `data: [DONE]` (`SPEC-0157`).
pub fn take_events(buffer: &mut String, done: &mut bool) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(end) = buffer.find('\n') {
        let line: String = buffer.drain(..=end).collect();
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            *done = true;
            continue;
        }
        let json: serde_json::Value = serde_json::from_str(data).unwrap_or_default();
        if let Some(piece) = json["choices"][0]["delta"]["content"].as_str()
            && !piece.is_empty()
        {
            out.push(piece.to_owned());
        }
    }
    out
}

/// Like [`chat`], with the answer streamed: `on_piece` gets each piece as the endpoint sends it and
/// stops the answer by returning false. The whole answer so far. An endpoint that answers with
/// plain JSON instead of events gives the answer as one piece (`SPEC-0157` behaviour 1).
pub fn chat_stream(
    settings: &AiSettings,
    key: Option<&str>,
    messages: &[Message],
    on_piece: &mut dyn FnMut(&str) -> bool,
) -> Result<String, AiError> {
    if let Some(cli) = subscribed(settings, key)? {
        return subscription::ask(
            cli,
            &settings.model,
            SYSTEM_PROMPT,
            messages,
            SUBSCRIPTION_TIMEOUT,
            on_piece,
        );
    }
    let body = body_with(settings.model.trim(), SYSTEM_PROMPT, messages, true);
    let request = chat_request(settings, key, body, true)?;
    let mut raw = Vec::new();
    let mut buffer = String::new();
    let mut answer = String::new();
    let mut done = false;
    let mut stopped = false;
    let mut events = None;
    let (status, headers) = send_streaming(&request, TIMEOUT, &mut |bytes: &[u8]| {
        raw.extend_from_slice(bytes);
        let sse = *events.get_or_insert_with(|| {
            String::from_utf8_lossy(&raw)
                .trim_start()
                .starts_with("data:")
                || String::from_utf8_lossy(&raw).trim_start().starts_with(':')
        });
        if !sse {
            return true;
        }
        // Keep incomplete UTF-8 at the end for the next piece.
        let valid = match std::str::from_utf8(&raw) {
            Ok(t) => t.len(),
            Err(e) => e.valid_up_to(),
        };
        buffer.push_str(std::str::from_utf8(&raw[..valid]).unwrap_or_default());
        raw.drain(..valid);
        for piece in take_events(&mut buffer, &mut done) {
            answer.push_str(&piece);
            if !on_piece(&piece) {
                stopped = true;
                return false;
            }
        }
        !done
    })
    .map_err(|e| AiError::Transport(e.to_string()))?;
    let _ = headers;
    if !(200..300).contains(&status) {
        return Err(status_error(status, &String::from_utf8_lossy(&raw), key));
    }
    if events == Some(true) || stopped {
        return Ok(answer);
    }
    let whole = answer_of(&String::from_utf8_lossy(&raw))?;
    on_piece(&whole);
    Ok(whole)
}

/// System prompt of inline completion.
pub const INLINE_PROMPT: &str = "You complete code. Answer with only the text to insert at \
<CURSOR>, without explanations and without repeating the code before or after it. Keep it short: \
the rest of the line or a few lines. Answer with nothing if no completion fits.";

/// Characters of code before and after the caret sent for inline completion.
pub const INLINE_CONTEXT: usize = 4000;

/// Removes Markdown code fences and repeated text before the caret from a completion.
pub fn clean_completion(answer: &str, before: &str) -> String {
    let mut t = answer.trim_end_matches(['\n', ' ']).to_owned();
    if t.trim_start().starts_with("```") {
        let inner: Vec<&str> = t.trim().lines().collect();
        let body = inner
            .iter()
            .skip(1)
            .take(inner.len().saturating_sub(2))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        t = body;
    }
    // Some models repeat the start of the line.
    let line = before.rsplit('\n').next().unwrap_or_default().trim_start();
    if !line.is_empty()
        && let Some(rest) = t.trim_start().strip_prefix(line)
    {
        t = rest.to_owned();
    }
    t
}

/// Asks for a completion at the caret of `language` code (`SPEC-0157` behaviour 3): the last
/// [`INLINE_CONTEXT`] characters before and the first after the caret; the cleaned text to insert
/// (empty when none fits).
pub fn inline_completion(
    settings: &AiSettings,
    key: Option<&str>,
    language: &str,
    before: &str,
    after: &str,
) -> Result<String, AiError> {
    inline_alternative(settings, key, language, before, after, &[])
}

/// Like [`inline_completion`], asking for a completion other than those in `shown` (`SPEC-0162`
/// behaviour 2); empty when the answer repeats one of them.
pub fn inline_alternative(
    settings: &AiSettings,
    key: Option<&str>,
    language: &str,
    before: &str,
    after: &str,
    shown: &[String],
) -> Result<String, AiError> {
    let start = before.chars().count().saturating_sub(INLINE_CONTEXT);
    let before_part: String = before.chars().skip(start).collect();
    let after_part: String = after.chars().take(INLINE_CONTEXT).collect();
    let (before_part, after_part) = masked_around_caret(&before_part, &after_part);
    let mut prompt = format!("```{language}\n{before_part}<CURSOR>{after_part}\n```");
    if !shown.is_empty() {
        prompt.push_str("\n\nGive a different completion than each of these:\n");
        for s in shown {
            prompt.push_str(&format!("```\n{s}\n```\n"));
        }
    }
    let answer = inline_answer(settings, key, &prompt, &before_part)?;
    Ok(if shown.iter().any(|s| s.trim() == answer.trim()) {
        String::new()
    } else {
        answer
    })
}

/// The code before and after the caret with secrets masked like attachments (`SPEC-0157` v2
/// behaviour 3, `SPEC-0313` behaviour 6). A secret on the caret's own line is masked on the whole
/// line, which then ends at the caret, so no part of it is sent on either side.
fn masked_around_caret(before: &str, after: &str) -> (String, String) {
    let (head, line_start) = before
        .rsplit_once('\n')
        .map_or(("", before), |(h, l)| (h, l));
    let (line_end, tail) = after.split_once('\n').map_or((after, ""), |(l, t)| (l, t));
    let line = format!("{line_start}{line_end}");
    let (masked_line, n) = context::mask_secrets(&line);
    let (start, end) = if n == 0 {
        (line_start.to_owned(), line_end.to_owned())
    } else {
        (masked_line, String::new())
    };
    let head = context::mask_secrets(head).0;
    let tail = context::mask_secrets(tail).0;
    let before = if before.contains('\n') {
        format!("{head}\n{start}")
    } else {
        start
    };
    let after = if after.contains('\n') {
        format!("{end}\n{tail}")
    } else {
        end
    };
    (before, after)
}

fn inline_answer(
    settings: &AiSettings,
    key: Option<&str>,
    prompt: &str,
    before_part: &str,
) -> Result<String, AiError> {
    let prompt = prompt.to_owned();
    let before_part = before_part.to_owned();
    if let Some(cli) = subscribed(settings, key)? {
        let answer = subscription::ask(
            cli,
            &settings.model,
            INLINE_PROMPT,
            &[Message::new("user", prompt)],
            Duration::from_secs(30),
            &mut |_| true,
        )?;
        return Ok(clean_completion(&answer, &before_part));
    }
    let body = body_with(
        settings.model.trim(),
        INLINE_PROMPT,
        &[Message::new("user", prompt)],
        false,
    );
    let request = chat_request(settings, key, body, false)?;
    let response = send_once(&request, Duration::from_secs(20))
        .map_err(|e| AiError::Transport(e.to_string()))?;
    let text = response.text.unwrap_or_default();
    if !(200..300).contains(&response.status) {
        return Err(status_error(response.status, &text, key));
    }
    Ok(clean_completion(&answer_of(&text)?, &before_part))
}

/// A fake OpenAI-compatible endpoint on the loopback address for tests: it answers every request
/// with `reply` (or the HTTP `status` given) and records the request bodies and authorization
/// headers.
pub mod fake {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A request the fake server received.
    #[derive(Debug, Clone, Default)]
    pub struct Seen {
        /// Request line, e.g. `POST /v1/chat/completions HTTP/1.1`.
        pub line: String,
        /// `Authorization` header, if sent.
        pub authorization: Option<String>,
        /// Body.
        pub body: String,
    }

    /// The running server.
    pub struct FakeEndpoint {
        /// Base URL, e.g. `http://127.0.0.1:5123/v1`.
        pub endpoint: String,
        /// Requests so far.
        pub seen: Arc<Mutex<Vec<Seen>>>,
    }

    /// Starts a server answering with `reply` and `status`.
    pub fn start(reply: &str, status: u16) -> std::io::Result<FakeEndpoint> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let reply = reply.to_owned();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut s = Seen::default();
                let mut length = 0;
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                s.line = line.trim().to_owned();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                    let (k, v) = h.split_once(':').unwrap_or((&h, ""));
                    if k.eq_ignore_ascii_case("content-length") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    if k.eq_ignore_ascii_case("authorization") {
                        s.authorization = Some(v.trim().to_owned());
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                s.body = String::from_utf8_lossy(&body).into_owned();
                let models = s.line.contains("/models ");
                let stream = s.body.contains("\"stream\":true");
                if let Ok(mut l) = log.lock() {
                    l.push(s);
                }
                if status == 200 && stream {
                    // Server-sent events, one per word, as streaming endpoints send them.
                    let mut stream = reader.into_inner();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                    );
                    for (i, word) in reply.split_inclusive(' ').enumerate() {
                        let event = serde_json::json!({"choices": [{"index": 0, "delta": {"content": word}}]});
                        let _ = write!(stream, "data: {event}\n\n");
                        let _ = stream.flush();
                        if i == 0 {
                            std::thread::sleep(std::time::Duration::from_millis(30));
                        }
                    }
                    let _ = write!(stream, "data: [DONE]\n\n");
                    continue;
                }
                let payload = if status == 200 && models {
                    serde_json::json!({"data": [{"id": "model-b"}, {"id": "model-a"}]})
                } else if status == 200 {
                    serde_json::json!({"choices": [{"message": {"role": "assistant", "content": reply}}]})
                } else {
                    serde_json::json!({"error": {"message": reply}})
                }
                .to_string();
                let mut stream = reader.into_inner();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
            }
        });
        Ok(FakeEndpoint {
            endpoint: format!("http://127.0.0.1:{port}/v1"),
            seen,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0356 AK-11: the API keys' service is one of those the rename takes over (the short
    /// name and `-ai`), so a key saved by an older version is still found.
    #[test]
    fn the_key_service_follows_the_product_name() {
        assert_eq!(
            KEY_SERVICE,
            format!("{}-ai", ferris_idea_database::KEYCHAIN_SERVICE)
        );
    }

    fn on(endpoint: &str) -> AiSettings {
        AiSettings {
            enabled: true,
            endpoint: endpoint.into(),
            model: "m1".into(),
            inline_completion: false,
            subscription: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn streams_the_answer_piece_by_piece() {
        let server = fake::start("It adds two numbers.", 200).unwrap();
        let mut pieces = Vec::new();
        let answer = chat_stream(
            &on(&server.endpoint),
            None,
            &[user_message("hi", None)],
            &mut |p| {
                pieces.push(p.to_owned());
                true
            },
        )
        .unwrap();
        assert_eq!(answer, "It adds two numbers.");
        assert_eq!(pieces, ["It ", "adds ", "two ", "numbers."]);
        assert!(
            server.seen.lock().unwrap()[0]
                .body
                .contains("\"stream\":true")
        );
        // Stopping keeps what came so far.
        let mut n = 0;
        let stopped = chat_stream(&on(&server.endpoint), None, &[], &mut |_| {
            n += 1;
            n < 2
        })
        .unwrap();
        assert_eq!(stopped, "It adds ");
        let mut buf =
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: [DO".to_owned();
        let mut done = false;
        assert_eq!(take_events(&mut buf, &mut done), ["a"]);
        assert!(!done);
        buf.push_str("NE]\n");
        assert!(take_events(&mut buf, &mut done).is_empty() && done);
    }

    #[test]
    fn alternatives_ask_for_something_else_and_drop_repeats() {
        let server = fake::start("b + 1", 200).unwrap();
        let other = inline_alternative(
            &on(&server.endpoint),
            None,
            "rust",
            "let x = foo(",
            ");",
            &["a".to_owned()],
        )
        .unwrap();
        assert_eq!(other, "b + 1");
        let body = server.seen.lock().unwrap()[0].body.clone();
        assert!(body.contains("different completion"), "{body}");
        let same = inline_alternative(
            &on(&server.endpoint),
            None,
            "rust",
            "let x = foo(",
            ");",
            &["b + 1".to_owned()],
        )
        .unwrap();
        assert_eq!(same, "", "a repeated suggestion is no alternative");
    }

    #[test]
    fn inline_completion_sends_the_code_around_the_caret() {
        let server = fake::start("```rust\nlet y = x + 1;\n```", 200).unwrap();
        let got = inline_completion(
            &on(&server.endpoint),
            None,
            "rust",
            "fn a(x: i32) {\n    ",
            "\n}",
        )
        .unwrap();
        assert_eq!(got, "let y = x + 1;");
        let body = server.seen.lock().unwrap()[0].body.clone();
        assert!(body.contains("<CURSOR>") && body.contains("You complete code"));
        assert_eq!(clean_completion("    let a = 1", "    let a"), " = 1");
    }

    #[test]
    fn inline_completion_masks_secrets_around_the_caret() {
        // Obviously fake tokens of published shapes (no credentials).
        let server = fake::start("x", 200).unwrap();
        inline_completion(
            &on(&server.endpoint),
            None,
            "python",
            "TOKEN = \"ghp_FAKEFAKEFAKEFAKEFAKEFAKE0000\"\nx = ",
            "\nAPI_KEY=abcdefabcdef\n",
        )
        .unwrap();
        let body = server.seen.lock().unwrap()[0].body.clone();
        assert!(
            !body.contains("FAKEFAKE") && !body.contains("abcdefabcdef"),
            "{body}"
        );
        assert!(body.contains("x = <CURSOR>"), "{body}");
        // A secret on the caret's line is masked whole; nothing of it follows the caret.
        let (b, a) = masked_around_caret("a\nAPI_TOKEN=abc", "def123\nb");
        assert_eq!((b.as_str(), a.as_str()), ("a\nAPI_TOKEN=‹masked›", "\nb"));
        let (b, a) = masked_around_caret("let x = ", ";");
        assert_eq!((b.as_str(), a.as_str()), ("let x = ", ";"));
    }

    #[test]
    fn an_error_never_repeats_the_key() {
        let failing = fake::start("Incorrect API key provided: sk-wrong-0123456789", 401).unwrap();
        let e = chat(&on(&failing.endpoint), Some("sk-wrong-0123456789"), &[]).unwrap_err();
        assert_eq!(
            e,
            AiError::Status {
                status: 401,
                message: "Incorrect API key provided: ‹masked›".into()
            }
        );
        let e = list_models(&failing.endpoint, Some("sk-wrong-0123456789")).unwrap_err();
        assert!(!e.to_string().contains("0123456789"), "{e}");
    }

    /// `SPEC-0160` v2 AK-7: with Claude's plan in the Terminal and no Anthropic API key nothing is
    /// sent — no program runs, the endpoint beside it is not asked (no silent fallback) — and every
    /// way of asking gets the message; with the Anthropic template, a model and a key the chat goes
    /// through the API; in Chat mode (the default) Claude Code stays the subscription.
    #[test]
    fn claude_plan_in_the_terminal_sends_nothing_without_an_api_key() {
        let server = fake::start("from the endpoint", 200).unwrap();
        let mut s = on(&server.endpoint);
        s.subscription = "claude".into();
        assert_eq!(
            s.claude_plan_mode,
            ferris_idea_core::ClaudePlanMode::Chat,
            "the default"
        );
        assert_eq!(subscribed(&s, None), Ok(Some(subscription::Cli::Claude)));
        s.claude_plan_mode = ferris_idea_core::ClaudePlanMode::Terminal;
        let question = [Message::new("user", "hi")];
        assert_eq!(chat(&s, Some("k"), &question), Err(AiError::ClaudePlan));
        let mut pieces = 0;
        assert_eq!(
            chat_stream(&s, Some("k"), &question, &mut |_| {
                pieces += 1;
                true
            }),
            Err(AiError::ClaudePlan)
        );
        assert_eq!(pieces, 0);
        assert_eq!(
            inline_completion(&s, Some("k"), "rust", "let a = ", ";"),
            Err(AiError::ClaudePlan)
        );
        assert!(server.seen.lock().unwrap().is_empty(), "nothing was sent");
        let message = AiError::ClaudePlan.to_string();
        assert!(
            message.starts_with(subscription::CLAUDE_PLAN_MESSAGE)
                && message.ends_with("Nothing was sent.")
        );
        assert_eq!(connection_name(&s), CLAUDE_TERMINAL_NAME);
        // The Anthropic template with a model and a key: the chat uses the API, not the plan.
        s.endpoint = "https://api.anthropic.com/v1".into();
        assert_eq!(subscribed(&s, None), Err(AiError::ClaudePlan), "no key yet");
        assert_eq!(subscribed(&s, Some("k")), Ok(None));
        assert_eq!(connection_name(&s), "https://api.anthropic.com/v1");
        // Off stays off first.
        s.enabled = false;
        assert_eq!(chat(&s, None, &question), Err(AiError::Disabled));
    }

    #[test]
    fn refuses_until_switched_on_and_configured() {
        let mut s = AiSettings::default();
        assert_eq!(chat(&s, None, &[]), Err(AiError::Disabled));
        s.enabled = true;
        assert_eq!(chat(&s, None, &[]), Err(AiError::NotConfigured));
        s.endpoint = "ftp://x".into();
        s.model = "m".into();
        assert!(matches!(chat(&s, None, &[]), Err(AiError::Endpoint(..))));
    }

    #[test]
    fn messages_and_body() {
        let a = Attachment {
            label: "src/a.rs:1-2".into(),
            language: "rust".into(),
            code: "fn a() {}\n".into(),
            ..Default::default()
        };
        let m = user_message(" What is it? ", Some(&a));
        assert_eq!(
            m.content,
            "src/a.rs:1-2:\n```rust\nfn a() {}\n```\n\nWhat is it?"
        );
        let body: serde_json::Value = serde_json::from_str(&request_body("m1", &[m])).unwrap();
        assert_eq!(body["model"], "m1");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(
            completions_url("http://h/v1/"),
            "http://h/v1/chat/completions"
        );
        assert_eq!(
            key_ref("http://localhost:11434/v1").unwrap().account,
            "localhost:11434"
        );
    }

    #[test]
    fn chats_with_a_compatible_endpoint() {
        let server = fake::start("It adds.", 200).unwrap();
        let answer = chat(
            &on(&server.endpoint),
            Some("sk-test"),
            &[user_message("hi", None)],
        )
        .unwrap();
        assert_eq!(answer, "It adds.");
        let seen = server.seen.lock().unwrap().clone();
        assert_eq!(seen[0].line, "POST /v1/chat/completions HTTP/1.1");
        assert_eq!(seen[0].authorization.as_deref(), Some("Bearer sk-test"));
        assert!(seen[0].body.contains("\"model\":\"m1\""));
        // Without a key no Authorization header is sent.
        chat(&on(&server.endpoint), None, &[user_message("hi", None)]).unwrap();
        assert_eq!(server.seen.lock().unwrap()[1].authorization, None);
        assert_eq!(
            list_models(&server.endpoint, Some("sk-test")).unwrap(),
            ["model-a", "model-b"]
        );
        assert_eq!(
            provider_of("https://api.anthropic.com/v1/"),
            Some("Anthropic")
        );
        assert_eq!(provider_of(&server.endpoint), None);
        let failing = fake::start("model not found", 404).unwrap();
        assert_eq!(
            chat(&on(&failing.endpoint), None, &[]),
            Err(AiError::Status {
                status: 404,
                message: "model not found".into()
            })
        );
    }
}
