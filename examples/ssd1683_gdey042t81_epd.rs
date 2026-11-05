//! # Good Display GDEY042T81 4.2" Monochrome E-Paper Example (`epdsi`)
//!
//! Example for the Adafruit Feather RP2040 ThinkInk driving the **GDEY042T81** 4.2" Monochrome
//! (Black/White, 400x300) E-Paper Display through `epdsi`'s `Ssd168xVariant::Ssd1683`.
//!
//! The panel is an **SSD1683**, a variant of the shared SSD168x controller, and nothing infers the
//! variant from the panel type: pairing `GDEY042T81` with another variant does not error, it
//! renders wrongly. This is not the `GDEY042T81-FL02` front-light variant.
//!
//! **Hardware-verified on the Feather RP2040 ThinkInk** (5 Nov 2026): Full 1585 ms, Partial 411 ms,
//! FastFull 1667 ms, matching the Pico 2 and the XIAO ESP32-C3, so `FastFull` is slower than `Full`
//! on this panel. Needs `epdsi` 0.10.1 or later for the variant. Register sequences follow Good Display's `AU-GDEY042T81-FP4G-20240801`
//! demo and the SSD1683 datasheet; see the `GDEY042T81` panel doc in `epdsi` for the departures.
//!
//! Demonstrates:
//! 1. **Phase 1**: Full monochrome refresh, then the image is copied into the secondary RAM
//!    (`0x26`) so Phase 2 has a correct base to diff against.
//! 2. **Phase 2**: Fast *differential* partial-window refresh loop, swapping the Ferris and Rust
//!    logos on every pass. The vendor partial byte is `0xFF`.
//! 3. **Phase 3**: Full-waveform cleanup pass over the content band.
//! 4. **Phase 4**: `FastFull` (`1A 6E`, then `0xC7`) on a full-frame redraw.
//!
//! For 4-level grayscale on the same panel see `ssd1683_gdey042t81_gray4_epd`. No SWD connector is
//! fitted on this board, so logging goes over USB CDC via `defmt-bbq`, as in the other examples.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEY042T81 4.2" e-Paper (400x300 Monochrome,
//!   24-pin FPC), seated directly in the board's 24-pin FPC socket.
//!
//! Connections are fixed by the socket — SCK GP22, MOSI GP23, CS GP19, DC GP18, RST GP17,
//! BUSY GP16. These are SPI0 on the RP2040, even though the Arduino core calls the port SPI1.
//!
//! **Swap panels with the board unpowered.**
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example ssd1683_gdey042t81_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1683_gdey042t81_epd
//! ```
//!
//! USB comes up within about a second of boot — core1 services it independently of the panel, so
//! the `until` loop above returns almost immediately instead of waiting for the run to finish.
//!
//! `cat` does not exit on its own — Ctrl-C once the output has printed.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` right after flashing just means enumeration hasn't
//! finished yet** — it should clear within a second or two. If it doesn't clear quickly, confirm
//! the board was actually in bootloader mode before flashing.
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

/// Row stride in bytes. 400 px is byte-aligned, so this is exactly 50 with no padding.
const STRIDE: usize = GDEY042T81::WIDTH.div_ceil(8) as usize;

/// Full frame buffer size: 50 x 300 = 15,000 bytes.
const FRAME_BYTES: usize = STRIDE * GDEY042T81::HEIGHT as usize;

/// Top Y coordinate of the content band repainted in Phases 2 and 3 — everything from just below
/// the title/subtitle separator down to the bottom of the panel, so the logo swap, footer labels
/// and status line are all inside the partial-refresh window. Only the border, title and subtitle
/// above it are painted once in Phase 1 and never touched again.
const BAND_Y: u32 = 52;

/// Height of the content band in pixels (y = 52..299).
const BAND_H: u32 = GDEY042T81::HEIGHT - BAND_Y;

/// Content band buffer size: 50 x 248 = 12,400 bytes.
const BAND_BYTES: usize = STRIDE * BAND_H as usize;

/// X coordinate of the left logo slot.
const LOGO_X_LEFT: i32 = 60;

