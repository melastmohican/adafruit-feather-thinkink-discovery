//! # GDEY0266Z90 2.66" Tri-Color `PageBufferPair` Draw Target Example (`epdsi`)
//!
//! Full-parity companion to `ssd1680_gdey0266z90_epd` on this board — same four phases, same
//! content, same timing measurements, same board bring-up (BSP, dual-core USB logging, pin
//! assignments) — but drawn entirely through `PageBufferPair`/`TriColor` instead of two separate
//! `PageBuffer`s and panel-specific `BinaryColor::On`/`Off` polarity choices. Exists to prove the
//! new draw-target API can do everything the manual approach does, not just a minimal smoke test.
//!
//! 1. **Phase 1**: Full tri-color refresh ([`Ssd168xRefreshMode::Full`]) — header, Black and
//!    Accent swatches, Ferris logo (Accent), Rust logo (Black), and text labels.
//! 2. **Phase 2**: Windowed refresh loop on the full waveform, repainting only the bottom status
//!    band, leaving the logos untouched.
//! 3. **Phase 3**: [`Ssd168xRefreshMode::FastFull`], timed against Phase 1.
//! 4. **Phase 4**: [`Ssd168xRefreshMode::BaseMap`] and [`Ssd168xRefreshMode::Partial`], the two
//!    modes ported from Good Display's reference driver, shown at their real cost.
//!
//! See `ssd1680_gdey0266z90_epd` for the full narrative on refresh-mode behavior, ink physics,
//! duty cycle, and glass provenance — none of that changed, so it isn't repeated here. What
//! differs is only the drawing code:
//!
//! - One `page: &mut PageBufferPair` replaces the two `bw`/`red` `PageBuffer` parameters
//!   `draw_static_content`/`draw_band`/`draw_band_bar` used to take.
//! - `TriColor::{Black, Accent}` replaces every panel-polarity-aware `BinaryColor::On`/`Off`
//!   choice — no more picking `Off` "because the Red plane is inverted."
//! - `PageBufferPair::clear()` replaces the pair of `clear_byte(0xFF)` / `clear_byte(0x00)` calls
//!   before each windowed redraw — one call, no raw byte to keep in sync with polarity.
//! - The `NO_RED_BAND` static the original example wrote for the BaseMap phase (a workaround for
//!   needing to hand-compute an all-`0x00` array of the right size) is gone: `page.accent()`'s
//!   own background bytes, derived from `PlanePolarity`, already are that array.
//!
//! ## Why this board is the interesting one
//!
//! The Arduino sketches this panel was first brought up with — `GDEY0266Z90.ino`,
//! `Waveshare_2in66br` and the GxEPD2 `Demo.ino` — were all written for **this** Feather. So it is
//! the one place where GxEPD2 and `epdsi` can be run against identical hardware with the same
//! panel, and any difference is unambiguously a driver difference rather than a host or carrier
//! one. It also has no carrier board and no jumper wiring: the 24-pin FPC socket is on the PCB.
//!
//! ## Results arrive live, one phase at a time
//!
//! There is no debug probe on this board, so logging goes over USB CDC via `defmt-bbq` rather than
//! RTT. USB CDC needs `usb_dev.poll()` every few milliseconds and `epd.refresh()` blocks for ~20 s
//! at a time, so USB is serviced on core1 while core0 runs the phases — see
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report). Each phase's timing is logged
//! as soon as it completes.
//!
//! **Watch the panel meanwhile** — it is the better instrument. A stage that finishes fast without
//! visibly changing the image has not driven the ink.
//!
//! ## Reference timings
//!
//! Measured on RP2350 with this glass (`ssd1680_gdey0266z90_epd`, the manual `PageBuffer` version).
//! This example only changes how the buffers are built and drawn into, not the SPI sequence sent
//! to the panel, so timings should match within measurement noise. A large deviation here would be
//! a real regression, not just board variance.
//!
//! | Mode | RP2350 |
//! |---|---:|
//! | `Full` | 20045 ms |
//! | `Full`, windowed | 20049 ms |
//! | `FastFull` | **16181 ms** |
//! | `BaseMap` | 19909 ms |
//! | `Partial` | 19908 ms |
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEY0266Z90 / Waveshare 2.66inch e-Paper Module (B), 152x296 BWR,
//!   seated directly in the board's 24-pin FPC socket. The unit this was written for is DKE glass,
//!   stamped `DEPG0266RWS800F34HP`, ribbon `FPC-7510 Rev. C`. `S800` is the SSD1680; the same
//!   glass also ships with a JD79651B (`F51B`) or UC8251d (`U25D`), which this driver cannot drive.
//!
//! Connections are fixed by the socket — SCK GP22, MOSI GP23, CS GP19, DC GP18, RST GP17,
//! BUSY GP16. These are SPI0 on the RP2040, even though the Arduino core calls the port SPI1.
//!
//! **Swap panels with the board unpowered.**
//!
//! ## Note on duty cycle
//!
//! Waveshare recommend at least 180 s between refreshes on this panel, and one update every 24 h
//! to avoid burn-in. This example runs seven refreshes seconds apart, which is fine as a one-off
//! but **should not be looped**, and is not a model for production pacing.
//!
//! ## Measured output
//!
//! Same shape as `ssd1680_gdey0266z90_epd`'s output; only the timings may drift within measurement
//! noise, since the buffer construction changed but the SPI sequence did not:
//!
//! ```text
//! === GDEY0266Z90 2.66" Tri-Color (epdsi SSD1680, Feather RP2040) ===
//! Phase 1 Full: 20044 ms
//! Phase 2 windowed Full: 20045 ms
//! Phase 2 windowed Full: 20045 ms
//! Phase 3 FastFull: 16176 ms
//! Phase 4 BaseMap: 19905 ms
//! Phase 4 Partial: 19905 ms
//! Phase 4 Partial: 19905 ms
//! === done ===
//! ```
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example ssd1680_gdey0266z90_tri_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1680_gdey0266z90_tri_epd
//! ```
//!
//! USB comes up within about a second of boot now — core1 services it independently of the panel,
//! so the `until` loop above returns almost immediately instead of waiting for the run to finish.
//! The panel then works for **about two minutes** — seven refreshes at ~20 s each — logging each
//! phase as it completes.
//!
//! `cat` does not exit on its own — Ctrl-C once the output has printed.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` right after flashing just means enumeration hasn't
//! finished yet** — it should clear within a second or two, not the full two-minute run. If it
//! doesn't clear quickly, confirm the board was actually in bootloader mode before flashing.
//!
//! Every example in this repo uses the same USB serial, so that glob never changes. To see
//! which firmware is actually on the board, read the USB product string:
//!
//! ```bash
//! ioreg -r -c IOUSBHostDevice -l | grep -o '"USB Product Name" = "Feather[^"]*"'
//! ```
//!
//! The first decoded log line names it too.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::hal::clocks::init_clocks_and_plls;
use bsp::hal::fugit::RateExtU32;
use bsp::hal::gpio::{FunctionSpi, Pins};
use bsp::hal::{spi, Clock, Sio, Timer, Watchdog};
use bsp::{entry, pac, XOSC_CRYSTAL_FREQ};

