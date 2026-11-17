# Releasing the macOS download

**Status:** v1 · 2026-11-17 · Experimental: not yet run with a Developer ID. How the signed and notarized macOS download of a release is built
([SPEC-0282](specifications/SPEC-0282-macos-app-bundle.md) v3, behaviours 4a and 9–15).

The download is a DMG with `IDECraft.app`, signed with the project's Developer ID, notarized by
Apple and stapled, so it opens on any Mac without a Gatekeeper warning, also offline.
`scripts/release-macos.sh` builds and checks it. It uploads nothing: publishing the DMG on the
release page is a separate step that needs the project owner's approval.

No credential, key, certificate hash, team ID or Apple account detail belongs in this repository,
in a commit, a log or a release note. The scripts read them from the keychain or the environment
at run time and never print them.

## One-time setup (the project owner, on the Mac that builds releases)

Only the holder of the Apple developer account can do these steps.

1. **Developer ID Application certificate.** In Xcode › Settings › Accounts, select the team,
   choose Manage Certificates, then **+** › Developer ID Application. Xcode creates the private key
   in the login keychain and installs the certificate. Check it is there:

   ```sh
   security find-identity -v -p codesigning
   ```

   One line must read `"Developer ID Application: … (…)"`. Back up the certificate with its
   private key (Keychain Access › export as `.p12`, strong password) into the password manager;
   without the private key no further release can be signed with it.

2. **Notary credentials**, stored in the keychain under a profile name of your choice. Either with
   an app-specific password of the Apple account (create it at account.apple.com › Sign-In and
   Security › App-Specific Passwords):

   ```sh
   xcrun notarytool store-credentials <profile> --apple-id <apple-id> --team-id <team-id>
   ```

   (notarytool asks for the password; do not pass it on the command line), or with an App Store
   Connect API key (App Store Connect › Users and Access › Integrations › Team Keys, role
   Developer):

   ```sh
   xcrun notarytool store-credentials <profile> --key <path/to/AuthKey.p8> --key-id <key-id> --issuer <issuer-id>
   ```

   After storing, move the `.p8` file into the password manager and delete the copy on disk.
   The script can also take the key directly (`IDECRAFT_NOTARY_KEY`, `IDECRAFT_NOTARY_KEY_ID`,
   `IDECRAFT_NOTARY_ISSUER`); it then refuses a key file that anyone but its owner can read.

3. **First signing.** The first time codesign uses the new key, macOS asks for the login keychain
   password and whether to allow codesign: choose Always Allow, so later runs do not stop there.

## Building a release download

On a clean checkout of the release commit (the commit tagged `vX.Y.Z`):

```sh
export IDECRAFT_SIGN_IDENTITY='Developer ID Application: …'   # full name or SHA-1, from find-identity
export IDECRAFT_NOTARY_PROFILE=<profile>
scripts/release-macos.sh --check   # preconditions only: clean tree, identity, credentials
scripts/release-macos.sh           # build, sign, notarize, staple, check
```

The run takes the release build's time plus two notarization rounds of usually a few minutes each
(the app, then the DMG). It ends with the commit, the version, the DMG's path
(`target/release-macos/IDECraft-<version>-macos-<arch>.dmg`) and its SHA-256, which is also
written next to it as `<dmg>.sha256`.

What it refuses: uncommitted or untracked changes; an identity that is not a Developer ID
Application identity (a development or App Store identity cannot sign a download); both kinds of
notary credentials at once or an incomplete set. A missing tag `v<version>` on the commit is a
warning.

What it checks before it ends: `codesign --verify --deep --strict`, `spctl --assess` and
`stapler validate` for the app, the DMG and the app inside the mounted DMG, and that this app
starts and reports the release version.

If notarization is not accepted, the script prints the submission ID; Apple's report is shown by
`xcrun notarytool log <submission-id> --keychain-profile <profile>`.

## Without a Developer ID

`scripts/bundle-macos.sh` alone builds a bundle for the Mac it runs on, signed ad hoc. With
`IDECRAFT_SIGN_IDENTITY` set to any signing identity it signs with the hardened runtime, which is
how the runtime is checked without the Developer ID; such a bundle is not distributed.

## Hardened runtime and entitlements

The app runs with the hardened runtime. `packaging/macos/IDECraft.entitlements` grants only Apple
Events, microphone and camera, each matching a usage description in the app's Info.plist, because
programs started in IDECraft's terminal count as IDECraft for the system's privacy prompts. It
grants no code-signing exception: no JIT, no unsigned executable memory, no disabled library
validation. The reasons are in SPEC-0282 behaviour 4a.
