//! Workspace scans: no fixed sleeps in the driver (`SPEC-0319` AK-3) and no system-wide synthetic
//! input anywhere (UISEC-9).

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if p.is_dir() {
            if !matches!(name, "target" | ".git" | "node_modules" | "fuzz") {
                rust_files(&p, out);
            }
        } else if p
            .extension()
            .is_some_and(|x| x == "rs" || x == "swift" || x == "c")
        {
            out.push(p);
        }
    }
}

/// AK-3: the driver crate and the window's driver host wait on frames, never on a timer.
#[test]
fn ak3_no_fixed_sleep_in_the_driver() {
    let root = workspace();
    let mut files = Vec::new();
    rust_files(&root.join("crates/uidriver/src"), &mut files);
    for f in [
        "crates/app/src/ui_driver.rs",
        "crates/workbench/src/semantics.rs",
    ] {
        files.push(root.join(f));
    }
    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for (i, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if code.contains("thread::sleep") || code.contains(".timer(") {
                found.push(format!("{}:{}: {}", f.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "fixed sleeps or timers in the driver:\n{}",
        found.join("\n")
    );
}

/// `Step::Act` stand-ins for clicks in the window self-test at the first implementation
/// (SPEC-0319 behaviour 44): they may only become fewer. Set-up that is not a user action uses
/// `Step::Setup`, clicks use the driver.
const STEP_ACT_STAND_INS: usize = 597;

/// `hit_targets::mark` sites at the first implementation: they may only become fewer, as views
/// get their annotation (`semantics::Annotated`).
const MARK_SITES: usize = 26;

fn count_lines(files: &[PathBuf], pattern: &str, skip: &[&str]) -> (usize, Vec<String>) {
    let mut n = 0;
    let mut at = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || skip.iter().any(|s| line.contains(s)) {
                continue;
            }
            let k = line.matches(pattern).count();
            if k > 0 {
                n += k;
                at.push(format!("{}:{}", f.display(), i + 1));
            }
        }
    }
    (n, at)
}

/// AK-11: the ratchet of the self-test's migration; it fails when a stand-in or a mark is added.
#[test]
fn ak11_ratchet_stand_ins_and_marks_only_fall() {
    let root = workspace();
    let mut app = Vec::new();
    rust_files(&root.join("crates/app/src"), &mut app);
    // The executor's own matches on the variant are not stand-ins.
    let (acts, _) = count_lines(&app, "Step::Act(", &["Step::Act(_)", "Step::Act(act)"]);
    assert!(
        acts <= STEP_ACT_STAND_INS,
        "{acts} `Step::Act` stand-ins, more than the {STEP_ACT_STAND_INS} recorded: click through the UI driver (Step::Click by test id) or use Step::Setup for set-up (SPEC-0319 behaviour 44)"
    );
    let mut crates = Vec::new();
    rust_files(&root.join("crates"), &mut crates);
    crates.retain(|f| !f.ends_with("hit_targets.rs") && !f.ends_with("tests/it/scans.rs"));
    let (marks, at) = count_lines(&crates, "hit_targets::mark(", &[]);
    assert!(
        marks <= MARK_SITES,
        "{marks} `hit_targets::mark` sites, more than the {MARK_SITES} recorded: annotate the element with `.semantics(…)` instead (SPEC-0319 behaviour 44):\n{}",
        at.join("\n")
    );
    // A count that fell is progress: lower the record in the same change.
    if acts < STEP_ACT_STAND_INS || marks < MARK_SITES {
        eprintln!(
            "ratchet: {acts} stand-ins (recorded {STEP_ACT_STAND_INS}), {marks} marks (recorded {MARK_SITES}): lower the records"
        );
    }
}

/// UISEC-9: nothing in the workspace posts input system-wide. `CGEventCreateCopy` in the
/// self-test's native clicks only copies an event of Ferris IDEA's own queue to set its button
/// number; the copy is posted to Ferris IDEA only (`SPEC-0319` v2 behaviour 20a).
#[test]
fn uisec9_no_system_wide_input() {
    let root = workspace();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    rust_files(&root.join("scripts"), &mut files);
    rust_files(&root.join("tests"), &mut files);
    let banned = [
        "CGEventPost",
        "CGEventCreateMouseEvent",
        "CGEventCreateKeyboardEvent",
        "CGEventCreateScrollWheelEvent",
        "CGEventTapCreate",
        "XTestFake",
        "XSendEvent",
        "SendInput(",
        "keybd_event",
        "mouse_event(",
        "AXUIElementPostKeyboardEvent",
        // Linux: XTEST through x11rb, a synthetic event sent through the X server, the
        // compositor's virtual pointer and keyboard, the kernel's input device (SPEC-0319 v5 20c).
        "xtest_fake_input",
        "send_event(",
        "zwlr_virtual_pointer",
        "zwp_virtual_keyboard",
        "/dev/uinput",
    ];
    // As a name of its own: `on_mouse_event(` of the toolkit is not `mouse_event(`.
    let words: Vec<regex::Regex> = banned
        .iter()
        .map(|b| {
            regex::Regex::new(&format!(r"(^|[^A-Za-z0-9_]){}", regex::escape(b))).expect("pattern")
        })
        .collect();
    let me = Path::new(file!()).file_name().unwrap();
    let mut found = Vec::new();
    for f in &files {
        if f.file_name() == Some(me) {
            continue;
        }
        let text = std::fs::read_to_string(f).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            for (b, word) in banned.iter().zip(&words) {
                if line.contains(b) && word.is_match(line) {
                    found.push(format!("{}:{}: {}", f.display(), i + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "system-wide input:\n{}", found.join("\n"));
    // Native events go to Ferris IDEA's own application queue only (macOS), or to the window's own
    // input handler (Linux, SPEC-0319 v5 behaviour 20c).
    let native = std::fs::read_to_string(root.join("crates/app/src/native_click.rs")).unwrap();
    assert!(native.contains("app.postEvent_atStart"));
    assert!(native.contains("window.dispatch_in_process_pointer_input("));
    // The vendored toolkit's hook is called from that file only, and it takes pointer events only.
    let hook = "dispatch_in_process_pointer_input(";
    let callers: Vec<_> = files
        .iter()
        .filter(|f| f.file_name() != Some(me) && !f.ends_with("crates/app/src/native_click.rs"))
        .filter(|f| {
            std::fs::read_to_string(f)
                .unwrap_or_default()
                .contains(hook)
        })
        .map(|f| f.display().to_string())
        .collect();
    assert!(
        callers.is_empty(),
        "the in-process pointer hook outside native_click.rs: {callers:?}"
    );
    let gpui = std::fs::read_to_string(root.join("vendor/gpui/src/window.rs")).unwrap();
    let at = gpui
        .find("pub fn dispatch_in_process_pointer_input(")
        .expect("the hook in vendor/gpui");
    let body = &gpui[at..at
        + gpui[at..]
            .find(
                "
    }
",
            )
            .expect("end of the hook")];
    for refused in [
        "PlatformInput::KeyDown(_)",
        "PlatformInput::KeyUp(_)",
        "PlatformInput::ModifiersChanged(_)",
        "PlatformInput::FileDrop(_) => false",
    ] {
        assert!(body.contains(refused), "the hook refuses {refused}");
    }
    // Every change in gpui's window.rs is one of these functions: the hook above, and the focus
    // that a drawn prompt keeps (SPEC-0281 v4 behaviour 9) and withdrawing a prompt (SPEC-0053 v11
    // behaviour 8) and capturing the frame (SPEC-0319 v7 behaviour 53), which dispatch no input. A new
    // change needs its place here.
    let changed: Vec<&str> = gpui
        .match_indices("Ferris IDEA change")
        .map(|(at, _)| {
            let rest = &gpui[at..];
            let f = rest.find("fn ").expect("a function after each change");
            rest[f + 3..]
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        changed,
        [
            "focus",
            "focus_ignoring_prompt",
            "blur",
            "has_drawn_prompt",
            "fallback_prompt",
            "withdraw_prompt",
            "capture_frame",
            "dispatch_in_process_pointer_input"
        ],
        "the changes in gpui's window.rs"
    );
    for name in [
        "focus_ignoring_prompt",
        "has_drawn_prompt",
        "fallback_prompt",
        "withdraw_prompt",
        "capture_frame",
    ] {
        let at = gpui.find(&format!("fn {name}(")).expect("the function");
        let end = at + gpui[at..].find("\n    }\n").expect("its end");
        assert!(
            !gpui[at..end].contains("dispatch"),
            "{name} dispatches no input"
        );
    }
}
