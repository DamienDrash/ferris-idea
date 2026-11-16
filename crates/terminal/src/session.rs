//! A program running in a pseudo terminal (`SPEC-0038` behaviour 1).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::screen::Terminal;

/// What went wrong with a terminal session.
#[derive(Debug, thiserror::Error)]
#[error("terminal: {0}")]
pub struct SessionError(String);

fn err<E: std::fmt::Display>(e: E) -> SessionError {
    SessionError(e.to_string())
}

/// A pseudo terminal elsewhere — the shell of a remote project's host behind its session
/// (`SPEC-0318` v2 window detail 2): the screen model is this machine's, the program is not.
pub trait PtyLink: Send + Sync {
    /// The next output, waiting up to `wait`.
    fn read(&self, wait: Duration) -> LinkRead;
    /// Sends keystrokes (never waits for the network).
    fn write(&self, bytes: &[u8]);
    /// Changes the size of the far pseudo terminal.
    fn resize(&self, rows: u16, cols: u16);
    /// Ends the program.
    fn close(&self);
}

/// What a [`PtyLink`] read gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkRead {
    /// Output, in order.
    Data(Vec<u8>),
    /// Output was lost: the screen starts again with what follows.
    Redraw,
    /// The program ended (exit code if known) with a note for the screen.
    Ended(Option<u32>, String),
    /// Nothing within the wait.
    Idle,
}

/// The end of a linked program: its exit code and the note.
type LinkEnd = Arc<Mutex<Option<(Option<u32>, String)>>>;

enum Backing {
    Pty {
        master: Box<dyn MasterPty + Send>,
        child: Box<dyn Child + Send + Sync>,
    },
    Link {
        link: Arc<dyn PtyLink>,
        end: LinkEnd,
    },
}

/// Writes to a [`PtyLink`].
struct LinkWriter(Arc<dyn PtyLink>);

impl Write for LinkWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A running program with its screen model.
pub struct Session {
    backing: Backing,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    terminal: Arc<Mutex<Terminal>>,
    generation: Arc<AtomicU64>,
}

/// How a session starts beyond its command and size.
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// Saved lines shown in front of the program's output (`SPEC-0205` behaviour 6).
    pub history: Vec<String>,
    /// Add the shell integration when the program is zsh or bash (`SPEC-0206` behaviour 5),
    /// with its files in this folder.
    pub integration: Option<std::path::PathBuf>,
    /// Variables set for the program, after those of the integration (so they win).
    pub env: Vec<(String, String)>,
}

