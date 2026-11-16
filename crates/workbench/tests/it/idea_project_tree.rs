//! The modules of the reference IDE's `.idea` in the Project tree (`SPEC-0049` v2 AK-6), in a
//! window of the test harness with real keys: the roles of module, source, test, resource and
//! excluded folders without a build file, the tree's keys reaching the rows, Synchronize after a
//! module file changed, and a Maven POM winning for its folders. Only the project's own `.idea`
//! is read, as SPEC-0049 allows; nothing in it is written.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_core::buildtools::FolderRole;
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};

type Bench = Entity<WorkbenchView>;

const MODULES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectModuleManager">
    <modules>
      <module fileurl="file://$PROJECT_DIR$/app.iml" filepath="$PROJECT_DIR$/app.iml" />
      <module fileurl="file://$PROJECT_DIR$/library/lib.iml" filepath="$PROJECT_DIR$/library/lib.iml" />
    </modules>
  </component>
</project>"#;

fn app_iml(extra: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<module type="JAVA_MODULE" version="4">
  <component name="NewModuleRootManager">
    <output url="file://$MODULE_DIR$/out/production" />
    <output-test url="file://$MODULE_DIR$/out/test" />
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/code" isTestSource="false" />
      <sourceFolder url="file://$MODULE_DIR$/checks" isTestSource="true" />
      <sourceFolder url="file://$MODULE_DIR$/assets" type="java-resource" />
      <sourceFolder url="file://$MODULE_DIR$/src/main/java" isTestSource="true" />
      {extra}
      <excludeFolder url="file://$MODULE_DIR$/tmp" />
    </content>
    <orderEntry type="inheritedJdk" />
  </component>
</module>"#
    )
}

const LIB_IML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<module type="JAVA_MODULE" version="4">
  <component name="NewModuleRootManager" inherit-compiler-output="true">
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/java" isTestSource="false" />
    </content>
  </component>
</module>"#;

fn project() -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-idea-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in [
        ".idea",
        "code/shop",
        "checks/shop",
        "assets",
        "src/main/java/shop",
        "tmp",
        "out/production",
        "out/test",
        "gen",
        "library/java/util",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let files = [
        (".idea/modules.xml", MODULES.to_owned()),
        ("app.iml", app_iml("")),
        ("library/lib.iml", LIB_IML.to_owned()),
        (
            "code/shop/Cart.java",
            "package shop;\n\npublic class Cart {}\n".to_owned(),
        ),
        (
            "checks/shop/CartTest.java",
            "package shop;\n\nclass CartTest {}\n".to_owned(),
        ),
        (
            "library/java/util/Text.java",
            "package util;\n\npublic class Text {}\n".to_owned(),
        ),
        (
            "src/main/java/shop/Main.java",
            "package shop;\n\nclass Main {}\n".to_owned(),
        ),
    ];
    for (rel, text) in files {
        std::fs::write(root.join(rel), text).unwrap();
    }
    root.canonicalize().unwrap()
}

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(idecraft_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

fn role(bench: &Bench, cx: &mut VisualTestContext, root: &Path, rel: &str) -> Option<FolderRole> {
    let path = if rel == "." {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    bench.read_with(cx, |b, _| b.tree_folder_info(&path).map(|i| i.role))
}

/// SPEC-0049 v2 AK-6.
#[gpui::test]
fn idea_modules_mark_the_project_tree(cx: &mut TestAppContext) {
    let root = project();
    let idea_before: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(root.join(".idea"))
        .unwrap()
        .flatten()
        .map(|e| (e.path(), e.metadata().unwrap().modified().unwrap()))
        .collect();
    let (bench, cx) = window(&root, cx);

    assert_eq!(role(&bench, cx, &root, "code"), Some(FolderRole::Sources));
    assert_eq!(role(&bench, cx, &root, "checks"), Some(FolderRole::Tests));
    assert_eq!(
        role(&bench, cx, &root, "assets"),
        Some(FolderRole::Resources)
    );
    assert_eq!(role(&bench, cx, &root, "tmp"), Some(FolderRole::Excluded));
    assert_eq!(
        role(&bench, cx, &root, "out/production"),
        Some(FolderRole::Excluded)
    );
    assert_eq!(
        role(&bench, cx, &root, "out/test"),
        Some(FolderRole::Excluded)
    );
    assert_eq!(role(&bench, cx, &root, "library"), Some(FolderRole::Module));
    assert_eq!(
        role(&bench, cx, &root, "library/java"),
        Some(FolderRole::Sources)
    );
    assert_eq!(
        role(&bench, cx, &root, "src/main/java"),
        Some(FolderRole::Tests)
    );
    assert_eq!(role(&bench, cx, &root, "gen"), None);
    let name = bench.read_with(cx, |b, _| {
        b.tree_folder_info(&root).and_then(|i| i.module_name)
    });
    assert_eq!(
        name.as_deref(),
        Some("app"),
        "the module's name differs from the folder's"
    );
    let roots = bench.read_with(cx, |b, _| b.tree_source_roots());
    assert!(roots.contains(&root.join("code")), "{roots:?}");
    assert!(roots.contains(&root.join("library/java")), "{roots:?}");

    // The tree's keys reach the marked rows: the project row expands with →, ↓ walks it.
    bench.update_in(cx, |b, window, cx| {
        assert!(b.reveal_in_tree(&root.join("assets"), cx));
        b.focus_tree(window, cx);
    });
    cx.run_until_parked();
    let rows = bench.read_with(cx, |b, _| b.tree_row_names());
    for name in ["assets", "checks", "code", "library", "tmp"] {
        assert!(
            rows.iter().any(|r| r.starts_with(name)),
            "{name} in {rows:?}"
        );
    }
    keys(cx, "down");
    assert_eq!(
        bench.read_with(cx, |b, _| b.tree_selected_path()),
        root.join("checks"),
        "↓ from assets reaches the test folder"
    );

    // The module file changes on disk; Synchronize (⌥⌘Y) marks the new source folder.
    std::fs::write(
        root.join("app.iml"),
        app_iml(r#"<sourceFolder url="file://$MODULE_DIR$/gen" isTestSource="false" />"#),
    )
    .unwrap();
    keys(cx, "cmd-alt-y");
    assert_eq!(role(&bench, cx, &root, "gen"), Some(FolderRole::Sources));

    // A Maven POM at the top wins for its folders: `src/main/java` is sources again.
    std::fs::write(
        root.join("pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>shop</artifactId><version>1</version></project>\n",
    )
    .unwrap();
    keys(cx, "cmd-alt-y");
    assert_eq!(
        role(&bench, cx, &root, "src/main/java"),
        Some(FolderRole::Sources)
    );
    assert_eq!(role(&bench, cx, &root, "code"), Some(FolderRole::Sources));

    // Nothing in `.idea` was written.
    for (path, modified) in idea_before {
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified,
            "{} is unchanged",
            path.display()
        );
    }
    assert_eq!(std::fs::read_dir(root.join(".idea")).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(&root);
}
