//! # GDEM0213B74 2.13" Monochrome E-Paper Example (`epdsi`)
//!
//! Port of the Raspberry Pi Pico 2 example from
//! [`rust-rpico2-discovery`](https://github.com/melastmohican/rust-rpico2-discovery) to the
//! Adafruit Feather RP2040 ThinkInk. Everything from `STRIDE` down to the end of `draw_logos` is
//! unchanged from that version; only `main`, the reporting and the board bring-up differ.
//!
//! Demonstrates:
//! 1. **Phase 1**: Full monochrome refresh — border, header, Ferris and Rust logos, footer labels.
//! 2. **Phase 2**: Fast *differential* partial-window refresh loop. Only the band y = 50..249 is
//!    rewritten, with the logos swapping each pass and a progress bar advancing. The header above
//!    the band is never re-sent, so it cannot flicker.
//! 3. **Phase 3**: Full-waveform cleanup pass over the band, restoring the ink density the
//!    shortened differential waveform leaves behind.
//!
//! Unlike the tri-colour `ssd1680_gdey0266z90_epd` in this repo, the partial updates here are
//! genuinely differential and complete in about a second — the SSD1680's built-in fast LUT exists
//! for monochrome panels only.
//!
//! ## Note on the 122 pixel panel width
//!
//! The panel is 122 pixels wide, which is not a byte multiple. The SSD1680 addresses RAM in whole
//! bytes, so `set_window(0, .., 121, ..)` selects RAM bytes 0..=15 and the controller expects
//! **16 bytes per row**. [`PageBuffer`] rounds its row stride up to a whole byte, so it is
//! constructed with the real visible width and the frame buffer is sized
//! `WIDTH.div_ceil(8) * HEIGHT` (4,000 bytes) rather than `WIDTH * HEIGHT / 8`. Pixels at
//! x = 122..127 fall in the off-panel padding and are clipped.
//!
//! ## Note on the secondary RAM
//!
//! On this **monochrome** panel `0x26` is not a colour plane but the "previous image" that
//! differential updates diff against, so it shares the Black/White polarity — `0xFF` is white —
//! and this example keeps it in step after every refresh. That is the opposite of the tri-colour
//! panel, where `0x26` is always the Red plane and `0xFF` would paint the region solid red. The
//! two conventions are easy to confuse; see `ssd1680_gdey0266z90_epd` for the other side of it.
//!
//! ## Results arrive at the end, not live
//!
//! There is no debug probe on this board, so logging goes over USB CDC via `defmt-bbq` rather than
//! RTT. USB CDC needs `usb_dev.poll()` every few milliseconds and `epd.refresh()` blocks for
//! seconds at a time, so the phases run silently into an array of timings and USB comes up
//! afterwards to report. The serial device appears roughly 20 seconds after boot.
//!
//! **Watch the panel meanwhile** — the logo swap is the point of Phase 2 and is impossible to miss.
//!
//! ## Reference timings
//!
//! | Stage | RP2350 | XIAO ESP32-C3 |
//! |---|---:|---:|
//! | Full refresh | 3894 ms | ~3891 ms |
//! | Differential partial | 1018 ms | ~1017 ms |
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEM0213B74 2.13" Monochrome, 122x250 (Adafruit 6383), seated
//!   directly in the board's 24-pin FPC socket. Its ribbon is stamped `FPC-7528B`.
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
//! cargo run --release --example ssd1680_gdem0213b74_epd
//! ```
//!
//! The panel then works for **about 20 seconds** with no USB at all — see "Results arrive at the
//! end" above. Only after that does the serial device enumerate. This waits for it and decodes:
//!
//! ```bash
//! for _ in $(seq 60); do ls /dev/cu.usbmodemEPD* >/dev/null 2>&1 && break; sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1680_gdem0213b74_epd
//! ```
//!
//! `cat` does not exit on its own — Ctrl-C once the output has printed.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` means the device has not enumerated yet**, not
//! that anything failed: the panel is still mid-run. Wait and retry, or use the loop above.
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

/// Row stride in bytes. 122 px rounds up to 16 bytes; see the module docs.
const STRIDE: usize = GDEM0213B74::WIDTH.div_ceil(8) as usize;

/// Full frame buffer size: 16 x 250 = 4,000 bytes.
const FRAME_BYTES: usize = STRIDE * GDEM0213B74::HEIGHT as usize;

/// Top Y coordinate of the content band repainted in Phase 2. The header and the separator
/// above it (y = 0..49) are painted once in Phase 1 and never touched again.
const BAND_Y: u32 = 50;

