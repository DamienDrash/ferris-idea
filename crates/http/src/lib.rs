//! HTTP client for Ferris IDEA.
//!
//! Implements `SPEC-0046`: request files (`.http`) with variables, environment files (`SPEC-0102`), and sending
//! requests over HTTP/1.1 with TLS from the system library (`native-tls`).

mod body;
mod client;
mod coding;
mod env;
mod file;
mod highlight;

pub use body::{MAX_BODY_FILE, body_files, finish_body};
pub use client::{
    DEFAULT_TIMEOUT, HttpError, MAX_REDIRECTS, Response, SendOptions, Target, fetch_small_text,
    parse_url, send, send_once, send_streaming, send_with,
};
pub use coding::{
    Decoded, MAX_DECODE_RATIO, MAX_DECODED_BODY, RATIO_FLOOR, limit_text as decode_limit,
};
pub use env::{
    ENV_FILE, EnvironmentChoice, Environments, PRIVATE_ENV_FILE, SHARED, load_environments,
    mask_for_log,
};
pub use file::{
    MAX_RANDOM_LENGTH, RawRequest, RequestFile, RequestSettings, ResolvedRequest, parse,
    request_line_words, resolve, substitute,
};
pub use highlight::{Highlight, highlight, overlay};
