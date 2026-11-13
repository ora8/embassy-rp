//! Wählt je nach Chip-Feature das passende `memory.x` und die Linker-Skripte.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let rp2040 = env::var_os("CARGO_FEATURE_RP2040").is_some();
    let memory = if rp2040 { "memory-rp2040.x" } else { "memory-rp2350.x" };

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), fs::read(memory).unwrap()).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory-rp2040.x");
    println!("cargo:rerun-if-changed=memory-rp2350.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    if rp2040 {
        // Platziert den Second-Stage-Bootloader (.boot2) am Anfang des Flash
        println!("cargo:rustc-link-arg-bins=-Tlink-rp.x");
    }
}
