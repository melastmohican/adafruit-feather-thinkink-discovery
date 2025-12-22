//! # BME280 Temperature/Humidity/Pressure Sensor Example for Adafruit Feather ThinkINK
//!
//! Reads temperature, humidity, and atmospheric pressure from a BME280 sensor over I2C1.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkINK
//! - **Sensor:** Adafruit BME280 Temperature Humidity Pressure Sensor (or compatible)
//! - **Connection:** STEMMA QT (I2C1)
//! - **I2C Address:** 0x77
//!
//! ## Wiring
//!
//! ```
//!      BME280 -> Feather ThinkINK (STEMMA QT)
//! (black)  GND -> GND
//! (red)    VCC -> 3.3V
//! (yellow) SCL -> GPIO3 (SCL1)
//! (blue)   SDA -> GPIO2 (SDA1)
//! ```
//!
//! ## I2C Address
//!
//! The BME280 can have two I2C addresses:
//! - 0x76 (SDO pin to GND) - use `BME280::new_primary()`
//! - 0x77 (SDO pin to VCC) - use `BME280::new_secondary()`
//!
//! The Adafruit BME280 uses address 0x77 by default.
//!
//! Run with `cargo run --example bme280_i2c`.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionI2C, Pins, PullUp};
use bsp::hal::{Sio, Timer, Watchdog, I2C};
use bsp::{entry, pac};
use defmt::*;
use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use panic_probe as _;

// The BME280 driver
use bme280::i2c::BME280;

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

    // Create a BME280 driver instance
    let mut bme280 = BME280::new_secondary(i2c);

    // Initialize the sensor
    if let Err(e) = bme280.init(&mut timer) {
        error!("Failed to initialize BME280: {:?}", defmt::Debug2Format(&e));
        loop {
            cortex_m::asm::wfi();
        }
    }

    info!("BME280 initialized successfully!");
    info!("Starting measurements...");

    loop {
        // Take a measurement
        match bme280.measure(&mut timer) {
            Ok(measurements) => {
                info!(
                    "Temperature: {} C, Pressure: {} hPa, Humidity: {} %",
                    measurements.temperature,
                    measurements.pressure / 100.0,
                    measurements.humidity
                );
            }
            Err(e) => {
                error!("Error reading BME280 sensor: {:?}", defmt::Debug2Format(&e));
            }
        }

        // Wait 1 second between measurements
        timer.delay_ms(1000);
    }
}
