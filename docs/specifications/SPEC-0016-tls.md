# SPEC-0016: TLS for PostgreSQL and MySQL sessions

- **Version:** 5 · **Date:** 2026-11-17 (v5: a certificate the check refused gets a hint about the certificate
  and the CA file, never the hint for a server without TLS and never a weaker mode, behaviour 15,
  AK-16, IMP-0497 — OBS-0076 C1); 2026-11-16 (v4: a CA file is the only trust anchor for SQL Server too —
  the chain the server shows must lead to a certificate of the file before any login data leaves;
  connector plugins get a connection without TLS only where the profile's mode allows plain text,
  elsewhere TLS that IDECraft makes for them — behaviours 10, 14, AK-14, AK-15, IMP-0426, IMP-0427);
  2026-11-16 (v3: `prefer` falls back to a plain connection only for a
  server on the loopback; on any other host it needs TLS like `require`, and the error says how to give
  the server TLS; the same rule for SQL Server and Oracle; SQL Server takes a CA file — IMP-0421,
  finding of the F-060 run of PR #869); 2026-11-07 (v2: `verify-ca`, CA and client certificate files, TLS 1.2 at
  least, `verify-full` for new connections, private key files checked, TLS through an SSH tunnel —
  OBS-0026 L23; v1 2026-10-31)
- **Sources:** STD (libpq `sslmode` semantics: `disable`, `prefer`, `require`, `verify-ca`,
  `verify-full`, `sslrootcert`, `sslcert`, `sslkey` and its refusal of key files others can read; MySQL
  `--ssl-mode` semantics: `DISABLED`, `PREFERRED`, `REQUIRED`, `VERIFY_CA`, `VERIFY_IDENTITY`, `--ssl-ca`;
  PKCS#8, PKCS#12; v3: TDS pre-login; how PostgreSQL, MySQL, MariaDB, SQL Server and Oracle servers are
  given TLS, from their vendors' documentation in own words; v4: X.509 certification paths, RFC 5280;
  the TLS 1.2 handshake, RFC 5246), GEN, ADR-0011, ADR-0025, SPEC-0008 (SSL modes in profiles), SPEC-0009/0010
  (sessions), SPEC-0108 v4 (dialog), OBS-0026 (the reference's SSH/SSL tab with CA, client certificate
  and key files), IMP-0370, IMP-0374, IMP-0421, IMP-0426, IMP-0427, SPEC-0116 v7 (SQL Server),
  SPEC-0142 v3 (Oracle), SPEC-0089 v12 (Database tool window), SPEC-0314 v2 (connector plugins)
- **Feature matrix rows:** F-051, F-060

## Purpose
Encrypt connections to PostgreSQL, MySQL, MariaDB, SQL Server and Oracle as the profile's SSL mode says, with certificate checks
against the operating system's trust store or the profile's own CA file, and never fall back silently
to less than requested.

## Behaviour
1. TLS uses the operating system's TLS implementation and trusted certificates (macOS Security framework,
   Windows SChannel, OpenSSL on Linux). [ADR-0011]
2. `disable`: no TLS. [STD]
3. `prefer`: TLS when the server offers it, without certificate checks; plain connection otherwise. [STD]
   **v3 – only on the loopback (IMP-0421):** a plain connection is allowed only for a server reached
   through the loopback — host `localhost` (any case), an address in 127.0.0.0/8, `::1` (also written
   `[::1]`), or a PostgreSQL socket folder (a host starting with `/`) — and only once, after TLS itself
   failed (the server offers none, or the handshake broke); a wrong password or a closed port is not
   tried again without TLS. The session then reports "not encrypted": Test Connection says "TLS: no"
   (SPEC-0108 v3), the connection's row in the Database tool window too (SPEC-0089 v12). On any other
   host `prefer` acts as `require` (behaviour 4): TLS or no connection. Through an SSH tunnel the host
   that counts is the profile's host, where the database is as the SSH server sees it (behaviour 12):
   `localhost` there stays on that machine, any other host crosses a network. This holds for
   PostgreSQL, MySQL, MariaDB, SQL Server (SPEC-0116 v6) and Oracle (SPEC-0142 v3). Up to v2 `prefer`
   fell back to plain text on every host without a word. [STD, GEN, rule 12]
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
    the PostgreSQL and MySQL families only. **v3:** SQL Server takes a CA file too (PEM, no client
    certificate), checked in `verify-ca` and `verify-full`; its driver trusts the file's certificates
    in addition to the system's roots and cannot replace them (SPEC-0116 v6 behaviour 15). **v4 – The
    CA file is the only anchor for SQL Server too (IMP-0426):** besides the driver's check of the
    certificate against the file and the system's roots together, IDECraft checks the chain the
    server showed against the file alone: it must lead to a certificate of the file, or be that
    certificate. Otherwise the connection ends before any login data is sent, with an error naming
    the file. Up to v3 a certificate for the host name from any authority the system trusts was
    accepted although the profile named a private CA (SPEC-0116 v7 behaviour 15). [STD, rule 12]
