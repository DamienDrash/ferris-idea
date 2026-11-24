//! M3.4 spike for ADR-0003 criterion 4: two windows, each split into a file tree, a text pane and a
//! table with 100 000 rows. Without `--interactive` both windows drive themselves (scroll the tree
//! and the table, expand and collapse the whole tree, move the split) and report frame intervals;
//! with it the panes are left to the mouse: click folders, drag the dividers, scroll.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, Context, MouseButton, MouseMoveEvent, ScrollStrategy,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, div, point, prelude::*, px,
    relative, rgb, size, uniform_list,
};

const WINDOWS: usize = 2;
const FRAMES: usize = 600;
const WARM_UP: usize = 10;
const ROWS: usize = 100_000;
/// Every this many frames the whole tree is expanded or collapsed.
const TOGGLE_EVERY: usize = 40;

static FINISHED: AtomicUsize = AtomicUsize::new(0);

struct Node {
    name: String,
    depth: usize,
    dir: bool,
    children: Vec<usize>,
}

/// Directory tree read once; folders named `target` and dot folders are skipped.
fn read_tree(root: &Path) -> Vec<Node> {
    fn walk(path: &Path, depth: usize, nodes: &mut Vec<Node>) -> usize {
        let ix = nodes.len();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        nodes.push(Node {
            name,
            depth,
            dir: path.is_dir(),
            children: Vec::new(),
        });
        if nodes[ix].dir {
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().unwrap_or_default().to_string_lossy();
                    !n.starts_with('.') && n != "target"
                })
                .collect();
            entries.sort_by_key(|p| (!p.is_dir(), p.file_name().map(|n| n.to_owned())));
            let children = entries.iter().map(|p| walk(p, depth + 1, nodes)).collect();
            nodes[ix].children = children;
        }
        ix
    }
    let mut nodes = Vec::new();
    walk(root, 0, &mut nodes);
    nodes
}

#[derive(Clone, Copy, PartialEq)]
enum Divider {
    Tree,
    Table,
}

struct LayoutView {
    nodes: std::rc::Rc<Vec<Node>>,
    expanded: HashSet<usize>,
    tree_scroll: UniformListScrollHandle,
    table_scroll: UniformListScrollHandle,
    /// Share of the window width taken by the tree, and of the right side taken by the table.
    tree_share: f32,
    table_share: f32,
    dragging: Option<Divider>,
    auto: bool,
    frame: usize,
    last: Option<Instant>,
    intervals: Vec<Duration>,
    renders: Vec<Duration>,
    largest_tree: usize,
    title: &'static str,
}

impl LayoutView {
    fn visible(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![0];
        while let Some(ix) = stack.pop() {
            out.push(ix);
            if self.expanded.contains(&ix) {
                stack.extend(self.nodes[ix].children.iter().rev());
            }
        }
        out
    }

    fn step(&mut self, visible: usize) {
        self.frame += 1;
        if self.frame.is_multiple_of(TOGGLE_EVERY) {
            if self.expanded.len() > 1 {
                self.expanded = HashSet::from([0]);
            } else {
                self.expanded = (0..self.nodes.len())
                    .filter(|&i| self.nodes[i].dir)
                    .collect();
            }
        }
        self.tree_scroll
            .scroll_to_item(self.frame % visible.max(1), ScrollStrategy::Top);
        self.table_scroll
            .scroll_to_item((self.frame * 97) % (ROWS - 50), ScrollStrategy::Top);
        let phase = self.frame as f32 / 60.0;
        self.tree_share = 0.28 + 0.08 * phase.sin();
        self.table_share = 0.55 + 0.15 * (phase * 0.7).cos();
    }

    fn measure(&mut self, window: &mut Window, cx: &mut Context<Self>, visible: usize) {
        let start = Instant::now();
        if let Some(last) = self.last {
            self.intervals.push(start - last);
        }
        self.last = Some(start);
        if self.intervals.len() == FRAMES + WARM_UP {
            println!(
                "{}",
                report(
                    self.title,
                    &self.intervals,
                    &self.renders,
                    self.largest_tree
                )
            );
            if FINISHED.fetch_add(1, Ordering::SeqCst) + 1 == WINDOWS {
                cx.quit();
            }
        } else if self.intervals.len() < FRAMES + WARM_UP {
            self.step(visible);
            let entity = cx.entity();
            window.on_next_frame(move |_, cx| entity.update(cx, |_, cx| cx.notify()));
        }
    }

    fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(divider) = self.dragging else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = None;
            return;
        }
        let viewport = window.viewport_size();
        match divider {
            Divider::Tree => {
                self.tree_share = (event.position.x / viewport.width).clamp(0.1, 0.8);
            }
            Divider::Table => {
                self.table_share = (1.0 - event.position.y / viewport.height).clamp(0.1, 0.9);
            }
        }
        cx.notify();
    }
}

fn splitter(id: &'static str, vertical: bool) -> gpui::Stateful<gpui::Div> {
    let bar = div().id(id).bg(rgb(0x393b40)).flex_none();
    if vertical {
        bar.w(px(4.)).h_full().cursor_col_resize()
    } else {
        bar.h(px(4.)).w_full().cursor_row_resize()
    }
}

