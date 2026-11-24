//! Window checks of OBS-0096 AE1 and AE2 (`SPEC-0076` v11 AK-11 to AK-13): ⌥↩ at a class with
//! `main` lists Run, Debug and Profile; an intention has a submenu with Disable and the intention
//! settings, and a disabled one is gone until the General page turns it on again; F1 shows the
//! preview beside the list, before and after side by side. No language server takes part: the
//! server's answer is handed to the list directly. The test's own settings file is used, never
//! the user's.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_languages::actions::CodeAction;
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{EditorView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

const INVENTORY: &str = "package shop;

public class Inventory {
    public static void main(String[] args) {
        String s = args[0];
        if (s == \"x\") {}
    }
}
";

fn folder(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-ctx-actions-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

fn open(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
    file: &Path,
) -> Entity<EditorView> {
    bench.update_in(cx, |b, window, cx| {
        b.open(file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
    bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open")
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn intention(title: &str) -> CodeAction {
    CodeAction {
        title: title.into(),
        kind: Some("source".into()),
        preferred: false,
        disabled: None,
        fixes: Vec::new(),
    }
}

#[gpui::test]
fn run_rows_intention_submenus_and_the_preview_beside_the_list(cx: &mut TestAppContext) {
    let settings = folder("settings");
    let file = settings.join("settings.toml");
    let root = folder("project");
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    let java = root.join("src/shop/Inventory.java");
    std::fs::write(&java, INVENTORY).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(file.clone()));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.clone();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    let editor = open(&bench, cx, &java);
    until(cx, "the run markers are read", |cx| {
        bench.read_with(cx, |b, _| !b.run_markers_of(&java).is_empty())
    });

    // AK-11: ⌥↩ on the class line lists Run, Debug and, where the profiler can run Java, Profile.
    let class = INVENTORY.find("Inventory {").unwrap();
    editor.update(cx, |e, cx| e.select_range(class..class, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ShowIntentionActions", window, cx)
    });
    until(cx, "the context actions open", |cx| {
        bench.read_with(cx, |b, _| b.intentions_popup().is_some())
    });
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    let titles = popup.read_with(cx, |v, _| v.titles());
    let profile = ferris_idea_profiler::can_profile(&["java".into(), "A.java".into()]);
    let mut expected = vec!["Run 'Inventory'", "Debug 'Inventory'"];
    if profile {
        expected.push("Profile 'Inventory'");
    }
    assert_eq!(&titles[..expected.len()], &expected[..], "{titles:?}");
    assert!(
        popup
            .read_with(cx, |v, _| v.footer())
            .contains("toggles preview")
    );

    // AK-11: the preview of the selected run row names its command.
    popup.update(cx, |v, cx| v.toggle_preview(cx));
    until(cx, "the run row's preview", |cx| {
        popup.read_with(cx, |v, _| v.preview_lines()).is_some()
    });
    let lines = popup
        .read_with(cx, |v, _| v.preview_lines())
        .unwrap()
        .unwrap();
    assert!(
        lines
            .iter()
            // The module is compiled before the run (SPEC-0097 v4 behaviour 10).
            .any(|l| l.contains("$ sh -c") && l.contains("'shop.Inventory'")),
        "{lines:?}"
    );
    cx.update(|window, cx| window.draw(cx).clear());

    // AK-13: on the comparison of strings the own fix's preview stands beside the list, before
    // and after.
    let eq = INVENTORY.find("==").unwrap();
    editor.update(cx, |e, cx| e.select_range(eq..eq, cx));
    cx.run_until_parked();
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ShowIntentionActions", window, cx)
    });
    until(cx, "the context actions at the comparison", |cx| {
        bench.read_with(cx, |b, cx| {
            b.intentions_popup()
                .is_some_and(|p| p.read(cx).titles().iter().any(|t| t.contains("equals")))
        })
    });
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    popup.update(cx, |v, cx| v.toggle_preview(cx));
    until(cx, "the fix's preview", |cx| {
        popup.read_with(cx, |v, _| v.preview_lines()).is_some()
    });
    let lines = popup
        .read_with(cx, |v, _| v.preview_lines())
        .unwrap()
        .unwrap();
    let blocks = ferris_idea_languages::context_actions::blocks_of_lines(&lines);
    assert!(
        blocks.iter().any(|b| matches!(b,
            ferris_idea_languages::context_actions::PreviewBlock::Change { before, after }
                if before.iter().any(|l| l.contains("s == \"x\""))
                    && after.iter().any(|l| l.contains("equals(s)")))),
        "{lines:?}"
    );
    cx.update(|window, cx| window.draw(cx).clear());

    // AK-12: an intention has Disable and the settings in its submenu; Disable writes the test's
    // settings file, and the intention is left out of the next list.
    editor.update(cx, |e, cx| e.select_range(class..class, cx));
    let javadoc = "Add Javadoc for 'Inventory'";
    bench.update_in(cx, |b, window, cx| {
        b.open_context_actions_with(
            vec![intention(javadoc), intention("Sort members")],
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    popup.update(cx, |v, cx| {
        v.select_title(javadoc, cx);
        v.open_submenu(cx);
    });
    assert_eq!(
        popup.read_with(cx, |v, _| v.submenu_labels()),
        Some(vec![
            format!("Disable '{javadoc}'"),
            "Edit intention settings".to_owned()
        ])
    );
    assert!(
        popup
            .read_with(cx, |v, _| v.footer())
            .ends_with("→ options")
    );
    cx.update(|window, cx| window.draw(cx).clear());
    popup.update(cx, |v, cx| {
        v.choose_option(&format!("Disable '{javadoc}'"), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        ferris_idea_core::load_ide_settings(&file)
            .intentions
            .disabled,
        [javadoc]
    );
    bench.update_in(cx, |b, window, cx| {
        b.open_context_actions_with(
            vec![intention(javadoc), intention("Sort members")],
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let popup = bench.read_with(cx, |b, _| b.intentions_popup()).unwrap();
    assert_eq!(popup.read_with(cx, |v, _| v.titles()), ["Sort members"]);

    // The General page lists it; Enter on its row turns it on again.
    bench.update_in(cx, |b, window, cx| {
        b.open_settings(
            ferris_idea_workbench::settings_model::Page::Editor,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let dialog = bench.read_with(cx, |b, _| b.settings_dialog()).unwrap();
    cx.update(|window, cx| window.draw(cx).clear());
    dialog.update_in(cx, |v, window, cx| {
        v.select_row(12, cx);
        window.focus(&v.focus());
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        ferris_idea_core::load_ide_settings(&file)
            .intentions
            .disabled
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}
