//! Configuration keys and gutter marks of the bean model in the workbench (`SPEC-0115` v5
//! behaviours 12 to 14, `SPEC-0134` v5 behaviour 7): Go to Declaration, Find Usages and quick
//! documentation on a configuration key, in the code and in the configuration files; the marks of
//! beans and injection points and the resolved values an editor shows.
//!
//! What a key leads to is decided in [`idecraft_languages::spring_nav`]; this module asks it for
//! the caret's place and opens what it answers.

use std::ops::Range;
use std::path::Path;

use gpui::{Context, Window};
use idecraft_languages::find_usages::{Target, UsageSearch, UsageSource};
use idecraft_languages::insight::Location;
use idecraft_languages::spring::SpringModel;
use idecraft_languages::spring_nav::{KeyInfo, KeySite, MarkKind};
use idecraft_languages::ts_structure::MarkTarget;

use super::WorkbenchView;
use crate::decl_marks_model::{BEAN, INJECTION, LineMark};
use crate::nav_model::Place;

/// A place of a file as a one-line range.
fn location(root: &Path, file: &Path, line: usize, column: usize, length: usize) -> Location {
    Location {
        path: root.join(file),
        range: idecraft_languages::insight::Range {
            start_line: line as u64,
            start_character: column as u64,
            end_line: line as u64,
            end_character: (column + length) as u64,
        },
    }
}

/// The gutter marks of the workspace-relative file `rel` and, when `values` is on, the resolved
/// values to show in place of its placeholders (byte ranges of the text the model read).
pub(crate) fn decorations(
    model: &SpringModel,
    root: &Path,
    rel: &Path,
    values: bool,
) -> (Vec<LineMark>, Vec<(Range<usize>, String)>) {
    let marks = model
        .gutter_marks(rel)
        .into_iter()
        .map(|m| LineMark {
            line: m.line,
            glyph: match m.kind {
                MarkKind::Bean => BEAN,
                MarkKind::Injection => INJECTION,
            },
            tip: m.tip,
            targets: m
                .targets
                .into_iter()
                .map(|t| MarkTarget {
                    location: location(root, &t.file, t.line, 0, 0),
                    // File and line first, then what stands there (v6 behaviour 17).
                    name: format!("{}  {}", file_line(&t.file, t.line), t.what),
                })
                .collect(),
        })
        .collect();
    let folds = if values {
        model
            .value_folds(rel)
            .into_iter()
            .map(|f| (f.start..f.end, f.text))
            .collect()
    } else {
        Vec::new()
    };
    (marks, folds)
}

/// The key at the caret: what the model knows about it, and whether the caret is in the code
/// (else in a configuration file).
struct KeyHere {
    info: KeyInfo,
    in_code: bool,
    origin: Place,
    offset: usize,
}

/// The name of a file and its 1-based line, as a chooser's row starts (`SPEC-0115` v6
/// behaviour 17): what the user chooses by comes first and is never cut off by a long folder.
pub(crate) fn file_line(file: &Path, line: usize) -> String {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());
    format!("{name}:{}", line + 1)
}

