//! M3.4 spike for ADR-0003 criterion 1: open a large file in a GPUI window, scroll it frame by
//! frame and report the frame intervals. Needs an unlocked screen (macOS sends no frames otherwise)
//! and, on Linux, an X11 or Wayland display.

use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, Context, ScrollStrategy, UniformListScrollHandle, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size, uniform_list,
};

/// Frames measured after a short warm-up.
const FRAMES: usize = 900;
const WARM_UP: usize = 10;
/// Lines scrolled per frame.
const STEP: usize = 3;

struct ScrollView {
    lines: Vec<String>,
    scroll: UniformListScrollHandle,
    top: usize,
    last: Option<Instant>,
    intervals: Vec<Duration>,
    renders: Vec<Duration>,
}

impl Render for ScrollView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let start = Instant::now();
        if let Some(last) = self.last {
            self.intervals.push(start - last);
        }
        self.last = Some(start);
        if self.intervals.len() >= FRAMES + WARM_UP {
            println!("{}", report(&self.intervals, &self.renders));
            cx.quit();
        } else {
            self.top = (self.top + STEP) % self.lines.len().saturating_sub(100).max(1);
            self.scroll.scroll_to_item(self.top, ScrollStrategy::Top);
            let entity = cx.entity();
            window.on_next_frame(move |_, cx| entity.update(cx, |_, cx| cx.notify()));
        }
        let view = div()
            .size_full()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xbcbec4))
            .font_family("Menlo")
            .text_sm()
            .child(
                uniform_list(
                    "lines",
                    self.lines.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|ix| {
                                div()
                                    .flex()
                                    .child(
                                        div()
                                            .w(px(56.))
                                            .text_color(rgb(0x6f737a))
                                            .child(format!("{:>6}", ix + 1)),
                                    )
                                    .child(this.lines[ix].clone())
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .h_full(),
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

/// One JSON line: frames per second and interval/render percentiles in milliseconds.
fn report(intervals: &[Duration], renders: &[Duration]) -> String {
    let mut i = intervals[WARM_UP..].to_vec();
    let mut r = renders[WARM_UP.min(renders.len())..].to_vec();
    let total: f64 = i.iter().map(Duration::as_secs_f64).sum();
    format!(
        "{{\"frames\":{},\"fps\":{:.1},\"interval_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"p99\":{:.2},\"max\":{:.2}}},\"render_ms\":{{\"p50\":{:.3},\"p95\":{:.3},\"max\":{:.3}}}}}",
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

/// 100 000 lines of code-like text with umlauts, used when no file is given.
pub(crate) fn generated() -> Vec<String> {
    (0..100_000)
        .map(|i| match i % 5 {
            0 => format!("fn function_{i}(value: u64) -> Result<u64, Error> {{"),
            1 => format!("    let doubled = value.checked_mul(2).ok_or(Error::Overflow({i}))?;"),
            2 => "    // Grüße: äöüß — a longer comment line that fills most of the width".into(),
            3 => format!("    Ok(doubled + {i})"),
            _ => "}".into(),
        })
        .collect()
}

/// Runs the scroll spike on `file` (or generated text) and prints the measurement.
pub fn scroll(file: Option<&std::path::Path>) -> Result<(), Box<dyn std::error::Error>> {
    let lines = match file {
        Some(f) => std::fs::read_to_string(f)?
            .lines()
            .map(str::to_owned)
            .collect(),
        None => generated(),
    };
    if lines.len() < 200 {
        return Err("the spike needs a file with at least 200 lines".into());
    }
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(900.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                idecraft_workbench::x11_wake::until_first_frame(window);
                cx.new(|_| ScrollView {
                    lines,
                    scroll: UniformListScrollHandle::new(),
                    top: 0,
                    last: None,
                    intervals: Vec::new(),
                    renders: Vec::new(),
                })
            },
        );
        if let Err(e) = opened {
            crate::no_window::failed(e, cx);
        }
        cx.activate(true);
    });
    crate::no_window::outcome()
}
