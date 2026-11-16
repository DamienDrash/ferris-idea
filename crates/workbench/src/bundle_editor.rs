//! The resource bundle editor (`SPEC-0212`): the keys of a bundle's `.properties` files in a list,
//! one value field per locale for the selected key; values are written back by the workbench.

use gpui::{Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use idecraft_languages::properties::Bundle;

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::workbench_view::c;

/// What the dialog asks the workbench to do.
pub enum BundleEditorEvent {
    /// Set `key` to `value` in the file of locale index `locale` (behaviour 4).
    Set {
        /// Locale index.
        locale: usize,
        /// Key.
        key: String,
        /// Value.
        value: String,
    },
    /// Remove `key` from every file (behaviour 5).
    Remove(String),
    /// Closed.
    Close,
}

/// The dialog (behaviour 1).
pub struct BundleEditor {
    bundle: Bundle,
    filter: Entity<EditorView>,
    values: Vec<Entity<EditorView>>,
    /// The selected key.
    selected: Option<String>,
    /// The field for a new key while ⌘N asks for one.
    new_key: Option<Entity<EditorView>>,
    /// Delete was pressed once on the selected key.
    confirm_remove: bool,
    note: Option<String>,
    focus: FocusHandle,
}

impl EventEmitter<BundleEditorEvent> for BundleEditor {}

impl BundleEditor {
    /// The dialog on `bundle`, with its first key selected.
    pub fn new(bundle: Bundle, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(EditorView::field);
        cx.observe(&filter, |this: &mut Self, _, cx| this.filter_changed(cx))
            .detach();
        let values = bundle
            .locales
            .iter()
            .map(|_| cx.new(EditorView::field))
            .collect();
        let selected = bundle.keys.first().map(|k| k.key.clone());
        let mut view = Self {
            bundle,
            filter,
            values,
            selected: None,
            new_key: None,
            confirm_remove: false,
            note: None,
            focus: cx.focus_handle(),
        };
        view.show_key(selected, cx);
        view
    }

    /// The bundle shown.
    pub fn bundle(&self) -> &Bundle {
        &self.bundle
    }

    /// Title, e.g. `Resource Bundle 'messages'`.
    pub fn title(&self) -> String {
        format!("Resource Bundle '{}'", self.bundle.base)
    }

    /// Focus handle of the key list.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The selected key.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The note under the list, e.g. the question before removing.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The keys the filter keeps, in order (behaviour 2).
    pub fn visible_keys(&self, cx: &gpui::App) -> Vec<String> {
        let filter = self.filter.read(cx).state.buffer.text().to_lowercase();
        self.bundle
            .keys
            .iter()
            .filter(|k| k.key.to_lowercase().contains(filter.trim()))
            .map(|k| k.key.clone())
            .collect()
    }

    /// The value field of locale index `locale` (tests).
    pub fn value_field(&self, locale: usize) -> Option<Entity<EditorView>> {
        self.values.get(locale).cloned()
    }

    /// The bundle again after a write; the selection stays if the key still exists.
    pub fn reload(&mut self, bundle: Bundle, cx: &mut Context<Self>) {
        self.bundle = bundle;
        let keep = self
            .selected
            .clone()
            .filter(|k| self.bundle.keys.iter().any(|b| &b.key == k))
            .or_else(|| self.bundle.keys.first().map(|k| k.key.clone()));
        self.selected = None;
        self.show_key(keep, cx);
    }

    /// Selects `key` and fills the value fields (behaviour 3).
    fn show_key(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        self.confirm_remove = false;
        self.note = None;
        let values: Vec<String> = match key
            .as_ref()
            .and_then(|k| self.bundle.keys.iter().find(|b| &b.key == k))
        {
            Some(k) => k
                .values
                .iter()
                .map(|v| v.clone().unwrap_or_default())
                .collect(),
            None => vec![String::new(); self.values.len()],
        };
        for (field, value) in self.values.iter().zip(values) {
            field.update(cx, |f, cx| f.set_text(&value, cx));
        }
        self.selected = key;
        cx.notify();
    }

    /// Selects `key` after writing the values typed for the current one (behaviour 4).
    pub fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.commit(cx);
        self.show_key(Some(key.to_owned()), cx);
    }

    fn filter_changed(&mut self, cx: &mut Context<Self>) {
        let visible = self.visible_keys(cx);
        if let Some(first) = visible.first()
            && !self.selected.as_ref().is_some_and(|s| visible.contains(s))
        {
            let first = first.clone();
            self.select(&first, cx);
        }
        cx.notify();
    }

    /// Writes every value field that differs from the bundle (behaviour 4).
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.selected.clone() else {
            return;
        };
        let Some(entry) = self.bundle.keys.iter().find(|b| b.key == key).cloned() else {
            return;
        };
        for (locale, field) in self.values.iter().enumerate() {
            let typed = field.read(cx).state.buffer.text();
            let before = entry.values.get(locale).cloned().flatten();
            // An empty field of a missing key stays missing.
            let changed = match &before {
                Some(v) => *v != typed,
                None => !typed.is_empty(),
            };
            if changed {
                cx.emit(BundleEditorEvent::Set {
                    locale,
                    key: key.clone(),
                    value: typed,
                });
            }
        }
    }

    /// Asks for a new key (⌘N, behaviour 5).
    pub fn start_new_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(EditorView::field);
        window.focus(&field.read(cx).focus());
        self.new_key = Some(field);
        self.note = Some("New key: type it and press Enter".into());
        cx.notify();
    }

    /// Adds the typed key with an empty value to the default file.
    pub fn add_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let key = key.trim();
        if key.is_empty() {
            return false;
        }
        if self.bundle.keys.iter().any(|k| k.key == key) {
            self.note = Some(format!("`{key}` exists already"));
            cx.notify();
            return false;
        }
        self.commit(cx);
        self.new_key = None;
        self.selected = Some(key.to_owned());
        cx.emit(BundleEditorEvent::Set {
            locale: 0,
            key: key.to_owned(),
            value: String::new(),
        });
        true
    }

    /// Delete on the key list: the first asks, the second removes the key (behaviour 5).
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.selected.clone() else {
            return;
        };
        if self.confirm_remove {
            self.confirm_remove = false;
            self.note = None;
            cx.emit(BundleEditorEvent::Remove(key));
        } else {
            self.confirm_remove = true;
            self.note = Some(format!(
                "Press Delete again to remove `{key}` from every file"
            ));
        }
        cx.notify();
    }

    fn step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self.visible_keys(cx);
        if visible.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|s| visible.iter().position(|k| k == s))
            .map_or(0, |i| {
                (i as isize + by).clamp(0, visible.len() as isize - 1) as usize
            });
        let key = visible[at].clone();
        self.select(&key, cx);
        window.focus(&self.focus);
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let command = k.modifiers.platform || k.modifiers.control;
        match k.key.as_str() {
            "n" if command => self.start_new_key(window, cx),
            "up" => self.step(-1, window, cx),
            "down" => self.step(1, window, cx),
            "enter" => {
                if let Some(field) = self.new_key.clone() {
                    let typed = field.read(cx).state.buffer.text();
                    if self.add_key(&typed, cx) {
                        window.focus(&self.focus);
                    }
                } else {
                    self.commit(cx);
                }
            }
            "escape" => {
                if self.new_key.take().is_some() {
                    self.note = None;
                    window.focus(&self.focus);
                    cx.notify();
                } else {
                    self.commit(cx);
                    cx.emit(BundleEditorEvent::Close);
                }
            }
            // Only on the key list: in a field Delete edits the text.
            "delete" | "backspace" if self.focus.is_focused(window) => self.remove_selected(cx),
            _ => {
                if self.confirm_remove {
                    self.confirm_remove = false;
                    self.note = None;
                    cx.notify();
                }
                return;
            }
        }
        cx.stop_propagation();
    }
}