/// X coordinate of the right logo slot.
const LOGO_X_RIGHT: i32 = 276;

/// Ferris's own Y offset (64x42 — shorter than Rust, so it sits a little lower to bottom-align).
const FERRIS_Y: i32 = 92;

/// Rust's own Y offset (64x64).
const RUST_Y: i32 = 82;

/// All-zero fill for the content band's secondary RAM, used for the Phase 3 cleanup pass. Good
/// Display's demo writes `0x00` to `0x26` before every full update, so the full waveform runs
/// against an all-black "previous image". Lives in flash rather than on the stack.
static ZERO_BAND: [u8; BAND_BYTES] = [0x00u8; BAND_BYTES];

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

/// Draws Ferris and Rust side by side (400 px fits the 64 px-wide logos with room to spare). `swapped` exchanges which logo occupies the
/// left slot: Phase 2 flips it on every partial update, the same idiom `ssd1680_gdem0213b74_epd`
/// uses for its stacked top/bottom swap.
fn draw_logos(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    swapped: bool,
) {
    let (ferris_x, rust_x) = if swapped {
        (LOGO_X_RIGHT, LOGO_X_LEFT)
    } else {
        (LOGO_X_LEFT, LOGO_X_RIGHT)
    };

    let ferris_pos = Point::new(ferris_x, FERRIS_Y);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

    let rust_pos = Point::new(rust_x, RUST_Y);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }
}

/// Draws the footer labels, mode line and the separator above them — identical every time it is
/// called, so Phase 2 can redraw it unchanged inside the content band alongside the swapped logos.
fn draw_footer(display: &mut PageBuffer, mode_label: &str) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    Text::new("Feather RP2040", Point::new(8, 170), small_text_style)
        .draw(display)
        .unwrap();
    Text::new("epdsi SSD1683", Point::new(8, 184), small_text_style)
        .draw(display)
        .unwrap();
    Text::new(mode_label, Point::new(8, 198), small_text_style)
        .draw(display)
        .unwrap();

    // Separator above the status line that Phase 2's counter/bar sits below.
    Line::new(Point::new(8, 210), Point::new(391, 210))
        .into_styled(stroke)
        .draw(display)
        .unwrap();
}

/// Draws the Phase 1 static content: border, title, subtitle, logos (never swapped here — only
/// Phase 2 swaps them) and footer.
fn draw_static_content(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    mode_label: &str,
) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    // Outer border, so a shifted or wrapped raster is obvious.
    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEY042T81::WIDTH, GDEY042T81::HEIGHT),
    )
    .into_styled(stroke)
    .draw(display)
    .unwrap();

    Text::new("GDEY042T81", Point::new(8, 22), text_style)
        .draw(display)
        .unwrap();

    Text::new("4.2\" Mono", Point::new(8, 40), small_text_style)
        .draw(display)
        .unwrap();

    Line::new(Point::new(8, 48), Point::new(391, 48))
        .into_styled(stroke)
        .draw(display)
        .unwrap();

    draw_logos(display, ferris_bmp, rust_bmp, false);
    draw_footer(display, mode_label);
}

/// Writes the full frame to Black/White RAM and zeros to the secondary RAM (`0x26`), the way Good
/// Display's `EPD_WhiteScreen_ALL` does before every full update.
fn write_full_frame<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, data: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEY042T81::WIDTH - 1, GDEY042T81::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, data).unwrap();

    epd.set_window(0, 0, GDEY042T81::WIDTH - 1, GDEY042T81::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0x00).unwrap();
}

/// Copies the frame now on glass into the secondary RAM (`0x26`), the last step of Good Display's
/// `EPD_SetRAMValue_BaseMap`, so Phase 2's differential updates diff against the right base.
fn seed_previous_frame<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, data: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEY042T81::WIDTH - 1, GDEY042T81::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, data).unwrap();
}

