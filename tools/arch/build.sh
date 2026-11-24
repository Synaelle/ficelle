#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Makes the Arch Linux package from a release's program, desktop entry and
# icons, with makepkg in an Arch container.
#   tools/arch/build.sh FILES VERSION
# FILES holds usr/bin/ficelle, usr/share/applications and usr/share/icons,
# as linux.yml uploads them. The package lands in target/arch, or in OUT_DIR
# when it is set, named Ficelle_VERSION_x86_64.pkg.tar.zst. DOCKER picks the
# command, like DOCKER="sudo docker".
set -euo pipefail

here=$(dirname "$(readlink -f "$0")")
root=$(git -C "$here" rev-parse --show-toplevel)
files=$(readlink -f "${1:?the folder of the files, then the version}")
version=${2:?the version, like 0.9.0}
out=${OUT_DIR:-$root/target/arch}
docker=${DOCKER:-docker}
image=archlinux:base-devel@sha256:949ecb1661609ab7e8f6854ba03efd62792e7a796313738e94b040c095c20d91
name="$(jq -r .displayName "$root/packages/brand/brand.json" | tr ' ' '-')_${version}_x86_64.pkg.tar.zst"

# a nightly's has its build number after the release's: 0.8.7.800802
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || { echo "not a release version: $version" >&2; exit 1; }
for path in usr/bin/ficelle usr/share/applications/Ficelle.desktop usr/share/icons; do
  [ -e "$files/$path" ] || { echo "missing in $files: $path" >&2; exit 1; }
done
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -r "$files/usr" "$work/usr"
sed "s/@VERSION@/$version/" "$here/PKGBUILD" > "$work/PKGBUILD"

echo "== makepkg $version $(date +%T)"
# makepkg refuses to run as root: a user of the host's id builds. The
# dependencies are the system's, so none is installed here (--nodeps)
timeout 600 $docker run --rm -v "$work:/build" -e HOST_UID="$(id -u)" "$image" bash -euc '
  useradd -u "$HOST_UID" -o -m builder
  chown -R builder /build
  cd /build
  su builder -c "makepkg --nodeps --noconfirm"
'
built=$(find "$work" -maxdepth 1 -name "ficelle-$version-1-x86_64.pkg.tar.zst")
[ -n "$built" ] || { echo "makepkg made no package" >&2; exit 1; }
mv "$built" "$out/$name"
echo "== $out/$name"
