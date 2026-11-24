# Linux packages: build and smoke tests

- **Spec:** [SPEC-0354](../specifications/SPEC-0354-linux-packages.md); script
  `scripts/release-linux.sh`, packaging files in `packaging/linux/` and `packaging/icons/linux/`
- **Measured:** 2026-11-18, by an agent on the coordinator's order
- **Rule:** every value is what was measured; nothing is rounded up. No package was published.

## Releasing

```sh
# On an x86_64 or aarch64 Linux machine (Ubuntu 24.04), in a clean checkout of the release commit:
scripts/release-linux.sh --out DIR --unsigned       # build, package, check; names carry -unsigned
# Later, on the machine that holds the project owner's key (also macOS; minisign or gpg needed):
FERRIS_IDEA_LINUX_MINISIGN_KEY=… FERRIS_IDEA_LINUX_MINISIGN_PUBKEY=… scripts/release-linux.sh --sign DIR
# Or in one run on a machine that holds the key:
FERRIS_IDEA_LINUX_GPG_KEY=<fingerprint> scripts/release-linux.sh --out DIR
```

The key never has to be on the build server: build `--unsigned` there, copy the folder, sign with
`--sign` (SPEC-0354 behaviour 16). The script uploads nothing; publishing needs the project
owner's approval (GO-4).

## Where it was built

| | x86_64 | aarch64 |
|---|---|---|
| Host | the project's build server, Ubuntu 24.04.5, build user without root, own checkout, own target folder, `nice -n 10`, 4 build jobs, next to the running CI | Lima 2.2.1 VM `idecraft-linux-ui` (Ubuntu 24.04, vz, the window self-test VM of [QG-1-LINUX-WINDOW.md](QG-1-LINUX-WINDOW.md)), own target folder, `nice -n 10`, 4 build jobs |
| Toolchain | rustc 1.99.1 | rustc 1.100.0 |
| Release build | 73 min 9 s (the CI ran full checks at the same time, load 9–14 on 8 cores) | 18 min 32 s |
| Release target folder | — | 3.8 GB |
| Commit | `00fc5373` (branch `linux-packages`; the binary is that of `main` at `e6cd4d02`, the branch adds only scripts and documents). The first round was packaged from `1e8e0760` with the same binaries; it lacked the font dependency found below and was replaced. | same |
| `SOURCE_DATE_EPOCH` | the commit's time | same |

