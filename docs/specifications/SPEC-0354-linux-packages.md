# SPEC-0354: Linux packages

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** STD (freedesktop.org Desktop Entry, Icon Theme and Shared MIME-info specifications;
  AppStream metainfo; Debian binary package format and policy for `control`, `md5sums` and
  `/usr/share/doc/<package>/copyright`; AppImage type 2 format; `SOURCE_DATE_EPOCH` of
  reproducible-builds.org; minisign and OpenPGP detached signatures; SHA-256 checksum lists as
  written by `sha256sum`); GEN (a developer tool is expected as a portable archive, as an AppImage
  and as a Debian package); ADR-0007 (no telemetry, no update that phones home), ADR-0025 and
  CLAUDE.md rule 12 (security first), SPEC-0282 (macOS download, the model of this spec), SPEC-0008
  v8 behaviour 18 (passwords in the Secret Service on Linux), SPEC-0094 v7 behaviour 18 (a file
  opens its project), QG-1 criterion 3 (window self-test on Linux)
- **Feature matrix rows:** F-001

## Purpose
Ferris IDEA can be installed on Linux next to the signed macOS download: as a portable archive that
runs from any folder, as an AppImage that is one executable file, and as a Debian package that
puts the `ferris-idea` command on the `PATH` and registers the application with the desktop. One
script builds all three for the machine it runs on (x86_64 or aarch64), checks them, writes their
SHA-256 checksums and signs the checksum list with the project owner's key. Building uploads
nothing; publishing the packages is a separate step that needs the project owner's approval
(GO-4).

## Behaviour

### Building
1. **`scripts/release-linux.sh --out <folder> [--arch x86_64|aarch64] [--unsigned]`** builds the
   packages of the commit it is run on, on Linux only. `--arch` defaults to the machine's
   architecture; a different one is refused, because the packages are built natively and not
   cross-compiled. `--check` stops after the preconditions (behaviours 2–4) and builds and signs
   nothing. Only the script's own working folder inside `--out` is replaced; other files there
   stay. [GEN]
2. **Clean tree.** The script refuses to start when the working tree has uncommitted or untracked
   changes, and prints the commit, the version of the `ferris-idea` package in Cargo and whether the
   tag `v<version>` is on that commit (a missing tag is a warning). It fails when `HEAD` moved
   during the run. [GEN]