// defmt-bbq is the global logger here, not defmt-rtt. Only one may be linked.
use defmt_bbq as _;
use panic_probe as _;

use adafruit_feather_thinkink_discovery::usb_report::{spawn_usb_log_pump, Core1Handles, UsbParts};

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;
use tinybmp::Bmp;

/// Row stride in bytes. 152 px is byte-aligned, so this is exactly 19 with no padding.
const STRIDE: usize = GDEY0266Z90::WIDTH.div_ceil(8) as usize;

/// Full frame buffer size per plane: 19 x 296 = 5,624 bytes.
const FRAME_BYTES: usize = STRIDE * GDEY0266Z90::HEIGHT as usize;

/// Top Y coordinate of the status band repainted in Phases 2 and 4. Everything above it is
/// painted in Phase 1 and never touched again, so the Accent logo stays put.
const BAND_Y: u32 = 220;

/// Height of the status band in pixels (y = 220..295).
const BAND_H: u32 = 76;

/// Status band buffer size: 19 x 76 = 1,444 bytes.
const BAND_BYTES: usize = STRIDE * BAND_H as usize;

/// The polarity this panel needs — Black/White plane normal, accent plane inverted. Passed to
/// every `PageBufferPair::new` call rather than assumed once, since a different panel could need
/// `PlanePolarity::UC8253` instead.
const POLARITY: PlanePolarity = PlanePolarity::SSD168X;

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

