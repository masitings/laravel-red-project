#!/usr/bin/env bash
# Build Valkey <version> and pack it as a RED runtime archive:
# bin/valkey-server and bin/valkey-cli at the archive's top level.
#
#   scripts/runtimes/build-valkey.sh 8.1.4 macos-aarch64 out/
#
# Built without TLS so the binaries only need system libraries.
set -euo pipefail

version=$1
platform=$2
out=$(mkdir -p "$3" && cd "$3" && pwd)
work=${RUNNER_TEMP:-/tmp}/red-valkey-$version-$platform
mkdir -p "$work"
cd "$work"

rm -rf src-tree
git clone --quiet --depth 1 --branch "$version" https://github.com/valkey-io/valkey.git src-tree
cd src-tree
export MACOSX_DEPLOYMENT_TARGET=12.0
jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)
make -j"$jobs" BUILD_TLS=no
./src/valkey-server --version

rm -rf ../rt && mkdir -p ../rt/bin
cp src/valkey-server src/valkey-cli ../rt/bin/
cd ..
line=$(echo "$version" | cut -d. -f1-2)
name="valkey-$version-$platform.tar.gz"
tar -C rt -czf "$out/$name" bin
sha=$( (sha256sum "$out/$name" 2>/dev/null || shasum -a 256 "$out/$name") | cut -d' ' -f1)
printf '{"kind":"valkey","line":"%s","version":"%s","platform":"%s","file":"%s","sha256":"%s"}\n' \
  "$line" "$version" "$platform" "$name" "$sha" > "$out/valkey-$version-$platform.json"
echo "built $name ($sha)"
