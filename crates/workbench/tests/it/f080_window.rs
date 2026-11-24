//! The plugin hand tests of F-080 (27, 82, 92, 150, 151; `SPEC-0104`, `SPEC-0156`, `SPEC-0166`,
//! `SPEC-0210`, `SPEC-0211`) in a window of the test harness, with keys and clicks on the controls
//! the semantic tree names (`SPEC-0319`): a text
//! plugin's action from Find Action next to a broken plugin; an `edit` action that needs the
//! project grant, granted with Space, undone with ⌘Z; a plugin inspection fixed with ⌥↩; the four
//! extension points of `tests/plugins`; and the review of Install, Reinstall, Update and Downgrade
//! with the grant kept on update.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::settings_model::Page;
use ferris_idea_workbench::tool_windows::ToolWindow;
use ferris_idea_workbench::{SettingsView, WorkbenchView, semantics};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

type Bench = Entity<WorkbenchView>;

fn workspace(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ferris-idea-f080-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
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
    draw(cx);
    (bench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

fn until(
    cx: &mut VisualTestContext,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        draw(cx);
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}

/// The node with test id `id` in the semantic tree of the frame just drawn, if it is in view:
/// its name and the middle of what is shown of it.
fn node(cx: &mut VisualTestContext, id: &str) -> Option<(String, gpui::Point<gpui::Pixels>)> {
    draw(cx);
    cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window))?;
        tree.iter()
            .filter(|n| n.test_id() == Some(id))
            .find_map(|n| {
                let r = n.visible_bounds.filter(|r| r.w > 0. && r.h > 0.)?;
                Some((
                    n.name().unwrap_or_default().to_owned(),
                    point(px(r.x + r.w / 2.), px(r.y + r.h / 2.)),
                ))
            })
    })
}

fn name_of(cx: &mut VisualTestContext, id: &str) -> String {
    node(cx, id)
        .unwrap_or_else(|| panic!("{id} is drawn and in view"))
        .0
}

fn click(cx: &mut VisualTestContext, id: &str) {
    let (_, at) = node(cx, id).unwrap_or_else(|| panic!("{id} is drawn and in view"));
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn text(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.active_text(cx))
        .unwrap_or_default()
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

fn open(bench: &Bench, cx: &mut VisualTestContext, file: &Path) {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    draw(cx);
}

/// ⇧⌘A, `query` typed, ↓ until the row of action `id` is selected, Enter.
fn find_action(bench: &Bench, cx: &mut VisualTestContext, query: &str, id: &str) {
    keys(cx, "cmd-shift-a");
    let popup = bench
        .read_with(cx, |b, _| b.everywhere())
        .expect("Find Action is open");
    cx.simulate_input(query);
    let row_of = |cx: &mut VisualTestContext| {
        popup.read_with(cx, |v, _| {
            let r = v.results();
            let at = r.rows.iter().position(|r| format!("{r:?}").contains(id));
            (at, r.selected, format!("{:?}", r.rows))
        })
    };
    let mut rows = String::new();
    until(cx, 10, &format!("Find Action lists {id}"), |cx| {
        let (at, _, shown) = row_of(cx);
        rows = shown;
        at.is_some()
    });
    for _ in 0..20 {
        let (at, selected, _) = row_of(cx);
        if at == selected {
            keys(cx, "enter");
            return;
        }
        keys(cx, "down");
    }
    panic!("{id} is never selected: {rows}");
}

/// A plugin module in the WebAssembly text format: the manifest at 16, `data` at their offsets,
/// a bump allocator from 8192 and `run` as the body of `ferris_idea_run` (the action id at `$a`, the
/// input at `$p` with length `$l`).
fn module(manifest: &str, imports: &str, data: &[(u32, &str)], run: &str) -> Vec<u8> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let data: String = data
        .iter()
        .map(|(at, s)| format!("  (data (i32.const {at}) \"{}\")\n", quote(s)))
        .collect();
    let wat = format!(
        r#"(module
  {imports}
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 8192))
  (data (i32.const 16) "{manifest}")
{data}  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const {len})))
  (func (export "ferris_idea_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    {run})
)"#,
        manifest = quote(manifest),
        len = manifest.len(),
    );
    wat::parse_str(&wat).unwrap()
}

