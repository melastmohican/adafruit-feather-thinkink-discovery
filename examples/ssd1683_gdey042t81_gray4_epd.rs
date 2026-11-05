//! # Good Display GDEY042T81 4.2" 4-Level Grayscale (Gray4) E-Paper Example (`epdsi`)
//!
//! Companion to `ssd1683_gdey042t81_epd` (plain 1-bit monochrome): same board, panel and wiring,
//! but it drives the panel's **4-level grayscale** mode through `Ssd168xRefreshMode::Gray4Otp`.
//!
//! The sequence comes from Good Display's `AU-GDEY042T81-FP4G-20240801` demo (`EPD_HW_Init_4G`,
//! `EPD_Update_4G`): the built-in waveform with `0x21 = 00 00`, `1A 5A`, `22 91`, then `22 CF`, no
//! LUT upload. Hardware-verified on the Pico 2, the XIAO ESP32-C3 and the Feather RP2040 ThinkInk
//! (5 Nov 2026): both screens render correctly.
//!
//! ## Hardware
//!
//! Same board, panel and wiring as `ssd1677_gdeq0426t82_epd` — see that example for the full pin
//! table. Repeated here for convenience:
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEY042T81 4.2" (400x300), driven in Gray4 mode, seated directly
//!   in the board's 24-pin FPC socket.
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
//! cargo run --release --example ssd1683_gdey042t81_gray4_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1683_gdey042t81_gray4_epd
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

/// Frame buffer size per bit-plane: 50 bytes per RAM row x 300 rows = 15,000 bytes.
const FRAME_BYTES: usize = GDEY042T81::WIDTH.div_ceil(8) as usize * GDEY042T81::HEIGHT as usize;

/// Drawing width. The panel is drawn in its native 400x300 landscape orientation.
const VIEW_W: u32 = GDEY042T81::WIDTH;

/// Drawing height.
const VIEW_H: u32 = GDEY042T81::HEIGHT;

/// Both RAM planes are inverted on this panel: a raw white pixel is `1` on both `0x24` and `0x26`,
/// which is how Good Display's `R24_DTM1`/`R26_DTM2` split a 2 bits-per-pixel image. That is the
/// same polarity as `Gray4Polarity::ADAFRUIT_SSD1677`, confirmed on hardware.
const POLARITY: Gray4Polarity = Gray4Polarity::ADAFRUIT_SSD1677;

/// Bit-plane A (Black/White). `static mut` rather than a stack local: see the module doc.
static mut PLANE_A: [u8; FRAME_BYTES] = [0u8; FRAME_BYTES];

/// Bit-plane B (Red/Yellow).
static mut PLANE_B: [u8; FRAME_BYTES] = [0u8; FRAME_BYTES];

/// Screen 1: title/subtitle banner over a 4-band Black/Dark/Light/White swatch.
fn draw_banner(page: &mut GrayBufferPair) {
    let title_style = MonoTextStyle::new(&FONT_10X20, Gray4Color::Black);
    let dark_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Dark);
    let light_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Light);
    let black_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::Black);
    let white_small_style = MonoTextStyle::new(&FONT_6X10, Gray4Color::White);

    // "SSD1683 Gray4" is 13 chars at 10px = 130px, centered in the 400px width.
    Text::new("SSD1683 Gray4", Point::new(135, 40), title_style)
        .draw(page)
        .unwrap();
    // "GDEY042T81 400x300" is 18 chars at 6px = 108px.
    Text::new("GDEY042T81 400x300", Point::new(146, 62), dark_style)
        .draw(page)
        .unwrap();
    // "epdsi Gray4 demo" is 16 chars at 6px = 96px.
    Text::new("epdsi Gray4 demo", Point::new(152, 84), light_style)
        .draw(page)
        .unwrap();

    // Four equal bands spanning the full 400px width, one per gray level.
    const BAR_Y: i32 = 120;
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
        (12, 16, Gray4Color::Light),
        (36, 12, Gray4Color::Dark),
        (60, 12, Gray4Color::Black),
        (84, 8, Gray4Color::White),
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
    let inner_pad = 84i32;
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

/// Writes the Black/White (`0x24`) and Red/Yellow (`0x26`) planes, each carrying one gray bit.
fn write_planes<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, plane_a: &[u8], plane_b: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEY042T81::WIDTH - 1, GDEY042T81::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, plane_a).unwrap();

    epd.set_window(0, 0, GDEY042T81::WIDTH - 1, GDEY042T81::HEIGHT - 1)
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
        "GDEY042T81 4.2\" Gray4 (epdsi SSD1683, Feather RP2040)",
        "Feather RP2040 GDEY042T81 Gray4",
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

    // Instantiate epdsi's SPI bus wrapper and the SSD168x controller. The SSD1683 is a variant of
    // the shared SSD168x driver, and nothing infers it from the panel type. Gray4 here is
    // `Gray4Otp`: the controller's built-in waveform, so there is no LUT bundle to pass.
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd168xController::for_panel::<GDEY042T81>(Ssd168xVariant::Ssd1683)
        .with_refresh_mode(Ssd168xRefreshMode::Gray4Otp);
    let mut epd = EpdBuilder::<_, GDEY042T81>::new(controller).build(epd_bus);

    defmt::info!("Initializing SSD1683 epdsi EPD driver (Gray4)...");
    epd.init(&mut timer).unwrap();

    defmt::info!("--- Screen 1: Banner & 4-Level Swatch ---");
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
            GDEY042T81::WIDTH,
            GDEY042T81::HEIGHT,
            0,
            POLARITY,
        );
        page.clear();
        draw_banner(&mut page);

        defmt::info!("Gray4Otp refresh (expect about 2 s)...");
        write_planes(
            &mut epd,
            page.plane_a().as_slice(),
            page.plane_b().as_slice(),
        );
        epd.refresh(&mut timer).unwrap();
    }

    timer.delay_ms(8000);

    defmt::info!("--- Screen 2: Concentric Geometric Grayscale Test Pattern ---");
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
            GDEY042T81::WIDTH,
            GDEY042T81::HEIGHT,
            0,
            POLARITY,
        );
        page.clear();
        draw_geometric(&mut page);

        defmt::info!("Gray4Otp refresh (expect about 2 s)...");
        write_planes(
            &mut epd,
            page.plane_a().as_slice(),
            page.plane_b().as_slice(),
        );
        epd.refresh(&mut timer).unwrap();
    }

    // Deep sleep. init() must be called again before any further frame.
    epd.sleep(&mut timer).unwrap();
    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
