#!/bin/bash

# Script to convert JPG/PNG images to 3-color dithered BMP for SSD1681 e-Ink displays
# Requires ImageMagick (magick)

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <input_image> <output_bmp>"
    echo "Example: $0 mocha200x200.jpg mocha200x200.bmp"
    exit 1
fi

INPUT=$1
OUTPUT=$2
PALETTE="/tmp/palette_tri.png"

# Check if magick is installed
if ! command -v magick &> /dev/null; then
    echo "Error: ImageMagick (magick) is not installed."
    echo "Install it using: brew install imagemagick"
    exit 1
fi

echo "Creating palette..."
magick -size 1x3 xc:white xc:black xc:red +append "$PALETTE"

echo "Converting $INPUT to $OUTPUT (200x200, dithered, 3-color)..."
# -resize 200x200^ -gravity center -extent 200x200: Ensures the image fits the display
# +dither -remap: Applies dithering and maps to our palette
magick "$INPUT" \
    -resize 200x200^ -gravity center -extent 200x200 \
    +dither -remap "$PALETTE" \
    "$OUTPUT"

echo "Done! Final colors in $OUTPUT:"
magick "$OUTPUT" -colors 4 -depth 8 -format "%c" histogram:info:

rm "$PALETTE"
