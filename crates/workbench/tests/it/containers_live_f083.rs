//! `SPEC-0119` v5 AK-12: the Containers window with real keys against the fake Docker Engine on a
//! real Unix socket — live updates from the event stream (D1 of OBS-0116), followed and searched
//! logs with earlier runs (D2), and container details with secrets masked (D5). The fake `kubectl`
//! stands in for Kubernetes so nothing of the user's kubeconfig is read.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ferris_idea_containers::fake::{FakeEngine, kubectl_script};
use ferris_idea_workbench::appearance::{CurrentIdeSettings, IdeSettingsFile, ThemeFile};
use ferris_idea_workbench::{ContainersView, WorkbenchView};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

type Bench = Entity<WorkbenchView>;

fn window(root: &Path, cx: &mut TestAppContext) -> (Bench, &'static mut VisualTestContext) {
    let settings = root.join(".test-settings");
    std::fs::create_dir_all(&settings).unwrap();
    cx.update(|cx| {
        cx.set_global(IdeSettingsFile(settings.join("settings.toml")));
        cx.set_global(ThemeFile(settings.join("theme")));
        cx.set_global(CurrentIdeSettings(ferris_idea_core::IdeSettings::default()));
    });
    let at = root.to_path_buf();
    let window = cx.add_window(move |_, cx| WorkbenchView::new(&at, cx));
    let bench = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.simulate_resize(gpui::size(px(1372.), px(860.)));
    cx.run_until_parked();
    (bench, cx)
}