impl Session {
    /// Starts `command` (or the user's shell when empty) in `cwd` with a `rows` × `cols` terminal.
    pub fn spawn(
        command: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<Self, SessionError> {
        Self::spawn_with(command, cwd, rows, cols, &SpawnOptions::default())
    }

    /// [`Session::spawn`] with saved history and shell integration.
    pub fn spawn_with(
        command: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
        options: &SpawnOptions,
    ) -> Result<Self, SessionError> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(err)?;
        let shell;
        let (program, args) = match command.split_first() {
            Some((program, args)) => (program, args),
            None => {
                shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
                (&shell, &[][..])
            }
        };
        let mut cmd = CommandBuilder::new(program);
        // A shell started without arguments gets the integration (not an SSH login or `-c`).
        if args.is_empty()
            && let Some(dir) = &options.integration
            && let Some(integration) = crate::integration::prepare(program, dir)
        {
            cmd.args(&integration.args);
            for (k, v) in &integration.env {
                cmd.env(k, v);
            }
        }
        cmd.args(args);
        for (k, v) in &options.env {
            cmd.env(k, v);
        }
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).map_err(err)?;
        // The child holds its own copy of the slave side.
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(err)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(err)?));
        let mut screen = Terminal::new(rows.into(), cols.into());
        screen.restore(&options.history);
        let terminal = Arc::new(Mutex::new(screen));
        let generation = Arc::new(AtomicU64::new(0));
        let (term, answer, changed) = (terminal.clone(), writer.clone(), generation.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let responses = {
                    let Ok(mut t) = term.lock() else { break };
                    t.feed(&buf[..n]);
                    t.screen.take_responses()
                };
                changed.fetch_add(1, Ordering::Release);
                if !responses.is_empty()
                    && let Ok(mut w) = answer.lock()
                {
                    let _ = w.write_all(&responses);
                }
            }
        });
        Ok(Self {
            backing: Backing::Pty {
                master: pair.master,
                child,
            },
            writer,
            terminal,
            generation,
        })
    }

    /// A session whose program runs behind `link` (a host terminal of a remote project), with a
    /// `rows` × `cols` screen and saved `history` in front.
    pub fn over_link(link: Arc<dyn PtyLink>, rows: u16, cols: u16, history: &[String]) -> Self {
        let mut screen = Terminal::new(rows.into(), cols.into());
        screen.restore(history);
        let terminal = Arc::new(Mutex::new(screen));
        let generation = Arc::new(AtomicU64::new(0));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(LinkWriter(Arc::clone(&link)))));
        let end: LinkEnd = Arc::new(Mutex::new(None));
        let (term, changed, reader, ended) = (
            terminal.clone(),
            generation.clone(),
            Arc::clone(&link),
            Arc::clone(&end),
        );
        std::thread::spawn(move || {
            loop {
                match reader.read(Duration::from_millis(100)) {
                    LinkRead::Idle => continue,
                    LinkRead::Data(bytes) => {
                        let responses = {
                            let Ok(mut t) = term.lock() else { break };
                            t.feed(&bytes);
                            t.screen.take_responses()
                        };
                        changed.fetch_add(1, Ordering::Release);
                        if !responses.is_empty() {
                            reader.write(&responses);
                        }
                    }
                    LinkRead::Redraw => {
                        let Ok(mut t) = term.lock() else { break };
                        let (rows, cols) = t.screen.size();
                        *t = Terminal::new(rows, cols);
                        changed.fetch_add(1, Ordering::Release);
                    }
                    LinkRead::Ended(code, note) => {
                        if let Ok(mut e) = ended.lock() {
                            *e = Some((code, note));
                        }
                        changed.fetch_add(1, Ordering::Release);
                        break;
                    }
                }
            }
        });
        Self {
            backing: Backing::Link { link, end },
            writer,
            terminal,
            generation,
        }
    }

    /// How a linked program ended: its note (`None` while it runs, and for local programs).
    pub fn end_note(&self) -> Option<String> {
        match &self.backing {
            Backing::Link { end, .. } => end.lock().ok()?.as_ref().map(|(_, n)| n.clone()),
            Backing::Pty { .. } => None,
        }
    }

    /// Whether the program runs behind a [`PtyLink`].
    pub fn is_linked(&self) -> bool {
        matches!(self.backing, Backing::Link { .. })
    }

    /// The screen model, shared with the reader thread, for the registry of tabs that readers on
    /// other threads may read (`SPEC-0313` behaviour 9).
    pub fn terminal_handle(&self) -> &Arc<Mutex<Terminal>> {
        &self.terminal
    }

    /// Counts output chunks read so far; a view redraws when it changes.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Sends input bytes (keys) to the program.
    pub fn write(&self, bytes: &[u8]) -> Result<(), SessionError> {
        let mut w = self.writer.lock().map_err(err)?;
        w.write_all(bytes).map_err(err)?;
        w.flush().map_err(err)
    }

    /// Changes the terminal size; the program receives `SIGWINCH`.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), SessionError> {
        self.terminal
            .lock()
            .map_err(err)?
            .screen
            .resize(rows.into(), cols.into());
        match &self.backing {
            Backing::Pty { master, .. } => master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(err),
            Backing::Link { link, .. } => {
                link.resize(rows, cols);
                Ok(())
            }
        }
    }

    /// Runs `f` on the current terminal state.
    pub fn with_terminal<T>(&self, f: impl FnOnce(&Terminal) -> T) -> T {
        let t = self.terminal.lock().unwrap_or_else(|e| e.into_inner());
        f(&t)
    }

    /// The shell's current folder (`SPEC-0161` behaviour 1): the one it reported with OSC 7, else
    /// the folder of its process as the system reports it.
    pub fn current_dir(&self) -> Option<std::path::PathBuf> {
        if let Some(d) = self.with_terminal(|t| t.screen.reported_dir().map(str::to_owned)) {
            return Some(d.into());
        }
        match &self.backing {
            Backing::Pty { child, .. } => process_dir(child.process_id()?),
            // The folder of a host's shell is not a folder of this machine.
            Backing::Link { .. } => None,
        }
    }

    /// The visible text.
    pub fn text(&self) -> String {
        self.with_terminal(|t| t.screen.text())
    }

    /// Waits until `done` holds for the visible text, up to `timeout`; returns whether it did.
    pub fn wait_for(&self, timeout: Duration, done: impl Fn(&str) -> bool) -> bool {
        let start = Instant::now();
        loop {
            if done(&self.text()) {
                return true;
            }
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Exit code once the program has ended, waiting up to `timeout`.
    pub fn wait_exit(&mut self, timeout: Duration) -> Result<Option<u32>, SessionError> {
        let start = Instant::now();
        loop {
            if let Some(code) = self.exited() {
                return Ok(Some(code));
            }
            if start.elapsed() >= timeout {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Ends the program.
    pub fn kill(&mut self) -> Result<(), SessionError> {
        match &mut self.backing {
            Backing::Pty { child, .. } => child.kill().map_err(err),
            Backing::Link { link, .. } => {
                link.close();
                Ok(())
            }
        }
    }

    /// Ends the program and everything it started: the PTY makes it lead its own session and
    /// process group (`SPEC-0207` behaviour 3).
    pub fn kill_group(&mut self) -> Result<(), SessionError> {
        #[cfg(unix)]
        if let Backing::Pty { child, .. } = &self.backing
            && let Some(pid) = child.process_id()
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        self.kill()
    }

    /// Exit code once the program has ended, without waiting (`SPEC-0205` behaviour 5).
    pub fn exited(&mut self) -> Option<u32> {
        match &mut self.backing {
            Backing::Pty { child, .. } => child
                .try_wait()
                .ok()
                .flatten()
                .map(|status| status.exit_code()),
            Backing::Link { end, .. } => end.lock().ok()?.as_ref().and_then(|(c, _)| *c),
        }
    }

    /// History and screen as text, without trailing empty lines (`SPEC-0207` behaviour 3).
    pub fn history_text(&self) -> String {
        self.with_terminal(|t| t.screen.history_lines(usize::MAX).join("\n"))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        match &mut self.backing {
            Backing::Pty { child, .. } => {
                if let Ok(None) = child.try_wait() {
                    let _ = child.kill();
                }
            }
            Backing::Link { link, .. } => link.close(),
        }
    }
}

/// The current folder of process `pid`: `/proc` on Linux, `lsof` on macOS.
pub fn process_dir(pid: u32) -> Option<std::path::PathBuf> {
    if let Ok(p) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
        return Some(p);
    }
    let out = std::process::Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('n'))
        .map(std::path::PathBuf::from)
}