/// Draws the Phase 1 / Phase 3 static content: everything above the status band.
///
/// One `page` addresses both RAM planes — no separate `bw`/`red` buffers, and no polarity-aware
/// `BinaryColor` choice: `TriColor::Black`/`TriColor::Accent` say what they mean.
fn draw_static_content(
    page: &mut PageBufferPair,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    mode_label: &str,
) {
    let stroke = PrimitiveStyle::with_stroke(TriColor::Black, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, TriColor::Black);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, TriColor::Black);
    let accent_small_text_style = MonoTextStyle::new(&FONT_6X10, TriColor::Accent);

    // Outer border (Black), so a shifted or wrapped raster is obvious.
    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT),
    )
    .into_styled(stroke)
    .draw(page)
    .unwrap();

    // Header (Black). 11 chars at 10 px each fits the 152 px width.
    Text::new("GDEY0266Z90", Point::new(8, 22), text_style)
        .draw(page)
        .unwrap();

    // Subtitle: "Tri-Color " in Black, "BWR" in Accent.
    Text::new("Tri-Color ", Point::new(8, 40), small_text_style)
        .draw(page)
        .unwrap();
    Text::new("BWR", Point::new(68, 40), accent_small_text_style)
        .draw(page)
        .unwrap();

    Line::new(Point::new(8, 48), Point::new(143, 48))
        .into_styled(stroke)
        .draw(page)
        .unwrap();

    // Colour swatches: Black left, Accent right, inside a shared outline.
    Rectangle::new(Point::new(8, 56), Size::new(136, 18))
        .into_styled(stroke)
        .draw(page)
        .unwrap();
    Rectangle::new(Point::new(10, 58), Size::new(64, 14))
        .into_styled(PrimitiveStyle::with_fill(TriColor::Black))
        .draw(page)
        .unwrap();
    Rectangle::new(Point::new(78, 58), Size::new(64, 14))
        .into_styled(PrimitiveStyle::with_fill(TriColor::Accent))
        .draw(page)
        .unwrap();

    // Ferris (64x42) in Accent and Rust (64x64) in Black, side by side — 128 px of artwork fits
    // the 152 px width, unlike the 122 px monochrome panel where they have to be stacked. Both
    // drawn straight onto the same `page`, unlike the original example's separate `red`/`bw`
    // targets.
    let ferris_pos = Point::new(10, 92);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_pos, TriColor::Accent)
                .draw(page)
                .unwrap();
        }
    }

    let rust_pos = Point::new(78, 82);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_pos, TriColor::Black)
                .draw(page)
                .unwrap();
        }
    }

    // Labels (Black). Board name differs from the RP2350 original; everything else matches.
    Text::new("Feather RP2040", Point::new(8, 170), small_text_style)
        .draw(page)
        .unwrap();
    Text::new("epdsi SSD1680", Point::new(8, 184), small_text_style)
        .draw(page)
        .unwrap();
    Text::new(mode_label, Point::new(8, 198), small_text_style)
        .draw(page)
        .unwrap();

    // Separator above the status band that Phases 2 and 4 repaint.
    Line::new(Point::new(8, 210), Point::new(143, 210))
        .into_styled(stroke)
        .draw(page)
        .unwrap();
}

