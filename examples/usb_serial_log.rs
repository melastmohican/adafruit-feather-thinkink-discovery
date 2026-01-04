//! Simple USB Serial text logging example for Adafruit RP2040 Feather ThinkInk.
//!
//! This example provides a human-readable serial console via USB.
//! It uses custom macros (`info!`, `warn!`, `error!`) to send text
//! directly to the serial port, making it compatible with `screen`.
//!
//! # How to use:
//!
//! 1. Put the board in BOOTSEL mode (hold BOOT, press RESET).
//!
//! 2. Flash and run:
//!    `cargo run --example usb_serial_log`
//!
//! 3. Open Serial terminal (directly readable):
//!    `screen /dev/cu.usbmodem* 115200`
//!
//! Exiting screen: Press `Ctrl+A` then `K` then `Y`.

#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::entry;
use bsp::hal::{
    clocks::{init_clocks_and_plls, Clock},
    pac,
    usb::UsbBus,
    watchdog::Watchdog,
    Sio,
};
use bsp::{Pins, XOSC_CRYSTAL_FREQ};

use core::cell::RefCell;
use core::fmt::Write;
use critical_section::Mutex;
use defmt_rtt as _; // Link defmt-rtt so panic-probe can use it for crash reports
use panic_probe as _;

use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

/// Global USB Serial instance
static USB_SERIAL: Mutex<RefCell<Option<SerialPort<UsbBus>>>> = Mutex::new(RefCell::new(None));

/// Internal function to write to serial
pub fn _print(args: core::fmt::Arguments) {
    critical_section::with(|cs| {
        if let Some(serial) = USB_SERIAL.borrow(cs).borrow_mut().as_mut() {
            let mut buf = heapless::String::<256>::new();
            let _ = write!(buf, "{}", args);
            let _ = serial.write(buf.as_bytes());
        }
    });
}

/// Custom logging macros
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::_print(format_args!("[INFO] {}\r\n", format_args!($($arg)*)));
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::_print(format_args!("[WARN] {}\r\n", format_args!($($arg)*)));
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::_print(format_args!("[ERROR] {}\r\n", format_args!($($arg)*)));
    };
}

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();

    let mut watchdog = Watchdog::new(pac.WATCHDOG);

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

    let _delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    let sio = Sio::new(pac.SIO);

    // Extract peripherals before partial moves occur
    let mut resets = pac.RESETS;
    let usb_regs = pac.USBCTRL_REGS;
    let usb_dpram = pac.USBCTRL_DPRAM;

    let _pins = Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut resets);

    // Set up the USB driver
    // SAFETY: We use singletons to ensure these stay alive for the duration of the program.
    let usb_bus =
        cortex_m::singleton!(: UsbBusAllocator<UsbBus> = UsbBusAllocator::new(UsbBus::new(
            usb_regs,
            usb_dpram,
            clocks.usb_clock,
            true,
            &mut resets,
        )))
        .unwrap();

    // Set up the USB Communications Class Device
    let serial = SerialPort::new(usb_bus);

    // Create a USB device
    let mut usb_dev = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Adafruit")
            .product("Feather RP2040 Serial Log")
            .serial_number("LOG1")])
        .unwrap()
        .device_class(2)
        .build();

    // Move serial into the global static
    critical_section::with(|cs| {
        *USB_SERIAL.borrow(cs).borrow_mut() = Some(serial);
    });

    let mut timer = 0u32;
    let mut counter = 0u32;

    info!("Serial Console Initialized.");

    loop {
        // Feed the watchdog to prevent system resets
        watchdog.feed();

        // Must be called as often as possible to keep the USB device alive
        critical_section::with(|cs| {
            if let Some(serial) = USB_SERIAL.borrow(cs).borrow_mut().as_mut() {
                if usb_dev.poll(&mut [serial]) {
                    let mut buf = [0u8; 64];
                    if let Ok(count) = serial.read(&mut buf) {
                        if count > 0 {
                            // Echo back
                            let _ = serial.write(b"Echo: ");
                            let _ = serial.write(&buf[..count]);
                            let _ = serial.write(b"\r\n");
                        }
                    }
                }
            }
        });

        // Periodically log messages
        timer += 1;
        if timer >= 2_000_000 {
            timer = 0;
            counter += 1;

            info!("Simple log heartbeat #{}", counter);

            if counter % 5 == 0 {
                warn!("This is a warning at count {}", counter);
            }
        }
    }
}
