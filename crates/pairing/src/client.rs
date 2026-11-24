//! The window's side of the paired channel (`SPEC-0321` behaviours 17, 22): pairing with a short
//! code and six digits, then connections to the pinned key.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use ferris_idea_session::Transport;

use crate::channel::{Channel, Limits, Pipe};
use crate::code::PairLink;
use crate::keys::{KeyError, Store};
use crate::noise::{
    Goal, HandshakeError, Identity, fingerprint, initiate, pairing_key, prologue, sas,
};

/// The host's last pairing message: the device and its rights are durably stored.
pub(crate) const STORED: &str = "stored";
/// The host's last pairing message when storing failed.
pub(crate) const NOT_STORED: &str = "not-stored";

/// A connection or handshake takes at most this long.
pub const HANDSHAKE_LIMIT: Duration = Duration::from_secs(10);

/// Why pairing or connecting failed; every case names what to do.
#[derive(Debug, thiserror::Error)]
pub enum PairError {
    /// The host cannot be reached.
    #[error("{address}: cannot connect: {why}. Is `ferris-idea serve --pair` running there?")]
    Connect {
        /// Address given.
        address: String,
        /// Reason.
        why: String,
    },
    /// The handshake failed.
    #[error("{address}: {why}")]
    Handshake {
        /// Address given.
        address: String,
        /// Reason.
        why: String,
    },
    /// The host's key changed since pairing.
    #[error(
        "{0}: the host refused the handshake: its key does not match the one paired with this \
         window, or this window was revoked there. If the host was reinstalled or revoked this \
         window, forget it and pair again; otherwise treat it as an attack."
    )]
    ChangedKey(String),
    /// The host's key is not the one in the pairing link.
    #[error(
        "{0}: the host's key differs from the key in the pairing link. Nothing was stored; treat \
         this as an attack unless the link is old."
    )]
    WrongKey(String),
    /// Not paired.
    #[error("{0}: not paired yet; pair with this host first")]
    NotPaired(String),
    /// The other side declined.
    #[error("the pairing was declined; nothing was stored")]
    Declined,
    /// The key store refused.
    #[error(transparent)]
    Keys(#[from] KeyError),
}

fn connect(address: &str) -> Result<TcpStream, PairError> {
    let bad = |why: String| PairError::Connect {
        address: address.to_owned(),
        why,
    };
    let addrs: Vec<_> = address
        .to_socket_addrs()
        .map_err(|e| bad(e.to_string()))?
        .collect();
    let mut last = "no address".to_owned();
    for a in addrs {
        match TcpStream::connect_timeout(&a, HANDSHAKE_LIMIT) {
            Ok(s) => {
                s.set_read_timeout(Some(HANDSHAKE_LIMIT))
                    .map_err(|e| bad(e.to_string()))?;
                s.set_write_timeout(Some(HANDSHAKE_LIMIT))
                    .map_err(|e| bad(e.to_string()))?;
                return Ok(s);
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(bad(last))
}

/// A pairing after its handshake: the digits are known, nothing is stored yet.
pub struct PendingPairing {
    address: String,
    channel: Channel,
    host: [u8; 32],
    digits: String,
}

impl PendingPairing {
    /// The six digits to compare with the ones the host shows.
    pub fn digits(&self) -> &str {
        &self.digits
    }

    /// The fingerprint of the host's key.
    pub fn host_fingerprint(&self) -> String {
        fingerprint(&self.host)
    }

    /// The user confirmed: tells the host, waits for its confirmation, then pins its key.
    pub fn confirm(self, store: &Store) -> Result<String, PairError> {
        let address = self.address.clone();
        let host = self.confirm_for_pin()?;
        store.add_host(&address, &host)?;
        Ok(fingerprint(&host))
    }

    /// As [`Self::confirm`] for a caller that keeps pins itself (`SPEC-0326`): the host's key to
    /// pin, returned only after the host confirmed too.
    pub fn confirm_for_pin(mut self) -> Result<[u8; 32], PairError> {
        let failed = |e: io::Error| PairError::Handshake {
            address: self.address.clone(),
            why: format!("the connection ended during pairing: {e}"),
        };
        // The host may have declined and closed already: its `no` is still read when this `ok`
        // can no longer be sent, so a rejection is reported as one (`SPEC-0321` v5 behaviour 17).
        let sent = self.channel.write_line("ok");
        let reply = match (self.channel.read_line(32), sent) {
            (Ok(reply), Err(_)) if reply != "ok" => reply,
            (Ok(reply), Ok(())) => reply,
            (_, Err(e)) | (Err(e), Ok(())) => {
                self.channel.close();
                return Err(failed(e));
            }
        };
        if reply != "ok" {
            self.channel.close();
            return Err(PairError::Declined);
        }
        // The host answers once more after storing this device with its rights; before that a
        // connection would be refused like an unknown key, so nothing is pinned earlier. The
        // channel's heartbeats keep it open while the host's user picks the rights.
        let stored = self.channel.read_line(32);
        self.channel.close();
        let why = match stored.as_deref() {
            Ok(STORED) => return Ok(self.host),
            Ok(NOT_STORED) => {
                "the host could not store this device (its console says why); nothing was \
                 stored here"
            }
            _ => {
                "the host did not confirm that it stored this device; nothing was stored here. \
                 Pair again; if the host runs an older Ferris IDEA, update it first"
            }
        };
        Err(PairError::Handshake {
            address: self.address.clone(),
            why: why.to_owned(),
        })
    }

    /// The user declined: nothing is stored on either side.
    pub fn decline(mut self) {
        let _ = self.channel.write_line("no");
        self.channel.close();
    }
}

/// A name for this device, from the host name.
pub fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "Ferris IDEA window".to_owned())
}

/// Pairs with the listener at `address` using its short `code`, up to the digits.
pub fn start_pairing(
    address: &str,
    code: &str,
    identity: &Identity,
    device: &str,
) -> Result<PendingPairing, PairError> {
    start_pairing_on(connect(address)?, address, code, None, identity, device)
}

/// Pairs using a QR link (`SPEC-0324` behaviour 3): the host's key must equal the link's.
pub fn start_pairing_with_link(
    link: &PairLink,
    identity: &Identity,
    device: &str,
) -> Result<PendingPairing, PairError> {
    start_pairing_on(
        connect(&link.address)?,
        &link.address,
        &link.secret,
        Some(&link.key),
        identity,
        device,
    )
}

/// Ends `pipe` unless the returned flag is set within the handshake limit: a pipe without read
/// timeouts cannot hang a handshake.
fn watch_handshake<P: Pipe>(pipe: &P) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let done = std::sync::Arc::new(AtomicBool::new(false));
    if let Ok(guard) = pipe.duplicate() {
        let flag = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < HANDSHAKE_LIMIT {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !flag.load(Ordering::SeqCst) {
                guard.shut();
            }
        });
    }
    done
}

