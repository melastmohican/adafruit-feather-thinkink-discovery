//! Display a BMP image on the JD79661 e-ink display
//!
//! Uses the `tinybmp` crate to parse a BMP image and draw it to the display.
//! This example uses all 4 colors: Black, White, Red, and Yellow.
//!
//! To run this example run:
//! `cargo run --example jd79661_image`

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionSpi, Pins};
use bsp::hal::{spi, Clock, Sio, Timer, Watchdog};
use bsp::{entry, pac};
use defmt::{info, println};
use defmt_rtt as _;
use panic_probe as _;

use embedded_graphics::prelude::*;
use embedded_hal_bus::spi::ExclusiveDevice;
use tinybmp::Bmp;

use adafruit_feather_thinkink_discovery::{DisplayBuffer, Jd79661, QuadColor, HEIGHT, WIDTH};

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::StatefulOutputPin;

#[entry]
fn main() -> ! {
    info!("Program start");
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);
    let clocks = init_clocks_and_plls(
        12_000_000u32,
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

    let mut led_pin = pins.gpio13.into_push_pull_output();

    let sck = pins.gpio22.into_function::<FunctionSpi>();
    let mosi = pins.gpio23.into_function::<FunctionSpi>();
    let miso = pins.gpio20.into_function::<FunctionSpi>();
    let cs = pins.gpio19.into_push_pull_output();
    let dc = pins.gpio18.into_push_pull_output();
    let rst = pins.gpio17.into_push_pull_output();
    let busy = pins.gpio16.into_pull_down_input();
    let dummy_cs = pins.gpio15.into_push_pull_output();

    let spi = spi::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        4_000_000u32.Hz(),
        embedded_hal::spi::MODE_0,
    );
    let mut spi_device = ExclusiveDevice::new_no_delay(spi, dummy_cs).unwrap();

    let mut epd = Jd79661::new(&mut spi_device, cs, busy, dc, rst, &mut delay).unwrap();
    let mut display = DisplayBuffer::new();

    let bmp_data = include_bytes!("mocha250x122.bmp");
    let bmp = Bmp::<embedded_graphics::pixelcolor::Rgb888>::from_slice(bmp_data).unwrap();

    for Pixel(point, color) in bmp.pixels() {
        if point.x >= 0 && point.x < WIDTH as i32 && point.y >= 0 && point.y < HEIGHT as i32 {
            let quad_color = if color == embedded_graphics::pixelcolor::Rgb888::BLACK {
                QuadColor::Black
            } else if color == embedded_graphics::pixelcolor::Rgb888::RED {
                QuadColor::Red
            } else if color == embedded_graphics::pixelcolor::Rgb888::YELLOW {
                QuadColor::Yellow
            } else {
                QuadColor::White
            };
            Pixel(point, quad_color).draw(&mut display).unwrap();
        }
    }

    println!("Send frames to display");
    epd.update_frames(&mut spi_device, &display).unwrap();

    println!("Update display");
    epd.display_frame(&mut spi_device, &mut delay).unwrap();

    println!("Done");
    loop {
        let _ = led_pin.toggle();
        delay.delay_ms(500);
    }
}
