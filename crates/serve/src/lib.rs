//! `idecraft serve` (`SPEC-0317` v2 detail 6, `ADR-0026`, M10.3): the headless backend of a
//! remote project. `--stdio` speaks the session protocol on standard input and output (the SSH
//! channel); standard error carries log lines only. `--attach <session>` bridges standard input
//! and output to a waiting session's reattach socket. `--pair` (`SPEC-0321`) starts the encrypted
//! listener for hosts without SSH, on loopback unless the network is explicitly allowed; without it
//! nothing listens on the network. `--wss` (`SPEC-0325`) starts the secure WebSocket endpoint for
//! browsers: TLS only, authenticated, on loopback unless the network is explicitly allowed.

pub mod device;
pub mod pair;
pub mod wss;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use idecraft_session::{Backend, BackendConfig, Transport};

struct Options {
    folder: Option<PathBuf>,
    stdio: bool,
    attach: Option<String>,
    state_dir: Option<PathBuf>,
    run_dir: Option<PathBuf>,
    reattach_minutes: Option<u64>,
    pair: bool,
    listen: Option<std::net::SocketAddr>,
    allow_network: bool,
    new_device: bool,
    grant: Option<Vec<idecraft_session::Right>>,
    key_file: bool,
    devices: bool,
    revoke: Option<String>,
    qr: bool,
    rights: Option<(String, Vec<idecraft_session::Right>)>,
    device_log: Option<Option<String>>,
    access_off: bool,
    wss: bool,
    wss_cert: Option<PathBuf>,
    wss_key: Option<PathBuf>,
    wss_origins: Vec<String>,
    wss_rights: Option<Vec<idecraft_session::Right>>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        folder: None,
        stdio: false,
        attach: None,
        state_dir: None,
        run_dir: None,
        reattach_minutes: None,
        pair: false,
        listen: None,
        allow_network: false,
        new_device: false,
        grant: None,
        key_file: false,
        devices: false,
        revoke: None,
        qr: false,
        rights: None,
        device_log: None,
        access_off: false,
        wss: false,
        wss_cert: None,
        wss_key: None,
        wss_origins: Vec::new(),
        wss_rights: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a.as_str() {
            "--stdio" => o.stdio = true,
            "--wss" => o.wss = true,
            "--wss-cert" => o.wss_cert = Some(PathBuf::from(value("--wss-cert")?)),
            "--wss-key" => o.wss_key = Some(PathBuf::from(value("--wss-key")?)),
            "--wss-origin" => o.wss_origins.push(value("--wss-origin")?),
            "--wss-rights" => o.wss_rights = Some(pair::parse_rights(&value("--wss-rights")?)?),
            "--pair" => o.pair = true,
            "--allow-network" => o.allow_network = true,
            "--new-device" => o.new_device = true,
            "--key-file" => o.key_file = true,
            "--devices" => o.devices = true,
            "--revoke" => o.revoke = Some(value("--revoke")?),
            "--qr" => o.qr = true,
            "--access-off" => o.access_off = true,
            "--device-log" => {
                o.device_log = Some(None);
            }
            "--device-log-of" => o.device_log = Some(Some(value("--device-log-of")?)),
            "--rights" => {
                let fingerprint = value("--rights")?;
                let list = pair::parse_rights(&value("--rights")?)?;
                o.rights = Some((fingerprint, list));
            }
            "--grant" => o.grant = Some(pair::parse_rights(&value("--grant")?)?),
            "--listen" => {
                let text = value("--listen")?;
                o.listen = Some(
                    text.parse()
                        .map_err(|_| format!("--listen takes <address>:<port>, not {text}"))?,
                );
            }
            "--attach" => o.attach = Some(value("--attach")?),
            "--state-dir" => o.state_dir = Some(PathBuf::from(value("--state-dir")?)),
            "--run-dir" => o.run_dir = Some(PathBuf::from(value("--run-dir")?)),
            "--reattach-minutes" => {
                let n: u64 = value("--reattach-minutes")?
                    .parse()
                    .map_err(|_| "--reattach-minutes takes a number".to_owned())?;
                if !(1..=60).contains(&n) {
                    return Err("--reattach-minutes is between 1 and 60".into());
                }
                o.reattach_minutes = Some(n);
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            folder if o.folder.is_none() => o.folder = Some(PathBuf::from(folder)),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(o)
}

/// The peer as the audit log names it: `ssh:<user>@<window host>` behind SSH.
fn peer() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    match std::env::var("SSH_CLIENT") {
        Ok(c) => format!(
            "ssh:{user}@{}",
            c.split_whitespace().next().unwrap_or("unknown")
        ),
        Err(_) => format!("local:{user}"),
    }
}

/// Runs `idecraft serve …`.
pub fn serve_cli(args: &[String]) -> ExitCode {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let run_dir = o
        .run_dir
        .clone()
        .or_else(idecraft_session::reattach::default_run_dir);
    if let Some(id) = &o.attach {
        let Some(dir) = run_dir else {
            eprintln!("error: no folder for reattach sockets (HOME unset)");
            return ExitCode::from(2);
        };
        #[cfg(unix)]
        {
            return match idecraft_session::reattach::bridge_stdio(&dir, id) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: session {id} cannot be reattached: {e}");
                    ExitCode::from(3)
                }
            };
        }
        #[cfg(not(unix))]
        {
            let _ = (dir, id);
            eprintln!("error: reattaching needs a Unix host");
            return ExitCode::FAILURE;
        }
    }
    let Some(state) = o.state_dir.clone().or_else(idecraft_core::user_config_dir) else {
        eprintln!("error: no folder for the key store and the audit log (HOME unset)");
        return ExitCode::from(2);
    };
    if o.devices {
        return pair::list_devices(&state, o.key_file);
    }
    if let Some(fingerprint) = &o.revoke {
        return pair::revoke(&state, fingerprint, o.key_file);
    }
    if let Some((fingerprint, rights)) = &o.rights {
        return pair::set_rights(&state, fingerprint, rights, o.key_file);
    }
    if let Some(fingerprint) = &o.device_log {
        return pair::show_log(&state, fingerprint.as_deref());
    }
    if o.access_off {
        return pair::switch_off(&state, o.key_file);
    }
    if !o.stdio && !o.pair && !o.wss {
        eprintln!("error: idecraft serve needs --stdio, --pair or --wss");
        return ExitCode::from(2);
    }
    if [o.stdio, o.pair, o.wss].into_iter().filter(|m| *m).count() > 1 {
        eprintln!("error: --stdio, --pair and --wss exclude each other");
        return ExitCode::from(2);
    }
    let mut config = BackendConfig {
        build: idecraft_core::release::version_line(),
        folder: o.folder.clone(),
        state_dir: o.state_dir.clone().or_else(idecraft_core::user_config_dir),
        run_dir,
        peer: peer(),
        registry: idecraft_editor::documents::DocumentRegistry::global().clone(),
        ..BackendConfig::default()
    };
    if let Some(m) = o.reattach_minutes {
        config.reattach = Duration::from_secs(m * 60);
    }
    // Untrusted host projects run nothing until the window trusts them (SPEC-0318 behaviour 5).
    idecraft_core::trust::enforce();
    // Git over SSH on the host follows the host's own settings (SPEC-0318 v5 behaviour 33).
    if let Some(file) = idecraft_core::ide_settings_file() {
        let settings = idecraft_core::load_ide_settings(&file);
        idecraft_remote::set_azure_classic_kex(settings.git.ssh.allow_classic_kex_for_azure_devops);
        idecraft_remote::set_own_proxy_command(settings.ssh.allow_own_proxy_command);
    }
    let backend = Backend::new(config);
    // Language servers, search, Git, terminals, runs, ports, the debug-adapter start and the
    // host's MCP tools (SPEC-0318).
    idecraft_host::bind(
        &backend,
        idecraft_host::HostOptions {
            #[cfg(feature = "mcp")]
            mcp: Some(Arc::new(ServeMcp)),
            ..idecraft_host::HostOptions::default()
        },
    );
    // The project room of this folder, if it has one (SPEC-0323); nothing more listens.
    idecraft_session::room::bind(&backend, idecraft_session::room::RoomConfig::default());
    if o.pair {
        let options = pair::PairOptions {
            addr: o.listen.unwrap_or_else(pair::default_address),
            allow_network: o.allow_network,
            new_device: o.new_device,
            grant: o.grant.clone(),
            key_file: o.key_file,
            qr: o.qr,
            has_folder: o.folder.is_some(),
        };
        return pair::run(backend, &state, options);
    }
    if o.wss {
        return wss::run(
            backend,
            wss::WssOptions {
                addr: o.listen,
                allow_network: o.allow_network,
                cert: o.wss_cert.clone(),
                key: o.wss_key.clone(),
                origins: o.wss_origins.clone(),
                rights: o.wss_rights.clone(),
                has_folder: o.folder.is_some(),
            },
        );
    }
    let watcher = backend.clone();
    let transport = Transport {
        reader: Box::new(std::io::stdin()),
        writer: Box::new(std::io::stdout()),
        // Standard input cannot be interrupted; without any session the process ends instead.
        closer: Arc::new(move || {
            if watcher.sessions() == 0 {
                std::process::exit(0);
            }
        }),
    };
    backend.serve(transport);
    // Standard input ended (bye or a broken channel): wait for the session to end.
    backend.wait_idle(Duration::from_secs(u64::MAX / 4));
    idecraft_languages::pool::shutdown();
    ExitCode::SUCCESS
}

