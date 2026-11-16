//! `SPEC-0327` v3 AK-9, AK-10 and AK-12 in a window of the test harness, by clicks on the
//! controls found by their test ids in the semantic tree (`SPEC-0319`): folders are made,
//! renamed and removed and configurations move into and out of them; the template of a type is
//! edited, written, and copied into a new configuration of the type.

#![cfg(target_os = "macos")]

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use idecraft_core::{BeforeTask, RunTemplate, parse_run_configs, parse_run_templates};
use idecraft_workbench::{RunConfigDialog, WorkbenchView, semantics};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-runcfg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".idecraft")).unwrap();
    std::fs::write(
        root.join(".idecraft/run.toml"),
        "# The API.\n[[run]]\nname = \"api\"\ncommand = [\"echo\", \"api\"]\n\n[[run]]\nname = \"web\"\ncommand = [\"echo\", \"web\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("main.js"), "console.log(1)\n").unwrap();
    root
}

fn open(
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<RunConfigDialog>,
    &'static mut VisualTestContext,
) {
    semantics::set_consumer(semantics::Consumer::SelfTest, true);
    idecraft_workbench::hit_targets::enable();
    let file = root.join("main.js");
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.open_run_config_editor(Some("api".into()), window, cx);
    });
    cx.run_until_parked();
    let dialog = bench
        .read_with(cx, |b, _| b.run_config_dialog())
        .expect("the editor is open");
    (bench, dialog, cx)
}

