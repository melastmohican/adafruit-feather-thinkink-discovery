//! # SSD1306 OLED Text & Graphics Example for Adafruit Feather ThinkINK
//!
//! This example demonstrates drawing text and shapes on a 128x64 SSD1306 display over I2C1 (STEMMA QT).
//!
//! ## Wiring
//!
//! - Connect SSD1306 OLED to the STEMMA QT port.
//!
//! Run with `cargo run --example ssd1306_text`.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionI2C, Pins, PullUp};
use bsp::hal::{Sio, Watchdog, I2C};
use bsp::{entry, pac};
use defmt_rtt as _;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use panic_probe as _;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

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

    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Configure I2C1 pins for STEMMA QT (GP2 = SDA, GP3 = SCL)
    let sda_pin = pins
        .gpio2
        .into_pull_type::<PullUp>()
        .into_function::<FunctionI2C>();
    let scl_pin = pins
        .gpio3
        .into_pull_type::<PullUp>()
        .into_function::<FunctionI2C>();

    // Create I2C1 peripheral
    let i2c = I2C::i2c1(
        pac.I2C1,
        sda_pin,
        scl_pin,
        400_000u32.Hz(),
        &mut pac.RESETS,
        &clocks.system_clock,
    );

    // Create display interface
    let interface = I2CDisplayInterface::new(i2c);

    // Create display driver
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    // Initialize the display
    display.init().unwrap();
    defmt::info!("Display initialized!");

    // Create text style
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    // Clear the display buffer
    display.clear(BinaryColor::Off).unwrap();

    // Draw title text
    Text::with_baseline("Rust Feather", Point::new(30, 0), text_style, Baseline::Top)
        .draw(&mut display)
        .unwrap();

    // Draw a separator line
    Line::new(Point::new(0, 12), Point::new(127, 12))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut display)
        .unwrap();

    // Draw a rectangle
    Rectangle::new(Point::new(10, 20), Size::new(40, 30))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut display)
        .unwrap();

    // Draw a filled circle
    Circle::new(Point::new(80, 35), 15)
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .unwrap();

    // Draw some text at bottom
    Text::with_baseline(
        "Hello, Rust!",
        Point::new(10, 54),
        text_style,
        Baseline::Top,
    )
    .draw(&mut display)
    .unwrap();

    // Flush to display
    display.flush().unwrap();
    defmt::info!("Display content rendered!");

    // Keep display showing
    loop {
        cortex_m::asm::wfi();
    }
}
