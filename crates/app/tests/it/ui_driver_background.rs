//! `SPEC-0319` v7 AK-15: the real `ferris-idea` window, started in the background
//! (`FERRIS_IDEA_UI_BACKGROUND=1`: not activated, minimized) with a settings folder of the test's own,
//! driven only through `ferris-idea drive` — a file opened, Problems and Containers shown, the ⌥↩
//! list opened and read, and a screenshot drawn by the window itself while it is minimized.
//!
//! It opens a real window, so it runs on a Mac with a logged-in, unlocked session only:
//! `cargo test -p ferris-idea --test it ui_driver_background -- --ignored`. The user's settings,
//! grants and keys are never read or written.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Ide {
    child: Child,
    base: PathBuf,
}

impl Drop for Ide {
    fn drop(&mut self) {
        // Only the window this test started, by its own PID.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn drive(settings: &Path, root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferris-idea"))
        .env("FERRIS_IDEA_CONFIG_DIR", settings)
        .arg("drive")
        .arg("--project")
        .arg(root)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}

fn ok(settings: &Path, root: &Path, args: &[&str]) -> Value {
    let out = drive(settings, root, args);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "drive {args:?}: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

#[test]
#[ignore = "opens a real window; run on the Mac with --ignored"]
fn ak15_the_background_window_is_driven_and_photographed_without_the_screen() {
    let base = std::env::temp_dir().join(format!("ferris-idea-uibg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("uibg");
    let settings = base.join("settings");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(&settings).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"uibg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    let settings = settings.canonicalize().unwrap();
    std::fs::write(
        settings.join("settings.toml"),
        "[mcp]\nhttp = true\n\n[ui.driver]\nexternal = true\n",
    )
    .unwrap();
    std::fs::write(
        settings.join("agent-permissions.toml"),
        format!(
            "[[workspace]]\npath = \"{}\"\nallow = [\"ui\"]\n",
            root.display()
        ),
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_ferris-idea"))
        .env("FERRIS_IDEA_CONFIG_DIR", &settings)
        .env("FERRIS_IDEA_UI_BACKGROUND", "1")
        .arg("ui")
        .arg(&root)
        .arg("--trust")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _ide = Ide {
        child,
        base: base.clone(),
    };

    // The window serves MCP once it is up.
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        if drive(&settings, &root, &["status"]).status.success() {
            break;
        }
        assert!(Instant::now() < end, "the window did not come up");
        std::thread::sleep(Duration::from_millis(500));
    }

    let r = ok(
        &settings,
        &root,
        &["open", "src/main.rs", "--line", "1", "--column", "4"],
    );
    assert!(
        r["active"]
            .as_str()
            .is_some_and(|a| a.ends_with("src/main.rs")),
        "{r}"
    );
    let r = ok(&settings, &root, &["tool-window", "problems"]);
    assert_eq!(r["shown"], Value::Bool(true), "{r}");
    // The tree of the minimized window answers, and a click goes in through the window's own
    // input handler (SPEC-0319 v7 behaviours 55a and 16b).
    let r = ok(&settings, &root, &["find", "id=problems-project", "--wait"]);
    assert_eq!(r["count"], Value::from(1), "{r}");
    ok(&settings, &root, &["click", "id=problems-project"]);
    ok(
        &settings,
        &root,
        &["expect", "id=problems-project", "selected"],
    );
    let r = ok(&settings, &root, &["snapshot", "--text"]);
    assert!(
        r.as_str().is_some_and(|t| t.contains("problems-file"))
            || r.to_string().contains("problems-file"),
        "{r}"
    );
    let r = ok(&settings, &root, &["tool-window", "containers"]);
    assert_eq!(r["shown"], Value::Bool(true), "{r}");
    ok(
        &settings,
        &root,
        &["open", "src/main.rs", "--line", "1", "--column", "4"],
    );
    ok(&settings, &root, &["invoke", "ShowIntentionActions"]);
    let mut popup = Value::Null;
    for _ in 0..40 {
        popup = ok(&settings, &root, &["popup"]);
        if popup["open"] == Value::Bool(true) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(popup["kind"], "intentions", "{popup}");
    assert!(
        popup["items"].as_array().is_some_and(|i| !i.is_empty()),
        "{popup}"
    );

    let shot = base.join("window.png");
    let r = ok(&settings, &root, &["screenshot", shot.to_str().unwrap()]);
    let image = image::open(&shot).unwrap().to_rgba8();
    assert_eq!(
        (Value::from(image.width()), Value::from(image.height())),
        (r["width"].clone(), r["height"].clone())
    );
    assert!(image.width() > 400 && image.height() > 300, "{r}");
    let first = *image.get_pixel(0, 0);
    assert!(
        image.pixels().any(|p| *p != first),
        "the screenshot shows the window, not one colour"
    );
    let part = base.join("part.png");
    let r = ok(
        &settings,
        &root,
        &[
            "screenshot",
            part.to_str().unwrap(),
            "--region",
            "0,0,200,100",
        ],
    );
    let scale = r["scale"].as_f64().unwrap();
    let part = image::open(&part).unwrap();
    assert_eq!(part.width(), (200.0 * scale).round() as u32);

    let log = std::fs::read_to_string(root.join(".ferris-idea/agent-log.jsonl")).unwrap();
    assert!(log.contains("\"tool\":\"ui_screenshot\""), "{log}");
    assert!(log.contains("open_file src/main.rs:1:4"), "{log}");
}
