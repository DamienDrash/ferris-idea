//! The ER diagram over the editor area (`SPEC-0111`): table and view boxes at their layered
//! positions and the foreign keys as lines with cardinality labels; scrolls both ways, a click
//! selects a table. v2: zoom (toolbar, ⌘=/⌘-/⌘0/⇧⌘0, ⌘ + wheel), fitted when opened, boxes dragged
//! with the mouse, layout back to the layers, export to local files and the clipboard only.
//! v8: lines go around boxes (the routes come from `er_layout`), and the export menu offers yEd
//! GraphML and draw.io (`SPEC-0111` behaviours 11 and 16).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ferris_idea_database::ErModel;
use ferris_idea_database::er_layout::{
    self, DiagramLayout, HEADER_HEIGHT, LABEL_CHAR_WIDTH, LINE_HEIGHT, fit_zoom, layout, move_box,
    zoom_in, zoom_out,
};
use gpui::{
    ClipboardItem, Context, EventEmitter, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, Point, ScrollWheelEvent, Window, canvas, div, point,
    prelude::*, px, rgb,
};

use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::tips::Tipped;
use crate::workbench_view::c;

/// What the view asks the workbench to do.
pub enum ErEvent {
    /// Close the diagram.
    Close,
    /// Show this in the status bar (export results).
    Status(String),
}

/// An export target of the diagram (v2 behaviour 11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErExport {
    /// Mermaid `erDiagram`.
    Mermaid,
    /// PlantUML entities.
    PlantUml,
    /// Graphviz DOT.
    Dot,
    /// SVG of the drawing as it stands.
    Svg,
    /// PNG at a scale.
    Png(f32),
    /// yEd GraphML of the drawing as it stands (v8).
    GraphMl,
    /// draw.io file of the drawing as it stands (v8).
    DrawIo,
}

impl ErExport {
    /// Every target, in menu order; the editor formats after the images (v8), so the earlier
    /// entries keep their places.
    pub const ALL: [ErExport; 9] = [
        ErExport::Mermaid,
        ErExport::PlantUml,
        ErExport::Dot,
        ErExport::Svg,
        ErExport::Png(1.),
        ErExport::Png(2.),
        ErExport::Png(4.),
        ErExport::GraphMl,
        ErExport::DrawIo,
    ];

    /// Menu label.
    pub fn label(self) -> String {
        match self {
            ErExport::Mermaid => "Mermaid".into(),
            ErExport::PlantUml => "PlantUML".into(),
            ErExport::Dot => "Graphviz DOT".into(),
            ErExport::Svg => "SVG".into(),
            ErExport::Png(s) => format!("PNG {s}×"),
            ErExport::GraphMl => "yEd GraphML".into(),
            ErExport::DrawIo => "draw.io".into(),
        }
    }

    fn extension(self) -> String {
        match self {
            ErExport::Mermaid => "mmd".into(),
            ErExport::PlantUml => "puml".into(),
            ErExport::Dot => "dot".into(),
            ErExport::Svg => "svg".into(),
            ErExport::Png(1.) => "png".into(),
            ErExport::Png(s) => format!("{s}x.png"),
            ErExport::GraphMl => "graphml".into(),
            ErExport::DrawIo => "drawio".into(),
        }
    }
}

/// Where an export of `connection` goes: `.ferris-idea/diagrams/<connection>.<ext>`, the name made
/// file-safe.
pub fn export_path(root: &Path, connection: &str, export: ErExport) -> PathBuf {
    let safe: String = connection
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join(".ferris-idea/diagrams")
        .join(format!("{safe}.{}", export.extension()))
}

/// What a key pressed with ⌘ (Ctrl) does to the zoom (`SPEC-0111` behaviours 7 and 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoomKey {
    In,
    Out,
    Actual,
    Fit,
}

