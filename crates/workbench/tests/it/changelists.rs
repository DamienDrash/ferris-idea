//! Changelists in a window of the test harness (`SPEC-0335`): the Commit tool window groups the
//! changed files by changelist, the actions New, Move, Edit, Set Active and Delete work through
//! their strip, a commit takes the committed file out of its list, and a state file that cannot
//! be used is said. Needs the `git` program to make the repository.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use ferris_idea_core::changelists::{CHANGELISTS_FILE, Changelists};
use ferris_idea_workbench::changelist_model::{
    LAST_LIST_STAYS, ListStrip, NO_LIST_TO_DELETE, NOTHING_TO_MOVE,
};
use ferris_idea_workbench::commit_model::RowKind;
use ferris_idea_workbench::keymap_actions::{
    DELETE_CHANGELIST, EDIT_CHANGELIST, MOVE_TO_CHANGELIST, NEW_CHANGELIST, SET_ACTIVE_CHANGELIST,
};
use ferris_idea_workbench::{CommitView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Forty lines, so a renamed file with one changed line is still the same file for Git.
fn body(first: &str) -> String {
    let mut text = format!("{first}\n");
    for i in 0..40 {
        text.push_str(&format!("line {i}\n"));
    }
    text
}

/// A repository with one commit of `a.txt`, `b.txt` and `c.txt`, of which `a.txt` and `b.txt`
/// are changed; `None` without `git`.
fn workspace(tag: &str) -> Option<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "ferris-idea-changelists-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(name), body(name)).unwrap();
    }
    if !git(&root, &["init", "-q", "-b", "main"]) {
        eprintln!("skipped: no git");
        return None;
    }
    for args in [
        &["config", "user.name", "T"][..],
        &["config", "user.email", "t@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        &["add", "."],
        &["commit", "-q", "-m", "base"],
    ] {
        assert!(git(&root, args), "git {args:?}");
    }
    for name in ["a.txt", "b.txt"] {
        std::fs::write(root.join(name), body("changed")).unwrap();
    }
    Some(root.canonicalize().unwrap())
}

fn open(
    root: &Path,
    cx: &mut TestAppContext,
) -> (
    Entity<WorkbenchView>,
    Entity<CommitView>,
    &'static mut VisualTestContext,
) {
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    bench.update_in(cx, |b, window, cx| {
        b.menu_action("ActivateCommitToolWindow", window, cx)
    });
    cx.run_until_parked();
    let commit = bench
        .read_with(cx, |b, _| b.commit_view())
        .expect("the Commit tool window is open");
    (bench, commit, cx)
}

fn action(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext, id: &str) {
    bench.update_in(cx, |b, window, cx| b.menu_action(id, window, cx));
    cx.run_until_parked();
}

fn status(bench: &Entity<WorkbenchView>, cx: &mut VisualTestContext) -> String {
    bench.read_with(cx, |b, _| b.status_note().unwrap_or_default().to_owned())
}

/// The lists as the window has them: name, whether active, files.
fn lists(
    view: &Entity<CommitView>,
    cx: &mut VisualTestContext,
) -> Vec<(String, bool, Vec<String>)> {
    view.read_with(cx, |v, _| {
        v.lists()
            .lists()
            .iter()
            .map(|l| {
                (
                    l.name.clone(),
                    l.name == v.lists().active(),
                    l.files.iter().cloned().collect(),
                )
            })
            .collect()
    })
}

fn list(name: &str, active: bool, files: &[&str]) -> (String, bool, Vec<String>) {
    (
        name.to_owned(),
        active,
        files.iter().map(|f| (*f).to_owned()).collect(),
    )
}