11. **v2 – Private files (IMP-0374):** a key file or PKCS#12 bundle that its group or others can read is
    refused with its path and mode ("mode 0644; restrict it with chmod 600"); a key protected by a
    passphrase is refused, since IDECraft never asks for or keeps one. The error never contains the
    file's content. [STD, GEN]
12. **v2 – Through an SSH tunnel** (SPEC-0108 v4 behaviour 8): the socket goes to the tunnel's local end,
    and certificate and host name are checked against the profile's host, not `127.0.0.1`. [GEN]
13. **v3 – A server off the loopback without TLS:** when `prefer` acts as `require` (behaviour 3) and
    no TLS comes about, there is no connection. The error names the mode `prefer`, that the server is
    not on this machine and so needs TLS, the reason (the server offers no TLS, or the handshake failed,
    with the driver's words) and how to give that server TLS:
    - PostgreSQL: `ssl = on` with a certificate and its key (`ssl_cert_file`, `ssl_key_file`) in
      `postgresql.conf`;
    - MySQL and MariaDB: a certificate and key for the server (`ssl_cert`, `ssl_key` in the server's
      option file; MySQL 8 and MariaDB 11.4 or newer make their own at the first start);
    - SQL Server: a certificate for the instance (`mssql-conf set network.tlscert` and
      `network.tlskey` on Linux, the SQL Server Configuration Manager on Windows);
    - Oracle: a TCPS listener with a wallet, and the TCPS port in the profile.
    It never proposes a weaker mode; `disable` stays the user's own explicit choice. [GEN, DOC]
14. **v4 – Connector plugins (IMP-0427):** a database system reached through a WebAssembly plugin
    (SPEC-0314) follows the same modes. The plugin gets a connection without TLS only where the
    profile's mode allows plain text — `disable` on any host, `prefer` for a server on the loopback
    (behaviour 3); everywhere else it gets only a connection whose TLS IDECraft makes, at TLS 1.2 or
    newer, with the checks of the mode and the profile's CA file instead of the system's roots
    (SPEC-0314 v2 behaviour 10). IDECraft cannot see what a plugin sends, so a connection without
    TLS is refused before anything is resolved, not watched. [GEN, rule 12]
15. **v5 – A refused certificate (OBS-0076 C1, IMP-0497):** when `verify-ca` or `verify-full` fails
    because the system does not accept the server's certificate (an unknown authority, a name that
    does not match, an expired certificate, a validity longer than the system allows — the driver's
    message speaks of the certificate, its validity or trust), the server does have TLS. The error
    then keeps the driver's words and adds, on any host, that the certificate of the authority
    that signed it can be named as CA file on the SSH/SSL page, with `verify-ca` when the
    certificate was made for another host name, and that this system refuses certificates valid for
    many years, such as the one a MySQL server makes for itself, so such a server needs one valid for
    a shorter time. It never proposes `disable`, `prefer` or `require`. The hint of behaviour 8 (a
    server on this machine without TLS) is given only when the failure is not about the
    certificate. Up to v4 a server on this machine whose certificate was refused got the hint of
    behaviour 8: that it might have no TLS and that `disable` was the user's choice. [GEN, rule 12]

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
- **AK-10** (v3, behaviour 3) What a profile's mode means on its host, for every server dialect:
  `localhost`, `LocalHost`, `127.0.0.1`, `127.1.2.3`, `::1`, `[::1]`, no host and a socket folder allow
  the one plain retry under `prefer`; `db.example`, `10.0.0.5`, `192.168.1.20`, `localhost.example`,
  `::ffff:10.0.0.5` and `0.0.0.0` make `prefer` act as `require`; `disable`, `require`, `verify-ca`
  and `verify-full` mean the same on every host (`session::tests::prefer_needs_tls_off_the_loopback`).
  Oracle's connect string for `prefer` is `tcps://` off the loopback and `//` on it
  (`oracle::tests::prefer_off_the_loopback_is_tcps`).