/// Writes both colour planes for the full frame, resetting the RAM window and cursor first.
///
/// Each RAM write restarts from the window origin, so the window and cursor have to be re-armed
/// before every plane rather than once per frame.
fn write_full_frame<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, page: &PageBufferPair)
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEY0266Z90::WIDTH - 1, GDEY0266Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, page.bw().as_slice())
        .unwrap();

    epd.set_window(0, 0, GDEY0266Z90::WIDTH - 1, GDEY0266Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, page.accent().as_slice())
        .unwrap();
}

/// Writes both colour planes for the status band window.
fn write_band<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, page: &PageBufferPair)
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, BAND_Y, GDEY0266Z90::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, page.bw().as_slice())
        .unwrap();

    epd.set_window(0, BAND_Y, GDEY0266Z90::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::RedYellow, page.accent().as_slice())
        .unwrap();
}

/// Draws the status band's Black content: label, counter and progress bar outline.
///
/// The bar *fill* is left to the caller as a separate call, always Accent.
fn draw_band(page: &mut PageBufferPair, count: u32, label: &str) {
    let stroke = PrimitiveStyle::with_stroke(TriColor::Black, 1);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, TriColor::Black);

    Text::new(label, Point::new(8, BAND_Y as i32 + 14), small_text_style)
        .draw(page)
        .unwrap();

    let mut count_buf = [0u8; 32];
    let count_str = format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap();
    Text::new(
        count_str,
        Point::new(8, BAND_Y as i32 + 28),
        small_text_style,
    )
    .draw(page)
    .unwrap();

    // Progress bar outline always lands on the Black/White plane.
    Rectangle::new(Point::new(8, BAND_Y as i32 + 38), Size::new(136, 16))
        .into_styled(stroke)
        .draw(page)
        .unwrap();
}

