//! Quick fixes and refactorings through the language server (`SPEC-0041` v2), the refactoring
//! shortcuts' places and messages (`SPEC-0095` v2).

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::insight::{Opened, Range};
use crate::lsp::LspError;
use crate::rename::{FileEdits, file_edits};

/// One action the server offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeAction {
    /// Title shown to the user.
    pub title: String,
    /// Kind, e.g. `quickfix` or `refactor.extract`.
    pub kind: Option<String>,
    /// Whether the server prefers it (e.g. the obvious fix).
    pub preferred: bool,
    /// Why it cannot be applied now, if disabled.
    pub disabled: Option<String>,
    /// Messages of the diagnostics it fixes.
    pub fixes: Vec<String>,
}

/// Opens `file` and collects the diagnostics the server publishes for it, waiting at most `wait`.
async fn open_with_diagnostics(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    wait: Duration,
) -> Result<(Opened, Vec<Value>), LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let diagnostics = o.diagnostics(wait.min(Duration::from_secs(10))).await;
    Ok((o, diagnostics))
}

fn overlaps(d: &Value, start: (u64, u64), end: (u64, u64)) -> bool {
    let pos = |p: &str| {
        (
            d.pointer(&format!("/range/{p}/line"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            d.pointer(&format!("/range/{p}/character"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
    };
    pos("start") <= end && pos("end") >= start
}

/// `CodeActionTriggerKind.Invoked` of LSP 3.17: the user or an agent asked for the actions.
pub const TRIGGER_INVOKED: u64 = 1;

/// The actions an agent is shown (`SPEC-0041` v3 behaviour 6): those that can be applied first,
/// then the disabled ones with their reason, each part in the server's order; with
/// `applicable_only` the disabled ones are left out. A title is listed once, as in the editor's
/// list (`SPEC-0041` v4 behaviour 7): `apply_code_action` chooses by title and takes the first.
pub fn applicable_first(actions: Vec<CodeAction>, applicable_only: bool) -> Vec<CodeAction> {
    let (mut applicable, disabled): (Vec<_>, Vec<_>) =
        actions.into_iter().partition(|a| a.disabled.is_none());
    if !applicable_only {
        applicable.extend(disabled);
    }
    let mut seen = std::collections::HashSet::new();
    applicable.retain(|a| seen.insert(a.title.clone()));
    applicable
}

/// Asks for the actions of `range` (characters converted to the server's units).
/// With `only` non-empty the server is asked for just those kinds (`context.only`, SPEC-0095 v2).
pub(crate) async fn request_actions(
    o: &mut Opened,
    range: &Range,
    diagnostics: &[Value],
    only: &[&str],
) -> Result<Vec<Value>, LspError> {
    let start = o.position(range.start_line, range.start_character)["position"].clone();
    let end = o.position(range.end_line, range.end_character)["position"].clone();
    let units = |v: &Value| {
        (
            v["line"].as_u64().unwrap_or(0),
            v["character"].as_u64().unwrap_or(0),
        )
    };
    let mut context: Vec<&Value> = diagnostics
        .iter()
        .filter(|d| overlaps(d, units(&start), units(&end)))
        .collect();
    let (mut start, mut end) = (start, end);
    // A caret outside every problem still gets the fixes of the problems on its line, and the
    // request covers them (`SPEC-0076` v2 behaviour 1).
    if context.is_empty() && start == end {
        let line = units(&start).0;
        context = diagnostics
            .iter()
            .filter(|d| overlaps(d, (line, 0), (line, u64::MAX)))
            .collect();
        if let Some(first) = context.first() {
            start = first["range"]["start"].clone();
            end = first["range"]["end"].clone();
            for d in &context[1..] {
                if units(&d["range"]["start"]) < units(&start) {
                    start = d["range"]["start"].clone();
                }
                if units(&d["range"]["end"]) > units(&end) {
                    end = d["range"]["end"].clone();
                }
            }
        }
    }
    // Every request here answers an explicit call (⌥↩, a refactoring shortcut, an MCP tool), so it
    // says so: typescript-language-server offers some refactorings only then, e.g. Inline variable
    // with the caret on a use (`SPEC-0041` v3 behaviour 2).
    let mut context = json!({ "diagnostics": context, "triggerKind": TRIGGER_INVOKED });
    if !only.is_empty() {
        context["only"] = json!(only);
    }
    let answer = o
        .ask(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": o.uri},
                "range": {"start": start, "end": end},
                "context": context,
            }),
        )
        .await?;
    Ok(answer.as_array().cloned().unwrap_or_default())
}

fn describe(a: &Value) -> CodeAction {
    CodeAction {
        title: a["title"].as_str().unwrap_or_default().to_owned(),
        kind: a["kind"].as_str().map(str::to_owned),
        preferred: a["isPreferred"].as_bool().unwrap_or(false),
        disabled: a["disabled"]["reason"].as_str().map(str::to_owned),
        fixes: a["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["message"].as_str().map(str::to_owned))
            .collect(),
    }
}

/// Whether `action` must be resolved before it can be applied: it carries neither an edit nor a
/// command and the server resolves actions (`SPEC-0041` v2 behaviour 3). A bare `Command` never is.
pub fn needs_resolve(action: &Value, capabilities: &Value) -> bool {
    !action["command"].is_string()
        && action["edit"].is_null()
        && action["command"].is_null()
        && capabilities["codeActionProvider"]["resolveProvider"] == true
}

/// Actions for `range` of `file` (behaviour 2).
pub async fn code_actions(
    root: &Path,
    file: &Path,
    range: &Range,
    wait: Duration,
) -> Result<Vec<CodeAction>, LspError> {
    code_actions_in(root, file, None, range, wait).await
}

/// Actions for `range` of `file` whose content is `text` (the editor's unsaved text), or the disk
/// content (`SPEC-0076`).
pub async fn code_actions_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    wait: Duration,
) -> Result<Vec<CodeAction>, LspError> {
    let (mut o, diagnostics) = open_with_diagnostics(root, file, text, wait).await?;
    let result = request_actions(&mut o, range, &diagnostics, &[])
        .await
        .map(|list| {
            let actions: Vec<CodeAction> = list.iter().map(describe).collect();
            // Java: IDECraft's own fixes where jdtls has none (`SPEC-0076` v7 behaviours 11, 12).
            let offered: Vec<String> = actions.iter().map(|a| a.title.clone()).collect();
            let own = crate::java_quick_fixes::own_fixes(
                file,
                &o.text,
                o.files.encoding,
                range,
                &diagnostics,
                &offered,
            );
            crate::java_quick_fixes::merged(actions, own)
        });
    o.close().await;
    result
}

/// A refactoring asked for by its own shortcut (`SPEC-0095`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Refactoring {
    /// `IntroduceVariable` (⌥⌘V).
    Variable,
    /// `IntroduceConstant` (⌥⌘C).
    Constant,
    /// `IntroduceField` (⌥⌘F, `SPEC-0320` behaviour 11).
    Field,
    /// `ExtractMethod` (⌥⌘M).
    Method,
    /// `IntroduceParameter` (⌥⌘P, `SPEC-0095` v11 behaviour 14).
    Parameter,
    /// `Inline` (⌥⌘N).
    Inline,
    /// `Refactorings.QuickListPopupAction` (⌃T): every refactoring.
    Any,
}

impl Refactoring {
    /// The refactoring of keymap action `id`.
    pub fn of_action(id: &str) -> Option<Self> {
        Some(match id {
            "IntroduceVariable" => Self::Variable,
            "IntroduceConstant" => Self::Constant,
            "IntroduceField" => Self::Field,
            "ExtractMethod" => Self::Method,
            "IntroduceParameter" => Self::Parameter,
            "Inline" => Self::Inline,
            "Refactorings.QuickListPopupAction" => Self::Any,
            _ => return None,
        })
    }

    /// What the status bar calls it when the server offers none.
    pub fn name(self) -> &'static str {
        match self {
            Self::Variable => "Extract Variable",
            Self::Constant => "Extract Constant",
            Self::Field => "Extract Field",
            Self::Method => "Extract Method",
            Self::Parameter => "Introduce Parameter",
            Self::Inline => "Inline",
            Self::Any => "Refactoring",
        }
    }

    /// Whether code action `a` is this refactoring: a `refactor` kind (or none) and, except for
    /// [`Refactoring::Any`], the matching words in the title, since servers name the kinds
    /// differently (rust-analyzer `refactor.extract`, clangd `refactor`).
    pub fn matches(self, a: &CodeAction) -> bool {
        let kind = a.kind.as_deref().unwrap_or("refactor");
        if !kind.starts_with("refactor") || a.disabled.is_some() {
            return false;
        }
        let title = a.title.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| title.contains(w));
        match self {
            Self::Any => true,
            Self::Inline => kind.starts_with("refactor.inline") || title.starts_with("inline"),
            // TypeScript has no "variable": its constant in the enclosing scope is the local
            // variable before the statement (`SPEC-0095` v6 behaviour 2).
            Self::Variable => {
                has(&["extract"])
                    && (has(&["variable", "local"])
                        || (kind == "refactor.extract.constant" && has(&["enclosing scope"])))
            }
            Self::Constant => has(&["extract", "introduce"]) && has(&["constant"]),
            Self::Field => has(&["extract", "introduce"]) && has(&["field"]),
            Self::Method => has(&["extract"]) && has(&["function", "method"]),
            Self::Parameter => has(&["introduce", "extract", "promote"]) && has(&["parameter"]),
        }
    }
}

