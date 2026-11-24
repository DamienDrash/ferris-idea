//! The TLS handshake of SQL Server over TDS 7 (`SPEC-0116` v6 behaviours 14 and 15, v7 behaviour
//! 15).
//!
//! In TDS 7 the TLS handshake travels inside pre-login messages (packet type `0x12`). The TLS
//! library of macOS hands over every record of a handshake flight on its own and asks for it to be
//! sent at once; the driver then frames each record as a message of its own, marked as the end of
//! the message, and SQL Server closes the connection after the client's second flight. This stream
//! sits between the socket and the driver: it collects the records of a flight and sends them as
//! one message when the driver next waits for the server, and it reads the server's first flight,
//! which TLS 1.2 sends in the clear: the ServerHello's protocol version, ending the handshake below
//! TLS 1.2, and, when the profile names a CA file, the certificate chain, ending the handshake when
//! it does not lead to a certificate of the file (the driver trusts the system's roots as well and
//! cannot leave them out). The login waits until the chain has passed. Every other message, and
//! everything after the handshake, passes unchanged.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use futures_util::io::{AsyncRead, AsyncWrite};
use rustls_pki_types::{CertificateDer, UnixTime};

/// TDS packet type of pre-login messages.
const PRELOGIN: u8 = 0x12;
/// Bytes of a TDS packet header.
const HEADER: usize = 8;
/// The packet size before the login has agreed on another.
const PACKET: usize = 4096;
/// Status bit of the last packet of a message.
const END_OF_MESSAGE: u8 = 0x01;
/// TLS record type of handshake messages.
const HANDSHAKE: u8 = 0x16;
/// Handshake message types (RFC 5246 §7.4).
const SERVER_HELLO: u8 = 2;
const CERTIFICATE: u8 = 11;
const SERVER_HELLO_DONE: u8 = 14;
/// Most bytes of the server's first flight kept to read it; a longer flight is not read further.
const MAX_FLIGHT: usize = 256 * 1024;

/// Whether `byte` starts a TLS record: change cipher spec, alert, handshake or application data.
fn is_tls_record(byte: u8) -> bool {
    (0x14..=0x17).contains(&byte)
}

/// What the handshake showed, shared with the code that opened the stream.
#[derive(Debug, Default)]
pub(crate) struct Seen {
    /// The version the server chose in its ServerHello: `0` while unknown, else major × 256 +
    /// minor (`0x0303` is TLS 1.2).
    version: AtomicU16,
    /// Why the stream ended the handshake or held the login back for the CA file.
    refusal: Mutex<Option<String>>,
}

impl Seen {
    /// The server's TLS version, `0` while unknown.
    pub(crate) fn version(&self) -> u16 {
        self.version.load(Ordering::Relaxed)
    }

    /// Why the CA file stopped the connection, if it did.
    pub(crate) fn refusal(&self) -> Option<String> {
        self.refusal.lock().ok().and_then(|r| r.clone())
    }
}

#[cfg(test)]
impl Seen {
    /// What a handshake showed that reached the ServerHello's `version`.
    pub(crate) fn with_version(version: u16) -> Self {
        Self {
            version: AtomicU16::new(version),
            refusal: Mutex::default(),
        }
    }

    /// As if the stream had stopped the handshake with `message`.
    pub(crate) fn set_refusal(&self, message: &str) {
        if let Ok(mut refusal) = self.refusal.lock() {
            *refusal = Some(message.to_owned());
        }
    }
}

/// The certificates of a profile's CA file: the only trust anchors of the connection
/// (`SPEC-0116` v7 behaviour 15).
#[derive(Debug, Clone)]
pub(crate) struct Anchors {
    /// The file as the profile names it, for the error.
    pub(crate) file: String,
    /// Its certificates, DER encoded.
    pub(crate) certificates: Vec<Vec<u8>>,
}

