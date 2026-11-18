#!/usr/bin/env bash
# Builds a signed debug APK of Chequer for arm64. Needs: the aarch64-linux-android rustup target, NDK, Android SDK, JDK.
# Usage: tools/build_apk.sh [output_dir]   (default: target/apk)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/crates/chequer-app"
OUT="${1:-$ROOT/target/apk}"
SDK="${ANDROID_SDK:-/opt/android-sdk}"
BT="$SDK/build-tools/35.0.0"
PLATFORM_JAR="$SDK/platforms/android-35/android.jar"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

cargo build --profile fastrel --target aarch64-linux-android -p chequer-app --lib --manifest-path "$ROOT/Cargo.toml"

rm -rf "$OUT/stage" && mkdir -p "$OUT/stage/res/mipmap-xxhdpi" "$OUT/stage/lib/arm64-v8a" "$OUT/stage/classes" "$OUT/stage/dex"
cp "$APP/assets/icon/chequer-256.png" "$OUT/stage/res/mipmap-xxhdpi/ic_launcher.png"
cp "$CARGO_TARGET_DIR/aarch64-linux-android/fastrel/libchequer_app.so" "$OUT/stage/lib/arm64-v8a/"

javac --release 11 -classpath "$PLATFORM_JAR" -d "$OUT/stage/classes" "$APP"/android/dev/huecoder/chequer/*.java
"$BT/d8" --min-api 26 --lib "$PLATFORM_JAR" --output "$OUT/stage/dex" $(find "$OUT/stage/classes" -name '*.class')

cd "$OUT/stage"
"$BT/aapt2" compile --dir res -o res.zip
"$BT/aapt2" link -o base.apk -I "$PLATFORM_JAR" --manifest "$APP/android/AndroidManifest.xml" res.zip
cp dex/classes.dex .
zip -q base.apk classes.dex && zip -q -r base.apk lib
"$BT/zipalign" -f -p 4 base.apk aligned.apk

KEYSTORE="$OUT/debug.keystore"
[ -f "$KEYSTORE" ] || keytool -genkeypair -keystore "$KEYSTORE" -storepass android -keypass android -alias k -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=debug" >/dev/null 2>&1
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android --out "$OUT/Chequer-android-debug.apk" aligned.apk
"$BT/apksigner" verify "$OUT/Chequer-android-debug.apk"
ls -l "$OUT/Chequer-android-debug.apk"
