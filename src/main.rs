//! LED blinkt alle 500 ms, jeder Wechsel wird auf dem Serial Monitor (USB) ausgegeben.
//!
//!     cargo run --release     # Pico 2 (RP2350)
//!     cargo run-rp2040        # Pico (RP2040)
//!
//! Gegenstück zu `embassy-xmc-test` aus dem XMC4700-Projekt.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp235::{println, serial};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Ticker};
use panic_halt as _;

/// Signal von led_task an counter_task (trägt den Zählerstand)
static BLINK_EVENT: Signal<CriticalSectionRawMutex, u32> = Signal::new();

/// Die Onboard-LED von Pico und Pico 2 hängt an GPIO 25. (Bei den W-Varianten sitzt sie am
/// WLAN-Chip und braucht den `cyw43`-Treiber.)
#[embassy_executor::task]
async fn led_task(mut led: Output<'static>) {
    // Ticker statt Timer::after: kein Drift, die Periode bleibt exakt
    let mut ticker = Ticker::every(Duration::from_millis(500));
    let mut n: u32 = 0;
    loop {
        led.toggle();
        n = n.wrapping_add(1);
        BLINK_EVENT.signal(n);
        ticker.next().await;
    }
}

/// Gibt jeden Blink-Zählerstand auf dem Serial Monitor aus.
#[embassy_executor::task]
async fn counter_task() {
    loop {
        let n = BLINK_EVENT.wait().await;
        println!("Blink {} bei {} ms", n, Instant::now().as_millis());
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Takt (RP2350: 150 MHz, RP2040: 125 MHz), Zeitgeber und Interrupts richtet embassy-rp ein.
    let p = embassy_rp::init(Default::default());
    serial::init(spawner, p.USB);
    println!(
        "embassy-rp235: {} gestartet, clk_sys = {} Hz, clk_peri = {} Hz",
        if cfg!(feature = "rp2040") { "RP2040" } else { "RP2350" },
        embassy_rp::clocks::clk_sys_freq(),
        embassy_rp::clocks::clk_peri_freq()
    );

    let led = Output::new(p.PIN_25, Level::Low);
    spawner.spawn(led_task(led).unwrap());
    spawner.spawn(counter_task().unwrap());
}