/// Whether `chain` — the server's certificate first, then the ones it sent along — leads to a
/// certificate of `anchors` at `now` (`SPEC-0116` v7 behaviour 15): the server's certificate is
/// one of them, or a path through the chain ends at one, with signatures, validity periods, CA
/// rights and the server use checked. The host name is the TLS library's check. The error names
/// the file.
pub(crate) fn anchored(chain: &[Vec<u8>], anchors: &Anchors, now: UnixTime) -> Result<(), String> {
    let file = &anchors.file;
    let Some(leaf) = chain.first() else {
        return Err(format!(
            "the server showed no certificate to check against the CA file {file}"
        ));
    };
    if anchors.certificates.contains(leaf) {
        return Ok(());
    }
    fn der(c: &Vec<u8>) -> CertificateDer<'_> {
        CertificateDer::from(c.as_slice())
    }
    let files: Vec<CertificateDer<'_>> = anchors.certificates.iter().map(der).collect();
    let roots: Vec<_> = files
        .iter()
        .filter_map(|c| webpki::anchor_from_trusted_cert(c).ok())
        .collect();
    let leaf = der(leaf);
    let leaf = webpki::EndEntityCert::try_from(&leaf).map_err(|e| {
        format!(
            "the server's certificate cannot be read ({e:?}) to check it against the CA file {file}"
        )
    })?;
    let sent: Vec<CertificateDer<'_>> = chain[1..].iter().map(der).collect();
    leaf.verify_for_usage(
        webpki::ALL_VERIFICATION_ALGS,
        &roots,
        &sent,
        now,
        webpki::KeyUsage::server_auth(),
        None,
        None,
    )
    .map(|_| ())
    .map_err(|e| match e {
        webpki::Error::UnknownIssuer => format!(
            "the server's certificate was not issued by a certificate in the CA file {file}, the \
             only trust anchor of a connection that names one"
        ),
        other => format!(
            "the server's certificate does not lead to a certificate in the CA file {file} \
             ({other:?}), the only trust anchor of a connection that names one"
        ),
    })
}

/// The error when the handshake is over and the chain was never in the clear (TLS 1.3).
fn chain_not_shown(anchors: &Anchors) -> String {
    format!(
        "the handshake did not show the server's certificate (TLS 1.3 encrypts it), so it could \
         not be checked against the CA file {}; allow TLS 1.2 on the server (`mssql-conf set \
         network.tlsprotocols 1.2` on Linux)",
        anchors.file
    )
}

/// What the server's bytes since the client's first flight show; TLS 1.2 sends all of it in the
/// clear.
#[derive(Debug, Default, PartialEq, Eq)]
struct Flight {
    /// The ServerHello's version, once its first bytes are there.
    version: Option<u16>,
    /// The chain of the Certificate message, once it is complete.
    chain: Option<Vec<Vec<u8>>>,
    /// Nothing more of the handshake comes in the clear: ServerHelloDone, a record that is no
    /// handshake (cipher change, alert, encrypted data), or bytes that are no pre-login packet.
    over: bool,
}

/// A 24-bit length.
fn u24(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take(3)
        .fold(0, |n, &b| (n << 8) | usize::from(b))
}

/// The certificates of a TLS 1.2 Certificate message's body, DER encoded.
fn certificate_list(body: &[u8]) -> Option<Vec<Vec<u8>>> {
    let total = u24(body.get(..3)?);
    let mut rest = body.get(3..3 + total)?;
    let mut chain = Vec::new();
    while !rest.is_empty() {
        let length = u24(rest.get(..3)?);
        chain.push(rest.get(3..3 + length)?.to_vec());
        rest = &rest[3 + length..];
    }
    Some(chain)
}

