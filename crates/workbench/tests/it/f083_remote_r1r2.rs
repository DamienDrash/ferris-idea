//! Gaps R1 and R2 of OBS-0118 in a window of the test harness with real keys against the system
//! `sshd` started as the current user (skipped without it): a file changed on the host since it
//! was read is never overwritten by a save — the question, Cancel, Compare, Merge and Overwrite
//! (`SPEC-0121` v7 AK-10) — and the Remote Host list as a tree: folders first without regard to
//! case, → opens a folder in place, ← closes it, typing jumps to a name (`SPEC-0253` v3 AK-7).

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use gpui::{TestAppContext, VisualTestContext};

use super::f083_handtests::{
    Bench, go_remote, keys, ready_host, remote_texts, status, until_host, window, workspace,
};

/// Connects the window to the test host and shows `folder`.
fn show_folder(
    bench: &Bench,
    cx: &mut VisualTestContext,
    host: &ferris_idea_remote::testing::TestHost,
    folder: &Path,
) {
    bench.update_in(cx, |b, window, cx| {
        b.set_remote_ssh(host.target.extra.clone());
        b.toggle_remote(window, cx);
        b.remote_connect(host.target.clone(), cx);
    });
    until_host(bench, cx, "the home folder of the host is shown", |cx| {
        bench.read_with(cx, |b, cx| {
            b.remote_view()
                .is_some_and(|v| v.read(cx).place().is_some())
        })
    });
    let field = bench.read_with(cx, |b, cx| {
        b.remote_view().map(|v| v.read(cx).field_focus(cx)).unwrap()
    });
    cx.update(|window, _| window.focus(&field));
    keys(cx, "cmd-a");
    cx.simulate_input(&folder.display().to_string());
    keys(cx, "enter");
    until_host(bench, cx, "the test folder is shown", |cx| {
        place(bench, cx).is_some_and(|p| Path::new(&p) == folder)
    });
}

fn place(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| {
        b.remote_view()
            .and_then(|v| v.read(cx).place().map(|(_, p)| p.clone()))
    })
}

fn selected(bench: &Bench, cx: &mut VisualTestContext) -> Option<String> {
    bench.read_with(cx, |b, cx| {
        b.remote_view().and_then(|v| v.read(cx).selected_text())
    })
}

fn test_host(tag: &str) -> Option<(PathBuf, ferris_idea_remote::testing::TestHost)> {
    let base = std::env::temp_dir().join(format!("ferris-idea-r1r2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let host = ready_host(&base)?;
    Some((base, host))
}

/// SPEC-0253 v3 AK-7: order, open in place, close, type to jump.
#[gpui::test]
fn the_remote_list_is_a_tree_with_type_to_jump(cx: &mut TestAppContext) {
    let _turn = ferris_idea_remote::testing::one_host_at_a_time();
    let Some((base, host)) = test_host("tree") else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    cx.executor().allow_parking();
    let folder = base.canonicalize().unwrap().join("tree");
    for d in ["alpha", "Beta"] {
        std::fs::create_dir_all(folder.join(d)).unwrap();
    }
    for f in [
        "zeta.txt",
        "Alpha.txt",
        "beta.txt",
        "alpha/in.txt",
        "alpha/Inner.txt",
    ] {
        std::fs::write(folder.join(f), "x\n").unwrap();
    }
    let root = workspace("r2-tree");
    let (bench, cx) = window(&root, cx);
    show_folder(&bench, cx, &host, &folder);
    assert_eq!(
        remote_texts(&bench, cx),
        ["..", "alpha/", "Beta/", "Alpha.txt", "beta.txt", "zeta.txt"],
        "folders first, then files, without regard to case"
    );

    // → opens alpha/ in place; the folder shown stays.
    go_remote(&bench, cx, "alpha/");
    keys(cx, "right");
    until_host(&bench, cx, "alpha/ opens in place", |cx| {
        remote_texts(&bench, cx).contains(&"  in.txt".to_owned())
    });
    assert_eq!(
        remote_texts(&bench, cx),
        [
            "..",
            "alpha/",
            "  in.txt",
            "  Inner.txt",
            "Beta/",
            "Alpha.txt",
            "beta.txt",
            "zeta.txt"
        ]
    );
    assert_eq!(
        place(&bench, cx).as_deref().map(Path::new),
        Some(folder.as_path())
    );

    // Enter on a file inside the open folder opens that file.
    keys(cx, "down");
    assert_eq!(selected(&bench, cx).as_deref(), Some("  in.txt"));
    keys(cx, "enter");
    until_host(&bench, cx, "alpha/in.txt opens", |cx| {
        bench.read_with(cx, |b, _| {
            b.active_file().is_some_and(|p| p.ends_with("alpha/in.txt"))
        })
    });

    // ← on an entry inside selects its folder, ← again closes it.
    go_remote(&bench, cx, "Inner.txt");
    keys(cx, "left");
    assert_eq!(selected(&bench, cx).as_deref(), Some("alpha/"));
    keys(cx, "left");
    assert_eq!(
        remote_texts(&bench, cx),
        ["..", "alpha/", "Beta/", "Alpha.txt", "beta.txt", "zeta.txt"]
    );

    // Typing jumps to the first matching name; Escape forgets the typed letters, not the window.
    keys(cx, "z e t");
    assert_eq!(selected(&bench, cx).as_deref(), Some("zeta.txt"));
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.remote_view().is_some()));
    keys(cx, "b e");
    assert_eq!(selected(&bench, cx).as_deref(), Some("Beta/"));
    keys(cx, "escape");
    keys(cx, "q q q");
    assert_eq!(
        selected(&bench, cx).as_deref(),
        Some("Beta/"),
        "nothing matches, nothing moves"
    );
    drop(host);
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&root);
}

