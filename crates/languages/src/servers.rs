//! Language server registry (`SPEC-0042`).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::LanguageId;
use crate::lsp::LspError;

/// A server that can serve a language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// Program name looked up on the `PATH`.
    pub program: &'static str,
    /// Arguments for stdio mode.
    pub args: &'static [&'static str],
    /// How to install it.
    pub install: &'static str,
    /// Whether it reports when it has loaded the workspace (`experimental/serverStatus` with
    /// `quiescent`, `language/status` with `ServiceReady`) and answers requests only usefully after
    /// that (`SPEC-0042` v2).
    pub reports_status: bool,
    /// Whether it answers about an opened file only after publishing its diagnostics (jdtls,
    /// `SPEC-0042` v3).
    pub diagnoses_first: bool,
    /// Text its answers contain while it is still loading the workspace (lua-language-server's
    /// "Workspace loading"); such a server is probed until the text is gone (`SPEC-0042` v4).
    pub loading_text: Option<&'static str>,
    /// Whether it ignores the text of `didOpen` and reads the file from disk; such a server gets
    /// the text again as a `didChange` (groovy-language-server, `SPEC-0042` v4).
    pub resend_on_open: bool,
    /// Whether a question asks for a file's diagnostics with `textDocument/diagnostic`: a server
    /// that publishes them in steps answers the pull complete (vue-language-server, `SPEC-0141`
    /// v4 behaviour 9); TypeScript 7's server gives them only when asked (`SPEC-0042` v13).
    pub pulls_diagnostics: bool,
    /// Whether it lists the project's files only after it has started and answers questions that
    /// span files (references, rename) from the opened file alone until then; it publishes the
    /// first opened file's diagnostics after the listing, so it is asked after them (pyright and
    /// basedpyright, `SPEC-0042` v14 behaviour 22).
    pub lists_files_first: bool,
    /// Whether it publishes an opened file's diagnostics in two steps: first those of its build of
    /// the saved file (a type error only by its line), then those of its own reading of the opened
    /// text with exact ranges. A question waits briefly for the second list and sends that one as
    /// context, since the fixes are found by the problem's exact range (jdtls, `SPEC-0076` v6
    /// behaviour 10).
    pub refines_diagnostics: bool,
    /// Whether, once started, it reads the project's saved files in the background and replaces
    /// its reading of an opened file with the saved content (bash-language-server 5.4 to 5.8): the
    /// first file opened in it is sent again when its text differs from the saved file, after its
    /// first diagnostics (`SPEC-0042` v18 behaviour 34).
    pub rereads_saved_files: bool,
    /// Whether it indexes the project only when its stored index is out of date and says nothing
    /// when it is not (intelephense): it is ready when its indexing ends or, when it has begun
    /// none, after [`crate::lsp::INDEXING_SETTLE`] without a message (`SPEC-0042` v17 behaviour
    /// 25a).
    pub skips_indexing_when_cached: bool,
    /// Whether it publishes an opened file's diagnostics twice, the second time after checking the
    /// file against its index (intelephense): the first list can be empty under load, so a
    /// question for diagnostics takes the second, or the first when no second comes within
    /// [`crate::insight::REFINE_WAIT`] (`SPEC-0042` v20 behaviour 36).
    pub publishes_twice: bool,
}

const fn c(
    program: &'static str,
    args: &'static [&'static str],
    install: &'static str,
) -> Candidate {
    Candidate {
        program,
        args,
        install,
        reports_status: false,
        diagnoses_first: false,
        loading_text: None,
        resend_on_open: false,
        pulls_diagnostics: false,
        lists_files_first: false,
        refines_diagnostics: false,
        rereads_saved_files: false,
        skips_indexing_when_cached: false,
        publishes_twice: false,
    }
}

const STDIO: &[&str] = &["--stdio"];
const HTML: &[Candidate] = &[c(
    "vscode-html-language-server",
    STDIO,
    "npm install -g vscode-langservers-extracted",
)];
/// Vue in standalone mode (`SPEC-0141`): 2.x, since 3.x works only with a TypeScript plugin in
/// the editor's own TypeScript server. It serves script and template itself, so no TypeScript
/// server runs beside it (v4 behaviour 8); its diagnostics come in steps (v4 behaviour 9).
const VUE: &[Candidate] = &[Candidate {
    pulls_diagnostics: true,
    ..c(
        "vue-language-server",
        STDIO,
        "npm install -g @vue/language-server@2 typescript",
    )
}];
const SVELTE: &[Candidate] = &[c(
    "svelteserver",
    STDIO,
    "npm install -g svelte-language-server",
)];
/// Angular templates in projects with `angular.json` (`SPEC-0141`).
const ANGULAR: Candidate = c("ngserver", STDIO, "npm install -g @angular/language-server");
const CSS: &[Candidate] = &[c(
    "vscode-css-language-server",
    STDIO,
    "npm install -g vscode-langservers-extracted",
)];
const JSON: &[Candidate] = &[c(
    "vscode-json-language-server",
    STDIO,
    "npm install -g vscode-langservers-extracted",
)];
const TS: &[Candidate] = &[
    c(
        "typescript-language-server",
        STDIO,
        "npm install -g typescript-language-server typescript@5",
    ),
    c("vtsls", STDIO, "npm install -g @vtsls/language-server"),
    // TypeScript 7's own language server (`SPEC-0042` v12 behaviour 21). It hands out a file's
    // diagnostics when asked (`textDocument/diagnostic`) instead of publishing them (v13).
    Candidate {
        pulls_diagnostics: true,
        ..c(
            "tsc",
            &["--lsp", "--stdio"],
            "npm install -g typescript@7 (TypeScript 7 brings its own language server)",
        )
    },
];

// Completes from the words of the file until it has parsed it, and publishes the file's
// diagnostics when it has (`SPEC-0042` v16 behaviour 24).
const CLANGD: &[Candidate] = &[Candidate {
    diagnoses_first: true,
    ..c(
        "clangd",
        &[],
        "install clangd (Xcode command line tools, `brew install llvm` or the distribution's clangd package)",
    )
}];
const RUST: &[Candidate] = &[Candidate {
    reports_status: true,
    ..c("rust-analyzer", &[], "rustup component add rust-analyzer")
}];
const PYTHON: &[Candidate] = &[
    Candidate {
        lists_files_first: true,
        ..c("basedpyright-langserver", STDIO, "pip install basedpyright")
    },
    Candidate {
        lists_files_first: true,
        ..c("pyright-langserver", STDIO, "npm install -g pyright")
    },
    c("pylsp", &[], "pip install python-lsp-server"),
];
const GO: &[Candidate] = &[c(
    "gopls",
    &[],
    "go install golang.org/x/tools/gopls@latest",
)];
const JAVA: &[Candidate] = &[Candidate {
    reports_status: true,
    diagnoses_first: true,
    refines_diagnostics: true,
    ..c(
        "jdtls",
        &[],
        "install Eclipse JDT Language Server (`brew install jdtls` or the distribution's package; needs the JDK it was built for)",
    )
}];
const KOTLIN: &[Candidate] = &[Candidate {
    // Answers from the syntax alone until it has compiled the file, which its diagnostics of the
    // file tell (`SPEC-0042` v20 behaviour 35).
    diagnoses_first: true,
    ..c(
        "kotlin-language-server",
        &[],
        "brew install kotlin-language-server",
    )
}];
const PHP: &[Candidate] = &[
    // Answers empty until the work-done progress of its indexing ends (`SPEC-0042` v16
    // behaviour 25).
    Candidate {
        reports_status: true,
        skips_indexing_when_cached: true,
        publishes_twice: true,
        ..c("intelephense", STDIO, "npm install -g intelephense")
    },
    c(
        "phpactor",
        &["language-server"],
        "composer global require phpactor/phpactor",
    ),
];
const RUBY: &[Candidate] = &[
    // Answers hover and definition empty until its indexing progress ends (`SPEC-0042` v6).
    Candidate {
        reports_status: true,
        ..c("ruby-lsp", &[], "gem install ruby-lsp")
    },
    c("solargraph", &["stdio"], "gem install solargraph"),
];