/// A click on the control with test id `id`, at the centre of what is shown of it. Every
/// control of the dialog is in view without scrolling (the window self-test's driver cannot
/// scroll), so a control that is not is a failure.
fn click(id: &str, cx: &mut VisualTestContext) {
    cx.run_until_parked();
    let at = cx.update(|window, _| {
        let tree = semantics::tree(semantics::window_id(window)).expect("a tree is built");
        let node = tree
            .iter()
            .find(|n| n.test_id() == Some(id))
            .unwrap_or_else(|| panic!("no control {id} in the window"));
        let r = node
            .visible_bounds
            .filter(|r| r.w > 0. && r.h > 0.)
            .unwrap_or_else(|| panic!("the control {id} is not in view"));
        point(px(r.x + r.w / 2.), px(r.y + r.h / 2.))
    });
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn has(id: &str, cx: &mut VisualTestContext) -> bool {
    cx.run_until_parked();
    cx.update(|window, _| {
        semantics::tree(semantics::window_id(window))
            .is_some_and(|t| t.iter().any(|n| n.test_id() == Some(id)))
    })
}

fn run_toml(root: &std::path::Path) -> String {
    std::fs::read_to_string(root.join(".idecraft/run.toml")).unwrap()
}

/// AK-9: New Folder, Rename, moving configurations in and out, Remove Folder; Apply writes the
/// `folder` keys and keeps the file's comments.
#[gpui::test]
fn folders_are_made_renamed_and_removed_by_clicks(cx: &mut TestAppContext) {
    let root = workspace("folders");
    let (_bench, dialog, cx) = open(&root, cx);
    let folders = |cx: &mut VisualTestContext| dialog.read_with(cx, |d, _| d.model().folders());
    let folder_of = |name: &str, cx: &mut VisualTestContext| {
        dialog.read_with(cx, |d, _| {
            let m = d.model();
            m.entries
                .iter()
                .position(|e| e.name() == name)
                .and_then(|i| m.folder_of(i))
        })
    };
    click("run-config-new-folder", cx);
    assert_eq!(folders(cx), ["New Folder"]);
    assert_eq!(
        dialog.read_with(cx, |d, _| d.model().selected_folder.clone()),
        Some("New Folder".to_owned())
    );
    assert!(has("run-config-folder-New Folder", cx));
    // A folder without configurations is not something to write.
    assert!(!dialog.read_with(cx, |d, _| d.model().dirty()));
    dialog.update(cx, |d, cx| d.set_field("Folder name", "Backend", cx));
    click("run-config-folder-rename", cx);
    assert_eq!(folders(cx), ["Backend"]);
    // Into the folder, by its chip under the Folder field.
    click("run-config-row-api", cx);
    click("run-config-move-to-Backend", cx);
    click("run-config-row-web", cx);
    click("run-config-move-to-Backend", cx);
    assert_eq!(folder_of("api", cx).as_deref(), Some("Backend"));
    assert_eq!(folder_of("web", cx).as_deref(), Some("Backend"));
    // And out again.
    click("run-config-move-out", cx);
    assert_eq!(folder_of("web", cx), None);
    click("run-config-apply", cx);
    let text = run_toml(&root);
    assert!(text.contains("# The API."), "{text}");
    let configs = parse_run_configs(&text);
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    assert_eq!(configs.configs[0].folder.as_deref(), Some("Backend"));
    assert_eq!(configs.configs[1].folder, None);
    // Rename a folder with a configuration in it: the configuration follows.
    click("run-config-folder-Backend", cx);
    dialog.update(cx, |d, cx| d.set_field("Folder name", "Services", cx));
    click("run-config-folder-rename", cx);
    assert_eq!(folder_of("api", cx).as_deref(), Some("Services"));
    // Remove Folder: the configuration stays, listed on its own.
    click("run-config-folder-remove", cx);
    assert!(folders(cx).is_empty());
    assert_eq!(folder_of("api", cx), None);
    assert!(has("run-config-row-api", cx));
    click("run-config-apply", cx);
    let configs = parse_run_configs(&run_toml(&root));
    assert_eq!(configs.configs.len(), 2);
    assert!(configs.configs.iter().all(|c| c.folder.is_none()));
    let _ = std::fs::remove_dir_all(&root);
}

/// AK-10: the template of a type is edited in the dialog and written to `[template.<type>]`; a
/// new configuration of the type is a copy of it; Reset removes it.
#[gpui::test]
fn a_types_template_is_edited_written_and_copied_by_clicks(cx: &mut TestAppContext) {
    let root = workspace("templates");
    let (_bench, dialog, cx) = open(&root, cx);
    click("run-config-templates", cx);
    assert!(dialog.read_with(cx, |d, _| d.templates_shown()));
    click("run-config-type-node_file", cx);
    dialog.update(cx, |d, cx| {
        d.set_field("Interpreter", "/opt/node22/bin/node", cx);
        d.set_field("Options", "--enable-source-maps", cx);
        d.set_field("Working directory", "web", cx);
        d.set_field("Environment", "NODE_ENV=development", cx);
        d.set_field("Timeout (seconds)", "120", cx);
    });
    click("run-config-allow-multiple", cx);
    click("run-config-before-add", cx);
    click("run-config-before-api", cx);
    click("run-config-before-add", cx);
    click("run-config-task-build", cx);
    click("run-config-apply", cx);
    assert_eq!(dialog.read_with(cx, |d, _| d.problems().to_vec()), [""; 0]);
    let text = run_toml(&root);
    assert!(text.contains("[template.node_file]"), "{text}");
    assert!(text.contains("# The API."));
    let templates = parse_run_templates(&text);
    assert!(templates.problems.is_empty(), "{:?}", templates.problems);
    let t = &templates.templates[&RunTemplate::NodeFile];
    assert_eq!(t.interpreter.as_deref(), Some("/opt/node22/bin/node"));
    assert_eq!(t.options, ["--enable-source-maps"]);
    assert_eq!(t.cwd.as_deref(), Some("web"));
    assert_eq!(t.before, ["api"]);
    assert_eq!(t.before_launch, [BeforeTask::build()]);
    assert!(t.allow_multiple);
    assert_eq!(t.timeout, Some(120));
    // The configurations are as they were.
    assert_eq!(parse_run_configs(&text).configs.len(), 2);
    // Back to the configurations; a new Node.js file configuration copies the template.
    click("run-config-configurations", cx);
    assert!(!dialog.read_with(cx, |d, _| d.templates_shown()));
    click("run-config-add", cx);
    click("run-config-new-node_file", cx);
    let made = dialog.read_with(cx, |d, _| {
        d.model().current().and_then(|e| e.form.config().ok())
    });
    let made = made.expect("a new configuration is selected");
    assert_eq!(made.name, "Node.js file");
    assert_eq!(
        made.command,
        ["/opt/node22/bin/node", "--enable-source-maps", "index.js"]
    );
    assert_eq!(made.cwd.as_deref(), Some("web"));
    assert_eq!(
        made.env.get("NODE_ENV").map(String::as_str),
        Some("development")
    );
    assert_eq!(made.before, ["api"]);
    assert_eq!(made.before_launch, [BeforeTask::build()]);
    assert!(made.allow_multiple);
    assert_eq!(made.timeout, Some(120));
    // Another type has no template: its command alone.
    click("run-config-add", cx);
    click("run-config-new-python_file", cx);
    let plain = dialog
        .read_with(cx, |d, _| {
            d.model().current().and_then(|e| e.form.config().ok())
        })
        .unwrap();
    assert_eq!(plain.command, ["python3", "main.py"]);
    assert_eq!(plain.cwd, None);
    // Reset removes the template; the configuration made from it keeps its copy.
    click("run-config-templates", cx);
    click("run-config-type-node_file", cx);
    click("run-config-template-reset", cx);
    click("run-config-apply", cx);
    let text = run_toml(&root);
    assert!(!text.contains("[template"), "{text}");
    let configs = parse_run_configs(&text);
    assert!(configs.problems.is_empty(), "{:?}", configs.problems);
    let site = configs
        .configs
        .iter()
        .find(|c| c.name == "Node.js file")
        .expect("written");
    assert_eq!(site.cwd.as_deref(), Some("web"));
    let _ = std::fs::remove_dir_all(&root);
}
