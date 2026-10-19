#!/usr/bin/env bash
# Build MariaDB <version> and pack it as a RED runtime archive: bin/mariadbd,
# scripts/mariadb-install-db, share/ and the client tools, laid out like
# mariadb.org's own binary tarballs (relocatable through --basedir).
#
#   scripts/runtimes/build-mariadb.sh 11.4.13 macos-aarch64 out/
#
# mariadb.org publishes binaries for Linux x86-64 only; RED builds the other
# platforms. OpenSSL is linked statically and PCRE, zlib and fmt are bundled,
# so the binaries need only system libraries; storage engines a dev machine
# never uses are left out.
set -euo pipefail

version=$1
platform=$2
out=$(mkdir -p "$3" && cd "$3" && pwd)
work=${RUNNER_TEMP:-/tmp}/red-mariadb-$version-$platform
mkdir -p "$work"
cd "$work"

if [[ $platform == macos-* ]]; then
  # The system bison is too old.
  brew list bison >/dev/null 2>&1 || brew install bison
  brew list openssl@3 >/dev/null 2>&1 || brew install openssl@3
  export PATH="$(brew --prefix bison)/bin:$PATH"
  openssl_root=$(brew --prefix openssl@3)
  export MACOSX_DEPLOYMENT_TARGET=12.0
elif command -v apt-get >/dev/null && [ -n "${CI:-}" ]; then
  sudo apt-get update -qq
  sudo apt-get install -y -qq cmake bison libncurses-dev libssl-dev pkg-config >/dev/null
fi

if [ ! -d src ]; then
  curl -fsSL --retry 3 -o src.tar.gz \
    "https://archive.mariadb.org/mariadb-$version/source/mariadb-$version.tar.gz"
  mkdir src
  tar -xzf src.tar.gz -C src --strip-components=1
fi

off=()
for plugin in TOKUDB MROONGA SPIDER OQGRAPH ROCKSDB CONNECT S3 COLUMNSTORE SPHINX \
  FEDERATED FEDERATEDX ARCHIVE BLACKHOLE CASSANDRA HASHICORP_KEY_MANAGEMENT DUCKDB \
  AUTH_GSSAPI AUTH_PAM; do
  off+=("-DPLUGIN_$plugin=NO")
done

cmake -S src -B build \
  -DCMAKE_BUILD_TYPE=Release \
  -DINSTALL_LAYOUT=STANDALONE \
  -DCMAKE_INSTALL_PREFIX=/ \
  -DWITH_SSL=system \
  -DOPENSSL_USE_STATIC_LIBS=ON \
  ${openssl_root:+-DOPENSSL_ROOT_DIR="$openssl_root"} \
  -DWITH_PCRE=bundled \
  -DWITH_ZLIB=bundled \
  -DWITH_LIBFMT=bundled \
  -DWITH_UNIT_TESTS=OFF \
  -DWITH_EMBEDDED_SERVER=OFF \
  -DWITH_WSREP=OFF \
  -DWITH_SYSTEMD=no \
  -DWITH_MARIABACKUP=OFF \
  -DCONC_WITH_UNIT_TESTS=OFF \
  -DINSTALL_MYSQLTESTDIR= \
  -DINSTALL_SQLBENCHDIR= \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-}" \
  "${off[@]}"

jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)
cmake --build build --parallel "$jobs"

rm -rf rt
DESTDIR="$work/rt" cmake --install build >/dev/null
# Drop what a local server does not need.
rm -rf rt/mysql-test rt/sql-bench rt/man rt/docs rt/include rt/lib/plugin/*.a rt/lib/*.a

rt/bin/mariadbd --version
# The server has to start from the unpacked directory alone.
tmp=$(mktemp -d)
as_root=()
[ "$(id -u)" = 0 ] && as_root=(--user=root)
rt/scripts/mariadb-install-db --no-defaults --basedir="$work/rt" --datadir="$tmp/data" \
  --auth-root-authentication-method=normal --skip-test-db ${as_root[@]+"${as_root[@]}"} >/dev/null
rm -rf "$tmp"

line=$(echo "$version" | cut -d. -f1-2)
name="mariadb-$version-$platform.tar.gz"
tar -C rt -czf "$out/$name" .
sha=$( (sha256sum "$out/$name" 2>/dev/null || shasum -a 256 "$out/$name") | cut -d' ' -f1)
printf '{"kind":"mariadb","line":"%s","version":"%s","platform":"%s","file":"%s","sha256":"%s"}\n' \
  "$line" "$version" "$platform" "$name" "$sha" > "$out/mariadb-$version-$platform.json"
echo "built $name ($sha)"