impl Render for BundleEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let warning = c(p.status_text(crate::palette::Status::Warning));
        let locale_name = |l: &str| {
            if l.is_empty() {
                "default".to_owned()
            } else {
                l.to_owned()
            }
        };
        let visible = self.visible_keys(cx);
        let rows = visible.into_iter().enumerate().map(|(i, key)| {
            let entry = self.bundle.keys.iter().find(|k| k.key == key);
            let missing: Vec<String> = entry
                .map(|e| {
                    e.missing()
                        .into_iter()
                        .map(|m| locale_name(&self.bundle.locales[m]))
                        .collect()
                })
                .unwrap_or_default();
            let on = self.selected.as_deref() == Some(key.as_str());
            let name = key.clone();
            div()
                .id(("bundle-key", i))
                .px_2()
                .rounded(px(4.))
                .flex()
                .gap_2()
                .whitespace_nowrap()
                .when(on, |d| d.bg(c(p.tree_selection)))
                .when(!on, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                .child(key)
                .when(!missing.is_empty(), |d| {
                    d.child(
                        div()
                            .text_color(warning)
                            .child(format!("⚠ {}", missing.join(", "))),
                    )
                })
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    this.select(&name, cx);
                    window.focus(&this.focus);
                }))
        });
        let selected = self
            .selected
            .as_ref()
            .and_then(|s| self.bundle.keys.iter().find(|k| &k.key == s));
        let fields = self
            .values
            .iter()
            .enumerate()
            .map(|(n, field)| {
                let missing = selected.is_some_and(|k| k.values[n].is_none());
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(80.))
                            .text_color(c(p.ui_muted))
                            .child(locale_name(&self.bundle.locales[n])),
                    )
                    .child(crate::metrics::field_frame().flex_1().child(field.clone()))
                    .when(missing, |d| {
                        d.child(div().text_color(warning).child("missing"))
                    })
            })
            .collect::<Vec<_>>();
        div()
            .id("bundle-editor")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(760.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.title()),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .w(px(280.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(crate::metrics::field_frame().child(self.filter.clone()))
                            .when_some(self.new_key.clone(), |d, f| {
                                d.child(crate::metrics::field_frame().child(f))
                            })
                            .child(
                                div()
                                    .id("bundle-keys")
                                    .h(px(300.))
                                    .overflow_y_scroll()
                                    .children(rows),
                            ),
                    )
                    .child(div().flex_1().flex().flex_col().gap_1().children(fields)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(c(p.ui_dim))
                    .child(self.note.clone().unwrap_or_else(|| {
                        "Enter saves · ⌘N new key · Delete removes the key · Esc closes".into()
                    })),
            )
    }
}
