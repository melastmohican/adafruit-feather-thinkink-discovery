# adafruit-feather-thinkink-discovery

This project explores the **Adafruit RP2040 Feather ThinkInk** board using Rust. It provides a set of working examples adapted specifically for this hardware's integrated e-Ink display and onboard sensors/LEDs.

## Project Creation

The project was initialized using the `rp2040-project-template`:

```bash
cargo generate --git https://github.com/rp-rs/rp2040-project-template
🤷   Project Name: adafruit-feather-thinkink-discovery
🔧   Destination: ~/src/adafruit-feather-thinkink-discovery ...
🔧   project-name: adafruit-feather-thinkink-discovery ...
🔧   Generating template ...
✔ 🤷   Which flashing method do you intend to use? · picotool
[ 1/17]   Done: .cargo/config.toml
...
✨   Done! New project created ~/src/adafruit-feather-thinkink-discovery
```

## Hardware Supported

- **Board**: [Adafruit RP2040 Feather ThinkInk](https://www.adafruit.com/product/5087)
- **Display**: Integrated 1.54" Tri-Color (Red/Black/White) e-Paper display (SSD1681).
- **NeoPixel**: Onboard WS2812 (Power: GP20, Data: GP21).
- **LED**: Onboard Red LED (GP13).

## Examples

The following examples have been adapted for the ThinkInk hardware:

### 1. Blinky (`examples/blinky.rs`)

Blinks the onboard red LED (GPIO 13).

```bash
cargo run --example blinky
```

### 2. NeoPixel Rainbow (`examples/neopixel_rainbow.rs`)

Cycles through a smooth rainbow on the onboard NeoPixel.

```bash
cargo run --example neopixel_rainbow
```

### 3. SSD1681 e-Ink Text (`examples/ssd1681.rs`)

Displays text and basic geometric primitives on the integrated e-Ink display.

```bash
cargo run --example ssd1681
```

### 4. SSD1681 Tri-Color Image (`examples/ssd1681_image.rs`)

Displays a high-quality, dithered tri-color image (`mocha200x200.bmp`) on the e-Ink display.

```bash
cargo run --example ssd1681_image
```

## Utilities

### Image Conversion Script (`examples/convert_bmp.sh`)

Automates the conversion of standard JPG/PNG images into the high-quality, dithered 3-color (Black, Red, White) BMP format required for the SSD1681.

```bash
cd examples
./convert_bmp.sh my_image.jpg output.bmp
```

## Development Features

- **panic-probe**: Provides detailed crash reports over RTT.
- **defmt**: High-efficiency logging for embedded systems.
- **picotool**: Seamless deployment using BOOTSEL mode (via USB).
