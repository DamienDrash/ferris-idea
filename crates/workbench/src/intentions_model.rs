//! Order and filter of the context-actions popup (`SPEC-0076`), free of the UI toolkit.

use idecraft_languages::actions::{CodeAction, OwnRefactoring, RefactoringTarget};

/// How long the language server has in all to say where a refactoring can act, and again to plan
/// the chosen one; after that the status bar names the server and the limit (`SPEC-0095` v9
/// behaviour 11).
pub const REFACTORING_LIMIT: std::time::Duration = std::time::Duration::from_secs(45);

/// Rank of an action: applicable before disabled, preferred first, then quick fixes, then
/// refactorings, then the rest (behaviour 2).
fn rank(a: &CodeAction) -> (bool, bool, u8) {
    let kind = a.kind.as_deref().unwrap_or_default();
    let group = if kind == "quickfix" || kind.starts_with("quickfix.") {
        0
    } else if kind.starts_with("refactor") {
        1
    } else {
        2
    };
    (a.disabled.is_some(), !a.preferred, group)
}

/// `actions` in the popup's order; equal ranks keep the server's order. An action whose title
/// came already is left out: jdtls offers some source actions twice (OBS-0027, `SPEC-0076` v4
/// behaviour 8), and applying looks the action up by its title.
pub fn order(mut actions: Vec<CodeAction>) -> Vec<CodeAction> {
    actions.sort_by_key(rank);
    let mut seen = std::collections::HashSet::new();
    actions.retain(|a| seen.insert(a.title.clone()));
    actions
}

/// Whether `a` is a quick fix (shown with the red bulb) rather than an intention.
pub fn is_fix(a: &CodeAction) -> bool {
    !a.fixes.is_empty() || a.kind.as_deref().is_some_and(|k| k.starts_with("quickfix"))
}

/// Indices of the actions whose title contains every word of `query`, ignoring case (speed search,
/// behaviour 3).
pub fn filter(actions: &[CodeAction], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    actions
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            let title = a.title.to_lowercase();
            words.iter().all(|w| title.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

/// The first selectable row of `shown`, if any (disabled actions cannot be chosen).
pub fn first_enabled(actions: &[CodeAction], shown: &[usize]) -> Option<usize> {
    shown.iter().position(|&i| actions[i].disabled.is_none())
}

/// A refactoring asked for by its own shortcut (`SPEC-0095`); lives with the code actions so that
/// agents and live tests match the same way.
pub use idecraft_languages::actions::Refactoring;

/// A place a refactoring shortcut can act on, as the popup lists it (`SPEC-0095` v2 behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetChoice {
    /// The row: the expression's text on one line, shortened.
    pub label: String,
    /// The range it stands for.
    pub range: idecraft_languages::insight::Range,
    /// The matching actions there; the first is applied.
    pub actions: Vec<CodeAction>,
}

/// Longest label before "…".
const LABEL_CHARS: usize = 60;

/// The rows for `targets`, inside out: the text with runs of white space joined to one space,
/// shortened to [`LABEL_CHARS`]; equal labels get their line number.
pub fn target_choices(
    targets: Vec<idecraft_languages::actions::RefactoringTarget>,
) -> Vec<TargetChoice> {
    let mut out: Vec<TargetChoice> = Vec::new();
    for t in targets {
        let joined = t.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut label: String = joined.chars().take(LABEL_CHARS).collect();
        if label.len() < joined.len() {
            label.push('…');
        }
        if label.is_empty() {
            label = t
                .actions
                .first()
                .map(|a| a.title.clone())
                .unwrap_or_default();
        }
        if out.iter().any(|o| o.label == label) {
            label = format!("{label} (line {})", t.range.start_line + 1);
        }
        out.push(TargetChoice {
            label,
            range: t.range,
            actions: t.actions,
        });
    }
    out
}

/// The language server as a refactoring's request sees it (`SPEC-0095` v10 behaviour 12): the
/// real one of a file ([`FileServer`]), or a scripted one in tests.
pub trait RefactoringServer {
    /// Its name in messages.
    fn name(&self) -> String;
    /// Whether it is still starting or loading the project, so that its answers are not final.
    fn loading(&self) -> bool;
    /// Where the refactoring can act, or why the question failed.
    fn targets(&mut self) -> Result<Vec<RefactoringTarget>, String>;
    /// Why IDECraft's own refactoring refuses the place, where there is one.
    fn own_refusal(&mut self) -> Option<String>;
    /// Waits at most `wait` until the server has loaded the project; whether it has.
    fn loaded_within(&mut self, wait: std::time::Duration) -> bool;
}

/// What a refactoring's shortcut asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The refactoring.
    pub refactoring: Refactoring,
    /// The selected text, `None` for a bare caret.
    pub selected: Option<String>,
    /// Whether the caret or the selection is on a name (Refactor This offers Rename then).
    pub on_name: bool,
}

