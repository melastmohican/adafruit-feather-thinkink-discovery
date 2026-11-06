//! # GDEY029F52 2.9" Quad-Color E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery)
//! to the Adafruit Feather RP2040 ThinkInk, driving the 2.9" 128x296 Quad-Color Good Display
//! **GDEY029F52** through `Jd79661Controller` and the `GDEY029F52` panel type.
//!
//! Two pages, one full refresh each, with a 20 s rest between them:
//!
//! 1. **Geometry page.** A black border on all four edges, a red square at the RAM origin and
//!    black, yellow and red bands across the middle third. A wrong resolution shows up here as
//!    a shifted or sheared picture.
//! 2. **Demo page.** The same page as the other colour examples: header, four-swatch colour
//!    bar, Ferris and Rust logos, and footer labels.
//!
//! Do not loop this. Each refresh takes about 16 s.
//!
//! ## Panel check
//!
//! Before the refresh the example resets the panel and reads its `R70H` revision register over
//! a bit-banged 3-wire bus, before SPI takes SCK and MOSI, then logs which unit it is. Seen so
//! far on this panel: `83 02 01`. It never powers on or refreshes the panel at that point, and
//! the example carries on whatever it reads.
//!
//! ## Panel identification
//!
//! Check the rear sticker first. This panel's reads `029SW-F52-B2`. A `GDEY029F51H` (JD79667,
//! 168x384) is a different Good Display product and will not draw correctly here. A unit
//! ordered as an F51H arrived with the F52 sticker, so the order label is not enough.
//!
//! Needs the `GDEY029F52` panel type, which is newer than epdsi 0.10.1.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** GDEY029F52 2.9" Quad-Color, 128x296 (JD79661), seated directly in the board's 24-pin FPC socket.
//!
//! ## Wiring
//!
//! Fixed by the socket; nothing to wire by hand. **Swap panels with the board unpowered.**
//!
//! | Signal | GPIO | | Signal | GPIO |
//! |--------|------|-|--------|------|
//! | SCK    | GP22 | | DC     | GP18 |
//! | MOSI   | GP23 | | RST    | GP17 |
//! | CS     | GP19 | | BUSY   | GP16 |
//!
//! These are SPI0 on the RP2040, even though the Arduino core calls the port SPI1.
//!
//! ## Results arrive live, one phase at a time
//!
//! No SWD connector is fitted on this board, so logging goes over USB CDC via `defmt-bbq` rather
//! than RTT. USB is serviced on core1 while core0 runs the panel, see
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report). Each page's refresh time is
//! logged as soon as it completes.
//!
//! **Watch the panel meanwhile.** A stage that completes fast *without* visibly changing the
//! display has not driven the ink, and no timing figure will tell you that.
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example jd79661_gdey029f52_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/jd79661_gdey029f52_epd
//! ```
//!
//! USB comes up within about a second of boot now — core1 services it independently of the panel,
//! so the `until` loop above returns almost immediately instead of waiting for the run to finish.
//!
//! The `until` loop is glob-free deliberately. In zsh an unmatched glob is a hard error raised by
//! the shell *before* the command runs, so `ls /dev/cu.usbmodemEPD* 2>/dev/null` still prints
//! `no matches found` and never executes `ls` — the redirect cannot suppress a message the shell
//! itself emits. Listing `/dev` and grepping avoids expansion entirely and behaves the same in
//! bash and zsh.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` right after flashing just means enumeration hasn't
//! finished yet** — it should clear within a second or two. If it doesn't clear quickly, confirm
//! the board was actually in bootloader mode before flashing.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use adafruit_feather_thinkink_discovery::usb_report::{spawn_usb_log_pump, Core1Handles, UsbParts};
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{
    FunctionSio, FunctionSpi, Pin, PinId, Pins, PullNone, SioInput, SioOutput, ValidFunction,
};
use bsp::hal::{spi, Clock, Sio, Timer, Watchdog};
use bsp::{entry, pac, XOSC_CRYSTAL_FREQ};

// defmt-bbq is the global logger here, not defmt-rtt. Only one may be linked.
use defmt_bbq as _;
use panic_probe as _;

use embedded_graphics::geometry::{Dimensions, Point, Size};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType as DigitalErrorType, InputPin, OutputPin};
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;
use tinybmp::Bmp;

/// Adapts an rp2040-hal GPIO pin to [`epdsi::bus3::DynamicPin`] by switching between the pin's
/// `FunctionSio<SioOutput>` and `FunctionSio<SioInput>` type states at runtime.
///
/// Required because the pre-flight revision read bit-bangs a single MOSI wire in both
/// directions (see `Spi3Bus`) — rp2040-hal (like most Rust HALs) represents input vs. output as
/// distinct pin types, so switching direction means converting and re-storing the pin, not just
/// flipping a register bit through a fixed type.
enum FlexPinInner<I: PinId> {
    In(Pin<I, FunctionSio<SioInput>, PullNone>),
    Out(Pin<I, FunctionSio<SioOutput>, PullNone>),
}

