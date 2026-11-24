//! The header row of the merge view in a window of the test harness (`SPEC-0101` v8 AK-21,
//! window check of 2026-11-18, M1): with two conflicted files, data.bin and notes.txt, in a view
//! about 1000 points wide, each file tab keeps the width of its name and is never squeezed to a
//! sliver, the shown tab and the buttons stay in view, and after Resolve Simple the note "1 simple
//! conflict resolved" stays on one line inside the header instead of running over the column
//! headers. Bounds come from the semantic tree (`SPEC-0319`). Nothing is drawn on a screen.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use ferris_idea_vcs::Conflict;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{MergeView, semantics};
use gpui::{
    AppContext, Context, Entity, IntoElement, Modifiers, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, point, px,
};

/// Height of the header row.
const HEADER: f32 = 36.;

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-merge-header-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A binary file that takes a whole side, and a text file with one simple conflict.
fn conflicts() -> Vec<Conflict> {
    vec![
        Conflict {
            path: "data.bin".into(),
            base: None,
            ours: None,
            theirs: None,
            has_base: true,
            has_ours: true,
            has_theirs: true,
            binary: true,
        },
        Conflict {
            path: "notes.txt".into(),
            base: Some("one\nalpha beta gamma\nthree\n".into()),
            ours: Some("one\nALPHA beta gamma\nthree\n".into()),
            theirs: Some("one\nalpha beta GAMMA\nthree\n".into()),
            has_base: true,
            has_ours: true,
            has_theirs: true,
            binary: false,
        },
    ]
}

/// The window's root around the view: it starts and ends the frames of the semantic tree, as the
/// workbench does.
struct Host(Entity<MergeView>);

impl Render for Host {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        semantics::begin_frame(window);
        div()
            .size_full()
            .child(self.0.clone())
            .children(semantics::frame_end())
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

/// The node `id`: its bounds unclipped (x, y, width, height) and whether it is wholly in view.
fn shown(id: &str, cx: &mut VisualTestContext) -> Option<((f32, f32, f32, f32), bool)> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        let n = tree.iter().find(|n| n.test_id() == Some(id))?;
        let b = n.bounds();
        let whole = n
            .visible_bounds
            .is_some_and(|v| b.h > 0. && v.h >= b.h - 0.5 && v.w >= b.w - 0.5);
        Some(((b.x, b.y, b.w, b.h), whole))
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let ((x, y, w, h), _) = shown(id, cx).unwrap_or_else(|| panic!("no control {id}"));
    let at = point(px(x + w / 2.), px(y + h / 2.));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// Each file tab is as wide as its name: "notes.txt" with its padding takes more than 50 points
/// (the window check saw a sliver showing "n"), and "data.bin" too.
fn tabs_keep_their_width(width: f32, cx: &mut VisualTestContext) {
    for i in 0..2 {
        let id = format!("merge-file-{i}");
        let ((_, y, w, h), _) = shown(&id, cx).unwrap_or_else(|| panic!("no tab {id}"));
        assert!(
            w > 50.,
            "{width} points: tab {i} is {w} points wide, squeezed"
        );
        assert!(y >= 0. && y + h <= HEADER + 0.5, "tab {i} in the header");
    }
}

#[gpui::test]
fn header_keeps_tabs_whole_and_the_note_on_one_line(cx: &mut TestAppContext) {
    let settings = folder();
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let root = settings.clone();
    let window = cx.add_window(move |_, cx| {
        let view = cx.new(|cx| MergeView::new(&root, conflicts(), cx));
        Host(view)
    });
    let host: Entity<Host> = window.root(cx).unwrap();
    let view = host.read_with(cx, |h, _| h.0.clone());
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();

    for width in [1000., 1200.] {
        cx.simulate_resize(gpui::size(px(width), px(640.)));
        draw(cx);
        tabs_keep_their_width(width, cx);
        assert!(
            shown("merge-close", cx).is_some_and(|s| s.1),
            "{width} points: Close in view"
        );
        assert!(
            shown("merge-file-0", cx).is_some_and(|s| s.1),
            "{width} points: the shown tab in view"
        );
    }

    // notes.txt by a click on its tab; Resolve Simple takes its simple conflict.
    cx.simulate_resize(gpui::size(px(1000.), px(640.)));
    draw(cx);
    // At this width the second tab is beyond the row's edge; the row rolls sideways.
    if !shown("merge-file-1", cx).is_some_and(|s| s.1) {
        let ((x, y, w, h), _) = shown("merge-file-0", cx).expect("the first tab");
        let at = point(px(x + w / 2.), px(y + h / 2.));
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(-400.), px(0.))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        draw(cx);
    }
    assert!(
        shown("merge-file-1", cx).is_some_and(|s| s.1),
        "the rolled row shows the second tab"
    );
    click("merge-file-1", cx);
    assert_eq!(view.read_with(cx, |v, _| v.file()), 1);
    assert!(
        shown("merge-file-1", cx).is_some_and(|s| s.1),
        "the shown tab in view"
    );
    click("merge-resolve-simple", cx);
    assert_eq!(view.read_with(cx, |v, _| v.open_conflicts()), 0);

    // The note stays on one line in the header; the tabs and the buttons keep their room.
    for width in [1000., 1200.] {
        cx.simulate_resize(gpui::size(px(width), px(640.)));
        draw(cx);
        let ((_, y, _, h), _) = shown("merge-note", cx).expect("the note is shown");
        // One line: the height of a tab without its padding of 2 points above and below.
        let ((_, _, _, tab), _) = shown("merge-file-0", cx).expect("the first tab");
        assert!(
            h <= tab - 4. + 0.5,
            "{width} points: the note is {h} points high, more than one line ({})",
            tab - 4.
        );
        assert!(
            y >= 0. && y + h <= HEADER + 0.5,
            "{width} points: the note ends at {}, below the header",
            y + h
        );
        tabs_keep_their_width(width, cx);
        assert!(
            shown("merge-apply", cx).is_some_and(|s| s.1),
            "{width} points: Apply in view"
        );
        assert!(
            shown("merge-close", cx).is_some_and(|s| s.1),
            "{width} points: Close in view"
        );
    }
    let _ = std::fs::remove_dir_all(&settings);
}