/// Height of the content band in pixels (y = 50..249).
const BAND_H: u32 = 200;

/// Content band buffer size: 16 x 200 = 3,200 bytes.
const BAND_BYTES: usize = STRIDE * BAND_H as usize;

/// All-white fill for the content band, used to blank the secondary RAM before the Phase 3
/// cleanup pass. `0xFF` is correct here because on this monochrome panel `0x26` shares the
/// Black/White polarity — see the module docs.
static WHITE_BAND: [u8; BAND_BYTES] = [0xFFu8; BAND_BYTES];

/// X coordinate that horizontally centres a 64 px logo on the 122 px panel.
const LOGO_X: i32 = (GDEM0213B74::WIDTH as i32 - 64) / 2;

/// Top Y coordinate of the upper logo slot.
const LOGO_TOP_Y: i32 = 52;

/// Draws the Ferris and Rust logos stacked vertically, horizontally centred on the panel.
///
/// The two 64 px-wide logos cannot sit side by side on a 122 px panel, so they are stacked.
/// `swapped` exchanges which logo occupies the upper slot: Phase 2 flips it on every partial
/// update so the differential refresh is obvious at a glance. Ferris is 64x42 and Rust is
/// 64x64, and the offsets are chosen so both arrangements end at y = 161.
fn draw_logos(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
    swapped: bool,
) {
    let (ferris_y, rust_y) = if swapped {
        (LOGO_TOP_Y + 68, LOGO_TOP_Y)
    } else {
        (LOGO_TOP_Y, LOGO_TOP_Y + 46)
    };

    // The Ferris BMP has the opposite polarity to the Rust BMP, hence the `Off` test here.
    let ferris_pos = Point::new(LOGO_X, ferris_y);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

    let rust_pos = Point::new(LOGO_X, rust_y);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }
}

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
    let controller = Ssd1680Controller::new(GDEM0213B74::WIDTH, GDEM0213B74::HEIGHT);
    let mut epd = EpdBuilder::<_, GDEM0213B74>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    // Clear both RAM banks to white. On this monochrome panel the secondary RAM (0x26) is not a
    // colour plane but the "previous image" used by differential updates, so both take 0xFF.
    epd.clear_frame(ColorChannel::BlackWhite, 0xFF).unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0xFF).unwrap();

    let mut bw_buf = [0xFFu8; FRAME_BYTES];

    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    // The panel is only 122 px wide, so the footer labels use the smaller 6x10 font.
    let small_text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    // Timings: [0] Phase 1 full, [1..=6] Phase 2 differential passes, [7] Phase 3 cleanup.
    let mut ms = [0u64; 8];

    // --- Phase 1: Full monochrome refresh. ---
    // Scoped so the full-frame borrow of `bw_buf` ends before Phase 2 re-borrows it.
    {
        let mut display = PageBuffer::new(&mut bw_buf, GDEM0213B74::WIDTH, GDEM0213B74::HEIGHT, 0);

        // Outer border (visible area only)
        Rectangle::new(
            Point::new(0, 0),
            Size::new(GDEM0213B74::WIDTH, GDEM0213B74::HEIGHT),
        )
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

        Text::new("GDEM0213B74", Point::new(6, 18), text_style)
            .draw(&mut display)
            .unwrap();

        Text::new("2.13\" Mono", Point::new(6, 38), text_style)
            .draw(&mut display)
            .unwrap();

        Line::new(Point::new(6, 45), Point::new(115, 45))
            .into_styled(style)
            .draw(&mut display)
            .unwrap();

        // Ferris on top, Rust below. Phase 2 swaps them on every partial update.
        draw_logos(&mut display, &ferris_bmp, &rust_bmp, false);

        Text::new("Feather RP2040", Point::new(6, 180), small_text_style)
            .draw(&mut display)
            .unwrap();

        Text::new("epdsi SSD1680", Point::new(6, 195), small_text_style)
            .draw(&mut display)
            .unwrap();

        Line::new(Point::new(6, 203), Point::new(115, 203))
            .into_styled(style)
            .draw(&mut display)
            .unwrap();

        // Each RAM write starts from the window origin, so reset window + cursor first.
        epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
            .unwrap();
        epd.set_cursor(0, 0).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
            .unwrap();

        ms[0] = timed_refresh(&mut epd, &mut timer);

        // Seed the "previous image" RAM (0x26) with what is now physically on the panel, so the
        // Phase 2 differential updates have a correct base to diff against.
        epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
            .unwrap();
        epd.set_cursor(0, 0).unwrap();
        epd.write_frame(ColorChannel::RedYellow, display.as_slice())
            .unwrap();
    }

    timer.delay_ms(2000);

    // --- Phase 2: Fast differential partial-window refresh, logos swapping each pass. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);

    for count in 1..=6u32 {
        // Flip the logo order on every pass — a swap of two 64 px logos is impossible to miss,
        // where a progress bar alone is easy to overlook.
        let swapped = count % 2 == 1;

        {
            let mut band = PageBuffer::new(
                &mut bw_buf[..BAND_BYTES],
                GDEM0213B74::WIDTH,
                BAND_H,
                BAND_Y,
            );
            band.clear_byte(0xFF);

            draw_logos(&mut band, &ferris_bmp, &rust_bmp, swapped);

            // Footer labels and separator, redrawn identically every pass. Differential mode sees
            // no change here, so they stay rock steady while the logos above them swap.
            Text::new("Feather RP2040", Point::new(6, 180), small_text_style)
                .draw(&mut band)
                .unwrap();

            Text::new("epdsi SSD1680", Point::new(6, 195), small_text_style)
                .draw(&mut band)
                .unwrap();

            Line::new(Point::new(6, 203), Point::new(115, 203))
                .into_styled(style)
                .draw(&mut band)
                .unwrap();

            let mut count_buf = [0u8; 32];
            let count_str =
                format_no_std::show(&mut count_buf, format_args!("Update #{}", count)).unwrap();
            Text::new(count_str, Point::new(6, 224), text_style)
                .draw(&mut band)
                .unwrap();

            Rectangle::new(Point::new(6, 230), Size::new(110, 14))
                .into_styled(style)
                .draw(&mut band)
                .unwrap();

            Rectangle::new(Point::new(8, 232), Size::new(count * 17, 10))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(&mut band)
                .unwrap();
        }

        // Restrict controller RAM to the band, then write the new image to Black/White RAM.
        epd.set_window(0, BAND_Y, GDEM0213B74::WIDTH - 1, BAND_Y + BAND_H - 1)
            .unwrap();
        epd.set_cursor(0, BAND_Y).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, &bw_buf[..BAND_BYTES])
            .unwrap();

        ms[count as usize] = timed_refresh(&mut epd, &mut timer);

        // Copy the band just displayed into the "previous image" RAM so the next iteration diffs
        // against what is actually on the panel rather than the Phase 1 content.
        epd.set_window(0, BAND_Y, GDEM0213B74::WIDTH - 1, BAND_Y + BAND_H - 1)
            .unwrap();
        epd.set_cursor(0, BAND_Y).unwrap();
        epd.write_frame(ColorChannel::RedYellow, &bw_buf[..BAND_BYTES])
            .unwrap();

        timer.delay_ms(1000);
    }

    // --- Phase 3: Full-waveform cleanup pass. ---
    // Differential updates drive the pixels with a much shorter waveform than the OTP full-refresh
    // LUT, so pixels that flip white -> black during Phase 2 settle at a dark grey rather than a
    // deep black. Re-running the final band content through the full waveform restores even ink
    // density. Blanking the secondary RAM first also stops it being read as a second colour plane.
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);

    epd.set_window(0, BAND_Y, GDEM0213B74::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::RedYellow, &WHITE_BAND)
        .unwrap();

    // `bw_buf` still holds the last band drawn in Phase 2, so re-send it unchanged.
    epd.set_window(0, BAND_Y, GDEM0213B74::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &bw_buf[..BAND_BYTES])
        .unwrap();

    ms[7] = timed_refresh(&mut epd, &mut timer);

    // Restore the full-frame RAM window for any subsequent updates.
    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();

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
            .product("Feather RP2040 GDEM0213B74")
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
            defmt::info!("=== GDEM0213B74 2.13\" Mono (epdsi SSD1680, Feather RP2040) ===");
            defmt::info!("  Phase 1 Full:      {} ms  (RP2350 3894, C3 ~3891)", ms[0]);
            defmt::info!(
                "  Phase 2 partial:   {} {} {} {} {} {} ms  (RP2350 1018, C3 ~1017)",
                ms[1],
                ms[2],
                ms[3],
                ms[4],
                ms[5],
                ms[6]
            );
            defmt::info!("  Phase 3 cleanup:   {} ms  (full waveform)", ms[7]);
            defmt::info!("=== done ===");
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
