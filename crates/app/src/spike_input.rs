//! M3.4 spike for ADR-0003 criteria 2 and 3: a text field fed by the platform's input method
//! (umlauts, dead keys, IME composition) that logs what arrives and measures the time from the
//! input event to the next rendered frame.

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui::{
    App, Application, Bounds, Context, ElementInputHandler, EntityInputHandler, FocusHandle,
    Focusable, KeyDownEvent, Pixels, UTF16Selection, Window, WindowBounds, WindowOptions, canvas,
    div, prelude::*, px, rgb, size,
};

struct InputSpike {
    content: String,
    /// Byte range of the text being composed by the input method.
    marked: Option<Range<usize>>,
    /// Byte offset of the caret.
    caret: usize,
    focus: FocusHandle,
    /// When the last input event arrived and nothing was drawn since.
    pending: Option<Instant>,
    latencies: Vec<Duration>,
}

impl InputSpike {
    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset.min(self.content.len())]
            .chars()
            .map(char::len_utf16)
            .sum()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut units = 0;
        for (i, ch) in self.content.char_indices() {
            if units >= offset {
                return i;
            }
            units += ch.len_utf16();
        }
        self.content.len()
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }

    fn replace(&mut self, range: Range<usize>, text: &str) {
        self.content.replace_range(range.clone(), text);
        self.caret = range.start + text.len();
        self.pending.get_or_insert_with(Instant::now);
    }
}

impl EntityInputHandler for InputSpike {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end));
        Some(self.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let at = self.offset_to_utf16(self.caret);
        Some(UTF16Selection {
            range: at..at,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|r| self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone())
            .unwrap_or(self.caret..self.caret);
        self.replace(range, text);
        self.marked = None;
        println!("commit {text:?} -> {:?}", self.content);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone())
            .unwrap_or(self.caret..self.caret);
        self.replace(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        println!("compose {text:?}");
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The candidate window opens below the field; exact glyph positions come with the editor view.
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.caret))
    }
}

impl Focusable for InputSpike {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for InputSpike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(at) = self.pending.take() {
            self.latencies.push(at.elapsed());
        }
        let (before, marked, after) = match &self.marked {
            Some(m) => (
                self.content[..m.start].to_owned(),
                self.content[m.clone()].to_owned(),
                self.content[m.end..].to_owned(),
            ),
            None => (
                self.content[..self.caret].to_owned(),
                String::new(),
                self.content[self.caret..].to_owned(),
            ),
        };
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .size_full()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xbcbec4))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _window, cx| {
                if e.keystroke.key == "backspace" && this.marked.is_none() && this.caret > 0 {
                    let start = this.content[..this.caret]
                        .char_indices()
                        .last()
                        .map_or(0, |(i, _)| i);
                    this.replace(start..this.caret, "");
                    println!("backspace -> {:?}", this.content);
                    cx.notify();
                }
            }))
            .child("IDECraft input spike — type: Grüße, ´ then e (é), ^ then a (â), Japanese 'nihongo' + Space + Enter. Close the window when done.")
            .child(
                div()
                    .relative()
                    .h(px(36.))
                    .px_2()
                    .bg(rgb(0x2b2d30))
                    .text_xl()
                    .font_family("Menlo")
                    .flex()
                    .items_center()
                    .child(before)
                    .child(div().underline().text_color(rgb(0xe5c07b)).child(marked))
                    .child("|")
                    .child(after)
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, cx| {
                                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .child(format!(
                "{} input event(s) drawn; last: {:?}",
                self.latencies.len(),
                self.latencies.last()
            ))
    }
}

fn percentile(values: &mut [Duration], p: f64) -> f64 {
    values.sort();
    let i = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[i].as_secs_f64() * 1000.0
}

/// Opens the input field; prints every commit and composition, and on close the latency summary.
pub fn input() -> Result<(), Box<dyn std::error::Error>> {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(260.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                idecraft_workbench::x11_wake::until_first_frame(window);
                let view = cx.new(|cx| InputSpike {
                    content: String::new(),
                    marked: None,
                    caret: 0,
                    focus: cx.focus_handle(),
                    pending: None,
                    latencies: Vec::new(),
                });
                window.focus(&view.read(cx).focus.clone());
                let summary = view.clone();
                cx.on_window_closed(move |cx| {
                    let spike = summary.read(cx);
                    let mut l = spike.latencies.clone();
                    if l.is_empty() {
                        println!("{{\"text\":{:?},\"events\":0}}", spike.content);
                    } else {
                        println!(
                            "{{\"text\":{:?},\"events\":{},\"latency_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"max\":{:.2}}}}}",
                            spike.content,
                            l.len(),
                            percentile(&mut l, 0.5),
                            percentile(&mut l, 0.95),
                            percentile(&mut l, 1.0)
                        );
                    }
                    cx.quit();
                })
                .detach();
                view
            },
        );
        if let Err(e) = opened {
            crate::no_window::failed(e, cx);
        }
        cx.activate(true);
    });
    crate::no_window::outcome()
}
