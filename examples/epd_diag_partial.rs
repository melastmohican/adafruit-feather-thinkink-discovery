//! # SSD1680 partial-refresh bisect (Feather RP2040 ThinkInk)
//!
//! The `epdsi` calls and the A/B/C sequence are identical to the ports of this diagnostic in
//! `rust-rpico2-discovery` and `xiao-esp32c3-blinky`; only the board bring-up and the reporting
//! differ. Those two log over RTT, which needs a debug probe. **This board has no SWD connector
//! fitted**, so results come over USB CDC via `defmt-bbq` instead — no probe and no soldering.
//!
//! ## Results arrive live, one test at a time
//!
//! USB CDC needs `usb_dev.poll()` called every few milliseconds or the host drops the device, and
//! `epd.refresh()` blocks for seconds at a time with nothing to pump it. USB is serviced on core1
//! while core0 runs the tests — see
//! [`usb_report`](adafruit_feather_thinkink_discovery::usb_report). Each test's timing is logged
//! as soon as it completes.
//!
//! **The panel is the better instrument here anyway.** Each stage draws a distinct pattern — A
//! horizontal stripes, B inverted stripes, C stripes only below y=50 — so you can watch the
//! sequence advance without any log at all. A stage that finishes fast *without* visibly changing
//! the panel has not driven the ink, and that is a different fault from one that stalls.
//!
//! ## Measurements so far
//!
//! Same panel, same driver, same 4 MHz SPI:
//!
//! | Host | A (full, `Full`) | B (full, `Partial`) | C (band, `Partial`) |
//! | :--- | ---: | ---: | ---: |
//! | RP2350 + DESPI-C02 | 3894 ms | 1018 ms | 1018 ms |
//! | XIAO ESP32-C3 (reference, when healthy) | ~3891 ms | ~1017 ms | ~1017 ms |
//! | XIAO ESP32-C3 (after degrading) | 7450 ms | 98 ms | 333 ms |
//! | **Feather RP2040 ThinkInk** | **?** | **?** | **?** |
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk ([Product 5727](https://www.adafruit.com/product/5727))
//! - **Display:** Good Display GDEM0213B74 2.13" Monochrome, 122x250, seated directly in the
//!   board's 24-pin FPC EPD socket. Its ribbon is stamped `FPC-7528B`.
//!
//! Connections are fixed by that socket — SCK GP22, MOSI GP23, CS GP19, DC GP18, RST GP17,
//! BUSY GP16. These are SPI0 on the RP2040, even though the Arduino core calls the port SPI1.
//!
//! ## Measured output
//!
//! ```text
//! === SSD1680 partial-refresh bisect (Feather RP2040 ThinkInk) ===
//! A  full frame, Full: 3893 ms
//! B  full frame, Partial (RP2350 1018, healthy C3 ~1017, faulty C3 98):   1018 ms
//! C  band, Partial       (RP2350 1018, healthy C3 ~1017, faulty C3 333):  1018 ms
//! === done ===
//! ```
//!
//! Two independent healthy hosts agreeing to a millisecond is what makes the faulty C3 row
//! obviously a host fault rather than a driver one.
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example epd_diag_partial
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/epd_diag_partial
//! ```
//!
//! USB comes up within about a second of boot now — core1 services it independently of the panel,
//! so the `until` loop above returns almost immediately instead of waiting for the run to finish.
//! The panel then runs A, B and C for **about 15 seconds**, and each result logs as it lands.
//!
//! `cat` does not exit on its own — Ctrl-C once the output has printed. `screen` will show binary
//! garbage: the frames are defmt-encoded, so `defmt-print` is required.
//!
//! **`zsh: no matches found: /dev/cu.usbmodem*` right after flashing just means enumeration hasn't
//! finished yet** — it should clear within a second or two, not the full 15 s run. If it doesn't
//! clear quickly, confirm the board was actually in bootloader mode before flashing.
//!
//! Every example in this repo uses the same USB serial, so that glob never changes. To see
//! which firmware is actually on the board, read the USB product string:
//!
//! ```bash
//! ioreg -r -c IOUSBHostDevice -l | grep -o '"USB Product Name" = "Feather[^"]*"'
//! ```
//!
//! The first decoded log line names it too.
//!
//! `defmt-print` must be given the ELF matching what is on the board — a mismatch yields garbled
//! or missing messages rather than an error. Every example here is run with `--release`, so the
//! path only ever differs in the final component.

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
const FRAME_BYTES: usize = STRIDE * GDEM0213B74::HEIGHT as usize;
const BAND_Y: u32 = 50;
const BAND_H: u32 = 200;
const BAND_BYTES: usize = STRIDE * BAND_H as usize;

/// Horizontal stripes, `phase` selecting which bands are black.
fn stripes(buf: &mut [u8], rows: usize, phase: usize) {
    buf.fill(0xFF);
    for row in 0..rows {
        if (row / 20) % 2 == phase {
            for b in 0..STRIDE {
                buf[row * STRIDE + b] = 0x00;
            }
        }
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

    // 4 MHz to match the other ports exactly — see the module docs.
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
        "SSD1680 partial-refresh bisect (Feather RP2040 ThinkInk)",
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

    // Unlike the hand-rolled `jd79661` example here, `SpiBusWrapper` expects the `SpiDevice` to
    // own CS, so the real chip-select goes to `ExclusiveDevice` rather than a dummy pin.
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1680Controller::new(GDEM0213B74::WIDTH, GDEM0213B74::HEIGHT);
    let mut epd = EpdBuilder::<_, GDEM0213B74>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    let mut buf = [0xFFu8; FRAME_BYTES];

    // --- A: full frame, Full mode. Baseline. Panel: horizontal stripes. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Full);
    stripes(&mut buf[..], GDEM0213B74::HEIGHT as usize, 0);

    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..]).unwrap();
    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, &buf[..]).unwrap();

    let t = timer.get_counter().ticks();
    epd.refresh(&mut timer).unwrap();
    let ms = (timer.get_counter().ticks() - t) / 1000;
    defmt::info!("A  full frame, Full: {} ms", ms);
    timer.delay_ms(3000);

    // --- B: full frame, Partial mode. Panel: stripes invert. ---
    epd.controller_mut()
        .set_refresh_mode(Ssd168xRefreshMode::Partial);
    stripes(&mut buf[..], GDEM0213B74::HEIGHT as usize, 1);

    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..]).unwrap();

    let t = timer.get_counter().ticks();
    epd.refresh(&mut timer).unwrap();
    let ms = (timer.get_counter().ticks() - t) / 1000;
    defmt::info!("B  full frame, Partial: {} ms", ms);
    timer.delay_ms(3000);

    // Keep the previous-image RAM in step so C diffs against what is on the panel.
    epd.set_window(0, 0, GDEM0213B74::WIDTH - 1, GDEM0213B74::HEIGHT - 1)
        .unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, &buf[..]).unwrap();

    // --- C: banded, Partial mode. Panel: lower part inverts. ---
    stripes(&mut buf[..BAND_BYTES], BAND_H as usize, 0);

    epd.set_window(0, BAND_Y, GDEM0213B74::WIDTH - 1, BAND_Y + BAND_H - 1)
        .unwrap();
    epd.set_cursor(0, BAND_Y).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, &buf[..BAND_BYTES])
        .unwrap();

    let t = timer.get_counter().ticks();
    epd.refresh(&mut timer).unwrap();
    let ms = (timer.get_counter().ticks() - t) / 1000;
    defmt::info!("C  band, Partial: {} ms", ms);

    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
