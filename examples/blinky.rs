//! Blinks the LED on an Adafruit RP2040 Feather ThinkInk board
//!
//! This will blink on-board LED.
#![no_std]
#![no_main]

use adafruit_feather_rp2040 as bsp;
use bsp::entry;
use bsp::{
    hal::{
        clocks::{init_clocks_and_plls, Clock},
        pac,
        watchdog::Watchdog,
        Sio,
    },
    Pins, XOSC_CRYSTAL_FREQ,
};
use defmt_rtt as _;
use embedded_hal::digital::OutputPin;
use panic_probe as _;

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

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    let sio = Sio::new(pac.SIO);
    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );
    let mut led_pin = pins.d13.into_push_pull_output();

    loop {
        led_pin.set_high().unwrap();
        delay.delay_ms(1500);
        led_pin.set_low().unwrap();
        delay.delay_ms(1500);
    }
}