/// What a refactoring's request ends in. Every outcome is something the user sees: a list, an
/// edit, or one line in the status bar (`SPEC-0095` v10 behaviour 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The places the refactoring can act on: one place with one action is applied, one place
    /// with several actions lists the actions, several places are listed.
    Places(Vec<RefactoringTarget>),
    /// Refactor This: the server's actions and IDECraft's own entries in one list, and what the
    /// status bar says beside it.
    Menu {
        /// The server's actions.
        actions: Vec<CodeAction>,
        /// IDECraft's own entries.
        own: Vec<OwnRefactoring>,
        /// The status line beside the list.
        note: Option<String>,
    },
    /// One line in the status bar.
    Status(String),
}

impl Outcome {
    /// Whether the user sees it: places with actions, a list with rows, or a line with text.
    pub fn visible(&self) -> bool {
        match self {
            Self::Places(places) => {
                !places.is_empty() && places.iter().all(|p| !p.actions.is_empty())
            }
            Self::Menu { actions, own, .. } => !actions.is_empty() || !own.is_empty(),
            Self::Status(line) => !line.trim().is_empty(),
        }
    }
}

/// Where a request stands after the server's first answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// It ended.
    Done(Outcome),
    /// The server is still loading the project: the status bar says this line, and the request
    /// goes on with [`ask_again`].
    Waiting(String),
}

/// What the status bar says from the moment a request named `what` starts, `loading` naming a
/// language server that is still starting or loading the project (`SPEC-0095` v10 behaviour 11).
pub fn progress_message(what: &str, loading: Option<&str>) -> String {
    match loading {
        Some(server) => format!("{what}… ({server} is still loading the project)"),
        None => format!("{what}…"),
    }
}

/// What the status bar says when the request named `what` failed with `error`, `loading` naming a
/// language server that is still starting or loading the project.
pub fn failed_message(what: &str, error: &str, loading: Option<&str>) -> String {
    match loading {
        Some(server) => format!(
            "{what} failed: {error}. {}",
            idecraft_languages::actions::loading_message(server)
        ),
        None => format!("{what} failed: {error}"),
    }
}

/// What the status bar says when ⌥↩ finds no context action, `loading` naming a language server
/// that is still starting or loading the project (`SPEC-0095` v10 behaviour 12).
pub fn no_actions_message(loading: Option<&str>) -> String {
    match loading {
        Some(server) => idecraft_languages::actions::loading_message(server),
        None => "No context actions available here.".into(),
    }
}

/// What the status bar says when the server has loaded the project but the repeated answer is
/// not used, because the text or the caret is no longer what was asked about.
pub fn not_repeated_message(refactoring: Refactoring) -> String {
    format!(
        "The project is loaded, but the text or the caret changed meanwhile: {} was not repeated.",
        refactoring.name()
    )
}

/// Asks `server` once and says where the request stands: an answer with places is taken; an
/// empty one or a failure from a server that is still loading the project is not final, and the
/// request waits ([`Step::Waiting`]) unless IDECraft's own refactoring gave its reason; from a
/// ready server the empty answer and the failure are said as they are (`SPEC-0095` v10
/// behaviours 12 and 13).
pub fn ask(server: &mut impl RefactoringServer, asked: &Asked) -> Step {
    let found = server.targets();
    answer(server, asked, found, true)
}

/// The rest of a request that waits for the server: when it has loaded the project within `left`
/// the question is repeated once and its answer is final; otherwise the status bar says that the
/// server is still loading.
pub fn ask_again(
    server: &mut impl RefactoringServer,
    asked: &Asked,
    left: std::time::Duration,
) -> Outcome {
    let still = Outcome::Status(idecraft_languages::actions::loading_message(&server.name()));
    if left.is_zero() || !server.loaded_within(left) {
        return still;
    }
    let found = server.targets();
    match answer(server, asked, found, false) {
        Step::Done(outcome) => outcome,
        Step::Waiting(_) => still,
    }
}

/// [`ask`] and, when the server is still loading, [`ask_again`] within `limit` in all; `say`
/// gets the line the status bar shows in between.
pub fn request(
    server: &mut impl RefactoringServer,
    asked: &Asked,
    limit: std::time::Duration,
    mut say: impl FnMut(&str),
) -> Outcome {
    let started = std::time::Instant::now();
    match ask(server, asked) {
        Step::Done(outcome) => outcome,
        Step::Waiting(line) => {
            say(&line);
            ask_again(server, asked, limit.saturating_sub(started.elapsed()))
        }
    }
}