/// Returns the `len` bytes at `at` (an i64 the host reads as pointer and length).
fn answer(at: u32, len: usize) -> String {
    format!("(i64.or (i64.shl (i64.const {at}) (i64.const 32)) (i64.const {len}))")
}

/// A text action: `Upper Case` turns the selection's ASCII letters into capitals.
fn upper_plugin() -> Vec<u8> {
    module(
        r#"{"name":"Upper","version":"1.0","actions":[{"id":"upper","title":"Upper Case"}]}"#,
        "",
        &[],
        r#"(local $i i32) (local $c i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $l)))
        (local.set $c (i32.load8_u (i32.add (local.get $p) (local.get $i))))
        (if (i32.and (i32.ge_u (local.get $c) (i32.const 97)) (i32.le_u (local.get $c) (i32.const 122)))
          (then (local.set $c (i32.sub (local.get $c) (i32.const 32)))))
        (i32.store8 (i32.add (local.get $p) (local.get $i)) (local.get $c))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (i64.or (i64.shl (i64.extend_i32_u (local.get $p)) (i64.const 32)) (i64.extend_i32_u (local.get $l)))"#,
    )
}

/// An `edit` action that needs the project grant: `Mark` reads `README.md` through the host; it
/// answers "denied" without the grant, else puts `// ` before the text and selects it.
fn marker_plugin(version: &str) -> Vec<u8> {
    let denied = r#"{"edits":[],"message":"denied"}"#;
    let marked =
        r#"{"edits":[{"start":0,"end":0,"text":"// "}],"select":[0,3],"message":"marked"}"#;
    module(
        &format!(
            r#"{{"name":"Marker","version":"{version}","permissions":["project-read"],"actions":[{{"id":"mark","title":"Mark","kind":"edit"}}]}}"#
        ),
        r#"(import "ferris-idea" "read_file" (func $read (param i32 i32) (result i64)))"#,
        &[(1024, denied), (2048, "README.md"), (3072, marked)],
        &format!(
            r#"(if (i64.lt_s (call $read (i32.const 2048) (i32.const 9)) (i64.const 0))
      (then (return {})))
    {}"#,
            answer(1024, denied.len()),
            answer(3072, marked.len())
        ),
    )
}

/// An `inspect` action on `.txt` files: a `TODO` at the start is a warning, its fix removes it.
fn todo_plugin() -> Vec<u8> {
    let found = r#"{"findings":[{"start":0,"end":4,"message":"TODO left","fix":{"title":"Remove TODO","edits":[{"start":0,"end":5,"text":""}]}}]}"#;
    module(
        r#"{"name":"Todo","version":"1.0","actions":[{"id":"todo","title":"TODO check","kind":"inspect","files":["txt"]}]}"#,
        "",
        &[(1024, found)],
        &answer(1024, found.len()),
    )
}

fn plugins_dir(bench: &Bench, cx: &mut VisualTestContext, root: &Path) -> PathBuf {
    let dir = root.join(".plugins");
    std::fs::create_dir_all(&dir).unwrap();
    bench.update(cx, |b, cx| {
        b.set_plugins_dir(dir.clone());
        b.reload_plugins(cx);
    });
    draw(cx);
    dir
}

fn settings(bench: &Bench, cx: &mut VisualTestContext) -> Entity<SettingsView> {
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(Page::Plugins, window, cx)
    });
    draw(cx);
    bench
        .read_with(cx, |b, _| b.settings_dialog())
        .expect("Settings is open")
}

