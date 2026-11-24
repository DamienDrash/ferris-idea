//! An SSH server inside the test process for [`crate::testing::TestHost`] (feature `test-host`,
//! turned on only by the dev-dependencies of the crates whose tests use the test host, never in
//! the product).
//!
//! Why not the system `sshd`: under heavy load the session process of macOS's `sshd` hangs before
//! it runs its command (in a lookup of the user's temporary folder through `opendirectoryd`), so
//! calls on the test host stalled without any fault of Ferris IDEA. This server runs commands itself:
//! it starts the user's shell with the command, as `sshd` does, but without a session process of
//! its own that asks the system for anything.
//!
//! What it offers, and nothing more: public-key login of the current user with exactly the test's
//! client key, a listener on `127.0.0.1` only, the hybrid post-quantum key exchange
//! `mlkem768x25519-sha256` (ADR-0025; the classical variant offers only classical methods, so
//! that tests can check Ferris IDEA refuses it), commands and shells with or without a terminal, and
//! forwarding to ports on the loopback address (`ssh -W`, `ssh -L`). No password or
//! keyboard-interactive login, no agent or X11 forwarding, no remote forwarding, no subsystems,
//! no environment from the client.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::PublicKey;
use russh::server::{Auth, ChannelOpenHandle, Config, Handle, Handler, Msg, Session};
use russh::server::{PublicKeyOffer, VerifiedPublicKey};
use russh::{Channel, ChannelId, ChannelMsg, ChannelOpenFailure, MethodKind, MethodSet, Sig};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// What a server is started with.
pub(crate) struct Setup {
    /// The private host key (OpenSSH format, made by `ssh-keygen`).
    pub host_key: PathBuf,
    /// The public key of the only client allowed in.
    pub client_key: PathBuf,
    /// The only user allowed in.
    pub user: String,
    /// Offer only classical key exchanges (a host Ferris IDEA must refuse).
    pub classical: bool,
    /// Variables every session gets on top of the login environment (`SetEnv` of `sshd`).
    pub env: Vec<(String, String)>,
}

/// A running server; stops with the value (its connections end, programs it started keep running
/// until they read from or write to the closed connection, as with `sshd`).
pub(crate) struct Server {
    runtime: Option<tokio::runtime::Runtime>,
    /// The port on `127.0.0.1` it listens on.
    pub port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// The login of every session: who may log in and the environment the session gets.
struct Login {
    client: PublicKey,
    user: String,
    shell: PathBuf,
    home: PathBuf,
    env: Vec<(String, String)>,
    port: u16,
}

impl Server {
    /// Starts a server on a free port of `127.0.0.1`; `None` when a key cannot be read or no port
    /// can be bound.
    pub(crate) fn start(setup: Setup) -> Option<Self> {
        let host_key = russh::keys::load_secret_key(&setup.host_key, None).ok()?;
        let client = russh::keys::load_public_key(&setup.client_key).ok()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ferris-idea-test-host")
            .enable_all()
            .build()
            .ok()?;
        // Bound here, not in the runtime: a test may start the host from inside its own runtime,
        // where blocking on another one is not allowed.
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        listener.set_nonblocking(true).ok()?;
        let port = listener.local_addr().ok()?.port();
        let config = Arc::new(config(host_key, setup.classical));
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|s| s.is_absolute() && s.exists())
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        let login = Arc::new(Login {
            client,
            user: setup.user,
            shell,
            home,
            env: setup.env,
            port,
        });
        runtime.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            loop {
                let (stream, peer) = match listener.accept().await {
                    Ok(c) => c,
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_nodelay(true);
                let handler = Connection {
                    login: login.clone(),
                    peer,
                };
                let config = config.clone();
                tokio::spawn(async move {
                    if let Ok(session) = russh::server::run_stream(config, stream, handler).await {
                        let _ = session.await;
                    }
                });
            }
        });
        Some(Self {
            runtime: Some(runtime),
            port,
        })
    }
}

