//! Gemeinsame Teile der Beispiele für den Raspberry Pi Pico 2 (RP2350) und den Pico (RP2040).
//!
//! Gegenstück zum Projekt `embassy-xmc-2`: Dort musste die HAL (GPIO, Zeitgeber, UART,
//! SPI) selbst geschrieben werden. Für den RP2350 liefert `embassy-rp` all das fertig,
//! hier liegt nur noch die Serial-Ausgabe über USB.

#![no_std]

#[cfg(all(feature = "rp2040", feature = "rp2350"))]
compile_error!("Nur ein Chip-Feature wählen: für den RP2040 `--no-default-features --features rp2040` (Alias `cargo run-rp2040`)");
#[cfg(not(any(feature = "rp2040", feature = "rp2350")))]
compile_error!("Chip-Feature fehlt: `rp2350` (Standard) oder `rp2040`");

pub mod serial;
