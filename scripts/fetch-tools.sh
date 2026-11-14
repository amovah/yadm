#!/usr/bin/env bash
# Downloads the pinned yt-dlp and Deno release binaries for bundling and
# checks them against the projects' published SHA-256 sums.
#   macOS: vendor/{yt-dlp,deno} — Deno for $ARCHS (default: universal)
#   Linux: vendor/linux/{yt-dlp,deno} — x86_64
# Windows does the same in scripts/bundle-windows.ps1.
set -euo pipefail

YTDLP_VERSION="${YTDLP_VERSION:-2026.10.26}"
DENO_VERSION="${DENO_VERSION:-2.9.8}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

die() { echo "error: $*" >&2; exit 1; }
fetch() { curl -fsSL --retry 3 -o "$2" "$1" || die "download failed: $1"; }
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
verify() { # file expected-hash
  local got; got="$(sha256 "$1")"
  [ -n "$2" ] && [ "$got" = "$(echo "$2" | tr 'A-F' 'a-f')" ] || die "checksum mismatch for $(basename "$1")"
}

case "$(uname -s)" in
  Darwin)
    OUT="$ROOT/vendor"; YTDLP_ASSET=yt-dlp_macos; DENO_OS=apple-darwin
    ARCHS="${ARCHS:-arm64 x86_64}" ;;
  Linux)
    OUT="$ROOT/vendor/linux"; YTDLP_ASSET=yt-dlp_linux; DENO_OS=unknown-linux-gnu
    ARCHS=x86_64 ;;
  *) die "unsupported OS; on Windows use scripts/bundle-windows.ps1" ;;
esac
mkdir -p "$OUT"

# yt-dlp: one standalone executable (universal on macOS).
base="https://github.com/yt-dlp/yt-dlp/releases/download/$YTDLP_VERSION"
fetch "$base/SHA2-256SUMS" "$WORK/yt-dlp.sums"
fetch "$base/$YTDLP_ASSET" "$WORK/$YTDLP_ASSET"
verify "$WORK/$YTDLP_ASSET" "$(awk -v f="$YTDLP_ASSET" '$2 == f { print $1 }' "$WORK/yt-dlp.sums")"
install -m 755 "$WORK/$YTDLP_ASSET" "$OUT/yt-dlp"
echo "yt-dlp $("$OUT/yt-dlp" --version 2>/dev/null || echo "$YTDLP_VERSION") → $OUT/yt-dlp"

# Deno: one zip per architecture, merged into a universal binary when several.
parts=()
for arch in $ARCHS; do
  case "$arch" in
    arm64 | aarch64) cpu=aarch64 ;;
    x86_64) cpu=x86_64 ;;
    *) die "unsupported arch $arch" ;;
  esac
  zip="deno-$cpu-$DENO_OS.zip"
  url="https://github.com/denoland/deno/releases/download/v$DENO_VERSION/$zip"
  fetch "$url" "$WORK/$zip"
  fetch "$url.sha256sum" "$WORK/$zip.sha256sum"
  verify "$WORK/$zip" "$(grep -oE '[0-9a-fA-F]{64}' "$WORK/$zip.sha256sum" | head -1)"
  mkdir -p "$WORK/$cpu"
  unzip -q -o "$WORK/$zip" -d "$WORK/$cpu"
  parts+=("$WORK/$cpu/deno")
done
if [ "${#parts[@]}" -gt 1 ]; then
  lipo -create "${parts[@]}" -output "$OUT/deno"
  chmod 755 "$OUT/deno"
else
  install -m 755 "${parts[0]}" "$OUT/deno"
fi
echo "deno $DENO_VERSION ($ARCHS) → $OUT/deno"
