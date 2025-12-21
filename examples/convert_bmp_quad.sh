#!/bin/bash

# Script to convert JPG/PNG images to 4-color dithered BMP for JD79661 e-Ink displays
# Requires ImageMagick (magick)

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <input_image> <output_bmp>"
    echo "Example: $0 mocha250x122.jpg mocha250x122.bmp"
    exit 1
fi

INPUT=$1
OUTPUT=$2
PALETTE="/tmp/palette_quad.png"

# Check if magick is installed
if ! command -v magick &> /dev/null; then
    echo "Error: ImageMagick (magick) is not installed."
    echo "Install it using: brew install imagemagick"
    exit 1
fi

echo "Creating palette (White, Black, Red, Yellow)..."
magick -size 1x4 xc:white xc:black xc:red xc:yellow +append "$PALETTE"

echo "Converting $INPUT to $OUTPUT (250x122, dithered, 4-color)..."
# -resize 250x122^ -gravity center -extent 250x122: Ensures the image fits the display
# +dither -remap: Applies dithering and maps to our palette
magick "$INPUT" \
    -resize 250x122^ -gravity center -extent 250x122 \
    +dither -remap "$PALETTE" \
    -type palette -colors 4 \
    "$OUTPUT"

echo "Done! Final colors in $OUTPUT:"
magick "$OUTPUT" -colors 4 -depth 8 -format "%c" histogram:info:

rm "$PALETTE"
