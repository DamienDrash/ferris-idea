//! SPEC-0091 v12 AK-17 (Java half), v16 AK-21 (the size of a list), SPEC-0127 v3 AK-6 and SPEC-0146 v3 AK-6: the further
//! debugger actions against real jdtls with java-debug — every thread suspended by a breakpoint,
//! thread states and groups, the thread dump, a code fragment, an evaluated view, Inspect, a field
//! watchpoint, Step Out of Code Block and hot swap. Skipped without jdtls or the plugin; the JVM
//! is slow under load, so the waits are long.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ferris_idea_debugger::{
    BreakpointRules, FieldWatchpoint, SourceBreakpoint, SpecialBreakpoints, Suspend,
};
use ferris_idea_workbench::debug_model::{Command, Controller, Kind, Launch, Update};

// `main` reaches the breakpoint only once the worker sleeps: otherwise a worker still starting at
// the stop is listed as RUNNING instead of SLEEPING, which the thread checks below expect (the wait
// stands on the line of `w.start()`, so the lines the test names stay where they are).
const PROGRAM: &str = r#"public class Hot {
    static class Counter { int n; }
    record Item(String name, int qty) {}
    static int value() {
        return 1;
    }
    public static void main(String[] args) throws Exception {
        Thread w = new Thread(() -> { try { Thread.sleep(120000); } catch (InterruptedException e) {} }, "worker-1");
        w.setDaemon(true);
        w.start(); while (w.getState() != Thread.State.TIMED_WAITING) Thread.yield();
        java.util.List<Item> items = new java.util.ArrayList<>(java.util.List.of(new Item("pen", 3)));
        Counter c = new Counter();
        int sum = 0;
        for (int i = 0; i < 3; i++) {
            sum += i;
            c.n = c.n + 1;
        }
        for (int k = 0; k < 3; k++) {
            System.out.println("value " + value());
        }
        System.out.println("end " + sum + " " + items.size());
    }
}
"#;

