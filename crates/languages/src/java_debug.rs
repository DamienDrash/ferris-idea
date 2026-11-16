//! Java debugging through jdtls with Microsoft's java-debug plugin (`SPEC-0097` behaviour 6): the
//! language server hosts a debug adapter and resolves the class path of a main class.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::LanguageId;
use crate::lsp::LspClient;

/// The java-debug plugin jar: `$IDECRAFT_JAVA_DEBUG`, else the newest
/// `com.microsoft.java.debug.plugin-*.jar` in `~/.local/opt/java-debug`.
pub fn java_debug_plugin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("IDECRAFT_JAVA_DEBUG").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let dir = PathBuf::from(std::env::var_os("HOME")?).join(".local/opt/java-debug");
    let mut jars: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("com.microsoft.java.debug.plugin-") && n.ends_with(".jar")
            })
        })
        .collect();
    jars.sort();
    jars.pop()
}

/// Our bundle on top of the java-debug plugin: the debug protocol and the wait for the program on
/// private listeners only (`SPEC-0322` behaviour 4). Built by `scripts/build-java-debug-private.sh`.
const PRIVATE_BUNDLE: &[u8] =
    include_bytes!("../java/debug-private/idecraft-java-debug-private.jar");

/// Whether `plugin` is a java-debug version the bundle was built for; it binds to that version
/// only, and without it nothing may start.
fn plugin_supported(plugin: &Path) -> bool {
    plugin
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("com.microsoft.java.debug.plugin-0.53."))
}

/// Whether Java debug sessions can start here: a java-debug our bundle fits.
pub fn java_debug_usable() -> bool {
    java_debug_plugin().is_some_and(|p| plugin_supported(&p))
}

