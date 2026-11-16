//! SPEC-0098 v9 behaviours 17–19 (OBS-0034 part 14, Q1–Q3): IDECraft's own Python quick fixes
//! change nothing a program does, which `python3` checks: the original and the fixed program print
//! the same; after "Create parameter" the function runs when the new argument is passed. Skipped
//! without `python3`.

use std::path::Path;
use std::process::Command;

use idecraft_languages::inspections::{fixes_at, inspect_text};

fn python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run(program: &str) -> (String, bool) {
    let out = Command::new("python3")
        .args(["-I", "-c", program])
        .output()
        .expect("python3 runs");
    (
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        out.status.success(),
    )
}

/// The text after the fix titled `title` at the first `needle`.
fn fixed(text: &str, needle: &str, title: &str) -> String {
    let at = text.find(needle).unwrap();
    let fix = fixes_at(Path::new("probe.py"), text, at)
        .into_iter()
        .find(|f| f.title == title)
        .unwrap_or_else(|| panic!("no {title:?} at {needle:?}"));
    idecraft_languages::jvm::apply(text, &fix.edits)
}

#[test]
fn fixed_programs_behave_the_same() {
    if !python3() {
        eprintln!("skipped: no python3");
        return;
    }
    let cases = [
        (
            "import os\nimport json\n\n\ndef f():\n    return json.dumps([1])\n\n\nprint(f())\n",
            "os\n",
            "Remove unused import",
        ),
        (
            "def f(a):\n    unused = 3\n    return a * 2\n\n\nprint(f(4))\n",
            "unused",
            "Remove statement",
        ),
        (
            "def f(a):\n    unused = 3\n    return a * 2\n\n\nprint(f(4))\n",
            "unused",
            "Remove assignment target",
        ),
        (
            "seen = []\n\n\ndef save(x):\n    seen.append(x)\n    return x\n\n\ndef f(a):\n    result = save(a)\n    return len(seen)\n\n\nprint(f(4), seen)\n",
            "result",
            "Remove assignment target",
        ),
        (
            "def f(flag):\n    if flag:\n        unused = (1, 'two', {3: None})\n    return 'done'\n\n\nprint(f(True))\n",
            "unused",
            "Remove statement",
        ),
    ];
    for (source, needle, title) in cases {
        let after = fixed(source, needle, title);
        assert_ne!(after, source, "{title}");
        let (before_out, ok) = run(source);
        assert!(ok, "{source}\n{before_out}");
        let (after_out, ok) = run(&after);
        assert!(ok, "{title}:\n{after}\n{after_out}");
        assert_eq!(after_out, before_out, "{title}:\n{after}");
    }
    // Create parameter: the function fails before, and runs once the argument is passed.
    let source = "def total(price, qty):\n    return price * qty * rate\n";
    let after = fixed(source, "rate\n", "Create parameter 'rate'");
    let (out, ok) = run(&format!("{source}\nprint(total(2, 3))\n"));
    assert!(!ok && out.contains("NameError"), "{out}");
    let (out, ok) = run(&format!("{after}\nprint(total(2, 3, 10))\n"));
    assert!(ok, "{after}\n{out}");
    assert_eq!(out, "60\n");
}

/// The file of OBS-0034 part 14: each of the three places has its fixes, and nothing is shown.
#[test]
fn the_places_of_the_observation_have_their_fixes() {
    let probe = "from app.prices import order_total\n\n\ndef price_of(item, factor):\n    return item * factor\n\n\ndef report(items):\n    unused = 3\n    return undefined_name + price_of(items, 2)\n";
    let titles = |needle: &str| -> Vec<String> {
        let at = probe.find(needle).unwrap();
        fixes_at(Path::new("probe.py"), probe, at)
            .into_iter()
            .map(|f| f.title)
            .collect()
    };
    assert_eq!(
        titles("order_total"),
        ["Remove unused import", "Optimize imports"]
    );
    assert_eq!(
        titles("unused ="),
        ["Remove assignment target", "Remove statement"]
    );
    assert_eq!(
        titles("undefined_name"),
        ["Create parameter 'undefined_name'"]
    );
    // Default level "no highlighting": IDECraft's own findings do not show in the editor.
    assert!(
        inspect_text(Path::new("probe.py"), probe)
            .iter()
            .filter(|f| f.id.starts_with("python.un"))
            .all(|f| f.level == idecraft_languages::inspections::Level::Info)
    );
}
