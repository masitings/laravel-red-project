#!/usr/bin/env bash
# Build PostgreSQL <version> and pack it as a RED runtime archive: the server
# (bin/postgres, initdb, pg_ctl), the client tools (psql, pg_dump,
# pg_restore, ...), libpq and the contrib extensions.
#
#   scripts/runtimes/build-postgres.sh 18.1 macos-aarch64 out/
#
# The tree is relocatable: PostgreSQL finds share/ and lib/ relative to its
# binaries, and the binaries find libpq through @loader_path (macOS) or
# $ORIGIN (Linux). Only system libraries are linked: zlib, libuuid (Linux)
# and libedit (macOS, for psql's line editing). ICU, OpenSSL and readline
# are left out, so pgcrypto and sslinfo are not built. Building needs bison
# and flex (the source tarballs no longer carry the generated parsers).
set -euo pipefail

version=$1
platform=$2
out=$(mkdir -p "$3" && cd "$3" && pwd)
work=${RUNNER_TEMP:-/tmp}/red-postgres-$version-$platform
prefix=/opt/red-postgres
mkdir -p "$work"
cd "$work"

configure=(--prefix="$prefix" --without-icu --with-zlib --with-uuid=e2fs --disable-rpath)
if [[ $platform == macos-* ]]; then
  export MACOSX_DEPLOYMENT_TARGET=12.0
  # Room in the headers for the @loader_path names set below.
  export LDFLAGS="${LDFLAGS:-} -Wl,-headerpad_max_install_names"
  configure+=(--with-libedit-preferred)
else
  if command -v apt-get >/dev/null && [ -n "${CI:-}" ]; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq bison flex zlib1g-dev uuid-dev patchelf >/dev/null
  fi
  # GNU readline would have to be on every user's machine; psql works
  # without line editing.
  configure+=(--without-readline)
fi

if [ ! -d src ]; then
  curl -fsSL --retry 3 -o src.tar.gz \
    "https://ftp.postgresql.org/pub/source/v$version/postgresql-$version.tar.gz"
  mkdir src
  tar -xzf src.tar.gz -C src --strip-components=1
fi

jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)
cd src
[ -f config.status ] || ./configure "${configure[@]}" >/dev/null
make -s -j"$jobs" >/dev/null
make -s -C contrib -j"$jobs" >/dev/null
rm -rf "$work/stage"
make -s install DESTDIR="$work/stage" >/dev/null
make -s -C contrib install DESTDIR="$work/stage" >/dev/null
cd "$work"

rm -rf rt
mv "stage$prefix" rt
# Drop what a local server does not need.
rm -rf rt/include rt/lib/pkgconfig rt/share/man rt/share/doc
find rt/lib -name '*.a' -delete

# Point the binaries and extensions at the libpq next to them.
if [[ $platform == macos-* ]]; then
  find rt -type f \( -perm -u+x -o -name '*.dylib' -o -name '*.so' \) -print0 |
    while IFS= read -r -d '' file; do
      file "$file" | grep -q Mach-O || continue
      dir=$(dirname "$file")
      rel=$(python3 -c 'import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' rt/lib "$dir")
      changed=0
      for dep in $(otool -L "$file" | awk 'NR>1 {print $1}' | grep "^$prefix/lib/" || true); do
        install_name_tool -change "$dep" "@loader_path/$rel/$(basename "$dep")" "$file"
        changed=1
      done
      # Shared libraries (libpq, ...) get a relative id; extensions are
      # bundles and have none.
      if otool -hv "$file" | grep -qw DYLIB; then
        install_name_tool -id "@rpath/$(basename "$file")" "$file"
        changed=1
      fi
      if [ "$changed" = 1 ]; then
        codesign --force --sign - "$file" 2>/dev/null
      fi
    done
  if otool -L rt/bin/* rt/lib/*.dylib 2>/dev/null | grep -q "$prefix"; then
    echo "absolute $prefix references left:" >&2
    otool -L rt/bin/* rt/lib/*.dylib | grep "$prefix" >&2
    exit 1
  fi
else
  # shellcheck disable=SC2016 # $ORIGIN is for the dynamic loader
  # (Extensions sit in lib/ itself: the prefix contains "postgres".)
  for file in rt/bin/* rt/lib/*.so*; do
    [ -f "$file" ] && [ ! -L "$file" ] || continue
    head -c 4 "$file" | grep -q ELF || continue
    case $file in
      rt/bin/*) patchelf --set-rpath '$ORIGIN/../lib' "$file" ;;
      *) patchelf --set-rpath '$ORIGIN' "$file" ;;
    esac
  done
fi

rt/bin/postgres --version
rt/bin/psql --version

# The server, the client tools and the extensions have to work from a copy
# somewhere else.
tmp=$(mktemp -d)
cp -R rt "$tmp/pg"
chmod -R a+rX "$tmp"
run=()
if [ "$(id -u)" = 0 ]; then
  # initdb refuses to run as root.
  chown -R nobody "$tmp"
  run=(su nobody -s /bin/sh -c)
else
  run=(sh -c)
fi
port=$((20000 + RANDOM % 10000))
"${run[@]}" "
  set -e
  cd '$tmp'
  pg/bin/initdb -D data -U root --auth=trust >/dev/null
  pg/bin/pg_ctl -D data -o '-p $port -k $tmp -c listen_addresses=' -l log -w start >/dev/null
  trap 'pg/bin/pg_ctl -D data -m fast stop >/dev/null' EXIT
  pg/bin/psql -h '$tmp' -p $port -U root -d postgres -qAt -v ON_ERROR_STOP=1 \
    -c 'CREATE DATABASE red_check' >/dev/null
  pg/bin/psql -h '$tmp' -p $port -U root -d red_check -qAt -v ON_ERROR_STOP=1 \
    -c 'CREATE EXTENSION \"uuid-ossp\"' -c 'CREATE EXTENSION pg_trgm' \
    -c 'CREATE EXTENSION hstore' -c 'CREATE EXTENSION citext' \
    -c 'CREATE TABLE t AS SELECT uuid_generate_v4() AS id' >/dev/null
  pg/bin/psql -h '$tmp' -p $port -U root -d red_check -qAt -v ON_ERROR_STOP=1 \
    -c 'CREATE EXTENSION dblink' -c 'CREATE EXTENSION postgres_fdw' >/dev/null
  pg/bin/pg_dump -h '$tmp' -p $port -U root -Fc red_check -f dump
  pg/bin/pg_restore -l dump | grep -q 'TABLE public t'
"
rm -rf "$tmp"

name="postgres-$version-$platform.tar.gz"
tar -C rt -czf "$out/$name" .
sha=$( (sha256sum "$out/$name" 2>/dev/null || shasum -a 256 "$out/$name") | cut -d' ' -f1)
line=$(echo "$version" | cut -d. -f1)
printf '{"kind":"postgres","line":"%s","version":"%s","platform":"%s","file":"%s","sha256":"%s"}\n' \
  "$line" "$version" "$platform" "$name" "$sha" > "$out/postgres-$version-$platform.json"
echo "built $name ($sha)"
