#!/usr/bin/env bash
# Descarga ffmpeg y ffprobe estáticos para arm64 y los deja con el nombre que
# espera Tauri para los sidecars (bundle.externalBin en tauri.conf.json).
#
# AVISO: el build que trae ffmpeg-static está compilado con --enable-gpl y
# --enable-nonfree, así que el binario NO es redistribuible. Sirve para
# compilar y usar la app en tu propia máquina. Para publicar binarios hay que
# sustituirlo por un build LGPL propio (--disable-gpl --disable-nonfree, sin
# libx264, dejando la codificación en h264_videotoolbox). Ver THIRD-PARTY.md.
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

echo "Listo:"
ls -lh "$DEST"
