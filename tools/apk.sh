#!/usr/bin/env bash
# Compila el APK de release (arm64, Nothing Phone 3), lo deja en dist/ y,
# si hay un móvil conectado por adb, lo instala encima (los datos se conservan).
#
#   tools/apk.sh            compila + instala si hay dispositivo
#   tools/apk.sh --no-install
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$ANDROID_HOME/ndk/28.2.13676358}"
export JAVA_HOME="${JAVA_HOME:-/opt/android-studio/jbr}"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"

cd "$root/apps/oxide-kcal"
cargo apk build --lib --release

target_dir="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
mkdir -p "$root/dist"
cp "$target_dir/release/apk/oxide-kcal.apk" "$root/dist/oxide-kcal.apk"
echo "APK: $root/dist/oxide-kcal.apk"

if [[ "${1:-}" != "--no-install" ]] && adb get-state >/dev/null 2>&1; then
    adb install -r "$root/dist/oxide-kcal.apk"
    adb shell am start -n dev.oxide.kcal/android.app.NativeActivity >/dev/null
fi
