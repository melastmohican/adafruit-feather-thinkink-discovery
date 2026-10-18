//! # Good Display GDEQ0426T82 4.26" Monochrome E-Paper MacPaint Demo (`epdsi`)
//!
//! Port of upiir's ePaper partial update "hello" handwriting animation
//! (<https://github.com/upiir/seeed_eink_partial_update>) for the 4.26" 800x480 Monochrome
//! E-Paper Panel (GDEQ0426T82 / SSD1677), ported from the XIAO ESP32-C3 example in
//! `xiao-esp32c3-blinky`.
//! Ported from `rust-rpico2-discovery`, which does the same on a Raspberry Pi Pico 2.
//!
//! Demonstrates:
//! 1. Initial fast full refresh displaying the 640x480 MacPaint canvas centered at x=80.
//! 2. Fast 0xFC partial update loop drawing 361 handwriting points for the "hello" cursive text.
//! 3. Bounding-box windowed updates (~18 bytes per update) keeping each step around 20-30 ms.
//!
//! The panel is driven in its native 800x480 landscape orientation (no rotation), unlike the
//! portrait Ferris/Rust example, because the MacPaint canvas is landscape.
//!
//! ## Hardware
//!
//! - **Board:** Adafruit Feather RP2040 ThinkInk
//! - **Display:** Dalian Good Display GDEQ0426T82 4.26" Monochrome E-Paper Display (800x480)
//! - **Wiring:** fixed by the ThinkInk FPC socket (SCK=GPIO22, MOSI=GPIO23, CS=GPIO19,
//!   DC=GPIO18, RST=GPIO17, BUSY=GPIO16). Logs go over the USB CDC pump; see the
//!   `ssd1677_gdeq0426t82_epd` example docs for the monitor recipe.
//!
//! ## Run
//!
//! This board has no SWD header, so there is no probe-rs. **Put the board in bootloader mode
//! first**: hold **BOOT**, press and release **RESET**, then release **BOOT** — the `RPI-RP2` USB
//! mass-storage volume has to be mounted before `cargo run` can flash it (`elf2uf2-rs` runner).
//!
//! ```bash
//! cargo run --release --example ssd1677_gdeq0426t82_macpaint
//! until ls /dev | grep -q "^cu\.usbmodemEPD"; do sleep 1; done
//! cat /dev/cu.usbmodemEPD* | defmt-print -e target/thumbv6m-none-eabi/release/examples/ssd1677_gdeq0426t82_macpaint
//! ```
//!
//! The `until` loop is glob-free on purpose: an unmatched glob is a hard error in zsh, raised
//! before the command runs. `zsh: no matches found` right after flashing just means USB hasn't
//! enumerated yet.

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

use embedded_graphics::geometry::Point;
use embedded_graphics::image::Image;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epdsi::prelude::*;
use tinybmp::Bmp;

/// Screen dimensions
const EPD_WIDTH: u32 = GDEQ0426T82::WIDTH; // 800
const EPD_HEIGHT: u32 = GDEQ0426T82::HEIGHT; // 480

const EPD_BYTES_PER_ROW: usize = (EPD_WIDTH / 8) as usize; // 100 bytes
const FRAME_BYTES: usize = EPD_BYTES_PER_ROW * EPD_HEIGHT as usize; // 48,000 bytes

const BRUSH_RADIUS: i16 = 4;