- **AK-11** (v3, behaviour 13) The hint per dialect names its server setting and no weaker mode
  (`session::tests::the_hint_says_how_to_give_the_server_tls`).
- **AK-12** (v3, live, `IDECRAFT_TEST_PG` without TLS) `prefer` with the profile host `db.example.invalid`,
  the socket going to the reference PostgreSQL as through a tunnel, fails with mode `prefer`, the
  reason and `ssl = on`; with host `localhost` it connects and reports "not encrypted"
  (`tests/tls.rs` `postgres_prefer_off_the_loopback_needs_tls`).
- **AK-13** (v3, live, `IDECRAFT_TEST_SQLSERVER`; SPEC-0116 v6 AK-10) On macOS `require` and
  `prefer` connect encrypted, `disable` does not; `verify-full` without a CA file fails on the
  certificate; with the test CA of `mssql.sh tls` (`IDECRAFT_TEST_SQLSERVER_CA`) it connects to
  `localhost` and fails on the host name for `127.0.0.1` (`tests/tls.rs` `sql_server_modes`).
- **AK-14** (v4, behaviour 10; SPEC-0116 v7 AK-11) Unit: a chain that leads to a certificate of the
  CA file passes, also the file's own server certificate; a chain from another authority is refused,
  also when that authority would be trusted elsewhere; the server's Certificate message is read from
  the pre-login packets of its handshake flight, and the first login bytes are held back when the
  chain does not lead to the file or was not shown (`tds_tls::tests`). Live (SQL Server of the
  reference environment, `IDECRAFT_TEST_SQLSERVER_CA`): `verify-full` with the test CA connects to
  `localhost`; with a CA file that did not sign the server's certificate it fails naming the file
  (`tests/tls.rs` `sql_server_modes`).
- **AK-15** (v4, behaviour 14; SPEC-0314 v2 AK-6) A plugin's connection without TLS is refused for
  `require`, `verify-ca`, `verify-full`, and for `prefer` off the loopback; allowed for `disable` and
  for `prefer` on the loopback; TLS made by IDECraft connects with `require`, and with `verify-full`
  only with the CA file that signed the server's certificate (`network::tests`).
- **AK-16** (v5, behaviour 15) `connection_form::tests::a_refused_certificate_points_to_the_ca_file`:
  MySQL's refusal of its own ten-year certificate under `verify-full` gets the CA-file hint on the
  loopback and on another host, without `disable`, `prefer` or `require`; under `require` the error
  stays as it is. Live headless against the reference MySQL 8.4 (OBS-0076 C1): the same text.

## Out of scope
Certificate revocation lists, passphrase-protected keys, OS keychain identities, PEM client
certificates for MySQL (bundle only), server certificate pinning by fingerprint, client certificates
for SQL Server and Oracle, a CA file for Oracle (its wallet holds the trusted certificates).

## Known deviations from the reference
New connections check certificate and host name by default (IMP-0370). Key files others can read and
encrypted keys are refused instead of used or unlocked with a stored passphrase (IMP-0374). The
reference's SSH/SSL tab was only seen (OBS-0026 item 12), not compared mode by mode. v3: `prefer`
reaches a plain connection only on the loopback, elsewhere it needs TLS (IMP-0421); how the reference
treats `prefer` was not recorded. v4: a SQL Server CA file is the only trust anchor (IMP-0426), and
connector plugins get TLS from IDECraft under the same modes (IMP-0427); the reference has no
sandboxed connector plugins. v5: a certificate the check refused gets a hint about the CA file and never
a weaker mode (IMP-0497); the reference's message for it was not recorded.
