#!/usr/bin/env bash
# Bygger APK-er: Rust-biblioteket med cargo-ndk, deretter Android-delen med Gradle.
#
#   scripts/build-android.sh            # debug-APK (og release-APK hvis RELEASE_KEYSTORE er satt)
#   ABIS="arm64-v8a" scripts/build-android.sh    # bare 64-bit ARM (raskere)
#
# Krever ANDROID_HOME (eller ANDROID_SDK_ROOT) og ANDROID_NDK_HOME, Rust-målene for ABI-ene og
# cargo-ndk. Se README.md.
set -euo pipefail
cd "$(dirname "$0")/.."

ABIS="${ABIS:-arm64-v8a armeabi-v7a x86_64}"
API=26
JNI=android/app/src/main/jniLibs

targets=()
for abi in $ABIS; do targets+=(-t "$abi"); done

rm -rf "$JNI"
# Rust bygges alltid optimalisert: debug-Rust er for treg til å vise store tegninger.
cargo ndk "${targets[@]}" -P "$API" -o "$JNI" build --release --lib

tasks=(assembleDebug)
if [[ -n "${RELEASE_KEYSTORE:-}" ]]; then
  tasks+=(assembleRelease)
fi
cd android
if [[ -x ./gradlew ]]; then ./gradlew --no-daemon "${tasks[@]}"; else gradle --no-daemon "${tasks[@]}"; fi

echo
find app/build/outputs/apk -name '*.apk' -print