/// The zoom command of `key` pressed with ⌘ (Ctrl). The system reports ⇧⌘0 as `0` with shift, or
/// as the character the layout types with ⇧0 and without shift: `)` on US layouts, `=` on German
/// ones. `us` is the US key in the place of `key` on the current layout
/// ([`ferris_idea_ui::keymap::us_key_for`]); where it is `)` the key is ⇧0 and fits, although its
/// character alone would zoom in (`SPEC-0111` v4, hand test 120).
fn zoom_key(key: &str, shift: bool, us: Option<char>) -> Option<ZoomKey> {
    match key {
        "+" => Some(ZoomKey::In),
        "-" => Some(ZoomKey::Out),
        ")" => Some(ZoomKey::Fit),
        "0" if shift => Some(ZoomKey::Fit),
        _ if us == Some(')') => Some(ZoomKey::Fit),
        "=" => Some(ZoomKey::In),
        "0" => Some(ZoomKey::Actual),
        _ if us == Some('0') => Some(ZoomKey::Actual),
        _ => None,
    }
}

/// The boxes in the order Tab visits them: layer by layer, top to bottom (`SPEC-0111` v5
/// behaviour 13). Boxes are in entity order.
pub fn tab_order(model: &ErModel) -> Vec<usize> {
    let mut order: Vec<usize> = (0..model.entities.len()).collect();
    order.sort_by_key(|&i| (model.entities[i].layer, model.entities[i].row));
    order
}

/// The box Tab (or ⇧Tab with `back`) selects after `selected`: from none the first (last), and
/// both wrap.
pub fn next_box(order: &[usize], selected: Option<usize>, back: bool) -> Option<usize> {
    let n = order.len();
    if n == 0 {
        return None;
    }
    let at = selected.and_then(|s| order.iter().position(|&i| i == s));
    let k = match (at, back) {
        (None, false) => 0,
        (None, true) => n - 1,
        (Some(k), false) => (k + 1) % n,
        (Some(k), true) => (k + n - 1) % n,
    };
    Some(order[k])
}

/// The export entry ↓ (`down`) or ↑ marks after `at` in a menu of `len` entries; both wrap.
pub fn next_export(at: usize, len: usize, down: bool) -> usize {
    match len {
        0 => 0,
        _ if down => (at + 1) % len,
        _ => (at + len - 1) % len,
    }
}

/// A box being dragged: its index and where it was grabbed, relative to its corner (canvas units).
#[derive(Debug, Clone, Copy)]
struct Drag {
    index: usize,
    grab: (f32, f32),
}

/// The diagram.
pub struct ErView {
    connection: String,
    model: ErModel,
    layout: DiagramLayout,
    selected: Option<usize>,
    zoom: f32,
    /// Fit as soon as the size of the diagram area is known (v2 behaviour 8).
    fit_pending: bool,
    /// Keep fitting when the diagram area changes size, until the zoom is set by hand (v3, D43: the
    /// area can still grow or shrink after the first paint while the window settles).
    auto_fit: bool,
    /// The area size the zoom was last fitted to.
    fitted_for: (f32, f32),
    /// Size of the diagram area, set while painting.
    viewport: Rc<Cell<(f32, f32)>>,
    /// Window position of the canvas origin, set while painting.
    origin: Rc<Cell<Point<Pixels>>>,
    drag: Option<Drag>,
    export_menu: bool,
    /// The export entry marked for Enter while the menu is open (v5 behaviour 13).
    export_mark: usize,
    root: PathBuf,
    focus: FocusHandle,
    /// A dependency diagram of the build tools window (SPEC-0100 v4 behaviour 15): its own
    /// Mermaid flowchart for the Mermaid export, boxes marked "conflict" instead of "view".
    dependencies: Option<String>,
}

impl EventEmitter<ErEvent> for ErView {}

impl ErView {
    /// A diagram of `model`, read from `connection`; exports go under `root`.
    pub fn new(connection: String, model: ErModel, root: PathBuf, cx: &mut Context<Self>) -> Self {
        let layout = layout(&model);
        Self {
            connection,
            model,
            layout,
            selected: None,
            zoom: 1.,
            fit_pending: true,
            auto_fit: true,
            fitted_for: (0., 0.),
            viewport: Rc::new(Cell::new((0., 0.))),
            origin: Rc::new(Cell::new(Point::default())),
            drag: None,
            export_menu: false,
            export_mark: 0,
            root,
            focus: cx.focus_handle(),
            dependencies: None,
        }
    }

