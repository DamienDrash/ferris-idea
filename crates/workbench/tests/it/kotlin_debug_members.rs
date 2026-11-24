//! SPEC-0132 v6 AK-10: the window's debug session with the real kotlin-debug-adapter. A Gradle
//! project whose second file declares seven classes and whose `main` reaches its first breakpoint
//! at once: the stop in the launched file, the stop in a member of a class named unlike its file,
//! and resume after the last stop runs the program to its end, in every launch. The program of the
//! window self-test: one stop, then the end. Skipped without the adapter or Gradle with the Kotlin
//! plugin in its cache.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ferris_idea_debugger::{SourceBreakpoint, SpecialBreakpoints, known_adapters};
use ferris_idea_workbench::debug_model::{Command, Controller, Kind, Launch, Update};

const BUILD: &str = "buildscript {\n    repositories { mavenCentral() }\n    dependencies { classpath(\"org.jetbrains.kotlin:kotlin-gradle-plugin:2.0.21\") }\n}\n\nplugins { application }\napply(plugin = \"org.jetbrains.kotlin.jvm\")\n\nrepositories { mavenCentral() }\n";

/// Seven top-level declarations: an abstract class, an interface with a default member, two
/// subclasses (one with a property getter), a sealed class with nested data classes and a class
/// with an empty body.
const ANIMALS: &str = r#"package zoo

/** Base type of all animals. */
abstract class Animal(val name: String) {
    abstract fun sound(): String

    open fun describe(): String = "$name says ${sound()}"
}

interface Pet {
    val owner: String
    fun friendly(): Boolean = true
}

class Dog(name: String) : Animal(name), Pet {
    override fun sound() = "woof"
    override val owner: String get() = "nobody"
}

class Cat(name: String) : Animal(name) {
    override fun sound() = "meow"
}

sealed class Shape {
    data class Circle(val radius: Double) : Shape()
    data class Square(val side: Double) : Shape()
}

class Person(val name: String, var age: Int) {
}
"#;

/// No pause before the first breakpoint (line 14).
const MAIN: &str = r#"package zoo

fun area(shape: Shape): Double = when (shape) {
    is Shape.Circle -> Math.PI * shape.radius * shape.radius
    is Shape.Square -> shape.side * shape.side
}

fun total(a: Int, b: Int): Int = a + b

fun main() {
    val names = mutableListOf("Ada")
    val first = names[0]
    val pet: Animal = Cat("Tom")
    println(pet.describe())
    println(area(Shape.Circle(2.0)) + total(1, 2))
    val ratio = 7 / 2
    println(ratio + first.length)
}
"#;

/// The program of the window self-test (`kotlin_debug_steps`).
const TWICE: &str = "package demo\n\nfun twice(x: Int): Int {\n    val y = x * 2\n    return y\n}\n\nfun main() {\n    Thread.sleep(500)\n    println(\"kotlin result \" + twice(21))\n}\n";

