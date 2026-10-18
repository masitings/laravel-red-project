#!/usr/bin/env bash
# Build PHP <line> (CLI + FPM) with static-php-cli and pack it as a RED
# runtime archive: bin/php and sbin/php-fpm at the archive's top level.
#
#   scripts/runtimes/build-php.sh 8.4 macos-aarch64 out/
#
# Writes out/php-<version>-<platform>.tar.gz and out/php-<version>-<platform>.json
# (the manifest entry, without the download URL).
set -euo pipefail

line=$1
platform=$2
out=$(mkdir -p "$3" && cd "$3" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
extensions=$(tr -d ' \n' < "$here/php-extensions.txt")
work=${RUNNER_TEMP:-/tmp}/red-php-$line-$platform
mkdir -p "$work"
cd "$work"

if [ ! -x spc ]; then
  curl -fsSL --retry 3 -o spc.tar.gz \
    "https://github.com/crazywhalecc/static-php-cli/releases/latest/download/spc-$platform.tar.gz" \
    && tar xzf spc.tar.gz \
    || curl -fsSL --retry 3 -o spc "https://dl.static-php.dev/static-php-cli/spc-bin/nightly/spc-$platform"
  chmod +x spc
fi

./spc doctor --auto-fix
./spc download --with-php="$line" --for-extensions="$extensions" --prefer-pre-built --retry=3
if ! ./spc build --build-cli --build-fpm "$extensions"; then
  # spc only prints the failing command; show the compiler's actual errors.
  echo "::group::compiler errors (log/spc.shell.log)"
  grep -n -E "error:|Error [0-9]+|undefined symbol|ld: " log/spc.shell.log | tail -60 || true
  echo "::endgroup::"
  exit 1
fi

version=$(buildroot/bin/php -r 'echo PHP_VERSION;')
case "$version" in
  "$line".*) ;;
  *) echo "built PHP $version, expected $line.x" >&2; exit 1 ;;
esac
# Capture once: `php -m | grep -q` lets grep exit early, php then dies of
# SIGPIPE and pipefail reports a missing extension that is really there.
modules=$(buildroot/bin/php -n -m)
printf '%s\n' "$modules"
for ext in pdo_sqlite pdo_pgsql pdo_mysql intl redis; do
  grep -qx "$ext" <<<"$modules" || { echo "missing extension $ext" >&2; exit 1; }
done

rm -rf rt && mkdir -p rt/bin rt/sbin
cp buildroot/bin/php rt/bin/php
cp buildroot/bin/php-fpm rt/sbin/php-fpm
name="php-$version-$platform.tar.gz"
tar -C rt -czf "$out/$name" bin sbin
sha=$( (sha256sum "$out/$name" 2>/dev/null || shasum -a 256 "$out/$name") | cut -d' ' -f1)
printf '{"kind":"php","line":"%s","version":"%s","platform":"%s","file":"%s","sha256":"%s"}\n' \
  "$line" "$version" "$platform" "$name" "$sha" > "$out/php-$version-$platform.json"
echo "built $name ($sha)"