const XML: &[Candidate] = &[c(
    "lemminx",
    &[],
    "download the lemminx binary from the vscode-xml releases (github.com/redhat-developer/vscode-xml)",
)];
const YAML: &[Candidate] = &[c(
    "yaml-language-server",
    STDIO,
    "npm install -g yaml-language-server",
)];
const MARKDOWN: &[Candidate] = &[c(
    "marksman",
    &["server"],
    "download marksman from github.com/artempyanykh/marksman/releases",
)];
const TOML: &[Candidate] = &[c(
    "taplo",
    &["lsp", "stdio"],
    "npm install -g @taplo/cli (or cargo install taplo-cli --features lsp)",
)];
// Reads its configuration after `initialize` and analyses the opened text only then.
const SHELL: &[Candidate] = &[Candidate {
    diagnoses_first: true,
    rereads_saved_files: true,
    ..c(
        "bash-language-server",
        &["start"],
        "npm install -g bash-language-server",
    )
}];
const DOCKERFILE: &[Candidate] = &[c(
    "docker-langserver",
    STDIO,
    "npm install -g dockerfile-language-server-nodejs",
)];
const GROOVY: &[Candidate] = &[Candidate {
    resend_on_open: true,
    ..c(
        "groovy-language-server",
        &[],
        "build github.com/GroovyLanguageServer/groovy-language-server and put a `groovy-language-server` start script on the PATH",
    )
}];
const SCALA: &[Candidate] = &[c(
    "metals",
    &[],
    "cs install metals (Coursier, get-coursier.io)",
)];
const SWIFT: &[Candidate] = &[c(
    "sourcekit-lsp",
    &[],
    "install Xcode or the Swift toolchain (swift.org), which bring sourcekit-lsp",
)];
// Answers "Workspace loading" for a few seconds after start, with no notification when done.
const LUA: &[Candidate] = &[Candidate {
    loading_text: Some("Workspace loading"),
    ..c(
        "lua-language-server",
        &[],
        "download lua-language-server from github.com/LuaLS/lua-language-server/releases",
    )
}];
// Parses the opened text in the background and answers definitions only after that.
const R: &[Candidate] = &[Candidate {
    diagnoses_first: true,
    ..c(
        "R",
        &["--no-echo", "-e", "languageserver::run()"],
        "install R and the languageserver package (install.packages(\"languageserver\"))",
    )
}];

/// Candidates for `language`, best first (behaviour 1).
pub fn candidates(language: LanguageId) -> &'static [Candidate] {
    match language {
        LanguageId::C | LanguageId::Cpp => CLANGD,
        LanguageId::Rust => RUST,
        LanguageId::Python => PYTHON,
        LanguageId::TypeScript | LanguageId::JavaScript => TS,
        LanguageId::Go => GO,
        LanguageId::Java => JAVA,
        LanguageId::Kotlin => KOTLIN,
        LanguageId::Php => PHP,
        LanguageId::Ruby => RUBY,
        LanguageId::Html => HTML,
        // vscode-css-language-server also serves SCSS and Less.
        LanguageId::Css | LanguageId::Scss | LanguageId::Less => CSS,
        LanguageId::Xml => XML,
        LanguageId::Yaml => YAML,
        LanguageId::Markdown => MARKDOWN,
        LanguageId::Toml => TOML,
        LanguageId::Shell => SHELL,
        LanguageId::Dockerfile => DOCKERFILE,
        LanguageId::Groovy => GROOVY,
        LanguageId::Scala => SCALA,
        LanguageId::Swift => SWIFT,
        LanguageId::Lua => LUA,
        LanguageId::R => R,
        LanguageId::Json => JSON,
        LanguageId::Vue => VUE,
        LanguageId::Svelte => SVELTE,
        LanguageId::Sql => &[],
    }
}

/// Why a candidate cannot be used, or where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "state", content = "detail")]
pub enum Availability {
    /// Usable at this path.
    Ready(PathBuf),
    /// Not on the `PATH`.
    Missing,
    /// On the `PATH` but not usable, with the reason.
    Unusable(String),
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The `rustup` next to `path` if `path` is one of its proxies.
#[cfg(unix)]
fn rustup_proxy(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let rustup = path.parent()?.join("rustup");
    let (a, b) = (
        std::fs::metadata(path).ok()?,
        std::fs::metadata(&rustup).ok()?,
    );
    (a.ino() == b.ino() && a.dev() == b.dev()).then_some(rustup)
}

#[cfg(not(unix))]
fn rustup_proxy(_: &Path) -> Option<PathBuf> {
    None
}

/// Why the rustup proxy `path` cannot run `program` in `root` (the folder whose toolchain rustup
/// chooses; `None`: the folder IDECraft runs in), or `None` when it can (`SPEC-0042` v15
/// behaviour 2a, v16 behaviour 23). rustup is asked without letting it install a toolchain.
fn rustup_problem(
    rustup: &Path,
    path: &Path,
    program: &str,
    root: Option<&Path>,
) -> Option<String> {
    let mut which = Command::new(rustup);
    which
        .args(["which", program])
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(root) = root.filter(|r| r.is_dir()) {
        which.current_dir(root);
    }
    // A rustup still open for writing ("Text file busy") runs a moment later.
    let said = match idecraft_core::spawn::output(&mut which) {
        Ok(o) if o.status.success() => return None,
        Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
        Err(_) => String::new(),
    };
    // rustup names the toolchain it chose for the folder in quotes.
    let toolchain = said
        .split_once("toolchain '")
        .and_then(|(_, rest)| rest.split_once('\''))
        .map(|(name, _)| name)
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        });
    let place = root.map_or_else(String::new, |r| format!(" for {}", r.display()));
    Some(match toolchain {
        Some(name) if said.contains("is not installed") => format!(
            "{} is a rustup proxy, and the toolchain {name} chosen{place} is not installed (rustup toolchain install {name} --component {program})",
            path.display()
        ),
        Some(name) => format!(
            "{} is a rustup proxy without the component in the toolchain {name} chosen{place} (rustup component add {program} --toolchain {name})",
            path.display()
        ),
        // rustup did not say which toolchain: the folder is what is known.
        None => match root {
            Some(root) => format!(
                "{} is a rustup proxy, and the toolchain rustup selects in {} has no {program} component",
                path.display(),
                root.display()
            ),
            None => format!("{} is a rustup proxy without the component", path.display()),
        },
    })
}

/// Availability of `program` with the given `PATH` value (behaviour 2).
pub fn find_in(path_var: &OsStr, program: &str) -> Availability {
    find_for(path_var, program, None)
}

/// [`find_in`] for a server that will run in the workspace `root` (`SPEC-0042` v15 behaviour 2a):
/// a rustup proxy starts the program of the toolchain rustup selects for the folder it runs in
/// (`rust-toolchain.toml`, a folder override, else the default), so whether that toolchain has
/// the component is asked there, not in the folder IDECraft happens to run in. rustup is asked
/// without letting it install a toolchain, and its answer names the toolchain (v16 behaviour 23).
pub fn find_for(path_var: &OsStr, program: &str, root: Option<&Path>) -> Availability {
    let mut unusable = None;
    for dir in std::env::split_paths(path_var) {
        let path = dir.join(program);
        if !path.exists() {
            continue;
        }
        if !is_executable(&path) {
            unusable.get_or_insert_with(|| format!("{} is not executable", path.display()));
            continue;
        }
        if let Some(rustup) = rustup_proxy(&path)
            && let Some(problem) = rustup_problem(&rustup, &path, program, root)
        {
            unusable.get_or_insert(problem);
            continue;
        }
        // TypeScript: which TypeScript the program comes with decides (`SPEC-0042` v11
        // behaviour 20).
        match program {
            "typescript-language-server" => {
                if let Some(TypeScriptBeside::Native { version, .. }) =
                    typescript_kind_beside(&path)
                {
                    unusable.get_or_insert_with(|| {
                        format!(
                            "{}: the TypeScript installed with it is {version}, which has no tsserver.js for typescript-language-server; TypeScript 7's own language server (tsc --lsp) is used instead, or install TypeScript 5 next to it: npm install -g typescript@5",
                            path.display()
                        )
                    });
                    continue;
                }
            }
            "tsc" => match typescript_kind_beside(&path) {
                Some(TypeScriptBeside::Native {
                    program: Some(native),
                    ..
                }) => return Availability::Ready(native),
                Some(TypeScriptBeside::Native {
                    version,
                    program: None,
                }) => {
                    unusable.get_or_insert_with(|| {
                        format!(
                            "{}: TypeScript {version} lacks its native program for this system ({}); install it again: npm install -g typescript@7",
                            path.display(),
                            native_package()
                        )
                    });
                    continue;
                }
                Some(TypeScriptBeside::Server { version }) => {
                    unusable.get_or_insert_with(|| {
                        format!(
                            "{} is TypeScript {version}, which has no language server of its own (TypeScript 7 has one)",
                            path.display()
                        )
                    });
                    continue;
                }
                None => {
                    unusable.get_or_insert_with(|| {
                        format!(
                            "{} does not belong to a TypeScript package IDECraft can read",
                            path.display()
                        )
                    });
                    continue;
                }
            },
            _ => {}
        }
        return Availability::Ready(path);
    }
    unusable.map_or(Availability::Missing, Availability::Unusable)
}