/// `mcp <folder>`: the MCP server over standard input and output.
#[cfg(feature = "mcp")]
pub fn mcp_stdio(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = idecraft_core::Workspace::open(path)?;
    // Untrusted projects are served in safe mode (SPEC-0278 behaviour 11).
    idecraft_core::trust::enforce();
    let runtime = tokio::runtime::Runtime::new()?;
    idecraft_languages::pool::enable();
    let served = runtime.block_on(idecraft_mcp::serve_stdio(workspace));
    idecraft_languages::pool::shutdown();
    served.map_err(|e| e.to_string().into())
}

/// The host's MCP tools for `mcp.*` (SPEC-0318 behaviour 29): IDECraft's MCP server for the host
/// project, with that project's agent rights and safe mode.
#[cfg(feature = "mcp")]
struct ServeMcp;

#[cfg(feature = "mcp")]
fn mcp_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    static RT: std::sync::OnceLock<Option<tokio::runtime::Runtime>> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("serve-mcp")
            .enable_all()
            .build()
            .ok()
    })
    .as_ref()
    .ok_or_else(|| "no runtime for MCP tools".to_owned())
}

#[cfg(feature = "mcp")]
impl idecraft_host::McpBridge for ServeMcp {
    fn tools(&self, root: &std::path::Path) -> Result<serde_json::Value, String> {
        let ws = idecraft_core::Workspace::open(root).map_err(|e| e.to_string())?;
        let server = idecraft_mcp::IdeServer::new(ws);
        mcp_runtime()?.block_on(idecraft_mcp::call::list_tools(server))
    }