/// Hand test 27 (`SPEC-0104`): a text plugin's action is found by Find Action and replaces the
/// selection; a broken plugin next to it is skipped and breaks nothing.
#[gpui::test]
fn hand_test_27_a_text_plugin_from_find_action_next_to_a_broken_one(cx: &mut TestAppContext) {
    let root = workspace("27", &[("shout.txt", "hello plugin\n")]);
    let (bench, cx) = window(&root, cx);
    let dir = root.join(".plugins");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("upper.wasm"), upper_plugin()).unwrap();
    std::fs::write(dir.join("broken.wasm"), b"not a module").unwrap();
    let dir = plugins_dir(&bench, cx, &root);
    assert_eq!(dir, root.join(".plugins"));
    assert_eq!(
        bench.read_with(cx, |b, _| b.plugin_count()),
        1,
        "the broken plugin is skipped"
    );
    open(&bench, cx, &root.join("shout.txt"));
    keys(
        cx,
        "shift-right shift-right shift-right shift-right shift-right",
    );
    find_action(&bench, cx, "Upper Case", "Plugin.upper.upper");
    until(cx, 10, "the selection is replaced", |cx| {
        text(&bench, cx) == "HELLO plugin\n"
    });
    // The window goes on working: typing still edits the file.
    keys(cx, "end");
    cx.simulate_input("!");
    draw(cx);
    assert_eq!(text(&bench, cx), "HELLO plugin!\n");
    // Settings › Plugins names the broken one with its problem.
    let view = settings(&bench, cx);
    let listed = view.read_with(cx, |v, _| {
        v.plugins()
            .iter()
            .map(|p| (p.file.clone(), p.manifest.is_some()))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        listed,
        [("broken".to_owned(), false), ("upper".to_owned(), true)]
    );
    keys(cx, "escape");
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 82 (`SPEC-0156`): an `edit` action without the project grant changes nothing and
/// says so; Space on the Plugins page grants it; then its edit is applied and selected, and one ⌘Z
/// undoes it.
#[gpui::test]
fn hand_test_82_an_edit_action_with_and_without_the_grant(cx: &mut TestAppContext) {
    let root = workspace("82", &[("mark.txt", "text\n"), ("README.md", "# Hello\n")]);
    let (bench, cx) = window(&root, cx);
    let dir = root.join(".plugins");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("marker.wasm"), marker_plugin("1.0")).unwrap();
    plugins_dir(&bench, cx, &root);
    open(&bench, cx, &root.join("mark.txt"));
    find_action(&bench, cx, "Mark", "Plugin.marker.mark");
    until(cx, 10, "the plugin says it was denied", |cx| {
        status(&bench, cx) == "denied"
    });
    assert_eq!(text(&bench, cx), "text\n", "nothing changed");

    let view = settings(&bench, cx);
    assert!(!view.read_with(cx, |v, _| v.plugin_granted("marker")));
    keys(cx, "space");
    assert!(
        view.read_with(cx, |v, _| v.plugin_granted("marker")),
        "Space grants the project access"
    );
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.settings_dialog()).is_none());
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    draw(cx);

    find_action(&bench, cx, "Mark", "Plugin.marker.mark");
    until(cx, 10, "the edit is applied", |cx| {
        text(&bench, cx) == "// text\n"
    });
    let selected = bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .and_then(|e| e.read(cx).selection_range())
    });
    assert_eq!(selected, Some(0..3), "the edit is selected");
    assert_eq!(status(&bench, cx), "marked");
    keys(cx, "cmd-z");
    assert_eq!(
        text(&bench, cx),
        "text\n",
        "one ⌘Z undoes the plugin's edit"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Hand test 92 (`SPEC-0166`): a plugin's inspection marks `TODO` as a warning after a pause;
/// ⌥↩ offers its fix, typing narrows the list to it and Enter applies it.
#[gpui::test]
fn hand_test_92_a_plugin_inspection_and_its_fix(cx: &mut TestAppContext) {
    let root = workspace("92", &[("todo.txt", "TODO write\n")]);
    let (bench, cx) = window(&root, cx);
    let dir = root.join(".plugins");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("todo.wasm"), todo_plugin()).unwrap();
    plugins_dir(&bench, cx, &root);
    let file = root.join("todo.txt");
    open(&bench, cx, &file);
    until(cx, 15, "the finding is a warning in the editor", |cx| {
        bench.read_with(cx, |b, cx| {
            b.plugin_findings(&file) == ["Todo: TODO left"]
                && b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).status().problems.1 > 0)
        })
    });
    keys(cx, "right");
    keys(cx, "alt-enter");
    let popup = bench
        .read_with(cx, |b, _| b.intentions_popup())
        .expect("⌥↩ opens the list");
    let titles = popup.read_with(cx, |v, _| v.titles());
    assert!(titles.iter().any(|t| t == "Remove TODO"), "{titles:?}");
    cx.simulate_input("Remove TODO");
    draw(cx);
    assert_eq!(
        popup
            .read_with(cx, |v, _| v.titles())
            .first()
            .map(String::as_str),
        Some("Remove TODO")
    );
    keys(cx, "enter");
    until(cx, 10, "the fix removes the TODO", |cx| {
        text(&bench, cx) == "write\n"
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// The sample plugins of `tests/plugins` (`SPEC-0210`), one per extension point.
const SAMPLES: [(&str, &[u8]); 4] = [
    (
        "complete",
        include_bytes!("../../../../tests/plugins/complete.wasm"),
    ),
    (
        "filetype",
        include_bytes!("../../../../tests/plugins/filetype.wasm"),
    ),
    (
        "status",
        include_bytes!("../../../../tests/plugins/status.wasm"),
    ),
    (
        "panel",
        include_bytes!("../../../../tests/plugins/panel.wasm"),
    ),
];

/// Hand test 150 (`SPEC-0210`): the plugin file type colours `notes.conf` and ⌘/ uses its `#`;
/// ⌃Space lists the plugin's words and Enter inserts one; the status bar shows "Lines: 3"; the
/// Plugins tool window shows "Sample Panel: Bookmarks" and a click on a row opens its line.
#[gpui::test]
fn hand_test_150_the_four_extension_points(cx: &mut TestAppContext) {
    let root = workspace(
        "150",
        &[
            ("notes.conf", "# notes\nserver on\nplu"),
            ("other.txt", "x\n"),
        ],
    );
    let (bench, cx) = window(&root, cx);
    let dir = root.join(".plugins");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in SAMPLES {
        std::fs::write(dir.join(format!("{name}.wasm")), bytes).unwrap();
    }
    plugins_dir(&bench, cx, &root);
    open(&bench, cx, &root.join("notes.conf"));
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .unwrap();
    until(cx, 10, "the plugin's file type colours the file", |cx| {
        editor.read_with(cx, |e, _| {
            e.plugin_file_type() == Some("Conf")
                && e.plugin_span_kinds() == ["comment", "keyword", "keyword"]
        })
    });
    until(cx, 10, "the status bar shows the plugin's widget", |cx| {
        let shown = node(cx, "plugin-status-0");
        shown.is_some_and(|(name, _)| name == "Lines: 3")
    });

    // ⌘/ on the second line uses the file type's `#`; ⌘Z takes it back.
    keys(cx, "down");
    keys(cx, "cmd-/");
    assert_eq!(text(&bench, cx), "# notes\n#server on\nplu");
    keys(cx, "cmd-z");
    assert_eq!(text(&bench, cx), "# notes\nserver on\nplu");

    // ⌃Space at the end lists the plugin's words; Enter inserts the first.
    keys(cx, "down");
    keys(cx, "end");
    keys(cx, "ctrl-space");
    until(cx, 10, "⌃Space lists the plugin's items", |cx| {
        bench.read_with(cx, |b, _| {
            b.completion_items()
                .is_some_and(|items| items == ["plugin_hello", "plugin_world"])
        })
    });
    keys(cx, "enter");
    assert_eq!(text(&bench, cx), "# notes\nserver on\nplugin_hello");

    // The Plugins tool window from Find Action; a click on a row opens its file at its line.
    open(&bench, cx, &root.join("other.txt"));
    find_action(&bench, cx, "Plugins", "ActivatePluginsToolWindow");
    until(cx, 10, "the Plugins tool window shows the panel", |cx| {
        bench.read_with(cx, |b, _| b.tool_window_shown(ToolWindow::Plugins))
            && node(cx, "plugin-panel-0").is_some_and(|(n, _)| n == "Sample Panel: Bookmarks")
            && node(cx, "plugin-panel-row-0-1").is_some()
    });
    click(cx, "plugin-panel-row-0-0");
    let (path, line) = bench.read_with(cx, |b, cx| {
        (
            b.active_path().map(|p| p.to_path_buf()),
            b.caret_position(cx).map(|(line, _)| line),
        )
    });
    assert!(
        path.is_some_and(|p| p.ends_with("notes.conf")),
        "the row opens notes.conf"
    );
    assert_eq!(line, Some(2), "at the row's line");
    let _ = std::fs::remove_dir_all(&root);
}

/// The headline of the review in the window, once it is shown.
fn review_headline(cx: &mut VisualTestContext) -> String {
    let mut headline = String::new();
    until(cx, 10, "the review is shown", |cx| {
        headline = node(cx, "plugin-review-headline")
            .map(|(n, _)| n)
            .unwrap_or_default();
        !headline.is_empty()
    });
    headline
}

/// Install Plugin from Disk… with the file the dialog would return (the harness's platform has
/// no file dialog; the dialog itself is checked next to the reference).
fn install_from_disk(view: &Entity<SettingsView>, cx: &mut VisualTestContext, file: &Path) {
    view.update(cx, |v, cx| v.install_plugin(file, cx));
    draw(cx);
}

fn installed_version(view: &Entity<SettingsView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.plugins()
            .iter()
            .filter_map(|p| p.manifest.as_ref().map(|m| m.version.clone()))
            .collect()
    })
}

