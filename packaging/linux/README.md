# Ferris IDEA for Linux

This folder holds Ferris IDEA for Linux on one architecture (x86_64 or aarch64). The release script
`scripts/release-linux.sh` copies this file into the portable archive as its `README.md`
(SPEC-0354 in the Ferris IDEA repository).

## Run or install

- Run it from here: `bin/ferris-idea launch` opens the most recent project, `bin/ferris-idea --version`
  prints the version.
- Install it for your user: `./install.sh` copies `bin/` and `share/` into `~/.local`, so the
  `ferris-idea` command, the menu entry, the icons and the file types are there. `~/.local/bin` must
  be on your `PATH`. `./install.sh --prefix /usr/local` installs elsewhere;
  `./install.sh --uninstall` removes exactly what it installed.
- The Debian package (`ferris-idea_<version>_<arch>.deb`) and the AppImage are the other forms of the
  same build.

## What it needs

- Linux with glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 or later).
- libxcb, libxkbcommon, libxkbcommon-x11, libwayland-client, the Vulkan loader (libvulkan) and a
  Vulkan driver (Mesa or your GPU vendor's), the fontconfig configuration and a font (DejaVu or
  Noto); without a font the window cannot start. On Debian and Ubuntu: `sudo apt install libxcb1
  libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libvulkan1 mesa-vulkan-drivers
  fontconfig-config fonts-dejavu-core`.
- A Secret Service (GNOME Keyring, KeePassXC or KWallet) to store database and host passwords.
  Without one, Ferris IDEA refuses to store passwords and says why; it never writes them in clear
  text.

## Checking the download

Each release has a checksum list `ferris-idea-<version>-linux-<arch>.sha256` and its signature
(`.minisig` or `.asc`). Check the signature first, then the files:

```sh
minisign -Vm ferris-idea-<version>-linux-<arch>.sha256 -P <the project's public key>
sha256sum -c --ignore-missing ferris-idea-<version>-linux-<arch>.sha256
```

## Privacy

Ferris IDEA sends no telemetry and does not look for updates by itself.

Licence: `share/doc/ferris-idea/copyright` (Apache License 2.0), with `NOTICE`,
`THIRD-PARTY-NOTICES.md` and `LICENSES/` beside it.
