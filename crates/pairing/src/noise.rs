//! The one Noise module of `ADR-0025` §1b, §1c: `XXpsk3` for the first pairing and `XKpsk3` for
//! every later connection, always hybrid with ML-KEM-1024. There is no negotiation and no code
//! path without the KEM: `psk3` is set only from the KEM secret (`SPEC-0321` behaviours 14, 15).

use std::io::{Read, Write};

use hkdf::Hkdf;
use libcrux_ml_kem::mlkem1024 as kem;
use sha2::{Digest, Sha256};
use snow::{Builder, HandshakeState, TransportState};
use zeroize::Zeroizing;

/// `Noise_XXpsk3` with the suite of `ADR-0025` §1b (first pairing).
pub const PAIR_PATTERN: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
/// `Noise_XKpsk3` with the suite of `ADR-0025` §1b (every later connection).
pub const KNOWN_PATTERN: &str = "Noise_XKpsk3_25519_ChaChaPoly_BLAKE2s";
/// The session protocol major in the prologue.
pub const PROTOCOL_MAJOR: u32 = 1;

/// Length of an ML-KEM-1024 encapsulation key and of its ciphertext.
pub const KEM_LEN: usize = 1568;
/// Length of message 1 of both patterns: an ephemeral key and the encapsulation key under an AEAD
/// tag (in a psk pattern the `e` token mixes a key, so `XXpsk3` encrypts it too).
pub const MSG1_LEN: usize = 32 + KEM_LEN + 16;

const MAX_MESSAGE: usize = 65_535;
const MAX_FINAL_PAYLOAD: usize = 512;

/// The prologue for `initiator` connecting to `responder` (`window`, `serve`, `device`).
pub fn prologue(initiator: &str, responder: &str) -> String {
    format!("idecraft-session/{PROTOCOL_MAJOR};pq=ML-KEM-1024;{initiator}>{responder}")
}

/// Why a handshake failed. Never names key material.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    /// The peer closed the connection during the handshake, without a reason (a refusal sends
    /// no byte).
    #[error("the other side closed the connection during the handshake")]
    Closed,
    /// The connection ended or stalled.
    #[error("the connection ended during the handshake: {0}")]
    Io(String),
    /// A message that is not part of the two hybrid handshakes.
    #[error("not an IDECraft handshake (message 1 has {0} bytes)")]
    NotAHandshake(usize),
    /// The KEM part is missing or invalid.
    #[error("the post-quantum part of the handshake is missing or invalid")]
    Kem,
    /// A Noise check failed (wrong key, wrong code, tampering, wrong prologue).
    #[error("the handshake failed: the keys or the pairing code do not match")]
    Noise,
    /// The peer's static key is not paired.
    #[error("this device is not paired with the host")]
    NotPaired,
    /// A message of the wrong shape.
    #[error("malformed handshake message")]
    Malformed,
}

impl HandshakeError {
    fn noise(_: snow::Error) -> Self {
        Self::Noise
    }
}

/// A long-term static X25519 key pair.
#[derive(Clone)]
pub struct Identity {
    /// Public key.
    pub public: [u8; 32],
    /// Private key.
    pub private: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identity({})", fingerprint(&self.public))
    }
}

impl Identity {
    /// A new key pair from the operating system's random source.
    pub fn generate() -> Result<Self, HandshakeError> {
        let params = PAIR_PATTERN.parse().map_err(HandshakeError::noise)?;
        let kp = Builder::new(params)
            .generate_keypair()
            .map_err(HandshakeError::noise)?;
        Self::from_bytes(&kp.public, &kp.private).ok_or(HandshakeError::Malformed)
    }

    /// From raw keys of 32 bytes.
    pub fn from_bytes(public: &[u8], private: &[u8]) -> Option<Self> {
        let public: [u8; 32] = public.try_into().ok()?;
        let private: [u8; 32] = private.try_into().ok()?;
        Some(Self {
            public,
            private: Zeroizing::new(private),
        })
    }
}

