//! # GDEM0213B74 4-Level Grayscale (Gray4) E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery) to the
//! Adafruit Feather RP2040 ThinkInk. Everything from `POLARITY` down to the end of `draw_geometric`
//! is unchanged from that version; only `main`, the reporting and the board bring-up differ.
//!
//! Companion to `ssd1680_gdem0213b74_epd` (plain 1-bit monochrome). Same board, panel and
//! wiring, but drives the panel's **4-level grayscale** mode instead: White/Light/Dark/Black
//! instead of just White/Black.
//!
//! Two static screens:
//!
//! 1. **Screen 1**: title/subtitle banner over a 4-band swatch (Black/Dark/Light/White), each
//!    band carrying a single-letter label (`B`/`D`/`L`/`W`) sized to fit this panel's narrower
//!    122px width without overflowing into its neighbor.
//! 2. **Screen 2**: three concentric rounded rectangles (Light/Dark/White) with a centered
//!    "4-Lvl Gray" label in the innermost White ring.
//!
//! Like `ssd1680_gdem0213b74_epd`, there is no partial/differential refresh phase here: Gray4
//! mode has exactly one trigger ([`Ssd168xRefreshMode::Gray4`]).
//!
//! ## Provenance and a board-specific risk to watch for
//!
//! `GDEM0213B74::GRAY4` is **not** Adafruit's own product-page default mode for this breakout.
//! It is transcribed verbatim from Adafruit_EPD's `ti_213mfgn_gray4_init_code`/
//! `ti_213mfgn_gray4_lut_code`, and is byte-identical to the already-shipped `GDEY0266T90::GRAY4`
//! bundle (confirmed by direct byte comparison, not assumed). See `epdsi`'s `GDEM0213B74` panel
//! doc for the full caveat.
//!
//! That caveat matters more on **this specific board** than elsewhere: `ssd1680_gdem0213b74_epd`'s
//! own module doc already notes this panel's ribbon is stamped `FPC-7528B`, the exact revision
//! Adafruit's own driver carries a `_colstart = 8` (not 0) offset for, which `epdsi` does not
//! implement in either mode. The plain-mono example on this same physical panel measures
//! correctly today, so if that offset mattered for mono it is not visually obvious in that demo's
//! centered, margined layout. If this Gray4 image renders shifted, that is the first thing to
//! check, not the register bundle, since this board's panel is the one most likely to actually
//! be the affected revision rather than just carrying the generic warning.
//!
//! ## Note on the 122 pixel panel width
//!
//! See `ssd1680_gdem0213b74_gray4_epd`'s own module doc in `rust-rpico2-discovery` for the full
//! layout-math rationale this port carries over unchanged: `FONT_6X10` throughout, single-letter
//! swatch labels, and three rings instead of the four `ssd1680_gdey0266t90_gray4_epd`'s wider
//! panel uses.
//!
//! ## Results arrive live, one phase at a time
//!
//! There is no debug probe on this board, so logging goes over USB CDC via `defmt-bbq` rather than
//! RTT. USB CDC needs `usb_dev.poll()` every few milliseconds and `epd.refresh()` blocks for
//! seconds at a time, so USB is serviced on core1 while core0 runs the phases. See
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report). Each screen's timing is logged
//! as soon as it completes.
//!
//! ## Hardware
//!
//! Same board, panel and wiring as `ssd1680_gdem0213b74_epd`. See that example for the full
//! details. Repeated here for convenience:
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEM0213B74 2.13" Monochrome, 122x250 (Adafruit 6383), seated
//!   directly in the board's 24-pin FPC socket. Its ribbon is stamped `FPC-7528B`.
//!
//! Connections are fixed by the socket: SCK GP22, MOSI GP23, CS GP19, DC GP18, RST GP17,
//! BUSY GP16. These are SPI0 on the RP2040, even though the Arduino core calls the port SPI1.
//!
//! **Swap panels with the board unpowered.**
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT**. The `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example ssd1680_gdem0213b74_gray4_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1680_gdem0213b74_gray4_epd
//! ```
//!
//! USB comes up within about a second of boot: core1 services it independently of the panel, so
//! the `until` loop above returns almost immediately instead of waiting for the run to finish.
//!
//! `cat` does not exit on its own. Ctrl-C once the output has printed.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` right after flashing just means enumeration hasn't
//! finished yet.** It should clear within a second or two. If it doesn't clear quickly, confirm
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
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;

