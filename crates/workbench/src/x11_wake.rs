//! A new window on X11 draws its first frame also without a window manager (ADR-0003, Linux;
//! found by the Linux window self-test, docs/quality/QG-1-LINUX-WINDOW.md).
//!
//! gpui's X11 client starts drawing a window when it handles the window's MapNotify. It handles
//! events when its connection's socket becomes readable; events that libxcb already read while
//! waiting for a reply (gpui sets the title and other properties with round trips right after the
//! map) stay in libxcb's queue. With a window manager more events follow (reparenting,
//! configuration, focus) and wake the loop; on a bare X server such as Xvfb nothing follows, the
//! map is never handled and the window stays empty with the process idle.
//!
//! [`until_first_frame`] wakes the loop: until the window has drawn its first frame (at most
//! [`LIMIT`]), a thread with its own X connection changes a property of this process's top-level
//! windows (found by `_NET_WM_PID`), and the PropertyNotify that gpui gets makes its socket
//! readable. The property carries no data of the user, and nothing happens on Wayland, macOS or
//! when the frame comes on its own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::Window;

/// How long the thread keeps waking a window that has not drawn yet.
pub const LIMIT: Duration = Duration::from_secs(10);
/// Pause between two wake-ups.
pub const INTERVAL: Duration = Duration::from_millis(100);
/// The property that is changed; its only content is a counter.
pub const PROPERTY: &str = "_IDECRAFT_WAKE";

/// Whether gpui talks to an X server in this environment: `DISPLAY` is set and `WAYLAND_DISPLAY`
/// is not (gpui prefers Wayland when both are there).
pub fn uses_x11(display: Option<&std::ffi::OsStr>, wayland: Option<&std::ffi::OsStr>) -> bool {
    let set = |v: Option<&std::ffi::OsStr>| v.is_some_and(|v| !v.is_empty());
    cfg!(target_os = "linux") && set(display) && !set(wayland)
}

/// Starts waking `window` until its first frame (see the module comment). Call it from the
/// window's build closure of `open_window`.
pub fn until_first_frame(window: &mut Window) {
    if !uses_x11(
        std::env::var_os("DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
    ) {
        return;
    }
    let drawn = Arc::new(AtomicBool::new(false));
    let flag = drawn.clone();
    window.on_next_frame(move |_, _| flag.store(true, Ordering::SeqCst));
    imp::spawn(drawn);
}

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode, Window};
    use x11rb::wrapper::ConnectionExt as _;

    use super::{INTERVAL, LIMIT, PROPERTY};

    pub fn spawn(drawn: Arc<AtomicBool>) {
        let _ = std::thread::Builder::new()
            .name("x11-first-frame".into())
            .spawn(move || {
                let Ok((conn, screen)) = x11rb::connect(None) else {
                    return;
                };
                let root = conn.setup().roots[screen].root;
                let start = Instant::now();
                let mut count: u32 = 0;
                while !drawn.load(Ordering::SeqCst) && start.elapsed() < LIMIT {
                    std::thread::sleep(INTERVAL);
                    if drawn.load(Ordering::SeqCst) {
                        break;
                    }
                    count = count.wrapping_add(1);
                    if wake(&conn, root, std::process::id(), count).is_err() {
                        return;
                    }
                }
            });
    }

    /// Changes [`PROPERTY`] on every top-level window of the process `pid`; returns how many.
    pub fn wake(
        conn: &impl Connection,
        root: Window,
        pid: u32,
        count: u32,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let pid_atom = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        let wake_atom = conn.intern_atom(false, PROPERTY.as_bytes())?.reply()?.atom;
        let mut woken = 0;
        for w in conn.query_tree(root)?.reply()?.children {
            let Ok(reply) = conn
                .get_property(false, w, pid_atom, AtomEnum::CARDINAL, 0, 1)?
                .reply()
            else {
                continue;
            };
            if reply.value32().and_then(|mut v| v.next()) == Some(pid) {
                conn.change_property32(
                    PropMode::REPLACE,
                    w,
                    wake_atom,
                    AtomEnum::CARDINAL,
                    &[count],
                )?;
                woken += 1;
            }
        }
        conn.flush()?;
        Ok(woken)
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn spawn(_: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
}

#[cfg(target_os = "linux")]
pub use imp::wake;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn only_an_x11_session_is_woken() {
        let x = Some(OsStr::new(":0"));
        let w = Some(OsStr::new("wayland-0"));
        let linux = cfg!(target_os = "linux");
        assert_eq!(uses_x11(x, None), linux);
        assert!(!uses_x11(x, w), "gpui takes Wayland when both are set");
        assert!(!uses_x11(None, None));
        assert!(!uses_x11(Some(OsStr::new("")), None));
        assert_eq!(uses_x11(x, Some(OsStr::new(""))), linux);
    }

    /// Live on an X server (`xvfb-run cargo test -p idecraft-workbench x11_wake`); returns early
    /// without `DISPLAY`. A window that carries this process's `_NET_WM_PID` gets a PropertyNotify
    /// on its own connection, a window of another process is left alone.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_windows_of_this_process_get_a_property_notify() {
        use x11rb::connection::Connection;
        use x11rb::protocol::Event;
        use x11rb::protocol::xproto::{
            AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, WindowClass,
        };
        use x11rb::wrapper::ConnectionExt as _;

        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("skipped: no DISPLAY");
            return;
        }
        let (app, screen) = x11rb::connect(None).expect("X connection");
        let root = app.setup().roots[screen].root;
        let pid_atom = app
            .intern_atom(false, b"_NET_WM_PID")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let make = |pid: u32| {
            let w = app.generate_id().unwrap();
            app.create_window(
                0,
                w,
                root,
                0,
                0,
                10,
                10,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .unwrap();
            app.change_property32(PropMode::REPLACE, w, pid_atom, AtomEnum::CARDINAL, &[pid])
                .unwrap();
            w
        };
        let ours = make(std::process::id());
        let other = make(std::process::id().wrapping_add(1));
        app.sync().unwrap();
        while app.poll_for_event().unwrap().is_some() {}

        let (waker, _) = x11rb::connect(None).expect("second X connection");
        let woken = wake(&waker, root, std::process::id(), 7).unwrap();
        assert_eq!(woken, 1);

        let wake_atom = app
            .intern_atom(false, PROPERTY.as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        while std::time::Instant::now() < deadline && seen.is_empty() {
            match app.poll_for_event().unwrap() {
                Some(Event::PropertyNotify(e)) if e.atom == wake_atom => seen.push(e.window),
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert_eq!(seen, [ours]);
        let value = app
            .get_property(false, other, wake_atom, AtomEnum::CARDINAL, 0, 1)
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(
            value.value_len, 0,
            "the other process's window is not touched"
        );
        app.destroy_window(ours).unwrap();
        app.destroy_window(other).unwrap();
        app.flush().unwrap();
    }
}
