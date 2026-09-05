//! # Waveshare 3.52" (B) SE0352N14TNGA0 Tri-Color E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery)
//! to the Adafruit Feather RP2040 ThinkInk, driving the
//! **SE0352N14TNGA0** 3.52" Tri-Color (Black/White/Red, 240x360) E-Paper Display using the
//! `Uc8253Controller` from the `epdsi` library, in its `Uc8253Variant::Se0352n14` profile.
//!
//! Demonstrates:
//! 1. **Phase 1**: A white | black | red panel test that makes miswiring self-evident.
//! 2. **Phase 2**: Full Tri-Color refresh showing a header, the Rust logo in black, the Ferris
//!    logo in red, text labels and a red accent bar.
//!
//! The test runs first so the panel is left showing the picture, matching the other EPD examples
//! in this repo.
//!
//! ## Note on this panel versus the other UC8253 panel
//!
//! The same driver IC drives the `GDEY037T03` in this repo, but the two panels are **not**
//! interchangeable behind one register profile, hence `Uc8253Variant`:
//!
//! - The Black/White plane is [`ColorChannel::BlackWhite`] -> `WRITE_OLD_DATA` (`0x10`) and red is
//!   [`ColorChannel::RedYellow`] -> `WRITE_NEW_DATA` (`0x13`). That is **swapped** relative to the
//!   `GDEY037T03`. It follows from the `KW/R` bit in Panel Setting selecting KWR mode.
//! - **Ink is a set bit and `0x00` is white, in both planes** — the opposite of the monochrome
//!   panel's `0xFF`. `PageBuffer` natively treats a *cleared* bit as ink, so both buffers start at
//!   `0x00` and everything is drawn with [`BinaryColor::Off`], aliased to `INK` below.
//! - **Full refresh only**, roughly 16-20 s. There is no partial or fast waveform: the red pigment
//!   needs the full OTP waveform. `Uc8253RefreshMode` is ignored for this variant, and no
//!   `set_window` call is made — a full-frame write must not be wrapped in a partial-window
//!   session.
//! - The controller drops its charge pump after each update, so `epdsi` issues `POWER_ON` at the
//!   start of every refresh. Skipping that does not error — `DISPLAY_REFRESH` is silently ignored,
//!   BUSY never asserts, and the refresh appears to finish instantly having drawn nothing.
//! - BUSY is active-**LOW**, as on the `GDEY037T03`, so the GPIO takes a pull-**up**.
//! - Orientation: 240 px across x 360 lines, which is the raster, not Waveshare's advertised
//!   360x240 landscape viewing orientation. This panel's native `(0,0)` is top-left with the FPC
//!   ribbon at the **top** (verified on hardware), which is the opposite of the `GDEY037T03` and
//!   `GDEQ0426T82` examples here. To keep one convention across the repo, every drawing surface is
//!   built through `frame()`, which applies [`DisplayRotation::Rotate180`] — so this example, like
//!   the others, renders **with the ribbon at the bottom**. Drop the rotation in `frame()` for the
//!   panel's raw orientation.
//!
//! ## Reading the Phase 1 panel test
//!
//! The bands are, left to right, **white | black | red**:
//!
//! - Black and red swapped -> the RAM plane routing is crossed (`0x10`/`0x13`).
//! - White and black swapped -> `CDI`/DDX polarity is wrong, or the buffers were not `0x00`-based.
//! - A cleared panel reading grey rather than white is this panel's white point, **not** a fault.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Waveshare 3.52" e-Paper HAT (B) panel SE0352N14-TNG-A0, 240x360 Tri-Color (UC8253), seated directly in the board's 24-pin FPC socket.
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
//! ## A refresh under a second means BUSY never asserted
//!
//! This panel takes ~16-20 s. The RP2350 version of this example warns inline when a refresh
//! returns faster than that; this port carries the same expectation in the logged labels instead
//! — a reading of a few milliseconds against a stated `~17 s` means the controller ignored
//! `DISPLAY_REFRESH` and the busy poll read idle, not that the panel is fast.
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
//! === SE0352N14TNGA0 3.52" Tri-Color (epdsi UC8253, Feather RP2040) ===
//! Phase 1 panel test: 17366 ms
//! Phase 2 Full tri-color: 17365 ms
//! === done ===
//! ```
//!
//! Two refreshes, because the example has two phases — the white | black | red panel test, then
//! the picture. Both land at ~17.37 s, matching the 16-20 s this panel is documented to take.
//!
//! **This panel does not work on the XIAO ESP32-C3**, per
//! `xiao-esp32c3-blinky/BRINGUP.md`, where refreshes return in ~0 ms having drawn nothing. That
//! finding is now in doubt: the C3 module used for it was later shown to be faulty by
//! substitution. Running here at the expected duration on the first attempt is a point against
//! the UC8253 being at fault, and in favour of the host having been.
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example uc8253_se0352n14_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/uc8253_se0352n14_epd
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
use bsp::hal::gpio::{FunctionSpi, Pins};
use bsp::hal::{spi, Clock, Sio, Timer, Watchdog};
use bsp::{entry, pac, XOSC_CRYSTAL_FREQ};

