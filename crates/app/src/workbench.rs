//! `idecraft ui [folder]`: the workbench window (`SPEC-0059`), and `--selftest`, which presses keys
//! in a real window on a temporary folder and checks what they did.

use idecraft_languages::insight::CallDirection;
use idecraft_workbench::doc_model::DocBlock;
use std::path::{Path, PathBuf};

use gpui::{App, AppContext, Application, Context, Entity, Keystroke, Window, div, prelude::*};
use idecraft_workbench::{DebugState, WorkbenchView};

use crate::native_click::Click;

/// `%xx` escapes of a file URL path decoded.
fn percent_decoded(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Opens the workbench on `path`: a folder as the project; a file opens its project and itself in
/// an editor, in a running window that serves the project when there is one (`SPEC-0094` v7
/// behaviours 18 and 19).
pub fn run(path: &Path, selftest: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (root, file) = if selftest {
        // A misspelt group ends the run before a window opens.
        crate::selftest_groups::from_env(&group_names(&SelfTest::blocks(KeyLayout::default())))?;
        (fixture()?, None)
    } else {
        let target = idecraft_core::project_root::open_target(path)?;
        (target.root, target.file)
    };
    if let Some(file) = &file
        && hand_off(&root, file)
    {
        return Ok(());
    }
    start(Some(root), file, selftest)
}

/// Hands `file` of the project at `root` to a running IDECraft that serves the project, through
/// its MCP server (`editor_tabs` `open`, `SPEC-0094` v7 behaviours 19 and 21). Whether one took it;
/// a refusal or no answer is reported and a window of this process opens instead.
fn hand_off(root: &Path, file: &Path) -> bool {
    let Some(config) = idecraft_core::user_config_dir() else {
        return false;
    };
    let Ok(relative) = file.strip_prefix(root) else {
        return false;
    };
    for window in idecraft_mcp::client::running_windows(&config).iter().rev() {
        let Some(served) = window
            .projects
            .iter()
            .find(|p| Path::new(p).canonicalize().ok().as_deref() == Some(root))
        else {
            continue;
        };
        let opened = idecraft_mcp::client::Client::connect_project(window, served, "idecraft-cli")
            .and_then(|mut c| {
                c.call(
                    "editor_tabs",
                    serde_json::json!({ "action": "open", "path": relative.display().to_string() }),
                )
            });
        match opened {
            Ok(_) => {
                println!(
                    "opened {} in IDECraft (pid {}, {})",
                    relative.display(),
                    window.pid,
                    root.display()
                );
                return true;
            }
            Err(e) => {
                eprintln!(
                    "warning: the running window did not take the file ({e}); opening a window"
                );
            }
        }
    }
    false
}

/// `idecraft launch [folder]` and a start from the app bundle (`SPEC-0282` behaviours 6–7): the
/// folder, else the most recent project that still exists, else the system's folder chooser.
pub fn launch(folder: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let recent = idecraft_core::recent_projects_file()
        .map(|f| idecraft_core::load_recent_projects(&f))
        .unwrap_or_default();
    // System Settings › Reopen the last project at start-up (SPEC-0337 behaviour 2).
    let reopen = idecraft_core::ide_settings_file()
        .map(|f| {
            idecraft_core::load_ide_settings(&f)
                .system
                .reopen_last_project
        })
        .unwrap_or(true);
    let root = match crate::launch::start_folder(folder, &recent, reopen) {
        Some(p) => Some(p.canonicalize()?),
        None => None,
    };
    // A file opens in an editor of its project (SPEC-0094 v7 behaviour 19).
    let file = folder
        .and_then(|f| idecraft_core::project_root::open_target(f).ok())
        .and_then(|t| t.file);
    if let (Some(root), Some(file)) = (&root, &file)
        && hand_off(root, file)
    {
        return Ok(());
    }
    start(root, file, false)
}

/// `idecraft open <file>` without a running window (`SPEC-0303` behaviour 17): starts as
/// `idecraft launch` does and shows the file's import preview in the first window.
pub fn launch_with_import(file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    idecraft_workbench::queue_opened(vec![file.canonicalize()?]);
    launch(None)
}

/// Hands files the system or `idecraft open` asked to open to the front project window, as soon
/// as there is one (`SPEC-0303` behaviour 17).
fn serve_opened_files(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let files = idecraft_workbench::take_opened();
            if files.is_empty() {
                continue;
            }
            let mut waiting = Vec::new();
            for f in files {
                let shown = cx.update(|cx| WorkbenchView::show_import_in_front(&f, cx));
                match shown {
                    Ok(Ok(_)) => {}
                    Ok(Err(_)) => waiting.push(f),
                    Err(_) => return,
                }
            }
            idecraft_workbench::queue_opened(waiting);
        }
    })
    .detach();
}

/// Runs the application with a window on `root`, or asks for a folder first when there is none;
/// `file` opens in an editor of that window.
fn start(
    root: Option<PathBuf>,
    file: Option<PathBuf>,
    selftest: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Untrusted projects open in safe mode (SPEC-0278); the self-test trusts its own folders, so
    // no question waits for an answer (behaviour 10).
    idecraft_core::trust::enforce();
    if selftest {
        idecraft_core::trust::trust_all_for_process();
    }
    // Language servers stay running while the window is open (SPEC-0090).
    idecraft_languages::pool::enable();
    // A signal asking to end quits the normal way, so quit work runs (SPEC-0262 behaviour 3).
    if let Err(e) = crate::termination::catch() {
        eprintln!("warning: signals end IDECraft at once: {e}");
    }
    let app = Application::new();
    // Exchange files opened from Finder or with `open` arrive as file URLs (SPEC-0303 behaviour
    // 17); the window shows their preview.
    app.on_open_urls(|urls| {
        let files: Vec<PathBuf> = urls
            .iter()
            .filter_map(|u| u.strip_prefix("file://"))
            .map(|p| PathBuf::from(percent_decoded(p)))
            .filter(|p| idecraft_core::format::is_exchange_file(p))
            .collect();
        idecraft_workbench::queue_opened(files);
    });
    app.run(move |cx: &mut App| {
        serve_opened_files(cx);
        cx.on_app_quit(|_| async { idecraft_languages::pool::shutdown() })
            .detach();
        // Programs of run configurations end with IDECraft, with their children (SPEC-0261 v3
        // behaviour 13). Done before the quit hooks' futures, which only get a short time.
        // However the quit came about, the process is gone within the deadline (SPEC-0261 v4
        // behaviour 17): ending windows with debug sessions took minutes in a window run.
        cx.on_app_quit(|_| {
            idecraft_core::arm_quit_deadline();
            idecraft_core::end_all_runs(std::time::Duration::from_millis(500));
            async {}
        })
        .detach();
        // A signal quits without a question, also with running work or a question shown, and
        // within the deadline even if the platform does not carry out the quit (SPEC-0262 v2
        // behaviour 3).
        cx.spawn(async move |cx| {
            while !crate::termination::requested() {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
            }
            idecraft_core::arm_quit_deadline();
            let _ = cx.update(|cx| cx.quit());
        })
        .detach();
        // The brand icon in the Dock, also for a bare binary (SPEC-0060 v6 behaviour 12).
        crate::app_icon::install();
        // The window buttons 20 pt apart (SPEC-0061 v7 behaviour 44).
        idecraft_workbench::traffic_lights::register(crate::traffic_lights::space);
        // Two quick clicks on a control of the title bar press it twice and never zoom the
        // window (SPEC-0061 v15 behaviour 61).
        idecraft_workbench::title_bar::register(crate::title_bar::pressed);
        if !idecraft_workbench::fonts::register(cx) {
            eprintln!("warning: JetBrains Mono could not be registered; using a system font");
        }
        if selftest {
            // Controls report where they are painted, so steps can click them (SPEC-0275).
            idecraft_workbench::hit_targets::enable();
            // With the driver switch, the semantic tree is built for the click and hover steps
            // (SPEC-0319 behaviour 42).
            if std::env::var("IDECRAFT_SELFTEST_DRIVER")
                .is_ok_and(|v| !matches!(v.as_str(), "" | "0" | "off"))
            {
                idecraft_workbench::semantics::set_consumer(
                    idecraft_workbench::semantics::Consumer::SelfTest,
                    true,
                );
            }
        }
        if selftest && let Some(dir) = FIXTURE.get() {
            // The self-test keeps its recent projects in its own folder (SPEC-0094).
            cx.set_global(idecraft_workbench::project_windows::RecentProjectsFile(
                dir.join(".idecraft/recent-projects"),
            ));
            // …and the state of remote projects: stand-in folders, recovery copies, the recent
            // remote projects (SPEC-0317 v3).
            cx.set_global(idecraft_workbench::RemoteStateDir(
                dir.join(".idecraft/remote"),
            ));
            // …and its theme choice (SPEC-0060 v5).
            cx.set_global(idecraft_workbench::appearance::ThemeFile(
                dir.join(".idecraft/theme"),
            ));
            cx.set_global(idecraft_workbench::keymaps::UserKeymapFile(
                dir.join(".idecraft/keymap.toml"),
            ));
            cx.set_global(idecraft_workbench::appearance::IdeSettingsFile(
                dir.join(".idecraft/ide-settings.toml"),
            ));
            // Pages are debugged in Chrome without a window (SPEC-0133).
            idecraft_workbench::debug_model::set_browser_headless(true);
        }
        // Notes about colours a theme's files lack go to the theme log (SPEC-0060 v10
        // behaviour 15); the self-test keeps it in its own folder.
        let theme_log = match (selftest, FIXTURE.get()) {
            (true, Some(dir)) => Some(dir.join(".idecraft/log")),
            (true, None) => None,
            (false, _) => idecraft_core::logs::log_dir(),
        };
        if let Some(dir) = theme_log {
            cx.set_global(idecraft_workbench::appearance::ThemeLog(
                dir.join(idecraft_workbench::appearance::THEME_LOG),
            ));
        }
        idecraft_workbench::appearance::apply_saved_theme(cx);
        idecraft_workbench::appearance::apply_saved_settings(cx);
        idecraft_workbench::keymaps::load(cx);
        // A key press hides the shown tooltip (SPEC-0294 behaviour 6).
        idecraft_workbench::tips::install(cx);
        idecraft_workbench::menu_bar::install(cx);
        // MCP over HTTP for agents (SPEC-0221); the self-test keeps its discovery file to itself.
        let discovery = if selftest {
            Some(
                std::env::temp_dir()
                    .join(format!("idecraft-selftest-config-{}", std::process::id())),
            )
        } else {
            idecraft_core::user_config_dir()
        };
        crate::mcp_http::install(discovery, cx);
        // The UI driver's host (SPEC-0319): serves the window's MCP clients when the user allows it.
        crate::ui_driver::install(cx);
        if selftest {
            // The self-test cannot answer native alerts, so confirmations are drawn (SPEC-0281
            // behaviour 7).
            cx.set_global(idecraft_workbench::dialogs::DrawnConfirmations);
        }
        // After Close Project closed the last project window: the folder chooser, as at a start
        // without a folder (SPEC-0094 v3 behaviour 9).
        cx.set_global(idecraft_workbench::project_windows::OnLastProjectClosed(
            std::rc::Rc::new(choose_folder),
        ));
        match root {
            Some(root) => open_first_window(root, file, selftest, cx),
            None => choose_folder(cx),
        }
        cx.activate(true);
    });
    crate::no_window::outcome()
}

/// Asks for a project folder with the system's chooser and opens it; cancelling quits
/// (`SPEC-0282` behaviour 6).
fn choose_folder(cx: &mut App) {
    let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        let folder = match chosen.await {
            Ok(Ok(Some(paths))) => paths.into_iter().next(),
            Ok(Err(e)) => {
                eprintln!("error: the folder chooser failed: {e}");
                None
            }
            _ => None,
        };
        let _ = cx.update(|cx| match folder {
            Some(folder) => {
                if let Err(e) = idecraft_workbench::project_windows::open_project(&folder, cx) {
                    eprintln!("error: {}: {e}", folder.display());
                    cx.quit();
                }
            }
            None => cx.quit(),
        });
    })
    .detach();
}

/// The first window, on `root`, with the self-test's view around the workbench; `file` opens in a
/// focused editor tab at once (`SPEC-0094` v7 behaviour 19).
fn open_first_window(root: PathBuf, file: Option<PathBuf>, selftest: bool, cx: &mut App) {
    let title = format!("IDECraft — {}", root.display());
    let options = idecraft_workbench::project_windows::window_options(&title, cx);
    let shown = root.clone();
    let opened = cx.open_window(options, move |window, cx| {
        idecraft_workbench::x11_wake::until_first_frame(window);
        window.set_window_title(&title);
        let bench = cx.new(|cx| WorkbenchView::new(&root, cx));
        idecraft_workbench::project_windows::close_button_closes_the_project(&bench, window, cx);
        bench.update(cx, |b, cx| b.watch_window(window, cx));
        window.focus(&bench.read(cx).start_focus(cx));
        idecraft_workbench::appearance::watch_os_appearance(window);
        if let Some(file) = &file {
            bench.update(cx, |b, cx| b.open(file, window, cx));
        }
        // An untrusted project asks once its window is there (SPEC-0278 behaviour 1).
        let asking = bench.clone();
        window.defer(cx, move |window, cx| {
            asking.update(cx, |b, cx| b.ask_trust(true, window, cx));
        });
        let layout = KeyLayout::of(cx);
        cx.new(|_| SelfTest::new(bench, selftest, layout))
    });
    match opened {
        Ok(handle) => {
            idecraft_workbench::project_windows::register_window(&shown, handle.into(), cx)
        }
        Err(e) => {
            crate::no_window::failed(e, cx);
        }
    }
}

/// What the keyboard layout of this machine changes in the keys the self-test presses, so that
/// a key is pressed the way a user of the layout presses it.
#[derive(Debug, Clone, Copy, Default)]
struct KeyLayout {
    /// The key in the place of `[` (German: `ö`), so Back is pressed the way the user presses it
    /// (SPEC-0071 v4 behaviour 6).
    open_bracket: Option<char>,
    /// Whether the layout types `=` with ⇧0 (German): ⌘= is then ⇧⌘0, Fit in the ER diagram,
    /// and zooming in is ⌘+ (SPEC-0111 v4).
    equals_is_shift_zero: bool,
}

impl KeyLayout {
    fn of(cx: &App) -> Self {
        let equivalents = cx.keyboard_mapper().get_key_equivalents();
        Self {
            open_bracket: equivalents.and_then(|e| e.get(&'[').copied()),
            equals_is_shift_zero: equivalents.and_then(|e| idecraft_ui::keymap::us_key_for("=", e))
                == Some(')'),
        }
    }
}

/// The self-test's folder, for checks that look at the repository.
static FIXTURE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn git(dir: &Path, args: &[&str]) -> std::io::Result<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The self-test's shells (SPEC-0313 AK-6) do not depend on the person's shell configuration:
/// zsh where it exists, with start-up files from a folder of the self-test's own (a fixed prompt,
/// nothing else) and IDECraft's shell integration; else `$SHELL` as usual.
fn selftest_shell() -> std::io::Result<()> {
    let zsh = "/bin/zsh";
    if !Path::new(zsh).exists() {
        return Ok(());
    }
    let zdotdir =
        std::env::temp_dir().join(format!("idecraft-ui-selftest-shell-{}", std::process::id()));
    std::fs::create_dir_all(&zdotdir)?;
    std::fs::write(
        zdotdir.join(".zshrc"),
        "# The window self-test's shells: no personal configuration, a fixed prompt.\nPS1='%1~ %# '\n",
    )?;
    idecraft_workbench::set_local_shell(
        Some(zsh.to_owned()),
        vec![(
            "IDECRAFT_USER_ZDOTDIR".to_owned(),
            zdotdir.to_string_lossy().into_owned(),
        )],
    );
    Ok(())
}

/// A Git repository with `a.rs`, `b.rs` and `bad.c` committed, then `b.rs` changed and `new.txt` added.
fn fixture() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("idecraft-ui-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("a.rs"), "fn a() {}\nfn b() {}\n")?;
    std::fs::write(dir.join("b.rs"), "fn c() {}\n")?;
    // A C file with an error for the language server (clangd) to report.
    std::fs::write(
        dir.join("bad.c"),
        "int helper(void) { return 1; }\nint main(void) { int a = helper(); return a + missing; }\n",
    )?;
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Self Test"],
        &["config", "user.email", "selftest@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        &["add", "-A"],
        &["commit", "-q", "-m", "initial"],
    ] {
        git(&dir, args)?;
    }
    std::fs::write(dir.join("b.rs"), "fn c() { changed() }\n")?;
    std::fs::write(dir.join("new.txt"), "untracked\n")?;
    let dir = dir.canonicalize()?;
    let _ = FIXTURE.set(dir.clone());
    selftest_shell()?;
    // The self-test does not depend on the dictionaries installed on this machine (SPEC-0159).
    idecraft_languages::grazie::set_typos(false);
    Ok(dir)
}

/// A check of the workbench state.
type Check = Box<dyn Fn(&WorkbenchView, &App) -> bool>;

/// How long a `WaitFor` step waits. Generous, since language servers answer slowly on a busy
/// machine (builds running beside the self-test); a passing step goes on at once.
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// One step: a keystroke, or a check of the state.
enum Step {
    Key(&'static str, Option<&'static str>),
    Expect(&'static str, Check),
    /// Like `Expect`, but retried every frame for up to [`WAIT`] (output of a program, a language
    /// server's answer).
    WaitFor(&'static str, Check),
    /// Like `WaitFor` with a longer limit, for steps that build a project first (Gradle and a
    /// debug adapter that may take up to two minutes to start).
    WaitLong(&'static str, std::time::Duration, Check),
    /// Does what a mouse click on a control does (the self-test sends keys, not clicks). Counted
    /// by the ratchet of SPEC-0319 behaviour 44: new steps use the driver for clicks and
    /// [`Step::Setup`] for set-up that is not a user action.
    Act(Act),
    /// Set-up that is not a user action (writing a fixture file, starting a server), run like
    /// `Act` but not counted as a click stand-in (SPEC-0319 behaviour 43).
    Setup(Act),
    /// A real mouse click on a control marked with `hit_targets::mark`, at its centre, posted as a
    /// native event (SPEC-0275 behaviour 12). The click arrives later: check with `WaitFor`. With
    /// `IDECRAFT_SELFTEST_DRIVER` set, the UI driver addresses the control by the mark's name as
    /// its test id and waits until it is actionable (SPEC-0319 behaviour 42).
    Click(String, Click),
    /// The pointer moves to the centre of a control marked with `hit_targets::mark` and rests
    /// there (SPEC-0294 AK-7); its tooltip appears after the delay: check with `WaitFor`.
    Hover(String),
    /// Not a step: the steps after it belong to the named group, up to the next mark. [`plan`]
    /// takes the marks out before a run.
    Group(&'static str),
}

/// The steps after this mark belong to group `name`.
fn group(name: &'static str) -> Step {
    Step::Group(name)
}

/// Steps built together, with the names of their groups in run order: the steps up to the first
/// [`Step::Group`] mark belong to the first name, each mark starts the group it names.
struct Block {
    names: Vec<&'static str>,
    build: Box<dyn FnOnce() -> Vec<Step>>,
}

impl Block {
    fn new(names: &[&'static str], build: impl FnOnce() -> Vec<Step> + 'static) -> Self {
        Self {
            names: names.to_vec(),
            build: Box::new(build),
        }
    }
}

/// The names of all groups in run order.
fn group_names(blocks: &[Block]) -> Vec<&'static str> {
    blocks
        .iter()
        .flat_map(|b| b.names.iter().copied())
        .collect()
}

/// What a run does: its steps, where each group ends, and the groups left out on request.
#[derive(Default)]
struct Plan {
    steps: Vec<Step>,
    /// Per group that runs, in run order: its name, the index after its last step and the number
    /// of its steps.
    spans: Vec<(&'static str, usize, usize)>,
    skipped: Vec<&'static str>,
}

/// The groups of one block with their steps. A mark that is not one of the block's names after
/// the first, or out of order, is an error: every step has to belong to a declared group.
fn split_block(block: Block) -> Result<Vec<(&'static str, Vec<Step>)>, String> {
    let mut groups: Vec<(&'static str, Vec<Step>)> =
        block.names.iter().map(|n| (*n, Vec::new())).collect();
    let mut at = 0;
    for step in (block.build)() {
        match step {
            Step::Group(name) => {
                at = groups
                    .iter()
                    .skip(at + 1)
                    .position(|(n, _)| *n == name)
                    .map(|i| at + 1 + i)
                    .ok_or_else(|| {
                        format!(
                            "self-test: the mark {name:?} is not a later group of the block {:?}",
                            block.names
                        )
                    })?;
            }
            step => groups[at].1.push(step),
        }
    }
    Ok(groups)
}

/// Builds the blocks a selection needs and keeps the steps of the groups it takes.
fn plan(
    blocks: Vec<Block>,
    select: impl FnOnce(&[&'static str]) -> Result<crate::selftest_groups::Selection, String>,
) -> Result<Plan, String> {
    let names = group_names(&blocks);
    let selection = select(&names)?;
    let mut plan = Plan {
        skipped: selection.skipped,
        ..Plan::default()
    };
    let mut first = 0;
    for block in blocks {
        let range = first..first + block.names.len();
        first = range.end;
        if !selection.run[range.clone()].iter().any(|run| *run) {
            continue;
        }
        for ((name, steps), run) in split_block(block)?.into_iter().zip(&selection.run[range]) {
            if *run {
                let count = steps.len();
                plan.steps.extend(steps);
                plan.spans.push((name, plan.steps.len(), count));
            }
        }
    }
    Ok(plan)
}

/// `idecraft ui --selftest --list-groups`: the groups in run order with the number of their steps
/// on this machine (a group whose tools are missing has none), then the meta groups. No window.
pub fn list_groups() -> Result<(), Box<dyn std::error::Error>> {
    let blocks = SelfTest::blocks(KeyLayout::default());
    let all = plan(blocks, |names| {
        crate::selftest_groups::select(names, None, None)
    })?;
    for (name, _, steps) in &all.spans {
        println!("{name}: {steps} steps");
    }
    for (meta, members) in crate::selftest_groups::META {
        println!("{meta} = {}", members.join(", "));
    }
    Ok(())
}

/// The pointer moves onto the control marked `target`.
fn hover(target: impl Into<String>) -> Step {
    Step::Hover(target.into())
}

/// Whether the tooltip shown says what `text` accepts, lies inside the window and leaves the pointer
/// free (SPEC-0294 behaviours 7 and 10).
fn tooltip_shows(text: impl Fn(&str) -> bool) -> bool {
    idecraft_ui::tooltip::shown().is_some_and(|s| text(&s.text) && s.well_placed())
}

/// The text the keymap in use gives the declared control `name` (SPEC-0294 behaviour 2).
fn declared_tip(name: &str) -> String {
    let control = idecraft_ui::tooltip::find(name).expect("declared control");
    idecraft_ui::tooltip::Tip::of(
        control,
        &idecraft_workbench::keymaps::current(),
        idecraft_workbench::tips::macos(),
    )
    .text()
}

/// Tooltips in the window (SPEC-0294 AK-7): the Run button and a stripe button show theirs with the
/// shortcut of the keymap in use, inside the window and away from the pointer; a key press hides
/// one, moving to another control hides it at once.
fn tooltip_steps() -> Vec<Step> {
    vec![
        hover("run-button"),
        Step::WaitFor(
            "hovering the Run button shows its tooltip with the Run shortcut, inside the window and away from the pointer",
            Box::new(|_, _| {
                let shortcut = idecraft_workbench::keymaps::current()
                    .shortcut_label("Run", idecraft_workbench::tips::macos());
                tooltip_shows(|t| t.starts_with("Run") && t.ends_with(&shortcut))
            }),
        ),
        Step::Key("escape", None),
        Step::WaitFor(
            "a key press hides the tooltip",
            Box::new(|_, _| idecraft_ui::tooltip::shown().is_none()),
        ),
        hover("stripe-project"),
        Step::WaitFor(
            "hovering the Project stripe button shows its title and activation shortcut",
            Box::new(|_, _| {
                let want = idecraft_ui::tooltip::stripe(
                    idecraft_workbench::tool_windows::ToolWindow::Project,
                    &idecraft_workbench::keymaps::current(),
                    idecraft_workbench::tips::macos(),
                )
                .text();
                tooltip_shows(|t| t == want)
            }),
        ),
        hover("project-widget"),
        Step::WaitFor(
            "moving to another control hides the stripe button's tooltip",
            Box::new(|_, _| {
                idecraft_ui::tooltip::shown().is_none_or(|s| !s.text.starts_with("Project"))
            }),
        ),
        Step::WaitFor(
            "the project widget then shows its own tooltip",
            Box::new(|_, _| tooltip_shows(|t| t == declared_tip("project-widget"))),
        ),
        Step::Expect(
            "the inspection lists the declared tooltips of the drawn controls",
            Box::new(|_, _| {
                let declared = idecraft_ui::tooltip::declared(
                    &idecraft_workbench::keymaps::current(),
                    idecraft_workbench::tips::macos(),
                );
                ["run-button", "stripe-project", "project-widget"]
                    .iter()
                    .all(|n| declared.iter().any(|(name, _)| name == n))
            }),
        ),
        Step::Key("escape", None),
    ]
}

/// What the system does on a double click in a title bar (System Settings, Desktop & Dock):
/// `true` when it zooms or fills the window, which is also the default; `false` when it minimises
/// the window or does nothing.
fn title_bar_double_click_zooms() -> bool {
    let set = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleActionOnDoubleClick"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    !matches!(set.as_deref(), Some("Minimize" | "None"))
}

/// `SPEC-0061` v15 AK-v15: two quick clicks (a native double click) on a control of the title
/// bar press the control twice and leave the window's size alone; on the empty part of the title
/// bar they stay the system's gesture and zoom the window, and zoom it back.
fn title_bar_click_steps() -> Vec<Step> {
    use std::cell::Cell;
    use std::time::{Duration, Instant};
    type Size = gpui::Size<gpui::Pixels>;
    thread_local! {
        static BEFORE: Cell<Option<Size>> = const { Cell::new(None) };
        static AFTER: Cell<Option<Size>> = const { Cell::new(None) };
        static SINCE: Cell<Option<Instant>> = const { Cell::new(None) };
        static PRESSES: Cell<usize> = const { Cell::new(0) };
    }
    /// Longer than the system's zoom animation, so a zoom that starts has ended.
    const SETTLE: Duration = Duration::from_millis(1200);
    // Notes the window's size before a double click.
    let before = || {
        Step::Setup(Box::new(|_, window, _| {
            BEFORE.with(|s| s.set(Some(window.viewport_size())));
            AFTER.with(|s| s.set(None));
            PRESSES.with(|s| s.set(crate::title_bar::presses()));
        }))
    };
    // Both presses of the double click reached the control under the pointer.
    let both_pressed = || crate::title_bar::presses() == PRESSES.with(Cell::get) + 2;
    // After a double click: waits as long as a zoom would take, then notes the size again.
    let settled = |waited: &'static str| {
        vec![
            Step::Setup(Box::new(|_, _, _| {
                SINCE.with(|s| s.set(Some(Instant::now())))
            })),
            Step::WaitFor(
                waited,
                Box::new(|_, _| SINCE.with(|s| s.get().is_some_and(|t| t.elapsed() >= SETTLE))),
            ),
            Step::Setup(Box::new(|_, window, _| {
                AFTER.with(|s| s.set(Some(window.viewport_size())))
            })),
        ]
    };
    let same = || {
        let (b, a) = (BEFORE.with(Cell::get), AFTER.with(Cell::get));
        b.is_some() && b == a
    };
    let mut steps = vec![
        Step::Setup(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        before(),
        click("run-widget", Click::Left(2)),
    ];
    steps.extend(settled(
        "the window had the time a zoom takes after two quick clicks on the run widget",
    ));
    steps.extend([
        Step::Expect(
            "two quick clicks on the run widget leave the window's size as it was",
            Box::new(move |_, _| same()),
        ),
        Step::Expect(
            "both of two quick clicks on the run widget reach the widget",
            Box::new(move |_, _| both_pressed()),
        ),
        before(),
        click("search-everywhere-button", Click::Left(2)),
    ]);
    steps.extend(settled(
        "the window had the time a zoom takes after two quick clicks on the search button",
    ));
    steps.extend([
        Step::Expect(
            "two quick clicks on the search button leave the window's size as it was",
            Box::new(move |_, _| same()),
        ),
        Step::Expect(
            "both of two quick clicks on the search button reach the button",
            Box::new(move |_, _| both_pressed()),
        ),
        Step::Key("escape", None),
        Step::WaitFor(
            "Escape closes Search Everywhere",
            Box::new(|b, _| b.everywhere().is_none()),
        ),
    ]);
    if !title_bar_double_click_zooms() {
        return steps;
    }
    // The empty part of the title bar: halfway between the widgets on the left and the run
    // widget. Not a control, so the spot is set from its neighbours.
    steps.extend([
        Step::Setup(Box::new(|_, _, _| {
            let get = idecraft_workbench::hit_targets::get;
            let left = get("branch-widget").or_else(|| get("project-widget"));
            if let (Some(left), Some(run)) = (left, get("run-widget")) {
                let x = (left.x + left.w + run.x) / 2.;
                idecraft_workbench::hit_targets::set(
                    "title-bar-empty",
                    idecraft_workbench::tool_windows::Rect {
                        x: x - 2.,
                        y: run.y + run.h / 2. - 2.,
                        w: 4.,
                        h: 4.,
                    },
                );
            }
        })),
        before(),
        click("title-bar-empty", Click::Left(2)),
    ]);
    steps.extend(settled(
        "the window had the time a zoom takes after a double click on the empty title bar",
    ));
    steps.extend([
        Step::Expect(
            "a double click on the empty title bar is no press on a control",
            Box::new(move |_, _| crate::title_bar::presses() == PRESSES.with(Cell::get)),
        ),
        // On macOS the toolbar lies in the system's title bar, whose double click zooms. Linux
        // draws the window manager's title bar above the toolbar (behaviour 61: "nothing changes
        // there"), so the Linux variant checks that the empty part of the toolbar is no title bar.
        if cfg!(target_os = "macos") {
            Step::Expect(
                "a double click on the empty title bar zooms the window",
                Box::new(move |_, _| {
                    let (b, a) = (BEFORE.with(Cell::get), AFTER.with(Cell::get));
                    b.is_some() && a.is_some() && b != a
                }),
            )
        } else {
            Step::Expect(
                "a double click on the empty part of the toolbar leaves the window's size (the window manager draws the title bar)",
                Box::new(move |_, _| same()),
            )
        },
        // And back: the second zoom restores the size the window had.
        click("title-bar-empty", Click::Left(2)),
    ]);
    steps.extend(settled(
        "the window had the time a zoom takes after the second double click on the empty title bar",
    ));
    steps.push(Step::Expect(
        "a second double click on the empty title bar gives the window its size back",
        Box::new(move |_, _| same()),
    ));
    steps
}

/// A click of `kind` on the control marked `target`.
fn click(target: impl Into<String>, kind: Click) -> Step {
    Step::Click(target.into(), kind)
}

/// A tool window's activation shortcut pressed while the editor has the focus (SPEC-0167 v3
/// behaviour 11): the first press moves the focus into the shown window and keeps it shown; only
/// the second, with the focus in the window, hides it.
fn shortcut_hides(
    key: &'static str,
    focuses: &'static str,
    hides: &'static str,
    shown: fn(&WorkbenchView) -> bool,
) -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        Step::Key(key, None),
        Step::Expect(focuses, Box::new(move |b, _| shown(b))),
        Step::Key(key, None),
        Step::Expect(hides, Box::new(move |b, _| !shown(b))),
    ]
}

/// Hides tool window `w` with its activation action as the user does (SPEC-0167 v3 behaviour 11):
/// a shown window without the focus takes the focus with the first activation and hides with the
/// second.
fn hide_tool_window(
    b: &mut WorkbenchView,
    w: idecraft_workbench::tool_windows::ToolWindow,
    window: &mut Window,
    cx: &mut Context<WorkbenchView>,
) {
    for _ in 0..2 {
        if b.tool_window_shown(w) {
            b.toggle_tool_window(w, window, cx);
        }
    }
}

/// Whether the first `rows` lines of a terminal's shown text contain `needle`, the lines joined and
/// blanks ignored: a prompt line wider than the terminal wraps onto the next rows (the user's prompt
/// and the window width vary), and each row loses its trailing blanks.
fn top_rows_contain(shown: &str, rows: usize, needle: &str) -> bool {
    let squeeze = |s: &str| s.split_whitespace().collect::<String>();
    squeeze(&shown.lines().take(rows).collect::<String>()).contains(&squeeze(needle))
}

/// The action of a click, run on the workbench.
type Act = Box<dyn Fn(&mut WorkbenchView, &mut Window, &mut Context<WorkbenchView>)>;

struct SelfTest {
    bench: Entity<WorkbenchView>,
    steps: Vec<Step>,
    /// Per group of the run: its name, the index after its last step, the number of its steps.
    spans: Vec<(&'static str, usize, usize)>,
    /// The group of `spans` the next step belongs to, and when it started.
    span_at: usize,
    span_since: Option<std::time::Instant>,
    /// How long the finished groups took.
    timings: Vec<crate::selftest_groups::Timing>,
    /// The groups `IDECRAFT_SELFTEST_SKIP` left out.
    skipped: Vec<&'static str>,
    next: usize,
    /// A scheduled keystroke has not been dispatched yet; other redraws must not advance.
    waiting: bool,
    /// When the current `WaitFor` step started waiting.
    since: Option<std::time::Instant>,
    failures: Vec<&'static str>,
    /// The UI driver for `Step::Click` and `Step::Hover` when `IDECRAFT_SELFTEST_DRIVER` is set
    /// (SPEC-0319 behaviour 42); the old path otherwise.
    driver: Option<SelfTestDriver>,
    /// `IDECRAFT_SELFTEST_HOLD_MS` (with `IDECRAFT_SELFTEST_HOLD`, a part of the step names): how
    /// long the window stays as it is after a passing wait, so a window check can look at it.
    hold: Option<(std::time::Duration, Option<String>)>,
    /// Since when the current passing wait holds.
    held: Option<std::time::Instant>,
}

/// The hold of a window check: `IDECRAFT_SELFTEST_HOLD_MS` milliseconds after each passing wait,
/// only for waits whose name contains one of the comma-separated parts of `IDECRAFT_SELFTEST_HOLD`
/// when that is set.
fn hold_from_env() -> Option<(std::time::Duration, Option<String>)> {
    let ms: u64 = std::env::var("IDECRAFT_SELFTEST_HOLD_MS")
        .ok()?
        .parse()
        .ok()?;
    let only = std::env::var("IDECRAFT_SELFTEST_HOLD")
        .ok()
        .filter(|s| !s.is_empty());
    (ms > 0).then(|| (std::time::Duration::from_millis(ms), only))
}

/// The UI driver of the self-test: its engine, the level it delivers on and the call waiting.
struct SelfTestDriver {
    engine: idecraft_uidriver::Engine,
    level: idecraft_uidriver::Level,
    op: Option<idecraft_uidriver::engine::OpId>,
}

impl SelfTestDriver {
    /// `IDECRAFT_SELFTEST_DRIVER=1` puts the native clicks of SPEC-0275 behind the driver (GPUI
    /// 0.2.2 takes window-level pointer events only from inside the toolkit, SPEC-0319 v2
    /// behaviour 20b). Unset: the old path.
    fn from_env() -> Option<Self> {
        let level = match std::env::var("IDECRAFT_SELFTEST_DRIVER").ok()?.as_str() {
            "" | "0" | "off" => return None,
            _ => idecraft_uidriver::Level::Native,
        };
        Some(Self {
            engine: idecraft_uidriver::Engine::new(),
            level,
            op: None,
        })
    }
}

/// The driver action of a click step.
fn click_action(kind: Click) -> idecraft_uidriver::Action {
    use idecraft_uidriver::{Action, Button};
    match kind {
        Click::Left(n) => Action::Click {
            button: Button::Left,
            count: u8::try_from(n).unwrap_or(1),
            modifiers: Vec::new(),
            position: None,
        },
        Click::Right => Action::ContextClick {
            position: None,
            control: false,
        },
        Click::ControlLeft => Action::ContextClick {
            position: None,
            control: true,
        },
    }
}

impl SelfTest {
    /// Records a failed check and prints what the workbench showed at that moment, so a failure
    /// that depends on timing can be told apart from a wrong result.
    fn fail(&mut self, what: &'static str, cx: &mut Context<Self>) {
        self.failures.push(what);
        let b = self.bench.read(cx);
        eprintln!(
            "self-test failed: {what}\n  problems (errors, warnings): {:?}\n  status: {:?}\n  text: {:?}\n  terminal: {:?}\n  painted: {:?}",
            b.active_problems(cx),
            b.status_message(cx),
            b.active_text(cx),
            b.terminal().map(|t| t.read(cx).shown_text()),
            b.painted_frame()
        );
        // The debugger's and the Containers window's state, to tell a product fault from a slow
        // adapter without a second run.
        if let Some(v) = b.debug_view() {
            let v = v.read(cx);
            eprintln!(
                "  debug: {:?}\n  variables: {:?}\n  results: {:?}\n  console tail: {:?}",
                v.state(),
                v.variable_texts(),
                v.result_texts(),
                v.console()
                    .chars()
                    .rev()
                    .take(600)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
        }
        if let Some(v) = b.containers_view() {
            eprintln!("  containers: {:?}", v.read(cx).texts());
        }
    }

    /// Every block of steps of a full run, in run order, with the names of its groups. Only the
    /// first block is built here; the others when a run takes one of their groups.
    fn blocks(layout: KeyLayout) -> Vec<Block> {
        // Platform-specific keys of the bundled keymaps.
        let (delete_line, close, project, next_tab, commit_window, commit_key) =
            if cfg!(target_os = "macos") {
                (
                    "cmd-backspace",
                    "cmd-w",
                    "cmd-1",
                    "cmd-shift-]",
                    "cmd-0",
                    "cmd-enter",
                )
            } else {
                (
                    "ctrl-y",
                    "ctrl-f4",
                    "alt-1",
                    "alt-right",
                    "alt-0",
                    "ctrl-enter",
                )
            };
        let git_window = if cfg!(target_os = "macos") {
            "cmd-9"
        } else {
            "alt-9"
        };
        let (run_key, choose_run, stop_key, run_window) = if cfg!(target_os = "macos") {
            ("ctrl-r", "ctrl-alt-r", "cmd-f2", "cmd-4")
        } else {
            ("shift-f10", "alt-shift-f10", "ctrl-f2", "alt-4")
        };
        // Recent Locations, Go to Symbol and Select All (SPEC-0069 v2, SPEC-0067 v5).
        let (recent_locations, goto_symbol, select_all) = if cfg!(target_os = "macos") {
            ("cmd-shift-e", "cmd-alt-o", "cmd-a")
        } else {
            ("ctrl-shift-e", "ctrl-alt-shift-n", "ctrl-a")
        };
        let (find, replace, find_in_files, goto_file, goto_action, save, goto_line, recent) =
            if cfg!(target_os = "macos") {
                (
                    "cmd-f",
                    "cmd-r",
                    "cmd-shift-f",
                    "cmd-shift-o",
                    "cmd-shift-a",
                    "cmd-s",
                    "cmd-l",
                    "cmd-e",
                )
            } else {
                (
                    "ctrl-f",
                    "ctrl-r",
                    "ctrl-shift-f",
                    "ctrl-shift-n",
                    "ctrl-shift-a",
                    "ctrl-s",
                    "ctrl-g",
                    "ctrl-e",
                )
            };
        let (declaration, back) = if cfg!(target_os = "macos") {
            let back: &'static str = match layout.open_bracket {
                Some(key) => Box::leak(format!("cmd-{key}").into_boxed_str()),
                None => "cmd-[",
            };
            ("cmd-b", back)
        } else {
            ("ctrl-b", "ctrl-alt-left")
        };
        let inline: Vec<Step> = {
            vec![
                // SPEC-0059 v5 behaviour 11: no tab yet, so the editor area lists the ways to open.
                Step::WaitFor(
                    "with no tab open the editor area lists the ways to open files",
                    Box::new(|b, _| {
                        b.tab_count() == 0
                            && idecraft_workbench::hit_targets::get("editor-empty").is_some()
                    }),
                ),
                // SPEC-0059 v6 behaviour 12: with the project tree shown the last line names the
                // navigation bar, not the project view.
                Step::Expect(
                    "with the project tree shown the empty editor area names the navigation bar",
                    Box::new(|b, _| {
                        let hints = b.empty_editor_hints();
                        !b.tool_window_shown(idecraft_workbench::tool_windows::ToolWindow::Project)
                            || hints.last().is_some_and(|(label, _)| *label == "Navigation Bar")
                    }),
                ),
                Step::WaitFor(
                    "changed files get their status colour, clean files none",
                    Box::new(|b, _| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.file_status_key(&dir.join("b.rs")) == Some("FILESTATUS_MODIFIED")
                            && b.file_status_key(&dir.join("new.txt")) == Some("FILESTATUS_UNKNOWN")
                            && b.file_status_key(&dir.join("a.rs")).is_none()
                    }),
                ),
                // Rows: the folder, `src`, `a.rs`, `b.rs`, `new.txt`.
                Step::Key("down", None),
                Step::Key("down", None),
                Step::Key("enter", None),
                Step::Expect(
                    "enter opens a.rs in a tab",
                    Box::new(|b, _| b.tab_count() == 1),
                ),
                // SPEC-0070 v4 behaviour 3: the problem indicator sits in the editor's corner.
                Step::WaitFor(
                    "an open file has the problem indicator at the editor's top right",
                    Box::new(|_, _| idecraft_workbench::hit_targets::get("problem-indicator").is_some()),
                ),
                // Back to the tree, open b.rs, then the next tab wraps around to a.rs.
                Step::Key(project, None),
                Step::Key("down", None),
                Step::Key("enter", None),
                Step::Expect(
                    "a second file opens in a second tab",
                    Box::new(|b, cx| {
                        b.tab_count() == 2
                            && b.active_text(cx).as_deref() == Some("fn c() { changed() }\n")
                    }),
                ),
                Step::WaitFor(
                    "the changed line of b.rs has a modified marker against HEAD",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.changes_of(&dir.join("b.rs"), cx).is_some_and(|c| {
                            c.len() == 1
                                && c[0].kind == idecraft_vcs::LineChangeKind::Modified
                                && (c[0].new_start, c[0].new_end) == (1, 1)
                        })
                    }),
                ),
                Step::Key(next_tab, None),
                Step::Expect(
                    "the next-tab shortcut wraps around to the first tab",
                    Box::new(|b, cx| {
                        b.active_text(cx).as_deref() == Some("fn a() {}\nfn b() {}\n")
                    }),
                ),
                // Find bar: typing searches, Enter and Shift+Enter step, Escape closes.
                Step::Key(find, None),
                Step::Key("f", Some("f")),
                Step::Key("n", Some("n")),
                Step::Expect(
                    "the find bar counts the matches of the typed text",
                    Box::new(|b, cx| b.find_label(cx).as_deref() == Some("1/2")),
                ),
                Step::Key("enter", None),
                Step::Expect(
                    "enter goes to the next match",
                    Box::new(|b, cx| b.find_label(cx).as_deref() == Some("2/2")),
                ),
                Step::Key("shift-enter", None),
                Step::Expect(
                    "shift+enter goes back and the text is unchanged",
                    Box::new(|b, cx| {
                        b.find_label(cx).as_deref() == Some("1/2")
                            && b.active_text(cx).as_deref() == Some("fn a() {}\nfn b() {}\n")
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the find bar",
                    Box::new(|b, cx| b.find_label(cx).is_none()),
                ),
                Step::Key("left", None),
                Step::Key("x", Some("x")),
                Step::Key("(", Some("(")),
                Step::Expect(
                    "typing goes into the editor (no closing bracket before a word)",
                    Box::new(|b, cx| {
                        b.active_text(cx).as_deref() == Some("x(fn a() {}\nfn b() {}\n")
                    }),
                ),
                Step::Key(delete_line, None),
                Step::Expect(
                    "the keymap deletes the line",
                    Box::new(|b, cx| b.active_text(cx).as_deref() == Some("fn b() {}\n")),
                ),
                Step::WaitFor(
                    "markers follow typing: the deleted first line of a.rs is marked",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.changes_of(&dir.join("a.rs"), cx).is_some_and(|c| {
                            c.len() == 1 && c[0].kind == idecraft_vcs::LineChangeKind::Deleted
                        })
                    }),
                ),
                // SPEC-0064 v3 AK-8: the change bar of the deletion marker, Rollback, undo.
                Step::Act(Box::new(|b, window, cx| b.show_change_bar(0, window, cx))),
                Step::Expect(
                    "a click on the deletion marker opens the change bar with the HEAD text",
                    Box::new(|b, cx| {
                        b.change_bar_index() == Some(0)
                            && b.change_bar_old_text(cx).as_deref() == Some("fn a() {}")
                    }),
                ),
                Step::Act(Box::new(|b, window, cx| {
                    b.change_bar_action("rollback", window, cx)
                })),
                Step::Expect(
                    "Rollback in the change bar restores the deleted line without saving",
                    Box::new(|b, cx| {
                        b.change_bar_index().is_none()
                            && b.active_text(cx).as_deref() == Some("fn a() {}\nfn b() {}\n")
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(e) = b.active_editor_entity() {
                        e.update(cx, |e, cx| e.run_action("$Undo", cx));
                    }
                })),
                Step::WaitFor(
                    "undo brings the deletion and its marker back",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.active_text(cx).as_deref() == Some("fn b() {}\n")
                            && b.changes_of(&dir.join("a.rs"), cx).is_some_and(|c| c.len() == 1)
                    }),
                ),
                // SPEC-0292 v4 AK-10: the author of the remaining function at its line end.
                Step::WaitFor(
                    "the function of a.rs shows its author at the end of its line",
                    Box::new(|b, cx| {
                        b.active_editor_entity().is_some_and(|e| {
                            e.read(cx)
                                .painted_author_hints()
                                .iter()
                                .any(|(line, text)| *line == 0 && text.starts_with("Self Test"))
                        })
                    }),
                ),
                // SPEC-0084 v2 AK-8: the diff keeps the tab row, U switches, » takes back.
                Step::Act(Box::new(|b, window, cx| {
                    b.menu_action("Compare.SameVersion", window, cx)
                })),
                Step::WaitFor(
                    "Show Diff opens the diff with a tab of its own in the tab row",
                    Box::new(|b, _| {
                        b.diff_view().is_some()
                            && idecraft_workbench::hit_targets::get("diff-tab").is_some()
                    }),
                ),
                Step::Key("u", Some("u")),
                Step::Expect(
                    "U switches the diff to the unified view",
                    Box::new(|b, cx| b.diff_view().is_some_and(|d| d.read(cx).unified())),
                ),
                Step::Key("u", Some("u")),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(d) = b.diff_view() {
                        d.update(cx, |d, cx| d.revert(0, cx));
                    }
                })),
                Step::WaitFor(
                    "» on the change puts the HEAD line back into the editor without saving",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.editor_for(&dir.join("a.rs"), cx).is_some_and(|e| {
                            e.read(cx).state.buffer.text() == "fn a() {}\nfn b() {}\n"
                        })
                    }),
                ),
                Step::Key("escape", None),
                Step::Act(Box::new(|b, window, cx| {
                    b.focus_editor(window, cx);
                    if let Some(e) = b.active_editor_entity() {
                        e.update(cx, |e, cx| e.run_action("$Undo", cx));
                    }
                })),
                Step::Expect(
                    "Escape closes the diff and undo takes the change back again",
                    Box::new(|b, cx| {
                        b.diff_view().is_none() && b.active_text(cx).as_deref() == Some("fn b() {}\n")
                    }),
                ),
                // SPEC-0112 v4 AK-5, SPEC-0070 v7 AK-7: the second view has the markers and
                // its own compact indicator.
                Step::Act(Box::new(|b, window, cx| b.split_editor(false, window, cx))),
                Step::WaitFor(
                    "after Split Right the second view of a.rs has the deletion marker too",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.split_decorations_of(&dir.join("a.rs"), cx)
                            .is_some_and(|(c, _)| c.len() == 1)
                            && idecraft_workbench::hit_targets::get("problem-indicator-compact-2")
                                .is_some()
                    }),
                ),
                Step::Act(Box::new(|b, window, cx| {
                    b.unsplit(window, cx);
                    b.focus_editor(window, cx);
                })),
                // Replace: query, Tab to the replacement, Enter replaces the current match.
                Step::Key(replace, None),
                Step::Key("b", Some("b")),
                Step::Expect(
                    "the replace bar finds the match",
                    Box::new(|b, cx| b.find_label(cx).as_deref() == Some("1/1")),
                ),
                Step::Key("tab", None),
                Step::Key("z", Some("z")),
                Step::Key("enter", None),
                Step::Expect(
                    "enter in the replacement field replaces the match",
                    Box::new(|b, cx| {
                        b.active_text(cx).as_deref() == Some("fn z() {}\n")
                            && b.find_label(cx).as_deref() == Some("0 results")
                    }),
                ),
                Step::Key("escape", None),
                Step::Key(close, None),
                Step::Expect(
                    "a modified tab survives the first close",
                    Box::new(|b, _| b.tab_count() == 2),
                ),
                Step::Key(close, None),
                Step::Expect(
                    "the second close closes it",
                    Box::new(|b, _| b.tab_count() == 1),
                ),
                group("terminal"),
                // Terminal tool window: a shell runs a command and its output appears.
                Step::Key("alt-f12", None),
                Step::Expect(
                    "the terminal tool window opens",
                    Box::new(|b, cx| b.terminal_text(cx).is_some()),
                ),
            ]
            .into_iter()
            .chain("echo idecraft-$((40+2))".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                let key: &'static str = if ch == ' ' { "space" } else { s };
                Step::Key(key, Some(s))
            }))
            .chain([
                Step::Key("enter", None),
                Step::WaitFor(
                    "the shell's output appears in the terminal",
                    Box::new(|b, cx| {
                        b.terminal_text(cx)
                            .is_some_and(|t| t.lines().any(|l| l.trim() == "idecraft-42"))
                    }),
                ),
                // Terminal tabs (SPEC-0062 v2): ⌘T opens a second shell, ⌘W closes it again.
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-t"
                    } else {
                        "ctrl-shift-t"
                    },
                    None,
                ),
                Step::Expect(
                    "a new terminal tab opens and is shown",
                    Box::new(|b, _| {
                        b.terminal_tabs() == ["Local", "Local (2)"] && b.terminal_tab() == 1
                    }),
                ),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-w"
                    } else {
                        "ctrl-shift-w"
                    },
                    None,
                ),
                Step::Expect(
                    "closing it shows the first shell with its output",
                    Box::new(|b, cx| {
                        b.terminal_tabs() == ["Local"]
                            && b.terminal_text(cx)
                                .is_some_and(|t| t.lines().any(|l| l.trim() == "idecraft-42"))
                    }),
                ),
                // Terminal mouse (SPEC-0118): the wheel scrolls back, a triple click selects a
                // line, ⌘C copies it, typing returns to the live screen; a program that asks
                // for the mouse gets SGR reports.
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, _| t.send_text("clear; seq 1 300\r"));
                    }
                })),
                Step::WaitFor(
                    "the long output fills the history",
                    Box::new(|b, cx| {
                        b.terminal_text(cx)
                            .is_some_and(|t| t.lines().any(|l| l.trim() == "300"))
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, cx| t.wheel(10, 0, 0, false, cx));
                    }
                })),
                Step::Expect(
                    "the wheel scrolls ten lines back into the history",
                    Box::new(|b, cx| {
                        b.terminal().is_some_and(|t| {
                            let t = t.read(cx);
                            t.scroll_offset() == 10
                                && !t.shown_text().lines().any(|l| l.trim() == "300")
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, cx| {
                            let none = (false, false, false);
                            t.press(0, 0, idecraft_workbench::TerminalMouseButton::Left, 3, none, cx);
                            t.release(0, 0, idecraft_workbench::TerminalMouseButton::Left, none, cx);
                        });
                    }
                })),
                Step::Expect(
                    "a triple click selects the top line shown",
                    Box::new(|b, cx| {
                        b.terminal().is_some_and(|t| {
                            let t = t.read(cx);
                            let top = t.shown_text().lines().next().unwrap_or_default().to_owned();
                            !top.is_empty() && t.selection_text() == Some(top)
                        })
                    }),
                ),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-c"
                    } else {
                        "ctrl-shift-c"
                    },
                    None,
                ),
                Step::Expect(
                    "copying puts the selected line on the clipboard",
                    Box::new(|b, cx| {
                        let clip = cx.read_from_clipboard().and_then(|c| c.text());
                        b.terminal()
                            .is_some_and(|t| clip.is_some() && t.read(cx).selection_text() == clip)
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, _| {
                            t.send_text("printf '\\033[?1000h\\033[?1006h'; cat -v\r")
                        });
                    }
                })),
                Step::WaitFor(
                    "typing returns to the live screen and the program's mouse mode is on",
                    Box::new(|b, cx| {
                        b.terminal().is_some_and(|t| {
                            let t = t.read(cx);
                            t.scroll_offset() == 0
                                && t.selection_text().is_none()
                                && t.mouse_mode() == idecraft_workbench::TerminalMouseMode::Press
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, cx| {
                            let none = (false, false, false);
                            t.press(2, 4, idecraft_workbench::TerminalMouseButton::Left, 1, none, cx);
                            t.release(2, 4, idecraft_workbench::TerminalMouseButton::Left, none, cx);
                            t.wheel(1, 2, 4, false, cx);
                        });
                    }
                })),
                Step::WaitFor(
                    "the program receives SGR press, release and wheel reports",
                    Box::new(|b, cx| {
                        b.terminal_text(cx).is_some_and(|t| {
                            t.contains("[<0;5;3M") && t.contains("[<0;5;3m") && t.contains("[<64;5;3M")
                        })
                    }),
                ),
                // ^C first, the next command only once the shell shows its prompt again: bash
                // (the usual login shell on Linux) drops what was typed while the program ended.
                Step::Key("ctrl-c", None),
                Step::WaitFor(
                    "^C ends the program and the shell prompts again",
                    Box::new(|b, cx| {
                        b.terminal_text(cx).is_some_and(|t| {
                            t.rfind("^C").is_some_and(|at| {
                                t[at..].trim_end().lines().count() >= 2
                            })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, _| {
                            t.send_text("printf '\\033[?1000l\\033[?1006l'; clear\r")
                        });
                    }
                })),
                Step::WaitFor(
                    "after the program the mouse selects again",
                    Box::new(|b, cx| {
                        b.terminal()
                            .is_some_and(|t| t.read(cx).mouse_mode() == idecraft_workbench::TerminalMouseMode::Off)
                    }),
                ),
                // Paste (D1): ⌘V and the menu bar's Paste send the clipboard to the shell.
                Step::Act(Box::new(|_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                        "echo pasted-$((2+3))\n".into(),
                    ));
                })),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-v"
                    } else {
                        "ctrl-shift-v"
                    },
                    None,
                ),
                // zsh turns bracketed paste on (mode 2004), so the pasted line, line end included,
                // waits on the prompt as in any terminal (SPEC-0118 behaviour 4); Return runs it.
                Step::WaitFor(
                    "⌘V pastes the clipboard into the shell",
                    Box::new(|b, cx| {
                        b.terminal_text(cx)
                            .is_some_and(|t| t.contains("echo pasted-$((2+3))"))
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, _| t.send_text("\r"));
                    }
                })),
                Step::WaitFor(
                    "Return runs the pasted command",
                    Box::new(|b, cx| {
                        b.terminal_text(cx)
                            .is_some_and(|t| t.lines().any(|l| l.trim() == "pasted-5"))
                    }),
                ),
                Step::Act(Box::new(|b, window, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                        "echo menu-$((3+4))\n".into(),
                    ));
                    b.menu_action("$Paste", window, cx);
                })),
                Step::WaitFor(
                    "the menu bar's Paste pastes into the shell with focus",
                    Box::new(|b, cx| {
                        b.terminal_text(cx)
                            .is_some_and(|t| t.contains("echo menu-$((3+4))"))
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, _| t.send_text("\r"));
                    }
                })),
                Step::WaitFor(
                    "Return runs the command pasted from the menu",
                    Box::new(|b, cx| {
                        b.terminal_text(cx)
                            .is_some_and(|t| t.lines().any(|l| l.trim() == "menu-7"))
                    }),
                ),
                // Terminal links (SPEC-0153): a file with line and column and a web address are
                // underlined under the mouse; opening the file link shows it at that place.
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, _| {
                            t.send_text("clear; echo 'see b.rs:1:4 and https://example.com/'\r")
                        });
                    }
                })),
                Step::WaitFor(
                    "the output with the links is shown",
                    Box::new(|b, cx| {
                        b.terminal()
                            .is_some_and(|t| t.read(cx).shown_text().lines().any(|l| l.starts_with("see ")))
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, cx| {
                            if let Some(row) = t.shown_text().lines().position(|l| l.starts_with("see ")) {
                                t.hover(row, 5, cx);
                            }
                        });
                    }
                })),
                Step::Expect(
                    "the mouse over b.rs:1:4 underlines the file link",
                    Box::new(|b, cx| {
                        b.terminal().is_some_and(|t| {
                            t.read(cx).hovered_link().is_some_and(|l| {
                                (l.start, l.end) == (4, 12)
                                    && l.target
                                        == idecraft_workbench::TerminalLinkTarget::File {
                                            path: "b.rs".into(),
                                            line: Some(1),
                                            column: Some(4),
                                        }
                            })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, cx| {
                            if let Some(row) = t.shown_text().lines().position(|l| l.starts_with("see ")) {
                                t.hover(row, 20, cx);
                            }
                        });
                    }
                })),
                Step::Expect(
                    "the mouse over the address underlines the web link",
                    Box::new(|b, cx| {
                        b.terminal().is_some_and(|t| {
                            t.read(cx).hovered_link().is_some_and(|l| {
                                l.target
                                    == idecraft_workbench::TerminalLinkTarget::Url {
                                        url: "https://example.com/".into(),
                                    }
                            })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(t) = b.terminal() {
                        t.update(cx, |t, cx| {
                            if let Some(row) = t.shown_text().lines().position(|l| l.starts_with("see ")) {
                                t.open_link(row, 5, cx);
                            }
                        });
                    }
                })),
                Step::WaitFor(
                    "opening the file link shows b.rs with the caret on line 1, column 4",
                    Box::new(|b, cx| {
                        b.active_text(cx).is_some_and(|t| t.starts_with("fn c()"))
                            && b.active_editor_entity().is_some_and(|e| {
                                let s = e.read(cx).status();
                                (s.line, s.column) == (1, 4)
                            })
                    }),
                ),
                group("commit"),
                // Commit tool window: the changed file is checked, the new one is not.
                Step::Key(commit_window, None),
                Step::Expect(
                    "the commit window lists the changes with only tracked files checked",
                    Box::new(|b, cx| {
                        b.commit_view()
                            .is_some_and(|v| v.read(cx).selected() == ["b.rs"])
                    }),
                ),
            ])
            .chain("selftest commit".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                let key: &'static str = if ch == ' ' { "space" } else { s };
                Step::Key(key, Some(s))
            }))
            .chain([
                Step::Key(commit_key, None),
                Step::WaitFor(
                    "the commit is in the repository with the typed message",
                    Box::new(|_, _| {
                        FIXTURE.get().is_some_and(|dir| {
                            git(dir, &["log", "-1", "--format=%s"])
                                .is_ok_and(|s| s == "selftest commit")
                                && git(dir, &["status", "--porcelain"]).is_ok_and(|s| {
                                    // `.idecraft/` holds IDECraft's own state (recent files).
                                    s.lines()
                                        .filter(|l| *l != "?? .idecraft/")
                                        .eq(["?? new.txt"])
                                })
                        })
                    }),
                ),
                Step::WaitFor(
                    "the committed file loses its status colour",
                    Box::new(|b, _| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.file_status_key(&dir.join("b.rs")).is_none()
                            && b.file_status_key(&dir.join("new.txt")).is_some()
                    }),
                ),
                group("search"),
                // Find in Files: matches of the workspace, Down and Enter open one.
                Step::Key(find_in_files, None),
                Step::Key("f", Some("f")),
                Step::Key("n", Some("n")),
                Step::WaitFor(
                    "find in files lists the matches of the workspace",
                    Box::new(|b, cx| {
                        b.find_in_files()
                            .is_some_and(|v| v.read(cx).summary() == "3 matches in 2 files")
                    }),
                ),
                Step::Key("down", None),
                Step::Key("enter", None),
                Step::Expect(
                    "enter opens the selected match",
                    Box::new(|b, cx| {
                        b.active_text(cx).as_deref() == Some("fn a() {}\nfn b() {}\n")
                    }),
                ),
                // Search Everywhere: Go to File finds a file by name, Go to Action runs an action.
                Step::Key(goto_file, None),
                Step::Key("n", Some("n")),
                Step::Key("e", Some("e")),
                Step::Key("w", Some("w")),
                Step::WaitFor(
                    "go to file lists the file first",
                    Box::new(|b, cx| {
                        b.everywhere().is_some_and(|v| {
                            matches!(
                                v.read(cx).results().selected_row(),
                                Some(idecraft_workbench::everywhere_model::Row::File(f))
                                    if f.path == "new.txt"
                            )
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::Expect(
                    "enter opens the file and closes the popup",
                    Box::new(|b, cx| {
                        b.active_text(cx).as_deref() == Some("untracked\n")
                            && b.everywhere().is_none()
                    }),
                ),
                Step::Key(goto_action, None),
            ])
            .chain("CloseContent".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                Step::Key(s, Some(s))
            }))
            .chain([
                Step::WaitFor(
                    "go to action finds the action by id",
                    Box::new(|b, cx| {
                        b.everywhere().is_some_and(|v| {
                            matches!(
                                v.read(cx).results().selected_row(),
                                Some(idecraft_workbench::everywhere_model::Row::Action(a))
                                    if a.id == "CloseContent"
                            )
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::Expect(
                    "enter runs the action: the file's tab is closed",
                    Box::new(|b, cx| {
                        b.everywhere().is_none()
                            && b.active_text(cx).as_deref() != Some("untracked\n")
                    }),
                ),
                group("disk-changes"),
                // Saving and changes on disk: another program edits the open, unmodified a.rs.
                Step::Expect(
                    "a.rs is the active, unmodified editor; another program rewrites it",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.active_text(cx).as_deref() == Some("fn a() {}\nfn b() {}\n")
                            && std::fs::write(dir.join("a.rs"), "fn outside() {}\n").is_ok()
                    }),
                ),
                Step::WaitFor(
                    "the unmodified editor reloads the change from disk",
                    Box::new(|b, cx| b.active_text(cx).as_deref() == Some("fn outside() {}\n")),
                ),
                Step::Key("x", Some("x")),
                Step::Expect(
                    "with unsaved changes, another program rewrites the file again",
                    Box::new(|_, _| {
                        let dir = FIXTURE.get().expect("fixture");
                        std::fs::write(dir.join("a.rs"), "fn again() {}\n").is_ok()
                    }),
                ),
                Step::WaitFor(
                    "the modified editor keeps its text and warns",
                    Box::new(|b, cx| {
                        b.active_text(cx)
                            .is_some_and(|t| t.contains('x') && t.contains("outside"))
                            && b.active_note(cx)
                                .is_some_and(|n| n.contains("changed on disk and here"))
                    }),
                ),
                Step::Key(save, None),
                Step::Expect(
                    "saving writes the editor's text and records the local history",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        std::fs::read_to_string(dir.join("a.rs")).ok() == b.active_text(cx)
                            && std::fs::read_to_string(
                                dir.join(".idecraft/history/revisions.jsonl"),
                            )
                            .is_ok_and(|log| {
                                log.lines().filter(|l| l.contains("a.rs")).count() >= 2
                            })
                    }),
                ),
                group("branches"),
                // Branch popup: a click on the branch widget, a new branch, back to main.
                Step::Act(Box::new(|b, window, cx| b.open_branches(window, cx))),
                Step::Expect(
                    "the branch popup selects the current branch",
                    Box::new(|b, cx| {
                        b.branches_popup().is_some_and(|v| {
                            matches!(
                                v.read(cx).list().selected_row(),
                                Some(idecraft_workbench::branches_model::BranchRow::Branch(br))
                                    if br.name == "main" && br.current
                            )
                        })
                    }),
                ),
            ])
            .chain("topic".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                Step::Key(s, Some(s))
            }))
            .chain([
                Step::Key("enter", None),
                Step::Expect(
                    "enter on a new name creates the branch and checks it out",
                    Box::new(|b, _| {
                        b.branch().as_deref() == Some("topic") && b.branches_popup().is_none()
                    }),
                ),
                Step::Act(Box::new(|b, window, cx| b.open_branches(window, cx))),
            ])
            .chain("main".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                Step::Key(s, Some(s))
            }))
            .chain([
                Step::Key("enter", None),
                Step::Expect(
                    "enter on an existing branch checks it out",
                    Box::new(|b, _| b.branch().as_deref() == Some("main")),
                ),
                // "New Branch…" at the top asks for the name (SPEC-0068 v2; hand test 2026-11-01).
                // SPEC-0068 v5: the actions come first, "New Branch…" among them.
                Step::Act(Box::new(|b, window, cx| {
                    b.open_branches(window, cx);
                    if let Some(v) = b.branches_popup() {
                        v.update(cx, |v, cx| {
                            v.select_row(
                                &idecraft_workbench::branches_model::BranchRow::New(String::new()),
                                cx,
                            )
                        });
                    }
                })),
                Step::Key("enter", None),
                Step::Expect(
                    "new branch… switches the field to the name of the new branch",
                    Box::new(|b, cx| b.branches_popup().is_some_and(|v| v.read(cx).naming())),
                ),
            ])
            .chain("named".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                Step::Key(s, Some(s))
            }))
            .chain([
                Step::Key("enter", None),
                Step::Expect(
                    "enter creates the named branch and checks it out",
                    Box::new(|b, _| b.branch().as_deref() == Some("named")),
                ),
                Step::Act(Box::new(|b, window, cx| b.open_branches(window, cx))),
            ])
            .chain("main".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                Step::Key(s, Some(s))
            }))
            .chain([
                Step::Key("enter", None),
                Step::Expect(
                    "and back to main",
                    Box::new(|b, _| b.branch().as_deref() == Some("main")),
                ),
                // SPEC-0068 v5 AK-10: actions, Recent, the submenu (→) and Delete from it.
                Step::Act(Box::new(|b, window, cx| b.open_branches(window, cx))),
                Step::Expect(
                    "the popup starts with Update, Commit, Push and lists the recent branches",
                    Box::new(|b, cx| {
                        use idecraft_workbench::branches_model::{BranchRow, PopupAction};
                        b.branches_popup().is_some_and(|v| {
                            let list = v.read(cx).list();
                            let recent = list.position(&BranchRow::Header("Recent"));
                            list.rows.first() == Some(&BranchRow::Action(PopupAction::Update))
                                && recent.is_some_and(|i| {
                                    matches!(list.rows.get(i + 1),
                                        Some(BranchRow::Branch(b)) if b.name == "named")
                                })
                        })
                    }),
                ),
                Step::Key("right", None),
                Step::Expect(
                    "→ on the current branch opens its submenu",
                    Box::new(|b, cx| {
                        b.branches_popup().is_some_and(|v| {
                            v.read(cx).submenu().is_some_and(|m| {
                                m.first().map(String::as_str) == Some("New Branch from 'main'…")
                            })
                        })
                    }),
                ),
                Step::Key("escape", None),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(v) = b.branches_popup() {
                        v.update(cx, |v, cx| {
                            v.select_branch("topic", cx);
                            v.open_submenu(cx);
                            // Delete is the last entry (v6 added Compare with, Show Diff with
                            // Working Tree and Push… before Rename); found by its label.
                            let delete = v
                                .submenu()
                                .and_then(|m| m.iter().position(|e| e == "Delete"))
                                .unwrap_or(usize::MAX);
                            v.choose_submenu(delete, cx);
                        });
                    }
                })),
                Step::WaitFor(
                    "Delete in the submenu deletes the merged branch topic",
                    Box::new(|b, _| b.status_note() == Some("Delete branch topic done")),
                ),
                // Go to Line and Recent Files.
                Step::Key(goto_line, None),
                Step::Key("1", Some("1")),
                Step::Key(",", Some(",")),
                Step::Key("4", Some("4")),
                Step::Key("enter", None),
                Step::Expect(
                    "go to line puts the caret at line 1, column 4",
                    Box::new(|b, cx| {
                        b.caret_position(cx) == Some((1, 4)) && b.goto_popup().is_none()
                    }),
                ),
                Step::Key(recent, None),
                Step::Expect(
                    "recent files lists the current file first and selects the previous one",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.goto_popup().is_some_and(|v| {
                            let v = v.read(cx);
                            let shown = v.shown();
                            shown.len() >= 2
                                && shown.first() == Some(&dir.join("a.rs"))
                                && v.selected() == 1
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::Expect(
                    "enter opens the previous file and the list is kept on disk",
                    Box::new(|b, cx| {
                        let dir = FIXTURE.get().expect("fixture");
                        b.active_text(cx).as_deref() == Some("untracked\n")
                            && std::fs::read_to_string(dir.join(".idecraft/recent-files"))
                                .is_ok_and(|t| t.lines().next() == Some("new.txt"))
                    }),
                ),
                // SPEC-0068 v8 AK-17: Space marks the highlighted branch as a favourite, its row
                // moves up with the star, the popup stays open; the submenu unmarks it again. Here,
                // after Recent Files, `.idecraft` exists already, so the project tree is unchanged.
                Step::Act(Box::new(|b, window, cx| {
                    b.open_branches(window, cx);
                    if let Some(v) = b.branches_popup() {
                        v.update(cx, |v, cx| v.select_branch("named", cx));
                    }
                })),
                Step::Key("space", Some(" ")),
                Step::Expect(
                    "space marks the branch as a favourite and keeps the popup open",
                    Box::new(|b, cx| {
                        use idecraft_workbench::branches_model::BranchRow;
                        let dir = FIXTURE.get().expect("fixture");
                        b.branches_popup().is_some_and(|v| {
                            let v = v.read(cx);
                            let list = v.list();
                            let local = list.position(&BranchRow::Header("Local"));
                            v.is_favorite("named", false)
                                && matches!(list.selected_row(),
                                    Some(BranchRow::Branch(br)) if br.name == "named")
                                && local.is_some_and(|i| {
                                    // main is current and a favourite; named follows it.
                                    matches!(list.rows.get(i + 2),
                                        Some(BranchRow::Branch(br)) if br.name == "named")
                                })
                        }) && std::fs::read_to_string(dir.join(".idecraft/branch-favorites.toml"))
                            .is_ok_and(|t| t.contains("\"named\""))
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(v) = b.branches_popup() {
                        v.update(cx, |v, cx| {
                            v.open_submenu(cx);
                            let last = v.submenu().map_or(0, |m| m.len().saturating_sub(1));
                            v.choose_submenu(last, cx);
                        });
                    }
                })),
                Step::Expect(
                    "Unmark as Favorite in the submenu unmarks it, the popup stays open",
                    Box::new(|b, cx| {
                        b.branches_popup().is_some_and(|v| {
                            let v = v.read(cx);
                            !v.is_favorite("named", false) && v.submenu().is_none()
                        })
                    }),
                ),
                Step::Key("escape", None),
                group("code-insight"),
                // Diagnostics: clangd reports the undeclared identifier in bad.c.
                Step::Key(goto_file, None),
                Step::Key("b", Some("b")),
                Step::Key("a", Some("a")),
                Step::Key("d", Some("d")),
                Step::WaitFor(
                    "go to file selects bad.c",
                    Box::new(|b, cx| {
                        b.everywhere().is_some_and(|v| {
                            matches!(
                                v.read(cx).results().selected_row(),
                                Some(idecraft_workbench::everywhere_model::Row::File(f))
                                    if f.path == "bad.c"
                            )
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::WaitFor(
                    "the language server's error appears in the editor",
                    Box::new(|b, cx| b.active_problems(cx).is_some_and(|(errors, _)| errors >= 1)),
                ),
                // Go to Declaration of `helper` and Back.
                Step::Key(goto_line, None),
                Step::Key("2", Some("2")),
                Step::Key(",", Some(",")),
                Step::Key("2", Some("2")),
                Step::Key("8", Some("8")),
                Step::Key("enter", None),
                Step::Key(declaration, None),
                Step::WaitFor(
                    "go to declaration jumps to the definition of helper",
                    Box::new(|b, cx| b.caret_position(cx) == Some((1, 5))),
                ),
                Step::Key(back, None),
                Step::Expect(
                    "back returns to the call",
                    Box::new(|b, cx| b.caret_position(cx) == Some((2, 28))),
                ),
                // SPEC-0069 v2 AK-4: Recent Locations lists the places left in other files.
                Step::Key(recent_locations, None),
                Step::WaitFor(
                    "recent locations lists a place of another file",
                    Box::new(|b, cx| {
                        b.goto_popup().is_some_and(|v| {
                            let v = v.read(cx);
                            let places = v.places();
                            !places.is_empty()
                                && places.iter().all(|p| !p.place.path.ends_with("bad.c")
                                    || p.place.line.abs_diff(1) > 3)
                                && places.iter().all(|p| !p.lines.is_empty())
                        })
                    }),
                ),
                Step::Key("escape", None),
                // SPEC-0067 v5 AK-6: Symbols with `name:line`, the recent files of an empty query,
                // the preview and the non-project toggle.
                Step::Key(goto_symbol, None),
            ])
            .chain("helper:1".chars().map(|ch| {
                let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                Step::Key(s, Some(s))
            }))
            .chain([
                Step::WaitFor(
                    "go to symbol finds helper and keeps the line",
                    Box::new(|b, cx| {
                        b.everywhere().is_some_and(|v| {
                            matches!(
                                v.read(cx).results().selected_row(),
                                Some(idecraft_workbench::everywhere_model::Row::Symbol(s))
                                    if s.name == "helper"
                            )
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::Expect(
                    "enter opens the symbol's file at the line asked for",
                    Box::new(|b, cx| {
                        b.everywhere().is_none() && b.caret_position(cx).map(|p| p.0) == Some(1)
                    }),
                ),
                Step::Key(goto_file, None),
                Step::Key(select_all, None),
                Step::Key("backspace", None),
                Step::WaitFor(
                    "an empty query lists the recent files without the current one",
                    Box::new(|b, cx| {
                        b.everywhere().is_some_and(|v| {
                            let rows = &v.read(cx).results().rows;
                            !rows.is_empty()
                                && rows.iter().all(|r| match r {
                                    idecraft_workbench::everywhere_model::Row::File(f) => {
                                        f.path != "bad.c"
                                    }
                                    _ => false,
                                })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(v) = b.everywhere() {
                        v.update(cx, |v, cx| v.set_preview(true, cx));
                    }
                })),
                Step::Expect(
                    "the preview shows the selected file's lines",
                    Box::new(|b, cx| {
                        b.everywhere().is_some_and(|v| {
                            let v = v.read(cx);
                            v.previewing() && !v.footer().is_empty()
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(v) = b.everywhere() {
                        v.update(cx, |v, cx| v.set_preview(false, cx));
                    }
                })),
                Step::Key(goto_file, None),
                Step::Expect(
                    "the tab's shortcut again includes non-project items",
                    Box::new(|b, cx| b.everywhere().is_some_and(|v| v.read(cx).everything())),
                ),
                Step::Key("escape", None),
                Step::Key(goto_file, None),
                Step::Expect(
                    "a new opening starts with the project scope (IMP-0333)",
                    Box::new(|b, cx| b.everywhere().is_some_and(|v| !v.read(cx).everything())),
                ),
                Step::Key("escape", None),
                // Back to the call of `helper` that Go to Symbol left.
                Step::Key(back, None),
                Step::Expect(
                    "back returns to the call after go to symbol",
                    Box::new(|b, cx| b.caret_position(cx) == Some((2, 28))),
                ),
                // SPEC-0293 AK-5: Find Usages of `helper` opens the Find tool window with the call
                // grouped under its file; the next usage opens; ⇧Esc hides the window.
                Step::Key("alt-f7", None),
                Step::WaitFor(
                    "find usages opens the find tool window with the call of helper",
                    Box::new(|b, cx| {
                        b.find_tool_window().is_some_and(|v| {
                            let v = v.read(cx);
                            v.title() == "helper in Project Files"
                                && v.tree().is_some_and(|t| {
                                    t.file_count() == 1
                                        && t.hits.iter().map(|h| h.place.line).collect::<Vec<_>>()
                                            == [1]
                                })
                        })
                    }),
                ),
                // SPEC-0293 v3 AK-8: grouped by usage kind, module, file and function.
                Step::Expect(
                    "the tree shows target, group, kind, module, file, function and usage",
                    Box::new(|b, cx| {
                        b.find_tool_window().is_some_and(|v| {
                            let rows = v.read(cx).visible();
                            let dir = FIXTURE.get().expect("fixture");
                            let module = dir
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            rows.len() == 7
                                && rows[0].1 == "helper"
                                && rows[1].1 == "Usages in Project Files"
                                && rows[2].1 == "Calls"
                                && rows[3].1 == module
                                && rows[4].1 == "bad.c"
                                && rows[5].1 == "main"
                                && rows[6].1.starts_with("2  ")
                        })
                    }),
                ),
                Step::Key(if cfg!(target_os = "macos") { "cmd-alt-down" } else { "ctrl-alt-down" }, None),
                Step::WaitFor(
                    "next usage opens the call of helper",
                    Box::new(|b, cx| b.caret_position(cx).is_some_and(|(line, _)| line == 2)),
                ),
                Step::Key("shift-escape", None),
                Step::Expect(
                    "shift+escape hides the find tool window",
                    Box::new(|b, _| b.find_tool_window().is_none()),
                ),
                // SPEC-0073 v5 AK-6: Show Usages leaves the declaration out; the call is the only
                // usage, so on it a hint opens instead of a popup.
                Step::Key(if cfg!(target_os = "macos") { "cmd-alt-f7" } else { "ctrl-alt-f7" }, None),
                Step::WaitFor(
                    "show usages on the only usage shows a hint and no popup",
                    Box::new(|b, _| {
                        b.usages_popup().is_none()
                            && b.doc_blocks().is_some_and(|blocks| {
                                blocks.iter().any(|block| {
                                    matches!(block, DocBlock::Text(t, _) if t.contains("no other usage"))
                                })
                            })
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the hint",
                    Box::new(|b, _| b.doc_blocks().is_none() && b.usages_popup().is_none()),
                ),
                // From the definition it jumps to the only usage and says so.
                Step::Key(goto_line, None),
                Step::Key("1", Some("1")),
                Step::Key(",", Some(",")),
                Step::Key("6", Some("6")),
                Step::Key("enter", None),
                Step::Key(if cfg!(target_os = "macos") { "cmd-alt-f7" } else { "ctrl-alt-f7" }, None),
                Step::WaitFor(
                    "show usages on the definition jumps to the only usage",
                    Box::new(|b, cx| {
                        b.usages_popup().is_none()
                            && b.caret_position(cx).is_some_and(|(line, _)| line == 2)
                            && b.doc_blocks().is_some_and(|blocks| {
                                blocks.iter().any(|block| {
                                    matches!(block, DocBlock::Text(t, _) if t.starts_with("Only usage of helper"))
                                })
                            })
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the hint after the jump",
                    Box::new(|b, _| b.doc_blocks().is_none()),
                ),
                // Back to `he|lper` in the call for the completion steps.
                Step::Key(goto_line, None),
                Step::Key("2", Some("2")),
                Step::Key(",", Some(",")),
                Step::Key("2", Some("2")),
                Step::Key("8", Some("8")),
                Step::Key("enter", None),
                Step::WaitFor(
                    "the caret is back in the call",
                    Box::new(|b, cx| b.caret_position(cx) == Some((2, 28))),
                ),
                // Completion on the identifier at the caret (`he|lper`), Escape, then Enter accepts.
                Step::Key("ctrl-space", None),
                Step::WaitFor(
                    "ctrl+space lists completions for the typed prefix",
                    Box::new(|b, _| {
                        b.completion_items()
                            .is_some_and(|items| items.iter().any(|i| i.starts_with("helper")))
                    }),
                ),
                // SPEC-0072 v5 behaviour 14: the hint line names the keys that accept.
                Step::Expect(
                    "the completion list shows the hint line",
                    Box::new(|b, _| {
                        b.completion_extras()
                            .is_some_and(|(hint, _)| hint.contains("insert") && hint.contains("replace"))
                    }),
                ),
                // SPEC-0072 v4 behaviour 9: every detail stays inside its one-line row.
                Step::WaitFor(
                    "each completion detail lies inside its row",
                    Box::new(|_, _| {
                        use idecraft_workbench::hit_targets::get;
                        get("completion-row-0").is_some()
                            && (0..10).all(|i| {
                                match (
                                    get(&format!("completion-row-{i}")),
                                    get(&format!("completion-detail-{i}")),
                                ) {
                                    (Some(row), Some(d)) => {
                                        d.y >= row.y - 0.5
                                            && d.y + d.h <= row.y + row.h + 0.5
                                            && d.x + d.w <= row.x + row.w + 0.5
                                    }
                                    (_, None) => true,
                                    (None, Some(_)) => false,
                                }
                            })
                    }),
                ),
                // SPEC-0072 v6 behaviour 16: the list and its documentation panel stay inside the
                // window (the status bar spans its width).
                Step::WaitFor(
                    "the completion list and its documentation lie inside the window",
                    Box::new(|b, _| {
                        use idecraft_workbench::hit_targets::get;
                        let Some(status) = b
                            .painted_frame()
                            .iter()
                            .find(|(n, _)| *n == "status")
                            .map(|(_, r)| *r)
                        else {
                            return false;
                        };
                        let right = status.x + status.w;
                        let inside = |r: idecraft_ui::tool_windows::Rect| {
                            r.x >= status.x - 0.5 && r.x + r.w <= right + 0.5
                        };
                        get("completion-list").is_some_and(inside)
                            && get("completion-doc").is_none_or(inside)
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the list and leaves the text",
                    Box::new(|b, cx| {
                        b.completion_items().is_none()
                            && b.active_text(cx)
                                .is_some_and(|t| t.contains("int a = helper();"))
                    }),
                ),
                // On macOS also ⌥Esc, which ⌃Space-swallowing input source switching leaves alone.
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "alt-escape"
                    } else {
                        "ctrl-space"
                    },
                    None,
                ),
                Step::WaitFor(
                    "the list opens again",
                    Box::new(|b, _| b.completion_items().is_some()),
                ),
                Step::Key("enter", None),
                // With snippets on (SPEC-0072 v7) clangd inserts the call `helper()` and the caret
                // goes after the parentheses of a function without parameters.
                Step::Expect(
                    "enter replaces the typed prefix with the suggestion",
                    Box::new(|b, cx| {
                        b.completion_items().is_none()
                            && b.active_text(cx).is_some_and(|t| {
                                // `he` became the suggestion; the rest of the old word stays.
                                t.contains("= helper()lper();")
                            })
                    }),
                ),
                // Back to `helperlper();`, the text the later steps expect.
                Step::Key("backspace", None),
                Step::Key("backspace", None),
                // Typing an identifier opens the list by itself (SPEC-0072 v3): delete the last
                // letter of `helper` and type it again; Escape leaves the text as it was.
                Step::Key("backspace", None),
                Step::Key("r", Some("r")),
                Step::WaitFor(
                    "typing opens the completion list without ctrl+space",
                    Box::new(|b, _| {
                        b.completion_items()
                            .is_some_and(|items| items.iter().any(|i| i.starts_with("helper")))
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape leaves the retyped text as before",
                    Box::new(|b, cx| {
                        b.completion_items().is_none()
                            && b.active_text(cx)
                                .is_some_and(|t| t.contains("= helperlper();"))
                    }),
                ),
                // Rename `helper` at its definition to `calc`.
                Step::Key(goto_line, None),
                Step::Key("1", Some("1")),
                Step::Key(",", Some(",")),
                Step::Key("7", Some("7")),
                Step::Key("enter", None),
                Step::Key("shift-f6", None),
                Step::Key("c", Some("c")),
                Step::Key("a", Some("a")),
                Step::Key("l", Some("l")),
                Step::Key("c", Some("c")),
                Step::Key("enter", None),
                Step::WaitFor(
                    "rename changes the definition in the editor",
                    Box::new(|b, cx| {
                        b.active_text(cx)
                            .is_some_and(|t| t.starts_with("int calc(void)"))
                    }),
                ),
                // Reformat Code: clangd breaks the two statements of `main` into lines.
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-alt-l"
                    } else {
                        "ctrl-alt-l"
                    },
                    None,
                ),
                Step::WaitFor(
                    "reformat code lays out main over several lines",
                    Box::new(|b, cx| {
                        b.active_text(cx)
                            .is_some_and(|t| t.contains("int main(void) {\n"))
                    }),
                ),
                // Context actions: remove the `;` after the call, Alt+Enter on `return` offers the
                // server's fix, Enter applies it.
                Step::Key(goto_line, None),
                Step::Key("3", Some("3")),
                Step::Key(",", Some(",")),
                Step::Key("2", Some("2")),
                Step::Key("4", Some("4")),
                Step::Key("enter", None),
                Step::Key("backspace", None),
                Step::Key(goto_line, None),
                Step::Key("4", Some("4")),
                Step::Key(",", Some(",")),
                Step::Key("3", Some("3")),
                Step::Key("enter", None),
                Step::Key("alt-enter", None),
                Step::WaitFor(
                    "alt+enter offers the fix for the missing semicolon",
                    Box::new(|b, cx| {
                        b.intentions_popup()
                            .is_some_and(|v| v.read(cx).titles().iter().any(|t| t.contains(';')))
                    }),
                ),
                // SPEC-0076 v3 AK-5: the Quick Documentation key previews the selected fix.
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "f1"
                    } else {
                        "ctrl-q"
                    },
                    None,
                ),
                Step::WaitFor(
                    "the preview shows the line with the added semicolon",
                    Box::new(|b, cx| {
                        b.intentions_popup().is_some_and(|v| {
                            v.read(cx).preview_lines().is_some_and(|r| {
                                r.is_ok_and(|lines| {
                                    lines.iter().any(|l| l.starts_with("+ ") && l.ends_with(';'))
                                })
                            })
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::WaitFor(
                    "enter applies the fix in the editor",
                    Box::new(|b, cx| {
                        b.intentions_popup().is_none()
                            && b.active_text(cx)
                                .is_some_and(|t| t.contains("helperlper();\n  return"))
                    }),
                ),
                // Quick Documentation of `calc` at its definition; Escape closes it.
                Step::Key(goto_line, None),
                Step::Key("1", Some("1")),
                Step::Key(",", Some(",")),
                Step::Key("6", Some("6")),
                Step::Key("enter", None),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "f1"
                    } else {
                        "ctrl-q"
                    },
                    None,
                ),
                Step::WaitFor(
                    "quick documentation shows the signature of calc",
                    Box::new(|b, _| {
                        b.doc_blocks().is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                matches!(block, DocBlock::Code(t) if t.contains("int calc(void)"))
                            })
                        })
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the documentation and keeps the caret",
                    Box::new(|b, cx| {
                        b.doc_blocks().is_none() && b.caret_position(cx) == Some((1, 6))
                    }),
                ),
                // Documentation on mouse hover (SPEC-0077 v6 AK-11): the pointer rests on `calc`,
                // then on empty space beside the line, then leaves the editor.
                Step::Setup(Box::new(|b, _, cx| {
                    if let Some(e) = b.active_editor_entity() {
                        let e = e.read(cx);
                        let head = e.state.carets.primary().head;
                        if let Some(p) = e.point_on_char(head) {
                            let (x, y) = (f32::from(p.x), f32::from(p.y));
                            let spot = |x| idecraft_ui::tool_windows::Rect { x, y, w: 1., h: 1. };
                            idecraft_workbench::hit_targets::set("doc-hover-word", spot(x));
                            idecraft_workbench::hit_targets::set("doc-hover-space", spot(x + 400.));
                        }
                    }
                })),
                hover("doc-hover-word"),
                Step::WaitFor(
                    "resting the pointer on calc shows its signature",
                    Box::new(|b, _| {
                        b.doc_hover_shown()
                            && b.doc_blocks().is_some_and(|blocks| {
                                blocks.iter().any(|block| {
                                    matches!(block, DocBlock::Code(t) if t.contains("int calc(void)"))
                                })
                            })
                    }),
                ),
                hover("doc-hover-space"),
                Step::WaitFor(
                    "moving the pointer off the word closes the documentation",
                    Box::new(|b, _| b.doc_blocks().is_none()),
                ),
                hover("project-widget"),
                // F2 jumps from the top to the first remaining error and shows its message, once
                // clangd has answered for the fixed text (the first run after a build is slow).
                // Newer clang versions also report the call of the undeclared `helperlper` as an
                // error (C99 has no implicit declarations), older ones as a warning; both are fine.
                Step::WaitFor(
                    "only the undeclared names are left as problems",
                    Box::new(|b, cx| {
                        b.active_editor_entity().is_some_and(|e| {
                            let problems = e.read(cx).problems();
                            problems.iter().any(|p| p.message.contains("'missing'"))
                                && problems.iter().all(|p| p.message.contains("ndeclared"))
                        })
                    }),
                ),
                Step::Key("f2", None),
                Step::Expect(
                    "f2 jumps to the first undeclared name and shows the message",
                    Box::new(|b, cx| {
                        matches!(b.caret_position(cx), Some((3, 11)) | Some((4, 14)))
                            && b.doc_blocks().is_some_and(|blocks| {
                                blocks.iter().any(|block| {
                                    matches!(block, DocBlock::Text(t, _) if t.contains("ndeclared"))
                                })
                            })
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the message",
                    Box::new(|b, _| b.doc_blocks().is_none()),
                ),
                // After saving, the problem under the caret still shows its message in the status
                // bar instead of the save note (hand test 2026-11-01).
                Step::Key(save, None),
                Step::WaitFor(
                    "the status bar shows the message of the problem under the caret after saving",
                    Box::new(|b, cx| {
                        b.status_message(cx).is_some_and(|m| m.contains("ndeclared"))
                    }),
                ),
                // File Structure opens on the function around the caret; typing filters, Enter jumps.
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-f12"
                    } else {
                        "ctrl-f12"
                    },
                    None,
                ),
                Step::WaitFor(
                    "file structure lists the functions and selects main",
                    Box::new(|b, cx| {
                        b.structure_popup().is_some_and(|v| {
                            let v = v.read(cx);
                            v.names() == ["calc", "main"]
                                && v.selected_name().as_deref() == Some("main")
                        })
                    }),
                ),
                Step::Key("c", Some("c")),
                Step::Key("a", Some("a")),
                Step::Expect(
                    "typing filters to calc",
                    Box::new(|b, cx| {
                        b.structure_popup().is_some_and(|v| {
                            let v = v.read(cx);
                            // SPEC-0079 v4: the typed text stands in the filter field.
                            v.names() == ["calc"]
                                && v.selected_name().as_deref() == Some("calc")
                                && v.query() == "ca"
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::Expect(
                    "enter jumps to calc",
                    Box::new(|b, cx| {
                        b.structure_popup().is_none()
                            && b.caret_position(cx).is_some_and(|(line, _)| line == 1)
                    }),
                ),
                // Parameter Info opens after `(`; leaving the call closes it; ⌘P opens it again.
                Step::Key(goto_line, None),
                Step::Key("4", Some("4")),
                Step::Key(",", Some(",")),
                Step::Key("3", Some("3")),
                Step::Key("enter", None),
                Step::Key("c", Some("c")),
                Step::Key("a", Some("a")),
                Step::Key("l", Some("l")),
                Step::Key("c", Some("c")),
                Step::Key("(", Some("(")),
                Step::WaitFor(
                    "parameter info opens by itself after the parenthesis",
                    Box::new(|b, _| {
                        b.param_info()
                            .is_some_and(|(labels, _)| labels.iter().any(|l| l.contains("calc(")))
                    }),
                ),
                Step::Key(")", Some(")")),
                Step::WaitFor(
                    "leaving the call closes parameter info",
                    Box::new(|b, _| b.param_info().is_none()),
                ),
                Step::Key("backspace", None),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-p"
                    } else {
                        "ctrl-p"
                    },
                    None,
                ),
                Step::WaitFor(
                    "cmd+p shows the signature of calc again",
                    Box::new(|b, _| {
                        b.param_info()
                            .is_some_and(|(labels, _)| labels.iter().any(|l| l.contains("calc(")))
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes parameter info",
                    Box::new(|b, _| b.param_info().is_none()),
                ),
                // Go to Type Declaration: a variable of a typedef'd type jumps to the typedef.
                Step::Key(goto_line, None),
                Step::Key("1", Some("1")),
                Step::Key("enter", None),
                Step::Key("t", Some("t")),
                Step::Key("y", Some("y")),
                Step::Key("p", Some("p")),
                Step::Key("e", Some("e")),
                Step::Key("d", Some("d")),
                Step::Key("e", Some("e")),
                Step::Key("f", Some("f")),
                Step::Key("space", Some(" ")),
                Step::Key("i", Some("i")),
                Step::Key("n", Some("n")),
                Step::Key("t", Some("t")),
                Step::Key("space", Some(" ")),
                Step::Key("n", Some("n")),
                Step::Key("u", Some("u")),
                Step::Key("m", Some("m")),
                Step::Key(";", Some(";")),
                Step::Key("space", Some(" ")),
                Step::Key("n", Some("n")),
                Step::Key("u", Some("u")),
                Step::Key("m", Some("m")),
                Step::Key("space", Some(" ")),
                Step::Key("g", Some("g")),
                Step::Key(";", Some(";")),
                Step::Key("enter", None),
                Step::Key(goto_line, None),
                Step::Key("1", Some("1")),
                Step::Key(",", Some(",")),
                Step::Key("2", Some("2")),
                Step::Key("2", Some("2")),
                Step::Key("enter", None),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-shift-b"
                    } else {
                        "ctrl-shift-b"
                    },
                    None,
                ),
                Step::WaitFor(
                    "go to type declaration jumps from g to the typedef",
                    Box::new(|b, cx| {
                        b.caret_position(cx)
                            .is_some_and(|(line, column)| line == 1 && column < 22)
                    }),
                ),
                // Call Hierarchy of `calc`: the tool window opens with callers, switches to callees,
                // Escape closes it.
                Step::Key(goto_line, None),
                Step::Key("2", Some("2")),
                Step::Key(",", Some(",")),
                Step::Key("6", Some("6")),
                Step::Key("enter", None),
                Step::Key("ctrl-alt-h", None),
                Step::WaitFor(
                    "call hierarchy opens at the right with the callers of calc",
                    Box::new(|b, cx| {
                        // SPEC-0082 v4 AK-6: docked at the right, every level open.
                        b.hierarchy_side() == Some(idecraft_ui::tool_windows::Side::Right)
                            && b.hierarchy().is_some_and(|v| {
                                let v = v.read(cx);
                                let rows = v.visible();
                                v.direction() == CallDirection::Callers
                                    && rows.first().map(|(_, n)| n.as_str()) == Some("calc")
                                    && rows.iter().any(|(depth, n)| *depth == 1 && n == "main")
                            })
                    }),
                ),
                Step::Act(Box::new(|b, window, cx| {
                    b.set_hierarchy_direction(CallDirection::Callees, window, cx)
                })),
                Step::WaitFor(
                    "the callees button shows what calc calls",
                    Box::new(|b, cx| {
                        b.hierarchy().is_some_and(|v| {
                            let v = v.read(cx);
                            v.direction() == CallDirection::Callees
                                && v.visible().first().map(|(_, n)| n.as_str()) == Some("calc")
                        })
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the call hierarchy",
                    Box::new(|b, _| b.hierarchy().is_none()),
                ),
                // ⇧Esc hides it from the editor too.
                Step::Key("ctrl-alt-h", None),
                Step::WaitFor(
                    "call hierarchy opens again",
                    Box::new(|b, _| b.hierarchy().is_some()),
                ),
                Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
                Step::Key("shift-escape", None),
                Step::Expect(
                    "shift+escape in the editor hides the call hierarchy",
                    Box::new(|b, _| b.hierarchy().is_none()),
                ),
                // Code folding: ⌘- folds the body of `main` around the caret, ⌘= unfolds it.
                Step::Key(goto_line, None),
                Step::Key("4", Some("4")),
                Step::Key(",", Some(",")),
                Step::Key("3", Some("3")),
                Step::Key("enter", None),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd--"
                    } else {
                        "ctrl--"
                    },
                    None,
                ),
                Step::WaitFor(
                    "collapse folds the body of main and keeps the caret visible",
                    Box::new(|b, cx| {
                        b.active_folds(cx)
                            .is_some_and(|f| f.len() == 1 && f[0].0 == 2 && f[0].1 >= 4)
                            && b.caret_position(cx).is_some_and(|(line, _)| line == 3)
                    }),
                ),
                Step::Key(
                    if cfg!(target_os = "macos") {
                        "cmd-="
                    } else {
                        "ctrl-="
                    },
                    None,
                ),
                Step::Expect(
                    "expand unfolds it again",
                    Box::new(|b, cx| b.active_folds(cx).is_some_and(|f| f.is_empty())),
                ),
                group("diff-history"),
                // Diff viewer: a double-click on `bad.c` in the Commit window shows HEAD and the
                // editor's text side by side; F7 moves on, Escape closes.
                Step::Act(Box::new(|b, _window, cx| {
                    if let Some(commit) = b.commit_view() {
                        commit.update(cx, |c, cx| c.click("bad.c", true, cx));
                    }
                })),
                Step::WaitFor(
                    "a double-click in the commit window opens the diff of bad.c",
                    Box::new(|b, cx| {
                        b.diff_view().is_some_and(|d| {
                            let d = d.read(cx);
                            d.differences() >= 1 && d.current_row().is_some()
                        })
                    }),
                ),
                Step::Key("f7", None),
                Step::Expect(
                    "f7 keeps a difference selected",
                    Box::new(|b, cx| {
                        b.diff_view()
                            .is_some_and(|d| d.read(cx).current_row().is_some())
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the diff viewer",
                    Box::new(|b, _| b.diff_view().is_none()),
                ),
                // Local History: saving records a revision; the view lists it with its diff against
                // the current text; Escape closes it.
                Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
                Step::Key(save, None),
                Step::Act(Box::new(|b, window, cx| b.show_history(window, cx))),
                Step::WaitFor(
                    "local history lists the saved revisions of bad.c",
                    Box::new(|b, cx| {
                        b.history_view()
                            .is_some_and(|h| h.read(cx).revision_count() >= 2)
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape closes the local history",
                    Box::new(|b, _| b.history_view().is_none()),
                ),
                group("git-log"),
                // Git log (SPEC-0086): ⌘9 lists the commits newest first with their graph and
                // nothing selected (v3); Down shows the newest's details; typing filters; a changed
                // file of the commit opens its diff; ⌘9 closes.
                Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
                Step::Key(git_window, None),
                Step::WaitFor(
                    "the git log lists the commits newest first with a graph row each and nothing selected",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            let subjects = v.subjects();
                            subjects.first().map(String::as_str) == Some("selftest commit")
                                && subjects.last().map(String::as_str) == Some("initial")
                                && v.graph().len() == subjects.len()
                                && v.selected_row().is_none()
                                && v.details().is_none()
                                && v.current_branch().is_some()
                        })
                    }),
                ),
                Step::Key("down", None),
                Step::WaitFor(
                    "down selects the newest commit and shows its details",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            v.read(cx).details().is_some_and(|d| {
                                d.info.summary == "selftest commit"
                                    && d.changes.iter().any(|c| c.path == "b.rs")
                            })
                        })
                    }),
                ),
                Step::Key("i", Some("i")),
                Step::Key("n", Some("n")),
                Step::Key("i", Some("i")),
                Step::Key("t", Some("t")),
                Step::WaitFor(
                    "typing filters the log to the matching commit, which is not selected yet",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.subjects() == ["initial"] && v.selected_row().is_none()
                        })
                    }),
                ),
                Step::Key("down", None),
                Step::WaitFor(
                    "down selects the filtered commit",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            v.read(cx)
                                .details()
                                .is_some_and(|d| d.info.summary == "initial")
                        })
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape clears the filter and keeps the selected commit",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.filter().is_empty()
                                && v.subjects().len() >= 2
                                && v.details().is_some_and(|d| d.info.summary == "initial")
                        })
                    }),
                ),
                Step::Key("tab", None),
                Step::Key("enter", None),
                Step::WaitFor(
                    "enter on a changed file shows its diff in the commit",
                    Box::new(|b, cx| b.diff_view().is_some_and(|d| d.read(cx).differences() > 0)),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "escape returns from the diff to the git log",
                    Box::new(|b, _| b.diff_view().is_none() && b.git_log().is_some()),
                ),
                // SPEC-0086 v2 (hand test 31, D14): a commit made outside on another branch shows
                // up without reopening the window.
                Step::Act(Box::new(|_, _, _| {
                    if let Some(dir) = FIXTURE.get() {
                        fixture_git(dir, &["checkout", "-q", "-b", "log-side"]);
                        fixture_git(dir, &["commit", "-q", "--allow-empty", "-m", "outside on side"]);
                        fixture_git(dir, &["checkout", "-q", "-"]);
                    }
                })),
                Step::WaitFor(
                    "the git log picks up a commit made outside on another branch",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            v.read(cx).subjects().iter().any(|s| s == "outside on side")
                        })
                    }),
                ),
                // SPEC-0086 v4: the branches pane lists the current branch on top and log-side
                // under Local; choosing the current branch hides the commit of log-side, choosing
                // it again lists everything.
                Step::Expect(
                    "the branches pane shows the current branch on top and log-side under Local",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            let rows = v.branch_rows();
                            rows.first().is_some_and(|r| {
                                r.current && r.text == idecraft_workbench::git_branch_tree::CURRENT_ROW
                            }) && rows.iter().any(|r| r.text == "Local")
                                && rows.iter().any(|r| r.text == "log-side" && r.depth == 1)
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            if let Some(current) = v.current_branch().map(str::to_owned) {
                                v.choose_branch(&current, cx);
                            }
                        });
                    }
                })),
                Step::Expect(
                    "choosing the current branch lists only its commits",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.branch_filter().is_some()
                                && !v.subjects().iter().any(|s| s == "outside on side")
                                && v.subjects().iter().any(|s| s == "selftest commit")
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            if let Some(current) = v.current_branch().map(str::to_owned) {
                                v.choose_branch(&current, cx);
                            }
                        });
                    }
                })),
                Step::Expect(
                    "choosing it again lists the commits of every branch",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.branch_filter().is_none()
                                && v.subjects().iter().any(|s| s == "outside on side")
                        })
                    }),
                ),
                // SPEC-0086 v5: ⌥R and ⌥C switch the text filter's modes; a regular expression,
                // the user and a path filter narrow the list; clearing lists everything again.
                Step::Key("alt-r", None),
                Step::Key("alt-c", None),
                Step::Expect(
                    "alt+r and alt+c make the text filter a case-sensitive regular expression",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let f = v.read(cx).log_filter().clone();
                            f.regex && f.case
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        // The expression is searched in subject, author and e-mail (SPEC-0086 v5
                        // behaviour 3) and the fixture's e-mails start with "sel" too, so it is
                        // anchored on both ends to match only the subject.
                        log.update(cx, |v, cx| {
                            v.update_filter(|f| f.text = r"^sel\w* commit$".into(), cx)
                        });
                    }
                })),
                Step::Expect(
                    "the regular expression keeps the commits whose subject is sel... commit",
                    Box::new(|b, cx| {
                        b.git_log()
                            .is_some_and(|v| v.read(cx).subjects() == ["selftest commit"])
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            v.clear_filters(cx);
                            let author = v.authors().first().cloned();
                            v.update_filter(|f| f.author = author, cx);
                        });
                    }
                })),
                Step::Expect(
                    "the user filter keeps the fixture author's commits",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.log_filter().author.is_some()
                                && !v.log_filter().regex
                                && v.subjects().iter().any(|s| s == "selftest commit")
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            v.clear_filters(cx);
                            v.update_filter(|f| f.paths = vec!["b.rs".into()], cx);
                        });
                    }
                })),
                Step::WaitFor(
                    "a path filter keeps the commits that change the file",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            !v.filtering_paths()
                                && v.subjects().iter().any(|s| s == "selftest commit")
                                && !v.subjects().iter().any(|s| s == "outside on side")
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| v.clear_filters(cx));
                    }
                })),
                Step::Expect(
                    "clearing the filters lists every commit again",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            !v.log_filter().active()
                                && v.subjects().iter().any(|s| s == "outside on side")
                        })
                    }),
                ),
                // SPEC-0086 v7: the commit menu of HEAD's commit disables Cherry-Pick with its
                // reason and offers Undo; New Tag… creates a tag whose label appears; New Branch…
                // of the branches pane's action bar creates a branch.
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            if let Some(row) =
                                v.subjects().iter().position(|s| s == "selftest commit")
                            {
                                v.open_commit_menu(row, None, cx);
                            }
                        });
                    }
                })),
                Step::Expect(
                    "the commit menu disables Cherry-Pick on the current branch with a reason",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            v.read(cx).commit_menu_entries().is_some_and(|e| {
                                e.iter().any(|(l, on, why)| {
                                    l == "Cherry-Pick"
                                        && !on
                                        && why.as_deref().is_some_and(|w| w.contains("current branch"))
                                }) && e.iter().any(|(l, on, _)| l == "Undo Commit" && *on)
                            })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            v.choose_commit_action(idecraft_vcs::CommitAction::CherryPick, cx);
                        });
                    }
                })),
                Step::Expect(
                    "choosing a disabled entry shows its reason and keeps the menu open",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            v.read(cx)
                                .commit_menu_note()
                                .is_some_and(|n| n.contains("current branch"))
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            v.choose_commit_action(idecraft_vcs::CommitAction::NewTag, cx);
                            v.set_menu_name("selftest-tag", cx);
                            v.confirm_menu_name(cx);
                        });
                    }
                })),
                Step::WaitFor(
                    "New Tag… creates the tag and the commit shows its label",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.commit_menu_page().is_none()
                                && v.subjects()
                                    .iter()
                                    .position(|s| s == "selftest commit")
                                    .is_some_and(|row| {
                                        v.labels_of_row(row).iter().any(|l| {
                                            l.text == "selftest-tag"
                                                && l.kind
                                                    == idecraft_workbench::git_log_marks::LabelKind::Tag
                                        })
                                    })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            v.branch_action(idecraft_workbench::BranchAction::New, cx);
                            v.set_new_branch_name("selftest-new", cx);
                            v.confirm_new_branch(cx);
                        });
                    }
                })),
                Step::WaitFor(
                    "New Branch… in the branches pane's action bar creates the branch",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            v.new_branch_field().is_none()
                                && v.branch_rows().iter().any(|r| r.text == "selftest-new")
                        })
                    }),
                ),
                // SPEC-0086 v10 AK-31: the action bar's star marks the highlighted branch, Show
                // Only Favorites then lists it and hides the other branches.
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            if v.highlight_branch_row("log-side", cx) {
                                v.branch_action(idecraft_workbench::BranchAction::Favorite, cx);
                                v.branch_action(idecraft_workbench::BranchAction::OnlyFavorites, cx);
                            }
                        });
                    }
                })),
                Step::Expect(
                    "the star marks log-side and Show Only Favorites keeps it, not selftest-new",
                    Box::new(|b, cx| {
                        b.git_log().is_some_and(|v| {
                            let v = v.read(cx);
                            let rows = v.branch_rows();
                            v.only_favorites()
                                && rows.iter().any(|r| r.text == "log-side" && r.favorite)
                                && !rows.iter().any(|r| r.text == "selftest-new")
                                && rows.first().is_some_and(|r| {
                                    r.text == idecraft_workbench::git_branch_tree::CURRENT_ROW
                                })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(log) = b.git_log() {
                        log.update(cx, |v, cx| {
                            v.branch_action(idecraft_workbench::BranchAction::OnlyFavorites, cx);
                            v.set_favorite("log-side", false, false, cx);
                        });
                    }
                })),
                Step::Act(Box::new(|_, _, _| {
                    if let Some(dir) = FIXTURE.get() {
                        fixture_git(dir, &["branch", "-q", "-D", "log-side"]);
                        fixture_git(dir, &["branch", "-q", "-D", "selftest-new"]);
                        fixture_git(dir, &["tag", "-d", "selftest-tag"]);
                    }
                })),
                Step::Key(git_window, None),
                Step::Expect(
                    "the shortcut closes the git log again",
                    Box::new(|b, _| b.git_log().is_none()),
                ),
                group("run"),
                // Run (SPEC-0087): the first configuration is selected; Run streams its output into
                // the Run tool window; the chooser runs another one, Stop ends it, ⌘4 hides the window.
                // The file is written only now: an `.idecraft` folder at the start would shift the
                // project tree the first steps walk through.
                Step::Act(Box::new(|b, window, cx| {
                    if let Some(dir) = FIXTURE.get() {
                        let _ = std::fs::create_dir_all(dir.join(".idecraft"));
                        let _ = std::fs::write(
                            dir.join(".idecraft/run.toml"),
                            "[[run]]\nname = \"hello\"\ncommand = [\"/bin/sh\", \"-c\", \"echo hello-from-run; echo oops >&2\"]\n\n\
                             [[run]]\nname = \"sleeper\"\ncommand = [\"/bin/sh\", \"-c\", \"echo sleeping; sleep 30\"]\n\n\
                             [[run]]\nname = \"tests\"\nkind = \"test\"\nreport = \"tap\"\ncommand = [\"/bin/sh\", \"-c\", \"\
                             echo 'TAP version 13'; echo 1..3; echo 'ok 1 - adds'; echo 'not ok 2 - divides'; \
                             echo '  ---'; echo '  message: expected 2, got 3'; echo '  ...'; \
                             echo 'ok 3 - skips # SKIP no db'\"]\n",
                        );
                    }
                    b.focus_editor(window, cx)
                })),
                Step::Key(run_key, None),
                Step::WaitFor(
                    "run shows the output and the exit code in the run tool window",
                    Box::new(|b, cx| {
                        b.run_view().is_some_and(|v| {
                            v.read(cx).selected_output().is_some_and(|(name, text)| {
                                name == "hello"
                                    && text.contains("hello-from-run")
                                    && text.contains("oops")
                                    && text.ends_with("Process finished with exit code 0")
                            })
                        })
                    }),
                ),
                Step::Expect(
                    "run selected the first configuration",
                    Box::new(|b, _| b.selected_run_configuration().as_deref() == Some("hello")),
                ),
                Step::Key(choose_run, None),
                Step::Expect(
                    "the chooser lists the configurations",
                    Box::new(|b, cx| {
                        b.run_chooser()
                            .is_some_and(|c| c.read(cx).names() == ["hello", "sleeper", "tests"])
                    }),
                ),
                Step::Key("down", None),
                Step::Key("enter", None),
                Step::WaitFor(
                    "the chosen configuration runs",
                    Box::new(|b, cx| {
                        b.run_chooser().is_none()
                            && b.selected_run_configuration().as_deref() == Some("sleeper")
                            && b.run_view().is_some_and(|v| {
                                let v = v.read(cx);
                                v.running()
                                    && v.selected_output()
                                        .is_some_and(|(_, t)| t.contains("sleeping"))
                            })
                    }),
                ),
                // SPEC-0261 v3 behaviour 14 (D58): quitting sees the run in this first window, whose
                // root view is the self-test's.
                Step::Expect(
                    "quitting lists the running configuration of the first window",
                    Box::new(|_, cx| {
                        WorkbenchView::pending_work(cx)
                            .1
                            .iter()
                            .any(|r| r.ends_with("run configuration \"sleeper\""))
                            && idecraft_core::live_runs() >= 1
                    }),
                ),
                // SPEC-0261 v4 behaviours 5 and 16: a quit request (what ⌘Q and the menu item run)
                // draws the question in this window; Escape is Cancel, so the window and the run
                // stay.
                Step::Setup(Box::new(|_, _, cx| WorkbenchView::quit(cx))),
                Step::WaitFor(
                    "a quit request with a running configuration draws the question in the window",
                    Box::new(|b, _| {
                        b.drawn_question().is_some_and(|q| {
                            q.message == "Stop running processes and quit?"
                                && q.detail.contains("run configuration \"sleeper\"")
                                && q.buttons == ["Stop and Quit", "Cancel"]
                        })
                    }),
                ),
                // SPEC-0261 v5 behaviour 5: the frame ends just below its buttons (its padding),
                // no tall empty area under them.
                Step::WaitFor(
                    "the drawn question is as tall as its content",
                    Box::new(|_, _| {
                        use idecraft_workbench::hit_targets::get;
                        match (get("drawn-question-frame"), get("drawn-question-button-0")) {
                            (Some(f), Some(b)) => {
                                let below = (f.y + f.h) - (b.y + b.h);
                                (0.0..=20.0).contains(&below) && f.h < 300.0
                            }
                            _ => false,
                        }
                    }),
                ),
                Step::Key("escape", None),
                Step::Expect(
                    "Cancel closes the quit question; the window and the run stay",
                    Box::new(|b, cx| {
                        b.drawn_question().is_none()
                            && b.run_view().is_some_and(|v| v.read(cx).running())
                            && idecraft_core::live_runs() >= 1
                    }),
                ),
                Step::Key(stop_key, None),
                Step::WaitFor(
                    "stop ends the running process",
                    Box::new(|b, cx| {
                        b.run_view().is_some_and(|v| {
                            let v = v.read(cx);
                            !v.running()
                                && v.selected_output()
                                    .is_some_and(|(_, t)| t.ends_with("(stopped)"))
                        })
                    }),
                ),
                // Test results tree (SPEC-0088): a test configuration's tab shows the tree with the
                // status line, the failed test selected with its message; passed tests can be hidden.
                // The chooser starts at the current configuration (sleeper); one down is tests.
                Step::Key(choose_run, None),
                Step::Key("down", None),
                Step::Key("enter", None),
                Step::WaitFor(
                    "the test configuration shows its results tree with the failed test selected",
                    Box::new(|b, cx| {
                        b.run_view().is_some_and(|v| {
                            v.read(cx).test_results().is_some_and(|t| {
                                t.summary == "Tests failed: 1, passed: 1, ignored: 1 of 3 tests"
                                    && t.rows.iter().any(|(_, n)| n == "divides")
                                    && t.details.is_some_and(|d| d.contains("expected 2, got 3"))
                            })
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(v) = b.run_view() {
                        v.update(cx, |v, cx| v.toggle_show_passed(cx));
                    }
                })),
                Step::Expect(
                    "hiding passed tests leaves the failed and the ignored one",
                    Box::new(|b, cx| {
                        b.run_view().is_some_and(|v| {
                            v.read(cx).test_results().is_some_and(|t| {
                                !t.rows.iter().any(|(_, n)| n == "adds")
                                    && t.rows.iter().any(|(_, n)| n == "divides")
                            })
                        })
                    }),
                ),
            ])
            .chain(shortcut_hides(
                run_window,
                "the run tool window shortcut focuses the shown window",
                "the run tool window shortcut hides it",
                |b| b.run_view().is_some(),
            ))
            .chain([
                group("database"),
                // Database tool window (SPEC-0089): a SQLite connection of the workspace lists its
                // tables, columns and keys once expanded; Escape hides the window.
                Step::Act(Box::new(|b, window, cx| {
                    if let Some(dir) = FIXTURE.get() {
                        let db = dir.join("shop.db");
                        let _ = std::process::Command::new("sqlite3")
                            .arg(&db)
                            .arg(
                                "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
                                 CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL \
                                 REFERENCES customers(id), note TEXT); \
                                 CREATE INDEX orders_by_customer ON orders(customer_id);",
                            )
                            .status();
                        let _ = std::fs::write(
                            dir.join(".idecraft/connections.toml"),
                            format!(
                                "[[connection]]\nid = \"shop\"\nname = \"shop\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
                                db.display()
                            ),
                        );
                    }
                    b.toggle_database(window, cx)
                })),
                Step::Expect(
                    "the database tool window lists the connection",
                    Box::new(|b, cx| {
                        b.database_view().is_some_and(|v| {
                            v.read(cx).visible().first().is_some_and(|(_, name, detail)| {
                                name == "shop" && detail == "SQLite · shop.db"
                            })
                        })
                    }),
                ),
                Step::Key("enter", None),
                Step::WaitFor(
                    "expanding the connection reads its tables",
                    Box::new(|b, cx| {
                        b.database_view().is_some_and(|v| {
                            v.read(cx)
                                .visible()
                                .iter()
                                .any(|(_, n, d)| n == "tables" && d == "2")
                        })
                    }),
                ),
                Step::Act(Box::new(|b, _, cx| {
                    if let Some(v) = b.database_view() {
                        v.update(cx, |v, cx| v.expand_all_tables(cx));
                    }
                })),
                Step::Expect(
                    "tables show their columns, keys and indexes",
                    Box::new(|b, cx| {
                        b.database_view().is_some_and(|v| {
                            let rows = v.read(cx).visible();
                            let has = |n: &str, d: &str| rows.iter().any(|(_, a, b)| a == n && b == d);
                            has("customer_id", "integer · not null")
                                && has("foreign key", "(customer_id) → customers(id)")
                                && has("orders_by_customer", "(customer_id)")
                        })
                    }),
                ),
            ])
            .collect()
        };
        vec![
            Block::new(
                &[
                    "editor",
                    "terminal",
                    "commit",
                    "search",
                    "disk-changes",
                    "branches",
                    "code-insight",
                    "diff-history",
                    "git-log",
                    "run",
                    "database",
                ],
                move || inline,
            ),
            Block::new(&["db-search-ddl"], db_search_ddl_steps),
            Block::new(&["db-rename"], db_rename_steps),
            Block::new(&["database-close"], || {
                vec![
                    Step::Key("escape", None),
                    Step::Expect(
                        "escape hides the database tool window",
                        Box::new(|b, _| b.database_view().is_none()),
                    ),
                ]
            }),
            Block::new(&["sql-console"], move || sql_console_steps(goto_line)),
            Block::new(&["sql-editor"], sql_editor_steps),
            Block::new(&["database-error"], database_error_steps),
            Block::new(&["grid-edit"], grid_edit_steps),
            Block::new(&["table-editor"], table_editor_steps),
            Block::new(&["connections"], connection_steps),
            Block::new(&["db-compare"], db_compare_steps),
            Block::new(&["er-diagram"], move || er_diagram_steps(layout)),
            Block::new(&["split"], split_steps),
            Block::new(&["split-move"], split_move_steps),
            Block::new(&["tabs"], editor_tabs_steps),
            Block::new(&["tool-window"], tool_window_steps),
            Block::new(&["tool-window-mode"], tool_window_mode_steps),
            Block::new(&["tool-window-replace"], tool_window_replace_steps),
            Block::new(&["tooltips"], tooltip_steps),
            Block::new(&["title-bar-clicks"], title_bar_click_steps),
            Block::new(&["status-bar"], status_bar_steps),
            Block::new(&["terminal-session"], terminal_session_steps),
            Block::new(&["http"], http_steps),
            Block::new(&["python-debug"], move || python_debug_steps(goto_line)),
            Block::new(&["attach"], move || attach_steps(goto_line)),
            Block::new(
                &["detected-run", "detected-run-debug", "run-widget-select"],
                detected_run_steps,
            ),
            Block::new(&["run-config-editor"], run_config_editor_steps),
            Block::new(
                &[
                    "run-config-folders",
                    "run-config-templates",
                    "run-config-instances",
                    "run-config-before-launch",
                ],
                run_config_parity_steps,
            ),
            Block::new(&["projects"], projects_steps),
            Block::new(&["debug"], move || debug_steps(goto_line)),
            Block::new(&["debug-values"], move || debug_value_steps(goto_line)),
            Block::new(&["replace"], replace_steps),
            Block::new(&["run-file"], run_file_steps),
            Block::new(&["python-run-file"], python_run_file_steps),
            Block::new(&["go-test"], go_test_steps),
            Block::new(&["java-debug-session"], java_debug_steps),
            Block::new(&["java-smart"], java_smart_step_steps),
            Block::new(&["java-debug-more"], java_debug_more_steps),
            Block::new(&["python-debug-more"], python_debug_more_steps),
            Block::new(&["node-debug"], node_debug_steps),
            Block::new(&["package-script-debug"], package_script_debug_steps),
            Block::new(&["kotlin-debug"], kotlin_debug_steps),
            Block::new(&["browser-debug"], browser_debug_steps),
            Block::new(
                &[
                    "breakpoint-dialog",
                    "breakpoint-condition-debug",
                    "breakpoint-moves",
                ],
                breakpoint_steps,
            ),
            Block::new(&["breakpoint-options"], breakpoint_option_steps),
            Block::new(&["problems"], problems_steps),
            Block::new(&["inspections"], inspection_steps),
            Block::new(&["refactor"], refactor_steps),
            Block::new(&["rename-text"], rename_text_steps),
            Block::new(&["undo-group"], undo_group_steps),
            Block::new(&["signature"], signature_steps),
            Block::new(&["move"], move_steps),
            Block::new(&["plugins"], plugin_steps),
            Block::new(&["plugin-inspections"], plugin_inspection_steps),
            Block::new(&["plugin-extensions"], plugin_extension_steps),
            Block::new(&["bundle-editor"], bundle_editor_steps),
            Block::new(&["ide-settings"], ide_settings_steps),
            Block::new(&["natural-language"], natural_language_steps),
            Block::new(&["compact-mode"], compact_mode_steps),
            Block::new(&["column-select"], column_select_steps),
            Block::new(&["column-mode"], column_mode_steps),
            Block::new(&["folding"], folding_steps),
            Block::new(&["open-plugin-feature"], open_plugin_feature_steps),
            Block::new(&["endpoints"], endpoints_steps),
            Block::new(&["spring"], spring_steps),
            Block::new(&["dialects"], dialect_steps),
            Block::new(&["connectors"], connector_steps),
            Block::new(&["store"], store_steps),
            Block::new(&["reports"], report_steps),
            Block::new(&["connector-import"], connector_import_steps),
            Block::new(&["plugin-network"], plugin_network_steps),
            Block::new(&["python"], python_steps),
            Block::new(&["containers"], containers_steps),
            Block::new(
                &["profiler", "profiler-attach", "profiler-close"],
                profiler_steps,
            ),
            Block::new(&["remote"], remote_steps),
            Block::new(&["remote-project-windows"], remote_project_steps),
            Block::new(&["remote-host"], remote_host_steps),
            Block::new(&["remote-tool-windows"], remote_tool_window_steps),
            Block::new(&["jvm"], jvm_steps),
            Block::new(&["kotlin-generate"], kotlin_generate_steps),
            Block::new(&["build-tools"], build_tools_steps),
            Block::new(&["theme"], theme_steps),
            Block::new(&["import"], import_steps),
            Block::new(&["settings"], settings_steps),
            Block::new(&["git-ops"], git_ops_steps),
            Block::new(&["push"], push_steps),
            Block::new(&["commit-window"], commit_window_steps),
            Block::new(&["java-insight"], java_insight_steps),
            Block::new(&["java-structure"], java_structure_steps),
            Block::new(&["java-inspections"], java_inspection_steps),
            Block::new(&["js-tools"], js_tool_steps),
            Block::new(&["menus"], menu_steps),
            Block::new(&["editorconfig"], editorconfig_steps),
            Block::new(&["ai"], ai_steps),
            Block::new(&["ai-context"], ai_context_steps),
            Block::new(&["merge"], merge_steps),
            Block::new(&["merge-edit"], merge_edit_steps),
            Block::new(&["ruby"], ruby_steps),
            Block::new(&["mcp-http"], mcp_http_steps),
            Block::new(&["prompt-keys"], prompt_keys_steps),
            Block::new(&["git-m63"], git_m63_steps),
            Block::new(&["mouse"], mouse_steps),
        ]
    }

    fn new(bench: Entity<WorkbenchView>, enabled: bool, layout: KeyLayout) -> Self {
        // The window opens only after `run` has checked the lists of groups.
        let plan = if enabled {
            plan(Self::blocks(layout), crate::selftest_groups::from_env).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                Plan::default()
            })
        } else {
            Plan::default()
        };
        for name in &plan.skipped {
            eprintln!("{}", crate::selftest_groups::skipped_line(name));
        }
        Self {
            bench,
            steps: plan.steps,
            spans: plan.spans,
            span_at: 0,
            span_since: None,
            timings: Vec::new(),
            skipped: plan.skipped,
            next: 0,
            waiting: false,
            since: None,
            failures: Vec::new(),
            driver: enabled.then(SelfTestDriver::from_env).flatten(),
            hold: enabled.then(hold_from_env).flatten(),
            held: None,
        }
    }

    /// Closes the groups whose last step has been taken and notes how long each took.
    fn clock_groups(&mut self) {
        let now = std::time::Instant::now();
        let trace = std::env::var_os("IDECRAFT_SELFTEST_TRACE").is_some();
        if trace
            && self.span_since.is_none()
            && let Some((first, ..)) = self.spans.first()
        {
            eprintln!("group {first}");
        }
        let mut since = *self.span_since.get_or_insert(now);
        while let Some(&(name, end, steps)) = self.spans.get(self.span_at)
            && end <= self.next
        {
            self.timings.push((name, now - since, steps));
            self.span_at += 1;
            since = now;
            self.span_since = Some(now);
            if trace && let Some((next, ..)) = self.spans.get(self.span_at) {
                eprintln!("group {next}");
            }
        }
    }

    /// Runs a click or hover step through the UI driver (SPEC-0319 behaviour 42): the call is
    /// submitted once, then every frame feeds the window's tree; returns `false` while it waits.
    fn drive(
        &mut self,
        target: &str,
        action: idecraft_uidriver::Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use idecraft_uidriver::engine::{Command, Op, Request, Target};
        let Some(d) = self.driver.as_mut() else {
            return true;
        };
        let id = idecraft_workbench::semantics::window_id(window);
        let now = std::time::Instant::now();
        if d.op.is_none() {
            let request = Request::selftest(Op::Act {
                target: Target::Locator(idecraft_uidriver::Locator::id(target)),
                action,
                level: Some(d.level),
                force: false,
            });
            match d.engine.submit(id, request, now) {
                Ok(op) => d.op = Some(op),
                Err(e) => {
                    let what: &'static str =
                        Box::leak(format!("cannot drive {target}: {e}").into());
                    self.fail(what, cx);
                    return true;
                }
            }
        }
        let tree = idecraft_workbench::semantics::tree(id)
            .unwrap_or_else(|| std::rc::Rc::new(idecraft_uidriver::Tree::empty(0)));
        let out = d
            .engine
            .frame(id, &tree, now, &crate::ui_driver::KeymapKeys);
        for command in out.commands {
            if let Command::Deliver { inputs, level, .. } = command {
                if level == idecraft_uidriver::Level::Native {
                    // Native events go to the application's queue, as the old path posts them.
                    if let Err(e) = crate::ui_driver::deliver(window, cx, &inputs, level) {
                        eprintln!("self-test: {target}: {e}");
                    }
                } else {
                    // Not inside this frame: toolkit events are dispatched after it.
                    cx.defer_in(window, move |_, window, cx| {
                        if let Err(e) = crate::ui_driver::deliver(window, cx, &inputs, level) {
                            eprintln!("self-test: {e}");
                        }
                    });
                }
            }
        }
        match out.done.into_iter().next() {
            Some(done) => {
                d.op = None;
                if let Err(e) = done.reply.result {
                    let what: &'static str =
                        Box::leak(format!("cannot drive {target}: {e}").into());
                    self.fail(what, cx);
                }
                true
            }
            None => {
                // Another frame decides; the window is asked for one, no timer.
                window.refresh();
                false
            }
        }
    }
}

impl Render for SelfTest {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.steps.is_empty() && !self.waiting {
            self.clock_groups();
            if let Some(step) = self.steps.get(self.next) {
                // `IDECRAFT_SELFTEST_TRACE=1` names each step on stderr, to find where a run hangs.
                if self.since.is_none() && std::env::var_os("IDECRAFT_SELFTEST_TRACE").is_some() {
                    let what = match step {
                        Step::Key(key, _) => format!("key {key}"),
                        Step::Expect(name, _)
                        | Step::WaitFor(name, _)
                        | Step::WaitLong(name, _, _) => (*name).to_owned(),
                        Step::Act(_) => "act".to_owned(),
                        Step::Setup(_) => "set-up".to_owned(),
                        Step::Click(target, kind) => format!("click {kind:?} {target}"),
                        Step::Hover(target) => format!("hover {target}"),
                        Step::Group(name) => format!("group {name}"),
                    };
                    eprintln!("step {}: {what}", self.next);
                }
                match step {
                    Step::Key(key, ch) => {
                        let mut k = Keystroke::parse(key).expect("valid keystroke");
                        k.key_char = ch.map(str::to_owned);
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |window, cx| {
                            window.dispatch_keystroke(k, cx);
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            });
                        });
                    }
                    Step::Click(target, kind) if self.driver.is_some() => {
                        let (target, action) = (target.clone(), click_action(*kind));
                        if !self.drive(&target, action, window, cx) {
                            self.waiting = true;
                            let me = cx.entity();
                            window.on_next_frame(move |_, cx| {
                                me.update(cx, |this, cx| {
                                    this.waiting = false;
                                    cx.notify()
                                })
                            });
                            return div().size_full().child(self.bench.clone());
                        }
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |_, cx| {
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            })
                        });
                    }
                    Step::Hover(target) if self.driver.is_some() => {
                        let target = target.clone();
                        let action = idecraft_uidriver::Action::Hover { position: None };
                        if !self.drive(&target, action, window, cx) {
                            self.waiting = true;
                            let me = cx.entity();
                            window.on_next_frame(move |_, cx| {
                                me.update(cx, |this, cx| {
                                    this.waiting = false;
                                    cx.notify()
                                })
                            });
                            return div().size_full().child(self.bench.clone());
                        }
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |_, cx| {
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            })
                        });
                    }
                    Step::Click(target, kind) => {
                        let posted =
                            idecraft_workbench::hit_targets::get(target).is_some_and(|r| {
                                crate::native_click::post(
                                    window,
                                    cx,
                                    r.x + r.w / 2.,
                                    r.y + r.h / 2.,
                                    *kind,
                                )
                            });
                        if !posted {
                            let what: &'static str =
                                Box::leak(format!("cannot click {target}").into());
                            self.fail(what, cx);
                        }
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |_, cx| {
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            })
                        });
                    }
                    Step::Hover(target) => {
                        // A native pointer move, like the clicks (SPEC-0294 AK-7).
                        let posted =
                            idecraft_workbench::hit_targets::get(target).is_some_and(|r| {
                                crate::native_click::post_move(
                                    window,
                                    cx,
                                    r.x + r.w / 2.,
                                    r.y + r.h / 2.,
                                )
                            });
                        if !posted {
                            let what: &'static str =
                                Box::leak(format!("cannot hover {target}").into());
                            self.fail(what, cx);
                        }
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |_, cx| {
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            })
                        });
                    }
                    Step::Act(_) | Step::Setup(_) => {
                        // Not inside this frame: an act that opens a window (a remote project)
                        // makes GPUI draw that window at once, and its draw clears the element
                        // arena this window's frame is still using ("pointer being freed was not
                        // allocated"). The act runs right after the frame instead.
                        let at = self.next;
                        self.waiting = true;
                        cx.defer_in(window, move |this, window, cx| {
                            if let Some(Step::Act(act) | Step::Setup(act)) = this.steps.get(at) {
                                this.bench.update(cx, |b, cx| act(b, window, cx));
                            }
                            let me = cx.entity();
                            window.on_next_frame(move |_, cx| {
                                me.update(cx, |this, cx| {
                                    this.waiting = false;
                                    cx.notify()
                                })
                            });
                            window.refresh();
                        });
                    }
                    Step::WaitFor(what, check) | Step::WaitLong(what, _, check) => {
                        let limit = match step {
                            Step::WaitLong(_, limit, _) => *limit,
                            _ => WAIT,
                        };
                        let since = *self.since.get_or_insert_with(std::time::Instant::now);
                        let passed = check(self.bench.read(cx), cx);
                        let hold = self.hold.as_ref().and_then(|(d, only)| {
                            only.as_deref()
                                .is_none_or(|o| o.split(',').any(|o| what.contains(o)))
                                .then_some(*d)
                        });
                        if passed && hold.is_some() && self.held.is_none() {
                            eprintln!("hold: {what}");
                        }
                        if passed
                            && let Some(hold) = hold
                            && self
                                .held
                                .get_or_insert_with(std::time::Instant::now)
                                .elapsed()
                                < hold
                        {
                            // A window check looks at the window as this wait left it.
                            std::thread::sleep(std::time::Duration::from_millis(16));
                            let me = cx.entity();
                            self.waiting = true;
                            window.on_next_frame(move |_, cx| {
                                me.update(cx, |this, cx| {
                                    this.waiting = false;
                                    cx.notify()
                                })
                            });
                            return div().size_full().child(self.bench.clone());
                        }
                        if passed {
                            self.held = None;
                            self.since = None;
                        } else if since.elapsed() < limit {
                            std::thread::sleep(std::time::Duration::from_millis(16));
                            let me = cx.entity();
                            self.waiting = true;
                            window.on_next_frame(move |_, cx| {
                                me.update(cx, |this, cx| {
                                    this.waiting = false;
                                    cx.notify()
                                })
                            });
                            return div().size_full().child(self.bench.clone());
                        } else {
                            self.since = None;
                            self.fail(what, cx);
                        }
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |_, cx| {
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            })
                        });
                    }
                    // `plan` takes the marks out; one left over only passes.
                    Step::Group(_) => cx.notify(),
                    Step::Expect(what, check) => {
                        if !check(self.bench.read(cx), cx) {
                            self.fail(what, cx);
                        }
                        let me = cx.entity();
                        self.waiting = true;
                        window.on_next_frame(move |_, cx| {
                            me.update(cx, |this, cx| {
                                this.waiting = false;
                                cx.notify()
                            })
                        });
                    }
                }
                self.next += 1;
            } else {
                let checks = self
                    .steps
                    .iter()
                    .filter(|s| {
                        matches!(s, Step::Expect(..) | Step::WaitFor(..) | Step::WaitLong(..))
                    })
                    .count();
                // Slow groups first, so a run near its time limit shows where the time goes.
                for line in crate::selftest_groups::timing_lines(&self.timings) {
                    eprintln!("{line}");
                }
                println!(
                    "{}",
                    crate::selftest_groups::result_line(checks, &self.failures, &self.skipped)
                );
                self.steps.clear();
                cx.quit();
            }
        }
        div().size_full().child(self.bench.clone())
    }
}

/// The Debug tool window (SPEC-0091) with lldb-dap on a small C program: a breakpoint by key, Debug
/// stops there with the call stack and locals, Step Into and Step Out move between the functions,
/// Resume runs to the end. Skipped when lldb-dap or a C compiler is missing.
fn debug_steps(goto_line: &'static str) -> Vec<Step> {
    let compiler = std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_ok();
    if idecraft_debugger::lldb_dap().is_none()
        || !idecraft_debugger::can_debug_unattended()
        || !compiler
    {
        eprintln!("debug steps skipped: lldb-dap or a C compiler is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (toggle, debug, step_into, step_out, resume, window) = if mac {
        ("cmd-f8", "ctrl-d", "f7", "shift-f8", "cmd-alt-r", "cmd-5")
    } else {
        ("ctrl-f8", "shift-f9", "f7", "shift-f8", "f9", "alt-5")
    };
    let state = |b: &WorkbenchView, cx: &App| {
        b.debug_view().map(|v| {
            let v = v.read(cx);
            (
                v.state().clone(),
                v.frame_names(),
                v.variable_texts(),
                v.console().to_owned(),
            )
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("dbg.c"),
                    "#include <stdio.h>\nint twice(int x) {\n    int y = x * 2;\n    return y;\n}\nint main(void) {\n    int a = 21;\n    int b = twice(a);\n    printf(\"result %d\\n\", b);\n    return 0;\n}\n",
                );
                let _ = std::process::Command::new("cc")
                    .args(["-g", "-O0", "-o", "dbg", "dbg.c"])
                    .current_dir(dir)
                    .status();
                let _ = std::fs::write(
                    dir.join(".idecraft/run.toml"),
                    "[[run]]\nname = \"hello\"\ncommand = [\"/bin/sh\", \"-c\", \"echo hello-from-run\"]\n\n\
                     [[run]]\nname = \"debugme\"\ncommand = [\"./dbg\"]\n",
                );
                b.select_run_configuration("debugme", cx);
                b.open(&dir.join("dbg.c"), window, cx);
            }
        })),
        Step::Key(goto_line, None),
        Step::Key("8", Some("8")),
        Step::Key("enter", None),
        Step::Key(toggle, None),
        Step::Expect(
            "the breakpoint shortcut sets a breakpoint on the caret line",
            Box::new(|b, cx| b.active_debug_marks(cx).is_some_and(|(bp, _)| bp == [7])),
        ),
        Step::Key(debug, None),
        Step::WaitFor(
            "debug stops at the breakpoint with the call stack and the locals",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("main"))
                        && vars.iter().any(|v| v == "a = 21")
                }) && b
                    .active_debug_marks(cx)
                    .is_some_and(|(_, exec)| exec == Some(7))
            }),
        ),
        Step::Key(step_into, None),
        Step::WaitFor(
            "step into enters twice",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("twice"))
                        && vars.iter().any(|v| v == "x = 21")
                }) && b
                    .active_debug_marks(cx)
                    .is_some_and(|(_, exec)| exec == Some(2))
            }),
        ),
        Step::Key(step_out, None),
        Step::WaitFor(
            "step out returns to main",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, _, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("main"))
                })
            }),
        ),
        Step::Key("alt-f8", None),
        Step::Expect(
            "the expression field is drawn wide and high enough to click into",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let (w, h) = v.read(cx).expression_size(cx);
                    w > 100.0 && h >= 16.0
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.set_expression("", cx));
            }
        })),
        Step::Expect(
            "the empty expression field shows the evaluate and watch hint",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).expression_placeholder(cx).is_some())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.set_expression("a * 2", cx));
            }
        })),
        Step::Expect(
            "the hint disappears as soon as the field has text",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).expression_placeholder(cx).is_none())
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "evaluate expression computes a value in the paused frame",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    v.read(cx)
                        .result_texts()
                        .first()
                        .is_some_and(|r| r == "a * 2 = 42")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume runs to the end and clears the execution point",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, _, _, console)| {
                    matches!(&s, DebugState::Ended(l) if l.contains("exit code 0"))
                        && console.contains("result 42")
                }) && b
                    .active_debug_marks(cx)
                    .is_some_and(|(bp, exec)| bp == [7] && exec.is_none())
            }),
        ),
        Step::Expect(
            "breakpoints are saved for the next start",
            Box::new(|_, _| {
                FIXTURE
                    .get()
                    .is_some_and(|dir| breakpoint_saved(dir, "dbg.c", 8))
            }),
        ),
    ]
    .into_iter()
    .chain(shortcut_hides(
        window,
        "the debug tool window shortcut focuses the shown window",
        "the debug tool window shortcut hides it",
        |b| b.debug_view().is_some(),
    ))
    .collect()
}

/// Whether the workspace at `dir` has a saved breakpoint on the 1-based `line` of `file`, read
/// the way the next start reads it: `.idecraft/breakpoints.toml`, else the file of older versions
/// (SPEC-0303 behaviour 14).
fn breakpoint_saved(dir: &Path, file: &str, line: usize) -> bool {
    line > 0
        && idecraft_debugger::saved::load_breakpoints(dir)
            .get(&dir.join(file))
            .is_some_and(|lines| lines.contains(&(line - 1)))
}

/// Set Value and Run to Cursor (SPEC-0194 AK-4) with lldb-dap on the program of `debug_steps`:
/// at the breakpoint on `int b = twice(a);`, F2 on `a` in the Variables list sets it to 5, ⌥F9 on
/// the `printf` line runs there, and Resume prints the result of the changed value. Skipped like
/// `debug_steps`.
fn debug_value_steps(goto_line: &'static str) -> Vec<Step> {
    let compiler = std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_ok();
    if idecraft_debugger::lldb_dap().is_none()
        || !idecraft_debugger::can_debug_unattended()
        || !compiler
    {
        eprintln!("debug value steps skipped: lldb-dap or a C compiler is missing");
        return Vec::new();
    }
    let (debug, resume, window) = if cfg!(target_os = "macos") {
        ("ctrl-d", "cmd-alt-r", "cmd-5")
    } else {
        ("shift-f9", "f9", "alt-5")
    };
    let paused_at = |b: &WorkbenchView, cx: &App| {
        b.debug_view()
            .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Paused(_)))
            .then(|| b.active_debug_marks(cx).and_then(|(_, exec)| exec))
            .flatten()
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                b.select_run_configuration("debugme", cx);
                b.open(&dir.join("dbg.c"), window, cx);
            }
        })),
        Step::Key(debug, None),
        Step::WaitFor(
            "debug stops at the saved breakpoint again",
            Box::new(move |b, cx| paused_at(b, cx) == Some(7)),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.select_variable("a", cx));
                window.focus(&v.read(cx).focus());
            }
        })),
        Step::Key("f2", None),
        Step::Expect(
            "f2 in the Debug window starts setting the selected variable",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).setting() == Some("a"))
            }),
        ),
        Step::Key("5", Some("5")),
        Step::Key("enter", None),
        Step::WaitFor(
            "enter sets the variable and the list shows the new value",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).variable_texts().iter().any(|t| t == "a = 5"))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                b.open(&dir.join("dbg.c"), window, cx);
            }
        })),
        Step::Key(goto_line, None),
        Step::Key("9", Some("9")),
        Step::Key("enter", None),
        Step::Key("alt-f9", None),
        Step::WaitFor(
            "run to cursor stops on the caret line",
            Box::new(move |b, cx| paused_at(b, cx) == Some(8)),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume ends with the result of the changed value; only the saved breakpoint is left",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Ended(_)) && v.console().contains("result 10")
                }) && b
                    .active_debug_marks(cx)
                    .is_some_and(|(bp, exec)| bp == [7] && exec.is_none())
            }),
        ),
        // The toolbar (SPEC-0194 v3 AK-6): Rerun after the end stops at the breakpoint again.
        Step::Expect(
            "after the end only Rerun and the breakpoint buttons are enabled",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let t = v.read(cx).toolbar();
                    t.rerun && !t.stop && !t.pause && !t.paused
                })
            }),
        ),
        // SPEC-0194 v4 AK-8, SPEC-0294 AK-7: the toolbar's Rerun and the breakpoint in the gutter
        // explain themselves.
        hover("debug-rerun"),
        Step::WaitFor(
            "hovering the debug toolbar's Rerun shows Rerun with its shortcut",
            Box::new(|_, _| tooltip_shows(|t| t == declared_tip("debug-rerun"))),
        ),
        hover("gutter-breakpoint-8"),
        Step::WaitFor(
            "hovering the breakpoint in the gutter says it is a line breakpoint",
            Box::new(|_, _| tooltip_shows(|t| t.starts_with("Line breakpoint"))),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.press("Rerun", cx));
            }
        })),
        Step::WaitFor(
            "rerun starts the configuration again and stops at the breakpoint",
            Box::new(move |b, cx| paused_at(b, cx) == Some(7)),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.press("XDebugger.MuteBreakpoints", cx));
            }
        })),
        Step::Expect(
            "mute breakpoints greys them; they stay in the gutter",
            Box::new(|b, cx| {
                b.breakpoints_muted()
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).breakpoints_muted())
                    && b.active_debug_marks(cx).is_some_and(|(bp, _)| bp == [7])
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.press("Rerun", cx));
            }
        })),
        Step::WaitFor(
            "while muted, rerun runs to the end without stopping",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Ended(l) if l.contains("exit code 0"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| {
                    v.press("XDebugger.MuteBreakpoints", cx);
                    v.press("ViewBreakpoints", cx);
                });
            }
        })),
        Step::Expect(
            "unmuted again; the View Breakpoints button opens the dialog",
            Box::new(|b, _| !b.breakpoints_muted() && b.breakpoint_dialog().is_some()),
        ),
        Step::Key("escape", None),
    ]
    .into_iter()
    .chain(shortcut_hides(
        window,
        "the debug tool window shortcut focuses the shown window",
        "the debug tool window shortcut hides it",
        |b| b.debug_view().is_some(),
    ))
    .collect()
}

/// Replace in Files (SPEC-0066 v2): ⇧⌘R shows the replace field; Replace All changes a closed file
/// on disk and an open one in its editor, unsaved.
fn replace_steps() -> Vec<Step> {
    let key = if cfg!(target_os = "macos") {
        "cmd-shift-r"
    } else {
        "ctrl-shift-r"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("r1.txt"), "alpha beta alpha\n");
                let _ = std::fs::write(dir.join("r2.txt"), "alpha\n");
                b.open(&dir.join("r2.txt"), window, cx);
            }
        })),
        Step::Key(key, None),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.find_in_files() {
                v.update(cx, |v, cx| {
                    v.set_query("alpha", cx);
                    v.set_replacement("omega", cx);
                });
            }
        })),
        Step::WaitFor(
            "replace in files shows the replace field and the matches",
            Box::new(|b, cx| {
                b.find_in_files().is_some_and(|v| {
                    let v = v.read(cx);
                    v.replacing() && v.summary() == "3 matches in 2 files"
                })
            }),
        ),
        Step::Expect(
            "the search, replacement and mask fields are drawn wide and high enough to click into",
            Box::new(|b, cx| {
                b.find_in_files().is_some_and(|v| {
                    let [query, replacement, mask] = v.read(cx).field_sizes(cx);
                    query.0 > 200.0
                        && replacement.0 > 200.0
                        && mask.0 > 80.0
                        && [query, replacement, mask].iter().all(|(_, h)| *h >= 16.0)
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.find_in_files() {
                v.update(cx, |v, cx| v.replace(true, cx));
            }
        })),
        Step::WaitFor(
            "replace all changes the closed file on disk and the open one in its editor",
            Box::new(|b, cx| {
                let dir = FIXTURE.get().expect("fixture");
                std::fs::read_to_string(dir.join("r1.txt")).ok().as_deref()
                    == Some("omega beta omega\n")
                    && std::fs::read_to_string(dir.join("r2.txt")).ok().as_deref()
                        == Some("alpha\n")
                    && b.active_text(cx).as_deref() == Some("omega\n")
                    && b.status_message(cx)
                        .is_some_and(|m| m == "Replaced 3 occurrences in 2 files")
            }),
        ),
    ]
}

/// HTTP client (SPEC-0096): ⌘↩ in a `.http` file sends the request at the caret to a local server
/// and shows the response, JSON indented, in the Services window.
fn http_steps() -> Vec<Step> {
    let execute = if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else {
                return;
            };
            let Ok(port) = listener.local_addr().map(|a| a.port()) else {
                return;
            };
            std::thread::spawn(move || {
                use std::io::{Read, Write};
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    let body = "{\"greeting\":\"hi\"}";
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            });
            if let Some(dir) = FIXTURE.get() {
                // The host comes from the environment file (SPEC-0102).
                let _ = std::fs::write(
                    dir.join("http-client.env.json"),
                    format!("{{\"local\": {{\"host\": \"127.0.0.1:{port}\"}}}}"),
                );
                let _ = std::fs::write(
                    dir.join("api.http"),
                    "### hello\nGET http://{{host}}/hello\nAccept: application/json\n",
                );
                b.open(&dir.join("api.http"), window, cx);
            }
        })),
        Step::Key(execute, None),
        Step::WaitFor(
            "the request at the caret shows its response in the services window",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running()
                        && v.selected_tab() == "Response"
                        && v.environment() == Some("local")
                        && v.response().is_some_and(|r| {
                            r.starts_with("HTTP 200 OK") && r.contains("\"greeting\": \"hi\"")
                        })
                        && v.output_lines()
                            .last()
                            .is_some_and(|l| l.contains("→ 200 OK"))
                })
            }),
        ),
    ]
}

/// Run the file in the editor (SPEC-0097): ⌃⇧R on a Java class with `main` runs it through the
/// JDK's source launcher and shows its output in the Run tool window.
fn run_file_steps() -> Vec<Step> {
    let key = if cfg!(target_os = "macos") {
        "ctrl-shift-r"
    } else {
        "ctrl-shift-f10"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("Hello.java"),
                    "public class Hello {\n    public static void main(String[] args) {\n        System.out.println(\"hello-from-java\");\n    }\n}\n",
                );
                b.open(&dir.join("Hello.java"), window, cx);
            }
        })),
        Step::Key(key, None),
        Step::WaitFor(
            "run file starts the Java class and shows its output",
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).selected_output().is_some_and(|(name, text)| {
                        name == "Hello" && text.contains("hello-from-java")
                    })
                })
            }),
        ),
    ]
}

/// Run the current Python file with the project's environment (SPEC-0050 v2 AK-5): ⌃⇧R on a
/// `.py` file starts `.venv/bin/python` (a stub that prints its name), not `python3` from PATH, and
/// the shown command line names it.
#[cfg(unix)]
fn python_run_file_steps() -> Vec<Step> {
    let key = if cfg!(target_os = "macos") {
        "ctrl-shift-r"
    } else {
        "ctrl-shift-f10"
    };
    vec![
        Step::Setup(Box::new(|b, window, cx| {
            use std::os::unix::fs::PermissionsExt;
            if let Some(dir) = FIXTURE.get() {
                let venv = dir.join(".venv");
                let _ = std::fs::create_dir_all(venv.join("bin"));
                let _ = std::fs::write(venv.join("pyvenv.cfg"), "version = 3.12.4\n");
                let python = venv.join("bin/python");
                let _ = std::fs::write(&python, "#!/bin/sh\necho \"venv-python $*\"\n");
                let _ = std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755));
                let _ = std::fs::write(
                    dir.join("hello_venv.py"),
                    "print('from the system python')\n",
                );
                b.open(&dir.join("hello_venv.py"), window, cx);
            }
        })),
        Step::Key(key, None),
        Step::WaitFor(
            "run file on a Python file starts the project's .venv interpreter and shows it",
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.selected_output().is_some_and(|(name, text)| {
                        name == "hello_venv.py" && text.contains("venv-python hello_venv.py")
                    }) && v
                        .selected_command_line()
                        .is_some_and(|l| l.contains(".venv/bin/python hello_venv.py"))
                })
            }),
        ),
        Step::Setup(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join(".venv"));
                let _ = std::fs::remove_file(dir.join("hello_venv.py"));
            }
        })),
    ]
}

#[cfg(not(unix))]
fn python_run_file_steps() -> Vec<Step> {
    Vec::new()
}

/// Test trees for more runners (SPEC-0045 v3): ⌃⇧R on a Go test file runs `go test -json` for its
/// package and shows the tests as a tree with the failure selected. Skipped without Go.
fn go_test_steps() -> Vec<Step> {
    let go = std::process::Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !go {
        eprintln!("go test steps skipped: go is not installed");
        return Vec::new();
    }
    let key = if cfg!(target_os = "macos") {
        "ctrl-shift-r"
    } else {
        "ctrl-shift-f10"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let module = dir.join("gomod");
                let _ = std::fs::create_dir_all(&module);
                let _ = std::fs::write(module.join("go.mod"), "module gomod\n\ngo 1.21\n");
                let _ = std::fs::write(
                    module.join("calc_test.go"),
                    "package gomod\n\nimport \"testing\"\n\nfunc TestGood(t *testing.T) {}\n\nfunc TestBroken(t *testing.T) { t.Error(\"want 3\") }\n",
                );
                b.open(&module.join("calc_test.go"), window, cx);
            }
        })),
        Step::Key(key, None),
        Step::WaitFor(
            "run file on a Go test shows its tests as a tree with the failure selected",
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).test_results().is_some_and(|t| {
                        t.rows.iter().any(|(_, n)| n.contains("TestGood"))
                            && t.details.as_deref().is_some_and(|d| d.contains("want 3"))
                    })
                })
            }),
        ),
    ]
}

/// Rows of the call stack before a folded row is opened (SPEC-0091 v10 behaviour 15).
static FOLDED_ROWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Debugging Java (SPEC-0097): ⌃⇧D on a class with a breakpoint stops there through java-debug in
/// jdtls with the locals; the next stop, in a record's method, shows the record's fields after the
/// line (SPEC-0091 v8), and resume runs to the end. Skipped without jdtls or the plugin.
fn java_debug_steps() -> Vec<Step> {
    // Debug Current File: ⌃⇧D on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(java_debug_steps_keys())
}

fn java_debug_steps_keys() -> Vec<Step> {
    if !idecraft_languages::java_debug::java_debug_usable()
        || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
    {
        eprintln!("java debug steps skipped: jdtls or java-debug is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (debug, resume) = if mac {
        ("ctrl-shift-d", "cmd-alt-r")
    } else {
        (DEBUG_CURRENT_FILE, "f9")
    };
    let state = |b: &WorkbenchView, cx: &App| {
        b.debug_view().map(|v| {
            let v = v.read(cx);
            (
                v.state().clone(),
                v.frame_names(),
                v.variable_texts(),
                v.console().to_owned(),
            )
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("Dbg.java"),
                    "public class Dbg {\n    record Item(int qty, int unitPriceCents) {\n        int totalCents() {\n            return qty * unitPriceCents;\n        }\n    }\n    static int twice(int x) {\n        int y = x * 2;\n        return y;\n    }\n    public static void main(String[] args) {\n        int n = 21;\n        int r = twice(n);\n        System.out.println(\"result \" + r + \" \" + java.util.stream.Stream.of(new Item(3, 25)).mapToInt(Item::totalCents).sum());\n    }\n}\n",
                );
                b.open(&dir.join("Dbg.java"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| {
                        e.toggle_breakpoint(12, cx);
                        e.toggle_breakpoint(3, cx);
                    });
                }
            }
        })),
        Step::Key(debug, None),
        // A cold jdtls imports the folder first (up to two minutes on a loaded Mac).
        Step::WaitLong(
            "debug class stops a Java program at the breakpoint with its locals",
            std::time::Duration::from_secs(180),
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("main"))
                        && vars.iter().any(|v| v == "n = 21")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "paused in a record's method, the editor shows the fields the line uses",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    e.read(cx)
                        .inline_values()
                        .contains(&(3, "qty: 3  unitPriceCents: 25".to_owned()))
                })
            }),
        ),
        // SPEC-0091 v10: the thread in the selector, the stream's frames folded into one row
        // between the record's method and `main`, shown by a click.
        Step::Expect(
            "the frames show the thread selector and fold the library frames",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    let rows = v.frame_row_texts();
                    v.shown_thread_name().is_some()
                        && rows.first().is_some_and(|r| r.contains("totalCents"))
                        && rows.last().is_some_and(|r| r.contains("main"))
                        && rows.iter().any(|r| r.contains("hidden frame"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| {
                    FOLDED_ROWS.store(
                        v.frame_row_texts().len(),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    let first = v.frame_rows().into_iter().find_map(|r| match r {
                        idecraft_workbench::debug_model::FrameRow::Hidden { first, .. } => {
                            Some(first)
                        }
                        _ => None,
                    });
                    if let Some(first) = first {
                        v.open_hidden(first, cx);
                    }
                });
            }
        })),
        Step::Expect(
            "a click on the folded row shows its frames in its place",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    v.read(cx).frame_row_texts().len()
                        > FOLDED_ROWS.load(std::sync::atomic::Ordering::Relaxed)
                })
            }),
        ),
        // SPEC-0091 v9: the fields the method uses at the top, `this` with the record's text.
        Step::WaitFor(
            "the variables show the used fields at the top and this with its text",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(_, _, vars, _)| {
                    vars.iter().any(|v| v == "qty = 3")
                        && vars.iter().any(|v| v == "unitPriceCents = 25")
                        && vars
                            .iter()
                            .any(|v| v.starts_with("this = ") && v.contains("\"Item[qty=3"))
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume runs the Java program to its end",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, _, _, console)| {
                    matches!(&s, DebugState::Ended(_)) && console.contains("result 42")
                })
            }),
        ),
    ]
}

/// SPEC-0091 v11 with jdtls and java-debug (OBS-0025): object texts of list elements, completion in
/// the expression field, Mark Object, the ⋮ menu, Smart Step Into into the second call of a line and
/// Reset Frame.
fn java_smart_step_steps() -> Vec<Step> {
    if !idecraft_languages::java_debug::java_debug_usable()
        || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
    {
        eprintln!("java smart step steps skipped: jdtls or java-debug is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (resume, smart) = if mac {
        ("cmd-alt-r", "shift-f7")
    } else {
        ("f9", "shift-f7")
    };
    let state = |b: &WorkbenchView, cx: &App| {
        b.debug_view().map(|v| {
            let v = v.read(cx);
            (
                v.state().clone(),
                v.frame_names(),
                v.variable_texts(),
                v.console().to_owned(),
            )
        })
    };
    with_keys_by_find_action(vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("Smart.java"),
                    "public class Smart {\n    record Item(String name, int qty) {}\n    static int twice(int x) {\n        return x * 2;\n    }\n    static int square(int x) {\n        return x * x;\n    }\n    static int add(int a, int b) {\n        return a + b;\n    }\n    public static void main(String[] args) {\n        java.util.List<Item> items = java.util.List.of(new Item(\"pen\", 3));\n        java.util.Map<String, Integer> stock = new java.util.HashMap<>();\n        stock.put(\"k\", 1);\n        int sum = 10;\n        int r = add(twice(sum), square(3));\n        System.out.println(\"smart \" + r + \" \" + items.size() + stock);\n    }\n}\n",
                );
                b.open(&dir.join("Smart.java"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(16, cx));
                    window.focus(&e.read(cx).focus());
                }
            }
        })),
        Step::Key(DEBUG_CURRENT_FILE, None),
        Step::WaitFor(
            "the Java program stops before the line with three calls",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("main"))
                        && vars.iter().any(|v| v == "sum = 10")
                })
            }),
        ),
        // v11 behaviour 17: an element of a list shows the record's own text.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.expand_variable("items", cx));
            }
        })),
        Step::WaitFor(
            "a list element shows its record's text",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(_, _, vars, _)| {
                    vars.iter()
                        .any(|v| v.starts_with("0 = ") && v.contains("Item[name=pen, qty=3]"))
                })
            }),
        ),
        // v11 behaviour 22: completion in the expression field.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| {
                    v.set_expression("stock.ge", cx);
                    v.request_completions(cx);
                });
            }
        })),
        Step::WaitFor(
            "typing stock.ge offers get",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    v.read(cx)
                        .completion_items()
                        .iter()
                        .any(|i| i == "get" || i == "get()")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| {
                    let i = v
                        .completion_items()
                        .iter()
                        .position(|i| i == "get" || i == "get()")
                        .unwrap_or(0);
                    v.accept_completion(i, cx);
                });
            }
        })),
        Step::Expect(
            "taking it completes the word",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).expression_text(cx).starts_with("stock.get"))
            }),
        ),
        // v11 behaviour 24: Mark Object labels the list.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| {
                    v.select_variable("items", cx);
                    if v.start_mark(cx) {
                        v.set_expression("basket", cx);
                        v.evaluate(cx);
                    }
                });
            }
        })),
        Step::Expect(
            "Mark Object puts the label on the object",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.variable_value("items")
                        .is_some_and(|value| v.mark_of(&value) == Some("basket"))
                })
            }),
        ),
        // v11 behaviour 21: the ⋮ menu.
        Step::Act(Box::new(|b, _, cx| {
            b.open_context_menu(
                idecraft_workbench::MenuPlace::Debug(idecraft_workbench::DebugMenu::More),
                None,
                gpui::point(gpui::px(300.), gpui::px(300.)),
                cx,
            );
        })),
        Step::Expect(
            "the ⋮ menu lists the further stepping actions, enabled while paused",
            Box::new(|b, _| {
                // v12 added seven; Reload Changed Classes waits for a change.
                b.context_menu_items().is_some_and(|(_, items)| {
                    items.len() == 15
                        && items
                            .iter()
                            .all(|(l, on)| *on || l == "Reload Changed Classes")
                        && items.iter().any(|(l, _)| l == "Smart Step Into")
                        && items.iter().any(|(l, _)| l == "Reset Frame")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                window.focus(&e.read(cx).focus());
            }
        })),
        // v11 behaviour 18: Smart Step Into offers the three calls; the second one is entered.
        Step::Key(smart, None),
        Step::WaitFor(
            "⇧F7 offers the calls of the line",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(place, items)| {
                    place == idecraft_workbench::MenuPlace::SmartStep
                        && items.len() == 3
                        && items.iter().any(|(l, _)| l.contains("square"))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            let label = b.context_menu_items().and_then(|(_, items)| {
                items
                    .into_iter()
                    .find(|(l, _)| l.starts_with("square"))
                    .map(|(l, _)| l)
            });
            if let Some(label) = label {
                b.choose_context_item(&label, window, cx);
            }
        })),
        Step::WaitFor(
            "choosing square stops in square with x = 3",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("square"))
                        && vars.iter().any(|v| v == "x = 3")
                })
            }),
        ),
        // v11 behaviour 19: Reset Frame runs the frame again; the program stops once more.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.reset_frame(cx));
            }
        })),
        Step::WaitFor(
            "Reset Frame stops again with main below",
            Box::new(move |b, cx| {
                // java-debug names the stop after a reset `restartframe`.
                state(b, cx).is_some_and(|(s, frames, _, console)| {
                    matches!(&s, DebugState::Paused(r) if r.contains("restart"))
                        && frames.last().is_some_and(|f| f.contains("main"))
                        && !console.contains("Reset Frame:")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume runs it to its end",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, _, _, console)| {
                    matches!(&s, DebugState::Ended(_)) && console.contains("smart 29")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.toggle_breakpoint(16, cx));
            }
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("Smart.java"));
            }
        })),
    ])
}

/// The further debugger actions of SPEC-0091 v12 and the breakpoint options of SPEC-0127 v3 in
/// Java (jdtls with java-debug): thread states and the dump, a code fragment, views, Inspect, a
/// coloured mark, a renderer, a filter, a field watchpoint, Step Out of Code Block, hot swap and
/// the console's side bar. Everything set is taken away at the end.
fn java_debug_more_steps() -> Vec<Step> {
    if !idecraft_languages::java_debug::java_debug_usable()
        || idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err()
    {
        eprintln!("java debug more steps skipped: jdtls or java-debug is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (resume, save) = if mac {
        ("cmd-alt-r", "cmd-s")
    } else {
        ("f9", "ctrl-s")
    };
    // `main` waits until the worker sleeps, so the thread list reads it as waiting even when a
    // loaded Mac starts the thread late.
    const SWAP: &str = "public class Swap {\n    static class Counter { int n; }\n    record Item(String name, int qty) {}\n    static int value() {\n        return 1;\n    }\n    public static void main(String[] args) throws Exception {\n        Thread w = new Thread(() -> { try { Thread.sleep(120000); } catch (InterruptedException e) {} }, \"worker-1\");\n        w.setDaemon(true);\n        w.start(); while (w.getState() != Thread.State.TIMED_WAITING) Thread.onSpinWait();\n        java.util.List<Item> items = new java.util.ArrayList<>(java.util.List.of(new Item(\"pen\", 3)));\n        Counter c = new Counter();\n        String none = null;\n        int big = 255;\n        for (int i = 0; i < 3; i++) {\n            c.n = c.n + 1;\n        }\n        for (int k = 0; k < 2; k++) {\n            System.out.println(\"value \" + value() + \" \" + big + none);\n        }\n    }\n}\n";
    let view = |b: &WorkbenchView| b.debug_view();
    let paused = |b: &WorkbenchView, cx: &App| {
        b.debug_view()
            .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Paused(_)))
    };
    with_keys_by_find_action(vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("Swap.java"), SWAP);
                b.open(&dir.join("Swap.java"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(15, cx));
                    window.focus(&e.read(cx).focus());
                }
            }
        })),
        Step::Key(DEBUG_CURRENT_FILE, None),
        // A cold jdtls imports the folder first (up to two minutes on a loaded Mac).
        Step::WaitLong(
            "the Java program stops in the first loop",
            std::time::Duration::from_secs(180),
            Box::new(move |b, cx| {
                paused(b, cx)
                    && view(b).is_some_and(|v| {
                        v.read(cx).variable_texts().iter().any(|t| t == "big = 255")
                    })
            }),
        ),
        // v12 behaviour 28: the thread list with states and groups.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| v.open_thread_list(cx));
            }
        })),
        Step::WaitFor(
            "the thread list shows the worker sleeping in the group main",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    v.read(cx)
                        .thread_infos()
                        .iter()
                        .any(|t| t.name == "worker-1" && t.state == "SLEEPING" && t.group == "main")
                })
            }),
        ),
        // Behaviour 26: the thread dump in the console.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| v.thread_dump(cx));
            }
        })),
        Step::WaitFor(
            "Get Thread Dump writes the threads with their frames to the console",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let c = v.read(cx).console().to_owned();
                    c.contains("Thread dump:")
                        && c.contains("\"worker-1\" SLEEPING (group main)")
                        && c.contains("\tat Swap.main(String[]) (Swap.java:16)")
                })
            }),
        ),
        // Behaviour 36: a code fragment of several statements.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.toggle_fragment(cx);
                    v.set_expression("int s = 0;\nfor (Item it : items) s += it.qty();\ns", cx);
                    v.evaluate(cx);
                });
            }
        })),
        Step::WaitFor(
            "the fragment gives the sum of the quantities",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    v.read(cx)
                        .result_texts()
                        .first()
                        .is_some_and(|t| t.ends_with(" = 3"))
                })
            }),
        ),
        // Behaviour 32: Hex and String views.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.toggle_fragment(cx);
                    v.select_variable("big", cx);
                    v.set_view_selected(idecraft_debugger::ViewAs::Hex, cx);
                    v.select_variable("items", cx);
                    v.set_view_selected(idecraft_debugger::ViewAs::Text, cx);
                });
            }
        })),
        Step::WaitFor(
            "big shows as 0xFF and the list as its text",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.variable_value("big").as_deref() == Some("0xFF")
                        && v.variable_value("items")
                            .is_some_and(|t| t.contains("Item[name=pen, qty=3]"))
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.select_variable("big", cx);
                    v.set_view_selected(idecraft_debugger::ViewAs::Auto, cx);
                    v.select_variable("items", cx);
                    v.set_view_selected(idecraft_debugger::ViewAs::Auto, cx);
                    // Behaviour 30: Inspect the list in a tab of its own.
                    v.inspect_selected(false, cx);
                });
            }
        })),
        Step::WaitFor(
            "Inspect shows the list with its element",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.inspect_titles() == ["Inspect items"]
                        && v.inspect_texts(0).iter().any(|t| t.starts_with("0 = "))
                })
            }),
        ),
        // Behaviour 35: a mark with a colour; behaviour 34: null values hidden.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.close_inspect(0, cx);
                    v.select_variable("c", cx);
                    if v.start_mark(cx) {
                        v.set_mark_color(3, cx);
                        v.set_expression("counter", cx);
                        v.evaluate(cx);
                    }
                    v.toggle_filter("nulls", cx);
                });
            }
        })),
        Step::Expect(
            "the counter is marked in blue and the null value is hidden",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.variable_value("c").is_some_and(|value| {
                        v.mark_of(&value) == Some("counter") && v.mark_color_of(&value) == Some(3)
                    }) && v.variable_value("none").is_none()
                })
            }),
        ),
        // Behaviour 33: a renderer for the record type.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.toggle_filter("nulls", cx);
                    v.expand_variable("items", cx);
                });
            }
        })),
        Step::WaitFor(
            "the list's element is shown",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| v.read(cx).variable_value("0").is_some())
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.select_variable("0", cx);
                    if v.start_renderer(cx) {
                        v.set_expression("this.name() + \" x\" + this.qty()", cx);
                        v.evaluate(cx);
                    }
                });
            }
        })),
        Step::WaitFor(
            "the element shows the renderer's text",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    v.read(cx)
                        .variable_value("0")
                        .is_some_and(|t| t.contains("pen x3"))
                })
            }),
        ),
        // SPEC-0146 v3: a field watchpoint from the variables.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    let kinds: Vec<String> =
                        v.renderers().iter().map(|r| r.type_name.clone()).collect();
                    for k in kinds {
                        v.set_renderer(&k, "", cx);
                    }
                    v.expand_variable("c", cx);
                });
            }
        })),
        Step::WaitFor(
            "the counter's field is shown",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| v.read(cx).variable_value("n").is_some())
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.select_variable("n", cx);
                    v.add_field_watch_selected(cx);
                });
            }
        })),
        Step::WaitFor(
            "Add Field Watchpoint saves the field's watchpoint",
            Box::new(|_, _| {
                FIXTURE.get().is_some_and(|d| {
                    idecraft_debugger::saved::load_special_breakpoints(d)
                        .fields
                        .iter()
                        .any(|f| f.field == "Swap$Counter#n")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.toggle_breakpoint(15, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key(resume, None),
        Step::WaitFor(
            "the watchpoint stops where the field is written",
            Box::new(move |b, cx| {
                view(b).is_some_and(
                    |v| matches!(v.read(cx).state(), DebugState::Paused(r) if r.contains("data")),
                )
            }),
        ),
        // Behaviour 25: Step Out of Code Block leaves the loop.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                idecraft_debugger::saved::save_special_breakpoints(dir, &Default::default());
            }
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.command(
                        idecraft_workbench::debug_model::Command::Special(Default::default()),
                        cx,
                    );
                    v.step_out_of_block(cx);
                });
            }
        })),
        Step::WaitFor(
            "Step Out of Code Block stops on the first line after the loop",
            Box::new(move |b, cx| {
                paused(b, cx)
                    && b.active_debug_marks(cx)
                        .is_some_and(|(_, exec)| exec == Some(17))
            }),
        ),
        // Behaviours 21 and 37: the ⋮ menu's further items; Reload waits for a change.
        Step::Act(Box::new(|b, _, cx| {
            b.open_context_menu(
                idecraft_workbench::MenuPlace::Debug(idecraft_workbench::DebugMenu::More),
                None,
                gpui::point(gpui::px(300.), gpui::px(300.)),
                cx,
            );
        })),
        Step::Expect(
            "the ⋮ menu lists the further actions",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(_, items)| {
                    items.len() == 15
                        && items
                            .iter()
                            .any(|(l, on)| l == "Step Out of Code Block" && *on)
                        && items
                            .iter()
                            .any(|(l, on)| l == "Reload Changed Classes" && !*on)
                })
            }),
        ),
        Step::Key("escape", None),
        // Behaviour 37: hot swap after saving a changed method.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    e.set_text(&SWAP.replace("return 1;", "return 2;"), cx)
                });
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key(save, None),
        Step::WaitFor(
            "saving the change offers Reload Changed Classes",
            Box::new(move |b, cx| view(b).is_some_and(|v| !v.read(cx).code_changed().is_empty())),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| v.reload_classes(cx));
            }
        })),
        Step::WaitFor(
            "the changed class is reloaded",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    // java-debug names every class built from the saved file, also the unchanged
                    // nested ones (`Swap$Counter`, `Swap$Item`).
                    let c = v.read(cx).console().to_owned();
                    c.lines().any(|l| {
                        l.starts_with("[IDECraft] Reloaded ")
                            && l.split([':', ',']).any(|n| n.trim() == "Swap")
                    })
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "the program runs the new code to its end",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Ended(_)) && v.console().contains("value 2 255")
                })
            }),
        ),
        // Behaviour 38: Up the Stack Trace opens the dump's frame; Soft-Wrap; Clear All.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.stack_step(true, cx);
                    v.toggle_soft_wrap(cx);
                });
            }
        })),
        Step::Expect(
            "Up the Stack Trace marks the dump's frame line, Soft-Wrap is off",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.stack_line().is_some() && !v.soft_wrap()
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.toggle_soft_wrap(cx);
                    v.clear_console(cx);
                });
            }
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("Swap.java"));
            }
        })),
        Step::Expect(
            "Clear All empties the console",
            Box::new(move |b, cx| view(b).is_some_and(|v| v.read(cx).console().is_empty())),
        ),
    ])
}

/// The breakpoint options of SPEC-0127 v3 in the Breakpoints dialog: suspend policy, messages,
/// removal after the hit and a class filter are saved with the breakpoint; the dialog groups the
/// breakpoints by kind. Taken away at the end.
fn breakpoint_option_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("opts.js"), "let n = 0;\nn += 1;\nn += 2;\n");
                b.open(&dir.join("opts.js"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(1, cx));
                }
            }
        })),
        // The workbench hears of the new breakpoint after the frame, so the dialog opens in the
        // next step (opened at once it would select another breakpoint).
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                b.open_breakpoints(Some((dir.join("opts.js"), 1)), window, cx);
            }
            if let Some(d) = b.breakpoint_dialog() {
                d.update(cx, |d, cx| {
                    d.toggle_rule("suspend", cx);
                    d.toggle_rule("log_hit", cx);
                    d.toggle_rule("remove_once_hit", cx);
                    d.set_field("Class filter", "Main* -Main.Test", cx);
                    d.done(cx);
                });
            }
        })),
        Step::Expect(
            "the further options are saved with the breakpoint",
            Box::new(|_, _| {
                FIXTURE.get().is_some_and(|d| {
                    std::fs::read_to_string(d.join(".idecraft/breakpoints.toml")).is_ok_and(|t| {
                        t.contains("suspend = \"all\"")
                            && t.contains("log_hit = true")
                            && t.contains("remove_once_hit = true")
                            && t.contains("class_filter = \"Main* -Main.Test\"")
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                b.open_breakpoints(Some((dir.join("opts.js"), 1)), window, cx);
            }
        })),
        Step::Expect(
            "the dialog reads the options back",
            Box::new(|b, cx| {
                b.breakpoint_dialog().is_some_and(|d| {
                    let r = d.read(cx).rules().clone();
                    r.suspend == Some(idecraft_debugger::Suspend::All)
                        && r.log_hit
                        && r.remove_once_hit
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(d) = b.breakpoint_dialog() {
                d.update(cx, |d, cx| {
                    d.remove_selected(cx);
                    d.done(cx);
                });
            }
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("opts.js"));
            }
        })),
    ]
}

/// Stands for Debug Current File (action `DebugClass`) in a list of steps; see
/// [`with_keys_by_find_action`].
const DEBUG_CURRENT_FILE: &str = "<debug current file>";

/// Stands for Reopen Closed Tab (action `ReopenClosedTab`) in a list of steps; see
/// [`with_keys_by_find_action`].
const REOPEN_CLOSED_TAB: &str = "<reopen closed tab>";

/// Stands for Next Splitter (action `NextSplitter`) in a list of steps; see
/// [`with_keys_by_find_action`].
const NEXT_SPLITTER: &str = "<next splitter>";

/// The actions the Linux keymap (the reference's default keymap) gives no shortcut: the stand-in
/// key, the macOS key, the action id and the check that Find Action has selected it. `DebugClass`:
/// ⌃⇧F9 there is Recompile. `ReopenClosedTab`: ⌃⇧T is Go to Test and ⌃⌥⇧T is Refactor This
/// (SPEC-0059 v9.9 behaviour 22b). `NextSplitter`: only the macOS keymap binds it (⌥⇥).
const BY_FIND_ACTION: [(&str, &str, &str, &str); 3] = [
    (
        NEXT_SPLITTER,
        "alt-tab",
        "NextSplitter",
        "Find Action selects Next Splitter",
    ),
    (
        DEBUG_CURRENT_FILE,
        "ctrl-shift-d",
        "DebugClass",
        "Find Action selects Debug Current File",
    ),
    (
        REOPEN_CLOSED_TAB,
        "cmd-shift-t",
        "ReopenClosedTab",
        "Find Action selects Reopen Closed Tab",
    ),
];

/// Debug Current File and Reopen Closed Tab from the keyboard: their macOS keys on macOS. On Linux
/// they have no shortcut, so the Linux variant opens Find Action, finds the action by its id, checks
/// that it is selected and runs it with Enter. Replaces every stand-in key of [`BY_FIND_ACTION`]
/// in `steps`.
fn with_keys_by_find_action(steps: Vec<Step>) -> Vec<Step> {
    let mut out = Vec::with_capacity(steps.len());
    for step in steps {
        match step {
            Step::Key(key, _) if BY_FIND_ACTION.iter().any(|(k, ..)| *k == key) => {
                let Some(&(_, mac, id, check)) = BY_FIND_ACTION.iter().find(|(k, ..)| *k == key)
                else {
                    continue;
                };
                if cfg!(target_os = "macos") {
                    out.push(Step::Key(mac, None));
                    continue;
                }
                out.push(Step::Key("ctrl-shift-a", None));
                out.extend(id.chars().map(|ch| {
                    let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
                    Step::Key(s, Some(s))
                }));
                out.push(Step::WaitFor(
                    check,
                    Box::new(move |b, cx| {
                        b.everywhere().is_some_and(|v| {
                            matches!(
                                v.read(cx).results().selected_row(),
                                Some(idecraft_workbench::everywhere_model::Row::Action(a))
                                    if a.id == id
                            )
                        })
                    }),
                ));
                out.push(Step::Key("enter", None));
            }
            step => out.push(step),
        }
    }
    out
}

/// The Python half of SPEC-0091 v12: the console's input runs statements in the paused frame,
/// Step Out of Code Block leaves a loop, and Force Return is refused with a reason.
fn python_debug_more_steps() -> Vec<Step> {
    let has_debugpy = idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some());
    if !has_debugpy {
        eprintln!("python debug more steps skipped: debugpy is missing");
        return Vec::new();
    }
    let view = |b: &WorkbenchView| b.debug_view();
    with_keys_by_find_action(vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("loop.py"),
                    "def main():\n    n = 0\n    for i in range(3):\n        n = n + i\n    print(\"after\", n)\n\nmain()\n",
                );
                b.open(&dir.join("loop.py"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(3, cx));
                    window.focus(&e.read(cx).focus());
                }
            }
        })),
        Step::Key(DEBUG_CURRENT_FILE, None),
        Step::WaitFor(
            "the Python loop stops",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Paused(_))
                        && v.variable_texts().iter().any(|t| t == "n = 0")
                })
            }),
        ),
        // Behaviour 38: the console's input line runs statements in the frame.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.set_console_input("print('from console', n + 40)", cx);
                    v.submit_console_input(cx);
                });
            }
        })),
        Step::WaitFor(
            "the printed text appears in the console",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| v.read(cx).console().contains("from console 40"))
            }),
        ),
        // Behaviour 29: Force Return is refused with a reason.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| v.force_return(cx));
            }
        })),
        Step::WaitFor(
            "Force Return says why it cannot",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| {
                    v.read(cx)
                        .console()
                        .contains("The debug adapter cannot return from a method early")
                })
            }),
        ),
        // Behaviour 25: out of the loop, without the breakpoint inside it.
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.toggle_breakpoint(3, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        // The session hears of the removed breakpoint after the frame; stepping in the same step
        // would stop on it again.
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| v.step_out_of_block(cx));
            }
        })),
        Step::WaitFor(
            "Step Out of Code Block stops on the print after the loop",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| v.read(cx).variable_texts().iter().any(|t| t == "n = 3"))
                    && b.active_debug_marks(cx)
                        .is_some_and(|(_, exec)| exec == Some(4))
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = view(b) {
                v.update(cx, |v, cx| {
                    v.command(idecraft_workbench::debug_model::Command::Continue, cx);
                });
            }
        })),
        Step::WaitFor(
            "the script ends",
            Box::new(move |b, cx| {
                view(b).is_some_and(|v| matches!(v.read(cx).state(), DebugState::Ended(_)))
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("loop.py"));
            }
        })),
    ])
}

/// Column selection (SPEC-0130): the rectangle an ⌥-drag spans selects the same columns on every
/// line, short lines get a caret at their end, and typing replaces each part.
fn column_select_steps() -> Vec<Step> {
    let close = if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-f4"
    };
    let save = if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("columns.txt"), "alpha one\nbe\ngamma two\n");
                b.open(&dir.join("columns.txt"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| {
                        e.column_select(
                            idecraft_editor::Position::new(0, 6),
                            idecraft_editor::Position::new(2, 9),
                            cx,
                        )
                    });
                }
            }
        })),
        Step::Expect(
            "the rectangle gives one part per line, a caret at the end of the short line",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    let parts: Vec<_> = e.state.carets.carets().iter().map(|c| c.range()).collect();
                    parts == [6..9, 12..12, 19..22]
                })
            }),
        ),
        Step::Key("X", Some("X")),
        Step::Expect(
            "typing replaces every part",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t == "alpha X\nbeX\ngamma X\n")
            }),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("columns.txt"));
            }
        })),
    ]
}

/// Functions of open plugins (SPEC-0155): Update Copyright writes the project's notice under the
/// shebang; a properties file shows its duplicate key and trailing spaces, ⌥↩ removes the spaces;
/// a repeated word in a text file is found and ⌥↩ removes it.
fn open_plugin_feature_steps() -> Vec<Step> {
    let (save, close) = if cfg!(target_os = "macos") {
        ("cmd-s", "cmd-w")
    } else {
        ("ctrl-s", "ctrl-f4")
    };
    let caret = |line: usize, column: usize| -> Step {
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(line, column, cx));
                window.focus(&e.read(cx).focus());
            }
        }))
    };
    let year = idecraft_languages::copyright::current_year();
    let notice: &'static str = Box::leak(
        format!("#!/bin/sh\n# Copyright (c) {year} Selftest\n\necho hi\n").into_boxed_str(),
    );
    let open = |name: &'static str, text: &'static str| -> Step {
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::write(d.join(name), text);
                b.open(&d.join(name), window, cx);
            }
        }))
    };
    let problem = |b: &WorkbenchView, cx: &App, what: &str| {
        b.active_editor_entity().is_some_and(|e| {
            e.read(cx)
                .problems()
                .iter()
                .any(|p| p.message.contains(what))
        })
    };
    vec![
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::create_dir_all(d.join(".idecraft"));
                let _ = std::fs::write(
                    d.join(".idecraft/copyright.txt"),
                    "Copyright (c) $today.year Selftest\n",
                );
            }
        })),
        open("notice.sh", "#!/bin/sh\necho hi\n"),
        Step::Act(Box::new(|b, _, cx| b.update_copyright(cx))),
        Step::Expect(
            "Update Copyright writes the notice under the shebang",
            Box::new(move |b, cx| b.active_text(cx).is_some_and(|t| t == notice)),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        open("msg.properties", "greeting=Hi  \ngreeting=Hello\n"),
        Step::WaitFor(
            "the properties file shows the duplicate key and the trailing spaces",
            Box::new(move |b, cx| {
                problem(b, cx, "Duplicate property key `greeting`")
                    && problem(b, cx, "Trailing spaces")
            }),
        ),
        caret(0, 12),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ offers to remove the trailing spaces",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str)
                        == Some("Remove trailing spaces")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter removes them",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t == "greeting=Hi\ngreeting=Hello\n")
            }),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        open("notes.txt", "Open the the file.\n"),
        Step::WaitFor(
            "a repeated word in a text file is found",
            Box::new(move |b, cx| problem(b, cx, "Repeated word")),
        ),
        caret(0, 9),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ offers to remove it",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str)
                        == Some("Remove the repeated word")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter removes the repeated word",
            Box::new(|b, cx| b.active_text(cx).is_some_and(|t| t == "Open the file.\n")),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                for f in [
                    "notice.sh",
                    "msg.properties",
                    "notes.txt",
                    ".idecraft/copyright.txt",
                ] {
                    let _ = std::fs::remove_file(d.join(f));
                }
            }
        })),
    ]
}

/// Column selection mode (SPEC-0151): ⇧⌘8 switches it on and the status bar says so; ⇧-arrows
/// stretch a rectangle over a short line; typing replaces every part; ⇧⌘8 switches it off.
fn column_mode_steps() -> Vec<Step> {
    let (toggle, save, close) = if cfg!(target_os = "macos") {
        ("cmd-shift-8", "cmd-s", "cmd-w")
    } else {
        ("alt-shift-insert", "ctrl-s", "ctrl-f4")
    };
    let mode = |b: &WorkbenchView, cx: &App| {
        b.active_editor_entity()
            .is_some_and(|e| e.read(cx).column_mode())
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("colmode.txt"), "alpha one\nbe\ngamma two\n");
                b.open(&dir.join("colmode.txt"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.go_to(0, 6, cx));
                }
            }
        })),
        Step::Key(toggle, None),
        Step::Expect(
            "⇧⌘8 switches column selection mode on",
            Box::new(move |b, cx| mode(b, cx)),
        ),
        Step::Key("shift-down", None),
        Step::Key("shift-down", None),
        Step::Key("shift-right", None),
        Step::Key("shift-right", None),
        Step::Key("shift-right", None),
        Step::Expect(
            "⇧-arrows select a rectangle with a caret at the end of the short line",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    let parts: Vec<_> = e.state.carets.carets().iter().map(|c| c.range()).collect();
                    parts == [6..9, 12..12, 19..22]
                })
            }),
        ),
        Step::Key("X", Some("X")),
        Step::Expect(
            "typing replaces every part of the rectangle",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t == "alpha X\nbeX\ngamma X\n")
            }),
        ),
        Step::Key(toggle, None),
        Step::Expect(
            "⇧⌘8 again switches it off",
            Box::new(move |b, cx| !mode(b, cx)),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("colmode.txt"));
            }
        })),
    ]
}

/// The Java file of the folding steps: a header comment 0–3, imports 4–6, a documentation
/// comment 8–11, the class body 12–34 with a custom region 13–16, line comments 18–19, a one-line
/// method 20–22 and `add` 24–33 with an `if` 25–28 (a `for` 26–28 in it) and an `else` 29–31.
const FOLDING_JAVA: &str = "/*\n * Header comment\n * over three lines.\n */\nimport java.util.ArrayList;\nimport java.util.List;\nimport java.util.Map;\n\n/**\n * A class to fold.\n * Second line of the doc.\n */\npublic class Folding {\n    // region Fields\n    private final List<String> names = new ArrayList<>();\n    private int count;\n    // endregion\n\n    // one\n    // two\n    public int getCount() {\n        return count;\n    }\n\n    public void add(String name) {\n        if (name != null) {\n            for (int i = 0; i < 2; i++) {\n                names.add(name + i);\n            }\n        } else {\n            count--;\n        }\n        count++;\n    }\n}\n";

/// Folding (SPEC-0083 v8 AK-28), without a language server: the imports and the one-line method
/// are folded when the file opens; the folding arrows of open regions come with the pointer and
/// go with it; ⇧⌘- folds everything but the class body and the folded lines read `/*...*/`,
/// `import ...`, the first sentence, the region's name, `//...`, `{...}`; ⌥⌘* 1 folds the `if` and
/// the `else`, ⌥⌘* 2 the `for`; ⇧⌘= opens everything.
fn folding_steps() -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let (collapse_all, expand_all, all_to_level) = if mac {
        ("cmd-shift--", "cmd-shift-=", "cmd-alt-*")
    } else {
        ("ctrl-shift--", "ctrl-shift-=", "ctrl-shift-*")
    };
    let arrows = |b: &WorkbenchView, cx: &App| {
        b.active_editor_entity()
            .map(|e| e.read(cx).painted_fold_arrows())
            .unwrap_or_default()
    };
    let drawn = |b: &WorkbenchView, cx: &App| -> Vec<(usize, String, String)> {
        b.active_editor_entity()
            .map(|e| e.read(cx).painted_placeholders().to_vec())
            .unwrap_or_default()
    };
    let all = [
        (0, 3),
        (4, 6),
        (8, 11),
        (13, 16),
        (18, 19),
        (20, 22),
        (24, 33),
    ];
    vec![
        Step::Setup(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("Folding.java"), FOLDING_JAVA);
                b.open(&dir.join("Folding.java"), window, cx);
            }
        })),
        Step::WaitFor(
            "the imports and the one-line method are folded when the file opens",
            Box::new(|b, cx| b.active_folds(cx).is_some_and(|f| f == [(4, 6), (20, 22)])),
        ),
        // Two spots for the pointer: on the first character of the text, and above the editor.
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                let e = e.read(cx);
                if let Some(p) = e.point_on_char(0) {
                    let (x, y) = (f32::from(p.x), f32::from(p.y));
                    let spot = |y| idecraft_ui::tool_windows::Rect { x, y, w: 1., h: 1. };
                    idecraft_workbench::hit_targets::set("folding-inside", spot(y));
                    idecraft_workbench::hit_targets::set("folding-away", spot(y - 80.));
                }
            }
        })),
        hover("folding-away"),
        Step::WaitFor(
            "without the pointer over the editor only folded regions have a folding arrow",
            Box::new(move |b, cx| arrows(b, cx) == [(4, true), (20, true)]),
        ),
        hover("folding-inside"),
        Step::WaitFor(
            "with the pointer over the editor every region has its folding arrow",
            Box::new(move |b, cx| {
                let shown = arrows(b, cx);
                [
                    (0, false),
                    (4, true),
                    (8, false),
                    (12, false),
                    (13, false),
                    (18, false),
                    (20, true),
                    (24, false),
                    (25, false),
                    (29, false),
                ]
                .iter()
                .all(|a| shown.contains(a))
            }),
        ),
        hover("folding-away"),
        Step::WaitFor(
            "the arrows of open regions go when the pointer leaves the editor",
            Box::new(move |b, cx| arrows(b, cx) == [(4, true), (20, true)]),
        ),
        Step::Key(collapse_all, None),
        Step::WaitFor(
            "collapse all folds everything in the class and leaves the class body open",
            Box::new(move |b, cx| b.active_folds(cx).is_some_and(|f| f == all)),
        ),
        Step::WaitFor(
            "folded lines show what they hide: comment signs, the keyword, the first sentence, the region's name, both braces",
            Box::new(move |b, cx| {
                let shown: Vec<(String, String)> = drawn(b, cx)
                    .into_iter()
                    .map(|(_, text, placeholder)| (text, placeholder))
                    .collect();
                let want = [
                    ("/*", "...*/"),
                    ("import ", "..."),
                    ("/**", " A class to fold. ...*/"),
                    ("    ", "Fields"),
                    ("    //", "..."),
                    ("    public int getCount() {", " return count; }"),
                    ("    public void add(String name) {", "...}"),
                ];
                shown.len() == want.len()
                    && shown
                        .iter()
                        .zip(want)
                        .all(|((text, placeholder), (t, p))| text == t && placeholder == p)
            }),
        ),
        Step::Key(all_to_level, None),
        Step::Key("1", Some("1")),
        Step::WaitFor(
            "all to level 1 folds the blocks directly in the method, the if and the else each",
            Box::new(|b, cx| {
                b.active_folds(cx)
                    .is_some_and(|f| f == [(25, 28), (29, 31)])
            }),
        ),
        Step::Key(all_to_level, None),
        Step::Key("2", Some("2")),
        Step::WaitFor(
            "all to level 2 folds the loop in the if",
            Box::new(|b, cx| b.active_folds(cx).is_some_and(|f| f == [(26, 28)])),
        ),
        Step::Key(expand_all, None),
        Step::WaitFor(
            "expand all opens everything; the text is unchanged",
            Box::new(|b, cx| {
                b.active_folds(cx).is_some_and(|f| f.is_empty())
                    && b.active_text(cx).as_deref() == Some(FOLDING_JAVA)
            }),
        ),
        Step::Key(if mac { "cmd-w" } else { "ctrl-f4" }, None),
        Step::Setup(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("Folding.java"));
            }
        })),
    ]
}

/// The Editor, Font, Build Tools, Languages and Terminal settings pages (SPEC-0129): the font size
/// changes every editor at once and is saved; saving removes trailing spaces and adds the final
/// line break when switched on; the shell is kept; the information pages list JDKs and servers.
/// EditorConfig (SPEC-0137): a `.txt` file indents with tabs of width 8; saving trims trailing
/// spaces and adds the final line break; saving a changed `.editorconfig` switches the open editor
/// to two spaces.
fn editorconfig_steps() -> Vec<Step> {
    let (save, close) = if cfg!(target_os = "macos") {
        ("cmd-s", "cmd-w")
    } else {
        ("ctrl-s", "ctrl-f4")
    };
    fn dir() -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join("ec"))
    }
    let editor_of = |b: &WorkbenchView, name: &str, cx: &App| {
        dir().and_then(|d| b.editor_for(&d.join(name), cx))
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = dir() {
                let _ = std::fs::create_dir_all(&d);
                let _ = std::fs::write(
                    d.join(".editorconfig"),
                    "root = true\n\n[*.txt]\nindent_style = tab\ntab_width = 8\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
                );
                let _ = std::fs::write(d.join("notes.txt"), "x\ny  ");
                b.open(&d.join("notes.txt"), window, cx);
            }
        })),
        Step::Expect(
            "the .txt file indents with a tab of width 8",
            Box::new(move |b, cx| {
                editor_of(b, "notes.txt", cx).is_some_and(|e| {
                    let e = e.read(cx);
                    e.state.indent == "\t" && e.tab_width() == 8
                })
            }),
        ),
        Step::Key("tab", None),
        Step::Key(save, None),
        Step::WaitFor(
            "saving trims the trailing spaces and adds the final line break",
            Box::new(|_, _| {
                dir().and_then(|d| std::fs::read_to_string(d.join("notes.txt")).ok())
                    == Some("\tx\ny\n".into())
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = dir() {
                b.open(&d.join(".editorconfig"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| {
                        e.set_text(
                            "root = true\n\n[*.txt]\nindent_style = space\nindent_size = 2\n",
                            cx,
                        )
                    });
                }
            }
        })),
        Step::Key(save, None),
        Step::WaitFor(
            "saving .editorconfig switches the open editor to two spaces",
            Box::new(move |b, cx| {
                editor_of(b, "notes.txt", cx).is_some_and(|e| e.read(cx).state.indent == "  ")
            }),
        ),
        Step::Key(close, None),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = dir() {
                b.open(&d.join("notes.txt"), window, cx);
            }
        })),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = dir() {
                let _ = std::fs::remove_dir_all(d);
            }
        })),
    ]
}

/// AI Assistant (SPEC-0138) against a fake OpenAI-compatible endpoint on the loopback address:
/// switched on in Settings › Tools › AI Assistant, Explain Code sends the selection, a follow-up
/// question carries the conversation, Escape hides the chat; switched off again at the end.
fn ai_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    static FAKE: std::sync::OnceLock<Option<idecraft_ai::fake::FakeEndpoint>> =
        std::sync::OnceLock::new();
    fn fake() -> Option<&'static idecraft_ai::fake::FakeEndpoint> {
        FAKE.get_or_init(|| idecraft_ai::fake::start("It adds two numbers.", 200).ok())
            .as_ref()
    }
    fn bodies() -> Vec<String> {
        fake()
            .and_then(|f| {
                f.seen.lock().ok().map(|s| {
                    s.iter()
                        .filter(|x| x.line.contains("/chat/completions"))
                        .map(|x| x.body.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }
    let chat = |b: &WorkbenchView, cx: &App| {
        b.ai_chat_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    };
    const CODE: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
    vec![
        Step::Act(Box::new(|b, window, cx| {
            let Some(f) = fake() else { return };
            b.open_settings(Page::Ai, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_ai(cx);
                    v.set_ai_fields(&f.endpoint, "test-model", cx);
                    v.apply_ai(cx);
                });
            }
        })),
        Step::Expect(
            "the AI Assistant page switches it on with the endpoint and model",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let ai = &v.read(cx).ide().ai;
                    ai.enabled
                        && ai.model == "test-model"
                        && ai.endpoint.starts_with("http://127.0.0.1:")
                })
            }),
        ),
        Step::WaitFor(
            "applying lists the endpoint's models without choosing one (SPEC-0143)",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.note()
                        .is_some_and(|n| n.contains("models: model-a, model-b"))
                        && v.ide().ai.model == "test-model"
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                // The three subscriptions (SPEC-0160), then the first template.
                v.update(cx, |v, cx| (0..4).for_each(|_| v.cycle_provider(cx)));
            }
        })),
        Step::Expect(
            "the Provider row fills in a template's endpoint",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    v.read(cx).note().is_some_and(|n| {
                        n.starts_with("OpenAI: endpoint https://api.openai.com/v1")
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            let Some(f) = fake() else { return };
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.set_ai_fields(&f.endpoint, "test-model", cx));
            }
        })),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::write(d.join("ai_add.rs"), CODE);
                b.open(&d.join("ai_add.rs"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.select_range(0..CODE.chars().count(), cx));
                }
                b.ai_explain(window, cx);
            }
        })),
        Step::WaitFor(
            "Explain Code sends the selection and shows the answer",
            Box::new(move |b, cx| {
                let t = chat(b, cx);
                t.iter().any(|l| l == "AI: It adds two numbers.")
                    && !b.ai_chat_view().is_some_and(|v| v.read(cx).busy())
                    && t.iter().any(|l| l.starts_with("You: ai_add.rs:1-4:"))
                    && bodies().first().is_some_and(|body| {
                        body.contains("fn add(a: i32, b: i32)")
                            && body.contains("\"model\":\"test-model\"")
                            && body.contains("Explain what this code does")
                    })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.ai_chat_view() {
                v.update(cx, |v, cx| v.set_question("And with floats?", cx));
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "a follow-up question carries the conversation",
            Box::new(move |b, cx| {
                chat(b, cx).iter().filter(|l| l.starts_with("AI: ")).count() == 2
                    && !b.ai_chat_view().is_some_and(|v| v.read(cx).busy())
                    && bodies().get(1).is_some_and(|body| {
                        body.contains("And with floats?") && body.contains("It adds two numbers.")
                    })
            }),
        ),
        Step::Expect(
            "answers are asked for as a stream (SPEC-0157)",
            Box::new(|_, _| bodies().iter().all(|body| body.contains("\"stream\":true"))),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape hides the chat",
            Box::new(|b, _| b.ai_chat_view().is_none()),
        ),
        // Inline completion (SPEC-0157): switched on, a pause after typing shows the endpoint's
        // suggestion as grey text, Tab inserts it.
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::Ai, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.toggle_inline_completion(cx));
            }
        })),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::write(d.join("ai_inline.rs"), "fn main() {\n    let x =\n}\n");
                b.open(&d.join("ai_inline.rs"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.go_to(1, 11, cx));
                    window.focus(&e.read(cx).focus());
                }
            }
        })),
        Step::Key("space", Some(" ")),
        Step::WaitFor(
            "after a pause the suggestion is shown at the caret",
            Box::new(|b, cx| {
                b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).ghost() == Some("It adds two numbers."))
                    && bodies().iter().any(|body| body.contains("<CURSOR>"))
            }),
        ),
        // SPEC-0162: ⌥] asks for a different suggestion; the fake endpoint repeats itself, which
        // is no alternative, so the shown one stays.
        Step::Key("alt-]", None),
        Step::WaitFor(
            "⌥] asks for another suggestion and keeps the shown one when none differs",
            Box::new(|b, cx| {
                b.status_note()
                    .is_some_and(|s| s.contains("No other suggestion"))
                    && bodies()
                        .iter()
                        .any(|body| body.contains("different completion"))
                    && b.inline_suggestions()
                        .is_some_and(|(s, i)| s.len() == 1 && i == 0)
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).ghost() == Some("It adds two numbers."))
            }),
        ),
        Step::Key("tab", None),
        Step::Expect(
            "Tab inserts it",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t.contains("let x = It adds two numbers.\n"))
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).ghost().is_none())
            }),
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-s"
            } else {
                "ctrl-s"
            },
            None,
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-f4"
            },
            None,
        ),
        // SPEC-0160: the provider list starts with the chat subscriptions; choosing one asks its
        // program whether the user is signed in (the answer depends on this machine).
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::Ai, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.cycle_provider(cx));
            }
        })),
        Step::WaitFor(
            "a subscription can be chosen as the provider and its sign-in is checked",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.ide().ai.subscription == "claude" && v.ai_login().is_some()
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                // Codex, Gemini CLI, then the first API template, which uses the endpoint again.
                v.update(cx, |v, cx| (0..3).for_each(|_| v.cycle_provider(cx)));
            }
        })),
        Step::Expect(
            "after the subscriptions come the API templates",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).ide().ai.subscription.is_empty())
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::Ai, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_inline_completion(cx);
                    v.toggle_ai(cx);
                    v.set_ai_fields("", "", cx);
                    v.apply_ai(cx);
                });
            }
        })),
        Step::Expect(
            "switched off again, without endpoint",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let ai = &v.read(cx).ide().ai;
                    !ai.enabled && ai.endpoint.is_empty() && !ai.inline_completion
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::remove_file(d.join("ai_add.rs"));
                let _ = std::fs::remove_file(d.join("ai_inline.rs"));
            }
        })),
    ]
}

/// SPEC-0313: terminal output in the AI chat, with consent. The terminal's context menu offers
/// the entries; the last command becomes a chip with its exit code and a masked fake token;
/// Explain with AI prefills the question without sending; Enter sends the masked chip; sharing
/// marks the tab and the next question carries the new output; stopping removes the marker;
/// Insert into Terminal types the answer's command without running it.
fn ai_context_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    static FAKE: std::sync::OnceLock<Option<idecraft_ai::fake::FakeEndpoint>> =
        std::sync::OnceLock::new();
    fn fake() -> Option<&'static idecraft_ai::fake::FakeEndpoint> {
        FAKE.get_or_init(|| {
            idecraft_ai::fake::start("Run this:\n```bash\necho fixed\n```", 200).ok()
        })
        .as_ref()
    }
    fn bodies() -> Vec<String> {
        fake()
            .and_then(|f| {
                f.seen.lock().ok().map(|s| {
                    s.iter()
                        .filter(|x| x.line.contains("/chat/completions"))
                        .map(|x| x.body.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }
    fn chat(b: &WorkbenchView, cx: &App) -> Option<Entity<idecraft_workbench::AiChatView>> {
        let _ = cx;
        b.ai_chat_view()
    }
    fn command_done(b: &WorkbenchView, cx: &App, command: &str) -> bool {
        b.terminal().is_some_and(|t| {
            t.read(cx)
                .prompts()
                .iter()
                .any(|p| p.command.as_deref() == Some(command) && p.exit == Some(0))
        })
    }
    // An obviously fake token of a published shape (no credential).
    const TOKEN_LINE: &str = "echo GITHUB_TOKEN=ghp_FAKEFAKEFAKEFAKEFAKEFAKE0000\r";
    vec![
        Step::Act(Box::new(|b, window, cx| {
            let Some(f) = fake() else { return };
            b.open_settings(Page::Ai, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_ai(cx);
                    v.set_ai_fields(&f.endpoint, "test-model", cx);
                    v.apply_ai(cx);
                });
            }
        })),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            b.new_terminal_tab(window, cx);
        })),
        Step::WaitFor(
            "a new shell marks its prompt",
            Box::new(|b, cx| {
                b.terminal()
                    .is_some_and(|t| !t.read(cx).prompts().is_empty())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text(TOKEN_LINE));
            }
        })),
        Step::WaitFor(
            "the shell marks the command with its exit code",
            Box::new(|b, cx| {
                command_done(b, cx, "echo GITHUB_TOKEN=ghp_FAKEFAKEFAKEFAKEFAKEFAKE0000")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| b.open_terminal_menu(cx))),
        Step::Expect(
            "the terminal's context menu offers Explain with AI, Attach to Chat and sharing",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(place, items)| {
                    place == idecraft_workbench::MenuPlace::Terminal
                        && items.contains(&("Explain with AI".to_owned(), true))
                        && items.contains(&("Attach to Chat › Last Command".to_owned(), true))
                        && items.contains(&("Attach to Chat › Selection".to_owned(), false))
                        && items.contains(&("Share Tab with This Chat".to_owned(), true))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Attach to Chat › Last Command", window, cx);
        })),
        Step::Expect(
            "the last command becomes a chip with its exit code and the token masked",
            Box::new(|b, cx| {
                chat(b, cx).is_some_and(|v| {
                    let chips = v.read(cx).chips();
                    chips.len() == 1
                        && chips[0].starts_with("Terminal \"")
                        && chips[0].contains("· last command · echo GITHUB_TOKEN=‹masked› · exit 0")
                        && chips[0].ends_with("· 2 masked")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_terminal_menu(cx);
            b.choose_context_item("Explain with AI", window, cx);
        })),
        Step::Expect(
            "Explain with AI adds the chip and prefills the question without sending it",
            Box::new(|b, cx| {
                chat(b, cx).is_some_and(|v| {
                    let v = v.read(cx);
                    v.chips().len() == 2
                        && v.question(cx) == idecraft_ai::context::EXPLAIN_OUTPUT
                        && !v.busy()
                }) && bodies().is_empty()
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter sends the chips with the token masked and shows the answer",
            Box::new(|b, cx| {
                chat(b, cx).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.busy() && v.chips().is_empty() && v.commands() == ["echo fixed"]
                }) && bodies().first().is_some_and(|body| {
                    body.contains("‹masked›")
                        && !body.contains("FAKEFAKE")
                        && body.contains("[exit code 0]")
                        && body.contains("Explain this output")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_terminal_menu(cx);
            b.choose_context_item("Share Tab with This Chat", window, cx);
        })),
        Step::Expect(
            "sharing marks the tab and the chat names it",
            Box::new(|b, cx| {
                !b.shared_terminal_tabs(cx).is_empty()
                    && chat(b, cx).is_some_and(|v| !v.read(cx).shared_names(cx).is_empty())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("echo second-run\r"));
            }
        })),
        Step::WaitFor(
            "the shared tab's next command ran",
            Box::new(|b, cx| command_done(b, cx, "echo second-run")),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.ai_chat_view() {
                v.update(cx, |v, cx| {
                    v.set_question("What now?", cx);
                    v.ask(None, cx);
                });
            }
        })),
        Step::WaitFor(
            "the next question carries the shared tab's new output",
            Box::new(|b, cx| {
                !chat(b, cx).is_some_and(|v| v.read(cx).busy())
                    && bodies().get(1).is_some_and(|body| {
                        body.contains("echo second-run")
                            && body.contains("(shared)")
                            && body.contains("What now?")
                    })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_terminal_menu(cx);
            b.choose_context_item("Stop Sharing with Chat", window, cx);
        })),
        Step::Expect(
            "Stop Sharing removes the marker",
            Box::new(|b, cx| b.shared_terminal_tabs(cx).is_empty()),
        ),
        Step::Act(Box::new(|b, window, cx| {
            let before = b.terminal().map_or(0, |t| t.read(cx).prompts().len());
            let command = b
                .ai_chat_view()
                .and_then(|v| v.read(cx).commands().last().cloned());
            if let Some(command) = command {
                b.ai_insert_command(&command, window, cx);
            }
            INSERT_PROMPTS.store(before, std::sync::atomic::Ordering::Relaxed);
        })),
        Step::WaitFor(
            "Insert into Terminal types the command without running it",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    let t = t.read(cx);
                    t.text()
                        .lines()
                        .last()
                        .is_some_and(|l| l.ends_with("echo fixed"))
                        && t.prompts().len()
                            == INSERT_PROMPTS.load(std::sync::atomic::Ordering::Relaxed)
                }) && b
                    .status_note()
                    .is_some_and(|s| s.contains("press Enter to run it"))
            }),
        ),
        // Behaviour 2 without marks: a shell without the integration (`/bin/sh`) offers the last
        // lines instead of the last command. The typed command is removed and its tab closed.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("\u{15}exit\r"));
            }
            let mut settings = idecraft_workbench::appearance::ide_settings(cx);
            settings.terminal.shell = Some("/bin/sh".to_owned());
            let _ = idecraft_workbench::appearance::set_ide_settings(settings.clone(), cx);
            b.new_terminal_tab(window, cx);
            settings.terminal.shell = None;
            let _ = idecraft_workbench::appearance::set_ide_settings(settings, cx);
        })),
        Step::WaitFor(
            "a shell without marks shows its prompt",
            Box::new(|b, cx| {
                b.terminal()
                    .is_some_and(|t| !t.read(cx).text().trim().is_empty())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("echo unmarked-$((40+2))\r"));
            }
        })),
        // The bottom panel may show a single row (the shell's next prompt): the output is looked
        // for in the whole history, as Last n Lines takes it.
        Step::WaitFor(
            "the unmarked command ran",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    let t = t.read(cx);
                    t.prompts().is_empty()
                        && t.history_text().lines().any(|l| l.trim() == "unmarked-42")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| b.open_terminal_menu(cx))),
        Step::Expect(
            "without marks Attach to Chat offers the last lines, not the last command",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(_, items)| {
                    !items
                        .iter()
                        .any(|(i, _)| i == "Attach to Chat › Last Command")
                        && items.contains(&("Attach to Chat › Last 200 Lines".to_owned(), true))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Attach to Chat › Last 200 Lines", window, cx);
        })),
        Step::Expect(
            "the last lines become a chip with the command's output",
            Box::new(|b, cx| {
                chat(b, cx).is_some_and(|v| {
                    v.read(cx).chips().last().is_some_and(|c| {
                        c.starts_with("Terminal \"")
                            && c.contains(" · last ")
                            && c.contains("lines")
                    })
                })
            }),
        ),
        // Clean up: the shell without marks ends, the assistant is switched off.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("\u{15}exit\r"));
            }
            b.open_settings(Page::Ai, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_ai(cx);
                    v.set_ai_fields("", "", cx);
                    v.apply_ai(cx);
                });
            }
        })),
        Step::Expect(
            "switched off again",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| !v.read(cx).ide().ai.enabled)
            }),
        ),
        Step::Key("escape", None),
    ]
}

/// Prompts of the terminal before Insert into Terminal (`ai_context_steps`).
static INSERT_PROMPTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Settings › Editor › Natural Languages (SPEC-0159): the five English dictionaries are listed;
/// nothing is downloaded here (no network in the self-test).
fn natural_language_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::NaturalLanguages, window, cx)
        })),
        Step::Expect(
            "the Natural Languages page lists the dictionaries to download",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page() == Page::NaturalLanguages && v.downloading().is_none()
                })
            }),
        ),
        Step::Key("escape", None),
        // SPEC-0163: a word saved to the project dictionary lands where the reference keeps it.
        Step::Act(Box::new(|b, _, cx| b.learn_word("Frobnicate", cx))),
        Step::Expect(
            "Save to project dictionary writes .idea/dictionaries/project.xml",
            Box::new(|b, _| {
                let dir = FIXTURE.get().expect("fixture");
                let file = std::fs::canonicalize(dir)
                    .unwrap_or(dir.clone())
                    .join(".idea/dictionaries/project.xml");
                b.status_note()
                    .is_some_and(|s| s.contains("saved to the project dictionary"))
                    && std::fs::read_to_string(file).is_ok_and(|t| t.contains("<w>frobnicate</w>"))
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                let idea = d.join(".idea");
                let _ = std::fs::remove_dir_all(idea.join("dictionaries"));
                let _ = std::fs::remove_dir(&idea);
            }
        })),
    ]
}

/// View › Appearance › Compact Mode (SPEC-0165): lower tree rows and toolbar, then back.
fn compact_mode_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ToggleCompactMode", window, cx)
        })),
        Step::Expect(
            "Compact mode lowers tree rows and the toolbar and is saved",
            Box::new(|_, cx| {
                let p = idecraft_workbench::palette::Palette::current();
                idecraft_workbench::appearance::ide_settings(cx)
                    .appearance
                    .compact_mode
                    && p.metrics.compact
                    && p.metrics.tree_row_height < 24.0
                    && p.metrics.toolbar_height == 32.0
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ToggleCompactMode", window, cx)
        })),
        Step::Expect(
            "switching it off restores the normal metrics",
            Box::new(|_, cx| {
                let p = idecraft_workbench::palette::Palette::current();
                !idecraft_workbench::appearance::ide_settings(cx)
                    .appearance
                    .compact_mode
                    && !p.metrics.compact
                    && p.metrics.toolbar_height == 40.0
            }),
        ),
    ]
}

fn ide_settings_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    let save = if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    };
    let close = if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-f4"
    };
    let ide = |b: &WorkbenchView, cx: &App| {
        b.settings_dialog()
            .map(|v| v.read(cx).ide().clone())
            .unwrap_or_default()
    };
    let saved = || {
        FIXTURE
            .get()
            .and_then(|d| std::fs::read_to_string(d.join(".idecraft/ide-settings.toml")).ok())
            .unwrap_or_default()
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::Font, window, cx)
        })),
        // SPEC-0150: the first row chooses the font family.
        Step::Expect(
            "the Font page offers the bundled JetBrains Mono first",
            Box::new(move |b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    v.read(cx).font_choices().first().map(String::as_str) == Some("JetBrains Mono")
                }) && ide(b, cx).editor.font_family == "JetBrains Mono"
            }),
        ),
        Step::Key("right", None),
        Step::Expect(
            "Right on Font chooses the next family and the editor uses it",
            Box::new(move |b, cx| {
                let choices = b
                    .settings_dialog()
                    .map(|v| v.read(cx).font_choices().to_vec())
                    .unwrap_or_default();
                let want = choices
                    .get(1)
                    .or(choices.first())
                    .cloned()
                    .unwrap_or_default();
                ide(b, cx).editor.font_family == want
                    && idecraft_workbench::fonts::editor_family() == want
            }),
        ),
        Step::Key("left", None),
        Step::Key("down", None),
        Step::Key("right", None),
        Step::Expect(
            "Right makes the editor font larger at once and saves it",
            Box::new(move |b, cx| {
                ide(b, cx).editor.font_size == 14.0
                    && idecraft_workbench::fonts::editor_font_size() == 14.0
                    && saved().contains("font_size = 14")
            }),
        ),
        Step::Key("left", None),
        Step::Expect(
            "Left takes it back",
            Box::new(move |b, cx| {
                ide(b, cx).editor.font_size == 13.0
                    && idecraft_workbench::fonts::editor_line_height() == 16.0
            }),
        ),
        // SPEC-0150 v2 AK-4: the ligatures checkbox under the font rows, off by default.
        Step::Key("down", None),
        Step::Key("down", None),
        Step::Expect(
            "ligatures are off by default",
            Box::new(move |b, cx| {
                !ide(b, cx).editor.ligatures && !idecraft_workbench::fonts::editor_ligatures()
            }),
        ),
        Step::Key("space", None),
        Step::Expect(
            "Space on Enable ligatures switches them on in the editor and saves it",
            Box::new(move |b, cx| {
                ide(b, cx).editor.ligatures
                    && idecraft_workbench::fonts::editor_ligatures()
                    && saved().contains("ligatures = true")
            }),
        ),
        Step::Key("space", None),
        Step::Expect(
            "Space again switches them off",
            Box::new(move |b, cx| {
                !ide(b, cx).editor.ligatures && !idecraft_workbench::fonts::editor_ligatures()
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.show(Page::Editor, cx));
            }
        })),
        Step::Key("enter", None),
        Step::Key("down", None),
        Step::Key("space", None),
        Step::Expect(
            "the Editor page switches both save options on",
            Box::new(move |b, cx| {
                let e = ide(b, cx).editor;
                e.strip_trailing_spaces && e.ensure_line_break_at_end
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.show(Page::Terminal, cx);
                    v.set_shell_text("/bin/sh", cx);
                    v.focus_shell(window, cx);
                });
            }
        })),
        // Enter in the field applies it instead of adding a line (D19, hand test 56).
        Step::Key("enter", None),
        Step::Expect(
            "Enter in the shell field keeps the shell",
            Box::new(move |b, cx| {
                ide(b, cx).terminal.shell.as_deref() == Some("/bin/sh")
                    && saved().contains("shell = \"/bin/sh\"")
                    && b.settings_dialog()
                        .is_some_and(|v| v.read(cx).shell_text(cx) == "/bin/sh")
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape leaves the field, Settings stay open",
            Box::new(|b, _| b.settings_dialog().is_some()),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.show(Page::BuildTools, cx));
            }
        })),
        Step::Expect(
            "Build Tools lists the JDKs and how Maven and Gradle run",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let info = v.read(cx).info().to_vec();
                    info.first().is_some_and(|(t, _)| t == "JDKs")
                        && info.iter().any(|(t, _)| t.trim_start().starts_with("mvn:"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.show(Page::Languages, cx));
            }
        })),
        Step::Expect(
            "Languages & Frameworks lists a server or install hint per language",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    v.read(cx)
                        .info()
                        .iter()
                        .any(|(t, _)| t.starts_with("rust: ") || t.starts_with("python: "))
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("tidy.txt"), "a  \nb\t\nc  ");
                b.open(&dir.join("tidy.txt"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.select_range(1..1, cx));
                }
            }
        })),
        Step::Key(save, None),
        Step::Expect(
            "saving strips trailing spaces except on the caret line and ends with a line break",
            Box::new(|_, _| {
                FIXTURE.get().is_some_and(|d| {
                    std::fs::read_to_string(d.join("tidy.txt")).is_ok_and(|t| t == "a  \nb\nc\n")
                })
            }),
        ),
        Step::Key(close, None),
        // SPEC-0150: Code Style sets the indentation open editors use.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("style.py"), "x = 1\n");
                b.open(&dir.join("style.py"), window, cx);
            }
            b.open_settings(Page::CodeStyle, window, cx);
            let python = idecraft_core::CODE_STYLE_LANGUAGES
                .iter()
                .position(|l| *l == "Python")
                .unwrap_or(0);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.select_row(2 + python, cx));
            }
        })),
        Step::Key("left", None),
        Step::Expect(
            "Left on Python makes its indent 3 and the open Python editor follows",
            Box::new(move |b, cx| {
                ide(b, cx).code_style.indent_for("Python").indent == 3
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).indent_unit() == "   ")
            }),
        ),
        Step::Key("space", None),
        Step::Expect(
            "Space switches Python to tabs",
            Box::new(move |b, cx| {
                ide(b, cx).code_style.indent_for("Python").use_tabs
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).indent_unit() == "\t")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.select_row(0, cx));
            }
        })),
        Step::Key("right", None),
        Step::Expect(
            "Right on Hard wrap moves the right margin",
            Box::new(move |b, cx| ide(b, cx).code_style.right_margin == 121),
        ),
        // SPEC-0150: Version Control.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.show(Page::VersionControl, cx);
                    v.select_row(1, cx);
                });
            }
        })),
        Step::Key("space", None),
        Step::Expect(
            "Space switches force push off and it is saved",
            Box::new(move |b, cx| {
                !ide(b, cx).vcs.allow_force_push && saved().contains("allow_force_push = false")
            }),
        ),
        Step::Key("escape", None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, cx| {
            let _ = idecraft_workbench::appearance::set_ide_settings(Default::default(), cx);
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("tidy.txt"));
                let _ = std::fs::remove_file(dir.join("style.py"));
            }
        })),
    ]
}

/// Speed search and Show DDL in the Database tool window (SPEC-0089 v5 AK-12): typing "ord" in the
/// tree opens the search field and selects `orders`; Escape closes the search, not the window; Show
/// DDL opens a read-only tab `orders (DDL).sql` with its `CREATE TABLE`, which is closed again.
fn db_search_ddl_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.select_label("shop", cx);
                });
                window.focus(&v.read(cx).focus());
            }
        })),
        Step::Key("o", Some("o")),
        Step::Key("r", Some("r")),
        Step::Key("d", Some("d")),
        Step::Expect(
            "typing ord in the database tree selects orders",
            Box::new(|b, cx| {
                b.database_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.search_text() == Some("ord")
                        && v.selected_row_label().as_deref() == Some("orders")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the speed search and keeps the window",
            Box::new(|b, cx| {
                b.database_view()
                    .is_some_and(|v| v.read(cx).search_text().is_none())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| v.show_ddl(cx));
            }
        })),
        Step::WaitFor(
            "show DDL opens the table's definition in a read-only tab",
            Box::new(|b, cx| {
                b.active_path()
                    .is_some_and(|p| p.file_name().is_some_and(|n| n == "orders (DDL).sql"))
                    // SQLite shows the stored statement in the spelling it was created with
                    // (SPEC-0089 v7 behaviour 17).
                    && b.active_text(cx).is_some_and(|t| {
                        let t = t.to_lowercase().replace('"', "");
                        t.contains("create table orders")
                    })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("CloseContent", window, cx)
        })),
        Step::Expect(
            "the DDL tab closes",
            Box::new(|b, _| {
                !b.main_tab_titles()
                    .iter()
                    .any(|t| t.starts_with("orders (DDL)"))
            }),
        ),
        // SPEC-0089 v6 AK-15: F1 shows the quick documentation from what the tree read, at once.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.select_label("orders", cx);
                });
                window.focus(&v.read(cx).focus());
            }
        })),
        Step::Key("f1", None),
        Step::Expect(
            "F1 on orders shows its columns and its foreign key without waiting",
            Box::new(|b, cx| {
                b.database_view().is_some_and(|v| {
                    v.read(cx).doc_lines().is_some_and(|l| {
                        l.first().is_some_and(|t| t == "table orders")
                            && l.iter().any(|x| x.starts_with("Foreign key"))
                            && l.iter().any(|x| x.contains("customer_id"))
                    })
                })
            }),
        ),
        Step::Key("f1", None),
        Step::Expect(
            "F1 again hides the documentation",
            Box::new(|b, cx| {
                b.database_view()
                    .is_some_and(|v| v.read(cx).doc_lines().is_none())
            }),
        ),
    ]
}

/// Renaming a column from the Database tool window (SPEC-0128): ⇧F6 on `orders.note` shows the
/// statement and the place in a SQL file; Enter renames it in the database and in the file; it is
/// renamed back the same way.
fn db_rename_steps() -> Vec<Step> {
    let rename = |from: &'static str, to: &'static str| -> Vec<Step> {
        vec![
            Step::Act(Box::new(move |b, window, cx| {
                if let Some(v) = b.database_view() {
                    v.update(cx, |v, cx| {
                        v.select_label(from, cx);
                    });
                    window.focus(&v.read(cx).focus());
                }
            })),
            Step::Key("shift-f6", None),
            Step::Expect(
                "shift+F6 on a column opens the rename dialog",
                Box::new(|b, _| b.db_rename_dialog().is_some()),
            ),
            Step::Act(Box::new(move |b, _, cx| {
                if let Some(d) = b.db_rename_dialog() {
                    d.update(cx, |d, cx| d.set_new_name(to, cx));
                }
            })),
            Step::Expect(
                "the dialog previews the statement and the place in the SQL file",
                Box::new(move |b, cx| {
                    b.db_rename_dialog().is_some_and(|d| {
                        d.read(cx).preview().is_ok_and(|p| {
                            p.ddl == format!("ALTER TABLE orders RENAME COLUMN {from} TO {to}")
                                && p.edit_count() == 1
                                && p.files
                                    .iter()
                                    .any(|f| f.file.label == "notes.sql" && f.included)
                        })
                    })
                }),
            ),
            // SPEC-0128 v2 AK-5: a file under `mysql/` is listed, unchecked; a click includes it.
            Step::Expect(
                "a file of another dialect is listed but not included",
                Box::new(move |b, cx| {
                    from != "note"
                        || b.db_rename_dialog().is_some_and(|d| {
                            d.read(cx).preview().is_ok_and(|p| {
                                p.files.iter().any(|f| {
                                    f.file.label.ends_with("notes.sql")
                                        && f.file.label.starts_with("mysql")
                                        && f.other
                                        && !f.included
                                })
                            })
                        })
                }),
            ),
            Step::Act(Box::new(move |b, _, cx| {
                if let Some(d) = b.db_rename_dialog().filter(|_| from == "note") {
                    let i = d
                        .read(cx)
                        .preview()
                        .ok()
                        .and_then(|p| p.files.iter().position(|f| f.other));
                    if let Some(i) = i {
                        d.update(cx, |d, cx| d.toggle_file(i, cx));
                    }
                }
            })),
            Step::Expect(
                "a click includes it",
                Box::new(move |b, cx| {
                    from != "note"
                        || b.db_rename_dialog().is_some_and(|d| {
                            d.read(cx).preview().is_ok_and(|p| p.edit_count() == 2)
                        })
                }),
            ),
            Step::Act(Box::new(move |b, _, cx| {
                if let Some(d) = b.db_rename_dialog().filter(|_| from == "note") {
                    let i = d
                        .read(cx)
                        .preview()
                        .ok()
                        .and_then(|p| p.files.iter().position(|f| f.other));
                    if let Some(i) = i {
                        d.update(cx, |d, cx| d.toggle_file(i, cx));
                    }
                }
            })),
            Step::Key("enter", None),
            Step::WaitFor(
                "enter renames the column in the database, the file and the tree",
                Box::new(move |b, cx| {
                    b.db_rename_dialog().is_none()
                        && FIXTURE.get().is_some_and(|d| {
                            std::fs::read_to_string(d.join("notes.sql"))
                                .is_ok_and(|t| t == format!("select {to} from orders;\n"))
                                && std::fs::read_to_string(d.join("mysql/notes.sql"))
                                    .is_ok_and(|t| t == "select note from orders;\n")
                        })
                        && b.database_view()
                            .is_some_and(|v| v.read(cx).visible().iter().any(|(_, n, _)| n == to))
                }),
            ),
            Step::Act(Box::new(|b, _, cx| {
                if let Some(v) = b.database_view() {
                    v.update(cx, |v, cx| v.expand_all_tables(cx));
                }
            })),
        ]
    };
    let mut steps = vec![Step::Act(Box::new(|_, _, _| {
        if let Some(d) = FIXTURE.get() {
            let _ = std::fs::write(d.join("notes.sql"), "select note from orders;\n");
            let _ = std::fs::create_dir_all(d.join("mysql"));
            let _ = std::fs::write(d.join("mysql/notes.sql"), "select note from orders;\n");
        }
    }))];
    steps.extend(rename("note", "remark"));
    steps.extend(rename("remark", "note"));
    steps.extend(db_refactoring_steps());
    steps.push(Step::Act(Box::new(|_, _, _| {
        if let Some(d) = FIXTURE.get() {
            let _ = std::fs::remove_file(d.join("notes.sql"));
            let _ = std::fs::remove_dir_all(d.join("mysql"));
        }
    })));
    steps.extend(sql_editor_refactoring_steps());
    steps
}

/// Database refactorings from the tree, by keys only (SPEC-0164 v3 AK-7): speed search selects the
/// column `note`, ⌘F6 opens Change column type, whose preview names SQLite's limitation for the
/// typed type; speed search selects the index `orders_by_customer`, ⇧F6 and a new name show
/// SQLite's refusal.
fn db_refactoring_steps() -> Vec<Step> {
    let change_type = if cfg!(target_os = "macos") {
        "cmd-f6"
    } else {
        "ctrl-f6"
    };
    let dialog = |title: &'static str, reason: &'static str| -> Check {
        Box::new(move |b, cx| {
            b.db_rename_dialog().is_some_and(|d| {
                let d = d.read(cx);
                d.title() == title && d.preview().is_err_and(|e| e.contains(reason))
            })
        })
    };
    vec![
        Step::Key("n", Some("n")),
        Step::Key("o", Some("o")),
        Step::Key("t", Some("t")),
        Step::Key("e", Some("e")),
        Step::Key("escape", None),
        Step::Expect(
            "speed search selects the column note",
            Box::new(|b, cx| {
                b.database_view()
                    .is_some_and(|v| v.read(cx).selected_row_label().as_deref() == Some("note"))
            }),
        ),
        Step::Key(change_type, None),
        Step::Key("i", Some("i")),
        Step::Key("n", Some("n")),
        Step::Key("t", Some("t")),
        Step::Expect(
            "cmd+F6 on a column row opens the type change; the preview names SQLite's limitation",
            dialog("Change type of column orders.note", "SQLite cannot change"),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes it",
            Box::new(|b, _| b.db_rename_dialog().is_none()),
        ),
        Step::Key("b", Some("b")),
        Step::Key("y", Some("y")),
        Step::Key("_", Some("_")),
        Step::Key("c", Some("c")),
        Step::Key("escape", None),
        Step::Key("shift-f6", None),
        Step::Key("x", Some("x")),
        Step::Expect(
            "shift+F6 on an index row opens its rename with SQLite's refusal",
            dialog(
                "Rename index orders_by_customer of orders",
                "SQLite cannot rename an index",
            ),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes it",
            Box::new(|b, _| b.db_rename_dialog().is_none()),
        ),
    ]
}

/// Refactorings in a SQL editor (SPEC-0152): ⇧F6 on a column name opens the database rename dialog
/// for that column; ⌥↩ on `*` offers "Expand column list" and Enter writes the columns.
fn sql_editor_refactoring_steps() -> Vec<Step> {
    let (save, close) = if cfg!(target_os = "macos") {
        ("cmd-s", "cmd-w")
    } else {
        ("ctrl-s", "ctrl-f4")
    };
    let caret = |line: usize, column: usize| -> Step {
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(line, column, cx));
                window.focus(&e.read(cx).focus());
            }
        }))
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::write(
                    d.join("refactor.sql"),
                    "select o.note from orders o;\nselect * from orders;\n",
                );
                b.open(&d.join("refactor.sql"), window, cx);
            }
        })),
        caret(0, 10),
        Step::Key("shift-f6", None),
        Step::WaitFor(
            "shift+F6 on a column in a SQL editor opens the rename dialog for it",
            Box::new(|b, _| b.db_rename_dialog().is_some()),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(d) = b.db_rename_dialog() {
                d.update(cx, |d, cx| d.set_new_name("remark", cx));
            }
        })),
        Step::Expect(
            "the dialog previews the column's statement and the place in the editor's file",
            Box::new(|b, cx| {
                b.db_rename_dialog().is_some_and(|d| {
                    d.read(cx).preview().is_ok_and(|p| {
                        p.ddl == "ALTER TABLE orders RENAME COLUMN note TO remark"
                            && p.files
                                .iter()
                                .any(|f| f.file.label == "refactor.sql" && f.edits.len() == 1)
                    })
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes it without renaming",
            Box::new(|b, _| b.db_rename_dialog().is_none()),
        ),
        // SPEC-0164: ⌘F6 on a column changes its type; this SQLite connection cannot.
        caret(0, 10),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-f6"
            } else {
                "ctrl-f6"
            },
            None,
        ),
        Step::WaitFor(
            "⌘F6 on a column opens Change column type, which names what SQLite cannot do",
            Box::new(|b, cx| {
                b.db_rename_dialog().is_some_and(|d| {
                    let d = d.read(cx);
                    d.title().starts_with("Change type of column orders.note")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(d) = b.db_rename_dialog() {
                d.update(cx, |d, cx| d.set_new_name("bigint", cx));
            }
        })),
        Step::Expect(
            "the preview explains that SQLite cannot change a column's type",
            Box::new(|b, cx| {
                b.db_rename_dialog().is_some_and(|d| {
                    d.read(cx)
                        .preview()
                        .is_err_and(|e| e.contains("SQLite cannot change a column's type"))
                })
            }),
        ),
        Step::Key("escape", None),
        caret(1, 8),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ on * offers Expand column list",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str) == Some("Expand column list")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter writes the table's columns",
            Box::new(|b, cx| {
                b.intentions_popup().is_none()
                    && b.active_text(cx)
                        .is_some_and(|t| t.ends_with("select id, customer_id, note from orders;\n"))
            }),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::remove_file(d.join("refactor.sql"));
            }
        })),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.database_view() {
                window.focus(&v.read(cx).focus());
            }
        })),
    ]
}

/// The Breakpoints dialog (SPEC-0127): ⇧⌘F8 on a breakpoint line opens its popup and ⇧⌘F8 there
/// the dialog with that breakpoint (v10 behaviour 19); a condition makes a Node.js loop stop only
/// on that pass (with js-debug); switching it off shows a ring that moves with an edit above.
fn breakpoint_steps() -> Vec<Step> {
    // Debug Current File: ⌃⇧D on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(breakpoint_steps_keys())
}

fn breakpoint_steps_keys() -> Vec<Step> {
    use idecraft_workbench::debug_model::BreakpointLook;
    let view = if cfg!(target_os = "macos") {
        "cmd-shift-f8"
    } else {
        "ctrl-shift-f8"
    };
    let (debug, resume) = if cfg!(target_os = "macos") {
        ("ctrl-shift-d", "cmd-alt-r")
    } else {
        (DEBUG_CURRENT_FILE, "f9")
    };
    let (save, close) = if cfg!(target_os = "macos") {
        ("cmd-s", "cmd-w")
    } else {
        ("ctrl-s", "ctrl-f4")
    };
    let js_debug = idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "js-debug" && a.command.is_some());
    let look = |b: &WorkbenchView, cx: &App, line: usize| {
        b.active_editor_entity()
            .map(|e| e.read(cx).breakpoint_look(line))
    };
    let mut steps = vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("passes.js"),
                    "let total = 0;\nfor (let i = 0; i < 5; i++) {\n  total += i;\n}\nconsole.log('total ' + total);\n",
                );
                b.open(&dir.join("passes.js"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| {
                        e.toggle_breakpoint(0, cx);
                        e.toggle_breakpoint(2, cx);
                        let at = e
                            .state
                            .buffer
                            .position_to_offset(idecraft_editor::Position::new(2, 2))
                            .unwrap_or(0);
                        e.select_range(at..at, cx);
                    });
                }
            }
        })),
        // On the caret's breakpoint the key opens its small popup first (SPEC-0127 v10 behaviour
        // 19); the key again in the popup opens the dialog with that breakpoint selected.
        Step::Key(view, None),
        Step::Expect(
            "the key on the caret's breakpoint opens its popup, not the dialog",
            Box::new(|b, cx| {
                b.breakpoint_dialog().is_none()
                    && b.breakpoint_popup().is_some_and(|p| {
                        p.read(cx)
                            .place()
                            .is_some_and(|(path, line)| line == 2 && path.ends_with("passes.js"))
                    })
            }),
        ),
        Step::Key(view, None),
        Step::Expect(
            "the key in the popup opens the dialog: both listed, the caret's one selected",
            Box::new(|b, cx| {
                b.breakpoint_popup().is_none()
                    && b.breakpoint_dialog().is_some_and(|d| {
                        let d = d.read(cx);
                        let ours = d
                            .list()
                            .iter()
                            .filter(|e| e.label.starts_with("passes.js:"))
                            .count();
                        ours == 2 && d.selected().is_some_and(|e| e.label == "passes.js:3")
                    })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(d) = b.breakpoint_dialog() {
                d.update(cx, |d, cx| d.set_field("Condition", "i === 3", cx));
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "Enter applies the condition, closes, and the dot shows it",
            Box::new(move |b, cx| {
                b.breakpoint_dialog().is_none()
                    && look(b, cx, 2) == Some(BreakpointLook::Conditional)
                    && look(b, cx, 0) == Some(BreakpointLook::Plain)
                    && FIXTURE.get().is_some_and(|d| {
                        std::fs::read_to_string(d.join(".idecraft/breakpoints.toml"))
                            .is_ok_and(|t| t.contains("i === 3"))
                    })
            }),
        ),
    ];
    steps.push(group("breakpoint-condition-debug"));
    if js_debug {
        steps.extend([
            // The unconditional breakpoint on the first line is switched off first.
            Step::Act(Box::new(|b, window, cx| {
                b.open_breakpoints(FIXTURE.get().map(|d| (d.join("passes.js"), 0)), window, cx);
                if let Some(d) = b.breakpoint_dialog() {
                    d.update(cx, |d, cx| {
                        d.set_enabled(false, cx);
                        d.done(cx);
                    });
                }
            })),
            Step::Key(debug, None),
            Step::WaitFor(
                "the conditional breakpoint stops the loop only on the pass where it holds",
                Box::new(|b, cx| {
                    b.debug_view().is_some_and(|v| {
                        let v = v.read(cx);
                        // 0 + 1 + 2 before the pass with i === 3.
                        matches!(v.state(), idecraft_workbench::DebugState::Paused(_))
                            && v.variable_texts().iter().any(|t| t == "total = 3")
                    })
                }),
            ),
            Step::Key(resume, None),
            Step::WaitFor(
                "resume runs the loop to its end without stopping again",
                Box::new(|b, cx| {
                    b.debug_view().is_some_and(|v| {
                        let v = v.read(cx);
                        matches!(v.state(), idecraft_workbench::DebugState::Ended(_))
                            && v.console().contains("total 10")
                    })
                }),
            ),
            Step::Expect(
                "the switched-off breakpoint shows as a ring",
                Box::new(move |b, cx| look(b, cx, 0) == Some(BreakpointLook::Disabled)),
            ),
        ]);
    }
    steps.push(group("breakpoint-moves"));
    steps.extend([
        // An edit above moves the breakpoints and their properties.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                window.focus(&e.read(cx).focus());
                e.update(cx, |e, cx| e.select_range(0..0, cx));
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "the condition moves with its breakpoint",
            Box::new(move |b, cx| {
                look(b, cx, 3) == Some(BreakpointLook::Conditional)
                    // The breakpoint at the edit's own line stays (SPEC-0091 behaviour 1).
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).breakpoints() == [0, 3])
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_breakpoints(None, window, cx);
            if let Some(d) = b.breakpoint_dialog() {
                for _ in 0..2 {
                    d.update(cx, |d, cx| {
                        let ours = d
                            .list()
                            .iter()
                            .position(|e| e.label.starts_with("passes.js:"));
                        if let Some(i) = ours {
                            d.select(i, cx);
                            d.remove_selected(cx);
                        }
                    });
                }
            }
        })),
        Step::Expect(
            "Remove in the dialog takes the breakpoints away",
            Box::new(|b, cx| {
                b.breakpoint_dialog().is_some_and(|d| {
                    !d.read(cx)
                        .list()
                        .iter()
                        .any(|e| e.label.starts_with("passes.js:"))
                }) && b
                    .active_editor_entity()
                    .is_some_and(|e| e.read(cx).breakpoints().is_empty())
            }),
        ),
        // SPEC-0146: an exception row and a method breakpoint, saved for the workspace. Since v6
        // (behaviour 15) each language has its own row; the first after the list is the first
        // language's.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = b.breakpoint_dialog() {
                d.update(cx, |d, cx| {
                    let n = d.list().len();
                    d.select(n, cx);
                });
                window.focus(&d.read(cx).focus(cx));
            }
        })),
        Step::Key("alt-u", None),
        Step::Key("cmd-n", None),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(d) = b.breakpoint_dialog() {
                d.update(cx, |d, cx| d.set_special_field("Method", "add", cx));
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "the Breakpoints dialog saves uncaught exceptions for the row's language only and a method breakpoint",
            Box::new(|b, _| {
                let [(first, _), (second, _), ..] = idecraft_debugger::EXCEPTION_LANGUAGES;
                b.breakpoint_dialog().is_none()
                    && FIXTURE.get().is_some_and(|dir| {
                        let s = idecraft_debugger::saved::load_special_breakpoints(dir);
                        let (row, other) = (s.exceptions_of(first), s.exceptions_of(second));
                        row.uncaught
                            && !row.caught
                            && !other.uncaught
                            && !other.caught
                            && s.functions.len() == 1
                            && s.functions[0].name == "add"
                    })
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                idecraft_debugger::saved::save_special_breakpoints(dir, &Default::default());
            }
        })),
        Step::Key("escape", None),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("passes.js"));
            }
        })),
    ]);
    steps
}

/// Debugging a page (SPEC-0133): ⌃⇧D in an HTML file opens it in Chrome (headless here) under
/// js-debug, stops at a breakpoint in its script with the locals, and resume lets it log. Skipped
/// without js-debug or Chrome.
fn browser_debug_steps() -> Vec<Step> {
    // Debug Current File: ⌃⇧D on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(browser_debug_steps_keys())
}

fn browser_debug_steps_keys() -> Vec<Step> {
    use idecraft_workbench::DebugState;
    let ready = idecraft_debugger::chrome().is_some()
        && idecraft_debugger::known_adapters()
            .iter()
            .any(|a| a.name == "js-debug" && a.command.is_some());
    if !ready {
        eprintln!("browser debug steps skipped: js-debug or Chrome is missing");
        return Vec::new();
    }
    let (debug, resume, close) = if cfg!(target_os = "macos") {
        ("ctrl-shift-d", "cmd-alt-r", "cmd-w")
    } else {
        (DEBUG_CURRENT_FILE, "f9", "ctrl-f4")
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let web = dir.join("web");
                let _ = std::fs::create_dir_all(&web);
                let _ = std::fs::write(
                    web.join("page.js"),
                    "function twice(n) {\n  const doubled = n * 2;\n  return doubled;\n}\nsetTimeout(() => console.log('page result ' + twice(21)), 300);\n",
                );
                let _ = std::fs::write(
                    web.join("index.html"),
                    "<!doctype html><html><body><script src=\"page.js\"></script></body></html>\n",
                );
                b.open(&web.join("page.js"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(2, cx));
                }
                b.open(&web.join("index.html"), window, cx);
            }
        })),
        Step::Key(debug, None),
        Step::WaitFor(
            "debug in an HTML file stops in its script in Chrome with the locals",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Paused(_))
                        && v.frame_names().first().is_some_and(|f| f.contains("twice"))
                        && v.variable_texts().iter().any(|t| t == "doubled = 42")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume lets the page log its result",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).console().contains("page result 42"))
            }),
        ),
        // SPEC-0133 v3 AK-3: Rerun restarts the page session while it runs (hand test 60 found
        // "the debug adapter ended"), and Stop closes the browser.
        click("debug-rerun", Click::Left(1)),
        Step::WaitFor(
            "Rerun of a running page session stops in its script again",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Paused(_))
                        && v.frame_names().first().is_some_and(|f| f.contains("twice"))
                })
            }),
        ),
        click("debug-stop", Click::Left(1)),
        Step::WaitFor(
            "after Stop no browser uses the project's profile folder",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| matches!(v.read(cx).state(), DebugState::Ended(_)))
                    && FIXTURE
                        .get()
                        .and_then(|dir| idecraft_debugger::BrowserProfile::for_project(dir))
                        .is_some_and(|p| {
                            idecraft_debugger::lifetime::using_profile(&p.folder).is_empty()
                        })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.debug_command(idecraft_workbench::debug_model::Command::Stop, cx);
            if let Some(p) = FIXTURE
                .get()
                .and_then(|dir| idecraft_debugger::BrowserProfile::for_project(dir))
            {
                let _ = std::fs::remove_dir_all(p.folder);
            }
            if let Some(dir) = FIXTURE.get() {
                b.open(&dir.join("web/page.js"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(2, cx));
                }
            }
            b.focus_editor(window, cx);
        })),
        Step::Key(close, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join("web"));
            }
        })),
    ]
}

/// Debugging Kotlin (SPEC-0132): ⌃⇧D in the main file of a Gradle project builds it, stops at the
/// breakpoint through kotlin-debug-adapter with the locals, and resume runs to the end. Skipped
/// without the adapter or the Kotlin Gradle plugin in Gradle's cache.
fn kotlin_debug_steps() -> Vec<Step> {
    // Debug Current File: ⌃⇧D on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(kotlin_debug_steps_keys())
}

fn kotlin_debug_steps_keys() -> Vec<Step> {
    use idecraft_workbench::DebugState;
    let adapter = idecraft_debugger::kotlin_debug_adapter().is_some();
    let plugin = std::env::var_os("HOME").is_some_and(|h| {
        std::path::Path::new(&h)
            .join(".gradle/caches/modules-2/files-2.1/org.jetbrains.kotlin/kotlin-gradle-plugin/2.0.21")
            .is_dir()
    });
    if !adapter || !plugin {
        eprintln!(
            "kotlin debug steps skipped: kotlin-debug-adapter or the Kotlin Gradle plugin is missing"
        );
        return Vec::new();
    }
    let (debug, resume, close) = if cfg!(target_os = "macos") {
        ("ctrl-shift-d", "cmd-alt-r", "cmd-w")
    } else {
        (DEBUG_CURRENT_FILE, "f9", "ctrl-f4")
    };
    const BUILD: &str = "buildscript {\n    repositories { mavenCentral() }\n    dependencies { classpath(\"org.jetbrains.kotlin:kotlin-gradle-plugin:2.0.21\") }\n}\n\nplugins { application }\napply(plugin = \"org.jetbrains.kotlin.jvm\")\n\nrepositories { mavenCentral() }\n\napplication { mainClass.set(\"demo.MainKt\") }\n";
    // The short pause works around a race in kotlin-debug-adapter: its event thread resumes the
    // new JVM before the breakpoints are requested, so code that runs at once may pass a
    // breakpoint (seen under load; PO-STATUS 2026-11-03). Breakpoints in loaded classes still apply.
    const MAIN: &str = "package demo\n\nfun twice(x: Int): Int {\n    val y = x * 2\n    return y\n}\n\nfun main() {\n    Thread.sleep(500)\n    println(\"kotlin result \" + twice(21))\n}\n";
    fn main_file() -> Option<std::path::PathBuf> {
        FIXTURE
            .get()
            .map(|d| d.join("ktdebug/src/main/kotlin/demo/Main.kt"))
    }
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let (Some(dir), Some(main)) = (FIXTURE.get(), main_file()) {
                let project = dir.join("ktdebug");
                let _ = std::fs::create_dir_all(main.parent().unwrap_or(&project));
                let _ = std::fs::write(
                    project.join("settings.gradle.kts"),
                    "rootProject.name = \"ktdebug\"\n",
                );
                let _ = std::fs::write(project.join("build.gradle.kts"), BUILD);
                let _ = std::fs::write(&main, MAIN);
                b.open(&main, window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(4, cx));
                }
            }
        })),
        Step::Key(debug, None),
        Step::WaitLong(
            "debug builds the Kotlin project and stops at the breakpoint with the locals",
            std::time::Duration::from_secs(180),
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Paused(_))
                        && v.frame_names().first().is_some_and(|f| f.contains("twice"))
                        && v.variable_texts().iter().any(|t| t == "y = 42")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume runs the Kotlin program to its end",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Ended(_))
                        && v.console().contains("kotlin result 42")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.toggle_breakpoint(4, cx));
            }
            b.focus_editor(window, cx);
        })),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join("ktdebug"));
            }
        })),
    ]
}

/// Debugging JavaScript (SPEC-0091 v4): ⌃⇧D on a script with a breakpoint stops there through
/// js-debug's child session with the locals, and resume runs to the end. Skipped without node or
/// js-debug.
fn node_debug_steps() -> Vec<Step> {
    // Debug Current File: ⌃⇧D on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(node_debug_steps_keys())
}

fn node_debug_steps_keys() -> Vec<Step> {
    let installed = idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "js-debug" && a.command.is_some());
    if !installed {
        eprintln!("node debug steps skipped: node or js-debug is missing");
        return Vec::new();
    }
    let (debug, resume) = if cfg!(target_os = "macos") {
        ("ctrl-shift-d", "cmd-alt-r")
    } else {
        (DEBUG_CURRENT_FILE, "f9")
    };
    let state = |b: &WorkbenchView, cx: &App| {
        b.debug_view().map(|v| {
            let v = v.read(cx);
            (
                v.state().clone(),
                v.frame_names(),
                v.variable_texts(),
                v.console().to_owned(),
            )
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("dbg.js"),
                    "function twice(x) {\n  const y = x * 2;\n  return y;\n}\nconst n = 21;\nconst r = twice(n);\nconsole.log('result ' + r);\n",
                );
                b.open(&dir.join("dbg.js"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(2, cx));
                }
            }
        })),
        Step::Key(debug, None),
        Step::WaitFor(
            "debug file stops a Node.js script at the breakpoint with its locals",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("twice"))
                        && vars.iter().any(|v| v == "y = 42")
                })
            }),
        ),
        // Watches, threads and values in the editor (SPEC-0091 v5).
        Step::Expect(
            "the paused function shows its values after the lines",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let values = e.read(cx).inline_values();
                    values.contains(&(1, "x: 21".to_owned()))
                        && values.contains(&(2, "y: 42".to_owned()))
                })
            }),
        ),
        // D2: the editor's menu offers the debugger's items while paused.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(1, 8, cx));
            }
            b.open_context_menu(
                idecraft_workbench::MenuPlace::Editor,
                b.active_path(),
                gpui::point(gpui::px(200.), gpui::px(200.)),
                cx,
            );
        })),
        Step::Expect(
            "the editor menu offers Evaluate Expression, Add to Watches and Run to Cursor while paused",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(_, items)| {
                    ["Evaluate Expression…", "Add to Watches", "Run to Cursor"]
                        .iter()
                        .all(|l| items.iter().any(|(i, on)| i == l && *on))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Add to Watches", window, cx);
        })),
        Step::WaitFor(
            "Add to Watches from the editor menu watches the name at the caret",
            Box::new(|b, cx| {
                b.debug_view()
                    .is_some_and(|v| v.read(cx).watch_texts().iter().any(|w| w == "y = 42"))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.debug_view() {
                v.update(cx, |v, cx| v.add_watch("y + 1", cx));
            }
        })),
        Step::WaitFor(
            "a watch shows its value in the paused frame and the threads are listed",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.watch_texts().iter().any(|w| w == "y + 1 = 43")
                        && !v.thread_names().is_empty()
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume runs the Node.js script to its end",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, _, _, console)| {
                    matches!(&s, DebugState::Ended(_)) && console.contains("result 42")
                })
            }),
        ),
        Step::Expect(
            "the values leave the editor with the session; the watches stay without a value",
            Box::new(|b, cx| {
                b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).inline_values().is_empty())
                    && b.debug_view()
                        .is_some_and(|v| v.read(cx).watch_texts() == ["y", "y + 1"])
            }),
        ),
    ]
}

/// Debugging a `package.json` script (SPEC-0168): the scripts get ▶ markers, Debug from the marker
/// runs the package manager under js-debug and stops in the TypeScript file the script starts,
/// with the locals; resume runs it to the end. Skipped without node with type stripping, npm or
/// js-debug.
fn package_script_debug_steps() -> Vec<Step> {
    let node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let npm = std::process::Command::new("npm")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let js_debug = idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "js-debug" && a.command.is_some());
    if !idecraft_debugger::strips_types(&node) || !npm || !js_debug {
        eprintln!("package script debug steps skipped: node 22.18+, npm or js-debug is missing");
        return Vec::new();
    }
    let (resume, close) = if cfg!(target_os = "macos") {
        ("cmd-alt-r", "cmd-w")
    } else {
        ("f9", "ctrl-f4")
    };
    const PACKAGE: &str = "{\n  \"name\": \"npmdbg\",\n  \"private\": true,\n  \"scripts\": {\n    \"build\": \"echo nothing to build\",\n    \"start\": \"node app.ts\"\n  }\n}\n";
    const APP: &str = "function twice(x: number): number {\n  const y: number = x * 2;\n  return y;\n}\nconsole.log('script result ' + twice(21));\n";
    fn project() -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join("npmdbg"))
    }
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = project() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("package.json"), PACKAGE);
                let _ = std::fs::write(dir.join("app.ts"), APP);
                b.open(&dir.join("app.ts"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(2, cx));
                }
                b.open(&dir.join("package.json"), window, cx);
            }
        })),
        Step::WaitFor(
            "the scripts of package.json get ▶ markers",
            Box::new(|b, cx| {
                b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).run_markers() == [4, 5])
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(p) = b.active_file() {
                b.run_marker_menu(&p, 5, window, cx);
            }
        })),
        Step::Expect(
            "the marker of a script offers Run and Debug with the package manager",
            Box::new(|b, cx| {
                b.jvm_popup().is_some_and(|p| {
                    p.read(cx).items() == ["Run 'npm run start'", "Debug 'npm run start'"]
                })
            }),
        ),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::WaitLong(
            "Debug runs npm and stops in the TypeScript file the script starts",
            std::time::Duration::from_secs(60),
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Paused(_))
                        && v.frame_names().first().is_some_and(|f| f.contains("twice"))
                        && v.variable_texts().iter().any(|t| t == "y = 42")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "resume runs the script to its end",
            Box::new(|b, cx| {
                b.debug_view().is_some_and(|v| {
                    let v = v.read(cx);
                    matches!(v.state(), DebugState::Ended(_))
                        && v.console().contains("script result 42")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = project() {
                b.open(&dir.join("app.ts"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.toggle_breakpoint(2, cx));
                }
            }
            b.focus_editor(window, cx);
        })),
        Step::Key(close, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = project() {
                let _ = std::fs::remove_dir_all(dir);
            }
        })),
    ]
}

/// The Problems tool window (SPEC-0092): ⌘6 lists the problems of the current file, Down and Enter
/// put the caret on one.
fn problems_steps() -> Vec<Step> {
    let key = if cfg!(target_os = "macos") {
        "cmd-6"
    } else {
        "alt-6"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                b.open(&dir.join("bad.c"), window, cx);
            }
        })),
        Step::WaitFor(
            "bad.c has its error",
            Box::new(|b, cx| b.active_problems(cx).is_some_and(|(errors, _)| errors >= 1)),
        ),
        Step::Key(key, None),
        Step::WaitFor(
            "the problems tool window lists the errors of the file",
            Box::new(|b, cx| {
                // The File tab starts with the heading of the file (SPEC-0092 v3 behaviour 6).
                b.problems_rows(cx).is_some_and(|rows| {
                    rows.first().is_some_and(|r| r == "bad.c")
                        && rows.len() > 1
                        && rows[1..].iter().all(|r| r.contains("clang:"))
                })
            }),
        ),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::Expect(
            "enter puts the caret on the problem",
            Box::new(|b, cx| {
                let first = b
                    .problems_list(cx)
                    .into_iter()
                    .min_by_key(|p| (p.line, p.column));
                first.is_some_and(|p| b.caret_position(cx) == Some((p.line + 1, p.column + 1)))
            }),
        ),
    ]
}

/// IDECraft's own inspections (SPEC-0098): an empty catch block in a Java file is marked in the
/// editor, and Inspect Code lists it for the whole project in the Problems tool window.
fn inspection_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let text = "class Insp {\n    void f() {\n        try { g(); } catch (Exception e) {}\n    }\n    void g() {}\n}\n";
                let _ = std::fs::write(dir.join("Insp.java"), text);
                b.open(&dir.join("Insp.java"), window, cx);
                b.set_problems_scope(idecraft_workbench::problems_model::Scope::File, cx);
            }
        })),
        Step::WaitFor(
            "the editor marks the empty catch block",
            Box::new(|b, cx| {
                b.problems_list(cx)
                    .iter()
                    .any(|p| p.line == 2 && p.message.starts_with("IDECraft: Empty 'catch' block"))
            }),
        ),
        // SPEC-0098 v3 behaviour 7: the scope dialog first, Whole project chosen.
        Step::Act(Box::new(|b, window, cx| b.inspect_code(window, cx))),
        Step::WaitFor(
            "inspect code asks for the scope",
            Box::new(|b, _| {
                b.inspect_dialog()
                    .is_some_and(|(choices, at)| at == 0 && choices[0] == "Whole project")
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.confirm_inspect_dialog(window, cx)
        })),
        Step::WaitFor(
            "inspect code lists the finding under its inspection and file",
            Box::new(|b, cx| {
                !b.inspecting()
                    && b.problems_rows(cx).is_some_and(|rows| {
                        rows.iter().any(|r| r.starts_with("Empty 'catch' block ("))
                            && rows.iter().any(|r| r.starts_with("Insp.java ("))
                            && rows.iter().any(|r| r.starts_with("3:"))
                    })
            }),
        ),
    ]
}

/// A plugin that turns ASCII letters into capitals (WebAssembly, built from the text format in
/// `crates/plugins/tests/runtime.rs`).
const UPPER_PLUGIN: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x12, 0x03, 0x60, 0x01, 0x7f, 0x01, 0x7f,
    0x60, 0x00, 0x01, 0x7e, 0x60, 0x04, 0x7f, 0x7f, 0x7f, 0x7f, 0x01, 0x7e, 0x03, 0x04, 0x03, 0x00,
    0x01, 0x02, 0x05, 0x03, 0x01, 0x00, 0x01, 0x06, 0x07, 0x01, 0x7f, 0x01, 0x41, 0x80, 0x20, 0x0b,
    0x07, 0x3e, 0x04, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x0e, 0x69, 0x64, 0x65,
    0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x61, 0x6c, 0x6c, 0x6f, 0x63, 0x00, 0x00, 0x11, 0x69, 0x64,
    0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x6d, 0x61, 0x6e, 0x69, 0x66, 0x65, 0x73, 0x74, 0x00,
    0x01, 0x0c, 0x69, 0x64, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x72, 0x75, 0x6e, 0x00, 0x02,
    0x0a, 0x6f, 0x03, 0x11, 0x01, 0x01, 0x7f, 0x23, 0x00, 0x21, 0x01, 0x23, 0x00, 0x20, 0x00, 0x6a,
    0x24, 0x00, 0x20, 0x01, 0x0b, 0x0b, 0x00, 0x42, 0x10, 0x42, 0x20, 0x86, 0x42, 0xc0, 0x00, 0x84,
    0x0b, 0x4f, 0x01, 0x02, 0x7f, 0x02, 0x40, 0x03, 0x40, 0x20, 0x04, 0x20, 0x03, 0x4f, 0x0d, 0x01,
    0x20, 0x02, 0x20, 0x04, 0x6a, 0x2d, 0x00, 0x00, 0x21, 0x05, 0x20, 0x05, 0x41, 0xe1, 0x00, 0x4f,
    0x20, 0x05, 0x41, 0xfa, 0x00, 0x4d, 0x71, 0x04, 0x40, 0x20, 0x05, 0x41, 0x20, 0x6b, 0x21, 0x05,
    0x0b, 0x20, 0x02, 0x20, 0x04, 0x6a, 0x20, 0x05, 0x3a, 0x00, 0x00, 0x20, 0x04, 0x41, 0x01, 0x6a,
    0x21, 0x04, 0x0c, 0x00, 0x0b, 0x0b, 0x20, 0x02, 0xad, 0x42, 0x20, 0x86, 0x20, 0x03, 0xad, 0x84,
    0x0b, 0x0b, 0x46, 0x01, 0x00, 0x41, 0x10, 0x0b, 0x40, 0x7b, 0x22, 0x6e, 0x61, 0x6d, 0x65, 0x22,
    0x3a, 0x22, 0x55, 0x70, 0x70, 0x65, 0x72, 0x22, 0x2c, 0x22, 0x61, 0x63, 0x74, 0x69, 0x6f, 0x6e,
    0x73, 0x22, 0x3a, 0x5b, 0x7b, 0x22, 0x69, 0x64, 0x22, 0x3a, 0x22, 0x75, 0x70, 0x70, 0x65, 0x72,
    0x22, 0x2c, 0x22, 0x74, 0x69, 0x74, 0x6c, 0x65, 0x22, 0x3a, 0x22, 0x55, 0x70, 0x70, 0x65, 0x72,
    0x20, 0x43, 0x61, 0x73, 0x65, 0x22, 0x7d, 0x5d, 0x7d, 0x00, 0x42, 0x04, 0x6e, 0x61, 0x6d, 0x65,
    0x02, 0x21, 0x02, 0x00, 0x02, 0x00, 0x03, 0x6c, 0x65, 0x6e, 0x01, 0x02, 0x61, 0x74, 0x02, 0x06,
    0x00, 0x01, 0x61, 0x01, 0x02, 0x61, 0x6c, 0x02, 0x01, 0x70, 0x03, 0x01, 0x6c, 0x04, 0x01, 0x69,
    0x05, 0x01, 0x63, 0x03, 0x0f, 0x01, 0x02, 0x02, 0x00, 0x04, 0x64, 0x6f, 0x6e, 0x65, 0x01, 0x04,
    0x6e, 0x65, 0x78, 0x74, 0x07, 0x07, 0x01, 0x00, 0x04, 0x6e, 0x65, 0x78, 0x74,
];

/// A plugin (SPEC-0156) whose `edit` action reads `b.rs` of the project through the host: with the
/// user's grant it inserts `// ` at the start and selects it ("marked"), else it says "denied".
/// Built from the WebAssembly text format.
const MARKER_PLUGIN: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x18, 0x04, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7e, 0x60, 0x01, 0x7f, 0x01, 0x7f, 0x60, 0x00, 0x01, 0x7e, 0x60, 0x04, 0x7f, 0x7f, 0x7f, 0x7f,
    0x01, 0x7e, 0x02, 0x16, 0x01, 0x08, 0x69, 0x64, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x09, 0x72,
    0x65, 0x61, 0x64, 0x5f, 0x66, 0x69, 0x6c, 0x65, 0x00, 0x00, 0x03, 0x04, 0x03, 0x01, 0x02, 0x03,
    0x05, 0x03, 0x01, 0x00, 0x01, 0x06, 0x07, 0x01, 0x7f, 0x01, 0x41, 0x80, 0x20, 0x0b, 0x07, 0x3e,
    0x04, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x0e, 0x69, 0x64, 0x65, 0x63, 0x72,
    0x61, 0x66, 0x74, 0x5f, 0x61, 0x6c, 0x6c, 0x6f, 0x63, 0x00, 0x01, 0x11, 0x69, 0x64, 0x65, 0x63,
    0x72, 0x61, 0x66, 0x74, 0x5f, 0x6d, 0x61, 0x6e, 0x69, 0x66, 0x65, 0x73, 0x74, 0x00, 0x02, 0x0c,
    0x69, 0x64, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x72, 0x75, 0x6e, 0x00, 0x03, 0x0a, 0x43,
    0x03, 0x11, 0x01, 0x01, 0x7f, 0x23, 0x00, 0x21, 0x01, 0x23, 0x00, 0x20, 0x00, 0x6a, 0x24, 0x00,
    0x20, 0x01, 0x0b, 0x0b, 0x00, 0x42, 0x10, 0x42, 0x20, 0x86, 0x42, 0xf7, 0x00, 0x84, 0x0b, 0x23,
    0x00, 0x41, 0xe8, 0x07, 0x41, 0x04, 0x10, 0x00, 0x42, 0x00, 0x53, 0x04, 0x7e, 0x42, 0x80, 0x06,
    0x42, 0x20, 0x86, 0x42, 0x14, 0x84, 0x05, 0x42, 0x80, 0x04, 0x42, 0x20, 0x86, 0x42, 0xce, 0x00,
    0x84, 0x0b, 0x0b, 0x0b, 0xf5, 0x01, 0x04, 0x00, 0x41, 0x10, 0x0b, 0x77, 0x7b, 0x22, 0x6e, 0x61,
    0x6d, 0x65, 0x22, 0x3a, 0x22, 0x4d, 0x61, 0x72, 0x6b, 0x65, 0x72, 0x22, 0x2c, 0x22, 0x76, 0x65,
    0x72, 0x73, 0x69, 0x6f, 0x6e, 0x22, 0x3a, 0x22, 0x31, 0x2e, 0x30, 0x22, 0x2c, 0x22, 0x70, 0x65,
    0x72, 0x6d, 0x69, 0x73, 0x73, 0x69, 0x6f, 0x6e, 0x73, 0x22, 0x3a, 0x5b, 0x22, 0x70, 0x72, 0x6f,
    0x6a, 0x65, 0x63, 0x74, 0x2d, 0x72, 0x65, 0x61, 0x64, 0x22, 0x5d, 0x2c, 0x22, 0x61, 0x63, 0x74,
    0x69, 0x6f, 0x6e, 0x73, 0x22, 0x3a, 0x5b, 0x7b, 0x22, 0x69, 0x64, 0x22, 0x3a, 0x22, 0x6d, 0x61,
    0x72, 0x6b, 0x22, 0x2c, 0x22, 0x74, 0x69, 0x74, 0x6c, 0x65, 0x22, 0x3a, 0x22, 0x4d, 0x61, 0x72,
    0x6b, 0x22, 0x2c, 0x22, 0x6b, 0x69, 0x6e, 0x64, 0x22, 0x3a, 0x22, 0x65, 0x64, 0x69, 0x74, 0x22,
    0x7d, 0x5d, 0x7d, 0x00, 0x41, 0x80, 0x04, 0x0b, 0x4e, 0x7b, 0x22, 0x65, 0x64, 0x69, 0x74, 0x73,
    0x22, 0x3a, 0x5b, 0x7b, 0x22, 0x73, 0x74, 0x61, 0x72, 0x74, 0x22, 0x3a, 0x30, 0x2c, 0x22, 0x65,
    0x6e, 0x64, 0x22, 0x3a, 0x30, 0x2c, 0x22, 0x74, 0x65, 0x78, 0x74, 0x22, 0x3a, 0x22, 0x2f, 0x2f,
    0x20, 0x22, 0x7d, 0x5d, 0x2c, 0x22, 0x73, 0x65, 0x6c, 0x65, 0x63, 0x74, 0x22, 0x3a, 0x5b, 0x30,
    0x2c, 0x33, 0x5d, 0x2c, 0x22, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x22, 0x3a, 0x22, 0x6d,
    0x61, 0x72, 0x6b, 0x65, 0x64, 0x22, 0x7d, 0x00, 0x41, 0x80, 0x06, 0x0b, 0x14, 0x7b, 0x22, 0x6d,
    0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x22, 0x3a, 0x22, 0x64, 0x65, 0x6e, 0x69, 0x65, 0x64, 0x22,
    0x7d, 0x00, 0x41, 0xe8, 0x07, 0x0b, 0x04, 0x62, 0x2e, 0x72, 0x73, 0x00, 0x23, 0x04, 0x6e, 0x61,
    0x6d, 0x65, 0x01, 0x07, 0x01, 0x00, 0x04, 0x72, 0x65, 0x61, 0x64, 0x02, 0x0a, 0x01, 0x01, 0x02,
    0x00, 0x01, 0x6e, 0x01, 0x02, 0x61, 0x74, 0x07, 0x07, 0x01, 0x00, 0x04, 0x6e, 0x65, 0x78, 0x74,
];

/// A plugin (SPEC-0166) with an `inspect` action on `.txt` files that reports `TODO` at the start
/// with the fix "Remove TODO" (built from `TODO` in `crates/plugins/tests/runtime.rs`).
const TODO_PLUGIN: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x12, 0x03, 0x60, 0x01, 0x7f, 0x01, 0x7f,
    0x60, 0x00, 0x01, 0x7e, 0x60, 0x04, 0x7f, 0x7f, 0x7f, 0x7f, 0x01, 0x7e, 0x03, 0x04, 0x03, 0x00,
    0x01, 0x02, 0x05, 0x03, 0x01, 0x00, 0x02, 0x06, 0x08, 0x01, 0x7f, 0x01, 0x41, 0x80, 0xc0, 0x00,
    0x0b, 0x07, 0x3e, 0x04, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x0e, 0x69, 0x64,
    0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x61, 0x6c, 0x6c, 0x6f, 0x63, 0x00, 0x00, 0x11, 0x69,
    0x64, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x6d, 0x61, 0x6e, 0x69, 0x66, 0x65, 0x73, 0x74,
    0x00, 0x01, 0x0c, 0x69, 0x64, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x5f, 0x72, 0x75, 0x6e, 0x00,
    0x02, 0x0a, 0x42, 0x03, 0x11, 0x01, 0x01, 0x7f, 0x23, 0x00, 0x21, 0x01, 0x23, 0x00, 0x20, 0x00,
    0x6a, 0x24, 0x00, 0x20, 0x01, 0x0b, 0x0b, 0x00, 0x42, 0x10, 0x42, 0x20, 0x86, 0x42, 0x9b, 0x01,
    0x84, 0x0b, 0x22, 0x00, 0x20, 0x00, 0x2d, 0x00, 0x00, 0x41, 0xe2, 0x00, 0x46, 0x04, 0x40, 0x42,
    0x80, 0x10, 0x42, 0x20, 0x86, 0x42, 0x36, 0x84, 0x0f, 0x0b, 0x42, 0x80, 0x08, 0x42, 0x20, 0x86,
    0x42, 0xfe, 0x00, 0x84, 0x0b, 0x0b, 0xe2, 0x02, 0x03, 0x00, 0x41, 0x10, 0x0b, 0x9b, 0x01, 0x7b,
    0x22, 0x6e, 0x61, 0x6d, 0x65, 0x22, 0x3a, 0x22, 0x54, 0x6f, 0x64, 0x6f, 0x22, 0x2c, 0x22, 0x76,
    0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x22, 0x3a, 0x22, 0x31, 0x2e, 0x30, 0x22, 0x2c, 0x22, 0x61,
    0x63, 0x74, 0x69, 0x6f, 0x6e, 0x73, 0x22, 0x3a, 0x5b, 0x7b, 0x22, 0x69, 0x64, 0x22, 0x3a, 0x22,
    0x74, 0x6f, 0x64, 0x6f, 0x22, 0x2c, 0x22, 0x74, 0x69, 0x74, 0x6c, 0x65, 0x22, 0x3a, 0x22, 0x54,
    0x4f, 0x44, 0x4f, 0x20, 0x63, 0x68, 0x65, 0x63, 0x6b, 0x22, 0x2c, 0x22, 0x6b, 0x69, 0x6e, 0x64,
    0x22, 0x3a, 0x22, 0x69, 0x6e, 0x73, 0x70, 0x65, 0x63, 0x74, 0x22, 0x2c, 0x22, 0x66, 0x69, 0x6c,
    0x65, 0x73, 0x22, 0x3a, 0x5b, 0x22, 0x74, 0x78, 0x74, 0x22, 0x5d, 0x7d, 0x2c, 0x7b, 0x22, 0x69,
    0x64, 0x22, 0x3a, 0x22, 0x62, 0x61, 0x64, 0x22, 0x2c, 0x22, 0x74, 0x69, 0x74, 0x6c, 0x65, 0x22,
    0x3a, 0x22, 0x42, 0x61, 0x64, 0x22, 0x2c, 0x22, 0x6b, 0x69, 0x6e, 0x64, 0x22, 0x3a, 0x22, 0x69,
    0x6e, 0x73, 0x70, 0x65, 0x63, 0x74, 0x22, 0x7d, 0x5d, 0x7d, 0x00, 0x41, 0x80, 0x08, 0x0b, 0x7e,
    0x7b, 0x22, 0x66, 0x69, 0x6e, 0x64, 0x69, 0x6e, 0x67, 0x73, 0x22, 0x3a, 0x5b, 0x7b, 0x22, 0x73,
    0x74, 0x61, 0x72, 0x74, 0x22, 0x3a, 0x30, 0x2c, 0x22, 0x65, 0x6e, 0x64, 0x22, 0x3a, 0x34, 0x2c,
    0x22, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x22, 0x3a, 0x22, 0x54, 0x4f, 0x44, 0x4f, 0x20,
    0x6c, 0x65, 0x66, 0x74, 0x22, 0x2c, 0x22, 0x66, 0x69, 0x78, 0x22, 0x3a, 0x7b, 0x22, 0x74, 0x69,
    0x74, 0x6c, 0x65, 0x22, 0x3a, 0x22, 0x52, 0x65, 0x6d, 0x6f, 0x76, 0x65, 0x20, 0x54, 0x4f, 0x44,
    0x4f, 0x22, 0x2c, 0x22, 0x65, 0x64, 0x69, 0x74, 0x73, 0x22, 0x3a, 0x5b, 0x7b, 0x22, 0x73, 0x74,
    0x61, 0x72, 0x74, 0x22, 0x3a, 0x30, 0x2c, 0x22, 0x65, 0x6e, 0x64, 0x22, 0x3a, 0x35, 0x2c, 0x22,
    0x74, 0x65, 0x78, 0x74, 0x22, 0x3a, 0x22, 0x22, 0x7d, 0x5d, 0x7d, 0x7d, 0x5d, 0x7d, 0x00, 0x41,
    0x80, 0x10, 0x0b, 0x36, 0x7b, 0x22, 0x66, 0x69, 0x6e, 0x64, 0x69, 0x6e, 0x67, 0x73, 0x22, 0x3a,
    0x5b, 0x7b, 0x22, 0x73, 0x74, 0x61, 0x72, 0x74, 0x22, 0x3a, 0x30, 0x2c, 0x22, 0x65, 0x6e, 0x64,
    0x22, 0x3a, 0x39, 0x39, 0x39, 0x39, 0x39, 0x2c, 0x22, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65,
    0x22, 0x3a, 0x22, 0x6f, 0x75, 0x74, 0x22, 0x7d, 0x5d, 0x7d, 0x00, 0x2b, 0x04, 0x6e, 0x61, 0x6d,
    0x65, 0x02, 0x1b, 0x02, 0x00, 0x02, 0x00, 0x03, 0x6c, 0x65, 0x6e, 0x01, 0x02, 0x61, 0x74, 0x02,
    0x04, 0x00, 0x01, 0x61, 0x01, 0x02, 0x61, 0x6c, 0x02, 0x01, 0x70, 0x03, 0x01, 0x6c, 0x07, 0x07,
    0x01, 0x00, 0x04, 0x6e, 0x65, 0x78, 0x74,
];

/// Plugin inspections (SPEC-0166): an installed plugin's finding appears in the editor after a
/// pause, its fix is offered by ⌥↩ and applied with Enter.
fn plugin_inspection_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let plugins = dir.join(".plugins-inspect");
                let _ = std::fs::create_dir_all(&plugins);
                let _ = std::fs::write(plugins.join("todo.wasm"), TODO_PLUGIN);
                b.set_plugins_dir(plugins);
                b.reload_plugins(cx);
                let _ = std::fs::write(dir.join("todo.txt"), "TODO write\n");
                b.open(&dir.join("todo.txt"), window, cx);
            }
        })),
        Step::WaitFor(
            "a plugin's inspection reports its finding in the editor after a pause",
            Box::new(|b, cx| {
                let dir = FIXTURE.get().expect("fixture");
                b.plugin_findings(&dir.join("todo.txt")) == ["Todo: TODO left"]
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).status().problems.1 > 0)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(0, 1, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ offers the plugin's fix",
            Box::new(|b, cx| {
                b.intentions_popup()
                    .is_some_and(|v| v.read(cx).titles().iter().any(|t| t == "Remove TODO"))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.intentions_popup() {
                v.update(cx, |v, cx| v.choose_title("Remove TODO", cx));
            }
        })),
        Step::WaitFor(
            "applying it edits the file",
            Box::new(|b, cx| b.active_text(cx).is_some_and(|t| t == "write\n")),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join(".plugins-inspect"));
                b.reload_plugins(cx);
            }
        })),
    ]
}

/// The sample plugins of `tests/plugins/` (SPEC-0210), one per extension point.
const SAMPLE_PLUGINS: [(&str, &[u8]); 4] = [
    (
        "complete",
        include_bytes!("../../../tests/plugins/complete.wasm"),
    ),
    (
        "filetype",
        include_bytes!("../../../tests/plugins/filetype.wasm"),
    ),
    (
        "status",
        include_bytes!("../../../tests/plugins/status.wasm"),
    ),
    ("panel", include_bytes!("../../../tests/plugins/panel.wasm")),
];

/// Plugin extension points (SPEC-0210): a plugin file type colours a `.conf` file and ⌘/ uses its
/// comment; ⌃Space lists the plugin's completion; the status bar shows its widget; the Plugins tool
/// window lists the panel's rows and a row opens its file.
fn plugin_extension_steps() -> Vec<Step> {
    let comment = if cfg!(target_os = "macos") {
        "cmd-/"
    } else {
        "ctrl-/"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let plugins = dir.join(".plugins-extend");
                let _ = std::fs::create_dir_all(&plugins);
                for (name, bytes) in SAMPLE_PLUGINS {
                    let _ = std::fs::write(plugins.join(format!("{name}.wasm")), bytes);
                }
                b.set_plugins_dir(plugins);
                let _ = std::fs::write(dir.join("notes.conf"), "# notes\nserver on\nplu");
                b.open(&dir.join("notes.conf"), window, cx);
                b.reload_plugins(cx);
            }
        })),
        Step::WaitFor(
            "the plugin's file type highlights the file",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    e.plugin_file_type() == Some("Conf")
                        && e.plugin_span_kinds() == ["comment", "keyword", "keyword"]
                })
            }),
        ),
        Step::WaitFor(
            "the plugin's status widget shows the file's lines",
            Box::new(|b, _| b.plugin_status_texts() == ["Lines: 3"]),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(1, 0, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key(comment, None),
        Step::Expect(
            "Comment with Line Comment uses the file type's prefix",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.lines()
                        .nth(1)
                        .is_some_and(|l| l.starts_with('#') && l.ends_with("server on"))
                })
            }),
        ),
        // The caret went one line down (SPEC-0056 v3 behaviour 15); back to uncomment the line.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(1, 0, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key(comment, None),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(2, 3, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key("ctrl-space", None),
        Step::WaitFor(
            "ctrl+space lists the plugin's completion items",
            Box::new(|b, _| {
                b.completion_items()
                    .is_some_and(|items| items == ["plugin_hello", "plugin_world"])
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "enter inserts the plugin's item",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t == "# notes\nserver on\nplugin_hello")
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ActivatePluginsToolWindow", window, cx)
        })),
        Step::WaitFor(
            "the Plugins tool window lists the panel's rows",
            Box::new(|b, _| {
                b.tool_window_shown(idecraft_workbench::tool_windows::ToolWindow::Plugins)
                    && b.plugin_panels().len() == 1
                    && b.plugin_panels()[0].title == "Sample Panel: Bookmarks"
                    && b.plugin_panels()[0]
                        .rows
                        .as_ref()
                        .is_ok_and(|rows| rows.len() == 2)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("other.txt"), "x\n");
                b.open(&dir.join("other.txt"), window, cx);
            }
            b.open_plugin_row(0, 0, window, cx);
        })),
        Step::Expect(
            "a row opens its file at its line",
            Box::new(|b, cx| {
                b.active_path().is_some_and(|p| p.ends_with("notes.conf"))
                    && b.caret_position(cx).is_some_and(|(line, _)| line == 2)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ActivatePluginsToolWindow", window, cx);
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join(".plugins-extend"));
            }
            b.reload_plugins(cx);
        })),
        Step::Expect(
            "without the plugins the file is plain again",
            Box::new(|b, cx| {
                !b.tool_window_shown(idecraft_workbench::tool_windows::ToolWindow::Plugins)
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).plugin_file_type().is_none())
            }),
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-s"
            } else {
                "ctrl-s"
            },
            None,
        ),
    ]
}

/// The resource bundle editor (SPEC-0212): keys with the missing one marked, a value written into
/// the open German file as escaped text, a key added to the default file and removed again.
fn bundle_editor_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("messages.properties"), "hello=Hello\nbye=Bye\n");
                let _ = std::fs::write(dir.join("messages_de.properties"), "hello=Hallo\n");
                b.open(&dir.join("messages_de.properties"), window, cx);
                b.menu_action("OpenResourceBundleEditor", window, cx);
            }
        })),
        Step::Expect(
            "the dialog lists the keys and marks the one missing in German",
            Box::new(|b, cx| {
                b.bundle_editor().is_some_and(|v| {
                    let v = v.read(cx);
                    v.title() == "Resource Bundle 'messages'"
                        && v.visible_keys(cx) == ["hello", "bye"]
                        && v.bundle().keys[1].missing() == [1]
                        && v.selected() == Some("hello")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.bundle_editor() {
                v.update(cx, |v, cx| {
                    v.select("bye", cx);
                    if let Some(f) = v.value_field(1) {
                        f.update(cx, |f, cx| f.set_text("Tschüss", cx));
                    }
                });
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "enter writes the value into the open German file, escaped as the file is ASCII",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t == "hello=Hallo\nbye=Tsch\\u00FCss\n")
                    && b.bundle_editor()
                        .is_some_and(|v| v.read(cx).bundle().keys[1].missing().is_empty())
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.bundle_editor() {
                v.update(cx, |v, cx| {
                    v.start_new_key(window, cx);
                    v.add_key("farewell", cx);
                });
            }
        })),
        Step::Expect(
            "a new key lands in the default file",
            Box::new(|b, cx| {
                let dir = FIXTURE.get().expect("fixture");
                std::fs::read_to_string(dir.join("messages.properties"))
                    .is_ok_and(|t| t == "hello=Hello\nbye=Bye\nfarewell=\n")
                    && b.bundle_editor()
                        .is_some_and(|v| v.read(cx).selected() == Some("farewell"))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.bundle_editor() {
                window.focus(&v.read(cx).focus());
            }
        })),
        Step::Key("delete", None),
        Step::Expect(
            "the first delete asks",
            Box::new(|b, cx| {
                b.bundle_editor().is_some_and(|v| {
                    v.read(cx)
                        .note()
                        .is_some_and(|n| n.starts_with("Press Delete again"))
                })
            }),
        ),
        Step::Key("delete", None),
        Step::Expect(
            "the second delete removes the key from every file",
            Box::new(|b, cx| {
                let dir = FIXTURE.get().expect("fixture");
                std::fs::read_to_string(dir.join("messages.properties"))
                    .is_ok_and(|t| t == "hello=Hello\nbye=Bye\n")
                    && b.bundle_editor()
                        .is_some_and(|v| v.read(cx).visible_keys(cx) == ["hello", "bye"])
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the dialog",
            Box::new(|b, _| b.bundle_editor().is_none()),
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-s"
            } else {
                "ctrl-s"
            },
            None,
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                for f in ["messages.properties", "messages_de.properties"] {
                    let _ = std::fs::remove_file(dir.join(f));
                }
            }
        })),
    ]
}

/// Plugins (SPEC-0104): an installed plugin's action is found by Search Everywhere and replaces the
/// selection with its output. The Settings page disables, enables, installs and removes plugins
/// (SPEC-0126).
fn plugin_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let plugins = dir.join(".plugins");
                let _ = std::fs::create_dir_all(&plugins);
                let _ = std::fs::write(plugins.join("upper.wasm"), UPPER_PLUGIN);
                b.set_plugins_dir(plugins);
                let _ = std::fs::write(dir.join("shout.txt"), "hello plugin\n");
                b.open(&dir.join("shout.txt"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.select_range(0..5, cx));
                }
            }
        })),
        Step::Act(Box::new(|b, _, cx| {
            let actions = b.reload_plugins(cx);
            if actions
                .iter()
                .any(|(id, title)| id == "Plugin.upper.upper" && title == "Upper: Upper Case")
            {
                b.run_plugin_action("Plugin.upper.upper", cx);
            }
        })),
        Step::WaitFor(
            "the plugin action replaces the selection with its output",
            Box::new(|b, cx| b.active_text(cx).is_some_and(|t| t == "HELLO plugin\n")),
        ),
        // Managing plugins in Settings (SPEC-0126).
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::Plugins, window, cx)
        })),
        Step::Expect(
            "the Plugins page lists the installed plugin, enabled",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page() == Page::Plugins
                        && v.plugins().len() == 1
                        && v.plugins()[0].enabled
                        && v.plugins()[0]
                            .manifest
                            .as_ref()
                            .is_some_and(|m| m.name == "Upper")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "enter disables it, and its action is gone",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| !v.read(cx).plugins()[0].enabled)
                    && b.plugin_count() == 0
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "enter enables it again",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).plugins()[0].enabled)
                    && b.plugin_count() == 1
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let (Some(dir), Some(v)) = (FIXTURE.get(), b.settings_dialog()) {
                let source = dir.join("annoy.wasm");
                let _ = std::fs::write(&source, UPPER_PLUGIN);
                // Kept until Enter installs it from there (SPEC-0211).
                v.update(cx, |v, cx| v.install_plugin(&source, cx));
            }
        })),
        // SPEC-0211: the file is shown for review first; only Enter installs it.
        Step::Expect(
            "installing from disk shows the plugin for review and installs nothing yet",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.plugins().len() == 1
                        && v.plugin_review().is_some_and(|r| {
                            r.headline() == "Install Upper" && r.contributions == ["1 action"]
                        })
                }) && b.plugin_count() == 1
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "enter installs it: a second plugin",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let files: Vec<_> = v
                        .read(cx)
                        .plugins()
                        .iter()
                        .map(|p| p.file.clone())
                        .collect();
                    files == ["annoy", "upper"]
                }) && b.plugin_count() == 2
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let (Some(dir), Some(v)) = (FIXTURE.get(), b.settings_dialog()) {
                let source = dir.join("annoy.wasm");
                let _ = std::fs::write(&source, UPPER_PLUGIN);
                v.update(cx, |v, cx| v.install_plugin(&source, cx));
                let _ = std::fs::remove_file(source);
            }
        })),
        Step::Expect(
            "the same file again is a reinstall",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    v.read(cx)
                        .plugin_review()
                        .is_some_and(|r| r.headline() == "Reinstall Upper")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape drops the review and keeps the dialog",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.plugin_review().is_none() && v.plugins().len() == 2
                })
            }),
        ),
        // The Mac's ⌫ removes as well while the search is empty (D18).
        Step::Key("backspace", None),
        Step::Expect(
            "the first delete only asks",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).plugins().len() == 2)
            }),
        ),
        Step::Key("backspace", None),
        Step::Expect(
            "the second delete removes it",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.plugins().len() == 1 && v.plugins()[0].file == "upper"
                }) && b.plugin_count() == 1
            }),
        ),
        Step::Key("escape", None),
        // The plugin API (SPEC-0156): an `edit` action gets the editor's context; reading the
        // project needs the user's grant (Space on the Plugins page).
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join(".plugins/marker.wasm"), MARKER_PLUGIN);
                let _ = std::fs::write(dir.join("mark.txt"), "text\n");
                b.open(&dir.join("mark.txt"), window, cx);
                b.reload_plugins(cx);
                b.run_plugin_action("Plugin.marker.mark", cx);
            }
        })),
        Step::WaitFor(
            "without the grant the plugin cannot read the project",
            Box::new(|b, cx| {
                b.status_message(cx).is_some_and(|m| m == "denied")
                    && b.active_text(cx).is_some_and(|t| t == "text\n")
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(Page::Plugins, window, cx);
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    if let Some(i) = v.plugins().iter().position(|p| p.file == "marker") {
                        v.select_row(i, cx);
                    }
                });
            }
        })),
        Step::Key("space", None),
        Step::Expect(
            "Space on the Plugins page grants it project access",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).plugin_granted("marker"))
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, _, cx| {
            b.run_plugin_action("Plugin.marker.mark", cx);
        })),
        Step::WaitFor(
            "with the grant its edit is applied and selected",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| t == "// text\n")
                    && b.active_editor_entity()
                        .is_some_and(|e| e.read(cx).selection_range() == Some(0..3))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = idecraft_plugins_uninstall(&dir.join(".plugins"), "marker");
            }
            b.reload_plugins(cx);
        })),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-s"
            } else {
                "ctrl-s"
            },
            None,
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-f4"
            },
            None,
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("mark.txt"));
            }
        })),
    ]
}

/// Removes a self-test plugin and its grant.
fn idecraft_plugins_uninstall(dir: &std::path::Path, file: &str) -> std::io::Result<()> {
    std::fs::remove_file(dir.join(format!("{file}.wasm")))?;
    let grants = dir.join("permissions.json");
    if grants.exists() {
        std::fs::remove_file(grants)?;
    }
    Ok(())
}

/// The build tools window (SPEC-0100): a Go module lists its tasks; running "test" shows the
/// output in the Run window. Skipped without Go.
fn build_tools_steps() -> Vec<Step> {
    let go = std::process::Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !go {
        eprintln!("build tools steps skipped: go is not installed");
        return Vec::new();
    }
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let module = dir.join("gomod");
                let _ = std::fs::create_dir_all(&module);
                let _ = std::fs::write(module.join("go.mod"), "module gomod\n\ngo 1.21\n");
                let _ = std::fs::write(
                    module.join("calc_test.go"),
                    "package gomod\n\nimport \"testing\"\n\nfunc TestGood(t *testing.T) {}\n\nfunc TestBroken(t *testing.T) { t.Error(\"want 3\") }\n",
                );
            }
            b.toggle_build_tools(window, cx);
        })),
        Step::WaitFor(
            "the build tools window lists the Go module",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.loading() && v.row_texts().iter().any(|r| r == "gomod · Go · gomod")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    // The Go module's Tasks group: the one after its module row.
                    let rows = v.row_texts();
                    let at = rows.iter().position(|r| r == "gomod · Go · gomod");
                    if let Some(at) = at
                        && rows.get(at + 1).is_some_and(|r| r.trim() == "Tasks")
                    {
                        v.activate(at + 1, cx);
                    }
                });
            }
        })),
        Step::Expect(
            "the tasks of a Go module are listed",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let rows = v.read(cx).row_texts();
                    ["build", "test", "vet", "mod tidy"]
                        .iter()
                        .all(|t| rows.iter().any(|r| r.trim() == *t))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    let rows = v.rows();
                    let at = rows.iter().position(|r| {
                        r.label == "test"
                            && matches!(&r.kind, idecraft_workbench::build_tools_model::RowKind::Task(t) if t.dir == "gomod")
                    });
                    if let Some(at) = at {
                        v.activate(at, cx);
                    }
                });
            }
        })),
        Step::WaitFor(
            "a task runs in the Run window with its output",
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).selected_output().is_some_and(|(name, text)| {
                        name == "gomod [test]" && text.contains("TestBroken")
                    })
                })
            }),
        ),
    ]
    .into_iter()
    .chain(maven_tree_steps())
    .chain(maven_window_steps())
    .chain(maven_v3_steps())
    .collect()
}

/// The build tools window of SPEC-0100 v2 on the Maven module of `maven_tree_steps` (AK-10):
/// Lifecycle with nine phases and Plugins with goals, offline and skip tests as flags of a task's
/// command, the task menu (Copy Command Line), a typed goal run in the module, and the sync badge
/// over the changed POM that ⇧⌘I clears. The active editor and the options are restored after.
fn maven_window_steps() -> Vec<Step> {
    let mvn = std::process::Command::new("mvn")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !mvn {
        eprintln!("maven window steps skipped: mvn is not installed");
        return Vec::new();
    }
    let save = if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    };
    let sync = if cfg!(target_os = "macos") {
        "cmd-shift-i"
    } else {
        "ctrl-shift-o"
    };
    let pom = || FIXTURE.get().map(|d| d.join("mvnapp/pom.xml"));
    let before: std::rc::Rc<std::cell::RefCell<Option<std::path::PathBuf>>> = Default::default();
    let remember = before.clone();
    vec![
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    let _ = v.select_label_in("mvnapp", "Lifecycle", cx);
                    if let Some(i) = v.rows().iter().position(|r| r.label == "mvnapp") {
                        // Open the module's Lifecycle and Plugins (the rows right below it).
                        v.expand_all(true, cx);
                        let _ = i;
                    }
                });
            }
        })),
        Step::Expect(
            "a Maven module lists the nine lifecycle phases and its plugins with goals",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let rows = v.read(cx).row_texts();
                    let at = rows
                        .iter()
                        .position(|r| r.trim() == "mvnapp · Maven · mvnapp");
                    at.is_some_and(|at| {
                        let rest: Vec<&str> = rows[at..].iter().map(|r| r.trim()).collect();
                        [
                            "Lifecycle",
                            "site",
                            "deploy",
                            "Plugins",
                            "compiler",
                            "compiler:testCompile",
                        ]
                        .iter()
                        .all(|t| rest.contains(t))
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    v.set_offline(true, cx);
                    v.set_skip_tests(true, cx);
                    let _ = v.select_label_in("mvnapp", "test", cx);
                });
            }
        })),
        Step::Expect(
            "offline and skip tests become flags of a task's command",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let v = v.read(cx);
                    match v.selected_row().map(|r| r.kind) {
                        Some(idecraft_workbench::build_tools_model::RowKind::Task(t)) => {
                            let config = v.run_config(&t);
                            config.command == ["mvn", "-o", "-DskipTests", "test"]
                                && config.name == "mvnapp [test]"
                        }
                        _ => false,
                    }
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    v.set_offline(false, cx);
                    v.set_skip_tests(false, cx);
                });
            }
            b.open_context_menu(
                idecraft_workbench::MenuPlace::BuildTools,
                None,
                gpui::point(gpui::px(900.), gpui::px(300.)),
                cx,
            );
            b.choose_context_item("Copy Command Line", window, cx);
        })),
        Step::Expect(
            "the task menu copies the command line",
            Box::new(|b, _| b.status_note() == Some("Copied: mvn test")),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| v.execute(cx));
            }
        })),
        // The Execute event reaches the workbench after this step's update.
        Step::WaitFor(
            "the Execute field opens for the module",
            Box::new(|b, _| {
                b.name_prompt_state()
                    .is_some_and(|(title, _)| title.starts_with("Execute Maven Goal"))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.set_name_field("mvn validate", cx);
            b.finish_name_prompt(true, window, cx);
        })),
        Step::WaitLong(
            "a typed goal runs in the selected module",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).selected_output().is_some_and(|(name, text)| {
                        name == "mvnapp [validate]" && text.contains("BUILD SUCCESS")
                    })
                })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            *remember.borrow_mut() = b.active_path();
            if let Some(p) = pom() {
                b.open(&p, window, cx);
            }
            b.check_build_files(cx);
        })),
        Step::Expect(
            "an opened POM is watched for changes",
            Box::new(move |b, _| pom().is_some_and(|p| b.build_file_watched(&p))),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    e.set_text(&text.replace("4.13.2", "4.13.1"), cx);
                });
            }
        })),
        Step::Key(save, None),
        Step::Act(Box::new(|b, _, cx| b.check_build_files(cx))),
        Step::WaitFor(
            "a changed POM shows the sync badge",
            Box::new(move |b, _| pom().is_some_and(|p| b.build_sync_badge_shown(&p))),
        ),
        Step::Key(sync, None),
        Step::Expect(
            "⇧⌘I syncs: the badge goes, the build tools window reads the model again",
            Box::new(move |b, cx| {
                pom().is_some_and(|p| !b.build_sync_badge_shown(&p))
                    && b.status_message(cx)
                        .is_some_and(|m| m.contains("Syncing build changes"))
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            // The POM back as it was, synced; the editor that was active before.
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    e.set_text(&text.replace("4.13.1", "4.13.2"), cx);
                });
            }
            b.save_all_editors(cx);
            b.sync_build_changes(window, cx);
            if let Some(p) = before.borrow().clone() {
                b.open(&p, window, cx);
            }
        })),
    ]
}

/// The build tools window of SPEC-0100 v3 on the Maven module of `maven_tree_steps` (AK-17):
/// suggestions in the Execute field and Tab, the task menu with Debug, the triggers and Assign
/// Shortcut, a Before Build task in Build Project, the analyzer, the diagram file, unlink and link,
/// and a recorded shortcut that runs its task. The editor, the triggers and the shortcut are put
/// back afterwards.
fn maven_v3_steps() -> Vec<Step> {
    let mvn = std::process::Command::new("mvn")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !mvn {
        eprintln!("maven v3 steps skipped: mvn is not installed");
        return Vec::new();
    }
    use idecraft_workbench::build_tools_model::RowKind;
    let shortcut = if cfg!(target_os = "macos") {
        "cmd-alt-shift-9"
    } else {
        "ctrl-alt-shift-9"
    };
    let action = "BuildTask:mvnapp|validate";
    let task_menu = |b: &mut WorkbenchView, cx: &mut gpui::Context<WorkbenchView>| {
        b.open_context_menu(
            idecraft_workbench::MenuPlace::BuildTools,
            None,
            gpui::point(gpui::px(900.), gpui::px(300.)),
            cx,
        );
    };
    let select = |label: &'static str| {
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    v.expand_all(true, cx);
                    let _ = v.select_label_in("mvnapp", label, cx);
                });
            }
        }))
    };
    vec![
        // Suggestions in the Execute field (behaviour 20).
        select("Lifecycle"),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| v.execute(cx));
            }
        })),
        Step::WaitFor(
            "the Execute field opens for the module",
            Box::new(|b, _| {
                b.name_prompt_state()
                    .is_some_and(|(title, _)| title.starts_with("Execute Maven Goal"))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| b.set_name_field("mvn comp", cx))),
        Step::Expect(
            "the Execute field suggests the compile phase and the compiler's goals for `mvn comp`",
            Box::new(|b, cx| {
                let list = b.build_command_suggestions(cx);
                list.first().is_some_and(|s| s.text == "compile")
                    && list.iter().any(|s| s.text == "compiler:compile")
            }),
        ),
        Step::Key("tab", None),
        Step::Expect(
            "Tab takes the first suggestion",
            Box::new(|b, cx| b.name_field_text(cx).as_deref() == Some("mvn compile ")),
        ),
        Step::Key("escape", None),
        // The task menu (behaviour 10) and a Before Build task in Build Project (behaviour 18).
        select("clean"),
        Step::Act(Box::new(move |b, _, cx| task_menu(b, cx))),
        Step::Expect(
            "the task menu offers Debug, the triggers and Assign Shortcut",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(_, items)| {
                    [
                        "Debug",
                        "Execute Before Build",
                        "Execute After Build",
                        "Execute Before Run",
                        "Assign Shortcut…",
                    ]
                    .iter()
                    .all(|l| items.iter().any(|(i, on)| i == l && *on))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Execute Before Build", window, cx);
        })),
        Step::Expect(
            "the task row shows its trigger",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    v.read(cx)
                        .row_texts()
                        .iter()
                        .any(|r| r.trim() == "clean · Before Build")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.build_project(window, cx))),
        Step::WaitLong(
            "Build Project runs the Before Build task, then mvn test-compile",
            std::time::Duration::from_secs(180),
            // One chain in the Run tab "Build Project" (v4; SPEC-0087 before-launch steps): each
            // step's command line, then its output.
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).selected_output().is_some_and(|(name, text)| {
                        let at = |goal: &str| {
                            text.lines().position(|l| {
                                l.starts_with("mvn ") && l.ends_with(&format!(" {goal}"))
                            })
                        };
                        let success = text.lines().filter(|l| l.contains("BUILD SUCCESS"));
                        name == "Build Project"
                            && at("clean")
                                .zip(at("test-compile"))
                                .is_some_and(|(clean, compile)| clean < compile)
                            && success.count() >= 2
                    })
                })
            }),
        ),
        select("clean"),
        Step::Act(Box::new(move |b, window, cx| {
            task_menu(b, cx);
            b.choose_context_item("Execute Before Build", window, cx);
        })),
        Step::Expect(
            "Execute Before Build again takes the trigger off",
            Box::new(|b, cx| {
                b.build_tools_view()
                    .is_some_and(|v| v.read(cx).selected_triggers() == [false; 5])
            }),
        ),
        // v4: a task before the sync runs in the tab "Before Sync", then the model is read.
        select("validate"),
        Step::Act(Box::new(move |b, window, cx| {
            task_menu(b, cx);
            b.choose_context_item("Execute Before Sync", window, cx);
        })),
        Step::Expect(
            "Execute Before Sync marks the task",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    v.read(cx)
                        .row_texts()
                        .iter()
                        .any(|r| r.trim() == "validate · Before Sync")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.sync_build_changes(window, cx))),
        Step::WaitLong(
            "a sync runs the Before Sync task first, then reads the model",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx)
                        .output_of("Before Sync")
                        .is_some_and(|t| t.contains("BUILD SUCCESS"))
                }) && b
                    // The note itself: a problem at the caret of the file left open by earlier
                    // steps stands in front of it in the status bar.
                    .status_note()
                    .is_some_and(|m| m.contains("Syncing build changes"))
            }),
        ),
        select("validate"),
        Step::Act(Box::new(move |b, window, cx| {
            task_menu(b, cx);
            b.choose_context_item("Execute Before Sync", window, cx);
        })),
        Step::Expect(
            "Execute Before Sync again takes it off",
            Box::new(|b, cx| {
                b.build_tools_view()
                    .is_some_and(|v| v.read(cx).selected_triggers() == [false; 5])
            }),
        ),
        // The dependency analyzer (behaviour 14).
        select("Lifecycle"),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| v.toggle_analysis(cx));
            }
        })),
        Step::WaitLong(
            "the analyzer shows the module's group at the top with all its artifacts",
            std::time::Duration::from_secs(180),
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let v = v.read(cx);
                    let rows = v.row_texts();
                    // Below the problems of the build files (behaviour 14), which other steps
                    // of the full run leave in the fixture.
                    let problems = v
                        .rows()
                        .iter()
                        .take_while(|r| matches!(r.kind, RowKind::Problem(_)))
                        .count();
                    v.analysis().is_some_and(|(m, _)| m == "mvnapp")
                        && rows.get(problems).is_some_and(|r| r == "Dependency Analyzer")
                        && rows.iter().any(|r| r.trim() == "No version conflicts")
                        // v4: every artifact below the conflicts.
                        && rows.iter().any(|r| r.trim() == "All Dependencies")
                        && rows.iter().any(|r| r.trim() == "junit:junit")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| v.toggle_analysis(cx));
            }
        })),
        // The diagram (v4 behaviour 15): drawn over the editor, its Mermaid text kept in
        // `.idecraft/diagrams/`; Escape closes it.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    let _ = v.select_label("mvnapp", cx);
                    v.show_diagram(cx);
                });
            }
        })),
        Step::WaitFor(
            "the diagram shows the module and junit as boxes and keeps the Mermaid text",
            Box::new(|b, cx| {
                let text = FIXTURE.get().and_then(|d| {
                    std::fs::read_to_string(d.join(".idecraft/diagrams/mvnapp-dependencies.mmd"))
                        .ok()
                });
                b.er_view().is_some_and(|v| {
                    let v = v.read(cx);
                    let names: Vec<&str> =
                        v.model().entities.iter().map(|e| e.name.as_str()).collect();
                    v.is_dependency_diagram()
                        && names.first() == Some(&"mvnapp")
                        // The version follows the POM steps before (4.13.1 or 4.13.2).
                        && names.iter().any(|n| n.starts_with("junit 4.13."))
                        && names.contains(&"hamcrest-core 1.3")
                        && v.model().relationships.len() == 2
                }) && text.is_some_and(|t| {
                    t.starts_with("graph LR\n  root[\"mvnapp\"]") && t.contains("junit")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the diagram",
            Box::new(|b, _| b.er_view().is_none()),
        ),
        // Unlink and link again (behaviour 16).
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    let _ = v.select_label("mvnapp", cx);
                    v.unlink_selected(cx);
                });
            }
        })),
        Step::WaitFor(
            "an unlinked module leaves the window",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.loading()
                        && v.links().1 == ["mvnapp"]
                        && !v
                            .rows()
                            .iter()
                            .any(|r| r.label == "mvnapp" && matches!(r.kind, RowKind::Module(_)))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    let _ = v.link("mvnapp/pom.xml", cx);
                });
            }
        })),
        Step::WaitFor(
            "linking its POM brings it back",
            Box::new(|b, cx| {
                b.build_tools_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.loading()
                        && v.links().1.is_empty()
                        && v.rows()
                            .iter()
                            .any(|r| r.label == "mvnapp" && matches!(r.kind, RowKind::Module(_)))
                })
            }),
        ),
        // A shortcut of its own (behaviour 19).
        select("validate"),
        Step::Act(Box::new(move |b, window, cx| {
            task_menu(b, cx);
            b.choose_context_item("Assign Shortcut…", window, cx);
        })),
        Step::Expect(
            "Assign Shortcut waits for the keys",
            Box::new(|b, cx| b.build_tools_view().is_some_and(|v| v.read(cx).recording())),
        ),
        Step::Key(shortcut, None),
        Step::Expect(
            "the keys become the task's shortcut",
            Box::new(move |_, _| {
                !idecraft_workbench::keymaps::current()
                    .shortcuts(action)
                    .is_empty()
            }),
        ),
        // The window keeps the focus; keys it does not use go to the keymap.
        Step::Key(shortcut, None),
        Step::WaitLong(
            "the shortcut runs the task",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx)
                        .output_of("mvnapp [validate]")
                        .is_some_and(|t| t.contains("BUILD SUCCESS"))
                })
            }),
        ),
        Step::Act(Box::new(move |_, _, cx| {
            let _ = idecraft_workbench::keymaps::set_shortcuts(action, None, cx);
        })),
    ]
}

/// Transitive Maven dependencies in the build tools window (SPEC-0149 AK-4): a new Maven module
/// shows its direct dependency first; the build tool's resolution then adds what junit needs.
/// Skipped without mvn.
fn maven_tree_steps() -> Vec<Step> {
    let mvn = std::process::Command::new("mvn")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !mvn {
        eprintln!("maven tree steps skipped: mvn is not installed");
        return Vec::new();
    }
    let junit = |b: &WorkbenchView, cx: &App| -> Option<(String, Vec<String>)> {
        let v = b.build_tools_view()?;
        let v = v.read(cx);
        let deps = v.dependencies_of("mvnapp")?.as_ref().ok()?;
        let j = deps.iter().find(|d| d.name == "junit:junit")?;
        Some((
            j.scope.clone(),
            j.children.iter().map(|c| c.name.clone()).collect(),
        ))
    };
    vec![
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let module = dir.join("mvnapp");
                let _ = std::fs::create_dir_all(&module);
                let _ = std::fs::write(
                    module.join("pom.xml"),
                    "<project xmlns=\"http://maven.apache.org/POM/4.0.0\"><modelVersion>4.0.0</modelVersion><groupId>com.acme</groupId><artifactId>mvnapp</artifactId><version>1.0</version><dependencies><dependency><groupId>junit</groupId><artifactId>junit</artifactId><version>4.13.2</version><scope>test</scope></dependency></dependencies></project>",
                );
            }
            if let Some(v) = b.build_tools_view() {
                v.update(cx, |v, cx| {
                    v.reload(cx);
                    v.resolve(false, cx);
                });
            }
        })),
        Step::Expect(
            "a stale Maven module is resolved by the build tool",
            Box::new(|b, cx| b.build_tools_view().is_some_and(|v| v.read(cx).resolving())),
        ),
        Step::WaitLong(
            "the resolved tree lists junit with its own dependency",
            std::time::Duration::from_secs(180),
            Box::new(move |b, cx| {
                b.build_tools_view()
                    .is_some_and(|v| !v.read(cx).resolving() && !v.read(cx).loading())
                    && junit(b, cx).is_some_and(|(scope, children)| {
                        scope == "test" && children == ["org.hamcrest:hamcrest-core"]
                    })
            }),
        ),
    ]
}

/// The Endpoints tool window (SPEC-0103): a Spring controller's routes are listed with the class
/// prefix; "Generate Request" writes a request for one and opens it.
fn endpoints_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("EpApi.java"),
                    "@RestController\n@RequestMapping(\"/ep\")\nclass EpApi {\n    @GetMapping(\"/ping\")\n    String ping() { return \"pong\"; }\n    @PostMapping\n    void add() {}\n}\n",
                );
            }
            b.toggle_endpoints(window, cx);
        })),
        Step::WaitFor(
            "the endpoints window lists the controller's routes with the class prefix",
            Box::new(|b, cx| {
                b.endpoints_view().is_some_and(|v| {
                    let v = v.read(cx);
                    let texts = v.texts();
                    !v.loading()
                        && texts.iter().any(|t| t == "GET /ep/ping EpApi.ping")
                        && texts.iter().any(|t| t == "POST /ep EpApi.add")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.endpoints_view() {
                v.update(cx, |v, cx| {
                    v.select_text("GET /ep/ping");
                    let _ = v.generate_request(cx);
                });
            }
        })),
        Step::WaitFor(
            "generate request writes and opens a request for the endpoint",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.contains("### EpApi.ping\nGET http://localhost:8080/ep/ping")
                })
            }),
        ),
    ]
}

/// Push and the VCS operations popup (SPEC-0109): with a bare repository next to the fixture as
/// `origin`, the Push dialog lists the local commit and pushes it; ⌃V (Alt+`) lists the operations
/// and its Fetch reports that everything is up to date.
fn push_steps() -> Vec<Step> {
    // The macOS keymap binds the popup to ⌃V, the default keymap to Alt+`.
    let quick_list = if cfg!(target_os = "macos") {
        "ctrl-v"
    } else {
        "alt-`"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let remote = dir.with_extension("remote.git");
                let _ = std::fs::remove_dir_all(&remote);
                let remote_text = remote.display().to_string();
                fixture_git(dir, &["init", "-q", "--bare", "-b", "main", &remote_text]);
                fixture_git(dir, &["remote", "add", "origin", &remote_text]);
                let branch = idecraft_workbench::chrome::current_branch(dir).unwrap_or_default();
                fixture_git(dir, &["push", "-q", "-u", "origin", &branch]);
                let _ = std::fs::write(dir.join("pushed.txt"), "to the remote\n");
                fixture_git(dir, &["add", "pushed.txt"]);
                fixture_git(dir, &["commit", "-q", "-m", "push me", "--", "pushed.txt"]);
            }
            b.open_push(window, cx);
        })),
        Step::Expect(
            "the Push dialog lists the commit the remote lacks",
            Box::new(|b, cx| {
                b.push_dialog().is_some_and(|v| {
                    v.read(cx).outgoing().is_some_and(|o| {
                        o.remote == "origin"
                            && o.commits.len() == 1
                            && o.commits[0].summary == "push me"
                    }) && v.read(cx).selected_files() == ["A  pushed.txt"]
                })
            }),
        ),
        // OBS-0022 L23: ⌘A with the dialog open selects the target field, the dialog stays.
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
            None,
        ),
        Step::Expect(
            "⌘A in the Push dialog stays in the dialog (SPEC-0109 v2 behaviour 8)",
            Box::new(|b, cx| {
                b.push_dialog()
                    .is_some_and(|v| v.read(cx).target_text(cx) == "main")
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter pushes the commit",
            Box::new(|b, _| b.status_note() == Some("Pushed 1 commit(s)")),
        ),
        Step::Key(quick_list, None),
        Step::Expect(
            "the VCS quick list shortcut lists the operations",
            Box::new(|b, cx| {
                b.vcs_ops_popup().is_some_and(|v| {
                    let rows = v.read(cx).rows();
                    // SPEC-0109 v2 behaviour 6: the reference's entries, 1–9 and 0.
                    rows.first().is_some_and(|r| r == "1. Commit…")
                        && rows.get(9).is_some_and(|r| r == "0. Unstash Changes…")
                        && rows.len() == 16
                })
            }),
        ),
        // Fetch has no digit any more.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.vcs_ops_popup() {
                v.update(cx, |v, cx| v.choose(11, cx));
            }
        })),
        Step::WaitFor(
            "Fetch from the popup reports that everything is up to date",
            Box::new(|b, _| b.status_note() == Some("Fetch: everything is up to date")),
        ),
        // SPEC-0144: another target branch with tags.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("pushed.txt"), "for review\n");
                fixture_git(
                    dir,
                    &["commit", "-q", "-m", "review me", "--", "pushed.txt"],
                );
                // Lightweight on purpose: `--follow-tags` alone left those behind (D35).
                fixture_git(dir, &["tag", "v-review"]);
            }
            b.open_push(window, cx);
            if let Some(v) = b.push_dialog() {
                v.update(cx, |v, cx| v.set_target("review/me", cx));
            }
        })),
        Step::Key("alt-t", None),
        Step::Expect(
            "the Push dialog takes a target branch and ⌥T adds tags",
            Box::new(|b, cx| {
                b.push_dialog().is_some_and(|v| {
                    let o = v.read(cx).options(cx);
                    o.target.as_deref() == Some("review/me") && o.tags && !o.force_with_lease
                })
            }),
        ),
        // SPEC-0150: main is protected, so ⌥F is refused for it.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.push_dialog() {
                v.update(cx, |v, cx| v.set_target("main", cx));
            }
        })),
        Step::Key("alt-f", None),
        Step::Expect(
            "force push to the protected main is refused with the reason",
            Box::new(|b, cx| {
                b.push_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.options(cx).force_with_lease
                        && v.refusal().is_some_and(|r| r.contains("protected"))
                        // OBS-0022 L24: the remote has main, so it is not "(new)".
                        && v.title(cx).ends_with("→ origin/main")
                })
            }),
        ),
        // Hand test 70: Enter in the target field pushes (no line break), and the title names the
        // typed target.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.push_dialog() {
                v.update(cx, |v, cx| v.set_target("review/me", cx));
                v.read(cx).focus_target(window, cx);
            }
        })),
        Step::Expect(
            "the Push dialog's title follows the target branch",
            Box::new(|b, cx| {
                b.push_dialog()
                    .is_some_and(|v| v.read(cx).title(cx).ends_with("→ origin/review/me (new)"))
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "the commit and its tag reach the target branch",
            Box::new(|b, _| {
                b.status_note() == Some("Pushed 1 commit(s)")
                    && FIXTURE.get().is_some_and(|dir| {
                        let refs = std::process::Command::new("git")
                            .args(["ls-remote", "origin"])
                            .current_dir(dir)
                            .output()
                            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                            .unwrap_or_default();
                        refs.contains("refs/heads/review/me") && refs.contains("refs/tags/v-review")
                    })
            }),
        ),
        Step::Key(quick_list, None),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.vcs_ops_popup() {
                v.update(cx, |v, cx| v.choose(10, cx));
            }
        })),
        Step::Expect(
            "Update Project asks merge or rebase, merge preselected",
            Box::new(|b, cx| {
                b.update_dialog()
                    .is_some_and(|v| v.read(cx).method() == idecraft_core::UpdateMethod::Merge)
            }),
        ),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::WaitFor(
            "Update with rebase runs and the choice is remembered",
            Box::new(|b, cx| {
                b.update_dialog().is_none()
                    && b.status_note() == Some("Update: already up to date")
                    && idecraft_workbench::appearance::ide_settings(cx)
                        .vcs
                        .update_method
                        == idecraft_core::UpdateMethod::Rebase
            }),
        ),
    ]
}

/// Commit tool window v5 (SPEC-0063 AK-7): placeholder, amend with the amended commit shown,
/// Group By Directory, Rollback with its confirmation, Commit and Push… opening the Push dialog.
/// Runs after `push_steps`, so `origin` exists.
fn commit_window_steps() -> Vec<Step> {
    fn with_commit(
        b: &mut WorkbenchView,
        cx: &mut Context<WorkbenchView>,
        f: impl FnOnce(
            &mut idecraft_workbench::CommitView,
            &mut Context<idecraft_workbench::CommitView>,
        ),
    ) {
        if let Some(view) = b.commit_view() {
            view.update(cx, f);
        }
    }
    fn commit_view_has(
        b: &WorkbenchView,
        cx: &App,
        f: impl Fn(&idecraft_workbench::CommitView, &App) -> bool,
    ) -> bool {
        b.commit_view().is_some_and(|v| f(v.read(cx), cx))
    }
    vec![
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("rollback.txt"), "base\n");
                fixture_git(dir, &["add", "rollback.txt"]);
                fixture_git(
                    dir,
                    &["commit", "-q", "-m", "rollback base", "--", "rollback.txt"],
                );
            }
            with_commit(b, cx, |c, cx| {
                c.set_message("", cx);
                c.refresh();
            });
        })),
        Step::Expect(
            "the empty message shows its placeholder and the amend switch names the last commit",
            Box::new(|b, cx| {
                commit_view_has(b, cx, |c, cx| {
                    c.placeholder_shown(cx)
                        && !c.amend()
                        && c.amend_target()
                            .is_some_and(|t| t.ends_with(" rollback base"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            with_commit(b, cx, |c, cx| c.set_amend(true, cx))
        })),
        Step::Expect(
            "amend fills the field with the last commit's message",
            Box::new(|b, cx| {
                commit_view_has(b, cx, |c, cx| c.amend() && c.message(cx) == "rollback base")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            with_commit(b, cx, |c, cx| c.set_amend(false, cx))
        })),
        Step::Expect(
            "turning amend off gives back the typed (empty) message",
            Box::new(|b, cx| commit_view_has(b, cx, |c, cx| !c.amend() && c.placeholder_shown(cx))),
        ),
        // Group By Directory nests the files; a chain of folders is one node.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::create_dir_all(dir.join("cw/deep/inner"));
                let _ = std::fs::write(dir.join("cw/deep/inner/x.txt"), "x\n");
            }
            with_commit(b, cx, |c, cx| {
                c.refresh();
                c.set_group_by(
                    idecraft_workbench::commit_model::GroupBy {
                        directory: true,
                        module: false,
                    },
                    cx,
                );
            });
        })),
        Step::Expect(
            "Group By Directory shows the folder chain as one node above the file",
            Box::new(|b, cx| {
                commit_view_has(b, cx, |c, _| {
                    let rows = c.rows();
                    rows.iter().enumerate().any(|(i, r)| {
                        r.kind == idecraft_workbench::commit_model::RowKind::Directory
                            && r.label == "cw/deep/inner"
                            && rows
                                .get(i + 1)
                                .is_some_and(|f| f.label == "x.txt" && f.depth == r.depth + 1)
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join("cw"));
                let _ = std::fs::write(dir.join("rollback.txt"), "changed\n");
            }
            with_commit(b, cx, |c, cx| {
                c.set_group_by(idecraft_workbench::commit_model::GroupBy::default(), cx);
                c.refresh();
                c.check_only(&["rollback.txt"], cx);
                c.press("rollback", cx);
            });
        })),
        Step::Expect(
            "Rollback asks first and changes nothing yet",
            Box::new(|b, cx| {
                commit_view_has(b, cx, |c, _| {
                    c.rollback_pending() == Some(&["rollback.txt".to_owned()][..])
                }) && FIXTURE.get().is_some_and(|dir| {
                    std::fs::read_to_string(dir.join("rollback.txt"))
                        .ok()
                        .as_deref()
                        == Some("changed\n")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            with_commit(b, cx, |c, cx| c.confirm_rollback(cx))
        })),
        Step::Expect(
            "confirming the rollback restores the file from the last commit",
            Box::new(|b, cx| {
                commit_view_has(b, cx, |c, _| {
                    c.rollback_pending().is_none()
                        && c.note() == Some("Rolled back 1 file(s)")
                        && !c.rows().iter().any(|r| r.key == "rollback.txt")
                }) && FIXTURE.get().is_some_and(|dir| {
                    std::fs::read_to_string(dir.join("rollback.txt"))
                        .ok()
                        .as_deref()
                        == Some("base\n")
                })
            }),
        ),
        // Commit and Push… commits, then opens the Push dialog with the new commit.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("pushed2.txt"), "commit and push\n");
            }
            with_commit(b, cx, |c, cx| {
                c.refresh();
                c.check_only(&["pushed2.txt"], cx);
                c.set_message("commit and push", cx);
                c.commit_and_push(cx);
            });
        })),
        Step::WaitFor(
            "Commit and Push commits and opens the Push dialog with the new commit",
            Box::new(|b, cx| {
                b.push_dialog().is_some_and(|v| {
                    v.read(cx).outgoing().is_some_and(|o| {
                        o.commits
                            .first()
                            .is_some_and(|c| c.summary == "commit and push")
                    })
                }) && FIXTURE.get().is_some_and(|dir| {
                    git(dir, &["log", "-1", "--format=%s"]).is_ok_and(|s| s == "commit and push")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the Push dialog without pushing",
            Box::new(|b, _| b.push_dialog().is_none()),
        ),
    ]
}

/// Stash, cherry-pick (SPEC-0106): the stash popup stashes the local changes and pops them back;
/// a commit of another branch is cherry-picked onto the current one.
fn git_ops_steps() -> Vec<Step> {
    let run = |args: &[&str]| {
        if let Some(dir) = FIXTURE.get() {
            fixture_git(dir, args);
        }
    };
    let text = |file: &str| {
        FIXTURE
            .get()
            .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
    };
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("stash-me.txt"), "local change\n");
            }
            run(&["add", "stash-me.txt"]);
            b.open_stashes(window, cx);
        })),
        Step::Expect(
            "the stash popup opens with its two commands",
            Box::new(|b, cx| {
                b.stash_popup().is_some_and(|v| {
                    v.read(cx).rows()[..2]
                        == ["Stash Changes", "Stash Changes with Untracked Files"]
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "stashing removes the local change",
            Box::new(move |b, _| {
                b.status_note() == Some("Stash done") && text("stash-me.txt").is_none()
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_stashes(window, cx);
            if let Some(v) = b.stash_popup() {
                v.update(cx, |v, cx| {
                    v.select(2, cx);
                    v.choose(true, cx);
                });
            }
        })),
        Step::WaitFor(
            "popping the stash brings the change back",
            Box::new(move |b, _| {
                b.status_note() == Some("Pop stash done")
                    && text("stash-me.txt").as_deref() == Some("local change\n")
            }),
        ),
        // SPEC-0106 v3 AK-10: Git.Stash asks for a message; ⌥K keeps the staged file.
        Step::Act(Box::new(move |b, window, cx| {
            run(&["add", "stash-me.txt"]);
            b.open_stash_dialog(window, cx);
            if let Some(v) = b.stash_dialog() {
                v.update(cx, |v, cx| v.set_message("keep the index", cx));
            }
        })),
        Step::Key("alt-k", None),
        Step::Key("enter", None),
        Step::WaitFor(
            "the stash dialog stashes with the message and keeps the index",
            Box::new(move |b, _| {
                b.status_note() == Some("Stash done")
                    && b.stash_dialog().is_none()
                    && text("stash-me.txt").as_deref() == Some("local change\n")
                    && FIXTURE.get().is_some_and(|dir| {
                        std::process::Command::new("git")
                            .args(["stash", "list", "--format=%gs"])
                            .current_dir(dir)
                            .output()
                            .is_ok_and(|o| {
                                String::from_utf8_lossy(&o.stdout)
                                    .lines()
                                    .next()
                                    .is_some_and(|l| l.ends_with(": keep the index"))
                            })
                    })
            }),
        ),
        Step::Act(Box::new(move |_, _, _| run(&["stash", "drop"]))),
        Step::Act(Box::new(move |b, _, cx| {
            run(&[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "keep the fixture",
            ]);
            run(&["switch", "-q", "-c", "side"]);
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("side.txt"), "from side\n");
            }
            run(&["add", "side.txt"]);
            run(&["commit", "-q", "-m", "side change"]);
            run(&["switch", "-q", "-"]);
            b.git_operation("Cherry-pick side".into(), |r| r.cherry_pick("side"), cx);
        })),
        Step::WaitFor(
            "cherry-picking a commit of another branch applies it",
            Box::new(move |b, _| {
                b.status_note() == Some("Cherry-pick side done")
                    && text("side.txt").as_deref() == Some("from side\n")
            }),
        ),
        // SPEC-0145: a stash's changes, then an interactive rebase that folds two commits.
        Step::Act(Box::new(|b, window, cx| {
            b.open_stashes(window, cx);
            if let Some(v) = b.stash_popup() {
                v.update(cx, |v, cx| v.select(2, cx));
            }
        })),
        Step::Key("cmd-d", None),
        Step::Expect(
            "⌘D in the stash popup shows the stash's changes file by file",
            Box::new(|b, cx| {
                b.stash_popup().is_none()
                    && b.diff_view().is_some_and(|v| {
                        v.read(cx).title().starts_with("stash@{0}: On ")
                            && v.read(cx).differences() > 0
                    })
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(move |b, window, cx| {
            for (file, msg) in [("one.txt", "rebase one"), ("two.txt", "rebase two")] {
                if let Some(dir) = FIXTURE.get() {
                    let _ = std::fs::write(dir.join(file), "x\n");
                }
                run(&["add", file]);
                run(&["commit", "-q", "-m", msg]);
            }
            b.open_rebase("HEAD~2", window, cx);
        })),
        Step::Expect(
            "the interactive rebase dialog lists the two commits oldest first",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    let s: Vec<String> = v
                        .read(cx)
                        .entries()
                        .iter()
                        .map(|e| e.subject.clone())
                        .collect();
                    s == ["rebase one", "rebase two"]
                })
            }),
        ),
        // SPEC-0145 v3 behaviour 7: the selected commit's files on the right; Tab and Enter show
        // the diff of its file, Escape closes the diff and returns to the list.
        Step::WaitFor(
            "the rebase dialog shows the files of the selected commit",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    v.read(cx).details().is_some_and(|d| {
                        d.info.summary == "rebase one"
                            && d.changes.iter().any(|c| c.path == "one.txt")
                    })
                })
            }),
        ),
        Step::Key("tab", None),
        Step::Key("enter", None),
        Step::Expect(
            "Tab and Enter show the diff of the commit's file in the dialog",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    v.read(cx)
                        .diff()
                        .is_some_and(|(p, rows)| p == "one.txt" && !rows.is_empty())
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the diff, a second Escape returns to the list",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.diff().is_none() && !v.in_files()
                })
            }),
        ),
        // Hand test 71: the title names the base commit (HEAD~2), Enter and Tab in the reword
        // field end the message instead of going into it (D19), and after a click on a row the
        // keys reach the list again (D33).
        Step::Expect(
            "the interactive rebase dialog names the base commit",
            Box::new(|b, cx| {
                let base = FIXTURE
                    .get()
                    .and_then(|dir| {
                        std::process::Command::new("git")
                            .args(["rev-parse", "--short=7", "HEAD~2"])
                            .current_dir(dir)
                            .output()
                            .ok()
                    })
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                    .unwrap_or_default();
                b.rebase_dialog().is_some_and(|v| {
                    v.read(cx).title() == format!("Interactive Rebase onto {base}")
                })
            }),
        ),
        Step::Key("r", Some("r")),
        Step::Key("enter", None),
        Step::Expect(
            "Enter in the reword field keeps the message without a line break",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.editing()
                        && v.entries()[0].message.as_deref() == Some("rebase one")
                        && !v.message_text(cx).contains('\n')
                })
            }),
        ),
        Step::Key("r", Some("r")),
        Step::Key("tab", None),
        Step::Expect(
            "Tab in the reword field ends the message instead of inserting a tab",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.editing() && !v.message_text(cx).contains('\t')
                })
            }),
        ),
        Step::Key("r", Some("r")),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.rebase_dialog() {
                v.update(cx, |v, cx| v.click_row(1, window, cx));
            }
        })),
        Step::Key("d", Some("d")),
        Step::Expect(
            "after a click on a row the keys reach the list again",
            Box::new(|b, cx| {
                b.rebase_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.editing()
                        && v.selected() == 1
                        && v.entries()[1].action == idecraft_vcs::TodoAction::Drop
                })
            }),
        ),
        Step::Key("p", Some("p")),
        Step::Key("up", None),
        Step::Key("p", Some("p")),
        Step::Key("down", None),
        Step::Key("f", Some("f")),
        Step::Key("enter", None),
        Step::WaitFor(
            "fixup folds the second commit into the first",
            Box::new(move |b, _| {
                b.status_note() == Some("Interactive rebase done")
                    && text("two.txt").is_some()
                    && FIXTURE.get().is_some_and(|dir| {
                        std::process::Command::new("git")
                            .args(["log", "-2", "--format=%s"])
                            .current_dir(dir)
                            .output()
                            .is_ok_and(|o| {
                                String::from_utf8_lossy(&o.stdout)
                                    .starts_with("rebase one\nside change")
                            })
                    })
            }),
        ),
        // Leave the fixture as it was: the stashed changes back, the stash file gone.
        Step::Act(Box::new(move |_, _, _| {
            run(&["stash", "pop"]);
            run(&["rm", "-q", "-f", "stash-me.txt"]);
        })),
    ]
}

/// Runs git in the fixture repository for a self-test step. A command that fails is tried again a
/// few times (the workbench reads the repository in the background meanwhile); one that keeps
/// failing is reported, so the check after it does not fail without a reason.
fn fixture_git(dir: &Path, args: &[&str]) {
    for attempt in 0..5 {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Self Test",
                "-c",
                "user.email=self@test.invalid",
            ])
            .args(args)
            .current_dir(dir)
            .output();
        match out {
            Ok(o) if o.status.success() => return,
            // A merge that stops on conflicts is what the step wants; one that never started
            // (exit 1 too, e.g. with staged changes) is not.
            Ok(o)
                if args.first() == Some(&"merge")
                    && o.status.code() == Some(1)
                    && dir.join(".git/MERGE_HEAD").is_file() =>
            {
                return;
            }
            Ok(o) if attempt == 4 => eprintln!(
                "self-test: git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) if attempt == 4 => eprintln!("self-test: git {}: {e}", args.join(" ")),
            _ => std::thread::sleep(std::time::Duration::from_millis(300)),
        }
    }
}

/// Java inspections (SPEC-0125): findings appear in the editor with their message, ⌥↩ at a finding
/// offers IDECraft's own fix first and Enter applies it.
/// The Java file of the editor insight steps (OBS-0011 finding 3).
const INSIGHT_JAVA: &str = "class Shapes {\n    static int count = 0;\n    int size;\n    static int add(int left, int right) {\n        return left + right;\n    }\n    void run() {\n        size = add(1, count);\n    }\n    record Pair(int a, int b) {}\n    Pair make() { return new Pair(1, 2); }\n}\n";

/// Semantic tokens, parameter hints, one-line folds and usage counts in a Java file with jdtls
/// (SPEC-0290, SPEC-0291, SPEC-0083 v2, SPEC-0292). Skipped without jdtls.
fn java_insight_steps() -> Vec<Step> {
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err() {
        eprintln!("java insight steps skipped: jdtls is missing");
        return Vec::new();
    }
    /// Char offset of the `n`-th occurrence of `needle` in the insight file.
    fn at(needle: &str, n: usize) -> usize {
        let byte = INSIGHT_JAVA
            .match_indices(needle)
            .nth(n)
            .map_or(0, |(i, _)| i);
        INSIGHT_JAVA[..byte].chars().count()
    }
    let key_at = |b: &WorkbenchView, cx: &App, offset: usize| -> Option<&'static str> {
        let e = b.active_editor_entity()?;
        let e = e.read(cx);
        e.semantic_spans()
            .iter()
            .find(|(r, _)| r.contains(&offset))
            .map(|(_, k)| *k)
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("Shapes.java"), INSIGHT_JAVA);
                b.open(&dir.join("Shapes.java"), window, cx);
            }
        })),
        // SPEC-0290 AK-4: the declared method, the static field and the static call get their keys,
        // and the static ones are italic.
        Step::WaitLong(
            "jdtls's semantic tokens colour the method declaration, static field and static call",
            std::time::Duration::from_secs(180),
            Box::new(move |b, cx| {
                key_at(b, cx, at("add(", 0) + 1) == Some("METHOD_DECLARATION_ATTRIBUTES")
                    && key_at(b, cx, at("count", 0) + 1) == Some("STATIC_FIELD_ATTRIBUTES")
                    && key_at(b, cx, at("add(", 1) + 1) == Some("STATIC_METHOD_ATTRIBUTES")
                    && key_at(b, cx, at("size", 1) + 1) == Some("INSTANCE_FIELD_ATTRIBUTES")
            }),
        ),
        // SPEC-0291 AK-4: `left:` before the literal, nothing before `count`, the text unchanged.
        Step::WaitLong(
            "jdtls's parameter name hint stands before the literal argument only",
            std::time::Duration::from_secs(120),
            Box::new(move |b, cx| {
                let Some(e) = b.active_editor_entity() else {
                    return false;
                };
                let hints = e.read(cx).parameter_hints();
                hints.contains(&(at("add(1", 0) + 4, "left:".to_owned()))
                    && !hints.iter().any(|(o, _)| *o == at("count);", 0))
                    && b.active_text(cx).as_deref() == Some(INSIGHT_JAVA)
            }),
        ),
        // SPEC-0083 v2 AK-3: both one-line methods are folded and show their statement.
        Step::Expect(
            "the one-line methods are folded on open with their statement as placeholder",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    e.read(cx).placeholder_folds()
                        == [
                            (3, 5, " return left + right; }".to_owned()),
                            (6, 8, " size = add(1, count); }".to_owned()),
                        ]
                })
            }),
        ),
        // SPEC-0083 v3 AK-7: the folded statements keep their colours (more than the plain text
        // and the brace colour) and both braces get the folded-text background.
        Step::WaitFor(
            "folded one-line methods keep the code's colours, only the braces are on grey",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let folds = e.read(cx).painted_one_line_folds();
                    folds.len() == 2
                        && folds
                            .iter()
                            .all(|&(_, colours, braces)| colours >= 3 && braces == 2)
                })
            }),
        ),
        // SPEC-0292 v2 AK-3: the nested record used twice has its count at the end of its line;
        // no row is added. Members have counts of their own since SPEC-0292 v5.
        Step::WaitLong(
            "the used record shows its usage count at the end of its line",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    let lines = e.state.buffer.len_lines();
                    let record = (9, "2 usages".to_owned());
                    e.usage_rows().contains(&record)
                        && e.usage_counts_at_line_end()
                        && e.painted_line_end_texts().contains(&record)
                        && e.visible_rows() == lines - 4
                })
            }),
        ),
        Step::Expect(
            "static members are painted in italics",
            Box::new(|_, _| {
                let p = idecraft_workbench::palette::Palette::current();
                p.key_style("STATIC_FIELD_ATTRIBUTES").italic
                    && p.key_style("STATIC_METHOD_ATTRIBUTES").italic
            }),
        ),
    ]
}

/// Java class structure with jdtls (SPEC-0082 v5 AK-10, SPEC-0122 v3 AK-9, OBS-0027): ⌃H shows
/// the class hierarchy, ⌘U jumps to the overridden method, ⇧⌘H marks which classes define it, ⌃I
/// lists the method to implement and adds it, ⌥⌘T surrounds a statement with `if` (one undo
/// step). Skipped without jdtls.
fn java_structure_steps() -> Vec<Step> {
    if idecraft_languages::servers::resolve(idecraft_languages::LanguageId::Java).is_err() {
        eprintln!("java structure steps skipped: jdtls is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (type_h, method_h, super_m, implement, surround, undo, close) = if mac {
        (
            "ctrl-h",
            "cmd-shift-h",
            "cmd-u",
            "ctrl-i",
            "cmd-alt-t",
            "cmd-z",
            "cmd-w",
        )
    } else {
        (
            "ctrl-h",
            "ctrl-shift-h",
            "ctrl-u",
            "ctrl-i",
            "ctrl-alt-t",
            "ctrl-z",
            "ctrl-f4",
        )
    };
    const ANIMAL: &str = "abstract class Animal {\n    abstract String sound();\n\n    String describe() {\n        return sound();\n    }\n}\n";
    const DOG: &str = "class Dog extends Animal {\n    String sound() {\n        String s = \"woof\";\n        return s;\n    }\n}\n";
    const CAT: &str = "class Cat extends Animal {\n\n}\n";
    fn file(name: &str) -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join(format!("{name}.java")))
    }
    let open_at = |name: &'static str, line: usize, column: usize| {
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(f) = file(name) {
                b.open(&f, window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.go_to(line, column, cx));
                }
            }
        }))
    };
    let popup_items = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.jvm_popup()
            .map(|p| p.read(cx).items().to_vec())
            .unwrap_or_default()
    };
    vec![
        Step::Act(Box::new(|_, _, _| {
            for (name, text) in [("Animal", ANIMAL), ("Dog", DOG), ("Cat", CAT)] {
                if let Some(f) = file(name) {
                    let _ = std::fs::write(f, text);
                }
            }
        })),
        // Files written to disk while jdtls runs are unknown to it until they are opened (OBS-0027
        // L35), so the subtypes are opened first, as after creating them in the IDE.
        open_at("Cat", 0, 0),
        open_at("Dog", 0, 0),
        open_at("Animal", 0, 16),
        Step::Key(type_h, None),
        Step::WaitLong(
            "⌃H shows Object › * Animal › Cat, Dog in the Hierarchy window",
            std::time::Duration::from_secs(240),
            Box::new(|b, cx| {
                b.hierarchy().is_some_and(|v| {
                    let v = v.read(cx);
                    let rows = v.marked();
                    v.mode()
                        == idecraft_workbench::HierarchyMode::Types(
                            idecraft_languages::java_structure::TypeDirection::Class,
                        )
                        && rows.first().map(String::as_str) == Some("Object")
                        && rows.iter().any(|r| r == "* Animal")
                        && rows.iter().any(|r| r == "Dog")
                        && rows.iter().any(|r| r == "Cat")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the type hierarchy",
            Box::new(|b, _| b.hierarchy().is_none()),
        ),
        open_at("Dog", 1, 12),
        Step::Key(super_m, None),
        Step::WaitLong(
            "⌘U in Dog.sound() goes to Animal.sound()",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.active_file().is_some_and(|f| f.ends_with("Animal.java"))
                    && b.caret_position(cx).is_some_and(|(line, _)| line == 2)
            }),
        ),
        open_at("Animal", 1, 22),
        Step::Key(method_h, None),
        Step::WaitLong(
            "⇧⌘H marks Animal and Dog as defining sound() and Cat as having to",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.hierarchy().is_some_and(|v| {
                    let v = v.read(cx);
                    let rows = v.marked();
                    v.mode() == idecraft_workbench::HierarchyMode::Method
                        && rows.first().map(String::as_str) == Some("+ Animal")
                        && rows.iter().any(|r| r == "+ Dog")
                        && rows.iter().any(|r| r == "! Cat")
                })
            }),
        ),
        Step::Key("escape", None),
        open_at("Cat", 1, 0),
        Step::Key(implement, None),
        Step::WaitLong(
            "⌃I lists sound() of Animal, checked",
            std::time::Duration::from_secs(120),
            Box::new(move |b, cx| {
                // The row carries the return type read from Animal's source (SPEC-0122 v7).
                popup_items(b, cx) == ["Animal", "sound(): String"]
                    && b.jvm_popup().is_some_and(|p| p.read(cx).checked() == [1])
            }),
        ),
        Step::Key("enter", None),
        Step::WaitLong(
            "Enter adds sound() with @Override",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t.contains("@Override") && t.contains("String sound()"))
            }),
        ),
        Step::Key(undo, None),
        Step::Expect(
            "one undo removes the added method",
            Box::new(|b, cx| b.active_text(cx).as_deref() == Some(CAT)),
        ),
        open_at("Dog", 2, 8),
        Step::Key(surround, None),
        Step::Expect(
            "⌥⌘T offers the statement wrappers",
            Box::new(move |b, cx| {
                let items = popup_items(b, cx);
                items.first().map(String::as_str) == Some("if")
                    && items.iter().any(|i| i == "try / catch")
                    && items.len() == 11
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "if wraps the line and keeps the used declaration outside",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.contains("        String s;\n        if () {\n            s = \"woof\";\n        }\n        return s;")
                }) && b.caret_position(cx) == Some((4, 13))
            }),
        ),
        Step::Key(undo, None),
        Step::Expect(
            "one undo restores the statement",
            Box::new(|b, cx| b.active_text(cx).as_deref() == Some(DOG)),
        ),
        Step::Key(close, None),
        Step::Act(Box::new(|b, window, cx| {
            for name in ["Animal", "Cat"] {
                if let Some(f) = file(name)
                    && b.active_file().is_some_and(|a| a == f)
                {
                    b.focus_editor(window, cx);
                }
            }
        })),
    ]
}

fn java_inspection_steps() -> Vec<Step> {
    let (save, close) = if cfg!(target_os = "macos") {
        ("cmd-s", "cmd-w")
    } else {
        ("ctrl-s", "ctrl-f4")
    };
    const SRC: &str = "import java.util.List;\n\nclass Check {\n    void f(boolean ready) {\n        int unused = 1;\n        if (ready == true) {}\n    }\n    void g(String a, String b) {\n        a.trim();\n        if (a == b) {}\n    }\n}\n";
    fn file() -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join("Check.java"))
    }
    let messages = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.active_editor_entity()
            .map(|e| {
                e.read(cx)
                    .problems()
                    .iter()
                    .map(|p| p.message.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(f) = file() {
                let _ = std::fs::write(&f, SRC);
                b.open(&f, window, cx);
            }
        })),
        Step::WaitFor(
            "IDECraft's Java findings appear in the editor",
            Box::new(move |b, cx| {
                let m = messages(b, cx);
                [
                    "IDECraft: Unused import 'java.util.List'",
                    "IDECraft: Variable 'unused' is never used",
                    "IDECraft: 'ready == true' can be simplified to 'ready'",
                    "IDECraft: Result of 'String.trim()' is ignored: String values do not change",
                    "IDECraft: Strings compared with '==' compare references, not values",
                ]
                .iter()
                .all(|want| m.iter().any(|x| x == want))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    let at = text[..text.find("ready == true").unwrap_or(0) + 3]
                        .chars()
                        .count();
                    e.select_range(at..at, cx);
                });
            }
        })),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ on the finding offers IDECraft's fix first",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str) == Some("Simplify to 'ready'")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter applies it as one edit",
            Box::new(|b, cx| {
                b.intentions_popup().is_none()
                    && b.active_text(cx)
                        .is_some_and(|t| t.contains("        if (ready) {}\n"))
            }),
        ),
        // SPEC-0139: a type-aware finding and its fix.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    let at = text[..text.find("a.trim();").unwrap_or(0) + 2]
                        .chars()
                        .count();
                    e.select_range(at..at, cx);
                });
            }
        })),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ on an ignored String result offers to assign it",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str)
                        == Some("Assign the result to 'a'")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter assigns the result",
            Box::new(|b, cx| {
                b.intentions_popup().is_none()
                    && b.active_text(cx)
                        .is_some_and(|t| t.contains("        a = a.trim();\n"))
            }),
        ),
        // SPEC-0070 v5 AK-5: stripe marks and the weak warnings counted apart.
        Step::WaitFor(
            "the error stripe has marks and the unused import counts as a weak warning",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    !e.stripe_marks().is_empty()
                        && e.status().problems.2 >= 1
                        && e.problems().iter().any(|p| p.unused)
                })
            }),
        ),
        // SPEC-0078 v2 AK-4: ⌘F1 shows the fix link, a second ⌘F1 the description; ⌥⇧↩ fixes.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    let at = text[..text.find("a == b").unwrap_or(0) + 2].chars().count();
                    e.select_range(at..at, cx);
                });
            }
        })),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ShowErrorDescription", window, cx)
        })),
        Step::WaitFor(
            "⌘F1 shows the message with IDECraft's fix",
            Box::new(|b, _| b.problem_popup_fix().is_some()),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ShowErrorDescription", window, cx)
        })),
        Step::WaitFor(
            "a second ⌘F1 adds the inspection's description",
            Box::new(|b, _| {
                b.doc_blocks().is_some_and(|blocks| {
                    blocks
                        .iter()
                        .any(|x| matches!(x, idecraft_workbench::doc_model::DocBlock::Heading(_)))
                })
            }),
        ),
        Step::Key("escape", None),
        // SPEC-0125 v2 AK-7: → opens the submenu, suppressing inserts the comment, ⌘Z undoes it.
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ lists the fix with its options",
            Box::new(|b, _| b.intentions_popup().is_some()),
        ),
        Step::Key("right", None),
        Step::WaitFor(
            "→ opens the finding's submenu",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx)
                        .submenu_labels()
                        .is_some_and(|l| l.iter().any(|x| x == "Suppress for statement"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.intentions_popup() {
                v.update(cx, |v, cx| v.choose_option("Suppress for statement", cx));
            }
        })),
        Step::WaitFor(
            "the statement is suppressed and the finding is gone",
            Box::new(move |b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t.contains("        //noinspection StringEquality\n"))
                    && !messages(b, cx)
                        .iter()
                        .any(|m| m.contains("Strings compared with '=='"))
            }),
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-z"
            } else {
                "ctrl-z"
            },
            None,
        ),
        Step::WaitFor(
            "⌘Z takes the suppression back",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| !t.contains("//noinspection"))
            }),
        ),
        // SPEC-0070 v6 AK-6: the highlighting level of the file (written, not yet run).
        Step::Act(Box::new(|b, _, cx| {
            b.set_file_highlight_level(
                idecraft_workbench::diagnostics_model::HighlightLevel::Syntax,
                cx,
            )
        })),
        Step::WaitFor(
            "at Syntax only errors stay and the warnings are counted as hidden",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    e.hidden_by_level() > 0
                        && e.problems().iter().all(|p| {
                            p.severity == idecraft_workbench::diagnostics_model::Severity::Error
                        })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            b.set_file_highlight_level(
                idecraft_workbench::diagnostics_model::HighlightLevel::All,
                cx,
            )
        })),
        Step::WaitFor(
            "at All the warnings are back",
            Box::new(move |b, cx| {
                messages(b, cx)
                    .iter()
                    .any(|m| m.contains("Strings compared with '=='"))
            }),
        ),
        // SPEC-0125 v3 AK-8: "Disable highlighting, keep fix" hides the mark, ⌥↩ keeps the fix.
        // ⌘Z put the caret where the comment was (SPEC-0057), so back onto `a == b` first.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    let at = text[..text.find("a == b").unwrap_or(0) + 2].chars().count();
                    e.select_range(at..at, cx);
                });
            }
        })),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ lists the fix with its options",
            Box::new(|b, _| b.intentions_popup().is_some()),
        ),
        Step::Key("right", None),
        Step::WaitFor(
            "the submenu has the three new entries",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).submenu_labels().is_some_and(|l| {
                        [
                            "Run inspection on…",
                            "Suppress all inspections for class",
                            "Disable highlighting, keep fix",
                        ]
                        .iter()
                        .all(|want| l.iter().any(|x| x == want))
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.intentions_popup() {
                v.update(cx, |v, cx| {
                    v.choose_option("Disable highlighting, keep fix", cx)
                });
            }
        })),
        Step::WaitFor(
            "the string comparison is no longer marked",
            Box::new(move |b, cx| {
                !messages(b, cx)
                    .iter()
                    .any(|m| m.contains("Strings compared with '=='"))
            }),
        ),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ still offers its fix",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx)
                        .titles()
                        .first()
                        .is_some_and(|t| t.starts_with("Replace with"))
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, _, cx| {
            b.reset_inspection("java.string-equality", cx)
        })),
        // "Run inspection on…": the scope dialog, then only that inspection.
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ lists the fix again",
            Box::new(|b, _| b.intentions_popup().is_some()),
        ),
        Step::Key("right", None),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.intentions_popup() {
                v.update(cx, |v, cx| v.choose_option("Run inspection on…", cx));
            }
        })),
        Step::WaitFor(
            "the scope dialog opens",
            Box::new(|b, _| b.inspect_dialog().is_some()),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.confirm_inspect_dialog(window, cx)
        })),
        Step::WaitFor(
            "only the string comparison is in the results",
            Box::new(|b, cx| {
                !b.inspecting()
                    && b.problems_rows(cx).is_some_and(|rows| {
                        rows.iter()
                            .any(|r| r.starts_with("String comparison with '=='"))
                            && !rows.iter().any(|r| r.starts_with("Unused"))
                    })
            }),
        ),
        // SPEC-0316 AK-5: Inspect Code over the project lists the unused declarations.
        Step::Act(Box::new(|b, window, cx| {
            b.inspect_code(window, cx);
            b.confirm_inspect_dialog(window, cx);
        })),
        Step::WaitFor(
            "Inspect Code lists the node of unused declarations",
            Box::new(|b, cx| {
                !b.inspecting()
                    && b.problems_rows(cx).is_some_and(|rows| {
                        rows.iter().any(|r| r.starts_with("Unused declaration"))
                    })
            }),
        ),
        // SPEC-0092 v4 AK-5: the File tab's editor preview shows the selected problem's line.
        Step::Act(Box::new(|b, _, cx| {
            b.set_problems_scope(idecraft_workbench::problems_model::Scope::File, cx);
            b.set_problems_preview(true, cx);
        })),
        Step::Key("down", None),
        Step::WaitFor(
            "the preview shows the lines of the selected problem",
            Box::new(|b, cx| {
                b.problem_preview_lines(cx)
                    .is_some_and(|l| l.iter().any(|x| x.contains("class Check")))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.set_problems_preview(false, cx);
            b.menu_action("ActivateProblemsViewToolWindow", window, cx);
            b.focus_editor(window, cx);
        })),
        // SPEC-0315 AK-4: the settings page lists the inspection; turning it off removes the
        // finding and writes the profile; Reset brings it back.
        Step::Act(Box::new(|b, window, cx| {
            b.open_inspection_settings(Some("java.string-equality"), window, cx)
        })),
        Step::WaitFor(
            "the Inspections page lists the Java inspections",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|s| {
                    s.read(cx)
                        .inspection_ids()
                        .contains(&"java.string-equality")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(s) = b.settings_dialog() {
                s.update(cx, |s, cx| s.change_inspection(false, cx));
            }
        })),
        Step::WaitFor(
            "turned off, the finding leaves the editor and the profile is written",
            Box::new(move |b, cx| {
                let written = FIXTURE.get().is_some_and(|d| {
                    std::fs::read_to_string(d.join(".idea/inspectionProfiles/Project_Default.xml"))
                        .is_ok_and(|x| x.contains("StringEquality") && x.contains("false"))
                });
                written
                    && !messages(b, cx)
                        .iter()
                        .any(|m| m.contains("Strings compared with '=='"))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.reset_inspection("java.string-equality", cx);
            let _ = window;
            if let Some(s) = b.settings_dialog() {
                s.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::SettingsEvent::Close)
                });
            }
        })),
        Step::WaitFor(
            "reset, the finding is back",
            Box::new(move |b, cx| {
                messages(b, cx)
                    .iter()
                    .any(|m| m.contains("Strings compared with '=='"))
            }),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(f) = file() {
                let _ = std::fs::remove_file(f);
            }
        })),
        // SPEC-0140: Kotlin with types.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(d) = FIXTURE.get() {
                let f = d.join("Ids.kt");
                let _ = std::fs::write(
                    &f,
                    "fun same(a: String, b: String): Boolean {\n    return a === b\n}\n",
                );
                b.open(&f, window, cx);
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| {
                        let at = "fun same(a: String, b: String): Boolean {\n    return a =".len();
                        e.select_range(at..at, cx);
                    });
                }
            }
        })),
        Step::WaitFor(
            "a Kotlin identity comparison of Strings is a problem",
            Box::new(move |b, cx| {
                messages(b, cx).iter().any(|m| {
                    m == "IDECraft: '===' compares the identity of String values, not their value"
                })
            }),
        ),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ offers to compare with '=='",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str) == Some("Replace with '=='")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter replaces the operator",
            Box::new(|b, cx| {
                b.intentions_popup().is_none()
                    && b.active_text(cx)
                        .is_some_and(|t| t.contains("    return a == b\n"))
            }),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::remove_file(d.join("Ids.kt"));
            }
        })),
    ]
}

/// ESLint and Prettier of the project (SPEC-0123), with the reference environment's installation
/// (`js.sh install`): findings in the editor, Fix ESLint Problems, Reformat
/// with Prettier on its shortcut. Without the installation the checks are skipped.
fn js_tool_steps() -> Vec<Step> {
    use std::path::PathBuf;
    use std::sync::OnceLock;
    static ENV: OnceLock<Option<PathBuf>> = OnceLock::new();
    fn env() -> Option<&'static PathBuf> {
        ENV.get_or_init(|| {
            std::env::var_os("IDECRAFT_TEST_JSLINT")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|h| PathBuf::from(h).join("ReferenceEnv/projects/js-lint"))
                })
                .filter(|d| d.join("node_modules/.bin/eslint").exists())
        })
        .as_ref()
    }
    fn package() -> Option<PathBuf> {
        FIXTURE.get().map(|d| d.join("web"))
    }
    let prettier = if cfg!(target_os = "macos") {
        "alt-shift-cmd-p"
    } else {
        "ctrl-alt-shift-p"
    };
    let (close, save) = if cfg!(target_os = "macos") {
        ("cmd-w", "cmd-s")
    } else {
        ("ctrl-f4", "ctrl-s")
    };
    const APP: &str = "let total = 0\nconst unused = 1;\nexport function add(a, b) {\n  if (a == b) { return a*2 }\n  return a+b;\n}\nconsole.log(add(total, 2))\n";
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let (Some(env), Some(dir)) = (env(), package()) {
                let _ = std::fs::create_dir_all(dir.join("src"));
                let _ =
                    std::os::unix::fs::symlink(env.join("node_modules"), dir.join("node_modules"));
                for f in ["package.json", "eslint.config.js", ".prettierrc"] {
                    let _ = std::fs::copy(env.join(f), dir.join(f));
                }
                let _ = std::fs::write(dir.join("src/app.js"), APP);
                b.open(&dir.join("src/app.js"), window, cx);
            }
        })),
        Step::WaitFor(
            "ESLint's findings appear in the editor: two errors, one warning",
            Box::new(|b, cx| env().is_none() || b.active_problems(cx) == Some((2, 1))),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if env().is_some() {
                b.menu_action("Javascript.Linters.EsLint.Fix", window, cx);
            }
        })),
        Step::WaitFor(
            "Fix ESLint Problems applies ESLint's fixes",
            Box::new(|b, cx| {
                env().is_none()
                    || (b
                        .active_text(cx)
                        .is_some_and(|t| t.starts_with("const total = 0\n"))
                        && b.status_message(cx).as_deref() == Some("Fixed ESLint problems")
                        && b.active_problems(cx) == Some((1, 1)))
            }),
        ),
        Step::Key(prettier, None),
        Step::WaitFor(
            "Reformat with Prettier formats with the project's configuration",
            Box::new(|b, cx| {
                env().is_none()
                    || b.active_text(cx).as_deref()
                        == Some(
                            "const total = 0;\nconst unused = 1;\nexport function add(a, b) {\n  if (a == b) {\n    return a * 2;\n  }\n  return a + b;\n}\nconsole.log(add(total, 2));\n",
                        )
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = package() {
                let _ = std::fs::remove_dir_all(dir);
            }
        })),
    ]
}

/// Menu bar (SPEC-0124): items show the keymap's shortcut, and an item's action reaches the
/// window through the toolkit's action dispatch.
fn menu_steps() -> Vec<Step> {
    vec![
        Step::Expect(
            "menu items carry the keymap's shortcuts",
            Box::new(|_, _| {
                let save = idecraft_workbench::menu_bar::menu_shortcut("SaveAll");
                let generate = idecraft_workbench::menu_bar::menu_shortcut("Generate");
                if cfg!(target_os = "macos") {
                    save.as_deref() == Some("cmd-s") && generate.as_deref() == Some("cmd-n")
                } else {
                    save.as_deref() == Some("ctrl-s")
                }
            }),
        ),
        Step::Expect(
            "the menu bar has Build before Run, Git after Tools and Window before Help",
            Box::new(|_, cx| {
                // SPEC-0124 v3 AK-5 (hand test 50); a platform without a menu bar reports none.
                cx.get_menus().is_none_or(|menus| {
                    let names: Vec<String> = menus.iter().map(|m| m.name.to_string()).collect();
                    names
                        == [
                            "IDECraft", "File", "Edit", "View", "Navigate", "Code", "Refactor",
                            "Build", "Run", "Tools", "Git", "Window", "Help",
                        ]
                })
            }),
        ),
        Step::Act(Box::new(|_, window, cx| {
            // Dispatched after this update, as a click in the menu is.
            window.dispatch_action(
                Box::new(idecraft_workbench::menu_bar::IdeAction {
                    id: "SearchEverywhere".into(),
                }),
                cx,
            );
        })),
        Step::WaitFor(
            "Navigate → Search Everywhere from the menu opens it",
            Box::new(|b, _| b.everywhere().is_some()),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes it again",
            Box::new(|b, _| b.everywhere().is_none()),
        ),
    ]
}

/// Resolving merge conflicts (SPEC-0101): a merge that stops with a conflict opens in the merge
/// view; taking the right side and Apply writes the file and clears the conflict. Runs last, as it
/// leaves the fixture repository in a merge.
fn merge_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let git = |args: &[&str]| fixture_git(dir, args);
                let _ = std::fs::write(dir.join("conflict.txt"), "one\ntwo\nthree\n");
                git(&["add", "conflict.txt"]);
                git(&["commit", "-q", "-m", "base", "--", "conflict.txt"]);
                git(&["branch", "-q", "conflict-topic"]);
                let _ = std::fs::write(dir.join("conflict.txt"), "one\ntwo (main)\nthree\n");
                git(&["commit", "-q", "-m", "main side", "--", "conflict.txt"]);
                git(&["checkout", "-q", "conflict-topic"]);
                let _ = std::fs::write(dir.join("conflict.txt"), "one\ntwo (topic)\nthree\n");
                git(&["commit", "-q", "-m", "topic side", "--", "conflict.txt"]);
                git(&["checkout", "-q", "-"]);
                git(&["merge", "-q", "--no-edit", "conflict-topic"]);
            }
            b.resolve_conflicts(window, cx);
        })),
        Step::Expect(
            "a stopped merge opens with its conflicted file and the conflict open",
            Box::new(|b, cx| {
                b.merge_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.file_names() == ["conflict.txt"] && v.open_conflicts() == 1
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.merge_view() {
                v.update(cx, |v, cx| {
                    v.resolve(0, idecraft_vcs::Resolution::Theirs, cx);
                    v.apply(cx);
                });
            }
        })),
        Step::WaitFor(
            "apply writes the right side and clears the conflict",
            Box::new(|b, _| {
                let dir = FIXTURE.get();
                b.merge_view().is_none()
                    && dir.is_some_and(|d| {
                        std::fs::read_to_string(d.join("conflict.txt"))
                            .is_ok_and(|t| t == "one\ntwo (topic)\nthree\n")
                            && idecraft_vcs::Repo::discover(d)
                                .and_then(|r| r.conflicts())
                                .is_ok_and(|c| c.is_empty())
                    })
            }),
        ),
    ]
}

/// The editable result column of the merge view (SPEC-0101 v7 AK-20): typed keys change the result
/// and resolve the conflict by hand, ⌘Z opens it again, F7 / ⇧F7 step through the changes, `»`
/// takes the left side and typing edits it, Escape asks before the work is dropped, and Apply
/// writes exactly the edited text; until then the file on disk keeps Git's markers.
fn merge_edit_steps() -> Vec<Step> {
    const FILE: &str = "medit.txt";
    const START: &str = "ONE\ntwo\nthree\nfour\nfive\n";
    const EDITED: &str = "ONE\ntwo\nthree (main)!\nfour\nfive\n";
    let on_disk = || {
        FIXTURE
            .get()
            .and_then(|d| std::fs::read_to_string(d.join(FILE)).ok())
    };
    let waiting = || {
        FIXTURE
            .get()
            .and_then(|d| idecraft_vcs::Repo::discover(d).ok())
            .and_then(|r| r.operation_in_progress())
    };
    let undo = if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    };
    // The shown file's result text, open conflicts and the region the caret is in.
    fn shown(b: &WorkbenchView, cx: &App) -> Option<(String, usize, usize)> {
        let view = b.merge_view()?;
        let view = view.read(cx);
        let session = view.session()?;
        Some((
            session.text(),
            session.open_conflicts(),
            session.caret_region(),
        ))
    }
    // The gutter controls and Apply are clicked by their test ids, which only the UI driver
    // resolves (SPEC-0319 behaviour 44); without it the run ends by keys and aborts the merge.
    let driven = std::env::var("IDECRAFT_SELFTEST_DRIVER")
        .is_ok_and(|v| !matches!(v.as_str(), "" | "0" | "off"));
    let asks = || {
        Step::Expect(
            "Escape asks before it drops the choices and edits",
            Box::new(|b, cx| {
                b.merge_view().is_some_and(|v| {
                    v.read(cx).asking() == Some(idecraft_workbench::MergeAsk::Close)
                })
            }),
        )
    };
    let mut steps = vec![
        Step::WaitFor(
            "no Git operation of an earlier group is still running",
            Box::new(move |_, _| waiting().is_none()),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let git = |args: &[&str]| fixture_git(dir, args);
                git(&["add", "-A"]);
                git(&["commit", "-q", "--allow-empty", "-m", "before merge-edit"]);
                let _ = std::fs::write(dir.join(FILE), "one\ntwo\nthree\nfour\nfive\n");
                git(&["add", FILE]);
                git(&["commit", "-q", "-m", "medit base", "--", FILE]);
                git(&["branch", "-q", "medit-topic"]);
                let _ = std::fs::write(dir.join(FILE), "ONE\ntwo\nthree (main)\nfour\nfive\n");
                git(&["commit", "-q", "-m", "medit main", "--", FILE]);
                git(&["checkout", "-q", "medit-topic"]);
                let _ = std::fs::write(dir.join(FILE), "one\ntwo\nthree (topic)\nfour\nfive\n");
                git(&["commit", "-q", "-m", "medit topic", "--", FILE]);
                git(&["checkout", "-q", "-"]);
                git(&["merge", "-q", "--no-edit", "medit-topic"]);
            }
            b.resolve_conflicts(window, cx);
        })),
        Step::Expect(
            "the result column holds the change of the left side and the base text of the conflict",
            Box::new(move |b, cx| {
                shown(b, cx) == Some((START.to_owned(), 1, 0))
                    && on_disk().is_some_and(|t| t.contains("<<<<<<<"))
            }),
        ),
        Step::Key("f7", None),
        Step::Expect(
            "F7 puts the caret on the conflict",
            Box::new(|b, cx| shown(b, cx).is_some_and(|(_, _, region)| region == 2)),
        ),
        Step::Key("x", Some("x")),
        Step::Key("y", Some("y")),
        Step::Expect(
            "typing changes the result and resolves the conflict by hand; the file is untouched",
            Box::new(move |b, cx| {
                shown(b, cx).is_some_and(|(text, open, _)| {
                    text == "ONE\ntwo\nxythree\nfour\nfive\n" && open == 0
                }) && on_disk().is_some_and(|t| t.contains("<<<<<<<"))
            }),
        ),
        Step::Key(undo, None),
        Step::Expect(
            "Undo takes the typed text back and the conflict is open again",
            Box::new(|b, cx| {
                shown(b, cx).is_some_and(|(text, open, _)| text == START && open == 1)
            }),
        ),
        Step::Key("shift-f7", None),
        Step::Expect(
            "⇧F7 goes back to the change before the conflict",
            Box::new(|b, cx| shown(b, cx).is_some_and(|(_, _, region)| region == 0)),
        ),
    ];
    if !driven {
        eprintln!(
            "merge-edit: the clicks on » and Apply need IDECRAFT_SELFTEST_DRIVER=1; the keys are checked, the merge is aborted"
        );
        steps.extend([
            Step::Key("z", Some("z")),
            Step::Key("escape", None),
            asks(),
            Step::Key("escape", None),
            Step::Expect(
                "Escape again keeps the view and its text",
                Box::new(|b, cx| {
                    b.merge_view()
                        .is_some_and(|v| v.read(cx).asking().is_none())
                        && shown(b, cx).is_some_and(|(text, _, _)| text.starts_with("zONE"))
                }),
            ),
            Step::Key(undo, None),
            Step::Key("escape", None),
            Step::Expect(
                "with nothing left to lose Escape closes the view at once",
                Box::new(|b, _| b.merge_view().is_none()),
            ),
            Step::Setup(Box::new(|b, _, _| {
                if let Some(dir) = FIXTURE.get() {
                    fixture_git(dir, &["merge", "--abort"]);
                }
                b.refresh_branch();
            })),
        ]);
        return steps;
    }
    steps.extend([
        click("merge-take-left-2", Click::Left(1)),
        Step::WaitFor(
            "» in the left gutter takes the left side and resolves the conflict in one step",
            Box::new(|b, cx| {
                shown(b, cx).is_some_and(|(text, open, region)| {
                    text == "ONE\ntwo\nthree (main)\nfour\nfive\n" && open == 0 && region == 2
                })
            }),
        ),
        Step::Key("end", None),
        Step::Key("!", Some("!")),
        Step::Expect(
            "the inserted side is ordinary text and can be edited",
            Box::new(|b, cx| shown(b, cx).is_some_and(|(text, _, _)| text == EDITED)),
        ),
        Step::Key("escape", None),
        asks(),
        Step::Key("escape", None),
        Step::Expect(
            "Escape again keeps the view and its text",
            Box::new(|b, cx| {
                b.merge_view()
                    .is_some_and(|v| v.read(cx).asking().is_none())
                    && shown(b, cx).is_some_and(|(text, _, _)| text == EDITED)
            }),
        ),
        click("merge-apply", Click::Left(1)),
        Step::WaitFor(
            "Apply writes exactly the edited text and finishes the merge",
            Box::new(move |b, _| {
                b.merge_view().is_none()
                    && on_disk().as_deref() == Some(EDITED)
                    && waiting().is_none()
            }),
        ),
    ]);
    steps
}

/// Git conflicts, stopped operations, smart checkout and commit hooks (SPEC-0101 v3 AK-7,
/// SPEC-0068 v3 AK-6, SPEC-0231 AK-5, SPEC-0230 AK-6): a merge from the branch popup stops and
/// opens the merge view by itself, Resolve Simple takes the simple conflict, Apply finishes the
/// merge; a stopped rebase shows in the widget and the popup aborts it; a checkout with a local
/// change in the way asks and puts it aside; a failing pre-commit hook blocks the commit.
fn git_m63_steps() -> Vec<Step> {
    let run = |args: &[&str]| {
        if let Some(dir) = FIXTURE.get() {
            fixture_git(dir, args);
        }
    };
    let write = |file: &str, text: &str| {
        if let Some(dir) = FIXTURE.get() {
            let _ = std::fs::write(dir.join(file), text);
        }
    };
    let text = |file: &str| {
        FIXTURE
            .get()
            .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
    };
    let git_out = |args: &[&str]| {
        FIXTURE.get().map_or(String::new(), |dir| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default()
        })
    };
    let repo = || {
        FIXTURE
            .get()
            .and_then(|d| idecraft_vcs::Repo::discover(d).ok())
    };
    vec![
        // Merge from the branch popup with ⌘↩: one simple and one real conflict.
        Step::Act(Box::new(move |b, window, cx| {
            // Whatever earlier steps left changed is committed first, so only this step's
            // files take part.
            run(&["add", "-A"]);
            run(&["commit", "-q", "--allow-empty", "-m", "before M6.3 steps"]);
            write("m63.txt", "let total = price * count;\nkeep\nvalue = 1\n");
            run(&["add", "m63.txt"]);
            run(&["commit", "-q", "-m", "m63 base"]);
            run(&["branch", "-q", "m63-topic"]);
            write("m63.txt", "let sum = price * count;\nkeep\nvalue = 2\n");
            run(&["commit", "-q", "-am", "m63 main"]);
            run(&["checkout", "-q", "m63-topic"]);
            write("m63.txt", "let total = price * amount;\nkeep\nvalue = 3\n");
            run(&["commit", "-q", "-am", "m63 topic"]);
            run(&["checkout", "-q", "-"]);
            b.refresh_branch();
            b.open_branches(window, cx);
            if let Some(v) = b.branches_popup() {
                v.update(cx, |v, cx| v.select_branch("m63-topic", cx));
            }
        })),
        Step::Expect(
            "the branch popup names its keys at the bottom",
            Box::new(|b, _| b.branches_popup().is_some()),
        ),
        Step::Key("cmd-enter", None),
        Step::WaitFor(
            "⌘↩ merges, the merge stops and the merge view opens by itself with two conflicts",
            Box::new(|b, cx| {
                b.merge_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.file_names() == ["m63.txt"] && v.open_conflicts() == 2
                }) && b
                    .branch_widget()
                    .is_some_and(|(l, warn)| warn && l.starts_with("Merging "))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.merge_view() {
                v.update(cx, |v, cx| {
                    v.resolve_simple(cx);
                });
            }
        })),
        Step::Expect(
            "Resolve Simple takes the simple conflict and leaves the real one",
            Box::new(|b, cx| {
                b.merge_view()
                    .is_some_and(|v| v.read(cx).open_conflicts() == 1)
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.merge_view() {
                v.update(cx, |v, cx| {
                    v.accept_all(false, cx);
                    v.apply(cx);
                });
            }
        })),
        Step::WaitFor(
            "Apply of the last file finishes the merge with a merge commit",
            Box::new(move |b, _| {
                b.merge_view().is_none()
                    && b.status_note() == Some("Continue done")
                    && text("m63.txt").as_deref()
                        == Some("let sum = price * amount;\nkeep\nvalue = 3\n")
                    && git_out(&["log", "-1", "--format=%P"])
                        .split_whitespace()
                        .count()
                        == 2
                    && b.branch_widget().is_some_and(|(_, warn)| !warn)
            }),
        ),
        // A rebase that stops: the widget warns, the popup offers Continue/Abort/Skip.
        Step::Act(Box::new(move |b, _, cx| {
            run(&["checkout", "-q", "-b", "m63-rb"]);
            write("m63.txt", "let sum = price * amount;\nkeep\nvalue = 4\n");
            run(&["commit", "-q", "-am", "m63 rb"]);
            run(&["checkout", "-q", "-"]);
            write("m63.txt", "let sum = price * amount;\nkeep\nvalue = 5\n");
            run(&["commit", "-q", "-am", "m63 main 5"]);
            run(&["checkout", "-q", "m63-rb"]);
            b.git_operation("Rebase onto main".into(), |r| r.rebase("@{-1}"), cx);
        })),
        Step::WaitFor(
            "the stopped rebase shows in the widget and opens the merge view",
            Box::new(|b, _| {
                b.merge_view().is_some()
                    && b.branch_widget() == Some(("Rebasing m63-rb".into(), true))
            }),
        ),
        // The merge view stays open behind the popup; Abort has to close it (D15).
        Step::Act(Box::new(|b, window, cx| b.open_branches(window, cx))),
        Step::Expect(
            "the branch popup starts with the rebase's Continue, Abort and Skip",
            Box::new(|b, cx| {
                use idecraft_workbench::branches_model::{BranchRow, OpAction};
                b.branches_popup().is_some_and(|v| {
                    let rows = &v.read(cx).list().rows;
                    rows.first() == Some(&BranchRow::Header("Rebase in progress"))
                        && matches!(rows.get(2), Some(BranchRow::Op(OpAction::Abort, _)))
                        && matches!(rows.get(3), Some(BranchRow::Op(OpAction::Skip, _)))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.branches_popup() {
                v.update(cx, |v, cx| v.choose_row(2, cx));
            }
        })),
        Step::WaitFor(
            "Abort Rebase restores the branch and closes the merge view",
            Box::new(move |b, _| {
                b.merge_view().is_none()
                    && b.status_note() == Some("Abort done")
                    && b.branch_widget() == Some(("m63-rb".into(), false))
                    && text("m63.txt").as_deref()
                        == Some("let sum = price * amount;\nkeep\nvalue = 4\n")
            }),
        ),
        // Smart checkout: a local change in the way is put aside and comes back.
        Step::Act(Box::new(move |b, window, cx| {
            run(&["checkout", "-q", "-"]);
            write("smart.txt", "a\nb\nc\n");
            run(&["add", "smart.txt"]);
            run(&["commit", "-q", "-m", "smart base"]);
            run(&["checkout", "-q", "-b", "m63-smart"]);
            write("smart.txt", "A\nb\nc\n");
            run(&["commit", "-q", "-am", "smart A"]);
            run(&["checkout", "-q", "-"]);
            write("smart.txt", "a\nb\nC\n");
            b.refresh_branch();
            b.open_branches(window, cx);
            if let Some(v) = b.branches_popup() {
                v.update(cx, |v, cx| v.select_branch("m63-smart", cx));
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "a checkout with a local change in the way asks for a smart checkout",
            Box::new(|b, cx| {
                b.jvm_popup().is_some_and(|v| {
                    let v = v.read(cx);
                    v.title() == "Local changes would be overwritten: smart.txt"
                        && v.items()[0].starts_with("Smart Checkout")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Smart Checkout switches and keeps the local change",
            Box::new(move |b, _| {
                b.branch().as_deref() == Some("m63-smart")
                    && b.status_note()
                        .is_some_and(|s| s.starts_with("Smart checkout m63-smart done"))
                    && text("smart.txt").as_deref() == Some("A\nb\nC\n")
                    && repo().is_some_and(|r| r.pending_smart_stash().is_none())
                    && !git_out(&["stash", "list"]).contains("IDECraft smart")
            }),
        ),
        // Commit hooks: a failing pre-commit blocks, unticking Run Git hooks commits.
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let hook = dir.join(".git/hooks/pre-commit");
                let _ = std::fs::create_dir_all(dir.join(".git/hooks"));
                let _ = std::fs::write(&hook, "#!/bin/sh\necho 'hook says no'\nexit 1\n");
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755));
            }
            if b.commit_view().is_none() {
                b.menu_action("ActivateCommitToolWindow", window, cx);
            }
            if let Some(v) = b.commit_view() {
                v.update(cx, |v, cx| {
                    v.refresh();
                    v.check_only(&["smart.txt"], cx);
                    v.set_message("smart C", cx);
                    v.commit_now(cx);
                });
            }
        })),
        Step::WaitFor(
            "a failing pre-commit hook blocks the commit and shows its output",
            Box::new(|b, cx| {
                b.commit_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.note() == Some("Commit blocked by pre-commit hook (exit 1)")
                        && v.hook_output() == Some("hook says no")
                        && v.message(cx) == "smart C"
                        && v.hooks() == ["pre-commit"]
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.commit_view() {
                v.update(cx, |v, cx| {
                    v.set_run_hooks(false, cx);
                    v.commit_now(cx);
                });
            }
        })),
        Step::WaitFor(
            "with Run Git hooks off the commit goes through",
            Box::new(move |b, _| {
                b.status_note()
                    .is_some_and(|s| s.starts_with("Committed 1 file(s)"))
                    && git_out(&["log", "-1", "--format=%s"]) == "smart C"
            }),
        ),
        Step::Act(Box::new(move |b, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join(".git/hooks/pre-commit"));
            }
            run(&["checkout", "-q", "-"]);
            b.refresh_branch();
        })),
    ]
}

/// Refactoring shortcuts (SPEC-0095): with `x * 2` selected in a C++ file, Extract Variable asks
/// clangd and applies its only matching refactoring to the editor text.
fn refactor_steps() -> Vec<Step> {
    let key = if cfg!(target_os = "macos") {
        "cmd-alt-v"
    } else {
        "ctrl-alt-v"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let text = "int twice_plus(int x) {\n    int y = x * 2 + 1;\n    return y;\n}\n";
                let _ = std::fs::write(dir.join("extract.cpp"), text);
                b.open(&dir.join("extract.cpp"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    let start = text.find("x * 2").unwrap_or(0);
                    e.update(cx, |e, cx| e.select_range(start..start + 5, cx));
                }
            }
        })),
        Step::Key(key, None),
        Step::WaitFor(
            "extract variable puts the selected expression into a new variable",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.lines().filter(|l| l.contains("x * 2")).count() == 1
                        && !t.contains("int y = x * 2 + 1;")
                })
            }),
        ),
        Step::WaitFor(
            "the rename popup asks for the new variable's name (SPEC-0095 v3 behaviour 8)",
            Box::new(|b, cx| {
                b.goto_popup()
                    .is_some_and(|g| g.read(cx).field_text(cx) == "placeholder")
            }),
        ),
        Step::Key("d", Some("d")),
        Step::Key("enter", None),
        Step::WaitFor(
            "the typed name replaces the server's name in declaration and use",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.matches("d = x * 2").count() == 1
                        && t.contains("d + 1")
                        && !t.contains("placeholder")
                })
            }),
        ),
    ]
    .into_iter()
    .chain(refactor_name_steps())
    .collect()
}

/// Hand tests 14 and 210 (window run 2026-11-10): ⌘Z and Escape update the status line
/// (SPEC-0193 v4 behaviour 9, SPEC-0095 v5 behaviour 8a), Extract Method on a whole line of the
/// hand test's `shapes.cpp` opens the name popup at `extracted` and Enter renames the function
/// (behaviours 8, 8b), and ⌃T on a name lists Rename next to the extractions (behaviour 4a).
///
/// The `main` body is the hand test's, `total + missing` included: clangd extracts the line
/// declaring `total` only because the one use after it is that broken expression; with a valid
/// use (`total` alone) clangd refuses to move a declaration that is used after the selection, as
/// `crates/languages/tests/it/clangd.rs` (the same file) shows.
fn refactor_name_steps() -> Vec<Step> {
    const SHAPES: &str = "#include <cstdio>\n\nstruct Shape {\n    virtual int area() const = 1;\n    \
                          virtual ~Shape() = default;\n};\n\nstruct Square : Shape {\n    int side = 2;\n    \
                          int area() const override { return side * side; }\n};\n\nint add(int a, int b) \
                          { return a + b; }\n\nint main() {\n    Square sq;\n    Shape &s = sq;\n    int \
                          total = add(s.area(), 1);\n    std::printf(\"%d\\n\", total + missing);\n    \
                          return 0;\n}\n";
    let (undo, extract, escape_key, refactor_this) = if cfg!(target_os = "macos") {
        ("cmd-z", "cmd-alt-m", "escape", "ctrl-t")
    } else {
        ("ctrl-z", "ctrl-alt-m", "escape", "ctrl-alt-shift-t")
    };
    let variable = if cfg!(target_os = "macos") {
        "cmd-alt-v"
    } else {
        "ctrl-alt-v"
    };
    vec![
        Step::Key(undo, None),
        Step::WaitFor(
            "⌘Z after the rename names what it took back (SPEC-0193 v4 behaviour 9)",
            Box::new(|b, cx| {
                b.status_note() == Some("Undone: Rename placeholder to d")
                    && b.active_text(cx).is_some_and(|t| t.contains("placeholder"))
            }),
        ),
        Step::Key(undo, None),
        Step::WaitFor(
            "the second ⌘Z takes back the extraction and says so",
            Box::new(|b, cx| {
                b.status_note()
                    .is_some_and(|s| s.starts_with("Undone: Extract"))
                    && b.active_text(cx)
                        .is_some_and(|t| t.contains("int y = x * 2 + 1;"))
            }),
        ),
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                let text = e.read(cx).state.buffer.text();
                let start = text.find("x * 2").unwrap_or(0);
                e.update(cx, |e, cx| e.select_range(start..start + 5, cx));
            }
        })),
        Step::Key(variable, None),
        Step::WaitFor(
            "Extract Variable asks for the name again",
            Box::new(|b, cx| {
                b.goto_popup()
                    .is_some_and(|g| g.read(cx).field_text(cx) == "placeholder")
                    && b.goto_at_name()
            }),
        ),
        Step::Key(escape_key, None),
        Step::WaitFor(
            "Escape keeps the server's name and the status line says so (SPEC-0095 v5 behaviour 8a)",
            Box::new(|b, cx| {
                b.goto_popup().is_none()
                    && b.status_note() == Some("Kept the name placeholder.")
                    && b.active_text(cx).is_some_and(|t| t.contains("placeholder"))
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("shapes_m.cpp"), SHAPES);
                b.open(&dir.join("shapes_m.cpp"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    let line = "    int total = add(s.area(), 1);\n";
                    let start = SHAPES.find(line).unwrap_or(0);
                    e.update(cx, |e, cx| e.select_range(start..start + line.len(), cx));
                }
            }
        })),
        Step::Key(extract, None),
        Step::WaitFor(
            "Extract Method on a whole line opens the name popup at `extracted` (hand test 210 step 6)",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t.contains("extracted(s);"))
                    && b.goto_popup()
                        .is_some_and(|g| g.read(cx).field_text(cx) == "extracted")
                    && b.goto_at_name()
                    && b.status_note() == Some("Extract Method: type the name, Enter")
            }),
        ),
        Step::Key("f", Some("f")),
        Step::Key("enter", None),
        Step::WaitFor(
            "the typed name replaces `extracted` in declaration and call",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.contains("void f(") && t.contains("f(s);") && !t.contains("extracted")
                })
            }),
        ),
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                let text = e.read(cx).state.buffer.text();
                let at = text.find("Square sq").map_or(0, |i| i + "Square s".len());
                e.update(cx, |e, cx| e.select_range(at..at, cx));
            }
        })),
        Step::Key(refactor_this, None),
        Step::WaitFor(
            "⌃T on a name lists Rename and the extractions (SPEC-0095 v5 behaviour 4a)",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    let titles = v.read(cx).titles();
                    titles.iter().any(|t| t == "Rename…")
                        && titles.iter().any(|t| t == "Extract Variable…")
                })
            }),
        ),
        Step::Key(escape_key, None),
    ]
}

/// One undo for a refactoring across files (SPEC-0193 AK-4, with clangd): a rename that changes the
/// open C++ file and a closed Markdown file is taken back in both by one ⌘Z in the editor and redone
/// by ⇧⌘Z.
fn undo_group_steps() -> Vec<Step> {
    let code = "int width(int s) { return s; }\nint main() { return width(2); }\n";
    let notes = "Call width first.\n";
    let (undo, redo, save, close) = if cfg!(target_os = "macos") {
        ("cmd-z", "cmd-shift-z", "cmd-s", "cmd-w")
    } else {
        ("ctrl-z", "ctrl-shift-z", "ctrl-s", "ctrl-f4")
    };
    let notes_text = || {
        FIXTURE
            .get()
            .and_then(|d| std::fs::read_to_string(d.join("WIDTH.md")).ok())
    };
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("width.cpp"), code);
                let _ = std::fs::write(dir.join("WIDTH.md"), notes);
                b.set_rename_options(idecraft_languages::rename_text::TextOptions {
                    comments_and_strings: true,
                    text_files: true,
                });
                b.open(&dir.join("width.cpp"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    let at = code.find("width").unwrap_or(0) + 1;
                    e.update(cx, |e, cx| e.select_range(at..at, cx));
                }
            }
        })),
        Step::Key("shift-f6", None),
        Step::Key("w", Some("w")),
        Step::Key("enter", None),
        Step::WaitFor(
            "the preview lists the Markdown occurrence",
            Box::new(|b, cx| {
                b.rename_preview()
                    .is_some_and(|p| !p.read(cx).rows().is_empty())
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "the rename changes the open file and the closed Markdown file",
            Box::new(move |b, cx| {
                b.active_text(cx).is_some_and(|t| t.contains("return w(2)"))
                    && notes_text().is_some_and(|t| t == "Call w first.\n")
            }),
        ),
        Step::Key(undo, None),
        Step::WaitFor(
            "one undo in the editor takes the rename back in both files",
            Box::new(move |b, cx| {
                b.active_text(cx).is_some_and(|t| t == code)
                    && notes_text().is_some_and(|t| t == notes)
                    && b.status_message(cx)
                        .is_some_and(|m| m.starts_with("Undone: Rename"))
            }),
        ),
        Step::Key(redo, None),
        Step::WaitFor(
            "redo renames in both files again",
            Box::new(move |b, cx| {
                b.active_text(cx).is_some_and(|t| t.contains("return w(2)"))
                    && notes_text().is_some_and(|t| t == "Call w first.\n")
            }),
        ),
        // A modified tab is never evicted; save and close it for the steps after.
        Step::Key(save, None),
        Step::Key(close, None),
    ]
}

/// Rename in comments, strings and text files (SPEC-0148 AK-4, with clangd): ⇧F6 shows the
/// options, ⌥T adds text occurrences; the preview lists the comment and the Markdown line; Space
/// leaves the Markdown out; Enter renames the code and the comment only.
fn rename_text_steps() -> Vec<Step> {
    use idecraft_languages::rename_text::OccurrenceKind;
    let code =
        "// area of a square\nint area(int s) { return s * s; }\nint main() { return area(2); }\n";
    let notes = "Call area for the area.\n";
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("area.cpp"), code);
                let _ = std::fs::write(dir.join("AREA.md"), notes);
                b.open(&dir.join("area.cpp"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    let at = code.find("int area").unwrap_or(0) + 5;
                    e.update(cx, |e, cx| e.select_range(at..at, cx));
                }
            }
        })),
        Step::Key("shift-f6", None),
        Step::Expect(
            "the rename popup searches comments and strings, not text files, by default",
            Box::new(|b, cx| {
                b.goto_popup()
                    .and_then(|g| g.read(cx).rename_options())
                    .is_some_and(|o| o.comments_and_strings && !o.text_files)
            }),
        ),
        Step::Key("alt-t", None),
        Step::Expect(
            "alt-t switches on text occurrences",
            Box::new(|b, cx| {
                b.goto_popup()
                    .and_then(|g| g.read(cx).rename_options())
                    .is_some_and(|o| o.text_files)
            }),
        ),
        Step::Key("s", Some("s")),
        Step::Key("q", Some("q")),
        Step::Key("enter", None),
        Step::WaitFor(
            "the preview lists the occurrence in the Markdown file and in the comment",
            Box::new(|b, cx| {
                b.rename_preview().is_some_and(|p| {
                    let kinds: Vec<_> = p
                        .read(cx)
                        .rows()
                        .iter()
                        .map(|(o, on)| (o.kind, *on))
                        .collect();
                    kinds
                        == [
                            (OccurrenceKind::Text, true),
                            (OccurrenceKind::Text, true),
                            (OccurrenceKind::CommentOrString, true),
                        ]
                })
            }),
        ),
        Step::Key("space", None),
        Step::Key("down", None),
        Step::Key("space", None),
        Step::Expect(
            "space leaves both Markdown occurrences out",
            Box::new(|b, cx| {
                b.rename_preview().is_some_and(|p| {
                    p.read(cx)
                        .rows()
                        .iter()
                        .map(|(_, on)| *on)
                        .collect::<Vec<_>>()
                        == [false, false, true]
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "enter renames the code and the comment, the Markdown file stays",
            Box::new(move |b, cx| {
                b.rename_preview().is_none()
                    && b.active_text(cx).is_some_and(|t| {
                        t == "// sq of a square\nint sq(int s) { return s * s; }\nint main() { return sq(2); }\n"
                    })
                    && FIXTURE.get().is_some_and(|d| {
                        std::fs::read_to_string(d.join("AREA.md")).is_ok_and(|t| t == notes)
                    })
            }),
        ),
    ]
}

/// Change Signature (SPEC-0113 AK-4, with clangd): ⌘F6 on a call opens the parameters of the
/// function; swapping them and adding one with a value rewrites the declaration and both calls.
fn signature_steps() -> Vec<Step> {
    let key = if cfg!(target_os = "macos") {
        "cmd-f6"
    } else {
        "ctrl-f6"
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let text = "int sub(int a, int b) { return a - b; }\nint main(void) { return sub(5, 2) + sub(1, 0); }\n";
                let _ = std::fs::write(dir.join("signature.c"), text);
                b.open(&dir.join("signature.c"), window, cx);
                if let Some(e) = b.active_editor_entity() {
                    let at = text.find("sub(5").unwrap_or(0) + 1;
                    e.update(cx, |e, cx| e.select_range(at..at, cx));
                }
            }
        })),
        Step::Key(key, None),
        Step::WaitFor(
            "the shortcut opens the parameters of the function at the caret",
            Box::new(|b, cx| {
                b.goto_popup()
                    .is_some_and(|g| g.read(cx).field_text(cx) == "int a, int b")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(g) = b.goto_popup() {
                g.update(cx, |g, cx| {
                    g.set_field_text("int b, int a, int scale = 1", cx)
                });
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "the declaration and every call follow the new parameter list",
            Box::new(|b, cx| {
                b.active_text(cx).as_deref()
                    == Some(
                        "int sub(int b, int a, int scale) { return a - b; }\nint main(void) { return sub(2, 5, 1) + sub(0, 1, 1); }\n",
                    )
            }),
        ),
    ]
}

/// Move (SPEC-0114 AK-4): F6 on an open header asks for the folder; moving it into a subfolder
/// rewrites the include of the file that uses it, and the tab follows the file.
fn move_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::create_dir_all(dir.join("mv"));
                let _ = std::fs::write(dir.join("mv/lib.h"), "int one(void);\n");
                let _ = std::fs::write(dir.join("mv/use.c"), "#include \"lib.h\"\n");
                b.open(&dir.join("mv/lib.h"), window, cx);
            }
        })),
        Step::Key("f6", None),
        Step::WaitFor(
            "F6 asks for the folder, starting with the file's own",
            Box::new(|b, cx| {
                b.goto_popup()
                    .is_some_and(|g| g.read(cx).field_text(cx) == "mv")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(g) = b.goto_popup() {
                g.update(cx, |g, cx| g.set_field_text("mv/include", cx));
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "the file moves, the include follows and the tab shows the new place",
            Box::new(|b, cx| {
                let Some(dir) = FIXTURE.get() else {
                    return false;
                };
                dir.join("mv/include/lib.h").exists()
                    && !dir.join("mv/lib.h").exists()
                    && std::fs::read_to_string(dir.join("mv/use.c"))
                        .ok()
                        .as_deref()
                        == Some("#include \"include/lib.h\"\n")
                    && b.status_message(cx)
                        .is_some_and(|m| m.starts_with("Moved to mv/include/lib.h"))
                    && b.active_text(cx).as_deref() == Some("int one(void);\n")
            }),
        ),
    ]
}

/// Whether the Spring tool window is drawn, not only marked as shown (SPEC-0115 AK-19): it was
/// painted since it was shown, with room for its header and rows, and the window that stood at
/// the bottom gave way.
fn spring_drawn(b: &WorkbenchView, cx: &gpui::App) -> bool {
    use idecraft_workbench::tool_windows::ToolWindow;
    b.spring_view().is_some_and(|v| {
        let (paints, width, height) = v.read(cx).painted();
        paints > 0 && width >= 200. && height >= 80.
    }) && b.tool_window_shown(ToolWindow::Spring)
        && !b.tool_window_shown(ToolWindow::Problems)
}

/// Spring (SPEC-0115 AK-4): an injection point without a bean and an undefined `@Value` key are
/// warnings in the editor; Go to Bean jumps from an interface-typed injection to its
/// implementation; the Spring tool window lists the problem and the beans.
///
/// SPEC-0115 AK-18 (v5): a `@Value` key shows its value in place of the placeholder and `•••`
/// for a secret; the bean and its injection point have gutter marks; Go to Declaration on the
/// key opens its line in `application.properties` and from there leads back; typed text narrows
/// the tool window and the panel shows what the selected bean needs.
fn spring_steps() -> Vec<Step> {
    let keys = "package spr;\n@Service\nclass Keys {\n  Keys(Store store) {}\n  @Value(\"${app.name}\") String name;\n  @Value(\"${app.api-key}\") String key;\n}\n";
    let declaration = if cfg!(target_os = "macos") {
        "cmd-b"
    } else {
        "ctrl-b"
    };
    let shown = |b: &WorkbenchView, cx: &gpui::App| -> Vec<(usize, String, String)> {
        b.active_editor_entity()
            .map(|e| e.read(cx).shown_values())
            .unwrap_or_default()
    };
    let problems = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.active_editor_entity()
            .map(|e| {
                e.read(cx)
                    .problems()
                    .iter()
                    .map(|p| p.message.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    let api = "package spr;\n@RestController\nclass Api {\n  Api(Store store, Missing missing) {}\n  @Value(\"${app.mode}\") String mode;\n}\n";
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let spr = dir.join("spr");
                let _ = std::fs::create_dir_all(&spr);
                let _ = std::fs::write(
                    spr.join("Store.java"),
                    "package spr;\npublic interface Store {}\n",
                );
                let _ = std::fs::write(
                    spr.join("Missing.java"),
                    "package spr;\npublic interface Missing {}\n",
                );
                let _ = std::fs::write(
                    spr.join("MemStore.java"),
                    "package spr;\n@Repository\nclass MemStore implements Store {}\n",
                );
                let _ = std::fs::write(spr.join("Api.java"), api);
                b.open(&spr.join("Api.java"), window, cx);
            }
        })),
        Step::WaitFor(
            "a type without a bean and an undefined @Value key are warnings in the editor",
            Box::new(move |b, cx| {
                let found = problems(b, cx);
                found.iter().any(|m| m == "Spring: No bean of type Missing found")
                    && found.iter().any(|m| {
                        m == "Spring: Property 'app.mode' is not defined in the application configuration"
                    })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                let at = api.find("Store store").unwrap_or(0) + 1;
                e.update(cx, |e, cx| e.select_range(at..at, cx));
            }
            b.goto_bean(window, cx);
        })),
        Step::Expect(
            "Go to Bean jumps from the interface to the bean that implements it",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| t.contains("class MemStore implements Store"))
            }),
        ),
        // SPEC-0115 AK-18 (v5): values, marks and Go to Declaration on a configuration key.
        Step::Setup(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let spr = dir.join("spr");
                let _ = std::fs::write(
                    spr.join("application.properties"),
                    "app.name=Demo\napp.api-key=abc123\n",
                );
                let _ = std::fs::write(spr.join("Keys.java"), keys);
                b.open(&spr.join("Keys.java"), window, cx);
            }
        })),
        Step::WaitFor(
            "the editor shows a key's value in place of its placeholder and masks a secret",
            Box::new(move |b, cx| {
                shown(b, cx)
                    == [
                        (4, "${app.name}".to_owned(), "Demo".to_owned()),
                        (5, "${app.api-key}".to_owned(), "•••".to_owned()),
                    ]
            }),
        ),
        Step::Expect(
            "the bean and its injection point have gutter marks",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    e.framework_mark(2).is_some_and(|m| m.glyph == "●")
                        && e.framework_mark(3).is_some_and(|m| {
                            m.glyph == "→"
                                && m.targets.len() == 1
                                && m.targets[0].location.path.ends_with("MemStore.java")
                        })
                })
            }),
        ),
        Step::Setup(Box::new(move |b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                let at = keys.find("app.name").unwrap_or(0) + 2;
                e.update(cx, |e, cx| e.select_range(at..at, cx));
            }
        })),
        Step::Expect(
            "the placeholder under the caret is shown as written",
            Box::new(move |b, cx| {
                shown(b, cx) == [(5, "${app.api-key}".to_owned(), "•••".to_owned())]
            }),
        ),
        Step::Key(declaration, None),
        Step::WaitFor(
            "Go to Declaration on the key opens its line in application.properties",
            Box::new(|b, cx| {
                b.active_path()
                    .is_some_and(|p| p.ends_with("application.properties"))
                    && b.caret_position(cx) == Some((1, 1))
            }),
        ),
        Step::Key(declaration, None),
        Step::WaitFor(
            "Go to Declaration on the key in the configuration file leads back to the code",
            Box::new(|b, cx| {
                b.active_path().is_some_and(|p| p.ends_with("Keys.java"))
                    && b.caret_position(cx).is_some_and(|(line, _)| line == 5)
            }),
        ),
        // Opened as View › Tool Windows › Spring does, while another window is shown at the
        // bottom (SPEC-0115 AK-19).
        Step::Setup(Box::new(|b, window, cx| {
            b.menu_action("ActivateProblemsViewToolWindow", window, cx);
        })),
        Step::Expect(
            "Problems is shown at the bottom before Spring opens",
            Box::new(|b, _| {
                b.tool_window_shown(idecraft_workbench::tool_windows::ToolWindow::Problems)
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            b.menu_action("Spring.ToolWindow", window, cx)
        })),
        Step::WaitFor(
            "the menu action draws the Spring tool window in place of the window at the bottom",
            Box::new(spring_drawn),
        ),
        Step::WaitFor(
            "the Spring tool window lists the problem and the beans",
            Box::new(|b, cx| {
                b.spring_view().is_some_and(|v| {
                    let t = v.read(cx).texts();
                    t.iter().any(|l| l == "! No bean of type Missing found")
                        && t.iter().any(|l| l == "  memStore: MemStore")
                        && t.iter().any(|l| l == "  api: Api")
                })
            }),
        ),
        // SPEC-0115 AK-18 (v5): typed text narrows the beans, the panel names what the selected
        // bean needs, Escape clears the text and leaves the window open.
        Step::Key("k", Some("k")),
        Step::Key("e", Some("e")),
        Step::Key("y", Some("y")),
        Step::Expect(
            "typed text narrows the Spring tool window to the beans that contain it",
            Box::new(|b, cx| {
                b.spring_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.filter().text == "key" && v.texts() == ["Service (1)", "  keys: Keys"]
                })
            }),
        ),
        Step::Key("down", None),
        Step::Expect(
            "the panel beside the list shows what the selected bean needs",
            Box::new(|b, cx| {
                b.spring_view().is_some_and(|v| {
                    let d = v.read(cx).details();
                    d.first().is_some_and(|l| l == "keys: Keys")
                        && d.iter().any(|l| l == "→ memStore: MemStore")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape clears the typed text and leaves the Spring tool window open",
            Box::new(|b, cx| {
                b.spring_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.filter().text.is_empty() && v.texts().iter().any(|l| l == "  api: Api")
                })
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            b.menu_action("Spring.ToolWindow", window, cx)
        })),
        Step::Expect(
            "the menu action hides the Spring tool window again",
            Box::new(|b, _| b.spring_view().is_none()),
        ),
        // SPEC-0115 AK-20 (v6): the chooser of a gutter mark with two targets opens at the mark,
        // is as wide as its rows and starts each row with the file and its line.
        Step::Setup(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let deep = dir.join("spr/payment/gateways/internal");
                let _ = std::fs::create_dir_all(&deep);
                let _ = std::fs::write(
                    dir.join("spr/Pay.java"),
                    "package spr;\npublic interface Pay {}\n",
                );
                let _ = std::fs::write(
                    deep.join("CardGatewayImplementation.java"),
                    "package spr.payment.gateways.internal;\n@Component\nclass CardGatewayImplementation implements spr.Pay {}\n",
                );
                let _ = std::fs::write(
                    deep.join("CashGatewayImplementation.java"),
                    "package spr.payment.gateways.internal;\n@Component\nclass CashGatewayImplementation implements spr.Pay {}\n",
                );
                let _ = std::fs::write(
                    dir.join("spr/Till.java"),
                    "package spr;\n@Service\nclass Till {\n  Till(Pay pay) {}\n}\n",
                );
                b.open(&dir.join("spr/Till.java"), window, cx);
            }
        })),
        Step::WaitFor(
            "the injection point with two beans has a gutter mark with two targets",
            Box::new(|b, cx| {
                b.active_path().is_some_and(|p| p.ends_with("Till.java"))
                    && b.active_editor_entity().is_some_and(|e| {
                        e.read(cx)
                            .framework_mark(3)
                            .is_some_and(|m| m.targets.len() == 2)
                    })
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            b.click_framework_mark(3, window, cx)
        })),
        Step::WaitFor(
            "the chooser of the mark opens at the mark, wide enough for its rows, file and line first",
            Box::new(|b, cx| {
                let Some(popup) = b.jvm_popup() else {
                    return false;
                };
                let popup = popup.read(cx);
                let rows = popup.items();
                let folders = popup.details();
                rows.len() == 2
                    && rows[0].starts_with("CardGatewayImplementation.java:3  ")
                    && rows[1].starts_with("CashGatewayImplementation.java:3  ")
                    && folders
                        .iter()
                        .all(|f| f.ends_with("spr/payment/gateways/internal"))
                    && popup.width() >= 460.
                    && b.caret_list_placement(cx).is_some()
            }),
        ),
        // SPEC-0115 AK-19: the row of the More tool windows list opens the window too, also
        // while a chooser is open and another window stands at the bottom.
        Step::Setup(Box::new(|b, window, cx| {
            b.menu_action("ActivateProblemsViewToolWindow", window, cx);
            b.toggle_more_tool_windows(cx);
        })),
        Step::Expect(
            "the More tool windows list has a row for Spring",
            Box::new(|b, _| {
                b.more_tool_windows_open()
                    && b.more_tool_windows_listed()
                        .contains(&idecraft_workbench::tool_windows::ToolWindow::Spring)
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            b.choose_more_tool_window(
                idecraft_workbench::tool_windows::ToolWindow::Spring,
                window,
                cx,
            )
        })),
        Step::WaitFor(
            "the row of the More tool windows list draws the Spring tool window with its beans",
            Box::new(|b, cx| {
                !b.more_tool_windows_open()
                    && spring_drawn(b, cx)
                    && b.spring_view()
                        .is_some_and(|v| v.read(cx).texts().iter().any(|l| l == "  api: Api"))
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            b.toggle_more_tool_windows(cx);
            b.choose_more_tool_window(
                idecraft_workbench::tool_windows::ToolWindow::Spring,
                window,
                cx,
            )
        })),
        Step::Expect(
            "the row hides the shown Spring tool window",
            Box::new(|b, _| b.spring_view().is_none() && !b.more_tool_windows_open()),
        ),
        // SPEC-0134: a Quarkus resource with CDI injection and a MicroProfile config key. As in a
        // generated Quarkus project the source imports nothing from io.quarkus; the module's
        // pom.xml names Quarkus (v2 behaviour 1).
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let cdi = dir.join("cdi");
                let _ = std::fs::create_dir_all(&cdi);
                let _ = std::fs::write(
                    cdi.join("pom.xml"),
                    "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>\n",
                );
                let _ = std::fs::write(
                    cdi.join("Hello.java"),
                    "package cdi;\nimport jakarta.ws.rs.Path;\nimport org.eclipse.microprofile.config.inject.ConfigProperty;\n@Path(\"/hello\")\nclass Hello {\n  @Inject Missing missing;\n  @ConfigProperty(name = \"greeting.text\") String text;\n}\n",
                );
                b.open(&cdi.join("Hello.java"), window, cx);
            }
        })),
        Step::WaitFor(
            "CDI injection and @ConfigProperty keys are checked and labelled Quarkus",
            Box::new(move |b, cx| {
                let found = problems(b, cx);
                found.iter().any(|m| m == "Quarkus: No bean of type Missing found")
                    && found.iter().any(|m| {
                        m == "Quarkus: Property 'greeting.text' is not defined in the application configuration"
                    })
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join("cdi"));
            }
        })),
    ]
}

/// Python environments (SPEC-0117): an environment chosen in the settings shows in the status bar;
/// the Python Packages window lists the missing declaration and the installed package; the
/// chooser marks the choice; a failing install is reported; everything is undone afterwards.
/// A package index on a local port with three names and one package (SPEC-0171 self-test).
fn fake_package_index() -> Option<String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let url = format!("http://{}", listener.local_addr().ok()?);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            use std::io::{BufRead, Write};
            let mut reader = std::io::BufReader::new(&stream);
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                    break;
                }
            }
            let (status, body) = match first.split_whitespace().nth(1).unwrap_or("") {
                "/simple/" => (
                    "200 OK",
                    r#"{"projects":[{"name":"rich"},{"name":"rich-click"},{"name":"enrich"},{"name":"flask"}]}"#,
                ),
                "/pypi/rich/json" => (
                    "200 OK",
                    r#"{"info":{"name":"rich","summary":"Rich text","version":"13.7.1","requires_python":">=3.7"},"releases":{"13.7.1":[{"yanked":false}],"13.7.0":[{"yanked":false}],"14.0.0a1":[{"yanked":false}]}}"#,
                ),
                _ => ("404 Not Found", ""),
            };
            let mut out = &stream;
            let _ = write!(
                out,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Some(url)
}

/// Pushes of the interpreter to the Python language server before the self-test's last change.
static PYTHON_PUSHES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn python_steps() -> Vec<Step> {
    let search = |key: &'static str| {
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = b.python_view() {
                v.update(cx, |v, cx| {
                    v.press(key, cx);
                });
            }
        }))
    };
    let texts = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.python_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    };
    let command = |b: &WorkbenchView, cx: &gpui::App| -> String {
        b.python_view()
            .and_then(|v| v.read(cx).command_text())
            .unwrap_or_default()
    };
    vec![
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("pyproject.toml"),
                    "[project]\nname = \"demo\"\ndependencies = [\"idecraft-demo-pkg>=1\"]\n",
                );
                let env = dir.join("pyenv-demo");
                let _ = std::fs::create_dir_all(env.join("bin"));
                let _ = std::fs::write(env.join("pyvenv.cfg"), "version = 3.12.4\n");
                let _ = std::fs::write(env.join("bin/python"), "");
                let meta = env.join("lib/python3.12/site-packages/rich-13.7.0.dist-info");
                let _ = std::fs::create_dir_all(&meta);
                let _ = std::fs::write(meta.join("METADATA"), "Name: rich\nVersion: 13.7.0\n");
                // With pip, so the installer is pip whatever this machine has (SPEC-0171 v2).
                let _ = std::fs::create_dir_all(env.join("lib/python3.12/site-packages/pip"));
                b.set_interpreter(Some(env.join("bin/python")), cx);
            }
        })),
        Step::WaitFor(
            "the chosen environment shows in the status bar",
            Box::new(|b, _| b.python_label().as_deref() == Some("Python 3.12 (pyenv-demo)")),
        ),
        Step::Act(Box::new(|b, window, cx| b.toggle_python(window, cx))),
        Step::WaitFor(
            "Python Packages lists the missing declaration first, then the installed package",
            Box::new(|b, cx| {
                b.python_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.texts() == ["! idecraft-demo-pkg >=1 (missing)", "rich 13.7.0"]
                        && v.label() == "Python 3.12 (pyenv-demo)"
                        && v.missing_specs() == ["idecraft-demo-pkg>=1"]
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.choose_interpreter(window, cx))),
        Step::Expect(
            "the chooser marks the chosen environment and offers the automatic choice and a new one",
            Box::new(|b, cx| {
                b.python_popup().is_some_and(|p| {
                    let items = p.read(cx).items().to_vec();
                    items[0].starts_with("● Python 3.12.4 · venv · configured")
                        && items.iter().any(|i| i.trim() == "Automatic choice")
                        && items
                            .iter()
                            .any(|i| i.trim() == "New virtual environment in .venv")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the chooser",
            Box::new(|b, _| b.python_popup().is_none()),
        ),
        // SPEC-0171: search the (fake) index, show a package, choose a release, go back.
        Step::Act(Box::new(|b, _, cx| {
            if let (Some(url), Some(dir)) = (fake_package_index(), FIXTURE.get()) {
                b.set_package_index(url, dir.join(".idecraft/pypi-names-selftest.txt"));
            }
            if let Some(v) = b.python_view() {
                v.update(cx, |v, cx| v.set_field("rich", cx));
            }
        })),
        Step::WaitFor(
            "typing a name lists the index's matches, the installed one marked",
            Box::new(move |b, cx| {
                texts(b, cx) == ["rich (installed 13.7.0)", "rich-click", "enrich"]
            }),
        ),
        search("enter"),
        Step::WaitFor(
            "Enter shows the releases with the latest chosen and the pip command",
            Box::new(move |b, cx| {
                let t = texts(b, cx);
                t.first().is_some_and(|l| l == "rich 13.7.1 — Rich text")
                    && t.iter().any(|l| l == "● 13.7.1")
                    && t.iter().any(|l| l == "  14.0.0a1 (pre-release)")
                    && command(b, cx).ends_with(
                        "pyenv-demo/bin/python -m pip install --disable-pip-version-check rich==13.7.1",
                    )
            }),
        ),
        search("right"),
        Step::Expect(
            "→ chooses the older, installed release and the command follows",
            Box::new(move |b, cx| {
                texts(b, cx).iter().any(|l| l == "● 13.7.0 (installed)")
                    && command(b, cx).ends_with("rich==13.7.0")
            }),
        ),
        search("escape"),
        Step::Expect(
            "Escape returns to the matches",
            Box::new(move |b, cx| {
                texts(b, cx)
                    .first()
                    .is_some_and(|l| l == "rich (installed 13.7.0)")
            }),
        ),
        search("escape"),
        Step::Expect(
            "Escape again empties the field and shows the installed list",
            Box::new(move |b, cx| {
                texts(b, cx) == ["! idecraft-demo-pkg >=1 (missing)", "rich 13.7.0"]
            }),
        ),
        // SPEC-0117 v4: ⌥↩ on an unresolved import installs the declared requirement. The
        // language server's mark is set here, so the step does not depend on pyright.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let file = dir.join("uses_demo.py");
                let _ = std::fs::write(&file, "import idecraft_demo_pkg\n");
                b.open(&file, window, cx);
            }
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    e.set_diagnostics(
                        &[idecraft_languages::lsp::Diagnostic {
                            severity: "warning".into(),
                            start_line: 0,
                            start_character: 7,
                            end_line: 0,
                            end_character: 24,
                            message: "Import \"idecraft_demo_pkg\" could not be resolved".into(),
                            source: Some("Pyright".into()),
                            code: Some("reportMissingImports".into()),
                            unnecessary: false,
                        }],
                        cx,
                    );
                    e.go_to(0, 2, cx);
                });
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ on the unresolved import offers to install its declared requirement",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str)
                        == Some("Install package idecraft-demo-pkg")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "installing with an interpreter that cannot run reports the failure",
            Box::new(|b, cx| {
                b.status_message(cx)
                    .is_some_and(|m| m.starts_with("Failed: "))
                    && b.python_view().is_some_and(|v| v.read(cx).busy().is_none())
            }),
        ),
        Step::Expect(
            "the failed command and its output show in the Python Packages window (SPEC-0117 v5)",
            Box::new(|b, cx| {
                b.python_view().is_some_and(|v| {
                    v.read(cx).failure().is_some_and(|f| {
                        f.command
                            .as_deref()
                            .is_some_and(|c| c.ends_with("idecraft-demo-pkg>=1"))
                    })
                })
            }),
        ),
        // SPEC-0117 v5 / SPEC-0171 v2: without pip in the environment the install goes through
        // uv (a stub on an injected PATH that fails); the hidden window opens with the failure,
        // which stays in the status bar in front of the import's problem at the caret.
        Step::Setup(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(
                    dir.join("pyenv-demo/lib/python3.12/site-packages/pip"),
                );
                let tools = dir.join("pyenv-tools");
                let _ = std::fs::create_dir_all(&tools);
                let _ = std::fs::write(
                    tools.join("uv"),
                    "#!/bin/sh\necho 'error: stub uv cannot reach the index' >&2\nexit 2\n",
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        tools.join("uv"),
                        std::fs::Permissions::from_mode(0o755),
                    );
                }
                b.set_python_tool_path(Some(tools.into()), cx);
            }
            b.toggle_python(window, cx);
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(0, 10, cx));
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Expect(
            "the window is hidden and the status bar shows the import's problem at the caret",
            Box::new(|b, cx| {
                b.python_view().is_none()
                    && b.status_message(cx)
                        .is_some_and(|m| m.contains("could not be resolved"))
            }),
        ),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ offers the install again",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str)
                        == Some("Install package idecraft-demo-pkg")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "without pip the install runs uv; its failure opens the window and holds the status bar",
            Box::new(|b, cx| {
                b.status_message(cx).as_deref()
                    == Some("Failed: error: stub uv cannot reach the index")
                    && b.python_view().is_some_and(|v| {
                        v.read(cx).failure().is_some_and(|f| {
                            f.title == "Command failed (exit code 2)"
                                && f.command.as_deref().is_some_and(|c| {
                                    c.contains("pyenv-tools/uv pip install --python ")
                                        && c.ends_with("pyenv-demo/bin/python idecraft-demo-pkg>=1")
                                })
                        })
                    })
            }),
        ),
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(0, 11, cx));
            }
        })),
        Step::Expect(
            "after the caret moved the problem at the caret is shown again (SPEC-0070 v9)",
            Box::new(|b, cx| {
                b.status_message(cx)
                    .is_some_and(|m| m.contains("could not be resolved"))
            }),
        ),
        Step::Key(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-f4"
            },
            None,
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.set_python_tool_path(None, cx);
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(dir.join("pyenv-tools"));
            }
            PYTHON_PUSHES.store(
                b.python_server_pushes(),
                std::sync::atomic::Ordering::SeqCst,
            );
            b.set_interpreter(None, cx);
            b.toggle_python(window, cx);
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("uses_demo.py"));
                let _ = std::fs::remove_file(dir.join("pyproject.toml"));
                let _ = std::fs::remove_dir_all(dir.join("pyenv-demo"));
            }
        })),
        Step::WaitFor(
            "the new interpreter is pushed to the Python language server (SPEC-0170)",
            Box::new(|b, _| {
                b.python_server_pushes() > PYTHON_PUSHES.load(std::sync::atomic::Ordering::SeqCst)
            }),
        ),
        Step::Expect(
            "the automatic choice is back and the window is hidden",
            Box::new(|b, _| {
                b.python_view().is_none()
                    && FIXTURE
                        .get()
                        .is_some_and(|d| !d.join(".idecraft/python.toml").exists())
            }),
        ),
    ]
}

/// What the self-test's save step of quitting returned (SPEC-0261 v3).
static QUIT_SAVE: std::sync::Mutex<Option<Result<(), String>>> = std::sync::Mutex::new(None);

/// Ruby (SPEC-0027 v5, SPEC-0185, SPEC-0042 v6): a Ruby file is highlighted; with a Ruby
/// language server installed, Quick Documentation of a class answers through it. The file is saved,
/// since ruby-lsp indexes saved files only.
/// What the self-test's MCP client over HTTP got back, and the port it used.
static MCP_HTTP_REPLY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static MCP_HTTP_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// One POST to the window's MCP server: the reply's session header and JSON-RPC message.
fn mcp_http_post(
    port: u16,
    token: &str,
    session: Option<&str>,
    body: &str,
) -> std::io::Result<(Option<String>, String)> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(20)))?;
    let session = session.map_or(String::new(), |s| format!("Mcp-Session-Id: {s}\r\n"));
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n{session}MCP-Protocol-Version: 2025-06-18\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    let (head, rest) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let id = head.lines().find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.eq_ignore_ascii_case("mcp-session-id")
            .then(|| v.trim().to_owned())
    });
    // The message is the JSON object in the body or in the event stream's data line.
    let message = rest
        .lines()
        .map(|l| l.strip_prefix("data:").unwrap_or(l).trim())
        .find(|l| l.starts_with('{'))
        .unwrap_or_default()
        .to_owned();
    Ok((id, message))
}

thread_local! {
    /// The window of the prompt-keys group, whose semantic tree its checks read.
    static PROMPT_WINDOW: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// The answer the group's last question gave (`None` while none was given).
    static PROMPT_ANSWER: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    /// Whether the semantic tree was built before the group switched it on.
    static PROMPT_TREE_WAS_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The tree of the prompt-keys window's last frame.
fn prompt_tree() -> Option<std::rc::Rc<idecraft_uidriver::Tree>> {
    idecraft_workbench::semantics::tree(PROMPT_WINDOW.get())
}

/// The drawn question is in the last frame's tree and its focused answer is answer `ix`; with
/// `name`, the answer is named so too.
fn prompt_focus_on(ix: usize, name: Option<&'static str>) -> Check {
    use idecraft_workbench::semantics::{DRAWN_PROMPT, drawn_prompt_answer};
    Box::new(move |_, _| {
        let Some(t) = prompt_tree() else {
            return false;
        };
        let Some(dialog) = t.iter().find(|n| n.test_id() == Some(DRAWN_PROMPT)) else {
            return false;
        };
        let Some(focused) = t.focused() else {
            return false;
        };
        t.is_ancestor_or_self(dialog.id, focused.id)
            && focused.test_id() == Some(drawn_prompt_answer(ix).as_str())
            && name.is_none_or(|n| focused.name() == Some(n))
    })
}

/// The question gave `answer` and is gone from the window.
fn prompt_answered(answer: &'static str) -> Check {
    Box::new(move |_, _| {
        PROMPT_ANSWER.with_borrow(|a| a.as_deref() == Some(answer))
            && prompt_tree().is_some_and(|t| {
                t.iter()
                    .all(|n| n.test_id() != Some(idecraft_workbench::semantics::DRAWN_PROMPT))
            })
    })
}

/// Asks the agent's request for a right through the window's own question (`mcp_http::ask`), as a
/// call over HTTP would; the answer is only recorded, nothing is granted.
fn prompt_ask_permission() -> Step {
    Step::Setup(Box::new(|b, _, cx| {
        PROMPT_ANSWER.set(None);
        let request = idecraft_mcp::window::PermissionRequest {
            client: "selftest-agent 1.0".into(),
            right: "write".into(),
            meaning: "create, change, move or delete files of the workspace".into(),
            action: "create_file {\"dir\":\".\",\"name\":\"prompt-keys.txt\"}".into(),
            workspace: b.project_root().to_path_buf(),
            scope: idecraft_mcp::window::Scope::Workspace,
        };
        // Outside this window's update: the question updates the window itself.
        cx.defer(move |cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let asking = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            crate::mcp_http::ask(request, tx, asking, cx);
            cx.spawn(async move |_| {
                let answer = rx
                    .await
                    .map_or_else(|_| "none".into(), |a| format!("{a:?}"));
                PROMPT_ANSWER.set(Some(answer));
            })
            .detach();
        });
    }))
}

/// Asks the trust question with the text and buttons of `WorkbenchView::ask_trust`; its answer is
/// only recorded (Don't Open would close the window, a trust answer would write the trust list).
fn prompt_ask_trust(opening: bool) -> Step {
    Step::Setup(Box::new(move |b, window, cx| {
        PROMPT_ANSWER.set(None);
        let root = b.project_root().to_path_buf();
        let answers = idecraft_workbench::trust_model::answers(&root, opening);
        let buttons = idecraft_workbench::trust_model::prompt_buttons(&answers);
        let labels: Vec<String> = answers.into_iter().map(|(label, _)| label).collect();
        let (message, detail) = idecraft_core::trust::question(&root);
        let reply = window.prompt(
            gpui::PromptLevel::Warning,
            &message,
            Some(&detail),
            &buttons,
            cx,
        );
        cx.spawn(async move |_, _| {
            let answer = reply
                .await
                .ok()
                .and_then(|ix| labels.get(ix).cloned())
                .unwrap_or_else(|| "none".into());
            PROMPT_ANSWER.set(Some(answer));
        })
        .detach();
    }))
}

/// The file the group's Delete questions are about.
fn prompt_file() -> Option<PathBuf> {
    FIXTURE.get().map(|d| d.join("prompt-keys.txt"))
}

/// Delete in the project tree, on a file of its own.
fn prompt_ask_delete() -> Step {
    Step::Setup(Box::new(|b, window, cx| {
        let Some(file) = prompt_file() else {
            return;
        };
        let _ = std::fs::write(&file, "kept unless Delete is the answer\n");
        b.ask_delete(std::slice::from_ref(&file), window, cx);
    }))
}

/// The Delete question was answered (it is gone and the workbench finished it), and the file is
/// there or not.
fn prompt_file_after_delete(exists: bool) -> Check {
    Box::new(move |b, _| {
        b.delete_question().is_none()
            && prompt_tree().is_some_and(|t| {
                t.iter()
                    .all(|n| n.test_id() != Some(idecraft_workbench::semantics::DRAWN_PROMPT))
            })
            && prompt_file().is_some_and(|f| f.exists() == exists)
    })
}

/// SPEC-0281 v4 behaviour 9, AK-10: the question the toolkit draws where the platform has no alert,
/// on the platform's own path — on Linux `window.prompt` finds no system alert and gpui draws its
/// fallback question over the window. The keys go in as every key of the self-test does
/// (`dispatch_keystroke` inside the app), never through the X server. Each check reads the
/// question from the window's semantic tree (its focused answer, its place, modal and reserved).
/// macOS asks in a system alert, which keys sent inside the app do not reach (hand test 219), so
/// the group has no steps there.
fn prompt_keys_steps() -> Vec<Step> {
    use idecraft_workbench::semantics::{self, Consumer, DRAWN_PROMPT, Reserved, Role};
    if !cfg!(target_os = "linux") {
        eprintln!("prompt-keys steps skipped: the platform asks in its own alerts");
        return Vec::new();
    }
    let key = |k: &'static str| Step::Key(k, None);
    let mut steps = vec![
        Step::Setup(Box::new(|_, window, cx| {
            // Confirmations as a user gets them: in native alerts, which gpui draws on Linux.
            if cx.has_global::<idecraft_workbench::dialogs::DrawnConfirmations>() {
                cx.remove_global::<idecraft_workbench::dialogs::DrawnConfirmations>();
            }
            PROMPT_TREE_WAS_ON.set(semantics::active());
            semantics::set_consumer(Consumer::SelfTest, true);
            PROMPT_WINDOW.set(semantics::window_id(window));
            window.refresh();
        })),
        // An agent's request for a right: Deny is the default and the safe answer.
        prompt_ask_permission(),
        Step::WaitFor(
            "an agent's request for a right is drawn by the toolkit, its focus ring on Deny",
            prompt_focus_on(0, Some("Deny")),
        ),
        Step::Expect(
            "the question is a modal, reserved alertdialog with its three answers in order",
            Box::new(|_, _| {
                let Some(t) = prompt_tree() else {
                    return false;
                };
                let Some(d) = t.iter().find(|n| n.test_id() == Some(DRAWN_PROMPT)) else {
                    return false;
                };
                let answers: Vec<_> = (0..3)
                    .filter_map(|ix| {
                        let id = semantics::drawn_prompt_answer(ix);
                        t.iter().find(|n| n.test_id() == Some(id.as_str()))
                    })
                    .collect();
                d.role() == Role::AlertDialog
                    && d.node.is_modal()
                    && d.reserved == Some(Reserved::Decision)
                    && d.name().is_some_and(|m| m.contains("selftest-agent 1.0"))
                    && answers.len() == 3
                    && answers
                        .iter()
                        .all(|a| a.reserved == Some(Reserved::Decision))
                    && answers.iter().map(|a| a.name().unwrap_or_default()).eq([
                        "Deny",
                        "Allow Once",
                        "Always for This Workspace",
                    ])
                    && answers
                        .windows(2)
                        .all(|w| w[0].bounds().y < w[1].bounds().y)
                    && t.reserved_modal().is_some_and(|m| m.id == d.id)
            }),
        ),
        // Escape: no cancel answer, the question stays; Tab then moves the ring.
        key("escape"),
        key("tab"),
        Step::WaitFor(
            "Escape gave no answer; Tab moved the focus ring to Allow Once",
            prompt_focus_on(1, Some("Allow Once")),
        ),
        key("tab"),
        Step::WaitFor(
            "Tab moved the focus ring to Always for This Workspace",
            prompt_focus_on(2, Some("Always for This Workspace")),
        ),
        key("tab"),
        Step::WaitFor(
            "Tab wrapped around to Deny",
            prompt_focus_on(0, Some("Deny")),
        ),
        key("shift-tab"),
        Step::WaitFor(
            "Shift+Tab wrapped back to Always for This Workspace",
            prompt_focus_on(2, Some("Always for This Workspace")),
        ),
        Step::Key("l", Some("l")),
        Step::WaitFor(
            "a typed character moved the focus ring back to Deny",
            prompt_focus_on(0, Some("Deny")),
        ),
        key("tab"),
        Step::WaitFor(
            "Tab moved the focus ring to Allow Once again",
            prompt_focus_on(1, Some("Allow Once")),
        ),
        key("enter"),
        Step::WaitFor(
            "Return gave the default, Deny, with the focus on Allow Once",
            prompt_answered("Deny"),
        ),
        prompt_ask_permission(),
        Step::WaitFor(
            "asked again, the focus ring is on Deny",
            prompt_focus_on(0, Some("Deny")),
        ),
        key("space"),
        Step::WaitFor(
            "Space gave the focused answer, Deny",
            prompt_answered("Deny"),
        ),
        prompt_ask_permission(),
        Step::WaitFor(
            "asked a third time, the focus ring is on Deny",
            prompt_focus_on(0, Some("Deny")),
        ),
        key("tab"),
        key("space"),
        Step::WaitFor(
            "Tab, then Space gave Allow Once, the answer chosen on purpose",
            prompt_answered("AllowOnce"),
        ),
        // The trust question when the window opens: Preview in Safe Mode is the default and the
        // safe answer, Don't Open the cancel answer.
        prompt_ask_trust(true),
        Step::WaitFor(
            "the trust question is drawn, its focus ring on Preview in Safe Mode",
            prompt_focus_on(0, Some("Preview in Safe Mode")),
        ),
        key("space"),
        Step::WaitFor(
            "Space gave Preview in Safe Mode",
            prompt_answered("Preview in Safe Mode"),
        ),
        prompt_ask_trust(true),
        Step::WaitFor(
            "asked again, the focus ring is on Preview in Safe Mode",
            prompt_focus_on(0, Some("Preview in Safe Mode")),
        ),
        key("enter"),
        Step::WaitFor(
            "Return gave Preview in Safe Mode",
            prompt_answered("Preview in Safe Mode"),
        ),
        prompt_ask_trust(true),
        Step::WaitFor(
            "asked a third time, the focus ring is on Preview in Safe Mode",
            prompt_focus_on(0, Some("Preview in Safe Mode")),
        ),
        key("escape"),
        Step::WaitFor("Escape gave Don't Open", prompt_answered("Don't Open")),
        // Asked again from the banner: no Don't Open, so Escape gives nothing.
        prompt_ask_trust(false),
        Step::WaitFor(
            "the trust question from the banner is drawn, its focus ring on Preview in Safe Mode",
            prompt_focus_on(0, Some("Preview in Safe Mode")),
        ),
        key("escape"),
        key("tab"),
        Step::WaitFor(
            "Escape gave no answer; Tab moved the focus ring to the second answer",
            prompt_focus_on(1, None),
        ),
        key("shift-tab"),
        Step::WaitFor(
            "Shift+Tab moved it back to Preview in Safe Mode",
            prompt_focus_on(0, Some("Preview in Safe Mode")),
        ),
        key("enter"),
        Step::WaitFor(
            "Return gave Preview in Safe Mode",
            prompt_answered("Preview in Safe Mode"),
        ),
        // Delete in the project tree: Delete is the default, Cancel the cancel and safe answer.
        prompt_ask_delete(),
        Step::WaitFor(
            "Delete in the project tree is drawn, its focus ring on Cancel",
            prompt_focus_on(1, Some("Cancel")),
        ),
        key("space"),
        Step::WaitFor(
            "Space gave Cancel: the file is still there",
            prompt_file_after_delete(true),
        ),
        prompt_ask_delete(),
        Step::WaitFor(
            "asked again, the focus ring is on Cancel",
            prompt_focus_on(1, Some("Cancel")),
        ),
        key("escape"),
        Step::WaitFor(
            "Escape gave Cancel: the file is still there",
            prompt_file_after_delete(true),
        ),
        prompt_ask_delete(),
        Step::WaitFor(
            "asked a third time, the focus ring is on Cancel",
            prompt_focus_on(1, Some("Cancel")),
        ),
        key("tab"),
        Step::WaitFor(
            "Tab moved the focus ring to Delete",
            prompt_focus_on(0, Some("Delete")),
        ),
        key("shift-tab"),
        Step::WaitFor(
            "Shift+Tab moved it back to Cancel",
            prompt_focus_on(1, Some("Cancel")),
        ),
        key("enter"),
        Step::WaitFor(
            "Return gave the default, Delete: the file is gone",
            prompt_file_after_delete(false),
        ),
    ];
    steps.push(Step::Setup(Box::new(|_, _, cx| {
        cx.set_global(idecraft_workbench::dialogs::DrawnConfirmations);
        if !PROMPT_TREE_WAS_ON.get() {
            semantics::set_consumer(Consumer::SelfTest, false);
        }
        if let Some(file) = prompt_file() {
            let _ = std::fs::remove_file(file);
        }
    })));
    steps
}

/// SPEC-0221 v3 AK-5 and AK-8, SPEC-0220 AK-3: MCP over HTTP is off by default and switching it on
/// in Settings › AI Assistant starts the server; an unsaved edit is in the document registry and a
/// client over HTTP reads it while Settings is open; switching `mcp.http` off closes the port.
fn mcp_http_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    let open_settings = if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-alt-s"
    };
    let note = || FIXTURE.get().map(|d| d.join("mcp-note.txt"));
    let mut steps = vec![
        Step::Expect(
            "MCP over HTTP is off by default: the window serves nothing",
            Box::new(|_, cx| {
                !idecraft_workbench::appearance::ide_settings(cx).mcp.http
                    && crate::mcp_http::running(cx).is_none()
            }),
        ),
        Step::Key(open_settings, None),
        Step::WaitFor(
            "Settings opens",
            Box::new(|b, _| b.settings_dialog().is_some()),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.show(Page::Ai, cx);
                    v.toggle_mcp_http(cx);
                });
            }
        })),
        Step::Expect(
            "switching it on in Settings › AI Assistant starts the server at once",
            Box::new(|_, cx| {
                idecraft_workbench::appearance::ide_settings(cx).mcp.http
                    && crate::mcp_http::running(cx).is_some()
            }),
        ),
        Step::Key("escape", None),
        Step::WaitFor(
            "Settings closes",
            Box::new(|b, _| b.settings_dialog().is_none()),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(file) = note() {
                let _ = std::fs::write(&file, "saved\n");
                b.open(&file, window, cx);
            }
        })),
    ];
    steps.extend("unsaved".chars().map(|ch| {
        let s: &'static str = Box::leak(ch.to_string().into_boxed_str());
        Step::Key(s, Some(s))
    }));
    steps.extend([
        // SPEC-0261 v3 behaviour 14 (D51): quitting sees the unsaved file of the first window.
        Step::Expect(
            "quitting lists the unsaved file of the first window and would ask",
            Box::new(|_, cx| {
                let (unsaved, running) = WorkbenchView::pending_work(cx);
                unsaved.iter().any(|f| f.ends_with("mcp-note.txt"))
                    && idecraft_workbench::quit_model::question(&unsaved, &running)
                        .is_some_and(|q| q.message == "Save changes before quitting?")
            }),
        ),
        Step::Expect(
            "the unsaved text is in the document registry, marked modified",
            Box::new(move |b, cx| {
                let Some(file) = note() else { return false };
                let doc = idecraft_editor::documents::DocumentRegistry::global().get(&file);
                doc.is_some_and(|d| {
                    d.modified && Some(d.text.to_string()) == b.active_text(cx)
                        && d.text.to_string().contains("unsaved")
                }) && std::fs::read_to_string(&file).is_ok_and(|t| t == "saved\n")
            }),
        ),
        Step::Key(open_settings, None),
        Step::Act(Box::new(|_, _, cx| {
            let Some((port, token)) = crate::mcp_http::running(cx) else {
                return;
            };
            MCP_HTTP_PORT.store(port, std::sync::atomic::Ordering::SeqCst);
            *MCP_HTTP_REPLY.lock().unwrap_or_else(|e| e.into_inner()) = None;
            std::thread::spawn(move || {
                let call = || -> std::io::Result<String> {
                    let (session, _) = mcp_http_post(
                        port,
                        &token,
                        None,
                        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"selftest","version":"0"}}}"#,
                    )?;
                    let session = session.as_deref();
                    mcp_http_post(
                        port,
                        &token,
                        session,
                        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                    )?;
                    let (_, reply) = mcp_http_post(
                        port,
                        &token,
                        session,
                        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"mcp-note.txt"}}}"#,
                    )?;
                    Ok(reply)
                };
                let reply = call().unwrap_or_else(|e| format!("error: {e}"));
                *MCP_HTTP_REPLY.lock().unwrap_or_else(|e| e.into_inner()) = Some(reply);
            });
        })),
        Step::WaitFor(
            "a tool over HTTP reads the unsaved buffer while Settings is open",
            Box::new(|b, _| {
                b.settings_dialog().is_some()
                    && MCP_HTTP_REPLY
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_deref()
                        .is_some_and(|r| r.contains("unsavedsaved"))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| {
                    v.show(Page::Ai, cx);
                    v.toggle_mcp_http(cx);
                });
            }
        })),
        Step::WaitFor(
            "switching MCP over HTTP off closes the port",
            Box::new(|_, cx| {
                let port = MCP_HTTP_PORT.load(std::sync::atomic::Ordering::SeqCst);
                crate::mcp_http::running(cx).is_none()
                    && std::net::TcpStream::connect(("127.0.0.1", port)).is_err()
            }),
        ),
        // Switching it on was the first step of this group; it stays off for the groups after.
        Step::Key("escape", None),
        // SPEC-0261 v3 behaviour 15 (D57): the save step of quitting (⌘Q's Save All, `quit_ide`
        // save_all, `idecraft quit --save-all`) saves the first window's editors. Whether it may
        // quit depends on every file the ~500 steps before left open, so the check is on this
        // file: saved, and not named among what keeps IDECraft open (AK-7).
        Step::Act(Box::new(|_, _, cx| {
            cx.defer(|cx| {
                let saved = WorkbenchView::prepare_quit(
                    idecraft_workbench::quit_model::QuitChoice::SaveAll,
                    cx,
                );
                if let Err(why) = &saved {
                    eprintln!("note: the save step of quitting would not quit: {why}");
                }
                *QUIT_SAVE.lock().unwrap_or_else(|e| e.into_inner()) = Some(saved);
            });
        })),
        Step::WaitFor(
            "the save step of quitting saves the unsaved file; the registry entry is no longer modified",
            Box::new(move |_, _| {
                QUIT_SAVE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .is_some_and(|r| r.as_ref().err().is_none_or(|e| !e.contains("mcp-note.txt")))
                    && note().is_some_and(|f| {
                        std::fs::read_to_string(&f).is_ok_and(|t| t.contains("unsaved"))
                            && idecraft_editor::documents::DocumentRegistry::global()
                                .get(&f)
                                .is_some_and(|d| !d.modified)
                    })
            }),
        ),
    ]);
    steps
}

fn ruby_steps() -> Vec<Step> {
    const TEXT: &str = "class Greeter\n  # Says hello to someone.\n  def greet(name)\n    \"Hello #{name}\"\n  end\nend\n\ngreeter = Greeter.new\ngreeter.greet(\"you\")\n";
    let mut steps = vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("greeter.rb"), TEXT);
                b.open(&dir.join("greeter.rb"), window, cx);
            }
        })),
        Step::WaitFor(
            "a Ruby file is highlighted: comment and string, not the keyword",
            Box::new(|b, cx| {
                let at = |s: &str| TEXT[..TEXT.find(s).unwrap_or(0)].chars().count() + 1;
                b.active_editor_entity().is_some_and(|e| {
                    let e = e.read(cx);
                    e.in_comment_or_string(at("Says"))
                        && e.in_comment_or_string(at("Hello"))
                        && !e.in_comment_or_string(at("class"))
                })
            }),
        ),
    ];
    let server = FIXTURE.get().and_then(|d| {
        idecraft_languages::servers::resolve_for(idecraft_languages::LanguageId::Ruby, d).ok()
    });
    let Some(server) = server else {
        eprintln!("ruby server steps skipped: no Ruby language server installed");
        return steps;
    };
    eprintln!("ruby server: {}", server.command_line());
    let goto_line = if cfg!(target_os = "macos") {
        "cmd-l"
    } else {
        "ctrl-g"
    };
    steps.extend([
        Step::Key(goto_line, None),
        Step::Key("8", Some("8")),
        Step::Key(",", Some(",")),
        Step::Key("1", Some("1")),
        Step::Key("3", Some("3")),
        Step::Key("enter", None),
        Step::Key(
            if cfg!(target_os = "macos") {
                "f1"
            } else {
                "ctrl-q"
            },
            None,
        ),
        Step::WaitFor(
            "quick documentation of a Ruby class answers through the Ruby language server",
            Box::new(|b, _| {
                b.doc_blocks().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        matches!(block, DocBlock::Code(t) | DocBlock::Text(t, _) if t.contains("Greeter"))
                    })
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the documentation",
            Box::new(|b, _| b.doc_blocks().is_none()),
        ),
    ]);
    steps
}

/// Containers (SPEC-0119) against a fake Docker daemon and a fake `kubectl`: listing, starting,
/// logs, restarting a deployment, switching context to one that refuses, removing with two
/// presses of Delete.
fn containers_steps() -> Vec<Step> {
    let texts = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.containers_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    };
    let press = |part: &'static str, keys: &'static [&'static str]| {
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = b.containers_view() {
                v.update(cx, |v, cx| {
                    v.select_text(part);
                    for k in keys {
                        v.press(k, cx);
                    }
                });
            }
        }))
    };
    // The steps before leave several windows at the bottom; stacked, each shows about three rows,
    // and the row the real click below aims at would lie outside the Containers window. They are
    // hidden for these steps and shown again after them.
    let hidden: std::rc::Rc<std::cell::RefCell<Option<idecraft_workbench::HiddenBottom>>> =
        std::rc::Rc::default();
    let restore = hidden.clone();
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            *hidden.borrow_mut() = Some(b.hide_bottom_windows(cx));
            let dir = std::path::PathBuf::from(format!("/tmp/idc-self-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let engine = idecraft_containers::fake::FakeEngine::start(&dir.join("d.sock")).ok();
            let kubectl = idecraft_containers::fake::kubectl_script(&dir)
                .ok()
                .map(|program| idecraft_containers::Kubectl { program });
            let host = engine
                .as_ref()
                .map(|e| idecraft_containers::DockerHost::Unix(e.socket.clone()));
            // The engine's thread serves until the self-test ends.
            std::mem::forget(engine);
            b.set_container_sources(host.clone(), kubectl);
            // Two names for the one fake engine, so `e` can switch (SPEC-0243 AK-5).
            if let Some(h) = host {
                b.set_container_engines(vec![
                    idecraft_containers::Engine::from_host("first", h.clone()),
                    idecraft_containers::Engine::from_host("second", h),
                ]);
            }
            // A Compose project whose `web` service is the fake's `web` container (SPEC-0242).
            let project = dir.join("shop");
            let _ = std::fs::create_dir_all(&project);
            let _ = std::fs::write(
                project.join("compose.yaml"),
                "services:\n  web:\n    image: nginx:1.27\n  worker:\n    image: busybox\n",
            );
            let cli = idecraft_containers::fake::compose_script(&dir)
                .ok()
                .map(|program| idecraft_containers::compose::ComposeCli {
                    program,
                    prefix: Vec::new(),
                });
            b.set_compose_sources(Some(project), cli);
            b.toggle_containers(window, cx);
        })),
        Step::WaitFor(
            "the Containers window lists containers, images and the Kubernetes workloads",
            Box::new(move |b, cx| {
                let t = texts(b, cx);
                t.iter()
                    .any(|l| l == "  ● web nginx:1.27 Up 3 minutes 0.0.0.0:8080->80/tcp")
                    && t.iter()
                        .any(|l| l == "  ○ db postgres:18 Exited (0) 2 hours ago")
                    && t.iter().any(|l| l.starts_with("  nginx:1.27 cccccccccccc"))
                    && t.iter().any(|l| l == "Kubernetes — dev / shop (3)")
                    && t.iter()
                        .any(|l| l.starts_with("  Deployment web Progressing 1/2"))
                    && t.iter()
                        .any(|l| l.starts_with("  Pod web-7d9 Running 1/1 ↻2"))
            }),
        ),
        // A click selects a row (D39); the selection stays on it when the rows change (D49).
        click("containers-row-container:db", Click::Left(1)),
        Step::WaitFor(
            "a click selects the container's row",
            Box::new(|b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx)
                        .selected_text()
                        .is_some_and(|t| t.contains("db postgres:18"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.containers_view() {
                v.update(cx, |v, cx| v.press("f5", cx));
            }
        })),
        Step::WaitFor(
            "after F5 the same container stays selected",
            Box::new(|b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx)
                        .selected_text()
                        .is_some_and(|t| t.contains("db postgres:18"))
                })
            }),
        ),
        press("○ db", &["s"]),
        Step::WaitFor(
            "s starts the stopped container",
            Box::new(move |b, cx| {
                texts(b, cx)
                    .iter()
                    .any(|l| l.starts_with("  ● db postgres:18"))
                    && b.status_message(cx).as_deref() == Some("db started")
            }),
        ),
        press("● web", &["enter"]),
        Step::WaitFor(
            "Enter shows the container's logs, stdout and stderr together",
            Box::new(|b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx).logs().is_some_and(|(title, text)| {
                        title == "Logs: web" && text == "GET / 200\nwarn: slow\n"
                    })
                })
            }),
        ),
        press("Pod web-7d9", &["enter"]),
        Step::WaitFor(
            "Enter on a pod shows its logs through kubectl",
            Box::new(|b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx).logs().is_some_and(|(title, text)| {
                        title == "Logs: Pod web-7d9" && text.contains("GET /healthz 200")
                    })
                })
            }),
        ),
        press("Deployment web", &["r"]),
        Step::WaitFor(
            "r restarts the deployment",
            Box::new(|b, cx| {
                b.status_message(cx).as_deref() == Some("deployment.apps/web restarted")
            }),
        ),
        Step::WaitFor(
            "the Compose project lists its services with their states and the other engine",
            Box::new(move |b, cx| {
                let t = texts(b, cx);
                t.iter().any(|l| l == "Compose: shop (compose.yaml)")
                    && t.iter().any(|l| l == "  ◆ web running")
                    && t.iter().any(|l| l == "  ◆ worker not created")
                    && t.iter().any(|l| l.starts_with("  Engine second unix://"))
            }),
        ),
        press("◆ web", &["enter"]),
        Step::WaitFor(
            "Enter on a service shows its containers' logs",
            Box::new(|b, cx| {
                b.containers_view().is_some_and(|v| {
                    v.read(cx).logs().is_some_and(|(title, text)| {
                        title == "Logs: service web" && text.contains("GET / 200")
                    })
                })
            }),
        ),
        press("Compose: shop", &["u"]),
        Step::WaitFor(
            "u on the project runs Compose up through the CLI",
            Box::new(|b, cx| {
                b.status_message(cx).as_deref()
                    == Some("Compose up shop: Container shop-web-1  Started")
            }),
        ),
        press("● web", &["o"]),
        Step::Expect(
            "o opens the published port",
            Box::new(|b, _| b.opened_urls().iter().any(|u| u == "http://localhost:8080")),
        ),
        press("● web", &["e"]),
        Step::WaitFor(
            "e opens a shell in the container in a terminal tab, bridged by IDECraft itself",
            Box::new(|b, cx| {
                b.terminal_tabs().last().is_some_and(|t| t == "Exec: web")
                    && b.terminal()
                        .is_some_and(|t| t.read(cx).history_text().contains("fake shell"))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("hello\r"));
            }
        })),
        Step::WaitFor(
            "what is typed reaches the shell inside the container",
            Box::new(|b, cx| {
                b.terminal()
                    .is_some_and(|t| t.read(cx).history_text().contains("you said hello"))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("exit 0\r"));
            }
            // The terminal replaced the Containers window at the bottom (SPEC-0061 v10
            // behaviour 49); one toggle shows it again.
            if b.containers_view().is_none() {
                b.toggle_containers(window, cx);
            }
        })),
        press("● db", &["x"]),
        Step::WaitFor(
            "x stops the container",
            Box::new(move |b, cx| {
                texts(b, cx)
                    .iter()
                    .any(|l| l.starts_with("  ○ db postgres:18"))
            }),
        ),
        press("○ db", &["e"]),
        Step::Expect(
            "e on a stopped container says to start it first",
            Box::new(|b, cx| {
                b.status_message(cx).as_deref() == Some("db is not running: start it first (s)")
            }),
        ),
        press("Pod web-7d9", &["f"]),
        Step::WaitFor(
            "f forwards the pod's port on 127.0.0.1 and lists the forward",
            Box::new(move |b, cx| {
                texts(b, cx).iter().any(|l| {
                    l.starts_with("  ⇄ pod/web-7d9 127.0.0.1:") && l.ends_with("→ 80 (dev / shop)")
                })
            }),
        ),
        press("⇄ pod/web-7d9", &["x"]),
        Step::WaitFor(
            "x on the forward stops it",
            Box::new(move |b, cx| {
                !texts(b, cx).iter().any(|l| l.contains("⇄ pod/web-7d9"))
                    && b.status_message(cx).as_deref() == Some("Port forward stopped")
            }),
        ),
        press("Docker 27.3.1", &["e"]),
        Step::WaitFor(
            "e on the Docker heading switches to the next engine",
            Box::new(move |b, cx| {
                b.status_message(cx)
                    .is_some_and(|s| s.starts_with("Docker engine: second (unix://"))
                    && texts(b, cx)
                        .iter()
                        .any(|l| l.starts_with("  Engine first unix://"))
            }),
        ),
        press("Kubernetes", &["c"]),
        Step::WaitFor(
            "c switches to the next context and shows why it cannot be read",
            Box::new(move |b, cx| {
                texts(b, cx).iter().any(|l| {
                    l == "  prod / default: kubectl: error: You must be logged in to the server (Unauthorized)"
                })
            }),
        ),
        press("● web", &["delete"]),
        Step::Expect(
            "the first Delete asks before removing",
            Box::new(|b, cx| {
                b.containers_view()
                    .is_some_and(|v| v.read(cx).removing() == Some("web"))
            }),
        ),
        press("● web", &["delete", "delete"]),
        Step::WaitFor(
            "the second Delete removes the container",
            Box::new(move |b, cx| {
                let t = texts(b, cx);
                t.iter().any(|l| l == "Containers (1)")
                    && !t.iter().any(|l| l.contains(" web nginx"))
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            b.toggle_containers(window, cx);
            b.set_container_sources(None, None);
            b.set_compose_sources(None, None);
            idecraft_containers::ports::PortForwards::global().stop_all();
            let _ = std::fs::remove_dir_all(format!("/tmp/idc-self-{}", std::process::id()));
            if let Some(h) = restore.borrow_mut().take() {
                b.restore_bottom_windows(h, cx);
            }
        })),
        Step::Expect(
            "the window hides again",
            Box::new(|b, _| b.containers_view().is_none()),
        ),
    ]
}

/// Whether `program` is a file in one of the PATH's folders.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// Profiler (SPEC-0120): the selected Python configuration is profiled with cProfile; the call
/// tree opens the hot path, Hot Spots lists the busy function first, Enter opens its source line,
/// the flame graph draws the frames, Escape hides the window.
fn profiler_steps() -> Vec<Step> {
    static SAVED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    static SPAWNED: std::sync::OnceLock<std::sync::Mutex<Vec<std::process::Child>>> =
        std::sync::OnceLock::new();
    let texts = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.profiler_view()
            .map(|v| v.read(cx).texts())
            .unwrap_or_default()
    };
    let press = |keys: &'static [&'static str]| {
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = b.profiler_view() {
                v.update(cx, |v, cx| {
                    for k in keys {
                        v.press(k, cx);
                    }
                });
            }
        }))
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = SAVED.set(std::fs::read_to_string(dir.join(".idecraft/run.toml")).ok());
                let _ = std::fs::write(
                    dir.join("prof.py"),
                    // The work is crunch's own: with a generator, cProfile splits it between the
                    // generator and `sum`, which came first in turn from run to run
                    // (Linux window self-test, 2026-11-15).
                    "def crunch(n):\n    t = 0; i = 0\n    while i < n: t += i * i; i += 1\n    return t\ndef main():\n    for _ in range(8):\n        crunch(200000)\n\n\nmain()\n",
                );
                let _ = std::fs::write(
                    dir.join(".idecraft/run.toml"),
                    "[[run]]\nname = \"prof\"\ncommand = [\"python3\", \"prof.py\"]\n",
                );
                b.select_run_configuration("prof", cx);
                b.profile_selected(window, cx);
            }
        })),
        Step::WaitFor(
            "profiling the python configuration shows a cProfile call tree down to crunch",
            Box::new(move |b, cx| {
                b.profiler_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.busy()
                        && v.summary().starts_with("prof · cProfile · ")
                        && v.tab() == idecraft_workbench::ProfilerTab::CallTree
                }) && texts(b, cx)
                    .iter()
                    .any(|l| l.trim_start().starts_with("▾ main") && l.ends_with("prof.py:5"))
            }),
        ),
        press(&["tab"]),
        Step::Expect(
            "Hot Spots lists crunch, the busy function, first",
            Box::new(move |b, cx| {
                b.profiler_view()
                    .is_some_and(|v| v.read(cx).tab() == idecraft_workbench::ProfilerTab::HotSpots)
                    && texts(b, cx)
                        .first()
                        .is_some_and(|l| l.starts_with("crunch") && l.contains("prof.py:1"))
            }),
        ),
        press(&["enter"]),
        Step::WaitFor(
            "Enter opens prof.py at the function's line",
            Box::new(|b, cx| {
                b.active_file().is_some_and(|p| p.ends_with("prof.py"))
                    && b.caret_position(cx).map(|(l, _)| l) == Some(1)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.profiler_view() {
                window.focus(&v.read(cx).focus());
                v.update(cx, |v, cx| {
                    v.press("3", cx);
                });
            }
        })),
        Step::Expect(
            "the flame graph draws main above crunch",
            Box::new(move |b, cx| {
                let t = texts(b, cx);
                let main = t.iter().position(|l| l.contains(" main "));
                let crunch = t.iter().position(|l| l.contains(" crunch "));
                matches!((main, crunch), (Some(m), Some(c)) if m < c)
            }),
        ),
        press(&["escape"]),
        Step::Expect(
            "Escape hides the window; the profile is kept in .idecraft/profiles",
            Box::new(|b, _| {
                b.profiler_view().is_none()
                    && FIXTURE
                        .get()
                        .is_some_and(|d| d.join(".idecraft/profiles/last.json").exists())
            }),
        ),
        // SPEC-0136: a Java configuration runs under JDK Flight Recorder.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("Busy.java"),
                    "public class Busy {\n    static long inner(long n) {\n        long s = 0;\n        for (long i = 0; i < n; i++) s += i % 7;\n        return s;\n    }\n    public static void main(String[] a) {\n        long t = 0;\n        for (int k = 0; k < 1000; k++) t += inner(1_000_000);\n        System.out.println(t);\n    }\n}\n",
                );
                let _ = std::fs::write(
                    dir.join(".idecraft/run.toml"),
                    "[[run]]\nname = \"jprof\"\ncommand = [\"java\", \"Busy.java\"]\n",
                );
                b.select_run_configuration("jprof", cx);
                b.profile_selected(window, cx);
            }
        })),
        Step::WaitFor(
            "profiling the Java configuration records with JFR",
            Box::new(|b, cx| {
                b.profiler_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.busy() && v.summary().starts_with("jprof · JFR · ")
                })
            }),
        ),
        press(&["2"]),
        Step::Expect(
            "Hot Spots lists Busy.inner first with its line in Busy.java",
            Box::new(move |b, cx| {
                texts(b, cx)
                    .first()
                    .is_some_and(|l| l.starts_with("Busy.inner") && l.contains("Busy.java:"))
            }),
        ),
        press(&["escape"]),
        // SPEC-0250: a memory profile of a Python configuration.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("hold.py"),
                    "keep = []\n\n\ndef make(n):\n    return [str(i) * 20 for i in range(n)]\n\n\nfor _ in range(4):\n    keep.append(make(20000))\n",
                );
                let _ = std::fs::write(
                    dir.join(".idecraft/run.toml"),
                    "[[run]]\nname = \"mem\"\ncommand = [\"python3\", \"hold.py\"]\n",
                );
                b.select_run_configuration("mem", cx);
                b.menu_action("Profiler.ProfileMemory", window, cx);
            }
        })),
        Step::WaitFor(
            "Profile Memory records the Python configuration with tracemalloc",
            Box::new(|b, cx| {
                b.profiler_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.busy() && v.summary().starts_with("mem · tracemalloc · ")
                })
            }),
        ),
        press(&["2"]),
        Step::Expect(
            "Hot Spots lists make first with its line and a size in bytes",
            Box::new(move |b, cx| {
                texts(b, cx).first().is_some_and(|l| {
                    l.starts_with("make") && l.contains("hold.py:5") && l.contains(" MB)")
                })
            }),
        ),
        group("profiler-attach"),
        // SPEC-0251: the process list, a refused look-alike of the reference, attaching.
        Step::Act(Box::new(|b, window, cx| {
            let spin = std::process::Command::new("python3")
                .args([
                    "-c",
                    "import time\nwhile True: sum(range(1000))",
                    "selftest-spin",
                ])
                .spawn()
                .ok();
            let fake = std::process::Command::new("/bin/sh")
                .args(["-c", "sleep 60; true", "com.intellij.selftest"])
                .spawn()
                .ok();
            let _ = SPAWNED.set(std::sync::Mutex::new(
                [spin, fake].into_iter().flatten().collect(),
            ));
            std::thread::sleep(std::time::Duration::from_millis(300));
            b.set_attach_seconds(2);
            b.menu_action("Profiler.AttachToProcess", window, cx);
        })),
        Step::WaitFor(
            "the process list shows the processes the test started, the look-alike refused",
            Box::new(|b, cx| {
                b.profiler_view().is_some_and(|v| {
                    let t = v.read(cx).process_texts();
                    t.iter()
                        .any(|l| l.contains("selftest-spin") && !l.ends_with("refused"))
                        && t.iter().any(|l| {
                            l.contains("com.intellij.selftest") && l.ends_with("— refused")
                        })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.profiler_view() {
                v.update(cx, |v, cx| {
                    v.type_filter("com.intellij.selftest", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::Expect(
            "Enter on the look-alike says why it is refused",
            Box::new(|b, cx| {
                b.profiler_view().is_some_and(|v| {
                    v.read(cx)
                        .picker_note()
                        .is_some_and(|n| n.contains("RESEARCH_BOUNDARIES"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.profiler_view() {
                v.update(cx, |v, cx| {
                    for _ in 0.."com.intellij.selftest".len() {
                        v.press("backspace", cx);
                    }
                    v.type_filter("selftest-spin", cx);
                    v.press("enter", cx);
                });
            }
        })),
        // macOS samples with /usr/bin/sample, which notes a missing Developer Tools permission in
        // the profile; Linux samples with perf, and a kernel that does not allow it
        // (kernel.perf_event_paranoid above 2, Ubuntu's default 4) ends the attach with that reason.
        // Without perf on Linux the step cannot sample at all and says so.
        if cfg!(target_os = "linux") && !on_path("perf") {
            eprintln!(
                "profiler-attach: sampling skipped: perf is missing (linux-tools of the running kernel)"
            );
            Step::Setup(Box::new(|_, _, _| {}))
        } else {
            Step::WaitFor(
                "attaching samples the running process (or reports the missing permission)",
                Box::new(|b, cx| {
                    b.profiler_view().is_some_and(|v| {
                        let v = v.read(cx);
                        let sampled_or_noted = v.shown_run().is_some_and(|r| {
                            r.attached.is_some()
                                && (r.profile.total > 0.0
                                    || r.profile
                                        .notes
                                        .iter()
                                        .any(|n| n.contains("Developer Tools")))
                        });
                        let refused_by_the_kernel = cfg!(target_os = "linux")
                            && v.shown_run().is_none()
                            && v.summary()
                                .contains("the kernel did not allow perf to record");
                        !v.busy() && !v.picking() && (sampled_or_noted || refused_by_the_kernel)
                    })
                }),
            )
        },
        Step::Act(Box::new(|_, _, _| {
            if let Some(list) = SPAWNED.get()
                && let Ok(mut list) = list.lock()
            {
                for c in list.iter_mut() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
        })),
        group("profiler-close"),
        press(&["escape"]),
        Step::Act(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("hold.py"));
                let _ = std::fs::remove_file(dir.join("Busy.java"));
                let _ = std::fs::remove_file(dir.join("prof.py"));
                let _ = std::fs::remove_dir_all(dir.join(".idecraft/profiles"));
                match SAVED.get().cloned().flatten() {
                    Some(text) => {
                        let _ = std::fs::write(dir.join(".idecraft/run.toml"), text);
                    }
                    None => {
                        let _ = std::fs::remove_file(dir.join(".idecraft/run.toml"));
                    }
                }
            }
        })),
    ]
}

/// Java in the editor (SPEC-0122): ▶ markers on the class and `main`, the marker's Run/Debug
/// Generate in Kotlin (SPEC-0132): ⌘N in a Kotlin class offers the Kotlin choices; toString()
/// lists the properties; one undo takes it back.
fn kotlin_generate_steps() -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let (generate, undo, save, close) = if mac {
        ("cmd-n", "cmd-z", "cmd-s", "cmd-w")
    } else {
        ("alt-insert", "ctrl-z", "ctrl-s", "ctrl-f4")
    };
    const SRC: &str = "class Point(val x: Int, var name: String) {\n    fun f() {}\n}\n";
    fn file() -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join("Point.kt"))
    }
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(f) = file() {
                let _ = std::fs::write(&f, SRC);
                b.open(&f, window, cx);
                if let Some(e) = b.active_editor_entity() {
                    let at = SRC.find("fun f").unwrap_or(0);
                    e.update(cx, |e, cx| e.select_range(at..at, cx));
                }
            }
        })),
        Step::Key(generate, None),
        Step::Expect(
            "Generate in Kotlin offers the Kotlin choices",
            Box::new(|b, cx| {
                b.jvm_popup().is_some_and(|p| {
                    p.read(cx).items()
                        == [
                            "Secondary Constructor",
                            "equals() and hashCode()",
                            "toString()",
                            "Override Methods…",
                            "Implement Methods…",
                        ]
                })
            }),
        ),
        Step::Key("down", None),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::Expect(
            "toString() lists the properties in Kotlin",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.contains("    override fun toString(): String {\n        return \"Point(x=$x, name='$name')\"\n    }")
                })
            }),
        ),
        Step::Key(undo, None),
        Step::Expect(
            "one undo takes it back",
            Box::new(|b, cx| b.active_text(cx).is_some_and(|t| t == SRC)),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(f) = file() {
                let _ = std::fs::remove_file(f);
            }
        })),
    ]
}

/// menu, Optimize Imports with ⌃⌥O, Generate with ⌘N as one undo step, and Run from the marker.
fn jvm_steps() -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let (optimize, generate, undo, close) = if mac {
        ("ctrl-alt-o", "cmd-n", "cmd-z", "cmd-w")
    } else {
        ("ctrl-alt-o", "alt-insert", "ctrl-z", "ctrl-f4")
    };
    const POINT: &str = "package geo;\n\nimport java.util.Map;\nimport java.util.List;\n\npublic class Point {\n    private int x;\n    private List<String> tags;\n\n    public static void main(String[] args) {\n        System.out.println(\"point-from-marker\");\n    }\n}\n";
    fn point() -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join("jvm/Point.java"))
    }
    let popup_items = |b: &WorkbenchView, cx: &gpui::App| -> Vec<String> {
        b.jvm_popup()
            .map(|p| p.read(cx).items().to_vec())
            .unwrap_or_default()
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(p) = point() {
                let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
                let _ = std::fs::write(&p, POINT);
                b.open(&p, window, cx);
            }
        })),
        Step::WaitFor(
            "a Java class with main gets ▶ markers on the class and on main",
            Box::new(|b, cx| {
                b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).run_markers() == [5, 9])
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(p) = b.active_file() {
                b.run_marker_menu(&p, 5, window, cx);
            }
        })),
        Step::Expect(
            "a click on ▶ offers Run and Debug",
            Box::new(move |b, cx| popup_items(b, cx) == ["Run 'Point'", "Debug 'Point'"]),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the menu",
            Box::new(|b, _| b.jvm_popup().is_none()),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(p) = b.active_file() {
                b.run_marker_menu(&p, 5, window, cx);
            }
        })),
        Step::Expect(
            "the menu of ▶ is open again",
            Box::new(|b, _| b.jvm_popup().is_some()),
        ),
        click("editor-text", Click::Left(1)),
        Step::WaitFor(
            "a click outside the menu of ▶ closes it (SPEC-0275 v4 behaviour 28)",
            Box::new(|b, _| b.jvm_popup().is_none()),
        ),
        // SPEC-0275 v5 behaviour 14a: the editor menu names the class and its main method.
        Step::Act(Box::new(|b, _, cx| {
            b.open_context_menu(
                idecraft_workbench::MenuPlace::Editor,
                b.active_path(),
                gpui::point(gpui::px(200.), gpui::px(200.)),
                cx,
            );
        })),
        Step::Expect(
            "the editor menu runs and debugs 'Point.main()' (SPEC-0275 v5 behaviour 14a)",
            Box::new(|b, _| {
                b.context_menu_items().is_some_and(|(_, items)| {
                    ["Run 'Point.main()'", "Debug 'Point.main()'"]
                        .iter()
                        .all(|l| items.iter().any(|(i, on)| i == l && *on))
                })
            }),
        ),
        Step::Key("escape", None),
        // SPEC-0061 v3 behaviour 5a: the breadcrumbs end at the symbol under the caret.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| e.go_to(10, 8, cx));
            }
        })),
        Step::WaitFor(
            "inside main the symbol path is Point › main() (SPEC-0061 v3 behaviour 5a)",
            Box::new(|b, cx| {
                b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).symbol_path() == ["Point", "main()"])
            }),
        ),
        Step::Key(optimize, None),
        Step::Expect(
            "Optimize Imports removes the unused import",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.contains("import java.util.List;\n\npublic class")
                        && !t.contains("java.util.Map")
                }) && b.status_message(cx).as_deref() == Some("Removed 1 import")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            // The caret goes onto a field, inside the class.
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    let at = text[..text.find("private int x").unwrap_or(0)]
                        .chars()
                        .count();
                    e.select_range(at..at, cx);
                });
            }
        })),
        Step::Key(generate, None),
        Step::Expect(
            "Generate offers the reference's choices",
            Box::new(move |b, cx| {
                popup_items(b, cx)
                    == [
                        "Constructor",
                        "Getter",
                        "Setter",
                        "Getter and Setter",
                        "equals() and hashCode()",
                        "toString()",
                    ]
            }),
        ),
        Step::Expect(
            "the Generate list stands at the caret, inside the editor (SPEC-0122 v5 behaviour 13)",
            Box::new(|b, cx| {
                b.caret_list_placement(cx)
                    .is_some_and(|(place, rows)| place.max_height > 0. && rows >= 6)
            }),
        ),
        Step::Key("down", None),
        Step::Key("down", None),
        Step::Key("down", None),
        Step::Key("enter", None),
        // The field chooser with every field checked, then the template list (SPEC-0122 v4).
        Step::Expect(
            "Getter and Setter asks which fields, all checked",
            Box::new(move |b, cx| {
                b.jvm_popup()
                    .is_some_and(|p| p.read(cx).multiple() && !p.read(cx).checked().is_empty())
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "then which template, the usual one first",
            Box::new(move |b, cx| {
                popup_items(b, cx).first().map(String::as_str) == Some("getX() and setX()")
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "Getter and Setter adds both for every field",
            Box::new(|b, cx| {
                b.active_text(cx).is_some_and(|t| {
                    t.contains("    public int getX() {\n        return x;\n    }")
                        && t.contains("    public void setTags(List<String> tags) {\n        this.tags = tags;\n    }")
                })
            }),
        ),
        Step::Key(undo, None),
        Step::Expect(
            "one undo removes everything that was generated",
            Box::new(|b, cx| {
                b.active_text(cx)
                    .is_some_and(|t| !t.contains("getX") && !t.contains("setTags"))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            // The import line is gone, so the class marker moved up a line.
            if let Some(p) = b.active_file()
                && let Some(m) = b.run_markers_of(&p).first()
            {
                b.run_marker_menu(&p, m.line, window, cx);
            }
        })),
        Step::Expect(
            "the markers follow the edit",
            Box::new(|b, cx| {
                b.active_editor_entity()
                    .is_some_and(|e| e.read(cx).run_markers() == [4, 8])
                    && b.jvm_popup().is_some()
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Run from the marker runs the class",
            Box::new(|b, cx| {
                b.run_view().is_some_and(|v| {
                    v.read(cx).selected_output().is_some_and(|(name, text)| {
                        name == "Point"
                            && text.contains("point-from-marker")
                            && text.ends_with("Process finished with exit code 0")
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.stop_run(cx);
            b.toggle_run_window(window, cx);
            b.focus_editor(window, cx);
        })),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(p) = point() {
                let _ = std::fs::remove_dir_all(p.parent().unwrap_or(&p));
            }
        })),
    ]
}

thread_local! {
    /// The window of the remote project the self-test opened (SPEC-0317 v3 AK-7).
    static REMOTE_WINDOW: std::cell::Cell<Option<gpui::AnyWindowHandle>> =
        const { std::cell::Cell::new(None) };
    /// Its workbench, held weakly: acts run while the self-test's own workbench is being
    /// updated, so the remote one is not looked up among all workbenches (reading the one being
    /// updated panics).
    static REMOTE_BENCH: std::cell::RefCell<Option<gpui::WeakEntity<WorkbenchView>>> =
        const { std::cell::RefCell::new(None) };
}

/// Remembers the remote project's window and its workbench.
fn set_remote_window(handle: gpui::AnyWindowHandle, cx: &mut App) {
    REMOTE_WINDOW.set(Some(handle));
    let bench = handle
        .update(cx, |root, _, _| root.downcast::<WorkbenchView>().ok())
        .ok()
        .flatten();
    REMOTE_BENCH.with_borrow_mut(|b| *b = bench.map(|b| b.downgrade()));
}

/// The workbench of the remote project's window, if one is open.
fn remote_bench(cx: &App) -> Option<Entity<WorkbenchView>> {
    REMOTE_BENCH
        .with_borrow(|b| b.as_ref().and_then(gpui::WeakEntity::upgrade))
        .filter(|b| b.read(cx).remote_project().is_some())
}

/// Runs `f` on the remote project's workbench in its own window.
fn in_remote_window(
    cx: &mut App,
    f: impl FnOnce(&mut WorkbenchView, &mut Window, &mut Context<WorkbenchView>),
) {
    let (Some(handle), Some(bench)) = (REMOTE_WINDOW.get(), remote_bench(cx)) else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        bench.update(cx, |b, cx| f(b, window, cx))
    });
}

/// The remote project's active editor text and whether its tab is marked unsaved.
fn remote_editor(cx: &App) -> Option<(String, bool)> {
    let b = remote_bench(cx)?;
    let b = b.read(cx);
    let e = b.active_editor_entity()?;
    let e = e.read(cx);
    let unsaved = e.is_modified() || !b.remote_unsaved_names(cx).is_empty();
    e.is_remote_document()
        .then(|| (e.state.buffer.text(), unsaved))
}

/// Opens the self-test's remote project: a backend in this process serving `folder` (SPEC-0317
/// v3 AK-7; the SSH path through the same handle is checked against the test `sshd` in
/// `crates/app/tests/it/remote_project.rs`).
fn open_test_remote_project(
    folder: &Path,
    cx: &mut App,
) -> Option<idecraft_remote::inprocess::HostSwitch> {
    let base = FIXTURE.get()?.join(".idecraft/remote");
    let project = idecraft_remote::inprocess::InProcessProject::open(
        "selftest-host",
        folder,
        &base.join("host-state"),
        std::time::Duration::from_secs(600),
        Some(base.join("recovery")),
        std::time::Duration::ZERO,
    )
    .ok()?;
    let switch = project.switch();
    let live =
        idecraft_remote::LiveProject::start(Box::new(project), idecraft_remote::Timing::default());
    let handle = idecraft_workbench::open_live_window(live, cx).ok()?;
    set_remote_window(handle, cx);
    Some(switch)
}

/// SPEC-0317 v3 AK-7: Remote Development in the project popup with the recent remote projects; a
/// remote project's window with the host in its title and status bar, the tree from the host,
/// typing and saving on the host, a break with the widget passing through Reconnecting, typing
/// and a pending save during it, the automatic reconnection and the save carried out, a host
/// change reloading the unmodified file, a host-side create in the tree, a crash of the window
/// with the recovery copy offered and restored, and closing by an agent closing the window.
/// SPEC-0318 AK-11 (window part, written with M6.12 step 4, run with the window check): a remote C
/// project whose backend in this process has the host functions bound — diagnostics from the
/// host's language server follow typing in the remote document, ⌘B jumps to a definition in it,
/// completion lists the host's proposals, and the project closes.
fn remote_host_steps() -> Vec<Step> {
    const MAIN: &str = "int add(int a, int b) {\n    return a + b;\n}\n\nint main(void) {\n    int x = add(1, 2);\n    return 0;\n}\n";
    fn at(needle: &str) -> usize {
        MAIN.find(needle).unwrap_or(0)
    }
    fn caret_to(offset: usize) -> Step {
        Step::Act(Box::new(move |_, _, cx| {
            in_remote_window(cx, |b, _, cx| {
                if let Some(e) = b.active_editor_entity() {
                    // An empty replacement does not move the caret: select the place.
                    e.update(cx, |e, cx| e.select_range(offset..offset, cx));
                }
            });
        }))
    }
    fn action(id: &'static str) -> Step {
        Step::Act(Box::new(move |_, _, cx| {
            in_remote_window(cx, |b, window, cx| b.menu_action(id, window, cx));
        }))
    }
    let long = std::time::Duration::from_secs(90);
    vec![
        Step::Act(Box::new(|_, _, cx| {
            let dir = std::env::temp_dir().join(format!("idc-rhost-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(dir.join("remote-c"));
            let dir = dir.canonicalize().unwrap_or(dir).join("remote-c");
            let _ = std::fs::write(dir.join("main.c"), MAIN);
            let Some(base) = FIXTURE.get().map(|f| f.join(".idecraft/remote-host")) else {
                return;
            };
            let setup: idecraft_remote::inprocess::Setup = std::sync::Arc::new(|b| {
                idecraft_host::bind(b, idecraft_host::HostOptions::default());
            });
            let Ok(project) = idecraft_remote::inprocess::InProcessProject::open_with(
                "selftest-host",
                &dir,
                &base.join("host-state"),
                std::time::Duration::from_secs(600),
                Some(base.join("recovery")),
                std::time::Duration::ZERO,
                Some(setup),
            ) else {
                return;
            };
            let live = idecraft_remote::LiveProject::start(
                Box::new(project),
                idecraft_remote::Timing::default(),
            );
            if let Ok(handle) = idecraft_workbench::open_live_window(live, cx) {
                set_remote_window(handle, cx);
            }
        })),
        Step::WaitFor(
            "a remote project with the host functions opens in a window of its own",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    b.read(cx).remote_title().as_deref() == Some("remote-c — selftest-host")
                })
            }),
        ),
        Step::Act(Box::new(|_, _, cx| {
            in_remote_window(cx, |b, window, cx| {
                if let Some(p) = b.remote_local_path("main.c") {
                    b.open(&p, window, cx);
                }
            });
        })),
        Step::WaitFor(
            "main.c opens as a remote document",
            Box::new(|_, cx| remote_editor(cx).is_some_and(|(t, _)| t == MAIN)),
        ),
        // SPEC-0317 v6 detail 11: the tree names the host folder, never the stand-in.
        Step::WaitFor(
            "the tree shows the host folder beside the project name",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.remote_project()
                        .is_some_and(|(_, folder)| b.tree_root_label() == folder)
                })
            }),
        ),
        // SPEC-0322 v5 behaviour 14: a stop marks the line and moves the caret there.
        Step::Setup(Box::new(|_, _, cx| {
            in_remote_window(cx, |b, window, cx| {
                if let Some(p) = b.remote_local_path("main.c") {
                    b.selftest_show_execution_point(&p, 1, window, cx);
                }
            });
        })),
        Step::WaitFor(
            "a stop in a remote file marks the line and moves the caret",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.active_debug_marks(cx)
                        .is_some_and(|(_, exec)| exec == Some(1))
                        && b.active_editor_entity()
                            .is_some_and(|e| e.read(cx).status().line == 2)
                })
            }),
        ),
        Step::Act(Box::new(|_, _, cx| {
            let offset = at("return 0");
            in_remote_window(cx, |b, _, cx| {
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| {
                        e.replace_ranges(&[(offset..offset, "missing_name = 1;\n    ".into())], cx)
                    });
                }
            });
        })),
        Step::WaitLong(
            "the host's language server reports the undeclared name in the remote document",
            long,
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    b.read(cx).active_editor_entity().is_some_and(|e| {
                        e.read(cx)
                            .problems()
                            .iter()
                            .any(|p| p.message.contains("missing_name"))
                    })
                })
            }),
        ),
        caret_to(at("add(1") + 1),
        action("GotoDeclaration"),
        Step::WaitLong(
            "⌘B on the call jumps to the definition on line 1 of the remote document",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .is_some_and(|b| b.read(cx).caret_position(cx).is_some_and(|(l, _)| l == 1))
            }),
        ),
        caret_to(at("add(1") + 2),
        action("CodeCompletion"),
        Step::WaitLong(
            "completion in the remote document lists the host's proposal `add`",
            long,
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    b.read(cx)
                        .completion_items()
                        .is_some_and(|items| items.iter().any(|i| i.starts_with("add")))
                })
            }),
        ),
        Step::Act(Box::new(|_, _, cx| {
            if let Some(live) = remote_bench(cx).and_then(|b| b.read(cx).remote_live()) {
                std::thread::spawn(move || {
                    let _ = live.close(idecraft_remote::Unsaved::Discard);
                });
            }
        })),
        Step::WaitFor(
            "the remote C project closes",
            Box::new(|_, cx| remote_bench(cx).is_none()),
        ),
    ]
}

/// SPEC-0318 v2 AK-11 (window details 2–13, written with M6.12 step 4, run with the window check):
/// a remote C project in a Git repository whose backend in this process has the host functions
/// bound — the trust banner and Trust Project; a host terminal tab named after the host, typing,
/// a break with "(Reconnecting)" and the reattach without repeating output; Run of a host
/// configuration with both streams; a ▶ marker of a Java file offering Run on the host; Find in
/// Files and Go to File on the host; semantic colours, parameter info, rename and context actions
/// from the host's language server; the Commit window committing on the host and the Git window
/// showing it; a program listening in the host terminal offered as a port, forwarded to loopback
/// and answering through the forward, a debug port refused; what agents see; closing.
fn remote_tool_window_steps() -> Vec<Step> {
    use std::sync::Mutex;
    static HOST: Mutex<Option<(idecraft_remote::inprocess::HostSwitch, PathBuf, u16)>> =
        Mutex::new(None);
    const MAIN: &str = "int add(int a, int b) {\n    return a + b;\n}\n\nint main(void) {\n    int x = add(1, 2);\n    return x;\n}\n";
    const APP: &str = "public class App {\n    public static void main(String[] args) {\n        System.out.println(\"hi\");\n    }\n}\n";
    fn folder() -> Option<PathBuf> {
        HOST.lock().ok()?.as_ref().map(|h| h.1.clone())
    }
    fn port() -> u16 {
        HOST.lock()
            .ok()
            .and_then(|h| h.as_ref().map(|h| h.2))
            .unwrap_or(0)
    }
    fn reachable(on: bool) {
        if let Ok(h) = HOST.lock()
            && let Some((s, _, _)) = h.as_ref()
        {
            s.set_reachable(on);
        }
    }
    fn git(dir: &Path, args: &[&str]) {
        let _ = std::process::Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output();
    }
    fn act(
        f: impl Fn(&mut WorkbenchView, &mut Window, &mut Context<WorkbenchView>) + 'static,
    ) -> Step {
        let f = std::rc::Rc::new(f);
        Step::Act(Box::new(move |_, _, cx| {
            let f = f.clone();
            in_remote_window(cx, move |b, window, cx| f(b, window, cx));
        }))
    }
    fn action(id: &'static str) -> Step {
        act(move |b, window, cx| {
            b.menu_action(id, window, cx);
        })
    }
    fn terminal_text(cx: &App) -> String {
        remote_bench(cx)
            .and_then(|b| b.read(cx).terminal())
            .map(|t| t.read(cx).text())
            .unwrap_or_default()
    }
    fn type_in_terminal(text: &'static str) -> Step {
        act(move |b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text(text));
            }
        })
    }
    fn caret_to(needle: &'static str, extra: usize) -> Step {
        act(move |b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = e.state.buffer.text();
                    if let Some(at) = text.find(needle) {
                        let at = text[..at].chars().count() + extra;
                        e.select_range(at..at, cx);
                    }
                });
            }
        })
    }
    fn status(cx: &App) -> String {
        remote_bench(cx)
            .and_then(|b| b.read(cx).status_message(cx))
            .unwrap_or_default()
    }
    let long = std::time::Duration::from_secs(90);
    vec![
        // Safe mode as for a user's project: the host's trust decides (detail 12).
        Step::Setup(Box::new(|_, _, _| {
            idecraft_core::trust::stop_trusting_all_for_process()
        })),
        Step::Act(Box::new(|_, _, cx| {
            let dir = std::env::temp_dir().join(format!("idc-rtools-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(dir.join("remote-tools/.idecraft"));
            let dir = dir.canonicalize().unwrap_or(dir).join("remote-tools");
            let _ = std::fs::write(dir.join("main.c"), MAIN);
            let _ = std::fs::write(dir.join("App.java"), APP);
            let _ = std::fs::write(
                dir.join(".idecraft/run.toml"),
                "[[run]]\nname = \"hello\"\ncommand = [\"sh\", \"-c\", \"echo hello-from-host; echo to-err >&2\"]\n\n\
                 [[run]]\nname = \"tests\"\nkind = \"test\"\nreport = \"tap\"\ncommand = [\"sh\", \"-c\", \"echo 1..2; echo ok 1 - first; echo not ok 2 - second\"]\n\n\
                 [[run]]\nname = \"slow\"\ncommand = [\"sh\", \"-c\", \"echo started; sleep 60\"]\n",
            );
            git(&dir, &["init", "-q", "-b", "main"]);
            git(&dir, &["add", "."]);
            git(&dir, &["commit", "-q", "-m", "first"]);
            let Some(base) = FIXTURE.get().map(|f| f.join(".idecraft/remote-tools")) else {
                return;
            };
            let trust = base.join("host-trust.toml");
            let setup: idecraft_remote::inprocess::Setup = std::sync::Arc::new(move |b| {
                idecraft_host::bind(
                    b,
                    idecraft_host::HostOptions {
                        shell: Some("/bin/sh".into()),
                        trust_file: Some(trust.clone()),
                        ..idecraft_host::HostOptions::default()
                    },
                );
            });
            let Ok(project) = idecraft_remote::inprocess::InProcessProject::open_with(
                "selftest-host",
                &dir,
                &base.join("host-state"),
                std::time::Duration::from_secs(600),
                Some(base.join("recovery")),
                std::time::Duration::ZERO,
                Some(setup),
            ) else {
                return;
            };
            let free = std::net::TcpListener::bind("127.0.0.1:0")
                .ok()
                .and_then(|l| l.local_addr().ok())
                .map_or(0, |a| a.port());
            if let Ok(mut h) = HOST.lock() {
                *h = Some((project.switch(), dir, free));
            }
            let live = idecraft_remote::LiveProject::start(
                Box::new(project),
                idecraft_remote::Timing::default(),
            );
            if let Ok(handle) = idecraft_workbench::open_live_window(live, cx) {
                set_remote_window(handle, cx);
            }
        })),
        Step::WaitFor(
            "the remote project with tool windows on the host opens in a window of its own",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    b.read(cx).remote_title().as_deref() == Some("remote-tools — selftest-host")
                })
            }),
        ),
        // Detail 12: the host project is not trusted (the self-test trusts every project of this
        // process until here), so the banner shows; Trust Project… asks once more.
        Step::WaitFor(
            "the run configurations come from the host and the trust banner says the host project is not trusted",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.remote_run_config_names()
                        .is_some_and(|n| n == ["hello", "tests", "slow"])
                        && b.remote_untrusted()
                        && folder().is_some_and(|f| !idecraft_core::trust::allows_project_code(&f))
                })
            }),
        ),
        act(|b, window, cx| b.remote_ask_trust(window, cx)),
        Step::WaitFor(
            "Trust Project… asks once more before anything runs on the host",
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).jvm_popup())
                    .is_some_and(|p| {
                        let p = p.read(cx);
                        p.title().starts_with("Trust ")
                            && p.items().first().is_some_and(|i| i == "Trust Project")
                    })
            }),
        ),
        act(|b, _, cx| {
            if let Some(p) = b.jvm_popup() {
                p.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::ChoiceEvent::Chosen(0))
                });
            }
        }),
        Step::WaitFor(
            "Trust Project trusts the folder on the host and the banner goes, no local safe mode",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    !b.remote_untrusted() && !b.safe_mode()
                })
            }),
        ),
        // Every project of the self-test is trusted again; the main window reads it at once.
        Step::Setup(Box::new(|b, _, cx| {
            idecraft_core::trust::trust_all_for_process();
            b.refresh_trust(cx);
        })),
        // Detail 2: a host terminal tab.
        action("ActivateTerminalToolWindow"),
        Step::WaitFor(
            "the Terminal tool window opens a host terminal named after the host",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.terminal_tabs() == ["selftest-host"]
                        && b.terminal().is_some_and(|t| t.read(cx).is_linked())
                })
            }),
        ),
        type_in_terminal("echo host-$((6*7))\n"),
        Step::WaitLong(
            "typing in the host terminal runs on the host",
            long,
            Box::new(|_, cx| terminal_text(cx).contains("host-42")),
        ),
        Step::Act(Box::new(|_, _, cx| {
            reachable(false);
            if let Some(l) = remote_bench(cx).and_then(|b| b.read(cx).remote_live()) {
                l.break_connection();
            }
        })),
        Step::WaitFor(
            "during a break the tab says Reconnecting",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    b.read(cx)
                        .terminal_tab_labels(cx)
                        .iter()
                        .any(|l| l == "selftest-host (Reconnecting)")
                })
            }),
        ),
        type_in_terminal("echo again-$((6*8))\n"),
        Step::Act(Box::new(|_, _, _| reachable(true))),
        Step::WaitLong(
            "after the resume the terminal goes on: typing from the break runs, nothing repeats",
            long,
            Box::new(|_, cx| {
                let text = terminal_text(cx);
                text.contains("again-48")
                    && text.matches("host-42").count() == 1
                    && remote_bench(cx)
                        .is_some_and(|b| b.read(cx).terminal_tab_labels(cx) == ["selftest-host"])
            }),
        ),
        // Detail 3: Run on the host.
        action("Run"),
        Step::WaitLong(
            "Run starts the host configuration; both streams arrive in the Run window",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).run_view())
                    .and_then(|v| v.read(cx).output_of("hello"))
                    .is_some_and(|o| {
                        o.contains("hello-from-host")
                            && o.contains("to-err")
                            && o.contains("Process finished with exit code 0")
                    })
            }),
        ),
        // The test tree of a host run, and Stop.
        act(|b, window, cx| {
            b.select_run_configuration("tests", cx);
            b.menu_action("Run", window, cx);
        }),
        Step::WaitLong(
            "a host test run shows its test tree with one passed and one failed test",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).run_view())
                    .and_then(|v| v.read(cx).test_results())
                    .is_some_and(|t| {
                        t.rows.iter().any(|(_, l)| l.contains("second")) && t.summary.contains('1')
                    })
            }),
        ),
        act(|b, window, cx| {
            b.select_run_configuration("slow", cx);
            b.menu_action("Run", window, cx);
        }),
        Step::WaitLong(
            "a long host run is running in the Run window",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).run_view())
                    .is_some_and(|v| {
                        let v = v.read(cx);
                        v.running_names().iter().any(|n| n == "slow")
                            && v.output_of("slow").is_some_and(|o| o.contains("started"))
                    })
            }),
        ),
        act(|b, _, cx| {
            if let Some(v) = b.run_view() {
                v.update(cx, |v, cx| {
                    v.stop_selected();
                    cx.notify();
                });
            }
        }),
        Step::WaitLong(
            "Stop ends the host run",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).run_view())
                    .is_some_and(|v| !v.read(cx).running_names().iter().any(|n| n == "slow"))
            }),
        ),
        act(|b, _, cx| b.select_run_configuration("hello", cx)),
        // Detail 4: a marker of a Java file runs on the host.
        act(|b, window, cx| {
            if let Some(p) = b.remote_local_path("App.java") {
                b.open(&p, window, cx);
            }
        }),
        Step::WaitFor(
            "the ▶ markers of a remote Java document are computed in the window",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.remote_local_path("App.java")
                        .is_some_and(|p| !b.run_markers_of(&p).is_empty())
                })
            }),
        ),
        act(|b, window, cx| {
            if let Some(p) = b.remote_local_path("App.java")
                && let Some(m) = b.run_markers_of(&p).first().cloned()
            {
                b.run_marker_menu(&p, m.line, window, cx);
            }
        }),
        Step::WaitFor(
            "the marker's menu offers Run on the host",
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).jvm_popup())
                    .is_some_and(|p| {
                        p.read(cx)
                            .items()
                            .first()
                            .is_some_and(|i| i == "Run 'App' on selftest-host")
                    })
            }),
        ),
        Step::Act(Box::new(|_, _, cx| {
            if let Some(p) = remote_bench(cx).and_then(|b| b.read(cx).jvm_popup()) {
                p.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::ChoiceEvent::Chosen(usize::MAX))
                });
            }
        })),
        // Detail 5: Find in Files on the host.
        action("FindInPath"),
        act(|b, _, cx| {
            if let Some(v) = b.find_in_files() {
                v.update(cx, |v, cx| v.set_query("int add", cx));
            }
        }),
        Step::WaitLong(
            "Find in Files lists the host's match in main.c",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).find_in_files())
                    .and_then(|v| v.read(cx).selected())
                    .is_some_and(|(p, r)| p.ends_with("main.c") && r.start == 0)
            }),
        ),
        // Detail 6: Go to File on the host.
        action("GotoFile"),
        act(|b, _, cx| {
            if let Some(v) = b.everywhere() {
                v.update(cx, |v, cx| v.set_query("App", cx));
            }
        }),
        Step::WaitLong(
            "Go to File lists the host file App.java",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).everywhere())
                    .is_some_and(|v| {
                        v.read(cx)
                            .results()
                            .rows
                            .iter()
                            .any(|r| r.path() == Some("App.java"))
                    })
            }),
        ),
        // Escape closes the popup, as the user does.
        act(|b, _, cx| {
            if let Some(v) = b.everywhere() {
                v.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::EverywhereEvent::Close)
                });
            }
        }),
        act(|b, window, cx| {
            if let Some(p) = b.remote_local_path("main.c") {
                b.open(&p, window, cx);
            }
        }),
        // Details 8–9: the host's language server.
        Step::WaitLong(
            "semantic colours of the remote C document come from the host",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).active_editor_entity())
                    .is_some_and(|e| {
                        let e = e.read(cx);
                        e.is_remote_document() && !e.semantic_spans().is_empty()
                    })
            }),
        ),
        caret_to("add(1", 4),
        action("ParameterInfo"),
        Step::WaitLong(
            "parameter info shows the host's signature",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).param_info())
                    .is_some_and(|(sigs, _)| sigs.iter().any(|s| s.contains("int a")))
            }),
        ),
        caret_to("add(1", 1),
        act(|b, window, cx| b.rename_symbol_at_caret("plus", window, cx)),
        Step::WaitLong(
            "rename on the host changes the definition and the call in the editor",
            long,
            Box::new(|_, cx| {
                remote_editor(cx).is_some_and(|(t, _)| {
                    t.contains("int plus(int a, int b)") && t.contains("plus(1, 2)")
                })
            }),
        ),
        // A misspelt call: the host's server offers its fix under ⌥⏎, applied on the host.
        caret_to("return x", 0),
        act(|b, _, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let at = e.caret();
                    e.replace_ranges(&[(at..at, "plux(3, 4);\n    ".into())], cx)
                });
            }
        }),
        Step::WaitLong(
            "the host's language server reports the misspelt call",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).active_editor_entity())
                    .is_some_and(|e| {
                        e.read(cx)
                            .problems()
                            .iter()
                            .any(|p| p.message.contains("plux"))
                    })
            }),
        ),
        caret_to("plux(", 2),
        action("ShowIntentionActions"),
        Step::WaitLong(
            "⌥⏎ lists the host server's fix for the misspelt call",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).intentions_popup())
                    .is_some_and(|p| p.read(cx).titles().iter().any(|t| t.contains("plus")))
            }),
        ),
        act(|b, _, cx| {
            if let Some(v) = b.intentions_popup() {
                let title = v
                    .read(cx)
                    .titles()
                    .iter()
                    .find(|t| t.contains("plus"))
                    .cloned();
                if let Some(title) = title {
                    v.update(cx, |v, cx| v.choose_title(&title, cx));
                }
            }
        }),
        Step::WaitLong(
            "applying it on the host corrects the call in the editor",
            long,
            Box::new(|_, cx| {
                remote_editor(cx)
                    .is_some_and(|(t, _)| t.contains("plus(3, 4);") && !t.contains("plux"))
            }),
        ),
        // Detail 7: Commit and Git on the host.
        action("ActivateCommitToolWindow"),
        Step::WaitLong(
            "the Commit window lists the host's changed main.c",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).commit_view())
                    .is_some_and(|v| {
                        let v = v.read(cx);
                        v.is_remote() && v.rows().iter().any(|r| r.label == "main.c")
                    })
            }),
        ),
        act(|b, _, cx| {
            if let Some(v) = b.commit_view() {
                v.update(cx, |v, cx| {
                    v.set_message("rename on the host", cx);
                    v.commit(cx);
                });
            }
        }),
        Step::WaitLong(
            "Commit saves the document on the host and commits there",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).commit_view())
                    .and_then(|v| v.read(cx).note().map(str::to_owned))
                    .is_some_and(|n| n.starts_with("Committed 1 file(s)"))
                    && folder().is_some_and(|f| {
                        std::fs::read_to_string(f.join("main.c"))
                            .is_ok_and(|t| t.contains("int plus("))
                    })
            }),
        ),
        action("ActivateVersionControlToolWindow"),
        Step::WaitLong(
            "the Git window shows the host's history with the new commit",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).git_log())
                    .is_some_and(|v| {
                        v.read(cx).summaries().first().map(String::as_str)
                            == Some("rename on the host")
                    })
            }),
        ),
        // Details 10–11: a program listening in the host terminal.
        action("ActivateTerminalToolWindow"),
        act(|b, _, cx| {
            let line = format!("python3 -m http.server {} --bind 127.0.0.1\n", port());
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text(&line));
            }
        }),
        Step::WaitLong(
            "the port the host program listens on is offered with Forward",
            long,
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).remote_port_offer())
                    .is_some_and(|(p, kind, _)| p == port() && kind == "service")
            }),
        ),
        act(|b, window, cx| {
            let p = port();
            b.remote_dismiss_offer(p);
            b.remote_forward_port(p, false, window, cx);
        }),
        Step::WaitLong(
            "Forward listens on loopback and the host program answers through it",
            long,
            Box::new(|_, cx| {
                let Some((host, folder)) =
                    remote_bench(cx).and_then(|b| b.read(cx).remote_project())
                else {
                    return false;
                };
                let forwards = idecraft_remote::ports::list(&host, &folder);
                let Some(local) = forwards
                    .iter()
                    .find(|f| f["remote_port"] == port())
                    .and_then(|f| f["local_port"].as_u64())
                else {
                    return false;
                };
                use std::io::{Read, Write};
                let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", local as u16)) else {
                    return false;
                };
                let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let _ = s.write_all(b"GET / HTTP/1.0\r\n\r\n");
                let mut body = String::new();
                let _ = s.read_to_string(&mut body);
                body.contains("200") && body.contains("main.c")
            }),
        ),
        // Detail 11: the Ports list shows the forward.
        act(|b, window, cx| b.remote_show_ports(window, cx)),
        Step::WaitFor(
            "Ports… lists the open forward with its loopback address",
            Box::new(|_, cx| {
                let want = format!("Port {} → 127.0.0.1:", port());
                remote_bench(cx)
                    .and_then(|b| b.read(cx).jvm_popup())
                    .is_some_and(|p| {
                        let p = p.read(cx);
                        p.title().contains("selftest-host")
                            && p.items()
                                .iter()
                                .any(|i| i.starts_with(&want) && i.ends_with("(forwarded)"))
                    })
            }),
        ),
        act(|b, _, cx| {
            if let Some(p) = b.jvm_popup() {
                p.update(cx, |_, cx| cx.emit(idecraft_workbench::ChoiceEvent::Close));
            }
        }),
        // Detail 10: a database port asks first; Cancel forwards nothing.
        act(|b, window, cx| b.remote_forward_port(27017, false, window, cx)),
        Step::WaitFor(
            "forwarding a database port warns first with Forward Anyway and Cancel, Cancel preselected",
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).jvm_popup())
                    .is_some_and(|p| {
                        let p = p.read(cx);
                        p.title().contains("looks like a database")
                            && p.items() == ["Forward Anyway", "Cancel"]
                            && p.selected() == 1
                    })
            }),
        ),
        act(|b, _, cx| {
            if let Some(p) = b.jvm_popup() {
                p.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::ChoiceEvent::Chosen(1))
                });
            }
        }),
        Step::WaitFor(
            "Cancel forwards nothing",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.jvm_popup().is_none()
                        && b.remote_project().is_some_and(|(host, folder)| {
                            !idecraft_remote::ports::list(&host, &folder)
                                .iter()
                                .any(|f| f["remote_port"] == 27017)
                        })
                })
            }),
        ),
        act(|b, window, cx| b.remote_forward_port(5005, false, window, cx)),
        Step::WaitFor(
            "a debug port is not forwarded",
            Box::new(|_, cx| status(cx).contains("debug port")),
        ),
        // SPEC-0318 v5 behaviour 42: the Ports tab of the Services window.
        act(|b, window, cx| b.show_remote_ports(window, cx)),
        Step::WaitFor(
            "the Services window lists the ports in its Ports tab",
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).services_view())
                    .is_some_and(|s| s.read(cx).port_rows().iter().any(|r| r.port == port()))
            }),
        ),
        // Behaviours 31 and 36: Fetch and the stash popup on the host repository.
        act(|b, _, cx| b.fetch_from_log(cx)),
        Step::WaitLong(
            "Fetch reports what it did on the host",
            long,
            Box::new(|_, cx| {
                let s = status(cx);
                s.contains("Fetch") && !s.ends_with('…')
            }),
        ),
        act(|b, window, cx| b.open_stashes(window, cx)),
        Step::WaitFor(
            "the stash popup opens on the host's stashes",
            Box::new(|_, cx| remote_bench(cx).is_some_and(|b| b.read(cx).stash_popup().is_some())),
        ),
        act(|b, _, cx| {
            if let Some(p) = b.stash_popup() {
                p.update(cx, |_, cx| cx.emit(idecraft_workbench::StashEvent::Close));
            }
        }),
        // Detail 13: what agents see.
        Step::Expect(
            "agents see the window's host terminal, its run and its forward",
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).remote_live())
                    .is_some_and(|l| {
                        let d = l.describe();
                        d["terminals"].as_array().is_some_and(|t| !t.is_empty())
                            && d["runs"].to_string().contains("hello")
                            && d["forwards"].as_array().is_some_and(|f| !f.is_empty())
                    })
            }),
        ),
        type_in_terminal("\u{3}"),
        Step::Act(Box::new(|_, _, cx| {
            if let Some(live) = remote_bench(cx).and_then(|b| b.read(cx).remote_live()) {
                std::thread::spawn(move || {
                    let _ = live.close(idecraft_remote::Unsaved::Discard);
                });
            }
        })),
        Step::WaitFor(
            "the remote project with tool windows closes, its forwards with it",
            Box::new(|_, cx| remote_bench(cx).is_none()),
        ),
        Step::Act(Box::new(|_, _, _| {
            REMOTE_WINDOW.set(None);
            if let Some(f) = folder().and_then(|f| f.parent().map(Path::to_path_buf)) {
                let _ = std::fs::remove_dir_all(f);
            }
        })),
    ]
}

fn remote_project_steps() -> Vec<Step> {
    use std::sync::Mutex;
    static HOST: Mutex<Option<(idecraft_remote::inprocess::HostSwitch, PathBuf)>> =
        Mutex::new(None);
    fn folder() -> Option<PathBuf> {
        HOST.lock().ok()?.as_ref().map(|h| h.1.clone())
    }
    fn reachable(on: bool) {
        if let Ok(h) = HOST.lock()
            && let Some((s, _)) = h.as_ref()
        {
            s.set_reachable(on);
        }
    }
    fn host_text(name: &str) -> Option<String> {
        std::fs::read_to_string(folder()?.join(name)).ok()
    }
    fn live(cx: &App) -> Option<idecraft_remote::LiveProject> {
        remote_bench(cx)?.read(cx).remote_live()
    }
    fn widget(cx: &App) -> String {
        remote_bench(cx)
            .and_then(|b| b.read(cx).remote_widget_text())
            .map(|(t, _)| t)
            .unwrap_or_default()
    }
    fn status(cx: &App) -> String {
        remote_bench(cx)
            .and_then(|b| b.read(cx).status_message(cx))
            .unwrap_or_default()
    }
    fn type_at(at: usize, text: &'static str) -> Step {
        Step::Act(Box::new(move |_, _, cx| {
            in_remote_window(cx, |b, _, cx| {
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.replace_ranges(&[(at..at, text.into())], cx));
                }
            });
        }))
    }
    fn save() -> Step {
        Step::Act(Box::new(|_, _, cx| {
            in_remote_window(cx, |b, _, cx| {
                if let Some(e) = b.active_editor_entity() {
                    e.update(cx, |e, cx| e.run_action("SaveAll", cx));
                }
            });
        }))
    }
    let long = std::time::Duration::from_secs(90);
    vec![
        // Remote Development lists the recent remote projects before the hosts.
        Step::Act(Box::new(|b, window, cx| {
            let dir = std::env::temp_dir().join(format!("idc-rproj-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(dir.join("remote-app/src"));
            let dir = dir.canonicalize().unwrap_or(dir).join("remote-app");
            let _ = std::fs::write(dir.join("notes.txt"), "remote notes\n");
            let _ = std::fs::write(dir.join("src/main.rs"), "fn main() {}\n");
            if let Some(base) = FIXTURE.get().map(|f| f.join(".idecraft/remote")) {
                let _ = idecraft_remote::remember_remote(
                    &idecraft_remote::recent_remote_file(&base),
                    "idecraft-test",
                    &dir.display().to_string(),
                );
            }
            // The host list of the self-test's window: an empty config of its own, never the
            // user's `~/.ssh/config`.
            if let Some(parent) = dir.parent() {
                b.set_remote_ssh(idecraft_remote::testing::empty_ssh_config(parent));
            }
            if let Ok(mut h) = HOST.lock() {
                *h = Some((idecraft_remote::inprocess::HostSwitch::default(), dir));
            }
            b.open_remote_development(window, cx);
        })),
        Step::Expect(
            "Remote Development lists the recent remote project first",
            Box::new(|b, cx| {
                b.remote_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.project_mode()
                        && v.texts()
                            .first()
                            .is_some_and(|t| t.starts_with("Recent: remote-app — idecraft-test"))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| v.set_project_mode(false, Vec::new(), cx));
            }
            b.toggle_remote(window, cx);
            let switch = folder().and_then(|f| open_test_remote_project(&f, cx));
            if let (Ok(mut h), Some(s)) = (HOST.lock(), switch)
                && let Some(entry) = h.as_mut()
            {
                entry.0 = s;
            }
        })),
        Step::WaitFor(
            "the remote project opens in a window of its own with the host in title and status bar",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    let b = b.read(cx);
                    b.remote_title().as_deref() == Some("remote-app — selftest-host")
                        && widget(cx).starts_with("selftest-host")
                        && !widget(cx).contains("Reconnecting")
                })
            }),
        ),
        Step::WaitFor(
            "the project tree lists the host folder through the session",
            Box::new(|_, cx| {
                remote_bench(cx).is_some_and(|b| {
                    b.read(cx).tree_row_names() == ["remote-app", "src", "notes.txt"]
                })
            }),
        ),
        Step::Act(Box::new(|_, _, cx| {
            in_remote_window(cx, |b, window, cx| {
                if let Some(p) = b.remote_local_path("notes.txt") {
                    b.open(&p, window, cx);
                }
            });
        })),
        Step::WaitFor(
            "a host file opens as a remote document, unmodified",
            Box::new(|_, cx| remote_editor(cx) == Some(("remote notes\n".into(), false))),
        ),
        type_at(0, "x"),
        Step::WaitFor(
            "typing changes the window at once and the edit reaches the backend",
            Box::new(|_, cx| {
                remote_editor(cx) == Some(("xremote notes\n".into(), true))
                    && live(cx).is_some_and(|l| l.pending_edits() == Some(0))
                    && host_text("notes.txt").as_deref() == Some("remote notes\n")
            }),
        ),
        save(),
        Step::WaitFor(
            "saving writes the file on the host and nothing on this machine",
            Box::new(|_, cx| {
                status(cx) == "Saved on host"
                    && host_text("notes.txt").as_deref() == Some("xremote notes\n")
                    && remote_editor(cx).is_some_and(|(_, m)| !m)
                    && remote_bench(cx)
                        .and_then(|b| b.read(cx).remote_local_path("notes.txt"))
                        .is_some_and(|p| !p.exists())
            }),
        ),
        // The network breaks and the host stays away for a while.
        Step::Act(Box::new(|_, _, cx| {
            reachable(false);
            if let Some(l) = live(cx) {
                l.break_connection();
            }
        })),
        Step::WaitFor(
            "after a break the widget shows Reconnecting",
            Box::new(|_, cx| widget(cx).contains("Reconnecting")),
        ),
        type_at(0, "y"),
        save(),
        Step::WaitFor(
            "typing goes on while disconnected and the save waits, said clearly",
            Box::new(|_, cx| {
                remote_editor(cx) == Some(("yxremote notes\n".into(), true))
                    && status(cx).starts_with("Not saved — reconnecting to selftest-host")
                    && widget(cx).contains("save pending")
                    && host_text("notes.txt").as_deref() == Some("xremote notes\n")
            }),
        ),
        Step::Act(Box::new(|_, _, _| reachable(true))),
        Step::WaitLong(
            "the window reconnects by itself, the waiting save is carried out, no text lost",
            long,
            Box::new(|_, cx| {
                !widget(cx).contains("Reconnecting")
                    && !widget(cx).contains("save pending")
                    && host_text("notes.txt").as_deref() == Some("yxremote notes\n")
                    && remote_editor(cx) == Some(("yxremote notes\n".into(), false))
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(f) = folder() {
                let _ = std::fs::write(f.join("notes.txt"), "changed on the host\n");
                let _ = std::fs::write(f.join("created.rs"), "fn c() {}\n");
            }
        })),
        Step::WaitFor(
            "a host change reloads the unmodified file and a new host file appears in the tree",
            Box::new(|_, cx| {
                remote_editor(cx) == Some(("changed on the host\n".into(), false))
                    && remote_bench(cx).is_some_and(|b| {
                        b.read(cx)
                            .tree_row_names()
                            .iter()
                            .any(|n| n == "created.rs")
                    })
            }),
        ),
        // An unsaved edit, then the window "crashes": it goes without closing its project.
        type_at(0, "z"),
        Step::WaitFor(
            "the unsaved edit is in the session",
            Box::new(|_, cx| live(cx).is_some_and(|l| l.unsaved() == ["notes.txt"])),
        ),
        Step::Act(Box::new(|_, _, cx| {
            if let Some(h) = REMOTE_WINDOW.take() {
                let _ = h.update(cx, |_, window, _| window.remove_window());
            }
        })),
        Step::WaitFor(
            "the crashed window is gone",
            Box::new(|_, cx| remote_bench(cx).is_none()),
        ),
        Step::Act(Box::new(|_, _, cx| {
            if let Some(f) = folder() {
                let _ = open_test_remote_project(&f, cx);
            }
        })),
        Step::WaitFor(
            "opening the project again offers the recovery copy",
            Box::new(|_, cx| {
                remote_bench(cx)
                    .and_then(|b| b.read(cx).jvm_popup())
                    .is_some_and(|p| {
                        let p = p.read(cx);
                        p.title().contains("notes.txt")
                            && p.items()
                                .first()
                                .is_some_and(|i| i.starts_with("Restore 1"))
                    })
            }),
        ),
        Step::Act(Box::new(|_, _, cx| {
            if let Some(p) = remote_bench(cx).and_then(|b| b.read(cx).jvm_popup()) {
                p.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::ChoiceEvent::Chosen(0))
                });
            }
        })),
        Step::WaitFor(
            "restoring opens the file with the unsaved text of the crashed window",
            Box::new(|_, cx| remote_editor(cx) == Some(("zchanged on the host\n".into(), true))),
        ),
        // An agent closes the project (`remote_project_close`, discard): the window goes.
        Step::Act(Box::new(|_, _, cx| {
            if let Some(l) = live(cx) {
                let _ = l.close(idecraft_remote::Unsaved::Discard);
            }
        })),
        Step::WaitFor(
            "closing the remote project closes its window",
            Box::new(|_, cx| remote_bench(cx).is_none()),
        ),
        Step::Act(Box::new(|_, _, _| {
            REMOTE_WINDOW.set(None);
            if let Some(f) = folder().and_then(|f| f.parent().map(Path::to_path_buf)) {
                let _ = std::fs::remove_dir_all(f);
            }
        })),
    ]
}

/// Remote host (SPEC-0121) against a real `sshd` started for the test: connecting, going to a
/// folder, opening a file, saving it back, a change on the host refused once, an SSH terminal in
/// the folder, back to the host list.
fn remote_steps() -> Vec<Step> {
    use std::sync::Mutex;
    static HOST: Mutex<Option<idecraft_remote::testing::TestHost>> = Mutex::new(None);
    let save = if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    };
    fn project() -> Option<std::path::PathBuf> {
        HOST.lock().ok()?.as_ref().map(|h| h.dir.join("project"))
    }
    let press = |keys: &'static [&'static str]| {
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    for k in keys {
                        v.press(k, cx);
                    }
                });
            }
        }))
    };
    let place = |b: &WorkbenchView, cx: &gpui::App| -> Option<(String, String)> {
        b.remote_view()
            .and_then(|v| v.read(cx).place().cloned())
            .filter(|_| !b.remote_view().is_some_and(|v| v.read(cx).busy()))
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            let dir = std::path::PathBuf::from(format!("/tmp/idc-remote-{}", std::process::id()));
            let host = idecraft_remote::testing::TestHost::start(&dir);
            if let Some(h) = &host {
                let project = h.dir.join("project");
                let _ = std::fs::create_dir_all(project.join("src"));
                let _ = std::fs::write(project.join("notes.txt"), "remote notes\n");
                b.set_remote_ssh(h.target.extra.clone());
            } else {
                // No test host: the host list still never reads the user's `~/.ssh/config`.
                b.set_remote_ssh(idecraft_remote::testing::empty_ssh_config(&dir));
            }
            if let Ok(mut slot) = HOST.lock() {
                *slot = host;
            }
            b.toggle_remote(window, cx);
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field("idecraft-test", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "Enter on a host connects over ssh and shows the home folder",
            Box::new(move |b, cx| {
                place(b, cx).is_some_and(|(host, path)| {
                    host == "idecraft-test"
                        && std::env::var("HOME")
                            .ok()
                            .and_then(|h| std::path::Path::new(&h).canonicalize().ok())
                            .is_some_and(|h| h.display().to_string() == path)
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let (Some(v), Some(p)) = (b.remote_view(), project()) {
                v.update(cx, |v, cx| {
                    v.type_field(&p.display().to_string(), cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "a path typed into the field is shown with its folders first",
            Box::new(move |b, cx| {
                let texts = b
                    .remote_view()
                    .map(|v| v.read(cx).texts())
                    .unwrap_or_default();
                place(b, cx).is_some_and(|(_, path)| {
                    project().is_some_and(|p| p.display().to_string() == path)
                }) && texts == ["..", "src/", "notes.txt"]
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.select_text("notes.txt");
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "Enter opens the host's file in the editor through a local copy",
            Box::new(|b, cx| {
                b.active_text(cx).as_deref() == Some("remote notes\n")
                    && b.active_file().is_some_and(|f| {
                        b.remote_file_of(&f).is_some_and(|(host, path)| {
                            host == "idecraft-test" && path.ends_with("/project/notes.txt")
                        })
                    })
            }),
        ),
        Step::Key("x", Some("x")),
        Step::Key(save, None),
        Step::WaitFor(
            "saving sends the file back to the host",
            Box::new(|b, cx| {
                b.status_message(cx)
                    .is_some_and(|s| s.starts_with("Uploaded to idecraft-test:"))
                    && project().is_some_and(|p| {
                        std::fs::read_to_string(p.join("notes.txt")).ok().as_deref()
                            == Some("xremote notes\n")
                    })
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(p) = project() {
                let _ = std::fs::write(p.join("notes.txt"), "changed on the host\n");
            }
        })),
        Step::Key("y", Some("y")),
        Step::Key(save, None),
        Step::WaitFor(
            "a file changed on the host meanwhile is not overwritten by the first save",
            Box::new(|b, cx| {
                b.status_message(cx)
                    .is_some_and(|s| s.contains("changed on the host since it was opened"))
                    && project().is_some_and(|p| {
                        std::fs::read_to_string(p.join("notes.txt")).ok().as_deref()
                            == Some("changed on the host\n")
                    })
            }),
        ),
        Step::Key(save, None),
        Step::WaitFor(
            "saving again overwrites it",
            Box::new(|b, cx| {
                b.status_message(cx)
                    .is_some_and(|s| s.starts_with("Uploaded to"))
                    && project().is_some_and(|p| {
                        std::fs::read_to_string(p.join("notes.txt")).ok().as_deref()
                            == Some("xyremote notes\n")
                    })
            }),
        ),
        // SPEC-0253: New File opens the new file, Rename renames it, Delete asks, then removes it.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.press("new-file", cx);
                    v.type_field("made.txt", cx);
                    v.press("enter", cx);
                });
            }
        })),
        // The file opens and the folder is read again independently; wait for both, so Rename
        // below acts on the new entry and not on whatever the stale list had selected.
        Step::WaitFor(
            "New File creates the file on the host, opens it and selects it in the list",
            Box::new(|b, cx| {
                project().is_some_and(|p| p.join("made.txt").is_file())
                    && b.active_file().is_some_and(|f| {
                        b.remote_file_of(&f)
                            .is_some_and(|(_, path)| path.ends_with("/project/made.txt"))
                    })
                    && b.remote_view()
                        .is_some_and(|v| v.read(cx).selected_text().as_deref() == Some("made.txt"))
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    if v.select_text("made.txt") {
                        v.press("rename", cx);
                        v.type_field("renamed.txt", cx);
                        v.press("enter", cx);
                    }
                });
            }
        })),
        Step::WaitFor(
            "Rename renames it on the host and selects the new name",
            Box::new(move |b, cx| {
                project().is_some_and(|p| {
                    p.join("renamed.txt").is_file() && !p.join("made.txt").exists()
                }) && project().is_some_and(|p| p.join("notes.txt").is_file())
                    && b.remote_view().is_some_and(|v| {
                        v.read(cx).selected_text().as_deref() == Some("renamed.txt")
                    })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    if v.select_text("renamed.txt") {
                        v.press("delete", cx);
                    }
                });
            }
        })),
        Step::WaitFor(
            "Delete asks first",
            Box::new(|b, cx| {
                b.remote_view().is_some_and(|v| {
                    matches!(v.read(cx).pending(), Some(idecraft_workbench::RemotePending::Delete(p, 0)) if p.ends_with("/renamed.txt"))
                })
            }),
        ),
        press(&["enter"]),
        Step::WaitFor(
            "Enter deletes it on the host",
            Box::new(move |b, cx| {
                project().is_some_and(|p| !p.join("renamed.txt").exists())
                    && b.remote_view().is_some_and(|v| {
                        let v = v.read(cx);
                        v.pending().is_none() && v.texts() == ["..", "src/", "notes.txt"]
                    })
            }),
        ),
        // IDECraft's backend on the host (SPEC-0158): started for the folder shown (this build is
        // uploaded to a test folder), `?text` searches the host's project, Enter opens the hit.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(p) = project() {
                let _ = std::fs::write(p.join("src/tool.py"), "def needle_fn():\n    return 1\n");
                let _ = std::fs::create_dir_all(p.join(".idecraft"));
                let _ = std::fs::write(
                    p.join(".idecraft/run.toml"),
                    "[[run]]\nname = \"hello\"\ncommand = [\"sh\", \"-c\", \"echo hello-from-host\"]\n",
                );
            }
            b.set_remote_backend_dir(&format!(".cache/idecraft-selftest-{}", std::process::id()));
            if let Some(v) = b.remote_view() {
                v.update(cx, |_, cx| {
                    cx.emit(idecraft_workbench::RemoteEvent::StartBackend)
                });
            }
        })),
        Step::WaitLong(
            "Start Backend runs IDECraft's backend on the host for the folder",
            std::time::Duration::from_secs(120),
            Box::new(|b, cx| {
                b.remote_view().is_some_and(|v| {
                    v.read(cx).backend().is_some_and(|(name, folder)| {
                        name.starts_with("idecraft") && folder.ends_with("/project")
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field("?remote", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "?remote searches the host's project through the backend",
            Box::new(|b, cx| {
                b.remote_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.texts() == ["notes.txt:1  xyremote notes"]
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field("", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "Enter on a hit opens the host's file",
            Box::new(|b, cx| {
                b.active_text(cx).as_deref() == Some("xyremote notes\n")
                    && b.active_file()
                        .is_some_and(|f| b.remote_file_of(&f).is_some())
            }),
        ),
        // SPEC-0158 v2: symbols, the host-side check and run configurations through the backend.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field("@needle", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "@needle finds the function in the host's project",
            Box::new(|b, cx| {
                b.remote_view().is_some_and(|v| {
                    v.read(cx)
                        .texts()
                        .iter()
                        .any(|t| t.starts_with("tool.py:1  needle_fn"))
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field("", cx);
                    v.select_text("tool.py:1");
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitLong(
            "opening the hit lets the backend check the file on the host",
            std::time::Duration::from_secs(90),
            Box::new(|b, cx| {
                b.status_message(cx).is_some_and(|s| {
                    s.contains("problems on the host in tool.py")
                        || s.starts_with("Host check of tool.py")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field(">", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "> lists the host project's run configurations",
            Box::new(|b, cx| {
                b.remote_view()
                    .is_some_and(|v| v.read(cx).texts() == ["▶ hello  sh -c echo hello-from-host"])
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.type_field("", cx);
                    v.press("enter", cx);
                });
            }
        })),
        Step::WaitFor(
            "Enter runs it on the host in an SSH terminal tab",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    let text = t.read(cx).history_text();
                    text.lines().any(|l| l.trim() == "hello-from-host")
                        && text.contains("Process finished with exit code 0")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            let last = b.terminal_tabs().len().saturating_sub(1);
            b.close_terminal_tab(last, window, cx);
            if let Some(v) = b.remote_view() {
                v.update(cx, |v, cx| {
                    v.press("escape", cx);
                });
            }
        })),
        Step::Act(Box::new(|_, _, _| {
            if let Some(home) = std::env::var_os("HOME") {
                let _ = std::fs::remove_dir_all(
                    std::path::Path::new(&home)
                        .join(format!(".cache/idecraft-selftest-{}", std::process::id())),
                );
            }
        })),
        Step::Act(Box::new(|b, window, cx| {
            b.remote_terminal(window, cx);
        })),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text("pwd\r"));
            }
        })),
        Step::WaitFor(
            "SSH Terminal logs in to the host in the folder shown",
            Box::new(|b, cx| {
                // The whole history: with many bottom windows open the terminal may show only
                // its prompt row (SPEC-0167 v2 behaviour 10).
                b.terminal().is_some_and(|t| {
                    let text = t.read(cx).history_text();
                    project()
                        .is_some_and(|p| text.lines().any(|l| l.trim() == p.display().to_string()))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            let last = b.terminal_tabs().len().saturating_sub(1);
            b.close_terminal_tab(last, window, cx);
            // The terminal replaced the Remote Host window at the bottom (SPEC-0061 v10
            // behaviour 49); it is shown again first.
            if b.remote_view().is_none() {
                b.toggle_remote(window, cx);
            }
            if let Some(v) = b.remote_view() {
                v.update(cx, |_, cx| cx.emit(idecraft_workbench::RemoteEvent::Hosts));
            }
        })),
        Step::Expect(
            "Hosts leaves the host",
            Box::new(|b, cx| {
                b.remote_view()
                    .is_some_and(|v| v.read(cx).place().is_none())
            }),
        ),
        press(&["escape"]),
        Step::Expect(
            "Escape hides the window",
            Box::new(|b, _| b.remote_view().is_none()),
        ),
        Step::Act(Box::new(|b, _, _| {
            b.set_remote_ssh(Vec::new());
            if let Ok(mut slot) = HOST.lock()
                && let Some(h) = slot.take()
            {
                let dir = h.dir.clone();
                drop(h);
                let _ = std::fs::remove_dir_all(dir);
            }
            let _ =
                std::fs::remove_dir_all(idecraft_remote::cache_dir().join("remote/idecraft-test"));
        })),
    ]
}

/// More dialects (SPEC-0116): the connection dialog offers SQL Server; a test against a closed
/// port goes through the T-SQL driver and reports where it tried; Escape closes the dialog.
fn dialect_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_dialect(idecraft_workbench::Dialect::SqlServer, cx);
                    v.set_field("Name", "shop", cx);
                    v.set_field("Host", "127.0.0.1", cx);
                    v.set_field("Port", "1", cx);
                    v.set_field("User", "sa", cx);
                    v.test(cx);
                });
            }
        })),
        Step::WaitFor(
            "Test Connection for SQL Server reports the closed port",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.form(cx).dialect == idecraft_workbench::Dialect::SqlServer
                        && v.note()
                            .is_some_and(|(n, error)| *error && n.contains("127.0.0.1:1"))
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the dialog without saving",
            Box::new(|b, _| b.connection_dialog().is_none()),
        ),
        // SPEC-0135: Oracle is offered; testing says that no driver is included yet.
        Step::Act(Box::new(|b, window, cx| {
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_dialect(idecraft_workbench::Dialect::Oracle, cx);
                    v.set_field("Name", "ora", cx);
                    v.set_field("Host", "127.0.0.1", cx);
                    v.set_field("Port", "1", cx);
                    v.set_field("User", "scott", cx);
                    v.test(cx);
                });
            }
        })),
        Step::WaitFor(
            "Test Connection for Oracle reports the closed port through Instant Client",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    v.read(cx)
                        .note()
                        .is_some_and(|(n, error)| *error && n.contains("127.0.0.1:1"))
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the Oracle dialog",
            Box::new(|b, _| b.connection_dialog().is_none()),
        ),
    ]
}

/// The New Data Source list (SPEC-0300 AK-7): typing filters by name, Enter picks TiDB with its
/// family and default port, a Beta system shows its badge, Escape closes the list and then the dialog.
fn connector_steps() -> Vec<Step> {
    let items = |b: &WorkbenchView, cx: &gpui::App| -> Vec<(String, Option<&'static str>)> {
        b.connection_dialog().map_or_else(Vec::new, |v| {
            v.read(cx)
                .picker_items(cx)
                .0
                .into_iter()
                .map(|c| (c.id, c.status.badge()))
                .collect()
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_picker(window, cx);
                    v.set_picker_query("tidb", cx);
                });
            }
        })),
        Step::Expect(
            "typing tidb leaves only TiDB in the system list",
            Box::new(move |b, cx| {
                let found = items(b, cx);
                found.len() == 1 && found[0].0 == "tidb"
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "Enter picks TiDB: MySQL family, list closed, port hint 4000",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    let form = v.form(cx);
                    !v.picker_open()
                        && form.dialect == idecraft_workbench::Dialect::MySql
                        && form.system.as_deref() == Some("tidb")
                        && v.connector()
                            .is_some_and(|c| c.connection.default_port == Some(4000))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_picker(window, cx);
                    v.set_picker_query("redshift", cx);
                });
            }
        })),
        Step::Expect(
            "Amazon Redshift is listed with the Beta badge",
            Box::new(move |b, cx| items(b, cx) == [("redshift".to_owned(), Some("Beta"))]),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the list and keeps the dialog with TiDB",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.picker_open() && v.form(cx).system.as_deref() == Some("tidb")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "a second Escape closes the dialog",
            Box::new(|b, _| b.connection_dialog().is_none()),
        ),
    ]
}

/// "Report a problem with this database" (SPEC-0301 AK-5): a Beta system offers the link, the
/// report opens with host names replaced, Copy says so, Escape closes the report and then the dialog.
fn report_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_system("redshift", cx);
                    v.set_field("Host", "cluster7.example.internal", cx);
                });
            }
        })),
        Step::Expect(
            "a Beta system offers Report a problem with this database",
            Box::new(|b, cx| {
                b.connection_dialog()
                    .is_some_and(|v| v.read(cx).offers_report())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| v.report(cx));
            }
        })),
        Step::Expect(
            "the report opens about Amazon Redshift without the host name",
            Box::new(|b, cx| {
                b.report_dialog().is_some_and(|r| {
                    let text = r.read(cx).report(cx).text();
                    text.starts_with("[Amazon Redshift]")
                        && text.contains("status beta")
                        && !text.contains("cluster7")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(r) = b.report_dialog() {
                r.update(cx, |r, cx| r.copy(cx));
            }
        })),
        Step::Expect(
            "Copy puts the report on the clipboard and says so",
            Box::new(|b, cx| {
                b.report_dialog().is_some_and(|r| {
                    r.read(cx).note().is_some_and(|n| n.starts_with("Copied"))
                        && cx
                            .read_from_clipboard()
                            .and_then(|c| c.text())
                            .is_some_and(|t| t.starts_with("[Amazon Redshift]"))
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the report and keeps the connection dialog",
            Box::new(|b, _| b.report_dialog().is_none() && b.connection_dialog().is_some()),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "a second Escape closes the connection dialog",
            Box::new(|b, _| b.connection_dialog().is_none()),
        ),
    ]
}

/// Non-relational stores (SPEC-0310 AK-9): Redis picked in the New Data Source list is saved as a
/// store in `.idecraft/stores.toml` (no password in the file), shown in the Database tool window
/// with its system and address, and expanding it against a closed port shows the reason. The
/// store file is removed again so later steps see only SQL connections.
fn store_steps() -> Vec<Step> {
    let row = |b: &WorkbenchView, cx: &gpui::App| -> Option<(String, String)> {
        b.database_view().and_then(|v| {
            v.read(cx)
                .visible()
                .into_iter()
                .find(|(_, n, _)| n == "cache")
                .map(|(_, n, d)| (n, d))
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if b.database_view().is_none() {
                b.toggle_database(window, cx);
            }
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.toggle_picker(window, cx);
                    v.set_picker_query("redis", cx);
                });
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "Enter picks Redis, a key-value store, from the system list",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.form(cx).system.as_deref() == Some("redis")
                        && v.connector().is_some_and(|c| c.is_store())
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_field("Name", "cache", cx);
                    v.set_field("Host", "127.0.0.1", cx);
                    v.set_field("Port", "1", cx);
                    v.set_field("Database", "2", cx);
                    v.save(cx);
                });
            }
        })),
        // Saving runs off the window since #414 (no wait while the keychain asks), so this waits.
        Step::WaitFor(
            "saving writes a store profile and the tool window shows it with system and address",
            Box::new(move |b, cx| {
                let file = FIXTURE
                    .get()
                    .and_then(|d| std::fs::read_to_string(d.join(".idecraft/stores.toml")).ok())
                    .unwrap_or_default();
                // New forms check certificates (SPEC-0108 v4 AK-12, #414), so Redis is saved
                // with TLS.
                b.connection_dialog().is_none()
                    && file.contains("url = \"rediss://127.0.0.1:1/2\"")
                    && row(b, cx).is_some_and(|(_, d)| d == "Redis · 127.0.0.1:1/2 · TLS")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    if v.select_label("cache", cx) {
                        v.expand(
                            idecraft_workbench::db_tree_model::Node::Store(0, Vec::new()),
                            cx,
                        );
                    }
                });
            }
        })),
        Step::WaitFor(
            "expanding the store against a closed port says why it could not be read",
            Box::new(|b, cx| {
                // The reason row drops the `connection … (address):` prefix like SQL rows do; the
                // address stays on the store row above it.
                b.database_view().is_some_and(|v| {
                    let rows = v.read(cx).visible();
                    rows.iter()
                        .position(|(_, n, d)| n == "cache" && d.contains("127.0.0.1:1"))
                        .and_then(|i| rows.get(i + 1))
                        .is_some_and(|(depth, n, _)| {
                            *depth == 1 && n.starts_with("Could not read it")
                        })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join(".idecraft/stores.toml"));
            }
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.reload();
                    cx.notify();
                });
            }
        })),
        Step::Expect(
            "without the store file the store row is gone",
            Box::new(move |b, cx| row(b, cx).is_none()),
        ),
    ]
}

/// "Import database system…" (SPEC-0302 AK-5): a definition file is loaded and reviewed with its
/// queries, Enter saves it with the Imported badge and selects it, Remove takes it out again. The
/// system has an id of its own, so the user's connectors are never touched.
fn connector_import_steps() -> Vec<Step> {
    const ID: &str = "idecraft-selftest-demo";
    fn file() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{ID}.idcconnector"))
    }
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = idecraft_workbench::database::connector::connectors_dir() {
                let _ = std::fs::remove_file(dir.join(format!("{ID}.idcconnector")));
            }
            idecraft_workbench::database::reload_catalogue();
            let _ = std::fs::write(
                file(),
                format!(
                    "[idecraft]\nkind = \"connector\"\nformat = 1\nid = \"{ID}\"\nname = \"Self-test Demo DB\"\n\n\
                     [connector]\nfamily = \"postgresql\"\ncategory = \"other\"\n\n\
                     [connection]\nschemes = [\"idecraftselftest\"]\ndefault_port = 6001\n\n\
                     [introspection]\nschemas = \"select nspname, '0' from pg_namespace\"\n"
                ),
            );
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| v.open_import(window, cx));
                if let Some(panel) = v.read(cx).import_panel() {
                    panel.update(cx, |p, cx| {
                        p.set_source(&file().display().to_string(), cx);
                        p.load(cx);
                    });
                }
            }
        })),
        Step::WaitFor(
            "the import shows the definition and the queries it will run",
            Box::new(|b, cx| {
                b.connection_dialog()
                    .and_then(|v| v.read(cx).import_panel())
                    .is_some_and(|p| {
                        p.read(cx).review().is_some_and(|r| {
                            r.ok()
                                && r.queries
                                    .first()
                                    .is_some_and(|q| q.0 == "introspection.schemas")
                        })
                    })
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "Enter saves it; the dialog shows the system with the Imported badge",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.import_panel().is_none()
                        && v.connector().is_some_and(|c| {
                            c.id == ID
                                && c.status
                                    == idecraft_workbench::database::connector::Status::Imported
                        })
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| v.remove_system(cx));
            }
            let _ = std::fs::remove_file(file());
        })),
        Step::Expect(
            "Remove takes the imported system out of the catalogue",
            Box::new(|_, _| idecraft_workbench::database::catalogue().get(ID).is_none()),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the connection dialog",
            Box::new(|b, _| b.connection_dialog().is_none()),
        ),
    ]
}

/// The warning before a connector plugin first connects (SPEC-0314 AK-5): Test Connection on a
/// plugin connector shows plugin, author, host and port, status and that IDECraft does not review
/// its code, with Cancel selected; Escape cancels with a note; Tab and Enter allow, store the
/// user's grant and run the test; a second test does not ask again.
fn plugin_network_steps() -> Vec<Step> {
    const ID: &str = "idecraft-selftest-net";
    const MODULE: &str = "selftestnet";
    fn plugins() -> Option<std::path::PathBuf> {
        FIXTURE.get().map(|d| d.join(".plugins-net"))
    }
    fn target() -> idecraft_workbench::plugins::Target {
        idecraft_workbench::plugins::Target::new("127.0.0.1", 6553)
    }
    fn note_has(b: &WorkbenchView, cx: &gpui::App, text: &str, error: bool) -> bool {
        b.connection_dialog().is_some_and(|v| {
            v.read(cx)
                .note()
                .is_some_and(|(n, e)| *e == error && n.contains(text))
        })
    }
    // A connector plugin that answers without connecting anywhere.
    let manifest = r#"{"name":"Self-test Net","version":"1.0","author":"IDECraft self-test","permissions":["profile-network"],"actions":[{"id":"connect","title":"Connect","kind":"connector"}]}"#;
    let answer = r#"{"ok":true,"note":"Plugin answered"}"#;
    let module = format!(
        r#"(module
  (memory (export "memory") 1)
  (global $next (mut i32) (i32.const 2048))
  (data (i32.const 0) "{m}")
  (data (i32.const 1024) "{a}")
  (func (export "idecraft_alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $n)))
    (local.get $p))
  (func (export "idecraft_manifest") (result i64) (i64.const {ml}))
  (func (export "idecraft_run") (param i32 i32 i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {al}))))"#,
        m = manifest.replace('"', "\\\""),
        a = answer.replace('"', "\\\""),
        ml = manifest.len(),
        al = answer.len(),
    );
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            let Some(dir) = plugins() else {
                return;
            };
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(format!("{MODULE}.wasm")), &module);
            let _ = idecraft_workbench::plugins::set_granted(
                &dir,
                MODULE,
                idecraft_workbench::plugins::Permission::ProfileNetwork,
                true,
            );
            b.set_plugins_dir(dir);
            if let Some(connectors) = idecraft_workbench::database::connector::connectors_dir() {
                let _ = std::fs::create_dir_all(&connectors);
                let _ = std::fs::write(
                    connectors.join(format!("{ID}.toml")),
                    format!(
                        "format = 1\nid = \"{ID}\"\nname = \"Self-test Net DB\"\nfamily = \"plugin\"\n\
                         category = \"other\"\n[plugin]\nmodule = \"{MODULE}\"\n\
                         [connection]\nschemes = [\"idcselftestnet\"]\ndefault_port = 6553\n"
                    ),
                );
            }
            idecraft_workbench::database::reload_catalogue();
            b.open_connection_dialog(None, window, cx);
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_system(ID, cx);
                    v.set_field("Host", "127.0.0.1", cx);
                    v.test(cx);
                });
            }
        })),
        Step::Expect(
            "Test Connection on a plugin connector shows the warning with Cancel selected",
            Box::new(|b, cx| {
                b.plugin_network_dialog().is_some_and(|d| {
                    let d = d.read(cx);
                    let w = d.warning();
                    !d.allow_selected()
                        && w.plugin == "Self-test Net"
                        && w.author == "IDECraft self-test"
                        && w.host.as_deref() == Some("127.0.0.1")
                        && w.port == Some(6553)
                        && w.status == "user"
                        && w.lines[0].contains("127.0.0.1:6553")
                        && w.text
                            .contains("IDECraft does not review the code of plugins")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape cancels: no grant, the note says the plugin was not allowed",
            Box::new(|b, cx| {
                b.plugin_network_dialog().is_none()
                    && note_has(b, cx, "was not allowed to connect", true)
                    && plugins().is_some_and(|d| {
                        !idecraft_workbench::plugins::network_granted(
                            &d,
                            MODULE,
                            &target(),
                            idecraft_workbench::plugins::Holder::User,
                        )
                    })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| v.test(cx));
            }
        })),
        Step::Expect(
            "a second Test asks again",
            Box::new(|b, _| b.plugin_network_dialog().is_some()),
        ),
        Step::Key("tab", None),
        Step::Expect(
            "Tab selects Allow and connect",
            Box::new(|b, cx| {
                b.plugin_network_dialog()
                    .is_some_and(|d| d.read(cx).allow_selected())
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter stores the user's grant and runs the test through the plugin",
            Box::new(|b, cx| {
                b.plugin_network_dialog().is_none()
                    && note_has(b, cx, "Plugin answered", false)
                    && plugins().is_some_and(|d| {
                        idecraft_workbench::plugins::network_granted(
                            &d,
                            MODULE,
                            &target(),
                            idecraft_workbench::plugins::Holder::User,
                        ) && !idecraft_workbench::plugins::network_granted(
                            &d,
                            MODULE,
                            &target(),
                            idecraft_workbench::plugins::Holder::Agent,
                        )
                    })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_note(String::new(), false, cx);
                    v.test(cx);
                });
            }
        })),
        Step::WaitFor(
            "with the grant the test runs at once, without the warning",
            Box::new(|b, cx| {
                b.plugin_network_dialog().is_none() && note_has(b, cx, "Plugin answered", false)
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Some(connectors) = idecraft_workbench::database::connector::connectors_dir() {
                let _ = std::fs::remove_file(connectors.join(format!("{ID}.toml")));
            }
            idecraft_workbench::database::reload_catalogue();
            if let Some(d) = plugins() {
                let _ = std::fs::remove_dir_all(d);
            }
        })),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the connection dialog",
            Box::new(|b, _| b.connection_dialog().is_none()),
        ),
    ]
}

/// The connection dialog (SPEC-0108): a new SQLite connection is tested and saved, shows up in the
/// Database tool window, and two presses of Delete remove it again (the SQL console steps before
/// rely on the workspace having one connection).
fn connection_steps() -> Vec<Step> {
    let listed = |b: &WorkbenchView, cx: &gpui::App, name: &str| {
        b.database_view()
            .is_some_and(|v| v.read(cx).visible().iter().any(|(_, n, _)| n == name))
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("notes.db"), b"");
            }
            if b.database_view().is_none() {
                b.toggle_database(window, cx);
            }
            b.open_connection_dialog(None, window, cx);
            if let (Some(v), Some(dir)) = (b.connection_dialog(), FIXTURE.get()) {
                v.update(cx, |v, cx| {
                    v.set_dialect(idecraft_workbench::Dialect::Sqlite, cx);
                    v.set_field("Name", "notes", cx);
                    v.set_field("File", &dir.join("notes.db").display().to_string(), cx);
                    v.test(cx);
                });
            }
        })),
        Step::WaitFor(
            "Test Connection connects to the new SQLite file",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    v.read(cx)
                        .note()
                        .is_some_and(|(n, error)| !error && n == "Connected to notes")
                })
            }),
        ),
        // SPEC-0108 v3 AK-9: the version line and the round trip under the note, Copy offered.
        Step::Expect(
            "The test result names the SQLite version, the round trip and the file",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    let lines = v.test_details();
                    lines.iter().any(|l| l.starts_with("Server: SQLite 3"))
                        && lines.iter().any(|l| l.starts_with("Round trip: "))
                        && lines
                            .iter()
                            .any(|l| l.starts_with("File: ") && l.ends_with("notes.db"))
                        && v.test_text()
                            .is_some_and(|t| t.starts_with("Connected to notes\nServer: SQLite"))
                })
            }),
        ),
        // SPEC-0108 v4 AK-15: a URL with a password is refused and not kept; Options and colour
        // are saved; a time limit that is not a number stops saving with the reason.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_field("URL", "postgresql://u:pw1234@h/db", cx);
                    v.apply_url_field(cx);
                });
                window.focus(&v.read(cx).focus(cx));
            }
        })),
        Step::Expect(
            "a pasted URL with a password is refused and the field is emptied",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.note()
                        .is_some_and(|(n, error)| *error && !n.contains("pw1234"))
                        && v.form(cx).dialect == idecraft_workbench::Dialect::Sqlite
                        && v.url(cx).is_some_and(|u| u.starts_with("sqlite:"))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_page(idecraft_workbench::DialogPage::Options, window, cx);
                    v.set_field("Query timeout", "soon", cx);
                    v.save(cx);
                });
            }
        })),
        Step::Expect(
            "a time limit that is not a number is named and nothing is saved",
            Box::new(|b, cx| {
                b.connection_dialog().is_some_and(|v| {
                    v.read(cx).note().is_some_and(|(n, error)| {
                        *error && n.contains("Query timeout must be a whole number")
                    })
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.connection_dialog() {
                v.update(cx, |v, cx| {
                    v.set_field("Query timeout", "30", cx);
                    v.set_field("Comment", "local notes", cx);
                    v.next_color(cx);
                    v.set_page(idecraft_workbench::DialogPage::General, window, cx);
                });
                window.focus(&v.read(cx).focus(cx));
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter saves the connection, closes the dialog and lists it",
            Box::new(move |b, cx| {
                b.connection_dialog().is_none()
                    && listed(b, cx, "notes")
                    && FIXTURE.get().is_some_and(|d| {
                        std::fs::read_to_string(d.join(".idecraft/connections.toml")).is_ok_and(
                            |t| {
                                t.contains("name = \"notes\"")
                                    && t.contains("query_timeout_secs = 30")
                                    && t.contains("color = \"red\"")
                                    && t.contains("comment = \"local notes\"")
                            },
                        )
                    })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(v) = b.database_view() {
                window.focus(&v.read(cx).focus());
                v.update(cx, |v, cx| {
                    let row = v.visible().iter().position(|(_, n, _)| n == "notes");
                    if let Some(row) = row {
                        v.select(row, cx);
                    }
                    v.remove_selected(cx);
                });
            }
        })),
        Step::Expect(
            "the first Delete asks before removing",
            Box::new(move |b, cx| {
                listed(b, cx, "notes")
                    && b.database_view()
                        .is_some_and(|v| v.read(cx).removing() == Some("notes"))
            }),
        ),
        Step::Key("delete", None),
        Step::WaitFor(
            "the second Delete removes the connection",
            Box::new(move |b, cx| !listed(b, cx, "notes") && listed(b, cx, "shop")),
        ),
        Step::Act(Box::new(|b, window, cx| b.toggle_database(window, cx))),
    ]
}

/// Plan and schema comparison (SPEC-0110 AK-2): Explain on a join shows the Plan tab; comparing the
/// workspace's SQLite database with a copy lacking a table and with an extra column lists both, writes
/// the script, and after the script ran on the copy the comparison finds no differences.
fn db_compare_steps() -> Vec<Step> {
    let services = |b: &WorkbenchView| b.services_view();
    let close = if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-f4"
    };
    // `refresh.sql` is open (D31 below).
    let opened_refresh = |b: &WorkbenchView, cx: &App| {
        FIXTURE
            .get()
            .is_some_and(|d| b.editor_for(&d.join("refresh.sql"), cx).is_some())
    };
    // The open `refresh.sql` marks the table `refreshed` as unknown.
    let unknown_refreshed = |b: &WorkbenchView, cx: &App| {
        FIXTURE
            .get()
            .and_then(|d| b.editor_for(&d.join("refresh.sql"), cx))
            .is_some_and(|e| {
                e.read(cx)
                    .problems()
                    .iter()
                    .any(|p| p.message.contains("unknown table `refreshed`"))
            })
    };
    // Changes `shop` outside IDECraft, as another program would.
    let shop_sql = |sql: &str| {
        if let Some(dir) = FIXTURE.get() {
            let _ = std::process::Command::new("sqlite3")
                .arg(dir.join("shop.db"))
                .arg(sql)
                .status();
        }
    };
    let select_shop = |b: &mut WorkbenchView, cx: &mut Context<WorkbenchView>| {
        if let Some(v) = b.database_view() {
            v.update(cx, |v, cx| {
                if let Some(row) = v.visible().iter().position(|(_, n, _)| n == "shop") {
                    v.select(row, cx);
                }
            });
        }
    };
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("plan.sql"),
                    "select c.name, o.note from customers c join orders o on o.customer_id = c.id where c.name = 'Ada';\n",
                );
                b.open(&dir.join("plan.sql"), window, cx);
            }
            select_shop(b, cx);
            b.explain_plan(false, window, cx);
        })),
        Step::WaitFor(
            "Explain shows the plan of the statement at the caret",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running()
                        && v.selected_tab() == "Plan"
                        && v.plan().is_some_and(|p| {
                            idecraft_workbench::db_compare_model::plan_rows(p).len() >= 2
                        })
                })
            }),
        ),
        // D31: Refresh in the Database window drops what the SQL editors know about the connection,
        // so tables created meanwhile are known at once; it inspects the connection's open `.sql`
        // files again with the schema read anew (SPEC-0192 v3 behaviour 11), which fills the
        // editors' cache again at once. An open file names a table that does not exist yet; the
        // table is created outside IDECraft; the mark stays until the Refresh and goes after it
        // without an edit (SPEC-0089 v11.1 AK-7). The connection steps before hide the window; it
        // is shown for the refresh and hidden again after it.
        Step::Setup(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("refresh.sql"), "select id from refreshed;\n");
                b.open(&dir.join("refresh.sql"), window, cx);
            }
        })),
        Step::WaitFor(
            "an open SQL file marks a table that does not exist yet as unknown",
            Box::new(move |b, cx| unknown_refreshed(b, cx)),
        ),
        Step::Setup(Box::new(move |_, _, _| {
            shop_sql("CREATE TABLE refreshed (id INTEGER);")
        })),
        Step::Expect(
            "the table created outside stays unknown until the Refresh",
            Box::new(move |b, cx| unknown_refreshed(b, cx)),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if b.database_view().is_none() {
                b.toggle_database(window, cx);
            }
            select_shop(b, cx);
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| v.refresh_selected(cx));
            }
        })),
        Step::WaitFor(
            "Refresh in the Database window refreshes the SQL editors' schema",
            Box::new(move |b, cx| {
                b.database_view()
                    .and_then(|v| v.read(cx).selected_profile())
                    .is_some_and(|p| p.name == "shop" && b.sql_schema_cached(&p.id))
                    && opened_refresh(b, cx)
                    && !unknown_refreshed(b, cx)
            }),
        ),
        // The table goes again and `shop` is read once more, so the steps after see the fixture's
        // two tables; the file is closed and removed.
        Step::Setup(Box::new(move |b, window, cx| {
            shop_sql("DROP TABLE refreshed;");
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| v.refresh_selected(cx));
            }
            b.focus_editor(window, cx);
        })),
        Step::Key(close, None),
        Step::Setup(Box::new(|_, _, _| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("refresh.sql"));
            }
        })),
        Step::Act(Box::new(|b, window, cx| {
            if b.database_view().is_some() {
                b.toggle_database(window, cx);
            }
        })),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let copy = dir.join("copy.db");
                let _ = std::fs::remove_file(&copy);
                let _ = std::process::Command::new("sqlite3")
                    .arg(&copy)
                    .arg("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, legacy TEXT); CREATE TABLE extra (id INTEGER); CREATE VIEW extra_v AS SELECT id FROM extra;")
                    .status();
                let file = dir.join(".idecraft/connections.toml");
                let mut text = std::fs::read_to_string(&file).unwrap_or_default();
                text.push_str(&format!(
                    "\n[[connection]]\nid = \"copy\"\nname = \"copy\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
                    copy.display()
                ));
                let _ = std::fs::write(&file, text);
            }
            select_shop(b, cx);
            b.compare_schemas(window, cx);
        })),
        // SPEC-0110 v3 (D13): even a single candidate is offered, never taken at once.
        Step::Expect(
            "Compare Schemas asks for the target, also when there is only one",
            Box::new(|b, cx| {
                b.compare_popup().is_some_and(|p| {
                    p.read(cx)
                        .items()
                        .first()
                        .is_some_and(|i| i.starts_with("copy · "))
                }) && b
                    .services_view()
                    .is_none_or(|v| v.read(cx).schema_diff().is_none())
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Compare Schemas lists the missing table and the extra column and writes the script",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running()
                        && v.selected_tab() == "Schema Diff"
                        && v.schema_diff().is_some_and(|d| {
                            let lines: Vec<String> = d
                                .lines
                                .iter()
                                .map(|l| format!("{} {}", l.mark.sign(), l.text))
                                .collect();
                            lines.iter().any(|l| l == "+ table orders")
                                && lines.iter().any(|l| l == "− column legacy TEXT")
                                && lines.iter().any(|l| l == "− view extra_v")
                        })
                }) && b.migration_script().is_some_and(|p| {
                    std::fs::read_to_string(p).is_ok_and(|t| t.contains("CREATE TABLE"))
                })
            }),
        ),
        // SPEC-0110 v2 AK-5: destructive statements start unchecked, the view goes first.
        Step::Expect(
            "DROP VIEW and DROP TABLE are listed unchecked, the view first",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    v.read(cx)
                        .schema_diff()
                        .and_then(|d| d.choice.clone())
                        .is_some_and(|c| {
                            let at = |sql: &str| c.statements.iter().position(|s| s.sql == sql);
                            let (view, table) =
                                (at("DROP VIEW \"extra_v\""), at("DROP TABLE \"extra\""));
                            view < table
                                && view.is_some_and(|i| !c.selected[i])
                                && table.is_some_and(|i| !c.selected[i])
                        })
                }) && b.migration_script().is_some_and(|p| {
                    std::fs::read_to_string(p).is_ok_and(|t| t.contains("-- Left out"))
                })
            }),
        ),
        // SPEC-0110 v4 (hand test 121): called from the editor with another tab in front, as from
        // Search Everywhere.
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.show_output(cx));
            }
            b.focus_editor(window, cx);
            b.include_destructive_changes(window, cx);
        })),
        Step::Expect(
            "including them asks first, naming what is lost",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    v.read(cx)
                        .schema_diff()
                        .and_then(|d| d.choice.as_ref()?.confirmation())
                        .is_some_and(|t| t.contains("all rows of table extra"))
                })
            }),
        ),
        Step::Expect(
            "the Schema Diff tab comes to the front with the question",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| v.read(cx).selected_tab() == "Schema Diff")
            }),
        ),
        // D44: the question answers to the keyboard, which only works when the Services window
        // took it from the editor; Escape cancels, Enter includes.
        Step::Key("escape", None),
        Step::Expect(
            "Escape cancels from the keyboard: nothing is included and the question is gone",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.confirming_migration()
                        && v.schema_diff()
                            .and_then(|d| d.choice.clone())
                            .is_some_and(|c| {
                                c.statements
                                    .iter()
                                    .zip(&c.selected)
                                    .all(|(s, on)| s.destructive.is_none() || !on)
                            })
                })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            b.focus_editor(window, cx);
            b.include_destructive_changes(window, cx);
        })),
        Step::Key("enter", None),
        Step::Expect(
            "after Include the script file has every statement in one transaction",
            Box::new(|b, _| {
                b.migration_script().is_some_and(|p| {
                    std::fs::read_to_string(p).is_ok_and(|t| {
                        !t.contains("-- Left out")
                            && t.contains("BEGIN;")
                            && t.contains("DROP VIEW")
                    })
                })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let (Some(dir), Some(script)) = (FIXTURE.get(), b.migration_script()) {
                let _ = std::process::Command::new("sqlite3")
                    .arg(dir.join("copy.db"))
                    .arg(format!(".read {}", script.display()))
                    .status();
            }
            b.compare_schemas(window, cx);
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "after the script ran on the copy the schemas are equal",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running()
                        && v.output_lines()
                            .last()
                            .is_some_and(|l| l.contains("0 difference(s)"))
                        && v.schema_diff().is_some_and(|d| d.lines.is_empty())
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if b.services_view().is_some() {
                b.toggle_services(window, cx);
            }
        })),
    ]
}

/// ER diagram (SPEC-0111 AK-2): ⌥⇧⌘U on the connection draws both tables with `customers` left of
/// `orders` and one line; on the table `orders` it draws it with its neighbour; Escape closes.
/// v2 (AK-5): it opens fitted, ⌘= / ⌘- / ⌘0 zoom, moving `orders` moves its line, Layout puts it
/// back, and the SVG export is written under `.idecraft/diagrams/`.
fn er_diagram_steps(layout: KeyLayout) -> Vec<Step> {
    use std::cell::Cell;
    use std::rc::Rc;
    // Where the layout types `=` with ⇧0, ⌘= is ⇧⌘0 and fits; zooming in is ⌘+ there.
    let zoom_in_mac = if layout.equals_is_shift_zero {
        "cmd-+"
    } else {
        "cmd-="
    };
    let (diagram, zoom_in, zoom_out, actual) = if cfg!(target_os = "macos") {
        ("cmd-alt-shift-u", zoom_in_mac, "cmd--", "cmd-0")
    } else {
        ("ctrl-alt-shift-u", "ctrl-=", "ctrl--", "ctrl-0")
    };
    let before = Rc::new(Cell::new(0f32));
    let zoom = |b: &WorkbenchView, cx: &gpui::App| b.er_view().map(|v| v.read(cx).zoom());
    let remember = {
        let before = before.clone();
        move || {
            let before = before.clone();
            Step::Act(Box::new(move |b, _, cx| {
                if let Some(z) = zoom(b, cx) {
                    before.set(z);
                }
            }))
        }
    };
    let orders = |b: &WorkbenchView, cx: &gpui::App| {
        b.er_view().and_then(|v| {
            let v = v.read(cx);
            let l = v.layout();
            let i = l.boxes.iter().position(|b| b.name == "orders")?;
            let line_end = l.lines.first()?.points.last()?.0;
            Some((i, l.boxes[i].x, l.boxes[i].y, line_end))
        })
    };
    let original_x = Rc::new(Cell::new(0f32));
    let select = |b: &mut WorkbenchView,
                  window: &mut Window,
                  cx: &mut Context<WorkbenchView>,
                  name: &str| {
        if b.database_view().is_none() {
            b.toggle_database(window, cx);
        }
        if let Some(v) = b.database_view() {
            window.focus(&v.read(cx).focus());
            v.update(cx, |v, cx| {
                if let Some(row) = v.visible().iter().position(|(_, n, _)| n == name) {
                    v.select(row, cx);
                }
            });
        }
    };
    let drawn = |b: &WorkbenchView, cx: &gpui::App| {
        b.er_view().is_some_and(|v| {
            let l = v.read(cx).layout().clone();
            let x = |n: &str| l.boxes.iter().find(|b| b.name == n).map(|b| b.x);
            matches!((x("customers"), x("orders")), (Some(c), Some(o)) if c < o)
                && l.boxes.len() == 2
                && l.lines.len() == 1
        })
    };
    vec![
        Step::Act(Box::new(move |b, window, cx| select(b, window, cx, "shop"))),
        Step::Key(diagram, None),
        Step::WaitFor(
            "the shortcut on the connection draws its tables, referenced ones to the left",
            Box::new(move |b, cx| drawn(b, cx)),
        ),
        Step::WaitFor(
            "the diagram opens fitted: every box inside the visible area",
            Box::new(|b, cx| {
                b.er_view().is_some_and(|v| {
                    let v = v.read(cx);
                    let (w, h) = v.viewport();
                    let z = v.zoom();
                    !v.fit_pending()
                        && w > 0.
                        && z <= 1.
                        && v.layout().boxes.iter().all(|b| {
                            (b.x + b.width) * z <= w + 0.5 && (b.y + b.height) * z <= h + 0.5
                        })
                })
            }),
        ),
        remember(),
        Step::Key(zoom_in, None),
        Step::Expect("⌘= zooms in one step", {
            let before = before.clone();
            Box::new(move |b, cx| {
                zoom(b, cx).is_some_and(|z| (z - (before.get() * 1.25).min(4.)).abs() < 0.001)
            })
        }),
        Step::Key(zoom_out, None),
        Step::Expect("⌘- zooms out again", {
            let before = before.clone();
            Box::new(move |b, cx| zoom(b, cx).is_some_and(|z| (z - before.get()).abs() < 0.001))
        }),
        Step::Key(zoom_in, None),
        Step::Key(actual, None),
        Step::Expect(
            "⌘0 shows the actual size",
            Box::new(move |b, cx| zoom(b, cx) == Some(1.)),
        ),
        Step::Act({
            let original_x = original_x.clone();
            Box::new(move |b, _, cx| {
                if let (Some((i, x, y, _)), Some(v)) = (orders(b, cx), b.er_view()) {
                    original_x.set(x);
                    // What dragging the box does.
                    v.update(cx, |v, cx| v.move_box(i, x + 260., y + 180., cx));
                }
            })
        }),
        Step::Expect("dragging orders moves the box and the end of its line", {
            let original_x = original_x.clone();
            Box::new(move |b, cx| {
                orders(b, cx).is_some_and(|(_, x, _, end)| x == original_x.get() + 260. && end == x)
            })
        }),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.er_view() {
                v.update(cx, |v, cx| v.relayout(cx));
            }
        })),
        Step::Expect("Layout puts the box back", {
            let original_x = original_x.clone();
            Box::new(move |b, cx| orders(b, cx).is_some_and(|(_, x, _, _)| x == original_x.get()))
        }),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.er_view() {
                v.update(cx, |v, cx| {
                    let _ = v.export(idecraft_workbench::ErExport::Svg, cx);
                });
            }
        })),
        Step::Expect(
            "the SVG export is written under .idecraft/diagrams",
            Box::new(|_, _| {
                FIXTURE.get().is_some_and(|d| {
                    std::fs::read_to_string(d.join(".idecraft/diagrams/shop.svg"))
                        .is_ok_and(|t| t.contains("<g data-table=\"orders\">"))
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the diagram",
            Box::new(|b, _| b.er_view().is_none()),
        ),
        Step::Act(Box::new(move |b, window, cx| select(b, window, cx, "shop"))),
        Step::Key("right", None),
        Step::WaitFor(
            "the connection shows its tables",
            Box::new(|b, cx| {
                b.database_view()
                    .is_some_and(|v| v.read(cx).visible().iter().any(|(_, n, _)| n == "tables"))
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| v.expand_all_tables(cx));
            }
            select(b, window, cx, "orders");
            b.show_er_diagram(window, cx);
        })),
        Step::WaitFor(
            "on a table the diagram shows it with its neighbour",
            Box::new(move |b, cx| drawn(b, cx)),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|b, window, cx| {
            if b.database_view().is_some() {
                b.toggle_database(window, cx);
            }
        })),
    ]
}

/// Split editors (SPEC-0112 AK-2): Split Right opens the file in a second group; typing there shows
/// in the main view, ⌥⇥ goes back, undo there reverts the text in both; Unsplit closes the group.
fn split_steps() -> Vec<Step> {
    // Next Splitter: ⌥⇥ on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(split_steps_keys())
}

fn split_steps_keys() -> Vec<Step> {
    let (undo, next) = if cfg!(target_os = "macos") {
        ("cmd-z", NEXT_SPLITTER)
    } else {
        ("ctrl-z", NEXT_SPLITTER)
    };
    let texts = |b: &WorkbenchView, cx: &gpui::App| b.split_texts(cx);
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("split.txt"), "first\nsecond\n");
                b.open(&dir.join("split.txt"), window, cx);
            }
            b.split_editor(false, window, cx);
        })),
        Step::Expect(
            "Split Right shows the file in a second group to the right, which has the focus",
            Box::new(|b, _| {
                b.split_state() == Some((vec!["split.txt".to_owned()], false)) && b.split_active()
            }),
        ),
        Step::Key("x", Some("x")),
        Step::Expect(
            "typing in the second group changes the main view too",
            Box::new(move |b, cx| {
                texts(b, cx).is_some_and(|(main, twin)| main == "xfirst\nsecond\n" && main == twin)
            }),
        ),
        Step::Expect(
            "the second group's tab is marked as changed too (D8b)",
            Box::new(|b, cx| b.split_tab_labels(cx) == ["split.txt *"]),
        ),
        // D54 (hand test 199): the second group's text has the editor menu.
        click("split-editor-text", Click::Right),
        Step::WaitFor(
            "a right click on the second group's text opens the editor menu",
            Box::new(|b, _| {
                b.split_active()
                    && b.context_menu_items().is_some_and(|(place, items)| {
                        place == idecraft_workbench::MenuPlace::Editor && !items.is_empty()
                    })
            }),
        ),
        Step::Key("escape", None),
        Step::Key(next, None),
        Step::Expect(
            "Next Splitter moves the focus to the main group",
            Box::new(|b, _| !b.split_active()),
        ),
        Step::Key(undo, None),
        Step::Expect(
            "undo in the main group reverts the typing in both views",
            Box::new(move |b, cx| {
                texts(b, cx).is_some_and(|(main, twin)| main == "first\nsecond\n" && main == twin)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.unsplit(window, cx))),
        Step::Expect(
            "Unsplit closes the second group",
            Box::new(|b, _| b.split_state().is_none()),
        ),
    ]
}

/// Moving tabs between split groups (SPEC-0112 AK-4): Split and Move Right takes the file out of
/// the main group; typing there and moving it back keeps the text; the emptied second group closes.
fn split_move_steps() -> Vec<Step> {
    let name = |b: &WorkbenchView| b.main_tab_titles().iter().any(|t| t == "move-b.txt");
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(dir.join("move-a.txt"), "first\n");
                let _ = std::fs::write(dir.join("move-b.txt"), "beta\n");
                b.open(&dir.join("move-a.txt"), window, cx);
                b.open(&dir.join("move-b.txt"), window, cx);
            }
            b.split_and_move(false, window, cx);
        })),
        Step::Expect(
            "Split and Move Right moves the file into a focused second group",
            Box::new(move |b, _| {
                b.split_state() == Some((vec!["move-b.txt".to_owned()], false))
                    && b.split_active()
                    && !name(b)
            }),
        ),
        Step::Key("x", Some("x")),
        Step::Act(Box::new(|b, window, cx| {
            b.move_to_opposite_group(window, cx)
        })),
        Step::Expect(
            "Move Tab to Opposite Group brings it back unchanged and the empty group closes",
            Box::new(move |b, cx| {
                b.split_state().is_none()
                    && !b.split_active()
                    && name(b)
                    && b.active_text(cx).as_deref() == Some("xbeta\n")
            }),
        ),
    ]
}

/// Editor tabs (SPEC-0059 AK-12) and nested groups (SPEC-0112 AK-6), written for the window
/// self-test: 14 files leave 14 tabs open with the active one visible; the chevron list activates
/// a hidden tab; ⇧⌘T reopens a closed tab at its former place; Close Other Tabs from the tab menu leaves the pinned and
/// the active tab; Split Right, then Split Down in the right group shows three editors.
fn editor_tabs_steps() -> Vec<Step> {
    // Reopen Closed Tab: ⇧⌘T on macOS, Find Action on Linux (no shortcut there).
    with_keys_by_find_action(editor_tabs_steps_keys())
}

fn editor_tabs_steps_keys() -> Vec<Step> {
    use idecraft_workbench::editor_layout::MAIN;
    let close = if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-f4"
    };
    let reopen = REOPEN_CLOSED_TAB;
    fn name(i: usize) -> String {
        format!("editor-tab-{i:02}.txt")
    }
    let titles = |b: &WorkbenchView| -> Vec<String> {
        b.main_tab_titles()
            .into_iter()
            .filter(|t| t.starts_with("editor-tab-"))
            .collect()
    };
    let active_is =
        |b: &WorkbenchView, i: usize| b.active_path().is_some_and(|p| p.ends_with(name(i)));
    vec![
        Step::Setup(Box::new(|b, window, cx| {
            b.close_all_tabs_unchecked(window, cx);
            if let Some(dir) = FIXTURE.get() {
                let d = dir.join("tabs");
                let _ = std::fs::create_dir_all(&d);
                for i in 1..=14 {
                    let _ = std::fs::write(d.join(name(i)), format!("tab {i}\n"));
                    b.open(&d.join(name(i)), window, cx);
                }
            }
        })),
        Step::WaitFor(
            "14 opened files leave all 14 tabs open with the active one fully visible",
            Box::new(move |b, _| {
                titles(b).len() == 14
                    && active_is(b, 14)
                    && b.active_tab_visible(MAIN) == Some(true)
                    && b.tab_bar_overflows(MAIN)
            }),
        ),
        click("tab-list", Click::Left(1)),
        Step::WaitFor(
            "the chevron lists every tab, the hidden ones too",
            Box::new(|b, cx| {
                b.tab_list_labels(cx).is_some_and(|rows| {
                    (1..=14).all(|i| rows.iter().any(|r| *r == format!("editor-tab-{i:02}.txt")))
                })
            }),
        ),
        Step::Key("0", Some("0")),
        Step::Key("3", Some("3")),
        Step::Key("enter", None),
        Step::WaitFor(
            "typing filters the list and Enter activates the hidden tab, scrolled into view",
            Box::new(move |b, cx| {
                b.tab_list_labels(cx).is_none()
                    && active_is(b, 3)
                    && b.active_tab_visible(MAIN) == Some(true)
            }),
        ),
        Step::Key(close, None),
        Step::Expect(
            "Close Tab closes it",
            Box::new(move |b, _| !titles(b).contains(&name(3))),
        ),
        Step::Key(reopen, None),
        Step::WaitFor(
            "Reopen Closed Tab brings it back at its former place, the third tab (SPEC-0059 AK-13)",
            Box::new(move |b, _| {
                titles(b).iter().position(|t| *t == name(3)) == Some(2)
                    && active_is(b, 3)
                    && b.active_tab_visible(MAIN) == Some(true)
            }),
        ),
        // The row shows the third tab at its left end; the first one comes into view through the
        // chevron list, as a user brings back a tab scrolled out of the row.
        click("tab-list", Click::Left(1)),
        Step::WaitFor(
            "the chevron opens the tab list again",
            Box::new(|b, cx| b.tab_list_labels(cx).is_some()),
        ),
        Step::Key("0", Some("0")),
        Step::Key("1", Some("1")),
        Step::Key("enter", None),
        Step::WaitFor(
            "choosing the first tab in the list scrolls the row back to its start",
            Box::new(move |b, cx| {
                b.tab_list_labels(cx).is_none()
                    && active_is(b, 1)
                    && b.active_tab_visible(MAIN) == Some(true)
            }),
        ),
        click("tab-0", Click::Right),
        Step::WaitFor(
            "the tab menu offers Pin Tab",
            Box::new(|b, _| {
                b.context_menu_items()
                    .is_some_and(|(_, items)| items.iter().any(|(l, _)| l == "Pin Tab"))
            }),
        ),
        click("menu-Pin Tab", Click::Left(1)),
        Step::WaitFor(
            "Pin Tab pins the first tab",
            Box::new(|b, _| {
                FIXTURE
                    .get()
                    .is_some_and(|d| b.tab_pinned(&d.join("tabs").join(name(1))))
            }),
        ),
        // The reopened tab is the active one, back in third place.
        click("tab-2", Click::Right),
        Step::WaitFor(
            "the active tab's menu offers Close Other Tabs",
            Box::new(|b, _| {
                b.context_menu_items()
                    .is_some_and(|(_, items)| items.iter().any(|(l, _)| l == "Close Other Tabs"))
            }),
        ),
        click("menu-Close Other Tabs", Click::Left(1)),
        Step::WaitFor(
            "Close Other Tabs leaves the pinned and the active tab",
            Box::new(move |b, _| titles(b) == [name(1), name(3)] && active_is(b, 3)),
        ),
        // SPEC-0112 AK-6: Split Right, then Split Down in the right group.
        click("tab-1", Click::Right),
        Step::WaitFor(
            "the tab menu offers Split Right",
            Box::new(|b, _| {
                b.context_menu_items()
                    .is_some_and(|(_, items)| items.iter().any(|(l, _)| l == "Split Right"))
            }),
        ),
        click("menu-Split Right", Click::Left(1)),
        Step::WaitFor(
            "Split Right shows a second group with the focus",
            Box::new(|b, _| b.group_ids().len() == 2 && b.split_active()),
        ),
        click("split-tab-0", Click::Right),
        Step::WaitFor(
            "the second group's tab has the menu too",
            Box::new(|b, _| {
                b.context_menu_items()
                    .is_some_and(|(_, items)| items.iter().any(|(l, _)| l == "Split Down"))
            }),
        ),
        click("menu-Split Down", Click::Left(1)),
        Step::WaitFor(
            "Split Down in the right group shows three editors: left, and a right column of two",
            Box::new(|b, cx| {
                let ids = b.group_ids();
                ids.len() == 3
                    && b.split_active()
                    && b.editor_tabs_state(cx)["groups"][2]["place"]
                        == serde_json::json!(["right", "bottom"])
            }),
        ),
        Step::Setup(Box::new(|b, window, cx| {
            b.close_all_tabs_unchecked(window, cx)
        })),
    ]
}

/// Tool window layout (SPEC-0167 AK-3): Terminal moved to the right shows there; Database on the
/// same side replaces it; hide all and restore; the stripes hide and come back.
fn tool_window_steps() -> Vec<Step> {
    use idecraft_workbench::tool_windows::{Side, ToolWindow};
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.move_tool_window(ToolWindow::Terminal, Side::Right, cx);
            if !b.tool_window_shown(ToolWindow::Terminal) {
                b.toggle_tool_window(ToolWindow::Terminal, window, cx);
            }
        })),
        Step::Expect(
            "Terminal moved to the right shows there, its button on the right stripe",
            Box::new(|b, _| {
                b.tool_window_shown(ToolWindow::Terminal)
                    && b.tool_layout().side(ToolWindow::Terminal) == Side::Right
                    && b.tool_layout().on(Side::Right).last() == Some(&ToolWindow::Terminal)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.toggle_tool_window(ToolWindow::Database, window, cx)
        })),
        Step::Expect(
            "showing Database on the right hides Terminal",
            Box::new(|b, _| {
                b.tool_window_shown(ToolWindow::Database)
                    && !b.tool_window_shown(ToolWindow::Terminal)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.hide_all_tool_windows(window, cx)
        })),
        Step::Expect(
            "Hide All Tool Windows hides every shown tool window",
            Box::new(|b, _| ToolWindow::ALL.iter().all(|w| !b.tool_window_shown(*w))),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.hide_all_tool_windows(window, cx)
        })),
        Step::Expect(
            "run again it brings them back",
            Box::new(|b, _| b.tool_window_shown(ToolWindow::Database)),
        ),
        Step::Act(Box::new(|b, _, cx| b.toggle_tool_window_bars(cx))),
        Step::Expect(
            "Tool Window Bars hides the stripes",
            Box::new(|b, _| !b.tool_layout().stripes),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.toggle_tool_window_bars(cx);
            b.move_tool_window(ToolWindow::Terminal, Side::Bottom, cx);
            hide_tool_window(b, ToolWindow::Database, window, cx);
        })),
        Step::Expect(
            "the stripes come back and Terminal returns to the bottom",
            Box::new(|b, _| {
                b.tool_layout().stripes
                    && b.tool_layout().side(ToolWindow::Terminal) == Side::Bottom
                    && !b.tool_window_shown(ToolWindow::Database)
            }),
        ),
    ]
}

/// View modes, split placements, "Remove from Sidebar", the More tool windows popup and the
/// Structure tool window (SPEC-0061 v7 AK-29): Terminal unpinned hides when the editor takes the
/// focus; undocked it lies over the editor; floating and as a window it opens a window of its
/// own, which goes again when it is hidden; Structure sits in the left side's second half next to
/// Project; Problems at Bottom Right sits beside Terminal; a removed button comes back when the
/// window is shown.
fn tool_window_mode_steps() -> Vec<Step> {
    use idecraft_workbench::tool_windows::{Placement, Side, ToolWindow, ViewMode};
    let painted = |b: &WorkbenchView, name: &str| -> Vec<idecraft_workbench::tool_windows::Rect> {
        b.painted_frame()
            .into_iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, r)| r)
            .collect()
    };
    vec![
        // Dock Unpinned: shown and focused, then the editor takes the focus.
        Step::Act(Box::new(|b, window, cx| {
            b.set_tool_window_mode(ToolWindow::Terminal, ViewMode::DockUnpinned, window, cx);
        })),
        Step::Expect(
            "Terminal in Dock Unpinned mode is shown with the focus",
            Box::new(|b, _| {
                b.tool_layout().mode(ToolWindow::Terminal) == ViewMode::DockUnpinned
                    && b.tool_window_shown(ToolWindow::Terminal)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        Step::WaitFor(
            "an unpinned Terminal hides once the editor has the focus",
            Box::new(|b, _| !b.tool_window_shown(ToolWindow::Terminal)),
        ),
        // Undock: over the editor, the editor keeps its room.
        Step::Act(Box::new(|b, window, cx| {
            b.set_tool_window_mode(ToolWindow::Terminal, ViewMode::Undock, window, cx);
        })),
        Step::WaitFor(
            "an undocked Terminal is drawn over the editor and takes no room",
            Box::new(move |b, _| {
                let over = painted(b, "undocked");
                let editor = painted(b, "editor");
                b.tool_window_shown(ToolWindow::Terminal)
                    && painted(b, "bottom").is_empty()
                    && over.len() == 1
                    && editor.first().is_some_and(|e| e.intersects(&over[0]))
            }),
        ),
        // Float and Window: a separate window each.
        Step::Act(Box::new(|b, window, cx| {
            b.set_tool_window_mode(ToolWindow::Terminal, ViewMode::Float, window, cx);
        })),
        // The separate windows open and close a frame later (deferred).
        Step::WaitFor(
            "a floating Terminal opens a window of its own",
            Box::new(|b, _| b.floating_windows() == vec![ToolWindow::Terminal]),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.set_tool_window_mode(ToolWindow::Terminal, ViewMode::Window, window, cx);
        })),
        // The separate windows open and close a frame later (deferred).
        Step::WaitFor(
            "as a Window it still has one window of its own",
            Box::new(|b, cx| {
                b.floating_windows() == vec![ToolWindow::Terminal]
                    && b.floating_window_count(cx) == 1
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            // As its stripe button: the keys are in its own window, not in this one.
            b.hide_shown_tool_window(ToolWindow::Terminal, window, cx);
        })),
        // The separate windows open and close a frame later (deferred).
        Step::WaitFor(
            "hiding it closes its window",
            Box::new(|b, cx| b.floating_windows().is_empty() && b.floating_window_count(cx) == 0),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.set_tool_window_mode(ToolWindow::Terminal, ViewMode::DockPinned, window, cx);
            hide_tool_window(b, ToolWindow::Terminal, window, cx);
        })),
        // Split halves: Structure below Project, Problems beside Terminal.
        Step::Act(Box::new(|b, window, cx| {
            if !b.tool_window_shown(ToolWindow::Project) {
                b.toggle_tool_window(ToolWindow::Project, window, cx);
            }
            b.toggle_tool_window(ToolWindow::Structure, window, cx);
        })),
        Step::WaitFor(
            "Structure (Left Bottom) shows below Project in the same column",
            Box::new(move |b, _| {
                let sides = painted(b, "side");
                b.tool_window_shown(ToolWindow::Structure)
                    && b.tool_window_shown(ToolWindow::Project)
                    && sides.len() == 2
                    && sides[0].x == sides[1].x
                    && sides[0].y != sides[1].y
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            hide_tool_window(b, ToolWindow::Structure, window, cx);
            b.place_tool_window(ToolWindow::Problems, Placement::new(Side::Bottom, true), cx);
            for w in [ToolWindow::Terminal, ToolWindow::Problems] {
                if !b.tool_window_shown(w) {
                    b.toggle_tool_window(w, window, cx);
                }
            }
        })),
        Step::WaitFor(
            "Problems at Bottom Right sits beside Terminal, its button on the right stripe",
            Box::new(move |b, _| {
                let bottom = painted(b, "bottom");
                bottom.len() == 2
                    && bottom[0].y == bottom[1].y
                    && bottom[0].x != bottom[1].x
                    && b.tool_layout()
                        .stripe(Side::Right)
                        .foot
                        .contains(&ToolWindow::Problems)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.place_tool_window(
                ToolWindow::Problems,
                Placement::new(Side::Bottom, false),
                cx,
            );
            hide_tool_window(b, ToolWindow::Problems, window, cx);
            hide_tool_window(b, ToolWindow::Terminal, window, cx);
        })),
        // Remove from Sidebar; shown again it has a button only while shown; Add to Sidebar
        // keeps it (SPEC-0061 v10 behaviour 52).
        Step::Act(Box::new(|b, _, cx| {
            b.remove_tool_window_from_sidebar(ToolWindow::Terminal, cx)
        })),
        Step::Expect(
            "Remove from Sidebar takes the Terminal button away",
            Box::new(|b, _| {
                b.tool_layout().removed(ToolWindow::Terminal)
                    && !b
                        .tool_layout()
                        .stripe(Side::Left)
                        .foot
                        .contains(&ToolWindow::Terminal)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ActivateTerminalToolWindow", window, cx)
        })),
        Step::Expect(
            "shown again it has a button while shown but stays off the sidebar",
            Box::new(|b, _| {
                b.tool_window_shown(ToolWindow::Terminal)
                    && b.tool_layout().removed(ToolWindow::Terminal)
                    && b.tool_layout()
                        .stripe_with(Side::Left, &[ToolWindow::Terminal])
                        .foot
                        .contains(&ToolWindow::Terminal)
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            b.restore_tool_window_to_sidebar(ToolWindow::Terminal, cx)
        })),
        Step::Expect(
            "Add to Sidebar keeps the Terminal button",
            Box::new(|b, _| {
                !b.tool_layout().removed(ToolWindow::Terminal)
                    && b.tool_layout()
                        .stripe(Side::Left)
                        .foot
                        .contains(&ToolWindow::Terminal)
            }),
        ),
        // The More tool windows popup.
        Step::Act(Box::new(|b, _, cx| b.toggle_more_tool_windows(cx))),
        Step::Expect(
            "the … button lists the tool windows by title, Structure among them",
            Box::new(|b, _| {
                let listed = b.more_tool_windows_listed();
                b.more_tool_windows_open()
                    && listed.contains(&ToolWindow::Structure)
                    && listed
                        .windows(2)
                        .all(|p| p[0].title().to_lowercase() <= p[1].title().to_lowercase())
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.toggle_more_tool_windows(cx);
            hide_tool_window(b, ToolWindow::Terminal, window, cx);
        })),
        Step::Expect(
            "the popup closes and the layout is back to its defaults",
            Box::new(|b, _| {
                !b.more_tool_windows_open()
                    && b.tool_layout().mode(ToolWindow::Terminal) == ViewMode::DockPinned
                    && b.tool_layout().placement(ToolWindow::Problems)
                        == Placement::new(Side::Bottom, false)
            }),
        ),
    ]
}

/// Replacing instead of stacking (SPEC-0061 v10 AK-37, behaviours 49–53; SPEC-0167 v4 AK-7):
/// Problems replaces Terminal at the bottom and Git replaces Problems through its action, one
/// bottom island each time; ⌥-click shows Terminal in Bottom Right beside Git without moving it,
/// and again hides it; the default stripes (AI Chat, Database, Build on the right; Plugins and
/// Endpoints off the sidebar, listed in "…"); Plugins shown from "…" has a button only while shown.
fn tool_window_replace_steps() -> Vec<Step> {
    use idecraft_workbench::tool_windows::{Placement, Side, ToolWindow};
    let bottoms = |b: &WorkbenchView| -> Vec<idecraft_workbench::tool_windows::Rect> {
        b.painted_frame()
            .into_iter()
            .filter(|(n, _)| *n == "bottom")
            .map(|(_, r)| r)
            .collect()
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if !b.tool_window_shown(ToolWindow::Terminal) {
                b.toggle_tool_window(ToolWindow::Terminal, window, cx);
            }
            b.toggle_tool_window(ToolWindow::Problems, window, cx);
        })),
        Step::WaitFor(
            "Problems replaces Terminal at the bottom: one bottom island",
            Box::new(move |b, _| {
                b.tool_window_shown(ToolWindow::Problems)
                    && !b.tool_window_shown(ToolWindow::Terminal)
                    && bottoms(b).len() == 1
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("ActivateVersionControlToolWindow", window, cx)
        })),
        Step::WaitFor(
            "Git through its action replaces Problems",
            Box::new(move |b, _| {
                b.tool_window_shown(ToolWindow::Git)
                    && !b.tool_window_shown(ToolWindow::Problems)
                    && bottoms(b).len() == 1
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.alt_click_tool_window(ToolWindow::Terminal, window, cx)
        })),
        Step::WaitFor(
            "⌥-click shows Terminal in Bottom Right beside Git, its placement unchanged",
            Box::new(move |b, _| {
                let bottom = bottoms(b);
                b.tool_window_shown(ToolWindow::Terminal)
                    && b.tool_window_shown(ToolWindow::Git)
                    && b.tool_layout().placement(ToolWindow::Terminal)
                        == Placement::new(Side::Bottom, false)
                    && b.tool_layout().shown_placement(ToolWindow::Terminal)
                        == Placement::new(Side::Bottom, true)
                    && bottom.len() == 2
                    && bottom[0].y == bottom[1].y
                    && bottom[0].x < bottom[1].x
                    && b.tool_layout()
                        .stripe(Side::Left)
                        .foot
                        .contains(&ToolWindow::Terminal)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.alt_click_tool_window(ToolWindow::Terminal, window, cx)
        })),
        Step::WaitFor(
            "⌥-click again hides it and ends the exception",
            Box::new(move |b, _| {
                !b.tool_window_shown(ToolWindow::Terminal)
                    && !b.tool_layout().is_shown_elsewhere(ToolWindow::Terminal)
                    && b.tool_window_shown(ToolWindow::Git)
                    && bottoms(b).len() == 1
            }),
        ),
        Step::Expect(
            "the default stripes: Notifications, AI Chat, Database, Build on the right, Plugins and Endpoints off",
            Box::new(|b, _| {
                let l = b.tool_layout();
                // Notifications is first on the right stripe since SPEC-0061 v17 behaviour 64; Build
                // keeps its place in the layout and shows its button after the first build (65).
                l.stripe(Side::Right).top
                    == vec![
                        ToolWindow::Notifications,
                        ToolWindow::AiChat,
                        ToolWindow::Database,
                        ToolWindow::Build,
                    ]
                    && l.stripe(Side::Left).top.first() == Some(&ToolWindow::Project)
                    && l.stripe(Side::Left).split == vec![ToolWindow::Structure]
                    && l.removed(ToolWindow::Plugins)
                    && l.removed(ToolWindow::Endpoints)
                    && b.more_tool_windows_listed()
                        .contains(&ToolWindow::Endpoints)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            hide_tool_window(b, ToolWindow::Git, window, cx);
            b.toggle_tool_window(ToolWindow::Plugins, window, cx);
        })),
        Step::Expect(
            "Plugins shown from \"…\" has a button while shown and stays off the sidebar",
            Box::new(|b, _| {
                b.tool_window_shown(ToolWindow::Plugins)
                    && b.tool_layout().removed(ToolWindow::Plugins)
                    && b.tool_layout()
                        .stripe_with(Side::Right, &[ToolWindow::Plugins])
                        .top
                        .contains(&ToolWindow::Plugins)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            hide_tool_window(b, ToolWindow::Plugins, window, cx);
        })),
        Step::Expect(
            "hidden again, Plugins is still off the sidebar",
            Box::new(|b, _| {
                !b.tool_window_shown(ToolWindow::Plugins)
                    && b.tool_layout().removed(ToolWindow::Plugins)
            }),
        ),
    ]
}

/// Terminal sessions (SPEC-0205) and search and shell integration (SPEC-0206): a renamed tab,
/// a split whose second shell runs a command and ends with `exit`, ⌘F through the history, the
/// prompt marks of the user's shell, ⌘↑ to the previous prompt, and the saved tabs coming back.
fn terminal_session_steps() -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let panes = |b: &WorkbenchView| b.terminal_panes();
    let send = |text: &'static str| {
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, _| t.send_text(text));
            }
        }))
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            b.new_terminal_tab(window, cx);
            let i = b.terminal_tab();
            b.start_terminal_rename(i, window, cx);
            if let Some(f) = b.terminal_rename_field() {
                f.update(cx, |f, cx| f.set_text("build", cx));
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "a double click's field renames the tab on Enter",
            Box::new(|b, _| {
                b.terminal_tabs().get(b.terminal_tab()).map(String::as_str) == Some("build")
                    && b.terminal_rename_field().is_none()
            }),
        ),
        Step::Key(if mac { "cmd-d" } else { "ctrl-shift-d" }, None),
        Step::Expect(
            "⌘D starts a second shell to the right with focus",
            Box::new(move |b, _| {
                let p = panes(b);
                p.len() == 2 && b.terminal_side_by_side() && b.terminal() == Some(p[1].clone())
            }),
        ),
        send("echo split-$((1+1))\r"),
        Step::WaitFor(
            "the second shell runs the command, the first does not show it",
            Box::new(move |b, cx| {
                let p = panes(b);
                p.len() == 2
                    && p[1]
                        .read(cx)
                        .history_text()
                        .lines()
                        .any(|l| l.trim() == "split-2")
                    && !p[0].read(cx).history_text().contains("split-2")
            }),
        ),
        Step::Key(if mac { "alt-cmd-left" } else { "ctrl-alt-left" }, None),
        Step::Expect(
            "⌥⌘← moves the focus to the first shell",
            Box::new(move |b, _| b.terminal() == panes(b).first().cloned()),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(p) = panes(b).get(1) {
                p.update(cx, |t, _| t.send_text("exit\r"));
            }
        })),
        Step::WaitFor(
            "`exit` closes the second shell, the tab stays",
            Box::new(move |b, _| {
                panes(b).len() == 1
                    && b.terminal_tabs().get(b.terminal_tab()).map(String::as_str) == Some("build")
            }),
        ),
        Step::Key(if mac { "cmd-d" } else { "ctrl-shift-d" }, None),
        Step::Key(
            if mac {
                "shift-cmd-d"
            } else {
                "ctrl-shift-alt-d"
            },
            None,
        ),
        Step::Expect(
            "⇧⌘D after ⌘D puts the third shell below the second, not to its right (D47)",
            Box::new(|b, _| {
                let cells = b.terminal_cells();
                cells.iter().map(Vec::len).collect::<Vec<_>>() == [1, 2]
                    && b.terminal_side_by_side()
                    && b.terminal() == cells.get(1).and_then(|c| c.get(1)).cloned()
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            for p in panes(b).iter().skip(1) {
                p.update(cx, |t, _| t.send_text("exit\r"));
            }
        })),
        Step::WaitFor(
            "`exit` in both new shells leaves one shell",
            Box::new(move |b, _| panes(b).len() == 1),
        ),
        send("seq -f 'row%g;' 1 200; false\r"),
        Step::WaitFor(
            "the long output fills the history",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    t.read(cx)
                        .history_text()
                        .lines()
                        .any(|l| l.trim() == "row200;")
                })
            }),
        ),
        Step::Key(if mac { "cmd-f" } else { "ctrl-shift-f" }, None),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(t) = b.terminal() {
                t.update(cx, |t, cx| t.set_search_query("42;", cx));
            }
        })),
        Step::Expect(
            "⌘F finds both lines ending in 42;, the bottom one first",
            Box::new(|b, cx| {
                b.terminal()
                    .is_some_and(|t| t.read(cx).search_label().as_deref() == Some("2/2"))
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "Enter goes to the line 42 above and scrolls it into view",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    let t = t.read(cx);
                    t.search_label().as_deref() == Some("1/2")
                        && t.scroll_offset() > 0
                        && t.shown_text().lines().any(|l| l.trim() == "row42;")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the field and returns to the live screen",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    let t = t.read(cx);
                    t.search_label().is_none() && t.scroll_offset() == 0
                })
            }),
        ),
        Step::WaitFor(
            "the shell integration marks the failed command with its status",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    t.read(cx).prompts().iter().any(|p| {
                        p.command.as_deref() == Some("seq -f 'row%g;' 1 200; false")
                            && p.exit == Some(1)
                    })
                })
            }),
        ),
        Step::Key(if mac { "cmd-up" } else { "ctrl-shift-up" }, None),
        Step::Expect(
            "⌘↑ scrolls to the previous prompt",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    let t = t.read(cx);
                    t.scroll_offset() > 0
                        && top_rows_contain(&t.shown_text(), 3, "seq -f 'row%g;' 1 200; false")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            // The tabs as saved, then all closed and started again as when the project opens.
            let saved = b.saved_terminals(cx);
            while !b.terminal_tabs().is_empty() {
                b.close_terminal_tab(0, window, cx);
            }
            let _ = saved.save(FIXTURE.get().expect("fixture"));
            b.reload_saved_terminals(window, cx);
        })),
        Step::Expect(
            "the saved tabs come back with their names",
            Box::new(|b, _| b.terminal_tabs().iter().any(|t| t == "build")),
        ),
        Step::Act(Box::new(|b, window, cx| {
            if let Some(i) = b.terminal_tabs().iter().position(|t| t == "build") {
                b.select_terminal_tab(i, window, cx);
            }
        })),
        Step::WaitFor(
            "the restored tab shows its saved output before the new prompt",
            Box::new(|b, cx| {
                b.terminal().is_some_and(|t| {
                    t.read(cx)
                        .history_text()
                        .lines()
                        .any(|l| l.trim() == "row199;")
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            while !b.terminal_tabs().is_empty() {
                b.close_terminal_tab(0, window, cx);
            }
        })),
        Step::Expect(
            "closing every tab hides the terminal and forgets the saved tabs",
            Box::new(|b, _| {
                b.terminal().is_none()
                    && !FIXTURE
                        .get()
                        .expect("fixture")
                        .join(idecraft_terminal::SAVED_FILE)
                        .exists()
            }),
        ),
    ]
}

/// The status bar stays free (SPEC-0061 v2 AK-5, SPEC-0167 v2 AK-5): with Terminal and Problems
/// side by side at the bottom (one window per part since SPEC-0061 v10), in a small and in a large
/// window, no island or stripe as painted reaches into the status bar, which spans the full width.
fn status_bar_steps() -> Vec<Step> {
    use idecraft_workbench::tool_windows::ToolWindow;
    // One window per bottom part since SPEC-0061 v10: Terminal in Bottom Left, Problems
    // ⌥-clicked into Bottom Right.
    const BOTTOM: [ToolWindow; 2] = [ToolWindow::Terminal, ToolWindow::Problems];
    thread_local! {
        static SIZE: std::cell::Cell<Option<gpui::Size<gpui::Pixels>>> =
            const { std::cell::Cell::new(None) };
    }
    let free = |width: f32| {
        move |b: &WorkbenchView, _: &App| {
            let frame = b.painted_frame();
            let Some(status) = frame.iter().find(|(n, _)| *n == "status").map(|(_, r)| *r) else {
                return false;
            };
            (status.w - width).abs() < 1.0
                && status.x.abs() < 0.5
                && frame.iter().filter(|(n, _)| *n == "bottom").count() == 2
                && frame.iter().any(|(n, _)| *n == "editor")
                && frame
                    .iter()
                    .filter(|(n, _)| *n != "status")
                    .all(|(_, r)| r.bottom() <= status.y + 0.5 && !r.intersects(&status))
        }
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            SIZE.with(|s| s.set(Some(window.viewport_size())));
            if !b.tool_window_shown(ToolWindow::Terminal) {
                b.toggle_tool_window(ToolWindow::Terminal, window, cx);
            }
            if !b.tool_window_shown(ToolWindow::Problems) {
                b.alt_click_tool_window(ToolWindow::Problems, window, cx);
            }
            window.resize(gpui::size(gpui::px(1100.), gpui::px(620.)));
        })),
        Step::WaitFor(
            "two bottom windows side by side in a small window stay above the full-width status bar",
            Box::new(free(1100.)),
        ),
        Step::Act(Box::new(|_, window, _| {
            window.resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
        })),
        Step::WaitFor(
            "two bottom windows side by side in a large window stay above the full-width status bar",
            Box::new(free(1600.)),
        ),
        Step::Act(Box::new(|b, window, cx| {
            for w in BOTTOM {
                hide_tool_window(b, w, window, cx);
            }
            if let Some(size) = SIZE.with(|s| s.take()) {
                window.resize(size);
            }
        })),
    ]
}

/// Editing a result (SPEC-0107): the rows of `customers` can be edited; a changed cell and an
/// added row are pending until ⌘↩ submits them, then the query runs again and shows the table.
/// The SQL editor bound to a data source (SPEC-0192): a console of the workspace's SQLite
/// connection with the schema in its toolbar, the run choice, timing notes, the inline unsafe-change
/// warning and its confirmation, and the closest-name fix (SPEC-0015 v2).
fn sql_editor_steps() -> Vec<Step> {
    let (execute, save, close) = if cfg!(target_os = "macos") {
        ("cmd-enter", "cmd-s", "cmd-w")
    } else {
        ("ctrl-enter", "ctrl-s", "ctrl-f4")
    };
    let text = |t: &'static str, line: usize, column: usize| -> Step {
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    e.set_text(t, cx);
                    e.go_to(line, column, cx);
                });
                window.focus(&e.read(cx).focus());
            }
            b.inspect_active_sql(cx);
        }))
    };
    let note = |b: &WorkbenchView, cx: &gpui::App, source: &str, line: usize, end: &str| {
        b.active_editor_entity().is_some_and(|e| {
            e.read(cx)
                .line_notes()
                .iter()
                .any(|(s, n)| *s == source && n.line == line && n.text.ends_with(end))
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::process::Command::new("sqlite3")
                    .arg(dir.join("shop.db"))
                    .arg("INSERT INTO orders(customer_id, note) VALUES (1, 'a'), (2, 'b');")
                    .status();
            }
            b.menu_action("Console.Open", window, cx);
        })),
        Step::WaitFor(
            "Console.Open opens a console of the connection, in its default schema",
            Box::new(|b, cx| {
                b.active_file()
                    .is_some_and(|p| p.ends_with(".idecraft/consoles/shop/console.sql"))
                    && b.sql_toolbar_label(cx).as_deref() == Some("shop · main")
            }),
        ),
        text("select 1;\nselect name from customers;\n", 1, 3),
        Step::Key(execute, None),
        Step::Expect(
            "⌘↩ offers the statement at the caret or the whole script",
            Box::new(|b, cx| {
                b.sql_popup().is_some_and(|v| {
                    v.read(cx).items()
                        == [
                            "Statement at caret: select name from customers",
                            "Whole script (2 statements)",
                        ]
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "the statement runs and its time and row count stand after it",
            Box::new(move |b, cx| {
                b.services_view().is_some_and(|v| !v.read(cx).running())
                    && note(b, cx, "run", 1, " ms · 2 rows")
                    && !note(b, cx, "run", 0, " row")
            }),
        ),
        Step::Key(execute, None),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::WaitFor(
            "Whole script runs both statements, each with its note",
            Box::new(move |b, cx| {
                b.services_view().is_some_and(|v| !v.read(cx).running())
                    && note(b, cx, "run", 0, " ms · 1 row")
                    && note(b, cx, "run", 1, " ms · 2 rows")
            }),
        ),
        text("delete from orders;\n", 0, 3),
        Step::WaitFor(
            "the unsafe-change warning stands after the statement",
            Box::new(move |b, cx| {
                note(
                    b,
                    cx,
                    "inspection",
                    0,
                    "DELETE without WHERE removes every row of `orders`",
                )
            }),
        ),
        Step::Key(execute, None),
        Step::WaitFor(
            "running it asks first, naming the table and its rows, Cancel preselected",
            Box::new(|b, cx| {
                b.sql_popup().is_some_and(|v| {
                    let v = v.read(cx);
                    v.title() == "Delete all 2 rows of `orders`?"
                        && v.selected() == 2
                        && v.items()[1] == "Run, don't ask again for shop in this session"
                })
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "Cancel runs nothing",
            Box::new(|b, _| {
                let rows = FIXTURE.get().and_then(|dir| {
                    std::process::Command::new("sqlite3")
                        .arg(dir.join("shop.db"))
                        .arg("SELECT count(*) FROM orders;")
                        .output()
                        .ok()
                });
                b.sql_popup().is_none()
                    && rows.is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "2")
            }),
        ),
        text("select nme from customers;\n", 0, 8),
        Step::WaitFor(
            "an unknown column is underlined",
            Box::new(|b, cx| {
                b.active_editor_entity().is_some_and(|e| {
                    e.read(cx)
                        .problems()
                        .iter()
                        .any(|p| p.message == "SQL: unknown column `nme`")
                })
            }),
        ),
        Step::Key("alt-enter", None),
        Step::WaitFor(
            "⌥↩ offers the closest name first",
            Box::new(|b, cx| {
                b.intentions_popup().is_some_and(|v| {
                    v.read(cx).titles().first().map(String::as_str) == Some("Replace with `name`")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter replaces the name",
            Box::new(|b, cx| {
                b.intentions_popup().is_none()
                    && b.active_text(cx).as_deref() == Some("select name from customers;\n")
            }),
        ),
        Step::Key(save, None),
        Step::Key(close, None),
        Step::Act(Box::new(|_, _, _| {
            if let Some(d) = FIXTURE.get() {
                let _ = std::fs::remove_dir_all(d.join(".idecraft/consoles"));
            }
        })),
    ]
}

/// A connection whose schema cannot be read shows the cause and a retry row (SPEC-0089 v3,
/// IMP-0051); retrying after the cause is gone lists its tables.
fn database_error_steps() -> Vec<Step> {
    let original = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let keep = original.clone();
    let restore = original.clone();
    vec![
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let file = dir.join(".idecraft/connections.toml");
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                *keep.borrow_mut() = text.clone();
                let _ = std::fs::remove_dir_all(dir.join("missing"));
                let _ = std::fs::write(
                    &file,
                    format!(
                        "{text}\n[[connection]]\nid = \"broken\"\nname = \"broken\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
                        dir.join("missing/broken.db").display()
                    ),
                );
            }
            b.toggle_database(window, cx);
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.reload();
                    v.select_label("broken", cx);
                });
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "a schema that cannot be read shows the cause and a retry row",
            Box::new(|b, cx| {
                b.database_view().is_some_and(|v| {
                    let rows = v.read(cx).visible();
                    rows.iter()
                        .any(|(d, n, _)| *d == 1 && n.starts_with("Could not read the schema: "))
                        && rows.iter().any(|(d, n, _)| *d == 1 && n == "Retry")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::create_dir_all(dir.join("missing"));
                let _ = std::process::Command::new("sqlite3")
                    .arg(dir.join("missing/broken.db"))
                    .arg("CREATE TABLE t (x INTEGER);")
                    .status();
            }
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.select_label("Retry", cx);
                });
            }
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "Retry reads the schema again",
            Box::new(|b, cx| {
                b.database_view().is_some_and(|v| {
                    let rows = v.read(cx).visible();
                    rows.iter().any(|(_, n, d)| n == "tables" && d == "1")
                        && !rows.iter().any(|(_, n, _)| n == "Retry")
                })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join(".idecraft/connections.toml"),
                    restore.borrow().as_str(),
                );
                let _ = std::fs::remove_dir_all(dir.join("missing"));
            }
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.reload();
                    cx.notify();
                });
            }
            b.toggle_database(window, cx);
        })),
        Step::Expect(
            "the database window is hidden again",
            Box::new(|b, _| b.database_view().is_none()),
        ),
    ]
}

fn grid_edit_steps() -> Vec<Step> {
    let submit = if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    };
    let services = |b: &WorkbenchView| b.services_view();
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("edit.sql"),
                    "select id, name from customers order by id;\n",
                );
                b.open(&dir.join("edit.sql"), window, cx);
            }
        })),
        // D12: the statement runs from the key, and no completion list is left open afterwards.
        Step::Key(submit, None),
        Step::WaitFor(
            "a query on one table with its key gives an editable result",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running() && v.shown_edits().is_some_and(|e| e.table() == "customers")
                })
            }),
        ),
        Step::Expect(
            "no completion list is open after running the statement",
            Box::new(|b, _| b.completion_items().is_none()),
        ),
        // D31: the file keeps the data source it ran on.
        Step::Expect(
            "running the file binds it to its connection",
            Box::new(|_, _| {
                FIXTURE.get().is_some_and(|d| {
                    std::fs::read_to_string(d.join(".idecraft/sql-files.toml"))
                        .is_ok_and(|t| t.contains("edit.sql"))
                })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = services(b) {
                window.focus(&v.read(cx).focus());
                v.update(cx, |v, cx| {
                    v.select_cell(0, 1, cx);
                    v.start_edit(window, cx);
                });
            }
        })),
        Step::Key("x", Some("x")),
        Step::Key("enter", None),
        Step::Expect(
            "Enter keeps the edited cell as a pending change",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.editing()
                        && v.cell_text_at(0, 1).as_deref() == Some("Adax")
                        && v.shown_edits().is_some_and(|e| e.count() == 1)
                })
            }),
        ),
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.add_row(cx);
                    v.select_cell(2, 1, cx);
                    v.start_edit(window, cx);
                });
            }
        })),
        Step::Key("g", Some("g")),
        Step::Key("r", Some("r")),
        Step::Key("a", Some("a")),
        Step::Key("enter", None),
        Step::Expect(
            "an added row is pending too",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.cell_text_at(2, 1).as_deref() == Some("gra")
                        && v.shown_edits().is_some_and(|e| e.count() == 2)
                })
            }),
        ),
        Step::Key(submit, None),
        Step::WaitFor(
            "submitting commits both changes and shows the table again",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.note() == Some("2 change(s) committed")
                        && v.shown_result().is_some_and(|r| {
                            r.rows.len() == 3
                                && r.rows[0][1].as_deref() == Some("Adax")
                                && r.rows[2][1].as_deref() == Some("gra")
                        })
                        && v.shown_edits().is_some_and(|e| e.is_empty())
                })
            }),
        ),
        // SPEC-0190 v4 AK-8: the value panel edits the selected cell; the change is pending like
        // an edit in the cell, and Revert takes it back.
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.select_cell(1, 1, cx);
                    v.start_value_edit(window, cx);
                    v.set_value_text("Linus\nTorvalds", cx);
                    v.finish_value_edit(false, window, cx);
                });
            }
        })),
        Step::Expect(
            "the value panel's text becomes the cell's pending value",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.value_editing()
                        && v.side_view() == Some(idecraft_workbench::result_model::SideView::Value)
                        && v.cell_text_at(1, 1).as_deref() == Some("Linus\nTorvalds")
                        && v.shown_edits().is_some_and(|e| e.count() == 1)
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.revert(cx);
                    v.show_side(None, cx);
                });
            }
        })),
        Step::Expect(
            "Revert takes the panel's change back",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.cell_text_at(1, 1).as_deref() == Some("Linus")
                        && v.shown_edits().is_some_and(|e| e.is_empty())
                })
            }),
        ),
        // SPEC-0093 v3 AK-10: Manual mode keeps the console's transaction open until Commit.
        Step::Act(Box::new(|b, window, cx| {
            b.set_console_manual(true, window, cx);
            if let Some(e) = b.active_editor_entity() {
                window.focus(&e.read(cx).focus());
            }
        })),
        Step::Key(submit, None),
        Step::WaitFor(
            "a run in Manual mode opens the console's transaction",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| !v.read(cx).running())
                    && b.console_transaction() == (true, 1)
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.finish_console_transaction(true, window, cx)
        })),
        Step::WaitFor(
            "Commit ends the transaction and says so in Output",
            Box::new(move |b, cx| {
                b.console_transaction() == (true, 0)
                    && services(b).is_some_and(|v| {
                        v.read(cx)
                            .output_lines()
                            .iter()
                            .any(|l| l == "Committed 1 statement(s)")
                    })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.set_console_manual(false, window, cx)
        })),
        Step::Expect(
            "the console is back in Auto mode",
            Box::new(|b, _| b.console_transaction() == (false, 0) && !b.console_cancellable()),
        ),
    ]
}

/// Table data editor (SPEC-0190), copying and exporting (SPEC-0021 v2) and copying a table
/// (SPEC-0191) on the `customers` table left by the grid steps (Adax, Linus, gra).
fn table_editor_steps() -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let (submit, export, execute) = if mac {
        ("cmd-enter", "cmd-alt-shift-e", "cmd-enter")
    } else {
        ("ctrl-enter", "ctrl-alt-shift-e", "ctrl-enter")
    };
    let services = |b: &WorkbenchView| b.services_view();
    let sqlite = |sql: &str| -> String {
        FIXTURE
            .get()
            .and_then(|d| {
                std::process::Command::new("sqlite3")
                    .arg(d.join("shop.db"))
                    .arg(sql)
                    .output()
                    .ok()
            })
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    // Shows the Database window, reads `shop` again and selects `customers` once it is listed (the
    // tree may come back collapsed when the window is shown again).
    let select_table = |what: &'static str| -> Vec<Step> {
        vec![
            Step::Act(Box::new(|b, window, cx| {
                if b.database_view().is_none() {
                    b.toggle_database(window, cx);
                }
                if let Some(v) = b.database_view() {
                    v.update(cx, |v, cx| v.refresh_connection("shop", cx));
                }
            })),
            Step::WaitFor(
                what,
                Box::new(|b, cx| {
                    b.database_view()
                        .is_some_and(|v| v.read(cx).visible().iter().any(|(_, n, _)| n == "tables"))
                }),
            ),
            Step::Act(Box::new(|b, window, cx| {
                if let Some(v) = b.database_view() {
                    v.update(cx, |v, cx| {
                        v.expand_all_tables(cx);
                        v.select_label("customers", cx);
                    });
                    window.focus(&v.read(cx).focus());
                }
            })),
        ]
    };
    let mut steps = select_table("the Database window reads the connection");
    steps.extend([
        Step::Key("f4", None),
        Step::WaitFor(
            "F4 on a table opens its data editor with its rows",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.selected_tab() == "customers"
                        && v.table_state()
                            .is_some_and(|(t, _, _, loading)| t == "customers" && !loading)
                        && v.shown_result().is_some_and(|r| r.rows.len() == 3)
                        && v.shown_edits().is_some_and(|e| e.table() == "customers")
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.set_clauses("name like 'A%'", "", cx));
            }
        })),
        Step::WaitFor(
            "a WHERE clause is applied on the server",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.page_text().as_deref() == Some("1 row")
                        && v.shown_rows().first().is_some_and(|r| r[1] == "Adax")
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.set_clauses("1; drop table customers", "", cx);
                });
            }
        })),
        Step::Expect(
            "a WHERE field that would end the statement is refused",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| v.read(cx).note().is_some_and(|n| n.contains("`;`")))
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.set_clauses("", "", cx);
                });
            }
        })),
        Step::WaitFor(
            "clearing the fields shows every row again",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.page_text().as_deref() == Some("3 rows")
                        && v.table_state().is_some_and(|s| !s.3)
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.sort_by(1, false, cx));
            }
        })),
        Step::WaitFor(
            "a header click orders on the server",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.table_state()
                        .is_some_and(|(_, _, o, loading)| o == "\"name\"" && !loading)
                        && v.shown_rows().first().is_some_and(|r| r[1] == "Adax")
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.sort_by(1, false, cx));
            }
        })),
        Step::WaitFor(
            "a second click orders descending",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.table_state()
                        .is_some_and(|(_, _, o, loading)| o == "\"name\" DESC" && !loading)
                        && v.shown_rows().first().is_some_and(|r| r[1] == "gra")
                })
            }),
        ),
        // Copying two rows and two columns as TSV and Markdown, with the header (IMP-0061).
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = services(b) {
                window.focus(&v.read(cx).focus());
                v.update(cx, |v, cx| {
                    v.select_cell(0, 0, cx);
                    v.extend_selection(1, 1, cx);
                });
            }
        })),
        Step::Key(if mac { "cmd-c" } else { "ctrl-c" }, None),
        Step::Expect(
            "the selected cells are copied as TSV with a header row",
            Box::new(move |b, cx| {
                services(b)
                    .is_some_and(|v| v.read(cx).last_copy() == Some("id\tname\n3\tgra\n2\tLinus\n"))
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.set_copy_format(idecraft_workbench::result_model::ExportFormat::Markdown, cx);
                    v.copy_selection(cx);
                });
            }
        })),
        Step::Expect(
            "Markdown has the header and aligns the numeric column right",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    v.read(cx).last_copy()
                        == Some("| id | name |\n| ---: | --- |\n| 3 | gra |\n| 2 | Linus |\n")
                })
            }),
        ),
        Step::Key(export, None),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.set_export_format(idecraft_workbench::result_model::ExportFormat::Csv, cx);
                    v.set_header(false, cx);
                });
            }
        })),
        Step::Expect(
            "the export preview follows the format and the header setting",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.exporting() && v.export_preview() == "3,gra\r\n2,Linus\r\n"
                })
            }),
        ),
        // D42: the dialog fits the panel, so its Export button is always on screen.
        Step::WaitFor(
            "the export dialog's buttons lie inside the Services panel",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let (footer, panel) = v.read(cx).export_edges();
                    footer > 0. && panel > 0. && footer <= panel + 0.5
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let (Some(v), Some(dir)) = (services(b), FIXTURE.get()) {
                let path = dir.join("export/customers.csv").display().to_string();
                v.update(cx, |v, cx| {
                    v.set_header(true, cx);
                    v.set_export_target(Some(&path), cx);
                });
            }
        })),
        // D42: Enter runs the export like the button.
        Step::Key("enter", None),
        Step::WaitFor(
            "exporting writes the file with the header",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| !v.read(cx).exporting())
                    && FIXTURE.get().is_some_and(|d| {
                        std::fs::read_to_string(d.join("export/customers.csv"))
                            .is_ok_and(|t| t == "id,name\r\n3,gra\r\n2,Linus\r\n")
                    })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.set_copy_format(idecraft_workbench::result_model::ExportFormat::Tsv, cx);
                    v.select_cell(0, 1, cx);
                });
            }
        })),
        Step::Key("shift-enter", None),
        Step::Expect(
            "shift+enter shows the selected row as a record",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.side_view() == Some(idecraft_workbench::result_model::SideView::Record)
                        && v.record()
                            == [
                                ("id".to_owned(), "3".to_owned()),
                                ("name".to_owned(), "gra".to_owned()),
                            ]
                })
            }),
        ),
        Step::Key("shift-enter", None),
        // Manual transaction: the edit is applied but not committed until Commit.
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.set_manual(true, cx);
                    // SPEC-0190 v3 behaviour 13: SQLite refuses read committed, keeps serializable.
                    v.set_isolation(idecraft_workbench::Isolation::ReadCommitted, cx);
                    v.set_isolation(idecraft_workbench::Isolation::Serializable, cx);
                    v.select_cell(0, 1, cx);
                    v.start_edit(window, cx);
                });
            }
        })),
        Step::Key("x", Some("x")),
        Step::Key("enter", None),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.toggle_preview(cx));
            }
        })),
        Step::Expect(
            "the preview shows the one UPDATE that submitting runs",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.previewing()
                        && v.isolation() == Some(idecraft_workbench::Isolation::Serializable)
                        && v.preview_sql().is_ok_and(|sql| {
                            sql.len() == 1
                                && sql[0].starts_with("UPDATE \"customers\" SET \"name\" = 'grax'")
                        })
                })
            }),
        ),
        Step::Key(submit, None),
        Step::WaitFor(
            "in manual mode the change is applied inside an open transaction",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.transaction() == Some((true, Some(1)))
                        && v.shown_rows().first().is_some_and(|r| r[1] == "grax")
                        && v.shown_edits().is_some_and(|e| e.is_empty())
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.finish_transaction(true, cx));
            }
        })),
        Step::WaitFor(
            "Commit ends the transaction and the change is in the database",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.transaction() == Some((true, None))
                        && v.note() == Some("1 change(s) committed")
                }) && sqlite("select name from customers where id = 3") == "grax"
            }),
        ),
        Step::Act(Box::new(move |_, _, _| {
            sqlite("update customers set name = 'gra' where id = 3");
        })),
        // Pinning keeps a console result over the next run.
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("pin.sql"),
                    "select id, name from customers order by id;\n",
                );
                b.open(&dir.join("pin.sql"), window, cx);
                b.execute_sql(window, cx);
            }
        })),
        Step::WaitFor(
            "a console run adds its result after the data editor",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running() && v.tab_titles() == ["customers", "Result 1"]
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| v.toggle_pin(cx));
            }
        })),
        Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        Step::Key(execute, None),
        Step::WaitFor(
            "the pinned result stays, the new run gets its own tab",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running() && v.tab_titles() == ["customers", "📌 Result 1", "Result 1"]
                })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            if let Some(v) = services(b) {
                v.update(cx, |v, cx| {
                    v.close_result(2, cx);
                    v.close_result(1, cx);
                    v.close_result(0, cx);
                });
            }
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::remove_file(dir.join("pin.sql"));
                let _ = std::fs::remove_dir_all(dir.join("export"));
            }
        })),
        Step::Expect(
            "closing the tabs leaves the output",
            Box::new(move |b, cx| {
                services(b).is_some_and(|v| {
                    let v = v.read(cx);
                    v.tab_titles().is_empty() && v.selected_tab() == "Output"
                })
            }),
        ),
    ]);
    // Copying a table keeps its key (SPEC-0191, IMP-0058).
    steps.extend(select_table("the Database window lists the tables again"));
    steps.extend([
        Step::Key("f5", None),
        Step::WaitFor(
            "F5 on a table opens the copy dialog with a preview that keeps the key",
            Box::new(|b, cx| {
                b.copy_table_dialog().is_some_and(|d| {
                    let d = d.read(cx);
                    d.title() == "Copy Table customers To…"
                        && d.target(cx).table == "customers_copy"
                        && d.preview(cx).is_ok_and(|(sql, _)| {
                            sql[0].contains("PRIMARY KEY (\"id\")")
                                && sql[0].contains("\"name\" TEXT NOT NULL")
                        })
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "enter copies the table with its rows and the tree shows it",
            Box::new(move |b, cx| {
                b.copy_table_dialog().is_none()
                    && sqlite("select count(*) from customers_copy") == "3"
                    && b.database_view().is_some_and(|v| {
                        v.read(cx)
                            .visible()
                            .iter()
                            .any(|(_, n, _)| n == "customers_copy")
                    })
            }),
        ),
        Step::Act(Box::new(move |b, _, cx| {
            sqlite("drop table customers_copy");
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| v.refresh_connection("shop", cx));
            }
        })),
        Step::Key("escape", None),
    ]);
    steps
}

/// SQL console (SPEC-0093): ⌘↩ runs the statement at the caret against the workspace's SQLite
/// connection and shows its rows in the Services window; a failing statement selects Output.
fn sql_console_steps(goto_line: &'static str) -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let (execute, services, complete) = if mac {
        ("cmd-enter", "cmd-8", "alt-escape")
    } else {
        ("ctrl-enter", "alt-8", "ctrl-space")
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::process::Command::new("sqlite3")
                    .arg(dir.join("shop.db"))
                    .arg("INSERT INTO customers(name) VALUES ('Ada'), ('Linus');")
                    .status();
                let _ = std::fs::write(
                    dir.join("q.sql"),
                    "select 1 as one;\nselect id, name, null as note from customers order by id;\nselect * from missing;\nwith recursive n(i) as (select 1 union all select i + 1 from n where i < 1200) select i from n;\n",
                );
                b.open(&dir.join("q.sql"), window, cx);
            }
        })),
        Step::Key(goto_line, None),
        Step::Key("2", Some("2")),
        Step::Key("enter", None),
        Step::Key(execute, None),
        // Several statements: ⌘↩ asks what to run, the statement at the caret first (SPEC-0192).
        Step::Expect(
            "⌘↩ in a script of several statements asks what to run",
            Box::new(|b, cx| {
                b.sql_popup().is_some_and(|v| {
                    let v = v.read(cx);
                    v.selected() == 0
                        && v.items()
                            == [
                                "Statement at caret: select id, name, null as note from customers or…",
                                "Whole script (4 statements)",
                            ]
                })
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "the statement at the caret shows its rows in the services window",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running()
                        && v.selected_tab() == "Result 1"
                        && v.shown_result().is_some_and(|r| {
                            r.columns == ["id", "name", "note"]
                                && r.rows.len() == 2
                                && r.rows[0][1].as_deref() == Some("Ada")
                                && r.rows[0][2].is_none()
                        })
                        && v.output_lines()
                            .last()
                            .is_some_and(|l| l.contains("2 rows"))
                })
            }),
        ),
        // Sorting and filtering the grid (SPEC-0099): two clicks on "name" sort descending, the
        // filter keeps matching rows.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.sort_by(1, false, cx);
                    v.sort_by(1, false, cx);
                });
            }
        })),
        Step::Expect(
            "two clicks on a column header sort the rows descending",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    v.read(cx)
                        .shown_rows()
                        .first()
                        .is_some_and(|r| r[1] == "Linus")
                })
            }),
        ),
        // SPEC-0099 v3 AK-9: the value filter of a column, the text and the tree view.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.open_value_menu(Some(1), cx);
                    v.toggle_value(1, Some("Ada".into()), cx);
                });
            }
        })),
        Step::Expect(
            "unchecking Ada in the name's value list keeps Linus and marks the column",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.value_menu() == Some(1)
                        && v.filters_column(1)
                        && v.column_values(1)
                            == [(Some("Ada".into()), 1), (Some("Linus".into()), 1)]
                        && v.shown_rows().len() == 1
                        && v.shown_rows()[0][1] == "Linus"
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.clear_values(Some(1), cx);
                    v.open_value_menu(None, cx);
                    v.set_result_view(idecraft_workbench::ResultView::Text, cx);
                });
            }
        })),
        Step::Expect(
            "Select all shows both rows again and the text view draws them as a table",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    let text = v.text_view();
                    v.shown_rows().len() == 2
                        && text.lines().nth(1).is_some_and(|l| l.starts_with("| id |"))
                        && text.contains("| Linus")
                        && text.contains("<null>")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.set_result_view(idecraft_workbench::ResultView::Tree, cx);
                    v.toggle_tree_row(0, cx);
                });
            }
        })),
        Step::Expect(
            "the tree view numbers the rows and expands the first into its columns",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let lines = v.read(cx).tree_lines();
                    lines.len() == 5
                        && lines[0].text.starts_with("1 {id: ")
                        && lines[1].text.starts_with("id: ")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.set_result_view(idecraft_workbench::ResultView::Table, cx)
                });
            }
        })),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| v.set_filter("ada", cx));
            }
        })),
        Step::WaitFor(
            "the filter keeps the rows that contain its text",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.shown_rows().len() == 1
                        && v.page_text().as_deref() == Some("1 row (filtered from 2)")
                })
            }),
        ),
        // Paging (SPEC-0099): 1200 rows show 500 at a time; the last page holds the rest.
        Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        Step::Key(goto_line, None),
        Step::Key("4", Some("4")),
        Step::Key("enter", None),
        Step::Key(execute, None),
        Step::Key("enter", None),
        Step::WaitFor(
            "a large result shows its first page",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running() && v.page_text().as_deref() == Some("1–500 of 1200")
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| v.turn_page(i64::MAX, cx));
            }
        })),
        Step::Expect(
            "the last page shows the remaining rows",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page_text().as_deref() == Some("1001–1200 of 1200")
                        && v.shown_rows().first().is_some_and(|r| r[0] == "1001")
                })
            }),
        ),
        // SPEC-0099 v2 behaviours 5, 7, 8 (OBS-0026): page size, transposed view, column width.
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| v.set_page_size(100, cx));
            }
        })),
        Step::Expect(
            "a page size of 100 shows the first 100 rows",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page_text().as_deref() == Some("1–100 of 1200")
                        && v.shown_rows().len() == 100
                        && v.page_size() == Some(100)
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.set_page_size(idecraft_workbench::grid_model::ALL_ROWS, cx);
                    v.toggle_transpose(cx);
                    v.set_column_width(0, Some(10.), cx);
                });
            }
        })),
        Step::Expect(
            "All shows every row on one page, transposed, with the dragged width kept at its minimum",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page_text().as_deref() == Some("1200 rows")
                        && v.shown_rows().len() == 1200
                        && v.transposed()
                        && v.column_width(0) == Some(24.)
                })
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.toggle_transpose(cx);
                    v.set_column_width(0, None, cx);
                    v.set_page_size(idecraft_workbench::grid_model::PAGE_SIZE, cx);
                });
            }
        })),
        Step::Expect(
            "the grid is back to rows, fitted columns and pages of 500",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.transposed()
                        && v.column_width(0).is_none()
                        && v.page_text().as_deref() == Some("1–500 of 1200")
                })
            }),
        ),
        // SPEC-0190 v5 AK-9 (F-054 window check): in a narrow window the toolbar keeps one row;
        // what does not fit goes into its ⋮ menu, whose entries act like the buttons.
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| v.set_toolbar_limit(Some(420.), cx));
            }
        })),
        Step::WaitFor(
            "a narrow toolbar keeps the pager and filter and offers the rest under ⋮",
            Box::new(|b, cx| {
                use idecraft_workbench::ToolbarGroup as G;
                b.services_view().is_some_and(|v| {
                    let (shown, more) = v.read(cx).shown_toolbar_groups();
                    shown.contains(&G::Pager)
                        && shown.contains(&G::Fields)
                        && more.contains(&G::Transpose)
                        && more.contains(&G::Export)
                }) && idecraft_workbench::hit_targets::get("services-more").is_some()
            }),
        ),
        hover("services-more"),
        Step::WaitFor(
            "resting on ⋮ shows its More tooltip",
            Box::new(|_, _| tooltip_shows(|t| t.starts_with("More"))),
        ),
        click("services-more", Click::Left(1)),
        // SPEC-0294 v4 AK-11: the tooltip does not stay beside the menu it opened.
        Step::WaitFor(
            "⋮ opens the menu with the toolbar's other actions, without its tooltip beside it",
            Box::new(|b, cx| {
                b.services_view()
                    .is_some_and(|v| v.read(cx).more_menu_open())
                    && idecraft_workbench::hit_targets::get("more-services-transpose").is_some()
                    && idecraft_ui::tooltip::shown().is_none()
            }),
        ),
        click("more-services-transpose", Click::Left(1)),
        Step::WaitFor(
            "Transpose from the ⋮ menu transposes the grid and closes the menu",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.transposed() && !v.more_menu_open()
                })
            }),
        ),
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(v) = b.services_view() {
                v.update(cx, |v, cx| {
                    v.toggle_transpose(cx);
                    v.set_toolbar_limit(None, cx);
                });
            }
        })),
        Step::Expect(
            "the grid is back to rows",
            Box::new(|b, cx| b.services_view().is_some_and(|v| !v.read(cx).transposed())),
        ),
        Step::Act(Box::new(|b, window, cx| b.focus_editor(window, cx))),
        Step::Key(goto_line, None),
        Step::Key("3", Some("3")),
        Step::Key("enter", None),
        Step::Key(execute, None),
        Step::Key("enter", None),
        Step::WaitFor(
            "a failing statement selects the output with the error",
            Box::new(|b, cx| {
                b.services_view().is_some_and(|v| {
                    let v = v.read(cx);
                    !v.running()
                        && v.selected_tab() == "Output"
                        && v.output_lines()
                            .last()
                            .is_some_and(|l| l.contains("no such table: missing"))
                })
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.focus_editor(window, cx);
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = "select * from cu";
                    e.set_text(text, cx);
                    e.select_range(text.len()..text.len(), cx);
                });
            }
        })),
        Step::Key(complete, None),
        Step::WaitFor(
            "completion in a sql file offers the tables of the connection",
            Box::new(|b, _| {
                b.completion_items()
                    .is_some_and(|items| items.iter().any(|i| i == "customers"))
            }),
        ),
        Step::Key("escape", None),
        // SPEC-0014 v3 behaviour 14: after JOIN the foreign key from orders to customers is
        // offered as a ready join clause, before the plain table.
        Step::Act(Box::new(|b, window, cx| {
            b.focus_editor(window, cx);
            if let Some(e) = b.active_editor_entity() {
                e.update(cx, |e, cx| {
                    let text = "select * from orders o join ";
                    e.set_text(text, cx);
                    e.select_range(text.len()..text.len(), cx);
                });
            }
        })),
        Step::Key(complete, None),
        Step::WaitFor(
            "completion after join offers the join clause from the foreign key",
            Box::new(|b, _| {
                b.completion_items().is_some_and(|items| {
                    items.first().map(String::as_str) == Some("customers c on o.customer_id = c.id")
                        && items.iter().any(|i| i == "customers")
                })
            }),
        ),
        Step::Key("escape", None),
    ]
    .into_iter()
    .chain(shortcut_hides(
        services,
        "the services shortcut focuses the shown window",
        "the services shortcut hides the window",
        |b| b.services_view().is_some(),
    ))
    .collect()
}

/// Debugging a Python script with debugpy (SPEC-0091 v2): breakpoint, stop with locals, step into,
/// run to the end. Skipped without debugpy.
fn python_debug_steps(goto_line: &'static str) -> Vec<Step> {
    let has_debugpy = idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some());
    if !has_debugpy {
        eprintln!("python debug steps skipped: debugpy is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (toggle, debug, step_into, resume) = if mac {
        ("cmd-f8", "ctrl-d", "f7", "cmd-alt-r")
    } else {
        ("ctrl-f8", "shift-f9", "f7", "f9")
    };
    let state = |b: &WorkbenchView, cx: &App| {
        b.debug_view().map(|v| {
            let v = v.read(cx);
            (
                v.state().clone(),
                v.frame_names(),
                v.variable_texts(),
                v.console().to_owned(),
            )
        })
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("dbg.py"),
                    "def twice(x):\n    y = x * 2\n    return y\n\n\nnumber = 21\nresult = twice(number)\nprint(f\"result {result}\")\n",
                );
                let _ = std::fs::write(
                    dir.join(".idecraft/run.toml"),
                    "[[run]]\nname = \"pydebug\"\ncommand = [\"python3\", \"dbg.py\"]\n",
                );
                b.select_run_configuration("pydebug", cx);
                b.open(&dir.join("dbg.py"), window, cx);
            }
        })),
        Step::Key(goto_line, None),
        Step::Key("7", Some("7")),
        Step::Key("enter", None),
        Step::Key(toggle, None),
        Step::Key(debug, None),
        Step::WaitFor(
            "debugging python stops at the breakpoint with the module's variables",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("module"))
                        && vars.iter().any(|v| v == "number = 21")
                }) && b
                    .active_debug_marks(cx)
                    .is_some_and(|(_, exec)| exec == Some(6))
            }),
        ),
        Step::Key(step_into, None),
        Step::WaitFor(
            "step into enters the python function",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, frames, vars, _)| {
                    matches!(s, DebugState::Paused(_))
                        && frames.first().is_some_and(|f| f.contains("twice"))
                        && vars.iter().any(|v| v == "x = 21")
                })
            }),
        ),
        Step::Key(resume, None),
        Step::WaitFor(
            "the python script runs to the end",
            Box::new(move |b, cx| {
                state(b, cx).is_some_and(|(s, _, _, console)| {
                    matches!(&s, DebugState::Ended(l) if l.contains("exit code 0"))
                        && console.contains("result 42")
                })
            }),
        ),
    ]
}

/// Attach to Process (SPEC-0147): a Python loop runs under `debugpy --listen`; ⌥⇧F5 (Ctrl+Alt+F5)
/// opens the dialog, `:port` offers each runtime, Python attaches and stops at the breakpoint in the
/// loop, Stop detaches and the program goes on. Skipped without debugpy.
fn attach_steps(goto_line: &'static str) -> Vec<Step> {
    use std::sync::Mutex;
    static PROGRAM: Mutex<Option<(std::process::Child, u16)>> = Mutex::new(None);
    let has_debugpy = idecraft_debugger::known_adapters()
        .iter()
        .any(|a| a.name == "debugpy" && a.command.is_some());
    if !has_debugpy {
        eprintln!("attach steps skipped: debugpy is missing");
        return Vec::new();
    }
    let mac = cfg!(target_os = "macos");
    let (toggle, attach, stop) = if mac {
        ("cmd-f8", "alt-shift-f5", "cmd-f2")
    } else {
        ("ctrl-f8", "ctrl-alt-f5", "ctrl-f2")
    };
    // Whether something listens on the local `port`, asked without connecting; where `lsof` is
    // missing the start alone counts, as before.
    fn listening(port: u16) -> bool {
        std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(true)
    }
    let alive = || {
        PROGRAM
            .lock()
            .ok()
            .and_then(|mut p| {
                p.as_mut()
                    .map(|(c, _)| c.try_wait().is_ok_and(|s| s.is_none()))
            })
            .unwrap_or(false)
    };
    vec![
        Step::Act(Box::new(|b, window, cx| {
            let Some(dir) = FIXTURE.get() else {
                return;
            };
            let script = dir.join("attach_loop.py");
            let _ = std::fs::write(
                &script,
                "import time\n\nticks = 0\nwhile True:\n    ticks += 1\n    time.sleep(0.05)\n",
            );
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .map(|a| a.port())
                .unwrap_or(5679);
            let child = std::process::Command::new("python3")
                .args(["-m", "debugpy", "--listen"])
                .arg(format!("127.0.0.1:{port}"))
                .arg(&script)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let (Ok(child), Ok(mut p)) = (child, PROGRAM.lock()) {
                *p = Some((child, port));
            }
            b.open(&script, window, cx);
        })),
        Step::Key(goto_line, None),
        Step::Key("5", Some("5")),
        Step::Key("enter", None),
        Step::Key(toggle, None),
        // debugpy serves one client, so readiness is not probed by connecting; the port is asked
        // for its listening socket instead (on a loaded Mac debugpy took longer than a second).
        Step::WaitFor(
            "the attach target has started",
            Box::new(move |_, _| {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let program = PROGRAM
                    .lock()
                    .ok()
                    .and_then(|p| p.as_ref().map(|(c, port)| (c.id(), *port)));
                alive()
                    && program.is_some_and(|(pid, port)| {
                        idecraft_debugger::processes()
                            .iter()
                            .any(|p| p.pid == pid && p.debug_port.is_some())
                            && listening(port)
                    })
            }),
        ),
        Step::Key(attach, None),
        Step::Expect(
            "alt-shift-f5 opens the Attach to Process dialog with the local processes",
            Box::new(|b, cx| {
                b.attach_dialog()
                    .is_some_and(|d| !d.read(cx).rows().is_empty())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            let port = PROGRAM
                .lock()
                .ok()
                .and_then(|p| p.as_ref().map(|(_, port)| *port));
            if let (Some(d), Some(port)) = (b.attach_dialog(), port) {
                d.update(cx, |d, cx| d.set_filter(&format!(":{port}"), cx));
            }
        })),
        Step::Expect(
            "a :port filter offers each runtime for that address",
            Box::new(|b, cx| {
                b.attach_dialog().is_some_and(|d| {
                    let rows = d.read(cx).rows().to_vec();
                    rows.len() >= 4
                        && rows[1].runtime == idecraft_debugger::Runtime::Python
                        && rows[0].title.starts_with("127.0.0.1:")
                })
            }),
        ),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::WaitFor(
            "attaching to the python loop stops at the breakpoint with its counter",
            Box::new(|b, cx| {
                b.attach_dialog().is_none()
                    && b.debug_view().is_some_and(|v| {
                        let v = v.read(cx);
                        matches!(v.state(), DebugState::Paused(_))
                            && v.console().starts_with("Attaching to 127.0.0.1:")
                            && v.variable_texts().iter().any(|t| t.starts_with("ticks = "))
                    })
            }),
        ),
        Step::Key(stop, None),
        Step::WaitFor(
            "stop detaches and the program goes on running",
            Box::new(move |b, cx| {
                b.debug_view().is_some_and(|v| {
                    matches!(v.read(cx).state(), DebugState::Ended(l) if l.starts_with("Detached"))
                }) && {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    alive()
                }
            }),
        ),
        Step::Act(Box::new(|_, _, _| {
            if let Ok(mut p) = PROGRAM.lock()
                && let Some((mut child, _)) = p.take()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        })),
    ]
}

/// The Settings dialog (SPEC-0105): ⌘, (Ctrl+Alt+S) opens it on Appearance; Tab shows the keymap;
/// typing finds Save All; Enter and a keystroke add a shortcut that the keymap uses at once and the
/// user keymap file keeps; ⇧⌫ on the chosen row and Shift+Delete restore the bundled shortcuts;
/// Escape closes.
fn settings_steps() -> Vec<Step> {
    use idecraft_workbench::settings_model::Page;
    let open = if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-alt-s"
    };
    let added = || {
        idecraft_workbench::keymaps::Keystroke::parse("control alt shift K")
            .map(|k| {
                idecraft_workbench::keymaps::current()
                    .actions_for(&k, None)
                    .contains(&"SaveAll")
            })
            .unwrap_or(false)
    };
    let saved = || {
        FIXTURE
            .get()
            .and_then(|d| std::fs::read_to_string(d.join(".idecraft/keymap.toml")).ok())
            .unwrap_or_default()
    };
    vec![
        Step::Key(open, None),
        Step::Expect(
            "the shortcut opens Settings on Appearance",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).page() == Page::Appearance)
            }),
        ),
        // SPEC-0105 v5 behaviour 3b: a setting is found by its label, typed in full: a part of
        // it ("sync") also finds the Inspections page by an inspection's name ("Empty 'finally'
        // or 'synchronized' block", SPEC-0315 behaviour 4).
        Step::Key("s", Some("s")),
        Step::Key("y", Some("y")),
        Step::Key("n", Some("n")),
        Step::Key("c", Some("c")),
        Step::Key("space", Some(" ")),
        Step::Key("w", Some("w")),
        Step::Key("i", Some("i")),
        Step::Key("t", Some("t")),
        Step::Key("h", Some("h")),
        Step::Key("space", Some(" ")),
        Step::Key("o", Some("o")),
        Step::Key("s", Some("s")),
        Step::Expect(
            "the search finds Appearance by the label Sync with OS",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page() == Page::Appearance && v.pages() == [Page::Appearance, Page::Keymap]
                })
            }),
        ),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Key("backspace", None),
        Step::Expect(
            "the emptied search lists every page again",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).pages() == Page::ALL)
            }),
        ),
        // SPEC-0105 v6: Tab goes through the pages of Appearance & Behavior to Keymap.
        Step::Key("tab", None),
        Step::Expect(
            "Tab goes from Appearance to System Settings",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).page() == Page::SystemSettings)
            }),
        ),
        Step::Key("tab", None),
        Step::Key("tab", None),
        Step::Key("tab", None),
        Step::Expect(
            "after the pages of Appearance & Behavior comes Keymap",
            Box::new(|b, cx| {
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).page() == Page::Keymap)
            }),
        ),
        Step::Key("s", Some("s")),
        Step::Key("a", Some("a")),
        Step::Key("v", Some("v")),
        Step::Key("e", Some("e")),
        Step::Key("a", Some("a")),
        Step::Key("l", Some("l")),
        Step::Key("l", Some("l")),
        Step::Expect(
            "typing on the keymap page finds Save All first",
            Box::new(|b, cx| {
                b.settings_dialog().is_some_and(|v| {
                    let v = v.read(cx);
                    v.page() == Page::Keymap
                        && v.rows().first().is_some_and(|r| r.action == "SaveAll")
                })
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "enter waits for the shortcut",
            Box::new(|b, cx| b.settings_dialog().is_some_and(|v| v.read(cx).recording())),
        ),
        Step::Key("ctrl-alt-shift-k", None),
        Step::Expect(
            "the keystroke becomes a shortcut of Save All, used and saved at once",
            Box::new(move |b, cx| {
                b.settings_dialog().is_some_and(|v| !v.read(cx).recording())
                    && added()
                    && saved().contains("\"control alt shift K\"")
            }),
        ),
        // SPEC-0105 v4 (hand test 30): on a Mac keyboard the restore is ⇧⌫, pressed with the
        // search still typed; the row was chosen with Enter, so it restores and the search stays.
        Step::Key("shift-backspace", None),
        Step::Expect(
            "shift+backspace on the chosen row restores and leaves the search as typed",
            Box::new(move |b, cx| {
                !added()
                    && !saved().contains("SaveAll")
                    && b.settings_dialog().is_some_and(|v| {
                        let v = v.read(cx);
                        v.page() == Page::Keymap
                            && v.rows().first().is_some_and(|r| r.action == "SaveAll")
                            && v.rows().len() < 20
                    })
            }),
        ),
        Step::Key("enter", None),
        Step::Key("ctrl-alt-shift-k", None),
        Step::Expect(
            "the shortcut is added a second time",
            Box::new(move |b, cx| {
                b.settings_dialog().is_some_and(|v| !v.read(cx).recording()) && added()
            }),
        ),
        Step::Key("shift-delete", None),
        Step::Expect(
            "shift+delete restores the bundled shortcuts",
            Box::new(move |_, _| !added() && !saved().contains("SaveAll")),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes Settings",
            Box::new(|b, _| b.settings_dialog().is_none()),
        ),
    ]
}

/// The import preview (SPEC-0303 behaviour 20): a theme file shows what it contains and changes,
/// Escape changes nothing, Enter imports and switches the theme; a file of a newer format shows its
/// message and cannot be imported. Ends on IDECraft Dark again.
fn import_steps() -> Vec<Step> {
    use idecraft_workbench::palette::{ThemeId, current_theme};
    let file = |name: &str, format: u32, theme: &str| {
        FIXTURE.get().map(|d| {
            let p = d.join(name);
            let _ = std::fs::write(
                &p,
                format!(
                    "[idecraft]\nkind = \"theme\"\nformat = {format}\nname = \"Light\"\n\n[theme]\nname = \"{theme}\"\n"
                ),
            );
            p
        })
    };
    let open = move |name: &'static str, format: u32| -> Step {
        Step::Act(Box::new(move |b, window, cx| {
            if let Some(p) = file(name, format, "IDECraft Light") {
                b.open_import(&p, window, cx);
            }
        }))
    };
    let lines = |b: &WorkbenchView, cx: &App| {
        b.import_preview()
            .map(|v| v.read(cx).lines())
            .unwrap_or_default()
    };
    let saved = || {
        FIXTURE
            .get()
            .and_then(|d| idecraft_core::chosen_theme(&d.join(".idecraft/ide-settings.toml"), None))
    };
    vec![
        open("light.idctheme", 1),
        Step::Expect(
            "the preview names the theme file and the change it makes",
            Box::new(move |b, cx| {
                let l = lines(b, cx);
                l.first().is_some_and(|h| h == "Import Theme: Light")
                    && l.iter().any(|x| x == "  theme IDECraft Light")
                    && l.iter().any(|x| {
                        x.starts_with("  replace appearance.theme in IDE settings:")
                            && x.ends_with("→ IDECraft Light")
                    })
                    && b.import_preview().is_some_and(|v| v.read(cx).can_import())
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the preview and changes nothing",
            Box::new(|b, _| {
                b.import_preview().is_none() && current_theme() == ThemeId::IdecraftDark
            }),
        ),
        open("light.idctheme", 1),
        Step::Key("enter", None),
        Step::Expect(
            "enter imports: the theme switches and is saved in the IDE settings",
            Box::new(move |b, _| {
                b.import_preview().is_none()
                    && current_theme() == ThemeId::IdecraftLight
                    && saved().as_deref() == Some("IDECraft Light")
            }),
        ),
        open("newer.idctheme", 2),
        Step::Expect(
            "a file of a newer format shows the version and cannot be imported",
            Box::new(move |b, cx| {
                lines(b, cx).iter().any(|x| {
                    x.contains("written by a newer IDECraft (format 2); this version reads theme files up to format 1")
                }) && b.import_preview().is_some_and(|v| !v.read(cx).can_import())
            }),
        ),
        Step::Key("enter", None),
        Step::Expect(
            "enter does not import a file with problems",
            Box::new(|b, _| b.import_preview().is_some()),
        ),
        Step::Key("escape", None),
        Step::Act(Box::new(|_, _, cx| {
            let _ = idecraft_workbench::appearance::choose_theme(ThemeId::IdecraftDark, cx);
        })),
        Step::Expect(
            "the preview is closed and IDECraft Dark is back",
            Box::new(|b, _| {
                b.import_preview().is_none() && current_theme() == ThemeId::IdecraftDark
            }),
        ),
    ]
}

/// Quick Switch Scheme (SPEC-0060 v5, v6): ⌃` lists the themes with the default, IDECraft Dark,
/// marked; choosing Light switches the palette and saves the choice; Islands Dark stays selectable
/// for comparison; switching back restores IDECraft Dark.
fn theme_steps() -> Vec<Step> {
    use idecraft_workbench::palette::{Palette, ThemeId, current_theme};
    let saved = || {
        FIXTURE.get().and_then(|d| {
            idecraft_core::chosen_theme(
                &d.join(".idecraft/ide-settings.toml"),
                Some(&d.join(".idecraft/theme")),
            )
        })
    };
    vec![
        Step::Key("ctrl-`", None),
        Step::Expect(
            "ctrl-` opens the theme popup with IDECraft Dark, the default, marked",
            Box::new(|b, cx| {
                b.themes_popup().is_some_and(|v| {
                    v.read(cx).rows()
                        == [
                            "✓ IDECraft Dark",
                            "IDECraft Light",
                            "Islands Dark",
                            "Islands Light",
                            "Dark",
                            "Light",
                        ]
                }) && Palette::current().background == 0x2e3440
            }),
        ),
        Step::Key("6", None),
        Step::Expect(
            "choosing Light switches the palette and saves the choice",
            Box::new(move |b, _| {
                b.themes_popup().is_none()
                    && current_theme() == ThemeId::Light
                    && !Palette::current().dark
                    && Palette::current().background == 0xffffff
                    && saved().as_deref() == Some("Light")
            }),
        ),
        Step::Act(Box::new(|b, window, cx| b.open_themes(window, cx))),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the theme popup without switching",
            Box::new(|b, _| b.themes_popup().is_none() && current_theme() == ThemeId::Light),
        ),
        Step::Act(Box::new(|b, _, cx| {
            b.choose_theme(ThemeId::IslandsDark, cx)
        })),
        Step::Expect(
            "Islands Dark stays selectable and is saved",
            Box::new(move |_, _| {
                current_theme() == ThemeId::IslandsDark
                    && Palette::current().dark
                    && Palette::current().background == 0x191a1c
                    && saved().as_deref() == Some("Islands Dark")
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            b.choose_theme(ThemeId::IdecraftDark, cx)
        })),
        Step::Expect(
            "switching back restores IDECraft Dark",
            Box::new(move |_, _| {
                current_theme() == ThemeId::IdecraftDark
                    && Palette::current().accent.rgb == 0x88c0d0
                    && saved().as_deref() == Some("IDECraft Dark")
            }),
        ),
        // Islands Light and following the system (SPEC-0131).
        Step::Act(Box::new(|b, _, cx| {
            b.choose_theme(ThemeId::IslandsLight, cx)
        })),
        Step::Expect(
            "Islands Light draws light islands",
            Box::new(|_, _| {
                current_theme() == ThemeId::IslandsLight
                    && !Palette::current().dark
                    && Palette::current().island_gap > 0.0
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_settings(
                idecraft_workbench::settings_model::Page::Appearance,
                window,
                cx,
            );
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.toggle_sync(cx));
            }
        })),
        Step::Expect(
            "Sync with OS shows the theme for the system's appearance",
            Box::new(|b, cx| {
                let dark = idecraft_workbench::appearance::os_is_dark(cx);
                b.settings_dialog()
                    .is_some_and(|v| v.read(cx).ide().appearance.sync_with_os)
                    && current_theme()
                        == if dark {
                            ThemeId::IdecraftDark
                        } else {
                            ThemeId::IdecraftLight
                        }
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            // A dark theme chosen while following the system becomes the dark one.
            b.choose_theme(ThemeId::Dark, cx);
        })),
        Step::Expect(
            "choosing a theme while following the system sets it for its kind",
            Box::new(|_, cx| {
                let s = idecraft_workbench::appearance::ide_settings(cx);
                s.appearance.dark_theme == "Dark"
                    && (current_theme() == ThemeId::Dark)
                        == idecraft_workbench::appearance::os_is_dark(cx)
            }),
        ),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(v) = b.settings_dialog() {
                v.update(cx, |v, cx| v.toggle_sync(cx));
            }
        })),
        Step::Expect(
            "switching Sync with OS off shows the theme chosen without it again (SPEC-0131 v4)",
            Box::new(move |_, cx| {
                !idecraft_workbench::appearance::ide_settings(cx)
                    .appearance
                    .sync_with_os
                    && current_theme() == ThemeId::IslandsLight
                    && saved().as_deref() == Some("Islands Light")
            }),
        ),
        // Back to the state the following steps start from; not a user action.
        Step::Setup(Box::new(|b, _, cx| {
            let _ = idecraft_workbench::appearance::set_ide_settings(Default::default(), cx);
            b.choose_theme(ThemeId::IdecraftDark, cx);
        })),
        Step::Key("escape", None),
        Step::Expect(
            "without syncing IDECraft Dark is back",
            Box::new(|b, cx| {
                b.settings_dialog().is_none()
                    && current_theme() == ThemeId::IdecraftDark
                    && !idecraft_workbench::appearance::ide_settings(cx)
                        .appearance
                        .sync_with_os
            }),
        ),
    ]
}

/// The project popup (SPEC-0094): it lists Open… and the other recent projects; choosing one opens
/// it in a window of its own.
fn projects_steps() -> Vec<Step> {
    vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let other = dir.join("other-project");
                let _ = std::fs::create_dir_all(&other);
                if let Ok(other) = other.canonicalize() {
                    let _ = idecraft_core::remember_project(
                        &dir.join(".idecraft/recent-projects"),
                        &other,
                    );
                }
            }
            b.open_projects(window, cx);
        })),
        Step::Expect(
            "the project popup lists its actions, this open project and the other recent project",
            Box::new(|b, cx| {
                b.projects_popup().is_some_and(|v| {
                    let rows = v.read(cx).rows();
                    rows[..4]
                        == [
                            "New Project…",
                            "Open…",
                            "Clone Repository…",
                            "Remote Development…",
                        ]
                        && rows.iter().any(|r| r.ends_with("(current)"))
                        && rows.last().is_some_and(|r| r == "other-project")
                })
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "escape closes the project popup",
            Box::new(|b, _| b.projects_popup().is_none()),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.open_projects(window, cx);
            if let Some(v) = b.projects_popup() {
                v.update(cx, |v, cx| {
                    let last = v.rows().len().saturating_sub(1);
                    v.choose(last, cx)
                });
            }
        })),
        Step::WaitFor(
            "choosing a recent project opens it in a new window",
            Box::new(|_, cx| cx.windows().len() == 2),
        ),
        Step::Act(Box::new(|_, window, cx| {
            let this = window.window_handle();
            for w in cx.windows() {
                if w != this {
                    let _ = w.update(cx, |_, window, _| window.remove_window());
                }
            }
        })),
        Step::WaitFor(
            "the other window closes",
            Box::new(|_, cx| cx.windows().len() == 1),
        ),
    ]
}

/// Run configurations detected from build files (SPEC-0087 v2): an npm script of `package.json` is
/// offered in the chooser after the configurations of `run.toml`, and runs.
fn detected_run_steps() -> Vec<Step> {
    let mac = cfg!(target_os = "macos");
    let choose = if mac { "ctrl-alt-r" } else { "alt-shift-f10" };
    let has_npm = std::process::Command::new("npm")
        .arg("--version")
        .output()
        .is_ok();
    let mut steps = vec![
        Step::Act(Box::new(|b, window, cx| {
            if let Some(dir) = FIXTURE.get() {
                let _ = std::fs::write(
                    dir.join("package.json"),
                    "{\"name\": \"fixture\", \"scripts\": {\"greet\": \"echo hello-from-npm\"}}\n",
                );
                // The configuration the group `python-debug` leaves behind, for a run without
                // that group.
                let run = dir.join(".idecraft/run.toml");
                if !std::fs::read_to_string(&run).is_ok_and(|t| t.contains("\"pydebug\"")) {
                    let _ = std::fs::create_dir_all(dir.join(".idecraft"));
                    let _ = std::fs::write(
                        &run,
                        "[[run]]\nname = \"pydebug\"\ncommand = [\"python3\", \"dbg.py\"]\n",
                    );
                }
                b.focus_editor(window, cx);
            }
        })),
        Step::Key(choose, None),
        Step::Expect(
            "the chooser offers the npm script after the configurations of run.toml",
            Box::new(|b, cx| {
                b.run_chooser().is_some_and(|c| {
                    let names = c.read(cx).names();
                    names.first().map(String::as_str) == Some("pydebug")
                        && names.iter().any(|n| n == "npm run greet")
                })
            }),
        ),
    ];
    if has_npm {
        steps.extend([
            Step::Key("down", None),
            Step::Key("enter", None),
            Step::WaitFor(
                "the detected npm script runs",
                Box::new(|b, cx| {
                    b.run_view().is_some_and(|v| {
                        v.read(cx).selected_output().is_some_and(|(name, text)| {
                            name == "npm run greet" && text.contains("hello-from-npm")
                        })
                    })
                }),
            ),
        ]);
    } else {
        steps.push(Step::Key("escape", None));
    }
    steps.push(group("detected-run-debug"));
    // ⌃D in the chooser acts on the highlighted entry, not on the configuration selected before
    // (D53).
    let debug = if mac { "ctrl-d" } else { "shift-f9" };
    steps.extend([
        Step::Act(Box::new(|b, window, cx| {
            b.select_run_configuration("pydebug", cx);
            b.focus_editor(window, cx);
        })),
        Step::Key(choose, None),
        Step::Act(Box::new(|b, _, cx| {
            if let Some(c) = b.run_chooser() {
                c.update(cx, |c, cx| c.highlight("npm run greet", cx));
            }
        })),
        Step::Key(debug, None),
        Step::Expect(
            "⌃D in the chooser debugs the highlighted entry and closes the chooser",
            Box::new(|b, _| {
                b.run_chooser().is_none()
                    && b.selected_run_configuration().as_deref() == Some("npm run greet")
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.menu_action("Stop", window, cx);
        })),
        group("run-widget-select"),
        // The run widget's list only selects (D23).
        Step::Act(Box::new(|b, window, cx| {
            b.open_run_chooser_as(true, window, cx);
            if let Some(c) = b.run_chooser() {
                c.update(cx, |c, cx| c.highlight("pydebug", cx));
            }
        })),
        Step::Key("enter", None),
        Step::Expect(
            "Enter in the run widget's list selects the configuration without starting it",
            Box::new(|b, _| {
                b.run_chooser().is_none()
                    && b.selected_run_configuration().as_deref() == Some("pydebug")
            }),
        ),
    ]);
    steps
}

/// The run configuration editor (SPEC-0327 AK-5): opened from the run widget's list (entry 0),
/// a Node.js file configuration is added with an argument with a space and an environment
/// variable, OK writes it, and Run gives the program both arguments and the variable.
fn run_config_editor_steps() -> Vec<Step> {
    static SAVED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let mac = cfg!(target_os = "macos");
    let (add, run) = if mac {
        ("cmd-n", "ctrl-r")
    } else {
        ("ctrl-n", "shift-f10")
    };
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let dir = || FIXTURE.get().cloned().unwrap_or_default();
    let mut steps = vec![
        Step::Setup(Box::new(move |b, window, cx| {
            let d = dir();
            let _ = SAVED.set(std::fs::read_to_string(d.join(".idecraft/run.toml")).ok());
            let _ = std::fs::write(
                d.join("args.js"),
                "console.log(JSON.stringify({args: process.argv.slice(2), greeting: process.env.GREETING}));\n",
            );
            b.menu_action("Stop", window, cx);
            b.focus_editor(window, cx);
        })),
        click("run-widget", Click::Left(1)),
        Step::WaitFor(
            "the run widget's list ends with Current File and Edit Configurations…",
            Box::new(|b, cx| {
                b.run_chooser().is_some_and(|c| {
                    let c = c.read(cx);
                    let rows = c.rows();
                    rows.len() >= 2
                        && rows[rows.len() - 2].label() == "Current File"
                        && rows[rows.len() - 1].label() == "Edit Configurations…"
                        && rows[rows.len() - 1].digit == Some(0)
                })
            }),
        ),
        Step::Key("0", None),
        Step::WaitFor(
            "entry 0 opens the configuration editor",
            Box::new(|b, cx| {
                b.run_chooser().is_none()
                    && b.run_config_dialog()
                        .is_some_and(|d| d.read(cx).read_only().is_none())
            }),
        ),
        Step::Key(add, None),
        Step::Expect(
            "Add opens the template chooser",
            Box::new(|b, cx| {
                b.run_config_dialog()
                    .is_some_and(|d| d.read(cx).templates_open())
            }),
        ),
        Step::Key("down", None),
        Step::Key("down", None),
        Step::Key("enter", None),
        Step::Expect(
            "the Node.js file template adds a configuration",
            Box::new(|b, cx| {
                b.run_config_dialog().is_some_and(|d| {
                    let d = d.read(cx);
                    d.model().current().is_some_and(|e| {
                        e.name() == "Node.js file" && e.form.command == "node index.js"
                    })
                })
            }),
        ),
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(d) = b.run_config_dialog() {
                d.update(cx, |d, cx| {
                    d.set_field("Command line", "node args.js alpha \"two words\"", cx);
                    d.set_field("Environment", "GREETING=hello there", cx);
                });
            }
        })),
        click("run-config-ok", Click::Left(1)),
        Step::WaitFor(
            "OK writes the configuration to run.toml and closes the editor",
            Box::new(move |b, _| {
                let text =
                    std::fs::read_to_string(dir().join(".idecraft/run.toml")).unwrap_or_default();
                b.run_config_dialog().is_none()
                    && idecraft_core::parse_run_configs(&text)
                        .configs
                        .iter()
                        .any(|c| {
                            c.name == "Node.js file"
                                && c.command == ["node", "args.js", "alpha", "two words"]
                                && c.env.get("GREETING").map(String::as_str) == Some("hello there")
                        })
            }),
        ),
    ];
    if has_node {
        steps.extend([
            Step::Setup(Box::new(|b, window, cx| {
                b.select_run_configuration("Node.js file", cx);
                b.focus_editor(window, cx);
            })),
            Step::Key(run, None),
            Step::WaitFor(
                "the program receives both arguments and the variable",
                Box::new(|b, cx| {
                    b.run_view().is_some_and(|v| {
                        v.read(cx).selected_output().is_some_and(|(name, text)| {
                            name == "Node.js file"
                                && text.contains("node args.js alpha 'two words'")
                                && text.contains(
                                    "{\"args\":[\"alpha\",\"two words\"],\"greeting\":\"hello there\"}",
                                )
                        })
                    })
                }),
            ),
        ]);
    } else {
        eprintln!("run configuration editor: node is missing, the run is skipped");
    }
    steps.push(Step::Setup(Box::new(move |b, window, cx| {
        let d = dir();
        match SAVED.get().cloned().flatten() {
            Some(text) => {
                let _ = std::fs::write(d.join(".idecraft/run.toml"), text);
            }
            None => {
                let _ = std::fs::remove_file(d.join(".idecraft/run.toml"));
            }
        }
        let _ = std::fs::remove_file(d.join("args.js"));
        b.focus_editor(window, cx);
    })));
    steps
}

/// What "Open browser" would have opened during the self-test: the system's opener is replaced
/// for these steps, so no page opens on the machine that runs them.
static RUN_PARITY_OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Folders, templates per type, instances and before-launch tasks of run configurations in the
/// window (SPEC-0327 v3 AK-12, SPEC-0087 v9 AK-11, SPEC-0047 v4 AK-14). The folder and template
/// controls are clicked by their test ids, which only the UI driver resolves (SPEC-0319
/// behaviour 44): without `IDECRAFT_SELFTEST_DRIVER=1` those two groups only say so. The
/// question of a second start and the before-launch steps go by keys and by the question's
/// buttons, with or without the driver.
fn run_config_parity_steps() -> Vec<Step> {
    static SAVED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    static FIRST_START: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    const RUN_TOML: &str = r#"[[run]]
name = "rc-sleeper"
command = ["sh", "-c", "echo started-$$; sleep 30"]

[[run]]
name = "rc-many"
command = ["sleep", "30"]
allow_multiple = true

[[run]]
name = "rc-tasks"
command = ["sh", "-c", "echo main-ran"]
before_launch = [
    { tool = ["sh", "-c", "echo tool-ran"] },
    { browser = "http://localhost:1/idecraft-selftest-page" },
]

[[run]]
name = "rc-fails"
command = ["sh", "-c", "echo never-ran"]
before_launch = [{ tool = ["sh", "-c", "exit 3"] }]
"#;
    let mac = cfg!(target_os = "macos");
    let (add, run) = if mac {
        ("cmd-n", "ctrl-r")
    } else {
        ("ctrl-n", "shift-f10")
    };
    let driven = std::env::var("IDECRAFT_SELFTEST_DRIVER")
        .is_ok_and(|v| !matches!(v.as_str(), "" | "0" | "off"));
    fn dir() -> PathBuf {
        FIXTURE.get().cloned().unwrap_or_default()
    }
    fn file_text() -> String {
        std::fs::read_to_string(dir().join(".idecraft/run.toml")).unwrap_or_default()
    }
    fn folder_in_file(name: &str) -> Option<String> {
        idecraft_core::parse_run_configs(&file_text())
            .configs
            .into_iter()
            .find(|c| c.name == name)
            .and_then(|c| c.folder)
    }
    fn folder_in_dialog(b: &WorkbenchView, cx: &gpui::App, name: &str) -> Option<String> {
        let d = b.run_config_dialog()?;
        let m = d.read(cx).model();
        let i = m.entries.iter().position(|e| e.name() == name)?;
        m.folder_of(i)
    }
    fn dialog_folders(b: &WorkbenchView, cx: &gpui::App) -> Option<Vec<String>> {
        b.run_config_dialog().map(|d| d.read(cx).model().folders())
    }
    fn running(b: &WorkbenchView, cx: &gpui::App, name: &str) -> usize {
        b.run_view()
            .map_or(0, |v| v.read(cx).running_instances(name))
    }
    fn output(b: &WorkbenchView, cx: &gpui::App, name: &str) -> String {
        b.run_view()
            .and_then(|v| v.read(cx).output_of(name))
            .unwrap_or_default()
    }
    fn asks_rerun(b: &WorkbenchView) -> bool {
        b.drawn_question().is_some_and(|q| {
            q.message.contains("rc-sleeper")
                && q.message.contains("already running")
                && q.buttons == ["Stop and Rerun", "Cancel"]
        })
    }
    // A click on the run widget. The widget is in the window's title bar, where the system
    // takes two clicks at one place within its double-click time as a double click and zooms
    // the window; the second click then opens nothing. These steps are faster than a hand, so
    // a click on the widget waits until the one before is a second old. The first of them
    // counts from its own start: a group before these may just have clicked the widget.
    static WIDGET_CLICKED: std::sync::Mutex<Option<std::time::Instant>> =
        std::sync::Mutex::new(None);
    let click_run_widget = || {
        vec![
            Step::WaitFor(
                "the last click on the run widget is a second ago",
                Box::new(|_, _| {
                    WIDGET_CLICKED
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get_or_insert_with(std::time::Instant::now)
                        .elapsed()
                        >= std::time::Duration::from_secs(1)
                }),
            ),
            Step::Setup(Box::new(|b, window, cx| {
                *WIDGET_CLICKED.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(std::time::Instant::now());
                b.focus_editor(window, cx);
            })),
            click("run-widget", Click::Left(1)),
        ]
    };
    // The editor as a user opens it: the run widget's list, entry 0.
    let open_editor = || {
        let mut steps = click_run_widget();
        steps.extend([
            Step::WaitFor(
                "the run widget's list is open",
                Box::new(|b, _| b.run_chooser().is_some()),
            ),
            Step::Key("0", None),
            Step::WaitFor(
                "entry 0 opens the configuration editor for writing",
                Box::new(|b, cx| {
                    b.run_chooser().is_none()
                        && b.run_config_dialog()
                            .is_some_and(|d| d.read(cx).read_only().is_none())
                }),
            ),
        ]);
        steps
    };
    let start = |name: &'static str| {
        vec![
            Step::Setup(Box::new(move |b, window, cx| {
                b.select_run_configuration(name, cx);
                b.focus_editor(window, cx);
            })),
            Step::Key(run, None),
        ]
    };
    // Each group starts from the same four configurations and puts the fixture's own file back
    // at its end, so that any of them runs alone (`IDECRAFT_SELFTEST_ONLY`).
    let prepare = || {
        Step::Setup(Box::new(|b, window, cx| {
            let d = dir();
            let _ = SAVED.set(std::fs::read_to_string(d.join(".idecraft/run.toml")).ok());
            let _ = std::fs::create_dir_all(d.join(".idecraft"));
            let _ = std::fs::write(d.join(".idecraft/run.toml"), RUN_TOML);
            RUN_PARITY_OPENED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            idecraft_core::set_browser_opener(Some(std::sync::Arc::new(|url: &str| {
                RUN_PARITY_OPENED
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(url.to_owned());
                Ok(())
            })));
            b.menu_action("Stop", window, cx);
            b.select_run_configuration("rc-sleeper", cx);
            b.focus_editor(window, cx);
        }))
    };
    let restore = || {
        Step::Setup(Box::new(|b, window, cx| {
            idecraft_core::set_browser_opener(None);
            if let Some(v) = b.run_view() {
                v.update(cx, |v, _| {
                    v.stop_all();
                });
            }
            let d = dir();
            match SAVED.get().cloned().flatten() {
                Some(text) => {
                    let _ = std::fs::write(d.join(".idecraft/run.toml"), text);
                }
                None => {
                    let _ = std::fs::remove_file(d.join(".idecraft/run.toml"));
                }
            }
            b.select_run_configuration("hello", cx);
            b.focus_editor(window, cx);
        }))
    };
    let mut steps = vec![prepare()];

    // --- Folders (SPEC-0327 v3 behaviour 15).
    if driven {
        steps.extend(open_editor());
        steps.extend([
            click("run-config-new-folder", Click::Left(1)),
            Step::WaitFor(
                "New Folder makes a folder without configurations and shows its pane",
                Box::new(|b, cx| {
                    dialog_folders(b, cx).is_some_and(|f| f == ["New Folder"])
                        && b.run_config_dialog().is_some_and(|d| {
                            let m = d.read(cx).model();
                            m.selected_folder.as_deref() == Some("New Folder") && !m.dirty()
                        })
                }),
            ),
            Step::Setup(Box::new(|b, _, cx| {
                if let Some(d) = b.run_config_dialog() {
                    d.update(cx, |d, cx| d.set_field("Folder name", "Jobs", cx));
                }
            })),
            click("run-config-folder-rename", Click::Left(1)),
            Step::WaitFor(
                "Rename gives the folder the typed name",
                Box::new(|b, cx| dialog_folders(b, cx).is_some_and(|f| f == ["Jobs"])),
            ),
            click("run-config-row-rc-sleeper", Click::Left(1)),
            click("run-config-move-to-Jobs", Click::Left(1)),
            Step::WaitFor(
                "a click on the folder under Folder puts the configuration into it",
                Box::new(|b, cx| folder_in_dialog(b, cx, "rc-sleeper").as_deref() == Some("Jobs")),
            ),
            click("run-config-row-rc-many", Click::Left(1)),
            click("run-config-move-to-Jobs", Click::Left(1)),
            Step::WaitFor(
                "a second configuration is in the folder",
                Box::new(|b, cx| folder_in_dialog(b, cx, "rc-many").as_deref() == Some("Jobs")),
            ),
            click("run-config-move-out", Click::Left(1)),
            Step::WaitFor(
                "No folder takes the configuration out again",
                Box::new(|b, cx| {
                    folder_in_dialog(b, cx, "rc-many").is_none()
                        && folder_in_dialog(b, cx, "rc-sleeper").as_deref() == Some("Jobs")
                }),
            ),
            click("run-config-ok", Click::Left(1)),
            Step::WaitFor(
                "OK writes the folder of the configuration to run.toml",
                Box::new(|b, _| {
                    b.run_config_dialog().is_none()
                        && folder_in_file("rc-sleeper").as_deref() == Some("Jobs")
                        && folder_in_file("rc-many").is_none()
                }),
            ),
        ]);
        steps.extend(click_run_widget());
        steps.extend([
            Step::WaitFor(
                "the run widget's list shows the configuration under its folder's heading",
                Box::new(|b, cx| {
                    b.run_chooser().is_some_and(|c| {
                        c.read(cx).rows().iter().any(|r| {
                            r.label() == "rc-sleeper"
                                && r.folder.as_deref() == Some("Jobs")
                                && r.heading.as_deref() == Some("Jobs")
                        })
                    })
                }),
            ),
            Step::Key("0", None),
            Step::WaitFor(
                "the editor opens again",
                Box::new(|b, _| b.run_config_dialog().is_some()),
            ),
            click("run-config-folder-Jobs", Click::Left(1)),
            click("run-config-folder-remove", Click::Left(1)),
            Step::WaitFor(
                "Remove Folder keeps the configuration and lists it on its own",
                Box::new(|b, cx| {
                    dialog_folders(b, cx).is_some_and(|f| f.is_empty())
                        && b.run_config_dialog().is_some_and(|d| {
                            d.read(cx)
                                .model()
                                .entries
                                .iter()
                                .any(|e| e.name() == "rc-sleeper")
                        })
                }),
            ),
            click("run-config-ok", Click::Left(1)),
            Step::WaitFor(
                "run.toml has no folder any more and still all four configurations",
                Box::new(|b, _| {
                    let configs = idecraft_core::parse_run_configs(&file_text());
                    b.run_config_dialog().is_none()
                        && configs.problems.is_empty()
                        && configs.configs.len() == 4
                        && configs.configs.iter().all(|c| c.folder.is_none())
                }),
            ),
        ]);
    } else {
        eprintln!(
            "run-config-folders: the folder controls are clicked by test id and need IDECRAFT_SELFTEST_DRIVER=1; skipped"
        );
    }

    steps.push(restore());

    // --- Templates per type (SPEC-0327 v3 behaviour 16).
    steps.push(group("run-config-templates"));
    steps.push(prepare());
    if driven {
        steps.extend(open_editor());
        steps.extend([
            click("run-config-templates", Click::Left(1)),
            click("run-config-type-shell_script", Click::Left(1)),
            Step::WaitFor(
                "Templates lists the types; a click shows the type's template",
                Box::new(|b, cx| {
                    b.run_config_dialog().is_some_and(|d| {
                        let d = d.read(cx);
                        d.templates_shown()
                            && d.model().template_selected
                                == idecraft_core::RunTemplate::ShellScript
                    })
                }),
            ),
            Step::Setup(Box::new(|b, _, cx| {
                if let Some(d) = b.run_config_dialog() {
                    d.update(cx, |d, cx| {
                        d.set_field("Options", "-e", cx);
                        d.set_field("Environment", "RC_TEMPLATE=from-template", cx);
                        d.set_field("Timeout (seconds)", "45", cx);
                    });
                }
            })),
            click("run-config-allow-multiple", Click::Left(1)),
            click("run-config-apply", Click::Left(1)),
            Step::WaitFor(
                "Apply writes the template of the type to run.toml",
                Box::new(|_, _| {
                    let t = idecraft_core::parse_run_templates(&file_text());
                    t.problems.is_empty()
                        && t.templates
                            .get(&idecraft_core::RunTemplate::ShellScript)
                            .is_some_and(|t| {
                                t.options == ["-e"]
                                    && t.env.get("RC_TEMPLATE").map(String::as_str)
                                        == Some("from-template")
                                    && t.allow_multiple
                                    && t.timeout == Some(45)
                            })
                }),
            ),
            click("run-config-configurations", Click::Left(1)),
            Step::WaitFor(
                "Configurations shows the configurations again",
                Box::new(|b, cx| {
                    b.run_config_dialog()
                        .is_some_and(|d| !d.read(cx).templates_shown())
                }),
            ),
            Step::Key(add, None),
            Step::WaitFor(
                "Add opens the chooser of types",
                Box::new(|b, cx| {
                    b.run_config_dialog()
                        .is_some_and(|d| d.read(cx).templates_open())
                }),
            ),
            click("run-config-new-shell_script", Click::Left(1)),
            Step::WaitFor(
                "a new Shell script configuration is a copy of the type's template",
                Box::new(|b, cx| {
                    b.run_config_dialog().is_some_and(|d| {
                        d.read(cx)
                            .model()
                            .current()
                            .and_then(|e| e.form.config().ok())
                            .is_some_and(|c| {
                                c.name == "Shell script"
                                    && c.command == ["sh", "-e", "script.sh"]
                                    && c.env.get("RC_TEMPLATE").map(String::as_str)
                                        == Some("from-template")
                                    && c.allow_multiple
                                    && c.timeout == Some(45)
                            })
                    })
                }),
            ),
            Step::Setup(Box::new(|b, _, cx| {
                if let Some(d) = b.run_config_dialog() {
                    d.update(cx, |d, cx| {
                        d.set_field(
                            "Command line",
                            "sh -e -c 'echo template-says-$RC_TEMPLATE'",
                            cx,
                        );
                    });
                }
            })),
            click("run-config-run", Click::Left(1)),
            Step::WaitFor(
                "Run starts it with the template's environment",
                Box::new(|b, cx| {
                    b.run_config_dialog().is_none()
                        && output(b, cx, "Shell script").contains("template-says-from-template")
                }),
            ),
        ]);
        steps.extend(open_editor());
        steps.extend([
            click("run-config-templates", Click::Left(1)),
            click("run-config-type-shell_script", Click::Left(1)),
            click("run-config-template-reset", Click::Left(1)),
            click("run-config-ok", Click::Left(1)),
            Step::WaitFor(
                "Reset and OK remove the template; the configuration made from it keeps its copy",
                Box::new(|b, _| {
                    let text = file_text();
                    let configs = idecraft_core::parse_run_configs(&text);
                    b.run_config_dialog().is_none()
                        && idecraft_core::parse_run_templates(&text)
                            .templates
                            .is_empty()
                        && configs
                            .configs
                            .iter()
                            .any(|c| c.name == "Shell script" && c.env.contains_key("RC_TEMPLATE"))
                }),
            ),
        ]);
    } else {
        eprintln!(
            "run-config-templates: the template controls are clicked by test id and need IDECRAFT_SELFTEST_DRIVER=1; skipped"
        );
    }

    steps.push(restore());

    // --- Instances (SPEC-0047 v3 behaviour 10, SPEC-0087 v9 behaviour 17).
    steps.push(group("run-config-instances"));
    steps.push(prepare());
    steps.extend(start("rc-sleeper"));
    steps.extend([
        Step::WaitFor(
            "the configuration runs once",
            Box::new(|b, cx| {
                let text = output(b, cx, "rc-sleeper");
                let first = text
                    .lines()
                    .find(|l| l.starts_with("started-"))
                    .map(str::to_owned);
                if let Some(first) = &first {
                    *FIRST_START.lock().unwrap_or_else(|e| e.into_inner()) = first.clone();
                }
                first.is_some() && running(b, cx, "rc-sleeper") == 1
            }),
        ),
        Step::Key(run, None),
        Step::WaitFor(
            "a second start of a running configuration asks: Stop and Rerun, Cancel",
            Box::new(|b, _| asks_rerun(b)),
        ),
        click("drawn-question-button-1", Click::Left(1)),
        Step::WaitFor(
            "Cancel leaves the running instance alone and starts nothing",
            Box::new(|b, cx| {
                let first = FIRST_START
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                b.drawn_question().is_none()
                    && running(b, cx, "rc-sleeper") == 1
                    && output(b, cx, "rc-sleeper").contains(&first)
                    && b.run_view().is_some_and(|v| {
                        v.read(cx)
                            .tabs()
                            .iter()
                            .filter(|(_, c)| c == "rc-sleeper")
                            .count()
                            == 1
                    })
            }),
        ),
        Step::Key(run, None),
        Step::WaitFor("the second start asks again", Box::new(|b, _| asks_rerun(b))),
        click("drawn-question-button-0", Click::Left(1)),
        Step::WaitFor(
            "Stop and Rerun ends the running instance and starts the configuration again in its tab",
            Box::new(|b, cx| {
                let first = FIRST_START
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let text = output(b, cx, "rc-sleeper");
                b.drawn_question().is_none()
                    && running(b, cx, "rc-sleeper") == 1
                    && text.lines().any(|l| l.starts_with("started-") && l != first)
                    && b.run_view().is_some_and(|v| {
                        v.read(cx)
                            .tabs()
                            .iter()
                            .filter(|(_, c)| c == "rc-sleeper")
                            .count()
                            == 1
                    })
            }),
        ),
    ]);
    steps.extend(start("rc-many"));
    steps.push(Step::WaitFor(
        "a configuration that allows several instances runs once",
        Box::new(|b, cx| running(b, cx, "rc-many") == 1),
    ));
    steps.extend([
        Step::Key(run, None),
        Step::WaitFor(
            "a second start runs beside the first, in a tab of its own named with a counter",
            Box::new(|b, cx| {
                b.drawn_question().is_none()
                    && running(b, cx, "rc-many") == 2
                    && b.run_view().is_some_and(|v| {
                        let titles: Vec<String> = v
                            .read(cx)
                            .tabs()
                            .into_iter()
                            .filter(|(_, c)| c == "rc-many")
                            .map(|(t, _)| t)
                            .collect();
                        titles == ["rc-many", "rc-many (2)"]
                    })
            }),
        ),
        Step::Setup(Box::new(|b, _, cx| {
            if let Some(v) = b.run_view() {
                v.update(cx, |v, _| {
                    v.stop_all();
                });
            }
        })),
        Step::WaitFor(
            "the instances end when stopped",
            Box::new(|b, cx| running(b, cx, "rc-many") == 0 && running(b, cx, "rc-sleeper") == 0),
        ),
    ]);

    steps.push(restore());

    // --- Before-launch tasks (SPEC-0047 v3 behaviours 8 and 9).
    steps.push(group("run-config-before-launch"));
    steps.push(prepare());
    steps.extend(start("rc-tasks"));
    steps.push(Step::WaitFor(
        "the external tool runs before the configuration, the page is opened after its start",
        Box::new(|b, cx| {
            let text = output(b, cx, "rc-tasks");
            let opened = RUN_PARITY_OPENED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            matches!(
                (text.find("tool-ran"), text.find("main-ran")),
                (Some(tool), Some(main)) if tool < main
            ) && text.contains("Opened http://localhost:1/idecraft-selftest-page in the browser")
                && opened == ["http://localhost:1/idecraft-selftest-page"]
        }),
    ));
    steps.extend(start("rc-fails"));
    steps.push(Step::WaitFor(
        "a failing before-launch step stops the launch and the output names the step",
        Box::new(|b, cx| {
            let text = output(b, cx, "rc-fails");
            text.contains("Run stopped: rc-fails · sh failed") && !text.contains("never-ran")
        }),
    ));
    if driven {
        steps.extend(open_editor());
        steps.extend([
            click("run-config-row-rc-tasks", Click::Left(1)),
            click("run-config-before-add", Click::Left(1)),
            click("run-config-task-build", Click::Left(1)),
            Step::WaitFor(
                "Before launch's Add offers Build and adds it after the tasks that are there",
                Box::new(|b, cx| {
                    b.run_config_dialog().is_some_and(|d| {
                        d.read(cx).model().current().is_some_and(|e| {
                            e.name() == "rc-tasks"
                                && e.form.tasks.lines().collect::<Vec<_>>()
                                    == [
                                        "tool sh -c 'echo tool-ran'",
                                        "browser http://localhost:1/idecraft-selftest-page",
                                        "build",
                                    ]
                        })
                    })
                }),
            ),
            Step::Setup(Box::new(|b, _, cx| {
                if let Some(d) = b.run_config_dialog() {
                    d.update(cx, |d, cx| {
                        d.set_field("Before-launch tasks", "browser vscode://x/y", cx)
                    });
                }
            })),
            click("run-config-apply", Click::Left(1)),
            Step::WaitFor(
                "a URL that is not http or https is refused in the form and nothing is written",
                Box::new(|b, cx| {
                    b.run_config_dialog().is_some_and(|d| {
                        d.read(cx)
                            .problems()
                            .iter()
                            .any(|p| p.contains("only http, https and file URLs"))
                    }) && !file_text().contains("vscode")
                }),
            ),
            click("run-config-cancel", Click::Left(1)),
            Step::WaitFor(
                "Cancel with changes asks first",
                Box::new(|b, _| {
                    b.drawn_question()
                        .is_some_and(|q| q.buttons == ["Discard", "Cancel"])
                }),
            ),
            click("drawn-question-button-0", Click::Left(1)),
            Step::WaitFor(
                "Discard closes the editor without writing",
                Box::new(|b, _| b.run_config_dialog().is_none() && !file_text().contains("build")),
            ),
        ]);
    }
    steps.push(restore());
    steps
}

/// The other way to a secondary click in the mouse group: a Control-click on macOS, the right
/// button elsewhere, where a Control-click is a primary click (SPEC-0275 behaviour 2, AK-2).
const SECONDARY_ALTERNATIVE: Click = if cfg!(target_os = "macos") {
    Click::ControlLeft
} else {
    Click::Right
};

/// Mouse handling (SPEC-0275): real right clicks, two-finger clicks and Control clicks open the
/// context menus; New, Rename and Delete in the project tree; one click on a tree's arrow toggles,
/// a double click opens; the editor and the database tree have their menus. Works in its own
/// folder `mouse/`, so it runs at the end of the full self-test as well as alone.
fn mouse_steps() -> Vec<Step> {
    use idecraft_workbench::MenuPlace;
    fn ends(path: Option<PathBuf>, name: &str) -> bool {
        path.is_some_and(|p| p.ends_with(name))
    }
    fn menu_has(b: &WorkbenchView, place: MenuPlace, item: &str) -> bool {
        b.context_menu_items()
            .is_some_and(|(p, items)| p == place && items.iter().any(|(l, on)| l == item && *on))
    }
    fn dir() -> PathBuf {
        FIXTURE.get().cloned().unwrap_or_default()
    }
    /// Shows `rel` in the project tree (scrolled into view).
    fn reveal(rel: &'static str) -> Step {
        Step::Act(Box::new(move |b, window, cx| {
            b.reveal_in_tree(&dir().join(rel), cx);
            b.focus_tree(window, cx);
        }))
    }
    vec![
        // The tabs of the steps before go, so that start.txt is the first tab, in view, as when
        // these steps run alone (a row scrolled to its end has no first tab to click).
        Step::Setup(Box::new(|b, window, cx| {
            b.close_all_tabs_unchecked(window, cx)
        })),
        // A folder of its own with two files, the tool windows out of the way, a.rs open.
        Step::Act(Box::new(|b, window, cx| {
            let d = dir().join("mouse");
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("pick.txt"), "pick\n");
            let _ = std::fs::write(d.join("start.txt"), "start\n");
            b.menu_action("HideAllWindows", window, cx);
            b.focus_tree(window, cx);
            b.open(&d.join("start.txt"), window, cx);
        })),
        reveal("mouse/start.txt"),
        Step::WaitFor(
            "mouse: start.txt is open in a tab",
            Box::new(|b, _| ends(b.active_path(), "start.txt") && b.tab_count() >= 1),
        ),
        click("tab-0", Click::Right),
        Step::WaitFor(
            "a right click on an editor tab opens its menu",
            Box::new(|b, _| b.tab_menu_open() == Some(0)),
        ),
        // A click outside an open menu only closes it (it goes no further), so the tab menu is
        // closed with Escape before the stripe button is clicked.
        Step::Key("escape", None),
        Step::WaitFor(
            "Escape closes the tab menu",
            Box::new(|b, _| b.tab_menu_open().is_none()),
        ),
        // A Control-click is a secondary click on macOS only (SPEC-0275 behaviour 2, AK-2); the
        // Linux variant opens the same menu with the secondary button.
        click("stripe-project", SECONDARY_ALTERNATIVE),
        Step::WaitFor(
            if cfg!(target_os = "macos") {
                "a Control-click on a stripe button opens its menu"
            } else {
                "a right click on a stripe button opens its menu (Control-click is secondary on macOS only)"
            },
            Box::new(|b, _| {
                b.stripe_menu_open() == Some(idecraft_workbench::tool_windows::ToolWindow::Project)
            }),
        ),
        click("editor-text", Click::Left(1)),
        Step::WaitFor(
            "a click elsewhere closes the stripe button's menu",
            Box::new(|b, _| b.stripe_menu_open().is_none()),
        ),
        // The project tree's menu (behaviour 1) and New › SQL File (behaviours 3 and 4).
        reveal("mouse"),
        click("tree-mouse", Click::Right),
        Step::WaitFor(
            "a right click on a folder in the project tree opens its menu",
            Box::new(|b, _| {
                [
                    "New › SQL File",
                    "New › Directory",
                    "Rename…",
                    "Delete…",
                    "Open in Terminal",
                ]
                .iter()
                .all(|i| menu_has(b, MenuPlace::ProjectTree, i))
                    && b.tree_selected_path().ends_with("mouse")
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("New › SQL File", window, cx);
        })),
        Step::Expect(
            "New › SQL File asks for the name",
            Box::new(|b, _| {
                b.context_menu_items().is_none()
                    && b.name_prompt_state()
                        .is_some_and(|(t, e)| t == "New SQL File" && e.is_none())
            }),
        ),
        Step::Act(Box::new(|b, _, cx| b.set_name_field("query", cx))),
        Step::Key("enter", None),
        Step::WaitFor(
            "the new SQL file exists in the folder and opens in the editor",
            Box::new(|b, _| {
                dir().join("mouse/query.sql").is_file()
                    && ends(b.active_path(), "mouse/query.sql")
                    && b.name_prompt_state().is_none()
            }),
        ),
        // The same name again is refused, the field stays open with the reason.
        reveal("mouse"),
        click("tree-mouse", SECONDARY_ALTERNATIVE),
        Step::WaitFor(
            if cfg!(target_os = "macos") {
                "a Control-click on a tree row opens the menu too"
            } else {
                "a right click on a tree row opens the menu again (Control-click is secondary on macOS only)"
            },
            Box::new(|b, _| menu_has(b, MenuPlace::ProjectTree, "New › SQL File")),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("New › SQL File", window, cx);
            b.set_name_field("query", cx);
        })),
        Step::Key("enter", None),
        Step::Expect(
            "an existing name is refused and the field stays open",
            Box::new(|b, _| {
                b.name_prompt_state()
                    .is_some_and(|(_, e)| e.is_some_and(|e| e.contains("already exists")))
            }),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the name field",
            Box::new(|b, _| b.name_prompt_state().is_none()),
        ),
        // Rename (behaviour 5) and Delete (behaviour 6) of the new file.
        reveal("mouse/query.sql"),
        click("tree-query.sql", Click::Right),
        Step::WaitFor(
            "the menu of a file offers Rename",
            Box::new(|b, _| menu_has(b, MenuPlace::ProjectTree, "Rename…")),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Rename…", window, cx);
            b.set_name_field("report.sql", cx);
        })),
        Step::Key("enter", None),
        Step::WaitFor(
            "Rename renames the file and its tab",
            Box::new(|b, _| {
                dir().join("mouse/report.sql").is_file()
                    && !dir().join("mouse/query.sql").exists()
                    && ends(b.active_path(), "mouse/report.sql")
            }),
        ),
        reveal("mouse/report.sql"),
        click("tree-report.sql", Click::Right),
        Step::WaitFor(
            "the menu of the renamed file opens",
            Box::new(|b, _| menu_has(b, MenuPlace::ProjectTree, "Delete…")),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Delete…", window, cx);
        })),
        Step::Expect(
            "Delete asks first",
            Box::new(|b, _| {
                b.delete_question().as_deref() == Some("Delete file \"report.sql\"?")
                    && dir().join("mouse/report.sql").exists()
            }),
        ),
        Step::Key("enter", None),
        Step::WaitFor(
            "Enter deletes the file and closes its tab",
            Box::new(|b, _| {
                !dir().join("mouse/report.sql").exists()
                    && b.delete_question().is_none()
                    && !ends(b.active_path(), "report.sql")
            }),
        ),
        // New for the project tree's selection (⌘N there, File › New…; behaviour 3).
        Step::Act(Box::new(|b, window, cx| {
            b.focus_tree(window, cx);
            window.dispatch_action(
                Box::new(idecraft_workbench::menu_bar::IdeAction {
                    id: "NewElement".into(),
                }),
                cx,
            );
        })),
        Step::WaitFor(
            "File › New… (⌘N in the project tree) opens New",
            Box::new(|b, _| menu_has(b, MenuPlace::New, "Java Class")),
        ),
        Step::Key("escape", None),
        Step::Expect(
            "Escape closes the menu",
            Box::new(|b, _| b.context_menu_items().is_none()),
        ),
        // Trees (behaviours 8 and 9): one click on the arrow toggles, one click on a row selects,
        // a double click opens a file.
        reveal("mouse"),
        click("tree-arrow-mouse", Click::Left(1)),
        Step::WaitFor(
            "one click on a folder's arrow toggles it",
            Box::new(|b, _| !b.tree_is_expanded(&dir().join("mouse"))),
        ),
        click("tree-arrow-mouse", Click::Left(1)),
        Step::WaitFor(
            "another click on the arrow toggles it back",
            Box::new(|b, _| b.tree_is_expanded(&dir().join("mouse"))),
        ),
        click("tree-pick.txt", Click::Left(1)),
        Step::WaitFor(
            "one click on a file selects it without opening it",
            Box::new(|b, _| {
                b.tree_selected_path().ends_with("pick.txt") && !ends(b.active_path(), "pick.txt")
            }),
        ),
        click("tree-pick.txt", Click::Left(2)),
        Step::WaitFor(
            "a double click on a file opens it",
            Box::new(|b, _| ends(b.active_path(), "pick.txt")),
        ),
        // The editor's menu (behaviour 14).
        click("editor-text", Click::Right),
        Step::WaitFor(
            "a right click on the editor text opens its menu",
            Box::new(|b, _| {
                [
                    "Show Context Actions",
                    "Paste",
                    "Go To › Declaration or Usages",
                    "Find Usages",
                    "Refactor › Rename…",
                    "Refactor › Extract Method…",
                    "Reformat Code",
                    "Column Selection Mode",
                    "Folding › Collapse All",
                    "Analyze › Inspect Code…",
                    "Generate…",
                    "Open In › Terminal",
                    "Compare with Clipboard",
                ]
                .iter()
                .all(|i| menu_has(b, MenuPlace::Editor, i))
            }),
        ),
        Step::Expect(
            "without a selection the editor menu has no Cut and Copy (SPEC-0275 v6 behaviour 14b)",
            Box::new(|b, _| {
                b.context_menu_items()
                    .is_some_and(|(_, items)| !items.iter().any(|(l, _)| l == "Cut" || l == "Copy"))
            }),
        ),
        Step::Key("escape", None),
        // The database tree (behaviours 8, 9 and 15).
        Step::Act(Box::new(|b, window, cx| {
            let d = dir();
            let db = d.join("mouse.db");
            let _ = std::process::Command::new("sqlite3")
                .arg(&db)
                .arg("CREATE TABLE item (id INTEGER PRIMARY KEY, name TEXT);")
                .status();
            let _ = std::fs::create_dir_all(d.join(".idecraft"));
            let _ = std::fs::write(
                d.join(".idecraft/connections.toml"),
                format!(
                    "[[connection]]\nid = \"mouse\"\nname = \"mouse\"\ndialect = \"sqlite\"\npath = \"{}\"\n",
                    db.display()
                ),
            );
            if b.database_view().is_none() {
                b.toggle_database(window, cx);
            }
            if let Some(v) = b.database_view() {
                v.update(cx, |v, cx| {
                    v.reload();
                    cx.notify()
                });
            }
        })),
        Step::WaitFor(
            "mouse: the database tree lists the connection",
            Box::new(|b, cx| {
                b.database_view()
                    .is_some_and(|v| v.read(cx).visible().first().is_some_and(|r| r.1 == "mouse"))
            }),
        ),
        click("db-arrow-mouse", Click::Left(1)),
        Step::WaitFor(
            "one click on a connection's arrow expands it",
            Box::new(|b, cx| {
                b.database_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.is_expanded("mouse") && v.visible().iter().any(|r| r.1 == "tables")
                })
            }),
        ),
        click("db-row-tables", Click::Left(1)),
        Step::WaitFor(
            "one click on a database row selects it without expanding it",
            Box::new(|b, cx| {
                b.database_view().is_some_and(|v| {
                    let v = v.read(cx);
                    v.selected_row_label().as_deref() == Some("tables") && !v.is_expanded("tables")
                })
            }),
        ),
        click("db-row-tables", Click::Left(2)),
        Step::WaitFor(
            "a double click on a database folder expands it",
            Box::new(|b, cx| {
                b.database_view()
                    .is_some_and(|v| v.read(cx).is_expanded("tables"))
            }),
        ),
        click("db-row-mouse", Click::Right),
        Step::WaitFor(
            "a right click on a connection opens the database menu",
            Box::new(|b, _| {
                [
                    "New › Data Source…",
                    "New › Query Console",
                    "Refresh",
                    "Properties",
                    "Delete…",
                ]
                .iter()
                .all(|i| menu_has(b, MenuPlace::Database, i))
            }),
        ),
        Step::Act(Box::new(|b, window, cx| {
            b.choose_context_item("Refresh", window, cx);
        })),
        Step::Expect(
            "choosing an item closes the menu",
            Box::new(|b, _| b.context_menu_items().is_none()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The self-test's ⌘↑ check (SPEC-0206 behaviour 4) finds the previous prompt's command also
    /// when the prompt line wraps at the terminal's width, at a letter or at a blank.
    #[test]
    fn the_prompt_check_reads_wrapped_rows() {
        let command = "seq -f 'row%g;' 1 200; false";
        // As in the failing window run, with the editor column 490 px wide.
        let wrapped =
            "user@host idecraft-ui-selftest-15600 % s\neq -f 'row%g;' 1 200; false\nrow1;";
        assert!(top_rows_contain(wrapped, 3, command));
        let at_blank = "user@host dir % seq -f 'row%g;' 1 200;\nfalse\nrow1;";
        assert!(top_rows_contain(at_blank, 3, command));
        assert!(top_rows_contain(&format!("% {command}\nrow1;"), 3, command));
        // Not scrolled to the prompt: the command is not in the top rows.
        let below = "row1;\nrow2;\nrow3;\n% seq -f 'row%g;' 1 200; false";
        assert!(!top_rows_contain(below, 3, command));
        assert!(!top_rows_contain("row199;\nrow200;\n% ", 3, command));
    }

    /// The self-test's "breakpoints are saved for the next start" check reads what the workbench
    /// writes on a toggle: `breakpoints.toml` with its envelope (SPEC-0303 behaviour 14), not the
    /// line file of older versions.
    #[test]
    fn the_saved_breakpoint_check_reads_breakpoints_toml() {
        let dir = std::env::temp_dir().join(format!("idecraft-bp-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!breakpoint_saved(&dir, "dbg.c", 8));
        // As `WorkbenchView::breakpoints_changed` saves after ⌘F8 on line 8.
        let lines = std::collections::BTreeMap::from([(dir.join("dbg.c"), vec![7])]);
        idecraft_debugger::saved::save_all(&dir, &lines, &Default::default());
        assert!(
            dir.join(idecraft_debugger::saved::BREAKPOINTS_FILE)
                .exists()
        );
        assert!(
            !dir.join(idecraft_debugger::saved::LEGACY_BREAKPOINTS_FILE)
                .exists()
        );
        assert!(breakpoint_saved(&dir, "dbg.c", 8));
        assert!(!breakpoint_saved(&dir, "dbg.c", 7));
        assert!(!breakpoint_saved(&dir, "other.c", 8));
        assert!(!breakpoint_saved(&dir, "dbg.c", 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tool window's shortcut from the editor (SPEC-0167 v3 behaviour 11): the steps put the
    /// focus in the editor, press once and expect the window still shown, press again and expect
    /// it hidden.
    #[test]
    fn a_shortcut_hides_a_window_on_the_second_press() {
        let steps = shortcut_hides("cmd-4", "focuses", "hides", |_| true);
        let shape: Vec<String> = steps
            .iter()
            .map(|s| match s {
                Step::Act(_) => "act".to_owned(),
                Step::Key(k, _) => format!("key {k}"),
                Step::Expect(n, _) => format!("expect {n}"),
                _ => "other".to_owned(),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "act",
                "key cmd-4",
                "expect focuses",
                "key cmd-4",
                "expect hides"
            ]
        );
    }

    /// Every group has one name, no group is called like a meta group, and every member of a
    /// meta group exists: `IDECRAFT_SELFTEST_ONLY` and `IDECRAFT_SELFTEST_SKIP` can name all of
    /// them.
    #[test]
    fn every_group_has_one_name_and_meta_groups_name_groups() {
        let names = group_names(&SelfTest::blocks(KeyLayout::default()));
        for (i, name) in names.iter().enumerate() {
            assert!(!names[..i].contains(name), "{name} twice");
            assert!(
                crate::selftest_groups::META.iter().all(|(m, _)| m != name),
                "{name} is also a meta group"
            );
        }
        for (meta, members) in crate::selftest_groups::META {
            for m in *members {
                assert!(names.contains(m), "{meta}: {m} is not a group");
            }
        }
        // The names `IDECRAFT_SELFTEST_ONLY` took before all blocks had names.
        for old in [
            "mouse",
            "tabs",
            "tooltips",
            "java-debug",
            "debug-more",
            "java-smart",
            "build-tools",
            "containers",
            "breakpoints",
            "remote-project",
        ] {
            assert!(
                crate::selftest_groups::expand(old, &names).is_ok_and(|g| !g.is_empty()),
                "{old}"
            );
        }
    }

    /// Every step of a full run belongs to exactly one named group: the blocks split at their
    /// marks into the declared groups (an undeclared mark is an error), the spans follow each
    /// other without a gap up to the last step, and no mark is left among the steps.
    #[test]
    fn every_step_belongs_to_exactly_one_named_group() {
        let names = group_names(&SelfTest::blocks(KeyLayout::default()));
        let all = plan(SelfTest::blocks(KeyLayout::default()), |names| {
            crate::selftest_groups::select(names, None, None)
        })
        .expect("declared marks");
        assert_eq!(
            all.spans.iter().map(|(n, ..)| *n).collect::<Vec<_>>(),
            names
        );
        let mut at = 0;
        for (name, end, steps) in &all.spans {
            assert_eq!(at + steps, *end, "{name}");
            at = *end;
        }
        assert_eq!(at, all.steps.len());
        assert!(!all.steps.iter().any(|s| matches!(s, Step::Group(_))));
        // The blocks that are built without looking for tools have steps in every group.
        for name in [
            "editor",
            "terminal",
            "code-insight",
            "git-log",
            "database",
            "profiler-attach",
        ] {
            let (_, _, steps) = all.spans.iter().find(|(n, ..)| *n == name).expect(name);
            assert!(*steps > 0, "{name}");
        }
    }

    /// A selection keeps the steps of its groups only, in run order, and does not build the
    /// blocks it leaves out.
    #[test]
    fn a_plan_keeps_the_selected_groups_and_builds_only_their_blocks() {
        let blocks = || {
            vec![
                Block::new(&["a", "b"], || {
                    vec![
                        Step::Key("1", None),
                        group("b"),
                        Step::Key("2", None),
                        Step::Key("3", None),
                    ]
                }),
                Block::new(&["c"], || panic!("not built")),
                Block::new(&["d"], || vec![Step::Key("4", None)]),
            ]
        };
        let keys = |p: &Plan| -> Vec<&'static str> {
            p.steps
                .iter()
                .map(|s| match s {
                    Step::Key(k, _) => *k,
                    _ => "?",
                })
                .collect()
        };
        let p = plan(blocks(), |names| {
            assert_eq!(names, ["a", "b", "c", "d"]);
            crate::selftest_groups::select(names, Some("d,b,c"), Some("c"))
        })
        .unwrap();
        assert_eq!(keys(&p), ["2", "3", "4"]);
        assert_eq!(p.spans, [("b", 2, 2), ("d", 3, 1)]);
        assert_eq!(p.skipped, ["c"]);
        // A mark the block does not declare is refused.
        let e = plan(vec![Block::new(&["a"], || vec![group("x")])], |names| {
            crate::selftest_groups::select(names, None, None)
        });
        assert!(e.is_err_and(|e| e.contains("\"x\"")));
    }
}
