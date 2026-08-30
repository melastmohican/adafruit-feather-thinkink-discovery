//! # GDEY0266Z90 2.66" Tri-Color E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery) to the
//! Adafruit Feather RP2040 ThinkInk. Everything from `STRIDE` down to the end of `draw_band_bar`
//! is unchanged from that version; only `main`, the reporting and the board bring-up differ.
//!
//! Demonstrates every refresh mode the SSD1680 exposes for this panel:
//!
//! 1. **Phase 1**: Full tri-color refresh ([`Ssd168xRefreshMode::Full`]) — header, Black and Red
//!    swatches, Ferris logo (Red), Rust logo (Black), and text labels.
//! 2. **Phase 2**: Windowed refresh loop on the full waveform, repainting only the bottom status
//!    band, leaving the logos untouched.
//! 3. **Phase 3**: [`Ssd168xRefreshMode::FastFull`], timed against Phase 1.
//! 4. **Phase 4**: [`Ssd168xRefreshMode::BaseMap`] and [`Ssd168xRefreshMode::Partial`], the two
//!    modes ported from Good Display's reference driver, shown at their real cost.
//!
//! ## Why this board is the interesting one
//!
//! The Arduino sketches this panel was first brought up with — `GDEY0266Z90.ino`,
//! `Waveshare_2in66br` and the GxEPD2 `Demo.ino` — were all written for **this** Feather. So it is
//! the one place where GxEPD2 and `epdsi` can be run against identical hardware with the same
//! panel, and any difference is unambiguously a driver difference rather than a host or carrier
//! one. It also has no carrier board and no jumper wiring: the 24-pin FPC socket is on the PCB.
//!
//! ## Results arrive at the end, not live
//!
//! There is no debug probe on this board, so logging goes over USB CDC via `defmt-bbq` rather than
//! RTT. USB CDC needs `usb_dev.poll()` every few milliseconds and `epd.refresh()` blocks for ~20 s
//! at a time, so the phases run silently into an array of timings and USB comes up afterwards to
//! report. The serial device appears roughly two minutes after boot with everything in it.
//!
//! **Watch the panel meanwhile** — it is the better instrument. A stage that finishes fast without
//! visibly changing the image has not driven the ink.
//!
//! ## Reference timings
//!
//! Measured on RP2350 with this glass. A large deviation here would be an RP2040 finding.
//!
//! | Mode | RP2350 |
//! |---|---:|
//! | `Full` | 20045 ms |
//! | `Full`, windowed | 20049 ms |
//! | `FastFull` | **16181 ms** |
//! | `BaseMap` | 19909 ms |
//! | `Partial` | 19908 ms |
//!
//! ## Note on ink polarity
//!
//! The two RAM planes disagree. `0xFF` is white in the Black/White plane (`0x24`), but the Red
//! plane (`0x26`) is **inverted**: `0x00` is no red and a *set* bit is red. So `red_buf` starts at
//! `0x00` and red content is drawn as [`BinaryColor::Off`], which is what sets a bit in
//! [`PageBuffer`].
//!
//! `0x26` is *always* the Red plane on a colour panel, never the previous-frame buffer it is on a
//! mono SSD1680. Seeding it with a Black/White image — correct in `epd_diag_partial` and in the
//! monochrome examples in the sibling repos — sets nearly every bit and renders the region solid
//! red. That mistake cost a debugging round on the RP2350; do not reintroduce it here.
//!
//! ## Note on duty cycle
//!
//! Waveshare recommend at least 180 s between refreshes on this panel, and one update every 24 h
//! to avoid burn-in. This example runs seven refreshes seconds apart, which is fine as a one-off
//! but **should not be looped**, and is not a model for production pacing.
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
//! ## Run
//!
//! Hold BOOT, press RESET, release BOOT so the `RPI-RP2` volume appears, then:
//!
//! ```bash
//! cargo run --release --example ssd1680_gdey0266z90_epd
//! ```
//!
//! The panel then works for **about two minutes** with no USB at all — seven refreshes at ~20 s
//! each, see "Results arrive at the end" above. Only after that does the serial device enumerate.
//! This waits for it and decodes:
//!
//! ```bash
//! for _ in $(seq 180); do ls /dev/cu.usbmodemEPD* >/dev/null 2>&1 && break; sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1680_gdey0266z90_epd
//! ```
//!
//! `cat` does not exit on its own — Ctrl-C once the output has printed.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` means the device has not enumerated yet**, not
//! that anything failed: the panel is still mid-run. Two minutes is a long time to wait if you
//! are expecting it to be seconds. Wait and retry, or use the loop above.
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
use bsp::hal::usb::UsbBus;
use bsp::hal::{spi, Clock, Sio, Timer, Watchdog};
use bsp::{entry, pac, XOSC_CRYSTAL_FREQ};