struct FlexPin<I: PinId>(Option<FlexPinInner<I>>);

impl<I> FlexPin<I>
where
    I: PinId + ValidFunction<FunctionSio<SioInput>> + ValidFunction<FunctionSio<SioOutput>>,
{
    fn new_output<P: bsp::hal::gpio::PullType>(pin: Pin<I, FunctionSio<SioOutput>, P>) -> Self {
        Self(Some(FlexPinInner::Out(pin.into_pull_type::<PullNone>())))
    }

    /// Consumes self, returning the pin configured as a floating input. Panics if the pin is
    /// currently in output mode — the revision read always leaves the data pin in input mode
    /// (its last operation is a bit-banged read), so this is safe to call afterward.
    fn release_as_input(mut self) -> Pin<I, FunctionSio<SioInput>, PullNone> {
        match self.0.take().expect("FlexPin poisoned") {
            FlexPinInner::In(p) => p,
            FlexPinInner::Out(_) => panic!("FlexPin::release_as_input called while in output mode"),
        }
    }
}

impl<I: PinId> DigitalErrorType for FlexPin<I> {
    type Error = core::convert::Infallible;
}

impl<I> epdsi::bus3::DynamicPin for FlexPin<I>
where
    I: PinId + ValidFunction<FunctionSio<SioInput>> + ValidFunction<FunctionSio<SioOutput>>,
{
    type Error = core::convert::Infallible;

    fn set_as_output(&mut self) -> Result<(), Self::Error> {
        let inner = self.0.take().expect("FlexPin poisoned");
        self.0 = Some(match inner {
            FlexPinInner::In(p) => FlexPinInner::Out(p.into_push_pull_output()),
            out @ FlexPinInner::Out(_) => out,
        });
        Ok(())
    }

    fn set_as_input(&mut self) -> Result<(), Self::Error> {
        let inner = self.0.take().expect("FlexPin poisoned");
        self.0 = Some(match inner {
            FlexPinInner::Out(p) => FlexPinInner::In(p.into_floating_input()),
            input @ FlexPinInner::In(_) => input,
        });
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        match self.0.as_mut().expect("FlexPin poisoned") {
            FlexPinInner::Out(p) => p.set_high(),
            FlexPinInner::In(_) => panic!("FlexPin::set_high called while in input mode"),
        }
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        match self.0.as_mut().expect("FlexPin poisoned") {
            FlexPinInner::Out(p) => p.set_low(),
            FlexPinInner::In(_) => panic!("FlexPin::set_low called while in input mode"),
        }
    }

    fn is_high(&mut self) -> Result<bool, Self::Error> {
        match self.0.as_mut().expect("FlexPin poisoned") {
            FlexPinInner::In(p) => p.is_high(),
            FlexPinInner::Out(_) => panic!("FlexPin::is_high called while in output mode"),
        }
    }
}

/// Says which kind of unit a `R70H` reading belongs to, from the units seen so far.
fn describe_revision(rev: [u8; 3]) -> &'static str {
    match rev {
        [0x83, 0x02, 0x01] => "matches the GDEY029F52 unit seen (sticker 029SW-F52-B2)",
        [0x03, 0x02, 0x01] => "matches the JD79661AA/JD79667AA datasheet value",
        [0xFF, 0xFF, 0xFF] | [0x00, 0x00, 0x00] => "read path not reaching the IC, no answer",
        _ => "new pattern, record it with the rear sticker text and lot",
    }
}

/// 4-color options for 2bpp e-Paper display (JD79661)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuadColor {
    Black = 0b00,
    White = 0b01,
    Yellow = 0b10,
    Red = 0b11,
}

/// 2-bit per pixel buffer for 4-color displays (1 byte = 4 pixels)
pub struct QuadColorBuffer<'a> {
    buffer: &'a mut [u8],
    width: u32,
    height: u32,
    ram_stride: u32,
    rotation: DisplayRotation,
}

impl<'a> QuadColorBuffer<'a> {
    pub fn new(buffer: &'a mut [u8], width: u32, height: u32) -> Self {
        // Fill with White (0b01010101 = 0x55)
        buffer.fill(0x55);
        // RAM row stride is aligned to 8-pixel byte boundary (128 pixels / 32 bytes for 122px width)
        let ram_stride = width.div_ceil(8) * 8;
        Self {
            buffer,
            width,
            height,
            ram_stride,
            rotation: DisplayRotation::Rotate0,
        }
    }