/// Waits up to `secs` for an update `pick` accepts; the updates seen are kept for the message.
fn wait_for<T>(
    c: &Controller,
    seen: &mut Vec<Update>,
    secs: u64,
    what: &str,
    mut pick: impl FnMut(&Update) -> Option<T>,
) -> T {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        for u in c.poll() {
            if let Update::Failed(e) = &u {
                panic!("{what}: failed: {e}\n{seen:#?}");
            }
            let found = pick(&u);
            seen.push(u);
            if let Some(t) = found {
                return t;
            }
        }
        assert!(Instant::now() < until, "{what}: not seen\n{seen:#?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stopped_line(u: &Update) -> Option<(i64, i64, Vec<ferris_idea_debugger::Variable>)> {
    match u {
        Update::Stopped { frames, locals, .. } => {
            frames.first().map(|f| (f.id, f.line, locals.clone()))
        }
        _ => None,
    }
}

#[test]
fn further_actions_with_java_debug() {
    if !ferris_idea_languages::java_debug::java_debug_usable()
        || ferris_idea_languages::servers::resolve(ferris_idea_languages::LanguageId::Java).is_err()
    {
        eprintln!("skipped: jdtls or java-debug is missing");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ferris-idea-javamore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root: PathBuf = dir.canonicalize().unwrap();
    ferris_idea_core::trust::trust_for_process(&root);
    let file = root.join("Hot.java");
    std::fs::write(&file, PROGRAM).unwrap();
    // Line 15 `sum += i;`, stopping every thread (SPEC-0127 v3 behaviour 6).
    let all = SourceBreakpoint {
        line: 15,
        rules: BreakpointRules {
            suspend: Some(Suspend::All),
            ..BreakpointRules::default()
        },
        ..SourceBreakpoint::default()
    };
    let c = Controller::start(Launch {
        kind: Kind::Java,
        adapter: Vec::new(),
        interpreter: None,
        program: PathBuf::from("Hot"),
        args: Vec::new(),
        cwd: root.clone(),
        env: BTreeMap::new(),
        breakpoints: BTreeMap::from([(file.clone(), vec![all])]),
        prepare: String::new(),
        special: SpecialBreakpoints::default(),
        attach: None,
        browser: None,
        remote: None,
    });
    let mut seen = Vec::new();
    let (frame, line, locals) = wait_for(&c, &mut seen, 240, "the first stop", stopped_line);
    assert_eq!(line, 15);

    // SPEC-0091 v16 behaviour 42: the list reads type, id and size in Ferris IDEA's form; a static
    // `main` has no `this`.
    let items = locals.iter().find(|v| v.name == "items").expect("items");
    let (object, summary) = items.value.split_once(' ').unwrap_or((&items.value, ""));
    assert!(
        object.starts_with("ArrayList@") && summary == "size = 1",
        "{locals:#?}"
    );
    assert_eq!(
        ferris_idea_workbench::debug_model::row_value(&items.value)
            .1
            .map(|id| &items.value[id]),
        object.strip_prefix("ArrayList")
    );
    assert!(locals.iter().all(|v| v.name != "this"), "{locals:#?}");

    // v12 behaviour 28: states and groups; with every thread suspended the worker has frames.
    c.send(Command::Threads(false, frame));
    let infos = wait_for(&c, &mut seen, 60, "thread states", |u| match u {
        Update::ThreadInfos(t) => Some(t.clone()),
        _ => None,
    });
    let worker = infos.iter().find(|t| t.name == "worker-1").expect("worker");
    assert_eq!(
        (worker.state.as_str(), worker.group.as_str()),
        ("SLEEPING", "main")
    );
    assert!(worker.suspended, "suspend all: {infos:#?}");
    let main = infos.iter().find(|t| t.name == "main").expect("main");
    assert!(main.suspended);

    // Behaviour 26: the dump names the threads with state, group and frames.
    c.send(Command::Threads(true, frame));
    let dump = wait_for(&c, &mut seen, 60, "the thread dump", |u| match u {
        Update::Dump(t) => Some(t.clone()),
        _ => None,
    });
    assert!(
        dump.contains("\"worker-1\" SLEEPING (group main)"),
        "{dump}"
    );
    assert!(
        dump.contains("\tat Hot.main(String[]) (Hot.java:15)"),
        "{dump}"
    );

    // Behaviour 36: a code fragment of several statements (the reference failed on this kind).
    c.send(Command::Fragment(
        "int s = 0;\nfor (Item it : items) s += it.qty();\ns".into(),
        frame,
    ));
    let value = wait_for(&c, &mut seen, 60, "the fragment's value", |u| match u {
        Update::Evaluated(_, r) => Some(r.clone()),
        _ => None,
    });
    assert_eq!(value.as_deref(), Ok("3"));

    // Behaviour 32: the String view of the list; behaviour 30: Inspect evaluates the row.
    c.send(Command::Render(
        vec![((0, "items".into()), "String.valueOf(items)".into())],
        frame,
    ));
    let rendered = wait_for(&c, &mut seen, 60, "the String view", |u| match u {
        Update::Rendered(v) => Some(v.clone()),
        _ => None,
    });
    let text = rendered[0].1.as_ref().unwrap().value.clone();
    assert!(text.contains("Item[name=pen, qty=3]"), "{text}");
    c.send(Command::Inspect(
        "Inspect items".into(),
        "items".into(),
        false,
        frame,
    ));
    let inspected = wait_for(&c, &mut seen, 60, "Inspect", |u| match u {
        Update::Inspected(_, v) => Some(v.clone()),
        _ => None,
    });
    assert!(inspected.unwrap().reference != 0);

    // SPEC-0146 v3: a field watchpoint on `c.n` stops where the loop writes it.
    let counter = locals.iter().find(|v| v.name == "c").expect("c").reference;
    c.send(Command::FieldWatch(counter, "n".into()));
    let id = wait_for(&c, &mut seen, 60, "the field's id", |u| match u {
        Update::FieldWatch(r) => Some(r.clone()),
        _ => None,
    })
    .unwrap();
    assert_eq!(id, "Hot$Counter#n");
    c.send(Command::Special(SpecialBreakpoints {
        fields: vec![FieldWatchpoint {
            field: id,
            ..FieldWatchpoint::default()
        }],
        ..SpecialBreakpoints::default()
    }));
    c.send(Command::Breakpoints(file.clone(), Vec::new()));
    c.send(Command::Continue);
    let (_, line, _) = wait_for(&c, &mut seen, 60, "the watchpoint's stop", stopped_line);
    assert_eq!(line, 16, "c.n is written on line 16");

    // Behaviour 25: Step Out of Code Block leaves the loop for the next one.
    c.send(Command::Special(SpecialBreakpoints::default()));
    c.send(Command::StepOutOfBlock);
    let (_, line, _) = wait_for(&c, &mut seen, 120, "out of the block", stopped_line);
    assert_eq!(line, 18, "the first line after the loop");

    // Behaviour 37: a changed method is built and reloaded into the running program.
    std::fs::write(&file, PROGRAM.replace("return 1;", "return 2;")).unwrap();
    c.send(Command::SourcesChanged(vec![file.clone()]));
    wait_for(&c, &mut seen, 120, "the build of the change", |u| {
        matches!(u, Update::CodeChanged(_)).then_some(())
    });
    c.send(Command::ReloadClasses);
    let reloaded = wait_for(&c, &mut seen, 120, "the reload", |u| match u {
        Update::Reloaded(r) => Some(r.clone()),
        _ => None,
    });
    assert!(reloaded.unwrap().iter().any(|c| c == "Hot"), "{seen:#?}");
    c.send(Command::Continue);
    wait_for(&c, &mut seen, 120, "the end", |u| {
        matches!(u, Update::Exited(_)).then_some(())
    });
    let output: String = seen
        .iter()
        .filter_map(|u| match u {
            Update::Output(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(output.contains("value 2"), "{output}");
    assert!(output.contains("end 3 1"), "{output}");
    let _ = std::fs::remove_dir_all(&root);
}
