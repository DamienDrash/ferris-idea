//! Before-launch tasks other than configurations, and the instances of a configuration that run
//! (`SPEC-0047` v3 behaviours 7–10, v4 behaviour 13: "Open browser" may wait for a port): "Build", "Run external tool" and "Open browser" as written
//! in `before_launch` of `.idecraft/run.toml`, the URLs "Open browser" may open, the system's
//! opener, and the rule for starting a configuration that is already running.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::run::RunConfig;

/// The only value of `build`: what Build Project builds.
pub const BUILD_PROJECT: &str = "project";

/// One entry of `before_launch` (behaviour 7): exactly one of `build`, `tool` and `browser` is
/// set; `cwd` belongs to `tool`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeforeTask {
    /// Build first: `"project"` runs the build steps the project model knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Run this program with its arguments first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Vec<String>>,
    /// Working directory of `tool`, workspace-relative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Open this URL in the browser once the configuration has started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    /// With `browser`: open the URL only once this port of the machine accepts connections
    /// (v4 behaviour 13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_port: Option<u16>,
}

/// What a [`BeforeTask`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind<'a> {
    /// Build the project.
    Build,
    /// Run a program in a folder.
    Tool(&'a [String], Option<&'a str>),
    /// Open a URL.
    Browser(&'a str),
}

impl BeforeTask {
    /// "Build".
    pub fn build() -> Self {
        Self {
            build: Some(BUILD_PROJECT.into()),
            ..Self::default()
        }
    }

    /// "Run external tool".
    pub fn tool(command: Vec<String>, cwd: Option<String>) -> Self {
        Self {
            tool: Some(command),
            cwd,
            ..Self::default()
        }
    }

    /// "Open browser".
    pub fn browser(url: &str) -> Self {
        Self {
            browser: Some(url.to_owned()),
            ..Self::default()
        }
    }

    /// "Open browser" once `port` accepts connections.
    pub fn browser_after_port(url: &str, port: u16) -> Self {
        Self {
            browser: Some(url.to_owned()),
            wait_port: Some(port),
            ..Self::default()
        }
    }

