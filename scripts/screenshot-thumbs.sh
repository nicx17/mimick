#!/bin/bash
# Rebuilds the WebP thumbnails the website shows instead of the full-size screenshots.
# Run it after adding, replacing, or removing anything in docs/screenshots.
set -euo pipefail

cd "$(dirname "$0")/../docs/screenshots"

rm -rf thumbs
mkdir thumbs
for png in *.png; do
    # 1200 px keeps a 400 px card sharp at 2x, tall shots included; clicking still opens the PNG.
    magick "$png" -resize '1200x1200>' -quality 80 "thumbs/${png%.png}.webp"
done
echo "Wrote $(ls thumbs | wc -l) thumbnails ($(du -sh thumbs | cut -f1))"