/// The TypeScript in the `node_modules` that holds `program` (symlinks followed): the version
/// typescript-language-server needs (with `lib/tsserver.js`), or TypeScript 7 and later, which
/// has no `tsserver.js` and starts its own language server as `tsc --lsp --stdio` from a native
/// program for this system (`SPEC-0042` v12 behaviour 21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeScriptBeside {
    /// A TypeScript with `lib/tsserver.js`.
    Server {
        /// Its version.
        version: String,
    },
    /// TypeScript 7 or later.
    Native {
        /// Its version.
        version: String,
        /// Its native program for this system, if installed.
        program: Option<PathBuf>,
    },
}

/// The npm package that holds TypeScript 7's native program for this system.
pub fn native_package() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("@typescript/typescript-{os}-{arch}")
}

/// What [`TypeScriptBeside`] the `typescript` package next to `program` is; `None` without one.
pub fn typescript_kind_beside(program: &Path) -> Option<TypeScriptBeside> {
    let real = program.canonicalize().ok()?;
    let modules = real.ancestors().find(|a| a.ends_with("node_modules"))?;
    let package = modules.join("typescript");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(package.join("package.json")).ok()?).ok()?;
    let version = manifest["version"].as_str()?.to_owned();
    if package.join("lib/tsserver.js").is_file() {
        return Some(TypeScriptBeside::Server { version });
    }
    let major: u32 = version.split('.').next()?.parse().ok()?;
    if major < 7 || !package.join("lib/getExePath.js").is_file() {
        return None;
    }
    let exe = if cfg!(windows) { "tsc.exe" } else { "tsc" };
    let name = native_package();
    // Inside the package, or hoisted beside it.
    let program = [package.join("node_modules"), modules.to_path_buf()]
        .into_iter()
        .map(|m| m.join(&name).join("lib").join(exe))
        .find(|p| is_executable(p));
    Some(TypeScriptBeside::Native { version, program })
}

/// A server ready to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolved {
    /// The candidate.
    pub candidate: Candidate,
    /// Full path of the program.
    pub command: PathBuf,
    /// Arguments before the candidate's (`exec <program>` when started through Bundler,
    /// `SPEC-0185` behaviour 8).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefix_args: Vec<String>,
    /// Arguments after the candidate's that depend on the workspace (ngserver's probe locations).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    /// `PATH` of the server process when it differs from the IDE's (the selected Ruby first,
    /// `SPEC-0185` behaviour 7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Started for a project in safe mode: without build execution (`SPEC-0278` behaviour 9).
    #[serde(rename = "safe_mode", skip_serializing_if = "std::ops::Not::not")]
    pub safe: bool,
}

impl Resolved {
    /// A server started as found, without workspace arguments.
    fn plain(candidate: Candidate, command: PathBuf) -> Self {
        Self {
            candidate,
            command,
            prefix_args: Vec::new(),
            extra_args: Vec::new(),
            path: None,
            safe: false,
        }
    }

    /// All arguments: the prefix, the candidate's, then the workspace's.
    pub fn args(&self) -> Vec<&str> {
        self.prefix_args
            .iter()
            .map(String::as_str)
            .chain(self.candidate.args.iter().copied())
            .chain(self.extra_args.iter().map(String::as_str))
            .collect()
    }

    /// The command line as one string, for reports.
    pub fn command_line(&self) -> String {
        std::iter::once(self.command.display().to_string())
            .chain(self.args().into_iter().map(str::to_owned))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Arguments that depend on the project `root`, after [`Resolved::args`]: jdtls gets its own
    /// workspace data folder, renewed when the inferred source roots changed (`SPEC-0042` v9
    /// behaviour 18); other servers none.
    pub fn project_args(&self, root: &Path) -> Result<Vec<String>, LspError> {
        self.project_args_with(root, crate::java_roots::workspace_data_dir)
    }

    /// [`Resolved::project_args`] with the data folder from `data_dir`.
    fn project_args_with(
        &self,
        root: &Path,
        data_dir: impl FnOnce(&Path) -> PathBuf,
    ) -> Result<Vec<String>, LspError> {
        if self.candidate.program != "jdtls" {
            return Ok(Vec::new());
        }
        let dir = data_dir(root);
        crate::java_roots::prepare_workspace_data(&dir, &crate::java_roots::source_roots(root))
            .map_err(|e| LspError::Io(format!("jdtls workspace data {}: {e}", dir.display())))?;
        let mut args = vec![
            // Eclipse's project files stay in the data folder, not in the user's project
            // (SPEC-0048 v5 behaviour 13); jdtls reads this at start, as a system property.
            format!("--jvm-arg=-D{METADATA_AT_ROOT}=false"),
            "-data".to_owned(),
            dir.display().to_string(),
        ];
        // Lombok's annotations are understood when the build declares Lombok and its jar is in the
        // local Maven or Gradle cache (SPEC-0122 v3 behaviour 9); never in safe mode, because the
        // agent is code of the project's dependencies.
        if !self.safe
            && let Some(jar) = lombok_agent(root, &home_dir())
        {
            args.insert(0, format!("--jvm-arg=-javaagent:{}", jar.display()));
        }
        Ok(args)
    }

    /// Starts the server in `root` with its `PATH`.
    pub fn spawn(&self, root: &Path) -> Result<crate::lsp::LspClient, LspError> {
        self.spawn_with_project_args(root, self.project_args(root)?)
    }

    /// Like [`Resolved::spawn`] for a second jdtls of the same project next to the editor's (the
    /// debugger's, `SPEC-0091` v13): its workspace data folder gets `suffix`, because Eclipse locks
    /// a data folder for one process, and the other process would renew it under the first one
    /// when the source roots change.
    pub fn spawn_beside(
        &self,
        root: &Path,
        suffix: &str,
    ) -> Result<crate::lsp::LspClient, LspError> {
        let project = self.project_args_with(root, |root| {
            beside(&crate::java_roots::workspace_data_dir(root), suffix)
        })?;
        self.spawn_with_project_args(root, project)
    }

    fn spawn_with_project_args(
        &self,
        root: &Path,
        project: Vec<String>,
    ) -> Result<crate::lsp::LspClient, LspError> {
        let mut args = self.args();
        args.extend(project.iter().map(String::as_str));
        crate::lsp::LspClient::spawn_with_path(
            &self.command.to_string_lossy(),
            &args,
            root,
            self.path.as_deref().map(OsStr::new),
        )
    }

    /// `initializationOptions` for the server. typescript-language-server gets the TypeScript
    /// installed next to it as fallback, so a project without its own `node_modules/typescript`
    /// still works; jdtls gets signature help switched on, which it leaves off by default
    /// (`SPEC-0042` v3 behaviour 7). In safe mode build execution is switched off where the server
    /// allows it (`SPEC-0278` behaviour 9). jdtls in a `root` without a build file gets the inferred
    /// Java source roots (`SPEC-0042` v8 behaviour 17).
    pub fn initialization_options(&self, root: &Path) -> serde_json::Value {
        match self.candidate.program {
            "typescript-language-server" => {
                // No syntax-only server: it answers references and signature help with nothing
                // until the project has loaded (`SPEC-0042` v10 behaviour 19, OBS-0028 item 5).
                let mut tsserver = serde_json::json!({"useSyntaxServer": "never"});
                if let Some(lib) = typescript_beside(&self.command) {
                    tsserver["fallbackPath"] = serde_json::json!(lib);
                    if self.safe {
                        // Never the project's TypeScript (or its tsserver plugins).
                        tsserver["path"] = serde_json::json!(lib);
                    }
                }
                serde_json::json!({ "tsserver": tsserver })
            }
            // `classFileContentsSupport`: declarations in JDK and library classes come back as
            // `jdt://` places instead of nothing (`SPEC-0042` v7 behaviour 16).
            "jdtls" => {
                let mut settings = if self.safe {
                    jdtls_safe_settings()
                } else {
                    serde_json::json!({"java": {"signatureHelp": {"enabled": true}}})
                };
                // Parameter names before literal arguments (SPEC-0291 behaviour 1).
                settings["java"]["inlayHints"] =
                    serde_json::json!({"parameterNames": {"enabled": "literals"}});
                // Method calls come with empty parentheses and JDK-internal types stay out of the
                // list (SPEC-0072 v7 behaviour 21).
                settings["java"]["completion"] = jdtls_completion_settings();
                // Eclipse's `.project`, `.classpath` and `.settings/` stay in the server's data
                // folder, not in the user's project (SPEC-0048 v5 behaviour 13, OBS-0024 re-check).
                settings["java"]["import"]["generatesMetadataFilesAtProjectRoot"] =
                    serde_json::json!(false);
                let roots = crate::java_roots::source_roots(root);
                if !roots.is_empty() {
                    settings["java"]["project"] = serde_json::json!({"sourcePaths": roots});
                }
                serde_json::json!({
                    "settings": settings,
                    "extendedClientCapabilities": {"classFileContentsSupport": true}
                })
            }
            "rust-analyzer" if self.safe => rust_analyzer_safe_options(),
            // These servers announce and do formatting only when asked to in `initialize`
            // (`SPEC-0043` v4 behaviour 10): HTML, CSS, SCSS, Less and JSON.
            "vscode-html-language-server"
            | "vscode-css-language-server"
            | "vscode-json-language-server" => serde_json::json!({"provideFormatter": true}),
            // An empty element closes without a space before `/>` (`SPEC-0043` v5 behaviour 14).
            "lemminx" => serde_json::json!({"settings": {"xml": {"format": {
                "spaceBeforeEmptyCloseTag": false,
            }}}}),
            // Standalone mode with the TypeScript installed next to the server.
            "vue-language-server" => {
                let tsdk = typescript_beside(&self.command)
                    .map(|l| l.display().to_string())
                    .unwrap_or_default();
                serde_json::json!({"typescript": {"tsdk": tsdk}, "vue": {"hybridMode": false}})
            }
            _ => serde_json::Value::Null,
        }
    }
}

/// rust-analyzer without build scripts, procedural macros and `cargo check` (`SPEC-0278`
/// behaviour 9).
fn rust_analyzer_safe_options() -> serde_json::Value {
    serde_json::json!({
        "cargo": {"buildScripts": {"enable": false}},
        "procMacro": {"enable": false},
        "checkOnSave": false
    })
}

/// How jdtls completes (`SPEC-0072` v7 behaviour 21): no guessed arguments, so a method call is
/// inserted with empty parentheses and the caret inside; internal packages of the JDK left out.
fn jdtls_completion_settings() -> serde_json::Value {
    serde_json::json!({
        "guessMethodArguments": "off",
        "filteredTypes": ["com.sun.*", "sun.*", "jdk.*", "org.graalvm.*"],
        "postfix": {"enabled": true}
    })
}

/// jdtls without Gradle and Maven import (`SPEC-0278` behaviour 9).
fn jdtls_safe_settings() -> serde_json::Value {
    serde_json::json!({"java": {
        "signatureHelp": {"enabled": true},
        "import": {
            "gradle": {"enabled": false, "wrapper": {"enabled": false}},
            "maven": {"enabled": false}
        },
        "configuration": {"updateBuildConfiguration": "disabled"}
    }})
}

/// What a server does in safe mode (`SPEC-0278` behaviour 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SafeMode {
    /// Starts as usual: it runs nothing of the project.
    Unchanged,
    /// Starts with build execution or the project's tools switched off.
    Restricted,
    /// Does not start: it would run project code and cannot be told not to.
    NotStarted,
}