    /// The dependency diagram `name` of a module (SPEC-0100 v4 behaviour 15): the artifacts as
    /// boxes in `model` (a marked box is in conflict), `mermaid` the same graph as text for the
    /// Mermaid export.
    pub fn dependency_diagram(
        name: String,
        model: ErModel,
        mermaid: String,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::new(name, model, root, cx);
        view.dependencies = Some(mermaid);
        view
    }

    /// Whether this is a dependency diagram rather than an ER diagram.
    pub fn is_dependency_diagram(&self) -> bool {
        self.dependencies.is_some()
    }

    /// The exports offered: a dependency diagram has no PlantUML or DOT entities, nor the editor
    /// formats of the ER diagram (v8).
    fn exports(&self) -> Vec<ErExport> {
        ErExport::ALL
            .into_iter()
            .filter(|e| {
                self.dependencies.is_none()
                    || !matches!(
                        e,
                        ErExport::PlantUml | ErExport::Dot | ErExport::GraphMl | ErExport::DrawIo
                    )
            })
            .collect()
    }

    /// Focus handle.
    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// The drawing.
    pub fn layout(&self) -> &DiagramLayout {
        &self.layout
    }

    /// The model drawn.
    pub fn model(&self) -> &ErModel {
        &self.model
    }

    /// Current zoom (1 = 100 %).
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Size of the diagram area as last painted (0 before the first paint).
    pub fn viewport(&self) -> (f32, f32) {
        self.viewport.get()
    }

    /// Whether the diagram still waits to be fitted.
    pub fn fit_pending(&self) -> bool {
        self.fit_pending
    }