// defmt-bbq is the global logger here, not defmt-rtt. Only one may be linked.
use defmt_bbq as _;
use panic_probe as _;

use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

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
/// painted in Phase 1 and never touched again, so the red logo stays put.
const BAND_Y: u32 = 220;

/// Height of the status band in pixels (y = 220..295).
const BAND_H: u32 = 76;

/// Status band buffer size: 19 x 76 = 1,444 bytes.
const BAND_BYTES: usize = STRIDE * BAND_H as usize;

/// Empty fill for the Red plane over the status band, clearing the red the Phase 2 progress bar
/// left there so the base-map pass lands on white.
///
/// Note the value: this is `0x00`, **not** the `0xFF` that means white in the Black/White plane.
/// The Red plane is inverted — a set bit is red — so `0xFF` here would paint the whole band solid
/// red. The monochrome examples use `0xFF` for their equivalent buffer because on those panels
/// `0x26` is a previous-frame buffer sharing the Black/White polarity. Do not copy that across.
static NO_RED_BAND: [u8; BAND_BYTES] = [0x00u8; BAND_BYTES];

/// Refreshes the panel and returns the elapsed milliseconds. Nothing is logged: USB is not up
/// during the phases, and `defmt-bbq` discards buffered data while the device is unconfigured.
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
/// `bw` receives Black content in the ordinary `BinaryColor::On` convention; `red` receives Red
/// content as `BinaryColor::Off`, which sets a bit in the `0x00`-based Red plane.
fn draw_static_content(
    bw: &mut PageBuffer,
    red: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    mode_label: &str,
) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    // Outer border (Black), so a shifted or wrapped raster is obvious.
    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT),
    )
    .into_styled(stroke)
    .draw(bw)
    .unwrap();

    // Header (Black). 11 chars at 10 px each fits the 152 px width.
    Text::new("GDEY0266Z90", Point::new(8, 22), text_style)
        .draw(bw)
        .unwrap();

    // Subtitle: "Tri-Color " in Black, "BWR" in Red.
    Text::new("Tri-Color ", Point::new(8, 40), small_text_style)
        .draw(bw)
        .unwrap();
    Text::new(
        "BWR",
        Point::new(68, 40),
        MonoTextStyle::new(&FONT_6X10, BinaryColor::Off),
    )
    .draw(red)
    .unwrap();

    Line::new(Point::new(8, 48), Point::new(143, 48))
        .into_styled(stroke)
        .draw(bw)
        .unwrap();

    // Colour swatches: Black left, Red right, inside a shared outline.
    Rectangle::new(Point::new(8, 56), Size::new(136, 18))
        .into_styled(stroke)
        .draw(bw)
        .unwrap();
    Rectangle::new(Point::new(10, 58), Size::new(64, 14))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(bw)
        .unwrap();
    Rectangle::new(Point::new(78, 58), Size::new(64, 14))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(red)
        .unwrap();

    // Ferris (64x42) in Red and Rust (64x64) in Black, side by side — 128 px of artwork fits the
    // 152 px width, unlike the 122 px monochrome panel where they have to be stacked.
    let ferris_pos = Point::new(10, 92);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_pos, BinaryColor::Off)
                .draw(red)
                .unwrap();
        }
    }

    let rust_pos = Point::new(78, 82);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_pos, BinaryColor::On).draw(bw).unwrap();
        }
    }

    // Labels (Black). Board name differs from the RP2350 original; everything else matches.
    Text::new("Feather RP2040", Point::new(8, 170), small_text_style)
        .draw(bw)
        .unwrap();
    Text::new("epdsi SSD1680", Point::new(8, 184), small_text_style)
        .draw(bw)
        .unwrap();
    Text::new(mode_label, Point::new(8, 198), small_text_style)
        .draw(bw)
        .unwrap();

    // Separator above the status band that Phases 2 and 4 repaint.
    Line::new(Point::new(8, 210), Point::new(143, 210))
        .into_styled(stroke)
        .draw(bw)
        .unwrap();
}

/// Writes both colour planes for the full frame, resetting the RAM window and cursor first.
///
/// Each RAM write restarts from the window origin, so the window and cursor have to be re-armed
/// before every plane rather than once per frame.
fn write_full_frame<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, bw: &[u8], red: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, 0, GDEY0266Z90::WIDTH - 1, GDEY0266Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, bw).unwrap();

    epd.set_window(0, 0, GDEY0266Z90::WIDTH - 1, GDEY0266Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, red).unwrap();
}

/// Writes both colour planes for the status band window.
fn write_band<BUS, C, P>(epd: &mut EpdDriver<BUS, C, P>, bw: &[u8], red: &[u8])
where
    C: EpdController<BUS>,
    C::Error: core::fmt::Debug,
    P: EpdPanel,
{
    epd.set_window(0, BAND_Y, GDEY0266Z90::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, bw).unwrap();

    epd.set_window(0, BAND_Y, GDEY0266Z90::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::RedYellow, red).unwrap();
}

