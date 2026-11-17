# SPEC-0282: macOS app bundle and signed download

- **Version:** 3 · **Date:** 2026-11-17 (v3: signing with an identity and the hardened runtime, the notarized and stapled DMG of a release, behaviours 4, 4a, 9–15, AK-4–AK-6; the project owner's decision of 2026-11-17 to publish 1.0 with a signed and notarized macOS download); 2026-11-15 (v2: a file opens its project and the file, SPEC-0094 v7 behaviours 18 and 19); 2026-11-04 (v1)
- **Sources:** STD (Apple bundle layout `Contents/{Info.plist,MacOS,Resources}`, Info.plist keys
  `CFBundleIdentifier`, `CFBundleExecutable`, `CFBundleIconFile`, `CFBundleShortVersionString`,
  `CFBundleVersion`, `LSMinimumSystemVersion`, `NSHighResolutionCapable`, privacy usage
  descriptions; `codesign` ad-hoc signatures; freedesktop.org Desktop Entry specification;
  v3: Apple's code signing with Developer ID, the hardened runtime and its resource-access
  entitlements, secure timestamps, `xcrun notarytool`, `xcrun stapler`, `spctl`, `hdiutil`);
  GEN (an IDE started from the Dock or Finder has no command line, so it needs a starting
  folder); ADR-0018 (app icon), SPEC-0060 v6 behaviour 12, SPEC-0094, SPEC-0124, SPEC-0278
- **Feature matrix rows:** F-001

## Purpose
IDECraft can be started like any Mac application — from Finder, the Dock or Spotlight — from a
bundle built locally out of the same binary. Building it needs no Apple developer account and
publishes nothing. For a release (v3), the same bundle is signed with the project's Developer ID,
notarized by Apple and packed into a DMG that opens on any Mac without a Gatekeeper warning, also
offline. Building the download uploads nothing to the release page; publishing it is a separate
step that needs the project owner's approval.

## Behaviour

### Building
1. **`scripts/bundle-macos.sh`** builds `IDECraft.app` from the release binary
   (`cargo build --release -p idecraft`); `--debug` takes the debug binary instead (for the quick
   check in `scripts/ci-local.sh`), `--no-build` takes the binary that is already built, and
   `--out <folder>` chooses where the bundle goes (default `target/bundle/macos`, honouring
   `CARGO_TARGET_DIR`). It runs on macOS only and replaces an older bundle at the same place.
2. **Contents.** `Contents/MacOS/idecraft` (the binary), `Contents/Resources/IDECraft.icns`
   (unchanged from `packaging/icons/macos/`), `Contents/Info.plist` and `Contents/PkgInfo`.
3. **Info.plist.** Identifier `dev.idecraft.IDECraft` and name `IDECraft` as in the plist embedded
   in the binary (SPEC-0124); executable `idecraft`; icon file `IDECraft`; package type `APPL`;
   `CFBundleShortVersionString` and `CFBundleVersion` = the version of the `idecraft` package in
   Cargo; `LSMinimumSystemVersion` = the minimum macOS the binary was linked for (read from the
   binary, not guessed); `NSHighResolutionCapable` true; category developer tools; usage
   descriptions for what IDECraft and the programs it starts (terminal, run configurations,
   debuggers) may ask macOS for: Desktop, Documents and Downloads folders, removable and network
   volumes, the local network (databases, containers, remote hosts), controlling other apps, the
   microphone and the camera. Each says in plain words why.
4. **Signature.** Without `IDECRAFT_SIGN_IDENTITY` the bundle is signed ad hoc
   (`codesign --force --sign -`), so macOS accepts it on the machine that built it, and the script
   says that this bundle is not for distribution. There is no default identity: an identity is
   only used when it is named. [ADR-0007] (v3: the earlier reason against the hardened runtime,
   plugins compiling WebAssembly at run time, no longer holds: plugins run in an interpreter.)
4a. **Signed with an identity** (v3). With `IDECRAFT_SIGN_IDENTITY` (the name or SHA-1 of a
   signing identity in the keychain) the script signs inside-out: every Mach-O file inside the
   bundle other than the main executable, deepest path first, then the bundle (and with it the
   main executable). Every signature uses the hardened runtime (`--options runtime`) and a secure
   timestamp (`--timestamp`). The bundle gets `packaging/macos/IDECraft.entitlements`, which holds
   only three resource-access entitlements and no code-signing exception:
   - Apple Events, microphone and camera — each matches a usage description of behaviour 3. The
     hardened runtime refuses these resources before macOS asks, and programs started in the
     terminal or a run configuration count as IDECraft for the system's privacy checks; without
     the entitlements such a program would fail silently. macOS still asks the user once each.
   - Not granted: JIT and unsigned executable memory (plugins run in an interpreter), disabled
     library validation (the binary links only system libraries and loads no code at run time),
     DYLD environment variables, debugging of IDECraft by other processes. Adding one needs a
     reason in this spec and the project owner's decision (security first).
   After signing, the script also fails unless the signature carries the runtime flag and a
   timestamp, `codesign --verify --deep --strict` accepts the bundle and the three entitlements are
   in the signature. The identity is never printed; the script names only its kind (the part
   before the colon, e.g. "Developer ID Application"), and codesign's messages are shown with the
   identity, a team ID in parentheses and certificate hashes replaced.
5. **Checks.** After building, the script fails unless `plutil -lint` accepts the Info.plist,
   `codesign --verify --strict` accepts the bundle, the icon equals the packaged file, and
   `Contents/MacOS/idecraft --version` prints `idecraft <version>` with the plist's version. It
   opens no window. With an identity, `--version` is also the check that the binary starts under
   the hardened runtime.

### Starting
6. **`idecraft launch [folder]`** opens the folder in a project window; without a folder it opens
   the most recent project that still exists (SPEC-0094) and, when there is none, asks for a
   folder with the system's folder chooser; cancelling quits. [GEN]
7. **From the bundle.** Started from the bundle without arguments (Finder, Dock, Spotlight,
   `open -a`), the binary behaves as `idecraft launch`. Arguments macOS adds when starting an
   application (`-psn_…`) are ignored. All other commands work unchanged from
   `IDECraft.app/Contents/MacOS/idecraft`. The Dock shows the IDECraft icon, the application menu
   is named IDECraft and About shows the version. [STD]
8. **Linux desktop entry.** `packaging/icons/linux/idecraft.desktop` starts `idecraft launch %f`,
   so a launcher click opens the most recent project and a folder dropped on it opens that
   folder (v2: a file opens its project, SPEC-0094 v7 behaviour 18, and the file in an editor). The README of `packaging/icons` says where to install it and the icons. [STD]

### Release download (v3)
9. **`scripts/release-macos.sh`** builds the download of the commit it is run on. It refuses to
   start when the working tree has uncommitted or untracked changes, and prints the commit, the
   version of the `idecraft` package and whether the tag `v<version>` is on that commit (a missing
   tag is a warning). `--check` stops after the preconditions (behaviours 9–11) and builds, signs
   and sends nothing; `--out <folder>` chooses where the DMG goes (default
   `target/release-macos`, honouring `CARGO_TARGET_DIR`); only its own working folder there is
   replaced.
10. **Identity.** `IDECRAFT_SIGN_IDENTITY` must name exactly one valid signing identity of the kind
   "Developer ID Application" (by full name or SHA-1); any other kind (development, App Store
   distribution) or no match is refused with the kind it found. It then runs the release build and
   `scripts/bundle-macos.sh` with that identity (behaviour 4a).
11. **Notary credentials**, read from the environment at run time and never written or printed:
   either `IDECRAFT_NOTARY_PROFILE` (a profile stored in the keychain with
   `xcrun notarytool store-credentials`) or all three of `IDECRAFT_NOTARY_KEY` (path of an App
   Store Connect API key file), `IDECRAFT_NOTARY_KEY_ID` and `IDECRAFT_NOTARY_ISSUER`. Both kinds
   at once, an incomplete trio, a missing key file or a key file readable by anyone but its owner
   are refused.
12. **Notarized app.** The signed app is zipped (`ditto -c -k --keepParent`), submitted with
   `xcrun notarytool submit --wait`, and stapled when the answer is "Accepted"; any other answer
   stops the script with the submission ID and the command that shows Apple's log. A stapled app
   still opens offline after it was copied out of the DMG.
13. **DMG.** The download is a DMG — chosen over a zip because a DMG can carry its own signature
   and stapled ticket, so the first open is checked offline too, and it offers the usual drag to
   Applications — named `IDECraft-<version>-macos-<arch>.dmg` (`arm64`, `x86_64` or `universal`,
   from the binary), compressed (UDZO, HFS+), volume name `IDECraft <version>`, holding the
   stapled app and a link to `/Applications`. The DMG is signed with the same identity and a
   timestamp, notarized and stapled like the app.
14. **Checks.** The script fails unless: `codesign --verify --deep --strict` accepts the app,
   `spctl --assess --type execute` accepts it and `xcrun stapler validate` finds its ticket;
   `codesign --verify --strict` accepts the DMG, `spctl --assess --type open --context
   context:primary-signature` accepts it and its ticket validates; and the app inside the mounted
   DMG (read-only, not shown in Finder) passes the same three app checks and its `--version`
   prints the release version. It also fails when `HEAD` moved during the run. At the end it
   writes `<dmg>.sha256` and prints the commit, the version, the DMG's path and its SHA-256.
15. **Nothing is published.** The script talks only to Apple's notary and timestamp services; it
   never creates a release, uploads to GitHub or anywhere else, or pushes a tag. Publishing the
   DMG and its checksum is a separate step that needs the project owner's approval.

## Acceptance criteria
- **AK-1** Bundle build and checks with the debug binary in `scripts/ci-local.sh` (macOS); the
  release build with `scripts/bundle-macos.sh`.
- **AK-2** Which folder `launch` opens, and when a binary counts as started from a bundle
  (`launch::tests` in `crates/app`).
- **AK-3** Hand test (round 5): start `IDECraft.app` from Finder; Dock icon, the most recent
  project opens, About shows the version, ⌘Q quits.
- **AK-4** (v3) Signing with an identity: `IDECRAFT_SIGN_IDENTITY=<identity>
  scripts/bundle-macos.sh` signs with runtime flag, timestamp and the three entitlements, and the
  bundled binary starts under the hardened runtime (`--version`, `ui --selftest --list-groups`,
  plugins loaded and run through `idecraft call`). Checked locally with a development identity;
  that bundle is not distributed.
- **AK-5** (v3) `scripts/release-macos.sh` refuses a dirty tree, a missing, unknown or
  non-Developer-ID identity and each wrong combination of notary credentials (behaviours 9–11),
  and its packaging steps (bundle, zip, DMG, DMG signature, mount, checks of the mounted app,
  SHA-256) run end to end with notarization and Gatekeeper stubbed out; `spctl` rejects the
  development-signed results, which shows the checks of behaviour 14 are not empty.
- **AK-6** (v3) First release run by the project owner with his Developer ID and notary
  credentials: the script ends with the SHA-256, and the DMG downloaded on another Mac opens
  without a Gatekeeper warning.

## Out of scope
An installer package, automatic updates, uploading the release (a separate, approved step); a
universal binary (the DMG holds the architecture it was built on); opening folders dropped on the
Dock icon while IDECraft runs; a Windows installer.

## Known deviations from the reference
None in behaviour; the reference ships a signed installer. IDECraft's download is a signed,
notarized and stapled DMG built by one script that checks what Gatekeeper checks before anyone
publishes it.