impl Refactoring {
    /// The refactoring named `name` in tools: `extract_variable`, `extract_constant`,
    /// `extract_method`, `inline` or `any`.
    pub fn of_name(name: &str) -> Option<Self> {
        Some(match name {
            "extract_variable" => Self::Variable,
            "extract_constant" => Self::Constant,
            "extract_field" => Self::Field,
            "extract_method" => Self::Method,
            "introduce_parameter" => Self::Parameter,
            "inline" => Self::Inline,
            "any" => Self::Any,
            _ => return None,
        })
    }

    /// Whether a bare caret is widened to the enclosing expressions and statements when the server
    /// offers nothing at the caret itself: extracting needs a selection (`SPEC-0095` v2 behaviour 6).
    pub fn widens(self) -> bool {
        matches!(
            self,
            Self::Variable | Self::Constant | Self::Field | Self::Method | Self::Parameter
        )
    }

    /// What the status bar says when the language server `server` offers this refactoring nowhere
    /// asked: for the selected `selected` text, or at a bare caret (`SPEC-0095` v2 behaviour 7).
    pub fn none_message(self, server: &str, selected: Option<&str>) -> String {
        let what = match selected {
            Some(t) => {
                let t = t.trim();
                let first = t.lines().next().unwrap_or_default();
                let short: String = first.chars().take(40).collect();
                if short.len() < t.len() {
                    format!("`{short}…`")
                } else {
                    format!("`{short}`")
                }
            }
            None => "anything at the caret".into(),
        };
        let (doing, hint) = match self {
            Self::Variable => (
                format!("extract {what} into a variable"),
                "Select part of an expression; a whole statement, or a call that already is a \
                 variable's whole value, is not extracted.",
            ),
            Self::Constant => (
                format!("extract {what} into a constant"),
                "Select a constant expression.",
            ),
            Self::Field => (
                format!("extract {what} into a field"),
                "Select an expression inside a class that needs no local value.",
            ),
            Self::Method => (
                format!("extract {what} into a function"),
                "Select one or more complete statements or an expression.",
            ),
            Self::Parameter => (
                format!("introduce {what} as a parameter"),
                "Select an expression inside a method or function.",
            ),
            Self::Inline => (
                format!("inline {what}"),
                "Put the caret on the name of a local variable or a function; not every server \
                 can inline (clangd cannot).",
            ),
            Self::Any => (
                format!("refactor {what}"),
                "Select an expression or put the caret on a name.",
            ),
        };
        format!("{server} cannot {doing}. {hint}")
    }
}

impl Refactoring {
    /// What the status bar says while the language server `server` is still starting or loading
    /// the project and this refactoring waits to be asked again (`SPEC-0095` v10 behaviour 12).
    pub fn repeating_message(self, server: &str) -> String {
        let name = match self {
            Self::Any => "Refactor This",
            other => other.name(),
        };
        format!("{server} is still loading the project; {name} is asked again when it is ready.")
    }
}

/// What the status bar says in place of "nothing to offer" while the language server `server` is
/// still starting or loading the project (`SPEC-0095` v10 behaviour 12).
pub fn loading_message(server: &str) -> String {
    format!("{server} is still loading the project; try again when it is ready.")
}

/// A place a refactoring can act on and the matching actions the server offers there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefactoringTarget {
    /// The range the actions are for (characters).
    pub range: Range,
    /// The text of the range.
    pub text: String,
    /// The matching, enabled actions.
    pub actions: Vec<CodeAction>,
}

/// How many enclosing ranges of a bare caret are tried at most.
const MAX_WIDENING: usize = 8;

/// The text of `range` (characters) in `text`.
pub fn text_of(text: &str, range: &Range) -> String {
    let offset = |line: u64, character: u64| {
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
    };
    let (a, b) = (
        offset(range.start_line, range.start_character),
        offset(range.end_line, range.end_character),
    );
    text.get(a.min(b)..b.max(a)).unwrap_or_default().to_owned()
}

