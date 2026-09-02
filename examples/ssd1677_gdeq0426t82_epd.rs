//! # Good Display GDEQ0426T82 4.26" Monochrome E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery)
//! to the Adafruit Feather RP2040 ThinkInk, driving the
//! **GDEQ0426T82** 4.26" Monochrome (Black/White, 800x480) E-Paper Display using the
//! `Ssd1677Controller` from the `epdsi` library.
//!
//! Demonstrates:
//! 0. **Phase 0**: Refreshes a cleared frame on its own, so the RAM auto-fill path added in
//!    epdsi 0.1.7 is checked on glass. Expect a uniform white panel; banding or a half-inverted
//!    plane means the `0x46`/`0x47` step encoding is wrong for this panel.
//! 1. **Phase 1**: Full monochrome refresh displaying a header bar, 4x-scaled Ferris and Rust
//!    logos stacked vertically, and text labels.
//! 2. **Phase 2**: Fast *differential* refresh loop that swaps the Ferris and Rust logos on every
//!    pass and advances a progress bar. Only the pixels that actually change move, so the header
//!    stays rock steady even though the whole frame is re-sent.
//! 3. **Phase 3**: Full-waveform cleanup pass, restoring the ink density that the shortened
//!    differential waveform leaves behind.
//!
//! ## Note on orientation
//!
//! The panel's native RAM layout is 800x480 landscape with the origin at the end away from the
//! FPC ribbon. This example renders **portrait, 480x800, with the ribbon at the bottom**, matching
//! the other `epdsi` examples in this repository, by applying [`DisplayRotation::Rotate270`] to
//! the [`PageBuffer`]: logical `(x, y)` maps to RAM `(y, 479 - x)`. Drawing coordinates below are
//! therefore in 480-wide by 800-tall portrait space.
//!
//! Because a 90-degree rotation turns a logical horizontal band into a *vertical* strip of
//! controller RAM, the banded sub-buffer trick used by the smaller `epdsi` examples does not apply
//! here. Phase 2 re-sends the whole frame instead, exactly as GxEPD2's `refresh(true)` does.
//!
//! ## Note on the reversed Y axis
//!
//! This panel's gates are physically wired in reverse and the SSD1677 has no gate-scan-direction
//! bit to compensate, so [`Ssd1677Controller`] flips the Y axis in software (Y-decrement data
//! entry mode) inside `set_window`/`set_cursor`. That is transparent to callers and is separate
//! from the portrait rotation applied above.
//!
//! ## Note on buffer size and refresh time
//!
//! 800 px is byte-aligned, so a RAM row is exactly 100 bytes and the full frame is 48,000 bytes —
//! held as a plain stack array. The RP2040 has 264 KB of SRAM, so that still fits, though with
//! rather less headroom than the RP2350's 512 KB this was written against.
//! [`Ssd1677Controller`]'s `trigger_refresh` busy-spins on the BUSY line with no delay, so a full
//! update blocks for several seconds. The built-in fast LUT (`0x22 = 0xFC`, selected by
//! [`Ssd1677RefreshMode::Partial`]) is far quicker. Differential mode diffs the new image in
//! Black/White RAM (`0x24`) against the *previous* image in the secondary RAM (`0x26`, reached
//! through [`ColorChannel::RedYellow`]), so this example keeps `0x26` in sync after every refresh.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEQ0426T82 4.26" Monochrome, 800x480 (SSD1677, Seeed 6398), seated directly in the board's 24-pin FPC socket.
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
//! ## Results arrive at the end, not live
//!
//! No SWD connector is fitted on this board, so logging goes over USB CDC via `defmt-bbq` rather
//! than RTT. USB needs polling every few milliseconds and `epd.refresh()` blocks for seconds, so
//! the panel runs silently and every timing is reported once it finishes — see
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report).
//!
//! **Watch the panel meanwhile.** A stage that completes fast *without* visibly changing the
//! display has not driven the ink, and no timing figure will tell you that.
//!
//! ## Measured output
//!
//! ```text
//! === GDEQ0426T82 4.26" Mono (epdsi SSD1677, Feather RP2040) ===
//!   Phase 0 Clear: 3745 ms
//!   Phase 1 Full: 3744 ms
//!   Phase 2 Partial: 500 ms   (x6)
//!   Phase 3 cleanup Full: 3745 ms
//! === done ===
//! ```
//!
//! | Stage | This board | `BRINGUP.md` reference |
//! | :--- | ---: | ---: |
//! | Full | 3744 ms | ~3745 ms |
//! | Partial (full window) | **500 ms** | **~1000 ms** |
//! | Cleanup (full waveform) | 3745 ms | — |
//!
//! This is the largest DC-DC boost load of any panel here — 800x480, a 48,000-byte frame — and it
//! drives from the Feather without trouble.
//!
//! **The partial figure is half the reference and that is unexplained.** Phase 2 uses the full
//! window, not a band, so it is directly comparable. Full refresh matches to within a
//! millisecond, so the host is not simply faster; and the 2.13" panel's partial matched its own
//! reference exactly (1017 vs ~1017) on this same board, so the partial path is not generally
//! mis-measured either.
//!
//! That fits a pattern worth knowing: the reference figures in `xiao-esp32c3-blinky/BRINGUP.md`
//! were gathered over time on a module later found to be faulty, and they are of mixed
//! reliability rather than uniformly wrong. Some match this board exactly; the 1.54" panel's
//! ~14 s and this ~1000 ms do not. Running this example on RP2350 would settle which figure is
//! right. Until then, treat 500 ms as the measurement and ~1000 ms as unverified.
//!
//! ## Run
//!
//! ```bash
//! cargo run --release --example ssd1677_gdeq0426t82_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1677_gdeq0426t82_epd
//! ```
//!
//! The `until` loop is glob-free deliberately. In zsh an unmatched glob is a hard error raised by
//! the shell *before* the command runs, so `ls /dev/cu.usbmodemEPD* 2>/dev/null` still prints
//! `no matches found` and never executes `ls` — the redirect cannot suppress a message the shell
//! itself emits. Listing `/dev` and grepping avoids expansion entirely and behaves the same in
//! bash and zsh.
//!
//! Seeing `no matches found` from a bare `cat` just means the panel is still running: `cargo run`
//! returns after flashing, not after the run. Ctrl-C the `cat` once the output has printed.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use adafruit_feather_thinkink_discovery::usb_report::{report_and_park, Report, UsbParts};
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