fn answer(
    server: &mut impl RefactoringServer,
    asked: &Asked,
    found: Result<Vec<RefactoringTarget>, String>,
    may_wait: bool,
) -> Step {
    use idecraft_languages::actions::loading_message;
    let refactoring = asked.refactoring;
    let name = server.name();
    let loading = server.loading();
    let none = || refactoring.none_message(&name, asked.selected.as_deref());
    // A place without an action is no place.
    let found = found.map(|mut places| {
        places.retain(|p| !p.actions.is_empty());
        places
    });
    if refactoring == Refactoring::Any {
        // Refactor This lists the server's refactorings at the one place it asks about and
        // IDECraft's own (behaviour 4a); a server that fails, is slow or still loads leaves the
        // own ones, and the status bar says beside the list what is missing.
        let (actions, note) = match found {
            Ok(mut places) if places.len() == 1 => (places.remove(0).actions, None),
            Ok(places) if !places.is_empty() => return Step::Done(Outcome::Places(places)),
            Ok(_) if loading => (Vec::new(), Some(loading_message(&name))),
            Ok(_) => (Vec::new(), None),
            Err(e) => (
                Vec::new(),
                Some(failed_message(
                    refactoring.name(),
                    &e,
                    loading.then_some(name.as_str()),
                )),
            ),
        };
        let own = idecraft_languages::actions::refactor_this_own(&actions, asked.on_name);
        if actions.is_empty() && own.is_empty() {
            return Step::Done(Outcome::Status(note.unwrap_or_else(none)));
        }
        return Step::Done(Outcome::Menu { actions, own, note });
    }
    let said = match found {
        Ok(places) if !places.is_empty() => return Step::Done(Outcome::Places(places)),
        // IDECraft's own refactoring says itself what it refuses (SPEC-0330 behaviour 14); that
        // does not depend on the server.
        Ok(_) => match server.own_refusal() {
            Some(own) => return Step::Done(Outcome::Status(own)),
            None => none(),
        },
        Err(e) => format!("{} failed: {e}", refactoring.name()),
    };
    if !loading {
        Step::Done(Outcome::Status(said))
    } else if may_wait {
        Step::Waiting(refactoring.repeating_message(&name))
    } else {
        Step::Done(Outcome::Status(loading_message(&name)))
    }
}

/// The language server of one file for one refactoring's question, on the editor's text
/// (`SPEC-0095` v9 behaviour 11, v10 behaviour 12).
pub struct FileServer {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    text: String,
    range: idecraft_languages::insight::Range,
    refactoring: Refactoring,
    name: String,
    runtime: tokio::runtime::Runtime,
}

impl FileServer {
    /// The server of `path` in the project `root` for `refactoring` at `range` of `text`.
    pub fn new(
        root: &std::path::Path,
        path: &std::path::Path,
        text: String,
        range: idecraft_languages::insight::Range,
        refactoring: Refactoring,
    ) -> Result<Self, String> {
        let name = idecraft_languages::LanguageId::detect(path)
            .and_then(|language| idecraft_languages::servers::resolve_for(language, root).ok())
            .map_or_else(
                || "The language server".to_owned(),
                |s| s.candidate.program.to_owned(),
            );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            text,
            range,
            refactoring,
            name,
            runtime,
        })
    }
}

impl RefactoringServer for FileServer {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn loading(&self) -> bool {
        idecraft_languages::pool::loading(&self.root, &self.path).is_some()
    }

    fn targets(&mut self) -> Result<Vec<RefactoringTarget>, String> {
        // The server has a limit in all, after which the status bar says so (behaviour 11).
        self.runtime
            .block_on(idecraft_languages::actions::refactoring_targets_within(
                &self.root,
                &self.path,
                Some(&self.text),
                &self.range,
                self.refactoring,
                std::time::Duration::from_secs(30),
                REFACTORING_LIMIT,
            ))
            .map_err(|e| e.to_string())
    }

    fn own_refusal(&mut self) -> Option<String> {
        idecraft_languages::actions::own_refusal(
            &self.path,
            Some(&self.text),
            &self.range,
            self.refactoring,
        )
    }

    fn loaded_within(&mut self, wait: std::time::Duration) -> bool {
        self.runtime
            .block_on(idecraft_languages::pool::loaded_within(
                &self.root, &self.path, wait,
            ))
    }
}

/// Lines a preview shows per file before "… n more" (`SPEC-0076` v3 behaviour 7).
pub const PREVIEW_LINES: usize = 12;

/// The preview of a planned action: per file a header line (path relative to `root`), then the
/// changed lines as `- old` and `+ new`, at most [`PREVIEW_LINES`] per file (behaviour 7). `text_of`
/// gives a file's current text (the editor's, else the disk's).
pub fn preview(
    files: &[idecraft_languages::rename::FileEdits],
    root: &std::path::Path,
    text_of: impl Fn(&std::path::Path) -> Option<String>,
) -> Vec<String> {
    use idecraft_languages::rename::{Edit, apply_edits};
    let mut out = Vec::new();
    for file in files {
        out.push(
            file.path
                .strip_prefix(root)
                .unwrap_or(&file.path)
                .display()
                .to_string(),
        );
        let Some(text) = text_of(&file.path) else {
            out.push("  (cannot be read)".into());
            continue;
        };
        let lines: Vec<&str> = text.split('\n').collect();
        // Edits sharing lines form one hunk.
        let mut hunks: Vec<(usize, usize, Vec<&Edit>)> = Vec::new();
        for e in &file.edits {
            let (a, b) = (e.range.start_line as usize, e.range.end_line as usize);
            match hunks.last_mut() {
                Some((_, end, list)) if a <= *end => {
                    *end = (*end).max(b);
                    list.push(e);
                }
                _ => hunks.push((a, b, vec![e])),
            }
        }
        let mut changed = Vec::new();
        for (a, b, list) in hunks {
            let b = b.min(lines.len().saturating_sub(1));
            let old = lines.get(a..=b).map(|l| l.join("\n")).unwrap_or_default();
            let shifted: Vec<Edit> = list
                .iter()
                .map(|e| {
                    let mut e = (*e).clone();
                    e.range.start_line -= a as u64;
                    e.range.end_line -= a as u64;
                    e
                })
                .collect();
            let Ok(new) = apply_edits(&old, &shifted) else {
                changed.push("  (changed since)".to_owned());
                continue;
            };
            changed.extend(old.split('\n').map(|l| format!("- {l}")));
            changed.extend(new.split('\n').map(|l| format!("+ {l}")));
        }
        let more = changed.len().saturating_sub(PREVIEW_LINES);
        out.extend(changed.into_iter().take(PREVIEW_LINES));
        if more > 0 {
            out.push(format!("  … {more} more"));
        }
    }
    out
}

