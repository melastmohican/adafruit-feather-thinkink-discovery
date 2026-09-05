//! # Good Display GDEY037T03 3.7" Monochrome E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery)
//! to the Adafruit Feather RP2040 ThinkInk, driving the
//! **GDEY037T03** 3.7" Monochrome (Black/White, 240x416) E-Paper Display using the
//! `Uc8253Controller` from the `epdsi` library.
//!
//! Demonstrates:
//! 1. **Phase 1**: Full monochrome refresh showing a header, separator, the Ferris and Rust logos
//!    side by side, and text labels.
//! 2. **Phase 2**: Partial-window refresh loop that repaints only the content band
//!    (y = 66..415), swapping the two logos on every pass and advancing a progress bar, while the
//!    header above the band is never touched.
//! 3. **Phase 3**: Full-panel, full-waveform cleanup pass restoring the ink density that the
//!    shortened partial waveform leaves behind.
//!
//! ## Note on the UC8253 command model
//!
//! The UC8253 differs from the SSD16xx family used by the other `epdsi` examples:
//!
//! - The RAM area must be set before writing image data, and the partial window is re-opened
//!   around *every* RAM write and again around the refresh
//!   (`PARTIAL_IN` -> `PARTIAL_WINDOW` -> operation -> `PARTIAL_OUT`). `set_window` records the
//!   area; `epdsi` emits the commands per operation.
//! - The two RAM banks are **old/new planes**, not colors: [`ColorChannel::BlackWhite`] maps to
//!   `WRITE_NEW_DATA` (`0x13`) and [`ColorChannel::RedYellow`] to `WRITE_OLD_DATA` (`0x10`).
//!   The old plane is primed to white before the first write.
//! - BUSY is active-**LOW** on this panel, the opposite of the SSD16xx panels, so the GPIO takes
//!   a pull-**up** rather than a pull-down.
//! - Orientation: buffer coordinates map directly to the panel, with `(0,0)` at the top-left
//!   when the **FPC ribbon is at the bottom**. No rotation or mirroring is applied.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEY037T03 3.7" Monochrome, 240x416 (UC8253, Adafruit 6395), seated directly in the board's 24-pin FPC socket.
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
//! === GDEY037T03 3.7" Mono (epdsi UC8253, Feather RP2040) ===
//! Phase 1 Full: 2615 ms
//! Phase 2 FastPartial: 410 ms   (x6)
//! Phase 3 cleanup Full: 2615 ms
//! === done ===
//! ```
//!
//! No prior figure exists for this panel anywhere, because until now it had never completed a
//! refresh under `epdsi` on any host that was being measured.
//!
//! ## This panel is recorded as not working on the XIAO ESP32-C3
//!
//! `xiao-esp32c3-blinky/BRINGUP.md` has a section titled "GDEY037T03 3.7" does not work on the
//! XIAO ESP32-C3", concluding: BUSY is driven and asserts correctly on reset, but the panel never
//! acts on SPI commands, `refresh` returns in 0 ms, sweeping the clock from 4 MHz to 100 kHz
//! changes nothing, and stock Arduino GxEPD2 fails identically. It ends "Stop changing driver code
//! for this."
//!
//! **That conclusion was about the host, not the panel.** The C3 module used for it was later
//! shown to be faulty by substitution — a XIAO MG24 runs clean on the same carrier, cable and
//! panel, and that module produces refresh timings deviating in both directions at once. This
//! board drives the panel correctly on the first attempt, as did the 3.52" that shares the UC8253
//! and was written off in the same section.
//!
//! So the driver was right the whole time, and "it is not the driver" was correct even though the
//! surrounding conclusion was not. Both UC8253 entries in `BRINGUP.md` need re-testing on
//! known-good hardware before they stand as findings.
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example uc8253_gdey037t03_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/uc8253_gdey037t03_epd
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

/// Row stride in bytes: 240 / 8 = 30. This panel is already byte-aligned.
const STRIDE: usize = GDEY037T03::WIDTH.div_ceil(8) as usize;

/// Full frame buffer size: 30 x 416 = 12,480 bytes.
const FRAME_BYTES: usize = STRIDE * GDEY037T03::HEIGHT as usize;

/// Top Y coordinate of the content band repainted in Phase 2.
const BAND_Y: u32 = 66;

/// Height of the content band in pixels (y = 66..415).
const BAND_H: u32 = 350;

/// Last row of the band.
const BAND_END: u32 = BAND_Y + BAND_H - 1;

/// Byte offset of the band's first row within the frame buffer.
const BAND_START_BYTE: usize = BAND_Y as usize * STRIDE;

/// Byte offset one past the band's last row.
const BAND_END_BYTE: usize = (BAND_END as usize + 1) * STRIDE;

/// X coordinate of the left logo slot.
const LOGO_LEFT_X: i32 = 30;

/// X coordinate of the right logo slot.
const LOGO_RIGHT_X: i32 = 146;

/// Draws the static chrome above the Phase 2 band: border, header, subtitle and separator.
fn draw_chrome(display: &mut PageBuffer) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEY037T03::WIDTH, GDEY037T03::HEIGHT),
    )
    .into_styled(style)
    .draw(display)
    .unwrap();

    Text::new("GDEY037T03", Point::new(10, 24), text_style)
        .draw(display)
        .unwrap();

    Text::new("3.7\" Mono", Point::new(10, 48), text_style)
        .draw(display)
        .unwrap();

    Line::new(Point::new(10, 58), Point::new(229, 58))
        .into_styled(style)
        .draw(display)
        .unwrap();
}