/// How `program` behaves in safe mode, and why it does not start when it does not.
pub fn safe_mode(program: &str) -> (SafeMode, Option<&'static str>) {
    match program {
        "rust-analyzer"
        | "jdtls"
        | "typescript-language-server"
        | "ngserver"
        | "pyright-langserver"
        | "basedpyright-langserver" => (SafeMode::Restricted, None),
        "kotlin-language-server" => (
            SafeMode::NotStarted,
            Some("it runs Gradle or Maven to find the class path"),
        ),
        "metals" => (SafeMode::NotStarted, Some("it imports the sbt build")),
        "sourcekit-lsp" => (
            SafeMode::NotStarted,
            Some("it compiles and runs Package.swift"),
        ),
        "lua-language-server" => (
            SafeMode::NotStarted,
            Some("it runs plugins named in the workspace"),
        ),
        "R" => (
            SafeMode::NotStarted,
            Some("R reads the project's .Rprofile"),
        ),
        "ruby-lsp" => (
            SafeMode::NotStarted,
            Some("it runs Bundler, which evaluates the Gemfile"),
        ),
        _ => (SafeMode::Unchanged, None),
    }
}

/// Why `program` is not started in safe mode, if it is not.
fn not_in_safe_mode(program: &str) -> Option<String> {
    match safe_mode(program) {
        (SafeMode::NotStarted, why) => Some(format!(
            "{program} is not started in safe mode: {} (trust the project to use it)",
            why.unwrap_or("it would run project code")
        )),
        _ => None,
    }
}

impl Resolved {
    /// Settings answered to the server's `workspace/configuration` requests in the workspace
    /// `root`: the selected interpreter for pyright and basedpyright (`SPEC-0170` behaviour 2),
    /// none (null) for other servers. In safe mode Python servers get no project interpreter and
    /// rust-analyzer its safe options (`SPEC-0278` behaviour 9).
    pub fn settings(&self, root: &Path) -> serde_json::Value {
        if crate::python_server::is_python_server(self.candidate.program) {
            if self.safe {
                crate::python_server::settings_for(None)
            } else {
                crate::python_server::settings(root)
            }
        } else if self.candidate.program == "typescript-language-server" {
            typescript_inlay_settings()
        } else if self.candidate.program == "kotlin-language-server" {
            // Parameter names only; the editor keeps those before literals (SPEC-0291 v5
            // behaviour 3d).
            serde_json::json!({"kotlin": {"inlayHints": {
                "typeHints": false,
                "parameterHints": true,
                "chainedHints": false,
            }}})
        } else if self.candidate.program == "vscode-html-language-server" {
            // No blank lines before `head`, `body` and `</html>` (`SPEC-0043` v5 behaviour 14).
            serde_json::json!({"html": {"format": {"extraLiners": ""}}})
        } else if self.candidate.program == "vscode-css-language-server" {
            // Without an answer to its question for these settings the server publishes no
            // diagnostics at all (`SPEC-0042` v16 behaviour 26).
            let on = serde_json::json!({"validate": true});
            serde_json::json!({"css": on, "scss": on, "less": on})
        } else if self.candidate.program == "gopls" {
            // Parameter names; gopls leaves every hint off unless asked (`SPEC-0291` v8
            // behaviour 3e).
            serde_json::json!({"gopls": {"hints": {"parameterNames": true}}})
        } else if self.safe && self.candidate.program == "rust-analyzer" {
            serde_json::json!({"rust-analyzer": rust_analyzer_safe_options()})
        } else {
            serde_json::Value::Null
        }
    }
}

/// Parameter names before literal arguments and the inferred types of variables and return values
/// (`SPEC-0291` v3 behaviours 1 and 2a), the types of class fields without an annotation and the
/// values of enum members without an initializer (v7 behaviour 2b). The server leaves every kind
/// off that is not named here.
fn typescript_inlay_settings() -> serde_json::Value {
    let hints = serde_json::json!({"inlayHints": {
        "includeInlayParameterNameHints": "literals",
        "includeInlayParameterNameHintsWhenArgumentMatchesName": false,
        "includeInlayVariableTypeHints": true,
        "includeInlayVariableTypeHintsWhenTypeMatchesName": false,
        "includeInlayFunctionLikeReturnTypeHints": true,
        "includeInlayPropertyDeclarationTypeHints": true,
        "includeInlayEnumMemberValueHints": true,
    }});
    serde_json::json!({"typescript": hints, "javascript": hints})
}