/// The enclosing ranges (with their text, inside out) worth offering for `refactoring`: for a variable
/// or constant the expressions up to the statement, for a function also the first whole statement.
/// Pieces that are no expression of their own (`x * 3, `, an argument list `(a, b)`, `s.area` of the
/// call `s.area()`) and repeats are left out; at most [`MAX_WIDENING`] (`SPEC-0095` v2 behaviour 6).
pub fn widened(refactoring: Refactoring, enclosing: Vec<(Range, String)>) -> Vec<(Range, String)> {
    let texts: Vec<String> = enclosing.iter().map(|(_, t)| t.trim().to_owned()).collect();
    let mut out: Vec<(Range, String)> = Vec::new();
    for (i, (r, _)) in enclosing.into_iter().enumerate() {
        let t = &texts[i];
        let statement = t.ends_with(';') || t.ends_with('}');
        let piece = t.is_empty()
            || t.ends_with(',')
            || (t.starts_with('(') && t.contains(','))
            || texts
                .get(i + 1)
                .and_then(|next| next.strip_prefix(t.as_str()))
                .is_some_and(|rest| rest.starts_with('('));
        let method = refactoring == Refactoring::Method;
        if t.starts_with('{') || (!method && (statement || r.start_line != r.end_line)) {
            break;
        }
        if !piece && out.iter().all(|(_, o)| o != t) {
            out.push((r, t.clone()));
        }
        if statement || out.len() == MAX_WIDENING {
            break;
        }
    }
    out
}

/// The ranges enclosing the caret from inside out, as the server's `textDocument/selectionRange`
/// gives them; empty ranges and repeats are left out (`SPEC-0095` v2 behaviour 6).
async fn enclosing_ranges(o: &mut Opened, line: u64, character: u64) -> Vec<Range> {
    let position = o.position(line, character)["position"].clone();
    let Ok(answer) = o
        .ask(
            "textDocument/selectionRange",
            json!({"textDocument": {"uri": o.uri}, "positions": [position]}),
        )
        .await
    else {
        return Vec::new();
    };
    let uri = o.uri.clone();
    let mut out: Vec<Range> = Vec::new();
    let mut next = answer.get(0);
    while let Some(node) = next {
        if let Some(r) = crate::insight::parse_range(&node["range"], &uri, &mut o.files)
            && (r.start_line, r.start_character) != (r.end_line, r.end_character)
            && out.last() != Some(&r)
        {
            out.push(r);
        }
        next = node.get("parent");
    }
    out
}

/// Where `refactoring` can act for `range` of `file` whose content is `text` (the editor's unsaved
/// text) or the disk content: the selection itself, or for a bare caret the caret and, when that
/// offers nothing and the refactoring [widens](Refactoring::widens), the enclosing expressions and
/// statements from inside out. Only `refactor` kinds are asked for (`SPEC-0095` v2).
///
/// In a Python file Extract Variable, Extract Method, Extract Constant, Inline and Refactor This
/// fall back to IDECraft's own actions when the server offers none there or cannot be asked
/// (`SPEC-0330` behaviours 1, 17 and 22); what they refuse is said by [`own_refusal`].
pub async fn refactoring_targets(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    refactoring: Refactoring,
    wait: Duration,
) -> Result<Vec<RefactoringTarget>, LspError> {
    targets(root, file, text, range, refactoring, wait, None).await
}

/// The program name of the language server of `file` in `root`, for messages.
pub fn server_name(root: &Path, file: &Path) -> String {
    crate::LanguageId::detect(file)
        .and_then(|language| crate::servers::resolve_for(language, root).ok())
        .map_or_else(
            || "the language server".to_owned(),
            |s| s.candidate.program.to_owned(),
        )
}

/// `question` to the language server named `server`, given up after `limit` in all with
/// [`LspError::Timeout`] naming the server (`SPEC-0095` v9 behaviour 11): each single wait inside
/// a question has its own limit, but a server shared through the pool keeps the limit it was
/// started with, and several waits add up.
pub async fn within<T>(
    limit: Duration,
    server: &str,
    question: impl Future<Output = Result<T, LspError>>,
) -> Result<T, LspError> {
    tokio::time::timeout(limit, question)
        .await
        .unwrap_or_else(|_| Err(LspError::Timeout(server.to_owned(), limit.as_secs())))
}

/// [`refactoring_targets`] with the language server given up after `limit` in all (`SPEC-0095` v9
/// behaviour 11): IDECraft's own refactorings are still found then, and without one the error
/// names the server and the limit.
pub async fn refactoring_targets_within(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    refactoring: Refactoring,
    wait: Duration,
    limit: Duration,
) -> Result<Vec<RefactoringTarget>, LspError> {
    targets(root, file, text, range, refactoring, wait, Some(limit)).await
}

async fn targets(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    refactoring: Refactoring,
    wait: Duration,
    limit: Option<Duration>,
) -> Result<Vec<RefactoringTarget>, LspError> {
    let asked = server_targets(root, file, text, range, refactoring, wait);
    let found = match limit {
        Some(limit) => within(limit, &server_name(root, file), asked).await,
        None => asked.await,
    };
    if !own_offers(file, refactoring) || matches!(&found, Ok(t) if !t.is_empty()) {
        return found;
    }
    let Some(content) = content_of(file, text) else {
        return found;
    };
    let (own, _) = own_find(file, &content, range, refactoring);
    // Refactor This keeps the server's failure next to its other entries.
    if own.is_empty() && refactoring == Refactoring::Any {
        return found;
    }
    Ok(own)
}

/// Whether IDECraft has an own `refactoring` for `file`: Python (`SPEC-0330`), or Extract Variable
/// in JavaScript and TypeScript (`SPEC-0320` v7 behaviour 23).
fn own_offers(file: &Path, refactoring: Refactoring) -> bool {
    crate::python_extract::offers(file, refactoring) || crate::ts_extract::offers(file, refactoring)
}

/// The places of the own `refactoring` and the reason when there is none.
fn own_find(
    file: &Path,
    content: &str,
    range: &Range,
    refactoring: Refactoring,
) -> (Vec<RefactoringTarget>, Option<String>) {
    if crate::ts_extract::offers(file, refactoring) {
        crate::ts_extract::find(file, content, range, refactoring)
    } else {
        crate::python_extract::find(content, range, refactoring)
    }
}

/// The editor's `text`, or the content of `file` on disk.
fn content_of(file: &Path, text: Option<&str>) -> Option<String> {
    match text {
        Some(t) => Some(t.to_owned()),
        None => std::fs::read_to_string(file).ok(),
    }
}

/// What the status bar says for a `refactoring` that neither the language server of `file` nor
/// IDECraft offers for its language (`SPEC-0095` v13 behaviour 7a): Extract Field and Introduce
/// Parameter in Python, Introduce Parameter in JavaScript and TypeScript. In place of the hint of
/// [`Refactoring::none_message`], which would ask for a selection that cannot help.
pub fn missing_message(file: &Path, refactoring: Refactoring) -> Option<String> {
    let python = crate::LanguageId::detect(file) == Some(crate::LanguageId::Python);
    let script = crate::ts_structure::is_script(file);
    let (name, language) = match refactoring {
        Refactoring::Field if python => ("Extract Field", "Python"),
        Refactoring::Parameter if python => ("Introduce Parameter", "Python"),
        Refactoring::Parameter if script => ("Introduce Parameter", "JavaScript and TypeScript"),
        _ => return None,
    };
    Some(format!(
        "{name} is not available for {language} files yet: the language server offers none and \
         IDECraft has no own one. Extract Variable and Extract Method are."
    ))
}

