//! USB-serial reporting for the e-paper examples.
//!
//! This board has no SWD connector fitted, so `defmt` output cannot go over RTT without soldering.
//! It goes over USB CDC via `defmt-bbq` instead. That imposes a shape on every e-paper example
//! here, and this module exists so the shape is written once rather than eight times.
//!
//! USB CDC needs `usb_dev.poll()` called every few milliseconds or the host drops the device, and
//! `epd.refresh()` blocks for seconds at a time with nothing to pump it. `defmt-bbq` also
//! *discards* buffered log data while the device is unconfigured, so anything logged before the
//! host connects is lost rather than queued.
//!
//! So an example runs the panel **silently**, recording timings into a [`Report`], and then hands
//! that to [`report_and_park`], which brings USB up, emits everything once the host has attached,
//! and never returns. The serial device appears only after the panel work finishes.
//!
//! The panel is the better progress indicator anyway: a stage that completes fast *without*
//! visibly changing the display has not driven the ink, which no timing figure will tell you.
//!
//! Used from an example with:
//!
//! ```ignore
//! use adafruit_feather_thinkink_discovery::usb_report::{report_and_park, Report, UsbParts};
//! ```

use adafruit_feather_rp2040 as bsp;
use bsp::hal::usb::UsbBus;
use bsp::hal::{clocks::UsbClock, Watchdog};
use bsp::pac;
use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

/// Maximum number of timings one example can record. Raise if a phase list outgrows it; recording
/// beyond this is dropped rather than panicking, since losing a log line mid-bring-up is a much
/// smaller problem than a panic that loses all of them.
pub const MAX_LINES: usize = 24;

/// A label and an elapsed time, as measured during the silent phase of a run.
#[derive(Clone, Copy)]
struct Line {
    label: &'static str,
    ms: u64,
}

/// Timings collected while USB is down, to be emitted once it comes up.
pub struct Report {
    lines: [Line; MAX_LINES],
    len: usize,
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}

impl Report {
    pub const fn new() -> Self {
        Self {
            lines: [Line { label: "", ms: 0 }; MAX_LINES],
            len: 0,
        }
    }

    /// Records one measurement. Silently ignored once [`MAX_LINES`] is reached.
    pub fn record(&mut self, label: &'static str, ms: u64) {
        if self.len < MAX_LINES {
            self.lines[self.len] = Line { label, ms };
            self.len += 1;
        }
    }

    /// Number of measurements recorded.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Everything `report_and_park` needs from the peripherals, gathered so the call stays readable.
pub struct UsbParts {
    pub regs: pac::USBCTRL_REGS,
    pub dpram: pac::USBCTRL_DPRAM,
    pub clock: UsbClock,
}

/// Brings USB up, emits `title` and every recorded line, then parks forever.
///
/// `product` becomes the USB product string, which is how you tell which firmware is on the board
/// without decoding anything:
///
/// ```bash
/// ioreg -r -c IOUSBHostDevice -l | grep -o '"USB Product Name" = "Feather[^"]*"'
/// ```
///
/// The serial number is deliberately the same `"EPD"` for every example, so the device node is
/// always `/dev/cu.usbmodemEPD*` and the decode command never changes as examples are added.
pub fn report_and_park(
    title: &'static str,
    product: &'static str,
    report: &Report,
    usb: UsbParts,
    resets: &mut pac::RESETS,
    watchdog: &mut Watchdog,
    mut bbq: defmt_bbq::DefmtConsumer,
) -> ! {
    let usb_bus = UsbBusAllocator::new(UsbBus::new(usb.regs, usb.dpram, usb.clock, true, resets));

    let mut serial = SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Adafruit")
            .product(product)
            .serial_number("EPD")])
        .unwrap()
        .device_class(2) // CDC
        .build();

    let mut reported = false;

    loop {
        watchdog.feed();

        // Must be called as often as possible to keep the USB device alive.
        if usb_dev.poll(&mut [&mut serial]) {
            let mut rx = [0u8; 64];
            let _ = serial.read(&mut rx);
        }

        // Emit once, and only after the host has configured the device — anything logged before
        // that is dropped by the drain below rather than buffered.
        if !reported && usb_dev.state() == UsbDeviceState::Configured {
            defmt::info!("=== {} ===", title);
            for line in &report.lines[..report.len] {
                defmt::info!("  {}: {} ms", line.label, line.ms);
            }
            defmt::info!("=== done ===");
            reported = true;
        }

        // Drain binary defmt-bbq frames to the USB serial port.
        while let Ok(grant) = bbq.read() {
            if usb_dev.state() == UsbDeviceState::Configured {
                if let Ok(written) = serial.write(&grant) {
                    grant.release(written);
                } else {
                    break;
                }
            } else {
                // Not configured: release rather than let the buffer fill.
                let len = grant.len();
                grant.release(len);
            }
        }
    }
}
