#!/usr/bin/env bash
# Builds a minimal LGPL ffmpeg + ffprobe for yt-dlp: enough to merge separate
# video/audio streams (remux, no video encoding) and to convert audio to MP3
# (static LAME). Links only system libraries, like scripts/build-aria2.sh.
#   macOS: vendor/{ffmpeg,ffprobe} — universal unless ARCHS is set (e.g. ARCHS=arm64)
#   Linux: vendor/linux/{ffmpeg,ffprobe} — run inside the yadm-build image
#          (scripts/build-linux.sh does this)
set -euo pipefail

FFMPEG_VERSION="${FFMPEG_VERSION:-9.0.2}"
LAME_VERSION="${LAME_VERSION:-3.100}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${WORK:-${TMPDIR:-/tmp}/yadm-ffmpeg-build}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"
OS="$(uname -s)"
if [ "$OS" = Darwin ]; then
  OUT="${OUT:-$ROOT/vendor}"
  ARCHS="${ARCHS:-arm64 x86_64}"
  export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
else
  OUT="${OUT:-$ROOT/vendor/linux}"
  ARCHS="$(uname -m)"
fi
mkdir -p "$WORK" "$OUT"
cd "$WORK"

if [ ! -d "ffmpeg-$FFMPEG_VERSION" ]; then
  curl -fsSL --retry 3 "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" | tar xJ
fi
if [ ! -d "lame-$LAME_VERSION" ]; then
  curl -fsSL --retry 3 "https://downloads.sourceforge.net/project/lame/lame/$LAME_VERSION/lame-$LAME_VERSION.tar.gz" | tar xz
  # Exported symbol that 3.100 lists but no longer defines (breaks macOS linking).
  sed -i.bak '/lame_init_old/d' "lame-$LAME_VERSION/include/libmp3lame.sym"
fi

HOST_ARCH="$(uname -m)"
built=()
for arch in $ARCHS; do
  prefix="$WORK/prefix-$arch"
  cflags="-O2"
  lame_host=()
  ff_cross=()
  if [ "$OS" = Darwin ]; then
    cflags="$cflags -arch $arch"
    if [ "$arch" != "$HOST_ARCH" ]; then
      lame_host=(--host="$([ "$arch" = arm64 ] && echo aarch64 || echo "$arch")-apple-darwin")
      ff_cross=(--enable-cross-compile --arch="$arch" --target-os=darwin)
    fi
  fi

  echo "== LAME $LAME_VERSION ($arch)"
  rm -rf "build-lame-$arch" && cp -R "lame-$LAME_VERSION" "build-lame-$arch"
  (
    cd "build-lame-$arch"
    ./configure --prefix="$prefix" --disable-shared --enable-static --disable-frontend \
      --disable-dependency-tracking ${lame_host[@]+"${lame_host[@]}"} CFLAGS="$cflags" LDFLAGS="$cflags" \
      > configure.log || { tail -40 configure.log >&2; exit 1; }
    make -j"$JOBS" install > make.log 2>&1 || { tail -40 make.log >&2; exit 1; }
  )

  echo "== FFmpeg $FFMPEG_VERSION ($arch)"
  rm -rf "build-ffmpeg-$arch" && mkdir "build-ffmpeg-$arch"
  (
    cd "build-ffmpeg-$arch"
    "../ffmpeg-$FFMPEG_VERSION/configure" \
      --prefix="$prefix" \
      ${ff_cross[@]+"${ff_cross[@]}"} \
      --cc="${CC:-cc}" \
      --extra-cflags="$cflags -I$prefix/include" \
      --extra-ldflags="$cflags -L$prefix/lib" \
      --disable-everything --disable-autodetect --disable-doc --disable-debug \
      --disable-network --disable-ffplay --disable-x86asm \
      --enable-small --enable-ffmpeg --enable-ffprobe \
      --enable-swresample --enable-swscale --enable-libmp3lame \
      --enable-protocol=file,pipe,concat \
      --enable-demuxer=mov,matroska,aac,mp3,ogg,flac,wav,mpegts,flv,concat,webvtt \
      --enable-muxer=mp4,ipod,mov,matroska,webm,mp3,ogg,opus,adts,flac,wav,mpegts,webvtt \
      --enable-parser=aac,h264,hevc,vp8,vp9,av1,opus,vorbis,mpegaudio,flac \
      --enable-bsf=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,vp9_superframe,extract_extradata,av1_frame_merge \
      --enable-decoder=aac,mp3,mp3float,opus,vorbis,flac,pcm_s16le,pcm_f32le,webvtt \
      --enable-encoder=libmp3lame,pcm_s16le,webvtt \
      --enable-filter=aresample,aformat,anull,null,format,copy,acopy \
      > configure.log || { tail -40 configure.log >&2; exit 1; }
    make -j"$JOBS" > make.log 2>&1 || { tail -40 make.log >&2; exit 1; }
    strip ffmpeg ffprobe 2>/dev/null || strip -x ffmpeg ffprobe
  )
  built+=("$arch")
done

for tool in ffmpeg ffprobe; do
  if [ "${#built[@]}" -gt 1 ]; then
    lipo -create $(for a in "${built[@]}"; do echo "build-ffmpeg-$a/$tool"; done) -output "$OUT/$tool"
  else
    cp "build-ffmpeg-${built[0]}/$tool" "$OUT/$tool"
  fi
  chmod 755 "$OUT/$tool"
done

echo "Built $OUT/ffmpeg and $OUT/ffprobe"
"$OUT/ffmpeg" -hide_banner -version | head -1
ls -lh "$OUT/ffmpeg" "$OUT/ffprobe"
if [ "$OS" = Darwin ]; then
  # Dependency lines are indented; "file (architecture x):" headers are not.
  if otool -L "$OUT/ffmpeg" "$OUT/ffprobe" | grep -E '^\s+/' | grep -vqE '^\s*/(usr/lib|System/Library)/'; then
    otool -L "$OUT/ffmpeg" >&2
    echo "error: ffmpeg links non-system libraries" >&2
    exit 1
  fi
fi
