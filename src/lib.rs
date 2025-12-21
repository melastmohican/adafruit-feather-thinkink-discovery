//! Shared driver code for JD79661 e-paper displays.
#![no_std]

use embedded_graphics::prelude::*;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};
use embedded_hal::spi::SpiDevice;

/// JD79661 driver implementation
pub struct Jd79661<CS, BUSY, DC, RST> {
    cs: CS,
    busy: BUSY,
    dc: DC,
    _rst: RST,
}

impl<CS, BUSY, DC, RST> Jd79661<CS, BUSY, DC, RST>
where
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin + StatefulOutputPin,
{
    pub fn new<SPI, DELAY>(
        spi: &mut SPI,
        cs: CS,
        busy: BUSY,
        dc: DC,
        mut rst: RST,
        delay: &mut DELAY,
    ) -> Result<Self, SPI::Error>
    where
        SPI: SpiDevice,
        DELAY: DelayNs,
    {
        // Hardware reset
        let _ = rst.set_low();
        delay.delay_ms(10);
        let _ = rst.set_high();
        delay.delay_ms(10);

        let mut driver = Self {
            cs,
            busy,
            dc,
            _rst: rst,
        };

        driver.wait_busy(delay);
        driver.command(spi, 0x01, &[])?; // SWRESET
        driver.wait_busy(delay);

        // Magic key from Adafruit driver
        driver.command(spi, 0x4D, &[0x78])?;

        // Panel Setting (128x250 resolution)
        driver.command(spi, 0x00, &[0x8F, 0x29])?;

        // Power setting
        driver.command(spi, 0x01, &[0x07, 0x00])?;

        // Power offset
        driver.command(spi, 0x03, &[0x10, 0x54, 0x44])?;

        // Booster Soft Start
        driver.command(spi, 0x06, &[0x05, 0x00, 0x3F, 0x0A, 0x25, 0x12, 0x1A])?;

        // CDI
        driver.command(spi, 0x50, &[0x37])?;

        // TCON
        driver.command(spi, 0x60, &[0x02, 0x02, 0x02])?;

        // Resolution (128 x 250)
        driver.command(spi, 0x61, &[0x00, 0x80, 0x00, 0xFA])?;

        // Additional config registers from Adafruit
        driver.command(spi, 0xE7, &[0x1C])?;
        driver.command(spi, 0xE3, &[0x22])?;
        driver.command(spi, 0xB4, &[0xD0])?;
        driver.command(spi, 0xB5, &[0x03])?;
        driver.command(spi, 0xE9, &[0x01])?;
        driver.command(spi, 0x30, &[0x08])?;

        // Power ON
        driver.command(spi, 0x04, &[])?;
        driver.wait_busy(delay);

        Ok(driver)
    }

    fn command<SPI: SpiDevice>(
        &mut self,
        spi: &mut SPI,
        cmd: u8,
        data: &[u8],
    ) -> Result<(), SPI::Error> {
        let _ = self.dc.set_low();
        let _ = self.cs.set_low();
        spi.write(&[cmd])?;
        let _ = self.cs.set_high();

        if !data.is_empty() {
            let _ = self.dc.set_high();
            let _ = self.cs.set_low();
            spi.write(data)?;
            let _ = self.cs.set_high();
        }
        Ok(())
    }

    fn wait_busy<DELAY: DelayNs>(&mut self, delay: &mut DELAY) {
        // Based on adafruit_jd79661.py, busy_state=False
        // This means it is BUSY when LOW.
        while self.busy.is_low().unwrap_or(false) {
            delay.delay_ms(1);
        }
    }

    pub fn update_frames<SPI: SpiDevice>(
        &mut self,
        spi: &mut SPI,
        display: &DisplayBuffer,
    ) -> Result<(), SPI::Error> {
        // Send command to start transmission
        self.command(spi, 0x10, &[])?;

        let _ = self.dc.set_high();
        let _ = self.cs.set_low();

        // 128x250 RAM.
        for ly_as_rx in 0..250 {
            for lx_as_ry_block in (0..128).step_by(4) {
                let mut byte = 0u8;
                for i in 0..4 {
                    let rx = lx_as_ry_block + i;
                    let ry = ly_as_rx;

                    let color_bits = if ry < 250 && rx < 122 {
                        let x = ry;
                        let y = 121 - rx;

                        let idx = (y * WIDTH + x) / 8;
                        let bit = 7 - (x % 8);

                        let bw = (display.bw[idx] >> bit) & 1;
                        let red = (display.red[idx] >> bit) & 1;
                        let yellow = (display.yellow[idx] >> bit) & 1;

                        // Mapping corrected based on hardware observation:
                        // 00 -> Black
                        // 01 -> White
                        // 10 -> Yellow
                        // 11 -> Red
                        if red == 0 {
                            3 // Red (11)
                        } else if yellow == 0 {
                            2 // Yellow (10)
                        } else if bw == 0 {
                            0 // Black (00)
                        } else {
                            1 // White (01)
                        }
                    } else {
                        1 // Padding (Yellow?)
                    };
                    byte = (byte << 2) | color_bits;
                }
                spi.write(&[byte])?;
            }
        }
        let _ = self.cs.set_high();
        Ok(())
    }

    pub fn display_frame<SPI: SpiDevice, DELAY: DelayNs>(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.command(spi, 0x12, &[])?; // Display Refresh
        self.wait_busy(delay);
        Ok(())
    }
}

pub const WIDTH: usize = 250;
pub const HEIGHT: usize = 122;
pub const BUF_SIZE: usize = (WIDTH * HEIGHT).div_ceil(8);

pub struct DisplayBuffer {
    pub bw: [u8; BUF_SIZE],
    pub red: [u8; BUF_SIZE],
    pub yellow: [u8; BUF_SIZE],
}

impl DisplayBuffer {
    pub fn new() -> Self {
        Self {
            bw: [0xFF; BUF_SIZE],     // All white (inverted logic: 1=White, 0=Black)
            red: [0xFF; BUF_SIZE],    // All clear (1=Clear, 0=Red)
            yellow: [0xFF; BUF_SIZE], // All clear (1=Clear, 0=Yellow)
        }
    }

    pub fn clear(&mut self) {
        self.bw.fill(0xFF);
        self.red.fill(0xFF);
        self.yellow.fill(0xFF);
    }
}

impl Default for DisplayBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum QuadColor {
    Black,
    White,
    Red,
    Yellow,
}

impl PixelColor for QuadColor {
    type Raw = embedded_graphics::pixelcolor::raw::RawU2;
}

impl DrawTarget for DisplayBuffer {
    type Color = QuadColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels.into_iter() {
            if point.x >= 0 && point.x < WIDTH as i32 && point.y >= 0 && point.y < HEIGHT as i32 {
                let idx = (point.y as usize * WIDTH + point.x as usize) / 8;
                let bit = 7 - (point.x as usize % 8);

                // Clear all bits at this position first (set to 1 = White/Clear)
                self.bw[idx] |= 1 << bit;
                self.red[idx] |= 1 << bit;
                self.yellow[idx] |= 1 << bit;

                match color {
                    QuadColor::Black => self.bw[idx] &= !(1 << bit),
                    QuadColor::Red => self.red[idx] &= !(1 << bit),
                    QuadColor::Yellow => self.yellow[idx] &= !(1 << bit),
                    QuadColor::White => {}
                }
            }
        }
        Ok(())
    }
}

impl OriginDimensions for DisplayBuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}