/// Full frame buffer size: 100 bytes per RAM row x 480 rows = 48,000 bytes.
const FRAME_BYTES: usize = GDEQ0426T82::WIDTH.div_ceil(8) as usize * GDEQ0426T82::HEIGHT as usize;

/// Visible width in the rotated portrait frame (the panel's 480 px axis).
const VIEW_W: u32 = GDEQ0426T82::HEIGHT;

/// Visible height in the rotated portrait frame (the panel's 800 px axis).
const VIEW_H: u32 = GDEQ0426T82::WIDTH;

/// Bottom Y coordinate of the header bar. Its border is shared with the content area below.
const HEADER_H: u32 = 60;

/// Integer scale factor applied to both 64 px logo bitmaps. At 4x they become 256 px wide,
/// which suits a 480 px-wide portrait frame.
const LOGO_SCALE: u32 = 4;

/// X coordinate that horizontally centres a 256 px scaled logo in the 480 px portrait frame.
const LOGO_X: i32 = (VIEW_W as i32 - 64 * LOGO_SCALE as i32) / 2;

/// Y coordinate of the top of the upper logo slot.
const LOGO_TOP_Y: i32 = 110;

/// Y coordinate both logo stacks are bottom-aligned to.
const LOGO_BOTTOM_Y: i32 = 574;

/// Draws a 1 bpp bitmap scaled up by [`LOGO_SCALE`], with each source pixel becoming a filled
/// square. `ink` selects which [`BinaryColor`] counts as set in the source: the Ferris and Rust
/// BMPs ship with opposite polarity.
fn draw_scaled(display: &mut PageBuffer, bmp: &Bmp<BinaryColor>, origin: Point, ink: BinaryColor) {
    let fill = PrimitiveStyle::with_fill(BinaryColor::On);
    let scale = LOGO_SCALE as i32;

    for pixel in bmp.pixels() {
        if pixel.1 == ink {
            Rectangle::new(
                origin + Point::new(pixel.0.x * scale, pixel.0.y * scale),
                Size::new(LOGO_SCALE, LOGO_SCALE),
            )
            .into_styled(fill)
            .draw(display)
            .unwrap();
        }
    }
}