/// The `typescript/lib` directory in the `node_modules` that holds `program` (symlinks followed).
fn typescript_beside(program: &Path) -> Option<PathBuf> {
    let real = program.canonicalize().ok()?;
    let modules = real.ancestors().find(|a| a.ends_with("node_modules"))?;
    let lib = modules.join("typescript").join("lib");
    lib.join("tsserver.js").is_file().then_some(lib)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The Lombok jar for the project at `root` (`SPEC-0122` v3 behaviour 9): when `pom.xml`,
/// `build.gradle` or `build.gradle.kts` names `lombok`, the version written there (else the newest)
/// from the local Maven repository or Gradle's cache under `home`.
pub fn lombok_agent(root: &Path, home: &Path) -> Option<PathBuf> {
    let build = ["pom.xml", "build.gradle", "build.gradle.kts"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(root.join(f)).ok())
        .find(|t| t.contains("lombok"))?;
    let wanted = build.find("lombok").and_then(|i| {
        let rest = &build[i..build.len().min(i + 200)];
        rest.split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .find(|w| w.split('.').count() >= 3 && w.split('.').all(|p| !p.is_empty()))
            .map(str::to_owned)
    });
    let mut jars: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    let maven = home.join(".m2/repository/org/projectlombok/lombok");
    for v in std::fs::read_dir(&maven).into_iter().flatten().flatten() {
        let version = v.file_name().to_string_lossy().into_owned();
        let jar = v.path().join(format!("lombok-{version}.jar"));
        if jar.is_file() {
            jars.push((version_key(&version), jar));
        }
    }
    let cache = home.join(".gradle/caches/modules-2/files-2.1/org.projectlombok/lombok");
    for v in std::fs::read_dir(&cache).into_iter().flatten().flatten() {
        let version = v.file_name().to_string_lossy().into_owned();
        for h in std::fs::read_dir(v.path()).into_iter().flatten().flatten() {
            let jar = h.path().join(format!("lombok-{version}.jar"));
            if jar.is_file() {
                jars.push((version_key(&version), jar));
            }
        }
    }
    if let Some(w) = wanted
        && let Some((_, jar)) = jars.iter().find(|(k, _)| *k == version_key(&w))
    {
        return Some(jar.clone());
    }
    jars.into_iter()
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, j)| j)
}

fn version_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// jdtls's switch for Eclipse's `.project`, `.classpath` and `.settings/` at the project root.
pub const METADATA_AT_ROOT: &str = "java.import.generatesMetadataFilesAtProjectRoot";

/// The server for `language` with the given `PATH` value (behaviour 3).
pub fn resolve_in(path_var: &OsStr, language: LanguageId) -> Result<Resolved, LspError> {
    resolve_mode(path_var, language, false, None)
}

/// [`resolve_in`]; in safe mode (`safe`) servers that would run project code are skipped, and
/// typescript-language-server only with a TypeScript of its own (`SPEC-0278` behaviour 9). With
/// the workspace `root` a rustup proxy is checked there ([`find_for`]).
fn resolve_mode(
    path_var: &OsStr,
    language: LanguageId,
    safe: bool,
    root: Option<&Path>,
) -> Result<Resolved, LspError> {
    let list = candidates(language);
    if list.is_empty() {
        return Err(LspError::NoServer(format!(
            "{}: IDECraft serves it itself, no language server is used",
            language.lsp_id()
        )));
    }
    let mut problems = Vec::new();
    for candidate in list {
        if safe && let Some(why) = not_in_safe_mode(candidate.program) {
            problems.push(why);
            continue;
        }
        match find_for(path_var, candidate.program, root) {
            Availability::Ready(command)
                if safe
                    && candidate.program == "typescript-language-server"
                    && typescript_beside(&command).is_none() =>
            {
                problems.push(format!(
                    "{} is not started in safe mode: no TypeScript is installed with it, and the project's would run (trust the project to use it)",
                    candidate.program
                ));
            }
            Availability::Ready(command) => {
                let mut r = Resolved::plain(*candidate, command);
                r.safe = safe;
                return Ok(r);
            }
            Availability::Missing => problems.push(format!(
                "{} not found (install: {})",
                candidate.program, candidate.install
            )),
            Availability::Unusable(why) => {
                problems.push(format!("{why} (install: {})", candidate.install))
            }
        }
    }
    Err(LspError::NoServer(format!(
        "{}: {}",
        language.lsp_id(),
        problems.join("; ")
    )))
}

fn path_var() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

/// The server for `language` on this machine.
pub fn resolve(language: LanguageId) -> Result<Resolved, LspError> {
    resolve_in(&path_var(), language)
}

/// Whether `root` is an Angular workspace (`angular.json`).
pub fn is_angular(root: &Path) -> bool {
    root.join("angular.json").is_file()
}

/// The server for `language` in the workspace `root`: HTML templates of an Angular workspace go
/// to the Angular language server when it is installed (`SPEC-0141`), everything else as
/// [`resolve`].
pub fn resolve_for(language: LanguageId, root: &Path) -> Result<Resolved, LspError> {
    resolve_for_in(&path_var(), language, root)
}

/// [`resolve_for`] with the given `PATH` value.
pub fn resolve_for_in(
    path_var: &OsStr,
    language: LanguageId,
    root: &Path,
) -> Result<Resolved, LspError> {
    let safe = !idecraft_core::trust::allows_project_code(root);
    resolve_for_mode(path_var, language, root, safe)
}

/// [`resolve_for_in`] for a project in safe mode (`safe`) or not (`SPEC-0278` behaviour 9).
pub fn resolve_for_mode(
    path_var: &OsStr,
    language: LanguageId,
    root: &Path,
    safe: bool,
) -> Result<Resolved, LspError> {
    if language == LanguageId::Html
        && is_angular(root)
        && let Availability::Ready(command) = find_in(path_var, ANGULAR.program)
    {
        // TypeScript and the Angular language service: the project's first, else the ones
        // installed with the server; in safe mode only the server's own.
        let modules = command.canonicalize().ok().and_then(|real| {
            real.ancestors()
                .find(|a| a.ends_with("node_modules"))
                .map(Path::to_path_buf)
        });
        let mut probes = if safe {
            Vec::new()
        } else {
            vec![root.display().to_string()]
        };
        if let Some(m) = &modules {
            probes.push(m.display().to_string());
            probes.push(m.join("@angular/language-server").display().to_string());
        }
        let probes = probes.join(",");
        return Ok(Resolved {
            prefix_args: Vec::new(),
            path: None,
            candidate: ANGULAR,
            command,
            extra_args: vec![
                "--tsProbeLocations".into(),
                probes.clone(),
                "--ngProbeLocations".into(),
                probes,
            ],
            safe,
        });
    }
    if language == LanguageId::Ruby {
        let mut search = idecraft_core::RubySearch::from_env(root);
        search.path_var = Some(path_var.to_os_string());
        return resolve_ruby_mode(&search, safe);
    }
    resolve_mode(path_var, language, safe, Some(root))
}

/// The Ruby server in the workspace of `search` (`SPEC-0185` behaviours 7–8): looked up with the
/// selected Ruby first on the search path; a candidate locked in `Gemfile.lock` starts through
/// `bundle exec` when `bundle` is found.
pub fn resolve_ruby(search: &idecraft_core::RubySearch) -> Result<Resolved, LspError> {
    resolve_ruby_mode(search, false)
}

/// [`resolve_ruby`]; in safe mode never through `bundle exec`, and without servers that run Bundler
/// (`SPEC-0278` behaviour 9).
fn resolve_ruby_mode(search: &idecraft_core::RubySearch, safe: bool) -> Result<Resolved, LspError> {
    let env = idecraft_core::ruby_environment(search);
    let dirs: Vec<PathBuf> = env
        .server_path
        .iter()
        .map(PathBuf::from)
        .chain(search.path_var.iter().flat_map(std::env::split_paths))
        .collect();
    let path = std::env::join_paths(dirs).unwrap_or_default();
    let bundle = match find_in(&path, "bundle") {
        Availability::Ready(b) => Some(b),
        _ => None,
    };
    let with_path = |mut r: Resolved| {
        r.path = Some(path.to_string_lossy().into_owned());
        r
    };
    let locked = |c: &Candidate| env.bundle.gems.contains_key(c.program);
    // Locked candidates first: the project chose them.
    let ordered = RUBY
        .iter()
        .filter(|c| locked(c))
        .chain(RUBY.iter().filter(|c| !locked(c)));
    let mut problems = Vec::new();
    for candidate in ordered {
        let locked = locked(candidate);
        if safe && let Some(why) = not_in_safe_mode(candidate.program) {
            problems.push(why);
            continue;
        }
        if !safe
            && locked
            && let Some(bundle) = &bundle
        {
            let mut r = Resolved::plain(*candidate, bundle.clone());
            r.prefix_args = vec!["exec".into(), candidate.program.into()];
            return Ok(with_path(r));
        }
        match find_in(&path, candidate.program) {
            Availability::Ready(command) => {
                let mut r = Resolved::plain(*candidate, command);
                r.safe = safe;
                return Ok(with_path(r));
            }
            Availability::Missing => problems.push(format!(
                "{} not found{} (install: {})",
                candidate.program,
                if locked {
                    " and bundle missing although Gemfile.lock has it"
                } else {
                    ""
                },
                candidate.install
            )),
            Availability::Unusable(why) => {
                problems.push(format!("{why} (install: {})", candidate.install))
            }
        }
    }
    Err(LspError::NoServer(format!("ruby: {}", problems.join("; "))))
}