/// Waits up to `secs` for an update `pick` accepts; `None` when it does not come.
fn wait_for<T>(
    c: &Controller,
    seen: &mut Vec<Update>,
    secs: u64,
    mut pick: impl FnMut(&Update) -> Option<T>,
) -> Option<T> {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        for u in c.poll() {
            if let Update::Failed(e) = &u {
                panic!("failed: {e}\n{seen:#?}");
            }
            let found = pick(&u);
            seen.push(u);
            if found.is_some() {
                return found;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn stop(u: &Update) -> Option<(String, i64)> {
    match u {
        Update::Stopped { frames, .. } => frames.first().map(|f| (f.name.clone(), f.line)),
        _ => None,
    }
}

fn output(seen: &[Update]) -> String {
    seen.iter()
        .filter_map(|u| match u {
            Update::Output(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// One session of the main class `main_class`, resumed at every stop: the stops by function and
/// line and the output once the program has ended.
fn session(
    adapter: &[String],
    dir: &std::path::Path,
    main_class: &str,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
) -> (Vec<(String, i64)>, String) {
    let c = Controller::start(Launch {
        kind: Kind::Kotlin,
        adapter: adapter.to_vec(),
        interpreter: None,
        program: PathBuf::from(main_class),
        args: Vec::new(),
        cwd: dir.to_path_buf(),
        env: BTreeMap::new(),
        breakpoints: breakpoints.clone(),
        prepare: String::new(),
        special: SpecialBreakpoints::default(),
        attach: None,
        browser: None,
        remote: None,
    });
    let mut seen = Vec::new();
    let mut stops = Vec::new();
    // The first wait covers the adapter's start and its class path from Gradle. 90 s were not
    // enough on the Mac runner with other builds running (2026-11-16, load above 70): the
    // adapter had not answered yet. A deadline, not a check: a stop or the end must still come.
    let mut wait = 240;
    loop {
        let next = wait_for(&c, &mut seen, wait, |u| match u {
            Update::Exited(_) => Some(None),
            other => stop(other).map(Some),
        });
        match next {
            Some(Some(at)) => {
                stops.push(at);
                c.send(Command::Continue);
                wait = 30;
            }
            Some(None) => return (stops, output(&seen)),
            None => {
                c.send(Command::Stop);
                panic!(
                    "after the stops {stops:?} neither a stop nor the end came\n{}",
                    output(&seen)
                );
            }
        }
    }
}

/// A Gradle project in a fresh temporary folder with `files` below `src/main/kotlin`, built
/// offline: the adapter's command and the folder. `None`, with the reason printed, without the
/// adapter, Gradle or trust in the folder.
fn project(name: &str, files: &[(&str, &str)]) -> Option<(Vec<String>, PathBuf)> {
    let Some(adapter) = known_adapters()
        .into_iter()
        .find(|a| a.name == "kotlin-debug-adapter")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: kotlin-debug-adapter is not installed");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("ferris-idea-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    std::fs::write(
        dir.join("settings.gradle.kts"),
        "rootProject.name = \"zoo\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("build.gradle.kts"), BUILD).unwrap();
    for (file, text) in files {
        let path = dir.join("src/main/kotlin").join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let built = std::process::Command::new("gradle")
        .args(["--offline", "-q", "classes"])
        .current_dir(&dir)
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        eprintln!("skipped: gradle with the Kotlin plugin 2.0.21 in its cache is not available");
        return None;
    }
    if ferris_idea_core::trust::require_trusted(&dir).is_err() {
        eprintln!("skipped: the temporary project is not trusted");
        return None;
    }
    Some((adapter, dir))
}

/// OBS-0035 L14 in the window: the adapter starts the program before it listens to it, and this
/// program is at its first breakpoint by then in most launches. No launch is tried again here.
#[test]
fn the_window_session_stops_in_a_member_and_runs_to_the_end() {
    let files = [("zoo/Animals.kt", ANIMALS), ("zoo/Main.kt", MAIN)];
    let Some((adapter, dir)) = project("ktwindow", &files) else {
        return;
    };
    let sources = dir.join("src/main/kotlin/zoo");
    let breakpoints = BTreeMap::from([
        (sources.join("Main.kt"), vec![SourceBreakpoint::from(14)]),
        (sources.join("Animals.kt"), vec![SourceBreakpoint::from(7)]),
    ]);
    for launch in 1..=4 {
        let (stops, output) = session(&adapter, &dir, "zoo.MainKt", &breakpoints);
        assert_eq!(
            stops,
            [("main".to_owned(), 14), ("describe".to_owned(), 7)],
            "launch {launch}: {output}"
        );
        assert!(
            output.contains("Tom says meow"),
            "launch {launch}: {output}"
        );
        assert!(output.contains("6\n"), "launch {launch}: {output}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The only breakpoint is in a member that is never called: the program ends at once, in most
/// launches before the adapter listens to it, and the end is reported all the same.
#[test]
fn a_program_that_ends_at_once_is_reported_as_ended() {
    let files = [("zoo/Animals.kt", ANIMALS), ("zoo/Main.kt", MAIN)];
    let Some((adapter, dir)) = project("ktended", &files) else {
        return;
    };
    let animals = dir.join("src/main/kotlin/zoo/Animals.kt");
    // Line 16: `Dog.sound`.
    let breakpoints = BTreeMap::from([(animals, vec![SourceBreakpoint::from(16)])]);
    for launch in 1..=4 {
        let (stops, output) = session(&adapter, &dir, "zoo.MainKt", &breakpoints);
        assert!(stops.is_empty(), "launch {launch}: {stops:?}");
        assert!(output.contains("6\n"), "launch {launch}: {output}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A program still running when Ferris IDEA asks for a lost stop is left alone: it stops at its
/// breakpoint later and runs to its end.
#[test]
fn a_program_that_runs_a_while_before_its_first_stop_is_left_alone() {
    let slow = TWICE.replace("Thread.sleep(500)", "Thread.sleep(5000)");
    let Some((adapter, dir)) = project("ktslow", &[("demo/Main.kt", &slow)]) else {
        return;
    };
    let main = dir.join("src/main/kotlin/demo/Main.kt");
    let breakpoints = BTreeMap::from([(main, vec![SourceBreakpoint::from(5)])]);
    let (stops, output) = session(&adapter, &dir, "demo.MainKt", &breakpoints);
    assert_eq!(stops, [("twice".to_owned(), 5)], "{output}");
    assert!(output.contains("kotlin result 42"), "{output}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// One stop in a top-level function, then resume runs the program to its end.
#[test]
fn resume_after_the_last_stop_runs_the_program_to_its_end() {
    let Some((adapter, dir)) = project("ktresume", &[("demo/Main.kt", TWICE)]) else {
        return;
    };
    let main = dir.join("src/main/kotlin/demo/Main.kt");
    let breakpoints = BTreeMap::from([(main, vec![SourceBreakpoint::from(5)])]);
    for launch in 1..=3 {
        let (stops, output) = session(&adapter, &dir, "demo.MainKt", &breakpoints);
        assert_eq!(
            stops,
            [("twice".to_owned(), 5)],
            "launch {launch}: {output}"
        );
        assert!(
            output.contains("kotlin result 42"),
            "launch {launch}: {output}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
