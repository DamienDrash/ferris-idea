//! The startup animation's window (`SPEC-0355` v2 behaviour 23): a small borderless window in
//! the middle of the screen that draws Ferris IDEA's brand sequence while start-up work runs, and
//! goes the moment the first window (project or Welcome) has drawn its first frame, within the
//! cap of [`crate::startup_animation::CAP`].
//!
//! Everything is drawn from the brand's vector outlines at the window's scale: the crab's cells as
//! rectangles, built row by row on their grid, the cursor bar as a rectangle, the wordmark as
//! filled paths behind a clipping mask that opens from left to right. No image, no
//! video, no text, no network.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, ContentMask, Context, Global, Hsla, PathBuilder,
    Pixels, Render, Rgba, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, canvas, div, fill, point, prelude::*, px, size,
};

use crate::startup_animation::{
    self as model, Brand, Frame, LOGO_HEIGHT, LOGO_WIDTH, Mode, Seg, symbol_rects,
};

/// The splash window's size in points.
pub const SPLASH_SIZE: (f32, f32) = (480.0, 160.0);

/// Clear space around the logo, in logo units (a quarter of the symbol, BRAND_GUIDELINES §4).
const CLEAR_SPACE: f32 = 8.0;

/// The longest the first window waits for the splash's first frame before it opens anyway (a
/// window that is not on screen draws no frame).
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(50);

/// Start-up work that runs once, from whichever comes first: the splash's frame or the timer.
type Once = Rc<RefCell<Option<Box<dyn FnOnce(&mut App)>>>>;

/// Native touches the application gives the splash window (on macOS: no window buttons).
pub type Styler = fn(&Window);

struct Splash {
    handle: WindowHandle<SplashView>,
    mode: Mode,
    started: Instant,
    ready: Option<Instant>,
}

#[derive(Default)]
struct SplashState(Option<Splash>);

impl Global for SplashState {}

/// The window that draws the startup animation.
pub struct SplashView {
    mode: Mode,
}

/// Opens the splash in `mode`; nothing for [`Mode::Off`] or without the brand's outlines.
pub fn show(mode: Mode, style: Option<Styler>, cx: &mut App) -> Option<WindowHandle<SplashView>> {
    if mode == Mode::Off || model::brand().is_none() {
        return None;
    }
    let bounds = Bounds::centered(None, size(px(SPLASH_SIZE.0), px(SPLASH_SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::Normal,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(ferris_idea_ui::brand::APP_ID.to_owned()),
        ..Default::default()
    };
    let handle = cx
        .open_window(options, move |window, cx| {
            crate::x11_wake::until_first_frame(window);
            if let Some(style) = style {
                style(window);
            }
            cx.new(|_| SplashView { mode })
        })
        .ok()?;
    let started = cx.background_executor().now();
    cx.set_global(SplashState(Some(Splash {
        handle,
        mode,
        started,
        ready: None,
    })));
    close_when_due(cx);
    Some(handle)
}

/// The open splash window, if any.
pub fn splash_window(cx: &App) -> Option<WindowHandle<SplashView>> {
    let s = cx.try_global::<SplashState>()?.0.as_ref()?;
    cx.windows()
        .contains(&AnyWindowHandle::from(s.handle))
        .then_some(s.handle)
}

/// How long the splash has been open, if it is.
pub fn splash_age(cx: &App) -> Option<Duration> {
    let s = cx.try_global::<SplashState>()?.0.as_ref()?;
    Some(cx.background_executor().now() - s.started)
}

/// Runs `then` once the splash has drawn its first frame, so it shows before the rest of the
/// start-up work keeps the main thread busy; at once without a splash, and after at most
/// [`FIRST_FRAME_WAIT`] if no frame comes.
pub fn after_first_frame(then: impl FnOnce(&mut App) + 'static, cx: &mut App) {
    let Some(handle) = splash_window(cx) else {
        then(cx);
        return;
    };
    let once: Once = Rc::new(RefCell::new(Some(Box::new(then))));
    let on_frame = once.clone();
    let hooked = handle.update(cx, |_, window, _| {
        window.on_next_frame(move |_, cx| {
            let next = on_frame.borrow_mut().take();
            if let Some(f) = next {
                f(cx);
            }
        });
        window.refresh();
    });
    if hooked.is_err() {
        let next = once.borrow_mut().take();
        if let Some(f) = next {
            f(cx);
        }
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_FRAME_WAIT).await;
        let next = once.borrow_mut().take();
        if let Some(f) = next {
            let _ = cx.update(f);
        }
    })
    .detach();
}

/// The first window is ready: the splash fades and closes (behaviour 23).
pub fn first_window_ready(cx: &mut App) {
    let now = cx.background_executor().now();
    let Some(state) = cx.try_global::<SplashState>() else {
        return;
    };
    if state.0.as_ref().is_none_or(|s| s.ready.is_some()) {
        return;
    }
    cx.update_global::<SplashState, _>(|state, _| {
        if let Some(s) = state.0.as_mut() {
            s.ready = Some(now);
        }
    });
    close_when_due(cx);
}