/// Waits up to `secs` seconds of real time until `done` holds; on a timeout the window's rows and
/// pane are shown.
fn until(
    cx: &mut VisualTestContext,
    view: &Entity<ContainersView>,
    secs: u64,
    what: &str,
    mut done: impl FnMut(&ContainersView) -> bool,
) -> Duration {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(secs) {
        cx.run_until_parked();
        if view.read_with(cx, |v, _| done(v)) {
            return started.elapsed();
        }
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    let (rows, pane, status) =
        view.read_with(cx, |v, _| (v.texts(), v.pane_texts(), v.pane_status()));
    panic!("timed out: {what}\nrows: {rows:#?}\npane: {pane:#?}\nstatus: {status:?}");
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
}

/// Moves the list's selection with ↑/↓ to the first row containing `part`.
fn go(cx: &mut VisualTestContext, view: &Entity<ContainersView>, part: &str) {
    cx.update(|window, cx| window.focus(&view.read(cx).focus()));
    cx.run_until_parked();
    for _ in 0..100 {
        let (texts, selected) = view.read_with(cx, |v, _| (v.texts(), v.selected_text()));
        if selected.as_deref().is_some_and(|s| s.contains(part)) {
            return;
        }
        let want = texts
            .iter()
            .position(|t| t.contains(part))
            .unwrap_or_else(|| panic!("no row with {part:?}: {texts:#?}"));
        let at = selected
            .and_then(|s| texts.iter().position(|t| *t == s))
            .unwrap_or(0);
        keys(cx, if want > at { "down" } else { "up" });
    }
    panic!("the selection does not reach {part:?}");
}

fn selected(cx: &mut VisualTestContext, view: &Entity<ContainersView>) -> String {
    view.read_with(cx, |v, _| v.selected_text())
        .unwrap_or_default()
}

#[gpui::test]
fn live_updates_logs_and_details_with_real_keys(cx: &mut TestAppContext) {
    let dir = PathBuf::from(format!("/tmp/idc-wlive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project/.ferris-idea")).unwrap();
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = ferris_idea_containers::DockerHost::Unix(engine.socket.clone());
    let kubectl = ferris_idea_containers::Kubectl {
        program: kubectl_script(&dir).unwrap(),
    };
    let root = dir.join("project").canonicalize().unwrap();
    let (bench, cx) = window(&root, cx);
    bench.update(cx, |b, _| {
        b.set_container_sources(Some(host.clone()), Some(kubectl));
        b.set_container_engines(vec![ferris_idea_containers::Engine::from_host(
            "fake", host,
        )]);
        b.set_compose_sources(None, None);
    });
    cx.update(|window, cx| bench.update(cx, |b, cx| b.toggle_containers(window, cx)));
    let view = bench
        .read_with(cx, |b, _| b.containers_view())
        .expect("the Containers window is shown");
    until(cx, &view, 10, "the containers are listed", |v| {
        v.texts()
            .iter()
            .any(|t| t.starts_with("  ○ db postgres:18"))
    });
    until(cx, &view, 5, "the event stream is followed", |_| {
        engine.event_followers() == 1
    });

    // D1: changes made outside show without F5; the selection stays on its container.
    go(cx, &view, "○ db");
    engine.outside_create("cache", "redis:7");
    let took = until(cx, &view, 5, "a container created outside shows", |v| {
        v.texts().iter().any(|t| t.starts_with("  ● cache redis:7"))
    });
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert!(selected(cx, &view).contains("db postgres:18"));
    engine.outside_set("db", true);
    until(
        cx,
        &view,
        5,
        "a container started outside shows as running",
        |v| {
            v.texts()
                .iter()
                .any(|t| t.starts_with("  ● db postgres:18"))
        },
    );
    assert!(
        selected(cx, &view).contains("db postgres:18"),
        "the selection moved with its container"
    );
    engine.outside_remove("cache");
    until(cx, &view, 5, "a container removed outside goes", |v| {
        !v.texts().iter().any(|t| t.contains("cache redis"))
    });
    engine.go_away();
    until(cx, &view, 5, "the engine going away is shown", |v| {
        v.texts()
            .iter()
            .any(|t| t.contains("Live updates paused") && t.contains("connecting again in"))
    });
    engine.come_back().unwrap();
    until(
        cx,
        &view,
        15,
        "coming back clears the note and reads again",
        |v| {
            let t = v.texts();
            !t.iter().any(|l| l.contains("Live updates paused"))
                && t.iter()
                    .any(|l| l.trim_start().starts_with("● web nginx:1.27"))
        },
    );

    // D2: Enter follows the output; ↑ pauses, End follows again.
    go(cx, &view, "● web");
    keys(cx, "enter");
    until(cx, &view, 5, "Enter shows the logs", |v| {
        v.pane_texts() == ["GET / 200", "warn: slow"]
    });
    engine.log_line("web", 1, "GET /live 200");
    until(cx, &view, 5, "a new line arrives while following", |v| {
        v.pane_texts().last().map(String::as_str) == Some("GET /live 200")
            && v.pane_status().as_deref() == Some("Following")
    });
    keys(cx, "tab");
    assert!(view.read_with(cx, |v, _| v.pane_has_keys()));
    keys(cx, "up");
    assert!(!view.read_with(cx, |v, _| v.pane_follows()), "↑ pauses");
    assert_eq!(
        view.read_with(cx, |v, _| v.pane_status()).as_deref(),
        Some("Paused (End follows)")
    );
    keys(cx, "end");
    assert!(view.read_with(cx, |v, _| v.pane_follows()), "End follows");
    engine.outside_restart("web");
    until(cx, &view, 5, "a restart folds the earlier run", |v| {
        let t = v.pane_texts();
        t.len() == 3
            && t[0].starts_with("▸ Run 1 (earlier)")
            && t[0].ends_with("3 lines ──")
            && t[1].starts_with("▾ Run 2 (current)")
            && t[2] == "restarted"
    });
    keys(cx, "p");
    assert_eq!(
        view.read_with(cx, |v, _| v.pane_texts()).len(),
        6,
        "p unfolds"
    );
    keys(cx, "/");
    synthetic_text(cx, "g");
    commit_text(cx, &view, "et");
    assert_eq!(
        view.read_with(cx, |v, _| v.pane_status()).as_deref(),
        Some("Following · Search: get — 2 matches")
    );
    keys(cx, "alt-c");
    assert!(
        view.read_with(cx, |v, _| v.pane_status())
            .unwrap()
            .contains("0 matches · Aa")
    );
    keys(cx, "alt-c alt-l");
    let only = view.read_with(cx, |v, _| v.pane_texts());
    assert!(only.contains(&"GET /live 200".to_owned()), "{only:#?}");
    assert!(!only.iter().any(|t| t == "warn: slow" || t == "restarted"));
    keys(cx, "escape escape");
    assert!(!view.read_with(cx, |v, _| v.pane_has_keys()));

    // D5: i shows the details, the password masked; o opens the port; j the JSON.
    go(cx, &view, "● web");
    keys(cx, "i");
    until(cx, &view, 5, "i shows the details", |v| {
        v.pane_texts()
            .iter()
            .any(|t| t == "  DB_PASSWORD=••••••  (masked)")
    });
    let shown = view.read_with(cx, |v, _| v.pane_texts()).join("\n");
    assert!(!shown.contains("fake-placeholder-pw"), "{shown}");
    assert!(shown.contains("  80/tcp → 0.0.0.0:8080  http://localhost:8080"));
    assert!(shown.contains("  volume web-data → /data (rw, z)"));
    keys(cx, "tab o");
    assert!(bench.read_with(cx, |b, _| {
        b.opened_urls().iter().any(|u| u == "http://localhost:8080")
    }));
    keys(cx, "j");
    let json = view.read_with(cx, |v, _| v.pane_texts()).join("\n");
    assert!(json.contains("\"Image\": \"nginx:1.27\""), "{json}");
    assert!(!json.contains("fake-placeholder-pw"));
    keys(cx, "escape");
    go(cx, &view, "db postgres");
    until(
        cx,
        &view,
        5,
        "moving to another container shows its details",
        |v| {
            v.pane_texts()
                .first()
                .is_some_and(|t| t.starts_with("db · postgres:18"))
        },
    );

    cx.update(|window, cx| bench.update(cx, |b, cx| b.toggle_containers(window, cx)));
    let mut on = false;
    let end = Instant::now() + Duration::from_secs(5);
    while engine.event_followers() > 0 && Instant::now() < end {
        // The engine notices a closed stream when it writes the next event.
        engine.outside_set("db", on);
        on = !on;
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        engine.event_followers(),
        0,
        "hiding the window ends the stream"
    );
    let _ = std::fs::remove_dir_all(dir);
}

fn status(bench: &Bench, cx: &mut VisualTestContext) -> String {
    bench
        .read_with(cx, |b, cx| b.status_message(cx))
        .unwrap_or_default()
}

/// Types `text` one key per character.
fn type_text(cx: &mut VisualTestContext, text: &str) {
    for ch in text.chars() {
        let key = if ch == ' ' {
            "space".to_owned()
        } else {
            ch.to_string()
        };
        keys(cx, &key);
    }
}

/// Text as synthetic input delivers it (computer use): every key event has the key code of `a`
/// and carries the real character only as its text (OBS-0116 D6).
fn synthetic_text(cx: &mut VisualTestContext, text: &str) {
    for ch in text.chars() {
        cx.simulate_keystrokes(&format!("a->{ch}"));
    }
    cx.run_until_parked();
}

/// A whole word committed at once, as dictation and input methods deliver it.
fn commit_text(cx: &mut VisualTestContext, view: &Entity<ContainersView>, text: &str) {
    view.update_in(cx, |v, window, cx| {
        gpui::EntityInputHandler::replace_text_in_range(v, None, text, window, cx)
    });
    cx.run_until_parked();
}

/// `SPEC-0119` v6 AK-15 and `SPEC-0242` v4 AK-7: Compose containers nest under their services
/// with the project network; a project of another folder is grouped by its labels, read-only but
/// with start and stop; images are pulled from a field, shown with the containers that use them
/// and removed with Delete twice, refused while used.
#[gpui::test]
fn images_and_compose_groups_with_real_keys(cx: &mut TestAppContext) {
    let dir = PathBuf::from(format!("/tmp/idc-wimg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project/.ferris-idea")).unwrap();
    let engine = FakeEngine::start(&dir.join("d.sock")).unwrap();
    let host = ferris_idea_containers::DockerHost::Unix(engine.socket.clone());
    let kubectl = ferris_idea_containers::Kubectl {
        program: kubectl_script(&dir).unwrap(),
    };
    let shop = dir.join("shop");
    std::fs::create_dir_all(&shop).unwrap();
    std::fs::write(
        shop.join("compose.yaml"),
        "services:\n  web:\n    image: nginx:1.27\n  worker:\n    image: busybox\n",
    )
    .unwrap();
    let root = dir.join("project").canonicalize().unwrap();
    let (bench, cx) = window(&root, cx);
    bench.update(cx, |b, _| {
        b.set_container_sources(Some(host.clone()), Some(kubectl));
        b.set_container_engines(vec![ferris_idea_containers::Engine::from_host(
            "fake", host,
        )]);
        b.set_compose_sources(Some(shop.clone()), None);
    });
    cx.update(|window, cx| bench.update(cx, |b, cx| b.toggle_containers(window, cx)));
    let view = bench
        .read_with(cx, |b, _| b.containers_view())
        .expect("the Containers window is shown");
    until(
        cx,
        &view,
        10,
        "the folder's project nests its containers",
        |v| {
            let t = v.texts();
            let at = |s: &str| t.iter().position(|l| l.starts_with(s));
            matches!(
                (at("Compose: shop"), at("  ◆ web running"), at("    ● web nginx:1.27"), at("  ◆ worker not created"), at("  network shop_default")),
                (Some(a), Some(b), Some(c), Some(d), Some(e)) if a < b && b < c && c < d && d < e
            ) && t.iter().any(|l| l == "Containers (1)")
                && t.iter().filter(|l| l.contains("● web nginx")).count() == 1
        },
    );

    // A project of another folder, grouped by its labels.
    engine.outside_compose(
        "billing",
        "api",
        "billing-api-1",
        "nginx:1.27",
        "/srv/billing",
    );
    until(cx, &view, 5, "the other folder's project is grouped", |v| {
        let t = v.texts();
        t.iter()
            .any(|l| l == "Compose: billing (/srv/billing, read-only)")
            && t.iter().any(|l| l == "  ◆ api running")
            && t.iter()
                .any(|l| l.starts_with("    ● billing-api-1 nginx:1.27"))
            && t.iter().any(|l| l == "  network billing_default")
            && t.iter().any(|l| l == "Containers (1)")
    });
    go(cx, &view, "Compose: billing");
    keys(cx, "u");
    assert!(
        status(&bench, cx).contains("only for the project folder"),
        "{}",
        status(&bench, cx)
    );
    go(cx, &view, "● billing-api-1");
    keys(cx, "x");
    until(
        cx,
        &view,
        5,
        "x stops a container of the other project",
        |v| {
            let t = v.texts();
            t.iter().any(|l| l.starts_with("    ○ billing-api-1"))
                && t.iter().any(|l| l == "  ◆ api exited")
        },
    );

    // Pull from the field: Escape cancels, Enter pulls with progress and digest.
    go(cx, &view, "Images (");
    keys(cx, "p");
    type_text(cx, "busybox:latest");
    assert_eq!(
        view.read_with(cx, |v, _| v.pull_input().map(str::to_owned))
            .as_deref(),
        Some("busybox:latest")
    );
    keys(cx, "escape");
    assert_eq!(
        view.read_with(cx, |v, _| v.pull_input().map(str::to_owned)),
        None
    );
    keys(cx, "p");
    // D6: synthetic keys and a committed word land as the text they mean.
    synthetic_text(cx, "busybox:1.36");
    commit_text(cx, &view, ".1");
    assert_eq!(
        view.read_with(cx, |v, _| v.pull_input().map(str::to_owned))
            .as_deref(),
        Some("busybox:1.36.1")
    );
    keys(cx, "enter");
    until(
        cx,
        &view,
        5,
        "the pull shows progress and the digest",
        |v| {
            let p = v.pane_texts();
            p.iter().any(|l| l == "a1b2c3d4e5f6: Pull complete")
                && p.iter()
                    .any(|l| l.starts_with("Pulled busybox:1.36.1 (sha256:1111"))
                && !p.iter().any(|l| l.contains("Downloading"))
                && v.texts().iter().any(|l| l.starts_with("  busybox:1.36.1 "))
        },
    );
    assert!(status(&bench, cx).starts_with("Pulled busybox:1.36.1 sha256:"));

    // Details with the containers that use the image.
    go(cx, &view, "  nginx:1.27 ");
    keys(cx, "enter");
    until(cx, &view, 5, "Enter shows the image's details", |v| {
        let p = v.pane_texts();
        p.iter().any(|l| l == "  Platform linux/arm64/v8")
            && p.iter().any(|l| l == "Used by (2)")
            && p.iter().any(|l| l == "  web (running)")
            && p.iter().any(|l| l == "  billing-api-1 (exited)")
    });
    keys(cx, "delete");
    assert_eq!(
        view.read_with(cx, |v, _| v.removing_image().map(str::to_owned))
            .as_deref(),
        Some("nginx:1.27")
    );
    keys(cx, "delete");
    let refused = Instant::now();
    while !status(&bench, cx).starts_with("Not removed:")
        && refused.elapsed() < Duration::from_secs(5)
    {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        status(&bench, cx),
        "Not removed: nginx:1.27 is used by web (running), billing-api-1 (exited): remove them first"
    );
    go(cx, &view, "  busybox:1.36.1 ");
    keys(cx, "delete delete");
    until(cx, &view, 5, "Delete twice removes an unused image", |v| {
        !v.texts().iter().any(|l| l.starts_with("  busybox:1.36.1 "))
    });
    assert_eq!(status(&bench, cx), "Image busybox:1.36.1 removed");
    cx.update(|window, cx| bench.update(cx, |b, cx| b.toggle_containers(window, cx)));
    let _ = std::fs::remove_dir_all(dir);
}
