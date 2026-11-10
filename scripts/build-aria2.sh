#!/usr/bin/env bash
# Builds a self-contained aria2c for macOS that links only system libraries
# (AppleTLS for HTTPS, system zlib), suitable for bundling inside YADM.app.
# Output: vendor/aria2c
set -euo pipefail

VERSION="${ARIA2_VERSION:-1.37.0}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${TMPDIR:-/tmp}/yadm-aria2-build"
OUT="$ROOT/vendor/aria2c"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"

mkdir -p "$WORK" "$ROOT/vendor"
cd "$WORK"
if [ ! -d "aria2-$VERSION" ]; then
  curl -fsSL "https://github.com/aria2/aria2/releases/download/release-$VERSION/aria2-$VERSION.tar.xz" -o aria2.tar.xz
  tar xf aria2.tar.xz
fi
cd "aria2-$VERSION"

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
# Universal binary unless ARCHS is overridden (e.g. ARCHS="arm64").
ARCHS="${ARCHS:-arm64 x86_64}"
ARCH_FLAGS=""
for a in $ARCHS; do ARCH_FLAGS="$ARCH_FLAGS -arch $a"; done

./configure \
  --disable-dependency-tracking \
  --with-appletls \
  --without-gnutls --without-openssl --without-libgcrypt --without-libnettle \
  --without-libxml2 --without-libexpat \
  --without-libcares --without-libssh2 --without-sqlite3 \
  --without-libuv \
  --enable-bittorrent \
  ARIA2_STATIC=no \
  CXXFLAGS="-O2 $ARCH_FLAGS" CFLAGS="-O2 $ARCH_FLAGS" LDFLAGS="$ARCH_FLAGS" \
  > configure.log

make -j"$JOBS" > make.log
cp src/aria2c "$OUT"
strip -x "$OUT"

echo "Built $OUT"
"$OUT" --version | head -1
echo "Linked libraries:"
otool -L "$OUT" | tail -n +2
if otool -L "$OUT" | tail -n +2 | grep -vqE '^\s*/(usr/lib|System/Library)/'; then
  echo "error: aria2c links non-system libraries" >&2
  exit 1
fi