    pub fn set_rotation(&mut self, rotation: DisplayRotation) {
        self.rotation = rotation;
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: QuadColor) {
        let (mapped_x, mapped_y) = match self.rotation {
            DisplayRotation::Rotate0 => (x, y),
            DisplayRotation::Rotate90 => (self.width.saturating_sub(1).saturating_sub(y), x),
            DisplayRotation::Rotate180 => (
                self.width.saturating_sub(1).saturating_sub(x),
                self.height.saturating_sub(1).saturating_sub(y),
            ),
            DisplayRotation::Rotate270 => (y, self.height.saturating_sub(1).saturating_sub(x)),
        };

        if mapped_x >= self.width || mapped_y >= self.height {
            return;
        }

        let pixel_index = mapped_y * self.ram_stride + mapped_x;
        let byte_index = (pixel_index / 4) as usize;
        let pixel_offset = 3 - (pixel_index % 4);
        let bit_shift = pixel_offset * 2;

        if byte_index < self.buffer.len() {
            let mask = !(0b11 << bit_shift);
            let val = (color as u8) << bit_shift;
            self.buffer[byte_index] = (self.buffer[byte_index] & mask) | val;
        }
    }

    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: QuadColor) {
        for px in x..(x + w) {
            for py in y..(y + h) {
                self.set_pixel(px, py, color);
            }
        }
    }

    pub fn draw_rect_outline(&mut self, x: u32, y: u32, w: u32, h: u32, color: QuadColor) {
        if w == 0 || h == 0 {
            return;
        }
        for px in x..(x + w) {
            self.set_pixel(px, y, color);
            self.set_pixel(px, y + h - 1, color);
        }
        for py in y..(y + h) {
            self.set_pixel(x, py, color);
            self.set_pixel(x + w - 1, py, color);
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.buffer
    }
}

impl<'a> DrawTarget for QuadColorBuffer<'a> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            if coord.x >= 0 && coord.y >= 0 {
                let c = match color {
                    BinaryColor::On => QuadColor::Black,
                    BinaryColor::Off => QuadColor::White,
                };
                self.set_pixel(coord.x as u32, coord.y as u32, c);
            }
        }
        Ok(())
    }
}

impl<'a> Dimensions for QuadColorBuffer<'a> {
    fn bounding_box(&self) -> Rectangle {
        let (w, h) = match self.rotation {
            DisplayRotation::Rotate0 | DisplayRotation::Rotate180 => (self.width, self.height),
            DisplayRotation::Rotate90 | DisplayRotation::Rotate270 => (self.height, self.width),
        };
        Rectangle::new(Point::zero(), Size::new(w, h))
    }
}

/// Page 1: geometry check.
fn draw_geometry_page(display: &mut QuadColorBuffer) {
    display.draw_rect_outline(0, 0, 128, 296, QuadColor::Black);
    display.fill_rect(1, 1, 24, 24, QuadColor::Red);
    // Three bands across the middle third, one per ink.
    display.fill_rect(1, 99, 42, 98, QuadColor::Black);
    display.fill_rect(43, 99, 42, 98, QuadColor::Yellow);
    display.fill_rect(85, 99, 42, 98, QuadColor::Red);
}

/// Page 2: the same page as the other colour examples.
fn draw_demo_page(display: &mut QuadColorBuffer) {
    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    // Outer border
    display.draw_rect_outline(0, 0, 128, 296, QuadColor::Black);

    Text::new("GDEY029F52", Point::new(14, 18), text_style)
        .draw(display)
        .unwrap();

    Line::new(Point::new(6, 23), Point::new(121, 23))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display)
        .unwrap();

    Text::new("JD79661 EPD", Point::new(9, 40), text_style)
        .draw(display)
        .unwrap();

    // Colour bar: one swatch per ink, so a wrong plane mapping is obvious at a glance.
    display.draw_rect_outline(9, 48, 110, 16, QuadColor::Black);
    display.fill_rect(11, 50, 25, 12, QuadColor::Black);
    display.fill_rect(38, 50, 25, 12, QuadColor::Yellow);
    display.fill_rect(65, 50, 25, 12, QuadColor::Red);
    display.fill_rect(92, 50, 25, 12, QuadColor::White);
    display.draw_rect_outline(92, 50, 25, 12, QuadColor::Black);

    // Logos, horizontally centred: (128 - 64) / 2 = 32.
    let ferris_offset = Point::new(32, 78);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_offset, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

    let rust_offset = Point::new(32, 150);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_offset, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

    Text::new("RP2040", Point::new(34, 262), text_style)
        .draw(display)
        .unwrap();

    Text::new("epdsi BWRY", Point::new(14, 284), text_style)
        .draw(display)
        .unwrap();
}