// defmt-bbq is the global logger here, not defmt-rtt. Only one may be linked.
use defmt_bbq as _;
use panic_probe as _;

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;
use tinybmp::Bmp;

/// Ink, in this panel's convention.
///
/// A **set** bit is ink in both RAM planes here, where `PageBuffer::set_pixel` treats a *cleared*
/// bit as ink (`BinaryColor::On` -> bit 0). So every draw call in this example uses
/// `BinaryColor::Off`, against buffers based at `0x00` rather than `0xFF`. Getting this backwards
/// renders a negative of the intended image.
const INK: BinaryColor = BinaryColor::Off;

/// Byte value that clears a plane to white on this panel.
const WHITE_BYTE: u8 = 0x00;

/// Row stride in bytes: 240 / 8 = 30. This panel is already byte-aligned.
const STRIDE: usize = SE0352N14TNGA0::WIDTH.div_ceil(8) as usize;

/// Frame buffer size for one plane: 30 x 360 = 10,800 bytes. Two planes are allocated, so this
/// example holds 21,600 bytes of frame buffer.
const FRAME_BYTES: usize = STRIDE * SE0352N14TNGA0::HEIGHT as usize;

/// X coordinate of the left logo slot.
const LOGO_LEFT_X: i32 = 30;

/// X coordinate of the right logo slot.
const LOGO_RIGHT_X: i32 = 146;

/// Builds a full-frame drawing surface for one RAM plane, rotated to this repo's convention.
///
/// This panel's native `(0,0)` is top-left with the FPC ribbon at the **top**, but every other EPD
/// example here renders with the ribbon at the **bottom**, so the surface is rotated 180°. Going
/// through one constructor keeps that from being applied to three planes out of four, which would
/// show up as one colour appearing upside down relative to the others.
fn frame(buf: &mut [u8]) -> PageBuffer<'_> {
    let mut page = PageBuffer::new(buf, SE0352N14TNGA0::WIDTH, SE0352N14TNGA0::HEIGHT, 0);
    page.set_rotation(DisplayRotation::Rotate180);
    page
}

/// Draws a BMP into `display`, treating source pixels equal to `source_ink` as ink.
///
/// The two bundled logos disagree on polarity: `ferrisbw.bmp` marks its subject with
/// `BinaryColor::Off` and `rustbw.bmp` with `BinaryColor::On`, so the test colour is a parameter.
fn blit(display: &mut PageBuffer, bmp: &Bmp<BinaryColor>, origin: Point, source_ink: BinaryColor) {
    for Pixel(point, color) in bmp.pixels() {
        if color == source_ink {
            Pixel(point + origin, INK).draw(display).unwrap();
        }
    }
}

