//! # Good Display GDEM0154Z90 1.54" Tri-Color E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery)
//! to the Adafruit Feather RP2040 ThinkInk, driving the
//! **GDEM0154Z90** 1.54" Tri-Color (Black/White/Red, 200x200) E-Paper Display using the
//! `Ssd1681Controller` from the `epdsi` library.
//!
//! Demonstrates:
//! 1. **Phase 1**: Full Tri-Color (Black/White/Red) refresh displaying header, colored accent banners,
//!    Ferris logo (Red), Rust logo (Black), and text labels.
//! 2. **Phase 2**: Partial *window* refresh loop that repaints only the bottom status band with an
//!    animated Black/Red progress bar, leaving the header and logos untouched.
//!
//! ## Note on refresh speed
//!
//! Tri-color (BWR) panels such as the GDEM0154Z90 have **no fast/differential waveform**: the red
//! pigment needs the long OTP waveform, so *every* update takes ~14 s. The SSD1681 "display mode 2"
//! trigger (`0x22 = 0xFC`) that enables sub-second updates on monochrome panels is not supported
//! here — using it just runs a slow update and, because only the Black/White RAM gets rewritten,
//! drops all red content. Phase 2 therefore keeps [`Ssd1681RefreshMode::Full`] (`0x22 = 0xF7`) and
//! narrows the RAM window instead, writing **both** the B/W and Red RAM for the updated region.
//! This mirrors GxEPD2's `GxEPD2_154_Z90c`, where `partial_refresh_time == full_refresh_time`.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEM0154Z90 1.54" Tri-Color, 200x200 (SSD1681), seated directly in the board's 24-pin FPC socket.
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
//! as soon as it completes, arriving on the host at essentially the same moment the panel finishes
//! visibly changing for that phase.
//!
//! **Watch the panel meanwhile.** A stage that completes fast *without* visibly changing the
//! display has not driven the ink, and no timing figure will tell you that.
//!
//! ## Measured output
//!
//! ```text
//! === GDEM0154Z90 1.54" Tri-Color (epdsi SSD1681, Feather RP2040) ===
//! Phase 1 Full tri-color: 17881 ms
//! Phase 2 windowed Full: 17881 ms
//! Phase 2 windowed Full: 17882 ms
//! Phase 2 windowed Full: 17881 ms
//! Phase 2 windowed Full: 17880 ms
//! Phase 2 windowed Full: 17881 ms
//! === done ===
//! ```
//!
//! Note that the windowed refreshes cost exactly as much as the full one. That is correct and is
//! the point of Phase 2: a colour panel has no differential waveform, so narrowing the RAM window
//! reduces what is redrawn, not what it costs. The 2.66" panel shows the same thing.
//!
//! **The ~14 s in those labels is unverified and probably wrong.** It comes from
//! `xiao-esp32c3-blinky/BRINGUP.md`, measured on a module later found to be faulty, and is
//! repeated in epdsi's README. This board measures 17.88 s with a spread of 2 ms across six
//! refreshes, which is a far more precise figure than the one it disagrees with. Since the 2.13"
//! and 2.66" panels both match RP2350 to within milliseconds on this host, a 27 % host-dependent
//! gap is implausible. Running this example on RP2350 would settle it; until then treat 17.9 s as
//! the measurement and ~14 s as folklore.
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example ssd1681_gdem0154z90_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1681_gdem0154z90_epd
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
    // SSD1681 BUSY is active-HIGH, so pull down: a floating line reads "idle".
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
        "GDEM0154Z90 1.54\" Tri-Color (epdsi SSD1681, Feather RP2040)",
        "Feather RP2040 GDEM0154Z90",
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

    // Instantiate epdsi SPI bus wrapper and dedicated SSD1681 controller
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1681Controller::new(GDEM0154Z90::WIDTH, GDEM0154Z90::HEIGHT);

    // Build EPD Driver using epdsi with GDEM0154Z90 panel specification (200x200)
    let mut epd = EpdBuilder::<_, GDEM0154Z90>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Clear display controller RAM
    epd.clear_frame(ColorChannel::BlackWhite, 0xFF).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0x00).unwrap();

    // Frame buffers: 200 x 200 / 8 = 5,000 bytes each
    let mut bw_buf = [0xFFu8; (GDEM0154Z90::WIDTH as usize * GDEM0154Z90::HEIGHT as usize) / 8];
    let mut red_buf = [0x00u8; (GDEM0154Z90::WIDTH as usize * GDEM0154Z90::HEIGHT as usize) / 8];

    // Load BMP images
    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    // Scoped so the full-frame borrows of `bw_buf` / `red_buf` end before Phase 2 re-borrows them
    // as smaller sub-region buffers.
    {
        let mut display_bw =
            PageBuffer::new(&mut bw_buf, GDEM0154Z90::WIDTH, GDEM0154Z90::HEIGHT, 0);
        let mut display_red =
            PageBuffer::new(&mut red_buf, GDEM0154Z90::WIDTH, GDEM0154Z90::HEIGHT, 0);

        // Outer border (Black)
        Rectangle::new(
            Point::new(0, 0),
            Size::new(GDEM0154Z90::WIDTH, GDEM0154Z90::HEIGHT),
        )
        .into_styled(style)
        .draw(&mut display_bw)
        .unwrap();

        // Header text (Black)
        Text::new("GDEM0154Z90 1.54\"", Point::new(10, 18), text_style)
            .draw(&mut display_bw)
            .unwrap();

        // Separator line (Black)
        Line::new(Point::new(10, 25), Point::new(190, 25))
            .into_styled(style)
            .draw(&mut display_bw)
            .unwrap();

        // Subtitle: "Tri-Color " in Black, "BWR" in Red
        Text::new("Tri-Color ", Point::new(10, 42), text_style)
            .draw(&mut display_bw)
            .unwrap();
        Text::new("BWR", Point::new(110, 42), text_style)
            .draw(&mut display_red)
            .unwrap();

        // Bounding box for color swatches
        Rectangle::new(Point::new(10, 50), Size::new(180, 16))
            .into_styled(style)
            .draw(&mut display_bw)
            .unwrap();

        // Black swatch banner inside bounding box (on Black/White frame)
        Rectangle::new(Point::new(12, 52), Size::new(84, 12))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(&mut display_bw)
            .unwrap();

        // Red swatch banner inside bounding box (on Red frame, BinaryColor::Off sets bit=1 in 0x00-base buffer)
        Rectangle::new(Point::new(104, 52), Size::new(84, 12))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(&mut display_red)
            .unwrap();

        // Draw Ferris logo in Red (left side: x=20, y=75)
        let ferris_pos = Point::new(20, 75);
        for pixel in ferris_bmp.pixels() {
            if pixel.1 == BinaryColor::Off {
                Pixel(pixel.0 + ferris_pos, BinaryColor::Off)
                    .draw(&mut display_red)
                    .unwrap();
            }
        }

        // Draw Rust logo in Black (right side: x=115, y=75)
        let rust_pos = Point::new(115, 75);
        for pixel in rust_bmp.pixels() {
            if pixel.1 == BinaryColor::On {
                Pixel(pixel.0 + rust_pos, BinaryColor::On)
                    .draw(&mut display_bw)
                    .unwrap();
            }
        }

        // Draw text labels (Black)
        Text::new("Feather RP2040", Point::new(10, 165), text_style)
            .draw(&mut display_bw)
            .unwrap();

        Text::new("epdsi SSD1681", Point::new(10, 185), text_style)
            .draw(&mut display_bw)
            .unwrap();

        // Each RAM write starts from the window origin, so reset window + cursor before both channels.
        epd.set_window(0, 0, GDEM0154Z90::WIDTH - 1, GDEM0154Z90::HEIGHT - 1)
            .unwrap();
        epd.set_cursor(0, 0).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, display_bw.as_slice())
            .unwrap();

        epd.set_window(0, 0, GDEM0154Z90::WIDTH - 1, GDEM0154Z90::HEIGHT - 1)
            .unwrap();
        epd.set_cursor(0, 0).unwrap();
        epd.write_frame(ColorChannel::RedYellow, display_red.as_slice())
            .unwrap();

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!("Phase 1 Full tri-color: {} ms", ms);
    }

    timer.delay_ms(2000);

    // The refresh mode deliberately stays `Full` (0x22 = 0xF7). Trigger 0xFC selects the SSD1681
    // built-in fast LUT, which only exists for monochrome panels — on a BWR panel it is slow *and*
    // discards red. What makes this phase "partial" is the narrowed RAM window below.
    debug_assert_eq!(epd.controller().refresh_mode(), Ssd1681RefreshMode::Full);

    // Bottom status band, updated in place. The Rust logo ends at y = 139 (75 + 64), so the band
    // starts at y = 140 and the header/logos painted in Phase 1 are never touched.
    const BAND_Y: u32 = 140;
    const BAND_H: u32 = 60;
    const BAND_BYTES: usize = (GDEM0154Z90::WIDTH as usize * BAND_H as usize) / 8;

    for count in 1..=5u32 {
        // Sub-region buffers: 200 x 60 / 8 = 1,500 bytes of the full-frame arrays.
        let mut band_bw = PageBuffer::new(
            &mut bw_buf[..BAND_BYTES],
            GDEM0154Z90::WIDTH,
            BAND_H,
            BAND_Y,
        );
        let mut band_red = PageBuffer::new(
            &mut red_buf[..BAND_BYTES],
            GDEM0154Z90::WIDTH,
            BAND_H,
            BAND_Y,
        );

        band_bw.clear_byte(0xFF);
        band_red.clear_byte(0x00);

        // Update counter label (Black)
        let mut count_buf = [0u8; 32];
        let count_str =
            format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap();
        Text::new(count_str, Point::new(10, 157), text_style)
            .draw(&mut band_bw)
            .unwrap();

        // Progress bar outline (Black)
        Rectangle::new(Point::new(10, 164), Size::new(180, 14))
            .into_styled(style)
            .draw(&mut band_bw)
            .unwrap();

        // Progress bar fill (Red) — proves the Red channel survives a partial window update
        Rectangle::new(Point::new(12, 166), Size::new(count * 35, 10))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(&mut band_red)
            .unwrap();

        Text::new("Partial window", Point::new(10, 195), text_style)
            .draw(&mut band_bw)
            .unwrap();

        // Restrict controller RAM to the band, then write BOTH channels for that region. Writing
        // only Black/White would leave stale Red RAM behind for the band.
        epd.set_window(0, BAND_Y, GDEM0154Z90::WIDTH - 1, BAND_Y + BAND_H - 1)
            .unwrap();
        epd.set_cursor(0, BAND_Y).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, band_bw.as_slice())
            .unwrap();

        epd.set_window(0, BAND_Y, GDEM0154Z90::WIDTH - 1, BAND_Y + BAND_H - 1)
            .unwrap();
        epd.set_cursor(0, BAND_Y).unwrap();
        epd.write_frame(ColorChannel::RedYellow, band_red.as_slice())
            .unwrap();

        let ms = timed_refresh(&mut epd, &mut timer);
        defmt::info!("Phase 2 windowed Full: {} ms", ms);

        timer.delay_ms(1000);
    }

    // Restore the full-frame RAM window for any subsequent updates.
    epd.set_window(0, 0, GDEM0154Z90::WIDTH - 1, GDEM0154Z90::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
