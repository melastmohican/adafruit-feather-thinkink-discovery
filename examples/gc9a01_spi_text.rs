//! # GC9A01 Round LCD Display SPI Text Example for Adafruit Feather RP2040 ThinkInk
//!
//! This example demonstrates drawing text and shapes on a 240x240 round GC9A01 display over SPI1.
//!
//! ## Wiring
//!
//! ```text
//! Feather RP2040 ThinkInk      GC9A01 Display
//! -----------------------      --------------
//! GPIO 14 (SCK)   -----------> SCL (SCK)  [SPI1 SCK]
//! GPIO 15 (MO)    -----------> SDA (MOSI) [SPI1 TX]
//! GPIO 8  (MI)    -----------> (Unused)   [SPI1 RX]
//! GPIO 6  (D6)    -----------> CS         [Chip Select]
//! GPIO 5  (D5)    -----------> DC         [Data/Command]
//! GPIO 9  (D9)    -----------> RST        [Reset]
//! 3V3             -----------> VCC
//! GND             -----------> GND
//! ```
//!
//! ## Run
//!
//! `cargo run --example gc9a01_spi_text --release`

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::entry;
use bsp::hal::{
    clocks::{init_clocks_and_plls, Clock},
    fugit::RateExtU32,
    gpio::{FunctionSpi, Pins},
    pac,
    sio::Sio,
    spi::Spi,
    watchdog::Watchdog,
    Timer,
};
use defmt_rtt as _;
use display_interface_spi::SPIInterface;
use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, ascii::FONT_6X10, ascii::FONT_9X15_BOLD, MonoTextStyleBuilder},
    pixelcolor::{Rgb565, RgbColor},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle},
    text::{Baseline, Text},
};
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal_bus::spi::ExclusiveDevice;
use mipidsi::{
    models::GC9A01,
    options::{ColorInversion, ColorOrder},
    Builder,
};
use panic_probe as _;

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    // External high-speed crystal on the pico board is 12Mhz
    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = init_clocks_and_plls(
        external_xtal_freq_hz,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut delay = Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    defmt::info!("Initializing GC9A01 round LCD display on SPI1...");

    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // SPI1 Pins
    // SCK: GPIO 14
    // MOSI: GPIO 15
    // MISO: GPIO 8
    let sclk = pins.gpio14.into_function::<FunctionSpi>();
    let mosi = pins.gpio15.into_function::<FunctionSpi>();
    let miso = pins.gpio8.into_function::<FunctionSpi>();

    // Control pins
    let cs = pins.gpio6.into_push_pull_output();
    let dc = pins.gpio5.into_push_pull_output();
    let mut rst = pins.gpio9.into_push_pull_output();

    // Create SPI1 bus
    let spi = Spi::<_, _, _, 8>::new(pac.SPI1, (mosi, miso, sclk)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        62_500_000u32.Hz(),
        embedded_hal::spi::MODE_0,
    );

    defmt::info!("SPI1 configured at 62.5 MHz");

    // Create exclusive SPI device with CS pin
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();

    // Create display interface
    let di = SPIInterface::new(spi_device, dc);

    // Reset the display
    let _ = rst.set_low();
    delay.delay_ms(10);
    let _ = rst.set_high();
    delay.delay_ms(120);

    // Create and initialize display using mipidsi
    let mut display = Builder::new(GC9A01, di)
        .invert_colors(ColorInversion::Inverted)
        .color_order(ColorOrder::Bgr)
        .display_size(240, 240)
        .init(&mut delay)
        .unwrap();

    defmt::info!("Display initialized!");

    // Clear screen to black
    display.clear(Rgb565::BLACK).unwrap();

    defmt::info!("Drawing text and shapes...");

    // Create text styles
    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .build();

    let subtitle_style = MonoTextStyleBuilder::new()
        .font(&FONT_9X15_BOLD)
        .text_color(Rgb565::YELLOW)
        .build();

    let small_text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(Rgb565::CYAN)
        .build();

    // Draw title text centered at top (accounting for round display shape)
    // Position slightly lower to avoid being cut off by circular edge
    Text::with_baseline("GC9A01", Point::new(75, 30), title_style, Baseline::Top)
        .draw(&mut display)
        .unwrap();

    // Draw subtitle on second line
    Text::with_baseline(
        "Round Display",
        Point::new(60, 55),
        subtitle_style,
        Baseline::Top,
    )
    .draw(&mut display)
    .unwrap();

    // Draw small text below
    Text::with_baseline(
        "240x240 RGB",
        Point::new(78, 75),
        small_text_style,
        Baseline::Top,
    )
    .draw(&mut display)
    .unwrap();

    // Draw a large circle outline in the center (emphasizes round shape)
    Circle::new(Point::new(50, 100), 90)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLUE, 2))
        .draw(&mut display)
        .unwrap();

    // Draw smaller concentric circle
    Circle::new(Point::new(80, 130), 30)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::GREEN, 2))
        .draw(&mut display)
        .unwrap();

    // Draw filled circles at strategic positions
    Circle::new(Point::new(95, 115), 15)
        .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
        .draw(&mut display)
        .unwrap();

    Circle::new(Point::new(130, 135), 12)
        .into_styled(PrimitiveStyle::with_fill(Rgb565::MAGENTA))
        .draw(&mut display)
        .unwrap();

    Circle::new(Point::new(105, 150), 10)
        .into_styled(PrimitiveStyle::with_fill(Rgb565::CSS_ORANGE))
        .draw(&mut display)
        .unwrap();

    // Draw lines radiating from center (like a clock face)
    let center = Point::new(120, 120);

    Line::new(center, Point::new(120, 40))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
        .draw(&mut display)
        .unwrap();

    Line::new(center, Point::new(200, 120))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
        .draw(&mut display)
        .unwrap();

    Line::new(center, Point::new(120, 200))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
        .draw(&mut display)
        .unwrap();

    Line::new(center, Point::new(40, 120))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
        .draw(&mut display)
        .unwrap();

    // Draw bottom text (positioned to fit within circular bounds)
    Text::with_baseline(
        "Feather RP2040 ThinkInk",
        Point::new(50, 205),
        small_text_style,
        Baseline::Top,
    )
    .draw(&mut display)
    .unwrap();

    defmt::info!("Display complete!");

    // Main loop
    loop {
        cortex_m::asm::nop();
    }
}
