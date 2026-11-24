# ADR-0029: Mobile apps: a Rust core with native interfaces

- **Status:** accepted · **Version:** 1 · **Date:** 2026-11-09 · decided by the implementing agent
  under the Project Owner's order of 2026-11-09 to implement M17 before QG-1 and decide unclear
  points (PO-STATUS; exception to [ADR-0015](0015-quality-gate-ai-native.md))
- **Related:** [ADR-0025](0025-network-security-for-remote-connections.md) (network security),
  [ADR-0026](0026-session-protocol-window-and-serve.md) (session protocol),
  [SPEC-0321](../specifications/SPEC-0321-every-host.md), [SPEC-0324](../specifications/SPEC-0324-remote-control.md),
  [SPEC-0326](../specifications/SPEC-0326-mobile-core-and-ios-client.md) (first slice),
  [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M17

## Context

M17 needs iOS and Android apps. GPUI has no mobile target. The channel, the session protocol and
the rights already exist in Rust and carry a post-quantum hybrid handshake that must not be written
twice in two more languages.

## Decision

1. **One Rust crate is the shared core** (`crates/mobile`, `ferris-idea-mobile`). It reuses
   `ferris-idea-pairing` and `ferris-idea-session` unchanged in protocol. A phone is a window-side client:
   same handshake, same pins, same rights. No session method was added for it.
2. **The interface is native:** SwiftUI on iOS, Jetpack Compose on Android. Design tokens and
   behaviour follow ADR-0009 where they fit touch.
3. **Bindings: UniFFI 0.32** (Mozilla, MPL-2.0, on the allow list; `cargo deny check licenses bans
   sources` passes). One definition gives Swift and Kotlin, including a foreign trait for the
   keychain. Compared: hand-written C ABI with Swift and JNI wrappers (two more unsafe surfaces to
   keep correct) and `swift-bridge` (Swift only). The workspace's `forbid(unsafe_code)` cannot hold
   for generated scaffolding, so this crate relaxes that lint alone, and a test
   (`no_handwritten_unsafe_code`) keeps its own sources free of `unsafe`.
4. **Keys stay in the platform store, through a trait.** The core never writes a key or pin to a
   file; the app implements `SecretVault`.
   - iOS: Keychain, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`, not synchronizable. Readable only
     while unlocked, never in backups or on another device. `WhenPasscodeSetThisDeviceOnly` would be
     stricter but refuses to store anything on a device without a passcode, and silently drops every
     pairing when the passcode is removed; rejected for a tool whose pairing is the only way in.
     Requiring biometrics per use is a possible later option, not in this slice.
   - Android: an AES key held in the Android Keystore wrapping the values in app-private storage,
     excluded from backups. Built in SPEC-0326 v3 (behaviours 11 and 12): AES-256-GCM, usable only
     while the device is unlocked, StrongBox where present, no user authentication per use for the
     same reason as on iOS.
5. **Paths:** direct TCP to a listener started with `--allow-network`, or a VPN address. A phone has
   no `ssh`, so `@ssh` hosts are refused and named. A relay is a later step of M16.
6. **Android links OpenSSL** only because `ferris-idea-session` depends on `ferris-idea-core`, which depends
   on `ferris-idea-http` and `native-tls`. It is unused at run time by this core. Splitting the
   session's client half from `ferris-idea-core` removes it and is the better end state; left for later
   because it touches the backend.

## App store rules for running code locally

Checked on **2026-11-09**. Sources were read through a summarizing web tool, so these are paraphrases;
re-read the original text before M17.3 or any submission.

| Rule | What it says | Source, date read |
|---|---|---|
| Apple 2.5.2 | Apps are self-contained and may not download, install or execute code that adds or changes features. Narrow exception: apps designed to teach, develop or let students test executable code may in limited circumstances download code, not for other purposes, with the source completely viewable and editable by the user | [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/), 2026-11-09 |
| Apple 4.7 | HTML5 and JavaScript mini apps, streaming games, chatbots and plug-ins not embedded in the binary may be offered, under the developer's responsibility, without exposing native APIs without permission | same |
| Apple 4.2.7 | Remote desktop clients that mirror specific software: only a user-owned PC or console on the local network, everything executed and rendered on the host; thin clients for cloud apps are not appropriate | same |
| Apple 2.5.4 | Background services only for their intended purposes (VoIP, audio, location, task completion, local notifications and similar) | same |
| Google Play, Device and Network Abuse | No executable code (dex, JAR, `.so`) downloaded from outside Google Play. The rule does not apply to code running in a virtual machine or interpreter with limited access to Android APIs, such as JavaScript in a WebView; interpreted code loaded at run time must still follow all Play policies | [Play policy page](https://support.google.com/googleplay/android-developer/answer/9888379), via search result, 2026-11-09; [The Register, 2021](https://www.theregister.com/2021/07/29/google_play_python_javascript/) for the interpreter wording |

Not verified by these sources, stated from general knowledge (`GEN`): iOS gives apps no child
processes and no JIT without special entitlements; Android apps targeting API 29 or later cannot
execute files from their writable home directory. No source was found on whether a Termux-style
environment fits Play's interpreter exception.

**What follows.**

- **Remote mode (M17.2) runs nothing on the phone**, so 2.5.2 does not apply to it. The open point is
  4.2.7: the app is a client of a structured protocol, not a mirrored screen, so it should fall
  outside "remote desktop", but App Review decides that. A server backend reached over the internet
  would be the case nearest to the "thin client" wording. **Not settled by this ADR.**
- **Local mode (M17.3) on iOS:** an interpreter compiled into the app that runs only code the user
  typed or opened, viewable and editable, is the defensible shape. Downloading interpreters or
  packages that extend the app is not. Whether a general development tool fits the "teach, develop"
  exception is uncertain; the plan's assumption is **not confirmed**, and running code on iOS
  stays conditional on a new check before M17.3 starts.
- **Local mode on Android:** interpreters ship inside the app bundle; none is downloaded at run time.

## Consequences

- The core is tested on the desktop against a real listener, and the same tests run on Android.
- The apps need a build step for the core: `scripts/build-mobile.sh`.
- Accounts, signing and distribution stay with the Project Owner (GO-4, M17.4).