3. **Signing key or `--unsigned`.** Without `--unsigned` the script needs exactly one signing key,
   named in the environment and never written or printed: `FERRIS_IDEA_LINUX_MINISIGN_KEY` (path of
   a minisign secret key file, readable by its owner only, with `FERRIS_IDEA_LINUX_MINISIGN_PUBKEY`,
   the matching public key file, to check the signature) or `FERRIS_IDEA_LINUX_GPG_KEY` (ID or
   fingerprint of an OpenPGP secret key in the user's keyring). No key, both kinds, a missing or
   group/world-readable key file, an unknown OpenPGP key or a missing `minisign`/`gpg` is refused
   before anything is built. There is no default key. With `--unsigned` no key may be set (the
   two contradict each other), and every file name carries `-unsigned` (behaviour 9). [STD]
4. **Pinned tools.** Tools the build host does not have are fetched once into a tools folder
   (`FERRIS_IDEA_RELEASE_TOOLS`, default `<target>/release-tools`), each from its project's official
   release place at a fixed version and checked against a SHA-256 written in the script before it
   is used; a mismatch stops the script and deletes the download. They are: `appimagetool` 1.9.1
   and the AppImage type 2 runtime of release 20251108 (both from the AppImage project's GitHub
   releases; the runtime is handed to `appimagetool`, which then downloads nothing), and the SVG
   renderer `resvg` 0.45.1 (crates.io, `cargo install --locked`, Cargo checks the crate's
   checksum). Nothing is installed system-wide and no system setting is changed. [GEN]
5. **Release build.** `cargo build --release --locked -p ferris-idea`, with the source folder and the
   Cargo home mapped to fixed names in the binary (`--remap-path-prefix`), so the build folder
   does not end up in it. `SOURCE_DATE_EPOCH` defaults to the commit time and fixes every time
   stamp in the archives (behaviour 15). The binary is stripped of its symbols. [STD]
6. **Contents** (the same in all three formats, under the prefix `/usr` of the Debian package, the
   root of the AppImage's `usr/` or the archive's top folder):
   - `bin/ferris-idea`, the stripped binary;
   - `share/applications/ferris-idea.desktop` from `packaging/icons/linux/ferris-idea.desktop`
     (`ferris-idea launch %f`, SPEC-0282 behaviour 8) with the MIME types of behaviour 7;
   - `share/icons/hicolor/scalable/apps/ferris-idea.svg` (unchanged from `packaging/icons/linux/`)
     and PNG icons rendered from it at 16, 24, 32, 48, 64, 128, 256 and 512 pixels into
     `share/icons/hicolor/<n>x<n>/apps/ferris-idea.png`;
   - `share/mime/packages/ferris-idea.xml` (behaviour 7);
   - `share/metainfo/dev.ferrisidea.FerrisIDEA.metainfo.xml` (AppStream: identifier
     `dev.ferrisidea.FerrisIDEA` as on macOS, launchable `ferris-idea.desktop`, licence, project URL,
     the `ferris-idea` binary as provided command);
   - `share/doc/ferris-idea/`: `copyright` (the licence, `LICENSE.md`), `NOTICE`,
     `THIRD-PARTY-NOTICES.md` and the `LICENSES/` folder. [STD]
7. **MIME types for project files.** The files Ferris IDEA owns on macOS (SPEC-0282, Info.plist
   document types) get a MIME type each, `application/x-ferris-idea-<kind>` with the glob of its
   extension: connector (`*.idcconnector`), sources (`*.idcsources`), run (`*.idcrun`), keymap
   (`*.idckeymap`), theme (`*.idctheme`), code-style (`*.idcstyle`), live-templates
   (`*.idctemplates`), settings (`*.idcsettings`) — text files, subclasses of `text/plain` — and
   plugin (`*.idcplugin`, a subclass of `application/zip`). The desktop entry lists them, so a
   double click on such a file opens its project and the file (SPEC-0094 v7 behaviour 18). Folders
   (`inode/directory`) are not claimed: the file manager stays the default for them. [STD]
8. **Runtime dependencies.** The binary links only to the C and C++ runtime, libxcb, libxkbcommon
   and libxkbcommon-x11; it loads libvulkan, a Vulkan driver and libwayland-client at run time.
   It finds the system's fonts through the fontconfig configuration (`/etc/fonts`) and stops
   without any font (found by the smoke test in a container without fontconfig, see
   docs/quality/LINUX-PACKAGES.md). The script fails when the binary links to any other library,
   so the declared dependencies stay true. The Debian package declares: `libc6 (>= <the newest
   GLIBC symbol version the binary needs, read from it>)`, `libgcc-s1`, `libstdc++6`, `libxcb1`,
   `libxkbcommon0`, `libxkbcommon-x11-0`, `libwayland-client0`, `libvulkan1`,
   `mesa-vulkan-drivers | vulkan-icd`, `fontconfig-config` and `fonts-dejavu-core |
   fonts-noto-core`; recommends `git` (the self-test's and most projects' version control); suggests `gnome-keyring |
   keepassxc` (a Secret Service). Without a Secret Service Ferris IDEA refuses to store passwords and
   says why; it never writes them in clear text (SPEC-0008 v8 behaviour 18). The AppImage and the
   archive carry the same libraries' list in their README and rely on the system for them; they
   bundle no shared library. [STD]

### Packages
9. **File names** (`<v>` the version, `<a>` the architecture, `<d>` the Debian architecture
   `amd64` or `arm64`, `<u>` empty or `-unsigned`): `ferris-idea-<v>-linux-<a><u>.tar.gz`,
   `ferris-idea-<v>-<a><u>.AppImage`, `ferris-idea_<v>_<d><u>.deb`, the checksum list
   `ferris-idea-<v>-linux-<a><u>.sha256` and its signature `….sha256.minisig` or `….sha256.asc`. [STD]
10. **Portable archive.** One top folder `ferris-idea-<v>-linux-<a>/` with `bin/`, `share/` (behaviour
    6), `README.md` and `install.sh`. `bin/ferris-idea` runs from where it was unpacked.
    `install.sh [--prefix <folder>]` (default `~/.local`) copies `bin/` and `share/` there, updates
    the desktop, MIME and icon caches when their tools are present, needs no root for the default
    and says when `<prefix>/bin` is not on the `PATH`; `install.sh --uninstall [--prefix
    <folder>]` removes exactly the files it installed. Written with sorted names, owner and group
    0, the fixed time of behaviour 5 and `gzip -n`. [GEN]
11. **AppImage.** Type 2, made with the pinned `appimagetool` and runtime, the contents of
    behaviour 6 under `usr/`, the desktop entry and the SVG icon at its root, and an `AppRun` that
    starts `usr/bin/ferris-idea` with the given arguments, or `ferris-idea launch` when there are none
    (so a double click opens the most recent project). It carries no update information and no
    zsync file, so it never looks for updates (ADR-0007); the script checks that
    `--appimage-updateinformation` prints nothing. The command line is the AppImage itself
    (`./ferris-idea-<v>-<a>.AppImage --version`); systems without FUSE run it with
    `--appimage-extract-and-run`. [STD]
12. **Debian package.** Package `ferris-idea`, version `<v>`, section `devel`, the dependencies of
    behaviour 8, `Installed-Size`, a `Homepage` (the project's repository), a `Description`, and an
    `md5sums` list; the binary is `/usr/bin/ferris-idea`, so the command is on the `PATH`. It has no
    maintainer scripts: the desktop, MIME and icon caches are updated by the triggers of the
    packages that own them. It adds no APT source and no key. Built with `dpkg-deb
    --root-owner-group`. `Maintainer` is `FERRIS_IDEA_DEB_MAINTAINER`, else the project's name and
    repository. [STD]

### Checks, checksums, signature
13. **Checks.** The script fails unless: the binary starts and its `--version` prints `ferris-idea
    <v>` (from the staged copy, from the unpacked archive, from the files of the Debian package
    and through the AppImage with `--appimage-extract-and-run`); the desktop entry passes
    `desktop-file-validate` and the metainfo `appstreamcli validate --no-net` when those tools are
    present (when absent, the script says so); `dpkg-deb --info` shows the fields of behaviour 12;
    and the binary links to no library outside behaviour 8. [STD]
14. **Checksums and signature.** The checksum list holds one `sha256sum` line per package. Signed
    builds sign it detached: minisign with a trusted comment naming version, architecture and
    commit, checked afterwards with the public key; or `gpg --detach-sign --armor`, checked
    afterwards with `gpg --verify`. Passphrases are asked for by minisign or gpg itself; the script
    never reads, stores or prints them. At the end the script prints the commit, the version and
    every file with its size and SHA-256. [STD]
15. **Reproducible where it is cheap.** Archives, Debian package and AppImage take their time
    stamps from `SOURCE_DATE_EPOCH`; file order and owners are fixed; the binary contains no build
    path. Packaging the same binary twice gives the same archive and Debian package byte for byte.
    A byte-identical binary across machines is not promised (it needs the same toolchain, system
    libraries and build path). [STD]
16. **Signing later.** `scripts/release-linux.sh --sign <folder>` signs a finished `--unsigned`
    build without rebuilding it: it checks every file against the `-unsigned` checksum list,
    renames the packages without `-unsigned`, writes the new list and signs it as in behaviour 14
    with the key of behaviour 3. It needs no Linux (it runs on the project owner's Mac too), so
    the key stays on a machine the project owner controls and never goes to a shared build
    server. [GEN]
17. **Nothing is published.** The script talks only to the release places of behaviour 4 and to
    crates.io; it never creates a release, uploads anything or pushes a tag. Publishing needs the
    project owner's approval (GO-4). [GEN]

## Acceptance criteria
- **AK-1** On an x86_64 Ubuntu 24.04 build host, `scripts/release-linux.sh --out <folder>
  --unsigned` ends with the three `-unsigned` packages and their checksum list, and the checks of
  behaviour 13 pass.
- **AK-2** The same on aarch64 (Ubuntu 24.04 in a VM).
- **AK-3** Each package of each architecture in a fresh Ubuntu 24.04 system that has only what the
  package declares: the Debian package installs with `apt install ./…deb` (dependencies resolved),
  `ferris-idea` is on the `PATH`, `ldd` finds every library; the archive unpacks and runs; the
  AppImage runs with `--appimage-extract-and-run`. In each, `ferris-idea --version` prints the
  version and `ferris-idea serve --devices --key-file` answers; and the window self-test
  (`ferris-idea ui --selftest`) of the installed binary passes under Xvfb with Mesa's software Vulkan.
- **AK-4** The refusals of behaviour 3 (no key, both kinds, key file readable by others, unknown
  OpenPGP key, `--unsigned` with a key) and of behaviours 1–2 (other architecture, dirty tree).
- **AK-5** Signing with a throw-away minisign key and a throw-away OpenPGP key (never published):
  the signature checks, and a changed checksum list fails the check. `--sign` turns an
  `-unsigned` build into a signed one with the same package checksums.
- **AK-6** Packaging the same binary twice gives byte-identical archive and Debian package.
- **AK-7** First signed release by the project owner with his key (open).

## Out of scope
Flatpak and `.rpm` (open points: Flatpak needs a runtime choice and a Flathub or own repository;
`.rpm` needs the Fedora/openSUSE dependency names and a signing key of its own kind); an APT
repository; automatic updates; packages for systems older than the build host's C library
(glibc 2.39, Ubuntu 24.04, Debian 13, Fedora 40 or later; an older build base is an open point);
cross-compiling; uploading or publishing the packages; a server-only package without the window.

## Known deviations from the reference
The reference ships a tarball with its own runtime and an update service. Ferris IDEA's packages are
built by one script that checks them, carry no update check (ADR-0007), declare the system
libraries they need instead of bundling them, and are signed through a checksum list with the
project owner's key, which never has to be on the build server (behaviour 16).