/// An option in the submenu of an own finding (`SPEC-0125` v2 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingOption {
    /// Fix every finding of the inspection in the file.
    FixAll,
    /// Suppress the inspection for the statement, method or class.
    Suppress(idecraft_languages::inspections::SuppressScope),
    /// Turn the inspection off in the project's profile.
    Disable,
    /// Run only this inspection over a scope (`SPEC-0125` v3).
    RunOnScope,
    /// Suppress every inspection for the enclosing class (`SPEC-0125` v3).
    SuppressAll,
    /// Set the inspection to "no highlighting, only fix" (`SPEC-0125` v3, `SPEC-0315` v2).
    FixOnly,
    /// Open the inspection's settings.
    Settings,
}

impl FindingOption {
    /// The option a submenu label stands for.
    pub fn of_label(label: &str) -> Option<Self> {
        use idecraft_languages::inspections::SuppressScope;
        Some(match label {
            l if l.starts_with("Fix all '") => Self::FixAll,
            "Suppress for statement" => Self::Suppress(SuppressScope::Statement),
            // Kotlin (SPEC-0098 v13 behaviour 28).
            "Suppress for declaration" => Self::Suppress(SuppressScope::Statement),
            "Suppress for file" => Self::Suppress(SuppressScope::File),
            "Suppress for method" => Self::Suppress(SuppressScope::Method),
            "Suppress for class" => Self::Suppress(SuppressScope::Class),
            "Disable inspection" => Self::Disable,
            "Run inspection on…" => Self::RunOnScope,
            "Suppress all inspections for class" => Self::SuppressAll,
            "Disable highlighting, keep fix" => Self::FixOnly,
            "Edit inspection settings" => Self::Settings,
            _ => return None,
        })
    }
}

/// The own findings at byte `at` of `text` as ⌥↩ rows with submenus (`SPEC-0125` v2 behaviour 8):
/// the targets (row title, inspection id, byte offset of the finding), the submenus, and the titles
/// of rows for findings without a fix (they need a row of their own).
#[allow(clippy::type_complexity)]
pub fn finding_options(
    path: &std::path::Path,
    text: &str,
    at: usize,
) -> (
    Vec<(String, &'static str, usize)>,
    Vec<crate::intentions_view::RowOptions>,
    Vec<String>,
) {
    finding_options_with(path, text, at, Vec::new())
}

/// [`finding_options`] with `extra` findings at the place besides the file's own, such as the
/// unused declarations of the project's index (`SPEC-0098` v12 behaviour 26). A fix that runs
/// Safe Delete has no "Fix all in file": every deletion is checked on its own.
#[allow(clippy::type_complexity)]
pub fn finding_options_with(
    path: &std::path::Path,
    text: &str,
    at: usize,
    extra: Vec<idecraft_languages::inspections::Finding>,
) -> (
    Vec<(String, &'static str, usize)>,
    Vec<crate::intentions_view::RowOptions>,
    Vec<String>,
) {
    use idecraft_languages::inspections::{findings_at, name_of};
    let java = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("java"));
    let kotlin = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("kt"));
    let mut targets = Vec::new();
    let mut options = Vec::new();
    let mut rows = Vec::new();
    for f in findings_at(path, text, at).into_iter().chain(extra) {
        // Spelling findings keep their own list (learn a word, SPEC-0163).
        if f.id.starts_with("grazie.") {
            continue;
        }
        let name = name_of(f.id);
        let fix = f.fix.as_ref().filter(|x| x.learn.is_none());
        let title = fix.map_or_else(|| name.to_owned(), |x| x.title.clone());
        if targets.iter().any(|(t, _, _)| *t == title) {
            continue;
        }
        let mut labels = Vec::new();
        let safe_delete =
            fix.is_some_and(|x| x.action == Some(idecraft_languages::reference_index::SAFE_DELETE));
        // A duplicate property key is removed place by place (SPEC-0155 v2 behaviour 5).
        if fix.is_some() && !safe_delete && idecraft_languages::inspections::fix_all_applies(f.id) {
            labels.push(format!("Fix all '{name}' problems in file"));
        }
        labels.push("Run inspection on…".into());
        if java {
            labels.extend(
                [
                    "Suppress for statement",
                    "Suppress for method",
                    "Suppress for class",
                    "Suppress all inspections for class",
                ]
                .map(String::from),
            );
        }
        // SPEC-0098 v13 behaviour 28: `@Suppress` on the declaration, the class or the file,
        // each only where it applies.
        if kotlin && f.id.starts_with("kotlin.") {
            use idecraft_languages::inspections::{SuppressScope, suppress};
            for (label, scope) in [
                ("Suppress for declaration", SuppressScope::Statement),
                ("Suppress for class", SuppressScope::Class),
                ("Suppress for file", SuppressScope::File),
            ] {
                if suppress(text, f.start, f.id, scope).is_some() {
                    labels.push(label.into());
                }
            }
        }
        // SPEC-0125 v3 behaviour 8: only with a fix, and not twice.
        if fix.is_some() && f.level != idecraft_languages::inspections::Level::Info {
            labels.push("Disable highlighting, keep fix".into());
        }
        labels.push("Disable inspection".into());
        labels.push("Edit inspection settings".into());
        if fix.is_none() {
            rows.push(title.clone());
        }
        options.push(crate::intentions_view::RowOptions {
            title: title.clone(),
            labels,
            only_options: fix.is_none(),
        });
        targets.push((title, f.id, f.start));
    }
    (targets, options, rows)
}