impl Render for LayoutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let start = Instant::now();
        let visible = self.visible();
        self.largest_tree = self.largest_tree.max(visible.len());
        if self.auto {
            self.measure(window, cx, visible.len());
        }
        let nodes = self.nodes.clone();
        let expanded = self.expanded.clone();
        let tree = uniform_list(
            "tree",
            visible.len(),
            cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|i| {
                        let ix = visible[i];
                        let node = &nodes[ix];
                        let marker = match (node.dir, expanded.contains(&ix)) {
                            (false, _) => "  ",
                            (true, true) => "▾ ",
                            (true, false) => "▸ ",
                        };
                        div()
                            .id(ix)
                            .pl(px(8. + 14. * node.depth as f32))
                            .hover(|s| s.bg(rgb(0x2e436e)))
                            .child(format!("{marker}{}", node.name))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.nodes[ix].dir && !this.expanded.remove(&ix) {
                                    this.expanded.insert(ix);
                                }
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.tree_scroll.clone())
        .h_full();

        let cell = |w: f32, text: String| div().w(px(w)).flex_none().overflow_hidden().child(text);
        let table = uniform_list(
            "table",
            ROWS,
            cx.processor(move |_this, range: std::ops::Range<usize>, _window, _cx| {
                range
                    .map(|r| {
                        div()
                            .flex()
                            .when(r % 2 == 1, |d| d.bg(rgb(0x232427)))
                            .child(cell(80., format!("{}", r + 1)))
                            .child(cell(220., format!("customer_{:06}", (r * 7919) % ROWS)))
                            .child(cell(160., format!("Straße {}", r % 400)))
                            .child(cell(120., format!("{:.2} €", (r % 10_000) as f64 * 1.37)))
                            .child(cell(
                                120.,
                                format!("2026-{:02}-{:02}", r % 12 + 1, r % 28 + 1),
                            ))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.table_scroll.clone())
        .flex_1();
        let header = div()
            .flex()
            .bg(rgb(0x2b2d30))
            .text_color(rgb(0xdfe1e5))
            .child(cell(80., "#".into()))
            .child(cell(220., "customer".into()))
            .child(cell(160., "street".into()))
            .child(cell(120., "amount".into()))
            .child(cell(120., "date".into()));

        let text =
            div()
                .p(px(8.))
                .flex()
                .flex_col()
                .children((1..=40).map(|i| {
                    format!("{i:>4}  fn pane_line_{i}() -> &'static str {{ \"Grüße\" }}")
                }));

        let view = div()
            .size_full()
            .flex()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xbcbec4))
            .font_family("Menlo")
            .text_sm()
            .on_mouse_move(cx.listener(Self::drag))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = None),
            )
            .child(div().w(relative(self.tree_share)).h_full().child(tree))
            .child(splitter("tree-divider", true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = Some(Divider::Tree)),
            ))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(relative(1.0 - self.table_share))
                            .w_full()
                            .overflow_hidden()
                            .child(text),
                    )
                    .child(splitter("table-divider", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.dragging = Some(Divider::Table)),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .flex()
                            .flex_col()
                            .child(header)
                            .child(table),
                    ),
            );
        self.renders.push(start.elapsed());
        view
    }
}

fn percentile(values: &mut [Duration], p: f64) -> f64 {
    values.sort();
    let i = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[i].as_secs_f64() * 1000.0
}

/// One JSON line per window: frame rate, interval and render percentiles, largest tree shown.
fn report(title: &str, intervals: &[Duration], renders: &[Duration], tree: usize) -> String {
    let mut i = intervals[WARM_UP..].to_vec();
    let mut r = renders[WARM_UP.min(renders.len())..].to_vec();
    let total: f64 = i.iter().map(Duration::as_secs_f64).sum();
    format!(
        "{{\"window\":\"{title}\",\"frames\":{},\"fps\":{:.1},\"interval_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"p99\":{:.2},\"max\":{:.2}}},\"render_ms\":{{\"p50\":{:.3},\"p95\":{:.3},\"max\":{:.3}}},\"table_rows\":{ROWS},\"largest_tree\":{tree}}}",
        i.len(),
        i.len() as f64 / total,
        percentile(&mut i, 0.5),
        percentile(&mut i, 0.95),
        percentile(&mut i, 0.99),
        percentile(&mut i, 1.0),
        percentile(&mut r, 0.5),
        percentile(&mut r, 0.95),
        percentile(&mut r, 1.0),
    )
}

/// Opens the two windows on the tree of `root`; `interactive` leaves them to the mouse.
pub fn layout(root: &Path, interactive: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()).into());
    }
    let nodes = std::rc::Rc::new(read_tree(root));
    Application::new().run(move |cx: &mut App| {
        for (n, title) in ["left", "right"].into_iter().enumerate() {
            let mut bounds = Bounds::centered(None, size(px(1100.0), px(800.0)), cx);
            bounds.origin += point(px(-300. + 600. * n as f32), px(0.));
            let nodes = nodes.clone();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    ferris_idea_workbench::x11_wake::until_first_frame(window);
                    cx.new(|_| LayoutView {
                        nodes,
                        expanded: HashSet::from([0]),
                        tree_scroll: UniformListScrollHandle::new(),
                        table_scroll: UniformListScrollHandle::new(),
                        tree_share: 0.28,
                        table_share: 0.55,
                        dragging: None,
                        auto: !interactive,
                        frame: 0,
                        last: None,
                        intervals: Vec::new(),
                        renders: Vec::new(),
                        largest_tree: 0,
                        title,
                    })
                },
            );
            if let Err(e) = opened {
                crate::no_window::failed(e, cx);
            }
        }
        cx.activate(true);
    });
    crate::no_window::outcome()
}
