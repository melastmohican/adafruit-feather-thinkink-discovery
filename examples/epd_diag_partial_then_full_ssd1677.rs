//! # SSD1677 Partial, then Full with no `init` (Feather RP2040 ThinkInk)
//!
//! The SSD1677 twin of `epd_diag_partial_then_full`, for the `GDEQ0426T82` 4.26". It settles the
//! `epdsi` backlog question for this controller: after a `Partial` refresh, can the caller go back
//! to `Full` with only `set_refresh_mode(Full)`, or does it have to call `init` again?
//!
//! ## Why it might matter
//!
//! `Ssd1677RefreshMode::Partial` starts its refresh with a hardware reset, then sends
//! `TEMP_CONTROL 0x80` and `BORDER_WAVEFORM_CONTROL 0x80`. Nothing re-runs `init_sequence`
//! afterwards. `init` is what writes the booster soft start (`0x0C`), driver output control
//! (`0x01`, the gate count) and the `0x3C` border value, and which of those a reset restores is
//! the open question. If the gate count or the booster settings fall back to defaults, the likely
//! signs are corner blocks missing at the top or bottom, a squashed or shifted image, or a
//! different border colour.
//!
//! Existing evidence: the `ssd1677_gdeq0426t82_epd` example already goes `Partial` then `Full`
//! with no `init` in its Phase 3 and measures 3745 ms. That shows the full waveform ran. It does
//! not record whether the picture was identical to one drawn after an `init`, which is what this
//! example photographs.
//!
//! ## Panel geometry
//!
//! The buffer is in the panel's native 800x480 layout, with no rotation: 100 bytes per row, 480
//! rows. The pattern is 12 horizontal bands plus a solid 8 px wide, 24 row tall block in each of
//! the four corners (first and last byte of the first and last 24 rows).
//!
//! ## Stages
//!
//! | Stage | What happens | Panel |
//! | :---: | :--- | :--- |
//! | 1 | `init`, `Full`, pattern on both RAM planes | baseline, what a correct Full looks like |
//! | 2 | `Partial`, inverted bands | bands invert, corner blocks stay |
//! | 3 | `set_refresh_mode(Full)` only, pattern | **the test**, held 20 s |
//! | 4 | `Partial` again, inverted bands | bands invert |
//! | 5 | `set_refresh_mode(Full)` then `init`, pattern | control, held 20 s |
//!
//! ## Reading the result
//!
//! Photograph the panel at the end of stage 3 and again at the end of stage 5, and compare both with
//! stage 1.
//!
//! - **3 matches 1 and 5:** no `init` needed after a `Partial` on the SSD1677.
//! - **3 differs, 5 matches 1:** a reset loses registers. Callers must `init` after a `Partial` on
//!   the SSD1677, and the `RefreshPolicy` doc says so.
//! - **3 and 5 both differ from 1:** the cause is something other than the reset.
//!
//! Stage 3 should take about as long as stage 1 (about 3.7 s). Stage 2 and 4 are about 0.5 s on
//! this board.
//!
//! ## Measured output
//!
//! ```text
//! === SSD1677 Partial then Full (Feather RP2040 ThinkInk) ===
//! 1  init + Full, pattern: 3746 ms
//! 2  Partial, inverted bands: 665 ms
//! 3  Full, NO init, pattern: 3746 ms
//! 4  Partial, inverted bands: 665 ms
//! 5  Full, WITH init, pattern: 3745 ms
//! ```
//!
//! One run on a `GDEQ0426T82` (21 Oct 2026). The panel at the end of stage 3 looked the same as at
//! the end of stage 5: same 12 bands, corner marks in the same places, nothing squashed or shifted.
//! So on the SSD1677, `set_refresh_mode(Full)` alone is enough after a `Partial`. Limits: one run,
//! photos taken at an angle, the 8 px corner blocks are small, and border colour was not judged.
//!
//! ## Hardware
//!
//! Adafruit Feather RP2040 ThinkInk with a GDEQ0426T82 4.26" (SSD1677) in the board's 24-pin
//! socket. **Swap panels with the board unpowered.** Results come over USB CDC on core1, as in the
//! other diagnostics here.
//!
//! ## Run
//!
//! Put the board in bootloader mode first (hold **BOOT**, press **RESET**, release **BOOT**).
//!
//! ```bash
//! cargo run --release --example epd_diag_partial_then_full_ssd1677
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/epd_diag_partial_then_full_ssd1677
//! ```
//!
//! The run takes about 70 seconds, most of it the two 20 s holds for photographs. The log prints
//! `STAGE 3 ON PANEL NOW` and `STAGE 5 ON PANEL NOW` when each hold starts.

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

