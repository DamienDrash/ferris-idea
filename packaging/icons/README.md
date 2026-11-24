# Application icons

Platform icons for the Ferris IDEA desktop application, copied unchanged from brand package
**IDECraft Brand Identity v1.1.5** (2026-11-03). Every file is rendered from the app icon master
[`docs/design/assets/idecraft-app-icon.svg`](../../docs/design/assets/idecraft-app-icon.svg);
do not edit the rasters by hand.

| Platform | Files | Sizes |
|---|---|---|
| macOS | `macos/FerrisIDEA.icns` | ICNS 16–1024 px (11 entries) |
| Windows | `windows/ferris-idea.ico` | 16, 24, 32, 48, 64, 128, 256 px |
| Linux | `linux/hicolor/scalable/apps/ferris-idea.svg`, `linux/ferris-idea.desktop` | scalable |

The package's PNG rasters (macOS iconset, Windows and Linux PNGs) are not in the repository: the
root `.gitignore` excludes `*.png` outside `docs/`. The ICNS, ICO and SVG cover every size.

Status: the macOS icon is used by a local app bundle (Experimental, below); no installer is built
yet. Since ADR-0018 the binary also embeds `macos/FerrisIDEA.icns` and sets it as the application
icon at start-up, so `ferris-idea ui` shows it in the macOS Dock without a bundle. Linux windows carry
the application id `ferris-idea`, matching `StartupWMClass` in `linux/ferris-idea.desktop`, so an
installed icon is found. Native checks: macOS Dock in hand test 192 (bare binary) and 205 (bundle);
Windows Explorer/taskbar and a Linux desktop are not verified.

## macOS app bundle (Experimental)

`scripts/bundle-macos.sh` builds `FerrisIDEA.app` from the release binary into `target/bundle/macos/`
(`SPEC-0282`):

```sh
scripts/bundle-macos.sh            # cargo build --release -p ferris-idea, then bundle and check
scripts/bundle-macos.sh --debug    # bundle the debug binary (quick check, also in scripts/ci-local.sh)
```

- **Contents:** `Contents/MacOS/ferris-idea`, `Contents/Resources/FerrisIDEA.icns` (this folder's file,
  unchanged), `Contents/Info.plist` (identifier `dev.ferrisidea.FerrisIDEA`, version from Cargo, the
  minimum macOS read from the binary, high-resolution support, privacy usage descriptions for
  project folders, removable and network volumes, the local network, and for programs started in
  the terminal: controlling other apps, microphone, camera).
- **Signature:** without `FERRIS_IDEA_SIGN_IDENTITY` ad hoc (`codesign --sign -`), so it runs on the
  Mac that built it. With `FERRIS_IDEA_SIGN_IDENTITY` set to a signing identity in the keychain, every
  nested Mach-O file and then the app are signed inside-out with the hardened runtime, the
  entitlements in [`../macos/FerrisIDEA.entitlements`](../macos/FerrisIDEA.entitlements) and a secure
  timestamp; there is no default identity. The download (Developer ID, notarized, stapled DMG) is
  built by `scripts/release-macos.sh`, which uploads nothing.
- **Checks:** the script fails unless `plutil -lint` and `codesign --verify --strict` pass, the icon
  equals this folder's file, and the bundled binary's `--version` matches the plist.
- **Starting:** from Finder, the Dock or Spotlight the app opens the most recent project that still
  exists, else asks for a folder (`ferris-idea launch`). Every command also works from
  `FerrisIDEA.app/Contents/MacOS/ferris-idea`.

## Linux desktop entry (Experimental)

`linux/ferris-idea.desktop` starts `ferris-idea launch %f`: a launcher click opens the most recent
project, a folder (or a file's folder) dropped on it opens that folder. Install for one user after
`cargo build --release -p ferris-idea`:

```sh
install -Dm755 target/release/ferris-idea ~/.local/bin/ferris-idea
install -Dm644 packaging/icons/linux/ferris-idea.desktop ~/.local/share/applications/ferris-idea.desktop
cp -r packaging/icons/linux/hicolor ~/.local/share/icons/
update-desktop-database ~/.local/share/applications 2>/dev/null || true
gtk-update-icon-cache ~/.local/share/icons/hicolor 2>/dev/null || true
```

`~/.local/bin` must be on the `PATH` of the desktop session. Not verified on a Linux desktop yet.

The Linux packages (portable archive, AppImage, Debian package; `scripts/release-linux.sh`,
`SPEC-0354`) install the same desktop entry with the MIME types of Ferris IDEA's own files
(`../linux/ferris-idea-mime.xml`), the scalable icon and PNG icons rendered from it at 16 to 512
pixels, and the AppStream metainfo `../linux/dev.ferrisidea.FerrisIDEA.metainfo.xml`.

Deviations from the package: the desktop entry's `Comment` reads "Experimental Rust-native IDE for
code and databases" instead of the package text, so that it matches what the current build does
([CONTENT_STYLE_GUIDE.md](../../docs/design/CONTENT_STYLE_GUIDE.md) §3); its `Exec` line is
`ferris-idea launch %f` instead of `ferris-idea %F`, because `ferris-idea` without a command only prints its
usage (`SPEC-0282` behaviour 8).
