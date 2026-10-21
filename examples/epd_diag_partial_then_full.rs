//! # SSD1680 Partial, then Full with no `init` (Feather RP2040 ThinkInk)
//!
//! Settles one question for the `epdsi` backlog item `ghost-debt-refresh-policy`: after a `Partial`
//! refresh, can the caller go back to `Full` with only `set_refresh_mode(Full)`, or does it have to
//! call `init` again?
//!
//! ## Why it might matter
//!
//! `Ssd168xRefreshMode::Partial` on the SSD1680 starts its refresh with a hardware reset. The
//! datasheet's power-on values (the ones a reset should restore) differ from what `init` writes:
//!
//! | Register | Power-on value | `init` writes | Visible effect if left at power-on |
//! | :--- | :--- | :--- | :--- |
//! | `0x21` source output mode (B7) | 0, sources S0 to S175 | 1, sources S8 to S167 | image shifted sideways by 8 px |
//! | `0x3C` border waveform | `0xC0` (VBD high impedance) | `0x05`, then `0x80` after a partial | border colour differs |
//! | `0x01` driver output control (MUX) | 296 gate lines | 250 | none expected |
//!
//! Whether a reset really restores those values on this chip is the open question. `Partial` is
//! hardware-verified on its own; this checks what comes after it.
//!
//! ## Stages
//!
//! The same asymmetric pattern is drawn in stages 1, 3 and 5: horizontal stripes plus a solid black
//! block in each corner. A sideways shift moves or clips the left and right blocks.
//!
//! | Stage | What happens | Panel |
//! | :---: | :--- | :--- |
//! | 1 | `init`, `Full`, pattern | baseline, what a correct Full looks like |
//! | 2 | `Partial`, inverted stripes | stripes invert, corner blocks stay |
//! | 3 | `set_refresh_mode(Full)` only, pattern | **the test**, held 20 s |
//! | 4 | `Partial` again, inverted stripes | stripes invert |
//! | 5 | `set_refresh_mode(Full)` then `init`, pattern | control, held 20 s |
//!
//! ## Reading the result
//!
//! Photograph the panel at the end of stage 3 and again at the end of stage 5, and compare both with
//! stage 1.
//!
//! - **3 matches 1 and 5:** no `init` needed. Record it in the backlog and drop the `init` advice.
//! - **3 differs, 5 matches 1:** a reset does lose registers. Callers must `init` after a `Partial`
//!   on the SSD1680 and SSD1677 (the SSD1677 partial path has the same reset), and the doc says so.
//! - **3 and 5 both differ from 1:** the cause is something other than the reset. Stop and look
//!   at stage 1 against the earlier A/B/C bisect in `epd_diag_partial`.
//!
//! The timings help too. Stage 3 should take about as long as stage 1 (about 3.9 s on this panel).
//! A very short Stage 3 means the update did not run the full waveform.
//!
//! ## Measured output
//!
//! ```text
//! === SSD1680 Partial then Full (Feather RP2040 ThinkInk) ===
//! 1  init + Full, pattern: 3894 ms
//! 2  Partial, inverted stripes: 1184 ms
//! 3  Full, NO init, pattern: 3893 ms
//! 4  Partial, inverted stripes: 1184 ms
//! 5  Full, WITH init, pattern: 3894 ms
//! ```
//!
//! Two runs on a `GDEM0213B74` (21 Oct 2026). The panel at the end of stage 3 looked the same as at
//! the end of stage 5 in both: same stripes, corner blocks intact at both ends, no sideways shift
//! and no visible border change. So on the SSD1680, `set_refresh_mode(Full)` alone is enough after
//! a `Partial`. Limits: two photos per run, matched to stages by timestamp, and border colour is
//! hard to judge from a photo.
//!
//! ## Hardware
//!
//! Same as `epd_diag_partial`: Adafruit Feather RP2040 ThinkInk with a GDEM0213B74 2.13" in the
//! board's 24-pin socket. Results come over USB CDC on core1, as in that example.
//!
//! ## Run
//!
//! Put the board in bootloader mode first (hold **BOOT**, press **RESET**, release **BOOT**).
//!
//! ```bash
//! cargo run --release --example epd_diag_partial_then_full
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/epd_diag_partial_then_full
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

const STRIDE: usize = GDEM0213B74::WIDTH.div_ceil(8) as usize;
const HEIGHT: usize = GDEM0213B74::HEIGHT as usize;
const FRAME_BYTES: usize = STRIDE * HEIGHT;
const BLOCK_ROWS: usize = 24;

/// Horizontal stripes, `phase` selecting which bands are black.
fn stripes(buf: &mut [u8], phase: usize) {
    buf.fill(0xFF);
    for row in 0..HEIGHT {
        if (row / 20) % 2 == phase {
            buf[row * STRIDE..(row + 1) * STRIDE].fill(0x00);
        }
    }
}

/// The reference pattern: stripes plus a solid 8 px wide block in each corner. The right-hand
/// block sits in the second-to-last byte, which is fully inside the 122 px width.
fn pattern(buf: &mut [u8]) {
    stripes(buf, 0);
    let left = 0;
    let right = STRIDE - 2;
    for row in (0..BLOCK_ROWS).chain(HEIGHT - BLOCK_ROWS..HEIGHT) {
        buf[row * STRIDE + left] = 0x00;
        buf[row * STRIDE + right] = 0x00;
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
    // SSD1680 BUSY is active-HIGH, so pull down: a floating line reads "idle".
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
        "SSD1680 Partial then Full (Feather RP2040 ThinkInk)",
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
    let controller = Ssd1680Controller::new(GDEM0213B74::WIDTH, GDEM0213B74::HEIGHT);
    let mut epd = EpdBuilder::<_, GDEM0213B74>::new(controller).build(epd_bus);

    let mut buf = [0xFFu8; FRAME_BYTES];

    // Both RAM planes get the frame, so a following Partial diffs against what is on the panel.
    macro_rules! write_both_planes {
        () => {{
            for channel in [ColorChannel::BlackWhite, ColorChannel::RedYellow] {
                epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
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
        .set_refresh_mode(Ssd168xRefreshMode::Full);
    pattern(&mut buf[..]);
    write_both_planes!();
    timed_refresh!("1  init + Full, pattern: {} ms");
    timer.delay_ms(3000);

    // --- 2: Partial, inverted stripes. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);
    stripes(&mut buf[..], 1);
    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..]).unwrap();
    timed_refresh!("2  Partial, inverted stripes: {} ms");
    timer.delay_ms(3000);

    // --- 3: THE TEST. Back to Full with the mode switch alone, no init. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);
    pattern(&mut buf[..]);
    write_both_planes!();
    timed_refresh!("3  Full, NO init, pattern: {} ms");
    defmt::info!("STAGE 3 ON PANEL NOW: photograph it, 20 s");
    timer.delay_ms(20_000);

    // --- 4: Partial again, so stage 5 starts from the same place stage 3 did. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);
    stripes(&mut buf[..], 1);
    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..]).unwrap();
    timed_refresh!("4  Partial, inverted stripes: {} ms");
    timer.delay_ms(3000);

    // --- 5: control. Back to Full with the mode switch and a fresh init. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);
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