/// Row stride in bytes. 122 px rounds up to 16, same alignment the plain-mono example uses.
const STRIDE: usize = GDEM0213B74::WIDTH.div_ceil(8) as usize;

/// Full frame buffer size per plane: 16 x 250 = 4,000 bytes.
const FRAME_BYTES: usize = STRIDE * GDEM0213B74::HEIGHT as usize;

/// Adafruit's SSD1680 Gray4 convention: neither RAM plane inverted, a set bit is the code bit
/// directly. The only convention `epdsi` has evidence for so far.
const POLARITY: Gray4Polarity = Gray4Polarity::ADAFRUIT_SSD1680;

/// Writes both bit-planes for the full frame, resetting the RAM window and cursor first.
fn write_full_frame<BUS, C>(epd: &mut EpdDriver<BUS, C, GDEM0213B74>, page: &GrayBufferPair)
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
{
    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, page.plane_a().as_slice())
        .unwrap();

    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, page.plane_b().as_slice())
        .unwrap();
}

/// Refreshes the panel and returns the elapsed milliseconds.
fn timed_refresh<BUS, C>(epd: &mut EpdDriver<BUS, C, GDEM0213B74>, timer: &mut Timer) -> u64
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
{
    let start = timer.get_counter().ticks();
    epd.refresh(timer).unwrap();
    (timer.get_counter().ticks() - start) / 1000
}

/// Screen 1: title/subtitle banner over a 4-band Black/Dark/Light/White swatch, each band
/// labeled with a single letter. All three text lines and all four band widths are sized to
/// this panel's 122px width explicitly (see the widths/positions computed below), not copied
/// from the wider `GDEY0266T90` layout.
fn draw_banner(page: &mut GrayBufferPair) {
    let dark_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Dark);
    let light_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Light);
    let black_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let white_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::White);

    // "SSD1680 Gray4" is 13 chars at 6px = 78px, centered in the 122px width: x = (122-78)/2 = 22.
    Text::new("SSD1680 Gray4", Point::new(22, 10), black_small_style)
        .draw(page)
        .unwrap();

    // "GDEM0213B74 122x250" is 19 chars at 6px = 114px: x = (122-114)/2 = 4.
    Text::new("GDEM0213B74 122x250", Point::new(4, 24), dark_style)
        .draw(page)
        .unwrap();

    // "epdsi Gray4 demo" is 16 chars at 6px = 96px: x = (122-96)/2 = 13.
    Text::new("epdsi Gray4 demo", Point::new(13, 38), light_style)
        .draw(page)
        .unwrap();

    // Four bands spanning the full 122px width. 122/4 = 30 remainder 2, so the last band is
    // widened to 32px rather than leaving a 2px gap: 30+30+30+32 = 122.
    const BAR_Y: i32 = 55;
    const BAR_H: u32 = 40;
    const BAR_W: u32 = GDEM0213B74::WIDTH / 4;
    const LAST_BAR_W: u32 = GDEM0213B74::WIDTH - 3 * BAR_W;

    let bands = [
        (0u32, BAR_W, Gray4Color::Black, "B", white_small_style),
        (1u32, BAR_W, Gray4Color::Dark, "D", white_small_style),
        (2u32, BAR_W, Gray4Color::Light, "L", black_small_style),
        (3u32, LAST_BAR_W, Gray4Color::White, "W", black_small_style),
    ];
    let mut x = 0i32;
    for (_index, width, fill, label, label_style) in bands {
        Rectangle::new(Point::new(x, BAR_Y), Size::new(width, BAR_H))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(page)
            .unwrap();
        // Single 6px-wide character, centered in the band: x + (width-6)/2.
        Text::new(
            label,
            Point::new(x + (width as i32 - 6) / 2, BAR_Y + BAR_H as i32 / 2 + 3),
            label_style,
        )
        .draw(page)
        .unwrap();
        x += width as i32;
    }
    // Outline around the White band so its edge is visible against the page background.
    Rectangle::new(
        Point::new(3 * BAR_W as i32, BAR_Y),
        Size::new(LAST_BAR_W, BAR_H),
    )
    .into_styled(PrimitiveStyle::with_stroke(Gray4Color::Black, 1))
    .draw(page)
    .unwrap();
}