/// Reads `bytes`, all the server sent since the client's first flight: pre-login packets, the TLS
/// records in them and the handshake messages in those, as far as they have arrived.
fn server_flight(bytes: &[u8]) -> Flight {
    let mut flight = Flight::default();
    let mut tls = Vec::new();
    let mut rest = bytes;
    while rest.len() >= HEADER {
        let length = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
        if rest[0] != PRELOGIN || length < HEADER {
            flight.over = true;
            break;
        }
        let end = length.min(rest.len());
        tls.extend_from_slice(&rest[HEADER..end]);
        rest = &rest[end..];
    }
    let mut handshake = Vec::new();
    let mut rest = tls.as_slice();
    while rest.len() >= 5 {
        if rest[0] != HANDSHAKE {
            flight.over = true;
            break;
        }
        let end = (5 + usize::from(u16::from_be_bytes([rest[3], rest[4]]))).min(rest.len());
        handshake.extend_from_slice(&rest[5..end]);
        rest = &rest[end..];
    }
    let mut rest = handshake.as_slice();
    while rest.len() >= 4 {
        let (kind, length, body) = (rest[0], u24(&rest[1..4]), &rest[4..]);
        if kind == SERVER_HELLO && body.len() >= 2 {
            flight.version = Some(u16::from_be_bytes([body[0], body[1]]));
        }
        let Some(body) = body.get(..length) else {
            break;
        };
        match kind {
            CERTIFICATE => flight.chain = certificate_list(body),
            SERVER_HELLO_DONE => flight.over = true,
            _ => {}
        }
        rest = &rest[4 + length..];
    }
    flight
}

/// The name of a TLS protocol version as the ServerHello carries it.
pub(crate) fn version_name(version: u16) -> String {
    match version {
        0x0300 => "SSL 3.0".into(),
        v if v >> 8 == 3 => format!("TLS 1.{}", (v & 0xff) - 1),
        v => format!("version {v:#06x}"),
    }
}

/// Whether the server chose a version below TLS 1.2 (`SPEC-0016` v2 behaviour 9).
pub(crate) fn too_old(version: u16) -> bool {
    version != 0 && version < 0x0303
}

/// A socket of a SQL Server connection, see the module documentation.
pub(crate) struct HandshakeFrames<S> {
    inner: S,
    /// Boxed, so the session stays small once the handshake is over.
    state: Box<Handshake>,
}

/// What [`HandshakeFrames`] keeps during the handshake.
struct Handshake {
    /// The handshake is over: bytes pass unchanged both ways.
    passing: bool,
    /// Bytes of a pre-login packet the driver has not finished writing.
    partial: Vec<u8>,
    /// The TLS records of the flight being collected, without their packet headers.
    flight: Vec<u8>,
    /// The header of the flight's first packet; its ids are kept as the driver set them.
    head: [u8; HEADER],
    /// Framed bytes waiting for the socket.
    out: Vec<u8>,
    /// The client's first flight went out and the server's flight is being read.
    watching: bool,
    /// Whether the server's flight was read already (it is read once).
    watched: bool,
    /// The server's bytes since the client's first flight, while they are read.
    server: Vec<u8>,
    /// The CA file, when it is the only trust anchor.
    anchors: Option<Anchors>,
    /// The server's chain led to the CA file.
    anchored: bool,
    seen: Arc<Seen>,
}

impl<S> HandshakeFrames<S> {
    /// Wraps `inner`; what the handshake shows is written to `seen`. With `anchors` the server's
    /// certificate chain must lead to one of them before the login leaves.
    pub(crate) fn new(inner: S, seen: Arc<Seen>, anchors: Option<Anchors>) -> Self {
        Self {
            inner,
            state: Box::new(Handshake {
                passing: false,
                partial: Vec::new(),
                flight: Vec::new(),
                head: [0; HEADER],
                out: Vec::new(),
                watching: false,
                watched: false,
                server: Vec::new(),
                anchors,
                anchored: false,
                seen,
            }),
        }
    }
}

impl Handshake {
    /// Frames the collected flight as one pre-login message, split into packets of at most
    /// [`PACKET`] bytes with only the last marked as the end of the message.
    fn frame_flight(&mut self) {
        if self.flight.is_empty() {
            return;
        }
        let count = self.flight.len().div_ceil(PACKET - HEADER);
        for (index, chunk) in self.flight.chunks(PACKET - HEADER).enumerate() {
            let mut header = self.head;
            header[1] = if index + 1 == count {
                END_OF_MESSAGE
            } else {
                0
            };
            // At most PACKET bytes, so the length fits the header's two bytes.
            let length = u16::try_from(chunk.len() + HEADER).unwrap_or(u16::MAX);
            header[2..4].copy_from_slice(&length.to_be_bytes());
            header[6] = header[6].wrapping_add(u8::try_from(index & 0xff).unwrap_or_default());
            self.out.extend_from_slice(&header);
            self.out.extend_from_slice(chunk);
        }
        self.flight.clear();
        if !self.watched {
            self.watching = true;
            self.watched = true;
        }
    }