/// Draws everything inside the Phase 2 band: the two logos, the footer labels and the progress
/// indicator.
///
/// `swapped` exchanges the two logo slots — Phase 2 flips it on every pass so the partial update
/// is obvious at a glance. Ferris is 64x42 and Rust is 64x64, and their Y positions differ so
/// their bottoms line up. `count` of 0 renders the Phase 1 state instead of an update counter.
fn draw_content(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    swapped: bool,
    count: u32,
) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    let (ferris_x, rust_x) = if swapped {
        (LOGO_RIGHT_X, LOGO_LEFT_X)
    } else {
        (LOGO_LEFT_X, LOGO_RIGHT_X)
    };

    // The Ferris BMP has the opposite polarity to the Rust BMP, hence the `Off` test here.
    let ferris_pos = Point::new(ferris_x, 112);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

    let rust_pos = Point::new(rust_x, 90);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

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

    let mut count_buf = [0u8; 32];
    let label = if count == 0 {
        "Full refresh"
    } else {
        format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap()
    };
    Text::new(label, Point::new(10, 285), text_style)
        .draw(display)
        .unwrap();

    Rectangle::new(Point::new(10, 300), Size::new(220, 22))
        .into_styled(style)
        .draw(display)
        .unwrap();

    // Progress bar fill: 6 steps of 35 px stay inside the 216 px interior
    if count > 0 {
        Rectangle::new(Point::new(12, 303), Size::new(count * 35, 16))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(display)
            .unwrap();
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
        "GDEY037T03 3.7\" Mono (epdsi UC8253, Feather RP2040)",
        "Feather RP2040 GDEY037T03",
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
    let controller = Uc8253Controller::new(GDEY037T03::WIDTH, GDEY037T03::HEIGHT);
    let mut epd = EpdBuilder::<_, GDEY037T03>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Prime the old plane to white so the update has a clean base.
    epd.clear_frame(ColorChannel::RedYellow, 0xFF).unwrap();

    let mut bw_buf = [0xFFu8; FRAME_BYTES];

    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    // Buffer coordinates map straight to the panel with the FPC ribbon at the bottom: (0,0) is
    // the top-left of the visible image. No rotation or mirroring is needed.
    let mut display = PageBuffer::new(&mut bw_buf, GDEY037T03::WIDTH, GDEY037T03::HEIGHT, 0);

    draw_chrome(&mut display);
    draw_content(&mut display, &ferris_bmp, &rust_bmp, false, 0);

    epd.set_window(0, 0, GDEY037T03::WIDTH - 1, GDEY037T03::HEIGHT - 1)
        .unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();

    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Phase 1 Full: {} ms", ms);

    // Sync the old plane with what is now on the panel.
    epd.set_window(0, 0, GDEY037T03::WIDTH - 1, GDEY037T03::HEIGHT - 1)
        .unwrap();
    epd.write_frame(ColorChannel::RedYellow, display.as_slice())
        .unwrap();

    timer.delay_ms(2000);

    // GxEPD2 declares `hasFastPartialUpdate = true` for this panel, so its partial path always
    // applies the CCSET/TSSET temperature override — that is `FastPartial` here, not `Partial`.
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::FastPartial);

    for count in 1..=6u32 {
        // Flip the logo slots on every pass so the partial update is unmistakable.
        let swapped = count % 2 == 1;

        // The whole buffer is redrawn, but only the band's rows are sent, so the chrome above the
        // band is never repainted.
        display.clear_byte(0xFF);
        draw_chrome(&mut display);
        draw_content(&mut display, &ferris_bmp, &rust_bmp, swapped, count);

        epd.set_window(0, BAND_Y, GDEY037T03::WIDTH - 1, BAND_END)
            .unwrap();
        epd.write_frame(
            ColorChannel::BlackWhite,
            &display.as_slice()[BAND_START_BYTE..BAND_END_BYTE],
        )
        .unwrap();

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!("Phase 2 FastPartial: {} ms", ms);

        // Keep the old plane in step with the panel for the next differential pass.
        epd.set_window(0, BAND_Y, GDEY037T03::WIDTH - 1, BAND_END)
            .unwrap();
        epd.write_frame(
            ColorChannel::RedYellow,
            &display.as_slice()[BAND_START_BYTE..BAND_END_BYTE],
        )
        .unwrap();

        timer.delay_ms(1000);
    }

    // The partial waveform settles pixels at a dark grey rather than a deep black. Redraw the
    // whole frame and run it through the full waveform to restore even ink density.
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::Full);

    display.clear_byte(0xFF);
    draw_chrome(&mut display);
    draw_content(&mut display, &ferris_bmp, &rust_bmp, false, 6);

    epd.set_window(0, 0, GDEY037T03::WIDTH - 1, GDEY037T03::HEIGHT - 1)
        .unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();

    let ms = timed_refresh(&mut epd, &mut timer);
    defmt::info!("Phase 3 cleanup Full: {} ms", ms);

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