    /// What the task is, or why it is none.
    pub fn kind(&self) -> Result<TaskKind<'_>, String> {
        if self.wait_port.is_some() && self.browser.is_none() {
            return Err("wait_port belongs to Open browser".into());
        }
        if self.wait_port == Some(0) {
            return Err("wait_port is a port from 1 to 65535".into());
        }
        match (&self.build, &self.tool, &self.browser) {
            (Some(b), None, None) if self.cwd.is_none() => {
                if b == BUILD_PROJECT {
                    Ok(TaskKind::Build)
                } else {
                    Err(format!("build must be \"{BUILD_PROJECT}\", not {b:?}"))
                }
            }
            (None, Some(t), None) => {
                if t.first().is_none_or(|p| p.trim().is_empty()) {
                    Err("the external tool has no command".into())
                } else {
                    Ok(TaskKind::Tool(t, self.cwd.as_deref()))
                }
            }
            (None, None, Some(u)) if self.cwd.is_none() => Ok(TaskKind::Browser(u)),
            (None, None, None) => Err("a task needs one of build, tool or browser".into()),
            (Some(_), None, None) | (None, None, Some(_)) => {
                Err("cwd belongs to an external tool".into())
            }
            _ => Err("a task is only one of build, tool or browser".into()),
        }
    }

    /// The problem of the task as written, if it has one (behaviour 7).
    pub fn problem(&self) -> Option<String> {
        match self.kind() {
            Err(e) => Some(e),
            Ok(TaskKind::Build) => None,
            Ok(TaskKind::Tool(_, cwd)) => cwd
                .filter(|d| !crate::run::inside(d))
                .map(|d| format!("{d} leaves the workspace")),
            Ok(TaskKind::Browser(url)) => browser_url_problem(url),
        }
    }

    /// The task as one line of the configuration form (`SPEC-0327` v2 behaviour 13): `build`,
    /// `tool [cwd=<folder>] <command line>` or `browser [wait=<port>] <URL>`.
    pub fn line(&self) -> String {
        match self.kind() {
            Ok(TaskKind::Build) => "build".into(),
            Ok(TaskKind::Tool(command, cwd)) => {
                let line = crate::run_edit::join_command_line(command);
                match cwd {
                    Some(d) => format!("tool cwd={} {line}", crate::run_edit::quote_arg(d)),
                    None => format!("tool {line}"),
                }
            }
            Ok(TaskKind::Browser(url)) => match self.wait_port {
                Some(port) => format!("browser wait={port} {url}"),
                None => format!("browser {url}"),
            },
            // A task that is none shows what it has, so the form names its problem.
            Err(_) => match (&self.build, &self.tool, &self.browser) {
                (Some(b), _, _) => format!("build {b}"),
                (_, Some(_), _) => "tool".into(),
                (_, _, Some(u)) => format!("browser {u}"),
                _ => String::new(),
            },
        }
    }

    /// The task a line of the form describes.
    pub fn parse_line(line: &str) -> Result<Self, String> {
        let line = line.trim();
        let (word, rest) = match line.split_once(char::is_whitespace) {
            Some((w, r)) => (w, r.trim()),
            None => (line, ""),
        };
        match word {
            "build" if rest.is_empty() => Ok(Self::build()),
            "build" => Err("build takes nothing after it".into()),
            "tool" => {
                let mut command = crate::run_edit::split_command_line(rest)?;
                let cwd = if rest.starts_with("cwd=") && !command.is_empty() {
                    Some(command.remove(0)["cwd=".len()..].to_owned())
                } else {
                    None
                };
                if command.is_empty() {
                    return Err("write tool <command line>".into());
                }
                Ok(Self::tool(command, cwd))
            }
            "browser" => {
                let (port, url) = match rest.strip_prefix("wait=") {
                    Some(after) => {
                        let (port, url) =
                            after.split_once(char::is_whitespace).unwrap_or((after, ""));
                        let port = port
                            .parse::<u16>()
                            .ok()
                            .filter(|p| *p > 0)
                            .ok_or("wait=<port> takes a port from 1 to 65535")?;
                        (Some(port), url.trim())
                    }
                    None => (None, rest),
                };
                if url.is_empty() || url.contains(char::is_whitespace) {
                    return Err("write browser [wait=<port>] <URL>".into());
                }
                Ok(Self {
                    wait_port: port,
                    ..Self::browser(url)
                })
            }
            _ => Err(format!(
                "{word:?} is no task: write build, tool <command line> or browser [wait=<port>] <URL>"
            )),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The URLs "Open browser" may open (behaviour 9).

/// Where a `file:` URL must start: only pages of the project are opened.
const WORKSPACE_FILE: &str = "file://${workspace}/";

/// Why "Open browser" does not open `url` as written, if it does not (behaviour 9): only `http:`
/// and `https:` URLs with a host, and `file:` URLs of `.html`/`.htm` files of the workspace
/// (`file://${workspace}/…`). The scheme is written out, never taken from a variable, so nothing
/// that starts a program (an application's own scheme, `javascript:`, a file that is not a page)
/// reaches the system's opener.
pub fn browser_url_problem(url: &str) -> Option<String> {
    let refuse = |why: &str| Some(format!("the URL {url:?} is not opened: {why}"));
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return refuse("it contains a blank or a control character");
    }
    let Some((scheme, rest)) = url.split_once(':') else {
        return refuse("it must start with http://, https:// or file://${workspace}/");
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            let Some(after) = rest.strip_prefix("//") else {
                return refuse("write // after the scheme");
            };
            let authority = after.split(['/', '?', '#']).next().unwrap_or_default();
            // `user@host` hides where the page really is; a host never starts with `-`.
            if authority.is_empty() || authority.starts_with('-') || authority.contains('@') {
                return refuse("it needs a host, without a user name");
            }
            None
        }
        "file" => {
            let Some(path) = url.strip_prefix(WORKSPACE_FILE) else {
                return refuse("a file URL must start with file://${workspace}/");
            };
            let path = path.split(['?', '#']).next().unwrap_or_default();
            if path.split('/').any(|part| part == ".." || part == ".")
                || path.contains('\\')
                || path.contains('%')
                || path.contains("${")
            {
                return refuse("the path must stay inside the workspace, written plainly");
            }
            let lower = path.to_ascii_lowercase();
            if !(lower.ends_with(".html") || lower.ends_with(".htm")) {
                return refuse("only .html and .htm files of the workspace are opened");
            }
            None
        }
        _ => refuse("only http, https and file URLs are opened"),
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The URL "Open browser" hands to the system's opener for `url` of a configuration of the
/// workspace at `root`: checked with [`browser_url_problem`], then `${workspace}` and
/// `${env:NAME}` replaced, then checked again for blanks and control characters. A `file:` URL
/// gets the workspace's path percent-encoded.
pub fn browser_url(root: &Path, url: &str) -> Result<String, String> {
    if let Some(p) = browser_url_problem(url) {
        return Err(p);
    }
    let expanded = match url.strip_prefix(WORKSPACE_FILE) {
        Some(rest) => format!(
            "file://{}/{}",
            percent_encode_path(root.to_string_lossy().trim_end_matches('/')),
            percent_encode_path(rest)
                .replace("%3F", "?")
                .replace("%23", "#")
        ),
        None => crate::run::expand_vars(url, root),
    };
    if expanded
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(format!(
            "the URL {url:?} is not opened: a variable put a blank or a control character into it"
        ));
    }
    // A variable must not move the host: what is opened starts as it was checked.
    if !url.starts_with(WORKSPACE_FILE) {
        let authority = |u: &str| {
            u.split_once("://").map(|(_, a)| {
                a.split(['/', '?', '#'])
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
        };
        let host = authority(&expanded).unwrap_or_default();
        if host.is_empty() || host.starts_with('-') || host.contains('@') {
            return Err(format!(
                "the URL {url:?} is not opened: with its variables replaced it has no plain host"
            ));
        }
    }
    Ok(expanded)
}

/// How a URL is handed to the browser.
pub type BrowserOpener = dyn Fn(&str) -> Result<(), String> + Send + Sync;

static OPENER: Mutex<Option<Arc<BrowserOpener>>> = Mutex::new(None);

/// Replaces the system's opener for this process (`None` restores it): a host of remote projects
/// opens nothing on its own screen, and tests record what would be opened.
pub fn set_browser_opener(opener: Option<Arc<BrowserOpener>>) {
    *OPENER.lock().unwrap_or_else(|e| e.into_inner()) = opener;
}

/// The system's opener: `open` on macOS, `xdg-open` elsewhere, with the URL as its only
/// argument and no shell.
fn system_open(url: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(unix) {
        "xdg-open"
    } else {
        return Err("no opener for URLs on this system".into());
    };
    let mut child = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// "Open browser" (behaviour 9): opens `url` of a configuration of the workspace at `root` with
/// the system's opener and returns the URL opened. Nothing is opened in an untrusted project
/// (safe mode, `SPEC-0278`: the URL comes from a project file) and nothing but the URLs
/// [`browser_url`] lets through.
pub fn open_browser(root: &Path, url: &str) -> Result<String, String> {
    crate::trust::require_trusted(root)?;
    let url = browser_url(root, url)?;
    let opener = OPENER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match opener {
        Some(open) => open(&url)?,
        None => system_open(&url)?,
    }
    Ok(url)
}

/// How long "Open browser" waits for its port (v4 behaviour 13).
pub const BROWSER_PORT_WAIT: Duration = Duration::from_secs(60);

/// Whether something on this machine accepts connections on `port` (loopback only: the wait
/// never talks to another host).
pub fn local_port_open(port: u16) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    let within = Duration::from_millis(200);
    [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
    ]
    .iter()
    .any(|a| TcpStream::connect_timeout(a, within).is_ok())
}

/// Waits until `port` of this machine accepts connections: `Ok` then; `Err` with the reason when
/// `ended` is set first (the program is gone) or `limit` has passed.
pub fn wait_for_local_port(port: u16, ended: &AtomicBool, limit: Duration) -> Result<(), String> {
    let until = Instant::now() + limit;
    loop {
        if local_port_open(port) {
            return Ok(());
        }
        if ended.load(Ordering::SeqCst) {
            return Err(format!("the program ended before port {port} opened"));
        }
        if Instant::now() >= until {
            return Err(format!(
                "port {port} did not open within {} s",
                limit.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------------------------
// Instances (behaviour 10).

/// What starting a configuration does when instances of it run already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondStart {
    /// Nothing of it runs: start it.
    Start,
    /// It runs and allows one instance: ask — stop and rerun, or cancel.
    Ask,
    /// It runs and allows several: start another instance beside it.
    NewInstance,
}

/// The rule for starting `c` while `running` instances of it run (behaviour 10).
pub fn second_start(c: &RunConfig, running: usize) -> SecondStart {
    match (running, c.allow_multiple) {
        (0, _) => SecondStart::Start,
        (_, true) => SecondStart::NewInstance,
        (_, false) => SecondStart::Ask,
    }
}

/// The question of [`SecondStart::Ask`]: headline, detail and the two answers, the first of
/// which stops the running instance and starts the configuration again.
pub fn second_start_question(name: &str) -> (String, String, [&'static str; 2]) {
    (
        format!("\"{name}\" is already running"),
        "It allows one instance at a time. Stop it and run it again? \"Allow multiple instances\" \
         in its configuration starts several side by side."
            .into(),
        ["Stop and Rerun", "Cancel"],
    )
}

/// The title of the Run tab of a further instance of `name`: the first of `name`, `name (2)`,
/// `name (3)`, … that is not in `busy` (the titles of tabs whose run still goes).
pub fn instance_title(name: &str, busy: &[String]) -> String {
    crate::run_edit::free_name(name, busy)
}

struct Instance {
    id: u64,
    root: PathBuf,
    name: String,
    stop: Arc<AtomicBool>,
}

static INSTANCES: Mutex<Vec<Instance>> = Mutex::new(Vec::new());
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

fn instances() -> MutexGuard<'static, Vec<Instance>> {
    INSTANCES.lock().unwrap_or_else(|e| e.into_inner())
}

fn key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

/// A run of a configuration started by this process, listed until it is dropped.
pub(crate) struct InstanceGuard(u64);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        instances().retain(|i| i.id != self.0);
    }
}

/// Lists a run of configuration `name` of the workspace at `root` that `stop` ends.
pub(crate) fn register_instance(root: &Path, name: &str, stop: Arc<AtomicBool>) -> InstanceGuard {
    let id = NEXT_INSTANCE.fetch_add(1, Ordering::SeqCst);
    instances().push(Instance {
        id,
        root: key(root),
        name: name.to_owned(),
        stop,
    });
    InstanceGuard(id)
}

/// How many runs of configuration `name` of the workspace at `root` this process has going.
pub fn running_instances(root: &Path, name: &str) -> usize {
    let root = key(root);
    instances()
        .iter()
        .filter(|i| i.root == root && i.name == name)
        .count()
}

/// Stops every run of configuration `name` of the workspace at `root` and waits up to `wait`
/// until they ended ("Stop and Rerun"). Returns how many were asked to stop; `Err` when one still
/// runs after `wait`.
pub fn stop_instances(root: &Path, name: &str, wait: Duration) -> Result<usize, String> {
    let root_key = key(root);
    let asked = {
        let list = instances();
        let mine: Vec<&Instance> = list
            .iter()
            .filter(|i| i.root == root_key && i.name == name)
            .collect();
        for i in &mine {
            i.stop.store(true, Ordering::SeqCst);
        }
        mine.len()
    };
    let until = Instant::now() + wait;
    while running_instances(root, name) > 0 {
        if Instant::now() >= until {
            return Err(format!("{name}: the running instance did not stop in time"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(asked)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0047 v3 AK-9: only http, https and project pages reach the opener.
    #[test]
    fn only_web_pages_and_project_pages_are_opened() {
        for ok in [
            "http://localhost:3000",
            "https://example.org/a/b?q=1#top",
            "HTTP://127.0.0.1:8080/",
            "http://localhost:${env:PORT}/",
            "file://${workspace}/docs/index.html",
            "file://${workspace}/out/report.HTM#summary",
        ] {
            assert_eq!(browser_url_problem(ok), None, "{ok}");
        }
        for bad in [
            "",
            "localhost:3000",
            "javascript:alert(1)",
            "vscode://file/etc/passwd",
            "x-apple.systempreferences:com.apple.preference",
            "ssh://host/",
            "tel:123",
            "mailto:a@b.c",
            "data:text/html,hi",
            "file:///Applications/Calculator.app",
            "file:///usr/bin/true",
            "file://${workspace}/run.command",
            "file://${workspace}/tool.app",
            "file://${workspace}/../other/index.html",
            "file://${workspace}/a/%2e%2e/index.html",
            "file://${workspace}/${env:HOME}/x.html",
            "file://localhost/etc/hosts.html",
            "http:/localhost",
            "http://",
            "http://-oProxyCommand=x/",
            "https://user:pw@example.org/",
            "http://local host/",
            "http://localhost/\n--args",
            "${env:SCHEME}://localhost",
            "-a Calculator",
        ] {
            assert!(browser_url_problem(bad).is_some(), "{bad}");
        }
    }

    #[test]
    fn the_opened_url_has_its_variables_replaced_and_is_checked_again() {
        let root = Path::new("/work/my project");
        assert_eq!(
            browser_url(root, "file://${workspace}/docs/in dex.html").ok(),
            None,
            "a blank is refused as written"
        );
        assert_eq!(
            browser_url(root, "file://${workspace}/docs/index.html#a").unwrap(),
            "file:///work/my%20project/docs/index.html#a"
        );
        assert_eq!(
            browser_url(root, "http://localhost:3000/${workspace}").unwrap_err(),
            "the URL \"http://localhost:3000/${workspace}\" is not opened: a variable put a blank \
             or a control character into it"
        );
        // An unset variable leaves no host behind.
        assert!(browser_url(root, "http://${env:IDECRAFT_NO_SUCH_VARIABLE_XYZ}/").is_err());
        assert_eq!(
            browser_url(Path::new("/w"), "https://example.org/x?y=${workspace}").unwrap(),
            "https://example.org/x?y=/w"
        );
    }

    #[test]
    fn a_task_is_one_kind_and_round_trips_as_a_line() {
        let tasks = [
            BeforeTask::build(),
            BeforeTask::tool(vec!["sh".into(), "gen.sh".into(), "two words".into()], None),
            BeforeTask::tool(vec!["make".into()], Some("native code".into())),
            BeforeTask::tool(vec!["cwd=x".into(), "a".into()], None),
            BeforeTask::browser("http://localhost:3000"),
            BeforeTask::browser_after_port("http://localhost:3000/app", 3000),
        ];
        let lines: Vec<String> = tasks.iter().map(BeforeTask::line).collect();
        assert_eq!(
            lines,
            [
                "build",
                "tool sh gen.sh 'two words'",
                "tool cwd='native code' make",
                "tool 'cwd=x' a",
                "browser http://localhost:3000",
                "browser wait=3000 http://localhost:3000/app",
            ]
        );
        for (t, l) in tasks.iter().zip(&lines) {
            assert_eq!(&BeforeTask::parse_line(l).unwrap(), t, "{l}");
            assert_eq!(t.problem(), None, "{l}");
        }
        for bad in [
            "",
            "build all",
            "tool",
            "tool cwd=x",
            "browser",
            "browser a b",
            "browser wait=3000",
            "browser wait=0 http://localhost",
            "browser wait=70000 http://localhost",
            "browser wait=soon http://localhost",
            "deploy",
        ] {
            assert!(BeforeTask::parse_line(bad).is_err(), "{bad}");
        }
        let both = BeforeTask {
            build: Some("project".into()),
            browser: Some("http://localhost".into()),
            ..BeforeTask::default()
        };
        assert!(both.problem().is_some());
        assert!(BeforeTask::default().problem().is_some());
        let mut other = BeforeTask::build();
        other.build = Some("module".into());
        assert!(other.problem().is_some());
        // A port to wait for belongs to a page, and is a port.
        let mut waits = BeforeTask::build();
        waits.wait_port = Some(80);
        assert!(waits.problem().is_some());
        assert!(
            BeforeTask::browser_after_port("http://localhost", 0)
                .problem()
                .is_some()
        );
        let mut with_cwd = BeforeTask::build();
        with_cwd.cwd = Some("x".into());
        assert!(with_cwd.problem().is_some());
        assert!(
            BeforeTask::tool(vec!["make".into()], Some("../out".into()))
                .problem()
                .is_some()
        );
        assert!(BeforeTask::tool(vec![], None).problem().is_some());
        assert!(
            BeforeTask::browser("javascript:alert(1)")
                .problem()
                .is_some()
        );
    }

    /// SPEC-0047 v3 AK-10 (rule).
    #[test]
    fn a_second_start_asks_unless_several_instances_are_allowed() {
        let mut c = crate::run_edit::RunTemplate::Command.config(&[]);
        assert_eq!(second_start(&c, 0), SecondStart::Start);
        assert_eq!(second_start(&c, 1), SecondStart::Ask);
        c.allow_multiple = true;
        assert_eq!(second_start(&c, 0), SecondStart::Start);
        assert_eq!(second_start(&c, 2), SecondStart::NewInstance);
        assert_eq!(instance_title("web", &[]), "web");
        assert_eq!(
            instance_title("web", &["web".into(), "web (2)".into()]),
            "web (3)"
        );
        let (headline, _, answers) = second_start_question("web");
        assert_eq!(headline, "\"web\" is already running");
        assert_eq!(answers, ["Stop and Rerun", "Cancel"]);
    }
}
