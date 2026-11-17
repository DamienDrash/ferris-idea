//! Known debug adapters and how to find them (`SPEC-0039` behaviour 6).

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Serialize;

/// A debug adapter the IDE knows how to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Adapter {
    /// Name, e.g. `lldb-dap`.
    pub name: &'static str,
    /// `adapterID` sent in `initialize`.
    pub adapter_id: &'static str,
    /// Languages it debugs.
    pub languages: &'static [&'static str],
    /// Command to start it, if installed.
    pub command: Option<Vec<String>>,
}

fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

fn succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Command for `lldb-dap`: on the PATH (also as versioned `lldb-dap-NN` or in an LLVM
/// directory, as Linux distributions install it), else through `xcrun` on macOS.
pub fn lldb_dap() -> Option<Vec<String>> {
    let versioned = (14..=30).rev().find_map(|v| {
        on_path(&format!("lldb-dap-{v}")).or_else(|| {
            let p = PathBuf::from(format!("/usr/lib/llvm-{v}/bin/lldb-dap"));
            p.is_file().then_some(p)
        })
    });
    if let Some(p) = on_path("lldb-dap").or(versioned) {
        return Some(vec![p.to_string_lossy().into_owned()]);
    }
    let out = Command::new("xcrun")
        .args(["-f", "lldb-dap"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !path.is_empty()).then(|| vec![path])
}

/// Delve (`SPEC-0039` v7 behaviour 11): `IDECRAFT_DLV`, else `dlv` on the PATH, else where
/// `go install` puts it (`$GOBIN`, `$GOPATH/bin`, `~/go/bin`).
pub fn delve_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("IDECRAFT_DLV").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let name = if cfg!(windows) { "dlv.exe" } else { "dlv" };
    on_path(name).or_else(|| {
        let gobin = std::env::var_os("GOBIN").map(PathBuf::from);
        let gopath = std::env::var_os("GOPATH")
            .and_then(|paths| std::env::split_paths(&paths).next())
            .map(|p| p.join("bin"));
        let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("go/bin"));
        [gobin, gopath, home]
            .into_iter()
            .flatten()
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

/// The DAP server script of js-debug (`dapDebugServer.js`): `IDECRAFT_JS_DEBUG`, else the newest
/// `~/.local/opt/js-debug*/js-debug/src/dapDebugServer.js` (the release archive unpacked there).
pub fn js_debug_server() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("IDECRAFT_JS_DEBUG").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let opt = PathBuf::from(std::env::var_os("HOME")?).join(".local/opt");
    let mut found: Vec<PathBuf> = std::fs::read_dir(opt)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("js-debug"))
        .map(|e| e.path().join("js-debug/src/dapDebugServer.js"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

/// The start script of kotlin-debug-adapter: `IDECRAFT_KOTLIN_DEBUG`, else the newest
/// `kotlin-debug-adapter*` in `~/.local/opt` (`kotlin-debug.sh install` of the reference environment), else
/// `kotlin-debug-adapter` on the PATH (`SPEC-0132` behaviour 3).
pub fn kotlin_debug_adapter() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("IDECRAFT_KOTLIN_DEBUG").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let opt = PathBuf::from(std::env::var_os("HOME")?).join(".local/opt");
    let mut found: Vec<PathBuf> = std::fs::read_dir(opt)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("kotlin-debug-adapter")
                })
                .map(|e| e.path().join("adapter/bin/kotlin-debug-adapter"))
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found.pop().or_else(|| on_path("kotlin-debug-adapter"))
}

/// A Chrome or Chromium for debugging pages (`SPEC-0133` behaviour 2): `IDECRAFT_CHROME`, else the
/// usual install places of this system, else `google-chrome`/`chromium` on the PATH.
pub fn chrome() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("IDECRAFT_CHROME").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let known = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];
    known
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .or_else(|| {
            [
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
            ]
            .iter()
            .find_map(|n| on_path(n))
        })
}

/// The js-debug launch request for a Node.js program (`SPEC-0091` v4 behaviour 10, also a
/// TypeScript file on Node's type stripping, `SPEC-0168` behaviour 3), or with `runner` for a
/// package manager that runs `args` (`npm run dev`, `SPEC-0168` behaviour 1); its child processes
/// are debugged. Source maps are read only from the project outside `node_modules`.
pub fn node_launch(
    program: &std::path::Path,
    runner: Option<&std::path::Path>,
    args: &[String],
    cwd: &std::path::Path,
    env: &std::collections::BTreeMap<String, String>,
) -> serde_json::Value {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut launch = serde_json::json!({
        "type": "pwa-node",
        "request": "launch",
        "cwd": cwd,
        "env": env,
        "console": "internalConsole",
        "resolveSourceMapLocations": [format!("{}/**", cwd.display()), "!**/node_modules/**"],
    });
    match runner {
        Some(runner) => {
            launch["name"] = serde_json::json!(format!("{name} {}", args.join(" ")));
            launch["runtimeExecutable"] = serde_json::json!(runner);
            launch["runtimeArgs"] = serde_json::json!(args);
            // Jest runs its transformed test files at once: js-debug waits for each script's
            // source map, so a breakpoint whose line the transformation moved is set before the
            // test runs (`SPEC-0045` v6 behaviour 12).
            if args.first().is_some_and(|a| a == "jest") {
                launch["pauseForSourceMap"] = serde_json::json!(true);
            }
        }
        None => {
            launch["name"] = serde_json::json!(name);
            launch["program"] = serde_json::json!(program);
            launch["args"] = serde_json::json!(args);
        }
    }
    launch
}

