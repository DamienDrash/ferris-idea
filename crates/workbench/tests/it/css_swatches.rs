//! Colour swatches in the gutter of stylesheets and HTML files (`SPEC-0042` v27 behaviour 48,
//! AK-54), in a window of the test harness without the screen: a square in the colour on each line
//! with a colour, left of the line numbers' edge, with the colour in its tooltip; an edit that
//! writes a colour adds one; a Rust file has none.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

const SITE_CSS: &str = ".card {\n  padding: 4px;\n  color: red;\n}\n\n#hello {\n  background: #0000ff;\n  border: 1px solid rgb(0 128 0);\n}\n";

const INDEX_HTML: &str = "<p class=\"red\" style=\"color: hsl(0, 0%, 100%)\">red</p>\n";

fn workspace(files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-swatches-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let dir = root.join(".test-settings");
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(dir.join("settings.toml")));
        cx.set_global(ThemeFile(dir.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    cx.executor().allow_parking();
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("the file is open")
}

/// The swatches drawn, once they are `want` (or what was drawn last after 10 s).
fn swatches(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    want: &[(usize, u32)],
) -> Vec<(usize, u32)> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        editor.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        let drawn = editor.read_with(cx, |e, _| e.color_swatches());
        if drawn == want || Instant::now() > end {
            return drawn;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// SPEC-0042 AK-54 (v27, behaviour 48).
#[gpui::test]
fn colour_swatches_in_the_gutter(cx: &mut TestAppContext) {
    let root = workspace(&[
        ("site.css", SITE_CSS),
        ("index.html", INDEX_HTML),
        ("main.rs", "fn main() { let red = 1; }\n"),
    ]);
    let (bench, cx) = window(&root, cx);
    let css = open(&bench, cx, &root.join("site.css"));
    let want = [(2, 0xff0000ff), (6, 0x0000ffff), (7, 0x008000ff)];
    assert_eq!(swatches(&css, cx, &want), want);
    let tip = css.read_with(cx, |e, _| e.gutter_tips_on_line(2));
    assert!(
        tip.iter().any(|t| t.title == "red (#ff0000)"),
        "{:?}",
        tip.iter().map(|t| t.title.clone()).collect::<Vec<_>>()
    );
    // A colour typed on the padding line gets its swatch at once.
    css.update(cx, |e, cx| e.go_to_line(1, cx));
    cx.simulate_keystrokes("cmd-right");
    cx.simulate_input(" outline-color: teal;");
    let want = [
        (1, 0x008080ff),
        (2, 0xff0000ff),
        (6, 0x0000ffff),
        (7, 0x008000ff),
    ];
    assert_eq!(swatches(&css, cx, &want), want);

    let page = open(&bench, cx, &root.join("index.html"));
    let want = [(0, 0xffffffff)];
    assert_eq!(swatches(&page, cx, &want), want);

    let rust = open(&bench, cx, &root.join("main.rs"));
    assert_eq!(swatches(&rust, cx, &[]), []);
    let _ = std::fs::remove_dir_all(&root);
}