/// Draws the panel-test black plane: title, separator, the pattern frame, the black band and the
/// band labels.
fn draw_test_black_plane(display: &mut PageBuffer) {
    let text_style = MonoTextStyle::new(&FONT_10X20, INK);

    Text::new("PLANE TEST", Point::new(10, 24), text_style)
        .draw(display)
        .unwrap();

    Line::new(Point::new(10, 34), Point::new(229, 34))
        .into_styled(PrimitiveStyle::with_stroke(INK, 1))
        .draw(display)
        .unwrap();

    // Black frame around the whole pattern. Without it the white band has no visible edge at all
    // and the pattern reads as two bands on a blank panel rather than three. Outer edges land on
    // x = 11/228 and y = 50/289, leaving a 216 x 238 interior.
    Rectangle::new(Point::new(11, 50), Size::new(218, 240))
        .into_styled(PrimitiveStyle::with_stroke(INK, 1))
        .draw(display)
        .unwrap();

    // Middle third solid black, inset by one pixel so it abuts the frame rather than overlapping
    // it. The left third is left bare, so it shows the panel's white.
    Rectangle::new(Point::new(84, 51), Size::new(72, 238))
        .into_styled(PrimitiveStyle::with_fill(INK))
        .draw(display)
        .unwrap();

    // One label centred under each band (interior thirds are 72 px, so centres are x = 48, 120,
    // 192; FONT_10X20 glyphs are 10 px wide). The frame stops at y = 289, so these sit on white
    // and stay legible whichever plane misbehaves.
    for (label, centre_x) in [("W", 48), ("B", 120), ("R", 192)] {
        Text::new(label, Point::new(centre_x - 5, 320), text_style)
            .draw(display)
            .unwrap();
    }
}

/// Draws the panel-test red plane: the right-hand red band.
///
/// Must not touch the frame drawn on the black plane — a pixel set in both planes has no defined
/// colour.
fn draw_test_red_plane(display: &mut PageBuffer) {
    Rectangle::new(Point::new(156, 51), Size::new(72, 238))
        .into_styled(PrimitiveStyle::with_fill(INK))
        .draw(display)
        .unwrap();
}

/// Draws the Phase 2 content black plane: border, header, separators and labels.
fn draw_black_plane(display: &mut PageBuffer, rust_bmp: &Bmp<BinaryColor>) {
    let style = PrimitiveStyle::with_stroke(INK, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, INK);

    Rectangle::new(
        Point::new(0, 0),
        Size::new(SE0352N14TNGA0::WIDTH, SE0352N14TNGA0::HEIGHT),
    )
    .into_styled(style)
    .draw(display)
    .unwrap();

    Text::new("SE0352N14", Point::new(10, 24), text_style)
        .draw(display)
        .unwrap();

    Text::new("3.52\" BWR", Point::new(10, 48), text_style)
        .draw(display)
        .unwrap();

    Line::new(Point::new(10, 58), Point::new(229, 58))
        .into_styled(style)
        .draw(display)
        .unwrap();

    blit(
        display,
        rust_bmp,
        Point::new(LOGO_RIGHT_X, 90),
        BinaryColor::On,
    );

    Text::new("Feather RP2040", Point::new(10, 200), text_style)
        .draw(display)
        .unwrap();

    Text::new("epdsi UC8253", Point::new(10, 225), text_style)
        .draw(display)
        .unwrap();

    Line::new(Point::new(10, 240), Point::new(229, 240))
        .into_styled(style)
        .draw(display)
        .unwrap();

    Text::new("Waveshare (B)", Point::new(10, 285), text_style)
        .draw(display)
        .unwrap();
}

