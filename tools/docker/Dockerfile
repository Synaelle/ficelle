# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The tools to build the client: the Linux AppImage and the Android APK. The
# source is not in the image, build.sh beside this file mounts the checkout and
# runs inside.sh in it. Ubuntu 22.04 like linux.yml, its glibc is the oldest
# the AppImage runs on. The versions follow the workflows and the repository.

FROM node:24-trixie-slim@sha256:54f84b4cf8432b21aaffd5d3ce8d41e12cea0d10d33c09d2f3c9879d49d6b446 AS node
FROM rust:1.99.0-trixie@sha256:81015672d6939b0265f09adf96d5de125482bb1330106076956b0518e74edc90 AS rust

FROM ubuntu:22.04@sha256:076e7bf82319619f28dd0aa9c02ab34b055ad37100395b4f49a2ffba04fb5e11
ARG PNPM_VERSION=12.15.0
ARG WASM_BINDGEN_VERSION=0.2.129
ARG RUST_VERSION=1.99.0
ARG CMDLINE_TOOLS=16111833
ARG CMDLINE_TOOLS_SHA256=0877a1d048fe4a24efe2eff536ca4223f7adeb58648bb81909d33c446918cfa8
ARG NDK_VERSION=29.0.14206865
ARG ANDROID_PLATFORM=android-37.0
ARG BUILD_TOOLS=36.0.0

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config perl make curl ca-certificates git unzip file xdg-utils \
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
      openjdk-21-jdk-headless \
 && rm -rf /var/lib/apt/lists/*

COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
 && npm install -g --no-fund --no-audit "pnpm@$PNPM_VERSION"

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
COPY --from=rust /usr/local/rustup /usr/local/rustup
COPY --from=rust /usr/local/cargo /usr/local/cargo
RUN rustup default "$RUST_VERSION" \
 && rustup target add --toolchain "$RUST_VERSION" wasm32-unknown-unknown \
      aarch64-linux-android armv7-linux-androideabi x86_64-linux-android \
 && rustup component add --toolchain "$RUST_VERSION" clippy rustfmt \
 && cargo install --locked wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" \
 && rm -rf /usr/local/cargo/registry

ENV JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 ANDROID_HOME=/opt/android-sdk
ENV NDK_HOME=$ANDROID_HOME/ndk/$NDK_VERSION
RUN curl -fsSL -o /tmp/tools.zip \
      "https://dl.google.com/android/repository/commandlinetools-linux-${CMDLINE_TOOLS}_latest.zip" \
 && echo "$CMDLINE_TOOLS_SHA256  /tmp/tools.zip" | sha256sum -c - \
 && mkdir -p "$ANDROID_HOME/cmdline-tools" \
 && unzip -q /tmp/tools.zip -d "$ANDROID_HOME/cmdline-tools" \
 && mv "$ANDROID_HOME/cmdline-tools/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest" \
 && rm /tmp/tools.zip \
 && sdk="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" \
 && (yes | "$sdk" --licenses > /dev/null) \
 && "$sdk" --install "ndk;$NDK_VERSION" "platforms;$ANDROID_PLATFORM" "build-tools;$BUILD_TOOLS" \
      "platform-tools" > /dev/null

# NDK 29 has no x86_64-linux-android-ranlib and the like, which OpenSSL's
# install step asks for. The cc crate reads these underscore names
ENV AR_aarch64_linux_android=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar \
    RANLIB_aarch64_linux_android=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ranlib \
    AR_armv7_linux_androideabi=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar \
    RANLIB_armv7_linux_androideabi=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ranlib \
    AR_x86_64_linux_android=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar \
    RANLIB_x86_64_linux_android=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ranlib

# linuxdeploy, which tauri runs for the AppImage, cannot mount itself in a container
ENV APPIMAGE_EXTRACT_AND_RUN=1