    fn call(
        &self,
        root: &std::path::Path,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use idecraft_mcp::call::{Answer, call_once};
        let ws = idecraft_core::Workspace::open(root).map_err(|e| e.to_string())?;
        let server = idecraft_mcp::IdeServer::new(ws);
        match mcp_runtime()?.block_on(call_once(server, tool, arguments))? {
            Answer::Ok(v) => Ok(v),
            Answer::ToolError(m) => Err(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_parsed_and_checked() {
        let args = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        let o = parse(&args(
            "/srv/p --stdio --state-dir /s --run-dir /r --reattach-minutes 5",
        ))
        .unwrap();
        assert_eq!(o.folder, Some(PathBuf::from("/srv/p")));
        assert!(o.stdio);
        assert_eq!(o.reattach_minutes, Some(5));
        assert_eq!(
            parse(&args("--attach abc")).unwrap().attach.as_deref(),
            Some("abc")
        );
        assert!(parse(&args("--reattach-minutes 0")).is_err());
        assert!(parse(&args("--reattach-minutes 61")).is_err());
        assert!(parse(&args("--listen 0.0.0.0")).is_err());
        let p = parse(&args(
            "--pair --listen 127.0.0.1:9 --grant edit --new-device",
        ))
        .unwrap();
        assert!(p.pair && p.new_device);
        assert_eq!(p.listen.unwrap().port(), 9);
        assert!(parse(&args("--grant root")).is_err());
        assert!(parse(&args("a b")).is_err());
        assert!(parse(&args("--attach")).is_err());
        let w = parse(&args(
            "/p --wss --wss-cert /c --wss-key /k --wss-origin https://a.test --wss-rights edit",
        ))
        .unwrap();
        assert!(w.wss);
        assert_eq!(w.wss_origins, ["https://a.test"]);
        assert!(parse(&args("--wss-rights root")).is_err());
    }
}
