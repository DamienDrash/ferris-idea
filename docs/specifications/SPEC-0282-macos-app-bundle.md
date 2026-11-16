# SPEC-0282: Local macOS app bundle

- **Version:** 2 · **Date:** 2026-11-15 (v2: a file opens its project and the file, SPEC-0094 v7 behaviours 18 and 19); 2026-11-04 (v1)
- **Sources:** STD (Apple bundle layout `Contents/{Info.plist,MacOS,Resources}`, Info.plist keys
  `CFBundleIdentifier`, `CFBundleExecutable`, `CFBundleIconFile`, `CFBundleShortVersionString`,
  `CFBundleVersion`, `LSMinimumSystemVersion`, `NSHighResolutionCapable`, privacy usage
  descriptions; `codesign` ad-hoc signatures; freedesktop.org Desktop Entry specification);
  GEN (an IDE started from the Dock or Finder has no command line, so it needs a starting
  folder); ADR-0018 (app icon), SPEC-0060 v6 behaviour 12, SPEC-0094, SPEC-0124, SPEC-0278
- **Feature matrix rows:** F-001

## Purpose
IDECraft can be started like any Mac application — from Finder, the Dock or Spotlight — from a
bundle built locally out of the same binary. Building it needs no Apple developer account and
publishes nothing.

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
4. **Signature.** The bundle is signed ad hoc (`codesign --force --sign -`), so macOS accepts it
   on the machine that built it. No Developer ID, no hardened runtime (plugins compile WebAssembly
   at run time), no notarization, no upload: distributing the bundle needs the project owner's approval of
   publishing (GO-4). [ADR-0007]
5. **Checks.** After building, the script fails unless `plutil -lint` accepts the Info.plist,
   `codesign --verify --strict` accepts the bundle, the icon equals the packaged file, and
   `Contents/MacOS/idecraft --version` prints `idecraft <version>` with the plist's version. It
   opens no window.

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

## Acceptance criteria
- **AK-1** Bundle build and checks with the debug binary in `scripts/ci-local.sh` (macOS); the
  release build with `scripts/bundle-macos.sh`.
- **AK-2** Which folder `launch` opens, and when a binary counts as started from a bundle
  (`launch::tests` in `crates/app`).
- **AK-3** Hand test (round 5): start `IDECraft.app` from Finder; Dock icon, the most recent
  project opens, About shows the version, ⌘Q quits.

## Out of scope
Developer ID signing, notarization, DMG or installer, automatic updates, release uploads (all
need GO-4); opening folders dropped on the Dock icon while IDECraft runs; a Windows installer.

## Known deviations from the reference
None in behaviour; the reference ships a signed installer.