/// One candidate's state for the overview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateStatus {
    /// The candidate.
    #[serde(flatten)]
    pub candidate: Candidate,
    /// Whether and where it is available.
    pub availability: Availability,
    /// What it does in a project in safe mode (`SPEC-0278` behaviour 9).
    pub safe_mode: SafeMode,
}

/// One language in the overview (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageServers {
    /// Language id.
    pub language: &'static str,
    /// Program that would be used, if any.
    pub selected: Option<&'static str>,
    /// All candidates.
    pub candidates: Vec<CandidateStatus>,
}

/// Overview of all languages on this machine.
pub fn overview() -> Vec<LanguageServers> {
    overview_at(None)
}

/// [`overview`] for the project folder `root`, where a rustup proxy is asked (`SPEC-0042` v16
/// behaviour 23).
pub fn overview_for(root: &Path) -> Vec<LanguageServers> {
    overview_at(Some(root))
}

fn overview_at(root: Option<&Path>) -> Vec<LanguageServers> {
    let path = path_var();
    LanguageId::ALL
        .iter()
        .map(|&language| {
            let candidates: Vec<CandidateStatus> = candidates(language)
                .iter()
                .map(|c| CandidateStatus {
                    candidate: *c,
                    availability: find_for(&path, c.program, root),
                    safe_mode: safe_mode(c.program).0,
                })
                .collect();
            LanguageServers {
                language: language.lsp_id(),
                selected: candidates
                    .iter()
                    .find(|c| matches!(c.availability, Availability::Ready(_)))
                    .map(|c| c.candidate.program),
                candidates,
            }
        })
        .collect()
}

/// What the status bar says when a file of `language` opens in `root` and none of its servers is
/// usable (`SPEC-0042` v7 behaviour 15): the language, the programs looked for and how to install
/// them. `None` when a server is usable, or when IDECraft serves the language itself.
pub fn missing_server_message(language: LanguageId, root: &Path) -> Option<String> {
    missing_server_message_in(&path_var(), language, root)
}

/// [`missing_server_message`] with the given `PATH` value.
pub fn missing_server_message_in(
    path_var: &OsStr,
    language: LanguageId,
    root: &Path,
) -> Option<String> {
    if candidates(language).is_empty() {
        return None;
    }
    match resolve_for_in(path_var, language, root) {
        Ok(_) => None,
        Err(LspError::NoServer(why)) => Some(no_server_message(language, &why)),
        Err(e) => Some(e.to_string()),
    }
}

/// The language's name as written in a message: "C++", "JSON", "Markdown".
pub fn language_name(language: LanguageId) -> String {
    match language {
        LanguageId::Cpp => "C++",
        LanguageId::Php => "PHP",
        LanguageId::Html => "HTML",
        LanguageId::Css => "CSS",
        LanguageId::Scss => "SCSS",
        LanguageId::Xml => "XML",
        LanguageId::Yaml => "YAML",
        LanguageId::Toml => "TOML",
        LanguageId::Json => "JSON",
        LanguageId::Sql => "SQL",
        LanguageId::Shell => "shell script",
        other => return format!("{other:?}"),
    }
    .to_owned()
}

/// "No Java language server: jdtls not found (install: …)" for the reason `why` of
/// [`LspError::NoServer`], which starts with the language's LSP id.
pub fn no_server_message(language: LanguageId, why: &str) -> String {
    let name = match language {
        LanguageId::Cpp => "C++".to_owned(),
        LanguageId::Shell => "shell script".to_owned(),
        other => format!("{other:?}"),
    };
    let problems = why
        .strip_prefix(language.lsp_id())
        .and_then(|r| r.strip_prefix(": "))
        .unwrap_or(why);
    format!("No {name} language server: {problems}")
}