/// Why IDECraft's own refactoring refuses `range` of `file` (content `text`, or the disk
/// content), for the status bar in place of [`Refactoring::none_message`]; `None` where there is
/// no own refactoring (`SPEC-0330` behaviour 14).
pub fn own_refusal(
    file: &Path,
    text: Option<&str>,
    range: &Range,
    refactoring: Refactoring,
) -> Option<String> {
    if let Some(missing) = missing_message(file, refactoring) {
        return Some(missing);
    }
    if !own_offers(file, refactoring) {
        return None;
    }
    own_find(file, &content_of(file, text)?, range, refactoring).1
}

/// What the server says to IDECraft's own Python extraction titled `title` for `range`: whether
/// it offers a refactoring of that title itself, and, when it does not, the hover texts Extract
/// Method asks for to type the new function (`SPEC-0330` v3 behaviour 16): pyright and
/// basedpyright only, each with its place, all within `wait`. Without a server: no, and none.
async fn server_for_own(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    title: &str,
    wait: Duration,
) -> (bool, Vec<((u64, u64), String)>) {
    let deadline = tokio::time::Instant::now() + wait;
    // A server that does not answer in time offers nothing: the own action goes on without it.
    let opened = tokio::time::timeout_at(deadline, Opened::open_text(root, file, text, wait)).await;
    let Ok(Ok(mut o)) = opened else {
        return (false, Vec::new());
    };
    let asked =
        tokio::time::timeout_at(deadline, request_actions(&mut o, range, &[], &["refactor"])).await;
    let offered = matches!(asked, Ok(Ok(list)) if list.iter().any(|a| a["title"] == title));
    let mut hovers = Vec::new();
    let typed = !offered
        && title == crate::python_extract::EXTRACT_METHOD
        && crate::servers::resolve_for(crate::LanguageId::Python, root)
            .is_ok_and(|s| crate::python_server::is_python_server(s.candidate.program));
    if typed {
        let uri = o.uri.clone();
        for (line, character) in crate::python_extract::type_questions(&o.text, range) {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let answer = o
                .ask_bounded(
                    "textDocument/hover",
                    o.position(line, character),
                    left,
                    None,
                )
                .await;
            if let Ok(v) = answer
                && let Some(h) = crate::insight::parse_hover(&v, &uri, &mut o.files)
            {
                hovers.push(((line, character), h.text));
            }
        }
    }
    o.close().await;
    (offered, hovers)
}

/// What the language server offers for [`refactoring_targets`].
async fn server_targets(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    refactoring: Refactoring,
    wait: Duration,
) -> Result<Vec<RefactoringTarget>, LspError> {
    let mut o = Opened::open_text(root, file, text, wait).await?;
    let result = async {
        let content = o.text.clone();
        let ask =
            async |o: &mut Opened, r: &Range| -> Result<Option<RefactoringTarget>, LspError> {
                let actions: Vec<CodeAction> = request_actions(o, r, &[], &["refactor"])
                    .await?
                    .iter()
                    .map(describe)
                    .filter(|a| refactoring.matches(a))
                    .collect();
                Ok((!actions.is_empty()).then(|| RefactoringTarget {
                    range: r.clone(),
                    text: text_of(&content, r),
                    actions,
                }))
            };
        if let Some(mut target) = ask(&mut o, range).await? {
            // Java: the variants of Inline Method (`SPEC-0122` v8 behaviour 17).
            if refactoring == Refactoring::Inline {
                let budget = wait.min(crate::java_refactor::VARIANTS_WAIT);
                target.actions = crate::java_refactor::inline_choices(
                    &mut o,
                    file,
                    range,
                    target.actions,
                    budget,
                )
                .await;
            }
            return Ok(vec![target]);
        }
        let caret =
            (range.start_line, range.start_character) == (range.end_line, range.end_character);
        if !caret || !refactoring.widens() {
            return Ok(Vec::new());
        }
        let enclosing: Vec<(Range, String)> =
            enclosing_ranges(&mut o, range.start_line, range.start_character)
                .await
                .into_iter()
                .map(|r| {
                    let t = text_of(&content, &r);
                    (r, t)
                })
                .collect();
        let mut found = Vec::new();
        for (r, _) in widened(refactoring, enclosing) {
            if let Some(target) = ask(&mut o, &r).await? {
                found.push(target);
            }
        }
        Ok(found)
    }
    .await;
    o.close().await;
    result
}

/// Plans the action titled `title` for `range` of `file` (behaviour 3).
pub async fn plan_code_action(
    root: &Path,
    file: &Path,
    range: &Range,
    title: &str,
    wait: Duration,
) -> Result<Vec<FileEdits>, LspError> {
    plan_code_action_in(root, file, None, range, title, wait).await
}

/// The workspace edits of `action` (named `title` in messages): its own edit, resolved first
/// where the server resolves actions, and the edits its command applies (`SPEC-0041` v2
/// behaviour 3). Empty when the action changes nothing.
pub(crate) async fn action_edits(
    o: &mut Opened,
    action: &Value,
    title: &str,
) -> Result<Vec<Value>, LspError> {
    let resolved;
    let action = if needs_resolve(action, &o.capabilities) {
        resolved = o
            .ask("codeAction/resolve", action.clone())
            .await
            .map_err(|e| LspError::Protocol(format!("{title} could not be resolved: {e}")))?;
        &resolved
    } else {
        action
    };
    let mut edits = Vec::new();
    if !action["edit"].is_null() {
        edits.push(action["edit"].clone());
    }
    // A bare `Command` has `command` as a string; a `CodeAction` nests it.
    let command = if action["command"].is_string() {
        Some(action.clone())
    } else {
        action.get("command").filter(|c| c.is_object()).cloned()
    };
    if let Some(command) = command {
        o.lsp().take_applied_edits();
        o.ask(
            "workspace/executeCommand",
            json!({"command": command["command"], "arguments": command["arguments"]}),
        )
        .await?;
        edits.extend(o.lsp().take_applied_edits());
    }
    Ok(edits)
}