/// Where the bundle is kept: a folder only this account can enter, rewritten when it differs.
fn private_bundle() -> Result<PathBuf, String> {
    let folder = idecraft_core::user_cache_dir()
        .ok_or("Cannot debug Java: no cache folder")?
        .join("java-debug-private");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    // Named by content: jdtls keeps a bundle installed as long as its location stays the same.
    let digest: String = {
        use sha2::{Digest, Sha256};
        Sha256::digest(PRIVATE_BUNDLE)[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let jar = folder.join(format!("idecraft-java-debug-private-{digest}.jar"));
    if std::fs::read(&jar).ok().as_deref() != Some(PRIVATE_BUNDLE) {
        let next = folder.join(format!("next-{}", std::process::id()));
        std::fs::write(&next, PRIVATE_BUNDLE).map_err(|e| e.to_string())?;
        std::fs::rename(&next, &jar).map_err(|e| e.to_string())?;
    }
    Ok(jar)
}

/// A new folder only this account can enter, for the adapter's socket: below the temporary folder,
/// or `/tmp` when the path would not fit a socket address (104 bytes on macOS).
#[cfg(unix)]
fn private_socket_folder() -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let name = format!("idecraft-jdap-{}-{nanos}", std::process::id());
    let temporary = std::env::temp_dir();
    let parent = if temporary.as_os_str().len() + name.len() + "/dap.sock".len() + 1 < 104 {
        temporary
    } else {
        PathBuf::from("/tmp")
    };
    let dir = parent.join(name);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Where the debug adapter listens (`SPEC-0322` behaviour 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugEndpoint {
    /// A Unix socket in a folder only this account can enter.
    Unix(PathBuf),
    /// A port on the loopback address, where there are no such sockets.
    Loopback(u16),
}

/// The argument of `vscode.java.updateDebugSettings` that limits exception breakpoints to `types`:
/// one JSON text, as the plugin reads it.
pub fn exception_settings(types: &[String]) -> Value {
    debug_settings(&json!({
        "exceptionFilters": {"exceptionTypes": types},
        "exceptionFiltersUpdated": true,
    }))
}

/// The argument of `vscode.java.updateDebugSettings` for `settings`: one JSON text with a log
/// level, without which java-debug 0.53 refuses every update (`String.length()` on a missing
/// name; `SPEC-0091` v12).
pub fn debug_settings(settings: &Value) -> Value {
    let mut settings = settings.clone();
    if settings.get("logLevel").is_none() {
        settings["logLevel"] = json!("WARN");
    }
    json!([settings.to_string()])
}

/// The command of our bundle that opens the private listener.
const START_PRIVATE: &str = "idecraft.java.startPrivateDebugSession";

/// A jdtls process with the debug plugin; kept alive while a Java program is debugged.
pub struct JavaDebugServer {
    runtime: tokio::runtime::Runtime,
    client: Option<LspClient>,
    /// Where the debug adapter listens.
    pub endpoint: DebugEndpoint,
    /// The socket's folder, removed with the server.
    folder: Option<PathBuf>,
    /// Versions of the files opened in jdtls, for the changes of hot swap (`SPEC-0091` v12).
    opened: std::sync::Mutex<std::collections::HashMap<PathBuf, i64>>,
}

impl JavaDebugServer {
    /// Starts jdtls for `root` with the plugin and our bundle and asks for a private adapter.
    pub fn start(root: &Path) -> Result<Self, String> {
        // Debugging runs the project (`SPEC-0278` behaviour 5).
        idecraft_core::trust::require_trusted(root)?;
        let plugin = java_debug_plugin().ok_or(
            "Cannot debug Java: the java-debug plugin is missing (com.microsoft.java.debug.plugin jar in ~/.local/opt/java-debug)",
        )?;
        if !plugin_supported(&plugin) {
            return Err(format!(
                "Cannot debug Java: this java-debug has no private listener ({}); java-debug 0.53 is needed",
                plugin.display()
            ));
        }
        let bundle = private_bundle()?;
        let server = crate::servers::resolve(LanguageId::Java).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let root = root.to_path_buf();
        let client = runtime.block_on(async {
            // Its own data folder: the editor's jdtls may run on the same project.
            let mut client = server
                .spawn_beside(&root, "debug")
                .map_err(|e| e.to_string())?;
            client.timeout = Duration::from_secs(120);
            let mut options = server.initialization_options(&root);
            if !options.is_object() {
                options = json!({});
            }
            options["bundles"] =
                json!([plugin.display().to_string(), bundle.display().to_string()]);
            client.init_options = options;
            client.initialize(&root).await.map_err(|e| e.to_string())?;
            Ok::<_, String>(client)
        })?;
        let mut server = Self {
            runtime,
            client: Some(client),
            endpoint: DebugEndpoint::Loopback(0),
            folder: None,
            opened: Default::default(),
        };
        server.start_adapter()?;
        // Changed classes are built on save and reloaded when asked (SPEC-0091 v12 behaviour 37).
        let _ = server.update_settings(&json!({"hotCodeReplace": "manual"}));
        Ok(server)
    }

    /// Opens the adapter's listener (`SPEC-0322` behaviour 4). java-debug's own
    /// `vscode.java.startDebugSession` is never called: it listens on every interface.
    #[cfg(unix)]
    fn start_adapter(&mut self) -> Result<(), String> {
        let folder = private_socket_folder()?;
        self.folder = Some(folder.clone());
        let socket = folder.join("dap.sock");
        self.command(START_PRIVATE, json!([socket.display().to_string()]))?;
        self.endpoint = DebugEndpoint::Unix(socket);
        Ok(())
    }

    #[cfg(not(unix))]
    fn start_adapter(&mut self) -> Result<(), String> {
        let port = self.command(START_PRIVATE, json!([]))?;
        self.endpoint = DebugEndpoint::Loopback(
            port.as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .ok_or_else(|| format!("jdtls gave no debug port: {port}"))?,
        );
        Ok(())
    }

    /// The address for `DapClient::connect`.
    pub fn dap_address(&self) -> String {
        match &self.endpoint {
            DebugEndpoint::Unix(path) => format!("unix:{}", path.display()),
            DebugEndpoint::Loopback(port) => format!("127.0.0.1:{port}"),
        }
    }

    fn command(&self, command: &str, arguments: Value) -> Result<Value, String> {
        let client = self.client.as_ref().ok_or("jdtls is closed")?;
        self.runtime
            .block_on(client.request(
                "workspace/executeCommand",
                json!({"command": command, "arguments": arguments}),
            ))
            .map_err(|e| format!("{command}: {e}"))
    }

    /// Limits java-debug's exception breakpoints to `types` (all with none): it reads them from its
    /// debug settings, not from `setExceptionBreakpoints` (`SPEC-0146` v2, D30).
    pub fn set_exception_types(&self, types: &[String]) -> Result<(), String> {
        self.command("vscode.java.updateDebugSettings", exception_settings(types))
            .map(|_| ())
    }

    /// Opens `file` in jdtls, so a source outside any build project becomes part of its workspace
    /// (jdtls creates a project for such files only when they are opened).
    pub fn open(&self, file: &Path) -> Result<(), String> {
        let client = self.client.as_ref().ok_or("jdtls is closed")?;
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        self.runtime
            .block_on(client.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": crate::lsp::file_uri(file), "languageId": "java", "version": 1, "text": text}}),
            ))
            .map_err(|e| e.to_string())?;
        if let Ok(mut opened) = self.opened.lock() {
            opened.insert(file.to_path_buf(), 1);
        }
        Ok(())
    }

    /// Applies java-debug's debug `settings` (`vscode.java.updateDebugSettings`: `hotCodeReplace`,
    /// `showStaticVariables`, `exceptionFilters`, …; `SPEC-0091` v12).
    pub fn update_settings(&self, settings: &Value) -> Result<(), String> {
        self.command("vscode.java.updateDebugSettings", debug_settings(settings))
            .map(|_| ())
    }

    /// Tells jdtls that `files` changed on disk (open, change and save), so it builds their
    /// classes again and java-debug reports the build for hot swap (`SPEC-0091` v12 behaviour 37).
    pub fn sources_changed(&self, files: &[PathBuf]) -> Result<(), String> {
        let client = self.client.as_ref().ok_or("jdtls is closed")?;
        for file in files {
            let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let uri = crate::lsp::file_uri(file);
            let version = self
                .opened
                .lock()
                .map_err(|e| e.to_string())?
                .get(file)
                .copied();
            let result = self.runtime.block_on(async {
                match version {
                    None => {
                        client
                            .notify(
                                "textDocument/didOpen",
                                json!({"textDocument": {"uri": uri, "languageId": "java", "version": 1, "text": text}}),
                            )
                            .await?;
                    }
                    Some(v) => {
                        client
                            .notify(
                                "textDocument/didChange",
                                json!({"textDocument": {"uri": uri, "version": v + 1}, "contentChanges": [{"text": text}]}),
                            )
                            .await?;
                    }
                }
                client
                    .notify("textDocument/didSave", json!({"textDocument": {"uri": uri}}))
                    .await
            });
            result.map_err(|e| e.to_string())?;
            if let Ok(mut opened) = self.opened.lock() {
                opened.insert(file.clone(), version.unwrap_or(0) + 1);
            }
        }
        Ok(())
    }

    /// Builds the workspace incrementally and waits for it (`java/buildWorkspace`, as editors do
    /// before a launch): a source opened just before would otherwise not be compiled yet when the
    /// JVM starts ("could not find or load main class"). A build with errors still launches; the
    /// JVM then reports what is missing.
    pub fn build(&self) -> Result<(), String> {
        let client = self.client.as_ref().ok_or("jdtls is closed")?;
        self.runtime
            .block_on(client.request("java/buildWorkspace", json!(false)))
            .map(|_| ())
            .map_err(|e| format!("java/buildWorkspace: {e}"))
    }

    /// The jdtls project that holds `main_class` (`vscode.java.resolveMainClass`), for the launch's
    /// `projectName`: without it java-debug finds the project for evaluations itself and, while
    /// jdtls is still busy with the import, refuses them ("specify projectName").
    pub fn project_name(&self, main_class: &str) -> Option<String> {
        let found = self
            .command("vscode.java.resolveMainClass", json!([]))
            .ok()?;
        project_of(&found, main_class)
    }

    /// The jdtls project for a program that already runs (attach), where no main class is known:
    /// the sources in `files` (those with breakpoints) are opened, so jdtls has their project,
    /// and the project of a main class declared in one of them is taken, else the project all
    /// main classes of the workspace share. Waits up to `wait` while jdtls imports; `None` when
    /// jdtls then knows no main class or the main classes are spread over several projects.
    /// Without the name java-debug evaluates nothing: no expression, no breakpoint condition,
    /// no log message (`SPEC-0147` v2 behaviour 3a).
    pub fn project_of_sources(&self, files: &[PathBuf], wait: Duration) -> Option<String> {
        for file in files
            .iter()
            .filter(|f| f.extension().is_some_and(|e| e == "java"))
        {
            let _ = self.open(file);
        }
        let until = Instant::now() + wait;
        loop {
            if let Ok(found) = self.command("vscode.java.resolveMainClass", json!([]))
                && let Some(project) = project_of_files(&found, files)
            {
                return Some(project);
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// The `java` program the project of `main_class` runs with (`vscode.java.resolveJavaExecutable`):
    /// the JDK jdtls compiles the project for, which need not be `JAVA_HOME`'s.
    pub fn java_executable(&self, main_class: &str, project: Option<&str>) -> Option<PathBuf> {
        let found = self
            .command(
                "vscode.java.resolveJavaExecutable",
                json!([main_class, project]),
            )
            .ok()?;
        found.as_str().filter(|p| !p.is_empty()).map(PathBuf::from)
    }

    /// Module and class path of `main_class`, waiting up to `wait` while jdtls imports the project.
    pub fn classpath(
        &self,
        main_class: &str,
        wait: Duration,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let until = Instant::now() + wait;
        loop {
            match self.command("vscode.java.resolveClasspath", json!([main_class, null])) {
                Ok(v) => {
                    let list = |i: usize| -> Vec<String> {
                        v[i].as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str().map(str::to_owned))
                            .collect()
                    };
                    let (modules, classes) = (list(0), list(1));
                    if !modules.is_empty() || !classes.is_empty() {
                        return Ok((modules, classes));
                    }
                }
                Err(e) if Instant::now() >= until => return Err(e),
                Err(_) => {}
            }
            if Instant::now() >= until {
                return Err(format!(
                    "jdtls could not resolve the class path of {main_class}"
                ));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for JavaDebugServer {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            self.runtime.block_on(client.shutdown());
        }
        if let Some(folder) = self.folder.take() {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

/// The project of `main_class` in an answer of `vscode.java.resolveMainClass` (a list of
/// `mainClass`, `projectName`, `filePath`).
pub fn project_of(found: &Value, main_class: &str) -> Option<String> {
    found.as_array()?.iter().find_map(|m| {
        (m["mainClass"].as_str() == Some(main_class))
            .then(|| m["projectName"].as_str().map(str::to_owned))
            .flatten()
            .filter(|p| !p.is_empty())
    })
}

/// The project to name when attaching, from an answer of `vscode.java.resolveMainClass`: the
/// project of a main class declared in one of `files`, else the one project every main class is
/// in; `None` without a named project or with several and no file to choose by.
pub fn project_of_files(found: &Value, files: &[PathBuf]) -> Option<String> {
    let entries: Vec<(&str, &str)> = found
        .as_array()?
        .iter()
        .filter_map(|m| {
            let project = m["projectName"].as_str().filter(|p| !p.is_empty())?;
            Some((project, m["filePath"].as_str().unwrap_or_default()))
        })
        .collect();
    let resolved = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let same_file = |a: &str, b: &Path| resolved(Path::new(a)) == resolved(b);
    if let Some((project, _)) = entries
        .iter()
        .find(|(_, file)| files.iter().any(|f| same_file(file, f)))
    {
        return Some((*project).to_owned());
    }
    let (first, _) = entries.first()?;
    entries
        .iter()
        .all(|(p, _)| p == first)
        .then(|| (*first).to_owned())
}

/// The main class of Java source `text` in `file`: its package and file name.
pub fn main_class(file: &Path, text: &str) -> Option<String> {
    let stem = file.file_stem()?.to_string_lossy().into_owned();
    let package = text.lines().map(str::trim).find_map(|l| {
        l.strip_prefix("package ")
            .map(|p| p.trim_end_matches(';').trim().to_owned())
    });
    Some(match package {
        Some(p) => format!("{p}.{stem}"),
        None => stem,
    })
}

#[cfg(test)]
mod tests {
    // SPEC-0147 v2 behaviour 3a: the project named when attaching.
    #[test]
    fn the_project_for_attaching_comes_from_the_sources_or_is_the_only_one() {
        use serde_json::json;
        use std::path::PathBuf;
        let two = json!([
            {"mainClass": "a.Main", "projectName": "app", "filePath": "/w/app/src/a/Main.java"},
            {"mainClass": "b.Tool", "projectName": "tools", "filePath": "/w/tools/src/b/Tool.java"},
            {"mainClass": "c.Odd", "projectName": "", "filePath": "/w/Odd.java"},
        ]);
        let files = [PathBuf::from("/w/tools/src/b/Tool.java")];
        assert_eq!(
            super::project_of_files(&two, &files).as_deref(),
            Some("tools")
        );
        // Several projects and no source to choose by: none is guessed.
        assert_eq!(super::project_of_files(&two, &[]), None);
        let one = json!([
            {"mainClass": "a.Main", "projectName": "app", "filePath": "/w/app/src/a/Main.java"},
            {"mainClass": "a.Other", "projectName": "app", "filePath": "/w/app/src/a/Other.java"},
        ]);
        assert_eq!(
            super::project_of_files(&one, &[]).as_deref(),
            Some("app"),
            "the one project of the workspace"
        );
        assert_eq!(super::project_of_files(&json!([]), &files), None);
        assert_eq!(super::project_of_files(&json!(null), &files), None);
    }

    // SPEC-0322 behaviour 4: the bundle binds to one java-debug version; others do not start.
    #[test]
    fn only_the_java_debug_version_of_the_bundle_is_used() {
        use std::path::Path;
        assert!(super::plugin_supported(Path::new(
            "/x/com.microsoft.java.debug.plugin-0.53.1.jar"
        )));
        assert!(!super::plugin_supported(Path::new(
            "/x/com.microsoft.java.debug.plugin-0.54.0.jar"
        )));
    }

    // SPEC-0091 v13: the launch names the project, so java-debug evaluates during the import.
    #[test]
    fn the_main_class_names_its_project() {
        let found = serde_json::json!([
            {"mainClass": "Dbg", "projectName": "app_1a2b", "filePath": "/w/Dbg.java"},
            {"mainClass": "dbg.Main", "projectName": "", "filePath": "/w/dbg/Main.java"},
        ]);
        assert_eq!(
            super::project_of(&found, "Dbg").as_deref(),
            Some("app_1a2b")
        );
        assert_eq!(
            super::project_of(&found, "dbg.Main"),
            None,
            "an empty name is none"
        );
        assert_eq!(super::project_of(&found, "Other"), None);
        assert_eq!(super::project_of(&serde_json::json!(null), "Dbg"), None);
    }

    // D30: the plugin reads one JSON text with the types and the "updated" flag.
    #[test]
    fn exception_types_become_one_settings_text() {
        let args = super::exception_settings(&["java.lang.NullPointerException".to_owned()]);
        let text = args[0].as_str().unwrap();
        let settings: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            settings["exceptionFilters"]["exceptionTypes"][0],
            "java.lang.NullPointerException"
        );
        assert_eq!(settings["exceptionFiltersUpdated"], true);
        // java-debug refuses settings without a log level (SPEC-0091 v12).
        assert_eq!(settings["logLevel"], "WARN");
    }
}