/// Draws the Ferris and Rust logos stacked vertically, horizontally centred.
///
/// `swapped` exchanges which logo occupies the upper slot: Phase 2 flips it on every differential
/// update so the refresh is obvious at a glance. Ferris is 64x42 and Rust is 64x64 before scaling,
/// and the offsets are chosen so both arrangements span the same [`LOGO_TOP_Y`]..[`LOGO_BOTTOM_Y`].
fn draw_logos(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    swapped: bool,
) {
    let ferris_h = (ferris_bmp.size().height * LOGO_SCALE) as i32;
    let rust_h = (rust_bmp.size().height * LOGO_SCALE) as i32;

    let (ferris_y, rust_y) = if swapped {
        (LOGO_BOTTOM_Y - ferris_h, LOGO_TOP_Y)
    } else {
        (LOGO_TOP_Y, LOGO_BOTTOM_Y - rust_h)
    };

    // The Ferris BMP has the opposite polarity to the Rust BMP, hence the differing `ink`.
    draw_scaled(
        display,
        ferris_bmp,
        Point::new(LOGO_X, ferris_y),
        BinaryColor::Off,
    );
    draw_scaled(
        display,
        rust_bmp,
        Point::new(LOGO_X, rust_y),
        BinaryColor::On,
    );
}

/// Renders the complete portrait frame: header bar, outer border, both logos, footer labels,
/// update counter, and progress bar.
fn draw_frame(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    swapped: bool,
    count: u32,
) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    // Header bar. Height HEADER_H + 1 puts its bottom edge exactly on the content area's top
    // edge, so the two rectangles meet on a single line rather than doubling up.
    Rectangle::new(Point::new(0, 0), Size::new(VIEW_W, HEADER_H + 1))
        .into_styled(stroke)
        .draw(display)
        .unwrap();

    Text::new("GDEQ0426T82  4.26\"", Point::new(24, 38), text_style)
        .draw(display)
        .unwrap();

    // Content area border
    Rectangle::new(
        Point::new(0, HEADER_H as i32),
        Size::new(VIEW_W, VIEW_H - HEADER_H),
    )
    .into_styled(stroke)
    .draw(display)
    .unwrap();

    draw_logos(display, ferris_bmp, rust_bmp, swapped);

    // Separator above the footer
    Line::new(Point::new(24, 620), Point::new(455, 620))
        .into_styled(stroke)
        .draw(display)
        .unwrap();

    Text::new("Feather RP2040", Point::new(24, 656), text_style)
        .draw(display)
        .unwrap();

    Text::new("epdsi SSD1677", Point::new(24, 682), text_style)
        .draw(display)
        .unwrap();

    // Update counter label
    let mut count_buf = [0u8; 32];
    let count_str = format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap();
    Text::new(count_str, Point::new(24, 720), text_style)
        .draw(display)
        .unwrap();

    // Progress bar outline
    Rectangle::new(Point::new(24, 732), Size::new(432, 28))
        .into_styled(stroke)
        .draw(display)
        .unwrap();

    // Progress bar fill
    Rectangle::new(Point::new(27, 735), Size::new(count * 71, 22))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(display)
        .unwrap();
}

