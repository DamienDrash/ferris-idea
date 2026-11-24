# SPEC-0326: Mobile core, iOS and Android clients (first slice of M17.2)

- **Version:** 3 · **Date:** 2026-11-10 (v1 2026-11-09; v2: no retry after pairing, behaviour 6,
  because the host now confirms storing the device before the phone pins it, SPEC-0321 v4
  behaviour 17; v3: Android app with a Keystore vault, behaviours 10–13, AK-12 to AK-15;
  `RemoteProject.close` renamed to `disconnect`)
- **Sources:** [`ADR-0029`](../adr/0029-mobile-rust-core-native-ui.md), [`ADR-0025`](../adr/0025-network-security-for-remote-connections.md),
  [`ADR-0026`](../adr/0026-session-protocol-window-and-serve.md), SPEC-0321, SPEC-0324 and the Project
  Owner's order of 2026-11-09 and decision of the same evening (Android only as an APK for now,
  remote mode only) (`GEN`). Apple Keychain and Android Keystore documentation (`DOC`, facts only). No
  observation of the reference is needed: mobile apps are Ferris IDEA's own scope.
- **Feature matrix rows:** M17 (no reference counterpart)

## Purpose

From a phone, pair with a machine running `ferris-idea serve --pair <folder>`, list the project's files
and read one, over the same encrypted channel and session protocol as the desktop window.

## Behaviour

### Core (`crates/mobile`)

1. The address is `host:port`, `host` (port 47100), `[ipv6]:port` or any of these after `paired://`.
   Anything else is refused and named. `…@ssh` is refused with the reason: a phone has no `ssh`.
   [ADR-0029 §5]
2. Pairing takes a code or an `ferris-idea-pair://` link. With a link, the host's key must equal the
   link's before any digits exist (SPEC-0324 behaviour 3). The six digits and the host's fingerprint
   are shown; only the user's confirmation, and then the host's, stores the pin. Declining stores
   nothing. [ADR-0025 §2]
3. An address already paired is refused until it is forgotten; a pin is never replaced in passing.
4. The device's key and the pins go only through `SecretVault`, which the app implements with the
   platform store. A stored key that cannot be read is an error and is not replaced. A vault failure
   stops the operation and names the status, never a value. [ADR-0029 §4, ADR-0025 §3a]
5. A later connection uses the pinned key. Another key is refused and reported as a changed key.
6. A pin is stored only after the host said it stored this device with its rights (SPEC-0321 v4
   behaviour 17), so a connection right after pairing is accepted and a refusal is reported at
   once, also right after pairing. (v1 retried for 30 seconds because the host answered before
   storing.) [GEN]
7. Opening needs the folder the host serves (SPEC-0324 behaviour 2). Listing uses `fs.list`, reading
   `fs.stat` and `fs.read`; paths are checked on both sides. Only regular files open, up to 8 MiB,
   checked before reading. A file that is not UTF-8 reports its size and no text. No method is added
   to the session protocol. Ending the session is `disconnect`, not `close`: in Kotlin `close` is
   `AutoCloseable`'s and frees the object, and the bindings would not compile.
8. The rights are the host's grant; the slice uses only `view`.

### iOS app (`mobile/ios`)

9. SwiftUI: pair, compare digits, open a project, browse folders, read a file. Blocking calls run off
   the main thread. The Keychain entry uses `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` and is not
   synchronized. A debug build only has a self-test driven by `FERRIS_IDEA_SELFTEST_*`, which confirms
   digits by itself and prints them so `scripts/mobile-selftest.py` can compare them with the host's;
   a release build contains none of it.

### Android app (`mobile/android`)

10. Jetpack Compose, the same screens and steps as behaviour 9; a back gesture goes one step back
    (file, folder, project, pairing, which it declines). Core calls run on the IO dispatcher; a call
    that only reads state the session lock guards (the project's root) is read once off the main
    thread. Generated Kotlin bindings (UniFFI) and the arm64 core are compiled into the APK; only
    `arm64-v8a` is built. Minimum Android 9 (API 28).
11. **Vault:** one AES-256 key created in the Android Keystore, GCM only, randomized encryption,
    usable only while the device is unlocked, in StrongBox where the device has it, otherwise the
    Keystore's default. It never leaves the Keystore. Each value is stored as
    `format byte · 12-byte IV · ciphertext and tag` in the app's `noBackupFilesDir`, with the value's
    name as associated data, so a file moved to another name fails. Writes go to a temporary file
    and are renamed. A file whose key is gone or that fails to decrypt is an error and stays on disk
    (behaviour 4). Not chosen: user authentication per use, which a device without a screen lock
    cannot create and which would ask on every connection (the iOS reasoning in ADR-0029 §4).
    [ADR-0029 §4, ADR-0025 §3a]
12. **No backups:** `allowBackup` off, no full-backup content, and data-extraction rules that exclude
    every domain from cloud backup and device transfer. A release build sets `FLAG_SECURE`, so
    project files stay out of screenshots and the recent-apps preview; a debug build allows
    screenshots for the self-test. No cleartext traffic. Only the `INTERNET` permission.
13. **Builds:** `scripts/build-mobile.sh android-apk` builds a debug APK and an unsigned, minified
    release APK. A debug APK is signed with a throwaway key made per build in a private temp folder
    and deleted at exit, never with the shared `~/.android/debug.keystore`; the same key signs a
    copy of the release APK only so that an emulator can install it. No key enters the repository.
    The debug build's self-test reads its inputs from intent extras and logs `SELFTEST` lines;
    `scripts/mobile-selftest.py --android` reaches the host's loopback listener at `10.0.2.2`. The
    release source set has a stub without any self-test code.

## Not in this slice

Reconnection after the app was suspended, editing, search, QR camera, a Play Store build and its
signing key (the project owner's account, GO-4), an x86_64 or 32-bit Android build,
a size limit enforced while a stream arrives (a compromised host could send more than it announced;
denial of service only), a way to learn the served folder without typing it.

## Acceptance criteria

- **AK-1** Pair by code, same digits on both sides, list, read a nested and a top-level file with `view` only.
- **AK-2** Key and pins are only in the vault; nothing passed to the interface carries a key.
- **AK-3** The key is reused; a damaged key is reported and left in place.
- **AK-4** Declining on either side stores nothing.
- **AK-5** A host with another key at a pinned address is refused as a changed key, at once and
  also right after pairing (v2).
- **AK-6** A link with another key is refused before any digits; a right link pairs.
- **AK-7** Paths outside the project, folders, oversized files are refused; a streamed and a binary file read correctly.
- **AK-8** A failing vault stops pairing and names its status.
- **AK-9** Double pairing is refused, forgetting works, an unpaired host is named.
- **AK-10** The debug app in an iOS simulator pairs with a real `ferris-idea-serve`, the digits match, the
  Keychain items report accessibility `aku`, and it lists and reads a file.
- **AK-11** A release build contains no self-test.
- **AK-12** The Kotlin bindings compile; the debug and the release APK build for `arm64-v8a`.
- **AK-13** The debug app in an Android emulator pairs with a real `ferris-idea-serve`, the digits match,
  and it lists and reads a file.
- **AK-14** After pairing, the Keystore key is AES-256-GCM, not exportable, the vault files hold no
  plaintext of their values, and the app does not allow backups.
- **AK-15** The release APK contains no self-test and starts in an emulator.