/// Plans the action titled `title` for `range` of `file` whose content is `text`, or the disk
/// content; edits of `file` refer to `text` (`SPEC-0076`).
pub async fn plan_code_action_in(
    root: &Path,
    file: &Path,
    text: Option<&str>,
    range: &Range,
    title: &str,
    wait: Duration,
) -> Result<Vec<FileEdits>, LspError> {
    // IDECraft's own extractions, unless the server has an action of that title (`SPEC-0330`
    // behaviour 1, `SPEC-0320` v7 behaviour 23).
    if crate::ts_extract::is_own(file, title) {
        let (offered, _) = server_for_own(root, file, text, range, title, wait).await;
        if !offered {
            let content = content_of(file, text)
                .ok_or_else(|| LspError::Protocol(format!("{} cannot be read", file.display())))?;
            let edits = crate::ts_extract::plan(file, &content, range, title)
                .map_err(LspError::Protocol)?;
            return Ok(vec![FileEdits {
                path: file.to_owned(),
                edits,
            }]);
        }
    }
    if crate::python_extract::is_own(file, title) {
        let (offered, hovers) = server_for_own(root, file, text, range, title, wait).await;
        if !offered {
            let content = content_of(file, text)
                .ok_or_else(|| LspError::Protocol(format!("{} cannot be read", file.display())))?;
            let edits = crate::python_extract::plan_typed(&content, range, title, &hovers)
                .map_err(LspError::Protocol)?;
            return Ok(vec![FileEdits {
                path: file.to_owned(),
                edits,
            }]);
        }
    }
    let (mut o, diagnostics) = open_with_diagnostics(root, file, text, wait).await?;
    let result = async {
        // Java: a variant of Inline Method (`SPEC-0122` v8 behaviour 17).
        if crate::java_refactor::is_inline_choice(file, title) {
            let edits = crate::java_refactor::plan_inline(&mut o, file, range, title, wait).await?;
            let files = file_edits(&mut o, &edits)?;
            crate::java_refactor::check_apart(title, &files)?;
            return Ok(files);
        }
        let actions = request_actions(&mut o, range, &diagnostics, &[]).await?;
        let Some(action) = actions.iter().find(|a| a["title"] == title) else {
            let titles: Vec<&str> = actions.iter().filter_map(|a| a["title"].as_str()).collect();
            // Java: one of IDECraft's own fixes (`SPEC-0076` v7 behaviours 11, 12).
            let offered: Vec<String> = titles.iter().map(|t| (*t).to_owned()).collect();
            let own = crate::java_quick_fixes::own_fixes(
                file,
                &o.text,
                o.files.encoding,
                range,
                &diagnostics,
                &offered,
            );
            if let Some(fix) = own.into_iter().find(|f| f.action.title == title) {
                return Ok(vec![FileEdits {
                    path: file.to_owned(),
                    edits: fix.edits,
                }]);
            }
            return Err(LspError::Protocol(format!(
                "no action titled {title:?}; available: {}",
                titles.join(", ")
            )));
        };
        if let Some(reason) = action["disabled"]["reason"].as_str() {
            return Err(LspError::Protocol(format!("{title} is disabled: {reason}")));
        }
        let edits = action_edits(&mut o, action, title).await?;
        if edits.is_empty() {
            return Err(LspError::Protocol(format!("{title} changes nothing")));
        }
        let mut files = file_edits(&mut o, &edits)?;
        // Java: the new constant or variable is named after its place (`SPEC-0122` v8
        // behaviour 16).
        let budget = wait.min(crate::java_refactor::NAME_WAIT);
        crate::java_refactor::improve_name(&mut o, file, range, title, &mut files, budget).await;
        Ok(files)
    }
    .await;
    o.close().await;
    result
}

/// Whether the code action titled `title` extracts a variable, a constant, a field or a method or
/// introduces a parameter, which introduces a name the user should choose (`SPEC-0095` v3
/// behaviour 8, v13 behaviour 8d).
pub fn introduces_name(title: &str) -> bool {
    let action = CodeAction {
        title: title.to_owned(),
        kind: None,
        preferred: false,
        disabled: None,
        fixes: Vec::new(),
    };
    [
        Refactoring::Variable,
        Refactoring::Constant,
        Refactoring::Field,
        Refactoring::Method,
        Refactoring::Parameter,
    ]
    .iter()
    .any(|r| r.matches(&action))
}

/// A name a refactoring introduced: the name and where its first occurrence in the changed part
/// starts (0-based line and character).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntroducedName {
    /// The name.
    pub name: String,
    /// Line of the occurrence.
    pub line: u64,
    /// Character of the occurrence in its line.
    pub character: u64,
}

/// The identifier words of `text` with their start (in characters); words starting with a digit are
/// left out.
fn words(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (i, c) in text.chars().chain(std::iter::once(' ')).enumerate() {
        if c.is_alphanumeric() || c == '_' || c == '$' {
            if current.is_empty() {
                start = i;
            }
            current.push(c);
        } else if !current.is_empty() {
            let word = std::mem::take(&mut current);
            if !word.starts_with(|c: char| c.is_ascii_digit()) {
                out.push((start, word));
            }
        }
    }
    out
}

/// The name an extraction introduced from `before` to `after` (`SPEC-0095` v3 behaviour 8): of the
/// names whose number of occurrences grew, the one that grew by at least two (declaration and
/// use), then one that was not in `before`, then the one that grew most, then the first; taken at
/// its first occurrence inside the changed part of `after`. `None` when no name grew.
pub fn introduced_name(before: &str, after: &str) -> Option<IntroducedName> {
    let count = |list: &[(usize, String)]| {
        let mut m: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (_, w) in list {
            *m.entry(w.clone()).or_default() += 1;
        }
        m
    };
    let (old, new) = (words(before), words(after));
    let (old_count, new_count) = (count(&old), count(&new));
    // The changed part of `after`, in characters.
    let (b, a): (Vec<char>, Vec<char>) = (before.chars().collect(), after.chars().collect());
    let prefix = b.iter().zip(&a).take_while(|(x, y)| x == y).count();
    let max_suffix = b.len().min(a.len()) - prefix;
    let suffix = b
        .iter()
        .rev()
        .zip(a.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    let changed = prefix..a.len() - suffix;
    // Rank: grew by two or more, was not there, how much it grew, earlier first.
    type Rank = (bool, bool, usize, std::cmp::Reverse<usize>);
    let mut best: Option<(Rank, usize, &str)> = None;
    for (start, word) in &new {
        let end = start + word.chars().count();
        // An occurrence touching the changed part (a name may extend an unchanged one).
        if end < changed.start || *start > changed.end {
            continue;
        }
        let had = old_count.get(word).copied().unwrap_or(0);
        let has = new_count[word];
        if has <= had {
            continue;
        }
        let grew = has - had;
        let rank = (grew >= 2, had == 0, grew, std::cmp::Reverse(*start));
        if best.as_ref().is_none_or(|(r, _, _)| rank > *r) {
            best = Some((rank, *start, word));
        }
    }
    let (_, start, word) = best?;
    let line = a[..start].iter().filter(|c| **c == '\n').count();
    let line_start = a[..start]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |i| i + 1);
    Some(IntroducedName {
        name: word.to_owned(),
        line: line as u64,
        character: (start - line_start) as u64,
    })
}

/// Completes a rename of a name an extraction just introduced into `path`, which was in no other
/// place of the file before (`SPEC-0095` v4 behaviour 10, D55): every occurrence of the old name in
/// `text` (the file's current text) is renamed, also those the server left out. A server can miss
/// them when the extracted expression does not compile, e.g. clangd, whose uses of a variable with
/// an undeducible type are no references.
pub fn complete_introduced_rename(plan: &mut crate::rename::RenamePlan, path: &Path, text: &str) {
    use crate::rename::Edit;
    let same = |p: &Path| {
        p == path
            || (p
                .canonicalize()
                .ok()
                .is_some_and(|c| path.canonicalize().ok() == Some(c)))
    };
    let index = match plan.files.iter().position(|f| same(&f.path)) {
        Some(i) => i,
        None => {
            plan.files.push(FileEdits {
                path: path.to_owned(),
                edits: Vec::new(),
            });
            plan.files.len() - 1
        }
    };
    let chars: Vec<char> = text.chars().collect();
    let file = &mut plan.files[index];
    for (start, word) in words(text) {
        if word != plan.old_name {
            continue;
        }
        let line = chars[..start].iter().filter(|c| **c == '\n').count() as u64;
        let line_start = chars[..start]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1);
        let character = (start - line_start) as u64;
        let range = Range {
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character + word.chars().count() as u64,
        };
        if file
            .edits
            .iter()
            .any(|e| e.range.start_line == line && e.range.start_character == character)
        {
            continue;
        }
        file.edits.push(Edit {
            range,
            old_text: word,
            new_text: plan.new_name.clone(),
        });
    }
    file.edits
        .sort_by_key(|e| (e.range.start_line, e.range.start_character));
    plan.files.sort_by(|a, b| a.path.cmp(&b.path));
}

