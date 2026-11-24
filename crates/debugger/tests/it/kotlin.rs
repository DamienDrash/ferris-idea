//! SPEC-0132 AK-2: a Kotlin program of a Gradle project stops at a breakpoint through
//! kotlin-debug-adapter with its locals, and resume runs it to its end. SPEC-0178 AK-3: a condition,
//! which this adapter cannot evaluate, is evaluated by Ferris IDEA and stops only once. SPEC-0132 v4 AK-7: the values of a stop — no `this`
//! in a top-level `main`, the sizes of collections. SPEC-0132 v5 AK-8: breakpoints in the members of classes of a second file. Skipped without the adapter (`kotlin-debug.sh install` of the reference environment) or Gradle
//! with the Kotlin plugin in its cache.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use ferris_idea_debugger::{
    Breakpoint, CLIENT_SIDE_NOTE, DebugSession, EvaluatedBy, SourceBreakpoint, StopState,
    known_adapters,
};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(120);
/// How long a launch may go without its first stop or end before it is tried again.
const FIRST: Duration = Duration::from_secs(45);

/// Launches and waits for the first stop or the end. kotlin-debug-adapter 0.4.4 sometimes starts
/// the program and never reports anything (seen in about one of five launches, also without
/// conditions); such a launch is ended and tried again, at most three times.
fn launch(
    command: &[String],
    launch: &serde_json::Value,
    breakpoints: &BTreeMap<PathBuf, Vec<SourceBreakpoint>>,
) -> (DebugSession, BTreeMap<PathBuf, Vec<Breakpoint>>, StopState) {
    for attempt in 1..=3 {
        let (mut session, placed) = DebugSession::launch(
            command,
            "kotlin",
            launch.clone(),
            breakpoints,
            &Default::default(),
        )
        .unwrap();
        match session.wait_state(FIRST, 10).unwrap() {
            StopState::Running => {
                eprintln!("launch {attempt}: no stop and no end, trying again");
                let _ = session.disconnect();
            }
            state => return (session, placed, state),
        }
    }
    panic!("kotlin-debug-adapter never stopped or ended in three launches");
}

const BUILD: &str = "buildscript {\n    repositories { mavenCentral() }\n    dependencies { classpath(\"org.jetbrains.kotlin:kotlin-gradle-plugin:2.0.21\") }\n}\n\nplugins { application }\napply(plugin = \"org.jetbrains.kotlin.jvm\")\n\nrepositories { mavenCentral() }\n\napplication { mainClass.set(\"demo.MainKt\") }\n";
/// The pause at the start of `main`: kotlin-debug-adapter 0.4.4 resumes the JVM at its first event,
/// before the breakpoints are registered, so code that runs at once can pass a breakpoint under load
/// (the program then ends without stopping). The window self-test pauses the same way.
const MAIN: &str = "package demo\n\nfun twice(x: Int): Int {\n    val y = x * 2\n    return y\n}\n\nfun main() {\n    Thread.sleep(500)\n    var total = 0\n    for (i in 1..3) {\n        total += twice(i)\n    }\n    println(\"total \" + total)\n}\n";

/// A Gradle project `name` with `main` as its `Main.kt`, built offline: the adapter's command, the
/// project folder and the source. `None`, with the reason printed, without the adapter or Gradle.
fn project(name: &str, main: &str) -> Option<(Vec<String>, PathBuf, PathBuf)> {
    project_with(name, main, &[])
}

/// [`project`] with further sources next to `Main.kt`, as file name and text.
fn project_with(
    name: &str,
    main: &str,
    others: &[(&str, &str)],
) -> Option<(Vec<String>, PathBuf, PathBuf)> {
    let Some(command) = known_adapters()
        .into_iter()
        .find(|a| a.name == "kotlin-debug-adapter")
        .and_then(|a| a.command)
    else {
        eprintln!("skipped: kotlin-debug-adapter is not installed");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("ferris-idea-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/main/kotlin/demo")).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    std::fs::write(
        dir.join("settings.gradle.kts"),
        "rootProject.name = \"ktdemo\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("build.gradle.kts"), BUILD).unwrap();
    let source = dir.join("src/main/kotlin/demo/Main.kt");
    std::fs::write(&source, main).unwrap();
    for (file, text) in others {
        std::fs::write(dir.join("src/main/kotlin/demo").join(file), text).unwrap();
    }
    let built = Command::new("gradle")
        .args(["--offline", "-q", "classes"])
        .current_dir(&dir)
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        eprintln!("skipped: gradle with the Kotlin plugin 2.0.21 in its cache is not available");
        return None;
    }
    Some((command, dir, source))
}

