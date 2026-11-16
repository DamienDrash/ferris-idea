# SPEC-0016: TLS for PostgreSQL and MySQL sessions

- **Version:** 2 · **Date:** 2026-11-07 (v2: `verify-ca`, CA and client certificate files, TLS 1.2 at
  least, `verify-full` for new connections, private key files checked, TLS through an SSH tunnel —
  OBS-0026 L23; v1 2026-10-31)
- **Sources:** STD (libpq `sslmode` semantics: `disable`, `prefer`, `require`, `verify-ca`,
  `verify-full`, `sslrootcert`, `sslcert`, `sslkey` and its refusal of key files others can read; MySQL
  `--ssl-mode` semantics: `DISABLED`, `PREFERRED`, `REQUIRED`, `VERIFY_CA`, `VERIFY_IDENTITY`, `--ssl-ca`;
  PKCS#8, PKCS#12), GEN, ADR-0011, ADR-0025, SPEC-0008 (SSL modes in profiles), SPEC-0009/0010
  (sessions), SPEC-0108 v4 (dialog), OBS-0026 (the reference's SSH/SSL tab with CA, client certificate
  and key files), IMP-0370, IMP-0374
- **Feature matrix rows:** F-051

## Purpose
Encrypt connections to PostgreSQL and MySQL as the profile's SSL mode says, with certificate checks
against the operating system's trust store or the profile's own CA file, and never fall back silently
to less than requested.

## Behaviour
1. TLS uses the operating system's TLS implementation and trusted certificates (macOS Security framework,
   Windows SChannel, OpenSSL on Linux). [ADR-0011]
2. `disable`: no TLS. [STD]
3. `prefer`: TLS when the server offers it, without certificate checks; plain connection otherwise. [STD]
4. `require`: TLS or no connection; the certificate is not checked (encryption against passive listeners
   only). [STD]
5. `verify-full`: TLS or no connection; the certificate must chain to a trusted root and match the host
   name. [STD]
6. When `require`, `verify-ca` or `verify-full` cannot be met, the error says which mode failed and why
   (server without TLS, untrusted certificate, wrong host name); there is no retry with a weaker mode.
   [GEN]
7. A session reports whether it is encrypted, so tools can show it. [GEN]
8. **v2 – Secure default (IMP-0370):** a new server connection — in the dialog and through MCP — starts
   at `verify-full`. Profiles already saved keep their mode; a URL without an SSL parameter still means
   `prefer`, as the standards say (SPEC-0008). When a connection to a server on this machine fails
   because it has no TLS, the error adds that `disable` is the user's choice for such a server; the mode
   is never lowered on its own. [GEN, STD]
9. **v2 – Protocol:** never below TLS 1.2, in every mode that uses TLS. [GEN]
10. **v2 – `verify-ca` and files:** `verify-ca` (MySQL `VERIFY_CA`) checks the chain, not the host name.
    A CA file (PEM, one or more certificates) replaces the system's trusted roots; it is checked only in
    `verify-ca` and `verify-full`, so a profile with a CA file and a weaker mode is refused. A client
    certificate is a PEM file with its PKCS#8 key file (PostgreSQL) or a PKCS#12 bundle (`.p12`, `.pfx`)
    without a password (PostgreSQL, MySQL, MariaDB; MySQL's driver takes bundles only). Paths may start
    with `~`. A file that cannot be read or parsed fails the connection with the file named, before
    anything is sent. SQL Server checks `verify-ca` as fully as `verify-full`; certificate files exist for
    the PostgreSQL and MySQL families only. [STD]
11. **v2 – Private files (IMP-0374):** a key file or PKCS#12 bundle that its group or others can read is
    refused with its path and mode ("mode 0644; restrict it with chmod 600"); a key protected by a
    passphrase is refused, since IDECraft never asks for or keeps one. The error never contains the
    file's content. [STD, GEN]
12. **v2 – Through an SSH tunnel** (SPEC-0108 v4 behaviour 8): the socket goes to the tunnel's local end,
    and certificate and host name are checked against the profile's host, not `127.0.0.1`. [GEN]

## Acceptance criteria
Against the reference databases (local PostgreSQL without TLS; local MySQL 8.4 with its self-signed
certificate):
- **AK-1** PostgreSQL `prefer` connects without TLS and reports "not encrypted".
- **AK-2** PostgreSQL `require` fails with an error naming `require` and that the server does not support TLS.
- **AK-3** MySQL `require` connects and reports "encrypted".
- **AK-4** MySQL `verify-full` fails with an error naming `verify-full` (self-signed certificate).
- **AK-5** MySQL `prefer` connects and reports "encrypted"; `disable` connects and reports "not encrypted".
- **AK-6** (v2) A CA file is read and replaces the system roots (`session::tests::ca_files_are_read`);
  `sslmode=verify-ca` and `ssl-mode=VERIFY_CA` in URLs round-trip (`profile::tests`).
- **AK-7** (v2) A key file with mode 0644 or 0640, an encrypted key, a CA file without certificates are
  refused naming the file, never its content (`session::tests::private_files_are_checked`; live
  `tests/tls.rs` `readable_client_keys_are_refused`).
- **AK-8** (v2, live, `IDECRAFT_TEST_MYSQL_CA` = the server's own CA): `verify-ca` with it connects
  encrypted (on macOS the system refuses MySQL's ten-year certificate, which the test accepts as the
  only other outcome); `verify-full` with it fails on the host name; `verify-ca` with a CA that did not
  sign the certificate fails — no fallback (`tests/tls.rs` `mysql_ca_files_are_checked`).
- **AK-9** (v2) New forms start at `verify-full`, a local server without TLS gets the hint
  (`connection_form::tests::secure_default_and_url_field`).

## Out of scope
Certificate revocation lists, passphrase-protected keys, OS keychain identities, PEM client
certificates for MySQL (bundle only), server certificate pinning by fingerprint.

## Known deviations from the reference
New connections check certificate and host name by default (IMP-0370). Key files others can read and
encrypted keys are refused instead of used or unlocked with a stored passphrase (IMP-0374). The
reference's SSH/SSL tab was only seen (OBS-0026 item 12), not compared mode by mode.