/// Whether `name` occurs in `text` as a word.
pub fn has_word(text: &str, name: &str) -> bool {
    words(text).iter().any(|(_, w)| w == name)
}

/// One of IDECraft's own entries in Refactor This: the title shown and the keymap action it runs
/// (`SPEC-0095` v5 behaviour 4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OwnRefactoring {
    /// Title in the popup.
    pub title: &'static str,
    /// Keymap action id run when it is chosen.
    pub action: &'static str,
    /// What the popup's preview says about it.
    pub preview: &'static str,
}

/// IDECraft's own entries of Refactor This next to the server's `actions` (`SPEC-0095` v5
/// behaviour 4a): Rename when `on_name` (the caret, or a selection that is exactly one name, is on
/// a name; a rename is no code action, so no server lists it), and Extract Variable and Extract
/// Method unless the server already lists a matching action. The extractions run their shortcuts,
/// which widen a bare caret and say why nothing can be extracted.
pub fn refactor_this_own(actions: &[CodeAction], on_name: bool) -> Vec<OwnRefactoring> {
    let mut out = Vec::new();
    if on_name {
        out.push(OwnRefactoring {
            title: "Rename…",
            action: "RenameElement",
            preview: "Asks for the new name; every use follows through the language server",
        });
    }
    let has = |r: Refactoring| actions.iter().any(|a| r.matches(a));
    if !has(Refactoring::Variable) {
        out.push(OwnRefactoring {
            title: "Extract Variable…",
            action: "IntroduceVariable",
            preview: "Asks the language server which expression around the caret it can extract",
        });
    }
    if !has(Refactoring::Method) {
        out.push(OwnRefactoring {
            title: "Extract Method…",
            action: "ExtractMethod",
            preview: "Asks the language server which statements it can extract into a function",
        });
    }
    out
}

/// The status line after the name popup of an extraction closed without a new name
/// (`SPEC-0095` v5 behaviour 8a).
pub fn kept_name_message(name: &str) -> String {
    format!("Kept the name {name}.")
}

/// The status line after ⌘Z (`redo` false) or ⇧⌘Z took back or re-applied the refactoring named
/// `name` in the editor (`SPEC-0193` v4 behaviour 9).
pub fn undo_message(name: &str, redo: bool) -> String {
    format!("{}: {name}", if redo { "Redone" } else { "Undone" })
}

#[cfg(test)]
mod tests {
    /// SPEC-0095 AK-19 (v11): Introduce Parameter takes the refactorings that introduce,
    /// extract or promote a parameter, and none of the others.
    #[test]
    fn introduce_parameter_matches_parameter_refactorings() {
        use super::*;
        let action = |title: &str, kind: Option<&str>| CodeAction {
            title: title.into(),
            kind: kind.map(str::to_owned),
            preferred: false,
            disabled: None,
            fixes: Vec::new(),
        };
        let p = Refactoring::Parameter;
        assert_eq!(Refactoring::of_action("IntroduceParameter"), Some(p));
        assert_eq!(Refactoring::of_name("introduce_parameter"), Some(p));
        assert!(p.matches(&action(
            "Introduce Parameter...",
            Some("refactor.introduce.parameter")
        )));
        assert!(p.matches(&action("Promote local to parameter", None)));
        assert!(!p.matches(&action(
            "Extract to local variable",
            Some("refactor.extract")
        )));
        assert!(!p.matches(&action("Add parameter", Some("quickfix"))));
        assert!(!Refactoring::Variable.matches(&action("Introduce Parameter...", None)));
        assert!(p.widens());
        assert_eq!(
            p.none_message("rust-analyzer", Some("a + 1")),
            "rust-analyzer cannot introduce `a + 1` as a parameter. Select an expression inside a method or function."
        );
    }

    use super::*;

    /// SPEC-0041 v3 AK-5: applicable actions come first, the disabled ones keep their reason
    /// and are left out on request; the order within each part is the server's.
    #[test]
    fn applicable_actions_come_first_and_disabled_ones_can_be_left_out() {
        let a = |title: &str, disabled: Option<&str>| CodeAction {
            title: title.into(),
            kind: Some("refactor".into()),
            preferred: false,
            disabled: disabled.map(str::to_owned),
            fixes: Vec::new(),
        };
        let offered = vec![
            a(
                "Convert default export to named export",
                Some("Could not find export statement"),
            ),
            a("Move to a new file", None),
            a(
                "Inline variable",
                Some("Could not find variable to inline."),
            ),
            a("Extract to constant in enclosing scope", None),
        ];
        let titles =
            |list: &[CodeAction]| -> Vec<String> { list.iter().map(|a| a.title.clone()).collect() };
        let all = applicable_first(offered.clone(), false);
        assert_eq!(
            titles(&all),
            [
                "Move to a new file",
                "Extract to constant in enclosing scope",
                "Convert default export to named export",
                "Inline variable",
            ]
        );
        assert_eq!(
            all[2].disabled.as_deref(),
            Some("Could not find export statement")
        );
        assert_eq!(
            titles(&applicable_first(offered, true)),
            [
                "Move to a new file",
                "Extract to constant in enclosing scope"
            ]
        );
        // SPEC-0041 v4 AK-7: jdtls sends "Organize imports" twice at an unused import (headless
        // check of 2026-11-16); the agent sees it once.
        let twice = vec![
            a("Organize imports", None),
            a("Organize imports", None),
            a("Add all missing imports", None),
            a("Add all missing imports", Some("nothing to add")),
        ];
        assert_eq!(
            titles(&applicable_first(twice, false)),
            ["Organize imports", "Add all missing imports"]
        );
    }

    /// SPEC-0095 v9 AK-16: a question is given up at its limit, naming the server; one answered
    /// in time is passed on.
    #[tokio::test]
    async fn a_question_is_given_up_at_its_limit() {
        let silent = std::future::pending::<Result<(), LspError>>();
        let gone = within(Duration::from_millis(50), "jdtls", silent).await;
        assert_eq!(gone, Err(LspError::Timeout("jdtls".into(), 0)));
        assert_eq!(
            LspError::Timeout("jdtls".into(), 45).to_string(),
            "jdtls: no answer within 45 s"
        );
        let answered = within(Duration::from_secs(5), "jdtls", async { Ok(7) }).await;
        assert_eq!(answered, Ok(7));
        assert_eq!(
            server_name(Path::new("/p"), Path::new("/p/notes.txt")),
            "the language server"
        );
    }

