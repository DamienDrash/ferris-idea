# SPEC-0325: Secure WebSocket endpoint of `idecraft serve` (first slice of M18)

- **Version:** 1 · **Date:** 2026-11-09
- **Sources:** [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M18 and the
  Project Owner's order of 2026-11-09 to implement M18 before QG-1 and decide open points (`GEN`);
  [`ADR-0025`](../adr/0025-network-security-for-remote-connections.md) §§1, 4, 5, 8, 10,
  [`ADR-0026`](../adr/0026-session-protocol-window-and-serve.md), SPEC-0317, SPEC-0323, SPEC-0324
  (`GEN`). TLS 1.3 RFC 8446, WebSocket RFC 6455, Fetch `Origin`, Content Security Policy, FIPS 203
  and `draft-ietf-tls-ecdhe-mlkem` (group `X25519MLKEM768`, `0x11EC`) (`STD`). Crates: `rustls` with
  its `ring` provider, `libcrux-ml-kem` (already chosen in ADR-0025 §10), `tokio-tungstenite`
  (`OSS`, section "Dependencies"). No observation of the reference is needed: a browser path to
  `idecraft serve` is IDECraft's own scope.
- **Feature matrix rows:** M18 (no reference counterpart)
- **Reuses, does not replace:** frames and sessions of `Backend` (ADR-0026), rights of ADR-0025 §4,
  rooms, roles and links of SPEC-0323.

## Purpose

A browser cannot speak SSH or the paired Noise channel. `idecraft serve --wss <folder>` gives it a
third path to the same backend: WebSocket over TLS 1.3 with a hybrid post-quantum key exchange,
authenticated before anything reaches the backend. On top runs the session protocol unchanged.
This slice has no web client; it ships a small connection check page (see behaviour 16).

## Behaviour

### Listening

1. Nothing listens without `--wss`. `--wss` excludes `--stdio` and `--pair`, and needs a `<folder>`:
   a connection is bound to one project, as a device is (SPEC-0324 behaviour 2). [ADR-0025 §4, §5]
2. `--wss` binds `127.0.0.1:47110` by default. A non-loopback `--listen` address is refused unless
   `--allow-network` is also given, and then at least one `--wss-origin` is required. The start line
   says when the listener is reachable from the network. [ADR-0025 §5]
3. There is no plaintext path in any build: the listener type can only be built from a TLS
   configuration, there is no feature or setting that drops TLS, and the first bytes on a connection
   go to the TLS stack. A plain HTTP or `ws://` request gets no HTTP answer. [ADR-0025 §1]

### TLS

4. TLS 1.3 only. The only key exchange group is `X25519MLKEM768`; a client offering only classical
   groups fails the handshake. This is the stricter reading of "post-quantum where offered" and is
   the same rule as for SSH (ADR-0025 §1a). Current browsers offer this group by default. The
   hybrid is built from `libcrux-ml-kem` and the X25519 of `ring` as a `rustls` key exchange group;
   `aws-lc-rs` is not used because its licence is not on the allow list. [ADR-0025 §1c, §10, STD]
5. `--wss-cert` and `--wss-key` name PEM files and are required; there is no self-signed fallback.
   On Unix a key file readable by group or others refuses the start. A missing or unusable file
   refuses the start before anything listens. [ADR-0025]
6. Certificate and key are never copied, logged or written anywhere. Test certificates are generated
   at test time. [ADR-0025]

### HTTP and origins

7. On every connection the request head is read with limits: at most 16 KiB, at most 64 header
   fields, within 10 seconds. Only `GET` is served. [ADR-0025 §8]
8. `Host` must be an allowed authority. With a loopback listener those are `localhost`, `127.0.0.1`
   and `[::1]` with the listener's port; `--wss-origin https://name[:port]` adds more. Anything else
   answers 421 (DNS rebinding). [ADR-0025 T6]
9. A WebSocket upgrade needs an `Origin` header that is exactly `https://<allowed authority>`. A
   missing origin, `null`, `http:` or any other origin answers 403 before any credential is read.
   `--wss-origin` accepts only `https://` origins without a path or wildcard. [ADR-0025 T6]
10. Every HTTP answer carries `Content-Security-Policy: default-src 'none'; script-src 'self';
    connect-src 'self'; style-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`,
    `Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`,
    `Cross-Origin-Resource-Policy: same-origin`, `X-Content-Type-Options: nosniff`,
    `Referrer-Policy: no-referrer` and `Cache-Control: no-store`. On a non-loopback listener also
    `Strict-Transport-Security: max-age=31536000`. [ADR-0025 T6, STD]

### Authentication

11. There is no token-less mode. After the upgrade the first WebSocket message must be a text message
    `{"credential":"<secret>"}`, within 10 seconds. It is not in the URL or in a header, so it stays
    out of logs. Nothing reaches the backend before it is verified. The answer is the text message
    `{"ok":true,"as":"owner"|"guest"}`; a refusal closes with code 1008 and always the same reason, so
    it tells a prober nothing. [ADR-0025, SPEC-0323 behaviour 17]
12. **Owner credential.** At start the server creates a random 256-bit secret, keeps only its SHA-256
    hash in memory and prints the secret once to its console, like the pairing code (SPEC-0324
    behaviour 3). It ends with the process. Its session's peer is `web:owner`, a user of the server in
    the sense of SPEC-0323 behaviour 18, with the rights of `--wss-rights` (deny by default: only
    `view` unless more are named; the names of ADR-0025 §4). [ADR-0025 §4]
13. **Room link.** An `anonymous` link of the project's room (SPEC-0323 behaviour 19), given as the
    bare secret or the whole link, is accepted when it is usable now. Its session's peer is
    `web-guest:<random>`, of kind guest, with `view` only; the client must then call `room.join` with
    the link (SPEC-0323 behaviour 15), so link expiry, revocation and removal work as in the room. An
    `organisation` link is refused here: it needs a person the server has authenticated (behaviour
    18), which is the owner credential until M18.6. [SPEC-0323]
14. Failed credentials are limited per address: after 20 failures in 60 seconds further connections
    from that address are closed for 60 seconds. At most 32 connections can be unauthenticated at
    once and at most 64 authenticated. Accepted and refused connections are written to the audit log
    with address and outcome, never with a secret. [ADR-0025 §4, §8]

### Session on top

15. After authentication WebSocket binary messages carry the session protocol's bytes unchanged
    (ADR-0026 §2: frames as on SSH); a message boundary has no meaning. A text message after
    authentication, or a message over 2 MiB, closes the connection. The server pings every 30 seconds
    and closes a connection silent for 90 seconds; reattaching works as on any other channel.
    [ADR-0026]

### Connection check page

16. `GET /` serves a page and `GET /check.js` its script, with the headers of behaviour 10. The page
    asks for a credential, opens the WebSocket on its own origin and shows whether authentication and
    `session.hello` succeeded. It is a diagnostic so a person can verify a real browser, not a web
    client. Every other path answers 404. [GEN]

### Tools

17. MCP tool `web_endpoint_check` (ADR-0008): given a `wss://` URL on a host and a PEM file of a
    certificate authority, it reports the TLS version and group, whether a classical client is
    refused, whether a plain request gets no HTTP answer, whether a foreign origin is refused and
    which security headers are present. It sends no credential, verifies the certificate against the
    file, needs the user-granted `ssh` right and is logged. No MCP tool starts the endpoint or
    creates a credential: switching remote access on is a user action (SPEC-0324 behaviour 4). [ADR-0008]

## Dependencies

| Crate | Purpose | Licence | Note |
|---|---|---|---|
| `rustls` 0.23 (`ring`, no `tls12`) | TLS 1.3 | Apache-2.0 OR ISC OR MIT | already in the tree |
| `tokio-rustls` 0.26 | TLS on the sockets | MIT OR Apache-2.0 | already in the tree |
| `tokio-tungstenite` | WebSocket framing | MIT | new, with `tungstenite` (MIT OR Apache-2.0) |
| `libcrux-ml-kem` `=0.0.11` | ML-KEM-768, FIPS 203 | Apache-2.0 | already in the tree (ADR-0025 §10) |
| `ring` | X25519, randomness | Apache-2.0 AND ISC | already in the tree |
| `httparse` | request head | MIT OR Apache-2.0 | already in the tree |
| `rcgen` | test certificates | MIT OR Apache-2.0 | dev-dependency only |

## Acceptance criteria

- **AK-1** Given a client that offers only classical groups, the handshake fails; given TLS 1.2 only, it fails; given `X25519MLKEM768` over TLS 1.3, it succeeds and the negotiated group is that one.
- **AK-2** The hybrid's secret is agreed between client and server halves, a malformed share is refused, and the ML-KEM half matches a second implementation.
- **AK-3** Given a plain HTTP request on the port, no HTTP answer comes back; the crate's sources contain no `ws://` listener or setting that drops TLS.
- **AK-4** Given a non-loopback `--listen` without `--allow-network`, or with it and no `--wss-origin`, the start is refused; given no `--wss-cert` or a world-readable key, the start is refused; given `--wss` without a folder, the start is refused.
- **AK-5** Given a foreign, `null`, `http:` or missing `Origin`, the upgrade answers 403; given a foreign `Host`, 421; an over-long request head is refused.
- **AK-6** Given every served answer, the security headers of behaviour 10 are present.
- **AK-7** Given no credential within the limit, a wrong credential, or an `organisation` link, the connection closes with 1008 and the same reason, and the backend saw no session; the secret is in no log.
- **AK-8** Given the owner credential on the real `idecraft-serve --wss` process, a session opens over real frames, reads a file and is capped to `--wss-rights`: without `edit` a `doc.edit` is `not_granted`.
- **AK-9** Given a room and an `anonymous` link, a guest connects, must `room.join`, is a viewer, cannot edit; once the link is revoked, new connections are refused and the session ends within a second.
- **AK-10** Given more failures than the limit from one address, that address is refused even with a good credential until the block passes.
- **AK-11** A text message after authentication closes the connection; a session frame split over two WebSocket messages arrives whole.
- **AK-12** `web_endpoint_check` reports the real endpoint's facts without a credential; without the `ssh` right it is refused.
- **AK-13** The method surface of the session protocol is unchanged.

## Out of scope (left for later slices)

The web client and its assets beyond the check page (M18.3); a pixel or typing measurement in a
browser (the rendering spike, ADR-0028 proposal); OIDC and passkeys (M18.6) so "organisation" links
do not yet work in a browser; an automatic certificate (ACME or a local authority); reverse proxies
and `X-Forwarded-*` (M18.5); the owner credential surviving a restart; a window or status UI.

## Known deviations from the reference

None: the reference has no such endpoint in scope here. Compared to the web IDE market the
deviations are recorded in `REMOTE_AND_MOBILE.md` section M18: no token-less mode and no plaintext.
