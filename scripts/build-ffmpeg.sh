#!/usr/bin/env bash
# Builds the ffmpeg and ffprobe sidecars from source, GPL and redistributable.
#
# Unlike fetch-ffmpeg.sh, which downloads a prebuilt binary compiled
# --enable-nonfree and therefore not redistributable, this produces a binary you
# can ship. It also drops nineteen external libraries Carta never touches, so it
# comes out at half the size.
#
# Needs: pkg-config, make, clang, git, curl. Only pkg-config is likely missing:
#     brew install pkg-config
set -euo pipefail

FFMPEG_VERSION="8.1.2"
# x264 has no releases, so the commit is pinned to keep the build reproducible.
X264_COMMIT="0480cb05fa188d37ae87e8f4fd8f1aea3711f7ee"

TRIPLE="aarch64-apple-darwin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/binaries"
WORK="$(mktemp -d)"
PREFIX="$WORK/out"
trap 'rm -rf "$WORK"' EXIT

command -v pkg-config >/dev/null || {
  echo "pkg-config is missing. Install it with:  brew install pkg-config" >&2
  exit 1
}

mkdir -p "$PREFIX" "$DEST"
cd "$WORK"

echo "==> x264 ${X264_COMMIT:0:8}"
git clone https://code.videolan.org/videolan/x264.git
cd x264
git checkout --quiet "$X264_COMMIT"
./configure --prefix="$PREFIX" --enable-static --disable-cli --disable-opencl >/dev/null
make -j"$(sysctl -n hw.ncpu)" >/dev/null
make install >/dev/null
cd "$WORK"

echo "==> ffmpeg $FFMPEG_VERSION"
curl -fsSL -o ffmpeg.tar.xz "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
tar xf ffmpeg.tar.xz
cd "ffmpeg-$FFMPEG_VERSION"

# Everything native stays on: the decoders, encoders and muxers Carta relies on
# are all built in. Only libx264 comes from outside, for the quality option.
#
# libxcb and xlib are disabled explicitly. configure picks them up when Homebrew
# has them installed and links the X11 screen-capture input, which pulls in
# dylibs under /opt/homebrew and destroys portability.
PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" ./configure \
  --prefix="$PREFIX" \
  --pkg-config-flags=--static \
  --extra-cflags="-I$PREFIX/include" \
  --extra-ldflags="-L$PREFIX/lib" \
  --enable-gpl \
  --enable-libx264 \
  --enable-videotoolbox \
  --enable-audiotoolbox \
  --disable-doc \
  --disable-ffplay \
  --disable-network \
  --disable-debug \
  --disable-libxcb \
  --disable-xlib \
  --disable-sdl2 >/dev/null
make -j"$(sysctl -n hw.ncpu)" >/dev/null

cp ffmpeg "$DEST/ffmpeg-$TRIPLE"
cp ffprobe "$DEST/ffprobe-$TRIPLE"
chmod +x "$DEST/ffmpeg-$TRIPLE" "$DEST/ffprobe-$TRIPLE"

echo "==> checking portability"
for tool in "$DEST/ffmpeg-$TRIPLE" "$DEST/ffprobe-$TRIPLE"; do
  if otool -L "$tool" | tail -n +2 | grep -qv "/System/\|/usr/lib/"; then
    echo "WARNING: $tool links something outside the system:" >&2
    otool -L "$tool" | tail -n +2 | grep -v "/System/\|/usr/lib/" >&2
    exit 1
  fi
done

echo
echo "Done. GPL v2+, redistributable:"
ls -lh "$DEST"