    /// SPEC-0095 v13 AK-24: the name jdtls gives an extracted field or an introduced parameter
    /// (`i`, seen in a headless check on 2026-11-16) is found like an extracted variable's.
    #[test]
    fn fields_and_parameters_introduce_a_name() {
        let before = "class Shop {\n    int total(int price, int qty) {\n        int sum = price * qty + 42;\n        return sum;\n    }\n}\n";
        let field = "class Shop {\n    private int i;\n\n    int total(int price, int qty) {\n        i = price * qty;\n        int sum = i + 42;\n        return sum;\n    }\n}\n";
        assert_eq!(
            introduced_name(before, field),
            Some(IntroducedName {
                name: "i".into(),
                line: 1,
                character: 16
            })
        );
        let parameter = "class Shop {\n    int total(int price, int qty, int i) {\n        int sum = i + 42;\n        return sum;\n    }\n}\n";
        assert_eq!(
            introduced_name(before, parameter).map(|n| (n.name, n.line)),
            Some(("i".into(), 1))
        );
    }

    /// SPEC-0095 v13 AK-25: a refactoring nobody offers for the language says so instead of
    /// asking for another selection.
    #[test]
    fn missing_refactorings_say_so() {
        let py = Path::new("/p/shop.py");
        let ts = Path::new("/p/shop.ts");
        let js = Path::new("/p/shop.js");
        let field = missing_message(py, Refactoring::Field).unwrap();
        assert!(
            field.starts_with("Extract Field is not available for Python files"),
            "{field}"
        );
        assert!(
            missing_message(py, Refactoring::Parameter)
                .unwrap()
                .starts_with("Introduce Parameter is not available for Python files")
        );
        for f in [ts, js] {
            assert!(
                missing_message(f, Refactoring::Parameter)
                    .unwrap()
                    .contains("JavaScript and TypeScript files"),
                "{f:?}"
            );
            assert_eq!(missing_message(f, Refactoring::Field), None);
        }
        for r in [
            Refactoring::Variable,
            Refactoring::Constant,
            Refactoring::Method,
            Refactoring::Inline,
            Refactoring::Any,
        ] {
            assert_eq!(missing_message(py, r), None, "{r:?}");
        }
        assert_eq!(
            missing_message(Path::new("/p/A.java"), Refactoring::Parameter),
            None
        );
        // It is the refusal the window and `code_actions` show.
        let range = Range {
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
        };
        assert_eq!(
            own_refusal(py, Some("x = 1\n"), &range, Refactoring::Field),
            Some(field)
        );
    }

    #[test]
    fn extractions_introduce_a_name() {
        assert!(introduces_name("Extract subexpression to variable"));
        assert!(introduces_name("Extract to function"));
        assert!(introduces_name("Extract into variable"));
        assert!(introduces_name("Extract into constant"));
        assert!(introduces_name(
            "Extract to local variable (replace all occurrences)"
        ));
        assert!(introduces_name("Extract to method"));
        // SPEC-0095 v13 AK-24: jdtls's field and parameter are named too.
        assert!(introduces_name("Extract to field"));
        assert!(introduces_name("Introduce Parameter..."));
        assert!(!introduces_name("Create parameter 'rate'"));
        assert!(!introduces_name("Inline variable"));
        assert!(!introduces_name("Add missing import"));
    }

    /// SPEC-0095 v4 behaviour 10 (D55): a rename of a just introduced name covers the uses the
    /// server left out, once each.
    #[test]
    fn a_new_name_is_renamed_everywhere() {
        use crate::rename::{Edit, RenamePlan, apply_edits};
        let text =
            "int main() {\n    auto placeholder = s.area();\n    int t = add(placeholder, 1);\n}\n";
        let path = Path::new("/p/shapes.cpp");
        let declaration = Edit {
            range: Range {
                start_line: 1,
                start_character: 9,
                end_line: 1,
                end_character: 20,
            },
            old_text: "placeholder".into(),
            new_text: "flaeche".into(),
        };
        let mut plan = RenamePlan {
            old_name: "placeholder".into(),
            new_name: "flaeche".into(),
            files: vec![FileEdits {
                path: path.to_owned(),
                edits: vec![declaration],
            }],
        };
        complete_introduced_rename(&mut plan, path, text);
        assert_eq!(plan.files[0].edits.len(), 2);
        assert_eq!(
            apply_edits(text, &plan.files[0].edits).unwrap(),
            "int main() {\n    auto flaeche = s.area();\n    int t = add(flaeche, 1);\n}\n"
        );
        assert!(has_word(text, "placeholder"));
        assert!(!has_word(text, "holder"));
    }

    #[test]
    fn the_introduced_name_is_found() {
        let at = |n: &IntroducedName| (n.name.clone(), n.line, n.character);
        // clangd, Extract Variable.
        let before = "int main() {\n    int total = add(s.area(), 1);\n}\n";
        let after = "int main() {\n    auto placeholder = s.area();\n    int total = add(placeholder, 1);\n}\n";
        let n = introduced_name(before, after).unwrap();
        assert_eq!(at(&n), ("placeholder".into(), 1, 9));
        // clangd, Extract Function: the parameter `s` grew by two as well, but existed before.
        let before = "int main() {\n    Shape &s = sq;\n    int total = add(s.area(), 1);\n}\n";
        let after = "int extracted(Shape &s) {\n    int total = add(s.area(), 1);\n    return total;\n}\nint main() {\n    Shape &s = sq;\n    extracted(s);\n}\n";
        let n = introduced_name(before, after).unwrap();
        assert_eq!(at(&n), ("extracted".into(), 0, 4));
        // rust-analyzer.
        let before = "fn main() {\n    let t = add(s.area(), 1);\n}\n";
        let after = "fn main() {\n    let var_name = s.area();\n    let t = add(var_name, 1);\n}\n";
        assert_eq!(introduced_name(before, after).unwrap().name, "var_name");
        let after = "fn main() {\n    fun_name(s);\n}\n\nfn fun_name(s: &S) {\n    let t = add(s.area(), 1);\n}\n";
        assert_eq!(introduced_name(before, after).unwrap().name, "fun_name");
        // jdtls: a name derived from the expression that already existed (the method `area`);
        // the occurrence taken is the new one, not the method's.
        let before = "class A {\n  int area() { return 1; }\n  void f() {\n    int t = add(area(), 1);\n  }\n}\n";
        let after = "class A {\n  int area() { return 1; }\n  void f() {\n    int area = area();\n    int t = add(area, 1);\n  }\n}\n";
        let n = introduced_name(before, after).unwrap();
        assert_eq!(at(&n), ("area".into(), 3, 8));
        // Nothing new: a pure removal, and no change.
        assert_eq!(introduced_name(after, before), None);
        assert_eq!(introduced_name(before, before), None);
    }

    fn r(sl: u64, sc: u64, el: u64, ec: u64) -> Range {
        Range {
            start_line: sl,
            start_character: sc,
            end_line: el,
            end_character: ec,
        }
    }

