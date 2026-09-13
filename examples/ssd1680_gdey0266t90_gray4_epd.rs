//! # GDEY0266T90 4-Level Grayscale (Gray4) E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery) to the
//! Adafruit Feather RP2040 ThinkInk. Everything from `POLARITY` down to the end of `draw_geometric`
//! is unchanged from that version; only `main`, the reporting and the board bring-up differ.
//!
//! Companion to `ssd1680_gdey0266t90_epd` (plain 1-bit monochrome) — same board, panel and
//! wiring, but drives the panel's **4-level grayscale** mode instead: White/Light/Dark/Black
//! instead of just White/Black.
//!
//! Ported from the Seeed XIAO MG24 Arduino sketch at
//! `XIAO_MG24/Adafruit_EPD/XIAO_Waveshare_2in66/XIAO_Waveshare_2in66.ino`, which drives this same
//! Waveshare 2.66" panel via Adafruit_EPD's `ThinkInk_266_Grayscale4_MFGN` class. Two static
//! screens, same content as that sketch:
//!
//! 1. **Screen 1**: title/subtitle banner over a 4-band swatch (Black/Dark/Light/White).
//! 2. **Screen 2**: four concentric rounded rectangles alternating gray levels, with a centered
//!    "4-Level Gray" label — left on-panel when the example finishes, same as the sketch.
//!
//! Unlike `ssd1680_gdey0266t90_epd`, there is no partial/differential refresh phase here: Gray4
//! mode has exactly one trigger ([`Ssd168xRefreshMode::Gray4`]), matching Adafruit_EPD's own
//! `Adafruit_SSD1680::update()`, which likewise has no partial-mode counterpart for this mode.
//!
//! ## Provenance — read before trusting this on hardware
//!
//! `GDEY0266T90::GRAY4` is **not** Good Display/Waveshare material — Waveshare's own spec lists 2
//! grayscale levels, and the GxEPD2 reference driver never writes a grayscale LUT. It is
//! transcribed verbatim from Adafruit_EPD's `ti_266mfgn_gray4_init_code`/`ti_266mfgn_gray4_lut_code`
//! (confirmed rendering four distinct gray levels on a XIAO MG24 running that sketch), but **has
//! not yet been verified through `epdsi`'s own from-scratch, init-once port of those registers on
//! physical hardware** — see `epdsi`'s `Gray4Registers` and `GDEY0266T90` panel docs for the full
//! caveat, and see `ssd1680_gdey0266t90_epd`'s own module docs for the RP2350 timing this panel's
//! plain monochrome mode measured. This example is exactly that verification: run it, and see what
//! actually lights up.
//!
//! Also note: this example draws in the panel's native (unrotated) orientation, matching
//! `ssd1680_gdey0266t90_epd`/`ssd1680_gdey0266z90_epd` — it does **not** replicate the Arduino
//! sketch's `setRotation(2)`, which corrects for Adafruit_EPD's own default origin convention, not
//! anything `epdsi` shares. Plain (non-rounded) corner radii aside, the rounded rectangles below
//! use `embedded-graphics`'s `RoundedRectangle::with_equal_corners`, the direct equivalent of the
//! sketch's `fillRoundRect`.
//!
//! ## Results arrive live, one phase at a time
//!
//! There is no debug probe on this board, so logging goes over USB CDC via `defmt-bbq` rather than
//! RTT. USB CDC needs `usb_dev.poll()` every few milliseconds and `epd.refresh()` blocks for
//! seconds at a time, so USB is serviced on core1 while core0 runs the phases — see
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report). Each screen's timing is logged
//! as soon as it completes.
//!
//! ## Hardware
//!
//! Same board, panel and wiring as `ssd1680_gdey0266t90_epd` — see that example for the full
//! details. Repeated here for convenience:
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEY0266T90 / Waveshare 2.66" e-Paper (SKU 18401, FPC-7510 REV.C),
//!   152x296 Monochrome, seated directly in the board's 24-pin FPC socket.
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
//! cargo run --release --example ssd1680_gdey0266t90_gray4_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1680_gdey0266t90_gray4_epd
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
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;

/// Row stride in bytes. 152 px is byte-aligned, so this is exactly 19 with no padding.
const STRIDE: usize = GDEY0266T90::WIDTH.div_ceil(8) as usize;

/// Full frame buffer size per plane: 19 x 296 = 5,624 bytes.
const FRAME_BYTES: usize = STRIDE * GDEY0266T90::HEIGHT as usize;

/// Adafruit's SSD1680 Gray4 convention: neither RAM plane inverted, a set bit is the code bit
/// directly. The only convention `epdsi` has evidence for so far.
const POLARITY: Gray4Polarity = Gray4Polarity::ADAFRUIT_SSD1680;

/// Writes both bit-planes for the full frame, resetting the RAM window and cursor first.
fn write_full_frame<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, page: &GrayBufferPair)
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEY0266T90::WIDTH - 1, GDEY0266T90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, page.plane_a().as_slice())
        .unwrap();

    epd.set_window(0, 0, GDEY0266T90::WIDTH - 1, GDEY0266T90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, page.plane_b().as_slice())
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

