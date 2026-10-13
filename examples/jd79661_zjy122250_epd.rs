//! # Good Display ZJY122250-0213AJH-E5 E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery)
//! to the Adafruit Feather RP2040 ThinkInk, driving the
//! **ZJY122250-0213AJH-E5** 2.13" 4-Color (Black/White/Yellow/Red, 122x250) E-Paper Display using the
//! `Jd79661Controller` from the `epdsi` library.
//!
//! ## Panel check
//!
//! Before the refresh the example resets the panel and reads its `R70H` revision register over
//! a bit-banged 3-wire bus, before SPI takes SCK and MOSI, then logs which unit it is. Seen so
//! far: `09 01 01` (Adafruit `ZJY122250`, this example), `09 02 01` and `89 02 01` (the
//! `0213SW-F52-B2` F52 units, which want `jd79676_gdey0213f52_epd`). It never powers on or
//! refreshes the panel, and the example carries on whatever it reads.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** ZJY122250 / GDEY0213F51 2.13" Quad-Color, 122x250 (JD79661), seated directly in the board's 24-pin FPC socket.
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
//! than RTT. USB needs polling every few milliseconds and `epd.refresh()` blocks for seconds, so
//! USB is serviced on core1 while core0 runs the panel — see
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report). Each phase's timing is logged
//! as soon as it completes.
//!
//! **Watch the panel meanwhile.** A stage that completes fast *without* visibly changing the
//! display has not driven the ink, and no timing figure will tell you that.
//!
//! ## Measured output
//!
//! ```text
//! === ZJY122250 2.13" Quad-Color (epdsi JD79661, Feather RP2040) ===
//! Full quad-color refresh: 20041 ms
//! === done ===
//! ```
//!
//! A later run (12 Oct 2026) with the panel check added read `R70H` = `09 01 01`, logged
//! "matches the Adafruit ZJY122250 unit seen" ahead of the banner, and measured 20297 ms.
//!
//! All four colours render correctly. This is the first recorded figure for this panel — the only
//! prior reference anywhere was "several s" in `xiao-esp32c3-blinky/BRINGUP.md`, which is too
//! vague to check anything against.
//!
//! Note it lands within 3 ms of the 2.66" tri-color's 20044 ms on this same board. Two pigments on
//! a 122x250 panel costing the same as two pigments on a 152x296 one is consistent with colour
//! refresh time being set by the waveform rather than by area.
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example jd79661_zjy122250_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/jd79661_zjy122250_epd
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

/// Says which rear-sticker family a `R70H` reading belongs to, from the units seen so far.
fn describe_revision(rev: [u8; 3]) -> &'static str {
    match rev {
        [0x09, 0x01, 0x01] => {
            "matches the Adafruit ZJY122250 unit seen (sticker ZJY122250-0213EAH-E5)"
        }
        [0x03, 0x02, 0x01] => "matches the JD79661AA datasheet (not seen on a unit before)",
        [0x09, 0x02, 0x01] | [0x89, 0x02, 0x01] => {
            "F52 unit (0213SW-F52-B2): this example is for the F51, use jd79676_gdey0213f52_epd"
        }
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
        "ZJY122250 2.13\" Quad-Color (epdsi JD79661, Feather RP2040)",
        "Feather RP2040 ZJY122250",
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
    let controller =
        Jd79661Controller::new(ZJY122250_0213AJH_E5::WIDTH, ZJY122250_0213AJH_E5::HEIGHT);

    // Build EPD Driver using epdsi with ZJY122250_0213AJH_E5 panel specification (122x250)
    let mut epd = EpdBuilder::<_, ZJY122250_0213AJH_E5>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Allocate 2bpp frame buffer: 32 bytes/row * 250 rows = 8,000 bytes
    let mut frame_buf = [0x55u8; 8000];
    let mut display = QuadColorBuffer::new(&mut frame_buf, 122, 250);

    // Set portrait orientation (122 width, 250 height) to match PDI examples
    display.set_rotation(DisplayRotation::Rotate0);

    // Load BMP images
    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    // Outer border
    display.draw_rect_outline(0, 0, 122, 250, QuadColor::Black);

    // Header text
    Text::new("ZJY122250", Point::new(16, 18), text_style)
        .draw(&mut display)
        .unwrap();

    // Separator line
    Line::new(Point::new(6, 23), Point::new(115, 23))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut display)
        .unwrap();

    // Subtitle
    Text::new("JD79661 EPD", Point::new(6, 40), text_style)
        .draw(&mut display)
        .unwrap();

    // Quad-Color preview bounding box & color swatches
    display.draw_rect_outline(6, 48, 110, 16, QuadColor::Black);
    // 1. Black swatch
    display.fill_rect(8, 50, 25, 12, QuadColor::Black);
    // 2. Yellow swatch
    display.fill_rect(35, 50, 25, 12, QuadColor::Yellow);
    // 3. Red swatch
    display.fill_rect(62, 50, 25, 12, QuadColor::Red);
    // 4. White swatch with inner border
    display.fill_rect(89, 50, 25, 12, QuadColor::White);
    display.draw_rect_outline(89, 50, 25, 12, QuadColor::Black);

    // Draw Ferris logo (centered: (122 - 64)/2 = 29)
    let ferris_offset = Point::new(29, 68);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_offset, BinaryColor::On)
                .draw(&mut display)
                .unwrap();
        }
    }

    // Draw Rust logo (centered: (122 - 64)/2 = 29)
    let rust_offset = Point::new(29, 138);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_offset, BinaryColor::On)
                .draw(&mut display)
                .unwrap();
        }
    }

    // Text labels
    Text::new("RP2040", Point::new(31, 220), text_style)
        .draw(&mut display)
        .unwrap();

    Text::new("epdsi BWRY", Point::new(11, 240), text_style)
        .draw(&mut display)
        .unwrap();

    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();

    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Full quad-color refresh: {} ms", ms);

    epd.sleep(&mut timer).unwrap();

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
