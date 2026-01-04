//! USB Serial CDC Defmt logging example for Adafruit RP2040 Feather ThinkInk.
//!
//! This example implements a `defmt` logger that sends binary frames over USB.
//!
//! # How to use:
//!
//! 1. Put the board in BOOTSEL mode (hold BOOT, press RESET).
//!
//! 2. Flash and run:
//!    `cargo run --example usb_serial_defmt`
//!
//! 3. View decoded logs in a SEPARATE terminal:
//!    `cat /dev/cu.usbmodem* | defmt-print -e target/thumbv6m-none-eabi/debug/examples/usb_serial_defmt`
//!
//! Note: `screen` will only show binary "garbage". Use `defmt-print` for text messages.

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

use defmt_bbq as _;
use panic_probe as _;

use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

#[entry]
fn main() -> ! {
    // Initialize defmt-bbq as the global logger
    let mut bbq = defmt_bbq::init().unwrap();
    defmt::info!("Program start. Logs are redirected to USB Serial.");

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
    let _pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Set up the USB driver
    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    // Set up the USB Communications Class Device
    let mut serial = SerialPort::new(&usb_bus);

    // Create a USB device with a unique PID for identification
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Adafruit")
            .product("Feather RP2040 Defmt")
            .serial_number("LOG1")])
        .unwrap()
        .device_class(2) // CDC class
        .build();

    let mut timer = 0u32;
    let mut counter = 0u32;

    loop {
        // Feed the watchdog to prevent system resets
        watchdog.feed();

        // Must be called as often as possible to keep the USB device alive
        if usb_dev.poll(&mut [&mut serial]) {
            let mut buf = [0u8; 64];
            // Read input to keep the buffers clear
            let _ = serial.read(&mut buf);
        }

        // Drain binary defmt-bbq logs to the USB serial port
        while let Ok(grant) = bbq.read() {
            if usb_dev.state() == UsbDeviceState::Configured {
                if let Ok(written) = serial.write(&grant) {
                    grant.release(written);
                } else {
                    break;
                }
            } else {
                // If not configured, we release to avoid buffer overflow
                let len = grant.len();
                grant.release(len);
            }
        }

        // Periodically log messages via defmt
        timer += 1;
        if timer >= 2_000_000 {
            timer = 0;
            counter += 1;

            // These messages will be binary-encoded and decoded by defmt-print
            defmt::info!("Heartbeat from Feather RP2040! Counter: {}", counter);

            if counter % 5 == 0 {
                defmt::warn!("This is a warning at count {}", counter);
            }
        }
    }
}
