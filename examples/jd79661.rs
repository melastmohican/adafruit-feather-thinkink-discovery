//! Simple no-std "Hello World" example for the Adafruit RP2040 Feather ThinkInk
//! with 2.13" Tri-Color e-paper display ([Product 6373](https://www.adafruit.com/product/6373))
//! which uses the JD79661 chipset.
//!
//! Connections (Integrated/FPC):
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
//! `cargo run --example jd79661`

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionSpi, Pins};
use bsp::hal::{spi, Clock, Sio, Watchdog};
use bsp::{entry, pac};
use defmt::{info, println};
use defmt_rtt as _;
use panic_probe as _;

use embedded_hal_bus::spi::ExclusiveDevice;

use adafruit_feather_thinkink_discovery::{DisplayBuffer, Jd79661, QuadColor};

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_graphics::Drawable;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::StatefulOutputPin;

use bsp::hal::Timer;

#[entry]
fn main() -> ! {
    info!("Program start");
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

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

    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut led_pin = pins.gpio13.into_push_pull_output();

    // ThinkInk E-Ink Connections:
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

    // Initialize display controller
    let mut epd = Jd79661::new(&mut spi_device, cs, busy, dc, rst, &mut delay).unwrap();

    // Create a single buffer for all colors
    let mut display = DisplayBuffer::new();

    // Draw some shapes
    Rectangle::new(Point::new(10, 10), Size::new(50, 50))
        .into_styled(PrimitiveStyle::with_fill(QuadColor::Yellow))
        .draw(&mut display)
        .unwrap();

    Circle::new(Point::new(100, 60), 40)
        .into_styled(PrimitiveStyle::with_fill(QuadColor::Red))
        .draw(&mut display)
        .unwrap();

    let style = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(QuadColor::Black)
        .build();

    Text::new("JD79661 4-Color", Point::new(70, 20), style)
        .draw(&mut display)
        .unwrap();

    let style_red = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(QuadColor::Red)
        .build();

    Text::new("Red Square", Point::new(70, 40), style_red)
        .draw(&mut display)
        .unwrap();

    let style_yellow = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(QuadColor::Yellow)
        .build();

    Text::new("Yellow Circle", Point::new(70, 55), style_yellow)
        .draw(&mut display)
        .unwrap();

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