/// Draws the progress bar fill for `count`, always in Accent — the original example's
/// `draw_band_bar` took a `BinaryColor` parameter, but every call site passed the same value
/// (`BinaryColor::Off`, targeting the Red plane), so there was nothing for that parameter to
/// vary; `TriColor::Accent` is simply hardcoded here instead.
fn draw_band_bar(page: &mut PageBufferPair, count: u32) {
    Rectangle::new(
        Point::new(10, BAND_Y as i32 + 40),
        Size::new(count * 33, 12),
    )
    .into_styled(PrimitiveStyle::with_fill(TriColor::Accent))
    .draw(page)
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
    // SSD1680 BUSY is active-HIGH, so pull down: a floating line reads "idle".
    let busy = pins.gpio16.into_pull_down_input();

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
        "GDEY0266Z90 2.66\" Tri-Color (epdsi SSD1680, Feather RP2040)",
        "Feather RP2040 GDEY0266Z90",
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
    // No variant selection needed: this panel shares the default SSD1680 register profile with
    // the GDEM0213B74 that the diagnostics in this repo drive.
    let controller = Ssd1680Controller::new(GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT)
        .with_refresh_mode(Ssd168xRefreshMode::Full);
    let mut epd = EpdBuilder::<_, GDEY0266Z90>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Frame buffers: 5,624 bytes each, pre-filled to each plane's own background byte under
    // POLARITY — no `clear_frame` call needed, since Phase 1's draw covers the whole panel.
    let mut bw_buf = [POLARITY.bw_background_byte(); FRAME_BYTES];
    let mut red_buf = [POLARITY.accent_background_byte(); FRAME_BYTES];

    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // --- Phase 1: Full tri-color refresh. ---
    let phase1 = {
        let mut page = PageBufferPair::new(
            &mut bw_buf,
            &mut red_buf,
            GDEY0266Z90::WIDTH,
            GDEY0266Z90::HEIGHT,
            0,
            POLARITY,
        );

        draw_static_content(&mut page, &ferris_bmp, &rust_bmp, "mode: Full");
        write_full_frame(&mut epd, &page);

        timed_refresh(&mut epd, &mut timer)
    };
    defmt::info!("Phase 1 Full: {} ms", phase1);

    timer.delay_ms(2000);

    // --- Phase 2: Windowed refresh on the full waveform. Both planes must be written. ---
    for count in 1..=2u32 {
        {
            let mut band = PageBufferPair::new(
                &mut bw_buf[..BAND_BYTES],
                &mut red_buf[..BAND_BYTES],
                GDEY0266Z90::WIDTH,
                BAND_H,
                BAND_Y,
                POLARITY,
            );
            band.clear();

            draw_band(&mut band, count, "Full window");
            draw_band_bar(&mut band, count);

            write_band(&mut epd, &band);
        }

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!("Phase 2 windowed Full: {} ms", ms);
        timer.delay_ms(1000);
    }

    // --- Phase 3: FastFull, same content as Phase 1, on the temperature-override waveform. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::FastFull);

    let phase3 = {
        let mut page = PageBufferPair::new(
            &mut bw_buf,
            &mut red_buf,
            GDEY0266Z90::WIDTH,
            GDEY0266Z90::HEIGHT,
            0,
            POLARITY,
        );
        page.clear();

        draw_static_content(&mut page, &ferris_bmp, &rust_bmp, "mode: FastFull");
        write_full_frame(&mut epd, &page);

        timed_refresh(&mut epd, &mut timer)
    };
    defmt::info!("Phase 3 FastFull: {} ms", phase3);

    timer.delay_ms(2000);

    // --- Phase 4: BaseMap, then Partial. Both write both planes, exactly as Phases 1-3. There is
    // no previous-frame seeding: on a Tri-Color panel 0x26 is *always* the Red plane. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::BaseMap);

    {
        let mut band = PageBufferPair::new(
            &mut bw_buf[..BAND_BYTES],
            &mut red_buf[..BAND_BYTES],
            GDEY0266Z90::WIDTH,
            BAND_H,
            BAND_Y,
            POLARITY,
        );
        band.clear();

        draw_band(&mut band, 0, "BaseMap");
        // count == 0 draws a zero-width rectangle — a no-op, kept only for structural parity
        // with Phase 2/the Partial loop below.
        draw_band_bar(&mut band, 0);

        // No `NO_RED_BAND` constant needed: `band.accent()` is already all background bytes,
        // since nothing drew Accent content into it above.
        write_band(&mut epd, &band);
    }
    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Phase 4 BaseMap: {} ms", ms);

    timer.delay_ms(1000);

    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);

    for count in 1..=2u32 {
        {
            let mut band = PageBufferPair::new(
                &mut bw_buf[..BAND_BYTES],
                &mut red_buf[..BAND_BYTES],
                GDEY0266Z90::WIDTH,
                BAND_H,
                BAND_Y,
                POLARITY,
            );
            band.clear();

            draw_band(&mut band, count, "Partial mode");
            draw_band_bar(&mut band, count);

            write_band(&mut epd, &band);
        }

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!("Phase 4 Partial: {} ms", ms);
        timer.delay_ms(1000);
    }

    // Restore the full-frame window and default waveform, then sleep the controller.
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);
    epd.set_window(0, 0, GDEY0266Z90::WIDTH - 1, GDEY0266Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.sleep(&mut timer).unwrap();

    // Full vs FastFull is the measurement worth reading: on RP2350 this glass gave 20045 vs 16181,
    // a 19% saving. Good Display quote ~20000 vs ~19000 on their own glass, so the saving is a
    // property of the OTP waveform rather than of the host -- measure, do not assume.

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
