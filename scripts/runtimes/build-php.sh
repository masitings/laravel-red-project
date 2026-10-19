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

# PHP 8.1's bcmath still has K&R-style function definitions, which the
# newer Apple clang rejects; build it as C17. (spc lets the environment
# override its env.ini; this is its macOS default plus -std=gnu17.)
if [ "$line" = "8.1" ] && [[ $platform == macos-* ]]; then
  arch=${platform#macos-}
  [ "$arch" = aarch64 ] && arch=arm64
  export SPC_CMD_VAR_PHP_MAKE_EXTRA_CFLAGS="-g -fstack-protector-strong -fpic -fpie -Werror=unknown-warning-option --target=$arch-apple-darwin -Os -std=gnu17"
fi

# Xdebug can only be a shared extension, and the default Linux build is
# fully static (musl) and cannot load one, so it is built on macOS only.
shared=()
downloads="$extensions"
if [[ $platform == macos-* ]]; then
  shared=(--build-shared=xdebug)
  downloads="$extensions,xdebug"
fi

show_errors() {
  # spc only prints the failing command; show the compiler's actual errors.
  echo "::group::compiler errors (log/spc.shell.log)"
  grep -n -E "error:|Error [0-9]+|undefined symbol|ld: " log/spc.shell.log | tail -60 || true
  echo "::endgroup::"
}

./spc doctor --auto-fix
./spc download --with-php="$line" --for-extensions="$downloads" --prefer-pre-built --retry=3
if ! ./spc build --build-cli --build-fpm "$extensions" "${shared[@]}"; then
  show_errors
  if [ ${#shared[@]} -eq 0 ]; then
    exit 1
  fi
  # Ship PHP without Xdebug rather than not at all.
  echo "::warning::building PHP $line with Xdebug failed; building it without"
  shared=()
  ./spc build --build-cli --build-fpm "$extensions" || { show_errors; exit 1; }
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

xdebug=""
if [ ${#shared[@]} -gt 0 ]; then
  xdebug=$(find buildroot -name xdebug.so | head -1)
  if [ -n "$xdebug" ] && buildroot/bin/php -n -d "zend_extension=$PWD/$xdebug" \
      -r 'exit(extension_loaded("xdebug") ? 0 : 1);'; then
    echo "Xdebug: $xdebug"
  else
    echo "::warning::xdebug.so was not built or does not load; shipping PHP $line without it"
    xdebug=""
  fi
fi

rm -rf rt && mkdir -p rt/bin rt/sbin
cp buildroot/bin/php rt/bin/php
cp buildroot/bin/php-fpm rt/sbin/php-fpm
name="php-$version-$platform.tar.gz"
dirs=(bin sbin)
if [ -n "$xdebug" ]; then
  mkdir -p rt/lib
  cp "$xdebug" rt/lib/xdebug.so
  dirs+=(lib)
fi
tar -C rt -czf "$out/$name" "${dirs[@]}"
sha=$( (sha256sum "$out/$name" 2>/dev/null || shasum -a 256 "$out/$name") | cut -d' ' -f1)
printf '{"kind":"php","line":"%s","version":"%s","platform":"%s","file":"%s","sha256":"%s"}\n' \
  "$line" "$version" "$platform" "$name" "$sha" > "$out/php-$version-$platform.json"
echo "built $name ($sha)"
