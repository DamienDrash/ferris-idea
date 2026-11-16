#!/usr/bin/env bash
# Builds the mobile core (crates/mobile) and its bindings (ADR-0029, SPEC-0326).
#   scripts/build-mobile.sh ios       iOS device + simulator libraries, Swift bindings, XCFramework
#   scripts/build-mobile.sh android   arm64 Android library, Kotlin bindings
#   scripts/build-mobile.sh android-test   the core's loopback tests as an arm64 Android program
#   scripts/build-mobile.sh android-apk    the above, then the debug APK and an unsigned release APK
#                                          (plus a copy signed with a throwaway key for emulators)
#                                          into $IDECRAFT_APK_DIR (default mobile/android/build/apk)
# Output goes to mobile/ios/Generated and mobile/android/generated (both ignored by Git).
set -euo pipefail

cd "$(dirname "$0")/.."
target_dir="${CARGO_TARGET_DIR:-target}"
profile="${MOBILE_PROFILE:-release}"
flag=()
[ "$profile" = release ] && flag=(--release)

bindgen() {
  # $1 language, $2 output folder. The metadata comes from a host build of the library.
  cargo build -p idecraft-mobile
  cargo run -q -p idecraft-mobile --features bindgen --bin uniffi-bindgen -- generate \
    --library "$target_dir/debug/libidecraft_mobile.dylib" --language "$1" --out-dir "$2"
}

ios() {
  local out=mobile/ios/Generated
  rm -rf "$out"
  mkdir -p "$out/headers"
  for t in aarch64-apple-ios aarch64-apple-ios-sim; do
    cargo build -p idecraft-mobile "${flag[@]}" --target "$t"
  done
  bindgen swift "$out"
  cp "$out"/idecraft_mobileFFI.h "$out/headers/"
  cp "$out"/idecraft_mobileFFI.modulemap "$out/headers/module.modulemap"
  xcodebuild -create-xcframework \
    -library "$target_dir/aarch64-apple-ios/$profile/libidecraft_mobile.a" -headers "$out/headers" \
    -library "$target_dir/aarch64-apple-ios-sim/$profile/libidecraft_mobile.a" -headers "$out/headers" \
    -output "$out/IDECraftCore.xcframework"
  echo "built: aarch64-apple-ios, aarch64-apple-ios-sim -> $out"
}

android_env() {
  local sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
  local ndk="${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* | sort -V | tail -1)}"
  local bin="$ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin"
  [ -x "$bin/aarch64-linux-android26-clang" ] || { echo "no Android NDK at $ndk" >&2; exit 1; }
  export ANDROID_NDK_HOME="$ndk" ANDROID_NDK_ROOT="$ndk"
  export PATH="$bin:$PATH"
  export CC_aarch64_linux_android="$bin/aarch64-linux-android26-clang"
  export AR_aarch64_linux_android="$bin/llvm-ar"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android26-clang"
}

android_test() {
  android_env
  cargo test -p idecraft-mobile --test paired --no-run --target aarch64-linux-android
}

android() {
  android_env
  local out=mobile/android/generated
  rm -rf "$out"
  mkdir -p "$out/jniLibs/arm64-v8a"
  cargo build -p idecraft-mobile "${flag[@]}" --target aarch64-linux-android "$@"
  cp "$target_dir/aarch64-linux-android/$profile/libidecraft_mobile.so" "$out/jniLibs/arm64-v8a/"
  bindgen kotlin "$out/kotlin"
  echo "built: aarch64-linux-android -> $out"
}

android_apk() {
  # The app needs Gradle 8.9 or later and JDK 17; nothing is downloaded but its dependencies.
  local gradle="${GRADLE:-gradle}"
  command -v "$gradle" >/dev/null || { echo "no gradle on PATH (8.9 or later)" >&2; exit 1; }
  android
  local sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
  local tools
  tools="$(ls -d "$sdk"/build-tools/* | sort -V | tail -1)"
  local out="${IDECRAFT_APK_DIR:-mobile/android/build/apk}"
  # A throwaway signing key per build, in a private temp folder that is removed on exit: never the
  # shared ~/.android/debug.keystore, never a key in the repository.
  # Global, not local: the EXIT trap runs after this function has returned.
  apk_keys="$(mktemp -d "${TMPDIR:-/tmp}/idecraft-apk-key.XXXXXX")"
  chmod 700 "$apk_keys"
  trap 'rm -rf "$apk_keys"' EXIT
  IDECRAFT_ANDROID_STORE="$apk_keys/throwaway.p12"
  IDECRAFT_ANDROID_STORE_PASSWORD="$(openssl rand -hex 24)"
  export IDECRAFT_ANDROID_STORE IDECRAFT_ANDROID_STORE_PASSWORD ANDROID_HOME="$sdk"
  keytool -genkeypair -keystore "$IDECRAFT_ANDROID_STORE" -storetype PKCS12 \
    -storepass:env IDECRAFT_ANDROID_STORE_PASSWORD -keypass:env IDECRAFT_ANDROID_STORE_PASSWORD \
    -alias idecraft-throwaway -keyalg EC -groupname secp256r1 -validity 30 \
    -dname "CN=IDECraft throwaway build key" >/dev/null 2>&1
  "$gradle" -q -p mobile/android assembleDebug assembleRelease
  mkdir -p "$out"
  cp mobile/android/app/build/outputs/apk/debug/app-debug.apk "$out/idecraft-debug.apk"
  cp mobile/android/app/build/outputs/apk/release/app-release-unsigned.apk \
    "$out/idecraft-release-unsigned.apk"
  # The same release, signed with the throwaway key only so that an emulator can install it.
  "$tools/apksigner" sign --ks "$IDECRAFT_ANDROID_STORE" \
    --ks-pass env:IDECRAFT_ANDROID_STORE_PASSWORD --ks-key-alias idecraft-throwaway \
    --out "$out/idecraft-release-throwaway-signed.apk" "$out/idecraft-release-unsigned.apk"
  rm -f "$out"/*.idsig
  ls -l "$out"
}

case "${1:-}" in
  ios) ios ;;
  android) shift; android "$@" ;;
  android-test) android_test ;;
  android-apk) android_apk ;;
  *) echo "usage: $0 ios|android|android-test|android-apk" >&2; exit 2 ;;
esac
