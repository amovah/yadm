#!/usr/bin/env bash
# Builds YADM for Linux inside Docker, so no -dev packages touch the host.
# Output: dist/YADM-linux/{yadm,extension/,aria2c*}
#   * aria2c is copied from the build image only if all its libraries exist
#     on this machine; otherwise install aria2 (apt install aria2).
#
#   scripts/build-linux.sh            # release build
#   PROFILE=dev scripts/build-linux.sh
#   docker rmi yadm-build             # remove the build image afterwards
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
IMAGE="${IMAGE:-yadm-build}"
PROFILE="${PROFILE:-release}"
TARGET="$ROOT/target/docker"

docker build -t "$IMAGE" scripts/linux

# Run as the calling user so build files aren't owned by root. Cargo's
# download cache lives under target/docker so rebuilds are incremental.
mkdir -p "$TARGET/cargo"
docker run --rm \
  -u "$(id -u):$(id -g)" \
  -e HOME=/tmp \
  -e CARGO_HOME=/src/target/docker/cargo \
  -e CARGO_TARGET_DIR=/src/target/docker \
  -v "$ROOT":/src -w /src \
  "$IMAGE" cargo build --profile "$PROFILE" -p yadm-app

OUT_DIR="$([ "$PROFILE" = dev ] && echo debug || echo "$PROFILE")"
DIST="$ROOT/dist/YADM-linux"
rm -rf "$DIST"
mkdir -p "$DIST"
cp "$TARGET/$OUT_DIR/yadm" "$DIST/yadm"
cp -R extension "$DIST/extension"

# Bundle the engine next to the binary (YADM looks there first) when it runs here.
docker run --rm -v "$DIST":/out "$IMAGE" sh -c "cp /opt/aria2c /out/ && chown $(id -u):$(id -g) /out/aria2c"
if ldd "$DIST/aria2c" | grep -q "not found"; then
  echo "note: bundled aria2c is missing libraries on this machine, removed it:" >&2
  ldd "$DIST/aria2c" | grep "not found" >&2
  rm "$DIST/aria2c"
  command -v aria2c >/dev/null || echo "note: install aria2 (sudo apt install aria2) or set YADM_ARIA2C" >&2
fi

if ldd "$DIST/yadm" | grep -q "not found"; then
  echo "warning: yadm needs libraries this machine lacks:" >&2
  ldd "$DIST/yadm" | grep "not found" >&2
fi
echo "Built $DIST — run $DIST/yadm"
