//! The paired channel of IDECraft (`SPEC-0321`, `ADR-0025` §1b, §1c): the hybrid Noise handshake,
//! the encrypted channel, the key store, pairing with a short code and six digits, and the
//! listener of `idecraft serve --pair`. A window reaches a host without SSH through this crate.

pub mod audit;
pub mod channel;
pub mod client;
pub mod code;
pub mod keys;
pub mod listener;
pub mod noise;

pub use channel::{Channel, ChildPipe, Limits, Pipe};
pub use client::{
    PairError, PendingPairing, connect_known, connect_known_on, connect_pinned,
    default_device_name, start_pairing, start_pairing_on, start_pairing_with_link,
};
pub use code::{PairLink, parse_link};
pub use keys::{Device, EXIT_REFUSED, KeyError, PairedHost, Side, Store};
pub use listener::{Console, ListenError, ListenOptions, Running, Timing, listen};
pub use noise::{Identity, fingerprint};

/// The default port of `idecraft serve --pair`.
pub const DEFAULT_PORT: u16 = 47100;