use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;

const STRIDE: usize = GDEQ0426T82::WIDTH.div_ceil(8) as usize;
const HEIGHT: usize = GDEQ0426T82::HEIGHT as usize;
const FRAME_BYTES: usize = STRIDE * HEIGHT;
const BAND_ROWS: usize = 40;
const BLOCK_ROWS: usize = 24;

/// Horizontal bands, `phase` selecting which ones are black.
fn bands(buf: &mut [u8], phase: usize) {
    buf.fill(0xFF);
    for row in 0..HEIGHT {
        if (row / BAND_ROWS) % 2 == phase {
            buf[row * STRIDE..(row + 1) * STRIDE].fill(0x00);
        }
    }
}

/// The reference pattern: bands plus a solid 8 px wide block in each corner. 800 px is byte
/// aligned, so the first and last byte of a row are fully inside the panel.
fn pattern(buf: &mut [u8]) {
    bands(buf, 0);
    for row in (0..BLOCK_ROWS).chain(HEIGHT - BLOCK_ROWS..HEIGHT) {
        buf[row * STRIDE] = 0x00;
        buf[row * STRIDE + STRIDE - 1] = 0x00;
    }
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

    // USB is serviced on core1 so `epd.refresh()` can block on core0 for as long as it needs.
    spawn_usb_log_pump(
        Core1Handles {
            psm: &mut pac.PSM,
            ppb: &mut pac.PPB,
            fifo: &mut sio.fifo,
        },
        "SSD1677 Partial then Full (Feather RP2040 ThinkInk)",
        "Feather RP2040 EPD diag",
        UsbParts {
            regs: pac.USBCTRL_REGS,
            dpram: pac.USBCTRL_DPRAM,
            clock: clocks.usb_clock,
        },
        pac.RESETS,
        watchdog,
        bbq,
    );

    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1677Controller::new(GDEQ0426T82::WIDTH, GDEQ0426T82::HEIGHT);
    let mut epd = EpdBuilder::<_, GDEQ0426T82>::new(controller).build(epd_bus);

    let mut buf = [0xFFu8; FRAME_BYTES];

    // Both RAM planes get the frame, so a following Partial diffs against what is on the panel.
    macro_rules! write_both_planes {
        () => {{
            for channel in [ColorChannel::BlackWhite, ColorChannel::RedYellow] {
                epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
                    .unwrap();
                epd.set_cursor(0, 0).unwrap();
                epd.write_frame(channel, &buf[..]).unwrap();
            }
        }};
    }
    macro_rules! timed_refresh {
        ($label:literal) => {{
            let t = timer.get_counter().ticks();
            epd.refresh(&mut timer).unwrap();
            let ms = (timer.get_counter().ticks() - t) / 1000;
            defmt::info!($label, ms);
        }};
    }

    // --- 1: init, Full, reference pattern. ---
    epd.init(&mut timer).unwrap();
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Full);
    pattern(&mut buf[..]);
    write_both_planes!();
    timed_refresh!("1  init + Full, pattern: {} ms");
    timer.delay_ms(3000);

    // --- 2: Partial, inverted bands. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Partial);
    bands(&mut buf[..], 1);
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..]).unwrap();
    timed_refresh!("2  Partial, inverted bands: {} ms");
    timer.delay_ms(3000);

    // --- 3: THE TEST. Back to Full with the mode switch alone, no init. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Full);
    pattern(&mut buf[..]);
    write_both_planes!();
    timed_refresh!("3  Full, NO init, pattern: {} ms");
    defmt::info!("STAGE 3 ON PANEL NOW: photograph it, 20 s");
    timer.delay_ms(20_000);

    // --- 4: Partial again, so stage 5 starts from the same place stage 3 did. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Partial);
    bands(&mut buf[..], 1);
    epd.set_window(0, 0, GDEQ0426T82::WIDTH - 1, GDEQ0426T82::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..]).unwrap();
    timed_refresh!("4  Partial, inverted bands: {} ms");
    timer.delay_ms(3000);

    // --- 5: control. Back to Full with the mode switch and a fresh init. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::Full);
    epd.init(&mut timer).unwrap();
    pattern(&mut buf[..]);
    write_both_planes!();
    timed_refresh!("5  Full, WITH init, pattern: {} ms");
    defmt::info!("STAGE 5 ON PANEL NOW: photograph it, 20 s");
    timer.delay_ms(20_000);

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
