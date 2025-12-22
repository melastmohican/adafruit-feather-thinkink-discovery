//! Draw a 1 bit per pixel black and white image on a 128x64 SSD1306 display over I2C1 (STEMMA QT).
//!
//! This example is for the Adafruit Feather RP2040 ThinkINK board.
//!
//! Wiring:
//! - Connect SSD1306 OLED to the STEMMA QT port.
//!
//! Run with `cargo run --example ssd1306`.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionI2C, Pins, PullUp};
use bsp::hal::{Sio, Watchdog, I2C};
use bsp::{entry, pac};
use defmt_rtt as _;
use embedded_graphics::{image::Image, pixelcolor::BinaryColor, prelude::*};
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

    let i2c = I2C::i2c1(
        pac.I2C1,
        sda_pin,
        scl_pin,
        400_000u32.Hz(),
        &mut pac.RESETS,
        &clocks.system_clock,
    );

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    display.init().unwrap();

    let logo = tinybmp::Bmp::<BinaryColor>::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    let im = Image::new(&logo, Point::new(32, 0));
    im.draw(&mut display).unwrap();

    display.flush().unwrap();

    loop {
        cortex_m::asm::wfi();
    }
}
