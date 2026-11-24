//! `SPEC-0060` v14 behaviour 7, AK-20: on a Linux system without any font configuration the
//! toolkit finds no system font, and the bundled JetBrains Mono becomes the last resort for all
//! text instead of the window stopping with "failed to resolve font".
//!
//! The toolkit reads the font configuration once, when the application starts, so the check runs
//! in a child process of this test binary whose `FONTCONFIG_FILE` names an empty configuration.

#![cfg(target_os = "linux")]

use std::process::Command;

use ferris_idea_workbench::fonts;
use gpui::{Application, font};

/// Set in the child process that runs the check.
const CHILD: &str = "FERRIS_IDEA_FONT_LAST_RESORT_CHILD";

#[test]
fn bundled_font_is_the_last_resort_without_system_fonts() {
    if std::env::var_os(CHILD).is_some() {
        check_in_child();
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-font-last-resort-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("fonts.conf");
    std::fs::write(
        &config,
        "<?xml version=\"1.0\"?>\n<fontconfig></fontconfig>\n",
    )
    .unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "bundled_font_is_the_last_resort_without_system_fonts",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env("FONTCONFIG_FILE", &config)
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "child failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The test harness prints "test … ... " in front of the child's own output on the same line.
    if let Some(at) = stdout.find("skipped: ") {
        let reason = stdout[at..].lines().next().unwrap_or_default();
        eprintln!("font_last_resort {reason}");
        return;
    }
    assert!(
        stdout.contains("last resort: JetBrains Mono"),
        "the check did not run in the child:\n{stdout}"
    );
}

/// Without system fonts nothing resolves; once the bundled fonts are registered the system UI font
/// (plain and bold) resolves to JetBrains Mono, with no panic. Ends the process.
fn check_in_child() {
    Application::headless().run(|cx| {
        let text = cx.text_system().clone();
        let names = text.all_font_names();
        // The text system may read the system font folders itself, past `FONTCONFIG_FILE`. On a
        // host with fonts installed the premise cannot be made, so the check says so and ends
        // instead of testing something else.
        if let Some(family) = ["DejaVu Sans", "Noto Sans", "Ubuntu", "Liberation Sans"]
            .into_iter()
            .find(|family| names.iter().any(|n| n == family))
        {
            println!(
                "skipped: {family} is visible to the text system despite the empty configuration"
            );
            use std::io::Write;
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        assert!(fonts::register(cx), "the bundled fonts were not added");
        let plain = text.resolve_font(&font(".SystemUIFont"));
        let family = |id| text.get_font_for_id(id).map(|f| f.family.to_string());
        assert_eq!(family(plain).as_deref(), Some(fonts::EDITOR_FONT));
        let bold = text.resolve_font(&font(".SystemUIFont").bold());
        assert_eq!(family(bold).as_deref(), Some(fonts::EDITOR_FONT));
        assert_ne!(plain, bold, "the bold cut is used for bold text");
        println!("last resort: {}", fonts::EDITOR_FONT);
        // A quit before the event loop runs would be forgotten, and the loop waits for events
        // that never come: the child ends here.
        use std::io::Write;
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    });
}