/// The folder of `file` below the project `root`, as the dimmed end of a chooser's row; empty
/// for a file in the root.
pub(crate) fn folder_of(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// One row of a chooser of defining lines: file and line, its profile and the value as shown.
fn definition_row(d: &idecraft_languages::spring_nav::KeyDefinition) -> String {
    let mut row = file_line(&d.file, d.line);
    if let Some(p) = &d.profile {
        row.push_str(&format!("  profile {p}"));
    }
    if let Some(v) = &d.value {
        row.push_str(&format!("  = {v}"));
    }
    row
}

/// The share of the editor's width a chooser may take.
const CHOOSER_SHARE: f32 = 0.8;

impl WorkbenchView {
    /// The configuration key at the caret of the active editor, when the bean model knows the
    /// place and the editor still has the text the model read there.
    fn config_key_here(&self, cx: &gpui::App) -> Option<KeyHere> {
        let origin = self.here(cx)?;
        let model = self.spring_model.as_ref()?;
        let rel = origin.path.strip_prefix(&self.root).ok()?;
        let editor = self.active_editor()?;
        let (text, offset) = {
            let e = editor.read(cx);
            (e.state.buffer.text(), e.state.carets.primary().head)
        };
        let byte = text
            .char_indices()
            .nth(offset)
            .map_or(text.len(), |(b, _)| b);
        let (written, in_code) = match model.key_at(rel, byte)? {
            KeySite::Reference(v) => (text.get(v.start..v.end)? == v.key, true),
            KeySite::Definition(p) => (p.key.ends_with(text.get(p.start..p.end)?), false),
        };
        // The file changed since the model read it: the place may be another key by now.
        if !written {
            return None;
        }
        Some(KeyHere {
            info: model.key_info_at(rel, byte)?,
            in_code,
            origin,
            offset,
        })
    }

    /// Go to Declaration on a configuration key (`SPEC-0115` v5 behaviour 12): from the code to
    /// the line that defines the key, from a configuration file to the place that reads it. One
    /// target is opened, several are offered in a chooser. `false` when the caret is on no key
    /// the model knows, so the language server is asked as before.
    pub(super) fn goto_config_key(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(here) = self.config_key_here(cx) else {
            return false;
        };
        let key = here.info.key.clone();
        let (title, rows, places): (String, Vec<String>, Vec<Place>) = if here.in_code {
            (
                format!("Definitions of {key}"),
                here.info.definitions.iter().map(definition_row).collect(),
                here.info
                    .definitions
                    .iter()
                    .map(|d| Place {
                        path: self.root.join(&d.file),
                        line: d.line,
                        column: d.column,
                    })
                    .collect(),
            )
        } else {
            (
                format!("Usages of {key}"),
                here.info
                    .usages
                    .iter()
                    .map(|u| format!("{}  {}", file_line(&u.file, u.line), u.key))
                    .collect(),
                here.info
                    .usages
                    .iter()
                    .map(|u| Place {
                        path: self.root.join(&u.file),
                        line: u.line,
                        column: u.column,
                    })
                    .collect(),
            )
        };
        match places.as_slice() {
            [] => {
                self.status = Some(if here.in_code {
                    format!(
                        "The configuration key '{key}' is not defined in the application configuration"
                    )
                } else {
                    format!(
                        "The configuration key '{key}' is not read in the project's Java sources"
                    )
                });
                cx.notify();
            }
            [only] => {
                self.remember_place(cx);
                self.show_place(only, window, cx);
            }
            _ => {
                let at = self.caret_list_place(cx);
                self.show_place_chooser(title, rows, places, at, window, cx);
            }
        }
        true
    }

    /// A chooser of places (`SPEC-0115` v6 behaviour 17): each row is `rows[i]`, which starts
    /// with the file and its line, and after it the file's folder, dimmed. The popup is as wide
    /// as its rows, up to a share of the editor's width, and opens at `at`.
    fn show_place_chooser(
        &mut self,
        title: String,
        rows: Vec<String>,
        places: Vec<Place>,
        at: super::JvmPopupPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders: Vec<String> = places
            .iter()
            .map(|p| folder_of(&self.root, &p.path))
            .collect();
        // The room: the editor the chooser opens over, else the window.
        let room = match &at {
            super::JvmPopupPlace::Caret(path, offset)
            | super::JvmPopupPlace::Mark(path, offset) => self
                .editors
                .get(path)
                .and_then(|e| e.read(cx).caret_spot(*offset))
                .map(|(_, area)| area.right - area.left),
            _ => None,
        }
        .unwrap_or_else(|| f32::from(window.viewport_size().width));
        self.show_jvm_popup(title, rows, window, cx, move |this, i, window, cx| {
            if let Some(p) = places.get(i) {
                this.remember_place(cx);
                this.show_place(p, window, cx);
            }
        });
        self.jvm_popup_place = at;
        if let Some(popup) = &self.jvm_popup {
            popup.update(cx, |p, _| p.set_details(folders, room * CHOOSER_SHARE));
        }
    }

    /// The usages of the configuration key at the caret as the outcome of a usage search
    /// (`SPEC-0115` v5 behaviour 12), for Find Usages and the usages popup: the places in the
    /// code that read the key, with the first defining line as its declaration. `None` when the
    /// caret is on no key the model knows.
    pub(super) fn config_key_usages(&self, cx: &gpui::App) -> Option<(Place, UsageSearch)> {
        let here = self.config_key_here(cx)?;
        let declaration = here
            .info
            .definitions
            .first()
            .map(|d| location(&self.root, &d.file, d.line, d.column, d.length));
        let mut usages: Vec<Location> = here
            .info
            .usages
            .iter()
            .map(|u| location(&self.root, &u.file, u.line, u.column, u.length))
            .collect();
        usages.sort_by(|a, b| {
            (&a.path, a.range.start_line, a.range.start_character).cmp(&(
                &b.path,
                b.range.start_line,
                b.range.start_character,
            ))
        });
        usages.dedup();
        Some((
            here.origin,
            UsageSearch {
                target: Target {
                    name: here.info.key,
                    declaration,
                    outside: None,
                    kind: Some("property".into()),
                },
                usages,
                details: Vec::new(),
                // The bean model's answer is exact, like a language server's.
                source: UsageSource::LanguageServer,
                note: None,
            },
        ))
    }

    /// Quick documentation of the configuration key at the caret (`SPEC-0115` v5 behaviour 12):
    /// its value, the lines that define it and how often it is read. `false` when the caret is
    /// on no key the model knows.
    pub(super) fn show_config_key_doc(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(here) = self.config_key_here(cx) else {
            return false;
        };
        self.doc = Some(super::DocPopup {
            path: here.origin.path,
            offset: here.offset,
            blocks: crate::doc_model::blocks(&SpringModel::key_doc(&here.info)),
            problem: None,
            pointer: None,
        });
        cx.notify();
        true
    }

    /// A click on the mark of a bean or an injection point on `line` (`SPEC-0115` v5 behaviour
    /// 14): its one target, or a choice; without a target the status bar says what the mark says.
    pub(super) fn framework_mark(
        &mut self,
        path: &Path,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mark) = self
            .editors
            .get(path)
            .and_then(|e| e.read(cx).framework_mark(line).cloned())
        else {
            return;
        };
        let places: Vec<Place> = mark
            .targets
            .iter()
            .map(|t| Place {
                path: t.location.path.clone(),
                line: t.location.range.start_line as usize,
                column: t.location.range.start_character as usize,
            })
            .collect();
        match places.as_slice() {
            [] => {
                self.status = Some(mark.tip.replace('\n', "; "));
                cx.notify();
            }
            [only] => {
                self.remember_place(cx);
                self.show_place(only, window, cx);
            }
            _ => {
                let items = mark.targets.iter().map(|t| t.name.clone()).collect();
                // At the mark: under its line, or over it without room (v6 behaviour 17).
                let at = self
                    .editors
                    .get(path)
                    .and_then(|e| {
                        e.read(cx)
                            .state
                            .buffer
                            .position_to_offset(idecraft_editor::Position::new(line, 0))
                            .ok()
                    })
                    .map_or(super::JvmPopupPlace::Top, |offset| {
                        super::JvmPopupPlace::Mark(path.to_path_buf(), offset)
                    });
                self.show_place_chooser(
                    mark.tip.replace('\n', "; "),
                    items,
                    places,
                    at,
                    window,
                    cx,
                );
            }
        }
    }

    /// A click on the mark of a bean or an injection point on 0-based `line` of the active
    /// editor, as the gutter does it (for the window self-test).
    pub fn click_framework_mark(
        &mut self,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.active_path() {
            self.framework_mark(&path, line, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// SPEC-0115 AK-13 and AK-14: the editor gets a bean's and an injection point's mark with
    /// absolute targets, and the values only while the setting is on.
    #[test]
    fn decorations_of_a_file() {
        let files = vec![
            (
                PathBuf::from("src/A.java"),
                "@Service\nclass A {\n    @Value(\"${a.name}\") String n;\n    A(B b) {}\n}\n"
                    .to_owned(),
            ),
            (
                PathBuf::from("src/B.java"),
                "@Component\nclass B {}\n".to_owned(),
            ),
            (
                PathBuf::from("application.properties"),
                "a.name=Shop\n".to_owned(),
            ),
        ];
        let model = idecraft_languages::spring::model_of(&files);
        let root = Path::new("/project");
        let (marks, folds) = decorations(&model, root, Path::new("src/A.java"), true);
        let rows: Vec<(usize, &str, Vec<&Path>)> = marks
            .iter()
            .map(|m| {
                (
                    m.line,
                    m.glyph,
                    m.targets
                        .iter()
                        .map(|t| t.location.path.as_path())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (1, BEAN, vec![]),
                (3, INJECTION, vec![Path::new("/project/src/B.java")])
            ]
        );
        let text = &files[0].1;
        assert_eq!(folds.len(), 1);
        assert_eq!(&text[folds[0].0.clone()], "${a.name}");
        assert_eq!(folds[0].1, "Shop");
        // A chooser's row starts with the file and its line; the folder follows, dimmed (v6
        // behaviour 17, AK-20).
        assert_eq!(marks[1].targets[0].name, "B.java:2  b: B");
        let target = &marks[1].targets[0].location.path;
        assert_eq!(folder_of(root, target), "src");
        assert_eq!(folder_of(root, Path::new("/project/pom.xml")), "");
        assert_eq!(
            file_line(Path::new("src/main/resources/application.yml"), 18),
            "application.yml:19"
        );
        // The setting switched off: marks, no values.
        let (marks, folds) = decorations(&model, root, Path::new("src/A.java"), false);
        assert_eq!(marks.len(), 2);
        assert!(folds.is_empty());
    }
}