/// The data folder next to `dir` for a second server of the same project: `dir` with `-suffix`.
fn beside(dir: &Path, suffix: &str) -> PathBuf {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.with_file_name(format!("{name}-{suffix}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// SPEC-0141 v4 behaviour 9: only the Vue server's diagnostics are pulled.
    #[test]
    fn only_vue_and_typescript_7_pull_diagnostics() {
        for language in LanguageId::ALL {
            for candidate in candidates(language) {
                assert_eq!(
                    candidate.pulls_diagnostics,
                    matches!(candidate.program, "vue-language-server" | "tsc"),
                    "{}",
                    candidate.program
                );
            }
        }
    }

    /// SPEC-0043/AK-7: the HTML, CSS and JSON servers are asked to format; nobody else gets the
    /// option.
    #[test]
    fn web_servers_are_asked_to_format() {
        for language in LanguageId::ALL {
            for candidate in candidates(language) {
                let options = Resolved::plain(*candidate, PathBuf::from(candidate.program))
                    .initialization_options(Path::new("/nonexistent"));
                assert_eq!(
                    options["provideFormatter"] == true,
                    matches!(
                        candidate.program,
                        "vscode-html-language-server"
                            | "vscode-css-language-server"
                            | "vscode-json-language-server"
                    ),
                    "{}",
                    candidate.program
                );
            }
        }
        assert_eq!(language_name(LanguageId::Cpp), "C++");
        assert_eq!(language_name(LanguageId::Json), "JSON");
        assert_eq!(language_name(LanguageId::Markdown), "Markdown");
    }

    /// SPEC-0043/AK-13: the options that give the layout seen next to the reference (OBS-0036
    /// FM2): no blank lines around `head` and `body` from the HTML server, no space before `/>`
    /// from lemminx.
    #[test]
    fn html_and_xml_servers_get_their_format_options() {
        let server = |program: &str| {
            let candidate = LanguageId::ALL
                .into_iter()
                .flat_map(candidates)
                .find(|c| c.program == program)
                .unwrap_or_else(|| panic!("{program} is no candidate"));
            Resolved::plain(*candidate, PathBuf::from(program))
        };
        let root = Path::new("/nonexistent");
        assert_eq!(
            server("vscode-html-language-server").settings(root)["html"]["format"]["extraLiners"],
            ""
        );
        assert_eq!(
            server("lemminx").initialization_options(root)["settings"]["xml"]["format"]["spaceBeforeEmptyCloseTag"],
            false
        );
    }

    /// SPEC-0042/AK-23: a rustup proxy is checked in the project folder, whose toolchain decides
    /// whether the component exists.
    #[test]
    fn rustup_proxy_is_checked_in_the_project() {
        let base = std::env::temp_dir().join(format!("idecraft-rustup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (bin, with, without) = (base.join("bin"), base.join("with"), base.join("without"));
        for dir in [&bin, &with, &without] {
            std::fs::create_dir_all(dir).unwrap();
        }
        // A stand-in rustup: the component exists where the folder says so.
        script(&bin, "rustup", "[ -f ./has-component ]", 0o755);
        std::fs::hard_link(bin.join("rustup"), bin.join("rust-analyzer")).unwrap();
        std::fs::write(with.join("has-component"), "").unwrap();
        let path = bin.clone().into_os_string();

        assert_eq!(
            find_for(&path, "rust-analyzer", Some(&with)),
            Availability::Ready(bin.join("rust-analyzer"))
        );
        let Availability::Unusable(why) = find_for(&path, "rust-analyzer", Some(&without)) else {
            panic!("usable without the component");
        };
        assert!(
            why.contains("is a rustup proxy")
                && why.contains(&without.display().to_string())
                && why.contains("has no rust-analyzer component"),
            "{why}"
        );
        assert!(resolve_for_mode(&path, LanguageId::Rust, &with, false).is_ok());
        let error = resolve_for_mode(&path, LanguageId::Rust, &without, false).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rustup component add rust-analyzer"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// SPEC-0042/AK-22: pyright and basedpyright are asked after their first diagnostics.
    #[test]
    fn only_pyright_lists_the_project_files_first() {
        for language in LanguageId::ALL {
            for candidate in candidates(language) {
                assert_eq!(
                    candidate.lists_files_first,
                    crate::python_server::is_python_server(candidate.program),
                    "{}",
                    candidate.program
                );
            }
        }
    }

    /// SPEC-0042/AK-24: a rustup proxy is asked in the project folder, without letting rustup
    /// install anything, and the message names the toolchain rustup chose there.
    #[test]
    fn rustup_proxy_is_asked_in_the_project_folder() {
        let base =
            std::env::temp_dir().join(format!("idecraft-rustup-asked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cargo = base.join("cargo/bin");
        let (with, without) = (base.join("with"), base.join("without"));
        for d in [&cargo, &with, &without] {
            std::fs::create_dir_all(d).unwrap();
        }
        // The component is there when the folder rustup runs in has `has-ra`; an install would
        // leave a mark.
        let rustup = script(
            &cargo,
            "rustup",
            "[ \"$RUSTUP_AUTO_INSTALL\" = 0 ] || touch \"$(dirname \"$0\")/installed\"\n[ -f has-ra ] && exit 0\necho \"error: unknown binary 'rust-analyzer' in toolchain 'nightly-test'\" >&2\nexit 1",
            0o755,
        );
        std::fs::hard_link(&rustup, cargo.join("rust-analyzer")).unwrap();
        std::fs::write(with.join("has-ra"), "").unwrap();
        let path = std::env::join_paths([&cargo]).unwrap();
        assert!(resolve_for_mode(&path, LanguageId::Rust, &with, false).is_ok());
        assert!(resolve_for_mode(&path, LanguageId::Rust, &with, true).is_ok());
        let err = resolve_for_mode(&path, LanguageId::Rust, &without, false)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rustup proxy without the component in the toolchain nightly-test"),
            "{err}"
        );
        assert!(err.contains(&without.display().to_string()), "{err}");
        assert!(
            err.contains("rustup component add rust-analyzer --toolchain nightly-test"),
            "{err}"
        );
        assert!(
            matches!(
                find_for(&path, "rust-analyzer", Some(&with)),
                Availability::Ready(_)
            ),
            "asked in the folder"
        );
        assert!(!cargo.join("installed").exists(), "rustup may not install");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// SPEC-0042/AK-25 to AK-27 and SPEC-0291/AK-6: which servers are asked later, and the
    /// settings two servers need.
    #[test]
    fn readiness_and_settings_of_clangd_intelephense_css_and_gopls() {
        assert!(candidates(LanguageId::Cpp)[0].diagnoses_first);
        assert!(candidates(LanguageId::C)[0].diagnoses_first);
        let php = candidates(LanguageId::Php);
        assert_eq!(
            (php[0].program, php[0].reports_status),
            ("intelephense", true)
        );
        assert!(php[0].skips_indexing_when_cached);
        assert!(!php[1].reports_status, "phpactor");
        let root = Path::new("/nonexistent");
        let css = Resolved::plain(candidates(LanguageId::Css)[0], PathBuf::from("css"));
        for section in ["css", "scss", "less"] {
            assert_eq!(css.settings(root)[section]["validate"], true, "{section}");
        }
        let go = Resolved::plain(candidates(LanguageId::Go)[0], PathBuf::from("gopls"));
        assert_eq!(go.settings(root)["gopls"]["hints"]["parameterNames"], true);
        let html = Resolved::plain(candidates(LanguageId::Html)[0], PathBuf::from("html"));
        // The HTML server gets none of these; its own are a format option (`SPEC-0043` v5
        // behaviour 14).
        assert_eq!(
            html.settings(root),
            serde_json::json!({"html": {"format": {"extraLiners": ""}}})
        );
    }

    #[test]
    fn missing_server_message() {
        let base = std::env::temp_dir().join(format!("idecraft-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let empty = std::env::join_paths([&bin]).unwrap();
        let msg = missing_server_message_in(&empty, LanguageId::Java, &base).unwrap();
        assert!(
            msg.starts_with("No Java language server: jdtls not found"),
            "{msg}"
        );
        assert!(msg.contains("brew install jdtls"), "{msg}");
        script(&bin, "jdtls", "exit 0", 0o755);
        assert_eq!(
            missing_server_message_in(&empty, LanguageId::Java, &base),
            None
        );
        // SQL is served by IDECraft itself.
        assert_eq!(
            missing_server_message_in(&empty, LanguageId::Sql, &base),
            None
        );
        let msg = missing_server_message_in(&empty, LanguageId::Cpp, &base).unwrap();
        assert!(
            msg.starts_with("No C++ language server: clangd not found"),
            "{msg}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// SPEC-0122 v3 behaviour 9: Lombok's jar from the local caches, the declared version first.
    #[test]
    fn lombok_agent_from_the_local_caches() {
        let base = std::env::temp_dir().join(format!("idecraft-lombok-{}", std::process::id()));
        let (home, root) = (base.join("home"), base.join("p"));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(lombok_agent(&root, &home), None);
        std::fs::write(
            root.join("pom.xml"),
            "<dependency><artifactId>lombok</artifactId><version>1.18.34</version></dependency>",
        )
        .unwrap();
        assert_eq!(lombok_agent(&root, &home), None, "no jar in the caches");
        for v in ["1.18.30", "1.18.34", "1.18.36"] {
            let d = home.join(format!(".m2/repository/org/projectlombok/lombok/{v}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("lombok-{v}.jar")), "").unwrap();
        }
        let jar = lombok_agent(&root, &home).unwrap();
        assert!(jar.ends_with("1.18.34/lombok-1.18.34.jar"), "{jar:?}");
        std::fs::write(root.join("pom.xml"), "<artifactId>lombok</artifactId>").unwrap();
        let jar = lombok_agent(&root, &home).unwrap();
        assert!(
            jar.ends_with("lombok-1.18.36.jar"),
            "the newest without a version: {jar:?}"
        );
        std::fs::remove_file(root.join("pom.xml")).unwrap();
        let d =
            home.join(".gradle/caches/modules-2/files-2.1/org.projectlombok/lombok/1.18.40/abc");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("lombok-1.18.40.jar"), "").unwrap();
        std::fs::write(
            root.join("build.gradle.kts"),
            "dependencies { compileOnly(\"org.projectlombok:lombok:1.18.40\") }",
        )
        .unwrap();
        let jar = lombok_agent(&root, &home).unwrap();
        assert!(jar.ends_with("abc/lombok-1.18.40.jar"), "{jar:?}");
        std::fs::remove_dir_all(&base).ok();
    }

    /// SPEC-0091 v13: the debugger's jdtls keeps its own data folder next to the editor's.
    #[test]
    fn a_second_jdtls_gets_a_folder_beside() {
        assert_eq!(
            beside(Path::new("/c/jdtls/shop-0a1b"), "debug"),
            PathBuf::from("/c/jdtls/shop-0a1b-debug")
        );
    }

    /// SPEC-0042 AK-14: jdtls gets `-data` with a data folder recording the roots; others nothing.
    #[test]
    fn jdtls_gets_its_workspace_data() {
        let base = std::env::temp_dir().join(format!("idecraft-jdtls-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("project");
        std::fs::create_dir_all(root.join("src/main/java/a")).unwrap();
        std::fs::write(root.join("src/main/java/a/A.java"), "package a;").unwrap();
        let cache = base.join("cache");
        let data = |r: &Path| crate::java_roots::workspace_data_dir_in(&cache, r);
        let jdtls = Resolved::plain(JAVA[0], PathBuf::from("/bin/jdtls"));
        let args = jdtls.project_args_with(&root, data).unwrap();
        // SPEC-0048 v5 behaviour 13: no Eclipse files in the project.
        assert_eq!(
            args[0],
            "--jvm-arg=-Djava.import.generatesMetadataFilesAtProjectRoot=false"
        );
        assert_eq!(args[1], "-data");
        let dir = PathBuf::from(&args[2]);
        assert!(dir.starts_with(&cache), "{args:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join("idecraft-source-roots")).unwrap(),
            "src/main/java"
        );
        let clangd = Resolved::plain(CLANGD[0], PathBuf::from("/bin/clangd"));
        assert!(clangd.project_args_with(&root, data).unwrap().is_empty());
        assert!(!cache.join("clangd").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    fn script(dir: &Path, name: &str, body: &str, mode: u32) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    /// An npm prefix with typescript-language-server and TypeScript `version` as `npm install -g`
    /// lays them out: `bin/` links into `lib/node_modules/`. `tsserver` gives TypeScript 5's
    /// `lib/tsserver.js`, `native` TypeScript 7's native program for this system.
    fn npm_prefix(base: &Path, version: &str, tsserver: bool, native: bool) -> PathBuf {
        let _ = std::fs::remove_dir_all(base);
        let modules = base.join("lib/node_modules");
        let ls = modules.join("typescript-language-server/lib");
        let ts = modules.join("typescript");
        for d in [&ls, &ts.join("lib"), &ts.join("bin"), &base.join("bin")] {
            std::fs::create_dir_all(d).unwrap();
        }
        script(&ls, "cli.mjs", "exit 0", 0o755);
        script(&ts.join("bin"), "tsc", "exit 0", 0o755);
        std::fs::write(
            ts.join("package.json"),
            format!("{{\"name\": \"typescript\", \"version\": \"{version}\"}}"),
        )
        .unwrap();
        if tsserver {
            std::fs::write(ts.join("lib/tsserver.js"), "").unwrap();
        } else {
            std::fs::write(ts.join("lib/getExePath.js"), "").unwrap();
        }
        if native {
            let dir = ts.join("node_modules").join(native_package()).join("lib");
            std::fs::create_dir_all(&dir).unwrap();
            script(&dir, "tsc", "exit 0", 0o755);
        }
        std::os::unix::fs::symlink(
            "../lib/node_modules/typescript-language-server/lib/cli.mjs",
            base.join("bin/typescript-language-server"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            "../lib/node_modules/typescript/bin/tsc",
            base.join("bin/tsc"),
        )
        .unwrap();
        base.join("bin")
    }

    /// SPEC-0042 v12 AK-19 (behaviour 21): TypeScript 7 next to typescript-language-server starts
    /// its own server from its native program; without that program, and for TypeScript 5's `tsc`,
    /// the reasons and the fixes are named; TypeScript 5 keeps typescript-language-server.
    #[test]
    fn typescript_7_starts_its_own_server() {
        let base = std::env::temp_dir().join(format!("idecraft-ts7-{}", std::process::id()));
        // TypeScript 7 with its native program.
        let bin = npm_prefix(&base.join("seven"), "7.0.2", false, true);
        let path = std::env::join_paths([&bin]).unwrap();
        let r = resolve_in(&path, LanguageId::TypeScript).unwrap();
        assert_eq!(r.candidate.program, "tsc");
        assert_eq!(r.args(), ["--lsp", "--stdio"]);
        let native = base
            .join("seven/lib/node_modules/typescript/node_modules")
            .join(native_package())
            .join("lib/tsc");
        assert_eq!(r.command, native.canonicalize().unwrap());
        let Availability::Unusable(why) = find_in(&path, "typescript-language-server") else {
            panic!("typescript-language-server with TypeScript 7 is not usable");
        };
        assert!(
            why.contains("7.0.2") && why.contains("npm install -g typescript@5"),
            "{why}"
        );
        // Also in safe mode: it is IDECraft's TypeScript, not the project's.
        assert_eq!(
            resolve_mode(&path, LanguageId::TypeScript, true, None)
                .unwrap()
                .candidate
                .program,
            "tsc"
        );

        // TypeScript 7 without the native program for this system: said, with the fix.
        let bin = npm_prefix(&base.join("lacking"), "7.0.2", false, false);
        let path = std::env::join_paths([&bin]).unwrap();
        let err = resolve_in(&path, LanguageId::TypeScript)
            .unwrap_err()
            .to_string();
        assert!(err.contains("lacks its native program"), "{err}");
        assert!(err.contains(&native_package()), "{err}");
        assert!(err.contains("npm install -g typescript@7"), "{err}");

        // TypeScript 5: typescript-language-server as before; its `tsc` has no server.
        let bin = npm_prefix(&base.join("five"), "5.6.3", true, false);
        let path = std::env::join_paths([&bin]).unwrap();
        let r = resolve_in(&path, LanguageId::TypeScript).unwrap();
        assert_eq!(r.candidate.program, "typescript-language-server");
        let Availability::Unusable(why) = find_in(&path, "tsc") else {
            panic!("TypeScript 5's tsc is no language server");
        };
        assert!(
            why.contains("5.6.3") && why.contains("no language server"),
            "{why}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolution_order_and_problems() {
        let base = std::env::temp_dir().join(format!("idecraft-servers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b, cargo) = (base.join("a"), base.join("b"), base.join("cargo"));
        for d in [&a, &b, &cargo] {
            std::fs::create_dir_all(d).unwrap();
        }
        // Python: pyright in `b` is used because basedpyright in `a` is not executable.
        script(&a, "basedpyright-langserver", "exit 0", 0o644);
        let pyright = script(&b, "pyright-langserver", "exit 0", 0o755);
        script(&b, "pylsp", "exit 0", 0o755);
        let path = std::env::join_paths([&a, &b]).unwrap();
        let r = resolve_in(&path, LanguageId::Python).unwrap();
        assert_eq!(
            (r.candidate.program, r.command),
            ("pyright-langserver", pyright)
        );
        assert!(matches!(
            find_in(&path, "basedpyright-langserver"),
            Availability::Unusable(why) if why.contains("not executable")
        ));

        // A rustup proxy without the component, then with it.
        let rustup = script(
            &cargo,
            "rustup",
            "[ -f \"$(dirname \"$0\")/has-ra\" ]",
            0o755,
        );
        std::fs::hard_link(&rustup, cargo.join("rust-analyzer")).unwrap();
        let path = std::env::join_paths([&cargo]).unwrap();
        let err = resolve_in(&path, LanguageId::Rust).unwrap_err().to_string();
        assert!(err.contains("rustup proxy without the component"), "{err}");
        assert!(err.contains("rustup component add rust-analyzer"), "{err}");
        std::fs::write(cargo.join("has-ra"), "").unwrap();
        assert!(resolve_in(&path, LanguageId::Rust).is_ok());

        // Nothing at all: every candidate with its install hint.
        let empty = std::env::join_paths([&base.join("none")]).unwrap();
        let err = resolve_in(&empty, LanguageId::Ruby)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ruby-lsp not found") && err.contains("gem install solargraph"));
        let sql = resolve_in(&empty, LanguageId::Sql).unwrap_err().to_string();
        assert!(sql.contains("IDECraft serves it itself"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ruby_with_its_ruby_and_bundler() {
        let base = std::env::temp_dir().join(format!("idecraft-ruby-srv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (home, root, other) = (base.join("home"), base.join("project"), base.join("other"));
        let bin = home.join(".rbenv/versions/3.3.6/bin");
        for d in [&bin, &root, &other] {
            std::fs::create_dir_all(d).unwrap();
        }
        script(&bin, "ruby", "exit 0", 0o755);
        script(&bin, "bundle", "exit 0", 0o755);
        let solargraph = script(&bin, "solargraph", "exit 0", 0o755);
        std::fs::write(root.join(".ruby-version"), "3.3.6\n").unwrap();
        let search = idecraft_core::RubySearch {
            root: root.clone(),
            path_var: Some(std::env::join_paths([&other]).unwrap()),
            home: Some(home.clone()),
            rbenv_root: Some(home.join(".rbenv")),
            ..Default::default()
        };
        let path_first = |r: &Resolved| {
            r.path
                .as_deref()
                .and_then(|p| std::env::split_paths(p).next())
        };

        // Not in the bundle: the program from the selected Ruby's bin.
        let r = resolve_ruby(&search).unwrap();
        assert_eq!((r.command.clone(), r.args()), (solargraph, vec!["stdio"]));
        assert_eq!(path_first(&r), Some(bin.clone()));

        // Locked in Gemfile.lock: through `bundle exec`.
        std::fs::write(root.join("Gemfile"), "gem 'solargraph'\n").unwrap();
        std::fs::write(
            root.join("Gemfile.lock"),
            "GEM\n  remote: https://rubygems.org/\n  specs:\n    solargraph (0.49.0)\n      thor (~> 1.0)\n\nBUNDLED WITH\n   2.5.22\n",
        )
        .unwrap();
        let r = resolve_ruby(&search).unwrap();
        assert_eq!(r.command, bin.join("bundle"));
        assert_eq!(r.args(), ["exec", "solargraph", "stdio"]);
        assert_eq!(r.candidate.program, "solargraph");
        assert_eq!(path_first(&r), Some(bin.clone()));
        assert!(r.command_line().ends_with("bundle exec solargraph stdio"));

        // The locked solargraph wins over a usable but unlocked ruby-lsp ...
        let ruby_lsp = script(&other, "ruby-lsp", "exit 0", 0o755);
        let r = resolve_ruby(&search).unwrap();
        assert_eq!(r.args(), ["exec", "solargraph", "stdio"]);
        // ... and without the lockfile ruby-lsp comes first.
        std::fs::remove_file(root.join("Gemfile.lock")).unwrap();
        let r = resolve_ruby(&search).unwrap();
        assert_eq!((r.command, r.prefix_args.len()), (ruby_lsp, 0));

        // Nothing usable: both install hints.
        let empty = idecraft_core::RubySearch {
            root: base.join("none"),
            path_var: Some(std::env::join_paths([&base.join("none")]).unwrap()),
            ..Default::default()
        };
        let err = resolve_ruby(&empty).unwrap_err().to_string();
        assert!(
            err.contains("gem install ruby-lsp") && err.contains("gem install solargraph"),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(all(test, unix))]
#[path = "servers_safe_tests.rs"]
mod safe_mode_tests;