/// Draws the status line: label, counter and progress bar. Fixed at an absolute Y position —
/// deliberately independent of [`BAND_Y`], which is just the partial-refresh window's top edge,
/// not where content starts. This sits well inside that window, below the logos and footer.
fn draw_band(band: &mut PageBuffer, count: u32, label: &str) {
    const STATUS_Y: i32 = 220;

    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    Text::new(label, Point::new(8, STATUS_Y + 14), small_text_style)
        .draw(band)
        .unwrap();

    let mut count_buf = [0u8; 32];
    let count_str = format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap();
    Text::new(count_str, Point::new(8, STATUS_Y + 28), small_text_style)
        .draw(band)
        .unwrap();

    Rectangle::new(Point::new(8, STATUS_Y + 38), Size::new(384, 16))
        .into_styled(stroke)
        .draw(band)
        .unwrap();

    // Capped at 380: the outline above is 384px wide starting at x=8, the fill starts 2px in at
    // x=10, so 380 lands the fill's right edge 2px inside the outline's, symmetric with the left
    // inset.
    Rectangle::new(
        Point::new(10, STATUS_Y + 40),
        Size::new((count * 77).min(380), 12),
    )
    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
    .draw(band)
    .unwrap();
}

/// Redraws the outer border's left, right and bottom edges for this band's row range.
///
/// Phase 1 draws the full-panel border once, but the content band's `clear_byte` + full redraw
/// on every Phase 2 pass (and the Phase 3 cleanup resend) wipes out whatever of that border falls
/// within the band — everything except the sliver above [`BAND_Y`], which is never touched. Without
/// this, the border only ever appears around the title and looks disconnected from the rest of the
/// content below it.
fn draw_band_border(band: &mut PageBuffer) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let bottom = GDEY042T81::HEIGHT as i32 - 1;
    let right = GDEY042T81::WIDTH as i32 - 1;

    Line::new(Point::new(0, BAND_Y as i32), Point::new(0, bottom))
        .into_styled(stroke)
        .draw(band)
        .unwrap();
    Line::new(Point::new(right, BAND_Y as i32), Point::new(right, bottom))
        .into_styled(stroke)
        .draw(band)
        .unwrap();
    Line::new(Point::new(0, bottom), Point::new(right, bottom))
        .into_styled(stroke)
        .draw(band)
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
    // SSD1683 BUSY is active-HIGH, so pull down: a floating line reads "idle".
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
        "GDEY042T81 4.2\" Mono (epdsi SSD1683, Feather RP2040)",
        "Feather RP2040 GDEY042T81",
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

    // Instantiate epdsi SPI bus wrapper and the SSD168x controller. The SSD1683 is a variant of the
    // shared SSD168x driver, and nothing infers it from the panel type: pairing GDEY042T81 with
    // the SSD1680 or SSD1681 variant does not error, it renders wrongly.
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd168xController::for_panel::<GDEY042T81>(Ssd168xVariant::Ssd1683)
        .with_refresh_mode(Ssd168xRefreshMode::Full);
    let mut epd = EpdBuilder::<_, GDEY042T81>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Black/White RAM starts white and the secondary RAM (0x26) starts at zero, as in Good
    // Display's `EPD_WhiteScreen_White`. On this monochrome panel 0x26 is the "previous image"
    // buffer used by differential updates, not a color plane.
    epd.clear_frame(ColorChannel::BlackWhite, 0xFF).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0x00).unwrap();

    let mut bw_buf = [0xFFu8; FRAME_BYTES];

    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // --- Phase 1: Full monochrome refresh. ---
    // Scoped so the full-frame borrow of `bw_buf` ends before Phase 2 re-borrows it.
    {
        let mut display = PageBuffer::new(&mut bw_buf, GDEY042T81::WIDTH, GDEY042T81::HEIGHT, 0);
        draw_static_content(&mut display, &ferris_bmp, &rust_bmp, "mode: Full");

        write_full_frame(&mut epd, display.as_slice());
    }

    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Phase 1 Full: {} ms", ms);

    // Prime the secondary RAM (0x26) with what is now on glass, as Good Display's
    // `EPD_SetRAMValue_BaseMap` does last, so Phase 2 diffs against the right base.
    seed_previous_frame(&mut epd, &bw_buf[..]);

    timer.delay_ms(2000);

    // --- Phase 2: Fast differential partial-window refresh loop (logo swap). ---
    // Select the SSD1683 built-in fast LUT (0x22 = 0xFF). Unlike the Tri-Color GDEY0266Z90, this
    // is a genuine differential update on this monochrome panel and should complete in well under
    // a second.
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);

    for count in 1..=5u32 {
        // Flip the logo order on every pass — same idiom `ssd1680_gdem0213b74_epd` and
        // `uc8253_gdey037t03_epd` use, just left/right instead of top/bottom.
        let swapped = count % 2 == 1;

        {
            let mut band =
                PageBuffer::new(&mut bw_buf[..BAND_BYTES], GDEY042T81::WIDTH, BAND_H, BAND_Y);
            band.clear_byte(0xFF);
            draw_band_border(&mut band);
            draw_logos(&mut band, &ferris_bmp, &rust_bmp, swapped);
            draw_footer(&mut band, "mode: Full");
            draw_band(&mut band, count, "Fast partial");
        }

        // Restrict controller RAM to the band, write the new image to Black/White RAM.
        epd.set_window(0, BAND_Y, GDEY042T81::WIDTH - 1, BAND_Y + BAND_H - 1)
            .unwrap();
        epd.set_cursor(0, BAND_Y).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, &bw_buf[..BAND_BYTES])
            .unwrap();

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!(
            "Phase 2 partial #{}: {} ms (logos {})",
            count,
            ms,
            if swapped { "swapped" } else { "normal" }
        );

        // Copy the band we just displayed into the "previous image" RAM so the next iteration
        // diffs against what is actually on the panel.
        epd.set_window(0, BAND_Y, GDEY042T81::WIDTH - 1, BAND_Y + BAND_H - 1)
            .unwrap();
        epd.set_cursor(0, BAND_Y).unwrap();
        epd.write_frame(ColorChannel::RedYellow, &bw_buf[..BAND_BYTES])
            .unwrap();

        timer.delay_ms(500);
    }

    // --- Phase 3: Full-waveform cleanup pass. ---
    // Differential updates drive the pixels with a shorter waveform than the OTP full-refresh
    // LUT, so ink density can drift after several Phase 2 passes. Re-running the final band
    // content through the full waveform restores even density. Blanking the secondary RAM
    // first stops it being read as a stale differential base afterwards.
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);

    epd.set_window(0, BAND_Y, GDEY042T81::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::RedYellow, &ZERO_BAND)
        .unwrap();

    // `bw_buf` still holds the last band drawn in Phase 2 — nothing has touched it since, so
    // re-sending it unchanged is safe.
    epd.set_window(0, BAND_Y, GDEY042T81::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &bw_buf[..BAND_BYTES])
        .unwrap();

    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Phase 3 cleanup: {} ms", ms);

    timer.delay_ms(2000);

    // --- Phase 4: FastFull on a full-frame redraw. ---
    // Phase 4 runs last and redraws the WHOLE frame. `bw_buf` holds band-relative content from
    // Phase 3, and `write_full_frame` sends a full-frame window, so reusing it as-is would paint a
    // shifted duplicate (the failure an earlier SSD1680 example hit). The vendor's fast path also
    // zeroes `0x26` before the update, which `write_full_frame` already does.
    {
        let mut display = PageBuffer::new(&mut bw_buf, GDEY042T81::WIDTH, GDEY042T81::HEIGHT, 0);
        display.clear_byte(0xFF);
        draw_static_content(&mut display, &ferris_bmp, &rust_bmp, "mode: FastFull");
        write_full_frame(&mut epd, display.as_slice());
    }
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::FastFull);
    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Phase 4 FastFull: {} ms", ms);

    // Restore the full-frame RAM window and the default waveform for any subsequent updates.
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);
    epd.set_window(0, 0, GDEY042T81::WIDTH - 1, GDEY042T81::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();

    // Deep sleep. init() must be called again before any further frame.
    epd.sleep(&mut timer).unwrap();
    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