/// The rows of the changelists as indented labels, the active list marked with `*`. The group
/// of the unversioned files below them is left out: Ferris IDEA's own files appear there.
fn shown(view: &Entity<CommitView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.rows()
            .iter()
            .take_while(|r| r.kind != RowKind::Group { unversioned: true })
            .map(|r| {
                let mark = match r.kind {
                    RowKind::Changelist { active: true } => "*",
                    _ => "",
                };
                format!("{}{}{mark}", "  ".repeat(r.depth), r.label)
            })
            .collect()
    })
}

/// Types `name` into the open strip and presses Enter.
fn confirm(view: &Entity<CommitView>, cx: &mut VisualTestContext, name: &str) {
    view.update(cx, |v, cx| v.set_list_name(name, cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

/// SPEC-0335 AK-8: the five actions from the window, with their strip, on real files.
#[gpui::test]
fn lists_are_made_filled_renamed_and_deleted_from_the_window(cx: &mut TestAppContext) {
    let Some(root) = workspace("actions") else {
        return;
    };
    let (bench, view, cx) = open(&root, cx);

    // A project starts with the active list Changes holding the changed files, and no file.
    assert_eq!(
        lists(&view, cx),
        [list("Changes", true, &["a.txt", "b.txt"])]
    );
    assert_eq!(shown(&view, cx), ["Changes*", "  a.txt", "  b.txt"]);
    assert!(!root.join(CHANGELISTS_FILE).exists());

    // New Changelist…: the strip offers a free name; Enter creates the list.
    action(&bench, cx, NEW_CHANGELIST);
    assert_eq!(
        view.read_with(cx, |v, _| v.list_strip().cloned()),
        Some(ListStrip::New { set_active: false })
    );
    assert_eq!(
        view.read_with(cx, |v, cx| v.list_name(cx)),
        "New changelist"
    );
    confirm(&view, cx, "  Cart ");
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);
    assert_eq!(
        lists(&view, cx),
        [
            list("Cart", false, &[]),
            list("Changes", true, &["a.txt", "b.txt"])
        ]
    );
    assert_eq!(status(&bench, cx), "Changelist 'Cart' created");
    // An empty list is shown; the state file is written and Git is told to ignore it.
    assert_eq!(shown(&view, cx), ["Cart", "Changes*", "  a.txt", "  b.txt"]);
    let (kept, problem) = Changelists::load(&root);
    assert_eq!(problem, None);
    assert!(kept.get("Cart").is_some());
    let ignore = std::fs::read_to_string(root.join(".ferris-idea/.gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == "changelists.toml"));

    // A taken name keeps the strip open and says why; Escape closes it without a change.
    action(&bench, cx, NEW_CHANGELIST);
    confirm(&view, cx, "Cart");
    assert!(view.read_with(cx, |v, _| v.list_strip().is_some()));
    assert_eq!(
        view.read_with(cx, |v, _| v.list_strip_error().map(str::to_owned)),
        Some("A changelist named 'Cart' already exists".to_owned())
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);
    assert_eq!(lists(&view, cx).len(), 2);

    // Move to Another Changelist…: the selected file, the other list preselected.
    view.update(cx, |v, cx| v.click("a.txt", false, cx));
    action(&bench, cx, MOVE_TO_CHANGELIST);
    assert_eq!(
        view.read_with(cx, |v, _| v.list_strip().cloned()),
        Some(ListStrip::Move {
            files: vec!["a.txt".into()],
            offered: vec!["Cart".into()],
            set_active: false
        })
    );
    assert_eq!(view.read_with(cx, |v, cx| v.list_name(cx)), "Cart");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(status(&bench, cx), "1 file moved to 'Cart'");
    assert_eq!(shown(&view, cx), ["Cart", "  a.txt", "Changes*", "  b.txt"]);
    // A name no list has makes the list.
    view.update(cx, |v, cx| v.click("b.txt", false, cx));
    action(&bench, cx, MOVE_TO_CHANGELIST);
    confirm(&view, cx, "Api");
    assert_eq!(
        status(&bench, cx),
        "1 file moved to the new changelist 'Api'"
    );
    assert_eq!(
        lists(&view, cx),
        [
            list("Api", false, &["b.txt"]),
            list("Cart", false, &["a.txt"]),
            list("Changes", true, &[])
        ]
    );
    assert_eq!(
        shown(&view, cx),
        ["Api", "  b.txt", "Cart", "  a.txt", "Changes*"]
    );

    // Edit Changelist… on the selected list: the files stay, the selection follows the name.
    view.update(cx, |v, cx| v.select_list("Api", cx));
    assert_eq!(view.read_with(cx, |v, _| v.current()), None);
    action(&bench, cx, EDIT_CHANGELIST);
    assert_eq!(
        view.read_with(cx, |v, _| v.list_strip().cloned()),
        Some(ListStrip::Edit {
            list: "Api".into(),
            set_active: false,
            fixed: false
        })
    );
    assert_eq!(view.read_with(cx, |v, cx| v.list_name(cx)), "Api");
    confirm(&view, cx, "Checkout");
    assert_eq!(status(&bench, cx), "Changelist 'Api' renamed to 'Checkout'");
    assert_eq!(
        view.read_with(cx, |v, _| v.selected_list().map(str::to_owned)),
        Some("Checkout".to_owned())
    );
    assert_eq!(
        lists(&view, cx)[2],
        list("Checkout", false, &["b.txt"]),
        "{:?}",
        lists(&view, cx)
    );

    // Set Active Changelist: a change seen from now on joins that list.
    action(&bench, cx, SET_ACTIVE_CHANGELIST);
    assert_eq!(
        status(&bench, cx),
        "'Checkout' is the active changelist now"
    );
    action(&bench, cx, SET_ACTIVE_CHANGELIST);
    assert_eq!(status(&bench, cx), "'Checkout' is the active changelist");
    std::fs::write(root.join("c.txt"), body("changed")).unwrap();
    view.update(cx, |v, cx| {
        v.refresh();
        cx.notify();
    });
    assert_eq!(
        shown(&view, cx),
        [
            "Cart",
            "  a.txt",
            "Changes",
            "Checkout*",
            "  b.txt",
            "  c.txt"
        ]
    );
    // What the window shows is what the file holds.
    let (kept, _) = Changelists::load(&root);
    assert_eq!(kept.active(), "Checkout");
    assert_eq!(kept.list_of("c.txt"), Some("Checkout"));
    assert_eq!(kept.list_of("a.txt"), Some("Cart"));

    // The checkbox of a list checks and unchecks its files.
    view.update(cx, |v, cx| v.toggle_list("Checkout", cx));
    assert_eq!(view.read_with(cx, |v, _| v.selected()), ["a.txt"]);
    view.update(cx, |v, cx| v.toggle_list("Checkout", cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.selected()),
        ["a.txt", "b.txt", "c.txt"]
    );

    // Delete Changelist needs a selected list.
    view.update(cx, |v, cx| v.click("a.txt", false, cx));
    action(&bench, cx, DELETE_CHANGELIST);
    assert_eq!(status(&bench, cx), NO_LIST_TO_DELETE);
    // A list that is not active and has files asks; its files join the active list.
    view.update(cx, |v, cx| v.select_list("Cart", cx));
    action(&bench, cx, DELETE_CHANGELIST);
    assert_eq!(
        view.read_with(cx, |v, _| v.list_strip().cloned()),
        Some(ListStrip::Delete {
            list: "Cart".into(),
            files: 1,
            choose: vec![],
            selected: 0
        })
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        status(&bench, cx),
        "Changelist 'Cart' deleted; 1 file moved to 'Checkout'"
    );
    assert_eq!(
        lists(&view, cx),
        [
            list("Changes", false, &[]),
            list("Checkout", true, &["a.txt", "b.txt", "c.txt"])
        ]
    );
    // The active list asks which list becomes active; its files go there.
    view.update(cx, |v, cx| v.select_list("Checkout", cx));
    action(&bench, cx, DELETE_CHANGELIST);
    assert_eq!(
        view.read_with(cx, |v, _| v.list_strip().map(|s| s.choices().to_vec())),
        Some(vec!["Changes".to_owned()])
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        lists(&view, cx),
        [list("Changes", true, &["a.txt", "b.txt", "c.txt"])]
    );
    assert_eq!(
        shown(&view, cx),
        ["Changes*", "  a.txt", "  b.txt", "  c.txt"]
    );
    // The last list stays.
    view.update(cx, |v, cx| v.select_list("Changes", cx));
    action(&bench, cx, DELETE_CHANGELIST);
    assert_eq!(status(&bench, cx), LAST_LIST_STAYS);
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);
    assert_eq!(lists(&view, cx).len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0335 AK-8 (behaviours 3, 4 and 15): a commit takes the committed file out of its list
/// and leaves the list; a file renamed in the index keeps its list.
#[gpui::test]
fn a_commit_takes_the_file_out_of_its_list(cx: &mut TestAppContext) {
    let Some(root) = workspace("commit") else {
        return;
    };
    let (bench, view, cx) = open(&root, cx);
    action(&bench, cx, NEW_CHANGELIST);
    confirm(&view, cx, "Cart");
    let move_to_cart = |file: &str, cx: &mut VisualTestContext| {
        view.update(cx, |v, cx| v.click(file, false, cx));
        action(&bench, cx, MOVE_TO_CHANGELIST);
        confirm(&view, cx, "Cart");
    };
    move_to_cart("a.txt", cx);
    assert_eq!(
        lists(&view, cx),
        [
            list("Cart", false, &["a.txt"]),
            list("Changes", true, &["b.txt"])
        ]
    );
    // Nothing to move without a file: an unversioned file is in no list.
    std::fs::write(root.join("new.txt"), "new\n").unwrap();
    view.update(cx, |v, cx| {
        v.refresh();
        v.click("new.txt", false, cx);
    });
    action(&bench, cx, MOVE_TO_CHANGELIST);
    assert_eq!(status(&bench, cx), NOTHING_TO_MOVE);
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);

    // Commit the one file of Cart: it leaves the list, the empty list stays and is shown.
    view.update(cx, |v, cx| {
        v.check_only(&["a.txt"], cx);
        v.set_message("Change a", cx);
        v.commit_now(cx);
    });
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |v, _| v
            .note()
            .is_some_and(|n| n.starts_with("Committed"))),
        "{:?}",
        view.read_with(cx, |v, _| v.note().map(str::to_owned))
    );
    assert_eq!(
        lists(&view, cx),
        [list("Cart", false, &[]), list("Changes", true, &["b.txt"])]
    );
    assert_eq!(shown(&view, cx), ["Cart", "Changes*", "  b.txt"]);
    // The unversioned file is in its own group, in no list.
    assert!(view.read_with(cx, |v, _| {
        v.rows().iter().any(|r| {
            r.key == "new.txt"
                && matches!(
                    r.kind,
                    RowKind::File {
                        unversioned: true,
                        ..
                    }
                )
        })
    }));
    let (kept, _) = Changelists::load(&root);
    assert_eq!(kept.list_of("a.txt"), None);
    assert_eq!(kept.list_of("b.txt"), Some("Changes"));
    assert!(kept.get("Cart").is_some_and(|l| l.files.is_empty()));

    // A rename in the index: the new path is in the list of the old one, not in the active list.
    move_to_cart("b.txt", cx);
    assert!(git(&root, &["mv", "b.txt", "renamed.txt"]));
    view.update(cx, |v, cx| {
        v.refresh();
        cx.notify();
    });
    assert_eq!(
        lists(&view, cx),
        [
            list("Cart", false, &["renamed.txt"]),
            list("Changes", true, &[])
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0335 AK-8 (behaviours 17 and 21): a state file that cannot be used is said without its
/// content; looking does not replace it, a change replaces a broken one, a newer one stays; a
/// change by another program is seen.
#[gpui::test]
fn a_broken_and_a_newer_state_file_are_said(cx: &mut TestAppContext) {
    let Some(root) = workspace("unusable") else {
        return;
    };
    std::fs::create_dir_all(root.join(".ferris-idea")).unwrap();
    let file = root.join(CHANGELISTS_FILE);
    let broken = "version = 1\n[[list]\nname = \"s3cr3t-token\"\n";
    std::fs::write(&file, broken).unwrap();
    let (bench, view, cx) = open(&root, cx);
    let said = view.read_with(cx, |v, _| v.note().unwrap_or_default().to_owned());
    assert!(said.contains(CHANGELISTS_FILE), "{said}");
    assert!(said.contains("were not read"), "{said}");
    assert!(!said.contains("s3cr3t"), "{said}");
    // The one list Changes is shown, and looking again does not replace the file.
    assert_eq!(shown(&view, cx), ["Changes*", "  a.txt", "  b.txt"]);
    view.update(cx, |v, cx| {
        v.refresh();
        cx.notify();
    });
    assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
    // A change replaces it with a good file.
    action(&bench, cx, NEW_CHANGELIST);
    confirm(&view, cx, "Cart");
    let (kept, problem) = Changelists::load(&root);
    assert_eq!(problem, None);
    assert!(kept.get("Cart").is_some());

    // Another program changes the file: the window sees that it changed and reads it.
    assert!(!view.read_with(cx, |v, _| v.changelists_file_changed()));
    let mut other = kept.clone();
    other.create("From outside", true).unwrap();
    other.save(&root).unwrap();
    assert!(view.read_with(cx, |v, _| v.changelists_file_changed()));
    view.update(cx, |v, cx| {
        v.refresh();
        cx.notify();
    });
    assert!(!view.read_with(cx, |v, _| v.changelists_file_changed()));
    assert_eq!(
        shown(&view, cx),
        ["Cart", "Changes", "  a.txt", "  b.txt", "From outside*"]
    );

    // A file of a newer version: the list Changes, the reason, and no change is taken.
    let newer = "version = 2\nshape = \"of the future\"\n";
    std::fs::write(&file, newer).unwrap();
    view.update(cx, |v, cx| {
        v.refresh();
        cx.notify();
    });
    let said = view.read_with(cx, |v, _| v.note().unwrap_or_default().to_owned());
    assert!(said.contains("version 2"), "{said}");
    assert!(said.contains("left as it is"), "{said}");
    assert_eq!(shown(&view, cx), ["Changes*", "  a.txt", "  b.txt"]);
    action(&bench, cx, NEW_CHANGELIST);
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);
    let said = status(&bench, cx);
    assert!(said.contains("cannot be changed"), "{said}");
    assert!(said.contains(CHANGELISTS_FILE), "{said}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
    let _ = std::fs::remove_dir_all(&root);
}

/// Types `text` and presses the keys `then`, every key through the window's key dispatch.
fn type_and_press(cx: &mut VisualTestContext, text: &str, then: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
    cx.simulate_keystrokes(then);
    cx.run_until_parked();
}

fn names(view: &Entity<CommitView>, cx: &mut VisualTestContext) -> Vec<String> {
    lists(view, cx).into_iter().map(|(name, ..)| name).collect()
}

/// SPEC-0335 AK-11 (v2): the name a strip opens with is selected, so typing replaces it and
/// End keeps it. Window check of 2026-11-14: typing `wip` made the list `wipNew changelist`.
#[gpui::test]
fn a_typed_name_replaces_the_name_a_strip_opens_with(cx: &mut TestAppContext) {
    let Some(root) = workspace("typed") else {
        return;
    };
    let (bench, view, cx) = open(&root, cx);

    // New Changelist…: the proposed name is replaced by what is typed.
    action(&bench, cx, NEW_CHANGELIST);
    assert_eq!(
        view.read_with(cx, |v, cx| v.list_name(cx)),
        "New changelist"
    );
    type_and_press(cx, "wip", "enter");
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);
    assert_eq!(names(&view, cx), ["Changes", "wip"]);

    // End drops the selection and keeps the proposal: the typed text goes behind it.
    action(&bench, cx, NEW_CHANGELIST);
    cx.simulate_keystrokes("end");
    type_and_press(cx, " two", "enter");
    assert_eq!(names(&view, cx), ["Changes", "New changelist two", "wip"]);

    // Edit Changelist…: the list's name is replaced.
    view.update(cx, |v, cx| v.select_list("wip", cx));
    action(&bench, cx, EDIT_CHANGELIST);
    assert_eq!(view.read_with(cx, |v, cx| v.list_name(cx)), "wip");
    type_and_press(cx, "work", "enter");
    assert_eq!(names(&view, cx), ["Changes", "New changelist two", "work"]);

    // Move to Another Changelist…: the preselected list's name is replaced, also after Down
    // put another list's name into the field; the typed name is a new list.
    view.update(cx, |v, cx| v.click("a.txt", false, cx));
    action(&bench, cx, MOVE_TO_CHANGELIST);
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, cx| v.list_name(cx)).is_empty());
    type_and_press(cx, "cart", "enter");
    assert_eq!(view.read_with(cx, |v, _| v.list_strip().cloned()), None);
    assert!(
        lists(&view, cx).contains(&list("cart", false, &["a.txt"])),
        "{:?}",
        lists(&view, cx)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0335 AK-11 (v2), the other inline name fields: a layout's name in Rename… is replaced
/// by what is typed, and Rename… of a file replaces the name before the extension (SPEC-0059).
#[gpui::test]
fn a_typed_name_replaces_the_name_in_the_other_name_fields(cx: &mut TestAppContext) {
    let Some(root) = workspace("other-fields") else {
        return;
    };
    let settings = std::env::temp_dir().join(format!(
        "ferris-idea-changelists-other-fields-settings-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&settings);
    std::fs::create_dir_all(&settings).unwrap();
    let file = settings.join("window-layouts.json");
    let (bench, _view, cx) = open(&root, cx);
    bench.update_in(cx, |b, _, cx| b.set_window_layouts_file(file.clone(), cx));
    let layout_names = |cx: &mut VisualTestContext| {
        bench.read_with(cx, |b, _| {
            b.window_layouts()
                .names()
                .iter()
                .map(|n| (*n).to_owned())
                .collect::<Vec<String>>()
        })
    };

    // Save Current Layout as New…: the field opens empty.
    action(&bench, cx, "StoreNewLayout");
    assert_eq!(
        bench
            .read_with(cx, |b, cx| b.name_field_text(cx))
            .as_deref(),
        Some("")
    );
    type_and_press(cx, "One", "enter");
    assert_eq!(layout_names(cx), ["One"]);

    // Rename… of the layout: its name is selected and replaced.
    action(&bench, cx, "FerrisIDEA.Layout.Rename:One");
    assert_eq!(
        bench
            .read_with(cx, |b, cx| b.name_field_text(cx))
            .as_deref(),
        Some("One")
    );
    type_and_press(cx, "Two", "enter");
    assert_eq!(layout_names(cx), ["Two"]);

    // Rename… of a file: the name before the extension is selected, the extension stays.
    bench.update_in(cx, |b, window, cx| {
        b.start_rename(root.join("c.txt"), window, cx)
    });
    cx.run_until_parked();
    type_and_press(cx, "d", "enter");
    assert!(root.join("d.txt").is_file(), "c.txt became d.txt");
    assert!(!root.join("c.txt").exists());
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}