fn question(
    bench: &Bench,
    cx: &mut VisualTestContext,
) -> Option<ferris_idea_workbench::dialogs::DrawnQuestion> {
    bench.read_with(cx, |b, _| b.drawn_question().cloned())
}

/// Replaces the open editor's text and saves with ⌘S.
fn edit_and_save(bench: &Bench, cx: &mut VisualTestContext, text: Option<&str>) {
    let editor = bench
        .read_with(cx, |b, _| b.active_editor_entity())
        .expect("an editor is open");
    if let Some(text) = text {
        editor.update(cx, |e, cx| {
            let len = e.state.buffer.len_chars();
            e.replace_ranges(&[(0..len, text.to_owned())], cx);
        });
    }
    bench.update_in(cx, |b, window, cx| b.focus_editor(window, cx));
    cx.run_until_parked();
    keys(cx, "cmd-s");
}

fn wait_question(
    bench: &Bench,
    cx: &mut VisualTestContext,
) -> ferris_idea_workbench::dialogs::DrawnQuestion {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < end {
        cx.run_until_parked();
        if let Some(q) = question(bench, cx) {
            return q;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
    }
    let (file, saves, bound) = bench.read_with(cx, |b, cx| {
        let file = b.active_file();
        let bound = file.as_deref().and_then(|f| b.remote_file_of(f));
        (
            file,
            b.active_editor_entity().map(|e| e.read(cx).saves()),
            bound,
        )
    });
    panic!(
        "the save did not ask about the host's change: status {:?}, file {file:?}, saves {saves:?}, bound {bound:?}",
        status(bench, cx)
    );
}

fn answer(bench: &Bench, cx: &mut VisualTestContext, label: &str) {
    let q = question(bench, cx).expect("a question is shown");
    let i = q
        .buttons
        .iter()
        .position(|b| b == label)
        .unwrap_or_else(|| panic!("no answer {label:?}: {:?}", q.buttons));
    bench.update_in(cx, |b, window, cx| b.answer_drawn_question(i, window, cx));
    cx.run_until_parked();
}

/// SPEC-0121 v7 AK-10: a save after a host-side change asks and sends nothing; Cancel keeps both,
/// Compare shows both, Merge uploads a result with both changes, Overwrite uploads the copy.
#[gpui::test]
fn a_file_changed_on_the_host_is_merged_never_overwritten_by_a_save(cx: &mut TestAppContext) {
    let _turn = ferris_idea_remote::testing::one_host_at_a_time();
    let Some((base, host)) = test_host("conflict") else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    cx.executor().allow_parking();
    let folder = base.canonicalize().unwrap().join("conflict");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("notes.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let root = workspace("r1-conflict");
    let (bench, cx) = window(&root, cx);
    show_folder(&bench, cx, &host, &folder);
    go_remote(&bench, cx, "notes.txt");
    keys(cx, "enter");
    until_host(&bench, cx, "notes.txt opens", |cx| {
        bench.read_with(cx, |b, _| {
            b.active_file().is_some_and(|p| p.ends_with("notes.txt"))
        })
    });
    let host_text = || std::fs::read_to_string(&file).unwrap();

    // Someone changes the file on the host; the save asks and sends nothing.
    std::fs::write(&file, "one\ntwo\nthree\nhost\n").unwrap();
    edit_and_save(&bench, cx, Some("one\nmine\nthree\n"));
    let q = wait_question(&bench, cx);
    assert_eq!(
        q.message,
        "notes.txt changed on the host since you opened it"
    );
    assert!(
        q.detail
            .contains("Overwrite discards the host's change: 1 line."),
        "{}",
        q.detail
    );
    assert_eq!(
        q.buttons,
        ["Merge…", "Compare", "Overwrite Host Version", "Cancel"]
    );
    assert_eq!(
        host_text(),
        "one\ntwo\nthree\nhost\n",
        "nothing is sent before an answer"
    );

    // Escape cancels: both versions stay; saving again asks again (no overwrite on the 2nd save).
    keys(cx, "escape");
    assert!(question(&bench, cx).is_none());
    assert!(
        status(&bench, cx).contains("Saving again asks again"),
        "{}",
        status(&bench, cx)
    );
    edit_and_save(&bench, cx, None);
    wait_question(&bench, cx);
    assert_eq!(host_text(), "one\ntwo\nthree\nhost\n");

    // Compare: the diff of the copy and the host's version; nothing is sent.
    answer(&bench, cx, "Compare");
    let titles = bench.read_with(cx, |b, cx| {
        b.diff_view().map(|d| {
            let d = d.read(cx);
            let (l, r) = d.side_titles();
            (l.to_owned(), r.to_owned(), d.differences())
        })
    });
    let (left, right, differences) = titles.expect("the diff viewer is shown");
    assert_eq!(
        (left.as_str(), right.as_str()),
        ("Your copy", "On the host")
    );
    assert!(differences > 0);
    assert_eq!(host_text(), "one\ntwo\nthree\nhost\n");

    // Escape closes the diff viewer.
    keys(cx, "escape");
    assert!(bench.read_with(cx, |b, _| b.diff_view().is_none()));

    // Merge: both changes in the result, uploaded on Apply.
    edit_and_save(&bench, cx, None);
    wait_question(&bench, cx);
    answer(&bench, cx, "Merge…");
    let merge = bench
        .read_with(cx, |b, _| b.merge_view())
        .expect("the merge view is shown");
    assert_eq!(merge.read_with(cx, |m, _| m.open_conflicts()), 0);
    assert_eq!(
        host_text(),
        "one\ntwo\nthree\nhost\n",
        "nothing is sent before Apply"
    );
    merge.update(cx, |m, cx| assert!(m.apply(cx)));
    until_host(&bench, cx, "the merged result is uploaded", |_| {
        host_text() == "one\nmine\nthree\nhost\n"
    });
    until_host(&bench, cx, "the upload is reported", |cx| {
        status(&bench, cx).starts_with("Uploaded to")
    });
    assert!(bench.read_with(cx, |b, _| b.merge_view().is_none()));

    // Overwrite: only on its own button, and it names what goes.
    std::fs::write(&file, "host only\n").unwrap();
    edit_and_save(&bench, cx, Some("mine only\n"));
    let q = wait_question(&bench, cx);
    assert!(
        q.detail.contains("Overwrite discards the host's change"),
        "{}",
        q.detail
    );
    assert_eq!(host_text(), "host only\n");
    answer(&bench, cx, "Overwrite Host Version");
    until_host(
        &bench,
        cx,
        "the copy is uploaded over the host's version",
        |_| host_text() == "mine only\n",
    );
    drop(host);
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&root);
}

/// SPEC-0121 v7 AK-11 (OBS-0118 R4): a host added to the SSH configuration appears in the open
/// window without a restart, and the selected host stays selected.
#[gpui::test]
fn the_host_list_follows_its_file_and_keeps_the_selection(cx: &mut TestAppContext) {
    let _turn = ferris_idea_remote::testing::one_host_at_a_time();
    let Some((base, host)) = test_host("hosts") else {
        eprintln!("skipped: no sshd or ssh-keygen");
        return;
    };
    cx.executor().allow_parking();
    let root = workspace("r4-hosts");
    let (bench, cx) = window(&root, cx);
    bench.update_in(cx, |b, window, cx| {
        b.set_remote_ssh(host.target.extra.clone());
        b.toggle_remote(window, cx);
    });
    until_host(&bench, cx, "the test host is listed", |cx| {
        remote_texts(&bench, cx)
            .iter()
            .any(|t| t.starts_with(&host.target.host))
    });
    go_remote(&bench, cx, &host.target.host);
    let config = PathBuf::from(&host.target.extra[1]);
    let mut text = std::fs::read_to_string(&config).unwrap();
    // A host sorted before the selected one, so its row number changes.
    text.insert_str(0, "Host aaa-added\n  HostName 127.0.0.1\n  Port 1\n\n");
    std::fs::write(&config, text).unwrap();
    until_host(
        &bench,
        cx,
        "the added host appears without a restart",
        |cx| {
            remote_texts(&bench, cx)
                .iter()
                .any(|t| t.starts_with("aaa-added"))
        },
    );
    assert!(
        selected(&bench, cx).is_some_and(|s| s.starts_with(&host.target.host)),
        "the selection stays on the host: {:?}",
        selected(&bench, cx)
    );
    drop(host);
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&root);
}
