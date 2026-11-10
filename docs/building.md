# Building Ficelle

Signed releases are built by the release workflow on GitHub, which holds the keys. The builds below need no key. Desktop builds leave out the update signature, and the APK is a debug build.

## Docker

The quickest way to get the Linux AppImage and the Android APK, with nothing to install but Docker.

```bash
tools/docker/build.sh            # both
tools/docker/build.sh appimage   # or one of them
```

The files land in `target/docker`, or in the folder `OUT_DIR` names. If your user cannot reach Docker, run it with `DOCKER="sudo docker"`.

The image is Ubuntu 22.04 with every tool at the version the workflows use, about 9 GB. Caches stay in the `ficelle-build-cache` volume, about 20 GB after both builds, so later builds only redo what changed. `docker volume rm ficelle-build-cache` frees the space. The container gets 8 GB of memory and half the cores, and `BUILD_MEMORY=12g` raises the memory if a build stops for lack of it.

The volume also keeps the debug key the APK is signed with. An APK from another machine or an emptied volume has another key, so Android refuses it as an update until the old one is uninstalled.

## Tools

Every build needs these.

- Rust from rustup. `rust-toolchain.toml` picks the version (1.99.0) and adds the WebAssembly target.
- Node 24 and pnpm 12.15.0 (`corepack enable` or `npm install -g pnpm@12.15.0`).
- wasm-bindgen for the end-to-end encryption code, `cargo install --locked wasm-bindgen-cli --version 0.2.129`.

Then, in the checkout, `pnpm install --frozen-lockfile`.

## Desktop

On Debian or Ubuntu, install the WebKitGTK and system packages first.

```bash
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev file
```

Windows needs the Visual Studio C++ build tools and Perl for OpenSSL, such as Strawberry Perl with `PERL` set to its `perl.exe`. A Mac needs the Xcode command line tools.

`pnpm dev` runs the app from the sources. To build it under its real name, without the update signature:

```bash
node tools/build-release.mjs --unsigned -- --bundles appimage   # Linux
node tools/build-release.mjs --unsigned -- --bundles nsis       # Windows
node tools/build-release.mjs --unsigned -- --bundles app,dmg    # macOS
```

The result is in `target/release/bundle/`. For a Mac app that runs on both Apple Silicon and Intel, run `rustup target add aarch64-apple-darwin x86_64-apple-darwin` and add `--target universal-apple-darwin`. A plain `pnpm --filter @app/client tauri build` gives Ficelle Dev instead, which keeps its notes apart from the real app.

## Android

Android needs JDK 21 and the Android SDK with `platforms;android-37.0` and NDK 29.0.14206865. Set `ANDROID_HOME` to the SDK and `NDK_HOME` to the NDK, and add the Rust targets.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
```

Gradle numbers the APK version 1 unless `tauri.properties` says otherwise, and Android refuses it as an update over any build already installed. Write it first.

```bash
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
printf 'tauri.android.versionName=%s\ntauri.android.versionCode=%s\n' \
  "$version" "$(node tools/version.mjs android-code)" > apps/client/src-tauri/gen/android/app/tauri.properties
```

NDK 29 has no `x86_64-linux-android-ranlib` and the like, so OpenSSL's install step fails. Point the variables below at the NDK's own tools, with underscores in the names, for each target you build.

```bash
bin=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
for t in aarch64_linux_android armv7_linux_androideabi x86_64_linux_android; do
  export AR_$t=$bin/llvm-ar RANLIB_$t=$bin/llvm-ranlib
done
pnpm --filter @app/client tauri android build --debug --target aarch64 armv7 x86_64 --apk
```

The APK is `apps/client/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`, about 400 MB with its debug symbols. It is signed with the SDK's debug key and installs as Ficelle Dev next to the real app. A release build without `--debug` needs the release key in `gen/android/keystore.properties` and comes out unsigned otherwise.