/// The server configuration: one host key, public-key login only, the key exchanges of the variant.
fn config(host_key: russh::keys::PrivateKey, classical: bool) -> Config {
    use russh::kex;
    const MARKERS: [kex::Name; 4] = [
        kex::EXTENSION_SUPPORT_AS_CLIENT,
        kex::EXTENSION_SUPPORT_AS_SERVER,
        kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT,
        kex::EXTENSION_OPENSSH_STRICT_KEX_AS_SERVER,
    ];
    let mut kexes = if classical {
        vec![
            kex::CURVE25519,
            kex::CURVE25519_PRE_RFC_8731,
            kex::ECDH_SHA2_NISTP256,
        ]
    } else {
        vec![kex::MLKEM768X25519_SHA256]
    };
    kexes.extend(MARKERS);
    Config {
        methods: MethodSet::from(&[MethodKind::PublicKey][..]),
        keys: vec![host_key],
        // OpenSSH asks with the method "none" first to learn the methods; that is no failed login.
        auth_rejection_time_initial: Some(Duration::ZERO),
        // Shared connections (`ControlPersist`) idle between calls; the client ends them.
        inactivity_timeout: None,
        nodelay: true,
        // russh's other defaults are already narrower than OpenSSH's (no SHA-1 MAC, no CBC); the
        // test's keys are Ed25519, so nothing else is offered for keys either.
        preferred: russh::Preferred {
            kex: kexes.into(),
            key: vec![russh::keys::Algorithm::Ed25519].into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// One client connection.
struct Connection {
    login: Arc<Login>,
    peer: SocketAddr,
}

impl Connection {
    fn check(&self, user: &str, key: &PublicKey) -> Auth {
        if user == self.login.user && key.key_data() == self.login.client.key_data() {
            Auth::Accept
        } else {
            Auth::reject()
        }
    }
}

impl Handler for Connection {
    type Error = russh::Error;

    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        offer: PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(self.check(user, offer.public_key))
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        Ok(self.check(user, key.public_key))
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let handle = session.handle();
        let (login, peer) = (self.login.clone(), self.peer);
        tokio::spawn(run_session(channel, handle, login, peer));
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let stream = match u16::try_from(port_to_connect) {
            Ok(port) => connect_loopback(host_to_connect, port).await,
            Err(_) => None,
        };
        match stream {
            Some(mut stream) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut channel = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut channel, &mut stream).await;
                });
            }
            None => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }

    // The session task reads the requests from its channel; here they are only answered.
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        _variable_name: &str,
        _variable_value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }
}

/// A connection to `host:port` when `host` is a loopback address or a name that resolves only to
/// loopback addresses; nothing else is reachable through the test host.
async fn connect_loopback(host: &str, port: u16) -> Option<tokio::net::TcpStream> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await.ok()?.collect();
    if addrs.is_empty() || !addrs.iter().all(|a| a.ip().is_loopback()) {
        return None;
    }
    for addr in addrs {
        if let Ok(s) = tokio::net::TcpStream::connect(addr).await {
            return Some(s);
        }
    }
    None
}

/// A terminal asked for with `pty-req`.
struct Terminal {
    term: String,
    cols: u16,
    rows: u16,
}

/// One session channel: waits for its command (`exec`) or shell, then runs it.
async fn run_session(
    mut channel: Channel<Msg>,
    handle: Handle,
    login: Arc<Login>,
    peer: SocketAddr,
) {
    let mut terminal = None;
    let command = loop {
        match channel.wait().await {
            Some(ChannelMsg::RequestPty {
                term,
                col_width,
                row_height,
                ..
            }) => {
                terminal = Some(Terminal {
                    term,
                    cols: u16::try_from(col_width).unwrap_or(80),
                    rows: u16::try_from(row_height).unwrap_or(24),
                });
            }
            Some(ChannelMsg::Exec { command, .. }) => {
                break Some(String::from_utf8_lossy(&command).into_owned());
            }
            Some(ChannelMsg::RequestShell { .. }) => break None,
            Some(ChannelMsg::Close) | None => return,
            Some(_) => {}
        }
    };
    let env = environment(&login, peer, terminal.as_ref().map(|t| t.term.as_str()));
    match terminal {
        Some(t) => run_in_terminal(channel, handle, &login, command, env, t).await,
        None => run_with_pipes(channel, handle, &login, command, env).await,
    }
}