/// The fingerprint of a public key: 16 hex digits of its SHA-256.
pub fn fingerprint(public: &[u8; 32]) -> String {
    Sha256::digest(public)[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The 32-byte pairing key derived from the short code (label of `ADR-0025` §2).
pub fn pairing_key(code: &str) -> Zeroizing<[u8; 32]> {
    let normal = crate::code::normalize(code);
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(None, normal.as_bytes())
        .expand(b"idecraft-pair-v2", out.as_mut())
        .unwrap_or_default();
    out
}

fn psk3(
    h2: &[u8],
    ss: &[u8],
    pair: Option<&[u8; 32]>,
    pattern: &str,
    prologue: &str,
) -> Zeroizing<[u8; 32]> {
    let mut ikm = Zeroizing::new(Vec::with_capacity(64));
    ikm.extend_from_slice(ss);
    if let Some(p) = pair {
        ikm.extend_from_slice(p);
    }
    let info = format!("idecraft-pq-v1{pattern}{prologue}");
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(h2), &ikm)
        .expand(info.as_bytes(), out.as_mut())
        .unwrap_or_default();
    out
}

/// The six digits both sides compare after a pairing handshake (`ADR-0025` §2).
pub fn sas(hash: &[u8], responder: &[u8; 32], initiator: &[u8; 32], device: &str) -> String {
    let mut info = b"idecraft-sas-v1".to_vec();
    info.extend_from_slice(fingerprint(responder).as_bytes());
    info.extend_from_slice(fingerprint(initiator).as_bytes());
    info.extend_from_slice(device.as_bytes());
    let mut out = [0u8; 4];
    Hkdf::<Sha256>::new(None, hash)
        .expand(&info, &mut out)
        .unwrap_or_default();
    format!("{:06}", u32::from_be_bytes(out) % 1_000_000)
}

fn random<const N: usize>() -> Result<[u8; N], HandshakeError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| HandshakeError::Io(e.to_string()))?;
    Ok(b)
}

/// Fixed inputs for the tests of SEC-15 (b). Absent in the product.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct Fixed {
    pub ephemeral: Option<[u8; 32]>,
    pub kem_seed: Option<[u8; 64]>,
    pub encaps: Option<[u8; 32]>,
    pub ss_override: Option<[u8; 32]>,
}

#[cfg(not(test))]
#[derive(Clone, Default)]
pub(crate) struct Fixed {}

fn builder<'a>(
    pattern: &str,
    prologue: &'a str,
    key: &'a [u8; 32],
    _fixed: &'a Fixed,
) -> Result<Builder<'a>, HandshakeError> {
    let params = pattern.parse().map_err(HandshakeError::noise)?;
    #[allow(unused_mut)]
    let mut b = Builder::new(params)
        .local_private_key(key)
        .map_err(HandshakeError::noise)?
        .prologue(prologue.as_bytes())
        .map_err(HandshakeError::noise)?;
    #[cfg(test)]
    if let Some(e) = &_fixed.ephemeral {
        b = b.fixed_ephemeral_key_for_testing_only(e);
    }
    Ok(b)
}

fn kem_seed(_fixed: &Fixed) -> Result<[u8; 64], HandshakeError> {
    #[cfg(test)]
    if let Some(s) = _fixed.kem_seed {
        return Ok(s);
    }
    random()
}

fn encaps_rand(_fixed: &Fixed) -> Result<[u8; 32], HandshakeError> {
    #[cfg(test)]
    if let Some(s) = _fixed.encaps {
        return Ok(s);
    }
    random()
}

/// Reads one length-prefixed message of at most `MAX_MESSAGE` bytes.
pub(crate) fn read_frame(r: &mut impl Read) -> Result<Vec<u8>, HandshakeError> {
    let failed = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            HandshakeError::Closed
        } else {
            HandshakeError::Io(e.to_string())
        }
    };
    let mut len = [0u8; 2];
    r.read_exact(&mut len).map_err(failed)?;
    let mut buf = vec![0u8; usize::from(u16::from_be_bytes(len))];
    r.read_exact(&mut buf).map_err(failed)?;
    Ok(buf)
}

