//! M3.4 criterion 2 and M3.7: the editor view on a file (`idecraft edit`), and a measurement that
//! types into the middle of a 100 000-line file and reports the time from each keystroke to the
//! frame that shows it (`idecraft spike-editor`).

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext, Application, Bounds, Context, Entity, Keystroke, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use idecraft_editor::Carets;
use idecraft_workbench::EditorView;

fn open(
    text: String,
    path: Option<std::path::PathBuf>,
    measure: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    Application::new().run(move |cx: &mut App| {
        if !idecraft_workbench::fonts::register(cx) {
            eprintln!("warning: JetBrains Mono could not be registered; using a system font");
        }
        let bounds = Bounds::centered(None, size(px(1200.0), px(900.0)), cx);
        let title = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "IDECraft editor spike".into());
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                idecraft_workbench::x11_wake::until_first_frame(window);
                window.set_window_title(&title);
                let editor = cx.new(|cx| EditorView::new(&text, path, cx));
                window.focus(&editor.read(cx).focus());
                cx.new(|cx| Driver::new(editor, measure, cx))
            },
        );
        if let Err(e) = opened {
            crate::no_window::failed(e, cx);
        }
        cx.activate(true);
    });
    crate::no_window::outcome()
}

/// Hosts the editor; in measurement mode it presses one keystroke per frame.
struct Driver {
    editor: Entity<EditorView>,
    steps: Vec<Keystroke>,
    next: usize,
    measure: bool,
    last_frame: Option<Instant>,
    intervals: Vec<Duration>,
}

impl Driver {
    fn new(editor: Entity<EditorView>, measure: bool, cx: &mut Context<Self>) -> Self {
        let mut steps = Vec::new();
        if measure {
            editor.update(cx, |e, _| {
                let middle = e
                    .state
                    .buffer
                    .position_to_offset(idecraft_editor::Position::new(50_000, 4))
                    .unwrap_or(0);
                e.state.carets = Carets::new(middle);
            });
            let parse = |s: &str| Keystroke::parse(s).expect("valid keystroke");
            for c in "let grüße = 42;".chars().cycle().take(120) {
                let key = if c == ' ' {
                    "space".to_owned()
                } else {
                    c.to_string()
                };
                let mut k = parse(&key);
                k.key_char = Some(c.to_string());
                steps.push(k);
            }
            steps.extend(std::iter::repeat_n(parse("backspace"), 60));
            steps.extend(std::iter::repeat_n(parse("down"), 30));
            steps.extend(std::iter::repeat_n(parse("enter"), 10));
        }
        Self {
            editor,
            steps,
            next: 0,
            measure,
            last_frame: None,
            intervals: Vec::new(),
        }
    }
}

impl Render for Driver {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.measure {
            if self.next < self.steps.len() {
                let stroke = self.steps[self.next].clone();
                self.next += 1;
                let me = cx.entity();
                let editor = self.editor.clone();
                window.on_next_frame(move |window, cx| {
                    let started = Instant::now();
                    window.dispatch_keystroke(stroke, cx);
                    editor.update(cx, |e, _| e.mark_input(started));
                    me.update(cx, |this, cx| {
                        if let Some(last) = this.last_frame.replace(started) {
                            this.intervals.push(started - last);
                        }
                        cx.notify()
                    });
                });
            } else {
                // Give the last keystroke its frame, then report.
                let latencies = self.editor.read(cx).latencies.clone();
                if latencies.len() >= self.steps.len() {
                    println!("{}", report(&latencies, &self.intervals, self.steps.len()));
                    cx.quit();
                } else {
                    let me = cx.entity();
                    window.on_next_frame(move |_, cx| me.update(cx, |_, cx| cx.notify()));
                }
            }
        }
        div().size_full().child(self.editor.clone())
    }
}

fn percentile(values: &mut [Duration], p: f64) -> f64 {
    values.sort();
    let i = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[i].as_secs_f64() * 1000.0
}

/// Keystroke-to-painted-frame latency, and the interval between keystrokes (one per frame, so it is
/// the frame interval while typing).
fn report(latencies: &[Duration], intervals: &[Duration], keystrokes: usize) -> String {
    let mut l = latencies[latencies.len().saturating_sub(keystrokes)..].to_vec();
    let slowest = l
        .iter()
        .enumerate()
        .max_by_key(|(_, d)| **d)
        .map_or(0, |(i, _)| i);
    let over = l.iter().filter(|d| d.as_secs_f64() * 1000.0 > 16.0).count();
    let mut i = intervals.to_vec();
    format!(
        "{{\"keystrokes\":{},\"latency_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"p99\":{:.2},\"max\":{:.2}}},\"over_16ms\":{over},\"slowest_keystroke\":{slowest},\"frame_interval_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"max\":{:.2}}}}}",
        l.len(),
        percentile(&mut l, 0.5),
        percentile(&mut l, 0.95),
        percentile(&mut l, 0.99),
        percentile(&mut l, 1.0),
        percentile(&mut i, 0.5),
        percentile(&mut i, 0.95),
        percentile(&mut i, 1.0),
    )
}

/// Opens `file` in the editor view.
pub fn edit(file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(file)?;
    open(text, Some(file.to_path_buf()), false)
}

/// Types into line 50 000 of `file` (or 100 000 generated Rust lines) and prints the latencies.
pub fn measure(file: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let (text, path) = match file {
        Some(f) => (std::fs::read_to_string(f)?, Some(f.to_path_buf())),
        None => (
            crate::spike::generated().join("\n"),
            Some(std::path::PathBuf::from("generated.rs")),
        ),
    };
    if text.lines().count() < 50_001 {
        return Err("the measurement needs at least 50 001 lines".into());
    }
    open(text, path, true)
}
