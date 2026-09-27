//! # Good Display GDEQ0426T82 4.26" 4-Level Grayscale (Gray4) E-Paper Example (`epdsi`)
//!
//! Companion to `ssd1677_gdeq0426t82_epd` (plain 1-bit monochrome) — same board, panel and
//! wiring, but drives the panel's **4-level grayscale** mode instead: White/Light/Dark/Black
//! instead of just White/Black.
//!
//! ## Provenance — read before trusting this on hardware
//!
//! `GDEQ0426T82::GRAY4` is **not** Good Display/Seeed material — Good Display's own spec lists this
//! as a 2-level (monochrome) panel. It is transcribed verbatim from Adafruit_EPD's
//! `ThinkInk_426_Grayscale4_GDEQ` reference driver (`ti_426_gray4_init_code` /
//! `ti_426_gray4_lut_code`), the only Gray4 reference for this controller/panel pairing.
//! **Confirmed on physical hardware** (RP2350 blocking and async, and ESP32-C3) rendering four
//! distinct gray levels correctly — this RP2040 port is the last of the four boards.
//!
//! Unlike single-pass SSD1680 Gray4 panels (see this repo's `ssd1680_gdey0266t90_gray4_epd`),
//! `Adafruit_SSD1677::update()`'s grayscale branch is **two-pass**: a full refresh with the OTP LUT
//! (Red/Yellow plane bypassed) sets a known monochrome baseline, then the custom LUT and voltage
//! registers are reloaded, then a second refresh with the real Black/White (LSB) and Red/Yellow
//! (MSB) planes. See `epdsi`'s `Ssd1677RefreshMode::Gray4Preclear`/`Gray4` docs for the full
//! sequence — this example drives it directly with `EpdDriver` primitives (`write_frame`/`refresh`/
//! `reload_gray4_lut`), the same way the confirmed RP2350/ESP32-C3 examples do.
//!
//! ## Note on orientation
//!
//! Same portrait convention as `ssd1677_gdeq0426t82_epd`: [`DisplayRotation::Rotate270`] turns the
//! panel's native 800x480 landscape RAM into a 480x800 portrait drawing surface with the FPC
//! ribbon at the bottom.
//!
//! ## Note on buffer size
//!
//! Two 48,000-byte bit-plane buffers (96,000 bytes total). Held as `static mut` rather than stack
//! arrays: the mono sibling's single 48,000-byte stack array already notes "rather less headroom"
//! on this board's 264 KB SRAM than the RP2350 this crate was first written against, and doubling
//! that on the stack is not worth the risk.
//!
//! ## Hardware
//!
//! Same board, panel and wiring as `ssd1677_gdeq0426t82_epd` — see that example for the full pin
//! table. Repeated here for convenience:
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEQ0426T82 4.26" (800x480, SSD1677, Seeed 6398), driven in Gray4
//!   mode, seated directly in the board's 24-pin FPC socket.
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
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report).
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
//! cargo run --release --example ssd1677_gdeq0426t82_gray4_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1677_gdeq0426t82_gray4_epd
//! ```
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
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;

/// Frame buffer size per bit-plane: 100 bytes per RAM row x 480 rows = 48,000 bytes.
const FRAME_BYTES: usize = GDEQ0426T82::WIDTH.div_ceil(8) as usize * GDEQ0426T82::HEIGHT as usize;

/// Visible width in the rotated portrait frame (the panel's 480 px axis).
const VIEW_W: u32 = GDEQ0426T82::HEIGHT;

/// Visible height in the rotated portrait frame (the panel's 800 px axis).
const VIEW_H: u32 = GDEQ0426T82::WIDTH;

/// This panel's two RAM planes are both inverted — see `Gray4Polarity::ADAFRUIT_SSD1677`'s doc
/// for the derivation from `ThinkInk_426_Grayscale4_GDEQ.h`.
const POLARITY: Gray4Polarity = Gray4Polarity::ADAFRUIT_SSD1677;

/// Bit-plane A (Black/White, LSB). `static mut` rather than a stack local — see the module doc's
/// "Note on buffer size".
static mut PLANE_A: [u8; FRAME_BYTES] = [0u8; FRAME_BYTES];

