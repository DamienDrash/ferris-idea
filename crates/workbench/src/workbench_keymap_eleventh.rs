//! The eleventh package of workbench actions that exist under their keymap id
//! (`docs/research/KEYMAP_COVERAGE.md`): Tab out of a closing bracket or quote (`SPEC-0056` v6
//! behaviour 22), a Python live template expanded by Tab (`SPEC-0072` v16 behaviour 53), external
//! documentation (`SPEC-0077` v8 behaviour 7), the context actions as quick actions (`SPEC-0076`
//! v9 behaviour 13), Find Usages with a choice of scope (`SPEC-0293` v10 behaviour 28), the
//! declaration of a variable's type from the debugger (`SPEC-0091` v20 behaviour 45) and the
//! system's character palette (`SPEC-0061` v18 behaviour 68).

use gpui::{Context, Window, prelude::*};

use super::WorkbenchView;
use crate::everywhere_model::Filter;

/// The status text where the system has no character palette (`SPEC-0061` v18 behaviour 68).
pub const NO_CHARACTER_PALETTE: &str =
    "Emoji & Symbols is the macOS character palette; this system has none";

/// The scopes Find Usages offers before it searches (`SPEC-0293` v10 behaviour 28).
pub const USAGE_SCOPES: [&str; 2] = ["Project Files", "This File"];

