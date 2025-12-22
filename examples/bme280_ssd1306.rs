//! # BME280 + SSD1306 OLED Combined Example for Adafruit Feather ThinkINK
//!
//! Reads temperature, humidity, and atmospheric pressure from a BME280 sensor
//! and displays the values on a 128x64 SSD1306 OLED.
//!
//! Both devices share the I2C1 (STEMMA QT) bus.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkINK
//! - **Sensor:** Adafruit BME280 (I2C Address: 0x77)
//! - **Display:** SSD1306 OLED (I2C Address: 0x3C)
//! - **Connection:** STEMMA QT (I2C1)
//!
//! ## Wiring
//!
//! - Daisy-chain the BME280 and OLED using STEMMA QT cables
//!   or connect both to the Feather's STEMMA QT port using a hub.
//!
//! Run with `cargo run --example bme280_ssd1306`.

#![no_std]
#![no_main]

use core::cell::RefCell;
use core::fmt::Write as _;
use embedded_hal::delay::DelayNs;

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionI2C, Pins, PullUp};
use bsp::hal::{Sio, Timer, Watchdog, I2C};
use bsp::{entry, pac};
use defmt::*;
use defmt_rtt as _;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use embedded_hal_bus::i2c::RefCellDevice;
use panic_probe as _;

// Drivers
use bme280::i2c::BME280;
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

    let mut timer = Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

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

    // Share the I2C bus using a RefCell
    let i2c_bus = RefCell::new(i2c);

    // Create proxies for the bus
    let bme_i2c = RefCellDevice::new(&i2c_bus);
    let oled_i2c = RefCellDevice::new(&i2c_bus);

    // Initialize BME280
    let mut bme280 = BME280::new_secondary(bme_i2c);
    if let Err(e) = bme280.init(&mut timer) {
        error!("Failed to initialize BME280: {:?}", defmt::Debug2Format(&e));
        // We can't display this error on OLED yet as it's not initialized
        loop {
            cortex_m::asm::wfi();
        }
    }

    // Initialize SSD1306 OLED
    let interface = I2CDisplayInterface::new(oled_i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    display.init().unwrap();

    info!("BME280 and OLED initialized!");

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let mut buf = heapless::String::<64>::new();

    loop {
        // Take a measurement
        match bme280.measure(&mut timer) {
            Ok(m) => {
                display.clear(BinaryColor::Off).unwrap();

                // Title
                Text::with_baseline(
                    "BME280 Readings",
                    Point::new(14, 0),
                    text_style,
                    Baseline::Top,
                )
                .draw(&mut display)
                .unwrap();

                // Temperature
                buf.clear();
                core::write!(&mut buf, "Temp: {} C", m.temperature as i32).unwrap();
                Text::with_baseline(&buf, Point::new(0, 20), text_style, Baseline::Top)
                    .draw(&mut display)
                    .unwrap();

                // Humidity
                buf.clear();
                core::write!(&mut buf, "Hum:  {} %", m.humidity as i32).unwrap();
                Text::with_baseline(&buf, Point::new(0, 35), text_style, Baseline::Top)
                    .draw(&mut display)
                    .unwrap();

                // Pressure
                buf.clear();
                core::write!(&mut buf, "Pres: {} hPa", (m.pressure / 100.0) as i32).unwrap();
                Text::with_baseline(&buf, Point::new(0, 50), text_style, Baseline::Top)
                    .draw(&mut display)
                    .unwrap();

                display.flush().unwrap();

                defmt::info!(
                    "Temp: {}, Pres: {}, Hum: {}",
                    m.temperature,
                    m.pressure / 100.0,
                    m.humidity
                );
            }
            Err(e) => {
                defmt::error!("Error reading BME280: {:?}", defmt::Debug2Format(&e));
            }
        }

        // Wait 1 second
        timer.delay_ms(1000);
    }
}
