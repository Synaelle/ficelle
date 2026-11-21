# Building Ficelle

These builds are unsigned. Desktop builds can't update themselves and the APK is a debug build.

## Docker

The quickest way to the Linux AppImage and Android APK needs only Docker without sudo.

```bash
tools/docker/build.sh            # both
tools/docker/build.sh appimage   # or one of them
```

Files land in `target/docker`. Caches and the APK's debug key stay in the `ficelle-build-cache` volume, about 20 GB. Emptying it changes the key, and Android then refuses the APK as an update.

## Tools

Rust from rustup, Node 24 and pnpm 12.15.0, then `pnpm install --frozen-lockfile`.

## Desktop

```bash
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev librsvg2-dev file
node tools/build-release.mjs --unsigned -- --bundles appimage
```

On Windows, install the Visual Studio C++ build tools and Strawberry Perl, then use `--bundles nsis`. Builds land in `target/release/bundle/`. `pnpm dev` runs the app from the sources.

## Android

You need JDK 21, the Android SDK with `platforms;android-37.0`, NDK 29, and `ANDROID_HOME` and `NDK_HOME` set.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
printf 'tauri.android.versionName=%s\ntauri.android.versionCode=%s\n' \
  "$version" "$(node tools/version.mjs android-code)" > apps/client/src-tauri/gen/android/app/tauri.properties
bin=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
for t in aarch64_linux_android armv7_linux_androideabi x86_64_linux_android; do
  export AR_$t=$bin/llvm-ar RANLIB_$t=$bin/llvm-ranlib
done
pnpm --filter @app/client tauri android build --debug --target aarch64 armv7 x86_64 --apk
```

The APK lands in `gen/android/app/build/outputs/apk/universal/debug/` and installs as Ficelle Dev beside the real app.
