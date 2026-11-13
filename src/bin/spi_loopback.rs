//! SPI-Selbsttest ohne Verdrahtung: Die Loopback-Funktion des SPI-Blocks (PL022,
//! `SSPCR1.LBM`) führt MOSI intern auf MISO zurück. Ergebnis auf dem Serial Monitor.
//!
//!     cargo run --release --bin spi_loopback     # Pico 2 (RP2350)
//!     cargo run-rp2040 --bin spi_loopback        # Pico (RP2040)
//!
//! SPI0: SCK = GP18, MOSI = GP19, MISO = GP16 (dieselben Pins wie die ILI9341-Demo).

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::spi::{self, Phase, Polarity, Spi};
use embassy_rp235::{println, serial};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::spi::SpiBus;
use panic_halt as _;

/// Loopback einschalten (nach jedem `set_config`, das CR1 neu schreibt)
fn enable_loopback() {
    let r = embassy_rp::pac::SPI0;
    r.cr1().write(|w| {
        w.set_lbm(true);
        w.set_sse(true);
    });
}

/// Tatsächliche Taktfrequenz: clk_peri / CPSDVSR / (SCR + 1)
fn actual_frequency() -> u32 {
    let r = embassy_rp::pac::SPI0;
    let cpsdvsr = r.cpsr().read().cpsdvsr() as u32;
    let scr = r.cr0().read().scr() as u32;
    embassy_rp::clocks::clk_peri_freq() / cpsdvsr / (scr + 1)
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    serial::init(spawner, p.USB);

    // Zeit lassen, das Terminal zu öffnen (die Ausgabe wird aber auch gepuffert)
    Timer::after(Duration::from_secs(3)).await;
    println!();
    println!(
        "SPI-Selbsttest (interne Rückschleife), clk_peri = {} Hz",
        embassy_rp::clocks::clk_peri_freq()
    );

    let mut spi = Spi::new_blocking(p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, spi::Config::default());

    let modes = [
        (Polarity::IdleLow, Phase::CaptureOnFirstTransition),
        (Polarity::IdleLow, Phase::CaptureOnSecondTransition),
        (Polarity::IdleHigh, Phase::CaptureOnFirstTransition),
        (Polarity::IdleHigh, Phase::CaptureOnSecondTransition),
    ];
    for (n, &(polarity, phase)) in modes.iter().enumerate() {
        for &freq in &[1_000_000u32, 10_000_000, 20_000_000, 40_000_000] {
            let mut config = spi::Config::default();
            config.frequency = freq;
            config.polarity = polarity;
            config.phase = phase;
            spi.set_config(&config);
            enable_loopback();

            let mut tx = [0u8; 256];
            for (i, b) in tx.iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(37) ^ 0xA5;
            }
            let mut rx = [0u8; 256];
            SpiBus::transfer(&mut spi, &mut rx, &tx).unwrap();
            let ok_transfer = rx == tx;
            if !ok_transfer {
                println!("  gesendet  {:02x?}", &tx[..8]);
                println!("  empfangen {:02x?}", &rx[..8]);
            }

            let mut buf = tx;
            SpiBus::transfer_in_place(&mut spi, &mut buf).unwrap();
            let ok_in_place = buf == tx;

            // Durchsatz: 16 KiB schreiben
            let block = [0x55u8; 1024];
            let t0 = Instant::now();
            for _ in 0..16 {
                SpiBus::write(&mut spi, &block).unwrap();
            }
            SpiBus::flush(&mut spi).unwrap();
            let us = t0.elapsed().as_micros().max(1);
            let kib_s = 16 * 1024 * 1_000_000 / us / 1024;

            println!(
                "Modus {}, Soll {} Hz, Ist {} Hz: transfer {}, in_place {}, {} KiB/s",
                n,
                freq,
                actual_frequency(),
                if ok_transfer { "OK" } else { "FEHLER" },
                if ok_in_place { "OK" } else { "FEHLER" },
                kib_s
            );
        }
    }
    println!("Fertig.");
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
