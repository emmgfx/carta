#!/usr/bin/env bash
# Downloads static arm64 ffmpeg and ffprobe and names them the way Tauri expects
# its sidecars (bundle.externalBin in tauri.conf.json).
#
# WARNING: the build shipped by ffmpeg-static is compiled with --enable-gpl and
# --enable-nonfree, so the binary is NOT redistributable. It is fine for
# building and running the app on your own machine. To publish binaries you must
# replace it with your own LGPL build (--disable-gpl --disable-nonfree, without
# libx264, leaving encoding to h264_videotoolbox). See THIRD-PARTY.md.
set -euo pipefail

TRIPLE="aarch64-apple-darwin"
DEST="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$DEST"
cd "$TMP"
npm init -y >/dev/null 2>&1
npm install ffmpeg-static @ffprobe-installer/darwin-arm64 >/dev/null

cp node_modules/ffmpeg-static/ffmpeg "$DEST/ffmpeg-$TRIPLE"
cp node_modules/@ffprobe-installer/darwin-arm64/ffprobe "$DEST/ffprobe-$TRIPLE"
chmod +x "$DEST"/ffmpeg-* "$DEST"/ffprobe-*

echo "Done:"
ls -lh "$DEST"