/// Screen 2: three concentric rounded rectangles (Light/Dark/White, outer to inner) with a
/// centered "4-Lvl Gray" label in the innermost White ring. One ring fewer than
/// `ssd1680_gdey0266t90_gray4_epd`'s four: that version's 76px-wide innermost ring has room for
/// a 12-character label; this panel's narrower width does not reach that at the same padding
/// (see the inner-ring math below), so the ring count and label text are sized down instead of
/// risking the overflow that example's own module doc warns about.
fn draw_geometric(page: &mut GrayBufferPair) {
    let stroke = PrimitiveStyle::with_stroke(Gray4Color::Black, 1);
    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEM0213B74::WIDTH, GDEM0213B74::HEIGHT),
    )
    .into_styled(stroke)
    .draw(page)
    .unwrap();

    // (padding, corner radius, fill) for each nested ring, outer to inner. Innermost pad=22
    // leaves an inner rectangle 122 - 2*22 = 78px wide: enough for the 60px label below plus a
    // margin, which pad=30 or higher (as used on the wider panel) would not be.
    let rings: [(u32, u32, Gray4Color); 3] = [
        (6, 6, Gray4Color::Light),
        (14, 5, Gray4Color::Dark),
        (22, 4, Gray4Color::White),
    ];
    for (pad, radius, fill) in rings {
        let rect = Rectangle::new(
            Point::new(pad as i32, pad as i32),
            Size::new(GDEM0213B74::WIDTH - 2 * pad, GDEM0213B74::HEIGHT - 2 * pad),
        );
        RoundedRectangle::with_equal_corners(rect, Size::new(radius, radius))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(page)
            .unwrap();
    }

    // Innermost ring (pad=22) is 78px wide. "4-Lvl Gray" at FONT_6X10 is 10 chars * 6px = 60px,
    // centered: x = 22 + (78-60)/2 = 31.
    let inner_pad = 22i32;
    let inner_width = GDEM0213B74::WIDTH as i32 - 2 * inner_pad;
    let label_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let label = "4-Lvl Gray";
    let label_width = label.len() as i32 * 6;
    Text::new(
        label,
        Point::new(
            inner_pad + (inner_width - label_width) / 2,
            GDEM0213B74::HEIGHT as i32 / 2,
        ),
        label_style,
    )
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
        "GDEM0213B74 Gray4 (epdsi SSD1680, Feather RP2040)",
        "Feather RP2040 GDEM0213B74 Gray4",
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

    // `for_panel` picks up `GDEM0213B74`'s dimensions; `.with_gray4` layers on the Adafruit_EPD-
    // sourced register bundle, and `Ssd168xRefreshMode::Gray4` selects its one refresh trigger.
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1680Controller::for_panel::<GDEM0213B74>()
        .with_gray4(GDEM0213B74::GRAY4)
        .with_refresh_mode(Ssd168xRefreshMode::Gray4);
    let mut epd = EpdBuilder::<_, GDEM0213B74>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    let mut plane_a = [0u8; FRAME_BYTES];
    let mut plane_b = [0u8; FRAME_BYTES];

    // --- Screen 1: Banner & 4-Level Swatch. ---
    {
        let mut page = GrayBufferPair::new(
            &mut plane_a,
            &mut plane_b,
            GDEM0213B74::WIDTH,
            GDEM0213B74::HEIGHT,
            0,
            POLARITY,
        );
        page.clear();
        draw_banner(&mut page);
        write_full_frame(&mut epd, &page);
    }
    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Screen 1 banner: {} ms", ms);

    timer.delay_ms(8000);

    // --- Screen 2: Concentric Geometric Grayscale Test Pattern. ---
    {
        let mut page = GrayBufferPair::new(
            &mut plane_a,
            &mut plane_b,
            GDEM0213B74::WIDTH,
            GDEM0213B74::HEIGHT,
            0,
            POLARITY,
        );
        page.clear();
        draw_geometric(&mut page);
        write_full_frame(&mut epd, &page);
    }
    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Screen 2 geometric: {} ms", ms);

    // Deep sleep. init() must be called again before any further frame.
    epd.sleep(&mut timer).unwrap();
    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
