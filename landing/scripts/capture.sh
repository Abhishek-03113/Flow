#!/usr/bin/env bash
# Regenerates landing/assets from the Flow UI on mock data. Needs node, ffmpeg (with libwebp)
# and a built UI served at $FLOW_UI_URL (default http://localhost:4173, i.e. `npm --prefix app/ui run preview`).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
assets="$here/../assets"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/st" "$work/vid" "$assets/shots" "$assets/motion"
node "$here/stills.mjs" "$work/st"
for t in light dark; do
  for f in home tray; do
    ffmpeg -v error -y -i "$work/st/$f-$t.png" -c:v libwebp -quality 84 "$assets/shots/$f-$t.webp"
  done
  # Just the performance cards from Diagnostics.
  ffmpeg -v error -y -i "$work/st/diagnostics-$t.png" -vf "crop=1600:440:520:120" -c:v libwebp -quality 86 \
    "$assets/shots/stats-$t.webp"
done

node "$here/motion.mjs" "$work/vid"
for list in "$work"/vid/*.txt; do
  n="$(basename "$list" .txt)"
  ffmpeg -v error -y -f concat -safe 0 -i "$list" \
    -vf "fps=30,scale=1650:-2:flags=lanczos,format=yuv420p" -c:v libx264 -preset slow -crf 24 \
    -movflags +faststart -an "$assets/motion/$n.mp4"
  ffmpeg -v error -y -i "$assets/motion/$n.mp4" -vframes 1 -c:v libwebp -quality 80 "$assets/motion/$n-poster.webp"
done
echo "Assets written to $assets"
