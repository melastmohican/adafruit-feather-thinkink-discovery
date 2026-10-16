//! # Good Display GDEW0215T12 2.15" Monochrome E-Paper Example (`epdsi`, Feather RP2040 ThinkInk)
//!
//! Adafruit Feather RP2040 ThinkInk, driving the **GDEW0215T12** 2.15" Monochrome (112x208) panel
//! on the board's FPC socket with `Uc8151dController` from `epdsi`.
//!
//! Verified on this board on 15 Oct 2026: all four screens and deep sleep completed (about 11 s).
//! The blocking API is also bench verified on an XIAO ESP32-C3 on the Seeed ePaper Driver Board.
//!
//! Four screens, held briefly (`HOLD_MS`) so a run takes about 20 s:
//! 1. Full refresh on the panel's OTP waveform: header, Rust and Ferris logos, text labels.
//! 2. Partial profile, whole frame, no window (the vendor's `EPD_Dis_PartAll`), with a counter band.
//! 3. Partial profile with a window, old then new band data (the vendor's `EPD_Dis_Part`).
//! 4. The same again, counter advanced.
//!
//! Then deep sleep. Logs go out over USB through the defmt-bbq pump, like the other ThinkInk
//! examples in this repo.
//!
//! ## Connections (fixed by the board's FPC socket)
//!
//! | Signal | Feather RP2040 GPIO |
//! |--------|---------------------|
//! | SCK    | GPIO22              |
//! | MOSI   | GPIO23              |
//! | MISO   | GPIO20              |
//! | CS     | GPIO19              |
//! | DC     | GPIO18              |
//! | RST    | GPIO17              |
//! | BUSY   | GPIO16 (active-LOW) |
//!
//! ## Run
//!
//! **Put the board in bootloader mode first**: hold **BOOT**, press and release **RESET**, then
//! release **BOOT** — the `RPI-RP2` USB mass-storage volume has to be mounted before `cargo run`
//! can flash it.
//!
//! ```bash
//! cargo run --release --example uc8151d_gdew0215t12_epd
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/uc8151d_gdew0215t12_epd
//! ```
//!
//! USB comes up within about a second of boot — core1 services it independently of the panel, so
//! the `until` loop above returns almost immediately instead of waiting for the run to finish.
//! The panel then works for **about 11 seconds**, logging each screen as it completes.
//!
//! `cat` does not exit on its own — Ctrl-C once the output has printed.
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

use defmt_bbq as _;
use panic_probe as _;

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;
use tinybmp::Bmp;

/// Row stride in bytes: 112 / 8 = 14.
const STRIDE: usize = GDEW0215T12::WIDTH.div_ceil(8) as usize;

/// Full frame size: 14 x 208 = 2,912 bytes.
const FRAME_BYTES: usize = STRIDE * GDEW0215T12::HEIGHT as usize;

/// How long each screen is held before the next one, in milliseconds.
const HOLD_MS: u32 = 1_500;

/// Top row of the counter band repainted in the partial steps.
const BAND_Y: u32 = 176;

/// Height of the band in rows (y = 176..207).
const BAND_H: u32 = 32;

/// Last row of the band.
const BAND_END: u32 = BAND_Y + BAND_H - 1;

/// Byte offset of the band's first row.
const BAND_START_BYTE: usize = BAND_Y as usize * STRIDE;

/// Byte offset one past the band's last row.
const BAND_END_BYTE: usize = (BAND_END as usize + 1) * STRIDE;

/// X of the 64 px wide logos, centered in the 112 px wide panel.
const LOGO_X: i32 = (GDEW0215T12::WIDTH as i32 - 64) / 2;

