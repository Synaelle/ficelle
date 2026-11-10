#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds the client in Docker from this checkout, without any signing key.
#   tools/docker/build.sh [appimage] [apk]    both when none is named
# The files land in target/docker, or in OUT_DIR when it is set. DOCKER picks
# the command, like DOCKER="sudo docker". The caches stay in the volume
# ficelle-build-cache. The debug Android builds link large libraries: the
# container gets BUILD_MEMORY (8g) and half the cores, so a build that runs
# out is stopped on its own instead of taking the machine down with it.
set -euo pipefail

here=$(dirname "$(readlink -f "$0")")
root=$(git -C "$here" rev-parse --show-toplevel)
out=${OUT_DIR:-$root/target/docker}
docker=${DOCKER:-docker}
memory=${BUILD_MEMORY:-8g}
jobs=$(( ($(nproc) + 1) / 2 ))
[ $# -gt 0 ] || set -- appimage apk

echo "== the build image $(date +%T)"
$docker build -t ficelle-build -f "$here/Dockerfile" "$here"
mkdir -p "$out"
$docker run --rm --memory "$memory" --memory-swap "$memory" -e CARGO_BUILD_JOBS="$jobs" \
  -v "$root:/src:ro" -v "$(readlink -f "$out"):/out" -v ficelle-build-cache:/cache \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  -e APP_COMMIT="$(git -C "$root" rev-parse --short=9 HEAD)" \
  -e APP_COMMIT_DATE="$(git -C "$root" show -s --format=%cs HEAD)" \
  ficelle-build bash /src/tools/docker/inside.sh "$@" || {
  status=$?
  [ "$status" != 137 ] || echo "stopped: the build needed more than $memory, set BUILD_MEMORY higher" >&2
  exit "$status"
}