/// The environment of a session, as `sshd` builds it: the login variables, the default `PATH`,
/// the connection, the terminal type, then the variables of the test (`SetEnv`).
fn environment(login: &Login, peer: SocketAddr, term: Option<&str>) -> Vec<(String, String)> {
    let path = if cfg!(target_os = "macos") {
        "/usr/bin:/bin:/usr/sbin:/sbin"
    } else {
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    };
    let mut env = vec![
        ("USER".to_owned(), login.user.clone()),
        ("LOGNAME".to_owned(), login.user.clone()),
        ("HOME".to_owned(), login.home.display().to_string()),
        ("SHELL".to_owned(), login.shell.display().to_string()),
        ("PATH".to_owned(), path.to_owned()),
        (
            "SSH_CLIENT".to_owned(),
            format!("{} {} {}", peer.ip(), peer.port(), login.port),
        ),
        (
            "SSH_CONNECTION".to_owned(),
            format!(
                "{} {} {} {}",
                peer.ip(),
                peer.port(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                login.port
            ),
        ),
    ];
    // macOS's `sshd` gives every session the user's temporary folder (the lookup that hangs
    // there under load); it is the one of this process.
    if cfg!(target_os = "macos")
        && let Some(tmp) = std::env::var_os("TMPDIR")
    {
        env.push(("TMPDIR".to_owned(), tmp.to_string_lossy().into_owned()));
    }
    if let Some(term) = term {
        env.push(("TERM".to_owned(), term.to_owned()));
    }
    for (k, v) in &login.env {
        env.retain(|(name, _)| name != k);
        env.push((k.clone(), v.clone()));
    }
    env
}

/// `-sh` for a login shell started for a shell request, as `sshd` names it.
fn login_name(shell: &Path) -> String {
    format!(
        "-{}",
        shell
            .file_name()
            .map_or_else(|| "sh".into(), |n| n.to_string_lossy())
    )
}

/// Runs `command` (or a login shell) with pipes for standard input, output and error.
async fn run_with_pipes(
    mut channel: Channel<Msg>,
    handle: Handle,
    login: &Login,
    command: Option<String>,
    env: Vec<(String, String)>,
) {
    let mut cmd = tokio::process::Command::new(&login.shell);
    match &command {
        Some(c) => {
            cmd.arg("-c").arg(c);
        }
        None => {
            cmd.arg0(login_name(&login.shell));
        }
    }
    cmd.env_clear()
        .envs(env)
        .current_dir(&login.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group, as a session of `sshd` gets its own session.
        .process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            let _ = channel.exit_status(127).await;
            let _ = channel.eof().await;
            let _ = channel.close().await;
            return;
        }
    };
    // Input goes through a queue of its own, so that a program that does not read never holds up
    // the connection's other channels.
    let (input, mut queue) = mpsc::unbounded_channel::<Option<Vec<u8>>>();
    let stdin = child.stdin.take();
    tokio::spawn(async move {
        let Some(mut stdin) = stdin else { return };
        while let Some(Some(data)) = queue.recv().await {
            if stdin.write_all(&data).await.is_err() || stdin.flush().await.is_err() {
                return;
            }
        }
    });
    let out = child
        .stdout
        .take()
        .map(|o| tokio::spawn(copy_out(o, channel.make_writer())));
    let err = child
        .stderr
        .take()
        .map(|e| tokio::spawn(copy_out(e, channel.make_writer_ext(Some(1)))));
    let id = channel.id();
    let mut finished = Box::pin(async move {
        let status = child.wait().await;
        if let Some(t) = out {
            let _ = t.await;
        }
        if let Some(t) = err {
            let _ = t.await;
        }
        status
    });
    loop {
        tokio::select! {
            status = &mut finished => {
                let _ = channel.eof().await;
                if let Ok(status) = status {
                    use std::os::unix::process::ExitStatusExt;
                    match (status.code(), status.signal()) {
                        (Some(code), _) => {
                            let _ = channel.exit_status(u32::try_from(code).unwrap_or(255)).await;
                        }
                        (None, Some(sig)) => {
                            let _ = handle
                                .exit_signal_request(id, signal_name(sig), false, String::new(), String::new())
                                .await;
                        }
                        (None, None) => {}
                    }
                }
                let _ = channel.close().await;
                return;
            }
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    let _ = input.send(Some(data.to_vec()));
                }
                Some(ChannelMsg::Eof) => {
                    let _ = input.send(None);
                }
                // The client is gone: the program keeps running until it uses the closed pipes.
                Some(ChannelMsg::Close) | None => return,
                Some(_) => {}
            },
        }
    }
}