/// Refreshes the panel and returns the elapsed milliseconds.
fn timed_refresh<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, timer: &mut Timer) -> u64
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    let start = timer.get_counter().ticks();
    epd.refresh(timer).unwrap();
    (timer.get_counter().ticks() - start) / 1000
}

#[entry]
fn main() -> ! {
    let bbq = defmt_bbq::init().unwrap();

    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let mut sio = Sio::new(pac.SIO);

    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
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

    // ThinkInk EPD connections, fixed by the board's FPC socket. SCK and MOSI start as plain GPIO
    // so the panel's revision register can be read over a bit-banged 3-wire bus first, then move
    // into the SPI peripheral.
    let miso = pins.gpio20.into_function::<FunctionSpi>();
    let cs = pins.gpio19.into_push_pull_output();
    let dc = pins.gpio18.into_push_pull_output();
    let mut rst = pins.gpio17.into_push_pull_output();
    let sck_gpio = pins.gpio22.into_push_pull_output();
    let mosi_flex = FlexPin::new_output(pins.gpio23.into_push_pull_output());
    // BUSY pull follows the RP2350 example, the reference verified on known-good hardware.
    // xiao-esp32c3-blinky uses Pull::Up here and its BRINGUP.md calls JD79661 active-LOW; that
    // host was later found faulty, so RP2350 is the one to trust until it is re-tested.
    let busy = pins.gpio16.into_pull_down_input();

    // --- Which panel is this? Read R70H before SPI0 takes SCK and MOSI. ---
    // Reset with the demo's timings, then read the register. Nothing is powered on or
    // refreshed here. The log line is buffered by defmt-bbq until the USB pump below starts.
    timer.delay_ms(20);
    rst.set_low().unwrap();
    timer.delay_ms(40);
    rst.set_high().unwrap();
    timer.delay_ms(50);

    let mut id_bus = Spi3Bus::new(cs, sck_gpio, mosi_flex, dc, rst, busy);
    id_bus.wait_busy(&mut timer).unwrap();
    let rev = id_bus.read_jd7966x_revision(&mut timer).unwrap();
    defmt::info!("Panel R70H = {:02X}: {}", rev, describe_revision(rev));
    let (cs, sck_gpio, mosi_flex, dc, rst, busy) = id_bus.release();

    // The revision read always ends in input mode, so the data pin can be released as an input.
    let sck = sck_gpio.into_function::<FunctionSpi>();
    let mosi = mosi_flex.release_as_input().into_function::<FunctionSpi>();

    let spi = spi::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        4_000_000u32.Hz(),
        embedded_hal::spi::MODE_0,
    );

    // `pac.RESETS` is free of further borrows past this point, so hand USB servicing to core1: it
    // polls independently of whatever core0 does next, so `epd.refresh()` can block for as long as
    // it needs to without starving the USB device.
    spawn_usb_log_pump(
        Core1Handles {
            psm: &mut pac.PSM,
            ppb: &mut pac.PPB,
            fifo: &mut sio.fifo,
        },
        "GDEY029F52 2.9\" Quad-Color (epdsi JD79661, Feather RP2040)",
        "Feather RP2040 GDEY029F52",
        UsbParts {
            regs: pac.USBCTRL_REGS,
            dpram: pac.USBCTRL_DPRAM,
            clock: clocks.usb_clock,
        },
        pac.RESETS,
        watchdog,
        bbq,
    );

    // `SpiBusWrapper` expects the `SpiDevice` to own CS, unlike the hand-rolled `jd79661` example.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();

    // Instantiate epdsi SPI bus wrapper and JD79661 controller
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Jd79661Controller::new(GDEY029F52::WIDTH, GDEY029F52::HEIGHT);

    // Build EPD Driver using epdsi with the GDEY029F52 panel specification (128x296)
    let mut epd = EpdBuilder::<_, GDEY029F52>::new(controller).build(epd_bus);

    // 2bpp frame buffer: 32 bytes per row x 296 rows = 9,472 bytes
    let mut frame_buf = [0x55u8; 9472];
    let mut display = QuadColorBuffer::new(&mut frame_buf, 128, 296);
    display.set_rotation(DisplayRotation::Rotate0);

    for page in 1..=2 {
        defmt::info!("--- page {} of 2 ---", page);
        epd.init(&mut timer).unwrap();

        // Start each page from white, so page 1 does not bleed into page 2.
        display.fill_rect(0, 0, 128, 296, QuadColor::White);
        if page == 1 {
            draw_geometry_page(&mut display);
        } else {
            draw_demo_page(&mut display);
        }

        epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
            .unwrap();

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!("Full quad-color refresh, page {}: {} ms", page, ms);

        epd.sleep(&mut timer).unwrap();

        if page == 1 {
            defmt::info!("Resting 20 s before page 2...");
            timer.delay_ms(20_000);
        }
    }

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