/// Writes one length-prefixed message in one piece.
pub(crate) fn write_frame(w: &mut impl Write, data: &[u8]) -> Result<(), HandshakeError> {
    let len = u16::try_from(data.len()).map_err(|_| HandshakeError::Malformed)?;
    let mut out = Vec::with_capacity(data.len() + 2);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(data);
    w.write_all(&out)
        .and_then(|()| w.flush())
        .map_err(|e| HandshakeError::Io(e.to_string()))
}

/// What a finished handshake gives.
pub struct Finished {
    /// The encrypted channel state.
    pub transport: TransportState,
    /// The final handshake hash (the SAS input).
    pub hash: Vec<u8>,
    /// The other side's static key.
    pub peer: [u8; 32],
    /// The initiator's name (from message 3).
    pub device: String,
}

impl std::fmt::Debug for Finished {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Finished({})", fingerprint(&self.peer))
    }
}

/// What the initiator knows beforehand.
pub enum Goal<'a> {
    /// Pairing with a short code.
    Pair(&'a [u8; 32]),
    /// A later connection to a pinned key.
    Known(&'a [u8; 32]),
}

/// Runs the initiator's side over `stream`.
pub fn initiate<S: Read + Write>(
    stream: &mut S,
    identity: &Identity,
    goal: Goal<'_>,
    prologue: &str,
    device: &str,
) -> Result<Finished, HandshakeError> {
    initiate_with(stream, identity, goal, prologue, device, &Fixed::default())
}

pub(crate) fn initiate_with<S: Read + Write>(
    stream: &mut S,
    identity: &Identity,
    goal: Goal<'_>,
    prologue: &str,
    device: &str,
    fixed: &Fixed,
) -> Result<Finished, HandshakeError> {
    let (pattern, pair) = match &goal {
        Goal::Pair(p) => (PAIR_PATTERN, Some(*p)),
        Goal::Known(_) => (KNOWN_PATTERN, None),
    };
    let b = builder(pattern, prologue, &identity.private, fixed)?;
    let b = match &goal {
        Goal::Known(key) => b
            .remote_public_key(key.as_slice())
            .map_err(HandshakeError::noise)?,
        Goal::Pair(_) => b,
    };
    let mut hs = b.build_initiator().map_err(HandshakeError::noise)?;
    let pair_kem = kem::generate_key_pair(kem_seed(fixed)?);
    let mut buf = Zeroizing::new(vec![0u8; MAX_MESSAGE]);
    let n = hs
        .write_message(pair_kem.pk(), &mut buf)
        .map_err(HandshakeError::noise)?;
    write_frame(stream, &buf[..n])?;
    let m2 = read_frame(stream)?;
    let mut ct = Zeroizing::new(vec![0u8; MAX_MESSAGE]);
    let n = hs
        .read_message(&m2, &mut ct)
        .map_err(HandshakeError::noise)?;
    if n != KEM_LEN {
        return Err(HandshakeError::Kem);
    }
    let bytes: [u8; KEM_LEN] = ct[..n].try_into().map_err(|_| HandshakeError::Kem)?;
    let ct = kem::MlKem1024Ciphertext::from(bytes);
    let ss = Zeroizing::new(kem::decapsulate(pair_kem.private_key(), &ct));
    #[cfg(test)]
    let ss = fixed.ss_override.map_or(ss, Zeroizing::new);
    let key = psk3(
        hs.get_handshake_hash(),
        ss.as_ref(),
        pair,
        pattern,
        prologue,
    );
    hs.set_psk(3, key.as_ref()).map_err(HandshakeError::noise)?;
    let hello = serde_json::json!({ "v": 1, "name": device }).to_string();
    if hello.len() > MAX_FINAL_PAYLOAD {
        return Err(HandshakeError::Malformed);
    }
    let n = hs
        .write_message(hello.as_bytes(), &mut buf)
        .map_err(HandshakeError::noise)?;
    write_frame(stream, &buf[..n])?;
    finish(hs, device.to_owned())
}

fn finish(hs: HandshakeState, device: String) -> Result<Finished, HandshakeError> {
    let peer: [u8; 32] = hs
        .get_remote_static()
        .and_then(|s| s.try_into().ok())
        .ok_or(HandshakeError::Noise)?;
    let hash = hs.get_handshake_hash().to_vec();
    let transport = hs.into_transport_mode().map_err(HandshakeError::noise)?;
    Ok(Finished {
        transport,
        hash,
        peer,
        device,
    })
}

/// What the responder accepts.
pub struct Accept<'a> {
    /// The pairing key while a pairing window is open.
    pub pairing: Option<&'a [u8; 32]>,
    /// Whether a static key may complete `XKpsk3` (checked after message 3).
    pub pinned: &'a dyn Fn(&[u8; 32]) -> bool,
}

