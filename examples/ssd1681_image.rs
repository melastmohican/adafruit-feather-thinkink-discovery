//! Display a BMP image on the SSD1681 e-ink display
//!
//! Uses the `tinybmp` crate to parse a 1-bit BMP image and draw it to the display.
//!
//! Connections (Integrated):
//!
//! | Pin         | GPIO  | Function |
//! |-------------|-------|----------|
//! | EPD_SCK     | GP22  | SCK      |
//! | EPD_MOSI    | GP23  | MOSI     |
//! | EPD_CS      | GP19  | CS       |
//! | EPD_BUSY    | GP16  | BUSY     |
//! | EPD_DC      | GP18  | DC       |
//! | EPD_RESET   | GP17  | RESET    |
//!
//! To run this example run:
//! `cargo run --example ssd1681_image`

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionSpi, Pins};
use bsp::hal::{spi, Clock, Sio, Timer, Watchdog};
use bsp::{entry, pac};
use defmt::{info, println};
use defmt_rtt as _;
use embedded_graphics::prelude::*;
use embedded_hal_bus::spi::ExclusiveDevice;
use panic_probe as _;
use ssd1681::driver::Ssd1681;
use ssd1681::graphics::{Display, Display1in54};
use tinybmp::Bmp;

#[entry]
fn main() -> ! {
    info!("Program start");
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = bsp::hal::clocks::init_clocks_and_plls(
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

    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // ThinkInk E-Ink Connections:
    let sck = pins.gpio22.into_function::<FunctionSpi>();
    let mosi = pins.gpio23.into_function::<FunctionSpi>();
    let miso = pins.gpio20.into_function::<FunctionSpi>();

    // Control Pins:
    let cs = pins.gpio19.into_push_pull_output();
    let dc = pins.gpio18.into_push_pull_output();
    let rst = pins.gpio17.into_push_pull_output();
    let busy = pins.gpio16.into_pull_down_input();

    // Use a dummy pin for ExclusiveDevice since Ssd1681 manages its own CS
    let dummy_cs = pins.gpio15.into_push_pull_output();

    // Create an SPI driver instance for the SPI0 device
    let spi = spi::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        4_000_000u32.Hz(),
        embedded_hal::spi::MODE_0,
    );

    let mut spi_device = ExclusiveDevice::new_no_delay(spi, dummy_cs).unwrap();

    // Initialize display controller
    let mut ssd1681 = Ssd1681::new(&mut spi_device, cs, busy, dc, rst, &mut delay).unwrap();

    // Clear frames on the display driver
    ssd1681.clear_bw_frame(&mut spi_device);
    ssd1681.clear_red_frame(&mut spi_device);

    // Create buffers for black and red
    let mut display_bw = Display1in54::bw();
    let mut display_red = Display1in54::red();

    // Load BMP image
    let bmp_data = include_bytes!("mocha200x200.bmp");
    let bmp = Bmp::<embedded_graphics::pixelcolor::Rgb888>::from_slice(bmp_data).unwrap();

    // Draw the image pixels to the respective buffers
    // Using Black and Red constants from ssd1681::color
    use ssd1681::color::{Black, Red};
    for Pixel(point, color) in bmp.pixels() {
        if color == embedded_graphics::pixelcolor::Rgb888::BLACK {
            Pixel(point, Black).draw(&mut display_bw).unwrap();
        } else if color == embedded_graphics::pixelcolor::Rgb888::RED {
            Pixel(point, Red).draw(&mut display_red).unwrap();
        }
    }

    println!("Send bw frame to display");
    ssd1681.update_bw_frame(&mut spi_device, display_bw.buffer());

    println!("Send red frame to display");
    ssd1681.update_red_frame(&mut spi_device, display_red.buffer());

    println!("Update display");
    ssd1681.display_frame(&mut spi_device);

    println!("Done");

    loop {
        cortex_m::asm::wfi();
    }
}