impl WorkbenchView {
    /// The actions of the eleventh package by id; false for another id or where the action is
    /// not available (then the next action of the chord is tried). `BraceOrQuoteOut` and
    /// `ExpandLiveTemplateByTab` are keys of the editor (`tab_out_or_expand`).
    pub(super) fn run_eleventh_package_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match id {
            "ExternalJavaDoc" => {
                if self.active_editor().is_none() {
                    return false;
                }
                self.external_doc(window, cx);
                true
            }
            "QuickActionPopup" => {
                if self.active_editor().is_none() {
                    return false;
                }
                self.show_intentions(window, cx);
                true
            }
            "ShowSettingsAndFindUsages" => {
                if self.active_editor().is_none() {
                    return false;
                }
                self.find_usages_with_scope(window, cx);
                true
            }
            "XDebugger.JumpToTypeSource" => self.jump_to_type_source(window, cx),
            "EmojiAndSymbols" => {
                if cfg!(target_os = "macos") {
                    window.show_character_palette();
                } else {
                    self.status = Some(NO_CHARACTER_PALETTE.into());
                    cx.notify();
                }
                true
            }
            _ => false,
        }
    }

    /// Tab in the editor without a list, template or parameter info: a Python live template
    /// named exactly as the word before the caret is expanded (`ExpandLiveTemplateByTab`), else
    /// the caret steps over the closing bracket or quote after it (`BraceOrQuoteOut`), each when
    /// the keymap in use binds the key to it. False leaves the key to the editor.
    pub(super) fn tab_out_or_expand(&mut self, bound: &[&str], cx: &mut Context<Self>) -> bool {
        let (Some(editor), Some(path)) = (self.active_editor(), self.active_path()) else {
            return false;
        };
        let (line, column, caret, single) = {
            let e = editor.read(cx);
            if e.ghost().is_some() {
                return false;
            }
            let s = e.status();
            let line = e
                .state
                .buffer
                .line(s.line - 1)
                .unwrap_or_default()
                .trim_end_matches(['\n', '\r'])
                .to_owned();
            let primary = e.state.carets.primary();
            let single = e.state.carets.carets().len() == 1 && primary.anchor == primary.head;
            (line, s.column - 1, e.caret(), single)
        };
        if !single {
            return false;
        }
        if bound.contains(&"ExpandLiveTemplateByTab")
            && path.extension().is_some_and(|x| x == "py")
            && let Some(item) = template_by_tab(&line, column)
        {
            self.show_completion_items(vec![item], false, cx);
            self.accept_completion(false, cx);
            crate::key_diagnostics::note_ran("ExpandLiveTemplateByTab", "workbench");
            return true;
        }
        if bound.contains(&"BraceOrQuoteOut") && brace_or_quote_out(&line, column) {
            editor.update(cx, |e, cx| e.select_range(caret + 1..caret + 1, cx));
            crate::key_diagnostics::note_ran("BraceOrQuoteOut", "workbench");
            return true;
        }
        false
    }

    /// `ExternalJavaDoc` (⇧F1): opens the first https link of the documentation at the caret in
    /// the browser, or says there is none (`SPEC-0077` v8 behaviour 7).
    fn external_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(here) = self.here(cx) else {
            return;
        };
        if idecraft_languages::LanguageId::detect(&here.path).is_none() {
            self.status = Some(NO_EXTERNAL_DOC.into());
            cx.notify();
            return;
        }
        let Some(editor) = self.active_editor() else {
            return;
        };
        let text = editor.read(cx).state.buffer.text();
        let root = self.root.clone();
        let remote = self.remote_ask(&here.path);
        cx.spawn_in(window, async move |this, cx| {
            let from = here.clone();
            let found = cx
                .background_spawn(async move {
                    if let Some(remote) = remote {
                        return remote.hover(from.line, from.column);
                    }
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    rt.block_on(idecraft_languages::insight::hover_in(
                        &root,
                        &from.path,
                        Some(&text),
                        from.line as u64,
                        from.column as u64,
                        std::time::Duration::from_secs(30),
                    ))
                    .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update_in(cx, |this, _, cx| {
                let link = match found {
                    Ok(Some(h)) => external_doc_link(&h.text),
                    Ok(None) => None,
                    Err(e) => {
                        this.status = Some(format!("Documentation failed: {e}"));
                        cx.notify();
                        return;
                    }
                };
                match link {
                    Some(url) => {
                        this.status = Some(format!("Opened {url}"));
                        cx.open_url(&url);
                    }
                    None => this.status = Some(NO_EXTERNAL_DOC.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// `XDebugger.JumpToTypeSource` (⇧F4 in the Variables list): Go to Class with the selected
    /// variable's type (`SPEC-0091` v20 behaviour 45). False without a selected variable, so
    /// the chord's next action runs.
    fn jump_to_type_source(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(view) = self
            .debug_view
            .clone()
            .filter(|v| v.read(cx).focus().contains_focused(window, cx))
        else {
            return false;
        };
        let Some(kind) = view.read(cx).selected_variable_type() else {
            return false;
        };
        match type_to_find(&kind) {
            Some(name) => {
                self.open_everywhere(Filter::Classes, window, cx);
                if let Some(everywhere) = self.everywhere.clone() {
                    everywhere.update(cx, |v, cx| v.set_query(&name, cx));
                }
            }
            None => {
                self.status = Some(format!("`{kind}` has no declaration to go to"));
                cx.notify();
            }
        }
        true
    }
}

/// The status text when the documentation at the caret has no https link (`SPEC-0077` v8
/// behaviour 7).
pub const NO_EXTERNAL_DOC: &str = "No external documentation link for the symbol at the caret";

/// Whether Tab at char `column` of `line` steps over a closing bracket or quote: the character
/// after the caret closes a pair whose opening character stands before the caret on the same
/// line (`SPEC-0056` v6 behaviour 22).
pub fn brace_or_quote_out(line: &str, column: usize) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let Some(&next) = chars.get(column) else {
        return false;
    };
    let before = &chars[..column.min(chars.len())];
    if before.iter().all(|c| c.is_whitespace()) {
        return false;
    }
    match next {
        '"' | '\'' | '`' => before.iter().filter(|c| **c == next).count() % 2 == 1,
        ')' | ']' | '}' => {
            let open = match next {
                ')' => '(',
                ']' => '[',
                _ => '{',
            };
            let mut depth = 0i32;
            for c in before.iter().rev() {
                if *c == next {
                    depth += 1;
                } else if *c == open {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
            }
            false
        }
        _ => false,
    }
}

/// The Python live template named exactly as the word before char `column` of `line`
/// (`SPEC-0072` v16 behaviour 53); a template that only starts with the word is not taken.
pub fn template_by_tab(
    line: &str,
    column: usize,
) -> Option<idecraft_languages::insight::CompletionItem> {
    let chars: Vec<char> = line.chars().collect();
    let column = column.min(chars.len());
    let mut start = column;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let typed: String = chars[start..column].iter().collect();
    idecraft_languages::python_completion::live_items(line, column)
        .into_iter()
        .find(|i| i.label == typed)
}

/// The first https link of a documentation text: `[text](https://…)`, `<https://…>` or a bare
/// `https://…`. Other schemes (`http:`, `file:`, `jdt:`) are not opened (`SPEC-0077` v8
/// behaviour 7).
pub fn external_doc_link(markdown: &str) -> Option<String> {
    let at = markdown.find("https://")?;
    let url: String = markdown[at..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ')' | '>' | '"' | '\'' | '`' | ']'))
        .collect();
    let url = url.trim_end_matches(['.', ',', ';', ':']).to_owned();
    (url.len() > "https://".len()).then_some(url)
}

/// The class name to look for from a debugger's type text: the simple name without package,
/// type arguments or array brackets; `None` for a primitive type (`SPEC-0091` v20 behaviour 45).
pub fn type_to_find(kind: &str) -> Option<String> {
    let base = kind.split('<').next().unwrap_or(kind).trim();
    let base = base.trim_end_matches("[]").trim_end_matches("...").trim();
    let simple = base.rsplit(['.', '$']).next().unwrap_or(base).trim();
    const PRIMITIVES: [&str; 9] = [
        "int", "long", "short", "byte", "char", "boolean", "float", "double", "void",
    ];
    if simple.is_empty()
        || PRIMITIVES.contains(&simple)
        || !simple.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(simple.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0056 v6 AK-17.
    #[test]
    fn tab_steps_out_of_a_pair_on_the_line() {
        assert!(brace_or_quote_out("foo(a, b)", 8));
        assert!(brace_or_quote_out("x = [1, 2]", 9));
        assert!(brace_or_quote_out("s = \"abc\"", 8));
        assert!(brace_or_quote_out("f(g(x))", 5), "the inner pair");
        assert!(brace_or_quote_out("f(g(x))", 6), "the outer pair");
        assert!(
            !brace_or_quote_out("    )", 4),
            "only indentation before it"
        );
        assert!(
            !brace_or_quote_out("a)", 1),
            "no opening character on the line"
        );
        assert!(
            !brace_or_quote_out("s = \"a\" + \"", 10),
            "the quote opens a string"
        );
        assert!(
            !brace_or_quote_out("foo(a, b)", 3),
            "not a closing character"
        );
        assert!(!brace_or_quote_out("foo()", 5), "nothing after the caret");
    }

    /// SPEC-0072 v16 AK-37.
    #[test]
    fn tab_expands_only_a_template_named_as_typed() {
        let main = template_by_tab("main", 4).expect("main is a template");
        assert!(main.snippet && main.insert.contains("__main__"), "{main:?}");
        assert!(template_by_tab("mai", 3).is_none(), "a prefix is not taken");
        assert!(
            template_by_tab("x = main", 8).is_none(),
            "not at a statement start"
        );
        assert!(template_by_tab("", 0).is_none());
    }

    /// SPEC-0077 v8 AK-13.
    #[test]
    fn the_first_https_link_is_opened() {
        assert_eq!(
            external_doc_link(
                "Returns a [`Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html)."
            )
            .as_deref(),
            Some("https://doc.rust-lang.org/std/vec/struct.Vec.html")
        );
        assert_eq!(
            external_doc_link("See <https://docs.python.org/3/library/os.html>, more").as_deref(),
            Some("https://docs.python.org/3/library/os.html")
        );
        assert_eq!(
            external_doc_link("[List](jdt://contents/rt.jar/java.util/List.class)"),
            None
        );
        assert_eq!(
            external_doc_link("see http://example.org/doc"),
            None,
            "only https"
        );
        assert_eq!(external_doc_link("https://"), None);
    }

    /// SPEC-0091 v20 AK-28.
    #[test]
    fn the_type_to_look_for() {
        assert_eq!(
            type_to_find("java.util.ArrayList<java.lang.String>").as_deref(),
            Some("ArrayList")
        );
        assert_eq!(
            type_to_find("com.shop.Order$Line[]").as_deref(),
            Some("Line")
        );
        assert_eq!(type_to_find("Order").as_deref(), Some("Order"));
        assert_eq!(type_to_find("int"), None);
        assert_eq!(type_to_find("int[]"), None);
        assert_eq!(type_to_find(""), None);
        assert_eq!(type_to_find("(lambda)"), None);
    }
}