/// Which handshake ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `XXpsk3`.
    Pair,
    /// `XKpsk3`.
    Known,
}

/// Runs the responder's side over `stream`.
pub fn respond<S: Read + Write>(
    stream: &mut S,
    identity: &Identity,
    accept: &Accept<'_>,
    prologue: &str,
) -> Result<(Finished, Kind), HandshakeError> {
    respond_with(stream, identity, accept, prologue, &Fixed::default())
}

pub(crate) fn respond_with<S: Read + Write>(
    stream: &mut S,
    identity: &Identity,
    accept: &Accept<'_>,
    prologue: &str,
    fixed: &Fixed,
) -> Result<(Finished, Kind), HandshakeError> {
    let m1 = read_frame(stream)?;
    if m1.len() != MSG1_LEN {
        return Err(HandshakeError::NotAHandshake(m1.len()));
    }
    let mut buf = Zeroizing::new(vec![0u8; MAX_MESSAGE]);
    // Nothing outside Noise says which pattern this is: `XKpsk3` decrypts only under this host's
    // static key; anything else is `XXpsk3`, which exists only while a pairing window is open.
    let mut hs = builder(KNOWN_PATTERN, prologue, &identity.private, fixed)?
        .build_responder()
        .map_err(HandshakeError::noise)?;
    let (kind, pattern, pair, n) = match hs.read_message(&m1, &mut buf) {
        Ok(n) => (Kind::Known, KNOWN_PATTERN, None, n),
        Err(_) => {
            let key = accept.pairing.ok_or(HandshakeError::Noise)?;
            hs = builder(PAIR_PATTERN, prologue, &identity.private, fixed)?
                .build_responder()
                .map_err(HandshakeError::noise)?;
            let n = hs
                .read_message(&m1, &mut buf)
                .map_err(HandshakeError::noise)?;
            (Kind::Pair, PAIR_PATTERN, Some(key), n)
        }
    };
    if n != KEM_LEN {
        return Err(HandshakeError::Kem);
    }
    let bytes: [u8; KEM_LEN] = buf[..n].try_into().map_err(|_| HandshakeError::Kem)?;
    let ek = kem::MlKem1024PublicKey::from(bytes);
    if !kem::validate_public_key(&ek) {
        return Err(HandshakeError::Kem);
    }
    let (ct, ss) = kem::encapsulate(&ek, encaps_rand(fixed)?);
    let ss = Zeroizing::new(ss);
    #[cfg(test)]
    let ss = fixed.ss_override.map_or(ss, Zeroizing::new);
    let n = hs
        .write_message(ct.as_slice(), &mut buf)
        .map_err(HandshakeError::noise)?;
    write_frame(stream, &buf[..n])?;
    let key = psk3(
        hs.get_handshake_hash(),
        ss.as_ref(),
        pair,
        pattern,
        prologue,
    );
    hs.set_psk(3, key.as_ref()).map_err(HandshakeError::noise)?;
    let m3 = read_frame(stream)?;
    let n = hs
        .read_message(&m3, &mut buf)
        .map_err(HandshakeError::noise)?;
    let hello: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|_| HandshakeError::Malformed)?;
    if hello["v"] != 1 {
        return Err(HandshakeError::Malformed);
    }
    let device = hello["name"].as_str().unwrap_or("device").to_owned();
    let finished = finish(hs, device)?;
    if kind == Kind::Known && !(accept.pinned)(&finished.peer) {
        return Err(HandshakeError::NotPaired);
    }
    Ok((finished, kind))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc;

    /// One end of an in-memory byte pipe.
    pub(crate) struct Pipe {
        tx: mpsc::Sender<Vec<u8>>,
        rx: mpsc::Receiver<Vec<u8>>,
        buf: VecDeque<u8>,
    }

    pub(crate) fn pipe() -> (Pipe, Pipe) {
        let (a_tx, a_rx) = mpsc::channel();
        let (b_tx, b_rx) = mpsc::channel();
        (
            Pipe {
                tx: a_tx,
                rx: b_rx,
                buf: VecDeque::new(),
            },
            Pipe {
                tx: b_tx,
                rx: a_rx,
                buf: VecDeque::new(),
            },
        )
    }

    impl Read for Pipe {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            while self.buf.is_empty() {
                match self.rx.recv() {
                    Ok(b) => self.buf.extend(b),
                    Err(_) => return Ok(0),
                }
            }
            let n = out.len().min(self.buf.len());
            for slot in &mut out[..n] {
                *slot = self.buf.pop_front().unwrap_or_default();
            }
            Ok(n)
        }
    }

    impl Write for Pipe {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.tx
                .send(b.to_vec())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn id() -> Identity {
        Identity::generate().unwrap()
    }

    fn roles() -> String {
        prologue("window", "serve")
    }

    type Pair = (
        Result<Finished, HandshakeError>,
        Result<(Finished, Kind), HandshakeError>,
    );

    fn run(
        goal_pair: Option<[u8; 32]>,
        accept_pair: Option<[u8; 32]>,
        client: &Identity,
        server: &Identity,
        pin: [u8; 32],
        fixed_c: Fixed,
        fixed_s: Fixed,
    ) -> Pair {
        let (mut a, mut b) = pipe();
        let (client, server) = (client.clone(), server.clone());
        let responder = std::thread::spawn(move || {
            let accept = Accept {
                pairing: accept_pair.as_ref(),
                pinned: &|_| true,
            };
            let r = respond_with(&mut b, &server, &accept, &roles(), &fixed_s);
            drop(b);
            r
        });
        let goal = match &goal_pair {
            Some(p) => Goal::Pair(p),
            None => Goal::Known(&pin),
        };
        let c = initiate_with(&mut a, &client, goal, &roles(), "laptop", &fixed_c);
        drop(a);
        (c, responder.join().unwrap())
    }

    fn encrypt_first(mut f: Finished) -> Vec<u8> {
        let mut out = vec![0u8; 256];
        let n = f.transport.write_message(b"marker", &mut out).unwrap();
        out.truncate(n);
        out
    }

    #[test]
    fn known_and_pair_handshakes_complete_and_agree() {
        let (c, s) = (id(), id());
        let (cf, sf) = run(
            None,
            None,
            &c,
            &s,
            s.public,
            Fixed::default(),
            Fixed::default(),
        );
        let (cf, (sf, kind)) = (cf.unwrap(), sf.unwrap());
        assert_eq!(kind, Kind::Known);
        assert_eq!(cf.hash, sf.hash);
        assert_eq!(cf.peer, s.public);
        assert_eq!(sf.peer, c.public);
        assert_eq!(sf.device, "laptop");

        let key = *pairing_key("ABCDEFGH23");
        let (cf, sf) = run(
            Some(key),
            Some(key),
            &c,
            &s,
            [0; 32],
            Fixed::default(),
            Fixed::default(),
        );
        let (cf, (sf, kind)) = (cf.unwrap(), sf.unwrap());
        assert_eq!(kind, Kind::Pair);
        assert_eq!(cf.hash, sf.hash);
        assert_eq!(
            sas(&cf.hash, &s.public, &c.public, "laptop"),
            sas(&sf.hash, &s.public, &c.public, "laptop")
        );
    }

    #[test]
    fn wrong_code_or_pin_fails() {
        let (c, s) = (id(), id());
        let (cf, sf) = run(
            Some(*pairing_key("AAAAAAAAAA")),
            Some(*pairing_key("BBBBBBBBBB")),
            &c,
            &s,
            [0; 32],
            Fixed::default(),
            Fixed::default(),
        );
        assert!(cf.is_err() || sf.is_err());
        let other = id();
        let (cf, sf) = run(
            None,
            None,
            &c,
            &s,
            other.public,
            Fixed::default(),
            Fixed::default(),
        );
        assert!(cf.is_err() || sf.is_err());
    }

    /// Sends `msg1` as message 1 and returns what the responder says.
    fn responder_rejects(msg1: Vec<u8>, pairing: bool) -> HandshakeError {
        let (mut a, mut b) = pipe();
        write_frame(&mut a, &msg1).unwrap();
        let key = *pairing_key("ABCDEFGH23");
        let server = id();
        let accept = Accept {
            pairing: pairing.then_some(&key),
            pinned: &|_| true,
        };
        respond(&mut b, &server, &accept, &roles()).err().unwrap()
    }

    #[test]
    fn sec15a_message_one_without_a_valid_encapsulation_key_is_refused() {
        // Length-based kind: too short, too long.
        assert_eq!(
            responder_rejects(vec![0; 32], false),
            HandshakeError::NotAHandshake(32)
        );
        assert_eq!(
            responder_rejects(vec![0; MSG1_LEN - 1], true),
            HandshakeError::NotAHandshake(MSG1_LEN - 1)
        );
        assert_eq!(
            responder_rejects(vec![0; MSG1_LEN + 1], false),
            HandshakeError::NotAHandshake(MSG1_LEN + 1)
        );

        // A well-formed XX message 1 whose encapsulation key has a coefficient >= q.
        let mut invalid = [0xffu8; KEM_LEN];
        invalid[KEM_LEN - 1] = 0;
        let (mut a, mut b) = pipe();
        let server = id();
        let key = *pairing_key("ABCDEFGH23");
        let params = PAIR_PATTERN.parse().unwrap();
        let prologue_text = roles();
        let client_key = id();
        let mut hs = Builder::new(params)
            .local_private_key(client_key.private.as_ref())
            .unwrap()
            .prologue(prologue_text.as_bytes())
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut buf = vec![0u8; MAX_MESSAGE];
        let n = hs.write_message(&invalid, &mut buf).unwrap();
        write_frame(&mut a, &buf[..n]).unwrap();
        let accept = Accept {
            pairing: Some(&key),
            pinned: &|_| true,
        };
        assert_eq!(
            respond(&mut b, &server, &accept, &roles()).err().unwrap(),
            HandshakeError::Kem
        );
    }

    #[test]
    fn sec15a_missing_ciphertext_is_refused() {
        let (mut a, mut b) = pipe();
        let c = id();
        let key = *pairing_key("ABCDEFGH23");
        let server = std::thread::spawn(move || {
            let s = id();
            let m1 = read_frame(&mut b).unwrap();
            let params = PAIR_PATTERN.parse().unwrap();
            let p = roles();
            let mut hs = Builder::new(params)
                .local_private_key(s.private.as_ref())
                .unwrap()
                .prologue(p.as_bytes())
                .unwrap()
                .build_responder()
                .unwrap();
            let mut buf = vec![0u8; MAX_MESSAGE];
            hs.read_message(&m1, &mut buf).unwrap();
            // An empty payload instead of the ciphertext.
            let n = hs.write_message(&[], &mut buf).unwrap();
            write_frame(&mut b, &buf[..n]).unwrap();
        });
        let e = initiate(&mut a, &c, Goal::Pair(&key), &roles(), "x").unwrap_err();
        assert_eq!(e, HandshakeError::Kem);
        server.join().unwrap();
    }

    fn plain_handshake_refused(pattern: &str) {
        let (mut a, mut b) = pipe();
        let s = id();
        let accept = Accept {
            pairing: None,
            pinned: &|_| true,
        };
        let c = id();
        let params = pattern.parse().unwrap();
        let p = roles();
        let builder = Builder::new(params)
            .local_private_key(c.private.as_ref())
            .unwrap()
            .prologue(p.as_bytes())
            .unwrap();
        let builder = if pattern.contains("XK") || pattern.contains("IK") {
            builder.remote_public_key(&s.public).unwrap()
        } else {
            builder
        };
        let mut hs = builder.build_initiator().unwrap();
        let mut buf = vec![0u8; MAX_MESSAGE];
        let n = hs.write_message(&[], &mut buf).unwrap();
        write_frame(&mut a, &buf[..n]).unwrap();
        assert!(matches!(
            respond(&mut b, &s, &accept, &roles()),
            Err(HandshakeError::NotAHandshake(_))
        ));
    }

    #[test]
    fn sec6_sec15a_other_patterns_are_refused() {
        for pattern in [
            "Noise_XK_25519_ChaChaPoly_BLAKE2s",
            "Noise_IK_25519_ChaChaPoly_BLAKE2s",
            "Noise_XX_25519_ChaChaPoly_BLAKE2s",
        ] {
            plain_handshake_refused(pattern);
        }
    }

    fn prologue_mismatch(client_prologue: &str) -> bool {
        let (mut a, mut b) = pipe();
        let (c, s) = (id(), id());
        let pin = s.public;
        let client_prologue = client_prologue.to_owned();
        let t = std::thread::spawn(move || {
            let accept = Accept {
                pairing: None,
                pinned: &|_| true,
            };
            let r = respond(&mut b, &s, &accept, &roles());
            drop(b);
            r.is_err()
        });
        let client_failed = initiate(&mut a, &c, Goal::Known(&pin), &client_prologue, "x").is_err();
        drop(a);
        client_failed || t.join().unwrap()
    }

    #[test]
    fn sec6_sec15a_prologue_differences_fail() {
        for p in [
            "idecraft-session/2;pq=ML-KEM-1024;window>serve",
            "idecraft-session/1;window>serve",
            "idecraft-session/1;pq=ML-KEM-768;window>serve",
            "idecraft-session/1;pq=ML-KEM-1024;device>serve",
        ] {
            assert!(prologue_mismatch(p), "{p}");
        }
    }

    fn fixed_run(ss: Option<[u8; 32]>) -> (Vec<u8>, String) {
        let c = Identity::from_bytes(&[1; 32], &[2; 32]).unwrap();
        let s = {
            let params = PAIR_PATTERN.parse().unwrap();
            let kp = Builder::new(params).generate_keypair().unwrap();
            let _ = kp;
            Identity::from_bytes(&[0; 32], &[3; 32]).unwrap()
        };
        let mut s = s;
        s.public = x25519_public(&s.private);
        let mut c = c;
        c.public = x25519_public(&c.private);
        let fixed_c = Fixed {
            ephemeral: Some([7; 32]),
            kem_seed: Some([9; 64]),
            ss_override: ss,
            ..Fixed::default()
        };
        let fixed_s = Fixed {
            ephemeral: Some([8; 32]),
            encaps: Some([5; 32]),
            ss_override: ss,
            ..Fixed::default()
        };
        let key = *pairing_key("ABCDEFGH23");
        let (cf, sf) = run(Some(key), Some(key), &c, &s, [0; 32], fixed_c, fixed_s);
        let cf = cf.unwrap();
        let _ = sf.unwrap();
        let digits = sas(&cf.hash, &s.public, &c.public, "laptop");
        (encrypt_first(cf), digits)
    }

    fn x25519_public(private: &[u8; 32]) -> [u8; 32] {
        let d = curve25519_dalek::scalar::clamp_integer(*private);
        curve25519_dalek::MontgomeryPoint::mul_base_clamped(d).to_bytes()
    }

    #[test]
    fn sec15b_the_kem_secret_enters_keys_and_digits() {
        let (first, digits) = fixed_run(Some([1; 32]));
        let (other, other_digits) = fixed_run(Some([2; 32]));
        assert_ne!(first, other);
        assert_ne!(digits, other_digits);
        // The same inputs repeat exactly.
        assert_eq!(fixed_run(Some([1; 32])).0, first);
    }

    #[test]
    fn sec15b_a_flipped_ciphertext_bit_fails() {
        let (mut a, mut b) = pipe();
        let (c, s) = (id(), id());
        let key = *pairing_key("ABCDEFGH23");
        let (mut toward_client, mut from_server) = pipe();
        // A man in the middle that flips one bit of message 2.
        let (mut mitm_a, mut mitm_b) = pipe();
        let relay = std::thread::spawn(move || {
            let m1 = read_frame(&mut b).unwrap();
            write_frame(&mut mitm_a, &m1).unwrap();
            let mut m2 = read_frame(&mut mitm_a).unwrap();
            let last = m2.len() - 1;
            m2[last] ^= 1;
            write_frame(&mut b, &m2).unwrap();
            if let Ok(m3) = read_frame(&mut b) {
                let _ = write_frame(&mut mitm_a, &m3);
            }
        });
        let _ = (&mut toward_client, &mut from_server);
        let server = std::thread::spawn(move || {
            let accept = Accept {
                pairing: Some(&key),
                pinned: &|_| true,
            };
            let r = respond(&mut mitm_b, &s, &accept, &roles());
            drop(mitm_b);
            r.is_err()
        });
        let failed = initiate(&mut a, &c, Goal::Pair(&key), &roles(), "x").is_err();
        drop(a);
        let _ = relay.join();
        assert!(failed || server.join().unwrap());
    }

    #[test]
    fn sec15c_libcrux_and_rustcrypto_agree() {
        use ml_kem::kem::{Decapsulate, Encapsulate, KeyExport, TryKeyInit};
        use ml_kem::{Seed, ml_kem_1024};

        // libcrux encapsulates to a RustCrypto key.
        let dk = ml_kem_1024::DecapsulationKey::from_seed(Seed::from([21u8; 64]));
        let ek_bytes: [u8; KEM_LEN] = dk
            .encapsulation_key()
            .to_bytes()
            .as_slice()
            .try_into()
            .unwrap();
        let pk = kem::MlKem1024PublicKey::from(ek_bytes);
        let (ct, ss) = kem::encapsulate(&pk, [11u8; 32]);
        let ours = dk.decapsulate_slice(ct.as_slice()).unwrap();
        assert_eq!(ours.as_slice(), ss.as_slice());

        // RustCrypto encapsulates to a libcrux key.
        let pair = kem::generate_key_pair([13u8; 64]);
        let ek = ml_kem_1024::EncapsulationKey::new_from_slice(pair.pk()).unwrap();
        let (ct, ss) = ek.encapsulate();
        let ct: [u8; KEM_LEN] = ct.as_slice().try_into().unwrap();
        let back = kem::decapsulate(pair.private_key(), &kem::MlKem1024Ciphertext::from(ct));
        assert_eq!(back.as_slice(), ss.as_slice());
    }

    #[test]
    fn sec15c_fixed_seed_gives_the_same_key_pair_in_both_implementations() {
        // Key generation from a fixed seed `d || z` gives the same encapsulation key in libcrux
        // and in the independent RustCrypto code.
        use ml_kem::Seed;
        use ml_kem::kem::KeyExport;
        let d_z = [0x2au8; 64];
        let pair = kem::generate_key_pair(d_z);
        let dk = ml_kem::ml_kem_1024::DecapsulationKey::from_seed(Seed::from(d_z));
        assert_eq!(
            dk.encapsulation_key().to_bytes().as_slice(),
            pair.pk().as_slice()
        );
    }

    #[test]
    fn digits_are_six_and_depend_on_every_input() {
        let (r, i) = ([1; 32], [2; 32]);
        let base = sas(&[7; 32], &r, &i, "a");
        assert_eq!(base.len(), 6);
        assert_ne!(base, sas(&[8; 32], &r, &i, "a"));
        assert_ne!(base, sas(&[7; 32], &i, &r, "a"));
        assert_ne!(base, sas(&[7; 32], &r, &i, "b"));
    }
}