    /// Records why the CA file stops the connection and turns it into the stream's error.
    fn refuse(&self, message: String) -> io::Error {
        if let Ok(mut refusal) = self.seen.refusal.lock() {
            *refusal = Some(message.clone());
        }
        io::Error::new(io::ErrorKind::InvalidData, message)
    }

    /// The handshake is over and the driver sends its first bytes after it, the login: with a CA
    /// file they leave only after the server's chain led to it (`SPEC-0116` v7 behaviour 15).
    fn handshake_over(&mut self) -> io::Result<()> {
        if let Some(anchors) = &self.anchors
            && !self.anchored
        {
            return Err(self.refuse(chain_not_shown(anchors)));
        }
        self.frame_flight();
        self.passing = true;
        self.watching = false;
        self.server = Vec::new();
        Ok(())
    }

    /// Sorts the complete packets in `partial`: TLS records join the flight, any other pre-login
    /// message goes out after the flight before it.
    fn sort_packets(&mut self) -> io::Result<()> {
        while self.partial.len() >= HEADER {
            let length = usize::from(u16::from_be_bytes([self.partial[2], self.partial[3]]));
            if length < HEADER || self.partial[0] != PRELOGIN {
                // Not a pre-login packet the handshake would send: stop interpreting.
                self.handshake_over()?;
                self.out.append(&mut self.partial);
                return Ok(());
            }
            if self.partial.len() < length {
                return Ok(());
            }
            let packet: Vec<u8> = self.partial.drain(..length).collect();
            if packet.get(HEADER).copied().is_some_and(is_tls_record) {
                if self.flight.is_empty() {
                    self.head.copy_from_slice(&packet[..HEADER]);
                }
                self.flight.extend_from_slice(&packet[HEADER..]);
            } else {
                self.frame_flight();
                self.out.extend_from_slice(&packet);
            }
        }
        Ok(())
    }

    /// Reads the server's flight as it arrives: the ServerHello's version, an error below TLS 1.2,
    /// and with a CA file the certificate chain, an error when it does not lead to the file.
    fn watch(&mut self, read: &[u8]) -> io::Result<()> {
        self.server.extend_from_slice(read);
        let flight = server_flight(&self.server);
        if let Some(version) = flight.version
            && self.seen.version() == 0
        {
            self.seen.version.store(version, Ordering::Relaxed);
            if too_old(version) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "the server chose {}; Ferris IDEA needs TLS 1.2 or newer",
                        version_name(version)
                    ),
                ));
            }
        }
        if let (Some(chain), Some(anchors)) = (&flight.chain, &self.anchors)
            && !self.anchored
        {
            match anchored(chain, anchors, UnixTime::now()) {
                Ok(()) => self.anchored = true,
                Err(message) => return Err(self.refuse(message)),
            }
        }
        let read_all = flight.version.is_some() && (self.anchors.is_none() || self.anchored);
        if flight.over || read_all || self.server.len() > MAX_FLIGHT {
            self.watching = false;
            self.server = Vec::new();
        }
        Ok(())
    }
}