#[test]
fn kotlin_program_stops_at_breakpoint() {
    let Some((command, dir, source)) = project("ktdebug", MAIN) else {
        return;
    };
    let launch = json!({"projectRoot": dir, "mainClass": "demo.MainKt", "cwd": dir});
    let conditional = BTreeMap::from([(
        source.clone(),
        vec![SourceBreakpoint {
            line: 4,
            condition: Some("x == 2".into()),
            hit_condition: None,
            log_message: None,
            ..SourceBreakpoint::default()
        }],
    )]);
    let (mut session, placed, state) = self::launch(&command, &launch, &conditional);
    assert_eq!(placed[&source][0].evaluated_by, EvaluatedBy::FerrisIdea);
    match state {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert_eq!(frames[0].name, "twice");
            let x = locals.iter().find(|v| v.name == "x").unwrap();
            assert_eq!(x.value, "2");
            session.resume_continue(thread_id).unwrap();
        }
        other => panic!("not stopped: {other:?}"),
    }
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { .. } => {}
        other => panic!("stopped again or still running: {other:?}"),
    }
    assert!(
        session.output().contains("total 12"),
        "{}",
        session.output()
    );
    assert!(session.console().contains(CLIENT_SIDE_NOTE));
    let breakpoints = BTreeMap::from([(source.clone(), vec![SourceBreakpoint::from(4)])]);
    let (mut session, _, state) = self::launch(&command, &launch, &breakpoints);
    let thread = match state {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert_eq!(frames[0].name, "twice");
            let x = locals.iter().find(|v| v.name == "x").unwrap();
            assert_eq!(x.value, "1");
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    // Stops twice more (i = 2, 3), then ends.
    session.resume_continue(thread).unwrap();
    for _ in 0..2 {
        if let StopState::Stopped { thread_id, .. } = session.wait_state(WAIT, 10).unwrap() {
            session.resume_continue(thread_id).unwrap();
        }
    }
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { .. } => {}
        other => panic!("not ended: {other:?}"),
    }
    assert!(
        session.output().contains("total 12"),
        "{}",
        session.output()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A top-level `main` with a list, a map, a set, an empty list, a boxed number and an array, and
/// a member function to step into. The pause at the start is the one of [`MAIN`].
const VALUES: &str = r#"package demo

class Dog(val name: String) {
    fun speak(): String {
        val s = name + "!"
        return s
    }
}

fun main() {
    Thread.sleep(500)
    val names = mutableListOf("Rex")
    val first = names[0]
    val pet = Dog(first)
    val ages = mapOf("Rex" to 3, "Tom" to 4)
    val seen = setOf(1.5, 2.5)
    val none = emptyList<String>()
    val boxed: Int? = 300
    val arr = intArrayOf(1, 2, 3)
    println(pet.speak() + names + ages + seen + none + boxed + arr.size)
}
"#;

/// `Type@<digits>` followed by `rest`.
fn object_with(value: &str, class: &str, rest: &str) -> bool {
    value
        .strip_prefix(class)
        .and_then(|v| v.strip_prefix('@'))
        .is_some_and(|v| {
            let id = v.bytes().take_while(u8::is_ascii_digit).count();
            id > 0 && &v[id..] == rest
        })
}

#[test]
fn kotlin_values_have_sizes_and_no_this_without_a_receiver() {
    let Some((command, dir, source)) = project("ktvalues", VALUES) else {
        return;
    };
    let launch = json!({"projectRoot": dir, "mainClass": "demo.MainKt", "cwd": dir});
    // Line 20: the `println` at the end of `main`.
    let breakpoints = BTreeMap::from([(source.clone(), vec![SourceBreakpoint::from(20)])]);
    let (mut session, _, state) = self::launch(&command, &launch, &breakpoints);
    let thread = match state {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert_eq!(frames[0].name, "main");
            // OBS-0035 L12: a top-level function has no receiver.
            assert!(locals.iter().all(|v| v.name != "this"), "{locals:#?}");
            let value = |name: &str| {
                locals
                    .iter()
                    .find(|v| v.name == name)
                    .unwrap_or_else(|| panic!("no {name}: {locals:#?}"))
                    .value
                    .as_str()
            };
            // OBS-0035 L11: type, id and size instead of `instance of …`.
            for (name, class, rest) in [
                ("names", "ArrayList", " size = 1"),
                ("ages", "LinkedHashMap", " size = 2"),
                ("seen", "LinkedHashSet", " size = 2"),
                ("none", "EmptyList", " size = 0"),
                ("pet", "Dog", ""),
                ("arr", "int[3]", ""),
            ] {
                assert!(object_with(value(name), class, rest), "{name}: {locals:#?}");
            }
            assert_eq!(value("first"), "\"Rex\"");
            assert_eq!(value("boxed"), "300");
            // The row still opens to the list's fields.
            let names = locals.iter().find(|v| v.name == "names").unwrap();
            let fields = session.variables(names.reference).unwrap();
            assert!(
                fields.iter().any(|f| f.name == "size" && f.value == "1"),
                "{fields:#?}"
            );
            thread_id
        }
        other => panic!("not stopped: {other:?}"),
    };
    // Into `pet.speak()`: a member function has its object.
    session.step_in(thread).unwrap();
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Stopped {
            frames,
            locals,
            thread_id,
            ..
        } => {
            assert_eq!(frames[0].name, "speak");
            let this = locals.iter().find(|v| v.name == "this").expect("this");
            assert!(object_with(&this.value, "Dog", ""), "{locals:#?}");
            assert_ne!(this.reference, 0);
            session.resume_continue(thread_id).unwrap();
        }
        other => panic!("not stopped in speak: {other:?}"),
    }
    match session.wait_state(WAIT, 10).unwrap() {
        StopState::Exited { .. } => {}
        other => panic!("not ended: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The launched file of the two-file project: it only calls into [`ZOO`].
const ZOO_MAIN: &str = r#"package demo

fun main() {
    Thread.sleep(500)
    val pet: Animal = Cat("Tom")
    println(pet.describe())
    println(pet.speak())
    println(pet.twice { it + 1 })
    pet.runner().run()
    println(Animal.count() + legs(pet))
}
"#;

/// Classes named unlike their file `Zoo.kt`: members with an expression body (line 6) and a block
/// body (9), a lambda in a member (15), an anonymous object (22), a companion object (28), a
/// second class (34) and a top-level function (38).
const ZOO: &str = r#"package demo

abstract class Animal(val name: String) {
    abstract fun sound(): String

    open fun describe(): String = "$name says ${sound()}"

    fun speak(): String {
        val s = name + "!"
        return s
    }

    fun twice(f: (Int) -> Int): Int {
        val g = { x: Int ->
            f(x) * 2
        }
        return g(1)
    }

    fun runner(): Runnable = object : Runnable {
        override fun run() {
            println("ran " + name)
        }
    }

    companion object {
        fun count(): Int {
            return 2
        }
    }
}

class Cat(name: String) : Animal(name) {
    override fun sound() = "meow"
}

fun legs(animal: Animal): Int {
    return if (animal is Cat) 4 else 2
}
"#;

/// Resumes from `state` until the program ends: file name, line and function of every stop.
fn stops(session: &mut DebugSession, mut state: StopState) -> Vec<(String, i64, String)> {
    let mut seen = Vec::new();
    loop {
        match state {
            StopState::Stopped {
                frames, thread_id, ..
            } => {
                let top = &frames[0];
                let file = top
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                seen.push((file, top.line, top.name.clone()));
                session.resume_continue(thread_id).unwrap();
            }
            StopState::Exited { .. } => return seen,
            StopState::Running => panic!("neither stopped nor ended after {seen:?}"),
        }
        state = session.wait_state(WAIT, 10).unwrap();
    }
}

/// OBS-0035 L14: a breakpoint in a member of a class was reported as set and never hit, because
/// the adapter looks only into classes named like the file.
#[test]
fn kotlin_breakpoints_in_class_members_of_another_file_are_hit() {
    let Some((command, dir, main)) = project_with("ktmembers", ZOO_MAIN, &[("Zoo.kt", ZOO)]) else {
        return;
    };
    let zoo = dir.join("src/main/kotlin/demo/Zoo.kt");
    let launch = json!({"projectRoot": dir, "mainClass": "demo.MainKt", "cwd": dir});
    let breakpoints = BTreeMap::from([
        (main.clone(), vec![SourceBreakpoint::from(6)]),
        (
            zoo.clone(),
            vec![SourceBreakpoint::from(6), SourceBreakpoint::from(9)],
        ),
    ]);
    let (mut session, placed, state) = self::launch(&command, &launch, &breakpoints);
    assert!(placed[&zoo].iter().all(|b| b.verified), "{placed:#?}");
    let seen = stops(&mut session, state);
    assert_eq!(
        seen,
        [
            ("Main.kt".to_owned(), 6, "main".to_owned()),
            ("Zoo.kt".to_owned(), 6, "describe".to_owned()),
            ("Zoo.kt".to_owned(), 9, "speak".to_owned()),
        ]
    );
    assert!(
        session.output().contains("Tom says meow"),
        "{}",
        session.output()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other places of a class file: a lambda in a member, an anonymous object, a companion
/// object, a second class and a top-level function; a member breakpoint with a condition
/// Ferris IDEA evaluates (SPEC-0178) stops like a plain one.
#[test]
fn kotlin_breakpoints_in_lambdas_inner_classes_and_companions_are_hit() {
    let Some((command, dir, _)) = project_with("ktinner", ZOO_MAIN, &[("Zoo.kt", ZOO)]) else {
        return;
    };
    let zoo = dir.join("src/main/kotlin/demo/Zoo.kt");
    let launch = json!({"projectRoot": dir, "mainClass": "demo.MainKt", "cwd": dir});
    let conditional = SourceBreakpoint {
        line: 9,
        condition: Some("s == \"Tom!\"".into()),
        ..SourceBreakpoint::default()
    };
    let mut list: Vec<SourceBreakpoint> = [15, 22, 28, 34, 38].map(SourceBreakpoint::from).into();
    list.push(conditional);
    let breakpoints = BTreeMap::from([(zoo.clone(), list)]);
    let (mut session, _, state) = self::launch(&command, &launch, &breakpoints);
    let seen: Vec<(String, i64)> = stops(&mut session, state)
        .into_iter()
        .map(|(file, line, _)| (file, line))
        .collect();
    let zoo_at = |line: i64| ("Zoo.kt".to_owned(), line);
    assert_eq!(
        seen,
        [
            zoo_at(34),
            zoo_at(9),
            zoo_at(15),
            zoo_at(22),
            zoo_at(28),
            zoo_at(38),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