/// Pairs over any byte pipe, `expected` being the host's key when a link named it.
pub fn start_pairing_on<P: Pipe>(
    mut stream: P,
    address: &str,
    code: &str,
    expected: Option<&[u8; 32]>,
    identity: &Identity,
    device: &str,
) -> Result<PendingPairing, PairError> {
    let finished = watch_handshake(&stream);
    let key = pairing_key(code);
    let done = initiate(
        &mut stream,
        identity,
        Goal::Pair(&key),
        &prologue("window", "serve"),
        device,
    )
    .map_err(|e| handshake(address, e));
    finished.store(true, std::sync::atomic::Ordering::SeqCst);
    let done = done?;
    if expected.is_some_and(|k| *k != done.peer) {
        stream.shut();
        return Err(PairError::WrongKey(address.to_owned()));
    }
    let digits = sas(&done.hash, &done.peer, &identity.public, device);
    let channel = open_channel(address, stream, done.transport, Limits::default())?;
    Ok(PendingPairing {
        address: address.to_owned(),
        channel,
        host: done.peer,
        digits,
    })
}

fn open_channel<P: Pipe>(
    address: &str,
    stream: P,
    transport: snow::TransportState,
    limits: Limits,
) -> Result<Channel, PairError> {
    Channel::new(stream, transport, limits).map_err(|e| PairError::Connect {
        address: address.to_owned(),
        why: e.to_string(),
    })
}

fn handshake(address: &str, e: HandshakeError) -> PairError {
    PairError::Handshake {
        address: address.to_owned(),
        why: e.to_string(),
    }
}

/// Connects to `address` with the key pinned at pairing; another key fails and is named.
pub fn connect_known(
    address: &str,
    store: &Store,
    identity: &Identity,
    device: &str,
) -> Result<Transport, PairError> {
    let pinned = store
        .host_key(address)?
        .ok_or_else(|| PairError::NotPaired(address.to_owned()))?;
    connect_known_on(connect(address)?, address, &pinned, identity, device)
}

/// Connects to `address` over TCP with a key the caller keeps itself (`SPEC-0326`).
pub fn connect_pinned(
    address: &str,
    pinned: &[u8; 32],
    identity: &Identity,
    device: &str,
) -> Result<Transport, PairError> {
    connect_known_on(connect(address)?, address, pinned, identity, device)
}

/// Connects over any byte pipe to the key pinned for `address`.
pub fn connect_known_on<P: Pipe>(
    mut stream: P,
    address: &str,
    pinned: &[u8; 32],
    identity: &Identity,
    device: &str,
) -> Result<Transport, PairError> {
    let finished = watch_handshake(&stream);
    let done = initiate(
        &mut stream,
        identity,
        Goal::Known(pinned),
        &prologue("window", "serve"),
        device,
    )
    .map_err(|e| match e {
        // With a wrong pin the responder cannot read message 1 and closes without a byte; a
        // changed key may also fail the Noise check at message 2.
        HandshakeError::Noise | HandshakeError::Closed => PairError::ChangedKey(address.to_owned()),
        other => handshake(address, other),
    });
    finished.store(true, std::sync::atomic::Ordering::SeqCst);
    let done = done?;
    Ok(open_channel(address, stream, done.transport, Limits::default())?.into_transport())
}
