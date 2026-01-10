//! # GC9A01 Round LCD Display SPI Example for Adafruit Feather RP2040 ThinkInk
//!
//! Draw images on a 240x240 round GC9A01 display over SPI1.
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
//! `cargo run --example feather_gc9a01 --release`

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::entry;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::{
    clocks::{init_clocks_and_plls, Clock},
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
    geometry::Point,
    image::Image,
    pixelcolor::{Rgb565, RgbColor},
    Drawable,
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
use tinybmp::Bmp;

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

    defmt::info!("Drawing images...");

    // Draw ferris (raw RGB565 image)
    let ferris = Bmp::from_slice(include_bytes!("ferris.bmp")).unwrap();
    let ferris = Image::new(&ferris, Point::new(120, 80));
    ferris.draw(&mut display).unwrap();

    defmt::info!("Ferris drawn!");

    // Draw Rust logo (BMP format)
    let logo = Bmp::from_slice(include_bytes!("rust.bmp")).unwrap();
    let logo = Image::new(&logo, Point::new(40, 80));
    logo.draw(&mut display).unwrap();

    defmt::info!("Rust logo drawn!");

    // Main loop
    loop {
        cortex_m::asm::nop();
    }
}
