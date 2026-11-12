# AGENTS

Every session follows the cycle protocol in `docs/ROADMAP.md` §0; read
`docs/NEXT_PROMPT.md` first.

## Toolchain

- Rust `1.99.1` via `nib-rs/rust-toolchain.toml` (components `rustfmt`, `clippy`;
  targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`)
- `cargo-ndk` 4.1.2
- `cargo-deny` 0.20.2
- Android NDK `29.0.14206865`
- JDK 17+
- Emulator AVD: `nib_api34` (`system-images;android-34;google_apis;arm64-v8a`, `pixel_6`)

## Commands

Environment setup (one-off):

```sh
rustup toolchain install 1.99.1 --profile minimal -c rustfmt -c clippy \
  -t aarch64-linux-android -t armv7-linux-androideabi -t x86_64-linux-android
cargo +1.99.1 install cargo-deny --locked --version 0.20.2
~/Library/Android/sdk/cmdline-tools/latest/bin/sdkmanager \
  "system-images;android-34;google_apis;arm64-v8a"
~/Library/Android/sdk/cmdline-tools/latest/bin/avdmanager create avd \
  -n nib_api34 -k "system-images;android-34;google_apis;arm64-v8a" -d pixel_6 --force
```

PDFium binaries:

```sh
nib-rs/tools/fetch-pdfium.sh            # all assets (host + android)
nib-rs/tools/fetch-pdfium.sh --host-only
nib-rs/tools/fetch-pdfium.sh --android-only
```

Rust checks (run inside `nib-rs/`; `rust-toolchain.toml` pins 1.99.1):

```sh
cd nib-rs
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

Generated Kotlin bindings and ELF page-size check:

```sh
nib-rs/tools/gen-bindings.sh <path-to-libnib_ffi.so|dylib> <out-dir>
nib-rs/tools/check-elf-alignment.sh <dir|.so|.aar|.apk>...
```

Note: `uniffi-bindgen generate` takes the library path **positionally**;
the `--library` flag is a deprecated no-op since uniffi 0.31.

Android cross-build (run from `nib-rs/`; `cargo ndk` resolves the manifest
from the working directory):

```sh
cd nib-rs
ANDROID_NDK_HOME=~/Library/Android/sdk/ndk/29.0.14206865 \
  cargo ndk -t arm64-v8a -P 26 -o <out-dir> build -p nib-ffi --release
```

## Android

Gradle (wrapper is Gradle 9.8.2, AGP 9.5.0 — no local Gradle needed):

```sh
./gradlew :nib-android:assembleRelease   # AAR at nib-android/build/outputs/aar/
./gradlew :sample-app:assembleRelease    # R8-minified APK
./gradlew ktlintFormat ktlintCheck
./gradlew :nib-android:connectedDebugAndroidTest
```

The build runs `cargoBuild<Variant>` (cargo-ndk) and `uniffiBindgen<Variant>`
automatically. Debug builds only the host ABI; override with
`-Pnib.abis=arm64-v8a,x86_64`. UniFFI bindings are generated from the host
debug `libnib_ffi` because `strip = true` in the release profile removes the
ELF symbols the UniFFI metadata lives in.

CI (`.github/workflows/ci.yml`) runs:

```sh
./gradlew --no-daemon ktlintCheck :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh \
  nib-android/build/outputs/aar/nib-android-release.aar \
  sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon :nib-android:connectedDebugAndroidTest -Pnib.abis=x86_64
```

Emulator (`adb` lives in `~/Library/Android/sdk/platform-tools`):

```sh
~/Library/Android/sdk/emulator/emulator -avd nib_api34 \
  -no-window -no-audio -no-boot-anim -no-snapshot &
adb wait-for-device
adb shell getprop sys.boot_completed          # wait for "1"
adb install -r sample-app/build/outputs/apk/release/sample-app-release.apk
adb shell am start -n io.github.nibpdf.sample/.MainActivity
adb exec-out screencap -p > /tmp/nib-sample.png
adb shell uiautomator dump /sdcard/ui.xml && adb shell cat /sdcard/ui.xml
```