/// Whether Node.js `version` (`v22.18.0`) runs TypeScript files without a flag (`SPEC-0168`
/// behaviour 3): 22.18 or newer.
pub fn strips_types(version: &str) -> bool {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let major: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    major > 22 || (major == 22 && minor >= 18)
}

/// The js-debug launch request for a page at `url` in Chrome (`SPEC-0133` v3 behaviour 2): sources
/// map to `web_root`, the browser runs with its own `profile` folder (never the user's, never in
/// the project: [`crate::BrowserProfile`]), headless when asked. Ending the session closes the
/// whole browser (`cleanUp`).
pub fn browser_launch(
    url: &str,
    web_root: &std::path::Path,
    chrome: &std::path::Path,
    profile: &std::path::Path,
    headless: bool,
) -> serde_json::Value {
    let mut args = vec![
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
    ];
    if headless {
        args.push("--headless=new".into());
    }
    serde_json::json!({
        "type": "pwa-chrome",
        "request": "launch",
        "name": url,
        "url": url,
        "webRoot": web_root,
        "runtimeExecutable": chrome,
        "runtimeArgs": args,
        "userDataDir": profile,
        "cleanUp": "wholeBrowser",
        "console": "internalConsole",
    })
}

/// js-debug's DAP server, started on a free local port; ends with the value, together with what it
/// started (the browser of a page, the debugged program), and never outlives IDECraft
/// (`SPEC-0263` v2 behaviour 3).
pub struct JsDebugServer {
    child: std::process::Child,
    guard: Option<crate::lifetime::ExitGuard>,
    /// The port it listens on.
    pub port: u16,
}

impl JsDebugServer {
    /// Starts `command` (`node` and the server script) with a free port and waits until it listens.
    pub fn start(command: &[String]) -> Result<Self, crate::DapError> {
        use std::io::BufRead;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_err(|e| crate::DapError::Start(e.to_string()))?
            .port();
        let (program, args) = command
            .split_first()
            .ok_or_else(|| crate::DapError::Start("empty js-debug command".into()))?;
        let mut command = Command::new(program);
        command
            .args(args)
            .args([port.to_string(), "127.0.0.1".to_owned()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Its own process group (SPEC-0263 behaviour 3).
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|e| crate::DapError::Start(format!("{program}: {e}")))?;
        let stdout = child.stdout.take();
        let guard = crate::lifetime::ExitGuard::watch(child.id());
        let server = Self { child, guard, port };
        let mut lines = std::io::BufReader::new(stdout.ok_or(crate::DapError::Closed)?).lines();
        match lines.next() {
            Some(Ok(line)) if line.contains("listening") => {}
            other => {
                // Dropping `server` ends it.
                return Err(crate::DapError::Start(format!(
                    "js-debug did not start: {}",
                    other.and_then(Result::ok).unwrap_or_default()
                )));
            }
        }
        // Keep reading so the server never blocks on a full pipe.
        std::thread::spawn(move || lines.for_each(drop));
        Ok(server)
    }

    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// `127.0.0.1:port`.
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

impl Drop for JsDebugServer {
    fn drop(&mut self) {
        // IDECraft ends it now, so the guard stands down first.
        drop(self.guard.take());
        crate::lifetime::end_tree(&mut self.child, std::time::Duration::from_secs(3));
    }
}

/// Whether this machine lets a debugger control a process without asking for a password:
/// on macOS that needs Developer Mode (`DevToolsSecurity`); elsewhere it is assumed.
pub fn can_debug_unattended() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    Command::new("DevToolsSecurity")
        .arg("-status")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("enabled"))
}

/// All known adapters with their start command when installed.
pub fn known_adapters() -> Vec<Adapter> {
    vec![
        Adapter {
            name: "lldb-dap",
            adapter_id: "lldb-dap",
            languages: &["c", "cpp", "rust", "swift"],
            command: lldb_dap(),
        },
        Adapter {
            name: "dlv",
            adapter_id: "dlv",
            languages: &["go"],
            command: delve_binary()
                .map(|p| vec![p.to_string_lossy().into_owned(), "dap".to_owned()]),
        },
        Adapter {
            name: "js-debug",
            adapter_id: "pwa-node",
            languages: &["javascript", "typescript"],
            command: on_path("node")
                .zip(js_debug_server())
                .map(|(node, server)| {
                    vec![
                        node.to_string_lossy().into_owned(),
                        server.to_string_lossy().into_owned(),
                    ]
                }),
        },
        Adapter {
            name: "kotlin-debug-adapter",
            adapter_id: "kotlin",
            languages: &["kotlin"],
            command: kotlin_debug_adapter().map(|p| vec![p.to_string_lossy().into_owned()]),
        },
        Adapter {
            name: "debugpy",
            adapter_id: "debugpy",
            languages: &["python"],
            command: succeeds("python3", &["-c", "import debugpy"]).then(|| {
                ["python3", "-m", "debugpy.adapter"]
                    .map(str::to_owned)
                    .to_vec()
            }),
        },
    ]
}