/// Copies a program's output into the channel until either ends.
async fn copy_out(mut from: impl AsyncRead + Unpin, mut to: impl AsyncWrite + Unpin) {
    let _ = tokio::io::copy(&mut from, &mut to).await;
    let _ = to.flush().await;
}

/// The SSH name of signal `n`.
fn signal_name(n: i32) -> Sig {
    match n {
        1 => Sig::HUP,
        2 => Sig::INT,
        3 => Sig::QUIT,
        4 => Sig::ILL,
        6 => Sig::ABRT,
        8 => Sig::FPE,
        9 => Sig::KILL,
        11 => Sig::SEGV,
        13 => Sig::PIPE,
        14 => Sig::ALRM,
        15 => Sig::TERM,
        _ => Sig::Custom(format!("SIG{n}")),
    }
}

/// What the terminal's reader thread passes on.
enum Output {
    Data(Vec<u8>),
    End,
}

/// Runs `command` (or a login shell) in a terminal of its own.
async fn run_in_terminal(
    mut channel: Channel<Msg>,
    _handle: Handle,
    login: &Login,
    command: Option<String>,
    env: Vec<(String, String)>,
    terminal: Terminal,
) {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    let size = |cols: u16, rows: u16| PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let fail = |channel: Channel<Msg>| async move {
        let _ = channel.exit_status(127).await;
        let _ = channel.eof().await;
        let _ = channel.close().await;
    };
    let Ok(pair) = native_pty_system().openpty(size(terminal.cols, terminal.rows)) else {
        return fail(channel).await;
    };
    let mut cmd = CommandBuilder::new(&login.shell);
    match &command {
        Some(c) => cmd.args(["-c", c.as_str()]),
        None => cmd.arg("-l"),
    }
    cmd.env_clear();
    for (k, v) in &env {
        cmd.env(k, v);
    }
    cmd.cwd(&login.home);
    let Ok(mut child) = pair.slave.spawn_command(cmd) else {
        return fail(channel).await;
    };
    drop(pair.slave);
    let master = pair.master;
    let (Ok(mut reader), Ok(mut writer)) = (master.try_clone_reader(), master.take_writer()) else {
        let _ = child.kill();
        return fail(channel).await;
    };
    let mut killer = child.clone_killer();
    let (out_tx, mut out_rx) = mpsc::channel::<Output>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx
                        .blocking_send(Output::Data(buf[..n].to_vec()))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
        let _ = out_tx.blocking_send(Output::End);
    });
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(data) = in_rx.recv() {
            if std::io::Write::write_all(&mut writer, &data).is_err() {
                return;
            }
            let _ = std::io::Write::flush(&mut writer);
        }
    });
    let mut waiter = tokio::task::spawn_blocking(move || child.wait());
    let mut status: Option<u32> = None;
    let mut output_open = true;
    loop {
        // After the program ended, its output is passed on until the terminal has nothing more
        // (a program it left behind may keep the terminal open; a second of quiet ends it).
        if status.is_some() && !output_open {
            break;
        }
        let quiet = async {
            if status.is_some() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            done = &mut waiter, if status.is_none() => {
                status = Some(match done {
                    Ok(Ok(s)) => s.exit_code(),
                    _ => 255,
                });
            }
            out = out_rx.recv(), if output_open => match out {
                Some(Output::Data(data)) => {
                    if channel.data_bytes(data).await.is_err() {
                        let _ = killer.kill();
                        return;
                    }
                }
                Some(Output::End) | None => output_open = false,
            },
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    let _ = in_tx.send(data.to_vec());
                }
                Some(ChannelMsg::WindowChange { col_width, row_height, .. }) => {
                    let _ = master.resize(size(
                        u16::try_from(col_width).unwrap_or(80),
                        u16::try_from(row_height).unwrap_or(24),
                    ));
                }
                // The client is gone: the terminal closes, its programs get a hangup.
                Some(ChannelMsg::Close) | None => {
                    let _ = killer.kill();
                    return;
                }
                Some(_) => {}
            },
            () = quiet => break,
        }
    }
    let _ = channel.eof().await;
    let _ = channel.exit_status(status.unwrap_or(255)).await;
    let _ = channel.close().await;
}