Pinned tools, fetched by the script into its tools folder (19 MB): appimagetool 1.9.1 and the type 2
runtime 20251108 (AppImage project's GitHub releases, SHA-256 in the script, equal to the digests
GitHub publishes for the assets), resvg 0.45.1 (`cargo install --locked` from crates.io). Present
on the hosts: `dpkg-deb` 1.22.6, `appstreamcli` (the metainfo passed: "Validation was successful:
pedantic: 1"); `desktop-file-validate` was not installed on either host, so the desktop entry was
not validated by the script (behaviour 13 says so in its output).

## Packages (unsigned, version 1.0.0)

| File | Bytes | SHA-256 |
|---|---:|---|
| `ferris-idea-1.0.0-linux-x86_64-unsigned.tar.gz` | 74 454 998 | `5d9982e72d04d71557cdc2c154300a75fd4a80b9dfcfcd2eee2eddccccca00b4` |
| `ferris-idea-1.0.0-x86_64-unsigned.AppImage` | 68 119 032 | `9228e7326dd6641ff27e7dfc7a9c831a096e53339d599f8e4287f8d3918ea185` |
| `ferris_idea_1.0.0_amd64-unsigned.deb` | 50 013 612 | `513f60889765ef7b26218323fa9739f4e35a3664b67d9c8103423869f8da4fd7` |
| `ferris-idea-1.0.0-linux-aarch64-unsigned.tar.gz` | 69 766 470 | `0496af5a45b8d61b9bf67505195a696bdc542f8b37020bb34f36100038006d57` |
| `ferris-idea-1.0.0-aarch64-unsigned.AppImage` | 63 703 560 | `4fb6ca2fad6898480264c120c51bc7555c15f9f183dca3025076fdb5ff25481f` |
| `ferris_idea_1.0.0_arm64-unsigned.deb` | 44 138 744 | `36e459287e1da6bd1ce5c420cfb3af0805e4cf9c0fb4d2c18a3aa55d3a2c0d9d` |

Both binaries need glibc 2.39 (Debian package: `libc6 (>= 2.39)`) and link only to libc, libm,
libgcc_s, libstdc++, libxcb, libxkbcommon and libxkbcommon-x11; libvulkan and libwayland-client are
loaded at run time.

## Smoke tests in fresh systems

x86_64: a new container from the official `ubuntu:24.04` image
(`sha256:73b999393d07f2bf65a40373778a44dc9a6d54ad7718d29247d8ff9413c4d6a8`, 2 CPUs, 4 GB) on the build
server. aarch64: a new Lima VM `idecraft-pkg-smoke` (Ubuntu 24.04.5 cloud image 20260926, vz, 4 CPUs,
4 GiB; deleted and created anew for the final packages). Neither had Ferris IDEA or its libraries
before. The table shows the final packages; the first round gave the same results.

| Check | x86_64 | aarch64 |
|---|---|---|
| Checksum list matches the files | ok | ok |
| `apt install --no-install-recommends ./ferris_idea_…deb` resolves the declared dependencies | ok (pulled in fontconfig-config, fonts-dejavu-core, libvulkan1, libwayland-client0, libxcb1, libxkbcommon0, libxkbcommon-x11-0, mesa-vulkan-drivers and their libraries) | ok (same; fontconfig, the font, libxcb1 and libxkbcommon0 were already in the cloud image) |
| `ferris-idea` on the `PATH` (`/usr/bin/ferris-idea`), `ferris-idea --version` | ok, `ferris-idea 1.0.0 (stable)` | ok, same |
| `ldd /usr/bin/ferris-idea`: every library found; libvulkan.so.1, libwayland-client.so.0 and a Vulkan driver (lavapipe among them) installed | ok | ok |
| `ferris-idea serve --help` | exit 2, `error: unknown option --help` (serve has no `--help`; finding, see open points) | same |
| `ferris-idea serve --devices --key-file --state-dir <new folder>` (the serve path without a Secret Service) | ok, exit 0, no devices | ok |
| Desktop entry, icons, MIME XML, metainfo, `copyright` installed | ok | ok |
| MIME cache built by the trigger | not built: shared-mime-info is not in the container image (on a desktop it is) | ok, `application/x-ferris-idea-run` registered |
| Archive: `bin/ferris-idea --version` and `serve --devices` where unpacked | ok | ok |
| Archive: `install.sh --prefix <folder>`, then `--uninstall` leaves only caches | ok | ok |
| AppImage: no update information (`--appimage-updateinformation` empty) | ok | ok |
| AppImage: `--appimage-extract-and-run --version` and `serve --devices` | ok | ok |
| AppImage started directly | needs FUSE: "No suitable fusermount binary found" in the container | ok (the VM has FUSE) |
| `apt remove ferris-idea` | ok | ok |

## Window self-test of the packaged binary

All runs under `xvfb-run -a -s '-screen 0 1920x1080x24'` with Mesa's software Vulkan (lavapipe).

| Run | x86_64 | aarch64 |
|---|---|---|
| Full self-test, binary from the archive, in the window VM with every tool the self-test uses | not run: the build server has no X server and must not get packages, and no x86_64 machine with the tools was at hand | **`{"checks":1104,"failed":[],"skipped":[]}`**, exit 0, about 4 minutes (the first round's archive; its binary has the same SHA-256 as the final one) |
| Core groups (`editor,terminal,tabs,settings,theme`), installed final Debian package, fresh system | **`{"checks":86,"failed":[],"skipped":[]}`**, exit 0 | **`{"checks":86,"failed":[],"skipped":[]}`**, exit 0 |
| 23 groups that need no language server or debugger (editor … mcp-http), first-round package, fresh system | 258 checks, 11 failed | 258 checks, the same 11 failed |

The 11 failures of the larger subset are the same on both architectures and pass in the window VM
with the development tools (same binary): steps with a database connection, a run configuration,
a rename across a Markdown file and the unpinned Terminal. They depend on what the bare system
lacks, not on the package; they were not traced one by one. A full self-test in a system without
development tools is not a meaningful check: the language-server, database and debugger groups
fail instead of being skipped (aarch64, stopped after 94 failed checks in 30 minutes; window,
editor and terminal were drawn). The self-test expects the tools of
[QG-1-LINUX-WINDOW.md](QG-1-LINUX-WINDOW.md).

**Found by the smoke test:** in the x86_64 container the window stopped at once ("failed to resolve
font '.SystemUIFont' or any of the fallbacks"): the official container image has a font but no
fontconfig configuration (`/etc/fonts`), and Ferris IDEA found no font. With `fontconfig-config`
installed the same package drew its window. The Debian package now depends on `fontconfig-config`
and on `fonts-dejavu-core | fonts-noto-core` (before: the font recommended only); SPEC-0354
behaviour 8 says so. Desktop systems have both anyway. A first run without `git` stopped with "No
such file or directory": the self-test's projects need `git`, which the package recommends.

## Signing, refusals, reproducibility

- **Refusals** (aarch64 VM, `--check`): other architecture, no key, both key kinds, minisign key
  file mode 644, no public key, no `minisign`, unknown OpenPGP key, `--unsigned` with a key set,
  untracked file in the tree — each refused with its message; `--check --unsigned` passes. On
  macOS: the build is refused ("built on Linux"), `--sign` on a missing folder is refused.
- **OpenPGP** (throw-away key in a temporary keyring, deleted afterwards): a signed build wrote
  `ferris-idea-1.0.0-linux-aarch64.sha256.asc`; `gpg --verify` says "Good signature"; a changed
  checksum list is rejected; the package checksums equal those of the unsigned build.
- **minisign `--sign`** (throw-away key, minisign 0.11 from the Ubuntu archive, deleted
  afterwards): the `-unsigned` set was checked, renamed and signed; `minisign -V` verifies the list
  and its trusted comment; `sha256sum -c` passes; a changed list is rejected; the package checksums
  did not change.
- **Reproducible packaging:** (first round, aarch64) the same commit packaged twice gave byte-identical archive, Debian
  package and AppImage (all three SHA-256 equal). A new commit changes `SOURCE_DATE_EPOCH` and
  with it every package.
- `shellcheck -s sh` 0.9.0: no findings for `scripts/release-linux.sh`, `packaging/linux/install.sh`
  and `packaging/linux/AppRun`.

## Open points

1. **Signing key.** The project owner's minisign or OpenPGP key and the public key to publish next
   to the packages; the first signed release (SPEC-0354 AK-7).
2. **No font, no window:** Ferris IDEA stops when the system offers no font; it could fall back to
   the fonts it embeds (JetBrains Mono). The package dependencies cover it for now.
3. **Maintainer field** of the Debian package: an address of the project owner's choice
   (`FERRIS_IDEA_DEB_MAINTAINER`); default "Ferris IDEA project (repository URL)".
4. **`ferris-idea serve --help`** prints "unknown option" (exit 2); the top-level usage names the serve
   options. A `--help` for serve is a small follow-up.
5. **Older systems:** built on Ubuntu 24.04 the packages need glibc 2.39. Building on an older base
   (Debian 12, glibc 2.36, or Ubuntu 22.04, 2.35) would widen the reach.
6. **Flatpak and `.rpm`** (SPEC-0354 out of scope), an APT repository.
7. **x86_64 full window self-test:** needs an x86_64 machine with an X server or Xvfb and the
   development tools (the build server must not get them); a VM or container image prepared once
   would do.
8. **`desktop-file-validate`** was not installed on the build hosts; installing `desktop-file-utils`
   there (or in the CI image) lets the script validate the entry.

## Proposal: daily x86_64 package in the server CI (not live)

1. A subcommand `packages` of the server CI script, started by a systemd timer of its own
   (`ferris-idea-ci-packages.timer`, `OnCalendar=*-*-* 03:30`, `Persistent=true`), so the check loop
   is unchanged.
2. It takes the CI's `check.lock`, so it never runs beside a check, and runs only when `main`'s tip
   has a `success` result and differs from the last packaged commit.
3. Disk guard: it runs only with at least the CI's minimum free space plus 15 GB; its own target
   folder (`$BASE/target-release`, about 4 GB for a release build) is deleted above 8 GB, like the
   CI's `room()`.
4. In a worktree of `main`'s tip under `$BASE/packages-src` (never the branch under test),
   `nice -n 10 scripts/release-linux.sh --out $BASE/packages/<date>-<sha> --unsigned` with
   `CARGO_BUILD_JOBS=4`; the pinned tools stay in `$BASE/release-tools`.
5. The last three package folders are kept; a result line (`packages <sha> success|failure seconds`
   with sizes and SHA-256) goes to `results/packages-<sha>`; with the status token a commit status
   `idecraft-ci/linux-packages` is set.
6. Nothing is uploaded and nothing is signed there. The install smoke test (apt in a fresh
   container) needs a container engine the CI user does not have; until then it stays a step on
   the Mac or by hand.
