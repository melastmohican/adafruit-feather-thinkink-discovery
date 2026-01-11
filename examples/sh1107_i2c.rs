//! Draw a 1 bit per pixel black and white image on a 128x64 SH1107 display over I2C1.
//!
//! This example is for the Adafruit Feather RP2040 ThinkINK board.
//!
//! Hardware:
//! - Adafruit FeatherWing OLED - 128x64 OLED (SH1107)
//!   https://www.adafruit.com/product/4650
//!
//! Wiring:
//! - Stack the FeatherWing OLED on top of the Feather board using the header pins.
//!   The FeatherWing connects to I2C1 (GP2/GP3) through the stacking headers.
//!
//! Run with `cargo run --example sh1107_i2c`.

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
// Assuming graphics feature exposes these. If not, I will debug further.
use sh1107_driver::{SH1107Color, SH1107};

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

    // Address is likely handled internally (0x3C common)
    let mut display = SH1107::new(i2c);
    // Wait, import says use sh1107_driver::{SH1107, ...}

    display.begin().unwrap();
    display.buffer_clear();
    display.display_all(); // Flattening buffer to display

    display.display_all(); // Flattening buffer to display

    let logo = tinybmp::Bmp::<BinaryColor>::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // Wrap display to use embedded-graphics
    let mut wrapper = Sh1107Wrapper(&mut display);

    let im = Image::new(&logo, Point::new(32, 0));
    im.draw(&mut wrapper).unwrap();

    display.display_all();

    loop {
        cortex_m::asm::wfi();
    }
}

struct Sh1107Wrapper<'a, I>(&'a mut SH1107<I>);

impl<'a, I: embedded_hal::i2c::I2c> OriginDimensions for Sh1107Wrapper<'a, I> {
    fn size(&self) -> Size {
        Size::new(128, 64)
    }
}

impl<'a, I: embedded_hal::i2c::I2c> DrawTarget for Sh1107Wrapper<'a, I> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<D>(&mut self, item: D) -> Result<(), Self::Error>
    where
        D: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in item {
            // Swap X and Y for 90 degree rotation
            // User Space: 128x64 Landscape
            // Driver Space: 64x128 Portrait

            let user_x = point.x;
            let user_y = point.y;

            if user_x >= 0 && user_y >= 0 {
                // Correction for 180 degree rotation from previous state:
                // Old: driver_x = 63 - user_y; driver_y = user_x + 32;
                // New: driver_x = user_y;      driver_y = (127 - user_x) + 32;

                let driver_x = user_y as usize;
                let driver_y = (128 - 1) - (user_x as usize);

                // SH1107 column address 0..127. With +32, range is 32..159.
                // This corresponds to segment 0..127 on the glass if mapped this way.
                // We check bounds against driver's buffer capability if needed, but SH1107 driver
                // usually clips or wraps.

                if driver_x < 64 {
                    let c = match color {
                        BinaryColor::On => SH1107Color::White,
                        BinaryColor::Off => SH1107Color::Black,
                    };
                    self.0.buffer_draw_pixel(driver_x, driver_y, &c);
                }
            }
        }
        Ok(())
    }
}