/// 361 coordinate pairs for the "hello" cursive handwriting path.
#[rustfmt::skip]
const POSITIONS: [(i16, i16); 361] = [
    (263, 288), (267, 285), (272, 284), (274, 281), (278, 278), (281, 275), (285, 273), (289, 270),
    (292, 267), (294, 265), (299, 262), (301, 259), (304, 257), (308, 253), (311, 250), (313, 247),
    (316, 244), (319, 240), (321, 238), (324, 234), (327, 231), (330, 227), (332, 223), (334, 220),
    (335, 216), (336, 212), (338, 208), (339, 203), (339, 199), (338, 194), (336, 190), (335, 186),
    (332, 184), (328, 181), (324, 181), (320, 184), (317, 186), (316, 190), (315, 194), (313, 199),
    (313, 203), (312, 208), (311, 212), (311, 216), (311, 220), (309, 224), (309, 228), (308, 232),
    (308, 238), (308, 242), (307, 246), (307, 250), (305, 254), (305, 258), (305, 262), (304, 267),
    (304, 271), (303, 275), (303, 280), (301, 284), (300, 288), (300, 292), (300, 289), (301, 285),
    (301, 281), (303, 275), (304, 271), (304, 267), (305, 263), (307, 259), (308, 255), (309, 251),
    (312, 247), (315, 244), (317, 242), (321, 240), (326, 240), (331, 240), (335, 240), (338, 243),
    (342, 246), (343, 250), (344, 254), (344, 258), (344, 262), (343, 266), (343, 271), (343, 275),
    (343, 280), (343, 284), (344, 288), (348, 290), (353, 292), (357, 292), (361, 292), (365, 290),
    (369, 289), (373, 288), (377, 286), (381, 285), (385, 284), (389, 281), (392, 278), (396, 275),
    (398, 273), (401, 270), (404, 267), (407, 263), (408, 259), (409, 255), (409, 251), (408, 247),
    (407, 243), (404, 239), (401, 236), (397, 236), (392, 236), (389, 238), (385, 242), (382, 244),
    (381, 248), (378, 253), (378, 257), (377, 261), (377, 265), (377, 269), (378, 273), (381, 277),
    (382, 281), (386, 284), (389, 286), (393, 289), (397, 290), (401, 292), (405, 292), (409, 292),
    (413, 292), (417, 292), (421, 290), (425, 288), (429, 286), (434, 284), (436, 281), (439, 278),
    (443, 275), (446, 271), (448, 269), (451, 265), (454, 262), (455, 258), (458, 254), (461, 251),
    (462, 247), (465, 243), (466, 239), (467, 235), (470, 231), (471, 227), (473, 223), (474, 219),
    (475, 215), (475, 211), (477, 207), (477, 203), (477, 199), (477, 194), (477, 189), (475, 186),
    (473, 182), (469, 181), (465, 182), (461, 185), (458, 188), (455, 192), (452, 196), (451, 199),
    (450, 203), (448, 207), (447, 212), (447, 216), (446, 220), (446, 224), (444, 228), (444, 232),
    (444, 236), (444, 242), (444, 246), (444, 250), (444, 254), (444, 258), (444, 262), (444, 267),
    (446, 271), (446, 275), (447, 280), (450, 284), (452, 286), (455, 289), (459, 292), (463, 293),
    (467, 293), (471, 293), (475, 292), (479, 290), (483, 288), (488, 286), (490, 284), (494, 281),
    (497, 277), (500, 274), (502, 270), (505, 267), (508, 263), (509, 261), (512, 257), (515, 253),
    (516, 248), (519, 246), (520, 242), (523, 238), (524, 234), (525, 230), (527, 226), (528, 221),
    (529, 217), (531, 213), (532, 209), (532, 205), (533, 201), (533, 197), (533, 192), (532, 188),
    (529, 185), (525, 184), (521, 184), (519, 186), (516, 190), (513, 193), (510, 197), (509, 201),
    (508, 205), (505, 209), (504, 213), (504, 217), (502, 221), (501, 226), (501, 230), (500, 234),
    (500, 239), (500, 243), (500, 247), (500, 251), (500, 255), (501, 259), (501, 265), (502, 269),
    (504, 273), (505, 277), (506, 281), (509, 284), (512, 288), (515, 290), (519, 292), (523, 292),
    (528, 292), (532, 292), (536, 289), (539, 288), (543, 285), (546, 281), (548, 278), (551, 274),
    (552, 270), (554, 267), (556, 263), (558, 259), (559, 255), (560, 251), (563, 247), (566, 244),
    (570, 242), (573, 239), (577, 238), (581, 239), (585, 240), (589, 242), (593, 244), (596, 248),
    (597, 253), (598, 257), (600, 261), (600, 265), (600, 269), (598, 273), (598, 277), (596, 281),
    (594, 285), (591, 288), (587, 290), (583, 293), (579, 293), (575, 293), (571, 293), (567, 290),
    (564, 288), (562, 284), (560, 281), (559, 277), (558, 271), (558, 267), (558, 263), (559, 259),
    (560, 255), (562, 251), (564, 247), (567, 244), (570, 242), (574, 239), (578, 238), (582, 238),
    (586, 239), (591, 240), (596, 240), (600, 242), (604, 242), (608, 242), (612, 242), (616, 239),
    (620, 238), (623, 235), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
    (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
    (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
    (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
    (627, 232),
];

/// Draws a filled circular brush stamp (black pixels = 0) into the frame buffer.
fn draw_brush(buf: &mut [u8; FRAME_BYTES], x0: i16, y0: i16, radius: i16) {
    let r2 = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= r2 {
                let px = x0 + dx;
                let py = y0 + dy;
                if px >= 0 && px < EPD_WIDTH as i16 && py >= 0 && py < EPD_HEIGHT as i16 {
                    let byte_idx = (py as usize) * EPD_BYTES_PER_ROW + (px as usize / 8);
                    let bit_mask = 0x80 >> (px % 8);
                    buf[byte_idx] &= !bit_mask;
                }
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
        "GDEQ0426T82 4.26\" Mono (epdsi SSD1677, Feather RP2040)",
        "Feather RP2040 GDEQ0426T82",
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

    // Instantiate epdsi SPI bus wrapper and dedicated SSD1677 controller
    let epd_bus = SpiBusWrapper::new(spi_device, dc, rst, busy);
    let controller = Ssd1677Controller::new(GDEQ0426T82::WIDTH, GDEQ0426T82::HEIGHT);

    // Build EPD Driver using epdsi with GDEQ0426T82 panel specification (800x480)
    let mut epd = EpdBuilder::<_, GDEQ0426T82>::new(controller).build(epd_bus);

    epd.init(&mut timer).unwrap();

    let macpaint_bmp: Bmp<BinaryColor> =
        Bmp::from_slice(include_bytes!("macos_paint_640x480.bmp")).unwrap();

    let mut bw_buf = [0xFFu8; FRAME_BYTES];

    defmt::info!("Initializing 48 KB buffer with MacPaint canvas...");
    let mut display = PageBuffer::new(&mut bw_buf, EPD_WIDTH, EPD_HEIGHT, 0);
    display.clear_byte(0xFF);

    // Draw centered 640x480 MacPaint BMP at x=80
    Image::new(&macpaint_bmp, Point::new(80, 0))
        .draw(&mut display)
        .unwrap();

    defmt::info!("Sending initial full refresh to SSD1677...");
    epd.controller_mut()
        .set_refresh_mode(Ssd1677RefreshMode::FastFull);

    // Write both current RAM (0x24) and previous differential baseline RAM (0x26).
    epd.set_window(0, 0, EPD_WIDTH - 1, EPD_HEIGHT - 1).unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::BlackWhite, display.as_slice())
        .unwrap();

    epd.set_window(0, 0, EPD_WIDTH - 1, EPD_HEIGHT - 1).unwrap();
    epd.set_cursor(0, 0).unwrap();
    epd.write_frame(ColorChannel::RedYellow, display.as_slice())
        .unwrap();

    epd.refresh(&mut timer).unwrap();
    defmt::info!("Full refresh complete. Ready for partial updates.");
    timer.delay_ms(1000);

    for (anim_idx, &(x0, y0)) in POSITIONS.iter().enumerate() {
        draw_brush(&mut bw_buf, x0, y0, BRUSH_RADIUS);

        // Compute bounding box around brush circle
        let raw_min_x = (x0 - BRUSH_RADIUS).max(0) as u32;
        let raw_max_x = (x0 + BRUSH_RADIUS).min((EPD_WIDTH - 1) as i16) as u32;
        let b_min_y = (y0 - BRUSH_RADIUS).max(0) as u32;
        let b_max_y = (y0 + BRUSH_RADIUS).min((EPD_HEIGHT - 1) as i16) as u32;

        let start_byte_x = raw_min_x / 8;
        let end_byte_x = raw_max_x / 8;
        let bytes_per_row = (end_byte_x - start_byte_x + 1) as usize;
        let aligned_x = start_byte_x * 8;
        let aligned_w = (bytes_per_row * 8) as u32;
        let box_h = b_max_y - b_min_y + 1;
        let total_bytes = bytes_per_row * box_h as usize;

        // Extract window rows into a stack buffer
        let mut sub_buf = [0xFFu8; 64];
        for r in 0..box_h {
            let src_offset = (b_min_y + r) as usize * EPD_BYTES_PER_ROW + start_byte_x as usize;
            let dst_offset = r as usize * bytes_per_row;
            sub_buf[dst_offset..dst_offset + bytes_per_row]
                .copy_from_slice(&bw_buf[src_offset..src_offset + bytes_per_row]);
        }

        // Send bounding box window to 0x24 (BW RAM)
        epd.set_window(aligned_x, b_min_y, aligned_x + aligned_w - 1, b_max_y)
            .unwrap();
        epd.set_cursor(aligned_x, b_min_y).unwrap();
        epd.write_frame(ColorChannel::BlackWhite, &sub_buf[..total_bytes])
            .unwrap();

        // Direct differential partial refresh trigger (0xFC) on SSD1677
        epd.bus_mut()
            .send_command_with_data(0x21, &[0x00, 0x00])
            .unwrap();
        epd.bus_mut().send_command_with_data(0x22, &[0xFC]).unwrap();
        epd.bus_mut().send_command(0x20).unwrap();
        epd.bus_mut().wait_busy(true).unwrap();

        // Sync differential previous buffer 0x26 with the same window
        epd.set_cursor(aligned_x, b_min_y).unwrap();
        epd.write_frame(ColorChannel::RedYellow, &sub_buf[..total_bytes])
            .unwrap();

        if (anim_idx + 1) % 25 == 0 || anim_idx + 1 >= POSITIONS.len() {
            defmt::info!(
                "Drawing progress: {} / {} points",
                anim_idx + 1,
                POSITIONS.len()
            );
        }

        timer.delay_ms(20);
    }

    defmt::info!("Handwriting animation finished. Display complete!");

    loop {
        timer.delay_ms(1000);
    }
}