/// Draws the Black/White half of the status band: label, counter and progress bar outline.
///
/// The bar *fill* is left to the caller, because which plane it belongs in differs by phase.
fn draw_band(band: &mut PageBuffer, count: u32, label: &str) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let small_text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    Text::new(label, Point::new(8, BAND_Y as i32 + 14), small_text_style)
        .draw(band)
        .unwrap();

    let mut count_buf = [0u8; 32];
    let count_str = format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap();
    Text::new(
        count_str,
        Point::new(8, BAND_Y as i32 + 28),
        small_text_style,
    )
    .draw(band)
    .unwrap();

    // Progress bar outline always lands on the Black/White plane.
    Rectangle::new(Point::new(8, BAND_Y as i32 + 38), Size::new(136, 16))
        .into_styled(stroke)
        .draw(band)
        .unwrap();
}

/// Draws the progress bar fill for `count` into `plane`.
///
/// `color` carries the plane's convention: `BinaryColor::Off` sets a bit, which is red in the
/// `0x00`-based Red plane; `BinaryColor::On` clears one, which is black in the `0xFF`-based
/// Black/White plane. Passing the wrong one yields an invisible bar, or a bar-shaped hole.
fn draw_band_bar(plane: &mut PageBuffer, count: u32, color: BinaryColor) {
    Rectangle::new(
        Point::new(10, BAND_Y as i32 + 40),
        Size::new(count * 33, 12),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(plane)
    .unwrap();
}

#[entry]
fn main() -> ! {
    let mut bbq = defmt_bbq::init().unwrap();

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
    // SSD1680 BUSY is active-HIGH, so pull down: a floating line reads "idle".
    let busy = pins.gpio16.into_pull_down_input();

    let spi = spi::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        4_000_000u32.Hz(),
        embedded_hal::spi::MODE_0,
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

    // The asymmetric pair: 0xFF is white in the Black/White plane, but the Red plane is inverted,
    // so 0x00 is *no* red.
    epd.clear_frame(ColorChannel::BlackWhite, 0xFF).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0x00).unwrap();

    let mut bw_buf = [0xFFu8; FRAME_BYTES];
    let mut red_buf = [0x00u8; FRAME_BYTES];

    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // Timings are collected here and reported once USB is up: full, window1, window2, fastfull,
    // basemap, partial1, partial2.
    let mut ms = [0u64; 7];

    // --- Phase 1: Full tri-color refresh. ---
    ms[0] = {
        let mut bw = PageBuffer::new(&mut bw_buf, GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT, 0);
        let mut red = PageBuffer::new(&mut red_buf, GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT, 0);

        draw_static_content(&mut bw, &mut red, &ferris_bmp, &rust_bmp, "mode: Full");
        write_full_frame(&mut epd, bw.as_slice(), red.as_slice());

        timed_refresh(&mut epd, &mut timer)
    };

    timer.delay_ms(2000);

    // --- Phase 2: Windowed refresh on the full waveform. Both planes must be written. ---
    for count in 1..=2u32 {
        {
            let mut band = PageBuffer::new(
                &mut bw_buf[..BAND_BYTES],
                GDEY0266Z90::WIDTH,
                BAND_H,
                BAND_Y,
            );
            band.clear_byte(0xFF);
            let mut band_red = PageBuffer::new(
                &mut red_buf[..BAND_BYTES],
                GDEY0266Z90::WIDTH,
                BAND_H,
                BAND_Y,
            );
            band_red.clear_byte(0x00);

            draw_band(&mut band, count, "Full window");
            draw_band_bar(&mut band_red, count, BinaryColor::Off);
        }

        write_band(&mut epd, &bw_buf[..BAND_BYTES], &red_buf[..BAND_BYTES]);
        ms[count as usize] = timed_refresh(&mut epd, &mut timer);
        timer.delay_ms(1000);
    }

    // --- Phase 3: FastFull, same content as Phase 1, on the temperature-override waveform. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::FastFull);

    ms[3] = {
        let mut bw = PageBuffer::new(&mut bw_buf, GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT, 0);
        bw.clear_byte(0xFF);
        let mut red = PageBuffer::new(&mut red_buf, GDEY0266Z90::WIDTH, GDEY0266Z90::HEIGHT, 0);
        red.clear_byte(0x00);

        draw_static_content(&mut bw, &mut red, &ferris_bmp, &rust_bmp, "mode: FastFull");
        write_full_frame(&mut epd, bw.as_slice(), red.as_slice());

        timed_refresh(&mut epd, &mut timer)
    };

    timer.delay_ms(2000);

    // --- Phase 4: BaseMap, then Partial. Both write both planes, exactly as Phases 1-3. There is
    // no previous-frame seeding: on a Tri-Color panel 0x26 is *always* the Red plane. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::BaseMap);

    {
        let mut band = PageBuffer::new(
            &mut bw_buf[..BAND_BYTES],
            GDEY0266Z90::WIDTH,
            BAND_H,
            BAND_Y,
        );
        band.clear_byte(0xFF);
        let mut band_red = PageBuffer::new(
            &mut red_buf[..BAND_BYTES],
            GDEY0266Z90::WIDTH,
            BAND_H,
            BAND_Y,
        );
        band_red.clear_byte(0x00);

        draw_band(&mut band, 0, "BaseMap");
        draw_band_bar(&mut band_red, 0, BinaryColor::Off);
    }

    // NO_RED_BAND rather than the drawn red band: 0x00 is *no* red.
    write_band(&mut epd, &bw_buf[..BAND_BYTES], &NO_RED_BAND);
    ms[4] = timed_refresh(&mut epd, &mut timer);

    timer.delay_ms(1000);

    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);

    for count in 1..=2u32 {
        {
            let mut band = PageBuffer::new(
                &mut bw_buf[..BAND_BYTES],
                GDEY0266Z90::WIDTH,
                BAND_H,
                BAND_Y,
            );
            band.clear_byte(0xFF);
            let mut band_red = PageBuffer::new(
                &mut red_buf[..BAND_BYTES],
                GDEY0266Z90::WIDTH,
                BAND_H,
                BAND_Y,
            );
            band_red.clear_byte(0x00);

            draw_band(&mut band, count, "Partial mode");
            draw_band_bar(&mut band_red, count, BinaryColor::Off);
        }

        write_band(&mut epd, &bw_buf[..BAND_BYTES], &red_buf[..BAND_BYTES]);
        ms[4 + count as usize] = timed_refresh(&mut epd, &mut timer);
        timer.delay_ms(1000);
    }

    // Restore the full-frame window and default waveform, then sleep the controller.
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);
    epd.set_window(0, 0, GDEY0266Z90::WIDTH - 1, GDEY0266Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.sleep(&mut timer).unwrap();

    // ---------------------------------------------------------------------------------------
    // Panel work is done. Bring USB up and report.
    // ---------------------------------------------------------------------------------------

    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    let mut serial = SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Adafruit")
            .product("Feather RP2040 GDEY0266Z90")
            // One shared serial number across every example in this repo, deliberately: macOS
            // builds the device node from it, so `/dev/cu.usbmodemEPD*` is the same command
            // whatever is flashed and there is no per-example lookup table to remember. Which
            // firmware is running is carried by the product string above instead, readable with
            //   ioreg -r -c IOUSBHostDevice -l | grep -o '"USB Product Name" = "Feather[^"]*"'
            // and stated again in the first decoded log line.
            .serial_number("EPD")])
        .unwrap()
        .device_class(2) // CDC
        .build();

    let mut reported = false;

    loop {
        watchdog.feed();

        if usb_dev.poll(&mut [&mut serial]) {
            let mut rx = [0u8; 64];
            let _ = serial.read(&mut rx);
        }

        if !reported && usb_dev.state() == UsbDeviceState::Configured {
            defmt::info!("=== GDEY0266Z90 2.66\" Tri-Color (epdsi SSD1680, Feather RP2040) ===");
            defmt::info!("  Phase 1 Full:            {} ms  (RP2350 20045)", ms[0]);
            defmt::info!("  Phase 2 windowed Full 1: {} ms  (RP2350 20049)", ms[1]);
            defmt::info!("  Phase 2 windowed Full 2: {} ms  (RP2350 20051)", ms[2]);
            defmt::info!("  Phase 3 FastFull:        {} ms  (RP2350 16181)", ms[3]);
            defmt::info!("  Phase 4 BaseMap:         {} ms  (RP2350 19909)", ms[4]);
            defmt::info!("  Phase 4 Partial 1:       {} ms  (RP2350 19908)", ms[5]);
            defmt::info!("  Phase 4 Partial 2:       {} ms  (RP2350 19908)", ms[6]);
            defmt::info!(
                "Full {} ms vs FastFull {} ms. On RP2350 this glass gave 20045 vs 16181, ~19% \
                 faster; Good Display quote ~20000 vs ~19000 on their own. The saving is the OTP \
                 waveform, so it varies by glass rather than by host.",
                ms[0],
                ms[3]
            );
            defmt::info!("=== done, controller asleep ===");
            reported = true;
        }

        while let Ok(grant) = bbq.read() {
            if usb_dev.state() == UsbDeviceState::Configured {
                if let Ok(written) = serial.write(&grant) {
                    grant.release(written);
                } else {
                    break;
                }
            } else {
                let len = grant.len();
                grant.release(len);
            }
        }
    }
}