/// Bit-plane B (Red/Yellow, MSB).
static mut PLANE_B: [u8; FRAME_BYTES] = [0u8; FRAME_BYTES];

/// Screen 1: title/subtitle banner over a 4-band Black/Dark/Light/White swatch.
fn draw_banner(page: &mut GrayBufferPair) {
    let title_style = MonoTextStyle::new(&FONT_10X20, Gray4Color::Black);
    let dark_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Dark);
    let light_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Light);
    let black_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let white_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::White);

    // "SSD1677 Gray4" is 13 chars at 10px = 130px, centered in the 480px width.
    Text::new("SSD1677 Gray4", Point::new(175, 40), title_style)
        .draw(page)
        .unwrap();
    // "GDEQ0426T82 800x480" is 20 chars at 6px = 120px.
    Text::new("GDEQ0426T82 800x480", Point::new(180, 62), dark_style)
        .draw(page)
        .unwrap();
    // "Feather RP2040 Gray4" is 21 chars at 6px = 126px.
    Text::new("Feather RP2040 Gray4", Point::new(177, 84), light_style)
        .draw(page)
        .unwrap();

    // Four equal bands spanning the full 480px width, one per gray level.
    const BAR_Y: i32 = 140;
    const BAR_H: u32 = 100;
    const BAR_W: u32 = VIEW_W / 4;

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
        Text::new(label, Point::new(x + 8, BAR_Y + 56), label_style)
            .draw(page)
            .unwrap();
    }
    // Outline around the White band so its edge is visible against the page background.
    Rectangle::new(Point::new(3 * BAR_W as i32, BAR_Y), Size::new(BAR_W, BAR_H))
        .into_styled(PrimitiveStyle::with_stroke(Gray4Color::Black, 1))
        .draw(page)
        .unwrap();
}

/// Screen 2: four concentric rounded rectangles alternating gray levels, with a centered label.
fn draw_geometric(page: &mut GrayBufferPair) {
    let stroke = PrimitiveStyle::with_stroke(Gray4Color::Black, 1);
    Rectangle::new(Point::new(0, 0), Size::new(VIEW_W, VIEW_H))
        .into_styled(stroke)
        .draw(page)
        .unwrap();

    // (padding, corner radius, fill) for each nested ring, outer to inner.
    let rings: [(u32, u32, Gray4Color); 4] = [
        (20, 24, Gray4Color::Light),
        (60, 18, Gray4Color::Dark),
        (100, 12, Gray4Color::Black),
        (140, 12, Gray4Color::White),
    ];
    for (pad, radius, fill) in rings {
        let rect = Rectangle::new(
            Point::new(pad as i32, pad as i32),
            Size::new(VIEW_W - 2 * pad, VIEW_H - 2 * pad),
        );
        RoundedRectangle::with_equal_corners(rect, Size::new(radius, radius))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(page)
            .unwrap();
    }

    // Centered "4-Level Gray" label inside the innermost White ring.
    let inner_pad = 140i32;
    let inner_width = VIEW_W as i32 - 2 * inner_pad;
    let label_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let label = "4-Level Gray";
    let label_width = label.len() as i32 * 6;
    Text::new(
        label,
        Point::new(
            inner_pad + (inner_width - label_width) / 2,
            VIEW_H as i32 / 2,
        ),
        label_style,
    )
    .draw(page)
    .unwrap();
}

/// Preclear pass: writes `data` to *both* the Black/White and Red/Yellow channels, so the
/// baseline OTP-LUT refresh (`Ssd1677RefreshMode::Gray4Preclear`) matches the final image's
/// black/white split.
fn write_preclear<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, data: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, data).unwrap();

    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, data).unwrap();
}

