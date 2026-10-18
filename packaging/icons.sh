#!/usr/bin/env bash
# Regenerate the Windows and Linux Pdf app icons from the project source PNG.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/assets/app-icon"
SOURCE="$DIR/pdf-icon-source.png"
ID="io.github.juanjoic005.pdf"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

command -v ffmpeg >/dev/null || { echo "error: ffmpeg is required" >&2; exit 1; }

# Crop the generator canvas and enforce a clean rounded-square alpha mask.
ffmpeg -hide_banner -loglevel error -y -i "$SOURCE" \
  -vf "crop=1120:1120:80:80,scale=1024:1024:flags=lanczos,format=rgba,geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='if(lte(hypot(max(abs(X-500)-380\\,0)\\,max(abs(Y-489)-361\\,0))\\,120)\\,alpha(X,Y)\\,0)'" \
  -frames:v 1 "$DIR/pdfcraft-1024.png"

for size in 16 24 32 48 64 128 256 512; do
  ffmpeg -hide_banner -loglevel error -y -i "$DIR/pdfcraft-1024.png" \
    -vf "scale=${size}:${size}:flags=lanczos" "$DIR/hicolor/${size}x${size}/apps/$ID.png"
done

pngs=()
for size in 16 20 24 32 40 48 64 128 256; do
  png="$TMP/pdf-$size.png"
  ffmpeg -hide_banner -loglevel error -y -i "$DIR/pdfcraft-1024.png" \
    -vf "scale=${size}:${size}:flags=lanczos" "$png"
  pngs+=("$png")
done

python3 - "$DIR/pdfcraft.ico" "${pngs[@]}" <<'PY'
import struct, sys
out, pngs = sys.argv[1], sys.argv[2:]
blobs = [open(path, "rb").read() for path in pngs]
head = struct.pack("<HHH", 0, 1, len(blobs))
entries, data, offset = b"", b"", 6 + 16 * len(blobs)
for blob in blobs:
    width, height = struct.unpack(">II", blob[16:24])
    entries += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(blob), offset)
    data += blob
    offset += len(blob)
open(out, "wb").write(head + entries + data)
PY

echo "Windows and Linux Pdf icons written to $DIR"
