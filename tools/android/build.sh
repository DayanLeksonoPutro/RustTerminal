#!/bin/bash
# Build the Android APK (arm64) with cargo-apk.
#
#   tools/android/build.sh           # release APK, signed with a local keystore
#   tools/android/build.sh debug     # debug APK, signed with the debug keystore
#   tools/android/build.sh install   # release APK installed on a connected device (adb)
#
# Needs: Android SDK + NDK, `rustup target add aarch64-linux-android`, `cargo install cargo-apk`.
# The release keystore is created once at ~/.android/rust-terminal-release.jks (keep it safe:
# updates of an installed app must be signed with the same key). Override with
# CARGO_APK_RELEASE_KEYSTORE / CARGO_APK_RELEASE_KEYSTORE_PASSWORD.
set -euo pipefail
cd "$(dirname "$0")/../.."

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
if [ -z "${ANDROID_NDK_ROOT:-}" ]; then
  ANDROID_NDK_ROOT="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | head -1)"
  export ANDROID_NDK_ROOT
fi
[ -d "$ANDROID_NDK_ROOT" ] || { echo "Android NDK not found (set ANDROID_NDK_ROOT)"; exit 1; }

MODE="${1:-release}"
if [ "$MODE" = "debug" ]; then
  cargo apk build --lib
  ls -la target/debug/apk/*.apk
  exit 0
fi

KS="${CARGO_APK_RELEASE_KEYSTORE:-$HOME/.android/rust-terminal-release.jks}"
PW="${CARGO_APK_RELEASE_KEYSTORE_PASSWORD:-rustterminal}"
if [ ! -f "$KS" ]; then
  mkdir -p "$(dirname "$KS")"
  keytool -genkeypair -keystore "$KS" -storepass "$PW" -keypass "$PW" -alias rust-terminal \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Rust Terminal" >/dev/null
  echo "created release keystore: $KS"
fi
export CARGO_APK_RELEASE_KEYSTORE="$KS" CARGO_APK_RELEASE_KEYSTORE_PASSWORD="$PW"

if [ "$MODE" = "install" ]; then
  cargo apk run --lib --release
else
  cargo apk build --lib --release
  ls -la target/release/apk/RustTerminal.apk
fi
