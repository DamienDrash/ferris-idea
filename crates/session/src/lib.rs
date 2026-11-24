//! The session protocol between the window and `ferris-idea serve` (`ADR-0026`, `SPEC-0317` v2;
//! security per `ADR-0025`).
//!
//! Layers: [`frame`] (length, type, stream id, payload; at most 1 MiB), [`rpc`] (JSON-RPC 2.0 with
//! `domain.verb` names and stable error codes), [`hello`] (version choice without downgrade,
//! resume tokens), [`events`] (numbered notifications, replay after a break), [`ot`] (edits and
//! their transformation), [`docs`] (the backend's documents), [`backend`] (sessions, rights,
//! audit, file watching, reattach socket), [`client`] and [`replica`] (the window's half), and
//! [`transport`] (byte streams, queues, credit, in-process pipes).

pub mod backend;
pub mod client;
pub mod docs;
pub mod events;
pub mod forward;
pub mod frame;
pub mod fsops;
pub mod hello;
pub mod ot;
pub mod paths;
pub mod reattach;
pub mod replica;
pub mod room;
pub mod rpc;
pub mod testing;
pub mod transport;

pub use backend::{
    Backend, BackendConfig, Call, Handler, Peer, Right, RoomLink, SessionHandle, StreamIn,
    StreamOut, StreamSink,
};
pub use client::{Connection, Inbound};
pub use ot::{Op, TextOp};
pub use replica::{ClientSession, Reconnected, Recovery, RecoveryCopy, RemoteDoc, SessionInfo};
pub use rpc::{ErrorCode, Message, RpcError};
pub use transport::{Transport, in_process};