/// Refreshes the panel and returns the elapsed milliseconds. Nothing is logged during the run:
/// USB is not up yet, and `defmt-bbq` discards buffered data while the device is unconfigured.
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
    let mut report = Report::new();

    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

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
    // SSD1677 BUSY is active-HIGH, so pull down: a floating line reads "idle".
    let busy = pins.gpio16.into_pull_down_input();

    let spi = spi::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        4_000_000u32.Hz(),
        embedded_hal::spi::MODE_0,
    );

    // `SpiBusWrapper` expects the `SpiDevice` to own CS, unlike the hand-rolled `jd79661` example.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();

    // Instantiate epdsi SPI bus wrapper and dedicated SSD1677 controller
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1677Controller::new(GDEQ0426T82::WIDTH, GDEQ0426T82::HEIGHT);

    // Build EPD Driver using epdsi with GDEQ0426T82 panel specification (800x480)
    let mut epd = EpdBuilder::<_, GDEQ0426T82>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Clear both display controller RAM banks to white. On this monochrome panel the secondary
    // RAM (0x26) is not a color plane but the "previous image" used by differential updates.
    // Re-assert window + cursor before each bank: `write_frame` starts from the window origin,
    // and the window is what a clear paints.
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.clear_frame(ColorChannel::BlackWhite, 0xFF).unwrap();

    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0xFF).unwrap();

    // --- Phase 0: show the cleared frame ------------------------------------------------
    //
    // Refresh here rather than drawing straight over the cleared RAM, so the clear is visible on
    // glass instead of being inferred from what comes after it.
    //
    // Since epdsi 0.1.7 a uniform full-plane clear is painted by the controller's own RAM
    // pattern generator (`0x46`/`0x47`) rather than streamed byte by byte. That register is a
    // *regular pattern* generator: if its step encoding were wrong, the plane would come out
    // banded or half-inverted rather than raising an error, and every later phase would happily
    // draw on top of the damage. No vendor reference driver uses these registers, so this screen
    // is the verification.
    //
    // Expect: a completely uniform white panel, no bands, no split down the middle, no darker
    // half. Anything else means the fast path is wrong on this panel — rebuild with
    // `Ssd1677Controller::new(..).with_ram_auto_fill(false)` to fall back to the byte stream and
    // confirm the panel itself is fine.
    report.record("Phase 0 Clear", timed_refresh(&mut epd, &mut timer));

    // Frame buffer: 800 x 480 / 8 = 48,000 bytes (0xFF = white)
    let mut bw_buf = [0xFFu8; FRAME_BYTES];

    // Load BMP images
    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // The buffer keeps the panel's native 800x480 RAM geometry; Rotate270 turns it into a
    // 480x800 portrait drawing surface with the FPC ribbon at the bottom.
    let mut display = PageBuffer::new(&mut bw_buf, GDEQ0426T82::WIDTH, GDEQ0426T82::HEIGHT, 0);
    display.set_rotation(DisplayRotation::Rotate270);

    draw_frame(&mut display, &ferris_bmp, &rust_bmp, false, 0);

    // Each RAM write starts from the window origin, so reset window + cursor first.
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();

    report.record("Phase 1 Full", timed_refresh(&mut epd, &mut timer));

    // Seed the "previous image" RAM (0x26) with what is now physically on the panel, so the
    // Phase 2 differential updates have a correct base to diff against.
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, display.as_slice())
        .unwrap();

    timer.delay_ms(2000);

    // Select the SSD1677 built-in fast LUT (0x22 = 0xFC), a real differential update on this
    // monochrome panel: only pixels that differ from RAM 0x26 are driven.
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Partial);

    for count in 1..=6u32 {
        // Flip the logo order on every pass. This is the whole point of the phase: a swap of two
        // 256 px logos is impossible to miss, where a progress bar alone is easy to overlook.
        let swapped = count % 2 == 1;

        display.clear_byte(0xFF);
        draw_frame(&mut display, &ferris_bmp, &rust_bmp, swapped, count);

        epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
            .unwrap();
        epd.set_cursor(0, 0).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
            .unwrap();

        report.record("Phase 2 Partial", timed_refresh(&mut epd, &mut timer));

        // Copy the frame we just displayed into the "previous image" RAM so the next iteration
        // diffs against what is actually on the panel.
        epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
            .unwrap();
        epd.set_cursor(0, 0).unwrap();
        epd.write_frame(ColorChannel::RedYellow, display.as_slice())
            .unwrap();

        timer.delay_ms(1000);
    }

    // Differential updates drive the pixels with a much shorter waveform than the OTP full-refresh
    // LUT, so pixels that flip white -> black during Phase 2 settle at a dark grey rather than a
    // deep black, while pixels already black from Phase 1 keep their full density. Re-running the
    // final frame through the full waveform restores even ink density across the panel.
    // Blanking the secondary RAM first also stops it being interpreted as a second color plane.
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Full);

    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0xFF).unwrap();

    // `display` still holds the last frame drawn in Phase 2, so re-send it unchanged.
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();

    report.record("Phase 3 cleanup Full", timed_refresh(&mut epd, &mut timer));

    report_and_park(
        "GDEQ0426T82 4.26\" Mono (epdsi SSD1677, Feather RP2040)",
        "Feather RP2040 GDEQ0426T82",
        &report,
        UsbParts {
            regs: pac.USBCTRL_REGS,
            dpram: pac.USBCTRL_DPRAM,
            clock: clocks.usb_clock,
        },
        &mut pac.RESETS,
        &mut watchdog,
        bbq,
    )
}
