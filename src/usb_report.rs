//! USB-serial reporting for the e-paper examples.
//!
//! This board has no SWD connector fitted, so `defmt` output cannot go over RTT without soldering.
//! It goes over USB CDC via `defmt-bbq` instead. That imposes a shape on every e-paper example
//! here, and this module exists so the shape is written once rather than eight times.
//!
//! USB CDC needs `usb_dev.poll()` called every few milliseconds or the host drops the device, and
//! `epd.refresh()` blocks for seconds at a time with nothing to pump it. Rather than deferring all
//! logging until the panel work finishes, USB is serviced on **core1** while core0 does the panel
//! work: [`spawn_usb_log_pump`] spawns core1 running forever, bringing USB up, polling it, and
//! draining `defmt-bbq` to serial whenever the host has the device configured. Core0 is free to
//! block in `epd.refresh()` for as long as it needs to — core1's poll loop never depends on it —
//! and can call `defmt::info!()` directly at each phase, with output reaching the host live
//! instead of in one dump at the end.
//!
//! **Core1 must never call `defmt::*!`.** This repo's `bbqueue` is built with the `thumbv6`
//! feature, so its atomics are implemented via `cortex_m::interrupt::free` (Cortex-M0+ has no
//! LDREX/STREX) — that only masks the *executing core's* interrupts, not the other core's.
//! `defmt-bbq`'s single-producer/single-consumer split is only sound with exactly one producer;
//! `bbqueue::Consumer` is `unsafe impl Send` specifically so its *consumer* half can move to
//! another core, but a second producer (a `defmt::*!` call from core1) would race core0's producer
//! on `defmt-bbq`'s internal state. So `defmt_bbq::init()` stays a core0 call, and only the
//! returned [`defmt_bbq::DefmtConsumer`] moves to core1.
//!
//! Used from an example with:
//!
//! ```ignore
//! use adafruit_feather_thinkink_discovery::usb_report::{
//!     spawn_usb_log_pump, Core1Handles, UsbParts,
//! };
//! ```

use adafruit_feather_rp2040 as bsp;
use bsp::hal::multicore::{Multicore, Stack};
use bsp::hal::sio::SioFifo;
use bsp::hal::usb::UsbBus;
use bsp::hal::{clocks::UsbClock, Watchdog};
use bsp::pac;
use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

/// Core1's stack for [`run_usb_log_pump`]. Sized generously (32 KiB) against the RP2040's 256 KiB
/// SRAM: it hosts a full USB CDC stack (`usb-device` + `usbd-serial`), not a trivial loop.
static mut CORE1_STACK: Stack<8192> = Stack::new();

// `timestamp!` must be defined exactly once across the crate graph (defmt panics/links otherwise);
// this is that one definition, shared by every example that pulls in this module. Reads the
// RP2040's free-running microsecond counter directly off the register block rather than through a
// `Timer` handle, since a `#[defmt::timestamp]` provider is a plain function with no state to
// close over — every phase's `defmt::info!` log line comes out tagged with when it was emitted, so
// gaps between lines on the host are visible instead of implied by wall-clock guesswork.
defmt::timestamp!("{=u64:us}", {
    let timer = unsafe { &*pac::TIMER::ptr() };
    let hi = timer.timerawh().read().bits() as u64;
    let lo = timer.timerawl().read().bits() as u64;
    (hi << 32) | lo
});

/// Everything `run_usb_log_pump` needs from the peripherals, gathered so the call stays readable.
pub struct UsbParts {
    pub regs: pac::USBCTRL_REGS,
    pub dpram: pac::USBCTRL_DPRAM,
    pub clock: UsbClock,
}

/// The peripherals [`spawn_usb_log_pump`] needs to bring up core1, borrowed only for the duration
/// of that call — gathered so the function stays under clippy's argument-count lint.
pub struct Core1Handles<'a> {
    pub psm: &'a mut pac::PSM,
    pub ppb: &'a mut pac::PPB,
    pub fifo: &'a mut SioFifo,
}

/// Brings up core1 as the USB log pump and returns immediately: logs `title` (core0 is the sole
/// defmt producer — see the module docs — so it can't be logged from inside the pump itself),
/// then spawns the USB/defmt drain loop on core1 with everything it needs to run forever.
pub fn spawn_usb_log_pump(
    core1: Core1Handles,
    title: &'static str,
    product: &'static str,
    usb: UsbParts,
    resets: pac::RESETS,
    watchdog: Watchdog,
    bbq: defmt_bbq::DefmtConsumer,
) {
    defmt::info!("=== {} ===", title);

    let mut mc = Multicore::new(core1.psm, core1.ppb, core1.fifo);
    let core1 = &mut mc.cores()[1];
    let stack_ptr = core::ptr::addr_of_mut!(CORE1_STACK);
    let stack: &'static mut [usize] = unsafe { &mut (*stack_ptr).mem };
    core1
        .spawn(stack, move || {
            run_usb_log_pump(product, usb, resets, watchdog, bbq)
        })
        .unwrap();
}

/// Runs forever on core1: brings USB CDC up, polls it, and drains `defmt-bbq` to serial whenever
/// the host has configured the device. Never calls `defmt::*!` itself — see the module docs for
/// why that's a hard requirement, not a style choice.
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
fn run_usb_log_pump(
    product: &'static str,
    usb: UsbParts,
    mut resets: pac::RESETS,
    watchdog: Watchdog,
    mut bbq: defmt_bbq::DefmtConsumer,
) -> ! {
    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        usb.regs,
        usb.dpram,
        usb.clock,
        true,
        &mut resets,
    ));

    let mut serial = SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Adafruit")
            .product(product)
            .serial_number("EPD")])
        .unwrap()
        .device_class(2) // CDC
        .build();

    loop {
        watchdog.feed();

        // Must be called as often as possible to keep the USB device alive.
        if usb_dev.poll(&mut [&mut serial]) {
            let mut rx = [0u8; 64];
            let _ = serial.read(&mut rx);
        }

        // Leave frames queued in defmt-bbq's ring buffer until the host has attached: draining
        // (and discarding) them early would throw away messages logged before enumeration
        // completes, such as the run's title banner logged just before core1 is spawned.
        if usb_dev.state() == UsbDeviceState::Configured {
            while let Ok(grant) = bbq.read() {
                if let Ok(written) = serial.write(&grant) {
                    grant.release(written);
                } else {
                    break;
                }
            }
        }
    }
}