/// Draws the static chrome: border, header, Rust and Ferris logos and two text labels.
fn draw_chrome(
    display: &mut PageBuffer,
    ferris_bmp: &Bmp<BinaryColor>,
    rust_bmp: &Bmp<BinaryColor>,
) {
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    Rectangle::new(
        Point::new(0, 0),
        Size::new(GDEW0215T12::WIDTH, GDEW0215T12::HEIGHT),
    )
    .into_styled(stroke)
    .draw(display)
    .unwrap();

    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Text::new("GDEW0215T12", Point::new(8, 16), style)
        .draw(display)
        .unwrap();
    Line::new(Point::new(0, 24), Point::new(111, 24))
        .into_styled(stroke)
        .draw(display)
        .unwrap();

    // Same polarity handling as the other epdsi examples: the Rust BMP draws where it is `On`,
    // the Ferris BMP where it is `Off`. On `PageBuffer`, `On` is the black ink.
    let rust_pos = Point::new(LOGO_X, 30);
    for pixel in rust_bmp.pixels() {
        if pixel.1 == BinaryColor::On {
            Pixel(pixel.0 + rust_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }
    let ferris_pos = Point::new(LOGO_X, 100);
    for pixel in ferris_bmp.pixels() {
        if pixel.1 == BinaryColor::Off {
            Pixel(pixel.0 + ferris_pos, BinaryColor::On)
                .draw(display)
                .unwrap();
        }
    }

    Text::new("Feather RP2040", Point::new(8, 160), style)
        .draw(display)
        .unwrap();
    Text::new("epdsi UC8151D", Point::new(8, 172), style)
        .draw(display)
        .unwrap();
}

/// Draws the counter band: a label and a bar that grows with the count.
fn draw_band(display: &mut PageBuffer, count: u32) {
    let clear = PrimitiveStyle::with_fill(BinaryColor::Off);
    Rectangle::new(
        Point::new(1, BAND_Y as i32),
        Size::new(GDEW0215T12::WIDTH - 2, BAND_H - 1),
    )
    .into_styled(clear)
    .draw(display)
    .unwrap();

    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let label = heapless_label(count);
    Text::new(label.as_str(), Point::new(8, BAND_Y as i32 + 12), style)
        .draw(display)
        .unwrap();

    let fill = PrimitiveStyle::with_fill(BinaryColor::On);
    Rectangle::new(
        Point::new(8, BAND_Y as i32 + 18),
        Size::new(20 * count + 4, 8),
    )
    .into_styled(fill)
    .draw(display)
    .unwrap();
}

/// A tiny fixed-size label buffer, so the example needs no allocator or `format_no_std`.
struct Label {
    buf: [u8; 12],
    len: usize,
}

impl Label {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("?")
    }
}

/// Builds `"update N"` for a single-digit `N`.
fn heapless_label(count: u32) -> Label {
    let mut buf = [0u8; 12];
    let text = b"update ";
    buf[..text.len()].copy_from_slice(text);
    buf[text.len()] = b'0' + (count % 10) as u8;
    Label {
        buf,
        len: text.len() + 1,
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
    // UC8151D BUSY is active-LOW, so pull up: a floating line reads "idle".
    let busy = pins.gpio16.into_pull_up_input();

    // 4 MHz: this chip's serial write cycle is at least 100 ns, so 10 MHz is the ceiling.
    let spi = spi::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        4_000_000u32.Hz(),
        embedded_hal::spi::MODE_0,
    );

    spawn_usb_log_pump(
        Core1Handles {
            psm: &mut pac.PSM,
            ppb: &mut pac.PPB,
            fifo: &mut sio.fifo,
        },
        "GDEW0215T12 2.15\" Mono (epdsi UC8151D, Feather RP2040)",
        "Feather RP2040 GDEW0215T12",
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
    let controller = Uc8151dController::new(GDEW0215T12::WIDTH, GDEW0215T12::HEIGHT);
    let mut epd = EpdBuilder::<_, GDEW0215T12>::new(controller).build(epd_bus);

    defmt::info!("Initializing UC8151D (Full profile)...");
    epd.init(&mut timer).unwrap();

    let mut buf = [0xFFu8; FRAME_BYTES];
    let mut display = PageBuffer::new(&mut buf, GDEW0215T12::WIDTH, GDEW0215T12::HEIGHT, 0);
    let ferris_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("ferrisbw.bmp")).unwrap();
    let rust_bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("rustbw.bmp")).unwrap();

    defmt::info!("--- Screen 1 of 4: full refresh on the OTP waveform ---");
    draw_chrome(&mut display, &ferris_bmp, &rust_bmp);

    // Old plane white first, as the vendor demo does, then the new plane.
    epd.set_window(0, 0, GDEW0215T12::WIDTH - 1, GDEW0215T12::HEIGHT - 1)
        .unwrap();
    epd.clear_frame(ColorChannel::RedYellow, 0xFF).unwrap();
    epd.set_window(0, 0, GDEW0215T12::WIDTH - 1, GDEW0215T12::HEIGHT - 1)
        .unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();
    epd.refresh(&mut timer).unwrap();
    timer.delay_ms(HOLD_MS);

    // The UC8151D's partial profile uploads its own LUT, so it needs a fresh init.
    epd.controller_mut()
        .set_refresh_mode(Uc8151dRefreshMode::Partial);
    epd.init(&mut timer).unwrap();
    epd.controller_mut().clear_window();

    defmt::info!("--- Screen 2 of 4: partial profile, whole frame, no window ---");
    epd.set_window(0, 0, GDEW0215T12::WIDTH - 1, GDEW0215T12::HEIGHT - 1)
        .unwrap();
    epd.write_frame(ColorChannel::RedYellow, display.as_slice())
        .unwrap();
    epd.controller_mut().clear_window();
    draw_band(&mut display, 1);
    epd.set_window(0, 0, GDEW0215T12::WIDTH - 1, GDEW0215T12::HEIGHT - 1)
        .unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();
    epd.controller_mut().clear_window();
    epd.refresh(&mut timer).unwrap();
    timer.delay_ms(HOLD_MS);

    for count in 2..=3u32 {
        defmt::info!(
            "--- Screen {} of 4: partial window y={}..{}, update {} ---",
            count + 1,
            BAND_Y,
            BAND_END,
            count
        );
        // Old band = what the panel shows now, which is the previous pass's band.
        epd.set_window(0, BAND_Y, GDEW0215T12::WIDTH - 1, BAND_END)
            .unwrap();
        epd.write_frame(
            ColorChannel::RedYellow,
            &display.as_slice()[BAND_START_BYTE..BAND_END_BYTE],
        )
        .unwrap();
        draw_band(&mut display, count);
        epd.write_frame(
            ColorChannel::BlackWhite,
            &display.as_slice()[BAND_START_BYTE..BAND_END_BYTE],
        )
        .unwrap();
        epd.refresh(&mut timer).unwrap();
        epd.controller_mut().clear_window();
        timer.delay_ms(HOLD_MS);
    }

    defmt::info!("--- Deep sleep ---");
    epd.sleep(&mut timer).unwrap();
    defmt::info!("=== done ===");

    // Core1 keeps servicing USB and draining defmt-bbq indefinitely; core0's work is done.
    loop {
        cortex_m::asm::wfi();
    }
}