/// Draws the Phase 2 content red plane: the Ferris logo and an accent bar.
///
/// Kept disjoint from the black plane — a pixel set in both planes has no defined colour.
fn draw_red_plane(display: &mut PageBuffer, ferris_bmp: &Bmp<BinaryColor>) {
    blit(
        display,
        ferris_bmp,
        Point::new(LOGO_LEFT_X, 112),
        BinaryColor::Off,
    );

    Rectangle::new(Point::new(10, 300), Size::new(220, 30))
        .into_styled(PrimitiveStyle::with_fill(INK))
        .draw(display)
        .unwrap();
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

    // ThinkInk EPD connections, fixed by the board's FPC socket.
    let sck = pins.gpio22.into_function::<FunctionSpi>();
    let mosi = pins.gpio23.into_function::<FunctionSpi>();
    let miso = pins.gpio20.into_function::<FunctionSpi>();
    let cs = pins.gpio19.into_push_pull_output();
    let dc = pins.gpio18.into_push_pull_output();
    let rst = pins.gpio17.into_push_pull_output();
    // UC8253 BUSY is active-LOW, so pull up: a floating line reads "idle".
    let busy = pins.gpio16.into_pull_up_input();

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
        "SE0352N14TNGA0 3.52\" Tri-Color (epdsi UC8253, Feather RP2040)",
        "Feather RP2040 SE0352N14",
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
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);

    // The variant is not optional. The default Gdey037t03 profile's init, plane order and CDI
    // value are all wrong for this panel, and it renders inverted or blank rather than erroring.
    let controller = Uc8253Controller::new(SE0352N14TNGA0::WIDTH, SE0352N14TNGA0::HEIGHT)
        .with_variant(Uc8253Variant::Se0352n14);
    let mut epd = EpdBuilder::<_, SE0352N14TNGA0>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Both planes start white. 0x00, not 0xFF — see INK above.
    epd.clear_frame(ColorChannel::BlackWhite, WHITE_BYTE)
        .unwrap();
    epd.clear_frame(ColorChannel::RedYellow, WHITE_BYTE)
        .unwrap();

    let mut bw_buf = [WHITE_BYTE; FRAME_BYTES];
    let mut red_buf = [WHITE_BYTE; FRAME_BYTES];

    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // The buffers start white, so nothing to clear yet.
    {
        let mut display_bw = frame(&mut bw_buf);
        draw_test_black_plane(&mut display_bw);
    }
    {
        let mut display_red = frame(&mut red_buf);
        draw_test_red_plane(&mut display_red);
    }

    // No set_window: a full-frame write must not be wrapped in a partial-window session, and this
    // panel has no partial mode anyway. Leaving the window unset keeps the SPI stream identical to
    // Waveshare's reference driver.
    epd.write_frame(ColorChannel::BlackWhite, &bw_buf).unwrap();
    epd.write_frame(ColorChannel::RedYellow, &red_buf).unwrap();

    let ms = refresh_timed(&mut epd, &mut timer);
    defmt::info!("Phase 1 panel test: {} ms", ms);

    timer.delay_ms(3000);

    // Drawn second so the panel is left showing the picture rather than the test pattern, matching
    // the other EPD examples in this repo.
    bw_buf.fill(WHITE_BYTE);
    red_buf.fill(WHITE_BYTE);

    {
        let mut display_bw = frame(&mut bw_buf);
        draw_black_plane(&mut display_bw, &rust_bmp);
    }
    {
        let mut display_red = frame(&mut red_buf);
        draw_red_plane(&mut display_red, &ferris_bmp);
    }

    epd.write_frame(ColorChannel::BlackWhite, &bw_buf).unwrap();
    epd.write_frame(ColorChannel::RedYellow, &red_buf).unwrap();

    let ms = refresh_timed(&mut epd, &mut timer);
    defmt::info!("Phase 2 Full tri-color: {} ms", ms);

    // After sleep the controller is in deep sleep: init() must be called again before drawing.
    epd.sleep(&mut timer).unwrap();

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}

/// Refreshes the panel and reports how long it took.
///
/// A full refresh on this panel is 16-20 s. Anything under a second means BUSY was never observed
/// asserted — most likely it had not gone low yet when the poll started, so the refresh is still
/// running and a following `sleep` would land mid-update.
fn refresh_timed<BUS, C, P, D>(epd: &mut EpdDriver<BUS, C, P>, timer: &mut D) -> u64
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
    D: DelayNs + FnCounter,
{
    let start = timer.now_us();
    epd.refresh(timer).unwrap();
    (timer.now_us().saturating_sub(start)) / 1_000
}

/// Minimal microsecond clock, so `refresh_timed` can stay generic over the delay provider.
trait FnCounter {
    fn now_us(&self) -> u64;
}

impl FnCounter for Timer {
    fn now_us(&self) -> u64 {
        self.get_counter().ticks()
    }
}
