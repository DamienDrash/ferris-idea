//! The ninth package of actions taken from the keymap, carried out in a window of the test
//! harness: Safe Delete of a declaration and in the Project tree (`SPEC-0095` v12 behaviours 15
//! and 16), Reformat File… (`SPEC-0075` v6 behaviour 8), Force Refresh (`SPEC-0059` v9.8
//! behaviour 30) and Header/Source (`SPEC-0071` v12 behaviour 19). Safe Delete's search is
//! answered without a language server.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use idecraft_languages::find_usages::UsageSource;
use idecraft_languages::insight::{Location, Range, Symbol};
use idecraft_languages::safe_delete::{SafeDeletePlan, plan_from};
use idecraft_workbench::WorkbenchView;
use idecraft_workbench::keymap_actions::{self, ReformatOption};

const MAIN: &str = "fn main() {\n    let x = 1;\n}\n\nfn unused() {\n    2;\n}\n";

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("idecraft-ninth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), MAIN).unwrap();
    std::fs::write(root.join("src/b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(root.join("notes.txt"), "one\ntwo\n").unwrap();
    root.canonicalize().unwrap()
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .expect("git")
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn commit_all(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "first"]);
}

fn open(
    root: &Path,
    file: &str,
    cx: &mut TestAppContext,
) -> (Entity<WorkbenchView>, &'static mut VisualTestContext) {
    let file = root.join(file);
    let root = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&root, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    open_file(&bench, &file, cx);
    (bench, cx)
}

fn open_file(bench: &Entity<WorkbenchView>, file: &Path, cx: &mut VisualTestContext) {
    let file = file.to_path_buf();
    bench.update_in(cx, |b, window, cx| {
        b.open(&file, window, cx);
        b.focus_editor(window, cx);
    });
    cx.run_until_parked();
}

fn act(bench: &Entity<WorkbenchView>, id: &str, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

fn text(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, cx| {
        b.active_editor_entity()
            .unwrap()
            .read(cx)
            .state
            .buffer
            .text()
    })
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

fn chooser(
    bench: &Entity<WorkbenchView>,
    cx: &mut VisualTestContext,
) -> Option<(String, Vec<String>)> {
    let popup = bench.read_with(cx, |b, _| b.compare_popup())?;
    Some(popup.read_with(cx, |v, _| (v.title().to_owned(), v.shown_items())))
}

fn active_name(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| {
        b.active_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default()
    })
}

fn at(path: &Path, line: u64, character: u64, len: u64) -> Location {
    Location {
        path: path.to_path_buf(),
        range: Range {
            start_line: line,
            start_character: character,
            end_line: line,
            end_character: character + len,
        },
    }
}

fn function(name: &str, line: u64, end: (u64, u64)) -> Symbol {
    Symbol {
        name: name.into(),
        kind: "function".into(),
        detail: None,
        line,
        character: 3,
        depth: 0,
        end_line: end.0,
        end_character: end.1,
        inherited_from: None,
        path: None,
    }
}

/// The plan of `unused` in `MAIN` at `file`, with `usages`.
fn unused_plan(file: &Path, text: &str, usages: Vec<Location>) -> SafeDeletePlan {
    plan_from(
        "unused",
        at(file, 4, 3, 6),
        text.to_owned(),
        &[function("main", 0, (2, 1)), function("unused", 4, (6, 1))],
        usages,
        UsageSource::LanguageServer,
    )
    .unwrap()
}

fn found(bench: &Entity<WorkbenchView>, plan: SafeDeletePlan, cx: &mut VisualTestContext) {
    bench.update_in(cx, |b, window, cx| b.safe_delete_found(plan, window, cx));
    cx.run_until_parked();
}

/// SPEC-0095 AK-21 (v12): without usages the function goes at once, as one undo step.
#[gpui::test]
fn safe_delete_without_usages_is_one_undo_step(cx: &mut TestAppContext) {
    let root = workspace("plain");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    let file = root.join("src/main.rs");
    found(&bench, unused_plan(&file, MAIN, Vec::new()), cx);
    assert_eq!(text(&bench, cx), "fn main() {\n    let x = 1;\n}\n");
    assert!(
        status(&bench, cx).starts_with("Deleted function `unused`;"),
        "{}",
        status(&bench, cx)
    );
    act(&bench, "$Undo", cx);
    assert_eq!(text(&bench, cx), MAIN, "one step brings it back");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0095 AK-21 (v12): with usages the list opens with Cancel selected; Enter keeps the
/// function, Delete anyway deletes it and names the usage left behind.
#[gpui::test]
fn safe_delete_with_usages_asks_first(cx: &mut TestAppContext) {
    let root = workspace("usages");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    let file = root.join("src/main.rs");
    let usage = vec![at(&root.join("src/b.rs"), 0, 3, 1)];
    found(&bench, unused_plan(&file, MAIN, usage.clone()), cx);
    assert_eq!(
        chooser(&bench, cx),
        Some((
            "Safe Delete unused: 1 usage".to_owned(),
            vec![
                "Cancel".to_owned(),
                "Delete anyway".to_owned(),
                "src/b.rs:1  fn b() {}".to_owned()
            ]
        ))
    );
    press("enter", cx);
    assert!(chooser(&bench, cx).is_none());
    assert_eq!(text(&bench, cx), MAIN, "Cancel deletes nothing");
    found(&bench, unused_plan(&file, MAIN, usage), cx);
    press("down", cx);
    press("enter", cx);
    assert_eq!(text(&bench, cx), "fn main() {\n    let x = 1;\n}\n");
    assert!(
        status(&bench, cx).ends_with("1 usage left behind"),
        "{}",
        status(&bench, cx)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0095 AK-21 (v12): a declaration in a file no editor has open is written on disk with
/// Local History before and after.
#[gpui::test]
fn safe_delete_in_a_closed_file_keeps_local_history(cx: &mut TestAppContext) {
    let root = workspace("closed");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    let b = root.join("src/b.rs");
    let plan = plan_from(
        "b",
        at(&b, 0, 3, 1),
        "fn b() {}\n".into(),
        &[function("b", 0, (0, 9))],
        Vec::new(),
        UsageSource::LanguageServer,
    )
    .unwrap();
    found(&bench, plan, cx);
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "");
    assert!(
        status(&bench, cx).contains("in src/b.rs; Local History has the old text"),
        "{}",
        status(&bench, cx)
    );
    let history = idecraft_core::LocalHistory::open(&root).unwrap();
    assert!(!history.revisions("src/b.rs").is_empty());
    assert_eq!(text(&bench, cx), MAIN, "the open file is untouched");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0095 AK-21 (v12): a file that changed since the search is left alone.
#[gpui::test]
fn safe_delete_leaves_a_changed_file_alone(cx: &mut TestAppContext) {
    let root = workspace("changed");
    let (bench, cx) = open(&root, "src/main.rs", cx);
    let file = root.join("src/main.rs");
    let plan = unused_plan(&file, MAIN, Vec::new());
    cx.simulate_input("// ");
    let typed = text(&bench, cx);
    assert_ne!(typed, MAIN);
    found(&bench, plan, cx);
    assert_eq!(text(&bench, cx), typed);
    assert_eq!(status(&bench, cx), keymap_actions::SAFE_DELETE_CHANGED);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0095 AK-22 (v12): ⌘⌦ in the tree asks the Delete question for a file nobody mentions
/// and lists the mentions of one that is named elsewhere; Delete anyway deletes it.
#[gpui::test]
fn safe_delete_in_the_project_tree(cx: &mut TestAppContext) {
    let root = workspace("tree");
    std::fs::write(root.join("src/lonely.txt"), "x\n").unwrap();
    std::fs::write(root.join("src/pricing.rs"), "pub fn total() {}\n").unwrap();
    std::fs::write(root.join("src/main.rs"), "mod pricing;\nfn main() {}\n").unwrap();
    let (bench, cx) = open(&root, "notes.txt", cx);
    let select = |name: &str, cx: &mut VisualTestContext| {
        let path = root.join("src").join(name);
        bench.update_in(cx, |b, window, cx| {
            b.reveal_in_tree(&path, cx);
            b.focus_tree(window, cx);
        });
        cx.run_until_parked();
    };
    select("lonely.txt", cx);
    press("cmd-delete", cx);
    until(cx, "the Delete question", |cx| {
        bench.read_with(cx, |b, _| b.delete_question()).is_some()
    });
    assert_eq!(
        bench.read_with(cx, |b, _| b.delete_question()).as_deref(),
        Some("Delete file \"lonely.txt\"?")
    );
    bench.update_in(cx, |b, window, cx| b.finish_delete(false, window, cx));
    cx.run_until_parked();
    select("pricing.rs", cx);
    press("cmd-delete", cx);
    until(cx, "the list of mentions", |cx| {
        chooser(&bench, cx).is_some()
    });
    let (title, rows) = chooser(&bench, cx).unwrap();
    assert_eq!(title, "Safe Delete pricing.rs: 1 mention");
    assert_eq!(rows[2], "src/main.rs:1  mod pricing;");
    assert!(root.join("src/pricing.rs").is_file(), "asked first");
    press("down", cx);
    press("enter", cx);
    assert!(!root.join("src/pricing.rs").exists());
    assert!(root.join("src/lonely.txt").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0075 AK-7 (v6): in a Java file under Git the options are the changed lines and the
/// imports; with both and no changed line the unused import goes as one undo step.
#[gpui::test]
fn reformat_file_dialog_optimizes_imports_in_one_step(cx: &mut TestAppContext) {
    let root = workspace("reformat");
    let java =
        "import java.util.List;\nimport java.util.Map;\n\nclass Shop {\n    List<String> l;\n}\n";
    std::fs::write(root.join("src/Shop.java"), java).unwrap();
    commit_all(&root);
    let (bench, cx) = open(&root, "src/Shop.java", cx);
    until(cx, "the change markers' base", |cx| {
        bench.read_with(cx, |b, cx| {
            b.active_editor_entity().unwrap().read(cx).has_base()
        })
    });
    press("cmd-alt-shift-l", cx);
    assert_eq!(
        chooser(&bench, cx),
        Some((
            "Reformat File: Shop.java".to_owned(),
            vec![
                "Only changed lines (Git)".to_owned(),
                "Optimize imports".to_owned()
            ]
        ))
    );
    press("space", cx);
    press("down", cx);
    press("space", cx);
    press("enter", cx);
    until(cx, "the imports", |cx| {
        status(&bench, cx).contains("import")
    });
    assert_eq!(
        status(&bench, cx),
        "No changed lines to format; Removed 1 import"
    );
    assert!(!text(&bench, cx).contains("Map"));
    act(&bench, "$Undo", cx);
    assert_eq!(text(&bench, cx), java, "one undo step");
    // The choice is kept for the next time.
    act(&bench, "ShowReformatFileDialog", cx);
    let popup = bench.read_with(cx, |b, _| b.compare_popup()).unwrap();
    assert_eq!(popup.read_with(cx, |v, _| v.checked()), [0, 1]);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0075 AK-7 (v6): a formatter of whole files refuses the selection.
#[gpui::test]
fn reformat_file_dialog_refuses_ranges_of_whole_file_formatters(cx: &mut TestAppContext) {
    let root = workspace("sql");
    std::fs::write(root.join("q.sql"), "select 1;\n").unwrap();
    let (bench, cx) = open(&root, "q.sql", cx);
    bench.update_in(cx, |b, window, cx| {
        let editor = b.active_editor_entity().unwrap();
        editor.update(cx, |e, cx| e.select_range(0..6, cx));
        b.reformat_file_with(&[ReformatOption::Selection], window, cx);
    });
    cx.run_until_parked();
    assert!(
        status(&bench, cx).contains("formats whole files only"),
        "{}",
        status(&bench, cx)
    );
    assert_eq!(text(&bench, cx), "select 1;\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0059 AK-18 (v9.8): a file rewritten with the same length and time stamp is taken by
/// Force Refresh, not by Reload All from Disk.
#[gpui::test]
fn force_refresh_reads_a_file_with_the_same_time_stamp(cx: &mut TestAppContext) {
    let root = workspace("stamp");
    let (bench, cx) = open(&root, "notes.txt", cx);
    let file = root.join("notes.txt");
    let stamp = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::fs::write(&file, "uno\ntwo\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    act(&bench, "Synchronize", cx);
    assert_eq!(text(&bench, cx), "one\ntwo\n", "the time stamp is trusted");
    act(&bench, "ForceRefresh", cx);
    assert_eq!(text(&bench, cx), "uno\ntwo\n");
    assert!(
        status(&bench, cx).starts_with("Force Refresh: 1 open file compared"),
        "{}",
        status(&bench, cx)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0059 AK-18 (v9.8): after a commit made outside IDECraft the change markers follow
/// Force Refresh.
#[gpui::test]
fn force_refresh_reads_head_again(cx: &mut TestAppContext) {
    let root = workspace("head");
    commit_all(&root);
    std::fs::write(root.join("notes.txt"), "one\ntwo\nthree\n").unwrap();
    let (bench, cx) = open(&root, "notes.txt", cx);
    let file = root.join("notes.txt");
    let markers = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, cx| {
            b.changes_of(&file, cx).unwrap_or_default().len()
        })
    };
    until(cx, "the markers", |cx| markers(cx) == 1);
    git(&root, &["commit", "-q", "-am", "second"]);
    act(&bench, "ForceRefresh", cx);
    until(cx, "no markers", |cx| markers(cx) == 0);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0071 AK-14 (v12): F10 opens the one header of a source file and lists the two sources
/// of a header.
#[gpui::test]
fn header_source_opens_or_lists(cx: &mut TestAppContext) {
    let root = workspace("header");
    std::fs::create_dir_all(root.join("include")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    for f in [
        "src/cart.cpp",
        "include/cart.h",
        "src/geo.h",
        "src/geo.c",
        "lib/geo.c",
    ] {
        std::fs::write(root.join(f), "\n").unwrap();
    }
    let (bench, cx) = open(&root, "src/cart.cpp", cx);
    press("f10", cx);
    until(cx, "the header", |cx| active_name(&bench, cx) == "cart.h");
    open_file(&bench, &root.join("src/geo.h"), cx);
    act(&bench, "SwitchHeaderSource", cx);
    until(cx, "the sources", |cx| {
        bench.read_with(cx, |b, _| b.usages_popup()).is_some()
    });
    let popup = bench.read_with(cx, |b, _| b.usages_popup()).unwrap();
    let (title, n) = popup.read_with(cx, |v, _| (v.title().to_owned(), v.usages().len()));
    assert_eq!(title, "Choose source for geo.h (2)");
    assert_eq!(n, 2);
    open_file(&bench, &root.join("notes.txt"), cx);
    act(&bench, "SwitchHeaderSource", cx);
    assert!(status(&bench, cx).starts_with("Header/Source applies"));
    let _ = std::fs::remove_dir_all(&root);
}