/// Final pass: writes the real Black/White (LSB) and Red/Yellow (MSB) planes.
fn write_real_planes<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, plane_a: &[u8], plane_b: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, plane_a).unwrap();

    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, plane_b).unwrap();
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
    // SSD1677 BUSY is active-HIGH, so pull down: a floating line reads "idle".
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
        "GDEQ0426T82 4.26\" Gray4 (epdsi SSD1677, Feather RP2040)",
        "Feather RP2040 GDEQ0426T82 Gray4",
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

    // `for_panel` picks up GDEQ0426T82's dimensions; `.with_gray4` layers on the Adafruit_EPD-
    // sourced register bundle. Start on `Gray4Preclear` — the first pass of every screen below.
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1677Controller::for_panel::<GDEQ0426T82>()
        .with_gray4(GDEQ0426T82::GRAY4)
        .with_refresh_mode(Ssd1677RefreshMode::Gray4Preclear);

    // Build EPD Driver using epdsi with GDEQ0426T82 panel specification (800x480)
    let mut epd = EpdBuilder::<_, GDEQ0426T82>::new(controller).build(epd_bus);

    defmt::info!("Initializing SSD1677 epdsi EPD driver (Gray4)...");
    epd.init(&mut timer).unwrap();

    // SAFETY: single-threaded core0 access, and these are the only references taken to the
    // buffers this side of the frame.
    let plane_a: &'static mut [u8; FRAME_BYTES] = unsafe { &mut *core::ptr::addr_of_mut!(PLANE_A) };
    let plane_b: &'static mut [u8; FRAME_BYTES] = unsafe { &mut *core::ptr::addr_of_mut!(PLANE_B) };

    defmt::info!("--- Screen 1: Banner & 4-Level Swatch ---");
    {
        let mut page = GrayBufferPair::new(
            plane_a,
            plane_b,
            GDEQ0426T82::WIDTH,
            GDEQ0426T82::HEIGHT,
            0,
            POLARITY,
        );
        page.set_rotation(DisplayRotation::Rotate270);
        page.clear();
        draw_banner(&mut page);

        defmt::info!("Preclear pass (mono baseline, expect several seconds)...");
        write_preclear(&mut epd, page.plane_a().as_slice());
        epd.refresh(&mut timer).unwrap();

        // The preclear refresh's OTP LUT load overwrote the custom LUT/voltage registers
        // uploaded during init — reload them before the real Gray4 refresh.
        {
            let (bus, controller) = epd.split_mut();
            controller.reload_gray4_lut(bus).unwrap();
        }
        epd.controller_mut()
            .set_refresh_mode(Ssd1677RefreshMode::Gray4);

        defmt::info!("Gray4 pass (real image, expect several seconds)...");
        write_real_planes(
            &mut epd,
            page.plane_a().as_slice(),
            page.plane_b().as_slice(),
        );
        epd.refresh(&mut timer).unwrap();
    }

    timer.delay_ms(8000);

    defmt::info!("--- Screen 2: Concentric Geometric Grayscale Test Pattern ---");
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Gray4Preclear);
    {
        // SAFETY: single-threaded core0 access, and these are the only references taken to the
        // buffers this side of the frame.
        let plane_a: &'static mut [u8; FRAME_BYTES] =
            unsafe { &mut *core::ptr::addr_of_mut!(PLANE_A) };
        let plane_b: &'static mut [u8; FRAME_BYTES] =
            unsafe { &mut *core::ptr::addr_of_mut!(PLANE_B) };

        let mut page = GrayBufferPair::new(
            plane_a,
            plane_b,
            GDEQ0426T82::WIDTH,
            GDEQ0426T82::HEIGHT,
            0,
            POLARITY,
        );
        page.set_rotation(DisplayRotation::Rotate270);
        page.clear();
        draw_geometric(&mut page);

        defmt::info!("Preclear pass (mono baseline, expect several seconds)...");
        write_preclear(&mut epd, page.plane_a().as_slice());
        epd.refresh(&mut timer).unwrap();

        {
            let (bus, controller) = epd.split_mut();
            controller.reload_gray4_lut(bus).unwrap();
        }
        epd.controller_mut()
            .set_refresh_mode(Ssd1677RefreshMode::Gray4);

        defmt::info!("Gray4 pass (real image, expect several seconds)...");
        write_real_planes(
            &mut epd,
            page.plane_a().as_slice(),
            page.plane_b().as_slice(),
        );
        epd.refresh(&mut timer).unwrap();
    }

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