/// Hand test 151 (`SPEC-0211`): Install from Disk shows the review; Space ticks the grant and the Install button installs; the same file again
/// is a Reinstall (Escape keeps everything); a newer one is an Update that keeps the grant; an
/// older one is a Downgrade, Cancel leaves the newer one.
#[gpui::test]
fn hand_test_151_install_reinstall_update_downgrade(cx: &mut TestAppContext) {
    let root = workspace("151", &[("notes.txt", "notes\n")]);
    for (folder, version) in [("v10", "1.0"), ("v11", "1.1")] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(
            root.join(folder).join("marker.wasm"),
            marker_plugin(version),
        )
        .unwrap();
    }
    let (old, new) = (root.join("v10/marker.wasm"), root.join("v11/marker.wasm"));
    let (bench, cx) = window(&root, cx);
    plugins_dir(&bench, cx, &root);
    let view = settings(&bench, cx);

    // The page offers the button that opens the file dialog (⌘O does the same).
    assert_eq!(
        name_of(cx, "settings-plugins-install-from-disk"),
        "Install Plugin from Disk…"
    );
    assert!(node(cx, "plugin-review-headline").is_none());

    install_from_disk(&view, cx, &old);
    assert_eq!(review_headline(cx), "Install Marker 1.0");
    assert!(
        view.read_with(cx, |v, _| v.plugins().is_empty()),
        "only shown"
    );
    assert_eq!(name_of(cx, "plugin-review-grant-0"), "reads the project");
    keys(cx, "space");
    assert_eq!(
        view.read_with(cx, |v, _| v.review_grants().len()),
        1,
        "Space ticks the grant"
    );
    click(cx, "plugin-review-install");
    assert_eq!(installed_version(&view, cx), ["1.0"]);
    assert!(view.read_with(cx, |v, _| v.plugin_granted("marker")));
    assert_eq!(bench.read_with(cx, |b, _| b.plugin_count()), 1, "loaded");

    install_from_disk(&view, cx, &old);
    assert_eq!(review_headline(cx), "Reinstall Marker 1.0");
    keys(cx, "escape");
    assert!(node(cx, "plugin-review-headline").is_none());
    assert!(
        view.read_with(cx, |v, _| v.page() == Page::Plugins),
        "Escape drops only the review"
    );

    install_from_disk(&view, cx, &new);
    assert_eq!(review_headline(cx), "Update Marker 1.0 → 1.1");
    click(cx, "plugin-review-install");
    assert_eq!(installed_version(&view, cx), ["1.1"]);
    assert!(
        view.read_with(cx, |v, _| v.plugin_granted("marker")),
        "the update keeps the grant"
    );

    install_from_disk(&view, cx, &old);
    assert_eq!(review_headline(cx), "Downgrade Marker 1.1 → 1.0");
    click(cx, "plugin-review-cancel");
    assert!(node(cx, "plugin-review-headline").is_none());
    assert_eq!(installed_version(&view, cx), ["1.1"], "Cancel keeps 1.1");
    let _ = std::fs::remove_dir_all(&root);
}