/// The preview of an own fix: the changed lines of `text` before (`- `) and after (`+ `) its byte
/// edits (`SPEC-0125` v2 behaviour 9, IMP-0307).
pub fn own_fix_preview(text: &str, edits: &[idecraft_languages::jvm::TextEdit]) -> Vec<String> {
    let mut sorted: Vec<&idecraft_languages::jvm::TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.range.start);
    let (Some(first), Some(last)) = (sorted.first(), sorted.iter().map(|e| e.range.end).max())
    else {
        return Vec::new();
    };
    let start = text[..first.range.start.min(text.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let last = last.min(text.len());
    let end = text[last..].find('\n').map_or(text.len(), |i| last + i);
    let mut after = String::new();
    let mut at = start;
    for e in &sorted {
        let (a, b) = (e.range.start.clamp(at, end), e.range.end.clamp(at, end));
        after.push_str(&text[at..a]);
        after.push_str(&e.text);
        at = b;
    }
    after.push_str(&text[at..end]);
    let mut out: Vec<String> = text[start..end].lines().map(|l| format!("- {l}")).collect();
    out.extend(after.lines().map(|l| format!("+ {l}")));
    if after.is_empty() {
        out.push("+ (line removed)".into());
    }
    out
}

#[cfg(test)]
mod tests {
    // SPEC-0125 v2 AK-5 (the workbench's part), IMP-0307.
    #[test]
    fn finding_options_and_own_fix_preview() {
        use super::{FindingOption, finding_options, own_fix_preview};
        let text = "class A {\n    void f(String s) {\n        if (s == \"x\") {}\n        try { g(); } catch (Exception e) {}\n    }\n}\n";
        let path = std::path::Path::new("A.java");
        let at = text.find("==").unwrap();
        let (targets, options, rows) = finding_options(path, text, at);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].1, "java.string-equality");
        assert!(
            rows.is_empty(),
            "the finding has a fix, so its fix is the row"
        );
        assert!(options[0].labels[0].starts_with("Fix all '"));
        assert_eq!(
            options[0].labels[1..],
            [
                "Run inspection on…",
                "Suppress for statement",
                "Suppress for method",
                "Suppress for class",
                "Suppress all inspections for class",
                "Disable highlighting, keep fix",
                "Disable inspection",
                "Edit inspection settings"
            ]
        );
        assert_eq!(
            FindingOption::of_label("Run inspection on…"),
            Some(FindingOption::RunOnScope)
        );
        assert_eq!(
            FindingOption::of_label("Suppress all inspections for class"),
            Some(FindingOption::SuppressAll)
        );
        assert_eq!(
            FindingOption::of_label("Disable highlighting, keep fix"),
            Some(FindingOption::FixOnly)
        );
        let at = text.find("{}\n    }").unwrap();
        let (targets, options, rows) = finding_options(path, text, at);
        assert_eq!(targets[0].1, "java.empty-catch");
        assert_eq!(rows, [targets[0].0.clone()]);
        assert!(options[0].only_options);
        assert_eq!(
            FindingOption::of_label("Disable inspection"),
            Some(FindingOption::Disable)
        );
        assert!(
            !options[0]
                .labels
                .iter()
                .any(|l| l == "Disable highlighting, keep fix"),
            "no fix, nothing to keep"
        );
        assert_eq!(
            FindingOption::of_label(&options[0].labels[1]),
            Some(FindingOption::Suppress(
                idecraft_languages::inspections::SuppressScope::Statement
            ))
        );
        let edit = idecraft_languages::jvm::TextEdit {
            range: text.find("s ==").unwrap()..text.find(") {}").unwrap(),
            text: "\"x\".equals(s)".into(),
        };
        assert_eq!(
            own_fix_preview(text, &[edit]),
            [
                "-         if (s == \"x\") {}",
                "+         if (\"x\".equals(s)) {}"
            ]
        );
    }

    use super::*;

    fn action(title: &str, kind: Option<&str>, preferred: bool, disabled: bool) -> CodeAction {
        CodeAction {
            title: title.into(),
            kind: kind.map(str::to_owned),
            preferred,
            disabled: disabled.then(|| "not here".into()),
            fixes: Vec::new(),
        }
    }

    #[test]
    fn orders_preferred_fixes_refactorings_then_disabled() {
        let list = order(vec![
            action("Extract variable", Some("refactor.extract"), false, false),
            action("Disabled", Some("quickfix"), true, true),
            action("Other", None, false, false),
            action("Insert ';'", Some("quickfix"), false, false),
            action("Preferred refactor", Some("refactor"), true, false),
        ]);
        let titles: Vec<&str> = list.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Preferred refactor",
                "Insert ';'",
                "Extract variable",
                "Other",
                "Disabled"
            ]
        );
    }

    /// SPEC-0076 v4 behaviour 8 (OBS-0027): a title jdtls sends twice is listed once.
    #[test]
    fn a_repeated_title_is_listed_once() {
        let list = order(vec![
            action("Generate toString()", Some("source"), false, false),
            action("Organize imports", Some("source"), false, false),
            action("Generate toString()", Some("source"), false, false),
        ]);
        let titles: Vec<&str> = list.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Generate toString()", "Organize imports"]);
    }

    #[test]
    fn filters_by_all_words_ignoring_case() {
        let list = vec![
            action("Extract subexpression to variable", None, false, false),
            action("Extract function", None, false, false),
            action("Swap operands", None, false, false),
        ];
        assert_eq!(filter(&list, ""), [0, 1, 2]);
        assert_eq!(filter(&list, "ext"), [0, 1]);
        assert_eq!(filter(&list, "EXTRACT var"), [0]);
        assert!(filter(&list, "inline").is_empty());
    }

    #[test]
    fn skips_disabled_rows_and_tells_fixes_from_intentions() {
        let list = vec![
            action("Off", Some("quickfix"), false, true),
            action("On", Some("refactor"), false, false),
        ];
        assert_eq!(first_enabled(&list, &[0, 1]), Some(1));
        assert_eq!(first_enabled(&list, &[0]), None);
        assert!(is_fix(&list[0]));
        assert!(!is_fix(&list[1]));
    }

    /// SPEC-0095 v2 AK-5: the rows of the expression list.
    #[test]
    fn target_rows_are_short_single_lines() {
        use idecraft_languages::actions::RefactoringTarget;
        use idecraft_languages::insight::Range;
        let t = |line: u64, text: &str| RefactoringTarget {
            range: Range {
                start_line: line,
                start_character: 0,
                end_line: line,
                end_character: 1,
            },
            text: text.into(),
            actions: vec![CodeAction {
                title: "Extract into variable".into(),
                kind: Some("refactor.extract".into()),
                preferred: false,
                disabled: None,
                fixes: Vec::new(),
            }],
        };
        let rows = target_choices(vec![
            t(1, "side * side"),
            t(2, "f(a,\n      b)"),
            t(3, &"x".repeat(70)),
            t(4, "side * side"),
        ]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels[0], "side * side");
        assert_eq!(labels[1], "f(a, b)");
        assert_eq!(labels[2], format!("{}…", "x".repeat(60)));
        assert_eq!(labels[3], "side * side (line 5)");
        assert_eq!(rows[3].range.start_line, 4);
    }

    /// A language server that answers from a script (SPEC-0095 v10 AK-17).
    #[derive(Default)]
    struct Scripted {
        /// Its answers in order; after the last it answers empty.
        answers: std::collections::VecDeque<Result<Vec<RefactoringTarget>, String>>,
        /// Whether it is still loading the project.
        loading: bool,
        /// Whether waiting for it ends with a loaded project.
        loads: bool,
        own_refusal: Option<String>,
        /// How often it was asked and how long it was waited for.
        asked: usize,
        waited: Vec<std::time::Duration>,
    }

    impl RefactoringServer for Scripted {
        fn name(&self) -> String {
            "jdtls".into()
        }

        fn loading(&self) -> bool {
            self.loading
        }

        fn targets(&mut self) -> Result<Vec<RefactoringTarget>, String> {
            self.asked += 1;
            self.answers.pop_front().unwrap_or(Ok(Vec::new()))
        }

        fn own_refusal(&mut self) -> Option<String> {
            self.own_refusal.clone()
        }

        fn loaded_within(&mut self, wait: std::time::Duration) -> bool {
            self.waited.push(wait);
            if self.loads {
                self.loading = false;
            }
            self.loads
        }
    }

    fn place(titles: &[&str]) -> RefactoringTarget {
        RefactoringTarget {
            range: idecraft_languages::insight::Range {
                start_line: 11,
                start_character: 22,
                end_line: 11,
                end_character: 22,
            },
            text: "totalValue".into(),
            actions: titles
                .iter()
                .map(|t| action(t, Some("refactor.inline"), false, false))
                .collect(),
        }
    }

    fn asked(refactoring: Refactoring) -> Asked {
        Asked {
            refactoring,
            selected: None,
            on_name: true,
        }
    }

    const LIMIT: std::time::Duration = std::time::Duration::from_secs(45);

    /// What `server` ends in for `refactoring`, and the lines said in between.
    fn run(server: &mut Scripted, refactoring: Refactoring) -> (Outcome, Vec<String>) {
        let mut said = Vec::new();
        let outcome = request(server, &asked(refactoring), LIMIT, |line| {
            said.push(line.to_owned())
        });
        (outcome, said)
    }

    /// SPEC-0095 v10 AK-17 (OBS-0027 W1b): a server that is still loading the project is not
    /// taken at its word, the status bar says so, and the question is repeated once when it is
    /// ready; a ready server's empty answer is said in the words of behaviour 7; an answer with
    /// actions is the list.
    #[test]
    fn a_loading_server_is_asked_again_and_a_ready_one_is_taken_at_its_word() {
        use Refactoring::Inline;
        let waiting = "jdtls is still loading the project; Inline is asked again when it is ready.";
        let still = "jdtls is still loading the project; try again when it is ready.";
        let nothing = Inline.none_message("jdtls", None);
        assert!(nothing.starts_with("jdtls cannot inline anything at the caret."));

        // Not ready and not ready in time: one line while waiting, one at the end.
        let mut server = Scripted {
            loading: true,
            ..Default::default()
        };
        let (outcome, said) = run(&mut server, Inline);
        assert_eq!(outcome, Outcome::Status(still.into()));
        assert_eq!(said, [waiting]);
        assert_eq!(server.asked, 1);
        assert!(server.waited.len() == 1 && server.waited[0] <= LIMIT);

        // Not ready, then ready with actions: the repeated question's list.
        let variants = place(&[
            "Inline Method: all usages, remove the method",
            "Inline Method",
        ]);
        let mut server = Scripted {
            loading: true,
            loads: true,
            answers: [Ok(Vec::new()), Ok(vec![variants.clone()])].into(),
            ..Default::default()
        };
        let (outcome, said) = run(&mut server, Inline);
        assert_eq!(outcome, Outcome::Places(vec![variants.clone()]));
        assert_eq!(said, [waiting]);
        assert_eq!(server.asked, 2);

        // Not ready, then ready and empty: the ready server's words, asked twice and no more.
        let mut server = Scripted {
            loading: true,
            loads: true,
            ..Default::default()
        };
        let (outcome, _) = run(&mut server, Inline);
        assert_eq!(outcome, Outcome::Status(nothing.clone()));
        assert_eq!(server.asked, 2);

        // A question given up at its limit while the server loads waits as well; with no time
        // left it is not repeated.
        let mut server = Scripted {
            loading: true,
            loads: true,
            answers: [Err("jdtls: no answer within 45 s".into())].into(),
            ..Default::default()
        };
        assert_eq!(
            ask(&mut server, &asked(Inline)),
            Step::Waiting(waiting.into())
        );
        let outcome = ask_again(&mut server, &asked(Inline), std::time::Duration::ZERO);
        assert_eq!(outcome, Outcome::Status(still.into()));
        assert_eq!((server.asked, server.waited.len()), (1, 0));

        // Ready and empty: said at once, nothing repeated.
        let mut server = Scripted::default();
        let (outcome, said) = run(&mut server, Inline);
        assert_eq!(outcome, Outcome::Status(nothing));
        assert!(said.is_empty() && server.asked == 1 && server.waited.is_empty());

        // Ready with actions: the list; ready and failing: the failure.
        let mut server = Scripted {
            answers: [Ok(vec![variants.clone()])].into(),
            ..Default::default()
        };
        assert_eq!(
            run(&mut server, Inline).0,
            Outcome::Places(vec![variants.clone()])
        );
        let mut server = Scripted {
            answers: [Err("jdtls: no answer within 45 s".into())].into(),
            ..Default::default()
        };
        assert_eq!(
            run(&mut server, Inline).0,
            Outcome::Status("Inline failed: jdtls: no answer within 45 s".into())
        );

        // IDECraft's own refusal does not wait for the server.
        let mut server = Scripted {
            loading: true,
            own_refusal: Some("Cannot inline a parameter.".into()),
            ..Default::default()
        };
        let (outcome, said) = run(&mut server, Inline);
        assert_eq!(
            outcome,
            Outcome::Status("Cannot inline a parameter.".into())
        );
        assert!(said.is_empty());

        // Refactor This keeps its own entries and says beside them that the server loads.
        let mut server = Scripted {
            loading: true,
            ..Default::default()
        };
        let (outcome, said) = run(&mut server, Refactoring::Any);
        let Outcome::Menu { actions, own, note } = outcome else {
            panic!("Refactor This lists its own entries");
        };
        assert!(actions.is_empty() && own.iter().any(|o| o.title == "Rename…"));
        assert_eq!(note.as_deref(), Some(still));
        assert!(said.is_empty() && server.waited.is_empty());

        // The lines of the other paths.
        assert_eq!(progress_message("Inline", None), "Inline…");
        assert_eq!(
            progress_message("Inline", Some("jdtls")),
            "Inline… (jdtls is still loading the project)"
        );
        assert_eq!(
            no_actions_message(None),
            "No context actions available here."
        );
        assert_eq!(no_actions_message(Some("jdtls")), still);
        assert_eq!(
            failed_message(
                "Inline Method",
                "jdtls: no answer within 45 s",
                Some("jdtls")
            ),
            format!("Inline Method failed: jdtls: no answer within 45 s. {still}")
        );
    }

    /// SPEC-0095 v10 AK-18: whatever the server answers and whatever its state, a refactoring's
    /// request ends in something the user sees — places with actions, a list with rows or a
    /// line of text — and asks the server at most twice.
    #[test]
    fn every_request_ends_in_a_list_or_a_status_line() {
        use Refactoring::*;
        let answers: [Result<Vec<RefactoringTarget>, String>; 4] = [
            Ok(Vec::new()),
            Err("jdtls: no answer within 45 s".into()),
            Ok(vec![place(&["Inline Method"])]),
            Ok(vec![place(&["Extract to variable"]), place(&["Extract"])]),
        ];
        let mut checked = 0;
        for refactoring in [Variable, Constant, Field, Method, Parameter, Inline, Any] {
            for (loading, loads) in [(false, false), (true, false), (true, true)] {
                for first in &answers {
                    for second in &answers {
                        for own_refusal in [None, Some("Select an expression.".to_owned())] {
                            let mut server = Scripted {
                                answers: [first.clone(), second.clone()].into(),
                                loading,
                                loads,
                                own_refusal,
                                ..Default::default()
                            };
                            let (outcome, said) = run(&mut server, refactoring);
                            let case = format!(
                                "{refactoring:?} loading={loading} loads={loads} {first:?} {second:?}"
                            );
                            assert!(outcome.visible(), "nothing shown: {case}: {outcome:?}");
                            assert!(server.asked <= 2, "asked too often: {case}");
                            // A wait is announced, and only a loading server is waited for.
                            assert_eq!(said.len(), server.waited.len(), "{case}");
                            assert!(loading || said.is_empty(), "{case}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 7 * 3 * 4 * 4 * 2);
        // What is no outcome: no place, a place without an action, an empty list, a blank line.
        assert!(!Outcome::Places(Vec::new()).visible());
        assert!(!Outcome::Places(vec![place(&[])]).visible());
        assert!(!Outcome::Status(" ".into()).visible());
        let empty = Outcome::Menu {
            actions: Vec::new(),
            own: Vec::new(),
            note: None,
        };
        assert!(!empty.visible());
    }

    #[test]
    fn refactorings_by_kind_and_title() {
        let a = |title: &str, kind: Option<&str>| CodeAction {
            title: title.into(),
            kind: kind.map(str::to_owned),
            preferred: false,
            disabled: None,
            fixes: Vec::new(),
        };
        let ra_var = a("Extract into variable", Some("refactor.extract"));
        let ra_fn = a("Extract into function", Some("refactor.extract"));
        let clangd_var = a("Extract subexpression to variable", Some("refactor"));
        let inline = a("Inline variable", Some("refactor.inline"));
        let fix = a("Change 'x' to 'y'", Some("quickfix"));
        use Refactoring::*;
        assert!(Variable.matches(&ra_var) && Variable.matches(&clangd_var));
        assert!(!Variable.matches(&ra_fn) && Method.matches(&ra_fn));
        assert!(Inline.matches(&inline) && !Inline.matches(&ra_var));
        assert!(!Any.matches(&fix) && Any.matches(&inline));
        assert_eq!(Refactoring::of_action("ExtractMethod"), Some(Method));
        assert_eq!(Refactoring::of_action("Find"), None);
    }

    #[test]
    fn preview_shows_changed_lines_per_file() {
        use idecraft_languages::insight::Range;
        use idecraft_languages::rename::{Edit, FileEdits};
        use std::path::{Path, PathBuf};
        let at = |line: u64, from: u64, to: u64, old: &str, new: &str| Edit {
            range: Range {
                start_line: line,
                start_character: from,
                end_line: line,
                end_character: to,
            },
            old_text: old.into(),
            new_text: new.into(),
        };
        let files = vec![
            FileEdits {
                path: PathBuf::from("/p/src/a.c"),
                edits: vec![at(1, 13, 13, "", ";")],
            },
            FileEdits {
                path: PathBuf::from("/p/src/b.c"),
                edits: (0..8).map(|l| at(l, 0, 1, "x", "y")).collect(),
            },
        ];
        let text_of = |p: &Path| {
            Some(if p.ends_with("a.c") {
                "int main() {\n    int x = 1\n}\n".to_owned()
            } else {
                "x\n".repeat(8)
            })
        };
        let lines = preview(&files, Path::new("/p"), text_of);
        assert_eq!(
            lines[..3],
            ["src/a.c", "-     int x = 1", "+     int x = 1;"]
        );
        assert_eq!(lines[3], "src/b.c");
        assert_eq!(lines.len(), 4 + PREVIEW_LINES + 1);
        assert_eq!(lines.last().unwrap(), "  … 4 more");
        let unreadable = preview(&files[..1], Path::new("/p"), |_| None);
        assert_eq!(unreadable, ["src/a.c", "  (cannot be read)"]);
    }
}
