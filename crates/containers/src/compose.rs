//! Docker Compose (`SPEC-0242`): the project's Compose file and services read by Ferris IDEA itself,
//! states from the container labels, up and down through the Compose CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{Container, DockerHost};

/// Time an up or down may take (image pulls included).
const RUN_TIMEOUT: Duration = Duration::from_secs(600);

/// Main file names in the order Compose tries them, each with its override file names.
const FILES: [(&str, [&str; 2]); 4] = [
    (
        "compose.yaml",
        ["compose.override.yaml", "compose.override.yml"],
    ),
    (
        "compose.yml",
        ["compose.override.yaml", "compose.override.yml"],
    ),
    (
        "docker-compose.yaml",
        [
            "docker-compose.override.yaml",
            "docker-compose.override.yml",
        ],
    ),
    (
        "docker-compose.yml",
        [
            "docker-compose.override.yaml",
            "docker-compose.override.yml",
        ],
    ),
];

/// A Compose project of a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeProject {
    /// Folder the project belongs to.
    pub dir: PathBuf,
    /// Compose file and its override file, if any.
    pub files: Vec<PathBuf>,
    /// Project name.
    pub name: String,
    /// Service names in file order (main file first, new ones of the override after).
    pub services: Vec<String>,
    /// Why the services could not be read.
    pub error: Option<String>,
}

/// A service with the state of its containers (behaviour 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceState {
    /// Service name.
    pub service: String,
    /// `running`, `n/m running`, `exited` or `not created`.
    pub state: String,
    /// Names of its containers.
    pub containers: Vec<String>,
}

/// What an action does (behaviour 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComposeAction {
    /// `up --detach`.
    Up,
    /// `down` for the project, `rm --stop --force` for a service.
    Down,
}

/// The Compose project of `dir`, if it has a Compose file (behaviours 1, 2).
pub fn detect(dir: &Path) -> Option<ComposeProject> {
    let (main, overrides) = FILES.iter().find(|(f, _)| dir.join(f).is_file())?;
    let mut files = vec![dir.join(main)];
    if let Some(o) = overrides.iter().find(|o| dir.join(o).is_file()) {
        files.push(dir.join(o));
    }
    let mut name = None;
    let mut services: Vec<String> = Vec::new();
    let mut error = None;
    for file in &files {
        let read = std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .map(|t| read(&t));
        match read {
            Ok((n, s, problem)) => {
                name = name.or(n);
                for svc in s {
                    if !services.contains(&svc) {
                        services.push(svc);
                    }
                }
                if let Some(p) = problem {
                    error.get_or_insert_with(|| {
                        format!(
                            "{}: {p}",
                            file.file_name().unwrap_or_default().to_string_lossy()
                        )
                    });
                }
            }
            Err(e) => {
                error.get_or_insert_with(|| {
                    format!(
                        "{}: {e}",
                        file.file_name().unwrap_or_default().to_string_lossy()
                    )
                });
            }
        }
    }
    Some(ComposeProject {
        dir: dir.to_path_buf(),
        files,
        name: name.unwrap_or_else(|| {
            project_name(&dir.file_name().unwrap_or_default().to_string_lossy())
        }),
        services,
        error,
    })
}

/// A folder name made a valid project name: lowercase, only letters, digits, `_` and `-`, starting
/// with a letter or digit.
pub fn project_name(folder: &str) -> String {
    let s: String = folder
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    s.trim_start_matches(['_', '-']).to_owned()
}

/// Strips a comment and surrounding quotes from a key or scalar.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = match s.find(" #") {
        Some(i) => s[..i].trim_end(),
        None => s,
    };
    s.trim_matches(|c| c == '"' || c == '\'').to_owned()
}

/// The top-level `name`, the keys of the top-level `services` block mapping and what could not be
/// read.
fn read(text: &str) -> (Option<String>, Vec<String>, Option<String>) {
    let mut problem = None;
    let mut name = None;
    let mut services = Vec::new();
    let mut in_services = false;
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let content = line.trim_end();
        let trimmed = content.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        let depth = content.len() - trimmed.len();
        if depth == 0 {
            in_services = false;
            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            match unquote(key).as_str() {
                "name" => name = Some(unquote(value)).filter(|n| !n.is_empty()),
                "services" => {
                    let v = unquote(value);
                    if v.starts_with('{') {
                        problem = Some("services in flow style ({…}) are not read".to_owned());
                    } else {
                        in_services = true;
                    }
                }
                _ => {}
            }
            continue;
        }
        if !in_services {
            continue;
        }
        let level = *indent.get_or_insert(depth);
        if depth == level
            && let Some((key, _)) = trimmed.split_once(':')
        {
            let key = unquote(key);
            if !key.is_empty() && !key.starts_with('-') {
                services.push(key);
            }
        }
    }
    (name, services, problem)
}

