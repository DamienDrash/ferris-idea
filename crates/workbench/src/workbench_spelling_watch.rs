//! Open editors follow a change of the spelling dictionaries (`SPEC-0159` v5 behaviour 3): made in
//! Settings › Editor › Natural Languages, or in the saved settings file by anything else, such as
//! `set_ide_settings` over MCP. The typo and variant marks change without a restart or an edit.

use std::time::{Duration, SystemTime};

use gpui::{App, AppContext, Context};

use super::WorkbenchView;

/// Time between two looks at the settings file and the dictionaries chosen.
pub const SPELLING_WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// Takes over the dictionaries `codes` found in the settings file (`SPEC-0159` v5 behaviour 3):
/// they are used from now on and the settings in effect hold them, so the next change in the
/// window does not write the old ones back.
pub fn take_spelling_dictionary(codes: &str, cx: &mut App) {
    let mut settings = crate::appearance::ide_settings(cx);
    codes.clone_into(&mut settings.editor.spelling_dictionary);
    cx.set_global(crate::appearance::CurrentIdeSettings(settings));
    ferris_idea_languages::grazie::choose_dictionary(codes);
}

/// Takes over the grammar scope found in the settings file (`SPEC-0155` v8 behaviour 14), as
/// [`take_spelling_dictionary`] does for the dictionaries.
pub fn take_grammar_settings(grammar: &ferris_idea_core::GrammarSettings, cx: &mut App) {
    let mut settings = crate::appearance::ide_settings(cx);
    settings.editor.grammar = grammar.clone();
    cx.set_global(crate::appearance::CurrentIdeSettings(settings));
    ferris_idea_languages::grazie::set_grammar_settings(grammar);
}

impl WorkbenchView {
    /// Every [`SPELLING_WATCH_INTERVAL`]: when the settings file changed on disk and names other
    /// dictionaries than those in use, takes them over; when the dictionaries in use changed
    /// (here, in another window, or from the file), reads their word lists and the variant table
    /// off the UI thread, then checks every open editor again. The variant table is read at start
    /// too, so the first check does not wait for it.
    pub(crate) fn watch_spelling(cx: &mut Context<Self>) {
        cx.background_spawn(async { ferris_idea_languages::spelling_variants::prepare() })
            .detach();
        cx.spawn(async move |this, cx| {
            let mut checked = ferris_idea_languages::grazie::dictionary_generation();
            let mut stamp: Option<SystemTime> = None;
            loop {
                cx.background_executor()
                    .timer(SPELLING_WATCH_INTERVAL)
                    .await;
                let Ok(file) = this.update(cx, |_, cx| crate::appearance::ide_settings_file(cx))
                else {
                    break;
                };
                let seen = stamp;
                let read = cx
                    .background_spawn(async move {
                        let file = file?;
                        let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok()?;
                        (Some(modified) != seen).then(|| {
                            let saved = ferris_idea_core::load_ide_settings(&file);
                            (
                                modified,
                                saved.editor.spelling_dictionary,
                                saved.editor.grammar,
                            )
                        })
                    })
                    .await;
                if let Some((modified, codes, grammar)) = read {
                    // The first look only notes the file: the window applied it at start.
                    let first = stamp.is_none();
                    stamp = Some(modified);
                    // The scope of the grammar check, e.g. from `set_ide_settings` (SPEC-0155 v8
                    // behaviour 14).
                    if !first && grammar != ferris_idea_languages::grazie::grammar_settings() {
                        let taken = this.update(cx, |_, cx| take_grammar_settings(&grammar, cx));
                        if taken.is_err() {
                            break;
                        }
                    }
                    if !first && codes != ferris_idea_languages::grazie::chosen_dictionary() {
                        let taken = this.update(cx, |_, cx| take_spelling_dictionary(&codes, cx));
                        if taken.is_err() {
                            break;
                        }
                    }
                }
                let now = ferris_idea_languages::grazie::dictionary_generation();
                if now == checked {
                    continue;
                }
                checked = now;
                cx.background_spawn(async {
                    let _ = ferris_idea_languages::grazie::Dictionary::current();
                    ferris_idea_languages::spelling_variants::prepare();
                })
                .await;
                if this
                    .update(cx, |this, cx| this.reinspect_editors(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}
