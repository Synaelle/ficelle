#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Runs in the build image, started by build.sh: copies the checkout mounted at
# /src (its files git does not ignore) to /work, builds what is asked and puts
# it in /out. The caches live in the volume at /cache.
set -euo pipefail

step() { echo "== $* $(date +%T)"; }

export CARGO_HOME=/cache/cargo GRADLE_USER_HOME=/cache/gradle ANDROID_USER_HOME=/cache/android
export GRADLE_OPTS=-Dorg.gradle.daemon=false
mkdir -p /cache/target /out

step copy the checkout
mkdir -p /work
git -c safe.directory=/src -C /src ls-files -z --cached --others --exclude-standard |
  tar -C /src --null --files-from=- --ignore-failed-read -c | tar -C /work -x
# kept between runs, so a second build only redoes what changed
ln -s /cache/target /work/target
cd /work

step install packages
timeout 1200 pnpm install --frozen-lockfile --store-dir /cache/pnpm-store

version=$(node -p "require('fs').readFileSync('Cargo.toml', 'utf8').match(/\[workspace\.package\][^[]*?\nversion = \"([^\"]+)\"/)[1]")
slug=$(node -p "require('./packages/brand/brand.json').slug")

for kind in "$@"; do
  case "$kind" in
    appimage)
      step build the AppImage
      timeout 5400 node tools/build-release.mjs --unsigned -- --bundles appimage
      cp target/release/bundle/appimage/*.AppImage /out/
      ;;
    apk)
      step build the APK
      # the version Android reads, as android.yml writes it: the cli does not,
      # and gradle falls back to 1, older than any build already installed
      printf 'tauri.android.versionName=%s\ntauri.android.versionCode=%s\n' \
        "$version" "$(node tools/version.mjs android-code)" \
        > apps/client/src-tauri/gen/android/app/tauri.properties
      timeout 5400 pnpm --filter @app/client tauri android build --debug --target aarch64 armv7 x86_64 --apk
      cp apps/client/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk \
        "/out/$slug-$version-android-debug.apk"
      ;;
    *)
      echo "unknown build: $kind (appimage or apk)" >&2
      exit 1
      ;;
  esac
done

chown -R "$HOST_UID:$HOST_GID" /out
step done
ls -la /out