impl<S: AsyncWrite + Unpin> HandshakeFrames<S> {
    /// Writes the framed bytes to the socket.
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.state.out.is_empty() {
            let written = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.state.out))?;
            if written == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.state.out.drain(..written);
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for HandshakeFrames<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        // The driver waits for the server: the flight it wrote goes out first.
        if !this.state.flight.is_empty() || !this.state.out.is_empty() {
            this.state.frame_flight();
            ready!(this.drain(cx))?;
            ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
        }
        let read = ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        if this.state.watching && read > 0 {
            this.state.watch(&buf[..read])?;
        }
        Poll::Ready(Ok(read))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for HandshakeFrames<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        if !this.state.passing
            && !buf.is_empty()
            && this.state.partial.is_empty()
            && buf.first() != Some(&PRELOGIN)
        {
            // The first bytes that are not a pre-login packet: the handshake is over.
            this.state.handshake_over()?;
        }
        if this.state.passing {
            ready!(this.drain(cx))?;
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        this.state.partial.extend_from_slice(buf);
        this.state.sort_packets()?;
        Poll::Ready(Ok(buf.len()))
    }

    /// Sends what is framed; a flight being collected waits for the next read.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        this.state.frame_flight();
        let state = &mut *this.state;
        state.out.append(&mut state.partial);
        ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Waker;

    /// A socket in memory: what was written, and what the server answers.
    #[derive(Default)]
    struct Wire {
        written: Vec<u8>,
        flushes: usize,
        answer: Vec<u8>,
    }

    impl AsyncRead for Wire {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let n = self.answer.len().min(buf.len());
            buf[..n].copy_from_slice(&self.answer[..n]);
            self.answer.drain(..n);
            Poll::Ready(Ok(n))
        }
    }

    impl AsyncWrite for Wire {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.written.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.flushes += 1;
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn packet(payload: &[u8]) -> Vec<u8> {
        let mut p = vec![PRELOGIN, END_OF_MESSAGE, 0, 0, 0, 0, 0, 0];
        p[2..4].copy_from_slice(&u16::try_from(payload.len() + HEADER).unwrap().to_be_bytes());
        p.extend_from_slice(payload);
        p
    }

    fn frames() -> HandshakeFrames<Wire> {
        HandshakeFrames::new(Wire::default(), Arc::default(), None)
    }

    fn write(f: &mut HandshakeFrames<Wire>, bytes: &[u8]) {
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            Pin::new(&mut *f).poll_write(&mut cx, bytes),
            Poll::Ready(Ok(n)) if n == bytes.len()
        ));
        assert!(matches!(
            Pin::new(&mut *f).poll_flush(&mut cx),
            Poll::Ready(Ok(()))
        ));
    }

    fn read(f: &mut HandshakeFrames<Wire>) -> io::Result<Vec<u8>> {
        let mut cx = Context::from_waker(Waker::noop());
        let mut buf = [0u8; 64];
        match Pin::new(&mut *f).poll_read(&mut cx, &mut buf) {
            Poll::Ready(Ok(n)) => Ok(buf[..n].to_vec()),
            Poll::Ready(Err(e)) => Err(e),
            Poll::Pending => panic!("pending"),
        }
    }

    /// The pre-login packets in `bytes`: (status, payload).
    fn packets(mut bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        while !bytes.is_empty() {
            assert_eq!(bytes[0], PRELOGIN);
            let length = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
            out.push((bytes[1], bytes[HEADER..length].to_vec()));
            bytes = &bytes[length..];
        }
        out
    }

    /// SPEC-0116 v6 AK-10: key exchange, cipher change and finished, each written and flushed on
    /// its own, leave as one message with one header when the driver reads next.
    #[test]
    fn a_flight_leaves_as_one_message() {
        let mut f = frames();
        let records: [&[u8]; 3] = [
            &[0x16, 3, 3, 0, 2, 0x10, 9],
            &[0x14, 3, 3, 0, 1, 1],
            &[0x16, 3, 3, 0, 1, 0xee],
        ];
        for r in records {
            write(&mut f, &packet(r));
        }
        assert!(f.inner.written.is_empty(), "nothing leaves before the read");
        let _ = read(&mut f);
        assert_eq!(
            packets(&f.inner.written),
            [(END_OF_MESSAGE, records.concat())]
        );
    }

    /// SPEC-0116 v6 AK-10: a flight longer than a packet leaves as packets of at most 4096
    /// bytes, only the last marked as the end of the message.
    #[test]
    fn a_long_flight_is_split_at_the_packet_size() {
        let mut f = frames();
        let mut record = vec![0x16, 3, 3, 0x17, 0x6f];
        record.resize(6000, 0xab);
        write(&mut f, &packet(&record));
        let _ = read(&mut f);
        let sent = packets(&f.inner.written);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, 0);
        assert_eq!(sent[0].1.len(), PACKET - HEADER);
        assert_eq!(sent[1].0, END_OF_MESSAGE);
        assert_eq!([sent[0].1.clone(), sent[1].1.clone()].concat(), record);
    }

    /// SPEC-0116 v6 AK-10: the pre-login message itself leaves at once, a login without TLS and
    /// everything after the handshake pass unchanged.
    #[test]
    fn other_messages_pass_unchanged() {
        let mut f = frames();
        let prelogin = packet(&[0x00, 0x00, 0x1a, 0x00, 0x06, 0xff]);
        write(&mut f, &prelogin);
        assert_eq!(f.inner.written, prelogin);
        let login = [0x10, 0x01, 0x00, 0x0a, 0, 0, 1, 0, 7, 7];
        write(&mut f, &login);
        let data = [0x17, 3, 3, 0, 2, 1, 2];
        write(&mut f, &data);
        assert_eq!(
            f.inner.written,
            [prelogin, login.to_vec(), data.to_vec()].concat()
        );
        assert!(f.state.passing);
    }

    fn server_hello(version: [u8; 2]) -> Vec<u8> {
        packet(&[
            0x16, 3, 3, 0, 0x2a, 0x02, 0, 0, 0x26, version[0], version[1], 0, 0,
        ])
    }

    /// SPEC-0116 v6 AK-10: a ServerHello with TLS 1.0 or 1.1 ends the handshake with the version
    /// named; TLS 1.2 passes and is remembered.
    #[test]
    fn old_tls_versions_are_refused() {
        for (version, name) in [([3, 1], "TLS 1.0"), ([3, 2], "TLS 1.1")] {
            let mut f = frames();
            write(&mut f, &packet(&[0x16, 3, 1, 0, 1, 1]));
            f.inner.answer = server_hello(version);
            let e = read(&mut f).unwrap_err().to_string();
            assert!(e.contains(name) && e.contains("TLS 1.2 or newer"), "{e}");
            assert!(too_old(f.state.seen.version()));
        }
        let mut f = frames();
        write(&mut f, &packet(&[0x16, 3, 1, 0, 1, 1]));
        f.inner.answer = server_hello([3, 3]);
        let answer = f.inner.answer.clone();
        assert_eq!(read(&mut f).unwrap(), answer);
        assert_eq!(f.state.seen.version(), 0x0303);
        assert_eq!(version_name(0x0303), "TLS 1.2");
        assert_eq!(version_name(0x0300), "SSL 3.0");
    }

    /// A certificate authority of the tests, with what it can sign.
    struct Authority {
        params: rcgen::CertificateParams,
        key: rcgen::KeyPair,
        der: Vec<u8>,
    }

    impl Authority {
        fn new(name: &str) -> Self {
            let key = rcgen::KeyPair::generate().unwrap();
            let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, name);
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params.key_usages = vec![
                rcgen::KeyUsagePurpose::KeyCertSign,
                rcgen::KeyUsagePurpose::CrlSign,
            ];
            let der = params.self_signed(&key).unwrap().der().to_vec();
            Self { params, key, der }
        }

        /// A server certificate for `localhost`, DER encoded.
        fn server(&self) -> Vec<u8> {
            let mut params = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
            params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
            let issuer = rcgen::Issuer::from_params(&self.params, &self.key);
            let key = rcgen::KeyPair::generate().unwrap();
            params.signed_by(&key, &issuer).unwrap().der().to_vec()
        }

        /// An intermediate authority signed by this one.
        fn intermediate(&self, name: &str) -> Self {
            let key = rcgen::KeyPair::generate().unwrap();
            let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, name);
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
            let issuer = rcgen::Issuer::from_params(&self.params, &self.key);
            let der = params.signed_by(&key, &issuer).unwrap().der().to_vec();
            Self { params, key, der }
        }
    }

    fn file(certificates: &[&[u8]]) -> Anchors {
        Anchors {
            file: "~/private-ca.pem".into(),
            certificates: certificates.iter().map(|c| c.to_vec()).collect(),
        }
    }

    /// SPEC-0116 v7 AK-11: a chain must lead to a certificate of the CA file, or be one; a
    /// certificate of another authority — one the system might trust — is refused naming the file.
    #[test]
    fn the_ca_file_is_the_only_anchor() {
        let ours = Authority::new("Private CA");
        let public = Authority::new("Some public CA");
        let now = UnixTime::now();
        let server = ours.server();
        assert_eq!(
            anchored(std::slice::from_ref(&server), &file(&[&ours.der]), now),
            Ok(())
        );
        // The file may hold several certificates, and the server's own certificate.
        assert_eq!(
            anchored(
                std::slice::from_ref(&server),
                &file(&[&public.der, &ours.der]),
                now
            ),
            Ok(())
        );
        assert_eq!(
            anchored(std::slice::from_ref(&server), &file(&[&server]), now),
            Ok(())
        );
        // Through an intermediate the server sends along, not without it.
        let middle = ours.intermediate("Issuing CA");
        let below = middle.server();
        assert_eq!(
            anchored(
                &[below.clone(), middle.der.clone()],
                &file(&[&ours.der]),
                now
            ),
            Ok(())
        );
        assert!(anchored(&[below], &file(&[&ours.der]), now).is_err());
        // Another authority's certificate for the same name: refused, the file named.
        let theirs = public.server();
        let e = anchored(&[theirs], &file(&[&ours.der]), now).unwrap_err();
        assert!(
            e.contains("not issued by a certificate in the CA file ~/private-ca.pem"),
            "{e}"
        );
        let e = anchored(&[], &file(&[&ours.der]), now).unwrap_err();
        assert!(e.contains("no certificate"), "{e}");
        let e = anchored(&[vec![0x30, 0x03, 1, 2, 3]], &file(&[&ours.der]), now).unwrap_err();
        assert!(e.contains("cannot be read"), "{e}");
    }

    /// A handshake message of `kind` with `body`.
    fn message(kind: u8, body: &[u8]) -> Vec<u8> {
        let n = u32::try_from(body.len()).unwrap().to_be_bytes();
        [&[kind, n[1], n[2], n[3]][..], body].concat()
    }

    /// A TLS record of `kind` with `body`.
    fn record(kind: u8, body: &[u8]) -> Vec<u8> {
        let n = u16::try_from(body.len()).unwrap().to_be_bytes();
        [&[kind, 3, 3, n[0], n[1]][..], body].concat()
    }

    /// The Certificate message of `chain`.
    fn certificate(chain: &[&[u8]]) -> Vec<u8> {
        let entries: Vec<u8> = chain
            .iter()
            .flat_map(|c| {
                let n = u32::try_from(c.len()).unwrap().to_be_bytes();
                [&n[1..], c].concat()
            })
            .collect();
        let n = u32::try_from(entries.len()).unwrap().to_be_bytes();
        message(CERTIFICATE, &[&n[1..], &entries[..]].concat())
    }

    /// A server's first TLS 1.2 flight with `chain`, as pre-login packets of at most `size` bytes.
    fn tls12_flight(chain: &[&[u8]], size: usize) -> Vec<u8> {
        let hello = message(SERVER_HELLO, &[3, 3, 0, 0]);
        let handshake = [hello, certificate(chain), message(SERVER_HELLO_DONE, &[])].concat();
        let tls = record(HANDSHAKE, &handshake);
        tls.chunks(size - HEADER).flat_map(packet).collect()
    }

    /// SPEC-0116 v7 AK-11: the chain is read from the server's flight, also across packets; the
    /// flight is over at ServerHelloDone, or at the first record that is no handshake.
    #[test]
    fn the_chain_is_read_from_the_server_flight() {
        let ca = Authority::new("Private CA");
        let server = ca.server();
        let bytes = tls12_flight(&[&server, &ca.der], 256);
        assert!(bytes.len() > 3 * 256, "split over several packets");
        assert_eq!(
            server_flight(&bytes),
            Flight {
                version: Some(0x0303),
                chain: Some(vec![server.clone(), ca.der.clone()]),
                over: true,
            }
        );
        // Half of it: the version is there, the chain not yet.
        let half = server_flight(&bytes[..bytes.len() / 2]);
        assert_eq!(
            (half.version, half.chain, half.over),
            (Some(0x0303), None, false)
        );
        // TLS 1.3: the ServerHello, then a cipher change; the chain never comes in the clear.
        let hidden = [
            packet(&record(HANDSHAKE, &message(SERVER_HELLO, &[3, 3, 0, 0]))),
            packet(&record(0x14, &[1])),
        ]
        .concat();
        let flight = server_flight(&hidden);
        assert_eq!((flight.chain, flight.over), (None, true));
    }

    fn reads(f: &mut HandshakeFrames<Wire>) -> io::Result<()> {
        while !f.inner.answer.is_empty() {
            read(f)?;
        }
        Ok(())
    }

    fn try_write(f: &mut HandshakeFrames<Wire>, bytes: &[u8]) -> io::Result<()> {
        let mut cx = Context::from_waker(Waker::noop());
        match Pin::new(&mut *f).poll_write(&mut cx, bytes) {
            Poll::Ready(r) => r.map(|_| ()),
            Poll::Pending => panic!("pending"),
        }
    }

    fn with_ca(ca: &Authority) -> HandshakeFrames<Wire> {
        HandshakeFrames::new(Wire::default(), Arc::default(), Some(file(&[&ca.der])))
    }

    /// SPEC-0116 v7 AK-11: a chain of another authority ends the handshake when it is read, before
    /// the client's next flight, with the file named; the chain of the file's authority lets the
    /// login pass.
    #[test]
    fn a_chain_outside_the_ca_file_ends_the_handshake() {
        let ours = Authority::new("Private CA");
        let public = Authority::new("Some public CA");
        let hello = packet(&[0x16, 3, 1, 0, 1, 1]);
        let login = [0x17, 3, 3, 0, 2, 7, 7];

        let mut f = with_ca(&ours);
        write(&mut f, &hello);
        f.inner.answer = tls12_flight(&[&public.server()], 4096);
        let e = reads(&mut f).unwrap_err().to_string();
        assert!(e.contains("CA file ~/private-ca.pem"), "{e}");
        assert_eq!(f.state.seen.refusal().as_deref(), Some(e.as_str()));

        let mut f = with_ca(&ours);
        write(&mut f, &hello);
        f.inner.answer = tls12_flight(&[&ours.server()], 4096);
        reads(&mut f).unwrap();
        assert!(f.state.anchored && f.state.seen.refusal().is_none());
        write(&mut f, &login);
        assert!(f.inner.written.ends_with(&login));
    }

    /// SPEC-0116 v7 AK-11: with a CA file, a handshake that never showed the chain (TLS 1.3) holds
    /// the login back with an error saying so; without a CA file the same login passes.
    #[test]
    fn no_login_without_a_checked_chain() {
        let ca = Authority::new("Private CA");
        let hello = packet(&[0x16, 3, 1, 0, 1, 1]);
        let tls13 = [
            packet(&record(HANDSHAKE, &message(SERVER_HELLO, &[3, 3, 0, 0]))),
            packet(&record(0x14, &[1])),
            packet(&record(0x17, &[9; 40])),
        ]
        .concat();
        let login = [0x17, 3, 3, 0, 2, 7, 7];

        let mut f = with_ca(&ca);
        write(&mut f, &hello);
        f.inner.answer = tls13.clone();
        reads(&mut f).unwrap();
        let sent = f.inner.written.len();
        let e = try_write(&mut f, &login).unwrap_err().to_string();
        assert!(e.contains("TLS 1.3") && e.contains("tlsprotocols"), "{e}");
        assert_eq!(f.inner.written.len(), sent, "no login byte left");
        assert!(!f.state.passing);

        let mut f = frames();
        write(&mut f, &hello);
        f.inner.answer = tls13;
        reads(&mut f).unwrap();
        write(&mut f, &login);
        assert!(f.inner.written.ends_with(&login));
    }
}