    /// SPEC-0095 v2 AK-5: the expressions around a caret offered for extracting.
    /// SPEC-0095 v6 behaviour 2, AK-14: the titles of typescript-language-server 5.3.0 with
    /// TypeScript 5.6.3, as it answered for `x>3`, `[1,2,3]` and a whole statement.
    #[test]
    fn typescript_extractions_match_their_refactorings() {
        let a = |title: &str, kind: &str, disabled: bool| CodeAction {
            title: title.into(),
            kind: Some(kind.into()),
            preferred: false,
            disabled: disabled.then(|| "Cannot extract".to_owned()),
            fixes: Vec::new(),
        };
        let enclosing = a(
            "Extract to constant in enclosing scope",
            "refactor.extract.constant",
            false,
        );
        let module = a(
            "Extract to constant in module scope",
            "refactor.extract.constant",
            false,
        );
        let inner = a(
            "Extract to inner function in function 'f'",
            "refactor.extract.function",
            false,
        );
        let function = a(
            "Extract to function in module scope",
            "refactor.extract.function",
            false,
        );
        let typedef = a("Extract to type alias", "refactor.extract.type", true);
        let refused = a(
            "Extract to constant in enclosing scope",
            "refactor.extract.constant",
            true,
        );
        use Refactoring::*;
        // Extract Variable: the constant before the statement, not the one at the top of the file.
        assert!(Variable.matches(&enclosing));
        assert!(!Variable.matches(&module) && !Variable.matches(&function));
        assert!(!Variable.matches(&refused) && !Variable.matches(&typedef));
        assert!(Constant.matches(&enclosing) && Constant.matches(&module));
        assert!(Method.matches(&inner) && Method.matches(&function));
        assert!(!Method.matches(&enclosing));
    }

    #[test]
    fn widening_keeps_expressions_and_stops_at_the_statement() {
        let text = "fn main() {\n    let total = add(sq.area(), 1);\n}\n";
        let ranges = [
            r(1, 20, 1, 22),
            r(1, 20, 1, 27),
            r(1, 20, 1, 29),
            r(1, 19, 1, 33),
            r(1, 16, 1, 33),
            r(1, 4, 1, 34),
            r(0, 10, 2, 1),
        ];
        let enclosing: Vec<(Range, String)> = ranges
            .iter()
            .map(|x| (x.clone(), text_of(text, x)))
            .collect();
        let texts = |r: Refactoring| -> Vec<String> {
            widened(r, enclosing.clone())
                .into_iter()
                .map(|(_, t)| t)
                .collect()
        };
        // `sq.area` is the callee of `sq.area()`, `(sq.area(), 1)` an argument list.
        assert_eq!(
            texts(Refactoring::Variable),
            ["sq", "sq.area()", "add(sq.area(), 1)"]
        );
        assert_eq!(
            texts(Refactoring::Method),
            [
                "sq",
                "sq.area()",
                "add(sq.area(), 1)",
                "let total = add(sq.area(), 1);"
            ]
        );
        assert_eq!(text_of("aé b\nc", &r(0, 1, 1, 1)), "é b\nc");
    }

    /// SPEC-0095 v2 AK-5: the status message names the server, the selection and what to do.
    #[test]
    fn refactorings_say_why_nothing_happened() {
        let m = Refactoring::Variable.none_message("clangd", Some(" add(s.area(), 1) "));
        assert_eq!(
            m,
            "clangd cannot extract `add(s.area(), 1)` into a variable. Select part of an \
             expression; a whole statement, or a call that already is a variable's whole value, is \
             not extracted."
        );
        let m = Refactoring::Inline.none_message("clangd", None);
        assert!(
            m.starts_with("clangd cannot inline anything at the caret."),
            "{m}"
        );
        let long = "x".repeat(50);
        let m = Refactoring::Method.none_message("rust-analyzer", Some(&long));
        assert!(m.contains(&format!("`{}…`", "x".repeat(40))), "{m}");
        assert_eq!(
            Refactoring::of_name("extract_variable"),
            Some(Refactoring::Variable)
        );
        assert!(Refactoring::Method.widens() && !Refactoring::Inline.widens());
    }

    /// SPEC-0095 v5 AK-10: Refactor This adds Rename on a name and the extractions the server did
    /// not list (hand test 14: clangd lists nothing on a bare name).
    #[test]
    fn refactor_this_adds_rename_and_missing_extractions() {
        let action = |title: &str| CodeAction {
            title: title.into(),
            kind: Some("refactor".into()),
            preferred: false,
            disabled: None,
            fixes: Vec::new(),
        };
        let titles = |own: Vec<OwnRefactoring>| own.iter().map(|o| o.title).collect::<Vec<_>>();
        // clangd on the name `placeholder`: no actions at all.
        assert_eq!(
            titles(refactor_this_own(&[], true)),
            ["Rename…", "Extract Variable…", "Extract Method…"]
        );
        // clangd with `s.area()` selected: its Extract Variable is listed, not ours.
        let own = refactor_this_own(&[action("Extract subexpression to variable")], false);
        assert_eq!(titles(own.clone()), ["Extract Method…"]);
        assert_eq!(own[0].action, "ExtractMethod");
        // rust-analyzer with a selection lists both extractions itself.
        let both = [
            action("Extract into variable"),
            action("Extract into function"),
        ];
        assert!(refactor_this_own(&both, false).is_empty());
        assert_eq!(refactor_this_own(&both, true)[0].action, "RenameElement");
        // A disabled action does not count as listed.
        let mut disabled = action("Extract into variable");
        disabled.disabled = Some("no expression".into());
        assert_eq!(
            titles(refactor_this_own(&[disabled], false))[0],
            "Extract Variable…"
        );
    }

    /// SPEC-0095 v5 AK-11, SPEC-0193 v4 AK-8: the status line follows Escape and ⌘Z.
    #[test]
    fn status_messages_follow_the_last_action() {
        assert_eq!(
            kept_name_message("placeholder"),
            "Kept the name placeholder."
        );
        assert_eq!(
            undo_message("Rename placeholder to flaeche", false),
            "Undone: Rename placeholder to flaeche"
        );
        assert_eq!(
            undo_message("Extract subexpression to variable", true),
            "Redone: Extract subexpression to variable"
        );
    }

    #[test]
    fn resolves_only_lazy_actions_of_resolving_servers() {
        let resolving = json!({"codeActionProvider": {"resolveProvider": true}});
        let plain = json!({"codeActionProvider": true});
        let lazy = json!({"title": "Extract into variable", "data": {"id": 1}});
        assert!(needs_resolve(&lazy, &resolving));
        assert!(!needs_resolve(&lazy, &plain), "nothing to resolve with");
        let with_edit = json!({"title": "Fix", "edit": {"changes": {}}});
        assert!(!needs_resolve(&with_edit, &resolving));
        let with_command = json!({"title": "Extract", "command": {"command": "x"}});
        assert!(!needs_resolve(&with_command, &resolving));
        let bare = json!({"title": "Run", "command": "x"});
        assert!(
            !needs_resolve(&bare, &resolving),
            "a bare command is no action"
        );
    }
}