/// The splash goes once `first` has drawn its first frame; when that window cannot be reached
/// the splash goes at once.
pub fn close_when_painted(first: AnyWindowHandle, cx: &mut App) {
    if splash_window(cx).is_none() {
        return;
    }
    let hooked = first.update(cx, |_, window, _| {
        window.on_next_frame(|_, cx| first_window_ready(cx));
        window.refresh();
    });
    if hooked.is_err() {
        first_window_ready(cx);
    }
}

/// Closes the splash now if its time is up, else waits until it is.
fn close_when_due(cx: &mut App) {
    let now = cx.background_executor().now();
    let Some(s) = cx.try_global::<SplashState>().and_then(|s| s.0.as_ref()) else {
        return;
    };
    let ready = s.ready.map(|r| r - s.started);
    let Some(end) = model::closes_at(s.mode, ready) else {
        return;
    };
    let elapsed = now - s.started;
    if elapsed >= end {
        let handle = s.handle;
        cx.set_global(SplashState(None));
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return;
    }
    let handle = s.handle;
    let wait = end - elapsed;
    cx.spawn(async move |cx| {
        cx.background_executor().timer(wait).await;
        let _ = cx.update(|cx| {
            let same = cx
                .try_global::<SplashState>()
                .and_then(|s| s.0.as_ref())
                .is_some_and(|s| s.handle == handle);
            if same {
                close_when_due(cx);
            }
        });
    })
    .detach();
}

impl SplashView {
    /// The frame drawn now; `None` once the splash's time is up.
    pub fn frame_now(&self, cx: &App) -> Option<Frame> {
        let s = cx.try_global::<SplashState>()?.0.as_ref()?;
        let now = cx.background_executor().now();
        model::frame(self.mode, now - s.started, s.ready.map(|r| r - s.started))
    }
}

impl Render for SplashView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.frame_now(cx);
        let moving = frame.is_some_and(|f| self.mode == Mode::Animated || f.opacity < 1.0);
        if moving {
            window.request_animation_frame();
        }
        let brand = model::brand();
        div().size_full().child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    if let (Some(f), Some(b)) = (frame, brand) {
                        paint(bounds, f, b, window);
                    }
                },
            )
            .size_full(),
        )
    }
}

fn color(rgb: u32, alpha: f32) -> Hsla {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: alpha,
    }
    .into()
}

/// Draws `f` into `bounds`: the background, the symbol shifted by `f.shift`, and the wordmark
/// through its mask.
fn paint(bounds: Bounds<Pixels>, f: Frame, b: &Brand, window: &mut Window) {
    let a = f.opacity.clamp(0.0, 1.0);
    window.paint_quad(fill(bounds, color(b.background, a)).corner_radii(px(10.0)));
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let unit = (w / (LOGO_WIDTH + 2.0 * CLEAR_SPACE)).min(h / (LOGO_HEIGHT + 2.0 * CLEAR_SPACE));
    let x0 = f32::from(bounds.origin.x) + (w - LOGO_WIDTH * unit) / 2.0;
    let y0 = f32::from(bounds.origin.y) + (h - LOGO_HEIGHT * unit) / 2.0;
    let pt = move |x: f32, y: f32| point(px(x0 + x * unit), px(y0 + y * unit));
    // The crab's cells and the cursor end on whole device pixels, so neighbouring cells meet
    // without a faint seam and the grid stays crisp at any scale.
    let scale = window.scale_factor();
    let snap = move |v: f32| (v * scale).round() / scale;
    let cell = move |x: f32, y: f32| point(px(snap(x0 + x * unit)), px(snap(y0 + y * unit)));
    let rect = |[l, t, r, btm]: [f32; 4]| Bounds::from_corners(cell(l, t), cell(r, btm));

    for [l, t, r, btm] in symbol_rects(&b.cells, f.symbol) {
        window.paint_quad(fill(
            rect([l + f.shift, t, r + f.shift, btm]),
            color(b.symbol.color, a),
        ));
    }
    if f.cursor {
        let [l, t, r, btm] = b.cursor.bounds();
        window.paint_quad(fill(
            rect([l + f.shift, t, r + f.shift, btm]),
            color(b.cursor.color, a),
        ));
    }
    let (left, right) = f.mask;
    if right <= left {
        return;
    }
    let mask = ContentMask {
        bounds: rect([left, -CLEAR_SPACE, right, LOGO_HEIGHT + CLEAR_SPACE]),
    };
    window.with_content_mask(Some(mask), |window| {
        for shape in &b.wordmark {
            let mut path = PathBuilder::fill();
            for seg in &shape.segs {
                match *seg {
                    Seg::Move((x, y)) => path.move_to(pt(x, y)),
                    Seg::Line((x, y)) => path.line_to(pt(x, y)),
                    Seg::Cubic((ax, ay), (bx, by), (x, y)) => {
                        path.cubic_bezier_to(pt(x, y), pt(ax, ay), pt(bx, by))
                    }
                    Seg::Close => path.close(),
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color(shape.color, a));
            }
        }
    });
}
