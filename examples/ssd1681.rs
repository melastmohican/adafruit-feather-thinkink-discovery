//! Simple no-std "Hello World" example for the Adafruit RP2040 Feather ThinkInk
//! with Dalian Good Display Tri-Color e-ink display 1.54 inch e-ink small display screen, [GDEM0154Z90](https://www.good-display.com/product/436.html)
//! using the integrated e-Ink connector.
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
//! `cargo run --example ssd1681`

#![no_std]
#![no_main]
use adafruit_feather_rp2040 as bsp;
use defmt_rtt as _;
use panic_probe as _;

use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionSpi, Pins};
use bsp::hal::{spi, Clock, Sio, Watchdog};
use bsp::{entry, pac};
use defmt::{info, println};
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::prelude::Primitive;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_graphics::Drawable;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::StatefulOutputPin;
use embedded_hal_bus::spi::ExclusiveDevice;
use ssd1681::color::{Black, Red, White};
use ssd1681::driver::Ssd1681;
use ssd1681::graphics::{Display, Display1in54, DisplayRotation};
use ssd1681::WIDTH;

use bsp::hal::Timer;

#[entry]
fn main() -> ! {
    info!("Program start");
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

    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut led_pin = pins.gpio13.into_push_pull_output();

    // ThinkInk E-Ink Connections:
    // SCK: GPIO 22
    // MOSI: GPIO 23
    // MISO: GPIO 20 (not used)
    let sck = pins.gpio22.into_function::<FunctionSpi>();
    let mosi = pins.gpio23.into_function::<FunctionSpi>();
    let miso = pins.gpio20.into_function::<FunctionSpi>();

    // Control Pins:
    // EPD_CS: GPIO 19
    // EPD_DC: GPIO 18
    // EPD_RESET: GPIO 17
    // EPD_BUSY: GPIO 16
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
        4_000_000u32.Hz(), // SSD1681 typical speed
        embedded_hal::spi::MODE_0,
    );

    let mut spi_device = ExclusiveDevice::new_no_delay(spi, dummy_cs).unwrap();

    // Initialize display controller
    let mut ssd1681 = Ssd1681::new(&mut spi_device, cs, busy, dc, rst, &mut delay).unwrap();

    // Clear frames on the display driver
    ssd1681.clear_red_frame(&mut spi_device);
    ssd1681.clear_bw_frame(&mut spi_device);

    // Create buffer for black and white
    let mut display_bw = Display1in54::bw();

    draw_rotation_and_rulers(&mut display_bw);

    display_bw.set_rotation(DisplayRotation::Rotate0);
    Rectangle::new(Point::new(60, 60), Size::new(100, 100))
        .into_styled(PrimitiveStyle::with_fill(Black))
        .draw(&mut display_bw)
        .unwrap();

    println!("Send bw frame to display");
    ssd1681.update_bw_frame(&mut spi_device, display_bw.buffer());

    // Draw red color
    let mut display_red = Display1in54::red();

    Circle::new(Point::new(100, 100), 20)
        .into_styled(PrimitiveStyle::with_fill(Red))
        .draw(&mut display_red)
        .unwrap();

    println!("Send red frame to display");
    ssd1681.update_red_frame(&mut spi_device, display_red.buffer());

    println!("Update display");
    ssd1681.display_frame(&mut spi_device);

    println!("Done");

    loop {
        let _ = led_pin.toggle();
        delay.delay_ms(500);
    }
}

fn draw_rotation_and_rulers(display: &mut Display1in54) {
    display.set_rotation(DisplayRotation::Rotate0);
    draw_text(display, "rotation 0", 25, 25);
    draw_ruler(display);

    display.set_rotation(DisplayRotation::Rotate90);
    draw_text(display, "rotation 90", 25, 25);
    draw_ruler(display);

    display.set_rotation(DisplayRotation::Rotate180);
    draw_text(display, "rotation 180", 25, 25);
    draw_ruler(display);

    display.set_rotation(DisplayRotation::Rotate270);
    draw_text(display, "rotation 270", 25, 25);
    draw_ruler(display);
}

fn draw_ruler(display: &mut Display1in54) {
    for col in 1..WIDTH {
        if col % 25 == 0 {
            Line::new(Point::new(col as i32, 0), Point::new(col as i32, 10))
                .into_styled(PrimitiveStyle::with_stroke(Black, 1))
                .draw(display)
                .unwrap();
        }

        if col % 50 == 0 {
            let mut buf = [0u8; 4];
            let label = format_no_std::show(&mut buf, format_args!("{}", col)).unwrap();
            draw_text(display, &label, col as i32, 12);
        }
    }
}

fn draw_text(display: &mut Display1in54, text: &str, x: i32, y: i32) {
    let style = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(Black)
        .background_color(White)
        .build();
    let _ = Text::new(text, Point::new(x, y), style).draw(display);
}