/// Screen 1: title/subtitle banner over a 4-band Black/Dark/Light/White swatch, the same content
/// as the Arduino sketch's first `display.display()` call.
fn draw_banner(page: &mut GrayBufferPair) {
    let title_style = MonoTextStyle::new(&FONT_10X20, Gray4Color::Black);
    let dark_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Dark);
    let light_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Light);
    let black_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let white_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::White);

    // "SSD1680 Gray4" is 13 chars at 10px = 130px, centered in the 152px width.
    Text::new("SSD1680 Gray4", Point::new(11, 24), title_style)
        .draw(page)
        .unwrap();

    // "GDEY0266T90 152x296" is 19 chars at 6px = 114px.
    Text::new("GDEY0266T90 152x296", Point::new(19, 44), dark_style)
        .draw(page)
        .unwrap();

    // "epdsi Gray4 demo" is 16 chars at 6px = 96px.
    Text::new("epdsi Gray4 demo", Point::new(28, 58), light_style)
        .draw(page)
        .unwrap();

    // Four equal 38px-wide bands spanning the full 152px width, one per gray level.
    const BAR_Y: i32 = 100;
    const BAR_H: u32 = 40;
    const BAR_W: u32 = GDEY0266T90::WIDTH / 4;

    let bands = [
        (0u32, Gray4Color::Black, "Black", white_small_style),
        (1u32, Gray4Color::Dark, "Dark", white_small_style),
        (2u32, Gray4Color::Light, "Light", black_small_style),
        (3u32, Gray4Color::White, "White", black_small_style),
    ];
    for (index, fill, label, label_style) in bands {
        let x = (index * BAR_W) as i32;
        Rectangle::new(Point::new(x, BAR_Y), Size::new(BAR_W, BAR_H))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(page)
            .unwrap();
        Text::new(label, Point::new(x + 4, BAR_Y + 24), label_style)
            .draw(page)
            .unwrap();
    }
    // Outline around the White band so its edge is visible against the page background.
    Rectangle::new(Point::new(3 * BAR_W as i32, BAR_Y), Size::new(BAR_W, BAR_H))
        .into_styled(PrimitiveStyle::with_stroke(Gray4Color::Black, 1))
        .draw(page)
        .unwrap();
}

/// Screen 2: four concentric rounded rectangles alternating gray levels, with a centered
/// "4-Level Gray" label — the same pattern as the Arduino sketch's second screen.
fn draw_geometric(page: &mut GrayBufferPair) {
    let stroke = PrimitiveStyle::with_stroke(Gray4Color::Black, 1);
    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEY0266T90::WIDTH, GDEY0266T90::HEIGHT),
    )
    .into_styled(stroke)
    .draw(page)
    .unwrap();

    // (padding, corner radius, fill) for each nested ring, outer to inner — matches the sketch's
    // `pad += 10` progression and radius sequence (8, 6, 4, 4).
    let rings: [(u32, u32, Gray4Color); 4] = [
        (8, 8, Gray4Color::Light),
        (18, 6, Gray4Color::Dark),
        (28, 4, Gray4Color::Black),
        (38, 4, Gray4Color::White),
    ];
    for (pad, radius, fill) in rings {
        let rect = Rectangle::new(
            Point::new(pad as i32, pad as i32),
            Size::new(GDEY0266T90::WIDTH - 2 * pad, GDEY0266T90::HEIGHT - 2 * pad),
        );
        RoundedRectangle::with_equal_corners(rect, Size::new(radius, radius))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(page)
            .unwrap();
    }

    // The innermost White ring (last entry above, pad=38) is the only safe place to put black
    // text without it running into a Dark/Black ring and vanishing — and at only
    // `WIDTH - 2*38 = 76`px wide, it's narrower than it looks relative to the panel's full width.
    // `FONT_10X20` at 120px for this string overflowed that by 44px each way, which is exactly
    // what showed up on hardware as unreadable text bleeding into the rings on both sides.
    // `FONT_6X10` at 72px fits inside it with a 2px margin on each side.
    let inner_pad = 38i32;
    let inner_width = GDEY0266T90::WIDTH as i32 - 2 * inner_pad;
    let label_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let label = "4-Level Gray";
    let label_width = label.len() as i32 * 6;
    Text::new(
        label,
        Point::new(
            inner_pad + (inner_width - label_width) / 2,
            GDEY0266T90::HEIGHT as i32 / 2,
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
        "GDEY0266T90 Gray4 (epdsi SSD1680, Feather RP2040)",
        "Feather RP2040 GDEY0266T90 Gray4",
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

    // `for_panel` picks up `GDEY0266T90`'s dimensions; `.with_gray4` layers on the Adafruit_EPD-
    // sourced register bundle, and `Ssd168xRefreshMode::Gray4` selects its one refresh trigger.
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1680Controller::for_panel::<GDEY0266T90>()
        .with_gray4(GDEY0266T90::GRAY4)
        .with_refresh_mode(Ssd168xRefreshMode::Gray4);
    let mut epd = EpdBuilder::<_, GDEY0266T90>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    let mut plane_a = [0u8; FRAME_BYTES];
    let mut plane_b = [0u8; FRAME_BYTES];

    // --- Screen 1: Banner & 4-Level Swatch. ---
    {
        let mut page = GrayBufferPair::new(
            &mut plane_a,
            &mut plane_b,
            GDEY0266T90::WIDTH,
            GDEY0266T90::HEIGHT,
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
            GDEY0266T90::WIDTH,
            GDEY0266T90::HEIGHT,
            0,
            POLARITY,
        );
        page.clear();
        draw_geometric(&mut page);
        write_full_frame(&mut epd, &page);
    }
    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Screen 2 geometric: {} ms", ms);

    // Deep sleep. init() must be called again before any further frame. Panel is left on the
    // geometric test pattern, matching the Arduino sketch's own final state.
    epd.sleep(&mut timer).unwrap();
    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