    /// Selects box `i` (behaviour 4).
    pub fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = (i < self.layout.boxes.len()).then_some(i);
        cx.notify();
    }

    /// Whether the export menu is open.
    pub fn export_menu_open(&self) -> bool {
        self.export_menu
    }

    /// The export entry marked in the open menu, by its label (v5 behaviour 13).
    pub fn marked_export(&self) -> Option<String> {
        self.export_menu
            .then(|| self.exports().get(self.export_mark).map(|e| e.label()))
            .flatten()
    }

    /// Opens or closes the export menu; opening marks the first entry.
    fn toggle_export_menu(&mut self, cx: &mut Context<Self>) {
        self.export_menu = !self.export_menu;
        self.export_mark = 0;
        cx.notify();
    }

    /// Name of the selected table.
    pub fn selected(&self) -> Option<&str> {
        self.selected
            .and_then(|i| self.layout.boxes.get(i))
            .map(|b| b.name.as_str())
    }

    /// One zoom step in (v2 behaviour 7).
    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(zoom_in(self.zoom), cx);
    }

    /// One zoom step out.
    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(zoom_out(self.zoom), cx);
    }

    /// 100 %.
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(1., cx);
    }

    fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(er_layout::ZOOM_MIN, er_layout::ZOOM_MAX);
        self.fit_pending = false;
        self.auto_fit = false;
        cx.notify();
    }

    /// Fits the whole canvas into the diagram area (v2 behaviour 8); waits for the first paint when
    /// the area is not known yet.
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        let (w, h) = self.viewport.get();
        self.auto_fit = true;
        if w <= 0. || h <= 0. {
            self.fit_pending = true;
        } else {
            self.zoom = fit_zoom(self.layout.width, self.layout.height, w, h);
            self.fitted_for = (w, h);
            self.fit_pending = false;
        }
        cx.notify();
    }

    /// Puts every box back at its layered position and fits (v2 behaviour 10).
    pub fn relayout(&mut self, cx: &mut Context<Self>) {
        self.layout = layout(&self.model);
        self.fit(cx);
    }

    /// Moves box `i` to canvas position (`x`, `y`) (v2 behaviour 9).
    pub fn move_box(&mut self, i: usize, x: f32, y: f32, cx: &mut Context<Self>) {
        move_box(&mut self.layout, &self.model, i, x, y);
        cx.notify();
    }

    /// Writes `export` to its file (and text formats to the clipboard); the path written.
    pub fn export(&mut self, export: ErExport, cx: &mut Context<Self>) -> Result<PathBuf, String> {
        use ferris_idea_database::er_editors::{to_drawio, to_graphml};
        use ferris_idea_database::{to_dot, to_mermaid, to_plantuml};
        self.export_menu = false;
        let path = export_path(&self.root, &self.connection, export);
        let bytes = match export {
            ErExport::Mermaid => match &self.dependencies {
                Some(text) => text.clone().into_bytes(),
                None => to_mermaid(&self.model).into_bytes(),
            },
            ErExport::PlantUml => to_plantuml(&self.model).into_bytes(),
            ErExport::Dot => to_dot(&self.model).into_bytes(),
            ErExport::Svg => er_layout::to_svg(&self.layout).into_bytes(),
            ErExport::Png(scale) => er_layout::to_png(&self.layout, scale)?,
            ErExport::GraphMl => to_graphml(&self.layout).into_bytes(),
            ErExport::DrawIo => to_drawio(&self.layout).into_bytes(),
        };
        if !matches!(export, ErExport::Png(_)) {
            cx.write_to_clipboard(ClipboardItem::new_string(
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
        path.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, &bytes))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        cx.notify();
        Ok(path)
    }

    fn export_and_report(&mut self, export: ErExport, cx: &mut Context<Self>) {
        let message = match self.export(export, cx) {
            Ok(path) => {
                let shown = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                let what = if self.dependencies.is_some() {
                    "Dependency diagram"
                } else {
                    "ER diagram"
                };
                if matches!(export, ErExport::Png(_)) {
                    format!("{what} exported to {shown}")
                } else {
                    format!("{what} copied and exported to {shown}")
                }
            }
            Err(e) => format!("Export failed: {e}"),
        };
        cx.emit(ErEvent::Status(message));
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let cmd = k.modifiers.platform || k.modifiers.control;
        let len = self.exports().len();
        match k.key.as_str() {
            "escape" if self.export_menu => {
                self.export_menu = false;
                cx.notify();
            }
            "escape" => cx.emit(ErEvent::Close),
            // SPEC-0111 v5 behaviour 13: the export menu, boxes and layout by keyboard.
            "down" | "up" if self.export_menu => {
                self.export_mark = next_export(self.export_mark, len, k.key == "down");
                cx.notify();
            }
            "enter" if self.export_menu => {
                if let Some(e) = self.exports().get(self.export_mark).copied() {
                    self.export_menu = false;
                    self.export_and_report(e, cx);
                }
            }
            "tab" if !cmd => {
                let order = tab_order(&self.model);
                self.selected = next_box(&order, self.selected, k.modifiers.shift);
                cx.notify();
            }
            "e" if k.modifiers.alt && !cmd => self.toggle_export_menu(cx),
            "l" if k.modifiers.alt && !cmd => self.relayout(cx),
            key if cmd => {
                let us = cx
                    .keyboard_mapper()
                    .get_key_equivalents()
                    .and_then(|e| ferris_idea_ui::keymap::us_key_for(key, e));
                match zoom_key(key, k.modifiers.shift, us) {
                    Some(ZoomKey::Fit) => self.fit(cx),
                    Some(ZoomKey::In) => self.zoom_in(cx),
                    Some(ZoomKey::Out) => self.zoom_out(cx),
                    Some(ZoomKey::Actual) => self.actual_size(cx),
                    None => return,
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Canvas point (unzoomed) of a window position.
    fn canvas_point(&self, position: Point<Pixels>) -> (f32, f32) {
        let o = self.origin.get();
        (
            f32::from(position.x - o.x) / self.zoom,
            f32::from(position.y - o.y) / self.zoom,
        )
    }

    fn on_box_down(&mut self, i: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let (x, y) = self.canvas_point(event.position);
        if let Some(b) = self.layout.boxes.get(i) {
            self.drag = Some(Drag {
                index: i,
                grab: (x - b.x, y - b.y),
            });
        }
        self.select(i, cx);
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let (x, y) = self.canvas_point(event.position);
        self.move_box(drag.index, x - drag.grab.0, y - drag.grab.1, cx);
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !event.modifiers.platform && !event.modifiers.control {
            return;
        }
        let dy = f32::from(event.delta.pixel_delta(px(LINE_HEIGHT)).y);
        if dy > 0. {
            self.zoom_in(cx);
        } else if dy < 0. {
            self.zoom_out(cx);
        }
        cx.stop_propagation();
    }
}

impl Render for ErView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (vw, vh) = self.viewport.get();
        if (self.fit_pending || (self.auto_fit && self.fitted_for != (vw, vh)))
            && vw > 0.
            && vh > 0.
        {
            self.zoom = fit_zoom(self.layout.width, self.layout.height, vw, vh);
            self.fitted_for = (vw, vh);
            self.fit_pending = false;
        }
        let z = self.zoom;
        let p = Palette::current();
        let selected = self.selected().map(str::to_owned);
        let line_colour = c(p.ui_muted);
        let accent = c(p.tab_selected_border);
        let lines = self.layout.lines.clone();
        let mono = gpui::font(crate::fonts::editor_family());
        let summary = if self.dependencies.is_some() {
            let conflicts = self.layout.boxes.iter().filter(|b| b.view).count();
            format!(
                "{} · {} artifact(s), {} dependenc{}{}",
                self.connection,
                self.layout.boxes.len().saturating_sub(1),
                self.model.relationships.len() + self.model.dependencies.len(),
                if self.model.relationships.len() + self.model.dependencies.len() == 1 {
                    "y"
                } else {
                    "ies"
                },
                if conflicts == 0 {
                    String::new()
                } else {
                    format!(", {conflicts} in conflict")
                }
            )
        } else {
            format!(
                "{} · {} table(s), {} relationship(s)",
                self.connection,
                self.layout.boxes.len(),
                self.model.relationships.len()
            )
        };
        let exports = self.exports();
        let marker = if self.dependencies.is_some() {
            "conflict"
        } else {
            "view"
        };
        let outside: Vec<String> = self
            .model
            .outside
            .iter()
            .map(|r| format!("{} → {} (not shown)", r.from, r.to))
            .collect();
        let button = |id: &'static str, label: String| {
            let name = match id {
                "er-zoom-out" => "Zoom Out",
                "er-zoom-in" => "Zoom In",
                "er-actual" => "Actual Size",
                "er-fit" => "Fit",
                "er-layout" => "Layout",
                _ => "Export",
            };
            div()
                .id(id)
                .px_2()
                .rounded(px(4.))
                .hover(|s| s.bg(c(p.tree_hover)))
                .child(label)
                .semantics(
                    Semantics::new(Role::Button)
                        .name(name)
                        .test_id(id)
                        .action(AxAction::Click)
                        .effects(&[Effect::None]),
                )
        };
        let toolbar = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                button("er-zoom-out", "−".into())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.zoom_out(cx))),
            )
            .child(
                div()
                    .w(px(44.))
                    .flex()
                    .justify_center()
                    .text_color(c(p.ui_muted))
                    .child(format!("{:.0} %", z * 100.)),
            )
            .child(
                button("er-zoom-in", "+".into())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.zoom_in(cx))),
            )
            .child(
                button("er-actual", "1:1".into()).on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.actual_size(cx)),
                ),
            )
            .child(
                button("er-fit", "Fit".into())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.fit(cx))),
            )
            .child(
                button("er-layout", "Layout".into())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.relayout(cx))),
            )
            .child(button("er-export", "Export ▾".into()).on_click(
                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.toggle_export_menu(cx)),
            ));
        let header = div()
            .h(px(crate::workbench_view::HEADER_HEIGHT))
            .flex_none()
            // A narrow editor area cuts the header instead of widening the drawing (SPEC-0100 v4).
            .min_w_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(c(p.separator))
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(
                if self.dependencies.is_some() {
                    "Dependency Diagram"
                } else {
                    "ER Diagram"
                },
            ))
            // The summary gives way to the toolbar in a narrow editor area (SPEC-0100 v4).
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(c(p.ui_muted))
                    .child(summary),
            )
            .children(
                outside
                    .into_iter()
                    .map(|o| div().text_color(c(p.ui_dim)).child(o)),
            )
            .child(div().flex_1())
            .child(div().flex_none().child(toolbar))
            .child(
                div()
                    .id("er-close")
                    .tip(crate::tips::declared!(OTHER_TOOL_WINDOWS, "er-close"))
                    .px_2()
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(p.tree_hover)))
                    .child("×")
                    .on_click(
                        cx.listener(|_, _: &gpui::ClickEvent, _, cx| cx.emit(ErEvent::Close)),
                    ),
            );
        let mark = self.export_mark;
        let menu = self.export_menu.then(|| {
            div()
                .absolute()
                .top(px(crate::workbench_view::HEADER_HEIGHT))
                .right(px(32.))
                .py_1()
                .bg(c(p.island))
                .border_1()
                .border_color(c(p.separator))
                .rounded(px(6.))
                .shadow_md()
                .children(exports.into_iter().enumerate().map(|(i, e)| {
                    div()
                        .id(("er-export-item", i))
                        .px_3()
                        .py(px(2.))
                        .when(i == mark, |d| d.bg(c(p.tree_selection)))
                        .when(i != mark, |d| d.hover(|s| s.bg(c(p.tree_hover))))
                        .child(e.label())
                        .semantics(
                            Semantics::new(Role::ListItem)
                                .name(e.label())
                                .test_id(format!("er-export-{}", e.extension()))
                                .selected(i == mark)
                                .action(AxAction::Click)
                                .effects(&[Effect::Write]),
                        )
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.export_and_report(e, cx)
                        }))
                }))
        });
        let sel = selected.clone();
        let origin = self.origin.clone();
        let edges = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let o = bounds.origin;
                origin.set(o);
                for l in &lines {
                    let on = sel.as_deref().is_some_and(|s| s == l.from || s == l.to);
                    let width = if on { 2. } else { 1.2 };
                    let colour = if on { accent } else { line_colour };
                    let at = |(x, y): (f32, f32)| point(o.x + px(x * z), o.y + px(y * z));
                    // Dependencies are drawn as dashes along each segment.
                    let segments: Vec<((f32, f32), (f32, f32))> =
                        l.points.windows(2).map(|w| (w[0], w[1])).collect();
                    if l.dependency {
                        for (a, b) in segments {
                            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                            let n = (len / 9.).floor().max(1.) as usize;
                            for k in (0..n).step_by(2) {
                                let t0 = k as f32 / n as f32;
                                let t1 = ((k + 1) as f32 / n as f32).min(1.);
                                let p0 = (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0);
                                let p1 = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
                                let mut path = PathBuilder::stroke(px(width));
                                path.move_to(at(p0));
                                path.line_to(at(p1));
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, colour);
                                }
                            }
                        }
                        continue;
                    }
                    let mut path = PathBuilder::stroke(px(width));
                    for (k, pt) in l.points.iter().enumerate() {
                        if k == 0 {
                            path.move_to(at(*pt));
                        } else {
                            path.line_to(at(*pt));
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, colour);
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let viewport = self.viewport.clone();
        let fit_pending = self.fit_pending || self.auto_fit;
        let measure = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if viewport.get() != size {
                    viewport.set(size);
                    if fit_pending {
                        // A refresh while painting is dropped; ask for the frame that fits once
                        // this one is drawn, so the diagram opens fitted without waiting for an
                        // unrelated redraw (SPEC-0111 v7, hand test 120 retest).
                        window.defer(cx, |window, _| window.refresh());
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let text = px(12. * z);
        let small = px(10. * z);
        let labels = self.layout.lines.iter().flat_map(|l| {
            let first = l.points[0];
            let last = l.points[l.points.len() - 1];
            let toward_first = l.points.get(1).copied().unwrap_or(first);
            let toward_last = l.points.iter().rev().nth(1).copied().unwrap_or(last);
            let label = |x: f32, y: f32, text: String| {
                div()
                    .absolute()
                    .left(px(x * z))
                    .top(px((y - 16.) * z))
                    .text_size(small)
                    .text_color(c(p.ui_muted))
                    .child(text)
            };
            let side = |p: (f32, f32), toward: (f32, f32)| {
                if toward.0 >= p.0 { p.0 + 4. } else { p.0 - 22. }
            };
            // The columns of the foreign key over the middle of the line (v5 behaviour 12).
            let half = l.label.chars().count() as f32 * LABEL_CHAR_WIDTH / 2.;
            let columns = div()
                .absolute()
                .left(px((l.label_at.0 - half) * z))
                .top(px((l.label_at.1 - 15.) * z))
                .text_size(small)
                .text_color(c(p.ui_muted))
                .whitespace_nowrap()
                .child(l.label.clone());
            [
                label(side(first, toward_first), first.1, l.to_label.clone()),
                label(side(last, toward_last), last.1, l.from_label.clone()),
                columns,
            ]
        });
        let boxes = self.layout.boxes.iter().enumerate().map(|(i, b)| {
            let on = selected.as_deref() == Some(b.name.as_str());
            let border = if on { accent } else { c(p.separator) };
            div()
                .id(("er-box", i))
                .absolute()
                .left(px(b.x * z))
                .top(px(b.y * z))
                .w(px(b.width * z))
                .h(px(b.height * z))
                .bg(c(p.island))
                .border_1()
                .border_color(border)
                .when(b.view, |d| d.border_dashed())
                .rounded(px(6. * z))
                .overflow_hidden()
                .text_size(text)
                .child(
                    div()
                        .h(px(HEADER_HEIGHT * z))
                        .px(px(8. * z))
                        .flex()
                        .items_center()
                        .gap(px(6. * z))
                        .border_b_1()
                        .border_color(c(p.separator))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(b.name.clone())
                        .when(b.view, |d| {
                            d.child(
                                div()
                                    .text_size(small)
                                    .text_color(c(p.ui_muted))
                                    .child(marker),
                            )
                        }),
                )
                .children(b.lines.iter().map(|(keys, name, ty, nullable)| {
                    let shown = er_layout::fitted_name(b, name, ty);
                    div()
                        .h(px(LINE_HEIGHT * z))
                        .px(px(8. * z))
                        .flex()
                        .items_center()
                        .gap(px(8. * z))
                        .whitespace_nowrap()
                        .font(mono.clone())
                        .text_size(text)
                        .child(
                            div()
                                .w(px(b.key_width * z))
                                .flex_none()
                                .text_color(c(p.ui_muted))
                                .child(keys.clone()),
                        )
                        .child(div().flex_1().min_w_0().overflow_hidden().child(shown))
                        .child(
                            div()
                                .flex_none()
                                .text_color(c(if *nullable { p.ui_dim } else { p.ui_muted }))
                                .child(ty.clone()),
                        )
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| this.on_box_down(i, e, cx)),
                )
        });
        div()
            .id("er-diagram")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, _| this.drag = None),
            )
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div().relative().flex_1().min_h_0().child(measure).child(
                    div()
                        .id("er-scroll")
                        .size_full()
                        .overflow_scroll()
                        .on_scroll_wheel(cx.listener(Self::on_wheel))
                        .child(
                            div()
                                .relative()
                                .flex_none()
                                .w(px(self.layout.width * z))
                                .h(px(self.layout.height * z))
                                .bg(rgb(p.console_background))
                                .child(edges)
                                .children(labels)
                                .children(boxes),
                        ),
                ),
            )
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_files_are_named_after_the_connection() {
        let root = Path::new("/w");
        assert_eq!(
            export_path(root, "ref pg/local", ErExport::Mermaid),
            Path::new("/w/.ferris-idea/diagrams/ref_pg_local.mmd")
        );
        assert_eq!(
            export_path(root, "a", ErExport::Png(2.)),
            Path::new("/w/.ferris-idea/diagrams/a.2x.png")
        );
        assert_eq!(
            export_path(root, "a", ErExport::Png(1.)),
            Path::new("/w/.ferris-idea/diagrams/a.png")
        );
        assert_eq!(ErExport::ALL.len(), 9);
        assert_eq!(ErExport::Png(4.).label(), "PNG 4×");
        assert_eq!(ErExport::ALL[3], ErExport::Svg, "↓ ↓ ↓ still marks SVG");
        assert_eq!(
            export_path(root, "a", ErExport::GraphMl),
            Path::new("/w/.ferris-idea/diagrams/a.graphml")
        );
        assert_eq!(
            export_path(root, "a", ErExport::DrawIo),
            Path::new("/w/.ferris-idea/diagrams/a.drawio")
        );
        assert_eq!(ErExport::GraphMl.label(), "yEd GraphML");
        assert_eq!(ErExport::DrawIo.label(), "draw.io");
    }

    /// SPEC-0111/AK-11 (v5).
    #[test]
    fn tab_follows_the_layout_and_the_export_mark_wraps() {
        use ferris_idea_database::{Entity, ErModel};
        let e = |name: &str, layer: usize, row: usize| Entity {
            name: name.into(),
            columns: vec![],
            layer,
            row,
            view: false,
        };
        let model = ErModel {
            entities: vec![e("orders", 1, 0), e("customers", 0, 0), e("item", 0, 1)],
            relationships: vec![],
            outside: vec![],
            dependencies: vec![],
        };
        let order = tab_order(&model);
        assert_eq!(order, [1, 2, 0]);
        assert_eq!(next_box(&order, None, false), Some(1));
        assert_eq!(next_box(&order, None, true), Some(0));
        assert_eq!(next_box(&order, Some(1), false), Some(2));
        assert_eq!(next_box(&order, Some(0), false), Some(1), "wraps");
        assert_eq!(next_box(&order, Some(1), true), Some(0), "wraps back");
        assert_eq!(next_box(&[], None, false), None);
        assert_eq!(next_export(0, 7, true), 1);
        assert_eq!(next_export(6, 7, true), 0);
        assert_eq!(next_export(0, 7, false), 6);
    }

    /// Hand test 120: ⇧⌘0 fits on every layout, also where ⇧0 types `=` (German) and the key
    /// would otherwise zoom in from 100 % to 125 %.
    #[test]
    fn shift_cmd_zero_fits_on_every_layout() {
        use ferris_idea_ui::keymap::us_key_for;
        // US: no equivalents; the system reports `)` or `0` with shift.
        assert_eq!(zoom_key(")", false, None), Some(ZoomKey::Fit));
        assert_eq!(zoom_key("0", true, None), Some(ZoomKey::Fit));
        assert_eq!(zoom_key("0", false, None), Some(ZoomKey::Actual));
        assert_eq!(zoom_key("=", false, None), Some(ZoomKey::In));
        assert_eq!(zoom_key("+", false, None), Some(ZoomKey::In));
        assert_eq!(zoom_key("-", false, None), Some(ZoomKey::Out));
        assert_eq!(zoom_key("a", false, None), None);
        // German: ⇧0 types `=`, `+` and `-` have their own keys.
        let german: std::collections::HashMap<char, char> =
            [(')', '='), ('=', '*'), ('(', ')'), ('/', 'ß')]
                .into_iter()
                .collect();
        let on = |key: &str| zoom_key(key, false, us_key_for(key, &german));
        assert_eq!(on("="), Some(ZoomKey::Fit));
        assert_eq!(on("+"), Some(ZoomKey::In));
        assert_eq!(on("-"), Some(ZoomKey::Out));
        assert_eq!(on("0"), Some(ZoomKey::Actual));
        assert_eq!(on("ß"), None);
        // A layout with the digits on the shifted row (AZERTY): ⇧0 types `0`, the key alone `à`.
        let azerty: std::collections::HashMap<char, char> =
            [(')', '0'), ('0', 'à')].into_iter().collect();
        let on = |key: &str| zoom_key(key, false, us_key_for(key, &azerty));
        assert_eq!(on("0"), Some(ZoomKey::Fit));
        assert_eq!(on("à"), Some(ZoomKey::Actual));
    }
}