/// States of the project's services from the containers' labels (behaviour 3).
pub fn service_states(project: &ComposeProject, containers: &[Container]) -> Vec<ServiceState> {
    project
        .services
        .iter()
        .map(|svc| {
            let mine: Vec<&Container> = containers
                .iter()
                .filter(|c| {
                    c.compose_project.as_deref() == Some(project.name.as_str())
                        && c.compose_service.as_deref() == Some(svc.as_str())
                })
                .collect();
            let running = mine.iter().filter(|c| c.state == "running").count();
            let state = match (running, mine.len()) {
                (_, 0) => "not created".to_owned(),
                (r, n) if r == n && n == 1 => "running".to_owned(),
                (0, _) => "exited".to_owned(),
                (r, n) => format!("{r}/{n} running"),
            };
            ServiceState {
                service: svc.clone(),
                state,
                containers: mine.iter().map(|c| c.name.clone()).collect(),
            }
        })
        .collect()
}

/// The Compose CLI: the Docker CLI with its plugin, or the standalone program (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeCli {
    /// Program to run.
    pub program: PathBuf,
    /// Arguments before Compose's own (`["compose"]` for the plugin).
    pub prefix: Vec<String>,
}

fn on_path(path_var: Option<&std::ffi::OsStr>, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

impl ComposeCli {
    /// `docker compose` when the Docker CLI has the plugin, else `docker-compose` on `PATH`.
    pub fn find(path_var: Option<&std::ffi::OsStr>) -> Option<Self> {
        if let Some(docker) = on_path(path_var, "docker") {
            let plugin = Command::new(&docker)
                .args(["compose", "version"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if plugin {
                return Some(Self {
                    program: docker,
                    prefix: vec!["compose".into()],
                });
            }
        }
        on_path(path_var, "docker-compose").map(|program| Self {
            program,
            prefix: Vec::new(),
        })
    }

    /// The arguments of an action on the project or one service (behaviour 4).
    pub fn args(
        &self,
        project: &ComposeProject,
        action: ComposeAction,
        service: Option<&str>,
    ) -> Result<Vec<String>, String> {
        if let Some(s) = service
            && !project.services.iter().any(|x| x == s)
        {
            return Err(format!("no service `{s}` in {}", project.name));
        }
        let mut args = self.prefix.clone();
        args.extend([
            "--project-directory".into(),
            project.dir.display().to_string(),
        ]);
        for f in &project.files {
            args.extend(["-f".into(), f.display().to_string()]);
        }
        args.extend(["-p".into(), project.name.clone()]);
        match (action, service) {
            (ComposeAction::Up, s) => {
                args.extend(["up".into(), "--detach".into()]);
                args.extend(s.map(str::to_owned));
            }
            (ComposeAction::Down, None) => args.push("down".into()),
            (ComposeAction::Down, Some(s)) => {
                args.extend(["rm".into(), "--stop".into(), "--force".into(), s.to_owned()]);
            }
        }
        Ok(args)
    }

    /// The whole command line, for previews.
    pub fn describe(&self, args: &[String]) -> Vec<String> {
        std::iter::once(self.program.display().to_string())
            .chain(args.iter().cloned())
            .collect()
    }

    /// Runs an action against engine `host` and returns the CLI's output (behaviour 5).
    pub fn run(
        &self,
        project: &ComposeProject,
        host: &DockerHost,
        action: ComposeAction,
        service: Option<&str>,
    ) -> Result<String, String> {
        let args = self.args(project, action, service)?;
        let mut cmd = Command::new(&self.program);
        cmd.args(&args)
            .current_dir(&project.dir)
            .env("DOCKER_HOST", host.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let DockerHost::Ssh { .. } = host {
            cmd.env_remove("DOCKER_TLS_VERIFY");
            // The CLI starts `ssh` from PATH itself: give it the stand-in with the shared options
            // and the route over jump hosts (post-quantum key exchange on every hop, ADR-0025).
            let shim = ferris_idea_remote::SshCommand::stand_in_dir().map_err(|e| e.to_string())?;
            let mut path = vec![shim];
            if let Some(old) = std::env::var_os("PATH") {
                path.extend(std::env::split_paths(&old));
            }
            cmd.env(
                "PATH",
                std::env::join_paths(path).map_err(|e| e.to_string())?,
            );
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        // Both streams line by line in the order they arrive (OBS-0116 D7).
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                if let Some(p) = pipe {
                    use std::io::BufRead;
                    for line in std::io::BufReader::new(p).lines().map_while(Result::ok) {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                }
            })
        };
        let out = read(child.stdout.take().map(|o| Box::new(o) as _));
        let err = read(child.stderr.take().map(|e| Box::new(e) as _));
        drop(tx);
        let started = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s;
            }
            if started.elapsed() > RUN_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no end within {} s", RUN_TIMEOUT.as_secs()));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let _ = (out.join(), err.join());
        let text = once_each(rx.try_iter());
        if status.success() {
            Ok(text)
        } else {
            Err(text
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("failed")
                .trim()
                .to_owned())
        }
    }
}

/// The CLI's lines joined, each line once: Compose repeats a progress line without a terminal
/// (`Network x Creating` twice), and a line repeated right after itself says nothing new.
pub fn once_each(lines: impl IntoIterator<Item = String>) -> String {
    let mut out = String::new();
    let mut last: Option<String> = None;
    for line in lines {
        let key = line.trim_end().to_owned();
        if last.as_deref() == Some(key.as_str()) {
            continue;
        }
        out.push_str(&line);
        out.push('\n');
        last = Some(key);
    }
    out
}

/// The last line of a CLI output worth showing in a status line.
pub fn last_line(text: &str) -> String {
    text.lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or("done")
        .trim()
        .to_owned()
}

/// The last `tail` lines of a service's containers, prefixed with the container name when there
/// are several (behaviour 4).
pub fn service_logs(
    host: &DockerHost,
    state: &ServiceState,
    tail: usize,
) -> Result<String, crate::DockerError> {
    let several = state.containers.len() > 1;
    let mut out = String::new();
    for c in &state.containers {
        let text = host.logs(c, tail)?;
        for line in text.lines() {
            if several {
                out.push_str(c);
                out.push_str(" | ");
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_once() {
        let lines = ["a", "a", "b ", "b", "a", "c"].map(String::from);
        assert_eq!(once_each(lines), "a\nb \na\nc\n");
        assert_eq!(once_each(Vec::<String>::new()), "");
    }

    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ferris-idea-compose-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn files_names_and_services() {
        let d = dir("My Shop");
        assert!(detect(&d).is_none());
        std::fs::write(
            d.join("docker-compose.yml"),
            "services:\n  old:\n    image: x\n",
        )
        .unwrap();
        std::fs::write(
            d.join("compose.yaml"),
            "# shop\nservices:\n  web:   # front\n    image: nginx\n    ports:\n      - \"8080:80\"\n  \"db\":\n    image: postgres\n    environment:\n      POSTGRES_DB: shop\nvolumes:\n  data:\n",
        )
        .unwrap();
        std::fs::write(
            d.join("compose.override.yml"),
            "services:\n  web:\n    environment:\n      DEBUG: 1\n  cache:\n    image: redis\n",
        )
        .unwrap();
        let p = detect(&d).unwrap();
        assert_eq!(
            p.files,
            [d.join("compose.yaml"), d.join("compose.override.yml")]
        );
        assert_eq!(
            p.name,
            format!("ferris-idea-compose-myshop-{}", std::process::id())
        );
        assert_eq!(p.services, ["web", "db", "cache"]);
        assert_eq!(p.error, None);
        std::fs::write(
            d.join("compose.yaml"),
            "name: \"shop\"\nservices: {web: {image: x}}\n",
        )
        .unwrap();
        let p = detect(&d).unwrap();
        assert_eq!(p.name, "shop");
        assert!(p.error.unwrap().contains("flow style"));
        let _ = std::fs::remove_dir_all(d);
        assert_eq!(project_name("_My App.2"), "myapp2");
    }

    #[test]
    fn states_and_command_lines() {
        let p = ComposeProject {
            dir: "/p".into(),
            files: vec!["/p/compose.yaml".into()],
            name: "shop".into(),
            services: vec!["web".into(), "db".into(), "cache".into()],
            error: None,
        };
        let c = |name: &str, svc: &str, state: &str| Container {
            id: name.into(),
            name: name.into(),
            image: "x".into(),
            image_id: String::new(),
            state: state.into(),
            status: String::new(),
            ports: vec![],
            compose_project: Some("shop".into()),
            compose_service: Some(svc.into()),
            compose_dir: None,
            networks: Vec::new(),
        };
        let states = service_states(
            &p,
            &[
                c("shop-web-1", "web", "running"),
                c("shop-web-2", "web", "exited"),
                c("shop-db-1", "db", "exited"),
            ],
        );
        let s: Vec<(&str, &str)> = states
            .iter()
            .map(|s| (s.service.as_str(), s.state.as_str()))
            .collect();
        assert_eq!(
            s,
            [
                ("web", "1/2 running"),
                ("db", "exited"),
                ("cache", "not created")
            ]
        );
        let cli = ComposeCli {
            program: "/bin/docker".into(),
            prefix: vec!["compose".into()],
        };
        let base = "compose --project-directory /p -f /p/compose.yaml -p shop";
        let line = |a, s| cli.args(&p, a, s).unwrap().join(" ");
        assert_eq!(line(ComposeAction::Up, None), format!("{base} up --detach"));
        assert_eq!(
            line(ComposeAction::Up, Some("web")),
            format!("{base} up --detach web")
        );
        assert_eq!(line(ComposeAction::Down, None), format!("{base} down"));
        assert_eq!(
            line(ComposeAction::Down, Some("db")),
            format!("{base} rm --stop --force db")
        );
        assert!(!line(ComposeAction::Down, None).contains("-v"));
        assert!(cli.args(&p, ComposeAction::Up, Some("nope")).is_err());
    }
}
