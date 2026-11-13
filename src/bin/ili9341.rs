//! ILI9341-Display (240×320, SPI) mit `mipidsi` und `embedded-graphics`.
//!
//!     cargo run --release --bin ili9341     # Pico 2 (RP2350)
//!     cargo run-rp2040 --bin ili9341        # Pico (RP2040)
//!
//! Der RP2040 (clk_peri 125 MHz) erreicht statt 20 MHz nur 15,6 MHz SPI-Takt.
//!
//! # Verdrahtung (Pico und Pico 2, SPI0, gleiche Pins)
//!
//! | Display   | Pico 2 | Pin am Pico |
//! |-----------|--------|-------------|
//! | VCC       | 3V3(OUT) | 36        |
//! | GND       | GND    | 23 (oder jeder GND) |
//! | CS        | GP17   | 22          |
//! | RESET     | GP21   | 27          |
//! | DC        | GP20   | 26          |
//! | SDI/MOSI  | GP19   | 25          |
//! | SCK       | GP18   | 24          |
//! | LED       | 3V3(OUT) | 36        |
//! | SDO/MISO  | GP16 (optional) | 21 |
//!
//! Sind Rot und Blau vertauscht, `ColorOrder::Bgr` durch `ColorOrder::Rgb` ersetzen.

#![no_std]
#![no_main]

use core::fmt::Write as _;

use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::spi::{self, Spi};
use embassy_rp235::{println, serial};
use embassy_time::{Delay, Duration, Instant, Ticker};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::{MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle, Triangle};
use embedded_graphics::text::Text;
use embedded_hal_bus::spi::ExclusiveDevice;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ILI9341Rgb565;
use mipidsi::options::{ColorOrder, Orientation, Rotation};
use mipidsi::Builder;
use panic_halt as _;

/// SPI-Takt. Das Datenblatt nennt 10 MHz für Schreibzugriffe; die meisten Module
/// laufen problemlos mit 20..40 MHz. Bei Bildfehlern verringern.
const SPI_HZ: u32 = 20_000_000;

/// Kleiner Text-Puffer für `write!` ohne Heap
struct Buf<const N: usize> {
    data: [u8; N],
    len: usize,
}

impl<const N: usize> Buf<N> {
    fn new() -> Self {
        Self { data: [0; N], len: 0 }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> core::fmt::Write for Buf<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(N - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    serial::init(spawner, p.USB);
    println!("ILI9341-Demo, clk_sys = {} Hz", embassy_rp::clocks::clk_sys_freq());

    // SPI-Bus + Chip-Select = SpiDevice
    let mut spi_config = spi::Config::default();
    spi_config.frequency = SPI_HZ;
    let spi = Spi::new_blocking(p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, spi_config);
    // Tatsächlicher Takt: clk_peri / CPSDVSR / (SCR + 1)
    let regs = embassy_rp::pac::SPI0;
    let spi_hz = embassy_rp::clocks::clk_peri_freq()
        / regs.cpsr().read().cpsdvsr() as u32
        / (regs.cr0().read().scr() as u32 + 1);
    println!("SPI-Takt {} Hz", spi_hz);
    let cs = Output::new(p.PIN_17, Level::High);
    let spi_dev = ExclusiveDevice::new(spi, cs, Delay).unwrap();

    // Display-Schnittstelle (DC-Pin + Sammelpuffer) und Treiber
    let dc = Output::new(p.PIN_20, Level::Low);
    let rst = Output::new(p.PIN_21, Level::High);
    let mut buffer = [0u8; 512];
    let di = SpiInterface::new(spi_dev, dc, &mut buffer);
    let mut display = Builder::new(ILI9341Rgb565, di)
        .reset_pin(rst)
        .color_order(ColorOrder::Bgr)
        .orientation(Orientation::new().rotate(Rotation::Deg90).flip_horizontal())
        .init(&mut Delay)
        .unwrap();
    println!("Display initialisiert");

    // Statisches Bild
    let t0 = Instant::now();
    display.clear(Rgb565::BLACK).unwrap();
    let t_clear = t0.elapsed().as_millis();

    let size = display.bounding_box().size;
    let title = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    Text::new("Hallo Pico 2 + Embassy!", Point::new(10, 25), title)
        .draw(&mut display)
        .unwrap();

    let colors = [
        Rgb565::RED,
        Rgb565::GREEN,
        Rgb565::BLUE,
        Rgb565::YELLOW,
        Rgb565::CYAN,
        Rgb565::MAGENTA,
    ];
    let w = size.width / colors.len() as u32;
    for (i, &c) in colors.iter().enumerate() {
        Rectangle::new(Point::new(i as i32 * w as i32, 40), Size::new(w, 30))
            .into_styled(PrimitiveStyle::with_fill(c))
            .draw(&mut display)
            .unwrap();
    }
    Circle::new(Point::new(30, 90), 80)
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::RED, 3))
        .draw(&mut display)
        .unwrap();
    Triangle::new(Point::new(150, 170), Point::new(210, 90), Point::new(270, 170))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::GREEN))
        .draw(&mut display)
        .unwrap();
    println!("Bild gezeichnet (clear: {} ms)", t_clear);

    // Laufende Anzeige: Zeit seit Start, jede Sekunde neu
    let small = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .background_color(Rgb565::BLACK)
        .build();
    let info = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_GRAY);
    let mut line = Buf::<48>::new();
    let _ = write!(line, "SPI {} kHz, clear {} ms", spi_hz / 1000, t_clear);
    Text::new(line.as_str(), Point::new(10, size.height as i32 - 8), info)
        .draw(&mut display)
        .unwrap();

    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        let s = Instant::now().as_secs();
        let mut line = Buf::<32>::new();
        let _ = write!(line, "Laufzeit {:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
        Text::new(line.as_str(), Point::new(10, 205), small)
            .draw(&mut display)
            .unwrap();
        ticker.next().await;
    }
}
